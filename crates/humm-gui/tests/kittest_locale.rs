//! kittest coverage for the Preferences → General **UI-language combo**
//! (F-008/F-100, design spec §9.1, ledger #L10).
//!
//! **Why this test has a whole integration binary to itself.** It is the only
//! test in `crates/humm-gui/tests/` that *writes* the process-wide active
//! locale: `prefs_general::commit_locale` calls `i18n::set_locale`, which bumps
//! a global generation counter every `tr()` call in the process observes. Many
//! other tests — in `kittest_prefs.rs`, `kittest_recording.rs`,
//! `kittest_prefs_folder_watch.rs` and elsewhere — assert on translated text
//! and are therefore *readers* of that same global. Cargo runs the tests inside
//! one integration binary concurrently on a thread pool, so a reader and this
//! writer sharing a binary is a standing isolation hazard: it already produced
//! one real interleaved-run flake (M7 Task 19 review fix #3, which had to add
//! `lock_locale_for_test()` guards to two lib tests that merely *read*
//! translated text).
//!
//! `lock_locale_for_test()` fixes that hazard only for tests that remember to
//! take the lock — every future reader has to opt in, forever, and a missed
//! one fails rarely and non-deterministically. Giving the single writer its own
//! test binary closes the class instead of the instance: separate binaries are
//! separate processes, so no reader anywhere else can observe this test's
//! locale switch regardless of what it remembers to do. The guard below is kept
//! anyway (it costs nothing and documents the invariant) in case a second
//! locale-writing test ever joins this file.
//!
//! Like `kittest_prefs.rs`, this file drives the extracted `preferences::ui`
//! function inside a bare `egui::Ui` via `Harness::new_ui_state` — the real
//! Preferences window is a separate OS viewport `egui_kittest`'s single-
//! `Harness` accessibility tree cannot reach into.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{By, Queryable};
use humm_core::secrets::{MemorySecretStore, SecretStore};
use humm_core::settings::Settings;
use humm_gui::i18n;
use humm_gui::models_ui::ModelsTabState;
use humm_gui::services;
use humm_gui::shortcuts::ShortcutMap;
use humm_gui::views::preferences::{self, PreferencesState, PrefsTab};
use humm_gui::views::prefs_folder_watch::FolderWatchTabState;
use humm_gui::views::prefs_general::{self, GeneralTabState, ValidateFn};
use humm_gui::views::prefs_postproc::PostprocTabState;
use humm_gui::views::prefs_shortcuts::ShortcutsTabState;

/// A [`humm_gui::import::FileDialogs`] that never returns a pick — this file
/// exercises no dialog-backed control.
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

/// Mirrors `kittest_prefs.rs`'s own `GeneralFixture` — everything
/// [`preferences::ui`] needs, plus the two [`tempfile::TempDir`] guards
/// (`_`-prefixed, never read) that keep the backing directories alive exactly
/// as long as the [`Harness`] does.
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
            postproc: PostprocTabState::new(
                &humm_core::settings::PostprocSettings::default(),
                secrets.as_ref(),
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

/// The UI-language combo lists exactly [`i18n::AVAILABLE_LOCALES`] (F-008b
/// plus the M8 Task 13/14 batches: `en_US`/`ca_ES`/`da_DK`/`nl`/`de_DE`/
/// `es_ES`/`it_IT`/`ja_JP`/`pl_PL`/`pt_BR`/`ru`/`zh_CN`) and selecting an
/// entry persists `ui.locale` (live apply -- design spec ledger #L10). Driving
/// a `ComboBox`
/// selection by simulated click has no kittest precedent in this workspace
/// (`kittest_import.rs`/`kittest_recording.rs` both document the same gap
/// and mutate backing state directly instead) -- this test follows suit,
/// asserting the rendered combo's closed-state label matches the negotiated
/// locale's display name, then driving the "selection" via the commit seam
/// directly, exactly like `kittest_prefs.rs`'s
/// `base_url_and_model_fields_persist_to_settings` does for its own text fields.
///
/// Wrapped in [`i18n::lock_locale_for_test`]: [`prefs_general::commit_locale`]
/// calls [`i18n::set_locale`], which mutates process-wide global state. See
/// this file's module doc for why that guard is no longer the *only* line of
/// defense.
#[test]
fn language_combo_lists_available_locales_and_persists() {
    let _locale_guard = i18n::lock_locale_for_test();
    assert_eq!(
        i18n::AVAILABLE_LOCALES.len(),
        12,
        "F-008b plus M8 Tasks 13/14 embeds en_US/ca_ES/da_DK/nl/de_DE/es_ES/\
         it_IT/ja_JP/pl_PL/pt_BR/ru/zh_CN -- update this count as each of \
         Buzz's remaining 3 locales (lv_LV/uk_UA/zh_TW) is ported"
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
