// SPDX-License-Identifier: GPL-3.0-or-later

//! Where API keys are kept: the OS keychain, or memory for tests and a session that keeps
//! none.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::Secret;

/// The keychain service every QuakeC Studio key is under.
const SERVICE: &str = "quakec-studio";

/// Why a key could not be read, written or deleted. It carries the store's own message and
/// never the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// The OS keychain cannot be reached (none on this system, or it is locked).
    Unavailable(String),
    /// The keychain refused or failed.
    Store(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "the keychain is not available: {why}"),
            Self::Store(why) => write!(f, "the keychain failed: {why}"),
        }
    }
}

impl std::error::Error for KeyError {}

/// A place keys are kept, by name ([`crate::ProviderConfig::key_name`]).
pub trait KeyStore {
    /// The key under `name`, if one is kept.
    ///
    /// # Errors
    ///
    /// [`KeyError`] when the store cannot be read.
    fn get(&self, name: &str) -> Result<Option<Secret>, KeyError>;

    /// Keeps `key` under `name`, replacing any there.
    ///
    /// # Errors
    ///
    /// [`KeyError`] when the store cannot be written.
    fn set(&self, name: &str, key: &Secret) -> Result<(), KeyError>;

    /// Forgets the key under `name`; forgetting none is no error.
    ///
    /// # Errors
    ///
    /// [`KeyError`] when the store cannot be written.
    fn delete(&self, name: &str) -> Result<(), KeyError>;
}

/// The OS keychain: macOS Keychain Services, the Windows Credential Manager, or the Secret
/// Service on Linux and other Unix systems, through `keyring`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Keychain;

/// The entry for `name`.
fn entry(name: &str) -> Result<keyring::Entry, KeyError> {
    keyring::Entry::new(SERVICE, name).map_err(store_error)
}

/// A keyring error as text with no key in it: only the error's `Display`, which for every
/// variant is fixed text or the platform's message, never the bytes `BadEncoding` holds.
fn store_error(error: keyring::Error) -> KeyError {
    match error {
        keyring::Error::NoDefaultStore | keyring::Error::NoStorageAccess(_) => {
            KeyError::Unavailable(error.to_string())
        }
        other => KeyError::Store(other.to_string()),
    }
}

impl KeyStore for Keychain {
    fn get(&self, name: &str) -> Result<Option<Secret>, KeyError> {
        match entry(name)?.get_password() {
            Ok(key) => Ok(Some(Secret::new(key))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(error)),
        }
    }

    fn set(&self, name: &str, key: &Secret) -> Result<(), KeyError> {
        entry(name)?.set_password(key.expose()).map_err(store_error)
    }

    fn delete(&self, name: &str) -> Result<(), KeyError> {
        match entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(store_error(error)),
        }
    }
}

/// Keys kept in memory only, gone when it is dropped.
#[derive(Debug, Default)]
pub struct MemoryKeys {
    keys: Mutex<BTreeMap<String, Secret>>,
}

impl MemoryKeys {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The map, recovered when a thread panicked holding it: it holds only whole values.
    fn keys(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Secret>> {
        self.keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl KeyStore for MemoryKeys {
    fn get(&self, name: &str) -> Result<Option<Secret>, KeyError> {
        Ok(self.keys().get(name).cloned())
    }

    fn set(&self, name: &str, key: &Secret) -> Result<(), KeyError> {
        self.keys().insert(name.to_owned(), key.clone());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), KeyError> {
        self.keys().remove(name);
        Ok(())
    }
}
