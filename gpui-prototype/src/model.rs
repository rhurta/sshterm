//! Connection tree model: pure data and operations, no GPUI types.
//!
//! This mirrors the production tree (`src-tauri/src/store/tree.rs`). When the shared model crate
//! is extracted (see `docs/gpui-port.md`), this module is replaced by it.

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Folder,
    Connection,
}

/// What the properties pane shows for a connection (sample data in this prototype).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionDetails {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: String,
    pub key: String,
    pub jump: String,
}

#[derive(Clone, Debug)]
pub struct TreeNode {
    pub id: u64,
    pub name: String,
    pub kind: NodeKind,
    pub children: Vec<TreeNode>,
    pub details: ConnectionDetails,
}

impl TreeNode {
    pub fn folder(id: u64, name: &str, children: Vec<TreeNode>) -> Self {
        Self { id, name: name.into(), kind: NodeKind::Folder, children, details: ConnectionDetails::default() }
    }

    pub fn connection(id: u64, name: &str, host: &str, user: &str, auth: &str) -> Self {
        Self {
            id,
            name: name.into(),
            kind: NodeKind::Connection,
            children: Vec::new(),
            details: ConnectionDetails {
                host: host.into(),
                port: 22,
                user: user.into(),
                auth: auth.into(),
                key: if auth == "Key" { "~/.ssh/id_ed25519".into() } else { String::new() },
                jump: String::new(),
            },
        }
    }

    pub fn with_jump(mut self, jump: &str) -> Self {
        self.details.jump = jump.into();
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Root,
    Node(u64),
}

pub fn sample_tree() -> Vec<TreeNode> {
    vec![
        TreeNode::folder(1, "Production", vec![
            TreeNode::connection(2, "api-prod-01", "10.0.1.11", "deploy", "Key").with_jump("bastion-east"),
            TreeNode::connection(3, "db-prod-01", "10.0.1.21", "dba", "Key").with_jump("bastion-east"),
            TreeNode::folder(4, "Bastions", vec![
                TreeNode::connection(5, "bastion-east", "198.51.100.10", "ops", "Key"),
                TreeNode::connection(6, "bastion-west", "198.51.100.20", "ops", "Key"),
            ]),
        ]),
        TreeNode::folder(7, "Development", vec![
            TreeNode::connection(8, "dev-api", "192.168.56.11", "dev", "Password"),
            TreeNode::connection(9, "dev-db", "192.168.56.12", "dev", "Password"),
        ]),
        TreeNode::connection(10, "localhost", "127.0.0.1", "me", "Agent"),
    ]
}

pub fn contains_id(nodes: &[TreeNode], id: u64) -> bool {
    nodes.iter().any(|n| n.id == id || contains_id(&n.children, id))
}

pub fn find_node(nodes: &[TreeNode], id: u64) -> Option<&TreeNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Folder names from the top level down to (not including) the node.
pub fn folder_path(nodes: &[TreeNode], id: u64) -> Option<Vec<String>> {
    fn walk(nodes: &[TreeNode], id: u64, path: &mut Vec<String>) -> bool {
        for node in nodes {
            if node.id == id {
                return true;
            }
            if node.kind == NodeKind::Folder {
                path.push(node.name.clone());
                if walk(&node.children, id, path) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }
    let mut path = Vec::new();
    walk(nodes, id, &mut path).then_some(path)
}

/// True when `target_id` is `source_id` or lies inside the source's subtree.
pub fn subtree_contains(nodes: &[TreeNode], source_id: u64, target_id: u64) -> bool {
    fn has(node: &TreeNode, id: u64) -> bool {
        node.id == id || node.children.iter().any(|child| has(child, id))
    }
    find_node(nodes, source_id).is_some_and(|source| has(source, target_id))
}

pub fn take_node(nodes: &mut Vec<TreeNode>, id: u64) -> Option<TreeNode> {
    if let Some(index) = nodes.iter().position(|n| n.id == id) {
        return Some(nodes.remove(index));
    }
    for node in nodes {
        if let Some(found) = take_node(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

pub fn find_node_mut(nodes: &mut [TreeNode], id: u64) -> Option<&mut TreeNode> {
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

pub fn find_siblings_mut(nodes: &mut Vec<TreeNode>, target_id: u64) -> Option<&mut Vec<TreeNode>> {
    if nodes.iter().any(|n| n.id == target_id) {
        return Some(nodes);
    }
    for node in nodes {
        if let Some(found) = find_siblings_mut(&mut node.children, target_id) {
            return Some(found);
        }
    }
    None
}

/// Applies a drag-and-drop move. Returns a status message; `Err` means nothing changed.
/// Folders append, connections insert before the target, the root drop appends.
pub fn move_node(
    nodes: &mut Vec<TreeNode>,
    expanded: &mut HashSet<u64>,
    source_id: u64,
    target: DropTarget,
) -> Result<&'static str, &'static str> {
    if !contains_id(nodes, source_id) {
        return Err("Move rejected: source no longer exists.");
    }
    if let DropTarget::Node(target_id) = target {
        if source_id == target_id || subtree_contains(nodes, source_id, target_id) {
            return Err("Move rejected: a folder cannot be dropped into itself or its descendants.");
        }
        if !contains_id(nodes, target_id) {
            return Err("Move rejected: target no longer exists.");
        }
    }
    let Some(moved) = take_node(nodes, source_id) else {
        return Err("Move rejected: source could not be removed.");
    };
    match target {
        DropTarget::Root => {
            nodes.push(moved);
            Ok("Moved item to the root.")
        }
        DropTarget::Node(target_id) => {
            if let Some(folder) = find_node_mut(nodes, target_id).filter(|n| n.kind == NodeKind::Folder) {
                expanded.insert(target_id);
                folder.children.push(moved);
                Ok("Moved item into folder.")
            } else if let Some(siblings) = find_siblings_mut(nodes, target_id) {
                let index = siblings.iter().position(|n| n.id == target_id).unwrap_or(siblings.len());
                siblings.insert(index, moved);
                Ok("Reordered item before the target.")
            } else {
                Err("Move rejected: target parent was not found.")
            }
        }
    }
}

/// Keeps nodes whose name matches (with their whole subtree) and the ancestors of matches.
/// Ancestors of a match are added to `expanded` so the match is visible.
pub fn filter_tree_nodes(nodes: &[TreeNode], query: &str, expanded: &mut HashSet<u64>) -> Vec<TreeNode> {
    let query = query.to_lowercase();
    nodes
        .iter()
        .filter_map(|node| {
            let children = filter_tree_nodes(&node.children, &query, expanded);
            let matches = node.name.to_lowercase().contains(&query);
            if matches || !children.is_empty() {
                if !children.is_empty() {
                    expanded.insert(node.id);
                }
                Some(TreeNode {
                    children: if matches { node.children.clone() } else { children },
                    ..node.clone()
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_moving_a_folder_into_its_descendant() {
        let tree = vec![TreeNode::folder(1, "A", vec![TreeNode::folder(2, "B", vec![])])];
        assert!(subtree_contains(&tree, 1, 2));
        let mut nodes = tree;
        let mut expanded = HashSet::new();
        assert!(move_node(&mut nodes, &mut expanded, 1, DropTarget::Node(2)).is_err());
        assert!(move_node(&mut nodes, &mut expanded, 1, DropTarget::Node(1)).is_err());
    }

    #[test]
    fn removes_a_node_from_a_nested_folder() {
        let mut tree = vec![TreeNode::folder(1, "A", vec![TreeNode::connection(2, "host", "h", "u", "Key")])];
        assert_eq!(take_node(&mut tree, 2).map(|n| n.name), Some("host".into()));
        assert!(!contains_id(&tree, 2));
    }

    #[test]
    fn moves_into_folders_before_connections_and_to_the_root() {
        let mut nodes = sample_tree();
        let mut expanded = HashSet::new();
        // connection into a folder
        move_node(&mut nodes, &mut expanded, 10, DropTarget::Node(7)).unwrap();
        assert!(expanded.contains(&7));
        assert_eq!(folder_path(&nodes, 10).unwrap(), vec!["Development".to_string()]);
        // before another connection
        move_node(&mut nodes, &mut expanded, 10, DropTarget::Node(8)).unwrap();
        let dev = find_node(&nodes, 7).unwrap();
        assert_eq!(dev.children.iter().map(|n| n.id).collect::<Vec<_>>(), vec![10, 8, 9]);
        // to the root
        move_node(&mut nodes, &mut expanded, 10, DropTarget::Root).unwrap();
        assert_eq!(nodes.last().map(|n| n.id), Some(10));
        assert!(move_node(&mut nodes, &mut expanded, 999, DropTarget::Root).is_err());
    }

    #[test]
    fn filter_keeps_matches_with_ancestors_and_expands_them() {
        let nodes = sample_tree();
        let mut expanded = HashSet::new();
        let filtered = filter_tree_nodes(&nodes, "bastion-w", &mut expanded);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Production");
        assert!(expanded.contains(&1) && expanded.contains(&4));
        assert!(filter_tree_nodes(&nodes, "zzz", &mut expanded).is_empty());
        // a matching folder keeps its whole subtree
        let by_folder = filter_tree_nodes(&nodes, "development", &mut expanded);
        assert_eq!(by_folder[0].children.len(), 2);
    }

    #[test]
    fn folder_path_lists_ancestors() {
        let nodes = sample_tree();
        assert_eq!(folder_path(&nodes, 5).unwrap(), vec!["Production".to_string(), "Bastions".to_string()]);
        assert_eq!(folder_path(&nodes, 10).unwrap(), Vec::<String>::new());
        assert!(folder_path(&nodes, 404).is_none());
    }
}
