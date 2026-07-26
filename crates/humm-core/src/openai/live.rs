//! [`LiveApiTranscriber`] — the per-chunk API transcriber for the F-061
//! live-transcription-via-API-backend closure (design spec §10, ledger
//! #L13). Task 18 composes this into the `EngineFn` closure
//! [`crate::live::session::run_session`] drives; this module owns only the
//! mechanism (WAV-encode-and-upload-a-chunk), never GUI wiring or settings
//! reads.
//!
//! Ported from Buzz's `RecordingTranscriber._transcribe_via_api`
//! (`buzz/transcriber/recording_transcriber.py:306-362`, reached only when
//! `ModelType.OPEN_AI_WHISPER_API` is selected, `:220-233`). Findings from
//! that read, with line references, since the design spec's own §10 sketch
//! is a summary, not a substitute for the source:
//!
//! - **WAV writer is `wave`, not `soundfile`.** `:309-316` uses the stdlib
//!   `wave` module (`import wave` at `:6`), not `soundfile` — see
//!   [`crate::wav`]'s module doc for the full correction and the exact
//!   header/PCM shape it writes (mono, 16-bit, the stream's sample rate,
//!   which is 16 kHz for humm's live pipeline).
//! - **`response_format` is hardcoded `"json"`** (`:322`), unconditionally —
//!   unlike [`super::engine::OpenAiEngine`]'s file-transcription path, which
//!   picks `"json"` vs `"verbose_json"` by model
//!   ([`super::parse::response_format_for`]). The live arm only ever wants
//!   `result["text"]`/`transcript.text`, never segment/word timing, so this
//!   port also hardcodes `"json"` rather than reusing
//!   `response_format_for` — a deliberate, documented non-reuse.
//! - **The rolling prompt is accepted as a parameter but never sent —
//!   likely a Buzz defect, not ported.** `_transcribe_via_api(self, samples,
//!   initial_prompt)` (`:306`) receives `initial_prompt` as its rolling-text
//!   parameter (the caller's loop rebinds its own `initial_prompt` local to
//!   each successful result's text, `:142`, and threads it through
//!   `_transcribe`, `:137,237,253`). But the request body at `:323` sends
//!   `"prompt": self.transcription_options.initial_prompt` — the **static**
//!   config value fixed for the whole session — never referencing the
//!   `initial_prompt` *parameter* at all. The same shadowing defect exists in
//!   `_transcribe_faster_whisper` (`:278`, same static-vs-parameter mixup);
//!   only `_transcribe_whisper` (`:261`) actually uses the passed rolling
//!   value. This reads as an oversight (the parameter exists and is
//!   correctly threaded by the caller, just never read), not a deliberate
//!   choice. House precedent is to fix accidental upstream corruption rather
//!   than port it byte-faithfully (`super::parse`'s `offset_ms` fix for the
//!   whisper.cpp branch cites the same principle) — [`Self::transcribe_chunk`]
//!   therefore sends whatever `prompt` its caller supplies, and
//!   [`crate::live::session::run_session`]'s loop already threads the
//!   rolling prompt uniformly for every backend (`session.rs`'s `prompt =
//!   text.clone()`), so Task 18's closure gets Buzz's evidently-intended
//!   behavior rather than its actual bug.
//! - **Task-based endpoint selection (closed, Task 18).** `:330-343` picks
//!   `/audio/transcriptions` when
//!   `self.transcription_options.task == Task.TRANSCRIBE` (or the request is
//!   actually headed to a local whisper.cpp server — irrelevant here,
//!   ADR-008 removed that half) and `/audio/translations` otherwise — i.e.
//!   Buzz's live-API arm **does** support the Translate task.
//!   [`Self::transcribe_chunk`] now takes a [`TaskKind`] parameter and
//!   applies the identical rule the file engine's
//!   [`super::engine::OpenAiEngine::transcribe`] already uses
//!   (`engine.rs:121-124`): `Transcribe` → `/audio/transcriptions`,
//!   `Translate` → `/audio/translations`. `language` is gated the same way
//!   Buzz's own call sites are (`:336-343`: the `language` kwarg is passed
//!   only to `transcriptions.create`, never to `translations.create`) —
//!   sent only when `task == Transcribe` **and** `language` is `Some`. This
//!   closes design spec ledger #L15 (the scope gap Task 17 recorded and left
//!   for this task to close, since the binding contract it was given fixed
//!   this exact signature with no task parameter at the time).
//! - **Response parsing reuses the existing shape-fallback, not a bespoke
//!   one.** `:345-353` joins `" ".join(segment["text"] for segment in ...)`
//!   when the body carries `segments`, else uses `transcript.text` directly.
//!   [`super::parse::parse_response`] (word-timestamps off) already produces
//!   exactly one [`crate::domain::Segment`] per `segments` array element, or
//!   a single pseudo-segment carrying the whole `text` field when no
//!   `segments` key is present — so calling it and joining the resulting
//!   segments' text with `" "` reproduces both of Buzz's branches: the
//!   join is exercised for the `segments` case, and is a no-op (one element)
//!   for the `text`-only case. No narrower bespoke parser was needed.
//! - **Error handling: humm's session ends on a chunk failure; Buzz's
//!   survives it by corrupting the transcript instead (fail-loud ruling,
//!   ledger #L17).** `:355-359` catches every exception from the API call
//!   and turns it into `{"text": f"Error: {e}"}` — surfaced as if it were
//!   transcribed text, and then fed straight back in as the *next* chunk's
//!   rolling prompt (`:142` unconditionally rebinds `initial_prompt` to
//!   whatever text just came back, fake string included) — when still
//!   running, or `{"text": ""}` when a Stop landed mid-request. The outer
//!   `while self.is_running` loop (`:108-151`) is never broken by a failed
//!   chunk; only `result is None` (which `_transcribe_via_api` never
//!   returns) would end it, so Buzz's session **keeps recording
//!   indefinitely** after an API failure, degrading into a stream of fake
//!   `"Error: ..."` segments built on an increasingly poisoned prompt.
//!   humm's `run_session` (`live/session.rs`) instead treats a genuine
//!   [`EngineError`] as loop-ending (`Err(err) => { sink(Error); break; }`)
//!   — the **session itself ends**, not merely its presentation of the
//!   failure — and only [`EngineError::Canceled`] is the silent,
//!   loop-continues-without-an-error-event case Buzz's `is_running`-gated
//!   branch mirrors. This loop-continues-vs-loop-ends divergence is a
//!   deliberate fail-loud ruling, not an accident of `run_session`'s
//!   pre-existing contract: Buzz's survival is actively harmful (a
//!   corrupted rolling prompt feeding every subsequent chunk), and humm
//!   already persists each accepted segment before a session can end
//!   (Q-10), so ending loud loses no completed work — a
//!   tolerate-N-consecutive-failures refinement is a candidate for M8, not
//!   this task. [`Self::transcribe_chunk`] only needs to return the right
//!   [`EngineError`] variant for each failure so `run_session`'s existing,
//!   already-verified contract (F-061's local-engine row) fires correctly —
//!   a cancel maps to [`EngineError::Canceled`] (silent stop, matching
//!   Buzz's `is_running == False` arm), everything else maps to
//!   [`EngineError::Api`], which ends the session (Buzz's `is_running ==
//!   True` arm would have kept going with corrupted output instead).

use std::time::Duration;

use serde_json::Value;

use crate::cancel::CancelToken;
use crate::domain::{Segment, TaskKind};
use crate::error::EngineError;
use crate::wav::write_wav_16k_mono_i16;

use super::client::{ApiFailure, build_client, cancelable, effective_base};
use super::engine::{OpenAiEngineConfig, body_excerpt, host_of, map_api_failure};
use super::parse::parse_response;

/// humm's per-chunk live-transcription API client (F-061 closure).
///
/// See the [module docs](self) for the port's provenance, findings, and
/// deviations. Construct with [`LiveApiTranscriber::new`]; Task 18 wraps
/// [`LiveApiTranscriber::transcribe_chunk`] in an `EngineFn` closure over the
/// live session's rolling prompt and shared [`CancelToken`].
pub struct LiveApiTranscriber {
    cfg: OpenAiEngineConfig,
    client: reqwest::blocking::Client,
}

impl LiveApiTranscriber {
    /// Per-request timeout for one chunk upload: 60 s.
    ///
    /// Buzz's live-API client (`recording_transcriber.py:224-228`) sets no
    /// explicit timeout, so it inherits the OpenAI Python SDK's own default
    /// (600 s — the same value [`super::engine::OpenAiEngine`] uses for its
    /// up-to-25-MiB file uploads, `engine.rs`'s `REQUEST_TIMEOUT`). A live
    /// chunk is a few seconds of 16 kHz mono audio (tens to low hundreds of
    /// KB once WAV-encoded) — nowhere near that scale — and a stuck request
    /// on this path blocks the *next* batch from ever being drained (the
    /// session loop calls the engine synchronously,
    /// `live/session.rs::run_session`), so a much tighter bound is more
    /// useful here than inheriting the file engine's allowance for a
    /// worst-case 25 MiB upload. 60 s is generous for a chunk this size while
    /// still failing a truly wedged request well before it could visibly
    /// stall a live session.
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

    /// Builds a transcriber from `cfg`, with its own bounded-timeout HTTP
    /// client (see [`Self::REQUEST_TIMEOUT`]).
    pub fn new(cfg: OpenAiEngineConfig) -> Self {
        let client = build_client(Self::REQUEST_TIMEOUT);
        LiveApiTranscriber { cfg, client }
    }

    /// Transcribes (or translates — see below) one live-recording chunk:
    /// encodes `pcm` (16 kHz mono samples) to an in-memory WAV
    /// ([`crate::wav::write_wav_16k_mono_i16`]) and uploads it to
    /// `{base}/audio/transcriptions` or `{base}/audio/translations`,
    /// returning the recognized text.
    ///
    /// `task` selects the endpoint exactly like the file engine's
    /// [`super::engine::OpenAiEngine::transcribe`] does (`engine.rs:121-124`,
    /// Buzz `recording_transcriber.py:330-343`, ledger #L15 closure):
    /// [`TaskKind::Transcribe`] → `/audio/transcriptions`,
    /// [`TaskKind::Translate`] → `/audio/translations`. `prompt` is sent as
    /// the request's `prompt` field only when non-empty; `language` is sent
    /// only when `task == Transcribe` **and** `Some` (the translations
    /// endpoint never receives a `language` part, matching Buzz's own
    /// `translations.create(**options)` call, which has no `language`
    /// kwarg at all) — see the [module docs](self) for why the prompt
    /// threading is what it is (Buzz's own apparent bug). The whole request
    /// runs through [`cancelable`], so a cancel mid-upload is acknowledged
    /// within ~100 ms and returns [`EngineError::Canceled`] — the same
    /// benign, silent-stop signal [`crate::live::session::run_session`]
    /// already expects from any engine closure.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Canceled`] if `cancel` was (or becomes)
    /// canceled before a result arrives, or [`EngineError::Api`] for any
    /// other transport/HTTP/shape failure, with an actionable detail naming
    /// the host and (for HTTP responses) the status and a body excerpt —
    /// never the API key.
    pub fn transcribe_chunk(
        &self,
        pcm: &[f32],
        prompt: &str,
        language: Option<&str>,
        task: TaskKind,
        cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        // Cooperative cancel before any work, matching
        // `OpenAiEngine::transcribe`'s own pre-check: a pre-canceled token
        // encodes nothing and makes zero HTTP requests.
        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }

        let mut wav_bytes = Vec::new();
        write_wav_16k_mono_i16(pcm, &mut wav_bytes).map_err(|e| EngineError::Api {
            detail: format!("failed to build in-memory WAV for upload: {e}"),
        })?;

        let base = effective_base(&self.cfg.auth.base_url);
        // Buzz `:330-343`: transcribe -> transcriptions, translate ->
        // translations (ledger #L15 closure; same rule as
        // `OpenAiEngine::transcribe_one`, `engine.rs:121-124`).
        let endpoint = match task {
            TaskKind::Transcribe => "audio/transcriptions",
            TaskKind::Translate => "audio/translations",
        };
        let url = format!("{base}/{endpoint}");
        let host = host_of(&url);

        let mut form = reqwest::blocking::multipart::Form::new()
            .part(
                "file",
                reqwest::blocking::multipart::Part::bytes(wav_bytes)
                    .file_name("chunk.wav")
                    .mime_str("audio/wav")
                    .expect("\"audio/wav\" is a valid MIME type"),
            )
            .text("model", self.cfg.api_model.clone())
            // Hardcoded, unlike the file engine's model-dependent
            // `response_format_for` -- Buzz's live arm always requests
            // `"json"` (`:322`); see the module doc comment.
            .text("response_format", "json");

        if !prompt.is_empty() {
            form = form.text("prompt", prompt.to_string());
        }
        // `language` goes only to the transcriptions endpoint, matching
        // Buzz's `translations.create(**options)` call, which never
        // receives a `language` kwarg at all (`:336-343`).
        if task == TaskKind::Transcribe
            && let Some(language) = language
        {
            form = form.text("language", language.to_string());
        }

        let client = self.client.clone();
        let api_key = self.cfg.auth.api_key.clone();
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

            // `offset_ms = 0`, `word_timestamps = false`: a live chunk is one
            // self-contained request with no chunk offset to apply, and the
            // caller only wants joined text, never segment timing.
            parse_response(&value, 0, false)
        });

        result.map(join_segment_text).map_err(map_api_failure)
    }
}

/// Joins parsed segments' text with a single space, matching Buzz's
/// `" ".join(segment["text"] for segment in
/// transcript.model_extra["segments"])`
/// (`recording_transcriber.py:346-351`) for the segments-present branch.
/// When the response instead carried only a top-level `"text"` field,
/// [`parse_response`]'s single-pseudo-segment fallback yields exactly one
/// segment, so this join is a no-op and returns that text unchanged —
/// matching Buzz's `else: result = {"text": transcript.text}` branch
/// (`:352-353`) with the same output either way.
fn join_segment_text(segments: Vec<Segment>) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::test_server::{Canned, TestServer};

    const TEST_KEY: &str = "sk-secret-live-test-key";

    fn transcriber_for(server: &TestServer, api_model: &str) -> LiveApiTranscriber {
        LiveApiTranscriber::new(OpenAiEngineConfig {
            auth: super::super::client::OpenAiAuth {
                base_url: Some(server.base_url()),
                api_key: TEST_KEY.to_string(),
            },
            api_model: api_model.to_string(),
        })
    }

    /// Byte-subsequence search -- a multipart body is arbitrary binary, so
    /// this can't be a `str` substring check.
    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty()
            && haystack
                .windows(needle.len())
                .any(|window| window == needle)
    }

    // -- join_segment_text (pure) -------------------------------------------

    #[test]
    fn join_segment_text_joins_multiple_segments_with_a_space() {
        let segs = vec![seg("hello"), seg("there")];
        assert_eq!(join_segment_text(segs), "hello there");
    }

    #[test]
    fn join_segment_text_single_segment_is_unchanged() {
        assert_eq!(join_segment_text(vec![seg("hello world")]), "hello world");
    }

    #[test]
    fn join_segment_text_empty_is_empty_string() {
        assert_eq!(join_segment_text(vec![]), "");
    }

    fn seg(text: &str) -> Segment {
        Segment {
            start: crate::domain::Millis(0),
            end: crate::domain::Millis(0),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    // -- mock-server tests (TCP) ---------------------------------------------
    //
    // Bind TCP (`TestServer::start`) -- blocked under the sandbox, run with
    // it lifted (see this task's report for the running count).

    #[test]
    fn request_shape_carries_model_prompt_and_wav_file_part() {
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::ok(br#"{"text":"hello world"}"#.to_vec()),
        );

        let transcriber = transcriber_for(&server, "whisper-1");
        let cancel = CancelToken::new();

        let result = transcriber.transcribe_chunk(
            &[0.0f32; 1600],
            "rolling prompt",
            None,
            TaskKind::Transcribe,
            &cancel,
        );
        assert_eq!(result.expect("ok"), "hello world");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/audio/transcriptions");

        let body = &requests[0].body;
        assert!(contains(body, b"name=\"file\""));
        assert!(contains(body, b"filename=\"chunk.wav\""));
        assert!(contains(body, b"RIFF"), "the wav part must start with RIFF");
        assert!(contains(body, b"name=\"model\""));
        assert!(contains(body, b"whisper-1"));
        assert!(contains(body, b"name=\"response_format\""));
        assert!(contains(body, b"json"));
        assert!(contains(body, b"name=\"prompt\""));
        assert!(contains(body, b"rolling prompt"));
        assert!(!contains(body, b"name=\"language\""));

        assert_eq!(
            requests[0].headers.get("authorization").map(String::as_str),
            Some("Bearer sk-secret-live-test-key"),
        );
    }

    #[test]
    fn prompt_omitted_when_empty() {
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::ok(br#"{"text":"hi"}"#.to_vec()),
        );

        let transcriber = transcriber_for(&server, "whisper-1");
        let cancel = CancelToken::new();

        transcriber
            .transcribe_chunk(&[0.0f32; 1600], "", None, TaskKind::Transcribe, &cancel)
            .expect("ok");

        assert!(!contains(&server.requests()[0].body, b"name=\"prompt\""));
    }

    #[test]
    fn language_part_present_when_some() {
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::ok(br#"{"text":"bonjour"}"#.to_vec()),
        );

        let transcriber = transcriber_for(&server, "whisper-1");
        let cancel = CancelToken::new();

        transcriber
            .transcribe_chunk(
                &[0.0f32; 1600],
                "",
                Some("fr"),
                TaskKind::Transcribe,
                &cancel,
            )
            .expect("ok");

        let body = &server.requests()[0].body;
        assert!(contains(body, b"name=\"language\""));
        assert!(contains(body, b"fr"));
    }

    #[test]
    fn translate_task_hits_translations_endpoint_and_omits_language() {
        // Mirrors `openai::engine`'s own
        // `translate_task_hits_translations_endpoint_and_omits_language`
        // (ledger #L15 closure): a language is configured, but the
        // translations endpoint must still never receive a `language` part
        // (Buzz `:336-343`).
        let server = TestServer::start();
        server.set_route(
            "/audio/translations",
            Canned::ok(br#"{"text":"bonjour le monde"}"#.to_vec()),
        );

        let transcriber = transcriber_for(&server, "whisper-1");
        let cancel = CancelToken::new();

        let result = transcriber
            .transcribe_chunk(
                &[0.0f32; 1600],
                "rolling prompt",
                Some("fr"),
                TaskKind::Translate,
                &cancel,
            )
            .expect("ok");

        assert_eq!(result, "bonjour le monde");
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/audio/translations");
        assert!(!contains(&requests[0].body, b"name=\"language\""));
        // `prompt` still goes through -- only `language` is endpoint-gated.
        assert!(contains(&requests[0].body, b"name=\"prompt\""));
    }

    #[test]
    fn segments_shaped_response_joins_text_with_spaces() {
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::ok(
                br#"{"segments":[{"start":0.0,"end":1.0,"text":"hello"},{"start":1.0,"end":2.0,"text":"world"}]}"#
                    .to_vec(),
            ),
        );

        let transcriber = transcriber_for(&server, "whisper-1");
        let cancel = CancelToken::new();

        let result = transcriber
            .transcribe_chunk(&[0.0f32; 1600], "", None, TaskKind::Transcribe, &cancel)
            .expect("ok");

        assert_eq!(result, "hello world");
    }

    #[test]
    fn non_2xx_response_maps_to_api_error_naming_status_and_host_without_key() {
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::status_with_body(
                401,
                br#"{"error":{"message":"Incorrect API key provided"}}"#.to_vec(),
            ),
        );

        let transcriber = transcriber_for(&server, "whisper-1");
        let cancel = CancelToken::new();

        let result =
            transcriber.transcribe_chunk(&[0.0f32; 1600], "", None, TaskKind::Transcribe, &cancel);

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
        let server = TestServer::start();
        server.set_route(
            "/audio/transcriptions",
            Canned::ok(br#"{"text":"hi"}"#.to_vec()),
        );

        let transcriber = transcriber_for(&server, "whisper-1");
        let cancel = CancelToken::new();
        cancel.cancel();

        let result =
            transcriber.transcribe_chunk(&[0.0f32; 1600], "", None, TaskKind::Transcribe, &cancel);

        assert!(
            matches!(result, Err(EngineError::Canceled)),
            "got {result:?}"
        );
        assert!(
            server.requests().is_empty(),
            "a pre-canceled run must make no HTTP request"
        );
    }

    /// A raw listener that accepts connections but never writes a response --
    /// used only to make a request hang long enough for [`cancelable`]'s
    /// poll-loop cancellation check to actually get consulted. Ported
    /// verbatim from `postproc::summary`'s mock-server tests
    /// (`postproc/summary.rs`'s `HangingServer`) -- see that struct's doc
    /// comment for why a *refused* connection doesn't work for this.
    struct HangingServer {
        addr: std::net::SocketAddr,
        _handle: std::thread::JoinHandle<()>,
    }

    impl HangingServer {
        fn start() -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind hanging server");
            let addr = listener.local_addr().expect("local_addr");
            let _handle = std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    std::mem::forget(stream);
                }
            });
            HangingServer { addr, _handle }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    #[test]
    fn cancel_mid_request_against_a_hanging_server_returns_canceled_promptly() {
        // Distinct from `pre_canceled_token_returns_canceled_with_zero_requests`:
        // that test's token is *already* canceled before the call, so
        // `transcribe_chunk`'s own eager pre-check (mirroring
        // `OpenAiEngine::transcribe`'s) short-circuits before ever building
        // a request -- it cannot exercise `cancelable`'s mid-flight poll
        // loop. Here `cancel` is untouched when `transcribe_chunk` starts
        // (the pre-check passes, the request is genuinely dispatched to a
        // server that never answers), and only trips ~50 ms later from a
        // background thread -- the poll loop must notice it and return
        // promptly instead of hanging for the full 60 s client timeout.
        let server = HangingServer::start();
        let transcriber = LiveApiTranscriber::new(OpenAiEngineConfig {
            auth: super::super::client::OpenAiAuth {
                base_url: Some(server.base_url()),
                api_key: TEST_KEY.to_string(),
            },
            api_model: "whisper-1".to_string(),
        });
        let cancel = CancelToken::new();
        let cancel_for_canceler = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancel_for_canceler.cancel();
        });

        let start = std::time::Instant::now();
        let result = transcriber.transcribe_chunk(
            &[0.0f32; 1600],
            "prompt",
            None,
            TaskKind::Transcribe,
            &cancel,
        );
        let elapsed = start.elapsed();

        assert!(
            matches!(result, Err(EngineError::Canceled)),
            "got {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "cancel should ack promptly even mid-request against a hanging server, took {elapsed:?}"
        );
    }
}
