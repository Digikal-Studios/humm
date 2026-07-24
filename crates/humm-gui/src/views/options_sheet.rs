//! The per-import options sheet (F-023): one form applying to every file (or
//! the one URL) in the request that opened it, with Buzz's sticky
//! `file_transcriber/*` persistence (prefill on open, write-back on Run
//! *and* Cancel — Buzz saves in `closeEvent` regardless of how the window
//! closed).
//!
//! Ported from `buzz/widgets/transcriber/file_transcriber_widget.py` (the
//! window) and `transcription_options_group_box.py`/
//! `file_transcription_form_widget.py` (the form rows) — see
//! `buzz-notes-options-shortcuts-telemetry.md` §F-023. Buzz's separate
//! "Advanced Settings" dialog is inlined here as a collapsing section. Its
//! "Model:" *type* combo (Whisper/Whisper.cpp/Faster Whisper) is dropped
//! since humm only ever runs whisper.cpp locally (ADR-008) — only the
//! *size* combo below is Buzz-parity there.
//!
//! ## F-041/F-031 additions (Task 14, design spec §7.2)
//!
//! A **backend combo** (`Whisper`/`OpenAI Whisper API`,
//! [`render_backend_combo`]) sits above the size combo — this is humm's own
//! row, not Buzz's "Model:" type combo (that label stays attached to the
//! size combo below, [`render_model_combo`], which is shared verbatim with
//! the recording window's own size row; giving the new row its own label
//! (`options-backend-label`) avoids two identically-labeled rows rather
//! than touching a combo the recording window still renders unchanged this
//! milestone — backend selection there is deferred, design spec §0).
//! Selecting
//! `OpenAI Whisper API` hides the size combo and shows an **Api Key:** row
//! (masked, eye-toggleable, seeded from the keyring, writing straight
//! through on commit — [`crate::views::secret_field`], shared with the
//! Preferences General tab, Task 13); it also hides the Advanced section's
//! Hugging Face repo field (Buzz's own `reset_visible_rows` hides it for
//! every non-local model type, `transcription_options_group_box.py:228-244`
//! — the API backend has no local model to resolve). The Advanced section
//! also gains the **Translation settings** block (F-031,
//! [`render_llm_translation_block`]): an "Enable AI translation" checkbox
//! plus a combo-seeded AI-model field and an instructions textarea, both
//! disabled while unchecked. Unlike Buzz, whose equivalent fields show
//! *display* defaults that never become real values until touched, humm's
//! seeded defaults (`DEFAULT_LLM_MODEL`/`DEFAULT_LLM_PROMPT`) are already
//! live buffer contents — a Run/Cancel with the checkbox on persists
//! whatever's in the field, seed or not (design spec §7.2 ledger #10).

use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;

use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, Input, LlmTranslationOptions, Millis, ModelSize, ModelSpec,
    OutputFormat, OutputSpec, Source, TaskKind, TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::language::Language;
use humm_core::secrets::{SecretKey, SecretStore};
use humm_core::settings::FileTranscriberSettings;
use humm_core::viewer::text_view::paragraph_split_from_env;

use crate::i18n::tr;
use crate::import::ImportRequest;
use crate::views::secret_field::{self, CommitOutcome};

/// The favorite-languages env var (Buzz: `BUZZ_FAVORITE_LANGUAGES`, renamed
/// to humm's own `HUMM_*` env convention — comma-separated codes, invalid
/// codes silently dropped).
const FAVORITE_LANGUAGES_ENV: &str = "HUMM_FAVORITE_LANGUAGES";

/// The AI-model field's preset picker entries (✚ UX_SPEC J-8, design spec
/// §7.2) — Buzz has no equivalent preset list, only a literal
/// `"add-model-id-here"` placeholder in an otherwise-empty field.
const LLM_MODEL_PRESETS: [&str; 3] = ["gpt-4o-mini", "gpt-4o", "gpt-4.1-mini"];

/// The AI-model field's seeded default: [`LLM_MODEL_PRESETS`]'s first entry
/// — unlike Buzz's placeholder, this is a real, usable value from the
/// moment the sheet opens (ledger #10: seeded defaults become real values
/// on save).
const DEFAULT_LLM_MODEL: &str = LLM_MODEL_PRESETS[0];

/// The AI-instructions field's seeded default — the spec's neutral J-8
/// template, byte-exact (not Buzz's English→Spanish sample). `{language}`
/// is literal text the user edits, not a Fluent placeable.
const DEFAULT_LLM_PROMPT: &str = "You are a professional translator. Translate each text sent to \
     you to {language}. Do not add any comments or notes — reply with the translation only.";

/// The sheet's live combo/checkbox/text-field state — everything needed to
/// both render the form and (via [`build_specs`]) produce the
/// [`TaskSpec`]s a Run click enqueues. Deliberately plain data (no `egui`
/// types) so it's unit-testable without a UI context.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetState {
    /// Which engine runs this batch (F-041): local whisper.cpp, or the
    /// OpenAI-compatible HTTP API. Gates the size-combo/Api-Key-row/hf-repo
    /// visibility ([`size_combo_visible`]/[`api_key_row_visible`]/
    /// [`hf_repo_row_visible`]) and maps straight onto `ModelSpec.backend`.
    pub backend: Backend,
    /// The whisper.cpp model size (title-cased for display).
    pub model_size: ModelSize,
    /// The Api Key row's live buffer (F-041, design spec §7.2) — seeded
    /// from the keyring at [`OptionsSheet::new`] time, **never** persisted
    /// by [`SheetState::write_to`]/read by [`SheetState::from_settings`]
    /// (the key travels only through [`humm_core::secrets::SecretStore`],
    /// never through `settings.toml` — design spec §5's acceptance
    /// invariant). Committed on blur/Enter ([`commit_api_key`]), which
    /// writes straight through to the keyring.
    pub api_key: String,
    /// Whether the Api Key row currently renders unmasked (the eye toggle).
    pub api_key_visible: bool,
    /// Transcribe, or translate to English.
    pub task: TaskKind,
    /// Forced source language, or `None` for "Detect Language".
    pub language: Option<Language>,
    /// Whether to request word-level timestamps.
    pub word_timestamps: bool,
    /// Priming text for the model.
    pub initial_prompt: String,
    /// Free-text Hugging Face repo id. Non-empty overrides `model_size` at
    /// resolve time (`ModelSpec.hf_repo`); `model_size` itself is left as
    /// whatever was last selected either way (it's simply ignored while
    /// `hf_repo` is set, not cleared — so switching `hf_repo` back to empty
    /// restores the same size without the user having to re-pick it).
    pub hf_repo: String,
    /// Checked export formats (TXT/SRT/VTT); empty is valid (Run then
    /// produces a task with nothing to export, mirroring
    /// [`OutputSpec::default`]'s own empty-set default).
    pub formats: BTreeSet<OutputFormat>,
    /// The Advanced section's "Enable AI translation" checkbox (F-031,
    /// design spec §7.2). Gates [`llm_options`]: `false` always maps to
    /// `TranscriptionOptions.llm = None` regardless of `llm_model`/
    /// `llm_prompt`.
    pub enable_llm_translation: bool,
    /// The "AI model:" field — seeded with [`DEFAULT_LLM_MODEL`], editable
    /// via free text or the preset picker ([`LLM_MODEL_PRESETS`]). An empty
    /// (trimmed) value also disables translation even when
    /// `enable_llm_translation` is `true` ([`llm_options`]).
    pub llm_model: String,
    /// The "Instructions for AI:" field — seeded with [`DEFAULT_LLM_PROMPT`].
    pub llm_prompt: String,
}

impl SheetState {
    /// Prefills from the last-saved `file_transcriber` settings (Buzz
    /// parity: sheet-open reads sticky prefs), falling back to
    /// [`TranscriptionOptions::default`]'s own values field-by-field for
    /// whatever a first run (or a hand-edited settings file) leaves unset.
    ///
    /// `custom_model_available` gates the persisted size back to `Small` if
    /// it was `Custom` but `settings.models.custom_model_url` is no longer
    /// set (the combo wouldn't even list `Custom` in that case — see
    /// [`selectable_model_sizes`] — so a stale `Custom` prefill must not
    /// silently stick around as an unselectable value).
    pub fn from_settings(prefill: &FileTranscriberSettings, custom_model_available: bool) -> Self {
        let mut model_size = prefill
            .model_size
            .as_deref()
            .and_then(|s| ModelSize::from_str(s).ok())
            .unwrap_or(ModelSize::Small);
        if model_size == ModelSize::Custom && !custom_model_available {
            model_size = ModelSize::Small;
        }

        let formats = prefill
            .export_formats
            .as_ref()
            .map(|list| {
                list.iter()
                    .filter_map(|s| OutputFormat::from_str(s).ok())
                    .collect()
            })
            .unwrap_or_default();

        SheetState {
            backend: prefill
                .backend
                .as_deref()
                .and_then(|s| Backend::from_str(s).ok())
                .unwrap_or_default(),
            model_size,
            // Seeded separately by `OptionsSheet::new` (needs the secret
            // store, which this settings-only prefill has no access to).
            api_key: String::new(),
            api_key_visible: false,
            task: prefill
                .task
                .as_deref()
                .and_then(|s| TaskKind::from_str(s).ok())
                .unwrap_or(TaskKind::Transcribe),
            language: prefill.language.as_deref().and_then(Language::parse),
            word_timestamps: prefill.word_timestamps.unwrap_or(false),
            initial_prompt: prefill.initial_prompt.clone().unwrap_or_default(),
            hf_repo: prefill.hf_repo.clone().unwrap_or_default(),
            formats,
            enable_llm_translation: prefill.enable_llm_translation,
            llm_model: prefill
                .llm_model
                .clone()
                .unwrap_or_else(|| DEFAULT_LLM_MODEL.to_string()),
            llm_prompt: prefill
                .llm_prompt
                .clone()
                .unwrap_or_else(|| DEFAULT_LLM_PROMPT.to_string()),
        }
    }

    /// Writes this state back into `settings` (Buzz parity: `closeEvent`
    /// persists on Run *and* plain close/Cancel). `last_import_folder` is
    /// not touched here — that's persisted at dialog time, not sheet-close
    /// time (binding decision). `api_key` is deliberately **never** written
    /// here — see its field doc comment.
    pub fn write_to(&self, settings: &mut FileTranscriberSettings) {
        settings.language = self.language.map(|language| language.code().to_string());
        settings.task = Some(self.task.as_str().to_string());
        settings.model_size = Some(self.model_size.as_str().to_string());
        settings.hf_repo = non_empty_trimmed(&self.hf_repo);
        settings.word_timestamps = Some(self.word_timestamps);
        settings.initial_prompt = Some(self.initial_prompt.clone());
        settings.export_formats = Some(
            self.formats
                .iter()
                .map(|format| format.as_str().to_string())
                .collect(),
        );
        settings.backend = Some(self.backend.as_str().to_string());
        settings.enable_llm_translation = self.enable_llm_translation;
        settings.llm_model = Some(self.llm_model.clone());
        settings.llm_prompt = Some(self.llm_prompt.clone());
    }
}

/// `s` trimmed and wrapped in `Some`, or `None` if that trims to empty —
/// shared by [`SheetState::write_to`] (the `hf_repo` field) and
/// [`build_specs`] (`ModelSpec.hf_repo`), so both apply the identical
/// "blank means unset" rule to the same free-text field.
fn non_empty_trimmed(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// `state.llm`'s value for [`build_specs`] (F-031, design spec §7.2):
/// `Some` iff the checkbox is on **and** `llm_model` is non-blank once
/// trimmed — matching the task brief's exact gate ("checked + non-empty
/// model"). The prompt is carried verbatim, untrimmed (an instructions
/// textarea's leading/trailing whitespace is inconsequential either way,
/// and this avoids surprising a user who deliberately padded it).
fn llm_options(state: &SheetState) -> Option<LlmTranslationOptions> {
    if !state.enable_llm_translation {
        return None;
    }
    let model = state.llm_model.trim();
    if model.is_empty() {
        return None;
    }
    Some(LlmTranslationOptions {
        model: model.to_string(),
        prompt: state.llm_prompt.clone(),
    })
}

/// Whether the size combo renders (F-041, design spec §7.2): local
/// whisper.cpp only — the API backend has no size to pick. A pure function
/// (rather than inlining the check at the one render call site) so it's
/// unit-testable without an `egui::Context`.
pub(crate) fn size_combo_visible(state: &SheetState) -> bool {
    state.backend == Backend::Whisper
}

/// Whether the Api Key row renders — the mirror image of
/// [`size_combo_visible`] (Buzz `transcription_options_group_box.py:242-244`:
/// `setRowVisible(self.openai_access_token_edit, model_type ==
/// ModelType.OPEN_AI_WHISPER_API)`).
pub(crate) fn api_key_row_visible(state: &SheetState) -> bool {
    state.backend == Backend::OpenAiApi
}

/// Whether the Advanced section's Hugging Face repo field renders: local
/// whisper.cpp only, same gate as [`size_combo_visible`] (Buzz's own
/// `reset_visible_rows` hides `hugging_face_search_line_edit` for every
/// non-local model type too — design spec §7.2 deliverable A's decision to
/// match that intent even though humm's hf field lives in the Advanced
/// section rather than Buzz's main form).
pub(crate) fn hf_repo_row_visible(state: &SheetState) -> bool {
    state.backend == Backend::Whisper
}

/// The sheet's own persistent state: which request it was opened for, plus
/// its live form state. Owned by `ActionState` (`main_window.rs`) — at most
/// one sheet open at a time (Buzz also only ever opens one at a time from a
/// single import path, though its own windows are technically independent;
/// humm simplifies to one, documented).
#[derive(Debug)]
pub struct OptionsSheet {
    /// The files (or URL) this sheet's Run will enqueue.
    pub request: ImportRequest,
    /// The live form state.
    pub state: SheetState,
    /// The Api Key row's commit modal (F-041, design spec §7.2), if a
    /// commit this frame produced one. Lives on the sheet (not
    /// [`SheetState`]) since it's transient render state, not something
    /// [`SheetState::write_to`] ever persists.
    pub modal: Option<SheetModal>,
}

impl OptionsSheet {
    /// Opens a new sheet for `request`, prefilled from `prefill`
    /// (`settings.file_transcriber`) and gated by `custom_model_available`
    /// (`settings.models.custom_model_url.is_some()`). `secrets` seeds the
    /// Api Key row from the keyring (design spec §7.2) — the one thing
    /// [`SheetState::from_settings`] cannot do itself, since it only ever
    /// sees `settings.file_transcriber`.
    pub fn new(
        request: ImportRequest,
        prefill: &FileTranscriberSettings,
        custom_model_available: bool,
        secrets: &dyn SecretStore,
    ) -> Self {
        let mut state = SheetState::from_settings(prefill, custom_model_available);
        state.api_key = secrets
            .get(SecretKey::OpenAiApiKey)
            .ok()
            .flatten()
            .unwrap_or_default();
        OptionsSheet {
            request,
            state,
            modal: None,
        }
    }
}

/// Which modal (if any) the options sheet's Api Key row commit produced
/// (F-041, design spec §7.2) — the sheet-local counterpart of
/// `prefs_general::GeneralModal`'s two key-commit variants (its `TestResult`
/// variant has no sheet equivalent; the sheet has no Test button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetModal {
    /// The Api Key row's committed value failed
    /// [`secret_field::is_base64_charset`] — Buzz's "Invalid API key"
    /// warning. Warn-only: the value is saved regardless.
    InvalidKeyCharset,
    /// [`SecretStore::set`] failed while committing the Api Key row —
    /// Buzz's keyring-save-failure error dialog.
    KeyringSaveFailed,
}

fn sheet_modal_title_and_body(modal: SheetModal) -> (String, String) {
    match modal {
        SheetModal::InvalidKeyCharset => (
            tr("prefs-general-invalid-key-title"),
            tr("prefs-general-invalid-key-body"),
        ),
        SheetModal::KeyringSaveFailed => (
            tr("prefs-general-key-save-error-title"),
            tr("prefs-general-key-save-error-body"),
        ),
    }
}

/// Commits the Api Key row: charset-checks + writes through to `secrets`
/// via [`secret_field::commit_secret`] (shared with the Preferences General
/// tab's key field, Task 13), mapping the outcome onto [`SheetModal`].
fn commit_api_key(value: &str, secrets: &Arc<dyn SecretStore>) -> Option<SheetModal> {
    match secret_field::commit_secret(secrets, SecretKey::OpenAiApiKey, value) {
        CommitOutcome::Saved => None,
        CommitOutcome::InvalidCharset => Some(SheetModal::InvalidKeyCharset),
        CommitOutcome::KeyringSaveFailed => Some(SheetModal::KeyringSaveFailed),
    }
}

/// What the sheet produced when it closed this frame — always paired with
/// the final [`SheetState`] (`sticky`) so the caller can persist it
/// regardless of Run vs. Cancel/Escape/backdrop-click (Buzz's `closeEvent`
/// saves prefs unconditionally).
#[derive(Debug)]
pub enum SheetOutcome {
    /// Run was clicked (or Enter, with no text field focused): one
    /// `(id, spec, source)` per file, or a single one for a URL request.
    Run {
        /// The tasks to enqueue, in request order.
        specs: Vec<(TranscriptionId, TaskSpec, Source)>,
        /// The final form state, to persist as the new sticky prefs.
        sticky: SheetState,
    },
    /// Cancel, Escape, or a backdrop click closed the sheet with nothing to
    /// run.
    Cancelled {
        /// The final form state, to persist as the new sticky prefs (Buzz
        /// parity: `closeEvent` saves even on a plain close).
        sticky: SheetState,
    },
}

/// Builds one [`TaskSpec`] per file in `request` (or the single URL task),
/// all sharing the same [`TranscriptionOptions`]/[`OutputSpec`] derived from
/// `state`. Pure and `egui`-free, so it's directly unit-testable against
/// synthetic sheet state without ever rendering anything.
///
/// `output.paragraph_split` is read fresh from `HUMM_PARAGRAPH_SPLIT_TIME`
/// here, at spec-build time — exactly like `humm-cli`'s `run.rs`, which sets
/// the same field from the same [`paragraph_split_from_env`] call (queue-path
/// parity, HUMM_SURFACE §5: previously this fell through to
/// `OutputSpec::default()`'s hardcoded 2000 ms and silently ignored the env
/// override for every queue-enqueued task, even though the viewer's own
/// Text-mode display and TXT export already read it fresh). One shared
/// source: this function, the CLI, and the viewer's export menu
/// (`export_menu::export_entry`) all call the very same core helper rather
/// than each re-deriving the "env or 2000 ms" rule.
pub fn build_specs(
    request: &ImportRequest,
    state: &SheetState,
) -> Vec<(TranscriptionId, TaskSpec, Source)> {
    build_specs_with_paragraph_split(request, state, paragraph_split_from_env())
}

/// Value-threaded core of [`build_specs`]: unit-testable without touching the
/// real process env (this crate's lib tests run in parallel and several
/// *other* tests call `paragraph_split_from_env()` too, so a real
/// `std::env::set_var` here would be a genuine flaky-test hazard — the same
/// "no env mutation in tests" convention `export_menu::write_export_file`
/// and `humm_core::paths`'s "Global Constraint 9" doc comment establish).
fn build_specs_with_paragraph_split(
    request: &ImportRequest,
    state: &SheetState,
    paragraph_split: Millis,
) -> Vec<(TranscriptionId, TaskSpec, Source)> {
    let options = TranscriptionOptions {
        language: state.language,
        task: state.task,
        model: ModelSpec {
            backend: state.backend,
            size: state.model_size,
            hf_repo: non_empty_trimmed(&state.hf_repo),
        },
        word_timestamps: state.word_timestamps,
        initial_prompt: state.initial_prompt.clone(),
        temperature: DEFAULT_TEMPERATURE.to_vec(),
        llm: llm_options(state),
    };
    let output = OutputSpec {
        formats: state.formats.clone(),
        paragraph_split,
        ..OutputSpec::default()
    };

    match request {
        ImportRequest::Files(paths) => paths
            .iter()
            .map(|path| {
                (
                    TranscriptionId::new(),
                    TaskSpec {
                        input: Input::Path(path.clone()),
                        options: options.clone(),
                        output: output.clone(),
                    },
                    Source::FileImport,
                )
            })
            .collect(),
        ImportRequest::Url(url) => vec![(
            TranscriptionId::new(),
            TaskSpec {
                input: Input::Url(url.clone()),
                options,
                output,
            },
            Source::UrlImport,
        )],
    }
}

/// The 12 non-Custom, non-Lumii model sizes, in [`ModelSize`]'s own
/// declaration order, plus `Custom` appended iff `custom_available` (Buzz:
/// `settings.models.custom_model_url.is_some()`). `Lumii` is never listed
/// (en-locale-only per the binding decision — Buzz itself gates it to the
/// `lv_LV` UI locale, which this milestone never is).
///
/// `pub(crate)`: also used by [`crate::models_ui::refresh`] (Preferences →
/// Models, F-044) — the same fixed size set applies there, since it's the
/// same Buzz gate (Custom row only when configured, Lumii never), just
/// listing on-disk state rather than a picker.
pub(crate) fn selectable_model_sizes(custom_available: bool) -> Vec<ModelSize> {
    let mut sizes = vec![
        ModelSize::Tiny,
        ModelSize::TinyEn,
        ModelSize::Base,
        ModelSize::BaseEn,
        ModelSize::Small,
        ModelSize::SmallEn,
        ModelSize::Medium,
        ModelSize::MediumEn,
        ModelSize::Large,
        ModelSize::LargeV2,
        ModelSize::LargeV3,
        ModelSize::LargeV3Turbo,
    ];
    if custom_available {
        sizes.push(ModelSize::Custom);
    }
    sizes
}

/// Title-cases `s` the way Python's `str.title()` does (Buzz's own display
/// convention for model sizes, `models_preferences_widget.py`): every
/// alphanumeric run starts with an uppercase letter, continuing lowercase;
/// any other character (`.`, `-`) is left as-is and starts a new run, e.g.
/// `"large-v3-turbo"` → `"Large-V3-Turbo"`.
///
/// `pub(crate)`: also used by [`crate::models_ui`] for the Preferences
/// window's Models tab, which wants the identical Buzz-parity display
/// convention for the same [`ModelSize`] values.
pub(crate) fn title_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            if capitalize_next {
                result.extend(ch.to_uppercase());
                capitalize_next = false;
            } else {
                result.extend(ch.to_lowercase());
            }
        } else {
            result.push(ch);
            capitalize_next = true;
        }
    }
    result
}

/// Parses `HUMM_FAVORITE_LANGUAGES` (comma-separated codes; invalid codes
/// silently dropped — Buzz parity, `BUZZ_FAVORITE_LANGUAGES`) into the
/// language-combo's favorites block. Empty (env unset, or nothing valid) —
/// the combo then renders with no favorites block at all.
fn favorite_languages() -> Vec<Language> {
    std::env::var(FAVORITE_LANGUAGES_ENV)
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(|code| Language::parse(code.trim()))
                .collect()
        })
        .unwrap_or_default()
}

/// The header text: `"N file(s)"` for a files request (pluralized: `"1
/// file"`/`"2 files"`), or the URL itself.
fn header_text(request: &ImportRequest) -> String {
    match request {
        ImportRequest::Files(paths) => {
            let n = paths.len();
            format!("{n} file{}", if n == 1 { "" } else { "s" })
        }
        ImportRequest::Url(url) => url.to_string(),
    }
}

/// The one-line live summary (e.g. `"Whisper · small · auto language ·
/// SRT+TXT"`), built fresh from `state` every frame — never persisted, purely
/// a render-time readout of the choices below it.
fn summary_text(state: &SheetState) -> String {
    let model_part = match non_empty_trimmed(&state.hf_repo) {
        Some(repo) => repo,
        None => title_case(state.model_size.as_str()),
    };
    let language_part = state
        .language
        .map(|language| language.english_name().to_string())
        .unwrap_or_else(|| tr("language-detect"));
    let formats_part = if state.formats.is_empty() {
        tr("options-summary-no-formats")
    } else {
        state
            .formats
            .iter()
            .map(|format| format.as_str().to_uppercase())
            .collect::<Vec<_>>()
            .join("+")
    };
    format!("Whisper \u{b7} {model_part} \u{b7} {language_part} \u{b7} {formats_part}")
}

/// The model-size combo row, shared verbatim by the options sheet and the
/// recording window (Buzz: both share one `TranscriptionOptionsGroupBox`,
/// `show_recording_settings` just adds rows — the size combo itself is
/// identical either way). `id_salt` keeps the two call sites' widget ids
/// distinct (they can render in the same frame — the sheet is a modal over
/// the main window, the recording window is a separate viewport).
pub(crate) fn render_model_combo(
    ui: &mut egui::Ui,
    id_salt: &str,
    model_size: &mut ModelSize,
    custom_available: bool,
) {
    ui.horizontal(|ui| {
        ui.label(tr("options-model-label"));
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(title_case(model_size.as_str()))
            .show_ui(ui, |ui| {
                for size in selectable_model_sizes(custom_available) {
                    ui.selectable_value(model_size, size, title_case(size.as_str()));
                }
            });
    });
}

/// The task combo row, shared by the options sheet and the recording window
/// (see [`render_model_combo`]'s doc comment for why/how these are shared).
pub(crate) fn render_task_combo(ui: &mut egui::Ui, id_salt: &str, task: &mut TaskKind) {
    ui.horizontal(|ui| {
        ui.label(tr("options-task-label"));
        let selected = match *task {
            TaskKind::Translate => tr("task-translate"),
            TaskKind::Transcribe => tr("task-transcribe"),
        };
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                // Buzz order: Translate first, Transcribe second.
                ui.selectable_value(task, TaskKind::Translate, tr("task-translate"));
                ui.selectable_value(task, TaskKind::Transcribe, tr("task-transcribe"));
            });
    });
}

/// The language combo row, shared by the options sheet and the recording
/// window (see [`render_model_combo`]'s doc comment for why/how these are
/// shared) — including the `HUMM_FAVORITE_LANGUAGES` favorites block.
pub(crate) fn render_language_combo(
    ui: &mut egui::Ui,
    id_salt: &str,
    language: &mut Option<Language>,
) {
    ui.horizontal(|ui| {
        ui.label(tr("options-language-label"));
        let selected = language
            .map(|language| language.english_name().to_string())
            .unwrap_or_else(|| tr("language-detect"));
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(language, None, tr("language-detect"));

                let favorites = favorite_languages();
                if !favorites.is_empty() {
                    ui.add_enabled(false, egui::Button::new("-------"));
                    for favorite in &favorites {
                        ui.selectable_value(language, Some(*favorite), favorite.english_name());
                    }
                    ui.add_enabled(false, egui::Button::new("-------"));
                }

                let mut all: Vec<Language> = Language::all().to_vec();
                all.sort_by_key(|language| language.english_name());
                for candidate in all {
                    ui.selectable_value(language, Some(candidate), candidate.english_name());
                }
            });
    });
}

fn checkbox_for_format(
    ui: &mut egui::Ui,
    formats: &mut BTreeSet<OutputFormat>,
    format: OutputFormat,
    label: String,
) {
    let mut checked = formats.contains(&format);
    if ui.checkbox(&mut checked, label).changed() {
        if checked {
            formats.insert(format);
        } else {
            formats.remove(&format);
        }
    }
}

fn render_export_checkboxes(ui: &mut egui::Ui, state: &mut SheetState) {
    ui.horizontal(|ui| {
        ui.label(tr("options-export-label"));
        checkbox_for_format(
            ui,
            &mut state.formats,
            OutputFormat::Txt,
            tr("options-export-txt"),
        );
        checkbox_for_format(
            ui,
            &mut state.formats,
            OutputFormat::Srt,
            tr("options-export-srt"),
        );
        checkbox_for_format(
            ui,
            &mut state.formats,
            OutputFormat::Vtt,
            tr("options-export-vtt"),
        );
    });
}

/// The backend combo row (F-041, design spec §7.2): `Whisper` /
/// `OpenAI Whisper API`, sitting above [`render_model_combo`]'s size row.
/// Sheet-only this milestone (recording-window backend selection is
/// deferred — spec §0) — not shared like `render_model_combo`/
/// `render_task_combo`/`render_language_combo`, so it stays a private `fn`.
fn render_backend_combo(ui: &mut egui::Ui, id_salt: &str, backend: &mut Backend) {
    ui.horizontal(|ui| {
        ui.label(tr("options-backend-label"));
        let selected = match *backend {
            Backend::Whisper => tr("options-backend-whisper"),
            Backend::OpenAiApi => tr("options-backend-openai-api"),
        };
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(backend, Backend::Whisper, tr("options-backend-whisper"));
                ui.selectable_value(
                    backend,
                    Backend::OpenAiApi,
                    tr("options-backend-openai-api"),
                );
            });
    });
}

/// The Api Key row (F-041, design spec §7.2): shown only while
/// [`api_key_row_visible`] — mirrors `prefs_general::ui`'s key row via the
/// shared [`secret_field`] helpers, committing on blur/Enter straight
/// through to `secrets` (the same keyring entry Preferences writes,
/// design spec §7.2 ledger #9). `modal` is [`OptionsSheet::modal`] threaded
/// in separately rather than the whole `&mut OptionsSheet` — [`ui`] already
/// holds `state`/`modal` as separate locals after destructuring the sheet.
fn render_api_key_row(
    ui: &mut egui::Ui,
    state: &mut SheetState,
    modal: &mut Option<SheetModal>,
    secrets: &Arc<dyn SecretStore>,
) {
    let response = secret_field::masked_field_row(
        ui,
        tr("options-api-key-label"),
        &mut state.api_key,
        &mut state.api_key_visible,
        tr("prefs-general-openai-key-placeholder"),
    );
    if response.lost_focus() {
        *modal = commit_api_key(&state.api_key, secrets);
    }
}

/// The Advanced section's "Translation settings" block (F-031, design spec
/// §7.2, Buzz `advanced_settings_dialog.py:75-121`): the enable checkbox,
/// then the AI-model field (a free-text `TextEdit` plus a small preset
/// picker `ComboBox`, ✚ J-8) and the instructions textarea, both disabled
/// while unchecked (Buzz's own `.setEnabled(enable_llm_translation)` on
/// every row below the checkbox).
fn render_llm_translation_block(ui: &mut egui::Ui, state: &mut SheetState) {
    ui.label(egui::RichText::new(tr("options-llm-heading")).strong());
    ui.checkbox(&mut state.enable_llm_translation, tr("options-llm-enable"));

    ui.add_enabled_ui(state.enable_llm_translation, |ui| {
        ui.horizontal(|ui| {
            ui.label(tr("options-llm-model-label"));
            ui.text_edit_singleline(&mut state.llm_model);
            egui::ComboBox::from_id_salt("humm-options-llm-model-presets")
                .selected_text(tr("options-llm-model-presets-button"))
                .show_ui(ui, |ui| {
                    for preset in LLM_MODEL_PRESETS {
                        if ui
                            .selectable_label(state.llm_model == preset, preset)
                            .clicked()
                        {
                            state.llm_model = preset.to_string();
                        }
                    }
                });
        });
        ui.label(tr("options-llm-prompt-label"));
        ui.text_edit_multiline(&mut state.llm_prompt);
    });
}

/// Renders the options sheet (a no-op returning `None` while `sheet` is
/// `None`). Returns `Some(`[`SheetOutcome`]`)` the frame it closes, whether
/// by Run, Cancel, Escape, or a backdrop click — every path carries the
/// final [`SheetState`] so the caller can persist it as the new sticky prefs
/// (Buzz's `closeEvent` parity: saved unconditionally).
///
/// Run also fires on Enter, but only when no text field currently has focus
/// (`ui.memory(|m| m.focused())`) — otherwise Enter reaches the focused
/// `TextEdit` normally (e.g. inserting a newline in the initial-prompt
/// field), matching the brief's documented Enter-vs-text-edit tradeoff:
/// egui has no per-widget "swallow Enter unless I'm focused" primitive, so a
/// global "is anything focused" check is the closest equivalent.
///
/// `secrets` seeds/commits the Api Key row (F-041) — the same store
/// Preferences' General tab reads/writes (`main_window::show`'s own
/// `secrets: &Arc<dyn SecretStore>` parameter, threaded from `app.rs`'s
/// `Services.secrets`).
pub fn ui(
    ctx: &egui::Context,
    sheet: &mut Option<OptionsSheet>,
    custom_model_available: bool,
    secrets: &Arc<dyn SecretStore>,
) -> Option<SheetOutcome> {
    let OptionsSheet {
        request,
        state,
        modal,
    } = sheet.as_mut()?;

    let mut outcome = None;
    let mut close = false;

    let modal_response = egui::Modal::new(egui::Id::new("humm-options-sheet")).show(ctx, |ui| {
        ui.set_min_width(420.0);

        let header = header_text(request);
        match request {
            ImportRequest::Files(paths) => {
                egui::CollapsingHeader::new(header)
                    .id_salt("humm-options-file-list")
                    .show(ui, |ui| {
                        for path in paths.iter() {
                            ui.label(path.display().to_string());
                        }
                    });
            }
            ImportRequest::Url(_) => {
                ui.heading(header);
            }
        }
        ui.label(summary_text(state));
        ui.separator();

        render_backend_combo(ui, "humm-options-backend", &mut state.backend);
        if size_combo_visible(state) {
            render_model_combo(
                ui,
                "humm-options-model-size",
                &mut state.model_size,
                custom_model_available,
            );
        }
        if api_key_row_visible(state) {
            render_api_key_row(ui, state, modal, secrets);
        }
        render_task_combo(ui, "humm-options-task", &mut state.task);
        render_language_combo(ui, "humm-options-language", &mut state.language);
        ui.checkbox(&mut state.word_timestamps, tr("options-word-timestamps"));

        let mut extract_speech = false;
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut extract_speech, tr("options-extract-speech")),
        )
        .on_disabled_hover_text(tr("options-extract-speech-tooltip"));

        render_export_checkboxes(ui, state);

        egui::CollapsingHeader::new(tr("options-advanced")).show(ui, |ui| {
            ui.label(tr("options-initial-prompt-label"));
            ui.text_edit_multiline(&mut state.initial_prompt);
            if hf_repo_row_visible(state) {
                ui.label(tr("options-hf-repo-label"));
                ui.text_edit_singleline(&mut state.hf_repo);
            }
            ui.separator();
            render_llm_translation_block(ui, state);
        });

        ui.separator();

        // Read focus/Enter *before* the buttons below (which would
        // themselves briefly take focus on click) — this reflects whether a
        // *text field* had focus going into this frame's button row.
        let no_text_edit_focused = ui.memory(|memory| memory.focused().is_none());
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));

        ui.horizontal(|ui| {
            let run_clicked = ui.button(tr("options-run")).clicked();
            if run_clicked || (enter_pressed && no_text_edit_focused) {
                outcome = Some(SheetOutcome::Run {
                    specs: build_specs(request, state),
                    sticky: state.clone(),
                });
                close = true;
            }
            if ui.button(tr("dialog-cancel")).clicked() {
                outcome = Some(SheetOutcome::Cancelled {
                    sticky: state.clone(),
                });
                close = true;
            }
        });
    });

    if !close && modal_response.should_close() {
        outcome = Some(SheetOutcome::Cancelled {
            sticky: state.clone(),
        });
        close = true;
    }

    // The Api Key row's own commit-failure modal (F-041) — a second,
    // independent `egui::Modal` stacked over the sheet's, mirroring
    // `prefs_general::ui`'s key-commit modal one level up (that one has no
    // outer modal to stack over). Suppressed the frame the sheet itself
    // closes (Run/Cancel/Escape) since `*sheet` is about to become `None`.
    if !close && let Some(pending) = *modal {
        let (title, body) = sheet_modal_title_and_body(pending);
        let mut modal_close = false;
        let key_modal_response = egui::Modal::new(egui::Id::new("humm-options-sheet-key-modal"))
            .show(ctx, |ui| {
                ui.heading(title);
                ui.label(body);
                if ui.button(tr("dialog-ok")).clicked() {
                    modal_close = true;
                }
            });
        if modal_close || key_modal_response.should_close() {
            *modal = None;
        }
    }

    if close {
        *sheet = None;
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn base_state() -> SheetState {
        SheetState {
            backend: Backend::Whisper,
            model_size: ModelSize::Small,
            api_key: String::new(),
            api_key_visible: false,
            task: TaskKind::Transcribe,
            language: None,
            word_timestamps: false,
            initial_prompt: String::new(),
            hf_repo: String::new(),
            formats: BTreeSet::new(),
            enable_llm_translation: false,
            llm_model: String::new(),
            llm_prompt: String::new(),
        }
    }

    // -- from_settings / defaults --------------------------------------------

    #[test]
    fn from_settings_on_a_fresh_install_matches_transcription_options_defaults() {
        let prefill = FileTranscriberSettings::default();
        let state = SheetState::from_settings(&prefill, false);
        assert_eq!(state.backend, Backend::Whisper);
        assert_eq!(state.model_size, ModelSize::Small);
        assert_eq!(
            state.api_key, "",
            "api_key is never seeded by from_settings"
        );
        assert!(!state.api_key_visible);
        assert_eq!(state.task, TaskKind::Transcribe);
        assert_eq!(state.language, None);
        assert!(!state.word_timestamps);
        assert_eq!(state.initial_prompt, "");
        assert_eq!(state.hf_repo, "");
        assert!(state.formats.is_empty());
        assert!(!state.enable_llm_translation);
        assert_eq!(
            state.llm_model, DEFAULT_LLM_MODEL,
            "the seeded AI-model default"
        );
        assert_eq!(
            state.llm_prompt, DEFAULT_LLM_PROMPT,
            "the seeded AI-instructions default"
        );
    }

    #[test]
    fn from_settings_falls_back_to_small_when_custom_is_stored_but_unavailable() {
        let prefill = FileTranscriberSettings {
            model_size: Some("custom".to_string()),
            ..Default::default()
        };
        let state = SheetState::from_settings(&prefill, false);
        assert_eq!(state.model_size, ModelSize::Small);
    }

    #[test]
    fn from_settings_keeps_custom_when_available() {
        let prefill = FileTranscriberSettings {
            model_size: Some("custom".to_string()),
            ..Default::default()
        };
        let state = SheetState::from_settings(&prefill, true);
        assert_eq!(state.model_size, ModelSize::Custom);
    }

    // -- stickiness round-trip ------------------------------------------------

    #[test]
    fn state_round_trips_through_settings() {
        let mut state = base_state();
        state.backend = Backend::OpenAiApi;
        state.model_size = ModelSize::LargeV3Turbo;
        state.task = TaskKind::Translate;
        state.language = Language::parse("fr");
        state.word_timestamps = true;
        state.initial_prompt = "proper nouns: Buzz, humm".to_string();
        state.hf_repo = "openai/whisper-tiny".to_string();
        state.formats = [OutputFormat::Srt, OutputFormat::Vtt].into_iter().collect();
        state.enable_llm_translation = true;
        state.llm_model = "gpt-4o".to_string();
        state.llm_prompt = "Translate to French.".to_string();

        let mut settings = FileTranscriberSettings::default();
        state.write_to(&mut settings);
        let round_tripped = SheetState::from_settings(&settings, false);

        assert_eq!(round_tripped, state);
    }

    /// Dedicated round-trip test for the backend combo alone (task brief's
    /// "backend sticky round-trip"), independent of every other field the
    /// broader [`state_round_trips_through_settings`] also exercises.
    #[test]
    fn backend_round_trips_through_settings() {
        for backend in [Backend::Whisper, Backend::OpenAiApi] {
            let mut state = base_state();
            state.backend = backend;

            let mut settings = FileTranscriberSettings::default();
            state.write_to(&mut settings);
            assert_eq!(settings.backend.as_deref(), Some(backend.as_str()));

            let round_tripped = SheetState::from_settings(&settings, false);
            assert_eq!(round_tripped.backend, backend);
        }
    }

    #[test]
    fn write_to_persists_none_language_as_none_for_detect() {
        let state = base_state();
        let mut settings = FileTranscriberSettings::default();
        state.write_to(&mut settings);
        assert_eq!(settings.language, None);
    }

    #[test]
    fn write_to_persists_blank_hf_repo_as_none() {
        let mut state = base_state();
        state.hf_repo = "   ".to_string();
        let mut settings = FileTranscriberSettings::default();
        state.write_to(&mut settings);
        assert_eq!(settings.hf_repo, None);
    }

    #[test]
    fn write_to_never_persists_the_api_key() {
        let mut state = base_state();
        state.api_key = "sk-super-secret-value".to_string();
        let mut settings = FileTranscriberSettings::default();
        state.write_to(&mut settings);

        // `FileTranscriberSettings` has no `api_key` field at all -- the
        // strongest possible proof, but also assert the serialized TOML
        // (what actually lands on disk) never contains the value, in case a
        // future field addition accidentally carries it.
        let serialized = toml::to_string(&settings).expect("settings serialize");
        assert!(
            !serialized.contains("sk-super-secret-value"),
            "the API key must never reach settings.toml (design spec §5)"
        );
    }

    // -- build_specs -----------------------------------------------------------

    #[test]
    fn build_specs_makes_one_spec_per_file_sharing_the_same_options() {
        let mut state = base_state();
        state.formats = [OutputFormat::Srt].into_iter().collect();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav"), PathBuf::from("/b.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs.len(), 2);
        let ids: BTreeSet<_> = specs.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(ids.len(), 2, "each spec must get a fresh, distinct id");

        for (_, spec, source) in &specs {
            assert_eq!(*source, Source::FileImport);
            assert_eq!(
                spec.output.formats,
                [OutputFormat::Srt].into_iter().collect()
            );
        }
        assert_eq!(specs[0].1.input, Input::Path(PathBuf::from("/a.wav")));
        assert_eq!(specs[1].1.input, Input::Path(PathBuf::from("/b.wav")));
    }

    #[test]
    fn build_specs_makes_a_single_url_import_spec() {
        let state = base_state();
        let url = url::Url::parse("https://example.com/a.mp3").unwrap();
        let request = ImportRequest::Url(url.clone());

        let specs = build_specs(&request, &state);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].1.input, Input::Url(url));
        assert_eq!(specs[0].2, Source::UrlImport);
    }

    #[test]
    fn build_specs_maps_non_empty_hf_repo_onto_model_spec() {
        let mut state = base_state();
        state.hf_repo = "openai/whisper-tiny".to_string();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(
            specs[0].1.options.model.hf_repo,
            Some("openai/whisper-tiny".to_string())
        );
    }

    #[test]
    fn build_specs_leaves_hf_repo_none_when_blank() {
        let state = base_state();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs[0].1.options.model.hf_repo, None);
    }

    #[test]
    fn build_specs_language_is_none_when_detect_is_selected() {
        let state = base_state(); // language: None ("Detect Language")
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs[0].1.options.language, None);
    }

    #[test]
    fn build_specs_uses_the_default_temperature_schedule() {
        let state = base_state();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs[0].1.options.temperature, DEFAULT_TEMPERATURE.to_vec());
    }

    // -- backend / LLM mapping (F-041, F-031, Task 14) -------------------------

    #[test]
    fn build_specs_maps_api_backend() {
        let mut state = base_state();
        state.backend = Backend::OpenAiApi;
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs[0].1.options.model.backend, Backend::OpenAiApi);
    }

    #[test]
    fn build_specs_maps_whisper_backend_by_default() {
        let state = base_state();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs[0].1.options.model.backend, Backend::Whisper);
    }

    #[test]
    fn build_specs_maps_llm_options_when_enabled() {
        let mut state = base_state();
        state.enable_llm_translation = true;
        state.llm_model = "gpt-4o-mini".to_string();
        state.llm_prompt = "Translate to German.".to_string();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(
            specs[0].1.options.llm,
            Some(LlmTranslationOptions {
                model: "gpt-4o-mini".to_string(),
                prompt: "Translate to German.".to_string(),
            })
        );
    }

    #[test]
    fn llm_none_when_unchecked() {
        let mut state = base_state();
        state.enable_llm_translation = false;
        state.llm_model = "gpt-4o-mini".to_string();
        state.llm_prompt = "Translate to German.".to_string();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs[0].1.options.llm, None);
    }

    #[test]
    fn llm_none_when_model_empty() {
        let mut state = base_state();
        state.enable_llm_translation = true;
        state.llm_model = String::new();
        state.llm_prompt = "Translate to German.".to_string();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(
            specs[0].1.options.llm, None,
            "an enabled checkbox with a blank model must still map to None"
        );
    }

    #[test]
    fn llm_none_when_model_is_only_whitespace() {
        let mut state = base_state();
        state.enable_llm_translation = true;
        state.llm_model = "   ".to_string();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(specs[0].1.options.llm, None);
    }

    /// Design spec §7.2 ledger #10: a sheet opened fresh (never touched)
    /// seeds the AI-model/instructions fields with real, usable defaults —
    /// checking the box and running immediately must persist those seeds
    /// verbatim as `options.llm`, unlike Buzz where the same-looking text is
    /// a display-only placeholder until edited.
    #[test]
    fn seeded_defaults_become_values_on_save() {
        let prefill = FileTranscriberSettings::default();
        let mut state = SheetState::from_settings(&prefill, false);
        state.enable_llm_translation = true; // the only field the user "touches"
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let specs = build_specs(&request, &state);
        assert_eq!(
            specs[0].1.options.llm,
            Some(LlmTranslationOptions {
                model: DEFAULT_LLM_MODEL.to_string(),
                prompt: DEFAULT_LLM_PROMPT.to_string(),
            })
        );

        // And the sticky write-back persists those same seeded strings, not
        // an empty/unset placeholder (ledger #10's "real value" claim).
        let mut settings = FileTranscriberSettings::default();
        state.write_to(&mut settings);
        assert_eq!(settings.llm_model.as_deref(), Some(DEFAULT_LLM_MODEL));
        assert_eq!(settings.llm_prompt.as_deref(), Some(DEFAULT_LLM_PROMPT));
        assert!(settings.enable_llm_translation);
    }

    #[test]
    fn default_llm_prompt_matches_the_spec_j8_template_byte_exact() {
        assert_eq!(
            DEFAULT_LLM_PROMPT,
            "You are a professional translator. Translate each text sent to you to \
             {language}. Do not add any comments or notes — reply with the translation only."
        );
    }

    // -- row visibility (pure fns, F-041, Task 14) -----------------------------

    #[test]
    fn whisper_backend_shows_the_size_combo_and_hf_repo_hides_the_api_key_row() {
        let state = base_state(); // backend: Backend::Whisper
        assert!(size_combo_visible(&state));
        assert!(hf_repo_row_visible(&state));
        assert!(!api_key_row_visible(&state));
    }

    #[test]
    fn api_backend_hides_the_size_combo_and_hf_repo_shows_the_api_key_row() {
        let mut state = base_state();
        state.backend = Backend::OpenAiApi;
        assert!(!size_combo_visible(&state));
        assert!(!hf_repo_row_visible(&state));
        assert!(api_key_row_visible(&state));
    }

    /// HUMM_SURFACE §5 queue-path gap: `build_specs` must read
    /// `HUMM_PARAGRAPH_SPLIT_TIME` at spec-build time, exactly like
    /// `humm-cli`'s `run.rs` does, rather than always taking
    /// `OutputSpec::default()`'s hardcoded 2000 ms. Proven via the
    /// value-threaded seam (`build_specs_with_paragraph_split`) rather than a
    /// real `std::env::set_var` — this crate's lib tests run in parallel and
    /// several *other* tests call `paragraph_split_from_env()` too, so
    /// mutating the real process env here would be a genuine flaky-test
    /// hazard (the same convention `export_menu`'s
    /// `export_txt_honors_paragraph_split_env` and `humm_core::paths`'s
    /// "Global Constraint 9" doc comment establish).
    #[test]
    fn build_specs_threads_a_non_default_paragraph_split() {
        let state = base_state();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);
        let overridden = Millis(500);

        let specs = build_specs_with_paragraph_split(&request, &state, overridden);
        assert_eq!(specs[0].1.output.paragraph_split, overridden);

        // And the default-reading entry point matches the default constant
        // when the env var is genuinely unset in this process.
        if std::env::var_os("HUMM_PARAGRAPH_SPLIT_TIME").is_none() {
            let default_specs = build_specs(&request, &state);
            assert_eq!(
                default_specs[0].1.output.paragraph_split,
                humm_core::export::DEFAULT_PARAGRAPH_SPLIT
            );
        }
    }

    // -- header/summary text ----------------------------------------------------

    #[test]
    fn header_text_pluralizes_file_count() {
        assert_eq!(
            header_text(&ImportRequest::Files(vec![PathBuf::from("/a.wav")])),
            "1 file"
        );
        assert_eq!(
            header_text(&ImportRequest::Files(vec![
                PathBuf::from("/a.wav"),
                PathBuf::from("/b.wav"),
            ])),
            "2 files"
        );
    }

    #[test]
    fn header_text_for_url_is_the_url_itself() {
        let url = url::Url::parse("https://example.com/a.mp3").unwrap();
        assert_eq!(
            header_text(&ImportRequest::Url(url.clone())),
            url.to_string()
        );
    }

    #[test]
    fn title_case_matches_buzz_examples() {
        assert_eq!(title_case("tiny"), "Tiny");
        assert_eq!(title_case("large-v2"), "Large-V2");
        assert_eq!(title_case("large-v3-turbo"), "Large-V3-Turbo");
    }

    // -- selectable_model_sizes --------------------------------------------------

    #[test]
    fn custom_is_listed_only_when_available() {
        assert!(!selectable_model_sizes(false).contains(&ModelSize::Custom));
        assert!(selectable_model_sizes(true).contains(&ModelSize::Custom));
    }

    #[test]
    fn lumii_is_never_listed() {
        assert!(!selectable_model_sizes(false).contains(&ModelSize::Lumii));
        assert!(!selectable_model_sizes(true).contains(&ModelSize::Lumii));
    }

    // -- Api Key row: seeding + commit (F-041, Task 14) ------------------------

    #[test]
    fn options_sheet_new_seeds_the_api_key_from_the_secret_store() {
        use humm_core::secrets::MemorySecretStore;

        let secrets = MemorySecretStore::new();
        secrets
            .set(SecretKey::OpenAiApiKey, "sk-seeded-value")
            .unwrap();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let sheet = OptionsSheet::new(
            request,
            &FileTranscriberSettings::default(),
            false,
            &secrets,
        );

        assert_eq!(sheet.state.api_key, "sk-seeded-value");
        assert_eq!(sheet.modal, None);
    }

    #[test]
    fn options_sheet_new_api_key_is_empty_when_nothing_is_stored() {
        use humm_core::secrets::MemorySecretStore;

        let secrets = MemorySecretStore::new();
        let request = ImportRequest::Files(vec![PathBuf::from("/a.wav")]);

        let sheet = OptionsSheet::new(
            request,
            &FileTranscriberSettings::default(),
            false,
            &secrets,
        );

        assert_eq!(sheet.state.api_key, "");
    }

    #[test]
    fn commit_api_key_saves_and_reports_no_modal_for_a_valid_charset() {
        use humm_core::secrets::MemorySecretStore;

        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let modal = commit_api_key("sk-Valid123+/=_-", &secrets);

        assert_eq!(modal, None);
        assert_eq!(
            secrets.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("sk-Valid123+/=_-".to_string())
        );
    }

    #[test]
    fn commit_api_key_warns_but_still_saves_for_an_invalid_charset() {
        use humm_core::secrets::MemorySecretStore;

        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let modal = commit_api_key("sk-bad key!", &secrets);

        assert_eq!(modal, Some(SheetModal::InvalidKeyCharset));
        assert_eq!(
            secrets.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("sk-bad key!".to_string()),
            "an invalid-charset key must still be saved (Buzz parity)"
        );
    }
}
