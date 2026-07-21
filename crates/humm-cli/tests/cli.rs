//! Integration tests for the `humm` CLI (`add` subcommand), driven through
//! `assert_cmd` against the real built binary.
//!
//! Three tiers:
//! - **Grammar/validation** (tests 1–18): no store, model, or network — a
//!   usage/parse error is reached before any runtime work.
//! - **Wired cli-int** (`add_*` below): a hermetic temp `HUMM_DATA_DIR`/
//!   `HUMM_CONFIG_DIR`/`HUMM_MODEL_ROOT` seeded with the Buzz stub `ggml-tiny`
//!   (loads under CPU, transcribes to zero segments — Global Constraint 4), so
//!   the full store + queue path runs network-free. Buzz import is disabled
//!   (`HUMM_BUZZ_SQLITE` → a nonexistent path) and GPU is forced off
//!   (`HUMM_FORCE_CPU`) so they run under the sandbox.
//! - **Acceptance** (`acceptance_*`, all `#[ignore]`): real tiny model + real
//!   audio, process signals, and a local HTTP server. Run sandbox-lifted with
//!   `cargo test -p humm-cli -- --ignored`.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as StdCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use predicates::prelude::*;

use humm_core::boundary::TaskStore;
use humm_core::domain::{
    Input, ModelSize, OutputFormat, OutputSpec, Source, Status, TaskSpec, TranscriptionId,
    TranscriptionOptions,
};
use humm_core::models::registry::{SENTINEL_SUFFIX, coreml_no_encoder_marker, ggml_file_name};
use humm_store::SqliteStore;

fn humm_cmd() -> Command {
    Command::cargo_bin("humm").expect("humm binary should build")
}

fn dummy_input(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"not real media, just needs to exist").expect("write dummy input");
    path
}

/// Root of Buzz's test fixtures (honoring `HUMM_BUZZ_TESTDATA`).
fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

// 1. `humm` with no subcommand -> clap help to stderr, exit 1 (ADR-007
// headless deviation; Buzz would launch the GUI here).
#[test]
fn no_subcommand_exits_1() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("add"));
}

// 2. `humm add` with zero file positionals -> exit 1, "No input files"
// (cli.py:212-213).
#[test]
fn add_without_files_says_no_input_files() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("No input files"));
}

// 3. Unknown language code -> exit 1, "Invalid language option"
// (cli.py:226-227), reached before any store/model work; keeps the usage hint.
#[test]
fn invalid_language_rejected() {
    let log_dir = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let input = dummy_input(input_dir.path(), "dummy.mp3");

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("-l")
        .arg("xx")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("Invalid language option")
                .and(predicate::str::contains("Run `humm add --help`")),
        );
}

// 4. Invalid --task value -> clap value-enum error, exit 1.
#[test]
fn invalid_task_rejected() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("-t")
        .arg("foo")
        .assert()
        .code(1);
}

// 5. Invalid --model-size value -> clap value-enum error, exit 1.
#[test]
fn invalid_size_rejected() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("-s")
        .arg("foo")
        .assert()
        .code(1);
}

// 8. `add --help` -> exit 0, lists flags and their defaults.
#[test]
fn help_lists_flags_and_defaults() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("--help")
        .assert()
        .code(0)
        .stdout(
            predicate::str::contains("--word-timestamps")
                .and(predicate::str::contains("--model-size"))
                .and(predicate::str::contains("small")),
        );
}

// 9. Every invocation writes to logs.txt in HUMM_LOG_DIR, even one that
// fails fast on validation (F-009 cli-int verification).
#[test]
fn log_file_written() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .assert()
        .code(1);

    let log_file = log_dir.path().join("logs.txt");
    assert!(
        log_file.is_file(),
        "expected {} to exist",
        log_file.display()
    );
    let contents = fs::read_to_string(&log_file).unwrap();
    assert!(!contents.trim().is_empty(), "expected non-empty log file");
}

// 10. `-e/--extract-speech` -> exit 1, exact not-yet-available message.
#[test]
fn extract_speech_not_yet_available() {
    let log_dir = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let input = dummy_input(input_dir.path(), "dummy.mp3");

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("-e")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "speech extraction is not yet available in humm — it arrives in a later release",
        ));
}

// 11. `-m openaiapi` -> exit 1, exact not-yet-available message.
#[test]
fn openaiapi_model_type_not_yet_available() {
    let log_dir = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let input = dummy_input(input_dir.path(), "dummy.mp3");

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("-m")
        .arg("openaiapi")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "the OpenAI API backend is not yet available in humm — it arrives with cloud \
             support; use a local model type meanwhile",
        ));
}

// 12. `-m huggingface` without `--hfid` -> exit 1, cli.py:147 verbatim.
#[test]
fn huggingface_without_hfid_rejected() {
    let log_dir = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let input = dummy_input(input_dir.path(), "dummy.mp3");

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("-m")
        .arg("huggingface")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "--hfid is required when --model-type is huggingface",
        ));
}

// 13. `-m bogus` -> clap value-enum error, exit 1.
#[test]
fn invalid_model_type_rejected() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("-m")
        .arg("bogus")
        .assert()
        .code(1);
}

// 18. `add --help` lists the full M2 grammar, including short forms.
#[test]
fn help_lists_full_m2_grammar() {
    let log_dir = tempfile::tempdir().unwrap();
    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .arg("add")
        .arg("--help")
        .assert()
        .code(0)
        .stdout(
            predicate::str::contains("-m, --model-type")
                .and(predicate::str::contains("--hfid"))
                .and(predicate::str::contains("-e, --extract-speech"))
                .and(predicate::str::contains("--openai-token"))
                .and(predicate::str::contains("--hide-gui"))
                .and(predicate::str::contains("--json"))
                .and(predicate::str::contains("--quiet")),
        );
}

// --- Hermetic wired cli-int (Task 11 Step 2) --------------------------------

/// A hermetic environment: temp log/data/config dirs and a temp model root
/// seeded with the Buzz stub `ggml-tiny` (+ completeness sentinel + CoreML
/// negative marker) so `-s tiny` resolves with no network. Buzz import is
/// disabled and GPU forced off.
struct Env {
    log: tempfile::TempDir,
    data: tempfile::TempDir,
    config: tempfile::TempDir,
    model_root: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let env = Env {
            log: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            config: tempfile::tempdir().unwrap(),
            model_root: tempfile::tempdir().unwrap(),
        };
        seed_stub_tiny(env.model_root.path());
        env
    }

    fn cmd(&self) -> Command {
        let mut c = humm_cmd();
        c.env("HUMM_LOG_DIR", self.log.path())
            .env("HUMM_DATA_DIR", self.data.path())
            .env("HUMM_CONFIG_DIR", self.config.path())
            .env("HUMM_MODEL_ROOT", self.model_root.path())
            .env("HUMM_BUZZ_SQLITE", self.data.path().join("no-buzz.sqlite"))
            .env("HUMM_FORCE_CPU", "true");
        c
    }

    fn db_path(&self) -> PathBuf {
        self.data.path().join("humm.sqlite")
    }

    fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.db_path()).expect("open store")
    }
}

/// Seeds `model_root` so `humm add -s tiny` resolves the model with no network:
/// the Buzz stub ggml file, its `.humm_complete` sentinel, and a CoreML
/// negative-cache marker (so the CoreML encoder is never probed either).
fn seed_stub_tiny(model_root: &Path) {
    let stub = testdata_dir().join("ggml-tiny.bin");
    assert!(
        stub.is_file(),
        "missing Buzz stub model fixture: {}",
        stub.display()
    );
    let name = ggml_file_name(ModelSize::Tiny);
    fs::copy(&stub, model_root.join(&name)).expect("seed stub ggml-tiny.bin");
    fs::write(model_root.join(format!("{name}{SENTINEL_SUFFIX}")), b"").expect("seed sentinel");
    fs::write(coreml_no_encoder_marker(model_root, ModelSize::Tiny), b"")
        .expect("seed coreml negative marker");
}

fn sample_spec() -> TaskSpec {
    TaskSpec {
        input: Input::Path(PathBuf::from("/tmp/stale-input.wav")),
        options: TranscriptionOptions::default(),
        output: OutputSpec::default(),
    }
}

// A good WAV -> exit 0, exactly one `completed` row whose persisted spec
// round-trips (input path + `txt` format) and which stored no segments (the
// stub transcribes to nothing).
#[test]
fn add_wav_completes_with_persisted_row_and_zero_segments() {
    let env = Env::new();
    let out = tempfile::tempdir().unwrap();
    let wav = testdata_dir().join("whisper-latvian.wav");
    assert!(wav.is_file(), "missing testdata: {}", wav.display());

    env.cmd()
        .arg("add")
        .arg(&wav)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .arg("-d")
        .arg(out.path())
        .assert()
        .code(0);

    let store = env.store();
    let tasks = store.list_tasks().expect("list");
    assert_eq!(tasks.len(), 1, "expected exactly one task row: {tasks:?}");
    assert_eq!(tasks[0].status, Status::Completed);

    let spec = store
        .load_task_spec(&tasks[0].id)
        .expect("load spec")
        .expect("options_json present");
    assert_eq!(spec.input, Input::Path(wav));
    assert_eq!(
        spec.output.formats,
        [OutputFormat::Txt].into_iter().collect()
    );

    let segments = store.get_segments(&tasks[0].id).expect("segments");
    assert!(
        segments.is_empty(),
        "stub model should transcribe to nothing, got {segments:?}"
    );
}

// A missing input alongside a good one -> exit 2 and TWO history rows: one
// `failed` (naming the path) and one `completed` (parity: failures persist).
#[test]
fn add_missing_and_good_persists_both_rows_and_exits_2() {
    let env = Env::new();
    let out = tempfile::tempdir().unwrap();
    let missing = out.path().join("missing.mp3");
    let wav = testdata_dir().join("whisper-latvian.wav");

    env.cmd()
        .arg("add")
        .arg(&missing)
        .arg(&wav)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .arg("-d")
        .arg(out.path())
        .assert()
        .code(2);

    let store = env.store();
    let tasks = store.list_tasks().expect("list");
    assert_eq!(tasks.len(), 2, "expected two task rows: {tasks:?}");
    let failed = tasks
        .iter()
        .find(|t| t.status == Status::Failed)
        .expect("a failed row");
    let completed = tasks
        .iter()
        .find(|t| t.status == Status::Completed)
        .expect("a completed row");
    assert!(
        failed
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("missing.mp3"),
        "failed row error should name the path: {:?}",
        failed.error_message
    );
    let spec = store.load_task_spec(&completed.id).unwrap().unwrap();
    assert_eq!(spec.input, Input::Path(wav));
}

// `--json` -> every stdout line is a JSON event; the good file's sequence is
// queued, started, ..., completed; completed.written files exist; nothing
// non-JSON is on stdout.
#[test]
fn add_json_emits_a_valid_event_stream() {
    let env = Env::new();
    let out = tempfile::tempdir().unwrap();
    let wav = testdata_dir().join("whisper-latvian.wav");

    let output = env
        .cmd()
        .arg("add")
        .arg(&wav)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .arg("--json")
        .arg("-d")
        .arg(out.path())
        .assert()
        .code(0);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let mut event_names = Vec::new();
    let mut completed_written: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON stdout line {line:?}: {e}"));
        let name = value["event"].as_str().expect("event field").to_string();
        if name == "completed" {
            completed_written = value["written"]
                .as_array()
                .expect("written array")
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
        }
        event_names.push(name);
    }

    assert_eq!(event_names.first().map(String::as_str), Some("queued"));
    assert!(event_names.iter().any(|e| e == "started"));
    assert_eq!(event_names.last().map(String::as_str), Some("completed"));
    assert!(!completed_written.is_empty());
    for path in &completed_written {
        assert!(
            Path::new(path).exists(),
            "completed.written path should exist: {path}"
        );
    }
}

// Relaunch recovery: a stale `in_progress` row is flipped to `canceled` (with
// `time_ended`) at the next startup (F-029).
#[test]
fn relaunch_flips_stale_in_progress_row_to_canceled() {
    let env = Env::new();
    let stale_id = {
        let store = env.store();
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_spec(), Source::FileImport)
            .unwrap();
        store.mark_started(&id).unwrap(); // -> in_progress
        id
    };

    let missing = env.data.path().join("nope.mp3");
    env.cmd()
        .arg("add")
        .arg(&missing)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(2);

    let store = env.store();
    let row = store.get_task(&stale_id).unwrap().unwrap();
    assert_eq!(row.status, Status::Canceled);
    assert!(row.time_ended.is_some(), "canceled row must set time_ended");
}

// `-s custom` with no configured URL -> exit 1, naming `models.custom_model_url`
// and the settings path, and WITHOUT the `--help` usage hint (runtime/config
// error, not a usage error).
#[test]
fn custom_size_without_settings_url_fails_with_actionable_message() {
    let env = Env::new();
    let input_dir = tempfile::tempdir().unwrap();
    let input = dummy_input(input_dir.path(), "dummy.mp3");

    env.cmd()
        .arg("add")
        .arg("-s")
        .arg("custom")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("models.custom_model_url")
                .and(predicate::str::contains("settings.toml"))
                .and(predicate::str::contains("Run `humm add --help`").not()),
        );
}

// A single missing input -> exit 2, a persisted `failed` row, and stderr names
// the path without ever printing a model download line (the stub is cached).
#[test]
fn missing_input_persists_failed_row_and_exits_2() {
    let env = Env::new();
    let missing = env.data.path().join("nope.mp3");

    env.cmd()
        .arg("add")
        .arg(&missing)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("nope.mp3").and(predicate::str::contains("Downloading").not()),
        );

    let store = env.store();
    let tasks = store.list_tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].status, Status::Failed);
}

// `--hide-gui` prints an informational note and does not short-circuit the run.
#[test]
fn hide_gui_notes_and_continues() {
    let env = Env::new();
    let missing = env.data.path().join("nope.mp3");

    env.cmd()
        .arg("add")
        .arg("--hide-gui")
        .arg(&missing)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("note: --hide-gui has no effect; humm is headless (ADR-007)")
                .and(predicate::str::contains("nope.mp3")),
        );
}

// `--hfid` with a non-huggingface `-m` is ignored (Buzz parity): same flow as
// without it.
#[test]
fn hfid_ignored_for_non_huggingface_model_type() {
    let env = Env::new();
    let missing = env.data.path().join("nope.mp3");

    env.cmd()
        .arg("add")
        .arg("-m")
        .arg("whisper")
        .arg("--hfid")
        .arg("some/repo")
        .arg(&missing)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("nope.mp3"));
}

// `--openai-token` without `-m openaiapi` is accepted-and-unused (Buzz parity).
#[test]
fn openai_token_accepted_and_unused_without_openaiapi_type() {
    let env = Env::new();
    let missing = env.data.path().join("nope.mp3");

    env.cmd()
        .arg("add")
        .arg("--openai-token")
        .arg("sk-not-real")
        .arg(&missing)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("nope.mp3"));
}

// `--json --quiet`: the failure surfaces as a JSON `failed` event on stdout
// (not a human stderr line), and `--quiet` suppresses progress lines.
#[test]
fn json_and_quiet_flags_wire_to_json_output() {
    let env = Env::new();
    let missing = env.data.path().join("nope.mp3");

    let output = env
        .cmd()
        .arg("add")
        .arg("--json")
        .arg("--quiet")
        .arg(&missing)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(2);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(!stdout.trim().is_empty(), "expected JSON events on stdout");
    let mut saw_failed = false;
    for line in stdout.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("valid JSON line");
        if value["event"] == "failed" {
            saw_failed = true;
            assert!(value["error"].as_str().unwrap().contains("nope.mp3"));
        }
    }
    assert!(saw_failed, "expected a failed event in: {stdout}");
}

// --- Acceptance (`#[ignore]`): real model + real audio + signals ------------

/// Fetches a real tiny model (cached across runs) and seeds `model_root` with
/// it plus its sentinel and a CoreML negative marker (so the `humm` subprocess
/// resolves it fully offline). Network is only touched on the very first fetch.
fn seed_real_tiny(model_root: &Path) {
    use humm_core::cancel::CancelToken;
    use humm_core::models::download::ModelDownloader;

    let real = ModelDownloader::new(humm_core::paths::model_root())
        .ensure_model(ModelSize::Tiny, false, &mut |_| {}, &CancelToken::new())
        .expect("fetch real ggml-tiny.bin (network on first run, cached thereafter)");
    let cache = real.parent().expect("model path has a parent");
    let name = ggml_file_name(ModelSize::Tiny);
    let sentinel = format!("{name}{SENTINEL_SUFFIX}");
    fs::copy(&real, model_root.join(&name)).expect("seed real ggml-tiny.bin");
    fs::copy(cache.join(&sentinel), model_root.join(&sentinel)).expect("seed sentinel");
    fs::write(coreml_no_encoder_marker(model_root, ModelSize::Tiny), b"")
        .expect("seed coreml negative marker");
}

/// Spawns the `humm` binary as a raw child process (so we can signal it),
/// wired to the given hermetic env.
fn spawn_humm(env: &Env, extra_env: &[(&str, &str)], args: &[&std::ffi::OsStr]) -> Child {
    let bin = assert_cmd::cargo::cargo_bin("humm");
    let mut cmd = StdCommand::new(bin);
    cmd.env("HUMM_LOG_DIR", env.log.path())
        .env("HUMM_DATA_DIR", env.data.path())
        .env("HUMM_CONFIG_DIR", env.config.path())
        .env("HUMM_MODEL_ROOT", env.model_root.path())
        .env("HUMM_BUZZ_SQLITE", env.data.path().join("no-buzz.sqlite"))
        .env("HUMM_FORCE_CPU", "true");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn humm")
}

/// Polls the store until a row satisfies `pred` or the timeout elapses.
fn poll_for<F>(env: &Env, timeout: Duration, pred: F) -> Option<humm_store::TaskRow>
where
    F: Fn(&humm_store::TaskRow) -> bool,
{
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(store) = SqliteStore::open(&env.db_path())
            && let Ok(tasks) = store.list_tasks()
            && let Some(row) = tasks.into_iter().find(&pred)
        {
            return Some(row);
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if start.elapsed() > timeout {
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

// kill -9 recovery (the milestone headline): a transcription killed mid-run
// leaves an `in_progress` row; the next launch flips it to `canceled`, and its
// full original `TaskSpec` is still retrievable (retry with full options).
#[test]
#[ignore = "requires a real tiny model + audio; run sandbox-lifted"]
fn acceptance_kill9_leaves_recoverable_canceled_row() {
    let env = Env::new();
    seed_real_tiny(env.model_root.path());
    let audio = testdata_dir().join("audio-long.mp3");
    assert!(audio.is_file(), "missing testdata: {}", audio.display());

    let mut child = spawn_humm(
        &env,
        &[],
        &[
            "add".as_ref(),
            audio.as_os_str(),
            "--txt".as_ref(),
            "-s".as_ref(),
            "tiny".as_ref(),
        ],
    );

    let in_progress = poll_for(&env, Duration::from_secs(60), |row| {
        row.status == Status::InProgress
    })
    .expect("a row should reach in_progress");

    // Hard-kill mid-transcription (no chance to mark terminal).
    child.kill().expect("SIGKILL");
    let _ = child.wait();

    // Relaunch (trivial input) runs cancel_stale_tasks at startup.
    let missing = env.data.path().join("relaunch-nope.mp3");
    env.cmd()
        .arg("add")
        .arg(&missing)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(2);

    let store = env.store();
    let row = store.get_task(&in_progress.id).unwrap().unwrap();
    assert_eq!(row.status, Status::Canceled);
    assert!(row.time_ended.is_some());

    let spec = store
        .load_task_spec(&in_progress.id)
        .unwrap()
        .expect("full TaskSpec is retained for retry");
    assert_eq!(spec.input, Input::Path(audio));
    assert_eq!(
        spec.output.formats,
        [OutputFormat::Txt].into_iter().collect()
    );
}

// Ctrl-C: a running transcription is canceled promptly on SIGINT; the process
// exits 130 and the row is `canceled`.
#[test]
#[ignore = "requires a real tiny model + audio; run sandbox-lifted"]
fn acceptance_ctrl_c_cancels_and_exits_130() {
    let env = Env::new();
    seed_real_tiny(env.model_root.path());
    let audio = testdata_dir().join("audio-long.mp3");

    let mut child = spawn_humm(
        &env,
        &[],
        &[
            "add".as_ref(),
            audio.as_os_str(),
            "--txt".as_ref(),
            "-s".as_ref(),
            "tiny".as_ref(),
        ],
    );

    let in_progress = poll_for(&env, Duration::from_secs(60), |row| {
        row.status == Status::InProgress
    })
    .expect("a row should reach in_progress");

    // SIGINT via /bin/kill (std has no portable SIGINT).
    StdCommand::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .expect("send SIGINT");

    let status = wait_for_exit(&mut child, Duration::from_secs(15)).expect("process should exit");
    assert_eq!(status.code(), Some(130), "SIGINT should yield exit 130");

    let store = env.store();
    let row = store.get_task(&in_progress.id).unwrap().unwrap();
    assert_eq!(row.status, Status::Canceled);
}

// URL import e2e (needs yt-dlp): a local HTTP server serves a testdata file;
// `humm add <url>` downloads it, transcribes, and records a url_import row with
// the URL and a resolved local file under the media dir.
#[test]
#[ignore = "requires yt-dlp + a real tiny model; run sandbox-lifted"]
fn acceptance_url_import_downloads_and_transcribes() {
    let env = Env::new();
    seed_real_tiny(env.model_root.path());
    let source = testdata_dir().join("whisper-french.mp3");
    let body = fs::read(&source).expect("read testdata");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local http server");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || serve_file(listener, body));

    let url = format!("http://127.0.0.1:{port}/whisper-french.mp3");
    env.cmd()
        .arg("add")
        .arg(&url)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .assert()
        .code(0);

    let store = env.store();
    let tasks = store.list_tasks().expect("list");
    assert_eq!(tasks.len(), 1, "expected one url task: {tasks:?}");
    let row = &tasks[0];
    assert_eq!(row.status, Status::Completed);
    assert_eq!(row.source, Some(Source::UrlImport));
    assert_eq!(row.url.as_deref(), Some(url.as_str()));

    let media_dir = env.data.path().join("media");
    let file = row.file.as_ref().expect("resolved local file");
    assert!(
        file.starts_with(&media_dir),
        "downloaded file should live under the media dir: {}",
        file.display()
    );
    // An export was written next to the downloaded media.
    let export_dir = file.parent().unwrap();
    let txt: Vec<_> = fs::read_dir(export_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"))
        .collect();
    assert_eq!(txt.len(), 1, "expected one txt export in {export_dir:?}");
}

/// Minimal HTTP/1.1 file server for the URL test: answers each connection with
/// the file, honoring a single `Range: bytes=start-[end]` request (yt-dlp may
/// probe/resume) and `HEAD` (headers only).
fn serve_file(listener: TcpListener, body: Vec<u8>) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));

        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue;
        }
        let is_head = request_line.starts_with("HEAD");
        let mut range: Option<(usize, usize)> = None;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                break;
            }
            if header == "\r\n" || header == "\n" {
                break;
            }
            if let Some(spec) = header
                .to_ascii_lowercase()
                .strip_prefix("range:")
                .and_then(|v| v.trim().strip_prefix("bytes="))
                .map(str::to_string)
            {
                let mut parts = spec.trim().splitn(2, '-');
                let start = parts
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                let end = parts
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(body.len().saturating_sub(1))
                    .min(body.len().saturating_sub(1));
                range = Some((start, end));
            }
        }

        let (status, slice): (&str, &[u8]) = match range {
            Some((start, end)) if start <= end && start < body.len() => {
                ("206 Partial Content", &body[start..=end])
            }
            _ => ("200 OK", &body),
        };
        let mut headers = format!(
            "HTTP/1.1 {status}\r\nContent-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\n\
             Content-Length: {}\r\nConnection: close\r\n",
            slice.len()
        );
        if let Some((start, end)) = range {
            headers.push_str(&format!(
                "Content-Range: bytes {start}-{end}/{}\r\n",
                body.len()
            ));
        }
        headers.push_str("\r\n");
        if stream.write_all(headers.as_bytes()).is_err() {
            continue;
        }
        if !is_head {
            let _ = stream.write_all(slice);
        }
        let _ = stream.flush();
    }
}

/// End-to-end: `humm add` transcribes a real French clip with a real tiny model
/// and writes SRT+TXT exports. Ignored by default (network needed on first run
/// to cache a real tiny model; ffmpeg required to decode).
#[test]
#[ignore = "requires ffmpeg + Buzz testdata + a real tiny model (network on first run)"]
fn add_transcribes_and_exports() {
    let env = Env::new();
    seed_real_tiny(env.model_root.path());

    let input = testdata_dir().join("whisper-french.mp3");
    assert!(
        input.is_file(),
        "missing Buzz testdata fixture: {}",
        input.display()
    );

    let out_dir = tempfile::tempdir().unwrap();
    let output = env
        .cmd()
        .arg("add")
        .arg(&input)
        .arg("--srt")
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .arg("-l")
        .arg("fr")
        .arg("-d")
        .arg(out_dir.path())
        .assert()
        .success();

    let entries: Vec<PathBuf> = fs::read_dir(out_dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();

    let srt_files: Vec<&PathBuf> = entries
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("srt"))
        .collect();
    let txt_files: Vec<&PathBuf> = entries
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"))
        .collect();
    assert_eq!(srt_files.len(), 1, "expected one .srt, found {entries:?}");
    assert_eq!(txt_files.len(), 1, "expected one .txt, found {entries:?}");

    let expected_stem_prefix = "whisper-french (transcribed on ";
    let srt_stem = srt_files[0].file_stem().unwrap().to_string_lossy();
    assert!(
        srt_stem.starts_with(expected_stem_prefix),
        "unexpected srt name: {srt_stem}"
    );

    let srt_contents = fs::read_to_string(srt_files[0]).unwrap();
    assert!(
        srt_contents.starts_with("1\n00:"),
        "srt did not start as expected: {srt_contents:?}"
    );
    let txt_contents = fs::read_to_string(txt_files[0]).unwrap();
    assert!(!txt_contents.trim().is_empty(), "txt export was empty");

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let printed: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        printed.len(),
        2,
        "expected two stdout lines (written paths), got: {stdout:?}"
    );
    assert!(printed.iter().any(|line| line.ends_with(".srt")));
    assert!(printed.iter().any(|line| line.ends_with(".txt")));
}
