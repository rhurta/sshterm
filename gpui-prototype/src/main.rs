//! GPUI Kit tree component experiment with application-owned drag/drop rules.
use std::collections::HashSet;

use gpui_kit::{
    App, Context, Render, Window, WindowOptions, div, prelude::*, px,
    component::{ListItem, h_flex, tree::{tree, TreeItem, TreeState}},
    *,
};

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
            .bg(gpui_kit::rgb(0x34445d))
            .text_color(gpui_kit::rgb(0xffffff))
            .rounded_md()
            .child(format!("Moving connection #{}", self.id))
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
    tree_state: gpui_kit::Entity<TreeState>,
    status: String,
}

impl SshTermPrototype {
    fn new(cx: &mut App) -> Self {
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
        let tree_items = build_tree_items(&nodes, &expanded);
        let tree_state = cx.new(|cx| TreeState::new(cx).items(tree_items));

        Self {
            nodes,
            expanded,
            tree_state,
            status: "Drag an item onto a folder or another row to move it.".into(),
        }
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let items = build_tree_items(&self.nodes, &self.expanded);
        self.tree_state.update(cx, |state, cx| state.set_items(items, cx));
    }

    fn move_node(&mut self, source_id: u64, target: DropTarget, cx: &mut Context<Self>) {
        if !contains_id(&self.nodes, source_id) {
            self.status = "Move rejected: source no longer exists.".into();
            cx.notify();
            return;
        }

        if let DropTarget::Node(target_id) = target {
            if source_id == target_id || subtree_contains(&self.nodes, source_id, target_id) {
                self.status = "Move rejected: a folder cannot be dropped into itself or its descendants.".into();
                cx.notify();
                return;
            }
            if !contains_id(&self.nodes, target_id) {
                self.status = "Move rejected: target no longer exists.".into();
                cx.notify();
                return;
            }
        }

        let Some(moved) = take_node(&mut self.nodes, source_id) else {
            self.status = "Move rejected: source could not be removed.".into();
            cx.notify();
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

        self.refresh_tree(cx);
        cx.notify();
    }
}

fn build_tree_items(nodes: &[TreeNode], expanded: &HashSet<u64>) -> Vec<TreeItem> {
    nodes.iter().map(|node| {
        let children = if node.kind == NodeKind::Folder {
            if node.children.is_empty() {
                // TreeItem infers folders from their children, so retain a disabled
                // placeholder to keep an emptied folder a valid drop target.
                vec![TreeItem::new(format!("empty-{}", node.id), "Empty folder").disabled(true)]
            } else {
                build_tree_items(&node.children, expanded)
            }
        } else {
            Vec::new()
        };

        TreeItem::new(node.id.to_string(), node.name.clone())
            .children(children)
            .expanded(expanded.contains(&node.id))
    }).collect()
}

impl Render for SshTermPrototype {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    return ListItem::new(ix)
                        .pl(px(12.0 + depth as f32 * 16.0))
                        .child(h_flex().gap_2().child("·").child(label));
                };

                let row_view = view.clone();
                view.update(app, move |this, cx| {
                    let row_label = if is_folder {
                        format!("{}  {}", if is_expanded { "▾" } else { "▸" }, label)
                    } else {
                        format!("◦  {}", label)
                    };

                    ListItem::new(ix)
                        .pl(px(12.0 + depth as f32 * 16.0))
                        .selected(selected)
                        .accessibility_label(row_label.clone())
                        .child(h_flex().gap_2().child(row_label))
                        .on_drag(
                            DragPayload { id: node_id },
                            |drag: &DragPayload, _window, _app, cx| cx.new(|_| *drag),
                        )
                        .on_drop(cx.listener(move |this, drag: &DragPayload, _, cx| {
                            this.move_node(drag.id, DropTarget::Node(node_id), cx);
                        }))
                })
            }
        });

        let root_drop = div()
            .id("root-drop-target")
            .w_full()
            .min_h(px(52.0))
            .mt_2()
            .px_3()
            .py_3()
            .border_1()
            .border_dashed()
            .rounded_md()
            .text_sm()
            .child("Drop here to move to root")
            .on_drop(cx.listener(|this, drag: &DragPayload, _, cx| {
                this.move_node(drag.id, DropTarget::Root, cx);
            }));

        div()
            .size_full()
            .flex()
            .bg(gpui_kit::rgb(0x151922))
            .text_color(gpui_kit::rgb(0xe6eaf2))
            .child(
                div()
                    .w(px(310.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .bg(gpui_kit::rgb(0x1c222d))
                    .border_r_1()
                    .border_color(gpui_kit::rgb(0x303a49))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_sm().child("CONNECTIONS"))
                            .child(div().text_xs().child("DRAG TO MOVE")),
                    )
                    .child(div().flex_1().min_h(px(0.0)).child(connections_tree))
                    .child(root_drop)
                    .child(div().text_xs().child(self.status.clone())),
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
                            .bg(gpui_kit::rgb(0x202734))
                            .rounded_md()
                            .text_sm()
                            .child("Terminal  ·  Preview"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .p_4()
                            .bg(gpui_kit::rgb(0x0d1118))
                            .rounded_md()
                            .border_1()
                            .border_color(gpui_kit::rgb(0x303a49))
                            .text_sm()
                            .text_color(gpui_kit::rgb(0xb9c7d9))
                            .child("sshterm GPUI Kit prototype\n\nNo SSH process is attached yet.\nThe terminal pane is a placeholder.\n\n$ _"),
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
    application().run(|cx| {
        init(cx);
        open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| SshTermPrototype::new(cx))
        })
        .expect("open GPUI Kit prototype window");
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
