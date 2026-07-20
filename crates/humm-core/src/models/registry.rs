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

/// Download URL for the Silero VAD model.
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

/// The download URL for `size`'s ggml model file
/// (`model_loader.py:495-517`).
pub fn ggml_url(size: ModelSize) -> Result<String, ModelError> {
    let repo = repo_for(size)?;
    Ok(format!(
        "https://huggingface.co/{repo}/resolve/main/{}",
        ggml_file_name(size)
    ))
}

/// The download URL for `size`'s CoreML encoder zip
/// (`model_loader.py:692`).
pub fn coreml_zip_url(size: ModelSize) -> Result<String, ModelError> {
    let repo = repo_for(size)?;
    Ok(format!(
        "https://huggingface.co/{repo}/resolve/main/ggml-{}-encoder.mlmodelc.zip",
        size.ggml_name()
    ))
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
    fn coreml_zip_url_custom_is_unsupported() {
        assert!(matches!(
            coreml_zip_url(ModelSize::Custom).unwrap_err(),
            ModelError::Unsupported { .. }
        ));
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
}
