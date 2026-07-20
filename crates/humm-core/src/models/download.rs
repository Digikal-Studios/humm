//! Model downloader: resumable, SHA256-verified downloads of whisper.cpp
//! ggml models, the optional CoreML encoder zip, and the Silero VAD model,
//! into the humm-owned flat cache (`docs/design/ARCHITECTURE.md` §5).
//!
//! Adapted from Buzz's download algorithm (`buzz/model_loader.py:800-1031`)
//! and CoreML zip handling (`670-738`), simplified to a single temp-file
//! scheme: the working file for artifact `<final>` is always
//! `<final>.partial`, whether starting fresh or resuming (Buzz sometimes
//! writes directly to the final path when resuming; humm never does). This
//! is a deliberate simplification of Buzz's on-disk mechanics — the
//! preserved parity behaviors are resume-via-Range, SHA256 verification
//! against upstream digests, and CoreML zip extraction (F-043).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::cancel::CancelToken;
use crate::domain::ModelSize;
use crate::error::ModelError;

use super::registry::{
    self, VAD_MODEL_FILE, VAD_MODEL_REPO, VAD_MODEL_URL, coreml_zip_url, ggml_file_name, ggml_url,
    partial_path_for, repo_for, sentinel_path_for,
};

/// One incremental progress update for a single artifact download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    /// File name of the artifact this update describes.
    pub file: String,
    /// Bytes downloaded so far, including any resumed offset.
    pub downloaded: u64,
    /// Total artifact size in bytes, if known from the response headers.
    pub total: Option<u64>,
}

/// Downloads and verifies whisper.cpp ggml models (optionally with a CoreML
/// encoder) and the Silero VAD model into a flat cache root.
pub struct ModelDownloader {
    client: reqwest::blocking::Client,
    root: PathBuf,
}

impl ModelDownloader {
    /// Creates a downloader rooted at `root` (typically
    /// [`crate::paths::model_root`]).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ModelDownloader {
            client: reqwest::blocking::Client::new(),
            root: root.into(),
        }
    }

    /// Ensures `size`'s ggml model file is present in the cache (downloading
    /// or resuming as needed), and, when `coreml_encoder` is `true`, ensures
    /// its CoreML encoder is extracted alongside it.
    ///
    /// Returns the path to the ggml model file. When `coreml_encoder` is
    /// `true`, only "this repo doesn't publish a CoreML encoder for this
    /// model" (HTTP 404) degrades gracefully — that case is resolved to
    /// `Ok(())` internally by [`Self::ensure_coreml_encoder`]
    /// (`model_loader.py:706-738`, Lumii ships no CoreML encoder at all).
    /// Every other CoreML failure (`ChecksumMismatch` from a
    /// corrupted/tampered zip, I/O errors, non-404 HTTP failures,
    /// cancellation) propagates to the caller — a real failure must not be
    /// silently downgraded to "no CoreML".
    pub fn ensure_model(
        &self,
        size: ModelSize,
        coreml_encoder: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        let repo = repo_for(size)?;
        let file_name = ggml_file_name(size);
        let url = ggml_url(size)?;
        let final_path = self.root.join(&file_name);

        self.ensure_artifact(&url, &final_path, Some(repo), progress, cancel)?;

        if coreml_encoder {
            propagate_coreml_result(self.ensure_coreml_encoder(size, progress, cancel))?;
        }

        Ok(final_path)
    }

    /// Ensures the Silero VAD model is present in the cache (downloading or
    /// resuming as needed). Returns its path.
    pub fn ensure_vad_model(
        &self,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        let final_path = self.root.join(VAD_MODEL_FILE);
        self.ensure_artifact(
            VAD_MODEL_URL,
            &final_path,
            Some(VAD_MODEL_REPO),
            progress,
            cancel,
        )?;
        Ok(final_path)
    }

    /// The per-artifact download algorithm: fast-path if already complete,
    /// otherwise download/resume to `<final_path>.partial`, verify SHA256
    /// when `sha_repo` is given, then rename to `final_path` and write the
    /// sentinel last.
    fn ensure_artifact(
        &self,
        url: &str,
        final_path: &Path,
        sha_repo: Option<&str>,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<(), ModelError> {
        let sentinel = sentinel_path_for(final_path);
        if final_path.is_file() && sentinel.is_file() {
            return Ok(());
        }

        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file_name = file_name_of(final_path);
        let partial_path = partial_path_for(final_path);

        let partial_len = std::fs::metadata(&partial_path).ok().map(|m| m.len());
        let range = resume_plan(partial_len);
        let response = self.send_get(url, range.as_deref())?;
        if !response.status().is_success() {
            return Err(ModelError::Http {
                detail: format!("{url}: HTTP {}", response.status()),
            });
        }
        self.write_response_stream(
            response,
            &partial_path,
            &file_name,
            range.is_some(),
            progress,
            cancel,
        )?;

        if let Some(repo) = sha_repo {
            self.verify_or_warn(repo, &file_name, &partial_path, final_path)?;
        }

        std::fs::rename(&partial_path, final_path)?;
        std::fs::write(&sentinel, b"")?;
        Ok(())
    }

    /// Downloads and extracts `size`'s CoreML encoder zip
    /// (`model_loader.py:706-735`). No completeness sentinel is written for
    /// the zip itself — the in-directory sentinel written after extraction
    /// is the sole completeness marker.
    fn ensure_coreml_encoder(
        &self,
        size: ModelSize,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<(), ModelError> {
        let ggml_name = size.ggml_name();
        let target_dir = self.root.join(format!("ggml-{ggml_name}-encoder.mlmodelc"));
        if target_dir.join(registry::SENTINEL_SUFFIX).is_file() {
            return Ok(());
        }

        let repo = repo_for(size)?;
        let url = coreml_zip_url(size)?;
        let zip_file_name = format!("ggml-{ggml_name}-encoder.mlmodelc.zip");
        let zip_path = self.root.join(&zip_file_name);
        let partial_zip_path = partial_path_for(&zip_path);

        std::fs::create_dir_all(&self.root)?;

        let partial_len = std::fs::metadata(&partial_zip_path).ok().map(|m| m.len());
        let range = resume_plan(partial_len);
        let response = self.send_get(&url, range.as_deref())?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            log::warn!(
                "no CoreML encoder published for {ggml_name} (HTTP 404); continuing without it"
            );
            let _ = std::fs::remove_file(&partial_zip_path);
            return Ok(());
        }
        if !response.status().is_success() {
            return Err(ModelError::Http {
                detail: format!("{url}: HTTP {}", response.status()),
            });
        }
        self.write_response_stream(
            response,
            &partial_zip_path,
            &zip_file_name,
            range.is_some(),
            progress,
            cancel,
        )?;

        self.verify_or_warn(repo, &zip_file_name, &partial_zip_path, &zip_path)?;

        std::fs::rename(&partial_zip_path, &zip_path)?;

        // No `tempfile` dependency in production code (Global Constraint
        // 10) — use a fixed, root-scoped scratch directory instead, wiped
        // before use in case a previous run was interrupted.
        let extract_dir = self.root.join(".humm-coreml-extract");
        if extract_dir.exists() {
            std::fs::remove_dir_all(&extract_dir)?;
        }
        std::fs::create_dir_all(&extract_dir)?;
        extract_zip_archive(&zip_path, &extract_dir)?;
        place_coreml_contents(&extract_dir, &target_dir)?;
        std::fs::remove_dir_all(&extract_dir)?;

        std::fs::remove_file(&zip_path)?;
        Ok(())
    }

    /// Fetches the expected SHA256 for `file_name` from `repo`'s HF tree
    /// API, hashes `partial_path`, and compares. `Ok(None)`/`Err` from the
    /// lookup are logged and treated as "skip verification" — Buzz only
    /// verifies "where upstream provides digests" (F-043). A mismatch
    /// deletes both the partial and (if present) the final path.
    fn verify_or_warn(
        &self,
        repo: &str,
        file_name: &str,
        partial_path: &Path,
        final_path: &Path,
    ) -> Result<(), ModelError> {
        match self.expected_sha256(repo, file_name) {
            Ok(Some(expected)) => {
                let actual = sha256_file(partial_path)?;
                if actual != expected {
                    remove_and_warn(partial_path);
                    remove_and_warn(final_path);
                    return Err(ModelError::ChecksumMismatch {
                        file: file_name.to_string(),
                    });
                }
                Ok(())
            }
            Ok(None) => {
                log::warn!("no SHA256 digest published for {file_name}; skipping verification");
                Ok(())
            }
            Err(e) => {
                log::warn!(
                    "could not fetch SHA256 digest for {file_name} ({e}); skipping verification"
                );
                Ok(())
            }
        }
    }

    /// `GET https://huggingface.co/api/models/{repo}/tree/main`, parsed for
    /// `file_name`'s `lfs.oid` (`docs/superpowers/.../task-5-brief.md`
    /// step 4).
    fn expected_sha256(&self, repo: &str, file_name: &str) -> Result<Option<String>, ModelError> {
        let url = format!("https://huggingface.co/api/models/{repo}/tree/main");
        let response = self.send_get(&url, None)?;
        if !response.status().is_success() {
            return Err(ModelError::Http {
                detail: format!("{url}: HTTP {}", response.status()),
            });
        }
        let text = response.text().map_err(|e| ModelError::Http {
            detail: e.to_string(),
        })?;
        Ok(find_sha256(&text, file_name))
    }

    /// Sends a `GET` request, optionally with a `Range` header. Only
    /// transport-level failures become `Err`; the caller inspects
    /// `response.status()` itself (some callers treat 404 specially).
    fn send_get(
        &self,
        url: &str,
        range: Option<&str>,
    ) -> Result<reqwest::blocking::Response, ModelError> {
        let mut request = self.client.get(url);
        if let Some(r) = range {
            request = request.header(reqwest::header::RANGE, r);
        }
        request.send().map_err(|e| ModelError::Http {
            detail: format!("{url}: {e}"),
        })
    }

    /// Streams `response`'s body into `partial_path` (appending if the
    /// server honored a `Range` request with `206`, truncating otherwise),
    /// calling `progress` after every 8 KiB chunk and checking `cancel`
    /// after each chunk — on cancellation, `partial_path` is left in place
    /// for a future resume (`ModelError::Canceled`).
    fn write_response_stream(
        &self,
        mut response: reqwest::blocking::Response,
        partial_path: &Path,
        file_name: &str,
        range_requested: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<(), ModelError> {
        let resumed = range_requested && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let existing_len = if resumed {
            std::fs::metadata(partial_path)
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };

        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let content_length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let total = total_size(
            content_range.as_deref(),
            content_length.as_deref(),
            existing_len,
        );

        let mut open_options = std::fs::OpenOptions::new();
        open_options.create(true);
        if resumed {
            open_options.append(true);
        } else {
            open_options.write(true).truncate(true);
        }
        let mut file = open_options.open(partial_path)?;

        let mut downloaded = existing_len;
        progress(DownloadProgress {
            file: file_name.to_string(),
            downloaded,
            total,
        });

        let mut buf = [0u8; 8192];
        loop {
            let n = response.read(&mut buf).map_err(|e| ModelError::Http {
                detail: e.to_string(),
            })?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            downloaded += n as u64;
            progress(DownloadProgress {
                file: file_name.to_string(),
                downloaded,
                total,
            });
            if cancel.is_canceled() {
                return Err(ModelError::Canceled);
            }
        }
        Ok(())
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

/// Removes `path`, logging a warning if deletion fails for a reason other
/// than the file already being absent (Buzz's
/// `except OSError as e: logging.warning(...)`, `model_loader.py:992-995`)
/// — used on the checksum-mismatch cleanup path, where `partial_path`
/// always exists (it was just hashed) but `final_path` usually doesn't yet
/// (verification happens before the `.partial` → final rename), so a
/// `NotFound` there is the expected common case, not a failure worth
/// surfacing.
fn remove_and_warn(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!(
            "failed to delete corrupted model file {}: {e}",
            path.display()
        );
    }
}

/// `ensure_model`'s CoreML-step propagation policy, factored out as a named,
/// independently-tested unit rather than inline call-site logic: every
/// `Err` reaching this point is a genuine failure and must pass through
/// unchanged.
///
/// [`ModelDownloader::ensure_coreml_encoder`] already resolves "this repo
/// doesn't publish a CoreML encoder for this model" (HTTP 404) to `Ok(())`
/// internally, so by the time a `Result` reaches this function there is no
/// "acceptable failure" left to swallow — a prior version of `ensure_model`
/// treated every non-`Canceled` error here as "log a warning and continue,"
/// which silently hid a corrupted/tampered CoreML zip's `ChecksumMismatch`
/// (and any I/O or non-404 HTTP failure) from the caller. Fixed per code
/// review.
fn propagate_coreml_result(result: Result<(), ModelError>) -> Result<(), ModelError> {
    result
}

/// Decides whether/how to resume a `.partial` download: `None` for a
/// missing or empty partial (start fresh, plain `GET`), `Some(range)` for a
/// nonempty partial (send `Range: bytes=<len>-`).
///
/// (Task 5 brief step 2: "Work file is `<final>.partial`. If it exists with
/// `len > 0`, send `Range: bytes=<len>-`".)
fn resume_plan(partial_len: Option<u64>) -> Option<String> {
    match partial_len {
        Some(len) if len > 0 => Some(format!("bytes={len}-")),
        _ => None,
    }
}

/// Determines the total artifact size from response headers: prefers the
/// `Content-Range` total (`bytes start-end/total`) when present, else
/// `resumed_offset + Content-Length`, else `None`.
fn total_size(
    content_range: Option<&str>,
    content_length: Option<&str>,
    resumed_offset: u64,
) -> Option<u64> {
    if let Some(cr) = content_range
        && let Some(total_str) = cr.rsplit('/').next()
        && let Ok(total) = total_str.parse::<u64>()
    {
        return Some(total);
    }
    content_length
        .and_then(|s| s.parse::<u64>().ok())
        .map(|len| resumed_offset + len)
}

/// Streamed SHA256 hex digest of the file at `path` (8 KiB chunks, matching
/// Buzz's `hashlib.sha256` loop, `model_loader.py:845-848`).
fn sha256_file(path: &Path) -> Result<String, ModelError> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Debug, serde::Deserialize)]
struct HfTreeEntry {
    path: String,
    #[serde(default)]
    lfs: Option<HfLfs>,
}

#[derive(Debug, serde::Deserialize)]
struct HfLfs {
    oid: String,
}

/// Parses a HuggingFace `tree/main` API JSON array, returning the `lfs.oid`
/// (SHA256) for the entry whose `path` matches `file_name`, if any.
fn find_sha256(tree_json: &str, file_name: &str) -> Option<String> {
    let entries: Vec<HfTreeEntry> = serde_json::from_str(tree_json).ok()?;
    entries
        .into_iter()
        .find(|entry| entry.path == file_name)
        .and_then(|entry| entry.lfs)
        .map(|lfs| lfs.oid)
}

/// Extracts the zip archive at `zip_path` into `dest` (must exist).
fn extract_zip_archive(zip_path: &Path, dest: &Path) -> Result<(), ModelError> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error_to_model_error)?;
    archive.extract(dest).map_err(zip_error_to_model_error)?;
    Ok(())
}

fn zip_error_to_model_error(err: zip::result::ZipError) -> ModelError {
    match err {
        zip::result::ZipError::Io(io_err) => ModelError::Io(io_err),
        other => ModelError::Http {
            detail: format!("corrupt CoreML zip: {other}"),
        },
    }
}

/// Buzz's CoreML zip placement algorithm (`model_loader.py:706-735`):
/// removes `__MACOSX`, then if the extracted contents are exactly one
/// directory, renames it to `target_dir`; otherwise moves every extracted
/// entry into a freshly created `target_dir`. Writes the in-directory
/// completeness sentinel last.
fn place_coreml_contents(extracted: &Path, target_dir: &Path) -> Result<(), ModelError> {
    let macosx = extracted.join("__MACOSX");
    if macosx.exists() {
        std::fs::remove_dir_all(&macosx)?;
    }

    let entries: Vec<PathBuf> = std::fs::read_dir(extracted)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();

    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir)?;
    }

    let single_dir = match entries.as_slice() {
        [only] if only.is_dir() => Some(only.clone()),
        _ => None,
    };

    if let Some(dir) = single_dir {
        std::fs::rename(&dir, target_dir)?;
    } else {
        std::fs::create_dir_all(target_dir)?;
        for entry in &entries {
            let file_name = entry.file_name().ok_or_else(|| {
                ModelError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "zip entry without a file name",
                ))
            })?;
            std::fs::rename(entry, target_dir.join(file_name))?;
        }
    }

    std::fs::write(target_dir.join(registry::SENTINEL_SUFFIX), b"")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::Cursor;

    #[test]
    fn propagate_coreml_result_passes_through_ok() {
        assert!(propagate_coreml_result(Ok(())).is_ok());
    }

    #[test]
    fn propagate_coreml_result_propagates_checksum_mismatch() {
        let err = ModelError::ChecksumMismatch {
            file: "ggml-small-encoder.mlmodelc.zip".to_string(),
        };
        assert!(matches!(
            propagate_coreml_result(Err(err)),
            Err(ModelError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn propagate_coreml_result_propagates_io_error() {
        let err = ModelError::Io(std::io::Error::other("disk full"));
        assert!(matches!(
            propagate_coreml_result(Err(err)),
            Err(ModelError::Io(_))
        ));
    }

    #[test]
    fn propagate_coreml_result_propagates_http_error() {
        let err = ModelError::Http {
            detail: "500".to_string(),
        };
        assert!(matches!(
            propagate_coreml_result(Err(err)),
            Err(ModelError::Http { .. })
        ));
    }

    #[test]
    fn propagate_coreml_result_propagates_canceled() {
        assert!(matches!(
            propagate_coreml_result(Err(ModelError::Canceled)),
            Err(ModelError::Canceled)
        ));
    }

    #[test]
    fn remove_and_warn_removes_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("corrupted.bin");
        std::fs::write(&path, b"bad checksum").expect("write");
        remove_and_warn(&path);
        assert!(!path.exists());
    }

    #[test]
    fn remove_and_warn_does_not_panic_on_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Must not panic even though the file was never created — this is
        // the common case for `final_path` in `verify_or_warn`, since
        // verification runs before the `.partial` -> final rename.
        remove_and_warn(&dir.path().join("never-existed.bin"));
    }

    #[test]
    fn resume_plan_none_when_no_partial() {
        assert_eq!(resume_plan(None), None);
    }

    #[test]
    fn resume_plan_none_when_partial_empty() {
        assert_eq!(resume_plan(Some(0)), None);
    }

    #[test]
    fn resume_plan_some_range_when_partial_nonempty() {
        assert_eq!(resume_plan(Some(100)), Some("bytes=100-".to_string()));
    }

    #[test]
    fn total_size_prefers_content_range_total() {
        // `Content-Length` on a 206 response is the *remaining* byte count,
        // not the total — deliberately different from the `Content-Range`
        // total below so this test can't pass via the `Content-Length`
        // fallback path by coincidence.
        assert_eq!(
            total_size(Some("bytes 100-199/500"), Some("100"), 100),
            Some(500)
        );
    }

    #[test]
    fn total_size_falls_back_to_offset_plus_content_length() {
        assert_eq!(total_size(None, Some("150"), 50), Some(200));
    }

    #[test]
    fn total_size_none_when_no_headers() {
        assert_eq!(total_size(None, None, 0), None);
    }

    #[test]
    fn total_size_malformed_content_range_falls_back_to_content_length() {
        assert_eq!(total_size(Some("bytes garbage"), Some("42"), 0), Some(42));
    }

    #[test]
    fn sha256_file_matches_known_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("humm.txt");
        std::fs::write(&path, b"humm").expect("write");
        // printf 'humm' | shasum -a 256
        assert_eq!(
            sha256_file(&path).unwrap(),
            "c1c82fa87440d56729c00525b3784e317bb2bde582fd072422d5d0f049ffbe8b"
        );
    }

    #[test]
    fn sha256_file_missing_is_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = sha256_file(&dir.path().join("nope.bin")).unwrap_err();
        assert!(matches!(err, ModelError::Io(_)));
    }

    #[test]
    fn find_sha256_locates_matching_entry() {
        let json = r#"[{"path":"ggml-tiny.bin","lfs":{"oid":"abcdef1234567890"}}]"#;
        assert_eq!(
            find_sha256(json, "ggml-tiny.bin"),
            Some("abcdef1234567890".to_string())
        );
    }

    #[test]
    fn find_sha256_none_when_file_not_in_tree() {
        let json = r#"[{"path":"ggml-tiny.bin","lfs":{"oid":"abcdef1234567890"}}]"#;
        assert_eq!(find_sha256(json, "ggml-small.bin"), None);
    }

    #[test]
    fn find_sha256_none_when_entry_has_no_lfs() {
        let json = r#"[{"path":"README.md"}]"#;
        assert_eq!(find_sha256(json, "README.md"), None);
    }

    #[test]
    fn find_sha256_none_on_malformed_json() {
        assert_eq!(find_sha256("not json", "ggml-tiny.bin"), None);
    }

    /// Builds an in-memory zip containing `__MACOSX/junk` (a macOS metadata
    /// artifact that must be discarded) and a single top-level directory
    /// `ggml-x-encoder.mlmodelc/model.mil`, matching the shape whisper.cpp's
    /// CoreML zips actually have.
    fn build_test_zip() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();

            writer.start_file("__MACOSX/junk", options).unwrap();
            writer.write_all(b"macos metadata junk").unwrap();

            writer
                .start_file("ggml-x-encoder.mlmodelc/model.mil", options)
                .unwrap();
            writer.write_all(b"fake coreml model bytes").unwrap();

            writer.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn extract_zip_archive_extracts_all_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip_path = dir.path().join("encoder.zip");
        std::fs::write(&zip_path, build_test_zip()).expect("write zip");

        let extract_dir = dir.path().join("extracted");
        std::fs::create_dir(&extract_dir).expect("mkdir");
        extract_zip_archive(&zip_path, &extract_dir).expect("extract");

        assert!(extract_dir.join("__MACOSX").join("junk").is_file());
        assert!(
            extract_dir
                .join("ggml-x-encoder.mlmodelc")
                .join("model.mil")
                .is_file()
        );
    }

    #[test]
    fn place_coreml_contents_flattens_single_nested_dir_and_drops_macosx() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip_path = dir.path().join("encoder.zip");
        std::fs::write(&zip_path, build_test_zip()).expect("write zip");

        let extract_dir = dir.path().join("extracted");
        std::fs::create_dir(&extract_dir).expect("mkdir");
        extract_zip_archive(&zip_path, &extract_dir).expect("extract");

        let target_dir = dir.path().join("ggml-x-encoder.mlmodelc");
        place_coreml_contents(&extract_dir, &target_dir).expect("place");

        assert!(!target_dir.join("__MACOSX").exists());
        assert!(target_dir.join("model.mil").is_file());
        assert_eq!(
            std::fs::read(target_dir.join("model.mil")).unwrap(),
            b"fake coreml model bytes"
        );
        assert!(target_dir.join(registry::SENTINEL_SUFFIX).is_file());
    }

    #[test]
    fn place_coreml_contents_moves_multiple_entries_into_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let extracted = dir.path().join("extracted");
        std::fs::create_dir(&extracted).expect("mkdir");
        std::fs::write(extracted.join("a.bin"), b"a").expect("write a");
        std::fs::write(extracted.join("b.bin"), b"b").expect("write b");

        let target_dir = dir.path().join("target.mlmodelc");
        place_coreml_contents(&extracted, &target_dir).expect("place");

        assert!(target_dir.join("a.bin").is_file());
        assert!(target_dir.join("b.bin").is_file());
        assert!(target_dir.join(registry::SENTINEL_SUFFIX).is_file());
    }
}
