//! kittest coverage for Task 16 (F-110/F-111/F-113/F-115/F-116 config home,
//! design spec §8.5): the Post-processing preferences tab -- one
//! enable-checkbox-plus-representative-field persistence test per section
//! (Skip Already Transcribed, Automatic Transcript Resizer, AI Summary,
//! Export to DOCX), the AI Summary key row's keyring-not-settings proof, and
//! the Browse-button dialog seam for the two folder fields.
//!
//! Per `views/preferences.rs`'s own module doc, the Preferences window is a
//! real OS viewport `egui_kittest`'s single-`Harness` accessibility tree
//! cannot reach into -- so, like `kittest_prefs_folder_watch.rs`, this file
//! drives `views::prefs_postproc::ui` directly inside a bare `egui::Ui` via
//! `Harness::new_ui_state`.
//!
//! Text fields are driven through the exposed `commit_*` seam rather than
//! simulated typing -- the same precedent `kittest_prefs.rs`'s own
//! `base_url_and_model_fields_persist_to_settings`/
//! `export_template_field_persists_empty_as_none` set for the General tab
//! once it grew several single-line text inputs (a bare `Role::TextInput`
//! query becomes ambiguous with more than one on screen, and this tab has
//! seven). Checkboxes, having unique labels, are driven through the real
//! rendered widget.
//!
//! The six-tab strip assembly test (F-101) lives in `kittest_prefs.rs`
//! (`all_six_tabs_are_selectable_and_render`), alongside the other
//! whole-window Preferences tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, HarnessBuilder};
use humm_core::secrets::{MemorySecretStore, SecretKey, SecretStore};
use humm_core::settings::Settings;
use humm_gui::import::FileDialogs;
use humm_gui::views::prefs_postproc::{self, PostprocTabState};

/// A [`FileDialogs`] that never returns a pick -- every test but the
/// Browse-seam one below only needs the tab to render and not panic when a
/// Browse button exists in the tree.
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

/// The Post-processing tab's own render-loop fixture.
struct PostprocFixture {
    state: PostprocTabState,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    dialogs: Arc<dyn FileDialogs>,
    _settings_dir: tempfile::TempDir,
}

/// Builds a harness seeded from `settings` (already containing whatever
/// `[postproc]` values the test wants) and `dialogs` (the Browse-button
/// seam). Sized well beyond the default: four collapsible sections' worth of
/// checkboxes/fields run well past a default-sized viewport -- the same
/// `.with_size(...)` fix `kittest_prefs_folder_watch.rs`'s own tall content
/// needs.
fn postproc_harness(
    settings: Settings,
    dialogs: Arc<dyn FileDialogs>,
) -> Harness<'static, PostprocFixture> {
    let settings_dir = tempfile::tempdir().expect("settings tempdir");
    let settings_path = settings_dir.path().join("settings.toml");
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let state = PostprocTabState::new(&settings.postproc, secrets.as_ref());

    let fixture = PostprocFixture {
        state,
        settings: Arc::new(Mutex::new(settings)),
        settings_path,
        secrets,
        dialogs,
        _settings_dir: settings_dir,
    };

    HarnessBuilder::default()
        .with_size(egui::Vec2::new(700.0, 1400.0))
        .build_ui_state(
            |ui, fixture: &mut PostprocFixture| {
                prefs_postproc::ui(
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

// -- Skip Already Transcribed (F-115) ----------------------------------------

#[test]
fn skip_section_enable_and_check_database_persist() {
    let mut harness = postproc_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    harness
        .get_by_label("Enable Skip Already Transcribed")
        .click();
    harness.run();
    harness
        .get_by_label("Check in transcription database")
        .click();
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(reloaded.postproc.skip_already_transcribed.enabled);
    assert!(reloaded.postproc.skip_already_transcribed.check_database);
}

// -- Automatic Transcript Resizer (F-116) ------------------------------------

#[test]
fn resizer_section_enable_and_merge_gap_seconds_persist() {
    let mut harness = postproc_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    harness
        .get_by_label("Enable Automatic Transcript Resizer")
        .click();
    harness.run();

    {
        let fixture = harness.state_mut();
        fixture.state.merge_gap_seconds_buffer = "0.75".to_string();
        prefs_postproc::commit_merge_gap_seconds(
            &fixture.state,
            &fixture.settings,
            &fixture.settings_path,
        );
    }

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(reloaded.postproc.transcript_resizer.enabled);
    assert_eq!(
        reloaded.postproc.transcript_resizer.merge_gap_seconds,
        "0.75"
    );
}

// -- AI Summary (F-111) ------------------------------------------------------

#[test]
fn summary_section_enable_and_output_folder_persist() {
    let mut harness = postproc_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    harness.get_by_label("Enable AI Summary").click();
    harness.run();

    {
        let fixture = harness.state_mut();
        fixture.state.summary_output_folder_buffer = "/summaries".to_string();
        prefs_postproc::commit_summary_output_folder(
            &fixture.state,
            &fixture.settings,
            &fixture.settings_path,
        );
    }

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(reloaded.postproc.ai_summary.enabled);
    assert_eq!(
        reloaded.postproc.ai_summary.output_folder,
        Some(PathBuf::from("/summaries"))
    );
}

/// The load-bearing key-hygiene test (task brief): committing the AI Summary
/// key field writes the [`MemorySecretStore`] double under
/// [`SecretKey::AiSummaryApiKey`] -- never `settings.toml`, and never even
/// touches the `settings` lock at all.
#[test]
fn summary_key_writes_the_secret_store_not_settings() {
    let mut harness = postproc_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    {
        let fixture = harness.state_mut();
        fixture.state.summary_key_buffer = "sk-summary-secret".to_string();
        prefs_postproc::commit_summary_key(&mut fixture.state, &fixture.secrets);
    }

    let secrets = harness.state().secrets.clone();
    assert_eq!(
        secrets.get(SecretKey::AiSummaryApiKey).unwrap(),
        Some("sk-summary-secret".to_string())
    );
    assert_eq!(harness.state().state.modal, None);

    // Never persisted to disk: no commit path from the key field into
    // `Settings` exists at all, and the settings file was never even
    // written by this test (mirrors
    // `kittest_prefs_folder_watch.rs`'s own `commit_options_never_writes_
    // the_api_key_anywhere`, one step stronger -- here the file shouldn't
    // exist at all).
    let settings_path = harness.state().settings_path.clone();
    assert!(
        !settings_path.exists(),
        "committing only the key field must never touch settings.toml"
    );
}

// -- Export to DOCX (F-113) --------------------------------------------------

#[test]
fn docx_section_enable_and_include_timestamps_persist() {
    let mut harness = postproc_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    harness.get_by_label("Enable Export to DOCX").click();
    harness.run();
    harness.get_by_label("Include timestamps").click();
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert!(reloaded.postproc.export_docx.enabled);
    assert!(reloaded.postproc.export_docx.include_timestamps);
}

// -- Gating: fields disabled until their section is enabled ------------------

#[test]
fn browse_buttons_disabled_until_their_section_is_enabled() {
    let mut harness = postproc_harness(Settings::default(), Arc::new(NullDialogs));
    harness.run();

    for browse in harness.get_all_by_label("Browse") {
        assert!(
            browse.accesskit_node().is_disabled(),
            "Browse must be disabled while its section is off"
        );
    }

    harness.get_by_label("Enable AI Summary").click();
    harness.run();
    harness.get_by_label("Enable Export to DOCX").click();
    harness.run();

    for browse in harness.get_all_by_label("Browse") {
        assert!(
            !browse.accesskit_node().is_disabled(),
            "Browse must be enabled once its section is on"
        );
    }
}

// -- Browse buttons: the FileDialogs seam ------------------------------------

#[test]
fn browse_button_uses_dialog_seam_for_summary_output_folder() {
    let mut settings = Settings::default();
    settings.postproc.ai_summary.enabled = true;
    let picked = PathBuf::from("/picked/summaries");
    let mut harness = postproc_harness(settings, Arc::new(FixedFolderDialogs(picked.clone())));
    harness.run();

    // The summary section's Browse button is the first "Browse" node in
    // tree order (summary section renders before the docx section).
    harness
        .get_all_by_label("Browse")
        .next()
        .expect("summary-section Browse button")
        .click();
    harness.run();

    let settings_path = harness.state().settings_path.clone();
    let reloaded = Settings::load(&settings_path).expect("settings file was written");
    assert_eq!(reloaded.postproc.ai_summary.output_folder, Some(picked));
}

#[cfg(test)]
mod smoke {
    use super::*;

    /// Sanity check that a completely default-settings tab renders without
    /// panicking (every section collapsed-open, every field readable) --
    /// the cheapest possible regression guard against a typo'd ftl key
    /// (`tr`'s missing-key fallback would still render *something*, but a
    /// bad widget call -- e.g. an id clash -- would panic under kittest).
    #[test]
    fn renders_with_default_settings() {
        let mut harness = postproc_harness(Settings::default(), Arc::new(NullDialogs));
        harness.run();

        // Exact-match lookups (`get_by_label_contains` would also match each
        // section's own "Enable …" checkbox, which contains the section
        // title as a substring): the four `CollapsingHeader` titles
        // themselves.
        harness.get_by_label("Skip Already Transcribed");
        harness.get_by_label("Automatic Transcript Resizer");
        harness.get_by_label("AI Summary");
        harness.get_by_label("Export to DOCX");
    }
}
