//! Commands the frontend can call with `invoke("name", {...})`.

use std::io::Write;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, State};

use crate::config::{self, Config};
use crate::paths;
use crate::store;
use crate::script_store::{self, ScriptsLibrary, ScriptsState};
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
pub async fn vault_create(
    state: State<'_, VaultState>,
    scripts: State<'_, ScriptsState>,
    password: String,
) -> Result<(), String> {
    let path = paths::vault_path();
    let (created, library) = tauri::async_runtime::spawn_blocking(move || {
        let created = Vault::create(&path, &password);
        let library = if created.is_ok() { script_store::open_with_master(&password) } else { ScriptsLibrary::Closed };
        (created, library)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut vault = created.map_err(code)?;
    *scripts.0.lock().unwrap_or_else(|e| e.into_inner()) = library;
    // M4: a new vault already has a typed payload, but run the same
    // migration path as unlock so both are always in a consistent shape.
    if store::migrate(&mut vault) {
        vault.save().map_err(code)?;
    }
    *lock(&state) = Some(vault);
    Ok(())
}

#[tauri::command]
pub async fn vault_unlock(
    state: State<'_, VaultState>,
    scripts: State<'_, ScriptsState>,
    password: String,
) -> Result<(), String> {
    let path = paths::vault_path();
    // The scripts library opens with the same password in the same background task.
    let (unlocked, library) = tauri::async_runtime::spawn_blocking(move || {
        let unlocked = Vault::unlock(&path, &password);
        let library = if unlocked.is_ok() { script_store::open_with_master(&password) } else { ScriptsLibrary::Closed };
        (unlocked, library)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut vault = unlocked.map_err(code)?;
    *scripts.0.lock().unwrap_or_else(|e| e.into_inner()) = library;
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
pub async fn vault_lock(app: AppHandle) -> Result<(), String> {
    // SFTP credentials live only in the encrypted vault. Close native SFTP
    // sessions before dropping it, regardless of the tunnel-lock preference.
    let sftp_state = app.state::<crate::sftp::SftpState>();
    let _ = crate::sftp::close_all(&sftp_state).await;

    tauri::async_runtime::spawn_blocking(move || {
        let state_app = app.clone();
        let state = state_app.state::<VaultState>();
        let tunnel_state = state_app.state::<crate::tunnels::TunnelState>();
        let app_behavior = Config::load(&paths::config_path()).app_behavior;
        let mut vault = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if app_behavior.stop_tunnels_on_lock {
            crate::tunnels::stop_all_for_vault_lock(&tunnel_state);
        }
        *vault = None;
        script_store::close(&state_app.state::<ScriptsState>());
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, VaultState>,
    scripts: State<'_, ScriptsState>,
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
    let new_password_for_scripts = new_password.clone();
    let (vault, result) = tauri::async_runtime::spawn_blocking(move || {
        let r = vault.change_password(&new_password);
        (vault, r)
    })
    .await
    .map_err(|e| e.to_string())?;
    *lock(&state) = Some(vault);
    result.map_err(code)?;
    // Keep the scripts library on the same password (when it was opened with it).
    script_store::change_master_password(&scripts, &new_password_for_scripts);
    Ok(())
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


/// Background image and transparency; stored outside the vault so the lock screen can use them.
#[tauri::command]
pub fn config_get_appearance_settings() -> config::AppearanceSettings {
    Config::load(&paths::config_path()).appearance
}

#[tauri::command]
pub fn config_set_appearance_settings(settings: config::AppearanceSettings) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.appearance = settings.normalized();
    cfg.save(&path).map_err(|e| e.to_string())
}

/// Largest background image the app will load (it is sent to the webview as a data URL).
const MAX_BACKGROUND_BYTES: u64 = 20 * 1024 * 1024;

/// Reads the background image and returns it as a `data:` URL. Relative paths are
/// resolved against config/, with the old exe-relative location retained as a migration fallback.
/// Errors are short codes the frontend translates.
#[tauri::command]
pub fn appearance_background_data(path: String) -> Result<String, String> {
    use base64::Engine as _;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("background_missing".into());
    }
    let mut file = std::path::PathBuf::from(trimmed);
    if file.is_relative() {
        file = paths::resolve_config_path(&file);
    }
    let mime = match file.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => return Err("background_format".into()),
    };
    let meta = std::fs::metadata(&file).map_err(|_| "background_missing".to_string())?;
    if !meta.is_file() {
        return Err("background_missing".into());
    }
    if meta.len() > MAX_BACKGROUND_BYTES {
        return Err("background_too_large".into());
    }
    let bytes = std::fs::read(&file).map_err(|_| "background_missing".to_string())?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Whether application diagnostics are enabled (stored outside the vault).
#[tauri::command]
pub fn config_get_debug_logging() -> bool {
    Config::load(&paths::config_path()).debug_logging
}

#[tauri::command]
pub fn config_set_debug_logging(enabled: bool) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.debug_logging = enabled;
    cfg.save(&path).map_err(|e| e.to_string())
}

/// Append one allowlisted diagnostic event. Terminal content and user input are
/// never passed here. Keep the file bounded and rotate it once it reaches 2 MiB.
static DEBUG_LOG_LOCK: Mutex<()> = Mutex::new(());

#[tauri::command]
pub fn debug_log_append(event: String) -> Result<(), String> {
    if !Config::load(&paths::config_path()).debug_logging {
        return Ok(());
    }
    let _guard = DEBUG_LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if event.len() > 4096 {
        return Err("debug_event_too_large".into());
    }
    let event = event.replace('\r', " ").replace('\n', " ");
    let dir = paths::logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("debug.log");
    const MAX_BYTES: u64 = 2 * 1024 * 1024;
    if std::fs::metadata(&path).map(|m| m.len() >= MAX_BYTES).unwrap_or(false) {
        let previous = dir.join("debug.log.1");
        let _ = std::fs::remove_file(&previous);
        std::fs::rename(&path, &previous).map_err(|e| e.to_string())?;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{timestamp} {event}").map_err(|e| e.to_string())
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

/// Application lifecycle preferences are stored outside the encrypted vault.
#[tauri::command]
pub fn config_get_app_behavior_settings() -> config::AppBehaviorSettings {
    Config::load(&paths::config_path()).app_behavior
}

#[tauri::command]
pub fn config_set_app_behavior_settings(settings: config::AppBehaviorSettings) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.app_behavior = settings;
    cfg.save(&path).map_err(|e| e.to_string())
}

/// Command-console layout preferences are non-secret and never include the draft.
#[tauri::command]
pub fn config_get_command_console_settings() -> config::CommandConsoleSettings {
    Config::load(&paths::config_path()).command_console.normalized()
}

#[tauri::command]
pub fn config_set_command_console_settings(
    settings: config::CommandConsoleSettings,
) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.command_console = settings.normalized();
    cfg.save(&path).map_err(|e| e.to_string())
}

/// Connection-tree and properties-panel visibility are non-secret UI preferences.
#[tauri::command]
pub fn config_get_workspace_panel_settings() -> config::WorkspacePanelSettings {
    Config::load(&paths::config_path()).workspace_panels
}

#[tauri::command]
pub fn config_set_workspace_panel_settings(
    settings: config::WorkspacePanelSettings,
) -> Result<(), String> {
    let path = paths::config_path();
    let mut cfg = Config::load(&path);
    cfg.workspace_panels = settings;
    cfg.save(&path).map_err(|e| e.to_string())
}
