//! The notify-backed folder-watch service (F-030, design spec §3.2), ported
//! from Buzz's `TranscriptionTaskFolderWatcher`
//! (`buzz/widgets/transcription_task_folder_watcher.py`, whole file read for
//! this port) and its lifecycle in `main_window.py:167-183,500-507`.
//!
//! Buzz's watcher is a `QFileSystemWatcher` subclass: it manually adds every
//! subdirectory to the OS watch list (`set_preferences`), rescans the
//! *entire* input tree on every `directoryChanged` signal
//! (`find_tasks`, `:53-131`), and does the identical full rescan once more at
//! startup and on every Preferences change (`main_window.py:167-183`). This
//! module keeps the exact same "rescan the whole tree on any change" model —
//! `notify::RecommendedWatcher` already recurses on our behalf
//! (`RecursiveMode::Recursive`), so there is no manual per-subdirectory
//! bookkeeping to port — but moves the actual scan/skip logic into
//! [`humm_core::folderwatch::scan`], the pure half of this port (already
//! landed): this module is only the OS-boundary glue around it (watch the
//! filesystem, drain events, rescan, enqueue), which is why it lives in
//! `humm-gui` rather than `humm-core` (design spec §3.2: folder watch is a
//! GUI-app feature in Buzz — the CLI has no folder-watch surface — and core
//! stays OS-boundary-free).
//!
//! ## Threading model
//!
//! One `notify` watcher (kept alive for its `Drop`-based unwatch) forwards
//! every filesystem event into a channel; one worker thread drains it in a
//! "drain-all-pending-then-rescan" loop — the coalescing equivalent of Buzz
//! rescanning its whole tree per `directoryChanged` signal, `:53-60` — plus
//! one initial scan at start (Buzz's own `find_tasks()` call immediately
//! after constructing the watcher, `main_window.py:172`), skipped only if the
//! service was paused before that scan ever got to run. Pausing
//! ([`FolderWatchService::pause`]) still drains events (so they don't pile up
//! unboundedly) but skips the rescan itself, and never touches the
//! [`SeenSet`] — resuming picks up exactly where scanning left off, not from
//! a blank slate.
//!
//! `queue`/`store`/`ctx` are all cheaply cloneable, thread-safe handles
//! (`Arc<QueueHandle>`/`Arc<SqliteStore>`/`egui::Context`) moved into the
//! worker thread once at [`FolderWatchService::start`] time. `QueueHandle`
//! itself has no `Clone` impl (`humm_core::queue`'s own doc: it uniquely owns
//! the runner's `JoinHandle`, by design — only one thing may ever call
//! [`humm_core::queue::QueueHandle::finish`]/`finish_within`), so `app.rs`
//! wraps the *one* handle `Services` constructs in an `Arc` once, shares it
//! with this service, and reclaims it for shutdown via `Arc::try_unwrap`
//! once the service (which is always stopped first) has dropped its own
//! clone — see `app.rs`'s `HummApp::queue`/`on_exit` doc comments.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use humm_core::domain::{Source, SourceDisposition, TranscriptionId};
use humm_core::folderwatch::{self, SeenSet};
use humm_core::queue::QueueHandle;
use humm_core::settings::{FileTranscriberSettings, FolderWatchSettings};
use humm_store::SqliteStore;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

use crate::views::options_sheet::spec_from_transcriber_settings;

/// A plain-data snapshot of `settings.folder_watch`, cheap to clone and
/// compare (`PartialEq`) so `app.rs`'s per-frame reconcile can tell "nothing
/// changed" from "the service needs a restart" without ever touching the
/// filesystem or the settings mutex more than once per frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderWatchSnapshot {
    /// Whether folder-watching is turned on at all.
    pub enabled: bool,
    /// The directory scanned for new media files (Buzz's `input_folder`).
    pub input_dir: Option<PathBuf>,
    /// The directory mirrored transcription outputs are written under
    /// (Buzz's `output_directory`).
    pub output_dir: Option<PathBuf>,
    /// Delete (`true`) vs. move-into-output (`false`) processed sources
    /// (design spec §3.4).
    pub delete_processed_files: bool,
    /// Per-auto-task file-transcription options (Buzz's
    /// `file_transcription_options` sub-group).
    pub options: FileTranscriberSettings,
}

impl FolderWatchSnapshot {
    /// Clones the live `[folder_watch]` settings into a snapshot.
    #[must_use]
    pub fn from_settings(settings: &FolderWatchSettings) -> Self {
        FolderWatchSnapshot {
            enabled: settings.enabled,
            input_dir: settings.input_dir.clone(),
            output_dir: settings.output_dir.clone(),
            delete_processed_files: settings.delete_processed_files,
            options: settings.options.clone(),
        }
    }

    /// Whether this snapshot has everything [`FolderWatchService::start`]
    /// needs: watching turned on, and both directories configured. Buzz's
    /// own equivalent gate is `preferences.enabled and preferences.
    /// input_directory` (`:42`, `:57`) — humm additionally requires
    /// `output_dir`, since a folder-watch task with nowhere to write its
    /// mirrored export has no Buzz behavior to match (Buzz's own
    /// `output_directory` is a required field of the same preferences
    /// object, never independently absent).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && self.input_dir.is_some() && self.output_dir.is_some()
    }

    /// The watched folder's basename, for the toolbar pill's label (design
    /// spec §3.3) — `None` whenever [`Self::is_active`] is `false`, which
    /// doubles as the pill's own visibility rule (`app.rs` only ever builds a
    /// pill state when this is `Some`). A pure, `egui`-free function so it's
    /// directly unit-testable.
    #[must_use]
    pub fn pill_folder_name(&self) -> Option<String> {
        if !self.is_active() {
            return None;
        }
        self.input_dir
            .as_ref()
            .and_then(|dir| dir.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }
}

/// What `app.rs`'s per-frame reconcile should do about the folder-watch
/// service, given the current settings snapshot, the snapshot the running
/// service (if any) was last started/restarted with, and whether a service
/// is currently running at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileAction {
    /// Nothing to do — either already running with this exact snapshot, or
    /// already stopped and still inactive.
    NoChange,
    /// No service is running; start one.
    Start,
    /// A service is running but its snapshot is stale; stop it and start a
    /// fresh one with the current snapshot.
    Restart,
    /// A service is running but the snapshot is no longer active (disabled,
    /// or a required directory was cleared); stop it.
    Stop,
}

/// The pure reconcile decision (design spec §3.2's "rebuild-on-settings-
/// change" rule), factored out of `app.rs` so the enabled/disabled/snapshot-
/// change matrix is directly unit-testable without a real watcher, queue, or
/// store. `cached` is the snapshot the running service (if any) was last
/// started with; `running` is whether `app.rs`'s own `Option<
/// FolderWatchService>` is currently `Some` (kept as a separate bool rather
/// than inferred from `cached.is_some()`, since a `start()` failure leaves
/// `cached` cleared but the caller may still want to retry on the very next
/// frame with an unchanged snapshot — see [`ReconcileAction::Start`]'s own
/// "not running" arm below, which fires whenever `!running` regardless of
/// whether `cached` happens to equal `current`).
#[must_use]
pub fn reconcile_folder_watch(
    current: &FolderWatchSnapshot,
    cached: Option<&FolderWatchSnapshot>,
    running: bool,
) -> ReconcileAction {
    if !current.is_active() {
        return if running {
            ReconcileAction::Stop
        } else {
            ReconcileAction::NoChange
        };
    }
    if !running {
        return ReconcileAction::Start;
    }
    match cached {
        Some(cached) if cached == current => ReconcileAction::NoChange,
        _ => ReconcileAction::Restart,
    }
}

/// One message the worker thread's channel carries: either a filesystem
/// event notice (any kind — see [`FolderWatchService::start`]'s doc for why
/// kinds are never filtered; the payload itself is logged, if it's an error,
/// right where it's received and never carried further, since nothing
/// downstream branches on it) or a shutdown request.
enum WorkerMsg {
    /// Something changed somewhere under the watched tree (or `notify`
    /// itself reported an internal error, e.g. an overflow) — either way,
    /// the worker treats it as "rescan".
    Fs,
    /// [`FolderWatchService::stop`] was called — the worker must return as
    /// soon as it observes this, without starting a new rescan.
    Shutdown,
}

/// The running folder-watch service: owns the `notify` watcher (so its watch
/// subscription lives exactly as long as this value does) and the worker
/// thread draining its events.
pub struct FolderWatchService {
    /// Kept alive so the OS-level watch subscription isn't torn down early —
    /// dropping this is the first step of [`Self::stop`]. A trait object
    /// (rather than the concrete [`RecommendedWatcher`]) solely so
    /// [`FolderWatchService::start_with_poll_watcher_for_test`] can store a
    /// [`notify::PollWatcher`] here too, sharing the rest of this type's
    /// logic verbatim — `notify::Watcher::new`/`kind` are declared `where
    /// Self: Sized`, so they're excluded from the trait's object-safety
    /// requirement and don't get in the way of storing either concrete
    /// watcher behind this one field.
    watcher: Box<dyn Watcher + Send>,
    /// Session-scoped pause flag (design spec §3.3's pill toggle) — mutating
    /// settings has nothing to do with pausing, so this lives only here, not
    /// in `settings.folder_watch`.
    paused: Arc<AtomicBool>,
    /// The worker's message channel: [`Self::stop`] sends
    /// [`WorkerMsg::Shutdown`] on this to wake a worker that might otherwise
    /// be blocked waiting for the next filesystem event; [`Self::pause`]
    /// sends a synthetic [`WorkerMsg::Fs`] on it when *resuming*, so
    /// anything that changed while paused is picked up immediately rather
    /// than waiting for the next unrelated filesystem event to happen to
    /// fire.
    worker_tx: mpsc::Sender<WorkerMsg>,
    /// The worker thread, joined by [`Self::stop`].
    worker: JoinHandle<()>,
}

impl FolderWatchService {
    /// Starts watching `snapshot.input_dir` (recursively) and returns the
    /// running service, or the `notify` error if the watcher itself
    /// couldn't be constructed or subscribed (e.g. the directory doesn't
    /// exist). Callers must only call this when `snapshot.is_active()` is
    /// `true` (`app.rs`'s reconcile function is the sole caller and upholds
    /// this); it panics via `expect` otherwise, since a snapshot with no
    /// input/output directory has nothing meaningful to watch.
    ///
    /// Spawns the worker thread, which performs one initial scan of
    /// whatever's already in `snapshot.input_dir` before waiting on the
    /// first filesystem event (Buzz's own `find_tasks()` call right after
    /// constructing the watcher, `main_window.py:172`) — asynchronously:
    /// this function itself returns as soon as the thread is spawned, not
    /// once that scan has run, so a caller cannot assume every already-
    /// present eligible file has been enqueued the instant this returns
    /// (the queue's own `Queued` event, or the store, is the thing to poll
    /// if that matters).
    ///
    /// `ctx` is cloned into the worker thread and used only for
    /// `request_repaint()` after a batch of candidates is enqueued, so the
    /// task table redraws promptly instead of waiting for the next
    /// naturally-scheduled frame (`egui::Context` is `Send + Sync + Clone`
    /// by design for exactly this cross-thread use).
    ///
    /// `notify`'s event granularity varies by backend (FSEvents on macOS
    /// coalesces and can report parent-directory-level events rather than
    /// precise per-file kinds) — this service deliberately never inspects
    /// `Event::kind` to decide *whether* to rescan; every event (any kind,
    /// even a `notify`-internal `Err`) triggers a rescan, exactly mirroring
    /// Buzz's `directoryChanged` handler, which also carries no kind
    /// information and just calls `find_tasks()` unconditionally (`:36`).
    pub fn start(
        snapshot: FolderWatchSnapshot,
        queue: Arc<QueueHandle>,
        store: Arc<SqliteStore>,
        ctx: egui::Context,
    ) -> Result<FolderWatchService, notify::Error> {
        Self::start_with::<RecommendedWatcher>(snapshot, queue, store, ctx, Config::default())
    }

    /// Test-only seam: identical to [`Self::start`], except the OS watcher
    /// is a [`notify::PollWatcher`] rather than the platform's
    /// [`RecommendedWatcher`] (FSEvents on macOS). Exists because FSEvents
    /// delivery proved unreliable inside this repository's sandboxed test
    /// environment (confirmed with a minimal standalone `notify` probe, not
    /// a bug in this module's own event handling — a plain `notify::
    /// recommended_watcher` closure over a freshly created temp directory
    /// never received a single event there either); polling has no such
    /// dependency on OS-level FS-event delivery, at the cost of
    /// `poll_interval`-bounded latency instead of near-instant notification.
    /// The production path (`Self::start`) is untouched — this is purely an
    /// integration-test seam (`tests/folder_watch_int.rs`), the same "plain
    /// `pub fn`, not `#[cfg(test)]`" pattern `app.rs`'s
    /// `inject_event_for_test`/`open_options_sheet_for_test` already
    /// establish (an integration test links the library's normal,
    /// non-`cfg(test)` build, so a `#[cfg(test)]` item wouldn't even be
    /// visible to it).
    pub fn start_with_poll_watcher_for_test(
        snapshot: FolderWatchSnapshot,
        queue: Arc<QueueHandle>,
        store: Arc<SqliteStore>,
        ctx: egui::Context,
        poll_interval: std::time::Duration,
    ) -> Result<FolderWatchService, notify::Error> {
        Self::start_with::<notify::PollWatcher>(
            snapshot,
            queue,
            store,
            ctx,
            Config::default().with_poll_interval(poll_interval),
        )
    }

    /// Shared body of [`Self::start`]/[`Self::start_with_poll_watcher_for_test`]:
    /// generic only over which concrete [`Watcher`] backend constructs and
    /// watches `snapshot.input_dir`, boxed into [`Self::watcher`] once
    /// constructed so the rest of this type never needs to know which one it
    /// is. Callers must only call this when `snapshot.is_active()` is `true`
    /// (both public entry points' callers uphold this); it panics via
    /// `expect` otherwise, since a snapshot with no input/output directory
    /// has nothing meaningful to watch.
    ///
    /// Spawns the worker thread, which performs one initial scan of
    /// whatever's already in `snapshot.input_dir` before waiting on the
    /// first filesystem event (Buzz's own `find_tasks()` call right after
    /// constructing the watcher, `main_window.py:172`) — asynchronously:
    /// this function itself returns as soon as the thread is spawned, not
    /// once that scan has run, so a caller cannot assume every already-
    /// present eligible file has been enqueued the instant this returns
    /// (the queue's own `Queued` event, or the store, is the thing to poll
    /// if that matters).
    ///
    /// `ctx` is cloned into the worker thread and used only for
    /// `request_repaint()` after a batch of candidates is enqueued, so the
    /// task table redraws promptly instead of waiting for the next
    /// naturally-scheduled frame (`egui::Context` is `Send + Sync + Clone`
    /// by design for exactly this cross-thread use).
    fn start_with<W: Watcher + Send + 'static>(
        snapshot: FolderWatchSnapshot,
        queue: Arc<QueueHandle>,
        store: Arc<SqliteStore>,
        ctx: egui::Context,
        config: Config,
    ) -> Result<FolderWatchService, notify::Error> {
        assert!(
            snapshot.is_active(),
            "FolderWatchService::start called with an inactive snapshot"
        );
        let input_dir = snapshot
            .input_dir
            .clone()
            .expect("is_active() guarantees input_dir is Some");
        let output_root = snapshot
            .output_dir
            .clone()
            .expect("is_active() guarantees output_dir is Some");
        let disposition = if snapshot.delete_processed_files {
            SourceDisposition::Delete
        } else {
            SourceDisposition::MoveToOutput
        };
        let options = snapshot.options;

        let (tx, rx) = mpsc::channel::<WorkerMsg>();
        let event_tx = tx.clone();
        let mut watcher: W = Watcher::new(
            move |event: notify::Result<notify::Event>| {
                if let Err(err) = event {
                    log::warn!(
                        "folder-watch: notify reported an error ({err}) — rescanning anyway"
                    );
                }
                let _ = event_tx.send(WorkerMsg::Fs);
            },
            config,
        )?;
        watcher.watch(&input_dir, RecursiveMode::Recursive)?;

        let paused = Arc::new(AtomicBool::new(false));
        let worker_paused = Arc::clone(&paused);
        let worker = std::thread::spawn(move || {
            run_worker(WorkerCtx {
                input_dir,
                output_root,
                options,
                disposition,
                queue,
                store,
                ctx,
                paused: worker_paused,
                rx,
            });
        });

        Ok(FolderWatchService {
            watcher: Box::new(watcher),
            paused,
            worker_tx: tx,
            worker,
        })
    }

    /// Pauses (`true`) or resumes (`false`) event handling — the pill's
    /// toggle button. Paused, the worker still drains incoming events (so
    /// they never pile up unboundedly) but performs no rescan; the
    /// [`SeenSet`] is untouched either way, so resuming never re-emits a file
    /// that was already scanned before pausing.
    ///
    /// Resuming (a `true` → `false` transition) additionally nudges the
    /// worker with a synthetic nudge equivalent to a filesystem event, so
    /// anything that arrived *while* paused is picked up immediately —
    /// without this, a file dropped during a pause would only surface
    /// whenever some unrelated later filesystem event happened to fire next,
    /// which would make "resume" feel broken.
    pub fn pause(&self, paused: bool) {
        let was_paused = self.paused.swap(paused, Ordering::Relaxed);
        if was_paused && !paused {
            let _ = self.worker_tx.send(WorkerMsg::Fs);
        }
    }

    /// Whether the service is currently paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Stops watching and joins the worker thread. Bounded: the watcher is
    /// dropped first (so no further events arrive), then a
    /// [`WorkerMsg::Shutdown`] wakes a worker that might be blocked in
    /// `recv`, then the thread is joined — the worker's own loop checks for
    /// `Shutdown` both at the blocking `recv` and while draining a batch, so
    /// it always returns promptly rather than this being an unbounded join
    /// on a stuck loop.
    pub fn stop(self) {
        let FolderWatchService {
            watcher,
            worker_tx,
            worker,
            ..
        } = self;
        drop(watcher);
        let _ = worker_tx.send(WorkerMsg::Shutdown);
        if worker.join().is_err() {
            log::warn!("folder-watch worker thread panicked during shutdown");
        }
    }
}

/// Everything [`run_worker`] needs, owned by the worker thread for its whole
/// lifetime — bundled into one struct purely to keep [`FolderWatchService::start`]'s
/// `thread::spawn` closure from becoming an unreadable wall of captures.
struct WorkerCtx {
    input_dir: PathBuf,
    output_root: PathBuf,
    options: FileTranscriberSettings,
    disposition: SourceDisposition,
    queue: Arc<QueueHandle>,
    store: Arc<SqliteStore>,
    ctx: egui::Context,
    paused: Arc<AtomicBool>,
    rx: mpsc::Receiver<WorkerMsg>,
}

/// The worker thread's body: one initial scan (skipped if paused — see
/// below), then drain-all-pending-then-rescan on every subsequent wakeup,
/// until [`WorkerMsg::Shutdown`] (or a disconnected channel — the same
/// thing, defensively) ends the loop.
fn run_worker(mut worker: WorkerCtx) {
    let mut seen = SeenSet::new();
    // Gated on `paused` at the moment this actually runs (not captured at
    // spawn time): `FolderWatchService::start` returns as soon as the thread
    // is *spawned*, not once this line has run, so a caller that pauses
    // immediately after `start()` (before this thread has even been
    // scheduled) must have that respected here too — otherwise this
    // unconditional initial scan could still enqueue whatever the caller
    // just tried to pause against, a real (if narrow) race under thread
    // scheduling delays, not merely a theoretical one (caught by this
    // module's own integration test under load).
    if !worker.paused.load(Ordering::Relaxed) {
        rescan_and_enqueue(&mut worker, &mut seen);
    }

    loop {
        match worker.rx.recv() {
            Err(_) | Ok(WorkerMsg::Shutdown) => return,
            Ok(WorkerMsg::Fs) => {
                // Drain every other message already queued (event
                // coalescing — Buzz's own equivalent is rescanning the whole
                // tree per `directoryChanged` signal regardless of how many
                // fired, `:53-60`), bailing out immediately on a shutdown
                // seen mid-drain rather than starting one more rescan.
                loop {
                    match worker.rx.try_recv() {
                        Ok(WorkerMsg::Shutdown) => return,
                        Ok(WorkerMsg::Fs) => continue,
                        Err(_) => break,
                    }
                }
                if worker.paused.load(Ordering::Relaxed) {
                    continue;
                }
                rescan_and_enqueue(&mut worker, &mut seen);
            }
        }
    }
}

/// One full rescan of `worker.input_dir`, enqueueing every new candidate and
/// recording it into `seen` immediately after its `enqueue` call (Buzz
/// records into `paths_emitted` only after actually emitting the task,
/// `:120-123`) — repaints once at the end iff at least one candidate was
/// enqueued, rather than once per candidate.
fn rescan_and_enqueue(worker: &mut WorkerCtx, seen: &mut SeenSet) {
    let store = &worker.store;
    let known = |path: &Path| match store.has_task_for_file(path) {
        Ok(known) => known,
        Err(err) => {
            log::warn!(
                "folder-watch: has_task_for_file failed for {} ({err}) — treating as not known",
                path.display()
            );
            false
        }
    };
    let candidates = folderwatch::scan(&worker.input_dir, seen, &known);
    if candidates.is_empty() {
        return;
    }

    let mut enqueued_any = false;
    for candidate in candidates {
        let mirrored =
            folderwatch::mirrored_output_dir(&worker.output_root, &candidate.relative_dir);
        if let Err(err) = std::fs::create_dir_all(&mirrored) {
            // Non-fatal: the pipeline's own export-writing step performs the
            // identical best-effort `create_dir_all` right before writing
            // (`pipeline.rs`), so this failure only means the task starts
            // without a guaranteed-to-exist output directory yet, not that
            // the export is doomed.
            log::warn!(
                "folder-watch: could not create output dir {} ({err})",
                mirrored.display()
            );
        }

        let spec = spec_from_transcriber_settings(
            &worker.options,
            true, // custom_model_available: see this module's doc — folder
            // watch intentionally never gates a stale "Custom" model
            // selection back to "Small" the way the live options sheet
            // does; that gate exists only to keep a picker's displayed
            // value from silently pointing at an unlisted entry, which
            // has no analogue for a headless auto-task. A cleared
            // `custom_model_url` still degrades gracefully: the engine
            // factory fails to resolve the model and the task fails
            // actionably, same as any other missing-model case.
            candidate.path.clone(),
            mirrored,
            worker.disposition,
        );
        worker
            .queue
            .enqueue(TranscriptionId::new(), spec, Source::FolderWatch);
        enqueued_any = true;

        match std::fs::metadata(&candidate.path) {
            Ok(metadata) => {
                if let Ok(mtime) = metadata.modified() {
                    seen.record(candidate.path.clone(), metadata.len(), mtime);
                } else {
                    log::debug!(
                        "folder-watch: mtime unavailable for {} after enqueue — it may re-emit \
                         on the next rescan",
                        candidate.path.display()
                    );
                }
            }
            Err(err) => log::debug!(
                "folder-watch: could not re-stat {} after enqueue ({err}) — it may re-emit on \
                 the next rescan",
                candidate.path.display()
            ),
        }
    }

    if enqueued_any {
        worker.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn settings(enabled: bool, input: Option<&str>, output: Option<&str>) -> FolderWatchSettings {
        FolderWatchSettings {
            enabled,
            input_dir: input.map(PathBuf::from),
            output_dir: output.map(PathBuf::from),
            delete_processed_files: false,
            options: FileTranscriberSettings::default(),
        }
    }

    // -- FolderWatchSnapshot::is_active / pill_folder_name -------------------

    #[test]
    fn inactive_when_disabled() {
        let snap = FolderWatchSnapshot::from_settings(&settings(false, Some("/in"), Some("/out")));
        assert!(!snap.is_active());
        assert_eq!(snap.pill_folder_name(), None);
    }

    #[test]
    fn inactive_when_input_dir_missing() {
        let snap = FolderWatchSnapshot::from_settings(&settings(true, None, Some("/out")));
        assert!(!snap.is_active());
        assert_eq!(snap.pill_folder_name(), None);
    }

    #[test]
    fn inactive_when_output_dir_missing() {
        let snap = FolderWatchSnapshot::from_settings(&settings(true, Some("/in"), None));
        assert!(!snap.is_active());
        assert_eq!(snap.pill_folder_name(), None);
    }

    #[test]
    fn active_when_enabled_with_both_dirs() {
        let snap = FolderWatchSnapshot::from_settings(&settings(
            true,
            Some("/watched/my-clips"),
            Some("/out"),
        ));
        assert!(snap.is_active());
        assert_eq!(snap.pill_folder_name(), Some("my-clips".to_string()));
    }

    // -- reconcile_folder_watch matrix ----------------------------------------

    #[test]
    fn inactive_and_not_running_is_no_change() {
        let snap = FolderWatchSnapshot::from_settings(&settings(false, None, None));
        assert_eq!(
            reconcile_folder_watch(&snap, None, false),
            ReconcileAction::NoChange
        );
    }

    #[test]
    fn inactive_but_running_stops() {
        let snap = FolderWatchSnapshot::from_settings(&settings(false, None, None));
        let cached = FolderWatchSnapshot::from_settings(&settings(true, Some("/in"), Some("/out")));
        assert_eq!(
            reconcile_folder_watch(&snap, Some(&cached), true),
            ReconcileAction::Stop
        );
    }

    #[test]
    fn active_and_not_running_starts() {
        let snap = FolderWatchSnapshot::from_settings(&settings(true, Some("/in"), Some("/out")));
        assert_eq!(
            reconcile_folder_watch(&snap, None, false),
            ReconcileAction::Start
        );
    }

    #[test]
    fn active_running_unchanged_snapshot_is_no_change() {
        let snap = FolderWatchSnapshot::from_settings(&settings(true, Some("/in"), Some("/out")));
        assert_eq!(
            reconcile_folder_watch(&snap, Some(&snap), true),
            ReconcileAction::NoChange
        );
    }

    #[test]
    fn active_running_changed_snapshot_restarts() {
        let current =
            FolderWatchSnapshot::from_settings(&settings(true, Some("/in"), Some("/out")));
        let cached =
            FolderWatchSnapshot::from_settings(&settings(true, Some("/in2"), Some("/out")));
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), true),
            ReconcileAction::Restart
        );
    }

    #[test]
    fn active_but_not_yet_running_starts_even_if_cached_matches() {
        // A prior `start()` failure clears `cached` in `app.rs`'s own
        // bookkeeping (see this module's doc), but defensively: even if a
        // caller somehow still has a matching `cached` snapshot while
        // `running` is `false`, the right thing is always to (re)try
        // starting, never to silently do nothing.
        let snap = FolderWatchSnapshot::from_settings(&settings(true, Some("/in"), Some("/out")));
        assert_eq!(
            reconcile_folder_watch(&snap, Some(&snap), false),
            ReconcileAction::Start
        );
    }
}
