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
