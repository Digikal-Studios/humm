//! Acceptance-gate integration test for Task 9 (F-030): a real `notify`
//! watcher over temp directories, driving `humm_gui::folder_watch::
//! FolderWatchService` directly against `services::init_with`'s real
//! ffmpeg decode/probe (a real, short Buzz-testdata `.wav`) plus a scripted
//! engine (the same `FakeSucceedingEngineFactory` shape `kittest_actions.rs`
//! already uses for its own real-decode Retry-to-completion test) — no
//! whisper model, no network. Drop a fixture file → `wait_for` the store to
//! show a `Completed` `Source::FolderWatch` row → assert the mirrored export
//! exists and the source was moved/deleted per the disposition; a third case
//! proves a paused service ignores a drop until resumed.
//!
//! `notify` + temp directories are local, network-free, and don't bind TCP,
//! so — unlike `tests/acceptance.rs`'s real-model run — these tests are
//! **not** `#[ignore]`d: they must (and do) pass in the default sandbox.
//! FSEvents (this machine's `notify` backend, macOS) needs no special
//! entitlement to watch a directory the test process itself owns.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use humm_core::boundary::{EngineCaps, EngineOptions, Pcm, SegmentSink, TranscriptionEngine};
use humm_core::cancel::CancelToken;
use humm_core::domain::{ModelSpec, Source, Status};
use humm_core::error::{CoreError, EngineError};
use humm_core::queue::EngineFactory;
use humm_core::settings::{FileTranscriberSettings, Settings};
use humm_gui::folder_watch::{FolderWatchService, FolderWatchSnapshot};
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_store::{SqliteStore, TaskRow};

/// A fake [`EngineFactory`]/[`TranscriptionEngine`] pair that always
/// succeeds with zero segments — same shape as `kittest_actions.rs`'s own
/// `FakeSucceedingEngineFactory`: the real ffmpeg decode/ffprobe probe
/// (`services::init_with`'s hard-coded real adapters) still runs for real
/// against the real fixture audio below, only the whisper inference step
/// itself is faked, so this test proves the folder-watch → queue → pipeline
/// → disposition path end to end without needing a real model.
struct FakeSucceedingEngineFactory;

impl EngineFactory for FakeSucceedingEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        Ok(Box::new(FakeEngine))
    }
}

struct FakeEngine;

impl TranscriptionEngine for FakeEngine {
    fn transcribe(
        &mut self,
        _audio: &Pcm,
        _options: &EngineOptions,
        _sink: &mut dyn SegmentSink,
        _cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        Ok(())
    }

    fn capabilities(&self) -> EngineCaps {
        EngineCaps {
            word_timestamps: false,
            translate: false,
            vad: false,
        }
    }
}

/// Root of Buzz's test fixtures (honoring `HUMM_BUZZ_TESTDATA`) — identical
/// helper to `kittest_actions.rs`/`kittest_phases.rs`/`tests/acceptance.rs`.
fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

/// Builds real [`Services`] (real ffmpeg/ffprobe adapters, a scripted
/// engine, an in-memory store) under `dir` — the same construction
/// `kittest_actions.rs`'s `test_services` uses, generalized only in that the
/// caller supplies the store (each test wants its own fresh one).
fn test_services(store: Arc<SqliteStore>, dir: &Path) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: dir.join("media"),
        model_root: dir.join("models"),
        settings: Settings::default(),
        settings_path: dir.join("settings.toml"),
        engines: Box::new(FakeSucceedingEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    })
}

/// Starts a [`FolderWatchService`] watching `input_dir`/`output_dir`, always
/// with one export format configured (`txt`) — a folder-watch job with an
/// empty format set would never write anything to assert on. Takes
/// `services.queue` out and `Arc`-wraps it, the same "sole owner takes it
/// once" seam `app.rs`'s `HummApp::build` performs for the real app.
///
/// Uses [`FolderWatchService::start_with_poll_watcher_for_test`], not the
/// production `start` (FSEvents): a standalone `notify::recommended_watcher`
/// probe against a freshly created temp directory in this sandbox never
/// received a single event within 10s, confirming the delivery gap is this
/// environment's FSEvents support, not this service's own event handling —
/// exactly the fallback the task brief anticipated. A short poll interval
/// keeps this test's own wall-clock budget small.
fn start_service(
    services: &mut Services,
    input_dir: &Path,
    output_dir: &Path,
    delete_processed_files: bool,
) -> FolderWatchService {
    let queue = Arc::new(
        services
            .queue
            .take()
            .expect("services::init_with always constructs Some(queue)"),
    );
    let snapshot = FolderWatchSnapshot {
        enabled: true,
        input_dir: Some(input_dir.to_path_buf()),
        output_dir: Some(output_dir.to_path_buf()),
        delete_processed_files,
        options: FileTranscriberSettings {
            export_formats: Some(vec!["txt".to_string()]),
            ..FileTranscriberSettings::default()
        },
    };
    FolderWatchService::start_with_poll_watcher_for_test(
        snapshot,
        queue,
        Arc::clone(&services.store),
        egui::Context::default(),
        Duration::from_millis(100),
    )
    .expect("start the folder-watch service over a real temp directory")
}

/// Copies the real, short Buzz-testdata fixture into `dest` — same fixture
/// `kittest_actions.rs`'s `retry_on_a_failed_row_reaches_completion` and
/// `tests/acceptance.rs` use for a real ffmpeg decode.
fn drop_fixture(dest: &Path) {
    let fixture = testdata_dir().join("whisper-latvian.wav");
    assert!(
        fixture.is_file(),
        "missing Buzz testdata: {}",
        fixture.display()
    );
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).expect("create destination dir");
    }
    std::fs::copy(&fixture, dest).expect("copy fixture into the watched tree");
}

/// Polls `store` (the authoritative source, not any UI) until a
/// `Source::FolderWatch` row reaches `Completed`, up to `timeout` — FSEvents
/// latency plus a real ffmpeg decode of a ~7s clip is comfortably inside 30s
/// even under sandboxed/loaded CI, the same generous-but-bounded margin
/// `kittest_actions.rs`'s own real-decode retry test (20s) and
/// `tests/acceptance.rs`'s real-model run (120s) use for the identical
/// "real background thread, real wall-clock work" reason.
fn wait_for_folder_watch_completed(store: &SqliteStore, timeout: Duration) -> TaskRow {
    let deadline = Instant::now() + timeout;
    loop {
        let tasks = store.list_tasks().expect("list_tasks");
        if let Some(row) = tasks
            .iter()
            .find(|row| row.source == Some(Source::FolderWatch))
        {
            if row.status == Status::Completed {
                return row.clone();
            }
            assert!(
                !matches!(row.status, Status::Failed | Status::Canceled),
                "folder-watch task reached a non-completing terminal state: {:?} ({:?})",
                row.status,
                row.error_message
            );
        }
        assert!(
            Instant::now() < deadline,
            "no completed Source::FolderWatch row within {}s (rows so far: {tasks:?})",
            timeout.as_secs()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Every entry directly under `dir` (non-recursive), or an empty `Vec` if
/// `dir` doesn't exist yet.
fn dir_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read_dir
        .map(|entry| entry.expect("dir entry").path())
        .collect()
}

// -- Move-to-output disposition ----------------------------------------------

#[test]
fn drop_creates_a_task_and_moves_the_source_to_the_mirrored_output_dir() {
    let root = tempfile::tempdir().expect("root tempdir");
    let input_dir = root.path().join("in");
    let output_dir = root.path().join("out");
    let sub = input_dir.join("sub");
    std::fs::create_dir_all(&sub).expect("create input/sub");
    std::fs::create_dir_all(&output_dir).expect("create output root");

    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let mut services = test_services(Arc::clone(&store), root.path());
    let service = start_service(&mut services, &input_dir, &output_dir, false);

    drop_fixture(&sub.join("clip.wav"));

    let row = wait_for_folder_watch_completed(&store, Duration::from_secs(30));
    assert_eq!(row.status, Status::Completed);

    // Source moved into the mirrored output subdirectory, under its own
    // basename (design spec §3.4's `MoveToOutput`). The store's own `file`
    // column keeps recording the *original* input path — the disposition
    // move is a filesystem-only side effect the pipeline never reflects
    // back into the row (Buzz has no such column to update either), so this
    // is a filesystem assertion, not a store one.
    let moved = output_dir.join("sub").join("clip.wav");
    assert!(
        moved.is_file(),
        "expected the source moved to {}",
        moved.display()
    );
    assert!(
        !sub.join("clip.wav").exists(),
        "the source must no longer exist at its original path"
    );

    // The mirrored export directory has at least the requested .txt export
    // (plus the moved source above) — existence only, not content: the
    // fake engine produces zero segments, so the export's contents aren't
    // the thing under test here.
    let exported = dir_entries(&output_dir.join("sub"))
        .into_iter()
        .any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"));
    assert!(
        exported,
        "expected a .txt export under {}, found {:?}",
        output_dir.join("sub").display(),
        dir_entries(&output_dir.join("sub"))
    );

    service.stop();
}

// -- Delete disposition -------------------------------------------------------

#[test]
fn drop_with_delete_processed_files_removes_the_source() {
    let root = tempfile::tempdir().expect("root tempdir");
    let input_dir = root.path().join("in");
    let output_dir = root.path().join("out");
    std::fs::create_dir_all(&input_dir).expect("create input root");
    std::fs::create_dir_all(&output_dir).expect("create output root");

    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let mut services = test_services(Arc::clone(&store), root.path());
    let service = start_service(&mut services, &input_dir, &output_dir, true);

    let source = input_dir.join("clip.wav");
    drop_fixture(&source);

    wait_for_folder_watch_completed(&store, Duration::from_secs(30));

    assert!(
        !source.exists(),
        "the source must have been deleted, not moved"
    );
    assert!(
        !output_dir.join("clip.wav").exists(),
        "delete disposition must not also leave a copy in the output dir"
    );
    let exported = dir_entries(&output_dir)
        .into_iter()
        .any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"));
    assert!(
        exported,
        "expected a .txt export under {}, found {:?}",
        output_dir.display(),
        dir_entries(&output_dir)
    );

    service.stop();
}

// -- Pause / resume -----------------------------------------------------------

#[test]
fn paused_service_ignores_a_drop_until_resumed() {
    let root = tempfile::tempdir().expect("root tempdir");
    let input_dir = root.path().join("in");
    let output_dir = root.path().join("out");
    std::fs::create_dir_all(&input_dir).expect("create input root");
    std::fs::create_dir_all(&output_dir).expect("create output root");

    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let mut services = test_services(Arc::clone(&store), root.path());
    let service = start_service(&mut services, &input_dir, &output_dir, false);

    service.pause(true);
    assert!(service.is_paused());

    drop_fixture(&input_dir.join("clip.wav"));

    // A short settle window: long enough for FSEvents to have fired and the
    // worker to have drained-and-skipped it at least once, nowhere near
    // `wait_for_folder_watch_completed`'s 30s budget — this is a negative
    // assertion (nothing should happen), not a race against real work.
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        store
            .list_tasks()
            .expect("list_tasks")
            .iter()
            .all(|row| row.source != Some(Source::FolderWatch)),
        "a paused service must not enqueue anything"
    );

    service.pause(false);
    assert!(!service.is_paused());

    let row = wait_for_folder_watch_completed(&store, Duration::from_secs(30));
    assert_eq!(row.status, Status::Completed);

    service.stop();
}
