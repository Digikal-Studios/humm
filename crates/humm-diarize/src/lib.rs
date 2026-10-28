//! `humm-diarize`: speaker diarization adapter implementing
//! `humm_core::diarize::Diarizer` via the `speakrs` crate (F-087, M8 design
//! spec §6, ADR-024).
//!
//! **Adapter isolation (Standing Policy):** this is the *only* crate in the
//! workspace naming `ort` or `speakrs` directly — `humm-core` knows only the
//! [`humm_core::diarize::Diarizer`] trait and its value types
//! ([`humm_core::diarize::SpeakerTurn`] etc.), never this crate or its
//! dependencies.
//!
//! **Crate pick and pin, per the spike (`validation/m8-spikes/README.md`
//! Spike B, ADR-024):** `speakrs` 0.5.0 over `pyannote-rs` 0.3.4 —
//! `pyannote-rs` fails to build against ADR-024's mandated exact `ort
//! =2.0.0-rc.12` pin (an upstream `ort::Error<SessionBuilder>`
//! `Send + Sync` regression between rc.10 and rc.12 breaks its `eyre`-based
//! error conversion). `Cargo.toml` pins `speakrs = "=0.5.0"` exactly, with
//! `default-features = false, features = ["openblas-system", "coreml"]`
//! (the crate's *default* features pull `ndarray-linalg`'s from-source
//! `openblas-static`, which fails to link on this machine's Homebrew
//! toolchain — `openblas-system` links Homebrew's prebuilt OpenBLAS instead;
//! macOS build prerequisite: `brew install openblas libomp`). Non-macOS
//! builds omit `coreml`: speakrs refuses to compile that feature off macOS.
//! macOS still uses the workspace feature set, `coreml` included.
//! **`online` deliberately dropped (task-9 review):** the spike's originally-recorded
//! feature set included `online`, but that was never a deliberate choice —
//! `online` gates only `speakrs::models::ModelManager`/`from_pretrained`'s
//! HuggingFace-download path, which this crate never calls (`OrtDiarizer`
//! uses `from_dir` exclusively, ungated by any feature). Dropping it removed
//! 20 transitive dependencies from `Cargo.lock` (`hf-hub`,
//! `cookie`/`cookie_store`, `hyper-tls`, `tokio-native-tls`, etc.) and was
//! verified to leave the workspace's shared `reqwest` purely `rustls`-based
//! (`cargo tree -e features --workspace` shows no `reqwest feature
//! "native-tls"` anywhere) — confirmed empirically, not assumed.
//!
//! **Resolved `ort` version, confirmed by the compiler (not just the spike's
//! notes) while implementing this task:** `cargo fetch`/`cargo build`
//! resolve `ort`/`ort-sys` to exactly `2.0.0-rc.12`, matching ADR-024's pin.
//! `ort-sys`'s build script fetches its prebuilt ONNX Runtime binary from
//! `cdn.pyke.io` at build time into `~/Library/Caches/ort.pyke.io/` (a
//! machine-global cache, outside any workspace/`CARGO_HOME` path) — the
//! first `cargo build -p humm-diarize` in a given environment needs the
//! sandbox lifted for that fetch (ADR-024's build note; confirmed: a plain
//! `cargo fetch` also needs the sandbox lifted for an unrelated reason —
//! unpacking one dependency's tarball hit a "no dotfiles" sandbox rule on a
//! `.vscode/settings.json` entry inside it, nothing to do with `ort`).
//!
//! **Execution mode scope for this task:** [`OrtDiarizer`] always runs
//! [`speakrs::ExecutionMode::Cpu`] — the mode the spike validated end-to-end
//! (2-speaker fixture sanity, ~3.8x realtime on this machine). `coreml`
//! compiles cleanly (this crate's pinned feature set includes it) but is not
//! wired up or benchmarked here; the spike explicitly left CoreML
//! benchmarking to a later task, and `speakrs`'s own published numbers
//! suggest it would be dramatically faster on Apple Silicon — a real
//! follow-up, not done in this task.
//!
//! **Model loading happens per [`Diarizer::diarize`] call, not once in
//! [`OrtDiarizer::new`]** — the same choice `humm-dfn`'s `DfNoiseReducer`
//! made for the same reason (see its module doc's "fresh model per call"
//! note): [`Diarizer::diarize`] takes `&self`, but `speakrs`'s
//! `OwnedDiarizationPipeline::run` needs `&mut self`; building a fresh
//! pipeline as a local inside `diarize` sidesteps that with no
//! `Mutex`/interior-mutability machinery, keeping [`OrtDiarizer`] trivially
//! `Send + Sync`. The spike measured this construction at ~69ms on the CPU
//! path, cheap enough to redo per call. This also happens to be exactly
//! where the [`humm_core::diarize::DiarizeStage::LoadingModels`] progress
//! stage is genuinely real: it is emitted immediately before the
//! `from_dir` call that does this loading.
//!
//! **Progress honesty (UX_SPEC: real, never cosmetic):** `speakrs`'s
//! `OwnedDiarizationPipeline::run` is a single monolithic call — segmentation,
//! embedding extraction, and clustering all happen inside it with no
//! intermediate callback or hook this crate can observe. So
//! [`Diarizer::diarize`] emits exactly two [`humm_core::diarize::DiarizeProgress`]
//! updates: [`humm_core::diarize::DiarizeStage::LoadingModels`] (before
//! `from_dir`) and [`humm_core::diarize::DiarizeStage::Segmenting`] (before
//! `run`), both with `fraction: None`. `Embedding` and `Clustering` exist in
//! the shared enum for the trait's sake (a future implementation, or a
//! future `speakrs` version exposing finer hooks, could emit them) but this
//! adapter never does — interpolating a fake fraction or synthesizing
//! Embedding/Clustering transitions with no underlying signal would be
//! exactly the "cosmetic progress" the UX spec rules out.
//!
//! **Cancellation granularity, honestly:** `cancel` is checked immediately
//! on entry, again right after model loading (before the heavy `run` call),
//! and again right after `run` returns. `speakrs::OwnedDiarizationPipeline::run`
//! exposes no mid-run hook, so a cancel that arrives *during* `run` is not
//! observed until that call returns — it resolves at the call boundary, not
//! mid-computation. The spike measured the whole pipeline at ~9.8s for the
//! 37.19s two-speaker fixture on CPU (RTF ≈ 0.26); that number is this
//! adapter's practical worst-case cancel-ack latency on comparable audio,
//! and the Task 11 GUI designer should budget for it (the cancel-ack story
//! for a multi-minute file is a fair question the GUI wiring, not this
//! crate, must answer — e.g. surfacing "cancelling…" while `run` still
//! finishes in the background).

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use humm_core::boundary::Pcm;
use humm_core::cancel::CancelToken;
use humm_core::diarize::{DiarizeError, DiarizeProgress, DiarizeStage, Diarizer, SpeakerTurn};
use humm_core::domain::Millis;
use humm_core::models::registry::DiarizationModelPaths;

use speakrs::{ExecutionMode, OwnedDiarizationPipeline};

/// Speaker diarization via an ort-backed segmentation + embedding +
/// clustering pipeline (`speakrs`, ADR-024). See the [module docs](self)
/// for the execution-mode, progress, and cancellation scope this
/// implementation covers.
#[derive(Debug)]
pub struct OrtDiarizer {
    models: DiarizationModelPaths,
}

impl OrtDiarizer {
    /// Validates that every diarization model file named by `models` exists
    /// on disk (a cheap existence check — no ONNX Runtime session is
    /// constructed here; see the [module docs](self) for why that happens
    /// per [`Diarizer::diarize`] call instead). Returns
    /// [`DiarizeError::ModelsMissing`], naming every missing file, if any
    /// are absent.
    ///
    /// # Errors
    /// [`DiarizeError::ModelsMissing`] naming the missing file name(s) and
    /// pointing at [`humm_core::models::download::ModelDownloader::ensure_diarization_models`]
    /// as the fix.
    ///
    /// This check only ever runs here, once, at construction — per the
    /// [module docs](self), the actual model load happens fresh inside every
    /// [`Diarizer::diarize`] call, not here. So a file that exists at
    /// construction time but is deleted before a later `diarize()` call
    /// (e.g. a user manually clearing the model cache mid-session) is *not*
    /// re-caught as `ModelsMissing`: `from_dir`/ONNX Runtime's own load
    /// failure surfaces as [`DiarizeError::Failed`] instead, with whatever
    /// message the underlying library gives for a missing file.
    pub fn new(models: DiarizationModelPaths) -> Result<Self, DiarizeError> {
        let missing: Vec<&str> = models
            .files()
            .into_iter()
            .filter(|(_, path)| !path.is_file())
            .map(|(name, _)| name)
            .collect();

        if !missing.is_empty() {
            return Err(DiarizeError::ModelsMissing {
                detail: format!(
                    "diarization model file(s) missing from {}: {} — run the diarization \
                     model download first (ModelDownloader::ensure_diarization_models)",
                    models.dir.display(),
                    missing.join(", ")
                ),
            });
        }

        Ok(OrtDiarizer { models })
    }
}

impl Diarizer for OrtDiarizer {
    fn diarize(
        &self,
        audio: &Pcm,
        progress: &mut dyn FnMut(DiarizeProgress),
        cancel: &CancelToken,
    ) -> Result<Vec<SpeakerTurn>, DiarizeError> {
        if cancel.is_canceled() {
            return Err(DiarizeError::Canceled);
        }

        progress(DiarizeProgress {
            stage: DiarizeStage::LoadingModels,
            fraction: None,
        });
        let mut pipeline = OwnedDiarizationPipeline::from_dir(&self.models.dir, ExecutionMode::Cpu)
            .map_err(|e| DiarizeError::Failed {
                detail: e.to_string(),
            })?;

        if cancel.is_canceled() {
            return Err(DiarizeError::Canceled);
        }

        progress(DiarizeProgress {
            stage: DiarizeStage::Segmenting,
            fraction: None,
        });
        let result = pipeline
            .run(audio.samples())
            .map_err(|e| DiarizeError::Failed {
                detail: e.to_string(),
            })?;

        if cancel.is_canceled() {
            return Err(DiarizeError::Canceled);
        }

        let mut turns = result
            .segments
            .iter()
            .map(to_speaker_turn)
            .collect::<Result<Vec<_>, _>>()?;
        // `Diarizer::diarize`'s contract is sorted-by-start output. `speakrs`'s
        // own `to_segments` already sorts its raw output, and empirically
        // `DiarizationResult.segments` comes back sorted too (see the
        // contract test) — but that is an observed property of today's
        // `speakrs` internals (its own docs make no ordering guarantee on
        // `DiarizationResult.segments` specifically), not a documented API
        // promise this crate can rely on across a future `speakrs` upgrade
        // or an inference-path change (e.g. the concurrent CoreML path).
        // Sorting here defensively costs nothing on already-sorted input and
        // makes the trait's guarantee actually held by this adapter, not
        // merely inherited by accident.
        turns.sort_by_key(|t| t.start.0);
        Ok(turns)
    }
}

/// Maps one `speakrs::Segment` (start/end seconds, `"SPEAKER_NN"` label)
/// to a [`SpeakerTurn`] (start/end [`Millis`], zero-based speaker index).
fn to_speaker_turn(segment: &speakrs::Segment) -> Result<SpeakerTurn, DiarizeError> {
    Ok(SpeakerTurn {
        start: seconds_to_millis(segment.start),
        end: seconds_to_millis(segment.end),
        speaker: parse_speaker_index(&segment.speaker)?,
    })
}

/// Converts fractional seconds to whole milliseconds, rounding to the
/// nearest millisecond (matching `humm-media`'s `probe.rs` duration
/// conversion convention, not truncation).
fn seconds_to_millis(seconds: f64) -> Millis {
    Millis((seconds * 1000.0).round() as i64)
}

/// Parses `speakrs`'s `"SPEAKER_00"`/`"SPEAKER_01"`/... labels into the
/// zero-based index `speakrs::segment::to_segments` encodes in them.
fn parse_speaker_index(label: &str) -> Result<usize, DiarizeError> {
    label
        .strip_prefix("SPEAKER_")
        .and_then(|n| n.parse::<usize>().ok())
        .ok_or_else(|| DiarizeError::Failed {
            detail: format!("unrecognized speaker label from speakrs: {label:?}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn paths_with_all_files_present(dir: &std::path::Path) -> DiarizationModelPaths {
        let paths = humm_core::models::registry::diarization_model_paths(dir);
        std::fs::create_dir_all(&paths.dir).expect("mkdir diarization dir");
        for (_, path) in paths.files() {
            std::fs::write(path, b"fake-model-bytes").expect("write fake model file");
        }
        paths
    }

    #[test]
    fn new_succeeds_when_every_file_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = paths_with_all_files_present(dir.path());
        assert!(OrtDiarizer::new(paths).is_ok());
    }

    #[test]
    fn new_reports_models_missing_naming_the_absent_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = paths_with_all_files_present(dir.path());
        // Remove two of the ten to prove the error names the specific
        // files, not just "something is missing".
        std::fs::remove_file(&paths.segmentation).expect("remove segmentation file");
        std::fs::remove_file(&paths.plda_mu).expect("remove plda_mu file");

        let err = OrtDiarizer::new(paths).expect_err("should report missing files");
        let DiarizeError::ModelsMissing { detail } = err else {
            panic!("expected ModelsMissing, got {err:?}");
        };
        assert!(
            detail.contains("segmentation-3.0.onnx"),
            "detail should name the missing segmentation file: {detail}"
        );
        assert!(
            detail.contains("plda_mu.npy"),
            "detail should name the missing plda_mu file: {detail}"
        );
        assert!(
            !detail.contains("wespeaker-voxceleb-resnet34.onnx.data"),
            "detail should not name a file that is actually present: {detail}"
        );
    }

    #[test]
    fn new_reports_models_missing_when_dir_does_not_exist_at_all() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = humm_core::models::registry::diarization_model_paths(dir.path());
        // Never created.
        let err = OrtDiarizer::new(paths).expect_err("should report missing files");
        assert!(matches!(err, DiarizeError::ModelsMissing { .. }));
    }

    #[test]
    fn seconds_to_millis_rounds_to_nearest() {
        assert_eq!(seconds_to_millis(1.5), Millis(1500));
        assert_eq!(seconds_to_millis(15.387), Millis(15387));
        assert_eq!(seconds_to_millis(0.0004), Millis(0));
        assert_eq!(seconds_to_millis(0.0006), Millis(1));
    }

    #[test]
    fn parse_speaker_index_parses_zero_padded_labels() {
        assert_eq!(parse_speaker_index("SPEAKER_00").unwrap(), 0);
        assert_eq!(parse_speaker_index("SPEAKER_01").unwrap(), 1);
        assert_eq!(parse_speaker_index("SPEAKER_12").unwrap(), 12);
    }

    #[test]
    fn parse_speaker_index_fails_on_unrecognized_label() {
        let err = parse_speaker_index("UNKNOWN").expect_err("should fail");
        assert!(matches!(err, DiarizeError::Failed { .. }));
    }

    #[test]
    fn to_speaker_turn_maps_start_end_and_speaker_index() {
        let segment = speakrs::Segment::new(15.387, 21.614, "SPEAKER_01");
        let turn = to_speaker_turn(&segment).expect("should map");
        assert_eq!(turn.start, Millis(15387));
        assert_eq!(turn.end, Millis(21614));
        assert_eq!(turn.speaker, 1);
    }
}
