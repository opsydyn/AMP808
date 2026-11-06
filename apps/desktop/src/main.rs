mod analysis;
mod app;
mod audio;
mod cosmic;
mod library;
mod media;
mod milkdrop;
mod playback;
mod queue;
mod skin;
mod ui;

fn main() {
    use gpui_kit::{
        AppContext, Bounds, Focusable, TitlebarOptions, WindowBounds, WindowOptions, px, size,
    };

    let window_spec = app::WindowSpec::main_player();

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        ui::init_keyboard_shortcuts(cx);

        let (width, height) = window_spec.initial_size();
        let (minimum_width, minimum_height) = window_spec.minimum_size();
        let bounds = Bounds::centered(None, size(px(f32::from(width)), px(f32::from(height))), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(
                px(f32::from(minimum_width)),
                px(f32::from(minimum_height)),
            )),
            titlebar: Some(TitlebarOptions {
                title: Some(window_spec.title().into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        gpui_kit::open_window(options, cx, |window, cx| {
            let view = cx.new(ui::PlayerView::new);
            window.focus(&view.read(cx).focus_handle(cx), cx);
            view
        })
        .expect("failed to open AMP808-R3TR0 window");
        cx.activate(true);
    });
}
