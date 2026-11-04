use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::button::Button;
use gpui_kit::{Context, IntoElement, PathPromptOptions, Render, Window, div, prelude::*, px, rgb};

use crate::app::DesktopApp;
use crate::app::transport::TransportState;
use crate::playback::{DecodeWorkerState, PlaybackSession, PlaybackSessionError};
use crate::skin::Amp808Skin;

pub struct PlayerView {
    app: DesktopApp,
    skin: Amp808Skin,
    session: Option<PlaybackSession>,
    error: Option<String>,
    seeking: bool,
}

impl PlayerView {
    pub fn new() -> Self {
        Self {
            app: DesktopApp::new(),
            skin: Amp808Skin::built_in(),
            session: None,
            error: None,
            seeking: false,
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
}

fn format_time(seconds: f64) -> String {
    let total_seconds = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
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
        let spectrum = snapshot
            .as_ref()
            .map(|snapshot| snapshot.analysis().spectrum().to_vec())
            .unwrap_or_else(|| vec![0.0; 24]);
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
        let track_title = self
            .session
            .as_ref()
            .map(|session| session.metadata().title().to_owned())
            .unwrap_or_else(|| "NO TRACK LOADED".to_owned());
        let signal_status = if self.seeking {
            "PCM / BUFFERING"
        } else {
            snapshot
                .as_ref()
                .map_or("PCM / IDLE", |snapshot| match snapshot.decode() {
                    DecodeWorkerState::Starting => "PCM / BUFFERING",
                    DecodeWorkerState::Running => "PCM / LIVE",
                    DecodeWorkerState::Ended => "PCM / DRAINING",
                    DecodeWorkerState::Failed(_) => "PCM / ERROR",
                    DecodeWorkerState::Stopped => "PCM / STOPPED",
                })
        };
        let detail = self.error.clone().unwrap_or_else(|| {
            self.session.as_ref().map_or_else(
                || "SELECT A LOCAL FILE TO BEGIN".to_owned(),
                |session| {
                    let metadata = session.metadata();
                    format!("{} HZ / {} CH", metadata.sample_rate(), metadata.channels())
                },
            )
        });
        if self.session.is_some() {
            window.request_animation_frame();
        }

        div()
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
                                    .items_center()
                                    .justify_center()
                                    .aspect_square()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(panel_raised)
                                    .bg(canvas)
                                    .text_color(text_muted)
                                    .child(track_title),
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
                                    .child("SPECTRUM")
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .size_2()
                                                    .rounded_full()
                                                    .bg(meter_high.opacity(0.45)),
                                            )
                                            .child(signal_status),
                                    ),
                            )
                            .child(
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
                                    })),
                            )
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
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size,
    };

    use super::PlayerView;

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
                |_, cx| cx.new(|_| PlayerView::new()),
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
                |_, cx| cx.new(|_| PlayerView::new()),
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
}
