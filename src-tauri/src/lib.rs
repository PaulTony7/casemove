mod accounts;

use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            let store =
                accounts::initialize_store(path, settings::legacy::legacy_store_candidates())?;
            app.manage(Mutex::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            accounts::store_get_accounts,
            accounts::store_delete_account
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
