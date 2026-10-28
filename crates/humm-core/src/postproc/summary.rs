//! F-111 `ai_summary` (design spec §6.2): after a transcription completes,
//! sends the transcript to an OpenAI-compatible chat completions API and
//! stores the resulting summary in the task's Notes field and/or a
//! `{stem}.summary.txt` file. Ported from `ai_summary/plugin.py`'s
//! `AISummaryPlugin.on_complete` (`:96-168`, whole file read for context).
//!
//! [`run`] was built and unit-tested standalone in Task 5 — Task 7 wires it
//! into `crate::queue::Runner::run_on_complete_chain`'s `on_complete` hook
//! chain (gated on [`crate::settings::AiSummarySettings::enabled`], the same
//! way [`crate::postproc::skip::check`] is gated on `cfg.skip.enabled` by its
//! *caller* rather than internally), second in the fixed **auto_resize →
//! summary → docx** order (design spec §2.1).
//!
//! Buzz's `_write_file` prefers `task.file_path or task.original_file_path`
//! (`plugin.py:157`) — the second alternative exists in Buzz only for the
//! (unported, F-027/M8) speech-extraction plugin's `_speech`-suffixed
//! intermediate file. humm has no such intermediate path yet, so `run` takes
//! a single `task_input` (the task's real input path) and derives the
//! `{stem}` from it directly — there is nothing `_speech`-suffixed to strip
//! (contrast `postproc::skip`'s `stem_for`, which *does* strip that suffix
//! because Demucs output can already exist on disk today even though this
//! milestone doesn't produce it).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::boundary::TaskStore;
use crate::cancel::CancelToken;
use crate::domain::{Segment, TranscriptionId};
use crate::openai::client::build_client;
use crate::openai::{ApiFailure, OpenAiAuth, cancelable};
use crate::settings::AiSummarySettings;

use super::PostprocConfig;
use crate::openai::chat;

/// Per-request timeout for the ai_summary chat-completions call: 120 s
/// (Buzz `timeout=120.0`, `ai_summary/plugin.py:144`). No retries — reqwest
/// has none by default, matching Buzz's explicit `max_retries=0` (`:137`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Outcome of one [`run`] call (design spec §6.2).
///
/// A *skip* (no key configured, an empty transcript, or a response whose
/// message content is itself empty/absent — Buzz's silent `if not summary:
/// return`) is not an error: both fields stay at their default and
/// [`Self::error`] stays `None` — only an actual failure (a canceled
/// request, a request/parse failure — including a structurally missing
/// `choices`/`message` — or a `save_to_notes`/`save_to_file` write failure)
/// populates it. When *both* the notes and file arms fail, their messages
/// are joined with `"; "` (Buzz handles each in its own `try`/`except` and
/// merely logs both independently; this struct has one `error` slot, so
/// both messages are preserved by concatenation rather than one silently
/// overwriting the other).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SummaryResult {
    /// Whether the task's Notes field was overwritten with the summary.
    pub notes_updated: bool,
    /// The summary file's path, if [`AiSummarySettings::save_to_file`] was
    /// set and the write succeeded.
    pub file_written: Option<PathBuf>,
    /// A non-fatal error recorded along the way, if any (see the struct
    /// doc comment for exactly when this is set vs. left `None`).
    pub error: Option<String>,
}

/// Runs the ai_summary hook for one completed task (F-111, design spec
/// §6.2), porting `AISummaryPlugin.on_complete` (`ai_summary/plugin.py:96-168`):
///
/// 1. **Gate on the API key** (`:98-101`): `cfg.summary_api_key` — resolved
///    from [`crate::secrets::SecretKey::AiSummaryApiKey`] by the
///    [`super::PostprocConfigSource`] impl that built `cfg`, deliberately its
///    *own* key rather than the transcription/translation key, mirroring
///    Buzz's own per-plugin secret — `None` or empty-after-trim is a silent
///    skip (logged at `warn`, Buzz's exact string), not an error.
/// 2. **Assemble the transcript** (`:103-108`): every segment with non-empty
///    `text`, joined with `"\n"`, then trimmed. Empty is a skip (logged at
///    `info`), not an error.
/// 3. **Request the summary** (`:125-153`): `model`/`prompt` fall back to
///    [`AiSummarySettings::DEFAULT_MODEL`]/[`AiSummarySettings::DEFAULT_PROMPT`]
///    via Buzz's `(value or default).strip()` idiom (`:132-134`) when the
///    configured value is exactly empty; the request goes out over a fresh,
///    120 s-timeout [`reqwest::blocking::Client`] via [`chat::complete`],
///    wrapped in [`cancelable`] (a cancel-ack ✚ over Buzz's uninterruptible
///    call — a canceled `cancel` yields `error: Some("canceled".into())`
///    with nothing written, never even reaching the write arms below). Any
///    other request failure (including a *structurally absent*
///    `choices`/`message`, `:150-153`) is recorded in
///    [`SummaryResult::error`] and logged, with nothing written — but a
///    response that *has* a message with an empty/absent `content` is a
///    **silent** no-op instead (Buzz's `if not summary: return`, `:111-112`
///    — no log beyond `debug`, no error, nothing written), since Buzz never
///    logs for that case.
/// 4. **`save_to_notes`** (`:114-121`): [`TaskStore::set_task_notes`]
///    overwrites the task's notes (Buzz's `update_transcription_notes` is
///    an overwrite too). A store error is logged and recorded, but does not
///    stop the file arm from still being attempted (Buzz's independent
///    per-arm `try`/`except`).
/// 5. **`save_to_file`** (`:122-123`, `:155-168`): `{stem}.summary.txt` —
///    `stem` from `task_input`'s file stem (see the module doc comment for
///    why there is no `_speech` stripping here) — written into
///    `cfg.summary.output_folder` if set, else `task_input`'s parent
///    directory, `create_dir_all`-ing the destination first. An I/O error is
///    logged and recorded, non-fatally.
pub fn run(
    cfg: &PostprocConfig,
    outcome_segments: &[Segment],
    task_input: &Path,
    store: &dyn TaskStore,
    task_id: &TranscriptionId,
    cancel: &CancelToken,
) -> SummaryResult {
    let Some(api_key) = cfg
        .summary_api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    else {
        log::warn!("AI Summary skipped: no API key configured");
        return SummaryResult::default();
    };

    let transcript = outcome_segments
        .iter()
        .filter(|segment| !segment.text.is_empty())
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let transcript = transcript.trim();
    if transcript.is_empty() {
        log::info!("AI Summary skipped: empty transcript");
        return SummaryResult::default();
    }

    let auth = OpenAiAuth {
        base_url: cfg.summary.api_url.clone(),
        api_key: api_key.to_string(),
    };
    let model = or_default_then_trim(&cfg.summary.model, AiSummarySettings::DEFAULT_MODEL);
    let prompt = or_default_then_trim(&cfg.summary.prompt, AiSummarySettings::DEFAULT_PROMPT);
    let transcript = transcript.to_string();

    let client = build_client(REQUEST_TIMEOUT);
    let result = cancelable(cancel, move || {
        chat::complete(&client, &auth, &model, &prompt, &transcript)
    });

    let summary = match result {
        Ok(content) => content,
        Err(ApiFailure::Canceled) => {
            return SummaryResult {
                error: Some(ApiFailure::Canceled.to_string()),
                ..SummaryResult::default()
            };
        }
        Err(e) => {
            let message = format!("AI Summary request failed: {e}");
            log::error!("{message}");
            return SummaryResult {
                error: Some(message),
                ..SummaryResult::default()
            };
        }
    };

    if summary.is_empty() {
        // Buzz's `_summarize` returns `completion.choices[0].message.content`
        // directly whenever `message` itself is present (even if `content`
        // is `None`/`""`) -- it only logs "empty response from server" when
        // `completion`/`choices`/`message` are *structurally absent*
        // (`plugin.py:150-153`), which `chat::complete` already maps to
        // `Err(ApiFailure::Shape)` above (the "request failed" arm). Back in
        // `on_complete`, `if not summary: return` (`:111-112`) then exits
        // with **no** log and **no** error for the present-but-empty case --
        // a skip, not a failure. `summary` is checked unstripped (`is_empty`,
        // no `.trim()`) because Buzz's `if not summary` is Python falsiness
        // on the raw content, with no stripping beforehand.
        log::debug!("AI Summary: server returned an empty summary (Buzz's silent no-op case)");
        return SummaryResult::default();
    }

    let mut errors: Vec<String> = Vec::new();
    let mut notes_updated = false;
    let mut file_written = None;

    if cfg.summary.save_to_notes {
        match store.set_task_notes(task_id, &summary) {
            Ok(()) => notes_updated = true,
            Err(e) => {
                let message = format!("Failed to save summary to notes: {e}");
                log::error!("{message}");
                errors.push(message);
            }
        }
    }

    if cfg.summary.save_to_file {
        match write_summary_file(cfg, task_input, &summary) {
            Ok(path) => file_written = Some(path),
            Err(e) => {
                let message = format!("Failed to write summary file: {e}");
                log::error!("{message}");
                errors.push(message);
            }
        }
    }

    SummaryResult {
        notes_updated,
        file_written,
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        },
    }
}

/// Buzz's `(value or default).strip()` idiom (`ai_summary/plugin.py:132-134`):
/// an exactly-empty `value` falls back to `default`; anything else (even a
/// whitespace-only string, which Python treats as truthy) is used as-is —
/// *then* both paths are `.trim()`-med. This deliberately reproduces the one
/// Buzz quirk that implies: a whitespace-only configured value is not
/// replaced by `default`, it simply trims down to an empty string.
fn or_default_then_trim(value: &str, default: &str) -> String {
    let base = if value.is_empty() { default } else { value };
    base.trim().to_string()
}

/// Writes `summary` to `{stem}.summary.txt` (Buzz `_write_file`,
/// `ai_summary/plugin.py:155-168`): `stem` from `task_input`'s file stem,
/// destination `cfg.summary.output_folder` if set and non-empty *after
/// trimming* (Buzz's `(cfg.get("output_folder") or "").strip()`,
/// `:159` — a whitespace-only configured folder falls back exactly like an
/// empty one, mirroring [`or_default_then_trim`]'s trim-before-decide
/// idiom), else `task_input`'s parent directory (Buzz's
/// `os.path.dirname(...) or os.getcwd()` — `Path::parent()` on a bare
/// relative filename already yields the empty path, which resolves against
/// the process's current directory exactly like Buzz's explicit fallback).
/// The destination directory is created (`create_dir_all`) before the
/// write. Returns the written file's path.
fn write_summary_file(
    cfg: &PostprocConfig,
    task_input: &Path,
    summary: &str,
) -> std::io::Result<PathBuf> {
    let stem = task_input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "transcript".to_string());

    let trimmed_output_folder = cfg
        .summary
        .output_folder
        .as_ref()
        .map(|folder| folder.to_string_lossy().trim().to_string())
        .filter(|folder| !folder.is_empty());

    let folder = match trimmed_output_folder {
        Some(folder) => PathBuf::from(folder),
        None => task_input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    };

    std::fs::create_dir_all(&folder)?;
    let out_path = folder.join(format!("{stem}.summary.txt"));
    std::fs::write(&out_path, summary)?;
    log::info!("AI Summary written to {}", out_path.display());
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::StoreError;
    use crate::settings::{
        AutoResizeSettings, ExportDocxSettings, LangDetectSettings, NoiseReductionSettings,
        SkipSettings,
    };
    use tempfile::TempDir;

    // -- a minimal `TaskStore` double: only `set_task_notes` is ever
    //    exercised by `run` -- every other method is unreachable here. -----

    #[derive(Default)]
    struct FakeStore {
        notes: std::sync::Mutex<Vec<(TranscriptionId, String)>>,
        fail_notes: bool,
    }

    impl TaskStore for FakeStore {
        fn create_task(
            &self,
            _id: &TranscriptionId,
            _spec: &crate::domain::TaskSpec,
            _source: crate::domain::Source,
        ) -> Result<(), StoreError> {
            unreachable!("run() never creates tasks")
        }

        fn requeue_task(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never requeues tasks")
        }

        fn mark_started(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks started")
        }

        fn update_progress(&self, _id: &TranscriptionId, _fraction: f64) -> Result<(), StoreError> {
            unreachable!("run() never updates progress")
        }

        fn set_task_name(&self, _id: &TranscriptionId, _name: &str) -> Result<(), StoreError> {
            unreachable!("run() never sets the task name")
        }

        fn update_file(&self, _id: &TranscriptionId, _path: &Path) -> Result<(), StoreError> {
            unreachable!("run() never updates the file path")
        }

        fn update_language(
            &self,
            _id: &TranscriptionId,
            _language: &str,
        ) -> Result<(), StoreError> {
            unreachable!("run() never updates the language")
        }

        fn mark_completed(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks completed")
        }

        fn mark_failed(&self, _id: &TranscriptionId, _error: &str) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks failed")
        }

        fn mark_canceled(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks canceled")
        }

        fn mark_skipped(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks skipped")
        }

        fn find_completed_by_file_name(
            &self,
            _name: &str,
        ) -> Result<Option<TranscriptionId>, StoreError> {
            unreachable!("run() never looks up completed tasks by name")
        }

        fn find_completed_by_url(&self, _url: &str) -> Result<Option<TranscriptionId>, StoreError> {
            unreachable!("run() never looks up completed tasks by url")
        }

        fn segments_for(&self, _id: &TranscriptionId) -> Result<Vec<Segment>, StoreError> {
            unreachable!("run() is handed its segments directly")
        }

        fn set_task_notes(&self, id: &TranscriptionId, notes: &str) -> Result<(), StoreError> {
            if self.fail_notes {
                return Err(StoreError::Query {
                    detail: "boom".to_string(),
                });
            }
            self.notes
                .lock()
                .expect("notes lock")
                .push((*id, notes.to_string()));
            Ok(())
        }

        fn replace_task_segments(
            &self,
            _id: &TranscriptionId,
            _segs: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("run() never replaces segments (that's postproc::auto_resize's job)")
        }
    }

    fn seg(text: &str) -> Segment {
        Segment {
            start: crate::domain::Millis(0),
            end: crate::domain::Millis(0),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    fn base_cfg() -> PostprocConfig {
        PostprocConfig {
            skip: SkipSettings::default(),
            summary: AiSummarySettings::default(),
            summary_api_key: Some("sk-test".to_string()),
            docx: ExportDocxSettings::default(),
            resize: AutoResizeSettings::default(),
            paragraph_split: crate::domain::Millis(2000),
            noise_reduction: NoiseReductionSettings::default(),
            language_detection: LangDetectSettings::default(),
        }
    }

    fn never_touched_store() -> FakeStore {
        FakeStore::default()
    }

    // -- gates: no key / empty transcript -----------------------------------

    #[test]
    fn no_key_none_skips_before_any_request() {
        let mut cfg = base_cfg();
        cfg.summary_api_key = None;
        let store = never_touched_store();

        let result = run(
            &cfg,
            &[seg("hello")],
            Path::new("/tmp/clip.mp3"),
            &store,
            &TranscriptionId::new(),
            &CancelToken::new(),
        );

        assert_eq!(result, SummaryResult::default());
    }

    #[test]
    fn no_key_empty_after_trim_skips() {
        let mut cfg = base_cfg();
        cfg.summary_api_key = Some("   ".to_string());
        let store = never_touched_store();

        let result = run(
            &cfg,
            &[seg("hello")],
            Path::new("/tmp/clip.mp3"),
            &store,
            &TranscriptionId::new(),
            &CancelToken::new(),
        );

        assert_eq!(result, SummaryResult::default());
    }

    #[test]
    fn empty_transcript_skips_all_blank_segments() {
        let cfg = base_cfg();
        let store = never_touched_store();

        let result = run(
            &cfg,
            &[seg(""), seg("   ")],
            Path::new("/tmp/clip.mp3"),
            &store,
            &TranscriptionId::new(),
            &CancelToken::new(),
        );

        assert_eq!(result, SummaryResult::default());
    }

    #[test]
    fn empty_transcript_skips_when_no_segments_at_all() {
        let cfg = base_cfg();
        let store = never_touched_store();

        let result = run(
            &cfg,
            &[],
            Path::new("/tmp/clip.mp3"),
            &store,
            &TranscriptionId::new(),
            &CancelToken::new(),
        );

        assert_eq!(result, SummaryResult::default());
    }

    // -- or_default_then_trim -------------------------------------------

    #[test]
    fn or_default_then_trim_empty_falls_back() {
        assert_eq!(or_default_then_trim("", "fallback"), "fallback");
    }

    #[test]
    fn or_default_then_trim_non_empty_is_trimmed_not_replaced() {
        assert_eq!(or_default_then_trim("  custom  ", "fallback"), "custom");
    }

    #[test]
    fn or_default_then_trim_whitespace_only_trims_to_empty_buzz_quirk() {
        // Buzz's `(value or default).strip()`: a whitespace-only string is
        // truthy in Python, so it is *not* replaced by `default` -- it just
        // trims down to "".
        assert_eq!(or_default_then_trim("   ", "fallback"), "");
    }

    // -- write_summary_file: folder placement --------------------------------

    #[test]
    fn write_summary_file_defaults_next_to_input() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let cfg = base_cfg();

        let path = write_summary_file(&cfg, &input, "the summary").expect("write");

        assert_eq!(path, tmp.path().join("clip.summary.txt"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "the summary");
    }

    #[test]
    fn write_summary_file_uses_configured_output_folder() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let out_dir = tmp.path().join("out");
        let mut cfg = base_cfg();
        cfg.summary.output_folder = Some(out_dir.clone());

        let path = write_summary_file(&cfg, &input, "the summary").expect("write");

        assert_eq!(path, out_dir.join("clip.summary.txt"));
        assert!(out_dir.is_dir(), "output_folder should be create_dir_all'd");
    }

    #[test]
    fn write_summary_file_whitespace_only_output_folder_falls_back_to_next_to_input() {
        // Buzz's `(cfg.get("output_folder") or "").strip()`: a
        // whitespace-only configured folder is *not* used verbatim -- it
        // trims down to "" and falls back exactly like an unset folder.
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let mut cfg = base_cfg();
        cfg.summary.output_folder = Some(PathBuf::from("   "));

        let path = write_summary_file(&cfg, &input, "the summary").expect("write");

        assert_eq!(path, tmp.path().join("clip.summary.txt"));
    }

    // -- mock-server tests (TCP) ---------------------------------------------
    //
    // Bind TCP (`TestServer::start`) -- blocked under the sandbox, run with
    // it lifted (see this task's report for the full pre-existing set / the
    // new running count).

    mod mock_server {
        use super::*;
        use crate::models::test_server::{Canned, TestServer};

        /// A raw listener that accepts connections but never writes a
        /// response — used only to make a request hang long enough for
        /// [`cancelable`]'s poll-loop cancellation check to actually get
        /// consulted. A *refused* connection (e.g. `127.0.0.1:1`, the style
        /// `openai::validate`'s transport-error test uses) fails too fast
        /// for that: the request thread's `Err` can arrive before the first
        /// ~100 ms poll tick, so a pre-canceled token's check is never
        /// reached and the request's own (non-canceled) failure wins the
        /// race instead — this is what actually happened the first time
        /// this test was written against a refused port. Binds TCP, so this
        /// joins the sandbox-blocked set like every other test in this
        /// module.
        struct HangingServer {
            addr: std::net::SocketAddr,
            _handle: std::thread::JoinHandle<()>,
        }

        impl HangingServer {
            fn start() -> Self {
                let listener =
                    std::net::TcpListener::bind("127.0.0.1:0").expect("bind hanging server");
                let addr = listener.local_addr().expect("local_addr");
                let _handle = std::thread::spawn(move || {
                    // Accept and hold every connection open, writing
                    // nothing back -- deliberately leaked; this is a
                    // short-lived test process.
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
        fn cancel_preset_returns_canceled_and_writes_nothing() {
            let server = HangingServer::start();
            let mut cfg = base_cfg();
            cfg.summary.api_url = Some(server.base_url());
            let store = never_touched_store();
            let cancel = CancelToken::new();
            cancel.cancel();

            let start = std::time::Instant::now();
            let result = run(
                &cfg,
                &[seg("hello")],
                Path::new("/tmp/clip.mp3"),
                &store,
                &TranscriptionId::new(),
                &cancel,
            );
            let elapsed = start.elapsed();

            assert_eq!(result.error.as_deref(), Some("canceled"));
            assert!(!result.notes_updated);
            assert_eq!(result.file_written, None);
            assert!(
                elapsed < Duration::from_secs(2),
                "cancel should ack promptly, took {elapsed:?}"
            );
        }

        fn choice_response(content: &str) -> Vec<u8> {
            serde_json::json!({
                "choices": [ { "message": { "content": content } } ]
            })
            .to_string()
            .into_bytes()
        }

        fn cfg_for(server: &TestServer) -> PostprocConfig {
            let mut cfg = base_cfg();
            cfg.summary.api_url = Some(server.base_url());
            cfg
        }

        #[test]
        fn request_shape_system_prompt_user_transcript() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("key points here")),
            );

            let mut cfg = cfg_for(&server);
            cfg.summary.model = "gpt-4o-mini".to_string();
            cfg.summary.prompt = "Summarize this.".to_string();
            let store = never_touched_store();

            let result = run(
                &cfg,
                &[seg(""), seg("hello"), seg("world")],
                Path::new("/tmp/clip.mp3"),
                &store,
                &TranscriptionId::new(),
                &CancelToken::new(),
            );

            assert_eq!(result.error, None);

            let requests = server.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].method, "POST");
            assert_eq!(requests[0].path, "/chat/completions");
            assert_eq!(
                requests[0].headers.get("authorization").map(String::as_str),
                Some("Bearer sk-test")
            );

            let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
            assert_eq!(body["model"], "gpt-4o-mini");
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(body["messages"][0]["content"], "Summarize this.");
            assert_eq!(body["messages"][1]["role"], "user");
            // Empty-text segment dropped, remaining two joined with "\n".
            assert_eq!(body["messages"][1]["content"], "hello\nworld");
        }

        #[test]
        fn model_and_prompt_default_when_configured_empty() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("summary")),
            );

            let mut cfg = cfg_for(&server);
            cfg.summary.model = String::new();
            cfg.summary.prompt = String::new();
            let store = never_touched_store();

            let result = run(
                &cfg,
                &[seg("hello")],
                Path::new("/tmp/clip.mp3"),
                &store,
                &TranscriptionId::new(),
                &CancelToken::new(),
            );

            assert_eq!(result.error, None);
            let requests = server.requests();
            let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
            assert_eq!(body["model"], AiSummarySettings::DEFAULT_MODEL);
            assert_eq!(
                body["messages"][0]["content"],
                AiSummarySettings::DEFAULT_PROMPT
            );
        }

        #[test]
        fn non_2xx_response_is_error_recorded_nothing_written() {
            let server = TestServer::start();
            server.set_route_for_method("POST", "/chat/completions", Canned::status(500));

            let cfg = cfg_for(&server);
            let store = never_touched_store();

            let result = run(
                &cfg,
                &[seg("hello")],
                Path::new("/tmp/clip.mp3"),
                &store,
                &TranscriptionId::new(),
                &CancelToken::new(),
            );

            assert!(result.error.is_some());
            assert!(!result.notes_updated);
            assert_eq!(result.file_written, None);
        }

        #[test]
        fn malformed_json_body_is_error_recorded_nothing_written() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(b"not json at all".to_vec()),
            );

            let cfg = cfg_for(&server);
            let store = never_touched_store();

            let result = run(
                &cfg,
                &[seg("hello")],
                Path::new("/tmp/clip.mp3"),
                &store,
                &TranscriptionId::new(),
                &CancelToken::new(),
            );

            assert!(result.error.is_some());
            assert!(!result.notes_updated);
            assert_eq!(result.file_written, None);
        }

        #[test]
        fn empty_response_content_is_a_silent_no_op_per_buzz() {
            // Buzz distinguishes "message present but content empty" (silent
            // skip, no log, no error -- `plugin.py:111-112,150`) from
            // "choices/message structurally absent" (logged error,
            // `:152-153`, covered by `malformed_json_body_is_error_recorded_
            // nothing_written` and `non_2xx_response_is_error_recorded_
            // nothing_written` above). This response has a real `message`
            // object with an empty `content` string -- the former case.
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("")),
            );

            let cfg = cfg_for(&server);
            let store = never_touched_store();

            let result = run(
                &cfg,
                &[seg("hello")],
                Path::new("/tmp/clip.mp3"),
                &store,
                &TranscriptionId::new(),
                &CancelToken::new(),
            );

            assert_eq!(result, SummaryResult::default());
        }

        #[test]
        fn successful_round_trip_writes_notes_and_default_file() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("the real summary")),
            );

            let tmp = TempDir::new().expect("tempdir");
            let input = tmp.path().join("clip.mp3");
            std::fs::write(&input, b"fake media").expect("write input");

            let mut cfg = cfg_for(&server);
            cfg.summary.save_to_notes = true;
            cfg.summary.save_to_file = true;
            let store = FakeStore::default();
            let task_id = TranscriptionId::new();

            let result = run(
                &cfg,
                &[seg("hello")],
                &input,
                &store,
                &task_id,
                &CancelToken::new(),
            );

            assert_eq!(result.error, None);
            assert!(result.notes_updated);
            let notes = store.notes.lock().expect("notes lock");
            assert_eq!(
                notes.as_slice(),
                &[(task_id, "the real summary".to_string())]
            );

            let file_written = result.file_written.expect("file should be written");
            assert_eq!(file_written, tmp.path().join("clip.summary.txt"));
            assert_eq!(
                std::fs::read_to_string(&file_written).unwrap(),
                "the real summary"
            );
        }

        #[test]
        fn save_to_file_uses_configured_output_folder() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("the real summary")),
            );

            let tmp = TempDir::new().expect("tempdir");
            let input = tmp.path().join("clip.mp3");
            std::fs::write(&input, b"fake media").expect("write input");
            let out_dir = tmp.path().join("out");

            let mut cfg = cfg_for(&server);
            cfg.summary.save_to_notes = false;
            cfg.summary.save_to_file = true;
            cfg.summary.output_folder = Some(out_dir.clone());
            let store = never_touched_store();

            let result = run(
                &cfg,
                &[seg("hello")],
                &input,
                &store,
                &TranscriptionId::new(),
                &CancelToken::new(),
            );

            assert_eq!(result.error, None);
            assert!(!result.notes_updated);
            assert_eq!(result.file_written, Some(out_dir.join("clip.summary.txt")));
        }

        #[test]
        fn both_arms_failing_joins_error_messages() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("the real summary")),
            );

            let tmp = TempDir::new().expect("tempdir");
            let input = tmp.path().join("clip.mp3");
            std::fs::write(&input, b"fake media").expect("write input");
            // A regular file where a directory is expected: `create_dir_all`
            // on a path through it fails, forcing the file arm to error.
            let blocker = tmp.path().join("blocker.txt");
            std::fs::write(&blocker, b"not a directory").expect("write blocker");

            let mut cfg = cfg_for(&server);
            cfg.summary.save_to_notes = true;
            cfg.summary.save_to_file = true;
            cfg.summary.output_folder = Some(blocker.join("nested"));
            let store = FakeStore {
                fail_notes: true,
                ..FakeStore::default()
            };

            let result = run(
                &cfg,
                &[seg("hello")],
                &input,
                &store,
                &TranscriptionId::new(),
                &CancelToken::new(),
            );

            assert!(!result.notes_updated);
            assert_eq!(result.file_written, None);
            let error = result.error.expect("both arms failed, error expected");
            assert!(
                error.contains("Failed to save summary to notes"),
                "error was: {error}"
            );
            assert!(
                error.contains("Failed to write summary file"),
                "error was: {error}"
            );
            assert!(error.contains("; "), "messages should be joined: {error}");
        }
    }

    // -- real-API contract (gated) -------------------------------------------
    //
    // The ai_summary twin of `translate::tests::real_chat_api_single_and_batch_round_trip`
    // (F-031) -- see design spec §0/§6.3: this environment has no local
    // OpenAI-compatible chat target, so this stays env-gated and
    // committed-runnable rather than executed in CI/acceptance.

    #[test]
    #[ignore = "requires a real OpenAI-compatible chat server (set HUMM_LLM_TEST_BASE)"]
    fn real_summary_api_round_trip() {
        let base = match std::env::var("HUMM_LLM_TEST_BASE") {
            Ok(v) if !v.is_empty() => v,
            _ => panic!(
                "HUMM_LLM_TEST_BASE is not set — point it at a real \
                 OpenAI-compatible chat server (any of ollama / LM Studio / \
                 llama.cpp server / cloud OpenAI)"
            ),
        };
        let key = std::env::var("HUMM_LLM_TEST_KEY").unwrap_or_else(|_| "sk-local-unused".into());
        let model = std::env::var("HUMM_LLM_TEST_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());

        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");

        let mut cfg = base_cfg();
        cfg.summary.api_url = Some(base);
        cfg.summary.model = model;
        cfg.summary_api_key = Some(key);
        cfg.summary.save_to_notes = true;
        cfg.summary.save_to_file = false;
        let store = FakeStore::default();
        let task_id = TranscriptionId::new();

        let result = run(
            &cfg,
            &[seg("Good morning, everyone. The weather is nice today.")],
            &input,
            &store,
            &task_id,
            &CancelToken::new(),
        );

        assert_eq!(result.error, None, "expected a clean round trip");
        assert!(result.notes_updated);
        let notes = store.notes.lock().expect("notes lock");
        assert_eq!(notes.len(), 1);
        assert!(
            !notes[0].1.trim().is_empty(),
            "real chat API returned an empty summary"
        );
    }
}
