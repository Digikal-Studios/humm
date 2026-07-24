//! The recording window's "Advanced Settings" sheet (F-060, spec §2.1.6/§2.3,
//! task brief): an `egui::Modal` over the recording window, following
//! `views/options_sheet.rs`'s own Modal/Ok-button-box conventions.
//!
//! Ported from Buzz `buzz/widgets/transcriber/advanced_settings_dialog.py`:
//! `_setup_transcription_section`/`_setup_translation_section` (`:59-121`,
//! the initial-prompt row and the LLM-translation block, which this milestone
//! renders permanently **disabled** — F-063 is M6, spec §2.1.6) and
//! `_setup_recording_section` (`:123-255`, the whole recording block) plus
//! every `on_*_changed` handler (`:298-365`).
//!
//! **Save timing (Buzz-exact, task brief's self-review checklist):**
//! `initial_prompt`/`silence_threshold`/`line_separator`/`transcription_step`
//! mutate [`crate::recording::RecordingOptions`] directly — the *same*
//! live object [`crate::recording::RecordingVm::options`] holds, which
//! [`crate::recording::RecordingVm::close`] already persists to
//! `settings.recording` on window close (Buzz `_do_close`, `:1202-1260`) —
//! so a Record click **before** the window closes already sees the edited
//! value (Buzz: `self.transcription_options` is the same object the dialog
//! mutated and the transcriber reads at start, `recording_transcriber_widget.py:667-673`).
//! `mode`/`hide_unconfirmed`/every export field/`upload_url` instead save
//! **immediately** to `settings.recording` on each change (Buzz's own
//! `self.settings.set_value(...)` calls in the same handlers) — this
//! module's [`AdvancedSheetState`] mirrors those for the widget bindings and
//! [`persist_recording`] writes through on every change.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use humm_core::live::RecordingMode;
use humm_core::settings::{RecordingSettings, Settings};

use crate::i18n::tr;
use crate::import::FileDialogs;
use crate::recording::RecordingOptions;

// -- Separator escape/unescape (Buzz `repr()[1:-1]` / `unicode_escape`) ------

/// Escapes `literal` the way Buzz's `repr(text)[1:-1]` displays it
/// (`advanced_settings_dialog.py:149`): backslash, newline, CR, and tab
/// become their two-character escapes; other C0 control characters (and
/// DEL) become `\xHH` (lowercase hex, matching CPython's own `repr`); every
/// other character — including quote characters, which Python's `repr`
/// picks a delimiter to avoid escaping in the common case — passes through
/// unchanged. This covers every practical line-separator value (whitespace
/// runs); a string containing *both* `'` and `"` is the one input where
/// CPython's real `repr` would escape one of them and this function would
/// not — never a realistic separator, so left undone rather than chasing
/// `repr`'s full delimiter-selection algorithm here.
pub fn escape_separator(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    for ch in literal.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// The separator field's display text: [`escape_separator`], falling back to
/// the literal 4-character hint `\n\n` when that's empty (Buzz:
/// `repr(...)[1:-1] or r"\n\n"` — an empty stored separator still shows the
/// default hint rather than a blank field, `:149`).
pub fn display_separator(literal: &str) -> String {
    let escaped = escape_separator(literal);
    if escaped.is_empty() {
        "\\n\\n".to_string()
    } else {
        escaped
    }
}

/// Unescapes `input` the way Buzz's `text.encode().decode("unicode_escape")`
/// does for the practical cases (`advanced_settings_dialog.py:302-307`):
/// `\n \r \t \\ \' \"` map to their characters, `\xHH`/`\uHHHH` map to the
/// codepoint the two/four hex digits spell (matching `unicode_escape`'s
/// latin-1-style byte semantics for `\x`, direct codepoint for `\u`), and any
/// other `\<char>` passes through **literally** — CPython's `unicode_escape`
/// codec does not error on an unrecognized escape letter, it keeps the
/// backslash and the character (verified against the real interpreter:
/// `"\z".encode().decode("unicode_escape") == "\\z"`). Returns `None` (Buzz's
/// `except UnicodeDecodeError: return` — keep the old value silently) only
/// for a trailing lone backslash or a truncated/invalid `\x`/`\u` escape.
/// Octal escapes, `\a`/`\b`/`\f`/`\v`, `\U########`, and `\N{...}` are not
/// implemented (never appear in a realistic line separator) and are treated
/// like any other unrecognized escape: kept literal, not an error — a
/// deliberate, documented scope reduction (task brief: "if cheap").
///
/// Operates on `char`s, not the raw UTF-8 bytes Python's `encode()` first
/// produces — identical to Python for the ASCII escapes this field actually
/// needs; a separator containing non-ASCII characters is an edge case Python
/// itself mangles here (`unicode_escape` decodes byte-by-byte), so this is
/// not a regression in practice.
pub fn unescape_separator(input: &str) -> Option<String> {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            None => return None,
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('x') => {
                let hi = chars.next().and_then(|c| c.to_digit(16));
                let lo = chars.next().and_then(|c| c.to_digit(16));
                match (hi, lo) {
                    (Some(h), Some(l)) => out.push(((h * 16 + l) as u8) as char),
                    _ => return None,
                }
            }
            Some('u') => {
                let mut value: u32 = 0;
                for _ in 0..4 {
                    let digit = chars.next().and_then(|c| c.to_digit(16))?;
                    value = value * 16 + digit;
                }
                out.push(char::from_u32(value)?);
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    Some(out)
}

/// Whether `mode_index` (Buzz's persisted `RECORDING_TRANSCRIBER_MODE`
/// index) resolves to append-and-correct — the single boolean that drives
/// Buzz's `_update_recording_mode_visibility` (`:315-322`): the separator
/// row shows iff **not** this; the transcription-step and hide-unconfirmed
/// rows show iff this. An out-of-range index (never persisted by this UI,
/// but a hand-edited settings file could) falls back to
/// [`RecordingMode::AppendBelow`] via [`RecordingMode::from_index`], i.e.
/// `false` — the separator row, Buzz's own index-0 default.
pub fn is_append_and_correct(mode_index: usize) -> bool {
    RecordingMode::from_index(mode_index) == RecordingMode::AppendAndCorrect
}

// -- Sheet state --------------------------------------------------------------

/// The advanced sheet's live widget-binding state for every **immediate-save**
/// field (Buzz: mode/hide-unconfirmed/export-*/upload-url, each written to
/// `settings.recording` the instant it changes — module doc). Snapshotted
/// from `settings.recording` when the sheet opens ([`Self::from_settings`]);
/// `initial_prompt`/`silence_threshold`/`line_separator`/`transcription_step`
/// are **not** here — the sheet binds those widgets directly to
/// [`RecordingOptions`] instead (module doc's save-timing split).
#[derive(Debug, Clone, PartialEq)]
pub struct AdvancedSheetState {
    /// The live-recording-mode combo's selected index.
    pub mode_index: usize,
    /// The separator field's live, escaped-display text — independent of
    /// [`RecordingOptions::line_separator`] once the sheet is open (Buzz: the
    /// line edit shows whatever was last typed, valid or not; only a
    /// successful unescape updates the underlying value, module doc).
    pub separator_display: String,
    /// The "Hide unconfirmed" checkbox.
    pub hide_unconfirmed: bool,
    /// The "Enable live recording export" checkbox — gates every export
    /// widget below it.
    pub export_enabled: bool,
    /// The export-folder text field's live text.
    pub export_folder: String,
    /// The export-filename-template text field's live text.
    pub export_file_name: String,
    /// The export file-type combo's selected value: `"txt"` or `"csv"`.
    pub export_file_type: String,
    /// The max-export-entries spinner (0 = all, range 0..=99).
    pub export_max_entries: u32,
    /// The upload-URL text field's live text (✚, UX_SPEC J-5).
    pub upload_url: String,
}

impl AdvancedSheetState {
    /// Prefills from `rs` (`settings.recording`, snapshotted when the Advanced
    /// button is clicked) and `line_separator` (the *live*
    /// [`RecordingOptions::line_separator`], not `rs`'s own — the two can
    /// differ while the window has been open a while without a close, module
    /// doc).
    pub fn from_settings(rs: &RecordingSettings, line_separator: &str) -> Self {
        AdvancedSheetState {
            mode_index: rs.mode,
            separator_display: display_separator(line_separator),
            hide_unconfirmed: rs.hide_unconfirmed,
            export_enabled: rs.export_enabled,
            export_folder: rs
                .export_folder
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            export_file_name: rs.export_file_name.clone().unwrap_or_default(),
            export_file_type: rs.export_file_type.clone(),
            export_max_entries: rs.export_max_entries,
            upload_url: rs.upload_url.clone().unwrap_or_default(),
        }
    }
}

/// `s` trimmed and wrapped in `Some`, or `None` if that trims to empty —
/// the same "blank means unset" rule `options_sheet.rs`'s own
/// `non_empty_trimmed` applies to a free-text field.
fn non_empty_trimmed(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// As [`non_empty_trimmed`], but wrapped as a [`PathBuf`] for the export-folder
/// field.
fn non_empty_path(s: &str) -> Option<PathBuf> {
    non_empty_trimmed(s).map(PathBuf::from)
}

/// Locks `settings`, applies `f` to `settings.recording`, and saves — the
/// diff-free, non-fatal-on-failure immediate-save contract shared by every
/// other GUI setting (`views/resizer.rs`'s `persist_create_new` is the
/// precedent this mirrors; unlike that one, every call site here already
/// knows the value changed, so no equality check is needed).
fn persist_recording(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut RecordingSettings),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.recording);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist recording advanced settings: {err}");
    }
}

fn mode_label(mode: RecordingMode) -> String {
    match mode {
        RecordingMode::AppendBelow => tr("recording-advanced-mode-append-below"),
        RecordingMode::AppendAbove => tr("recording-advanced-mode-append-above"),
        RecordingMode::AppendAndCorrect => tr("recording-advanced-mode-append-and-correct"),
    }
}

fn export_file_type_label(file_type: &str) -> String {
    if file_type == "csv" {
        tr("recording-advanced-export-file-type-csv")
    } else {
        tr("recording-advanced-export-file-type-txt")
    }
}

// -- Render ---------------------------------------------------------------

/// Renders the advanced sheet (a no-op while it's closed and `advanced_requested`
/// is false). Opens on `*advanced_requested` (Task 10's seam, consumed here —
/// cleared unconditionally, one-shot), snapshotting `settings.recording` and
/// the live `options.line_separator` into a fresh [`AdvancedSheetState`].
/// Closes on the Ok button, Escape, or a backdrop click (`modal.should_close()`)
/// — Buzz's dialog has no Cancel (`_setup_button_box`, `:257-264`): every
/// change is already live/saved, so closing never discards anything.
#[allow(clippy::too_many_arguments)]
pub fn ui(
    ctx: &egui::Context,
    advanced_requested: &mut bool,
    sheet: &mut Option<AdvancedSheetState>,
    options: &mut RecordingOptions,
    settings: &Mutex<Settings>,
    settings_path: &Path,
    dialogs: &dyn FileDialogs,
) {
    if std::mem::take(advanced_requested) && sheet.is_none() {
        let rs = {
            let guard = settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.recording.clone()
        };
        *sheet = Some(AdvancedSheetState::from_settings(
            &rs,
            &options.line_separator,
        ));
    }
    let Some(state) = sheet.as_mut() else {
        return;
    };

    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("humm-recording-advanced")).show(ctx, |ui| {
        ui.set_min_width(480.0);
        ui.heading(tr("recording-advanced-title"));
        ui.separator();

        heading(ui, "recording-advanced-speech-heading");
        ui.label(tr("recording-advanced-initial-prompt-label"));
        ui.text_edit_multiline(&mut options.initial_prompt);

        ui.add_space(8.0);
        heading(ui, "recording-advanced-translation-heading");
        translation_block(ui);

        ui.add_space(8.0);
        heading(ui, "recording-advanced-recording-heading");
        recording_block(ui, state, options, settings, settings_path, dialogs);

        ui.separator();
        if ui.button(tr("recording-advanced-ok")).clicked() {
            close = true;
        }
    });

    if close || modal.should_close() {
        *sheet = None;
    }
}

fn heading(ui: &mut egui::Ui, key: &str) {
    ui.label(egui::RichText::new(tr(key)).strong());
}

/// The disabled LLM-translation block (spec §2.1.6/§7: F-063 is M6). Every
/// row renders Buzz's exact labels but permanently disabled (the M3
/// Record-button precedent) — no state is modeled or persisted since nothing
/// in this milestone reads it; the text fields bind to per-frame throwaway
/// buffers purely so the widgets render in their Buzz shape (a disabled,
/// empty text box), not because any input is ever accepted.
fn translation_block(ui: &mut egui::Ui) {
    let mut enabled = false;
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut enabled, tr("recording-advanced-llm-enable")),
    )
    .on_disabled_hover_text(tr("recording-advanced-llm-tooltip"));

    ui.horizontal(|ui| {
        ui.add_enabled(
            false,
            egui::Label::new(tr("recording-advanced-llm-model-label")),
        );
        let mut throwaway = String::new();
        ui.add_enabled(false, egui::TextEdit::singleline(&mut throwaway));
    });
    ui.horizontal(|ui| {
        ui.add_enabled(
            false,
            egui::Label::new(tr("recording-advanced-llm-instructions-label")),
        );
        let mut throwaway = String::new();
        ui.add_enabled(false, egui::TextEdit::multiline(&mut throwaway));
    });
}

/// The "Recording settings" block: silence threshold, mode + its
/// mode-dependent rows, the export block, and the upload-URL field.
#[allow(clippy::too_many_arguments)]
fn recording_block(
    ui: &mut egui::Ui,
    state: &mut AdvancedSheetState,
    options: &mut RecordingOptions,
    settings: &Mutex<Settings>,
    settings_path: &Path,
    dialogs: &dyn FileDialogs,
) {
    ui.horizontal(|ui| {
        ui.label(tr("recording-advanced-silence-threshold-label"));
        ui.add(
            egui::DragValue::new(&mut options.silence_threshold)
                .range(0.0..=1.0)
                .speed(0.0005)
                .fixed_decimals(4),
        );
    });

    ui.horizontal(|ui| {
        ui.label(tr("recording-advanced-mode-label"));
        let mut mode_index = state.mode_index;
        egui::ComboBox::from_id_salt("humm-recording-advanced-mode")
            .selected_text(mode_label(RecordingMode::from_index(mode_index)))
            .show_ui(ui, |ui| {
                for (idx, mode) in [
                    RecordingMode::AppendBelow,
                    RecordingMode::AppendAbove,
                    RecordingMode::AppendAndCorrect,
                ]
                .into_iter()
                .enumerate()
                {
                    ui.selectable_value(&mut mode_index, idx, mode_label(mode));
                }
            });
        if mode_index != state.mode_index {
            state.mode_index = mode_index;
            persist_recording(settings, settings_path, |rs| rs.mode = mode_index);
        }
    });

    if is_append_and_correct(state.mode_index) {
        ui.horizontal(|ui| {
            ui.label(tr("recording-advanced-step-label"));
            ui.add(
                egui::DragValue::new(&mut options.transcription_step)
                    .range(
                        RecordingSettings::MIN_TRANSCRIPTION_STEP
                            ..=RecordingSettings::MAX_TRANSCRIPTION_STEP,
                    )
                    .speed(0.1)
                    .fixed_decimals(1),
            );
        });
        let mut hide = state.hide_unconfirmed;
        if ui
            .checkbox(&mut hide, tr("recording-advanced-hide-unconfirmed"))
            .changed()
        {
            state.hide_unconfirmed = hide;
            persist_recording(settings, settings_path, |rs| rs.hide_unconfirmed = hide);
        }
    } else {
        ui.horizontal(|ui| {
            ui.label(tr("recording-advanced-separator-label"));
            let resp = ui.text_edit_singleline(&mut state.separator_display);
            if resp.changed()
                && let Some(literal) = unescape_separator(&state.separator_display)
            {
                options.line_separator = literal;
            }
        });
    }

    let mut export_enabled = state.export_enabled;
    if ui
        .checkbox(&mut export_enabled, tr("recording-advanced-export-enable"))
        .changed()
    {
        state.export_enabled = export_enabled;
        persist_recording(settings, settings_path, |rs| {
            rs.export_enabled = export_enabled
        });
    }

    ui.add_enabled_ui(state.export_enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(tr("recording-advanced-export-folder-label"));
            let resp = ui.text_edit_singleline(&mut state.export_folder);
            if resp.changed() {
                let folder = non_empty_path(&state.export_folder);
                persist_recording(settings, settings_path, |rs| rs.export_folder = folder);
            }
            if ui.button(tr("recording-advanced-export-browse")).clicked() {
                let start = non_empty_path(&state.export_folder);
                if let Some(picked) = dialogs.pick_folder(start.as_deref()) {
                    state.export_folder = picked.to_string_lossy().into_owned();
                    persist_recording(settings, settings_path, |rs| {
                        rs.export_folder = Some(picked)
                    });
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label(tr("recording-advanced-export-file-name-label"));
            let resp = ui.text_edit_singleline(&mut state.export_file_name);
            if resp.changed() {
                let name = non_empty_trimmed(&state.export_file_name);
                persist_recording(settings, settings_path, |rs| rs.export_file_name = name);
            }
        });

        ui.horizontal(|ui| {
            ui.label(tr("recording-advanced-export-file-type-label"));
            let mut file_type = state.export_file_type.clone();
            egui::ComboBox::from_id_salt("humm-recording-advanced-export-type")
                .selected_text(export_file_type_label(&file_type))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut file_type,
                        "txt".to_string(),
                        tr("recording-advanced-export-file-type-txt"),
                    );
                    ui.selectable_value(
                        &mut file_type,
                        "csv".to_string(),
                        tr("recording-advanced-export-file-type-csv"),
                    );
                });
            if file_type != state.export_file_type {
                state.export_file_type = file_type.clone();
                persist_recording(settings, settings_path, |rs| {
                    rs.export_file_type = file_type
                });
            }
        });

        ui.horizontal(|ui| {
            ui.label(tr("recording-advanced-export-max-entries-label"));
            let mut max_entries = state.export_max_entries;
            if ui
                .add(egui::DragValue::new(&mut max_entries).range(0..=99))
                .changed()
            {
                state.export_max_entries = max_entries;
                persist_recording(settings, settings_path, |rs| {
                    rs.export_max_entries = max_entries
                });
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label(tr("recording-advanced-upload-url-label"));
        let resp = ui.text_edit_singleline(&mut state.upload_url);
        if resp.changed() {
            let url = non_empty_trimmed(&state.upload_url);
            persist_recording(settings, settings_path, |rs| rs.upload_url = url);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- escape_separator / display_separator --------------------------------

    #[test]
    fn escapes_the_default_separator_to_the_double_backslash_n_display() {
        assert_eq!(escape_separator("\n\n"), "\\n\\n");
    }

    #[test]
    fn escape_passes_through_plain_text_unchanged() {
        assert_eq!(escape_separator("---"), "---");
    }

    #[test]
    fn escape_handles_backslash_cr_tab() {
        assert_eq!(escape_separator("a\\b\rc\td"), "a\\\\b\\rc\\td");
    }

    #[test]
    fn display_separator_falls_back_to_the_hint_when_empty() {
        assert_eq!(display_separator(""), "\\n\\n");
    }

    #[test]
    fn display_separator_matches_escape_when_non_empty() {
        assert_eq!(display_separator("\n"), "\\n");
    }

    // -- unescape_separator ---------------------------------------------------

    #[test]
    fn unescape_round_trips_the_default_separator() {
        assert_eq!(unescape_separator("\\n\\n"), Some("\n\n".to_string()));
    }

    #[test]
    fn unescape_handles_backslash_cr_tab_quotes() {
        assert_eq!(
            unescape_separator("a\\\\b\\rc\\td\\'e\\\"f"),
            Some("a\\b\rc\td'e\"f".to_string())
        );
    }

    #[test]
    fn unescape_handles_hex_and_unicode_escapes() {
        assert_eq!(unescape_separator("\\x41"), Some("A".to_string()));
        assert_eq!(unescape_separator("\\u0041"), Some("A".to_string()));
    }

    #[test]
    fn unescape_keeps_an_unrecognized_escape_literal() {
        // Matches CPython: "\z".encode().decode("unicode_escape") == "\\z".
        assert_eq!(unescape_separator("\\z"), Some("\\z".to_string()));
    }

    #[test]
    fn unescape_rejects_a_truncated_hex_escape() {
        assert_eq!(unescape_separator("\\xZZ"), None);
        assert_eq!(unescape_separator("\\x4"), None);
    }

    #[test]
    fn unescape_rejects_a_trailing_lone_backslash() {
        assert_eq!(unescape_separator("abc\\"), None);
    }

    #[test]
    fn unescape_plain_text_is_unchanged() {
        assert_eq!(unescape_separator("plain"), Some("plain".to_string()));
    }

    /// Invalid-keeps-old: the sheet's own render logic only overwrites
    /// `options.line_separator` when `unescape_separator` returns `Some` —
    /// this pins the pure contract that makes that true; the render-level
    /// behavior itself is kittest-covered.
    #[test]
    fn invalid_escape_signals_keep_old_via_none() {
        let old = "\n\n".to_string();
        let mut current = old.clone();
        if let Some(literal) = unescape_separator("\\xZZ") {
            current = literal;
        }
        assert_eq!(
            current, old,
            "invalid input must not overwrite the old value"
        );
    }

    // -- is_append_and_correct (mode-visibility matrix) ------------------------

    #[test]
    fn mode_visibility_matrix() {
        assert!(!is_append_and_correct(0), "Append below -> separator row");
        assert!(!is_append_and_correct(1), "Append above -> separator row");
        assert!(
            is_append_and_correct(2),
            "Append and correct -> step/hide rows"
        );
        assert!(
            !is_append_and_correct(99),
            "out-of-range index falls back to Append below (separator row)"
        );
    }

    // -- AdvancedSheetState::from_settings --------------------------------------

    #[test]
    fn from_settings_prefills_every_immediate_save_field() {
        let rs = RecordingSettings {
            mode: 2,
            hide_unconfirmed: false,
            export_enabled: true,
            export_folder: Some(PathBuf::from("/tmp/out")),
            export_file_name: Some("custom".to_string()),
            export_file_type: "csv".to_string(),
            export_max_entries: 7,
            upload_url: Some("http://example.com".to_string()),
            ..RecordingSettings::default()
        };
        let state = AdvancedSheetState::from_settings(&rs, "\n\n");

        assert_eq!(state.mode_index, 2);
        assert_eq!(state.separator_display, "\\n\\n");
        assert!(!state.hide_unconfirmed);
        assert!(state.export_enabled);
        assert_eq!(state.export_folder, "/tmp/out");
        assert_eq!(state.export_file_name, "custom");
        assert_eq!(state.export_file_type, "csv");
        assert_eq!(state.export_max_entries, 7);
        assert_eq!(state.upload_url, "http://example.com");
    }

    #[test]
    fn from_settings_uses_the_live_line_separator_not_the_settings_one() {
        // The live `options.line_separator` can differ from `settings.recording`'s
        // own copy while the window has been open a while without a close
        // (module doc) -- `from_settings` must display the *live* value.
        let rs = RecordingSettings::default();
        let state = AdvancedSheetState::from_settings(&rs, "---");
        assert_eq!(state.separator_display, "---");
    }

    #[test]
    fn from_settings_on_defaults_matches_buzz() {
        let state = AdvancedSheetState::from_settings(&RecordingSettings::default(), "\n\n");
        assert_eq!(state.mode_index, 0);
        assert_eq!(state.separator_display, "\\n\\n");
        assert!(state.hide_unconfirmed);
        assert!(!state.export_enabled);
        assert_eq!(state.export_folder, "");
        assert_eq!(state.export_file_name, "");
        assert_eq!(state.export_file_type, "txt");
        assert_eq!(state.export_max_entries, 0);
        assert_eq!(state.upload_url, "");
    }

    // -- non_empty_trimmed / non_empty_path -------------------------------------

    #[test]
    fn non_empty_trimmed_blanks_to_none() {
        assert_eq!(non_empty_trimmed("   "), None);
        assert_eq!(non_empty_trimmed(" hi "), Some("hi".to_string()));
    }

    #[test]
    fn non_empty_path_blanks_to_none() {
        assert_eq!(non_empty_path("  "), None);
        assert_eq!(non_empty_path("/a/b"), Some(PathBuf::from("/a/b")));
    }
}
