//! Connection and tree store (M4).
//!
//! All commands work on the unlocked vault in memory and persist with
//! `Vault::save()`. The typed payload and pure tree algorithms live in
//! `vault/model.rs` and `store/tree.rs`; importing has its own submodule.

pub mod import;
pub mod tree;

use tauri::State;

use crate::commands::VaultState;
use crate::vault::{Auth, Connection, TreeNode, Vault};

/// What the sidebar shows: never includes a password or key path.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionSummary {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl From<&Connection> for ConnectionSummary {
    fn from(c: &Connection) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            host: c.host.clone(),
            port: c.port,
            user: c.user.clone(),
        }
    }
}

fn guard<'r>(state: &'r State<'r, VaultState>) -> std::sync::MutexGuard<'r, Option<Vault>> {
    // `State` holds a `&'r VaultState` reference; the MutexGuard lives as long
    // as that reference, not as long as the borrow of `state`.
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn find(vault: &Vault, id: &str) -> Option<Connection> {
    vault.data.connections.get(id).cloned()
}

/// Migrates M1-M3 payloads in memory: `password` becomes `auth::password` and
/// the flat connection list is wrapped in one "Connections" folder. Returns
/// true when the vault must be saved again.
pub fn migrate(vault: &mut Vault) -> bool {
    let mut changed = false;
    for conn in vault.data.connections.values_mut() {
        if conn.migrate_legacy() {
            changed = true;
        }
    }
    if tree::reconcile(&mut vault.data.tree, &vault.data.connections) {
        changed = true;
    }
    changed
}

fn normalize(mut c: Connection) -> Result<Connection, &'static str> {
    c.id = c.id.trim().to_string();
    c.name = c.name.trim().to_string();
    c.host = c.host.trim().to_string();
    c.user = c.user.trim().to_string();
    if c.id.is_empty() || c.host.is_empty() || c.user.is_empty() || c.port == 0 {
        return Err("bad_connection");
    }
    if c.name.is_empty() {
        c.name = c.host.clone();
    }
    if c.keepalive_secs == 0 {
        c.keepalive_secs = 30;
    }
    // Consume a legacy password field if the editor sent one back unchanged.
    c.migrate_legacy();
    c.legacy_password = None;
    // Empty credentials mean "no password": ssh uses a key or the agent.
    match &c.auth {
        Auth::Password { password } if password.is_empty() => c.auth = Auth::Agent,
        Auth::Key { path, .. } if path.trim().is_empty() => c.auth = Auth::Agent,
        _ => {}
    }
    Ok(c)
}

#[tauri::command]
pub fn conn_list(state: State<'_, VaultState>) -> Result<Vec<ConnectionSummary>, String> {
    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    let mut out: Vec<ConnectionSummary> = vault
        .data
        .connections
        .values()
        .map(ConnectionSummary::from)
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[tauri::command]
pub fn conn_get(state: State<'_, VaultState>, id: String) -> Result<Option<Connection>, String> {
    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    Ok(find(vault, &id))
}

#[tauri::command]
pub fn conn_upsert(
    state: State<'_, VaultState>,
    conn: Connection,
    parent_id: Option<String>,
) -> Result<(), String> {
    let conn = normalize(conn).map_err(str::to_string)?;
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    let id = conn.id.clone();
    vault.data.connections.insert(id.clone(), conn);
    tree::link(&mut vault.data.tree, &id, parent_id.as_deref());
    tree::reconcile(&mut vault.data.tree, &vault.data.connections);
    vault.save().map_err(|e| e.code().to_string())
}

#[tauri::command]
pub fn conn_delete(state: State<'_, VaultState>, id: String) -> Result<(), String> {
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    vault.data.connections.remove(&id);
    tree::unlink(&mut vault.data.tree, &id);
    for conn in vault.data.connections.values_mut() {
        conn.jump.retain(|hop| hop != &id);
    }
    tree::reconcile(&mut vault.data.tree, &vault.data.connections);
    vault.save().map_err(|e| e.code().to_string())
}

#[tauri::command]
pub fn tree_get(state: State<'_, VaultState>) -> Result<Vec<TreeNode>, String> {
    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    Ok(vault.data.tree.clone())
}

#[tauri::command]
pub fn folder_upsert(
    state: State<'_, VaultState>,
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
) -> Result<String, String> {
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    let result = tree::upsert_folder(&mut vault.data.tree, id, parent_id, name)
        .map_err(str::to_string)?;
    vault.save().map_err(|e| e.code().to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn folder_delete(
    state: State<'_, VaultState>,
    id: String,
    cascade: bool,
) -> Result<(), String> {
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    let ids = tree::delete_folder(&mut vault.data.tree, &id, cascade)
        .map_err(str::to_string)?;
    for connection_id in ids {
        vault.data.connections.remove(&connection_id);
        for conn in vault.data.connections.values_mut() {
            conn.jump.retain(|hop| hop != &connection_id);
        }
    }
    tree::reconcile(&mut vault.data.tree, &vault.data.connections);
    vault.save().map_err(|e| e.code().to_string())
}

#[tauri::command]
pub fn move_node(
    state: State<'_, VaultState>,
    id: String,
    parent_id: Option<String>,
    index: usize,
) -> Result<(), String> {
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    tree::move_node(&mut vault.data.tree, &id, parent_id.as_deref(), index)
        .map_err(str::to_string)?;
    tree::reconcile(&mut vault.data.tree, &vault.data.connections);
    vault.save().map_err(|e| e.code().to_string())
}

#[tauri::command]
pub fn search(state: State<'_, VaultState>, query: String) -> Result<Vec<tree::SearchHit>, String> {
    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    let paths = tree::paths(&vault.data.tree);
    Ok(tree::search(&vault.data.connections, &paths, &query))
}

/// Read-only parse. The dialog shows candidates, skips and warnings before
/// anything is written to the vault.
#[tauri::command]
pub fn import_preview(
    source: String,
    path: Option<String>,
) -> Result<import::ImportPreview, String> {
    import::preview(&source, path.as_deref()).map_err(str::to_string)
}

#[tauri::command]
pub fn import_apply(
    state: State<'_, VaultState>,
    source: String,
    path: Option<String>,
    parent_id: Option<String>,
    policy: String,
) -> Result<import::ImportReport, String> {
    let preview = import::preview(&source, path.as_deref()).map_err(str::to_string)?;
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    let report = import::apply(&mut vault.data, preview, parent_id.as_deref(), &policy)
        .map_err(str::to_string)?;
    vault.save().map_err(|e| e.code().to_string())?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::VaultData;

    #[test]
    fn normalize_fills_defaults_and_keeps_password_auth() {
        let c = Connection {
            id: " c1 ".into(),
            name: "  ".into(),
            host: " 10.0.0.5 ".into(),
            user: " admin ".into(),
            auth: Auth::Password { password: "secret".into() },
            keepalive_secs: 0,
            ..Connection::default()
        };
        let c = normalize(c).unwrap();
        assert_eq!(c.id, "c1");
        assert_eq!(c.name, "10.0.0.5");
        assert_eq!(c.host, "10.0.0.5");
        assert_eq!(c.keepalive_secs, 30);
        assert_eq!(c.password().as_deref(), Some("secret"));
    }

    #[test]
    fn normalize_converts_a_legacy_password_and_clears_the_field() {
        let c = Connection {
            id: "c1".into(),
            name: "old".into(),
            host: "h".into(),
            user: "u".into(),
            legacy_password: Some("legacy".into()),
            ..Connection::default()
        };
        let c = normalize(c).unwrap();
        assert_eq!(c.auth, Auth::Password { password: "legacy".into() });
        assert!(c.legacy_password.is_none());
        assert_eq!(c.password().as_deref(), Some("legacy"));
    }

    #[test]
    fn normalize_rejects_incomplete_connections() {
        let base = Connection::default();
        for bad in [
            Connection { host: "".into(), ..base.clone() },
            Connection { user: "".into(), ..base.clone() },
            Connection { port: 0, ..base.clone() },
            Connection { id: "".into(), ..base.clone() },
        ] {
            assert_eq!(normalize(bad).unwrap_err(), "bad_connection");
        }
    }

    #[test]
    fn empty_credentials_become_agent_auth() {
        for auth in [
            Auth::Password { password: "".into() },
            Auth::Key { path: "  ".into(), passphrase: None },
        ] {
            let c = normalize(Connection {
                id: "c1".into(),
                host: "h".into(),
                user: "u".into(),
                auth,
                ..Connection::default()
            })
            .unwrap();
            assert_eq!(c.auth, Auth::Agent);
            assert!(c.password().is_none());
        }
    }

    #[test]
    fn summary_never_contains_a_password() {
        let summary = ConnectionSummary::from(&Connection {
            id: "c1".into(),
            name: "srv".into(),
            host: "h".into(),
            user: "u".into(),
            auth: Auth::Password { password: "secret".into() },
            ..Connection::default()
        });
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("password"));
    }

    #[test]
    fn migration_wraps_a_flat_payload() {
        let mut data = VaultData::default();
        data.connections.insert(
            "c1".into(),
            Connection {
                id: "c1".into(),
                name: "old".into(),
                host: "h".into(),
                user: "u".into(),
                legacy_password: Some("pw".into()),
                ..Connection::default()
            },
        );
        // Exercise the same pure pieces `migrate()` uses: a real Vault needs
        // an encrypted file on disk, which the vault tests already cover.
        for conn in data.connections.values_mut() {
            conn.migrate_legacy();
        }
        tree::reconcile(&mut data.tree, &data.connections);
        assert!(matches!(data.tree[0], TreeNode::Folder { .. }));
        assert_eq!(data.connections["c1"].password().as_deref(), Some("pw"));
    }
}
