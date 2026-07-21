//! J-1 acceptance kittest (Task 13, `docs/design/UX_SPEC.md` §J-1): import a
//! real file through the real options sheet, Run, and poll until the real
//! queue — driven by real [`services::init`] (real ffmpeg/ffprobe decode +
//! probe, real whisper.cpp via a real cached tiny model, real sqlite store)
//! — reaches `Completed`, with real SRT/TXT exports on disk.
//!
//! Every other kittest file in this crate builds [`Services`] via
//! [`services::init_with`] + [`TestDeps`], injecting hermetic fakes and never
//! touching `std::env`. This file is the one exception: it exercises the
//! *real* product path end to end, which means it must call
//! [`services::init`] itself, and `init` reads `HUMM_DATA_DIR`/
//! `HUMM_CONFIG_DIR`/`HUMM_MODEL_ROOT`/`HUMM_BUZZ_SQLITE`/
//! `HUMM_DISABLE_UPDATE_CHECK`/`HUMM_DISABLE_TELEMETRY`/`HUMM_DISABLE_VAD`/
//! `HUMM_FORCE_CPU` directly from the real process environment
//! (`humm_core::paths`, `humm_core::env`).
//!
//! # Process-global env — this file must be the ONLY test in this binary
//!
//! `std::env::set_var` mutates process-global state; Cargo already compiles
//! and runs every file directly under `tests/` as its own separate test
//! binary/process, so that binary boundary is the *entire* isolation this
//! test relies on. This file therefore carries exactly one `#[test]` and
//! nothing else — do not add a second test here, and do not move this test
//! into a file that has (or later gains) others.
//!
//! # Running
//!
//! `#[ignore]`: needs ffmpeg on `PATH`, real Buzz testdata, and a real
//! whisper tiny model (network only if neither `$HUMM_WHISPER_TEST_MODEL` nor
//! an already-cached real tiny model is available — see
//! [`ensure_tiny_model`]). GPU/Metal initialization segfaults under this
//! repo's sandboxed cargo, so always run sandbox-lifted:
//!
//! ```text
//! CARGO_HOME="$PWD/.cargo-home" cargo test -p humm-gui --test acceptance -- --ignored --nocapture
//! ```
//!
//! # Model / media / backend
//!
//! Real whisper tiny model: `$HUMM_WHISPER_TEST_MODEL` if set, else the real
//! (network-on-first-run, cached thereafter) model via `ModelDownloader` —
//! the exact same mechanism `crates/humm-whisper/tests/contract.rs`'s own
//! `ensure_tiny_model` uses. Buzz's own `testdata/ggml-tiny.bin` is a
//! truncated ~575 KB stub that loads but transcribes to *zero* segments
//! (correct header, no weights) — deliberately never used here, since a
//! vacuous "completed with an empty transcript" pass would satisfy every
//! assertion below without proving anything. Real Buzz testdata audio
//! (`whisper-latvian.wav`, ~7s — the same file `kittest_phases.rs`/
//! `kittest_actions.rs` already decode for real), copied into a tempdir
//! first: the M3 options sheet always writes SRT/TXT exports alongside the
//! input (no output-directory control exists in this milestone,
//! `options_sheet::build_specs`'s `OutputSpec { directory: None, .. }`), so
//! copying is the only way to keep this run's exports out of the real Buzz
//! checkout. CPU, not Metal (`HUMM_FORCE_CPU=true`): the same choice
//! `humm-cli`'s own real `acceptance_*` tests make
//! (`crates/humm-cli/tests/cli.rs`) — Metal under a test-harness process is
//! exactly the combination this milestone's brief warns segfaults/flakes on
//! this machine, and CPU on a tiny model over ~7s of audio is well inside
//! this test's 120s poll timeout regardless.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::{Harness, HarnessBuilder, kittest::Queryable};
use humm_core::cancel::CancelToken;
use humm_core::domain::{ModelSize, Status};
use humm_core::models::download::ModelDownloader;
use humm_core::models::registry::{SENTINEL_SUFFIX, coreml_no_encoder_marker, ggml_file_name};
use humm_core::settings::Settings;
use humm_gui::app::HummApp;
use humm_gui::import::ImportRequest;
use humm_gui::services;

/// Root of Buzz's test fixtures (honoring `HUMM_BUZZ_TESTDATA`) — identical
/// helper to `kittest_phases.rs`/`kittest_actions.rs`/`crates/humm-cli/tests/cli.rs`.
fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

/// Same mechanism as `crates/humm-whisper/tests/contract.rs::ensure_tiny_model`:
/// `$HUMM_WHISPER_TEST_MODEL` if set (and must exist), else the real tiny
/// model via `ModelDownloader`, fetched once and cached thereafter under
/// `humm_core::paths::model_root()`. Deliberately called *before* this
/// test's own `HUMM_MODEL_ROOT` override further down, so `paths::model_root()`
/// here still resolves to the real default cache — not the hermetic tempdir
/// being built for the actual run.
fn ensure_tiny_model() -> PathBuf {
    if let Some(path) = std::env::var_os("HUMM_WHISPER_TEST_MODEL") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "HUMM_WHISPER_TEST_MODEL does not exist: {}",
            path.display()
        );
        return path;
    }
    let downloader = ModelDownloader::new(humm_core::paths::model_root());
    downloader
        .ensure_model(ModelSize::Tiny, false, &mut |_| {}, &CancelToken::new())
        .expect("fetch real ggml-tiny.bin (network on first run, cached thereafter)")
}

/// Seeds `model_root` with the real tiny model (+ completeness sentinel)
/// plus a CoreML negative-cache marker, so the real `DownloaderEnsurer`
/// (`services::init`'s `RunnerDeps::artifacts`) resolves it fully offline —
/// same technique as `crates/humm-cli/tests/cli.rs::seed_real_tiny`. The
/// negative marker matters even under `HUMM_FORCE_CPU`: `services::init`
/// always requests the CoreML encoder on Apple Silicon
/// (`engine_factory.rs`'s `coreml: cfg!(all(target_os = "macos", target_arch
/// = "aarch64")))`), independent of whether the engine ends up using GPU —
/// without the marker, `resolve_model` would still attempt a real network
/// fetch for the encoder zip.
fn seed_real_tiny(model_root: &Path) {
    let real = ensure_tiny_model();
    let cache = real.parent().expect("model path has a parent");
    let name = ggml_file_name(ModelSize::Tiny);
    let sentinel = format!("{name}{SENTINEL_SUFFIX}");
    std::fs::copy(&real, model_root.join(&name)).expect("seed real ggml-tiny.bin");
    std::fs::copy(cache.join(&sentinel), model_root.join(&sentinel)).expect("seed sentinel");
    std::fs::write(coreml_no_encoder_marker(model_root, ModelSize::Tiny), b"")
        .expect("seed coreml negative marker");
}

/// Widened well past the table's full column width (same convention as
/// `kittest_table.rs`'s/`kittest_phases.rs`'s own `wide_harness`).
fn wide_harness(services: services::Services) -> Harness<'static, HummApp> {
    HarnessBuilder::default()
        .with_size(egui::Vec2::new(1700.0, 400.0))
        .build_eframe(move |_cc| HummApp::new_for_test(services))
}

#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model + real Buzz testdata audio; run sandbox-lifted"]
fn j1_import_to_export_end_to_end() {
    // -- Hermetic env, all under one root tempdir ---------------------------
    let root = tempfile::tempdir().expect("root tempdir");
    let data_dir = root.path().join("data");
    let config_dir = root.path().join("config");
    let model_root = root.path().join("models");
    let input_dir = root.path().join("input");
    for dir in [&data_dir, &config_dir, &model_root, &input_dir] {
        std::fs::create_dir_all(dir).expect("create hermetic subdir");
    }

    seed_real_tiny(&model_root);

    // Sticky prefill: model size "tiny". The options sheet's own default
    // (with no prior sticky prefs) is "Small" (`SheetState::from_settings`,
    // M3.md item 19) — seeding this avoids ever having to drive the
    // model-size `egui::ComboBox` through kittest's synthetic input, which
    // no test in this crate does (the header-context-menu popup's own
    // checkbox interactions were already found too timing-fragile for
    // kittest, `kittest_table.rs`'s module doc).
    let mut settings = Settings::default();
    settings.file_transcriber.model_size = Some("tiny".to_string());
    settings
        .save(&config_dir.join("settings.toml"))
        .expect("seed sticky model-size prefill");

    // Real audio, copied into the tempdir: the run's SRT/TXT exports land
    // next to the input (see this file's module doc), so the input itself
    // must not be the real Buzz checkout path.
    let source_wav = testdata_dir().join("whisper-latvian.wav");
    assert!(
        source_wav.is_file(),
        "missing Buzz testdata: {}",
        source_wav.display()
    );
    let input_wav = input_dir.join("whisper-latvian.wav");
    std::fs::copy(&source_wav, &input_wav).expect("copy input audio into the tempdir");

    // Process-global env — see this file's module doc: safe only because
    // this file is its test binary's sole `#[test]`, and this is the first
    // statement to run in it (no other thread — the queue runner, the event
    // pump — has been spawned yet to race a concurrent env read against).
    unsafe {
        std::env::set_var("HUMM_DATA_DIR", &data_dir);
        std::env::set_var("HUMM_CONFIG_DIR", &config_dir);
        std::env::set_var("HUMM_MODEL_ROOT", &model_root);
        // Nonexistent on purpose: disables the one-time Buzz-import path
        // (`paths::buzz_sqlite_path`'s own existence gate) so a real Buzz
        // history on this machine, if any, never leaks into this run.
        std::env::set_var("HUMM_BUZZ_SQLITE", data_dir.join("no-buzz.sqlite"));
        std::env::set_var("HUMM_DISABLE_UPDATE_CHECK", "1");
        std::env::set_var("HUMM_DISABLE_TELEMETRY", "1");
        std::env::set_var("HUMM_DISABLE_VAD", "1");
        // Metal under a test-harness process is exactly the combination
        // this milestone's brief warns segfaults/flakes on this machine;
        // `humm-cli`'s own real `acceptance_*` tests make the identical
        // choice (`crates/humm-cli/tests/cli.rs`).
        std::env::set_var("HUMM_FORCE_CPU", "true");
    }

    let services = services::init(true).expect("services::init against the hermetic env above");
    let store = Arc::clone(&services.store);

    let mut harness = wide_harness(services);
    harness.run(); // spawns the (lazily-created) event pump

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![input_wav.clone()]));
    harness.run();
    harness.get_by_label_contains("1 file");

    harness.get_by_label("SRT").click();
    harness.run();
    harness.get_by_label("TXT").click();
    harness.run();
    harness.get_by_label("Run").click();
    harness.run();

    // The sheet closed: its file-count header is gone (same assertion
    // `kittest_import.rs`'s Run test makes) — this is a same-frame UI state
    // change, unlike the row's own creation below, which depends on the
    // async runner thread draining `Command::Enqueue`.
    assert!(
        harness.query_by_label_contains("1 file").is_none(),
        "the options sheet should have closed after Run"
    );

    // -- Poll the store (authoritative) until Completed, up to 120s --------
    let deadline = Instant::now() + Duration::from_secs(120);
    let id = loop {
        harness.run();
        let tasks = store.list_tasks().expect("list_tasks");
        if let Some(row) = tasks.first() {
            assert_eq!(tasks.len(), 1, "expected exactly one enqueued task");
            if row.status == Status::Completed {
                break row.id;
            }
            assert!(
                !matches!(
                    row.status,
                    Status::Failed | Status::Canceled | Status::Skipped
                ),
                "task reached a non-completing terminal state: {:?} ({:?})",
                row.status,
                row.error_message
            );
        }
        assert!(
            Instant::now() < deadline,
            "task did not reach Completed within 120s"
        );
        std::thread::sleep(Duration::from_millis(50));
    };

    // -- Store assertions -----------------------------------------------------
    let segments = store.get_segments(&id).expect("get_segments");
    assert!(
        !segments.is_empty(),
        "expected a non-empty transcript from the real tiny model — an empty \
         transcript would vacuously satisfy every other assertion here \
         without proving the real engine actually ran"
    );

    // -- Export assertions -----------------------------------------------------
    let entries: Vec<PathBuf> = std::fs::read_dir(&input_dir)
        .expect("read_dir input_dir")
        .map(|entry| entry.expect("dir entry").path())
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
        "expected one .srt export, found {entries:?}"
    );
    assert_eq!(
        txt_files.len(),
        1,
        "expected one .txt export, found {entries:?}"
    );

    let srt_contents = std::fs::read_to_string(srt_files[0]).expect("read srt");
    assert!(
        srt_contents.starts_with("1\n"),
        "srt did not start as expected: {srt_contents:?}"
    );
    let txt_contents = std::fs::read_to_string(txt_files[0]).expect("read txt");
    assert!(!txt_contents.trim().is_empty(), "txt export was empty");

    // -- UI assertion: the row shows Completed via AccessKit --------------------
    // Same technique as `kittest_phases.rs`/`kittest_actions.rs`: the "Date
    // Completed" column header already contributes one "…Completed…" match
    // from the very first frame, so a second match proves the row's own
    // status cell also rendered something containing "Completed".
    harness.run();
    assert!(
        harness.query_all_by_label_contains("Completed").count() >= 2,
        "expected the row's own status cell to also render something containing \"Completed\""
    );
}
