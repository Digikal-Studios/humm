//! kittest coverage for the live-recording window: the chrome renders
//! (`views::recording::recording_ui`, driven directly over a bare `egui::Ui`
//! via `Harness::new_ui_state` — the recording window is a deferred OS
//! viewport an `egui_kittest` `Harness` cannot reach into, the same
//! constraint `kittest_viewer.rs`'s own module doc explains for the
//! transcript viewer) and the Record click's UI state machine (Task 10,
//! F-060/F-004), plus (Task 11, F-060/F-061/F-064/Q-10) a scripted
//! end-to-end session driven through a full `HummApp` (the only way to
//! exercise `app.rs`'s real harvest/drain wiring, since the recording
//! window's own Record/Stop button is unreachable — the e2e test drives
//! `RecordingVm::on_record_clicked` directly instead, exactly what a real
//! click would invoke).
//!
//! Every fixture here uses `humm_audio::MockCapture` — never `CpalCapture` —
//! including the full-`HummApp` toolbar test: `HummApp::new_for_test` always
//! wires a `MockCapture` (see `app.rs`'s `recording_deps` field doc), so no
//! test in this file ever touches real audio hardware. The Task 11 e2e test
//! additionally scripts the model ensurer/engine/upload seams
//! (`humm_gui::live_session`) so it never touches the network or a real
//! whisper model either (Global Constraint 3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui_kittest::kittest::{AccessKitNode, NodeT, Queryable};
use egui_kittest::{Harness, HarnessBuilder};
use humm_audio::MockCapture;
use humm_core::boundary::{AudioCapture, AudioDevice, CaptureHandle, MeterSnapshot};
use humm_core::cancel::CancelToken;
use humm_core::domain::{ModelSpec, Status, TranscriptionId};
use humm_core::error::{AudioError, CoreError, EngineError};
use humm_core::models::download::DownloadProgress;
use humm_core::queue::{ArtifactEnsurer, EngineFactory};
use humm_core::settings::{PresentationSettings, RecordingSettings, Settings};
use humm_gui::app::HummApp;
use humm_gui::live_session::{
    LiveEngineFactory, LiveEngineSession, LiveStore, LiveUpload, UploadFactory,
};
use humm_gui::recording::{RecordingDeps, RecordingStatus, RecordingVm};
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_gui::viewer::can_open;
use humm_gui::viewmodel::TasksVm;
use humm_gui::views::recording::{CopyLabelState, recording_ui};
use humm_gui::views::recording_advanced::AdvancedSheetState;
use humm_store::SqliteStore;
use humm_whisper::LiveOptions;

/// A fake [`EngineFactory`] for tests that never actually run a job: nothing
/// in this file enqueues a transcription task (via the task *queue* — the
/// Task 11 e2e test's live session is a separate path, scripted via
/// `humm_gui::live_session`'s own seams below).
struct NeverCalledEngineFactory;

impl EngineFactory for NeverCalledEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn humm_core::boundary::TranscriptionEngine>, CoreError> {
        unreachable!("no task is ever enqueued in this file's tests")
    }
}

/// A [`LiveStore`]/[`LiveEngineFactory`]/[`UploadFactory`] stand-in for the
/// Task 10-only fixtures in this file, which never drive a real session
/// (`fixture_with_devices`'s `RecordingVm` is never harvested by a real
/// `HummApp` — nothing here calls these).
struct NeverCalledLiveStore;
impl LiveStore for NeverCalledLiveStore {
    fn insert_live_session(
        &self,
        _name: &str,
        _options: &humm_core::domain::TranscriptionOptions,
    ) -> Result<humm_core::domain::TranscriptionId, humm_core::error::StoreError> {
        unreachable!("Task 10 fixtures never start a real session")
    }
    fn append_live_segment(
        &self,
        _id: humm_core::domain::TranscriptionId,
        _start: humm_core::domain::Millis,
        _end: humm_core::domain::Millis,
        _text: &str,
    ) -> Result<(), humm_core::error::StoreError> {
        unreachable!("Task 10 fixtures never start a real session")
    }
    fn complete_live_session(
        &self,
        _id: humm_core::domain::TranscriptionId,
    ) -> Result<(), humm_core::error::StoreError> {
        unreachable!("Task 10 fixtures never start a real session")
    }
}

struct NeverCalledEnsurer;
impl ArtifactEnsurer for NeverCalledEnsurer {
    fn ensure_model(
        &self,
        _model: &ModelSpec,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError> {
        unreachable!("Task 10 fixtures never start a real session")
    }
    fn ensure_vad(
        &self,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Option<PathBuf> {
        unreachable!("Task 10 fixtures never start a real session")
    }
}

struct NeverCalledLiveEngineFactory;
impl LiveEngineFactory for NeverCalledLiveEngineFactory {
    fn load(
        &self,
        _model_path: &Path,
        _use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        unreachable!("Task 10 fixtures never start a real session")
    }
}

struct NeverCalledUploadFactory;
impl UploadFactory for NeverCalledUploadFactory {
    fn build(&self, _url: String) -> Box<dyn LiveUpload> {
        unreachable!("Task 10 fixtures never start a real session")
    }
}

/// Two scripted mock input devices — a default and a non-default — so
/// device-selection/enablement assertions have more than one candidate to
/// distinguish.
fn two_mock_devices() -> Vec<AudioDevice> {
    vec![
        AudioDevice {
            id: "dev-a".to_string(),
            name: "Dev A".to_string(),
            is_default: true,
        },
        AudioDevice {
            id: "dev-b".to_string(),
            name: "Dev B".to_string(),
            is_default: false,
        },
    ]
}

/// The recording-window render fixture: a `MockCapture`-backed `RecordingVm`
/// plus everything `recording_ui` needs, including a real (temp-file)
/// `Settings`/`settings_path` pair for the advanced sheet's and presentation
/// bar's immediate-save fields (Task 12) — mirrors `kittest_resizer.rs`'s own
/// `Mutex<Settings>` + temp `settings_path` fixture shape.
struct Fixture {
    vm: RecordingVm,
    deps: RecordingDeps,
    custom_model_available: bool,
    measured_speed: BTreeMap<String, f32>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    _settings_dir: tempfile::TempDir,
}

fn fixture_with_devices(devices: Vec<AudioDevice>) -> Fixture {
    let deps = RecordingDeps {
        capture: Mutex::new(Box::new(MockCapture::new(Vec::new()).with_devices(devices))),
        store: Arc::new(NeverCalledLiveStore),
        model_ensurer: Arc::new(NeverCalledEnsurer),
        engine_factory: Arc::new(NeverCalledLiveEngineFactory),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
    };
    let settings = Settings::default();
    let vm = RecordingVm::open(&deps, &settings);
    let settings_dir = tempfile::tempdir().expect("tempdir");
    let settings_path = settings_dir.path().join("settings.toml");
    Fixture {
        vm,
        deps,
        custom_model_available: false,
        measured_speed: BTreeMap::new(),
        settings: Mutex::new(settings),
        settings_path,
        _settings_dir: settings_dir,
    }
}

fn fixture() -> Fixture {
    fixture_with_devices(two_mock_devices())
}

fn build_harness(fixture: Fixture) -> Harness<'static, Fixture> {
    Harness::new_ui_state(
        |ui, fixture: &mut Fixture| {
            recording_ui(
                ui,
                &mut fixture.vm,
                &fixture.deps,
                fixture.custom_model_available,
                &fixture.measured_speed,
                &fixture.settings,
                &fixture.settings_path,
            );
        },
        fixture,
    )
}

// -- Window renders ----------------------------------------------------------

#[test]
fn window_renders_with_the_click_record_placeholder() {
    let mut harness = build_harness(fixture());
    harness.step();

    // The transcription panel's hint text (`TextEdit::hint_text`) is exposed
    // via AccessKit's `Placeholder` property, not `label`/`value` — see
    // `accesskit_consumer::Node::placeholder`. `get_by` panics with a clear
    // message if nothing matches, which is exactly the assertion wanted here.
    harness
        .get_by(|node: &AccessKitNode<'_>| node.placeholder() == Some("Click Record to begin..."));
}

#[test]
fn window_renders_the_record_button_and_microphone_label() {
    let mut harness = build_harness(fixture());
    harness.step();

    harness.get_by_label("Record");
    harness.get_by_label("Microphone:");
    harness.get_by_label("Advanced...");
}

// -- Record click state machine ----------------------------------------------

#[test]
fn record_click_disables_options_and_shows_the_presentation_placeholder() {
    let mut harness = build_harness(fixture());
    harness.step();

    assert!(
        !harness
            .get_by_label("Advanced...")
            .accesskit_node()
            .is_disabled(),
        "the options group starts enabled while stopped"
    );
    assert_eq!(harness.state().vm.status, RecordingStatus::Stopped);

    harness.get_by_label("Record").click();
    harness.step();
    // The click's release event is processed on this frame's own
    // `recording_ui` call, which reads `vm.status` for the options group's
    // enablement *before* the (later-in-the-same-call) meter/record row
    // detects the click and mutates it — ordinary immediate-mode one-frame
    // lag, not a bug (a real user would never perceive it at frame rate).
    // One more settle frame renders fully consistent with the new status.
    harness.step();

    assert_eq!(harness.state().vm.status, RecordingStatus::Recording);
    assert!(
        harness
            .get_by_label("Advanced...")
            .accesskit_node()
            .is_disabled(),
        "the options group (and mic row, same enablement gate) disables while recording"
    );
    assert!(
        harness.state().vm.presentation_bar_visible,
        "the presentation-bar placeholder shows once recording starts"
    );
    assert!(!harness.state().vm.copy_bar_visible);
    harness.get_by_label("Stop");
}

#[test]
fn stop_click_moves_to_stopping_and_disables_the_button_pending_the_session_ack() {
    // Two-phase stop (spec §2.1, Task 11): a bare `recording_ui` fixture
    // (no real session/`app.rs` harvest behind it) can only exercise the
    // *click*'s own effect — moving to `Stopping` and disabling the button
    // — not the eventual restore, which `finish_stopping` performs once a
    // real session's `Stopped` message arrives (see the sibling test for
    // that half).
    let mut harness = build_harness(fixture());
    harness.step();
    harness.get_by_label("Record").click();
    harness.step();
    harness.step(); // settle frame — see the sibling test's comment.
    assert_eq!(harness.state().vm.status, RecordingStatus::Recording);

    harness.get_by_label("Stop").click();
    harness.step();
    harness.step(); // settle frame.

    assert_eq!(harness.state().vm.status, RecordingStatus::Stopping);
    // Options/device/mic row stay disabled (status != Stopped).
    assert!(
        harness
            .get_by_label("Advanced...")
            .accesskit_node()
            .is_disabled()
    );
    // Bars are unchanged from the Recording state — still mid-stop.
    assert!(harness.state().vm.presentation_bar_visible);
    assert!(!harness.state().vm.copy_bar_visible);
    let stop_button = harness.get_by_label("Stop");
    assert!(
        stop_button.accesskit_node().is_disabled(),
        "Buzz disables the button until the session thread's ack (`stop_recording`, :1103-1111)"
    );
}

#[test]
fn finish_stopping_restores_the_stopped_ui_once_the_session_acks() {
    let mut fixture = fixture();
    fixture.vm.on_record_clicked(&fixture.deps); // -> Recording
    fixture.vm.on_record_clicked(&fixture.deps); // -> Stopping
    assert_eq!(fixture.vm.status, RecordingStatus::Stopping);

    // Simulates `app.rs::finish_live_session`'s non-`pending_close` arm —
    // this fixture has no real session thread to send the `Stopped`
    // message, so the ack is invoked directly.
    let mut presentation_settings = PresentationSettings::default();
    let _flushed = fixture
        .vm
        .finish_stopping(&fixture.deps, &mut presentation_settings);
    let mut harness = build_harness(fixture);
    harness.step();

    assert_eq!(harness.state().vm.status, RecordingStatus::Stopped);
    assert!(
        !harness
            .get_by_label("Advanced...")
            .accesskit_node()
            .is_disabled(),
        "the options group re-enables once stopped"
    );
    assert!(!harness.state().vm.presentation_bar_visible);
    assert!(harness.state().vm.copy_bar_visible);
    harness.get_by_label("Record");
}

#[test]
fn empty_device_list_leaves_the_record_button_clickable() {
    let mut harness = build_harness(fixture_with_devices(Vec::new()));
    harness.step();

    assert_eq!(harness.state().vm.selected_device, None);
    let record = harness.get_by_label("Record");
    assert!(
        !record.accesskit_node().is_disabled(),
        "Buzz never gates the Record button on device presence"
    );
    record.click();
    harness.step();

    assert_eq!(harness.state().vm.status, RecordingStatus::Recording);
}

// -- Meter painting -----------------------------------------------------------

#[test]
fn meter_paints_several_frames_without_panicking() {
    let mut harness = build_harness(fixture());
    for _ in 0..5 {
        harness.step();
    }
    // Still there after repeated frames — proves nothing panicked.
    harness.get_by_label("Record");
}

// -- Realtime-fit warning ------------------------------------------------------

#[test]
fn realtime_fit_warning_renders_when_the_measured_speed_is_below_one() {
    let mut fixture = fixture();
    fixture.measured_speed.insert("small".to_string(), 0.7);
    let mut harness = build_harness(fixture);
    harness.step();

    harness.get_by_label_contains("0.7");
}

#[test]
fn no_realtime_fit_warning_renders_with_no_measured_entry() {
    let mut harness = build_harness(fixture());
    harness.step();

    let matches = harness
        .query_all_by(|node: &AccessKitNode<'_>| {
            node.value().is_some_and(|value| value.contains("realtime"))
        })
        .count();
    assert_eq!(matches, 0, "no measured entry means no warning label");
}

// -- Task 12: advanced sheet ---------------------------------------------------

#[test]
fn advanced_button_click_opens_the_sheet() {
    let mut harness = build_harness(fixture());
    harness.step();

    harness.get_by_label("Advanced...").click();
    harness.step();
    harness.step(); // settle frame -- same one-frame click/render-order lag
    // the Record-click tests document (`recording.rs`'s own click handler
    // runs before this frame's later `recording_advanced::ui` call sees the
    // flag it just set; one more frame renders fully consistent).

    harness.get_by_label("Advanced Settings");
    harness.get_by_label("Speech recognition settings");
    harness.get_by_label("Translation settings");
    harness.get_by_label("Recording settings");
    harness.get_by_label("Initial Prompt:");
    harness.get_by_label("Silence threshold:");
    harness.get_by_label("Live recording mode:");
    harness.get_by_label("Ok");
}

#[test]
fn disabled_llm_block_renders_with_its_tooltip_target_present() {
    let mut harness = build_harness(fixture());
    harness.get_by_label("Advanced...").click();
    harness.step();
    harness.step();

    let checkbox = harness.get_by_label("Enable AI translation");
    assert!(
        checkbox.accesskit_node().is_disabled(),
        "F-063 is M6 -- the LLM-translation checkbox renders permanently disabled"
    );
    harness.get_by_label("AI model:");
    harness.get_by_label("Instructions for AI:");
}

#[test]
fn advanced_sheet_mode_dependent_rows_swap_with_the_mode() {
    // Opened directly with a known `AdvancedSheetState` (mode 0, Append
    // below) rather than via a click+combo-select round trip -- driving an
    // `egui::ComboBox` selection has no kittest precedent in this workspace;
    // `recording_advanced::is_append_and_correct`'s own unit tests already
    // pin the pure index->visibility mapping this render reacts to, so this
    // test only needs to prove the *render* reacts to `mode_index` changing.
    let mut fixture = fixture();
    fixture.vm.advanced_sheet = Some(AdvancedSheetState::from_settings(
        &RecordingSettings::default(),
        &fixture.vm.options.line_separator,
    ));
    let mut harness = build_harness(fixture);
    harness.step();

    harness.get_by_label("Line separator:");
    assert!(harness.query_by_label("Transcription step:").is_none());
    assert!(harness.query_by_label("Hide unconfirmed").is_none());

    harness
        .state_mut()
        .vm
        .advanced_sheet
        .as_mut()
        .expect("sheet is open")
        .mode_index = 2; // Append and correct
    harness.step();

    harness.get_by_label("Transcription step:");
    harness.get_by_label("Hide unconfirmed");
    assert!(harness.query_by_label("Line separator:").is_none());
}

#[test]
fn export_block_gates_on_the_enable_checkbox() {
    let mut harness = build_harness(fixture());
    harness.get_by_label("Advanced...").click();
    harness.step();
    harness.step();

    let browse = harness.get_by_label("Browse");
    assert!(
        browse.accesskit_node().is_disabled(),
        "export fields start disabled -- export_enabled defaults to false"
    );

    harness.get_by_label("Enable live recording export").click();
    harness.step();

    let browse = harness.get_by_label("Browse");
    assert!(
        !browse.accesskit_node().is_disabled(),
        "checking Enable live recording export re-enables the export block"
    );
    // Immediate-save contract (Buzz `on_export_enabled_changed`): the change
    // has already reached `settings.recording`, not just the sheet's own
    // mirror.
    assert!(
        harness
            .state()
            .settings
            .lock()
            .unwrap()
            .recording
            .export_enabled
    );
}

// -- Task 12: presentation bar --------------------------------------------------

#[test]
fn presentation_bar_show_button_opens_the_presentation_window_flag() {
    let mut fixture = fixture();
    fixture.vm.on_record_clicked(&fixture.deps); // -> Recording, presentation bar visible
    let mut harness = build_harness(fixture);
    harness.step();

    assert!(!harness.state().vm.presentation.open);
    harness.get_by_label("Show").click();
    harness.step();

    assert!(harness.state().vm.presentation.open);
}

#[test]
fn presentation_bar_custom_theme_reveals_color_buttons() {
    let mut fixture = fixture();
    fixture.vm.on_record_clicked(&fixture.deps);
    fixture.vm.presentation.theme = humm_gui::views::presentation::PresentationTheme::Custom;
    let mut harness = build_harness(fixture);
    harness.step();

    harness.get_by_label("Text Color");
    harness.get_by_label("Background Color");
}

#[test]
fn presentation_bar_light_theme_hides_color_buttons() {
    let mut fixture = fixture();
    fixture.vm.on_record_clicked(&fixture.deps);
    let mut harness = build_harness(fixture);
    harness.step();

    assert!(harness.query_by_label("Text Color").is_none());
    assert!(harness.query_by_label("Background Color").is_none());
}

// -- Task 12: copy bar ---------------------------------------------------------

#[test]
fn copy_click_relabels_to_copied_for_a_non_empty_panel() {
    let mut fixture = fixture();
    fixture.vm.copy_bar_visible = true;
    fixture.vm.panel_text = "hello world".to_string();
    let mut harness = build_harness(fixture);
    harness.step();

    harness.get_by_label("Copy").click();
    harness.step();
    harness.step(); // settle frame -- same click/render-order lag as the Record-click tests.

    harness.get_by_label("Copied!");
    assert!(matches!(
        harness.state().vm.copy_label,
        CopyLabelState::Copied(_)
    ));
}

#[test]
fn copy_click_relabels_to_nothing_to_copy_for_an_empty_panel() {
    let mut fixture = fixture();
    fixture.vm.copy_bar_visible = true;
    fixture.vm.panel_text = "   ".to_string();
    let mut harness = build_harness(fixture);
    harness.step();

    harness.get_by_label("Copy").click();
    harness.step();
    harness.step(); // settle frame -- same click/render-order lag as the Record-click tests.

    harness.get_by_label("Nothing to copy!");
    assert!(matches!(
        harness.state().vm.copy_label,
        CopyLabelState::NothingToCopy(_)
    ));
}

// -- Toolbar Record button (F-004/F-060 unlock) -------------------------------

fn test_services(store: Arc<SqliteStore>, dir: &Path) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: dir.join("media"),
        model_root: dir.join("models"),
        settings: Settings::default(),
        settings_path: dir.join("settings.toml"),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
    })
}

#[test]
fn toolbar_record_is_unconditionally_enabled_and_its_click_opens_the_recording_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let services = test_services(store, dir.path());
    let mut harness = HarnessBuilder::default()
        .with_size(egui::Vec2::new(1000.0, 600.0))
        .build_eframe(move |_cc| HummApp::new_for_test(services));
    harness.step();

    let record = harness.get_by_label("Record");
    assert!(
        !record.accesskit_node().is_disabled(),
        "the toolbar Record button must be unconditionally enabled (F-004/F-060)"
    );
    assert!(
        !harness.state().is_recording_window_open_for_test(),
        "the window is not open before the click"
    );

    record.click();
    harness.step();

    assert!(
        harness.state().is_recording_window_open_for_test(),
        "clicking the toolbar Record button opens the live-recording window"
    );
}

// -- Task 11: scripted end-to-end live session (F-060/F-061/F-064, Q-10) -----
//
// Record -> 2 scripted utterances -> Stop, driven through a full `HummApp`
// (the real `harvest_recording_intent`/`drain_live_session`/
// `finish_live_session` wiring). Every adapter is scripted/network-free:
// - `store`: a REAL `SqliteStore` at a temp-file path (not `:memory:` — the
//   restart-survival assertion reopens the same file).
// - `model_ensurer`/`engine_factory`: instant, no network, no real ggml file
//   ever opened (`ScriptedEngine` never touches the resolved path).
// - `capture`: `MockCapture`, shared with the scripted engine so the engine
//   can push the second batch mid-call — mirrors `humm-core`'s own
//   `live::session` test pattern ("the engine closure owns the producer").
// - `upload_factory`: records calls instead of ever binding a socket.

/// Wraps a [`MockCapture`] behind a shared handle so both `RecordingDeps.capture`
/// (via this thin delegate) and the scripted engine (to push the second
/// batch mid-call) can drive the same instance — no real-time pacing, no
/// polling: the session thread's own `drain_ring`/engine-call sequencing is
/// the only synchronization needed (see `ScriptedEngine::transcribe_live`).
#[derive(Clone)]
struct SharedMockCapture(Arc<Mutex<MockCapture>>);

impl AudioCapture for SharedMockCapture {
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).devices()
    }

    fn start(
        &mut self,
        device_id: Option<&str>,
        sink: rtrb::Producer<f32>,
        meter: Arc<MeterSnapshot>,
    ) -> Result<Box<dyn CaptureHandle>, AudioError> {
        let mut inner = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let handle = inner.start(device_id, sink, meter)?;
        // Batch 1: 80_000 uniform-loud samples (5s @ 16kHz, AppendBelow's
        // fixed `n_batch_samples`; uniform amplitude avoids a
        // silence-driven early cut — `humm-core`'s own `live::session`
        // tests use the identical fixture for the identical reason).
        // Pushed here, synchronously, before `run_session`'s loop ever
        // starts, so its very first `drain_ring` call already sees the
        // whole batch — no idle-pause is ever reached for batch 1.
        inner.pump(BATCH_SAMPLES);
        Ok(handle)
    }
}

const BATCH_SAMPLES: usize = 80_000;

struct ScriptedEngineFactory {
    capture: Arc<Mutex<MockCapture>>,
    calls: Arc<AtomicUsize>,
}

impl LiveEngineFactory for ScriptedEngineFactory {
    fn load(
        &self,
        _model_path: &Path,
        _use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        Ok(Box::new(ScriptedEngine {
            capture: Arc::clone(&self.capture),
            calls: Arc::clone(&self.calls),
        }))
    }
}

/// Returns "utterance one" then "utterance two"; on its *first* call, pushes
/// the second batch into the shared `MockCapture` — by the time
/// `run_session`'s loop reaches its next `drain_ring`, the ring (already
/// drained empty by the time this call started) has room for the whole
/// batch again, so this never overflows regardless of scheduling.
struct ScriptedEngine {
    capture: Arc<Mutex<MockCapture>>,
    calls: Arc<AtomicUsize>,
}

impl LiveEngineSession for ScriptedEngine {
    fn transcribe_live(
        &mut self,
        _samples: &[f32],
        _opts: &LiveOptions,
        _prompt: &str,
        _cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            self.capture
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .pump(BATCH_SAMPLES);
            Ok("utterance one".to_string())
        } else {
            Ok("utterance two".to_string())
        }
    }
}

struct PanickingEngineFactory {
    capture: Arc<Mutex<MockCapture>>,
    calls: Arc<AtomicUsize>,
}

impl LiveEngineFactory for PanickingEngineFactory {
    fn load(
        &self,
        _model_path: &Path,
        _use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        Ok(Box::new(PanickingEngine {
            capture: Arc::clone(&self.capture),
            calls: Arc::clone(&self.calls),
        }))
    }
}

/// Returns "utterance one" on its first call (pushing the second batch into
/// the shared `MockCapture`, same pattern as [`ScriptedEngine`]), then
/// panics on its second call — simulating a session-thread crash for
/// `HummApp::recover_crashed_session`'s own kittest coverage (Task 11
/// review finding 2).
struct PanickingEngine {
    capture: Arc<Mutex<MockCapture>>,
    calls: Arc<AtomicUsize>,
}

impl LiveEngineSession for PanickingEngine {
    fn transcribe_live(
        &mut self,
        _samples: &[f32],
        _opts: &LiveOptions,
        _prompt: &str,
        _cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            self.capture
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .pump(BATCH_SAMPLES);
            Ok("utterance one".to_string())
        } else {
            panic!("scripted engine crash — Task 11 review finding 2's own kittest coverage");
        }
    }
}

/// Resolves instantly to a scripted path — never a real ggml file, and
/// never opened for real (`ScriptedEngine` ignores it entirely).
struct InstantEnsurer {
    model_path: PathBuf,
}

impl ArtifactEnsurer for InstantEnsurer {
    fn ensure_model(
        &self,
        _model: &ModelSpec,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError> {
        Ok(self.model_path.clone())
    }

    fn ensure_vad(
        &self,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Option<PathBuf> {
        None
    }
}

/// One recorded [`RecordingUpload`] call — the network-free stand-in for a
/// real `TestServer` (brief: "do NOT bind TCP in kittest").
#[derive(Debug, Clone, PartialEq)]
enum UploadCall {
    Clear,
    Transcript { text: String, replace: bool },
}

struct RecordingUpload {
    calls: Arc<Mutex<Vec<UploadCall>>>,
}

impl LiveUpload for RecordingUpload {
    fn clear(&self) {
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(UploadCall::Clear);
    }

    fn transcript(&self, text: &str, replace: bool) {
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(UploadCall::Transcript {
                text: text.to_string(),
                replace,
            });
    }
}

struct RecordingUploadFactory {
    calls: Arc<Mutex<Vec<UploadCall>>>,
}

impl UploadFactory for RecordingUploadFactory {
    fn build(&self, _url: String) -> Box<dyn LiveUpload> {
        Box::new(RecordingUpload {
            calls: Arc::clone(&self.calls),
        })
    }
}

/// Polls `condition` every 5 ms (the established deadline-loop idiom this
/// workspace's other kittest files use for real background-thread work —
/// `kittest_actions.rs`/`kittest_phases.rs`/`acceptance.rs`), stepping
/// `harness` each time so the app's own per-frame harvest/drain runs.
/// Panics with `message` if `condition` hasn't been met within 10 s.
fn wait_for(
    harness: &mut Harness<'_, HummApp>,
    message: &str,
    mut condition: impl FnMut(&HummApp) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.step();
        if condition(harness.state()) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out: {message}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn scripted_end_to_end_record_two_utterances_then_stop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("humm.sqlite3");
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));
    let export_dir = dir.path().join("export");
    std::fs::create_dir_all(&export_dir).expect("mkdir export");
    let model_path = dir.path().join("fake-model.bin");
    std::fs::write(&model_path, b"never opened by the scripted engine").expect("write");

    let mut settings = Settings::default();
    settings.recording.export_enabled = true;
    settings.recording.export_folder = Some(export_dir.clone());
    settings.recording.upload_url = Some("http://upload.invalid/ingest".to_string());
    // `settings.recording.mode` defaults to 0 (AppendBelow) — Buzz parity,
    // and the mode this test's separator/export-format assertions target.

    let services = services::init_with(TestDeps {
        store: Arc::clone(&store),
        media_dir: dir.path().join("media"),
        model_root: dir.path().join("models"),
        settings,
        settings_path: dir.path().join("settings.toml"),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
    });

    // Uniform-loud script covering both batches (0.5 amplitude — well above
    // the default `silence_threshold` of 0.0025, and uniform so
    // `find_silence_cut_point` never cuts short, same fixture `humm-core`'s
    // own `live::session` tests use).
    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));
    let upload_calls: Arc<Mutex<Vec<UploadCall>>> = Arc::new(Mutex::new(Vec::new()));

    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(ScriptedEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(RecordingUploadFactory {
            calls: Arc::clone(&upload_calls),
        }),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
    };

    let mut harness = HarnessBuilder::default()
        .with_size(egui::Vec2::new(1000.0, 600.0))
        .build_eframe(move |_cc| {
            let mut app = HummApp::new_for_test(services);
            app.set_recording_deps_for_test(scripted_deps);
            app
        });
    harness.step();

    harness.state_mut().open_recording_window_for_test();
    harness.step();
    let vm_arc = harness
        .state()
        .recording_vm_for_test()
        .expect("window is open");
    let deps_arc = harness.state().recording_deps_for_test();

    // Record click (`RecordingVm::on_record_clicked` is exactly what a real
    // click on the window's own button invokes).
    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();

    // Poll until the session row exists (spec §4: inserted before the
    // first chunk), learning its id from a store read, then assert the
    // *running app's own table* — not a fresh `TasksVm::load` — already
    // shows it (review finding 3: this is what actually exercises
    // `SessionMessage::TableRefresh`'s wiring end to end, via
    // `HummApp::tasks_row_for_test`).
    let mut live_row_id: Option<TranscriptionId> = None;
    wait_for(&mut harness, "the live session row to appear", |_app| {
        let Some(row) = TasksVm::load(&store).sorted_rows().first().cloned() else {
            return false;
        };
        live_row_id = Some(row.id);
        true
    });
    let live_row_id = live_row_id.expect("the wait_for deadline-loop only exits on success");
    let app_row_at_start = harness
        .state()
        .tasks_row_for_test(live_row_id)
        .expect("the running app's own table shows the live row at session start");
    assert_eq!(
        app_row_at_start.status,
        Status::InProgress,
        "TableRefresh at insert must reach the running app's table, not just the store"
    );

    wait_for(&mut harness, "the second utterance to land on the panel", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .panel_text
                .contains("utterance two")
        }
    });

    assert_eq!(
        vm_arc.lock().unwrap_or_else(|p| p.into_inner()).panel_text,
        "utterance one\n\nutterance two",
        "append-below, default separator"
    );

    // Stop click.
    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    wait_for(&mut harness, "the session to stop", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc.lock().unwrap_or_else(|p| p.into_inner()).status == RecordingStatus::Stopped
        }
    });
    assert!(
        harness.state().is_recording_window_open_for_test(),
        "the window stays open once Stopped — it only closes on a real close request"
    );

    // Review finding 3, other half: the running app's own table must also
    // have flipped to Completed via the *second* `TableRefresh` (sent after
    // `complete_live_session`), not just the store underneath it.
    let app_row_at_stop = harness
        .state()
        .tasks_row_for_test(live_row_id)
        .expect("the running app's own table still shows the live row after Stop");
    assert_eq!(
        app_row_at_stop.status,
        Status::Completed,
        "TableRefresh at completion must reach the running app's table, not just the store"
    );

    // -- Store: InProgress -> Completed, visible in `TasksVm`, `can_open`
    //    admits it, 2 ordered non-overlapping raw-text segments.
    let tasks = TasksVm::load(&store);
    let row = tasks
        .sorted_rows()
        .first()
        .expect("exactly one live session row");
    assert_eq!(row.id, live_row_id, "the same row throughout the session");
    assert_eq!(row.status, Status::Completed);
    assert!(
        can_open(row.status),
        "a completed live row must open in the viewer"
    );
    assert!(
        row.display_name.starts_with("Live recording <"),
        "row name: {}",
        row.display_name
    );

    let segments = store.get_segment_rows(row.id).expect("segments");
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].text, "utterance one");
    assert_eq!(segments[1].text, "utterance two");
    assert!(
        segments[0].start <= segments[0].end && segments[0].end <= segments[1].start,
        "segments must be ordered and non-overlapping: {segments:?}"
    );

    // -- TXT export file, exact bytes (append-below: "text" + separator per
    //    utterance, no rewrite pass since `export_max_entries` is 0).
    let export_files: Vec<_> = std::fs::read_dir(&export_dir)
        .expect("read export dir")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(export_files.len(), 1, "exactly one export file");
    let export_content = std::fs::read_to_string(export_files[0].path()).expect("read export");
    assert_eq!(export_content, "utterance one\n\nutterance two\n\n");

    // -- Upload: clear at start, then one "append" transcript call per
    //    utterance (append-below is never "replace" — that's
    //    append-and-correct only).
    let uploads = upload_calls
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    assert_eq!(
        uploads,
        vec![
            UploadCall::Clear,
            UploadCall::Transcript {
                text: "utterance one".to_string(),
                replace: false,
            },
            UploadCall::Transcript {
                text: "utterance two".to_string(),
                replace: false,
            },
        ]
    );

    // -- Restart survival (acceptance #4's automated half): drop and reopen
    //    the same on-disk store — the row and its segments survive intact.
    drop(store);
    let reopened = SqliteStore::open(&db_path).expect("reopen store");
    let reopened_row = reopened
        .get_task(&row.id)
        .expect("get_task")
        .expect("row survives a restart");
    assert_eq!(reopened_row.status, Status::Completed);
    let reopened_segments = reopened
        .get_segment_rows(row.id)
        .expect("segments survive a restart");
    assert_eq!(reopened_segments.len(), 2);
    assert_eq!(reopened_segments[0].text, "utterance one");
    assert_eq!(reopened_segments[1].text, "utterance two");
}

// -- Task 11 review finding 2: session-thread panic recovery -----------------

#[test]
fn session_thread_panic_recovers_the_window_and_completes_the_store_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("humm.sqlite3");
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));
    let model_path = dir.path().join("fake-model.bin");
    std::fs::write(&model_path, b"never opened by the scripted engine").expect("write");

    let services = services::init_with(TestDeps {
        store: Arc::clone(&store),
        media_dir: dir.path().join("media"),
        model_root: dir.path().join("models"),
        settings: Settings::default(),
        settings_path: dir.path().join("settings.toml"),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
    });

    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));

    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(PanickingEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
    };

    let mut harness = HarnessBuilder::default()
        .with_size(egui::Vec2::new(1000.0, 600.0))
        .build_eframe(move |_cc| {
            let mut app = HummApp::new_for_test(services);
            app.set_recording_deps_for_test(scripted_deps);
            app
        });
    harness.step();

    harness.state_mut().open_recording_window_for_test();
    harness.step();
    let vm_arc = harness
        .state()
        .recording_vm_for_test()
        .expect("window is open");
    let deps_arc = harness.state().recording_deps_for_test();

    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();

    // The scripted engine's second call panics — the session thread dies
    // without ever sending `Stopped`, breaking `run_live_session`'s own
    // "always emits Stopped last" contract. Poll (bounded, no sleeping
    // beyond the established deadline-loop idiom — `wait_for`) until
    // `HummApp::recover_crashed_session` observes the dead `JoinHandle` and
    // recovers on its own.
    wait_for(&mut harness, "crash recovery to restore the stopped UI", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
            vm.status == RecordingStatus::Stopped
        }
    });

    {
        let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
        // "Window closable": `show_recording_viewport`'s own close-request
        // handling (Task 10/11, tested separately) vetoes a close only
        // while `status != Stopped` — proving the crash recovered `status`
        // all the way back to `Stopped` (with no lingering close-request
        // state) *is* the proof the veto has lifted; the deferred viewport
        // itself is unreachable from kittest (this file's own module doc),
        // so this is the correct-altitude assertion, matching every other
        // close-related assertion already in this file. (`RecordingVm::session`
        // — also cleared by recovery — is `pub(crate)`, not visible from
        // this external test crate; `status == Stopped` is the externally
        // observable proof.)
        assert!(
            !vm.pending_close,
            "no close was ever requested in this test"
        );
        assert!(
            vm.error_dialog
                .as_deref()
                .is_some_and(|text| !text.is_empty()),
            "a crash must surface a non-empty error dialog, got {:?}",
            vm.error_dialog
        );
    }

    // -- Store: the crash-recovery fallback still marks the row Completed
    //    (Q-10 — a partial transcript is finished work worth opening), with
    //    only the one utterance that landed before the crash.
    let tasks = TasksVm::load(&store);
    let row = tasks
        .sorted_rows()
        .first()
        .expect("exactly one live session row");
    assert_eq!(row.status, Status::Completed);
    assert!(can_open(row.status));

    let segments = store.get_segment_rows(row.id).expect("segments");
    assert_eq!(segments.len(), 1, "only the pre-crash utterance landed");
    assert_eq!(segments[0].text, "utterance one");
}
