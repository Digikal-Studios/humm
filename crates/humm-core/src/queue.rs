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
//! - **Resizer-created rows are the documented exception (ADR-018 scope).**
//!   The queue owns `status`/`progress` for tasks it *executes*. The transcript
//!   resizer (F-086, design spec §4 "status-write boundary note") is the one
//!   feature that writes those columns outside the queue: its
//!   `copy_transcription` / `mark_replaced_completed` / `update_progress_direct`
//!   store calls create and complete rows that never enter this queue at all,
//!   so the "single writer / queue owns status transitions" invariant is scoped
//!   to queue-executed tasks and does not conflict with them.
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

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::boundary::{
    AudioDecoder, FetchProgress, MediaFetcher, MediaProber, SegmentSink, TaskStore,
    TranscriptionEngine,
};
use crate::cancel::CancelToken;
use crate::domain::{Backend, Input, ModelSpec, Segment, Source, TaskSpec, TranscriptionId};
use crate::error::{CoreError, EngineError, MediaError, ModelError, StoreError};
use crate::models::download::DownloadProgress;
use crate::openai::LlmEndpointSource;
use crate::pipeline::{EngineTuning, JobOutcome, run_file_job};

/// The message emitted to yt-dlp-less URL tasks: exactly this string is
/// written to the store and carried on the `Failed` event.
const YTDLP_MISSING: &str =
    "yt-dlp missing — install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP";

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
    /// Succeeded, with the pipeline outcome (segments + written files).
    Completed(JobOutcome),
    /// Canceled (cooperatively) during the run.
    Canceled,
    /// Failed, with an actionable message.
    Failed(String),
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
    /// Ensures model/VAD artifacts per job when configured (GUI). `None`
    /// (CLI) skips artifact-ensure entirely — see [`RunnerDeps::artifacts`].
    artifacts: Option<Arc<dyn ArtifactEnsurer>>,
    /// Resolves the per-task LLM-translation endpoint (F-031), threaded into
    /// [`run_file_job`]. `None` disables translation entirely — see
    /// [`RunnerDeps::llm`].
    llm: Option<Arc<dyn LlmEndpointSource>>,
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
        self.set_current(None);

        match terminal {
            Terminal::Completed(outcome) => {
                if let Err(err) = self.store.mark_completed(&id, &outcome.segments) {
                    self.store_write_failed(id, "mark_completed", &err);
                }
                self.emit(TaskEvent {
                    task_id: id,
                    kind: TaskEventKind::Completed {
                        written: outcome.written,
                    },
                });
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
        let input_path = match &spec.input {
            Input::Path(path) => path.clone(),
            Input::Url(url) => match self.fetch_url(id, url, token) {
                Ok(path) => path,
                Err(terminal) => return terminal,
            },
        };

        let model = spec.options.model.clone();
        let job = crate::domain::JobSpec {
            input: input_path,
            options: spec.options.clone(),
            output: spec.output.clone(),
        };

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
            tuning,
            &*self.decoder,
            &*self.prober,
            engine.as_mut(),
            &mut sink,
            token,
            self.llm.as_deref(),
        );

        self.engine_cache = Some((model, engine));

        match result {
            Ok(outcome) => Terminal::Completed(outcome),
            Err(err) => classify(err),
        }
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
        let subscribers = Arc::clone(&self.subscribers);
        let mut throttle = ModelDownloadThrottle::default();
        let mut on_progress = move |update: DownloadProgress| {
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
        };

        let resolved = ensurer
            .ensure_model(model, &mut on_progress, token)
            .map_err(classify)?;

        let mut tuning = self.tuning.clone();
        tuning.vad_model = ensurer.ensure_vad(&mut on_progress, token);

        Ok((resolved, tuning))
    }

    /// Downloads a URL input into `media_dir/<id>/`, emitting `DownloadProgress`
    /// events (only while the total is known) and recording the local path via
    /// `update_file`. Returns the local path, or a [`Terminal`] on failure.
    fn fetch_url(
        &self,
        id: TranscriptionId,
        url: &url::Url,
        token: &CancelToken,
    ) -> Result<PathBuf, Terminal> {
        let Some(fetcher) = self.fetcher.as_ref() else {
            return Err(Terminal::Failed(YTDLP_MISSING.to_string()));
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

        let local = match fetcher.fetch(url, &dest, &mut progress, token) {
            Ok(path) => path,
            Err(err) => return Err(classify(CoreError::from(err))),
        };

        if let Err(err) = self.store.update_file(&id, &local) {
            self.store_write_failed(id, "update_file", &err);
        }
        Ok(local)
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
        artifacts: deps.artifacts,
        llm: deps.llm,
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
        Backend, LlmTranslationOptions, Millis, ModelSize, OutputSpec, TranscriptionOptions,
    };
    use crate::error::{ModelError, StoreError};
    use crate::models::test_server::{Canned, TestServer};
    use crate::openai::{LlmEndpoint, OpenAiAuth};
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
        Completed(TranscriptionId, Vec<Segment>),
        Failed(TranscriptionId, String),
        Canceled(TranscriptionId),
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

    struct FakeProber;

    impl MediaProber for FakeProber {
        fn probe(&self, _path: &Path) -> Result<MediaInfo, MediaError> {
            Ok(MediaInfo {
                duration: Some(Millis(1000)),
                has_audio: true,
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
            }))
        }
    }

    // -- Fake fetcher: emits scripted progress, writes a dummy file ----------

    struct FakeFetcher {
        steps: Vec<FetchProgress>,
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
        ) -> Result<PathBuf, MediaError> {
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
            Ok(path)
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

    // -- Harness -------------------------------------------------------------

    struct Harness {
        store_calls: Arc<Mutex<Vec<StoreCall>>>,
        created: Arc<Mutex<Vec<ModelSpec>>>,
        decoded: Arc<Mutex<Vec<PathBuf>>>,
        fetch_calls: Arc<Mutex<Vec<url::Url>>>,
        max_in_flight: Arc<AtomicUsize>,
        resolved_calls: Arc<Mutex<Vec<Option<PathBuf>>>>,
        vad_seen: Arc<Mutex<Vec<Option<PathBuf>>>>,
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

        fn input_file(&self, name: &str) -> PathBuf {
            let path = self.tmp.path().join(name);
            std::fs::write(&path, b"fake media").expect("write input");
            path
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
        fetch_panics: bool,
        ensurer: Option<Arc<dyn ArtifactEnsurer>>,
        llm: Option<Arc<dyn LlmEndpointSource>>,
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

        let store = FakeStore {
            calls: Arc::clone(&store_calls),
            fail_started: cfg.fail_started,
            fail_next_create: std::sync::atomic::AtomicBool::new(cfg.fail_create),
            fail_requeue: cfg.fail_requeue,
            completed_gate: cfg.completed_gate,
            canceled_gate: cfg.canceled_gate,
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
        };
        let fetcher: Option<Arc<dyn MediaFetcher>> = if cfg.no_fetcher {
            None
        } else {
            Some(Arc::new(FakeFetcher {
                steps: cfg.fetch_steps,
                calls: Arc::clone(&fetch_calls),
                panics: cfg.fetch_panics,
            }))
        };

        let deps = RunnerDeps {
            store: Arc::new(store),
            decoder: Arc::new(decoder),
            prober: Arc::new(FakeProber),
            fetcher,
            engines: Box::new(factory),
            tuning: EngineTuning::default(),
            media_dir: tmp.path().join("media"),
            artifacts: cfg.ensurer,
            llm: cfg.llm,
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

        let expected = "yt-dlp missing — install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP";
        let calls = h.store_calls();
        assert!(calls.contains(&StoreCall::Failed(a, expected.to_string())));
        assert!(events.iter().any(|e| e.task_id == a
            && matches!(&e.kind, TaskEventKind::Failed { message } if message == expected)));
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
    /// the pipeline observes the cancel *before* the drain and takes the
    /// `stop_detached` + return `Canceled` path (design spec §6) — never
    /// merging or persisting any translation. Deterministic; the endpoint
    /// points at a dead loopback port so the detached worker's lone request
    /// can never reach a real service, and the test never waits on that
    /// worker.
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
}
