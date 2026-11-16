//! Isolated GPUI UI experiment. It deliberately has no dependency on Tauri.
use std::collections::HashSet;

use gpui::{
    App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb,
    size,
};
use gpui_platform::application;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeKind {
    Folder,
    Connection,
}

#[derive(Clone, Debug)]
struct TreeNode {
    id: u64,
    name: String,
    kind: NodeKind,
    children: Vec<TreeNode>,
}

impl TreeNode {
    fn folder(id: u64, name: &str, children: Vec<TreeNode>) -> Self {
        Self { id, name: name.into(), kind: NodeKind::Folder, children }
    }

    fn connection(id: u64, name: &str) -> Self {
        Self { id, name: name.into(), kind: NodeKind::Connection, children: Vec::new() }
    }
}

#[derive(Clone, Copy)]
struct DragPayload {
    id: u64,
}

impl Render for DragPayload {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .bg(rgb(0x34445d))
            .border_1()
            .border_color(rgb(0x82b4f5))
            .rounded_md()
            .text_color(rgb(0xffffff))
            .child(format!("Moving item #{}", self.id))
    }
}

#[derive(Clone, Copy)]
enum DropTarget {
    Root,
    Node(u64),
}

struct SshTermPrototype {
    nodes: Vec<TreeNode>,
    expanded: HashSet<u64>,
    selected: Option<u64>,
    status: String,
}

impl SshTermPrototype {
    fn new() -> Self {
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
        Self {
            nodes,
            expanded: [1, 4, 7].into_iter().collect(),
            selected: Some(2),
            status: "Drag a connection onto a folder or another connection.".into(),
        }
    }

    fn move_node(&mut self, source_id: u64, target: DropTarget) {
        if !contains_id(&self.nodes, source_id) {
            self.status = "Move rejected: source no longer exists.".into();
            return;
        }

        if let DropTarget::Node(target_id) = target {
            if source_id == target_id || subtree_contains(&self.nodes, source_id, target_id) {
                self.status = "Move rejected: a folder cannot be dropped into itself or its descendants.".into();
                return;
            }
            if !contains_id(&self.nodes, target_id) {
                self.status = "Move rejected: target no longer exists.".into();
                return;
            }
        }

        let Some(moved) = take_node(&mut self.nodes, source_id) else {
            self.status = "Move rejected: source could not be removed.".into();
            return;
        };

        match target {
            DropTarget::Root => {
                self.nodes.push(moved);
                self.status = "Moved item to the root.".into();
            }
            DropTarget::Node(target_id) => {
                if let Some(folder) = find_node_mut(&mut self.nodes, target_id)
                    .filter(|node| node.kind == NodeKind::Folder)
                {
                    self.expanded.insert(target_id);
                    folder.children.push(moved);
                    self.status = "Moved item into folder.".into();
                } else if let Some(siblings) = find_siblings_mut(&mut self.nodes, target_id) {
                    let index = siblings.iter().position(|node| node.id == target_id).unwrap_or(siblings.len());
                    siblings.insert(index, moved);
                    self.status = "Reordered item before the target.".into();
                } else {
                    self.status = "Move rejected: target parent was not found.".into();
                }
            }
        }
    }

    fn render_node(&self, node: &TreeNode, depth: usize, cx: &mut Context<Self>) -> gpui::Div {
        let id = node.id;
        let kind = node.kind;
        let is_open = self.expanded.contains(&id);
        let label = node.name.clone();
        let payload = DragPayload { id };

        let row = div()
            .id(("tree-node", id))
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_2()
            .pl(px(10.0 + depth as f32 * 15.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(0x2a3342))
            .hover(|style| style.bg(rgb(0x252e3d)).border_color(rgb(0x6588b7)))
            .cursor_move()
            .text_sm()
            .text_color(rgb(0xdce4f0))
            .child(if kind == NodeKind::Folder {
                format!("{}  ▸  {}", if is_open { "▾" } else { "▸" }, label)
            } else {
                format!("   ◦  {}", label)
            })
            .when(self.selected == Some(id), |style| {
                style.bg(rgb(0x293a52)).border_color(rgb(0x82b4f5))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(id);
                if kind == NodeKind::Folder {
                    if !this.expanded.insert(id) {
                        this.expanded.remove(&id);
                    }
                }
                cx.notify();
            }))
            .on_drag(payload, |drag: &DragPayload, _, _, cx| {
                cx.new(|_| *drag)
            })
            .on_drop(cx.listener(move |this, drag: &DragPayload, _, cx| {
                this.move_node(drag.id, DropTarget::Node(id));
                cx.notify();
            }));

        let mut block = div().flex().flex_col().w_full().child(row);
        if kind == NodeKind::Folder && is_open {
            block = block.children(node.children.iter().map(|child| self.render_node(child, depth + 1, cx)));
        }
        block
    }
}

impl Render for SshTermPrototype {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tree = div()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .children(self.nodes.iter().map(|node| self.render_node(node, 0, cx)));

        let root_drop = div()
            .id("root-drop-target")
            .w_full()
            .min_h(px(54.0))
            .mt_2()
            .px_3()
            .py_3()
            .border_1()
            .border_dashed()
            .border_color(rgb(0x53647c))
            .rounded_md()
            .hover(|style| style.bg(rgb(0x26364a)).border_color(rgb(0x82b4f5)))
            .text_sm()
            .text_color(rgb(0x9eacc0))
            .child("Drop here to move to root")
            .on_drop(cx.listener(|this, drag: &DragPayload, _, cx| {
                this.move_node(drag.id, DropTarget::Root);
                cx.notify();
            }));

        div()
            .size_full()
            .flex()
            .bg(rgb(0x151922))
            .text_color(rgb(0xe6eaf2))
            .child(
                div()
                    .w(px(292.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .bg(rgb(0x1c222d))
                    .border_r_1()
                    .border_color(rgb(0x303a49))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_sm().text_color(rgb(0xa9b8cc)).child("CONNECTIONS"))
                            .child(div().text_xs().text_color(rgb(0x718096)).child("DRAG TO MOVE")),
                    )
                    .child(tree)
                    .child(root_drop)
                    .child(div().text_xs().text_color(rgb(0x7e8da3)).child(self.status.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .p_3()
                    .gap_2()
                    .child(
                        div()
                            .h(px(36.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .bg(rgb(0x202734))
                            .rounded_md()
                            .text_sm()
                            .child("Terminal  ·  Preview"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .p_4()
                            .bg(rgb(0x0d1118))
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x303a49))
                            .text_sm()
                            .text_color(rgb(0xb9c7d9))
                            .child("sshterm GPUI prototype\n\nNo SSH process is attached yet.\nThis pane is intentionally a placeholder.\n\n$ _"),
                    ),
            )
    }
}

fn contains_id(nodes: &[TreeNode], id: u64) -> bool {
    nodes.iter().any(|node| node.id == id || contains_id(&node.children, id))
}

fn subtree_contains(nodes: &[TreeNode], source_id: u64, target_id: u64) -> bool {
    fn find(nodes: &[TreeNode], id: u64) -> Option<&TreeNode> {
        for node in nodes {
            if node.id == id {
                return Some(node);
            }
            if let Some(found) = find(&node.children, id) {
                return Some(found);
            }
        }
        None
    }
    fn has(node: &TreeNode, id: u64) -> bool {
        node.id == id || node.children.iter().any(|child| has(child, id))
    }
    find(nodes, source_id).is_some_and(|source| has(source, target_id))
}

fn take_node(nodes: &mut Vec<TreeNode>, id: u64) -> Option<TreeNode> {
    if let Some(index) = nodes.iter().position(|node| node.id == id) {
        return Some(nodes.remove(index));
    }
    for node in nodes {
        if let Some(found) = take_node(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

fn find_node_mut(nodes: &mut [TreeNode], id: u64) -> Option<&mut TreeNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

fn find_siblings_mut(nodes: &mut Vec<TreeNode>, target_id: u64) -> Option<&mut Vec<TreeNode>> {
    if nodes.iter().any(|node| node.id == target_id) {
        return Some(nodes);
    }
    for node in nodes {
        if let Some(found) = find_siblings_mut(&mut node.children, target_id) {
            return Some(found);
        }
    }
    None
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1120.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| SshTermPrototype::new()),
        )
        .expect("open GPUI prototype window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_moving_a_folder_into_its_descendant() {
        let tree = vec![TreeNode::folder(1, "A", vec![TreeNode::folder(2, "B", vec![])])];
        assert!(subtree_contains(&tree, 1, 2));
    }

    #[test]
    fn removes_a_node_from_a_nested_folder() {
        let mut tree = vec![TreeNode::folder(1, "A", vec![TreeNode::connection(2, "host")])];
        assert_eq!(take_node(&mut tree, 2).map(|node| node.name), Some("host".into()));
        assert!(!contains_id(&tree, 2));
    }
}
