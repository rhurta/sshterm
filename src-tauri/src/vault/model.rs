use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const DATA_VERSION: u32 = 1;

/// Contenido descifrado del vault (se guarda como JSON dentro del archivo cifrado).
///
/// En M1 `tree` y `connections` son JSON libre. En M4 se sustituyen por
/// estructuras tipadas (carpetas, conexiones, túneles, colores...).
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
