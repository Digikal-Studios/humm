//! Small, pure environment-variable parsing rules shared by both binaries
//! (`humm-cli`'s `humm add` and `humm-gui`'s `services::init` engine-tuning
//! wiring): every function here takes an already-read `Option<&str>` rather
//! than touching `std::env` itself, so each is trivially unit-testable
//! without mutating real process-global state.
//!
//! Moved here from `humm-cli/src/run.rs` (M3 Task 10) once `humm-gui` needed
//! the identical `HUMM_FORCE_CPU`/`HUMM_WHISPER_N_THREADS`/`HUMM_DISABLE_VAD`
//! semantics for its own engine-tuning/VAD-ensure wiring — core is the
//! natural home for logic two binaries share verbatim.

/// `HUMM_FORCE_CPU` semantics mirror Buzz's `BUZZ_FORCE_CPU`
/// (`buzz/transcriber/whisper_cpp.py:191-193`): unset or `"false"` leaves GPU
/// use enabled; any other value forces CPU.
pub fn use_gpu_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some(v) if v != "false")
}

/// The full `use_gpu` resolution once `settings.models.force_cpu`
/// (design spec §8.1, Preferences → General → Disable GPU) joins the
/// env-only [`use_gpu_from_env`] check: `HUMM_FORCE_CPU` wins whenever it is
/// set to anything but `"false"` (forces CPU regardless of the setting);
/// otherwise the setting decides (`force_cpu_setting = true` forces CPU when
/// the env var is unset or `"false"`). Equivalent to `!(env_forces_cpu ||
/// force_cpu_setting)` — the exact precedence
/// [`crate::settings::ModelSettings::force_cpu`]'s own doc comment
/// documents.
pub fn use_gpu_resolution(env_value: Option<&str>, force_cpu_setting: bool) -> bool {
    use_gpu_from_env(env_value) && !force_cpu_setting
}

/// `HUMM_DISABLE_VAD` semantics: unset or empty leaves VAD on (the default for
/// real users); any non-empty value disables it — a test-only kill-switch so
/// hermetic tests never reach the network for the Silero model.
pub fn vad_disabled_from_env(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// `HUMM_DISABLE_KEYRING` semantics (M6 Task 11, F-002/F-041): unset or empty
/// leaves the real OS keyring in play (the default for real users); any
/// non-empty value makes the CLI's OpenAI-token keyring lookup report "no
/// entry" without ever opening a `keyring::Entry` — a test-only kill-switch,
/// same shape as [`vad_disabled_from_env`], so hermetic `humm-cli` tests
/// never touch the real OS keychain when exercising `-m openaiapi`'s
/// token-resolution failure path.
pub fn keyring_disabled_from_env(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// Thread-count override; `None` on absence or an unparseable value (the
/// adapter's own default then applies).
pub fn threads_from_env(value: Option<&str>) -> Option<u32> {
    value.and_then(|v| v.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn use_gpu_from_env_unset_allows_gpu() {
        assert!(use_gpu_from_env(None));
    }

    #[test]
    fn use_gpu_from_env_false_allows_gpu() {
        assert!(use_gpu_from_env(Some("false")));
    }

    #[test]
    fn use_gpu_from_env_any_other_value_forces_cpu() {
        assert!(!use_gpu_from_env(Some("true")));
        assert!(!use_gpu_from_env(Some("1")));
        assert!(!use_gpu_from_env(Some("")));
    }

    #[test]
    fn use_gpu_resolution_env_wins() {
        // Any non-"false" env value forces CPU regardless of the setting.
        assert!(!use_gpu_resolution(Some("true"), false));
        assert!(!use_gpu_resolution(Some("true"), true));
        assert!(!use_gpu_resolution(Some("1"), false));
    }

    #[test]
    fn use_gpu_resolution_setting_forces_cpu_when_env_unset_or_false() {
        assert!(!use_gpu_resolution(None, true));
        assert!(!use_gpu_resolution(Some("false"), true));
    }

    #[test]
    fn use_gpu_resolution_allows_gpu_when_neither_forces_cpu() {
        assert!(use_gpu_resolution(None, false));
        assert!(use_gpu_resolution(Some("false"), false));
    }

    #[test]
    fn vad_disabled_from_env_unset_or_empty_leaves_vad_on() {
        assert!(!vad_disabled_from_env(None));
        assert!(!vad_disabled_from_env(Some("")));
    }

    #[test]
    fn vad_disabled_from_env_any_non_empty_value_disables_vad() {
        assert!(vad_disabled_from_env(Some("1")));
        assert!(vad_disabled_from_env(Some("true")));
        assert!(vad_disabled_from_env(Some("false")));
    }

    #[test]
    fn keyring_disabled_from_env_unset_or_empty_leaves_keyring_on() {
        assert!(!keyring_disabled_from_env(None));
        assert!(!keyring_disabled_from_env(Some("")));
    }

    #[test]
    fn keyring_disabled_from_env_any_non_empty_value_disables_keyring() {
        assert!(keyring_disabled_from_env(Some("1")));
        assert!(keyring_disabled_from_env(Some("true")));
        assert!(keyring_disabled_from_env(Some("false")));
    }

    #[test]
    fn threads_from_env_parses_valid_value() {
        assert_eq!(threads_from_env(Some("4")), Some(4));
    }

    #[test]
    fn threads_from_env_none_when_unset_or_invalid() {
        assert_eq!(threads_from_env(None), None);
        assert_eq!(threads_from_env(Some("not-a-number")), None);
    }
}
