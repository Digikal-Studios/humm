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
use humm_core::domain::{Backend, ModelSpec, Status, TaskKind, TranscriptionId};
use humm_core::error::{AudioError, CoreError, EngineError};
use humm_core::language::Language;
use humm_core::models::download::DownloadProgress;
use humm_core::queue::{ArtifactEnsurer, EngineFactory};
use humm_core::secrets::{MemorySecretStore, SecretKey, SecretStore};
use humm_core::settings::{PresentationSettings, RecordingSettings, Settings};
use humm_gui::app::HummApp;
use humm_gui::live_session::{
    LiveEngineFactory, LiveEngineSession, LiveStore, LiveUpload, UploadFactory,
};
use humm_gui::recording::{RecordingDeps, RecordingStatus, RecordingVm};
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_gui::translate_seam::{
    DeferredScriptedTranslateSpawner, ScriptedTranslateSpawner, TranslateSpawner,
};
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
    ) -> Result<i64, humm_core::error::StoreError> {
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
    include_lumii: bool,
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
        secrets: Arc::new(MemorySecretStore::new()),
    };
    let settings = Settings::default();
    let vm = RecordingVm::open(&deps, &settings);
    let settings_dir = tempfile::tempdir().expect("tempdir");
    let settings_path = settings_dir.path().join("settings.toml");
    Fixture {
        vm,
        deps,
        custom_model_available: false,
        include_lumii: false,
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
                fixture.include_lumii,
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

// -- Task 18: backend selector + Api Key row (F-061b) ------------------------

/// Selecting the API backend hides the model-size combo (+ its Apple-Silicon
/// load-note icon) and shows the Api Key row; switching back to Whisper
/// restores it — the recording window's own instance of
/// `kittest_import.rs::api_backend_selection_hides_size_combo_and_shows_api_key_row`'s
/// identical assertion for the file-options sheet. Driven directly via
/// `fixture.vm.options.backend` (this workspace has no kittest precedent for
/// driving an `egui::ComboBox` selection by click, per that test's own doc).
#[test]
fn api_backend_selection_hides_size_combo_and_shows_api_key_row() {
    let mut harness = build_harness(fixture());
    harness.step();

    // The backend combo itself always renders, regardless of which backend
    // is currently selected (unlike the size combo/Api Key row below, which
    // are conditional).
    harness.get_by_label("Backend:");

    // Whisper is the default backend: the size combo shows, the Api Key row
    // doesn't.
    harness.get_by_label("Model:");
    assert!(harness.query_by_label("Api Key:").is_none());

    harness.state_mut().vm.options.backend = Backend::OpenAiApi;
    harness.step();

    assert!(
        harness.query_by_label("Model:").is_none(),
        "the size combo must hide once the API backend is selected"
    );
    harness.get_by_label("Api Key:");

    harness.state_mut().vm.options.backend = Backend::Whisper;
    harness.step();

    harness.get_by_label("Model:");
    assert!(harness.query_by_label("Api Key:").is_none());
}

/// The backend combo is part of the window-close save group, exactly like
/// `model_size`/`task`/`language` (Buzz `_do_close:1231-1233` — no
/// immediate-save handler exists for the model-*type* combo, the task
/// report's own finding): a selection made in the window survives
/// `RecordingVm::close` -> `RecordingOptions::write_to` -> reload via
/// `RecordingOptions::from_settings`.
#[test]
fn backend_selection_persists_through_window_close() {
    let mut fixture = fixture();
    fixture.vm.options.backend = Backend::OpenAiApi;

    let mut settings = RecordingSettings::default();
    fixture.vm.close(&mut settings);

    assert_eq!(settings.backend.as_deref(), Some("openai_api"));
    let reloaded = humm_gui::recording::RecordingOptions::from_settings(&settings, false);
    assert_eq!(reloaded.backend, Backend::OpenAiApi);
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
fn advanced_llm_block_is_live_and_the_enable_edit_persists_to_live_options_not_settings() {
    // F-063: the LLM-translation block is now live (was permanently disabled
    // in M5). The "Enable AI translation" checkbox is enabled, and toggling it
    // mutates the *live* `RecordingOptions` (close-persisted, Buzz
    // `_do_close`) — NOT `settings.recording` immediately (unlike the
    // mode/export immediate-save fields).
    let mut harness = build_harness(fixture());
    harness.get_by_label("Advanced...").click();
    harness.step();
    harness.step();

    let checkbox = harness.get_by_label("Enable AI translation");
    assert!(
        !checkbox.accesskit_node().is_disabled(),
        "the LLM-translation checkbox is live now (F-063)"
    );
    assert!(!harness.state().vm.options.enable_llm_translation);

    harness.get_by_label("Enable AI translation").click();
    harness.step();
    harness.step(); // settle frame -- the same click/render-order lag other tests note.

    assert!(
        harness.state().vm.options.enable_llm_translation,
        "toggling the checkbox flips the live RecordingOptions"
    );
    assert!(
        !harness
            .state()
            .settings
            .lock()
            .unwrap()
            .recording
            .enable_llm_translation,
        "the LLM fields are close-persisted, not immediate-save — settings.recording is untouched until close"
    );

    // The model/instructions fields are present and enabled (they reuse the
    // shared options-llm-* strings).
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

// -- F-063: translation panel visibility --------------------------------------

#[test]
fn translation_panel_is_hidden_when_translation_is_disabled() {
    // Default fixture: `enable_llm_translation` is off — the translation panel
    // (and its "Waiting for AI translation..." placeholder) must not render.
    let mut harness = build_harness(fixture());
    harness.step();

    assert!(!harness.state().vm.options.enable_llm_translation);
    assert!(
        harness
            .query_by(|node: &AccessKitNode<'_>| {
                node.placeholder() == Some("Waiting for AI translation...")
            })
            .is_none(),
        "the translation panel is hidden while translation is disabled"
    );
}

#[test]
fn translation_panel_shows_with_its_placeholder_when_enabled_live() {
    // Enabling translation shows the panel immediately (Buzz `:544-547`'s
    // live show/hide) — its placeholder is the byte-exact Buzz string.
    let mut fixture = fixture();
    fixture.vm.options.enable_llm_translation = true;
    let mut harness = build_harness(fixture);
    harness.step();

    harness.get_by(|node: &AccessKitNode<'_>| {
        node.placeholder() == Some("Waiting for AI translation...")
    });
}

#[test]
fn translation_panel_shows_the_translation_text_when_present() {
    let mut fixture = fixture();
    fixture.vm.options.enable_llm_translation = true;
    fixture.vm.translation_panel_text = "la traducción en vivo".to_string();
    let mut harness = build_harness(fixture);
    harness.step();

    // The panel is a read-only `TextEdit`, so its text is the node's `value`
    // (not a `label`) — the same shape as the transcription panel. (The value
    // appears on both the text-input node and its inner text-run child, so
    // `query_all` + a count check rather than `get_by`'s exactly-one.)
    assert!(
        harness
            .query_all_by(|node: &AccessKitNode<'_>| {
                node.value().as_deref() == Some("la traducción en vivo")
            })
            .count()
            >= 1,
        "the translation panel shows the current translation text"
    );
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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
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

/// The `(task, language)` provenance log [`ScriptedEngine`] records each
/// `transcribe_live` call into (review finding: closes the API-arm e2e
/// test's provenance gap).
type OptsLog = Arc<Mutex<Vec<(TaskKind, Option<Language>)>>>;

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
            opts_log: Arc::new(Mutex::new(Vec::new())),
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
    /// Records `(task, language)` for every `transcribe_live` call — closes
    /// the review-flagged provenance gap for the API-arm e2e test, which
    /// asserts against this log that the session actually threaded
    /// `LiveOptions.task`/`.language` through to the scripted engine (a
    /// literal `TaskKind::Transcribe` default would otherwise pass the test
    /// silently). Unused (but still populated) by the Whisper-arm e2e test,
    /// which has no equivalent assertion.
    opts_log: OptsLog,
}

impl LiveEngineSession for ScriptedEngine {
    fn transcribe_live(
        &mut self,
        _samples: &[f32],
        opts: &LiveOptions,
        _prompt: &str,
        _cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        self.opts_log
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((opts.task, opts.language));
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

struct FailThenSucceedEngineFactory {
    calls: Arc<AtomicUsize>,
}

impl LiveEngineFactory for FailThenSucceedEngineFactory {
    fn load(
        &self,
        _model_path: &Path,
        _use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        Ok(Box::new(FailThenSucceedEngine {
            calls: Arc::clone(&self.calls),
        }))
    }
}

/// M8 Task 12 (spec §7.4, ledger #9): fails the first chunk with a genuine
/// (non-canceled) `EngineError`, then succeeds on the second — the
/// `chunk_skipped_shows_transient_warning` kittest's own scripted-session
/// seam, proving the tolerate-3 wiring end to end: `run_session` emits
/// `LiveEvent::ChunkSkipped`, `run_live_session`'s sink forwards
/// `SessionMessage::ChunkSkipped`, `app.rs::apply_session_message` sets
/// `RecordingVm::chunk_skipped_warning`, and the second call's success
/// clears it again via `SessionMessage::ChunkSkippedCleared`.
///
/// Deliberately does **not** pump the second batch itself (unlike
/// `ScriptedEngine` above): the chunk-skipped warning is transient by
/// design, so if the second batch were already queued the moment the first
/// call returns, the session thread could race straight through both calls
/// before the test's own polling ever observed the warning in between. The
/// test pumps the second batch itself, only after it has confirmed the
/// warning is showing — deterministic pacing instead of a race.
struct FailThenSucceedEngine {
    calls: Arc<AtomicUsize>,
}

impl LiveEngineSession for FailThenSucceedEngine {
    fn transcribe_live(
        &mut self,
        _samples: &[f32],
        _opts: &LiveOptions,
        _prompt: &str,
        _cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Err(EngineError::Inference {
                detail: "boom".to_string(),
            })
        } else {
            Ok("utterance two".to_string())
        }
    }
}

struct AlwaysFailEngineFactory {
    capture: Arc<Mutex<MockCapture>>,
    calls: Arc<AtomicUsize>,
}

impl LiveEngineFactory for AlwaysFailEngineFactory {
    fn load(
        &self,
        _model_path: &Path,
        _use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        Ok(Box::new(AlwaysFailEngine {
            capture: Arc::clone(&self.capture),
            calls: Arc::clone(&self.calls),
        }))
    }
}

/// Review-fix coverage: fails every chunk, unconditionally — the
/// `chunk_skipped_warning_clears_under_the_terminal_error_state` kittest's
/// own engine, reaching `run_session`'s 3rd-consecutive-failure terminal
/// `Error` (spec §7.4). Unlike `FailThenSucceedEngine`, it *does* pump each
/// next batch itself (on calls 1 and 2, i.e. before the 2nd and 3rd calls) —
/// this test only asserts the *end* state after the terminal error, not an
/// intermediate transient one, so the same race the other engine's doc
/// comment describes cannot bite here: there is no window in which an
/// intermediate state needs to be observed before it's overwritten.
struct AlwaysFailEngine {
    capture: Arc<Mutex<MockCapture>>,
    calls: Arc<AtomicUsize>,
}

impl LiveEngineSession for AlwaysFailEngine {
    fn transcribe_live(
        &mut self,
        _samples: &[f32],
        _opts: &LiveOptions,
        _prompt: &str,
        _cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            self.capture
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .pump(BATCH_SAMPLES);
        }
        Err(EngineError::Inference {
            detail: format!("boom{n}"),
        })
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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
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
        secrets: Arc::new(MemorySecretStore::new()),
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

    // Poll until the *running app's own table* shows the session row (spec
    // §4: inserted before the first chunk). The id is learned from a store
    // read; the success condition is `HummApp::tasks_row_for_test`, which is
    // what exercises `SessionMessage::TableRefresh` end to end (review
    // finding 3). The store commit is visible a moment before that message
    // is drained, so waiting only on `TasksVm::load` and then sampling the
    // app table once races that frame (macos-14 gates run 41049615029).
    let mut live_row_id: Option<TranscriptionId> = None;
    wait_for(
        &mut harness,
        "the running app's table to show the live row at session start",
        |app| {
            let Some(row) = TasksVm::load(&store).sorted_rows().first().cloned() else {
                return false;
            };
            live_row_id = Some(row.id);
            app.tasks_row_for_test(row.id)
                .is_some_and(|shown| shown.status == Status::InProgress)
        },
    );
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

// -- M8 Task 12: tolerate-3 chunk-skipped warning (spec §7.4, ledger #9) -----
//
// The transcript e2e's exact shape (Record -> Stop through a full `HummApp`),
// scripted so the first chunk fails and the second succeeds
// (`FailThenSucceedEngine` above) — proves the whole wiring chain from
// `LiveEvent::ChunkSkipped` (humm-core) through `SessionMessage::ChunkSkipped`
// (`live_session::run_live_session`'s sink) to `app.rs::apply_session_message`
// setting `RecordingVm::chunk_skipped_warning`, and that the second chunk's
// success clears it again via `SessionMessage::ChunkSkippedCleared`. The
// recording window itself is a deferred OS viewport this harness cannot
// query (this file's own module doc) — asserting on the running app's own
// `RecordingVm` state is the correct-altitude proof, the same convention
// every other assertion in this file's e2e tests already uses.

#[test]
fn chunk_skipped_shows_transient_warning() {
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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    });

    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));

    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(FailThenSucceedEngineFactory {
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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

    assert!(
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .chunk_skipped_warning
            .is_none(),
        "no warning before a session ever starts"
    );

    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();

    // The first (failing) chunk's detail lands on the VM as a transient
    // warning.
    wait_for(&mut harness, "the chunk-skipped warning to appear", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .chunk_skipped_warning
                .is_some()
        }
    });
    {
        let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
        assert!(
            vm.chunk_skipped_warning
                .as_deref()
                .is_some_and(|detail| detail.contains("boom")),
            "the warning must carry the engine error's own detail, got {:?}",
            vm.chunk_skipped_warning
        );
        assert!(
            vm.panel_text.is_empty(),
            "a skipped chunk contributes no panel text"
        );
    }

    // Now feed the second batch (deliberately withheld until this point —
    // see `FailThenSucceedEngine`'s own doc comment for why): the session
    // thread was idle-pausing on an empty ring since the first (failed)
    // chunk, so this is the only thing that can make a second engine call
    // happen.
    mock_capture
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .pump(BATCH_SAMPLES);

    // The second (successful) chunk clears the warning again — proving it's
    // transient, not sticky for the rest of the session.
    wait_for(&mut harness, "the second chunk to clear the warning", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
            vm.chunk_skipped_warning.is_none() && vm.panel_text.contains("utterance two")
        }
    });

    // Stop click — the session ends cleanly despite the earlier tolerated
    // failure.
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
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .chunk_skipped_warning
            .is_none(),
        "no lingering warning once the session has stopped"
    );
}

/// Review fix: the sibling case `chunk_skipped_shows_transient_warning`
/// doesn't cover — a session that fails once (the warning shows) and is
/// then **stopped directly, with no intervening successful chunk** to
/// clear it via `SessionMessage::ChunkSkippedCleared`. Isolates
/// `RecordingVm::finish_stopping`'s own clear (`recording.rs`): before the
/// fix, nothing on this path ever cleared `chunk_skipped_warning`, so it
/// would still read `Some("boom")` on the stopped view.
#[test]
fn chunk_skipped_warning_clears_on_stop_without_a_following_success() {
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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    });

    // Only the first batch is ever pumped in this test (`SharedMockCapture`'s
    // own `start()`) — a second is never fed, so `FailThenSucceedEngine`'s
    // second call (and therefore any success) is never reached.
    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));

    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(FailThenSucceedEngineFactory {
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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

    wait_for(&mut harness, "the chunk-skipped warning to appear", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .chunk_skipped_warning
                .is_some()
        }
    });

    // Stop directly — no batch2 pump, no intervening success.
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
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .chunk_skipped_warning
            .is_none(),
        "finish_stopping must clear a still-showing chunk-skipped warning \
         even with no intervening successful chunk"
    );
}

/// Review fix: the 3rd-consecutive-chunk-failure path (spec §7.4) must not
/// leave the tolerated-failure warning showing underneath its own terminal
/// error — isolates `SessionMessage::Error`'s own clear (`app.rs`).
#[test]
fn chunk_skipped_warning_clears_under_the_terminal_error_state() {
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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    });

    // 3 batches' worth: `AlwaysFailEngine` pumps the 2nd and 3rd itself
    // (see its own doc comment for why that's race-free here).
    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 3 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));

    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(AlwaysFailEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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

    // The 3rd consecutive failure ends the session with a terminal error
    // dialog (session.rs's own tolerate-3 contract).
    wait_for(&mut harness, "the terminal error to appear", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .error_dialog
                .is_some()
        }
    });

    assert!(
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .chunk_skipped_warning
            .is_none(),
        "the terminal error must not leave a stale chunk-skipped warning \
         showing underneath its own error dialog"
    );
}

// -- Task 18: API backend end-to-end (F-061b) --------------------------------
//
// Record -> 2 scripted utterances -> Stop against `Backend::OpenAiApi`,
// driven through the same real `HummApp` wiring as the Whisper-arm e2e test
// above. Per the task brief ("do NOT bind TCP in kittest"), this proves the
// *wiring* -- `run_live_session` selects the API arm and calls
// `LiveEngineFactory::load_api` instead of `ensure_model`/`load` -- via a
// scripted `LiveEngineFactory` whose `load_api` returns a canned
// `LiveEngineSession` and whose `load`/the model ensurer both panic if ever
// reached (proving the local-engine path is genuinely skipped, not merely
// undocumented). The real `LiveApiTranscriber`/HTTP request shape is instead
// covered by `humm-core`'s own `openai::live` mock-server tests (task
// report) -- this test never touches the network.

/// Overrides only [`LiveEngineFactory::load_api`] (F-061b); `load` panics if
/// ever called, proving `run_live_session`'s API arm never reaches the
/// local-engine path.
struct ScriptedApiEngineFactory {
    capture: Arc<Mutex<MockCapture>>,
    calls: Arc<AtomicUsize>,
    /// Shared with the returned [`ScriptedEngine`] so the test can assert
    /// what `task`/`language` actually reached it.
    opts_log: OptsLog,
}

impl LiveEngineFactory for ScriptedApiEngineFactory {
    fn load(
        &self,
        _model_path: &Path,
        _use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        unreachable!("the API backend must never call LiveEngineFactory::load")
    }

    fn load_api(&self, _cfg: humm_core::openai::OpenAiEngineConfig) -> Box<dyn LiveEngineSession> {
        Box::new(ScriptedEngine {
            capture: Arc::clone(&self.capture),
            calls: Arc::clone(&self.calls),
            opts_log: Arc::clone(&self.opts_log),
        })
    }
}

/// Panics if ever asked to ensure a model -- the API backend has none to
/// resolve (F-061b: `run_live_session` skips `model_ensurer` entirely
/// whenever `LiveSessionRequest::api_config` is `Some`).
struct NeverCalledEnsurerForApi;

impl ArtifactEnsurer for NeverCalledEnsurerForApi {
    fn ensure_model(
        &self,
        _model: &ModelSpec,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError> {
        unreachable!("the API backend must never call ArtifactEnsurer::ensure_model")
    }

    fn ensure_vad(
        &self,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Option<PathBuf> {
        None
    }
}

#[test]
fn api_backend_scripted_end_to_end_record_two_utterances_then_stop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("humm.sqlite3");
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));

    let mut settings = Settings::default();
    // F-061b: select the API backend before the window ever opens --
    // `RecordingVm::open` prefills `RecordingOptions` from exactly this
    // sticky field (`RecordingOptions::from_settings`).
    settings.recording.backend = Some("openai_api".to_string());
    // Deliberately non-default task/language (review finding: a hardcoded
    // `TaskKind::Transcribe`/`None` in the session-wiring code would pass
    // silently against the defaults `TaskKind::Transcribe`/`None` already
    // are -- selecting `Translate`/`fr` here means the `opts_log` assertion
    // below only passes if `LiveOptions.task`/`.language` genuinely carried
    // this session's real selections through to the scripted engine).
    settings.recording.task = Some("translate".to_string());
    settings.recording.language = Some("fr".to_string());

    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    // The window's own Api Key row seeds from this same store
    // (`RecordingVm::open`), and `app.rs::start_live_session`'s F-061b
    // missing-key precheck resolves the session's key from it too
    // (`RecordingDeps::secrets`) -- a real (if fake) key here means the
    // precheck passes and a session actually starts.
    secrets
        .set(SecretKey::OpenAiApiKey, "sk-test-key")
        .expect("seed a fake key");

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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    });

    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));
    let opts_log: OptsLog = Arc::new(Mutex::new(Vec::new()));

    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(NeverCalledEnsurerForApi),
        engine_factory: Arc::new(ScriptedApiEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
            opts_log: Arc::clone(&opts_log),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::clone(&secrets),
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
    assert_eq!(
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .options
            .backend,
        humm_core::domain::Backend::OpenAiApi,
        "the window opened with the sticky API backend selected"
    );
    let deps_arc = harness.state().recording_deps_for_test();

    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();

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
        "utterance one\n\nutterance two"
    );
    // No missing-key inline error fired (the precheck passed) -- the
    // ordinary transcription flow ran instead.
    assert_eq!(
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .error_dialog,
        None
    );

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

    let tasks = TasksVm::load(&store);
    let row = tasks
        .sorted_rows()
        .first()
        .expect("exactly one live session row");
    assert_eq!(row.status, Status::Completed);
    let segments = store.get_segment_rows(row.id).expect("segments");
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].text, "utterance one");
    assert_eq!(segments[1].text, "utterance two");
    assert_eq!(
        engine_calls.load(Ordering::SeqCst),
        2,
        "load_api's scripted engine ran exactly twice, never load/ensure_model"
    );

    // Provenance (review finding): the session's real task/language
    // selections -- `Translate`/`fr`, deliberately not the defaults --
    // must have reached the scripted engine via `LiveOptions`, not just
    // whatever `transcribe_chunk` itself defaults to.
    let logged = opts_log.lock().unwrap_or_else(|p| p.into_inner()).clone();
    assert_eq!(
        logged,
        vec![
            (TaskKind::Translate, Language::parse("fr")),
            (TaskKind::Translate, Language::parse("fr")),
        ],
        "both scripted-engine calls must carry the session's real task/language"
    );
}

#[test]
fn api_backend_missing_key_shows_an_inline_error_and_never_starts_a_session() {
    // F-061b: unlike Buzz (no precheck at all -- see the task report), a
    // missing/empty key aborts the Record click inline, before any session
    // thread is ever spawned.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));

    let mut settings = Settings::default();
    settings.recording.backend = Some("openai_api".to_string());

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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    });

    // `RecordingDeps::secrets` is left with no stored key at all (a fresh
    // `MemorySecretStore`) -- the precheck must degrade this to "missing",
    // same as a keyring read error would.
    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(MockCapture::new(Vec::new()))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(NeverCalledEnsurerForApi),
        engine_factory: Arc::new(ScriptedApiEngineFactory {
            capture: Arc::new(Mutex::new(MockCapture::new(Vec::new()))),
            calls: Arc::new(AtomicUsize::new(0)),
            opts_log: Arc::new(Mutex::new(Vec::new())),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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

    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&harness.state().recording_deps_for_test());
    harness.step();
    harness.step(); // settle: `harvest_recording_intent` runs the precheck this frame.

    let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(
        vm.status,
        RecordingStatus::Stopped,
        "the optimistic Stopped->Recording flip must be reverted, not left stranded"
    );
    assert_eq!(
        vm.error_dialog.as_deref(),
        Some(humm_gui::i18n::tr("viewer-translate-key-required-body").as_str()),
        "the missing-key dialog must be the reused viewer-translate-key-required-body string, \
         not a fresh ad-hoc message"
    );
    drop(vm);

    // No store row at all: no session thread was ever spawned to insert one.
    assert!(TasksVm::load(&store).sorted_rows().is_empty());
}

#[test]
fn missing_key_precheck_does_not_tear_down_a_still_running_translator_from_a_prior_session() {
    // Review fix (task 18): the F-061b missing-key precheck must run
    // *before* `start_live_session` tears down the previous session's
    // still-running live-translation worker (spec §7.4: a translator
    // outlives its own session's Stop). Session 1 (Whisper backend,
    // translation enabled) leaves a translator attached to the VM; session
    // 2 selects the API backend with no key configured, so its Record click
    // must abort inline *without* touching that translator.
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("humm.sqlite3");
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));
    let model_path = dir.path().join("fake-model.bin");
    std::fs::write(&model_path, b"never opened by the scripted engine").expect("write");

    let mut settings = Settings::default();
    settings.recording.enable_llm_translation = true;
    settings.recording.llm_model = Some("gpt-4o-mini".to_string());
    settings.recording.llm_prompt = Some("Translate.".to_string());

    // Session 1 runs on the Whisper backend and needs a key only for the
    // translation spawn path (`services.secrets`) -- session 2's own
    // precheck reads `RecordingDeps.secrets` instead (left empty below),
    // which is exactly what makes it fail.
    let translate_secrets = MemorySecretStore::new();
    translate_secrets
        .set(SecretKey::OpenAiApiKey, "sk-test")
        .expect("seed key");

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
        secrets: Arc::new(translate_secrets) as Arc<dyn SecretStore>,
        translate: Arc::new(ScriptedTranslateSpawner::new(Arc::new(|text: &str| {
            format!("X:{text}")
        }))),
        llm: None,
        validate: services::default_test_validate(),
    });

    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));

    // `RecordingDeps.secrets` starts with no OpenAI key at all -- session
    // 2's own precheck (which reads this store, not `services.secrets`)
    // must fail against it.
    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(ScriptedEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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

    // Session 1: Record -> both utterances -> Stop, translation on.
    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();
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
    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    wait_for(&mut harness, "session 1 to stop", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc.lock().unwrap_or_else(|p| p.into_inner()).status == RecordingStatus::Stopped
        }
    });
    assert!(
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .has_live_translator_for_test(),
        "session 1's translator must still be attached after Stop (spec §7.4 lifecycle)"
    );

    // Session 2: switch to the API backend (no key in `RecordingDeps.secrets`)
    // and click Record -- the precheck must abort inline, and must not have
    // touched session 1's translator on the way.
    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .options
        .backend = Backend::OpenAiApi;
    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();
    harness.step(); // settle: `harvest_recording_intent` runs the precheck this frame.

    let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(
        vm.status,
        RecordingStatus::Stopped,
        "the aborted second click must revert to Stopped"
    );
    assert!(
        vm.error_dialog.is_some(),
        "a missing key must show an inline error"
    );
    assert!(
        vm.has_live_translator_for_test(),
        "the still-running translator from session 1 must survive an aborted Record click -- \
         the precheck must run before the translator teardown, not after"
    );
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
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
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
        secrets: Arc::new(MemorySecretStore::new()),
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

// -- Task 17: scripted end-to-end live translation (F-063) -------------------
//
// The transcript e2e's exact shape (Record -> 2 scripted utterances -> Stop,
// through a full `HummApp`), plus the live-translation half: a scripted
// translate spawner mapping `text -> "X:" + text`, a seeded keyring key so the
// endpoint resolves, and `enable_llm_translation` on. Asserts the translation
// panel, the `.translated` export bytes, and the persisted per-segment
// translations — while the transcription panel/segments are unaffected.

#[test]
fn live_translation_end_to_end_panel_export_and_store() {
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
    // Translation on, with a configured model (append-below, default separator).
    settings.recording.enable_llm_translation = true;
    settings.recording.llm_model = Some("gpt-4o-mini".to_string());
    settings.recording.llm_prompt = Some("Translate.".to_string());

    // A seeded keyring key so `resolve_endpoint` returns `Some` (the scripted
    // spawner never touches the network regardless).
    let secrets = MemorySecretStore::new();
    secrets
        .set(SecretKey::OpenAiApiKey, "sk-test")
        .expect("seed key");

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
        secrets: Arc::new(secrets) as Arc<dyn SecretStore>,
        // The scripted translator: every enqueued transcript comes back as
        // `"X:" + text`, synchronously on the forwarder thread.
        translate: Arc::new(ScriptedTranslateSpawner::new(Arc::new(|text: &str| {
            format!("X:{text}")
        }))),
        llm: None,
        validate: services::default_test_validate(),
    });

    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));

    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(ScriptedEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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
    assert!(
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .options
            .enable_llm_translation,
        "the window opened with translation enabled (seeded from settings.recording)"
    );

    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();

    // Poll until both translations have landed on the translation panel.
    wait_for(&mut harness, "both translations to land on the panel", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .translation_panel_text
                .contains("X:utterance two")
        }
    });

    {
        let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(
            vm.translation_panel_text, "X:utterance one\n\nX:utterance two",
            "translation panel: append-below, default separator, both utterances"
        );
        // The transcription panel is unaffected by translation.
        assert_eq!(
            vm.panel_text, "utterance one\n\nutterance two",
            "the transcription panel carries the raw transcripts, not the translations"
        );
    }

    // Stop.
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

    // -- Store: each segment carries its `X:`-prefixed translation.
    let tasks = TasksVm::load(&store);
    let row = tasks
        .sorted_rows()
        .first()
        .expect("exactly one live session row")
        .clone();
    let segments = store.get_segment_rows(row.id).expect("segments");
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].text, "utterance one");
    assert_eq!(segments[1].text, "utterance two");
    assert_eq!(
        segments[0].translation, "X:utterance one",
        "segment 0 carries its persisted translation"
    );
    assert_eq!(
        segments[1].translation, "X:utterance two",
        "segment 1 carries its persisted translation"
    );

    // -- Export: the `.translated` sibling has exact bytes; the transcript file
    //    is the un-translated transcripts (both share one filename base).
    let mut translated: Option<PathBuf> = None;
    let mut transcript: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&export_dir).expect("read export dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.contains(".translated.") {
            translated = Some(path);
        } else {
            transcript = Some(path);
        }
    }
    let translated = translated.expect("a .translated export file exists");
    let transcript = transcript.expect("a transcript export file exists");
    assert_eq!(
        std::fs::read_to_string(&translated).expect("read translated"),
        "X:utterance one\n\nX:utterance two\n\n",
        "translated export: raw translations, append-below separators"
    );
    assert_eq!(
        std::fs::read_to_string(&transcript).expect("read transcript"),
        "utterance one\n\nutterance two\n\n",
        "transcript export is untouched by translation"
    );
}

// -- Task 17 review fix (F-063): late translation arrivals after Stop ---------
//
// The exact regression the review found: a translation that completes *after*
// the session's Stop (the session tail — common with real network latency) must
// still land in the panel/store/export while the recording window is open (spec
// §7.4). The synchronous `ScriptedTranslateSpawner` can't reproduce this — it
// translates the instant it enqueues, always mid-session. The
// `DeferredScriptedTranslateSpawner` queues each enqueue and lets the test fire
// every callback via `flush()` at a moment of its choosing: here, only *after*
// the session has fully stopped (`RecordingStatus::Stopped`, `vm.session` gone).
#[test]
fn late_translation_arrivals_after_stop_reach_panel_store_and_export() {
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
    settings.recording.enable_llm_translation = true;
    settings.recording.llm_model = Some("gpt-4o-mini".to_string());
    settings.recording.llm_prompt = Some("Translate.".to_string());

    let secrets = MemorySecretStore::new();
    secrets
        .set(SecretKey::OpenAiApiKey, "sk-test")
        .expect("seed key");

    // The deferred spawner: every enqueued transcript is *queued*, not
    // translated, until this test calls `flush()`. Held here so the test drives
    // the callback timing (and polls `pending_len`).
    let spawner = Arc::new(DeferredScriptedTranslateSpawner::new(Arc::new(
        |text: &str| format!("X:{text}"),
    )));

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
        secrets: Arc::new(secrets) as Arc<dyn SecretStore>,
        translate: Arc::clone(&spawner) as Arc<dyn TranslateSpawner>,
        llm: None,
        validate: services::default_test_validate(),
    });

    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));
    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(ScriptedEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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

    // Record.
    vm_arc
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_record_clicked(&deps_arc);
    harness.step();

    // Both utterances are transcribed and *enqueued* while recording — but the
    // deferred spawner has translated nothing yet, so the panel stays empty.
    wait_for(
        &mut harness,
        "both utterances to be enqueued for translation",
        {
            let spawner = Arc::clone(&spawner);
            move |_app| spawner.pending_len() == 2
        },
    );
    assert!(
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .translation_panel_text
            .is_empty(),
        "no translation has been flushed yet, so the panel is still empty while recording"
    );

    // Stop, and let the session fully finish. Reaching `RecordingStatus::Stopped`
    // means `finish_live_session` ran, which sets `vm.session = None` (dropping
    // the session's `SessionMessage` receiver) immediately before restoring the
    // stopped UI — so this status is exactly the point past which the old
    // session channel is dead.
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
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .translation_panel_text
            .is_empty(),
        "still empty right after Stop — nothing has been translated yet"
    );

    // NOW the translations arrive — strictly after Stop, with the session (and
    // its receiver) already gone. Pre-fix, these panel updates were routed
    // through the dead `SessionMessage` channel and silently vanished.
    let flushed = spawner.flush();
    assert_eq!(flushed, 2, "both queued translations fire on flush");

    // -- Panel: the late arrivals reach it (the pre-fix symptom — an empty
    //    panel after a post-Stop flush — is gone).
    wait_for(&mut harness, "the late translations to reach the panel", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .translation_panel_text
                .contains("X:utterance two")
        }
    });
    {
        let vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(
            vm.translation_panel_text, "X:utterance one\n\nX:utterance two",
            "the panel shows both late translations, applied after Stop"
        );
        assert!(
            !vm.translation_panel_text.is_empty(),
            "pre-fix symptom is gone: the panel is non-empty after a post-Stop flush"
        );
    }

    // -- Store: each segment carries its late-arriving translation.
    let tasks = TasksVm::load(&store);
    let row = tasks
        .sorted_rows()
        .first()
        .expect("exactly one live session row")
        .clone();
    let segments = store.get_segment_rows(row.id).expect("segments");
    assert_eq!(segments.len(), 2);
    assert_eq!(
        segments[0].translation, "X:utterance one",
        "segment 0 carries its post-Stop translation"
    );
    assert_eq!(
        segments[1].translation, "X:utterance two",
        "segment 1 carries its post-Stop translation"
    );

    // -- Export: the `.translated` sibling was written by the post-Stop flush.
    let mut translated: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&export_dir).expect("read export dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.contains(".translated.") {
            translated = Some(path);
        }
    }
    let translated =
        translated.expect("a .translated export file exists after the post-Stop flush");
    assert_eq!(
        std::fs::read_to_string(&translated).expect("read translated"),
        "X:utterance one\n\nX:utterance two\n\n",
        "translated export carries the late translations"
    );
}

#[test]
fn live_translation_disabled_spawns_no_translator_and_persists_no_translations() {
    // The negative: translation off means no worker, no `.translated` file, no
    // per-segment translations — even with a seeded key and scripted spawner
    // available. Proves the enable flag actually gates the whole wiring.
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("humm.sqlite3");
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));
    let export_dir = dir.path().join("export");
    std::fs::create_dir_all(&export_dir).expect("mkdir export");
    let model_path = dir.path().join("fake-model.bin");
    std::fs::write(&model_path, b"never opened").expect("write");

    let mut settings = Settings::default();
    settings.recording.export_enabled = true;
    settings.recording.export_folder = Some(export_dir.clone());
    settings.recording.enable_llm_translation = false; // off

    let secrets = MemorySecretStore::new();
    secrets.set(SecretKey::OpenAiApiKey, "sk-test").unwrap();

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
        secrets: Arc::new(secrets) as Arc<dyn SecretStore>,
        translate: Arc::new(ScriptedTranslateSpawner::new(Arc::new(|text: &str| {
            format!("X:{text}")
        }))),
        llm: None,
        validate: services::default_test_validate(),
    });

    let mock_capture = Arc::new(Mutex::new(MockCapture::new(vec![0.5; 2 * BATCH_SAMPLES])));
    let engine_calls = Arc::new(AtomicUsize::new(0));
    let scripted_deps = RecordingDeps {
        capture: Mutex::new(Box::new(SharedMockCapture(Arc::clone(&mock_capture)))),
        store: Arc::clone(&store) as Arc<dyn LiveStore>,
        model_ensurer: Arc::new(InstantEnsurer { model_path }),
        engine_factory: Arc::new(ScriptedEngineFactory {
            capture: Arc::clone(&mock_capture),
            calls: Arc::clone(&engine_calls),
        }),
        upload_factory: Arc::new(NeverCalledUploadFactory),
        dialogs: Arc::new(humm_gui::import::RfdFileDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
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

    wait_for(&mut harness, "the second utterance to land", {
        let vm_arc = Arc::clone(&vm_arc);
        move |_app| {
            vm_arc
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .panel_text
                .contains("utterance two")
        }
    });
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
        vm_arc
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .translation_panel_text
            .is_empty(),
        "no translation panel text without translation"
    );

    let tasks = TasksVm::load(&store);
    let row = tasks.sorted_rows().first().expect("one row").clone();
    let segments = store.get_segment_rows(row.id).expect("segments");
    assert_eq!(segments.len(), 2);
    assert!(
        segments.iter().all(|s| s.translation.is_empty()),
        "no per-segment translations were persisted"
    );

    // No `.translated` sibling was written (only the transcript file).
    let translated_exists = std::fs::read_dir(&export_dir)
        .expect("read export dir")
        .filter_map(Result::ok)
        .any(|e| {
            e.path()
                .file_name()
                .map(|n| n.to_string_lossy().contains(".translated."))
                .unwrap_or(false)
        });
    assert!(
        !translated_exists,
        "no .translated file without translation"
    );
}
