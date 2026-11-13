use app_commands::SavedAccount;
use serde_json::{Map, Value};
use settings::SettingsStore;
use std::path::PathBuf;
use std::sync::Mutex;

pub fn initialize_store(
    path: PathBuf,
    candidates: Vec<PathBuf>,
) -> Result<SettingsStore, settings::store::Error> {
    let mut store = SettingsStore::open(path)?;
    if !store.data().as_object().is_some_and(|data| data.is_empty()) {
        return Ok(store);
    }
    if let Some(legacy_path) = candidates.into_iter().find(|candidate| candidate.is_file()) {
        match settings::migrate_legacy_store(&legacy_path) {
            Ok(migration) => migration.apply(&mut store)?,
            Err(error) => {
                eprintln!("Could not import legacy settings: {error}");
                return Ok(store);
            }
        }
    }
    store.set("migration.legacyComplete", true)?;
    Ok(store)
}

fn list_accounts(store: &SettingsStore) -> Vec<SavedAccount> {
    let entries = store
        .get::<Map<String, Value>>("account")
        .unwrap_or_default();
    let order = store
        .get::<Vec<String>>("accountKeyList")
        .unwrap_or_default();
    let mut accounts: Vec<_> = entries
        .iter()
        .map(|(username, entry)| SavedAccount {
            username: username.clone(),
            display_name: entry
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(username)
                .to_owned(),
        })
        .collect();
    accounts.sort_by_key(|account| {
        order
            .iter()
            .position(|name| name == &account.username)
            .unwrap_or(usize::MAX)
    });
    accounts
}

fn remove_account(store: &mut SettingsStore, username: &str) -> Result<(), String> {
    let mut entries = store
        .get::<Map<String, Value>>("account")
        .unwrap_or_default();
    entries.remove(username);
    store
        .set("account", entries)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn store_get_accounts(
    store: tauri::State<'_, Mutex<SettingsStore>>,
) -> Result<Vec<SavedAccount>, String> {
    let store = store.lock().map_err(|error| error.to_string())?;
    Ok(list_accounts(&store))
}

#[tauri::command]
pub fn store_delete_account(
    username: String,
    store: tauri::State<'_, Mutex<SettingsStore>>,
) -> Result<Vec<SavedAccount>, String> {
    let mut store = store.lock().map_err(|error| error.to_string())?;
    remove_account(&mut store, &username)?;
    Ok(list_accounts(&store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_import_is_secret_free_and_does_not_repeat_after_removal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/settings/tests/fixtures/conf-encrypted.bin");
        let original = std::fs::read(&fixture).unwrap();
        let mut store = initialize_store(path.clone(), vec![fixture.clone()]).unwrap();
        assert_eq!(list_accounts(&store).len(), 3);
        assert_eq!(
            store.get::<String>("pricing.currency").as_deref(),
            Some("USD")
        );
        let entries = store.get::<Value>("account").unwrap();
        assert!(entries["alice"].get("refreshToken").is_none());
        assert!(entries["bob"].get("safeData").is_none());
        remove_account(&mut store, "alice").unwrap();
        let reopened = initialize_store(path, vec![fixture.clone()]).unwrap();
        assert_eq!(list_accounts(&reopened).len(), 2);
        assert_eq!(std::fs::read(fixture).unwrap(), original);
    }

    #[test]
    fn existing_rust_settings_take_priority_over_legacy_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        SettingsStore::open(&path)
            .unwrap()
            .set("pricing.currency", "EUR")
            .unwrap();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/settings/tests/fixtures/conf-encrypted.bin");
        let store = initialize_store(path, vec![fixture]).unwrap();
        assert_eq!(
            store.get::<String>("pricing.currency").as_deref(),
            Some("EUR")
        );
        assert!(list_accounts(&store).is_empty());
    }

    #[test]
    fn account_metadata_is_ordered_secret_free_and_removal_persists() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::open(&path).unwrap();
        store
            .set(
                "account",
                serde_json::json!({
                    "alice": {"displayName": "Alice", "refreshToken": "private"},
                    "bob": {"displayName": "Bob", "safeData": "private"}
                }),
            )
            .unwrap();
        store.set("accountKeyList", vec!["bob", "alice"]).unwrap();
        let accounts = list_accounts(&store);
        assert_eq!(accounts[0].username, "bob");
        assert_eq!(accounts[1].display_name, "Alice");
        assert!(
            !serde_json::to_string(&accounts)
                .unwrap()
                .contains("private")
        );
        remove_account(&mut store, "bob").unwrap();
        let reopened = SettingsStore::open(path).unwrap();
        assert_eq!(list_accounts(&reopened).len(), 1);
        assert_eq!(list_accounts(&reopened)[0].username, "alice");
    }
}
