//! Command console: a multi-line editor and the send/clear actions.
//!
//! Built from GPUI Kit's `Textarea` and `Button`. Sending is a placeholder until sessions exist.

use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
    input::{Textarea, TextareaState},
};
use gpui_kit::{Context, Entity, IntoElement, Render, SharedString, Window, base::StyledExt, div, prelude::*, px};

pub struct CommandConsole {
    input: Entity<TextareaState>,
    status: SharedString,
}

impl CommandConsole {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Type commands to send to the active session…"));
        Self { input, status: "Send is a placeholder: no session is attached yet.".into() }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value();
        let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
        self.status = if lines == 0 {
            "Nothing to send.".into()
        } else {
            format!("Would send {lines} line(s) to the active session (prototype: not connected).").into()
        };
        cx.notify();
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.set_value("", window, cx));
        self.status = "Cleared.".into();
        cx.notify();
    }
}

impl Render for CommandConsole {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_2()
            .p_3()
            .bg(cx.theme().secondary)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child("COMMAND CONSOLE"))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Send to: Active session")),
            )
            .child(div().flex_1().min_h(px(0.)).child(Textarea::new(&self.input).size_full()))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(self.status.clone()))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("console-clear")
                                    .small()
                                    .ghost()
                                    .label("Clear")
                                    .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx))),
                            )
                            .child(
                                Button::new("console-send")
                                    .small()
                                    .primary()
                                    .label("Send")
                                    .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
                            ),
                    ),
            )
    }
}
