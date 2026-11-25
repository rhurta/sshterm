//! Connection and tree store (M4).
//!
//! All commands work on the unlocked vault in memory and persist with
//! `Vault::save()`. The typed payload and pure tree algorithms live in
//! `vault/model.rs` and `store/tree.rs`; importing has its own submodule.

pub mod import;
pub mod tree;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use tauri::State;

use crate::commands::VaultState;
use crate::transport::PortForward;
use crate::vault::{Auth, Connection, TreeNode, Tunnel, Vault, VaultData};

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
    /// Configured jump IDs currently disabled by the user.
    pub jump_disabled: Vec<String>,
    pub tunnel_count: usize,
    pub protected: bool,
    pub identity_color: Option<String>,
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
            jump_disabled: c.jump_disabled.clone(),
            tunnel_count: c.tunnels.iter().filter(|t| match t {
                Tunnel::Local { enabled, .. }
                | Tunnel::Remote { enabled, .. }
                | Tunnel::Dynamic { enabled, .. } => *enabled,
            }).count(),
            protected: c.protected,
            identity_color: c.identity_color.clone(),
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

/// Host and user end up in ssh's argv and in the generated ssh config, so they
/// must never be able to look like an option (`-oProxyCommand=...`) or carry
/// separators. Applies to the connection itself, not just to jump hops.
pub fn valid_address(user: &str, host: &str) -> bool {
    valid_jump_user(user) && valid_jump_host(host)
}

fn valid_jump_user(user: &str) -> bool {
    !user.is_empty()
        && !user.starts_with('-')
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
            if hop.jump_disabled.iter().any(|disabled| disabled == nested) {
                continue;
            }
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
        if target.jump_disabled.iter().any(|disabled| disabled == hop_id) {
            continue;
        }
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

/// Validate and normalize the saved tunnel definitions (local, reverse, dynamic).
fn normalize_tunnels(tunnels: &mut [Tunnel]) -> Result<(), &'static str> {
    let mut listeners = std::collections::HashSet::<(&str, String)>::new();
    for tunnel in tunnels {
        match tunnel {
            Tunnel::Local { bind, dest, enabled } => {
                if !*enabled {
                    continue;
                }
                *bind = normalize_endpoint(bind, true)?;
                *dest = normalize_endpoint(dest, false)?;
                if !listeners.insert(("local", bind.to_ascii_lowercase())) {
                    return Err("tunnel_duplicate_listener");
                }
            }
            Tunnel::Dynamic { bind, enabled } => {
                if !*enabled {
                    continue;
                }
                *bind = normalize_endpoint(bind, true)?;
                // -D listens on this computer like -L, so they share listener keys.
                if !listeners.insert(("local", bind.to_ascii_lowercase())) {
                    return Err("tunnel_duplicate_listener");
                }
            }
            Tunnel::Remote { bind, dest, enabled } => {
                if !*enabled {
                    continue;
                }
                *bind = normalize_endpoint(bind, true)?;
                *dest = normalize_endpoint(dest, false)?;
                if !listeners.insert(("remote", bind.to_ascii_lowercase())) {
                    return Err("tunnel_duplicate_listener");
                }
            }
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
        .filter(|tunnel| match tunnel {
            Tunnel::Local { enabled, .. }
            | Tunnel::Remote { enabled, .. }
            | Tunnel::Dynamic { enabled, .. } => *enabled,
        })
        .map(|tunnel| match tunnel {
            Tunnel::Local { bind, dest, .. } => PortForward::Local { bind, destination: dest },
            Tunnel::Remote { bind, dest, .. } => PortForward::Remote { bind, destination: dest },
            Tunnel::Dynamic { bind, .. } => PortForward::Dynamic { bind },
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
    if !valid_address(&c.user, &c.host) {
        return Err("bad_address");
    }
    if c.name.is_empty() {
        c.name = c.host.clone();
    }
    // keepalive_secs == 0 is meaningful for ssh (ServerAliveInterval 0 = off),
    // so it is kept as the user typed it. New connections default to 30.
    normalize_tunnels(&mut c.tunnels)?;
    if c.highlight.len() > 32 || c.highlight.iter().any(|name| name.trim().is_empty() || name.len() > 128) {
        return Err("highlight_bad_assignment");
    }
    let mut seen_highlight = std::collections::HashSet::new();
    c.highlight.retain(|name| seen_highlight.insert(name.clone()));
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
        .map(|connection| {
            let mut summary = ConnectionSummary::from(connection);
            summary.identity_color = tree::resolve_identity_color(&vault.data.tree, &connection.id, connection.identity_color.as_deref());
            summary
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}


#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportReport {
    pub connections: usize,
    pub skipped: usize,
}

fn serialize_export(data: &VaultData) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec_pretty(data)
}

fn canonical_export_path(path: &Path) -> Result<PathBuf, String> {
    let name = path.file_name().ok_or_else(|| "bad_export_path".to_string())?;
    if path.exists() {
        return std::fs::canonicalize(path).map_err(|_| "io_error".to_string());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = std::fs::canonicalize(parent).map_err(|_| "io_error".to_string())?;
    Ok(parent.join(name))
}

fn same_export_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn validate_export_destination(destination: &Path) -> Result<(), String> {
    if destination.as_os_str().is_empty() {
        return Err("bad_export_path".into());
    }

    let destination_resolved = canonical_export_path(destination)?;
    let vault_path = crate::paths::vault_path();
    let vault_resolved = canonical_export_path(&vault_path)?;
    let scripts_vault_path = crate::paths::scripts_vault_path();
    // A configured scripts vault may live beneath a directory that has not
    // been created yet. Only resolve it when its parent exists; the save dialog
    // cannot select a file inside a missing directory.
    let scripts_vault_conflict = scripts_vault_path
        .parent()
        .filter(|parent| parent.exists())
        .and_then(|_| canonical_export_path(&scripts_vault_path).ok())
        .is_some_and(|path| same_export_path(&destination_resolved, &path));
    if same_export_path(&destination_resolved, &vault_resolved) || scripts_vault_conflict {
        return Err("export_overwrites_vault".into());
    }
    Ok(())
}

/// Generate a conservative OpenSSH Host alias from a connection name.
/// Wildcards, whitespace and config syntax are replaced rather than emitted.
fn safe_host_alias(value: &str) -> String {
    let mut alias = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            alias.push(ch);
            last_dash = false;
        } else if !last_dash {
            alias.push('-');
            last_dash = true;
        }
    }
    let alias = alias.trim_matches('-').to_string();
    if alias.is_empty() { "connection".into() } else { alias }
}

fn host_aliases(data: &VaultData) -> std::collections::BTreeMap<String, String> {
    let mut aliases = std::collections::BTreeMap::new();
    let mut used = std::collections::HashSet::new();
    for (id, connection) in &data.connections {
        let base = safe_host_alias(&connection.name);
        let mut alias = base.clone();
        if !used.insert(alias.to_ascii_lowercase()) {
            let suffix = safe_host_alias(id);
            alias = format!("{base}-{suffix}");
            let mut sequence = 2usize;
            while !used.insert(alias.to_ascii_lowercase()) {
                alias = format!("{base}-{suffix}-{sequence}");
                sequence += 1;
            }
        }
        aliases.insert(id.clone(), alias);
    }
    aliases
}

fn collect_ssh_config_order(
    nodes: &[TreeNode],
    folder_path: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    output: &mut Vec<(String, Vec<String>)>,
) {
    for node in nodes {
        match node {
            TreeNode::Folder { name, children, .. } => {
                folder_path.push(name.clone());
                collect_ssh_config_order(children, folder_path, seen, output);
                folder_path.pop();
            }
            TreeNode::Connection { id } if seen.insert(id.clone()) => {
                output.push((id.clone(), folder_path.clone()));
            }
            TreeNode::Connection { .. } => {}
        }
    }
}

fn ssh_config_quote(value: &str) -> String {
    #[cfg(windows)]
    let normalized = value.replace('\\', "/");
    #[cfg(not(windows))]
    let normalized = value.to_string();
    format!("\"{}\"", normalized.replace('"', "\\\""))
}

fn ssh_config_comment(value: &str) -> String {
    value.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Serialize the OpenSSH-compatible subset of a vault. Passwords, key
/// passphrases, host-key trust, and UI-only settings deliberately stay out.
fn serialize_ssh_config(data: &VaultData) -> (String, usize) {
    let aliases = host_aliases(data);
    let mut order = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_ssh_config_order(&data.tree, &mut Vec::new(), &mut seen, &mut order);
    for id in data.connections.keys() {
        if seen.insert(id.clone()) {
            order.push((id.clone(), Vec::new()));
        }
    }

    let mut output = String::from(
        "# OpenSSH client configuration exported by SSH Term.\n\
         # Saved passwords and key passphrases are not included.\n\
         # Host-key trust is not transferred; OpenSSH uses its own known_hosts file.\n\
         # Per-connection colors, highlighting and other UI-only settings are omitted.\n\n",
    );
    let mut skipped = 0usize;

    for (id, folders) in order {
        let Some(connection) = data.connections.get(&id) else { continue };
        if connection.port == 0 || !valid_address(&connection.user, &connection.host)
            || resolve_jump_chain(&data.connections, &id).is_err()
        {
            let label = ssh_config_comment(&connection.name);
            let _ = writeln!(output, "# Skipped {label}: invalid host or jump-host route.\n");
            skipped += 1;
            continue;
        }

        let Some(alias) = aliases.get(&id) else { continue };
        if !folders.is_empty() {
            let folder = ssh_config_comment(&folders.join(" / "));
            let _ = writeln!(output, "# Folder: {folder}");
        }
        let _ = writeln!(output, "Host {alias}");
        let _ = writeln!(output, "    HostName {}", connection.host);
        let _ = writeln!(output, "    User {}", connection.user);
        let _ = writeln!(output, "    Port {}", connection.port);
        let _ = writeln!(output, "    ServerAliveInterval {}", connection.keepalive_secs);

        if let Some(key_path) = connection.key_path().filter(|path| !path.chars().any(char::is_control)) {
            let _ = writeln!(output, "    IdentityFile {}", ssh_config_quote(&key_path));
            let _ = writeln!(output, "    IdentitiesOnly yes");
        }
        if matches!(&connection.auth, Auth::Password { .. }) {
            let _ = writeln!(output, "    PreferredAuthentications password,keyboard-interactive");
        }

        let mut jump_aliases = Vec::new();
        let mut jump_seen = std::collections::HashSet::new();
        for jump_id in &connection.jump {
            if connection.jump_disabled.iter().any(|disabled| disabled == jump_id) {
                continue;
            }
            if jump_seen.insert(jump_id) {
                if let Some(jump_alias) = aliases.get(jump_id) {
                    jump_aliases.push(jump_alias.as_str());
                }
            }
        }
        if !jump_aliases.is_empty() {
            let _ = writeln!(output, "    ProxyJump {}", jump_aliases.join(","));
        }

        for saved_tunnel in &connection.tunnels {
            let mut tunnel = saved_tunnel.clone();
            if normalize_tunnels(std::slice::from_mut(&mut tunnel)).is_err() {
                continue;
            }
            match tunnel {
                Tunnel::Local { bind, dest, enabled: true } => {
                    let _ = writeln!(output, "    LocalForward {bind} {dest}");
                }
                Tunnel::Remote { bind, dest, enabled: true } => {
                    let _ = writeln!(output, "    RemoteForward {bind} {dest}");
                }
                Tunnel::Dynamic { bind, enabled: true } => {
                    let _ = writeln!(output, "    DynamicForward {bind}");
                }
                _ => {}
            }
        }
        output.push('\n');
    }

    (output, skipped)
}

/// Exports the complete decrypted vault payload as plaintext JSON.
/// The frontend obtains the destination from the native save-file dialog.
#[tauri::command]
pub fn conn_export(state: State<'_, VaultState>, path: String) -> Result<ExportReport, String> {
    let destination = PathBuf::from(path.trim());
    validate_export_destination(&destination)?;

    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    let bytes = serialize_export(&vault.data).map_err(|_| "serialize_error".to_string())?;
    std::fs::write(&destination, bytes).map_err(|_| "io_error".to_string())?;

    Ok(ExportReport {
        connections: vault.data.connections.len(),
        skipped: 0,
    })
}


/// Exports SSH-compatible connection settings to an OpenSSH client config.
/// Secret credentials and local host-key trust are intentionally not exported.
#[tauri::command]
pub fn conn_export_ssh_config(
    state: State<'_, VaultState>,
    path: String,
) -> Result<ExportReport, String> {
    let destination = PathBuf::from(path.trim());
    validate_export_destination(&destination)?;

    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    let (config, skipped) = serialize_ssh_config(&vault.data);
    std::fs::write(&destination, config).map_err(|_| "io_error".to_string())?;

    Ok(ExportReport {
        connections: vault.data.connections.len().saturating_sub(skipped),
        skipped,
    })
}

#[tauri::command]
pub fn conn_get(state: State<'_, VaultState>, id: String) -> Result<Option<Connection>, String> {
    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    Ok(find(vault, &id))
}


#[tauri::command]
pub fn highlight_resolve(
    state: State<'_, VaultState>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    let connection = vault.data.connections.get(&connection_id).ok_or("connection_not_found")?;
    tree::resolve_highlight_rules(
        &vault.data.tree,
        &connection_id,
        &connection.highlight,
    ).ok_or("connection_not_in_tree".into())
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
        conn.jump_disabled.retain(|hop| hop != &id);
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


fn valid_identity_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

#[tauri::command]
pub fn conn_set_identity_color(state: State<'_, VaultState>, id: String, color: Option<String>) -> Result<(), String> {
    if color.as_deref().is_some_and(|value| !valid_identity_color(value)) { return Err("color_bad_value".into()); }
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    let connection = vault.data.connections.get_mut(&id).ok_or("connection_not_found")?;
    connection.identity_color = color;
    vault.save().map_err(|e| e.code().to_string())
}

#[tauri::command]
pub fn folder_set_color(state: State<'_, VaultState>, id: String, color: Option<String>) -> Result<(), String> {
    if color.as_deref().is_some_and(|value| !valid_identity_color(value)) { return Err("color_bad_value".into()); }
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    if !tree::set_folder_color(&mut vault.data.tree, &id, color) { return Err("folder_not_found".into()); }
    vault.save().map_err(|e| e.code().to_string())
}

#[tauri::command]
pub fn folder_set_highlight(
    state: State<'_, VaultState>,
    id: String,
    highlight: Vec<String>,
) -> Result<(), String> {
    if highlight.len() > 32 || highlight.iter().any(|name| name.trim().is_empty() || name.len() > 128) {
        return Err("highlight_bad_assignment".into());
    }
    let mut seen = std::collections::HashSet::new();
    let highlight: Vec<String> = highlight.into_iter().filter(|name| seen.insert(name.clone())).collect();
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    if !tree::set_folder_highlight(&mut vault.data.tree, &id, highlight) {
        return Err("folder_not_found".into());
    }
    vault.save().map_err(|e| e.code().to_string())
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
    fn connection_export_roundtrips_the_complete_vault_payload() {
        let mut data = VaultData::default();
        data.tree = vec![TreeNode::Folder {
            id: "folder".into(),
            name: "Production".into(),
            children: vec![TreeNode::Connection { id: "db".into() }],
            highlight: vec!["database".into()],
        }];
        data.connections.insert(
            "db".into(),
            Connection {
                id: "db".into(),
                name: "database".into(),
                host: "db.internal".into(),
                user: "admin".into(),
                auth: Auth::Password { password: "secret".into() },
                jump: vec!["bastion".into()],
                tunnels: vec![Tunnel::Local {
                    bind: "15432".into(),
                    dest: "db.internal:5432".into(),
                    enabled: true,
                }],
                protected: true,
                ..Connection::default()
            },
        );
        data.known_hosts.insert("db.internal:22".into(), "SHA256:trusted".into());

        let exported = serialize_export(&data).unwrap();
        let json = String::from_utf8(exported.clone()).unwrap();
        assert!(json.contains("secret"), "export includes saved authentication credentials");
        let restored: VaultData = serde_json::from_slice(&exported).unwrap();
        assert_eq!(restored, data);
    }


    #[test]
    fn openssh_export_includes_aliases_jumps_keys_and_enabled_forwards_but_no_secrets() {
        let mut data = VaultData::default();
        data.tree = vec![TreeNode::Folder {
            id: "prod".into(),
            name: "Production".into(),
            children: vec![
                TreeNode::Connection { id: "bastion".into() },
                TreeNode::Connection { id: "db".into() },
            ],
            highlight: Vec::new(),
        }];
        data.connections.insert(
            "bastion".into(),
            Connection {
                id: "bastion".into(),
                name: "Bastion Host".into(),
                host: "bastion.example.com".into(),
                user: "jump-user".into(),
                auth: Auth::Key {
                    path: r"C:\Users\me\.ssh\jump key".into(),
                    passphrase: Some("jump-passphrase".into()),
                },
                ..Connection::default()
            },
        );
        data.connections.insert(
            "db".into(),
            Connection {
                id: "db".into(),
                name: "Production Database".into(),
                host: "db.internal".into(),
                user: "db-user".into(),
                auth: Auth::Password { password: "database-password".into() },
                keepalive_secs: 15,
                jump: vec!["bastion".into()],
                tunnels: vec![
                    Tunnel::Local { bind: "127.0.0.1:15432".into(), dest: "db.internal:5432".into(), enabled: true },
                    Tunnel::Remote { bind: "18080".into(), dest: "127.0.0.1:8080".into(), enabled: true },
                    Tunnel::Dynamic { bind: "1080".into(), enabled: true },
                    Tunnel::Local { bind: "15433".into(), dest: "db.internal:5432".into(), enabled: false },
                ],
                ..Connection::default()
            },
        );

        let (config, skipped) = serialize_ssh_config(&data);
        assert_eq!(skipped, 0);
        assert!(config.contains("# Folder: Production"));
        assert!(config.contains("Host Bastion-Host"));
        #[cfg(windows)]
        assert!(config.contains("IdentityFile \"C:/Users/me/.ssh/jump key\""));
        #[cfg(not(windows))]
        assert!(config.contains(r#"IdentityFile "C:\Users\me\.ssh\jump key""#));
        assert!(config.contains("Host Production-Database"));
        assert!(config.contains("ProxyJump Bastion-Host"));
        assert!(config.contains("ServerAliveInterval 15"));
        assert!(config.contains("LocalForward 127.0.0.1:15432 db.internal:5432"));
        assert!(config.contains("RemoteForward 18080 127.0.0.1:8080"));
        assert!(config.contains("DynamicForward 1080"));
        assert!(!config.contains("15433"), "disabled forwards are not exported");
        assert!(!config.contains("database-password"));
        assert!(!config.contains("jump-passphrase"));
    }

    #[test]
    fn openssh_export_sanitizes_host_aliases_and_skips_invalid_jump_routes() {
        let mut data = VaultData::default();
        data.connections.insert(
            "one".into(),
            Connection {
                id: "one".into(),
                name: "Database * Primary".into(),
                host: "db.example.com".into(),
                user: "admin".into(),
                ..Connection::default()
            },
        );
        data.connections.insert(
            "two".into(),
            Connection {
                id: "two".into(),
                name: "Database * Primary".into(),
                host: "bad host\nHost *".into(),
                user: "admin".into(),
                ..Connection::default()
            },
        );
        let (config, skipped) = serialize_ssh_config(&data);
        assert_eq!(skipped, 1);
        assert!(config.contains("Host Database-Primary"));
        assert!(config.contains("# Skipped Database * Primary: invalid host or jump-host route."));
        assert!(!config.contains("Host *"));
    }

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
        assert_eq!(c.keepalive_secs, 0, "0 means keepalive disabled and is preserved");
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
                Tunnel::Local { bind: "127.0.0.1:015432".into(), dest: "db.internal:5432".into(), enabled: true },
                Tunnel::Remote { bind: "18080".into(), dest: "[::1]:8080".into(), enabled: true },
            ],
            ..Connection::default()
        }).unwrap();
        assert_eq!(connection.tunnels, vec![
            Tunnel::Local { bind: "127.0.0.1:15432".into(), dest: "db.internal:5432".into(), enabled: true },
            Tunnel::Remote { bind: "18080".into(), dest: "[::1]:8080".into(), enabled: true },
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
    fn disabled_tunnels_are_saved_but_not_passed_to_ssh() {
        let tunnels = vec![
            Tunnel::Local {
                bind: "127.0.0.1:15432".into(),
                dest: "db.internal:5432".into(),
                enabled: false,
            },
            Tunnel::Remote {
                bind: "18080".into(),
                dest: "127.0.0.1:8080".into(),
                enabled: true,
            },
        ];
        let forwards = resolve_forwards(&tunnels).unwrap();
        assert_eq!(forwards, vec![PortForward::Remote {
            bind: "18080".into(),
            destination: "127.0.0.1:8080".into(),
        }]);
    }

    #[test]
    fn dynamic_tunnel_is_normalized_and_resolved_to_a_socks_forward() {
        let mut tunnels = vec![
            Tunnel::Dynamic { bind: " 1080 ".into(), enabled: true },
            Tunnel::Dynamic { bind: "[::1]:1081".into(), enabled: true },
        ];
        normalize_tunnels(&mut tunnels).unwrap();
        let forwards = resolve_forwards(&tunnels).unwrap();
        assert_eq!(forwards, vec![
            PortForward::Dynamic { bind: "1080".into() },
            PortForward::Dynamic { bind: "[::1]:1081".into() },
        ]);
        assert_eq!(forwards[0].option(), "-D");
        assert_eq!(forwards[1].argument(), "[::1]:1081");
    }

    #[test]
    fn dynamic_tunnel_rejects_bad_endpoints_and_clashes_with_local_listeners() {
        for bind in ["", "0", "host:", "-oProxyCommand=x:1", "a b:1", "70000"] {
            let tunnels = vec![Tunnel::Dynamic { bind: bind.into(), enabled: true }];
            assert_eq!(resolve_forwards(&tunnels).unwrap_err(), "tunnel_bad_endpoint", "{bind:?}");
        }
        let clash = vec![
            Tunnel::Local { bind: "127.0.0.1:1080".into(), dest: "db:5432".into(), enabled: true },
            Tunnel::Dynamic { bind: "127.0.0.1:1080".into(), enabled: true },
        ];
        assert_eq!(resolve_forwards(&clash).unwrap_err(), "tunnel_duplicate_listener");
        // A reverse forward listens on the server, so the same port is fine.
        let ok = vec![
            Tunnel::Remote { bind: "1080".into(), dest: "127.0.0.1:80".into(), enabled: true },
            Tunnel::Dynamic { bind: "1080".into(), enabled: true },
        ];
        assert_eq!(resolve_forwards(&ok).unwrap().len(), 2);
    }

    #[test]
    fn disabled_invalid_or_deferred_tunnel_does_not_block_saving() {
        let connection = normalize(Connection {
            id: "disabled".into(),
            host: "server".into(),
            user: "user".into(),
            tunnels: vec![
                Tunnel::Local {
                    bind: "not-a-port".into(),
                    dest: "".into(),
                    enabled: false,
                },
                Tunnel::Dynamic {
                    bind: "127.0.0.1:1080".into(),
                    enabled: false,
                },
            ],
            ..Connection::default()
        }).unwrap();
        assert!(resolve_forwards(&connection.tunnels).unwrap().is_empty());
    }

    #[test]
    fn disabled_duplicate_tunnel_does_not_block_enabled_listener() {
        let connection = normalize(Connection {
            id: "tunnels".into(),
            host: "server".into(),
            user: "user".into(),
            tunnels: vec![
                Tunnel::Local {
                    bind: "127.0.0.1:15432".into(),
                    dest: "db-old:5432".into(),
                    enabled: false,
                },
                Tunnel::Local {
                    bind: "127.0.0.1:15432".into(),
                    dest: "db-new:5432".into(),
                    enabled: true,
                },
            ],
            ..Connection::default()
        }).unwrap();
        assert_eq!(resolve_forwards(&connection.tunnels).unwrap().len(), 1);
    }

    #[test]
    fn rejects_invalid_or_duplicate_tunnel_listeners() {
        let invalid = normalize(Connection {
            id: "bad".into(), host: "server".into(), user: "user".into(),
            tunnels: vec![Tunnel::Local { bind: "127.0.0.1:0".into(), dest: "db:5432".into(), enabled: true }],
            ..Connection::default()
        });
        assert_eq!(invalid.unwrap_err(), "tunnel_bad_endpoint");

        let duplicate = normalize(Connection {
            id: "duplicate".into(), host: "server".into(), user: "user".into(),
            tunnels: vec![
                Tunnel::Local { bind: "127.0.0.1:15432".into(), dest: "db1:5432".into(), enabled: true },
                Tunnel::Local { bind: "127.0.0.1:15432".into(), dest: "db2:5432".into(), enabled: true },
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
    fn normalize_rejects_option_injection_in_user_and_host() {
        let base = Connection { id: "c1".into(), host: "h".into(), user: "u".into(), ..Connection::default() };
        for (user, host) in [
            ("-oProxyCommand=calc", "h"),
            ("u", "-oProxyCommand=calc"),
            ("u", "h,evil"),
            ("u@x", "h"),
            ("u", "h x"),
            ("u", "h\"evil"),
            ("u", ""),
        ] {
            let c = Connection { user: user.into(), host: host.into(), ..base.clone() };
            assert!(normalize(c).is_err(), "{user}@{host} must be rejected");
        }
        for (user, host) in [("DOM\\user", "10.0.0.5"), ("admin", "2001:db8::1"), ("a.b-c_d", "srv-1.example.com")] {
            let c = Connection { user: user.into(), host: host.into(), ..base.clone() };
            assert!(normalize(c).is_ok(), "{user}@{host} must be accepted");
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
            protected: true,
            ..Connection::default()
        });
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("password"));
        assert!(summary.protected);
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
    fn disabled_jump_hosts_are_omitted_from_the_route() {
        let mut connections = std::collections::BTreeMap::new();
        let first = Connection {
            id: "first".into(), name: "first".into(), host: "first".into(), user: "u".into(),
            ..Connection::default()
        };
        let second = Connection {
            id: "second".into(), name: "second".into(), host: "second".into(), user: "u".into(),
            ..Connection::default()
        };
        let target = Connection {
            id: "target".into(), name: "target".into(), host: "target".into(), user: "u".into(),
            jump: vec!["first".into(), "second".into()],
            jump_disabled: vec!["first".into()],
            ..Connection::default()
        };
        connections.insert(first.id.clone(), first);
        connections.insert(second.id.clone(), second);
        connections.insert(target.id.clone(), target);
        let chain = resolve_jump_chain(&connections, "target").unwrap();
        assert_eq!(chain.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["second"]);
    }

    #[test]
    fn disabled_nested_jump_skips_its_entire_nested_route() {
        let mut connections = std::collections::BTreeMap::new();
        let gateway = Connection {
            id: "gateway".into(), name: "gateway".into(), host: "gateway".into(), user: "u".into(),
            ..Connection::default()
        };
        let bastion = Connection {
            id: "bastion".into(), name: "bastion".into(), host: "bastion".into(), user: "u".into(),
            jump: vec!["gateway".into()],
            jump_disabled: vec!["gateway".into()],
            ..Connection::default()
        };
        let target = Connection {
            id: "target".into(), name: "target".into(), host: "target".into(), user: "u".into(),
            jump: vec!["bastion".into()], ..Connection::default()
        };
        connections.insert(gateway.id.clone(), gateway);
        connections.insert(bastion.id.clone(), bastion);
        connections.insert(target.id.clone(), target);
        let chain = resolve_jump_chain(&connections, "target").unwrap();
        assert_eq!(chain.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["bastion"]);
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
