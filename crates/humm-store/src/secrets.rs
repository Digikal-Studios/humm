//! [`KeyringSecretStore`]: the [`SecretStore`] implementation backed by the
//! OS credential store (macOS Keychain, Windows Credential Manager, *nix
//! Secret Service via the pure-Rust `zbus` D-Bus client), via the `keyring`
//! crate (ADR-012). humm's own service string ([`SERVICE`]) keeps every
//! entry under `humm`'s own namespace, distinct from Buzz's `"Buzz"` service
//! — a fresh humm setup never reads a key Buzz already stored.

use humm_core::secrets::{SERVICE, SecretError, SecretKey, SecretStore};

/// An OS-keyring-backed [`SecretStore`].
///
/// Each call opens a fresh `keyring::Entry` scoped to `service` and the
/// key's [`SecretKey::entry_name`] — the `keyring` crate's `Entry` is a cheap
/// handle (no connection is held between calls), so there is no other state
/// to hold here.
#[derive(Debug, Clone, Copy)]
pub struct KeyringSecretStore {
    service: &'static str,
}

impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyringSecretStore {
    /// Creates a new keyring-backed secret store scoped to humm's production
    /// service name ([`SERVICE`], `"humm"`) — the service the shipped app
    /// reads and writes.
    pub fn new() -> Self {
        KeyringSecretStore { service: SERVICE }
    }

    /// Creates a keyring-backed secret store scoped to an arbitrary
    /// `service`, instead of the production [`SERVICE`].
    ///
    /// For contract tests only: it lets a test round-trip through the real
    /// OS keyring without ever touching the production `"humm"` entry the
    /// shipped app reads (a machine with a real key stored would otherwise
    /// have it overwritten then deleted by the test). [`KeyringSecretStore::new`]
    /// is the constructor production code should use — it always keeps
    /// service `"humm"`.
    pub fn with_service(service: &'static str) -> Self {
        KeyringSecretStore { service }
    }

    /// Opens the platform keyring entry for `key`, mapping any failure to
    /// [`SecretError::Backend`].
    fn entry(&self, key: SecretKey) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(self.service, key.entry_name()).map_err(|err| SecretError::Backend {
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
