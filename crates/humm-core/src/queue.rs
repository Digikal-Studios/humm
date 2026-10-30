//! The task queue and event bus: a single [`Runner`] thread that owns a FIFO of
//! transcription tasks, runs them strictly one at a time, drives each through
//! the [`TaskStore`] status transitions, and broadcasts [`TaskEvent`]s to
//! subscribers (`docs/design/ARCHITECTURE.md` §3).
//!
//! Ported from Buzz's `FileTranscriberQueueWorker`
//! (`buzz/file_transcriber_queue_worker.py`): FIFO, one transcription at a
//! time (serialized execution is a feature — GPU contention), a *canceled-set*
//! that lets a task be canceled before it starts, and URL downloads that run
//! inside the task (`buzz/transcriber/file_transcriber.py:38-73`). Qt signals
//! become `flume` channels; Buzz's error-string sniffing for "canceled"
//! (`queue_worker.py:388-394`) becomes typed [`CoreError`] classification.
//!
//! # Concurrency invariants
//!
//! - **Single writer.** Only the runner thread touches [`TaskStore`]; it is the
//!   sole writer of a task's status (`ARCHITECTURE.md` §5).
//! - **Resizer- and Speaker-ID-created rows are the documented exception
//!   (ADR-018 scope).** The queue owns `status`/`progress` for tasks it
//!   *executes*. The transcript resizer (F-086, design spec §4 "status-write
//!   boundary note") and Speaker-ID's Save (F-087, `humm-gui`'s
//!   `speaker_id::save`) are the two features that write those columns
//!   outside the queue: both call the identical `copy_transcription` +
//!   `mark_replaced_completed` pair (the resizer's `update_progress_direct`
//!   use has no Speaker-ID analogue) to create and complete a row that never
//!   enters this queue at all. Still safe, for the same reason in both
//!   cases: each only ever writes a **freshly minted** id it just created via
//!   `copy_transcription` — never a queue-owned id already in flight — and
//!   `copy_transcription`'s own row always starts `Queued` before either
//!   caller immediately calls `mark_replaced_completed` on it, so the
//!   "single writer / queue owns status transitions" invariant stays scoped
//!   to queue-executed tasks and never actually conflicts with either
//!   feature.
//! - **Terminal status precedes the terminal event.** `mark_completed` /
//!   `mark_failed` / `mark_canceled` is written to the store *before* the
//!   matching `Completed`/`Failed`/`Canceled` event is emitted, so a consumer
//!   reacting to the event always sees a consistent store.
//! - **Panics are contained.** `run_job` runs inside `catch_unwind`: an
//!   engine/pipeline panic is mapped to task failure (never propagated as a
//!   panic — one bad task must not wedge the queue) and discards the cached
//!   engine, since an engine that panicked mid-run must not be reused.
//! - **Store-write failures never abort a run (ADR-018).** Every store write
//!   (`mark_started`/`mark_completed`/`mark_failed`/`mark_canceled`/
//!   `update_progress`/`update_file`) is logged *and* emits a
//!   [`TaskEventKind::StoreWriteFailed`] event on failure, so subscribers
//!   (CLI, GUI) can surface it — but the run continues regardless: finishing
//!   the transcription beats aborting on a DB hiccup, and a task's terminal
//!   state is decided by the pipeline outcome, not by whether the store write
//!   for it happened to succeed.
//! - **Two mutexes, never nested.** [`Runner`] and [`QueueHandle`] share a
//!   subscriber list and a "current task" slot, each behind its own `Mutex`.
//!   Every critical section locks exactly one of them and releases it before
//!   locking the other, so no lock-ordering deadlock is possible.
//! - **Model downloads are non-modal for the GUI (`docs/design/UX_SPEC.md`
//!   §J-1).** The CLI resolves a task's model synchronously *before*
//!   enqueueing (Buzz parity, `cli.py:219-221`) and never configures
//!   [`RunnerDeps::artifacts`], so this never applies to it. A GUI caller
//!   instead supplies `Some(Arc<dyn ArtifactEnsurer>)`: the runner ensures
//!   the model (and re-resolves the VAD model) as the task's *own* first
//!   phase, so an absent model downloads without blocking the rest of the
//!   queue, with progress visible per-task. The per-task event order becomes
//!   `Started` → [`DownloadProgress`* for a URL task's media fetch] →
//!   [`ModelDownloadProgress`* while an artifact download is in flight] →
//!   `Progress`/`SegmentAdded`* → terminal.
//! - **M8 pre-transcription chain (F-027/F-112/F-114, design spec §3/§5).**
//!   `Runner::run_job` gains a fixed-order isolation → noise-reduction →
//!   language-detection block between `check_skip` and artifact-ensure
//!   ([`RunnerDeps::isolator`]/[`RunnerDeps::reducer`]/[`RunnerDeps::lang_detector`],
//!   each `None` = that hook never runs). Detection decodes the file itself
//!   (to feed [`crate::boundary::LanguageDetector::detect`]), so the file
//!   decodes twice whenever detection actually runs — once there, once again
//!   inside [`run_file_job`] moments later. Buzz pays this same cost the same
//!   way (a separate `whisper-cli --detect-language` process ahead of the
//!   main transcription process).

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::boundary::{
    AudioDecoder, FetchProgress, LanguageDetector, MediaFetcher, MediaProber, NoiseReducer,
    SegmentSink, SpeechIsolator, TaskStore, TranscriptionEngine,
};
use crate::cancel::CancelToken;
use crate::domain::{
    Backend, Input, ModelSize, ModelSpec, Segment, Source, TaskSpec, TranscriptionId,
};
use crate::error::{CoreError, EngineError, MediaError, ModelError, StoreError};
use crate::language::Language;
use crate::models::download::DownloadProgress;
use crate::openai::LlmEndpointSource;
use crate::pipeline::{EngineTuning, JobOutcome, run_file_job};
use crate::postproc::{self, PostprocConfig, PostprocConfigSource};

/// The message emitted to yt-dlp-less URL tasks: exactly this string is
/// written to the store and carried on the `Failed` event. The install
/// example matches the host OS (Homebrew, apt, or winget).
fn ytdlp_missing_message() -> String {
    format!(
        "yt-dlp missing — {} or set HUMM_YTDLP",
        crate::platform::subprocess_install_hint(std::env::consts::OS, "yt-dlp")
    )
}

/// One observable moment in a task's lifecycle, broadcast to every subscriber.
///
/// The per-task order is `Queued` → (`Started` → [`DownloadProgress`* for a
/// URL task's media fetch] → [`ModelDownloadProgress`* when
/// [`RunnerDeps::artifacts`] is configured and an artifact needs fetching] →
/// `Progress`/`SegmentAdded`* → one of `Completed`/`Failed`) — unless the
/// task is canceled before it starts, in which case it is `Queued` →
/// `Canceled` with no `Started`. `StoreWriteFailed` is out of band: it may
/// interleave anywhere in that sequence (ADR-018) and never changes it.
/// [`QueueHandle::retry`] restarts this whole sequence for the same id: a
/// terminal event is followed by a fresh `Queued`, as though the id had
/// been enqueued again.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskEventKind {
    /// The task was accepted and persisted; it is waiting its turn.
    Queued,
    /// The task began running (its input is now being fetched/decoded).
    Started,
    /// Download progress as a fraction in `0.0..=1.0`. Emitted only for URL
    /// tasks and only while the total size is known (Buzz parity —
    /// `file_transcriber.py:172-174`).
    DownloadProgress(f32),
    /// Model/VAD artifact download progress for a task run with an
    /// [`ArtifactEnsurer`] configured (GUI only —
    /// [`RunnerDeps::artifacts`]). Emitted between `Started` and
    /// `Progress`/`SegmentAdded`, throttled across the whole artifact-ensure
    /// phase (mirrors `QueueSink::on_progress`'s store-write throttle, same
    /// file): a [`crate::models::download::DownloadProgress`] callback from
    /// [`ArtifactEnsurer::ensure_model`]/[`ArtifactEnsurer::ensure_vad`]
    /// that reports a known, nonzero `total` fires an event only the first
    /// time a new whole percent is reached; one that reports an unknown (or
    /// zero) `total` fires on the very first such callback, then again only
    /// once `downloaded` crosses into a new whole mebibyte (see
    /// [`ModelDownloadThrottle`]).
    ///
    /// The CLI never constructs an `ArtifactEnsurer` — it resolves the model
    /// synchronously before enqueueing (Buzz parity) and always leaves
    /// `RunnerDeps.artifacts` as `None` — so this variant never fires there;
    /// its exhaustive event matches handle it as unreachable-by-construction
    /// (render nothing, in both human and `--json` modes).
    ModelDownloadProgress {
        /// File name of the artifact this update describes.
        file: String,
        /// Bytes downloaded so far, including any resumed offset.
        downloaded: u64,
        /// Total artifact size in bytes, if known.
        total: Option<u64>,
    },
    /// Transcription progress as a fraction in `0.0..=1.0`.
    Progress(f32),
    /// A newly recognized (and text-trimmed) segment.
    SegmentAdded(Segment),
    /// The task finished successfully.
    Completed {
        /// Export files written, one per requested format, in format order.
        written: Vec<PathBuf>,
    },
    /// The task failed.
    Failed {
        /// A human-readable, actionable failure message.
        message: String,
    },
    /// The task was canceled (before or during its run).
    Canceled,
    /// A store write for this task failed (ADR-018): the run continues
    /// regardless — the task's terminal state is decided by the pipeline
    /// outcome, not by whether this write succeeded.
    StoreWriteFailed {
        /// The store method that failed, e.g. `"mark_started"`.
        operation: String,
        /// The failure's diagnostic detail (the store error's message).
        detail: String,
    },
    /// The task was skipped without running: a post-processing hook (F-115,
    /// `skip_already_transcribed`) found the input already transcribed, and
    /// the found segments are the task's transcript. A terminal state,
    /// parity-wise equivalent to `Completed` — Buzz counts a skip toward
    /// "quit on complete" (`main_window.py:435-443`) and this crate's CLI
    /// counts it toward exit-code success. [`TaskStore::mark_skipped`] is
    /// this event's store counterpart, emitted from `run_task`'s
    /// `Terminal::Skipped` arm right after that write — driven by
    /// [`postproc::skip::check`] via [`RunnerDeps::postproc`]'s `check_skip`
    /// seam in `run_job` (design spec §2.1).
    Skipped,
    /// A post-processing hook chain ran after the task completed (F-110's
    /// `on_complete` hook seam, driving the F-116 `transcript_resizer`,
    /// F-111 `ai_summary`, and F-113 `export_docx` built-ins, in that fixed
    /// order — [`Runner::run_on_complete_chain`]) and is reporting its
    /// combined outcome — mirrors Buzz's `refresh_row`-after-hooks
    /// (`main_window.py:453-477`). Emitted once, after `Completed`, **iff**
    /// at least one hook is enabled; never emitted for a task with no
    /// enabled hooks.
    PostProcessed {
        /// Whether a hook updated the task's notes (e.g. `ai_summary`).
        notes_updated: bool,
        /// Whether a hook replaced the task's persisted segments (e.g. the
        /// auto-resizer).
        segments_replaced: bool,
        /// Additional export files written by a hook (e.g. `export_docx`),
        /// in hook-run order.
        written: Vec<PathBuf>,
        /// One message per hook failure; a hook failure never fails the
        /// task itself.
        errors: Vec<String>,
    },
    /// The task moved into a distinguishable mid-run phase that isn't a
    /// plain progress fraction (M8, design spec §3/§8) — additive, like
    /// [`Self::Skipped`]/[`Self::PostProcessed`] before it. Currently
    /// emitted only for [`TaskPhase::IsolatingSpeech`] (F-027's demucs
    /// pre-pass, wired in both binaries — see
    /// [`crate::boundary::SpeechIsolator`]'s doc); extensible to future
    /// phases without another `TaskEventKind` variant.
    PhaseChanged {
        /// Which phase the task entered.
        phase: TaskPhase,
    },
}

/// A distinguishable mid-run phase reported by [`TaskEventKind::PhaseChanged`]
/// (M8, design spec §3/§8). `#[non_exhaustive]` is deliberately not used —
/// exhaustive matches on this enum are this crate's convention, so adding a
/// future phase is a compile-time-visible change at every match site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPhase {
    /// Isolating speech from background noise/music with demucs before
    /// transcription (F-027). Indeterminate progress: the subprocess only
    /// emits human-oriented tqdm output on stderr, which Standing Policy
    /// rule 3 forbids parsing, so no fraction accompanies this phase.
    IsolatingSpeech,
}

/// A [`TaskEventKind`] tagged with the task it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskEvent {
    /// The task this event is about.
    pub task_id: TranscriptionId,
    /// What happened.
    pub kind: TaskEventKind,
}

/// Ensures on-disk model/VAD artifacts are present, downloading them if
/// necessary, so a GUI caller can enqueue a task without a model already
/// resolved (`docs/design/UX_SPEC.md` §J-1: model downloads must never block
/// other imports). Configured via [`RunnerDeps::artifacts`]; the CLI never
/// constructs one (it resolves the model synchronously before enqueueing —
/// Buzz parity, `cli.py:219-221`).
pub trait ArtifactEnsurer: Send + Sync {
    /// Ensures `model` is present locally, downloading it if needed, and
    /// returns its local path.
    ///
    /// `progress` is called with incremental updates, mirroring
    /// [`crate::models::download::ModelDownloader::ensure_model`]'s
    /// contract; `cancel` is checked cooperatively during the download.
    /// Errors are classified exactly like every other pipeline error:
    /// cancellation — reported as either [`CoreError::Canceled`] or
    /// `CoreError::Model(ModelError::Canceled)` (what a
    /// [`crate::models::download::ModelDownloader`]-backed implementation
    /// naturally produces when a download is canceled) — ends the task in
    /// a canceled terminal state, never a failed one; any other error
    /// fails the task with that error's message.
    fn ensure_model(
        &self,
        model: &ModelSpec,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError>;

    /// Ensures the VAD model is present, returning its path — or `None` if
    /// it could not be obtained (offline, network blip, canceled).
    ///
    /// Unlike [`Self::ensure_model`], failure here never fails the task: VAD
    /// is a nice-to-have (Buzz's own absent-model behavior,
    /// `whisper_cpp.py:380`), so a failed ensure simply degrades to no VAD
    /// for this job, exactly like the CLI's `ensure_vad` helper.
    fn ensure_vad(
        &self,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Option<PathBuf>;
}

/// Builds a [`TranscriptionEngine`] for a given [`ModelSpec`].
///
/// The runner caches the most recently built engine and reuses it while
/// consecutive tasks request an identical `ModelSpec` (one CLI run typically
/// uses one model), calling [`EngineFactory::create`] again only when the
/// model changes.
pub trait EngineFactory: Send {
    /// Creates an engine for `model`, or returns why it could not.
    ///
    /// `resolved` is the model's on-disk path, already ensured for this job
    /// by an [`ArtifactEnsurer`] — `Some` when [`RunnerDeps::artifacts`] is
    /// configured (GUI) and this call follows a fresh `ensure_model`; `None`
    /// when no `ArtifactEnsurer` is configured (CLI, which resolves the
    /// model itself before enqueueing and whose factory ignores this
    /// parameter).
    fn create(
        &mut self,
        model: &ModelSpec,
        resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError>;
}

/// Everything the runner thread needs to process tasks.
pub struct RunnerDeps {
    /// The status writer / persistence layer.
    pub store: Arc<dyn TaskStore>,
    /// Decodes media files to PCM.
    pub decoder: Arc<dyn AudioDecoder>,
    /// Probes media metadata.
    pub prober: Arc<dyn MediaProber>,
    /// Downloads remote (URL) inputs. `None` makes URL tasks fail actionably
    /// (the yt-dlp adapter is wired in by the binary, Task 6).
    pub fetcher: Option<Arc<dyn MediaFetcher>>,
    /// Builds transcription engines on demand.
    pub engines: Box<dyn EngineFactory>,
    /// Engine tuning (VAD model, GPU, threads).
    pub tuning: EngineTuning,
    /// Directory under which URL downloads land (`media_dir/<task-id>/`).
    pub media_dir: PathBuf,
    /// Root directory downloaded whisper models live under (`paths::model_root()`
    /// in both binaries) — needed independently of any per-task
    /// [`crate::domain::ModelSpec`] so [`postproc::langdetect::select_model`]
    /// (M8, F-114) can scan for the largest already-downloaded model. Read
    /// only when [`Self::lang_detector`] is configured and enabled.
    pub model_root: PathBuf,
    /// Ensures model/VAD artifacts before a task's engine runs, enabling
    /// non-modal downloads for GUI callers (`docs/design/UX_SPEC.md` §J-1).
    /// `None` (the CLI) means models are already resolved before
    /// enqueueing — `run_job` then skips artifact-ensure entirely and
    /// behaves byte-for-byte as it did before this field existed.
    pub artifacts: Option<Arc<dyn ArtifactEnsurer>>,
    /// Resolves the per-task LLM-translation endpoint at run time (F-031,
    /// design spec §6), threaded into [`run_file_job`]. `None` (the CLI, and
    /// any caller not wiring translation) means the file path's "Enable AI
    /// translation" option is inert — no endpoint is ever resolved and no
    /// translator is spawned, exactly as before this field existed.
    ///
    /// There is deliberately **no** `encoder` field alongside this: the
    /// OpenAI transcription engine's [`crate::boundary::AudioEncoder`] is held
    /// by the binaries' own [`EngineFactory`] impls, which construct the
    /// engine — the queue never builds an engine itself, so it never needs
    /// the encoder (design spec §3, Task 9 brief deliverable B).
    pub llm: Option<Arc<dyn LlmEndpointSource>>,
    /// Supplies a fresh [`crate::postproc::PostprocConfig`] snapshot per
    /// task, gating the `check_skip` seam in [`Runner::run_job`] (design
    /// spec §2.1/§2.4). `None` — every caller that predates M7, and any
    /// test double that doesn't need hooks — means the seam never runs and
    /// every task transcribes exactly as it did before this field existed.
    pub postproc: Option<Arc<dyn PostprocConfigSource>>,
    /// Isolates speech from background noise/music before transcription
    /// (F-027, design spec §3), gated on [`crate::domain::TranscriptionOptions::extract_speech`].
    /// `None` (every pre-M8 caller, and any test double that doesn't need
    /// the hook) means it never runs — `run_job` behaves byte-for-byte as it
    /// did before this field existed, the `postproc: None` precedent.
    pub isolator: Option<Arc<dyn SpeechIsolator>>,
    /// Reduces background noise before transcription (F-112, design spec
    /// §4), gated on `PostprocConfig::noise_reduction.enabled`. `None`
    /// disables the hook entirely, same precedent as [`Self::isolator`].
    pub reducer: Option<Arc<dyn NoiseReducer>>,
    /// Detects the spoken language before transcription (F-114, design spec
    /// §5), gated on `PostprocConfig::language_detection.enabled` and
    /// [`postproc::langdetect::should_detect`]. `None` disables the hook
    /// entirely, same precedent as [`Self::isolator`].
    pub lang_detector: Option<Arc<dyn LanguageDetector>>,
    /// PCM speaker diarizer used when a task sets `identify_speakers`.
    /// `None` means that flag cannot be honored.
    pub diarizer: Option<Arc<dyn crate::diarize::Diarizer>>,
}

/// Shared registry of live subscriber senders.
type SubscriberList = Arc<Mutex<Vec<flume::Sender<TaskEvent>>>>;

/// Shared slot holding the running task's id and cancel token, so the handle
/// can trip cancellation promptly without waiting for the runner loop.
type CurrentTask = Arc<Mutex<Option<(TranscriptionId, CancelToken)>>>;

/// Locks a `Mutex`, recovering the guard if a previous holder panicked.
///
/// The runner is panic-free by construction, so poisoning should not occur;
/// recovering (rather than unwrapping) keeps the queue alive even if a
/// misbehaving store/engine impl panics while holding a lock.
fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sends `event` to every subscriber, pruning any whose receiver was dropped.
fn emit(subscribers: &Mutex<Vec<flume::Sender<TaskEvent>>>, event: TaskEvent) {
    let mut subs = lock_recover(subscribers);
    subs.retain(|tx| tx.send(event.clone()).is_ok());
}

/// Logs and emits [`TaskEventKind::StoreWriteFailed`] for a store write that
/// failed at `operation` (the store method name, e.g. `"mark_started"`) —
/// the ADR-018 policy: the failure is surfaced to subscribers, but never
/// aborts the run. Shared by [`Runner`] (which owns `subscribers`) and
/// [`QueueSink`] (which holds a clone of it), so every call site reports the
/// same way.
fn emit_store_write_failed(
    subscribers: &Mutex<Vec<flume::Sender<TaskEvent>>>,
    id: TranscriptionId,
    operation: &str,
    err: &StoreError,
) {
    log::error!("{operation} failed for task {id}: {err}");
    emit(
        subscribers,
        TaskEvent {
            task_id: id,
            kind: TaskEventKind::StoreWriteFailed {
                operation: operation.to_string(),
                detail: err.to_string(),
            },
        },
    );
}

/// A command sent from a [`QueueHandle`] to the runner thread.
enum Command {
    /// Persist and enqueue a new task.
    Enqueue {
        /// The task's id.
        id: TranscriptionId,
        /// The task specification (boxed — much larger than the other
        /// variants, which would otherwise bloat every `Command`).
        spec: Box<TaskSpec>,
        /// Where the task came from.
        source: Source,
    },
    /// Insert a `queued` row and emit [`TaskEventKind::Queued`] without
    /// starting the task. Folder watch does this on first sight.
    Stage {
        /// The task's id.
        id: TranscriptionId,
        /// The task specification.
        spec: Box<TaskSpec>,
        /// Where the task came from.
        source: Source,
    },
    /// Cancel a staged task that never started. Emits
    /// [`TaskEventKind::Canceled`]. Does not record the path as seen.
    Abandon(TranscriptionId),
    /// Run a task whose row was already inserted as `queued`. Folder watch
    /// creates the row when the file is first seen and only then hands it
    /// to the runner, after the file's size and mtime have been stable.
    Adopt {
        /// The task's id. The store row must already exist.
        id: TranscriptionId,
        /// The task specification to run.
        spec: Box<TaskSpec>,
    },
    /// Re-enqueue an already-terminated task's *existing* row for a retry:
    /// same `id`, fresh `spec` (see [`QueueHandle::retry`]).
    Retry {
        /// The task's id, reused rather than minting a new one.
        id: TranscriptionId,
        /// The (possibly edited) task specification to retry with.
        spec: Box<TaskSpec>,
    },
    /// Add `id` to the canceled-set (and, if it is the current task, its token
    /// was already tripped by the handle).
    Cancel(TranscriptionId),
    /// Cancel the current task and every task pending at this moment (one-shot;
    /// tasks enqueued later are unaffected).
    CancelAll,
    /// No more enqueues; drain the remaining queue, then stop.
    Finish,
}

/// The controlling handle returned by [`spawn_task_runner`].
///
/// Owns the command channel to the runner and a shared registry of event
/// subscribers. Dropping the handle without calling [`QueueHandle::finish`]
/// disconnects the command channel; the runner then drains its remaining queue
/// and exits.
pub struct QueueHandle {
    /// Command channel to the runner.
    commands: flume::Sender<Command>,
    /// Shared subscriber registry (also held by the runner).
    subscribers: SubscriberList,
    /// Shared current-task slot (also held by the runner).
    current: CurrentTask,
    /// The runner thread.
    join: JoinHandle<()>,
}

impl QueueHandle {
    /// Subscribes to the event stream, returning a fresh unbounded receiver.
    ///
    /// Every subscriber sees every event emitted after it subscribes. A
    /// subscriber that stops draining and is dropped is pruned on the next
    /// emit; a slow subscriber simply buffers (the channel is unbounded).
    pub fn subscribe(&self) -> flume::Receiver<TaskEvent> {
        let (tx, rx) = flume::unbounded();
        lock_recover(&self.subscribers).push(tx);
        rx
    }

    /// Enqueues a task for transcription. Returns immediately; the runner
    /// persists (`create_task`) and emits `Queued` on its own thread.
    pub fn enqueue(&self, id: TranscriptionId, spec: TaskSpec, source: Source) {
        let _ = self.commands.send(Command::Enqueue {
            id,
            spec: Box::new(spec),
            source,
        });
    }

    /// Persists a queued row and emits `Queued`. Does not start the task.
    pub fn stage(&self, id: TranscriptionId, spec: TaskSpec, source: Source) {
        let _ = self.commands.send(Command::Stage {
            id,
            spec: Box::new(spec),
            source,
        });
    }

    /// Cancels a staged task that was never adopted. Emits `Canceled`.
    pub fn abandon(&self, id: TranscriptionId) {
        let _ = self.commands.send(Command::Abandon(id));
    }

    /// Starts a task that is already a `queued` row. Does not call
    /// `create_task`. Folder watch uses this after the file has settled.
    pub fn adopt(&self, id: TranscriptionId, spec: TaskSpec) {
        let _ = self.commands.send(Command::Adopt {
            id,
            spec: Box::new(spec),
        });
    }

    /// Retries a terminated task: reuses `id`'s existing row rather than
    /// creating a new one — the runner calls [`TaskStore::requeue_task`],
    /// not `create_task` (Buzz DAO parity:
    /// `TranscriptionDAO.reset_transcription_for_restart`,
    /// `transcription_dao.py:353-370`, resets the row in place) — and
    /// re-enqueues it with `spec`. Returns immediately; the runner requeues
    /// and emits `Queued` on its own thread, same as [`Self::enqueue`].
    ///
    /// Before enqueueing, the runner purges any stale canceled-set entry for
    /// `id`. Such an entry can outlive the task it targeted in two ways:
    /// canceling the task *while it runs* leaves one until `run_task`'s own
    /// tail-drain consumes it at terminal time, and canceling a task that
    /// has *already* terminated leaves one that nothing ever consumes (no
    /// task with that id is pending or current again) — see
    /// `cancel_of_running_task_leaves_no_stale_canceled_id`. Left in place,
    /// either would make `run_task` treat the retried run as pre-canceled
    /// and skip it straight to `Canceled` without running it. Buzz's own
    /// queue worker has the identical guard: `add_task` removes the task's
    /// id from `canceled_tasks` "for restart functionality"
    /// (`file_transcriber_queue_worker.py:357-361`) before enqueueing.
    ///
    /// A `requeue_task` store failure emits `Failed` instead of `Queued`
    /// (mirroring [`Self::enqueue`]'s `create_task`-failure arm) and the
    /// task never runs.
    pub fn retry(&self, id: TranscriptionId, spec: TaskSpec) {
        let _ = self.commands.send(Command::Retry {
            id,
            spec: Box::new(spec),
        });
    }

    /// Requests cancellation of `id`.
    ///
    /// If `id` is the running task, its cancel token is tripped immediately
    /// (prompt cancellation); the command also enters the canceled-set so a
    /// still-pending `id` is canceled without ever starting.
    pub fn cancel(&self, id: TranscriptionId) {
        let _ = self.commands.send(Command::Cancel(id));
        if let Some((current_id, token)) = lock_recover(&self.current).as_ref()
            && *current_id == id
        {
            token.cancel();
        }
    }

    /// Requests cancellation of the running task and every task pending at the
    /// moment of the call. Tasks enqueued afterward run normally (one-shot).
    pub fn cancel_all(&self) {
        let _ = self.commands.send(Command::CancelAll);
        if let Some((_, token)) = lock_recover(&self.current).as_ref() {
            token.cancel();
        }
    }

    /// Signals "no more enqueues", waits for the runner to drain its remaining
    /// queue, and joins the thread.
    ///
    /// Returns `Ok(())` once the runner has stopped. The runner is panic-free
    /// by construction, so a join panic is logged as a defect and still
    /// reported as `Ok` (the runner has, in fact, stopped).
    pub fn finish(self) -> Result<(), CoreError> {
        let QueueHandle { commands, join, .. } = self;
        let _ = commands.send(Command::Finish);
        drop(commands);
        match join.join() {
            Ok(()) => Ok(()),
            Err(_) => {
                log::error!("task runner thread panicked");
                Ok(())
            }
        }
    }

    /// Signals "no more enqueues" and waits for the runner to drain, exactly
    /// like [`Self::finish`] — except bounded by `timeout` rather than
    /// waiting forever: the belt-and-braces for app exit (ADR-022) that lets
    /// a wedged engine/pipeline never block process shutdown.
    ///
    /// [`Self::finish`] stays unbounded on purpose (it is the CLI's own
    /// execution mechanism — enqueue N, drain all; a timeout there would
    /// truncate legitimate work). `finish_within` exists for callers that
    /// only want a bounded *exit* guarantee, e.g. GUI shutdown after
    /// [`Self::cancel_all`].
    ///
    /// Mechanism: send [`Command::Finish`] and drop the command sender (same
    /// as [`Self::finish`]), then hand the (unbounded) `join.join()` to a
    /// small waiter thread that signals an `mpsc` channel with its result;
    /// this call blocks only on that channel's `recv_timeout(timeout)` —
    /// the same "watcher thread + bounded channel recv" shape
    /// [`crate::openai::client::cancelable`] uses to bound a blocking HTTP
    /// call (ADR-022).
    ///
    /// - **Drained within `timeout`:** returns `Ok(true)`. A runner panic is
    ///   still logged as a defect and still reported `Ok` — the same
    ///   contract [`Self::finish`] has (the runner has, in fact, stopped
    ///   either way).
    /// - **Still running at `timeout`:** logs `log::error!` naming the
    ///   timeout, then returns `Ok(false)`. **Both the runner thread and the
    ///   waiter thread are deliberately leaked** — neither is joined or
    ///   aborted. Leaking is sound here: process exit reclaims every OS
    ///   thread regardless of whether anything ever joined it, and every
    ///   blocking request an engine makes through
    ///   [`crate::openai::client::cancelable`] has its own bounded
    ///   per-request timeout (ADR-022) — so even a genuinely wedged runner
    ///   thread eventually finishes its current step and exits quietly on
    ///   its own, just after this call has already returned control to the
    ///   caller.
    pub fn finish_within(self, timeout: Duration) -> Result<bool, CoreError> {
        let QueueHandle { commands, join, .. } = self;
        let _ = commands.send(Command::Finish);
        drop(commands);

        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = join.join();
            // The receiver may already be gone (we timed out and returned
            // control to the caller already) — expected and fine, not an
            // error worth handling.
            let _ = done_tx.send(result);
        });

        match done_rx.recv_timeout(timeout) {
            Ok(Ok(())) => Ok(true),
            Ok(Err(_)) => {
                log::error!("task runner thread panicked");
                Ok(true)
            }
            Err(_) => {
                log::error!(
                    "queue did not drain within {timeout:?} — leaking the runner thread \
                     deliberately so app exit is never blocked by a wedged engine (ADR-022); \
                     every engine request has its own bounded timeout, so the thread will exit \
                     quietly on its own once that elapses"
                );
                Ok(false)
            }
        }
    }
}

/// A task waiting in the runner's FIFO (already persisted as `queued`).
struct PendingTask {
    /// The task's id.
    id: TranscriptionId,
    /// The task specification.
    spec: Box<TaskSpec>,
}

/// How a task's run ended, before it is written to the store.
enum Terminal {
    /// Succeeded, with the pipeline outcome (segments + written files) and
    /// the extra data [`Runner::run_task`]'s `on_complete` hook chain needs
    /// (see [`PostCompleteContext`]). Boxed: [`PostprocConfig`] (nested
    /// inside `PostCompleteContext`) is large enough that an unboxed second
    /// field would make every other `Terminal` variant pay for its size
    /// (`clippy::large_enum_variant`) — this is the common case (`None`
    /// whenever no [`RunnerDeps::postproc`] source is configured), and it's
    /// paid once per completed task either way, so the extra indirection
    /// costs nothing observable.
    Completed(JobOutcome, Box<PostCompleteContext>),
    /// Canceled (cooperatively) during the run.
    Canceled,
    /// Failed, with an actionable message.
    Failed(String),
    /// Skipped: the `check_skip` seam (F-115, [`postproc::skip::check`])
    /// found the input already transcribed before the engine ever ran;
    /// `segments` are the task's transcript, copied from wherever the check
    /// found them (a result file or a prior completed row).
    Skipped(Vec<Segment>),
}

/// Data [`Runner::run_job`] computes for a task that completes
/// successfully, needed by [`Runner::run_task`]'s `on_complete` hook chain
/// (F-116/F-111/F-113, design spec §2.1) after the pipeline has already
/// returned control to it:
///
/// - `input_path`: the resolved local input path — a URL task's *fetched*
///   file, not the original `TaskSpec::input`. `run_job` already computed
///   this once (as `job.input`) to run the pipeline; threading it here
///   avoids re-deriving it (which would otherwise need a fresh store read
///   for URL tasks, since nothing else persists it for `run_task` to see).
/// - `postproc`: the [`PostprocConfig`] snapshot taken once for this task
///   (design spec §2.4), reused here rather than re-snapshotted in
///   `run_task` — [`PostprocConfig`]'s own doc comment promises one
///   consistent settings view per task across *both* hook seams
///   (`check_skip` above in `run_job`, `on_complete` here), even if the
///   user edits Preferences while this task is mid-run. `None` exactly
///   when [`RunnerDeps::postproc`] is `None` (every pre-M7 caller, and any
///   test double that doesn't need hooks) — the chain then never runs, as
///   if this field didn't exist.
/// - `detected_language`: F-114's detected language, if the pre-transcription
///   chain's detection step resolved one (M8, review fix). Buzz's own plugin
///   mutates `task.transcription_options.language` in place, so its
///   `on_complete`-equivalent hooks (notably F-116's transcript resizer,
///   whose regroup step chooses a CJK vs. Latin word-join separator by
///   language, `plugins/enhanced_language_detection/plugin.py:99-101`) see
///   the detected language too — `run_task` folds this in as
///   `detected_language.or(spec.options.language)` when calling
///   `run_on_complete_chain`, so the chain observes the same effective
///   language [`run_file_job`] transcribed with.
struct PostCompleteContext {
    input_path: PathBuf,
    postproc: Option<PostprocConfig>,
    detected_language: Option<Language>,
}

/// Drop-based cleanup of the M8 pre-transcription chain's temp derivative
/// files (design spec §3/§4): the isolated `_speech` file and the denoised
/// `_DeepFilterNet3` file, if either step ran and actually produced one (a
/// step whose output path is identical to its input — see the
/// `!= audio_input`/`!= prior_audio` checks at each call site — is never
/// recorded here at all, so a misbehaving/no-op adapter can never cause this
/// guard to delete the user's own source). Constructed once per
/// [`Runner::run_job`] call and held for that call's whole duration, so every
/// *successfully-produced* file it does know about is cleaned up on **every**
/// exit path — `Completed`, `Failed`, `Canceled`, and a panic unwinding
/// through `run_job` (which is itself already `catch_unwind`-contained one
/// level up, in `Runner::run_task` — the unwind still passes through this
/// guard's stack frame on its way out, running `Drop` normally). A step that
/// fails after writing a *partial* output before returning `Err` (e.g. a
/// killed subprocess that had already started writing its target file) is
/// not covered here — that partial file was never returned to this guard to
/// record, so removing it is the adapter's own obligation (a later task's
/// `DemucsIsolator`, not this seam). This is humm's improvement over Buzz,
/// which deletes its `_speech.mp3` only in its `on_task_completed` handler
/// and leaks it on failure/cancel (deviations ledger #3); F-112's
/// `_DeepFilterNet3` file gets the same all-terminals treatment (ledger,
/// same rule).
///
/// `denoised` is exempted from cleanup when `keep_denoised_file` is set
/// (`PostprocConfig::noise_reduction.keep_denoised_file`) — the user asked
/// to keep it as an artifact next to the input. `isolated` has no such
/// setting: Buzz's own `_speech.mp3` is never a keepable artifact either.
struct TempAudioGuard {
    isolated: Option<PathBuf>,
    denoised: Option<PathBuf>,
    keep_denoised: bool,
    /// Unregisters on drop. Deletion stays in [`Drop`] so a normal return
    /// still removes the file; a second Ctrl-C, which skips `Drop`, uses
    /// the registration instead.
    isolated_track: Option<crate::abort::TrackedTemp>,
    denoised_track: Option<crate::abort::TrackedTemp>,
}

impl Drop for TempAudioGuard {
    fn drop(&mut self) {
        if let Some(path) = self.isolated.take() {
            remove_temp_audio_file(&path);
        }
        if self.keep_denoised {
            return;
        }
        if let Some(path) = self.denoised.take() {
            remove_temp_audio_file(&path);
        }
    }
}

/// Removes a temp derivative file, logging (not panicking) on any failure
/// other than "already gone" — cleanup running twice, or a file that was
/// never actually created because the step it belongs to never ran, must
/// never be an error.
fn remove_temp_audio_file(path: &Path) {
    if let Err(err) = std::fs::remove_file(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("failed to remove temp audio file {}: {err}", path.display());
    }
}

/// `true` if `error` represents cancellation rather than a genuine failure.
///
/// `CoreError::Model(ModelError::Canceled)` is included alongside the bare
/// `CoreError::Canceled`: a real (downloader-backed) [`ArtifactEnsurer`]
/// wraps [`crate::models::download::ModelDownloader`], whose own
/// `Result<_, ModelError>` surfaces cancellation as `ModelError::Canceled`
/// (`download.rs`) — `?`/`.into()` on that then produces
/// `CoreError::Model(ModelError::Canceled)`, which must classify as
/// cancellation exactly like every other cancellation path here.
fn is_canceled(error: &CoreError) -> bool {
    matches!(
        error,
        CoreError::Canceled
            | CoreError::Engine(EngineError::Canceled)
            | CoreError::Media(MediaError::Canceled)
            | CoreError::Model(ModelError::Canceled)
    )
}

/// Maps a pipeline error to a [`Terminal`] (cancellation vs failure).
fn classify(error: CoreError) -> Terminal {
    if is_canceled(&error) {
        Terminal::Canceled
    } else {
        Terminal::Failed(error.to_string())
    }
}

/// Extracts a human-readable message from a `catch_unwind` panic payload:
/// `panic!("literal")` payloads downcast to `&str`, `panic!("{}", x)` and
/// `.expect(msg)` payloads downcast to `String`; anything else (a custom
/// payload passed to `panic_any`) falls back to a generic message.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic payload".to_string())
}

/// Whether a hook's own `error` should be folded into the aggregated
/// `PostProcessed.errors` list — `None` (dropped) whenever `cancel` is
/// tripped *by the time the hook returns*: design spec §2.2's ai_summary
/// cancellation (`cancelable`, acknowledged sub-second) is a user-initiated
/// interruption, not a hook failure, so it must never surface as one — the
/// same way a between-hooks cancellation never even attempts the next
/// hook, let alone records an error for it not running. `cancel` is
/// re-checked *after* the call (not just before, which
/// `Runner::run_on_complete_chain`'s own gate already does) because the
/// cancellation this guards against can land *while* the hook's request is
/// still in flight — the whole reason `cancel` wasn't already tripped when
/// the call started.
fn hook_error_after_cancel_check(error: Option<String>, cancel: &CancelToken) -> Option<String> {
    if cancel.is_canceled() { None } else { error }
}

/// Throttles [`TaskEventKind::ModelDownloadProgress`] emission across one
/// job's whole artifact-ensure phase (module doc): [`Runner::ensure_artifacts`]
/// builds a fresh instance per job and feeds it every
/// [`crate::models::download::DownloadProgress`] callback from both
/// `ensure_model` and `ensure_vad` in sequence, so the two artifacts' progress
/// streams share (and are throttled against) the same running state.
///
/// Mirrors [`QueueSink::on_progress`]'s store-write throttle: a callback with
/// a known, nonzero `total` fires only the first time a new whole percent is
/// reached. A callback with an unknown (or zero — avoids a division by zero)
/// `total` has no percentage to round to, so bytes stand in for it instead:
/// it fires on the very first such callback (`last_mib` starts `None`), then
/// again only once `downloaded` crosses into a new whole mebibyte.
#[derive(Debug, Default)]
struct ModelDownloadThrottle {
    /// Last whole percent an event was emitted for, if any (`total` known).
    last_pct: Option<i64>,
    /// Last whole mebibyte an event was emitted for, if any (`total`
    /// unknown or zero).
    last_mib: Option<u64>,
}

impl ModelDownloadThrottle {
    /// Whether `update` should produce an event, recording it as the new
    /// baseline when it does.
    fn should_emit(&mut self, update: &DownloadProgress) -> bool {
        match update.total {
            Some(total) if total > 0 => {
                let pct = ((update.downloaded as f64 / total as f64) * 100.0).floor() as i64;
                if self.last_pct == Some(pct) {
                    false
                } else {
                    self.last_pct = Some(pct);
                    true
                }
            }
            _ => {
                let mib = update.downloaded / (1024 * 1024);
                if self.last_mib == Some(mib) {
                    false
                } else {
                    self.last_mib = Some(mib);
                    true
                }
            }
        }
    }
}

/// Builds a throttled [`TaskEventKind::ModelDownloadProgress`]-emitting
/// closure for one artifact-download phase, sharing
/// [`ModelDownloadThrottle`]'s discipline — factored out so both
/// [`Runner::ensure_artifacts`] (the task's own model/VAD) and
/// [`Runner::ensure_tiny_lang_detect_model`] (F-114's tiny-model fallback,
/// M8) emit progress identically without duplicating the throttle wiring.
fn model_download_progress_fn(
    subscribers: SubscriberList,
    id: TranscriptionId,
) -> impl FnMut(DownloadProgress) {
    let mut throttle = ModelDownloadThrottle::default();
    move |update: DownloadProgress| {
        if !throttle.should_emit(&update) {
            return;
        }
        emit(
            &subscribers,
            TaskEvent {
                task_id: id,
                kind: TaskEventKind::ModelDownloadProgress {
                    file: update.file,
                    downloaded: update.downloaded,
                    total: update.total,
                },
            },
        );
    }
}

/// The [`SegmentSink`] the runner hands to the pipeline: it forwards segment
/// and progress updates as [`TaskEvent`]s and writes progress to the store,
/// throttled to whole-percent changes.
///
/// The throttle is a deliberate deviation from Buzz, which writes on every
/// progress signal (`queue_worker.py:398-400`): whole-percent granularity is
/// visually identical but far cheaper on the store. Events are *not* throttled
/// — every callback forwards a `Progress` event.
struct QueueSink {
    /// The task these updates belong to.
    id: TranscriptionId,
    /// The store to write throttled progress to.
    store: Arc<dyn TaskStore>,
    /// The subscriber registry to emit to.
    subscribers: SubscriberList,
    /// Last whole percent written to the store, if any.
    last_pct: Option<i64>,
}

impl SegmentSink for QueueSink {
    fn on_segment(&mut self, segment: Segment) {
        emit(
            &self.subscribers,
            TaskEvent {
                task_id: self.id,
                kind: TaskEventKind::SegmentAdded(segment),
            },
        );
    }

    fn on_progress(&mut self, fraction: f32) {
        let pct = (fraction * 100.0).floor() as i64;
        if self.last_pct != Some(pct) {
            self.last_pct = Some(pct);
            if let Err(err) = self.store.update_progress(&self.id, f64::from(fraction)) {
                emit_store_write_failed(&self.subscribers, self.id, "update_progress", &err);
            }
        }
        emit(
            &self.subscribers,
            TaskEvent {
                task_id: self.id,
                kind: TaskEventKind::Progress(fraction),
            },
        );
    }
}

/// The single worker thread: owns the FIFO, the canceled-set, and the engine
/// cache, and drives each task through the store.
struct Runner {
    /// Incoming commands from the handle.
    rx: flume::Receiver<Command>,
    /// Shared subscriber registry.
    subscribers: SubscriberList,
    /// Shared current-task slot.
    current: CurrentTask,
    /// Status writer / persistence.
    store: Arc<dyn TaskStore>,
    /// Media decoder.
    decoder: Arc<dyn AudioDecoder>,
    /// Media prober.
    prober: Arc<dyn MediaProber>,
    /// URL downloader, if configured.
    fetcher: Option<Arc<dyn MediaFetcher>>,
    /// Engine builder.
    engines: Box<dyn EngineFactory>,
    /// Engine tuning.
    tuning: EngineTuning,
    /// Root directory for URL downloads.
    media_dir: PathBuf,
    /// Root directory for downloaded whisper models — see
    /// [`RunnerDeps::model_root`].
    model_root: PathBuf,
    /// Ensures model/VAD artifacts per job when configured (GUI). `None`
    /// (CLI) skips artifact-ensure entirely — see [`RunnerDeps::artifacts`].
    artifacts: Option<Arc<dyn ArtifactEnsurer>>,
    /// Resolves the per-task LLM-translation endpoint (F-031), threaded into
    /// [`run_file_job`]. `None` disables translation entirely — see
    /// [`RunnerDeps::llm`].
    llm: Option<Arc<dyn LlmEndpointSource>>,
    /// Supplies a fresh post-processing config snapshot per task. `None`
    /// disables the `check_skip` seam entirely — see [`RunnerDeps::postproc`].
    postproc: Option<Arc<dyn PostprocConfigSource>>,
    /// Isolates speech before transcription (F-027) — see
    /// [`RunnerDeps::isolator`].
    isolator: Option<Arc<dyn SpeechIsolator>>,
    /// Reduces noise before transcription (F-112) — see
    /// [`RunnerDeps::reducer`].
    reducer: Option<Arc<dyn NoiseReducer>>,
    /// Detects the spoken language before transcription (F-114) — see
    /// [`RunnerDeps::lang_detector`].
    lang_detector: Option<Arc<dyn LanguageDetector>>,
    /// PCM diarizer for tasks that set `identify_speakers`.
    diarizer: Option<Arc<dyn crate::diarize::Diarizer>>,
    /// Tasks accepted but not yet run (FIFO).
    pending: VecDeque<PendingTask>,
    /// Ids canceled but not yet dequeued (Buzz's canceled-set). *Consumed*: a
    /// pending id is removed as soon as its task is dequeued. A `Cancel(id)`
    /// sent for the task that is *already* running isn't processed until the
    /// runner next drains its command channel — which `run_task` does once
    /// more at its own tail, right before removing `id` there too — so that
    /// redundant insert can never outlive the task it targeted.
    canceled: HashSet<TranscriptionId>,
    /// Once set, exit after the pending queue drains.
    finishing: bool,
    /// The cached engine and the model it was built for.
    engine_cache: Option<(ModelSpec, Box<dyn TranscriptionEngine>)>,
}

/// RAII guard clearing the shared "current task" slot (`Runner::current`)
/// on drop — belt-and-braces for `Runner::run_task`'s whole terminal-write
/// region (the `mark_*` store write, the terminal event, and the
/// `on_complete` hook chain): even if something in that region panicked,
/// the slot would still be cleared as the unwind passes through this
/// guard's `Drop`, rather than leaking a permanent "task `id` is current"
/// entry that would make every later `cancel`/`cancel_all` call target a
/// task that no longer exists (and, being current, may never dequeue
/// again). In practice this should never actually fire on a panic: every
/// individual hook call inside `Runner::run_on_complete_chain` is already
/// contained in its own `catch_unwind`, so a hook panicking can't reach
/// here — this guard exists for the region's *other* code (the `mark_*`
/// store writes, `emit`), which was never wrapped in `catch_unwind` even
/// before M7 and isn't specifically hardened by this milestone either; it
/// is cheap enough to add unconditionally rather than carefully arguing
/// that exposure is unchanged.
struct CurrentGuard<'a>(&'a Runner);

impl Drop for CurrentGuard<'_> {
    fn drop(&mut self) {
        self.0.set_current(None);
    }
}

impl Runner {
    /// The runner loop: drain commands, run the next pending task, repeat;
    /// block for a command when idle; exit once finishing and drained.
    fn run(mut self) {
        loop {
            self.drain_commands_nonblocking();

            if let Some(task) = self.pending.pop_front() {
                self.run_task(task);
                continue;
            }

            if self.finishing {
                break;
            }

            match self.rx.recv() {
                Ok(command) => self.handle_command(command),
                // All command senders dropped: drain what's left, then stop.
                Err(_) => self.finishing = true,
            }
        }
    }

    /// Processes every immediately-available command without blocking.
    fn drain_commands_nonblocking(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(command) => self.handle_command(command),
                Err(flume::TryRecvError::Empty) => break,
                Err(flume::TryRecvError::Disconnected) => {
                    self.finishing = true;
                    break;
                }
            }
        }
    }

    /// Applies one command to the runner's state.
    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Stage { id, spec, source } => {
                match self.store.create_task(&id, &spec, source) {
                    Ok(()) => {
                        self.emit(TaskEvent {
                            task_id: id,
                            kind: TaskEventKind::Queued,
                        });
                    }
                    Err(err) => {
                        let message = CoreError::from(err).to_string();
                        log::error!("create_task failed for task {id}: {message}");
                        if let Err(err2) = self.store.mark_failed(&id, &message) {
                            self.store_write_failed(id, "mark_failed", &err2);
                        }
                        self.emit(TaskEvent {
                            task_id: id,
                            kind: TaskEventKind::Failed { message },
                        });
                    }
                }
            }
            Command::Abandon(id) => {
                self.write_canceled(id);
            }
            Command::Adopt { id, spec } => {
                self.pending.push_back(PendingTask { id, spec });
            }
            Command::Enqueue { id, spec, source } => {
                match self.store.create_task(&id, &spec, source) {
                    Ok(()) => {
                        self.emit(TaskEvent {
                            task_id: id,
                            kind: TaskEventKind::Queued,
                        });
                        self.pending.push_back(PendingTask { id, spec });
                    }
                    Err(err) => {
                        let message = CoreError::from(err).to_string();
                        log::error!("create_task failed for task {id}: {message}");
                        // `create_task` just failed, so no row for `id` normally
                        // exists yet: this `mark_failed` is then a harmless 0-row
                        // UPDATE (carried ticket 6). It is kept for symmetry with
                        // the requeue-failure arm below and so that, if a partial
                        // `create_task` *did* leave a row behind, the store still
                        // ends up reflecting the failure rather than a stray
                        // half-written row.
                        if let Err(err2) = self.store.mark_failed(&id, &message) {
                            self.store_write_failed(id, "mark_failed", &err2);
                        }
                        self.emit(TaskEvent {
                            task_id: id,
                            kind: TaskEventKind::Failed { message },
                        });
                    }
                }
            }
            Command::Retry { id, spec } => {
                // Purge any stale canceled-set entry for `id` *before*
                // requeueing — see `QueueHandle::retry`'s doc for why one
                // can be sitting here (a task canceled mid-run, whose
                // tail-drain hasn't run yet, or a task canceled after it
                // already reached a terminal state, whose entry nothing
                // ever consumes). Left in place, `run_task`'s dequeue-time
                // canceled-set check would treat this retry as already
                // canceled and skip it without running.
                self.canceled.remove(&id);
                match self.store.requeue_task(&id) {
                    Ok(()) => {
                        self.emit(TaskEvent {
                            task_id: id,
                            kind: TaskEventKind::Queued,
                        });
                        self.pending.push_back(PendingTask { id, spec });
                    }
                    Err(err) => {
                        let message = CoreError::from(err).to_string();
                        log::error!("requeue_task failed for task {id}: {message}");
                        if let Err(err2) = self.store.mark_failed(&id, &message) {
                            self.store_write_failed(id, "mark_failed", &err2);
                        }
                        self.emit(TaskEvent {
                            task_id: id,
                            kind: TaskEventKind::Failed { message },
                        });
                    }
                }
            }
            Command::Cancel(id) => {
                self.canceled.insert(id);
            }
            Command::CancelAll => {
                // One-shot (Buzz-style): cancel the current task and every task
                // pending *now* — not tasks enqueued later. Snapshot the current
                // backlog into the canceled-set, and trip the running task's
                // token. Tripping here (in addition to the handle, which does it
                // at call time for promptness) is the sole path that catches a
                // task dequeued into `current` between pop and the handle's
                // read, since that task is no longer in `pending`.
                let pending_ids: Vec<TranscriptionId> =
                    self.pending.iter().map(|task| task.id).collect();
                self.canceled.extend(pending_ids);
                if let Some((_, token)) = lock_recover(&self.current).as_ref() {
                    token.cancel();
                }
            }
            Command::Finish => {
                self.finishing = true;
            }
        }
    }

    /// Runs one dequeued task end to end, writing its terminal status before
    /// emitting the terminal event.
    fn run_task(&mut self, task: PendingTask) {
        let PendingTask { id, spec } = task;
        let token = CancelToken::new();

        // Register as current *before* the cancel check, so a concurrent
        // `cancel(id)`/`cancel_all` from the handle trips this token; then
        // re-drain commands so a just-arrived `Cancel`/`CancelAll` is honored
        // (a `CancelAll` drained here trips this now-current token).
        self.set_current(Some((id, token.clone())));
        self.drain_commands_nonblocking();

        // Consume this id from the canceled-set (keeping it bounded) while
        // deciding whether the task was canceled before it could start.
        let canceled_before_start = self.canceled.remove(&id);
        if canceled_before_start || token.is_canceled() {
            self.set_current(None);
            self.write_canceled(id);
            return;
        }

        if let Err(err) = self.store.mark_started(&id) {
            self.store_write_failed(id, "mark_started", &err);
        }
        self.emit(TaskEvent {
            task_id: id,
            kind: TaskEventKind::Started,
        });

        // Contain a panicking engine/pipeline: it must fail this one task,
        // never take down the runner thread (and the rest of the queue with
        // it). `run_job` takes `&mut self`, so the closure is not
        // (statically) unwind-safe — `AssertUnwindSafe` is sound here because
        // a panic mid-`run_job` leaves no in-progress mutation we rely on
        // afterward: `engine_cache` is explicitly cleared below rather than
        // trusted, and every other field `run_job` might have touched is
        // either write-only-on-success (`engine_cache`, again) or unrelated
        // to this task's outcome.
        let terminal = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_job(id, &spec, &token)
        })) {
            Ok(terminal) => terminal,
            Err(payload) => {
                let msg = panic_message(&*payload);
                log::error!("task {id} panicked: {msg}");
                // A previously cached engine — or one `run_job` built but
                // never got to cache (the panic happened before its
                // unconditional cache-assignment ran) — must not be reused:
                // discard it unconditionally.
                self.engine_cache = None;
                Terminal::Failed(format!(
                    "internal error: task panicked: {msg} — see the log file"
                ))
            }
        };

        // `current` is cleared by `_current_guard`'s `Drop` at the end of
        // this block (rather than right after the pipeline itself
        // returned): the `on_complete` hook chain below may still be
        // running an interruptible hook (ai_summary's request), and
        // `QueueHandle::cancel`/`cancel_all`'s prompt-cancel fast path only
        // trips a token it finds in `current` — clearing this any earlier
        // would make a cancel sent *during* the hook chain silently do
        // nothing until this task's `id` is next dequeued (which, for a
        // task that already reached a terminal state, is never), defeating
        // design spec §2.2's "cancel mid-summary is acked sub-second"
        // promise. The guard (rather than a bare call after the match)
        // means the slot is cleared even if something in this block
        // panics — see `CurrentGuard`'s own doc comment.
        {
            let _current_guard = CurrentGuard(&*self);
            match terminal {
                Terminal::Completed(outcome, ctx) => {
                    if let Err(err) = self.store.mark_completed(&id, &outcome.segments) {
                        self.store_write_failed(id, "mark_completed", &err);
                    }
                    self.emit(TaskEvent {
                        task_id: id,
                        kind: TaskEventKind::Completed {
                            written: outcome.written,
                        },
                    });
                    // `on_complete` hook chain (F-116/F-111/F-113, design
                    // spec §2.1) — a no-op when no config source was
                    // configured at all (`ctx.postproc: None`, checked
                    // inside `run_on_complete_chain` itself). The effective
                    // language is F-114's detected language when the
                    // pre-transcription chain resolved one, else the task's
                    // own choice — `PostCompleteContext::detected_language`'s
                    // doc comment explains why the chain must see this and
                    // not just `spec.options.language` (review fix).
                    let chain_language = ctx.detected_language.or(spec.options.language);
                    self.run_on_complete_chain(
                        id,
                        &ctx,
                        &outcome.segments,
                        spec.options.word_timestamps,
                        chain_language.map(Language::code),
                        &token,
                    );
                }
                Terminal::Canceled => self.write_canceled(id),
                Terminal::Failed(message) => {
                    if let Err(err) = self.store.mark_failed(&id, &message) {
                        self.store_write_failed(id, "mark_failed", &err);
                    }
                    self.emit(TaskEvent {
                        task_id: id,
                        kind: TaskEventKind::Failed { message },
                    });
                }
                Terminal::Skipped(segments) => {
                    if let Err(err) = self.store.mark_skipped(&id, &segments) {
                        self.store_write_failed(id, "mark_skipped", &err);
                    }
                    self.emit(TaskEvent {
                        task_id: id,
                        kind: TaskEventKind::Skipped,
                    });
                }
            }
        }

        // Consume the id from the canceled-set at terminal time. `cancel(id)`
        // always sends `Command::Cancel(id)` over the channel — even for the
        // task that's already current, where the token (tripped directly by
        // the handle) is the real cancellation mechanism and this command is
        // otherwise redundant — and nothing else drains the channel while
        // this task is running, so that command is still unprocessed here.
        // Drain it now, before removing, so it cannot outlive this task and
        // leak a permanent entry for an id that will never be dequeued again.
        //
        // The removal is conditional on `id` still being absent from
        // `pending`: a `Retry(id)` drained in this same window re-queues
        // `id` (after its own handler already purged any *stale* canceled-
        // set entry — `QueueHandle::retry`'s doc comment) — and if a fresh
        // `Cancel(id)` for that retry is drained right behind it, this tail
        // would otherwise erase that Cancel's insert before the retried run
        // ever gets a chance to observe it, silently un-canceling a retry
        // the caller asked to cancel. Skipping the removal whenever `id` is
        // (again) pending leaves that fresh insert intact for the retried
        // run's own dequeue-time check to consume instead.
        self.drain_commands_nonblocking();
        if !self.pending.iter().any(|task| task.id == id) {
            self.canceled.remove(&id);
        }
    }

    /// Writes `canceled` to the store, then emits the `Canceled` event.
    fn write_canceled(&self, id: TranscriptionId) {
        if let Err(err) = self.store.mark_canceled(&id) {
            self.store_write_failed(id, "mark_canceled", &err);
        }
        self.emit(TaskEvent {
            task_id: id,
            kind: TaskEventKind::Canceled,
        });
    }

    /// Logs and emits [`TaskEventKind::StoreWriteFailed`] for a store write
    /// that failed at `operation` (ADR-018).
    fn store_write_failed(&self, id: TranscriptionId, operation: &str, err: &StoreError) {
        emit_store_write_failed(&self.subscribers, id, operation, err);
    }

    /// Resolves the input (downloading a URL if needed), ensures artifacts
    /// when configured (module doc: `Started` → [`DownloadProgress`*] →
    /// [`ModelDownloadProgress`*] → `Progress`/`SegmentAdded`*), acquires the
    /// engine, and runs the file-transcription pipeline.
    fn run_job(&mut self, id: TranscriptionId, spec: &TaskSpec, token: &CancelToken) -> Terminal {
        let (input_path, display_name) = match &spec.input {
            Input::Path(path) => (path.clone(), None),
            Input::Url(url) => match self.fetch_url(id, url, token) {
                Ok(fetched) => fetched,
                Err(terminal) => return terminal,
            },
        };

        let model = spec.options.model.clone();
        let job = crate::domain::JobSpec {
            input: input_path,
            options: spec.options.clone(),
            output: spec.output.clone(),
            display_name,
        };

        // A single [`PostprocConfig`] snapshot for this whole task (design
        // spec §2.4), taken once here — iff a config source is configured
        // at all — and reused for *both* hook seams: the F-115 `check_skip`
        // gate right below, and (threaded into `Terminal::Completed` via
        // [`PostCompleteContext`]) the `on_complete` chain `run_task` runs
        // after this job returns. One snapshot per task means the whole
        // chain sees one consistent settings view even if the user edits
        // Preferences mid-run — re-snapshotting separately in `run_task`
        // would let a mid-run edit apply inconsistently between the two
        // seams.
        let postproc_cfg = self.postproc.as_ref().map(|source| source.snapshot());

        // F-115 `check_skip` seam (design spec §2.1): after input resolution
        // (a URL task's `job.input` is by now the fetched local file, named
        // `source.<ext>`, not the title) and
        // before artifact-ensure/engine-build — Buzz's own ordering
        // (`file_transcriber_queue_worker.py:208-230`), which runs the check
        // right after `mark_started`/`Started` too. Dispatches on
        // `cfg.skip.enabled` — no source (`None`) or a disabled hook leaves
        // this job's behavior byte-for-byte unchanged.
        if let Some(cfg) = postproc_cfg.as_ref()
            && cfg.skip.enabled
            && let Some(segments) = postproc::skip::check(
                &cfg.skip,
                &job.input,
                job.output.directory.as_deref(),
                self.store.as_ref(),
                match &spec.input {
                    Input::Url(url) => Some(url.as_str()),
                    Input::Path(_) => None,
                },
            )
        {
            return Terminal::Skipped(segments);
        }

        // M8 pre-transcription chain (design spec §3/§5): isolation → noise
        // reduction → language detection, in that fixed order (ledger #5),
        // placed between `check_skip` (above) and artifact-ensure (below) —
        // deliberately *after* Buzz's own ordering, which extracts speech
        // before `check_skip` and thereby breaks skip+extract together
        // (ledger #2). `audio_input` tracks the current decode target as
        // each successful step swaps it; `job.input` is never touched — it
        // stays the export-naming/DB-row/viewer identity throughout (Buzz
        // parity: the DB insert already happened before extraction,
        // `plugins/manager.py:107-119`'s path-swap is in-memory only).
        // `guard` cleans up every temp derivative file on every exit path
        // from this function, including a panic (`TempAudioGuard`'s doc
        // comment).
        let mut audio_input = job.input.clone();
        let mut detected_language: Option<Language> = None;
        let mut guard = TempAudioGuard {
            isolated: None,
            denoised: None,
            keep_denoised: postproc_cfg
                .as_ref()
                .map(|cfg| cfg.noise_reduction.keep_denoised_file)
                .unwrap_or(false),
            isolated_track: None,
            denoised_track: None,
        };

        // (1) F-027 speech isolation — gated on the task's own option
        // (`extract_speech` is a per-task choice, not a Preferences hook, so
        // it is deliberately *not* gated on `postproc_cfg`) and an isolator
        // actually being wired (`self.isolator` — both binaries wire a real
        // `DemucsIsolator`; absence of the demucs binary is handled inside
        // the adapter, not by leaving this seam `None`).
        // Not `catch_unwind`-contained beyond `run_task`'s existing
        // outer containment: a genuine isolation panic fails the task (Buzz
        // parity) — unlike reduction/detection below, which degrade instead.
        if job.options.extract_speech
            && let Some(isolator) = self.isolator.clone()
        {
            self.emit(TaskEvent {
                task_id: id,
                kind: TaskEventKind::PhaseChanged {
                    phase: TaskPhase::IsolatingSpeech,
                },
            });
            match isolator.isolate(&audio_input, token) {
                Ok(isolated) => {
                    // Only queue `isolated` for cleanup when it's actually a
                    // *new* file distinct from what was passed in — a
                    // misbehaving (or fallback/no-op) isolator that echoes
                    // its input path back must never cause `TempAudioGuard`
                    // to delete the user's own source (review fix).
                    if isolated != audio_input {
                        // The source path is not registered. Only a new file
                        // is, so emergency cleanup cannot delete the user's media.
                        guard.isolated_track =
                            Some(crate::abort::track_temp_file(isolated.clone(), false));
                        guard.isolated = Some(isolated.clone());
                    }
                    audio_input = isolated;
                }
                Err(err) => {
                    // A cancellation mid-isolation (the isolator kills its
                    // child process and reports `MediaError::Canceled`, or
                    // any other cooperative-cancel error, per
                    // `SpeechIsolator::isolate`'s doc) must end the task
                    // `Canceled`, not `Failed` with a misleading
                    // "isolation failed" message — checked before the
                    // no-audio/actionable-failure classification below, same
                    // as every other cancel check in this chain.
                    if token.is_canceled() {
                        return Terminal::Canceled;
                    }
                    // Buzz's `IndexError -> "no_audio"` arm (`:80-83`): a
                    // silent video (or any other input with no audio stream)
                    // degrades to the original file instead of failing.
                    let no_audio = matches!(
                        self.prober.probe(&job.input),
                        Ok(info) if !info.has_audio
                    );
                    if no_audio {
                        log::warn!(
                            "speech isolation skipped, {} has no audio stream: {err}",
                            job.input.display()
                        );
                    } else {
                        return Terminal::Failed(format!(
                            "speech isolation failed: {err} — check that demucs is \
                             installed and on PATH, and your internet connection \
                             (demucs may need to download a model on first run)"
                        ));
                    }
                }
            }
        }

        if token.is_canceled() {
            return Terminal::Canceled;
        }

        // (2) F-112 noise reduction — gated on Preferences
        // (`cfg.noise_reduction.enabled`) and a reducer being wired; chained
        // onto whatever `audio_input` is by now (the isolated file if step
        // 1 ran and succeeded, else the original). `catch_unwind`-contained:
        // a reducer failure *or panic* degrades to the prior audio rather
        // than failing the task (Buzz returns `None` on exception,
        // `plugin.py:58-60`).
        if let Some(cfg) = postproc_cfg.as_ref()
            && cfg.noise_reduction.enabled
            && let Some(reducer) = self.reducer.clone()
        {
            let prior_audio = audio_input.clone();
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                reducer.reduce(&prior_audio, token)
            })) {
                Ok(Ok(denoised)) => {
                    // Same identity check as the isolation step above: never
                    // queue the reducer's own input path for cleanup.
                    if denoised != prior_audio {
                        guard.denoised_track = Some(crate::abort::track_temp_file(
                            denoised.clone(),
                            guard.keep_denoised,
                        ));
                        guard.denoised = Some(denoised.clone());
                    }
                    audio_input = denoised;
                }
                Ok(Err(err)) => {
                    log::warn!("noise reduction failed, proceeding with the prior audio: {err}");
                }
                Err(payload) => {
                    let msg = panic_message(&*payload);
                    log::error!("noise reduction hook panicked for task {id}: {msg}");
                }
            }
        }

        if token.is_canceled() {
            return Terminal::Canceled;
        }

        // (3) F-114 enhanced language detection — gated on Preferences
        // (`cfg.language_detection.enabled`), a detector being wired, and
        // Buzz's own selection rule (`postproc::langdetect::should_detect`:
        // unset or explicitly English, the en-quirk). `catch_unwind`-
        // contained: a detector failure *or panic* degrades to auto rather
        // than failing the task.
        //
        // Detection needs decoded audio, which this seam decodes itself
        // (below) via `self.decoder` — the *same* `audio_input` `run_file_job`
        // will decode again moments later once this whole pre-transcription
        // chain returns control to it. This file therefore decodes twice
        // whenever detection actually runs. Buzz pays the same cost the same
        // way: its `enhanced_language_detection` plugin spawns a separate
        // `whisper-cli --detect-language` process (`whisper_cpp.py:403+`)
        // ahead of the main transcription process, so ported faithfully
        // rather than optimized away.
        if let Some(cfg) = postproc_cfg.as_ref()
            && cfg.language_detection.enabled
            && let Some(detector) = self.lang_detector.clone()
            && postproc::langdetect::should_detect(job.options.language)
        {
            let model_path = match postproc::langdetect::select_model(&self.model_root) {
                Some(path) => Some(path),
                None if cfg.language_detection.download_tiny_if_missing => {
                    match self.artifacts.clone() {
                        Some(ensurer) => {
                            self.ensure_tiny_lang_detect_model(id, ensurer.as_ref(), token)
                        }
                        None => None,
                    }
                }
                None => None,
            };

            match model_path {
                None => {
                    log::warn!("language detection skipped: no whisper.cpp model available");
                }
                Some(model_path) => {
                    let decoder = &*self.decoder;
                    let detect_audio = audio_input.clone();
                    let outcome: std::thread::Result<Result<Option<Language>, String>> =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let pcm = decoder
                                .decode(&detect_audio, token)
                                .map_err(|err| err.to_string())?;
                            detector
                                .detect(&pcm, &model_path, token)
                                .map_err(|err| err.to_string())
                        }));
                    match outcome {
                        Ok(Ok(Some(lang))) => {
                            // Same mid-job store-write convention `fetch_url`'s
                            // `update_file` write already uses (ADR-018):
                            // log + emit `StoreWriteFailed`, never abort the
                            // run over a DB hiccup — the detected language
                            // still reaches `run_file_job` below either way.
                            if let Err(err) = self.store.update_language(&id, lang.code()) {
                                self.store_write_failed(id, "update_language", &err);
                            }
                            detected_language = Some(lang);
                        }
                        Ok(Ok(None)) => {
                            log::info!("language detection did not return a language");
                        }
                        Ok(Err(msg)) => {
                            log::warn!("language detection failed, proceeding on auto: {msg}");
                        }
                        Err(payload) => {
                            let msg = panic_message(&*payload);
                            log::error!("language detection hook panicked for task {id}: {msg}");
                        }
                    }
                }
            }
        }

        if token.is_canceled() {
            return Terminal::Canceled;
        }

        // When an ArtifactEnsurer is configured (GUI), ensure the model/VAD
        // artifacts as this job's own phase and use a job-local tuning with
        // the freshly-resolved VAD path; otherwise (CLI) behavior is
        // byte-for-byte unchanged — `self.tuning` is borrowed directly, no
        // clone, `resolved_path` stays `None`.
        //
        // The `Backend::OpenAiApi` backend has no local model or VAD model to
        // resolve (design spec §2.1): the whole ArtifactEnsurer phase is
        // skipped for it — no `ModelDownloadProgress` events, `resolved_path`
        // stays `None`, and the engine factory routes on `spec.backend`. Local
        // (`Whisper`) backend behavior is entirely unchanged.
        let mut ensured_tuning: Option<EngineTuning> = None;
        let mut resolved_path: Option<PathBuf> = None;
        if model.backend != Backend::OpenAiApi
            && let Some(ensurer) = self.artifacts.clone()
        {
            match self.ensure_artifacts(id, ensurer.as_ref(), &model, token) {
                Ok((path, tuning)) => {
                    resolved_path = Some(path);
                    ensured_tuning = Some(tuning);
                }
                Err(terminal) => return terminal,
            }
        }
        let tuning: &EngineTuning = ensured_tuning.as_ref().unwrap_or(&self.tuning);

        // Reuse the cached engine when the model matches, else build a new
        // one. A cache hit skips `create` (the just-ensured artifact is not
        // needed to build an engine that already exists) but the ensure
        // above still ran, keeping it fresh on disk regardless.
        let cached = self.engine_cache.take();
        let mut engine = match cached {
            Some((cached_model, engine)) if cached_model == model => engine,
            _ => match self.engines.create(&model, resolved_path.as_deref()) {
                Ok(engine) => engine,
                Err(err) => return classify(err),
            },
        };

        let mut sink = QueueSink {
            id,
            store: Arc::clone(&self.store),
            subscribers: Arc::clone(&self.subscribers),
            last_pct: None,
        };

        let result = run_file_job(
            &job,
            &audio_input,
            detected_language,
            tuning,
            &*self.decoder,
            &*self.prober,
            engine.as_mut(),
            &mut sink,
            token,
            self.llm.as_deref(),
            self.diarizer.as_deref(),
        );

        self.engine_cache = Some((model, engine));

        match result {
            Ok(outcome) => Terminal::Completed(
                outcome,
                Box::new(PostCompleteContext {
                    input_path: job.input,
                    postproc: postproc_cfg,
                    detected_language,
                }),
            ),
            Err(err) => classify(err),
        }
    }

    /// Runs the `on_complete` post-processing hook chain for one
    /// just-completed task (F-116/F-111/F-113, design spec §2.1/§2.2),
    /// entirely synchronously on this (the queue runner) thread: fixed
    /// order **auto_resize → summary → docx** (ledger #L2 — resizer first
    /// so `docx`, which re-reads the store, sees the resize; `summary`
    /// reads the in-memory `outcome_segments`, which resizing never
    /// textually changes). `ctx.postproc` being `None` (no
    /// [`RunnerDeps::postproc`] source configured for this run at all) is a
    /// silent no-op, exactly like every hook being individually disabled.
    /// Each hook is individually gated on its own `enabled` flag — the same
    /// convention every hook module's own `run` already follows for its
    /// *other* settings.
    ///
    /// `cancel` is checked before *every* hook, including the first
    /// (symmetric with the between-hooks checks: a cancellation landed in
    /// the tiny window between the pipeline returning `Completed` and this
    /// chain actually starting must skip `auto_resize` too, not just
    /// `summary`/`docx`). A cancellation observed there skips every hook
    /// from that point on, but the task's own status is untouched (it is
    /// already `Completed`) and the aggregated event below still reports
    /// whatever ran before the cancellation — design spec §2.2's
    /// "surviving hooks are skipped, and the task stays `Completed`". A
    /// cancellation that lands *during* `summary`'s own in-flight request
    /// (rather than between hooks) is handled by
    /// [`hook_error_after_cancel_check`]: it is likewise not folded into
    /// `errors` — a user-initiated interruption is not a hook failure.
    ///
    /// Each hook call is individually wrapped in `catch_unwind`: a
    /// panicking hook is contained exactly like `run_job`'s own panic
    /// containment (`panic_message`) — logged, folded into `errors` as an
    /// actionable message — and the chain continues to the *next* hook
    /// (Buzz's per-plugin `try`/`except` keeps going after a plugin
    /// failure, `manager.py:136-148`) rather than unwinding into
    /// `Runner::run_task` and taking down the whole runner thread.
    ///
    /// Emits exactly one [`TaskEventKind::PostProcessed`], and only when at
    /// least one hook is actually enabled (design spec §2.3: "iff at least
    /// one hook is enabled") — a `cfg` with every hook disabled (or no
    /// `cfg` at all) emits nothing, matching the T2 contract that this
    /// event is additive and optional.
    fn run_on_complete_chain(
        &self,
        id: TranscriptionId,
        ctx: &PostCompleteContext,
        outcome_segments: &[Segment],
        word_timestamps: bool,
        language: Option<&str>,
        cancel: &CancelToken,
    ) {
        let Some(cfg) = ctx.postproc.as_ref() else {
            return;
        };
        if !(cfg.resize.enabled || cfg.summary.enabled || cfg.docx.enabled) {
            return;
        }

        let mut notes_updated = false;
        let mut segments_replaced = false;
        let mut written: Vec<PathBuf> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        if !cancel.is_canceled() && cfg.resize.enabled {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                postproc::auto_resize::run(cfg, &id, word_timestamps, language, self.store.as_ref())
            })) {
                Ok(result) => {
                    segments_replaced = result.replaced;
                    if let Some(err) = result.error {
                        errors.push(err);
                    }
                }
                Err(payload) => {
                    let msg = panic_message(&*payload);
                    log::error!("transcript resizer hook panicked for task {id}: {msg}");
                    errors.push(format!("transcript resizer panicked: {msg}"));
                }
            }
        }

        if !cancel.is_canceled() && cfg.summary.enabled {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                postproc::summary::run(
                    cfg,
                    outcome_segments,
                    &ctx.input_path,
                    self.store.as_ref(),
                    &id,
                    cancel,
                )
            })) {
                Ok(result) => {
                    notes_updated = result.notes_updated;
                    if let Some(path) = result.file_written {
                        written.push(path);
                    }
                    if let Some(err) = hook_error_after_cancel_check(result.error, cancel) {
                        errors.push(err);
                    }
                }
                Err(payload) => {
                    let msg = panic_message(&*payload);
                    log::error!("AI Summary hook panicked for task {id}: {msg}");
                    errors.push(format!("AI Summary panicked: {msg}"));
                }
            }
        }

        if !cancel.is_canceled() && cfg.docx.enabled {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                postproc::docx::run(cfg, &id, &ctx.input_path, self.store.as_ref())
            })) {
                Ok(result) => {
                    if let Some(path) = result.written {
                        written.push(path);
                    }
                    if let Some(err) = result.error {
                        errors.push(err);
                    }
                }
                Err(payload) => {
                    let msg = panic_message(&*payload);
                    log::error!("Export to DOCX hook panicked for task {id}: {msg}");
                    errors.push(format!("Export to DOCX panicked: {msg}"));
                }
            }
        }

        self.emit(TaskEvent {
            task_id: id,
            kind: TaskEventKind::PostProcessed {
                notes_updated,
                segments_replaced,
                written,
                errors,
            },
        });
    }

    /// Ensures `model` (and the VAD model) via `ensurer`, emitting a
    /// throttled [`TaskEventKind::ModelDownloadProgress`] per
    /// [`ModelDownloadThrottle`] for progress callbacks from either. Only
    /// called when `self.artifacts` is `Some`.
    ///
    /// A failed/canceled `ensure_model` becomes this job's [`Terminal`]
    /// directly, via the same [`classify`] every other pipeline error goes
    /// through — an ensurer that reports cancellation as
    /// [`CoreError::Canceled`] therefore ends the task `Canceled`, not
    /// `Failed`. `ensure_vad` never fails the job: its result (`Some`/`None`)
    /// simply becomes this job's `tuning.vad_model`.
    fn ensure_artifacts(
        &self,
        id: TranscriptionId,
        ensurer: &dyn ArtifactEnsurer,
        model: &ModelSpec,
        token: &CancelToken,
    ) -> Result<(PathBuf, EngineTuning), Terminal> {
        let mut on_progress = model_download_progress_fn(Arc::clone(&self.subscribers), id);

        let resolved = ensurer
            .ensure_model(model, &mut on_progress, token)
            .map_err(classify)?;

        let mut tuning = self.tuning.clone();
        tuning.vad_model = ensurer.ensure_vad(&mut on_progress, token);

        Ok((resolved, tuning))
    }

    /// F-114's tiny-model download fallback (design spec §5, Buzz
    /// `plugin.py:153-169`): called only when
    /// [`postproc::langdetect::select_model`] found nothing already
    /// downloaded and `download_tiny_if_missing` is set. Reuses
    /// [`Self::artifacts`]'s [`ArtifactEnsurer::ensure_model`] with a `Tiny`
    /// [`ModelSpec`] so the download goes through the exact same
    /// resume/sentinel/progress machinery as the task's own model —
    /// emitting the same [`TaskEventKind::ModelDownloadProgress`] events.
    ///
    /// Never fails the task: a download failure is logged and detection is
    /// simply skipped (`None`), matching Buzz's own `except Exception` arm
    /// there (`:164-167`) — language detection is a nice-to-have, not a
    /// requirement.
    fn ensure_tiny_lang_detect_model(
        &self,
        id: TranscriptionId,
        ensurer: &dyn ArtifactEnsurer,
        token: &CancelToken,
    ) -> Option<PathBuf> {
        let mut on_progress = model_download_progress_fn(Arc::clone(&self.subscribers), id);
        let tiny_spec = ModelSpec {
            backend: Backend::Whisper,
            size: ModelSize::Tiny,
            hf_repo: None,
        };
        match ensurer.ensure_model(&tiny_spec, &mut on_progress, token) {
            Ok(path) => Some(path),
            Err(err) => {
                log::warn!(
                    "language detection: failed to download the tiny model, skipping \
                     detection for this task: {err}"
                );
                None
            }
        }
    }

    /// Downloads a URL input into `media_dir/<id>/`, emitting `DownloadProgress`
    /// events (only while the total is known) and recording the local path via
    /// `update_file`. A raw title, when the fetcher reports one, is stored
    /// with [`TaskStore::set_task_name`] and returned for the export stem.
    /// The CLI progress label stays the URL (`Input::display_name`).
    /// Returns `(local path, raw title)`, or a [`Terminal`] on failure.
    fn fetch_url(
        &self,
        id: TranscriptionId,
        url: &url::Url,
        token: &CancelToken,
    ) -> Result<(PathBuf, Option<String>), Terminal> {
        let Some(fetcher) = self.fetcher.as_ref() else {
            return Err(Terminal::Failed(ytdlp_missing_message()));
        };

        let dest = self.media_dir.join(id.to_string());
        if let Err(err) = std::fs::create_dir_all(&dest) {
            return Err(classify(CoreError::from(MediaError::Io(err))));
        }

        let subscribers = Arc::clone(&self.subscribers);
        let mut progress = move |fetched: FetchProgress| {
            if let Some(total) = fetched.total
                && total > 0
            {
                // Clamped to the doc'd `0.0..=1.0` range: a low/stale
                // `total_bytes_estimate` can otherwise let `downloaded`
                // exceed `total`.
                let fraction = (fetched.downloaded as f32 / total as f32).min(1.0);
                emit(
                    &subscribers,
                    TaskEvent {
                        task_id: id,
                        kind: TaskEventKind::DownloadProgress(fraction),
                    },
                );
            }
        };

        let fetched = match fetcher.fetch(url, &dest, &mut progress, token) {
            Ok(fetched) => fetched,
            Err(err) => return Err(classify(CoreError::from(err))),
        };

        if let Some(title) = fetched.title.as_deref().filter(|title| !title.is_empty())
            && let Err(err) = self.store.set_task_name(&id, title)
        {
            self.store_write_failed(id, "set_task_name", &err);
        }
        if let Err(err) = self.store.update_file(&id, &fetched.path) {
            self.store_write_failed(id, "update_file", &err);
        }
        Ok((fetched.path, fetched.title))
    }

    /// Emits `event` to all subscribers.
    fn emit(&self, event: TaskEvent) {
        emit(&self.subscribers, event);
    }

    /// Sets (or clears) the shared current-task slot.
    fn set_current(&self, value: Option<(TranscriptionId, CancelToken)>) {
        *lock_recover(&self.current) = value;
    }
}

/// Spawns the runner thread and returns a handle to enqueue, subscribe, cancel,
/// and finish.
pub fn spawn_task_runner(deps: RunnerDeps) -> QueueHandle {
    let (command_tx, command_rx) = flume::unbounded::<Command>();
    let subscribers: SubscriberList = Arc::new(Mutex::new(Vec::new()));
    let current: CurrentTask = Arc::new(Mutex::new(None));

    let runner = Runner {
        rx: command_rx,
        subscribers: Arc::clone(&subscribers),
        current: Arc::clone(&current),
        store: deps.store,
        decoder: deps.decoder,
        prober: deps.prober,
        fetcher: deps.fetcher,
        engines: deps.engines,
        tuning: deps.tuning,
        media_dir: deps.media_dir,
        model_root: deps.model_root,
        artifacts: deps.artifacts,
        llm: deps.llm,
        postproc: deps.postproc,
        isolator: deps.isolator,
        reducer: deps.reducer,
        lang_detector: deps.lang_detector,
        diarizer: deps.diarizer,
        pending: VecDeque::new(),
        canceled: HashSet::new(),
        finishing: false,
        engine_cache: None,
    };

    let join = std::thread::spawn(move || runner.run());

    QueueHandle {
        commands: command_tx,
        subscribers,
        current,
        join,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::{EngineCaps, EngineOptions, MediaInfo, Pcm, PlaybackPcm};
    use crate::domain::{
        Backend, LlmTranslationOptions, Millis, ModelSize, OutputFormat, OutputSpec,
        TranscriptionOptions,
    };
    use crate::error::{ModelError, StoreError};
    use crate::models::test_server::{Canned, TestServer};
    use crate::openai::{LlmEndpoint, OpenAiAuth};
    use crate::postproc::{PostprocConfig, PostprocConfigSource};
    use crate::settings::{
        AiSummarySettings, AutoResizeSettings, ExportDocxSettings, LangDetectSettings,
        NoiseReductionSettings, SkipSettings,
    };
    use pretty_assertions::assert_eq;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tempfile::TempDir;

    // -- A one-shot rendezvous: the gated side signals "entered" then blocks
    //    on "proceed"; the controlling side waits for entry, then releases. --

    #[derive(Clone)]
    struct Gate {
        entered_tx: flume::Sender<()>,
        proceed_rx: flume::Receiver<()>,
    }

    impl Gate {
        fn wait(&self) {
            let _ = self.entered_tx.send(());
            let _ = self.proceed_rx.recv();
        }
    }

    struct GateControl {
        entered_rx: flume::Receiver<()>,
        proceed_tx: flume::Sender<()>,
    }

    impl GateControl {
        fn wait_entered(&self) {
            self.entered_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("gate was never entered");
        }

        fn release(&self) {
            let _ = self.proceed_tx.send(());
        }
    }

    fn gate() -> (GateControl, Gate) {
        let (entered_tx, entered_rx) = flume::bounded(1);
        let (proceed_tx, proceed_rx) = flume::bounded(1);
        (
            GateControl {
                entered_rx,
                proceed_tx,
            },
            Gate {
                entered_tx,
                proceed_rx,
            },
        )
    }

    // -- Fake store that records its calls (and can gate/fail specific ones) --

    #[derive(Debug, Clone, PartialEq)]
    enum StoreCall {
        Create(TranscriptionId),
        Requeue(TranscriptionId),
        Started(TranscriptionId),
        Progress(TranscriptionId, f64),
        UpdateFile(TranscriptionId, PathBuf),
        SetName(TranscriptionId, String),
        Completed(TranscriptionId, Vec<Segment>),
        Failed(TranscriptionId, String),
        Canceled(TranscriptionId),
        Skipped(TranscriptionId, Vec<Segment>),
        /// `segments_for` — the `on_complete` chain's own reads (the
        /// auto-resizer's initial read, and `export_docx`'s post-resize
        /// re-read): recorded so a queue-level test can observe ordering.
        SegmentsFor(TranscriptionId),
        /// `replace_task_segments` — the auto-resizer's write.
        ReplaceSegments(TranscriptionId, Vec<Segment>),
        /// `set_task_notes` — the ai_summary hook's `save_to_notes` write.
        SetNotes(TranscriptionId, String),
        /// `update_language` — F-114's detected-language write (M8).
        UpdateLanguage(TranscriptionId, String),
    }

    struct FakeStore {
        calls: Arc<Mutex<Vec<StoreCall>>>,
        fail_started: bool,
        // When true, the *first* create_task fails and later ones succeed, so a
        // test can assert the queue survives a create failure and keeps going.
        fail_next_create: std::sync::atomic::AtomicBool,
        // When true, every requeue_task call fails — exercises the retry
        // failure arm (mirrors `fail_started`'s always-fail shape, since the
        // tests that set this only ever retry once).
        fail_requeue: bool,
        completed_gate: Option<Gate>,
        canceled_gate: Option<Gate>,
        // Backing store for `segments_for`/`replace_task_segments`, so a
        // resize hook's write is actually visible to a later `segments_for`
        // read in the *same* chain (e.g. `export_docx`'s post-resize
        // re-read) — the on_complete chain-order tests depend on this.
        segments: Arc<Mutex<Vec<Segment>>>,
        // See `Cfg::replace_gate`'s doc comment.
        replace_gate: Option<Gate>,
        // When true, `replace_task_segments` panics *before* touching any
        // mutex (so the panic never poisons `calls`/`segments` for a later
        // task sharing this same `FakeStore`) — exercises the
        // `on_complete` chain's own panic containment.
        panic_in_replace_task_segments: bool,
    }

    impl FakeStore {
        fn record(&self, call: StoreCall) {
            self.calls.lock().expect("calls lock").push(call);
        }
    }

    impl TaskStore for FakeStore {
        fn create_task(
            &self,
            id: &TranscriptionId,
            _spec: &TaskSpec,
            _source: Source,
        ) -> Result<(), StoreError> {
            if self.fail_next_create.swap(false, Ordering::SeqCst) {
                return Err(StoreError::Query {
                    detail: "create boom".into(),
                });
            }
            self.record(StoreCall::Create(*id));
            Ok(())
        }

        fn requeue_task(&self, id: &TranscriptionId) -> Result<(), StoreError> {
            if self.fail_requeue {
                return Err(StoreError::Query {
                    detail: "requeue boom".into(),
                });
            }
            self.record(StoreCall::Requeue(*id));
            Ok(())
        }

        fn mark_started(&self, id: &TranscriptionId) -> Result<(), StoreError> {
            if self.fail_started {
                return Err(StoreError::Query {
                    detail: "started boom".into(),
                });
            }
            self.record(StoreCall::Started(*id));
            Ok(())
        }

        fn update_progress(&self, id: &TranscriptionId, fraction: f64) -> Result<(), StoreError> {
            self.record(StoreCall::Progress(*id, fraction));
            Ok(())
        }

        fn update_file(&self, id: &TranscriptionId, path: &Path) -> Result<(), StoreError> {
            self.record(StoreCall::UpdateFile(*id, path.to_path_buf()));
            Ok(())
        }

        fn set_task_name(&self, id: &TranscriptionId, name: &str) -> Result<(), StoreError> {
            self.record(StoreCall::SetName(*id, name.to_string()));
            Ok(())
        }

        fn update_language(&self, id: &TranscriptionId, language: &str) -> Result<(), StoreError> {
            self.record(StoreCall::UpdateLanguage(*id, language.to_string()));
            Ok(())
        }

        fn mark_completed(
            &self,
            id: &TranscriptionId,
            segments: &[Segment],
        ) -> Result<(), StoreError> {
            self.record(StoreCall::Completed(*id, segments.to_vec()));
            if let Some(g) = &self.completed_gate {
                g.wait();
            }
            Ok(())
        }

        fn mark_failed(&self, id: &TranscriptionId, error: &str) -> Result<(), StoreError> {
            self.record(StoreCall::Failed(*id, error.to_string()));
            Ok(())
        }

        fn mark_canceled(&self, id: &TranscriptionId) -> Result<(), StoreError> {
            self.record(StoreCall::Canceled(*id));
            if let Some(g) = &self.canceled_gate {
                g.wait();
            }
            Ok(())
        }

        fn mark_skipped(
            &self,
            id: &TranscriptionId,
            segments: &[Segment],
        ) -> Result<(), StoreError> {
            self.record(StoreCall::Skipped(*id, segments.to_vec()));
            Ok(())
        }

        fn find_completed_by_file_name(
            &self,
            _name: &str,
        ) -> Result<Option<TranscriptionId>, StoreError> {
            // No queue-level test exercises the DB-check path (that's
            // `postproc::skip`'s own unit tests) — always "no match".
            Ok(None)
        }

        fn find_completed_by_url(&self, _url: &str) -> Result<Option<TranscriptionId>, StoreError> {
            Ok(None)
        }

        fn segments_for(&self, id: &TranscriptionId) -> Result<Vec<Segment>, StoreError> {
            self.record(StoreCall::SegmentsFor(*id));
            Ok(self.segments.lock().expect("segments lock").clone())
        }

        fn set_task_notes(&self, id: &TranscriptionId, notes: &str) -> Result<(), StoreError> {
            self.record(StoreCall::SetNotes(*id, notes.to_string()));
            Ok(())
        }

        fn replace_task_segments(
            &self,
            id: &TranscriptionId,
            segs: &[Segment],
        ) -> Result<(), StoreError> {
            if self.panic_in_replace_task_segments {
                panic!("replace_task_segments boom");
            }
            self.record(StoreCall::ReplaceSegments(*id, segs.to_vec()));
            *self.segments.lock().expect("segments lock") = segs.to_vec();
            if let Some(g) = &self.replace_gate {
                g.wait();
            }
            Ok(())
        }
    }

    // -- Fake decoder / prober (reused from the pipeline test patterns) ------

    struct FakeDecoder {
        pcm: Pcm,
        decoded: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl AudioDecoder for FakeDecoder {
        fn decode(&self, path: &Path, _cancel: &CancelToken) -> Result<Pcm, MediaError> {
            self.decoded
                .lock()
                .expect("decoded lock")
                .push(path.to_path_buf());
            Ok(self.pcm.clone())
        }

        fn decode_playback(
            &self,
            _path: &Path,
            _cancel: &CancelToken,
        ) -> Result<PlaybackPcm, MediaError> {
            Ok(PlaybackPcm::new(vec![0.0; 4], 2, 44_100))
        }
    }

    /// Reports `has_audio: false` for exactly the first
    /// `no_audio_for_first_n_calls` calls, then `true` for every call after
    /// — `0` (the default) means every call reports `has_audio: true`,
    /// every pre-M8 test's implicit assumption. The M8 isolation
    /// no-audio-degrade test uses `1`: the isolation-failure handler's own
    /// probe (call 1) sees `false` and degrades gracefully, while
    /// `run_file_job`'s later probe of the (unmodified) original file
    /// (call 2+) sees `true` and transcribes normally — modeling demucs's
    /// own stricter/different audio-stream check (Buzz's `IndexError`)
    /// tripping on a file ffprobe still considers to have a usable stream.
    struct FakeProber {
        no_audio_for_first_n_calls: usize,
        calls: AtomicUsize,
    }

    impl MediaProber for FakeProber {
        fn probe(&self, _path: &Path) -> Result<MediaInfo, MediaError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(MediaInfo {
                duration: Some(Millis(1000)),
                has_audio: n >= self.no_audio_for_first_n_calls,
                has_video: false,
                video: None,
            })
        }
    }

    // -- Fake engine that plays a script, tracks overlap, can block ----------

    enum EngineScript {
        Emit {
            progress: Vec<f32>,
            segments: Vec<Segment>,
        },
        Fail,
        /// Panics instead of returning — exercises the runner's
        /// `catch_unwind` containment.
        Panic,
        Block(Gate),
        /// Emits `segments`, then trips the shared cancel token as its final
        /// act and returns `Ok` — lands a cancel *between* transcription
        /// finishing and the F-031 translation drain, deterministically
        /// (`cancel_during_translation_drain_yields_canceled`).
        EmitThenCancel {
            segments: Vec<Segment>,
        },
    }

    struct FakeEngine {
        scripts: Arc<Mutex<VecDeque<EngineScript>>>,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
        /// `options.vad_model` as seen by this call, in call order — lets a
        /// test prove a per-job `EngineTuning` override (from
        /// `ArtifactEnsurer::ensure_vad`) actually reached the pipeline.
        vad_seen: Arc<Mutex<Vec<Option<PathBuf>>>>,
        /// `options.language` as seen by this call, in call order — lets a
        /// test prove F-114's detected language actually reached
        /// `EngineOptions` (M8).
        language_seen: Arc<Mutex<Vec<Option<Language>>>>,
    }

    impl TranscriptionEngine for FakeEngine {
        fn transcribe(
            &mut self,
            _audio: &Pcm,
            options: &EngineOptions,
            sink: &mut dyn SegmentSink,
            cancel: &CancelToken,
        ) -> Result<(), EngineError> {
            let n = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(n, Ordering::SeqCst);
            self.vad_seen
                .lock()
                .expect("vad seen lock")
                .push(options.vad_model.clone());
            self.language_seen
                .lock()
                .expect("language seen lock")
                .push(options.language);
            let script = self.scripts.lock().expect("scripts lock").pop_front();
            let result = match script {
                Some(EngineScript::Emit { progress, segments }) => {
                    for p in progress {
                        sink.on_progress(p);
                    }
                    for s in segments {
                        sink.on_segment(s);
                    }
                    Ok(())
                }
                Some(EngineScript::Fail) => Err(EngineError::Inference {
                    detail: "boom".into(),
                }),
                Some(EngineScript::Panic) => panic!("engine exploded"),
                Some(EngineScript::Block(g)) => {
                    g.wait();
                    if cancel.is_canceled() {
                        Err(EngineError::Canceled)
                    } else {
                        Ok(())
                    }
                }
                Some(EngineScript::EmitThenCancel { segments }) => {
                    for s in segments {
                        sink.on_segment(s);
                    }
                    // Trip the token as the engine's last act: transcription
                    // reports success, but the cancel is now set for the
                    // pipeline's post-transcription translation-drain check.
                    cancel.cancel();
                    Ok(())
                }
                None => Ok(()),
            };
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            result
        }

        fn capabilities(&self) -> EngineCaps {
            EngineCaps {
                word_timestamps: false,
                translate: false,
                vad: false,
            }
        }
    }

    struct FakeFactory {
        scripts: Arc<Mutex<VecDeque<EngineScript>>>,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
        created: Arc<Mutex<Vec<ModelSpec>>>,
        /// The `resolved` path `create` was called with, in call order —
        /// lets a test prove the just-ensured model path reaches the
        /// factory (`None` on every call when no `ArtifactEnsurer` is
        /// configured — CLI parity).
        resolved_calls: Arc<Mutex<Vec<Option<PathBuf>>>>,
        vad_seen: Arc<Mutex<Vec<Option<PathBuf>>>>,
        language_seen: Arc<Mutex<Vec<Option<Language>>>>,
    }

    impl EngineFactory for FakeFactory {
        fn create(
            &mut self,
            model: &ModelSpec,
            resolved: Option<&Path>,
        ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
            self.created
                .lock()
                .expect("created lock")
                .push(model.clone());
            self.resolved_calls
                .lock()
                .expect("resolved calls lock")
                .push(resolved.map(Path::to_path_buf));
            Ok(Box::new(FakeEngine {
                scripts: Arc::clone(&self.scripts),
                in_flight: Arc::clone(&self.in_flight),
                max_in_flight: Arc::clone(&self.max_in_flight),
                vad_seen: Arc::clone(&self.vad_seen),
                language_seen: Arc::clone(&self.language_seen),
            }))
        }
    }

    // -- Fake fetcher: emits scripted progress, writes a dummy file ----------

    struct FakeFetcher {
        steps: Vec<FetchProgress>,
        title: Option<String>,
        calls: Arc<Mutex<Vec<url::Url>>>,
        /// When set, `fetch` panics instead of returning — exercises a panic
        /// that unwinds *before* `run_job` ever reaches the cached-engine
        /// check (`fetch_url` runs ahead of engine acquisition), so any
        /// engine already sitting in `engine_cache` from a prior task is
        /// still there when the unwind hits.
        panics: bool,
    }

    impl MediaFetcher for FakeFetcher {
        fn fetch(
            &self,
            url: &url::Url,
            dest_dir: &Path,
            progress: &mut dyn FnMut(FetchProgress),
            _cancel: &CancelToken,
        ) -> Result<crate::boundary::FetchedMedia, MediaError> {
            if self.panics {
                panic!("fetcher exploded");
            }
            self.calls
                .lock()
                .expect("fetch calls lock")
                .push(url.clone());
            for step in &self.steps {
                progress(*step);
            }
            let path = dest_dir.join("audio.wav");
            std::fs::write(&path, b"fake audio").map_err(MediaError::Io)?;
            Ok(crate::boundary::FetchedMedia {
                path,
                title: self.title.clone(),
            })
        }
    }

    // -- Fake ArtifactEnsurer: records calls, scripts progress + results -----

    /// One scripted outcome for a single `ensure_model` call, consumed in
    /// call order — mirrors `EngineScript`/`FakeFetcher`'s "one script entry
    /// per call" pattern.
    enum EnsureModelScript {
        /// Emit `progress` steps, then resolve to `path`.
        Ok {
            progress: Vec<DownloadProgress>,
            path: PathBuf,
        },
        /// Fail with `err` (no progress emitted).
        Err(CoreError),
    }

    /// Fake [`ArtifactEnsurer`]: records every `ensure_model`/`ensure_vad`
    /// call and plays back one scripted result per call, in call order.
    /// `model_gate`, when set, blocks `ensure_model` after recording the
    /// call (before consuming its script) — used to land a `cancel()`
    /// mid-ensure.
    struct FakeEnsurer {
        model_scripts: Arc<Mutex<VecDeque<EnsureModelScript>>>,
        vad_scripts: Arc<Mutex<VecDeque<Option<PathBuf>>>>,
        model_calls: Arc<Mutex<Vec<ModelSpec>>>,
        vad_calls: Arc<Mutex<usize>>,
        model_gate: Option<Gate>,
    }

    impl ArtifactEnsurer for FakeEnsurer {
        fn ensure_model(
            &self,
            model: &ModelSpec,
            progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Result<PathBuf, CoreError> {
            self.model_calls
                .lock()
                .expect("model calls lock")
                .push(model.clone());
            if let Some(gate) = &self.model_gate {
                gate.wait();
            }
            match self
                .model_scripts
                .lock()
                .expect("model scripts lock")
                .pop_front()
            {
                Some(EnsureModelScript::Ok {
                    progress: steps,
                    path,
                }) => {
                    for step in steps {
                        progress(step);
                    }
                    Ok(path)
                }
                Some(EnsureModelScript::Err(err)) => Err(err),
                None => Ok(PathBuf::from("/fake/default-model.bin")),
            }
        }

        fn ensure_vad(
            &self,
            _progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Option<PathBuf> {
            *self.vad_calls.lock().expect("vad calls lock") += 1;
            self.vad_scripts
                .lock()
                .expect("vad scripts lock")
                .pop_front()
                .flatten()
        }
    }

    // -- Scripted M8 pre-transcription doubles: one script entry per call,
    //    the same house pattern as `EngineScript`/`FakeFetcher` above. -------

    enum IsolateScript {
        Ok(PathBuf),
        Err(MediaError),
        /// Trips `cancel` (mirroring a real isolator's cooperative-cancel
        /// kill, `SpeechIsolator::isolate`'s doc) as its last act, then
        /// returns `Err(MediaError::Canceled)` — the isolation
        /// cancel-vs-failed test's setup.
        Cancel,
    }

    /// Scripted [`SpeechIsolator`]: records every `isolate` call's input
    /// path and plays back one scripted result per call, in call order.
    struct ScriptedIsolator {
        scripts: Arc<Mutex<VecDeque<IsolateScript>>>,
        calls: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl SpeechIsolator for ScriptedIsolator {
        fn isolate(&self, input: &Path, cancel: &CancelToken) -> Result<PathBuf, MediaError> {
            self.calls
                .lock()
                .expect("isolator calls lock")
                .push(input.to_path_buf());
            match self
                .scripts
                .lock()
                .expect("isolator scripts lock")
                .pop_front()
            {
                Some(IsolateScript::Ok(path)) => Ok(path),
                Some(IsolateScript::Err(err)) => Err(err),
                Some(IsolateScript::Cancel) => {
                    cancel.cancel();
                    Err(MediaError::Canceled)
                }
                None => Ok(input.to_path_buf()),
            }
        }

        fn available(&self) -> bool {
            true
        }
    }

    enum ReduceScript {
        Ok(PathBuf),
        Err(MediaError),
        Panic,
    }

    /// Scripted [`NoiseReducer`]: records every `reduce` call's input path
    /// and plays back one scripted result per call, in call order.
    struct ScriptedReducer {
        scripts: Arc<Mutex<VecDeque<ReduceScript>>>,
        calls: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl NoiseReducer for ScriptedReducer {
        fn reduce(&self, input: &Path, _cancel: &CancelToken) -> Result<PathBuf, MediaError> {
            self.calls
                .lock()
                .expect("reducer calls lock")
                .push(input.to_path_buf());
            match self
                .scripts
                .lock()
                .expect("reducer scripts lock")
                .pop_front()
            {
                Some(ReduceScript::Ok(path)) => Ok(path),
                Some(ReduceScript::Err(err)) => Err(err),
                Some(ReduceScript::Panic) => panic!("reducer exploded"),
                None => Ok(input.to_path_buf()),
            }
        }
    }

    enum DetectScript {
        Ok(Option<Language>),
        Err(EngineError),
        Panic,
    }

    /// Scripted [`LanguageDetector`]: records every `detect` call's model
    /// path and plays back one scripted result per call, in call order.
    struct ScriptedDetector {
        scripts: Arc<Mutex<VecDeque<DetectScript>>>,
        calls: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl LanguageDetector for ScriptedDetector {
        fn detect(
            &self,
            _audio: &Pcm,
            model: &Path,
            _cancel: &CancelToken,
        ) -> Result<Option<Language>, EngineError> {
            self.calls
                .lock()
                .expect("detector calls lock")
                .push(model.to_path_buf());
            match self
                .scripts
                .lock()
                .expect("detector scripts lock")
                .pop_front()
            {
                Some(DetectScript::Ok(lang)) => Ok(lang),
                Some(DetectScript::Err(err)) => Err(err),
                Some(DetectScript::Panic) => panic!("detector exploded"),
                None => Ok(None),
            }
        }
    }

    // -- Harness -------------------------------------------------------------

    struct Harness {
        store_calls: Arc<Mutex<Vec<StoreCall>>>,
        created: Arc<Mutex<Vec<ModelSpec>>>,
        decoded: Arc<Mutex<Vec<PathBuf>>>,
        fetch_calls: Arc<Mutex<Vec<url::Url>>>,
        max_in_flight: Arc<AtomicUsize>,
        resolved_calls: Arc<Mutex<Vec<Option<PathBuf>>>>,
        vad_seen: Arc<Mutex<Vec<Option<PathBuf>>>>,
        language_seen: Arc<Mutex<Vec<Option<Language>>>>,
        model_root: PathBuf,
        tmp: TempDir,
    }

    impl Harness {
        fn store_calls(&self) -> Vec<StoreCall> {
            self.store_calls.lock().expect("store calls").clone()
        }

        fn resolved_calls(&self) -> Vec<Option<PathBuf>> {
            self.resolved_calls.lock().expect("resolved calls").clone()
        }

        fn vad_seen(&self) -> Vec<Option<PathBuf>> {
            self.vad_seen.lock().expect("vad seen").clone()
        }

        fn language_seen(&self) -> Vec<Option<Language>> {
            self.language_seen.lock().expect("language seen").clone()
        }

        fn input_file(&self, name: &str) -> PathBuf {
            let path = self.tmp.path().join(name);
            std::fs::write(&path, b"fake media").expect("write input");
            path
        }

        /// Plants a fake-but-complete downloaded model (file + sentinel) for
        /// `size` under [`Self::model_root`] — the shape
        /// `postproc::langdetect::select_model` looks for.
        fn seed_model(&self, size: crate::domain::ModelSize) {
            let root = &self.model_root;
            std::fs::create_dir_all(root).expect("create model root");
            let name = crate::models::registry::ggml_file_name(size);
            std::fs::write(root.join(&name), b"fake model bytes").expect("write model");
            std::fs::write(
                root.join(format!(
                    "{name}{}",
                    crate::models::registry::SENTINEL_SUFFIX
                )),
                b"",
            )
            .expect("write sentinel");
        }
    }

    #[derive(Default)]
    struct Cfg {
        scripts: Vec<EngineScript>,
        no_fetcher: bool,
        fail_started: bool,
        fail_create: bool,
        fail_requeue: bool,
        completed_gate: Option<Gate>,
        canceled_gate: Option<Gate>,
        fetch_steps: Vec<FetchProgress>,
        fetch_title: Option<String>,
        fetch_panics: bool,
        ensurer: Option<Arc<dyn ArtifactEnsurer>>,
        llm: Option<Arc<dyn LlmEndpointSource>>,
        postproc: Option<Arc<dyn PostprocConfigSource>>,
        // Seeds `FakeStore`'s `segments_for`/`replace_task_segments` backing
        // store — the on_complete chain tests plant word-timestamped rows
        // here for the auto-resizer to read.
        initial_segments: Vec<Segment>,
        // Blocks inside `FakeStore::replace_task_segments` (after it has
        // recorded the call and updated the backing segments) until
        // released — lets a test land an external `cancel()` exactly
        // between the resize hook finishing and the next hook starting.
        replace_gate: Option<Gate>,
        // See `FakeStore::panic_in_replace_task_segments`'s doc comment.
        panic_in_replace_task_segments: bool,
        // M8 pre-transcription seam (`RunnerDeps::isolator`/`reducer`/
        // `lang_detector`) — `None` (the default) means the seam never runs,
        // exactly like every other pre-M8 caller.
        isolator: Option<Arc<dyn SpeechIsolator>>,
        reducer: Option<Arc<dyn NoiseReducer>>,
        lang_detector: Option<Arc<dyn LanguageDetector>>,
        // See `FakeProber::no_audio_for_first_n_calls`'s doc comment — the
        // isolation no-audio-degrade test's setup (M8).
        prober_no_audio_for_first_n_calls: usize,
    }

    fn build(cfg: Cfg) -> (QueueHandle, Harness) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store_calls = Arc::new(Mutex::new(Vec::new()));
        let created = Arc::new(Mutex::new(Vec::new()));
        let decoded = Arc::new(Mutex::new(Vec::new()));
        let fetch_calls = Arc::new(Mutex::new(Vec::new()));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let resolved_calls = Arc::new(Mutex::new(Vec::new()));
        let vad_seen = Arc::new(Mutex::new(Vec::new()));
        let language_seen = Arc::new(Mutex::new(Vec::new()));
        let model_root = tmp.path().join("models");

        let store = FakeStore {
            calls: Arc::clone(&store_calls),
            fail_started: cfg.fail_started,
            fail_next_create: std::sync::atomic::AtomicBool::new(cfg.fail_create),
            fail_requeue: cfg.fail_requeue,
            completed_gate: cfg.completed_gate,
            canceled_gate: cfg.canceled_gate,
            segments: Arc::new(Mutex::new(cfg.initial_segments)),
            replace_gate: cfg.replace_gate,
            panic_in_replace_task_segments: cfg.panic_in_replace_task_segments,
        };
        let decoder = FakeDecoder {
            pcm: Pcm::new(vec![0.0; 16_000]),
            decoded: Arc::clone(&decoded),
        };
        let factory = FakeFactory {
            scripts: Arc::new(Mutex::new(cfg.scripts.into_iter().collect())),
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_in_flight: Arc::clone(&max_in_flight),
            created: Arc::clone(&created),
            resolved_calls: Arc::clone(&resolved_calls),
            vad_seen: Arc::clone(&vad_seen),
            language_seen: Arc::clone(&language_seen),
        };
        let fetcher: Option<Arc<dyn MediaFetcher>> = if cfg.no_fetcher {
            None
        } else {
            Some(Arc::new(FakeFetcher {
                steps: cfg.fetch_steps,
                title: cfg.fetch_title,
                calls: Arc::clone(&fetch_calls),
                panics: cfg.fetch_panics,
            }))
        };

        let deps = RunnerDeps {
            store: Arc::new(store),
            decoder: Arc::new(decoder),
            prober: Arc::new(FakeProber {
                no_audio_for_first_n_calls: cfg.prober_no_audio_for_first_n_calls,
                calls: AtomicUsize::new(0),
            }),
            fetcher,
            engines: Box::new(factory),
            tuning: EngineTuning::default(),
            media_dir: tmp.path().join("media"),
            model_root: model_root.clone(),
            artifacts: cfg.ensurer,
            llm: cfg.llm,
            postproc: cfg.postproc,
            isolator: cfg.isolator,
            reducer: cfg.reducer,
            lang_detector: cfg.lang_detector,
            diarizer: None,
        };

        let handle = spawn_task_runner(deps);
        (
            handle,
            Harness {
                store_calls,
                created,
                decoded,
                fetch_calls,
                max_in_flight,
                resolved_calls,
                vad_seen,
                language_seen,
                model_root,
                tmp,
            },
        )
    }

    /// Like [`build`], but with `cfg.ensurer` set to `ensurer` — the shared
    /// harness for every `ensurer_*` test below.
    fn build_with_ensurer(
        mut cfg: Cfg,
        ensurer: Arc<dyn ArtifactEnsurer>,
    ) -> (QueueHandle, Harness) {
        cfg.ensurer = Some(ensurer);
        build(cfg)
    }

    // -- Fake PostprocConfigSource: returns a fixed snapshot every call ------

    struct FixedPostprocSource(PostprocConfig);

    impl PostprocConfigSource for FixedPostprocSource {
        fn snapshot(&self) -> PostprocConfig {
            self.0.clone()
        }
    }

    /// A [`PostprocConfig`] with every hook at its Buzz-parity default
    /// except [`PostprocConfig::skip`], overridden by the caller.
    fn postproc_cfg_with_skip(skip: SkipSettings) -> PostprocConfig {
        PostprocConfig {
            skip,
            summary: AiSummarySettings::default(),
            summary_api_key: None,
            docx: ExportDocxSettings::default(),
            resize: AutoResizeSettings::default(),
            paragraph_split: Millis(2000),
            noise_reduction: NoiseReductionSettings::default(),
            language_detection: LangDetectSettings::default(),
        }
    }

    /// A [`PostprocConfig`] with [`PostprocConfig::skip`] disabled (no
    /// on_complete-chain test below plants a sibling result file) and the
    /// three post-hooks set to the caller's values — the on_complete chain
    /// tests' own counterpart to [`postproc_cfg_with_skip`].
    fn postproc_cfg_with_hooks(
        resize: AutoResizeSettings,
        summary: AiSummarySettings,
        docx: ExportDocxSettings,
    ) -> PostprocConfig {
        PostprocConfig {
            skip: SkipSettings::default(),
            summary,
            summary_api_key: Some("sk-test".to_string()),
            docx,
            resize,
            paragraph_split: Millis(2000),
            noise_reduction: NoiseReductionSettings::default(),
            language_detection: LangDetectSettings::default(),
        }
    }

    /// A [`PostprocConfig`] with every hook at its Buzz-parity default except
    /// [`PostprocConfig::noise_reduction`]/[`PostprocConfig::language_detection`]
    /// — the M8 pre-transcription seam tests' own counterpart to
    /// [`postproc_cfg_with_skip`]/[`postproc_cfg_with_hooks`].
    fn postproc_cfg_with_m8(
        noise_reduction: NoiseReductionSettings,
        language_detection: LangDetectSettings,
    ) -> PostprocConfig {
        PostprocConfig {
            skip: SkipSettings::default(),
            summary: AiSummarySettings::default(),
            summary_api_key: None,
            docx: ExportDocxSettings::default(),
            resize: AutoResizeSettings::default(),
            paragraph_split: Millis(2000),
            noise_reduction,
            language_detection,
        }
    }

    fn path_task(input: PathBuf) -> TaskSpec {
        TaskSpec {
            input: Input::Path(input),
            options: TranscriptionOptions::default(),
            output: OutputSpec::default(),
        }
    }

    fn url_task() -> TaskSpec {
        TaskSpec {
            input: Input::Url(url::Url::parse("https://example.com/video").expect("url")),
            options: TranscriptionOptions::default(),
            output: OutputSpec::default(),
        }
    }

    fn options_for(size: ModelSize) -> TranscriptionOptions {
        TranscriptionOptions {
            model: ModelSpec {
                backend: Backend::Whisper,
                size,
                hf_repo: None,
            },
            ..TranscriptionOptions::default()
        }
    }

    fn seg(text: &str) -> Segment {
        Segment {
            start: Millis(0),
            end: Millis(100),
            text: text.into(),
            translation: String::new(),
        }
    }

    fn drain(rx: &flume::Receiver<TaskEvent>) -> Vec<TaskEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.recv_timeout(Duration::from_secs(5)) {
            events.push(event);
        }
        events
    }

    fn recv(rx: &flume::Receiver<TaskEvent>) -> TaskEvent {
        rx.recv_timeout(Duration::from_secs(5)).expect("event")
    }

    /// Receives events until one satisfies `pred` (inclusive), returning
    /// every event seen along the way. Times out (via `recv`) if `pred` is
    /// never satisfied.
    fn events_until(
        rx: &flume::Receiver<TaskEvent>,
        pred: impl Fn(&TaskEvent) -> bool,
    ) -> Vec<TaskEvent> {
        let mut events = Vec::new();
        loop {
            let event = recv(rx);
            let matched = pred(&event);
            events.push(event);
            if matched {
                return events;
            }
        }
    }

    fn kinds(events: &[TaskEvent], id: TranscriptionId) -> Vec<TaskEventKind> {
        events
            .iter()
            .filter(|e| e.task_id == id)
            .map(|e| e.kind.clone())
            .collect()
    }

    // -- Tests ---------------------------------------------------------------

    #[test]
    fn two_tasks_run_fifo_one_at_a_time_with_ordered_statuses_and_events() {
        let a_seg = seg("alpha");
        let b_seg = seg("beta");
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![a_seg.clone()],
                },
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![b_seg.clone()],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // Strictly one task at a time.
        assert_eq!(h.max_in_flight.load(Ordering::SeqCst), 1);

        // Per-task events in contract order.
        let a_kinds = kinds(&events, a);
        assert!(matches!(
            a_kinds.as_slice(),
            [
                TaskEventKind::Queued,
                TaskEventKind::Started,
                TaskEventKind::SegmentAdded(s),
                TaskEventKind::Completed { written }
            ] if *s == a_seg && written.is_empty()
        ));
        let b_kinds = kinds(&events, b);
        assert!(matches!(
            b_kinds.as_slice(),
            [
                TaskEventKind::Queued,
                TaskEventKind::Started,
                TaskEventKind::SegmentAdded(s),
                TaskEventKind::Completed { .. }
            ] if *s == b_seg
        ));

        // FIFO + one-at-a-time: A completes before B starts.
        let a_completed = events
            .iter()
            .position(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
            .expect("A completed");
        let b_started = events
            .iter()
            .position(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Started))
            .expect("B started");
        assert!(a_completed < b_started);

        // Statuses + segments handed to mark_completed.
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Completed(a, vec![a_seg])));
        assert!(calls.contains(&StoreCall::Completed(b, vec![b_seg])));
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Started(x) if *x == a))
        );
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Started(x) if *x == b))
        );
    }

    #[test]
    fn failed_task_marks_failed_and_queue_continues() {
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Fail,
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("ok")],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        assert!(calls.iter().any(|c| matches!(
            c,
            StoreCall::Failed(x, m) if *x == a && m.contains("inference failed") && m.contains("boom")
        )));
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(x, _) if *x == b))
        );
        assert!(events.iter().any(|e| e.task_id == a
            && matches!(&e.kind, TaskEventKind::Failed { message } if message.contains("boom"))));
        assert!(
            events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }

    #[test]
    fn canceling_a_pending_task_marks_it_canceled_without_starting() {
        let (engine_ctrl, engine_gate) = gate();
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Block(engine_gate)],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        // A is now executing in the engine.
        engine_ctrl.wait_entered();
        // Cancel the still-pending B (its Enqueue precedes this Cancel in the
        // channel, so B is created before it is canceled and dequeued).
        q.cancel(b);
        // Let A finish.
        engine_ctrl.release();
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Canceled(b)));
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, StoreCall::Started(x) if *x == b))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Started))
        );
        assert!(
            events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Canceled))
        );
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(x, _) if *x == a))
        );
    }

    #[test]
    fn canceling_the_current_task_marks_canceled_before_the_event() {
        let (engine_ctrl, engine_gate) = gate();
        let (cancel_ctrl, cancel_gate) = gate();
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Block(engine_gate)],
            canceled_gate: Some(cancel_gate),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        // Started is emitted before the engine blocks.
        engine_ctrl.wait_entered();
        assert!(matches!(recv(&rx).kind, TaskEventKind::Queued));
        assert!(matches!(recv(&rx).kind, TaskEventKind::Started));
        // Cancel the running task: its token trips, engine returns Canceled.
        q.cancel(a);
        engine_ctrl.release();
        // The runner enters the (gated) mark_canceled before emitting Canceled.
        cancel_ctrl.wait_entered();
        // The Canceled event is not observable while the store write is in
        // flight — status is written before the event.
        assert!(rx.try_recv().is_err());
        cancel_ctrl.release();
        assert!(matches!(recv(&rx).kind, TaskEventKind::Canceled));

        q.finish().expect("finish");
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Canceled(a)));
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(..) | StoreCall::Failed(..)))
        );
    }

    #[test]
    fn terminal_status_is_written_before_the_completed_event() {
        let (done_ctrl, done_gate) = gate();
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("x")],
            }],
            completed_gate: Some(done_gate),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        assert!(matches!(recv(&rx).kind, TaskEventKind::Queued));
        assert!(matches!(recv(&rx).kind, TaskEventKind::Started));
        assert!(matches!(recv(&rx).kind, TaskEventKind::SegmentAdded(_)));
        // The runner is inside the gated mark_completed, before the event.
        done_ctrl.wait_entered();
        assert!(rx.try_recv().is_err());
        done_ctrl.release();
        assert!(matches!(recv(&rx).kind, TaskEventKind::Completed { .. }));
        q.finish().expect("finish");
    }

    #[test]
    fn url_task_without_fetcher_fails_with_actionable_message() {
        let (q, h) = build(Cfg {
            no_fetcher: true,
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, url_task(), Source::UrlImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let expected = ytdlp_missing_message();
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Failed(a, expected.to_string())));
        assert!(events.iter().any(|e| e.task_id == a
            && matches!(&e.kind, TaskEventKind::Failed { message } if message == &expected)));
        // Started was emitted before the failure.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Started))
        );
    }

    #[test]
    fn url_task_with_fetcher_downloads_then_transcribes() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            fetch_steps: vec![
                FetchProgress {
                    downloaded: 10,
                    total: None,
                },
                FetchProgress {
                    downloaded: 50,
                    total: Some(100),
                },
                FetchProgress {
                    downloaded: 100,
                    total: Some(100),
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, url_task(), Source::UrlImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // DownloadProgress only when the total is known.
        let downloads: Vec<f32> = events
            .iter()
            .filter_map(|e| match e.kind {
                TaskEventKind::DownloadProgress(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(downloads, vec![0.5, 1.0]);

        // update_file recorded with the fetched path; the decoder saw the same.
        let fetched = h.decoded.lock().expect("decoded").clone();
        assert_eq!(fetched.len(), 1);
        let local = fetched[0].clone();
        assert!(local.ends_with("audio.wav"));
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::UpdateFile(a, local)));
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(x, _) if *x == a))
        );
        assert_eq!(h.fetch_calls.lock().expect("fetch").len(), 1);
    }

    #[test]
    fn adopt_runs_a_row_that_already_exists_without_creating_it() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            ..Cfg::default()
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("clip.wav");
        std::fs::write(&input, b"bytes").expect("write");
        let id = TranscriptionId::new();
        q.adopt(id, path_task(input));
        q.finish().expect("finish");
        let calls = h.store_calls();
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, StoreCall::Create(_))),
            "{calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, StoreCall::Started(x) if *x == id))
        );
    }

    #[test]
    fn stage_emits_queued_without_starting_and_abandon_cancels() {
        let (q, h) = build(Cfg::default());
        let rx = q.subscribe();
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("clip.wav");
        std::fs::write(&input, b"bytes").expect("write");
        let id = TranscriptionId::new();
        q.stage(id, path_task(input), Source::FolderWatch);
        let queued = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("queued event");
        assert!(matches!(queued.kind, TaskEventKind::Queued));
        assert_eq!(queued.task_id, id);
        q.abandon(id);
        let canceled = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("canceled event");
        assert!(matches!(canceled.kind, TaskEventKind::Canceled));
        q.finish().expect("finish");
        let calls = h.store_calls();
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, StoreCall::Create(x) if *x == id))
        );
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, StoreCall::Canceled(x) if *x == id))
        );
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, StoreCall::Started(_)))
        );
    }

    #[test]
    fn url_task_stores_the_raw_title_and_keeps_the_url_as_the_cli_label() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            fetch_title: Some("C:/x".to_string()),
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        let spec = url_task();
        assert_eq!(spec.input.display_name(), "https://example.com/video");
        q.enqueue(a, spec, Source::UrlImport);
        q.finish().expect("finish");
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::SetName(a, "C:/x".to_string())));
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, StoreCall::SetName(_, name) if name == "C__x"))
        );
    }

    #[test]
    fn url_task_download_progress_fraction_is_clamped_to_one() {
        // A low/stale `total` estimate (e.g. from a server's
        // `Content-Length` on a 206 response) can let `downloaded` exceed
        // `total`; the `DownloadProgress` doc promises `0.0..=1.0`.
        let (q, _h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            fetch_steps: vec![FetchProgress {
                downloaded: 150,
                total: Some(100),
            }],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, url_task(), Source::UrlImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let downloads: Vec<f32> = events
            .iter()
            .filter_map(|e| match e.kind {
                TaskEventKind::DownloadProgress(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(downloads, vec![1.0]);
    }

    #[test]
    fn progress_writes_are_throttled_to_whole_percent_but_events_are_not() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![0.111, 0.112, 0.113, 0.25],
                segments: vec![],
            }],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // Store writes only when the whole percent changes (11%, 25%).
        let pcts: Vec<i64> = h
            .store_calls()
            .iter()
            .filter_map(|c| match c {
                StoreCall::Progress(x, f) if *x == a => Some((*f * 100.0).floor() as i64),
                _ => None,
            })
            .collect();
        assert_eq!(pcts, vec![11, 25]);

        // Every progress callback forwarded an event.
        let progress: Vec<f32> = events
            .iter()
            .filter_map(|e| match e.kind {
                TaskEventKind::Progress(f) => Some(f),
                _ => None,
            })
            .collect();
        assert_eq!(progress, vec![0.111, 0.112, 0.113, 0.25]);
    }

    #[test]
    fn finish_drains_the_queue_before_stopping() {
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }

    #[test]
    fn engine_is_reused_for_same_model_and_recreated_when_it_changes() {
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
            ],
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        let c = TranscriptionId::new();
        let mut spec_a = path_task(h.input_file("a.mp3"));
        let mut spec_b = path_task(h.input_file("b.mp3"));
        let mut spec_c = path_task(h.input_file("c.mp3"));
        spec_a.options = options_for(ModelSize::Small);
        spec_b.options = options_for(ModelSize::Small);
        spec_c.options = options_for(ModelSize::Base);
        q.enqueue(a, spec_a, Source::FileImport);
        q.enqueue(b, spec_b, Source::FileImport);
        q.enqueue(c, spec_c, Source::FileImport);
        q.finish().expect("finish");

        let created = h.created.lock().expect("created").clone();
        // Small built once (reused for B), then Base built once.
        assert_eq!(
            created,
            vec![
                ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Base,
                    hf_repo: None,
                },
            ]
        );
    }

    #[test]
    fn cancel_all_cancels_the_current_task_and_every_pending_task() {
        let (engine_ctrl, engine_gate) = gate();
        let (q, h) = build(Cfg {
            // Only A reaches the engine; B and C are canceled before starting.
            scripts: vec![EngineScript::Block(engine_gate)],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        let c = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        q.enqueue(c, path_task(h.input_file("c.mp3")), Source::FileImport);
        // A is running; cancel everything.
        engine_ctrl.wait_entered();
        q.cancel_all();
        // A's token trips, so releasing the engine yields a canceled result.
        engine_ctrl.release();
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        // All three end canceled.
        assert!(calls.contains(&StoreCall::Canceled(a)));
        assert!(calls.contains(&StoreCall::Canceled(b)));
        assert!(calls.contains(&StoreCall::Canceled(c)));
        // Neither pending task ever started.
        assert!(
            !calls
                .iter()
                .any(|c2| matches!(c2, StoreCall::Started(x) if *x == b))
        );
        assert!(
            !calls
                .iter()
                .any(|c2| matches!(c2, StoreCall::Started(x) if *x == c))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Started))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == c && matches!(e.kind, TaskEventKind::Started))
        );
        // Nothing completed or failed.
        assert!(
            !calls
                .iter()
                .any(|c2| matches!(c2, StoreCall::Completed(..) | StoreCall::Failed(..)))
        );
    }

    #[test]
    fn cancel_all_is_one_shot_tasks_enqueued_afterward_still_run() {
        let (engine_ctrl, engine_gate) = gate();
        let (q, h) = build(Cfg {
            // A blocks (canceled mid-run); B is canceled while pending; C, which
            // is enqueued *after* cancel_all, must run to completion.
            scripts: vec![
                EngineScript::Block(engine_gate),
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("c")],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        let c = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        engine_ctrl.wait_entered();
        q.cancel_all();
        engine_ctrl.release();
        // Enqueued strictly after cancel_all (which is already in the channel):
        // the one-shot semantics must not cancel it.
        q.enqueue(c, path_task(h.input_file("c.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Canceled(a)));
        assert!(calls.contains(&StoreCall::Canceled(b)));
        // C ran to completion and was never canceled.
        assert!(
            calls
                .iter()
                .any(|x| matches!(x, StoreCall::Completed(id, _) if *id == c))
        );
        assert!(!calls.contains(&StoreCall::Canceled(c)));
        assert!(
            events
                .iter()
                .any(|e| e.task_id == c && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == c && matches!(e.kind, TaskEventKind::Canceled))
        );
    }

    #[test]
    fn create_task_failure_fails_that_task_without_wedging_the_queue() {
        let (q, h) = build(Cfg {
            // The first create_task errors; the second succeeds.
            fail_create: true,
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![],
            }],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // A's create failed: a Failed event, never Queued or Started.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. }))
        );
        assert!(
            !events.iter().any(|e| e.task_id == a
                && matches!(e.kind, TaskEventKind::Queued | TaskEventKind::Started))
        );
        // B still runs to completion — the queue did not wedge.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        let calls = h.store_calls();
        assert!(
            calls
                .iter()
                .any(|x| matches!(x, StoreCall::Completed(id, _) if *id == b))
        );
        // A's create never recorded (it errored before recording).
        assert!(!calls.contains(&StoreCall::Create(a)));
    }

    #[test]
    fn engine_panic_fails_the_task_and_queue_survives() {
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Panic,
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("ok")],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        // If the panic escaped the runner thread, B would never run: `finish`
        // would still return `Ok` (it logs a join panic and moves on), but B's
        // events would never arrive and this call would hang until the
        // `drain` timeout. Reaching past `finish` with B completed is itself
        // evidence the unwind never left the runner thread.
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        assert!(calls.iter().any(|c| matches!(
            c,
            StoreCall::Failed(x, m)
                if *x == a && m.contains("task panicked") && m.contains("engine exploded")
        )));
        assert!(events.iter().any(|e| e.task_id == a
            && matches!(&e.kind, TaskEventKind::Failed { message }
                if message.contains("task panicked") && message.contains("engine exploded"))));
        assert!(
            events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }

    #[test]
    fn cancel_of_running_task_leaves_no_stale_canceled_id() {
        let (engine_ctrl, engine_gate) = gate();
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Block(engine_gate),
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("b")],
                },
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("c")],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        let c = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        // A is now executing in the engine; cancel it and let it terminate.
        engine_ctrl.wait_entered();
        q.cancel(a);
        engine_ctrl.release();
        // Wait for A's own Canceled event before proceeding — `events_until`
        // blocks (or times out) until it sees one, so reaching past this line
        // is itself the assertion that A actually terminated.
        events_until(&rx, |e| {
            e.task_id == a && matches!(e.kind, TaskEventKind::Canceled)
        });

        // Re-canceling an id whose task has already terminated targets
        // nothing (no task is pending or current with this id) — a no-op.
        q.cancel(a);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        q.enqueue(c, path_task(h.input_file("c.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        // A was written canceled exactly once — the post-termination
        // `cancel(a)` triggered no second write.
        assert_eq!(
            calls
                .iter()
                .filter(|c| matches!(c, StoreCall::Canceled(x) if *x == a))
                .count(),
            1
        );
        // B and C — enqueued fresh, unrelated ids — ran to completion and
        // were never canceled: a leftover `canceled` entry for the
        // already-terminated `a` cannot affect them.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| e.task_id == c && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            !calls
                .iter()
                .any(|c2| matches!(c2, StoreCall::Canceled(x) if *x == b || *x == c))
        );
    }

    // -- Retry (Task 3: retry/rename/notes/delete plumbing, F-026) ----------

    #[test]
    fn retry_after_cancel_of_running_task_runs_to_completion() {
        // A is canceled *while running* — Task 1's tail-drain in `run_task`
        // consumes its canceled-set entry at terminal time, so this exercises
        // the "no leftover entry" path (unlike the terminated-task case
        // below, which exercises the *other* stale-entry source: canceling
        // an id that never gets dequeued again). Either way, `retry` must
        // still purge the canceled-set before enqueueing: without that purge
        // this test would only pass by accident (the tail-drain already
        // cleaned up), so it alone doesn't prove the purge is needed — the
        // `_terminated_task_` test below is the one that actually requires it.
        let (engine_ctrl, engine_gate) = gate();
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Block(engine_gate),
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("retry-ok")],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let spec = path_task(h.input_file("a.mp3"));
        q.enqueue(a, spec.clone(), Source::FileImport);
        engine_ctrl.wait_entered();
        q.cancel(a);
        engine_ctrl.release();
        events_until(&rx, |e| {
            e.task_id == a && matches!(e.kind, TaskEventKind::Canceled)
        });

        q.retry(a, spec);
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Requeue(a)));
        let started_count = calls
            .iter()
            .filter(|c| matches!(c, StoreCall::Started(id) if *id == a))
            .count();
        assert_eq!(started_count, 2, "A must start again after the retry");
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(id, _) if *id == a)),
            "A must complete on retry, not stay canceled: {calls:?}"
        );
        // Requeue happens strictly after the first run's Canceled write.
        let canceled_idx = calls
            .iter()
            .position(|c| matches!(c, StoreCall::Canceled(id) if *id == a))
            .expect("first run canceled");
        let requeue_idx = calls
            .iter()
            .position(|c| matches!(c, StoreCall::Requeue(id) if *id == a))
            .expect("requeue recorded");
        assert!(canceled_idx < requeue_idx);

        let a_kinds = kinds(&events, a);
        assert!(matches!(
            a_kinds.last(),
            Some(TaskEventKind::Completed { .. })
        ));
    }

    #[test]
    fn retry_after_cancel_of_terminated_task_still_runs() {
        // A fails normally (no cancellation involved), then `cancel(a)` is
        // called *after* A already reached a terminal state: no task is
        // pending or current with this id, so — per
        // `cancel_of_running_task_leaves_no_stale_canceled_id` — that insert
        // is never consumed and would sit in the canceled-set forever. If
        // `retry`'s runner arm didn't purge it before enqueueing, dequeuing
        // the retried task would see `id` in the canceled-set and skip
        // straight to `Canceled` without ever running — this is exactly the
        // Task 1 review finding the brief calls out.
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Fail,
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("retry-ok")],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let spec = path_task(h.input_file("a.mp3"));
        q.enqueue(a, spec.clone(), Source::FileImport);
        events_until(&rx, |e| {
            e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. })
        });

        // Stale insert: A is already terminal, so this targets nothing.
        q.cancel(a);
        q.retry(a, spec);
        q.finish().expect("finish");
        let events = drain(&rx);

        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Requeue(a)));
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(id, _) if *id == a)),
            "the retry must complete, not be swallowed by the stale cancel: {calls:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| matches!(c, StoreCall::Canceled(id) if *id == a))
                .count(),
            0,
            "A must never be written canceled in this scenario"
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Canceled))
        );
    }

    #[test]
    fn retry_requeues_same_id_and_runs() {
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Fail,
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("retry-ok")],
                },
            ],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let spec = path_task(h.input_file("a.mp3"));
        q.enqueue(a, spec.clone(), Source::FileImport);
        // `events_until` drains the channel through the first run's Failed
        // event, so those events must be folded into `events` below
        // alongside whatever `drain` collects after the retry — otherwise
        // they'd be lost rather than just consumed for synchronization.
        let mut events = events_until(&rx, |e| {
            e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. })
        });

        q.retry(a, spec);
        q.finish().expect("finish");
        events.extend(drain(&rx));

        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Requeue(a)));
        let started_count = calls
            .iter()
            .filter(|c| matches!(c, StoreCall::Started(id) if *id == a))
            .count();
        assert_eq!(started_count, 2);
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(id, _) if *id == a))
        );

        // Full per-id event order: Queued, Started, Failed (first run), then
        // a fresh Queued, Started, ..., Completed for the retry.
        let a_kinds = kinds(&events, a);
        let first_failed = a_kinds
            .iter()
            .position(|k| matches!(k, TaskEventKind::Failed { .. }))
            .expect("first run failed");
        assert!(matches!(a_kinds[first_failed + 1], TaskEventKind::Queued));
        assert!(matches!(
            a_kinds.last(),
            Some(TaskEventKind::Completed { .. })
        ));
    }

    #[test]
    fn retry_requeue_store_failure_emits_failed() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Fail],
            fail_requeue: true,
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let spec = path_task(h.input_file("a.mp3"));
        q.enqueue(a, spec.clone(), Source::FileImport);
        // See the identical comment in `retry_requeues_same_id_and_runs`:
        // fold the pre-retry events back in rather than losing them.
        let mut events = events_until(&rx, |e| {
            e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. })
        });

        q.retry(a, spec);
        q.finish().expect("finish");
        events.extend(drain(&rx));

        // requeue_task itself failed: no Requeue call recorded, the task
        // never runs again (no second Started/Completed), and a Failed
        // event fires for the retry attempt (mirrors the create_task
        // failure arm).
        let calls = h.store_calls();
        assert!(!calls.iter().any(|c| matches!(c, StoreCall::Requeue(_))));
        let started_count = calls
            .iter()
            .filter(|c| matches!(c, StoreCall::Started(id) if *id == a))
            .count();
        assert_eq!(started_count, 1);
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(id, _) if *id == a))
        );
        let a_kinds = kinds(&events, a);
        assert_eq!(
            a_kinds
                .iter()
                .filter(|k| matches!(k, TaskEventKind::Failed { .. }))
                .count(),
            2,
            "first-run Failed plus the retry's requeue-failure Failed: {a_kinds:?}"
        );
    }

    #[test]
    fn retry_and_cancel_racing_the_tail_drain_purge_leaves_the_retry_canceled() {
        // Regression test for the tail-drain purge refinement (final-review
        // fix 6): `run_task`'s tail used to remove `id` from the
        // canceled-set unconditionally after its own post-terminal drain. A
        // `Retry(id)` immediately followed by a `Cancel(id)`, both landing in
        // that exact drain window, re-queues `id` (the `Retry` handler's own
        // purge clears any *stale* entry first) and then re-inserts it (the
        // `Cancel`) — but the unconditional removal right after would erase
        // that fresh insert, silently un-canceling the very retry the caller
        // just asked to cancel, letting it run to completion instead.
        //
        // Gate the first run's `mark_completed` store write (which runs
        // *before* the tail-drain) so the test can land both commands in
        // that window deterministically, with no reliance on thread timing.
        let (done_ctrl, done_gate) = gate();
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("first-run")],
            }],
            completed_gate: Some(done_gate),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let spec = path_task(h.input_file("a.mp3"));
        q.enqueue(a, spec.clone(), Source::FileImport);
        assert!(matches!(recv(&rx).kind, TaskEventKind::Queued));
        assert!(matches!(recv(&rx).kind, TaskEventKind::Started));
        assert!(matches!(recv(&rx).kind, TaskEventKind::SegmentAdded(_)));

        // The runner is inside the gated `mark_completed` for the first
        // run — strictly before `run_task`'s own tail-drain has run.
        done_ctrl.wait_entered();
        q.retry(a, spec);
        q.cancel(a);
        done_ctrl.release();

        // The first run's own Completed event still fires (its store write
        // already happened before the gate).
        assert!(matches!(recv(&rx).kind, TaskEventKind::Completed { .. }));

        q.finish().expect("finish");
        let events = drain(&rx);
        let calls = h.store_calls();

        assert!(
            calls.contains(&StoreCall::Requeue(a)),
            "the retry must have been requeued: {calls:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| matches!(c, StoreCall::Completed(id, _) if *id == a))
                .count(),
            1,
            "the retry must not run to completion a second time: {calls:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| matches!(c, StoreCall::Started(id) if *id == a))
                .count(),
            1,
            "the retried run must never actually start — it should be pre-canceled at \
             dequeue: {calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Canceled(id) if *id == a)),
            "the retry must end canceled: {calls:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Canceled)),
            "expected a Canceled event for the retried run: {events:?}"
        );
    }

    #[test]
    fn engine_panic_discards_cached_engine() {
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Panic,
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
            ],
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        let mut spec_a = path_task(h.input_file("a.mp3"));
        let mut spec_b = path_task(h.input_file("b.mp3"));
        spec_a.options = options_for(ModelSize::Small);
        spec_b.options = options_for(ModelSize::Small);
        q.enqueue(a, spec_a, Source::FileImport);
        q.enqueue(b, spec_b, Source::FileImport);
        q.finish().expect("finish");

        let created = h.created.lock().expect("created").clone();
        // Same model for both, but the factory was asked twice: the engine
        // that panicked mid-A was discarded, not reused for B.
        assert_eq!(
            created,
            vec![
                ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
            ]
        );
    }

    #[test]
    fn engine_panic_before_acquisition_still_discards_cached_engine() {
        // A (Small) completes normally and caches its engine. B is a URL
        // task whose *fetch* panics — `fetch_url` runs before `run_job` ever
        // reaches the cached-engine check, so the panic unwinds with A's
        // engine still sitting in `engine_cache`, untouched by anything
        // `run_job` did for B. This isolates the unconditional
        // `engine_cache = None` on panic from the "panic happened while
        // holding the engine" case already covered by
        // `engine_panic_discards_cached_engine`. C (Small) must then rebuild
        // rather than silently reuse A's stale cached engine.
        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![],
                },
            ],
            fetch_panics: true,
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        let c = TranscriptionId::new();
        let mut spec_a = path_task(h.input_file("a.mp3"));
        spec_a.options = options_for(ModelSize::Small);
        let mut spec_b = url_task();
        spec_b.options = options_for(ModelSize::Small);
        let mut spec_c = path_task(h.input_file("c.mp3"));
        spec_c.options = options_for(ModelSize::Small);
        q.enqueue(a, spec_a, Source::FileImport);
        q.enqueue(b, spec_b, Source::UrlImport);
        q.enqueue(c, spec_c, Source::FileImport);
        q.finish().expect("finish");

        let created = h.created.lock().expect("created").clone();
        // Two creates, not one: B never reaches engine acquisition at all
        // (it panics in the fetch), so it contributes nothing here; A and C
        // both had to build their own engine — without the explicit clear,
        // C would have silently reused A's cached engine and this vec would
        // have only A's entry.
        assert_eq!(
            created,
            vec![
                ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
            ]
        );
    }

    #[test]
    fn store_write_failure_emits_store_write_failed_event() {
        let (q, h) = build(Cfg {
            fail_started: true,
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("ok")],
            }],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(events.iter().any(|e| e.task_id == a
            && matches!(&e.kind, TaskEventKind::StoreWriteFailed { operation, .. }
                if operation == "mark_started")));
        // Policy: a store-write failure never aborts the run — the task still
        // completes.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }

    // -- ArtifactEnsurer (Task 2: in-task model ensure) -----------------------

    #[test]
    fn ensurer_model_progress_is_emitted_per_task_between_started_and_progress() {
        // Steps are chosen so the expected emissions are unambiguous under
        // `ModelDownloadThrottle`: 10% and 10.4% share the same whole
        // percent (10) — the second is throttled away — while 42% is a new
        // whole percent and fires.
        let ensurer = FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from(vec![EnsureModelScript::Ok {
                progress: vec![
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 10,
                        total: Some(100),
                    },
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 104,
                        total: Some(1000),
                    },
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 420,
                        total: Some(1000),
                    },
                ],
                path: PathBuf::from("/models/ggml-small.bin"),
            }]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::new(Mutex::new(Vec::new())),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: None,
        };
        let (q, h) = build_with_ensurer(
            Cfg {
                scripts: vec![EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("hi")],
                }],
                ..Cfg::default()
            },
            Arc::new(ensurer),
        );
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let a_kinds = kinds(&events, a);
        assert!(
            matches!(
                a_kinds.as_slice(),
                [
                    TaskEventKind::Queued,
                    TaskEventKind::Started,
                    TaskEventKind::ModelDownloadProgress {
                        downloaded: 10,
                        total: Some(100),
                        ..
                    },
                    TaskEventKind::ModelDownloadProgress {
                        downloaded: 420,
                        total: Some(1000),
                        ..
                    },
                    TaskEventKind::SegmentAdded(_),
                    TaskEventKind::Completed { .. },
                ]
            ),
            "unexpected event sequence (10.4% should have been throttled away): {a_kinds:?}"
        );
    }

    #[test]
    fn model_download_progress_events_are_throttled_to_whole_percent_or_full_mib() {
        // Pins `ModelDownloadThrottle`'s two branches in one scripted
        // download: a known-`total` run of steps (10%, 10.4% suppressed,
        // 42%) followed by an unknown-`total` run of steps (first MiB
        // fires, a same-MiB follow-up is suppressed, the next MiB fires).
        let ensurer = FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from(vec![EnsureModelScript::Ok {
                progress: vec![
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 10,
                        total: Some(100),
                    },
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 104,
                        total: Some(1000),
                    },
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 420,
                        total: Some(1000),
                    },
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 500_000,
                        total: None,
                    },
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 600_000,
                        total: None,
                    },
                    DownloadProgress {
                        file: "ggml-small.bin".to_string(),
                        downloaded: 1_200_000,
                        total: None,
                    },
                ],
                path: PathBuf::from("/models/ggml-small.bin"),
            }]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::new(Mutex::new(Vec::new())),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: None,
        };
        let (q, h) = build_with_ensurer(Cfg::default(), Arc::new(ensurer));
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let downloads: Vec<(u64, Option<u64>)> = events
            .iter()
            .filter_map(|e| match &e.kind {
                TaskEventKind::ModelDownloadProgress {
                    downloaded, total, ..
                } => Some((*downloaded, *total)),
                _ => None,
            })
            .collect();
        assert_eq!(
            downloads,
            vec![
                (10, Some(100)),
                (420, Some(1000)),
                (500_000, None),
                (1_200_000, None),
            ],
            "10.4% (same whole percent as 10%) and the second sub-MiB step must be throttled away"
        );
    }

    #[test]
    fn ensurer_model_failure_fails_task_with_ensurer_message() {
        let ensurer = FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from(vec![EnsureModelScript::Err(
                CoreError::Model(ModelError::Http {
                    detail: "connection refused".to_string(),
                }),
            )]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::new(Mutex::new(Vec::new())),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: None,
        };
        let (q, h) = build_with_ensurer(Cfg::default(), Arc::new(ensurer));
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(events.iter().any(|e| e.task_id == a
            && matches!(&e.kind, TaskEventKind::Failed { message } if message.contains("connection refused"))));
        let calls = h.store_calls();
        assert!(calls.iter().any(|c| matches!(
            c,
            StoreCall::Failed(x, m) if *x == a && m.contains("connection refused")
        )));
        // No engine was ever built — the ensure failed before engine
        // acquisition.
        assert!(h.created.lock().expect("created").is_empty());
    }

    #[test]
    fn ensurer_vad_result_overrides_tuning_per_job_and_none_degrades() {
        let vad_path = PathBuf::from("/models/vad/silero.bin");
        let model_path_a = PathBuf::from("/models/ggml-small.bin");
        let model_path_b = PathBuf::from("/models/ggml-base.bin");
        let model_calls = Arc::new(Mutex::new(Vec::new()));
        let vad_calls = Arc::new(Mutex::new(0));
        let ensurer = FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from(vec![
                EnsureModelScript::Ok {
                    progress: vec![],
                    path: model_path_a.clone(),
                },
                EnsureModelScript::Ok {
                    progress: vec![],
                    path: model_path_b.clone(),
                },
            ]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::from(vec![
                Some(vad_path.clone()),
                None,
            ]))),
            model_calls: Arc::clone(&model_calls),
            vad_calls: Arc::clone(&vad_calls),
            model_gate: None,
        };
        let (q, h) = build_with_ensurer(
            Cfg {
                scripts: vec![
                    EngineScript::Emit {
                        progress: vec![],
                        segments: vec![],
                    },
                    EngineScript::Emit {
                        progress: vec![],
                        segments: vec![],
                    },
                ],
                ..Cfg::default()
            },
            Arc::new(ensurer),
        );
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        // Different model sizes so the engine cache misses for both — each
        // task drives its own `create` call (and thus its own `resolved`).
        let mut spec_a = path_task(h.input_file("a.mp3"));
        let mut spec_b = path_task(h.input_file("b.mp3"));
        spec_a.options = options_for(ModelSize::Small);
        spec_b.options = options_for(ModelSize::Base);
        q.enqueue(a, spec_a, Source::FileImport);
        q.enqueue(b, spec_b, Source::FileImport);
        q.finish().expect("finish");

        // ensure_model/ensure_vad each ran once per task.
        assert_eq!(model_calls.lock().expect("model calls").len(), 2);
        assert_eq!(*vad_calls.lock().expect("vad calls"), 2);

        // engines.create received the model just ensured for each task.
        assert_eq!(
            h.resolved_calls(),
            vec![Some(model_path_a), Some(model_path_b)]
        );

        // The per-job VAD override reached the pipeline: `Some` for A,
        // `None` (degraded) for B — despite both tasks sharing the same
        // base `EngineTuning` (default: no VAD).
        assert_eq!(h.vad_seen(), vec![Some(vad_path), None]);
    }

    #[test]
    fn ensurer_cache_hit_still_ensures_model_but_skips_create() {
        let model_calls = Arc::new(Mutex::new(Vec::new()));
        let ensurer = FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from(vec![
                EnsureModelScript::Ok {
                    progress: vec![],
                    path: PathBuf::from("/models/ggml-small.bin"),
                },
                EnsureModelScript::Ok {
                    progress: vec![],
                    path: PathBuf::from("/models/ggml-small.bin"),
                },
            ]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::clone(&model_calls),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: None,
        };
        let (q, h) = build_with_ensurer(
            Cfg {
                scripts: vec![
                    EngineScript::Emit {
                        progress: vec![],
                        segments: vec![],
                    },
                    EngineScript::Emit {
                        progress: vec![],
                        segments: vec![],
                    },
                ],
                ..Cfg::default()
            },
            Arc::new(ensurer),
        );
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        // Same (default) model for both — the engine cache hits for B.
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.enqueue(b, path_task(h.input_file("b.mp3")), Source::FileImport);
        q.finish().expect("finish");

        // ensure_model ran for both tasks — the artifact stays fresh even on
        // a cache hit.
        assert_eq!(model_calls.lock().expect("model calls").len(), 2);
        // ...but engines.create ran only once: the cache hit skipped it.
        assert_eq!(h.created.lock().expect("created").len(), 1);
    }

    #[test]
    fn ensurer_cancel_during_ensure_yields_canceled_not_failed() {
        let (ensure_ctrl, ensure_gate) = gate();
        let ensurer = FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from(vec![EnsureModelScript::Err(
                CoreError::Canceled,
            )]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::new(Mutex::new(Vec::new())),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: Some(ensure_gate),
        };
        let (q, h) = build_with_ensurer(Cfg::default(), Arc::new(ensurer));
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        // The runner is inside `ensure_model`, blocked on the gate.
        ensure_ctrl.wait_entered();
        // A is the current task, so this trips its token directly.
        q.cancel(a);
        ensure_ctrl.release();
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Canceled))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. }))
        );
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Canceled(a)));
        assert!(!calls.iter().any(|c| matches!(c, StoreCall::Failed(..))));
    }

    #[test]
    fn ensurer_model_error_wrapping_canceled_yields_canceled_not_failed() {
        // A real (downloader-backed) ArtifactEnsurer wraps `ModelDownloader`,
        // whose own `Result<_, ModelError>` surfaces cancellation as
        // `ModelError::Canceled` — `?`/`.into()` on that produces
        // `CoreError::Model(ModelError::Canceled)`, distinct from the bare
        // `CoreError::Canceled` the other cancel test uses. `is_canceled`
        // must treat this the same way: Canceled, never Failed.
        let (ensure_ctrl, ensure_gate) = gate();
        let ensurer = FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from(vec![EnsureModelScript::Err(
                CoreError::from(ModelError::Canceled),
            )]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::new(Mutex::new(Vec::new())),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: Some(ensure_gate),
        };
        let (q, h) = build_with_ensurer(Cfg::default(), Arc::new(ensurer));
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        ensure_ctrl.wait_entered();
        q.cancel(a);
        ensure_ctrl.release();
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Canceled))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. }))
        );
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Canceled(a)));
        assert!(!calls.iter().any(|c| matches!(c, StoreCall::Failed(..))));
    }

    #[test]
    fn no_ensurer_behaves_exactly_as_before() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // No ModelDownloadProgress events fire without an ArtifactEnsurer.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.kind, TaskEventKind::ModelDownloadProgress { .. }))
        );
        // The factory always sees `resolved = None` — the CLI pre-resolves
        // the model itself and its factory ignores this parameter, but the
        // runner must still pass `None` when no ArtifactEnsurer configured.
        assert_eq!(h.resolved_calls(), vec![None]);
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }

    // -- F-031: per-task LLM translation through the queue -------------------
    //
    // Two of these five bind TCP (they drive the real translator worker
    // against a `TestServer`) and so join this task's documented
    // sandbox-blocked set — named in the task report. The other three are
    // network-free and pass under the sandbox.

    /// A test [`LlmEndpointSource`] that hands back a fixed endpoint, or
    /// `None` (modeling "no key available anywhere").
    struct FixedEndpoint {
        auth: Option<OpenAiAuth>,
    }

    impl LlmEndpointSource for FixedEndpoint {
        fn resolve(&self) -> Option<LlmEndpoint> {
            self.auth.clone().map(|auth| LlmEndpoint { auth })
        }
    }

    /// A canned `{"choices":[{"message":{"content": "..."}}]}` chat-completion
    /// body (mirrors `openai::translate`'s worker-test helper of the same
    /// name).
    fn choice_response(content: &str) -> Vec<u8> {
        serde_json::json!({ "choices": [ { "message": { "content": content } } ] })
            .to_string()
            .into_bytes()
    }

    /// A file [`TaskSpec`] whose options opt into LLM translation (F-031).
    fn llm_task_spec(input: PathBuf) -> TaskSpec {
        TaskSpec {
            input: Input::Path(input),
            options: TranscriptionOptions {
                llm: Some(LlmTranslationOptions {
                    model: "gpt-4o-mini".to_string(),
                    prompt: "Translate.".to_string(),
                }),
                ..TranscriptionOptions::default()
            },
            output: OutputSpec::default(),
        }
    }

    /// Pulls the segments handed to `mark_completed` for `id` out of the
    /// recorded store calls. The queue's `FakeStore` records exactly what the
    /// real store's `mark_completed` would persist — the `translation` column
    /// included (see the store's own `mark_completed_persists_segment_translations`).
    fn completed_segments(h: &Harness, id: TranscriptionId) -> Vec<Segment> {
        h.store_calls()
            .into_iter()
            .find_map(|c| match c {
                StoreCall::Completed(x, segs) if x == id => Some(segs),
                _ => None,
            })
            .expect("task completed")
    }

    /// TCP (`TestServer::start`) — sandbox-blocked; run with the sandbox
    /// lifted. End-to-end through the runner: a scripted engine emits two
    /// segments, the translator resolves against a `TestServer`, and the
    /// persisted segments carry the per-segment translations mapped to the
    /// *correct* ordinal — not just "some" translation, which a merge bug
    /// that swapped row0/row1 could still satisfy.
    ///
    /// The translator worker's batch composition is nondeterministic by
    /// design (the worker is already parked in `recv()` when the first
    /// segment is enqueued, so the second segment may or may not have
    /// arrived by the time the worker checks for more before dispatching):
    /// both segments can land in ONE numbered-batch request, or as TWO
    /// single-item requests. This test scripts the `/chat/completions` route
    /// with a two-response QUEUE (Task 4's `set_route_for_method`, served in
    /// call order) and branches on how many requests actually landed,
    /// asserting composition-appropriate, ordinal-aware expectations either
    /// way — both branches would catch a swapped merge:
    ///
    /// - 1 request (batched): the numbered-batch protocol parses
    ///   `"[1] uno\n[2] dos"` into exactly `["uno", "dos"]`, zipped with the
    ///   batch in enqueue order, so row0 must be "uno" and row1 "dos". The
    ///   single recorded request's user-message content must mention both
    ///   segment texts.
    /// - 2 requests (split): the single-item form has no `[N]` parsing — it
    ///   uses the whole response content verbatim — so the queue's first
    ///   response (the literal string `"[1] uno\n[2] dos"`) goes to whichever
    ///   request the worker sent first, and the second response ("SECOND")
    ///   to whichever it sent second. The worker is a single thread issuing
    ///   one blocking request at a time, so request arrival order == enqueue
    ///   order == segment ordinal order: row0 must be the literal
    ///   `"[1] uno\n[2] dos"` and row1 must be "SECOND". The request bodies
    ///   are also checked directly: request 0's user content must be segment
    ///   0's text ("one"), request 1's segment 1's ("two").
    #[test]
    fn file_task_with_llm_stores_per_segment_translations() {
        let server = TestServer::start();
        server.set_route_for_method(
            "POST",
            "/chat/completions",
            Canned::ok(choice_response("[1] uno\n[2] dos")),
        );
        server.set_route_for_method(
            "POST",
            "/chat/completions",
            Canned::ok(choice_response("SECOND")),
        );
        let source: Arc<dyn LlmEndpointSource> = Arc::new(FixedEndpoint {
            auth: Some(OpenAiAuth {
                base_url: Some(server.base_url()),
                api_key: "sk-test".to_string(),
            }),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("one"), seg("two")],
            }],
            llm: Some(source),
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        q.enqueue(a, llm_task_spec(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");

        let segments = completed_segments(&h, a);
        assert_eq!(segments.len(), 2);

        let requests = server.requests();
        match requests.len() {
            1 => {
                // Batched composition (see doc comment): one numbered-batch
                // request carrying both segment texts.
                let body: serde_json::Value =
                    serde_json::from_slice(&requests[0].body).expect("request body is JSON");
                let user = body["messages"][1]["content"]
                    .as_str()
                    .expect("user message content is a string");
                assert!(user.contains("one"), "batched user content: {user:?}");
                assert!(user.contains("two"), "batched user content: {user:?}");
                assert_eq!(segments[0].translation, "uno");
                assert_eq!(segments[1].translation, "dos");
            }
            2 => {
                // Split composition (see doc comment): two single-item
                // requests; arrival order == enqueue order == ordinal order.
                let body0: serde_json::Value =
                    serde_json::from_slice(&requests[0].body).expect("request 0 body is JSON");
                let body1: serde_json::Value =
                    serde_json::from_slice(&requests[1].body).expect("request 1 body is JSON");
                assert_eq!(
                    body0["messages"][1]["content"].as_str(),
                    Some("one"),
                    "request 0 should carry segment 0's text"
                );
                assert_eq!(
                    body1["messages"][1]["content"].as_str(),
                    Some("two"),
                    "request 1 should carry segment 1's text"
                );
                assert_eq!(segments[0].translation, "[1] uno\n[2] dos");
                assert_eq!(segments[1].translation, "SECOND");
            }
            n => panic!("expected 1 (batched) or 2 (split) /chat/completions requests, got {n}"),
        }
    }

    /// TCP (`TestServer::start`) — sandbox-blocked; run with the sandbox
    /// lifted. Every `/chat/completions` returns 500: every item degrades to
    /// "" (Buzz parity) and the task still Completes.
    #[test]
    fn llm_batch_error_yields_empty_translations_and_completed_task() {
        let server = TestServer::start();
        server.set_route_for_method("POST", "/chat/completions", Canned::status(500));
        let source: Arc<dyn LlmEndpointSource> = Arc::new(FixedEndpoint {
            auth: Some(OpenAiAuth {
                base_url: Some(server.base_url()),
                api_key: "sk-test".to_string(),
            }),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("one"), seg("two")],
            }],
            llm: Some(source),
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        q.enqueue(a, llm_task_spec(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");

        let segments = completed_segments(&h, a);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].translation, "");
        assert_eq!(segments[1].translation, "");
    }

    /// Network-free (passes sandboxed): `resolve()` returns `None`, so no
    /// translator is spawned; the task completes untranslated (a warning is
    /// logged) rather than being gated on translation.
    #[test]
    fn no_key_resolves_none_task_completes_untranslated_with_warning() {
        let source: Arc<dyn LlmEndpointSource> = Arc::new(FixedEndpoint { auth: None });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("one"), seg("two")],
            }],
            llm: Some(source),
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        q.enqueue(a, llm_task_spec(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");

        let segments = completed_segments(&h, a);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].translation, "");
        assert_eq!(segments[1].translation, "");
    }

    /// Network-free (passes sandboxed): a translator IS spawned (the endpoint
    /// resolves), but the engine trips the cancel token as its last act, so
    /// by the time the pipeline reaches the drain, `cancel` is already
    /// canceled — `TranslatorHandle::stop(Some(cancel))`'s poll-join (M7,
    /// design spec §14) sees that on its very first check, before ever
    /// waiting on the join, and returns `false` immediately, which the
    /// pipeline turns into `Canceled` (design spec §6) — never merging or
    /// persisting any translation. Deterministic; the endpoint points at a
    /// dead loopback port so the detached worker's lone request can never
    /// reach a real service, and the test never waits on that worker.
    #[test]
    fn cancel_during_translation_drain_yields_canceled() {
        let source: Arc<dyn LlmEndpointSource> = Arc::new(FixedEndpoint {
            auth: Some(OpenAiAuth {
                base_url: Some("http://127.0.0.1:1".to_string()),
                api_key: "sk-test".to_string(),
            }),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::EmitThenCancel {
                segments: vec![seg("one")],
            }],
            llm: Some(source),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, llm_task_spec(h.input_file("a.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // The task ends Canceled — never Completed — so no segment rows (and
        // therefore no translations) are ever persisted.
        let calls = h.store_calls();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Canceled(x) if *x == a))
        );
        assert!(!calls.iter().any(|c| matches!(c, StoreCall::Completed(..))));
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Canceled))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }

    /// Network-free (passes sandboxed): an `ArtifactEnsurer` is configured,
    /// but the task's backend is `OpenAiApi` — which has no local model/VAD to
    /// resolve — so the whole ensure phase is skipped (design spec §2.1): zero
    /// ensure calls, no `ModelDownloadProgress` events, and the engine factory
    /// is handed `resolved: None`. The task still runs to completion.
    #[test]
    fn openaiapi_task_skips_model_ensure_and_vad() {
        let model_calls = Arc::new(Mutex::new(Vec::new()));
        let vad_calls = Arc::new(Mutex::new(0usize));
        let ensurer = Arc::new(FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::new())),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::clone(&model_calls),
            vad_calls: Arc::clone(&vad_calls),
            model_gate: None,
        });
        let (q, h) = build_with_ensurer(
            Cfg {
                scripts: vec![EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("hi")],
                }],
                ..Cfg::default()
            },
            ensurer,
        );
        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let mut spec = path_task(h.input_file("a.mp3"));
        spec.options = TranscriptionOptions {
            model: ModelSpec {
                backend: Backend::OpenAiApi,
                size: ModelSize::Small,
                hf_repo: None,
            },
            ..TranscriptionOptions::default()
        };
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // The ensurer was configured but never consulted for this backend.
        assert!(model_calls.lock().expect("model calls").is_empty());
        assert_eq!(*vad_calls.lock().expect("vad calls"), 0);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.kind, TaskEventKind::ModelDownloadProgress { .. }))
        );
        // The task still ran to completion, and the factory saw `resolved =
        // None` (no ensured path for the API backend).
        let calls = h.store_calls();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(x, _) if *x == a))
        );
        assert_eq!(h.resolved_calls(), vec![None]);
    }

    // -- finish_within (ADR-022) ----------------------------------------

    #[test]
    fn finish_within_returns_true_when_drained() {
        let (q, _h) = build(Cfg::default());
        assert!(matches!(q.finish_within(Duration::from_secs(2)), Ok(true)));
    }

    #[test]
    fn finish_within_returns_true_after_tasks_complete() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);

        assert!(matches!(q.finish_within(Duration::from_secs(2)), Ok(true)));
        assert!(
            h.store_calls()
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(x, _) if *x == a))
        );
    }

    /// The case `finish_within` exists for (ADR-022): the engine blocks on
    /// a never-signaled channel — a scripted stand-in for a wedged
    /// `transcribe` call — so the runner thread can never drain. The call
    /// must still return `Ok(false)` on its own bounded deadline, well
    /// inside a generous wall-clock bound, rather than waiting on the
    /// engine's.
    ///
    /// `engine_ctrl` is deliberately **never released**: the runner thread
    /// stays parked on the gate for the rest of the test (this is the
    /// point — a leaked thread parked on a channel recv is exactly what
    /// "deliberately leaked" means), and the test never joins it either
    /// directly or indirectly. `engine_ctrl` going out of scope at the end
    /// of the test drops its `proceed_tx`, which will eventually unblock
    /// the parked engine call in the background (a disconnected `recv` on
    /// the gate's channel, same as any other dropped sender) — harmless,
    /// since by then this test has already made its assertions and the
    /// process-wide store/decoder/etc. the leaked thread still touches are
    /// kept alive by its own `Arc` clones, not by anything this test scope
    /// owns.
    #[test]
    fn finish_within_times_out_on_wedged_engine() {
        let (engine_ctrl, engine_gate) = gate();
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Block(engine_gate)],
            ..Cfg::default()
        });
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("a.mp3")), Source::FileImport);
        // Don't return until the engine has actually entered and blocked —
        // otherwise a slow scheduler could dequeue the task *after*
        // `finish_within` already gave up, which would not exercise the
        // wedged case at all.
        engine_ctrl.wait_entered();

        let start = std::time::Instant::now();
        let result = q.finish_within(Duration::from_millis(200));
        let elapsed = start.elapsed();

        assert!(matches!(result, Ok(false)));
        assert!(
            elapsed < Duration::from_secs(5),
            "finish_within took {elapsed:?} to give up — expected well under 5s"
        );
    }

    // -- F-115 check_skip seam ------------------------------------------

    /// A planted sibling `.txt` result file + a configured, enabled skip
    /// source: the engine must never run (scripted to panic if it does),
    /// the task ends `Skipped` with the parsed segments persisted, and the
    /// event sequence is `Queued` → `Started` → `Skipped`, with no
    /// `Completed` anywhere.
    #[test]
    fn skip_check_result_file_hit_bypasses_the_engine() {
        let (q, h) = build(Cfg {
            // If the engine were ever invoked despite the skip, this makes
            // the failure loud and unmistakable rather than silently
            // completing with the wrong segments.
            scripts: vec![EngineScript::Panic],
            postproc: Some(Arc::new(FixedPostprocSource(postproc_cfg_with_skip(
                SkipSettings {
                    enabled: true,
                    check_result_files: true,
                    check_database: false,
                },
            )))),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        std::fs::write(input.with_file_name("clip.txt"), "already transcribed")
            .expect("write sibling result file");

        let a = TranscriptionId::new();
        q.enqueue(a, path_task(input), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert_eq!(
            kinds(&events, a),
            vec![
                TaskEventKind::Queued,
                TaskEventKind::Started,
                TaskEventKind::Skipped,
            ]
        );
        assert!(
            h.created.lock().expect("created").is_empty(),
            "the engine factory must never be consulted for a skipped task"
        );
        let calls = h.store_calls();
        assert!(
            calls.iter().any(|c| matches!(
                c,
                StoreCall::Skipped(x, segs)
                    if *x == a
                        && segs.len() == 1
                        && segs[0].text == "already transcribed"
            )),
            "expected a Skipped store call with the parsed result-file segment: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| matches!(c, StoreCall::Completed(..))),
            "a skipped task must never also be marked completed"
        );
    }

    /// `RunnerDeps::postproc: None` (every pre-M7 caller, and most test
    /// doubles) must leave the engine path completely untouched — even
    /// with a sibling result file sitting right there, one that *would*
    /// trigger a skip if any source were configured.
    #[test]
    fn postproc_none_leaves_the_engine_path_unchanged() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("transcribed for real")],
            }],
            postproc: None,
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        std::fs::write(input.with_file_name("clip.txt"), "would have been skipped")
            .expect("write sibling result file");

        let a = TranscriptionId::new();
        q.enqueue(a, path_task(input), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert_eq!(h.created.lock().expect("created").len(), 1);
    }

    /// A postproc source is configured, but [`SkipSettings::enabled`] is
    /// `false`: the seam must not run at all, and the task transcribes
    /// normally despite the same sibling result file being present.
    #[test]
    fn skip_disabled_in_config_engine_still_runs() {
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("transcribed for real")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(postproc_cfg_with_skip(
                SkipSettings {
                    enabled: false,
                    check_result_files: true,
                    check_database: false,
                },
            )))),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        std::fs::write(input.with_file_name("clip.txt"), "would have been skipped")
            .expect("write sibling result file");

        let a = TranscriptionId::new();
        q.enqueue(a, path_task(input), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert_eq!(h.created.lock().expect("created").len(), 1);
    }

    // -- `on_complete` hook chain (F-116/F-111/F-113, design spec §2.1) -----

    /// Extracts the [`TaskEventKind::PostProcessed`] payload for `id` from a
    /// drained event stream, panicking if none is present.
    fn post_processed_for(
        events: &[TaskEvent],
        id: TranscriptionId,
    ) -> (bool, bool, Vec<PathBuf>, Vec<String>) {
        events
            .iter()
            .find_map(|e| {
                if e.task_id != id {
                    return None;
                }
                match &e.kind {
                    TaskEventKind::PostProcessed {
                        notes_updated,
                        segments_replaced,
                        written,
                        errors,
                    } => Some((
                        *notes_updated,
                        *segments_replaced,
                        written.clone(),
                        errors.clone(),
                    )),
                    _ => None,
                }
            })
            .expect("a PostProcessed event was expected but none was found")
    }

    /// A word-timestamped [`TaskSpec`] over `input` (every on_complete-chain
    /// test needs `word_timestamps: true` for the auto-resizer's own gate 1
    /// to pass).
    fn word_timestamped_task(input: PathBuf) -> TaskSpec {
        let mut spec = path_task(input);
        spec.options.word_timestamps = true;
        spec
    }

    /// `resize` replaces the store's segments *before* `docx` re-reads them
    /// (design spec §2.1's whole reason for the fixed order): observed via
    /// the store-call sequence — resize's own `segments_for` read, then its
    /// `replace_task_segments` write, then docx's *separate* `segments_for`
    /// re-read — never the other way around.
    #[test]
    fn on_complete_chain_runs_resize_before_docx_rereads_segments() {
        let out_dir = tempfile::tempdir().expect("out tempdir");
        let cfg = postproc_cfg_with_hooks(
            AutoResizeSettings {
                enabled: true,
                ..AutoResizeSettings::default()
            },
            AiSummarySettings::default(), // disabled: not part of this test
            ExportDocxSettings {
                enabled: true,
                output_folder: Some(out_dir.path().to_path_buf()),
                ..ExportDocxSettings::default()
            },
        );

        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hello world")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            initial_segments: vec![seg("hello there.")],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        let a = TranscriptionId::new();
        q.enqueue(a, word_timestamped_task(input), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let (_, segments_replaced, written, errors) = post_processed_for(&events, a);
        assert!(segments_replaced, "resize should have replaced segments");
        assert_eq!(
            written.len(),
            1,
            "docx should have written one file: {written:?}"
        );
        assert!(
            errors.is_empty(),
            "neither hook should have failed: {errors:?}"
        );

        let calls = h.store_calls();
        let segments_for_positions: Vec<usize> = calls
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, StoreCall::SegmentsFor(x) if *x == a))
            .map(|(i, _)| i)
            .collect();
        let replace_position = calls
            .iter()
            .position(|c| matches!(c, StoreCall::ReplaceSegments(x, _) if *x == a))
            .expect("resize should have called replace_task_segments");

        assert_eq!(
            segments_for_positions.len(),
            2,
            "resize reads once and docx re-reads once: {calls:?}"
        );
        assert!(
            segments_for_positions[0] < replace_position,
            "resize's own read must precede its own write: {calls:?}"
        );
        assert!(
            replace_position < segments_for_positions[1],
            "docx's re-read must observe resize's write, not race ahead of it: {calls:?}"
        );
    }

    /// One `PostProcessed` event aggregates all three hooks' independent
    /// outcomes: the auto-resizer's `segments_replaced`, `export_docx`'s
    /// `written` path, and a genuine (network-free) ai_summary failure in
    /// `errors` — simultaneously. The summary failure is produced by a
    /// syntactically invalid `api_url` (`"this is not a url"`): `reqwest`
    /// rejects it while building the request, entirely offline (no socket
    /// is ever opened), matching this task's "network-free" requirement
    /// while still exercising a *real* `postproc::summary::run` failure
    /// path (not a fake/mocked one).
    #[test]
    fn post_processed_event_aggregates_resize_summary_docx_outcomes() {
        let out_dir = tempfile::tempdir().expect("out tempdir");
        let cfg = postproc_cfg_with_hooks(
            AutoResizeSettings {
                enabled: true,
                ..AutoResizeSettings::default()
            },
            AiSummarySettings {
                enabled: true,
                api_url: Some("this is not a url".to_string()),
                ..AiSummarySettings::default()
            },
            ExportDocxSettings {
                enabled: true,
                output_folder: Some(out_dir.path().to_path_buf()),
                ..ExportDocxSettings::default()
            },
        );

        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hello world")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            initial_segments: vec![seg("hello there.")],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        let a = TranscriptionId::new();
        q.enqueue(a, word_timestamped_task(input), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let (notes_updated, segments_replaced, written, errors) = post_processed_for(&events, a);
        assert!(segments_replaced, "resize should have replaced segments");
        assert_eq!(
            written.len(),
            1,
            "docx should still have written its file: {written:?}"
        );
        assert!(
            !notes_updated,
            "summary errored before reaching the notes write"
        );
        assert_eq!(
            errors.len(),
            1,
            "summary's malformed-URL failure should be the only recorded error: {errors:?}"
        );
    }

    /// A `cfg` with every post-hook disabled must emit no `PostProcessed`
    /// event at all (design spec §2.3/T2 contract) — even though a
    /// `PostprocConfigSource` is configured.
    #[test]
    fn no_hooks_enabled_emits_no_post_processed_event() {
        let cfg = postproc_cfg_with_hooks(
            AutoResizeSettings::default(), // enabled: false
            AiSummarySettings::default(),  // enabled: false
            ExportDocxSettings::default(), // enabled: false
        );

        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hello world")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(input), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::PostProcessed { .. })),
            "no hook enabled must never emit PostProcessed: {events:?}"
        );
    }

    /// `Completed` always precedes `PostProcessed` in the event stream
    /// (design spec §2.1: hooks run *after* `mark_completed`+`Completed`).
    #[test]
    fn completed_always_precedes_post_processed() {
        let cfg = postproc_cfg_with_hooks(
            AutoResizeSettings {
                enabled: true,
                ..AutoResizeSettings::default()
            },
            AiSummarySettings::default(),
            ExportDocxSettings::default(),
        );

        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hello world")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            initial_segments: vec![seg("hello there.")],
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        let a = TranscriptionId::new();
        q.enqueue(a, word_timestamped_task(input), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let completed_pos = events
            .iter()
            .position(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
            .expect("Completed expected");
        let post_pos = events
            .iter()
            .position(|e| e.task_id == a && matches!(e.kind, TaskEventKind::PostProcessed { .. }))
            .expect("PostProcessed expected");
        assert!(
            completed_pos < post_pos,
            "Completed must precede PostProcessed: {events:?}"
        );
    }

    /// A cancel landed *between* hooks (right after resize's write, before
    /// summary would start) skips every remaining hook, but the aggregated
    /// event still reports resize's own result — and the task's status
    /// stays `Completed` (it already was, design spec §2.2). Also proves
    /// `current` must stay populated through the whole hook chain: this
    /// only passes because `QueueHandle::cancel`'s prompt-cancel fast path
    /// can still find and trip this task's token *while a hook is running*.
    #[test]
    fn cancel_between_hooks_skips_remaining_hooks_but_keeps_resize_result() {
        let out_dir = tempfile::tempdir().expect("out tempdir");
        let (control, gate) = gate();
        let cfg = postproc_cfg_with_hooks(
            AutoResizeSettings {
                enabled: true,
                ..AutoResizeSettings::default()
            },
            AiSummarySettings {
                enabled: true,
                // A syntactically invalid URL: if the cancel-between-hooks
                // check ever regressed and let this hook run anyway, it
                // would fail offline rather than actually reaching the
                // network — defense in depth, not the behavior under test.
                api_url: Some("this is not a url".to_string()),
                ..AiSummarySettings::default()
            },
            ExportDocxSettings {
                enabled: true,
                output_folder: Some(out_dir.path().to_path_buf()),
                ..ExportDocxSettings::default()
            },
        );

        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hello world")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            initial_segments: vec![seg("hello there.")],
            replace_gate: Some(gate),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        let a = TranscriptionId::new();
        q.enqueue(a, word_timestamped_task(input), Source::FileImport);

        // The runner is now blocked inside resize's `replace_task_segments`
        // -- `current` still names this task (design spec §2.2's whole
        // point), so this reaches the token directly rather than merely
        // queuing a canceled-set entry nothing would consume until a next
        // dequeue that never comes.
        control.wait_entered();
        q.cancel(a);
        control.release();

        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. })),
            "the task must stay Completed -- it already was before the hook chain ran"
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Canceled)),
            "a cancel mid-chain must not retroactively cancel the already-completed task"
        );
        let (notes_updated, segments_replaced, written, errors) = post_processed_for(&events, a);
        assert!(
            segments_replaced,
            "resize already finished before the cancel was observed"
        );
        assert!(
            written.is_empty(),
            "docx must be skipped after the cancel: {written:?}"
        );
        assert!(!notes_updated, "summary must be skipped after the cancel");
        assert!(
            errors.is_empty(),
            "skipped hooks contribute no errors: {errors:?}"
        );
    }

    /// A cancel landed *before* the chain even starts (the tiny window
    /// between `run_job` returning `Completed` and `run_on_complete_chain`
    /// running its first gate) must skip every hook, including resize —
    /// symmetry with the between-hooks checks (design spec §2.2). Uses
    /// `completed_gate` (fires inside `mark_completed`, strictly before the
    /// `Completed` event and the hook chain) rather than `replace_gate`
    /// (which fires *inside* resize, too late to test "before the first
    /// hook").
    #[test]
    fn cancel_before_chain_starts_skips_every_hook() {
        let (control, gate) = gate();
        let cfg = postproc_cfg_with_hooks(
            AutoResizeSettings {
                enabled: true,
                ..AutoResizeSettings::default()
            },
            AiSummarySettings {
                enabled: true,
                api_url: Some("this is not a url".to_string()),
                ..AiSummarySettings::default()
            },
            ExportDocxSettings {
                enabled: true,
                ..ExportDocxSettings::default()
            },
        );

        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hello world")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            initial_segments: vec![seg("hello there.")],
            completed_gate: Some(gate),
            ..Cfg::default()
        });
        let rx = q.subscribe();
        let input = h.input_file("clip.mp3");
        let a = TranscriptionId::new();
        q.enqueue(a, word_timestamped_task(input), Source::FileImport);

        // Blocked inside `mark_completed` -- strictly before the `Completed`
        // event and before the hook chain's first gate check even runs.
        control.wait_entered();
        q.cancel(a);
        control.release();

        q.finish().expect("finish");
        let events = drain(&rx);

        let (notes_updated, segments_replaced, written, errors) = post_processed_for(&events, a);
        assert!(
            !segments_replaced,
            "resize must be skipped when already canceled before the chain starts"
        );
        assert!(!notes_updated);
        assert!(written.is_empty());
        assert!(errors.is_empty());

        let calls = h.store_calls();
        assert!(
            !calls.iter().any(|c| matches!(
                c,
                StoreCall::SegmentsFor(_) | StoreCall::ReplaceSegments(..)
            )),
            "resize must never touch the store when already canceled: {calls:?}"
        );
    }

    /// A hook that panics (a defect this milestone didn't anticipate, e.g.
    /// a misbehaving `TaskStore`) is contained exactly like `run_job`'s own
    /// panic containment: the panic message lands in the aggregated
    /// event's `errors`, the runner thread survives, `current` is cleared
    /// (a subsequently enqueued task still runs to completion), and the
    /// panicking task itself still ends up `Completed` (it already was
    /// before the chain started).
    #[test]
    fn on_complete_chain_hook_panic_is_contained_runner_survives_and_next_task_runs() {
        let cfg = postproc_cfg_with_hooks(
            AutoResizeSettings {
                enabled: true,
                ..AutoResizeSettings::default()
            },
            AiSummarySettings::default(), // disabled: isolates the panic to resize
            ExportDocxSettings::default(), // disabled
        );

        let (q, h) = build(Cfg {
            scripts: vec![
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("hello world")],
                },
                EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("second task")],
                },
            ],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            initial_segments: vec![seg("hello there.")],
            panic_in_replace_task_segments: true,
            ..Cfg::default()
        });
        let rx = q.subscribe();

        let input_a = h.input_file("clip.mp3");
        let a = TranscriptionId::new();
        q.enqueue(a, word_timestamped_task(input_a), Source::FileImport);

        // Task b has `word_timestamps: false` (plain `path_task`), so its
        // own resize hook exits at gate 1 before ever calling
        // `replace_task_segments` -- it must complete normally, proving the
        // runner thread survived task a's hook panic.
        let input_b = h.input_file("clip2.mp3");
        let b = TranscriptionId::new();
        q.enqueue(b, path_task(input_b), Source::FileImport);

        q.finish().expect("finish");
        let events = drain(&rx);

        let (_, segments_replaced, written, errors) = post_processed_for(&events, a);
        assert!(
            !segments_replaced,
            "the panicking write must not be counted as a successful replace"
        );
        assert!(written.is_empty());
        assert_eq!(
            errors.len(),
            1,
            "the panic must be the only recorded error: {errors:?}"
        );
        assert!(
            errors[0].contains("replace_task_segments boom"),
            "the panic payload's message should be actionable: {errors:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. })),
            "task a must still end up Completed -- it already was before the chain ran"
        );

        assert!(
            events
                .iter()
                .any(|e| e.task_id == b && matches!(e.kind, TaskEventKind::Completed { .. })),
            "task b must still run to completion -- the runner thread must have survived \
             task a's hook panic: {events:?}"
        );
    }

    // -- `hook_error_after_cancel_check` (F-111 cancel-mid-hook semantics) --

    #[test]
    fn hook_error_after_cancel_check_drops_the_error_once_canceled() {
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            hook_error_after_cancel_check(Some("boom".to_string()), &cancel),
            None,
            "a cancellation observed after the hook call is not a hook failure"
        );
        assert_eq!(hook_error_after_cancel_check(None, &cancel), None);
    }

    #[test]
    fn hook_error_after_cancel_check_keeps_the_error_when_not_canceled() {
        let cancel = CancelToken::new();
        assert_eq!(
            hook_error_after_cancel_check(Some("boom".to_string()), &cancel),
            Some("boom".to_string())
        );
        assert_eq!(hook_error_after_cancel_check(None, &cancel), None);
    }

    // ======================================================================
    // M8: pre-transcription seam (F-027/F-112/F-114, design spec §3/§5)
    // ======================================================================

    #[test]
    fn extract_speech_swaps_decode_input_and_emits_phase() {
        let isolator_scripts = Arc::new(Mutex::new(VecDeque::new()));
        let isolator_calls = Arc::new(Mutex::new(Vec::new()));
        let isolator = Arc::new(ScriptedIsolator {
            scripts: Arc::clone(&isolator_scripts),
            calls: Arc::clone(&isolator_calls),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            isolator: Some(isolator),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("a.mp3");
        let isolated = input.with_file_name("a_speech.mp3");
        // The isolated file must actually exist: `run_file_job`'s
        // input-exists check (M8, `audio_input` not `job.input`) is real
        // production behavior, not something this test should route around.
        std::fs::write(&isolated, b"fake isolated audio").expect("write isolated file");
        isolator_scripts
            .lock()
            .expect("isolator scripts")
            .push_back(IsolateScript::Ok(isolated.clone()));

        let mut spec = path_task(input.clone());
        spec.options.extract_speech = true;
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events.iter().any(|e| e.task_id == a
                && matches!(
                    e.kind,
                    TaskEventKind::PhaseChanged {
                        phase: TaskPhase::IsolatingSpeech
                    }
                )),
            "expected an IsolatingSpeech phase event: {events:?}"
        );
        assert_eq!(*isolator_calls.lock().expect("isolator calls"), vec![input]);
        assert_eq!(
            h.decoded.lock().expect("decoded").clone(),
            vec![isolated],
            "the decoder must see the isolated path, not the original input"
        );
        assert!(
            h.store_calls()
                .iter()
                .any(|c| matches!(c, StoreCall::Completed(x, _) if *x == a))
        );
    }

    #[test]
    fn isolation_failure_no_audio_proceeds_on_original() {
        let isolator = Arc::new(ScriptedIsolator {
            scripts: Arc::new(Mutex::new(VecDeque::from([IsolateScript::Err(
                MediaError::DecodeFailed {
                    detail: "no audio stream".into(),
                },
            )]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            isolator: Some(isolator),
            // Call 1 (the isolation-failure handler's own probe) sees
            // `has_audio: false` and degrades gracefully; call 2+
            // (`run_file_job`'s later probe of the unmodified original)
            // sees `true` and transcribes it normally — see
            // `FakeProber::no_audio_for_first_n_calls`'s doc comment.
            prober_no_audio_for_first_n_calls: 1,
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("silent.mp4");
        let mut spec = path_task(input.clone());
        spec.options.extract_speech = true;
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // Degrades to the original file instead of failing (Buzz's
        // `IndexError -> "no_audio"` arm).
        assert_eq!(h.decoded.lock().expect("decoded").clone(), vec![input]);
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. }))
        );
    }

    #[test]
    fn isolation_failure_with_audio_fails_task_actionably() {
        let isolator = Arc::new(ScriptedIsolator {
            scripts: Arc::new(Mutex::new(VecDeque::from([IsolateScript::Err(
                MediaError::DecodeFailed {
                    detail: "demucs exited with status 1".into(),
                },
            )]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            isolator: Some(isolator),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        let mut spec = path_task(input);
        spec.options.extract_speech = true;
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let msg = events
            .iter()
            .find_map(|e| match &e.kind {
                TaskEventKind::Failed { message } if e.task_id == a => Some(message.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected a Failed event: {events:?}"));
        let lower = msg.to_lowercase();
        assert!(
            lower.contains("demucs"),
            "message should name demucs: {msg}"
        );
        assert!(
            lower.contains("download") || lower.contains("model"),
            "message should mention the possible first-run model download: {msg}"
        );
        // Never reaches the decoder/engine.
        assert!(h.decoded.lock().expect("decoded").is_empty());
    }

    #[test]
    fn isolation_cancel_yields_canceled_not_failed() {
        // A cancellation mid-isolation must end the task `Canceled`, never
        // `Failed` with the (misleading, in this case) actionable-demucs
        // message — self-review finding: the no-audio-vs-actionable-failure
        // classification must not shadow an in-flight cancel.
        let isolator = Arc::new(ScriptedIsolator {
            scripts: Arc::new(Mutex::new(VecDeque::from([IsolateScript::Cancel]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            isolator: Some(isolator),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let mut spec = path_task(h.input_file("clip.mp3"));
        spec.options.extract_speech = true;
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Canceled)),
            "expected a Canceled event: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. })),
            "a cancellation must never surface as Failed: {events:?}"
        );
        assert!(h.decoded.lock().expect("decoded").is_empty());
    }

    #[test]
    fn isolation_output_deleted_on_completed_and_on_failed() {
        // -- Completed path --
        {
            let isolator_scripts = Arc::new(Mutex::new(VecDeque::new()));
            let isolator = Arc::new(ScriptedIsolator {
                scripts: Arc::clone(&isolator_scripts),
                calls: Arc::new(Mutex::new(Vec::new())),
            });
            let (q, h) = build(Cfg {
                scripts: vec![EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("hi")],
                }],
                isolator: Some(isolator),
                ..Cfg::default()
            });
            let rx = q.subscribe();
            let a = TranscriptionId::new();
            let input = h.input_file("a.mp3");
            let isolated = input.with_file_name("a_speech.mp3");
            std::fs::write(&isolated, b"fake isolated audio").expect("write isolated file");
            isolator_scripts
                .lock()
                .expect("isolator scripts")
                .push_back(IsolateScript::Ok(isolated.clone()));

            let mut spec = path_task(input);
            spec.options.extract_speech = true;
            q.enqueue(a, spec, Source::FileImport);
            q.finish().expect("finish");
            let events = drain(&rx);

            assert!(
                events
                    .iter()
                    .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
            );
            assert!(
                !isolated.exists(),
                "isolated file must be cleaned up after Completed"
            );
        }

        // -- Failed path (engine fails after isolation succeeds) --
        {
            let isolator_scripts = Arc::new(Mutex::new(VecDeque::new()));
            let isolator = Arc::new(ScriptedIsolator {
                scripts: Arc::clone(&isolator_scripts),
                calls: Arc::new(Mutex::new(Vec::new())),
            });
            let (q, h) = build(Cfg {
                scripts: vec![EngineScript::Fail],
                isolator: Some(isolator),
                ..Cfg::default()
            });
            let rx = q.subscribe();
            let a = TranscriptionId::new();
            let input = h.input_file("b.mp3");
            let isolated = input.with_file_name("b_speech.mp3");
            std::fs::write(&isolated, b"fake isolated audio").expect("write isolated file");
            isolator_scripts
                .lock()
                .expect("isolator scripts")
                .push_back(IsolateScript::Ok(isolated.clone()));

            let mut spec = path_task(input);
            spec.options.extract_speech = true;
            q.enqueue(a, spec, Source::FileImport);
            q.finish().expect("finish");
            let events = drain(&rx);

            assert!(
                events
                    .iter()
                    .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Failed { .. }))
            );
            assert!(
                !isolated.exists(),
                "isolated file must be cleaned up after Failed"
            );
        }
    }

    /// Review fix (Important 3): an isolator that echoes its input path back
    /// unchanged — exactly `ScriptedIsolator`'s own no-script fallback arm,
    /// and a plausible shape for a real no-op/misconfigured adapter — must
    /// never have that path recorded for `TempAudioGuard` cleanup, or the
    /// guard would delete the user's own source file on task completion.
    #[test]
    fn temp_guard_does_not_delete_isolator_echoed_input() {
        let isolator = Arc::new(ScriptedIsolator {
            scripts: Arc::new(Mutex::new(VecDeque::new())), // no script -> echoes input
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            isolator: Some(isolator),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        let mut spec = path_task(input.clone());
        spec.options.extract_speech = true;
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            input.exists(),
            "the isolator echoed the input path back unchanged — it must \
             never be deleted as though it were a genuine derivative"
        );
    }

    /// Review fix (Important 3): same identity check, for the reducer.
    #[test]
    fn temp_guard_does_not_delete_reducer_echoed_input() {
        let reducer = Arc::new(ScriptedReducer {
            scripts: Arc::new(Mutex::new(VecDeque::new())), // no script -> echoes input
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings {
                enabled: true,
                keep_denoised_file: false,
            },
            LangDetectSettings::default(),
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            reducer: Some(reducer),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        q.enqueue(a, path_task(input.clone()), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            input.exists(),
            "the reducer echoed its input path back unchanged — it must \
             never be deleted as though it were a genuine derivative"
        );
    }

    #[test]
    fn reducer_failure_proceeds_on_prior_audio() {
        let reducer = Arc::new(ScriptedReducer {
            scripts: Arc::new(Mutex::new(VecDeque::from([ReduceScript::Err(
                MediaError::DecodeFailed {
                    detail: "boom".into(),
                },
            )]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings {
                enabled: true,
                keep_denoised_file: false,
            },
            LangDetectSettings::default(),
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            reducer: Some(reducer),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        q.enqueue(a, path_task(input.clone()), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert_eq!(h.decoded.lock().expect("decoded").clone(), vec![input]);
    }

    #[test]
    fn reducer_panic_is_contained() {
        let reducer = Arc::new(ScriptedReducer {
            scripts: Arc::new(Mutex::new(VecDeque::from([ReduceScript::Panic]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings {
                enabled: true,
                keep_denoised_file: false,
            },
            LangDetectSettings::default(),
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            reducer: Some(reducer),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        q.enqueue(a, path_task(input.clone()), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // The panic must not crash the runner thread nor fail the task.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert_eq!(h.decoded.lock().expect("decoded").clone(), vec![input]);
    }

    #[test]
    fn denoised_output_deleted_on_completed_by_default() {
        let reducer_scripts = Arc::new(Mutex::new(VecDeque::new()));
        let reducer = Arc::new(ScriptedReducer {
            scripts: Arc::clone(&reducer_scripts),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings {
                enabled: true,
                keep_denoised_file: false,
            },
            LangDetectSettings::default(),
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            reducer: Some(reducer),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        let denoised = input.with_file_name("clip_DeepFilterNet3.wav");
        std::fs::write(&denoised, b"fake denoised audio").expect("write denoised file");
        reducer_scripts
            .lock()
            .expect("reducer scripts")
            .push_back(ReduceScript::Ok(denoised.clone()));

        q.enqueue(a, path_task(input), Source::FileImport);
        q.finish().expect("finish");
        let _events = drain(&rx);

        assert!(
            !denoised.exists(),
            "denoised file must be cleaned up by default"
        );
    }

    #[test]
    fn denoised_output_kept_when_keep_denoised_file_set() {
        let reducer_scripts = Arc::new(Mutex::new(VecDeque::new()));
        let reducer = Arc::new(ScriptedReducer {
            scripts: Arc::clone(&reducer_scripts),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings {
                enabled: true,
                keep_denoised_file: true,
            },
            LangDetectSettings::default(),
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            reducer: Some(reducer),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        let denoised = input.with_file_name("clip_DeepFilterNet3.wav");
        std::fs::write(&denoised, b"fake denoised audio").expect("write denoised file");
        reducer_scripts
            .lock()
            .expect("reducer scripts")
            .push_back(ReduceScript::Ok(denoised.clone()));

        q.enqueue(a, path_task(input), Source::FileImport);
        q.finish().expect("finish");
        let _events = drain(&rx);

        assert!(
            denoised.exists(),
            "denoised file must be kept when keep_denoised_file is set"
        );
    }

    #[test]
    fn detector_runs_only_when_language_none_or_english() {
        fn call_count(language: Option<Language>, enabled: bool) -> usize {
            let detector_calls = Arc::new(Mutex::new(Vec::new()));
            let detector = Arc::new(ScriptedDetector {
                scripts: Arc::new(Mutex::new(VecDeque::from([DetectScript::Ok(None)]))),
                calls: Arc::clone(&detector_calls),
            });
            let cfg = postproc_cfg_with_m8(
                NoiseReductionSettings::default(),
                LangDetectSettings {
                    enabled,
                    download_tiny_if_missing: false,
                },
            );
            let (q, h) = build(Cfg {
                scripts: vec![EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("hi")],
                }],
                lang_detector: Some(detector),
                postproc: Some(Arc::new(FixedPostprocSource(cfg))),
                ..Cfg::default()
            });
            h.seed_model(ModelSize::Small);

            let rx = q.subscribe();
            let a = TranscriptionId::new();
            let input = h.input_file("clip.mp3");
            let mut spec = path_task(input);
            spec.options.language = language;
            q.enqueue(a, spec, Source::FileImport);
            q.finish().expect("finish");
            let _events = drain(&rx);

            detector_calls.lock().expect("detector calls").len()
        }

        assert_eq!(
            call_count(None, true),
            1,
            "unset language must run detection"
        );
        assert_eq!(
            call_count(Language::parse("en"), true),
            1,
            "explicit English must still run detection (Buzz's en-quirk)"
        );
        assert_eq!(
            call_count(Language::parse("fr"), true),
            0,
            "an explicit non-English language must skip detection"
        );
        assert_eq!(
            call_count(None, false),
            0,
            "a disabled hook must skip detection even with language unset"
        );
    }

    #[test]
    fn detector_failure_proceeds_on_auto() {
        let detector = Arc::new(ScriptedDetector {
            scripts: Arc::new(Mutex::new(VecDeque::from([DetectScript::Err(
                EngineError::Inference {
                    detail: "boom".into(),
                },
            )]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings::default(),
            LangDetectSettings {
                enabled: true,
                download_tiny_if_missing: false,
            },
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            lang_detector: Some(detector),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });
        h.seed_model(ModelSize::Small);

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("clip.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // Detection failure never fails the task, and no language is
        // written to the store — the job proceeds on auto.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert!(
            !h.store_calls()
                .iter()
                .any(|c| matches!(c, StoreCall::UpdateLanguage(..)))
        );
        assert_eq!(h.language_seen(), vec![None]);
    }

    #[test]
    fn detector_panic_is_contained() {
        let detector = Arc::new(ScriptedDetector {
            scripts: Arc::new(Mutex::new(VecDeque::from([DetectScript::Panic]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings::default(),
            LangDetectSettings {
                enabled: true,
                download_tiny_if_missing: false,
            },
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            lang_detector: Some(detector),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });
        h.seed_model(ModelSize::Small);

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("clip.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // The panic must not crash the runner thread nor fail the task.
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
        assert_eq!(h.language_seen(), vec![None]);
    }

    #[test]
    fn detected_language_reaches_engine_export_name_and_store() {
        let detector = Arc::new(ScriptedDetector {
            scripts: Arc::new(Mutex::new(VecDeque::from([DetectScript::Ok(
                Language::parse("fr"),
            )]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings::default(),
            LangDetectSettings {
                enabled: true,
                download_tiny_if_missing: true,
            },
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("bonjour")],
            }],
            lang_detector: Some(detector),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            ..Cfg::default()
        });
        h.seed_model(ModelSize::Small);

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        let spec = TaskSpec {
            input: Input::Path(input),
            options: TranscriptionOptions::default(),
            output: OutputSpec {
                formats: [OutputFormat::Txt].into_iter().collect(),
                directory: Some(h.tmp.path().to_path_buf()),
                file_name_template: Some("{{ input_file_name }}-{{ language }}".to_string()),
                paragraph_split: Millis(2000),
                source_disposition: None,
            },
        };
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let _events = drain(&rx);

        assert_eq!(h.language_seen(), vec![Language::parse("fr")]);
        assert!(
            h.store_calls()
                .contains(&StoreCall::UpdateLanguage(a, "fr".to_string()))
        );
        let expected = h.tmp.path().join("clip-fr.txt");
        assert!(
            expected.exists(),
            "expected export named with the detected language at {expected:?}"
        );
    }

    /// Review fix (Important 2): the detected language must reach the
    /// `on_complete` hook chain too, not just the engine/export name —
    /// Buzz's own plugin mutates the shared `task.transcription_options`, so
    /// downstream hooks (F-116's transcript resizer, whose `regroup` step
    /// picks a CJK-vs-Latin word-join separator by language) see it. The
    /// task's own `options.language` stays unset (`word_timestamped_task`'s
    /// default); the detector resolves `ja` — a non-space language
    /// (`regroup::words::NON_SPACE_LANGUAGES`). Two un-punctuated,
    /// close-in-time segments ("hello"/"world") land in one `regroup`
    /// sentence group; the resizer's `replace_task_segments` write is
    /// observed directly, joined with `""` (a no-op separator, since the
    /// non-space language already glues them without one).
    #[test]
    fn detected_language_reaches_on_complete_chain_regroup() {
        let detector = Arc::new(ScriptedDetector {
            scripts: Arc::new(Mutex::new(VecDeque::from([DetectScript::Ok(
                Language::parse("ja"),
            )]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        });
        let cfg = PostprocConfig {
            skip: SkipSettings::default(),
            summary: AiSummarySettings::default(),
            summary_api_key: None,
            docx: ExportDocxSettings::default(),
            resize: AutoResizeSettings {
                enabled: true,
                ..AutoResizeSettings::default()
            },
            paragraph_split: Millis(2000),
            noise_reduction: NoiseReductionSettings::default(),
            language_detection: LangDetectSettings {
                enabled: true,
                download_tiny_if_missing: true,
            },
        };
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            lang_detector: Some(detector),
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            initial_segments: vec![
                Segment {
                    start: Millis(0),
                    end: Millis(400),
                    text: "hello".to_string(),
                    translation: String::new(),
                },
                Segment {
                    start: Millis(450),
                    end: Millis(800),
                    text: "world".to_string(),
                    translation: String::new(),
                },
            ],
            ..Cfg::default()
        });
        h.seed_model(ModelSize::Small);

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(
            a,
            word_timestamped_task(h.input_file("clip.mp3")),
            Source::FileImport,
        );
        q.finish().expect("finish");
        let _events = drain(&rx);

        assert_eq!(h.language_seen(), vec![Language::parse("ja")]);

        let replaced = h
            .store_calls()
            .into_iter()
            .find_map(|c| match c {
                StoreCall::ReplaceSegments(id, segs) if id == a => Some(segs),
                _ => None,
            })
            .expect("resize hook should have replaced segments");
        let joined: String = replaced.iter().map(|s| s.text.clone()).collect();
        assert!(
            joined.contains("helloworld"),
            "the on_complete chain must see the *detected* ja, not the \
             task's unset language, so regroup's word separator is \"\" \
             (non-space language) rather than \" \": {joined:?}"
        );
        assert!(
            !joined.contains("hello world"),
            "a space-joined result means the chain fell back to the task's \
             own (unset) language instead of the detected ja: {joined:?}"
        );
    }

    #[test]
    fn detector_absent_model_downloads_tiny_when_enabled() {
        let model_calls = Arc::new(Mutex::new(Vec::new()));
        let tiny_path = PathBuf::from("/fake/tiny-model.bin");
        let ensurer = Arc::new(FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::from([EnsureModelScript::Ok {
                progress: vec![DownloadProgress {
                    file: "ggml-tiny.bin".to_string(),
                    downloaded: 40,
                    total: Some(100),
                }],
                path: tiny_path.clone(),
            }]))),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::clone(&model_calls),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: None,
        });
        let detector_calls = Arc::new(Mutex::new(Vec::new()));
        let detector = Arc::new(ScriptedDetector {
            scripts: Arc::new(Mutex::new(VecDeque::from([DetectScript::Ok(None)]))),
            calls: Arc::clone(&detector_calls),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings::default(),
            LangDetectSettings {
                enabled: true,
                download_tiny_if_missing: true,
            },
        );
        let (q, h) = build_with_ensurer(
            Cfg {
                scripts: vec![EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("hi")],
                }],
                lang_detector: Some(detector),
                postproc: Some(Arc::new(FixedPostprocSource(cfg))),
                ..Cfg::default()
            },
            ensurer,
        );
        // No model seeded under `h.model_root()`: `select_model` finds
        // nothing, forcing the tiny-download fallback.

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("clip.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        let recorded = model_calls.lock().expect("model calls").clone();
        assert!(
            recorded.iter().any(|m| m.size == ModelSize::Tiny),
            "expected a Tiny model ensure call: {recorded:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e.kind, TaskEventKind::ModelDownloadProgress { .. })),
            "the tiny-model download's progress must surface as \
             ModelDownloadProgress, same as the task's own model ensure: \
             {events:?}"
        );
        assert_eq!(
            *detector_calls.lock().expect("detector calls"),
            vec![tiny_path],
            "the detector must run against exactly the path the ensurer \
             resolved for the downloaded tiny model"
        );
    }

    /// The gate test symmetric with the above: no model downloaded, but
    /// `download_tiny_if_missing` is off — detection must skip entirely
    /// rather than falling back to a download, with zero `ensure_model`
    /// calls and zero detector calls.
    #[test]
    fn detector_absent_model_and_tiny_download_disabled_skips_with_zero_ensure_calls() {
        let model_calls = Arc::new(Mutex::new(Vec::new()));
        let ensurer = Arc::new(FakeEnsurer {
            model_scripts: Arc::new(Mutex::new(VecDeque::new())),
            vad_scripts: Arc::new(Mutex::new(VecDeque::new())),
            model_calls: Arc::clone(&model_calls),
            vad_calls: Arc::new(Mutex::new(0)),
            model_gate: None,
        });
        let detector_calls = Arc::new(Mutex::new(Vec::new()));
        let detector = Arc::new(ScriptedDetector {
            scripts: Arc::new(Mutex::new(VecDeque::from([DetectScript::Ok(None)]))),
            calls: Arc::clone(&detector_calls),
        });
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings::default(),
            LangDetectSettings {
                enabled: true,
                download_tiny_if_missing: false,
            },
        );
        let (q, h) = build_with_ensurer(
            Cfg {
                scripts: vec![EngineScript::Emit {
                    progress: vec![],
                    segments: vec![seg("hi")],
                }],
                lang_detector: Some(detector),
                postproc: Some(Arc::new(FixedPostprocSource(cfg))),
                ..Cfg::default()
            },
            ensurer,
        );
        // No model seeded under `h.model_root()`.

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        q.enqueue(a, path_task(h.input_file("clip.mp3")), Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        // `build_with_ensurer` wires the same `ensurer` for the task's own
        // (unrelated) model resolution too — `path_task`'s default Whisper
        // backend always ensures its own `Small` model regardless of
        // language detection, so `model_calls` isn't expected to be empty.
        // What must never appear is a `Tiny` call: that's specifically
        // detection's own fallback, which must not fire here.
        let recorded = model_calls.lock().expect("model calls").clone();
        assert!(
            !recorded.iter().any(|m| m.size == ModelSize::Tiny),
            "no Tiny-model ensure call should happen when the tiny fallback \
             is disabled: {recorded:?}"
        );
        assert!(
            detector_calls.lock().expect("detector calls").is_empty(),
            "detection must be skipped entirely, not attempted with no model"
        );
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }

    #[test]
    fn hooks_never_run_when_deps_none() {
        let cfg = postproc_cfg_with_m8(
            NoiseReductionSettings {
                enabled: true,
                keep_denoised_file: false,
            },
            LangDetectSettings {
                enabled: true,
                download_tiny_if_missing: true,
            },
        );
        let (q, h) = build(Cfg {
            scripts: vec![EngineScript::Emit {
                progress: vec![],
                segments: vec![seg("hi")],
            }],
            postproc: Some(Arc::new(FixedPostprocSource(cfg))),
            // `isolator`/`reducer`/`lang_detector` all stay `None`
            // (`Cfg::default()`) even though every hook's own Preferences
            // flag is "on" — the seam must never run without a wired
            // collaborator (the `postproc: None` precedent, extended).
            ..Cfg::default()
        });

        let rx = q.subscribe();
        let a = TranscriptionId::new();
        let input = h.input_file("clip.mp3");
        let mut spec = path_task(input.clone());
        spec.options.extract_speech = true;
        q.enqueue(a, spec, Source::FileImport);
        q.finish().expect("finish");
        let events = drain(&rx);

        assert!(
            !events
                .iter()
                .any(|e| matches!(e.kind, TaskEventKind::PhaseChanged { .. }))
        );
        assert_eq!(h.decoded.lock().expect("decoded").clone(), vec![input]);
        assert!(
            !h.store_calls()
                .iter()
                .any(|c| matches!(c, StoreCall::UpdateLanguage(..)))
        );
        assert!(
            events
                .iter()
                .any(|e| e.task_id == a && matches!(e.kind, TaskEventKind::Completed { .. }))
        );
    }
}
