//! First-stage smoke test: a minimal native GPUI window.
use gpui::{App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size};
use gpui_platform::application;

struct Smoke;

impl Render for Smoke {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x171b24))
            .text_color(rgb(0xe6eaf2))
            .text_xl()
            .child("GPUI native window launched successfully")
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(640.0), px(360.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Smoke),
        )
        .expect("open GPUI smoke-test window");
        cx.activate(true);
    });
}
