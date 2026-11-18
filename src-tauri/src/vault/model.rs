//! Typed vault payload (M4).
//!
//! Until M3 `tree` and `connections` were free-form JSON. M4 gives them real
//! types while keeping payload `version: 1` and the on-disk file format (and
//! therefore the encryption header) unchanged. Old vaults are migrated in
//! memory by `store::migrate()` when they are unlocked.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

pub const DATA_VERSION: u32 = 1;

/// Stable id of the folder that old flat vaults are wrapped into.
pub const DEFAULT_FOLDER_ID: &str = "f_connections";

/// A node of the sidebar tree. Folder nodes own their children; connection
/// nodes only reference an id in `VaultData::connections`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TreeNode {
    Folder {
        id: String,
        name: String,
        #[serde(default)]
        children: Vec<TreeNode>,
        /// Rule sets inherited by descendants unless a connection overrides them.
        #[serde(default)]
        highlight: Vec<String>,
    },
    Connection {
        id: String,
    },
}

impl TreeNode {
    pub fn id(&self) -> &str {
        match self {
            TreeNode::Folder { id, .. } | TreeNode::Connection { id } => id,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, TreeNode::Folder { .. })
    }
}

/// How a connection authenticates. `key` stores only the path to the key file;
/// the key itself always stays outside the vault (spec §12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Auth {
    Password {
        #[serde(default)]
        password: String,
    },
    Key {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
    },
    Agent,
}

impl Default for Auth {
    fn default() -> Self {
        Auth::Agent
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Colors {
    pub fg: String,
    pub bg: String,
}

/// Existing vault tunnel entries without an explicit enabled field remain
/// enabled when deserialized, preserving the behavior of older vaults.
fn default_enabled() -> bool {
    true
}

/// Forward declaration for M7. Kept in the payload now so no further vault
/// migration is needed when tunnels are implemented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Tunnel {
    Local {
        bind: String,
        dest: String,
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
    Remote {
        bind: String,
        dest: String,
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
    Dynamic {
        bind: String,
        #[serde(default = "default_enabled")]
        enabled: bool,
    },
}

/// Forward declaration for M8.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SftpOptions {
    pub port: u16,
    pub start_dir: String,
}

impl Default for SftpOptions {
    fn default() -> Self {
        Self { port: 22, start_dir: String::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    pub keepalive_secs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Colors>,
    pub protected: bool,
    /// Ordered jump chain (M5).
    pub jump: Vec<String>,
    /// IDs in jump that are configured but currently disabled.
    #[serde(default)]
    pub jump_disabled: Vec<String>,
    pub tunnels: Vec<Tunnel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sftp: Option<SftpOptions>,
    /// Rule-set names for M6b. A folder may pass them down to its children.
    pub highlight: Vec<String>,
    /// Legacy M1-M3 field named `password` in JSON. It is consumed by
    /// `migrate_legacy()` and never written back after migration.
    #[serde(rename = "password", default, skip_serializing_if = "Option::is_none")]
    pub legacy_password: Option<String>,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 22,
            user: String::new(),
            auth: Auth::Agent,
            keepalive_secs: 30,
            colors: None,
            protected: false,
            jump: Vec::new(),
            jump_disabled: Vec::new(),
            tunnels: Vec::new(),
            sftp: None,
            highlight: Vec::new(),
            legacy_password: None,
        }
    }
}

impl Connection {
    /// Whether this connection is configured for password authentication.
    /// This is independent of whether a password is saved: an empty field can
    /// still mean that the user will type the password into the terminal.
    pub fn uses_password_auth(&self) -> bool {
        matches!(&self.auth, Auth::Password { .. })
            || self.legacy_password.as_ref().is_some_and(|password| !password.is_empty())
    }

    /// Secret delivered to the askpass helper: a login password for password
    /// auth, or the stored passphrase of a key file. Key/agent auth without a
    /// passphrase returns None. Legacy M1-M3 passwords are still honoured.
    pub fn password(&self) -> Option<String> {
        match &self.auth {
            Auth::Password { password } if !password.is_empty() => Some(password.clone()),
            Auth::Key { passphrase: Some(p), .. } if !p.is_empty() => Some(p.clone()),
            _ => self.legacy_password.clone().filter(|p| !p.is_empty()),
        }
    }

    /// Path passed to `ssh.exe -i` when the connection uses a key file.
    pub fn key_path(&self) -> Option<String> {
        match &self.auth {
            Auth::Key { path, .. } if !path.trim().is_empty() => Some(path.trim().to_string()),
            _ => None,
        }
    }

    /// Moves an M1-M3 `password` field into `auth`. Returns true when the
    /// payload changed and must be saved again.
    pub fn migrate_legacy(&mut self) -> bool {
        let legacy = self.legacy_password.take();
        match legacy {
            Some(p) if !p.is_empty() => {
                self.auth = Auth::Password { password: p };
                true
            }
            Some(_) => true, // an empty password meant "key or agent"
            None => false,
        }
    }
}

/// Old vaults may contain a `tree` that is not in the M4 shape (it was never
/// written by M1-M3, but keep unlocking robust). An unreadable tree becomes
/// empty and is rebuilt by `store::tree::reconcile`.
fn lenient_tree<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<TreeNode>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

fn default_version() -> u32 {
    DATA_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultData {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, deserialize_with = "lenient_tree")]
    pub tree: Vec<TreeNode>,
    #[serde(default)]
    pub connections: BTreeMap<String, Connection>,
    #[serde(default)]
    pub known_hosts: BTreeMap<String, String>,
}

impl Default for VaultData {
    fn default() -> Self {
        Self {
            version: DATA_VERSION,
            tree: Vec::new(),
            connections: BTreeMap::new(),
            known_hosts: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> VaultData {
        let mut connections = BTreeMap::new();
        connections.insert(
            "c1".into(),
            Connection {
                id: "c1".into(),
                name: "pgw-01".into(),
                host: "10.0.0.5".into(),
                user: "admin".into(),
                auth: Auth::Password { password: "secret".into() },
                ..Connection::default()
            },
        );
        VaultData {
            tree: vec![TreeNode::Folder {
                id: "f1".into(),
                name: "Production".into(),
                children: vec![TreeNode::Connection { id: "c1".into() }],
                highlight: Vec::new(),
            }],
            connections,
            ..VaultData::default()
        }
    }

    #[test]
    fn old_folder_nodes_default_to_no_highlight_rules() {
        let node: TreeNode = serde_json::from_str(
            r#"{"type":"folder","id":"f1","name":"Legacy","children":[]}"#,
        ).unwrap();
        match node {
            TreeNode::Folder { highlight, .. } => assert!(highlight.is_empty()),
            _ => panic!("expected folder"),
        }
    }

    #[test]
    fn typed_payload_roundtrips() {
        let json = serde_json::to_string(&sample_payload()).unwrap();
        let back: VaultData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sample_payload());
        assert_eq!(back.version, DATA_VERSION);
    }

    #[test]
    fn tree_uses_the_spec_json_shape() {
        let json = serde_json::to_string(&sample_payload().tree).unwrap();
        assert!(json.contains("\"type\":\"folder\""));
        assert!(json.contains("\"type\":\"connection\""));
        assert!(json.contains("\"children\""));
    }

    #[test]
    fn legacy_flat_vault_deserializes_and_migrates() {
        let raw = r#"{
            "version": 1,
            "tree": [],
            "connections": {
                "c1": {
                    "id": "c1", "name": "old-box", "host": "10.0.0.5",
                    "port": 22, "user": "admin", "password": "legacy-secret",
                    "keepalive_secs": 30
                }
            },
            "known_hosts": {}
        }"#;
        let mut data: VaultData = serde_json::from_str(raw).unwrap();
        assert_eq!(data.connections["c1"].password().as_deref(), Some("legacy-secret"));
        assert!(data.connections.get_mut("c1").unwrap().migrate_legacy());
        assert_eq!(
            data.connections["c1"].auth,
            Auth::Password { password: "legacy-secret".into() }
        );
        let json = serde_json::to_string(&data.connections["c1"]).unwrap();
        assert!(json.contains("\"auth\":{\"type\":\"password\""));
        let back: Connection = serde_json::from_str(&json).unwrap();
        assert!(back.legacy_password.is_none());
        assert_eq!(back.auth, Auth::Password { password: "legacy-secret".into() });
    }

    #[test]
    fn unknown_fields_and_missing_optionals_are_tolerated() {
        let raw = r#"{
            "version": 1,
            "tree": [{"unexpected": true}],
            "connections": {
                "c1": {
                    "id": "c1", "name": "n", "host": "h", "port": 22,
                    "user": "u", "auth": {"type": "key", "path": "C:/keys/id"},
                    "future_field": 42
                }
            }
        }"#;
        let data: VaultData = serde_json::from_str(raw).unwrap();
        assert!(data.tree.is_empty()); // unreadable tree ignored, rebuilt by reconcile
        let c = &data.connections["c1"];
        assert_eq!(c.keepalive_secs, 30);
        assert_eq!(c.key_path().as_deref(), Some("C:/keys/id"));
        assert!(c.password().is_none());
        assert!(c.jump.is_empty() && c.tunnels.is_empty() && c.highlight.is_empty());
    }

    #[test]
    fn password_authentication_mode_is_detected_without_a_saved_secret() {
        let password = Connection {
            auth: Auth::Password { password: String::new() },
            ..Connection::default()
        };
        assert!(password.uses_password_auth());

        let legacy_password = Connection {
            legacy_password: Some("legacy-secret".into()),
            ..Connection::default()
        };
        assert!(legacy_password.uses_password_auth());

        let key = Connection {
            auth: Auth::Key {
                path: "C:/keys/id".into(),
                passphrase: Some("phrase".into()),
            },
            ..Connection::default()
        };
        assert!(!key.uses_password_auth());
        assert!(!Connection::default().uses_password_auth());
    }

    #[test]
    fn key_passphrase_is_delivered_to_askpass() {
        let key = Connection {
            auth: Auth::Key { path: "C:/keys/id".into(), passphrase: Some("phrase".into()) },
            ..Connection::default()
        };
        assert_eq!(key.password().as_deref(), Some("phrase"));
        assert_eq!(key.key_path().as_deref(), Some("C:/keys/id"));
        let bare = Connection {
            auth: Auth::Key { path: "C:/keys/id".into(), passphrase: None },
            ..Connection::default()
        };
        assert!(bare.password().is_none());
    }

    #[test]
    fn pre_toggle_vault_entries_deserialize_as_enabled() {
        let raw = r#"{
            "id": "target",
            "name": "database",
            "host": "db.internal",
            "port": 22,
            "user": "admin",
            "jump": ["bastion"],
            "tunnels": [
                { "kind": "local", "bind": "127.0.0.1:15432", "dest": "db:5432" },
                { "kind": "remote", "bind": "18080", "dest": "web:8080" }
            ]
        }"#;
        let connection: Connection = serde_json::from_str(raw).unwrap();
        assert!(connection.jump_disabled.is_empty());
        assert_eq!(connection.jump, vec!["bastion"]);
        assert!(matches!(
            connection.tunnels.first(),
            Some(Tunnel::Local { enabled: true, .. })
        ));
        assert!(matches!(
            connection.tunnels.get(1),
            Some(Tunnel::Remote { enabled: true, .. })
        ));

        let saved = serde_json::to_value(&connection).unwrap();
        assert_eq!(saved["tunnels"][0]["enabled"], true);
        assert_eq!(saved["jump_disabled"], serde_json::json!([]));
    }

    #[test]
    fn auth_variants_serialize_as_tagged_json() {
        let key = Auth::Key { path: "C:/keys/id".into(), passphrase: None };
        assert_eq!(serde_json::to_string(&key).unwrap(), r#"{"type":"key","path":"C:/keys/id"}"#);
        assert_eq!(serde_json::to_string(&Auth::Agent).unwrap(), r#"{"type":"agent"}"#);
    }
}
