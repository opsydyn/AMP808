use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::{Context, IntoElement, Render, Window, div, prelude::*, px, rgb};

use crate::app::DesktopApp;
use crate::skin::Amp808Skin;

pub struct PlayerView {
    app: DesktopApp,
    skin: Amp808Skin,
}

impl PlayerView {
    pub fn new() -> Self {
        Self {
            app: DesktopApp::new(),
            skin: Amp808Skin::built_in(),
        }
    }

    fn color(&self, value: crate::skin::SkinColor) -> gpui_kit::Rgba {
        rgb(value.hex())
    }
}

impl Render for PlayerView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = self.color(self.skin.canvas());
        let panel = self.color(self.skin.panel());
        let panel_raised = self.color(self.skin.panel_raised());
        let text = self.color(self.skin.text());
        let text_muted = self.color(self.skin.text_muted());
        let accent = self.color(self.skin.accent());
        let meter_low = self.color(self.skin.meter_low());
        let meter_high = self.color(self.skin.meter_high());

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
                            .accessibility_label("Open a local audio file"),
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
                                    .child("NO TRACK LOADED"),
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
                                            .child("PCM / IDLE"),
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
                                    .children((0..24).map(|_| {
                                        div()
                                            .flex_1()
                                            .h(px(3.0))
                                            .rounded_sm()
                                            .bg(meter_low.opacity(0.18))
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child("00:00")
                                    .child("SELECT A LOCAL FILE TO BEGIN")
                                    .child("00:00"),
                            ),
                    ),
            )
    }
}
