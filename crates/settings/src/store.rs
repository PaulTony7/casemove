use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// JSON settings file with dot-path access, mirroring the legacy
/// electron-store API (`getValue('pricing.currency')` etc.).
pub struct SettingsStore {
    path: PathBuf,
    data: Value,
}

impl SettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        let data = match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
            Err(err) => return Err(err.into()),
        };
        Ok(Self { path, data })
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut current = &self.data;
        for part in key.split('.') {
            current = current.get(part)?;
        }
        serde_json::from_value(current.clone()).ok()
    }

    pub fn set<T: Serialize>(&mut self, key: &str, value: T) -> Result<(), Error> {
        let value = serde_json::to_value(value)?;
        let previous = self.data.clone();
        let parts: Vec<&str> = key.split('.').collect();
        let (last, parents) = parts.split_last().expect("empty settings key");

        let mut current = &mut self.data;
        for part in parents {
            if !current.is_object() {
                *current = Value::Object(Map::new());
            }
            current = current
                .as_object_mut()
                .expect("just checked")
                .entry(*part)
                .or_insert_with(|| Value::Object(Map::new()));
        }
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
        current
            .as_object_mut()
            .expect("just checked")
            .insert((*last).to_string(), value);
        if let Err(error) = self.save() {
            self.data = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    fn save(&self) -> Result<(), Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&self.data)?)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_dot_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let mut store = SettingsStore::open(&path).unwrap();
        assert_eq!(store.get::<bool>("fastmove"), None);
        store.set("fastmove", false).unwrap();
        store.set("pricing.currency", "EUR").unwrap();
        store.set("deeply.nested.key", 42u32).unwrap();

        let reopened = SettingsStore::open(&path).unwrap();
        assert_eq!(reopened.get::<bool>("fastmove"), Some(false));
        assert_eq!(
            reopened.get::<String>("pricing.currency"),
            Some("EUR".into())
        );
        assert_eq!(reopened.get::<u32>("deeply.nested.key"), Some(42));
        assert_eq!(reopened.get::<String>("pricing.missing"), None);
    }

    #[test]
    fn missing_file_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::open(dir.path().join("nope.json")).unwrap();
        assert!(store.data().as_object().unwrap().is_empty());
    }

    #[test]
    fn failed_write_does_not_publish_unsaved_settings_in_memory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut store = SettingsStore::open(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(store.set("account.alice.hasSavedLogin", true).is_err());
        assert_eq!(store.get::<bool>("account.alice.hasSavedLogin"), None);
    }
}
