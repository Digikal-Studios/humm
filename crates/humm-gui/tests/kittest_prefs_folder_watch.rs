//! kittest coverage for Task 15 (F-030c, design spec §8.4): the Folder Watch
//! preferences tab -- enable-gates-everything-else, path-field commit on
//! focus-out/Enter, the delete-processed checkbox, the embedded options
//! form's commit, and the Browse-button dialog seam.
//!
//! Per `views/preferences.rs`'s own module doc, the Preferences window is a
//! real OS viewport `egui_kittest`'s single-`Harness` accessibility tree
//! cannot reach into -- so, like `kittest_prefs.rs`/`kittest_prefs_shortcuts.rs`,
//! this file drives `views::prefs_folder_watch::ui` directly inside a bare
//! `egui::Ui` via `Harness::new_ui_state`.
//!
//! `ComboBox` selections (the embedded model-size combo) have no
//! click-driving precedent anywhere in this workspace --
//! `kittest_locale.rs`'s `language_combo_lists_available_locales_and_persists`
//! documents the same gap (`kittest_import.rs`/`kittest_recording.rs` too) --
//! so this file follows the same established convention: assert what the
//! combo renders, then drive persistence via the exposed commit seam
//! directly for that one control, while every checkbox/text-field control
//! is driven through the real rendered widget.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, HarnessBuilder};
use humm_core::domain::ModelSize;
use humm_core::secrets::{MemorySecretStore, SecretStore};
use humm_core::settings::Settings;
use humm_gui::i18n::tr;
use humm_gui::import::FileDialogs;
use humm_gui::views::prefs_folder_watch::{self, FolderWatchTabState};

/// A [`FileDialogs`] that never returns a pick -- every test but
/// `browse_button_uses_dialog_seam` below only needs the tab to render and
/// not panic when a Browse button exists in the tree.
struct NullDialogs;

impl FileDialogs for NullDialogs {
    fn pick_files(&self, _start_dir: Option<&Path>) -> Option<Vec<PathBuf>> {
        None
    }
    fn pick_folder(&self, _start_dir: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn save_file(
        &self,
        _title: &str,
        _default_path: &Path,
        _filter_name: &str,
        _extension: &str,
    ) -> Option<PathBuf> {
        None
    }
}

/// A [`FileDialogs`] that always "picks" the same fixed folder -- the
/// scripted double `browse_button_uses_dialog_seam` needs.
struct FixedFolderDialogs(PathBuf);

impl FileDialogs for FixedFolderDialogs {
    fn pick_files(&self, _start_dir: Option<&Path>) -> Option<Vec<PathBuf>> {
        None
    }
    fn pick_folder(&self, _start_dir: Option<&Path>) -> Option<PathBuf> {
        Some(self.0.clone())
    }
    fn save_file(
        &self,
        _title: &str,
        _default_path: &Path,
        _filter_name: &str,
        _extension: &str,
    ) -> Option<PathBuf> {
        None
    }
}

/// The Folder Watch tab's own render-loop fixture.
struct FolderWatchFixture {
    state: FolderWatchTabState,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    dialogs: Arc<dyn FileDialogs>,
    _settings_dir: tempfile::TempDir,
}

/// Builds a harness seeded from `settings` (already containing whatever
/// `[folder_watch]` values the test wants) and `dialogs` (the Browse-button
/// seam). Sized well beyond the `HarnessBuilder` default: the enable
/// checkbox, two folder rows, the delete checkbox + caption, and the whole
/// embedded options form run well past a default-sized viewport -- the same
/// `.with_size(...)` fix `kittest_prefs_shortcuts.rs`'s own tall content
/// needs.
fn folder_watch_harness(
    settings: Settings,
    dialogs: Arc<dyn FileDialogs>,
) -> Harness<'static, FolderWatchFixture> {
    let settings_dir = tempfile::tempdir().expect("settings tempdir");
    let settings_path = settings_dir.path().join("settings.toml");
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let state = FolderWatchTabState::new(&settings.folder_watch, secrets.as_ref(), false, false);

    let fixture = FolderWatchFixture {
        state,
        settings: Arc::new(Mutex::new(settings)),
        settings_path,
        secrets,
        dialogs,
        _settings_dir: settings_dir,
    };

    HarnessBuilder::default()
        .with_size(egui::Vec2::new(700.0, 1100.0))
        .build_ui_state(
            |ui, fixture: &mut FolderWatchFixture| {
                prefs_folder_watch::ui(
                    ui,
                    &mut fixture.state,
                    &fixture.settings,
                    &fixture.settings_path,
                    &fixture.secrets,
                    fixture.dialogs.as_ref(),
                );
            },
            fixture,
        )
}

// -- Enable checkbox gates everything else (Buzz `_set_settings_enabled`) ---

#[test]
fn enable_checkbox_gates_all_other_fields() {
    let mut harness = folder_watch_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    // Fresh settings start disabled -- every row but the enable checkbox
    // itself must report disabled.
    assert!(
        harness
            .get_by_label("Delete processed files")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        harness
            .get_by_label("Word-level timings")
            .accesskit_node()
            .is_disabled()
    );
    for browse in harness.get_all_by_label("Browse") {
        assert!(
            browse.accesskit_node().is_disabled(),
            "Browse must be disabled"
        );
    }
    for text_input in harness.get_all_by_role(Role::TextInput) {
        assert!(
            text_input.accesskit_node().is_disabled(),
            "every text field must be disabled"
        );
    }
    for combo in harness.get_all_by_role(Role::ComboBox) {
        assert!(
            combo.accesskit_node().is_disabled(),
            "every combo must be disabled"
        );
    }

    harness.get_by_label("Enable folder watch").click();
    harness.run();

    assert!(
        !harness
            .get_by_label("Delete processed files")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !harness
            .get_by_label("Word-level timings")
            .accesskit_node()
            .is_disabled()
    );
    for browse in harness.get_all_by_label("Browse") {
        assert!(
            !browse.accesskit_node().is_disabled(),
            "Browse must be enabled"
        );
    }
    // Only the input/output-folder fields (the first two `TextInput`s in
    // tree order) are gated solely by the outer "Enable folder watch"
    // checkbox -- the embedded LLM block's own AI-model field (the third)
    // has its own *nested* `add_enabled_ui(enable_llm_translation, ..)` gate
    // (`options_sheet::render_llm_translation_block`) and must stay disabled
    // here, since that checkbox is still unchecked. This is correct nested
    // gating, not a bug: egui's disabled flag only ever narrows down a
    // subtree, so re-enabling the outer scope cannot itself re-enable a
    // still-unchecked inner gate.
    let mut text_inputs = harness.get_all_by_role(Role::TextInput);
    assert!(
        !text_inputs
            .next()
            .expect("input-folder field")
            .accesskit_node()
            .is_disabled(),
        "the input-folder field must be enabled"
    );
    assert!(
        !text_inputs
            .next()
            .expect("output-folder field")
            .accesskit_node()
            .is_disabled(),
        "the output-folder field must be enabled"
    );
    assert!(
        text_inputs
            .next()
            .expect("AI-model field")
            .accesskit_node()
            .is_disabled(),
        "the AI-model field must stay disabled -- Enable AI translation is still unchecked"
    );
    // Backend/model-size/task/language: gated solely by the outer checkbox,
    // must now be enabled. The fifth and last `ComboBox` in tree order is
    // the embedded LLM block's own AI-model presets picker
    // (`render_llm_model_prompt_fields`'s inner `ComboBox`) -- same nested
    // `enable_llm_translation` gate as the AI-model text field above, so it
    // must stay disabled here too.
    let mut combos = harness.get_all_by_role(Role::ComboBox);
    for label in ["backend", "model-size", "task", "language"] {
        assert!(
            !combos
                .next()
                .unwrap_or_else(|| panic!("{label} combo"))
                .accesskit_node()
                .is_disabled(),
            "the {label} combo must be enabled"
        );
    }
    assert!(
        combos
            .next()
            .expect("AI-model presets combo")
            .accesskit_node()
            .is_disabled(),
        "the AI-model presets combo must stay disabled -- Enable AI translation is still unchecked"
    );
}

// -- Path fields: commit on focus-out/Enter, empty -> None ------------------

#[test]
fn input_folder_commit_persists() {
    let mut settings = Settings::default();
    settings.folder_watch.enabled = true;
    let mut harness = folder_watch_harness(settings, Arc::new(NullDialogs));
    harness.run();

    // The input-folder field is the first `Role::TextInput` in tree order
    // (enable checkbox has no text field; input row renders before output
    // row, which renders before the embedded form's own AI-model field).
    harness
        .get_all_by_role(Role::TextInput)
        .next()
        .expect("input-folder field")
        .focus();
    harness.run();
    harness
        .get_all_by_role(Role::TextInput)
        .next()
        .expect("input-folder field")
        .type_text("/watch/in");
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    assert!(
        !settings_path.exists(),
        "typing into the field must not persist per keystroke"
    );

    harness.key_press(egui::Key::Tab);
    harness.run();

    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.folder_watch.input_dir,
        Some(PathBuf::from("/watch/in"))
    );
}

#[test]
fn output_folder_commit_persists() {
    let mut settings = Settings::default();
    settings.folder_watch.enabled = true;
    let mut harness = folder_watch_harness(settings, Arc::new(NullDialogs));
    harness.run();

    // The output-folder field is the second `Role::TextInput` in tree order.
    harness
        .get_all_by_role(Role::TextInput)
        .nth(1)
        .expect("output-folder field")
        .focus();
    harness.run();
    harness
        .get_all_by_role(Role::TextInput)
        .nth(1)
        .expect("output-folder field")
        .type_text("/watch/out");
    harness.run();
    harness.key_press(egui::Key::Tab);
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.folder_watch.output_dir,
        Some(PathBuf::from("/watch/out"))
    );
}

/// Emptying a previously-set path field commits `None`, not an empty
/// string. Widget-level clearing (select-all + delete) has no precedent in
/// this workspace's kittest suite for any field -- `kittest_prefs.rs`'s own
/// `export_template_field_persists_empty_as_none` proves the identical
/// "emptied field -> None" behavior via the exposed commit seam directly
/// rather than simulated backspacing, and this test follows the same
/// convention (still through a live `Harness` fixture, so the tab's
/// `settings`/`settings_path` wiring is exercised end to end).
#[test]
fn empty_path_commits_none() {
    let mut settings = Settings::default();
    settings.folder_watch.enabled = true;
    settings.folder_watch.input_dir = Some(PathBuf::from("/previously/set"));
    let mut harness = folder_watch_harness(settings, Arc::new(NullDialogs));
    harness.run();

    assert_eq!(
        harness.state().state.input_dir_buffer,
        "/previously/set",
        "the buffer must seed from the existing setting"
    );

    {
        let fixture = harness.state_mut();
        fixture.state.input_dir_buffer = String::new();
        prefs_folder_watch::commit_input_dir(
            &fixture.state,
            &fixture.settings,
            &fixture.settings_path,
        );
    }

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.folder_watch.input_dir, None,
        "an emptied field must clear the setting to None, not persist an empty string"
    );
}

// -- Delete-processed checkbox -----------------------------------------------

#[test]
fn delete_checkbox_persists() {
    let mut settings = Settings::default();
    settings.folder_watch.enabled = true;
    let mut harness = folder_watch_harness(settings, Arc::new(NullDialogs));
    harness.run();

    harness.get_by_label("Delete processed files").click();
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(reloaded.folder_watch.delete_processed_files);

    harness.get_by_label("Delete processed files").click();
    harness.run();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(!reloaded.folder_watch.delete_processed_files);
}

// -- Embedded options form: model/checkbox edits persist --------------------

#[test]
fn embedded_options_model_change_persists_into_folder_watch_options() {
    let mut settings = Settings::default();
    settings.folder_watch.enabled = true;
    let mut harness = folder_watch_harness(settings, Arc::new(NullDialogs));
    harness.run();

    // Word-level timings + one export-format checkbox: real click-driven,
    // proving `prefs_folder_watch::ui`'s own before/after diff commits the
    // whole embedded form when a real widget click changes it (not merely
    // `SheetState::write_to` in isolation, already unit-tested in
    // `prefs_folder_watch.rs`'s own test module).
    harness.get_by_label("Word-level timings").click();
    harness.run();
    harness.get_by_label("TXT").click();
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(reloaded.folder_watch.options.word_timestamps, Some(true));
    assert_eq!(
        reloaded.folder_watch.options.export_formats,
        Some(vec!["txt".to_string()])
    );

    // Model-size combo: no `ComboBox` click-driving precedent anywhere in
    // this workspace (module doc) -- drive the commit seam directly.
    {
        let fixture = harness.state_mut();
        fixture.state.sheet.model_size = ModelSize::Medium;
        prefs_folder_watch::commit_options(
            &fixture.state,
            &fixture.settings,
            &fixture.settings_path,
        );
    }
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(
        reloaded.folder_watch.options.model_size.as_deref(),
        Some("medium")
    );
}

// -- Browse buttons: the FileDialogs seam ------------------------------------

#[test]
fn browse_button_uses_dialog_seam() {
    let mut settings = Settings::default();
    settings.folder_watch.enabled = true;
    let picked = PathBuf::from("/picked/input/folder");
    let dialogs: Arc<dyn FileDialogs> = Arc::new(FixedFolderDialogs(picked.clone()));
    let mut harness = folder_watch_harness(settings, dialogs);
    harness.run();

    // Input folder's Browse button is the first "Browse" node in tree order.
    harness
        .get_all_by_label("Browse")
        .next()
        .expect("input-folder Browse button")
        .click();
    harness.run();

    assert_eq!(
        harness.state().state.input_dir_buffer,
        picked.display().to_string()
    );
    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(reloaded.folder_watch.input_dir, Some(picked));
}

#[test]
fn output_folder_browse_button_uses_dialog_seam() {
    let mut settings = Settings::default();
    settings.folder_watch.enabled = true;
    let picked = PathBuf::from("/picked/output/folder");
    let dialogs: Arc<dyn FileDialogs> = Arc::new(FixedFolderDialogs(picked.clone()));
    let mut harness = folder_watch_harness(settings, dialogs);
    harness.run();

    // Output folder's Browse button is the second "Browse" node in tree order.
    harness
        .get_all_by_label("Browse")
        .nth(1)
        .expect("output-folder Browse button")
        .click();
    harness.run();

    assert_eq!(
        harness.state().state.output_dir_buffer,
        picked.display().to_string()
    );
    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(reloaded.folder_watch.output_dir, Some(picked));
}

// -- Disposition caption: static, presence-only (spec §8.4/§3.4) ------------

#[test]
fn disposition_note_is_present() {
    let mut harness = folder_watch_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    harness.get_by_label(&tr("prefs-fw-disposition-note"));
}
