//! Secrets: the [`SecretStore`] boundary trait (ADR-012) plus an in-memory
//! test double.
//!
//! Buzz stores its OpenAI API key via `keyring_store.py`, addressed by the
//! `Key` enum's stable entry names (`Key.OPENAI_API_KEY = "OpenAI API key"`,
//! `keyring_store.py:14-16`). humm ports the entry-name semantics but not the
//! storage mechanism: [`SecretStore`] is a core-defined trait so the real
//! implementation (`humm_store::KeyringSecretStore`, backed by the `keyring`
//! crate) lives behind the adapter boundary like every other non-Rust
//! component (`docs/design/ARCHITECTURE.md` §2). humm's keyring **service**
//! string is [`SERVICE`] (`"humm"`), deliberately not Buzz's `"Buzz"` — a
//! fresh humm setup never reads a key Buzz already stored (design spec §5).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// A well-known secret this app stores in the OS keyring, addressed by a
/// stable entry name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKey {
    /// The user's OpenAI API key (F-041 the OpenAI-compatible transcription
    /// backend, F-031 LLM translation).
    OpenAiApiKey,
}

impl SecretKey {
    /// The keyring entry name for this key — Buzz parity
    /// (`Key.OPENAI_API_KEY = "OpenAI API key"`, `keyring_store.py:14-16`).
    /// humm's *service* string ([`SERVICE`]) is its own, so this name-level
    /// parity alone does not let humm read a key Buzz already stored.
    pub fn entry_name(self) -> &'static str {
        match self {
            SecretKey::OpenAiApiKey => "OpenAI API key",
        }
    }
}

/// The keyring **service** name humm's [`SecretStore`] entries are stored
/// under. Deliberately `"humm"`, not Buzz's `"Buzz"` (`keyring_store.py`'s
/// `APP_NAME`) — humm never reads Buzz's stored key; a fresh setup after
/// porting is the documented, deliberate behavior (design spec §5).
pub const SERVICE: &str = "humm";

/// Errors from a [`SecretStore`] implementation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SecretError {
    /// The OS keyring/credential-store backend failed to read, write, or
    /// delete a secret — e.g. the platform store is locked, unavailable, or
    /// access was denied. Distinct from a simply-absent entry, which
    /// [`SecretStore::get`] reports as `Ok(None)`, not an error.
    #[error("keyring backend error: {detail}")]
    Backend {
        /// Diagnostic detail from the underlying backend.
        detail: String,
    },
}

/// Reads, writes, and deletes named secrets in a platform credential store.
///
/// Implemented by `humm_store::KeyringSecretStore` (the real OS-keyring
/// adapter, ADR-012) and [`MemorySecretStore`] (a test double). Never
/// implemented against a plaintext file or the settings TOML — that is the
/// entire point of the boundary (design spec §5's acceptance invariant: keys
/// live in the OS keyring, process memory, and `Authorization` headers only,
/// never in `settings.toml`, the DB, or logs).
pub trait SecretStore: Send + Sync {
    /// Reads `key`'s secret. `Ok(None)` means no entry is stored (never set,
    /// or previously deleted) — not an error. An empty stored string is
    /// returned as `Ok(Some(String::new()))`; callers that care treat empty
    /// as absent.
    fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError>;

    /// Writes `value` as `key`'s secret, overwriting any existing entry.
    fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError>;

    /// Deletes `key`'s secret. Deleting an already-absent entry is not an
    /// error (Buzz parity: `keyring_store.py:238-241`'s
    /// `PasswordDeleteError` swallow) — `Ok(())` either way.
    fn delete(&self, key: SecretKey) -> Result<(), SecretError>;
}

/// An in-memory [`SecretStore`] test double: a `Mutex<HashMap<SecretKey,
/// String>>`.
///
/// Deliberately **not** `#[cfg(test)]`-gated: GUI and CLI test suites in
/// other crates construct this directly so nothing in the workspace's normal
/// test run touches the real OS keychain — only the `#[ignore]`d contract
/// test in `humm-store` does that.
///
/// [`std::fmt::Debug`] is implemented by hand rather than derived: a derived
/// impl would print every stored secret value verbatim if this type were
/// ever formatted (e.g. in a `dbg!` left behind, or a panic message), which
/// would defeat the point of keeping secrets out of logs (design spec §5).
/// The manual impl below prints only the type name and entry count.
#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<SecretKey, String>>,
}

impl std::fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Best-effort: a poisoned mutex still yields a usable entry count via
        // `into_inner`, and this is diagnostic output, not a fallible path.
        let entry_count = match self.values.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        };
        f.debug_struct("MemorySecretStore")
            .field("entry_count", &entry_count)
            .finish()
    }
}

impl MemorySecretStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Locks the value map, mapping mutex poisoning (a prior panic while
    /// held) to [`SecretError::Backend`] rather than panicking.
    fn lock(&self) -> Result<MutexGuard<'_, HashMap<SecretKey, String>>, SecretError> {
        self.values.lock().map_err(|_| SecretError::Backend {
            detail: "in-memory secret store mutex poisoned".to_string(),
        })
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError> {
        Ok(self.lock()?.get(&key).cloned())
    }

    fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError> {
        self.lock()?.insert(key, value.to_string());
        Ok(())
    }

    fn delete(&self, key: SecretKey) -> Result<(), SecretError> {
        self.lock()?.remove(&key);
        Ok(())
    }
}

/// A [`SecretStore`] test double whose [`SecretStore::set`] always fails
/// with [`SecretError::Backend`] — the counterpart to [`MemorySecretStore`]
/// for exercising a commit path's keyring-failure branch (e.g. the options
/// sheet's Api Key row keyring-save-failure modal,
/// `humm-gui/src/views/options_sheet.rs`), which no existing double could
/// trigger.
///
/// Deliberately **not** `#[cfg(test)]`-gated, same rationale as
/// [`MemorySecretStore`]: other crates' test suites (`humm-gui`'s kittest
/// harness) construct this directly, so it must be visible outside this
/// crate's own `#[cfg(test)]` build.
///
/// [`SecretStore::get`] always returns `Ok(None)` (never seeds a value —
/// there's nothing to seed since nothing can ever be [`set`](SecretStore::set)
/// successfully) and [`SecretStore::delete`] always returns `Ok(())` (Buzz
/// parity: deleting is never the failure this double exists to simulate).
#[derive(Debug, Default)]
pub struct FailingSecretStore;

impl FailingSecretStore {
    /// Creates a store whose every [`SecretStore::set`] call fails.
    pub fn new() -> Self {
        Self
    }
}

impl SecretStore for FailingSecretStore {
    fn get(&self, _key: SecretKey) -> Result<Option<String>, SecretError> {
        Ok(None)
    }

    fn set(&self, _key: SecretKey, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::Backend {
            detail: "scripted failure".to_string(),
        })
    }

    fn delete(&self, _key: SecretKey) -> Result<(), SecretError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_absent_key_is_none() {
        let store = MemorySecretStore::new();
        assert_eq!(store.get(SecretKey::OpenAiApiKey).unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips() {
        let store = MemorySecretStore::new();
        store.set(SecretKey::OpenAiApiKey, "sk-test-123").unwrap();
        assert_eq!(
            store.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("sk-test-123".to_string())
        );
    }

    #[test]
    fn delete_removes_the_entry() {
        let store = MemorySecretStore::new();
        store.set(SecretKey::OpenAiApiKey, "sk-test-123").unwrap();
        store.delete(SecretKey::OpenAiApiKey).unwrap();
        assert_eq!(store.get(SecretKey::OpenAiApiKey).unwrap(), None);
    }

    #[test]
    fn delete_of_absent_key_is_not_an_error() {
        let store = MemorySecretStore::new();
        assert!(store.delete(SecretKey::OpenAiApiKey).is_ok());
    }

    #[test]
    fn set_overwrites_existing_value() {
        let store = MemorySecretStore::new();
        store.set(SecretKey::OpenAiApiKey, "first").unwrap();
        store.set(SecretKey::OpenAiApiKey, "second").unwrap();
        assert_eq!(
            store.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("second".to_string())
        );
    }

    #[test]
    fn entry_name_matches_buzz_key_value() {
        assert_eq!(SecretKey::OpenAiApiKey.entry_name(), "OpenAI API key");
    }

    #[test]
    fn debug_output_redacts_stored_secret_values() {
        let store = MemorySecretStore::new();
        let secret_value = "sk-super-secret-value-must-not-be-printed";
        store.set(SecretKey::OpenAiApiKey, secret_value).unwrap();

        let debug_output = format!("{store:?}");

        assert!(
            !debug_output.contains(secret_value),
            "Debug output must not contain the stored secret value, got: {debug_output}"
        );
        // Sanity check the impl still reports something useful.
        assert!(debug_output.contains("MemorySecretStore"));
        assert!(debug_output.contains("entry_count"));
    }

    // -- FailingSecretStore -----------------------------------------------------

    #[test]
    fn set_always_fails_with_a_backend_error() {
        let store = FailingSecretStore::new();
        let err = store
            .set(SecretKey::OpenAiApiKey, "sk-anything")
            .unwrap_err();
        assert!(matches!(err, SecretError::Backend { .. }));
    }

    #[test]
    fn get_and_delete_are_ok_despite_set_always_failing() {
        let store = FailingSecretStore::new();
        assert_eq!(store.get(SecretKey::OpenAiApiKey).unwrap(), None);
        assert!(store.delete(SecretKey::OpenAiApiKey).is_ok());
    }
}
