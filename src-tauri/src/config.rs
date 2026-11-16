//! Non-secret settings, saved in `config.toml` next to the exe.
//!
//! They are kept outside the vault on purpose: they are needed before unlocking
//! (for example, the lock screen language and application theme).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_LANGUAGE: &str = "en";

/// Application appearance, available before the vault is unlocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppTheme {
    Dark,
    Light,
    Nord,
}

impl Default for AppTheme {
    fn default() -> Self {
        Self::Dark
    }
}

/// What goes into the per-session log file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogMode {
    /// Plain text of what the terminal shows (default). Built in the frontend from
    /// the xterm.js buffer, so redraws sent by ConPTY never repeat lines.
    Screen,
    /// The bytes of the PTY exactly as received, escape codes included. For
    /// diagnostics only: ConPTY output is a stream of screen redraws.
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    pub copy_on_selection: bool,
    pub right_click_copy: bool,
    pub middle_click_copy: bool,
    pub right_click_paste: bool,
    pub middle_click_paste: bool,
    pub ctrl_c_copy: bool,
    /// Ask before sending clipboard contents containing more than one line.
    pub confirm_multiline_paste: bool,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            copy_on_selection: true,
            right_click_copy: true,
            middle_click_copy: true,
            right_click_paste: true,
            middle_click_paste: true,
            ctrl_c_copy: true,
            confirm_multiline_paste: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)] // missing fields take their default value
pub struct Config {
    pub language: String,
    /// UI theme; stored outside the encrypted vault so the lock screen can use it.
    pub theme: AppTheme,
    /// Write a log file per session (default true).
    pub log_output: bool,
    /// Kind of log (default: plain text of the screen).
    pub log_mode: LogMode,
    /// Terminal clipboard and mouse behavior.
    pub terminal: TerminalSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: DEFAULT_LANGUAGE.to_string(),
            theme: AppTheme::default(),
            log_output: true,
            log_mode: LogMode::Screen,
            terminal: TerminalSettings::default(),
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
        assert_eq!(cfg.theme, AppTheme::Dark);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config { language: "es".into(), theme: AppTheme::Nord, ..Default::default() }.save(&path).unwrap();
        let cfg = Config::load(&path);
        assert_eq!(cfg.language, "es");
        assert_eq!(cfg.theme, AppTheme::Nord);
        assert!(cfg.log_output);
        assert_eq!(cfg.log_mode, LogMode::Screen);
        assert!(!dir.path().join("config.toml.tmp").exists());
    }

    #[test]
    fn logging_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "language = \"es\"\n").unwrap();
        let cfg = Config::load(&path);
        assert!(cfg.log_output);
        assert_eq!(cfg.log_mode, LogMode::Screen);
        assert_eq!(cfg.theme, AppTheme::Dark);
    }

    #[test]
    fn log_mode_is_read_from_the_file_and_old_fields_are_ignored() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // `strip_special` and `log_input` existed before: they must not break loading
        fs::write(&path, "log_mode = \"raw\"\nstrip_special = true\nlog_input = false\n").unwrap();
        assert_eq!(Config::load(&path).log_mode, LogMode::Raw);
    }

    #[test]
    fn terminal_settings_default_on_and_old_config_gets_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "language = \"es\"\n").unwrap();
        let cfg = Config::load(&path);
        assert_eq!(cfg.terminal, TerminalSettings::default());
        assert!(cfg.terminal.copy_on_selection);
        assert!(cfg.terminal.right_click_copy);
        assert!(cfg.terminal.middle_click_copy);
        assert!(cfg.terminal.right_click_paste);
        assert!(cfg.terminal.middle_click_paste);
        assert!(cfg.terminal.ctrl_c_copy);
        assert!(cfg.terminal.confirm_multiline_paste);
    }

    #[test]
    fn terminal_settings_are_persisted_independently() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.terminal.copy_on_selection = false;
        cfg.terminal.middle_click_paste = false;
        cfg.terminal.confirm_multiline_paste = false;
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path);
        assert!(!loaded.terminal.copy_on_selection);
        assert!(loaded.terminal.right_click_copy);
        assert!(!loaded.terminal.middle_click_paste);
        assert!(loaded.terminal.right_click_paste);
        assert!(!loaded.terminal.confirm_multiline_paste);
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
