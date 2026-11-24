//! The main window: title bar, tree sidebar, properties pane, vertical toolbar (rail), pane area
//! and command console, in the same global layout as the production app (see `docs/gpui-port.md`).

use std::{collections::HashSet, rc::Rc};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, TitleBar, h_flex, v_flex,
    button::{Button, ButtonVariants as _},
    dock::{DockArea, DockLayout, DockPlacement, DockSkin, PanelId, panel_handle},
    input::{Input, InputEvent, InputState},
    list::ListItem,
    resizable::{h_resizable, resizable_panel, v_resizable},
    theme::{Theme, ThemeRegistry},
    tree::{TreeItem, TreeState, tree},
};
use gpui_kit::{AnyElement, Context, Entity, IntoElement, Render, SharedString, Subscription, Window, base::StyledExt, div, prelude::*, px};

use crate::console::CommandConsole;
use crate::layout::{LayoutPreset, distribute};
use crate::model::{
    ConnectionDetails, DropTarget, NodeKind, TreeNode, filter_tree_nodes, find_node, folder_path, move_node, sample_tree,
};
use crate::panes::TerminalPanel;

pub const THEME_NAMES: [&str; 13] = [
    "Tokyo Night", "Tokyo Storm", "Tokyo Moon", "Ayu Light", "Ayu Dark", "Catppuccin Latte", "Catppuccin Frappe",
    "Catppuccin Macchiato", "Catppuccin Mocha", "Gruvbox Light", "Gruvbox Dark", "Solarized Light", "Solarized Dark",
];

// Sizes from the production layout (docs/gpui-port.md).
const TREE_WIDTH: f32 = 280.0;
const PROPERTIES_WIDTH: f32 = 320.0;
const RAIL_WIDTH: f32 = 48.0;
const CONSOLE_HEIGHT: f32 = 160.0;

#[derive(Clone, Copy)]
struct DragPayload {
    id: u64,
}

impl Render for DragPayload {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().foreground)
            .rounded_md()
            .child(format!("Moving connection #{}", self.id))
    }
}

/// The read-only fields of the properties pane, one `Input` each.
struct PropertyFields {
    rows: Vec<(&'static str, Entity<InputState>)>,
}

impl PropertyFields {
    fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let labels = ["Name", "Folder", "Host", "Port", "Username", "Auth method", "SSH key", "Jump host"];
        Self { rows: labels.into_iter().map(|label| (label, cx.new(|cx| InputState::new(window, cx)))).collect() }
    }

    fn show(&self, node: Option<(&TreeNode, String)>, window: &mut Window, cx: &mut Context<Workspace>) {
        let values: Vec<String> = match node {
            Some((node, folder)) => {
                let ConnectionDetails { host, port, user, auth, key, jump } = node.details.clone();
                vec![node.name.clone(), folder, host, port.to_string(), user, auth, key, jump]
            }
            None => vec![String::new(); self.rows.len()],
        };
        for ((_, state), value) in self.rows.iter().zip(values) {
            state.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }
}

pub struct Workspace {
    nodes: Vec<TreeNode>,
    expanded: HashSet<u64>,
    tree_state: Entity<TreeState>,
    search: Entity<InputState>,
    _search_subscription: Subscription,
    search_query: String,
    selected: Option<u64>,
    properties: PropertyFields,
    // Panel visibility (the rail toggles these).
    tree_visible: bool,
    properties_visible: bool,
    console_visible: bool,
    // Pane area.
    dock_area: Entity<DockArea>,
    _dock_skin: Rc<DockSkin>,
    terminals: Vec<Entity<TerminalPanel>>,
    preset: LayoutPreset,
    console: Entity<CommandConsole>,
    active_theme: usize,
    status: String,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nodes = sample_tree();
        let expanded: HashSet<u64> = [1, 4, 7].into_iter().collect();
        let tree_state = cx.new(|cx| TreeState::new(cx).items(build_tree_items(&nodes, &expanded)));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search connections…"));
        let search_subscription = cx.subscribe(&search, |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_query = state.read(cx).value().to_string();
                this.refresh_tree(cx);
                cx.notify();
            }
        });
        let (dock_area, dock_skin) = DockSkin::dock_area("sshterm-dock", Some(1), window, cx);
        let console = cx.new(|cx| CommandConsole::new(window, cx));
        let properties = PropertyFields::new(window, cx);
        Self {
            nodes,
            expanded,
            tree_state,
            search,
            _search_subscription: search_subscription,
            search_query: String::new(),
            selected: None,
            properties,
            tree_visible: true,
            properties_visible: true,
            console_visible: true,
            dock_area,
            _dock_skin: dock_skin,
            terminals: Vec::new(),
            preset: LayoutPreset::Single,
            console,
            active_theme: 0,
            status: "Click a connection to see its properties; double-click to open a tab.".into(),
        }
    }

    // --- tree -------------------------------------------------------------------------------

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

    fn select_connection(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = find_node(&self.nodes, id) else { return };
        self.selected = Some(id);
        if node.kind == NodeKind::Connection {
            let folder = folder_path(&self.nodes, id).map(|path| path.join(" / ")).unwrap_or_default();
            let node = node.clone();
            self.properties.show(Some((&node, folder)), window, cx);
        } else {
            self.properties.show(None, window, cx);
        }
        cx.notify();
    }

    fn move_item(&mut self, source: u64, target: DropTarget, cx: &mut Context<Self>) {
        self.status = match move_node(&mut self.nodes, &mut self.expanded, source, target) {
            Ok(message) | Err(message) => message.to_string(),
        };
        self.refresh_tree(cx);
        cx.notify();
    }

    // --- panes ------------------------------------------------------------------------------

    fn is_open(&self, terminal: &Entity<TerminalPanel>, cx: &Context<Self>) -> bool {
        self.dock_area.read(cx).panel(PanelId::from(terminal.entity_id())).is_some()
    }

    fn open_connection(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = find_node(&self.nodes, id).filter(|node| node.kind == NodeKind::Connection) else { return };
        let (name, endpoint) = (node.name.clone(), format!("{}@{}:{}", node.details.user, node.details.host, node.details.port));
        let existing = self.terminals.iter().find(|t| t.read(cx).connection_id == id && self.is_open(t, cx)).cloned();
        if let Some(terminal) = existing {
            let panel_id = PanelId::from(terminal.entity_id());
            self.dock_area.update(cx, |area, cx| area.select_panel(panel_id, window, cx));
        } else {
            let terminal = cx.new(|cx| TerminalPanel::new(id, &name, endpoint, cx));
            self.terminals.push(terminal.clone());
            self.dock_area.update(cx, |area, cx| {
                area.add_panel_view(panel_handle(terminal), DockPlacement::Center, None, window, cx);
            });
        }
        self.status = format!("{name}: SSH transport is not connected in this prototype.");
        cx.notify();
    }

    /// Re-homes every open tab into the groups of the chosen preset.
    fn apply_layout(&mut self, preset: LayoutPreset, window: &mut Window, cx: &mut Context<Self>) {
        self.preset = preset;
        let live: Vec<Entity<TerminalPanel>> = self.terminals.iter().filter(|t| self.is_open(t, cx)).cloned().collect();
        self.terminals = live.clone();
        let plan = distribute(live.len(), preset);
        if plan.is_empty() {
            cx.notify();
            return;
        }
        let group = |indices: &[usize], cx: &Context<Self>| {
            let mut tabs = DockLayout::tabs();
            for &index in indices {
                tabs = tabs.panel_view(panel_handle(live[index].clone()), cx);
            }
            tabs
        };
        let layout = match plan.as_slice() {
            [only] => group(only, cx),
            [a, b] if preset == LayoutPreset::TopBottom => {
                DockLayout::v_split().child(group(a, cx), None).child(group(b, cx), None)
            }
            [a, b] => DockLayout::h_split().child(group(a, cx), None).child(group(b, cx), None),
            [a, b, c] => DockLayout::v_split()
                .child(DockLayout::h_split().child(group(a, cx), None).child(group(b, cx), None), None)
                .child(group(c, cx), None),
            [a, b, c, d, ..] => DockLayout::v_split()
                .child(DockLayout::h_split().child(group(a, cx), None).child(group(b, cx), None), None)
                .child(DockLayout::h_split().child(group(c, cx), None).child(group(d, cx), None), None),
            [] => return,
        };
        self.dock_area.update(cx, |area, cx| area.set_center(layout, window, cx));
        cx.notify();
    }

    // --- theme ------------------------------------------------------------------------------

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

    // --- rendering --------------------------------------------------------------------------

    fn render_title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme_name = THEME_NAMES[self.active_theme];
        TitleBar::new().h(px(38.0)).bg(cx.theme().secondary).border_b_1().border_color(cx.theme().border).child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .px_3()
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::Cpu).small())
                        .child(div().font_semibold().child("SSHTerm"))
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child("GPUI prototype")),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("tunnels")
                                .small()
                                .ghost()
                                .icon(IconName::Network)
                                .label("0 tunnels")
                                .tooltip("Tunnels (not connected in this prototype)"),
                        )
                        .child(
                            Button::new("cycle-theme")
                                .small()
                                .ghost()
                                .icon(IconName::Palette)
                                .label(theme_name)
                                .tooltip("Change theme")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let next = (this.active_theme + 1) % THEME_NAMES.len();
                                    this.apply_theme(next, cx);
                                })),
                        )
                        .child(Button::new("settings").small().ghost().icon(IconName::Settings).tooltip("Settings"))
                        .child(
                            Button::new("lock")
                                .small()
                                .ghost()
                                .icon(Icon::new(gpui_kit::assets::IconName::Lock))
                                .tooltip("Lock the vault"),
                        ),
                ),
        )
            .into_any_element()
    }

    fn render_tree_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
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
                    return ListItem::new(ix).pl(px(12.0 + depth as f32 * 16.0)).child(h_flex().gap_2().child("·").child(label));
                };
                view.update(app, move |_this, cx| {
                    let (icon, icon_color) = if is_folder {
                        (if is_expanded { IconName::FolderOpen } else { IconName::FolderClosed }, cx.theme().warning)
                    } else {
                        (IconName::Cpu, cx.theme().primary)
                    };
                    let row_label = label.clone();
                    ListItem::new(ix)
                        .pl(px(12.0 + depth as f32 * 16.0))
                        .selected(selected)
                        .accessibility_label(row_label.clone())
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .text_color(cx.theme().foreground)
                                .child(Icon::new(icon).small().text_color(icon_color))
                                .child(row_label),
                        )
                        .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                            this.select_connection(node_id, window, cx);
                            if event.click_count() >= 2 {
                                this.open_connection(node_id, window, cx);
                            }
                        }))
                        .on_drag(DragPayload { id: node_id }, |drag: &DragPayload, _window, _app, cx| cx.new(|_| *drag))
                        .on_drop(cx.listener(move |this, drag: &DragPayload, _, cx| {
                            this.move_item(drag.id, DropTarget::Node(node_id), cx);
                        }))
                })
            }
        });

        v_flex()
            .size_full()
            .gap_2()
            .p_3()
            .bg(cx.theme().secondary)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child("CONNECTIONS"))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("DRAG TO MOVE")),
            )
            .child(Input::new(&self.search).small().w_full())
            .child(div().flex_1().min_h(px(0.0)).child(connections_tree))
            .child(
                div()
                    .id("root-drop-target")
                    .w_full()
                    .min_h(px(36.0))
                    .px_3()
                    .py_2()
                    .border_1()
                    .border_dashed()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Drop here to move to root")
                    .on_drop(cx.listener(|this, drag: &DragPayload, _, cx| this.move_item(drag.id, DropTarget::Root, cx))),
            )
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(self.status.clone()))
            .into_any_element()
    }

    fn render_properties_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let hint = if self.selected.is_none() { "Select a connection to see its properties." } else { "" };
        v_flex()
            .size_full()
            .gap_2()
            .p_3()
            .bg(cx.theme().secondary)
            .border_l_1()
            .border_color(cx.theme().border)
            .child(div().text_sm().font_semibold().child("PROPERTIES"))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(hint))
            .children(self.properties.rows.iter().map(|(label, state)| {
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(96.0)).flex_shrink_0().text_xs().text_color(cx.theme().muted_foreground).child(*label))
                    .child(div().flex_1().min_w(px(0.0)).child(Input::new(state).small().disabled(true)))
            }))
            .into_any_element()
    }

    fn render_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::assets::IconName as Lucide;
        let separator = |cx: &mut Context<Self>| div().w(px(24.0)).h(px(1.0)).my_1().bg(cx.theme().border);
        let preset_button = |id: &'static str, icon: Lucide, tip: &'static str, preset: LayoutPreset, cx: &mut Context<Self>| {
            Button::new(id)
                .small()
                .ghost()
                .icon(Icon::new(icon))
                .tooltip(tip)
                .selected(self.preset == preset)
                .on_click(cx.listener(move |this, _, window, cx| this.apply_layout(preset, window, cx)))
        };
        v_flex()
            .size_full()
            .items_center()
            .py_2()
            .gap_1()
            .bg(cx.theme().secondary)
            .border_x_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("toggle-tree")
                    .small()
                    .ghost()
                    .icon(Icon::new(Lucide::PanelLeft))
                    .tooltip("Toggle connection tree")
                    .selected(self.tree_visible)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tree_visible = !this.tree_visible;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("toggle-properties")
                    .small()
                    .ghost()
                    .icon(Icon::new(Lucide::PanelRight))
                    .tooltip("Toggle properties")
                    .selected(self.properties_visible)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.properties_visible = !this.properties_visible;
                        cx.notify();
                    })),
            )
            .child(separator(cx))
            .child(
                Button::new("new-tab")
                    .small()
                    .ghost()
                    .icon(Icon::new(Lucide::Plus))
                    .tooltip("New tab: open the selected connection")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(id) = this.selected {
                            this.open_connection(id, window, cx);
                        }
                    })),
            )
            .child(preset_button("layout-single", Lucide::Square, "Layout: single pane", LayoutPreset::Single, cx))
            .child(preset_button("layout-left-right", Lucide::Columns2, "Layout: left / right", LayoutPreset::LeftRight, cx))
            .child(preset_button("layout-top-bottom", Lucide::Rows2, "Layout: top / bottom", LayoutPreset::TopBottom, cx))
            .child(preset_button("layout-grid", Lucide::LayoutGrid, "Layout: grid", LayoutPreset::Grid, cx))
            .child(separator(cx))
            .child(preset_button("split-left-right", Lucide::SquareSplitHorizontal, "Split left / right", LayoutPreset::LeftRight, cx))
            .child(preset_button("split-top-bottom", Lucide::SquareSplitVertical, "Split top / bottom", LayoutPreset::TopBottom, cx))
            .child(div().flex_1())
            .child(
                Button::new("toggle-console")
                    .small()
                    .ghost()
                    .icon(Icon::new(Lucide::PanelBottom))
                    .tooltip(if self.console_visible { "Hide command console" } else { "Show command console" })
                    .selected(self.console_visible)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.console_visible = !this.console_visible;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tree_pane = self.render_tree_pane(cx);
        let properties_pane = self.render_properties_pane(cx);
        let rail = self.render_rail(cx);

        // Main area: pane area over the command console.
        let main_area = v_resizable("main-area")
            .child(resizable_panel().child(self.dock_area.clone()))
            .child(
                resizable_panel()
                    .visible(self.console_visible)
                    .size(px(CONSOLE_HEIGHT))
                    .size_range(px(100.0)..px(400.0))
                    .child(self.console.clone()),
            );

        // Left to right: tree, properties, rail (fixed), main area.
        let shell = h_resizable("shell")
            .child(
                resizable_panel()
                    .visible(self.tree_visible)
                    .size(px(TREE_WIDTH))
                    .size_range(px(200.0)..px(400.0))
                    .child(tree_pane),
            )
            .child(
                resizable_panel()
                    .visible(self.properties_visible)
                    .size(px(PROPERTIES_WIDTH))
                    .size_range(px(250.0)..px(450.0))
                    .child(properties_pane),
            )
            .child(resizable_panel().size(px(RAIL_WIDTH)).size_range(px(RAIL_WIDTH)..px(RAIL_WIDTH)).fixed().child(rail))
            .child(resizable_panel().child(main_area));

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h(px(0.0)).child(shell))
    }
}

pub fn build_tree_items(nodes: &[TreeNode], expanded: &HashSet<u64>) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|node| {
            let children = if node.kind == NodeKind::Folder {
                if node.children.is_empty() {
                    // The Kit tree infers folders from children; keep emptied folders droppable.
                    vec![TreeItem::new(format!("empty-{}", node.id), "Empty folder").disabled(true)]
                } else {
                    build_tree_items(&node.children, expanded)
                }
            } else {
                Vec::new()
            };
            TreeItem::new(node.id.to_string(), node.name.clone()).children(children).expanded(expanded.contains(&node.id))
        })
        .collect()
}

