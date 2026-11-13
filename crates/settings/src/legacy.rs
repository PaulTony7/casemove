use aes::cipher::{block_padding::Pkcs7, BlockModeDecrypt, KeyIvInit};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

/// The `encryptionKey` legacy passes to electron-store (conf). It only
/// obfuscates; the value is baked into the legacy app source.
pub const LEGACY_ENCRYPTION_KEY: &str = "this_only_obfuscates";

const CONF_SEPARATOR: u8 = b':';
const CONF_ROUNDS: u32 = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a conf-encrypted store (byte 16 is not ':')")]
    NotEncrypted,
    #[error("decryption failed")]
    Decrypt,
    #[error("legacy store root is not a JSON object")]
    BadFormat,
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// Decrypt a `conf` v10 encrypted store file:
/// `IV(16) + ':' + AES-256-CBC(PKCS7)` with key
/// `PBKDF2-HMAC-SHA512(encryption_key, IV-as-utf8-string, 10000, 32)`.
///
/// The lossy UTF-8 round-trip on the IV replicates Node's
/// `Buffer.toString()` → `pbkdf2Sync(salt)` behavior exactly.
pub fn decrypt_conf_store(data: &[u8], encryption_key: &str) -> Result<Value, Error> {
    if data.len() < 17 || data[16] != CONF_SEPARATOR {
        return Err(Error::NotEncrypted);
    }
    let iv = &data[..16];
    let salt = String::from_utf8_lossy(iv);

    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha512>(
        encryption_key.as_bytes(),
        salt.as_bytes(),
        CONF_ROUNDS,
        &mut key,
    );

    let decryptor = Aes256CbcDec::new_from_slices(&key, iv).map_err(|_| Error::Decrypt)?;
    let plaintext = decryptor
        .decrypt_padded_vec::<Pkcs7>(&data[17..])
        .map_err(|_| Error::Decrypt)?;
    Ok(serde_json::from_slice(&plaintext)?)
}

/// A saved account migrated from the legacy store. `refreshToken`/`safeData`
/// blobs are Electron `safeStorage`-encrypted and cannot be read outside
/// Electron, so those accounts are flagged for one full re-login.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyAccount {
    pub username: String,
    pub display_name: Option<String>,
    pub image_url: Option<String>,
    pub position: Option<u64>,
    pub needs_reauth: bool,
}

#[derive(Debug)]
pub struct LegacyMigration {
    /// Every legacy top-level key except `account`.
    pub prefs: Map<String, Value>,
    pub accounts: Vec<LegacyAccount>,
}

pub fn migrate_legacy_store(path: &Path) -> Result<LegacyMigration, Error> {
    migrate_legacy_bytes(&fs::read(path)?, LEGACY_ENCRYPTION_KEY)
}

pub fn migrate_legacy_bytes(data: &[u8], encryption_key: &str) -> Result<LegacyMigration, Error> {
    let store = decrypt_conf_store(data, encryption_key)?;
    let mut prefs = store
        .as_object()
        .ok_or(Error::BadFormat)?
        .clone();

    let accounts = match prefs.remove("account") {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(username, entry)| LegacyAccount {
                username: username.clone(),
                display_name: entry
                    .get("displayName")
                    .and_then(Value::as_str)
                    .map(String::from),
                image_url: entry.get("imageURL").and_then(Value::as_str).map(String::from),
                position: entry.get("position").and_then(Value::as_u64),
                needs_reauth: entry.get("refreshToken").is_some()
                    || entry.get("safeData").is_some(),
            })
            .collect(),
        _ => Vec::new(),
    };

    Ok(LegacyMigration { prefs, accounts })
}

impl LegacyMigration {
    /// One-time import into the new settings store. Secrets are not migrated;
    /// flagged accounts must log in once so the new app can store fresh tokens
    /// in the OS keyring.
    pub fn apply(self, store: &mut crate::store::SettingsStore) -> Result<(), crate::store::Error> {
        for (key, value) in self.prefs {
            store.set(&key, value)?;
        }
        if !self.accounts.is_empty() {
            let mut entries = Map::new();
            for account in self.accounts {
                let mut entry = Map::new();
                if let Some(name) = account.display_name {
                    entry.insert("displayName".into(), name.into());
                }
                if let Some(url) = account.image_url {
                    entry.insert("imageURL".into(), url.into());
                }
                if let Some(position) = account.position {
                    entry.insert("position".into(), position.into());
                }
                if account.needs_reauth {
                    entry.insert("needsReauth".into(), true.into());
                }
                entries.insert(account.username, Value::Object(entry));
            }
            store.set("account", Value::Object(entries))?;
        }
        Ok(())
    }
}

/// Locations the legacy store may live in: packaged app uses
/// `~/.config/Casemove`, dev runs used `~/.config/Electron`.
pub fn legacy_store_candidates() -> Vec<std::path::PathBuf> {
    let Some(config) = dirs_config() else {
        return Vec::new();
    };
    vec![
        config.join("Casemove").join("casemoveEnc.json"),
        config.join("Electron").join("casemoveEnc.json"),
    ]
}

#[cfg(target_os = "windows")]
fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(std::path::PathBuf::from)
}

#[cfg(target_os = "macos")]
fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(|home| std::path::PathBuf::from(home).join("Library/Application Support"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
}
