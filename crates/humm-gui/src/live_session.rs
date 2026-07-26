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
//!
//! ## Capture teardown/restart ordering (enforced, not just documented)
//!
//! [`SessionMessage::Stopped`] is what makes the UI thread restart the
//! meter-only capture on the same device
//! ([`crate::recording::RecordingVm::finish_stopping`]). If that message
//! reached the UI *before* the real session capture had actually finished
//! tearing down, the restart could race an in-flight stream teardown on the
//! same device (two streams on one device — the exact failure mode Buzz's
//! own `reset_recording_amplitude_listener` avoids). [`run_live_session`]
//! therefore never forwards core's `LiveEvent::Stopped` straight to the
//! caller: its wrapping sink only *records* the measurement; the real
//! [`SessionMessage::Stopped`] send happens after `run_session` has
//! returned *and* `CaptureHandle::stop` has actually run — see
//! [`run_live_session`]'s own doc comment for the exact sequencing, and its
//! test module for a wrapper-ordering test that proves it (a scripted
//! capture handle whose `stop()` and the eventual `Stopped` send both
//! append to one shared, order-preserving log).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;

use humm_core::boundary::MeterSnapshot;
use humm_core::cancel::CancelToken;
use humm_core::domain::{Millis, ModelSpec, TranscriptionId, TranscriptionOptions};
use humm_core::error::{CoreError, EngineError, ModelError, StoreError};
use humm_core::language::Language;
use humm_core::live::{
    ExportFileType, ExportSink, LiveEvent, LiveMeasurement, RealClock, RealSleeper, RecordingMode,
    SessionConfig, UploadClient, filter_text, run_session,
};
use humm_core::live::{LIVE_DATETIME_FORMAT, MergeState, PanelText};
use humm_core::models::download::DownloadProgress;
use humm_core::openai::{LiveApiTranscriber, OpenAiEngineConfig};
use humm_store::SqliteStore;
use humm_whisper::{LiveOptions, WhisperEngine};

use crate::live_translation::TranslationEnqueue;

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
    /// See [`SqliteStore::append_live_segment`] -- returns the inserted
    /// segment row id (`last_insert_rowid`), which F-063 tags the enqueued
    /// translation with so it can later be persisted onto that same row.
    fn append_live_segment(
        &self,
        id: TranscriptionId,
        start: Millis,
        end: Millis,
        text: &str,
    ) -> Result<i64, StoreError>;
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
    ) -> Result<i64, StoreError> {
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

    /// Builds the API backend's [`LiveEngineSession`] from a resolved `cfg`
    /// (F-061b, design spec §10): `run_live_session`'s API arm calls this
    /// instead of [`Self::load`] whenever `LiveSessionRequest::api_config`
    /// is `Some`, skipping model resolution/download and `load` entirely --
    /// there is no local model for this backend. Unlike `load`, this cannot
    /// fail: `app.rs::start_live_session` already confirmed a usable key
    /// before ever spawning this thread (F-061b's missing-key precheck), so
    /// building the [`LiveApiTranscriber`]-backed session is just
    /// constructing a bounded-timeout HTTP client, no I/O.
    ///
    /// Defaulted to `unreachable!` so every pre-F-061b implementor --
    /// including every scripted kittest fake that predates this task --
    /// keeps compiling unchanged, and a test that somehow reached this path
    /// without scripting one fails loudly instead of silently making a real
    /// HTTP request (the same "prove nothing reaches it by accident"
    /// contract `app.rs`'s `UnwiredUploadFactory` already uses).
    /// [`RealLiveEngineFactory`] is the only production override; a kittest
    /// fake that wants API-arm coverage overrides it too, to return a
    /// scripted [`LiveEngineSession`] instead of a real
    /// [`LiveApiTranscriber`] (brief: "do NOT bind TCP in kittest" --
    /// `openai::live`'s own mock-server tests already carry the HTTP truth
    /// for `LiveApiTranscriber::transcribe_chunk` itself).
    fn load_api(&self, cfg: OpenAiEngineConfig) -> Box<dyn LiveEngineSession> {
        let _ = cfg;
        unreachable!(
            "LiveEngineFactory::load_api has no real implementation here -- override it (or \
             script one via a test fake) before a session selects the OpenAI API backend"
        )
    }
}

/// The real [`LiveEngineFactory`]: wraps [`WhisperEngine::load`] for the
/// local backend, [`ApiLiveEngineSession`]/[`LiveApiTranscriber`] for the
/// API backend (F-061b).
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

    fn load_api(&self, cfg: OpenAiEngineConfig) -> Box<dyn LiveEngineSession> {
        Box::new(ApiLiveEngineSession::new(cfg))
    }
}

/// The API-backend arm of [`LiveEngineSession`] (F-061b): wraps
/// [`LiveApiTranscriber`], built by [`RealLiveEngineFactory::load_api`]
/// whenever a session's `LiveSessionRequest::api_config` is `Some`. No local
/// model, no engine load -- just a bounded-timeout HTTP client built once
/// per session. `transcribe_live` threads `opts.task`/`opts.language`
/// straight through to [`LiveApiTranscriber::transcribe_chunk`], whose own
/// endpoint selection (transcriptions vs. translations, ledger #L15 closure)
/// and language-gating apply exactly as the file engine's do -- this wrapper
/// makes no decisions of its own, it only maps field names across the two
/// crates' `LiveOptions`/`transcribe_chunk` shapes.
struct ApiLiveEngineSession {
    transcriber: LiveApiTranscriber,
}

impl ApiLiveEngineSession {
    fn new(cfg: OpenAiEngineConfig) -> Self {
        ApiLiveEngineSession {
            transcriber: LiveApiTranscriber::new(cfg),
        }
    }
}

impl LiveEngineSession for ApiLiveEngineSession {
    fn transcribe_live(
        &mut self,
        samples: &[f32],
        opts: &LiveOptions,
        prompt: &str,
        cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        self.transcriber.transcribe_chunk(
            samples,
            prompt,
            opts.language.map(Language::code),
            opts.task,
            cancel,
        )
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
    /// The session's export sink (F-064/F-063), or `None` when export is
    /// disabled. Built by `app.rs::start_live_session` from
    /// [`export_request_from_settings`] and wrapped in an `Arc` **there** (not
    /// created on the session thread) precisely so the F-063 live-translation
    /// callback — which runs on the translator's worker thread and writes the
    /// `.translated` sibling — can share the *same* sink instance: one
    /// [`ExportSink::setup`] means one filename timestamp for both the
    /// transcript and its `.translated` sibling (two independent setups would
    /// pick two different `chrono::Local::now()` timestamps, breaking the
    /// sibling relationship). `ExportSink`'s writers are all `&self` and touch
    /// disjoint files (transcript vs translated), so the two threads never
    /// contend. Also matches Buzz, which sets export up on the GUI thread
    /// (`_init_export_settings` in `start_recording`).
    pub export_sink: Option<Arc<ExportSink>>,
    /// The effective upload URL, or `None` when upload is disabled (see
    /// [`humm_core::live::effective_upload_url`]).
    pub upload_url: Option<String>,
    /// The shared meter telemetry -- the *same* `Arc` the recording window's
    /// meter widget reads every repaint (spec §2.5).
    pub meter: Arc<MeterSnapshot>,
    /// The live-translation enqueue handle (F-063), `None` when translation is
    /// off (or unconfigured / endpoint unresolvable) for this session. Passed
    /// through to [`SessionApplier::with_translation_enqueue`] so the session
    /// thread enqueues each accepted utterance for translation.
    pub translation_enqueue: Option<TranslationEnqueue>,
    /// The API backend's resolved config (F-061b, design spec §10), or
    /// `None` for the local whisper.cpp backend. `app.rs::start_live_session`
    /// resolves this (keyring key + `settings.openai`) **before** ever
    /// spawning this session -- a missing/empty key aborts the Record click
    /// inline instead of ever reaching this struct, so `Some` here always
    /// carries a usable key. [`run_live_session`] branches on this field to
    /// skip model resolution/download and [`LiveEngineFactory::load`]
    /// entirely, calling [`LiveEngineFactory::load_api`] instead.
    pub api_config: Option<OpenAiEngineConfig>,
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
    /// The session thread. Never `.join()`-ed from the UI thread on the
    /// normal-completion path (that would block the UI frame on this
    /// thread's post-`Stopped` cleanup) -- simply dropped once
    /// [`SessionMessage::Stopped`] has been observed, which detaches it; by
    /// that point [`run_live_session`]'s own enforced ordering (see the
    /// module doc's "Capture teardown/restart ordering" section) guarantees
    /// every persistent side effect, *and* the real capture's teardown,
    /// have already happened. Kept (not discarded at spawn time) precisely
    /// so `app.rs` can also poll [`JoinHandle::is_finished`] as a liveness
    /// check -- a thread that exited *without* ever sending `Stopped` (a
    /// panic) is the crash-recovery fallback's own signal.
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

/// Records `ratio` as `key`'s measured live-transcription speed (spec §2.8:
/// last-run-wins per key — a later session for the same key simply
/// overwrites the earlier entry, `BTreeMap::insert`'s own contract already
/// gives this for free; named here only so the "last run wins" behavior has
/// one obviously-correct, directly testable definition). `key` is
/// [`crate::recording::measured_speed_key`]'s output — a model-size wire
/// string for the local whisper.cpp backend, or the API backend's own
/// backend-qualified key (F-061b, ledger #L14) — this function itself makes
/// no backend distinction, it just records whatever key the caller resolved.
pub fn record_measured_speed(
    measured_speed: &mut std::collections::BTreeMap<String, f32>,
    key: &str,
    ratio: f64,
) {
    measured_speed.insert(key.to_string(), ratio as f32);
}

/// Spawns [`run_live_session`] on its own thread, returning the
/// [`ActiveSession`] handle `RecordingVm` holds for the rest of the
/// session's life. `deps` is the same `Arc<RecordingDeps>` `app.rs` shares
/// with the meter-only capture path -- `AudioCapture::start` takes `&mut
/// self`, so the session thread locks `deps.capture` for the session's
/// lifetime exactly like [`crate::recording::RecordingVm`]'s own meter-only
/// drain does while stopped.
///
/// The `SessionMessage` channel `(tx, rx)` is created by the caller
/// (`app.rs::start_live_session`) rather than here purely for symmetry with the
/// rest of that method's setup. The F-063 live-translation panel text does
/// **not** travel on this channel: it would be lost the moment the session ends
/// (this `rx` is dropped when `RecordingVm::session` becomes `None` at Stop),
/// so the translation worker posts panel text into a separate channel owned by
/// the longer-lived [`crate::live_translation::LiveTranslator`] instead (spec
/// §7.4 — late arrivals after Stop must still reach the panel).
pub fn spawn_live_session(
    deps: Arc<crate::recording::RecordingDeps>,
    request: LiveSessionRequest,
    tx: mpsc::Sender<SessionMessage>,
    rx: mpsc::Receiver<SessionMessage>,
) -> ActiveSession {
    let cancel = CancelToken::new();
    let cancel_for_thread = cancel.clone();
    let thread = std::thread::spawn(move || {
        // `run_live_session` takes its sink as `&dyn Fn(SessionMessage)`
        // (not the concrete `mpsc::Sender` directly) precisely so its own
        // tests can inject an order-observing fake instead of a real
        // channel -- see the module doc's "Capture teardown/restart
        // ordering" section and this function's own test module.
        let sink = move |message: SessionMessage| {
            let _ = tx.send(message);
        };
        run_live_session(&deps, request, &sink, &cancel_for_thread);
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
    /// The inserted segment row id (`append_live_segment`'s
    /// `last_insert_rowid`), when a session id was present and the write
    /// succeeded -- `None` when there was no session to persist against
    /// (ADR-018) or the write failed. F-063's live-translation wiring (a
    /// later task) enqueues the raw utterance tagged with this id so the
    /// arriving translation can be persisted onto the same row via
    /// `update_segment_translation`.
    pub segment_row_id: Option<i64>,
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
    /// The live-translation enqueue handle (F-063), `None` when translation is
    /// off for this session. Set via [`Self::with_translation_enqueue`] rather
    /// than the constructor so the existing (non-translation) call sites and
    /// unit tests are untouched. Owned (not borrowed) — it's a cheap channel
    /// closure the UI thread hands off for the session's lifetime.
    translation_enqueue: Option<TranslationEnqueue>,
}

impl<'a> SessionApplier<'a> {
    /// Builds a fresh applier (empty panel, empty merge state) for one
    /// session. `session_id` is `None` when `insert_live_session` failed
    /// (ADR-018: the session proceeds without persistence). Live translation
    /// is off by default — see [`Self::with_translation_enqueue`].
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
            translation_enqueue: None,
        }
    }

    /// Attaches a live-translation enqueue handle (F-063): every accepted
    /// utterance's raw filtered text is enqueued (tagged with its persisted
    /// segment row id) for translation, at Buzz's exact placement — after the
    /// filter/empty-check, before mode dispatch (`on_next_transcription:969-970`).
    #[must_use]
    pub fn with_translation_enqueue(mut self, enqueue: TranslationEnqueue) -> Self {
        self.translation_enqueue = Some(enqueue);
        self
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
                segment_row_id: None,
            };
        }

        // Capture both the row id (F-063: the translation enqueue tags it)
        // and any failure detail (ADR-018: surfaced, but the utterance's
        // other effects still apply) from the one store write.
        let (segment_row_id, store_error) = if let Some(id) = self.session_id {
            match self.store.append_live_segment(id, start, end, &filtered) {
                Ok(row_id) => (Some(row_id), None),
                Err(err) => (None, Some(err.to_string())),
            }
        } else {
            (None, None)
        };

        // F-063: enqueue the raw filtered text for translation *before* mode
        // dispatch (Buzz `on_next_transcription:969-970`: `self.translator.enqueue(text)`
        // sits right after the filter/empty-check and before any panel/export
        // work), tagged with the persisted segment row id so the arriving
        // translation can be written onto that same row. A `None` row id
        // (store write failed / no session) still enqueues — the translation
        // just won't be persisted (its panel/export effects still apply),
        // exactly as Buzz (which has no store) enqueues regardless.
        if let Some(enqueue) = &self.translation_enqueue {
            enqueue(filtered.clone(), segment_row_id);
        }

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
            segment_row_id,
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
///
/// `sink` is a plain `&dyn Fn(SessionMessage)` rather than a concrete
/// `mpsc::Sender` -- [`spawn_live_session`] wraps a real channel in one for
/// production; this function's own tests inject an order-observing fake
/// instead (see the module doc's "Capture teardown/restart ordering"
/// section, and this function's own real-capture-path tests below).
///
/// **Ordering invariant (enforced, not just documented):** once real audio
/// capture has started (i.e. past the `capture_handle` block below), this
/// function *never* forwards core's `LiveEvent::Stopped` straight to `sink`
/// -- it only records the measurement in a local. The real
/// [`SessionMessage::Stopped`] send happens after `run_session` has
/// returned *and* `capture_handle.stop()` has actually run, so by the time
/// the UI thread ever sees `Stopped` (and therefore might restart the
/// meter-only capture on the same device,
/// [`crate::recording::RecordingVm::finish_stopping`]), the real session's
/// own stream is already fully torn down.
pub fn run_live_session(
    deps: &crate::recording::RecordingDeps,
    mut request: LiveSessionRequest,
    sink: &dyn Fn(SessionMessage),
    cancel: &CancelToken,
) {
    // F-061b: the API backend has no local model to resolve/download and no
    // engine to `load` (`LiveSessionRequest::api_config`'s own doc) --
    // `deps.engine_factory.load_api` just constructs a bounded-timeout HTTP
    // client, so this branch skips `model_ensurer`/`load` entirely rather
    // than routing a no-op model resolution through them.
    let mut engine: Box<dyn LiveEngineSession> = if let Some(api_cfg) = request.api_config.take() {
        deps.engine_factory.load_api(api_cfg)
    } else {
        let mut progress_cb = |update: DownloadProgress| {
            sink(SessionMessage::Preparing(PrepUpdate::Downloading {
                downloaded: update.downloaded,
                total: update.total,
            }));
        };
        let model_path =
            match deps
                .model_ensurer
                .ensure_model(&request.model, &mut progress_cb, cancel)
            {
                Ok(path) => path,
                Err(err) => {
                    if !is_model_ensure_cancellation(&err) {
                        sink(SessionMessage::Error(err.to_string()));
                    }
                    sink(SessionMessage::Stopped { measurement: None });
                    return;
                }
            };

        sink(SessionMessage::Preparing(PrepUpdate::LoadingEngine));
        if cancel.is_canceled() {
            sink(SessionMessage::Stopped { measurement: None });
            return;
        }
        match deps.engine_factory.load(&model_path, request.use_gpu) {
            Ok(engine) => engine,
            Err(err) => {
                sink(SessionMessage::Error(err.to_string()));
                sink(SessionMessage::Stopped { measurement: None });
                return;
            }
        }
    };
    if cancel.is_canceled() {
        sink(SessionMessage::Stopped { measurement: None });
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
            sink(SessionMessage::TableRefresh(id));
            Some(id)
        }
        Err(err) => {
            sink(SessionMessage::StoreWriteFailed {
                operation: "insert_live_session".to_string(),
                detail: err.to_string(),
            });
            None
        }
    };

    // The export sink was built (and its files cleared) by `app.rs` on the UI
    // thread and shared here via an `Arc` so the F-063 translation callback can
    // hold the same instance (see `LiveSessionRequest::export_sink`'s doc).
    let export = request.export_sink.clone();
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
    // teardown-before-start ordering, *and* this function's own
    // capture-before-`Stopped` ordering below).
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
                sink(SessionMessage::Error(err.to_string()));
                sink(SessionMessage::Stopped { measurement: None });
                return;
            }
        }
    };
    sink(SessionMessage::Ready);

    let mut applier = SessionApplier::new(
        request.session_config.mode,
        request.separator.clone(),
        request.hide_unconfirmed,
        session_id,
        deps.store.as_ref(),
        export.as_deref(),
        upload.as_deref(),
    );
    if let Some(enqueue) = request.translation_enqueue.take() {
        applier = applier.with_translation_enqueue(enqueue);
    }
    let applier = RefCell::new(applier);
    // Captures the session's final measurement without ever forwarding
    // `LiveEvent::Stopped` to `sink` directly -- see this function's own
    // "Ordering invariant" doc above. `Cell` (not `RefCell`): `LiveMeasurement`
    // is `Copy`, and this is only ever written once (`run_session`'s own
    // contract: `Stopped` is emitted exactly once, always last).
    let stopped_measurement: std::cell::Cell<Option<Option<LiveMeasurement>>> =
        std::cell::Cell::new(None);
    let live_sink = |event: LiveEvent| match event {
        LiveEvent::Text { text, start, end } => {
            let outcome = applier.borrow_mut().apply_text(&text, start, end);
            if let Some(detail) = outcome.store_error {
                sink(SessionMessage::StoreWriteFailed {
                    operation: "append_live_segment".to_string(),
                    detail,
                });
            }
            if let Some(panel) = outcome.panel {
                sink(SessionMessage::PanelText(panel));
            }
        }
        LiveEvent::MeterBatch { .. } => {
            // No forwarding needed -- `run_session` already wrote
            // `average_rms`/`queue_len` directly onto `request.meter`
            // (module doc).
        }
        LiveEvent::Error(msg) => {
            sink(SessionMessage::Error(msg));
        }
        LiveEvent::Stopped { measurement } => {
            stopped_measurement.set(Some(measurement));
        }
    };

    let mut engine_call = |samples: &[f32], prompt: &str| {
        engine.transcribe_live(samples, &request.live_options, prompt, cancel)
    };

    run_session(
        consumer,
        &mut engine_call,
        &request.meter,
        &live_sink,
        cancel,
        &request.session_config,
        &RealClock,
        &RealSleeper,
    );

    // `run_session` always calls the sink with exactly one `Stopped` as its
    // very last action before returning -- so this is always `Some`.
    let measurement = stopped_measurement.get().unwrap_or(None);

    // Tear the real capture down *before* announcing the session is over
    // (the ordering invariant this function's doc comment states).
    capture_handle.stop();

    if let Some(id) = session_id {
        match deps.store.complete_live_session(id) {
            Ok(()) => sink(SessionMessage::TableRefresh(id)),
            Err(err) => sink(SessionMessage::StoreWriteFailed {
                operation: "complete_live_session".to_string(),
                detail: err.to_string(),
            }),
        }
    }
    sink(SessionMessage::Stopped { measurement });
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::Backend;
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
        ) -> Result<i64, StoreError> {
            if self.fail {
                return Err(StoreError::Query {
                    detail: "boom".to_string(),
                });
            }
            let mut calls = self.calls.lock().unwrap();
            calls.push((id, start, end, text.to_string()));
            // Ascending, 1-based ids -- like the real store's rowids.
            Ok(calls.len() as i64)
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
        // A failed write has no row to tag a translation against (F-063).
        assert_eq!(out.segment_row_id, None);
    }

    #[test]
    fn apply_text_threads_the_inserted_segment_row_id() {
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

        // Each successful append surfaces its inserted row id (ascending),
        // which F-063 tags the enqueued translation with.
        let out1 = applier.apply_text("hello", Millis(0), Millis(1000));
        let out2 = applier.apply_text("world", Millis(1000), Millis(2000));
        assert_eq!(out1.segment_row_id, Some(1));
        assert_eq!(out2.segment_row_id, Some(2));
    }

    #[test]
    fn apply_text_without_a_session_id_has_no_row_id() {
        let store = FakeStore::default();
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            None, // insert_live_session failed (ADR-018) -- nothing persisted.
            &store,
            None,
            None,
        );

        let out = applier.apply_text("hi there", Millis(0), Millis(500));
        assert_eq!(out.segment_row_id, None);
    }

    // -- SessionApplier: F-063 translation enqueue ----------------------------

    /// The `(text, id)` pairs a scripted translation enqueue records.
    type EnqueueLog = Arc<StdMutex<Vec<(String, Option<i64>)>>>;

    #[test]
    fn apply_text_enqueues_the_filtered_text_tagged_with_the_row_id() {
        let store = FakeStore::default();
        let sid = TranscriptionId::new();
        let enqueued: EnqueueLog = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&enqueued);
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        )
        .with_translation_enqueue(Box::new(move |text, id| {
            sink.lock().unwrap().push((text, id));
        }));

        applier.apply_text("  hello  ", Millis(0), Millis(1000));
        applier.apply_text("world", Millis(1000), Millis(2000));

        // Buzz enqueues the FILTERED text (trimmed), tagged with the inserted
        // segment row id (ascending) so the arriving translation can be
        // persisted onto the same row.
        assert_eq!(
            *enqueued.lock().unwrap(),
            vec![
                ("hello".to_string(), Some(1)),
                ("world".to_string(), Some(2)),
            ]
        );
    }

    #[test]
    fn apply_text_does_not_enqueue_a_filtered_away_utterance() {
        let store = FakeStore::default();
        let sid = TranscriptionId::new();
        let enqueued: EnqueueLog = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&enqueued);
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        )
        .with_translation_enqueue(Box::new(move |text, id| {
            sink.lock().unwrap().push((text, id));
        }));

        // Whitespace-only filters away (Buzz `on_next_transcription:965-967`
        // returns before the `translator.enqueue`): nothing is enqueued.
        applier.apply_text("   ", Millis(0), Millis(500));

        assert!(enqueued.lock().unwrap().is_empty());
    }

    #[test]
    fn apply_text_enqueues_even_when_the_store_write_failed() {
        // A store failure (ADR-018) still enqueues the translation (Buzz
        // enqueues regardless of persistence — it has no store); the row id
        // is `None` so the translation just won't be persisted.
        let store = FakeStore {
            fail: true,
            ..Default::default()
        };
        let sid = TranscriptionId::new();
        let enqueued: EnqueueLog = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&enqueued);
        let mut applier = SessionApplier::new(
            RecordingMode::AppendBelow,
            "\n\n".to_string(),
            true,
            Some(sid),
            &store,
            None,
            None,
        )
        .with_translation_enqueue(Box::new(move |text, id| {
            sink.lock().unwrap().push((text, id));
        }));

        applier.apply_text("hi there", Millis(0), Millis(500));

        assert_eq!(
            *enqueued.lock().unwrap(),
            vec![("hi there".to_string(), None)]
        );
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
    fn record_measured_speed_last_run_wins_per_key() {
        let mut measured = std::collections::BTreeMap::new();
        record_measured_speed(&mut measured, "small", 2.0);
        assert_eq!(measured.get("small"), Some(&2.0));

        record_measured_speed(&mut measured, "small", 0.5);
        assert_eq!(
            measured.get("small"),
            Some(&0.5),
            "a later session for the same key overwrites the earlier entry"
        );

        record_measured_speed(&mut measured, "base", 3.0);
        assert_eq!(measured.len(), 2, "a different key is a separate entry");
    }

    #[test]
    fn record_measured_speed_keys_the_api_backend_separately_from_any_model_size() {
        // Ledger #L14: an API-backend session's measurement lives under its
        // own backend-qualified key, never touching a local size's entry.
        let mut measured = std::collections::BTreeMap::new();
        record_measured_speed(&mut measured, "small", 1.5);
        record_measured_speed(&mut measured, "openai_api", 0.6);

        assert_eq!(measured.get("small"), Some(&1.5));
        assert_eq!(measured.get("openai_api"), Some(&0.6));
        assert_eq!(measured.len(), 2);
    }

    // -- run_live_session: capture-teardown-before-Stopped ordering ---------
    //
    // Review finding: `SessionMessage::Stopped` must never reach the caller
    // until `capture_handle.stop()` has actually completed, or
    // `RecordingVm::finish_stopping` could restart the meter-only capture
    // while the old stream is still tearing down (two streams on one
    // device). This drives a full, real `run_live_session` call (ensure ->
    // load -> insert -> capture.start -> one real batch -> Stop) with a
    // scripted `CaptureHandle` and sink that both append to one shared,
    // order-preserving log, and asserts the capture's entry comes first.

    use humm_core::boundary::{AudioCapture, AudioDevice, CaptureHandle};
    use humm_core::error::AudioError;
    use humm_core::queue::ArtifactEnsurer;

    use crate::recording::RecordingDeps;

    struct OrderingOkStore;
    impl LiveStore for OrderingOkStore {
        fn insert_live_session(
            &self,
            _name: &str,
            _options: &TranscriptionOptions,
        ) -> Result<TranscriptionId, StoreError> {
            Ok(TranscriptionId::new())
        }

        fn append_live_segment(
            &self,
            _id: TranscriptionId,
            _start: Millis,
            _end: Millis,
            _text: &str,
        ) -> Result<i64, StoreError> {
            Ok(1)
        }

        fn complete_live_session(&self, _id: TranscriptionId) -> Result<(), StoreError> {
            Ok(())
        }
    }

    struct OrderingOkEnsurer;
    impl ArtifactEnsurer for OrderingOkEnsurer {
        fn ensure_model(
            &self,
            _model: &ModelSpec,
            _progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Result<PathBuf, CoreError> {
            Ok(PathBuf::from("dummy-model"))
        }

        fn ensure_vad(
            &self,
            _progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Option<PathBuf> {
            None
        }
    }

    /// Returns one utterance, then requests cancellation via the *same*
    /// token `run_session` forwards through -- the Task 8 critical contract
    /// this crate relies on (Stop must abort in-flight calls) -- so this
    /// test drives one real batch through a real session rather than an
    /// artificially empty one.
    struct SelfCancelingEngine;
    impl LiveEngineSession for SelfCancelingEngine {
        fn transcribe_live(
            &mut self,
            _samples: &[f32],
            _opts: &LiveOptions,
            _prompt: &str,
            cancel: &CancelToken,
        ) -> Result<String, EngineError> {
            cancel.cancel();
            Ok("ordering test utterance".to_string())
        }
    }

    struct OrderingOkEngineFactory;
    impl LiveEngineFactory for OrderingOkEngineFactory {
        fn load(
            &self,
            _model_path: &Path,
            _use_gpu: bool,
        ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
            Ok(Box::new(SelfCancelingEngine))
        }
    }

    struct OrderingNeverCalledUploadFactory;
    impl UploadFactory for OrderingNeverCalledUploadFactory {
        fn build(&self, _url: String) -> Box<dyn LiveUpload> {
            unreachable!("request.upload_url is None in the ordering test")
        }
    }

    /// Appends `"capture_stopped"` to the shared log when `stop()` runs.
    struct LoggingCaptureHandle(Arc<StdMutex<Vec<String>>>);
    impl CaptureHandle for LoggingCaptureHandle {
        fn stop(self: Box<Self>) {
            self.0.lock().unwrap().push("capture_stopped".to_string());
        }
    }

    /// Pushes one full batch's worth of uniform-loud samples straight into
    /// the ring on `start()` (synchronously, on the caller's thread -- no
    /// real capture thread involved), so `run_session`'s very first
    /// `drain_ring` already has a whole batch to work with -- no real
    /// sleeping anywhere in this test.
    struct LoggingCapture {
        log: Arc<StdMutex<Vec<String>>>,
    }
    impl AudioCapture for LoggingCapture {
        fn devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
            Ok(Vec::new())
        }

        fn start(
            &mut self,
            _device_id: Option<&str>,
            mut sink: rtrb::Producer<f32>,
            _meter: Arc<MeterSnapshot>,
        ) -> Result<Box<dyn CaptureHandle>, AudioError> {
            for _ in 0..SESSION_RING_CAPACITY {
                let _ = sink.push(0.5);
            }
            Ok(Box::new(LoggingCaptureHandle(Arc::clone(&self.log))))
        }
    }

    fn ordering_test_request() -> LiveSessionRequest {
        LiveSessionRequest {
            device_id: None,
            model: ModelSpec {
                backend: Backend::Whisper,
                size: humm_core::domain::ModelSize::Small,
                hf_repo: None,
            },
            use_gpu: false,
            live_options: LiveOptions {
                language: None,
                task: humm_core::domain::TaskKind::Transcribe,
                temperature: humm_core::domain::DEFAULT_TEMPERATURE.to_vec(),
                threads: None,
            },
            session_config: SessionConfig {
                mode: RecordingMode::AppendBelow,
                transcription_step_secs: 3.5,
                silence_threshold: 0.0025,
                initial_prompt: String::new(),
            },
            separator: "\n\n".to_string(),
            hide_unconfirmed: true,
            options_snapshot: TranscriptionOptions::default(),
            session_name: "Live recording <test>".to_string(),
            export_sink: None,
            upload_url: None,
            meter: Arc::new(MeterSnapshot::new()),
            translation_enqueue: None,
            api_config: None,
        }
    }

    #[test]
    fn stopped_is_sent_only_after_capture_teardown_completes() {
        let log: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let deps = RecordingDeps {
            capture: std::sync::Mutex::new(Box::new(LoggingCapture {
                log: Arc::clone(&log),
            })),
            store: Arc::new(OrderingOkStore),
            model_ensurer: Arc::new(OrderingOkEnsurer),
            engine_factory: Arc::new(OrderingOkEngineFactory),
            upload_factory: Arc::new(OrderingNeverCalledUploadFactory),
            dialogs: Arc::new(crate::import::RfdFileDialogs),
            secrets: Arc::new(humm_core::secrets::MemorySecretStore::new()),
        };
        let cancel = CancelToken::new();

        let log_for_sink = Arc::clone(&log);
        let sink = move |message: SessionMessage| {
            if matches!(message, SessionMessage::Stopped { .. }) {
                log_for_sink
                    .lock()
                    .unwrap()
                    .push("sent:Stopped".to_string());
            }
        };

        run_live_session(&deps, ordering_test_request(), &sink, &cancel);

        assert_eq!(
            log.lock().unwrap().clone(),
            vec!["capture_stopped".to_string(), "sent:Stopped".to_string()],
            "capture_handle.stop() must complete before SessionMessage::Stopped is sent"
        );
    }
}
