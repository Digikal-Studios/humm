//! Preferences → General tab, complete (F-100, design spec §8.1). Buzz's row
//! order (`general_preferences_widget.py:66-225`), reproduced here: **UI
//! language** combo (live apply — [`apply_font_size`]'s sibling for locale is
//! [`crate::i18n::set_locale`], design spec ledger #L10), **font size**
//! `DragValue` 8–32 (live apply via [`apply_font_size`]), the M6 **OpenAI
//! block** unchanged in place (key field with its show/hide eye toggle and
//! Test button, base-URL field, API-model field), **default export file
//! name** (commit-on-focus-out → `file_transcriber.default_export_template`),
//! and **Disable GPU** (`models.force_cpu`).
//!
//! Ported from Buzz:
//! - `buzz/widgets/preferences_dialog/general_preferences_widget.py` —
//!   `ui_locales` (`:34-50`), the row construction (`:66-225`), and every
//!   `on_*` handler (`:227-335`, `ValidateOpenAIApiKeyJob` at `:338-385`).
//!   Not ported (ledger #L11/#L12, design spec §8.1): the live-export
//!   block, the live-recording-mode combo, and "Reduce GPU RAM".
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
//!
//! ## Shared with the options sheet (Task 14)
//!
//! The charset check and the commit-to-keyring plumbing now live in
//! [`crate::views::secret_field`] — this module's [`commit_key`] is a thin
//! wrapper mapping [`secret_field::CommitOutcome`] onto [`GeneralModal`], and
//! the key row itself renders via [`secret_field::masked_field_row`]. The
//! options sheet's own Api Key row (`views/options_sheet.rs`, design spec
//! §7.2) reuses both rather than re-porting Buzz's
//! `openai_api_key_line_edit.py`/`BASE64_PATTERN` a second time.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use humm_core::export::DEFAULT_EXPORT_TEMPLATE;
use humm_core::openai::{OpenAiAuth, ValidateOutcome, validate_key};
use humm_core::secrets::{SecretKey, SecretStore};
use humm_core::settings::{
    FileTranscriberSettings, ModelSettings, OpenAiSettings, Settings, UiSettings,
};

use crate::i18n::{self, tr};
use crate::views::secret_field::{self, CommitOutcome};

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

/// As [`persist_openai`], but for `settings.ui` (locale, font size).
fn persist_ui(settings: &Mutex<Settings>, settings_path: &Path, f: impl FnOnce(&mut UiSettings)) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.ui);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the UI settings: {err}");
    }
}

/// As [`persist_openai`], but for `settings.file_transcriber` (the default
/// export template).
fn persist_file_transcriber(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut FileTranscriberSettings),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.file_transcriber);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the file-transcriber settings: {err}");
    }
}

/// As [`persist_openai`], but for `settings.models` (Disable GPU).
fn persist_models(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut ModelSettings),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.models);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the model settings: {err}");
    }
}

// -- Font-size scaling (design spec §8.1) -------------------------------------

/// egui's own default [`egui::TextStyle::Body`] size, in points
/// (`egui::Style::default()`'s `default_text_styles()`) — the reference point
/// [`scale_text_styles`] scales every other style relative to: "font size N
/// maps Body to N points, every other style scales by the same ratio" (this
/// module's own documented mapping, design spec §8.1). Also
/// [`GeneralTabState`]'s seed value for the font-size `DragValue` the first
/// time "Custom font size" is checked with nothing stored yet, so turning the
/// checkbox on doesn't itself visibly change anything until the value is
/// actually edited (Buzz's own spinbox always starts at the font's *current*
/// point size, `self.font().pointSize()`; humm's closest equivalent starting
/// point is egui's own untouched default).
pub const DEFAULT_BODY_SIZE: u32 = 13;

/// [`apply_font_size`]'s one-time capture of egui's pristine default text
/// styles, taken from whichever `egui::Context` first calls it — before any
/// override this module ever applies. Every later `Some`/`None` call scales
/// from (or restores exactly) this snapshot, never from the *current*
/// (possibly already-scaled) style, so repeated `Some(n)` calls are
/// idempotent and a `None` after any number of `Some` calls always lands back
/// on the same pristine sizes.
static DEFAULT_TEXT_STYLES: OnceLock<BTreeMap<egui::TextStyle, egui::FontId>> = OnceLock::new();

/// Pure core of [`apply_font_size`]: applies `size` to `style.text_styles`,
/// scaling every entry proportionally from `defaults` (egui's pristine
/// per-[`egui::TextStyle`] sizes, captured once — see [`apply_font_size`]'s
/// own doc). `None` restores `style.text_styles` to `defaults` verbatim, byte
/// for byte. Free of any `egui::Context`, so it's testable directly against a
/// plain [`egui::Style`] value.
pub fn scale_text_styles(
    style: &mut egui::Style,
    size: Option<u32>,
    defaults: &BTreeMap<egui::TextStyle, egui::FontId>,
) {
    match size {
        None => style.text_styles = defaults.clone(),
        Some(points) => {
            let default_body = defaults
                .get(&egui::TextStyle::Body)
                .map_or(DEFAULT_BODY_SIZE as f32, |font_id| font_id.size);
            let scale = points as f32 / default_body;
            style.text_styles = defaults
                .iter()
                .map(|(text_style, font_id)| {
                    (
                        text_style.clone(),
                        egui::FontId::new(font_id.size * scale, font_id.family.clone()),
                    )
                })
                .collect();
        }
    }
}

/// Applies `size` to `ctx`'s style (both the light and dark theme variants,
/// via `egui::Context::all_styles_mut` — font size must not depend on which
/// theme is active): `None` restores egui's pristine default text styles;
/// `Some(n)` scales every [`egui::TextStyle`] proportionally so
/// [`egui::TextStyle::Body`] lands at exactly `n` points ([`scale_text_styles`]'s
/// own doc has the full mapping).
///
/// The very first call this process ever makes (from either call site —
/// startup, `app.rs::HummApp::new`, or a Preferences edit) captures `ctx`'s
/// current text styles into [`DEFAULT_TEXT_STYLES`] as the pristine baseline
/// every later call scales from; call this **before** any other code has
/// touched `ctx`'s style, i.e. at startup right after the settings file loads
/// — the same "resolved once, before the first frame paints" contract the
/// locale negotiation right next to it in `HummApp::new` already follows.
pub fn apply_font_size(ctx: &egui::Context, size: Option<u32>) {
    let defaults =
        DEFAULT_TEXT_STYLES.get_or_init(|| ctx.style_of(egui::Theme::Light).text_styles.clone());
    ctx.all_styles_mut(|style| scale_text_styles(style, size, defaults));
}

// -- Modal --------------------------------------------------------------------

/// Which of the tab's three possible modals (if any) is currently showing.
/// Only one at a time — see [`commit_key`]'s doc comment for what happens
/// when both the charset warning and the keyring-save error would otherwise
/// fire from the same commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneralModal {
    /// The key field's committed value failed
    /// [`secret_field::is_base64_charset`] — Buzz's "Invalid API key"
    /// warning (`:261-267`). Warn-only: the value is saved regardless (Buzz
    /// saves too).
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
    /// The UI-language combo's current selection — a [`crate::i18n::
    /// LocaleEntry::buzz_code`] from [`crate::i18n::AVAILABLE_LOCALES`].
    /// Seeded from `crate::i18n::negotiate(settings.ui.locale...)`, matching
    /// whatever locale is actually active the moment the window opens.
    pub locale_code: String,
    /// Whether the font-size row's "Custom font size" checkbox is on — humm's
    /// own gate for [`UiSettings::font_size`]'s `Option`: Buzz's spinbox
    /// always has a concrete value, but humm's `None` means "use egui's
    /// built-in default" (Task 1's contract), so an explicit on/off toggle is
    /// needed to distinguish "never customized" from "customized to exactly
    /// the default size".
    pub font_size_custom: bool,
    /// The font-size `DragValue`'s live buffer, 8–32
    /// ([`UiSettings::MIN_FONT_SIZE`]–[`UiSettings::MAX_FONT_SIZE`]).
    /// Meaningful only while [`Self::font_size_custom`] is `true`; seeded
    /// from `settings.ui.font_size` when set, else [`DEFAULT_BODY_SIZE`] (a
    /// visually-neutral starting point — see that constant's doc).
    pub font_size_value: u32,
    /// The "Default export file name" field's live text (empty means "use
    /// [`DEFAULT_EXPORT_TEMPLATE`]", shown as the field's hint text).
    pub export_template_buffer: String,
    /// The "Disable GPU" checkbox's live value (`settings.models.force_cpu`).
    pub force_cpu: bool,
}

impl GeneralTabState {
    /// Builds the tab's initial state: `key_buffer` from `secrets` (an
    /// absent entry, or a backend error, both seed an empty field — the same
    /// "nothing stored yet" starting point as a fresh Buzz install), and every
    /// other field from `settings` at the moment the Preferences window
    /// opens (`openai`, `ui.{locale,font_size}`,
    /// `file_transcriber.default_export_template`, `models.force_cpu`).
    pub fn new(secrets: &dyn SecretStore, settings: &Settings) -> Self {
        let key_buffer = secrets
            .get(SecretKey::OpenAiApiKey)
            .ok()
            .flatten()
            .unwrap_or_default();
        let locale_code = i18n::negotiate(settings.ui.locale.as_deref())
            .buzz_code
            .to_string();
        GeneralTabState {
            key_buffer,
            key_visible: false,
            test_running: false,
            test_rx: None,
            modal: None,
            base_url_buffer: settings.openai.base_url.clone().unwrap_or_default(),
            api_model_buffer: settings.openai.api_model.clone(),
            locale_code,
            font_size_custom: settings.ui.font_size.is_some(),
            font_size_value: settings.ui.font_size.unwrap_or(DEFAULT_BODY_SIZE),
            export_template_buffer: settings
                .file_transcriber
                .default_export_template
                .clone()
                .unwrap_or_default(),
            force_cpu: settings.models.force_cpu,
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
            .field("locale_code", &self.locale_code)
            .field("font_size_custom", &self.font_size_custom)
            .field("font_size_value", &self.font_size_value)
            .field("export_template_buffer", &self.export_template_buffer)
            .field("force_cpu", &self.force_cpu)
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
    let outcome = secret_field::commit_secret(secrets, SecretKey::OpenAiApiKey, &state.key_buffer);
    state.modal = match outcome {
        CommitOutcome::Saved => None,
        CommitOutcome::InvalidCharset => Some(GeneralModal::InvalidKeyCharset),
        CommitOutcome::KeyringSaveFailed => Some(GeneralModal::KeyringSaveFailed),
    };
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

/// Commits a UI-language selection: switches the process-wide active locale
/// (live apply — design spec ledger #L10, no restart hint) and persists
/// `ui.locale`. `code` should already be a valid [`crate::i18n::
/// AVAILABLE_LOCALES`] entry's `buzz_code` (the combo only ever offers those),
/// but [`crate::i18n::set_locale`]'s own `en_US` fallback absorbs anything
/// else without panicking.
pub fn commit_locale(code: &str, settings: &Mutex<Settings>, settings_path: &Path) {
    i18n::set_locale(code);
    let code = code.to_string();
    persist_ui(settings, settings_path, move |ui| ui.locale = Some(code));
}

/// Commits the font-size row: `state.font_size_custom` gates whether
/// `settings.ui.font_size` becomes `Some(state.font_size_value.clamp(MIN,
/// MAX))` or `None` (Task 1's contract — `None` is egui's own default, never
/// persisted as a concrete number); either way, [`apply_font_size`] applies
/// the resolved value to `ctx` immediately (live, no restart).
pub fn commit_font_size(
    state: &GeneralTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
    ctx: &egui::Context,
) {
    let value = state.font_size_custom.then(|| {
        state
            .font_size_value
            .clamp(UiSettings::MIN_FONT_SIZE, UiSettings::MAX_FONT_SIZE)
    });
    persist_ui(settings, settings_path, |ui| ui.font_size = value);
    apply_font_size(ctx, value);
}

/// Commits the "Default export file name" field: empty → `None` (shown as
/// the field's own hint text, [`DEFAULT_EXPORT_TEMPLATE`]) — same
/// empty-means-unset rule [`empty_to_none`] already applies to the base-URL
/// field.
pub fn commit_export_template(
    state: &GeneralTabState,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let value = empty_to_none(&state.export_template_buffer);
    persist_file_transcriber(settings, settings_path, |ft| {
        ft.default_export_template = value
    });
}

/// Commits the "Disable GPU" checkbox: persists `models.force_cpu`, read
/// fresh at every engine-construction site (`engine_factory.rs::
/// create_whisper`'s own doc, `humm_core::env::use_gpu_resolution`), so this
/// takes effect on the very next transcription with no restart.
pub fn commit_force_cpu(state: &GeneralTabState, settings: &Mutex<Settings>, settings_path: &Path) {
    let value = state.force_cpu;
    persist_models(settings, settings_path, |models| models.force_cpu = value);
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

/// Renders the General tab, Buzz row order (module doc): UI language, font
/// size, the OpenAI block (key field + eye toggle, Test button, base-URL,
/// API-model), default export file name, Disable GPU — plus whichever modal
/// (if any) is currently open.
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut GeneralTabState,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    secrets: &Arc<dyn SecretStore>,
    validate: &ValidateFn,
) {
    drain_test_result(state);

    // 1. UI language (Buzz row 1) -- lists exactly `AVAILABLE_LOCALES`
    // (en-only this milestone; Task 12 appends more), applies live, no
    // restart hint (ledger #L10).
    ui.horizontal(|ui| {
        ui.label(tr("prefs-general-ui-language-label"));
        let mut selected = state.locale_code.clone();
        egui::ComboBox::from_id_salt("humm-prefs-general-ui-language")
            .selected_text(i18n::display_name(&selected))
            .show_ui(ui, |ui| {
                for entry in i18n::AVAILABLE_LOCALES {
                    ui.selectable_value(
                        &mut selected,
                        entry.buzz_code.to_string(),
                        i18n::display_name(entry.buzz_code),
                    );
                }
            });
        if selected != state.locale_code {
            state.locale_code = selected;
            commit_locale(&state.locale_code, settings, settings_path);
        }
    });

    // 2. Font size (Buzz row 2, QSpinBox 8-32) -- a "Custom font size"
    // checkbox gates the DragValue (module doc: humm's `None`/egui-default
    // has no Buzz-parity concrete value to always show). Applies live.
    ui.horizontal(|ui| {
        let mut custom = state.font_size_custom;
        if ui
            .checkbox(&mut custom, tr("prefs-general-font-size-custom-label"))
            .changed()
        {
            state.font_size_custom = custom;
            commit_font_size(state, settings, settings_path, ui.ctx());
        }
        ui.label(tr("prefs-general-font-size-label"));
        let mut value = state.font_size_value;
        let response = ui.add_enabled(
            state.font_size_custom,
            egui::DragValue::new(&mut value)
                .range(UiSettings::MIN_FONT_SIZE..=UiSettings::MAX_FONT_SIZE),
        );
        if response.changed() {
            state.font_size_value = value;
            commit_font_size(state, settings, settings_path, ui.ctx());
        }
    });

    // 3-5. The existing OpenAI block, unchanged in place.
    let key_response = secret_field::masked_field_row(
        ui,
        tr("prefs-general-openai-key-label"),
        &mut state.key_buffer,
        &mut state.key_visible,
        tr("prefs-general-openai-key-placeholder"),
    );
    // `lost_focus()` alone covers Enter too -- egui's single-line `TextEdit`
    // surrenders focus on Enter rather than inserting a newline (same
    // rationale `models_ui.rs`'s custom-URL field documents for its own
    // `lost_focus()`-only commit).
    if key_response.lost_focus() {
        commit_key(state, secrets);
    }

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

    // 6. Default export file name -- commit-on-focus-out, empty -> None
    // (hint text = `DEFAULT_EXPORT_TEMPLATE`).
    ui.horizontal(|ui| {
        ui.label(tr("prefs-general-export-template-label"));
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.export_template_buffer)
                .hint_text(DEFAULT_EXPORT_TEMPLATE),
        );
        if response.lost_focus() {
            commit_export_template(state, settings, settings_path);
        }
    });

    // 7. Disable GPU (Buzz row last) -- Buzz has both a form-row label
    // ("Disable GPU") and a separate checkbox-own label ("Use only CPU and
    // disable GPU acceleration"); egui's checkbox has one text slot, so both
    // render here, row label then checkbox. Tooltip verbatim, product name
    // swapped ("Buzz" -> "humm", module doc).
    ui.horizontal(|ui| {
        ui.label(tr("prefs-general-force-cpu-row-label"));
        let mut force_cpu = state.force_cpu;
        if ui
            .checkbox(&mut force_cpu, tr("prefs-general-force-cpu-label"))
            .on_hover_text(tr("prefs-general-force-cpu-tooltip"))
            .changed()
        {
            state.force_cpu = force_cpu;
            commit_force_cpu(state, settings, settings_path);
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
            locale_code: "en_US".to_string(),
            font_size_custom: false,
            font_size_value: DEFAULT_BODY_SIZE,
            export_template_buffer: String::new(),
            force_cpu: false,
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

        let state = GeneralTabState::new(&secrets, &Settings::default());

        assert_eq!(state.key_buffer, "sk-seeded");
    }

    #[test]
    fn new_key_buffer_is_empty_when_nothing_is_stored() {
        let secrets = MemorySecretStore::new();
        let state = GeneralTabState::new(&secrets, &Settings::default());
        assert_eq!(state.key_buffer, "");
    }

    #[test]
    fn new_seeds_url_and_model_buffers_from_settings() {
        let secrets = MemorySecretStore::new();
        let settings = Settings {
            openai: OpenAiSettings {
                base_url: Some("https://example.com/v1".to_string()),
                api_model: "whisper-1-custom".to_string(),
            },
            ..Settings::default()
        };

        let state = GeneralTabState::new(&secrets, &settings);

        assert_eq!(state.base_url_buffer, "https://example.com/v1");
        assert_eq!(state.api_model_buffer, "whisper-1-custom");
    }

    #[test]
    fn new_base_url_buffer_is_empty_by_default() {
        let secrets = MemorySecretStore::new();
        let state = GeneralTabState::new(&secrets, &Settings::default());
        assert_eq!(state.base_url_buffer, "");
        assert_eq!(state.api_model_buffer, "whisper-1");
    }

    // -- GeneralTabState::new -- the M7 Task 11 additions ------------------------

    #[test]
    fn new_seeds_locale_code_from_negotiated_settings_locale() {
        let secrets = MemorySecretStore::new();
        // No `ui.locale` set: negotiates to the fallback, `en_US` (the only
        // embedded locale this milestone).
        let state = GeneralTabState::new(&secrets, &Settings::default());
        assert_eq!(state.locale_code, "en_US");

        let settings = Settings {
            ui: UiSettings {
                locale: Some("en_US".to_string()),
                ..UiSettings::default()
            },
            ..Settings::default()
        };
        let state = GeneralTabState::new(&secrets, &settings);
        assert_eq!(state.locale_code, "en_US");
    }

    #[test]
    fn new_seeds_font_size_custom_and_value_from_settings() {
        let secrets = MemorySecretStore::new();

        let state = GeneralTabState::new(&secrets, &Settings::default());
        assert!(
            !state.font_size_custom,
            "no stored font_size -> Custom is off"
        );
        assert_eq!(state.font_size_value, DEFAULT_BODY_SIZE);

        let settings = Settings {
            ui: UiSettings {
                font_size: Some(20),
                ..UiSettings::default()
            },
            ..Settings::default()
        };
        let state = GeneralTabState::new(&secrets, &settings);
        assert!(state.font_size_custom, "a stored font_size -> Custom is on");
        assert_eq!(state.font_size_value, 20);
    }

    #[test]
    fn new_seeds_export_template_buffer_and_force_cpu_from_settings() {
        let secrets = MemorySecretStore::new();
        let settings = Settings {
            file_transcriber: FileTranscriberSettings {
                default_export_template: Some("{{ input_file_name }}-custom".to_string()),
                ..Default::default()
            },
            models: ModelSettings {
                force_cpu: true,
                ..Default::default()
            },
            ..Settings::default()
        };

        let state = GeneralTabState::new(&secrets, &settings);

        assert_eq!(state.export_template_buffer, "{{ input_file_name }}-custom");
        assert!(state.force_cpu);
    }

    #[test]
    fn new_export_template_buffer_is_empty_by_default() {
        let secrets = MemorySecretStore::new();
        let state = GeneralTabState::new(&secrets, &Settings::default());
        assert_eq!(state.export_template_buffer, "");
        assert!(!state.force_cpu);
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
        let state = state_with("sk-super-secret-value", false);

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

    // -- commit_locale --------------------------------------------------------

    #[test]
    fn commit_locale_persists_ui_locale() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        commit_locale("en_US", &settings, &settings_path);

        assert_eq!(settings.lock().unwrap().ui.locale.as_deref(), Some("en_US"));
        let reloaded = Settings::load(&settings_path).expect("settings file was written");
        assert_eq!(reloaded.ui.locale.as_deref(), Some("en_US"));
    }

    // -- commit_font_size -------------------------------------------------------

    #[test]
    fn commit_font_size_custom_persists_the_clamped_value() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let ctx = egui::Context::default();

        let mut state = state_with("", false);
        state.font_size_custom = true;
        state.font_size_value = 99; // above UiSettings::MAX_FONT_SIZE (32)
        commit_font_size(&state, &settings, &settings_path, &ctx);

        assert_eq!(
            settings.lock().unwrap().ui.font_size,
            Some(UiSettings::MAX_FONT_SIZE),
            "an out-of-range value must be clamped before it's persisted"
        );
    }

    #[test]
    fn commit_font_size_not_custom_persists_none() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut settings = Settings::default();
        settings.ui.font_size = Some(20);
        let settings = Mutex::new(settings);
        let ctx = egui::Context::default();

        let mut state = state_with("", false);
        state.font_size_custom = false;
        state.font_size_value = 20;
        commit_font_size(&state, &settings, &settings_path, &ctx);

        assert_eq!(
            settings.lock().unwrap().ui.font_size,
            None,
            "unchecking Custom must clear the setting back to the egui default"
        );
    }

    // -- commit_export_template ---------------------------------------------------

    #[test]
    fn commit_export_template_empty_clears_the_setting() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut settings = Settings::default();
        settings.file_transcriber.default_export_template = Some("old".to_string());
        let settings = Mutex::new(settings);

        let mut state = state_with("", false);
        state.export_template_buffer = String::new();
        commit_export_template(&state, &settings, &settings_path);

        assert_eq!(
            settings
                .lock()
                .unwrap()
                .file_transcriber
                .default_export_template,
            None
        );
    }

    #[test]
    fn commit_export_template_non_empty_saves_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = state_with("", false);
        state.export_template_buffer = "{{ input_file_name }}-custom".to_string();
        commit_export_template(&state, &settings, &settings_path);

        assert_eq!(
            settings
                .lock()
                .unwrap()
                .file_transcriber
                .default_export_template,
            Some("{{ input_file_name }}-custom".to_string())
        );
    }

    // -- commit_force_cpu -----------------------------------------------------

    #[test]
    fn commit_force_cpu_persists_true_and_false() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());

        let mut state = state_with("", false);
        state.force_cpu = true;
        commit_force_cpu(&state, &settings, &settings_path);
        assert!(settings.lock().unwrap().models.force_cpu);

        state.force_cpu = false;
        commit_force_cpu(&state, &settings, &settings_path);
        assert!(!settings.lock().unwrap().models.force_cpu);
    }

    // -- scale_text_styles / apply_font_size -------------------------------------

    #[test]
    fn apply_font_size_scales_all_text_styles_and_restores() {
        let defaults = egui::Style::default().text_styles;
        let default_body = defaults[&egui::TextStyle::Body].size;
        assert_eq!(
            default_body, DEFAULT_BODY_SIZE as f32,
            "this test's own assumed baseline must match the module's documented default"
        );

        // Some(26) doubles Body (13 -> 26) and every other entry by the same
        // ratio, relative to the *pristine* defaults, not whatever the style
        // already holds.
        let mut style = egui::Style::default();
        scale_text_styles(&mut style, Some(26), &defaults);
        assert_eq!(style.text_styles[&egui::TextStyle::Body].size, 26.0);
        for (text_style, font_id) in &defaults {
            let scaled = style.text_styles[text_style].size;
            assert!(
                (scaled - font_id.size * 2.0).abs() < 1e-4,
                "{text_style:?}: expected {} scaled 2x, got {scaled}",
                font_id.size
            );
        }

        // Idempotent: re-applying Some(26) onto the already-scaled `style`
        // must land on exactly the same sizes -- proving the scale is always
        // computed from `defaults`, never from the current (possibly
        // already-scaled) style.
        let before = style.text_styles.clone();
        scale_text_styles(&mut style, Some(26), &defaults);
        assert_eq!(style.text_styles, before);

        // A second, independent `Style` scaled the same way matches too.
        let mut style2 = egui::Style::default();
        scale_text_styles(&mut style2, Some(26), &defaults);
        assert_eq!(style.text_styles, style2.text_styles);

        // None restores exactly the pristine defaults, byte for byte.
        scale_text_styles(&mut style, None, &defaults);
        assert_eq!(style.text_styles, defaults);
    }

    #[test]
    fn apply_font_size_via_ctx_scales_body_to_the_requested_points() {
        let ctx = egui::Context::default();

        apply_font_size(&ctx, Some(26));
        let body_size = ctx.style_of(egui::Theme::Light).text_styles[&egui::TextStyle::Body].size;
        assert_eq!(body_size, 26.0);
        // Both theme variants are scaled identically (font size must not
        // depend on light/dark).
        let dark_body_size =
            ctx.style_of(egui::Theme::Dark).text_styles[&egui::TextStyle::Body].size;
        assert_eq!(dark_body_size, 26.0);

        apply_font_size(&ctx, None);
        let restored = ctx.style_of(egui::Theme::Light).text_styles[&egui::TextStyle::Body].size;
        assert_eq!(restored, DEFAULT_BODY_SIZE as f32);
    }
}
