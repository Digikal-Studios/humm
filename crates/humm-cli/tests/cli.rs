//! Integration tests for the `humm` CLI (`add` subcommand), driven through
//! `assert_cmd` against the real built binary.
//!
//! Tests 1-9 (brief `task-8-brief.md` Step 1) run in the normal suite: no
//! model, no network, no ffmpeg needed. Every invocation sets `HUMM_LOG_DIR`
//! to a tempdir so runs never touch the real platform log location.
//!
//! `add_transcribes_and_exports` is the amended Step 3 e2e test: `#[ignore]`
//! (network on first run to cache a real tiny model; ffmpeg + Buzz testdata
//! required). Run explicitly with `cargo test -p humm-cli -- --ignored`.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

fn humm_cmd() -> Command {
    Command::cargo_bin("humm").expect("humm binary should build")
}

fn dummy_input(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"not real media, just needs to exist").expect("write dummy input");
    path
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
// (cli.py:226-227). A dummy (existing) input file isolates this from the
// "no input files" check.
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
        .stderr(predicate::str::contains("Invalid language option"));
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

// 6. `-s custom` is a syntactically valid ModelSize, but model resolution
// hits ModelError::Unsupported ("custom model URLs arrive with M2") before
// any network call (repo_for fails first) -> exit 1, message mentions M2.
#[test]
fn custom_size_points_to_m2() {
    let log_dir = tempfile::tempdir().unwrap();
    let model_root = tempfile::tempdir().unwrap();
    let input_dir = tempfile::tempdir().unwrap();
    let input = dummy_input(input_dir.path(), "dummy.mp3");

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("HUMM_MODEL_ROOT", model_root.path())
        .arg("add")
        .arg("-s")
        .arg("custom")
        .arg(&input)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("M2"));
}

// 7. All inputs missing -> exit 2, names the missing path, and never
// attempts a model download (empty HUMM_MODEL_ROOT + missing input -> fast
// exit, proven by asserting "Downloading" never appears in stderr).
#[test]
fn missing_inputs_exit_2_without_model() {
    let log_dir = tempfile::tempdir().unwrap();
    let model_root = tempfile::tempdir().unwrap();

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("HUMM_MODEL_ROOT", model_root.path())
        .arg("add")
        .arg("/nope.mp3")
        .arg("--txt")
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("/nope.mp3")
                .and(predicate::str::contains("Downloading").not()),
        );
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

// --- M2 Task 10 grammar matrix (F-002): `-m/--model-type`, `--hfid`,
// `-e/--extract-speech`, `--openai-token`, `--hide-gui`, `--json`, `--quiet`.
// All validation-only — no model/network needed (task-10-brief.md Step 1).

// 10. `-e/--extract-speech` -> exit 1, exact not-yet-available message
// (speech extraction arrives in a later release, F-027 is M8).
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

// 11. `-m openaiapi` -> exit 1, exact not-yet-available message (honest gap;
// cloud support arrives later).
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

// 13. `-m bogus` -> clap value-enum error, exit 1 (existing
// exit_code_for_parse_error mapping, same as invalid_task_rejected /
// invalid_size_rejected above).
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

// 14. `--hide-gui` prints an informational note to stderr but does not block
// the run: paired here with a missing input so the rest of the flow (exit 2,
// no model download) proves the note didn't short-circuit anything.
#[test]
fn hide_gui_notes_and_continues() {
    let log_dir = tempfile::tempdir().unwrap();
    let model_root = tempfile::tempdir().unwrap();

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("HUMM_MODEL_ROOT", model_root.path())
        .arg("add")
        .arg("--hide-gui")
        .arg("/nope.mp3")
        .arg("--txt")
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("note: --hide-gui has no effect; humm is headless (ADR-007)")
                .and(predicate::str::contains("/nope.mp3"))
                .and(predicate::str::contains("Downloading").not()),
        );
}

// 15. `--hfid` with a non-huggingface `-m` is ignored exactly as Buzz
// ignores it — no validation error, same missing-input flow as without it.
#[test]
fn hfid_ignored_for_non_huggingface_model_type() {
    let log_dir = tempfile::tempdir().unwrap();
    let model_root = tempfile::tempdir().unwrap();

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("HUMM_MODEL_ROOT", model_root.path())
        .arg("add")
        .arg("-m")
        .arg("whisper")
        .arg("--hfid")
        .arg("some/repo")
        .arg("/nope.mp3")
        .arg("--txt")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("/nope.mp3"));
}

// 16. `--openai-token` without `-m openaiapi` is accepted-and-unused (Buzz
// behavior) — no validation error, same missing-input flow as without it.
#[test]
fn openai_token_accepted_and_unused_without_openaiapi_type() {
    let log_dir = tempfile::tempdir().unwrap();
    let model_root = tempfile::tempdir().unwrap();

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("HUMM_MODEL_ROOT", model_root.path())
        .arg("add")
        .arg("--openai-token")
        .arg("sk-not-real")
        .arg("/nope.mp3")
        .arg("--txt")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("/nope.mp3"));
}

// 17. `--json` and `--quiet` parse without altering validation/exit-code
// behavior (Task 11 wires their actual effect).
#[test]
fn json_and_quiet_flags_parse() {
    let log_dir = tempfile::tempdir().unwrap();
    let model_root = tempfile::tempdir().unwrap();

    humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("HUMM_MODEL_ROOT", model_root.path())
        .arg("add")
        .arg("--json")
        .arg("--quiet")
        .arg("/nope.mp3")
        .arg("--txt")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("/nope.mp3"));
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

/// End-to-end: `humm add` transcribes a real French clip with a real tiny
/// model and writes SRT+TXT exports. Ignored by default (network needed on
/// first run to cache a real tiny model; ffmpeg required to decode). Run
/// explicitly with `cargo test -p humm-cli -- --ignored`.
///
/// Global Constraint 4 / Task 6 finding: Buzz's `testdata/ggml-tiny.bin` is a
/// 575 KB stub that loads but transcribes to zero segments, so this test
/// fetches a genuine tiny model via `ModelDownloader::ensure_model` into the
/// default `model_root()` (cached across runs — a real tiny model may
/// already be cached there from Task 6's own contract runs), then seeds a
/// hermetic temp `HUMM_MODEL_ROOT` by copying the cached `ggml-tiny.bin` +
/// its `.humm_complete` sentinel. The `humm` subprocess itself never touches
/// the user's real model cache or the network.
#[test]
#[ignore = "requires ffmpeg + Buzz testdata + a real tiny model (network on first run)"]
fn add_transcribes_and_exports() {
    use humm_core::cancel::CancelToken;
    use humm_core::domain::ModelSize;
    use humm_core::models::download::ModelDownloader;
    use humm_core::models::registry::{SENTINEL_SUFFIX, ggml_file_name};

    let real_model_path = ModelDownloader::new(humm_core::paths::model_root())
        .ensure_model(ModelSize::Tiny, false, &mut |_| {}, &CancelToken::new())
        .expect("fetch real ggml-tiny.bin (network needed on first run, then cached)");
    let cache_root = real_model_path
        .parent()
        .expect("model path has a parent directory")
        .to_path_buf();

    let file_name = ggml_file_name(ModelSize::Tiny);
    let sentinel_name = format!("{file_name}{SENTINEL_SUFFIX}");

    let seeded_model_root = tempfile::tempdir().unwrap();
    fs::copy(&real_model_path, seeded_model_root.path().join(&file_name))
        .expect("seed hermetic model root with the cached tiny model");
    fs::copy(
        cache_root.join(&sentinel_name),
        seeded_model_root.path().join(&sentinel_name),
    )
    .expect("seed hermetic model root with the cached tiny model's sentinel");

    let testdata_dir = std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"));
    let input = testdata_dir.join("whisper-french.mp3");
    assert!(
        input.is_file(),
        "missing Buzz testdata fixture: {}",
        input.display()
    );

    let out_dir = tempfile::tempdir().unwrap();
    let log_dir = tempfile::tempdir().unwrap();

    let assert = humm_cmd()
        .env("HUMM_LOG_DIR", log_dir.path())
        .env("HUMM_MODEL_ROOT", seeded_model_root.path())
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
    assert_eq!(
        srt_files.len(),
        1,
        "expected exactly one .srt file, found {entries:?}"
    );
    assert_eq!(
        txt_files.len(),
        1,
        "expected exactly one .txt file, found {entries:?}"
    );

    let expected_stem_prefix = "whisper-french (transcribed on ";
    let srt_stem = srt_files[0].file_stem().unwrap().to_string_lossy();
    assert!(
        srt_stem.starts_with(expected_stem_prefix),
        "unexpected srt name: {srt_stem}"
    );
    let txt_stem = txt_files[0].file_stem().unwrap().to_string_lossy();
    assert!(
        txt_stem.starts_with(expected_stem_prefix),
        "unexpected txt name: {txt_stem}"
    );

    let srt_contents = fs::read_to_string(srt_files[0]).unwrap();
    assert!(
        srt_contents.starts_with("1\n00:"),
        "srt did not start as expected: {srt_contents:?}"
    );

    let txt_contents = fs::read_to_string(txt_files[0]).unwrap();
    assert!(!txt_contents.trim().is_empty(), "txt export was empty");

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let printed: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        printed.len(),
        2,
        "expected exactly two stdout lines (written paths), got: {stdout:?}"
    );
    assert!(printed.iter().any(|line| line.ends_with(".srt")));
    assert!(printed.iter().any(|line| line.ends_with(".txt")));
}
