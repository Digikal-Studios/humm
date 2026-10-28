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

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use crate::cancel::CancelToken;
use crate::domain::ModelSize;
use crate::error::ModelError;

use super::registry::{
    self, CoremlState, VAD_MODEL_FILE, VAD_MODEL_REPO, coreml_encoder_dir,
    coreml_no_encoder_marker, coreml_state, ggml_file_name, partial_path_for, repo_for,
    sentinel_path_for,
};

/// Process-wide registry of per-final-path download locks (carried M3
/// ticket, `docs/superpowers/.../task-13-brief.md` step 1): guards
/// [`ModelDownloader::ensure_artifact`]'s whole partial→verify→promote
/// sequence, so two concurrent callers racing the *same* artifact (e.g. a
/// Preferences-tab prefetch and an in-task `ensure_model` call for the same
/// model) never both download it — the second comer blocks on the artifact's
/// lock, then re-checks the completeness sentinel and takes the fast path.
///
/// Keyed by the artifact's `final_path` rather than a coarser process-wide
/// lock so unrelated artifacts (e.g. two different model sizes) never
/// serialize against each other. Entries are never removed — the number of
/// distinct artifacts a process ever ensures in its lifetime is small (a
/// handful of model sizes plus VAD/custom), so the leaked `Arc<Mutex<()>>`
/// map entries are not worth the complexity of pruning.
static ARTIFACT_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Returns (creating if necessary) the shared lock guarding `final_path` in
/// [`ARTIFACT_LOCKS`].
fn artifact_lock(final_path: &Path) -> Arc<Mutex<()>> {
    let mut locks = lock_recover(&ARTIFACT_LOCKS);
    Arc::clone(
        locks
            .entry(final_path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(()))),
    )
}

/// Locks a `Mutex`, recovering the guard if a previous holder panicked —
/// same rationale as `queue::lock_recover`: a panic elsewhere (e.g. a test)
/// must not permanently poison the shared lock registry for every other
/// caller in the process.
fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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
    hf_base: String,
}

impl ModelDownloader {
    /// Creates a downloader rooted at `root` (typically
    /// [`crate::paths::model_root`]), resolving all URLs against the
    /// default [`registry::HF_BASE`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_hf_base(root, registry::HF_BASE)
    }

    /// Creates a downloader rooted at `root` that resolves all URLs
    /// (ggml, CoreML zip, VAD model, SHA-digest lookups) against `hf_base`
    /// instead of the real HuggingFace host — for tests and mirrors.
    pub fn with_hf_base(root: impl Into<PathBuf>, hf_base: impl Into<String>) -> Self {
        ModelDownloader {
            client: reqwest::blocking::Client::new(),
            root: root.into(),
            hf_base: hf_base.into(),
        }
    }

    /// Ensures `size`'s ggml model file is present in the cache (downloading
    /// or resuming as needed), and, when `coreml_encoder` is `true`, ensures
    /// its CoreML encoder is extracted alongside it.
    ///
    /// Returns the path to the ggml model file. When `coreml_encoder` is
    /// `true`, only "this repo doesn't publish a CoreML encoder for this
    /// model" (HTTP 404) degrades gracefully — that case is resolved to
    /// `Ok(())` internally by [`Self::ensure_coreml_encoder`] (which also
    /// records a negative-cache marker) (`model_loader.py:706-738`, Lumii
    /// ships no CoreML encoder at all). Every other CoreML failure
    /// (`ChecksumMismatch` from a corrupted/tampered zip, I/O errors,
    /// non-404 HTTP failures, cancellation) propagates to the caller — a real
    /// failure must not be silently downgraded to "no CoreML".
    ///
    /// Callers that already hold a *usable* local ggml and want to tolerate a
    /// CoreML transport blip (not hard-fail a working model on a network
    /// hiccup) should use [`Self::resolve_model`] instead.
    pub fn ensure_model(
        &self,
        size: ModelSize,
        coreml_encoder: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        let final_path = self.ensure_ggml(size, progress, cancel)?;

        if coreml_encoder {
            // Every error reaching here is genuine: 404 ("no encoder
            // published") is already resolved to `Ok(())` inside
            // `ensure_coreml_encoder`. Propagate the rest — a corrupt zip's
            // `ChecksumMismatch`, an I/O failure, a non-404 HTTP error, or
            // cancellation must never be silently downgraded to "no CoreML".
            self.ensure_coreml_encoder(size, progress, cancel)?;
        }

        Ok(final_path)
    }

    /// Ensures just `size`'s ggml model file (no CoreML), returning its path.
    /// The shared prerequisite of [`Self::ensure_model`] and
    /// [`Self::resolve_model`].
    fn ensure_ggml(
        &self,
        size: ModelSize,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        let repo = repo_for(size)?;
        let final_path = self.root.join(ggml_file_name(size));
        let url = registry::ggml_url_with_base(&self.hf_base, size)?;
        self.ensure_artifact(&url, &final_path, Some(repo), progress, cancel)?;
        Ok(final_path)
    }

    /// Resolves `size`'s ggml model for use, ensuring the CoreML encoder when
    /// `coreml_encoder` is `true`, and — crucially — tolerating a CoreML
    /// *transport* failure once the ggml model is usable locally.
    ///
    /// This is the model fast path the CLI runs on every `humm add`. Unlike a
    /// bare [`crate::models::registry::local_model_path`] check (ggml file +
    /// sentinel), it does not treat "ggml present" as "fully resolved": if the
    /// CoreML encoder step never completed on an earlier run (a network drop,
    /// a cancel, a checksum failure), [`CoremlState::Missing`] is retried here
    /// instead of being silently skipped forever. [`CoremlState::Complete`]
    /// and [`CoremlState::KnownAbsent`] (upstream publishes none) both need no
    /// work.
    ///
    /// Failure policy once the ggml is usable (see [`coreml_failure_action`]):
    /// a transport error (`Http`) degrades to a warning and
    /// [`ResolvedModel::coreml_degraded`] `= true`, so the run continues
    /// without CoreML acceleration and retries next run; a `ChecksumMismatch`
    /// (corrupt/tampered encoder zip) or any other non-transport error
    /// propagates. Failure to obtain the ggml itself always propagates —
    /// without it there is no usable model.
    pub fn resolve_model(
        &self,
        size: ModelSize,
        coreml_encoder: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<ResolvedModel, ModelError> {
        let path = self.ensure_ggml(size, progress, cancel)?;

        let mut coreml_degraded = false;
        if coreml_encoder
            && coreml_state(&self.root, size) == CoremlState::Missing
            && let Err(err) = self.ensure_coreml_encoder(size, progress, cancel)
        {
            match coreml_failure_action(&err) {
                CoremlFailureAction::Degrade => {
                    log::warn!(
                        "CoreML encoder for {} unavailable ({err}); the local ggml model is \
                         usable, so continuing without CoreML acceleration (it will be retried \
                         on the next run)",
                        size.ggml_name()
                    );
                    coreml_degraded = true;
                }
                CoremlFailureAction::Propagate => return Err(err),
            }
        }

        Ok(ResolvedModel {
            path,
            coreml_degraded,
        })
    }

    /// Ensures a user-supplied custom ggml model (`-s custom`) is present in
    /// the cache at [`registry::custom_model_file_name`], downloading or
    /// resuming from `url` as needed (`model_loader.py:656-675`).
    ///
    /// No SHA256 verification: a custom URL is user-supplied, so there is no
    /// upstream digest to check it against — Buzz doesn't verify custom
    /// downloads either (`model_loader.py:670-675` calls
    /// `download_model_to_path` with `expected_sha256=None`). The
    /// completeness sentinel is still written, so a second call is a
    /// network-free fast path exactly like every other artifact.
    pub fn ensure_custom_model(
        &self,
        url: &str,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        let final_path = self.root.join(registry::custom_model_file_name());
        self.ensure_artifact(url, &final_path, None, progress, cancel)?;
        Ok(final_path)
    }

    /// Resolves a Hugging Face Whisper-family repo id to a local ggml model
    /// (`-m huggingface --hfid <repo>`, approved Q-2 "fetch ggml
    /// equivalents" scope — MMS/non-Whisper repos are out of scope).
    ///
    /// Two arms:
    /// 1. `repo` is `openai/whisper-<name>` and `<name>` maps to a known
    ///    [`ModelSize`] ([`registry::openai_whisper_model_size`]) — these are
    ///    the canonical OpenAI Whisper repos, whose ggml equivalent is just
    ///    the ordinary whisper.cpp/lumii size; delegate to
    ///    [`Self::resolve_model`] verbatim (same CoreML behavior, same cache
    ///    path, same everything).
    /// 2. Otherwise, list `repo`'s files via the Hugging Face Hub API
    ///    (`GET {hf_base}/api/models/{repo}`) and pick the ggml candidate:
    ///    files matching `ggml-*.bin` are preferred; if none, every `.bin`
    ///    file is a candidate. Exactly one candidate downloads from
    ///    `{hf_base}/{repo}/resolve/main/{file}` to
    ///    [`registry::hf_repo_model_file_name`] via [`Self::ensure_artifact`]
    ///    with no SHA verification (third-party repos don't expose a
    ///    per-file digest through this API) and, deliberately, no CoreML
    ///    encoder (third-party repos don't publish the encoder zip
    ///    whisper.cpp's own repos do — `coreml_degraded` is always `false`
    ///    here, not a transport degradation, just "never attempted").
    ///    Multiple candidates or zero candidates are both `Unsupported`,
    ///    with an actionable message (constraint 4): multiple lists the
    ///    candidate names and points at the custom-URL setting
    ///    (`models.custom_model_url`) to pick one explicitly; zero explains
    ///    that only ggml-format Whisper repos are supported here (MMS/other
    ///    non-Whisper repos are the scope cut) and points at both
    ///    whisper.cpp's conversion docs and the custom-URL setting. A
    ///    non-2xx or malformed API response is a transport-class failure —
    ///    [`ModelError::Http`].
    pub fn resolve_hf_repo(
        &self,
        repo: &str,
        coreml_encoder: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<ResolvedModel, ModelError> {
        if let Some(size) = registry::openai_whisper_model_size(repo) {
            return self.resolve_model(size, coreml_encoder, progress, cancel);
        }

        let siblings = self.list_hf_repo_files(repo)?;
        let candidates = select_ggml_candidates(&siblings);

        match candidates.as_slice() {
            [] => Err(ModelError::Unsupported {
                detail: format!(
                    "Hugging Face repo '{repo}' has no ggml-format model file (no `ggml-*.bin` \
                     or `*.bin` entries); humm's Hugging Face support only covers ggml-format \
                     Whisper-family repos — non-Whisper repos (e.g. MMS/wav2vec2) are out of \
                     scope. Convert the repo to ggml with whisper.cpp's conversion docs \
                     (https://github.com/ggerganov/whisper.cpp/tree/master/models#conversion) \
                     and point humm at the result via the custom-model-url setting \
                     (`models.custom_model_url`)."
                ),
            }),
            [file] => {
                let final_path = self.root.join(registry::hf_repo_model_file_name(repo));
                let url = format!("{}/{repo}/resolve/main/{file}", self.hf_base);
                self.ensure_artifact(&url, &final_path, None, progress, cancel)?;
                Ok(ResolvedModel {
                    path: final_path,
                    coreml_degraded: false,
                })
            }
            multiple => Err(ModelError::Unsupported {
                detail: format!(
                    "Hugging Face repo '{repo}' has multiple candidate ggml files ({}); humm \
                     can't pick one for you — download the one you want yourself and set it via \
                     the custom-model-url setting (`models.custom_model_url`) instead of `--hfid`.",
                    multiple.join(", ")
                ),
            }),
        }
    }

    /// Ensures the Silero VAD model is present in the cache (downloading or
    /// resuming as needed). Returns its path.
    pub fn ensure_vad_model(
        &self,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        let final_path = self.root.join(VAD_MODEL_FILE);
        let url = registry::vad_model_url_with_base(&self.hf_base);
        self.ensure_artifact(&url, &final_path, Some(VAD_MODEL_REPO), progress, cancel)?;
        Ok(final_path)
    }

    /// Ensures every diarization model file (F-087, ADR-024) is present in
    /// the cache under `root/`[`registry::DIARIZE_DIR`], downloading or
    /// resuming each as needed. Returns their resolved paths.
    ///
    /// Each of the 10 files ([`registry::DIARIZE_MODEL_FILES`]) goes through
    /// [`Self::ensure_artifact`] independently — same resume/sentinel/
    /// per-path-lock semantics as every other artifact this downloader
    /// manages, so a partial run (network drop after file 4 of 10) resumes
    /// from where it left off on the next call rather than re-fetching
    /// completed files. `sha_repo` is `Some(registry::DIARIZE_MODEL_REPO)`
    /// for every file: 9 of the 10 publish a Git-LFS `sha256` digest via the
    /// same `tree/main` API the whisper.cpp/VAD downloads already use
    /// (confirmed against the real HuggingFace API while implementing this);
    /// the tenth (a 4-byte, non-LFS text file) has none, and
    /// [`Self::verify_or_warn`]'s existing "no digest published, skip
    /// verification" fallback already handles that — no special-casing
    /// needed here.
    ///
    /// No cross-file progress aggregation: `progress` receives one
    /// [`DownloadProgress`] sequence per file (its `file` field names which
    /// one), the same shape [`Self::ensure_model`] already uses across its
    /// ggml + CoreML-zip pair — there is no existing precedent for
    /// synthesizing a single aggregate fraction across multiple artifacts,
    /// and this follows it rather than inventing one.
    pub fn ensure_diarization_models(
        &self,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<registry::DiarizationModelPaths, ModelError> {
        let paths = registry::diarization_model_paths(&self.root);
        for (file_name, path) in paths.files() {
            let url = registry::diarize_model_url_with_base(&self.hf_base, file_name);
            self.ensure_artifact(
                &url,
                path,
                Some(registry::DIARIZE_MODEL_REPO),
                progress,
                cancel,
            )?;
        }
        Ok(paths)
    }

    /// The per-artifact download algorithm: fast-path if already complete,
    /// otherwise download/resume to `<final_path>.partial`, verify SHA256
    /// when `sha_repo` is given, then rename to `final_path` and write the
    /// sentinel last.
    ///
    /// Handles HTTP 416 (Range Not Satisfiable) explicitly: a resume request
    /// whose offset is past the end of the resource means the `.partial` is
    /// already (at least) complete — most likely a fully-downloaded body
    /// whose process died in the narrow window between the last write and the
    /// rename. Recovering (verify-and-promote, or discard-and-restart-once)
    /// keeps a completed-but-unrenamed `.partial` from wedging the download in
    /// a permanent 416 loop.
    ///
    /// This is a thin sentinel-writing wrapper around
    /// [`Self::download_and_verify`] — the CoreML zip path (which has its own
    /// completion marker, the extracted directory's in-directory sentinel,
    /// and so never wants *this* sentinel written) calls
    /// [`Self::download_and_verify`] directly instead (`F-043` ticket b).
    ///
    /// Holds [`artifact_lock`]'s per-`final_path` lock across the entire
    /// partial→verify→promote sequence (carried M3 ticket, step 1): a second
    /// concurrent caller for the same artifact blocks here rather than
    /// racing its own download, then — once it acquires the lock — re-checks
    /// the completeness sentinel (now written by the first caller) and takes
    /// the same fast path a call arriving after completion would.
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

        let lock = artifact_lock(final_path);
        let _guard = lock_recover(&lock);

        // Re-check under the lock: another thread may have completed the
        // download (and written the sentinel) while this caller waited.
        if final_path.is_file() && sentinel.is_file() {
            return Ok(());
        }

        self.download_and_verify(url, final_path, sha_repo, progress, cancel)?;
        std::fs::write(sentinel, b"")?;
        Ok(())
    }

    /// Ensures `url`'s content ends up verified at `final_path` (resuming a
    /// `.partial` if present, recovering from a 416 if the resume offset is
    /// past EOF), without writing any completeness sentinel — callers that
    /// use a sibling-sentinel scheme write it themselves once this returns
    /// `Ok`. Shared by [`Self::ensure_artifact`] (ggml, VAD) and the CoreML
    /// zip fetch in [`Self::ensure_coreml_encoder`], so both go through the
    /// same 416-recovery logic (`F-043` ticket b).
    fn download_and_verify(
        &self,
        url: &str,
        final_path: &Path,
        sha_repo: Option<&str>,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<(), ModelError> {
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let partial_path = partial_path_for(final_path);
        let partial_len = std::fs::metadata(&partial_path).ok().map(|m| m.len());
        let range = resume_plan(partial_len);
        let response = self.send_get(url, range.as_deref())?;

        if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return self
                .recover_from_range_not_satisfiable(url, final_path, sha_repo, progress, cancel);
        }
        if !response.status().is_success() {
            return Err(ModelError::Http {
                detail: format!("{url}: HTTP {}", response.status()),
            });
        }

        self.download_body(
            response,
            final_path,
            sha_repo,
            range.is_some(),
            progress,
            cancel,
        )
    }

    /// Common download tail shared by the resume, fresh, and 416-restart
    /// paths: stream `response` into the `.partial`, verify its SHA256 when
    /// `sha_repo` is given, then rename to `final_path`. Writes no sentinel —
    /// see [`Self::download_and_verify`]. `resumed_range` records whether the
    /// request that produced `response` carried a `Range` header (so a `206`
    /// appends rather than truncates).
    fn download_body(
        &self,
        response: reqwest::blocking::Response,
        final_path: &Path,
        sha_repo: Option<&str>,
        resumed_range: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<(), ModelError> {
        let file_name = file_name_of(final_path);
        let partial_path = partial_path_for(final_path);

        self.write_response_stream(
            response,
            &partial_path,
            &file_name,
            resumed_range,
            progress,
            cancel,
        )?;

        if let Some(repo) = sha_repo {
            self.verify_or_warn(repo, &file_name, &partial_path, final_path)?;
        }

        std::fs::rename(&partial_path, final_path)?;
        Ok(())
    }

    /// Downloads `url` from scratch (no `Range`) into `final_path`. Used to
    /// recover from a 416 whose `.partial` could not be verified as complete.
    fn download_fresh(
        &self,
        url: &str,
        final_path: &Path,
        sha_repo: Option<&str>,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<(), ModelError> {
        let response = self.send_get(url, None)?;
        if !response.status().is_success() {
            return Err(ModelError::Http {
                detail: format!("{url}: HTTP {}", response.status()),
            });
        }
        self.download_body(response, final_path, sha_repo, false, progress, cancel)
    }

    /// Recovers from an HTTP 416 on a resume request (see
    /// [`Self::download_and_verify`]).
    ///
    /// If a digest is obtainable and the `.partial` matches it, promote the
    /// `.partial` in place (rename only — no sentinel; see
    /// [`Self::download_and_verify`]) — no re-download. Otherwise (no digest
    /// obtainable, or a mismatch) discard the `.partial` and download fresh
    /// exactly once, so a stale or corrupt `.partial` can never wedge the
    /// download in a permanent 416 loop. Shared verbatim by the ggml/VAD path
    /// and the CoreML zip path (`F-043` ticket b).
    fn recover_from_range_not_satisfiable(
        &self,
        url: &str,
        final_path: &Path,
        sha_repo: Option<&str>,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<(), ModelError> {
        let file_name = file_name_of(final_path);
        let partial_path = partial_path_for(final_path);

        let verified_complete =
            self.verify_partial_is_complete(sha_repo, &file_name, &partial_path)?;

        match on_range_not_satisfiable(verified_complete) {
            RangeRecovery::Promote => {
                std::fs::rename(&partial_path, final_path)?;
                Ok(())
            }
            RangeRecovery::RestartFresh => {
                remove_and_warn(&partial_path);
                self.download_fresh(url, final_path, sha_repo, progress, cancel)
            }
        }
    }

    /// Attempts to verify the `.partial` at `partial_path` as a complete
    /// artifact against its upstream digest, for 416 recovery. Returns
    /// `Some(true)` if a published digest matches, `Some(false)` on mismatch,
    /// and `None` when verification is impossible (no `sha_repo`, no digest
    /// published, or the digest lookup failed as a transport error — an
    /// unverifiable partial is restarted fresh, never trusted). A local I/O
    /// error while hashing propagates.
    fn verify_partial_is_complete(
        &self,
        sha_repo: Option<&str>,
        file_name: &str,
        partial_path: &Path,
    ) -> Result<Option<bool>, ModelError> {
        let Some(repo) = sha_repo else {
            return Ok(None);
        };
        match self.expected_sha256(repo, file_name) {
            Ok(Some(expected)) => {
                let actual = sha256_file(partial_path)?;
                Ok(Some(actual == expected))
            }
            Ok(None) | Err(_) => Ok(None),
        }
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
        // Both terminal states — a complete extracted encoder and a recorded
        // "upstream has none" — need no work and no network.
        match coreml_state(&self.root, size) {
            CoremlState::Complete | CoremlState::KnownAbsent => return Ok(()),
            CoremlState::Missing => {}
        }

        let ggml_name = size.ggml_name();
        let target_dir = coreml_encoder_dir(&self.root, size);

        let repo = repo_for(size)?;
        let url = registry::coreml_zip_url_with_base(&self.hf_base, size)?;
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
            // Record the known-absent state so future runs treat the missing
            // encoder as a *resolved* (absent) artifact — the model fast path
            // stops retrying and stops re-probing the network (Lumii, e.g.,
            // publishes no CoreML encoder at all).
            let marker = coreml_no_encoder_marker(&self.root, size);
            if let Err(e) = std::fs::write(&marker, b"") {
                log::warn!(
                    "failed to write CoreML negative-cache marker {}: {e}",
                    marker.display()
                );
            }
            return Ok(());
        }
        if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            // Same recovery the ggml/VAD path uses (`F-043` ticket b): a
            // resume request past EOF most likely means an earlier run's
            // zip `.partial` already completed but died before the rename —
            // verify-and-promote it, or discard and restart once, instead of
            // hard-failing on a spurious 416.
            self.recover_from_range_not_satisfiable(&url, &zip_path, Some(repo), progress, cancel)?;
        } else if !response.status().is_success() {
            return Err(ModelError::Http {
                detail: format!("{url}: HTTP {}", response.status()),
            });
        } else {
            self.download_body(
                response,
                &zip_path,
                Some(repo),
                range.is_some(),
                progress,
                cancel,
            )?;
        }

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

    /// `GET {hf_base}/api/models/{repo}/tree/main`, parsed for `file_name`'s
    /// `lfs.oid` (`docs/superpowers/.../task-5-brief.md` step 4). Resolves
    /// against the same `hf_base` as every other URL this downloader builds
    /// (`F-043` ticket a), so tests can serve the digest lookup locally too.
    fn expected_sha256(&self, repo: &str, file_name: &str) -> Result<Option<String>, ModelError> {
        let url = tree_api_url(&self.hf_base, repo);
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

    /// `GET {hf_base}/api/models/{repo}` (the Hugging Face Hub repo-info API,
    /// distinct from the `tree/main` endpoint [`Self::expected_sha256`]
    /// uses), parsed for every `siblings[].rfilename` — the repo's full file
    /// listing, used by [`Self::resolve_hf_repo`] to find a ggml candidate.
    /// A non-2xx status or a response body that doesn't parse as the
    /// expected JSON shape are both transport-class failures: the caller
    /// cannot tell "repo doesn't exist" from "API changed shape" from here,
    /// and either way there is nothing useful to fall back to.
    fn list_hf_repo_files(&self, repo: &str) -> Result<Vec<String>, ModelError> {
        let url = hf_repo_api_url(&self.hf_base, repo);
        let response = self.send_get(&url, None)?;
        if !response.status().is_success() {
            return Err(ModelError::Http {
                detail: format!(
                    "listing Hugging Face repo '{repo}' files: {url}: HTTP {}",
                    response.status()
                ),
            });
        }
        let text = response.text().map_err(|e| ModelError::Http {
            detail: format!("listing Hugging Face repo '{repo}' files: {url}: {e}"),
        })?;
        let info: HfModelInfo = serde_json::from_str(&text).map_err(|e| ModelError::Http {
            detail: format!(
                "Hugging Face repo '{repo}' returned a malformed API response ({url}): {e}"
            ),
        })?;
        Ok(info.siblings.into_iter().map(|s| s.rfilename).collect())
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

/// The HuggingFace tree-listing API URL for `repo` at a given `base` —
/// shared by [`ModelDownloader::expected_sha256`] and its tests, so the two
/// can never drift apart.
fn tree_api_url(base: &str, repo: &str) -> String {
    format!("{base}/api/models/{repo}/tree/main")
}

/// The HuggingFace repo-info API URL for `repo` at a given `base` — shared
/// by [`ModelDownloader::list_hf_repo_files`] and its tests. Distinct from
/// [`tree_api_url`]: this endpoint returns the whole repo record (including
/// `siblings[].rfilename`, the full file listing), not per-file digests.
fn hf_repo_api_url(base: &str, repo: &str) -> String {
    format!("{base}/api/models/{repo}")
}

/// Picks the ggml model candidates out of a Hugging Face repo's file listing
/// (`resolve_hf_repo`'s selection rule, approved Q-2 scope): files matching
/// `ggml-*.bin` are preferred; if none match, every `.bin` file is a
/// candidate instead (kept deliberately simple — no further filtering of
/// non-model `.bin` names). Order is preserved from `files`.
fn select_ggml_candidates(files: &[String]) -> Vec<String> {
    let ggml_bin: Vec<String> = files
        .iter()
        .filter(|f| f.starts_with("ggml-") && f.ends_with(".bin"))
        .cloned()
        .collect();
    if !ggml_bin.is_empty() {
        return ggml_bin;
    }
    files
        .iter()
        .filter(|f| f.ends_with(".bin"))
        .cloned()
        .collect()
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

/// Outcome of [`ModelDownloader::resolve_model`]: the usable ggml model path,
/// plus whether a wanted CoreML encoder had to be skipped due to a transport
/// error (so the caller can surface a user-facing notice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    /// Path to the usable ggml model file.
    pub path: PathBuf,
    /// `true` iff CoreML was wanted and [`CoremlState::Missing`] but could not
    /// be ensured due to a transport error, while the local ggml stayed
    /// usable, so the run proceeds without CoreML acceleration (and retries on
    /// the next run).
    pub coreml_degraded: bool,
}

/// Whether a failure to ensure the CoreML encoder — when the ggml model is
/// already usable locally — should be tolerated (degrade to "no CoreML") or
/// propagated as a hard error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoremlFailureAction {
    /// A transport blip (`Http`): a usable local ggml must not be hard-failed
    /// over a network hiccup — warn, continue without CoreML, retry later.
    Degrade,
    /// A genuine artifact or environment failure (`ChecksumMismatch` from a
    /// corrupt/tampered zip, I/O error, cancellation, ...): propagate.
    Propagate,
}

/// The degrade-vs-propagate decision for a CoreML-ensure failure, given that
/// the ggml is already usable. Only a transport error (`Http`) degrades; a
/// `ChecksumMismatch` (a corrupt artifact, not a network blip) and every
/// other error propagate.
fn coreml_failure_action(err: &ModelError) -> CoremlFailureAction {
    match err {
        ModelError::Http { .. } => CoremlFailureAction::Degrade,
        _ => CoremlFailureAction::Propagate,
    }
}

/// The two outcomes of an HTTP 416 recovery, decided purely from whether the
/// `.partial` could be verified as a complete artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeRecovery {
    /// The `.partial` verified as complete — promote it (rename + sentinel).
    Promote,
    /// The `.partial` could not be verified (no digest obtainable, or a
    /// mismatch) — discard it and download fresh once.
    RestartFresh,
}

/// Decides how to recover from a 416 given the `.partial` verification result:
/// `Some(true)` (verified complete) promotes it; `Some(false)` (mismatch) and
/// `None` (no digest obtainable) both restart fresh. There is deliberately no
/// "loop on 416" outcome, so a completed-but-unrenamed or stale `.partial`
/// cannot wedge the download permanently.
fn on_range_not_satisfiable(verified_complete: Option<bool>) -> RangeRecovery {
    match verified_complete {
        Some(true) => RangeRecovery::Promote,
        Some(false) | None => RangeRecovery::RestartFresh,
    }
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

/// The shape of a Hugging Face Hub `GET /api/models/{repo}` response that
/// [`ModelDownloader::list_hf_repo_files`] cares about — just the file
/// listing, ignoring every other repo-metadata field the real API returns.
#[derive(Debug, serde::Deserialize)]
struct HfModelInfo {
    siblings: Vec<HfSibling>,
}

#[derive(Debug, serde::Deserialize)]
struct HfSibling {
    rfilename: String,
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
///
/// Does not call [`zip::ZipArchive::extract`]. zip 8.6.0 creates a symlink
/// with the archive's target bytes and does not require that target to stay
/// inside `dest` (`RUSTSEC-2026-0368`). A later entry, or
/// [`place_coreml_contents`], can then write through that link. Symlink
/// entries whose target leaves `dest` are refused and not created. File
/// modes from the archive are not applied, so a zip cannot mark an extracted
/// file setuid.
fn extract_zip_archive(zip_path: &Path, dest: &Path) -> Result<(), ModelError> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error_to_model_error)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_error_to_model_error)?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(zip_path_error("CoreML zip entry path escapes the archive"));
        };
        let outpath = dest.join(relative);
        if path_escapes(dest, &outpath)? {
            return Err(zip_path_error(
                "CoreML zip entry is reached through a symlink that leaves the extract directory",
            ));
        }
        if entry.is_symlink() {
            let mut target_bytes = Vec::new();
            entry
                .read_to_end(&mut target_bytes)
                .map_err(ModelError::Io)?;
            let target = String::from_utf8(target_bytes)
                .map_err(|_| zip_path_error("CoreML zip symlink target is not UTF-8"))?;
            if proposed_symlink_escapes(&outpath, Path::new(&target), dest) {
                return Err(zip_path_error(
                    "CoreML zip symlink target leaves the extract directory",
                ));
            }
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            create_contained_symlink(Path::new(&target), &outpath)?;
            continue;
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&outpath)?;
        std::io::copy(&mut entry, &mut outfile)?;
    }
    Ok(())
}

fn zip_path_error(detail: &str) -> ModelError {
    ModelError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, detail))
}

/// How many symlink hops one resolution may follow before it is treated as
/// an escape. A cycle must not hang extraction.
const MAX_SYMLINK_HOPS: usize = 40;

/// `true` when `link` is a symlink whose target, after following links that
/// already exist, is not inside `root`. A missing path is not an escaping link.
fn symlink_target_escapes(link: &Path, root: &Path) -> Result<bool, ModelError> {
    let meta = match std::fs::symlink_metadata(link) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    if !meta.file_type().is_symlink() {
        return Ok(false);
    }
    let target = std::fs::read_link(link)?;
    Ok(proposed_symlink_escapes(link, &target, root))
}

/// Whether creating `link` → `target` would leave `root` once existing
/// symlinks are followed. `..` is applied to the resolved directory, not
/// cancelled against a symlink component lexically. A target whose final
/// component does not exist yet still counts as an escape when that
/// location is outside `root`.
fn proposed_symlink_escapes(link: &Path, target: &Path, root: &Path) -> bool {
    let Some(parent) = link.parent() else {
        return true;
    };
    match resolve_within(root, parent, target) {
        Ok(_) => false,
        Err(WalkError::Escape) => true,
        Err(WalkError::Io(_)) => true,
    }
}

/// `true` when `path` is outside `root` or reaches outside by following a
/// symlink that already exists.
fn path_escapes(root: &Path, path: &Path) -> Result<bool, ModelError> {
    if path.is_absolute() && !path.starts_with(root) {
        return Ok(true);
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    match resolve_within(root, root, relative) {
        Ok(_) => Ok(false),
        Err(WalkError::Escape) => Ok(true),
        Err(WalkError::Io(err)) => Err(err.into()),
    }
}

enum WalkError {
    Escape,
    Io(std::io::Error),
}

/// Walk `extra` starting from `start`, following symlinks that exist.
/// The result stays inside `root` or the walk returns [`WalkError::Escape`].
fn resolve_within(root: &Path, start: &Path, extra: &Path) -> Result<PathBuf, WalkError> {
    let mut hops = 0;
    let mut current = if extra.is_absolute() {
        PathBuf::new()
    } else if start == root {
        root.to_path_buf()
    } else {
        let relative = start.strip_prefix(root).map_err(|_| WalkError::Escape)?;
        let mut base = root.to_path_buf();
        apply_components(root, &mut base, relative, &mut hops)?;
        base
    };
    apply_components(root, &mut current, extra, &mut hops)?;
    if !is_inside(root, &current) {
        return Err(WalkError::Escape);
    }
    Ok(current)
}

fn apply_components(
    root: &Path,
    current: &mut PathBuf,
    path: &Path,
    hops: &mut usize,
) -> Result<(), WalkError> {
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // A missing component is not a real directory. Popping it
                // would cancel `..` lexically before a later zip entry can
                // make that name a symlink (`a/b/link` → `missing/../../outside`
                // while `missing` does not exist yet).
                match std::fs::symlink_metadata(&*current) {
                    Ok(meta) if meta.is_dir() => {
                        if !pop_inside(root, current) {
                            return Err(WalkError::Escape);
                        }
                    }
                    Ok(_) => return Err(WalkError::Escape),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        return Err(WalkError::Escape);
                    }
                    Err(err) => return Err(WalkError::Io(err)),
                }
            }
            Component::Normal(name) => {
                let next = current.join(name);
                match std::fs::symlink_metadata(&next) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        *hops += 1;
                        if *hops > MAX_SYMLINK_HOPS {
                            return Err(WalkError::Escape);
                        }
                        let target = std::fs::read_link(&next).map_err(WalkError::Io)?;
                        if target.is_absolute() {
                            *current = PathBuf::new();
                        }
                        apply_components(root, current, &target, hops)?;
                    }
                    Ok(_) => *current = next,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => *current = next,
                    Err(err) => return Err(WalkError::Io(err)),
                }
                if !is_inside(root, current) {
                    return Err(WalkError::Escape);
                }
            }
            Component::RootDir => *current = PathBuf::from("/"),
            Component::Prefix(prefix) => *current = PathBuf::from(prefix.as_os_str()),
        }
    }
    Ok(())
}

fn is_inside(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}

/// `..` from `root` leaves the extract directory. Pop only while `current`
/// is a strict descendant of `root`.
fn pop_inside(root: &Path, current: &mut PathBuf) -> bool {
    if *current == root || !current.starts_with(root) {
        return false;
    }
    current.pop();
    *current == root || current.starts_with(root)
}

fn create_contained_symlink(target: &Path, outpath: &Path) -> Result<(), ModelError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, outpath)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (target, outpath);
        Err(zip_path_error(
            "CoreML zip symlinks are not extracted on this platform",
        ))
    }
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
    // `remove_dir_all` and `exists` follow symlinks. An entry named
    // `__MACOSX` that points outside `extracted` must be unlinked, not traversed.
    if symlink_target_escapes(&macosx, extracted)? {
        return Err(zip_path_error(
            "CoreML zip symlink target leaves the extract directory",
        ));
    }
    match std::fs::symlink_metadata(&macosx) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(&macosx)?,
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&macosx)?,
        Ok(_) => std::fs::remove_file(&macosx)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    let entries: Vec<PathBuf> = std::fs::read_dir(extracted)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    for entry in &entries {
        if symlink_target_escapes(entry, extracted)? {
            return Err(zip_path_error(
                "CoreML zip symlink target leaves the extract directory",
            ));
        }
    }

    // Same rule for a pre-existing `target_dir`: never `remove_dir_all` or
    // write the sentinel through a link that leaves `extracted`.
    if symlink_target_escapes(target_dir, extracted)? {
        return Err(zip_path_error(
            "CoreML zip symlink target leaves the extract directory",
        ));
    }
    match std::fs::symlink_metadata(target_dir) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(target_dir)?,
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(target_dir)?,
        Ok(_) => std::fs::remove_file(target_dir)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    let single_dir = match entries.as_slice() {
        [only] if !is_symlink(only)? && only.is_dir() => Some(only.clone()),
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

    if symlink_target_escapes(target_dir, extracted)? {
        return Err(zip_path_error(
            "CoreML zip symlink target leaves the extract directory",
        ));
    }
    std::fs::write(target_dir.join(registry::SENTINEL_SUFFIX), b"")?;
    Ok(())
}

fn is_symlink(path: &Path) -> Result<bool, ModelError> {
    Ok(std::fs::symlink_metadata(path)?.file_type().is_symlink())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::Cursor;

    #[test]
    fn coreml_failure_action_degrades_only_on_transport_error() {
        // A usable local ggml must not be hard-failed over a network blip.
        assert_eq!(
            coreml_failure_action(&ModelError::Http {
                detail: "connection reset".to_string(),
            }),
            CoremlFailureAction::Degrade
        );
    }

    #[test]
    fn coreml_failure_action_propagates_checksum_mismatch() {
        // A corrupt/tampered encoder zip is not a transport blip — surface it.
        assert_eq!(
            coreml_failure_action(&ModelError::ChecksumMismatch {
                file: "ggml-small-encoder.mlmodelc.zip".to_string(),
            }),
            CoremlFailureAction::Propagate
        );
    }

    #[test]
    fn coreml_failure_action_propagates_io_canceled_and_unsupported() {
        assert_eq!(
            coreml_failure_action(&ModelError::Io(std::io::Error::other("disk full"))),
            CoremlFailureAction::Propagate
        );
        assert_eq!(
            coreml_failure_action(&ModelError::Canceled),
            CoremlFailureAction::Propagate
        );
        assert_eq!(
            coreml_failure_action(&ModelError::Unsupported {
                detail: "nope".to_string(),
            }),
            CoremlFailureAction::Propagate
        );
    }

    #[test]
    fn on_range_not_satisfiable_promotes_only_verified_partial() {
        assert_eq!(on_range_not_satisfiable(Some(true)), RangeRecovery::Promote);
    }

    #[test]
    fn on_range_not_satisfiable_restarts_on_mismatch_or_unverifiable() {
        // Mismatch (corrupt/stale partial) and "no digest obtainable" both
        // discard and restart fresh — never trust an unverified partial, and
        // never loop on 416.
        assert_eq!(
            on_range_not_satisfiable(Some(false)),
            RangeRecovery::RestartFresh
        );
        assert_eq!(on_range_not_satisfiable(None), RangeRecovery::RestartFresh);
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

    /// A symlink entry whose target is outside the extract directory, followed
    /// by a file that would be written through that link.
    fn build_escaping_symlink_zip() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            writer
                .add_symlink("encoder", "../outside", options)
                .expect("symlink entry");
            writer
                .start_file("encoder/pwned", options)
                .expect("file through the link");
            writer.write_all(b"owned").expect("payload");
            writer.finish().expect("finish zip");
        }
        buf.into_inner()
    }

    #[cfg(unix)]
    #[test]
    fn extract_zip_archive_does_not_create_an_escaping_symlink() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).expect("outside dir");
        let zip_path = dir.path().join("encoder.zip");
        std::fs::write(&zip_path, build_escaping_symlink_zip()).expect("write zip");

        let extract_dir = dir.path().join("extracted");
        std::fs::create_dir(&extract_dir).expect("mkdir");
        let err = extract_zip_archive(&zip_path, &extract_dir).expect_err("escaping symlink");
        assert!(matches!(err, ModelError::Io(_)), "error was {err:?}");
        assert!(
            std::fs::symlink_metadata(extract_dir.join("encoder")).is_err(),
            "the escaping symlink must not be created"
        );
        assert!(
            std::fs::read_dir(&outside)
                .expect("read outside")
                .next()
                .is_none(),
            "nothing may be written outside the extract directory"
        );
    }

    /// `a/b/missing` → `..` first, then `a/b/link` → `missing/../../outside`,
    /// then file `a/b/link/pwned`. `missing` exists before `link` is checked.
    fn build_symlink_chain_zip() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            writer
                .add_symlink("a/b/missing", "..", options)
                .expect("missing symlink");
            writer
                .add_symlink("a/b/link", "missing/../../outside", options)
                .expect("chain symlink");
            writer
                .start_file("a/b/link/pwned", options)
                .expect("file through the chain");
            writer.write_all(b"owned").expect("payload");
            writer.finish().expect("finish zip");
        }
        buf.into_inner()
    }

    #[cfg(unix)]
    #[test]
    fn extract_zip_archive_rejects_symlink_chain_that_escapes_via_dotdot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).expect("outside dir");
        let zip_path = dir.path().join("encoder.zip");
        std::fs::write(&zip_path, build_symlink_chain_zip()).expect("write zip");

        let extract_dir = dir.path().join("extracted");
        std::fs::create_dir(&extract_dir).expect("mkdir");
        let err = extract_zip_archive(&zip_path, &extract_dir).expect_err("escaping chain");
        assert!(matches!(err, ModelError::Io(_)), "error was {err:?}");
        assert!(
            !outside.join("pwned").exists(),
            "the chained symlink must not write outside the extract directory"
        );
        assert!(
            std::fs::symlink_metadata(extract_dir.join("a/b/link")).is_err(),
            "the escaping link must not be created"
        );
    }

    /// Same chain with `link` created before `missing` exists. A lexical
    /// pop of the missing name would accept `missing/../../outside`.
    fn build_swapped_symlink_chain_zip() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            writer
                .add_symlink("a/b/link", "missing/../../outside", options)
                .expect("link before missing");
            writer
                .add_symlink("a/b/missing", "..", options)
                .expect("missing symlink");
            writer
                .start_file("a/b/link/pwned", options)
                .expect("file through the chain");
            writer.write_all(b"owned").expect("payload");
            writer.finish().expect("finish zip");
        }
        buf.into_inner()
    }

    #[cfg(unix)]
    #[test]
    fn extract_zip_archive_rejects_symlink_chain_when_dotdot_precedes_the_link() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).expect("outside dir");
        let zip_path = dir.path().join("encoder.zip");
        std::fs::write(&zip_path, build_swapped_symlink_chain_zip()).expect("write zip");

        let extract_dir = dir.path().join("extracted");
        std::fs::create_dir(&extract_dir).expect("mkdir");
        let err = extract_zip_archive(&zip_path, &extract_dir).expect_err("swapped chain");
        assert!(matches!(err, ModelError::Io(_)), "error was {err:?}");
        assert!(
            std::fs::symlink_metadata(extract_dir.join("a/b/link")).is_err(),
            "link must not be created before missing exists"
        );
        assert!(
            std::fs::read_dir(&outside)
                .expect("read outside")
                .next()
                .is_none(),
            "outside must stay empty"
        );

        let target_dir = dir.path().join("placed.mlmodelc");
        let placed = place_coreml_contents(&extract_dir, &target_dir);
        if placed.is_ok() {
            assert!(
                std::fs::symlink_metadata(target_dir.join("b/link")).is_err(),
                "place must not install the escaping link"
            );
        }
        assert!(
            std::fs::read_dir(&outside)
                .expect("read outside")
                .next()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn place_coreml_contents_does_not_follow_an_escaping_symlink() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).expect("outside dir");
        let extracted = dir.path().join("extracted");
        std::fs::create_dir(&extracted).expect("mkdir");
        std::os::unix::fs::symlink("../outside", extracted.join("encoder")).expect("symlink");

        let target_dir = dir.path().join("placed.mlmodelc");
        let err = place_coreml_contents(&extracted, &target_dir).expect_err("escaping symlink");
        assert!(matches!(err, ModelError::Io(_)), "error was {err:?}");
        assert!(
            !outside.join(registry::SENTINEL_SUFFIX).exists(),
            "sentinel must not be written through the symlink"
        );
        assert!(
            std::fs::read_dir(&outside)
                .expect("read outside")
                .next()
                .is_none()
        );
        assert!(!target_dir.join(registry::SENTINEL_SUFFIX).exists());
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

    // --- HTTP seam tests (M2 F-043 tickets a/b/c) — all network-free, via
    // `crate::models::test_server::TestServer`. ---

    use crate::models::test_server::{Canned, TestServer};

    fn sha256_bytes(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    /// The request-target path (no scheme/host) of a `_with_base`-built URL,
    /// for matching against `TestServer`'s per-path routes.
    fn path_of(url: &str) -> String {
        url::Url::parse(url).expect("parse url").path().to_string()
    }

    /// Ticket a (the M1 final reviewer's explicit acceptance criterion):
    /// a valid, complete ggml + sentinel is already cached and CoreML state
    /// is `Missing` — `resolve_model` with `coreml_encoder = true` must still
    /// *attempt* the encoder zip fetch rather than silently skipping it.
    #[test]
    fn resolve_model_attempts_coreml_zip_fetch_when_missing_and_wanted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;

        let ggml_path = dir.path().join(ggml_file_name(size));
        std::fs::write(&ggml_path, b"already-downloaded ggml bytes").expect("write ggml");
        std::fs::write(sentinel_path_for(&ggml_path), b"").expect("write ggml sentinel");
        assert_eq!(coreml_state(dir.path(), size), CoremlState::Missing);

        let server = TestServer::start();
        let zip_url = registry::coreml_zip_url_with_base(&server.base_url(), size).unwrap();
        let zip_path = path_of(&zip_url);
        // A 404 here still counts as "attempted": the point being pinned is
        // that the request happens at all, not what it returns.
        server.set_route(zip_path.clone(), Canned::status(404));

        let downloader = ModelDownloader::with_hf_base(dir.path(), server.base_url());
        let cancel = CancelToken::new();
        let resolved = downloader
            .resolve_model(size, true, &mut |_| {}, &cancel)
            .expect("resolve_model should succeed when upstream has no CoreML encoder");

        assert_eq!(resolved.path, ggml_path);
        assert!(!resolved.coreml_degraded);
        assert!(
            server
                .requests()
                .iter()
                .any(|r| r.method == "GET" && r.path == zip_path),
            "expected a GET for the CoreML encoder zip in the request log: {:?}",
            server.requests()
        );
    }

    /// Ticket b: a resume request whose `.partial` is a COMPLETE, valid zip
    /// gets a 416 from the server — recovery must verify it against the
    /// published digest (served from the same local server) and promote it
    /// in place, ending with the encoder directory `Complete`, without a
    /// second network round-trip for the zip body itself.
    #[test]
    fn ensure_coreml_encoder_416_recovery_promotes_verified_partial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;
        let repo = repo_for(size).unwrap();

        let zip_bytes = build_test_zip();
        let zip_file_name = format!("ggml-{}-encoder.mlmodelc.zip", size.ggml_name());
        let zip_path = dir.path().join(&zip_file_name);
        let partial_zip_path = partial_path_for(&zip_path);
        std::fs::write(&partial_zip_path, &zip_bytes).expect("write complete partial");

        let server = TestServer::start();
        let base = server.base_url();

        let zip_url = registry::coreml_zip_url_with_base(&base, size).unwrap();
        let zip_url_path = path_of(&zip_url);
        server.set_route(zip_url_path.clone(), Canned::forced_416());

        let tree_url_path = path_of(&tree_api_url(&base, repo));
        let tree_json = format!(
            r#"[{{"path":"{zip_file_name}","lfs":{{"oid":"{}"}}}}]"#,
            sha256_bytes(&zip_bytes)
        );
        server.set_route(tree_url_path, Canned::ok(tree_json.into_bytes()));

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        downloader
            .ensure_coreml_encoder(size, &mut |_| {}, &cancel)
            .expect("ensure_coreml_encoder should recover from 416 by verifying and promoting");

        assert_eq!(coreml_state(dir.path(), size), CoremlState::Complete);
        assert!(!partial_zip_path.exists());
        assert!(
            !zip_path.exists(),
            "zip should be cleaned up after extraction"
        );

        let zip_requests: Vec<_> = server
            .requests()
            .into_iter()
            .filter(|r| r.path == zip_url_path)
            .collect();
        assert_eq!(
            zip_requests.len(),
            1,
            "the verified partial should be promoted without a re-download: {zip_requests:?}"
        );
    }

    /// Ticket b (continued): if the restart-fresh attempt *also* gets a 416,
    /// the result must be a hard error — never a second recovery attempt
    /// (no infinite 416 loop).
    #[test]
    fn ensure_coreml_encoder_416_after_restart_errors_without_looping() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;

        let zip_bytes = build_test_zip();
        let zip_path = dir
            .path()
            .join(format!("ggml-{}-encoder.mlmodelc.zip", size.ggml_name()));
        let partial_zip_path = partial_path_for(&zip_path);
        std::fs::write(&partial_zip_path, &zip_bytes).expect("write partial");

        let server = TestServer::start();
        let base = server.base_url();
        let zip_url = registry::coreml_zip_url_with_base(&base, size).unwrap();
        let zip_url_path = path_of(&zip_url);
        // No tree-api route registered, so the digest lookup 404s and the
        // partial is treated as unverifiable — recovery discards it and
        // restarts fresh exactly once. Every request to this path (ranged or
        // not) gets 416, so that restart also fails.
        server.set_route(zip_url_path.clone(), Canned::forced_416());

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let err = downloader
            .ensure_coreml_encoder(size, &mut |_| {}, &cancel)
            .expect_err("a second 416 after restart should error, not loop forever");
        assert!(matches!(err, ModelError::Http { .. }));
        assert_eq!(coreml_state(dir.path(), size), CoremlState::Missing);

        let zip_requests: Vec<_> = server
            .requests()
            .into_iter()
            .filter(|r| r.path == zip_url_path)
            .collect();
        assert_eq!(
            zip_requests.len(),
            2,
            "expected exactly one ranged retry then one non-ranged restart, no more: {zip_requests:?}"
        );
        assert!(
            zip_requests[0].range.is_some(),
            "first request should carry Range"
        );
        assert!(
            zip_requests[1].range.is_none(),
            "restart-fresh request should carry no Range"
        );
    }

    /// Ticket c: a corrupt zip downloads fine (HTTP 200) but fails
    /// extraction — the failure must propagate without leaving a `Complete`
    /// state, and a second run against a good zip must issue a fresh GET
    /// (proving the failed placement didn't wedge the cache) and complete.
    #[test]
    fn ensure_coreml_encoder_placement_failure_redownloads_on_next_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;

        let server = TestServer::start();
        let base = server.base_url();
        let zip_url = registry::coreml_zip_url_with_base(&base, size).unwrap();
        let zip_url_path = path_of(&zip_url);

        server.set_route(
            zip_url_path.clone(),
            Canned::ok(b"not a real zip archive".to_vec()),
        );

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        downloader
            .ensure_coreml_encoder(size, &mut |_| {}, &cancel)
            .expect_err("a corrupt zip should fail extraction");
        assert_eq!(
            coreml_state(dir.path(), size),
            CoremlState::Missing,
            "a failed placement must not leave a Complete state"
        );

        server.set_route(zip_url_path.clone(), Canned::ok(build_test_zip()));
        downloader
            .ensure_coreml_encoder(size, &mut |_| {}, &cancel)
            .expect("a fresh download of a good zip should complete");
        assert_eq!(coreml_state(dir.path(), size), CoremlState::Complete);

        let zip_requests: Vec<_> = server
            .requests()
            .into_iter()
            .filter(|r| r.path == zip_url_path)
            .collect();
        assert_eq!(
            zip_requests.len(),
            2,
            "a failed placement must not wedge the cache — the second run must re-fetch: {zip_requests:?}"
        );
        assert!(
            zip_requests[1].range.is_none(),
            "no partial survives a failed placement, so the second fetch should be fresh"
        );
    }

    /// Happy-path seam test: a full `resolve_model` (ggml + CoreML zip, both
    /// with real SHA verification) served entirely by the local server —
    /// proving the whole flow is testable, and future-upgrade-testable,
    /// offline.
    #[test]
    fn resolve_model_happy_path_full_seam_from_local_server() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;
        let repo = repo_for(size).unwrap();

        let ggml_bytes = b"fake ggml model bytes for the happy path".to_vec();
        let zip_bytes = build_test_zip();
        let ggml_file = ggml_file_name(size);
        let zip_file = format!("ggml-{}-encoder.mlmodelc.zip", size.ggml_name());

        let server = TestServer::start();
        let base = server.base_url();

        let ggml_url_path = path_of(&registry::ggml_url_with_base(&base, size).unwrap());
        server.set_route(ggml_url_path.clone(), Canned::ok(ggml_bytes.clone()));

        let zip_url_path = path_of(&registry::coreml_zip_url_with_base(&base, size).unwrap());
        server.set_route(zip_url_path.clone(), Canned::ok(zip_bytes.clone()));

        let tree_url_path = path_of(&tree_api_url(&base, repo));
        let tree_json = format!(
            r#"[{{"path":"{ggml_file}","lfs":{{"oid":"{}"}}}},{{"path":"{zip_file}","lfs":{{"oid":"{}"}}}}]"#,
            sha256_bytes(&ggml_bytes),
            sha256_bytes(&zip_bytes),
        );
        server.set_route(tree_url_path, Canned::ok(tree_json.into_bytes()));

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let resolved = downloader
            .resolve_model(size, true, &mut |_| {}, &cancel)
            .expect("resolve_model should succeed fully offline via the local server");

        assert_eq!(resolved.path, dir.path().join(&ggml_file));
        assert!(!resolved.coreml_degraded);
        assert!(resolved.path.is_file());
        assert!(sentinel_path_for(&resolved.path).is_file());
        assert_eq!(coreml_state(dir.path(), size), CoremlState::Complete);

        let requests = server.requests();
        assert!(requests.iter().any(|r| r.path == ggml_url_path));
        assert!(requests.iter().any(|r| r.path == zip_url_path));
    }

    // --- Custom models: ggml URL + HF Whisper-family repo id resolution
    // (M2 Task 8, F-042 full, Q-2). ---

    #[test]
    fn select_ggml_candidates_prefers_ggml_prefixed_bin_files() {
        let files = vec![
            "ggml-model.bin".to_string(),
            "pytorch_model.bin".to_string(),
            "README.md".to_string(),
        ];
        assert_eq!(select_ggml_candidates(&files), vec!["ggml-model.bin"]);
    }

    #[test]
    fn select_ggml_candidates_falls_back_to_any_bin_file() {
        let files = vec!["pytorch_model.bin".to_string(), "config.json".to_string()];
        assert_eq!(select_ggml_candidates(&files), vec!["pytorch_model.bin"]);
    }

    #[test]
    fn select_ggml_candidates_empty_when_no_bin_files() {
        let files = vec!["config.json".to_string(), "README.md".to_string()];
        assert!(select_ggml_candidates(&files).is_empty());
    }

    #[test]
    fn select_ggml_candidates_lists_every_ggml_match() {
        let files = vec![
            "ggml-a.bin".to_string(),
            "ggml-b.bin".to_string(),
            "README.md".to_string(),
        ];
        assert_eq!(
            select_ggml_candidates(&files),
            vec!["ggml-a.bin".to_string(), "ggml-b.bin".to_string()]
        );
    }

    /// `ensure_custom_model` resumes an interrupted download (a `.partial`
    /// left over from a prior attempt) via `Range`, verifies no SHA is
    /// requested (no `sha_repo`), writes the sentinel, and a second call is a
    /// network-free fast path.
    #[test]
    fn ensure_custom_model_downloads_resumes_and_fast_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let full_bytes = b"totally-fake-custom-ggml-bytes-for-testing".to_vec();

        let server = TestServer::start();
        let custom_path = "/someuser/somerepo/resolve/main/custom-model.bin";
        server.set_route(custom_path, Canned::ok(full_bytes.clone()));
        let url = format!("{}{custom_path}", server.base_url());

        let downloader = ModelDownloader::with_hf_base(dir.path(), server.base_url());
        let cancel = CancelToken::new();

        // Simulate an interrupted prior attempt: half the bytes already sit
        // in `.partial`.
        let final_path = dir.path().join(registry::custom_model_file_name());
        let partial_path = partial_path_for(&final_path);
        let half_len = full_bytes.len() / 2;
        std::fs::write(&partial_path, &full_bytes[..half_len]).expect("seed partial");

        let mut updates = Vec::new();
        let resumed_path = downloader
            .ensure_custom_model(&url, &mut |p| updates.push(p), &cancel)
            .expect("ensure_custom_model should resume and complete");

        assert_eq!(resumed_path, final_path);
        assert_eq!(std::fs::read(&resumed_path).unwrap(), full_bytes);
        assert!(sentinel_path_for(&final_path).is_file());
        assert!(!updates.is_empty());

        let requests = server.requests();
        assert_eq!(
            requests.len(),
            1,
            "exactly one GET for the resume: {requests:?}"
        );
        assert_eq!(
            requests[0].range.as_deref(),
            Some(format!("bytes={half_len}-")).as_deref()
        );

        // Fast path: sentinel + file present -> zero further requests.
        let second = downloader
            .ensure_custom_model(&url, &mut |_| {}, &cancel)
            .expect("second call should hit the fast path");
        assert_eq!(second, final_path);
        assert_eq!(
            server.requests().len(),
            1,
            "fast path must not touch the network"
        );
    }

    /// F-087 (M8): all 10 diarization model files download and each gets its
    /// own completeness sentinel; a second call is a pure fast path (no
    /// further network requests at all — not even a sha-digest lookup, since
    /// `ensure_artifact`'s sentinel check short-circuits before building any
    /// URL).
    ///
    /// Binds TCP (`TestServer::start`) — blocked under the sandbox, run with
    /// it lifted (see this task's report for the full pre-existing set).
    #[test]
    fn ensure_diarization_models_downloads_all_and_writes_sentinels() {
        let dir = tempfile::tempdir().expect("tempdir");
        let server = TestServer::start();

        // Every diarization file gets distinct (fake, small) bytes so a mixup
        // between files would be caught by the content assertions below.
        for (i, file_name) in registry::DIARIZE_MODEL_FILES.iter().enumerate() {
            let path = format!("/{}/resolve/main/{file_name}", registry::DIARIZE_MODEL_REPO);
            server.set_route(path, Canned::ok(format!("fake-bytes-{i}").into_bytes()));
        }

        let downloader = ModelDownloader::with_hf_base(dir.path(), server.base_url());
        let cancel = CancelToken::new();

        let mut updates = Vec::new();
        let paths = downloader
            .ensure_diarization_models(&mut |p| updates.push(p), &cancel)
            .expect("ensure_diarization_models should download every file");

        assert_eq!(paths.dir, dir.path().join("diarization"));
        assert!(!updates.is_empty());

        for (i, (file_name, path)) in paths.files().into_iter().enumerate() {
            assert!(path.is_file(), "{file_name} should exist at {path:?}");
            assert_eq!(
                std::fs::read(path).unwrap(),
                format!("fake-bytes-{i}").into_bytes(),
                "{file_name} content"
            );
            assert!(
                sentinel_path_for(path).is_file(),
                "{file_name} should have a completeness sentinel"
            );
        }

        let requests_after_first_call = server.requests().len();
        assert!(requests_after_first_call > 0);

        // Fast path: every file + sentinel already present -> the second
        // call touches the network not at all (not even the file GETs let
        // alone the sha-digest lookups).
        let second = downloader
            .ensure_diarization_models(&mut |_| {}, &cancel)
            .expect("second call should hit the fast path for every file");
        assert_eq!(second, paths);
        assert_eq!(
            server.requests().len(),
            requests_after_first_call,
            "fast path must not touch the network"
        );
    }

    /// Carried M3 ticket (spec §8.1, step 1): two threads calling
    /// `ensure_artifact` (via `ensure_custom_model`, which — like `ensure_ggml`/
    /// `ensure_vad_model` — routes every download through it) for the *same*
    /// final path must serialize on the per-artifact lock, not each start
    /// their own download. A `Barrier` releases both threads at essentially
    /// the same instant so the missing-lock bug gets a real chance to race
    /// (rather than relying on incidental thread-spawn timing to ever
    /// overlap); with the lock in place the outcome is deterministic either
    /// way: exactly one full download, both calls `Ok`.
    ///
    /// Binds TCP (`TestServer::start`) — blocked under the sandbox, run with
    /// it lifted (see this task's report for the full pre-existing set).
    #[test]
    fn concurrent_ensure_of_same_artifact_downloads_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let full_bytes = b"totally-fake-concurrent-download-bytes-for-testing".to_vec();

        let server = TestServer::start();
        let custom_path = "/someuser/somerepo/resolve/main/concurrent-model.bin";
        server.set_route(custom_path, Canned::ok(full_bytes.clone()));
        let url = format!("{}{custom_path}", server.base_url());

        let downloader = Arc::new(ModelDownloader::with_hf_base(dir.path(), server.base_url()));
        let barrier = Arc::new(std::sync::Barrier::new(2));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let downloader = Arc::clone(&downloader);
                let url = url.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let cancel = CancelToken::new();
                    barrier.wait();
                    downloader.ensure_custom_model(&url, &mut |_| {}, &cancel)
                })
            })
            .collect();

        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("thread panicked"))
            .collect();
        for result in &results {
            assert!(
                result.is_ok(),
                "both concurrent callers must succeed: {result:?}"
            );
        }

        let requests = server.requests();
        let full_downloads = requests.iter().filter(|r| r.path == custom_path).count();
        assert_eq!(
            full_downloads, 1,
            "the per-artifact lock must serialize concurrent ensures into exactly one \
             download, with the second comer taking the fast path: {requests:?}"
        );

        let final_path = dir.path().join(registry::custom_model_file_name());
        assert_eq!(std::fs::read(&final_path).unwrap(), full_bytes);
        assert!(sentinel_path_for(&final_path).is_file());
    }

    /// `openai/whisper-tiny` maps to `ModelSize::Tiny` and delegates to
    /// `resolve_model`, fetching the standard whisper.cpp ggml (proving the
    /// Q-2 "ggml equivalents" mapping works, not a third-party repo fetch).
    #[test]
    fn resolve_hf_repo_openai_prefix_delegates_to_resolve_model() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Tiny;
        let ggml_bytes = b"fake tiny ggml bytes".to_vec();

        let server = TestServer::start();
        let base = server.base_url();
        let ggml_url_path = path_of(&registry::ggml_url_with_base(&base, size).unwrap());
        server.set_route(ggml_url_path.clone(), Canned::ok(ggml_bytes.clone()));

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let resolved = downloader
            .resolve_hf_repo("openai/whisper-tiny", false, &mut |_| {}, &cancel)
            .expect("openai/whisper-tiny should resolve via the ggml equivalent");

        assert_eq!(resolved.path, dir.path().join(ggml_file_name(size)));
        assert!(!resolved.coreml_degraded);
        assert!(
            server.requests().iter().any(|r| r.path == ggml_url_path),
            "expected a GET for ggml-tiny.bin at the whisper.cpp repo path: {:?}",
            server.requests()
        );
    }

    /// A third-party repo whose file listing has exactly one ggml candidate
    /// downloads it to `ggml-hf-<repo>.bin`, with no SHA verification and no
    /// CoreML attempt (even though `coreml_encoder` is requested).
    #[test]
    fn resolve_hf_repo_single_candidate_downloads_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = "acme/whisper-x";
        let server = TestServer::start();
        let base = server.base_url();

        let api_path = format!("/api/models/{repo}");
        server.set_route(
            api_path.clone(),
            Canned::ok(
                br#"{"siblings":[{"rfilename":"ggml-model.bin"},{"rfilename":"README.md"}]}"#
                    .to_vec(),
            ),
        );

        let model_bytes = b"fake third-party ggml bytes".to_vec();
        let resolve_path = format!("/{repo}/resolve/main/ggml-model.bin");
        server.set_route(resolve_path.clone(), Canned::ok(model_bytes.clone()));

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let resolved = downloader
            .resolve_hf_repo(repo, true, &mut |_| {}, &cancel)
            .expect("single candidate should download");

        let expected_path = dir.path().join(registry::hf_repo_model_file_name(repo));
        assert_eq!(resolved.path, expected_path);
        assert!(
            !resolved.coreml_degraded,
            "third-party repos never get CoreML, even when requested"
        );
        assert_eq!(std::fs::read(&resolved.path).unwrap(), model_bytes);
        assert!(sentinel_path_for(&resolved.path).is_file());

        let requests = server.requests();
        assert!(requests.iter().any(|r| r.path == api_path));
        assert!(requests.iter().any(|r| r.path == resolve_path));
        assert_eq!(
            requests.len(),
            2,
            "no CoreML zip request should ever be attempted for a third-party repo: {requests:?}"
        );
    }

    /// Two `.bin` candidates: `Unsupported`, naming both files and pointing
    /// at the custom-URL mechanism to pick one explicitly.
    #[test]
    fn resolve_hf_repo_multiple_candidates_lists_them_in_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = "acme/multi";
        let server = TestServer::start();
        let base = server.base_url();
        server.set_route(
            format!("/api/models/{repo}"),
            Canned::ok(
                br#"{"siblings":[{"rfilename":"ggml-a.bin"},{"rfilename":"ggml-b.bin"}]}"#.to_vec(),
            ),
        );

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let err = downloader
            .resolve_hf_repo(repo, false, &mut |_| {}, &cancel)
            .expect_err("multiple candidates must not auto-pick one");

        let ModelError::Unsupported { detail } = err else {
            panic!("expected Unsupported, got {err:?}");
        };
        assert!(detail.contains("ggml-a.bin"), "{detail}");
        assert!(detail.contains("ggml-b.bin"), "{detail}");
        assert!(
            detail.contains("custom_model_url") || detail.contains("custom-model-url"),
            "should point at the custom-URL mechanism: {detail}"
        );
    }

    /// Zero `.bin` candidates: `Unsupported`, explaining the ggml-only /
    /// MMS-out-of-scope cut and pointing at conversion docs + the custom-URL
    /// mechanism.
    #[test]
    fn resolve_hf_repo_zero_candidates_explains_scope_cut() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = "facebook/mms-1b-all";
        let server = TestServer::start();
        let base = server.base_url();
        server.set_route(
            format!("/api/models/{repo}"),
            Canned::ok(
                br#"{"siblings":[{"rfilename":"config.json"},{"rfilename":"model.safetensors"}]}"#
                    .to_vec(),
            ),
        );

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let err = downloader
            .resolve_hf_repo(repo, false, &mut |_| {}, &cancel)
            .expect_err("no ggml/.bin files means no whisper.cpp-compatible model");

        let ModelError::Unsupported { detail } = err else {
            panic!("expected Unsupported, got {err:?}");
        };
        assert!(detail.contains("ggml"), "{detail}");
        assert!(
            detail.contains("MMS") || detail.contains("non-Whisper"),
            "should say MMS/non-Whisper repos are out of scope: {detail}"
        );
        assert!(
            detail.contains("whisper.cpp") && detail.contains("conversion"),
            "should point at whisper.cpp's conversion docs: {detail}"
        );
        assert!(
            detail.contains("custom_model_url") || detail.contains("custom-model-url"),
            "should point at the custom-URL mechanism: {detail}"
        );
    }

    /// A non-2xx response from the repo-listing API is a transport-class
    /// failure.
    #[test]
    fn resolve_hf_repo_api_404_is_http_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = "does/not-exist";
        let server = TestServer::start();
        let base = server.base_url();
        server.set_route(format!("/api/models/{repo}"), Canned::status(404));

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let err = downloader
            .resolve_hf_repo(repo, false, &mut |_| {}, &cancel)
            .expect_err("404 should be a transport-class failure");
        assert!(matches!(err, ModelError::Http { .. }));
    }

    /// A malformed API response body (not the expected JSON shape) is also a
    /// transport-class failure, naming the repo.
    #[test]
    fn resolve_hf_repo_malformed_api_json_is_http_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = "acme/broken";
        let server = TestServer::start();
        let base = server.base_url();
        server.set_route(
            format!("/api/models/{repo}"),
            Canned::ok(b"not json".to_vec()),
        );

        let downloader = ModelDownloader::with_hf_base(dir.path(), base);
        let cancel = CancelToken::new();
        let err = downloader
            .resolve_hf_repo(repo, false, &mut |_| {}, &cancel)
            .expect_err("malformed API response should be a transport-class failure");
        let ModelError::Http { detail } = err else {
            panic!("expected Http, got {err:?}");
        };
        assert!(detail.contains(repo), "{detail}");
    }
}
