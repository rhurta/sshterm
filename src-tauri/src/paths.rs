use std::path::PathBuf;

/// Carpeta que contiene el exe. Todos los datos de la aplicación viven aquí.
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
#[allow(dead_code)]
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
