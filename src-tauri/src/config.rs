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
    /// Write terminal output to the per-session log file (default true).
    pub log_output: bool,
    /// Also write typed input to the log file (default false: keeps secrets out).
    pub log_input: bool,
    /// Strip special characters from the log file — ANSI/escape sequences and
    /// other control characters (carriage returns, bell, backspace, …) — leaving
    /// readable text (default true). Set to false to keep the raw byte stream.
    pub strip_special: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: DEFAULT_LANGUAGE.to_string(),
            log_output: true,
            log_input: false,
            strip_special: true,
        }
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
        Config { language: "es".into(), ..Default::default() }.save(&path).unwrap();
        let cfg = Config::load(&path);
        assert_eq!(cfg.language, "es");
        assert!(cfg.log_output);
        assert!(!cfg.log_input);
        assert!(cfg.strip_special);
        assert!(!dir.path().join("config.toml.tmp").exists());
    }

    #[test]
    fn logging_fields_default_off_for_secrets() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "language = \"es\"\n").unwrap();
        let cfg = Config::load(&path);
        assert!(cfg.log_output); // output on by default
        assert!(!cfg.log_input); // never log typed input by default
        assert!(cfg.strip_special); // special characters stripped by default
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
