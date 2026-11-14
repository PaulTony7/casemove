use app_commands::SavedAccount;
use serde_json::{Map, Value};
use settings::SettingsStore;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;
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

#[cfg(test)]
pub(crate) fn save_account_metadata(
    store: &mut SettingsStore,
    account: &str,
) -> Result<(), String> {
    save_account_identity(store, account, None, None)
}

pub(crate) fn save_account_identity(
    store: &mut SettingsStore,
    account: &str,
    display_name: Option<&str>,
    avatar_url: Option<&str>,
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
    if let Some(name) = display_name.map(str::trim).filter(|name| !name.is_empty()) {
        entry.insert("displayName".into(), name.to_owned().into());
    }
    if let Some(url) = avatar_url.filter(|url| is_steam_avatar_url(url)) {
        entry.insert("imageURL".into(), url.to_owned().into());
    }
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
    // A keyring read failure must not make an account undeletable: treat a
    // failed pre-read as "no previous token".
    let previous = vault.load(account).ok().flatten().map(Zeroizing::new);
    // Token cleanup is best-effort: a locked/unavailable keyring still allows
    // removing the account (any leftover token is inaccessible anyway).
    if vault.remove(account).is_err() {
        eprintln!(
            "Warning: the saved login token for {account} could not be removed from secure storage."
        );
    }
    if let Err(error) = remove_account(store, account) {
        if let Some(token) = previous {
            let _ = restore_token(vault, account, Some(token.as_str()));
        }
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
                image_url: entry
                    .get("imageURL")
                    .and_then(Value::as_str)
                    .filter(|url| is_steam_avatar_url(url))
                    .map(str::to_owned),
                username,
                has_saved_login: false,
            });
        account.has_saved_login |= has_saved_login;
        if account.image_url.is_none() {
            account.image_url = entry
                .get("imageURL")
                .and_then(Value::as_str)
                .filter(|url| is_steam_avatar_url(url))
                .map(str::to_owned);
        }
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

pub(crate) fn is_steam_avatar_url(url: &str) -> bool {
    // Legacy path-scoped CDN.
    if url.starts_with("https://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/") {
        return true;
    }
    // Steam serves avatars from several edge CDNs that all live under
    // steamstatic.com (avatars., avatars.fastly., avatars.akamai., cdn.akamai. …),
    // so allow by host suffix instead of enumerating edges.
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let Some(slash) = rest.find('/') else {
        return false;
    };
    let host = &rest[..slash];
    host == "steamstatic.com" || host.ends_with(".steamstatic.com")
}

fn set_account_position(
    store: &mut SettingsStore,
    username: &str,
    index: usize,
) -> Result<Vec<SavedAccount>, String> {
    let mut order: Vec<_> = list_accounts(store)
        .into_iter()
        .map(|account| account.username)
        .collect();
    let username = settings::secret::canonical_account(username);
    let current_index = order
        .iter()
        .position(|account| account == &username)
        .ok_or_else(|| "The saved account no longer exists.".to_owned())?;
    if index >= order.len() {
        return Err("The requested account position is invalid.".into());
    }
    let account = order.remove(current_index);
    order.insert(index, account);
    store
        .set("accountKeyList", order)
        .map_err(|error| error.to_string())?;
    Ok(list_accounts(store))
}

fn remove_account(store: &mut SettingsStore, username: &str) -> Result<(), String> {
    let username = settings::secret::canonical_account(username);
    let mut entries = store
        .get::<Map<String, Value>>("account")
        .unwrap_or_default();
    entries.retain(|name, _| settings::secret::canonical_account(name) != username);
    let order: Vec<String> = store
        .get::<Vec<String>>("accountKeyList")
        .unwrap_or_default()
        .into_iter()
        .filter(|name| settings::secret::canonical_account(name) != username)
        .collect();
    store
        .set("accountKeyList", order)
        .map_err(|error| error.to_string())?;
    store
        .set("account", entries)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn store_get_accounts(
    store: tauri::State<'_, Mutex<SettingsStore>>,
) -> Result<Vec<SavedAccount>, String> {
    let store = store
        .lock()
        .map_err(|_| "Saved-account storage is unavailable.".to_owned())?;
    Ok(list_accounts(&store))
}

#[tauri::command]
pub async fn store_delete_account(
    username: String,
    app: tauri::AppHandle,
) -> Result<Vec<SavedAccount>, String> {
    // Keyring (zbus) calls can block for seconds — keep them off the main thread.
    tauri::async_runtime::spawn_blocking(move || {
        let settings = app.state::<Mutex<SettingsStore>>();
        let mut store = settings
            .lock()
            .map_err(|_| "Saved-account storage is unavailable.".to_owned())?;
        forget_with_vault(&mut store, &login_vault(), &username)?;
        Ok(list_accounts(&store))
    })
    .await
    .map_err(|_| "The account operation failed.".to_owned())?
}

#[tauri::command]
pub fn store_set_account_position(
    username: String,
    index: usize,
    store: tauri::State<'_, Mutex<SettingsStore>>,
) -> Result<Vec<SavedAccount>, String> {
    let mut store = store
        .lock()
        .map_err(|_| "Saved-account storage is unavailable.".to_owned())?;
    set_account_position(&mut store, &username, index)
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
    fn vault_failure_blocks_remember_but_never_blocks_deletion() {
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
        // A broken keyring must not make an account undeletable: metadata
        // removal is authoritative, token cleanup is best-effort.
        assert!(forget_with_vault(&mut store, &vault, "alice").is_ok());
        assert!(list_accounts(&store).is_empty());
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

    #[test]
    fn account_order_is_persisted_and_avatars_are_limited_to_steam_hosts() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::open(directory.path().join("settings.json")).unwrap();
        store
            .set(
                "account",
                serde_json::json!({
                    "alice": {"displayName": "Alice", "imageURL": "https://avatars.steamstatic.com/alice_medium.jpg"},
                    "bob": {"displayName": "Bob", "imageURL": "https://example.org/avatar.png"}
                }),
            )
            .unwrap();

        assert_eq!(
            list_accounts(&store)[0].image_url.as_deref(),
            Some("https://avatars.steamstatic.com/alice_medium.jpg")
        );
        assert_eq!(list_accounts(&store)[1].image_url, None);

        let accounts = set_account_position(&mut store, " ALICE ", 1).unwrap();
        assert_eq!(
            accounts
                .iter()
                .map(|account| account.username.as_str())
                .collect::<Vec<_>>(),
            ["bob", "alice"]
        );
        let reopened = SettingsStore::open(directory.path().join("settings.json")).unwrap();
        assert_eq!(
            reopened.get::<Vec<String>>("accountKeyList").unwrap(),
            ["bob", "alice"]
        );
    }

    #[test]
    fn account_order_rejects_missing_names_and_out_of_range_positions() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::open(directory.path().join("settings.json")).unwrap();
        store
            .set(
                "account",
                serde_json::json!({"alice": {"displayName": "Alice"}}),
            )
            .unwrap();

        assert!(set_account_position(&mut store, "missing", 0).is_err());
        assert!(set_account_position(&mut store, "alice", 1).is_err());
        assert_eq!(store.get::<Vec<String>>("accountKeyList"), None);
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
                web_logon_token: None,
                steam_id: None,
                qr_challenge_tx: None,
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
        let order: Vec<String> = reopened.get("accountKeyList").unwrap();
        assert_eq!(order, vec!["alice".to_string()]);
    }

    #[test]
    fn saved_identity_stores_validated_avatar_and_display_name() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::open(directory.path().join("settings.json")).unwrap();

        save_account_identity(
            &mut store,
            "alice",
            Some("  Alice Synth  "),
            Some("https://avatars.fastly.steamstatic.com/a_medium.jpg"),
        )
        .unwrap();
        // The fastly avatar host is the current Steam CDN (legacy allowlist
        // predates it); non-Steam hosts are rejected.
        save_account_identity(&mut store, "bob", None, Some("https://evil.example/a.jpg")).unwrap();

        let accounts = list_accounts(&store);
        let alice = accounts
            .iter()
            .find(|account| account.username == "alice")
            .unwrap();
        assert_eq!(alice.display_name, "Alice Synth");
        assert_eq!(
            alice.image_url.as_deref(),
            Some("https://avatars.fastly.steamstatic.com/a_medium.jpg")
        );
        let bob = accounts
            .iter()
            .find(|account| account.username == "bob")
            .unwrap();
        assert_eq!(bob.image_url, None);

        // A later save without identity data must not clobber what we have.
        save_account_identity(&mut store, "alice", None, None).unwrap();
        let accounts = list_accounts(&store);
        let alice = accounts
            .iter()
            .find(|account| account.username == "alice")
            .unwrap();
        assert_eq!(alice.display_name, "Alice Synth");
        assert!(alice.has_saved_login);
    }

    #[test]
    fn avatar_allowlist_covers_every_steam_edge_but_no_other_hosts() {
        for url in [
            "https://avatars.steamstatic.com/x_medium.jpg",
            "https://avatars.fastly.steamstatic.com/x_medium.jpg",
            "https://avatars.akamai.steamstatic.com/x_full.jpg",
            "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/avatars/x.jpg",
            "https://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/x.jpg",
            "https://steamstatic.com/x.jpg",
        ] {
            assert!(is_steam_avatar_url(url), "expected allow: {url}");
        }
        for url in [
            "https://avatars.akamai.steamstatic.com.evil.example/x.jpg",
            "https://notsteamstatic.com/x.jpg",
            "https://evil.example/steamstatic.com/x.jpg",
            "http://avatars.steamstatic.com/x.jpg",
            "https://evil.example/x.jpg",
        ] {
            assert!(!is_steam_avatar_url(url), "expected reject: {url}");
        }
    }
}
