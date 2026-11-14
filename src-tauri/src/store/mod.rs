//! Minimal connection store (M2).
//!
//! `VaultData.connections` remains free-form JSON until M4; here it is read
//! and written with a proper type, without changing the vault format. This
//! lets M2 retrieve the password from the vault and deliver it to askpass.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::commands::VaultState;
use crate::vault::Vault;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub keepalive_secs: u32,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 22,
            user: String::new(),
            password: None,
            keepalive_secs: 30,
        }
    }
}

/// What is shown in the sidebar: never includes the password.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionSummary {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl From<Connection> for ConnectionSummary {
    fn from(c: Connection) -> Self {
        Self { id: c.id, name: c.name, host: c.host, port: c.port, user: c.user }
    }
}

fn guard<'r>(state: &'r State<'r, VaultState>) -> std::sync::MutexGuard<'r, Option<Vault>> {
    // `State` holds a `&'r VaultState` reference; the MutexGuard lives as long
    // as that reference, not as long as the borrow of `state`.
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn find(vault: &Vault, id: &str) -> Option<Connection> {
    vault
        .data
        .connections
        .get(id)
        .and_then(|v: &Value| serde_json::from_value(v.clone()).ok())
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
    // An empty password means "no password" (key or agent).
    c.password = c.password.filter(|p| !p.is_empty());
    Ok(c)
}

#[tauri::command]
pub fn conn_list(state: State<'_, VaultState>) -> Result<Vec<ConnectionSummary>, String> {
    let g = guard(&state);
    let vault = g.as_ref().ok_or("locked")?;
    let mut out: Vec<ConnectionSummary> = vault
        .data
        .connections
        .iter()
        .filter_map(|(_, v)| serde_json::from_value::<Connection>(v.clone()).ok())
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
pub fn conn_upsert(state: State<'_, VaultState>, conn: Connection) -> Result<(), String> {
    let conn = normalize(conn).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&conn).map_err(|_| "serialize_error".to_string())?;
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    vault.data.connections.insert(conn.id.clone(), value);
    vault.save().map_err(|e| e.code().to_string())
}

#[tauri::command]
pub fn conn_delete(state: State<'_, VaultState>, id: String) -> Result<(), String> {
    let mut g = guard(&state);
    let vault = g.as_mut().ok_or("locked")?;
    vault.data.connections.remove(&id);
    vault.save().map_err(|e| e.code().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fills_defaults_and_strips_empty_password() {
        let c = Connection {
            id: " c1 ".into(),
            name: "  ".into(),
            host: " 10.0.0.5 ".into(),
            port: 22,
            user: " admin ".into(),
            password: Some("".into()),
            keepalive_secs: 0,
        };
        let c = normalize(c).unwrap();
        assert_eq!(c.id, "c1");
        assert_eq!(c.name, "10.0.0.5");
        assert_eq!(c.host, "10.0.0.5");
        assert_eq!(c.user, "admin");
        assert_eq!(c.keepalive_secs, 30);
        assert!(c.password.is_none());
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
    fn summary_never_contains_a_password() {
        let summary = ConnectionSummary::from(Connection {
            id: "c1".into(),
            name: "srv".into(),
            host: "h".into(),
            port: 22,
            user: "u".into(),
            password: Some("secret".into()),
            keepalive_secs: 30,
        });
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("password"));
    }
}
