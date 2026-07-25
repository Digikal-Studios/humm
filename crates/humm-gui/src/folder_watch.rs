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
//! ## Watch identity vs. job template
//!
//! A `[folder_watch]` snapshot splits into two halves with very different
//! restart costs: `input_dir` is the watch's *identity* — changing it means
//! tearing down and rebuilding the OS-level subscription (FSEvents/
//! PollWatcher), since that's what the watcher was actually told to watch —
//! while `output_dir`/`delete_processed_files`/`options`/
//! `default_export_template` are a **job template** the running worker
//! thread reads fresh on every rescan via shared state ([`JobTemplate`],
//! `Arc<Mutex<_>>`) and [`FolderWatchService::update_template`] can swap out
//! without touching the watcher at all. This matters because Task 15 lands
//! a live-editable options form for `folder_watch.options` — without the
//! split, every keystroke committing a field would restart the FSEvents
//! subscription; with it, only an actual `input_dir` change (or the feature
//! being turned on/off) ever does. The same split is why
//! `default_export_template` (a *global* Preferences → General setting, not
//! part of `[folder_watch]` at all) is threaded through as its own
//! [`FolderWatchSnapshot`] field rather than folded into `options`: an edit
//! there must also resolve to `UpdateTemplate`, never `Restart`.
//! [`reconcile_folder_watch`] is the pure decision function choosing between
//! `Restart` (identity changed) and `UpdateTemplate` (only the template did).
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
//! a blank slate. A `stopping` flag (distinct from `paused`) is observed
//! before the initial scan and between every already-found candidate inside a
//! rescan, so [`FolderWatchService::stop`]'s join is bounded by, at worst, one
//! full tree walk (this is not "instant even mid-scan": [`humm_core::
//! folderwatch::scan`]'s own recursive walk — and its per-file
//! `has_task_for_file` check — is not itself interruptible, and `stopping`
//! is only consulted once that walk has already returned its candidate
//! list) plus whatever per-candidate work was still left to do. This matters
//! most for `Restart`, whose caller (`app.rs`, on the UI thread) calls
//! `stop()` synchronously: a very large watched tree means a visible-but-
//! bounded hitch on that click, accepted for M7 — revisit if a real
//! directory this large in practice makes that hitch noticeable.
//!
//! ## `SeenSet` lifetime
//!
//! The [`SeenSet`] (Buzz's `paths_emitted`) is **not** owned by this service —
//! it's passed in as an `Arc<Mutex<SeenSet>>` and shared across every
//! `start()` for the process's whole lifetime (`app.rs`'s `HummApp` owns the
//! one instance, constructed once). Buzz's own `paths_emitted` lives on the
//! single `TranscriptionTaskFolderWatcher` object `MainWindow` constructs
//! once and reuses across every `set_preferences` call — only a full app
//! restart clears it. Giving each `start()` a *fresh* `SeenSet` instead (an
//! earlier revision of this module did exactly that) opens a real race: a
//! rescan's `queue.enqueue()` is fire-and-forget (the runner's own
//! `create_task` may not have landed in the store yet), so a Preferences edit
//! landing in that narrow window could restart the watcher with a blank seen
//! set and a store that still doesn't (yet) know about the file just
//! enqueued — double-enqueueing it. Sharing the *same* `SeenSet` across
//! restarts closes that window unconditionally: rule 6 (`scan`'s "seen at
//! this exact size/mtime") fires regardless of whether the store has caught
//! up. Proven end to end by `tests/folder_watch_int.rs::
//! seen_set_survives_a_rebuild_so_a_recently_enqueued_file_is_not_double_enqueued`.
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

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
/// filesystem more than once per frame (building one is a small, bounded
/// clone of a handful of `String`/`Option<String>`/`Vec<String>` fields,
/// taken under the settings lock — not literally free, but cheap enough to
/// redo unconditionally every frame).
#[derive(Debug, Clone, PartialEq)]
pub struct FolderWatchSnapshot {
    /// Whether folder-watching is turned on at all.
    pub enabled: bool,
    /// The directory scanned for new media files (Buzz's `input_folder`) —
    /// the watch's *identity* (see this module's doc): changing this is the
    /// only settings change that restarts the OS-level subscription.
    pub input_dir: Option<PathBuf>,
    /// The directory mirrored transcription outputs are written under
    /// (Buzz's `output_directory`) — part of the job **template**, updated
    /// in place without a restart.
    pub output_dir: Option<PathBuf>,
    /// Delete (`true`) vs. move-into-output (`false`) processed sources
    /// (design spec §3.4) — part of the job template.
    pub delete_processed_files: bool,
    /// Per-auto-task file-transcription options (Buzz's
    /// `file_transcription_options` sub-group) — part of the job template.
    pub options: FileTranscriberSettings,
    /// The **global** default export-file-name template
    /// (`settings.file_transcriber.default_export_template`, Preferences →
    /// General — design spec §8.1) — part of the job template, review-fixed
    /// (M7 Task 11 follow-up): the earlier wiring read
    /// `options.default_export_template` (`folder_watch.options`'s own
    /// field), which nothing in the UI ever writes — folder-watch tasks
    /// share the one global template every other spec-build site
    /// (options-sheet, CLI, viewer export) reads, not a per-watch override.
    /// Living here (not on `options`) keeps it correctly on the *template*
    /// half of the watch-identity/job-template split: a Preferences → General
    /// edit changes only this field, so [`reconcile_folder_watch`] resolves
    /// `ReconcileAction::UpdateTemplate`, never `Restart` (`options.
    /// default_export_template` participating in equality would have had
    /// the same effect, but reading the *global* setting through the wrong
    /// field is exactly the bug this fixes).
    pub default_export_template: Option<String>,
}

impl FolderWatchSnapshot {
    /// Clones the live `[folder_watch]` settings into a snapshot.
    /// `default_export_template` is threaded in separately — the caller's
    /// `settings.file_transcriber.default_export_template` (the *global*
    /// setting; see this struct's own field doc for why it isn't read off
    /// `settings.options` instead).
    #[must_use]
    pub fn from_settings(
        settings: &FolderWatchSettings,
        default_export_template: Option<String>,
    ) -> Self {
        FolderWatchSnapshot {
            enabled: settings.enabled,
            input_dir: settings.input_dir.clone(),
            output_dir: settings.output_dir.clone(),
            delete_processed_files: settings.delete_processed_files,
            options: settings.options.clone(),
            default_export_template,
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

    /// Whether `self` and `other` watch the same directory — the identity
    /// half of the watch-identity/job-template split (this module's doc).
    /// Only meaningful when both are already known [`Self::is_active`]
    /// (callers only ever reach for this once `reconcile_folder_watch` has
    /// already established both are active and a service is running).
    #[must_use]
    fn same_watch_identity(&self, other: &Self) -> bool {
        self.input_dir == other.input_dir
    }

    /// The job-template half of this snapshot, for
    /// [`FolderWatchService::start`]/[`FolderWatchService::update_template`].
    /// Panics via `expect` if `output_dir` is `None` — callers must only call
    /// this once [`Self::is_active`] is known `true`.
    #[must_use]
    fn template(&self) -> JobTemplate {
        JobTemplate {
            output_dir: self
                .output_dir
                .clone()
                .expect("is_active() guarantees output_dir is Some"),
            delete_processed_files: self.delete_processed_files,
            options: self.options.clone(),
            default_export_template: self.default_export_template.clone(),
        }
    }
}

/// The job-template half of a [`FolderWatchSnapshot`] (this module's doc):
/// everything the running worker thread needs to build a `TaskSpec` for a
/// new candidate, *except* which directory it's watching. Shared with a
/// running service via `Arc<Mutex<JobTemplate>>` so
/// [`FolderWatchService::update_template`] can swap it in without touching
/// the OS-level watch subscription at all.
#[derive(Debug, Clone, PartialEq)]
struct JobTemplate {
    output_dir: PathBuf,
    delete_processed_files: bool,
    options: FileTranscriberSettings,
    /// The global default export template — see
    /// [`FolderWatchSnapshot::default_export_template`]'s doc; threaded into
    /// [`crate::views::options_sheet::spec_from_transcriber_settings`] as an
    /// explicit override, not read off `options`.
    default_export_template: Option<String>,
}

/// What `app.rs`'s per-frame reconcile should do about the folder-watch
/// service, given the current settings snapshot, the snapshot the running
/// service (if any) was last started/restarted/updated with, and whether a
/// service is currently running at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileAction {
    /// Nothing to do — either already running with this exact snapshot, or
    /// already stopped/failed with this exact (inactive, or unchanged)
    /// snapshot already cached, so retrying every frame would just spin.
    NoChange,
    /// No service is running; start one.
    Start,
    /// A service is running but the watch *identity* (`input_dir`) changed;
    /// stop it and start a fresh one with the current snapshot.
    Restart,
    /// A service is running and only the job *template*
    /// (`output_dir`/`delete_processed_files`/`options`/
    /// `default_export_template`) changed —
    /// [`FolderWatchService::update_template`] in place, no restart.
    UpdateTemplate,
    /// A service is running but the snapshot is no longer active (disabled,
    /// or a required directory was cleared); stop it.
    Stop,
}

/// The pure reconcile decision (design spec §3.2's "rebuild-on-settings-
/// change" rule, refined by this module's watch-identity/job-template
/// split), factored out of `app.rs` so the whole enabled/disabled/identity-
/// change/template-change matrix is directly unit-testable without a real
/// watcher, queue, or store.
///
/// `cached` is the snapshot `app.rs` last *reacted to* — not only while a
/// service is running: `app.rs` caches a `Stop`'s snapshot and a failed
/// `Start`/`Restart`'s snapshot too (see `app.rs`'s
/// `HummApp::sync_folder_watch` doc), specifically so this function can
/// answer "nothing changed since we last handled this" (`NoChange`) even
/// while `running` is `false` — without that, a `start()` failure (e.g. a
/// bad path) would retry, and warn, on every single frame at UI frame rate
/// until the user fixes it. `running` is whether `app.rs`'s own
/// `Option<FolderWatchService>` is currently `Some`.
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
    match cached {
        Some(cached) if cached == current => ReconcileAction::NoChange,
        Some(cached) if running && cached.same_watch_identity(current) => {
            ReconcileAction::UpdateTemplate
        }
        _ if running => ReconcileAction::Restart,
        _ => ReconcileAction::Start,
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
    /// Set by [`Self::stop`] before anything else, so the worker's initial
    /// scan (if not yet run) and the per-candidate loop inside a rescan
    /// already in flight both notice and bail out promptly — see this
    /// module's "Threading model" doc.
    stopping: Arc<AtomicBool>,
    /// The job-template half of the running snapshot (this module's doc) —
    /// [`Self::update_template`] swaps it in place; the worker reads a fresh
    /// clone at the start of every rescan.
    template: Arc<Mutex<JobTemplate>>,
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
    /// `seen` is the `SeenSet` shared across every `start()` for this
    /// process's whole lifetime (see this module's "`SeenSet` lifetime"
    /// doc) — `app.rs` constructs it once and passes the same `Arc` every
    /// time.
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
        seen: Arc<Mutex<SeenSet>>,
    ) -> Result<FolderWatchService, notify::Error> {
        Self::start_with::<RecommendedWatcher>(
            snapshot,
            queue,
            store,
            ctx,
            seen,
            false,
            Config::default(),
        )
    }

    /// Test-only seam: identical to [`Self::start`], except the OS watcher
    /// is a [`notify::PollWatcher`] rather than the platform's
    /// [`RecommendedWatcher`] (FSEvents on macOS). Exists because FSEvents
    /// delivery proved unreliable inside this repository's sandboxed test
    /// environment (confirmed with a minimal standalone `notify` probe, not
    /// a bug in this module's own event handling — a plain `notify::
    /// recommended_watcher` closure over a freshly created temp directory
    /// never received a single event there either; the production path was
    /// separately confirmed to work correctly unsandboxed); polling has no
    /// such dependency on OS-level FS-event delivery, at the cost of
    /// `poll_interval`-bounded latency instead of near-instant notification.
    /// The production path (`Self::start`) is untouched — this is purely an
    /// integration-test seam (`tests/folder_watch_int.rs`), the same "plain
    /// `pub fn`, not `#[cfg(test)]`" pattern `app.rs`'s
    /// `inject_event_for_test`/`open_options_sheet_for_test` already
    /// establish (an integration test links the library's normal,
    /// non-`cfg(test)` build, so a `#[cfg(test)]` item wouldn't even be
    /// visible to it).
    ///
    /// `initially_paused` starts the service already paused, set atomically
    /// at construction (no window at all between "started" and "paused") —
    /// closes the pause race deterministically for a test that wants to
    /// assert a drop is ignored from the very first instant, rather than
    /// calling `start` then immediately `pause(true)` and hoping the worker
    /// thread hasn't already scheduled and scanned in between (the runtime
    /// paused-check inside the worker, documented on [`Self::pause`],
    /// remains the general-purpose safety net either way).
    pub fn start_with_poll_watcher_for_test(
        snapshot: FolderWatchSnapshot,
        queue: Arc<QueueHandle>,
        store: Arc<SqliteStore>,
        ctx: egui::Context,
        seen: Arc<Mutex<SeenSet>>,
        initially_paused: bool,
        poll_interval: Duration,
    ) -> Result<FolderWatchService, notify::Error> {
        Self::start_with::<notify::PollWatcher>(
            snapshot,
            queue,
            store,
            ctx,
            seen,
            initially_paused,
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
    fn start_with<W: Watcher + Send + 'static>(
        snapshot: FolderWatchSnapshot,
        queue: Arc<QueueHandle>,
        store: Arc<SqliteStore>,
        ctx: egui::Context,
        seen: Arc<Mutex<SeenSet>>,
        initially_paused: bool,
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
        let template = Arc::new(Mutex::new(snapshot.template()));

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

        let paused = Arc::new(AtomicBool::new(initially_paused));
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_paused = Arc::clone(&paused);
        let worker_stopping = Arc::clone(&stopping);
        let worker_template = Arc::clone(&template);
        let worker = std::thread::spawn(move || {
            run_worker(WorkerCtx {
                input_dir,
                template: worker_template,
                queue,
                store,
                ctx,
                paused: worker_paused,
                stopping: worker_stopping,
                seen,
                rx,
            });
        });

        Ok(FolderWatchService {
            watcher: Box::new(watcher),
            paused,
            stopping,
            template,
            worker_tx: tx,
            worker,
        })
    }

    /// Swaps in a new job template (`output_dir`/`delete_processed_files`/
    /// `options`/`default_export_template`) without touching the watcher at
    /// all — the `UpdateTemplate`
    /// half of this module's watch-identity/job-template split. `snapshot`
    /// must already be known active (`app.rs`'s reconcile function is the
    /// sole caller and upholds this).
    pub fn update_template(&self, snapshot: &FolderWatchSnapshot) {
        *self
            .template
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot.template();
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

    /// Stops watching and joins the worker thread. `stopping` is set first
    /// (so the worker's initial scan, if not yet run, and the per-candidate
    /// loop inside a rescan already in flight both notice and return
    /// promptly), then the watcher is dropped (so no further events arrive),
    /// then a [`WorkerMsg::Shutdown`] wakes a worker that might be blocked in
    /// `recv`, then the thread is joined. Bounded, but not instant even
    /// mid-scan: [`humm_core::folderwatch::scan`]'s own recursive walk (plus
    /// its per-file `has_task_for_file` check) is not itself interruptible,
    /// so a `stop()` landing while that walk is in progress still waits for
    /// it to finish before `stopping` is even consulted — see this module's
    /// "Threading model" doc for the accepted-for-M7 tradeoff that implies
    /// for `Restart` on the UI thread.
    pub fn stop(self) {
        let FolderWatchService {
            watcher,
            stopping,
            worker_tx,
            worker,
            ..
        } = self;
        stopping.store(true, Ordering::Relaxed);
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
    /// The watch's identity — fixed for this worker's whole lifetime (a
    /// change restarts the service entirely, see this module's doc).
    input_dir: PathBuf,
    /// The job template, shared and possibly updated in place by
    /// [`FolderWatchService::update_template`] between rescans.
    template: Arc<Mutex<JobTemplate>>,
    queue: Arc<QueueHandle>,
    store: Arc<SqliteStore>,
    ctx: egui::Context,
    paused: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
    /// Shared across every `start()` for the process's lifetime — see this
    /// module's "`SeenSet` lifetime" doc.
    seen: Arc<Mutex<SeenSet>>,
    rx: mpsc::Receiver<WorkerMsg>,
}

/// The worker thread's body: one initial scan (skipped if paused or already
/// stopping — see below), then drain-all-pending-then-rescan on every
/// subsequent wakeup, until [`WorkerMsg::Shutdown`] (or a disconnected
/// channel — the same thing, defensively) ends the loop.
fn run_worker(worker: WorkerCtx) {
    // Gated on `paused`/`stopping` at the moment this actually runs (not
    // captured at spawn time): `FolderWatchService::start` returns as soon
    // as the thread is *spawned*, not once this line has run, so a caller
    // that pauses (or immediately stops) right after `start()` — before this
    // thread has even been scheduled — must have that respected here too;
    // otherwise this initial scan could still enqueue whatever the caller
    // just tried to pause/stop against, a real (if narrow) race under thread
    // scheduling delays, not merely a theoretical one (caught by this
    // module's own integration test under load).
    if !worker.stopping.load(Ordering::Relaxed) && !worker.paused.load(Ordering::Relaxed) {
        rescan_and_enqueue(&worker);
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
                if worker.stopping.load(Ordering::Relaxed) {
                    return;
                }
                if worker.paused.load(Ordering::Relaxed) {
                    continue;
                }
                rescan_and_enqueue(&worker);
            }
        }
    }
}

/// One full rescan of `worker.input_dir`, enqueueing every new candidate and
/// recording it into the shared `SeenSet` immediately after its `enqueue`
/// call (Buzz records into `paths_emitted` only after actually emitting the
/// task, `:120-123`) — repaints once at the end iff at least one candidate
/// was enqueued, rather than once per candidate. `folderwatch::scan`'s own
/// recursive walk runs to completion uninterrupted (it isn't itself
/// `stopping`-aware); only the per-candidate work *after* that walk returns
/// bails out early once [`FolderWatchService::stop`] has set `stopping` in
/// the meantime — see this module's "Threading model" doc for what that
/// does and doesn't bound.
fn rescan_and_enqueue(worker: &WorkerCtx) {
    let template = worker
        .template
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
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

    let mut seen = worker
        .seen
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let candidates = folderwatch::scan(&worker.input_dir, &seen, &known);
    if candidates.is_empty() {
        return;
    }

    let disposition = if template.delete_processed_files {
        SourceDisposition::Delete
    } else {
        SourceDisposition::MoveToOutput
    };

    let mut enqueued_any = false;
    for candidate in candidates {
        if worker.stopping.load(Ordering::Relaxed) {
            break;
        }

        let mirrored =
            folderwatch::mirrored_output_dir(&template.output_dir, &candidate.relative_dir);
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
            &template.options,
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
            disposition,
            template.default_export_template.as_deref(),
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
    drop(seen);

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

    /// A [`FolderWatchSnapshot`] built the same way `app.rs`'s reconcile
    /// call site does — `settings(...)` for the `[folder_watch]` half, plus
    /// the *global* `default_export_template` (`None` for every test that
    /// isn't specifically exercising it, matching a fresh install).
    fn snapshot(
        enabled: bool,
        input: Option<&str>,
        output: Option<&str>,
        default_export_template: Option<&str>,
    ) -> FolderWatchSnapshot {
        FolderWatchSnapshot::from_settings(
            &settings(enabled, input, output),
            default_export_template.map(str::to_string),
        )
    }

    // -- FolderWatchSnapshot::is_active / pill_folder_name -------------------

    #[test]
    fn inactive_when_disabled() {
        let snap = snapshot(false, Some("/in"), Some("/out"), None);
        assert!(!snap.is_active());
        assert_eq!(snap.pill_folder_name(), None);
    }

    #[test]
    fn inactive_when_input_dir_missing() {
        let snap = snapshot(true, None, Some("/out"), None);
        assert!(!snap.is_active());
        assert_eq!(snap.pill_folder_name(), None);
    }

    #[test]
    fn inactive_when_output_dir_missing() {
        let snap = snapshot(true, Some("/in"), None, None);
        assert!(!snap.is_active());
        assert_eq!(snap.pill_folder_name(), None);
    }

    #[test]
    fn active_when_enabled_with_both_dirs() {
        let snap = snapshot(true, Some("/watched/my-clips"), Some("/out"), None);
        assert!(snap.is_active());
        assert_eq!(snap.pill_folder_name(), Some("my-clips".to_string()));
    }

    /// M7 Task 11 review fix (F-100/F-030): the **global**
    /// `file_transcriber.default_export_template` — not `folder_watch.
    /// options.default_export_template` (a decoy field nothing in the UI
    /// writes, the original, inert wiring's bug) — survives
    /// `FolderWatchSnapshot::from_settings` → `FolderWatchSnapshot::template`
    /// → `spec_from_transcriber_settings`'s explicit override parameter,
    /// end to end to the produced `TaskSpec`. This is the flow
    /// `rescan_and_enqueue` relies on to hand a watched file's task the same
    /// global template every other spec-build site uses (design spec §8.1).
    #[test]
    fn template_carries_the_default_export_template_setting() {
        let global_template = "{{ input_file_name }}-watched";
        let snap = snapshot(true, Some("/in"), Some("/out"), Some(global_template));
        let template = snap.template();
        assert_eq!(
            template.default_export_template.as_deref(),
            Some(global_template)
        );

        let spec = spec_from_transcriber_settings(
            &template.options,
            true,
            PathBuf::from("/in/clip.wav"),
            template.output_dir.clone(),
            humm_core::domain::SourceDisposition::Delete,
            template.default_export_template.as_deref(),
        );
        assert_eq!(
            spec.output.file_name_template.as_deref(),
            Some(global_template)
        );
    }

    // -- reconcile_folder_watch matrix ----------------------------------------

    #[test]
    fn inactive_and_not_running_is_no_change() {
        let snap = snapshot(false, None, None, None);
        assert_eq!(
            reconcile_folder_watch(&snap, None, false),
            ReconcileAction::NoChange
        );
    }

    #[test]
    fn inactive_but_running_stops() {
        let snap = snapshot(false, None, None, None);
        let cached = snapshot(true, Some("/in"), Some("/out"), None);
        assert_eq!(
            reconcile_folder_watch(&snap, Some(&cached), true),
            ReconcileAction::Stop
        );
    }

    #[test]
    fn active_and_not_running_starts() {
        let snap = snapshot(true, Some("/in"), Some("/out"), None);
        assert_eq!(
            reconcile_folder_watch(&snap, None, false),
            ReconcileAction::Start
        );
    }

    #[test]
    fn active_running_unchanged_snapshot_is_no_change() {
        let snap = snapshot(true, Some("/in"), Some("/out"), None);
        assert_eq!(
            reconcile_folder_watch(&snap, Some(&snap), true),
            ReconcileAction::NoChange
        );
    }

    #[test]
    fn active_running_input_dir_change_restarts() {
        let current = snapshot(true, Some("/in"), Some("/out"), None);
        let cached = snapshot(true, Some("/in2"), Some("/out"), None);
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), true),
            ReconcileAction::Restart
        );
    }

    #[test]
    fn active_running_output_dir_change_updates_template_not_restart() {
        let current = snapshot(true, Some("/in"), Some("/out"), None);
        let cached = snapshot(true, Some("/in"), Some("/out2"), None);
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), true),
            ReconcileAction::UpdateTemplate
        );
    }

    #[test]
    fn active_running_delete_processed_files_change_updates_template_not_restart() {
        let current = snapshot(true, Some("/in"), Some("/out"), None);
        let mut cached = current.clone();
        cached.delete_processed_files = true;
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), true),
            ReconcileAction::UpdateTemplate
        );
    }

    #[test]
    fn active_running_options_change_updates_template_not_restart() {
        let current = snapshot(true, Some("/in"), Some("/out"), None);
        let mut cached = current.clone();
        cached.options.word_timestamps = Some(true);
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), true),
            ReconcileAction::UpdateTemplate
        );
    }

    /// Review-fix pin (M7 Task 11 follow-up): a Preferences → General edit
    /// to the *global* `file_transcriber.default_export_template` must
    /// reconcile as `UpdateTemplate`, never `Restart` — it's part of the job
    /// template (this module's doc / `FolderWatchSnapshot::
    /// default_export_template`'s own doc), not the watch identity, exactly
    /// like `output_dir`/`delete_processed_files`/`options` above.
    #[test]
    fn active_running_default_export_template_change_updates_template_not_restart() {
        let current = snapshot(
            true,
            Some("/in"),
            Some("/out"),
            Some("{{ input_file_name }}-new"),
        );
        let cached = snapshot(
            true,
            Some("/in"),
            Some("/out"),
            Some("{{ input_file_name }}-old"),
        );
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), true),
            ReconcileAction::UpdateTemplate
        );
    }

    #[test]
    fn disabled_to_enabled_transition_starts_not_restarts() {
        // Going from inactive (not running) to active is always `Start`,
        // never `Restart` — there's no watcher to tear down yet.
        let current = snapshot(true, Some("/in"), Some("/out"), None);
        let cached = snapshot(false, Some("/in"), Some("/out"), None);
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), false),
            ReconcileAction::Start
        );
    }

    #[test]
    fn active_but_failed_previously_with_unchanged_snapshot_does_not_retry_every_frame() {
        // `app.rs` caches the snapshot it last reacted to even when a
        // `start()` attempt failed (`folder_watch` stays `None`, i.e.
        // `running` is `false`, but `folder_watch_snapshot` is still
        // `Some`) — specifically so an unchanged snapshot doesn't retry (and
        // re-warn) on every single frame at UI frame rate. Retrying only
        // happens once the snapshot actually changes again.
        let snap = snapshot(true, Some("/in"), Some("/out"), None);
        assert_eq!(
            reconcile_folder_watch(&snap, Some(&snap), false),
            ReconcileAction::NoChange
        );
    }

    #[test]
    fn active_not_running_with_changed_snapshot_retries() {
        // The counterpart to the no-spin case above: once the cached
        // (failed or stopped) snapshot no longer matches the fresh one, a
        // retry is exactly right.
        let current = snapshot(true, Some("/in"), Some("/out"), None);
        let cached = snapshot(true, Some("/in2"), Some("/out"), None);
        assert_eq!(
            reconcile_folder_watch(&current, Some(&cached), false),
            ReconcileAction::Start
        );
    }
}
