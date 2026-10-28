//! F-116 `transcript_resizer` (design spec §5): after a transcription
//! completes with word-level timings, regroups the stored segments using the
//! same merge-by-gap/split-by-punctuation/split-by-max-length algorithm as
//! the viewer's Merge feature (F-086, `crate::regroup`) and replaces the
//! transcription with the result. Ported from
//! `transcript_resizer/plugin.py`'s `TranscriptResizerPlugin.on_complete`
//! (whole file read for context).
//!
//! [`run`] is built and unit-tested standalone in this task (M7 Task 7);
//! `crate::queue`'s `on_complete` hook chain (design spec §2.1, fixed order
//! **auto_resize → summary → docx**) calls it first, gated on
//! [`crate::settings::AutoResizeSettings::enabled`] by the *caller* — the
//! same convention [`crate::postproc::summary::run`]/
//! [`crate::postproc::docx::run`] already follow.

use crate::boundary::TaskStore;
use crate::domain::TranscriptionId;
use crate::regroup;
use crate::settings::AutoResizeSettings;

use super::PostprocConfig;

/// Outcome of one [`run`] call.
///
/// A *skip* (word-level timings off, an empty regroup algorithm, or a
/// regroup that legitimately produces zero segments) is not an error: both
/// fields stay at their default — [`Self::replaced`] is `false` and
/// [`Self::error`] is `None`. Only an actual failure (a store read failure,
/// a regroup-algorithm parse error, or a store write failure) populates
/// [`Self::error`]; in every one of those cases the original transcription
/// is left untouched (Buzz's `_regroup` catches the regroup exception and
/// returns `[]`, which `on_complete` then treats exactly like a legitimate
/// empty result — "keep the original" — logging the failure at `error`
/// level rather than `info` first, `plugin.py:212-214,117-119`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResizeResult {
    /// Whether the task's persisted segments were replaced with the
    /// regrouped result.
    pub replaced: bool,
    /// A non-fatal error recorded along the way, if any (see the struct
    /// doc comment for exactly when this is set vs. left `None`).
    pub error: Option<String>,
}

/// Runs the transcript-resizer hook for one completed task (F-116, design
/// spec §5), porting `TranscriptResizerPlugin.on_complete`
/// (`transcript_resizer/plugin.py:91-127`):
///
/// 1. **Gate on word-level timings** (`:91-98`): `word_timestamps` — the
///    task's own transcription options, not this hook's settings — false is
///    a silent skip (logged at `info`, Buzz's exact string).
/// 2. **Build the regroup algorithm string** (`:100-103,129-151`): the same
///    composition the viewer's Merge feature uses —
///    [`regroup::default_algo`] from `cfg.resize`'s checkboxes/fields (via
///    Buzz's `(value or default)` fallback idiom, `:133-135`; see
///    [`built_algo`]) — with the `HUMM_MERGE_REGROUP_RULE` env override
///    applied *last* ([`regroup::algo_from_env_or`], `:151`; the override
///    wins even when set to the empty string). An empty result is a silent
///    skip (logged at `info`, Buzz's exact string).
/// 3. **Read segments from the store**: Buzz's `get_transcript` callback
///    re-reads `context.transcription_service.get_transcription_segments`
///    from the DB rather than using the in-memory `on_complete` segments
///    parameter (`:178-182`) — this hook does the same via
///    [`TaskStore::segments_for`], which is also why its own signature takes
///    no segments parameter.
///
///    **Dropped Buzz check, design spec §5 ledger #L5:**
///    `_resolve_audio_path`'s skip-if-source-audio-missing check
///    (`:105-110,153-167`) is deliberately not ported here. It exists only
///    because stable-ts's `transcribe_any` demands an `audio` argument that
///    the regroup call then completely ignores (`vad=False,
///    suppress_silence=False`) — a Python API-shape artifact, not a real
///    data dependency. humm's [`regroup::regroup_segments`] is a pure
///    function over the store's segment rows, so this hook runs correctly
///    even after folder-watch has moved or deleted the source media file —
///    strictly more capable, with no output difference when the source
///    audio does still exist.
/// 4. **Regroup** ([`regroup::regroup_segments`], `:169-223`): a parse
///    failure (an invalid algorithm string — reachable today only via a
///    malformed `merge_gap_seconds`/`max_length`/`punctuation` field, or a
///    `HUMM_MERGE_REGROUP_RULE` override naming an unsupported op) is
///    recorded in [`ResizeResult::error`] and logged at `error` (Buzz's
///    `_regroup` catches the stable-ts exception, logs it, and returns
///    `[]`, `:212-214`); an empty (but successfully parsed) result is a
///    silent skip at `info` (`:117-119`) — both leave the original
///    transcription untouched.
/// 5. **Replace** ([`TaskStore::replace_task_segments`], `:121-127`): only
///    once the regrouped result is non-empty.
pub fn run(
    cfg: &PostprocConfig,
    transcription_id: &TranscriptionId,
    word_timestamps: bool,
    language: Option<&str>,
    store: &dyn TaskStore,
) -> ResizeResult {
    if !word_timestamps {
        log::info!("Transcript resizer skipped: word-level timings not enabled");
        return ResizeResult::default();
    }

    let algo = regroup::algo_from_env_or(built_algo(&cfg.resize));
    if algo.is_empty() {
        log::info!("Transcript resizer skipped: no merge options enabled");
        return ResizeResult::default();
    }

    let segments = match store.segments_for(transcription_id) {
        Ok(segments) => segments,
        Err(e) => {
            let message = format!("Transcript resizer failed: could not read segments: {e}");
            log::error!("{message}");
            return ResizeResult {
                replaced: false,
                error: Some(message),
            };
        }
    };

    let regrouped = match regroup::regroup_segments(&segments, language, &algo) {
        Ok(segments) => segments,
        Err(e) => {
            let message = format!("Transcript resizer regroup failed: {e}");
            log::error!("{message}");
            return ResizeResult {
                replaced: false,
                error: Some(message),
            };
        }
    };

    if regrouped.is_empty() {
        log::info!("Transcript resizer produced no segments; keeping original");
        return ResizeResult::default();
    }

    match store.replace_task_segments(transcription_id, &regrouped) {
        Ok(()) => {
            log::info!(
                "Transcript resizer replaced transcription with {} segments",
                regrouped.len()
            );
            ResizeResult {
                replaced: true,
                error: None,
            }
        }
        Err(e) => {
            let message = format!("Transcript resizer failed to save: {e}");
            log::error!("{message}");
            ResizeResult {
                replaced: false,
                error: Some(message),
            }
        }
    }
}

/// Buzz's `_build_regroup_string` (`transcript_resizer/plugin.py:129-151`),
/// minus its trailing `os.getenv(...)` override — [`run`] applies that
/// separately via [`regroup::algo_from_env_or`], preserving Buzz's own
/// composition order (env wins *last*, `:151`) while keeping this half
/// testable without touching real process environment.
///
/// `merge_gap_seconds`/`max_length` go through Buzz's `(value or
/// default).strip()` idiom (`:133,135` — an exactly-empty configured value
/// falls back to the type's Buzz default, then *both* paths are trimmed);
/// `punctuation` uses Buzz's plain `value or default` with **no** trim
/// (`:134` — the default punctuation string's trailing space is
/// significant, and Buzz never strips a configured value here).
fn built_algo(cfg: &AutoResizeSettings) -> String {
    let gap = or_default_then_trim(
        &cfg.merge_gap_seconds,
        AutoResizeSettings::DEFAULT_MERGE_GAP_SECONDS,
    );
    let punctuation = or_default(&cfg.punctuation, AutoResizeSettings::DEFAULT_PUNCTUATION);
    let max_length = or_default_then_trim(&cfg.max_length, AutoResizeSettings::DEFAULT_MAX_LENGTH);

    regroup::default_algo(
        cfg.merge_by_gap,
        &gap,
        cfg.split_by_punctuation,
        punctuation,
        cfg.split_by_max_length,
        &max_length,
    )
}

/// Buzz's `(value or default).strip()` idiom: an exactly-empty `value`
/// falls back to `default`; anything else (even a whitespace-only string,
/// which Python treats as truthy) is used as-is — *then* both paths are
/// `.trim()`-med. Mirrors `postproc::summary`'s own private
/// `or_default_then_trim` exactly (each hook module keeps its own copy
/// rather than sharing one private helper across files).
fn or_default_then_trim(value: &str, default: &str) -> String {
    let base = if value.is_empty() { default } else { value };
    base.trim().to_string()
}

/// Buzz's plain `value or default` idiom, with **no** trim
/// (`punctuation`'s own field, `:134`): an exactly-empty `value` falls back
/// to `default` verbatim; anything else is used exactly as configured.
fn or_default<'a>(value: &'a str, default: &'a str) -> &'a str {
    if value.is_empty() { default } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Millis, Segment};
    use crate::error::StoreError;
    use crate::settings::{
        AiSummarySettings, ExportDocxSettings, LangDetectSettings, NoiseReductionSettings,
        SkipSettings,
    };
    use std::path::Path;

    // -- a minimal `TaskStore` double: only `segments_for`/
    //    `replace_task_segments` are ever exercised by `run`. -----------------

    #[derive(Default)]
    struct FakeStore {
        segments: std::sync::Mutex<Vec<Segment>>,
        replaced: std::sync::Mutex<Vec<(TranscriptionId, Vec<Segment>)>>,
        fail_read: bool,
        fail_write: bool,
    }

    impl TaskStore for FakeStore {
        fn create_task(
            &self,
            _id: &TranscriptionId,
            _spec: &crate::domain::TaskSpec,
            _source: crate::domain::Source,
        ) -> Result<(), StoreError> {
            unreachable!("run() never creates tasks")
        }

        fn requeue_task(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never requeues tasks")
        }

        fn mark_started(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks started")
        }

        fn update_progress(&self, _id: &TranscriptionId, _fraction: f64) -> Result<(), StoreError> {
            unreachable!("run() never updates progress")
        }

        fn update_file(&self, _id: &TranscriptionId, _path: &Path) -> Result<(), StoreError> {
            unreachable!("run() never updates the file path")
        }

        fn set_task_name(&self, _id: &TranscriptionId, _name: &str) -> Result<(), StoreError> {
            unreachable!("run() never sets the task name")
        }

        fn update_language(
            &self,
            _id: &TranscriptionId,
            _language: &str,
        ) -> Result<(), StoreError> {
            unreachable!("run() never updates the language")
        }

        fn mark_completed(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks completed")
        }

        fn mark_failed(&self, _id: &TranscriptionId, _error: &str) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks failed")
        }

        fn mark_canceled(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks canceled")
        }

        fn mark_skipped(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks skipped")
        }

        fn find_completed_by_file_name(
            &self,
            _name: &str,
        ) -> Result<Option<TranscriptionId>, StoreError> {
            unreachable!("run() never looks up completed tasks by name")
        }

        fn find_completed_by_url(&self, _url: &str) -> Result<Option<TranscriptionId>, StoreError> {
            unreachable!("run() never looks up completed tasks by url")
        }

        fn segments_for(&self, _id: &TranscriptionId) -> Result<Vec<Segment>, StoreError> {
            if self.fail_read {
                return Err(StoreError::Query {
                    detail: "boom".to_string(),
                });
            }
            Ok(self.segments.lock().expect("segments lock").clone())
        }

        fn set_task_notes(&self, _id: &TranscriptionId, _notes: &str) -> Result<(), StoreError> {
            unreachable!("run() never sets task notes")
        }

        fn replace_task_segments(
            &self,
            id: &TranscriptionId,
            segs: &[Segment],
        ) -> Result<(), StoreError> {
            if self.fail_write {
                return Err(StoreError::Query {
                    detail: "boom".to_string(),
                });
            }
            self.replaced
                .lock()
                .expect("replaced lock")
                .push((*id, segs.to_vec()));
            Ok(())
        }
    }

    fn seg(start: i64, end: i64, text: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    fn base_cfg() -> PostprocConfig {
        PostprocConfig {
            skip: SkipSettings::default(),
            summary: AiSummarySettings::default(),
            summary_api_key: None,
            docx: ExportDocxSettings::default(),
            resize: AutoResizeSettings::default(),
            paragraph_split: Millis(2000),
            noise_reduction: NoiseReductionSettings::default(),
            language_detection: LangDetectSettings::default(),
        }
    }

    /// Skips the current test if `HUMM_MERGE_REGROUP_RULE` happens to be set
    /// in the real process environment: that override would win over
    /// whatever this test builds from `AutoResizeSettings` (design spec §5:
    /// "env wins even when empty"), defeating a test that's specifically
    /// exercising the *built* string or a deliberately-invalid one. Matches
    /// `humm-gui`'s `resizer.rs` test convention (same guard, same reason) —
    /// no test in this module ever mutates the real environment.
    fn skip_if_env_override_set() -> bool {
        std::env::var_os("HUMM_MERGE_REGROUP_RULE").is_some()
    }

    // -- gate 1: word-level timings -----------------------------------------

    #[test]
    fn word_timestamps_off_skips_before_any_store_read() {
        let cfg = base_cfg();
        let store = FakeStore::default();

        let result = run(&cfg, &TranscriptionId::new(), false, None, &store);

        assert_eq!(result, ResizeResult::default());
        assert!(store.replaced.lock().expect("replaced lock").is_empty());
    }

    // -- built_algo: the 8 checkbox combos, hand-traced against Buzz --------
    //
    // `_build_regroup_string` (plugin.py:129-151), traced by hand for every
    // (merge_by_gap, split_by_punctuation, split_by_max_length) combination
    // at Buzz's own defaults (gap "0.2", punctuation DEFAULT_PUNCTUATION,
    // max_length "42").

    #[test]
    fn algo_matrix_all_three_on_full_default_composite() {
        let cfg = AutoResizeSettings::default();
        assert_eq!(
            built_algo(&cfg),
            "mg=0.2++42+1_sp=.* /./. /。/?/? /？/!/! /！/,/, _sl=42"
        );
    }

    #[test]
    fn algo_matrix_mg_only() {
        let cfg = AutoResizeSettings {
            split_by_punctuation: false,
            split_by_max_length: false,
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "mg=0.2");
    }

    #[test]
    fn algo_matrix_sp_only() {
        let cfg = AutoResizeSettings {
            merge_by_gap: false,
            split_by_max_length: false,
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "sp=.* /./. /。/?/? /？/!/! /！/,/, ");
    }

    #[test]
    fn algo_matrix_sl_only() {
        let cfg = AutoResizeSettings {
            merge_by_gap: false,
            split_by_punctuation: false,
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "sl=42");
    }

    #[test]
    fn algo_matrix_mg_and_sp_no_len_suffix_on_mg() {
        // split_by_max_length is off, so `mg` gets no `++len+1` suffix even
        // though merge_by_gap is on (`:140-141`).
        let cfg = AutoResizeSettings {
            split_by_max_length: false,
            ..AutoResizeSettings::default()
        };
        assert_eq!(
            built_algo(&cfg),
            "mg=0.2_sp=.* /./. /。/?/? /？/!/! /！/,/, "
        );
    }

    #[test]
    fn algo_matrix_mg_and_sl_gets_len_suffix() {
        let cfg = AutoResizeSettings {
            split_by_punctuation: false,
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "mg=0.2++42+1_sl=42");
    }

    #[test]
    fn algo_matrix_sp_and_sl() {
        let cfg = AutoResizeSettings {
            merge_by_gap: false,
            ..AutoResizeSettings::default()
        };
        assert_eq!(
            built_algo(&cfg),
            "sp=.* /./. /。/?/? /？/!/! /！/,/, _sl=42"
        );
    }

    #[test]
    fn algo_matrix_all_three_off_is_empty() {
        let cfg = AutoResizeSettings {
            merge_by_gap: false,
            split_by_punctuation: false,
            split_by_max_length: false,
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "");
    }

    // -- built_algo: the (value or default) fallback idioms ------------------

    #[test]
    fn empty_merge_gap_seconds_falls_back_and_trims() {
        let cfg = AutoResizeSettings {
            split_by_punctuation: false,
            split_by_max_length: false,
            merge_gap_seconds: String::new(),
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "mg=0.2");
    }

    #[test]
    fn non_empty_merge_gap_seconds_is_used_and_trimmed() {
        let cfg = AutoResizeSettings {
            split_by_punctuation: false,
            split_by_max_length: false,
            merge_gap_seconds: "  0.5  ".to_string(),
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "mg=0.5");
    }

    #[test]
    fn empty_punctuation_falls_back_verbatim_no_trim() {
        let cfg = AutoResizeSettings {
            merge_by_gap: false,
            split_by_max_length: false,
            punctuation: String::new(),
            ..AutoResizeSettings::default()
        };
        assert_eq!(
            built_algo(&cfg),
            format!("sp={}", AutoResizeSettings::DEFAULT_PUNCTUATION)
        );
    }

    #[test]
    fn non_empty_punctuation_is_used_verbatim_not_trimmed() {
        // Buzz's `punctuation = cfg.get("punctuation") or DEFAULT_PUNCTUATION`
        // has no `.strip()` -- a configured value with meaningful leading/
        // trailing whitespace must survive untouched.
        let cfg = AutoResizeSettings {
            merge_by_gap: false,
            split_by_max_length: false,
            punctuation: " , ".to_string(),
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "sp= , ");
    }

    #[test]
    fn empty_max_length_falls_back_and_trims() {
        let cfg = AutoResizeSettings {
            merge_by_gap: false,
            split_by_punctuation: false,
            max_length: String::new(),
            ..AutoResizeSettings::default()
        };
        assert_eq!(built_algo(&cfg), "sl=42");
    }

    // -- env-override composition (via `override_algo`'s testable seam, not
    //    real env mutation) --------------------------------------------------

    #[test]
    fn env_override_wins_even_when_empty() {
        let cfg = AutoResizeSettings::default();
        let built = built_algo(&cfg);
        assert_eq!(regroup::override_algo(built.clone(), Some("da=1")), "da=1");
        assert_eq!(regroup::override_algo(built, Some("")), "");
    }

    #[test]
    fn no_env_override_keeps_built_string() {
        let cfg = AutoResizeSettings::default();
        let built = built_algo(&cfg);
        assert_eq!(regroup::override_algo(built.clone(), None), built);
    }

    // -- gate 2: empty algo skip ----------------------------------------------

    #[test]
    fn empty_algo_skips_before_any_store_read() {
        if skip_if_env_override_set() {
            return;
        }
        let mut cfg = base_cfg();
        cfg.resize = AutoResizeSettings {
            merge_by_gap: false,
            split_by_punctuation: false,
            split_by_max_length: false,
            ..AutoResizeSettings::default()
        };
        let store = FakeStore::default();

        let result = run(&cfg, &TranscriptionId::new(), true, None, &store);

        assert_eq!(result, ResizeResult::default());
        assert!(store.replaced.lock().expect("replaced lock").is_empty());
    }

    // -- store read failure ---------------------------------------------------

    #[test]
    fn store_read_failure_is_recorded_as_error() {
        if skip_if_env_override_set() {
            return;
        }
        let cfg = base_cfg();
        let store = FakeStore {
            fail_read: true,
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), true, None, &store);

        assert!(!result.replaced);
        assert!(result.error.is_some());
    }

    // -- regroup error: a legitimately-reachable bad `merge_gap_seconds` -----

    #[test]
    fn regroup_error_is_recorded_and_keeps_original() {
        if skip_if_env_override_set() {
            return;
        }
        // A user-typed non-numeric merge gap -- `mg`'s arg 0 must parse as a
        // number (`regroup::parser::parse_mg`); this is a legitimately
        // reachable settings misconfiguration (the field is a free-text UI
        // input), not an env-injected string, so it stays network/env-free.
        let mut cfg = base_cfg();
        cfg.resize = AutoResizeSettings {
            merge_by_gap: true,
            split_by_punctuation: false,
            split_by_max_length: false,
            merge_gap_seconds: "not-a-number".to_string(),
            ..AutoResizeSettings::default()
        };
        let store = FakeStore {
            segments: std::sync::Mutex::new(vec![seg(0, 1000, "hello.")]),
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), true, None, &store);

        assert!(!result.replaced);
        let error = result
            .error
            .expect("a regroup parse failure must be recorded");
        assert!(error.contains("regroup failed"), "error was: {error}");
        assert!(store.replaced.lock().expect("replaced lock").is_empty());
    }

    // -- empty regroup result: keep original, no error ------------------------

    #[test]
    fn empty_store_segments_produce_no_replacement_no_error() {
        if skip_if_env_override_set() {
            return;
        }
        let cfg = base_cfg();
        let store = FakeStore::default(); // no segments planted

        let result = run(&cfg, &TranscriptionId::new(), true, None, &store);

        assert_eq!(result, ResizeResult::default());
    }

    // -- store-backed end-to-end replace: word-timestamped rows -> regrouped
    //    rows, translations dropped per `replace_task_segments`'s contract ---

    #[test]
    fn end_to_end_replaces_segments_with_regrouped_result() {
        if skip_if_env_override_set() {
            return;
        }
        let cfg = base_cfg(); // full default algo: mg+sp+sl all on
        let store = FakeStore {
            segments: std::sync::Mutex::new(vec![
                seg(0, 1000, "Hello there."),
                seg(1000, 2000, "How are you?"),
            ]),
            ..FakeStore::default()
        };
        let id = TranscriptionId::new();

        let result = run(&cfg, &id, true, Some("en"), &store);

        assert!(
            result.replaced,
            "a non-empty regroup must replace: {result:?}"
        );
        assert_eq!(result.error, None);
        let replaced = store.replaced.lock().expect("replaced lock");
        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].0, id);
        assert!(!replaced[0].1.is_empty());
        // Buzz's `replace_transcription_segments` hardcodes `translation=''`
        // regardless of what the input segment carries
        // (`transcription_service.py:81-92`) -- `TaskStore::replace_task_segments`
        // is documented to do the same, so this only proves the hook feeds
        // it segments at all, not a resize-specific translation contract.
        assert!(replaced[0].1.iter().all(|s| s.translation.is_empty()));
    }

    // -- CJK non-space-language separator, exercising the reused
    //    `words_from_segments` reconstruction (plugin.py:178-200) end to end --

    #[test]
    fn cjk_language_uses_non_space_separator_end_to_end() {
        if skip_if_env_override_set() {
            return;
        }
        // Neither input segment ends with sentence-ending punctuation, so
        // `words_from_segments`'s own sentence-grouping step (no regroup op
        // even needs to run) already combines both into one pseudo-segment
        // -- the "trailing words form a final group" branch. The `zh`
        // separator is `""`, so the joined text has no injected space
        // between the per-segment texts, exactly plugin.py's
        // `get_transcript` shape (`buzz_segment.text + separator`,
        // separator "" for zh/ja/th/lo/km/my, `:35,176,188`). Default merge
        // settings (mg/sp/sl all on, 42-char max) run on top of that single
        // group and change nothing: "你好世界" has neither a merge boundary
        // nor punctuation nor a length overrun to split on.
        let cfg = base_cfg();
        let store = FakeStore {
            segments: std::sync::Mutex::new(vec![seg(0, 1000, "你好"), seg(1000, 2000, "世界")]),
            ..FakeStore::default()
        };
        let id = TranscriptionId::new();

        let result = run(&cfg, &id, true, Some("zh"), &store);

        assert!(result.replaced, "{result:?}");
        let replaced = store.replaced.lock().expect("replaced lock");
        assert_eq!(replaced[0].1.len(), 1);
        assert_eq!(replaced[0].1[0].text, "你好世界");
    }

    // -- store write failure ---------------------------------------------------

    #[test]
    fn store_write_failure_is_recorded_as_error() {
        if skip_if_env_override_set() {
            return;
        }
        let cfg = base_cfg();
        let store = FakeStore {
            segments: std::sync::Mutex::new(vec![seg(0, 1000, "hello.")]),
            fail_write: true,
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), true, None, &store);

        assert!(!result.replaced);
        assert!(result.error.is_some());
    }
}
