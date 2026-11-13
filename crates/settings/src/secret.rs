use keyring::Entry;

pub fn canonical_account(account: &str) -> String {
    account.trim().to_ascii_lowercase()
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("keyring: {0}")]
    Keyring(#[from] keyring::Error),
}

/// Secrets (refresh tokens, shared secrets) in the OS keyring instead of the
/// legacy `safeStorage`-encrypted JSON blobs.
pub struct SecretStore {
    service: String,
}

impl SecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<Entry, Error> {
        Ok(Entry::new(&self.service, &canonical_account(account))?)
    }

    pub fn set(&self, account: &str, secret: &str) -> Result<(), Error> {
        self.entry(account)?.set_password(secret)?;
        Ok(())
    }

    pub fn get(&self, account: &str) -> Result<String, Error> {
        Ok(self.entry(account)?.get_password()?)
    }

    pub fn get_optional(&self, account: &str) -> Result<Option<String>, Error> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn delete(&self, account: &str) -> Result<(), Error> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}
