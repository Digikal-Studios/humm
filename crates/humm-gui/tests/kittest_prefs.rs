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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui_kittest::kittest::{By, NodeT, Queryable};
use egui_kittest::{Harness, HarnessBuilder};
use humm_core::domain::ModelSize;
use humm_core::models::registry::{SENTINEL_SUFFIX, ggml_file_name};
use humm_core::openai::{OpenAiAuth, ValidateOutcome};
use humm_core::secrets::{MemorySecretStore, SecretKey, SecretStore};
use humm_core::settings::Settings;
use humm_core::update::UpdateInfo;
use humm_gui::app::HummApp;
use humm_gui::i18n;
use humm_gui::models_ui::{self, ModelsTabState};
use humm_gui::services::{self, Capabilities, TestDeps, UpdateNotice};
use humm_gui::shortcuts::ShortcutMap;
use humm_gui::views::preferences::{self, PreferencesState, PrefsTab};
use humm_gui::views::prefs_folder_watch::FolderWatchTabState;
use humm_gui::views::prefs_general::{self, GeneralModal, GeneralTabState, ValidateFn};
use humm_gui::views::prefs_shortcuts::ShortcutsTabState;
use humm_store::SqliteStore;

/// A [`FileDialogs`] that never returns a pick — every test in this file
/// exercises tabs whose rendering doesn't need a real dialog (the Folder
/// Watch tab's own Browse-button coverage lives in
/// `kittest_prefs_folder_watch.rs`, with its own scripted double).
struct NullDialogs;

impl humm_gui::import::FileDialogs for NullDialogs {
    fn pick_files(&self, _start_dir: Option<&std::path::Path>) -> Option<Vec<PathBuf>> {
        None
    }
    fn pick_folder(&self, _start_dir: Option<&std::path::Path>) -> Option<PathBuf> {
        None
    }
    fn save_file(
        &self,
        _title: &str,
        _default_path: &std::path::Path,
        _filter_name: &str,
        _extension: &str,
    ) -> Option<PathBuf> {
        None
    }
}

/// Polls `condition` every 5 ms, stepping `harness` each time so its own
/// per-frame render (and, for the General tab, its `drain_test_result`
/// channel poll) actually runs — the same deadline-loop idiom
/// `kittest_recording.rs`'s own `wait_for` uses for real background-thread
/// work, generalized over the fixture type since this file's fixtures aren't
/// a `HummApp`. Panics with `message` if `condition` hasn't been met within
/// 10 s.
fn wait_for<S>(harness: &mut Harness<'_, S>, message: &str, mut condition: impl FnMut(&S) -> bool) {
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

// -- Privacy tab: toggles persist to the settings file -----------------------

/// The Privacy tab's own render-loop fixture: `state` starts on
/// [`PrefsTab::Privacy`] so [`preferences::ui`] renders that tab's checkboxes
/// straight away.
struct PrivacyFixture {
    state: PreferencesState,
    model_root: PathBuf,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    validate: ValidateFn,
    shortcuts_generation: Arc<AtomicUsize>,
    current_shortcuts: ShortcutMap,
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
        media_dir: settings_dir.path().join("media"),
        model_root: model_root_dir.path().to_path_buf(),
        settings: Settings::default(),
        settings_path: settings_path.clone(),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    });
    let settings = Arc::clone(&services.settings);
    let secrets = Arc::clone(&services.secrets);
    let validate = Arc::clone(&services.validate);

    let fixture = PrivacyFixture {
        state: PreferencesState {
            tab: PrefsTab::Privacy,
            general: GeneralTabState::new(secrets.as_ref(), &Settings::default()),
            models: ModelsTabState::new(model_root_dir.path(), None, false),
            shortcuts: ShortcutsTabState::default(),
            folder_watch: FolderWatchTabState::new(
                &humm_core::settings::FolderWatchSettings::default(),
                secrets.as_ref(),
                false,
                false,
            ),
        },
        model_root: model_root_dir.path().to_path_buf(),
        settings,
        settings_path: settings_path.clone(),
        secrets,
        validate,
        shortcuts_generation: Arc::new(AtomicUsize::new(0)),
        current_shortcuts: ShortcutMap::from_settings(&HashMap::new()),
    };

    let mut harness = Harness::new_ui_state(
        |ui, fixture: &mut PrivacyFixture| {
            preferences::ui(
                ui,
                &mut fixture.state,
                &fixture.model_root,
                &fixture.settings,
                &fixture.settings_path,
                &fixture.secrets,
                &fixture.validate,
                &fixture.shortcuts_generation,
                &fixture.current_shortcuts,
                &NullDialogs,
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

// -- General tab: OpenAI block (F-100 complete, design spec §8.1) ------------

/// The General tab's own render-loop fixture: `state` starts on
/// [`PrefsTab::General`] so [`preferences::ui`] renders the OpenAI block
/// straight away. Owns its two backing [`tempfile::TempDir`] guards (`_
///`-prefixed, never read) purely so they live exactly as long as the
/// fixture (and therefore the [`Harness`]) does and clean up on drop at the
/// end of each test, rather than being deleted the moment [`general_harness`]
/// returns.
struct GeneralFixture {
    state: PreferencesState,
    model_root: PathBuf,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    validate: ValidateFn,
    shortcuts_generation: Arc<AtomicUsize>,
    current_shortcuts: ShortcutMap,
    _model_root_dir: tempfile::TempDir,
    _settings_dir: tempfile::TempDir,
}

/// Builds a [`GeneralFixture`]'s harness from caller-supplied `secrets`/
/// `validate` seams — every test below wires its own `secrets`
/// ([`MemorySecretStore`], never the real keychain) and `validate` (a
/// scripted closure, never the network).
fn general_harness(
    secrets: Arc<dyn SecretStore>,
    validate: ValidateFn,
) -> Harness<'static, GeneralFixture> {
    let model_root_dir = tempfile::tempdir().expect("model root tempdir");
    let settings_dir = tempfile::tempdir().expect("settings tempdir");
    let settings_path = settings_dir.path().join("settings.toml");
    let model_root = model_root_dir.path().to_path_buf();

    let fixture = GeneralFixture {
        state: PreferencesState {
            tab: PrefsTab::General,
            general: GeneralTabState::new(secrets.as_ref(), &Settings::default()),
            models: ModelsTabState::new(&model_root, None, false),
            shortcuts: ShortcutsTabState::default(),
            folder_watch: FolderWatchTabState::new(
                &humm_core::settings::FolderWatchSettings::default(),
                secrets.as_ref(),
                false,
                false,
            ),
        },
        model_root,
        settings: Arc::new(Mutex::new(Settings::default())),
        settings_path,
        secrets,
        validate,
        shortcuts_generation: Arc::new(AtomicUsize::new(0)),
        current_shortcuts: ShortcutMap::from_settings(&HashMap::new()),
        _model_root_dir: model_root_dir,
        _settings_dir: settings_dir,
    };

    Harness::new_ui_state(
        |ui, fixture: &mut GeneralFixture| {
            preferences::ui(
                ui,
                &mut fixture.state,
                &fixture.model_root,
                &fixture.settings,
                &fixture.settings_path,
                &fixture.secrets,
                &fixture.validate,
                &fixture.shortcuts_generation,
                &fixture.current_shortcuts,
                &NullDialogs,
            );
        },
        fixture,
    )
}

/// Drives the commit seam directly — the same "state seam, not simulated
/// typing" precedent `kittest_viewer.rs`'s `speed_combo_echoes_clamped_value`
/// sets for `playback_controls::commit_speed_text`: mutate the buffer, call
/// the pure commit function, assert the side effect. No `Harness` is needed
/// at all for this one, since nothing about the assertion depends on
/// egui's rendering.
#[test]
fn general_tab_key_field_persists_to_secret_store() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let mut state = GeneralTabState::new(secrets.as_ref(), &Settings::default());
    state.key_buffer = "sk-committed-via-seam".to_string();

    prefs_general::commit_key(&mut state, &secrets);

    assert_eq!(
        secrets.get(SecretKey::OpenAiApiKey).unwrap(),
        Some("sk-committed-via-seam".to_string())
    );
    assert_eq!(
        state.modal, None,
        "a valid-charset key must not open the warning modal"
    );
}

#[test]
fn test_button_disabled_without_key_enabled_with() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let mut harness = general_harness(secrets, services::default_test_validate());
    harness.run();

    assert!(
        harness.get_by_label("Test").accesskit_node().is_disabled(),
        "Test must start disabled -- the key field is empty"
    );

    harness.state_mut().state.general.key_buffer = "sk-now-has-a-key".to_string();
    harness.run();

    assert!(
        !harness.get_by_label("Test").accesskit_node().is_disabled(),
        "Test must enable once the key field is non-empty"
    );
}

#[test]
fn test_button_success_shows_valid_modal() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let validate: ValidateFn = Arc::new(|_auth: OpenAiAuth| ValidateOutcome::Valid);
    let mut harness = general_harness(secrets, validate);
    harness.state_mut().state.general.key_buffer = "sk-good-key".to_string();
    harness.run();

    harness.get_by_label("Test").click();
    wait_for(&mut harness, "the test-result modal to open", |fixture| {
        fixture.state.general.modal.is_some()
    });
    harness.run();

    assert_eq!(
        harness.state().state.general.modal,
        Some(GeneralModal::TestResult(
            "Your API key is valid. humm will use this key to perform Whisper API \
             transcriptions and AI translations."
                .to_string()
        ))
    );
    harness.get_by_label_contains("OpenAI API Key Test");
    harness.get_by_label_contains(
        "Your API key is valid. humm will use this key to perform Whisper API \
         transcriptions and AI translations.",
    );
    assert!(
        !harness.state().state.general.test_running,
        "test_running must clear once the outcome lands"
    );
}

#[test]
fn test_button_failure_shows_message() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let validate: ValidateFn = Arc::new(|_auth: OpenAiAuth| ValidateOutcome::Invalid {
        message: "Incorrect API key provided".to_string(),
    });
    let mut harness = general_harness(secrets, validate);
    harness.state_mut().state.general.key_buffer = "sk-bad-key".to_string();
    harness.run();

    harness.get_by_label("Test").click();
    wait_for(&mut harness, "the test-result modal to open", |fixture| {
        fixture.state.general.modal.is_some()
    });
    harness.run();

    assert_eq!(
        harness.state().state.general.modal,
        Some(GeneralModal::TestResult(
            "Incorrect API key provided".to_string()
        ))
    );
    harness.get_by_label_contains("Incorrect API key provided");
}

#[test]
fn base_url_and_model_fields_persist_to_settings() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let mut harness = general_harness(secrets, services::default_test_validate());
    harness.run();

    {
        let fixture = harness.state_mut();
        fixture.state.general.base_url_buffer = "https://gateway.example.com/v1".to_string();
        prefs_general::commit_base_url(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
        );
        fixture.state.general.api_model_buffer = "whisper-1-custom".to_string();
        prefs_general::commit_api_model(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
        );
    }

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.openai.base_url.as_deref(),
        Some("https://gateway.example.com/v1")
    );
    assert_eq!(reloaded.openai.api_model, "whisper-1-custom");

    // Emptying the base-URL field clears it to `None`; emptying the model
    // field resets to the "whisper-1" default (design spec §7.1, this
    // module's documented divergence for the model field).
    {
        let fixture = harness.state_mut();
        fixture.state.general.base_url_buffer = String::new();
        prefs_general::commit_base_url(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
        );
        fixture.state.general.api_model_buffer = String::new();
        prefs_general::commit_api_model(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
        );
    }
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(reloaded.openai.base_url, None);
    assert_eq!(reloaded.openai.api_model, "whisper-1");
}

// -- General tab: the M7 Task 11 rows (F-100 complete) -----------------------

/// The UI-language combo lists exactly [`i18n::AVAILABLE_LOCALES`] (the
/// F-008b starter set: `en_US`/`de_DE`/`es_ES`/`zh_CN`) and selecting an entry persists
/// `ui.locale` (live apply -- design spec ledger #L10). Driving a `ComboBox`
/// selection by simulated click has no kittest precedent in this workspace
/// (`kittest_import.rs`/`kittest_recording.rs` both document the same gap
/// and mutate backing state directly instead) -- this test follows suit,
/// asserting the rendered combo's closed-state label matches the negotiated
/// locale's display name, then driving the "selection" via the commit seam
/// directly, exactly like `base_url_and_model_fields_persist_to_settings`
/// above does for its own text fields.
///
/// Wrapped in [`i18n::lock_locale_for_test`]: [`prefs_general::commit_locale`]
/// calls [`i18n::set_locale`], which mutates process-wide global state other
/// tests (in this file and `i18n.rs`) also touch.
#[test]
fn language_combo_lists_available_locales_and_persists() {
    let _locale_guard = i18n::lock_locale_for_test();
    assert_eq!(
        i18n::AVAILABLE_LOCALES.len(),
        4,
        "the F-008b starter set embeds en_US/de_DE/es_ES/zh_CN -- update this \
         count as each of Buzz's remaining 11 locales is ported"
    );

    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let mut harness = general_harness(secrets, services::default_test_validate());
    harness.run();

    // The combo's closed-state control shows the negotiated locale's display
    // name as its accessible *value* (accesskit's `ComboBox` role reports its
    // current selection there, not as a `label`) -- proves it renders
    // through `i18n::display_name`, not a hardcoded string.
    harness.get(
        By::new()
            .role(Role::ComboBox)
            .value(&i18n::display_name("en_US")),
    );

    {
        let fixture = harness.state_mut();
        fixture.state.general.locale_code = "en_US".to_string();
        prefs_general::commit_locale(
            &fixture.state.general.locale_code,
            &fixture.settings,
            &fixture.settings_path,
        );
    }

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(reloaded.ui.locale.as_deref(), Some("en_US"));
}

/// The font-size row: a `DragValue` has no kittest-driving precedent in this
/// workspace either (confirmed: zero uses across `crates/humm-gui/tests/`),
/// so this drives the commit seam directly (`prefs_general::commit_font_size`)
/// -- the same "state seam, not simulated widget interaction" precedent
/// `base_url_and_model_fields_persist_to_settings` sets for this file's own
/// text fields. Proves both persistence and the `UiSettings::MIN_FONT_SIZE`/
/// `MAX_FONT_SIZE` clamp (Task 1's contract) -- an out-of-range value is
/// clamped *at commit time* here, matching `Settings::load`'s own
/// `UiSettings::clamp` for a value that reached disk some other way (a
/// hand-edited file).
#[test]
fn font_size_persists_and_clamps() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let mut harness = general_harness(secrets, services::default_test_validate());
    harness.run();

    let ctx = egui::Context::default();
    {
        let fixture = harness.state_mut();
        fixture.state.general.font_size_custom = true;
        fixture.state.general.font_size_value = 99; // above MAX_FONT_SIZE (32)
        prefs_general::commit_font_size(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
            &ctx,
        );
    }

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.ui.font_size,
        Some(humm_core::settings::UiSettings::MAX_FONT_SIZE),
        "an out-of-range value must be clamped before it's persisted"
    );

    // Unchecking "Custom font size" clears the setting back to `None` (the
    // egui default), regardless of whatever number is still in the buffer.
    {
        let fixture = harness.state_mut();
        fixture.state.general.font_size_custom = false;
        prefs_general::commit_font_size(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
            &ctx,
        );
    }
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(reloaded.ui.font_size, None);
}

/// The "Default export file name" field: commit-on-focus-out, empty ->
/// `None` (`prefs_general::commit_export_template`), driven through the real
/// `TextEdit` widget via focus/type/Tab -- the same shape
/// `custom_url_field_persists_on_lost_focus_not_per_keystroke` uses for the
/// Models tab's own custom-URL field. Unlike that field, the General tab now
/// has several single-line text inputs, so this locates the export-template
/// field specifically by its hint text (the placeholder
/// `DEFAULT_EXPORT_TEMPLATE`) rather than a bare `Role::TextInput` query,
/// which would be ambiguous here.
#[test]
fn export_template_field_persists_empty_as_none() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let mut harness = general_harness(secrets, services::default_test_validate());
    harness.run();

    // Seam-level round trip first (mirrors `base_url_and_model_fields_
    // persist_to_settings`): a non-empty value saves verbatim, an emptied
    // value clears to `None`.
    {
        let fixture = harness.state_mut();
        fixture.state.general.export_template_buffer = "{{ input_file_name }}-custom".to_string();
        prefs_general::commit_export_template(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
        );
    }
    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.file_transcriber.default_export_template.as_deref(),
        Some("{{ input_file_name }}-custom")
    );

    {
        let fixture = harness.state_mut();
        fixture.state.general.export_template_buffer = String::new();
        prefs_general::commit_export_template(
            &fixture.state.general,
            &fixture.settings,
            &fixture.settings_path,
        );
    }
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.file_transcriber.default_export_template, None,
        "an emptied field must clear the setting to None, not persist an \
         empty string"
    );
}

/// The "Disable GPU" checkbox: driven by label click, exactly like the
/// Privacy tab's own checkboxes (`privacy_toggles_persist_to_the_settings_
/// file`) -- egui checkboxes render with an accessible label matching their
/// caption text, and this row's checkbox caption
/// ("Use only CPU and disable GPU acceleration") is unique on the tab.
#[test]
fn force_cpu_checkbox_persists() {
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let mut harness = general_harness(secrets, services::default_test_validate());
    harness.run();

    harness
        .get_by_label("Use only CPU and disable GPU acceleration")
        .click();
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(
        reloaded.models.force_cpu,
        "clicking the checkbox must persist force_cpu = true"
    );

    harness
        .get_by_label("Use only CPU and disable GPU acceleration")
        .click();
    harness.run();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(
        !reloaded.models.force_cpu,
        "clicking it again must persist force_cpu = false"
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

    let state = ModelsTabState::new(root.path(), None, false);
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

// -- Models tab: custom-URL field persists on focus-loss, not per keystroke --

/// Carried M3 ticket (spec §8.3): typing into the custom-URL field must not
/// hit disk per keystroke — only once the field loses focus (here, via Tab;
/// egui's singleline `TextEdit` treats Enter the same way, per its own
/// internal `surrender_focus` on Enter). `.focus()`/`.type_text()` on the
/// field's `Role::TextInput` node (not literal synthetic keystrokes) is the
/// same "drive the real widget, don't fight cursor/selection mechanics"
/// approach `kittest_viewer.rs`'s Esc/Enter-in-search-bar tests use — safe
/// here because the field starts empty, so there is no prefilled text/cursor
/// state to fight (unlike `kittest_actions.rs`'s Rename dialog, which avoids
/// simulated typing for exactly that reason).
#[test]
fn custom_url_field_persists_on_lost_focus_not_per_keystroke() {
    let root = tempfile::tempdir().expect("model root tempdir");
    let settings_path = root.path().join("settings.toml");
    let state = ModelsTabState::new(root.path(), None, false);

    let fixture = ModelsFixture {
        state,
        root: root.path().to_path_buf(),
        settings: Arc::new(Mutex::new(Settings::default())),
        settings_path: settings_path.clone(),
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

    harness.get(By::new().role(Role::TextInput)).focus();
    harness.run(); // the focus request takes effect

    harness
        .get(By::new().role(Role::TextInput))
        .type_text("https://example.com/custom.bin");
    harness.run();

    assert!(
        !settings_path.exists(),
        "typing into the field must not persist per keystroke"
    );

    // Tab away: a singleline `TextEdit` surrenders focus (same path Enter
    // takes), which must now commit.
    harness.key_press(egui::Key::Tab);
    harness.run();

    assert!(
        settings_path.exists(),
        "losing focus must persist the field"
    );
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.models.custom_model_url.as_deref(),
        Some("https://example.com/custom.bin")
    );
}

// -- Models tab: measured-speed badge (J-9 ✚, design spec §8.2) -------------

fn models_harness(fixture: ModelsFixture) -> Harness<'static, ModelsFixture> {
    Harness::new_ui_state(
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
    )
}

#[test]
fn models_tab_shows_speed_badge_when_measured() {
    let root = tempfile::tempdir().expect("model root tempdir");
    let mut settings = Settings::default();
    settings
        .recording
        .measured_speed
        .insert("tiny".to_string(), 2.5);

    let fixture = ModelsFixture {
        state: ModelsTabState::new(root.path(), None, false),
        root: root.path().to_path_buf(),
        settings: Arc::new(Mutex::new(settings)),
        settings_path: root.path().join("unused-settings.toml"),
    };

    let mut harness = models_harness(fixture);
    harness.run();

    // Two separate `contains` checks, not one spanning string: Fluent's
    // default `use_isolating` wraps the interpolated `{ $ratio }` value in
    // invisible bidi-isolation marks, so "2.5" and the literal "× realtime"
    // that follows it in the catalog are not byte-adjacent in the rendered
    // label.
    harness.get_by_label_contains("2.5");
    harness.get_by_label_contains("realtime");
}

#[test]
fn models_tab_shows_no_badge_without_a_measured_entry() {
    let root = tempfile::tempdir().expect("model root tempdir");
    let fixture = ModelsFixture {
        state: ModelsTabState::new(root.path(), None, false),
        root: root.path().to_path_buf(),
        settings: Arc::new(Mutex::new(Settings::default())),
        settings_path: root.path().join("unused-settings.toml"),
    };

    let mut harness = models_harness(fixture);
    harness.run();

    assert!(
        harness.query_by_label_contains("realtime").is_none(),
        "a fresh install (empty measured_speed) must render no badge at all"
    );
}

// -- Models tab: Lumii row gated on the lv_LV locale (F-044, design spec §8.2) --

#[test]
fn models_tab_hides_lumii_without_lv_locale() {
    let root = tempfile::tempdir().expect("model root tempdir");
    let settings = Settings::default();
    let include_lumii = i18n::lumii_locale_requested(&settings, None);
    assert!(
        !include_lumii,
        "premise: en-locale settings never gate Lumii open"
    );

    let fixture = ModelsFixture {
        state: ModelsTabState::new(root.path(), None, include_lumii),
        root: root.path().to_path_buf(),
        settings: Arc::new(Mutex::new(settings)),
        settings_path: root.path().join("unused-settings.toml"),
    };

    let mut harness = models_harness(fixture);
    harness.run();

    assert!(harness.query_by_label_contains("Lumii").is_none());
}

#[test]
fn models_tab_shows_lumii_under_lv_locale() {
    let root = tempfile::tempdir().expect("model root tempdir");
    let mut settings = Settings::default();
    settings.ui.locale = Some("lv_LV".to_string());
    let include_lumii = i18n::lumii_locale_requested(&settings, None);
    assert!(
        include_lumii,
        "premise: an explicit lv_LV setting gates Lumii open"
    );

    let fixture = ModelsFixture {
        state: ModelsTabState::new(root.path(), None, include_lumii),
        root: root.path().to_path_buf(),
        settings: Arc::new(Mutex::new(settings)),
        settings_path: root.path().join("unused-settings.toml"),
    };

    let mut harness = models_harness(fixture);
    harness.run();

    harness.get_by_label_contains("Lumii");
}

// -- Toolbar update chip: appears once an Available notice lands -------------

#[test]
fn update_chip_appears_when_available_notice_is_injected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let services = services::init_with(TestDeps {
        store,
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
    // `Services::update_tx` is exposed exactly so a test can drive this
    // hermetically instead of racing a real network check.
    let update_tx = services.update_tx.clone();

    // `HarnessBuilder::default()` + `.with_max_steps` rather than the
    // shorthand `Harness::new_eframe` (which is just that builder with the
    // default `max_steps` of 4): a real `HummApp` here is backed by
    // `init_with`'s live queue/event-pump thread, which can occasionally
    // need a couple of extra `Harness::run` steps to settle under
    // event-pump churn (final-review fix 5, same rationale as
    // `kittest_import.rs`'s `harness_with`).
    let mut harness = HarnessBuilder::default()
        .with_max_steps(32)
        .build_eframe(move |_cc| HummApp::new_for_test(services));
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
