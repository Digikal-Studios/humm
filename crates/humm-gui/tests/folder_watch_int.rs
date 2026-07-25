//! Acceptance-gate integration test for Task 9 (F-030): a real `notify`
//! watcher over temp directories, driving `humm_gui::folder_watch::
//! FolderWatchService` directly against `services::init_with`'s real
//! ffmpeg decode/probe (a real, short Buzz-testdata `.wav`) plus a scripted
//! engine (the same `FakeSucceedingEngineFactory` shape `kittest_actions.rs`
//! already uses for its own real-decode Retry-to-completion test) — no
//! whisper model, no network. Drop a fixture file → `wait_for` the store to
//! show a `Completed` `Source::FolderWatch` row → assert the mirrored export
//! exists and the source was moved/deleted per the disposition; a pause/
//! resume case; and a rebuild case proving the shared `SeenSet` (Buzz's
//! `paths_emitted`) survives a service restart without double-enqueueing.
//!
//! ## Watcher backend: `PollWatcher`, not FSEvents
//!
//! Every test here uses [`FolderWatchService::start_with_poll_watcher_for_test`],
//! not the production `start` (`notify::RecommendedWatcher`, FSEvents on
//! macOS): a standalone `notify::recommended_watcher` probe against a
//! freshly created temp directory in this sandbox never received a single
//! event within 10s, confirming the delivery gap is this sandboxed test
//! environment's FSEvents support, not this module's own event handling — a
//! review pass separately confirmed the production FSEvents path works
//! correctly when run unsandboxed. `notify` + temp directories are still
//! local, network-free, and don't bind TCP either way, so — unlike
//! `tests/acceptance.rs`'s real-model run — these tests are **not**
//! `#[ignore]`d: they must (and do) pass in the default sandbox via the
//! polling fallback.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use humm_core::boundary::{EngineCaps, EngineOptions, Pcm, SegmentSink, TranscriptionEngine};
use humm_core::cancel::CancelToken;
use humm_core::domain::{ModelSpec, Source, Status};
use humm_core::error::{CoreError, EngineError};
use humm_core::folderwatch::SeenSet;
use humm_core::queue::{EngineFactory, QueueHandle};
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

/// One export format configured (`txt`) — a folder-watch job with an empty
/// format set would never write anything to assert on.
fn folder_watch_options() -> FileTranscriberSettings {
    FileTranscriberSettings {
        export_formats: Some(vec!["txt".to_string()]),
        ..FileTranscriberSettings::default()
    }
}

/// Starts a [`FolderWatchService`] watching `input_dir`/`output_dir` via
/// [`FolderWatchService::start_with_poll_watcher_for_test`] (see this file's
/// module doc for why not FSEvents) — a short poll interval keeps each
/// test's own wall-clock budget small. `seen`/`initially_paused` are exposed
/// so tests can exercise the shared-`SeenSet`-across-a-rebuild and
/// deterministic-pause-race-closure cases; every other test just wants a
/// fresh, unpaused `SeenSet`.
fn start_service(
    queue: Arc<QueueHandle>,
    store: Arc<SqliteStore>,
    input_dir: &Path,
    output_dir: &Path,
    delete_processed_files: bool,
    seen: Arc<Mutex<SeenSet>>,
    initially_paused: bool,
) -> FolderWatchService {
    let snapshot = FolderWatchSnapshot {
        enabled: true,
        input_dir: Some(input_dir.to_path_buf()),
        output_dir: Some(output_dir.to_path_buf()),
        delete_processed_files,
        options: folder_watch_options(),
    };
    FolderWatchService::start_with_poll_watcher_for_test(
        snapshot,
        queue,
        store,
        egui::Context::default(),
        seen,
        initially_paused,
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
/// `Source::FolderWatch` row reaches `Completed`, up to `timeout` — polling
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

/// A fresh, empty, unshared `SeenSet` — the common case for tests that don't
/// care about cross-rebuild sharing.
fn fresh_seen() -> Arc<Mutex<SeenSet>> {
    Arc::new(Mutex::new(SeenSet::new()))
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
    let queue = Arc::new(services.queue.take().expect("queue present"));
    let service = start_service(
        Arc::clone(&queue),
        Arc::clone(&store),
        &input_dir,
        &output_dir,
        false,
        fresh_seen(),
        false,
    );

    // Settle past the (poll-interval-scale) initial scan before dropping the
    // fixture — the initial scan runs on an empty directory and finds
    // nothing, so detection below must come from the watcher noticing the
    // *new* file on a later poll, pinning event-driven detection rather than
    // a lucky race where the initial scan itself happened to run after the
    // drop.
    std::thread::sleep(Duration::from_millis(20));
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
    let queue = Arc::new(services.queue.take().expect("queue present"));
    let service = start_service(
        Arc::clone(&queue),
        Arc::clone(&store),
        &input_dir,
        &output_dir,
        true,
        fresh_seen(),
        false,
    );

    // See the move test's identical comment: settle past the initial scan
    // so detection is pinned to the event-driven poll path.
    std::thread::sleep(Duration::from_millis(20));
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
    let queue = Arc::new(services.queue.take().expect("queue present"));
    // `initially_paused: true` closes the pause race deterministically (no
    // window between "started" and "paused" to race against) — see
    // `start_with_poll_watcher_for_test`'s own doc.
    let service = start_service(
        Arc::clone(&queue),
        Arc::clone(&store),
        &input_dir,
        &output_dir,
        false,
        fresh_seen(),
        true,
    );
    assert!(service.is_paused());

    drop_fixture(&input_dir.join("clip.wav"));

    // A short settle window: long enough for several polls to have fired and
    // the worker to have drained-and-skipped each one, nowhere near
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

// -- SeenSet survives a rebuild (F-030 review fix 2) -------------------------

#[test]
fn seen_set_survives_a_rebuild_so_a_recently_enqueued_file_is_not_double_enqueued() {
    // Simulates a Preferences-edit-triggered restart racing the
    // enqueue→create_task window: `QueueHandle::enqueue` is fire-and-forget
    // (the runner thread's own `create_task` may not have landed in the
    // store yet), so if a rebuild ever handed the new service a *fresh*
    // `SeenSet`, a restart landing in that exact window would find the file
    // "not known" (no store row yet) and "not seen" (blank set) and enqueue
    // it a second time. `app.rs`'s `HummApp::folder_watch_seen` instead
    // shares one `Arc<Mutex<SeenSet>>` across every start/restart for the
    // process's lifetime (Buzz's own `paths_emitted` lives on the one
    // `TranscriptionTaskFolderWatcher` object the same way) — this test
    // proves that sharing is what actually prevents the double-enqueue,
    // by driving the exact race deterministically: wait only for the
    // *shared* `SeenSet` to record the candidate (which happens
    // synchronously, right after `queue.enqueue`, deliberately *before*
    // ever waiting for the store to reflect it), then stop service A and
    // start service B with the same `Arc<Mutex<SeenSet>>` and rescan.
    let root = tempfile::tempdir().expect("root tempdir");
    let input_dir = root.path().join("in");
    let output_dir = root.path().join("out");
    std::fs::create_dir_all(&input_dir).expect("create input root");
    std::fs::create_dir_all(&output_dir).expect("create output root");

    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let mut services = test_services(Arc::clone(&store), root.path());
    let queue = Arc::new(services.queue.take().expect("queue present"));
    let seen = fresh_seen();

    let source = input_dir.join("clip.wav");
    drop_fixture(&source);

    let service_a = start_service(
        Arc::clone(&queue),
        Arc::clone(&store),
        &input_dir,
        &output_dir,
        false,
        Arc::clone(&seen),
        false,
    );

    // Wait for service A's (unconditional) initial scan to have recorded the
    // candidate into the *shared* SeenSet — deliberately not waiting for the
    // store to show a row at all, so we stop A as close as possible to the
    // real enqueue→create_task race window.
    let metadata = std::fs::metadata(&source).expect("stat fixture");
    let mtime = metadata.modified().expect("mtime");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let recorded = seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .matches(&source, metadata.len(), mtime);
        if recorded {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "service A never recorded the candidate into the shared SeenSet"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    service_a.stop();

    // Rebuild with the SAME shared SeenSet (and the same queue/store) — the
    // exact shape `app.rs`'s `sync_folder_watch` Restart path performs.
    let service_b = start_service(
        Arc::clone(&queue),
        Arc::clone(&store),
        &input_dir,
        &output_dir,
        false,
        Arc::clone(&seen),
        false,
    );

    // Give B's own initial scan (and any settling) time to run, then assert
    // exactly one Source::FolderWatch row exists — not two.
    std::thread::sleep(Duration::from_millis(500));
    let folder_watch_rows: Vec<TaskRow> = store
        .list_tasks()
        .expect("list_tasks")
        .into_iter()
        .filter(|row| row.source == Some(Source::FolderWatch))
        .collect();
    assert_eq!(
        folder_watch_rows.len(),
        1,
        "expected exactly one enqueued task across the rebuild, got {folder_watch_rows:?}"
    );

    service_b.stop();
}
