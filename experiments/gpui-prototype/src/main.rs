Warning: fs was declared with const; use let for reassignable variables.
use anyhow::{Context as _, Result};
use gpui::{
    AppContext as _, Application, Context, Edges, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Window, WindowOptions, div, px, rgb,
};
use gpui::prelude::FluentBuilder;
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
};
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use parking_lot::Mutex;
use portable_pty::{
    Child, CommandBuilder, MasterPty, PtySize, native_pty_system,
};
use std::{io::{Read, Write}, sync::Arc};

const APP_BG: u32 = 0x1e1e2e;
const SURFACE: u32 = 0x181825;
const RAISED: u32 = 0x313244;
const BORDER: u32 = 0x45475a;
const TEXT: u32 = 0xcdd6f4;
const MUTED: u32 = 0xa6adc8;
const ACCENT: u32 = 0x89b4fa;
const SUCCESS: u32 = 0xa6e3a1;
const DANGER: u32 = 0xf38ba8;

#[derive(Clone)]
struct SavedConnection {
    id: String,
    name: String,
    host: String,
    user: String,
    port: u16,
    local: bool,
}

struct ProcessSession {
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Box<dyn Child + Send + Sync>,
}

struct PrototypeApp {
    search: Entity<InputState>,
    name_input: Entity<InputState>,
    host_input: Entity<InputState>,
    user_input: Entity<InputState>,
    port_input: Entity<InputState>,
    connections: Vec<SavedConnection>,
    terminal: Entity<TerminalView>,
    child: Box<dyn Child + Send + Sync>,
    selected_id: String,
    selected_label: String,
    error: Option<String>,
}

impl PrototypeApp {
    fn new(window: &mut Window, cx: &mut Context<Self>, session: ProcessSession) -> Self {
        let search = Self::input(window, cx, "Search connections");
        let name_input = Self::input(window, cx, "Connection name");
        let host_input = Self::input(window, cx, "Host name or IP");
        let user_input = Self::input(window, cx, "Username");
        let port_input = Self::input(window, cx, "22");

        for input in [&search, &name_input, &host_input, &user_input, &port_input] {
            cx.subscribe(input, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }

        let terminal = Self::terminal(cx, session.reader, session.writer, session.master.clone());
        terminal.read(cx).focus_handle().focus(window);

        Self {
            search,
            name_input,
            host_input,
            user_input,
            port_input,
            connections: vec![SavedConnection {
                id: "local-shell".into(),
                name: "Local PowerShell".into(),
                host: String::new(),
                user: String::new(),
                port: 22,
                local: true,
            }],
            terminal,
            child: session.child,
            selected_id: "local-shell".into(),
            selected_label: "Local PowerShell".into(),
            error: None,
        }
    }

    fn input(window: &mut Window, cx: &mut Context<Self>, placeholder: &'static str) -> Entity<InputState> {
        cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
    }

    fn terminal(
        cx: &mut Context<Self>,
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    ) -> Entity<TerminalView> {
        let colors = ColorPalette::builder()
            .background(0x1e, 0x1e, 0x2e)
            .foreground(0xcd, 0xd6, 0xf4)
            .cursor(0xf5, 0xe0, 0xdc)
            .black(0x45, 0x47, 0x5a)
            .red(0xf3, 0x8b, 0xa8)
            .green(0xa6, 0xe3, 0xa1)
            .yellow(0xf9, 0xe2, 0xaf)
            .blue(0x89, 0xb4, 0xfa)
            .magenta(0xf5, 0xc2, 0xe7)
            .cyan(0x94, 0xe2, 0xd5)
            .white(0xba, 0xc2, 0xde)
            .bright_black(0x58, 0x5b, 0x70)
            .bright_red(0xf3, 0x8b, 0xa8)
            .bright_green(0xa6, 0xe3, 0xa1)
            .bright_yellow(0xf9, 0xe2, 0xaf)
            .bright_blue(0x89, 0xb4, 0xfa)
            .bright_magenta(0xf5, 0xc2, 0xe7)
            .bright_cyan(0x94, 0xe2, 0xd5)
            .bright_white(0xa6, 0xad, 0xc8)
            .build();
        let config = TerminalConfig {
            font_family: "Cascadia Mono".into(),
            font_size: px(14.0),
            cols: 88,
            rows: 26,
            scrollback: 5000,
            line_height_multiplier: 1.05,
            padding: Edges::all(px(12.0)),
            colors,
        };
        cx.new(|cx| {
            TerminalView::new(writer, reader, config, cx).with_resize_callback(move |cols, rows| {
                let _ = master.lock().resize(PtySize {
                    cols: cols as u16,
                    rows: rows as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            })
        })
    }

    fn add_connection(&mut self, _: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().trim().to_owned();
        let host = self.host_input.read(cx).value().trim().to_owned();
        let user = self.user_input.read(cx).value().trim().to_owned();
        let port_text = self.port_input.read(cx).value();
        let port = port_text.trim().parse::<u16>().ok();

        if name.is_empty() || host.is_empty() || user.is_empty() || port.map(|value| value == 0).unwrap_or(true) {
            self.error = Some("Enter a name, host, username, and port from 1 to 65535.".into());
            cx.notify();
            return;
        }

        let connection = SavedConnection {
            id: format!("ssh-{}", self.connections.len() + 1),
            name,
            host,
            user,
            port: port.unwrap_or(22),
            local: false,
        };
        self.connections.push(connection);
        self.error = None;
        cx.notify();
    }

    fn select_connection(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(connection) = self.connections.get(index).cloned() else { return };
        let session = if connection.local {
            spawn_process("powershell.exe", &["-NoLogo"])
        } else {
            let destination = format!("{}@{}", connection.user, connection.host);
            spawn_process("ssh", &["-tt", "-p", &connection.port.to_string(), &destination])
        };

        match session {
            Ok(session) => {
                let _ = self.child.kill();
                self.terminal = Self::terminal(cx, session.reader, session.writer, session.master);
                self.child = session.child;
                self.selected_id = connection.id;
                self.selected_label = connection.name;
                self.error = None;
                self.terminal.read(cx).focus_handle().focus(window);
            }
            Err(error) => self.error = Some(format!("Could not start session: {error:#}")),
        }
        cx.notify();
    }

    fn connection_button(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let connection = &self.connections[index];
        let active = connection.id == self.selected_id;
        let name = connection.name.clone();
        let host = if connection.local {
            "Local shell".to_owned()
        } else {
            format!("{}@{}:{}", connection.user, connection.host, connection.port)
        };
        let row = div()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(5.0))
            .bg(if active { rgb(RAISED) } else { rgb(SURFACE) })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w(px(7.0))
                            .h(px(7.0))
                            .rounded(px(4.0))
                            .bg(if active { rgb(SUCCESS) } else { rgb(MUTED) }),
                    )
                    .child(div().text_size(px(12.0)).text_color(rgb(TEXT)).child(name)),
            )
            .child(
                div()
                    .pl(px(15.0))
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child(host),
            );
        Button::new(("connection", index as u32))
            .ghost()
            .w_full()
            .child(row)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_connection(index, window, cx)
            }))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let visible = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, connection)| {
                query.is_empty()
                    || connection.name.to_lowercase().contains(&query)
                    || connection.host.to_lowercase().contains(&query)
                    || connection.user.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let local_index = visible.iter().copied().find(|&index| self.connections[index].local);
        let ssh_indices = visible
            .into_iter()
            .filter(|&index| !self.connections[index].local)
            .collect::<Vec<_>>();

        div()
            .w(px(292.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(14.0))
            .bg(rgb(SURFACE))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(14.0)).font_weight(gpui::FontWeight::SEMIBOLD).child("Connections"))
                    .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child(format!("{} SSH", self.connections.len() - 1))),
            )
            .child(Input::new(&self.search).w_full())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(div().px(px(4.0)).text_size(px(10.0)).text_color(rgb(MUTED)).child("LOCAL"))
                    .when_some(local_index, |element, index| element.child(self.connection_button(index, cx)))
                    .when(local_index.is_none(), |element| {
                        element.child(div().px(px(4.0)).text_size(px(11.0)).text_color(rgb(MUTED)).child("No matching connection"))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(div().px(px(4.0)).text_size(px(10.0)).text_color(rgb(MUTED)).child("SAVED SSH"))
                    .children(ssh_indices.into_iter().map(|index| self.connection_button(index, cx))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
                    .p(px(10.0))
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
    }
}

impl Render for PrototypeApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let terminal_label = self.selected_label.clone();
        let error = self.error.clone();
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
                    .px(px(16.0))
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
                    .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child("single session")),
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
                            .bg(rgb(APP_BG))
                            .child(
                                div()
                                    .h(px(36.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px(px(12.0))
                                    .bg(rgb(SURFACE))
                                    .border_b_1()
                                    .border_color(rgb(BORDER))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(div().w(px(7.0)).h(px(7.0)).rounded(px(4.0)).bg(rgb(ACCENT)))
                                            .child(div().text_size(px(12.0)).child(terminal_label)),
                                    )
                                    .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child("OpenSSH / ConPTY")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p(px(8.0))
                                    .child(self.terminal.clone()),
                            )
                            .when_some(error, |element, error| {
                                element.child(
                                    div()
                                        .px(px(12.0))
                                        .py(px(8.0))
                                        .text_size(px(12.0))
                                        .text_color(rgb(DANGER))
                                        .child(error),
                                )
                            }),
                    ),
            )
    }
}

impl Drop for PrototypeApp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn spawn_process(program: &str, args: &[&str]) -> Result<ProcessSession> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize { rows: 26, cols: 88, pixel_width: 0, pixel_height: 0 })
        .context("opening a terminal session")?;

    let mut command = CommandBuilder::new(program);
    for arg in args {
        command.arg(arg);
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");

    let child = pair.slave.spawn_command(command).context("starting the process")?;
    let writer = pair.master.take_writer().context("opening terminal input")?;
    let reader = pair.master.try_clone_reader().context("opening terminal output")?;
    let master = Arc::new(Mutex::new(pair.master));
    drop(pair.slave);

    Ok(ProcessSession { reader, writer, master, child })
}

fn main() -> Result<()> {
    let initial_session = spawn_process("powershell.exe", &["-NoLogo"])?;
    let app = Application::new();
    app.run(move |cx| {
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
                move |window, cx| {
                    let root = cx.new(|cx| PrototypeApp::new(window, cx, initial_session));
                    cx.new(|cx| Root::new(root, window, cx))
                },
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
    Ok(())
}
