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

/// `HUMM_DISABLE_VAD` semantics: unset or empty leaves VAD on (the default for
/// real users); any non-empty value disables it — a test-only kill-switch so
/// hermetic tests never reach the network for the Silero model.
pub fn vad_disabled_from_env(value: Option<&str>) -> bool {
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
    fn threads_from_env_parses_valid_value() {
        assert_eq!(threads_from_env(Some("4")), Some(4));
    }

    #[test]
    fn threads_from_env_none_when_unset_or_invalid() {
        assert_eq!(threads_from_env(None), None);
        assert_eq!(threads_from_env(Some("not-a-number")), None);
    }
}
