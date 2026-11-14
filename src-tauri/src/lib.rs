mod commands;
mod config;
mod paths;
mod vault;

#[tauri::command]
fn app_info() -> String {
    paths::app_dir().display().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // La caché de WebView2 se guarda junto al exe (aplicación portable).
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
        .invoke_handler(tauri::generate_handler![
            app_info,
            commands::vault_status,
            commands::vault_create,
            commands::vault_unlock,
            commands::vault_lock,
            commands::vault_change_password,
            commands::config_get_language,
            commands::config_set_language,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}