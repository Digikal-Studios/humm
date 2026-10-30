//! Integration tests for the `humm` CLI (`add` subcommand), driven through
//! `assert_cmd` against the real built binary.
//!
//! Three tiers:
//! - **Grammar/validation** (tests 1–18, except 11 — moved to the wired tier
//!   now that `-m openaiapi` is real, Task 11/F-002/F-041): no store, model,
//!   or network — a usage/parse error is reached before any runtime work.
//! - **Wired cli-int** (`add_*` below, plus the `openaiapi_*` tests): a
//!   hermetic temp `HUMM_DATA_DIR`/`HUMM_CONFIG_DIR`/`HUMM_MODEL_ROOT` seeded
//!   with the Buzz stub `ggml-tiny` (loads under CPU, transcribes to zero
//!   segments — Global Constraint 4), so the full store + queue path runs
//!   network-free. Buzz import is disabled (`HUMM_BUZZ_SQLITE` → a
//!   nonexistent path), GPU is forced off (`HUMM_FORCE_CPU`), and VAD is
//!   disabled (`HUMM_DISABLE_VAD`) so they run under the sandbox with no
//!   network at all. `openaiapi_with_token_transcribes_against_mock` is the
//!   one exception: it binds a local TCP mock server, so — like humm-core's
//!   own `openai::engine`/`models::test_server` mock-server tests — it fails
//!   under the sandbox (`Operation not permitted` on `bind`) and is meant to
//!   be run with it lifted.
//! - **Acceptance** (`acceptance_*`, all `#[ignore]`): real tiny model + real
//!   audio, process signals, and a local HTTP server. Run sandbox-lifted with
//!   `cargo test -p humm-cli -- --ignored`.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
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

// 10. `-e/--extract-speech` is live now (Task 5, F-027/F-002): with `PATH`
// scrubbed of any `demucs` (and `HUMM_DEMUCS` unset), validation fails fast
// with an actionable install hint naming demucs — replaces the retired
// "not yet available" pin.
#[test]
fn e_flag_without_demucs_fails_actionably() {
    let log_dir = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let input = dummy_input(input_dir.path(), "dummy.mp3");
    // A fresh empty directory as `PATH`: guaranteed to contain no `demucs`
    // binary regardless of what's actually installed on the host running
    // this test suite.
    let empty_path_dir = tempfile::tempdir().unwrap();

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("PATH", empty_path_dir.path())
        .env_remove("HUMM_DEMUCS")
        .arg("add")
        .arg("-e")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("demucs not found")
                .and(predicate::str::contains("pipx install demucs")),
        );
}

/// Writes a fake `demucs` executable into `dir` that emulates the
/// spike-confirmed contract layout (`validation/m8-spikes/README.md`,
/// "Spike C"): it reads the `-o <scratch>` argument and copies whatever the
/// real trailing input-file argument is to `<scratch>/htdemucs/vocals.mp3`
/// — a genuinely decodable copy of the real input (not fixture bytes),
/// since this stub's output is fed straight into the real `ffmpeg` decode
/// step by the CLI's own pipeline, exactly like a real `vocals.mp3` would
/// be.
#[cfg(unix)]
fn write_stub_demucs(dir: &Path) {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    // Write via temp + fsync + rename so spawn cannot race with a truncated
    // inode still open for write (Linux ETXTBSY on some ARM CI filesystems).
    let path = dir.join("demucs");
    let tmp = dir.join(format!(".demucs.tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp).expect("create stub demucs tmp");
        file.write_all(
            b"#!/bin/sh\n\
              outdir=\"\"\n\
              input=\"\"\n\
              while [ $# -gt 0 ]; do\n\
              case \"$1\" in\n\
              -o) outdir=\"$2\"; shift 2 ;;\n\
              *) input=\"$1\"; shift ;;\n\
              esac\n\
              done\n\
              mkdir -p \"$outdir/htdemucs\"\n\
              cp \"$input\" \"$outdir/htdemucs/vocals.mp3\"\n",
        )
        .expect("write stub demucs script");
        file.sync_all().expect("fsync stub demucs");
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)).expect("chmod stub demucs");
    fs::rename(&tmp, &path).expect("rename stub demucs into place");
    if let Ok(dir_file) = fs::File::open(dir) {
        let _ = dir_file.sync_all();
    }
}

// 10b. `-e/--extract-speech` with a stub `demucs` discovered via `PATH`
// (prepended ahead of the real system `PATH` so the real `ffmpeg` the
// pipeline needs afterward is still found): the task actually runs through
// the isolator and completes, and the `_speech.mp3` derivative — created
// beside the (copied, so nothing is written next to the shared Buzz
// testdata fixture) input — is cleaned up afterward (`TempAudioGuard`'s
// all-terminal-paths cleanup, design spec §3 item 4).
#[test]
#[cfg(unix)]
fn e_flag_accepted_with_stub_demucs() {
    let env = Env::new();
    let out = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let wav = input_dir.path().join("clip.wav");
    fs::copy(testdata_dir().join("whisper-latvian.wav"), &wav).expect("copy fixture");

    let demucs_dir = tempfile::tempdir().unwrap();
    write_stub_demucs(demucs_dir.path());
    let path_var = format!(
        "{}:{}",
        demucs_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = env
        .cmd()
        .env("PATH", path_var)
        .arg("add")
        .arg(&wav)
        .arg("--txt")
        .arg("-s")
        .arg("tiny")
        .arg("-e")
        .arg("--json")
        .arg("-d")
        .arg(out.path())
        .assert()
        .code(0);

    // Review fix (Important 3): every assertion below (exit 0, one
    // completed row, `_speech.mp3` absent afterward) would pass identically
    // if the isolator were never wired at all — `-e` skips isolation
    // silently whenever `RunnerDeps.isolator` is `None`, and a task that
    // never isolates also never leaves a `_speech.mp3` to clean up. The
    // `--json` stream's `phase`/`isolating_speech` event is the one
    // observable that actually proves the isolator ran (design spec §3,
    // `TaskEventKind::PhaseChanged`) — `json_phase_matches_schema` (`run.rs`
    // unit tests) pins its exact shape.
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let saw_isolating_phase = stdout.lines().any(|line| {
        let value: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        value["event"] == "phase" && value["phase"] == "isolating_speech"
    });
    assert!(
        saw_isolating_phase,
        "expected a phase=isolating_speech event proving the isolator actually ran: {stdout}"
    );

    let store = env.store();
    let tasks = store.list_tasks().expect("list");
    assert_eq!(tasks.len(), 1, "expected exactly one task row: {tasks:?}");
    assert_eq!(tasks[0].status, Status::Completed);

    let speech_path = input_dir.path().join("clip_speech.mp3");
    assert!(
        !speech_path.exists(),
        "expected the isolated _speech.mp3 to be cleaned up after the run: {}",
        speech_path.display()
    );
}

// 11 moved to the wired tier below (`openaiapi_without_token_or_keyring_fails_actionably`
// et al.) — `-m openaiapi` is real now (Task 11, F-002/F-041 completion), so
// exercising it needs the hermetic `Env` (settings/store), not the bare
// `humm_cmd()` this grammar section uses.

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
/// disabled, GPU forced off, and VAD disabled (`HUMM_DISABLE_VAD`) so no test
/// reaches the network for the Silero model either (Constraint 8: no network
/// in non-ignored tests) — the `#[ignore]` acceptance tests that want real VAD
/// override it back with `.env("HUMM_DISABLE_VAD", "")`.
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
            .env("HUMM_FORCE_CPU", "true")
            .env("HUMM_DISABLE_VAD", "1");
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
/// negative-cache marker (so the downloader does not fetch an encoder).
/// `HUMM_FORCE_CPU` is what keeps whisper from probing CoreML at state init.
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

// M7 Task 11 (F-100): `settings.file_transcriber.default_export_template`
// (Preferences → General → "Default export file name") is wired into
// `run.rs`'s own `OutputSpec` construction — a custom template in
// `settings.toml` names the exported file, not the hardcoded
// `DEFAULT_EXPORT_TEMPLATE`. Hermetic: the stub-tiny model + a local wav
// fixture, no network (same shape as `add_wav_completes_with_persisted_row_
// and_zero_segments` above).
#[test]
fn default_export_template_setting_names_output_file() {
    let env = Env::new();
    let wav = testdata_dir().join("whisper-latvian.wav");
    assert!(wav.is_file(), "missing testdata: {}", wav.display());
    let out = tempfile::tempdir().unwrap();

    fs::write(
        env.config.path().join("settings.toml"),
        "[file_transcriber]\ndefault_export_template = \"{{ input_file_name }}-custom-name\"\n",
    )
    .expect("write settings.toml");

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

    let entries: Vec<PathBuf> = fs::read_dir(out.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one export: {entries:?}");
    let file_name = entries[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(file_name, "whisper-latvian-custom-name.txt");
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

    // `HUMM_DISABLE_VAD` (set by `Env::cmd()`) keeps this hermetic run free of
    // a VAD-fetch `model_download_progress` event, but the filter stays
    // defensive: any such event would legitimately precede the task stream
    // (both are JSON — the "nothing non-JSON on stdout" check above already
    // covered them) rather than break this assertion. Assert the per-task
    // subsequence.
    let task_events: Vec<&str> = event_names
        .iter()
        .map(String::as_str)
        .filter(|e| *e != "model_download_progress")
        .collect();
    assert_eq!(task_events.first().copied(), Some("queued"));
    assert!(task_events.contains(&"started"));
    assert_eq!(task_events.last().copied(), Some("completed"));
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

// A single missing input -> exit 2 with a persisted `failed` row whose human
// error line on stderr names the path. (The ggml model resolves from the cached
// stub; a first-run VAD fetch may still occur, so this does not assert the
// absence of any download line.)
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
        .stderr(predicate::str::contains("nope.mp3"));

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

// --- OpenAI API backend (Task 11, F-002/F-041 completion) -------------------

// 11 (repurposed): `-m openaiapi` with no `--openai-token` and the keyring
// lookup short-circuited via `HUMM_DISABLE_KEYRING` (this test never touches
// the real OS keychain) fails actionably before any model/VAD work with
// Buzz's exact message (`cli.py:235`), exit 1, and — a config error, not a
// usage error — no `--help` hint. Network-free.
#[test]
fn openaiapi_without_token_or_keyring_fails_actionably() {
    let env = Env::new();
    let input = dummy_input(env.data.path(), "dummy.mp3");

    env.cmd()
        .env("HUMM_DISABLE_KEYRING", "1")
        .arg("add")
        .arg("-m")
        .arg("openaiapi")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("No OpenAI access token found")
                .and(predicate::str::contains("Run `humm add --help`").not()),
        );
}

// `-m openaiapi --openai-token sk-test` against a local mock server (its URL
// lands in a `settings.toml` written into the hermetic `HUMM_CONFIG_DIR`)
// transcribes end to end: exit 0, a `completed` event, the TXT export
// written with the mocked segment text, and — the API backend has no local
// model — zero `model_download_progress` events anywhere in the stream.
//
// TCP-BINDING: this test binds a local `TcpListener` and will fail under the
// sandbox (`Operation not permitted` on `bind`), joining humm-core's
// existing sandbox-blocked TCP test set (documented growth — see this
// task's report — not a regression; run sandbox-lifted to exercise it).
#[test]
fn openaiapi_with_token_transcribes_against_mock() {
    let env = Env::new();
    let wav = testdata_dir().join("whisper-latvian.wav");
    assert!(wav.is_file(), "missing testdata: {}", wav.display());
    let out = tempfile::tempdir().unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local mock server");
    let port = listener.local_addr().unwrap().port();
    let response_body =
        br#"{"segments":[{"start":0.0,"end":1.0,"text":"hello from mock"}]}"#.to_vec();
    thread::spawn(move || serve_openai_mock(listener, response_body));

    fs::write(
        env.config.path().join("settings.toml"),
        format!("[openai]\nbase_url = \"http://127.0.0.1:{port}/v1\"\n"),
    )
    .expect("write settings.toml");

    let output = env
        .cmd()
        .arg("add")
        .arg(&wav)
        .arg("-m")
        .arg("openaiapi")
        .arg("--openai-token")
        .arg("sk-test")
        .arg("--txt")
        .arg("--json")
        .arg("-d")
        .arg(out.path())
        .assert()
        .code(0);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(!stdout.trim().is_empty(), "expected JSON events on stdout");
    let mut saw_completed = false;
    for line in stdout.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("valid JSON line");
        assert_ne!(
            value["event"], "model_download_progress",
            "the API backend must skip model resolution entirely: {stdout}"
        );
        if value["event"] == "completed" {
            saw_completed = true;
        }
    }
    assert!(saw_completed, "expected a completed event in: {stdout}");

    let txt: Vec<_> = fs::read_dir(out.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"))
        .collect();
    assert_eq!(txt.len(), 1, "expected one txt export in {:?}", out.path());
    let txt_contents = fs::read_to_string(&txt[0]).unwrap();
    assert!(
        txt_contents.contains("hello from mock"),
        "txt export should contain the mocked segment text: {txt_contents:?}"
    );
}

// The M6 acceptance's "one recorded real-API run" (F-041): end to end through
// the real binary against a REAL OpenAI-compatible server — no mock, no canned
// response; the transcript text comes from genuine model inference. Gated on
// `HUMM_OPENAI_TEST_BASE` (e.g. `http://127.0.0.1:18080/v1`), with
// `HUMM_OPENAI_TEST_KEY` optional (local servers ignore auth; defaults to a
// placeholder). The recorded acceptance run used the stock whisper.cpp
// `whisper-server` behind the path-rewriting shim in
// `validation/m6-real-api/oai_shim.py` (see its module doc for why the shim
// exists and what it does NOT touch) — the same server class Buzz's
// `is_whisper_cpp` response branch exists for.
#[test]
#[ignore = "requires a real OpenAI-compatible server (set HUMM_OPENAI_TEST_BASE)"]
fn real_api_transcription_end_to_end() {
    let base = match std::env::var("HUMM_OPENAI_TEST_BASE") {
        Ok(v) if !v.is_empty() => v,
        _ => panic!(
            "HUMM_OPENAI_TEST_BASE is not set — point it at a real \
             OpenAI-compatible server (see validation/m6-real-api/oai_shim.py)"
        ),
    };
    let key =
        std::env::var("HUMM_OPENAI_TEST_KEY").unwrap_or_else(|_| "sk-local-unused".to_string());

    let env = Env::new();
    let audio = testdata_dir().join("whisper-french.mp3");
    assert!(audio.is_file(), "missing testdata: {}", audio.display());
    let out = tempfile::tempdir().unwrap();

    fs::write(
        env.config.path().join("settings.toml"),
        format!("[openai]\nbase_url = \"{base}\"\n"),
    )
    .expect("write settings.toml");

    let output = env
        .cmd()
        .arg("add")
        .arg(&audio)
        .arg("-m")
        .arg("openaiapi")
        .arg("--openai-token")
        .arg(&key)
        .arg("--txt")
        .arg("--json")
        .arg("-d")
        .arg(out.path())
        .assert()
        .code(0);

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let mut saw_completed = false;
    for line in stdout.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("valid JSON line");
        assert_ne!(
            value["event"], "model_download_progress",
            "the API backend must skip model resolution entirely: {stdout}"
        );
        if value["event"] == "completed" {
            saw_completed = true;
        }
    }
    assert!(saw_completed, "expected a completed event in: {stdout}");

    let txt: Vec<_> = fs::read_dir(out.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"))
        .collect();
    assert_eq!(txt.len(), 1, "expected one txt export in {:?}", out.path());
    let txt_contents = fs::read_to_string(&txt[0]).unwrap();
    assert!(
        !txt_contents.trim().is_empty(),
        "real-API transcription produced an empty transcript"
    );
}

/// Single-connection mock for the OpenAI-compatible `/audio/transcriptions`
/// endpoint: accepts exactly one connection, drains the request body per its
/// `Content-Length` header (a multipart mp3 upload — never parsed, just
/// consumed so the client's write completes), and replies with a canned
/// `200 application/json` body. The CLI's one small-file (`< MAX_CHUNK_SIZE`)
/// request lands as this listener's single accepted connection.
fn serve_openai_mock(listener: TcpListener, response_body: Vec<u8>) {
    let Ok((mut stream, _)) = listener.accept() else {
        return;
    };
    {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some(value) = line
                .to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().to_string())
            {
                content_length = value.parse().unwrap_or(0);
            }
        }
        if content_length > 0 {
            let mut body = vec![0u8; content_length];
            let _ = reader.read_exact(&mut body);
        }
    }
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        response_body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&response_body);
    let _ = stream.flush();
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
        // Real VAD for this acceptance test (Env::cmd()'s default disables
        // it for hermetic tests).
        .env("HUMM_DISABLE_VAD", "")
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
        // Real VAD for this acceptance test (Env::cmd()'s default disables
        // it for hermetic tests).
        .env("HUMM_DISABLE_VAD", "")
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
