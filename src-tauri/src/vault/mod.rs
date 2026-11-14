//! Vault cifrado: guarda sesiones, contraseñas y ajustes protegidos por la
//! contraseña maestra. Este módulo no depende de Tauri, así que se puede
//! probar con `cargo test` sin abrir ninguna ventana.

mod crypto;
mod error;
mod model;

#[cfg(test)]
mod tests;

use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crypto::{derive_key, encode_header, open, parse_header, random_bytes, seal, KEY_LEN, SALT_LEN};
pub use crypto::KdfParams;
pub use error::VaultError;
pub use model::VaultData;

/// Vault desbloqueado. Mientras exista, la clave está en memoria;
/// al soltarlo (bloquear) la clave se borra (`Zeroizing`).
pub struct Vault {
    path: PathBuf,
    key: Zeroizing<[u8; KEY_LEN]>,
    salt: [u8; SALT_LEN],
    params: KdfParams,
    pub data: VaultData,
}

// Debug manual: nunca imprimir la clave ni los datos.
impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault").field("path", &self.path).finish_non_exhaustive()
    }
}

impl Vault {
    /// Crea un vault nuevo y vacío. Falla si ya existe uno.
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
        let vault = Vault { path: path.to_path_buf(), key, salt, params, data: VaultData::default() };
        vault.save()?;
        Ok(vault)
    }

    /// Abre un vault existente.
    pub fn unlock(path: &Path, password: &str) -> Result<Self, VaultError> {
        let bytes = fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { VaultError::NotFound } else { VaultError::Io(e) }
        })?;
        let header = parse_header(&bytes)?;
        let key = derive_key(password, &header.salt, &header.params)?;
        let plain = open(&key, &bytes)?;
        let data: VaultData = serde_json::from_slice(&plain)?;
        Ok(Vault { path: path.to_path_buf(), key, salt: header.salt, params: header.params, data })
    }

    /// Guarda los datos actuales (nonce nuevo, escritura atómica).
    pub fn save(&self) -> Result<(), VaultError> {
        let bytes = serialize(&self.data, &self.key, &self.salt, &self.params)?;
        write_atomic(&self.path, &bytes)
    }

    /// Cambia la contraseña maestra: sal nueva, clave nueva, todo re-cifrado.
    /// Si el guardado falla, el vault en memoria queda como estaba.
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

fn serialize(
    data: &VaultData,
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

/// Escribe en un archivo temporal, lo fuerza a disco, guarda una copia `.bak`
/// del vault anterior y renombra. Un corte de luz no deja un vault a medias.
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
