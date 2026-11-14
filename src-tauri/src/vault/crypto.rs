//! Criptografía y formato binario del archivo.
//!
//! Disposición del archivo:
//!
//! | bytes  | contenido                                              |
//! |--------|--------------------------------------------------------|
//! | 0..4   | firma "SSHV"                                           |
//! | 4      | versión del formato (1)                                |
//! | 5..9   | Argon2: memoria en KiB (u32, little endian)           |
//! | 9..13  | Argon2: iteraciones (u32)                              |
//! | 13..17 | Argon2: carriles (u32)                                 |
//! | 17..33 | sal aleatoria (16 bytes)                               |
//! | 33..57 | nonce aleatorio (24 bytes), nuevo en cada guardado     |
//! | 57..   | datos cifrados con XChaCha20-Poly1305 (+16 de etiqueta)|
//!
//! Los primeros 33 bytes (la cabecera) se autentican como "datos asociados":
//! si alguien los modifica, el descifrado falla.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    Key, XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroizing;

use super::error::VaultError;

pub const MAGIC: &[u8; 4] = b"SSHV";
pub const FORMAT_VERSION: u8 = 1;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
pub const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
pub const HEADER_LEN: usize = 4 + 1 + 12 + SALT_LEN;
pub const MIN_FILE_LEN: usize = HEADER_LEN + NONCE_LEN + TAG_LEN;

/// Parámetros de Argon2id. Se guardan en la cabecera para poder subirlos en el futuro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self { memory_kib: 64 * 1024, iterations: 3, lanes: 1 }
    }
}

impl KdfParams {
    /// Un archivo manipulado podría pedir gigabytes de memoria: se limita antes de derivar.
    fn validate(&self) -> Result<(), VaultError> {
        let ok = (1..=16).contains(&self.lanes)
            && (1..=20).contains(&self.iterations)
            && self.memory_kib >= 8 * self.lanes
            && self.memory_kib <= 1_048_576; // 1 GiB
        if ok { Ok(()) } else { Err(VaultError::InvalidParams) }
    }
}

pub struct Header {
    pub params: KdfParams,
    pub salt: [u8; SALT_LEN],
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

/// Contraseña + sal -> clave de 32 bytes. Lento a propósito (frena la fuerza bruta).
pub fn derive_key(
    password: &str,
    salt: &[u8; SALT_LEN],
    p: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
    p.validate()?;
    let params = Params::new(p.memory_kib, p.iterations, p.lanes, Some(KEY_LEN))
        .map_err(|_| VaultError::InvalidParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key[..])
        .map_err(|e| VaultError::Crypto(e.to_string()))?;
    Ok(key)
}

pub fn encode_header(params: &KdfParams, salt: &[u8; SALT_LEN]) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(MAGIC);
    h[4] = FORMAT_VERSION;
    h[5..9].copy_from_slice(&params.memory_kib.to_le_bytes());
    h[9..13].copy_from_slice(&params.iterations.to_le_bytes());
    h[13..17].copy_from_slice(&params.lanes.to_le_bytes());
    h[17..33].copy_from_slice(salt);
    h
}

pub fn parse_header(file: &[u8]) -> Result<Header, VaultError> {
    if file.len() < 5 || &file[0..4] != MAGIC {
        return Err(VaultError::NotAVault);
    }
    if file[4] != FORMAT_VERSION {
        return Err(VaultError::UnsupportedVersion(file[4]));
    }
    if file.len() < MIN_FILE_LEN {
        return Err(VaultError::WrongPasswordOrCorrupt); // archivo truncado
    }
    let u32_at = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().unwrap());
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&file[17..33]);
    let params = KdfParams { memory_kib: u32_at(5), iterations: u32_at(9), lanes: u32_at(13) };
    params.validate()?;
    Ok(Header { params, salt })
}

/// Cifra y devuelve el archivo completo: cabecera + nonce nuevo + datos cifrados.
pub fn seal(
    key: &[u8; KEY_LEN],
    header: &[u8; HEADER_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = random_bytes::<NONCE_LEN>();
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad: header })
        .map_err(|_| VaultError::Crypto("encryption failed".into()))?;
    let mut out = Vec::with_capacity(HEADER_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(header);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Descifra el archivo completo. Falla si la clave es incorrecta o si algo cambió.
pub fn open(key: &[u8; KEY_LEN], file: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    if file.len() < MIN_FILE_LEN {
        return Err(VaultError::WrongPasswordOrCorrupt);
    }
    let header = &file[..HEADER_LEN];
    let nonce = &file[HEADER_LEN..HEADER_LEN + NONCE_LEN];
    let ct = &file[HEADER_LEN + NONCE_LEN..];
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: header })
        .map(Zeroizing::new)
        .map_err(|_| VaultError::WrongPasswordOrCorrupt)
}
