//! kittest coverage for Task 10 (F-060/F-004): the recording window's chrome
//! renders (`views::recording::recording_ui`, driven directly over a bare
//! `egui::Ui` via `Harness::new_ui_state` — the recording window is a
//! deferred OS viewport an `egui_kittest` `Harness` cannot reach into, the
//! same constraint `kittest_viewer.rs`'s own module doc explains for the
//! transcript viewer), the Record click's UI state machine, the meter's
//! repeated-paint stability, and the toolbar Record button's F-004 unlock.
//!
//! Every fixture here uses `humm_audio::MockCapture` — never `CpalCapture` —
//! including the full-`HummApp` toolbar test: `HummApp::new_for_test` always
//! wires a `MockCapture` (see `app.rs`'s `recording_deps` field doc), so no
//! test in this file ever touches real audio hardware.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use egui_kittest::kittest::{AccessKitNode, NodeT, Queryable};
use egui_kittest::{Harness, HarnessBuilder};
use humm_audio::MockCapture;
use humm_core::boundary::AudioDevice;
use humm_core::domain::ModelSpec;
use humm_core::error::CoreError;
use humm_core::queue::EngineFactory;
use humm_core::settings::Settings;
use humm_gui::app::HummApp;
use humm_gui::recording::{RecordingDeps, RecordingStatus, RecordingVm};
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_gui::views::recording::recording_ui;
use humm_store::SqliteStore;

/// A fake [`EngineFactory`] for tests that never actually run a job: nothing
/// in this file enqueues a transcription task.
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
/// plus everything `recording_ui` needs.
struct Fixture {
    vm: RecordingVm,
    deps: RecordingDeps,
    custom_model_available: bool,
    measured_speed: BTreeMap<String, f32>,
}

fn fixture_with_devices(devices: Vec<AudioDevice>) -> Fixture {
    let deps = RecordingDeps {
        capture: Mutex::new(Box::new(MockCapture::new(Vec::new()).with_devices(devices))),
    };
    let settings = Settings::default();
    let vm = RecordingVm::open(&deps, &settings);
    Fixture {
        vm,
        deps,
        custom_model_available: false,
        measured_speed: BTreeMap::new(),
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
fn stop_click_flips_back_and_shows_the_copy_bar_placeholder() {
    let mut harness = build_harness(fixture());
    harness.step();
    harness.get_by_label("Record").click();
    harness.step();
    harness.step(); // settle frame — see the sibling test's comment.
    assert_eq!(harness.state().vm.status, RecordingStatus::Recording);

    harness.get_by_label("Stop").click();
    harness.step();
    harness.step(); // settle frame.

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
