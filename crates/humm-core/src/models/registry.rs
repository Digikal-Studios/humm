//! Model registry: repo/URL/filename mapping and local completeness checks
//! for the humm-owned flat model cache (`docs/design/ARCHITECTURE.md` §5).
//!
//! Ported from Buzz's repo-id and filename conventions
//! (`buzz/model_loader.py:491-527`, `670-738`), deliberately adapted to
//! humm's flat on-disk layout rather than Buzz's `huggingface_hub` snapshot
//! layout — the parity behavior being preserved is the sentinel/resume/
//! SHA256 semantics (F-043), not the directory shape.

use std::path::{Path, PathBuf};

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
/// `Custom` has no fixed repo — humm doesn't yet support custom model URLs
/// (they arrive with M2), so this returns [`ModelError::Unsupported`].
pub(crate) fn repo_for(size: ModelSize) -> Result<&'static str, ModelError> {
    match size {
        ModelSize::Custom => Err(ModelError::Unsupported {
            detail: "custom model URLs arrive with M2".to_string(),
        }),
        ModelSize::Lumii => Ok(LUMII_REPO),
        _ => Ok(WHISPER_CPP_REPO),
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
        assert!(err.to_string().contains("custom model URLs arrive with M2"));
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
}
