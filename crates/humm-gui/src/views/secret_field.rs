//! Shared masked-secret-field plumbing (Task 13's Preferences → General
//! key row, `views/prefs_general.rs`; Task 14's options-sheet Api Key row,
//! `views/options_sheet.rs`): the Buzz `BASE64_PATTERN` charset check, the
//! render row (masked `TextEdit` + Show/Hide eye toggle,
//! `OpenAIAPIKeyLineEdit` parity), and the commit-to-keyring outcome —
//! factored out here so both call sites share one behavior rather than
//! each porting Buzz's `general_preferences_widget.py`/
//! `openai_api_key_line_edit.py` a second time (design spec §7.2's "reuse
//! Task 13's commit helper" instruction).
//!
//! Every write goes straight through [`SecretStore::set`] — there is no
//! intermediate buffer anywhere that could end up serialized into
//! `settings.toml` or `options_json` (design spec §5's acceptance
//! invariant).

use std::sync::Arc;

use humm_core::secrets::{SecretKey, SecretStore};

use crate::i18n::tr;

/// Buzz's exact key-charset pattern
/// (`general_preferences_widget.py:32`: `BASE64_PATTERN`).
const BASE64_CHARSET_PATTERN: &str = r"^[A-Za-z0-9+/=_-]*$";

/// Lazily compiles [`BASE64_CHARSET_PATTERN`] once and reuses it (same
/// one-`OnceLock` idiom `views/url_dialog.rs`'s `compiled_pattern` uses for
/// its own fixed, non-user-supplied pattern).
fn compiled_charset_pattern() -> &'static regex::Regex {
    static REGEX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    REGEX.get_or_init(|| {
        regex::Regex::new(BASE64_CHARSET_PATTERN).expect("BASE64_CHARSET_PATTERN is a valid regex")
    })
}

/// Whether `s` matches Buzz's base64-charset check
/// (`on_openai_api_key_focus_out`, `:261-267`). An empty string matches (the
/// pattern's own `*` quantifier), same as Buzz's regex.
pub fn is_base64_charset(s: &str) -> bool {
    compiled_charset_pattern().is_match(s)
}

/// The result of [`commit_secret`]: which (if any) modal the caller should
/// show. A keyring-save failure always wins over a charset warning when
/// both would otherwise apply — the two are independent failures with no
/// shared state, and a failed save (the edit wasn't persisted at all) is
/// strictly more actionable than the charset warning (which is advisory
/// only; Buzz saves the value either way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOutcome {
    /// Saved cleanly; no modal.
    Saved,
    /// Saved, but `value` failed [`is_base64_charset`] — warn-only (Buzz
    /// parity: the value is saved regardless).
    InvalidCharset,
    /// [`SecretStore::set`] itself failed; `value` was not persisted.
    KeyringSaveFailed,
}

/// Charset-checks `value` (warn-only, per [`CommitOutcome::InvalidCharset`]'s
/// doc comment), then writes it to `secrets` under `key` regardless of the
/// charset result — mirroring Buzz's `on_openai_api_key_focus_out` (charset
/// warning) plus `on_openai_access_token_changed` (the actual keyring
/// write), folded into one commit step since neither call site here has a
/// separate per-keystroke write to hang the charset check off of (design
/// spec §7.1 ledger #7).
pub fn commit_secret(secrets: &Arc<dyn SecretStore>, key: SecretKey, value: &str) -> CommitOutcome {
    let charset_ok = is_base64_charset(value);
    match secrets.set(key, value) {
        Ok(()) => {
            if charset_ok {
                CommitOutcome::Saved
            } else {
                CommitOutcome::InvalidCharset
            }
        }
        Err(err) => {
            log::warn!("could not save the secret to the keyring: {err}");
            CommitOutcome::KeyringSaveFailed
        }
    }
}

/// Renders one masked-field row: `label`, a password-masked `TextEdit`
/// bound to `buffer`, and a Show/Hide eye-toggle bound to `visible` (Buzz's
/// `OpenAIAPIKeyLineEdit`). Returns the `TextEdit`'s own [`egui::Response`]
/// so the caller decides when to commit (`lost_focus()` — focus-out/Enter,
/// design spec §7.1/§7.2).
pub fn masked_field_row(
    ui: &mut egui::Ui,
    label: String,
    buffer: &mut String,
    visible: &mut bool,
    placeholder: String,
) -> egui::Response {
    let mut text_response = None;
    ui.horizontal(|ui| {
        ui.label(label);
        text_response = Some(
            ui.add(
                egui::TextEdit::singleline(buffer)
                    .password(!*visible)
                    .hint_text(placeholder),
            ),
        );
        let eye_label = if *visible {
            tr("prefs-general-key-hide")
        } else {
            tr("prefs-general-key-show")
        };
        if ui.button(eye_label).clicked() {
            *visible = !*visible;
        }
    });
    text_response.expect("ui.horizontal always invokes its closure synchronously")
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::secrets::MemorySecretStore;
    use pretty_assertions::assert_eq;

    // -- is_base64_charset ----------------------------------------------------

    #[test]
    fn charset_accepts_a_realistic_key() {
        assert!(is_base64_charset("sk-Abc123+/=_-XYZ0123456789abcdefghij"));
    }

    #[test]
    fn charset_accepts_the_empty_string() {
        assert!(is_base64_charset(""));
    }

    #[test]
    fn charset_rejects_a_space() {
        assert!(!is_base64_charset("sk-abc 123"));
    }

    #[test]
    fn charset_rejects_a_hash() {
        assert!(!is_base64_charset("sk-abc#123"));
    }

    #[test]
    fn charset_rejects_non_ascii() {
        assert!(!is_base64_charset("sk-café"));
    }

    // -- commit_secret ----------------------------------------------------------

    #[test]
    fn commit_secret_saves_and_reports_saved_for_a_valid_charset() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let outcome = commit_secret(&secrets, SecretKey::OpenAiApiKey, "sk-Valid123+/=_-");

        assert_eq!(outcome, CommitOutcome::Saved);
        assert_eq!(
            secrets.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("sk-Valid123+/=_-".to_string())
        );
    }

    #[test]
    fn commit_secret_warns_but_still_saves_for_an_invalid_charset() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let outcome = commit_secret(&secrets, SecretKey::OpenAiApiKey, "sk-bad key!");

        assert_eq!(outcome, CommitOutcome::InvalidCharset);
        assert_eq!(
            secrets.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("sk-bad key!".to_string()),
            "an invalid-charset key must still be saved (Buzz parity)"
        );
    }
}
