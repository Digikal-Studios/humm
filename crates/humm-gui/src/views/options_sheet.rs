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
//! "Advanced Settings" dialog is inlined here as a collapsing section
//! (documented deviation — no OpenAI/LLM-translation rows: those backends
//! aren't ported, ADR-008/Q-2); its "Model:" *type* combo (Whisper/
//! Whisper.cpp/Faster Whisper) is also dropped since humm only ever runs
//! whisper.cpp (ADR-008) — only the *size* combo below is Buzz-parity.

use std::collections::BTreeSet;
use std::str::FromStr;

use humm_core::domain::{
    DEFAULT_TEMPERATURE, Input, Millis, ModelSize, ModelSpec, OutputFormat, OutputSpec, Source,
    TaskKind, TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::language::Language;
use humm_core::settings::FileTranscriberSettings;
use humm_core::viewer::text_view::paragraph_split_from_env;

use crate::i18n::tr;
use crate::import::ImportRequest;

/// The favorite-languages env var (Buzz: `BUZZ_FAVORITE_LANGUAGES`, renamed
/// to humm's own `HUMM_*` env convention — comma-separated codes, invalid
/// codes silently dropped).
const FAVORITE_LANGUAGES_ENV: &str = "HUMM_FAVORITE_LANGUAGES";

/// The sheet's live combo/checkbox/text-field state — everything needed to
/// both render the form and (via [`build_specs`]) produce the
/// [`TaskSpec`]s a Run click enqueues. Deliberately plain data (no `egui`
/// types) so it's unit-testable without a UI context.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetState {
    /// The whisper.cpp model size (title-cased for display).
    pub model_size: ModelSize,
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
            model_size,
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
        }
    }

    /// Writes this state back into `settings` (Buzz parity: `closeEvent`
    /// persists on Run *and* plain close/Cancel). `last_import_folder` is
    /// not touched here — that's persisted at dialog time, not sheet-close
    /// time (binding decision).
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
}

impl OptionsSheet {
    /// Opens a new sheet for `request`, prefilled from `prefill`
    /// (`settings.file_transcriber`) and gated by `custom_model_available`
    /// (`settings.models.custom_model_url.is_some()`).
    pub fn new(
        request: ImportRequest,
        prefill: &FileTranscriberSettings,
        custom_model_available: bool,
    ) -> Self {
        OptionsSheet {
            request,
            state: SheetState::from_settings(prefill, custom_model_available),
        }
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
            size: state.model_size,
            hf_repo: non_empty_trimmed(&state.hf_repo),
        },
        word_timestamps: state.word_timestamps,
        initial_prompt: state.initial_prompt.clone(),
        temperature: DEFAULT_TEMPERATURE.to_vec(),
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
pub fn ui(
    ctx: &egui::Context,
    sheet: &mut Option<OptionsSheet>,
    custom_model_available: bool,
) -> Option<SheetOutcome> {
    let OptionsSheet { request, state } = sheet.as_mut()?;

    let mut outcome = None;
    let mut close = false;

    let modal = egui::Modal::new(egui::Id::new("humm-options-sheet")).show(ctx, |ui| {
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

        render_model_combo(
            ui,
            "humm-options-model-size",
            &mut state.model_size,
            custom_model_available,
        );
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
            ui.label(tr("options-hf-repo-label"));
            ui.text_edit_singleline(&mut state.hf_repo);
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

    if !close && modal.should_close() {
        outcome = Some(SheetOutcome::Cancelled {
            sticky: state.clone(),
        });
        close = true;
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
            model_size: ModelSize::Small,
            task: TaskKind::Transcribe,
            language: None,
            word_timestamps: false,
            initial_prompt: String::new(),
            hf_repo: String::new(),
            formats: BTreeSet::new(),
        }
    }

    // -- from_settings / defaults --------------------------------------------

    #[test]
    fn from_settings_on_a_fresh_install_matches_transcription_options_defaults() {
        let prefill = FileTranscriberSettings::default();
        let state = SheetState::from_settings(&prefill, false);
        assert_eq!(state.model_size, ModelSize::Small);
        assert_eq!(state.task, TaskKind::Transcribe);
        assert_eq!(state.language, None);
        assert!(!state.word_timestamps);
        assert_eq!(state.initial_prompt, "");
        assert_eq!(state.hf_repo, "");
        assert!(state.formats.is_empty());
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
        state.model_size = ModelSize::LargeV3Turbo;
        state.task = TaskKind::Translate;
        state.language = Language::parse("fr");
        state.word_timestamps = true;
        state.initial_prompt = "proper nouns: Buzz, humm".to_string();
        state.hf_repo = "openai/whisper-tiny".to_string();
        state.formats = [OutputFormat::Srt, OutputFormat::Vtt].into_iter().collect();

        let mut settings = FileTranscriberSettings::default();
        state.write_to(&mut settings);
        let round_tripped = SheetState::from_settings(&settings, false);

        assert_eq!(round_tripped, state);
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
}
