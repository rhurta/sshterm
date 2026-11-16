//! First-stage smoke test using the GPUI Kit application entrypoint.
use gpui_kit::*;

struct Smoke;

impl Render for Smoke {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_xl()
            .child("GPUI Kit native window launched successfully")
    }
}

fn main() {
    application().run(|cx| {
        init(cx);
        open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Smoke))
            .expect("open GPUI Kit smoke-test window");
    });
}
