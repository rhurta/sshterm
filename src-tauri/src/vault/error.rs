use std::fmt;

/// All possible vault errors.
#[derive(Debug)]
pub enum VaultError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    /// The vault file does not exist.
    NotFound,
    /// A vault already exists and is not overwritten.
    AlreadyExists,
    /// The file does not start with the "SSHV" magic.
    NotAVault,
    /// A format version this application does not know.
    UnsupportedVersion(u8),
    /// Argon2 parameters out of range (tampered or damaged file).
    InvalidParams,
    EmptyPassword,
    /// Wrong password OR altered file: the cipher does not distinguish
    /// the two cases on purpose.
    WrongPasswordOrCorrupt,
    Crypto(String),
}

impl VaultError {
    /// Short, stable code. The frontend translates it to a user-facing message.
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
