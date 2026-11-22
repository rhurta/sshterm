//! Scripts library: the payload of `scripts.vault`.
//!
//! A tree of folders and scripts. Everything here is pure data manipulation (no
//! Tauri, no I/O) so it is covered by plain unit tests. Errors are short codes the
//! frontend translates.

use serde::{Deserialize, Serialize};

pub const SCRIPTS_VERSION: u32 = 1;
/// Largest script body, in bytes. Keeps the vault small and the preview responsive.
pub const MAX_SCRIPT_BYTES: usize = 256 * 1024;
pub const MAX_NAME_CHARS: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ScriptNode {
    Folder {
        id: String,
        name: String,
        #[serde(default)]
        children: Vec<ScriptNode>,
    },
    Script {
        id: String,
        name: String,
        #[serde(default)]
        body: String,
        /// Unix seconds of the last save.
        #[serde(default)]
        updated_at: u64,
        /// Shown as a one-click button under the command console.
        #[serde(default)]
        favorite: bool,
    },
}

impl ScriptNode {
    pub fn id(&self) -> &str {
        match self {
            Self::Folder { id, .. } | Self::Script { id, .. } => id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Folder { name, .. } | Self::Script { name, .. } => name,
        }
    }

    fn set_name(&mut self, new_name: String) {
        match self {
            Self::Folder { name, .. } | Self::Script { name, .. } => *name = new_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptsData {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub tree: Vec<ScriptNode>,
}

fn default_version() -> u32 {
    SCRIPTS_VERSION
}

impl Default for ScriptsData {
    fn default() -> Self {
        Self { version: SCRIPTS_VERSION, tree: Vec::new() }
    }
}

/// What the tree view needs: no script bodies.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ScriptSummary {
    Folder { id: String, name: String, children: Vec<ScriptSummary> },
    Script { id: String, name: String, updated_at: u64, size: usize, favorite: bool },
}

/// One script with its body and the folder it lives in (`None` = top level).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScriptDetail {
    pub id: String,
    pub name: String,
    pub body: String,
    pub folder_id: Option<String>,
    pub favorite: bool,
}

/// A favorite script as the console's quick-load row needs it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FavoriteScript {
    pub id: String,
    pub name: String,
    /// Folder names from the top level, joined with " / " ("" at the top level).
    pub folder_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptError {
    InvalidName,
    NameExists,
    TooLarge,
    NotFound,
    NotFolder,
    Cycle,
}

impl ScriptError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidName => "invalid_name",
            Self::NameExists => "name_exists",
            Self::TooLarge => "too_large",
            Self::NotFound => "not_found",
            Self::NotFolder => "not_found",
            Self::Cycle => "cycle",
        }
    }
}

type Result<T> = std::result::Result<T, ScriptError>;

pub fn new_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// Trims and validates a folder or script name.
pub fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(ScriptError::InvalidName);
    }
    Ok(name.to_string())
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn find<'a>(nodes: &'a [ScriptNode], id: &str) -> Option<&'a ScriptNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let ScriptNode::Folder { children, .. } = node {
            if let Some(found) = find(children, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Children list of a folder (`None` = top level).
fn list_mut<'a>(nodes: &'a mut Vec<ScriptNode>, parent: Option<&str>) -> Result<&'a mut Vec<ScriptNode>> {
    let Some(parent) = parent else { return Ok(nodes) };
    for node in nodes.iter_mut() {
        match node {
            ScriptNode::Folder { id, children, .. } => {
                if id == parent {
                    return Ok(children);
                }
                if find(children, parent).is_some() {
                    return list_mut(children, Some(parent));
                }
            }
            ScriptNode::Script { id, .. } if id == parent => return Err(ScriptError::NotFolder),
            ScriptNode::Script { .. } => {}
        }
    }
    Err(ScriptError::NotFound)
}

/// Removes a node from wherever it is and returns it.
fn take(nodes: &mut Vec<ScriptNode>, id: &str) -> Option<ScriptNode> {
    if let Some(at) = nodes.iter().position(|node| node.id() == id) {
        return Some(nodes.remove(at));
    }
    for node in nodes.iter_mut() {
        if let ScriptNode::Folder { children, .. } = node {
            if let Some(found) = take(children, id) {
                return Some(found);
            }
        }
    }
    None
}

fn parent_of(nodes: &[ScriptNode], id: &str, parent: Option<&str>) -> Option<Option<String>> {
    for node in nodes {
        if node.id() == id {
            return Some(parent.map(str::to_string));
        }
        if let ScriptNode::Folder { id: folder_id, children, .. } = node {
            if let Some(found) = parent_of(children, id, Some(folder_id)) {
                return Some(found);
            }
        }
    }
    None
}

fn name_taken(list: &[ScriptNode], name: &str, except: Option<&str>) -> bool {
    list.iter().any(|node| Some(node.id()) != except && same_name(node.name(), name))
}

fn contains_id(node: &ScriptNode, id: &str) -> bool {
    match node {
        ScriptNode::Folder { children, .. } => children.iter().any(|child| child.id() == id || contains_id(child, id)),
        ScriptNode::Script { .. } => false,
    }
}

impl ScriptsData {
    pub fn summaries(&self) -> Vec<ScriptSummary> {
        fn convert(nodes: &[ScriptNode]) -> Vec<ScriptSummary> {
            nodes
                .iter()
                .map(|node| match node {
                    ScriptNode::Folder { id, name, children } => {
                        ScriptSummary::Folder { id: id.clone(), name: name.clone(), children: convert(children) }
                    }
                    ScriptNode::Script { id, name, body, updated_at, favorite } => ScriptSummary::Script {
                        id: id.clone(),
                        name: name.clone(),
                        updated_at: *updated_at,
                        size: body.len(),
                        favorite: *favorite,
                    },
                })
                .collect()
        }
        convert(&self.tree)
    }

    pub fn get(&self, id: &str) -> Result<ScriptDetail> {
        match find(&self.tree, id) {
            Some(ScriptNode::Script { id, name, body, favorite, .. }) => Ok(ScriptDetail {
                id: id.clone(),
                name: name.clone(),
                body: body.clone(),
                folder_id: parent_of(&self.tree, id, None).flatten(),
                favorite: *favorite,
            }),
            _ => Err(ScriptError::NotFound),
        }
    }

    pub fn create_folder(&mut self, parent: Option<&str>, name: &str) -> Result<String> {
        let name = validate_name(name)?;
        let list = list_mut(&mut self.tree, parent)?;
        if name_taken(list, &name, None) {
            return Err(ScriptError::NameExists);
        }
        let id = new_id();
        list.push(ScriptNode::Folder { id: id.clone(), name, children: Vec::new() });
        Ok(id)
    }

    /// Saves a script.
    ///
    /// - `id = Some(..)`: updates that script (body, and name/folder when they changed).
    /// - `id = None`: creates a script in `parent`; if a script with that name exists
    ///   there it is replaced only when `overwrite` is true, otherwise `NameExists`.
    ///
    /// `favorite`: `Some(flag)` sets the favorite mark, `None` keeps it (new scripts
    /// start without one).
    pub fn save_script(
        &mut self,
        id: Option<&str>,
        parent: Option<&str>,
        name: &str,
        body: &str,
        overwrite: bool,
        now: u64,
        favorite: Option<bool>,
    ) -> Result<String> {
        let name = validate_name(name)?;
        if body.len() > MAX_SCRIPT_BYTES {
            return Err(ScriptError::TooLarge);
        }
        match id {
            Some(id) => {
                if !matches!(find(&self.tree, id), Some(ScriptNode::Script { .. })) {
                    return Err(ScriptError::NotFound);
                }
                // Validate the destination before touching anything.
                if list_mut(&mut self.tree, parent).map(|list| name_taken(list, &name, Some(id)))? {
                    return Err(ScriptError::NameExists);
                }
                let mut node = take(&mut self.tree, id).ok_or(ScriptError::NotFound)?;
                if let ScriptNode::Script { body: old_body, updated_at, favorite: mark, .. } = &mut node {
                    *old_body = body.to_string();
                    *updated_at = now;
                    if let Some(flag) = favorite {
                        *mark = flag;
                    }
                }
                node.set_name(name);
                list_mut(&mut self.tree, parent)?.push(node);
                Ok(id.to_string())
            }
            None => {
                let list = list_mut(&mut self.tree, parent)?;
                if let Some(existing) = list.iter_mut().find(|node| same_name(node.name(), &name)) {
                    return match existing {
                        ScriptNode::Script { id, body: old_body, updated_at, favorite: mark, .. } if overwrite => {
                            *old_body = body.to_string();
                            *updated_at = now;
                            if let Some(flag) = favorite {
                                *mark = flag;
                            }
                            Ok(id.clone())
                        }
                        _ => Err(ScriptError::NameExists),
                    };
                }
                let id = new_id();
                list.push(ScriptNode::Script {
                    id: id.clone(),
                    name,
                    body: body.to_string(),
                    updated_at: now,
                    favorite: favorite.unwrap_or(false),
                });
                Ok(id)
            }
        }
    }

    pub fn set_favorite(&mut self, id: &str, favorite: bool) -> Result<()> {
        fn walk(nodes: &mut [ScriptNode], id: &str, flag: bool) -> bool {
            for node in nodes {
                match node {
                    ScriptNode::Script { id: script_id, favorite, .. } if script_id == id => {
                        *favorite = flag;
                        return true;
                    }
                    ScriptNode::Folder { children, .. } => {
                        if walk(children, id, flag) {
                            return true;
                        }
                    }
                    ScriptNode::Script { .. } => {}
                }
            }
            false
        }
        if walk(&mut self.tree, id, favorite) { Ok(()) } else { Err(ScriptError::NotFound) }
    }

    /// Favorite scripts sorted by name (case-insensitive), with their folder path.
    pub fn favorites(&self) -> Vec<FavoriteScript> {
        fn walk(nodes: &[ScriptNode], path: &mut Vec<String>, out: &mut Vec<FavoriteScript>) {
            for node in nodes {
                match node {
                    ScriptNode::Folder { name, children, .. } => {
                        path.push(name.clone());
                        walk(children, path, out);
                        path.pop();
                    }
                    ScriptNode::Script { id, name, favorite: true, .. } => {
                        out.push(FavoriteScript { id: id.clone(), name: name.clone(), folder_path: path.join(" / ") });
                    }
                    ScriptNode::Script { .. } => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.tree, &mut Vec::new(), &mut out);
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
        out
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = validate_name(name)?;
        let parent = parent_of(&self.tree, id, None).ok_or(ScriptError::NotFound)?;
        let list = list_mut(&mut self.tree, parent.as_deref())?;
        if name_taken(list, &name, Some(id)) {
            return Err(ScriptError::NameExists);
        }
        list.iter_mut().find(|node| node.id() == id).ok_or(ScriptError::NotFound)?.set_name(name);
        Ok(())
    }

    /// Moves a script or folder into `parent` (`None` = top level).
    pub fn move_node(&mut self, id: &str, parent: Option<&str>) -> Result<()> {
        let node = find(&self.tree, id).ok_or(ScriptError::NotFound)?;
        if let Some(parent) = parent {
            if parent == id || contains_id(node, parent) {
                return Err(ScriptError::Cycle);
            }
        }
        let name = node.name().to_string();
        if list_mut(&mut self.tree, parent).map(|list| name_taken(list, &name, Some(id)))? {
            return Err(ScriptError::NameExists);
        }
        let node = take(&mut self.tree, id).ok_or(ScriptError::NotFound)?;
        list_mut(&mut self.tree, parent)?.push(node);
        Ok(())
    }

    /// Deletes a script, or a folder with everything in it.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        take(&mut self.tree, id).map(drop).ok_or(ScriptError::NotFound)
    }

    /// IDs of the scripts whose name or body contains the text (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<String> {
        let query = query.trim().to_lowercase();
        let mut found = Vec::new();
        if query.is_empty() {
            return found;
        }
        fn walk(nodes: &[ScriptNode], query: &str, found: &mut Vec<String>) {
            for node in nodes {
                match node {
                    ScriptNode::Folder { children, .. } => walk(children, query, found),
                    ScriptNode::Script { id, name, body, .. } => {
                        if name.to_lowercase().contains(query) || body.to_lowercase().contains(query) {
                            found.push(id.clone());
                        }
                    }
                }
            }
        }
        walk(&self.tree, &query, &mut found);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (ScriptsData, String, String, String) {
        let mut data = ScriptsData::default();
        let ops = data.create_folder(None, "Ops").unwrap();
        let net = data.create_folder(Some(&ops), "Network").unwrap();
        let script = data.save_script(None, Some(&net), "Check routes", "ip route\nip -6 route", false, 10, None).unwrap();
        (data, ops, net, script)
    }

    #[test]
    fn names_are_trimmed_and_validated() {
        assert_eq!(validate_name("  a b  ").unwrap(), "a b");
        assert_eq!(validate_name("   "), Err(ScriptError::InvalidName));
        assert_eq!(validate_name(&"x".repeat(101)), Err(ScriptError::InvalidName));
        assert_eq!(validate_name("bad\nname"), Err(ScriptError::InvalidName));
        assert!(validate_name(&"x".repeat(100)).is_ok());
    }

    #[test]
    fn save_creates_and_detail_reports_folder() {
        let (mut data, _ops, net, script) = sample();
        let detail = data.get(&script).unwrap();
        assert_eq!(detail.name, "Check routes");
        assert_eq!(detail.body, "ip route\nip -6 route");
        assert_eq!(detail.folder_id.as_deref(), Some(net.as_str()));
        let top = data.save_script(None, None, "Top", "x", false, 1, None).unwrap();
        assert_eq!(data.get(&top).unwrap().folder_id, None);
    }

    #[test]
    fn same_name_needs_overwrite_and_keeps_the_id() {
        let (mut data, _ops, net, script) = sample();
        assert_eq!(data.save_script(None, Some(&net), "check ROUTES", "new", false, 20, None), Err(ScriptError::NameExists));
        let again = data.save_script(None, Some(&net), "check ROUTES", "new", true, 20, None).unwrap();
        assert_eq!(again, script);
        assert_eq!(data.get(&script).unwrap().body, "new");
        // a folder with that name is never overwritten
        data.create_folder(Some(&net), "Sub").unwrap();
        assert_eq!(data.save_script(None, Some(&net), "sub", "x", true, 1, None), Err(ScriptError::NameExists));
    }

    #[test]
    fn updating_by_id_can_rename_and_move() {
        let (mut data, ops, _net, script) = sample();
        let id = data.save_script(Some(&script), Some(&ops), "Routes v2", "ip r", false, 30, None).unwrap();
        assert_eq!(id, script);
        let detail = data.get(&script).unwrap();
        assert_eq!((detail.name.as_str(), detail.body.as_str()), ("Routes v2", "ip r"));
        assert_eq!(detail.folder_id.as_deref(), Some(ops.as_str()));
        // collision at the destination leaves everything untouched
        data.save_script(None, None, "Taken", "x", false, 1, None).unwrap();
        assert_eq!(data.save_script(Some(&script), None, "taken", "y", false, 40, None), Err(ScriptError::NameExists));
        assert_eq!(data.get(&script).unwrap().body, "ip r");
        assert_eq!(data.save_script(Some("nope"), None, "n", "b", false, 1, None), Err(ScriptError::NotFound));
    }

    #[test]
    fn size_limit_and_bad_parents() {
        let mut data = ScriptsData::default();
        let big = "a".repeat(MAX_SCRIPT_BYTES + 1);
        assert_eq!(data.save_script(None, None, "big", &big, false, 1, None), Err(ScriptError::TooLarge));
        assert!(data.save_script(None, None, "ok", &"a".repeat(MAX_SCRIPT_BYTES), false, 1, None).is_ok());
        assert_eq!(data.save_script(None, Some("missing"), "n", "b", false, 1, None), Err(ScriptError::NotFound));
        let script = data.save_script(None, None, "s", "b", false, 1, None).unwrap();
        assert_eq!(data.create_folder(Some(&script), "inside"), Err(ScriptError::NotFolder));
    }

    #[test]
    fn rename_and_move_respect_siblings_and_cycles() {
        let (mut data, ops, net, script) = sample();
        let other = data.create_folder(None, "Other").unwrap();
        data.rename(&net, "Net").unwrap();
        assert_eq!(data.rename(&other, "ops"), Err(ScriptError::NameExists));
        assert_eq!(data.rename(&script, ""), Err(ScriptError::InvalidName));
        assert_eq!(data.move_node(&ops, Some(&net)), Err(ScriptError::Cycle));
        assert_eq!(data.move_node(&ops, Some(&ops)), Err(ScriptError::Cycle));
        data.move_node(&script, Some(&other)).unwrap();
        assert_eq!(data.get(&script).unwrap().folder_id.as_deref(), Some(other.as_str()));
        data.move_node(&script, None).unwrap();
        assert_eq!(data.get(&script).unwrap().folder_id, None);
    }

    #[test]
    fn delete_removes_folders_recursively() {
        let (mut data, ops, _net, script) = sample();
        data.delete(&ops).unwrap();
        assert_eq!(data.get(&script), Err(ScriptError::NotFound));
        assert!(data.tree.is_empty());
        assert_eq!(data.delete(&ops), Err(ScriptError::NotFound));
    }

    #[test]
    fn search_matches_names_and_bodies_case_insensitively() {
        let (mut data, _ops, _net, script) = sample();
        let other = data.save_script(None, None, "Reboot host", "sudo REBOOT", false, 1, None).unwrap();
        assert_eq!(data.search("ROUTE"), vec![script.clone()]);
        assert_eq!(data.search("reboot"), vec![other]);
        assert!(data.search("   ").is_empty());
        assert!(data.search("zzz").is_empty());
    }

    #[test]
    fn favorites_are_marked_listed_with_paths_and_survive_updates() {
        let (mut data, ops, _net, script) = sample();
        let top = data.save_script(None, None, "alpha", "echo a", false, 1, Some(true)).unwrap();
        assert_eq!(data.favorites().len(), 1);
        data.set_favorite(&script, true).unwrap();
        let favorites = data.favorites();
        assert_eq!(
            favorites.iter().map(|f| (f.name.as_str(), f.folder_path.as_str())).collect::<Vec<_>>(),
            vec![("alpha", ""), ("Check routes", "Ops / Network")]
        );
        assert!(data.get(&script).unwrap().favorite);
        // updating without a flag keeps the mark; an explicit flag changes it
        data.save_script(Some(&script), Some(&ops), "Routes", "ip r", false, 2, None).unwrap();
        assert!(data.get(&script).unwrap().favorite);
        data.save_script(Some(&script), Some(&ops), "Routes", "ip r", false, 3, Some(false)).unwrap();
        assert!(!data.get(&script).unwrap().favorite);
        // overwriting by name follows the same rule
        data.save_script(None, None, "ALPHA", "echo b", true, 4, None).unwrap();
        assert!(data.get(&top).unwrap().favorite);
        data.set_favorite(&top, false).unwrap();
        assert!(data.favorites().is_empty());
        assert_eq!(data.set_favorite("nope", true), Err(ScriptError::NotFound));
        // folders cannot be favorites
        assert_eq!(data.set_favorite(&ops, true), Err(ScriptError::NotFound));
        // deleting removes the favorite with the script
        data.set_favorite(&top, true).unwrap();
        data.delete(&top).unwrap();
        assert!(data.favorites().is_empty());
    }

    #[test]
    fn old_libraries_without_the_favorite_field_still_load() {
        let json = r#"{"version":1,"tree":[{"type":"script","id":"a","name":"n","body":"b","updated_at":1}]}"#;
        let data: ScriptsData = serde_json::from_str(json).unwrap();
        assert!(!data.get("a").unwrap().favorite);
    }

    #[test]
    fn summaries_hide_bodies_and_json_roundtrips_with_defaults() {
        let (data, _ops, _net, _script) = sample();
        let json = serde_json::to_string(&data.summaries()).unwrap();
        assert!(!json.contains("ip route"));
        assert!(json.contains("\"size\":"));
        let restored: ScriptsData = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(restored, data);
        let empty: ScriptsData = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ScriptsData::default());
    }
}
