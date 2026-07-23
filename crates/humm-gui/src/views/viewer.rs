//! The transcript-viewer window chrome (F-080): toolbar (View menu live; the
//! rest disabled stubs this task), the per-mode central content, and the
//! empty-state label.
//!
//! A pure render function of [`ViewerVm`] — no window/viewport plumbing (that
//! is `app.rs`'s deferred-viewport management), so an `egui_kittest` harness
//! can drive it directly over a bare `egui::Ui` (`Harness::new_ui_state`),
//! exactly as the M3 preferences/models views are tested.
//!
//! Ported from `transcription_viewer_widget.py` (`_setup_toolbar`,
//! `reset_view`) and `transcription_view_mode_tool_button.py` (the View menu).
//! Playback controls, the editor table, the find bar, export, and resize are
//! filled in by Tasks 8-12; here they are disabled buttons with tooltips.

use std::sync::Mutex;

use humm_core::settings::Settings;

use crate::i18n::tr;
use crate::shortcuts::{ShortcutAction, ShortcutMap};
use crate::viewer::{PlayerState, ViewMode, ViewerDeps, ViewerVm, display_timestamp};

/// Renders one transcript viewer's chrome into `ui`.
///
/// `deps`/`settings` are part of the stable render signature the later M4
/// tasks consume (playback opens streams through `deps`; splitter/rate
/// persistence writes `settings`); this task renders neither, so they are
/// accepted and reserved rather than read — see the `let _` below.
pub fn viewer_ui(
    ui: &mut egui::Ui,
    vm: &mut ViewerVm,
    deps: &ViewerDeps,
    shortcuts: &ShortcutMap,
    settings: &Mutex<Settings>,
) {
    // Reserved for later M4 tasks (Task 10 playback opens streams through
    // `deps`; Tasks 8/10 persist splitter/rate into `settings`). The window
    // skeleton this task renders touches neither.
    let _ = (deps, settings);

    consume_mode_shortcuts(ui, vm, shortcuts);
    show_toolbar(ui, vm, shortcuts);
    ui.separator();

    if vm.rows.is_empty() {
        show_empty_state(ui);
        return;
    }

    match vm.mode {
        ViewMode::Timestamps => show_timestamps(ui, vm),
        ViewMode::Text | ViewMode::Translation => show_text_body(ui, vm),
    }
}

/// Consumes the three view-mode shortcuts (⌘E/⌘L/⌘T) — but only from *this*
/// viewport's input, so they act on the focused viewer alone (design spec §7).
/// ⌘L is a no-op without translations (guarded inside [`ViewerVm::set_mode`]).
fn consume_mode_shortcuts(ui: &egui::Ui, vm: &mut ViewerVm, shortcuts: &ShortcutMap) {
    let (text, translation, timestamps) = ui.input_mut(|input| {
        (
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ViewTranscriptText)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ViewTranscriptTranslation)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ViewTranscriptTimestamps)),
        )
    });
    if text {
        vm.set_mode(ViewMode::Text);
    }
    if translation {
        vm.set_mode(ViewMode::Translation);
    }
    if timestamps {
        vm.set_mode(ViewMode::Timestamps);
    }
}

/// The toolbar: View menu (live) then the disabled-stub buttons. The two
/// permanent-tooltip buttons (Translate → M6 API backend, Identify Speakers →
/// M8) name their milestone; the three this-milestone buttons (Export/Resize/
/// Find) get a "lands later this milestone" tooltip until their task enables
/// them (Tasks 11/12/9).
fn show_toolbar(ui: &mut egui::Ui, vm: &mut ViewerVm, shortcuts: &ShortcutMap) {
    ui.horizontal_wrapped(|ui| {
        ui.menu_button(tr("viewer-view"), |ui| {
            if mode_menu_entry(
                ui,
                tr("viewer-mode-text"),
                shortcuts,
                ShortcutAction::ViewTranscriptText,
            ) {
                vm.set_mode(ViewMode::Text);
                ui.close();
            }
            // Translation entry is visible only when the transcript has
            // translations (Buzz `translation_action.setVisible(has_translation)`).
            if vm.has_translations
                && mode_menu_entry(
                    ui,
                    tr("viewer-mode-translation"),
                    shortcuts,
                    ShortcutAction::ViewTranscriptTranslation,
                )
            {
                vm.set_mode(ViewMode::Translation);
                ui.close();
            }
            if mode_menu_entry(
                ui,
                tr("viewer-mode-timestamps"),
                shortcuts,
                ShortcutAction::ViewTranscriptTimestamps,
            ) {
                vm.set_mode(ViewMode::Timestamps);
                ui.close();
            }
        });
        ui.separator();
        disabled_button(ui, tr("viewer-export"), tr("viewer-export-pending"));
        disabled_button(ui, tr("viewer-translate"), tr("viewer-translate-tooltip"));
        disabled_button(ui, tr("viewer-resize"), tr("viewer-resize-pending"));
        disabled_button(
            ui,
            tr("viewer-identify-speakers"),
            tr("viewer-identify-speakers-tooltip"),
        );
        disabled_button(ui, tr("viewer-find"), tr("viewer-find-pending"));
    });
}

/// One View-menu entry with its shortcut hint; returns whether it was clicked.
fn mode_menu_entry(
    ui: &mut egui::Ui,
    label: String,
    shortcuts: &ShortcutMap,
    action: ShortcutAction,
) -> bool {
    let hint = ui.ctx().format_shortcut(&shortcuts.get(action));
    ui.add(egui::Button::new(label).shortcut_text(hint))
        .clicked()
}

/// A permanently-disabled toolbar button carrying `tooltip` on hover.
fn disabled_button(ui: &mut egui::Ui, label: String, tooltip: String) {
    ui.add_enabled(false, egui::Button::new(label))
        .on_disabled_hover_text(tooltip);
}

/// The empty-transcript state: a centered explanatory label (UX_SPEC §1).
fn show_empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(tr("viewer-empty"));
    });
}

/// Timestamps mode: a virtualized placeholder list of the segments (one row
/// per segment: `HH:MM:SS.mmm  text`) standing in for the editor table Task 8
/// ships, plus the stub player strip. Virtualized via `show_rows` so even a
/// 100k-segment transcript renders only the visible rows.
fn show_timestamps(ui: &mut egui::Ui, vm: &ViewerVm) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let available = ui.available_height();
    // Reserve a strip for the stub player at the bottom.
    let player_strip = row_height * 2.0;
    let table_height = (available - player_strip).max(row_height);

    egui::ScrollArea::vertical()
        .max_height(table_height)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, vm.rows.len(), |ui, range| {
            for row in &vm.rows[range] {
                ui.label(format!("{}  {}", display_timestamp(row.start), row.text));
            }
        });

    show_player_strip(ui, vm);
}

/// The stub player strip (Task 10 replaces it with the real audio/video
/// player). While the player is [`PlayerState::Unavailable`] it is an inert,
/// muted placeholder — never a fake "play" affordance.
fn show_player_strip(ui: &mut egui::Ui, vm: &ViewerVm) {
    ui.separator();
    match vm.player {
        PlayerState::Unavailable => {
            ui.add_enabled(
                false,
                egui::Label::new(egui::RichText::new(tr("viewer-player-pending")).weak()),
            );
        }
    }
}

/// Text / Translation mode: the recomputed combined text in a read-only,
/// selectable, scrollable area (Buzz's read-only `text_display_box`).
fn show_text_body(ui: &mut egui::Ui, vm: &ViewerVm) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(egui::Label::new(&vm.text_body).selectable(true).wrap());
        });
}
