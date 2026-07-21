//! The main window's chrome and central content (F-004/F-026, Task 8 adds
//! the menu bar, toolbar, store-health banner, Clear-History confirm modal,
//! and wires the Rename/Notes dialog + toast stack in on top of Task 6/7's
//! hero/table).
//!
//! This module renders; it never calls `Services` directly (no store/queue
//! access) — every action it can't perform itself is either pushed onto the
//! `actions: &mut Vec<TableAction>` accumulator (Task 7's pattern: toolbar
//! Cancel/Clear share it with the table's own context-menu/keyboard
//! actions) or returned via [`ShowResult`] (the Rename/Notes dialog outcome,
//! the Clear-confirm decision, and any toast click). `app.rs` is the sole
//! consumer of both.

use std::time::Instant;

use humm_core::domain::Status;

use crate::i18n::tr;
use crate::shortcuts::{ShortcutAction, ShortcutMap};
use crate::viewmodel::{TableAction, TasksVm};
use crate::views::rename_notes::{self, Dialog, DialogOutcome};
use crate::views::task_table::{self, TableLayout};
use crate::views::toasts::{self, ToastAction, ToastStack};

/// Which import flow (if any) the user just requested via the toolbar's
/// "New File"/"New URL" buttons, the File menu, or their shortcuts.
/// **This task only sets this field** — Task 9's file/URL/folder-dialog
/// wiring is the consumer; nothing reads it yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PendingImportRequest {
    /// Nothing pending.
    #[default]
    None,
    /// "New File Transcription" / File → "Import File…" / ⌘O.
    File,
    /// "New URL Transcription" / File → "Import URL…" / ⌘U.
    Url,
    /// File → "Import Folder…" (no shortcut, no toolbar button — Buzz
    /// parity: folder import is menu-only).
    Folder,
}

/// The persistent multi-window-flow state Task 8 introduces:
/// import/About/Preferences requests (stub fields Task 9/11 consume), the
/// Clear-History confirm modal, the Rename/Notes dialog, the toast stack,
/// and the store-health banner. Owned by `HummApp`, threaded through
/// [`show`] each frame.
#[derive(Debug, Default)]
pub struct ActionState {
    /// Set by the toolbar/File-menu import buttons/shortcuts; consumed by
    /// Task 9.
    pub pending_import: PendingImportRequest,
    /// Set by Help → "About humm"; a no-op stub this task — Task 11 builds
    /// the About window.
    pub about_requested: bool,
    /// Set by Help → "Preferences…" / ⌘,; a no-op stub this task — Task 11
    /// builds the Preferences window.
    pub preferences_requested: bool,
    /// Whether the Clear-History confirm modal is currently open.
    pub clear_confirm_open: bool,
    /// The Rename/Notes dialog (at most one open at a time).
    pub dialog: Dialog,
    /// The toast stack (completed/failed/canceled notices).
    pub toasts: ToastStack,
    /// The `StoreWriteFailed` health banner.
    pub banner: StoreHealthBanner,
}

/// The store-health banner's state machine (F-004): hidden until the first
/// `StoreWriteFailed` event, then a persistent (non-auto-dismissing)
/// message with a counter suffix for every subsequent event while it's
/// still visible. Dismissing hides it *and* resets the counter, so the
/// banner "reappears on the next event" (per the brief) as a fresh
/// single-occurrence notice, not a continuation of the old count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreHealthBanner {
    visible: bool,
    count: u32,
}

impl StoreHealthBanner {
    /// Records one more `StoreWriteFailed` event: shows the banner (if
    /// hidden) and increments its counter.
    pub fn on_store_write_failed(&mut self) {
        self.count += 1;
        self.visible = true;
    }

    /// Dismisses the banner and resets its counter.
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.count = 0;
    }

    /// Whether the banner is currently shown.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The banner's current text, or `None` while hidden: the base message
    /// alone for exactly one occurrence, else with a `" (N)"` counter
    /// suffix. The suffix is plain `format!` interpolation, not a Fluent
    /// placeable — same rationale as `format.rs`'s status/ETA text
    /// (`tr_args` wraps substituted values in Unicode bidi-isolation marks
    /// by default, which this exact-count display has no use for).
    pub fn message(&self) -> Option<String> {
        if !self.visible {
            return None;
        }
        let base = tr("banner-store-health");
        if self.count > 1 {
            Some(format!("{base} ({})", self.count))
        } else {
            Some(base)
        }
    }
}

/// Buzz's toolbar enablement rule, verbatim
/// (`main_window.py:350-382`/`should_enable_stop_transcription_action`/
/// `should_enable_clear_history_action`): both actions require a
/// **non-empty** selection where *every* selected row's status is in the
/// relevant set. Pure and `egui`-free so the enablement matrix is directly
/// unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolbarEnablement {
    /// Cancel Transcription: enabled iff every selected row is
    /// queued/in-progress.
    pub cancel: bool,
    /// Clear History: enabled iff every selected row is in a terminal
    /// status (completed/failed/canceled/skipped).
    pub clear: bool,
}

/// Computes [`ToolbarEnablement`] from the current selection's statuses.
pub fn toolbar_enablement(statuses: &[Status]) -> ToolbarEnablement {
    let non_empty = !statuses.is_empty();
    ToolbarEnablement {
        cancel: non_empty
            && statuses
                .iter()
                .all(|s| matches!(s, Status::InProgress | Status::Queued)),
        clear: non_empty
            && statuses.iter().all(|s| {
                matches!(
                    s,
                    Status::Completed | Status::Failed | Status::Canceled | Status::Skipped
                )
            }),
    }
}

/// Everything this frame's `show` produced that `app.rs` (not this module)
/// must act on: it owns `Services` (store/queue), which this module never
/// touches.
#[derive(Debug, Default)]
pub struct ShowResult {
    /// A Rename/Notes dialog was confirmed with something to persist.
    pub dialog_outcome: Option<DialogOutcome>,
    /// The Clear-History confirm modal's Ok button was clicked this frame.
    pub clear_confirmed: bool,
    /// Any actions requested from a toast/its detail popover.
    pub toast_actions: Vec<ToastAction>,
}

/// Renders the main window: global shortcuts, menu bar, toolbar, the
/// store-health banner (when visible), the hero/table body, and the
/// Clear-confirm/Rename/Notes modals plus the toast stack — in that order
/// (later elements draw on top).
///
/// `ui` is the root viewport's `egui::Ui` (eframe 0.35 hands this directly).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    vm: &mut TasksVm,
    layout: &mut TableLayout,
    actions: &mut Vec<TableAction>,
    shortcuts: &ShortcutMap,
    state: &mut ActionState,
    now: Instant,
) -> ShowResult {
    handle_global_shortcuts(ui, actions, shortcuts, state);

    egui::Panel::top("humm-menu-bar").show(ui, |ui| {
        show_menu_bar(ui, shortcuts, state);
    });
    egui::Panel::top("humm-toolbar").show(ui, |ui| {
        show_toolbar(ui, vm, actions, state);
    });
    if state.banner.visible() {
        egui::Panel::top("humm-store-health-banner").show(ui, |ui| {
            show_banner(ui, &mut state.banner);
        });
    }

    egui::CentralPanel::default().show(ui, |ui| {
        if vm.sorted_rows().is_empty() {
            show_hero(ui, state);
        } else {
            task_table::ui(ui, vm, layout, actions);
        }
    });

    let clear_confirmed = show_clear_confirm(ui.ctx(), &mut state.clear_confirm_open);
    let dialog_outcome = rename_notes::ui(ui.ctx(), &mut state.dialog);
    let toast_actions = toasts::ui(ui.ctx(), &mut state.toasts, vm, now);

    ShowResult {
        dialog_outcome,
        clear_confirmed,
        toast_actions,
    }
}

/// Consumes the five Task-8 global shortcuts (⌘X/⌘S/⌘O/⌘U/⌘,), routing each
/// to the same effect its toolbar/menu counterpart has. ⌘X/⌘S push onto
/// `actions` (the same accumulator the toolbar buttons use) rather than
/// acting directly — `app.rs`'s single drain loop re-gates both (defense in
/// depth), so a shortcut can never bypass the enablement rule a click
/// would've been subject to.
fn handle_global_shortcuts(
    ui: &mut egui::Ui,
    actions: &mut Vec<TableAction>,
    shortcuts: &ShortcutMap,
    state: &mut ActionState,
) {
    ui.input_mut(|input| {
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::StopTranscription)) {
            actions.push(TableAction::CancelSelected);
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::ClearHistory)) {
            actions.push(TableAction::ClearSelected);
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::OpenImportWindow)) {
            state.pending_import = PendingImportRequest::File;
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::OpenImportUrlWindow)) {
            state.pending_import = PendingImportRequest::Url;
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::OpenPreferencesWindow)) {
            state.preferences_requested = true;
        }
    });
}

/// The in-window menu bar (design spec §3.3/deviation #1: egui has no
/// native macOS menu bar; this is function parity). File → Import File…
/// (⌘O) / Import URL… (⌘U) / Import Folder… (no shortcut); Help → About
/// humm / Help (disabled, no docs site yet) / Preferences… (⌘,). Buzz's
/// "Plugins…" entry is not ported (ADR-010).
fn show_menu_bar(ui: &mut egui::Ui, shortcuts: &ShortcutMap, state: &mut ActionState) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(tr("menu-file"), |ui| {
            let import_file_shortcut = shortcuts.get(ShortcutAction::OpenImportWindow);
            let hint = ui.ctx().format_shortcut(&import_file_shortcut);
            if ui
                .add(egui::Button::new(tr("menu-file-import-file")).shortcut_text(hint))
                .clicked()
            {
                state.pending_import = PendingImportRequest::File;
                ui.close();
            }
            let import_url_shortcut = shortcuts.get(ShortcutAction::OpenImportUrlWindow);
            let hint = ui.ctx().format_shortcut(&import_url_shortcut);
            if ui
                .add(egui::Button::new(tr("menu-file-import-url")).shortcut_text(hint))
                .clicked()
            {
                state.pending_import = PendingImportRequest::Url;
                ui.close();
            }
            if ui.button(tr("menu-file-import-folder")).clicked() {
                state.pending_import = PendingImportRequest::Folder;
                ui.close();
            }
        });
        ui.menu_button(tr("menu-help-title"), |ui| {
            if ui.button(tr("menu-help-about")).clicked() {
                state.about_requested = true;
                ui.close();
            }
            ui.add_enabled(false, egui::Button::new(tr("menu-help-docs")))
                .on_disabled_hover_text(tr("menu-help-docs-tooltip"));
            let preferences_shortcut = shortcuts.get(ShortcutAction::OpenPreferencesWindow);
            let hint = ui.ctx().format_shortcut(&preferences_shortcut);
            if ui
                .add(egui::Button::new(tr("menu-help-preferences")).shortcut_text(hint))
                .clicked()
            {
                state.preferences_requested = true;
                ui.close();
            }
        });
    });
}

/// The toolbar (design spec §3.3): Record (disabled) · New File
/// Transcription · New URL Transcription · Open Transcript (disabled) ·
/// Cancel Transcription · Clear History · [Update chip — hidden, Task 11].
fn show_toolbar(
    ui: &mut egui::Ui,
    vm: &TasksVm,
    actions: &mut Vec<TableAction>,
    state: &mut ActionState,
) {
    let enablement = toolbar_enablement(&vm.selected_statuses());
    ui.horizontal(|ui| {
        ui.add_enabled(false, egui::Button::new(tr("toolbar-record")))
            .on_disabled_hover_text(tr("toolbar-record-tooltip"));
        ui.separator();
        if ui.button(tr("toolbar-new-file")).clicked() {
            state.pending_import = PendingImportRequest::File;
        }
        if ui.button(tr("toolbar-new-url")).clicked() {
            state.pending_import = PendingImportRequest::Url;
        }
        ui.add_enabled(false, egui::Button::new(tr("toolbar-open-transcript")))
            .on_disabled_hover_text(tr("toolbar-open-transcript-tooltip"));
        if ui
            .add_enabled(enablement.cancel, egui::Button::new(tr("toolbar-cancel")))
            .clicked()
        {
            actions.push(TableAction::CancelSelected);
        }
        if ui
            .add_enabled(enablement.clear, egui::Button::new(tr("toolbar-clear")))
            .clicked()
        {
            actions.push(TableAction::ClearSelected);
        }
        ui.separator();
        // Update-available chip: hidden this task — Task 11 fills it in.
    });
}

/// The persistent store-health banner: base message plus a dismiss button.
fn show_banner(ui: &mut egui::Ui, banner: &mut StoreHealthBanner) {
    let Some(message) = banner.message() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(message);
        if ui.button("\u{00d7}").clicked() {
            banner.dismiss();
        }
    });
}

/// The Clear-History confirm modal (Buzz parity strings, relabeled Ok/
/// Cancel buttons — `on_clear_history_action_triggered`,
/// `main_window.py:232-255`). Returns whether Ok was clicked this frame;
/// Cancel/Escape/backdrop-click just close it.
fn show_clear_confirm(ctx: &egui::Context, open: &mut bool) -> bool {
    if !*open {
        return false;
    }
    let mut confirmed = false;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("humm-clear-confirm")).show(ctx, |ui| {
        ui.heading(tr("clear-confirm-title"));
        ui.label(tr("clear-confirm-body"));
        ui.horizontal(|ui| {
            if ui.button(tr("clear-confirm-ok")).clicked() {
                confirmed = true;
                close = true;
            }
            if ui.button(tr("clear-confirm-cancel")).clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        *open = false;
    }
    confirmed
}

/// The empty-state hero: a centered invitation to import media, plus an
/// explicit button (design spec §3.4). The button now sets the same
/// `pending_import` field the toolbar's "New File Transcription" does —
/// Task 9's dialog wiring is the actual consumer either way.
fn show_hero(ui: &mut egui::Ui, state: &mut ActionState) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.heading(tr("hero-drop"));
        ui.add_space(12.0);
        if ui.button(tr("hero-import-button")).clicked() {
            state.pending_import = PendingImportRequest::File;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- toolbar_enablement -----------------------------------------------

    #[test]
    fn empty_selection_disables_both_actions() {
        let enablement = toolbar_enablement(&[]);
        assert_eq!(enablement, ToolbarEnablement::default());
    }

    #[test]
    fn all_queued_or_in_progress_enables_cancel_only() {
        let enablement = toolbar_enablement(&[Status::Queued, Status::InProgress]);
        assert!(enablement.cancel);
        assert!(!enablement.clear);
    }

    #[test]
    fn all_terminal_enables_clear_only() {
        let enablement = toolbar_enablement(&[
            Status::Completed,
            Status::Failed,
            Status::Canceled,
            Status::Skipped,
        ]);
        assert!(!enablement.cancel);
        assert!(enablement.clear);
    }

    #[test]
    fn mixed_selection_disables_both() {
        let enablement = toolbar_enablement(&[Status::Queued, Status::Completed]);
        assert!(!enablement.cancel);
        assert!(!enablement.clear);
    }

    #[test]
    fn single_queued_row_enables_cancel() {
        let enablement = toolbar_enablement(&[Status::Queued]);
        assert!(enablement.cancel);
    }

    #[test]
    fn single_completed_row_enables_clear() {
        let enablement = toolbar_enablement(&[Status::Completed]);
        assert!(enablement.clear);
    }

    // -- StoreHealthBanner --------------------------------------------------

    #[test]
    fn banner_hidden_initially() {
        let banner = StoreHealthBanner::default();
        assert!(!banner.visible());
        assert_eq!(banner.message(), None);
    }

    #[test]
    fn banner_shows_base_message_on_first_event() {
        let mut banner = StoreHealthBanner::default();
        banner.on_store_write_failed();
        assert!(banner.visible());
        assert_eq!(banner.message(), Some(tr("banner-store-health")));
    }

    #[test]
    fn banner_appends_counter_on_subsequent_events() {
        let mut banner = StoreHealthBanner::default();
        banner.on_store_write_failed();
        banner.on_store_write_failed();
        banner.on_store_write_failed();
        let msg = banner.message().expect("visible");
        assert!(msg.contains("(3)"), "expected a (3) suffix, got {msg:?}");
    }

    #[test]
    fn dismiss_hides_and_resets_counter() {
        let mut banner = StoreHealthBanner::default();
        banner.on_store_write_failed();
        banner.on_store_write_failed();
        banner.dismiss();
        assert!(!banner.visible());
        assert_eq!(banner.message(), None);
    }

    #[test]
    fn dismissed_banner_reappears_fresh_on_next_event() {
        let mut banner = StoreHealthBanner::default();
        banner.on_store_write_failed();
        banner.on_store_write_failed();
        banner.dismiss();

        banner.on_store_write_failed();

        assert!(banner.visible());
        // Fresh reappearance: exactly the base message, no "(N)" suffix.
        assert_eq!(banner.message(), Some(tr("banner-store-health")));
    }
}
