mod accounts;
mod inventory;
mod session;

use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(inventory::InventoryState::default())
        .manage(steam_core::SessionManager::new(
            steam_core::SteamClient,
            std::time::Duration::from_secs(120),
        ))
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            let store =
                accounts::initialize_store(path, settings::legacy::legacy_store_candidates())?;
            app.manage(Mutex::new(store));
            app.manage(item_schema::ItemSchema::bundled()?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            accounts::store_get_accounts,
            accounts::store_delete_account,
            session::steam_login,
            session::steam_session,
            session::steam_sign_out,
            inventory::refresh_inventory
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
