//! `humm add`: resolve the model, then drive one transcription task per input
//! through the persistent store + task queue (F-002, F-024, F-025, F-026,
//! F-029).
//!
//! Flow (parity order from `buzz/cli.py`, model resolved synchronously
//! *before* any task is queued — `cli.py:219-221`):
//! 1. Grammar/usage validation (Task 10) + language parse.
//! 2. Load [`Settings`] (`paths::settings_path()`).
//! 3. Open [`SqliteStore`]; first-run Buzz import (non-fatal), then
//!    `cancel_stale_tasks()` (a crashed/killed prior run leaves stale
//!    `queued`/`in_progress` rows — F-029).
//! 4. Resolve the model synchronously, streaming download progress.
//! 5. Ensure the VAD model (F-045): use the cached copy if present, else fetch
//!    it once. Buzz ships `ggml-silero-v6.2.0.bin` next to whisper-cli at build
//!    time (FEATURE_INVENTORY F-045), so every Buzz user has VAD by default;
//!    humm ships no file and downloads it on first run to match that outcome.
//!    A fetch failure degrades to no-VAD (`whisper_cpp.py:380`'s absent-model
//!    case) — the *failure* mode, never the default — and never fails the run.
//!    `HUMM_DISABLE_VAD` (test-only) skips this step outright.
//! 6. Spawn the task runner; enqueue one [`TaskSpec`] per input.
//! 7. Install a Ctrl-C handler: first signal cancels every task, second exits.
//! 8. Drain [`TaskEvent`]s, rendering progress/ETA (human) or one JSON object
//!    per line (`--json`); print written paths to stdout, errors to stderr.
//! 9. Exit code: any canceled → 130; else any failed → 2; else 0 (ADR-007).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

use humm_core::boundary::{
    AudioDecoder, MediaFetcher, MediaProber, TaskStore, TranscriptionEngine,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    DEFAULT_TEMPERATURE, Input, Millis, ModelSize, ModelSpec, OutputFormat, OutputSpec, Source,
    TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::error::CoreError;
use humm_core::language::Language;
use humm_core::models::download::{DownloadProgress, ModelDownloader};
use humm_core::models::registry::local_vad_model_path;
use humm_core::paths;
use humm_core::pipeline::EngineTuning;
use humm_core::queue::{
    EngineFactory, QueueHandle, RunnerDeps, TaskEvent, TaskEventKind, spawn_task_runner,
};
use humm_core::settings::Settings;
use humm_media::{FfmpegDecoder, FfprobeProber, YtDlpFetcher, probe_binaries};
use humm_store::SqliteStore;
use humm_whisper::WhisperEngine;

use crate::args::{AddArgs, ModelTypeArg};

/// Runs `humm add`. Returns the process exit code per ADR-007: `0` every input
/// succeeded, `1` a usage/config error aborted the run before any task could be
/// queued, `2` at least one task failed, `130` at least one task was canceled
/// (SIGINT / stale-recovery).
pub fn run_add(args: AddArgs) -> ExitCode {
    // Informational, not a validation gate (ADR-007: `--hide-gui` is
    // accepted-but-inert since humm is headless).
    if args.hide_gui {
        eprintln!("note: --hide-gui has no effect; humm is headless (ADR-007)");
    }

    if let Some(exit_code) = validate_add_args(&args) {
        return exit_code;
    }

    let language = match parse_language(&args.language) {
        Ok(language) => language,
        Err(message) => return usage_fail(message),
    };

    execute(&args, language)
}

/// Validates the semantics clap's grammar can't express, in the order
/// `task-10-brief.md` specifies — all before any store/model work. Returns
/// `Some(exit_code)` (a usage error, with the `--help` hint) to abort, `None`
/// once every check passes.
fn validate_add_args(args: &AddArgs) -> Option<ExitCode> {
    if args.files.is_empty() {
        return Some(usage_fail("No input files"));
    }

    if args.extract_speech {
        return Some(usage_fail(
            "speech extraction is not yet available in humm — it arrives in a later release",
        ));
    }

    if args.model_type == ModelTypeArg::OpenAiApi {
        return Some(usage_fail(
            "the OpenAI API backend is not yet available in humm — it arrives with cloud \
             support; use a local model type meanwhile",
        ));
    }

    if args.model_type == ModelTypeArg::HuggingFace && args.hfid.is_none() {
        // cli.py:146-147, verbatim.
        return Some(usage_fail(
            "--hfid is required when --model-type is huggingface",
        ));
    }

    // `-m whisper|whispercpp|fasterwhisper` all run the local whisper-rs
    // backend (ADR-008); `--hfid` with a non-huggingface type and
    // `--openai-token` without `-m openaiapi` are both accepted-and-ignored
    // exactly as Buzz does — neither needs code here.
    None
}

/// The real run: steps 2–9 above. Returns the process exit code. Config-level
/// failures (settings/store/model) print an actionable cause via [`run_fail`]
/// (no usage hint) and return `1`.
fn execute(args: &AddArgs, language: Option<Language>) -> ExitCode {
    let settings = match Settings::load(&paths::settings_path()) {
        Ok(settings) => settings,
        Err(err) => return run_fail(err),
    };

    let store = match SqliteStore::open(&paths::db_path()) {
        Ok(store) => Arc::new(store),
        Err(err) => return run_fail(err),
    };

    import_buzz_history(&store);
    match store.cancel_stale_tasks() {
        Ok(0) => {}
        Ok(n) => log::info!("recovered {n} stale task(s) from a prior run as canceled"),
        Err(err) => log::warn!("cancel_stale_tasks failed (continuing): {err}"),
    }

    let model_root = paths::model_root();
    // The model download is cancellable in principle; the Ctrl-C handler is
    // installed later (brief step 7), so a SIGINT during a long model download
    // is the default terminate, matching Buzz.
    let download_cancel = CancelToken::new();
    let resolved = match resolve_model(args, &settings, &model_root, &download_cancel) {
        Ok(resolved) => resolved,
        Err(exit_code) => return exit_code,
    };
    if resolved.coreml_degraded && !args.json {
        eprintln!(
            "Note: the CoreML encoder could not be downloaded (network issue); using the local \
             model without CoreML acceleration. It will be retried on the next run."
        );
    }

    let use_gpu = use_gpu_from_env(std::env::var("HUMM_FORCE_CPU").ok().as_deref());
    let vad_model = ensure_vad(&model_root, args.json, args.quiet, &download_cancel);
    let tuning = EngineTuning {
        vad_model,
        use_gpu,
        threads: threads_from_env(std::env::var("HUMM_WHISPER_N_THREADS").ok().as_deref()),
    };

    let formats = output_formats(args);
    if formats.is_empty() {
        log::warn!("no export format selected (--txt/--srt/--vtt); nothing will be written");
    }

    let options = TranscriptionOptions {
        language,
        task: args.task,
        model: ModelSpec {
            size: args.model_size,
            hf_repo: resolved.hf_repo.clone(),
        },
        word_timestamps: args.word_timestamps,
        initial_prompt: args.prompt.clone(),
        temperature: DEFAULT_TEMPERATURE.to_vec(),
    };
    let output = OutputSpec {
        formats,
        directory: args.output_directory.clone(),
        file_name_template: None,
        paragraph_split: paragraph_split_from_env(
            std::env::var("HUMM_PARAGRAPH_SPLIT_TIME").ok().as_deref(),
        ),
    };

    let decoder: Arc<dyn AudioDecoder> = Arc::new(FfmpegDecoder::new());
    let prober: Arc<dyn MediaProber> = Arc::new(FfprobeProber::new());
    let fetcher = build_fetcher(&settings);
    let store_dyn: Arc<dyn TaskStore> = Arc::clone(&store) as Arc<dyn TaskStore>;

    let deps = RunnerDeps {
        store: store_dyn,
        decoder,
        prober,
        fetcher,
        engines: Box::new(CliEngineFactory {
            model_path: resolved.path,
            use_gpu,
        }),
        tuning,
        media_dir: paths::media_dir(),
    };

    let handle = spawn_task_runner(deps);
    let events = handle.subscribe();

    let mut enqueued: Vec<TranscriptionId> = Vec::new();
    let mut labels: HashMap<TranscriptionId, String> = HashMap::new();
    for file in &args.files {
        let input = Input::parse(file);
        let source = match &input {
            Input::Url(_) => Source::UrlImport,
            Input::Path(_) => Source::FileImport,
        };
        let id = TranscriptionId::new();
        labels.insert(id, input.display_name());
        let spec = TaskSpec {
            input,
            options: options.clone(),
            output: output.clone(),
        };
        handle.enqueue(id, spec, source);
        enqueued.push(id);
    }

    // Hand the handle to the Ctrl-C surface (shared so `cancel_all` can run
    // while the drain owns nothing) and install the signal handler.
    let shared: Arc<Mutex<Option<QueueHandle>>> = Arc::new(Mutex::new(Some(handle)));
    install_ctrlc(Arc::clone(&shared));

    let exit = drain_and_render(&events, &enqueued, &labels, args.json, args.quiet);

    // Signal "no more work" and join the runner. Take the handle out from
    // under the lock *before* the (blocking) `finish`, so a late Ctrl-C never
    // waits on us.
    let handle = shared.lock().ok().and_then(|mut guard| guard.take());
    if let Some(handle) = handle {
        let _ = handle.finish();
    }

    exit
}

/// Runs the one-time Buzz import when a Buzz database is present and the import
/// hasn't already completed. Non-fatal by contract (Q-6): failures are logged,
/// never surfaced. Must run *before* `cancel_stale_tasks` so an imported stale
/// row is flipped like any other (Task 4 contract).
fn import_buzz_history(store: &SqliteStore) {
    match store.buzz_import_done() {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            log::warn!("could not check Buzz-import state (skipping import): {err}");
            return;
        }
    }
    let Some(buzz_db) = paths::buzz_sqlite_path() else {
        return;
    };
    match store.import_buzz_db(&buzz_db) {
        Ok(outcome) => log::info!(
            "imported Buzz history from {}: {} task(s), {} segment(s)",
            buzz_db.display(),
            outcome.tasks,
            outcome.segments
        ),
        Err(err) => log::warn!("Buzz import failed (continuing without it): {err}"),
    }
}

/// The resolved model: the local ggml path to load, whether CoreML degraded,
/// and the Hugging Face repo id (for the persisted [`ModelSpec`]).
struct ResolvedModelInfo {
    path: PathBuf,
    coreml_degraded: bool,
    hf_repo: Option<String>,
}

/// Resolves the model synchronously (parity: before any task queues), streaming
/// download progress as a human stderr line or a `model_download_progress`
/// JSON event. Failures return an `Err(ExitCode)` already printed by
/// [`run_fail`] (exit 1, no usage hint).
fn resolve_model(
    args: &AddArgs,
    settings: &Settings,
    model_root: &std::path::Path,
    cancel: &CancelToken,
) -> Result<ResolvedModelInfo, ExitCode> {
    let is_custom =
        args.model_type != ModelTypeArg::HuggingFace && args.model_size == ModelSize::Custom;
    if is_custom && settings.models.custom_model_url.is_none() {
        return Err(run_fail(format!(
            "no custom model URL configured — set models.custom_model_url in {}",
            paths::settings_path().display()
        )));
    }

    let downloader = ModelDownloader::new(model_root.to_path_buf());
    let coreml = coreml_encoder();
    let json = args.json;
    let quiet = args.quiet;

    let mut line = ProgressLine::new();
    let mut render = |update: DownloadProgress| report_download(json, quiet, &mut line, &update);

    let result = if args.model_type == ModelTypeArg::HuggingFace {
        // Validation guarantees `hfid` is `Some` for the huggingface type.
        let repo = args.hfid.as_deref().unwrap_or_default();
        downloader
            .resolve_hf_repo(repo, coreml, &mut render, cancel)
            .map(|resolved| (resolved.path, resolved.coreml_degraded))
    } else if is_custom {
        // `is_none()` was rejected above, so this unwrap-or-default never hits
        // the default.
        let url = settings
            .models
            .custom_model_url
            .as_deref()
            .unwrap_or_default();
        downloader
            .ensure_custom_model(url, &mut render, cancel)
            .map(|path| (path, false))
    } else {
        downloader
            .resolve_model(args.model_size, coreml, &mut render, cancel)
            .map(|resolved| (resolved.path, resolved.coreml_degraded))
    };

    // The closure's mutable borrow of `line` ends here (its last use is in the
    // `result` computation above), so `finish` can flush the final line.
    line.finish();

    match result {
        Ok((path, coreml_degraded)) => Ok(ResolvedModelInfo {
            path,
            coreml_degraded,
            hf_repo: if args.model_type == ModelTypeArg::HuggingFace {
                args.hfid.clone()
            } else {
                None
            },
        }),
        Err(err) => Err(run_fail(err)),
    }
}

/// Renders one download progress update — a `model_download_progress` JSON line
/// (`--json`) or a human stderr line (unless `--quiet`). Shared by the model and
/// VAD fetches (both are just artifacts to the user).
fn report_download(json: bool, quiet: bool, line: &mut ProgressLine, update: &DownloadProgress) {
    if json {
        if let Some(text) = to_json_line(&JsonEvent::ModelDownloadProgress {
            file: &update.file,
            downloaded: update.downloaded,
            total: update.total,
        }) {
            println!("{text}");
        }
    } else if !quiet {
        render_download_progress(line, update);
    }
}

/// Ensures the VAD model (F-045): the cached copy if present, else a one-time
/// fetch. Buzz ships `ggml-silero-v6.2.0.bin` next to whisper-cli at build time,
/// so every Buzz user has VAD by default; humm downloads it on first run to
/// match that outcome. Any fetch failure (offline, network blip) degrades to
/// `None` — VAD silently off, the run continues — which is `whisper_cpp.py:380`'s
/// absent-model behavior as the *failure* mode, not the default.
///
/// `HUMM_DISABLE_VAD` (any non-empty value) skips both the cache lookup and
/// the download outright, returning `None` immediately — a test-only
/// kill-switch (hermetic `humm-cli` tests would otherwise reach the network
/// on every run to fetch the Silero model) that never changes the default:
/// real users keep VAD on unless they set it.
fn ensure_vad(
    model_root: &std::path::Path,
    json: bool,
    quiet: bool,
    cancel: &CancelToken,
) -> Option<PathBuf> {
    if vad_disabled_from_env(std::env::var("HUMM_DISABLE_VAD").ok().as_deref()) {
        return None;
    }
    if let Some(path) = local_vad_model_path(model_root) {
        return Some(path);
    }
    let downloader = ModelDownloader::new(model_root.to_path_buf());
    let mut line = ProgressLine::new();
    let mut render = |update: DownloadProgress| report_download(json, quiet, &mut line, &update);
    let result = downloader.ensure_vad_model(&mut render, cancel);
    line.finish();
    match result {
        Ok(path) => Some(path),
        Err(err) => {
            log::warn!("VAD model unavailable ({err}); continuing without VAD");
            None
        }
    }
}

/// Builds the URL fetcher when yt-dlp is available, else `None` (URL tasks then
/// fail actionably — the queue's `YTDLP_MISSING` message). Cookie file resolves
/// `HUMM_DOWNLOAD_COOKIEFILE` (env wins) then `settings.downloads.cookie_file`.
fn build_fetcher(settings: &Settings) -> Option<Arc<dyn MediaFetcher>> {
    // No yt-dlp on this machine → no URL fetcher (URL tasks then fail
    // actionably in the queue). The version string itself is unused.
    probe_binaries().yt_dlp?;
    let cookie_file = std::env::var_os("HUMM_DOWNLOAD_COOKIEFILE")
        .map(PathBuf::from)
        .or_else(|| settings.downloads.cookie_file.clone());
    Some(Arc::new(YtDlpFetcher::new(cookie_file)))
}

/// The [`EngineFactory`] the runner uses: loads a [`WhisperEngine`] from the
/// pre-resolved model path. The runner caches per equal [`ModelSpec`], and one
/// CLI run has one spec, so `create` runs once.
struct CliEngineFactory {
    model_path: PathBuf,
    use_gpu: bool,
}

impl EngineFactory for CliEngineFactory {
    fn create(&mut self, _model: &ModelSpec) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        WhisperEngine::load(&self.model_path, self.use_gpu)
            .map(|engine| Box::new(engine) as Box<dyn TranscriptionEngine>)
            .map_err(CoreError::from)
    }
}

/// Drains the event stream until every enqueued task has reached a terminal
/// state (the runner stays alive until `finish`), rendering each event, and
/// returns the exit code: any canceled → 130; else any failed → 2; else 0.
fn drain_and_render(
    events: &flume::Receiver<TaskEvent>,
    enqueued: &[TranscriptionId],
    labels: &HashMap<TranscriptionId, String>,
    json: bool,
    quiet: bool,
) -> ExitCode {
    let mut remaining: HashSet<TranscriptionId> = enqueued.iter().copied().collect();
    let mut any_failed = false;
    let mut any_canceled = false;
    let mut human = HumanRenderer::new(quiet, labels);

    while !remaining.is_empty() {
        match events.recv() {
            Ok(event) => {
                if json {
                    emit_json(&event, labels);
                } else {
                    human.on_event(&event);
                }
                match &event.kind {
                    TaskEventKind::Completed { .. } => {
                        remaining.remove(&event.task_id);
                    }
                    TaskEventKind::Failed { .. } => {
                        any_failed = true;
                        remaining.remove(&event.task_id);
                    }
                    TaskEventKind::Canceled => {
                        any_canceled = true;
                        remaining.remove(&event.task_id);
                    }
                    _ => {}
                }
            }
            // The runner dropped every subscriber sender unexpectedly (it is
            // panic-free by construction, so this is not expected): stop with
            // whatever terminal states we saw.
            Err(_) => break,
        }
    }

    if any_canceled {
        ExitCode::from(130)
    } else if any_failed {
        ExitCode::from(2)
    } else {
        ExitCode::from(0)
    }
}

/// Installs the Ctrl-C handler: the first SIGINT cancels every queued/running
/// task (turning the run into exit 130); a second exits immediately. The
/// handler borrows the shared handle (`cancel_all` is `&self`); the main flow
/// later `take`s it for `finish`.
fn install_ctrlc(shared: Arc<Mutex<Option<QueueHandle>>>) {
    let signaled = Arc::new(AtomicBool::new(false));
    let result = ctrlc::set_handler(move || {
        if signaled.swap(true, Ordering::SeqCst) {
            // Second signal: don't wait for a graceful drain.
            std::process::exit(130);
        }
        if let Ok(guard) = shared.lock()
            && let Some(handle) = guard.as_ref()
        {
            handle.cancel_all();
        }
        eprintln!("canceling… (press Ctrl-C again to exit immediately)");
    });
    if let Err(err) = result {
        log::warn!("could not install Ctrl-C handler ({err}); cancellation via SIGINT disabled");
    }
}

// -- Human-readable rendering ------------------------------------------------

/// Renders task events as human-readable progress (stderr) and results
/// (written paths to stdout, errors to stderr). Tasks run one at a time, so a
/// single rewriting [`ProgressLine`] suffices.
struct HumanRenderer<'a> {
    line: ProgressLine,
    quiet: bool,
    labels: &'a HashMap<TranscriptionId, String>,
    started: HashMap<TranscriptionId, Instant>,
}

impl<'a> HumanRenderer<'a> {
    fn new(quiet: bool, labels: &'a HashMap<TranscriptionId, String>) -> Self {
        HumanRenderer {
            line: ProgressLine::new(),
            quiet,
            labels,
            started: HashMap::new(),
        }
    }

    fn label(&self, id: &TranscriptionId) -> String {
        self.labels
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }

    fn on_event(&mut self, event: &TaskEvent) {
        match &event.kind {
            TaskEventKind::Queued => {}
            TaskEventKind::Started => {
                self.started.insert(event.task_id, Instant::now());
                self.line.reset();
            }
            TaskEventKind::DownloadProgress(fraction) => {
                if self.quiet {
                    return;
                }
                let pct = pct_of(*fraction);
                let label = self.label(&event.task_id);
                // Negative key namespace keeps the download phase from
                // colliding with the (same-percent) transcribe phase.
                self.line
                    .update(-(pct + 1), &format!("downloading {label}  {pct}%"));
            }
            TaskEventKind::Progress(fraction) => {
                if self.quiet {
                    return;
                }
                let pct = pct_of(*fraction);
                let elapsed = self
                    .started
                    .get(&event.task_id)
                    .map(|start| start.elapsed().as_secs_f64())
                    .unwrap_or(0.0);
                let label = self.label(&event.task_id);
                let text = match format_eta(elapsed, *fraction) {
                    Some(eta) => format!("{label}  {pct}% — ETA: {eta}"),
                    None => format!("{label}  {pct}%"),
                };
                self.line.update(pct, &text);
            }
            TaskEventKind::SegmentAdded(_) => {}
            TaskEventKind::Completed { written } => {
                self.line.finish();
                for path in written {
                    println!("{}", path.display());
                }
            }
            TaskEventKind::Failed { message } => {
                self.line.finish();
                let label = self.label(&event.task_id);
                eprintln!("error: {label}: {message}");
            }
            TaskEventKind::Canceled => {
                self.line.finish();
            }
            TaskEventKind::StoreWriteFailed { operation, detail } => {
                if !self.quiet {
                    // Finish the `\r`-rewritten progress line first, so the
                    // warning gets its own line instead of appending to a
                    // partially-drawn one; the progress line re-renders
                    // cleanly on the next update (out of band — ADR-018 —
                    // this doesn't otherwise change the task's phase).
                    self.line.finish();
                    eprintln!("warning: history not recorded ({operation}): {detail}");
                }
            }
        }
    }
}

/// Throttled single-line progress renderer: rewrites the same terminal line via
/// `\r` on a TTY; emits plain change-triggered lines otherwise.
struct ProgressLine {
    tty: bool,
    last_key: Option<i64>,
}

impl ProgressLine {
    fn new() -> Self {
        ProgressLine {
            tty: std::io::stderr().is_terminal(),
            last_key: None,
        }
    }

    /// Forgets the last key, so the next `update` always renders (used at task
    /// boundaries where the percent counter restarts).
    fn reset(&mut self) {
        self.last_key = None;
    }

    fn update(&mut self, key: i64, text: &str) {
        if self.last_key == Some(key) {
            return;
        }
        self.last_key = Some(key);
        if self.tty {
            eprint!("\r{text}");
            let _ = std::io::stderr().flush();
        } else {
            eprintln!("{text}");
        }
    }

    fn finish(&mut self) {
        if self.tty && self.last_key.is_some() {
            eprintln!();
        }
        self.last_key = None;
    }
}

fn render_download_progress(progress: &mut ProgressLine, update: &DownloadProgress) {
    let downloaded_mb = update.downloaded as f64 / 1_000_000.0;
    match update.total.filter(|&total| total > 0) {
        Some(total) => {
            let percent = ((update.downloaded as f64 / total as f64) * 100.0).round() as i64;
            let total_mb = total as f64 / 1_000_000.0;
            progress.update(
                percent,
                &format!(
                    "Downloading {}  {percent}% ({downloaded_mb:.1}/{total_mb:.1} MB)",
                    update.file
                ),
            );
        }
        None => {
            let bucket = (update.downloaded / 1_000_000) as i64;
            progress.update(
                bucket,
                &format!("Downloading {}  ({downloaded_mb:.1} MB)", update.file),
            );
        }
    }
}

/// Estimated time remaining, ported from Buzz's in-progress ETA
/// (`buzz/widgets/transcription_tasks_table_widget.py:73-90`): with `elapsed >
/// 2s` and `0 < fraction < 1`, `remaining = (1 - p) * elapsed / p`, rendered
/// `NNm NNs` (≥ 60 s) or `NNs`. `None` when no meaningful estimate exists.
///
/// The remaining seconds are rounded to the nearest whole second before the
/// minute/second split — a deliberate, minor refinement over Buzz's truncation
/// that also absorbs the `f32`-fraction representation error (so, e.g.,
/// `(300.0, 0.2)` renders the intended `20m 0s`, not `19m 59s`).
fn format_eta(elapsed_secs: f64, fraction: f32) -> Option<String> {
    let p = f64::from(fraction);
    if elapsed_secs <= 2.0 || p <= 0.0 || p >= 1.0 {
        return None;
    }
    let remaining = ((1.0 - p) * elapsed_secs / p).round();
    let secs = remaining as i64;
    if secs >= 60 {
        Some(format!("{}m {}s", secs / 60, secs % 60))
    } else {
        Some(format!("{secs}s"))
    }
}

fn pct_of(fraction: f32) -> i64 {
    (fraction.clamp(0.0, 1.0) * 100.0).round() as i64
}

// -- JSON rendering ----------------------------------------------------------

/// The `--json` event schema (stable from M2; field names verbatim). Each
/// serializes as one line: `{"event":"<name>", ...}`.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum JsonEvent<'a> {
    ModelDownloadProgress {
        file: &'a str,
        downloaded: u64,
        total: Option<u64>,
    },
    Queued {
        task_id: String,
        input: &'a str,
    },
    Started {
        task_id: String,
    },
    DownloadProgress {
        task_id: String,
        fraction: f32,
    },
    Progress {
        task_id: String,
        fraction: f32,
    },
    Segment {
        task_id: String,
        start_ms: i64,
        end_ms: i64,
        text: &'a str,
    },
    Completed {
        task_id: String,
        written: Vec<String>,
    },
    Failed {
        task_id: String,
        error: &'a str,
    },
    Canceled {
        task_id: String,
    },
    StoreWriteFailed {
        task_id: String,
        operation: &'a str,
        detail: &'a str,
    },
}

/// Maps a [`TaskEvent`] to its [`JsonEvent`] and prints it as one line on
/// stdout. Nothing else is written to stdout in `--json` mode.
fn emit_json(event: &TaskEvent, labels: &HashMap<TranscriptionId, String>) {
    let task_id = event.task_id.to_string();
    let empty = String::new();
    let label = labels.get(&event.task_id).unwrap_or(&empty);
    let value = match &event.kind {
        TaskEventKind::Queued => JsonEvent::Queued {
            task_id,
            input: label,
        },
        TaskEventKind::Started => JsonEvent::Started { task_id },
        TaskEventKind::DownloadProgress(fraction) => JsonEvent::DownloadProgress {
            task_id,
            fraction: *fraction,
        },
        TaskEventKind::Progress(fraction) => JsonEvent::Progress {
            task_id,
            fraction: *fraction,
        },
        TaskEventKind::SegmentAdded(segment) => JsonEvent::Segment {
            task_id,
            start_ms: segment.start.0,
            end_ms: segment.end.0,
            text: &segment.text,
        },
        TaskEventKind::Completed { written } => JsonEvent::Completed {
            task_id,
            written: written.iter().map(|p| p.display().to_string()).collect(),
        },
        TaskEventKind::Failed { message } => JsonEvent::Failed {
            task_id,
            error: message,
        },
        TaskEventKind::Canceled => JsonEvent::Canceled { task_id },
        TaskEventKind::StoreWriteFailed { operation, detail } => JsonEvent::StoreWriteFailed {
            task_id,
            operation,
            detail,
        },
    };
    if let Some(text) = to_json_line(&value) {
        println!("{text}");
    }
}

/// Serializes `value` to a single JSON line, or `None` (logged) on the
/// otherwise-impossible serialization error — never printing a partial/empty
/// line that would corrupt the `--json` stream.
fn to_json_line(value: &impl Serialize) -> Option<String> {
    match serde_json::to_string(value) {
        Ok(text) => Some(text),
        Err(err) => {
            log::error!("failed to serialize JSON event: {err}");
            None
        }
    }
}

// -- Shared helpers ----------------------------------------------------------

fn coreml_encoder() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

fn output_formats(args: &AddArgs) -> BTreeSet<OutputFormat> {
    let mut formats = BTreeSet::new();
    if args.srt {
        formats.insert(OutputFormat::Srt);
    }
    if args.vtt {
        formats.insert(OutputFormat::Vtt);
    }
    if args.txt {
        formats.insert(OutputFormat::Txt);
    }
    formats
}

/// Empty string = auto-detect (`Ok(None)`); a non-empty, unrecognized code is
/// `cli.py`'s `"Invalid language option"` (`cli.py:226-227`).
fn parse_language(input: &str) -> Result<Option<Language>, &'static str> {
    if input.is_empty() {
        Ok(None)
    } else {
        Language::parse(input)
            .map(Some)
            .ok_or("Invalid language option")
    }
}

/// A usage/validation error: actionable cause + the `--help` hint, exit 1.
fn usage_fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {message}");
    eprintln!("Run `humm add --help` for usage.");
    ExitCode::from(1)
}

/// A runtime/config error (settings, model download, engine load): the
/// actionable cause alone, no usage hint, exit 1 (M1 messaging ticket).
fn run_fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(1)
}

/// `HUMM_FORCE_CPU` semantics mirror Buzz's `BUZZ_FORCE_CPU`
/// (`buzz/transcriber/whisper_cpp.py:191-193`): unset or `"false"` leaves GPU
/// use enabled; any other value forces CPU.
fn use_gpu_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some(v) if v != "false")
}

/// `HUMM_DISABLE_VAD` semantics: unset or empty leaves VAD on (the default for
/// real users); any non-empty value disables it — a test-only kill-switch so
/// hermetic `humm-cli` tests never reach the network for the Silero model.
fn vad_disabled_from_env(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

fn threads_from_env(value: Option<&str>) -> Option<u32> {
    value.and_then(|v| v.parse().ok())
}

fn paragraph_split_from_env(value: Option<&str>) -> Millis {
    value
        .and_then(|v| v.parse::<i64>().ok())
        .map(Millis)
        .unwrap_or(Millis(2000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::Segment;
    use pretty_assertions::assert_eq;

    // -- format_eta (Step 1) --------------------------------------------------

    #[test]
    fn format_eta_none_when_elapsed_under_two_seconds() {
        assert_eq!(format_eta(1.0, 0.5), None);
    }

    #[test]
    fn format_eta_seconds_only_under_a_minute() {
        assert_eq!(format_eta(10.0, 0.5), Some("10s".to_string()));
    }

    #[test]
    fn format_eta_minutes_and_seconds_over_a_minute() {
        assert_eq!(format_eta(300.0, 0.2), Some("20m 0s".to_string()));
    }

    #[test]
    fn format_eta_none_at_zero_or_full_fraction() {
        assert_eq!(format_eta(10.0, 0.0), None);
        assert_eq!(format_eta(10.0, 1.0), None);
    }

    // -- JSON event serialization (Step 1) ------------------------------------

    fn json_for(kind: TaskEventKind, id: &str, label: &str) -> String {
        let task_id = TranscriptionId::parse(id).expect("valid uuid");
        let mut labels = HashMap::new();
        labels.insert(task_id, label.to_string());
        // Re-derive exactly what `emit_json` would build, then serialize.
        let task_id_str = task_id.to_string();
        let value = match &kind {
            TaskEventKind::Queued => JsonEvent::Queued {
                task_id: task_id_str,
                input: label,
            },
            TaskEventKind::Started => JsonEvent::Started {
                task_id: task_id_str,
            },
            TaskEventKind::DownloadProgress(f) => JsonEvent::DownloadProgress {
                task_id: task_id_str,
                fraction: *f,
            },
            TaskEventKind::Progress(f) => JsonEvent::Progress {
                task_id: task_id_str,
                fraction: *f,
            },
            TaskEventKind::SegmentAdded(s) => JsonEvent::Segment {
                task_id: task_id_str,
                start_ms: s.start.0,
                end_ms: s.end.0,
                text: &s.text,
            },
            TaskEventKind::Completed { written } => JsonEvent::Completed {
                task_id: task_id_str,
                written: written.iter().map(|p| p.display().to_string()).collect(),
            },
            TaskEventKind::Failed { message } => JsonEvent::Failed {
                task_id: task_id_str,
                error: message,
            },
            TaskEventKind::Canceled => JsonEvent::Canceled {
                task_id: task_id_str,
            },
            TaskEventKind::StoreWriteFailed { operation, detail } => JsonEvent::StoreWriteFailed {
                task_id: task_id_str,
                operation,
                detail,
            },
        };
        to_json_line(&value).expect("serializes")
    }

    const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn json_model_download_progress_matches_schema() {
        let text = to_json_line(&JsonEvent::ModelDownloadProgress {
            file: "ggml-small.bin",
            downloaded: 1024,
            total: Some(487_000_000),
        })
        .expect("serializes");
        assert_eq!(
            text,
            r#"{"event":"model_download_progress","file":"ggml-small.bin","downloaded":1024,"total":487000000}"#
        );
    }

    #[test]
    fn json_queued_matches_schema() {
        assert_eq!(
            json_for(TaskEventKind::Queued, ID, "/path/or/url"),
            r#"{"event":"queued","task_id":"550e8400-e29b-41d4-a716-446655440000","input":"/path/or/url"}"#
        );
    }

    #[test]
    fn json_started_matches_schema() {
        assert_eq!(
            json_for(TaskEventKind::Started, ID, ""),
            r#"{"event":"started","task_id":"550e8400-e29b-41d4-a716-446655440000"}"#
        );
    }

    #[test]
    fn json_download_progress_matches_schema() {
        assert_eq!(
            json_for(TaskEventKind::DownloadProgress(0.5), ID, ""),
            r#"{"event":"download_progress","task_id":"550e8400-e29b-41d4-a716-446655440000","fraction":0.5}"#
        );
    }

    #[test]
    fn json_progress_matches_schema() {
        assert_eq!(
            json_for(TaskEventKind::Progress(0.5), ID, ""),
            r#"{"event":"progress","task_id":"550e8400-e29b-41d4-a716-446655440000","fraction":0.5}"#
        );
    }

    #[test]
    fn json_segment_matches_schema() {
        let segment = Segment {
            start: Millis(40),
            end: Millis(299),
            text: "hello".to_string(),
            translation: String::new(),
        };
        assert_eq!(
            json_for(TaskEventKind::SegmentAdded(segment), ID, ""),
            r#"{"event":"segment","task_id":"550e8400-e29b-41d4-a716-446655440000","start_ms":40,"end_ms":299,"text":"hello"}"#
        );
    }

    #[test]
    fn json_completed_matches_schema() {
        assert_eq!(
            json_for(
                TaskEventKind::Completed {
                    written: vec![PathBuf::from("/path.srt")]
                },
                ID,
                ""
            ),
            r#"{"event":"completed","task_id":"550e8400-e29b-41d4-a716-446655440000","written":["/path.srt"]}"#
        );
    }

    #[test]
    fn json_failed_matches_schema() {
        assert_eq!(
            json_for(
                TaskEventKind::Failed {
                    message: "boom".to_string()
                },
                ID,
                ""
            ),
            r#"{"event":"failed","task_id":"550e8400-e29b-41d4-a716-446655440000","error":"boom"}"#
        );
    }

    #[test]
    fn json_canceled_matches_schema() {
        assert_eq!(
            json_for(TaskEventKind::Canceled, ID, ""),
            r#"{"event":"canceled","task_id":"550e8400-e29b-41d4-a716-446655440000"}"#
        );
    }

    #[test]
    fn json_store_write_failed_matches_schema() {
        assert_eq!(
            json_for(
                TaskEventKind::StoreWriteFailed {
                    operation: "mark_started".to_string(),
                    detail: "database query failed: boom".to_string(),
                },
                ID,
                ""
            ),
            r#"{"event":"store_write_failed","task_id":"550e8400-e29b-41d4-a716-446655440000","operation":"mark_started","detail":"database query failed: boom"}"#
        );
    }

    // -- Validation / helpers (carried from M1) -------------------------------

    #[test]
    fn parse_language_empty_is_auto_detect() {
        assert_eq!(parse_language(""), Ok(None));
    }

    #[test]
    fn parse_language_known_code_resolves() {
        assert_eq!(parse_language("fr"), Ok(Language::parse("fr")));
    }

    #[test]
    fn parse_language_unknown_code_is_invalid_language_option() {
        assert_eq!(parse_language("xx"), Err("Invalid language option"));
    }

    #[test]
    fn validate_add_args_rejects_empty_files() {
        let args = base_add_args();
        assert!(validate_add_args(&args).is_some());
    }

    #[test]
    fn validate_add_args_rejects_extract_speech() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.extract_speech = true;
        assert!(validate_add_args(&args).is_some());
    }

    #[test]
    fn validate_add_args_rejects_openaiapi_model_type() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::OpenAiApi;
        assert!(validate_add_args(&args).is_some());
    }

    #[test]
    fn validate_add_args_rejects_huggingface_without_hfid() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::HuggingFace;
        args.hfid = None;
        assert!(validate_add_args(&args).is_some());
    }

    #[test]
    fn validate_add_args_accepts_huggingface_with_hfid() {
        // The huggingface backend is now wired (Task 11): a type + hfid pair
        // passes validation and resolves at runtime.
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::HuggingFace;
        args.hfid = Some("openai/whisper-tiny".to_string());
        assert!(validate_add_args(&args).is_none());
    }

    #[test]
    fn validate_add_args_ignores_hfid_for_non_huggingface_type() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::Whisper;
        args.hfid = Some("some/repo".to_string());
        assert!(validate_add_args(&args).is_none());
    }

    #[test]
    fn validate_add_args_ignores_openai_token_for_non_openaiapi_type() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::Whisper;
        args.openai_token = Some("token".to_string());
        assert!(validate_add_args(&args).is_none());
    }

    #[test]
    fn validate_add_args_accepts_local_backend_types() {
        for model_type in [
            ModelTypeArg::Whisper,
            ModelTypeArg::WhisperCpp,
            ModelTypeArg::FasterWhisper,
        ] {
            let mut args = base_add_args();
            args.files = vec!["dummy.mp3".to_string()];
            args.model_type = model_type;
            assert!(validate_add_args(&args).is_none());
        }
    }

    #[test]
    fn use_gpu_from_env_unset_allows_gpu() {
        assert!(use_gpu_from_env(None));
    }

    #[test]
    fn use_gpu_from_env_false_allows_gpu() {
        assert!(use_gpu_from_env(Some("false")));
    }

    #[test]
    fn use_gpu_from_env_any_other_value_forces_cpu() {
        assert!(!use_gpu_from_env(Some("true")));
        assert!(!use_gpu_from_env(Some("1")));
        assert!(!use_gpu_from_env(Some("")));
    }

    #[test]
    fn vad_disabled_from_env_unset_or_empty_leaves_vad_on() {
        assert!(!vad_disabled_from_env(None));
        assert!(!vad_disabled_from_env(Some("")));
    }

    #[test]
    fn vad_disabled_from_env_any_non_empty_value_disables_vad() {
        assert!(vad_disabled_from_env(Some("1")));
        assert!(vad_disabled_from_env(Some("true")));
        assert!(vad_disabled_from_env(Some("false")));
    }

    #[test]
    fn threads_from_env_parses_valid_value() {
        assert_eq!(threads_from_env(Some("4")), Some(4));
    }

    #[test]
    fn threads_from_env_none_when_unset_or_invalid() {
        assert_eq!(threads_from_env(None), None);
        assert_eq!(threads_from_env(Some("not-a-number")), None);
    }

    #[test]
    fn paragraph_split_from_env_defaults_to_2000() {
        assert_eq!(paragraph_split_from_env(None), Millis(2000));
        assert_eq!(paragraph_split_from_env(Some("bogus")), Millis(2000));
    }

    #[test]
    fn paragraph_split_from_env_parses_override() {
        assert_eq!(paragraph_split_from_env(Some("500")), Millis(500));
    }

    #[test]
    fn output_formats_collects_selected_flags_only() {
        let mut args = base_add_args();
        args.srt = true;
        args.txt = true;
        let formats = output_formats(&args);
        assert_eq!(
            formats,
            [OutputFormat::Srt, OutputFormat::Txt].into_iter().collect()
        );
    }

    #[test]
    fn output_formats_empty_when_no_flags_selected() {
        let args = base_add_args();
        assert!(output_formats(&args).is_empty());
    }

    #[test]
    fn pct_of_rounds_and_clamps() {
        assert_eq!(pct_of(0.0), 0);
        assert_eq!(pct_of(0.425), 43);
        assert_eq!(pct_of(1.0), 100);
        assert_eq!(pct_of(1.5), 100);
        assert_eq!(pct_of(-0.5), 0);
    }

    fn base_add_args() -> AddArgs {
        AddArgs {
            task: humm_core::domain::TaskKind::Transcribe,
            model_type: ModelTypeArg::Whisper,
            model_size: humm_core::domain::ModelSize::Small,
            hfid: None,
            language: String::new(),
            prompt: String::new(),
            word_timestamps: false,
            extract_speech: false,
            openai_token: None,
            srt: false,
            vtt: false,
            txt: false,
            output_directory: None,
            hide_gui: false,
            json: false,
            quiet: false,
            files: Vec::new(),
        }
    }
}
