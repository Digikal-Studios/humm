//! `humm add`: resolve the model, then drive one transcription task per input
//! through the persistent store + task queue (F-002, F-024, F-025, F-026,
//! F-029).
//!
//! Flow (parity order from `buzz/cli.py`, model resolved synchronously
//! *before* any task is queued — `cli.py:219-221`):
//! 1. Grammar/usage validation (Task 10) + language parse.
//! 2. Load [`Settings`] (`paths::settings_path()`).
//! 3. Open [`SqliteStore`]; first-run Buzz import (non-fatal), then acquire
//!    the single-instance lock (ADR-017) and run `cancel_stale_tasks()` only
//!    if it reports this process as the sole live instance (a crashed/killed
//!    prior run leaves stale `queued`/`in_progress` rows — F-029; but a
//!    *second, still-running* humm process must never have its live row
//!    flipped by this one — `docs/HUMM_SURFACE.md` §7).
//! 4. `-m openaiapi` only (F-002/F-041 completion): resolve the OpenAI access
//!    token before any model/VAD work — `--openai-token` if non-empty, else a
//!    keyring lookup (Buzz `cli.py:231-235`), else fail actionably: exit 1,
//!    no usage hint (a config error, not a usage error — `run_fail`).
//! 5. Local backends only: resolve the model synchronously, streaming
//!    download progress. The API backend has no local model to resolve, so
//!    this step — and the VAD ensure, next — is skipped entirely for it: no
//!    `model_download_progress` events either (design spec §2.1).
//! 6. Local backends only: ensure the VAD model (F-045): use the cached copy
//!    if present, else fetch it once. Buzz ships `ggml-silero-v6.2.0.bin` next
//!    to whisper-cli at build time (FEATURE_INVENTORY F-045), so every Buzz
//!    user has VAD by default; humm ships no file and downloads it on first
//!    run to match that outcome. A fetch failure degrades to no-VAD
//!    (`whisper_cpp.py:380`'s absent-model case) — the *failure* mode, never
//!    the default — and never fails the run. `HUMM_DISABLE_VAD` (test-only)
//!    skips this step outright.
//! 7. Spawn the task runner; enqueue one [`TaskSpec`] per input.
//! 8. Install a Ctrl-C handler: first signal cancels every task, second exits.
//! 9. Drain [`TaskEvent`]s, rendering progress/ETA (human) or one JSON object
//!    per line (`--json`); print written paths to stdout, errors to stderr.
//! 10. Exit code: any canceled → 130; else any failed → 2; else 0 (ADR-007).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

use humm_core::boundary::{
    AudioDecoder, AudioEncoder, MediaFetcher, MediaProber, SpeechIsolator, TaskStore,
    TranscriptionEngine,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, Input, ModelSize, ModelSpec, OutputFormat, OutputSpec, Source,
    TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::env::{
    keyring_disabled_from_env, threads_from_env, use_gpu_resolution, vad_disabled_from_env,
};
use humm_core::error::CoreError;
use humm_core::format::format_eta;
use humm_core::instance::{self, InstanceGuard};
use humm_core::language::Language;
use humm_core::models::download::{DownloadProgress, ModelDownloader};
use humm_core::models::registry::local_vad_model_path;
use humm_core::openai::{OpenAiAuth, OpenAiEngine, OpenAiEngineConfig};
use humm_core::paths;
use humm_core::pipeline::EngineTuning;
use humm_core::postproc::{PostprocConfig, PostprocConfigSource};
use humm_core::queue::{
    EngineFactory, QueueHandle, RunnerDeps, TaskEvent, TaskEventKind, TaskPhase, spawn_task_runner,
};
use humm_core::secrets::{SecretKey, SecretStore};
use humm_core::settings::{OpenAiSettings, PostprocSettings, Settings};
use humm_media::{
    DemucsIsolator, FfmpegDecoder, FfprobeProber, YtDlpFetcher, kill_all_registered_process_groups,
    probe_binaries,
};
use humm_store::{KeyringSecretStore, SqliteStore};
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

    // Only probed when actually needed (`-e` was passed) — cheap (a `PATH`
    // scan, no subprocess spawn, `DemucsIsolator::available`'s doc), but no
    // reason to touch the filesystem at all for the common case where the
    // flag isn't set (the value is never consulted in that case either).
    let demucs_available = if args.extract_speech {
        DemucsIsolator::new().available()
    } else {
        false
    };
    if let Some(exit_code) = validate_add_args(&args, demucs_available) {
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
///
/// `demucs_available` is the caller's already-probed answer to "is `demucs`
/// installed" (`DemucsIsolator::available`) — threaded in rather than probed
/// here so this function stays a pure, easily unit-tested decision (Task 5,
/// F-027/F-002): `-e` fails fast with an actionable install hint when it's
/// `false`, exactly like Buzz's own binary-missing checks elsewhere.
fn validate_add_args(args: &AddArgs, demucs_available: bool) -> Option<ExitCode> {
    if args.files.is_empty() {
        return Some(usage_fail("No input files"));
    }

    if args.extract_speech && !demucs_available {
        return Some(usage_fail(
            "demucs not found — install it (e.g. pipx install demucs) or omit --extract-speech",
        ));
    }

    if args.model_type == ModelTypeArg::HuggingFace && args.hfid.is_none() {
        // cli.py:146-147, verbatim.
        return Some(usage_fail(
            "--hfid is required when --model-type is huggingface",
        ));
    }

    // `-m whisper|whispercpp|fasterwhisper` all run the local whisper-rs
    // backend (ADR-008); `-m openaiapi` is real (F-041, `execute`'s token
    // resolution + `CliEngineFactory`'s API arm); `--hfid` with a
    // non-huggingface type and `--openai-token` without `-m openaiapi` are
    // both accepted-and-ignored exactly as Buzz does — neither needs code
    // here.
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

    // Single-instance stale-flip gate (ADR-017, `docs/HUMM_SURFACE.md` §7):
    // only a process that wins the exclusive claim is provably the sole live
    // humm process, so only it may treat every `in_progress`/`queued` row as
    // stale — otherwise this run could cancel a second, still-running
    // process's genuine work. `_instance` is bound (deliberately not
    // `let _ = ..`, which would drop the guard immediately) and lives until
    // `execute` returns, holding the claim for this run's whole lifetime.
    let (_instance, should_flip_stale_tasks) = acquire_instance_lock_or_fallback();
    if should_flip_stale_tasks {
        match store.cancel_stale_tasks() {
            Ok(0) => {}
            Ok(n) => log::info!("recovered {n} stale task(s) from a prior run as canceled"),
            Err(err) => log::warn!("cancel_stale_tasks failed (continuing): {err}"),
        }
    } else {
        log::info!("another humm process is running — skipping stale-task recovery");
    }

    // `-m openaiapi` token resolution (F-002/F-041 completion, Buzz
    // `cli.py:231-235`) runs before any model/VAD work — a missing token is a
    // config error, reported before the CLI plans any network I/O.
    let is_api_backend = args.model_type == ModelTypeArg::OpenAiApi;
    let openai_token = if is_api_backend {
        match resolve_openai_token(args) {
            Ok(token) => Some(token),
            Err(exit_code) => return exit_code,
        }
    } else {
        None
    };

    let model_root = paths::model_root();
    // The model download is cancellable in principle; the Ctrl-C handler is
    // installed later (brief step 8), so a SIGINT during a long model download
    // is the default terminate, matching Buzz.
    let download_cancel = CancelToken::new();
    // `HUMM_FORCE_CPU` wins whenever set (any non-"false" value); otherwise
    // `settings.models.force_cpu` decides (Preferences → General → Disable
    // GPU, design spec §8.1) — `settings` is already loaded above, so this is
    // a fresh, current-run read, matching Buzz's own always-fresh env check.
    let use_gpu = use_gpu_resolution(
        std::env::var("HUMM_FORCE_CPU").ok().as_deref(),
        settings.models.force_cpu,
    );

    // The API backend has no local model or VAD model to resolve (design
    // spec §2.1): both the synchronous model-download phase and the VAD
    // ensure are skipped entirely for it — no `model_download_progress`
    // events, no `custom_model_url`/HF/size network touch. Local backends
    // keep today's exact behavior.
    let (model_path, hf_repo, vad_model) = if is_api_backend {
        (PathBuf::new(), None, None)
    } else {
        let resolved = match resolve_model(args, &settings, &model_root, &download_cancel) {
            Ok(resolved) => resolved,
            Err(exit_code) => return exit_code,
        };
        if resolved.coreml_degraded && !args.json {
            eprintln!(
                "Note: the CoreML encoder could not be downloaded (network issue); using the \
                 local model without CoreML acceleration. It will be retried on the next run."
            );
        }
        let vad_model = ensure_vad(&model_root, args.json, args.quiet, &download_cancel);
        (resolved.path, resolved.hf_repo, vad_model)
    };

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
            backend: if is_api_backend {
                Backend::OpenAiApi
            } else {
                Backend::Whisper
            },
            size: args.model_size,
            hf_repo,
        },
        word_timestamps: args.word_timestamps,
        initial_prompt: args.prompt.clone(),
        temperature: DEFAULT_TEMPERATURE.to_vec(),
        // No CLI flag yet enables LLM translation (F-031 wiring is a later
        // M6 task).
        llm: None,
        // `validate_add_args` above already fails fast with an actionable
        // install hint when this is `true` and demucs isn't found (Task 5,
        // F-027/F-002) — reaching here means either it's `false`, or it's
        // `true` and the isolator wired into `RunnerDeps` below is real.
        extract_speech: args.extract_speech,
    };
    let output = OutputSpec {
        formats,
        directory: args.output_directory.clone(),
        // `None` uses `export::DEFAULT_EXPORT_TEMPLATE` (design spec §8.1:
        // wired into every spec-build site).
        file_name_template: settings.file_transcriber.default_export_template.clone(),
        paragraph_split: humm_core::viewer::text_view::paragraph_split_from_env(),
        // The CLI has no folder-watch surface (Buzz or humm, design spec
        // §3.5) -- it never sets a source disposition.
        source_disposition: None,
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
            model_path,
            use_gpu,
            openai_token,
            openai_settings: settings.openai.clone(),
        }),
        tuning,
        media_dir: paths::media_dir(),
        model_root,
        // The CLI resolves the model synchronously before enqueueing (Buzz
        // parity, `cli.py:219-221`) — no ArtifactEnsurer is ever configured.
        artifacts: None,
        // No CLI flag enables LLM translation in M6 (Buzz's CLI has none
        // either, design spec §6) — the file path's translation stays inert.
        llm: None,
        // The CLI runs post-processing hooks too (Buzz parity, design spec
        // §2.4/§11) — always wired, since each hook's own `enabled` flag is
        // what actually gates it; `settings.postproc` is loaded once here,
        // matching the CLI's no-live-editing-surface pattern
        // (`CliEngineFactory::openai_settings`, above).
        postproc: Some(Arc::new(CliPostprocSource {
            postproc: settings.postproc.clone(),
        }) as Arc<dyn PostprocConfigSource>),
        // M8 pre-transcription seam. The CLI's `-e` flag wires a real
        // `isolator` (design spec §9's F-002 "`-e` goes live" row) — always
        // constructed (not gated on `args.extract_speech`), since
        // `queue::Runner::run_job` only ever calls `isolate` when the
        // *task's own* `options.extract_speech` is set (`RunnerDeps::
        // isolator`'s doc). `reducer`/`lang_detector` stay `None` on the CLI,
        // unlike `postproc` above (which the CLI wires, not `None`): F-112
        // noise reduction and F-114 language detection are GUI-configured-
        // only pre-hooks with no CLI adapter yet, so a CLI user who enables
        // `noise_reduction.enabled`/`language_detection.enabled` in
        // settings.toml sees no effect and no error — `run_job`'s own gate
        // is `cfg.enabled && self.reducer/lang_detector.is_some()`, and the
        // CLI's `None` here silently fails that second half. A real
        // deviation from GUI parity (see `docs/HUMM_SURFACE.md`'s
        // postproc-hook table and this milestone's deviations ledger), not
        // a deliberate "postproc: None" stance — the CLI's `postproc` field
        // is `Some` and does run skip/resizer/summary/docx from
        // settings.toml.
        isolator: Some(Arc::new(DemucsIsolator::new()) as Arc<dyn SpeechIsolator>),
        reducer: None,
        lang_detector: None,
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

/// Acquires the single-instance lock (ADR-017) at `paths::data_dir()` and
/// reports whether the stale-task flip should run this time. Returns the held
/// [`InstanceGuard`] (`None` only on the I/O-error fallback below, where no
/// claim was taken) alongside that decision.
///
/// Callers must bind the guard with `let _instance = ..` — never `let _ =
/// ..`, which drops a temporary immediately rather than holding it — and keep
/// that binding alive for the rest of the run, so the claim is held until
/// this process exits (`execute`'s local scope does this naturally).
///
/// A lock-file I/O error (unwritable `data_dir`, permissions) is logged and
/// treated as "flip anyway": a lock failure must never brick the CLI, so this
/// falls back to exactly today's pre-ADR-017 unconditional behavior.
fn acquire_instance_lock_or_fallback() -> (Option<InstanceGuard>, bool) {
    match instance::acquire_instance_lock(&paths::data_dir()) {
        Ok(lock) => (Some(lock.guard), lock.sole_instance),
        Err(err) => {
            log::warn!(
                "could not acquire the single-instance lock ({err}); falling back to \
                 unconditional stale-task recovery"
            );
            (None, true)
        }
    }
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

/// Resolves the OpenAI access token for `-m openaiapi` (F-002/F-041
/// completion, Buzz `cli.py:231-235`, exact): `--openai-token` wins if
/// non-empty; else a keyring lookup (`SecretKey::OpenAiApiKey`, ADR-012);
/// both empty (or the lookup itself fails — Buzz's own `get_password`
/// swallows every backend error into `""`, `keyring_store.py:216-218`, so
/// humm's own backend failure collapses to the same outcome) fails with
/// Buzz's exact message, config-error style (`run_fail`: the actionable
/// cause alone, no `--help` hint, exit 1) — called from `execute` before any
/// model-resolution/VAD/queue work runs, so a missing token is reported
/// before the CLI plans any network I/O.
///
/// `HUMM_DISABLE_KEYRING` (test-only, any non-empty value) skips the keyring
/// lookup entirely, reporting "no entry" without ever opening a real
/// `keyring::Entry` — hermetic `humm-cli` tests would otherwise reach the
/// real OS keychain on every `-m openaiapi` invocation with no
/// `--openai-token`.
fn resolve_openai_token(args: &AddArgs) -> Result<String, ExitCode> {
    const NOT_FOUND: &str = "No OpenAI access token found";

    if let Some(token) = args.openai_token.as_deref()
        && !token.is_empty()
    {
        return Ok(token.to_string());
    }

    if keyring_disabled_from_env(std::env::var("HUMM_DISABLE_KEYRING").ok().as_deref()) {
        return Err(run_fail(NOT_FOUND));
    }

    match KeyringSecretStore::new().get(SecretKey::OpenAiApiKey) {
        Ok(Some(token)) if !token.is_empty() => Ok(token),
        Ok(_) => Err(run_fail(NOT_FOUND)),
        Err(err) => {
            log::warn!("keyring lookup failed ({err}); treating as no stored token");
            Err(run_fail(NOT_FOUND))
        }
    }
}

/// The CLI's [`PostprocConfigSource`] (design spec §2.4): `settings.postproc`,
/// loaded once at startup — the CLI has no live-editing surface (unlike the
/// GUI's Preferences window), so a single snapshot cloned at `execute` time
/// is enough.
struct CliPostprocSource {
    postproc: PostprocSettings,
}

impl PostprocConfigSource for CliPostprocSource {
    fn snapshot(&self) -> PostprocConfig {
        let summary = self.postproc.ai_summary.clone();
        // Only resolve the key when the hook that uses it is actually
        // enabled: `ai_summary` defaults to off (Buzz plugin parity), so
        // this keeps every ordinary `humm add` run — and every existing
        // hermetic CLI test, none of which enable it — from touching the
        // real OS keychain at all, exactly like `resolve_openai_token`'s own
        // `is_api_backend` gate above.
        let summary_api_key = if summary.enabled {
            resolve_ai_summary_key()
        } else {
            None
        };
        PostprocConfig {
            skip: self.postproc.skip_already_transcribed.clone(),
            summary,
            summary_api_key,
            docx: self.postproc.export_docx.clone(),
            resize: self.postproc.transcript_resizer.clone(),
            paragraph_split: humm_core::viewer::text_view::paragraph_split_from_env(),
            noise_reduction: self.postproc.noise_reduction.clone(),
            language_detection: self.postproc.language_detection.clone(),
        }
    }
}

/// Resolves the AI-summary hook's API key (F-111) the same way
/// [`resolve_openai_token`] resolves the transcription key: a real keyring
/// lookup, skipped (reporting "absent") under `HUMM_DISABLE_KEYRING` so
/// hermetic tests never touch the real OS keychain. Unlike
/// `resolve_openai_token`, an absent/failed key is not a run-ending error —
/// the `ai_summary` hook's own `enabled` gate and empty-key skip already
/// handle that (design spec §6.2), so this simply returns `None`.
fn resolve_ai_summary_key() -> Option<String> {
    if keyring_disabled_from_env(std::env::var("HUMM_DISABLE_KEYRING").ok().as_deref()) {
        return None;
    }
    match KeyringSecretStore::new().get(SecretKey::AiSummaryApiKey) {
        Ok(Some(key)) if !key.is_empty() => Some(key),
        Ok(_) => None,
        Err(err) => {
            log::warn!("keyring lookup for AI Summary API key failed: {err}");
            None
        }
    }
}

/// The [`EngineFactory`] the runner uses: routes on [`Backend`]. `Whisper`
/// loads a [`WhisperEngine`] from the pre-resolved model path; `OpenAiApi`
/// builds an [`OpenAiEngine`] from the resolved token + `[openai]` settings.
/// The runner caches per equal [`ModelSpec`], and one CLI run has one spec,
/// so `create` runs once.
struct CliEngineFactory {
    /// The pre-resolved local model path (`Whisper` only; unused —
    /// `PathBuf::new()` — for `OpenAiApi`, which has no local model).
    model_path: PathBuf,
    use_gpu: bool,
    /// The resolved OpenAI access token (`resolve_openai_token`). `Some` iff
    /// `-m openaiapi` was requested — `execute` never builds an `OpenAiApi`
    /// [`ModelSpec`] without a successfully resolved token, so the `create`
    /// arm below only reaches its `unwrap_or_default` fallback in the
    /// otherwise-unreachable case of a caller wiring this factory directly.
    openai_token: Option<String>,
    /// `settings.openai` (base URL + API model), read once at `execute` time
    /// — Buzz reads its equivalent `QSettings` at transcriber construction
    /// too (`openai_whisper_api_file_transcriber.py:41-53`).
    openai_settings: OpenAiSettings,
}

impl EngineFactory for CliEngineFactory {
    fn create(
        &mut self,
        model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        // `_resolved` is ignored for both arms: the CLI already resolved
        // whatever a backend needs before enqueueing (`resolve_model`/
        // `resolve_openai_token`, above) — no ArtifactEnsurer is ever
        // configured for the CLI (`RunnerDeps.artifacts: None`), so this is
        // always `None` in practice anyway.
        match model.backend {
            Backend::Whisper => WhisperEngine::load(&self.model_path, self.use_gpu)
                .map(|engine| Box::new(engine) as Box<dyn TranscriptionEngine>)
                .map_err(CoreError::from),
            Backend::OpenAiApi => {
                let encoder: Arc<dyn AudioEncoder> = Arc::new(FfmpegDecoder::new());
                let engine = OpenAiEngine::new(
                    OpenAiEngineConfig {
                        auth: OpenAiAuth {
                            base_url: self.openai_settings.base_url.clone(),
                            api_key: self.openai_token.clone().unwrap_or_default(),
                        },
                        api_model: self.openai_settings.api_model.clone(),
                    },
                    encoder,
                );
                Ok(Box::new(engine) as Box<dyn TranscriptionEngine>)
            }
        }
    }
}

/// Drains the event stream until every enqueued task has reached a terminal
/// state (the runner stays alive until `finish`), rendering each event, and
/// returns the exit code: any canceled → 130; else any failed → 2; else 0
/// (`Skipped` counts toward the `0` arm, same as `Completed` — Buzz treats a
/// skip as completion, `main_window.py:435-443`).
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
                    TaskEventKind::Skipped => {
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
///
/// Locking `shared` (a plain `std::sync::Mutex`) inside this closure is
/// sound: the `ctrlc` crate's own signal handler (the thing actually
/// registered via `sigaction`) does nothing but `sem_post` an
/// async-signal-safe semaphore and return immediately — this closure never
/// runs in that async-signal context at all. `ctrlc::set_handler` instead
/// spawns a dedicated, ordinary `"ctrl-c"` OS thread that blocks on that
/// semaphore (`sem_wait_forever`) and invokes this closure only after that
/// blocking wait returns, i.e. in normal thread context, exactly where a
/// `Mutex::lock` (or, below, `kill_all_registered_process_groups`'s own
/// lock) is fine to call (confirmed by reading the vendored `ctrlc` 3.5.2
/// source, `src/lib.rs`'s `set_handler_inner` and
/// `src/platform/unix/mod.rs`'s `os_handler`/`block_ctrl_c`).
fn install_ctrlc(shared: Arc<Mutex<Option<QueueHandle>>>) {
    let signaled = Arc::new(AtomicBool::new(false));
    let result = ctrlc::set_handler(move || {
        if signaled.swap(true, Ordering::SeqCst) {
            // Second signal: don't wait for a graceful drain. Emergency
            // last-resort sweep first (D5b-2/D5b-4, M9): SIGKILLs any
            // process-grouped demucs/yt-dlp children still registered,
            // since `std::process::exit` bypasses Rust's normal
            // unwind/`Drop` machinery entirely — nothing else would ever
            // signal their detached process groups, orphaning them
            // (`crates/humm-media/src/demucs.rs`'s module doc explains why
            // they're detached from this terminal's foreground group in
            // the first place, so a second Ctrl-C at the terminal never
            // reaches them directly either).
            kill_all_registered_process_groups();
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
            TaskEventKind::ModelDownloadProgress { .. } => {
                // Unreachable by construction: the CLI resolves its model
                // synchronously before enqueueing and never configures an
                // ArtifactEnsurer (`RunnerDeps.artifacts` stays `None`), so
                // the runner never emits this variant here. Render nothing.
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
            TaskEventKind::Skipped => {
                // A terminal state, same footing as `Completed`/`Failed`
                // above: always printed, never gated by `--quiet` (quiet
                // only suppresses progress/warning lines).
                self.line.finish();
                let label = self.label(&event.task_id);
                eprintln!("{}", skipped_line(&label));
            }
            TaskEventKind::PostProcessed { errors, .. } => {
                // Follows `Completed`, which already finished the progress
                // line — nothing to do here but report hook failures, if
                // any (never fails the task itself). A warning line, so
                // `--quiet` suppresses it like `StoreWriteFailed`'s.
                if self.quiet {
                    return;
                }
                for err in errors {
                    eprintln!("warning: post-processing: {err}");
                }
            }
            TaskEventKind::PhaseChanged { phase } => {
                if self.quiet {
                    return;
                }
                let label = self.label(&event.task_id);
                match phase {
                    // Indeterminate (module doc on `TaskPhase::IsolatingSpeech`):
                    // no fraction to render, just the phase name — a distinct
                    // key namespace from `Progress`'s `0..100` and
                    // `DownloadProgress`'s negative range keeps this from
                    // being mistaken for either.
                    TaskPhase::IsolatingSpeech => {
                        self.line
                            .update(PHASE_ISOLATING_KEY, &isolating_speech_line(&label));
                    }
                }
            }
        }
    }
}

/// [`ProgressLine`] key for [`TaskPhase::IsolatingSpeech`] — outside both
/// `Progress`'s `0..100` and `DownloadProgress`'s `-1..-101` ranges, so it
/// can never collide with either.
const PHASE_ISOLATING_KEY: i64 = i64::MIN;

/// The stderr line printed for a `Skipped` event's terminal state, human
/// mode: `<label>  skipped (already transcribed)` — the progress line's own
/// `<label>  NN%` convention, but for a task that never ran (F-115; Buzz
/// counts a skip toward "quit on complete", `main_window.py:435-443`).
fn skipped_line(label: &str) -> String {
    format!("{label}  skipped (already transcribed)")
}

/// The progress-line text rendered for [`TaskEventKind::PhaseChanged`]'s
/// [`TaskPhase::IsolatingSpeech`], human mode: `<label>  isolating speech` —
/// the progress line's own `<label>  NN%` convention, but for a phase with
/// no fraction to report (demucs only emits human-oriented tqdm on stderr,
/// which Standing Policy rule 3 forbids parsing).
fn isolating_speech_line(label: &str) -> String {
    format!("{label}  isolating speech")
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
    Skipped {
        task_id: String,
    },
    PostProcessed {
        task_id: String,
        notes_updated: bool,
        segments_replaced: bool,
        written: Vec<String>,
        errors: &'a [String],
    },
    Phase {
        task_id: String,
        phase: &'static str,
    },
}

/// Maps a [`TaskPhase`] to its stable `--json` wire string (`docs/HUMM_SURFACE.md`
/// §3): `snake_case`, matching the schema's other event/field naming.
fn task_phase_wire_str(phase: TaskPhase) -> &'static str {
    match phase {
        TaskPhase::IsolatingSpeech => "isolating_speech",
    }
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
        TaskEventKind::Skipped => JsonEvent::Skipped { task_id },
        TaskEventKind::PostProcessed {
            notes_updated,
            segments_replaced,
            written,
            errors,
        } => JsonEvent::PostProcessed {
            task_id,
            notes_updated: *notes_updated,
            segments_replaced: *segments_replaced,
            written: written.iter().map(|p| p.display().to_string()).collect(),
            errors,
        },
        TaskEventKind::ModelDownloadProgress { .. } => {
            // Unreachable by construction (see `HumanRenderer::on_event`'s
            // matching arm): the CLI never configures an ArtifactEnsurer, so
            // the runner never emits this variant here. Print nothing —
            // this is *not* the pre-queue `model_download_progress` JSON
            // event (`report_download`, above), which is unaffected.
            return;
        }
        TaskEventKind::PhaseChanged { phase } => JsonEvent::Phase {
            task_id,
            phase: task_phase_wire_str(*phase),
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

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::{Millis, Segment};
    use pretty_assertions::assert_eq;

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
            TaskEventKind::Skipped => JsonEvent::Skipped {
                task_id: task_id_str,
            },
            TaskEventKind::PostProcessed {
                notes_updated,
                segments_replaced,
                written,
                errors,
            } => JsonEvent::PostProcessed {
                task_id: task_id_str,
                notes_updated: *notes_updated,
                segments_replaced: *segments_replaced,
                written: written.iter().map(|p| p.display().to_string()).collect(),
                errors,
            },
            TaskEventKind::ModelDownloadProgress { .. } => {
                // `emit_json` never reaches its own matching arm for this
                // variant either (see its comment) — no test exercises this
                // arm of `json_for` with it.
                unreachable!("ModelDownloadProgress is never routed through emit_json/json_for")
            }
            TaskEventKind::PhaseChanged { phase } => JsonEvent::Phase {
                task_id: task_id_str,
                phase: task_phase_wire_str(*phase),
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

    #[test]
    fn json_skipped_matches_schema() {
        assert_eq!(
            json_for(TaskEventKind::Skipped, ID, ""),
            r#"{"event":"skipped","task_id":"550e8400-e29b-41d4-a716-446655440000"}"#
        );
    }

    #[test]
    fn json_post_processed_matches_schema() {
        assert_eq!(
            json_for(
                TaskEventKind::PostProcessed {
                    notes_updated: true,
                    segments_replaced: false,
                    written: vec![PathBuf::from("/out.docx")],
                    errors: vec!["summary: timed out".to_string()],
                },
                ID,
                ""
            ),
            r#"{"event":"post_processed","task_id":"550e8400-e29b-41d4-a716-446655440000","notes_updated":true,"segments_replaced":false,"written":["/out.docx"],"errors":["summary: timed out"]}"#
        );
    }

    #[test]
    fn json_post_processed_empty_written_and_errors() {
        assert_eq!(
            json_for(
                TaskEventKind::PostProcessed {
                    notes_updated: false,
                    segments_replaced: true,
                    written: vec![],
                    errors: vec![],
                },
                ID,
                ""
            ),
            r#"{"event":"post_processed","task_id":"550e8400-e29b-41d4-a716-446655440000","notes_updated":false,"segments_replaced":true,"written":[],"errors":[]}"#
        );
    }

    #[test]
    fn json_phase_matches_schema() {
        assert_eq!(
            json_for(
                TaskEventKind::PhaseChanged {
                    phase: TaskPhase::IsolatingSpeech,
                },
                ID,
                ""
            ),
            r#"{"event":"phase","task_id":"550e8400-e29b-41d4-a716-446655440000","phase":"isolating_speech"}"#
        );
    }

    // -- Human-mode rendering (Step 2) ----------------------------------------

    #[test]
    fn skipped_line_reports_already_transcribed() {
        // `HumanRenderer::on_event` has no injectable writer (it prints
        // directly to stdout/stderr), so this pins the exact terminal-line
        // text it emits for `Skipped` via the same pure helper `on_event`
        // calls, rather than capturing process-wide stderr.
        assert_eq!(
            skipped_line("clip.mp3"),
            "clip.mp3  skipped (already transcribed)"
        );
    }

    #[test]
    fn isolating_speech_line_renders_phase_text() {
        // Same "no injectable writer" rationale as `skipped_line_reports_
        // already_transcribed` above: pins the exact text `on_event`'s
        // `PhaseChanged` arm passes to the progress line.
        assert_eq!(
            isolating_speech_line("clip.mp3"),
            "clip.mp3  isolating speech"
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
        assert!(validate_add_args(&args, false).is_some());
    }

    #[test]
    fn validate_add_args_rejects_extract_speech_when_demucs_unavailable() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.extract_speech = true;
        let exit_code = validate_add_args(&args, false);
        assert!(exit_code.is_some());
    }

    #[test]
    fn validate_add_args_accepts_extract_speech_when_demucs_available() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.extract_speech = true;
        assert!(validate_add_args(&args, true).is_none());
    }

    #[test]
    fn validate_add_args_ignores_demucs_availability_when_extract_speech_unset() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.extract_speech = false;
        assert!(validate_add_args(&args, false).is_none());
    }

    // `resolve_openai_token`'s no-flag branches fall through to a real
    // `KeyringSecretStore` lookup (guarded only by `HUMM_DISABLE_KEYRING`,
    // an env var this in-process unit test must never set — that would
    // mutate global process state shared with every other test in this
    // binary). Only the flag-wins branch is safe to exercise here; the
    // keyring-fallback and both-empty branches are cli-int tested instead
    // (`crates/humm-cli/tests/cli.rs`, which sets `HUMM_DISABLE_KEYRING` in
    // an isolated subprocess).
    #[test]
    fn resolve_openai_token_uses_the_flag_when_non_empty() {
        let mut args = base_add_args();
        args.openai_token = Some("sk-test-token".to_string());
        assert_eq!(resolve_openai_token(&args).unwrap(), "sk-test-token");
    }

    // `CliPostprocSource::snapshot`'s `ai_summary.enabled == true` branch
    // falls through to a real keyring lookup (`resolve_ai_summary_key`),
    // gated only by `HUMM_DISABLE_KEYRING` — the same env var this
    // in-process test module must never set (see the comment above). Only
    // the disabled-gate branch, which never touches the keyring at all, is
    // safe to exercise here.
    #[test]
    fn postproc_snapshot_skips_keyring_when_ai_summary_disabled() {
        let source = CliPostprocSource {
            postproc: PostprocSettings::default(),
        };
        let cfg = source.snapshot();
        assert!(!cfg.summary.enabled);
        assert_eq!(cfg.summary_api_key, None);
    }

    #[test]
    fn postproc_snapshot_carries_skip_settings_through() {
        let postproc = PostprocSettings {
            skip_already_transcribed: humm_core::settings::SkipSettings {
                enabled: true,
                check_database: true,
                ..humm_core::settings::SkipSettings::default()
            },
            ..PostprocSettings::default()
        };
        let source = CliPostprocSource { postproc };
        let cfg = source.snapshot();
        assert!(cfg.skip.enabled);
        assert!(cfg.skip.check_database);
    }

    #[test]
    fn validate_add_args_accepts_openaiapi_model_type() {
        // `-m openaiapi` is real now (Task 11, F-002/F-041 completion):
        // grammar validation accepts it unconditionally — token resolution
        // happens later, in `execute`, not here.
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::OpenAiApi;
        assert!(validate_add_args(&args, false).is_none());
    }

    #[test]
    fn validate_add_args_rejects_huggingface_without_hfid() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::HuggingFace;
        args.hfid = None;
        assert!(validate_add_args(&args, false).is_some());
    }

    #[test]
    fn validate_add_args_accepts_huggingface_with_hfid() {
        // The huggingface backend is now wired (Task 11): a type + hfid pair
        // passes validation and resolves at runtime.
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::HuggingFace;
        args.hfid = Some("openai/whisper-tiny".to_string());
        assert!(validate_add_args(&args, false).is_none());
    }

    #[test]
    fn validate_add_args_ignores_hfid_for_non_huggingface_type() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::Whisper;
        args.hfid = Some("some/repo".to_string());
        assert!(validate_add_args(&args, false).is_none());
    }

    #[test]
    fn validate_add_args_ignores_openai_token_for_non_openaiapi_type() {
        let mut args = base_add_args();
        args.files = vec!["dummy.mp3".to_string()];
        args.model_type = ModelTypeArg::Whisper;
        args.openai_token = Some("token".to_string());
        assert!(validate_add_args(&args, false).is_none());
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
            assert!(validate_add_args(&args, false).is_none());
        }
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
