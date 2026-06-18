use std::{cell::RefCell, io, rc::Rc};

use amp808_core::web_audio::{
    analyser_bands_to_heights, analyser_bins_to_bands, HostedAudioIssue, WebAudioSource,
};
use ratzilla::backend::webgl2::WebGl2BackendOptions;
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use ratzilla::{WebGl2Backend, WebRenderer};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    window, AnalyserNode, AudioContext, Document, Event, EventTarget, HtmlAudioElement,
    HtmlButtonElement, HtmlElement, HtmlInputElement, MediaElementAudioSourceNode, Url,
};

const BAND_COUNT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransportState {
    Idle,
    Ready,
    Playing,
    Paused,
    Ended,
    Error,
}

impl TransportState {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Ready => "READY",
            Self::Playing => "PLAYING",
            Self::Paused => "PAUSED",
            Self::Ended => "ENDED",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone)]
struct WebAppState {
    source: Option<WebAudioSource>,
    transport: TransportState,
    status: String,
    error: Option<String>,
    current_time: f64,
    duration: Option<f64>,
    bands: Vec<f32>,
}

impl Default for WebAppState {
    fn default() -> Self {
        Self {
            source: None,
            transport: TransportState::Idle,
            status: "Load a local audio file or a CORS-enabled hosted URL".to_string(),
            error: None,
            current_time: 0.0,
            duration: None,
            bands: vec![0.0; BAND_COUNT],
        }
    }
}

struct AudioGraph {
    audio: HtmlAudioElement,
    context: AudioContext,
    _source: MediaElementAudioSourceNode,
    analyser: AnalyserNode,
}

fn main() -> io::Result<()> {
    let backend = WebGl2Backend::new_with_options(WebGl2BackendOptions::new().grid_id("app"))?;
    let terminal = Terminal::new(backend)?;
    let state = Rc::new(RefCell::new(WebAppState::default()));
    let graph = install_audio_graph(Rc::clone(&state)).map_err(js_to_io_error)?;

    terminal.draw_web(move |frame| {
        sample_analyser(&graph, &state);
        let snapshot = state.borrow().clone();
        render_web_808(frame, &snapshot);
    });

    Ok(())
}

fn install_audio_graph(state: Rc<RefCell<WebAppState>>) -> Result<Rc<AudioGraph>, JsValue> {
    let document = window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("document is not available"))?;
    let audio = HtmlAudioElement::new()?;
    audio.set_preload("metadata");

    let context = AudioContext::new()?;
    let source = context.create_media_element_source(&audio)?;
    let analyser = context.create_analyser()?;
    analyser.set_fft_size(1024);
    analyser.set_smoothing_time_constant(0.78);
    source.connect_with_audio_node(&analyser)?;
    analyser.connect_with_audio_node(&context.destination())?;

    let graph = Rc::new(AudioGraph {
        audio,
        context,
        _source: source,
        analyser,
    });

    wire_controls(&document, Rc::clone(&graph), state)?;
    Ok(graph)
}

fn wire_controls(
    document: &Document,
    graph: Rc<AudioGraph>,
    state: Rc<RefCell<WebAppState>>,
) -> Result<(), JsValue> {
    let file_input: HtmlInputElement = element_by_id(document, "amp808-file")?;
    let url_input: HtmlInputElement = element_by_id(document, "amp808-url")?;
    let toggle_button: HtmlButtonElement = element_by_id(document, "amp808-toggle")?;
    let load_url_button: HtmlButtonElement = element_by_id(document, "amp808-load-url")?;
    let control_status: HtmlElement = element_by_id(document, "amp808-control-status")?;
    let object_url: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        let object_url = Rc::clone(&object_url);
        let file_input = file_input.clone();
        add_event_listener(file_input.clone().as_ref(), "change", move |_| {
            let Some(files) = file_input.files() else {
                return;
            };
            let Some(file) = files.get(0) else {
                return;
            };

            revoke_object_url(&object_url);
            match Url::create_object_url_with_blob(&file) {
                Ok(url) => {
                    graph.audio.set_cross_origin(None);
                    graph.audio.set_src(&url);
                    graph.audio.load();
                    *object_url.borrow_mut() = Some(url);

                    {
                        let mut state = state.borrow_mut();
                        state.source = Some(WebAudioSource::local_file(file.name()));
                        state.transport = TransportState::Ready;
                        state.status = "Local audio loaded".to_string();
                        state.error = None;
                        state.current_time = 0.0;
                        state.duration = None;
                        state.bands = vec![0.0; BAND_COUNT];
                    }
                    sync_controls(&toggle_button, &control_status, &state.borrow());
                }
                Err(error) => set_error(
                    &state,
                    &toggle_button,
                    &control_status,
                    format!("Could not create browser object URL: {error:?}"),
                ),
            }
        })?;
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        let object_url = Rc::clone(&object_url);
        let url_input = url_input.clone();
        add_event_listener(load_url_button.as_ref(), "click", move |_| {
            let url = url_input.value().trim().to_string();
            if url.is_empty() {
                set_error(
                    &state,
                    &toggle_button,
                    &control_status,
                    "Enter a hosted audio URL first.".to_string(),
                );
                return;
            }

            revoke_object_url(&object_url);
            graph.audio.set_cross_origin(Some("anonymous"));
            graph.audio.set_src(&url);
            graph.audio.load();

            {
                let mut state = state.borrow_mut();
                state.source = Some(WebAudioSource::hosted_url(url));
                state.transport = TransportState::Ready;
                state.status =
                    "Hosted audio loaded; CORS must allow AMP808 web playback.".to_string();
                state.error = None;
                state.current_time = 0.0;
                state.duration = None;
                state.bands = vec![0.0; BAND_COUNT];
            }
            sync_controls(&toggle_button, &control_status, &state.borrow());
        })?;
    }

    {
        let graph = Rc::clone(&graph);
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(toggle_button.clone().as_ref(), "click", move |_| {
            if graph.audio.paused() {
                let resume = graph.context.resume().ok();
                match graph.audio.play() {
                    Ok(play) => {
                        let state = Rc::clone(&state);
                        let toggle_button = toggle_button.clone();
                        let control_status = control_status.clone();
                        spawn_local(async move {
                            if let Some(resume) = resume {
                                let _ = JsFuture::from(resume).await;
                            }
                            if let Err(error) = JsFuture::from(play).await {
                                set_error(
                                    &state,
                                    &toggle_button,
                                    &control_status,
                                    format!("Browser refused playback: {error:?}"),
                                );
                            }
                        });
                    }
                    Err(error) => set_error(
                        &state,
                        &toggle_button,
                        &control_status,
                        format!("Browser refused playback: {error:?}"),
                    ),
                }
            } else if let Err(error) = graph.audio.pause() {
                set_error(
                    &state,
                    &toggle_button,
                    &control_status,
                    format!("Could not pause playback: {error:?}"),
                );
            }
        })?;
    }

    wire_audio_events(&graph.audio, state, toggle_button, control_status)?;
    Ok(())
}

fn wire_audio_events(
    audio: &HtmlAudioElement,
    state: Rc<RefCell<WebAppState>>,
    toggle_button: HtmlButtonElement,
    control_status: HtmlElement,
) -> Result<(), JsValue> {
    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.clone().as_ref(), "loadedmetadata", move |_| {
            let mut state = state.borrow_mut();
            state.duration = finite_duration(&audio);
            state.transport = TransportState::Ready;
            state.status = "Audio metadata loaded".to_string();
            state.error = None;
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        add_event_listener(audio.clone().as_ref(), "timeupdate", move |_| {
            let mut state = state.borrow_mut();
            state.current_time = audio.current_time();
            state.duration = finite_duration(&audio);
        })?;
    }

    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.clone().as_ref(), "play", move |_| {
            let mut state = state.borrow_mut();
            state.current_time = audio.current_time();
            state.duration = finite_duration(&audio);
            state.transport = TransportState::Playing;
            state.status = "Playback running from browser audio".to_string();
            state.error = None;
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let audio = audio.clone();
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.clone().as_ref(), "pause", move |_| {
            let mut state = state.borrow_mut();
            if !audio.ended() && state.transport != TransportState::Error {
                state.transport = TransportState::Paused;
                state.status = "Playback paused".to_string();
            }
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.as_ref(), "ended", move |_| {
            let mut state = state.borrow_mut();
            state.transport = TransportState::Ended;
            state.status = "Playback ended".to_string();
            state.bands = vec![0.0; BAND_COUNT];
            sync_controls(&toggle_button, &control_status, &state);
        })?;
    }

    {
        let state = Rc::clone(&state);
        let toggle_button = toggle_button.clone();
        let control_status = control_status.clone();
        add_event_listener(audio.as_ref(), "error", move |_| {
            let source = state.borrow().source.clone();
            let message = if source.as_ref().is_some_and(WebAudioSource::is_hosted_url) {
                HostedAudioIssue::CorsRequired.user_message().to_string()
            } else {
                "Browser could not load this audio file.".to_string()
            };
            set_error(&state, &toggle_button, &control_status, message);
        })?;
    }

    Ok(())
}

fn sample_analyser(graph: &AudioGraph, state: &Rc<RefCell<WebAppState>>) {
    if state.borrow().transport != TransportState::Playing {
        return;
    }

    let mut bins = vec![0; graph.analyser.frequency_bin_count() as usize];
    graph.analyser.get_byte_frequency_data(&mut bins);
    let bands = analyser_bins_to_bands(&bins, BAND_COUNT);
    state.borrow_mut().bands = bands;
}

fn render_web_808(frame: &mut Frame<'_>, state: &WebAppState) {
    let area = frame.area();
    let block = Block::default()
        .title("AMP808 WEB 808")
        .title_style(
            Style::default()
                .fg(Color::Rgb(0xff, 0x7a, 0x45))
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(0xf6, 0xa6, 0x23)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(inner);

    let source_label = state
        .source
        .as_ref()
        .map(WebAudioSource::label)
        .unwrap_or("No audio loaded");
    let source = Paragraph::new(Text::from(Line::from(vec![
        Span::styled("Source: ", Style::default().fg(Color::Gray)),
        Span::styled(source_label, Style::default().fg(Color::White)),
    ])))
    .alignment(Alignment::Center);
    frame.render_widget(source, rows[0]);

    let transport = Paragraph::new(Text::from(Line::from(vec![
        Span::styled(
            format!("{} ", state.transport.label()),
            Style::default()
                .fg(transport_color(state.transport))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format_time_status(state.current_time, state.duration),
            Style::default().fg(Color::Rgb(0xc9, 0xc9, 0xc9)),
        ),
    ])))
    .alignment(Alignment::Center);
    frame.render_widget(transport, rows[1]);

    render_visualizer(frame, rows[2], &state.bands);

    let footer_text = state.error.as_deref().unwrap_or(&state.status);
    let footer_color = if state.error.is_some() {
        Color::Red
    } else {
        Color::Rgb(0xc9, 0xc9, 0xc9)
    };
    let footer = Paragraph::new(Text::from(Line::from(Span::styled(
        footer_text,
        Style::default().fg(footer_color),
    ))))
    .alignment(Alignment::Center);
    frame.render_widget(footer, rows[3]);
}

fn render_visualizer(frame: &mut Frame<'_>, area: Rect, bands: &[f32]) {
    let block = Block::default()
        .title(" REAL ANALYSER ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(0xff, 0x7a, 0x45)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width < 2 || bands.is_empty() {
        return;
    }

    let visible_bands = (usize::from(inner.width) / 2).clamp(1, bands.len());
    let bands = &bands[..visible_bands];
    let heights = analyser_bands_to_heights(bands, inner.height);
    let mut lines = Vec::with_capacity(usize::from(inner.height));

    for row in (1..=inner.height).rev() {
        let row_ratio = f64::from(row) / f64::from(inner.height.max(1));
        let style = spectrum_style(row_ratio);
        let mut spans = Vec::with_capacity(heights.len());
        for height in &heights {
            let cell = if *height >= row { "##" } else { "  " };
            spans.push(Span::styled(cell, style));
        }
        lines.push(Line::from(spans));
    }

    let visualizer = Paragraph::new(Text::from(lines)).alignment(Alignment::Center);
    frame.render_widget(visualizer, inner);
}

fn transport_color(transport: TransportState) -> Color {
    match transport {
        TransportState::Playing => Color::Rgb(0xf1, 0xf8, 0x27),
        TransportState::Ready | TransportState::Paused => Color::Rgb(0xf8, 0xa1, 0x25),
        TransportState::Error => Color::Red,
        TransportState::Idle | TransportState::Ended => Color::Gray,
    }
}

fn spectrum_style(row_ratio: f64) -> Style {
    let color = if row_ratio > 0.66 {
        Color::Rgb(0xe7, 0x2e, 0x2e)
    } else if row_ratio > 0.33 {
        Color::Rgb(0xf8, 0xa1, 0x25)
    } else {
        Color::Rgb(0xf1, 0xf8, 0x27)
    };
    Style::default().fg(color)
}

fn format_time_status(current_time: f64, duration: Option<f64>) -> String {
    match duration {
        Some(duration) => format!(
            "{} / {}",
            format_seconds(current_time),
            format_seconds(duration)
        ),
        None => format!("{} / --:--", format_seconds(current_time)),
    }
}

fn format_seconds(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--".to_string();
    }

    let total_seconds = seconds.round() as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn finite_duration(audio: &HtmlAudioElement) -> Option<f64> {
    let duration = audio.duration();
    duration.is_finite().then_some(duration)
}

fn sync_controls(button: &HtmlButtonElement, status: &HtmlElement, state: &WebAppState) {
    button.set_disabled(state.source.is_none());
    let label = if state.transport == TransportState::Playing {
        "Pause"
    } else {
        "Play"
    };
    button.set_text_content(Some(label));
    status.set_text_content(Some(state.error.as_deref().unwrap_or(&state.status)));
}

fn set_error(
    state: &Rc<RefCell<WebAppState>>,
    button: &HtmlButtonElement,
    status: &HtmlElement,
    message: String,
) {
    let mut state = state.borrow_mut();
    state.transport = TransportState::Error;
    state.error = Some(message);
    state.bands = vec![0.0; BAND_COUNT];
    sync_controls(button, status, &state);
}

fn revoke_object_url(object_url: &Rc<RefCell<Option<String>>>) {
    if let Some(url) = object_url.borrow_mut().take() {
        let _ = Url::revoke_object_url(&url);
    }
}

fn element_by_id<T>(document: &Document, id: &str) -> Result<T, JsValue>
where
    T: JsCast,
{
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("missing #{id}")))?
        .dyn_into()
        .map_err(|_| JsValue::from_str(&format!("invalid element type for #{id}")))
}

fn add_event_listener(
    target: &EventTarget,
    name: &'static str,
    handler: impl FnMut(Event) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::<dyn FnMut(Event)>::wrap(Box::new(handler));
    target.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

fn js_to_io_error(error: JsValue) -> io::Error {
    io::Error::other(format!("{error:?}"))
}
