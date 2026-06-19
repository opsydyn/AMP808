use std::{cell::RefCell, io, rc::Rc};

use amp808_core::web_audio::{
    analyser_bands_to_heights, analyser_bins_to_bands, HostedAudioIssue, WebAudioSource,
};
use ratzilla::backend::webgl2::WebGl2BackendOptions;
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span, Text},
    widgets::{
        canvas::{Canvas, Line as CanvasLine},
        Block, Borders, Paragraph,
    },
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
struct ClassicColor {
    red: u8,
    green: u8,
    blue: u8,
}

impl ClassicColor {
    const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    const fn ratatui(self) -> Color {
        Color::Rgb(self.red, self.green, self.blue)
    }
}

struct Classic808Palette;

impl Classic808Palette {
    const FACEPLATE: ClassicColor = ClassicColor::new(0x09, 0x0a, 0x08);
    const IVORY: ClassicColor = ClassicColor::new(0xee, 0xea, 0xdc);
    const ORANGE: ClassicColor = ClassicColor::new(0xf0, 0x5a, 0x28);
    const AMBER: ClassicColor = ClassicColor::new(0xf6, 0xa6, 0x23);
    const YELLOW: ClassicColor = ClassicColor::new(0xff, 0xd4, 0x00);
    const RED: ClassicColor = ClassicColor::new(0xd7, 0x26, 0x2e);
    const RED_TEXT: ClassicColor = ClassicColor::new(0xff, 0x5a, 0x45);
    const GREY: ClassicColor = ClassicColor::new(0xc9, 0xc9, 0xc9);
    const DIM: ClassicColor = ClassicColor::new(0x66, 0x66, 0x66);
    const LABEL: ClassicColor = ClassicColor::new(0xa7, 0xaa, 0x7a);
    const OLIVE: ClassicColor = ClassicColor::new(0x48, 0x4b, 0x30);
}

#[cfg(test)]
fn contrast_ratio(foreground: ClassicColor, background: ClassicColor) -> f64 {
    let foreground = relative_luminance(foreground);
    let background = relative_luminance(background);
    let lighter = foreground.max(background);
    let darker = foreground.min(background);
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
fn relative_luminance(color: ClassicColor) -> f64 {
    fn channel(value: u8) -> f64 {
        let value = f64::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassicPadFamily {
    Red,
    Orange,
    Yellow,
    Ivory,
}

fn classic_pad_family(step_index: usize) -> ClassicPadFamily {
    match step_index / 4 {
        0 => ClassicPadFamily::Red,
        1 => ClassicPadFamily::Orange,
        2 => ClassicPadFamily::Yellow,
        _ => ClassicPadFamily::Ivory,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InstrumentControlSpec {
    short_label: &'static str,
    instrument_label: &'static str,
    parameter_label: &'static str,
    family: ClassicPadFamily,
}

fn instrument_control_specs() -> &'static [InstrumentControlSpec; 12] {
    &[
        InstrumentControlSpec {
            short_label: "AC",
            instrument_label: "ACCENT",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "BD",
            instrument_label: "BASS",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "SD",
            instrument_label: "SNARE",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "LT",
            instrument_label: "LOW TOM",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Red,
        },
        InstrumentControlSpec {
            short_label: "MT",
            instrument_label: "MID TOM",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "HT",
            instrument_label: "HI TOM",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "CL",
            instrument_label: "CLAVES",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "RS",
            instrument_label: "RIM",
            parameter_label: "LEVEL",
            family: ClassicPadFamily::Orange,
        },
        InstrumentControlSpec {
            short_label: "CP",
            instrument_label: "CLAP",
            parameter_label: "SNAP",
            family: ClassicPadFamily::Yellow,
        },
        InstrumentControlSpec {
            short_label: "CB",
            instrument_label: "COWBELL",
            parameter_label: "TUNE",
            family: ClassicPadFamily::Yellow,
        },
        InstrumentControlSpec {
            short_label: "CY",
            instrument_label: "CYMBAL",
            parameter_label: "DECAY",
            family: ClassicPadFamily::Yellow,
        },
        InstrumentControlSpec {
            short_label: "OH",
            instrument_label: "OPEN HAT",
            parameter_label: "DECAY",
            family: ClassicPadFamily::Ivory,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelRole {
    Transport,
    Instrument,
    Analyser,
    Steps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelState {
    Idle,
    Armed,
    Active,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WebPanelSpec {
    title: &'static str,
    state: PanelState,
    lamp: Option<PanelState>,
}

fn web_panel_spec(role: PanelRole, state: &WebAppState) -> WebPanelSpec {
    let panel_state = match (role, state.transport) {
        (_, TransportState::Error) => PanelState::Error,
        (PanelRole::Transport, TransportState::Playing) => PanelState::Active,
        (PanelRole::Transport, TransportState::Ready | TransportState::Paused) => PanelState::Armed,
        (PanelRole::Analyser | PanelRole::Steps, TransportState::Playing) => PanelState::Active,
        (
            PanelRole::Analyser | PanelRole::Steps,
            TransportState::Ready | TransportState::Paused,
        ) => PanelState::Armed,
        (PanelRole::Instrument, TransportState::Idle | TransportState::Ended) => PanelState::Idle,
        (PanelRole::Instrument, _) => PanelState::Armed,
        _ => PanelState::Idle,
    };

    WebPanelSpec {
        title: match role {
            PanelRole::Transport => " BASIC RHYTHM ",
            PanelRole::Instrument => " INSTRUMENT SELECT / LEVEL ",
            PanelRole::Analyser => " SCOPE / ANALYSER ",
            PanelRole::Steps => " BASIC RHYTHM STEP BUTTONS ",
        },
        state: panel_state,
        lamp: matches!(
            role,
            PanelRole::Transport | PanelRole::Analyser | PanelRole::Steps
        )
        .then_some(panel_state),
    }
}

fn analyser_empty_state_text(state: &WebAppState) -> Option<&'static str> {
    match state.transport {
        TransportState::Idle | TransportState::Ended => Some("LOAD AUDIO OR CORS URL"),
        TransportState::Ready => Some("READY - PRESS PLAY"),
        TransportState::Paused => Some("PAUSED"),
        TransportState::Error => Some("CHECK SOURCE / CORS"),
        TransportState::Playing => None,
    }
}

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
        .title(" AMP808 WEB ")
        .title_style(
            Style::default()
                .fg(Classic808Palette::ORANGE.ratatui())
                .add_modifier(Modifier::BOLD),
        )
        .style(classic_faceplate_style())
        .borders(Borders::ALL)
        .border_style(classic_line_style());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(14),
            Constraint::Length(2),
        ])
        .split(inner);

    render_machine_header(frame, rows[0], state);

    if rows[1].width < 90 {
        let deck_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(12),
                Constraint::Length(7),
                Constraint::Min(7),
                Constraint::Length(5),
            ])
            .split(rows[1]);
        render_left_control_panel(frame, deck_rows[0], state);
        render_knob_bank(frame, deck_rows[1], state);
        render_visualizer(frame, deck_rows[2], state);
        render_step_strip(frame, deck_rows[3], state);
    } else {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(24)])
            .split(rows[1]);
        render_left_control_panel(frame, body[0], state);

        let deck_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Min(7),
                Constraint::Length(5),
            ])
            .split(body[1]);
        render_knob_bank(frame, deck_rows[0], state);
        render_visualizer(frame, deck_rows[1], state);
        render_step_strip(frame, deck_rows[2], state);
    }

    let compact_status = "Load audio or CORS URL";
    let footer_text = match state.error.as_deref() {
        Some(error) => error,
        None if rows[2].width < 70 => compact_status,
        None => &state.status,
    };
    let footer_color = if state.error.is_some() {
        Classic808Palette::RED_TEXT.ratatui()
    } else {
        Classic808Palette::IVORY.ratatui()
    };
    let footer = Paragraph::new(Text::from(Line::from(Span::styled(
        footer_text,
        Style::default().fg(footer_color),
    ))))
    .alignment(Alignment::Center);
    frame.render_widget(footer, rows[2]);
}

fn render_machine_header(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let source_label = state
        .source
        .as_ref()
        .map(WebAudioSource::label)
        .unwrap_or("No audio loaded");

    let lines = vec![
        Line::from(vec![
            Span::styled(
                "AMP808 ",
                Style::default()
                    .fg(Classic808Palette::ORANGE.ratatui())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Rhythm Composer ",
                Style::default().fg(Classic808Palette::AMBER.ratatui()),
            ),
            Span::styled(
                "TR-808 WEB",
                Style::default()
                    .fg(Classic808Palette::ORANGE.ratatui())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "SOURCE ",
                Style::default().fg(Classic808Palette::YELLOW.ratatui()),
            ),
            Span::styled(
                source_label,
                Style::default().fg(Classic808Palette::IVORY.ratatui()),
            ),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        area,
    );
}

fn render_808_panel(frame: &mut Frame<'_>, area: Rect, spec: WebPanelSpec) -> Rect {
    let block = Block::default()
        .title(panel_title(spec))
        .style(classic_faceplate_style())
        .borders(Borders::ALL)
        .border_style(panel_border_style(spec.state));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

fn panel_title(spec: WebPanelSpec) -> Line<'static> {
    let mut spans = vec![Span::styled(spec.title, classic_label_style())];
    if let Some(lamp) = spec.lamp {
        spans.push(Span::styled(panel_lamp_label(lamp), panel_lamp_style(lamp)));
    }
    Line::from(spans)
}

fn panel_lamp_label(state: PanelState) -> &'static str {
    match state {
        PanelState::Idle => "[ ]",
        PanelState::Armed => "[R]",
        PanelState::Active => "[*]",
        PanelState::Error => "[!]",
    }
}

fn panel_lamp_style(state: PanelState) -> Style {
    Style::default()
        .fg(match state {
            PanelState::Idle => Classic808Palette::DIM.ratatui(),
            PanelState::Armed => Classic808Palette::AMBER.ratatui(),
            PanelState::Active => Classic808Palette::YELLOW.ratatui(),
            PanelState::Error => Classic808Palette::RED_TEXT.ratatui(),
        })
        .add_modifier(if matches!(state, PanelState::Active | PanelState::Error) {
            Modifier::BOLD
        } else {
            Modifier::empty()
        })
}

fn panel_border_style(state: PanelState) -> Style {
    Style::default().fg(match state {
        PanelState::Idle => Classic808Palette::ORANGE.ratatui(),
        PanelState::Armed => Classic808Palette::AMBER.ratatui(),
        PanelState::Active => Classic808Palette::YELLOW.ratatui(),
        PanelState::Error => Classic808Palette::RED_TEXT.ratatui(),
    })
}

fn render_left_control_panel(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let inner = render_808_panel(frame, area, web_panel_spec(PanelRole::Transport, state));

    if inner.width < 18 || inner.height < 8 {
        render_left_control_fallback(frame, inner, state);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(inner);

    let status_lines = vec![
        Line::from(vec![
            Span::styled("MODE     ", classic_small_label_style()),
            Span::styled("WEB AUDIO", classic_value_style()),
        ]),
        Line::from(vec![
            Span::styled("STATE    ", classic_small_label_style()),
            Span::styled(
                state.transport.label(),
                Style::default()
                    .fg(transport_color(state.transport))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("TIME     ", classic_small_label_style()),
            Span::styled(
                format_time_status(state.current_time, state.duration),
                classic_value_style(),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(status_lines)), rows[0]);

    render_tempo_dial(frame, rows[1], 0.5, "120");

    let control_lines = vec![
        Line::from(vec![
            Span::styled("MASTER VOL ", classic_small_label_style()),
            Span::styled("O", classic_knob_style()),
        ]),
        Line::from(vec![
            Span::styled("PATTERN A/B ", classic_small_label_style()),
            Span::styled("A ", active_lamp_style()),
            Span::styled("B", inactive_lamp_style()),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(control_lines)), rows[2]);
}

fn render_left_control_fallback(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let lines = vec![
        Line::from(vec![
            Span::styled("STATE ", classic_small_label_style()),
            Span::styled(
                state.transport.label(),
                Style::default().fg(transport_color(state.transport)),
            ),
        ]),
        Line::from(vec![
            Span::styled("TIME  ", classic_small_label_style()),
            Span::styled(
                format_time_status(state.current_time, state.duration),
                classic_value_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("TEMPO ", classic_small_label_style()),
            Span::styled("120 BPM", classic_value_style()),
        ]),
        Line::from(vec![
            Span::styled("PATTERN ", classic_small_label_style()),
            Span::styled("A ", active_lamp_style()),
            Span::styled("B", inactive_lamp_style()),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_tempo_dial(frame: &mut Frame<'_>, area: Rect, bpm_norm: f64, bpm_label: &str) {
    if area.width < 8 || area.height < 4 {
        let fallback = Line::from(vec![
            Span::styled("TEMPO ", classic_small_label_style()),
            Span::styled(format!("{bpm_label} BPM"), classic_value_style()),
        ]);
        frame.render_widget(Paragraph::new(fallback).alignment(Alignment::Center), area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(1)])
        .split(area);
    let (canvas_area, label_area) = (chunks[0], chunks[1]);
    let geometry = tempo_dial_geometry_808(canvas_area);
    let start_angle = 210.0_f64.to_radians();
    let end_angle = (-30.0_f64).to_radians();
    let sweep = start_angle - end_angle;
    let val_angle = start_angle - bpm_norm.clamp(0.0, 1.0) * sweep;
    let bpm_label = bpm_label.to_string();

    let canvas = Canvas::default()
        .x_bounds(geometry.x_bounds)
        .y_bounds(geometry.y_bounds)
        .marker(Marker::Braille)
        .paint(move |ctx| {
            for i in 0..60usize {
                let t1 = i as f64 / 60.0;
                let t2 = (i + 1) as f64 / 60.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: geometry.radius * a1.cos(),
                    y1: geometry.radius * a1.sin(),
                    x2: geometry.radius * a2.cos(),
                    y2: geometry.radius * a2.sin(),
                    color: Classic808Palette::IVORY.ratatui(),
                });
            }

            let active_segs = (bpm_norm.clamp(0.0, 1.0) * 60.0) as usize;
            for i in 0..active_segs {
                let t1 = i as f64 / 60.0;
                let t2 = (i + 1) as f64 / 60.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: geometry.radius * a1.cos(),
                    y1: geometry.radius * a1.sin(),
                    x2: geometry.radius * a2.cos(),
                    y2: geometry.radius * a2.sin(),
                    color: dial_arc_color((t1 + t2) / 2.0),
                });
            }

            ctx.draw(&CanvasLine {
                x1: 0.0,
                y1: 0.0,
                x2: geometry.radius * 0.72 * val_angle.cos(),
                y2: geometry.radius * 0.72 * val_angle.sin(),
                color: Classic808Palette::AMBER.ratatui(),
            });

            for i in 0u32..=10 {
                let t = i as f64 / 10.0;
                let angle = start_angle - t * sweep;
                ctx.draw(&CanvasLine {
                    x1: (geometry.radius + 0.35) * angle.cos(),
                    y1: (geometry.radius + 0.35) * angle.sin(),
                    x2: (geometry.radius + 0.9) * angle.cos(),
                    y2: (geometry.radius + 0.9) * angle.sin(),
                    color: Classic808Palette::DIM.ratatui(),
                });
            }

            ctx.print(
                -1.4,
                1.1,
                Span::styled(
                    "BPM",
                    Style::default().fg(Classic808Palette::IVORY.ratatui()),
                ),
            );
            ctx.print(
                -1.7,
                -0.5,
                Span::styled(
                    bpm_label.clone(),
                    Style::default()
                        .fg(Classic808Palette::AMBER.ratatui())
                        .add_modifier(Modifier::BOLD),
                ),
            );
        });

    frame.render_widget(canvas, canvas_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "TEMPO",
            classic_small_label_style(),
        )))
        .alignment(Alignment::Center),
        label_area,
    );
}

#[derive(Clone, Copy, Debug)]
struct TempoDialGeometry808 {
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    radius: f64,
}

fn tempo_dial_geometry_808(area: Rect) -> TempoDialGeometry808 {
    const Y_HALF: f64 = 9.0;
    const LABEL_PAD: f64 = 1.5;
    let visual_ratio = area.width as f64 / (area.height.max(1) as f64 * 2.0);
    let x_half = (Y_HALF * visual_ratio).max(4.8);
    let radius = (x_half.min(Y_HALF) - LABEL_PAD - 0.2).clamp(2.2, 6.2);
    TempoDialGeometry808 {
        x_bounds: [-x_half, x_half],
        y_bounds: [-Y_HALF, Y_HALF],
        radius,
    }
}

fn dial_arc_color(position: f64) -> Color {
    if position > 0.72 {
        Classic808Palette::RED.ratatui()
    } else if position > 0.36 {
        Classic808Palette::ORANGE.ratatui()
    } else {
        Classic808Palette::AMBER.ratatui()
    }
}

fn render_knob_bank(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let inner = render_808_panel(frame, area, web_panel_spec(PanelRole::Instrument, state));
    let specs = instrument_control_specs();
    let visible = (usize::from(inner.width) / 6).clamp(1, specs.len());
    let specs = &specs[..visible];

    if inner.height < 4 {
        let mut knob_row = Vec::with_capacity(specs.len());
        let mut label_row = Vec::with_capacity(specs.len());
        for spec in specs {
            knob_row.push(Span::styled(" (@) ", classic_knob_style()));
            label_row.push(instrument_short_span(spec, 5));
        }
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(knob_row),
                Line::from(label_row),
            ]))
            .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    let knob_cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, visible as u32); visible])
        .split(rows[0]);

    for (index, (cell, spec)) in knob_cells.iter().zip(specs.iter()).enumerate() {
        render_canvas_knob(frame, *cell, web_knob_value(index), spec, index == 1);
    }

    frame.render_widget(
        Paragraph::new(Line::from(instrument_parameter_spans(specs))).alignment(Alignment::Center),
        rows[1],
    );
}

fn render_canvas_knob(
    frame: &mut Frame<'_>,
    area: Rect,
    value: f64,
    spec: &InstrumentControlSpec,
    selected: bool,
) {
    if area.width < 5 || area.height < 3 {
        let fallback = vec![
            Line::from(Span::styled("(@)", classic_knob_style())),
            Line::from(instrument_short_span(spec, usize::from(area.width.max(1)))),
        ];
        frame.render_widget(
            Paragraph::new(Text::from(fallback)).alignment(Alignment::Center),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(1)])
        .split(area);
    let canvas_area = chunks[0];
    let label_area = chunks[1];
    let (x_bounds, y_bounds) = knob_canvas_bounds_808(canvas_area);
    let value = value.clamp(0.0, 1.0);
    let start_angle = 210.0_f64.to_radians();
    let end_angle = (-30.0_f64).to_radians();
    let sweep = start_angle - end_angle;
    let val_angle = start_angle - value * sweep;
    let radius = 3.5;
    let accent = if selected {
        Classic808Palette::YELLOW.ratatui()
    } else {
        Classic808Palette::GREY.ratatui()
    };

    let canvas = Canvas::default()
        .x_bounds(x_bounds)
        .y_bounds(y_bounds)
        .marker(Marker::Braille)
        .paint(move |ctx| {
            for i in 0..24usize {
                let t1 = i as f64 / 24.0;
                let t2 = (i + 1) as f64 / 24.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: radius * a1.cos(),
                    y1: radius * a1.sin(),
                    x2: radius * a2.cos(),
                    y2: radius * a2.sin(),
                    color: Classic808Palette::IVORY.ratatui(),
                });
            }

            let active_steps = (value * 24.0) as usize;
            for i in 0..active_steps {
                let t1 = i as f64 / 24.0;
                let t2 = (i + 1) as f64 / 24.0;
                let a1 = start_angle - t1 * sweep;
                let a2 = start_angle - t2 * sweep;
                ctx.draw(&CanvasLine {
                    x1: radius * a1.cos(),
                    y1: radius * a1.sin(),
                    x2: radius * a2.cos(),
                    y2: radius * a2.sin(),
                    color: dial_arc_color((t1 + t2) / 2.0),
                });
            }

            ctx.draw(&CanvasLine {
                x1: 0.0,
                y1: 0.0,
                x2: radius * 0.68 * val_angle.cos(),
                y2: radius * 0.68 * val_angle.sin(),
                color: accent,
            });
        });

    frame.render_widget(canvas, canvas_area);
    let label_style = if selected {
        instrument_strip_style(spec.family)
            .fg(Classic808Palette::YELLOW.ratatui())
            .add_modifier(Modifier::BOLD)
    } else {
        instrument_strip_style(spec.family)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(spec.short_label, label_style)))
            .alignment(Alignment::Center),
        label_area,
    );
}

fn instrument_parameter_spans(specs: &[InstrumentControlSpec]) -> Vec<Span<'static>> {
    specs
        .iter()
        .map(|spec| instrument_parameter_span(spec, 6))
        .collect()
}

fn instrument_short_span(spec: &InstrumentControlSpec, width: usize) -> Span<'static> {
    Span::styled(
        format!("{:^width$}", spec.short_label, width = width.max(2)),
        instrument_strip_style(spec.family).add_modifier(Modifier::BOLD),
    )
}

fn instrument_parameter_span(spec: &InstrumentControlSpec, width: usize) -> Span<'static> {
    let label = if width <= 5 {
        abbreviate_parameter_label(spec.parameter_label)
    } else {
        spec.parameter_label
    };
    Span::styled(
        format!("{label:^width$}", width = width.max(4)),
        instrument_parameter_style(spec.family),
    )
}

fn abbreviate_parameter_label(label: &'static str) -> &'static str {
    match label {
        "LEVEL" => "LVL",
        "TUNE" => "TUN",
        "DECAY" => "DEC",
        "SNAP" => "SNP",
        other => other,
    }
}

fn knob_canvas_bounds_808(area: Rect) -> ([f64; 2], [f64; 2]) {
    const Y_HALF: f64 = 4.0;
    let visual_ratio = area.width as f64 / (area.height.max(1) as f64 * 2.0);
    let x_half = (Y_HALF * visual_ratio).max(3.8);
    ([-x_half, x_half], [-Y_HALF, Y_HALF])
}

fn web_knob_value(index: usize) -> f64 {
    const VALUES: [f64; 12] = [
        0.72, 0.62, 0.55, 0.48, 0.58, 0.66, 0.35, 0.44, 0.7, 0.5, 0.77, 0.68,
    ];
    VALUES[index % VALUES.len()]
}

fn render_visualizer(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let inner = render_808_panel(frame, area, web_panel_spec(PanelRole::Analyser, state));
    let bands = &state.bands;

    if inner.height == 0 || inner.width < 2 || bands.is_empty() {
        return;
    }

    if let Some(message) = analyser_empty_state_text(state) {
        render_analyser_empty_state(frame, inner, message, state.transport);
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

fn render_analyser_empty_state(
    frame: &mut Frame<'_>,
    area: Rect,
    message: &'static str,
    transport: TransportState,
) {
    let spacer_count = area.height.saturating_sub(3) / 2;
    let mut lines = Vec::with_capacity(usize::from(spacer_count) + 3);
    for _ in 0..spacer_count {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        message,
        Style::default()
            .fg(transport_color(transport))
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "WEB AUDIO ANALYSER",
        classic_small_label_style(),
    )));
    lines.push(Line::from(Span::styled(
        "REAL BANDS ONLY",
        classic_value_style(),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        area,
    );
}

fn render_step_strip(frame: &mut Frame<'_>, area: Rect, state: &WebAppState) {
    let inner = render_808_panel(frame, area, web_panel_spec(PanelRole::Steps, state));
    let bands = &state.bands;

    if inner.width < 32 {
        return;
    }

    let step_count = (usize::from(inner.width) / 4).clamp(1, 16);
    let mut numbers = Vec::with_capacity(step_count);
    let mut pads = Vec::with_capacity(step_count);
    for step in 0..step_count {
        numbers.push(Span::styled(
            format!("{:^4}", step + 1),
            classic_small_label_style(),
        ));
        let energy = bands.get(step).copied().unwrap_or_default();
        let active = energy > 0.08;
        pads.push(Span::styled(
            " ## ",
            Style::default()
                .fg(classic_pad_color(classic_pad_family(step), active))
                .add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
    }

    let lines = vec![
        Line::from(numbers),
        Line::from(pads),
        Line::from(vec![
            Span::styled("START / STOP  ", classic_small_label_style()),
            Span::styled("TAP  ", classic_pad_style(ClassicPadFamily::Ivory)),
            Span::styled("A", active_lamp_style()),
            Span::styled(" B", inactive_lamp_style()),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        inner,
    );
}

fn transport_color(transport: TransportState) -> Color {
    match transport {
        TransportState::Playing => Classic808Palette::YELLOW.ratatui(),
        TransportState::Ready | TransportState::Paused => Classic808Palette::AMBER.ratatui(),
        TransportState::Error => Classic808Palette::RED_TEXT.ratatui(),
        TransportState::Idle | TransportState::Ended => Classic808Palette::GREY.ratatui(),
    }
}

fn spectrum_style(row_ratio: f64) -> Style {
    let color = if row_ratio > 0.66 {
        Classic808Palette::RED.ratatui()
    } else if row_ratio > 0.33 {
        Classic808Palette::AMBER.ratatui()
    } else {
        Classic808Palette::YELLOW.ratatui()
    };
    Style::default().fg(color)
}

fn classic_faceplate_style() -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .bg(Classic808Palette::FACEPLATE.ratatui())
}

fn classic_line_style() -> Style {
    Style::default().fg(Classic808Palette::ORANGE.ratatui())
}

fn classic_label_style() -> Style {
    Style::default()
        .fg(Classic808Palette::YELLOW.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn classic_small_label_style() -> Style {
    Style::default().fg(Classic808Palette::LABEL.ratatui())
}

fn classic_value_style() -> Style {
    Style::default().fg(Classic808Palette::IVORY.ratatui())
}

fn classic_knob_style() -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn instrument_strip_style(family: ClassicPadFamily) -> Style {
    Style::default()
        .fg(Classic808Palette::IVORY.ratatui())
        .bg(instrument_family_bg(family))
}

fn instrument_parameter_style(family: ClassicPadFamily) -> Style {
    Style::default()
        .fg(instrument_family_fg(family))
        .add_modifier(Modifier::BOLD)
}

fn instrument_family_fg(family: ClassicPadFamily) -> Color {
    match family {
        ClassicPadFamily::Red => Classic808Palette::RED_TEXT.ratatui(),
        ClassicPadFamily::Orange => Classic808Palette::ORANGE.ratatui(),
        ClassicPadFamily::Yellow => Classic808Palette::YELLOW.ratatui(),
        ClassicPadFamily::Ivory => Classic808Palette::IVORY.ratatui(),
    }
}

fn instrument_family_bg(family: ClassicPadFamily) -> Color {
    match family {
        ClassicPadFamily::Red => Color::Rgb(0x4c, 0x15, 0x12),
        ClassicPadFamily::Orange => Color::Rgb(0x4a, 0x2a, 0x0e),
        ClassicPadFamily::Yellow => Color::Rgb(0x4b, 0x42, 0x10),
        ClassicPadFamily::Ivory => Classic808Palette::OLIVE.ratatui(),
    }
}

fn active_lamp_style() -> Style {
    Style::default()
        .fg(Classic808Palette::RED_TEXT.ratatui())
        .add_modifier(Modifier::BOLD)
}

fn inactive_lamp_style() -> Style {
    Style::default().fg(Classic808Palette::DIM.ratatui())
}

fn classic_pad_style(family: ClassicPadFamily) -> Style {
    Style::default()
        .fg(classic_pad_color(family, true))
        .add_modifier(Modifier::BOLD)
}

fn classic_pad_color(family: ClassicPadFamily, active: bool) -> Color {
    match (family, active) {
        (ClassicPadFamily::Red, true) => Classic808Palette::RED.ratatui(),
        (ClassicPadFamily::Orange, true) => Classic808Palette::ORANGE.ratatui(),
        (ClassicPadFamily::Yellow, true) => Classic808Palette::YELLOW.ratatui(),
        (ClassicPadFamily::Ivory, true) => Classic808Palette::IVORY.ratatui(),
        (ClassicPadFamily::Red, false) => Color::Rgb(0xa4, 0x21, 0x1a),
        (ClassicPadFamily::Orange, false) => Color::Rgb(0xb8, 0x56, 0x19),
        (ClassicPadFamily::Yellow, false) => Color::Rgb(0xa4, 0xaa, 0x24),
        (ClassicPadFamily::Ivory, false) => Color::Rgb(0xb4, 0xae, 0x92),
    }
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

#[cfg(test)]
mod tests {
    use super::{
        analyser_empty_state_text, classic_pad_family, contrast_ratio, instrument_control_specs,
        instrument_family_bg, instrument_family_fg, web_panel_spec, Classic808Palette,
        ClassicColor, ClassicPadFamily, PanelRole, PanelState, TransportState, WebAppState,
    };
    use ratzilla::ratatui::style::Color;

    #[test]
    fn classic_pad_family_matches_tr_808_step_groups() {
        let families = (0..16).map(classic_pad_family).collect::<Vec<_>>();

        assert_eq!(
            families,
            vec![
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Ivory,
                ClassicPadFamily::Ivory,
                ClassicPadFamily::Ivory,
                ClassicPadFamily::Ivory,
            ]
        );
    }

    #[test]
    fn classic_palette_keeps_normal_text_at_aa_contrast() {
        let faceplate = Classic808Palette::FACEPLATE;
        let normal_text = [
            ("ivory", Classic808Palette::IVORY),
            ("orange", Classic808Palette::ORANGE),
            ("amber", Classic808Palette::AMBER),
            ("yellow", Classic808Palette::YELLOW),
            ("grey", Classic808Palette::GREY),
            ("label", Classic808Palette::LABEL),
        ];

        for (name, color) in normal_text {
            assert!(
                contrast_ratio(color, faceplate) >= 4.5,
                "{name} should pass AA contrast on the 808 faceplate"
            );
        }

        assert!(
            contrast_ratio(Classic808Palette::RED, faceplate) < 4.5,
            "hardware red should stay reserved for lamps/buttons, not normal text"
        );
    }

    #[test]
    fn classic_palette_keeps_controls_and_borders_at_aa_contrast() {
        let faceplate = Classic808Palette::FACEPLATE;
        let pairs = [
            ("orange border", Classic808Palette::ORANGE, faceplate),
            ("error text", Classic808Palette::RED_TEXT, faceplate),
            ("orange button text", faceplate, Classic808Palette::ORANGE),
            ("yellow button text", faceplate, Classic808Palette::YELLOW),
            ("ivory button text", faceplate, Classic808Palette::IVORY),
        ];

        for (name, foreground, background) in pairs {
            assert!(
                contrast_ratio(foreground, background) >= 4.5,
                "{name} should pass AA contrast"
            );
        }
    }

    #[test]
    fn instrument_family_colors_keep_strip_labels_readable() {
        for family in [
            ClassicPadFamily::Red,
            ClassicPadFamily::Orange,
            ClassicPadFamily::Yellow,
            ClassicPadFamily::Ivory,
        ] {
            assert!(
                contrast_ratio(
                    Classic808Palette::IVORY,
                    color_to_classic(instrument_family_bg(family))
                ) >= 4.5,
                "{family:?} strip label should pass AA contrast"
            );
            assert!(
                contrast_ratio(
                    color_to_classic(instrument_family_fg(family)),
                    Classic808Palette::FACEPLATE
                ) >= 4.5,
                "{family:?} parameter label should pass AA contrast"
            );
        }
    }

    fn color_to_classic(color: Color) -> ClassicColor {
        match color {
            Color::Rgb(red, green, blue) => ClassicColor { red, green, blue },
            other => panic!("expected RGB color, got {other:?}"),
        }
    }

    #[test]
    fn web_panel_specs_make_transport_state_visible() {
        let mut state = WebAppState {
            transport: TransportState::Playing,
            ..WebAppState::default()
        };

        assert_eq!(
            web_panel_spec(PanelRole::Transport, &state).state,
            PanelState::Active
        );
        assert_eq!(
            web_panel_spec(PanelRole::Analyser, &state).state,
            PanelState::Active
        );
        assert_eq!(
            web_panel_spec(PanelRole::Instrument, &state).state,
            PanelState::Armed
        );

        state.transport = TransportState::Error;
        state.error = Some("CORS blocked".to_string());

        assert_eq!(
            web_panel_spec(PanelRole::Transport, &state).state,
            PanelState::Error
        );
        assert_eq!(
            web_panel_spec(PanelRole::Analyser, &state).state,
            PanelState::Error
        );
    }

    #[test]
    fn analyser_empty_state_text_tracks_browser_audio_state() {
        let mut state = WebAppState::default();
        assert_eq!(
            analyser_empty_state_text(&state),
            Some("LOAD AUDIO OR CORS URL")
        );

        state.transport = TransportState::Ready;
        assert_eq!(
            analyser_empty_state_text(&state),
            Some("READY - PRESS PLAY")
        );

        state.transport = TransportState::Paused;
        assert_eq!(analyser_empty_state_text(&state), Some("PAUSED"));

        state.transport = TransportState::Error;
        state.error = Some("CORS blocked".to_string());
        assert_eq!(
            analyser_empty_state_text(&state),
            Some("CHECK SOURCE / CORS")
        );

        state.transport = TransportState::Playing;
        state.error = None;
        assert_eq!(analyser_empty_state_text(&state), None);
    }

    #[test]
    fn instrument_control_specs_match_808_web_strip() {
        let specs = instrument_control_specs();

        assert_eq!(specs.len(), 12);
        assert_eq!(specs[0].short_label, "AC");
        assert_eq!(specs[0].parameter_label, "LEVEL");
        assert_eq!(specs[1].short_label, "BD");
        assert_eq!(specs[1].instrument_label, "BASS");
        assert_eq!(specs[2].instrument_label, "SNARE");
        assert_eq!(specs[9].short_label, "CB");
        assert_eq!(specs[9].parameter_label, "TUNE");
        assert_eq!(specs[10].short_label, "CY");
        assert_eq!(specs[10].parameter_label, "DECAY");
        assert_eq!(specs[11].short_label, "OH");
        assert_eq!(specs[11].parameter_label, "DECAY");

        let families = specs
            .iter()
            .map(|spec| spec.family)
            .collect::<Vec<ClassicPadFamily>>();
        assert_eq!(
            families,
            vec![
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Red,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Orange,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Yellow,
                ClassicPadFamily::Ivory,
            ]
        );
    }
}
