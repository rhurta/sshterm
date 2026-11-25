//! Centralized application data paths.
//!
//! Mutable app data lives in the portable `config/` directory beside the exe.
//! Session logs deliberately remain in `logs/` beside it.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::config::Config;

/// Folder containing the executable and the separate `logs/` folder.
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Mutable application data: vaults, settings, trusted hosts, UI state and caches.
pub fn config_dir() -> PathBuf {
    app_dir().join("config")
}

/// Move the pre-config-folder data layout into `config/` without overwriting
/// anything already there. Existing user-configured relative vault/background
/// paths are moved too, retaining their relative layout.
pub fn migrate_legacy_layout() -> io::Result<()> {
    migrate_legacy_layout_at(&app_dir())
}

fn is_safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| matches!(component, Component::Normal(_)))
}

fn migrate_path(source: &Path, destination: &Path) -> io::Result<()> {
    if !source.exists() {
        return Ok(());
    }

    if destination.exists() {
        // A previous partial migration or an existing config directory may
        // have created the destination first. Merge missing children only;
        // never replace an existing user's file.
        if source.is_dir() && destination.is_dir() {
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                migrate_path(&entry.path(), &destination.join(entry.file_name()))?;
            }
            let _ = fs::remove_dir(source);
        }
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(source, destination)
}

fn migrate_relative_setting(app: &Path, config: &Path, value: &str) -> io::Result<()> {
    let path = Path::new(value.trim());
    if path.is_absolute() || !is_safe_relative(path) {
        return Ok(());
    }
    migrate_path(&app.join(path), &config.join(path))
}

fn migrate_legacy_layout_at(app: &Path) -> io::Result<()> {
    let config = app.join("config");
    fs::create_dir_all(&config)?;

    // Read the existing settings before moving their file so custom relative
    // paths can be migrated alongside the built-in data files.
    let new_config_file = config.join("config.toml");
    let old_config_file = app.join("config.toml");
    let settings = if new_config_file.exists() {
        Config::load(&new_config_file)
    } else {
        Config::load(&old_config_file)
    };

    for name in [
        "config.toml",
        "sshterm.vault",
        "scripts.vault",
        "known_hosts",
        "window-state.json",
        "webview-data",
        "highlight",
    ] {
        migrate_path(&app.join(name), &config.join(name))?;
    }

    migrate_relative_setting(app, &config, &settings.scripts_vault)?;
    migrate_relative_setting(app, &config, &settings.appearance.background_image)?;
    Ok(())
}

pub fn vault_path() -> PathBuf {
    config_dir().join("sshterm.vault")
}

/// Scripts library file. Relative custom paths are resolved within `config/`;
/// absolute paths remain explicit external locations.
pub fn scripts_vault_path() -> PathBuf {
    let configured = Config::load(&config_path()).scripts_vault;
    if configured.trim().is_empty() {
        return config_dir().join("scripts.vault");
    }
    resolve_config_file(&configured)
}

fn resolve_config_file(configured: &str) -> PathBuf {
    let path = PathBuf::from(configured.trim());
    if path.is_absolute() {
        return path;
    }
    let new_path = config_dir().join(&path);
    let legacy_path = app_dir().join(&path);
    if new_path.exists() || !legacy_path.exists() {
        new_path
    } else {
        // Keep a working fallback if moving a custom relative file was blocked.
        legacy_path
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn known_hosts_path() -> PathBuf {
    config_dir().join("known_hosts")
}

/// Resolve a configured relative asset path from config/ while continuing to
/// recognize an old exe-relative path if migration could not move that asset.
pub fn resolve_config_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let new_path = config_dir().join(path);
    let legacy_path = app_dir().join(path);
    if new_path.exists() || !legacy_path.exists() {
        new_path
    } else {
        legacy_path
    }
}

#[allow(dead_code)]
pub fn logs_dir() -> PathBuf {
    app_dir().join("logs")
}

#[allow(dead_code)]
pub fn highlight_dir() -> PathBuf {
    config_dir().join("highlight")
}

pub fn webview_data_dir() -> PathBuf {
    config_dir().join("webview-data")
}

/// M2: first `bin/ssh.exe` next to the exe (portability), then the OpenSSH
/// client shipped with Windows 11. The bundled executable is not user data.
pub fn ssh_program() -> Result<PathBuf, &'static str> {
    let bundled = app_dir().join("bin").join("ssh.exe");
    if bundled.is_file() {
        return Ok(bundled);
    }
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let candidates = [
        root.join("System32").join("OpenSSH").join("ssh.exe"),
        root.join("System32").join("ssh.exe"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("ssh_not_found")
}

/// The exe itself acts as the SSH_ASKPASS helper (mode detected by environment
/// variable in `main`). This avoids creating any auxiliary file on disk.
pub fn askpass_program() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| app_dir().join("sshterm.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scripts_vault_path_defaults_and_resolves_relative_paths() {
        let base = Path::new("base");
        assert_eq!(resolve_config_file_from("", base), base.join("scripts.vault"));
        assert_eq!(resolve_config_file_from("  ", base), base.join("scripts.vault"));
        assert_eq!(resolve_config_file_from("lib/team.vault", base), base.join("lib/team.vault"));
        let absolute = std::env::temp_dir().join("shared.vault");
        assert_eq!(resolve_config_file_from(absolute.to_str().unwrap(), base), absolute);
    }

    fn resolve_config_file_from(configured: &str, base: &Path) -> PathBuf {
        let configured = configured.trim();
        if configured.is_empty() {
            return base.join("scripts.vault");
        }
        let path = PathBuf::from(configured);
        if path.is_absolute() { path } else { base.join(path) }
    }

    #[test]
    fn migrates_all_legacy_user_data_but_leaves_logs_and_bundled_files_in_place() {
        let dir = tempdir().unwrap();
        let app = dir.path();
        fs::write(app.join("config.toml"), r#"
scripts_vault = "team/scripts.vault"
[appearance]
background_image = "assets/background.png"
"#).unwrap();
        fs::write(app.join("sshterm.vault"), b"encrypted-main").unwrap();
        fs::write(app.join("scripts.vault"), b"encrypted-default-scripts").unwrap();
        fs::write(app.join("known_hosts"), b"known hosts").unwrap();
        fs::write(app.join("window-state.json"), b"window state").unwrap();
        fs::create_dir_all(app.join("webview-data/cache")).unwrap();
        fs::write(app.join("webview-data/cache/index"), b"cache").unwrap();
        fs::create_dir_all(app.join("highlight")).unwrap();
        fs::write(app.join("highlight/linux.toml"), b"rules").unwrap();
        fs::create_dir_all(app.join("team")).unwrap();
        fs::write(app.join("team/scripts.vault"), b"custom scripts").unwrap();
        fs::create_dir_all(app.join("assets")).unwrap();
        fs::write(app.join("assets/background.png"), b"image").unwrap();
        fs::create_dir_all(app.join("logs")).unwrap();
        fs::write(app.join("logs/session.log"), b"log").unwrap();
        fs::create_dir_all(app.join("bin")).unwrap();
        fs::write(app.join("bin/ssh.exe"), b"ssh").unwrap();

        migrate_legacy_layout_at(app).unwrap();

        let config = app.join("config");
        let migrated_settings = Config::load(&config.join("config.toml"));
        assert_eq!(migrated_settings.scripts_vault, "team/scripts.vault");
        assert_eq!(migrated_settings.appearance.background_image, "assets/background.png");
        assert_eq!(fs::read(config.join("sshterm.vault")).unwrap(), b"encrypted-main");
        assert_eq!(fs::read(config.join("scripts.vault")).unwrap(), b"encrypted-default-scripts");
        assert_eq!(fs::read(config.join("known_hosts")).unwrap(), b"known hosts");
        assert_eq!(fs::read(config.join("window-state.json")).unwrap(), b"window state");
        assert_eq!(fs::read(config.join("webview-data/cache/index")).unwrap(), b"cache");
        assert_eq!(fs::read(config.join("highlight/linux.toml")).unwrap(), b"rules");
        assert_eq!(fs::read(config.join("team/scripts.vault")).unwrap(), b"custom scripts");
        assert_eq!(fs::read(config.join("assets/background.png")).unwrap(), b"image");
        assert_eq!(fs::read(app.join("logs/session.log")).unwrap(), b"log");
        assert_eq!(fs::read(app.join("bin/ssh.exe")).unwrap(), b"ssh");
        assert!(!app.join("sshterm.vault").exists());
    }

    #[test]
    fn migration_never_overwrites_data_already_in_config_folder() {
        let dir = tempdir().unwrap();
        let app = dir.path();
        fs::write(app.join("sshterm.vault"), b"legacy").unwrap();
        fs::create_dir_all(app.join("config")).unwrap();
        fs::write(app.join("config/sshterm.vault"), b"current").unwrap();

        migrate_legacy_layout_at(app).unwrap();

        assert_eq!(fs::read(app.join("config/sshterm.vault")).unwrap(), b"current");
        assert_eq!(fs::read(app.join("sshterm.vault")).unwrap(), b"legacy");
    }
}
