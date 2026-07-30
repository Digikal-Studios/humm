//! The toolbar "Update Available" chip and its modal dialog (F-006,
//! ADR-015): notify-only — no installer download/launch (Buzz's own
//! `update_dialog.py` "Download and Install" flow is deliberately not
//! ported), just a link to the release page.
//!
//! Self-contained (no [`crate::services::Services`] access needed — same
//! shape as [`crate::views::url_dialog`]/[`crate::views::rename_notes`]):
//! [`UpdateNoticeState`] is driven by `app.rs` draining
//! [`crate::services::UpdateNotice`]s off `Services::update_rx`, and
//! rendered from `main_window.rs` (the chip inside `show_toolbar`, the
//! dialog alongside the other modals in `show`).

use humm_core::update::UpdateInfo;

use crate::i18n::tr;

/// humm's releases page (M9: the public GitHub mirror, `digikal/humm`) — the
/// update dialog's "Open release page" button (notify-only, ADR-015: no
/// installer download/launch in v1).
pub const RELEASES_PAGE_URL: &str = "https://github.com/digikal/humm/releases/latest";

/// The toolbar chip + dialog's live state: `info` drives the chip's
/// visibility (`Some` ⇒ shown) and the dialog's contents; `dialog_open` is
/// independent of `info` being `Some` so the dialog can be dismissed without
/// losing the chip (clicking it again reopens the same dialog).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct UpdateNoticeState {
    /// The available update, once known (persisted-stale from a previous
    /// session, or fresh from this session's check) — `None` hides the chip
    /// entirely.
    pub info: Option<UpdateInfo>,
    /// Whether the update dialog (opened by clicking the chip) is showing.
    pub dialog_open: bool,
}

/// Renders the toolbar chip: nothing while `state.info` is `None` (Task 8's
/// hidden-by-default placeholder); a button labeled "Update Available"
/// otherwise, which opens the dialog on click. Call from `show_toolbar`.
pub fn chip_ui(ui: &mut egui::Ui, state: &mut UpdateNoticeState) {
    if state.info.is_none() {
        return;
    }
    if ui.button(tr("update-chip-label")).clicked() {
        state.dialog_open = true;
    }
}

/// Renders the update dialog (a modal) iff `state.dialog_open` — a no-op
/// otherwise. Call from `main_window::show` alongside the other modals.
///
/// Contents (binding decision): title "Update Available", "A new version of
/// humm is available!", current/new version lines, release notes in a
/// plain-text scroll area (max height ~150, only when non-empty — Buzz's
/// own dialog only shows the section at all when `release_notes` is
/// truthy), "Open release page" (`open::that`), and an explicit Close button
/// (egui modals need one — Buzz's own dialog is chrome-only, a documented
/// deviation).
pub fn dialog_ui(ctx: &egui::Context, state: &mut UpdateNoticeState, current_version: &str) {
    if !state.dialog_open {
        return;
    }
    let Some(info) = state.info.clone() else {
        // `info` was cleared out from under an already-open dialog (e.g. a
        // fresh check found nothing newer) — nothing left to show.
        state.dialog_open = false;
        return;
    };

    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("humm-update-dialog")).show(ctx, |ui| {
        ui.set_min_width(360.0);
        ui.heading(tr("update-dialog-title"));
        ui.label(tr("update-dialog-header"));
        ui.label(format!(
            "{} {current_version}",
            tr("update-dialog-current-version")
        ));
        ui.label(format!(
            "{} {}",
            tr("update-dialog-new-version"),
            info.version
        ));

        if !info.release_notes.is_empty() {
            ui.separator();
            ui.label(tr("update-dialog-release-notes"));
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .show(ui, |ui| {
                    ui.label(&info.release_notes);
                });
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(tr("update-dialog-open-release-page")).clicked()
                && let Err(err) = open::that(RELEASES_PAGE_URL)
            {
                log::warn!("could not open the release page: {err}");
            }
            if ui.button(tr("update-dialog-close")).clicked() {
                close = true;
            }
        });
    });

    if close || modal.should_close() {
        state.dialog_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn info(version: &str) -> UpdateInfo {
        UpdateInfo {
            version: version.to_string(),
            release_notes: String::new(),
            download_urls: Vec::new(),
        }
    }

    #[test]
    fn default_state_has_no_info_and_a_closed_dialog() {
        let state = UpdateNoticeState::default();
        assert_eq!(state.info, None);
        assert!(!state.dialog_open);
    }

    #[test]
    fn releases_page_url_matches_the_binding_decision() {
        assert_eq!(
            RELEASES_PAGE_URL,
            "https://github.com/digikal/humm/releases/latest"
        );
    }

    #[test]
    fn state_with_info_is_ready_for_the_chip_to_render() {
        // The chip's own visibility check (`state.info.is_some()`) is
        // exercised end-to-end via kittest (drives real egui rendering);
        // this just pins the state shape the check reads.
        let state = UpdateNoticeState {
            info: Some(info("2.0.0")),
            dialog_open: false,
        };
        assert!(state.info.is_some());
    }
}
