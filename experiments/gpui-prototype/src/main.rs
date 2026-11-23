//! GPUI Kit workspace prototype: searchable connection tree, terminal tabs and themes.
use std::{collections::HashSet, path::PathBuf};

use gpui_kit::{
    Context, Entity, Render, SharedString, Subscription, Window, base::StyledExt, div, prelude::*, px,
    component::{
        ActiveTheme as _, Icon, IconName,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::{InputEvent, InputState},
        list::ListItem,
        tab::{Tab, TabBar},
        theme::{Theme, ThemeRegistry},
        tree::{tree, TreeItem, TreeState},
        TitleBar,
    },
    component::Sizable,
    *,
};

const THEME_NAMES: [&str; 13] = ["Tokyo Night", "Tokyo Storm", "Tokyo Moon", "Ayu Light", "Ayu Dark", "Catppuccin Latte", "Catppuccin Frappe", "Catppuccin Macchiato", "Catppuccin Mocha", "Gruvbox Light", "Gruvbox Dark", "Solarized Light", "Solarized Dark"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeKind { Folder, Connection }

#[derive(Clone, Debug)]
struct TreeNode { id: u64, name: String, kind: NodeKind, children: Vec<TreeNode> }

impl TreeNode {
    fn folder(id: u64, name: &str, children: Vec<TreeNode>) -> Self {
        Self { id, name: name.into(), kind: NodeKind::Folder, children }
    }
    fn connection(id: u64, name: &str) -> Self {
        Self { id, name: name.into(), kind: NodeKind::Connection, children: Vec::new() }
    }
}

#[derive(Clone, Copy)]
struct DragPayload { id: u64 }

impl Render for DragPayload {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().px_3().py_2().bg(cx.theme().secondary).text_color(cx.theme().foreground)
            .rounded_md().child(format!("Moving connection #{}", self.id))
    }
}

#[derive(Clone, Copy)]
enum DropTarget { Root, Node(u64) }

#[derive(Clone)]
struct TerminalTab { connection_id: u64, label: String }

struct SshTermPrototype {
    nodes: Vec<TreeNode>,
    expanded: HashSet<u64>,
    tree_state: Entity<TreeState>,
    search: Entity<InputState>,
    _search_subscription: Subscription,
    search_query: String,
    tabs: Vec<TerminalTab>,
    active_tab: usize,
    split_enabled: bool,
    split_tab: Option<usize>,
    show_tree: bool,
    show_properties: bool,
    console_open: bool,
    command_input: Entity<InputState>,
    command_status: Option<String>,
    active_theme: usize,
    status: String,
}

impl SshTermPrototype {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nodes = vec![
            TreeNode::folder(1, "Production", vec![
                TreeNode::connection(2, "api-prod-01"),
                TreeNode::connection(3, "db-prod-01"),
                TreeNode::folder(4, "Bastions", vec![
                    TreeNode::connection(5, "bastion-east"),
                    TreeNode::connection(6, "bastion-west"),
                ]),
            ]),
            TreeNode::folder(7, "Development", vec![
                TreeNode::connection(8, "dev-api"),
                TreeNode::connection(9, "dev-db"),
            ]),
            TreeNode::connection(10, "localhost"),
        ];
        let expanded = [1, 4, 7].into_iter().collect::<HashSet<_>>();
        let tree_state = cx.new(|cx| TreeState::new(cx).items(build_tree_items(&nodes, &expanded)));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search connections…"));
        let search_subscription = cx.subscribe(&search, |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_query = state.read(cx).value().to_string();
                this.refresh_tree(cx);
                cx.notify();
            }
        });
        let command_input = cx.new(|cx| InputState::new(window, cx).placeholder("Type a command…"));
        cx.subscribe(&command_input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) { cx.notify(); }
        }).detach();
        Self {
            nodes, expanded, tree_state, search, _search_subscription: search_subscription,
            search_query: String::new(),
            tabs: vec![
                TerminalTab { connection_id: 10, label: "localhost · SSH".into() },
                TerminalTab { connection_id: 10, label: "localhost · SFTP".into() },
            ],
            active_tab: 0,
            split_enabled: true,
            split_tab: None,
            show_tree: true,
            show_properties: true,
            console_open: true,
            command_input,
            command_status: None,
            active_theme: 0,
            status: "Select a connection to open a session tab. Drag rows to organize connections.".into(),
        }
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let mut expanded = self.expanded.clone();
        let filtered = if self.search_query.trim().is_empty() {
            self.nodes.clone()
        } else {
            filter_tree_nodes(&self.nodes, self.search_query.trim(), &mut expanded)
        };
        let items = build_tree_items(&filtered, &expanded);
        self.tree_state.update(cx, |state, cx| state.set_items(items, cx));
    }

    fn open_connection(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(node) = find_node(&self.nodes, id) {
            if node.kind != NodeKind::Connection { return; }
            if let Some(index) = self.tabs.iter().position(|tab| tab.connection_id == id) {
                self.active_tab = index;
            } else {
                self.tabs.push(TerminalTab { connection_id: id, label: node.name.clone() });
                self.active_tab = self.tabs.len() - 1;
            }
            self.status = format!("{} selected — SSH transport is not connected in this prototype.", node.name);
            cx.notify();
        }
    }

    fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() { return; }
        self.tabs.remove(index);
        if self.tabs.is_empty() { self.active_tab = 0; }
        else if self.active_tab > index { self.active_tab -= 1; }
        else if self.active_tab >= self.tabs.len() { self.active_tab = self.tabs.len() - 1; }
        cx.notify();
    }

    fn send_command(&mut self, _: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let command = self.command_input.read(cx).value().trim().to_owned();
        self.command_status = Some(if command.is_empty() {
            "Enter a command first.".into()
        } else {
            "Command sending is pending while terminal I/O is being fixed.".into()
        });
        cx.notify();
    }

    fn apply_theme(&mut self, index: usize, cx: &mut Context<Self>) {
        let index = index % THEME_NAMES.len();
        let name: SharedString = THEME_NAMES[index].into();
        if let Some(theme) = ThemeRegistry::global(cx).themes().get(&name).cloned() {
            Theme::update(cx, |current| current.apply_config(&theme));
            self.active_theme = index;
            self.status = format!("Theme changed to {}.", THEME_NAMES[index]);
        } else {
            self.status = format!("Theme {} is not loaded. Check gpui-prototype/themes.", THEME_NAMES[index]);
        }
        cx.notify();
    }

    fn move_node(&mut self, source_id: u64, target: DropTarget, cx: &mut Context<Self>) {
        if !contains_id(&self.nodes, source_id) {
            self.status = "Move rejected: source no longer exists.".into(); cx.notify(); return;
        }
        if let DropTarget::Node(target_id) = target {
            if source_id == target_id || subtree_contains(&self.nodes, source_id, target_id) {
                self.status = "Move rejected: a folder cannot be dropped into itself or its descendants.".into();
                cx.notify(); return;
            }
            if !contains_id(&self.nodes, target_id) {
                self.status = "Move rejected: target no longer exists.".into(); cx.notify(); return;
            }
        }
        let Some(moved) = take_node(&mut self.nodes, source_id) else {
            self.status = "Move rejected: source could not be removed.".into(); cx.notify(); return;
        };
        match target {
            DropTarget::Root => { self.nodes.push(moved); self.status = "Moved item to the root.".into(); }
            DropTarget::Node(target_id) => {
                if let Some(folder) = find_node_mut(&mut self.nodes, target_id).filter(|n| n.kind == NodeKind::Folder) {
                    self.expanded.insert(target_id);
                    folder.children.push(moved);
                    self.status = "Moved item into folder.".into();
                } else if let Some(siblings) = find_siblings_mut(&mut self.nodes, target_id) {
                    let index = siblings.iter().position(|n| n.id == target_id).unwrap_or(siblings.len());
                    siblings.insert(index, moved);
                    self.status = "Reordered item before the target.".into();
                } else { self.status = "Move rejected: target parent was not found.".into(); }
            }
        }
        self.refresh_tree(cx);
        cx.notify();
    }
}

fn filter_tree_nodes(nodes: &[TreeNode], query: &str, expanded: &mut HashSet<u64>) -> Vec<TreeNode> {
    let query = query.to_lowercase();
    nodes.iter().filter_map(|node| {
        let children = filter_tree_nodes(&node.children, &query, expanded);
        let matches = node.name.to_lowercase().contains(&query);
        if matches || !children.is_empty() {
            if !children.is_empty() { expanded.insert(node.id); }
            Some(TreeNode {
                id: node.id, name: node.name.clone(), kind: node.kind,
                children: if matches { node.children.clone() } else { children },
            })
        } else { None }
    }).collect()
}

fn build_tree_items(nodes: &[TreeNode], expanded: &HashSet<u64>) -> Vec<TreeItem> {
    nodes.iter().map(|node| {
        let children = if node.kind == NodeKind::Folder {
            if node.children.is_empty() {
                vec![TreeItem::new(format!("empty-{}", node.id), "Empty folder").disabled(true)]
            } else { build_tree_items(&node.children, expanded) }
        } else { Vec::new() };
        TreeItem::new(node.id.to_string(), node.name.clone())
            .children(children).expanded(expanded.contains(&node.id))
    }).collect()
}

impl Render for SshTermPrototype {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let theme_name = THEME_NAMES[self.active_theme];
        let connections_tree = tree(&self.tree_state, {
            let view = view.clone();
            move |ix, entry, selected, _window, app| {
                let item = entry.item();
                let id = item.id.to_string();
                let depth = entry.depth();
                let is_folder = entry.is_folder();
                let is_expanded = entry.is_expanded();
                let label = item.label.clone();
                let Ok(node_id) = id.parse::<u64>() else {
                    return ListItem::new(ix).pl(px(12.0 + depth as f32 * 16.0))
                        .child(h_flex().gap_2().child("·").child(label));
                };
                view.update(app, move |_this, cx| {
                    let (icon, icon_color) = if is_folder {
                        (if is_expanded { IconName::FolderOpen } else { IconName::FolderClosed }, cx.theme().warning)
                    } else { (IconName::Cpu, cx.theme().primary) };
                    let row_label = label.clone();
                    ListItem::new(ix)
                        .pl(px(12.0 + depth as f32 * 16.0))
                        .selected(selected)
                        .accessibility_label(row_label.clone())
                        .child(h_flex().items_center().gap_2().text_color(cx.theme().foreground)
                            .child(Icon::new(icon).small().text_color(icon_color)).child(row_label))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_connection(node_id, cx)))
                        .on_drag(DragPayload { id: node_id }, |drag: &DragPayload, _window, _app, cx| cx.new(|_| *drag))
                        .on_drop(cx.listener(move |this, drag: &DragPayload, _, cx| {
                            this.move_node(drag.id, DropTarget::Node(node_id), cx);
                        }))
                })
            }
        });

        let tabs = TabBar::new("terminal-tabs").folder().selected_index(self.active_tab).menu(true)
            .on_click(cx.listener(|this, index, _, cx| { this.active_tab = *index; cx.notify(); }))
            .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                let close = Button::new(format!("close-tab-{index}")).ghost().xsmall().icon(IconName::Close)
                    .on_click(cx.listener(move |this, _, _, cx| this.close_tab(index, cx)));
                Tab::new().label(tab.label.clone()).suffix(close)
            }));

        let active_label = self.tabs.get(self.active_tab).map(|tab| tab.label.clone()).unwrap_or_else(|| "No session".into());
        let split_label = self.split_tab
            .and_then(|index| self.tabs.get(index))
            .map(|tab| tab.label.clone());
        let terminal_text = if self.tabs.is_empty() {
            "No session tab is open.\nSelect a connection from the tree to open one.".to_string()
        } else if active_label.ends_with("· SFTP") {
            "SFTP BROWSER\n\nlocalhost · connection pending\n\n/\n  home/\n  etc/\n  var/\n\nSFTP browsing is a layout prototype; no remote file session is attached.".to_string()
        } else {
            format!("sshterm GPUI Kit prototype\n\nSession: {active_label}\n\nTERMINAL I/O PENDING\nNo process is attached. This pane is a layout placeholder.")
        };

        div().size_full().flex().flex_col().bg(cx.theme().background).text_color(cx.theme().foreground)
            .child(TitleBar::new().h(px(38.0)).bg(cx.theme().secondary)
                .border_b_1().border_color(cx.theme().border)
                .child(h_flex().w_full().items_center().justify_between().px_3()
                    .child(h_flex().items_center().gap_2()
                        .child(Icon::new(IconName::Cpu).small())
                        .child(div().font_semibold().child("SSHTerm"))
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child("GPUI prototype")))
                    .child(h_flex().items_center().gap_2()
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Theme"))
                        .child(Button::new("cycle-theme").small().ghost().label(theme_name)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let next = (this.active_theme + 1) % THEME_NAMES.len();
                                this.apply_theme(next, cx);
                            })))
                        .child(Button::new("toggle-split").small().ghost()
                            .label(if self.split_enabled { "Close split" } else { "Split right" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.split_enabled = !this.split_enabled;
                                cx.notify();
                            })))
                        .child(Button::new("toggle-console").small().ghost()
                            .label(if self.console_open { "Hide commands" } else { "Show commands" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.console_open = !this.console_open;
                                cx.notify();
                            })))
                        .child(div().text_xs().text_color(cx.theme().warning).child("TERMINAL I/O PENDING")))),
            )
            .child(div().flex_1().min_h(px(0.0)).flex()
                .when(self.show_tree, |row| row.child(div().w(px(300.0)).h_full().flex().flex_col().gap_2().p_3()
                    .bg(cx.theme().secondary).border_r_1().border_color(cx.theme().border)
                    .child(h_flex().items_center().justify_between()
                        .child(div().text_sm().font_semibold().child("CONNECTIONS"))
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child("DRAG TO MOVE")))
                    .child(gpui_kit::component::input::Input::new(&self.search).small().w_full())
                    .child(div().flex_1().min_h(px(0.0)).child(connections_tree))
                    .child(div().id("root-drop-target").w_full().min_h(px(42.0)).mt_1().px_3().py_2()
                        .border_1().border_dashed().border_color(cx.theme().border).rounded_md()
                        .text_sm().text_color(cx.theme().muted_foreground).child("Drop here to move to root")
                        .on_drop(cx.listener(|this, drag: &DragPayload, _, cx| this.move_node(drag.id, DropTarget::Root, cx))))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(self.status.clone()))))
                .when(self.show_properties, |row| row.child(div().w(px(300.0)).h_full().flex().flex_col().gap_2().p_3()
                    .bg(cx.theme().secondary).border_r_1().border_color(cx.theme().border)
                    .child(h_flex().h(px(40.0)).items_center().px_3().border_b_1().border_color(cx.theme().border)
                        .child(div().text_sm().font_semibold().child("Connection properties")))
                    .child(gpui_kit::component::input::Input::new(&self.search).small().w_full())
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Selected connection"))
                    .child(div().text_sm().child("localhost"))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Host"))
                    .child(div().text_sm().child("127.0.0.1"))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Port"))
                    .child(div().text_sm().child("22"))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Authentication"))
                    .child(div().text_sm().child("Local prototype · no credentials loaded"))))
                .child(div().w(px(72.0)).h_full().flex().flex_col().items_center().gap_2().py_2()
                    .bg(cx.theme().secondary).border_r_1().border_color(cx.theme().border)
                    .child(Button::new("toggle-tree").small().ghost().label(if self.show_tree { "Hide tree" } else { "Show tree" })
                        .on_click(cx.listener(|this, _, _, cx| { this.show_tree = !this.show_tree; cx.notify(); })))
                    .child(Button::new("toggle-properties").small().ghost().label(if self.show_properties { "Hide info" } else { "Show info" })
                        .on_click(cx.listener(|this, _, _, cx| { this.show_properties = !this.show_properties; cx.notify(); })))
                    .child(div().flex_1())
                    .child(Button::new("toggle-console-toolbar").small().ghost().label(if self.console_open { "Hide cmd" } else { "Show cmd" })
                        .on_click(cx.listener(|this, _, _, cx| { this.console_open = !this.console_open; cx.notify(); }))))
                .child(div().flex_1().h_full().min_w(px(0.0)).flex().flex_col().p_3().gap_2()
                    .child(tabs)
                    .child(div().flex_1().w_full().min_h(px(0.0)).flex().gap_2()
                        .child(div().flex_1().min_w(px(0.0)).p_4().bg(cx.theme().background).rounded_md()
                            .border_1().border_color(cx.theme().border).text_sm().text_color(cx.theme().foreground)
                            .child(terminal_text.clone()))
                        .when(self.split_enabled, |row| {
                            row.child(div().w(px(1.0)).flex_shrink_0().bg(cx.theme().border))
                                .child(div().flex_1().min_w(px(0.0)).flex().flex_col().items_center().justify_center().gap_3()
                                    .p_4().bg(cx.theme().background).rounded_md()
                                    .border_1().border_color(cx.theme().border)
                                    .child(div().text_sm().font_semibold().child(split_label.clone().unwrap_or_else(|| "Empty split pane".into())))
                                    .child(div().text_sm().text_color(cx.theme().muted_foreground)
                                        .child(if split_label.is_some() {
                                            "Terminal I/O is pending in both panes."
                                        } else {
                                            "Choose a connection to place in this pane."
                                        }))
                                    .when(split_label.is_none(), |pane| {
                                        pane.child(Button::new("open-active-in-split").small().ghost()
                                            .label("Open active tab here")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.split_tab = Some(this.active_tab);
                                                cx.notify();
                                            })))
                                    })
                                )
                        }))
                    )
                    .when(self.console_open, |column| {
                        column.child(div().w_full().h(px(142.0)).flex_shrink_0().flex().flex_col()
                            .bg(cx.theme().secondary).rounded_md()
                            .border_1().border_color(cx.theme().border)
                            .child(h_flex().h(px(34.0)).flex_shrink_0().items_center().justify_between().px_3()
                                .border_b_1().border_color(cx.theme().border)
                                .child(div().text_sm().font_semibold().child("Commands"))
                                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Send to active tab")))
                            .child(h_flex().gap_2().p_2()
                                .child(gpui_kit::component::input::Input::new(&self.command_input).small().flex_1())
                                .child(Button::new("send-command").small().label("Send")
                                    .on_click(cx.listener(Self::send_command))))
                            .when_some(self.command_status.clone(), |panel, message| {
                                panel.child(div().px_3().text_xs().text_color(cx.theme().warning).child(message))
                            })
                            .when(self.command_status.is_none(), |panel| {
                                panel.child(div().px_3().text_xs().text_color(cx.theme().muted_foreground)
                                    .child("Command sending is pending while terminal I/O is being fixed."))
                            })
                        )
                    })
                )
    }
}

fn contains_id(nodes: &[TreeNode], id: u64) -> bool {
    nodes.iter().any(|n| n.id == id || contains_id(&n.children, id))
}
fn find_node(nodes: &[TreeNode], id: u64) -> Option<&TreeNode> {
    for node in nodes {
        if node.id == id { return Some(node); }
        if let Some(found) = find_node(&node.children, id) { return Some(found); }
    }
    None
}
fn subtree_contains(nodes: &[TreeNode], source_id: u64, target_id: u64) -> bool {
    fn find(nodes: &[TreeNode], id: u64) -> Option<&TreeNode> {
        for n in nodes {
            if n.id == id { return Some(n); }
            if let Some(found) = find(&n.children, id) { return Some(found); }
        }
        None
    }
    fn has(node: &TreeNode, id: u64) -> bool {
        node.id == id || node.children.iter().any(|child| has(child, id))
    }
    find(nodes, source_id).is_some_and(|source| has(source, target_id))
}
fn take_node(nodes: &mut Vec<TreeNode>, id: u64) -> Option<TreeNode> {
    if let Some(index) = nodes.iter().position(|n| n.id == id) { return Some(nodes.remove(index)); }
    for node in nodes {
        if let Some(found) = take_node(&mut node.children, id) { return Some(found); }
    }
    None
}
fn find_node_mut(nodes: &mut [TreeNode], id: u64) -> Option<&mut TreeNode> {
    for node in nodes {
        if node.id == id { return Some(node); }
        if let Some(found) = find_node_mut(&mut node.children, id) { return Some(found); }
    }
    None
}
fn find_siblings_mut(nodes: &mut Vec<TreeNode>, target_id: u64) -> Option<&mut Vec<TreeNode>> {
    if nodes.iter().any(|n| n.id == target_id) { return Some(nodes); }
    for node in nodes {
        if let Some(found) = find_siblings_mut(&mut node.children, target_id) { return Some(found); }
    }
    None
}

fn main() {
    application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        init(cx);
        let default_theme: SharedString = "Tokyo Night".into();
        if let Err(error) = ThemeRegistry::watch_dir(PathBuf::from("experiments/gpui-prototype/themes"), cx, move |cx| {
            if let Some(theme) = ThemeRegistry::global(cx).themes().get(&default_theme).cloned() {
                Theme::update(cx, |current| current.apply_config(&theme));
            }
        }) { eprintln!("Failed to load bundled themes: {error}"); }
        open_window(TitleBar::window_options(), cx, |window, cx| {
            cx.new(|cx| SshTermPrototype::new(window, cx))
        }).expect("open GPUI Kit prototype window");
    });
}

