use std::fmt;

/// Todos los errores posibles del vault.
#[derive(Debug)]
pub enum VaultError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    /// El archivo del vault no existe.
    NotFound,
    /// Ya existe un vault y no se sobrescribe.
    AlreadyExists,
    /// El archivo no empieza por la firma "SSHV".
    NotAVault,
    /// Versión de formato que esta aplicación no conoce.
    UnsupportedVersion(u8),
    /// Parámetros de Argon2 fuera de rango (archivo manipulado o dañado).
    InvalidParams,
    EmptyPassword,
    /// Contraseña incorrecta O archivo alterado: el cifrado no distingue
    /// los dos casos a propósito.
    WrongPasswordOrCorrupt,
    Crypto(String),
}

impl VaultError {
    /// Código corto y estable. El frontend lo traduce a un mensaje para el usuario.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io_error",
            Self::Serialize(_) => "serialize_error",
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::NotAVault => "not_a_vault",
            Self::UnsupportedVersion(_) => "unsupported_version",
            Self::InvalidParams => "invalid_params",
            Self::EmptyPassword => "empty_password",
            Self::WrongPasswordOrCorrupt => "wrong_password",
            Self::Crypto(_) => "crypto_error",
        }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Serialize(e) => write!(f, "serialization error: {e}"),
            Self::NotFound => write!(f, "vault file not found"),
            Self::AlreadyExists => write!(f, "a vault already exists"),
            Self::NotAVault => write!(f, "not a vault file"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported vault version {v}"),
            Self::InvalidParams => write!(f, "invalid key derivation parameters"),
            Self::EmptyPassword => write!(f, "empty password"),
            Self::WrongPasswordOrCorrupt => write!(f, "wrong password or corrupted vault"),
            Self::Crypto(m) => write!(f, "crypto error: {m}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<std::io::Error> for VaultError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for VaultError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e)
    }
}
