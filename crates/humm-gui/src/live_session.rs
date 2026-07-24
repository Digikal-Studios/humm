//! The live-recording session thread (Task 11, F-060/F-061/F-064, Q-10):
//! everything that runs *off* the UI thread once Record is clicked --
//! resolving/loading the model, opening the real audio capture, driving
//! [`humm_core::live::session::run_session`], and -- per this task's
//! threading decision (documented below) -- fully applying every accepted
//! utterance (filter, mode dispatch/merge, persistence, export, upload)
//! right here, before anything crosses to the UI.
//!
//! Ground truth: `buzz/widgets/recording_transcriber_widget.py:595-737`
//! (record click -> model ensure -> transcriber start), `:963-1035`
//! (`on_next_transcription` -- filter, mode dispatch, export, upload),
//! `:1103-1132` (stop/finished/error), `docs/superpowers/specs/
//! 2026-07-23-m5-live-recording-design.md` §2.1/§2.3/§2.4/§3/§4.
//!
//! ## Threading split (task brief's "pick a split, document it")
//!
//! Every side effect of an accepted utterance -- `append_live_segment`, the
//! export writers, and the upload POST -- runs on *this* thread, inside
//! [`SessionApplier::apply_text`], the same thread `run_session`'s loop (and
//! therefore the blocking engine call and the blocking upload POST) already
//! runs on. The UI thread's per-frame drain (`app.rs`) only ever receives a
//! [`SessionMessage`]: a whole-panel-text replace, a table-refresh
//! notification, a store-write-failure notice (ADR-018), or a terminal
//! error/stopped signal -- cheap values it applies to `RecordingVm` state
//! and, for `TableRefresh`, one `SqliteStore` *read* (`TasksVm::refresh_row`)
//! local to the UI thread. This keeps the hard requirement (upload never
//! touches the UI thread) trivially true, and keeps every store *write* off
//! the UI thread too (the brief's other allowed option), at the cost of
//! duplicating nothing: there is exactly one place ([`SessionApplier`]) that
//! ever computes the mode-dispatched/merged display text, so the UI's panel
//! and the export file's contents can never disagree.
//!
//! [`humm_core::live::session::run_session`]'s own [`LiveEvent::MeterBatch`]
//! needs no forwarding at all: the `meter: &MeterSnapshot` it also writes
//! directly is the exact same `Arc<MeterSnapshot>` `RecordingVm` already
//! reads every repaint (spec §2.5), so the meter animates live with zero
//! additional plumbing.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;

use humm_core::boundary::MeterSnapshot;
use humm_core::cancel::CancelToken;
use humm_core::domain::{Millis, ModelSpec, TranscriptionId, TranscriptionOptions};
use humm_core::error::{CoreError, EngineError, ModelError, StoreError};
use humm_core::live::{
    ExportFileType, ExportSink, LiveEvent, LiveMeasurement, RealClock, RealSleeper, RecordingMode,
    SessionConfig, UploadClient, filter_text, run_session,
};
use humm_core::live::{LIVE_DATETIME_FORMAT, MergeState, PanelText};
use humm_core::models::download::DownloadProgress;
use humm_store::SqliteStore;
use humm_whisper::{LiveOptions, WhisperEngine};

/// Ring-buffer capacity for the real session's capture: a transient
/// hand-off buffer between the RT callback and this thread's
/// [`run_session`] drain loop, sized to comfortably hold the *largest
/// single batch* any mode ever cuts (5.0 s @ 16 kHz — both append modes'
/// fixed `n_batch_samples` and append-and-correct's own
/// `RecordingSettings::MAX_TRANSCRIPTION_STEP`). This is more generous than
/// the meter-only ring's own ~1 s sizing (`recording.rs::RING_CAPACITY`) —
/// deliberately: `drain_ring` empties the whole ring every loop iteration,
/// so a ring this size is never actually *needed* under normal scheduling,
/// but sizing it to one full batch means a scheduling hiccup between two
/// `drain_ring` calls can never itself cause a drop mid-batch. The
/// `n_batch_samples`-scale *backlog* beyond that lives in `Chunker`'s own
/// accumulation queue (`max_queue_size`), not in this ring.
pub const SESSION_RING_CAPACITY: usize = 5 * humm_core::boundary::SAMPLE_RATE as usize;

// -- Store seam (mirrors `AudioCapture`'s "trait over a concrete adapter") --

/// The three live-session store operations (Task 7, `humm-store`), behind a
/// trait so unit tests can inject a scripted fake without a real SQLite file
/// while kittest/production use the real [`SqliteStore`] (already a real,
/// often temp-file, store even in test builds -- see `services::TestDeps`).
pub trait LiveStore: Send + Sync {
    /// See [`SqliteStore::insert_live_session`].
    fn insert_live_session(
        &self,
        name: &str,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionId, StoreError>;
    /// See [`SqliteStore::append_live_segment`].
    fn append_live_segment(
        &self,
        id: TranscriptionId,
        start: Millis,
        end: Millis,
        text: &str,
    ) -> Result<(), StoreError>;
    /// See [`SqliteStore::complete_live_session`].
    fn complete_live_session(&self, id: TranscriptionId) -> Result<(), StoreError>;
}

impl LiveStore for SqliteStore {
    fn insert_live_session(
        &self,
        name: &str,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionId, StoreError> {
        SqliteStore::insert_live_session(self, name, options)
    }

    fn append_live_segment(
        &self,
        id: TranscriptionId,
        start: Millis,
        end: Millis,
        text: &str,
    ) -> Result<(), StoreError> {
        SqliteStore::append_live_segment(self, id, start, end, text)
    }

    fn complete_live_session(&self, id: TranscriptionId) -> Result<(), StoreError> {
        SqliteStore::complete_live_session(self, id)
    }
}

// -- Engine seam -------------------------------------------------------------

/// One loaded live-transcription engine (Task 6's
/// [`WhisperEngine::transcribe_live`] in production) -- the per-session
/// object [`LiveEngineFactory::load`] hands back.
pub trait LiveEngineSession: Send {
    /// See [`WhisperEngine::transcribe_live`].
    fn transcribe_live(
        &mut self,
        samples: &[f32],
        opts: &LiveOptions,
        prompt: &str,
        cancel: &CancelToken,
    ) -> Result<String, EngineError>;
}

impl LiveEngineSession for WhisperEngine {
    fn transcribe_live(
        &mut self,
        samples: &[f32],
        opts: &LiveOptions,
        prompt: &str,
        cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        WhisperEngine::transcribe_live(self, samples, opts, prompt, cancel)
    }
}

/// Loads a [`LiveEngineSession`] from a resolved model path -- the
/// session-thread analogue of [`crate::engine_factory::GuiEngineFactory`],
/// which loads files' one-shot engine on the queue's runner thread; this one
/// loads a session's long-lived engine on the recording session thread
/// (never the UI thread -- CoreML's first-load compile can take minutes).
pub trait LiveEngineFactory: Send + Sync {
    /// Loads a model for live transcription. `use_gpu` mirrors
    /// [`WhisperEngine::load`]'s own parameter.
    fn load(
        &self,
        model_path: &Path,
        use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError>;
}

/// The real [`LiveEngineFactory`]: wraps [`WhisperEngine::load`].
pub struct RealLiveEngineFactory;

impl LiveEngineFactory for RealLiveEngineFactory {
    fn load(
        &self,
        model_path: &Path,
        use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        WhisperEngine::load(model_path, use_gpu)
            .map(|engine| Box::new(engine) as Box<dyn LiveEngineSession>)
    }
}

// -- Upload seam --------------------------------------------------------------

/// One session's upload sink (Task 3's [`UploadClient`] in production) --
/// behind a trait so kittest can assert upload calls through a scripted,
/// network-free fake (brief: "do NOT bind TCP in kittest") instead of a real
/// [`UploadClient`].
pub trait LiveUpload: Send + Sync {
    /// See [`UploadClient::clear`].
    fn clear(&self);
    /// See [`UploadClient::transcript`].
    fn transcript(&self, text: &str, replace: bool);
}

impl LiveUpload for UploadClient {
    fn clear(&self) {
        UploadClient::clear(self);
    }

    fn transcript(&self, text: &str, replace: bool) {
        UploadClient::transcript(self, text, replace);
    }
}

/// Builds a [`LiveUpload`] for one session's effective upload URL --
/// injectable so kittest never constructs a real, network-touching
/// [`UploadClient`].
pub trait UploadFactory: Send + Sync {
    /// Builds an uploader targeting `url`.
    fn build(&self, url: String) -> Box<dyn LiveUpload>;
}

/// The real [`UploadFactory`]: builds a real [`UploadClient`].
pub struct RealUploadFactory;

impl UploadFactory for RealUploadFactory {
    fn build(&self, url: String) -> Box<dyn LiveUpload> {
        Box::new(UploadClient::new(url))
    }
}

// -- Session <-> UI messages ---------------------------------------------------

/// Model-download / engine-load progress, before the transcription loop
/// itself starts (spec §2.1: "shown inline in the window").
#[derive(Debug, Clone, PartialEq)]
pub enum PrepUpdate {
    /// Downloading the model artifact.
    Downloading {
        /// Bytes downloaded so far.
        downloaded: u64,
        /// Total artifact size, once known.
        total: Option<u64>,
    },
    /// The model file is present; loading it into the engine (CoreML's
    /// first-load compile can take minutes, with no incremental progress to
    /// report).
    LoadingEngine,
}

/// One message the session thread sends to the recording window's per-frame
/// drain (`app.rs`) -- see the module doc's "threading split" for what each
/// variant does and does not carry.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionMessage {
    /// See [`PrepUpdate`].
    Preparing(PrepUpdate),
    /// The model is loaded and real audio capture has started -- the
    /// transcription loop is about to run. Buzz's `on_model_loaded` ->
    /// `reset_recording_controls()` (`:656-657`) clears its progress UI at
    /// this exact point, not on the first utterance (which may be delayed
    /// by silence, or never arrive at all) -- the UI clears
    /// [`crate::recording::RecordingVm::prep`] here for the same reason.
    /// Sent at most once, before any `PanelText`/`Error`/`Stopped`.
    Ready,
    /// A live session row was inserted or completed successfully -- the UI
    /// should refresh (or insert) this id's table row. Sent once after
    /// `insert_live_session` succeeds and once more after
    /// `complete_live_session` succeeds (spec: "emit at session start AND
    /// stop").
    TableRefresh(TranscriptionId),
    /// A store write failed (ADR-018): the session proceeds without this
    /// write persisted; the UI surfaces the store-health banner.
    StoreWriteFailed {
        /// Which store call failed (`"insert_live_session"`,
        /// `"append_live_segment"`, or `"complete_live_session"`).
        operation: String,
        /// The underlying [`StoreError`]'s message.
        detail: String,
    },
    /// One accepted utterance's fully-applied panel text (already filtered,
    /// mode-dispatched, and merged if append-and-correct) -- a whole-panel
    /// replace, not an append (the sender already holds the full text via
    /// its own [`PanelText`]).
    PanelText(String),
    /// A genuine failure (Buzz's error-dialog cause, still the raw
    /// message -- `app.rs` wraps it in the byte-exact dialog text, since
    /// [`crate::i18n::tr`] may only run on the UI thread). The loop has
    /// already ended; a [`SessionMessage::Stopped`] follows immediately.
    Error(String),
    /// The session has ended -- normal stop, or after an `Error`. Sent
    /// exactly once, always last.
    Stopped {
        /// This session's realtime measurement (spec §2.8), if any batch was
        /// ever transcribed.
        measurement: Option<LiveMeasurement>,
    },
}

/// The export folder/format/retention settings needed to set up one
/// session's [`ExportSink`] (Task 3) -- `None` at the [`LiveSessionRequest`]
/// level means export is off for this session (`export_enabled` was false),
/// distinct from [`ExportSink::enabled`] being false at *setup* time (the
/// configured folder doesn't exist -- Buzz parity, spec §2.4).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSetupRequest {
    /// The configured export folder.
    pub folder: PathBuf,
    /// TXT or CSV.
    pub file_type: ExportFileType,
    /// 0 = keep all entries.
    pub max_entries: usize,
    /// Custom filename template, or `None` for the default.
    pub template: Option<String>,
}

/// Everything [`run_live_session`] needs for one recording session --
/// assembled by `app.rs` from the `RecordingVm`'s live options snapshot and
/// `settings.recording` at the moment Record is clicked.
pub struct LiveSessionRequest {
    /// The selected input device id, or `None` (system default / no
    /// devices).
    pub device_id: Option<String>,
    /// The model to resolve (recording never targets a Hugging Face repo --
    /// `hf_repo` is always `None` here, spec §2.1).
    pub model: ModelSpec,
    /// Whether to allow GPU (Metal/CoreML) acceleration.
    pub use_gpu: bool,
    /// Live engine parameters (language/task/temperature/threads).
    pub live_options: LiveOptions,
    /// The chunker/loop configuration (mode, transcription step, silence
    /// threshold, initial prompt).
    pub session_config: SessionConfig,
    /// The panel/export line separator (append modes).
    pub separator: String,
    /// Append-and-correct's hide-unconfirmed setting.
    pub hide_unconfirmed: bool,
    /// The full options snapshot to persist via `insert_live_session`.
    pub options_snapshot: TranscriptionOptions,
    /// The session row's name (spec §4: `"Live recording <date>"`, see
    /// [`live_session_name`]).
    pub session_name: String,
    /// Export setup, or `None` when export is disabled.
    pub export: Option<ExportSetupRequest>,
    /// The effective upload URL, or `None` when upload is disabled (see
    /// [`humm_core::live::effective_upload_url`]).
    pub upload_url: Option<String>,
    /// The shared meter telemetry -- the *same* `Arc` the recording window's
    /// meter widget reads every repaint (spec §2.5).
    pub meter: Arc<MeterSnapshot>,
}

/// The session-thread handle `RecordingVm` holds while a real session is
/// active: the cooperative-stop token, the message channel, and the
/// (never blockingly joined -- see the module doc) thread handle.
pub struct ActiveSession {
    /// Signals cooperative stop -- also cancels an in-flight model download
    /// or engine call.
    pub cancel: CancelToken,
    /// Drained (`try_iter`) every frame by `app.rs`.
    pub rx: mpsc::Receiver<SessionMessage>,
    /// The session thread. Never `.join()`-ed from the UI thread (that would
    /// block the UI frame on this thread's post-`Stopped` cleanup, e.g.
    /// [`humm_core::boundary::CaptureHandle::stop`]) -- simply dropped once
    /// [`SessionMessage::Stopped`] has been observed, which detaches it; by
    /// that point every persistent side effect has already happened.
    pub thread: Option<JoinHandle<()>>,
}

/// Buzz's `"An error occurred while starting a new recording:" + cause +
/// ". " + "Please check your audio devices or check the application logs
/// for more information."` (`recording_transcriber_widget.py:1123-1132`),
/// byte-exact including the missing space before `cause`. Must run on the
/// UI thread ([`crate::i18n::tr`]'s own thread-local constraint) -- the
/// session thread only ever sends the raw `cause` via
/// [`SessionMessage::Error`].
pub fn recording_error_dialog_text(cause: &str) -> String {
    format!(
        "{}{cause}. {}",
        crate::i18n::tr("recording-error-prefix"),
        crate::i18n::tr("recording-error-suffix")
    )
}

/// Buzz's `"Live recording <%d-%b-%Y %H-%M-%S>"` (spec §4), sharing
/// [`LIVE_DATETIME_FORMAT`] with the export filename template's own
/// `{{ date_time }}` substitution so the table row and any exported file
/// read the same timestamp.
pub fn live_session_name(now: chrono::NaiveDateTime) -> String {
    format!("Live recording <{}>", now.format(LIVE_DATETIME_FORMAT))
}

/// Builds a session's [`ExportSetupRequest`] from `settings.recording`, or
/// `None` when export is off (`export_enabled` false) or no folder is
/// configured yet — the GUI-level gating decision `app.rs::start_live_session`
/// makes before ever touching [`ExportSink::setup`]. Distinct from
/// [`ExportSink::setup`]'s own *later* gate (`enabled()` — the configured
/// folder must actually exist on disk, Buzz parity, spec §2.4): this
/// function only decides whether to attempt setup at all.
pub fn export_request_from_settings(
    settings: &humm_core::settings::RecordingSettings,
) -> Option<ExportSetupRequest> {
    if !settings.export_enabled {
        return None;
    }
    let folder = settings.export_folder.clone()?;
    Some(ExportSetupRequest {
        folder,
        file_type: if settings.export_file_type == "csv" {
            ExportFileType::Csv
        } else {
            ExportFileType::Txt
        },
        max_entries: settings.export_max_entries as usize,
        template: settings.export_file_name.clone(),
    })
}

/// Records `ratio` as `size`'s measured live-transcription speed
/// (spec §2.8: last-run-wins per model size — a later session for the same
/// size simply overwrites the earlier entry, `BTreeMap::insert`'s own
/// contract already gives this for free; named here only so the "last
/// run wins" behavior has one obviously-correct, directly testable
/// definition).
pub fn record_measured_speed(
    measured_speed: &mut std::collections::BTreeMap<String, f32>,
    size: humm_core::domain::ModelSize,
    ratio: f64,
) {
    measured_speed.insert(size.as_str().to_string(), ratio as f32);
}

/// Spawns [`run_live_session`] on its own thread, returning the
/// [`ActiveSession`] handle `RecordingVm` holds for the rest of the
/// session's life. `deps` is the same `Arc<RecordingDeps>` `app.rs` shares
/// with the meter-only capture path -- `AudioCapture::start` takes `&mut
/// self`, so the session thread locks `deps.capture` for the session's
/// lifetime exactly like [`crate::recording::RecordingVm`]'s own meter-only
/// drain does while stopped.
pub fn spawn_live_session(
    deps: Arc<crate::recording::RecordingDeps>,
    request: LiveSessionRequest,
) -> ActiveSession {
    let cancel = CancelToken::new();
    let (tx, rx) = mpsc::channel();
    let cancel_for_thread = cancel.clone();
    let thread = std::thread::spawn(move || {
        run_live_session(&deps, request, &tx, &cancel_for_thread);
    });
    ActiveSession {
        cancel,
        rx,
        thread: Some(thread),
    }
}

/// The result of applying one [`LiveEvent::Text`] -- see [`SessionApplier::apply_text`].
pub struct TextOutcome {
    /// The panel's new whole text, or `None` when the utterance was filtered
    /// away entirely (Buzz `on_next_transcription:963-967`: no store write,
    /// no export, no upload, no panel change).
    pub panel: Option<String>,
    /// `Some(detail)` iff `append_live_segment` was attempted and failed
    /// (ADR-018 -- the utterance's other effects still apply).
    pub store_error: Option<String>,
}

/// Owns one session's append-and-correct merge state and panel text, and
/// fully applies each accepted utterance: filter -> store append (raw text)
/// -> mode dispatch (panel + merge) -> export write -> upload POST. See the
/// module doc's "threading split" for why this lives entirely on the
/// session thread. Pure enough to unit-test directly against fakes (no
/// thread/channel involved).
pub struct SessionApplier<'a> {
    mode: RecordingMode,
    separator: String,
    hide_unconfirmed: bool,
    panel: PanelText,
    merge: MergeState,
    session_id: Option<TranscriptionId>,
    store: &'a dyn LiveStore,
    export: Option<&'a ExportSink>,
    upload: Option<&'a dyn LiveUpload>,
}

impl<'a> SessionApplier<'a> {
    /// Builds a fresh applier (empty panel, empty merge state) for one
    /// session. `session_id` is `None` when `insert_live_session` failed
    /// (ADR-018: the session proceeds without persistence).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mode: RecordingMode,
        separator: String,
        hide_unconfirmed: bool,
        session_id: Option<TranscriptionId>,
        store: &'a dyn LiveStore,
        export: Option<&'a ExportSink>,
        upload: Option<&'a dyn LiveUpload>,
    ) -> Self {
        SessionApplier {
            mode,
            separator,
            hide_unconfirmed,
            panel: PanelText::new(),
            merge: MergeState::new(),
            session_id,
            store,
            export,
            upload,
        }
    }

    /// Ported from Buzz's `on_next_transcription` (`:963-1035`): `filter_text`
    /// first -- an empty result is a full no-op, matching Buzz's `if
    /// len(text) == 0: return` (no store write, no mode dispatch, no export,
    /// no upload, no panel mutation). Otherwise: `append_live_segment` with
    /// the **raw filtered** text (spec §4 -- the merge is a display
    /// transform, segments keep the honest timeline); mode dispatch (append
    /// below/above apply the raw text, append-and-correct feeds
    /// [`MergeState::push`] and applies its merged output); export (
    /// `on_utterance` for append modes, `on_merged` for append-and-correct);
    /// upload (`text` is always the **raw filtered** text regardless of
    /// mode -- Buzz `:1028`; `replace` iff append-and-correct, `:1029`).
    pub fn apply_text(&mut self, text: &str, start: Millis, end: Millis) -> TextOutcome {
        let filtered = filter_text(text);
        if filtered.is_empty() {
            return TextOutcome {
                panel: None,
                store_error: None,
            };
        }

        let store_error = if let Some(id) = self.session_id {
            self.store
                .append_live_segment(id, start, end, &filtered)
                .err()
                .map(|err| err.to_string())
        } else {
            None
        };

        let display = match self.mode {
            RecordingMode::AppendBelow | RecordingMode::AppendAbove => filtered.clone(),
            RecordingMode::AppendAndCorrect => self.merge.push(&filtered, self.hide_unconfirmed),
        };
        self.panel.apply(self.mode, &display, &self.separator);

        if let Some(export) = self.export {
            match self.mode {
                RecordingMode::AppendAndCorrect => export.on_merged(&display),
                RecordingMode::AppendBelow | RecordingMode::AppendAbove => {
                    export.on_utterance(&filtered);
                }
            }
        }
        if let Some(upload) = self.upload {
            upload.transcript(&filtered, self.mode == RecordingMode::AppendAndCorrect);
        }

        TextOutcome {
            panel: Some(self.panel.as_str().to_string()),
            store_error,
        }
    }
}

/// Classifies a [`CoreError`] from [`ArtifactEnsurer::ensure_model`] as a
/// cancellation (per its own doc comment: `CoreError::Canceled` or
/// `CoreError::Model(ModelError::Canceled)`) versus a genuine failure.
fn is_model_ensure_cancellation(err: &CoreError) -> bool {
    matches!(
        err,
        CoreError::Canceled | CoreError::Model(ModelError::Canceled)
    )
}

/// The session thread's whole body (Buzz `start_recording`/`on_model_loaded`
/// through `on_transcriber_finished`/`on_transcriber_error`, `:623-737,
/// 1103-1132`): resolve the model (downloading with inline progress if
/// needed), load the engine, persist the session row, set up export/upload,
/// start real audio capture, then drive [`run_session`] with a wrapping sink
/// that fully applies every utterance via [`SessionApplier`]. Every exit path
/// -- a cancel at any phase, a genuine failure at any phase, or a normal
/// `run_session` return -- ends by sending exactly one
/// [`SessionMessage::Stopped`], mirroring `run_session`'s own "always emits
/// `Stopped` last" contract so the caller has one terminal signal to wait
/// for regardless of when the session ended.
pub fn run_live_session(
    deps: &crate::recording::RecordingDeps,
    request: LiveSessionRequest,
    tx: &mpsc::Sender<SessionMessage>,
    cancel: &CancelToken,
) {
    let tx_for_progress = tx.clone();
    let mut progress_cb = move |update: DownloadProgress| {
        let _ = tx_for_progress.send(SessionMessage::Preparing(PrepUpdate::Downloading {
            downloaded: update.downloaded,
            total: update.total,
        }));
    };
    let model_path = match deps
        .model_ensurer
        .ensure_model(&request.model, &mut progress_cb, cancel)
    {
        Ok(path) => path,
        Err(err) => {
            if !is_model_ensure_cancellation(&err) {
                let _ = tx.send(SessionMessage::Error(err.to_string()));
            }
            let _ = tx.send(SessionMessage::Stopped { measurement: None });
            return;
        }
    };

    let _ = tx.send(SessionMessage::Preparing(PrepUpdate::LoadingEngine));
    if cancel.is_canceled() {
        let _ = tx.send(SessionMessage::Stopped { measurement: None });
        return;
    }
    let mut engine = match deps.engine_factory.load(&model_path, request.use_gpu) {
        Ok(engine) => engine,
        Err(err) => {
            let _ = tx.send(SessionMessage::Error(err.to_string()));
            let _ = tx.send(SessionMessage::Stopped { measurement: None });
            return;
        }
    };
    if cancel.is_canceled() {
        let _ = tx.send(SessionMessage::Stopped { measurement: None });
        return;
    }

    // Persist the session row (spec §4: after the model is ready, before the
    // first chunk). A failure is ADR-018: the session proceeds without
    // persistence, never aborts on a store error.
    let session_id = match deps
        .store
        .insert_live_session(&request.session_name, &request.options_snapshot)
    {
        Ok(id) => {
            let _ = tx.send(SessionMessage::TableRefresh(id));
            Some(id)
        }
        Err(err) => {
            let _ = tx.send(SessionMessage::StoreWriteFailed {
                operation: "insert_live_session".to_string(),
                detail: err.to_string(),
            });
            None
        }
    };

    let export = request.export.as_ref().map(|cfg| {
        ExportSink::setup(
            &cfg.folder,
            cfg.file_type,
            cfg.max_entries,
            &request.separator,
            request.session_config.mode,
            cfg.template.as_deref(),
            request.live_options.task,
            request.live_options.language.map(|l| l.code()),
            request.model.size.as_str(),
            chrono::Local::now().naive_local(),
        )
    });
    let upload: Option<Box<dyn LiveUpload>> = request
        .upload_url
        .clone()
        .map(|url| deps.upload_factory.build(url));
    if let Some(upload) = &upload {
        upload.clear();
    }

    let (producer, consumer) = rtrb::RingBuffer::<f32>::new(SESSION_RING_CAPACITY);
    // `AudioCapture::start` needs `&mut self`, so this thread locks the
    // shared capture adapter for the session's lifetime (Task 10's
    // meter-only drain uses the same adapter while stopped; the two are
    // never both active on the same device, `recording::RecordingVm`'s own
    // teardown-before-start ordering).
    let capture_handle = {
        let mut capture = match deps.capture.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match capture.start(
            request.device_id.as_deref(),
            producer,
            Arc::clone(&request.meter),
        ) {
            Ok(handle) => handle,
            Err(err) => {
                let _ = tx.send(SessionMessage::Error(err.to_string()));
                let _ = tx.send(SessionMessage::Stopped { measurement: None });
                return;
            }
        }
    };
    let _ = tx.send(SessionMessage::Ready);

    let applier = RefCell::new(SessionApplier::new(
        request.session_config.mode,
        request.separator.clone(),
        request.hide_unconfirmed,
        session_id,
        deps.store.as_ref(),
        export.as_ref(),
        upload.as_deref(),
    ));
    let sink = |event: LiveEvent| match event {
        LiveEvent::Text { text, start, end } => {
            let outcome = applier.borrow_mut().apply_text(&text, start, end);
            if let Some(detail) = outcome.store_error {
                let _ = tx.send(SessionMessage::StoreWriteFailed {
                    operation: "append_live_segment".to_string(),
                    detail,
                });
            }
            if let Some(panel) = outcome.panel {
                let _ = tx.send(SessionMessage::PanelText(panel));
            }
        }
        LiveEvent::MeterBatch { .. } => {
            // No forwarding needed -- `run_session` already wrote
            // `average_rms`/`queue_len` directly onto `request.meter`
            // (module doc).
        }
        LiveEvent::Error(msg) => {
            let _ = tx.send(SessionMessage::Error(msg));
        }
        LiveEvent::Stopped { measurement } => {
            if let Some(id) = session_id {
                match deps.store.complete_live_session(id) {
                    Ok(()) => {
                        let _ = tx.send(SessionMessage::TableRefresh(id));
                    }
                    Err(err) => {
                        let _ = tx.send(SessionMessage::StoreWriteFailed {
                            operation: "complete_live_session".to_string(),
                            detail: err.to_string(),
                        });
                    }
                }
            }
            let _ = tx.send(SessionMessage::Stopped { measurement });
        }
    };

    let mut engine_call = |samples: &[f32], prompt: &str| {
        engine.transcribe_live(samples, &request.live_options, prompt, cancel)
    };

    run_session(
        consumer,
        &mut engine_call,
        &request.meter,
        &sink,
        cancel,
        &request.session_config,
        &RealClock,
        &RealSleeper,
    );

    capture_handle.stop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::settings::RecordingSettings;
    use pretty_assertions::assert_eq;
    use std::sync::Mutex as StdMutex;

    // -- SessionApplier -----------------------------------------------------

    /// Records every `append_live_segment` call (or fails them all, per
    /// `fail`); `insert_live_session`/`complete_live_session` are never
    /// exercised by `SessionApplier` — panicking there proves that.
    #[derive(Default)]
    struct FakeStore {
        calls: StdMutex<Vec<(TranscriptionId, Millis, Millis, String)>>,
        fail: bool,
    }

    impl LiveStore for FakeStore {
        fn insert_live_session(
            &self,
            _name: &str,
            _options: &TranscriptionOptions,
        ) -> Result<TranscriptionId, StoreError> {
            unreachable!("SessionApplier never calls insert_live_session")
        }

        fn append_live_segment(
            &self,
            id: TranscriptionId,
            start: Millis,
            end: Millis,
            text: &str,
        ) -> Result<(), StoreError> {
            if self.fail {
                return Err(StoreError::Query {
                    detail: "boom".to_string(),
                });
            }
            self.calls
                .lock()
                .unwrap()
                .push((id, start, end, text.to_string()));
            Ok(())
        }

        fn complete_live_session(&self, _id: TranscriptionId) -> Result<(), StoreError> {
            unreachable!("SessionApplier never calls complete_live_session")
        }
    }

    #[test]
    fn append_below_appends_with_separator_and_persists_raw_text() {
        let store = FakeStore::default();
        let sid = TranscriptionId::new();
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        );

        let out1 = applier.apply_text("hello", Millis(0), Millis(1000));
        assert_eq!(out1.panel.as_deref(), Some("hello"));
        assert_eq!(out1.store_error, None);

        let out2 = applier.apply_text("world", Millis(1000), Millis(2000));
        assert_eq!(out2.panel.as_deref(), Some("hello\n\nworld"));

        let calls = store.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0],
            (sid, Millis(0), Millis(1000), "hello".to_string())
        );
        assert_eq!(
            calls[1],
            (sid, Millis(1000), Millis(2000), "world".to_string())
        );
    }

    #[test]
    fn append_above_prepends_and_persists_raw_text() {
        let store = FakeStore::default();
        let sid = TranscriptionId::new();
        let mut applier = SessionApplier::new(
            RecordingMode::AppendAbove,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        );

        let out1 = applier.apply_text("hello", Millis(0), Millis(1000));
        assert_eq!(out1.panel.as_deref(), Some("hello\n\n"));

        let out2 = applier.apply_text("world", Millis(1000), Millis(2000));
        assert_eq!(out2.panel.as_deref(), Some("world\n\nhello\n\n"));

        let calls = store.calls.lock().unwrap();
        assert_eq!(calls[0].3, "hello");
        assert_eq!(calls[1].3, "world");
    }

    #[test]
    fn append_and_correct_persists_raw_text_not_the_merged_display() {
        let store = FakeStore::default();
        let sid = TranscriptionId::new();
        let mut applier = SessionApplier::new(
            RecordingMode::AppendAndCorrect,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        );

        applier.apply_text("the quick brown fox", Millis(0), Millis(1000));
        let out2 = applier.apply_text("brown fox jumps over", Millis(1000), Millis(2000));

        // The store's raw segment text is the second call's own filtered
        // text, untouched by the merge -- never the merged/displayed panel
        // text (spec §4).
        let calls = store.calls.lock().unwrap();
        assert_eq!(calls[1].3, "brown fox jumps over");
        // The panel, in contrast, shows the merged output (distinct from
        // the raw text whenever a real overlap was found).
        assert_ne!(out2.panel.as_deref(), Some("brown fox jumps over"));
    }

    #[test]
    fn empty_after_filter_is_a_full_no_op() {
        let store = FakeStore::default();
        let sid = TranscriptionId::new();
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        );

        let out = applier.apply_text("   ", Millis(0), Millis(500));

        assert_eq!(out.panel, None);
        assert_eq!(out.store_error, None);
        assert!(store.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_missing_session_id_skips_the_store_call_but_still_applies_the_panel() {
        let store = FakeStore::default();
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            None, // insert_live_session failed (ADR-018) -- no id to persist against.
            &store,
            None,
            None,
        );

        let out = applier.apply_text("hi there", Millis(0), Millis(500));

        assert_eq!(out.panel.as_deref(), Some("hi there"));
        assert_eq!(out.store_error, None);
        assert!(store.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_store_write_failure_is_surfaced_but_the_utterance_still_applies() {
        let store = FakeStore {
            fail: true,
            ..Default::default()
        };
        let sid = TranscriptionId::new();
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        );

        let out = applier.apply_text("hi there", Millis(0), Millis(500));

        assert_eq!(out.panel.as_deref(), Some("hi there"));
        assert!(out.store_error.is_some());
    }

    // -- is_model_ensure_cancellation ----------------------------------------

    #[test]
    fn core_canceled_is_a_cancellation() {
        assert!(is_model_ensure_cancellation(&CoreError::Canceled));
    }

    #[test]
    fn model_canceled_is_a_cancellation() {
        assert!(is_model_ensure_cancellation(&CoreError::Model(
            ModelError::Canceled
        )));
    }

    #[test]
    fn a_genuine_model_error_is_not_a_cancellation() {
        assert!(!is_model_ensure_cancellation(&CoreError::Model(
            ModelError::Http {
                detail: "offline".to_string()
            }
        )));
    }

    // -- recording_error_dialog_text -------------------------------------------

    #[test]
    fn recording_error_dialog_text_matches_buzz_byte_exact() {
        assert_eq!(
            recording_error_dialog_text("no input device"),
            "An error occurred while starting a new recording:no input device. Please check \
             your audio devices or check the application logs for more information."
        );
    }

    // -- live_session_name ----------------------------------------------------

    #[test]
    fn live_session_name_matches_the_buzz_format_byte_exact() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 7, 24)
            .unwrap()
            .and_hms_opt(14, 3, 59)
            .unwrap();
        assert_eq!(
            live_session_name(now),
            "Live recording <24-Jul-2026 14-03-59>"
        );
    }

    // -- export_request_from_settings ------------------------------------------

    #[test]
    fn export_disabled_yields_no_request() {
        let settings = RecordingSettings {
            export_enabled: false,
            export_folder: Some(PathBuf::from("/tmp/exports")),
            ..Default::default()
        };
        assert!(export_request_from_settings(&settings).is_none());
    }

    #[test]
    fn export_enabled_without_a_folder_yields_no_request() {
        let settings = RecordingSettings {
            export_enabled: true,
            export_folder: None,
            ..Default::default()
        };
        assert!(export_request_from_settings(&settings).is_none());
    }

    #[test]
    fn export_enabled_with_a_folder_yields_a_request() {
        let settings = RecordingSettings {
            export_enabled: true,
            export_folder: Some(PathBuf::from("/tmp/exports")),
            export_file_type: "csv".to_string(),
            export_max_entries: 5,
            export_file_name: Some("custom".to_string()),
            ..Default::default()
        };

        let request = export_request_from_settings(&settings).expect("some request");
        assert_eq!(request.folder, PathBuf::from("/tmp/exports"));
        assert_eq!(request.file_type, ExportFileType::Csv);
        assert_eq!(request.max_entries, 5);
        assert_eq!(request.template.as_deref(), Some("custom"));
    }

    #[test]
    fn export_file_type_defaults_to_txt_for_any_non_csv_value() {
        let settings = RecordingSettings {
            export_enabled: true,
            export_folder: Some(PathBuf::from("/tmp/exports")),
            export_file_type: "txt".to_string(),
            ..Default::default()
        };

        let request = export_request_from_settings(&settings).expect("some request");
        assert_eq!(request.file_type, ExportFileType::Txt);
    }

    // -- record_measured_speed -------------------------------------------------

    #[test]
    fn record_measured_speed_last_run_wins_per_size() {
        let mut measured = std::collections::BTreeMap::new();
        record_measured_speed(&mut measured, humm_core::domain::ModelSize::Small, 2.0);
        assert_eq!(measured.get("small"), Some(&2.0));

        record_measured_speed(&mut measured, humm_core::domain::ModelSize::Small, 0.5);
        assert_eq!(
            measured.get("small"),
            Some(&0.5),
            "a later session for the same size overwrites the earlier entry"
        );

        record_measured_speed(&mut measured, humm_core::domain::ModelSize::Base, 3.0);
        assert_eq!(measured.len(), 2, "a different size is a separate entry");
    }
}
