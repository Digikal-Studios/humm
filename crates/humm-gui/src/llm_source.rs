//! [`GuiLlmSource`]: the GUI's [`LlmEndpointSource`] (F-031, design spec §6),
//! wired into `services::init`'s `RunnerDeps::llm`.
//!
//! The file-transcription pipeline queries this once per task, at task start,
//! for the LLM-translation endpoint (Buzz reads its endpoint at translator
//! construction — `translator.py:36-53`). Resolution is the env → settings →
//! keyring precedence of [`humm_core::openai::resolve_endpoint`]: the two
//! `HUMM_TRANSLATION_API_*` env vars are read **at call time** (fresh per
//! task, never snapshotted at construction), the base URL falls back to
//! `settings.openai.base_url`, and the key falls back to the [`SecretStore`]
//! (the keyring in production). No key anywhere → `None`, and the pipeline
//! skips translation entirely (design spec §6's "endpoint unresolvable"
//! degradation).

use std::sync::{Arc, Mutex};

use humm_core::openai::{ENV_BASE, ENV_KEY, LlmEndpoint, LlmEndpointSource, resolve_endpoint};
use humm_core::secrets::SecretStore;
use humm_core::settings::Settings;

/// The GUI's [`LlmEndpointSource`]: resolves the LLM-translation endpoint from
/// the two `HUMM_TRANSLATION_API_*` env vars, the shared live `settings`, and
/// the `secrets` store, in that precedence (design spec §4.3/§6).
pub struct GuiLlmSource {
    /// The shared live settings — `openai.base_url` is read fresh on every
    /// [`LlmEndpointSource::resolve`] call, so a base URL the user changes in
    /// Preferences after `services::init` is honored by the next task.
    pub settings: Arc<Mutex<Settings>>,
    /// The secret store the fallback API key is read from (the OS keyring in
    /// production, an in-memory double in tests).
    pub secrets: Arc<dyn SecretStore>,
}

impl GuiLlmSource {
    /// [`LlmEndpointSource::resolve`]'s real body, taking the two env values
    /// as explicit arguments so tests thread synthetic values rather than
    /// mutating process-global environment state (Global Constraint 7: tests
    /// never `set_var`, mirroring `openai::translate`'s own env-free
    /// `resolve_endpoint` tests). [`LlmEndpointSource::resolve`] itself reads
    /// the live process env and delegates here.
    ///
    /// The settings lock is held only long enough to clone `openai.base_url`,
    /// then dropped before [`resolve_endpoint`] does any (potentially
    /// blocking) keyring read (Standing Policy: never hold a lock across
    /// blocking work).
    pub(crate) fn resolve_with_env(
        &self,
        env_base: Option<String>,
        env_key: Option<String>,
    ) -> Option<LlmEndpoint> {
        let settings_base = {
            let guard = self
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.openai.base_url.clone()
        };
        resolve_endpoint(env_base, env_key, &settings_base, &*self.secrets)
    }
}

impl LlmEndpointSource for GuiLlmSource {
    fn resolve(&self) -> Option<LlmEndpoint> {
        // Read the env vars fresh, per task (design spec §4.3). `.ok()` folds
        // both "unset" and "not valid UTF-8" to `None`; an env var set to the
        // empty string is passed through as `Some("")`, which
        // `resolve_endpoint` itself treats as unset.
        let env_base = std::env::var(ENV_BASE).ok();
        let env_key = std::env::var(ENV_KEY).ok();
        self.resolve_with_env(env_base, env_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::secrets::{MemorySecretStore, SecretKey};

    /// Builds a `GuiLlmSource` over a fresh `MemorySecretStore` (optionally
    /// pre-seeded with a keyring key) and `settings.openai.base_url`.
    fn source(settings_base: Option<&str>, keyring_key: Option<&str>) -> GuiLlmSource {
        let secrets = MemorySecretStore::new();
        if let Some(key) = keyring_key {
            secrets.set(SecretKey::OpenAiApiKey, key).unwrap();
        }
        let mut settings = Settings::default();
        settings.openai.base_url = settings_base.map(str::to_string);
        GuiLlmSource {
            settings: Arc::new(Mutex::new(settings)),
            secrets: Arc::new(secrets),
        }
    }

    #[test]
    fn env_key_wins_over_keyring() {
        let src = source(None, Some("keyring-key"));
        let endpoint = src
            .resolve_with_env(None, Some("env-key".to_string()))
            .expect("endpoint resolves");
        assert_eq!(endpoint.auth.api_key, "env-key");
    }

    #[test]
    fn env_base_wins_over_settings_base() {
        let src = source(Some("http://settings-base.example"), Some("some-key"));
        let endpoint = src
            .resolve_with_env(Some("http://env-base.example".to_string()), None)
            .expect("endpoint resolves");
        assert_eq!(
            endpoint.auth.base_url,
            Some("http://env-base.example".to_string())
        );
    }

    #[test]
    fn settings_base_used_when_no_env_base() {
        let src = source(Some("http://settings-base.example"), Some("some-key"));
        let endpoint = src.resolve_with_env(None, None).expect("endpoint resolves");
        assert_eq!(
            endpoint.auth.base_url,
            Some("http://settings-base.example".to_string())
        );
        // The keyring key is the one used when no env key is present.
        assert_eq!(endpoint.auth.api_key, "some-key");
    }

    #[test]
    fn no_key_anywhere_is_none() {
        let src = source(Some("http://settings-base.example"), None);
        assert!(src.resolve_with_env(None, None).is_none());
    }
}
