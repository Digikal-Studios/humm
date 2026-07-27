//! [`WhisperLangDetector`]: the whisper-rs implementation of
//! [`humm_core::boundary::LanguageDetector`] (F-114).
//!
//! Ported from Buzz's `enhanced_language_detection` plugin
//! (`buzz/plugins/enhanced_language_detection/plugin.py`), which shells out to
//! `whisper-cli --detect-language` — a fresh process per detection — and reads
//! back the printed language code (`buzz/transcriber/whisper_cpp.py:403-470`).
//! whisper-rs 0.16 exposes the same underlying whisper.cpp routine
//! (`whisper_lang_auto_detect_with_state`) as a library call instead of a
//! subprocess, so this adapter creates a **fresh** [`WhisperContext`] and
//! [`whisper_rs::WhisperState`] per [`LanguageDetector::detect`] call and drops
//! both at the end of it — the same cost shape as Buzz spawning and tearing
//! down a whole process each time, ported faithfully rather than cached (there
//! is no session concept for detection the way [`crate::engine::WhisperEngine`]
//! has one for live transcription).
//!
//! The gating (only unset/explicit-English tasks run this at all), model
//! selection (largest already-downloaded ggml, or a tiny-model download
//! fallback), and store/export write-back all live in
//! `humm_core::queue::Runner::run_job` and `humm_core::postproc::langdetect`
//! (Task 4) — this module is only the real detection call itself.

use std::path::Path;

use humm_core::boundary::{LanguageDetector, Pcm};
use humm_core::cancel::CancelToken;
use humm_core::error::EngineError;
use humm_core::language::Language;

use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::engine::{default_threads, init_logging};

/// Maps a whisper.cpp language id (as returned by
/// [`whisper_rs::WhisperState::lang_detect`]) to a [`Language`], via
/// [`whisper_rs::get_lang_str`]'s short code lookup.
///
/// `None` for any id whisper.cpp itself doesn't recognize (`-1`, the "no
/// detection" sentinel some callers might pass in error, or anything past
/// [`whisper_rs::get_lang_max_id`]) **and** for the vanishingly unlikely case
/// where whisper.cpp knows a code humm's own [`Language`] table doesn't (there
/// are none today — whisper.cpp's `g_lang` table and humm's `LANGUAGES` table
/// were both ported from the same upstream source and agree on all 100
/// entries — but `Language::parse` degrading to `None` rather than panicking
/// keeps that never a crash if the two ever drift). Either way this is never
/// an error: [`LanguageDetector::detect`]'s own contract is that an
/// unrecognized result means "detection ran but returned nothing usable",
/// exactly like Buzz's plugin treating a `None` return as "did not return a
/// language" (`plugin.py:93-95`).
pub(crate) fn map_lang_id(id: i32) -> Option<Language> {
    let code = whisper_rs::get_lang_str(id)?;
    Language::parse(code)
}

/// Detects the spoken language of already-decoded audio via whisper.cpp's
/// standalone language-detection routine (F-114).
///
/// Constructed once (`WhisperLangDetector::new`) and wired into
/// [`humm_core::queue::RunnerDeps::lang_detector`]; every [`Self::detect`]
/// call loads its own fresh [`WhisperContext`]/[`whisper_rs::WhisperState`]
/// from the `model` path the caller resolved (`postproc::langdetect::
/// select_model`/the tiny-download fallback, both in `Runner::run_job`) —
/// this type never caches a context between calls (module doc).
#[derive(Debug, Clone, Copy)]
pub struct WhisperLangDetector {
    /// `true` forces CPU-only inference (no Metal/CoreML) for every context
    /// this detector creates. The GUI's `services::init` computes this from
    /// the same two inputs `GuiEngineFactory::create_whisper` combines for
    /// the main transcription engine (`HUMM_FORCE_CPU` + `settings.models.
    /// force_cpu`) — but, unlike `create_whisper`, which re-reads the setting
    /// fresh on every engine construction so a Preferences checkbox flip is
    /// honored by the very next task with no restart, this detector is
    /// constructed exactly once at startup (this struct's own `new` takes a
    /// plain `bool`, not a live settings handle), so a force-CPU toggle only
    /// takes effect for language detection after a restart. Documented
    /// divergence, not a bug: `WhisperLangDetector::new(force_cpu: bool)`'s
    /// signature is a snapshot by design.
    force_cpu: bool,
}

impl WhisperLangDetector {
    /// Builds a detector that loads every context with GPU acceleration
    /// disabled when `force_cpu` is `true`, enabled otherwise (Metal/CoreML on
    /// Apple Silicon, plain CPU elsewhere — same acceleration rule
    /// [`crate::engine::WhisperEngine::load`] applies to its own `use_gpu`
    /// flag, just inverted here to match Buzz's `--no-gpu` framing,
    /// `whisper_cpp.py:464-466`).
    pub fn new(force_cpu: bool) -> Self {
        WhisperLangDetector { force_cpu }
    }
}

impl LanguageDetector for WhisperLangDetector {
    /// Detects `audio`'s spoken language using the ggml model at `model`.
    ///
    /// Loads a fresh context + state (checking `cancel` cooperatively before
    /// each blocking step, since none of `pcm_to_mel`/`lang_detect` accept an
    /// abort callback the way [`crate::engine::WhisperEngine::transcribe`]'s
    /// `full` call does), converts `audio` to a mel spectrogram, then runs
    /// whisper.cpp's language-auto-detect at offset `0` — Buzz's
    /// `--detect-language` runs the identical routine on the whole decoded
    /// file (`whisper_cpp.py:403-470`), so no windowing/offset choice is made
    /// here either. An empty `audio` short-circuits to `Ok(None)`
    /// (`pcm_to_mel`/whisper.cpp's own routine has nothing to detect from —
    /// same empty-input convention `WhisperEngine::transcribe` uses).
    ///
    /// # Errors
    /// [`EngineError::Canceled`] if `cancel` fires before or between steps;
    /// [`EngineError::ModelLoad`] if `model` can't be loaded;
    /// [`EngineError::Inference`] if state creation, `pcm_to_mel`, or
    /// `lang_detect` itself fails. Every error here is caught and logged by
    /// the caller (`Runner::run_job`'s `catch_unwind`-contained pre-hook,
    /// module doc) — detection failing never fails the task, it only leaves
    /// the language on auto.
    fn detect(
        &self,
        audio: &Pcm,
        model: &Path,
        cancel: &CancelToken,
    ) -> Result<Option<Language>, EngineError> {
        init_logging();

        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }
        if audio.is_empty() {
            return Ok(None);
        }

        let mut params = WhisperContextParameters::default();
        params.use_gpu(!self.force_cpu);

        let context =
            WhisperContext::new_with_params(model, params).map_err(|e| EngineError::ModelLoad {
                path: model.to_path_buf(),
                detail: e.to_string(),
            })?;

        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }

        let mut state = context.create_state().map_err(|e| EngineError::Inference {
            detail: format!("failed to create whisper state: {e}"),
        })?;

        let threads = default_threads() as usize;
        state
            .pcm_to_mel(audio.samples(), threads)
            .map_err(|e| EngineError::Inference {
                detail: format!("pcm_to_mel failed: {e}"),
            })?;

        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }

        let (lang_id, _probs) =
            state
                .lang_detect(0, threads)
                .map_err(|e| EngineError::Inference {
                    detail: format!("language detection failed: {e}"),
                })?;

        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }

        Ok(map_lang_id(lang_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- map_lang_id: boundary ids (pure, no model needed) --------------------

    #[test]
    fn map_lang_id_negative_one_is_none() {
        // whisper.cpp's own "not found" sentinel.
        assert_eq!(map_lang_id(-1), None);
    }

    #[test]
    fn map_lang_id_zero_is_english() {
        assert_eq!(map_lang_id(0), Language::parse("en"));
    }

    #[test]
    fn map_lang_id_max_id_is_the_last_language() {
        let max_id = whisper_rs::get_lang_max_id();
        let mapped = map_lang_id(max_id).expect("whisper.cpp's own max id must map to a language");
        // whisper.cpp's `g_lang` table and humm's `Language` table were both
        // ported from the same upstream whisper.cpp source and agree on
        // every entry (module doc) -- id 99 is Cantonese in both.
        assert_eq!(mapped.code(), whisper_rs::get_lang_str(max_id).unwrap());
    }

    #[test]
    fn map_lang_id_out_of_range_is_none() {
        let out_of_range = whisper_rs::get_lang_max_id() + 1;
        assert_eq!(map_lang_id(out_of_range), None);
    }

    // -- WhisperLangDetector::new ---------------------------------------------

    #[test]
    fn new_stores_the_force_cpu_flag() {
        assert!(WhisperLangDetector::new(true).force_cpu);
        assert!(!WhisperLangDetector::new(false).force_cpu);
    }

    // -- detect: empty-audio short-circuit (no model needed) -----------------

    #[test]
    fn detect_empty_audio_is_none_without_touching_the_model() {
        let detector = WhisperLangDetector::new(true);
        let audio = Pcm::new(Vec::new());
        // A nonexistent model path proves this never gets past the
        // empty-audio guard to attempt loading it.
        let result = detector.detect(
            &audio,
            Path::new("/no/such/ggml-model.bin"),
            &CancelToken::new(),
        );
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn detect_pre_canceled_token_errors_before_touching_the_model() {
        let detector = WhisperLangDetector::new(true);
        let audio = Pcm::new(vec![0.0_f32; 16_000]);
        let cancel = CancelToken::new();
        cancel.cancel();

        let result = detector.detect(&audio, Path::new("/no/such/ggml-model.bin"), &cancel);
        assert!(matches!(result, Err(EngineError::Canceled)));
    }

    #[test]
    fn detect_missing_model_is_model_load_error() {
        let detector = WhisperLangDetector::new(true);
        let audio = Pcm::new(vec![0.0_f32; 16_000]);
        let err = detector
            .detect(
                &audio,
                Path::new("/no/such/ggml-model.bin"),
                &CancelToken::new(),
            )
            .expect_err("missing model must error");
        assert!(matches!(err, EngineError::ModelLoad { .. }));
    }
}
