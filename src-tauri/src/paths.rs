use std::path::PathBuf;

/// Folder containing the exe. All application data lives here.
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn vault_path() -> PathBuf {
    app_dir().join("sshterm.vault")
}
/// Scripts library file: `config.toml` `scripts_vault`, absolute or relative to the
/// exe folder; `scripts.vault` next to the exe by default.
pub fn scripts_vault_path() -> PathBuf {
    let configured = crate::config::Config::load(&config_path()).scripts_vault;
    resolve_scripts_vault(&configured, &app_dir())
}

fn resolve_scripts_vault(configured: &str, base: &std::path::Path) -> PathBuf {
    let configured = configured.trim();
    if configured.is_empty() {
        return base.join("scripts.vault");
    }
    let path = PathBuf::from(configured);
    if path.is_absolute() { path } else { base.join(path) }
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.toml")
}
pub fn known_hosts_path() -> PathBuf {
    app_dir().join("known_hosts")
}
#[allow(dead_code)]
pub fn logs_dir() -> PathBuf {
    app_dir().join("logs")
}
#[allow(dead_code)]
pub fn highlight_dir() -> PathBuf {
    app_dir().join("highlight")
}
pub fn webview_data_dir() -> PathBuf {
    app_dir().join("webview-data")
}

/// M2: first `bin/ssh.exe` next to the exe (portability), then the OpenSSH
/// client shipped with Windows 11. Returning a short code lets the frontend
/// translate the error without the backend knowing UI strings.
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
    use std::path::Path;

    #[test]
    fn scripts_vault_path_defaults_and_resolves_relative_paths() {
        let base = Path::new("base");
        assert_eq!(resolve_scripts_vault("", base), base.join("scripts.vault"));
        assert_eq!(resolve_scripts_vault("  ", base), base.join("scripts.vault"));
        assert_eq!(resolve_scripts_vault("lib/team.vault", base), base.join("lib/team.vault"));
        let absolute = std::env::temp_dir().join("shared.vault");
        assert_eq!(resolve_scripts_vault(absolute.to_str().unwrap(), base), absolute);
    }
}
