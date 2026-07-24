//! Contract test for [`KeyringSecretStore`] against the *real* OS keyring
//! (macOS Keychain / Windows Credential Manager / *nix Secret Service),
//! not a mock. `#[ignore]`d so the normal `cargo test --workspace` run never
//! touches the OS credential store or prompts the user — run explicitly with
//! `cargo test -p humm-store -- --ignored` once the sandbox is lifted.
//!
//! This deliberately runs against [`CONTRACT_TEST_SERVICE`], an isolated
//! keyring **service** distinct from production's `"humm"`
//! ([`humm_core::secrets::SERVICE`]) — never the shipped app's own
//! `"OpenAI API key"` entry. A machine with a real key stored under `"humm"`
//! must be unaffected by running this test.

use humm_core::secrets::{SecretError, SecretKey, SecretStore};
use humm_store::KeyringSecretStore;

/// Keyring service used only by this contract test — kept separate from
/// production's `"humm"` service so a real, user-stored key is never
/// overwritten or deleted by running the test suite.
const CONTRACT_TEST_SERVICE: &str = "humm-secretstore-contract-test";

/// Deletes `key` from `store` on drop, ignoring errors — an RAII cleanup
/// guard so a panic partway through the test (e.g. a failed assertion
/// between `set` and `delete`) still leaves the real keychain clean instead
/// of leaking the throwaway entry.
struct DeleteOnDrop<'a> {
    store: &'a KeyringSecretStore,
    key: SecretKey,
}

impl Drop for DeleteOnDrop<'_> {
    fn drop(&mut self) {
        let _: Result<(), SecretError> = self.store.delete(self.key);
    }
}

#[test]
#[ignore = "touches the real OS keychain"]
fn set_get_delete_round_trips_through_the_real_keyring() {
    let store = KeyringSecretStore::with_service(CONTRACT_TEST_SERVICE);
    let _cleanup = DeleteOnDrop {
        store: &store,
        key: SecretKey::OpenAiApiKey,
    };
    let throwaway_value = "sk-humm-contract-test-throwaway-value";

    store
        .set(SecretKey::OpenAiApiKey, throwaway_value)
        .expect("set should succeed against the real keyring");

    let read_back = store
        .get(SecretKey::OpenAiApiKey)
        .expect("get should succeed against the real keyring");
    assert_eq!(read_back.as_deref(), Some(throwaway_value));

    store
        .delete(SecretKey::OpenAiApiKey)
        .expect("delete should succeed against the real keyring");

    let after_delete = store
        .get(SecretKey::OpenAiApiKey)
        .expect("get after delete should succeed, not error");
    assert_eq!(after_delete, None);
}
