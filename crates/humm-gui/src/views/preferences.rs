//! The Preferences window (⌘, / Help → Preferences…): tabs **Models**
//! (F-044 basic, [`crate::models_ui`]) and **Privacy** (ADR-009, UX §J-11) —
//! `General`/`Shortcuts`/`Folder Watch` arrive M7
//! (`buzz-notes-import-progress-models.md`'s preferences-shell notes).
//!
//! Rendered as a real OS window (an egui *immediate* viewport, min
//! 650×500 — `app.rs::HummApp::show_preferences_viewport`). Immediate
//! rather than deferred: a deferred viewport's callback must be
//! `Send + Sync + 'static`, which would force `PreferencesState`'s
//! `Arc<Mutex<Settings>>`/model-download threads into an `Arc<Mutex<..>>`
//! wrapper of their own just to cross that bound; immediate viewports take
//! a plain (non-`'static`) `FnMut`, so [`ui`] below can borrow `HummApp`'s
//! state directly. The tradeoff (documented in `viewport.rs`'s own docs):
//! an immediate viewport's parent (the main window) repaints every time
//! this one does and vice versa — acceptable for a Preferences window that
//! isn't itself animating, unlike e.g. a live waveform view would be.
//!
//! [`ui`] itself needs no viewport (only `&mut egui::Ui` + this module's
//! plain state), so a kittest harness drives it directly inside an ordinary
//! `egui::CentralPanel` — only the viewport wrapping in `app.rs` is left to
//! manual verification (no display in this sandboxed dev environment;
//! `docs/gui-manual/M3.md` covers it).

use std::path::Path;
use std::sync::{Arc, Mutex};

use humm_core::secrets::SecretStore;
use humm_core::settings::Settings;

use crate::i18n::tr;
use crate::models_ui::{self, ModelsTabState};
use crate::views::prefs_general::{self, GeneralTabState, ValidateFn};

/// Which tab is currently selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsTab {
    /// UI language, font size, the OpenAI block, default export template,
    /// and Disable GPU (F-100 complete, design spec §8.1). Shortcuts/Folder
    /// Watch/Post-processing tabs are later M7 tasks.
    General,
    /// F-044 basic model list.
    Models,
    /// ADR-009/UX §J-11 telemetry + update-check toggles.
    Privacy,
}

/// The Preferences window's full live state, owned by `HummApp` for as long
/// as the window is open (rebuilt fresh — `Models` re-scanned from disk,
/// `General`'s key field re-read from the keyring — each time it's
/// (re)opened; see `app.rs`).
#[derive(Debug)]
pub struct PreferencesState {
    /// The selected tab.
    pub tab: PrefsTab,
    /// The General tab's own state (F-100 complete, design spec §8.1).
    pub general: GeneralTabState,
    /// The Models tab's own state.
    pub models: ModelsTabState,
}

impl PreferencesState {
    /// Builds fresh state opened on the General tab — Buzz's own preferences
    /// dialog always opens on its first tab (`preferences_dialog.py:46-48`),
    /// and General is that first tab both in Buzz and in this tab strip's own
    /// order. `models` is scanned from `root` and seeded from the current
    /// custom-URL setting; `general` is seeded from `secrets`/`settings`
    /// (a full snapshot the caller takes when the window opens — General
    /// now reads `openai`, `ui.{locale,font_size}`,
    /// `file_transcriber.default_export_template`, and `models.force_cpu`,
    /// F-100 complete).
    pub fn new(
        root: &Path,
        custom_url: Option<&str>,
        secrets: &Arc<dyn SecretStore>,
        settings: &Settings,
    ) -> Self {
        PreferencesState {
            tab: PrefsTab::General,
            general: GeneralTabState::new(secrets.as_ref(), settings),
            models: ModelsTabState::new(root, custom_url),
        }
    }
}

/// Renders the tab strip + the selected tab's content.
#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut PreferencesState,
    model_root: &Path,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    secrets: &Arc<dyn SecretStore>,
    validate: &ValidateFn,
) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.tab, PrefsTab::General, tr("prefs-tab-general"));
        ui.selectable_value(&mut state.tab, PrefsTab::Models, tr("prefs-tab-models"));
        ui.selectable_value(&mut state.tab, PrefsTab::Privacy, tr("prefs-tab-privacy"));
    });
    ui.separator();

    match state.tab {
        PrefsTab::General => {
            prefs_general::ui(
                ui,
                &mut state.general,
                settings,
                settings_path,
                secrets,
                validate,
            );
        }
        PrefsTab::Models => {
            models_ui::ui(ui, &mut state.models, model_root, settings, settings_path);
        }
        PrefsTab::Privacy => privacy_tab_ui(ui, settings, settings_path),
    }
}

/// The Privacy tab (ADR-009, UX §J-11): a telemetry toggle with a
/// plain-English list of exactly what one `app_launched` event contains
/// (the six property keys, spelled out, plus the anonymous identifier), and
/// an update-check toggle. Both persist immediately on change (Buzz
/// preferences apply live — no separate Ok/Apply step).
fn privacy_tab_ui(ui: &mut egui::Ui, settings: &Arc<Mutex<Settings>>, settings_path: &Path) {
    let (mut telemetry_enabled, mut check_enabled) = {
        let guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (guard.privacy.telemetry_enabled, guard.updates.check_enabled)
    };

    let telemetry_response = ui.checkbox(&mut telemetry_enabled, tr("privacy-telemetry-checkbox"));

    if disabled_at_compile_time() {
        ui.label(tr("privacy-telemetry-disabled-caption"));
    }

    ui.indent("humm-privacy-telemetry-items", |ui| {
        ui.label(tr("privacy-telemetry-items-intro"));
        ui.label(format!(
            "\u{2022} {}",
            tr("privacy-telemetry-item-app-version")
        ));
        ui.label(format!(
            "\u{2022} {}",
            tr("privacy-telemetry-item-system-name")
        ));
        ui.label(format!(
            "\u{2022} {}",
            tr("privacy-telemetry-item-os-version")
        ));
        ui.label(format!(
            "\u{2022} {}",
            tr("privacy-telemetry-item-kernel-release")
        ));
        ui.label(format!(
            "\u{2022} {}",
            tr("privacy-telemetry-item-cpu-architecture")
        ));
        ui.label(format!(
            "\u{2022} {}",
            tr("privacy-telemetry-item-system-locale")
        ));
        ui.label(format!(
            "\u{2022} {}",
            tr("privacy-telemetry-item-identifier")
        ));
    });

    ui.separator();
    let update_response = ui.checkbox(&mut check_enabled, tr("privacy-update-checkbox"));

    if telemetry_response.changed() || update_response.changed() {
        let mut guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.privacy.telemetry_enabled = telemetry_enabled;
        guard.updates.check_enabled = check_enabled;
        if let Err(err) = guard.save(settings_path) {
            log::warn!("could not persist the privacy settings: {err}");
        }
    }
}

/// Whether telemetry is off *for compile-time reasons* regardless of the
/// toggle above it — no build-time PostHog key, or a debug build (ADR-009).
/// Rendered as an explanatory caption under the checkbox (binding decision)
/// so a user who flips it on in a debug build isn't left wondering why
/// nothing gets sent.
fn disabled_at_compile_time() -> bool {
    option_env!("HUMM_POSTHOG_KEY").is_none() || cfg!(debug_assertions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::secrets::MemorySecretStore;
    use pretty_assertions::assert_eq;

    #[test]
    fn new_opens_on_the_general_tab() {
        let dir = tempfile::tempdir().expect("tempdir");
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let state = PreferencesState::new(dir.path(), None, &secrets, &Settings::default());
        assert_eq!(state.tab, PrefsTab::General);
    }

    #[test]
    fn disabled_at_compile_time_is_true_in_debug_builds() {
        // Every test build is a debug build (`cargo test` never sets
        // `--release`), and this dev environment never sets
        // `HUMM_POSTHOG_KEY` at build time either — both gates independently
        // already force this `true`, which is exactly the "no accidental
        // network calls from tests" property the module relies on.
        assert!(disabled_at_compile_time());
    }
}
