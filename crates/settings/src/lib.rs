//! Persistent settings: JSON store, OS-keyring secrets, legacy migration.

pub mod legacy;
pub mod secret;
pub mod store;

pub use legacy::{
    decrypt_conf_store, migrate_legacy_bytes, migrate_legacy_store, LegacyAccount, LegacyMigration,
    LEGACY_ENCRYPTION_KEY,
};
pub use secret::SecretStore;
pub use store::SettingsStore;
