//! The URL-import dialog (F-022 GUI half): a single-line URL field validated
//! against Buzz's exact regex, producing a validated [`url::Url`] for the
//! options sheet.
//!
//! Ported from `buzz/widgets/import_url_dialog.py` (54 lines) — see
//! `buzz-notes-import-progress-models.md` §F-022. Buzz's own dialog has no
//! Cancel-vs-close distinction worth porting (`prompt()` returns the url or
//! `None` either way); this module mirrors that: only a successful Ok
//! produces `Some`, everything else (Cancel, Escape, backdrop click) closes
//! with nothing to do.

use crate::i18n::tr;

/// Buzz's URL-validation regex, ported **verbatim**
/// (`import_url_dialog.py:12-14`): the pattern's own `^`/`$` anchors already
/// require a full match, so `Regex::is_match` alone is a full-match check
/// here — no separate anchoring is needed.
const URL_PATTERN: &str = r"^((http|https)://)[-a-zA-Z0-9@:%._\+~#?&//=]{2,256}\.[a-z]{2,6}\b([-a-zA-Z0-9@:%._\+~#?&//=]*)$";

/// Lazily compiles [`URL_PATTERN`] once and reuses it — the pattern is fixed
/// (not user input), so a single process-lifetime compile is safe to share.
fn compiled_pattern() -> &'static regex::Regex {
    static REGEX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    REGEX.get_or_init(|| regex::Regex::new(URL_PATTERN).expect("URL_PATTERN is a valid regex"))
}

/// Validates `s` against Buzz's URL regex. A `true` result does not by
/// itself guarantee `url::Url::parse(s)` succeeds (the regex is looser than
/// full URL grammar) — callers still attempt the real parse and treat its
/// failure the same as a regex rejection (Buzz parity: both paths show the
/// identical "invalid URL" message).
pub fn validate_url(s: &str) -> bool {
    compiled_pattern().is_match(s)
}

/// The dialog's state: closed, or open with its in-progress text and (once a
/// validation attempt has failed) an inline error message.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum UrlDialog {
    /// No URL dialog is open.
    #[default]
    None,
    /// Open, with the field's current text and any validation error from the
    /// last failed Ok attempt.
    Open {
        /// The text field's current (editable) contents.
        text: String,
        /// Set after a failed validation attempt; cleared as soon as the
        /// dialog is reopened fresh via [`UrlDialog::open`].
        error: Option<String>,
    },
}

impl UrlDialog {
    /// Opens the dialog with an empty field and no error — the state every
    /// import-URL request (toolbar/menu/shortcut) opens it in.
    pub fn open() -> Self {
        UrlDialog::Open {
            text: String::new(),
            error: None,
        }
    }
}

/// Renders the URL dialog (a no-op returning `None` when `state` is
/// [`UrlDialog::None`]). Returns `Some(url)` the frame Ok is clicked with
/// text that both passes [`validate_url`] and parses as a real
/// [`url::Url`]; either failure shows the exact Buzz message inline and
/// leaves the dialog open (`url-dialog-invalid`, byte-exact with
/// `import_url_dialog.py`'s `QMessageBox.critical` text). Cancel/Escape/
/// backdrop-click close the dialog with no result.
pub fn ui(ctx: &egui::Context, state: &mut UrlDialog) -> Option<url::Url> {
    let UrlDialog::Open { text, error } = state else {
        return None;
    };

    let mut outcome = None;
    let mut close = false;
    let mut working_text = text.clone();
    let mut working_error = error.clone();

    let modal = egui::Modal::new(egui::Id::new("humm-url-dialog")).show(ctx, |ui| {
        ui.heading(tr("url-dialog-title"));
        ui.horizontal(|ui| {
            ui.label(tr("url-dialog-label"));
            ui.add(
                egui::TextEdit::singleline(&mut working_text)
                    .hint_text(tr("url-dialog-placeholder"))
                    .desired_width(350.0),
            );
        });
        if let Some(message) = &working_error {
            ui.colored_label(egui::Color32::RED, message);
        }
        ui.horizontal(|ui| {
            // Buzz relabels its `QDialogButtonBox` to "Ok"/"Cancel"
            // (`import_url_dialog.py:25-29`) — the same casing as the
            // Clear-History confirm's relabeled buttons, and deliberately
            // *not* "OK" (the Rename/Notes modals' native `QInputDialog`
            // spelling — see `dialog-ok`'s own doc comment in the `.ftl`).
            if ui.button(tr("url-dialog-ok")).clicked() {
                if !validate_url(&working_text) {
                    working_error = Some(tr("url-dialog-invalid"));
                } else {
                    match url::Url::parse(&working_text) {
                        Ok(url) => {
                            outcome = Some(url);
                            close = true;
                        }
                        Err(_) => {
                            working_error = Some(tr("url-dialog-invalid"));
                        }
                    }
                }
            }
            if ui.button(tr("url-dialog-cancel")).clicked() {
                close = true;
            }
        });
    });

    if close || modal.should_close() {
        *state = UrlDialog::None;
    } else {
        *state = UrlDialog::Open {
            text: working_text,
            error: working_error,
        };
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_https_mp3_url() {
        assert!(validate_url("https://example.com/audio.mp3"));
    }

    // The task brief's own accept vector for this case was
    // `http://a.io/x?y=1` — verified against Buzz's regex (directly
    // re-read from `buzz/widgets/import_url_dialog.py:12-14`, byte-identical
    // to `URL_PATTERN` above) that string actually does *not* match: the
    // one-character host label `a` is one short of the pattern's own
    // `{2,256}` minimum before the `.io` TLD (there is exactly one `.` in
    // the string, so no other split point exists either). This is a
    // correction to the brief's test vector, not a regex change — the
    // pattern itself is ported verbatim. `ab.io` (a 2-character host)
    // exercises the same "http + query string" shape while actually
    // satisfying the pattern.
    #[test]
    fn accepts_an_http_url_with_a_query_string() {
        assert!(validate_url("http://ab.io/x?y=1"));
    }

    #[test]
    fn a_one_character_host_label_is_one_short_of_the_minimum_and_is_rejected() {
        assert!(!validate_url("http://a.io/x?y=1"));
    }

    #[test]
    fn rejects_a_non_http_scheme() {
        assert!(!validate_url("ftp://x.com/a"));
    }

    #[test]
    fn rejects_a_url_with_no_scheme() {
        assert!(!validate_url("example.com/a"));
    }

    #[test]
    fn rejects_a_host_with_no_dot() {
        assert!(!validate_url("https://nodot/x"));
    }

    #[test]
    fn rejects_an_empty_string() {
        assert!(!validate_url(""));
    }

    #[test]
    fn dialog_default_is_none() {
        assert_eq!(UrlDialog::default(), UrlDialog::None);
    }

    #[test]
    fn open_starts_with_empty_text_and_no_error() {
        assert_eq!(
            UrlDialog::open(),
            UrlDialog::Open {
                text: String::new(),
                error: None,
            }
        );
    }
}
