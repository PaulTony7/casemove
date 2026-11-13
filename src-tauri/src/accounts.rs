use app_commands::SavedAccount;
use serde_json::{Map, Value};
use settings::SettingsStore;
use std::path::PathBuf;
use std::sync::Mutex;
use zeroize::Zeroizing;

pub(crate) fn login_vault() -> settings::SecretStore {
    settings::SecretStore::new("com.nombers.casemove.login")
}

trait TokenVault {
    fn load(&self, account: &str) -> Result<Option<String>, ()>;
    fn save(&self, account: &str, token: &str) -> Result<(), ()>;
    fn remove(&self, account: &str) -> Result<(), ()>;
}

impl TokenVault for settings::SecretStore {
    fn load(&self, account: &str) -> Result<Option<String>, ()> {
        self.get_optional(account).map_err(|_| ())
    }
    fn save(&self, account: &str, token: &str) -> Result<(), ()> {
        self.set(account, token).map_err(|_| ())
    }
    fn remove(&self, account: &str) -> Result<(), ()> {
        self.delete(account).map_err(|_| ())
    }
}

fn restore_token(
    vault: &impl TokenVault,
    account: &str,
    previous: Option<&str>,
) -> Result<(), String> {
    match previous {
        Some(token) => vault.save(account, token),
        None => vault.remove(account),
    }
    .map_err(|_| {
        "Secure credential rollback failed. Check this account in your OS credential manager."
            .to_owned()
    })
}

fn remember_with_vault(
    store: &mut SettingsStore,
    vault: &impl TokenVault,
    account: &str,
    token: &str,
    commit: impl FnOnce(&mut SettingsStore) -> Result<(), String>,
) -> Result<(), String> {
    let previous = vault
        .load(account)
        .map_err(|_| "Secure credential storage is unavailable.".to_owned())?
        .map(Zeroizing::new);
    vault
        .save(account, token)
        .map_err(|_| "Could not save this login in secure credential storage.".to_owned())?;
    if let Err(error) = commit(store) {
        restore_token(
            vault,
            account,
            previous.as_ref().map(|token| token.as_str()),
        )?;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn remember_login(
    store: &mut SettingsStore,
    account: &str,
    token: &str,
    commit: impl FnOnce(&mut SettingsStore) -> Result<(), String>,
) -> Result<(), String> {
    remember_with_vault(store, &login_vault(), account, token, commit)
}

pub(crate) fn save_account_metadata(
    store: &mut SettingsStore,
    account: &str,
) -> Result<(), String> {
    let account = settings::secret::canonical_account(account);
    let mut entries = store
        .get::<Map<String, Value>>("account")
        .unwrap_or_default();
    let aliases: Vec<_> = entries
        .keys()
        .filter(|name| settings::secret::canonical_account(name) == account)
        .cloned()
        .collect();
    let mut metadata = entries.remove(&account);
    for alias in aliases {
        if let Some(entry) = entries.remove(&alias) {
            metadata.get_or_insert(entry);
        }
    }
    let entry = entries
        .entry(account.clone())
        .or_insert_with(|| metadata.unwrap_or_else(|| serde_json::json!({"displayName": account})));
    let entry = entry
        .as_object_mut()
        .ok_or_else(|| "Saved-account metadata is invalid.".to_owned())?;
    entry.insert("hasSavedLogin".into(), true.into());
    entry.insert("needsReauth".into(), false.into());
    store
        .set("account", entries)
        .map_err(|_| "Could not save account metadata.".to_owned())
}

fn forget_with_vault(
    store: &mut SettingsStore,
    vault: &impl TokenVault,
    account: &str,
) -> Result<(), String> {
    let previous = vault
        .load(account)
        .map_err(|_| "Secure credential storage is unavailable.".to_owned())?
        .map(Zeroizing::new);
    vault.remove(account).map_err(|_| {
        "Could not remove the saved login from secure credential storage.".to_owned()
    })?;
    if let Err(error) = remove_account(store, account) {
        restore_token(
            vault,
            account,
            previous.as_ref().map(|token| token.as_str()),
        )?;
        return Err(error);
    }
    Ok(())
}

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
    let mut unique = std::collections::BTreeMap::<String, SavedAccount>::new();
    for (username, entry) in entries {
        let username = settings::secret::canonical_account(&username);
        let has_saved_login = entry
            .get("hasSavedLogin")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let account = unique
            .entry(username.clone())
            .or_insert_with(|| SavedAccount {
                display_name: entry
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&username)
                    .to_owned(),
                username,
                has_saved_login: false,
            });
        account.has_saved_login |= has_saved_login;
    }
    let mut accounts: Vec<_> = unique.into_values().collect();
    accounts.sort_by_key(|account| {
        order
            .iter()
            .position(|name| settings::secret::canonical_account(name) == account.username)
            .unwrap_or(usize::MAX)
    });
    accounts
}

fn remove_account(store: &mut SettingsStore, username: &str) -> Result<(), String> {
    let mut entries = store
        .get::<Map<String, Value>>("account")
        .unwrap_or_default();
    let username = settings::secret::canonical_account(username);
    entries.retain(|name, _| settings::secret::canonical_account(name) != username);
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
    forget_with_vault(&mut store, &login_vault(), &username)?;
    Ok(list_accounts(&store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryVault {
        tokens: RefCell<HashMap<String, String>>,
        fail_write: Cell<bool>,
    }
    impl TokenVault for MemoryVault {
        fn load(&self, account: &str) -> Result<Option<String>, ()> {
            Ok(self
                .tokens
                .borrow()
                .get(&settings::secret::canonical_account(account))
                .cloned())
        }
        fn save(&self, account: &str, token: &str) -> Result<(), ()> {
            if self.fail_write.get() {
                return Err(());
            }
            self.tokens
                .borrow_mut()
                .insert(settings::secret::canonical_account(account), token.into());
            Ok(())
        }
        fn remove(&self, account: &str) -> Result<(), ()> {
            if self.fail_write.get() {
                return Err(());
            }
            self.tokens
                .borrow_mut()
                .remove(&settings::secret::canonical_account(account));
            Ok(())
        }
    }

    #[test]
    fn remembered_login_publishes_only_metadata_and_removal_cleans_up_the_token() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::open(&path).unwrap();
        let vault = MemoryVault::default();
        remember_with_vault(&mut store, &vault, "alice", "synthetic-token", |store| {
            save_account_metadata(store, "alice")
        })
        .unwrap();
        assert!(list_accounts(&store)[0].has_saved_login);
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("synthetic-token")
        );
        assert!(
            !serde_json::to_string(&list_accounts(&store))
                .unwrap()
                .contains("synthetic-token")
        );
        forget_with_vault(&mut store, &vault, "alice").unwrap();
        assert!(list_accounts(&store).is_empty());
        assert_eq!(vault.load("alice").unwrap(), None);
    }

    #[test]
    fn vault_failure_does_not_create_an_account_or_remove_existing_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::open(directory.path().join("settings.json")).unwrap();
        let vault = MemoryVault::default();
        vault.fail_write.set(true);
        assert!(
            remember_with_vault(&mut store, &vault, "alice", "synthetic", |store| {
                save_account_metadata(store, "alice")
            })
            .is_err()
        );
        assert!(list_accounts(&store).is_empty());
        save_account_metadata(&mut store, "alice").unwrap();
        assert!(forget_with_vault(&mut store, &vault, "alice").is_err());
        assert_eq!(list_accounts(&store).len(), 1);
    }

    #[test]
    fn cancelled_or_failed_metadata_commit_restores_the_previous_secure_token() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::open(directory.path().join("settings.json")).unwrap();
        let vault = MemoryVault::default();
        vault.save("alice", "previous-synthetic-token").unwrap();
        assert!(
            remember_with_vault(&mut store, &vault, "alice", "new-synthetic-token", |_| Err(
                "Cancelled".into()
            ))
            .is_err()
        );
        assert_eq!(
            vault.load("alice").unwrap().as_deref(),
            Some("previous-synthetic-token")
        );
        assert!(list_accounts(&store).is_empty());
        assert!(
            remember_with_vault(&mut store, &vault, "bob", "new-synthetic-token", |_| Err(
                "Cancelled".into()
            ))
            .is_err()
        );
        assert_eq!(vault.load("bob").unwrap(), None);
    }

    #[test]
    fn padded_and_mixed_case_aliases_share_one_metadata_and_keyring_identity() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::open(directory.path().join("settings.json")).unwrap();
        store.set("account", serde_json::json!({" Alice ": {"displayName": "Alice"}, "ALICE": {"hasSavedLogin": false}})).unwrap();
        assert_eq!(list_accounts(&store).len(), 1);
        let vault = MemoryVault::default();
        remember_with_vault(&mut store, &vault, "alice", "synthetic-token", |store| {
            save_account_metadata(store, " alice ")
        })
        .unwrap();
        assert_eq!(store.get::<Map<String, Value>>("account").unwrap().len(), 1);
        assert_eq!(
            vault.load(" ALICE ").unwrap().as_deref(),
            Some("synthetic-token")
        );
        forget_with_vault(&mut store, &vault, " ALICE ").unwrap();
        assert!(list_accounts(&store).is_empty());
        assert_eq!(vault.load("alice").unwrap(), None);
    }

    #[test]
    fn failed_metadata_deletion_restores_the_token_and_in_memory_account() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::open(&path).unwrap();
        save_account_metadata(&mut store, "alice").unwrap();
        let vault = MemoryVault::default();
        vault.save("alice", "synthetic-token").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(forget_with_vault(&mut store, &vault, "alice").is_err());
        assert!(list_accounts(&store)[0].has_saved_login);
        assert_eq!(
            vault.load("alice").unwrap().as_deref(),
            Some("synthetic-token")
        );
    }

    struct BlockingVault {
        tokens: Mutex<HashMap<String, String>>,
        replaced: std::sync::Barrier,
        release: std::sync::Barrier,
    }
    impl TokenVault for BlockingVault {
        fn load(&self, account: &str) -> Result<Option<String>, ()> {
            Ok(self.tokens.lock().unwrap().get(account).cloned())
        }
        fn save(&self, account: &str, token: &str) -> Result<(), ()> {
            self.tokens
                .lock()
                .unwrap()
                .insert(account.into(), token.into());
            if token == "stale-synthetic" {
                self.replaced.wait();
                self.release.wait();
            }
            Ok(())
        }
        fn remove(&self, account: &str) -> Result<(), ()> {
            self.tokens.lock().unwrap().remove(account);
            Ok(())
        }
    }
    struct ImmediateSteam;
    impl steam_core::SteamTransport for ImmediateSteam {
        type Connection = ();
        async fn login(
            &self,
            credentials: steam_core::Credentials,
        ) -> Result<((), steam_core::Profile), steam_core::LoginError> {
            Ok((
                (),
                steam_core::Profile {
                    account: credentials.account.clone(),
                    steam_id: "76561198000000000".into(),
                    generation: 0,
                },
            ))
        }
    }

    #[tokio::test]
    async fn cancelled_blocking_write_rolls_back_before_new_remember_or_delete() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        for delete in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let store = Arc::new(Mutex::new(
                SettingsStore::open(directory.path().join("settings.json")).unwrap(),
            ));
            save_account_metadata(&mut store.lock().unwrap(), "alice").unwrap();
            let vault = Arc::new(BlockingVault {
                tokens: Mutex::new(HashMap::new()),
                replaced: std::sync::Barrier::new(2),
                release: std::sync::Barrier::new(2),
            });
            vault.save("alice", "previous-synthetic").unwrap();
            let session = Arc::new(steam_core::SessionManager::new(
                ImmediateSteam,
                std::time::Duration::from_secs(30),
            ));
            let credentials = || steam_core::Credentials {
                account: "alice".into(),
                password: "synthetic-password".into(),
                guard_code: None,
                refresh_token: None,
                shared_secret: None,
            };
            let generation = session.login(credentials()).await.unwrap().generation;
            let stale_metadata = Arc::new(AtomicBool::new(false));
            let old = {
                let (store, vault, session, mutated) = (
                    store.clone(),
                    vault.clone(),
                    session.clone(),
                    stale_metadata.clone(),
                );
                std::thread::spawn(move || {
                    remember_with_vault(
                        &mut store.lock().unwrap(),
                        &*vault,
                        "alice",
                        "stale-synthetic",
                        |store| {
                            session
                                .in_generation(generation, || {
                                    mutated.store(true, Ordering::SeqCst);
                                    save_account_metadata(store, "alice")
                                })
                                .map_err(|error| error.to_string())?
                        },
                    )
                })
            };
            vault.replaced.wait();
            session.sign_out().unwrap();
            let newer = session.login(credentials()).await.unwrap().generation;
            assert!(store.try_lock().is_err());
            let next = {
                let (store, vault, session) = (store.clone(), vault.clone(), session.clone());
                std::thread::spawn(move || {
                    let mut store = store.lock().unwrap();
                    if delete {
                        forget_with_vault(&mut store, &*vault, "alice")
                    } else {
                        remember_with_vault(
                            &mut store,
                            &*vault,
                            "alice",
                            "newest-synthetic",
                            |store| {
                                session
                                    .in_generation(newer, || save_account_metadata(store, "alice"))
                                    .map_err(|error| error.to_string())?
                            },
                        )
                    }
                })
            };
            vault.release.wait();
            assert!(old.join().unwrap().is_err());
            next.join().unwrap().unwrap();
            assert!(!stale_metadata.load(Ordering::SeqCst));
            if delete {
                assert_eq!(vault.load("alice").unwrap(), None);
                assert!(list_accounts(&store.lock().unwrap()).is_empty());
            } else {
                assert_eq!(
                    vault.load("alice").unwrap().as_deref(),
                    Some("newest-synthetic")
                );
                assert!(list_accounts(&store.lock().unwrap())[0].has_saved_login);
            }
        }
    }

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
