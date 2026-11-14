use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const DATA_VERSION: u32 = 1;

/// Decrypted vault contents (stored as JSON inside the encrypted file).
///
/// In M1 `tree` and `connections` are free-form JSON. In M4 they will be
/// replaced by typed structures (folders, connections, tunnels, colors...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultData {
    pub version: u32,
    #[serde(default)]
    pub tree: Vec<Value>,
    #[serde(default)]
    pub connections: BTreeMap<String, Value>,
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
