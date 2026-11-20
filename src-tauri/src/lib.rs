mod askpass;
mod commands;
mod config;
mod logger;
mod highlight;
mod paths;
mod session;
mod store;
mod transport;
mod tunnels;
mod vault;

use serde::{Deserialize, Serialize};
use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, Size, WindowEvent};

#[tauri::command]
fn app_info() -> String {
    paths::app_dir().display().to_string()
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SavedWindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
}

fn window_state_path() -> std::path::PathBuf {
    paths::app_dir().join("window-state.json")
}

fn save_window_state(window: &tauri::WebviewWindow, path: &std::path::Path) {
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let maximized = window.is_maximized().unwrap_or(false);
    let previous = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SavedWindowState>(&bytes).ok());
    let bounds = if maximized {
        previous
    } else {
        let position = window.outer_position().ok();
        let size = window.outer_size().ok();
        position.zip(size).map(|(position, size)| SavedWindowState {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized: false,
        })
    };
    if let Some(mut state) = bounds {
        state.maximized = maximized;
        if let Ok(bytes) = serde_json::to_vec_pretty(&state) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

fn restore_window_state(window: &tauri::WebviewWindow) {
    let Ok(bytes) = std::fs::read(window_state_path()) else { return };
    let Ok(state) = serde_json::from_slice::<SavedWindowState>(&bytes) else { return };
    if state.width < 640 || state.height < 480 {
        return;
    }
    let _ = window.set_size(Size::Physical(PhysicalSize::new(state.width, state.height)));
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(state.x, state.y)));
    if state.maximized {
        let _ = window.maximize();
    }
}

/// SSH_ASKPASS helper mode: `ssh.exe` launches this same exe with the pipe
/// environment variable (or with `--askpass`). It is detected before creating
/// the window, so the helper does not initialize WebView2 or write anything
/// to disk. Returns `Some(exit_code)` when this process is the helper.
pub fn maybe_run_askpass() -> Option<i32> {
    let by_env = std::env::var_os(askpass::PIPE_ENV).is_some();
    let by_arg = std::env::args().any(|a| a == "--askpass");
    if by_env || by_arg {
        Some(askpass::run_helper())
    } else {
        None
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebView2 cache is stored next to the exe (portable application).
    let data = paths::webview_data_dir();
    let _ = std::fs::create_dir_all(&data);
    // SAFETY: `set_var` is `unsafe` since Rust edition 2024 because changing the
    // environment while another thread reads it is undefined behaviour. Here it runs
    // at the start of `run()`, before Tauri creates any thread or window.
    // Do not move it after `tauri::Builder`, and do not add other threads before it.
    unsafe {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &data);
    }

    tauri::Builder::default()
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                restore_window_state(&window);
                let save_path = window_state_path();
                let event_window = window.clone();
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::CloseRequested { .. }) {
                        save_window_state(&event_window, &save_path);
                    }
                });
            }
            Ok(())
        })
        .manage(commands::VaultState::default())
        .manage(session::SessionState::default())
        .manage(tunnels::TunnelState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            highlight::highlight_list,
            highlight::highlight_test,
            highlight::highlight_match,
            store::highlight_resolve,
            commands::vault_status,
            commands::vault_create,
            commands::vault_unlock,
            commands::vault_lock,
            commands::vault_change_password,
            commands::config_get_language,
            commands::config_set_language,
            commands::config_get_theme,
            commands::config_set_theme,
            commands::config_get_terminal_settings,
            commands::config_set_terminal_settings,
            commands::config_get_command_console_settings,
            commands::config_set_command_console_settings,
            store::conn_list,
            store::conn_get,
            store::conn_upsert,
            store::conn_delete,
            store::tree_get,
            store::folder_upsert,
            store::folder_set_highlight,
            store::folder_delete,
            store::move_node,
            store::search,
            store::import_preview,
            store::import_apply,
            session::session_open,
            session::session_write,
            session::session_resize,
            session::session_close,
            session::session_close_all,
            session::session_log_append,
            session::session_log_close,
            tunnels::tunnel_list,
            tunnels::tunnel_start,
            tunnels::tunnel_stop,
            tunnels::tunnel_stop_all,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
