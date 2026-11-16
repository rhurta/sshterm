//! Connection and tree store (M4).
//!
//! All commands work on the unlocked vault in memory and persist with
//! `Vault::save()`. The typed payload and pure tree algorithms live in
//! `vault/model.rs` and `store/tree.rs`; importing has its own submodule.

pub mod import;
pub mod tree;

use tauri::State;

use crate::commands::VaultState;
use crate::transport::PortForward;
use crate::vault::{Auth, Connection, TreeNode, Tunnel, Vault};

/// What the sidebar shows: never includes a password or key path.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionSummary {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Saved connection IDs in the configured jump-host order.
    pub jump: Vec<String>,
    pub tunnel_count: usize,
}

impl From<&Connection> for ConnectionSummary {
    fn from(c: &Connection) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            host: c.host.clone(),
            port: c.port,
            user: c.user.clone(),
            jump: c.jump.clone(),
            tunnel_count: c.tunnels.len(),
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

fn valid_jump_host(host: &str) -> bool {
    let raw = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    if raw.contains(':') {
        return raw.parse::<std::net::Ipv6Addr>().is_ok();
    }
    !raw.is_empty()
        && !raw.starts_with('-')
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

fn valid_jump_user(user: &str) -> bool {
    !user.is_empty()
        && user.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '+' | '\\')
        })
}

/// Resolves the configured jump hosts into the exact OpenSSH traversal order.
/// Nested jump configurations are flattened from the outermost host inward.
/// Duplicate hops, missing IDs, cycles, and excessively deep chains are rejected
/// before any child process is spawned.
pub fn resolve_jump_chain(
    connections: &std::collections::BTreeMap<String, Connection>,
    connection_id: &str,
) -> Result<Vec<Connection>, &'static str> {
    let target = connections.get(connection_id).ok_or("connection_not_found")?;
    fn append(
        connections: &std::collections::BTreeMap<String, Connection>,
        root_id: &str,
        hop_id: &str,
        active: &mut std::collections::HashSet<String>,
        included: &mut std::collections::HashSet<String>,
        output: &mut Vec<Connection>,
    ) -> Result<(), &'static str> {
        if hop_id == root_id || active.contains(hop_id) {
            return Err("jump_cycle");
        }
        if included.contains(hop_id) {
            return Err("jump_duplicate");
        }
        if active.len() >= 16 {
            return Err("jump_too_deep");
        }
        let hop = connections.get(hop_id).ok_or("jump_not_found")?.clone();
        if hop.port == 0 || !valid_jump_user(&hop.user) || !valid_jump_host(&hop.host) {
            return Err("jump_bad_address");
        }
        active.insert(hop_id.to_string());
        for nested in &hop.jump {
            append(connections, root_id, nested, active, included, output)?;
        }
        active.remove(hop_id);
        included.insert(hop_id.to_string());
        output.push(hop);
        Ok(())
    }

    let mut output = Vec::new();
    let mut active = std::collections::HashSet::new();
    let mut included = std::collections::HashSet::new();
    for hop_id in &target.jump {
        append(
            connections,
            connection_id,
            hop_id,
            &mut active,
            &mut included,
            &mut output,
        )?;
    }
    Ok(output)
}

fn validate_jump_chains(
    connections: &std::collections::BTreeMap<String, Connection>,
) -> Result<(), &'static str> {
    for id in connections.keys() {
        resolve_jump_chain(connections, id)?;
    }
    Ok(())
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

/// Parse and canonicalize a TCP endpoint. Listener endpoints may be a
/// bare port (OpenSSH's loopback default) or host:port; destinations require
/// host:port. IPv6 literals must use brackets, e.g. [::1]:5432.
fn normalize_endpoint(value: &str, allow_port_only: bool) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err("tunnel_bad_endpoint");
    }

    if !value.contains(':') {
        if !allow_port_only {
            return Err("tunnel_bad_endpoint");
        }
        let port = value.parse::<u16>().map_err(|_| "tunnel_bad_endpoint")?;
        if port == 0 {
            return Err("tunnel_bad_endpoint");
        }
        return Ok(port.to_string());
    }

    let (host, port_text) = if let Some(bracketed) = value.strip_prefix('[') {
        let end = bracketed.find(']').ok_or("tunnel_bad_endpoint")?;
        let host = &bracketed[..end];
        let tail = bracketed[end + 1..]
            .strip_prefix(':')
            .ok_or("tunnel_bad_endpoint")?;
        let ip = host.parse::<std::net::IpAddr>().map_err(|_| "tunnel_bad_endpoint")?;
        if !ip.is_ipv6() {
            return Err("tunnel_bad_endpoint");
        }
        (format!("[{host}]"), tail)
    } else {
        let (host, port) = value.split_once(':').ok_or("tunnel_bad_endpoint")?;
        if host.is_empty()
            || host.chars().any(|ch| {
                !ch.is_ascii_alphanumeric() && !matches!(ch, '.' | '-' | '_')
            })
            || host.starts_with('-')
        {
            return Err("tunnel_bad_endpoint");
        }
        (host.to_string(), port)
    };

    if port_text.is_empty() || port_text.contains(':') {
        return Err("tunnel_bad_endpoint");
    }
    let port = port_text.parse::<u16>().map_err(|_| "tunnel_bad_endpoint")?;
    if port == 0 {
        return Err("tunnel_bad_endpoint");
    }
    Ok(format!("{host}:{port}"))
}

/// Validate and normalize the saved local/reverse tunnel definitions.
fn normalize_tunnels(tunnels: &mut [Tunnel]) -> Result<(), &'static str> {
    let mut listeners = std::collections::HashSet::<(&str, String)>::new();
    for tunnel in tunnels {
        match tunnel {
            Tunnel::Local { bind, dest } => {
                *bind = normalize_endpoint(bind, true)?;
                *dest = normalize_endpoint(dest, false)?;
                if !listeners.insert(("local", bind.to_ascii_lowercase())) {
                    return Err("tunnel_duplicate_listener");
                }
            }
            Tunnel::Remote { bind, dest } => {
                *bind = normalize_endpoint(bind, true)?;
                *dest = normalize_endpoint(dest, false)?;
                if !listeners.insert(("remote", bind.to_ascii_lowercase())) {
                    return Err("tunnel_duplicate_listener");
                }
            }
            Tunnel::Dynamic { .. } => return Err("tunnel_dynamic_unsupported"),
        }
    }
    Ok(())
}

/// Converts the stored tunnel model to the OpenSSH flags for session startup.
/// This validates again because older vaults may contain unvalidated entries.
pub fn resolve_forwards(tunnels: &[Tunnel]) -> Result<Vec<PortForward>, &'static str> {
    let mut normalized = tunnels.to_vec();
    normalize_tunnels(&mut normalized)?;
    Ok(normalized
        .into_iter()
        .map(|tunnel| match tunnel {
            Tunnel::Local { bind, dest } => PortForward::Local { bind, destination: dest },
            Tunnel::Remote { bind, dest } => PortForward::Remote { bind, destination: dest },
            Tunnel::Dynamic { .. } => unreachable!("dynamic tunnels rejected above"),
        })
        .collect())
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
    normalize_tunnels(&mut c.tunnels)?;
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
    let mut candidate = vault.data.connections.clone();
    candidate.insert(id.clone(), conn);
    validate_jump_chains(&candidate).map_err(str::to_string)?;
    vault.data.connections = candidate;
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
    fn normalizes_local_and_reverse_tunnel_endpoints() {
        let connection = normalize(Connection {
            id: "tunnels".into(),
            name: "tunnels".into(),
            host: "server".into(),
            user: "user".into(),
            tunnels: vec![
                Tunnel::Local { bind: "127.0.0.1:015432".into(), dest: "db.internal:5432".into() },
                Tunnel::Remote { bind: "18080".into(), dest: "[::1]:8080".into() },
            ],
            ..Connection::default()
        }).unwrap();
        assert_eq!(connection.tunnels, vec![
            Tunnel::Local { bind: "127.0.0.1:15432".into(), dest: "db.internal:5432".into() },
            Tunnel::Remote { bind: "18080".into(), dest: "[::1]:8080".into() },
        ]);
        let forwards = resolve_forwards(&connection.tunnels).unwrap();
        assert_eq!(forwards[0], PortForward::Local {
            bind: "127.0.0.1:15432".into(),
            destination: "db.internal:5432".into(),
        });
        assert_eq!(forwards[1], PortForward::Remote {
            bind: "18080".into(),
            destination: "[::1]:8080".into(),
        });
    }

    #[test]
    fn rejects_invalid_or_duplicate_tunnel_listeners() {
        let invalid = normalize(Connection {
            id: "bad".into(), host: "server".into(), user: "user".into(),
            tunnels: vec![Tunnel::Local { bind: "127.0.0.1:0".into(), dest: "db:5432".into() }],
            ..Connection::default()
        });
        assert_eq!(invalid.unwrap_err(), "tunnel_bad_endpoint");

        let duplicate = normalize(Connection {
            id: "duplicate".into(), host: "server".into(), user: "user".into(),
            tunnels: vec![
                Tunnel::Local { bind: "127.0.0.1:15432".into(), dest: "db1:5432".into() },
                Tunnel::Local { bind: "127.0.0.1:15432".into(), dest: "db2:5432".into() },
            ],
            ..Connection::default()
        });
        assert_eq!(duplicate.unwrap_err(), "tunnel_duplicate_listener");
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
    fn jump_chain_resolves_in_order_and_flattens_nested_hops() {
        let mut connections = std::collections::BTreeMap::new();
        let leaf = Connection {
            id: "leaf".into(),
            name: "edge".into(),
            host: "edge.internal".into(),
            user: "edge-user".into(),
            ..Connection::default()
        };
        let middle = Connection {
            id: "middle".into(),
            name: "bastion".into(),
            host: "bastion.internal".into(),
            user: "bastion-user".into(),
            jump: vec!["leaf".into()],
            ..Connection::default()
        };
        let target = Connection {
            id: "target".into(),
            name: "database".into(),
            host: "db.internal".into(),
            user: "db-user".into(),
            jump: vec!["middle".into()],
            ..Connection::default()
        };
        connections.insert(leaf.id.clone(), leaf);
        connections.insert(middle.id.clone(), middle);
        connections.insert(target.id.clone(), target);

        let chain = resolve_jump_chain(&connections, "target").unwrap();
        assert_eq!(chain.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["leaf", "middle"]);
    }

    #[test]
    fn jump_chain_rejects_invalid_proxyjump_syntax() {
        let mut connections = std::collections::BTreeMap::new();
        connections.insert("target".into(), Connection {
            id: "target".into(), name: "target".into(), host: "target".into(), user: "u".into(),
            jump: vec!["bad".into()], ..Connection::default()
        });
        connections.insert("bad".into(), Connection {
            id: "bad".into(), name: "bad".into(), host: "host,other".into(), user: "u".into(),
            ..Connection::default()
        });
        assert_eq!(resolve_jump_chain(&connections, "target").unwrap_err(), "jump_bad_address");
        connections.get_mut("bad").unwrap().host = "host.example".into();
        connections.get_mut("bad").unwrap().user = "user@other".into();
        assert_eq!(resolve_jump_chain(&connections, "target").unwrap_err(), "jump_bad_address");
    }

    #[test]
    fn jump_chain_rejects_missing_duplicate_and_cyclic_hops() {
        let mut connections = std::collections::BTreeMap::new();
        connections.insert("a".into(), Connection {
            id: "a".into(), name: "a".into(), host: "a".into(), user: "u".into(),
            jump: vec!["b".into()], ..Connection::default()
        });
        connections.insert("b".into(), Connection {
            id: "b".into(), name: "b".into(), host: "b".into(), user: "u".into(),
            jump: vec!["a".into()], ..Connection::default()
        });
        assert_eq!(resolve_jump_chain(&connections, "a").unwrap_err(), "jump_cycle");

        connections.get_mut("a").unwrap().jump = vec!["missing".into()];
        assert_eq!(resolve_jump_chain(&connections, "a").unwrap_err(), "jump_not_found");

        connections.get_mut("a").unwrap().jump = vec!["b".into(), "b".into()];
        connections.get_mut("b").unwrap().jump.clear();
        assert_eq!(resolve_jump_chain(&connections, "a").unwrap_err(), "jump_duplicate");
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
