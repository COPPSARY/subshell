use crate::contracts::CommandError;

const SERVICE: &str = "com.subshell.desktop";

pub trait SecretStore: Send + Sync {
    fn get(&self, account_id: &str) -> Result<Option<Vec<u8>>, CommandError>;
    fn set(&self, account_id: &str, secret: &[u8]) -> Result<(), CommandError>;
    fn delete(&self, account_id: &str) -> Result<(), CommandError>;
}

#[derive(Clone, Default)]
pub struct SystemSecretStore;

impl SecretStore for SystemSecretStore {
    fn get(&self, account_id: &str) -> Result<Option<Vec<u8>>, CommandError> {
        match entry(account_id)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    fn set(&self, account_id: &str, secret: &[u8]) -> Result<(), CommandError> {
        entry(account_id)?.set_secret(secret).map_err(keyring_error)
    }

    fn delete(&self, account_id: &str) -> Result<(), CommandError> {
        match entry(account_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

fn entry(account_id: &str) -> Result<keyring::Entry, CommandError> {
    if account_id.trim().is_empty() {
        return Err(CommandError::new(
            "invalid_account",
            "Account id is required",
        ));
    }
    keyring::Entry::new(SERVICE, account_id).map_err(keyring_error)
}

fn keyring_error(error: keyring::Error) -> CommandError {
    CommandError::new(
        "keychain_unavailable",
        format!("OS keychain operation failed: {error}"),
    )
}

#[derive(Clone, Default)]
#[cfg(test)]
pub struct MemorySecretStore {
    values: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>>,
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn get(&self, account_id: &str) -> Result<Option<Vec<u8>>, CommandError> {
        Ok(self
            .values
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .get(account_id)
            .cloned())
    }

    fn set(&self, account_id: &str, secret: &[u8]) -> Result<(), CommandError> {
        self.values
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .insert(account_id.into(), secret.into());
        Ok(())
    }

    fn delete(&self, account_id: &str) -> Result<(), CommandError> {
        self.values
            .lock()
            .unwrap_or_else(|lock| lock.into_inner())
            .remove(account_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_store_contract_round_trips_and_deletes_opaque_bytes() {
        let store = MemorySecretStore::default();
        assert_eq!(store.get("account").unwrap(), None);
        store.set("account", b"secret-marker").unwrap();
        assert_eq!(
            store.get("account").unwrap(),
            Some(b"secret-marker".to_vec())
        );
        store.delete("account").unwrap();
        assert_eq!(store.get("account").unwrap(), None);
    }
}
