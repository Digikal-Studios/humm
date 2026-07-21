//! The About window (Help → "About humm", F-006 gui-manual half): version,
//! a manual "Check for updates" that bypasses the ≤7-day cadence/toggle
//! gates (but reuses the exact same
//! [`crate::services::run_update_check`] code path the startup check does —
//! binding decision: Buzz's separate GitHub-API endpoint + naive string
//! compare (`about_dialog.py:103-124`) is a defect, not ported), and "Show
//! logs".
//!
//! Rendered as a real OS window (an egui *immediate* viewport —
//! `app.rs::HummApp::show_about_viewport`): egui_kittest cannot drive a
//! separate OS viewport, so [`ui`] is deliberately a plain function of
//! `&mut egui::Ui` + this module's own state, with every side effect
//! (spawning the manual check, opening the log folder, closing the window)
//! reported back as an [`AboutAction`] rather than performed here — that
//! keeps this function itself renderable (and therefore testable) inside an
//! ordinary `egui::CentralPanel` in a kittest harness, with only the
//! viewport *plumbing* around it left unverified by tests (documented at the
//! `app.rs` call site).

use crate::i18n::tr;

/// The result of a manual "Check for updates" click, once its background
/// thread reports back — `None` while no check has completed yet (including
/// "never clicked" and "a check is currently in flight").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutCheckResult {
    /// The current version is already the latest — Buzz's "You're up to
    /// date!" `QMessageBox` (`about_dialog.py:119`).
    UpToDate,
    /// The check itself failed (network/parse error); the detail string
    /// isn't shown inline (humm points at the log file instead — see
    /// `about-check-failed`'s catalog text), but is kept here for a future
    /// richer surface / for tests to assert against.
    Failed(String),
}

/// The About window's persistent state, owned by `HummApp`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AboutState {
    /// Whether a manual check is currently in flight (disables the button;
    /// lets `app.rs` tell the About window's own manual-check notices apart
    /// from the startup check's — see `services::UpdateNotice`'s doc
    /// comment).
    pub checking: bool,
    /// The last manual check's result, if it resolved to "up to date" or
    /// "failed" (an "available" result instead opens the update dialog —
    /// see `app.rs` — so it never lands here).
    pub result: Option<AboutCheckResult>,
}

/// What this frame's [`ui`] call wants `app.rs` to do — collected rather
/// than performed inline because none of the three actions ("start a manual
/// check", "open the log folder", "close this window") are things this
/// module can do by itself: they need `Services` (settings/update channel)
/// or the surrounding viewport, neither of which a plain, kittest-drivable
/// function has access to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AboutAction {
    /// "Check for updates" was clicked (only reachable while
    /// `!state.checking` — the button is disabled otherwise).
    pub check_clicked: bool,
    /// "Show logs" was clicked.
    pub show_logs_clicked: bool,
    /// "Close" was clicked.
    pub close_clicked: bool,
}

/// Renders the About window's content: "humm" + "Version `<version>`",
/// Check-for-updates/Show-logs/Close buttons, and — once a manual check
/// resolves to something other than "available" — an inline "You're up to
/// date!" or "Update check failed — see the log file" label.
pub fn ui(ui: &mut egui::Ui, version: &str, state: &AboutState) -> AboutAction {
    let mut action = AboutAction::default();

    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        ui.heading(tr("about-app-name"));
        ui.label(format!("{} {version}", tr("about-version-prefix")));
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            let check_response =
                ui.add_enabled(!state.checking, egui::Button::new(tr("about-check-button")));
            if check_response.clicked() {
                action.check_clicked = true;
            }
            if ui.button(tr("about-show-logs-button")).clicked() {
                action.show_logs_clicked = true;
            }
            if ui.button(tr("about-close-button")).clicked() {
                action.close_clicked = true;
            }
        });

        match &state.result {
            Some(AboutCheckResult::UpToDate) => {
                ui.add_space(8.0);
                ui.label(tr("about-up-to-date"));
            }
            Some(AboutCheckResult::Failed(_)) => {
                ui.add_space(8.0);
                ui.label(tr("about-check-failed"));
            }
            None => {}
        }
    });

    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn default_state_is_idle_with_no_result() {
        let state = AboutState::default();
        assert!(!state.checking);
        assert_eq!(state.result, None);
    }

    #[test]
    fn about_action_default_is_all_false() {
        assert_eq!(
            AboutAction::default(),
            AboutAction {
                check_clicked: false,
                show_logs_clicked: false,
                close_clicked: false,
            }
        );
    }
}
