use settings::{
    decrypt_conf_store, migrate_legacy_bytes, migrate_legacy_store, SettingsStore,
    LEGACY_ENCRYPTION_KEY,
};
use serde_json::Value;
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conf-encrypted.bin")
}

#[test]
fn decrypts_conf_format() {
    let data = std::fs::read(fixture()).unwrap();
    let value = decrypt_conf_store(&data, LEGACY_ENCRYPTION_KEY).unwrap();
    let obj = value.as_object().unwrap();
    assert_eq!(obj["fastmove"], false);
    assert_eq!(obj["os"], "linux");
    assert_eq!(obj["pricing"]["currency"], "USD");
    assert!(obj.get("account").is_some());
}

#[test]
fn rejects_unencrypted_input() {
    let err = decrypt_conf_store(b"{\"fastmove\":false}", LEGACY_ENCRYPTION_KEY).unwrap_err();
    assert!(matches!(err, settings::legacy::Error::NotEncrypted));
}

#[test]
fn migrates_prefs_and_account_metadata() {
    let data = std::fs::read(fixture()).unwrap();
    let migration = migrate_legacy_bytes(&data, LEGACY_ENCRYPTION_KEY).unwrap();

    // prefs: everything except `account`
    assert!(migration.prefs.contains_key("fastmove"));
    assert!(migration.prefs.contains_key("pricing"));
    assert!(!migration.prefs.contains_key("account"));

    // accounts: metadata kept, secret blobs flagged for re-login
    assert_eq!(migration.accounts.len(), 3);
    let alice = migration
        .accounts
        .iter()
        .find(|a| a.username == "alice")
        .unwrap();
    assert_eq!(alice.display_name.as_deref(), Some("Alice"));
    assert_eq!(alice.position, Some(0));
    assert!(alice.needs_reauth);

    let bob = migration.accounts.iter().find(|a| a.username == "bob").unwrap();
    assert!(bob.needs_reauth);

    let carol = migration
        .accounts
        .iter()
        .find(|a| a.username == "carol")
        .unwrap();
    assert!(!carol.needs_reauth, "no secrets -> no re-auth needed");
}

#[test]
fn apply_writes_new_store() {
    let data = std::fs::read(fixture()).unwrap();
    let migration = migrate_legacy_bytes(&data, LEGACY_ENCRYPTION_KEY).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut store = SettingsStore::open(&path).unwrap();
    migration.apply(&mut store).unwrap();

    let reopened = SettingsStore::open(&path).unwrap();
    assert_eq!(reopened.get::<bool>("fastmove"), Some(false));
    assert_eq!(
        reopened.get::<String>("pricing.currency"),
        Some("USD".into())
    );
    let accounts: serde_json::Value = reopened.get("account").unwrap();
    assert_eq!(accounts["alice"]["displayName"], "Alice");
    assert_eq!(accounts["alice"]["needsReauth"], true);
    assert_eq!(accounts["carol"]["needsReauth"], Value::Null);
}

#[test]
fn migrates_from_path() {
    let migration = migrate_legacy_store(&fixture()).unwrap();
    assert_eq!(migration.accounts.len(), 3);
}
