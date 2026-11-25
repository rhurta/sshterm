mod askpass;
mod commands;
mod config;
mod crash;
mod logger;
mod highlight;
mod local_browser;
mod paths;
mod script_store;
mod session;
mod store;
pub mod sftp;
mod transport;
mod tunnels;
mod vault;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::window::{Effect, EffectsBuilder};
use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, RunEvent, Size, WebviewWindow, WebviewWindowBuilder, WindowEvent};

/// Window backdrop the main window was created with (changing the setting needs a restart).
struct ActiveBackdrop(config::WindowBackdrop);

#[tauri::command]
fn window_backdrop_active(state: tauri::State<'_, ActiveBackdrop>) -> config::WindowBackdrop {
    state.0
}

/// Creates the main window from `tauri.conf.json` (it is declared with `create: false`)
/// so transparency, which can only be chosen at creation time, follows the saved setting.
fn build_main_window(app: &tauri::App, backdrop: config::WindowBackdrop) -> tauri::Result<WebviewWindow> {
    let window_config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let mut builder = WebviewWindowBuilder::from_config(app.handle(), &window_config)?;
    if backdrop != config::WindowBackdrop::Off {
        // An undecorated window with a shadow gets a 1px light border on Windows;
        // it would show around a see-through window.
        builder = builder.transparent(true).shadow(false);
    }
    let window = builder.build()?;
    if backdrop == config::WindowBackdrop::Mica {
        // Needs Windows 11. Anywhere else it fails and the window stays see-through.
        let _ = window.set_effects(EffectsBuilder::new().effect(Effect::Mica).build());
    }
    Ok(window)
}

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

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
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
    // First thing: leave evidence in logs/crash.log if any thread panics
    // (release builds abort silently: panic = "abort", no console).
    crash::install();

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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let backdrop = crate::config::Config::load(&paths::config_path()).appearance.window_backdrop;
            app.manage(ActiveBackdrop(backdrop));
            build_main_window(app, backdrop)?;
            if let Some(window) = app.get_webview_window("main") {
                restore_window_state(&window);
                let save_path = window_state_path();
                let event_window = window.clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                        save_window_state(&event_window, &save_path);
                    }
                    WindowEvent::CloseRequested { api, .. } => {
                        save_window_state(&event_window, &save_path);
                        if crate::config::Config::load(&paths::config_path()).app_behavior.close_to_tray {
                            api.prevent_close();
                            let _ = event_window.hide();
                        }
                    }
                    _ => {}
                });
            }

            // Keep a tray entry available so a hidden window can always be restored or exited.
            if let Some(icon) = app.default_window_icon().cloned() {
                let open = MenuItem::with_id(app, "open", "Open SSH Term", true, None::<&str>)?;
                let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &exit])?;
                let tray = TrayIconBuilder::new()
                    .icon(icon)
                    .tooltip("SSH Term")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "open" => show_main_window(app),
                        "exit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::DoubleClick { .. } = event {
                            show_main_window(tray.app_handle());
                        }
                    })
                    .build(app)?;
                // TrayIcon is reference-counted; retain a handle for the app lifetime.
                app.manage(tray);
            }
            Ok(())
        })
        .manage(commands::VaultState::default())
        .manage(script_store::ScriptsState::default())
        .manage(session::SessionState::default())
        .manage(tunnels::TunnelState::default())
        .manage(sftp::SftpState::default())
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
            commands::config_get_appearance_settings,
            window_backdrop_active,
            script_store::scripts_status,
            script_store::scripts_unlock,
            script_store::scripts_tree,
            script_store::scripts_get,
            script_store::scripts_search,
            script_store::scripts_save,
            script_store::scripts_favorites,
            script_store::scripts_set_favorite,
            script_store::scripts_create_folder,
            script_store::scripts_rename,
            script_store::scripts_move,
            script_store::scripts_delete,
            commands::config_set_appearance_settings,
            commands::appearance_background_data,
            commands::config_get_terminal_settings,
            commands::config_set_terminal_settings,
            commands::config_get_command_console_settings,
            commands::config_set_command_console_settings,
            commands::config_get_workspace_panel_settings,
            commands::config_set_workspace_panel_settings,
            commands::config_get_app_behavior_settings,
            commands::config_set_app_behavior_settings,
            store::conn_list,
            store::conn_export,
            store::conn_export_ssh_config,
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
            local_browser::local_list_directory,
            local_browser::local_create_directory,
            local_browser::local_rename_entry,
            local_browser::local_delete_many,
            sftp::sftp_open,
            sftp::sftp_list,
            sftp::sftp_mkdir,
            sftp::sftp_rename,
            sftp::sftp_delete,
            sftp::sftp_delete_many,
            sftp::sftp_transfer_start,
            sftp::sftp_transfer_cancel,
            sftp::sftp_close,
            sftp::sftp_close_all,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if matches!(event, RunEvent::Exit) {
                let tunnels = app_handle.state::<tunnels::TunnelState>();
                tunnels::stop_all_for_app_exit(&tunnels);
                let sftp = app_handle.state::<sftp::SftpState>();
                let _ = tauri::async_runtime::block_on(sftp::close_all(&sftp));
            }
        });
}
