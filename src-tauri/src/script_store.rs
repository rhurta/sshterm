//! Scripts library commands.
//!
//! The library is a second encrypted file (`scripts.vault`) next to the connections
//! vault. It is opened with the same master password during unlock; if that fails
//! because the file has its own password, the UI asks for it (`scripts_unlock`).

use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::paths;
use crate::vault::scripts::{ScriptDetail, ScriptError, ScriptSummary, ScriptsData};
use crate::vault::{ScriptsVault, VaultError};

pub enum ScriptsLibrary {
    /// Not opened (the connections vault is locked).
    Closed,
    /// `with_master`: opened with the master password, so changing the master
    /// password re-encrypts the library too.
    Ready { vault: Box<ScriptsVault>, with_master: bool },
    /// The file exists but the master password does not open it.
    NeedsPassword,
    Failed(String),
}

impl Default for ScriptsLibrary {
    fn default() -> Self {
        Self::Closed
    }
}

#[derive(Default)]
pub struct ScriptsState(pub Mutex<ScriptsLibrary>);

fn guard<'a>(state: &'a State<'_, ScriptsState>) -> MutexGuard<'a, ScriptsLibrary> {
    state.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Opens the library with the master password, creating it when the file is
/// missing. Never fails the unlock: problems become a state the UI explains.
pub fn open_with_master(password: &str) -> ScriptsLibrary {
    let path = paths::scripts_vault_path();
    match ScriptsVault::unlock(&path, password) {
        Ok(vault) => ScriptsLibrary::Ready { vault: Box::new(vault), with_master: true },
        Err(VaultError::NotFound) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match ScriptsVault::create(&path, password) {
                Ok(vault) => ScriptsLibrary::Ready { vault: Box::new(vault), with_master: true },
                Err(error) => ScriptsLibrary::Failed(error.code().to_string()),
            }
        }
        Err(VaultError::WrongPasswordOrCorrupt) => ScriptsLibrary::NeedsPassword,
        Err(error) => ScriptsLibrary::Failed(error.code().to_string()),
    }
}

/// Re-encrypts the library with the new master password when it was opened with the
/// old one. Best effort: the connections vault has already changed by then.
pub fn change_master_password(state: &ScriptsState, new_password: &str) {
    let mut library = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if let ScriptsLibrary::Ready { vault, with_master } = &mut *library {
        if *with_master && vault.change_password(new_password).is_err() {
            *with_master = false;
        }
    }
}

pub fn close(state: &ScriptsState) {
    *state.0.lock().unwrap_or_else(|e| e.into_inner()) = ScriptsLibrary::Closed;
}

#[derive(Serialize)]
pub struct ScriptsStatus {
    /// "ready" | "needs_password" | "failed" | "closed"
    pub state: &'static str,
    pub detail: Option<String>,
}

#[tauri::command]
pub fn scripts_status(state: State<'_, ScriptsState>) -> ScriptsStatus {
    match &*guard(&state) {
        ScriptsLibrary::Ready { .. } => ScriptsStatus { state: "ready", detail: None },
        ScriptsLibrary::NeedsPassword => ScriptsStatus { state: "needs_password", detail: None },
        ScriptsLibrary::Failed(code) => ScriptsStatus { state: "failed", detail: Some(code.clone()) },
        ScriptsLibrary::Closed => ScriptsStatus { state: "closed", detail: None },
    }
}

#[tauri::command]
pub async fn scripts_unlock(state: State<'_, ScriptsState>, password: String) -> Result<(), String> {
    let path = paths::scripts_vault_path();
    let vault = tauri::async_runtime::spawn_blocking(move || ScriptsVault::unlock(&path, &password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.code().to_string())?;
    *guard(&state) = ScriptsLibrary::Ready { vault: Box::new(vault), with_master: false };
    Ok(())
}

fn library_error(library: &ScriptsLibrary) -> String {
    match library {
        ScriptsLibrary::Closed => "locked".into(),
        ScriptsLibrary::NeedsPassword => "needs_password".into(),
        ScriptsLibrary::Failed(code) => format!("failed:{code}"),
        ScriptsLibrary::Ready { .. } => unreachable!("ready libraries are handled by the caller"),
    }
}

fn read<T>(state: &State<'_, ScriptsState>, f: impl FnOnce(&ScriptsData) -> Result<T, ScriptError>) -> Result<T, String> {
    let library = guard(state);
    match &*library {
        ScriptsLibrary::Ready { vault, .. } => f(&vault.data).map_err(|e| e.code().to_string()),
        other => Err(library_error(other)),
    }
}

/// Applies a change and saves it; on a failed save the in-memory data is restored.
fn mutate<T>(
    state: &State<'_, ScriptsState>,
    f: impl FnOnce(&mut ScriptsData) -> Result<T, ScriptError>,
) -> Result<T, String> {
    let mut library = guard(state);
    match &mut *library {
        ScriptsLibrary::Ready { vault, .. } => {
            let backup = vault.data.clone();
            let value = f(&mut vault.data).map_err(|e| e.code().to_string())?;
            if let Err(error) = vault.save() {
                vault.data = backup;
                return Err(error.code().to_string());
            }
            Ok(value)
        }
        other => Err(library_error(other)),
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[tauri::command]
pub fn scripts_tree(state: State<'_, ScriptsState>) -> Result<Vec<ScriptSummary>, String> {
    read(&state, |data| Ok(data.summaries()))
}

#[tauri::command]
pub fn scripts_get(state: State<'_, ScriptsState>, id: String) -> Result<ScriptDetail, String> {
    read(&state, |data| data.get(&id))
}

#[tauri::command]
pub fn scripts_search(state: State<'_, ScriptsState>, query: String) -> Result<Vec<String>, String> {
    read(&state, |data| Ok(data.search(&query)))
}

#[derive(Deserialize)]
pub struct SaveScript {
    /// Set to update an existing script (body, and name/folder if they changed).
    pub id: Option<String>,
    pub folder_id: Option<String>,
    pub name: String,
    pub body: String,
    /// Replace a script with the same name in the folder (new scripts only).
    #[serde(default)]
    pub overwrite: bool,
}

#[tauri::command]
pub fn scripts_save(state: State<'_, ScriptsState>, request: SaveScript) -> Result<String, String> {
    mutate(&state, |data| {
        data.save_script(
            request.id.as_deref(),
            request.folder_id.as_deref(),
            &request.name,
            &request.body,
            request.overwrite,
            now(),
        )
    })
}

#[tauri::command]
pub fn scripts_create_folder(
    state: State<'_, ScriptsState>,
    parent_id: Option<String>,
    name: String,
) -> Result<String, String> {
    mutate(&state, |data| data.create_folder(parent_id.as_deref(), &name))
}

#[tauri::command]
pub fn scripts_rename(state: State<'_, ScriptsState>, id: String, name: String) -> Result<(), String> {
    mutate(&state, |data| data.rename(&id, &name))
}

#[tauri::command]
pub fn scripts_move(state: State<'_, ScriptsState>, id: String, parent_id: Option<String>) -> Result<(), String> {
    mutate(&state, |data| data.move_node(&id, parent_id.as_deref()))
}

#[tauri::command]
pub fn scripts_delete(state: State<'_, ScriptsState>, id: String) -> Result<(), String> {
    mutate(&state, |data| data.delete(&id))
}
