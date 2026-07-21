//! The main window's central content.
//!
//! This task ships only the empty-state hero: the task table (F-003), menu
//! bar/toolbar (F-004), and drop-overlay/toast/banner chrome all arrive in
//! later M3 tasks (`docs/superpowers/specs/2026-07-21-m3-gui-shell-design.md`
//! §1's module layout). Until the table exists, the hero is always shown —
//! there is no "populated" state yet to switch away from it.

use crate::i18n::tr;

/// Renders the main window's central panel.
///
/// `ui` is the root viewport's `egui::Ui` (eframe 0.35's `App::ui` hands this
/// directly rather than a bare `Context`); wrapping it in `CentralPanel`
/// gives the usual margin/background egui apps expect.
pub fn show(ui: &mut egui::Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        show_hero(ui);
    });
}

/// The empty-state hero: a centered invitation to import media, plus an
/// explicit button (design spec §3.4 — the drag-drop overlay and file dialog
/// wiring itself land in a later task; the button exists now so the hero's
/// layout and copy are final).
fn show_hero(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.heading(tr("hero-drop"));
        ui.add_space(12.0);
        if ui.button(tr("hero-import-button")).clicked() {
            // File-dialog wiring (F-020, `rfd`) lands with the import-flow
            // task; the button is present now so its i18n key and placement
            // are settled.
        }
    });
}
