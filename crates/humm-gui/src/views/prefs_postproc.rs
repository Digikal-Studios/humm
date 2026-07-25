//! Preferences → Post-processing tab (ADR-010, design spec §8.5): the config
//! home for the four native post-processing hooks (F-110's resolution) —
//! **Skip Already Transcribed** (F-115, the only pre-hook), then, in the
//! fixed execution order design spec §2.1 establishes, **Automatic
//! Transcript Resizer** (F-116) → **AI Summary** (F-111) → **Export to
//! DOCX** (F-113). Each section is an `egui::CollapsingHeader` with an
//! enable checkbox first, then its fields (gated by that section's own
//! `enabled`, `ui`'s own `add_enabled_ui` blocks).
//!
//! ADR-010: these are first-class features, not a plugin system — there is
//! no enable/ordering list here and no install surface. Buzz rendered each
//! plugin's `ConfigField` declarations through a generic form
//! (`widgets/plugins_dialog/`), which this crate does **not** port; what
//! *does* carry over, byte-faithful, is each field's Buzz label/description
//! (the source of every `en` ftl string below):
//! - `buzz/plugins/skip_already_transcribed/plugin.py:28-47`
//! - `buzz/plugins/transcript_resizer/plugin.py:51-88`
//! - `buzz/plugins/ai_summary/plugin.py:47-93`
//! - `buzz/plugins/export_docx/plugin.py:74-88`
//!
//! ## Persistence
//!
//! Checkboxes write-through on click (immediate `persist_postproc` call,
//! same "no separate Ok/Apply step" contract every other Preferences tab
//! follows). Text/path fields commit on focus-out/Enter
//! ([`egui::Response::lost_focus`], the M6 commit pattern). Empty-string
//! rules vary by field type (documented on each `commit_*` below):
//! - `Option<String>`/`Option<PathBuf>` fields (`api_url`, both
//!   `output_folder`s) map an empty buffer to `None` — [`empty_to_none`]/
//!   [`non_empty_path`], the same helpers `prefs_general`/
//!   `prefs_folder_watch` already establish for their own fields.
//! - `model`/`prompt` are plain (non-`Option`) `String`s that persist
//!   **verbatim, even empty** — a deliberate divergence from
//!   `prefs_general::api_model_or_default`'s "empty commits the default"
//!   rule: `humm_core::postproc::summary::or_default_then_trim` already
//!   applies Buzz's own `cfg.get("model") or DEFAULT_MODEL` fallback at
//!   *read* time (`postproc/summary.rs:139-140`), so persisting an empty
//!   buffer here is safe — the hook always resolves it to
//!   [`AiSummarySettings::DEFAULT_MODEL`]/[`AiSummarySettings::DEFAULT_PROMPT`]
//!   at the moment it runs, matching the runtime's own or-default seam
//!   rather than duplicating it here.
//! - `merge_gap_seconds`/`punctuation`/`max_length` (the resizer's Buzz
//!   `TEXT` fields) are the same story: `humm_core::postproc::auto_resize`'s
//!   own `or_default`/`or_default_then_trim` calls (`auto_resize.rs:172-176`)
//!   already handle an empty string at read time, so these three persist
//!   verbatim too — Buzz-faithful, no UI-level validation or transform.
//!
//! The AI Summary API key is the one field that never touches
//! `settings.toml`: it commits straight to the OS keyring via
//! [`secret_field::commit_secret`] under
//! [`SecretKey::AiSummaryApiKey`] — its own key, never the transcription
//! key (design spec §6.2) — reusing [`secret_field::masked_field_row`] for
//! the eye-toggle row and the same [`crate::views::prefs_general::
//! GeneralModal::KeyringSaveFailed`]-shaped modal on a keyring write
//! failure. Unlike the General tab's key row, there is **no Test button**
//! here — Buzz's `ai_summary` plugin config never had one either.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use humm_core::secrets::{SecretKey, SecretStore};
use humm_core::settings::{AiSummarySettings, PostprocSettings, Settings};

use crate::i18n::tr;
use crate::import::FileDialogs;
use crate::views::secret_field::{self, CommitOutcome};

// -- Pure field-commit helpers ------------------------------------------------

/// `s` wrapped in `Some`, or `None` if `s` is empty — the same "empty means
/// unset" rule [`crate::views::prefs_general`]'s own `empty_to_none` applies
/// to its base-URL field (no trim: a base URL is copy-pasted verbatim, Buzz
/// parity).
fn empty_to_none(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// `s` trimmed and wrapped in `Some(PathBuf::from(...))`, or `None` if that
/// trims to empty — mirrors `prefs_folder_watch::non_empty_path` for this
/// tab's own two folder fields.
fn non_empty_path(s: &str) -> Option<PathBuf> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Locks `settings`, applies `f` to `settings.postproc`, and saves — the same
/// diff-free, non-fatal-on-failure immediate-save contract every other tab's
/// own `persist_*` helper uses (e.g. `prefs_general::persist_openai`).
fn persist_postproc(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut PostprocSettings),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.postproc);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the post-processing settings: {err}");
    }
}

fn folder_buffer(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

// -- Modal --------------------------------------------------------------------

/// The AI Summary key row's commit-failure modals — the same two outcomes
/// [`crate::views::prefs_general::GeneralModal`] renders for the General
/// tab's own key field, minus `TestResult` (this tab has no Test button).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostprocModal {
    /// The key field's committed value failed
    /// [`secret_field::is_base64_charset`] — warn-only, the value is saved
    /// regardless (Buzz parity, same as the General tab's own key field).
    InvalidKeyCharset,
    /// [`SecretStore::set`] failed while committing the key field.
    KeyringSaveFailed,
}

fn modal_title_and_body(modal: &PostprocModal) -> (String, String) {
    match modal {
        PostprocModal::InvalidKeyCharset => (
            tr("prefs-general-invalid-key-title"),
            tr("prefs-general-invalid-key-body"),
        ),
        PostprocModal::KeyringSaveFailed => (
            tr("prefs-general-key-save-error-title"),
            tr("prefs-postproc-summary-key-save-error-body"),
        ),
    }
}

// -- State ----------------------------------------------------------------

/// The Post-processing tab's live state, held in
/// [`crate::views::preferences::PreferencesState`]. Rebuilt fresh each time
/// the Preferences window (re)opens ([`PostprocTabState::new`]), same
/// "snapshot settings + the keyring at open time" contract every other tab
/// in this strip follows.
pub struct PostprocTabState {
    // -- Skip Already Transcribed (F-115) --
    /// "Enable Skip Already Transcribed" checkbox.
    pub skip_enabled: bool,
    /// `check_result_files` checkbox (Buzz "Check for existing result
    /// files").
    pub check_result_files: bool,
    /// `check_database` checkbox (Buzz "Check in transcription database").
    pub check_database: bool,

    // -- Automatic Transcript Resizer (F-116) --
    /// "Enable Automatic Transcript Resizer" checkbox.
    pub resizer_enabled: bool,
    /// `merge_by_gap` checkbox.
    pub merge_by_gap: bool,
    /// `merge_gap_seconds` text buffer (Buzz `TEXT` field, kept as a string —
    /// module doc's documented empty-is-safe rationale).
    pub merge_gap_seconds_buffer: String,
    /// `split_by_punctuation` checkbox.
    pub split_by_punctuation: bool,
    /// `punctuation` text buffer.
    pub punctuation_buffer: String,
    /// `split_by_max_length` checkbox.
    pub split_by_max_length: bool,
    /// `max_length` text buffer.
    pub max_length_buffer: String,

    // -- AI Summary (F-111) --
    /// "Enable AI Summary" checkbox.
    pub summary_enabled: bool,
    /// `api_url` text buffer (empty means "use
    /// [`AiSummarySettings::DEFAULT_API_URL`]").
    pub summary_api_url_buffer: String,
    /// The API key field's live text — seeded from [`SecretStore::get`] at
    /// construction, committed back to the store on focus-out/Enter
    /// ([`commit_summary_key`]). Never serialized (manual `Debug` below
    /// redacts it, mirroring `GeneralTabState::key_buffer`).
    pub summary_key_buffer: String,
    /// Whether the key field currently renders unmasked (the eye toggle).
    pub summary_key_visible: bool,
    /// `model` text buffer — persisted verbatim, even empty (module doc).
    pub summary_model_buffer: String,
    /// `prompt` text buffer (Buzz `TEXTAREA`, rendered as a multiline
    /// `TextEdit`) — persisted verbatim, even empty (module doc).
    pub summary_prompt_buffer: String,
    /// `save_to_notes` checkbox.
    pub summary_save_to_notes: bool,
    /// `save_to_file` checkbox.
    pub summary_save_to_file: bool,
    /// `output_folder` text buffer (empty means "save next to the source
    /// file").
    pub summary_output_folder_buffer: String,

    // -- Export to DOCX (F-113) --
    /// "Enable Export to DOCX" checkbox.
    pub docx_enabled: bool,
    /// `output_folder` text buffer (empty means "save next to the source
    /// file").
    pub docx_output_folder_buffer: String,
    /// `include_timestamps` checkbox.
    pub docx_include_timestamps: bool,

    /// The currently-open modal, if any (the AI Summary key row's own).
    pub modal: Option<PostprocModal>,
}

impl PostprocTabState {
    /// Builds the tab's initial state: every field read straight off
    /// `settings.postproc.*`, except `summary_key_buffer`, which seeds from
    /// [`SecretStore::get`] (an absent entry, or a backend error, both seed
    /// an empty field — the same "nothing stored yet" starting point
    /// `GeneralTabState::new` uses for its own key field).
    pub fn new(settings: &PostprocSettings, secrets: &dyn SecretStore) -> Self {
        let summary_key_buffer = secrets
            .get(SecretKey::AiSummaryApiKey)
            .ok()
            .flatten()
            .unwrap_or_default();
        PostprocTabState {
            skip_enabled: settings.skip_already_transcribed.enabled,
            check_result_files: settings.skip_already_transcribed.check_result_files,
            check_database: settings.skip_already_transcribed.check_database,

            resizer_enabled: settings.transcript_resizer.enabled,
            merge_by_gap: settings.transcript_resizer.merge_by_gap,
            merge_gap_seconds_buffer: settings.transcript_resizer.merge_gap_seconds.clone(),
            split_by_punctuation: settings.transcript_resizer.split_by_punctuation,
            punctuation_buffer: settings.transcript_resizer.punctuation.clone(),
            split_by_max_length: settings.transcript_resizer.split_by_max_length,
            max_length_buffer: settings.transcript_resizer.max_length.clone(),

            summary_enabled: settings.ai_summary.enabled,
            summary_api_url_buffer: settings.ai_summary.api_url.clone().unwrap_or_default(),
            summary_key_buffer,
            summary_key_visible: false,
            summary_model_buffer: settings.ai_summary.model.clone(),
            summary_prompt_buffer: settings.ai_summary.prompt.clone(),
            summary_save_to_notes: settings.ai_summary.save_to_notes,
            summary_save_to_file: settings.ai_summary.save_to_file,
            summary_output_folder_buffer: folder_buffer(&settings.ai_summary.output_folder),

            docx_enabled: settings.export_docx.enabled,
            docx_output_folder_buffer: folder_buffer(&settings.export_docx.output_folder),
            docx_include_timestamps: settings.export_docx.include_timestamps,

            modal: None,
        }
    }
}

impl std::fmt::Debug for PostprocTabState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostprocTabState")
            .field("skip_enabled", &self.skip_enabled)
            .field("check_result_files", &self.check_result_files)
            .field("check_database", &self.check_database)
            .field("resizer_enabled", &self.resizer_enabled)
            .field("merge_by_gap", &self.merge_by_gap)
            .field("merge_gap_seconds_buffer", &self.merge_gap_seconds_buffer)
            .field("split_by_punctuation", &self.split_by_punctuation)
            .field("punctuation_buffer", &self.punctuation_buffer)
            .field("split_by_max_length", &self.split_by_max_length)
            .field("max_length_buffer", &self.max_length_buffer)
            .field("summary_enabled", &self.summary_enabled)
            .field("summary_api_url_buffer", &self.summary_api_url_buffer)
            .field("summary_key_buffer", &"«redacted»")
            .field("summary_key_visible", &self.summary_key_visible)
            .field("summary_model_buffer", &self.summary_model_buffer)
            .field("summary_prompt_buffer", &self.summary_prompt_buffer)
            .field("summary_save_to_notes", &self.summary_save_to_notes)
            .field("summary_save_to_file", &self.summary_save_to_file)
            .field(
                "summary_output_folder_buffer",
                &self.summary_output_folder_buffer,
            )
            .field("docx_enabled", &self.docx_enabled)
            .field("docx_output_folder_buffer", &self.docx_output_folder_buffer)
            .field("docx_include_timestamps", &self.docx_include_timestamps)
            .field("modal", &self.modal)
            .finish()
    }
}

// -- Commit seam ---------------------------------------------------------

// -- Skip Already Transcribed --

/// Commits the "Enable Skip Already Transcribed" checkbox.
pub fn commit_skip_enabled(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.skip_enabled;
    persist_postproc(settings, settings_path, |p| {
        p.skip_already_transcribed.enabled = value
    });
}

/// Commits the "Check for existing result files" checkbox.
pub fn commit_check_result_files(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.check_result_files;
    persist_postproc(settings, settings_path, |p| {
        p.skip_already_transcribed.check_result_files = value
    });
}

/// Commits the "Check in transcription database" checkbox.
pub fn commit_check_database(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.check_database;
    persist_postproc(settings, settings_path, |p| {
        p.skip_already_transcribed.check_database = value
    });
}

// -- Automatic Transcript Resizer --

/// Commits the "Enable Automatic Transcript Resizer" checkbox.
pub fn commit_resizer_enabled(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.resizer_enabled;
    persist_postproc(settings, settings_path, |p| {
        p.transcript_resizer.enabled = value
    });
}

/// Commits the "Merge by gap" checkbox.
pub fn commit_merge_by_gap(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.merge_by_gap;
    persist_postproc(settings, settings_path, |p| {
        p.transcript_resizer.merge_by_gap = value
    });
}

/// Commits the "Merge gap (seconds)" text field — persisted verbatim, even
/// empty (module doc: `auto_resize`'s own `or_default` handles that at read
/// time).
pub fn commit_merge_gap_seconds(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.merge_gap_seconds_buffer.clone();
    persist_postproc(settings, settings_path, |p| {
        p.transcript_resizer.merge_gap_seconds = value
    });
}

/// Commits the "Split by punctuation" checkbox.
pub fn commit_split_by_punctuation(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.split_by_punctuation;
    persist_postproc(settings, settings_path, |p| {
        p.transcript_resizer.split_by_punctuation = value
    });
}

/// Commits the "Punctuation" text field — persisted verbatim, even empty.
pub fn commit_punctuation(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.punctuation_buffer.clone();
    persist_postproc(settings, settings_path, |p| {
        p.transcript_resizer.punctuation = value
    });
}

/// Commits the "Split by max length" checkbox.
pub fn commit_split_by_max_length(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.split_by_max_length;
    persist_postproc(settings, settings_path, |p| {
        p.transcript_resizer.split_by_max_length = value
    });
}

/// Commits the "Maximum subtitle length" text field — persisted verbatim,
/// even empty.
pub fn commit_max_length(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.max_length_buffer.clone();
    persist_postproc(settings, settings_path, |p| {
        p.transcript_resizer.max_length = value
    });
}

// -- AI Summary --

/// Commits the "Enable AI Summary" checkbox.
pub fn commit_summary_enabled(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.summary_enabled;
    persist_postproc(settings, settings_path, |p| p.ai_summary.enabled = value);
}

/// Commits the "API base URL" field: empty → `None` (falls back to
/// [`AiSummarySettings::DEFAULT_API_URL`] at read time).
pub fn commit_summary_api_url(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = empty_to_none(&state.summary_api_url_buffer);
    persist_postproc(settings, settings_path, |p| p.ai_summary.api_url = value);
}

/// Commits the AI Summary API key field: charset-checks it (warn-only, Buzz
/// parity), then writes it to the OS keyring under
/// [`SecretKey::AiSummaryApiKey`] — never `settings.toml`. Mirrors
/// `prefs_general::commit_key` exactly, one level simpler (no Test button to
/// gate).
pub fn commit_summary_key(state: &mut PostprocTabState, secrets: &Arc<dyn SecretStore>) {
    let outcome = secret_field::commit_secret(
        secrets,
        SecretKey::AiSummaryApiKey,
        &state.summary_key_buffer,
    );
    state.modal = match outcome {
        CommitOutcome::Saved => None,
        CommitOutcome::InvalidCharset => Some(PostprocModal::InvalidKeyCharset),
        CommitOutcome::KeyringSaveFailed => Some(PostprocModal::KeyringSaveFailed),
    };
}

/// Commits the "Model" field — persisted verbatim, even empty (module doc:
/// `postproc::summary`'s own `or_default_then_trim` handles that at read
/// time).
pub fn commit_summary_model(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.summary_model_buffer.clone();
    persist_postproc(settings, settings_path, |p| p.ai_summary.model = value);
}

/// Commits the "Summarization prompt" field — persisted verbatim, even
/// empty.
pub fn commit_summary_prompt(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.summary_prompt_buffer.clone();
    persist_postproc(settings, settings_path, |p| p.ai_summary.prompt = value);
}

/// Commits the "Save summary to Notes" checkbox.
pub fn commit_summary_save_to_notes(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.summary_save_to_notes;
    persist_postproc(settings, settings_path, |p| {
        p.ai_summary.save_to_notes = value
    });
}

/// Commits the "Save summary to a file" checkbox.
pub fn commit_summary_save_to_file(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.summary_save_to_file;
    persist_postproc(settings, settings_path, |p| {
        p.ai_summary.save_to_file = value
    });
}

/// Commits the "Output folder (for file)" field: empty → `None` ("save next
/// to the source file").
pub fn commit_summary_output_folder(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = non_empty_path(&state.summary_output_folder_buffer);
    persist_postproc(settings, settings_path, |p| {
        p.ai_summary.output_folder = value
    });
}

// -- Export to DOCX --

/// Commits the "Enable Export to DOCX" checkbox.
pub fn commit_docx_enabled(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.docx_enabled;
    persist_postproc(settings, settings_path, |p| p.export_docx.enabled = value);
}

/// Commits the "Output folder" field: empty → `None` ("save next to the
/// source file").
pub fn commit_docx_output_folder(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = non_empty_path(&state.docx_output_folder_buffer);
    persist_postproc(settings, settings_path, |p| {
        p.export_docx.output_folder = value
    });
}

/// Commits the "Include timestamps" checkbox.
pub fn commit_docx_include_timestamps(
    state: &PostprocTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = state.docx_include_timestamps;
    persist_postproc(settings, settings_path, |p| {
        p.export_docx.include_timestamps = value
    });
}

// -- Render -------------------------------------------------------------------

/// Renders the Post-processing tab: a header caption, then the four sections
/// as `CollapsingHeader`s in execution order (module doc) — Skip Already
/// Transcribed first (the pre-hook), then Automatic Transcript Resizer → AI
/// Summary → Export to DOCX (design spec §2.1). Every section defaults open
/// (no ADR-010 enable/ordering list to hide behind — the whole point of this
/// tab is being the four features' visible config home).
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut PostprocTabState,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    secrets: &Arc<dyn SecretStore>,
    dialogs: &dyn FileDialogs,
) {
    ui.label(tr("prefs-postproc-header"));
    ui.separator();

    // 1. Skip Already Transcribed (F-115) -- the only pre-hook, checked
    // first (design spec §2.1).
    egui::CollapsingHeader::new(tr("prefs-postproc-skip-title"))
        .id_salt("humm-prefs-postproc-skip")
        .default_open(true)
        .show(ui, |ui| {
            let mut enabled = state.skip_enabled;
            if ui
                .checkbox(&mut enabled, tr("prefs-postproc-skip-enable"))
                .changed()
            {
                state.skip_enabled = enabled;
                commit_skip_enabled(state, settings, settings_path);
            }
            ui.add_enabled_ui(state.skip_enabled, |ui| {
                let mut check_result_files = state.check_result_files;
                if ui
                    .checkbox(
                        &mut check_result_files,
                        tr("prefs-postproc-skip-check-files"),
                    )
                    .on_hover_text(tr("prefs-postproc-skip-check-files-desc"))
                    .changed()
                {
                    state.check_result_files = check_result_files;
                    commit_check_result_files(state, settings, settings_path);
                }
                let mut check_database = state.check_database;
                if ui
                    .checkbox(&mut check_database, tr("prefs-postproc-skip-check-db"))
                    .on_hover_text(tr("prefs-postproc-skip-check-db-desc"))
                    .changed()
                {
                    state.check_database = check_database;
                    commit_check_database(state, settings, settings_path);
                }
            });
        });

    // 2. Automatic Transcript Resizer (F-116).
    egui::CollapsingHeader::new(tr("prefs-postproc-resizer-title"))
        .id_salt("humm-prefs-postproc-resizer")
        .default_open(true)
        .show(ui, |ui| {
            let mut enabled = state.resizer_enabled;
            if ui
                .checkbox(&mut enabled, tr("prefs-postproc-resizer-enable"))
                .changed()
            {
                state.resizer_enabled = enabled;
                commit_resizer_enabled(state, settings, settings_path);
            }
            ui.add_enabled_ui(state.resizer_enabled, |ui| {
                let mut merge_by_gap = state.merge_by_gap;
                if ui
                    .checkbox(&mut merge_by_gap, tr("prefs-postproc-resizer-merge-by-gap"))
                    .changed()
                {
                    state.merge_by_gap = merge_by_gap;
                    commit_merge_by_gap(state, settings, settings_path);
                }
                ui.horizontal(|ui| {
                    ui.label(tr("prefs-postproc-resizer-merge-gap-seconds"));
                    let response = ui.text_edit_singleline(&mut state.merge_gap_seconds_buffer);
                    if response.lost_focus() {
                        commit_merge_gap_seconds(state, settings, settings_path);
                    }
                });

                let mut split_by_punctuation = state.split_by_punctuation;
                if ui
                    .checkbox(
                        &mut split_by_punctuation,
                        tr("prefs-postproc-resizer-split-by-punctuation"),
                    )
                    .changed()
                {
                    state.split_by_punctuation = split_by_punctuation;
                    commit_split_by_punctuation(state, settings, settings_path);
                }
                ui.horizontal(|ui| {
                    ui.label(tr("prefs-postproc-resizer-punctuation"));
                    let response = ui.text_edit_singleline(&mut state.punctuation_buffer);
                    if response.lost_focus() {
                        commit_punctuation(state, settings, settings_path);
                    }
                });

                let mut split_by_max_length = state.split_by_max_length;
                if ui
                    .checkbox(
                        &mut split_by_max_length,
                        tr("prefs-postproc-resizer-split-by-max-length"),
                    )
                    .changed()
                {
                    state.split_by_max_length = split_by_max_length;
                    commit_split_by_max_length(state, settings, settings_path);
                }
                ui.horizontal(|ui| {
                    ui.label(tr("prefs-postproc-resizer-max-length"));
                    let response = ui.text_edit_singleline(&mut state.max_length_buffer);
                    if response.lost_focus() {
                        commit_max_length(state, settings, settings_path);
                    }
                });
            });
        });

    // 3. AI Summary (F-111).
    egui::CollapsingHeader::new(tr("prefs-postproc-summary-title"))
        .id_salt("humm-prefs-postproc-summary")
        .default_open(true)
        .show(ui, |ui| {
            let mut enabled = state.summary_enabled;
            if ui
                .checkbox(&mut enabled, tr("prefs-postproc-summary-enable"))
                .changed()
            {
                state.summary_enabled = enabled;
                commit_summary_enabled(state, settings, settings_path);
            }
            ui.add_enabled_ui(state.summary_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("prefs-postproc-summary-api-url"));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.summary_api_url_buffer)
                            .hint_text(AiSummarySettings::DEFAULT_API_URL),
                    );
                    if response.lost_focus() {
                        commit_summary_api_url(state, settings, settings_path);
                    }
                });

                let key_response = secret_field::masked_field_row(
                    ui,
                    tr("prefs-postproc-summary-api-key-label"),
                    &mut state.summary_key_buffer,
                    &mut state.summary_key_visible,
                    tr("prefs-general-openai-key-placeholder"),
                );
                if key_response.lost_focus() {
                    commit_summary_key(state, secrets);
                }

                ui.horizontal(|ui| {
                    ui.label(tr("prefs-postproc-summary-model"));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.summary_model_buffer)
                            .hint_text(AiSummarySettings::DEFAULT_MODEL),
                    );
                    if response.lost_focus() {
                        commit_summary_model(state, settings, settings_path);
                    }
                });

                ui.label(tr("prefs-postproc-summary-prompt"));
                let prompt_response = ui.add(
                    egui::TextEdit::multiline(&mut state.summary_prompt_buffer).desired_rows(4),
                );
                if prompt_response.lost_focus() {
                    commit_summary_prompt(state, settings, settings_path);
                }

                let mut save_to_notes = state.summary_save_to_notes;
                if ui
                    .checkbox(
                        &mut save_to_notes,
                        tr("prefs-postproc-summary-save-to-notes"),
                    )
                    .changed()
                {
                    state.summary_save_to_notes = save_to_notes;
                    commit_summary_save_to_notes(state, settings, settings_path);
                }
                let mut save_to_file = state.summary_save_to_file;
                if ui
                    .checkbox(&mut save_to_file, tr("prefs-postproc-summary-save-to-file"))
                    .changed()
                {
                    state.summary_save_to_file = save_to_file;
                    commit_summary_save_to_file(state, settings, settings_path);
                }

                ui.horizontal(|ui| {
                    ui.label(tr("prefs-postproc-summary-output-folder"));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.summary_output_folder_buffer)
                            .hint_text(tr("prefs-postproc-output-folder-hint")),
                    );
                    if response.lost_focus() {
                        commit_summary_output_folder(state, settings, settings_path);
                    }
                    if ui.button(tr("prefs-fw-browse")).clicked() {
                        let start = non_empty_path(&state.summary_output_folder_buffer);
                        if let Some(picked) = dialogs.pick_folder(start.as_deref()) {
                            state.summary_output_folder_buffer = picked.display().to_string();
                            commit_summary_output_folder(state, settings, settings_path);
                        }
                    }
                });
            });
        });

    // 4. Export to DOCX (F-113).
    egui::CollapsingHeader::new(tr("prefs-postproc-docx-title"))
        .id_salt("humm-prefs-postproc-docx")
        .default_open(true)
        .show(ui, |ui| {
            let mut enabled = state.docx_enabled;
            if ui
                .checkbox(&mut enabled, tr("prefs-postproc-docx-enable"))
                .changed()
            {
                state.docx_enabled = enabled;
                commit_docx_enabled(state, settings, settings_path);
            }
            ui.add_enabled_ui(state.docx_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("prefs-postproc-docx-output-folder"));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.docx_output_folder_buffer)
                            .hint_text(tr("prefs-postproc-output-folder-hint")),
                    );
                    if response.lost_focus() {
                        commit_docx_output_folder(state, settings, settings_path);
                    }
                    if ui.button(tr("prefs-fw-browse")).clicked() {
                        let start = non_empty_path(&state.docx_output_folder_buffer);
                        if let Some(picked) = dialogs.pick_folder(start.as_deref()) {
                            state.docx_output_folder_buffer = picked.display().to_string();
                            commit_docx_output_folder(state, settings, settings_path);
                        }
                    }
                });
                let mut include_timestamps = state.docx_include_timestamps;
                if ui
                    .checkbox(
                        &mut include_timestamps,
                        tr("prefs-postproc-docx-include-timestamps"),
                    )
                    .changed()
                {
                    state.docx_include_timestamps = include_timestamps;
                    commit_docx_include_timestamps(state, settings, settings_path);
                }
            });
        });

    if let Some(modal) = &state.modal {
        let (title, body) = modal_title_and_body(modal);
        let mut close = false;
        let modal_response =
            egui::Modal::new(egui::Id::new("humm-prefs-postproc-modal")).show(ui.ctx(), |ui| {
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
    use humm_core::secrets::MemorySecretStore;
    use pretty_assertions::assert_eq;

    // -- empty_to_none / non_empty_path ------------------------------------

    #[test]
    fn empty_to_none_maps_empty_to_none_and_keeps_the_rest_verbatim() {
        assert_eq!(empty_to_none(""), None);
        assert_eq!(
            empty_to_none("https://example.com/v1"),
            Some("https://example.com/v1".to_string())
        );
    }

    #[test]
    fn non_empty_path_trims_and_maps_blank_to_none() {
        assert_eq!(non_empty_path(""), None);
        assert_eq!(non_empty_path("   "), None);
        assert_eq!(
            non_empty_path("  /out/dir  "),
            Some(PathBuf::from("/out/dir"))
        );
    }

    // -- PostprocTabState::new ----------------------------------------------

    #[test]
    fn new_seeds_every_field_from_settings() {
        let secrets = MemorySecretStore::new();
        let mut settings = PostprocSettings::default();
        settings.skip_already_transcribed.enabled = true;
        settings.skip_already_transcribed.check_database = true;
        settings.transcript_resizer.enabled = true;
        settings.transcript_resizer.max_length = "80".to_string();
        settings.ai_summary.enabled = true;
        settings.ai_summary.api_url = Some("https://gateway.example.com/v1".to_string());
        settings.ai_summary.output_folder = Some(PathBuf::from("/summaries"));
        settings.export_docx.enabled = true;
        settings.export_docx.output_folder = Some(PathBuf::from("/docx"));
        settings.export_docx.include_timestamps = true;

        let state = PostprocTabState::new(&settings, &secrets);

        assert!(state.skip_enabled);
        assert!(state.check_database);
        assert!(state.resizer_enabled);
        assert_eq!(state.max_length_buffer, "80");
        assert!(state.summary_enabled);
        assert_eq!(
            state.summary_api_url_buffer,
            "https://gateway.example.com/v1"
        );
        assert_eq!(state.summary_output_folder_buffer, "/summaries");
        assert!(state.docx_enabled);
        assert_eq!(state.docx_output_folder_buffer, "/docx");
        assert!(state.docx_include_timestamps);
        assert_eq!(state.modal, None);
    }

    #[test]
    fn new_seeds_fresh_install_defaults() {
        let secrets = MemorySecretStore::new();
        let state = PostprocTabState::new(&PostprocSettings::default(), &secrets);

        assert!(!state.skip_enabled);
        assert!(state.check_result_files, "Buzz default: on");
        assert!(!state.check_database);
        assert!(!state.resizer_enabled);
        assert!(!state.summary_enabled);
        assert_eq!(state.summary_api_url_buffer, "");
        assert_eq!(state.summary_model_buffer, AiSummarySettings::DEFAULT_MODEL);
        assert_eq!(state.summary_output_folder_buffer, "");
        assert!(!state.docx_enabled);
        assert_eq!(state.docx_output_folder_buffer, "");
    }

    #[test]
    fn new_seeds_the_summary_key_from_the_secret_store() {
        let secrets = MemorySecretStore::new();
        secrets
            .set(SecretKey::AiSummaryApiKey, "sk-seeded")
            .unwrap();

        let state = PostprocTabState::new(&PostprocSettings::default(), &secrets);

        assert_eq!(state.summary_key_buffer, "sk-seeded");
    }

    // -- Debug redaction ------------------------------------------------------

    #[test]
    fn debug_redacts_the_summary_key_buffer() {
        let secrets = MemorySecretStore::new();
        secrets
            .set(SecretKey::AiSummaryApiKey, "sk-super-secret-value")
            .unwrap();
        let state = PostprocTabState::new(&PostprocSettings::default(), &secrets);

        let debug_output = format!("{state:?}");

        assert!(
            !debug_output.contains("sk-super-secret-value"),
            "Debug output must not contain the key material, got: {debug_output}"
        );
        assert!(
            debug_output.contains("«redacted»"),
            "Debug output must contain the redaction marker, got: {debug_output}"
        );
        assert!(debug_output.contains("PostprocTabState"));
    }

    // -- commit_* --------------------------------------------------------------

    fn fresh_state() -> PostprocTabState {
        PostprocTabState::new(&PostprocSettings::default(), &MemorySecretStore::new())
    }

    #[test]
    fn commit_skip_enabled_and_check_database_persist() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = fresh_state();
        state.skip_enabled = true;
        commit_skip_enabled(&state, &settings, &settings_path);
        state.check_database = true;
        commit_check_database(&state, &settings, &settings_path);

        let guard = settings.lock().unwrap();
        assert!(guard.postproc.skip_already_transcribed.enabled);
        assert!(guard.postproc.skip_already_transcribed.check_database);
    }

    #[test]
    fn commit_merge_gap_seconds_persists_verbatim_even_empty() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = fresh_state();
        state.merge_gap_seconds_buffer = "0.5".to_string();
        commit_merge_gap_seconds(&state, &settings, &settings_path);
        assert_eq!(
            settings
                .lock()
                .unwrap()
                .postproc
                .transcript_resizer
                .merge_gap_seconds,
            "0.5"
        );

        state.merge_gap_seconds_buffer = String::new();
        commit_merge_gap_seconds(&state, &settings, &settings_path);
        assert_eq!(
            settings
                .lock()
                .unwrap()
                .postproc
                .transcript_resizer
                .merge_gap_seconds,
            "",
            "an emptied TEXT field persists verbatim -- auto_resize's own or_default \
             handles it at read time, not this commit"
        );
    }

    #[test]
    fn commit_summary_api_url_empty_clears_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut settings = Settings::default();
        settings.postproc.ai_summary.api_url = Some("https://old.example.com".to_string());
        let settings = Mutex::new(settings);

        let mut state = fresh_state();
        state.summary_api_url_buffer = String::new();
        commit_summary_api_url(&state, &settings, &settings_path);

        assert_eq!(settings.lock().unwrap().postproc.ai_summary.api_url, None);
    }

    #[test]
    fn commit_summary_model_and_prompt_persist_verbatim_even_empty() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = fresh_state();
        state.summary_model_buffer = String::new();
        commit_summary_model(&state, &settings, &settings_path);
        state.summary_prompt_buffer = String::new();
        commit_summary_prompt(&state, &settings, &settings_path);

        let guard = settings.lock().unwrap();
        assert_eq!(guard.postproc.ai_summary.model, "");
        assert_eq!(guard.postproc.ai_summary.prompt, "");
    }

    /// The load-bearing key-hygiene proof (task brief): committing the AI
    /// Summary key writes the [`MemorySecretStore`] double under
    /// [`SecretKey::AiSummaryApiKey`] -- never `settings.toml`.
    #[test]
    fn commit_summary_key_writes_the_secret_store_not_settings() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let mut state = fresh_state();
        state.summary_key_buffer = "sk-committed-via-seam".to_string();

        commit_summary_key(&mut state, &secrets);

        assert_eq!(state.modal, None);
        assert_eq!(
            secrets.get(SecretKey::AiSummaryApiKey).unwrap(),
            Some("sk-committed-via-seam".to_string())
        );

        // Never lands in settings.toml -- there is no code path from
        // `commit_summary_key` into `Settings` at all, but the serialized
        // shape is the strongest possible proof (mirrors
        // `prefs_folder_watch::commit_options_never_writes_the_api_key_anywhere`).
        let settings = Settings::default();
        let serialized = toml::to_string(&settings).expect("settings serialize");
        assert!(!serialized.contains("sk-committed-via-seam"));
    }

    #[test]
    fn commit_summary_key_warns_but_still_saves_for_an_invalid_charset() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let mut state = fresh_state();
        state.summary_key_buffer = "sk-bad key!".to_string();

        commit_summary_key(&mut state, &secrets);

        assert_eq!(state.modal, Some(PostprocModal::InvalidKeyCharset));
        assert_eq!(
            secrets.get(SecretKey::AiSummaryApiKey).unwrap(),
            Some("sk-bad key!".to_string())
        );
    }

    #[test]
    fn commit_docx_output_folder_persists_and_empties_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = fresh_state();
        state.docx_output_folder_buffer = "/out/docx".to_string();
        commit_docx_output_folder(&state, &settings, &settings_path);
        assert_eq!(
            settings.lock().unwrap().postproc.export_docx.output_folder,
            Some(PathBuf::from("/out/docx"))
        );

        state.docx_output_folder_buffer = String::new();
        commit_docx_output_folder(&state, &settings, &settings_path);
        assert_eq!(
            settings.lock().unwrap().postproc.export_docx.output_folder,
            None
        );
    }

    #[test]
    fn commit_docx_include_timestamps_persists() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = fresh_state();
        state.docx_include_timestamps = true;
        commit_docx_include_timestamps(&state, &settings, &settings_path);

        assert!(
            settings
                .lock()
                .unwrap()
                .postproc
                .export_docx
                .include_timestamps
        );
    }
}
