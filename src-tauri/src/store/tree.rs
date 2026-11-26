//! Pure tree algorithms (no Tauri state), so they can be unit-tested directly.
//! Every mutating command keeps one invariant: each connection id appears
//! exactly once in the tree, always inside exactly one folder.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::vault::{Connection, TreeNode, DEFAULT_FOLDER_ID};

pub const DEFAULT_FOLDER_NAME: &str = "Connections";

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Backend-generated ids for migrated folders and imported connections.
/// Nodes created in the UI use `crypto.randomUUID()` instead.
pub fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{nanos:x}_{:x}", ID_COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Folder names from the root down to the connection's parent.
    pub path: Vec<String>,
}

/// Drops orphaned and duplicate connection nodes, then appends every
/// connection that is not represented yet to the default folder. This is what
/// migrates an M1-M3 flat vault into a tree without a format bump.
pub fn reconcile(tree: &mut Vec<TreeNode>, connections: &BTreeMap<String, Connection>) -> bool {
    let mut seen = BTreeSet::new();
    let mut changed = false;
    prune(tree, connections, &mut seen, &mut changed);

    let mut missing: Vec<String> = connections
        .keys()
        .filter(|id| !seen.contains(*id))
        .cloned()
        .collect();
    if missing.is_empty() {
        return changed;
    }
    missing.sort_by(|a, b| {
        connections[a].name.cmp(&connections[b].name).then_with(|| a.cmp(b))
    });
    let root = ensure_default_folder(tree);
    for id in missing {
        root.push(TreeNode::Connection { id });
    }
    true
}

fn prune(
    nodes: &mut Vec<TreeNode>,
    connections: &BTreeMap<String, Connection>,
    seen: &mut BTreeSet<String>,
    changed: &mut bool,
) {
    nodes.retain_mut(|node| match node {
        TreeNode::Folder { children, .. } => {
            prune(children, connections, seen, changed);
            true
        }
        TreeNode::Connection { id } => {
            if connections.contains_key(id) && seen.insert(id.clone()) {
                true
            } else {
                *changed = true;
                false
            }
        }
    });
}

fn ensure_default_folder(tree: &mut Vec<TreeNode>) -> &mut Vec<TreeNode> {
    let is_default = |n: &TreeNode| {
        matches!(n, TreeNode::Folder { id, .. } if id == DEFAULT_FOLDER_ID)
    };
    if !tree.iter().any(is_default) {
        tree.push(TreeNode::Folder {
            id: DEFAULT_FOLDER_ID.into(),
            name: DEFAULT_FOLDER_NAME.into(),
            children: Vec::new(),
            highlight: Vec::new(),
            color: None,
        });
    }
    let idx = tree.iter().position(is_default).unwrap();
    match &mut tree[idx] {
        TreeNode::Folder { children, .. } => children,
        _ => unreachable!("position() only matched the default folder"),
    }
}


/// Resolve effective M6b rule sets for a connection. Folder rule sets accumulate
/// from the root toward the connection, preserving order and removing duplicates.
/// A non-empty connection assignment is an explicit override of folder inheritance.
pub fn resolve_highlight_rules(
    tree: &[TreeNode],
    connection_id: &str,
    connection_rules: &[String],
) -> Option<Vec<String>> {
    fn visit(nodes: &[TreeNode], connection_id: &str, inherited: &[String]) -> Option<Vec<String>> {
        for node in nodes {
            match node {
                TreeNode::Connection { id } if id == connection_id => return Some(inherited.to_vec()),
                TreeNode::Folder { children, highlight, .. } => {
                    let mut next = inherited.to_vec();
                    for rule_set in highlight {
                        if !next.contains(rule_set) {
                            next.push(rule_set.clone());
                        }
                    }
                    if let Some(found) = visit(children, connection_id, &next) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let inherited = visit(tree, connection_id, &[])?;
    Some(if connection_rules.is_empty() { inherited } else { connection_rules.to_vec() })
}


/// Resolve a connection color from its explicit value or nearest colored ancestor folder.
pub fn resolve_identity_color(tree: &[TreeNode], connection_id: &str, explicit: Option<&str>) -> Option<String> {
    fn visit(nodes: &[TreeNode], id: &str, inherited: Option<&str>) -> Option<String> {
        for node in nodes {
            match node {
                TreeNode::Connection { id: found } if found == id => return inherited.map(str::to_string),
                TreeNode::Folder { children, color, .. } => {
                    if let Some(found) = visit(children, id, color.as_deref().or(inherited)) { return Some(found); }
                }
                _ => {}
            }
        }
        None
    }
    explicit.map(str::to_string).or_else(|| visit(tree, connection_id, None))
}

pub fn set_folder_color(nodes: &mut [TreeNode], id: &str, color: Option<String>) -> bool {
    let Some(node) = folder_mut(nodes, id) else { return false };
    match node {
        TreeNode::Folder { color: current, .. } => { *current = color; true }
        TreeNode::Connection { .. } => false,
    }
}

/// Replace the rule-set assignments on one folder without disturbing its children.
pub fn set_folder_highlight(nodes: &mut [TreeNode], id: &str, highlight: Vec<String>) -> bool {
    let Some(node) = folder_mut(nodes, id) else { return false };
    match node {
        TreeNode::Folder { highlight: current, .. } => {
            *current = highlight;
            true
        }
        TreeNode::Connection { .. } => false,
    }
}

pub fn contains_id(nodes: &[TreeNode], id: &str) -> bool {
    nodes.iter().any(|node| match node {
        TreeNode::Folder { children, .. } => node.id() == id || contains_id(children, id),
        TreeNode::Connection { .. } => node.id() == id,
    })
}

pub fn folder_mut<'a>(nodes: &'a mut [TreeNode], id: &str) -> Option<&'a mut TreeNode> {
    // Two passes: the first only borrows immutably, so returning a mutable
    // reference out of the loop is sound. The second pass recurses into the
    // children that were not a match at this level.
    let direct = nodes.iter().position(|n| n.is_folder() && n.id() == id);
    if let Some(index) = direct {
        return Some(&mut nodes[index]);
    }
    for node in nodes.iter_mut() {
        if let TreeNode::Folder { children, .. } = node {
            if let Some(found) = folder_mut(children, id) {
                return Some(found);
            }
        }
    }
    None
}

fn find_node<'a>(nodes: &'a [TreeNode], id: &str) -> Option<&'a TreeNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let TreeNode::Folder { children, .. } = node {
            if let Some(found) = find_node(children, id) {
                return Some(found);
            }
        }
    }
    None
}

pub fn take_node(nodes: &mut Vec<TreeNode>, id: &str) -> Option<TreeNode> {
    for i in 0..nodes.len() {
        if nodes[i].id() == id {
            return Some(nodes.remove(i));
        }
        if let TreeNode::Folder { children, .. } = &mut nodes[i] {
            if let Some(found) = take_node(children, id) {
                return Some(found);
            }
        }
    }
    None
}

fn folder_children<'a>(
    nodes: &'a mut Vec<TreeNode>,
    parent: Option<&str>,
) -> Option<&'a mut Vec<TreeNode>> {
    match parent {
        None => Some(nodes),
        Some(id) => match folder_mut(nodes, id) {
            Some(TreeNode::Folder { children, .. }) => Some(children),
            _ => None,
        },
    }
}

pub fn upsert_folder(
    tree: &mut Vec<TreeNode>,
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
) -> Result<String, &'static str> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("bad_name");
    }
    if let Some(id) = id {
        match folder_mut(tree, &id) {
            Some(TreeNode::Folder { name: current, .. }) => {
                *current = name;
                Ok(id)
            }
            _ => Err("folder_not_found"),
        }
    } else {
        let folder_id = loop {
            let candidate = new_id("f");
            if !contains_id(tree, &candidate) {
                break candidate;
            }
        };
        let Some(children) = folder_children(tree, parent_id.as_deref()) else {
            return Err("folder_not_found");
        };
        children.push(TreeNode::Folder {
            id: folder_id.clone(),
            name,
            children: Vec::new(),
            highlight: Vec::new(),
            color: None,
        });
        Ok(folder_id)
    }
}

/// Returns the connection ids contained in the deleted subtree. The command
/// layer removes those from the connections map when `cascade` is true.
pub fn delete_folder(
    tree: &mut Vec<TreeNode>,
    id: &str,
    cascade: bool,
) -> Result<Vec<String>, &'static str> {
    let (direct_children, ids) = {
        let node = folder_mut(tree, id).ok_or("folder_not_found")?;
        match node {
            TreeNode::Folder { children, .. } => {
                (children.len(), collect_connections(children))
            }
            _ => return Err("folder_not_found"),
        }
    };
    if !cascade && direct_children > 0 {
        return Err("folder_not_empty");
    }
    take_node(tree, id);
    Ok(ids)
}

fn collect_connections(nodes: &[TreeNode]) -> Vec<String> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            TreeNode::Connection { id } => out.push(id.clone()),
            TreeNode::Folder { children, .. } => out.extend(collect_connections(children)),
        }
    }
    out
}

pub fn move_node(
    tree: &mut Vec<TreeNode>,
    id: &str,
    parent_id: Option<&str>,
    index: usize,
) -> Result<(), &'static str> {
    // Validate the destination before removing the source. An invalid move
    // must leave the original tree untouched (including its original parent).
    let source = find_node(tree, id).ok_or("node_not_found")?;
    if let Some(parent) = parent_id {
        if subtree_contains(source, parent) {
            return Err("cycle");
        }
        if folder_mut(tree, parent).is_none() {
            return Err("folder_not_found");
        }
    }
    let node = take_node(tree, id).ok_or("node_not_found")?;
    let children = folder_children(tree, parent_id)
        .ok_or("folder_not_found")?;
    let at = index.min(children.len());
    children.insert(at, node);
    Ok(())
}

fn subtree_contains(node: &TreeNode, id: &str) -> bool {
    if node.id() == id {
        return true;
    }
    match node {
        TreeNode::Folder { children, .. } => {
            children.iter().any(|child| subtree_contains(child, id))
        }
        _ => false,
    }
}

pub fn unlink(tree: &mut Vec<TreeNode>, conn_id: &str) -> bool {
    fn walk(nodes: &mut Vec<TreeNode>, id: &str) -> bool {
        for i in 0..nodes.len() {
            if matches!(&nodes[i], TreeNode::Connection { id: cid } if cid == id) {
                nodes.remove(i);
                return true;
            }
            if let TreeNode::Folder { children, .. } = &mut nodes[i] {
                if walk(children, id) {
                    return true;
                }
            }
        }
        false
    }
    walk(tree, conn_id)
}

pub fn link(tree: &mut Vec<TreeNode>, conn_id: &str, parent_id: Option<&str>) {
    if contains_id(tree, conn_id) {
        return;
    }
    let node = TreeNode::Connection { id: conn_id.to_string() };
    match parent_id {
        // Without an explicit destination, new and imported connections go
        // into the default folder instead of floating at the root level.
        None => ensure_default_folder(tree).push(node),
        Some(parent) => match folder_children(tree, Some(parent)) {
            Some(children) => children.push(node),
            None => ensure_default_folder(tree).push(node),
        },
    }
}

/// Creates or reuses a chain of named folders and returns the leaf folder id
/// plus the number of folders actually created. `parent_id: None` means the
/// tree root. Matching is case-insensitive so importing the same source twice
/// does not duplicate its folder structure.
pub fn ensure_path(
    tree: &mut Vec<TreeNode>,
    parent_id: Option<&str>,
    names: &[String],
) -> (Option<String>, usize) {
    let mut current = parent_id.map(str::to_string);
    let mut created = 0usize;
    for raw in names {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let existing = {
            let Some(children) = folder_children(tree, current.as_deref()) else {
                return (None, created);
            };
            children.iter().find_map(|node| match node {
                TreeNode::Folder { id, name: current_name, .. }
                    if current_name.eq_ignore_ascii_case(name) =>
                {
                    Some(id.clone())
                }
                _ => None,
            })
        };
        match existing {
            Some(id) => current = Some(id),
            None => {
                let id = new_id("f");
                let Some(children) = folder_children(tree, current.as_deref()) else {
                    return (None, created);
                };
                children.push(TreeNode::Folder {
                    id: id.clone(),
                    name: name.to_string(),
                    children: Vec::new(),
                    highlight: Vec::new(),
            color: None,
                });
                created += 1;
                current = Some(id);
            }
        }
    }
    (current.or_else(|| parent_id.map(str::to_string)), created)
}

pub fn paths(tree: &[TreeNode]) -> BTreeMap<String, Vec<String>> {
    fn walk(
        nodes: &[TreeNode],
        prefix: &mut Vec<String>,
        out: &mut BTreeMap<String, Vec<String>>,
    ) {
        for node in nodes {
            match node {
                TreeNode::Connection { id } => {
                    out.insert(id.clone(), prefix.clone());
                }
                TreeNode::Folder { name, children, .. } => {
                    prefix.push(name.clone());
                    walk(children, prefix, out);
                    prefix.pop();
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(tree, &mut Vec::new(), &mut out);
    out
}

pub fn search(
    connections: &BTreeMap<String, Connection>,
    paths: &BTreeMap<String, Vec<String>>,
    query: &str,
) -> Vec<SearchHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<SearchHit> = connections
        .iter()
        .filter_map(|(id, c)| {
            let matches = c.name.to_lowercase().contains(&needle)
                || c.host.to_lowercase().contains(&needle)
                || c.user.to_lowercase().contains(&needle);
            if !matches {
                return None;
            }
            Some(SearchHit {
                id: id.clone(),
                name: c.name.clone(),
                host: c.host.clone(),
                port: c.port,
                user: c.user.clone(),
                path: paths.get(id).cloned().unwrap_or_default(),
            })
        })
        .collect();
    hits.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: &str, name: &str, host: &str) -> Connection {
        Connection {
            id: id.into(),
            name: name.into(),
            host: host.into(),
            user: "admin".into(),
            ..Connection::default()
        }
    }

    fn map(entries: Vec<Connection>) -> BTreeMap<String, Connection> {
        entries.into_iter().map(|c| (c.id.clone(), c)).collect()
    }

    fn folder(id: &str, name: &str, children: Vec<TreeNode>) -> TreeNode {
        TreeNode::Folder { id: id.into(), name: name.into(), children, highlight: Vec::new(), color: None }
    }

    fn link_to(id: &str) -> TreeNode {
        TreeNode::Connection { id: id.into() }
    }


    fn folder_with_rules(id: &str, name: &str, highlight: Vec<String>, children: Vec<TreeNode>) -> TreeNode {
        TreeNode::Folder { id: id.into(), name: name.into(), children, highlight, color: None }
    }


    #[test]
    fn identity_colors_inherit_from_nearest_folder_and_allow_connection_override() {
        let mut tree = vec![folder(
            "root", "Root",
            vec![folder("nested", "Nested", vec![link_to("c1")])],
        )];
        assert_eq!(resolve_identity_color(&tree, "c1", None), None);
        assert!(set_folder_color(&mut tree, "root", Some("#336699".into())));
        assert_eq!(resolve_identity_color(&tree, "c1", None).as_deref(), Some("#336699"));
        assert!(set_folder_color(&mut tree, "nested", Some("#993366".into())));
        assert_eq!(resolve_identity_color(&tree, "c1", None).as_deref(), Some("#993366"));
        assert_eq!(resolve_identity_color(&tree, "c1", Some("#228844")).as_deref(), Some("#228844"));
        assert!(set_folder_color(&mut tree, "nested", None));
        assert_eq!(resolve_identity_color(&tree, "c1", None).as_deref(), Some("#336699"));
        assert!(!set_folder_color(&mut tree, "missing", Some("#ffffff".into())));
    }

    #[test]
    fn set_folder_highlight_changes_only_the_requested_folder() {
        let mut tree = vec![folder(
            "root", "Root",
            vec![folder("nested", "Nested", vec![link_to("c1")])],
        )];
        assert!(set_folder_highlight(&mut tree, "nested", vec!["linux".into()]));
        match &tree[0] {
            TreeNode::Folder { highlight, children, .. } => {
                assert!(highlight.is_empty());
                match &children[0] {
                    TreeNode::Folder { highlight, .. } => assert_eq!(highlight, &vec!["linux".to_string()]),
                    _ => panic!("expected nested folder"),
                }
            }
            _ => panic!("expected root folder"),
        }
        assert!(!set_folder_highlight(&mut tree, "missing", Vec::new()));
    }

    #[test]
    fn folder_highlight_rules_inherit_and_connection_rules_override() {
        let tree = vec![folder_with_rules(
            "root", "Root", vec!["linux".into(), "common".into()],
            vec![folder_with_rules(
                "nested", "Nested", vec!["network".into(), "common".into()],
                vec![link_to("c1")],
            )],
        )];
        assert_eq!(
            resolve_highlight_rules(&tree, "c1", &[]),
            Some(vec!["linux".into(), "common".into(), "network".into()])
        );
        assert_eq!(
            resolve_highlight_rules(&tree, "c1", &["custom".into()]),
            Some(vec!["custom".into()])
        );
        assert_eq!(resolve_highlight_rules(&tree, "missing", &[]), None);
    }

    #[test]
    fn reconcile_wraps_a_flat_vault_into_one_folder() {
        let connections = map(vec![conn("c1", "pgw-01", "10.0.0.5"), conn("c2", "db-01", "10.0.0.6")]);
        let mut tree = Vec::new();
        assert!(reconcile(&mut tree, &connections));
        assert_eq!(tree.len(), 1);
        match &tree[0] {
            TreeNode::Folder { id, name, children, .. } => {
                assert_eq!(id, DEFAULT_FOLDER_ID);
                assert_eq!(name, DEFAULT_FOLDER_NAME);
                assert_eq!(children.len(), 2);
            }
            _ => panic!("expected a root folder"),
        }
        assert!(!reconcile(&mut tree, &connections), "reconcile is idempotent");
    }

    #[test]
    fn reconcile_drops_orphans_and_duplicates() {
        let connections = map(vec![conn("c1", "pgw-01", "10.0.0.5")]);
        let mut tree = vec![
            folder("f1", "A", vec![link_to("c1"), link_to("c1"), link_to("gone")]),
            folder("f2", "B", vec![]),
        ];
        assert!(reconcile(&mut tree, &connections));
        assert_eq!(paths(&tree).len(), 1);
        assert_eq!(paths(&tree)["c1"], vec!["A".to_string()]);
    }

    #[test]
    fn move_rejects_a_folder_into_its_own_descendant() {
        let mut tree = vec![folder(
            "f1",
            "Production",
            vec![folder("f2", "Edge", vec![link_to("c1")])],
        )];
        assert_eq!(move_node(&mut tree, "f1", Some("f2"), 0), Err("cycle"));
        assert!(contains_id(&tree, "f1"));
        assert!(contains_id(&tree, "f2"));
        assert_eq!(paths(&tree)["c1"], vec!["Production", "Edge"]);
    }

    #[test]
    fn rejected_move_preserves_the_original_parent_and_position() {
        let mut tree = vec![folder(
            "f1",
            "Production",
            vec![folder("f2", "Edge", vec![link_to("c1")]), link_to("c2")],
        )];
        assert_eq!(move_node(&mut tree, "f1", Some("f2"), 0), Err("cycle"));
        assert_eq!(move_node(&mut tree, "c2", Some("missing"), 0), Err("folder_not_found"));
        assert_eq!(paths(&tree)["c1"], vec!["Production", "Edge"]);
        assert_eq!(paths(&tree)["c2"], vec!["Production"]);
        match &tree[0] {
            TreeNode::Folder { children, .. } => assert_eq!(children[1].id(), "c2"),
            _ => panic!("expected a root folder"),
        }
    }

    #[test]
    fn move_inserts_at_the_requested_index() {
        let connections = map(vec![conn("c1", "a", "h1"), conn("c2", "b", "h2")]);
        let mut tree = vec![folder("f1", "Root", vec![link_to("c1"), link_to("c2")])];
        assert_eq!(move_node(&mut tree, "c2", Some("f1"), 0), Ok(()));
        match &tree[0] {
            TreeNode::Folder { children, .. } => {
                assert_eq!(children[0].id(), "c2");
                assert_eq!(children[1].id(), "c1");
            }
            _ => panic!("expected a folder"),
        }
        assert_eq!(paths(&tree).len(), 2);
        assert!(connections.contains_key("c1"));
    }

    #[test]
    fn delete_folder_requires_cascade_and_returns_affected_ids() {
        let mut tree = vec![folder(
            "f1",
            "Production",
            vec![folder("f2", "Edge", vec![link_to("c1")]), link_to("c2")],
        )];
        assert_eq!(delete_folder(&mut tree, "f1", false), Err("folder_not_empty"));
        assert!(contains_id(&tree, "f1"));
        let ids = delete_folder(&mut tree, "f1", true).unwrap();
        assert_eq!(ids, vec!["c1".to_string(), "c2".to_string()]);
        assert!(tree.is_empty());
        assert_eq!(delete_folder(&mut tree, "f1", true), Err("folder_not_found"));
    }

    #[test]
    fn upsert_folder_creates_renames_and_rejects_bad_parent() {
        let mut tree = Vec::new();
        let id = upsert_folder(&mut tree, None, None, "  Production  ".into()).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(folder_mut(&mut tree, &id).unwrap().id(), id.as_str());
        upsert_folder(&mut tree, Some(id.clone()), None, "Prod".into()).unwrap();
        match &tree[0] {
            TreeNode::Folder { name, .. } => assert_eq!(name, "Prod"),
            _ => panic!("expected a folder"),
        }
        assert_eq!(
            upsert_folder(&mut tree, None, Some("nope".to_string()), "X".into()),
            Err("folder_not_found")
        );
        assert_eq!(upsert_folder(&mut tree, None, None, "  ".into()), Err("bad_name"));
    }

    #[test]
    fn ensure_path_creates_nested_folders_and_reuses_them() {
        let mut tree = Vec::new();
        let names = vec!["Production".to_string(), "Edge".to_string()];
        let (leaf, created) = ensure_path(&mut tree, None, &names);
        assert_eq!(created, 2);
        let leaf = leaf.unwrap();
        assert!(contains_id(&tree, &leaf));

        link(&mut tree, "c1", Some(&leaf));
        assert_eq!(paths(&tree)["c1"], vec!["Production", "Edge"]);

        // Same names, including different casing, must not create duplicates.
        let (leaf_again, created_again) = ensure_path(
            &mut tree,
            None,
            &["production".to_string(), "EDGE".to_string()],
        );
        assert_eq!(created_again, 0);
        assert_eq!(leaf_again.as_deref(), Some(leaf.as_str()));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn ensure_path_can_start_under_an_existing_folder() {
        let mut tree = vec![folder("f1", "Imported", Vec::new())];
        let (leaf, created) =
            ensure_path(&mut tree, Some("f1"), &["A".to_string(), "B".to_string()]);
        assert_eq!(created, 2);
        link(&mut tree, "c1", leaf.as_deref());
        assert_eq!(paths(&tree)["c1"], vec!["Imported", "A", "B"]);
        assert_eq!(
            ensure_path(&mut tree, Some("missing"), &["X".to_string()]),
            (None, 0)
        );
    }

    #[test]
    fn search_matches_host_and_user_and_returns_breadcrumbs() {
        let mut c = conn("c1", "pgw-01", "10.0.0.5");
        c.user = "dba".into();
        c.port = 2222;
        let connections = map(vec![c, conn("c2", "web-01", "10.0.0.9")]);
        let tree = vec![folder(
            "f1",
            "Production",
            vec![folder("f2", "Edge", vec![link_to("c1")]), link_to("c2")],
        )];
        let paths = paths(&tree);
        let hits = search(&connections, &paths, "10.0.0.5");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, vec!["Production", "Edge"]);
        assert_eq!(hits[0].port, 2222);
        assert_eq!(search(&connections, &paths, "dba").len(), 1);
        assert_eq!(search(&connections, &paths, "  ").len(), 0);
    }
}
