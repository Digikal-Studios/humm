//! Contract test for [`KeyringSecretStore`] against the *real* OS keyring
//! (macOS Keychain / Windows Credential Manager / *nix Secret Service),
//! not a mock. `#[ignore]`d so the normal `cargo test --workspace` run never
//! touches the OS credential store or prompts the user — run explicitly with
//! `cargo test -p humm-store -- --ignored` once the sandbox is lifted.
//!
//! This overwrites (then deletes) whatever is currently stored under the
//! `"humm"` service's `"OpenAI API key"` entry — by design, the same entry
//! the real app reads via `SecretKey::OpenAiApiKey`. Don't run this on a
//! machine with a real key you care about preserving.

use humm_core::secrets::{SecretKey, SecretStore};
use humm_store::KeyringSecretStore;

#[test]
#[ignore = "touches the real OS keychain"]
fn set_get_delete_round_trips_through_the_real_keyring() {
    let store = KeyringSecretStore::new();
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
