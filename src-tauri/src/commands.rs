//! Commands the frontend can call with `invoke("name", {...})`.

use std::sync::{Mutex, MutexGuard};

use tauri::State;

use crate::config::{self, Config};
use crate::paths;
use crate::store;
use crate::vault::{Vault, VaultError};

/// Shared application state: the open vault, or nothing if locked.
#[derive(Default)]
pub struct VaultState(pub Mutex<Option<Vault>>);

fn lock<'a>(state: &'a State<'_, VaultState>) -> MutexGuard<'a, Option<Vault>> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Errors travel to the frontend as a short code ("wrong_password"...).
fn code(e: VaultError) -> String {
    e.code().to_string()
}

/// "missing" (no vault), "locked" (exists but closed) or "unlocked".
#[tauri::command]
pub fn vault_status(state: State<'_, VaultState>) -> &'static str {
    if lock(&state).is_some() {
        "unlocked"
    } else if paths::vault_path().exists() {
        "locked"
    } else {
        "missing"
    }
}

/// Argon2 uses 64 MiB and takes a fraction of a second: it runs on a separate
/// thread (`spawn_blocking`) so the window does not freeze.
#[tauri::command]
pub async fn vault_create(state: State<'_, VaultState>, password: String) -> Result<(), String> {
    let path = paths::vault_path();
    let mut vault = tauri::async_runtime::spawn_blocking(move || Vault::create(&path, &password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(code)?;
    // M4: a new vault already has a typed payload, but run the same
    // migration path as unlock so both are always in a consistent shape.
    if store::migrate(&mut vault) {
        vault.save().map_err(code)?;
    }
    *lock(&state) = Some(vault);
    Ok(())
}

#[tauri::command]
pub async fn vault_unlock(state: State<'_, VaultState>, password: String) -> Result<(), String> {
    let path = paths::vault_path();
    let mut vault = tauri::async_runtime::spawn_blocking(move || Vault::unlock(&path, &password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(code)?;
    // M4: M1-M3 vaults have a flat connection list and a plain `password`
    // field. Wrap them in one folder and move the password into `auth` before
    // the UI sees the data. Payload version and file format stay unchanged.
    if store::migrate(&mut vault) {
        vault.save().map_err(code)?;
    }
    *lock(&state) = Some(vault);
    Ok(())
}

/// Locking = dropping the vault. The key is cleared from memory on drop.
#[tauri::command]
pub fn vault_lock(state: State<'_, VaultState>) {
    *lock(&state) = None;
}

#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, VaultState>,
    current: String,
    new_password: String,
) -> Result<(), String> {
    // 1) verify the current password against the file
    let path = paths::vault_path();
    tauri::async_runtime::spawn_blocking(move || Vault::unlock(&path, &current).map(drop))
        .await
        .map_err(|e| e.to_string())?
        .map_err(code)?;

    // 2) take the vault out of state, change the password, and put it back
    let mut vault = lock(&state).take().ok_or_else(|| "locked".to_string())?;
    let (vault, result) = tauri::async_runtime::spawn_blocking(move || {
        let r = vault.change_password(&new_password);
        (vault, r)
    })
    .await
    .map_err(|e| e.to_string())?;
    *lock(&state) = Some(vault);
    result.map_err(code)
}

/// Language saved in config.toml ("en" if nothing is saved).
#[tauri::command]
pub fn config_get_language() -> String {
    Config::load(&paths::config_path()).language
}

#[tauri::command]
pub fn config_set_language(language: String) -> Result<(), String> {
    let language = config::normalize_language(&language).ok_or_else(|| "invalid_language".to_string())?;
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.language = language;
    cfg.save(&path).map_err(|e| e.to_string())
}

/// Theme is stored outside the vault so it is available on the lock screen.
#[tauri::command]
pub fn config_get_theme() -> config::AppTheme {
    Config::load(&paths::config_path()).theme
}

#[tauri::command]
pub fn config_set_theme(theme: config::AppTheme) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.theme = theme;
    cfg.save(&path).map_err(|e| e.to_string())
}


/// Terminal clipboard and mouse preferences stored in config.toml.
#[tauri::command]
pub fn config_get_terminal_settings() -> config::TerminalSettings {
    Config::load(&paths::config_path()).terminal
}

#[tauri::command]
pub fn config_set_terminal_settings(settings: config::TerminalSettings) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.terminal = settings;
    cfg.save(&path).map_err(|e| e.to_string())
}
