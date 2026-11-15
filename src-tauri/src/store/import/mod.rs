//! Import from other SSH clients (M4).
//!
//! Parsing is always a separate, read-only step (`import_preview`) so the UI
//! can show exactly what will be created, including skipped entries and their
//! reasons. `import_apply` re-parses and writes; it never silently overwrites
//! an existing connection.

pub mod ssh_config;
pub mod winsshterm;
#[cfg(windows)]
pub mod putty;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::store::tree;
use crate::vault::{Auth, Connection, VaultData};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCandidate {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    /// Where the entry came from, for the preview list.
    pub label: String,
    /// Folder names from the source hierarchy, outermost first. Empty for
    /// sources that have no folders (`~/.ssh/config`, PuTTY).
    #[serde(default)]
    pub folder_path: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skipped {
    pub label: String,
    /// Short code translated by the frontend.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub source: String,
    pub candidates: Vec<ImportCandidate>,
    pub skipped: Vec<Skipped>,
    pub notices: Vec<String>,
}

impl ImportPreview {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            candidates: Vec::new(),
            skipped: Vec::new(),
            notices: Vec::new(),
        }
    }

    pub fn skip(&mut self, label: impl Into<String>, reason: &str) {
        self.skipped.push(Skipped { label: label.into(), reason: reason.into() });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub created: usize,
    /// Folders created to reproduce the source hierarchy. Existing folders
    /// with the same name are reused, so repeat imports report zero.
    pub folders_created: usize,
    pub skipped: Vec<Skipped>,
}

/// User name to use when the source does not specify one. SSH needs a user in
/// the `user@host` argument; OpenSSH would otherwise use the local account.
pub fn default_user() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "root".to_string())
}

pub fn home_dir() -> Result<std::path::PathBuf, &'static str> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(std::path::PathBuf::from)
        .map_err(|_| "home_not_found")
}

/// Resolves `~` and relative key paths against the directory of the source
/// file, without adding a dependency on a home-directory crate.
pub fn expand_path(raw: &str, base: &std::path::Path) -> String {
    let raw = raw.trim().trim_matches('"');
    let path = std::path::Path::new(raw);
    if let Ok(rest) = path.strip_prefix("~") {
        if let Ok(home) = home_dir() {
            return home.join(rest).display().to_string();
        }
        return raw.to_string();
    }
    if path.is_absolute() {
        raw.to_string()
    } else {
        base.join(path).display().to_string()
    }
}

pub fn preview(source: &str, path: Option<&str>) -> Result<ImportPreview, &'static str> {
    match source {
        "ssh_config" => ssh_config::parse(path),
        "putty" => putty_parse(),
        "winsshterm" => winsshterm::parse(path),
        _ => Err("unknown_import_source"),
    }
}

#[cfg(windows)]
fn putty_parse() -> Result<ImportPreview, &'static str> {
    putty::parse()
}

#[cfg(not(windows))]
fn putty_parse() -> Result<ImportPreview, &'static str> {
    Err("platform_unsupported")
}

/// Writes the candidates into the vault. `policy` is `"skip"` (default) or
/// `"copy"`; both avoid overwriting an existing `(host, port, user)` triple.
/// Each candidate's `folder_path` is recreated under `parent_id` (or at the
/// tree root), with same-named folders reused across candidates and imports.
pub fn apply(
    data: &mut VaultData,
    preview: ImportPreview,
    parent_id: Option<&str>,
    policy: &str,
) -> Result<ImportReport, &'static str> {
    let mut skipped = preview.skipped;
    let mut created = 0usize;
    let mut folders_created = 0usize;

    for candidate in preview.candidates {
        let duplicate = data.connections.values().any(|c| {
            c.host.eq_ignore_ascii_case(&candidate.host)
                && c.port == candidate.port
                && c.user.eq_ignore_ascii_case(&candidate.user)
        });
        if duplicate {
            if policy != "copy" {
                skipped.push(Skipped {
                    label: candidate.label.clone(),
                    reason: "duplicate".into(),
                });
                continue;
            }
        }
        let name = if duplicate {
            unique_name(&data.connections, &candidate.name)
        } else {
            candidate.name.clone()
        };
        let id = tree::new_id("c");
        let conn = Connection {
            id: id.clone(),
            name,
            host: candidate.host.clone(),
            port: candidate.port,
            user: candidate.user.clone(),
            auth: candidate.auth.clone(),
            ..Connection::default()
        };
        data.connections.insert(id.clone(), conn);
        let destination = if candidate.folder_path.is_empty() {
            parent_id.map(str::to_string)
        } else {
            let (leaf, added) =
                tree::ensure_path(&mut data.tree, parent_id, &candidate.folder_path);
            folders_created += added;
            leaf.or_else(|| parent_id.map(str::to_string))
        };
        tree::link(&mut data.tree, &id, destination.as_deref());
        created += 1;
    }

    tree::reconcile(&mut data.tree, &data.connections);
    Ok(ImportReport { created, folders_created, skipped })
}

fn unique_name(connections: &BTreeMap<String, Connection>, base: &str) -> String {
    let taken = |name: &str| connections.values().any(|c| c.name == name);
    if !taken(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}_{n}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::TreeNode;

    fn candidate(name: &str, host: &str) -> ImportCandidate {
        ImportCandidate {
            name: name.into(),
            host: host.into(),
            port: 22,
            user: "admin".into(),
            auth: Auth::Agent,
            label: name.into(),
            folder_path: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Synthetic copy of a WinSSHTerm export: all values are invented.
    const NESTED_XML: &str = r#"<WinSSHTerm>
  <Node Name="root" Type="Connection" Hostname="10.0.0.1" Username="u" />
  <Node Name="Production" Type="Container">
    <Node Name="db-01" Type="Connection" Hostname="10.0.0.5" Username="u" />
    <Node Name="Edge" Type="Container">
      <Node Name="gw-01" Type="Connection" Hostname="10.0.0.9" Username="u" />
    </Node>
  </Node>
</WinSSHTerm>"#;

    #[test]
    fn apply_skips_duplicates_by_default() {
        let mut data = VaultData::default();
        let mut preview = ImportPreview::new("test");
        preview.candidates.push(candidate("db-01", "10.0.0.5"));
        apply(&mut data, preview.clone(), None, "skip").unwrap();
        let report = apply(&mut data, preview, None, "skip").unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, "duplicate");
        assert_eq!(data.connections.len(), 1);
    }

    #[test]
    fn apply_with_copy_policy_renames_the_duplicate() {
        let mut data = VaultData::default();
        let mut preview = ImportPreview::new("test");
        preview.candidates.push(candidate("db-01", "10.0.0.5"));
        apply(&mut data, preview.clone(), None, "skip").unwrap();
        let report = apply(&mut data, preview, None, "copy").unwrap();
        assert_eq!(report.created, 1);
        assert_eq!(data.connections.len(), 2);
        assert!(data.connections.values().any(|c| c.name == "db-01_2"));
    }

    #[test]
    fn every_imported_connection_is_linked_exactly_once() {
        let mut data = VaultData::default();
        let mut preview = ImportPreview::new("test");
        preview.candidates.push(candidate("a", "h1"));
        preview.candidates.push(candidate("b", "h2"));
        apply(&mut data, preview, None, "skip").unwrap();
        assert_eq!(tree::paths(&data.tree).len(), 2);
        assert!(matches!(data.tree[0], TreeNode::Folder { .. }));
    }

    #[test]
    fn apply_recreates_the_winsshterm_folder_hierarchy() {
        let mut data = VaultData::default();
        let preview = winsshterm::parse_text(NESTED_XML);
        assert_eq!(preview.candidates.len(), 3);
        let report = apply(&mut data, preview, None, "skip").unwrap();
        assert_eq!(report.created, 3);
        assert_eq!(report.folders_created, 2);

        let id_of = |host: &str| {
            data.connections
                .values()
                .find(|c| c.host == host)
                .unwrap()
                .id
                .clone()
        };
        let paths = tree::paths(&data.tree);
        assert_eq!(paths[&id_of("10.0.0.5")], vec!["Production"]);
        assert_eq!(paths[&id_of("10.0.0.9")], vec!["Production", "Edge"]);
        // Connections without a folder still land in the default folder.
        assert_eq!(
            paths[&id_of("10.0.0.1")],
            vec![tree::DEFAULT_FOLDER_NAME.to_string()]
        );
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn a_second_import_reuses_the_same_folders() {
        let mut data = VaultData::default();
        apply(&mut data, winsshterm::parse_text(NESTED_XML), None, "skip").unwrap();
        let before = tree::paths(&data.tree).len();
        let report = apply(&mut data, winsshterm::parse_text(NESTED_XML), None, "copy").unwrap();
        assert_eq!(report.created, 3);
        assert_eq!(report.folders_created, 0);
        assert_eq!(tree::paths(&data.tree).len(), before + 3);
    }

    #[test]
    fn expand_path_handles_home_and_relative_paths() {
        let base = std::path::Path::new("C:/Users/me/.ssh");
        // Windows joins with backslashes; compare in a separator-neutral way.
        let relative = expand_path("keys/id", base).replace('\\', "/");
        assert!(relative.ends_with(".ssh/keys/id"), "got {relative}");
        assert_eq!(expand_path("C:/keys/id", base), "C:/keys/id");
        assert!(expand_path("~/.ssh/id_ed25519", base).contains("id_ed25519"));
    }
}
