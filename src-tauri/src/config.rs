//! Non-secret settings, saved in `config.toml` next to the exe.
//!
//! They are kept outside the vault on purpose: they are needed before unlocking
//! (for example, the lock screen language).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_LANGUAGE: &str = "en";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)] // missing fields take their default value
pub struct Config {
    pub language: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { language: DEFAULT_LANGUAGE.to_string() }
    }
}

impl Config {
    /// Missing or unreadable file -> default values. A broken config.toml
    /// must never prevent the application from starting.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes to a temporary file and renames it (atomic write).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        {
            let mut f = File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)
    }
}

/// Accepts language codes like "en", "es", "pt-BR". Rejects everything else,
/// so nothing unexpected reaches the file.
pub fn normalize_language(code: &str) -> Option<String> {
    let code = code.trim();
    let valid = !code.is_empty()
        && code.len() <= 16
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !code.starts_with('-')
        && !code.ends_with('-');
    valid.then(|| code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_gives_english() {
        let dir = tempdir().unwrap();
        let cfg = Config::load(&dir.path().join("config.toml"));
        assert_eq!(cfg.language, "en");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config { language: "es".into() }.save(&path).unwrap();
        assert_eq!(Config::load(&path).language, "es");
        assert!(!dir.path().join("config.toml.tmp").exists());
    }

    #[test]
    fn broken_file_falls_back_to_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "this is [[[ not toml").unwrap();
        assert_eq!(Config::load(&path), Config::default());
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "future_setting = 3\n").unwrap();
        assert_eq!(Config::load(&path).language, "en");
        fs::write(&path, "language = \"es\"\nfuture_setting = 3\n").unwrap();
        assert_eq!(Config::load(&path).language, "es");
    }

    #[test]
    fn language_codes_are_validated() {
        assert_eq!(normalize_language("es").as_deref(), Some("es"));
        assert_eq!(normalize_language(" pt-BR ").as_deref(), Some("pt-BR"));
        for bad in ["", "-", "es-", "-es", "en\nUS", "a/b", "x".repeat(17).as_str(), "é"] {
            assert!(normalize_language(bad).is_none(), "should reject {bad:?}");
        }
    }
}
