//! Model registry: repo/URL/filename mapping and local completeness checks
//! for the humm-owned flat model cache (`docs/design/ARCHITECTURE.md` §5).
//!
//! Ported from Buzz's repo-id and filename conventions
//! (`buzz/model_loader.py:491-527`, `670-738`), deliberately adapted to
//! humm's flat on-disk layout rather than Buzz's `huggingface_hub` snapshot
//! layout — the parity behavior being preserved is the sentinel/resume/
//! SHA256 semantics (F-043), not the directory shape.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::domain::ModelSize;
use crate::error::ModelError;

/// The default HuggingFace base URL every `*_url` fn resolves against.
/// [`crate::models::download::ModelDownloader::new`] uses this; tests and
/// mirrors point [`crate::models::download::ModelDownloader::with_hf_base`]
/// at a different base instead (M2 F-043 downloader HTTP seam).
pub const HF_BASE: &str = "https://huggingface.co";

/// HuggingFace repo id for the mainline whisper.cpp ggml models
/// (`model_loader.py:491`).
pub const WHISPER_CPP_REPO: &str = "ggerganov/whisper.cpp";

/// HuggingFace repo id for the Lumii (Latvian) whisper.cpp model
/// (`model_loader.py:492`).
pub const LUMII_REPO: &str = "RaivisDejus/whisper.cpp-lv";

/// File name of the Silero VAD model whisper.cpp expects next to its binary
/// (`buzz/transcriber/whisper_cpp.py:184`).
pub const VAD_MODEL_FILE: &str = "ggml-silero-v6.2.0.bin";

/// HuggingFace repo id hosting the Silero VAD model, used both for the
/// download URL and the SHA256-digest lookup.
pub const VAD_MODEL_REPO: &str = "ggml-org/whisper-vad";

/// Download URL for the Silero VAD model, at the default [`HF_BASE`]
/// (equal to `vad_model_url_with_base(HF_BASE)`; kept as a const for
/// callers that just want the default URL without an `impl Into<String>`
/// round-trip).
pub const VAD_MODEL_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";

/// Suffix marking a fully-downloaded, verified artifact.
///
/// For a file artifact it is appended to the file name to form a sibling
/// marker file (e.g. `ggml-small.bin.humm_complete`). For a directory
/// artifact (the extracted CoreML encoder) it is instead used verbatim as a
/// marker file's name *inside* the directory. Buzz's equivalent is
/// `.buzz_complete` (`model_loader.py:126`).
pub const SENTINEL_SUFFIX: &str = ".humm_complete";

/// Suffix marking a *negative* cache entry for the CoreML encoder: a sibling
/// marker file (e.g. `ggml-small-encoder.mlmodelc.humm_no_coreml`) recording
/// that upstream publishes no CoreML encoder for this model (the encoder zip
/// URL returned HTTP 404, as it does for the Lumii repo). Its presence turns
/// "encoder absent" from an unknown state — which would re-probe the network
/// on every run — into a *known* one, so the model fast path can treat the
/// ggml as fully resolved without CoreML.
pub const NO_COREML_SUFFIX: &str = ".humm_no_coreml";

/// The ggml model file name whisper.cpp expects for `size`:
/// `ggml-{ggml_name}.bin` (`model_loader.py:504`).
pub fn ggml_file_name(size: ModelSize) -> String {
    format!("ggml-{}.bin", size.ggml_name())
}

/// The HuggingFace repo id hosting `size`'s whisper.cpp files
/// (`model_loader.py:499-502`).
///
/// `Custom` has no fixed repo — a custom model is a user-supplied ggml URL,
/// fetched directly by
/// [`crate::models::download::ModelDownloader::ensure_custom_model`], never
/// by repo-and-file-name lookup — so this returns [`ModelError::Unsupported`]
/// for any caller (`ggml_url`, `coreml_zip_url`, `ensure_ggml`) that tries to
/// resolve `Custom` through the repo-hosted path instead.
pub(crate) fn repo_for(size: ModelSize) -> Result<&'static str, ModelError> {
    match size {
        ModelSize::Custom => Err(ModelError::Unsupported {
            detail: "the custom model size has no download repo — custom models are fetched \
                     from the models.custom_model_url setting"
                .to_string(),
        }),
        ModelSize::Lumii => Ok(LUMII_REPO),
        _ => Ok(WHISPER_CPP_REPO),
    }
}

/// The cache file name for a user-supplied custom ggml model (`-s custom`,
/// `model_loader.py:495-497`'s `ggml-model-whisper-custom.bin` equivalent in
/// humm's flat layout): `"ggml-custom.bin"`.
///
/// Deliberately equal to `ggml_file_name(ModelSize::Custom)` — a custom model
/// occupies the same "custom slot" in the cache whichever name a caller
/// reaches for, so the two must never drift apart (see the
/// `custom_model_file_name_matches_ggml_file_name_for_custom_size` test).
/// [`crate::models::download::ModelDownloader::ensure_custom_model`] uses
/// this name; there is no fixed upstream repo for a custom model; see
/// [`repo_for`].
pub fn custom_model_file_name() -> &'static str {
    "ggml-custom.bin"
}

/// The cache file name for a resolved third-party Hugging Face Whisper-family
/// repo's ggml model (`-m huggingface --hfid <repo>`, approved Q-2 scope):
/// `"ggml-hf-" + repo, with '/' replaced by '--', + ".bin"`.
///
/// Repo ids are namespaced (`owner/name`); replacing the separator keeps the
/// result a single flat file name in humm's cache (`docs/design/ARCHITECTURE.md`
/// §5) while staying legible and, in practice, collision-free across distinct
/// repos (HF repo ids are restricted to `[A-Za-z0-9._-]` segments, so no
/// other character needs sanitizing).
pub fn hf_repo_model_file_name(repo: &str) -> String {
    format!("ggml-hf-{}.bin", repo.replace('/', "--"))
}

/// Recognizes an `openai/whisper-<name>` repo id whose `<name>` maps to a
/// known [`ModelSize`] (e.g. `openai/whisper-tiny`, `openai/whisper-large`,
/// `openai/whisper-large-v3-turbo`) — the canonical OpenAI Whisper repos,
/// whose ggml equivalents are the ordinary whisper.cpp/lumii sizes this
/// registry already resolves (approved Q-2 "fetch ggml equivalents"
/// mechanics).
///
/// Only the exact `openai/` prefix is recognized: a third-party mirror or
/// fork (e.g. `someone-else/whisper-tiny`) is a different repo with no
/// guaranteed ggml equivalent, and falls through to
/// [`crate::models::download::ModelDownloader::resolve_hf_repo`]'s generic
/// repo-file-listing path instead.
///
/// `Custom` is deliberately excluded even though `"custom"` parses as a
/// [`ModelSize`]: it has no fixed repo (see [`repo_for`]), so an
/// `openai/whisper-custom` repo id must fall through to the generic
/// repo-file-listing path too, instead of hitting `repo_for`'s
/// [`ModelError::Unsupported`] via this mapping.
pub fn openai_whisper_model_size(repo: &str) -> Option<ModelSize> {
    let size = repo
        .strip_prefix("openai/whisper-")
        .and_then(|name| ModelSize::from_str(name).ok())?;
    if size == ModelSize::Custom {
        None
    } else {
        Some(size)
    }
}

/// The download URL for `size`'s ggml model file at a given `base`
/// (`model_loader.py:495-517`). [`ggml_url`] is this at the default
/// [`HF_BASE`].
pub fn ggml_url_with_base(base: &str, size: ModelSize) -> Result<String, ModelError> {
    let repo = repo_for(size)?;
    Ok(format!(
        "{base}/{repo}/resolve/main/{}",
        ggml_file_name(size)
    ))
}

/// The download URL for `size`'s ggml model file
/// (`model_loader.py:495-517`).
pub fn ggml_url(size: ModelSize) -> Result<String, ModelError> {
    ggml_url_with_base(HF_BASE, size)
}

/// The download URL for `size`'s CoreML encoder zip at a given `base`
/// (`model_loader.py:692`). [`coreml_zip_url`] is this at the default
/// [`HF_BASE`].
pub fn coreml_zip_url_with_base(base: &str, size: ModelSize) -> Result<String, ModelError> {
    let repo = repo_for(size)?;
    Ok(format!(
        "{base}/{repo}/resolve/main/ggml-{}-encoder.mlmodelc.zip",
        size.ggml_name()
    ))
}

/// The download URL for `size`'s CoreML encoder zip
/// (`model_loader.py:692`).
pub fn coreml_zip_url(size: ModelSize) -> Result<String, ModelError> {
    coreml_zip_url_with_base(HF_BASE, size)
}

/// The download URL for the Silero VAD model at a given `base`.
/// [`VAD_MODEL_URL`] is this at the default [`HF_BASE`].
pub fn vad_model_url_with_base(base: &str) -> String {
    format!("{base}/{VAD_MODEL_REPO}/resolve/main/{VAD_MODEL_FILE}")
}

/// The path to `size`'s locally cached ggml file, if fully downloaded.
///
/// `Some` iff both `<root>/<ggml_file_name>` and its
/// `<ggml_file_name>.humm_complete` sibling exist (flat humm-owned layout,
/// `docs/design/ARCHITECTURE.md` §5).
pub fn local_model_path(root: &Path, size: ModelSize) -> Option<PathBuf> {
    complete_file_path(root, &ggml_file_name(size))
}

/// The path to the locally cached Silero VAD model, if fully downloaded.
/// Same sentinel rule as [`local_model_path`].
pub fn local_vad_model_path(root: &Path) -> Option<PathBuf> {
    complete_file_path(root, VAD_MODEL_FILE)
}

/// The directory `size`'s extracted CoreML encoder occupies in the cache
/// (`ggml-{ggml_name}-encoder.mlmodelc`, `model_loader.py:692`).
pub fn coreml_encoder_dir(root: &Path, size: ModelSize) -> PathBuf {
    root.join(format!("ggml-{}-encoder.mlmodelc", size.ggml_name()))
}

/// The negative-cache marker path for `size`'s CoreML encoder: the encoder
/// directory's path with [`NO_COREML_SUFFIX`] appended to its name.
pub fn coreml_no_encoder_marker(root: &Path, size: ModelSize) -> PathBuf {
    root.join(format!(
        "ggml-{}-encoder.mlmodelc{NO_COREML_SUFFIX}",
        size.ggml_name()
    ))
}

/// The three distinguishable on-disk states of `size`'s CoreML encoder,
/// letting the model fast path tell "not yet attempted" (`Missing`) apart
/// from "upstream has none" (`KnownAbsent`) — the distinction whose absence
/// let a failed first-run CoreML step degrade every later run silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoremlState {
    /// The encoder directory exists with its in-directory completeness
    /// sentinel — ready to use.
    Complete,
    /// A negative-cache marker records that upstream publishes no encoder
    /// (HTTP 404); the ggml is complete on its own, no retry needed.
    KnownAbsent,
    /// Neither a complete encoder nor a negative marker exists: the encoder
    /// has not been (successfully) fetched yet, and should be attempted when
    /// the caller wants CoreML.
    Missing,
}

/// Classifies `size`'s CoreML encoder into a [`CoremlState`] by inspecting the
/// cache at `root`. `Complete` takes precedence over `KnownAbsent` (a real
/// extracted encoder wins over a stale negative marker, should both exist).
pub fn coreml_state(root: &Path, size: ModelSize) -> CoremlState {
    if coreml_encoder_dir(root, size)
        .join(SENTINEL_SUFFIX)
        .is_file()
    {
        CoremlState::Complete
    } else if coreml_no_encoder_marker(root, size).is_file() {
        CoremlState::KnownAbsent
    } else {
        CoremlState::Missing
    }
}

fn complete_file_path(root: &Path, file_name: &str) -> Option<PathBuf> {
    let final_path = root.join(file_name);
    let sentinel = sentinel_path_for(&final_path);
    if final_path.is_file() && sentinel.is_file() {
        Some(final_path)
    } else {
        None
    }
}

/// Subdirectory of `model_root()` holding diarization model artifacts
/// (F-087, ADR-024) — kept apart from the flat whisper.cpp/VAD files so a
/// user can clear one feature's cache without disturbing the other.
pub const DIARIZE_DIR: &str = "diarization";

/// HuggingFace repo id hosting the `speakrs` diarization pipeline's
/// ONNX/PLDA artifacts (`validation/m8-spikes/README.md` Spike B "Model
/// artifacts"), used both for the download URL and the SHA256-digest
/// lookup — the same `tree/main` API the whisper.cpp/VAD downloads already
/// use ([`crate::models::download::ModelDownloader`]'s `expected_sha256`),
/// since this repo publishes standard Git LFS pointers (confirmed for 9 of
/// the 10 files below; the tenth, [`DIARIZE_EMBEDDING_MIN_SAMPLES_FILE`], is
/// a plain 4-byte blob below HF's LFS threshold and so publishes no
/// `lfs.oid` — [`crate::models::download::ModelDownloader::verify_or_warn`]'s
/// existing "no digest published, skip verification" fallback already
/// handles that file gracefully, with no special-casing needed here).
pub const DIARIZE_MODEL_REPO: &str = "avencera/speakrs-models";

/// Segmentation model file name (`speakrs::ExecutionMode::Cpu`).
pub const DIARIZE_SEGMENTATION_FILE: &str = "segmentation-3.0.onnx";
/// Speaker-embedding model file name.
pub const DIARIZE_EMBEDDING_FILE: &str = "wespeaker-voxceleb-resnet34.onnx";
/// External-data sibling of [`DIARIZE_EMBEDDING_FILE`] (the ONNX model's
/// weights, split out by the exporter).
pub const DIARIZE_EMBEDDING_DATA_FILE: &str = "wespeaker-voxceleb-resnet34.onnx.data";
/// PLDA LDA-transform parameter file.
pub const DIARIZE_PLDA_LDA_FILE: &str = "plda_lda.npy";
/// PLDA total-variability-transform parameter file.
pub const DIARIZE_PLDA_TR_FILE: &str = "plda_tr.npy";
/// PLDA mean (mu) parameter file.
pub const DIARIZE_PLDA_MU_FILE: &str = "plda_mu.npy";
/// PLDA psi (within/between-class covariance) parameter file.
pub const DIARIZE_PLDA_PSI_FILE: &str = "plda_psi.npy";
/// PLDA first-mean parameter file.
pub const DIARIZE_PLDA_MEAN1_FILE: &str = "plda_mean1.npy";
/// PLDA second-mean parameter file.
pub const DIARIZE_PLDA_MEAN2_FILE: &str = "plda_mean2.npy";
/// Minimum-sample-count sidecar for [`DIARIZE_EMBEDDING_FILE`] (a 4-byte
/// text file; not LFS-tracked — see [`DIARIZE_MODEL_REPO`]'s doc).
pub const DIARIZE_EMBEDDING_MIN_SAMPLES_FILE: &str =
    "wespeaker-voxceleb-resnet34.min_num_samples.txt";

/// Every diarization model file `speakrs::ExecutionMode::Cpu` needs
/// (`validation/m8-spikes/README.md` Spike B "Model artifacts" table, 10
/// files, 59.75 MB total — `59,751,799` bytes exactly, per `DECISIONS.md`
/// ADR-024's corrected figure), in the order [`ModelDownloader::ensure_diarization_models`]
/// fetches them and [`diarization_model_paths`] resolves them.
///
/// [`ModelDownloader::ensure_diarization_models`]: crate::models::download::ModelDownloader::ensure_diarization_models
pub const DIARIZE_MODEL_FILES: [&str; 10] = [
    DIARIZE_SEGMENTATION_FILE,
    DIARIZE_EMBEDDING_FILE,
    DIARIZE_EMBEDDING_DATA_FILE,
    DIARIZE_PLDA_LDA_FILE,
    DIARIZE_PLDA_TR_FILE,
    DIARIZE_PLDA_MU_FILE,
    DIARIZE_PLDA_PSI_FILE,
    DIARIZE_PLDA_MEAN1_FILE,
    DIARIZE_PLDA_MEAN2_FILE,
    DIARIZE_EMBEDDING_MIN_SAMPLES_FILE,
];

/// The download URL for one diarization model `file_name` at a given `base`
/// (standard HF `resolve/main` URL, public repo, no auth —
/// `validation/m8-spikes/README.md` Spike B). Mirrors
/// [`vad_model_url_with_base`]'s single-repo templating.
pub fn diarize_model_url_with_base(base: &str, file_name: &str) -> String {
    format!("{base}/{DIARIZE_MODEL_REPO}/resolve/main/{file_name}")
}

/// Local paths to the ensured diarization model artifacts (F-087),
/// resolved under `model_root()/`[`DIARIZE_DIR`].
///
/// All 10 files sit flat in [`Self::dir`] with no subdirectories or
/// manifest (confirmed in the spike: `speakrs::models::ModelBundle::from_dir`
/// just joins each fixed file name onto one directory) — `humm-diarize`'s
/// `OrtDiarizer` hands [`Self::dir`] straight to
/// `speakrs::OwnedDiarizationPipeline::from_dir`. The individual file
/// fields exist so callers (the downloader, and `OrtDiarizer::new`'s
/// existence check) can address/validate each file without restating its
/// name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiarizationModelPaths {
    /// The directory containing every file below, flat.
    pub dir: PathBuf,
    /// Path to [`DIARIZE_SEGMENTATION_FILE`].
    pub segmentation: PathBuf,
    /// Path to [`DIARIZE_EMBEDDING_FILE`].
    pub embedding: PathBuf,
    /// Path to [`DIARIZE_EMBEDDING_DATA_FILE`].
    pub embedding_data: PathBuf,
    /// Path to [`DIARIZE_PLDA_LDA_FILE`].
    pub plda_lda: PathBuf,
    /// Path to [`DIARIZE_PLDA_TR_FILE`].
    pub plda_tr: PathBuf,
    /// Path to [`DIARIZE_PLDA_MU_FILE`].
    pub plda_mu: PathBuf,
    /// Path to [`DIARIZE_PLDA_PSI_FILE`].
    pub plda_psi: PathBuf,
    /// Path to [`DIARIZE_PLDA_MEAN1_FILE`].
    pub plda_mean1: PathBuf,
    /// Path to [`DIARIZE_PLDA_MEAN2_FILE`].
    pub plda_mean2: PathBuf,
    /// Path to [`DIARIZE_EMBEDDING_MIN_SAMPLES_FILE`].
    pub embedding_min_samples: PathBuf,
}

impl DiarizationModelPaths {
    /// Every `(file name, resolved path)` pair, in [`DIARIZE_MODEL_FILES`]
    /// order — used to iterate the download set and to check/report which
    /// specific file(s) are missing.
    pub fn files(&self) -> [(&'static str, &Path); 10] {
        [
            (DIARIZE_SEGMENTATION_FILE, &self.segmentation),
            (DIARIZE_EMBEDDING_FILE, &self.embedding),
            (DIARIZE_EMBEDDING_DATA_FILE, &self.embedding_data),
            (DIARIZE_PLDA_LDA_FILE, &self.plda_lda),
            (DIARIZE_PLDA_TR_FILE, &self.plda_tr),
            (DIARIZE_PLDA_MU_FILE, &self.plda_mu),
            (DIARIZE_PLDA_PSI_FILE, &self.plda_psi),
            (DIARIZE_PLDA_MEAN1_FILE, &self.plda_mean1),
            (DIARIZE_PLDA_MEAN2_FILE, &self.plda_mean2),
            (
                DIARIZE_EMBEDDING_MIN_SAMPLES_FILE,
                &self.embedding_min_samples,
            ),
        ]
    }
}

/// Resolves every diarization model file's local path under
/// `root/`[`DIARIZE_DIR`] (flat, no completeness check — see
/// [`local_model_path`]'s sentinel-checked equivalent for the whisper.cpp
/// case; here [`ModelDownloader::ensure_diarization_models`] is the sole
/// writer and always ensures every file before returning, so a bare
/// existence check is what [`crate::diarize`]'s adapter needs).
///
/// [`ModelDownloader::ensure_diarization_models`]: crate::models::download::ModelDownloader::ensure_diarization_models
pub fn diarization_model_paths(root: &Path) -> DiarizationModelPaths {
    let dir = root.join(DIARIZE_DIR);
    DiarizationModelPaths {
        segmentation: dir.join(DIARIZE_SEGMENTATION_FILE),
        embedding: dir.join(DIARIZE_EMBEDDING_FILE),
        embedding_data: dir.join(DIARIZE_EMBEDDING_DATA_FILE),
        plda_lda: dir.join(DIARIZE_PLDA_LDA_FILE),
        plda_tr: dir.join(DIARIZE_PLDA_TR_FILE),
        plda_mu: dir.join(DIARIZE_PLDA_MU_FILE),
        plda_psi: dir.join(DIARIZE_PLDA_PSI_FILE),
        plda_mean1: dir.join(DIARIZE_PLDA_MEAN1_FILE),
        plda_mean2: dir.join(DIARIZE_PLDA_MEAN2_FILE),
        embedding_min_samples: dir.join(DIARIZE_EMBEDDING_MIN_SAMPLES_FILE),
        dir,
    }
}

/// The sentinel sibling path for a file artifact: `final_path` with
/// [`SENTINEL_SUFFIX`] appended to its file name.
pub(crate) fn sentinel_path_for(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(SENTINEL_SUFFIX);
    final_path.with_file_name(name)
}

/// The `.partial` working-file path for a file artifact
/// (`docs/superpowers/plans/2026-07-20-m1-headless-core-path.md` Task 5:
/// "one temp-file scheme").
pub(crate) fn partial_path_for(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".partial");
    final_path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn ggml_file_name_large_uses_v1_suffix() {
        assert_eq!(ggml_file_name(ModelSize::Large), "ggml-large-v1.bin");
    }

    #[test]
    fn ggml_file_name_small_matches_wire_value() {
        assert_eq!(ggml_file_name(ModelSize::Small), "ggml-small.bin");
    }

    #[test]
    fn ggml_url_small_is_exact() {
        assert_eq!(
            ggml_url(ModelSize::Small).unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
        );
    }

    #[test]
    fn ggml_url_lumii_uses_lumii_repo() {
        assert_eq!(
            ggml_url(ModelSize::Lumii).unwrap(),
            "https://huggingface.co/RaivisDejus/whisper.cpp-lv/resolve/main/ggml-lumii.bin"
        );
    }

    #[test]
    fn ggml_url_custom_is_unsupported() {
        let err = ggml_url(ModelSize::Custom).unwrap_err();
        assert!(matches!(err, ModelError::Unsupported { .. }));
        assert!(
            err.to_string()
                .contains("custom models are fetched from the models.custom_model_url setting")
        );
    }

    #[test]
    fn custom_model_file_name_is_ggml_custom_bin() {
        assert_eq!(custom_model_file_name(), "ggml-custom.bin");
    }

    #[test]
    fn custom_model_file_name_matches_ggml_file_name_for_custom_size() {
        // The two naming paths (the fixed "custom slot" name, and the
        // generic `ggml_file_name` used by every other `ModelSize`) must
        // never drift apart, or a caller mixing the two would silently
        // address two different files.
        assert_eq!(custom_model_file_name(), ggml_file_name(ModelSize::Custom));
    }

    #[test]
    fn hf_repo_model_file_name_replaces_slash_with_double_dash() {
        assert_eq!(
            hf_repo_model_file_name("acme/whisper-x"),
            "ggml-hf-acme--whisper-x.bin"
        );
    }

    #[test]
    fn hf_repo_model_file_name_leaves_non_slash_characters_alone() {
        assert_eq!(
            hf_repo_model_file_name("ggerganov/whisper.cpp"),
            "ggml-hf-ggerganov--whisper.cpp.bin"
        );
    }

    #[test]
    fn openai_whisper_model_size_maps_known_sizes() {
        assert_eq!(
            openai_whisper_model_size("openai/whisper-tiny"),
            Some(ModelSize::Tiny)
        );
        assert_eq!(
            openai_whisper_model_size("openai/whisper-large-v3-turbo"),
            Some(ModelSize::LargeV3Turbo)
        );
    }

    #[test]
    fn openai_whisper_model_size_maps_bare_large_to_v1() {
        // Buzz's `WhisperModelSize.LARGE` ("large") is whisper.cpp's
        // `large-v1` (`ggml_name`); the openai repo id for it is
        // `openai/whisper-large` (no version suffix).
        assert_eq!(
            openai_whisper_model_size("openai/whisper-large"),
            Some(ModelSize::Large)
        );
    }

    #[test]
    fn openai_whisper_model_size_none_for_unknown_name() {
        assert_eq!(openai_whisper_model_size("openai/whisper-bogus"), None);
    }

    #[test]
    fn openai_whisper_model_size_none_for_non_openai_prefix() {
        // Only the exact `openai/` prefix triggers the mapping — a
        // third-party mirror or fork falls through to the generic
        // repo-file-listing path instead.
        assert_eq!(openai_whisper_model_size("someone-else/whisper-tiny"), None);
    }

    #[test]
    fn openai_whisper_model_size_none_for_custom() {
        // `"custom"` parses as `ModelSize::Custom`, but `Custom` has no fixed
        // repo (`repo_for`) — this mapping must not route
        // `openai/whisper-custom` into `repo_for`'s `Unsupported` error;
        // it falls through to the generic repo-file-listing path instead.
        assert_eq!(openai_whisper_model_size("openai/whisper-custom"), None);
    }

    #[test]
    fn coreml_zip_url_small_is_exact() {
        assert_eq!(
            coreml_zip_url(ModelSize::Small).unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-encoder.mlmodelc.zip"
        );
    }

    #[test]
    fn ggml_url_with_base_swaps_the_host() {
        assert_eq!(
            ggml_url_with_base("http://127.0.0.1:9", ModelSize::Small).unwrap(),
            "http://127.0.0.1:9/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
        );
    }

    #[test]
    fn ggml_url_delegates_to_with_base_at_hf_base() {
        assert_eq!(
            ggml_url(ModelSize::Small).unwrap(),
            ggml_url_with_base(HF_BASE, ModelSize::Small).unwrap()
        );
    }

    #[test]
    fn coreml_zip_url_with_base_swaps_the_host() {
        assert_eq!(
            coreml_zip_url_with_base("http://127.0.0.1:9", ModelSize::Small).unwrap(),
            "http://127.0.0.1:9/ggerganov/whisper.cpp/resolve/main/ggml-small-encoder.mlmodelc.zip"
        );
    }

    #[test]
    fn coreml_zip_url_custom_is_unsupported() {
        assert!(matches!(
            coreml_zip_url(ModelSize::Custom).unwrap_err(),
            ModelError::Unsupported { .. }
        ));
    }

    #[test]
    fn vad_model_url_with_base_matches_default_const_at_hf_base() {
        assert_eq!(vad_model_url_with_base(HF_BASE), VAD_MODEL_URL);
    }

    #[test]
    fn vad_model_url_with_base_swaps_the_host() {
        assert_eq!(
            vad_model_url_with_base("http://127.0.0.1:9"),
            "http://127.0.0.1:9/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin"
        );
    }

    #[test]
    fn local_model_path_none_when_file_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("ggml-small.bin"), b"partial").expect("write model file");
        assert_eq!(local_model_path(dir.path(), ModelSize::Small), None);
    }

    #[test]
    fn local_model_path_some_when_file_and_sentinel_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_path = dir.path().join("ggml-small.bin");
        std::fs::write(&model_path, b"complete").expect("write model file");
        std::fs::write(dir.path().join("ggml-small.bin.humm_complete"), b"")
            .expect("write sentinel");
        assert_eq!(
            local_model_path(dir.path(), ModelSize::Small),
            Some(model_path)
        );
    }

    #[test]
    fn local_model_path_none_when_neither_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(local_model_path(dir.path(), ModelSize::Small), None);
    }

    #[test]
    fn local_vad_model_path_none_when_sentinel_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(VAD_MODEL_FILE), b"partial").expect("write vad file");
        assert_eq!(local_vad_model_path(dir.path()), None);
    }

    #[test]
    fn local_vad_model_path_some_when_complete() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vad_path = dir.path().join(VAD_MODEL_FILE);
        std::fs::write(&vad_path, b"complete").expect("write vad file");
        std::fs::write(
            dir.path()
                .join(format!("{VAD_MODEL_FILE}{SENTINEL_SUFFIX}")),
            b"",
        )
        .expect("write sentinel");
        assert_eq!(local_vad_model_path(dir.path()), Some(vad_path));
    }

    #[test]
    fn sentinel_path_for_appends_suffix() {
        let final_path = Path::new("/root/ggml-small.bin");
        assert_eq!(
            sentinel_path_for(final_path),
            Path::new("/root/ggml-small.bin.humm_complete")
        );
    }

    #[test]
    fn partial_path_for_appends_partial() {
        let final_path = Path::new("/root/ggml-small.bin");
        assert_eq!(
            partial_path_for(final_path),
            Path::new("/root/ggml-small.bin.partial")
        );
    }

    #[test]
    fn coreml_no_encoder_marker_is_sibling_of_encoder_dir() {
        let root = Path::new("/root");
        assert_eq!(
            coreml_no_encoder_marker(root, ModelSize::Small),
            Path::new("/root/ggml-small-encoder.mlmodelc.humm_no_coreml")
        );
    }

    #[test]
    fn coreml_state_missing_when_nothing_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            coreml_state(dir.path(), ModelSize::Small),
            CoremlState::Missing
        );
    }

    #[test]
    fn coreml_state_missing_when_dir_present_but_no_sentinel() {
        // An interrupted extraction leaves the directory without its
        // in-directory completeness sentinel — still `Missing`, so a retry
        // is attempted.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(coreml_encoder_dir(dir.path(), ModelSize::Small))
            .expect("mkdir encoder dir");
        assert_eq!(
            coreml_state(dir.path(), ModelSize::Small),
            CoremlState::Missing
        );
    }

    #[test]
    fn coreml_state_complete_when_dir_and_in_dir_sentinel_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let encoder_dir = coreml_encoder_dir(dir.path(), ModelSize::Small);
        std::fs::create_dir(&encoder_dir).expect("mkdir encoder dir");
        std::fs::write(encoder_dir.join(SENTINEL_SUFFIX), b"").expect("write in-dir sentinel");
        assert_eq!(
            coreml_state(dir.path(), ModelSize::Small),
            CoremlState::Complete
        );
    }

    #[test]
    fn coreml_state_known_absent_when_negative_marker_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(coreml_no_encoder_marker(dir.path(), ModelSize::Small), b"")
            .expect("write negative marker");
        assert_eq!(
            coreml_state(dir.path(), ModelSize::Small),
            CoremlState::KnownAbsent
        );
    }

    #[test]
    fn coreml_state_complete_wins_over_negative_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let encoder_dir = coreml_encoder_dir(dir.path(), ModelSize::Small);
        std::fs::create_dir(&encoder_dir).expect("mkdir encoder dir");
        std::fs::write(encoder_dir.join(SENTINEL_SUFFIX), b"").expect("write in-dir sentinel");
        std::fs::write(coreml_no_encoder_marker(dir.path(), ModelSize::Small), b"")
            .expect("write negative marker");
        assert_eq!(
            coreml_state(dir.path(), ModelSize::Small),
            CoremlState::Complete
        );
    }

    // -- F-087 diarization registry (M8) -----------------------------------

    #[test]
    fn diarize_model_files_has_ten_entries_matching_the_spike() {
        // `validation/m8-spikes/README.md` Spike B "Model artifacts": exactly
        // 10 files for `speakrs::ExecutionMode::Cpu`.
        assert_eq!(DIARIZE_MODEL_FILES.len(), 10);
        assert_eq!(DIARIZE_MODEL_FILES[0], "segmentation-3.0.onnx");
        assert_eq!(DIARIZE_MODEL_FILES[1], "wespeaker-voxceleb-resnet34.onnx");
        assert_eq!(
            DIARIZE_MODEL_FILES[2],
            "wespeaker-voxceleb-resnet34.onnx.data"
        );
        assert_eq!(DIARIZE_MODEL_FILES[9], DIARIZE_EMBEDDING_MIN_SAMPLES_FILE);
    }

    #[test]
    fn diarize_model_url_with_base_is_exact_at_hf_base() {
        assert_eq!(
            diarize_model_url_with_base(HF_BASE, DIARIZE_SEGMENTATION_FILE),
            "https://huggingface.co/avencera/speakrs-models/resolve/main/segmentation-3.0.onnx"
        );
    }

    #[test]
    fn diarize_model_url_with_base_swaps_the_host() {
        assert_eq!(
            diarize_model_url_with_base("http://127.0.0.1:9", DIARIZE_PLDA_LDA_FILE),
            "http://127.0.0.1:9/avencera/speakrs-models/resolve/main/plda_lda.npy"
        );
    }

    #[test]
    fn diarization_model_paths_dir_is_root_join_diarize_dir() {
        let root = Path::new("/root");
        let paths = diarization_model_paths(root);
        assert_eq!(paths.dir, Path::new("/root/diarization"));
    }

    #[test]
    fn diarization_model_paths_joins_every_file_under_dir() {
        let root = Path::new("/root");
        let paths = diarization_model_paths(root);
        assert_eq!(
            paths.segmentation,
            Path::new("/root/diarization/segmentation-3.0.onnx")
        );
        assert_eq!(
            paths.embedding,
            Path::new("/root/diarization/wespeaker-voxceleb-resnet34.onnx")
        );
        assert_eq!(
            paths.embedding_data,
            Path::new("/root/diarization/wespeaker-voxceleb-resnet34.onnx.data")
        );
        assert_eq!(paths.plda_lda, Path::new("/root/diarization/plda_lda.npy"));
        assert_eq!(paths.plda_tr, Path::new("/root/diarization/plda_tr.npy"));
        assert_eq!(paths.plda_mu, Path::new("/root/diarization/plda_mu.npy"));
        assert_eq!(paths.plda_psi, Path::new("/root/diarization/plda_psi.npy"));
        assert_eq!(
            paths.plda_mean1,
            Path::new("/root/diarization/plda_mean1.npy")
        );
        assert_eq!(
            paths.plda_mean2,
            Path::new("/root/diarization/plda_mean2.npy")
        );
        assert_eq!(
            paths.embedding_min_samples,
            Path::new("/root/diarization/wespeaker-voxceleb-resnet34.min_num_samples.txt")
        );
    }

    #[test]
    fn diarization_model_paths_files_lists_all_ten_in_registry_order() {
        let paths = diarization_model_paths(Path::new("/root"));
        let files = paths.files();
        assert_eq!(files.len(), 10);
        for (i, name) in DIARIZE_MODEL_FILES.iter().enumerate() {
            assert_eq!(files[i].0, *name);
        }
    }
}
