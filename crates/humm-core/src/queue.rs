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
//! - **Terminal status precedes the terminal event.** `mark_completed` /
//!   `mark_failed` / `mark_canceled` is written to the store *before* the
//!   matching `Completed`/`Failed`/`Canceled` event is emitted, so a consumer
//!   reacting to the event always sees a consistent store.
//! - **No panics.** Engine/pipeline errors are mapped to task failure, never
//!   propagated as a panic — one bad task must not wedge the queue. Store
//!   *write* failures (`mark_started`/`mark_completed`/`mark_failed`/
//!   `mark_canceled`/`update_progress`/`update_file`) are a different case:
//!   they are logged and the run continues regardless — a deliberate choice,
//!   since finishing the transcription beats aborting on a DB hiccup, and the
//!   task's terminal state is decided by the pipeline outcome, not by whether
//!   the store write for it happened to succeed.
//! - **Two mutexes, never nested.** [`Runner`] and [`QueueHandle`] share a
//!   subscriber list and a "current task" slot, each behind its own `Mutex`.
//!   Every critical section locks exactly one of them and releases it before
//!   locking the other, so no lock-ordering deadlock is possible.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::boundary::{
    AudioDecoder, FetchProgress, MediaFetcher, MediaProber, SegmentSink, TaskStore,
    TranscriptionEngine,
};
use crate::cancel::CancelToken;
use crate::domain::{Input, ModelSpec, Segment, Source, TaskSpec, TranscriptionId};
use crate::error::{CoreError, EngineError, MediaError};
use crate::pipeline::{EngineTuning, JobOutcome, run_file_job};

/// The message emitted to yt-dlp-less URL tasks: exactly this string is
/// written to the store and carried on the `Failed` event.
const YTDLP_MISSING: &str =
    "yt-dlp missing — install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP";

/// One observable moment in a task's lifecycle, broadcast to every subscriber.
///
/// The per-task order is `Queued` → (`Started` → `DownloadProgress`* →
/// `Progress`/`SegmentAdded`* → one of `Completed`/`Failed`) — unless the task
/// is canceled before it starts, in which case it is `Queued` → `Canceled`
/// with no `Started`.
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
}

/// A [`TaskEventKind`] tagged with the task it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskEvent {
    /// The task this event is about.
    pub task_id: TranscriptionId,
    /// What happened.
    pub kind: TaskEventKind,
}

/// Builds a [`TranscriptionEngine`] for a given [`ModelSpec`].
///
/// The runner caches the most recently built engine and reuses it while
/// consecutive tasks request an identical `ModelSpec` (one CLI run typically
/// uses one model), calling [`EngineFactory::create`] again only when the
/// model changes.
pub trait EngineFactory: Send {
    /// Creates an engine for `model`, or returns why it could not.
    fn create(&mut self, model: &ModelSpec) -> Result<Box<dyn TranscriptionEngine>, CoreError>;
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
fn is_canceled(error: &CoreError) -> bool {
    matches!(
        error,
        CoreError::Canceled
            | CoreError::Engine(EngineError::Canceled)
            | CoreError::Media(MediaError::Canceled)
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
                log::error!("update_progress failed for task {}: {err}", self.id);
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
    /// Tasks accepted but not yet run (FIFO).
    pending: VecDeque<PendingTask>,
    /// Ids canceled but not yet dequeued (Buzz's canceled-set). *Consumed*: an
    /// id is removed when its task is dequeued-and-canceled, so the set cannot
    /// grow without bound.
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
            Command::Enqueue { id, spec, source } => match self
                .store
                .create_task(&id, &spec, source)
            {
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
                    if let Err(err2) = self.store.mark_failed(&id, &message) {
                        log::error!("mark_failed after create_task error failed for {id}: {err2}");
                    }
                    self.emit(TaskEvent {
                        task_id: id,
                        kind: TaskEventKind::Failed { message },
                    });
                }
            },
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
            log::error!("mark_started failed for task {id}: {err}");
        }
        self.emit(TaskEvent {
            task_id: id,
            kind: TaskEventKind::Started,
        });

        let terminal = self.run_job(id, &spec, &token);
        self.set_current(None);

        match terminal {
            Terminal::Completed(outcome) => {
                if let Err(err) = self.store.mark_completed(&id, &outcome.segments) {
                    log::error!("mark_completed failed for task {id}: {err}");
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
                    log::error!("mark_failed failed for task {id}: {err}");
                }
                self.emit(TaskEvent {
                    task_id: id,
                    kind: TaskEventKind::Failed { message },
                });
            }
        }
    }

    /// Writes `canceled` to the store, then emits the `Canceled` event.
    fn write_canceled(&self, id: TranscriptionId) {
        if let Err(err) = self.store.mark_canceled(&id) {
            log::error!("mark_canceled failed for task {id}: {err}");
        }
        self.emit(TaskEvent {
            task_id: id,
            kind: TaskEventKind::Canceled,
        });
    }

    /// Resolves the input (downloading a URL if needed), acquires the engine,
    /// and runs the file-transcription pipeline.
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

        // Reuse the cached engine when the model matches, else build a new one.
        let cached = self.engine_cache.take();
        let mut engine = match cached {
            Some((cached_model, engine)) if cached_model == model => engine,
            _ => match self.engines.create(&model) {
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
            &self.tuning,
            &*self.decoder,
            &*self.prober,
            engine.as_mut(),
            &mut sink,
            token,
        );

        self.engine_cache = Some((model, engine));

        match result {
            Ok(outcome) => Terminal::Completed(outcome),
            Err(err) => classify(err),
        }
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
            log::error!("update_file failed for task {id}: {err}");
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
    use crate::boundary::{EngineCaps, EngineOptions, MediaInfo, Pcm};
    use crate::domain::{Millis, ModelSize, OutputSpec, TranscriptionOptions};
    use crate::error::StoreError;
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
    }

    struct FakeProber;

    impl MediaProber for FakeProber {
        fn probe(&self, _path: &Path) -> Result<MediaInfo, MediaError> {
            Ok(MediaInfo {
                duration: Some(Millis(1000)),
                has_audio: true,
                has_video: false,
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
        Block(Gate),
    }

    struct FakeEngine {
        scripts: Arc<Mutex<VecDeque<EngineScript>>>,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    impl TranscriptionEngine for FakeEngine {
        fn transcribe(
            &mut self,
            _audio: &Pcm,
            _options: &EngineOptions,
            sink: &mut dyn SegmentSink,
            cancel: &CancelToken,
        ) -> Result<(), EngineError> {
            let n = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(n, Ordering::SeqCst);
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
                Some(EngineScript::Block(g)) => {
                    g.wait();
                    if cancel.is_canceled() {
                        Err(EngineError::Canceled)
                    } else {
                        Ok(())
                    }
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
    }

    impl EngineFactory for FakeFactory {
        fn create(&mut self, model: &ModelSpec) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
            self.created
                .lock()
                .expect("created lock")
                .push(model.clone());
            Ok(Box::new(FakeEngine {
                scripts: Arc::clone(&self.scripts),
                in_flight: Arc::clone(&self.in_flight),
                max_in_flight: Arc::clone(&self.max_in_flight),
            }))
        }
    }

    // -- Fake fetcher: emits scripted progress, writes a dummy file ----------

    struct FakeFetcher {
        steps: Vec<FetchProgress>,
        calls: Arc<Mutex<Vec<url::Url>>>,
    }

    impl MediaFetcher for FakeFetcher {
        fn fetch(
            &self,
            url: &url::Url,
            dest_dir: &Path,
            progress: &mut dyn FnMut(FetchProgress),
            _cancel: &CancelToken,
        ) -> Result<PathBuf, MediaError> {
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

    // -- Harness -------------------------------------------------------------

    struct Harness {
        store_calls: Arc<Mutex<Vec<StoreCall>>>,
        created: Arc<Mutex<Vec<ModelSpec>>>,
        decoded: Arc<Mutex<Vec<PathBuf>>>,
        fetch_calls: Arc<Mutex<Vec<url::Url>>>,
        max_in_flight: Arc<AtomicUsize>,
        tmp: TempDir,
    }

    impl Harness {
        fn store_calls(&self) -> Vec<StoreCall> {
            self.store_calls.lock().expect("store calls").clone()
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
        completed_gate: Option<Gate>,
        canceled_gate: Option<Gate>,
        fetch_steps: Vec<FetchProgress>,
    }

    fn build(cfg: Cfg) -> (QueueHandle, Harness) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store_calls = Arc::new(Mutex::new(Vec::new()));
        let created = Arc::new(Mutex::new(Vec::new()));
        let decoded = Arc::new(Mutex::new(Vec::new()));
        let fetch_calls = Arc::new(Mutex::new(Vec::new()));
        let max_in_flight = Arc::new(AtomicUsize::new(0));

        let store = FakeStore {
            calls: Arc::clone(&store_calls),
            fail_started: cfg.fail_started,
            fail_next_create: std::sync::atomic::AtomicBool::new(cfg.fail_create),
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
        };
        let fetcher: Option<Arc<dyn MediaFetcher>> = if cfg.no_fetcher {
            None
        } else {
            Some(Arc::new(FakeFetcher {
                steps: cfg.fetch_steps,
                calls: Arc::clone(&fetch_calls),
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
                tmp,
            },
        )
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
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                ModelSpec {
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
}
