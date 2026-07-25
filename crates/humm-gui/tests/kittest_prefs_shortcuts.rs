//! kittest coverage for Task 14 (F-005 editor + ✚ conflict detection, design
//! spec §8.3): row rendering, the click-to-capture flow (a bound key,
//! Esc-cancels), conflict captions, and Reset to Defaults — plus one
//! app-level test proving `settings.shortcuts` overrides are actually seeded
//! into `HummApp`'s resolved map at startup.
//!
//! Per `views/preferences.rs`'s own module doc, the Preferences window is a
//! real OS viewport `egui_kittest`'s single-`Harness` accessibility tree
//! cannot reach into — so, like `kittest_prefs.rs`'s General/Privacy
//! coverage, this file drives `views::prefs_shortcuts::ui` directly inside a
//! bare `egui::Ui` via `Harness::new_ui_state`, mirroring exactly the
//! generation-check-then-rebuild loop `app.rs::HummApp::
//! rebuild_shortcuts_if_generation_changed` runs for real, so this fixture's
//! own `current_shortcuts` behaves the same way the production one does.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, HarnessBuilder};
use humm_core::settings::Settings;
use humm_gui::app::HummApp;
use humm_gui::i18n::{tr, tr_args};
use humm_gui::services::{self, Capabilities, TestDeps};
use humm_gui::shortcuts::{ShortcutAction, ShortcutMap, parse_sequence};
use humm_gui::views::prefs_shortcuts::{self, ShortcutsTabState};
use humm_store::SqliteStore;

/// The conflict caption's literal (non-placeable) text, with Fluent's
/// `use_isolating` bidi-isolation marks (U+2066-U+2069, invisible) stripped
/// out — `tr_args`-ing the key with an empty `other` value leaves those
/// marks behind even after `.trim()` (they aren't ASCII/Unicode whitespace),
/// so a plain `contains` check against the untouched result would silently
/// never match the real rendered caption (which has actual description text
/// between the marks, not nothing).
fn conflict_caption_prefix() -> String {
    tr_args("prefs-shortcuts-conflict", &[("other", "")])
        .replace(['\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'], "")
        .trim()
        .to_string()
}

/// The formatted display text [`prefs_shortcuts::ui`] would show for
/// `sequence` under this file's harnesses — every `Harness::new_ui_state`
/// fixture runs with `egui::os::OperatingSystem::Nix`
/// (`HarnessBuilder::default()`'s own documented default), independent of
/// the host machine actually running the test, so hardcoding that OS here
/// keeps this helper correct on every CI platform.
fn display_text(sequence: &str) -> String {
    let shortcut = parse_sequence(sequence).expect("a valid Buzz-style sequence");
    let ctx = egui::Context::default();
    ctx.set_os(egui::os::OperatingSystem::Nix);
    ctx.format_shortcut(&shortcut)
}

/// The Shortcuts tab's own render-loop fixture. `last_generation` mirrors
/// `HummApp::last_shortcuts_generation`: the closure below checks it against
/// `shortcuts_generation` every step and rebuilds `current_shortcuts` from
/// `settings.shortcuts` exactly the way `app.rs`'s real per-frame rebuild
/// does, so this fixture's live re-apply behaves identically to production
/// (module doc).
struct ShortcutsFixture {
    state: ShortcutsTabState,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    shortcuts_generation: Arc<AtomicUsize>,
    current_shortcuts: ShortcutMap,
    last_generation: usize,
    _settings_dir: tempfile::TempDir,
}

/// Builds a harness seeded from `settings` (already containing whatever
/// `[shortcuts]` overrides the test wants, if any). Sized well beyond the
/// `HarnessBuilder` default (800×600, `builder.rs`'s own doc): the 20
/// rows + Reset button run past 600px, and a widget positioned outside the
/// harness's `screen_rect` never registers as hovered/clicked (the same
/// `.with_size(...)` fix `kittest_import.rs`/`kittest_actions.rs`/
/// `kittest_recording.rs` all apply for their own tall content).
fn shortcuts_harness(settings: Settings) -> Harness<'static, ShortcutsFixture> {
    let settings_dir = tempfile::tempdir().expect("settings tempdir");
    let settings_path = settings_dir.path().join("settings.toml");
    let current_shortcuts = ShortcutMap::from_settings(&settings.shortcuts);

    let fixture = ShortcutsFixture {
        state: ShortcutsTabState::default(),
        settings: Arc::new(Mutex::new(settings)),
        settings_path,
        shortcuts_generation: Arc::new(AtomicUsize::new(0)),
        current_shortcuts,
        last_generation: 0,
        _settings_dir: settings_dir,
    };

    HarnessBuilder::default()
        .with_size(egui::Vec2::new(700.0, 1200.0))
        .build_ui_state(
            |ui, fixture: &mut ShortcutsFixture| {
                let generation = fixture.shortcuts_generation.load(Ordering::Relaxed);
                if generation != fixture.last_generation {
                    fixture.last_generation = generation;
                    let overrides = fixture
                        .settings
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .shortcuts
                        .clone();
                    fixture.current_shortcuts = ShortcutMap::from_settings(&overrides);
                }
                prefs_shortcuts::ui(
                    ui,
                    &mut fixture.state,
                    &fixture.settings,
                    &fixture.settings_path,
                    &fixture.shortcuts_generation,
                    &fixture.current_shortcuts,
                );
            },
            fixture,
        )
}

// -- Row rendering -------------------------------------------------------

#[test]
fn all_twenty_rows_render_their_description_label_and_default_sequence() {
    let mut harness = shortcuts_harness(Settings::default());
    harness.run();

    for action in ShortcutAction::ALL {
        harness.get_by_label(&tr(action.description_key()));
        harness.get_by_label(&display_text(action.default_sequence()));
    }
}

// -- Capture flow ---------------------------------------------------------

#[test]
fn clicking_a_field_shows_the_press_prompt() {
    let mut harness = shortcuts_harness(Settings::default());
    harness.run();

    harness
        .get_by_label(&display_text(
            ShortcutAction::OpenImportWindow.default_sequence(),
        ))
        .click();
    harness.run();

    harness.get_by_label(&tr("prefs-shortcuts-press"));
    assert_eq!(
        harness.state().state.capturing,
        Some(ShortcutAction::OpenImportWindow)
    );
}

#[test]
fn a_key_press_with_modifiers_persists_the_override_bumps_generation_and_shows_the_new_sequence() {
    let mut harness = shortcuts_harness(Settings::default());
    harness.run();

    harness
        .get_by_label(&display_text(
            ShortcutAction::OpenImportWindow.default_sequence(),
        ))
        .click();
    harness.run();

    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::I,
    );
    harness.run();

    assert_eq!(
        harness.state().shortcuts_generation.load(Ordering::Relaxed),
        1,
        "a resolved capture must bump the generation counter exactly once"
    );
    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded
            .shortcuts
            .get(ShortcutAction::OpenImportWindow.settings_key()),
        Some(&"Ctrl+Shift+I".to_string())
    );
    assert_eq!(
        harness.state().state.capturing,
        None,
        "capture must end once a binding resolves"
    );
    harness.get_by_label(&display_text("Ctrl+Shift+I"));
}

#[test]
fn escape_cancels_capture_without_binding_anything() {
    let mut harness = shortcuts_harness(Settings::default());
    harness.run();

    harness
        .get_by_label(&display_text(
            ShortcutAction::OpenImportWindow.default_sequence(),
        ))
        .click();
    harness.run();

    harness.key_press(egui::Key::Escape);
    harness.run();

    assert_eq!(
        harness.state().state.capturing,
        None,
        "Esc must cancel capture"
    );
    let settings_path = harness.state().settings_path.clone();
    assert!(
        !settings_path.exists(),
        "Esc must not write anything -- no commit ever happened"
    );
    // The field must show the original default again, not a stray "Press
    // shortcut..." prompt left over from the canceled capture.
    harness.get_by_label(&display_text(
        ShortcutAction::OpenImportWindow.default_sequence(),
    ));
}

// -- Conflict detection ✚ ---------------------------------------------------

#[test]
fn conflict_caption_appears_on_both_rows_sharing_a_sequence() {
    let mut settings = Settings::default();
    // Force OpenImportWindow onto ClearHistory's own default sequence.
    settings.shortcuts.insert(
        ShortcutAction::OpenImportWindow.settings_key().to_string(),
        ShortcutAction::ClearHistory.default_sequence().to_string(),
    );
    let mut harness = shortcuts_harness(settings);
    harness.run();

    // Each row's caption names the *other* side of the pair, so the two
    // rendered captions are distinct strings (unlike a shared "Conflicts
    // with" prefix or either bare description label alone, both of which
    // also appear elsewhere on the tab -- the row's own label, or the
    // opposite row's caption -- so `get_by_label`'s exact-match, not a
    // `contains` search, is what keeps this test unambiguous).
    let caption_on_open_import_window_row = tr_args(
        "prefs-shortcuts-conflict",
        &[("other", &tr(ShortcutAction::ClearHistory.description_key()))],
    );
    let caption_on_clear_history_row = tr_args(
        "prefs-shortcuts-conflict",
        &[(
            "other",
            &tr(ShortcutAction::OpenImportWindow.description_key()),
        )],
    );
    assert_ne!(
        caption_on_open_import_window_row, caption_on_clear_history_row,
        "premise: the two rows' captions must differ (each names the other side)"
    );
    harness.get_by_label(&caption_on_open_import_window_row);
    harness.get_by_label(&caption_on_clear_history_row);
}

#[test]
fn no_conflict_caption_on_defaults() {
    let mut harness = shortcuts_harness(Settings::default());
    harness.run();

    let caption_prefix = conflict_caption_prefix();
    assert!(
        harness.query_by_label_contains(&caption_prefix).is_none(),
        "Buzz's 20 conflict-free defaults must render no conflict caption at all"
    );
}

// -- Reset to Defaults ------------------------------------------------------

#[test]
fn reset_to_defaults_clears_overrides_and_rows_show_defaults() {
    let mut settings = Settings::default();
    settings.shortcuts.insert(
        ShortcutAction::OpenImportWindow.settings_key().to_string(),
        "Ctrl+Shift+I".to_string(),
    );
    let mut harness = shortcuts_harness(settings);
    harness.run();

    harness.get_by_label(&tr("prefs-shortcuts-reset")).click();
    harness.run();

    assert_eq!(
        harness.state().shortcuts_generation.load(Ordering::Relaxed),
        1
    );
    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(
        reloaded.shortcuts.is_empty(),
        "Reset to Defaults must clear every override"
    );
    harness.get_by_label(&display_text(
        ShortcutAction::OpenImportWindow.default_sequence(),
    ));
}

// -- Per-row clear -----------------------------------------------------------

#[test]
fn per_row_clear_restores_only_that_actions_default() {
    let mut settings = Settings::default();
    settings.shortcuts.insert(
        ShortcutAction::OpenImportWindow.settings_key().to_string(),
        "Ctrl+Shift+I".to_string(),
    );
    settings.shortcuts.insert(
        ShortcutAction::ClearHistory.settings_key().to_string(),
        "Ctrl+Shift+S".to_string(),
    );
    let mut harness = shortcuts_harness(settings);
    harness.run();

    // The per-row clear button's tooltip text doubles as its accessible
    // label under accesskit for an icon-only button -- unique per row is not
    // needed here since we only assert on settings after clicking the
    // *first* one encountered would be ambiguous, so instead drive the
    // pure seam directly for this row-scoped assertion (mirrors
    // `prefs_shortcuts::tests::commit_clear_removes_only_that_actions_override`,
    // proven again here through the render loop's own generation bump).
    {
        let fixture = harness.state();
        prefs_shortcuts::commit_clear(
            &fixture.settings,
            &fixture.settings_path,
            &fixture.shortcuts_generation,
            ShortcutAction::OpenImportWindow,
        );
    }
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(
        !reloaded
            .shortcuts
            .contains_key(ShortcutAction::OpenImportWindow.settings_key())
    );
    assert_eq!(
        reloaded
            .shortcuts
            .get(ShortcutAction::ClearHistory.settings_key()),
        Some(&"Ctrl+Shift+S".to_string()),
        "clearing one row must not disturb another action's override"
    );
    harness.get_by_label(&display_text(
        ShortcutAction::OpenImportWindow.default_sequence(),
    ));
    harness.get_by_label(&display_text("Ctrl+Shift+S"));
}

// -- App-level: startup seeds overrides from settings ------------------------

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

#[test]
fn startup_reads_shortcut_overrides_from_settings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut settings = Settings::default();
    settings.shortcuts.insert(
        ShortcutAction::OpenImportWindow.settings_key().to_string(),
        "Ctrl+Shift+I".to_string(),
    );
    let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
    let services = services::init_with(TestDeps {
        store,
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

    let app = HummApp::new_for_test(services);
    let map = app.shortcuts_for_test();

    assert_eq!(
        map.get(ShortcutAction::OpenImportWindow),
        parse_sequence("Ctrl+Shift+I").expect("parses"),
        "HummApp::new_for_test must seed the resolved map from settings.shortcuts, \
         not always the Buzz defaults"
    );
    // Every other action is untouched -- still its own Buzz default.
    assert_eq!(
        map.get(ShortcutAction::ClearHistory),
        parse_sequence(ShortcutAction::ClearHistory.default_sequence()).unwrap()
    );
}
