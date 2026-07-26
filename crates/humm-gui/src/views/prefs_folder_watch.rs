//! Preferences → Folder Watch tab (F-030c, design spec §8.4): the enable
//! checkbox, input/output folder rows (each a path field + native Browse via
//! the [`FileDialogs`] seam), a "Delete processed files" checkbox with a
//! disposition caption (design spec §3.4, ✚ — Buzz never told users this),
//! and the embedded auto-task options form over a [`SheetState`] seeded from
//! `settings.folder_watch.options`.
//!
//! Ported from Buzz's `folder_watch_preferences_widget.py` (whole file): the
//! enable checkbox (`:42-45`), the input-folder row + Browse (`:52-65`), the
//! output-folder row + Browse (`:55-74`), the delete-processed checkbox
//! (`:47-50`), the embedded `FileTranscriptionFormWidget` (`:85-92`), and
//! `_set_settings_enabled` gating everything except the enable checkbox
//! (`:132-138`). Buzz applies these prefs only on the Preferences dialog's
//! OK button; humm is write-through-on-commit instead (established, spec
//! ledger #L8) — checkboxes commit on click, path fields commit on
//! focus-out/Enter (the same M6 commit-helper pattern every other humm
//! path/text field uses), and the embedded options form's controls commit as
//! one `SheetState::write_to` call whenever anything in that block changes.
//!
//! **Browse-cancel divergence (deliberate, not a port)**: Buzz's own
//! `on_click_browse_input_folder`/`on_click_browse_output_folder`
//! (`:114-117`, `:123-126`) call `line_edit.setText(folder)` unconditionally,
//! even when `QFileDialog.getExistingDirectory` returns `""` on cancel —
//! clearing an already-configured field just by opening and canceling the
//! dialog. This looks like an unintentional quirk local to this one widget,
//! not a deliberate Buzz behavior: the app's own general convention guards
//! exactly this case (`main_window.py`'s `on_import_folder_action_
//! triggered`, `:290-292`, `if not folder: return`). [`ui`]'s own `if let
//! Some(picked) = dialogs.pick_folder(..)` only ever updates the buffer on
//! an actual pick, leaving it untouched on cancel — the same "cancel is a
//! no-op" convention every other Browse-button call site in this codebase
//! already follows (`views/recording_advanced.rs`'s own export-folder
//! Browse), so this is a non-port of an unintentional Buzz quirk, not a
//! tracked deviation from an intentional one.
//!
//! ## Watcher rebuild: no extra wiring needed
//!
//! Every write this tab makes lands in `settings.folder_watch.*` under the
//! same `Arc<Mutex<Settings>>` every other Preferences tab writes through.
//! `app.rs::HummApp::sync_folder_watch` already re-derives a
//! `FolderWatchSnapshot` from `settings.folder_watch` fresh every frame
//! (design spec §3.2's "rebuild-on-commit") and reconciles the running
//! service against it — an `input_dir` change while already running
//! resolves to `ReconcileAction::Restart`; the enabled/disabled transition
//! itself resolves to `Start`/`Stop`, not `Restart` (pinned by
//! `folder_watch.rs`'s own `disabled_to_enabled_transition_starts_not_
//! restarts` test); everything else
//! (`output_dir`/`delete_processed_files`/`options`) resolves to
//! `ReconcileAction::UpdateTemplate` (no watcher teardown at all). This
//! module needs no direct handle on the service, no "changed" flag of its
//! own, and no call into `folder_watch.rs` — the existing per-frame compare
//! already picks up every field this tab edits, confirmed by reading
//! `FolderWatchSnapshot::from_settings` (it clones exactly `enabled`/
//! `input_dir`/`output_dir`/`delete_processed_files`/`options`, i.e. every
//! field [`FolderWatchTabState`] below can touch).
//!
//! ## The embedded options form
//!
//! Shares [`crate::views::options_sheet::SheetState`] and its shared row
//! renderers wholesale (`render_model_combo`/`render_task_combo`/
//! `render_language_combo`/`render_backend_combo`/`render_api_key_row`/
//! `render_export_checkboxes`/`render_llm_translation_block`, each promoted
//! to `pub(crate)` this task) rather than re-deriving a second copy of the
//! same form — every renderer already takes an `id_salt`, so this tab's
//! `ComboBox`es (`humm-prefs-fw-*`) never collide with the options sheet's
//! own (`humm-options-*`) even when both windows are open and rendering in
//! the same frame (they're separate `egui` viewports sharing one `Context`'s
//! `Id` namespace). The Api Key row's `secrets` plumbing and keyring commit
//! behavior are identical to the options sheet's own — [`SheetState::api_key`]
//! is never read by [`SheetState::write_to`], so no key material can reach
//! `settings.folder_watch.options` even by accident: `FileTranscriberSettings`
//! (what `write_to` writes into) structurally has no key-shaped field at all.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use humm_core::secrets::{SecretKey, SecretStore};
use humm_core::settings::{FileTranscriberSettings, FolderWatchSettings, Settings};

use crate::i18n::tr;
use crate::import::FileDialogs;
use crate::views::options_sheet::{
    SheetModal, SheetState, api_key_row_visible, render_api_key_row, render_backend_combo,
    render_export_checkboxes, render_language_combo, render_llm_translation_block,
    render_model_combo, render_task_combo, sheet_modal_title_and_body, size_combo_visible,
};

/// The Folder Watch tab's own live state, owned by
/// [`crate::views::preferences::PreferencesState`]. Rebuilt fresh each time
/// the Preferences window (re)opens ([`FolderWatchTabState::new`]), same
/// "snapshot settings + the keyring at open time" contract every other tab
/// in this strip follows.
///
/// No manual `Debug` impl is needed here (unlike
/// `views::prefs_general::GeneralTabState`/`views::options_sheet::OptionsSheet`,
/// which both hand-write one): [`SheetState`] already redacts its own
/// `api_key` field in its `Debug` impl, so `#[derive(Debug)]` on this struct
/// inherits that redaction for the embedded `sheet` field for free — nothing
/// else here is sensitive.
#[derive(Debug)]
pub struct FolderWatchTabState {
    /// "Enable folder watch" checkbox (Buzz `:42-45`). Gates every other row
    /// via [`ui`]'s own `add_enabled_ui` (Buzz `_set_settings_enabled`,
    /// `:132-138`) — this checkbox itself is never gated.
    pub enabled: bool,
    /// The input-folder path field's live text buffer (Buzz's
    /// `input_folder_line_edit`) — commits on focus-out/Enter into
    /// `settings.folder_watch.input_dir` (empty, once trimmed, → `None`).
    pub input_dir_buffer: String,
    /// The output-folder path field's live text buffer, same commit rule,
    /// into `settings.folder_watch.output_dir`.
    pub output_dir_buffer: String,
    /// "Delete processed files" checkbox (Buzz `:47-50`).
    pub delete_processed_files: bool,
    /// The embedded auto-task options form's live state, seeded from
    /// `settings.folder_watch.options` via [`SheetState::from_settings`] —
    /// the same derivation the options sheet's own call site uses.
    pub sheet: SheetState,
    /// The embedded Api Key row's commit-failure modal, if a commit this
    /// frame produced one (mirrors `OptionsSheet::modal`).
    pub modal: Option<SheetModal>,
    /// Snapshot of `settings.models.custom_model_url.is_some()` taken at
    /// [`FolderWatchTabState::new`] time — gates the embedded model-size
    /// combo's "Custom" entry. Captured once, not re-derived per frame, the
    /// same "snapshotted at window-open time" contract
    /// `ModelsTabState::include_lumii` already establishes for this tab
    /// strip (a locale/custom-URL change made elsewhere takes effect the
    /// next time the Preferences window reopens, not live mid-session).
    pub custom_model_available: bool,
    /// Snapshot of the Lumii locale gate (F-044/J-9, design spec §8.2), same
    /// "captured once" contract as `custom_model_available` above.
    pub include_lumii: bool,
}

impl FolderWatchTabState {
    /// Builds the tab's initial state: the enable/paths/delete fields read
    /// straight off `settings`, the embedded `sheet` seeded from
    /// `settings.options` (`custom_model_available` gates a stale `Custom`
    /// prefill back to `Small`, exactly like [`OptionsSheet::new`]) with its
    /// Api Key row additionally seeded from `secrets` (the one thing
    /// `SheetState::from_settings` cannot do on its own).
    pub fn new(
        settings: &FolderWatchSettings,
        secrets: &dyn SecretStore,
        custom_model_available: bool,
        include_lumii: bool,
    ) -> Self {
        let mut sheet = SheetState::from_settings(&settings.options, custom_model_available);
        sheet.api_key = secrets
            .get(SecretKey::OpenAiApiKey)
            .ok()
            .flatten()
            .unwrap_or_default();
        FolderWatchTabState {
            enabled: settings.enabled,
            input_dir_buffer: settings
                .input_dir
                .as_ref()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
            output_dir_buffer: settings
                .output_dir
                .as_ref()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
            delete_processed_files: settings.delete_processed_files,
            sheet,
            modal: None,
            custom_model_available,
            include_lumii,
        }
    }
}

// -- Pure field-commit helpers ------------------------------------------------

/// `s` trimmed and wrapped in `Some(PathBuf::from(...))`, or `None` if that
/// trims to empty — the folder-watch path fields' own "blank means unset"
/// rule (mirrors `prefs_general::empty_to_none`/`models_ui::non_empty`, this
/// module's path-flavored version of the same idiom).
fn non_empty_path(s: &str) -> Option<PathBuf> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Locks `settings`, applies `f` to `settings.folder_watch`, and saves — the
/// same diff-free, non-fatal-on-failure immediate-save contract every other
/// tab's own `persist_*` helper uses (e.g. `prefs_general::persist_openai`).
fn persist_folder_watch(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut FolderWatchSettings),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.folder_watch);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the folder-watch settings: {err}");
    }
}

/// Commits the "Enable folder watch" checkbox.
pub fn commit_enabled(
    state: &FolderWatchTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.enabled;
    persist_folder_watch(settings, settings_path, |fw| fw.enabled = value);
}

/// Commits the "Delete processed files" checkbox.
pub fn commit_delete_processed_files(
    state: &FolderWatchTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.delete_processed_files;
    persist_folder_watch(settings, settings_path, |fw| {
        fw.delete_processed_files = value
    });
}

/// Commits the input-folder path field: empty (trimmed) → `None`.
pub fn commit_input_dir(
    state: &FolderWatchTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = non_empty_path(&state.input_dir_buffer);
    persist_folder_watch(settings, settings_path, |fw| fw.input_dir = value);
}

/// Commits the output-folder path field: empty (trimmed) → `None`.
pub fn commit_output_dir(
    state: &FolderWatchTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = non_empty_path(&state.output_dir_buffer);
    persist_folder_watch(settings, settings_path, |fw| fw.output_dir = value);
}

/// Commits the embedded options form as a whole: `SheetState::write_to` is
/// total over `FileTranscriberSettings` (every field the shared renderers
/// above can edit — `backend`/`model_size`/`hf_repo`/`task`/`language`/
/// `word_timestamps`/`initial_prompt`/`export_formats`/
/// `enable_llm_translation`/`llm_model`/`llm_prompt`), so one call here
/// covers every control in the form regardless of which one actually
/// changed this frame. `api_key`/`api_key_visible`/`export_template` are
/// deliberately never written by `write_to` (see [`SheetState`]'s own field
/// docs) — `last_import_folder` (unused in this context) is left untouched
/// too, since `write_to` never touches it.
pub fn commit_options(
    state: &FolderWatchTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    persist_folder_watch(settings, settings_path, |fw| {
        state.sheet.write_to(&mut fw.options)
    });
}

/// The projection of `sheet` that [`SheetState::write_to`] actually persists
/// — used by [`ui`] as a before/after fingerprint so the embedded form's
/// commit ([`commit_options`]) fires exactly when something `write_to` cares
/// about changed, and not merely because the Api Key row's `api_key`/
/// `api_key_visible` buffers (never persisted here) happened to change too.
fn persisted_projection(sheet: &SheetState) -> FileTranscriberSettings {
    let mut projection = FileTranscriberSettings::default();
    sheet.write_to(&mut projection);
    projection
}

// -- Render -------------------------------------------------------------------

/// Renders the Folder Watch tab, Buzz row order (module doc): enable
/// checkbox, input folder + Browse, output folder + Browse, delete-processed
/// checkbox + disposition caption, then the embedded options form — all but
/// the enable checkbox disabled while the feature is off (Buzz
/// `_set_settings_enabled`).
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut FolderWatchTabState,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    secrets: &Arc<dyn SecretStore>,
    dialogs: &dyn FileDialogs,
) {
    let mut enabled = state.enabled;
    if ui.checkbox(&mut enabled, tr("prefs-fw-enable")).changed() {
        state.enabled = enabled;
        commit_enabled(state, settings, settings_path);
    }

    ui.add_enabled_ui(state.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(tr("prefs-fw-input-folder-label"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.input_dir_buffer)
                    .hint_text(tr("prefs-fw-input-folder-placeholder")),
            );
            if response.lost_focus() {
                commit_input_dir(state, settings, settings_path);
            }
            if ui.button(tr("prefs-fw-browse")).clicked() {
                let start = non_empty_path(&state.input_dir_buffer);
                if let Some(picked) = dialogs.pick_folder(start.as_deref()) {
                    state.input_dir_buffer = picked.display().to_string();
                    commit_input_dir(state, settings, settings_path);
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label(tr("prefs-fw-output-folder-label"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.output_dir_buffer)
                    .hint_text(tr("prefs-fw-output-folder-placeholder")),
            );
            if response.lost_focus() {
                commit_output_dir(state, settings, settings_path);
            }
            if ui.button(tr("prefs-fw-browse")).clicked() {
                let start = non_empty_path(&state.output_dir_buffer);
                if let Some(picked) = dialogs.pick_folder(start.as_deref()) {
                    state.output_dir_buffer = picked.display().to_string();
                    commit_output_dir(state, settings, settings_path);
                }
            }
        });

        let mut delete_processed_files = state.delete_processed_files;
        if ui
            .checkbox(&mut delete_processed_files, tr("prefs-fw-delete-checkbox"))
            .changed()
        {
            state.delete_processed_files = delete_processed_files;
            commit_delete_processed_files(state, settings, settings_path);
        }
        ui.label(tr("prefs-fw-disposition-note"));

        ui.separator();

        // The embedded options form: shared renderers over `state.sheet`,
        // this tab's own `id_salt`s so its `ComboBox`es never collide with
        // the options sheet's (module doc). Committed as one unit below,
        // via a before/after fingerprint over exactly the fields
        // `SheetState::write_to` persists.
        let before = persisted_projection(&state.sheet);

        render_backend_combo(ui, "humm-prefs-fw-backend", &mut state.sheet.backend);
        if size_combo_visible(state.sheet.backend) {
            render_model_combo(
                ui,
                "humm-prefs-fw-model-size",
                &mut state.sheet.model_size,
                state.custom_model_available,
                state.include_lumii,
            );
        }
        if api_key_row_visible(state.sheet.backend) {
            render_api_key_row(
                ui,
                &mut state.sheet.api_key,
                &mut state.sheet.api_key_visible,
                &mut state.modal,
                secrets,
            );
        }
        render_task_combo(ui, "humm-prefs-fw-task", &mut state.sheet.task);
        render_language_combo(ui, "humm-prefs-fw-language", &mut state.sheet.language);
        ui.checkbox(
            &mut state.sheet.word_timestamps,
            tr("options-word-timestamps"),
        );
        render_export_checkboxes(ui, &mut state.sheet.formats);
        render_llm_translation_block(ui, "humm-prefs-fw-llm-model-presets", &mut state.sheet);

        let after = persisted_projection(&state.sheet);
        if before != after {
            commit_options(state, settings, settings_path);
        }
    });

    if let Some(modal) = state.modal {
        let (title, body) = sheet_modal_title_and_body(modal);
        let mut close = false;
        let modal_response =
            egui::Modal::new(egui::Id::new("humm-prefs-fw-key-modal")).show(ui.ctx(), |ui| {
                ui.heading(title);
                ui.label(body);
                if ui.button(tr("dialog-ok")).clicked() {
                    close = true;
                }
            });
        if close || modal_response.should_close() {
            state.modal = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::{Backend, OutputFormat, TaskKind};
    use humm_core::secrets::MemorySecretStore;
    use pretty_assertions::assert_eq;

    // -- non_empty_path -----------------------------------------------------

    #[test]
    fn non_empty_path_trims_and_maps_blank_to_none() {
        assert_eq!(non_empty_path(""), None);
        assert_eq!(non_empty_path("   "), None);
        assert_eq!(
            non_empty_path("  /watch/in  "),
            Some(PathBuf::from("/watch/in"))
        );
    }

    // -- FolderWatchTabState::new ---------------------------------------------

    #[test]
    fn new_seeds_every_field_from_settings() {
        let secrets = MemorySecretStore::new();
        let settings = FolderWatchSettings {
            enabled: true,
            input_dir: Some(PathBuf::from("/watch/in")),
            output_dir: Some(PathBuf::from("/watch/out")),
            delete_processed_files: true,
            options: FileTranscriberSettings {
                task: Some("translate".to_string()),
                ..Default::default()
            },
        };

        let state = FolderWatchTabState::new(&settings, &secrets, false, false);

        assert!(state.enabled);
        assert_eq!(state.input_dir_buffer, "/watch/in");
        assert_eq!(state.output_dir_buffer, "/watch/out");
        assert!(state.delete_processed_files);
        assert_eq!(state.sheet.task, TaskKind::Translate);
        assert_eq!(state.modal, None);
    }

    #[test]
    fn new_seeds_empty_buffers_on_a_fresh_install() {
        let secrets = MemorySecretStore::new();
        let state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);

        assert!(!state.enabled);
        assert_eq!(state.input_dir_buffer, "");
        assert_eq!(state.output_dir_buffer, "");
        assert!(!state.delete_processed_files);
    }

    #[test]
    fn new_seeds_the_api_key_from_the_secret_store() {
        let secrets = MemorySecretStore::new();
        secrets.set(SecretKey::OpenAiApiKey, "sk-seeded").unwrap();

        let state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);

        assert_eq!(state.sheet.api_key, "sk-seeded");
    }

    // -- Debug redaction --------------------------------------------------

    #[test]
    fn debug_redacts_the_embedded_api_key() {
        let secrets = MemorySecretStore::new();
        secrets
            .set(SecretKey::OpenAiApiKey, "sk-super-secret-value")
            .unwrap();
        let state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);

        let debug_output = format!("{state:?}");

        assert!(
            !debug_output.contains("sk-super-secret-value"),
            "Debug output must not contain the key material, got: {debug_output}"
        );
        assert!(
            debug_output.contains("«redacted»"),
            "Debug output must contain the redaction marker, got: {debug_output}"
        );
    }

    // -- commit_* --------------------------------------------------------------

    #[test]
    fn commit_enabled_persists() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let secrets = MemorySecretStore::new();

        let mut state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);
        state.enabled = true;
        commit_enabled(&state, &settings, &settings_path);

        assert!(settings.lock().unwrap().folder_watch.enabled);
    }

    #[test]
    fn commit_delete_processed_files_persists() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let secrets = MemorySecretStore::new();

        let mut state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);
        state.delete_processed_files = true;
        commit_delete_processed_files(&state, &settings, &settings_path);

        assert!(settings.lock().unwrap().folder_watch.delete_processed_files);
    }

    #[test]
    fn commit_input_dir_persists_and_empties_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let secrets = MemorySecretStore::new();

        let mut state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);
        state.input_dir_buffer = "/watch/in".to_string();
        commit_input_dir(&state, &settings, &settings_path);
        assert_eq!(
            settings.lock().unwrap().folder_watch.input_dir,
            Some(PathBuf::from("/watch/in"))
        );

        state.input_dir_buffer = String::new();
        commit_input_dir(&state, &settings, &settings_path);
        assert_eq!(settings.lock().unwrap().folder_watch.input_dir, None);
    }

    #[test]
    fn commit_output_dir_persists_and_empties_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let secrets = MemorySecretStore::new();

        let mut state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);
        state.output_dir_buffer = "/watch/out".to_string();
        commit_output_dir(&state, &settings, &settings_path);
        assert_eq!(
            settings.lock().unwrap().folder_watch.output_dir,
            Some(PathBuf::from("/watch/out"))
        );

        state.output_dir_buffer = String::new();
        commit_output_dir(&state, &settings, &settings_path);
        assert_eq!(settings.lock().unwrap().folder_watch.output_dir, None);
    }

    #[test]
    fn commit_options_writes_the_whole_sheet_into_folder_watch_options() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let secrets = MemorySecretStore::new();

        let mut state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);
        state.sheet.backend = Backend::OpenAiApi;
        state.sheet.task = TaskKind::Translate;
        state.sheet.word_timestamps = true;
        state.sheet.formats = [OutputFormat::Srt].into_iter().collect();
        commit_options(&state, &settings, &settings_path);

        let guard = settings.lock().unwrap();
        assert_eq!(
            guard.folder_watch.options.backend.as_deref(),
            Some("openai_api")
        );
        assert_eq!(
            guard.folder_watch.options.task.as_deref(),
            Some("translate")
        );
        assert_eq!(guard.folder_watch.options.word_timestamps, Some(true));
        assert_eq!(
            guard.folder_watch.options.export_formats,
            Some(vec!["srt".to_string()])
        );
    }

    #[test]
    fn commit_options_never_writes_the_api_key_anywhere() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let secrets = MemorySecretStore::new();

        let mut state =
            FolderWatchTabState::new(&FolderWatchSettings::default(), &secrets, false, false);
        state.sheet.api_key = "sk-super-secret-value".to_string();
        commit_options(&state, &settings, &settings_path);

        let serialized = toml::to_string(&*settings.lock().unwrap()).expect("settings serialize");
        assert!(
            !serialized.contains("sk-super-secret-value"),
            "the API key must never reach settings.toml (design spec §5) — \
             FileTranscriberSettings has no key-shaped field for it to land in"
        );
    }

    // -- persisted_projection --------------------------------------------------

    #[test]
    fn persisted_projection_ignores_api_key_and_visibility() {
        let mut sheet = SheetState::from_settings(&FileTranscriberSettings::default(), false);
        let before = persisted_projection(&sheet);

        sheet.api_key = "sk-typed-but-not-committed".to_string();
        sheet.api_key_visible = true;
        let after = persisted_projection(&sheet);

        assert_eq!(
            before, after,
            "the api_key/api_key_visible buffers must not affect the persisted projection"
        );
    }

    #[test]
    fn persisted_projection_reflects_a_real_field_change() {
        let mut sheet = SheetState::from_settings(&FileTranscriberSettings::default(), false);
        let before = persisted_projection(&sheet);

        sheet.word_timestamps = !sheet.word_timestamps;
        let after = persisted_projection(&sheet);

        assert_ne!(before, after);
    }
}
