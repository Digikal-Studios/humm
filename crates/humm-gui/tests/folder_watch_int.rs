//! Acceptance-gate integration test for Task 9 (F-030): a real `notify`
//! watcher over temp directories, driving `humm_gui::folder_watch::
//! FolderWatchService` directly against `services::init_with`'s real
//! ffmpeg decode/probe (a real, short Buzz-testdata `.wav`) plus a scripted
//! engine (the same `FakeSucceedingEngineFactory` shape `kittest_actions.rs`
//! already uses for its own real-decode Retry-to-completion test) — no
//! whisper model, no network. Drop a fixture file → `wait_for` the store to
//! show a `Completed` `Source::FolderWatch` row → assert the mirrored export
//! exists and the source was moved/deleted per the disposition; a pause/
//! resume case; a stop during settle, which must not record `SeenSet`; and a
//! file that settles while the runner is inside another task, which is
//! adopted once that task ends rather than left queued.
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
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use humm_core::boundary::{EngineCaps, EngineOptions, Pcm, SegmentSink, TranscriptionEngine};
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    Input, ModelSpec, OutputSpec, Source, Status, TaskSpec, TranscriptionId, TranscriptionOptions,
};
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

/// Builds real [`Services`] (real ffmpeg/ffprobe adapters, an in-memory
/// store) under `dir`, with the given scripted `engines` factory — the same
/// construction `kittest_actions.rs`'s `test_services` uses, generalized
/// over both the store (each test wants its own fresh one) and the engine
/// factory (the busy-runner regression test below needs a gated one).
fn test_services_with_engine(
    store: Arc<SqliteStore>,
    dir: &Path,
    engines: Box<dyn EngineFactory>,
) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: dir.join("media"),
        model_root: dir.join("models"),
        settings: Settings::default(),
        settings_path: dir.join("settings.toml"),
        engines,
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    })
}

/// [`test_services_with_engine`] with the always-succeeds
/// [`FakeSucceedingEngineFactory`] — what every test except the busy-runner
/// regression below wants.
fn test_services(store: Arc<SqliteStore>, dir: &Path) -> Services {
    test_services_with_engine(store, dir, Box::new(FakeSucceedingEngineFactory))
}

/// An [`EngineFactory`]/[`TranscriptionEngine`] pair whose `transcribe` call
/// blocks until released — lets a test hold the queue's single runner thread
/// busy inside `run_task` on demand (`humm_core::queue`'s own doc: commands
/// only drain *between* tasks, never while one is running), to deterministically
/// open the enqueue→`create_task` race window
/// `seen_set_survives_a_rebuild_so_a_recently_enqueued_file_is_not_double_enqueued`
/// needs. Every engine instance shares the same gate, so releasing it once
/// (for the blocker task) also means every *later* task's `transcribe` call
/// returns immediately — no re-closing needed.
struct GatedEngineFactory {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl GatedEngineFactory {
    fn new() -> (Self, Arc<(Mutex<bool>, Condvar)>) {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        (
            GatedEngineFactory {
                gate: Arc::clone(&gate),
            },
            gate,
        )
    }
}

impl EngineFactory for GatedEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        Ok(Box::new(GatedEngine {
            gate: Arc::clone(&self.gate),
        }))
    }
}

struct GatedEngine {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl TranscriptionEngine for GatedEngine {
    fn transcribe(
        &mut self,
        _audio: &Pcm,
        _options: &EngineOptions,
        _sink: &mut dyn SegmentSink,
        _cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        let (lock, cvar) = &*self.gate;
        let mut released = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while !*released {
            released = cvar
                .wait(released)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
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

/// Releases every [`GatedEngine`] sharing `gate` — wakes a `transcribe` call
/// currently blocked in it, and makes every later call on the same gate a
/// no-op wait (the flag stays `true`).
fn release_gate(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, cvar) = &**gate;
    *lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    cvar.notify_all();
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
        // No global template override needed for this integration test — the
        // export-format assertions below only care about the file's
        // extension, not its stem.
        default_export_template: None,
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

/// A plain, non-folder-watch [`TaskSpec`] over `input` — the busy-runner
/// regression test's "blocker" task, occupying the queue's single runner
/// thread deliberately (`Source::FileImport`, so it never shows up in a
/// `Source::FolderWatch` row count).
fn blocker_spec(input: PathBuf) -> TaskSpec {
    TaskSpec {
        input: Input::Path(input),
        options: TranscriptionOptions::default(),
        output: OutputSpec::default(),
    }
}

/// Polls `store.get_task(id)` until its status is `status`, up to `timeout`.
fn wait_for_status(store: &SqliteStore, id: TranscriptionId, status: Status, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(Some(row)) = store.get_task(&id)
            && row.status == status
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "task did not reach {status:?} within {}s",
            timeout.as_secs()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Polls `store` (the authoritative source, not any UI) until a
/// `Source::FolderWatch` row reaches `Completed`, up to `timeout` — polling
/// latency plus a 30s settle and a real ffmpeg decode of a ~7s clip needs
/// more than 30s
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

    let row = wait_for_folder_watch_completed(&store, Duration::from_secs(60));
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

    wait_for_folder_watch_completed(&store, Duration::from_secs(60));

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

    let row = wait_for_folder_watch_completed(&store, Duration::from_secs(60));
    assert_eq!(row.status, Status::Completed);

    service.stop();
}

// -- SeenSet survives a rebuild (F-030 review fix 2, reshaped per re-review) --

#[test]
fn seen_set_survives_a_rebuild_so_a_recently_enqueued_file_is_not_double_enqueued() {
    // Stopping during the 30s settle cancels the staged row and does not
    // record the seen-set. A rebuilt watch must create a new task. The
    // blocker keeps the runner from adopting anything until we release it,
    // so both the abandon and the second stage are still ordered commands.
    let root = tempfile::tempdir().expect("root tempdir");
    let input_dir = root.path().join("in");
    let output_dir = root.path().join("out");
    let blocker_input = root.path().join("blocker.wav");
    std::fs::create_dir_all(&input_dir).expect("create input root");
    std::fs::create_dir_all(&output_dir).expect("create output root");
    drop_fixture(&blocker_input);

    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let (engine_factory, gate) = GatedEngineFactory::new();
    let mut services =
        test_services_with_engine(Arc::clone(&store), root.path(), Box::new(engine_factory));
    let queue = Arc::new(services.queue.take().expect("queue present"));
    let seen = fresh_seen();

    // Occupy the runner: enqueue the blocker and wait for it to actually be
    // running (not merely queued) before touching folder watch at all.
    let blocker_id = TranscriptionId::new();
    queue.enqueue(blocker_id, blocker_spec(blocker_input), Source::FileImport);
    wait_for_status(
        &store,
        blocker_id,
        Status::InProgress,
        Duration::from_secs(10),
    );

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

    // Stop during the 30s settle. The seen-set is recorded only when the
    // file settles, so it must still be empty, and the staged row is
    // abandoned. A rebuilt watch has to be able to find the file again.
    std::thread::sleep(Duration::from_millis(500));
    let metadata = std::fs::metadata(&source).expect("stat fixture");
    let mtime = metadata.modified().expect("mtime");
    assert!(
        !seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .matches(&source, metadata.len(), mtime),
        "stopping during settle must not record the path"
    );
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

    // Give B's own initial scan time to run (immediate on spawn, not tied to
    // the poll interval) — still while the runner is busy on the blocker.
    std::thread::sleep(Duration::from_millis(300));

    // Release the blocker: the runner drains every command that piled up
    // behind it (A's enqueue, and — only on a regression — a second one from
    // B) in one shot, fixing the final row count right there.
    release_gate(&gate);
    wait_for_status(
        &store,
        blocker_id,
        Status::Completed,
        Duration::from_secs(10),
    );

    // A little settle so any (correctly single) candidate task the drain
    // just queued gets its own store row before we count.
    std::thread::sleep(Duration::from_millis(300));

    let folder_watch_rows: Vec<TaskRow> = store
        .list_tasks()
        .expect("list_tasks")
        .into_iter()
        .filter(|row| row.source == Some(Source::FolderWatch))
        .collect();
    let live: Vec<_> = folder_watch_rows
        .iter()
        .filter(|row| row.status != Status::Canceled)
        .collect();
    assert_eq!(
        live.len(),
        1,
        "a stop during settle must not block a new task, got {folder_watch_rows:?}"
    );

    service_b.stop();
}

/// A file that reaches the 30s settle while the runner is inside another
/// task must be adopted after that task ends. `Stage` cannot create the row
/// until `run_task` returns, so the settle must not record `SeenSet` first.
#[test]
fn file_that_settles_during_a_long_task_is_adopted() {
    let root = tempfile::tempdir().expect("root tempdir");
    let input_dir = root.path().join("in");
    let output_dir = root.path().join("out");
    let blocker_input = root.path().join("blocker.wav");
    std::fs::create_dir_all(&input_dir).expect("create input root");
    std::fs::create_dir_all(&output_dir).expect("create output root");
    drop_fixture(&blocker_input);

    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let (engine_factory, gate) = GatedEngineFactory::new();
    let mut services =
        test_services_with_engine(Arc::clone(&store), root.path(), Box::new(engine_factory));
    let queue = Arc::new(services.queue.take().expect("queue present"));
    let seen = fresh_seen();

    let blocker_id = TranscriptionId::new();
    queue.enqueue(blocker_id, blocker_spec(blocker_input), Source::FileImport);
    wait_for_status(
        &store,
        blocker_id,
        Status::InProgress,
        Duration::from_secs(10),
    );

    let source = input_dir.join("clip.wav");
    drop_fixture(&source);
    let service = start_service(
        Arc::clone(&queue),
        Arc::clone(&store),
        &input_dir,
        &output_dir,
        false,
        Arc::clone(&seen),
        false,
    );

    // Past the 30s settle and one 5s poll, still inside the blocker. The
    // staged row must not exist yet, and the path must not be seen.
    std::thread::sleep(Duration::from_secs(40));
    assert_eq!(
        store
            .get_task(&blocker_id)
            .expect("blocker row")
            .expect("blocker exists")
            .status,
        Status::InProgress,
        "the blocker must still be running when the second file settles"
    );
    let folder_rows: Vec<_> = store
        .list_tasks()
        .expect("list_tasks")
        .into_iter()
        .filter(|row| row.source == Some(Source::FolderWatch))
        .collect();
    assert!(
        folder_rows.is_empty(),
        "Stage must still be queued behind the blocker, got {folder_rows:?}"
    );
    let metadata = std::fs::metadata(&source).expect("stat watched file");
    let mtime = metadata.modified().expect("mtime");
    assert!(
        !seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .matches(&source, metadata.len(), mtime),
        "a settle before the row exists must not record the path"
    );

    release_gate(&gate);
    let row = wait_for_folder_watch_completed(&store, Duration::from_secs(30));
    assert_eq!(row.status, Status::Completed);
    assert!(
        seen.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .matches(&source, metadata.len(), mtime),
        "the path is seen only once the queued row was adopted"
    );

    service.stop();
}
