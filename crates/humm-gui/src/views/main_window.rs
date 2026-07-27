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

use std::sync::Arc;
use std::time::Instant;

use humm_core::domain::Status;
use humm_core::secrets::SecretStore;

use crate::i18n::{tr, tr_args};
use crate::shortcuts::{ShortcutAction, ShortcutMap};
use crate::viewmodel::{TableAction, TasksVm};
use crate::views::options_sheet::{self, OptionsSheet, SheetOutcome};
use crate::views::rename_notes::{self, Dialog, DialogOutcome};
use crate::views::task_table::{self, TableLayout};
use crate::views::toasts::{self, ToastAction, ToastStack};
use crate::views::update_notice::{self, UpdateNoticeState};
use crate::views::url_dialog::{self, UrlDialog};

/// Which import flow (if any) the user just requested via the toolbar's
/// "New File"/"New URL" buttons, the File menu, or their shortcuts. Set
/// here (Task 8); consumed by `HummApp::drain_pending_import` (Task 9),
/// which dispatches the actual file/URL/folder dialog flow.
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
/// import/About/Preferences requests (the import field is fully wired as of
/// Task 9; About/Preferences stay stub fields for Task 11), the
/// Clear-History confirm modal, the Rename/Notes dialog, the toast stack,
/// and the store-health banner. Owned by `HummApp`, threaded through
/// [`show`] each frame.
///
/// The options sheet (F-023) and URL dialog (F-022) are Task 9 additions:
/// `app.rs` opens/populates `options_sheet` (it needs `Services`' settings,
/// which this module never touches); `url_dialog` is fully self-contained
/// (no `Services` access needed) and this module drives it directly.
#[derive(Debug, Default)]
pub struct ActionState {
    /// Set by the toolbar/File-menu import buttons/shortcuts; consumed by
    /// `app.rs` right after `show` returns (`HummApp::drain_pending_import`).
    pub pending_import: PendingImportRequest,
    /// Set by the toolbar Record button / Ctrl+R (F-004/F-060): `app.rs`
    /// opens (or focuses) the live-recording window on the next drain,
    /// mirroring `about_requested`/`preferences_requested`'s own one-shot
    /// flag shape.
    pub record_requested: bool,
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
    /// The open options sheet (F-023), if any — opened by `app.rs` from a
    /// file dialog pick, a folder scan, a dropped-files event, argv, or a
    /// confirmed URL dialog.
    pub options_sheet: Option<OptionsSheet>,
    /// The URL-import dialog (F-022): closed, or open with its
    /// in-progress text/validation error.
    pub url_dialog: UrlDialog,
    /// The toolbar's "Update Available" chip + its dialog (F-006, ADR-015):
    /// `app.rs` seeds `info` at startup (persisted-stale chip,
    /// `services::chip_visible_on_startup`) and updates it as
    /// `Services::update_rx` notices arrive; this module only renders it.
    pub update_notice: UpdateNoticeState,
}

impl ActionState {
    /// Whether any modal currently has the user's attention: the Rename/
    /// Notes dialog, the Clear-History confirm, a failed-task toast's
    /// error-detail popover, the options sheet, or the URL dialog. Global
    /// shortcuts (`handle_global_shortcuts`) and the table's own Delete/⌘C/
    /// Return handling (`task_table::handle_keyboard_shortcuts`) must both
    /// be suppressed while this is true — a modal's text field owns
    /// keyboard input, and forwarding e.g. Delete/⌘X to the table
    /// underneath would silently act on the (still-selected, now-hidden-
    /// behind-the-modal) row instead of whatever the user is actually
    /// looking at. Concrete collisions this fixes: Delete while editing a
    /// Rename field previously opened the Clear-confirm modal stacked on
    /// top of it; ⌘X swallowed a cut and canceled the selected row; ⌘S
    /// stacked a second confirm modal.
    pub fn modal_open(&self) -> bool {
        self.clear_confirm_open
            || !matches!(self.dialog, Dialog::None)
            || self.toasts.detail.is_some()
            || self.options_sheet.is_some()
            || !matches!(self.url_dialog, UrlDialog::None)
            || self.update_notice.dialog_open
    }
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
    /// Open Transcript: enabled iff the selection is non-empty and every
    /// selected row can be opened (completed/skipped — `can_open`,
    /// `main_window.py:350-364`). Return mirrors this same all-selected rule
    /// (design spec §2.1); double-click gates its single row separately.
    pub open: bool,
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
        open: non_empty && statuses.iter().all(|s| crate::viewer::can_open(*s)),
    }
}

/// The folder-watch toolbar pill's live state (F-030, design spec §3.3).
/// `app.rs` builds this fresh every frame from whether
/// `crate::folder_watch::FolderWatchService` is *actually* running — not
/// merely `settings.folder_watch.enabled` — so the pill only ever promises a
/// watcher that's really up (a `start()` failure, or the feature simply
/// being off/unconfigured, both mean "no pill", handled by `app.rs` passing
/// `None` to [`show`] rather than by any visibility check in this module).
#[derive(Debug, Clone, PartialEq)]
pub struct FolderWatchPillState {
    /// The watched input directory's basename (Buzz parity: the folder's
    /// own name, not its full path — `FolderWatchSnapshot::pill_folder_name`).
    pub folder_name: String,
    /// Whether the service is currently paused (the pill's own toggle,
    /// session-scoped — never a persisted setting).
    pub paused: bool,
}

/// The pill's label text: "Watching {name}", with a dimmed "— Paused" suffix
/// while paused (design spec §3.3). Pure (no `egui::Ui` involved), so it's
/// directly unit-testable.
pub fn folder_watch_pill_label(state: &FolderWatchPillState) -> String {
    let watching = tr_args(
        "folderwatch-pill-watching",
        &[("name", state.folder_name.as_str())],
    );
    if state.paused {
        format!("{watching} — {}", tr("folderwatch-pill-paused"))
    } else {
        watching
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
    /// The options sheet closed this frame (Run or Cancel/Escape/backdrop) —
    /// `app.rs` persists the sticky prefs either way and, for `Run`,
    /// enqueues every spec.
    pub sheet_outcome: Option<SheetOutcome>,
    /// The URL dialog produced a validated URL this frame — `app.rs` opens
    /// the options sheet with it (needs `Services.settings` for prefill,
    /// which this module never touches).
    pub url_dialog_result: Option<url::Url>,
    /// The folder-watch pill's pause/resume toggle button was clicked this
    /// frame (F-030, design spec §3.3) — `app.rs` flips the running
    /// service's pause flag (it owns the service, which this module never
    /// touches).
    pub folder_watch_pause_toggled: bool,
}

/// Renders the main window: global shortcuts, menu bar, toolbar, the
/// store-health banner (when visible), the hero/table body, and the
/// Clear-confirm/Rename/Notes/options-sheet/URL-dialog modals plus the
/// toast stack and drag-drop overlay — in that order (later elements draw
/// on top).
///
/// `ui` is the root viewport's `egui::Ui` (eframe 0.35 hands this directly).
/// `custom_model_available`/`include_lumii`/`yt_dlp_available`/
/// `demucs_available` are precomputed by `app.rs` from `Services`
/// (`settings.models.custom_model_url.is_some()` /
/// `crate::i18n::lumii_locale_requested` / `capabilities.yt_dlp` /
/// `capabilities.demucs`) — this module never reads `Services` directly.
/// `demucs_available` gates the options sheet's "Extract speech" checkbox
/// the same way `yt_dlp_available` gates URL import (F-027, design spec §3).
/// `current_version` is
/// `env!("CARGO_PKG_VERSION")`, threaded in for the update dialog's
/// "Current version: …" line. `secrets` is threaded straight through to the
/// options sheet's Api Key row (F-041, Task 14) — the one piece of
/// `Services` this module passes on without touching itself, same as
/// `custom_model_available`/`include_lumii`/`yt_dlp_available`.
/// `folder_watch_pill` is `app.rs`'s own precomputed pill state (F-030,
/// design spec §3.3) — `None` hides the pill entirely; this module never
/// reaches into `crate::folder_watch` itself, same "app.rs computes, this
/// module only renders" split as everything else in this list.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    vm: &mut TasksVm,
    layout: &mut TableLayout,
    actions: &mut Vec<TableAction>,
    shortcuts: &ShortcutMap,
    state: &mut ActionState,
    now: Instant,
    custom_model_available: bool,
    include_lumii: bool,
    yt_dlp_available: bool,
    demucs_available: bool,
    current_version: &str,
    secrets: &Arc<dyn SecretStore>,
    folder_watch_pill: Option<&FolderWatchPillState>,
) -> ShowResult {
    handle_global_shortcuts(ui, actions, shortcuts, state, yt_dlp_available);

    let mut folder_watch_pause_toggled = false;
    egui::Panel::top("humm-menu-bar").show(ui, |ui| {
        show_menu_bar(ui, shortcuts, state, yt_dlp_available);
    });
    egui::Panel::top("humm-toolbar").show(ui, |ui| {
        folder_watch_pause_toggled =
            show_toolbar(ui, vm, actions, state, yt_dlp_available, folder_watch_pill);
    });
    if state.banner.visible() {
        egui::Panel::top("humm-store-health-banner").show(ui, |ui| {
            show_banner(ui, &mut state.banner);
        });
    }

    let modal_open = state.modal_open();
    egui::CentralPanel::default().show(ui, |ui| {
        if vm.sorted_rows().is_empty() {
            show_hero(ui, state);
        } else {
            task_table::ui(ui, vm, layout, actions, modal_open);
        }
    });

    let clear_confirmed = show_clear_confirm(ui.ctx(), &mut state.clear_confirm_open);
    let dialog_outcome = rename_notes::ui(ui.ctx(), &mut state.dialog);
    let toast_actions = toasts::ui(ui.ctx(), &mut state.toasts, vm, now);
    let sheet_outcome = options_sheet::ui(
        ui.ctx(),
        &mut state.options_sheet,
        custom_model_available,
        include_lumii,
        demucs_available,
        secrets,
    );
    let url_dialog_result = url_dialog::ui(ui.ctx(), &mut state.url_dialog);
    update_notice::dialog_ui(ui.ctx(), &mut state.update_notice, current_version);
    show_drop_overlay(ui);

    ShowResult {
        dialog_outcome,
        clear_confirmed,
        toast_actions,
        sheet_outcome,
        url_dialog_result,
        folder_watch_pause_toggled,
    }
}

/// The full-viewport "Drop to import" veil (F-021), shown only while
/// `egui::RawInput::hovered_files` is non-empty — i.e. while the OS reports
/// files hovering over the window mid-drag, before they're actually
/// dropped. Purely visual: the actual drop (`dropped_files`) is handled by
/// `app.rs` (it needs `Services.settings` to open the options sheet), not
/// here.
fn show_drop_overlay(ui: &mut egui::Ui) {
    let hovering_files = ui.ctx().input(|input| !input.raw.hovered_files.is_empty());
    if !hovering_files {
        return;
    }
    let screen = ui.ctx().input(|input| input.viewport_rect());
    egui::Area::new(egui::Id::new("humm-drop-overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::Pos2::ZERO)
        .show(ui.ctx(), |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(180));
            ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading(tr("drop-overlay-text"));
                });
            });
        });
}

/// Consumes the six global shortcuts (⌘R/⌘X/⌘S/⌘O/⌘U/⌘, — ⌘R added F-004/
/// F-060, Task 10; the other five are Task 8's), routing each to the same
/// effect its toolbar/menu counterpart has. ⌘X/⌘S push onto `actions` (the
/// same accumulator the toolbar buttons use) rather than acting directly —
/// `app.rs`'s single drain loop re-gates both (defense in depth), so a
/// shortcut can never bypass the enablement rule a click would've been
/// subject to.
///
/// A no-op while [`ActionState::modal_open`] is true: none of these
/// shortcuts are consumed at all in that case (not even to discard them),
/// so a modal's own text field still receives the raw key event normally
/// (e.g. ⌘X actually cuts text instead of being swallowed as "Cancel
/// Transcription").
///
/// ⌘U (Import URL) is itself a no-op while `yt_dlp_available` is `false`
/// (Policy rule 6) — consistent with the toolbar button/menu item it
/// otherwise mirrors, both of which are disabled in that case
/// (`show_toolbar`/`show_menu_bar`).
fn handle_global_shortcuts(
    ui: &mut egui::Ui,
    actions: &mut Vec<TableAction>,
    shortcuts: &ShortcutMap,
    state: &mut ActionState,
    yt_dlp_available: bool,
) {
    if state.modal_open() {
        return;
    }
    ui.input_mut(|input| {
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::OpenRecordWindow)) {
            state.record_requested = true;
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::StopTranscription)) {
            actions.push(TableAction::CancelSelected);
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::ClearHistory)) {
            actions.push(TableAction::ClearSelected);
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::OpenImportWindow)) {
            state.pending_import = PendingImportRequest::File;
        }
        if input.consume_shortcut(&shortcuts.get(ShortcutAction::OpenImportUrlWindow))
            && yt_dlp_available
        {
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
///
/// "Import URL…" is disabled with a tooltip when `yt_dlp_available` is
/// `false` (Policy rule 6: a missing binary degrades the feature
/// gracefully, never crashes).
fn show_menu_bar(
    ui: &mut egui::Ui,
    shortcuts: &ShortcutMap,
    state: &mut ActionState,
    yt_dlp_available: bool,
) {
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
            let import_url_response = ui.add_enabled(
                yt_dlp_available,
                egui::Button::new(tr("menu-file-import-url")).shortcut_text(hint),
            );
            if !yt_dlp_available {
                import_url_response.on_disabled_hover_text(tr("url-import-unavailable-tooltip"));
            } else if import_url_response.clicked() {
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

/// The toolbar (design spec §3.3): Record (F-004/F-060, unconditionally
/// enabled — Buzz parity, `main_window.py`'s own Record toolbar button is
/// never gated) · New File Transcription · New URL Transcription · Open
/// Transcript (enabled when the selection is non-empty and every selected row
/// is completed/skipped — F-004, emits [`TableAction::OpenSelected`]) ·
/// Cancel Transcription · Clear History · Update Available chip (F-006,
/// hidden while `state.update_notice.info` is `None`).
///
/// "New URL Transcription" is disabled with a tooltip when
/// `yt_dlp_available` is `false` (Policy rule 6).
///
/// Returns whether the folder-watch pill's pause/resume button was clicked
/// this frame (F-030, design spec §3.3) — `show`'s own caller (`app.rs`)
/// owns the actual [`crate::folder_watch::FolderWatchService`], so the click
/// is only ever reported up, never acted on here.
fn show_toolbar(
    ui: &mut egui::Ui,
    vm: &TasksVm,
    actions: &mut Vec<TableAction>,
    state: &mut ActionState,
    yt_dlp_available: bool,
    folder_watch_pill: Option<&FolderWatchPillState>,
) -> bool {
    let enablement = toolbar_enablement(&vm.selected_statuses());
    let mut pause_toggled = false;
    ui.horizontal(|ui| {
        if ui.button(tr("toolbar-record")).clicked() {
            state.record_requested = true;
        }
        ui.separator();
        if ui.button(tr("toolbar-new-file")).clicked() {
            state.pending_import = PendingImportRequest::File;
        }
        let new_url_response =
            ui.add_enabled(yt_dlp_available, egui::Button::new(tr("toolbar-new-url")));
        if !yt_dlp_available {
            new_url_response.on_disabled_hover_text(tr("url-import-unavailable-tooltip"));
        } else if new_url_response.clicked() {
            state.pending_import = PendingImportRequest::Url;
        }
        let open_response = ui.add_enabled(
            enablement.open,
            egui::Button::new(tr("toolbar-open-transcript")),
        );
        if enablement.open {
            if open_response.clicked() {
                actions.push(TableAction::OpenSelected);
            }
        } else {
            open_response.on_disabled_hover_text(tr("toolbar-open-transcript-tooltip"));
        }
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
        update_notice::chip_ui(ui, &mut state.update_notice);
        if let Some(pill) = folder_watch_pill {
            ui.separator();
            pause_toggled = show_folder_watch_pill(ui, pill);
        }
    });
    pause_toggled
}

/// The folder-watch pill itself (design spec §3.3): the label text
/// ([`folder_watch_pill_label`]), dimmed while paused, plus a small icon-only
/// toggle button (⏸/▶ — the same raw-glyph-button precedent as
/// [`show_banner`]'s own dismiss "×") with a tooltip explaining what it does.
/// Returns whether that button was clicked this frame.
fn show_folder_watch_pill(ui: &mut egui::Ui, pill: &FolderWatchPillState) -> bool {
    let label = folder_watch_pill_label(pill);
    if pill.paused {
        ui.weak(label);
    } else {
        ui.label(label);
    }
    let (icon, tooltip_key) = if pill.paused {
        ("\u{25b6}", "folderwatch-pill-resume-tip")
    } else {
        ("\u{23f8}", "folderwatch-pill-pause-tip")
    };
    ui.button(icon).on_hover_text(tr(tooltip_key)).clicked()
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
    use humm_core::domain::TranscriptionId;

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

    #[test]
    fn all_completed_or_skipped_enables_open() {
        let enablement = toolbar_enablement(&[Status::Completed, Status::Skipped]);
        assert!(enablement.open);
    }

    #[test]
    fn open_disabled_when_any_selected_row_is_not_openable() {
        // Buzz `should_enable_open_transcript_action`: all-or-nothing over the
        // selection — one non-completed/skipped row disables Open.
        assert!(!toolbar_enablement(&[Status::Completed, Status::Queued]).open);
        assert!(!toolbar_enablement(&[Status::Failed]).open);
        assert!(!toolbar_enablement(&[]).open);
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
        // This test never calls `set_locale` itself, but it does assert on
        // translated text (`message()` reads through `crate::i18n::tr`, and
        // the expected value re-evaluates `tr` independently), so it can
        // still observe another concurrently running test's locale switch
        // mid-flight without this guard -- `lock_locale_for_test`'s own doc
        // comment now says so explicitly.
        let _guard = crate::i18n::lock_locale_for_test();
        let mut banner = StoreHealthBanner::default();
        banner.on_store_write_failed();
        assert!(banner.visible());
        assert_eq!(banner.message(), Some(tr("banner-store-health")));
    }

    #[test]
    fn banner_appends_counter_on_subsequent_events() {
        // Same read-only locale hazard as
        // `banner_shows_base_message_on_first_event`.
        let _guard = crate::i18n::lock_locale_for_test();
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
        // Same read-only locale hazard as
        // `banner_shows_base_message_on_first_event`.
        let _guard = crate::i18n::lock_locale_for_test();
        let mut banner = StoreHealthBanner::default();
        banner.on_store_write_failed();
        banner.on_store_write_failed();
        banner.dismiss();

        banner.on_store_write_failed();

        assert!(banner.visible());
        // Fresh reappearance: exactly the base message, no "(N)" suffix.
        assert_eq!(banner.message(), Some(tr("banner-store-health")));
    }

    // -- folder_watch_pill_label ---------------------------------------------

    #[test]
    fn pill_label_watching_has_no_paused_suffix() {
        // Same read-only locale hazard as
        // `banner_shows_base_message_on_first_event`.
        let _guard = crate::i18n::lock_locale_for_test();
        let state = FolderWatchPillState {
            folder_name: "my-clips".to_string(),
            paused: false,
        };
        assert_eq!(
            folder_watch_pill_label(&state),
            tr_args("folderwatch-pill-watching", &[("name", "my-clips")])
        );
    }

    #[test]
    fn pill_label_paused_appends_the_paused_suffix() {
        // Same read-only locale hazard as
        // `banner_shows_base_message_on_first_event`.
        let _guard = crate::i18n::lock_locale_for_test();
        let state = FolderWatchPillState {
            folder_name: "my-clips".to_string(),
            paused: true,
        };
        let label = folder_watch_pill_label(&state);
        assert!(label.contains(&tr_args(
            "folderwatch-pill-watching",
            &[("name", "my-clips")]
        )));
        assert!(label.contains(&tr("folderwatch-pill-paused")));
    }

    // -- ActionState::modal_open --------------------------------------------

    #[test]
    fn modal_open_is_false_with_nothing_open() {
        assert!(!ActionState::default().modal_open());
    }

    #[test]
    fn modal_open_is_true_while_clear_confirm_is_open() {
        let state = ActionState {
            clear_confirm_open: true,
            ..ActionState::default()
        };
        assert!(state.modal_open());
    }

    #[test]
    fn modal_open_is_true_while_the_rename_dialog_is_open() {
        let state = ActionState {
            dialog: Dialog::Rename {
                id: TranscriptionId::new(),
                text: "clip.wav".to_string(),
            },
            ..ActionState::default()
        };
        assert!(state.modal_open());
    }

    #[test]
    fn modal_open_is_true_while_the_notes_dialog_is_open() {
        let state = ActionState {
            dialog: Dialog::Notes {
                id: TranscriptionId::new(),
                text: String::new(),
            },
            ..ActionState::default()
        };
        assert!(state.modal_open());
    }

    #[test]
    fn modal_open_is_true_while_a_toast_error_detail_is_open() {
        let mut state = ActionState::default();
        state.toasts.detail = Some(crate::views::toasts::ErrorDetail {
            task_id: TranscriptionId::new(),
            full_message: "boom".to_string(),
        });
        assert!(state.modal_open());
    }
}
