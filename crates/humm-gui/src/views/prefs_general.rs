//! Preferences → General tab, OpenAI block only (partial F-100, design spec
//! §7.1). The rest of Buzz's General tab (UI language, font size, export
//! template, live-export relocation, GPU toggles) is M7 — this module renders
//! *only* the four OpenAI rows Buzz's `GeneralPreferencesWidget` places first
//! (`general_preferences_widget.py:101-139`): the API key field with its
//! show/hide eye toggle and Test button, the base-URL field, and the API
//! model field.
//!
//! Ported from Buzz:
//! - `buzz/widgets/preferences_dialog/general_preferences_widget.py`
//!   `BASE64_PATTERN` (`:32`), the row construction (`:101-139`), and every
//!   `on_*` handler (`:230-273`, `ValidateOpenAIApiKeyJob` at `:338-385`).
//! - `buzz/widgets/openai_api_key_line_edit.py` — the masked field + eye
//!   toggle.
//! - `buzz/widgets/main_window.py:323-331` — the keyring-write failure
//!   dialog (`on_openai_access_token_changed`).
//!
//! ## Save timing (a deliberate deviation, spec §7.1 ledger #7)
//!
//! Buzz writes the OS keyring on **every keystroke** (`textChanged` →
//! `on_openai_api_key_changed` → `main_window.on_openai_access_token_changed`
//! → `set_password`). This module instead commits the key field on
//! **focus-out or Enter** ([`commit_key`]) — the same final state, far fewer
//! keyring writes. The charset check Buzz runs on its own separate
//! `focus_out` signal (`:261-267`) is folded into that same commit step here,
//! since humm has no per-keystroke write to hang a separate check off of; the
//! *result* is identical — a value that fails the charset check is still
//! saved, with a warning shown alongside.
//!
//! The base-URL and API-model fields save on every change instead
//! (`resp.changed()`, matching `views/recording_advanced.rs`'s own
//! immediate-save text fields, e.g. its `upload_url` row) — Buzz's own
//! `textChanged`-per-keystroke behavior for these two fields is ported
//! as-is, since neither writes anywhere as sensitive/expensive as the OS
//! keyring.
//!
//! ## The `ValidateFn` seam
//!
//! The Test button's background check goes through [`ValidateFn`] rather
//! than calling [`humm_core::openai::validate::validate_key`] directly, so a
//! kittest harness can script the outcome and never touch the network
//! (mirrors `translate_seam.rs`'s `TranslateSpawner` seam, one level
//! simpler: a single call-and-return rather than a whole worker).

use std::path::Path;
use std::sync::{Arc, Mutex};

use humm_core::openai::{OpenAiAuth, ValidateOutcome, validate_key};
use humm_core::secrets::{SecretKey, SecretStore};
use humm_core::settings::{OpenAiSettings, Settings};

use crate::i18n::tr;

/// Signature for the Test button's background key-validation check:
/// `Fn(OpenAiAuth) -> ValidateOutcome`, run on its own thread ([`start_test`]).
/// The real implementation ([`real_validate_fn`]) is
/// [`humm_core::openai::validate::validate_key`]; a kittest harness injects a
/// scripted `Arc::new(|_auth| ValidateOutcome::Valid)` (or `Invalid`) instead
/// so no test ever issues a real HTTP request.
pub type ValidateFn = Arc<dyn Fn(OpenAiAuth) -> ValidateOutcome + Send + Sync>;

/// The production [`ValidateFn`]: a real network check via
/// [`validate_key`].
pub fn real_validate_fn() -> ValidateFn {
    Arc::new(|auth| validate_key(&auth))
}

/// A hermetic test [`ValidateFn`] that never runs — every kittest fixture
/// that doesn't specifically exercise the Test button wires this in (mirrors
/// `services::default_test_translate`'s neutral default). Returns `Invalid`
/// with a message that would fail any assertion actually depending on the
/// real check, so a test that accidentally exercises the Test button without
/// scripting its own [`ValidateFn`] fails loudly rather than silently
/// reaching the network.
pub fn stub_validate_fn() -> ValidateFn {
    Arc::new(|_auth| ValidateOutcome::Invalid {
        message: "no ValidateFn was scripted for this test".to_string(),
    })
}

// -- Charset check (Buzz `BASE64_PATTERN`, `:32`) ----------------------------

/// Buzz's exact key-charset pattern (`general_preferences_widget.py:32`).
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

// -- Pure field-commit helpers ------------------------------------------------

/// `s` wrapped in `Some`, or `None` if `s` is empty — the base-URL field's
/// "empty means unset" rule (design spec §7.1: "saved to
/// `settings.openai.base_url` on change (empty → `None`)"). Deliberately
/// does **not** trim (Buzz saves the raw text verbatim,
/// `on_custom_openai_base_url_changed`; only the empty-to-`None` mapping is
/// humm's own addition, since `OpenAiSettings::base_url` is an `Option`
/// where Buzz's `QSettings` value is a plain, possibly-empty string).
fn empty_to_none(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// `s`, or [`OpenAiSettings::DEFAULT_API_MODEL`] if `s` is empty.
///
/// Deliberate divergence from Buzz (documented, task brief): Buzz's
/// `on_openai_api_model_changed` saves the raw text unconditionally, so an
/// emptied field persists as `""` — harmless in Buzz/Python, where the empty
/// string is read back and substituted for `"whisper-1"` at request-build
/// time in a couple of call sites. `OpenAiSettings::api_model` is a plain
/// (non-`Option`) `String` consumed verbatim wherever the API engine reads it
/// (design spec §3.3); persisting `""` here would make every subsequent
/// request send an empty `model` field instead. Falling back to the same
/// default the settings type itself already defaults to keeps the field's
/// invariant ("never empty") intact without the engine needing an empty-model
/// special case of its own.
fn api_model_or_default(s: &str) -> String {
    if s.is_empty() {
        OpenAiSettings::DEFAULT_API_MODEL.to_string()
    } else {
        s.to_string()
    }
}

/// Locks `settings`, applies `f` to `settings.openai`, and saves — the same
/// diff-free, non-fatal-on-failure immediate-save contract
/// `views/recording_advanced.rs`'s `persist_recording` uses for its own
/// section.
fn persist_openai(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut OpenAiSettings),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.openai);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the OpenAI settings: {err}");
    }
}

// -- Modal --------------------------------------------------------------------

/// Which of the tab's three possible modals (if any) is currently showing.
/// Only one at a time — see [`commit_key`]'s doc comment for what happens
/// when both the charset warning and the keyring-save error would otherwise
/// fire from the same commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneralModal {
    /// The key field's committed value failed [`is_base64_charset`] — Buzz's
    /// "Invalid API key" warning (`:261-267`). Warn-only: the value is saved
    /// regardless (Buzz saves too).
    InvalidKeyCharset,
    /// [`SecretStore::set`] failed while committing the key field — Buzz's
    /// `main_window.on_openai_access_token_changed` error dialog (`:323-331`).
    KeyringSaveFailed,
    /// A Test click finished: `Valid` renders as the byte-exact success body;
    /// `Invalid { message }` renders `message` verbatim — both under the
    /// title "OpenAI API Key Test" (Buzz `:244-254`).
    TestResult(String),
}

fn modal_title_and_body(modal: &GeneralModal) -> (String, String) {
    match modal {
        GeneralModal::InvalidKeyCharset => (
            tr("prefs-general-invalid-key-title"),
            tr("prefs-general-invalid-key-body"),
        ),
        GeneralModal::KeyringSaveFailed => (
            tr("prefs-general-key-save-error-title"),
            tr("prefs-general-key-save-error-body"),
        ),
        GeneralModal::TestResult(body) => (tr("prefs-general-test-title"), body.clone()),
    }
}

// -- State ----------------------------------------------------------------

/// The General tab's live state, held in
/// [`crate::views::preferences::PreferencesState`].
pub struct GeneralTabState {
    /// The API key field's live text — seeded from [`SecretStore::get`] at
    /// construction, committed back to the store on focus-out/Enter
    /// ([`commit_key`]).
    pub key_buffer: String,
    /// Whether the key field currently renders unmasked (the eye toggle).
    pub key_visible: bool,
    /// `true` while a Test click's background check is in flight — gates the
    /// Test button ([`test_button_enabled`]) and is cleared once
    /// [`drain_test_result`] receives the outcome.
    pub test_running: bool,
    /// The in-flight Test check's result channel, `Some` only while
    /// `test_running` is `true`.
    pub test_rx: Option<flume::Receiver<ValidateOutcome>>,
    /// The currently-open modal, if any.
    pub modal: Option<GeneralModal>,
    /// The base-URL field's live text (empty means "use the default").
    pub base_url_buffer: String,
    /// The API-model field's live text.
    pub api_model_buffer: String,
}

impl GeneralTabState {
    /// Builds the tab's initial state: `key_buffer` from `secrets` (an
    /// absent entry, or a backend error, both seed an empty field — the same
    /// "nothing stored yet" starting point as a fresh Buzz install), and the
    /// URL/model buffers from `openai` (`settings.openai` at the moment the
    /// Preferences window opens).
    pub fn new(secrets: &dyn SecretStore, openai: &OpenAiSettings) -> Self {
        let key_buffer = secrets
            .get(SecretKey::OpenAiApiKey)
            .ok()
            .flatten()
            .unwrap_or_default();
        GeneralTabState {
            key_buffer,
            key_visible: false,
            test_running: false,
            test_rx: None,
            modal: None,
            base_url_buffer: openai.base_url.clone().unwrap_or_default(),
            api_model_buffer: openai.api_model.clone(),
        }
    }
}

impl std::fmt::Debug for GeneralTabState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneralTabState")
            .field("key_buffer", &"«redacted»")
            .field("key_visible", &self.key_visible)
            .field("test_running", &self.test_running)
            .field("test_rx", &self.test_rx)
            .field("modal", &self.modal)
            .field("base_url_buffer", &self.base_url_buffer)
            .field("api_model_buffer", &self.api_model_buffer)
            .finish()
    }
}

/// Whether the Test button is enabled: a non-empty key field and no check
/// already in flight (Buzz `update_test_openai_api_key_button`, `:230-231` —
/// Buzz's own gate is key-non-empty only, since its `QThreadPool` job already
/// disables the button for the duration via
/// `on_click_test_openai_api_key_button`/`on_test_openai_api_key_success`;
/// `test_running` here is that same in-flight disable, made explicit).
pub fn test_button_enabled(state: &GeneralTabState) -> bool {
    !state.key_buffer.is_empty() && !state.test_running
}

// -- Commit seam (kittest-driven directly, `playback_controls::
// commit_speed_text`'s precedent — see this crate's kittest_prefs.rs) --------

/// Commits the key field: charset-checks it (warns, but always still saves —
/// Buzz parity, module doc), then writes it to `secrets`.
///
/// If **both** the charset check fails and the keyring write fails, only the
/// keyring-failure modal is shown — a deliberate simplification: the two are
/// independent failures in Buzz too (separate signal handlers with no shared
/// state), this module has a single `Option<GeneralModal>` slot, and a failed
/// save is the more actionable of the two (the charset warning is purely
/// advisory — "may cause errors" — while a keyring failure means the edit
/// was not persisted at all).
pub fn commit_key(state: &mut GeneralTabState, secrets: &Arc<dyn SecretStore>) {
    let mut modal = if is_base64_charset(&state.key_buffer) {
        None
    } else {
        Some(GeneralModal::InvalidKeyCharset)
    };

    if let Err(err) = secrets.set(SecretKey::OpenAiApiKey, &state.key_buffer) {
        log::warn!("could not save the OpenAI API key to the keyring: {err}");
        modal = Some(GeneralModal::KeyringSaveFailed);
    }

    state.modal = modal;
}

/// Commits the base-URL field: empty → `None`, saved immediately.
pub fn commit_base_url(state: &GeneralTabState, settings: &Mutex<Settings>, settings_path: &Path) {
    let value = empty_to_none(&state.base_url_buffer);
    persist_openai(settings, settings_path, |openai| openai.base_url = value);
}

/// Commits the API-model field: empty → [`OpenAiSettings::DEFAULT_API_MODEL`]
/// (module doc's documented divergence), saved immediately.
pub fn commit_api_model(state: &GeneralTabState, settings: &Mutex<Settings>, settings_path: &Path) {
    let value = api_model_or_default(&state.api_model_buffer);
    persist_openai(settings, settings_path, |openai| openai.api_model = value);
}

/// Starts a Test click: snapshots `settings.openai.base_url` *now* (spec
/// §7.1 — "base URL read from settings at click time", so a base-URL edit
/// made after the click doesn't retroactively change what's being tested),
/// clones the key field, and runs `validate` on a background thread — the
/// `UpdateNotice` channel pattern (`services.rs`'s own doc comment), one
/// level simpler since there's only ever one outcome per click, not a stream.
/// `ctx.request_repaint()` on completion wakes the UI even if nothing else is
/// animating (same reason `models_ui::start_download`'s thread does it).
pub fn start_test(
    state: &mut GeneralTabState,
    settings: &Mutex<Settings>,
    validate: &ValidateFn,
    ctx: egui::Context,
) {
    let base_url = {
        let guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.openai.base_url.clone()
    };
    let auth = OpenAiAuth {
        base_url,
        api_key: state.key_buffer.clone(),
    };
    let validate = Arc::clone(validate);
    let (tx, rx) = flume::unbounded();

    state.test_running = true;
    state.test_rx = Some(rx);

    std::thread::spawn(move || {
        let outcome = validate(auth);
        // The receiver may already be gone (the Preferences window closed
        // mid-check) -- dropping the outcome silently is correct there, same
        // as `run_update_check`'s own `tx.send(..).is_err()` handling.
        let _ = tx.send(outcome);
        ctx.request_repaint();
    });
}

/// Drains `state.test_rx` (a no-op while no check is in flight or none has
/// landed yet this frame) and, once an outcome arrives, clears `test_running`
/// and opens the result modal. Call once per frame before rendering.
pub fn drain_test_result(state: &mut GeneralTabState) {
    let Some(rx) = &state.test_rx else {
        return;
    };
    match rx.try_recv() {
        Ok(outcome) => {
            state.test_running = false;
            state.test_rx = None;
            let body = match outcome {
                ValidateOutcome::Valid => tr("prefs-general-test-success-body"),
                ValidateOutcome::Invalid { message } => message,
            };
            state.modal = Some(GeneralModal::TestResult(body));
        }
        Err(flume::TryRecvError::Empty) => {}
        Err(flume::TryRecvError::Disconnected) => {
            // The sender thread panicked without sending -- surface nothing
            // rather than leave the button permanently disabled.
            state.test_running = false;
            state.test_rx = None;
        }
    }
}

// -- Render -------------------------------------------------------------------

/// Renders the General tab's OpenAI block: the key field + eye toggle, the
/// Test button, the base-URL field, and the API-model field, plus whichever
/// modal (if any) is currently open.
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut GeneralTabState,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    secrets: &Arc<dyn SecretStore>,
    validate: &ValidateFn,
) {
    drain_test_result(state);

    ui.horizontal(|ui| {
        ui.label(tr("prefs-general-openai-key-label"));
        let key_response = ui.add(
            egui::TextEdit::singleline(&mut state.key_buffer)
                .password(!state.key_visible)
                .hint_text(tr("prefs-general-openai-key-placeholder")),
        );
        let eye_label = if state.key_visible {
            tr("prefs-general-key-hide")
        } else {
            tr("prefs-general-key-show")
        };
        if ui.button(eye_label).clicked() {
            state.key_visible = !state.key_visible;
        }
        // `lost_focus()` alone covers Enter too -- egui's single-line
        // `TextEdit` surrenders focus on Enter rather than inserting a
        // newline (same rationale `models_ui.rs`'s custom-URL field
        // documents for its own `lost_focus()`-only commit).
        if key_response.lost_focus() {
            commit_key(state, secrets);
        }
    });

    if ui
        .add_enabled(
            test_button_enabled(state),
            egui::Button::new(tr("prefs-general-test-button")),
        )
        .clicked()
    {
        start_test(state, settings, validate, ui.ctx().clone());
    }

    ui.horizontal(|ui| {
        ui.label(tr("prefs-general-base-url-label"));
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.base_url_buffer)
                .hint_text(tr("prefs-general-base-url-placeholder")),
        );
        if response.changed() {
            commit_base_url(state, settings, settings_path);
        }
    });

    ui.horizontal(|ui| {
        ui.label(tr("prefs-general-model-label"));
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.api_model_buffer)
                .hint_text(tr("prefs-general-model-placeholder")),
        );
        if response.changed() {
            commit_api_model(state, settings, settings_path);
        }
    });

    if let Some(modal) = &state.modal {
        let (title, body) = modal_title_and_body(modal);
        let mut close = false;
        let modal_response =
            egui::Modal::new(egui::Id::new("humm-prefs-general-modal")).show(ui.ctx(), |ui| {
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

    // -- empty_to_none ----------------------------------------------------------

    #[test]
    fn empty_base_url_maps_to_none() {
        assert_eq!(empty_to_none(""), None);
    }

    #[test]
    fn non_empty_base_url_is_kept_verbatim_untrimmed() {
        assert_eq!(
            empty_to_none("https://example.com/v1"),
            Some("https://example.com/v1".to_string())
        );
        // No trim -- Buzz parity (module doc).
        assert_eq!(empty_to_none("  padded  "), Some("  padded  ".to_string()));
    }

    // -- api_model_or_default ----------------------------------------------------

    #[test]
    fn empty_model_commits_the_default() {
        assert_eq!(api_model_or_default(""), "whisper-1");
        assert_eq!(api_model_or_default(""), OpenAiSettings::DEFAULT_API_MODEL);
    }

    #[test]
    fn non_empty_model_is_kept_verbatim() {
        assert_eq!(api_model_or_default("whisper-1-custom"), "whisper-1-custom");
    }

    // -- test_button_enabled ----------------------------------------------------

    fn state_with(key_buffer: &str, test_running: bool) -> GeneralTabState {
        GeneralTabState {
            key_buffer: key_buffer.to_string(),
            key_visible: false,
            test_running,
            test_rx: None,
            modal: None,
            base_url_buffer: String::new(),
            api_model_buffer: String::new(),
        }
    }

    #[test]
    fn test_button_enablement_matrix() {
        assert!(
            !test_button_enabled(&state_with("", false)),
            "empty key, idle -> disabled"
        );
        assert!(
            test_button_enabled(&state_with("sk-x", false)),
            "non-empty key, idle -> enabled"
        );
        assert!(
            !test_button_enabled(&state_with("sk-x", true)),
            "non-empty key, running -> disabled"
        );
        assert!(
            !test_button_enabled(&state_with("", true)),
            "empty key, running -> disabled"
        );
    }

    // -- GeneralTabState::new ----------------------------------------------------

    #[test]
    fn new_seeds_key_buffer_from_the_secret_store() {
        let secrets = MemorySecretStore::new();
        secrets.set(SecretKey::OpenAiApiKey, "sk-seeded").unwrap();

        let state = GeneralTabState::new(&secrets, &OpenAiSettings::default());

        assert_eq!(state.key_buffer, "sk-seeded");
    }

    #[test]
    fn new_key_buffer_is_empty_when_nothing_is_stored() {
        let secrets = MemorySecretStore::new();
        let state = GeneralTabState::new(&secrets, &OpenAiSettings::default());
        assert_eq!(state.key_buffer, "");
    }

    #[test]
    fn new_seeds_url_and_model_buffers_from_settings() {
        let secrets = MemorySecretStore::new();
        let openai = OpenAiSettings {
            base_url: Some("https://example.com/v1".to_string()),
            api_model: "whisper-1-custom".to_string(),
        };

        let state = GeneralTabState::new(&secrets, &openai);

        assert_eq!(state.base_url_buffer, "https://example.com/v1");
        assert_eq!(state.api_model_buffer, "whisper-1-custom");
    }

    #[test]
    fn new_base_url_buffer_is_empty_by_default() {
        let secrets = MemorySecretStore::new();
        let state = GeneralTabState::new(&secrets, &OpenAiSettings::default());
        assert_eq!(state.base_url_buffer, "");
        assert_eq!(state.api_model_buffer, "whisper-1");
    }

    // -- commit_key ---------------------------------------------------------------

    #[test]
    fn commit_key_saves_and_sets_no_modal_for_a_valid_charset() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let mut state = state_with("sk-Valid123+/=_-", false);

        commit_key(&mut state, &secrets);

        assert_eq!(state.modal, None);
        assert_eq!(
            secrets.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("sk-Valid123+/=_-".to_string())
        );
    }

    #[test]
    fn commit_key_warns_but_still_saves_for_an_invalid_charset() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let mut state = state_with("sk-bad key!", false);

        commit_key(&mut state, &secrets);

        assert_eq!(state.modal, Some(GeneralModal::InvalidKeyCharset));
        assert_eq!(
            secrets.get(SecretKey::OpenAiApiKey).unwrap(),
            Some("sk-bad key!".to_string()),
            "an invalid-charset key must still be saved (Buzz parity)"
        );
    }

    // -- commit_base_url / commit_api_model ----------------------------------------

    #[test]
    fn commit_base_url_empty_clears_the_setting() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut settings = Settings::default();
        settings.openai.base_url = Some("https://old.example.com".to_string());
        let settings = Mutex::new(settings);

        let mut state = state_with("", false);
        state.base_url_buffer = String::new();
        commit_base_url(&state, &settings, &settings_path);

        assert_eq!(
            settings.lock().unwrap().openai.base_url,
            None,
            "an emptied base-URL field must clear the setting to None"
        );
    }

    #[test]
    fn commit_base_url_non_empty_saves_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = state_with("", false);
        state.base_url_buffer = "https://custom.example.com/v1".to_string();
        commit_base_url(&state, &settings, &settings_path);

        assert_eq!(
            settings.lock().unwrap().openai.base_url,
            Some("https://custom.example.com/v1".to_string())
        );
    }

    #[test]
    fn commit_api_model_empty_resets_to_whisper_1() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut settings = Settings::default();
        settings.openai.api_model = "gpt-4o-transcribe".to_string();
        let settings = Mutex::new(settings);

        let mut state = state_with("", false);
        state.api_model_buffer = String::new();
        commit_api_model(&state, &settings, &settings_path);

        assert_eq!(settings.lock().unwrap().openai.api_model, "whisper-1");
    }

    #[test]
    fn commit_api_model_non_empty_saves_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = state_with("", false);
        state.api_model_buffer = "whisper-1-custom".to_string();
        commit_api_model(&state, &settings, &settings_path);

        assert_eq!(
            settings.lock().unwrap().openai.api_model,
            "whisper-1-custom"
        );
    }

    // -- Debug redaction --------------------------------------------------

    #[test]
    fn debug_redacts_key_buffer() {
        let state = GeneralTabState {
            key_buffer: "sk-super-secret-value".to_string(),
            key_visible: false,
            test_running: false,
            test_rx: None,
            modal: None,
            base_url_buffer: String::new(),
            api_model_buffer: String::new(),
        };

        let debug_output = format!("{state:?}");

        assert!(
            !debug_output.contains("sk-super-secret-value"),
            "Debug output must not contain the key material, got: {debug_output}"
        );
        assert!(
            debug_output.contains("«redacted»"),
            "Debug output must contain the redaction marker, got: {debug_output}"
        );
        assert!(
            debug_output.contains("GeneralTabState"),
            "Debug output must contain the struct name, got: {debug_output}"
        );
    }
}
