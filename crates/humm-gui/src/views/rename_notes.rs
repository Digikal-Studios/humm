//! Rename/Notes modals (F-026, Task 8).
//!
//! [`Dialog`] owns whichever of the two is currently open (never both) plus
//! the in-progress text the user is editing; [`ui`] renders it as an
//! `egui::Modal` and returns `Some(`[`DialogOutcome`]`)` the frame the user
//! confirms with something to persist. This module never touches
//! `Services`/the store itself — `app.rs` is the intended consumer: it
//! performs the actual `SqliteStore::set_task_name`/`set_task_notes` write
//! and refreshes the row (`TasksVm::refresh_row`), keeping this module a
//! pure render-plus-local-state function (`ARCHITECTURE.md` §7).

use humm_core::domain::TranscriptionId;

use crate::i18n::tr;

/// Which modal (if any) is open, plus its in-progress text.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Dialog {
    /// No rename/notes modal is open.
    #[default]
    None,
    /// The Rename modal (title "Rename Transcription", label "Enter new
    /// name:"), single-line, prefilled per [`rename_prefill`].
    Rename {
        /// The row being renamed.
        id: TranscriptionId,
        /// The text field's current (editable) contents.
        text: String,
    },
    /// The Notes modal (title "Notes", label "Enter some relevant notes for
    /// this transcription:"), multiline, prefilled with the row's current
    /// notes (empty string if none) — empty is a valid save.
    Notes {
        /// The row whose notes are being edited.
        id: TranscriptionId,
        /// The text field's current (editable) contents.
        text: String,
    },
}

/// What the user confirmed, for `app.rs` to persist. Not emitted for a
/// canceled/escaped/empty-rename close — those simply close the dialog with
/// nothing to do (Buzz parity: `on_rename_action`'s `if ok and
/// new_name.strip():` guard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    /// Rename confirmed with a non-empty, already-trimmed name.
    Rename {
        /// The row that was renamed.
        id: TranscriptionId,
        /// The trimmed, non-empty new name.
        name: String,
    },
    /// Notes confirmed (empty text is a valid, deliberate save — Buzz
    /// parity: notes have no non-empty guard).
    Notes {
        /// The row whose notes were edited.
        id: TranscriptionId,
        /// The (possibly empty) notes text.
        text: String,
    },
}

/// Buzz's Rename prefill order (`on_rename_action`,
/// `transcription_tasks_table_widget.py:642-648`): `name` else `url` else
/// `basename(file)` else empty. This is a genuinely *different* fallback
/// order than the File Name column's own `name else basename(file) else
/// url` (`viewmodel.rs`'s `display_name_for`) — re-checked directly against
/// Buzz source, not a copy-paste of the column logic, because Buzz itself
/// uses two different orders for these two purposes.
pub fn rename_prefill(
    name: Option<&str>,
    url: Option<&str>,
    file: Option<&std::path::Path>,
) -> String {
    if let Some(name) = name.filter(|s| !s.is_empty()) {
        return name.to_string();
    }
    if let Some(url) = url.filter(|s| !s.is_empty()) {
        return url.to_string();
    }
    if let Some(file) = file
        && let Some(base) = file.file_name().and_then(|s| s.to_str())
    {
        return base.to_string();
    }
    String::new()
}

/// Renders whichever dialog `state` currently holds — a no-op returning
/// `None` when it's [`Dialog::None`]. Ok/Cancel/Escape/backdrop-click all
/// close the dialog (`*state = Dialog::None`); only Ok returns `Some`, and
/// only when there's something to persist (a non-empty trimmed rename, or
/// any notes text at all).
pub fn ui(ctx: &egui::Context, state: &mut Dialog) -> Option<DialogOutcome> {
    match std::mem::take(state) {
        Dialog::None => None,
        Dialog::Rename { id, mut text } => {
            let mut outcome = None;
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("humm-rename-dialog")).show(ctx, |ui| {
                ui.heading(tr("rename-title"));
                ui.label(tr("rename-label"));
                ui.text_edit_singleline(&mut text);
                ui.horizontal(|ui| {
                    if ui.button(tr("dialog-ok")).clicked() {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            outcome = Some(DialogOutcome::Rename {
                                id,
                                name: trimmed.to_string(),
                            });
                        }
                        close = true;
                    }
                    if ui.button(tr("dialog-cancel")).clicked() {
                        close = true;
                    }
                });
            });
            if !close && !modal.should_close() {
                *state = Dialog::Rename { id, text };
            }
            outcome
        }
        Dialog::Notes { id, mut text } => {
            let mut outcome = None;
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("humm-notes-dialog")).show(ctx, |ui| {
                ui.heading(tr("notes-title"));
                ui.label(tr("notes-label"));
                ui.text_edit_multiline(&mut text);
                ui.horizontal(|ui| {
                    if ui.button(tr("dialog-ok")).clicked() {
                        outcome = Some(DialogOutcome::Notes {
                            id,
                            text: text.clone(),
                        });
                        close = true;
                    }
                    if ui.button(tr("dialog-cancel")).clicked() {
                        close = true;
                    }
                });
            });
            if !close && !modal.should_close() {
                *state = Dialog::Notes { id, text };
            }
            outcome
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rename_prefill_prefers_name() {
        assert_eq!(
            rename_prefill(
                Some("My Name"),
                Some("https://example.com/a.mp3"),
                Some(&PathBuf::from("/tmp/a.wav"))
            ),
            "My Name"
        );
    }

    #[test]
    fn rename_prefill_falls_back_to_url_before_basename() {
        // The Buzz-verified order: url beats basename(file), unlike the File
        // Name column's own basename-before-url order.
        assert_eq!(
            rename_prefill(
                None,
                Some("https://example.com/a.mp3"),
                Some(&PathBuf::from("/tmp/a.wav"))
            ),
            "https://example.com/a.mp3"
        );
    }

    #[test]
    fn rename_prefill_falls_back_to_basename_when_no_name_or_url() {
        assert_eq!(
            rename_prefill(None, None, Some(&PathBuf::from("/tmp/clip.wav"))),
            "clip.wav"
        );
    }

    #[test]
    fn rename_prefill_empty_name_and_url_are_treated_as_absent() {
        assert_eq!(
            rename_prefill(Some(""), Some(""), Some(&PathBuf::from("/tmp/clip.wav"))),
            "clip.wav"
        );
    }

    #[test]
    fn rename_prefill_all_absent_is_empty() {
        assert_eq!(rename_prefill(None, None, None), "");
    }

    #[test]
    fn dialog_default_is_none() {
        assert_eq!(Dialog::default(), Dialog::None);
    }
}
