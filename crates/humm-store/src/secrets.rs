//! [`KeyringSecretStore`]: the [`SecretStore`] implementation backed by the
//! OS credential store (macOS Keychain, Windows Credential Manager, *nix
//! Secret Service via the pure-Rust `zbus` D-Bus client), via the `keyring`
//! crate (ADR-012). humm's own service string ([`SERVICE`]) keeps every
//! entry under `humm`'s own namespace, distinct from Buzz's `"Buzz"` service
//! — a fresh humm setup never reads a key Buzz already stored.

use humm_core::secrets::{SERVICE, SecretError, SecretKey, SecretStore};

/// An OS-keyring-backed [`SecretStore`].
///
/// Each call opens a fresh `keyring::Entry` scoped to [`SERVICE`] and the
/// key's [`SecretKey::entry_name`] — the `keyring` crate's `Entry` is a cheap
/// handle (no connection is held between calls), so there is no state to
/// hold here.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    /// Creates a new keyring-backed secret store.
    pub fn new() -> Self {
        KeyringSecretStore
    }

    /// Opens the platform keyring entry for `key`, mapping any failure to
    /// [`SecretError::Backend`].
    fn entry(&self, key: SecretKey) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE, key.entry_name()).map_err(|err| SecretError::Backend {
            detail: format!("failed to open keyring entry: {err}"),
        })
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(SecretError::Backend {
                detail: format!("failed to read {} from keyring: {err}", key.entry_name()),
            }),
        }
    }

    fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError> {
        self.entry(key)?
            .set_password(value)
            .map_err(|err| SecretError::Backend {
                detail: format!("failed to save {} to keyring: {err}", key.entry_name()),
            })
    }

    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(SecretError::Backend {
                detail: format!("failed to delete {} from keyring: {err}", key.entry_name()),
            }),
        }
    }
}
