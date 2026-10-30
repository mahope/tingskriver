//! Tingskriver GPUI shell. Opens an empty window until the screens are
//! migrated from the Tauri app on top of `tingskriver-core`.

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use gpui_component::Root;

struct Shell;

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tingskriver".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let shell = cx.new(|_| Shell);
                cx.new(|cx| Root::new(shell, window, cx))
            },
        )
        .expect("failed to open the Tingskriver window");
        cx.activate(true);
    });
}
