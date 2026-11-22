//! Encrypted vault: stores sessions, passwords and settings protected by the
//! master password. This module does not depend on Tauri, so it can be tested
//! with `cargo test` without opening any window.

mod crypto;
mod error;
mod model;
pub mod scripts;

#[cfg(test)]
mod tests;

use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use zeroize::Zeroizing;

use crypto::{derive_key, encode_header, open, parse_header, random_bytes, seal, KEY_LEN, SALT_LEN};
pub use crypto::KdfParams;
pub use error::VaultError;
pub use model::{Auth, Connection, TreeNode, Tunnel, VaultData, DEFAULT_FOLDER_ID};
pub use scripts::ScriptsData;

/// Unlocked encrypted file with a typed payload. While it exists, the key is in
/// memory; on drop (locking) the key is cleared (`Zeroizing`). The connections
/// vault and the scripts library are two instances of this type.
pub struct SealedFile<D> {
    path: PathBuf,
    key: Zeroizing<[u8; KEY_LEN]>,
    salt: [u8; SALT_LEN],
    params: KdfParams,
    pub data: D,
}

/// The connections vault (`sshterm.vault`).
pub type Vault = SealedFile<VaultData>;
/// The scripts library (`scripts.vault`).
pub type ScriptsVault = SealedFile<ScriptsData>;

// Manual Debug: never print the key or the data.
impl<D> fmt::Debug for SealedFile<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault").field("path", &self.path).finish_non_exhaustive()
    }
}

impl<D: Serialize + DeserializeOwned + Default> SealedFile<D> {
    /// Creates a new, empty vault. Fails if one already exists.
    pub fn create(path: &Path, password: &str) -> Result<Self, VaultError> {
        Self::create_with(path, password, KdfParams::default())
    }

    pub fn create_with(path: &Path, password: &str, params: KdfParams) -> Result<Self, VaultError> {
        if path.exists() {
            return Err(VaultError::AlreadyExists);
        }
        if password.is_empty() {
            return Err(VaultError::EmptyPassword);
        }
        let salt = random_bytes::<SALT_LEN>();
        let key = derive_key(password, &salt, &params)?;
        let vault = SealedFile { path: path.to_path_buf(), key, salt, params, data: D::default() };
        vault.save()?;
        Ok(vault)
    }

    /// Opens an existing vault.
    pub fn unlock(path: &Path, password: &str) -> Result<Self, VaultError> {
        let bytes = fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { VaultError::NotFound } else { VaultError::Io(e) }
        })?;
        let header = parse_header(&bytes)?;
        let key = derive_key(password, &header.salt, &header.params)?;
        let plain = open(&key, &bytes)?;
        let data: D = serde_json::from_slice(&plain)?;
        Ok(SealedFile { path: path.to_path_buf(), key, salt: header.salt, params: header.params, data })
    }

    /// Saves the current data (fresh nonce, atomic write).
    pub fn save(&self) -> Result<(), VaultError> {
        let bytes = serialize(&self.data, &self.key, &self.salt, &self.params)?;
        write_atomic(&self.path, &bytes)
    }

    /// Changes the master password: new salt, new key, everything re-encrypted.
    /// If saving fails, the in-memory vault remains as it was.
    pub fn change_password(&mut self, new_password: &str) -> Result<(), VaultError> {
        if new_password.is_empty() {
            return Err(VaultError::EmptyPassword);
        }
        let salt = random_bytes::<SALT_LEN>();
        let key = derive_key(new_password, &salt, &self.params)?;
        let bytes = serialize(&self.data, &key, &salt, &self.params)?;
        write_atomic(&self.path, &bytes)?;
        self.salt = salt;
        self.key = key;
        Ok(())
    }
}

fn serialize<D: Serialize>(
    data: &D,
    key: &[u8; KEY_LEN],
    salt: &[u8; SALT_LEN],
    params: &KdfParams,
) -> Result<Vec<u8>, VaultError> {
    let plain = Zeroizing::new(serde_json::to_vec(data)?);
    let header = encode_header(params, salt);
    seal(key, &header, &plain)
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Writes to a temporary file, forces it to disk, saves a `.bak` copy of the
/// previous vault, and renames. A power cut never leaves a half-written vault.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let tmp = sibling(path, ".tmp");
    let bak = sibling(path, ".bak");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if path.exists() {
        fs::copy(path, &bak)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}
