//! Live-recording window view-model (F-060, F-004): device list + selection,
//! the Record/Stop UI state machine, sticky-options prefill/persistence
//! (`RecordingSettings`), and the meter-only capture drain — Buzz's
//! `RecordingAmplitudeListener` (`buzz/recording.py`) parity for the window
//! open/not-recording state.
//!
//! Ported from `buzz/widgets/recording_transcriber_widget.py:80-264`
//! (init/layout/state) and `:551-621` (`reset_transcriber_controls`, device
//! change, `on_record_button_clicked`'s state flips).
//!
//! **This task's scope, deliberately:** the Record/Stop click fully flips the
//! UI state machine (options/device/mic-label enablement, presentation/copy
//! bar visibility, meter-only capture teardown+restart, meter reset) — but it
//! does **not** spawn a real transcription session. [`RecordingIntent`]/
//! [`RecordingVm::pending_intent`] is the seam: `app.rs`'s per-frame harvest
//! reads and clears it (a no-op today) so Task 11 can wire the real
//! `live::session` start/stop onto the exact same click without touching this
//! state machine again.
//!
//! **Meter ownership** (decided in Task 5/8 reviews, spec §2.5/§1.1): the
//! capture adapter writes only [`MeterSnapshot::set_current_rms`] per
//! callback. While the window is open and **not** recording, this module's
//! [`MeterOnlyDrain`] (run on [`spawn_meter_only_drain`]'s thread) drains the
//! capture ring, accumulates exactly [`humm_core::boundary::SAMPLE_RATE`]
//! samples (1 s, since the adapter always resamples to 16 kHz), and writes
//! `average_rms` once per accumulation — Buzz's `RecordingAmplitudeListener`
//! never touches `queue_len` either (only a real recording's chunker does),
//! so this drain leaves it untouched (always 0 in meter-only mode).

use std::io::{BufWriter, Write};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use humm_core::boundary::{AudioCapture, AudioDevice, CaptureHandle, MeterSnapshot, SAMPLE_RATE};
use humm_core::cancel::CancelToken;
use humm_core::domain::{Backend, ModelSize, TaskKind, TranscriptionId};
use humm_core::language::Language;
use humm_core::live::{RealSleeper, Sleeper};
use humm_core::queue::ArtifactEnsurer;
use humm_core::secrets::{SecretKey, SecretStore};
use humm_core::settings::{PresentationSettings, RecordingSettings, Settings};

use crate::import::FileDialogs;
use crate::live_session::{ActiveSession, LiveEngineFactory, LiveStore, PrepUpdate, UploadFactory};
use crate::live_translation::LiveTranslator;
use crate::views::options_sheet::{DEFAULT_LLM_MODEL, DEFAULT_LLM_PROMPT, SheetModal};
use crate::views::presentation::PresentationVm;
use crate::views::recording::CopyLabelState;
use crate::views::recording_advanced::AdvancedSheetState;

/// Ring-buffer capacity for the meter-only capture: ~1 s at 16 kHz (the
/// capture adapter's own documented sizing, `humm-audio`'s `capture` module).
const RING_CAPACITY: usize = SAMPLE_RATE as usize;
/// Poll interval for the meter-only drain thread when the ring is empty —
/// short enough to keep the meter responsive without busy-spinning (Buzz's
/// own listener is callback-driven and has no equivalent constant; this is
/// the polling-loop price of humm's ring-buffer threading model, spec §1.1).
const DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The env-gated meter-repaint-cadence log (`HUMM_SMOKE_METER_FRAME_LOG`, M5
/// Task 14 acceptance item 5): appends one epoch-millisecond line every time
/// the recording window actually redraws while
/// [`RecordingStatus::Recording`] (the meter is animating — spec §2.5's
/// ~30 ms repaint-while-visible cadence, `views::audio_meter`) — the
/// consecutive-line deltas *are* the observed frame-to-frame interval, so a
/// windowed run's cadence/jank can be computed from this file after the
/// fact, the same after-the-run-parse shape as [`crate::viewer::DriftLog`]
/// (which this mirrors byte-for-byte in structure). Buffered, flushed on
/// drop; absent unless the env var points at a writable path.
pub(crate) struct MeterFrameLog {
    writer: BufWriter<std::fs::File>,
}

impl MeterFrameLog {
    /// Opens (create-or-append) a frame log at `path`, or `None` if it
    /// cannot be opened. Split from [`Self::from_env`] (mirrors
    /// [`crate::viewer::DriftLog::open`]'s own split) so a unit test can
    /// exercise the write/flush path against a temp file without mutating
    /// real process env.
    pub(crate) fn open(path: &Path) -> Option<Self> {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(|file| MeterFrameLog {
                writer: BufWriter::new(file),
            })
    }

    /// Opens the frame log named by `HUMM_SMOKE_METER_FRAME_LOG`, or `None`
    /// when the var is unset/empty or the path cannot be opened.
    fn from_env() -> Option<Self> {
        let path = std::env::var_os("HUMM_SMOKE_METER_FRAME_LOG")?;
        if path.is_empty() {
            return None;
        }
        Self::open(Path::new(&path))
    }

    /// Appends one line: milliseconds since the Unix epoch (best-effort — a
    /// write error is dropped rather than crashing the recording window;
    /// this is test-only instrumentation).
    pub(crate) fn record_now(&mut self) {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let _ = writeln!(self.writer, "{millis}");
    }
}

impl Drop for MeterFrameLog {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}

/// Buzz `RecordingTranscriberWidget.RecordingStatus` (`:88-90`), extended
/// with a third, humm-only [`RecordingStatus::Stopping`] state: Buzz encodes
/// "stop requested, awaiting the transcriber thread's `finished`/`error`
/// signal" as the *same* `RECORDING` status plus a disabled Record button
/// (`stop_recording`, `:1103-1111`: `self.record_button.setDisabled(True)`);
/// humm makes that wait an explicit state instead; every render-side
/// enablement check that Buzz derives from "is the button disabled" derives
/// from `status != Stopped` (options/device) or `status == Stopping`
/// (button) here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingStatus {
    /// Not recording — the meter-only capture may be running, options/device
    /// controls are enabled.
    #[default]
    Stopped,
    /// A session is preparing (model ensure/engine load) or actively
    /// transcribing. Options/device/mic-label are disabled; the Record
    /// button shows "Stop" and stays clickable (clicking it, or requesting
    /// model-download/engine-load cancellation, both move to
    /// [`RecordingStatus::Stopping`]).
    Recording,
    /// Stop was requested (button click, or a deferred window close, spec
    /// §2.1's two-phase close): the session's cooperative-cancel token has
    /// been tripped and the UI is waiting for its `Stopped` acknowledgement
    /// (Buzz `stop_recording`: Record button disabled until `finished`).
    Stopping,
}

/// A Record/Stop transition the click just made, latched on
/// [`RecordingVm::pending_intent`] for `app.rs`'s harvest to act on (Task 11
/// wires the real session start/stop here; today it is drained and
/// discarded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingIntent {
    /// Stopped → Recording: Task 11 should start the live session.
    StartRequested,
    /// Recording → Stopped: Task 11 should signal + await the session's stop.
    StopRequested,
}

/// The options-group snapshot (model size / task / language / initial
/// prompt), prefilled from [`RecordingSettings`]'s sticky fields the same way
/// [`crate::views::options_sheet::SheetState::from_settings`] prefills the
/// file-transcriber sheet — Buzz's recording window and file sheet share one
/// `TranscriptionOptionsGroupBox` (`show_recording_settings=True`), so the
/// same resolution rules apply: unparsed/absent falls back to
/// [`ModelSize::Small`]/[`TaskKind::Transcribe`]/no forced language/empty
/// prompt, and a persisted `Custom` size resets to `Small` when
/// `custom_model_available` is false (the combo wouldn't list it either).
#[derive(Clone, PartialEq)]
pub struct RecordingOptions {
    /// Which engine runs this session (F-061b): local whisper.cpp, or the
    /// OpenAI-compatible HTTP API. Gates the size-combo/Api-Key-row
    /// visibility in `views::recording::recording_ui` the same way
    /// `views::options_sheet::{size_combo_visible, api_key_row_visible}`
    /// gate the file-options sheet's own rows, and maps straight onto
    /// `ModelSpec.backend` at session-start (`app.rs::start_live_session`).
    /// Close-persisted alongside every other field below (Buzz parity: the
    /// model-*type* combo is written only in `_do_close`,
    /// `recording_transcriber_widget.py:1231-1233` — no immediate-save
    /// handler exists for it, unlike the export/mode fields the advanced
    /// sheet owns).
    pub backend: Backend,
    /// The Api Key row's live buffer (F-061b), shown only while
    /// `backend == Backend::OpenAiApi`. Seeded from the keyring at
    /// [`RecordingVm::open`] time (mirrors
    /// `views::options_sheet::OptionsSheet::new`'s identical seeding for the
    /// file-options sheet's own Api Key row) and committed straight through
    /// to the keyring on blur/Enter ([`crate::views::options_sheet::
    /// render_api_key_row`]) — **never** persisted by [`Self::write_to`]/read
    /// by [`Self::from_settings`] (same "the key travels only through
    /// `SecretStore`, never `settings.toml`" invariant as
    /// `SheetState::api_key`).
    pub api_key: String,
    /// Whether the Api Key row currently renders unmasked (the eye toggle).
    pub api_key_visible: bool,
    /// The whisper.cpp model size.
    pub model_size: ModelSize,
    /// Transcribe, or translate to English.
    pub task: TaskKind,
    /// Forced source language, or `None` for "Detect Language".
    pub language: Option<Language>,
    /// Priming text for the model.
    pub initial_prompt: String,
    /// Silence threshold for skipping batches (RMS), edited live by the
    /// advanced sheet (Task 12, `views::recording_advanced`). The **same**
    /// object [`crate::app::HummApp::start_live_session`] reads at session
    /// start (Buzz: `self.transcription_options.silence_threshold`, the
    /// dialog's own live in-memory mutation target,
    /// `advanced_settings_dialog.py:298-300` +
    /// `recording_transcriber_widget.py:667-673`) — an edit here already
    /// applies to the *next* Record click, before the window ever closes.
    pub silence_threshold: f32,
    /// Line separator used in append modes, stored literal (not escaped —
    /// the advanced sheet owns escaping/unescaping display). Same live-edit
    /// contract as [`Self::silence_threshold`].
    pub line_separator: String,
    /// Chunk size in seconds for append-and-correct mode. Same live-edit
    /// contract as [`Self::silence_threshold`].
    pub transcription_step: f64,
    /// Whether live LLM translation is enabled (F-063, Buzz
    /// `enable_llm_translation`). Close-persisted (Buzz `_do_close:1234-1245`
    /// saves this among the same group as language/task/model/prompt), and
    /// live-edited by the advanced sheet's translation block — a toggle here
    /// shows/hides the recording window's translation panel immediately (Buzz
    /// `:544-547`) and is read at the *next* Record click to decide whether to
    /// spawn a translator.
    pub enable_llm_translation: bool,
    /// The AI model id used for translation (Buzz `llm_model`). Seeded with
    /// [`crate::views::options_sheet::DEFAULT_LLM_MODEL`] when
    /// `settings.recording.llm_model` is unset — the "seeded defaults become
    /// real values on save" rule (ledger #10), so an untouched enabled session
    /// still has a usable model. Held as a plain `String` (not `Option`, unlike
    /// the on-disk `settings.recording.llm_model`) so the advanced sheet's
    /// reused model field ([`crate::views::options_sheet::render_llm_model_prompt_fields`],
    /// an `&mut String` widget) binds to it directly, exactly as
    /// `initial_prompt`/`line_separator` do — [`Self::write_to`] adapts back to
    /// `Option<String>` for storage. An empty (trimmed) value is treated as
    /// "unconfigured" at Record time (translation is skipped with a warning).
    pub llm_model: String,
    /// The AI translation instructions (Buzz `llm_prompt`), seeded with
    /// [`crate::views::options_sheet::DEFAULT_LLM_PROMPT`] when unset. Same
    /// `String`/close-persist/seeded-default contract as [`Self::llm_model`].
    pub llm_prompt: String,
}

/// Manual `Debug` (not derived) so [`RecordingOptions::api_key`] never
/// prints in plaintext — the same redaction discipline
/// `views::options_sheet::SheetState`'s own manual `Debug` impl already
/// established for its identical field.
impl std::fmt::Debug for RecordingOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingOptions")
            .field("backend", &self.backend)
            .field("api_key", &"«redacted»")
            .field("api_key_visible", &self.api_key_visible)
            .field("model_size", &self.model_size)
            .field("task", &self.task)
            .field("language", &self.language)
            .field("initial_prompt", &self.initial_prompt)
            .field("silence_threshold", &self.silence_threshold)
            .field("line_separator", &self.line_separator)
            .field("transcription_step", &self.transcription_step)
            .field("enable_llm_translation", &self.enable_llm_translation)
            .field("llm_model", &self.llm_model)
            .field("llm_prompt", &self.llm_prompt)
            .finish()
    }
}

impl RecordingOptions {
    /// Prefills from `settings.recording`'s sticky fields (Buzz parity:
    /// window-open reads sticky prefs; see the module doc for the shared
    /// resolution rule with the file-transcriber sheet). `silence_threshold`/
    /// `line_separator`/`transcription_step` come from `settings.recording`
    /// too — already clamped/defaulted by [`Settings::load`]
    /// ([`RecordingSettings::clamp`]), so no further validation is needed
    /// here.
    pub fn from_settings(settings: &RecordingSettings, custom_model_available: bool) -> Self {
        let mut model_size = settings
            .model_size
            .as_deref()
            .and_then(|s| ModelSize::from_str(s).ok())
            .unwrap_or(ModelSize::Small);
        if model_size == ModelSize::Custom && !custom_model_available {
            model_size = ModelSize::Small;
        }

        RecordingOptions {
            backend: settings
                .backend
                .as_deref()
                .and_then(|s| Backend::from_str(s).ok())
                .unwrap_or_default(),
            // Seeded separately by `RecordingVm::open` (needs the secret
            // store, which this settings-only prefill has no access to) —
            // mirrors `SheetState::from_settings`'s identical split.
            api_key: String::new(),
            api_key_visible: false,
            model_size,
            task: settings
                .task
                .as_deref()
                .and_then(|s| TaskKind::from_str(s).ok())
                .unwrap_or(TaskKind::Transcribe),
            language: settings.language.as_deref().and_then(Language::parse),
            initial_prompt: settings.initial_prompt.clone().unwrap_or_default(),
            silence_threshold: settings.silence_threshold,
            line_separator: settings.line_separator.clone(),
            transcription_step: settings.transcription_step,
            enable_llm_translation: settings.enable_llm_translation,
            // Seeded-defaults-become-values (ledger #10), exactly like the
            // file-options sheet's `SheetState::from_settings`: an unset
            // sticky model/prompt resolves to the shared J-8 default here (a
            // real, usable value), which `write_to` then persists on close.
            llm_model: settings
                .llm_model
                .clone()
                .unwrap_or_else(|| DEFAULT_LLM_MODEL.to_string()),
            llm_prompt: settings
                .llm_prompt
                .clone()
                .unwrap_or_else(|| DEFAULT_LLM_PROMPT.to_string()),
        }
    }

    /// Writes this snapshot back into `settings` (Buzz `_do_close`,
    /// `recording_transcriber_widget.py:1202-1260` — every field Buzz's own
    /// `_do_close` writes from `self.transcription_options`: language/task/
    /// model/initial-prompt (Task 10) plus silence-threshold/line-separator/
    /// transcription-step (Task 12, `:1247-1256`). The advanced sheet's
    /// *immediate*-save fields — mode/hide-unconfirmed/export-*/upload-url —
    /// are not here; they write straight to `settings.recording` on each
    /// change instead (`views::recording_advanced`'s own module doc).
    pub fn write_to(&self, settings: &mut RecordingSettings) {
        settings.language = self.language.map(|language| language.code().to_string());
        settings.task = Some(self.task.as_str().to_string());
        settings.model_size = Some(self.model_size.as_str().to_string());
        settings.initial_prompt = Some(self.initial_prompt.clone());
        settings.silence_threshold = self.silence_threshold;
        settings.line_separator = self.line_separator.clone();
        settings.transcription_step = self.transcription_step;
        settings.enable_llm_translation = self.enable_llm_translation;
        // `String` -> `Option<String>` for storage: the in-memory value is
        // always populated (seeded default), so this always persists a
        // concrete `Some(..)` (ledger #10), mirroring the file sheet's own
        // `settings.llm_model = Some(self.llm_model.clone())`.
        settings.llm_model = Some(self.llm_model.clone());
        settings.llm_prompt = Some(self.llm_prompt.clone());
        // F-061b: close-persisted alongside every field above (Buzz parity —
        // see this field's own doc comment); `api_key`/`api_key_visible` are
        // deliberately never written here, same as `SheetState::write_to`.
        settings.backend = Some(self.backend.as_str().to_string());
    }
}

/// Buzz `AudioDevicesComboBox.get_default_device_id`
/// (`audio_devices_combo_box.py:46-54`): the system default input device if
/// one is flagged, else the first device in the list, else `None` — Buzz's
/// own `-1` ("no devices, or no default"). An empty `devices` list yields
/// `None`: recording still opens (the Record button stays clickable per
/// Buzz's own `reset_transcriber_controls`, which never gates on device
/// presence), but no meter-only capture starts (see [`RecordingVm::open`]).
pub fn default_device_selection(devices: &[AudioDevice]) -> Option<String> {
    devices
        .iter()
        .find(|device| device.is_default)
        .or_else(|| devices.first())
        .map(|device| device.id.clone())
}

/// The key [`realtime_fit_warning_visible`]/[`crate::live_session::
/// record_measured_speed`] read/write into `settings.recording.measured_speed`
/// for the current backend/model-size combination (F-061b, design spec §10
/// ledger #L14): the model size's own wire string for the local whisper.cpp
/// backend, or [`Backend::OpenAiApi`]'s own wire string (`"openai_api"`) for
/// every API-backend session regardless of size — a slow network's measured
/// ratio must never poison a local model size's own numbers, and the API
/// backend has no size dimension to begin with (its size combo is hidden,
/// `views::recording::recording_ui`'s own gate).
pub fn measured_speed_key(backend: Backend, model_size: ModelSize) -> &'static str {
    match backend {
        Backend::OpenAiApi => Backend::OpenAiApi.as_str(),
        Backend::Whisper => model_size.as_str(),
    }
}

/// Whether the realtime-fit caution label is visible (spec §2.8): `key`
/// (see [`measured_speed_key`]) has a stored measured-speed ratio strictly
/// below `1.0`. No stored entry (never measured, or a different key was
/// measured last) means no warning — first run is the measurement.
pub fn realtime_fit_warning_visible(
    measured_speed: &std::collections::BTreeMap<String, f32>,
    key: &str,
) -> bool {
    measured_speed.get(key).is_some_and(|ratio| *ratio < 1.0)
}

/// The bundle of adapter handles the recording window's render closure and
/// the live-session thread (`crate::live_session::run_live_session`) need.
///
/// `capture` is `Mutex`-wrapped (not `Arc`-cloned per open, like
/// [`crate::viewer::ViewerDeps`]'s read-only adapters) because
/// [`AudioCapture::start`] takes `&mut self`: the single recording window
/// (meter-only capture while stopped) and, once started, the live-session
/// thread (Task 11) share one adapter instance across open/device-change/
/// record/stop calls, so interior mutability is required here where the
/// read-only viewer adapters didn't need it. The two capture users are never
/// concurrent — `RecordingVm::on_record_clicked` always tears the meter-only
/// capture down *before* a session start is requested, and the session
/// thread only restarts it (`RecordingVm::finish_stopping`) after its own
/// `capture_handle.stop()` has already run. That second half is a
/// *mechanically enforced* ordering, not just a documented convention:
/// `live_session::run_live_session` never lets `SessionMessage::Stopped` —
/// the message that triggers `finish_stopping` — reach the UI thread until
/// `capture_handle.stop()` has actually completed (see that function's own
/// "Ordering invariant" doc and its wrapper-ordering test).
pub struct RecordingDeps {
    /// The audio-input capture adapter (`humm_audio::CpalCapture` in
    /// production, `humm_audio::MockCapture` in tests — never the reverse,
    /// since a real `CpalCapture::start` opens an actual input stream and
    /// this crate's kittest coverage must never touch real audio hardware).
    pub capture: Mutex<Box<dyn AudioCapture>>,
    /// The live-session store seam (real `humm_store::SqliteStore` in
    /// production and kittest — already a real, often temp-file, store even
    /// in test builds).
    pub store: Arc<dyn LiveStore>,
    /// The model artifact ensurer (real
    /// `crate::engine_factory::DownloaderEnsurer` in production; a
    /// network-free fake in kittest — Task 11's brief: "do NOT bind TCP in
    /// kittest").
    pub model_ensurer: Arc<dyn ArtifactEnsurer>,
    /// The live engine loader (real `crate::live_session::RealLiveEngineFactory`
    /// in production; a scripted fake in tests).
    pub engine_factory: Arc<dyn LiveEngineFactory>,
    /// The upload-client builder (real `crate::live_session::RealUploadFactory`
    /// in production; a network-free recording fake in kittest).
    pub upload_factory: Arc<dyn UploadFactory>,
    /// Seam over the (blocking, native) folder picker for the advanced
    /// sheet's export-folder Browse button (F-064/Task 12) — always
    /// [`crate::import::RfdFileDialogs`] in both real and kittest
    /// construction, mirroring [`crate::viewer::ViewerDeps::dialogs`] and
    /// `HummApp::dialogs`'s own "always real, nothing in kittest drives it"
    /// convention (no test in this workspace clicks Browse — kittest can't
    /// drive a real rfd dialog).
    pub dialogs: Arc<dyn FileDialogs>,
    /// The secret store the Api Key row's live buffer is seeded from
    /// ([`RecordingVm::open`]) and committed straight through to
    /// ([`crate::views::options_sheet::render_api_key_row`]), and the same
    /// store `app.rs::start_live_session`'s F-061b missing-key precheck
    /// resolves the session's key from (`crate::engine_factory::
    /// resolve_openai_config`) — the OS keyring in production, a
    /// `MemorySecretStore` in tests. One source of truth for "does this
    /// recording window have a usable OpenAI key", matching the rest of
    /// this struct's "real adapter in production, scripted/in-memory fake in
    /// tests" shape.
    pub secrets: Arc<dyn SecretStore>,
}

/// The pure accumulate-and-average logic behind Buzz's
/// `RecordingAmplitudeListener.stream_callback` (`buzz/recording.py:47-57`):
/// appends samples to a buffer; once its length reaches `accumulation_size`
/// (1 s of audio), returns the RMS over the *whole* accumulated buffer (which
/// may be a little larger than `accumulation_size` if the appended chunk
/// overshot it in one push — Buzz's own `np.append` + size check has exactly
/// this same behavior: no carry-over, no clipping to precisely
/// `accumulation_size`), and empties the buffer. Pure and thread-free — the
/// thread wrapper is [`spawn_meter_only_drain`].
pub struct MeterOnlyDrain {
    buffer: Vec<f32>,
    accumulation_size: usize,
}

impl MeterOnlyDrain {
    /// Builds a drain that emits once its buffer reaches `accumulation_size`
    /// samples (`RecordingVm`'s callers always pass
    /// [`humm_core::boundary::SAMPLE_RATE`] — 1 s at the adapter's fixed
    /// 16 kHz output rate).
    pub fn new(accumulation_size: usize) -> Self {
        MeterOnlyDrain {
            buffer: Vec::new(),
            accumulation_size,
        }
    }

    /// Appends `chunk`; once the buffer reaches `accumulation_size`, returns
    /// `Some(rms)` over the whole buffer and empties it. Returns `None` on
    /// every push that doesn't cross the threshold.
    pub fn push(&mut self, chunk: &[f32]) -> Option<f32> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() < self.accumulation_size {
            return None;
        }
        let sum_sq: f64 = self
            .buffer
            .iter()
            .map(|&sample| f64::from(sample) * f64::from(sample))
            .sum();
        let rms = (sum_sq / self.buffer.len() as f64).sqrt() as f32;
        self.buffer.clear();
        Some(rms)
    }
}

/// The meter-only drain thread body (Buzz `RecordingAmplitudeListener`
/// parity): polls `consumer` for whatever the capture ring currently holds,
/// folds it through [`MeterOnlyDrain::push`], writes `meter.set_average_rms`
/// on every accumulated second, and repeats until `cancel` trips. Never
/// writes `queue_len` (see the module doc). `sleeper` is the testability seam
/// ([`humm_core::live::Sleeper`], reused verbatim from `live::export`/
/// `live::session`); production always passes [`RealSleeper`].
fn run_meter_only_drain(
    mut consumer: rtrb::Consumer<f32>,
    meter: Arc<MeterSnapshot>,
    cancel: CancelToken,
    sleeper: &dyn Sleeper,
) {
    let mut drain = MeterOnlyDrain::new(SAMPLE_RATE as usize);
    let mut scratch = Vec::new();
    while !cancel.is_canceled() {
        scratch.clear();
        while let Ok(sample) = consumer.pop() {
            scratch.push(sample);
        }
        if scratch.is_empty() {
            sleeper.sleep(DRAIN_POLL_INTERVAL);
            continue;
        }
        if let Some(rms) = drain.push(&scratch) {
            meter.set_average_rms(rms);
        }
    }
}

/// Spawns [`run_meter_only_drain`] on its own thread with the production
/// [`RealSleeper`].
fn spawn_meter_only_drain(
    consumer: rtrb::Consumer<f32>,
    meter: Arc<MeterSnapshot>,
    cancel: CancelToken,
) -> JoinHandle<()> {
    std::thread::spawn(move || run_meter_only_drain(consumer, meter, cancel, &RealSleeper))
}

/// The live-recording window's state (F-060). Owned by `app.rs` behind
/// `Arc<Mutex<_>>` (the viewer/resizer windows' own shape) since the deferred
/// viewport's render closure needs shared access across frames.
pub struct RecordingVm {
    /// Stopped, Recording, or Stopping — see [`RecordingStatus`]'s doc.
    pub status: RecordingStatus,
    /// Every available input device, refreshed only at [`Self::open`] (Buzz
    /// itself never re-enumerates after construction either).
    pub devices: Vec<AudioDevice>,
    /// The selected device's id, or `None` when `devices` is empty.
    pub selected_device: Option<String>,
    /// The options-group snapshot (model/task/language/prompt).
    pub options: RecordingOptions,
    /// The meter widget's own smoothed display value
    /// (`audio_meter::smoothed_amplitude`'s running state) — GUI-side only,
    /// updated once per repaint from `meter.get_current_rms()`.
    pub meter_current_amplitude: f32,
    /// The shared meter telemetry: `current_rms` from the capture adapter,
    /// `average_rms`/`queue_len` from this module's meter-only drain (while
    /// stopped) or the session/chunker layer (while recording, Task 11).
    pub meter: Arc<MeterSnapshot>,
    /// Set when the Advanced… button is clicked;
    /// [`crate::views::recording_advanced::ui`] consumes and clears it,
    /// opening [`Self::advanced_sheet`].
    pub advanced_requested: bool,
    /// The advanced sheet's own live state while open (Task 12); `None`
    /// while closed. See `views::recording_advanced`'s module doc for the
    /// save-timing split with [`Self::options`].
    pub advanced_sheet: Option<AdvancedSheetState>,
    /// The presentation window's state (Task 12, F-065): open/focus/
    /// fullscreen intents plus the persisted display preferences. Read
    /// fresh from `settings.presentation` at [`Self::open`]; the
    /// presentation viewport (`app.rs::show_presentation_viewport`) is shown
    /// only while both this window and [`Self::presentation`]'s `open` flag
    /// are true, so it closes automatically when the recording window does
    /// (Buzz `_do_close`, `:1204-1206`) — no explicit teardown needed here.
    pub presentation: PresentationVm,
    /// The Copy button's current label/timer state (Task 12, F-065 §2.7).
    pub copy_label: CopyLabelState,
    /// The read-only transcription panel text — the live session thread's
    /// most recent [`crate::live_session::SessionMessage::PanelText`] (a
    /// whole-panel replace), applied verbatim.
    pub panel_text: String,
    /// The read-only **translation** panel text (F-063) — the most recent
    /// whole-panel replace drained from the [`LiveTranslator`]'s dedicated
    /// panel channel ([`LiveTranslator::drain_panels`], applied by
    /// `app.rs::drain_translation_panel`); the text itself was already
    /// mode-dispatched/merged/`strip_newlines`'d by the off-UI-thread
    /// [`humm_core::live::translation::TranslationApplier`]. That channel's
    /// receiver lives on [`Self::live_translator`], which outlives the session,
    /// so a translation arriving after Stop still updates this while the window
    /// is open (spec §7.4). The recording window's translation panel is shown
    /// iff [`RecordingOptions::enable_llm_translation`] (live show/hide on
    /// toggle, Buzz `:544-547`); this text feeds both that panel and, when
    /// non-empty, the presentation window's translation region.
    pub translation_panel_text: String,
    /// Whether the presentation options bar (`views::presentation::bar_ui`
    /// — show-window button, text-size/theme/color controls, fullscreen
    /// button) is shown: visible only while recording (Buzz
    /// `presentation_options_bar`).
    pub presentation_bar_visible: bool,
    /// Whether the copy bar (`views::recording`'s copy-bar render, the
    /// "Copy"/"Copied!"/"Nothing to copy!" button) is shown: visible once a
    /// recording has stopped (Buzz `copy_actions_bar`).
    pub copy_bar_visible: bool,
    /// The Record/Stop transition the click just made, for `app.rs`'s
    /// harvest (`app.rs::harvest_recording_intent` starts/signals the real
    /// session).
    pub pending_intent: Option<RecordingIntent>,
    /// Set by the deferred-viewport render closure when the OS window's
    /// close is requested while [`RecordingStatus::Stopped`] (an immediate
    /// close) **or** once a deferred close (spec §2.1's two-phase close,
    /// see [`Self::pending_close`]) has reached [`RecordingStatus::Stopped`]
    /// again. The app harvests this each frame to persist sticky prefs and
    /// drop the window (`app.rs::harvest_recording_close`, unchanged from
    /// Task 10).
    pub close_requested: bool,
    /// Set when the window is re-invoked (toolbar/Ctrl+R) while already open —
    /// focus it instead of duplicating (the viewer's own deviation #1 shape).
    pub focus_requested: bool,
    /// Set when an OS window-close was requested while
    /// [`RecordingStatus::Recording`]/[`RecordingStatus::Stopping`] (spec
    /// §2.1's two-phase close, Buzz `closeEvent`/`_closing`): the close is
    /// deferred until the active session's `Stopped` acknowledgement
    /// arrives, at which point `app.rs` sets [`Self::close_requested`]
    /// instead of restoring the ordinary stopped-state UI (skipping
    /// [`Self::finish_stopping`]'s meter-only-capture restart, since the
    /// window is about to close anyway — Buzz's own `_do_close` skips
    /// `set_recording_status_stopped` on this path too).
    pub pending_close: bool,
    /// The model-download/engine-load progress row (spec §2.1: "shown
    /// inline in the window"), or `None` once the session is actually
    /// transcribing (or hasn't started).
    pub prep: Option<PrepUpdate>,
    /// Buzz's error-dialog text (byte-exact,
    /// [`crate::live_session::recording_error_dialog_text`]), or `None`
    /// while no error is showing. The render fn shows this as a dismissible
    /// `egui::Modal`; dismissing clears it.
    pub error_dialog: Option<String>,
    /// The Api Key row's commit-failure modal (F-061b), if a commit this
    /// frame produced one — the recording window's own instance of
    /// [`crate::views::options_sheet::OptionsSheet::modal`]'s identical role
    /// for the file-options sheet. Rendered by `views::recording::
    /// recording_ui` via [`crate::views::options_sheet::sheet_modal_title_and_body`].
    pub api_key_modal: Option<SheetModal>,

    /// The active live session, `None` while [`RecordingStatus::Stopped`].
    /// Owned by `app.rs`'s harvest (`start`/`stop`/drain) — see
    /// `crate::live_session`'s module doc for the threading split.
    pub(crate) session: Option<ActiveSession>,
    /// The live-translation worker for this session (F-063), `None` unless the
    /// last Record click had [`RecordingOptions::enable_llm_translation`] on
    /// **and** an endpoint resolved. Owned by the VM (not the session thread)
    /// because it outlives the session: Stop does not kill it (queued items
    /// keep draining, spec §7.4 lifecycle) — it's stopped only at window close
    /// ([`Self::close`]) or the next Record click (`app.rs::start_live_session`
    /// tears down the old one before spawning a fresh worker+applier). The
    /// advanced sheet forwards mid-session model/prompt edits through it
    /// ([`LiveTranslator::update_options`]).
    pub(crate) live_translator: Option<LiveTranslator>,
    /// The current session's store row id, once known — set (idempotently)
    /// whenever a `SessionMessage::TableRefresh` arrives (the same id both
    /// times: right after `insert_live_session` succeeds, and again at
    /// completion). `None` until then, or if `insert_live_session` failed
    /// (ADR-018 — nothing to complete). Exists solely for `app.rs`'s
    /// session-crashed liveness fallback (`Self::session`'s `JoinHandle`
    /// finished without ever sending `Stopped`): normal completion runs
    /// `complete_live_session` on the session thread itself and never reads
    /// this field.
    pub(crate) session_id: Option<TranscriptionId>,

    meter_only_handle: Option<Box<dyn CaptureHandle>>,
    meter_only_cancel: Option<CancelToken>,
    meter_only_thread: Option<JoinHandle<()>>,
    /// `HUMM_SMOKE_METER_FRAME_LOG` acceptance instrumentation (M5 Task 14,
    /// spec §8 item 5) — opened on Record, flushed (via [`MeterFrameLog`]'s
    /// `Drop`) on Stop. `None` for the entire lifetime of every ordinary
    /// (non-smoke) run, since the env var is never set outside a Task 14
    /// windowed acceptance invocation.
    meter_frame_log: Option<MeterFrameLog>,
}

impl RecordingVm {
    /// Opens the recording window's view-model: enumerates input devices via
    /// `deps.capture`, preselects the default (or first, or none — see
    /// [`default_device_selection`]), prefills the options group from
    /// `settings.recording`, and starts the meter-only capture on the
    /// selected device (a no-op when there is none). A device-enumeration
    /// failure is logged and degrades to an empty list (Buzz parity: its own
    /// `get_audio_devices` catches its one documented failure mode and still
    /// opens the window) — this never fails outright.
    pub fn open(deps: &RecordingDeps, settings: &Settings) -> Self {
        let devices = {
            let capture = deps
                .capture
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            capture.devices().unwrap_or_else(|err| {
                log::warn!("could not list audio input devices: {err}");
                Vec::new()
            })
        };
        let selected_device = default_device_selection(&devices);
        let custom_model_available = settings.models.custom_model_url.is_some();
        let mut options =
            RecordingOptions::from_settings(&settings.recording, custom_model_available);
        // F-061b: seed the Api Key row from the keyring here (needs
        // `deps.secrets`, which `RecordingOptions::from_settings` has no
        // access to) — mirrors `OptionsSheet::new`'s identical seeding of
        // `SheetState::api_key` for the file-options sheet.
        options.api_key = deps
            .secrets
            .get(SecretKey::OpenAiApiKey)
            .ok()
            .flatten()
            .unwrap_or_default();

        let mut vm = RecordingVm {
            status: RecordingStatus::Stopped,
            devices,
            selected_device,
            options,
            meter_current_amplitude: 0.0,
            meter: Arc::new(MeterSnapshot::new()),
            advanced_requested: false,
            advanced_sheet: None,
            presentation: PresentationVm::from_settings(&settings.presentation),
            copy_label: CopyLabelState::default(),
            panel_text: String::new(),
            translation_panel_text: String::new(),
            presentation_bar_visible: false,
            copy_bar_visible: false,
            pending_intent: None,
            close_requested: false,
            focus_requested: false,
            pending_close: false,
            prep: None,
            error_dialog: None,
            api_key_modal: None,
            session: None,
            session_id: None,
            live_translator: None,
            meter_only_handle: None,
            meter_only_cancel: None,
            meter_only_thread: None,
            meter_frame_log: None,
        };
        vm.start_meter_only_capture(deps);
        vm
    }

    /// Device combo change while stopped (Buzz `on_device_changed`,
    /// `:563-565`): updates the selection and restarts the meter-only
    /// capture on the new device. A no-op while [`RecordingStatus::Recording`]
    /// (the combo is disabled in the UI then anyway; defensive here too).
    pub fn on_device_changed(&mut self, device_id: String, deps: &RecordingDeps) {
        self.selected_device = Some(device_id);
        if self.status == RecordingStatus::Stopped {
            self.start_meter_only_capture(deps);
        }
    }

    /// Record/Stop button clicked (Buzz `on_record_button_clicked`,
    /// `:591-621`).
    ///
    /// Stopped→Recording: tears down the meter-only capture, resets the
    /// meter (Buzz `audio_meter_widget.reset_amplitude()`, called *before*
    /// the transcriber starts), clears the panel/any stale error dialog,
    /// disables the options/device controls (via `status`, which the render
    /// fn reads), and shows the presentation options bar. Latches
    /// [`Self::pending_intent`] so `app.rs`'s harvest starts the real
    /// session (`crate::live_session::spawn_live_session`).
    ///
    /// Recording→Stopping: latches a stop intent and moves to
    /// [`RecordingStatus::Stopping`] — **no** UI restore and **no**
    /// meter-only-capture restart here (Task 10's original behavior):
    /// spec §2.1/Buzz `stop_recording` disables the button and waits for
    /// the session thread's acknowledgement; `app.rs`'s harvest cancels the
    /// active session and, once its `Stopped` message arrives, calls
    /// [`Self::finish_stopping`] to do the restore this method used to do
    /// eagerly.
    ///
    /// Stopping→Stopping: a defensive no-op (the button is disabled in this
    /// state; nothing double-latches a second stop intent).
    ///
    /// `_deps` is unused now that the Recording→Stopping transition no
    /// longer restarts the meter-only capture eagerly (see
    /// [`Self::finish_stopping`]) — kept in the signature (rather than
    /// removed) so every existing call site (views/tests) stays unchanged.
    pub fn on_record_clicked(&mut self, _deps: &RecordingDeps) {
        match self.status {
            RecordingStatus::Stopped => {
                self.stop_meter_only_capture();
                self.meter.reset();
                self.meter_current_amplitude = 0.0;
                self.status = RecordingStatus::Recording;
                self.presentation_bar_visible = true;
                self.copy_bar_visible = false;
                self.panel_text.clear();
                // Fresh translation panel per session (Buzz clears the
                // translation text box implicitly by rebuilding; humm clears
                // it here so a prior session's translation never lingers).
                self.translation_panel_text.clear();
                self.error_dialog = None;
                self.prep = None;
                self.session_id = None;
                self.pending_intent = Some(RecordingIntent::StartRequested);
                self.meter_frame_log = MeterFrameLog::from_env();
            }
            RecordingStatus::Recording => {
                self.status = RecordingStatus::Stopping;
                self.pending_intent = Some(RecordingIntent::StopRequested);
            }
            RecordingStatus::Stopping => {}
        }
    }

    /// Records one meter-repaint-cadence sample (`HUMM_SMOKE_METER_FRAME_LOG`,
    /// M5 Task 14 acceptance item 5) — a no-op unless the log is armed
    /// (`Self::meter_frame_log`, only ever `Some` during a
    /// `HUMM_SMOKE_LIVE_FIXTURE` windowed acceptance run) **and**
    /// [`Self::status`] is [`RecordingStatus::Recording`] (the meter is
    /// actually animating then — spec §2.5). Called once per actual
    /// recording-window redraw from `app.rs::show_recording_viewport`'s
    /// deferred-viewport closure, i.e. at the window's own real repaint
    /// cadence, not once per root frame.
    pub(crate) fn record_meter_frame(&mut self) {
        if self.status != RecordingStatus::Recording {
            return;
        }
        if let Some(log) = &mut self.meter_frame_log {
            log.record_now();
        }
    }

    /// The session thread's `Stopped` acknowledgement has arrived (Buzz
    /// `on_transcriber_finished`/`on_transcriber_error` ->
    /// `set_recording_status_stopped`, `:755-763`): restores the ordinary
    /// stopped-state UI (options/device re-enabled via `status`,
    /// presentation bar hidden, copy bar shown) and restarts the meter-only
    /// capture. Not called when [`Self::pending_close`] is set — the window
    /// is closing anyway, so `app.rs` sets [`Self::close_requested`]
    /// instead (Buzz's `_do_close` skips this same restore on its
    /// close-while-recording path — [`Self::close`] performs the equivalent
    /// flush on that path instead).
    ///
    /// Also unconditionally flushes any pending debounced presentation
    /// text-size/color edit (`PresentationVm::flush_pending_save`,
    /// review fix): hiding the presentation bar here
    /// (`presentation_bar_visible = false`) means `views::presentation::bar_ui`
    /// — the only place that drives the normal ~100 ms debounce
    /// (`drive_pending_save_debounce`) — stops rendering the instant this
    /// returns. Without this flush, an edit made in the last ~100 ms before
    /// Stop is silently lost: `pending_save_last_edit` stays armed forever
    /// with nothing left to ever check it again until the *next* recording
    /// re-shows the bar, by which point the in-memory value has often moved
    /// on. Returns whether anything was actually flushed, so the caller
    /// (`app.rs`) knows whether `presentation_settings` needs persisting.
    #[must_use]
    pub fn finish_stopping(
        &mut self,
        deps: &RecordingDeps,
        presentation_settings: &mut PresentationSettings,
    ) -> bool {
        self.status = RecordingStatus::Stopped;
        self.presentation_bar_visible = false;
        self.copy_bar_visible = true;
        self.prep = None;
        self.start_meter_only_capture(deps);
        self.meter_frame_log = None; // flushes via MeterFrameLog::Drop
        self.presentation.flush_pending_save(presentation_settings)
    }

    /// Window close while stopped (Buzz `_do_close`,
    /// `recording_transcriber_widget.py:1202-1260`): tears down the
    /// meter-only capture and writes the sticky options back into
    /// `settings` (the caller persists `settings` to disk — this only
    /// mutates the in-memory struct, matching
    /// [`crate::views::options_sheet::SheetState::write_to`]'s own split of
    /// concerns). Also drops any lingering [`Self::session`] handle
    /// (defensive — by the time this runs the session has already sent its
    /// `Stopped` message and `app.rs` has taken it, so this is normally a
    /// no-op).
    ///
    /// Does **not** flush a pending presentation edit itself (unlike
    /// [`Self::finish_stopping`]) — this method only ever touches
    /// `RecordingSettings`, and the caller (`app.rs::harvest_recording_close`)
    /// already holds the full `Settings` lock this window's close needs, so
    /// it calls [`crate::views::presentation::PresentationVm::flush_pending_save`]
    /// directly on `self.presentation` alongside this call, piggybacking on
    /// the same unconditional post-close `settings.save`. The invariant
    /// still holds (review fix): the whole recording window disappearing
    /// means every future `bar_ui` render disappears with it, so a pending
    /// edit must be flushed here too, one way or another.
    pub fn close(&mut self, settings: &mut RecordingSettings) {
        self.stop_meter_only_capture();
        self.session = None;
        // Stop the live-translation worker (Buzz `_do_close:1213-1214`:
        // `translator.stop()` + non-blocking thread quit) — `LiveTranslator::stop`
        // is detached, so this never blocks the UI thread on an in-flight
        // request. `Drop` is the backstop for any path that skips `close`.
        if let Some(translator) = &self.live_translator {
            translator.stop();
        }
        self.live_translator = None;
        self.options.write_to(settings);
    }

    /// Stops and joins any running meter-only capture (idempotent — a no-op
    /// when none is running). Always run before starting a new one, and on
    /// window close/Record click.
    fn stop_meter_only_capture(&mut self) {
        if let Some(cancel) = self.meter_only_cancel.take() {
            cancel.cancel();
        }
        if let Some(handle) = self.meter_only_handle.take() {
            handle.stop();
        }
        if let Some(thread) = self.meter_only_thread.take() {
            let _ = thread.join();
        }
    }

    /// Starts the meter-only capture on [`Self::selected_device`] (Buzz
    /// `reset_recording_amplitude_listener`, `:567-585`): a no-op when
    /// [`Self::devices`] is empty (Buzz: `selected_device_id == -1` guard —
    /// "listening to audio will fail if there are no input devices", so it
    /// simply never starts one; the Record button stays clickable
    /// regardless). Tears down any existing capture first (idempotent
    /// restart, e.g. on a device change).
    fn start_meter_only_capture(&mut self, deps: &RecordingDeps) {
        self.stop_meter_only_capture();
        if self.devices.is_empty() {
            return;
        }

        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
        let mut capture = deps
            .capture
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match capture.start(
            self.selected_device.as_deref(),
            producer,
            Arc::clone(&self.meter),
        ) {
            Ok(handle) => {
                let cancel = CancelToken::new();
                let thread =
                    spawn_meter_only_drain(consumer, Arc::clone(&self.meter), cancel.clone());
                self.meter_only_handle = Some(handle);
                self.meter_only_cancel = Some(cancel);
                self.meter_only_thread = Some(thread);
            }
            Err(err) => {
                log::warn!("meter-only capture failed to start: {err}");
            }
        }
    }
}

impl Drop for RecordingVm {
    /// Backstop for [`Self::close`]: guarantees the meter-only drain thread
    /// is always canceled and joined when a `RecordingVm` goes away, even
    /// via a path that never calls `close` (a test fixture going out of
    /// scope; a future close-while-recording path Task 11 might add). The
    /// normal window-close flow (`app.rs`'s `harvest_recording_close`) already
    /// calls [`Self::close`] first, so this is almost always a no-op by the
    /// time it runs (`stop_meter_only_capture` is idempotent either way).
    fn drop(&mut self) {
        self.stop_meter_only_capture();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    fn device(id: &str, name: &str, is_default: bool) -> AudioDevice {
        AudioDevice {
            id: id.to_string(),
            name: name.to_string(),
            is_default,
        }
    }

    // -- MeterFrameLog ----------------------------------------------------------

    #[test]
    fn meter_frame_log_writes_one_line_per_record_and_flushes_on_drop() {
        // Mirrors `viewer::tests::drift_log_writes_pairs_when_env_set`'s own
        // "inject the path, don't mutate real process env" shape.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("meter-frames.log");
        {
            let mut log = MeterFrameLog::open(&path).expect("open meter frame log");
            log.record_now();
            log.record_now();
        } // drop flushes the buffered writer
        let lines: Vec<i64> = std::fs::read_to_string(&path)
            .expect("read")
            .lines()
            .map(|line| line.parse().expect("epoch millis line"))
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1] >= lines[0]);
    }

    #[test]
    fn record_meter_frame_only_writes_while_recording() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("meter-frames.log");
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        vm.meter_frame_log = MeterFrameLog::open(&path);

        // Stopped: no-op.
        vm.record_meter_frame();
        // Recording: writes.
        vm.status = RecordingStatus::Recording;
        vm.record_meter_frame();
        vm.record_meter_frame();
        vm.meter_frame_log = None; // flush

        let written = std::fs::read_to_string(&path).expect("read");
        assert_eq!(written.lines().count(), 2);
    }

    // -- default_device_selection ---------------------------------------------

    #[test]
    fn prefers_the_default_flagged_device() {
        let devices = vec![
            device("a", "Device A", false),
            device("b", "Device B", true),
            device("c", "Device C", false),
        ];
        assert_eq!(default_device_selection(&devices), Some("b".to_string()));
    }

    #[test]
    fn falls_back_to_the_first_device_when_none_is_default() {
        let devices = vec![
            device("a", "Device A", false),
            device("b", "Device B", false),
        ];
        assert_eq!(default_device_selection(&devices), Some("a".to_string()));
    }

    #[test]
    fn empty_device_list_selects_none() {
        assert_eq!(default_device_selection(&[]), None);
    }

    // -- RecordingOptions::from_settings ---------------------------------------

    #[test]
    fn from_settings_on_a_fresh_install_matches_buzz_defaults() {
        let settings = RecordingSettings::default();
        let options = RecordingOptions::from_settings(&settings, false);
        assert_eq!(options.backend, Backend::Whisper);
        assert_eq!(options.api_key, "");
        assert!(!options.api_key_visible);
        assert_eq!(options.model_size, ModelSize::Small);
        assert_eq!(options.task, TaskKind::Transcribe);
        assert_eq!(options.language, None);
        assert_eq!(options.initial_prompt, "");
        assert_eq!(
            options.silence_threshold,
            RecordingSettings::DEFAULT_SILENCE_THRESHOLD
        );
        assert_eq!(
            options.line_separator,
            RecordingSettings::DEFAULT_LINE_SEPARATOR
        );
        assert_eq!(
            options.transcription_step,
            RecordingSettings::DEFAULT_TRANSCRIPTION_STEP
        );
        // F-063: a fresh install has translation off, and the seeded J-8
        // defaults become the real in-memory model/prompt values (ledger #10).
        assert!(!options.enable_llm_translation);
        assert_eq!(options.llm_model, DEFAULT_LLM_MODEL);
        assert_eq!(options.llm_prompt, DEFAULT_LLM_PROMPT);
    }

    #[test]
    fn from_settings_keeps_stored_llm_values_over_the_seeded_defaults() {
        let settings = RecordingSettings {
            enable_llm_translation: true,
            llm_model: Some("gpt-4o".to_string()),
            llm_prompt: Some("Translate to French.".to_string()),
            ..Default::default()
        };
        let options = RecordingOptions::from_settings(&settings, false);
        assert!(options.enable_llm_translation);
        assert_eq!(options.llm_model, "gpt-4o");
        assert_eq!(options.llm_prompt, "Translate to French.");
    }

    #[test]
    fn from_settings_falls_back_to_small_when_custom_is_stored_but_unavailable() {
        let settings = RecordingSettings {
            model_size: Some("custom".to_string()),
            ..Default::default()
        };
        let options = RecordingOptions::from_settings(&settings, false);
        assert_eq!(options.model_size, ModelSize::Small);
    }

    #[test]
    fn from_settings_keeps_custom_when_available() {
        let settings = RecordingSettings {
            model_size: Some("custom".to_string()),
            ..Default::default()
        };
        let options = RecordingOptions::from_settings(&settings, true);
        assert_eq!(options.model_size, ModelSize::Custom);
    }

    #[test]
    fn options_round_trip_through_settings() {
        let mut options = RecordingOptions::from_settings(&RecordingSettings::default(), false);
        options.backend = Backend::OpenAiApi;
        options.model_size = ModelSize::LargeV3Turbo;
        options.task = TaskKind::Translate;
        options.language = Language::parse("fr");
        options.initial_prompt = "proper nouns: Buzz, humm".to_string();
        options.silence_threshold = 0.01;
        options.line_separator = "---".to_string();
        options.transcription_step = 4.2;
        options.enable_llm_translation = true;
        options.llm_model = "gpt-4.1-mini".to_string();
        options.llm_prompt = "Translate to German, terse.".to_string();

        let mut settings = RecordingSettings::default();
        options.write_to(&mut settings);
        let round_tripped = RecordingOptions::from_settings(&settings, false);

        assert_eq!(round_tripped, options);
    }

    #[test]
    fn llm_fields_close_persist_round_trip_through_settings() {
        // The three F-063 fields ride the same close-persisted group as the
        // rest of `RecordingOptions` (Buzz `_do_close:1234-1245`): a
        // write-then-reload preserves them exactly.
        let mut options = RecordingOptions::from_settings(&RecordingSettings::default(), false);
        options.enable_llm_translation = true;
        options.llm_model = "gpt-4o".to_string();
        options.llm_prompt = "Custom instructions.".to_string();

        let mut settings = RecordingSettings::default();
        options.write_to(&mut settings);

        assert!(settings.enable_llm_translation);
        assert_eq!(settings.llm_model.as_deref(), Some("gpt-4o"));
        assert_eq!(settings.llm_prompt.as_deref(), Some("Custom instructions."));

        let reloaded = RecordingOptions::from_settings(&settings, false);
        assert_eq!(reloaded.enable_llm_translation, true);
        assert_eq!(reloaded.llm_model, "gpt-4o");
        assert_eq!(reloaded.llm_prompt, "Custom instructions.");
    }

    // -- measured_speed_key ------------------------------------------------------

    #[test]
    fn measured_speed_key_is_the_model_size_for_the_whisper_backend() {
        assert_eq!(
            measured_speed_key(Backend::Whisper, ModelSize::Small),
            "small"
        );
        assert_eq!(
            measured_speed_key(Backend::Whisper, ModelSize::LargeV3Turbo),
            "large-v3-turbo"
        );
    }

    #[test]
    fn measured_speed_key_is_backend_qualified_for_the_api_backend_regardless_of_size() {
        assert_eq!(
            measured_speed_key(Backend::OpenAiApi, ModelSize::Small),
            "openai_api"
        );
        assert_eq!(
            measured_speed_key(Backend::OpenAiApi, ModelSize::LargeV3Turbo),
            "openai_api"
        );
    }

    // -- realtime_fit_warning_visible -------------------------------------------

    #[test]
    fn no_measured_entry_hides_the_warning() {
        assert!(!realtime_fit_warning_visible(&BTreeMap::new(), "small"));
    }

    #[test]
    fn a_ratio_at_or_above_one_hides_the_warning() {
        let mut measured = BTreeMap::new();
        measured.insert("small".to_string(), 1.2);
        assert!(!realtime_fit_warning_visible(&measured, "small"));

        measured.insert("small".to_string(), 1.0);
        assert!(!realtime_fit_warning_visible(&measured, "small"));
    }

    #[test]
    fn a_ratio_below_one_shows_the_warning() {
        let mut measured = BTreeMap::new();
        measured.insert("small".to_string(), 0.8);
        assert!(realtime_fit_warning_visible(&measured, "small"));
    }

    #[test]
    fn a_measured_entry_for_a_different_key_does_not_apply() {
        let mut measured = BTreeMap::new();
        measured.insert("base".to_string(), 0.5);
        assert!(!realtime_fit_warning_visible(&measured, "small"));
    }

    #[test]
    fn a_ratio_below_one_under_the_api_backend_key_shows_the_warning_without_touching_local_sizes()
    {
        // Ledger #L14: the network-speed measurement lives under its own
        // backend-qualified key, never poisoning (or being poisoned by) a
        // local model size's own numbers.
        let mut measured = BTreeMap::new();
        measured.insert("small".to_string(), 1.5); // local size stays fast
        measured.insert("openai_api".to_string(), 0.6); // slow network run
        assert!(!realtime_fit_warning_visible(&measured, "small"));
        assert!(realtime_fit_warning_visible(&measured, "openai_api"));
    }

    // -- MeterOnlyDrain ----------------------------------------------------------

    #[test]
    fn emits_nothing_before_the_accumulation_size_is_reached() {
        let mut drain = MeterOnlyDrain::new(10);
        assert_eq!(drain.push(&[0.5; 9]), None);
    }

    #[test]
    fn emits_the_rms_exactly_at_the_boundary_and_resets() {
        let mut drain = MeterOnlyDrain::new(4);
        // RMS of [1.0, 1.0, 1.0, 1.0] is 1.0.
        let rms = drain.push(&[1.0, 1.0, 1.0, 1.0]).expect("boundary reached");
        assert!((rms - 1.0).abs() < 1e-6);

        // Buffer was reset: the next push starts from empty again.
        assert_eq!(drain.push(&[0.5; 3]), None);
    }

    #[test]
    fn a_single_push_that_overshoots_the_boundary_averages_the_whole_buffer() {
        // accumulation_size=4, but one push delivers 6 samples in one go —
        // Buzz's own np.append + size-check has the same "may overshoot,
        // never clips" behavior (module doc).
        let mut drain = MeterOnlyDrain::new(4);
        let rms = drain
            .push(&[2.0, 2.0, 2.0, 2.0, 2.0, 2.0])
            .expect("overshoot still crosses the boundary");
        assert!((rms - 2.0).abs() < 1e-6);
    }

    #[test]
    fn accumulates_across_multiple_pushes_before_emitting() {
        let mut drain = MeterOnlyDrain::new(6);
        assert_eq!(drain.push(&[1.0, 1.0]), None);
        assert_eq!(drain.push(&[1.0, 1.0]), None);
        let rms = drain
            .push(&[1.0, 1.0])
            .expect("sixth sample crosses the boundary");
        assert!((rms - 1.0).abs() < 1e-6);
    }

    #[test]
    fn silence_emits_zero_rms() {
        let mut drain = MeterOnlyDrain::new(4);
        let rms = drain.push(&[0.0, 0.0, 0.0, 0.0]).expect("boundary reached");
        assert_eq!(rms, 0.0);
    }

    // -- RecordingVm state machine (MockCapture-backed) --------------------------

    /// A [`LiveStore`] that panics if ever called — nothing in this module's
    /// tests drives a real session (that's `live_session`'s own unit tests
    /// and the kittest end-to-end coverage); this is only here so
    /// [`RecordingDeps`] can be constructed at all.
    struct NeverCalledStore;
    impl LiveStore for NeverCalledStore {
        fn insert_live_session(
            &self,
            _name: &str,
            _options: &humm_core::domain::TranscriptionOptions,
        ) -> Result<humm_core::domain::TranscriptionId, humm_core::error::StoreError> {
            unreachable!("recording.rs's own tests never start a real session")
        }
        fn append_live_segment(
            &self,
            _id: humm_core::domain::TranscriptionId,
            _start: humm_core::domain::Millis,
            _end: humm_core::domain::Millis,
            _text: &str,
        ) -> Result<i64, humm_core::error::StoreError> {
            unreachable!("recording.rs's own tests never start a real session")
        }
        fn complete_live_session(
            &self,
            _id: humm_core::domain::TranscriptionId,
        ) -> Result<(), humm_core::error::StoreError> {
            unreachable!("recording.rs's own tests never start a real session")
        }
    }

    struct NeverCalledEnsurer;
    impl ArtifactEnsurer for NeverCalledEnsurer {
        fn ensure_model(
            &self,
            _model: &humm_core::domain::ModelSpec,
            _progress: &mut dyn FnMut(humm_core::models::download::DownloadProgress),
            _cancel: &CancelToken,
        ) -> Result<std::path::PathBuf, humm_core::error::CoreError> {
            unreachable!("recording.rs's own tests never start a real session")
        }

        fn ensure_vad(
            &self,
            _progress: &mut dyn FnMut(humm_core::models::download::DownloadProgress),
            _cancel: &CancelToken,
        ) -> Option<std::path::PathBuf> {
            unreachable!("recording.rs's own tests never start a real session")
        }
    }

    struct NeverCalledEngineFactory;
    impl LiveEngineFactory for NeverCalledEngineFactory {
        fn load(
            &self,
            _model_path: &std::path::Path,
            _use_gpu: bool,
        ) -> Result<Box<dyn crate::live_session::LiveEngineSession>, humm_core::error::EngineError>
        {
            unreachable!("recording.rs's own tests never start a real session")
        }
    }

    struct NeverCalledUploadFactory;
    impl UploadFactory for NeverCalledUploadFactory {
        fn build(&self, _url: String) -> Box<dyn crate::live_session::LiveUpload> {
            unreachable!("recording.rs's own tests never start a real session")
        }
    }

    fn mock_deps(devices: Vec<AudioDevice>) -> RecordingDeps {
        RecordingDeps {
            capture: Mutex::new(Box::new(
                humm_audio::MockCapture::new(Vec::new()).with_devices(devices),
            )),
            store: Arc::new(NeverCalledStore),
            model_ensurer: Arc::new(NeverCalledEnsurer),
            engine_factory: Arc::new(NeverCalledEngineFactory),
            upload_factory: Arc::new(NeverCalledUploadFactory),
            dialogs: Arc::new(crate::import::RfdFileDialogs),
            secrets: Arc::new(humm_core::secrets::MemorySecretStore::new()),
        }
    }

    #[test]
    fn open_preselects_the_default_device_and_starts_meter_only_capture() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let vm = RecordingVm::open(&deps, &Settings::default());
        assert_eq!(vm.status, RecordingStatus::Stopped);
        assert_eq!(vm.selected_device, Some("a".to_string()));
    }

    #[test]
    fn open_seeds_the_api_key_row_from_the_keyring() {
        // F-061b: mirrors `OptionsSheet::new`'s identical seeding of
        // `SheetState::api_key` for the file-options sheet.
        let secrets: Arc<dyn SecretStore> = Arc::new(humm_core::secrets::MemorySecretStore::new());
        secrets.set(SecretKey::OpenAiApiKey, "sk-test-123").unwrap();
        let deps = RecordingDeps {
            secrets,
            ..mock_deps(vec![device("a", "Device A", true)])
        };
        let vm = RecordingVm::open(&deps, &Settings::default());
        assert_eq!(vm.options.api_key, "sk-test-123");
        assert!(!vm.options.api_key_visible);
    }

    #[test]
    fn open_with_no_stored_key_seeds_an_empty_api_key_row() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let vm = RecordingVm::open(&deps, &Settings::default());
        assert_eq!(vm.options.api_key, "");
    }

    #[test]
    fn record_click_flips_stopped_to_recording_and_latches_start_intent() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());

        vm.on_record_clicked(&deps);

        assert_eq!(vm.status, RecordingStatus::Recording);
        assert_eq!(vm.pending_intent, Some(RecordingIntent::StartRequested));
        assert!(vm.presentation_bar_visible);
        assert!(!vm.copy_bar_visible);
    }

    #[test]
    fn second_record_click_moves_recording_to_stopping_and_latches_stop_intent() {
        // Two-phase stop (spec §2.1, Task 11): the click only latches the
        // stop intent and moves to `Stopping` — the UI restore (bars,
        // meter-only capture) waits for `finish_stopping`, called once the
        // session thread's `Stopped` message arrives (`app.rs`'s harvest).
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        vm.on_record_clicked(&deps);

        vm.on_record_clicked(&deps);

        assert_eq!(vm.status, RecordingStatus::Stopping);
        assert_eq!(vm.pending_intent, Some(RecordingIntent::StopRequested));
        // Bars are unchanged from the Recording state — still mid-stop.
        assert!(vm.presentation_bar_visible);
        assert!(!vm.copy_bar_visible);
    }

    #[test]
    fn a_third_click_while_stopping_is_a_defensive_no_op() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        vm.on_record_clicked(&deps); // -> Recording
        vm.on_record_clicked(&deps); // -> Stopping
        vm.pending_intent = None;

        vm.on_record_clicked(&deps);

        assert_eq!(vm.status, RecordingStatus::Stopping);
        assert_eq!(vm.pending_intent, None);
    }

    #[test]
    fn finish_stopping_restores_the_stopped_ui_and_restarts_the_meter_only_capture() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        vm.on_record_clicked(&deps); // -> Recording
        vm.on_record_clicked(&deps); // -> Stopping
        vm.prep = Some(crate::live_session::PrepUpdate::LoadingEngine);

        let mut presentation_settings = PresentationSettings::default();
        let flushed = vm.finish_stopping(&deps, &mut presentation_settings);

        assert_eq!(vm.status, RecordingStatus::Stopped);
        assert!(!vm.presentation_bar_visible);
        assert!(vm.copy_bar_visible);
        assert_eq!(vm.prep, None);
        assert!(
            !flushed,
            "nothing was pending -- no presentation edit was made"
        );
        assert_eq!(
            presentation_settings,
            PresentationSettings::default(),
            "untouched when nothing was pending"
        );
    }

    /// Review fix: a presentation text-size/color edit made in the last
    /// ~100ms before Stop must not be silently lost. Before the fix,
    /// `finish_stopping` hid the presentation bar (`presentation_bar_visible
    /// = false`) with the pending edit still only sitting in
    /// `PresentationVm::pending_save_last_edit` — nothing was ever going to
    /// render `bar_ui` again to drive `drive_pending_save_debounce` to
    /// completion, so the edit never reached `settings.presentation`.
    #[test]
    fn finish_stopping_flushes_a_pending_presentation_edit_before_the_debounce_elapses() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        vm.on_record_clicked(&deps); // -> Recording, presentation bar shows

        // Simulate a color edit `bar_ui` just staged (field updated, shared
        // debounce timer armed) — an injected clock (`Instant::now()`, never
        // advanced) stands in for "well within the 100ms window", proving
        // the flush does not depend on any elapsed time at all.
        vm.presentation.text_color = "#abcdef".to_string();
        vm.presentation.pending_save_last_edit = Some(std::time::Instant::now());

        vm.on_record_clicked(&deps); // -> Stopping (user hits Stop mid-edit)

        let mut presentation_settings = PresentationSettings::default();
        let flushed = vm.finish_stopping(&deps, &mut presentation_settings);

        assert!(flushed, "the pending edit must be reported as flushed");
        assert_eq!(
            presentation_settings.text_color, "#abcdef",
            "settings.presentation must contain the new value, not the pre-edit default"
        );
        assert_eq!(
            vm.presentation.pending_save_last_edit, None,
            "the pending timer must clear so a later real debounce fire doesn't double-save"
        );
    }

    #[test]
    fn record_click_with_no_devices_still_flips_status() {
        // Buzz semantics: the Record button is never gated on device
        // presence (`reset_transcriber_controls`); the meter-only capture
        // simply never starts (`start_meter_only_capture`'s own empty-list
        // guard).
        let deps = mock_deps(Vec::new());
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        assert_eq!(vm.selected_device, None);

        vm.on_record_clicked(&deps);

        assert_eq!(vm.status, RecordingStatus::Recording);
    }

    #[test]
    fn device_changed_while_stopped_updates_the_selection() {
        let deps = mock_deps(vec![
            device("a", "Device A", true),
            device("b", "Device B", false),
        ]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        assert_eq!(vm.selected_device, Some("a".to_string()));

        vm.on_device_changed("b".to_string(), &deps);

        assert_eq!(vm.selected_device, Some("b".to_string()));
    }

    /// Carried M5 minor (task G): the `on_device_changed` recording-branch
    /// guard was untested. While [`RecordingStatus::Recording`], a device
    /// change updates the selection (defensive — the combo is disabled in the
    /// UI then) but must **not** restart the meter-only capture: the real
    /// session owns the shared capture adapter for its lifetime, so a
    /// meter-only restart here would race two streams onto one device (the
    /// exact failure `start_meter_only_capture`'s teardown-before-start
    /// ordering exists to avoid). `on_record_clicked` (Stopped→Recording)
    /// already tore the meter-only capture down, so its thread handle is
    /// `None`; this pins that a device change while Recording leaves it `None`.
    #[test]
    fn device_changed_while_recording_updates_selection_without_restarting_meter_capture() {
        let deps = mock_deps(vec![
            device("a", "Device A", true),
            device("b", "Device B", false),
        ]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        // Open started a meter-only capture (a default device exists).
        assert!(vm.meter_only_thread.is_some());

        vm.on_record_clicked(&deps); // Stopped -> Recording: tears the meter-only capture down.
        assert_eq!(vm.status, RecordingStatus::Recording);
        assert!(
            vm.meter_only_thread.is_none(),
            "the Record click stops meter-only capture (the session owns capture now)"
        );

        vm.on_device_changed("b".to_string(), &deps);

        assert_eq!(vm.selected_device, Some("b".to_string()));
        assert!(
            vm.meter_only_thread.is_none(),
            "a device change while Recording must NOT restart meter-only capture"
        );
    }

    #[test]
    fn close_writes_options_back_into_settings() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        vm.options.model_size = ModelSize::Base;

        let mut settings = RecordingSettings::default();
        vm.close(&mut settings);

        assert_eq!(settings.model_size, Some("base".to_string()));
    }
}
