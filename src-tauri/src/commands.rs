//! Comandos que el frontend puede llamar con `invoke("nombre", {...})`.

use std::sync::{Mutex, MutexGuard};

use tauri::State;

use crate::config::{self, Config};
use crate::paths;
use crate::vault::{Vault, VaultError};

/// Estado compartido de la aplicación: el vault abierto, o nada si está bloqueado.
#[derive(Default)]
pub struct VaultState(pub Mutex<Option<Vault>>);

fn lock<'a>(state: &'a State<'_, VaultState>) -> MutexGuard<'a, Option<Vault>> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Los errores viajan al frontend como un código corto ("wrong_password"...).
fn code(e: VaultError) -> String {
    e.code().to_string()
}

/// "missing" (no hay vault), "locked" (existe pero está cerrado) o "unlocked".
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

/// Argon2 usa 64 MiB y tarda una fracción de segundo: se ejecuta en un hilo aparte (`spawn_blocking`)
/// para que la ventana no se congele.
#[tauri::command]
pub async fn vault_create(state: State<'_, VaultState>, password: String) -> Result<(), String> {
    let path = paths::vault_path();
    let vault = tauri::async_runtime::spawn_blocking(move || Vault::create(&path, &password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(code)?;
    *lock(&state) = Some(vault);
    Ok(())
}

#[tauri::command]
pub async fn vault_unlock(state: State<'_, VaultState>, password: String) -> Result<(), String> {
    let path = paths::vault_path();
    let vault = tauri::async_runtime::spawn_blocking(move || Vault::unlock(&path, &password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(code)?;
    *lock(&state) = Some(vault);
    Ok(())
}

/// Bloquear = soltar el vault. La clave se borra de memoria al destruirse.
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
    // 1) comprobar la contraseña actual contra el archivo
    let path = paths::vault_path();
    tauri::async_runtime::spawn_blocking(move || Vault::unlock(&path, &current).map(drop))
        .await
        .map_err(|e| e.to_string())?
        .map_err(code)?;

    // 2) sacar el vault del estado, cambiar la contraseña y devolverlo
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

/// Idioma guardado en config.toml ("en" si no hay nada guardado).
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
