//! [`OpenAiEngine`] — humm's OpenAI-compatible API file-transcription engine
//! (F-041, design spec §3), the second [`TranscriptionEngine`] and ADR-008's
//! final shipped backend.
//!
//! Ported from Buzz's `OpenAIWhisperAPIFileTranscriber`
//! (`buzz/transcriber/openai_whisper_api_file_transcriber.py:38-183`) with two
//! deliberate mechanism deviations documented in the design spec's ledger:
//!
//! - **Ledger #2:** the engine receives already-decoded 16 kHz mono [`Pcm`]
//!   and uploads an mp3 encoded from *that* (via the [`AudioEncoder`]
//!   boundary), rather than re-encoding the original media file as Buzz does
//!   (`:65-94`). Whisper-family APIs downmix/resample to 16 kHz mono anyway,
//!   so the model-visible content is identical and the upload is smaller.
//! - **Ledger #3:** files over [`MAX_CHUNK_SIZE`] are split by slicing the
//!   **PCM** into equal-duration sample ranges ([`chunk_plan`]) and encoding
//!   each slice on its own, instead of Buzz's `ffmpeg -c copy` split of the
//!   already-encoded mp3 (`:145-171`). Same chunk-count/duration math,
//!   sample-exact (rather than mp3-frame-imprecise) boundaries, one fewer
//!   ffmpeg subprocess mode.
//!
//! The pure response parsing lives in [`super::parse`]; the shared HTTP
//! plumbing (base-URL normalization, the redacting [`OpenAiAuth`] credential,
//! failure mapping, and the run-on-its-own-thread cancel mechanism) lives in
//! [`super::client`]. The API key travels only as the in-memory `String`
//! inside [`OpenAiAuth`]: constructor argument → `Authorization: Bearer`
//! header, never logged, serialized, `Debug`-printed, or placed in an error
//! message (design spec §2).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::boundary::{
    AudioEncoder, EngineCaps, EngineOptions, Pcm, SAMPLE_RATE, SegmentSink, TranscriptionEngine,
};
use crate::cancel::CancelToken;
use crate::domain::{Segment, TaskKind};
use crate::error::{EngineError, MediaError};

use super::client::{ApiFailure, OpenAiAuth, cancelable, effective_base};
use super::parse::{parse_response, response_format_for};

/// The upload-size ceiling above which a file is split into chunks, in bytes:
/// 25 MiB, OpenAI's documented request limit and Buzz's `max_chunk_size`
/// (`openai_whisper_api_file_transcriber.py:127`).
pub const MAX_CHUNK_SIZE: u64 = 25 * 1024 * 1024;

/// Construction parameters for an [`OpenAiEngine`] (design spec §3): the
/// endpoint credentials and the API model id.
pub struct OpenAiEngineConfig {
    /// The endpoint credentials — optional custom base URL and the bearer
    /// API key. Its `Debug` impl redacts the key (see [`OpenAiAuth`]).
    pub auth: OpenAiAuth,
    /// The API model id, humm's mapping of Buzz's `transcriber/openai-api-model`
    /// setting (default `"whisper-1"`, `:51-53`). Drives the request's
    /// `model` part and, via [`response_format_for`], its `response_format`.
    pub api_model: String,
}

/// humm's OpenAI-compatible API file-transcription engine (F-041).
///
/// See the [module docs](self) for the port's provenance and deviations.
/// Construct with [`OpenAiEngine::new`]; drive it through the
/// [`TranscriptionEngine`] trait.
pub struct OpenAiEngine {
    cfg: OpenAiEngineConfig,
    encoder: Arc<dyn AudioEncoder>,
    client: reqwest::blocking::Client,
}

impl OpenAiEngine {
    /// Per-request timeout for every transcription/translation upload: 600 s,
    /// openai-python's default that Buzz inherits with `max_retries=0`
    /// (design spec §3.3). No retries.
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

    /// Builds an engine from `cfg` and an [`AudioEncoder`] (the ffmpeg mp3
    /// encoder in production; a fake in tests).
    pub fn new(cfg: OpenAiEngineConfig, encoder: Arc<dyn AudioEncoder>) -> Self {
        // A blocking client with only a timeout set is documented-impossible
        // to fail to build; on the theoretical error we fall back to the
        // default client rather than panicking — the non-`expect` pattern
        // `live::export`/`telemetry` already use for reqwest client builds.
        let client = reqwest::blocking::Client::builder()
            .timeout(Self::REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        OpenAiEngine {
            cfg,
            encoder,
            client,
        }
    }

    /// Uploads a single already-encoded mp3 to the transcriptions
    /// (or translations) endpoint and parses its response, applying
    /// `offset_ms` to every produced timestamp (design spec §3.3/§3.4).
    ///
    /// The whole request runs through [`cancelable`] so a mid-request cancel
    /// is acknowledged within ~100 ms (§8/ADR-022).
    fn transcribe_one(
        &self,
        mp3_path: &Path,
        options: &EngineOptions,
        offset_ms: i64,
        cancel: &CancelToken,
    ) -> Result<Vec<Segment>, EngineError> {
        let bytes = std::fs::read(mp3_path).map_err(|e| EngineError::Api {
            detail: format!(
                "could not read encoded upload file {}: {e}",
                mp3_path.display()
            ),
        })?;

        let base = effective_base(&self.cfg.auth.base_url);
        // Buzz `:208-215`: transcribe → `/audio/transcriptions`,
        // translate → `/audio/translations`.
        let endpoint = match options.task {
            TaskKind::Transcribe => "audio/transcriptions",
            TaskKind::Translate => "audio/translations",
        };
        let url = format!("{base}/{endpoint}");
        let host = host_of(&url);

        let mut form = reqwest::blocking::multipart::Form::new()
            .part(
                "file",
                reqwest::blocking::multipart::Part::bytes(bytes)
                    .file_name("audio.mp3")
                    .mime_str("audio/mpeg")
                    .expect("\"audio/mpeg\" is a valid MIME type"),
            )
            .text("model", self.cfg.api_model.clone())
            .text("response_format", response_format_for(&self.cfg.api_model))
            // Buzz sends `prompt` unconditionally (`:202`).
            .text("prompt", options.initial_prompt.clone());

        // `language` goes only to the transcriptions endpoint, and only when
        // a language was chosen: Buzz passes `language=None` through the SDK
        // for auto-detect (which omits the field), and the translations
        // endpoint never receives it (`:208-215`).
        if options.task == TaskKind::Transcribe
            && let Some(language) = options.language
        {
            form = form.text("language", language.code());
        }

        // Buzz `:205-206`: request word-granularity timings when asked.
        if options.word_timestamps {
            form = form.text("timestamp_granularities[]", "word");
        }

        let client = self.client.clone();
        let api_key = self.cfg.auth.api_key.clone();
        let word_timestamps = options.word_timestamps;
        let url_for_send = url.clone();

        let result = cancelable(cancel, move || {
            let response = client
                .post(&url_for_send)
                .bearer_auth(&api_key)
                .multipart(form)
                .send()
                .map_err(|e| ApiFailure::Http {
                    detail: format!("request to {host} failed: {e}"),
                })?;

            let status = response.status();
            let body = response.text().map_err(|e| ApiFailure::Http {
                detail: format!("failed reading response from {host}: {e}"),
            })?;

            if !status.is_success() {
                return Err(ApiFailure::Http {
                    detail: format!(
                        "{host} returned HTTP {status} — check the API key and base URL. \
                         Response: {}",
                        body_excerpt(&body)
                    ),
                });
            }

            let value: Value = serde_json::from_str(&body).map_err(|e| ApiFailure::Shape {
                detail: format!("{host} returned a non-JSON response body (HTTP {status}): {e}"),
            })?;

            parse_response(&value, offset_ms, word_timestamps)
        });

        result.map_err(map_api_failure)
    }
}

impl TranscriptionEngine for OpenAiEngine {
    /// Transcribes `audio` by encoding it to mp3 and uploading it (in one or
    /// more 25 MiB-bounded chunks) to the OpenAI-compatible endpoint,
    /// streaming parsed segments to `sink` in order.
    ///
    /// Ported from Buzz's `transcribe` (`:58-183`): emits the initial
    /// `on_progress(0.0)` (Buzz's `progress.emit((0, 100))`), then either a
    /// single request (files under [`MAX_CHUNK_SIZE`], with *no* trailing
    /// progress — Buzz's single-file arm emits only the initial value) or
    /// per-chunk requests with `on_progress((i + 1) / n)` after each.
    /// Cooperative cancel is checked before any work and between chunks, and
    /// inside every request via [`cancelable`] (design spec §3.2, ledger #4 —
    /// Buzz's `stop()` is a no-op). Every temporary mp3 is removed on every
    /// exit path (success, error, cancel, panic) by [`TempMp3`]'s `Drop`.
    fn transcribe(
        &mut self,
        audio: &Pcm,
        options: &EngineOptions,
        sink: &mut dyn SegmentSink,
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        // Cooperative cancel before any work: a pre-canceled token encodes
        // nothing and makes zero HTTP requests.
        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }

        // 1. Encode the whole PCM to a temp mp3 (`TempMp3`'s Drop deletes it
        //    on every exit path — Buzz `:65-72`, our upload source).
        let whole = TempMp3::new();
        self.encoder
            .encode_mp3(audio, whole.path(), cancel)
            .map_err(map_encode_error)?;

        // 2. Buzz's `self.progress.emit((0, 100))` (`:129`).
        sink.on_progress(0.0);

        // 3. Size check (Buzz `:126-131`).
        let total_size = std::fs::metadata(whole.path())
            .map_err(|e| EngineError::Api {
                detail: format!("could not stat encoded upload file: {e}"),
            })?
            .len();

        if total_size < MAX_CHUNK_SIZE {
            // Single-file path (Buzz `:131-132`): one request at offset 0
            // and — matching Buzz — *no* trailing progress emit (only the
            // initial `(0, 100)` above).
            let segments = self.transcribe_one(whole.path(), options, 0, cancel)?;
            for segment in segments {
                sink.on_segment(segment);
            }
            return Ok(());
        }

        // Chunked path (Buzz `:134-183`). `chunk_plan` re-applies the size
        // check, so at exactly `MAX_CHUNK_SIZE` (not `< MAX`) this arm still
        // runs with a single-range plan and emits the trailing `1.0` —
        // exactly as Buzz's `total_size < max` guard sends the equal case
        // down its chunked loop.
        let plan = chunk_plan(total_size, audio.len());
        let num_chunks = plan.len();

        for (index, range) in plan.iter().enumerate() {
            // Cancel between chunks (design spec §3.2 step 4).
            if cancel.is_canceled() {
                return Err(EngineError::Canceled);
            }

            let slice = Pcm::new(audio.samples()[range.clone()].to_vec());
            let chunk = TempMp3::new();
            self.encoder
                .encode_mp3(&slice, chunk.path(), cancel)
                .map_err(map_encode_error)?;

            // Offset derived exactly from the chunk's start sample:
            // `samples / 16` = ms at 16 kHz (design spec §3.2).
            let offset_ms = offset_ms_for_sample(range.start);
            let segments = self.transcribe_one(chunk.path(), options, offset_ms, cancel)?;
            for segment in segments {
                sink.on_segment(segment);
            }

            // Delete the chunk file before emitting progress (Buzz order,
            // `:180-181`); `TempMp3`'s Drop also covers every error path
            // above.
            drop(chunk);

            sink.on_progress((index + 1) as f32 / num_chunks as f32);
        }

        Ok(())
    }

    /// The API engine supports word timestamps and translate-to-English, but
    /// not local VAD (design spec §3.4): VAD stays a local-engine phase,
    /// skipped for `Backend::OpenAiApi`.
    fn capabilities(&self) -> EngineCaps {
        EngineCaps {
            word_timestamps: true,
            translate: true,
            vad: false,
        }
    }
}

/// Splits a decoded PCM sample range into upload chunks, porting Buzz's
/// chunk math (`openai_whisper_api_file_transcriber.py:127-143`) onto the
/// sample domain (design spec §3.2, ledger #3).
///
/// - `total_size` — the size in bytes of the whole-file mp3 (the encoded
///   upload, which is what OpenAI's 25 MiB limit applies to).
/// - `pcm_len` — the sample count of the decoded PCM being uploaded.
///
/// When `total_size < `[`MAX_CHUNK_SIZE`] the whole file fits in one request
/// (`0..pcm_len`). Otherwise `num_chunks = ceil(total_size / MAX_CHUNK_SIZE)`
/// (Buzz `:136`) equal-duration slices tile `0..pcm_len` exactly with no
/// overlap: each of the first `num_chunks - 1` ranges spans
/// `pcm_len / num_chunks` samples (integer division), and the last range
/// takes the remainder up to `pcm_len`. At exactly `MAX_CHUNK_SIZE` this
/// yields a single range via the chunked arm (`ceil(MAX / MAX) = 1`), which
/// [`OpenAiEngine::transcribe`] relies on to reproduce Buzz's progress
/// behavior for that boundary.
pub fn chunk_plan(total_size: u64, pcm_len: usize) -> Vec<Range<usize>> {
    if total_size < MAX_CHUNK_SIZE {
        // A single range spanning the whole PCM (`std::iter::once` rather
        // than `vec![0..pcm_len]`, which reads as "the integers 0..pcm_len").
        return std::iter::once(0..pcm_len).collect();
    }

    let num_chunks = total_size.div_ceil(MAX_CHUNK_SIZE) as usize;
    let chunk_len = pcm_len / num_chunks;

    (0..num_chunks)
        .map(|i| {
            let start = i * chunk_len;
            let end = if i == num_chunks - 1 {
                pcm_len
            } else {
                (i + 1) * chunk_len
            };
            start..end
        })
        .collect()
}

/// Milliseconds at [`SAMPLE_RATE`] for a sample index: `samples / 16` at
/// 16 kHz (`samples * 1000 / 16000`), truncating toward zero — used to derive
/// a chunk's `offset_ms` exactly from its start sample rather than from a
/// re-derived float duration (design spec §3.2).
fn offset_ms_for_sample(start_sample: usize) -> i64 {
    start_sample as i64 / (SAMPLE_RATE as i64 / 1000)
}

/// Maps an [`ApiFailure`] from the request/parse path to an [`EngineError`]:
/// a cancel becomes [`EngineError::Canceled`]; every other failure becomes
/// [`EngineError::Api`], whose actionable detail already names the host and
/// (for HTTP responses) the status and a body excerpt, and never the key.
///
/// `pub(crate)` — reused verbatim by [`super::live::LiveApiTranscriber`],
/// which maps its own request/parse failures through the identical rule
/// (design spec §10's reuse-discipline note). Purely a visibility change
/// from this module's original private helper; behavior is untouched, so
/// this module's own tests stay green.
pub(crate) fn map_api_failure(failure: ApiFailure) -> EngineError {
    match failure {
        ApiFailure::Canceled => EngineError::Canceled,
        ApiFailure::Http { detail } | ApiFailure::Shape { detail } => EngineError::Api { detail },
    }
}

/// Maps a mp3-encode [`MediaError`] to an [`EngineError`]: a cancel during
/// encoding becomes [`EngineError::Canceled`]; every other encode failure
/// (ffmpeg missing, subprocess error, I/O) becomes [`EngineError::Api`] with
/// an actionable detail (the message text itself makes clear it was the
/// upload-preparation step).
fn map_encode_error(error: MediaError) -> EngineError {
    match error {
        MediaError::Canceled => EngineError::Canceled,
        other => EngineError::Api {
            detail: format!("could not encode audio for upload: {other}"),
        },
    }
}

/// The host (with port, when present) of `url`, for actionable error
/// messages — falls back to `url` verbatim if it doesn't parse. Never
/// includes the request's `Authorization` header, so it can never leak the
/// key (design spec §2).
///
/// `pub(crate)` — shared with [`super::live::LiveApiTranscriber`], the only
/// other caller building actionable request-failure messages the same way.
pub(crate) fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            parsed.host_str().map(|host| match parsed.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            })
        })
        .unwrap_or_else(|| url.to_string())
}

/// A short, char-boundary-safe excerpt of a response body for an error
/// message: the first ~300 characters, with an ellipsis when truncated.
///
/// `pub(crate)` — shared with [`super::live::LiveApiTranscriber`].
pub(crate) fn body_excerpt(body: &str) -> String {
    const MAX: usize = 300;
    if body.len() <= MAX {
        return body.to_string();
    }
    let mut end = MAX;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &body[..end])
}

/// A scope guard for a temporary mp3 upload file: its [`Drop`] removes the
/// file, so an encoded chunk/whole-file mp3 is cleaned up on *every* exit
/// path from [`OpenAiEngine::transcribe`] — success, error, cancel, or panic
/// (design spec §3.2 step 1). Names carry a distinctive `humm-openai-`
/// prefix so leaks are assertable.
struct TempMp3 {
    path: PathBuf,
}

impl TempMp3 {
    /// A fresh, unique temp path under [`std::env::temp_dir`] (uuid-named).
    /// No file is created until the encoder writes it.
    fn new() -> Self {
        let name = format!("humm-openai-{}.mp3", uuid::Uuid::new_v4());
        TempMp3 {
            path: std::env::temp_dir().join(name),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempMp3 {
    fn drop(&mut self) {
        // Best-effort: the file may not exist (e.g. the encoder failed
        // before writing it), which is fine — nothing to surface here.
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Millis;
    use crate::language::Language;
    use crate::models::test_server::{Canned, TestServer};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // A verbose_json body with a single non-word segment at 0.5s..1.0s.
    const VERBOSE_JSON: &[u8] = br#"{"segments":[{"start":0.5,"end":1.0,"text":"hello"}]}"#;

    // A process-wide guard serializing the mock-server tests. They all write
    // mp3 files into the *global* `std::env::temp_dir()` under the same
    // `humm-openai-` prefix, and the two cleanup tests assert no such file
    // remains — a claim only meaningful if no *other* engine test is mid-run
    // (holding a live temp file) at the same instant. Serializing makes the
    // cleanup assertion race-free without per-test temp dirs (the engine
    // hardcodes `temp_dir()`, matching Buzz).
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        // Tolerate poisoning: a sandbox `TestServer::start` bind panic while
        // the guard is held poisons it, but the lock state is still valid.
        TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// An [`AudioEncoder`] fake that writes a sparse file of a scripted size
    /// per call, so the whole-file encode's reported size drives the
    /// chunking path without invoking real ffmpeg. Call `i` uses `sizes[i]`,
    /// clamped to the last entry — so `[BIG, small]` means "big whole file,
    /// then small per-chunk files".
    struct FakeEncoder {
        sizes: Vec<u64>,
        calls: AtomicUsize,
    }

    impl FakeEncoder {
        fn new(sizes: Vec<u64>) -> Arc<Self> {
            Arc::new(FakeEncoder {
                sizes,
                calls: AtomicUsize::new(0),
            })
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl AudioEncoder for FakeEncoder {
        fn encode_mp3(
            &self,
            _pcm: &Pcm,
            dest: &Path,
            _cancel: &CancelToken,
        ) -> Result<(), MediaError> {
            let i = self.calls.fetch_add(1, Ordering::SeqCst);
            let size = *self
                .sizes
                .get(i)
                .or_else(|| self.sizes.last())
                .unwrap_or(&0);
            // A sparse file: `metadata().len()` reports `size` without
            // actually writing `size` bytes, so a >25 MiB "whole file" is
            // cheap to fabricate.
            let file = std::fs::File::create(dest)?;
            file.set_len(size)?;
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        segments: Vec<Segment>,
        progress: Vec<f32>,
    }

    impl SegmentSink for RecordingSink {
        fn on_segment(&mut self, segment: Segment) {
            self.segments.push(segment);
        }
        fn on_progress(&mut self, fraction: f32) {
            self.progress.push(fraction);
        }
    }

    const TEST_KEY: &str = "sk-secret-test-key";

    fn engine_for(
        server: &TestServer,
        encoder: Arc<dyn AudioEncoder>,
        api_model: &str,
    ) -> OpenAiEngine {
        OpenAiEngine::new(
            OpenAiEngineConfig {
                auth: OpenAiAuth {
                    base_url: Some(server.base_url()),
                    api_key: TEST_KEY.to_string(),
                },
                api_model: api_model.to_string(),
            },
            encoder,
        )
    }

    fn base_options(task: TaskKind) -> EngineOptions {
        EngineOptions {
            language: None,
            task,
            initial_prompt: "prime".to_string(),
            word_timestamps: false,
            temperature: vec![],
            vad_model: None,
            threads: None,
        }
    }

    /// Byte-subsequence search — a multipart body is arbitrary binary, so
    /// this can't be a `str` substring check.
    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty()
            && haystack
                .windows(needle.len())
                .any(|window| window == needle)
    }

    fn count_temp_mp3s() -> usize {
        std::fs::read_dir(std::env::temp_dir())
            .expect("read temp_dir")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("humm-openai-")
            })
            .count()
    }

    // -- pure: chunk_plan + offset math (sandbox-green) --------------------

    #[test]
    fn chunk_plan_under_limit_is_one_full_range() {
        let plan = chunk_plan(1024, 48_000);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0], 0..48_000);

        let boundary = chunk_plan(MAX_CHUNK_SIZE - 1, 10);
        assert_eq!(boundary.len(), 1);
        assert_eq!(boundary[0], 0..10);
    }

    #[test]
    fn chunk_plan_exactly_max_is_a_single_range_via_chunk_arm() {
        // Buzz `:131`: `total_size < max` is *false* at exactly MAX, so the
        // chunked arm runs — but ceil(MAX / MAX) = 1, so it is still one
        // range. Pinned because `transcribe`'s progress behavior at this
        // boundary depends on it.
        let plan = chunk_plan(MAX_CHUNK_SIZE, 40_000);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0], 0..40_000);
    }

    #[test]
    fn chunk_plan_just_over_max_is_two_equal_duration_ranges() {
        let plan = chunk_plan(MAX_CHUNK_SIZE + 1, 40_001);
        assert_eq!(plan.len(), 2);
        // Tiles 0..40_001 exactly, no overlap.
        assert_eq!(plan[0].start, 0);
        assert_eq!(plan[0].end, plan[1].start);
        assert_eq!(plan[1].end, 40_001);
        // Equal duration ±1 sample.
        let a = plan[0].end - plan[0].start;
        let b = plan[1].end - plan[1].start;
        assert!((a as i64 - b as i64).abs() <= 1, "chunk lengths {a} vs {b}");
    }

    #[test]
    fn chunk_plan_large_asymmetric_case_tiles_exactly_without_overlap() {
        // ceil(75 MiB / 25 MiB) = 3 chunks over an odd sample count.
        let total = 3 * MAX_CHUNK_SIZE;
        let pcm_len = 1_000_003;
        let plan = chunk_plan(total, pcm_len);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].start, 0);
        assert_eq!(plan[0].end, plan[1].start);
        assert_eq!(plan[1].end, plan[2].start);
        assert_eq!(plan[2].end, pcm_len);
        for range in &plan {
            assert!(range.start <= range.end);
        }
    }

    #[test]
    fn chunk_plan_ranges_cover_pcm_exactly_for_many_chunk_counts() {
        for &num_target in &[2usize, 3, 4, 5, 7, 10] {
            let total = num_target as u64 * MAX_CHUNK_SIZE;
            let pcm_len = 123_457; // prime, forces a remainder in the last chunk
            let plan = chunk_plan(total, pcm_len);
            assert_eq!(plan.len(), num_target, "for {num_target} chunks");
            assert_eq!(plan.first().unwrap().start, 0);
            assert_eq!(plan.last().unwrap().end, pcm_len);
            for pair in plan.windows(2) {
                assert_eq!(pair[0].end, pair[1].start, "gapless tiling");
            }
        }
    }

    #[test]
    fn offset_ms_for_sample_converts_at_16khz() {
        // 800_000 samples / 16 = 50_000 ms (design spec §3.2 offset math).
        assert_eq!(offset_ms_for_sample(800_000), 50_000);
        assert_eq!(offset_ms_for_sample(0), 0);
        assert_eq!(offset_ms_for_sample(16_000), 1000);
    }

    // -- mock-server tests (TCP) ------------------------------------------
    //
    // Bind TCP (`TestServer::start`) — blocked under the sandbox, run with
    // it lifted (see this task's report for the full pre-existing set).

    #[test]
    fn single_file_happy_path_streams_segments_and_only_initial_progress() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route("/audio/transcriptions", Canned::ok(VERBOSE_JSON.to_vec()));

        let encoder = FakeEncoder::new(vec![1024]); // < MAX → single file
        let mut engine = engine_for(&server, encoder, "whisper-1");
        let mut sink = RecordingSink::default();

        let result = engine.transcribe(
            &Pcm::new(vec![0.0; 16_000]),
            &base_options(TaskKind::Transcribe),
            &mut sink,
            &CancelToken::new(),
        );
        assert!(result.is_ok(), "transcribe failed: {result:?}");

        assert_eq!(sink.segments.len(), 1);
        assert_eq!(sink.segments[0].text, "hello");
        assert_eq!(sink.segments[0].start, Millis(500));

        // Buzz single-file emits only the initial (0, 100) — no trailing 1.0.
        assert_eq!(sink.progress, vec![0.0]);

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/audio/transcriptions");

        let body = &requests[0].body;
        assert!(contains(body, b"name=\"file\""));
        assert!(contains(body, b"filename=\"audio.mp3\""));
        assert!(contains(body, b"name=\"model\""));
        assert!(contains(body, b"whisper-1"));
        assert!(contains(body, b"name=\"response_format\""));
        assert!(contains(body, b"verbose_json"));
        assert!(contains(body, b"name=\"prompt\""));
        assert!(contains(body, b"prime"));

        assert_eq!(
            requests[0].headers.get("authorization").map(String::as_str),
            Some("Bearer sk-secret-test-key"),
        );
    }

    #[test]
    fn translate_task_hits_translations_endpoint_and_omits_language() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route("/audio/translations", Canned::ok(VERBOSE_JSON.to_vec()));

        let mut engine = engine_for(&server, FakeEncoder::new(vec![1024]), "whisper-1");
        let mut sink = RecordingSink::default();

        // A language is configured, but the translations endpoint must still
        // never receive a `language` part (Buzz `:208-215`).
        let mut options = base_options(TaskKind::Translate);
        options.language = Some(Language::parse("es").expect("es"));

        engine
            .transcribe(
                &Pcm::new(vec![0.0; 16_000]),
                &options,
                &mut sink,
                &CancelToken::new(),
            )
            .expect("ok");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/audio/translations");
        assert!(!contains(&requests[0].body, b"name=\"language\""));
    }

    #[test]
    fn language_part_present_for_transcribe_when_some() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route("/audio/transcriptions", Canned::ok(VERBOSE_JSON.to_vec()));

        let mut engine = engine_for(&server, FakeEncoder::new(vec![1024]), "whisper-1");
        let mut sink = RecordingSink::default();
        let mut options = base_options(TaskKind::Transcribe);
        options.language = Some(Language::parse("fr").expect("fr"));

        engine
            .transcribe(
                &Pcm::new(vec![0.0; 16_000]),
                &options,
                &mut sink,
                &CancelToken::new(),
            )
            .expect("ok");

        let body = &server.requests()[0].body;
        assert!(contains(body, b"name=\"language\""));
        assert!(contains(body, b"fr"));
    }

    #[test]
    fn language_part_absent_for_transcribe_when_none() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route("/audio/transcriptions", Canned::ok(VERBOSE_JSON.to_vec()));

        let mut engine = engine_for(&server, FakeEncoder::new(vec![1024]), "whisper-1");
        let mut sink = RecordingSink::default();
        let options = base_options(TaskKind::Transcribe); // language None

        engine
            .transcribe(
                &Pcm::new(vec![0.0; 16_000]),
                &options,
                &mut sink,
                &CancelToken::new(),
            )
            .expect("ok");

        assert!(!contains(&server.requests()[0].body, b"name=\"language\""));
    }

    #[test]
    fn timestamp_granularities_word_part_present_when_word_timestamps() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::ok(
                br#"{"segments":[{"start":0,"end":1,"text":"hi","words":[{"word":"hi","start":0.0,"end":0.5}]}]}"#
                    .to_vec(),
            ),
        );

        let mut engine = engine_for(&server, FakeEncoder::new(vec![1024]), "whisper-1");
        let mut sink = RecordingSink::default();
        let mut options = base_options(TaskKind::Transcribe);
        options.word_timestamps = true;

        engine
            .transcribe(
                &Pcm::new(vec![0.0; 16_000]),
                &options,
                &mut sink,
                &CancelToken::new(),
            )
            .expect("ok");

        let body = &server.requests()[0].body;
        assert!(contains(body, b"name=\"timestamp_granularities[]\""));
        assert!(contains(body, b"word"));
    }

    #[test]
    fn chunked_path_makes_n_requests_with_per_chunk_offsets_and_progress_steps() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route("/audio/transcriptions", Canned::ok(VERBOSE_JSON.to_vec()));

        // Whole-file encode reports > MAX → ceil((MAX+1)/MAX) = 2 chunks;
        // per-chunk encodes are small. pcm_len = 32_000 samples (2 s), so
        // chunk boundaries are 0..16_000 and 16_000..32_000 → offsets 0 ms
        // and 1000 ms.
        let encoder = FakeEncoder::new(vec![MAX_CHUNK_SIZE + 1, 1024]);
        let mut engine = engine_for(&server, encoder.clone(), "whisper-1");
        let mut sink = RecordingSink::default();

        engine
            .transcribe(
                &Pcm::new(vec![0.0; 32_000]),
                &base_options(TaskKind::Transcribe),
                &mut sink,
                &CancelToken::new(),
            )
            .expect("ok");

        // Two chunk requests (the whole-file encode makes no request).
        assert_eq!(server.requests().len(), 2);
        // Encoder called 3× total: whole file + 2 chunks.
        assert_eq!(encoder.call_count(), 3);

        // One segment per chunk: chunk 0 offset 0 → start 500; chunk 1
        // offset 1000 ms → start 1500 (the offset is visible in the times).
        assert_eq!(sink.segments.len(), 2);
        assert_eq!(sink.segments[0].start, Millis(500));
        assert_eq!(sink.segments[1].start, Millis(1500));

        // Progress: 0.0 (initial), then 1/2, then 2/2.
        assert_eq!(sink.progress, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn auth_failure_maps_to_api_error_naming_status_and_host_without_key() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::status_with_body(
                401,
                br#"{"error":{"message":"Incorrect API key provided"}}"#.to_vec(),
            ),
        );

        let mut engine = engine_for(&server, FakeEncoder::new(vec![1024]), "whisper-1");
        let mut sink = RecordingSink::default();

        let result = engine.transcribe(
            &Pcm::new(vec![0.0; 16_000]),
            &base_options(TaskKind::Transcribe),
            &mut sink,
            &CancelToken::new(),
        );

        let error = result.expect_err("a 401 must fail");
        assert!(matches!(error, EngineError::Api { .. }), "got {error:?}");
        let message = error.to_string();
        assert!(message.contains("401"), "must name the status: {message}");
        assert!(
            message.contains("127.0.0.1"),
            "must name the host: {message}"
        );
        assert!(
            !message.contains(TEST_KEY),
            "must not leak the key: {message}"
        );
    }

    #[test]
    fn pre_canceled_token_returns_canceled_with_zero_requests() {
        let _g = guard();
        let server = TestServer::start();
        server.set_route("/audio/transcriptions", Canned::ok(VERBOSE_JSON.to_vec()));

        let encoder = FakeEncoder::new(vec![1024]);
        let mut engine = engine_for(&server, encoder.clone(), "whisper-1");
        let mut sink = RecordingSink::default();

        let cancel = CancelToken::new();
        cancel.cancel();

        let result = engine.transcribe(
            &Pcm::new(vec![0.0; 16_000]),
            &base_options(TaskKind::Transcribe),
            &mut sink,
            &cancel,
        );

        assert!(
            matches!(result, Err(EngineError::Canceled)),
            "got {result:?}"
        );
        assert!(
            server.requests().is_empty(),
            "a pre-canceled run must make no HTTP request"
        );
        assert_eq!(
            encoder.call_count(),
            0,
            "a pre-canceled run must not even encode"
        );
        assert!(sink.progress.is_empty());
    }

    #[test]
    fn successful_run_leaves_no_temp_files() {
        let _g = guard();
        // The guard serializes engine tests, so no other engine test holds
        // a live temp file at this instant; `before` also absorbs any stray
        // leftover from an unrelated crashed run.
        let before = count_temp_mp3s();

        let server = TestServer::start();
        server.set_route("/audio/transcriptions", Canned::ok(VERBOSE_JSON.to_vec()));
        // Exercise the chunked path too (whole file + 2 chunk temp files).
        let mut engine = engine_for(
            &server,
            FakeEncoder::new(vec![MAX_CHUNK_SIZE + 1, 1024]),
            "whisper-1",
        );
        let mut sink = RecordingSink::default();

        engine
            .transcribe(
                &Pcm::new(vec![0.0; 32_000]),
                &base_options(TaskKind::Transcribe),
                &mut sink,
                &CancelToken::new(),
            )
            .expect("ok");

        assert_eq!(
            count_temp_mp3s(),
            before,
            "a successful run must leave no humm-openai- temp files behind"
        );
    }

    #[test]
    fn failed_run_leaves_no_temp_files() {
        let _g = guard();
        let before = count_temp_mp3s();

        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::status_with_body(401, b"nope".to_vec()),
        );
        let mut engine = engine_for(&server, FakeEncoder::new(vec![1024]), "whisper-1");
        let mut sink = RecordingSink::default();

        let result = engine.transcribe(
            &Pcm::new(vec![0.0; 16_000]),
            &base_options(TaskKind::Transcribe),
            &mut sink,
            &CancelToken::new(),
        );
        assert!(result.is_err());

        assert_eq!(
            count_temp_mp3s(),
            before,
            "even a failed run must clean up its temp files"
        );
    }
}
