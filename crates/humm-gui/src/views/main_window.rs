//! The main window's central content.
//!
//! This task adds the task table (F-003) alongside the empty-state hero
//! (Task 6): the hero shows when there are no rows to display, the table
//! otherwise. The menu bar/toolbar (F-004) and drop-overlay/toast/banner
//! chrome still arrive in later M3 tasks
//! (`docs/superpowers/specs/2026-07-21-m3-gui-shell-design.md` §1's module
//! layout).

use crate::i18n::tr;
use crate::viewmodel::{TableAction, TasksVm};
use crate::views::task_table::{self, TableLayout};

/// Renders the main window's central panel: the empty-state hero when
/// `vm` has no rows, the virtualized task table otherwise.
///
/// `ui` is the root viewport's `egui::Ui` (eframe 0.35's `App::ui` hands this
/// directly rather than a bare `Context`); wrapping it in `CentralPanel`
/// gives the usual margin/background egui apps expect.
pub fn show(
    ui: &mut egui::Ui,
    vm: &mut TasksVm,
    layout: &mut TableLayout,
    actions: &mut Vec<TableAction>,
) {
    egui::CentralPanel::default().show(ui, |ui| {
        if vm.sorted_rows().is_empty() {
            show_hero(ui);
        } else {
            task_table::ui(ui, vm, layout, actions);
        }
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
