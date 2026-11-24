//! Pane area content: the terminal placeholder panel hosted by GPUI Kit's `DockArea`.
//!
//! The dock supplies tab groups, splits, dragging tabs between groups, splitting by dropping on an
//! edge, zoom and layout persistence, so no custom tab/split code lives here.

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, h_flex,
    dock::{BasePanel, Panel, PanelEvent},
};
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Window, div,
    prelude::*, px,
};

/// One open connection. A placeholder until a session service and a terminal emulator exist.
pub struct TerminalPanel {
    pub connection_id: u64,
    name: SharedString,
    endpoint: SharedString,
    focus_handle: FocusHandle,
}

impl TerminalPanel {
    pub fn new(connection_id: u64, name: &str, endpoint: String, cx: &mut Context<Self>) -> Self {
        Self { connection_id, name: name.to_string().into(), endpoint: endpoint.into(), focus_handle: cx.focus_handle() }
    }
}

impl EventEmitter<PanelEvent> for TerminalPanel {}

impl Focusable for TerminalPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "TerminalPanel"
    }
}

impl Panel for TerminalPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().items_center().gap_1().child(Icon::new(IconName::SquareTerminal).small()).child(self.name.clone())
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .bg(cx.theme().background)
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(format!(
                "sshterm GPUI prototype\n\nSession: {}\nEndpoint: {}\n\nNo SSH process is attached yet.\nThis pane is a placeholder for the terminal view.\n\n$ _",
                self.name, self.endpoint
            ))
            .min_h(px(0.))
    }
}
