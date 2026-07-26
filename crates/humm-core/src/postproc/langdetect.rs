//! F-114 enhanced language detection: gating + model selection (design spec
//! §5).
//!
//! Ported from Buzz's `enhanced_language_detection` plugin
//! (`plugins/enhanced_language_detection/plugin.py:59-169`): detection runs
//! only when the task's language is unset or explicitly English (Buzz's
//! `"en"` is its UI's no-selection default, so an explicit English pick
//! still triggers detection and can be overridden by it — the "en-quirk",
//! `:64-72`, deviations ledger #6), against the largest already-downloaded
//! whisper.cpp model (`Custom`/`Lumii` excluded as special-purpose models,
//! not general size variants, `:131`).
//!
//! The actual detection call and the tiny-model-download fallback live in
//! `crate::queue::Runner::run_job` (they need the runner's decoder/
//! artifact-ensurer collaborators, both private to that module); this module
//! holds only the two pure decisions Buzz's plugin makes before ever
//! touching either.

use std::path::{Path, PathBuf};

use crate::domain::ModelSize;
use crate::language::Language;
use crate::models::registry;

/// Whisper.cpp model sizes considered for language-detection model
/// selection. `Custom` and `Lumii` are deliberately excluded (Buzz
/// `plugin.py:131`): they are special-purpose models, not general size
/// variants comparable by size.
const CANDIDATE_SIZES: &[ModelSize] = &[
    ModelSize::Tiny,
    ModelSize::TinyEn,
    ModelSize::Base,
    ModelSize::BaseEn,
    ModelSize::Small,
    ModelSize::SmallEn,
    ModelSize::Medium,
    ModelSize::MediumEn,
    ModelSize::Large,
    ModelSize::LargeV2,
    ModelSize::LargeV3,
    ModelSize::LargeV3Turbo,
];

/// Buzz's `WHISPER_MODEL_SIZES` byte-size ranking (`model_loader.py:158-171`),
/// used here purely for relative ordering ("which downloaded model is
/// biggest") — the exact byte counts are Buzz's approximate expected
/// download sizes, not measurements of humm's own files. `Custom`/`Lumii`
/// rank `0` but are never reached: [`CANDIDATE_SIZES`] excludes them.
fn size_rank(size: ModelSize) -> u64 {
    match size {
        ModelSize::Tiny | ModelSize::TinyEn => 72,
        ModelSize::Base | ModelSize::BaseEn => 138,
        ModelSize::Small | ModelSize::SmallEn => 460,
        ModelSize::Medium | ModelSize::MediumEn => 1500,
        ModelSize::LargeV3Turbo => 1550,
        ModelSize::Large | ModelSize::LargeV2 | ModelSize::LargeV3 => 2870,
        ModelSize::Custom | ModelSize::Lumii => 0,
    }
}

/// Whether F-114 detection should run for a task whose transcription
/// language is `language` — Buzz's own gating rule (`plugin.py:64-72`):
/// unset (`None`, auto-detect), or explicitly English (the "en-quirk" above).
pub fn should_detect(language: Option<Language>) -> bool {
    match language {
        None => true,
        Some(lang) => lang == Language::parse("en").expect("\"en\" is a known language code"),
    }
}

/// The path to the largest already-downloaded whisper.cpp model under
/// `model_root`, or `None` if none is downloaded.
///
/// "Downloaded" means both the model file and its completeness sentinel
/// exist ([`registry::local_model_path`]'s rule — the same resume-safe check
/// every other model consumer uses). `Custom` and `Lumii` are never
/// considered (see [`CANDIDATE_SIZES`]).
///
/// Ties (e.g. `Small`/`SmallEn`, or `Large`/`LargeV2`/`LargeV3`, all sharing
/// a rank) keep the **first**-declared [`CANDIDATE_SIZES`] entry — Buzz's
/// own `_resolve_model_path` uses a strict `rank > best_size` comparison
/// (`plugin.py:145-147`), so an equal-ranked later size never overwrites an
/// earlier winner. This matters concretely: `Small` ties `SmallEn`, but
/// `SmallEn` is English-only and would make detection unable to ever return
/// anything else. A plain `Iterator::max_by_key` would pick the *last* tied
/// element instead (review fix, Important 1) — hence the explicit
/// strict-greater-than fold below rather than `max_by_key`.
pub fn select_model(model_root: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for size in CANDIDATE_SIZES.iter().copied() {
        let Some(path) = registry::local_model_path(model_root, size) else {
            continue;
        };
        let rank = size_rank(size);
        let is_new_best = match &best {
            Some((best_rank, _)) => rank > *best_rank,
            None => true,
        };
        if is_new_best {
            best = Some((rank, path));
        }
    }
    best.map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_detect_true_when_language_unset() {
        assert!(should_detect(None));
    }

    #[test]
    fn should_detect_true_when_language_explicitly_english() {
        assert!(should_detect(Language::parse("en")));
    }

    #[test]
    fn should_detect_false_when_language_is_other() {
        assert!(!should_detect(Language::parse("fr")));
        assert!(!should_detect(Language::parse("de")));
    }

    #[test]
    fn should_detect_false_when_disabled_is_the_caller_s_own_gate() {
        // `should_detect` only implements Buzz's en-quirk; the
        // `language_detection.enabled` Preferences gate is a separate check
        // the caller (`Runner::run_job`) makes on `PostprocConfig` before
        // ever consulting this function. Documented here so the two gates
        // are never confused for one another.
        assert!(should_detect(None));
    }

    fn touch_model(root: &Path, size: ModelSize) {
        std::fs::create_dir_all(root).expect("create model root");
        let name = registry::ggml_file_name(size);
        std::fs::write(root.join(&name), b"fake model bytes").expect("write model");
        std::fs::write(
            root.join(format!("{name}{}", registry::SENTINEL_SUFFIX)),
            b"",
        )
        .expect("write sentinel");
    }

    #[test]
    fn select_model_prefers_largest_downloaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch_model(dir.path(), ModelSize::Tiny);
        touch_model(dir.path(), ModelSize::Small);

        assert_eq!(
            select_model(dir.path()),
            registry::local_model_path(dir.path(), ModelSize::Small)
        );
    }

    /// Review fix (Important 1): `Small` and `SmallEn` share a rank (both
    /// 460 in [`size_rank`]); Buzz's strict `>` comparison keeps the first
    /// declared (`Small`), not the last. A naive `Iterator::max_by_key`
    /// would have picked `SmallEn` instead — an English-only model that
    /// could never let detection return anything but English.
    #[test]
    fn select_model_tie_prefers_first_declared_size_over_later_equal_rank() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch_model(dir.path(), ModelSize::Small);
        touch_model(dir.path(), ModelSize::SmallEn);

        assert_eq!(
            select_model(dir.path()),
            registry::local_model_path(dir.path(), ModelSize::Small),
            "Small and SmallEn tie in rank; the first-declared size (Small) \
             must win, not the English-only SmallEn"
        );
    }

    /// Same tie-break rule, in the `Large` family (`Large`/`LargeV2`/
    /// `LargeV3` all rank 2870): `Large` is declared first and must win.
    #[test]
    fn select_model_large_family_tie_prefers_large_over_large_v3() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch_model(dir.path(), ModelSize::LargeV3);
        touch_model(dir.path(), ModelSize::Large);

        assert_eq!(
            select_model(dir.path()),
            registry::local_model_path(dir.path(), ModelSize::Large)
        );
    }

    #[test]
    fn select_model_skips_custom_and_lumii() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch_model(dir.path(), ModelSize::Custom);
        touch_model(dir.path(), ModelSize::Lumii);

        assert_eq!(
            select_model(dir.path()),
            None,
            "custom/lumii are the only downloaded models, so nothing qualifies"
        );

        // A real candidate size still wins once one exists alongside them.
        touch_model(dir.path(), ModelSize::Tiny);
        assert_eq!(
            select_model(dir.path()),
            registry::local_model_path(dir.path(), ModelSize::Tiny)
        );
    }

    #[test]
    fn select_model_none_when_nothing_downloaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(select_model(dir.path()), None);
    }

    #[test]
    fn select_model_none_when_file_present_but_sentinel_missing() {
        // An interrupted download: the file exists but not its sentinel —
        // `registry::local_model_path` (and therefore `select_model`) must
        // not treat it as downloaded.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(registry::ggml_file_name(ModelSize::Small)),
            b"partial",
        )
        .expect("write partial model");
        assert_eq!(select_model(dir.path()), None);
    }
}
