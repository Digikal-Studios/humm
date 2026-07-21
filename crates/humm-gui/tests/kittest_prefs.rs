//! kittest coverage for Task 11 (F-044 basic, ADR-009, F-006/ADR-015):
//! Privacy-tab toggle persistence, Models-tab planted-file state + the
//! Delete confirm flow, and the toolbar's update chip appearing once an
//! `UpdateNotice::Available` lands on the update channel.
//!
//! `Preferences`/`About` are real OS viewports (`app.rs::HummApp::
//! show_preferences_viewport`/`show_about_viewport`), which `egui_kittest`'s
//! single-`Harness` accessibility tree cannot reach into — only the *root*
//! viewport a `Harness` drives is visible to it. Per `views::preferences`'s
//! own module doc, this file instead drives the extracted, plain
//! `preferences::ui`/`models_ui::ui` functions directly, rendered inside a
//! bare `egui::Ui` via `Harness::new_ui_state` (no `HummApp`/`eframe::App`
//! involved for those two tests) — proving the tab logic itself end to end.
//! The update-chip test *does* go through a real `HummApp` (the chip lives
//! in the always-visible main window, not a child viewport), the same shape
//! every other kittest file in this crate uses.
//!
//! What this file does **not** (and structurally cannot) cover: actually
//! opening the Preferences/About windows as separate OS windows, or a real
//! multi-window smoke test — that's `docs/gui-manual/M3.md`'s manual
//! checklist (M3 Task 13), run on an unsandboxed machine with a display.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use humm_core::domain::ModelSize;
use humm_core::models::registry::{SENTINEL_SUFFIX, ggml_file_name};
use humm_core::settings::Settings;
use humm_core::update::UpdateInfo;
use humm_gui::app::HummApp;
use humm_gui::models_ui::{self, ModelsTabState};
use humm_gui::services::{self, Capabilities, TestDeps, UpdateNotice};
use humm_gui::views::preferences::{self, PreferencesState, PrefsTab};
use humm_store::SqliteStore;

// -- Privacy tab: toggles persist to the settings file -----------------------

/// The Privacy tab's own render-loop fixture: `state` starts on
/// [`PrefsTab::Privacy`] so [`preferences::ui`] renders that tab's checkboxes
/// straight away.
struct PrivacyFixture {
    state: PreferencesState,
    model_root: PathBuf,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
}

#[test]
fn privacy_toggles_persist_to_the_settings_file() {
    let model_root_dir = tempfile::tempdir().expect("model root tempdir");
    let settings_dir = tempfile::tempdir().expect("settings tempdir");
    let settings_path = settings_dir.path().join("settings.toml");

    // `services::init_with` gives a realistic `Arc<Mutex<Settings>>`/
    // `settings_path` pair (temp file, never touched by any other test) —
    // the same construction every other kittest fixture in this crate uses,
    // per the brief's "temp settings path via init_with".
    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let services = services::init_with(TestDeps {
        store,
        media_dir: PathBuf::from("/unused-media-dir"),
        model_root: model_root_dir.path().to_path_buf(),
        settings: Settings::default(),
        settings_path: settings_path.clone(),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
    });
    let settings = Arc::clone(&services.settings);

    let fixture = PrivacyFixture {
        state: PreferencesState {
            tab: PrefsTab::Privacy,
            models: ModelsTabState::new(model_root_dir.path(), None),
        },
        model_root: model_root_dir.path().to_path_buf(),
        settings,
        settings_path: settings_path.clone(),
    };

    let mut harness = Harness::new_ui_state(
        |ui, fixture: &mut PrivacyFixture| {
            preferences::ui(
                ui,
                &mut fixture.state,
                &fixture.model_root,
                &fixture.settings,
                &fixture.settings_path,
            );
        },
        fixture,
    );
    harness.run();

    // Both toggles default on (`UpdateSettings::check_enabled`/
    // `PrivacySettings::telemetry_enabled` both default `true`) — clicking
    // each turns it off, which is what this test proves lands on disk.
    harness
        .get_by_label("Send anonymous launch telemetry")
        .click();
    harness.run();
    harness
        .get_by_label("Check for updates automatically")
        .click();
    harness.run();

    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(
        !reloaded.privacy.telemetry_enabled,
        "the telemetry toggle must persist to disk, not just in memory"
    );
    assert!(
        !reloaded.updates.check_enabled,
        "the update-check toggle must persist to disk, not just in memory"
    );
}

struct NeverCalledEngineFactory;

impl humm_core::queue::EngineFactory for NeverCalledEngineFactory {
    fn create(
        &mut self,
        _model: &humm_core::domain::ModelSpec,
        _resolved: Option<&std::path::Path>,
    ) -> Result<Box<dyn humm_core::boundary::TranscriptionEngine>, humm_core::error::CoreError>
    {
        unreachable!("no task is ever run in this test")
    }
}

// -- Models tab: planted-tiny renders Downloaded; Delete confirm removes it --

struct ModelsFixture {
    state: ModelsTabState,
    root: PathBuf,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
}

#[test]
fn models_tab_shows_planted_tiny_as_downloaded_and_delete_confirm_removes_it() {
    let root = tempfile::tempdir().expect("model root tempdir");
    let ggml_path = root.path().join(ggml_file_name(ModelSize::Tiny));
    std::fs::write(&ggml_path, b"fake tiny ggml").expect("write ggml");
    let mut sentinel_name = ggml_path.file_name().expect("file name").to_os_string();
    sentinel_name.push(SENTINEL_SUFFIX);
    std::fs::write(ggml_path.with_file_name(sentinel_name), b"").expect("write sentinel");

    let state = ModelsTabState::new(root.path(), None);
    // Premise: refresh already found the planted Tiny model as Downloaded.
    let tiny = state
        .rows
        .iter()
        .find(|row| row.size == ModelSize::Tiny)
        .expect("tiny row present");
    assert!(
        matches!(tiny.state, models_ui::RowState::Downloaded(_)),
        "expected the planted Tiny model to refresh as Downloaded, got {:?}",
        tiny.state
    );

    let fixture = ModelsFixture {
        state,
        root: root.path().to_path_buf(),
        settings: Arc::new(Mutex::new(Settings::default())),
        settings_path: root.path().join("unused-settings.toml"),
    };

    let mut harness = Harness::new_ui_state(
        |ui, fixture: &mut ModelsFixture| {
            models_ui::ui(
                ui,
                &mut fixture.state,
                &fixture.root,
                &fixture.settings,
                &fixture.settings_path,
            );
        },
        fixture,
    );
    harness.run();

    // Only a Downloaded row offers Delete/Show file location — proves the
    // rendered row actually reflects the planted on-disk state, not just
    // the state struct's own field.
    harness.get_by_label("Delete").click();
    harness.run();

    harness.get_by_label_contains("Delete Model");
    harness.get_by_label_contains("Are you sure you want to delete the selected model?");

    harness.get_by_label("OK").click();
    harness.run();

    assert!(
        !ggml_path.exists(),
        "confirming Delete must remove the ggml file from disk"
    );
}

// -- Toolbar update chip: appears once an Available notice lands -------------

#[test]
fn update_chip_appears_when_available_notice_is_injected() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let services = services::init_with(TestDeps {
        store,
        media_dir: PathBuf::from("/unused-media-dir"),
        model_root: PathBuf::from("/unused-model-root"),
        settings: Settings::default(),
        settings_path: PathBuf::from("/unused-settings-path"),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
    });
    // `Services::update_tx` is exposed exactly so a test can drive this
    // hermetically instead of racing a real network check.
    let update_tx = services.update_tx.clone();

    let mut harness = Harness::new_eframe(move |_cc| HummApp::new_for_test(services));
    harness.run();
    assert!(
        harness.query_by_label("Update Available").is_none(),
        "the chip must start hidden (Task 8's placeholder default)"
    );

    update_tx
        .send(UpdateNotice::Available(UpdateInfo {
            version: "99.0.0".to_string(),
            release_notes: "Big release".to_string(),
            download_urls: Vec::new(),
        }))
        .expect("send update notice");
    harness.run();

    harness.get_by_label("Update Available");
}
