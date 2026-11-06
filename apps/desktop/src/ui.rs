use gpui_kit::base::{Button as BaseButton, Disableable, StyledExt};
use gpui_kit::component::button::Button;
use gpui_kit::{
    App, Context, FocusHandle, Focusable, IntoElement, KeyBinding, PathPromptOptions, Render, Role,
    TestSupportExt, Window, canvas as gpui_canvas, div, prelude::*, px, rgb,
};
use std::time::{Duration, Instant};

use crate::app::DesktopApp;
use crate::app::transport::TransportState;
use crate::milkdrop::{MilkdropStatus, MilkdropViewHost};
use crate::playback::{DecodeWorkerState, PlaybackSession, PlaybackSessionError};
use crate::skin::Amp808Skin;

gpui_kit::actions!(
    amp808,
    [
        OpenFile,
        TogglePlayback,
        SeekBack,
        SeekForward,
        VolumeDown,
        VolumeUp,
        ToggleVisualizer,
    ]
);

pub fn init_keyboard_shortcuts(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-o", OpenFile, Some("Amp808Player")),
        KeyBinding::new(
            "space",
            TogglePlayback,
            Some("Amp808Player && !Amp808Control"),
        ),
        KeyBinding::new("left", SeekBack, Some("Amp808Player")),
        KeyBinding::new("right", SeekForward, Some("Amp808Player")),
        KeyBinding::new("down", VolumeDown, Some("Amp808Player")),
        KeyBinding::new("up", VolumeUp, Some("Amp808Player")),
        KeyBinding::new("v", ToggleVisualizer, Some("Amp808Player")),
    ]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisualizerMode {
    Spectrum,
    Waveform,
    MilkDrop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackToggle {
    Pause,
    Play,
    Restart,
    Ignore,
}

fn playback_toggle(state: &TransportState) -> PlaybackToggle {
    match state {
        TransportState::Playing => PlaybackToggle::Pause,
        TransportState::Paused | TransportState::Ready => PlaybackToggle::Play,
        TransportState::Ended => PlaybackToggle::Restart,
        TransportState::Empty
        | TransportState::Loading
        | TransportState::Buffering { .. }
        | TransportState::Failed(_) => PlaybackToggle::Ignore,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrackDisplay {
    title: String,
    artist: String,
    album: String,
}

impl TrackDisplay {
    fn new(title: &str, artist: Option<&str>, album: Option<&str>) -> Self {
        Self {
            title: title.to_owned(),
            artist: artist.unwrap_or("UNKNOWN ARTIST").to_owned(),
            album: album.unwrap_or("UNKNOWN ALBUM").to_owned(),
        }
    }

    fn empty() -> Self {
        Self {
            title: "NO TRACK LOADED".to_owned(),
            artist: "--".to_owned(),
            album: "--".to_owned(),
        }
    }

    fn accessibility_label(&self) -> String {
        if self.title == "NO TRACK LOADED" {
            format!(
                "No track loaded. Artist: {}. Album: {}.",
                self.artist, self.album
            )
        } else {
            format!(
                "Now playing: {}. Artist: {}. Album: {}.",
                self.title, self.artist, self.album
            )
        }
    }

    #[cfg(test)]
    fn title(&self) -> &str {
        &self.title
    }

    #[cfg(test)]
    fn artist(&self) -> &str {
        &self.artist
    }

    #[cfg(test)]
    fn album(&self) -> &str {
        &self.album
    }
}

pub struct PlayerView {
    app: DesktopApp,
    focus: FocusHandle,
    skin: Amp808Skin,
    session: Option<PlaybackSession>,
    milkdrop: MilkdropViewHost,
    error: Option<String>,
    seeking: bool,
    visualizer_mode: VisualizerMode,
    performance_probe: PerformanceProbe,
}

impl PlayerView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            app: DesktopApp::new(),
            focus: cx.focus_handle(),
            skin: Amp808Skin::built_in(),
            session: None,
            milkdrop: MilkdropViewHost::new(),
            error: None,
            seeking: false,
            visualizer_mode: VisualizerMode::Spectrum,
            performance_probe: PerformanceProbe::from_env(),
        }
    }

    fn color(&self, value: crate::skin::SkinColor) -> gpui_kit::Rgba {
        rgb(value.hex())
    }

    fn install_session(&mut self, mut session: PlaybackSession) {
        self.milkdrop.replace_pcm(session.take_milkdrop_pcm());
        self.session = Some(session);
    }

    fn clear_session(&mut self) {
        self.session = None;
        self.milkdrop.replace_pcm(None);
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open in AMP808-R3TR0".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match selection.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |view, cx| {
                        view.error = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            let result = PlaybackSession::open(path);
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(session) => {
                        view.install_session(session);
                        view.error = None;
                        view.seeking = false;
                    }
                    Err(error) => {
                        view.clear_session();
                        view.error = Some(error.to_string());
                        view.seeking = false;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.seeking {
            return;
        }
        let Some(action) = self
            .session
            .as_ref()
            .map(|session| playback_toggle(session.transport_state()))
        else {
            return;
        };
        match action {
            PlaybackToggle::Restart => {
                let position = self
                    .session
                    .as_ref()
                    .map_or(0.0, PlaybackSession::position_seconds);
                self.seek_by(-position, cx);
            }
            PlaybackToggle::Pause | PlaybackToggle::Play => {
                let session = self
                    .session
                    .as_mut()
                    .expect("playback action requires an active session");
                let result = match action {
                    PlaybackToggle::Pause => session.pause(),
                    PlaybackToggle::Play => session.play(),
                    PlaybackToggle::Restart | PlaybackToggle::Ignore => unreachable!(
                        "restart and ignore playback actions are handled before session mutation"
                    ),
                };
                if let Err(error) = result {
                    self.error = Some(error.to_string());
                }
                cx.notify();
            }
            PlaybackToggle::Ignore => {}
        }
    }

    fn adjust_volume(&mut self, delta: f32, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        session.set_volume(session.volume() + delta);
        cx.notify();
    }

    fn seek_by(&mut self, delta_seconds: f64, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let duration = session.metadata().duration_seconds().unwrap_or(f64::MAX);
        let target = (session.position_seconds() + delta_seconds).clamp(0.0, duration);
        let path = session.path().to_path_buf();
        let resume_paused = session.transport_state() == &TransportState::Paused;
        self.seeking = true;
        self.error = None;
        cx.notify();

        let rebuild = cx.background_executor().spawn(async move {
            let mut session = PlaybackSession::open_at(path, target)?;
            if resume_paused {
                session.pause()?;
            }
            Ok::<PlaybackSession, PlaybackSessionError>(session)
        });
        cx.spawn(async move |this, cx| {
            let result = rebuild.await;
            let _ = this.update(cx, |view, cx| {
                view.seeking = false;
                match result {
                    Ok(session) => view.install_session(session),
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_visualizer(&mut self, cx: &mut Context<Self>) {
        self.visualizer_mode = match self.visualizer_mode {
            VisualizerMode::Spectrum => VisualizerMode::Waveform,
            VisualizerMode::Waveform => VisualizerMode::MilkDrop,
            VisualizerMode::MilkDrop => VisualizerMode::Spectrum,
        };
        cx.notify();
    }
}

impl Focusable for PlayerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn format_time(seconds: f64) -> String {
    let total_seconds = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

fn signal_status(
    seeking: bool,
    transport: Option<&TransportState>,
    decode: Option<&DecodeWorkerState>,
) -> &'static str {
    if seeking {
        return "PCM / BUFFERING";
    }
    if transport == Some(&TransportState::Ended) {
        return "PCM / ENDED";
    }
    match decode {
        None => "PCM / IDLE",
        Some(DecodeWorkerState::Starting) => "PCM / BUFFERING",
        Some(DecodeWorkerState::Running) => "PCM / LIVE",
        Some(DecodeWorkerState::Ended) => "PCM / DRAINING",
        Some(DecodeWorkerState::Failed(_)) => "PCM / ERROR",
        Some(DecodeWorkerState::Stopped) => "PCM / STOPPED",
    }
}

fn milkdrop_status_label(status: &MilkdropStatus, is_playing: bool) -> String {
    match status {
        MilkdropStatus::Unavailable => "MilkDrop unavailable",
        MilkdropStatus::Initializing => "MilkDrop initializing",
        MilkdropStatus::Running if is_playing => "MilkDrop active",
        MilkdropStatus::Running => "MilkDrop paused",
        MilkdropStatus::Failed(error) => return format!("MilkDrop error: {error}"),
    }
    .into()
}

const PERFORMANCE_REPORT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq)]
struct PerformanceSample {
    elapsed: Duration,
    rendered_frames: u64,
    average_fps: f64,
    underflow_frames: u64,
}

#[derive(Debug)]
struct PerformanceProbe {
    enabled: bool,
    report_interval: Duration,
    baseline: Option<(Instant, u64, u64)>,
    last_report_at: Option<Instant>,
}

impl PerformanceProbe {
    fn from_env() -> Self {
        Self {
            enabled: std::env::var("AMP808_MILKDROP_METRICS").is_ok_and(|value| value == "1"),
            report_interval: PERFORMANCE_REPORT_INTERVAL,
            baseline: None,
            last_report_at: None,
        }
    }

    #[cfg(test)]
    const fn enabled(report_interval: Duration) -> Self {
        Self {
            enabled: true,
            report_interval,
            baseline: None,
            last_report_at: None,
        }
    }

    fn observe(
        &mut self,
        now: Instant,
        active: bool,
        rendered_frames: u64,
        underflow_frames: u64,
    ) -> Option<PerformanceSample> {
        if !self.enabled {
            return None;
        }
        if !active {
            self.baseline = None;
            self.last_report_at = None;
            return None;
        }

        let (started_at, initial_frames, initial_underflows) =
            *self.baseline.get_or_insert_with(|| {
                self.last_report_at = Some(now);
                (now, rendered_frames, underflow_frames)
            });
        let last_report_at = self.last_report_at.unwrap_or(started_at);
        if now.saturating_duration_since(last_report_at) < self.report_interval {
            return None;
        }

        self.last_report_at = Some(now);
        let elapsed = now.saturating_duration_since(started_at);
        let completed_frames = rendered_frames.saturating_sub(initial_frames);
        Some(PerformanceSample {
            elapsed,
            rendered_frames: completed_frames,
            average_fps: completed_frames as f64 / elapsed.as_secs_f64(),
            underflow_frames: underflow_frames.saturating_sub(initial_underflows),
        })
    }
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = self.color(self.skin.canvas());
        let panel = self.color(self.skin.panel());
        let panel_raised = self.color(self.skin.panel_raised());
        let text = self.color(self.skin.text());
        let text_muted = self.color(self.skin.text_muted());
        let accent = self.color(self.skin.accent());
        let meter_low = self.color(self.skin.meter_low());
        let meter_high = self.color(self.skin.meter_high());
        let reduce_motion = cx.reduce_motion();
        let motion_label = if reduce_motion {
            "Reduced motion enabled; essential audio levels remain active"
        } else {
            "Standard motion enabled; essential audio levels remain active"
        };
        let status_opacity = if reduce_motion { 0.3 } else { 0.45 };
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.poll()
        {
            self.error = Some(error.to_string());
        }
        let snapshot = self
            .session
            .as_ref()
            .and_then(PlaybackSession::try_snapshot);
        if let Some(snapshot) = &snapshot {
            if let Some(error) = snapshot.output_error() {
                self.error = Some(error.to_owned());
            } else if let DecodeWorkerState::Failed(error) = snapshot.decode() {
                self.error = Some(error.clone());
            }
        }
        let has_session = self.session.is_some();
        let is_playing = self
            .session
            .as_ref()
            .is_some_and(|session| session.transport_state() == &TransportState::Playing);
        let milkdrop_active = self.visualizer_mode == VisualizerMode::MilkDrop
            && is_playing
            && matches!(self.milkdrop.status(), MilkdropStatus::Running);
        let underflow_frames = self
            .session
            .as_ref()
            .map_or(0, PlaybackSession::underflow_frames);
        if let Some(sample) = self.performance_probe.observe(
            Instant::now(),
            milkdrop_active,
            self.milkdrop.rendered_frames(),
            underflow_frames,
        ) {
            eprintln!(
                "AMP808_MILKDROP_METRICS elapsed_s={:.3} rendered_frames={} average_fps={:.3} underflow_frames={}",
                sample.elapsed.as_secs_f64(),
                sample.rendered_frames,
                sample.average_fps,
                sample.underflow_frames
            );
        }
        if self.visualizer_mode != VisualizerMode::MilkDrop {
            self.milkdrop.hide();
        }
        let (visualizer_label, visualizer_action_label) = match self.visualizer_mode {
            VisualizerMode::Spectrum => ("SPECTRUM", "Show waveform visualizer"),
            VisualizerMode::Waveform => ("WAVEFORM", "Show MilkDrop visualizer"),
            VisualizerMode::MilkDrop => ("MILKDROP", "Show spectrum visualizer"),
        };
        let visualizer = match self.visualizer_mode {
            VisualizerMode::Spectrum => {
                let spectrum = snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.analysis().spectrum().to_vec())
                    .unwrap_or_else(|| vec![0.0; 24]);
                div()
                    .flex()
                    .flex_1()
                    .items_end()
                    .gap_1()
                    .rounded_md()
                    .bg(canvas)
                    .p_4()
                    .children(spectrum.into_iter().map(|magnitude| {
                        div()
                            .flex_1()
                            .h(px(3.0 + magnitude * 220.0))
                            .rounded_sm()
                            .bg(meter_low.opacity(0.18 + magnitude * 0.82))
                    }))
                    .into_any_element()
            }
            VisualizerMode::Waveform => {
                let waveform = snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.analysis().waveform().to_vec())
                    .unwrap_or_else(|| vec![0.0; 128]);
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .rounded_md()
                    .bg(canvas)
                    .p_4()
                    .children(waveform.into_iter().map(|sample| {
                        let positive = sample.max(0.0);
                        let negative = (-sample).max(0.0);
                        let opacity = 0.18 + sample.abs() * 0.82;
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .child(
                                div().flex().flex_1().items_end().child(
                                    div()
                                        .w_full()
                                        .h(px(1.0 + positive * 100.0))
                                        .bg(meter_high.opacity(opacity)),
                                ),
                            )
                            .child(
                                div().flex().flex_1().items_start().child(
                                    div()
                                        .w_full()
                                        .h(px(1.0 + negative * 100.0))
                                        .bg(meter_low.opacity(opacity)),
                                ),
                            )
                    }))
                    .into_any_element()
            }
            VisualizerMode::MilkDrop => {
                let status = self.milkdrop.status();
                let status_label = milkdrop_status_label(&status, is_playing);
                let status_message = match &status {
                    MilkdropStatus::Unavailable if !has_session => {
                        Some("LOAD A TRACK FOR MILKDROP")
                    }
                    MilkdropStatus::Unavailable => Some("MILKDROP UNAVAILABLE"),
                    MilkdropStatus::Initializing => None,
                    MilkdropStatus::Failed(_) => Some("MILKDROP RENDERER ERROR"),
                    MilkdropStatus::Running => None,
                };
                let mut visualizer = div()
                    .id("milkdrop-status")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(status_label)
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .bg(canvas)
                    .text_sm()
                    .text_color(text_muted);
                if matches!(
                    status,
                    MilkdropStatus::Initializing | MilkdropStatus::Running
                ) {
                    let host = self.milkdrop.clone();
                    let surface = gpui_canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _| host.present(bounds, window, is_playing),
                    )
                    .size_full();
                    visualizer = visualizer.child(surface);
                }
                if let Some(message) = status_message {
                    visualizer = visualizer.child(message);
                }
                visualizer.into_any_element()
            }
        };
        let play_label = if is_playing { "PAUSE" } else { "PLAY" };
        let play_accessibility_label = if is_playing { "Pause" } else { "Play" };
        let position = self.session.as_ref().map_or_else(
            || "00:00".to_owned(),
            |session| format_time(session.position_seconds()),
        );
        let duration = self.session.as_ref().map_or_else(
            || "00:00".to_owned(),
            |session| {
                session
                    .metadata()
                    .duration_seconds()
                    .map_or_else(|| "--:--".to_owned(), format_time)
            },
        );
        let progress_value = self.session.as_ref().and_then(|session| {
            let duration = session.metadata().duration_seconds()?;
            (duration.is_finite() && duration > 0.0)
                .then(|| (session.position_seconds() / duration * 100.0).clamp(0.0, 100.0))
        });
        let progress_accessibility_label =
            format!("Playback progress: {position} elapsed of {duration}");
        let volume = self
            .session
            .as_ref()
            .map_or(100, |session| (session.volume() * 100.0).round() as u32);
        let track_display = self
            .session
            .as_ref()
            .map_or_else(TrackDisplay::empty, |session| {
                let metadata = session.metadata();
                TrackDisplay::new(metadata.title(), metadata.artist(), metadata.album())
            });
        let track_accessibility_label = track_display.accessibility_label();
        let TrackDisplay {
            title,
            artist,
            album,
        } = track_display;
        let signal_status = signal_status(
            self.seeking,
            self.session.as_ref().map(PlaybackSession::transport_state),
            snapshot.as_ref().map(|snapshot| snapshot.decode()),
        );
        let playback_status_label = format!("Playback status: {signal_status}. {motion_label}");
        let detail = self.session.as_ref().map_or_else(
            || "SELECT A LOCAL FILE TO BEGIN".to_owned(),
            |session| {
                let metadata = session.metadata();
                format!("{} HZ / {} CH", metadata.sample_rate(), metadata.channels())
            },
        );
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.transport_state() == &TransportState::Playing)
        {
            window.request_animation_frame();
        } else if self.visualizer_mode == VisualizerMode::MilkDrop
            && has_session
            && matches!(self.milkdrop.status(), MilkdropStatus::Initializing)
        {
            // The native view attaches during canvas paint. Request one follow-up frame so GPUI's
            // status text reflects the result without animating indefinitely while paused.
            window.request_animation_frame();
        }

        div()
            .id("player-root")
            .test_support()
            .track_focus(&self.focus)
            .tab_group()
            .key_context("Amp808Player")
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| this.choose_file(cx)))
            .on_action(cx.listener(|this, _: &TogglePlayback, _, cx| {
                this.toggle_playback(cx);
            }))
            .on_action(cx.listener(|this, _: &SeekBack, _, cx| this.seek_by(-10.0, cx)))
            .on_action(cx.listener(|this, _: &SeekForward, _, cx| this.seek_by(10.0, cx)))
            .on_action(cx.listener(|this, _: &VolumeDown, _, cx| {
                this.adjust_volume(-0.05, cx);
            }))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| {
                this.adjust_volume(0.05, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleVisualizer, _, cx| {
                this.toggle_visualizer(cx);
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(canvas)
            .text_color(text)
            .p_5()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(accent)
                    .pb_3()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_3()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_semibold()
                                    .text_color(accent)
                                    .child(self.app.product_name()),
                            )
                            .child(
                                div()
                                    .pb_1()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child("MACOS PLAYER / LOCAL SIGNAL"),
                            ),
                    )
                    .child(
                        Button::new("open-file")
                            .key_context("Amp808Control")
                            .label("OPEN FILE")
                            .accessibility_label("Open a local audio file")
                            .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                    ),
            )
            .when_some(self.error.clone(), |root, error| {
                root.child(
                    div()
                        .id("playback-error")
                        .test_support()
                        .role(Role::Alert)
                        .aria_label(format!("Playback error: {error}"))
                        .flex()
                        .items_center()
                        .gap_3()
                        .rounded_md()
                        .border_1()
                        .border_color(accent)
                        .bg(panel_raised)
                        .px_4()
                        .py_3()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(accent)
                                .child("PLAYBACK ERROR"),
                        )
                        .child(div().text_sm().text_color(text).child(error)),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(310.0))
                            .min_w(px(310.0))
                            .rounded_lg()
                            .border_1()
                            .border_color(panel_raised)
                            .bg(panel)
                            .p_4()
                            .gap_4()
                            .child(div().text_xs().text_color(text_muted).child("NOW PLAYING"))
                            .child(
                                div()
                                    .id("now-playing-summary")
                                    .test_support()
                                    .role(Role::Group)
                                    .aria_label(track_accessibility_label)
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .aspect_square()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(panel_raised)
                                    .bg(canvas)
                                    .text_color(text_muted)
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_semibold()
                                            .text_color(text)
                                            .child(title),
                                    )
                                    .child(div().text_sm().child(artist))
                                    .child(div().text_xs().text_color(text_muted).child(album)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        Button::new("seek-back")
                                            .key_context("Amp808Control")
                                            .label("-10S")
                                            .accessibility_label("Seek back 10 seconds")
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.seek_by(-10.0, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("play-pause")
                                            .key_context("Amp808Control")
                                            .label(play_label)
                                            .accessibility_label(play_accessibility_label)
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_playback(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("seek-forward")
                                            .key_context("Amp808Control")
                                            .label("+10S")
                                            .accessibility_label("Seek forward 10 seconds")
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.seek_by(10.0, cx);
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Button::new("volume-down")
                                            .key_context("Amp808Control")
                                            .label("VOL-")
                                            .accessibility_label("Decrease volume")
                                            .disabled(!has_session)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.adjust_volume(-0.05, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("volume-status")
                                            .test_support()
                                            .role(Role::Status)
                                            .aria_label(format!("Volume {volume} percent"))
                                            .flex_1()
                                            .text_center()
                                            .text_xs()
                                            .text_color(text_muted)
                                            .child(format!("VOL {volume:03}%")),
                                    )
                                    .child(
                                        Button::new("volume-up")
                                            .key_context("Amp808Control")
                                            .label("VOL+")
                                            .accessibility_label("Increase volume")
                                            .disabled(!has_session)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.adjust_volume(0.05, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .rounded_lg()
                            .border_1()
                            .border_color(panel_raised)
                            .bg(panel)
                            .p_4()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(
                                        BaseButton::new("visualizer-mode")
                                            .key_context("Amp808Control")
                                            .accessibility_label(visualizer_action_label)
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(panel_raised)
                                            .focus_visible(|style| {
                                                style.border_2().border_color(accent).text_color(text)
                                            })
                                            .px_2()
                                            .py_1()
                                            .child(visualizer_label)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_visualizer(cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("motion-policy")
                                            .test_support()
                                            .role(Role::Status)
                                            .aria_label(playback_status_label)
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .size_2()
                                                    .rounded_full()
                                                    .bg(meter_high.opacity(status_opacity)),
                                            )
                                            .child(signal_status),
                                    ),
                            )
                            .child(visualizer)
                            .child(
                                div()
                                    .id("playback-progress")
                                    .test_support()
                                    .role(Role::ProgressIndicator)
                                    .aria_label(progress_accessibility_label)
                                    .aria_min_numeric_value(0.0)
                                    .aria_max_numeric_value(100.0)
                                    .when_some(progress_value, |progress, value| {
                                        progress.aria_numeric_value(value)
                                    })
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(position)
                                    .child(detail)
                                    .child(duration),
                            ),
                    ),
            )
            .child(
                div()
                    .id("keyboard-help")
                    .test_support()
                    .aria_label(
                        "Keyboard shortcuts: Command O open file, Space play or pause, Left and Right seek, Up and Down adjust volume, V switch visualizer",
                    )
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(text_muted)
                    .child(
                        "⌘O OPEN  ·  SPACE PLAY/PAUSE  ·  ←/→ SEEK  ·  ↑/↓ VOLUME  ·  V VISUALIZER",
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, Focusable, Point, Role, TestAppContext, WindowBounds, WindowOptions,
        px, size,
    };
    use tempfile::tempdir;

    use crate::app::transport::TransportState;
    use crate::milkdrop::MilkdropStatus;
    use crate::playback::DecodeWorkerState;

    use super::{
        PerformanceProbe, PlaybackToggle, PlayerView, TrackDisplay, milkdrop_status_label,
        playback_toggle, signal_status,
    };

    #[test]
    fn performance_probe_reports_average_fps_and_underflow_delta_from_its_active_baseline() {
        let mut probe = PerformanceProbe::enabled(Duration::from_secs(5));
        let started_at = Instant::now();

        assert_eq!(probe.observe(started_at, true, 100, 7), None);
        let sample = probe
            .observe(started_at + Duration::from_secs(5), true, 400, 9)
            .expect("five active seconds produce a performance sample");

        assert_eq!(sample.elapsed, Duration::from_secs(5));
        assert_eq!(sample.rendered_frames, 300);
        assert!((sample.average_fps - 60.0).abs() < f64::EPSILON);
        assert_eq!(sample.underflow_frames, 2);
    }

    #[test]
    fn performance_probe_restarts_qualification_after_milkdrop_becomes_inactive() {
        let mut probe = PerformanceProbe::enabled(Duration::from_secs(5));
        let started_at = Instant::now();

        assert_eq!(probe.observe(started_at, true, 100, 3), None);
        assert_eq!(
            probe.observe(started_at + Duration::from_secs(3), false, 280, 4),
            None
        );
        assert_eq!(
            probe.observe(started_at + Duration::from_secs(4), true, 300, 5),
            None
        );
        let sample = probe
            .observe(started_at + Duration::from_secs(9), true, 450, 5)
            .expect("the restarted active interval produces its own sample");

        assert_eq!(sample.elapsed, Duration::from_secs(5));
        assert_eq!(sample.rendered_frames, 150);
        assert_eq!(sample.underflow_frames, 0);
    }

    #[test]
    fn play_from_ended_requests_a_restart() {
        assert_eq!(
            playback_toggle(&TransportState::Ended),
            PlaybackToggle::Restart
        );
    }

    #[test]
    fn drained_transport_is_presented_as_ended() {
        assert_eq!(
            signal_status(
                false,
                Some(&TransportState::Ended),
                Some(&DecodeWorkerState::Ended)
            ),
            "PCM / ENDED"
        );
    }

    #[test]
    fn milkdrop_status_distinguishes_initializing_active_paused_and_failed() {
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Initializing, true),
            "MilkDrop initializing"
        );
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Running, true),
            "MilkDrop active"
        );
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Running, false),
            "MilkDrop paused"
        );
        assert_eq!(
            milkdrop_status_label(&MilkdropStatus::Failed("preset rejected".into()), true),
            "MilkDrop error: preset rejected"
        );
    }

    #[test]
    fn track_display_preserves_available_title_artist_and_album_tags() {
        let display = TrackDisplay::new("Halcyon", Some("Orbital"), Some("Brown Album"));

        assert_eq!(display.title(), "Halcyon");
        assert_eq!(display.artist(), "Orbital");
        assert_eq!(display.album(), "Brown Album");
    }

    #[test]
    fn track_display_labels_missing_optional_tags_honestly() {
        let display = TrackDisplay::new("Untitled", None, None);

        assert_eq!(display.title(), "Untitled");
        assert_eq!(display.artist(), "UNKNOWN ARTIST");
        assert_eq!(display.album(), "UNKNOWN ALBUM");
    }

    #[gpui_kit::test]
    fn now_playing_metadata_is_exposed_to_assistive_technology(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let summary = window.find("now-playing-summary");
            assert_eq!(summary.role(), Some(Role::Group));
            assert_eq!(
                summary.label(),
                Some("No track loaded. Artist: --. Album: --.")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn volume_value_is_exposed_to_assistive_technology(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let volume = window.find("volume-status");
            assert_eq!(volume.role(), Some(Role::Status));
            assert_eq!(volume.label(), Some("Volume 100 percent"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn pcm_status_is_exposed_with_the_motion_policy(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let status = window.find("motion-policy");
            assert_eq!(status.role(), Some(Role::Status));
            assert_eq!(
                status.label(),
                Some(
                    "Playback status: PCM / IDLE. Standard motion enabled; essential audio levels remain active"
                )
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn playback_time_is_exposed_as_progress(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let progress = window.find("playback-progress");
            assert_eq!(progress.role(), Some(Role::ProgressIndicator));
            assert_eq!(
                progress.label(),
                Some("Playback progress: 00:00 elapsed of 00:00")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn open_button_requests_one_local_file(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("open-file", cx);
        })
        .unwrap();

        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|options| {
            assert!(options.files);
            assert!(!options.directories);
            assert!(!options.multiple);
            None
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn corrupt_file_failure_is_exposed_as_an_accessible_alert(cx: &mut TestAppContext) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("broken.wav");
        fs::write(&path, b"not audio").unwrap();
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("open-file", cx);
        })
        .unwrap();
        cx.simulate_path_prompt_response(move |_| Some(vec![path]));
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let alert = window.find("playback-error");
            assert_eq!(alert.role(), Some(Role::Alert));
            assert!(
                alert
                    .label()
                    .is_some_and(|label| label.contains("broken.wav"))
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn transport_controls_expose_accessible_desktop_actions(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("seek-back").label(),
                Some("Seek back 10 seconds")
            );
            assert_eq!(window.find("play-pause").label(), Some("Play"));
            assert_eq!(
                window.find("seek-forward").label(),
                Some("Seek forward 10 seconds")
            );
            assert_eq!(window.find("volume-down").label(), Some("Decrease volume"));
            assert_eq!(window.find("volume-up").label(), Some("Increase volume"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn visualizer_control_cycles_through_milkdrop_with_honest_unavailable_state(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show waveform visualizer")
            );
            window.click("visualizer-mode", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show MilkDrop visualizer")
            );
            window.click("visualizer-mode", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show spectrum visualizer")
            );
            assert_eq!(
                window.find("milkdrop-status").label(),
                Some("MilkDrop unavailable")
            );
            window.click("visualizer-mode", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show waveform visualizer")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn focused_visualizer_control_accepts_keyboard_activation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init_keyboard_shortcuts(cx);
        });
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let view = cx.new(PlayerView::new);
                    window.focus(&view.read(cx).focus_handle(cx), cx);
                    view
                },
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("player-root").focused(), Some(true));
            window.press("tab", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("open-file").focused(), Some(true));
            window.press("tab", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("visualizer-mode").focused(), Some(true));

            window.press("enter", cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show MilkDrop visualizer")
            );
            window.press("space", cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show spectrum visualizer")
            );
            window.press("enter", cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show waveform visualizer")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn player_keyboard_shortcuts_open_files_and_switch_visualizers(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init_keyboard_shortcuts(cx);
        });
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let view = cx.new(PlayerView::new);
                    window.focus(&view.read(cx).focus_handle(cx), cx);
                    view
                },
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.press("cmd-o", cx);
        })
        .unwrap();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.press("v", cx);
            window.render_frame(cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show MilkDrop visualizer")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn keyboard_shortcuts_are_visible_and_accessibly_described(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(PlayerView::new)
            })
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let help = window.find("keyboard-help");
            assert_eq!(
                help.label(),
                Some(
                    "Keyboard shortcuts: Command O open file, Space play or pause, Left and Right seek, Up and Down adjust volume, V switch visualizer"
                )
            );
            assert!(help.visible());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn reduced_motion_keeps_essential_audio_visualization_available(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (window, _view) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1_120.0), px(720.0)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(PlayerView::new),
            )
            .unwrap()
        });

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let policy = window.find("motion-policy");
            assert_eq!(policy.role(), Some(Role::Status));
            assert_eq!(
                policy.label(),
                Some(
                    "Playback status: PCM / IDLE. Reduced motion enabled; essential audio levels remain active"
                )
            );
            assert!(window.find("visualizer-mode").visible());
        })
        .unwrap();
    }
}
