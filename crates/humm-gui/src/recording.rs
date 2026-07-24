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

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use humm_core::boundary::{AudioCapture, AudioDevice, CaptureHandle, MeterSnapshot, SAMPLE_RATE};
use humm_core::cancel::CancelToken;
use humm_core::domain::{ModelSize, TaskKind};
use humm_core::language::Language;
use humm_core::live::{RealSleeper, Sleeper};
use humm_core::settings::{RecordingSettings, Settings};

/// Ring-buffer capacity for the meter-only capture: ~1 s at 16 kHz (the
/// capture adapter's own documented sizing, `humm-audio`'s `capture` module).
const RING_CAPACITY: usize = SAMPLE_RATE as usize;
/// Poll interval for the meter-only drain thread when the ring is empty —
/// short enough to keep the meter responsive without busy-spinning (Buzz's
/// own listener is callback-driven and has no equivalent constant; this is
/// the polling-loop price of humm's ring-buffer threading model, spec §1.1).
const DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Buzz `RecordingTranscriberWidget.RecordingStatus` (`:88-90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingStatus {
    /// Not recording — the meter-only capture may be running, options/device
    /// controls are enabled.
    #[default]
    Stopped,
    /// Recording is (nominally, pending Task 11) in progress — options/
    /// device/mic-label are disabled, the presentation bar placeholder shows.
    Recording,
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
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingOptions {
    /// The whisper.cpp model size.
    pub model_size: ModelSize,
    /// Transcribe, or translate to English.
    pub task: TaskKind,
    /// Forced source language, or `None` for "Detect Language".
    pub language: Option<Language>,
    /// Priming text for the model.
    pub initial_prompt: String,
}

impl RecordingOptions {
    /// Prefills from `settings.recording`'s sticky fields (Buzz parity:
    /// window-open reads sticky prefs; see the module doc for the shared
    /// resolution rule with the file-transcriber sheet).
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
            model_size,
            task: settings
                .task
                .as_deref()
                .and_then(|s| TaskKind::from_str(s).ok())
                .unwrap_or(TaskKind::Transcribe),
            language: settings.language.as_deref().and_then(Language::parse),
            initial_prompt: settings.initial_prompt.clone().unwrap_or_default(),
        }
    }

    /// Writes this snapshot back into `settings` (Buzz `_do_close`,
    /// `recording_transcriber_widget.py:1202-1260` — the language/task/model/
    /// initial-prompt fields Task 10 owns; the recording-specific advanced
    /// fields, e.g. silence threshold/export, are Task 12's).
    pub fn write_to(&self, settings: &mut RecordingSettings) {
        settings.language = self.language.map(|language| language.code().to_string());
        settings.task = Some(self.task.as_str().to_string());
        settings.model_size = Some(self.model_size.as_str().to_string());
        settings.initial_prompt = Some(self.initial_prompt.clone());
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

/// Whether the realtime-fit caution label is visible (spec §2.8): the
/// selected model size has a stored measured-speed ratio strictly below
/// `1.0`. No stored entry (never measured, or a different size was measured
/// last) means no warning — first run is the measurement.
pub fn realtime_fit_warning_visible(
    measured_speed: &std::collections::BTreeMap<String, f32>,
    model_size: ModelSize,
) -> bool {
    measured_speed
        .get(model_size.as_str())
        .is_some_and(|ratio| *ratio < 1.0)
}

/// The bundle of adapter handles the recording window's render closure
/// needs. Only `capture` this task (F-060 skeleton); the engine/session/store
/// seams Task 11 needs arrive there, per the task brief — adding them now
/// would be seam creep with nothing yet to plug into them.
///
/// `Mutex`-wrapped (not `Arc`-cloned per open, like [`crate::viewer::ViewerDeps`]'s
/// read-only adapters) because [`AudioCapture::start`] takes `&mut self`: the
/// single recording window shares one adapter instance across
/// open/device-change/stop calls, so interior mutability is required here
/// where the read-only viewer adapters didn't need it.
pub struct RecordingDeps {
    /// The audio-input capture adapter (`humm_audio::CpalCapture` in
    /// production, `humm_audio::MockCapture` in tests — never the reverse,
    /// since a real `CpalCapture::start` opens an actual input stream and
    /// this crate's kittest coverage must never touch real audio hardware).
    pub capture: Mutex<Box<dyn AudioCapture>>,
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
    /// Stopped, or (nominally) Recording — see the module doc for this
    /// task's scope.
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
    /// Set when the Advanced… button is clicked; Task 12's advanced sheet
    /// consumes and clears it. Harmless while unconsumed (this task never
    /// reads it back).
    pub advanced_requested: bool,
    /// The read-only transcription panel text — empty (placeholder shown)
    /// until Task 11 starts filling it with live utterances.
    pub panel_text: String,
    /// Presentation-bar placeholder visibility (Buzz
    /// `presentation_options_bar` — real controls are a later task; this
    /// task only flips the same show/hide Buzz's own click handler does).
    pub presentation_bar_visible: bool,
    /// Copy-bar placeholder visibility (Buzz `copy_actions_bar`).
    pub copy_bar_visible: bool,
    /// The Record/Stop transition the click just made, for `app.rs`'s
    /// harvest (Task 11's seam — see the module doc).
    pub pending_intent: Option<RecordingIntent>,
    /// Set by the deferred-viewport render closure when the OS window's
    /// close is requested while [`RecordingStatus::Stopped`] (a close while
    /// Recording is deferred this task, spec §2.1 — Task 11 needs the
    /// session-thread-finish gate to close then). The app harvests this each
    /// frame to persist sticky prefs and drop the window.
    pub close_requested: bool,
    /// Set when the window is re-invoked (toolbar/Ctrl+R) while already open —
    /// focus it instead of duplicating (the viewer's own deviation #1 shape).
    pub focus_requested: bool,

    meter_only_handle: Option<Box<dyn CaptureHandle>>,
    meter_only_cancel: Option<CancelToken>,
    meter_only_thread: Option<JoinHandle<()>>,
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
        let options = RecordingOptions::from_settings(&settings.recording, custom_model_available);

        let mut vm = RecordingVm {
            status: RecordingStatus::Stopped,
            devices,
            selected_device,
            options,
            meter_current_amplitude: 0.0,
            meter: Arc::new(MeterSnapshot::new()),
            advanced_requested: false,
            panel_text: String::new(),
            presentation_bar_visible: false,
            copy_bar_visible: false,
            pending_intent: None,
            close_requested: false,
            focus_requested: false,
            meter_only_handle: None,
            meter_only_cancel: None,
            meter_only_thread: None,
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
    /// `:591-621`). Flips the full UI state machine this task (see the
    /// module doc): Stopped→Recording tears down the meter-only capture,
    /// resets the meter (Buzz `audio_meter_widget.reset_amplitude()`, called
    /// *before* the transcriber starts), disables the options/device
    /// controls (via `status`, which the render fn reads), and shows the
    /// presentation-bar placeholder; Recording→Stopped flips back
    /// immediately (no ack-wait — there is no real session yet to wait on)
    /// and restarts the meter-only capture. Either way, latches
    /// [`Self::pending_intent`] for Task 11's harvest.
    pub fn on_record_clicked(&mut self, deps: &RecordingDeps) {
        match self.status {
            RecordingStatus::Stopped => {
                self.stop_meter_only_capture();
                self.meter.reset();
                self.meter_current_amplitude = 0.0;
                self.status = RecordingStatus::Recording;
                self.presentation_bar_visible = true;
                self.copy_bar_visible = false;
                self.pending_intent = Some(RecordingIntent::StartRequested);
            }
            RecordingStatus::Recording => {
                self.status = RecordingStatus::Stopped;
                self.presentation_bar_visible = false;
                self.copy_bar_visible = true;
                self.pending_intent = Some(RecordingIntent::StopRequested);
                self.start_meter_only_capture(deps);
            }
        }
    }

    /// Window close while stopped (Buzz `_do_close`,
    /// `recording_transcriber_widget.py:1202-1260`): tears down the
    /// meter-only capture and writes the sticky options back into
    /// `settings` (the caller persists `settings` to disk — this only
    /// mutates the in-memory struct, matching
    /// [`crate::views::options_sheet::SheetState::write_to`]'s own split of
    /// concerns).
    pub fn close(&mut self, settings: &mut RecordingSettings) {
        self.stop_meter_only_capture();
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
        assert_eq!(options.model_size, ModelSize::Small);
        assert_eq!(options.task, TaskKind::Transcribe);
        assert_eq!(options.language, None);
        assert_eq!(options.initial_prompt, "");
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
        options.model_size = ModelSize::LargeV3Turbo;
        options.task = TaskKind::Translate;
        options.language = Language::parse("fr");
        options.initial_prompt = "proper nouns: Buzz, humm".to_string();

        let mut settings = RecordingSettings::default();
        options.write_to(&mut settings);
        let round_tripped = RecordingOptions::from_settings(&settings, false);

        assert_eq!(round_tripped, options);
    }

    // -- realtime_fit_warning_visible -------------------------------------------

    #[test]
    fn no_measured_entry_hides_the_warning() {
        assert!(!realtime_fit_warning_visible(
            &BTreeMap::new(),
            ModelSize::Small
        ));
    }

    #[test]
    fn a_ratio_at_or_above_one_hides_the_warning() {
        let mut measured = BTreeMap::new();
        measured.insert("small".to_string(), 1.2);
        assert!(!realtime_fit_warning_visible(&measured, ModelSize::Small));

        measured.insert("small".to_string(), 1.0);
        assert!(!realtime_fit_warning_visible(&measured, ModelSize::Small));
    }

    #[test]
    fn a_ratio_below_one_shows_the_warning() {
        let mut measured = BTreeMap::new();
        measured.insert("small".to_string(), 0.8);
        assert!(realtime_fit_warning_visible(&measured, ModelSize::Small));
    }

    #[test]
    fn a_measured_entry_for_a_different_size_does_not_apply() {
        let mut measured = BTreeMap::new();
        measured.insert("base".to_string(), 0.5);
        assert!(!realtime_fit_warning_visible(&measured, ModelSize::Small));
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

    fn mock_deps(devices: Vec<AudioDevice>) -> RecordingDeps {
        RecordingDeps {
            capture: Mutex::new(Box::new(
                humm_audio::MockCapture::new(Vec::new()).with_devices(devices),
            )),
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
    fn second_record_click_flips_recording_back_to_stopped_and_latches_stop_intent() {
        let deps = mock_deps(vec![device("a", "Device A", true)]);
        let mut vm = RecordingVm::open(&deps, &Settings::default());
        vm.on_record_clicked(&deps);

        vm.on_record_clicked(&deps);

        assert_eq!(vm.status, RecordingStatus::Stopped);
        assert_eq!(vm.pending_intent, Some(RecordingIntent::StopRequested));
        assert!(!vm.presentation_bar_visible);
        assert!(vm.copy_bar_visible);
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
