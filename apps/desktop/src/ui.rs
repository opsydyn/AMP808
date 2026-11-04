use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::{Context, IntoElement, PathPromptOptions, Render, Window, div, prelude::*, px, rgb};

use crate::app::DesktopApp;
use crate::playback::{DecodeWorkerState, PlaybackSession};
use crate::skin::Amp808Skin;

pub struct PlayerView {
    app: DesktopApp,
    skin: Amp808Skin,
    session: Option<PlaybackSession>,
    error: Option<String>,
}

impl PlayerView {
    pub fn new() -> Self {
        Self {
            app: DesktopApp::new(),
            skin: Amp808Skin::built_in(),
            session: None,
            error: None,
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
                    }
                    Err(error) => {
                        view.session = None;
                        view.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
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
        let track_title = self
            .session
            .as_ref()
            .map(|session| session.metadata().title().to_owned())
            .unwrap_or_else(|| "NO TRACK LOADED".to_owned());
        let signal_status =
            snapshot
                .as_ref()
                .map_or("PCM / IDLE", |snapshot| match snapshot.decode() {
                    DecodeWorkerState::Starting => "PCM / BUFFERING",
                    DecodeWorkerState::Running => "PCM / LIVE",
                    DecodeWorkerState::Ended => "PCM / DRAINING",
                    DecodeWorkerState::Failed(_) => "PCM / ERROR",
                    DecodeWorkerState::Stopped => "PCM / STOPPED",
                });
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
                                        Button::new("previous")
                                            .label("PREV")
                                            .accessibility_label("Previous track"),
                                    )
                                    .child(
                                        Button::new("play-pause")
                                            .label("PLAY")
                                            .accessibility_label("Play"),
                                    )
                                    .child(
                                        Button::new("next")
                                            .label("NEXT")
                                            .accessibility_label("Next track"),
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
                                    .child("00:00")
                                    .child(detail)
                                    .child("00:00"),
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
}
