mod askpass;
mod commands;
mod config;
mod logger;
mod paths;
mod session;
mod store;
mod transport;
mod vault;

#[tauri::command]
fn app_info() -> String {
    paths::app_dir().display().to_string()
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
        .manage(commands::VaultState::default())
        .manage(session::SessionState::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            commands::vault_status,
            commands::vault_create,
            commands::vault_unlock,
            commands::vault_lock,
            commands::vault_change_password,
            commands::config_get_language,
            commands::config_set_language,
            store::conn_list,
            store::conn_get,
            store::conn_upsert,
            store::conn_delete,
            store::tree_get,
            store::folder_upsert,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
