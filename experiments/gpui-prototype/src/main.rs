use anyhow::Result;
use gpui::{
    AppContext as _, Application, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Window, WindowOptions, div, px, rgb,
};
use gpui::prelude::FluentBuilder;
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
};

const APP_BG: u32 = 0x1e1e2e;
const SURFACE: u32 = 0x181825;
const RAISED: u32 = 0x313244;
const BORDER: u32 = 0x45475a;
const TEXT: u32 = 0xcdd6f4;
const MUTED: u32 = 0xa6adc8;
const ACCENT: u32 = 0x89b4fa;
const SUCCESS: u32 = 0xa6e3a1;
const WARNING: u32 = 0xf9e2af;

#[derive(Clone)]
struct Connection {
    id: String,
    name: String,
    host: String,
    user: String,
    port: u16,
    local: bool,
}

#[derive(Clone)]
struct TerminalTab {
    connection_id: String,
    title: String,
}

struct PrototypeApp {
    search: Entity<InputState>,
    name_input: Entity<InputState>,
    host_input: Entity<InputState>,
    user_input: Entity<InputState>,
    port_input: Entity<InputState>,
    command_input: Entity<InputState>,
    connections: Vec<Connection>,
    tabs: Vec<TerminalTab>,
    active_tab: usize,
    split_tab: Option<usize>,
    split_enabled: bool,
    console_open: bool,
    add_error: Option<String>,
    console_feedback: Option<String>,
}

impl PrototypeApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = Self::input(window, cx, "Search connections");
        let name_input = Self::input(window, cx, "Connection name");
        let host_input = Self::input(window, cx, "Host name or IP");
        let user_input = Self::input(window, cx, "Username");
        let port_input = Self::input(window, cx, "22");
        let command_input = Self::input(window, cx, "Type a command");

        for input in [
            &search,
            &name_input,
            &host_input,
            &user_input,
            &port_input,
            &command_input,
        ] {
            cx.subscribe(input, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }

        Self {
            search,
            name_input,
            host_input,
            user_input,
            port_input,
            command_input,
            connections: vec![Connection {
                id: "local-shell".into(),
                name: "Local PowerShell".into(),
                host: String::new(),
                user: String::new(),
                port: 22,
                local: true,
            }],
            tabs: vec![TerminalTab {
                connection_id: "local-shell".into(),
                title: "Local PowerShell".into(),
            }],
            active_tab: 0,
            split_tab: None,
            split_enabled: true,
            console_open: true,
            add_error: None,
            console_feedback: None,
        }
    }

    fn input(window: &mut Window, cx: &mut Context<Self>, placeholder: &'static str) -> Entity<InputState> {
        cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
    }

    fn add_connection(&mut self, _: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().trim().to_owned();
        let host = self.host_input.read(cx).value().trim().to_owned();
        let user = self.user_input.read(cx).value().trim().to_owned();
        let port = self.port_input.read(cx).value().trim().parse::<u16>().ok();

        if name.is_empty() || host.is_empty() || user.is_empty() || !port.is_some_and(|value| value > 0) {
            self.add_error = Some("Enter a name, host, username, and port from 1 to 65535.".into());
            cx.notify();
            return;
        }

        let port = port.unwrap_or(22);
        self.connections.push(Connection {
            id: format!("ssh-{}", self.connections.len()),
            name,
            host,
            user,
            port,
            local: false,
        });
        self.add_error = None;
        cx.notify();
    }

    fn open_connection(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(connection) = self.connections.get(index) else { return };
        if let Some(existing) = self.tabs.iter().position(|tab| tab.connection_id == connection.id) {
            self.active_tab = existing;
        } else {
            self.tabs.push(TerminalTab {
                connection_id: connection.id.clone(),
                title: connection.name.clone(),
            });
            self.active_tab = self.tabs.len() - 1;
        }
        self.console_feedback = None;
        cx.notify();
    }

    fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active_tab = index;
            cx.notify();
        }
    }

    fn new_tab(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.tabs.get(self.active_tab).cloned() else { return };
        let number = self.tabs.len() + 1;
        self.tabs.push(TerminalTab {
            connection_id: current.connection_id,
            title: format!("{} ({number})", current.title),
        });
        self.active_tab = self.tabs.len() - 1;
        cx.notify();
    }

    fn send_command(&mut self, _: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let command = self.command_input.read(cx).value().trim().to_owned();
        self.console_feedback = if command.is_empty() {
            Some("Enter a command first.".into())
        } else {
            Some("Command sending is pending while terminal I/O is being fixed.".into())
        };
        cx.notify();
    }

    fn connection_button(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let connection = &self.connections[index];
        let active_id = &self.tabs[self.active_tab].connection_id;
        let active = connection.id == *active_id;
        let title = connection.name.clone();
        let detail = if connection.local {
            "Local shell".to_owned()
        } else {
            format!("{}@{}:{}", connection.user, connection.host, connection.port)
        };
        let row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(7.0))
            .rounded(px(5.0))
            .bg(if active { rgb(RAISED) } else { rgb(SURFACE) })
            .child(div().w(px(7.0)).h(px(7.0)).rounded(px(4.0)).bg(if active { rgb(ACCENT) } else { rgb(MUTED) }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().text_size(px(12.0)).text_color(rgb(TEXT)).child(title))
                    .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child(detail)),
            );

        Button::new(("connection", index as u32))
            .ghost()
            .w_full()
            .child(row)
            .on_click(cx.listener(move |this, _, _, cx| this.open_connection(index, cx)))
    }

    fn tab_button(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = &self.tabs[index];
        let title = tab.title.clone();
        let active = index == self.active_tab;
        Button::new(("terminal-tab", index as u32))
            .ghost()
            .h(px(34.0))
            .px(px(10.0))
            .bg(if active { rgb(RAISED) } else { rgb(SURFACE) })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .child(div().w(px(6.0)).h(px(6.0)).rounded(px(3.0)).bg(if active { rgb(WARNING) } else { rgb(MUTED) }))
                    .child(div().text_size(px(12.0)).text_color(if active { rgb(TEXT) } else { rgb(MUTED) }).child(title)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.activate_tab(index, cx)))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let visible = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                query.is_empty()
                    || item.name.to_lowercase().contains(&query)
                    || item.host.to_lowercase().contains(&query)
                    || item.user.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let local = visible.iter().copied().find(|&index| self.connections[index].local);
        let saved = visible.into_iter().filter(|&index| !self.connections[index].local).collect::<Vec<_>>();
        let has_saved = !saved.is_empty();
        let saved_rows = saved
            .into_iter()
            .map(|index| self.connection_button(index, cx).into_any_element())
            .collect::<Vec<_>>();

        div()
            .w(px(278.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .p(px(12.0))
            .bg(rgb(SURFACE))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(14.0)).font_weight(gpui::FontWeight::SEMIBOLD).child("Connections"))
                    .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child(format!("{}", self.connections.len()))),
            )
            .child(Input::new(&self.search).w_full())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(div().px(px(5.0)).text_size(px(10.0)).text_color(rgb(MUTED)).child("LOCAL"))
                    .when_some(local, |element, index| element.child(self.connection_button(index, cx))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .px(px(5.0))
                    .pt(px(5.0))
                    .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child("▾"))
                    .child(div().text_size(px(10.0)).text_color(rgb(MUTED)).child("SAVED SSH")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .pl(px(8.0))
                    .children(saved_rows)
                    .when(!has_saved, |element| {
                        element.child(div().pl(px(14.0)).text_size(px(11.0)).text_color(rgb(MUTED)).child("No matching connections"))
                    }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .p(px(9.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(div().text_size(px(12.0)).text_color(rgb(TEXT)).child("Add SSH connection"))
                    .child(Input::new(&self.name_input).w_full())
                    .child(Input::new(&self.host_input).w_full())
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(Input::new(&self.user_input).flex_1())
                            .child(Input::new(&self.port_input).w(px(58.0))),
                    )
                    .child(
                        Button::new("save-connection")
                            .primary()
                            .label("Add connection")
                            .on_click(cx.listener(Self::add_connection)),
                    ),
            )
            .when_some(self.add_error.clone(), |element, message| {
                element.child(div().text_size(px(11.0)).text_color(rgb(WARNING)).child(message))
            })
    }

    fn terminal_pane(&self, title: String, active: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if active { "ACTIVE TAB" } else { "SPLIT PANE" };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .bg(rgb(APP_BG))
            .child(
                div()
                    .h(px(32.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(11.0))
                    .bg(rgb(SURFACE))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .child(div().w(px(6.0)).h(px(6.0)).rounded(px(3.0)).bg(if active { rgb(WARNING) } else { rgb(MUTED) }))
                            .child(div().text_size(px(11.0)).text_color(rgb(TEXT)).child(title)),
                    )
                    .child(div().text_size(px(10.0)).text_color(rgb(MUTED)).child(label)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(10.0))
                    .p(px(24.0))
                    .child(div().text_size(px(13.0)).text_color(rgb(TEXT)).child("Terminal pane"))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .child("Terminal input and output are pending; this pane is a layout prototype."),
                    )
                    .when(!active, |element| {
                        element.child(
                            Button::new("open-in-split")
                                .ghost()
                                .label("Open active tab in this pane")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.split_tab = Some(this.active_tab);
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }

    fn command_console(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(146.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(SURFACE))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(34.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.0))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(div().text_size(px(12.0)).font_weight(gpui::FontWeight::SEMIBOLD).child("Commands"))
                    .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child("Send to active tab")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .p(px(10.0))
                    .child(Input::new(&self.command_input).flex_1())
                    .child(
                        Button::new("send-command")
                            .primary()
                            .label("Send")
                            .on_click(cx.listener(Self::send_command)),
                    ),
            )
            .when_some(self.console_feedback.clone(), |element, message| {
                element.child(div().px(px(12.0)).text_size(px(11.0)).text_color(rgb(WARNING)).child(message))
            })
            .when(self.console_feedback.is_none(), |element| {
                element.child(
                    div()
                        .px(px(12.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child("Commands will be routed to the active terminal when I/O is ready."),
                )
            })
    }
}

impl Render for PrototypeApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_title = self.tabs.get(self.active_tab).map(|tab| tab.title.clone()).unwrap_or_else(|| "No tab".into());
        let split_title = self.split_tab.and_then(|index| self.tabs.get(index)).map(|tab| tab.title.clone());
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, _)| self.tab_button(index, cx).into_any_element())
            .collect::<Vec<_>>();

        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Segoe UI")
            .text_color(rgb(TEXT))
            .bg(rgb(APP_BG))
            .child(
                div()
                    .h(px(42.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(14.0))
                    .bg(rgb(SURFACE))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .child(div().text_size(px(14.0)).font_weight(gpui::FontWeight::SEMIBOLD).child("SSHTerm"))
                            .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child("GPUI exploration")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(div().text_size(px(11.0)).text_color(rgb(WARNING)).child("TERMINAL I/O PENDING"))
                            .child(
                                Button::new("toggle-split")
                                    .ghost()
                                    .label(if self.split_enabled { "Close split" } else { "Split right" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.split_enabled = !this.split_enabled;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("toggle-console")
                                    .ghost()
                                    .label(if self.console_open { "Hide commands" } else { "Show commands" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.console_open = !this.console_open;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .h(px(38.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .px(px(6.0))
                                    .bg(rgb(SURFACE))
                                    .border_b_1()
                                    .border_color(rgb(BORDER))
                                    .children(tabs)
                                    .child(
                                        Button::new("new-tab")
                                            .ghost()
                                            .label("+")
                                            .on_click(cx.listener(|this, _, _, cx| this.new_tab(cx))),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .child(self.terminal_pane(active_title, true, cx))
                                    .when(self.split_enabled, |row| {
                                        row.child(div().w(px(1.0)).flex_shrink_0().bg(rgb(BORDER)))
                                            .child(self.terminal_pane(split_title.unwrap_or_else(|| "Empty pane".into()), false, cx))
                                    }),
                            )
                            .when(self.console_open, |column| column.child(self.command_console(cx))),
                    ),
            )
    }
}

fn main() -> Result<()> {
    let app = Application::new();
    app.run(|cx| {
        gpui_component::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(
                WindowOptions {
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("SSHTerm — GPUI Prototype".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(gpui::size(px(960.0), px(600.0))),
                    ..Default::default()
                },
                |window, cx| {
                    let root = cx.new(|cx| PrototypeApp::new(window, cx));
                    cx.new(|cx| Root::new(root, window, cx))
                },
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
    Ok(())
}
