use gpui_kit::base::{Button as BaseButton, Disableable, StyledExt};
use gpui_kit::component::button::Button;
use gpui_kit::{
    App, Context, FocusHandle, Focusable, IntoElement, PathPromptOptions, Render, Role,
    TestSupportExt, Window, div, prelude::*, px, rgb,
};

use crate::app::DesktopApp;
use crate::app::transport::TransportState;
use crate::playback::{DecodeWorkerState, PlaybackSession, PlaybackSessionError};
use crate::skin::Amp808Skin;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisualizerMode {
    Spectrum,
    Waveform,
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
    error: Option<String>,
    seeking: bool,
    visualizer_mode: VisualizerMode,
}

impl PlayerView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            app: DesktopApp::new(),
            focus: cx.focus_handle(),
            skin: Amp808Skin::built_in(),
            session: None,
            error: None,
            seeking: false,
            visualizer_mode: VisualizerMode::Spectrum,
        }
    }

    fn color(&self, value: crate::skin::SkinColor) -> gpui_kit::Rgba {
        rgb(value.hex())
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
                        view.session = Some(session);
                        view.error = None;
                        view.seeking = false;
                    }
                    Err(error) => {
                        view.session = None;
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
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let result = match session.transport_state() {
            TransportState::Playing => session.pause(),
            TransportState::Paused | TransportState::Ready => session.play(),
            _ => return,
        };
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
        cx.notify();
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
                    Ok(session) => view.session = Some(session),
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
            VisualizerMode::Waveform => VisualizerMode::Spectrum,
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
        let (visualizer_label, visualizer_action_label) = match self.visualizer_mode {
            VisualizerMode::Spectrum => ("SPECTRUM", "Show waveform visualizer"),
            VisualizerMode::Waveform => ("WAVEFORM", "Show spectrum visualizer"),
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
        };
        let has_session = self.session.is_some();
        let is_playing = self
            .session
            .as_ref()
            .is_some_and(|session| session.transport_state() == &TransportState::Playing);
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
        }

        div()
            .id("player-root")
            .test_support()
            .track_focus(&self.focus)
            .tab_group()
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
                                            .label("-10S")
                                            .accessibility_label("Seek back 10 seconds")
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.seek_by(-10.0, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("play-pause")
                                            .label(play_label)
                                            .accessibility_label(play_accessibility_label)
                                            .disabled(!has_session || self.seeking)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_playback(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("seek-forward")
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
                                            .label("VOL-")
                                            .accessibility_label("Decrease volume")
                                            .disabled(!has_session)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.adjust_volume(-0.05, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_center()
                                            .text_xs()
                                            .text_color(text_muted)
                                            .child(format!("VOL {volume:03}%")),
                                    )
                                    .child(
                                        Button::new("volume-up")
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
                                            .accessibility_label(visualizer_action_label)
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(panel_raised)
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
                                            .aria_label(motion_label)
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
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, Focusable, Point, Role, TestAppContext, WindowBounds, WindowOptions,
        px, size,
    };
    use tempfile::tempdir;

    use crate::app::transport::TransportState;
    use crate::playback::DecodeWorkerState;

    use super::{PlayerView, TrackDisplay, signal_status};

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
    fn visualizer_control_switches_between_spectrum_and_waveform(cx: &mut TestAppContext) {
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
                Some("Show spectrum visualizer")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn focused_visualizer_control_accepts_keyboard_activation(cx: &mut TestAppContext) {
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
                Some("Show spectrum visualizer")
            );
            window.press("space", cx);
            assert_eq!(
                window.find("visualizer-mode").label(),
                Some("Show waveform visualizer")
            );
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
                Some("Reduced motion enabled; essential audio levels remain active")
            );
            assert!(window.find("visualizer-mode").visible());
        })
        .unwrap();
    }
}
