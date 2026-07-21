//! `HummApp`: the `eframe::App` implementation — owns the runtime
//! [`Services`], the shared event-pump buffer, the task table's
//! view-model/layout, and (F-004/F-026, this task) the toolbar/menu/dialog/
//! toast/banner state, rendering the main window every frame (design spec
//! §1/§4).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use humm_core::domain::{Status, TranscriptionId};
use humm_core::queue::{TaskEvent, TaskEventKind};
use humm_core::settings::{FileTranscriberSettings, GuiSettings};

use crate::events::{self, SharedEventQueue};
use crate::i18n::tr;
use crate::import::{FileDialogs, ImportRequest, RfdFileDialogs, scan_folder};
use crate::services::{self, Services, UpdateNotice};
use crate::shortcuts::ShortcutMap;
use crate::viewmodel::{TableAction, TasksVm};
use crate::views;
use crate::views::about::{self, AboutCheckResult, AboutState};
use crate::views::main_window::{ActionState, PendingImportRequest, toolbar_enablement};
use crate::views::options_sheet::{OptionsSheet, SheetOutcome, SheetState};
use crate::views::preferences::{self, PreferencesState};
use crate::views::rename_notes::{self, DialogOutcome};
use crate::views::task_table::TableLayout;
use crate::views::toasts::{ToastAction, ToastKind};
use crate::views::update_notice::UpdateNoticeState;
use crate::views::url_dialog::UrlDialog;

/// The version string every version-facing view (the update dialog's
/// "Current version: …", the About window's "Version …") shows — the crate's
/// own `Cargo.toml` version at build time, the same value `services.rs`
/// feeds [`humm_core::update::check_for_update`] as `current_version`.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The eframe application.
pub struct HummApp {
    services: Services,
    /// Shared buffer the event-pump thread feeds. `None` until the pump is
    /// spawned: eagerly in [`HummApp::new`] (a real `egui::Context` is
    /// available immediately from [`eframe::CreationContext`]); lazily, on
    /// the first `eframe::App::ui` call, in [`HummApp::new_for_test`] (which
    /// has no `CreationContext` to draw one from — see that constructor's
    /// doc comment).
    event_queue: Option<SharedEventQueue>,
    /// The task table's view-model (F-003): loaded once at construction
    /// (`TasksVm::load`), then kept current by folding in each drained
    /// [`humm_core::queue::TaskEvent`] (`TasksVm::apply`) rather than ever
    /// re-querying the whole table.
    tasks: TasksVm,
    /// The task table's persisted column layout (visible/order); sort state
    /// itself lives on `tasks` (`TasksVm::sort`).
    table_layout: TableLayout,
    /// The `[gui]` settings snapshot last written to disk — compared against
    /// the current `table_layout`/`tasks.sort()` snapshot every frame so a
    /// change is persisted exactly once, not re-saved on every unchanged
    /// frame.
    last_saved_gui_settings: GuiSettings,
    /// `TableAction`s the task table/toolbar/shortcuts have emitted
    /// (retry/rename/notes/clear/open/cancel) this frame — drained every
    /// frame by [`Self::drain_table_actions`], *after* `main_window::show`
    /// returns (so actions pushed by this same frame's clicks are the ones
    /// executed, not last frame's).
    pending_table_actions: Vec<TableAction>,
    /// The resolved keyboard-shortcut bindings (F-005 defaults; `[shortcuts]`
    /// settings overrides aren't wired yet — that's the shortcut-editor UI,
    /// M7 — so this is always `ShortcutMap::from_settings(&HashMap::new())`,
    /// i.e. Buzz's defaults unmodified).
    shortcuts: ShortcutMap,
    /// Toolbar/menu/dialog/toast/banner state (F-004/F-026): see
    /// [`ActionState`]'s own field docs.
    action_state: ActionState,
    /// Seam over the (blocking, native) file/folder pickers (F-020/F-022) —
    /// always [`RfdFileDialogs`] in both real and kittest construction;
    /// nothing in this milestone's kittest coverage drives it (the sheet
    /// itself is opened directly via [`HummApp::open_options_sheet_for_test`]
    /// instead), but the seam exists so a future test can inject a fake
    /// without touching call sites.
    dialogs: Box<dyn FileDialogs>,
    /// Files passed as argv (Finder "open with" parity, design spec
    /// deviation #12) — opened as the options sheet's request on the first
    /// `eframe::App::ui` call, then cleared so it never re-triggers. Empty
    /// (the common case) is a permanent no-op.
    pending_argv_import: Vec<PathBuf>,
    /// Whether [`services::fire_launch_telemetry`] has already run this
    /// launch — set on the very first `eframe::App::ui` call so the one
    /// `app_launched` event fires exactly once (F-007, ADR-009), never
    /// again after.
    telemetry_fired: bool,
    /// Whether the About window (a real OS viewport,
    /// [`Self::show_about_viewport`]) is currently open.
    about_open: bool,
    /// The About window's own state (manual-check in-flight flag + last
    /// result) — persists across the window closing/reopening within one
    /// launch, same as Buzz's dialog would if kept alive.
    about_state: AboutState,
    /// Whether the Preferences window (a real OS viewport,
    /// [`Self::show_preferences_viewport`]) is currently open.
    preferences_open: bool,
    /// The Preferences window's state, rebuilt fresh (Models tab re-scanned
    /// from disk) each time it's opened; `None` while closed.
    preferences_state: Option<PreferencesState>,
}

impl HummApp {
    /// Builds the app for real use (`main.rs`), spawning the event pump
    /// immediately from the window's own [`egui::Context`]
    /// (`cc.egui_ctx`). `argv_import` is `main.rs`'s parsed command-line
    /// file paths (Finder "open with" parity) — opened as the options
    /// sheet's request on the first frame; an empty `Vec` (the common case)
    /// is a no-op.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        services: Services,
        argv_import: Vec<PathBuf>,
    ) -> Self {
        let event_queue = events::spawn_event_pump(services.events.clone(), cc.egui_ctx.clone());
        Self::build(services, Some(event_queue), argv_import)
    }

    /// Builds the app directly from injected [`Services`], for kittest
    /// harnesses. Every later kittest task constructs `HummApp` this way.
    ///
    /// Unlike [`HummApp::new`], no [`eframe::CreationContext`] is available
    /// at this call site (the brief's constructor signature takes only
    /// `services`), so the event pump is instead spawned lazily on the first
    /// `eframe::App::ui` call, using the `egui::Context` that call
    /// receives — for an `egui_kittest` harness, that is the *same*
    /// `Context` its own `CreationContext` carries, so the net effect is
    /// identical; it just can't be captured before the first frame runs.
    ///
    /// Never carries a pending argv import (kittest harnesses have no real
    /// argv to speak of) — a test that wants to exercise the argv-import
    /// path opens the sheet directly via
    /// [`HummApp::open_options_sheet_for_test`], which is exactly what the
    /// real argv path itself resolves to on its first frame.
    pub fn new_for_test(services: Services) -> Self {
        Self::build(services, None, Vec::new())
    }

    /// Shared constructor body: loads the task table's view-model from
    /// `services.store` and restores its persisted layout/sort from
    /// `services.settings.gui` before the first frame ever renders.
    fn build(
        services: Services,
        event_queue: Option<SharedEventQueue>,
        pending_argv_import: Vec<PathBuf>,
    ) -> Self {
        let (gui_settings, stored_available_version) = {
            let settings = services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                settings.gui.clone(),
                settings.updates.available_version.clone(),
            )
        };

        let mut tasks = TasksVm::load(&services.store);
        let (sort_col, sort_descending) = TableLayout::sort_from_gui_settings(&gui_settings);
        tasks.set_sort_exact(sort_col, sort_descending);

        let table_layout = TableLayout::from_gui_settings(&gui_settings);
        let last_saved_gui_settings = table_layout.to_gui_settings(tasks.sort());

        // No `[shortcuts]` settings section exists yet (the override-editor
        // UI is M7); an empty overrides map resolves every action to its
        // Buzz default (`ShortcutMap::from_settings`'s own contract).
        let shortcuts = ShortcutMap::from_settings(&HashMap::new());

        // Chip persistence across sessions (binding decision, Buzz-equivalent):
        // a stale-but-still-newer `available_version` from a previous
        // session's check shows the chip immediately, before this session's
        // own check (if any) has even run. Only the version string survives
        // a restart — release notes/download URLs aren't persisted, so the
        // dialog shows an empty notes section until a fresh check backfills
        // them.
        let update_notice = if services::chip_visible_on_startup(
            CURRENT_VERSION,
            stored_available_version.as_deref(),
        ) {
            UpdateNoticeState {
                info: Some(humm_core::update::UpdateInfo {
                    version: stored_available_version.unwrap_or_default(),
                    release_notes: String::new(),
                    download_urls: Vec::new(),
                }),
                dialog_open: false,
            }
        } else {
            UpdateNoticeState::default()
        };

        HummApp {
            services,
            event_queue,
            tasks,
            table_layout,
            last_saved_gui_settings,
            pending_table_actions: Vec::new(),
            shortcuts,
            action_state: ActionState {
                update_notice,
                ..ActionState::default()
            },
            dialogs: Box::new(RfdFileDialogs),
            pending_argv_import,
            telemetry_fired: false,
            about_open: false,
            about_state: AboutState::default(),
            preferences_open: false,
            preferences_state: None,
        }
    }

    /// Writes `table_layout`/`tasks.sort()` to `services.settings.gui` and
    /// persists it (`Settings::save`) if it differs from what was last
    /// saved. A save failure is logged, not fatal — the in-memory layout the
    /// user sees is unaffected either way, only next launch's restore would
    /// miss this particular change.
    fn persist_layout_if_changed(&mut self) {
        let current = self.table_layout.to_gui_settings(self.tasks.sort());
        if current == self.last_saved_gui_settings {
            return;
        }
        {
            let mut settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settings.gui = current.clone();
            if let Err(err) = settings.save(&self.services.settings_path) {
                log::warn!("could not persist task table layout: {err}");
            }
        }
        self.last_saved_gui_settings = current;
    }

    /// Test-only seam: injects `event` directly into the event-pump buffer,
    /// as if the real queue's runner thread had emitted it. Lets a kittest
    /// harness exercise event-driven UI reactions (the `StoreWriteFailed`
    /// banner, in particular) deterministically, without racing the real
    /// queue/runner threads for a precise failure-injection point — the
    /// same "plain `pub fn`, not `#[cfg(test)]`" pattern as
    /// [`Self::new_for_test`] (an integration test in `tests/` links the
    /// library's normal, non-`cfg(test)` build, so a `#[cfg(test)]` method
    /// wouldn't even be visible to it). A no-op before the event pump
    /// exists (i.e. before the first `eframe::App::ui` call) — harmless,
    /// but there is nothing to inject into yet.
    pub fn inject_event_for_test(&mut self, event: TaskEvent) {
        if let Some(queue) = &self.event_queue {
            queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push_back(event);
        }
    }

    /// Test-only seam: opens the options sheet directly with `request`, as
    /// if the corresponding dialog/drag-drop/argv flow had already produced
    /// it. Lets a kittest harness exercise the sheet (and Run→enqueue→
    /// sticky-prefs flow) without ever driving a real, blocking rfd dialog —
    /// same "plain `pub fn`, not `#[cfg(test)]`" pattern as
    /// [`Self::inject_event_for_test`].
    pub fn open_options_sheet_for_test(&mut self, request: ImportRequest) {
        self.open_options_sheet(request);
    }

    /// Whether `settings.models.custom_model_url` is set — gates the
    /// options sheet's model-size combo's "Custom" entry (F-023).
    fn custom_model_available(&self) -> bool {
        self.custom_model_url().is_some()
    }

    /// The current `settings.models.custom_model_url` value — the Models
    /// tab's own custom-URL row gate (F-044) needs the value itself, not
    /// just [`Self::custom_model_available`]'s boolean.
    fn custom_model_url(&self) -> Option<String> {
        let settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.models.custom_model_url.clone()
    }

    /// Opens the options sheet for `request`, prefilled from the current
    /// `settings.file_transcriber`/`settings.models` snapshot.
    fn open_options_sheet(&mut self, request: ImportRequest) {
        let prefill: FileTranscriberSettings = {
            let settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settings.file_transcriber.clone()
        };
        let custom_model_available = self.custom_model_available();
        self.action_state.options_sheet =
            Some(OptionsSheet::new(request, &prefill, custom_model_available));
    }

    /// Applies the options sheet's close outcome (`views::main_window::show`
    /// already rendered it this frame): persists the sticky prefs
    /// unconditionally (Buzz's `closeEvent` parity — Run *and* Cancel/
    /// Escape/backdrop-click all save), then, for `Run`, enqueues every
    /// spec with a fresh id.
    fn apply_sheet_outcome(&mut self, outcome: SheetOutcome) {
        match outcome {
            SheetOutcome::Run { specs, sticky } => {
                self.persist_sticky_prefs(&sticky);
                for (id, spec, source) in specs {
                    self.services.queue.enqueue(id, spec, source);
                }
            }
            SheetOutcome::Cancelled { sticky } => self.persist_sticky_prefs(&sticky),
        }
    }

    /// Writes `sticky` into `settings.file_transcriber` and persists it. A
    /// save failure is logged, not fatal (same non-fatal-settings-write
    /// contract as [`Self::persist_layout_if_changed`]/
    /// [`Self::persist_last_import_folder`]).
    fn persist_sticky_prefs(&mut self, sticky: &SheetState) {
        let mut settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sticky.write_to(&mut settings.file_transcriber);
        if let Err(err) = settings.save(&self.services.settings_path) {
            log::warn!("could not persist file-transcriber options: {err}");
        }
    }

    /// Persists `folder` as `settings.file_transcriber.last_import_folder`
    /// (Buzz parity: persisted at dialog time, independent of the sheet's
    /// own sticky write-back). A no-op if `folder` is `None` (the file
    /// dialog's pick had no parent directory — practically unreachable, but
    /// harmless either way).
    fn persist_last_import_folder(&mut self, folder: Option<&Path>) {
        let Some(folder) = folder else {
            return;
        };
        let mut settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.file_transcriber.last_import_folder = Some(folder.to_path_buf());
        if let Err(err) = settings.save(&self.services.settings_path) {
            log::warn!("could not persist the last import folder: {err}");
        }
    }

    /// The last-used import folder, if any, to seed the file/folder dialog's
    /// starting directory.
    fn last_import_folder(&self) -> Option<PathBuf> {
        let settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.file_transcriber.last_import_folder.clone()
    }

    /// Drains `action_state.pending_import` (set by the toolbar/File-menu/
    /// shortcuts this frame's `main_window::show` call) and dispatches the
    /// corresponding import flow. Always resets it to `None` first — a
    /// canceled dialog must not re-trigger itself on the very next frame.
    /// Guarded by `!modal_open()` (symmetry with
    /// [`Self::drain_dropped_files`]'s own guard): a pending import queued
    /// the same frame some other modal is already up (e.g. Clear-confirm)
    /// must not silently clobber it with a freshly-opened dialog/sheet.
    fn drain_pending_import(&mut self) {
        let request = std::mem::take(&mut self.action_state.pending_import);
        if self.action_state.modal_open() {
            return;
        }
        match request {
            PendingImportRequest::None => {}
            PendingImportRequest::File => self.handle_file_import_request(),
            PendingImportRequest::Url => {
                self.action_state.url_dialog = UrlDialog::open();
            }
            PendingImportRequest::Folder => self.handle_folder_import_request(),
        }
    }

    /// "New File Transcription" / File → "Import File…" / ⌘O
    /// (`main_window.py:268-281`): opens the (blocking) file dialog seeded
    /// with the last import folder, persists the parent of the first pick,
    /// and opens the options sheet with every picked file. A cancel (`None`)
    /// or an empty pick is a silent no-op.
    fn handle_file_import_request(&mut self) {
        let start_dir = self.last_import_folder();
        let Some(paths) = self.dialogs.pick_files(start_dir.as_deref()) else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        self.persist_last_import_folder(paths[0].parent());
        self.open_options_sheet(ImportRequest::Files(paths));
    }

    /// File → "Import Folder…" (`main_window.py:288-304`): opens the
    /// (blocking) folder dialog seeded with the last import folder,
    /// persists the chosen folder itself, recursively scans it
    /// (`import::scan_folder`), and opens the options sheet with whatever
    /// matched. Zero matches is a silent no-op (Buzz parity) — same for a
    /// cancel (`None`).
    fn handle_folder_import_request(&mut self) {
        let start_dir = self.last_import_folder();
        let Some(folder) = self.dialogs.pick_folder(start_dir.as_deref()) else {
            return;
        };
        self.persist_last_import_folder(Some(&folder));
        let files = scan_folder(&folder);
        if files.is_empty() {
            return;
        }
        self.open_options_sheet(ImportRequest::Files(files));
    }

    /// Drag-and-drop (F-021, `main_window.py:196-205`): reads
    /// `egui::RawInput::dropped_files` (no extension filtering — Buzz
    /// parity) and, if any files were dropped this frame, opens the options
    /// sheet with them. Guarded by `!modal_open()` so a drop landing while
    /// some other modal (e.g. Clear-confirm) is already up doesn't silently
    /// clobber it with a freshly-opened sheet — a humm-only defensive
    /// addition (Buzz's own top-level-window drop target has no equivalent
    /// modal to protect).
    fn drain_dropped_files(&mut self, ctx: &egui::Context) {
        if self.action_state.modal_open() {
            return;
        }
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        self.open_options_sheet(ImportRequest::Files(dropped));
    }

    /// Consumes `pending_argv_import` (Finder "open with" parity) on
    /// whichever frame first calls this — always the very first
    /// `eframe::App::ui` call, since it's cleared unconditionally the first
    /// time regardless of whether it was empty. An empty `Vec` (the common
    /// case: no argv files) never opens a sheet.
    fn drain_pending_argv_import(&mut self) {
        let paths = std::mem::take(&mut self.pending_argv_import);
        if paths.is_empty() {
            return;
        }
        self.open_options_sheet(ImportRequest::Files(paths));
    }

    /// Reacts to one drained event with a toast/banner side effect, keyed on
    /// `event.kind`. `TasksVm::apply` (already called for every event, just
    /// before this) owns row state; this owns only the toast stack and the
    /// store-health banner, neither of which lives on any row.
    ///
    /// Chosen over teaching `TasksVm::apply` to return a toast-driving
    /// value (the brief's other offered seam): every payload this method
    /// needs (`task_id`, `written`/`message`) is already fully available on
    /// `event` right here, in the same loop that calls `apply` — threading
    /// the same data back out through `apply`'s return value would only
    /// duplicate it for no benefit, and keeps `TasksVm`'s own tests free of
    /// toast/banner concerns that have nothing to do with row state.
    fn react_to_event(&mut self, event: &TaskEvent, now: Instant) {
        match &event.kind {
            TaskEventKind::Completed { written } => {
                let message = if written.is_empty() {
                    tr("toast-completed")
                } else {
                    format!(
                        "{} \u{2014} {} {}",
                        tr("toast-completed"),
                        written.len(),
                        tr("toast-completed-suffix")
                    )
                };
                self.action_state
                    .toasts
                    .push(ToastKind::Success, message, now);
            }
            TaskEventKind::Failed { message } => {
                let first_line = message.lines().next().unwrap_or(message).to_string();
                self.action_state.toasts.push(
                    ToastKind::TaskFailed {
                        task_id: event.task_id,
                        full_message: message.clone(),
                    },
                    first_line,
                    now,
                );
            }
            TaskEventKind::Canceled => {
                self.action_state
                    .toasts
                    .push(ToastKind::Info, tr("toast-canceled"), now);
            }
            TaskEventKind::StoreWriteFailed { .. } => {
                self.action_state.banner.on_store_write_failed();
            }
            _ => {}
        }
    }

    /// Applies a confirmed Rename/Notes dialog outcome: writes the store,
    /// then refreshes the row so the table reflects it immediately. A store
    /// write failure is logged and surfaced as an error toast (never a
    /// panic, never silently dropped) — the dialog has already closed by
    /// this point either way.
    fn apply_dialog_outcome(&mut self, outcome: DialogOutcome, now: Instant) {
        let store = Arc::clone(&self.services.store);
        match outcome {
            DialogOutcome::Rename { id, name } => match store.set_task_name(&id, &name) {
                Ok(()) => self.tasks.refresh_row(id, &store),
                Err(err) => {
                    log::warn!("set_task_name failed for {id}: {err}");
                    self.push_error_toast(format!("{}: {err}", tr("rename-error")), now);
                }
            },
            DialogOutcome::Notes { id, text } => match store.set_task_notes(&id, &text) {
                Ok(()) => self.tasks.refresh_row(id, &store),
                Err(err) => {
                    log::warn!("set_task_notes failed for {id}: {err}");
                    self.push_error_toast(format!("{}: {err}", tr("notes-error")), now);
                }
            },
        }
    }

    /// Applies the Clear-History confirm modal's Ok click: deletes every
    /// currently-selected row from the store, then drops them from the
    /// view-model (which also clears their selection). A store failure is
    /// logged and surfaced as an error toast; the view-model is left
    /// unchanged in that case (the rows are still there — because they're
    /// still in the store).
    ///
    /// Re-checks the terminal-status gate here too (the same
    /// `toolbar_enablement(..).clear` rule `handle_clear_selected` already
    /// checked before ever opening the confirm modal) — defense in depth:
    /// the modal is open for as long as the user takes to click Ok/Cancel,
    /// and task events keep flowing into `TasksVm` regardless of whether a
    /// modal is up, so a selected row could in principle transition to a
    /// non-terminal status in that window. Failing the gate silently
    /// aborts the delete (nothing selected is safe to delete anymore) —
    /// no toast, matching `handle_clear_selected`'s own silent-ignore
    /// contract for a gate that fails.
    fn apply_clear_confirmed(&mut self, now: Instant) {
        if !toolbar_enablement(&self.tasks.selected_statuses()).clear {
            return;
        }
        let ids: Vec<TranscriptionId> = self.tasks.selection().iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        match self.services.store.delete_tasks(&ids) {
            Ok(_) => self.tasks.remove(&ids),
            Err(err) => {
                log::warn!("delete_tasks failed: {err}");
                self.push_error_toast(format!("{}: {err}", tr("clear-error")), now);
            }
        }
    }

    /// Applies a toast/detail-popover action.
    fn apply_toast_action(&mut self, action: ToastAction, now: Instant) {
        match action {
            ToastAction::Retry(id) => self.handle_retry(id, now),
            ToastAction::OpenLogFolder => {
                if let Err(err) = open::that(humm_core::paths::log_dir()) {
                    log::warn!("could not open the log folder: {err}");
                }
            }
        }
    }

    /// Drains this frame's `pending_table_actions` (Task 7's accumulator —
    /// the table's context menu/keyboard shortcuts and the toolbar/global
    /// shortcuts all push onto the same `Vec`), executing each.
    fn drain_table_actions(&mut self, now: Instant) {
        let actions = std::mem::take(&mut self.pending_table_actions);
        for action in actions {
            match action {
                TableAction::ClearSelected => self.handle_clear_selected(),
                TableAction::OpenSelected => {
                    // The transcript viewer is M4; nothing to open yet
                    // (toolbar's Open Transcript button is disabled this
                    // milestone, but Return's shortcut still reaches here).
                }
                TableAction::Retry(id) => self.handle_retry(id, now),
                TableAction::Rename(id) => self.open_rename_dialog(id),
                TableAction::Notes(id) => self.open_notes_dialog(id),
                TableAction::CancelSelected => self.handle_cancel_selected(),
            }
        }
    }

    /// Toolbar "Cancel Transcription" / ⌘X: `queue.cancel(id)` for every
    /// selected row whose status is queued/in-progress — filtered here
    /// (rather than gated all-or-nothing) so a selection the toolbar would
    /// have disabled entirely still cancels whichever of its rows *are*
    /// cancelable if reached via a path that doesn't re-check the toolbar's
    /// own enablement (defense in depth). Never writes the store directly:
    /// the queue owns status, and a canceled row flips via the ordinary
    /// `Canceled` event.
    fn handle_cancel_selected(&mut self) {
        let ids: Vec<TranscriptionId> = self
            .tasks
            .sorted_rows()
            .iter()
            .filter(|row| self.tasks.selection().contains(&row.id))
            .filter(|row| matches!(row.status, Status::InProgress | Status::Queued))
            .map(|row| row.id)
            .collect();
        for id in ids {
            self.services.queue.cancel(id);
        }
    }

    /// Toolbar "Clear History" / ⌘S / Delete key: gates to a non-empty,
    /// all-terminal-status selection (the same rule
    /// [`toolbar_enablement`] uses to gray out the toolbar button) and, if
    /// it passes, opens the confirm modal. A selection that fails the gate
    /// is silently ignored on every path, including the Delete key —
    /// deviation §6.2: Buzz's own Delete-key handler bypasses this gate
    /// (a defect, not ported).
    fn handle_clear_selected(&mut self) {
        if toolbar_enablement(&self.tasks.selected_statuses()).clear {
            self.action_state.clear_confirm_open = true;
        }
    }

    /// Retry: reachable from the row context menu's "Restart Transcription"
    /// and a failed-transcription toast's detail-popover Retry button.
    /// Re-checks the row's current status before doing anything (defense in
    /// depth — the queue itself has no terminal-status guard, and the
    /// row's status can change between the click that offered Retry and
    /// this handler actually running).
    ///
    /// Spec source: `load_task_spec` (the full persisted options); on
    /// `Ok(None)` (an imported/foreign row with no `options_json`), falls
    /// back to `spec_from_columns`. Any error from `load_task_spec` itself,
    /// or an `Ok(None)`/error from the fallback, surfaces the toast
    /// "Could not restart: saved options are unavailable" — the row is left
    /// exactly as it was (still failed/canceled/skipped, retryable again
    /// later).
    fn handle_retry(&mut self, id: TranscriptionId, now: Instant) {
        let Some(row) = self.tasks.sorted_rows().iter().find(|row| row.id == id) else {
            return;
        };
        if !retry_allowed(row.status) {
            return;
        }

        let store = Arc::clone(&self.services.store);
        let spec = match store.load_task_spec(&id) {
            Ok(Some(spec)) => Ok(Some(spec)),
            Ok(None) => store.spec_from_columns(&id),
            Err(err) => Err(err),
        };
        match spec {
            Ok(Some(spec)) => self.services.queue.retry(id, spec),
            Ok(None) | Err(_) => {
                self.push_error_toast(tr("retry-error-unavailable"), now);
            }
        }
    }

    /// Row context menu "Rename": prefills from the row's *current* store
    /// columns (not the view-model's `display_name`, which uses a
    /// different fallback order — see [`rename_notes::rename_prefill`]'s
    /// doc comment) and opens the modal. A store-read failure or a
    /// since-deleted row silently does nothing (no dialog opens).
    fn open_rename_dialog(&mut self, id: TranscriptionId) {
        let Ok(Some(row)) = self.services.store.get_task(&id) else {
            return;
        };
        let text = rename_notes::rename_prefill(
            row.name.as_deref(),
            row.url.as_deref(),
            row.file.as_deref(),
        );
        self.action_state.dialog = rename_notes::Dialog::Rename { id, text };
    }

    /// Row context menu "Add/Edit Notes" (or a Notes-cell double-click):
    /// prefills from the view-model's current notes text and opens the
    /// modal. A since-removed row silently does nothing.
    fn open_notes_dialog(&mut self, id: TranscriptionId) {
        let Some(row) = self.tasks.sorted_rows().iter().find(|row| row.id == id) else {
            return;
        };
        let text = row.notes.clone().unwrap_or_default();
        self.action_state.dialog = rename_notes::Dialog::Notes { id, text };
    }

    /// Pushes a plain (non-`TaskFailed`, non-clickable) error toast.
    fn push_error_toast(&mut self, message: String, now: Instant) {
        self.action_state
            .toasts
            .push(ToastKind::Error, message, now);
    }

    /// Fires the one-per-launch telemetry event (F-007, ADR-009) on the
    /// first call only — every later call is a no-op. See
    /// [`services::fire_launch_telemetry`]'s doc comment for why this lives
    /// on the first UI frame rather than in `services::init`.
    fn fire_telemetry_once(&mut self) {
        if self.telemetry_fired {
            return;
        }
        self.telemetry_fired = true;
        services::fire_launch_telemetry(&self.services);
    }

    /// Drains `services.update_rx` and applies every [`UpdateNotice`] this
    /// frame: `Available` always updates the toolbar chip (whichever check
    /// — startup or the About window's manual one — found it); `UpToDate`
    /// always clears the chip (binding decision: "clear when a check says
    /// up-to-date"), regardless of source. Which UI surfaces which notice
    /// beyond that is decided by `self.about_state.checking`: while a manual
    /// check is in flight, this frame's notice *is* that check's result
    /// (`about_state.checking` is cleared here and the About window's inline
    /// label/the update dialog reflects it); otherwise it's the startup
    /// check's own (silent, log-only) result.
    ///
    /// Simplification (documented): if a startup check and a manual check
    /// somehow overlap in the same frame window, this can't tell their
    /// notices apart and may attribute one to the other. Both come from
    /// `run_update_check`'s exact same shape either way, and the startup
    /// check only ever fires once, very early in the process's life, so in
    /// practice the two paths don't actually race within a session.
    fn drain_update_notices(&mut self) {
        for notice in self.services.update_rx.try_iter().collect::<Vec<_>>() {
            let awaiting_manual = self.about_state.checking;
            match notice {
                UpdateNotice::Available(info) => {
                    self.action_state.update_notice.info = Some(info);
                    if awaiting_manual {
                        self.about_state.checking = false;
                        self.about_state.result = None;
                        self.action_state.update_notice.dialog_open = true;
                    }
                }
                UpdateNotice::UpToDate => {
                    self.action_state.update_notice.info = None;
                    if awaiting_manual {
                        self.about_state.checking = false;
                        self.about_state.result = Some(AboutCheckResult::UpToDate);
                    } else {
                        log::debug!("startup update check: already up to date");
                    }
                }
                UpdateNotice::CheckFailed(detail) => {
                    if awaiting_manual {
                        self.about_state.checking = false;
                        self.about_state.result = Some(AboutCheckResult::Failed(detail));
                    } else {
                        log::debug!("startup update check failed: {detail}");
                    }
                }
            }
        }
    }

    /// Spawns the About window's manual "Check for updates" — bypasses
    /// every startup gate (the ≤7-day cadence, the Privacy toggle, and the
    /// `HUMM_DISABLE_UPDATE_CHECK` env var: binding decision, an explicit
    /// user click), but funnels through the exact same
    /// [`services::run_update_check`] the startup path uses, onto the same
    /// [`Services::update_tx`] channel.
    fn spawn_manual_update_check(&self) {
        let settings = Arc::clone(&self.services.settings);
        let settings_path = self.services.settings_path.clone();
        let tx = self.services.update_tx.clone();
        std::thread::spawn(move || {
            services::run_update_check(
                &settings,
                &settings_path,
                &tx,
                humm_core::update::VERSION_INFO_URL,
                CURRENT_VERSION,
            );
        });
    }

    /// Opens/keeps open the About window (Help → "About humm") as a real OS
    /// viewport — an *immediate* viewport (see `views::about`'s module doc
    /// for the immediate-vs-deferred tradeoff). [`about::ui`] itself is the
    /// plain, kittest-drivable function; this wrapper is the (manually
    /// verified — no display in this sandboxed environment)
    /// viewport-plumbing part.
    fn show_about_viewport(&mut self, ctx: &egui::Context) {
        let about_state = &self.about_state;
        let mut action = about::AboutAction::default();
        let mut close_requested = false;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("humm-about-viewport"),
            egui::ViewportBuilder::default()
                .with_title(tr("about-app-name"))
                .with_inner_size([380.0, 240.0])
                .with_resizable(false),
            |ui, _class| {
                egui::CentralPanel::default().show(ui, |ui| {
                    action = about::ui(ui, CURRENT_VERSION, about_state);
                });
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    close_requested = true;
                }
            },
        );

        if action.check_clicked && !self.about_state.checking {
            self.about_state.checking = true;
            self.about_state.result = None;
            self.spawn_manual_update_check();
        }
        if action.show_logs_clicked
            && let Err(err) = open::that(humm_core::paths::log_dir())
        {
            log::warn!("could not open the log folder: {err}");
        }
        if action.close_clicked || close_requested {
            self.about_open = false;
        }
    }

    /// Opens/keeps open the Preferences window (⌘,/Help → "Preferences…")
    /// as a real OS viewport, min 650×500 (binding decision). Same
    /// immediate-viewport shape as [`Self::show_about_viewport`]; see
    /// `views::preferences`'s module doc for the tradeoff this implies.
    fn show_preferences_viewport(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.preferences_state else {
            return;
        };
        let settings = &self.services.settings;
        let settings_path = &self.services.settings_path;
        let model_root = &self.services.model_root;
        let mut close_requested = false;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("humm-preferences-viewport"),
            egui::ViewportBuilder::default()
                .with_title(tr("prefs-title"))
                .with_inner_size([650.0, 500.0])
                .with_min_inner_size([650.0, 500.0]),
            |ui, _class| {
                egui::CentralPanel::default().show(ui, |ui| {
                    preferences::ui(ui, state, model_root, settings, settings_path);
                });
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    close_requested = true;
                }
            },
        );

        if close_requested {
            self.preferences_open = false;
            self.preferences_state = None;
        }
    }
}

/// Buzz's Restart gate (`transcription_tasks_table_widget.py:696-725`):
/// only failed/canceled/skipped rows may be retried — a completed row is
/// never restartable. Pure and free of any `HummApp` state so the gate
/// itself is directly unit-testable, independent of
/// [`HummApp::handle_retry`]'s store/queue plumbing.
fn retry_allowed(status: Status) -> bool {
    matches!(status, Status::Failed | Status::Canceled | Status::Skipped)
}

impl eframe::App for HummApp {
    /// eframe 0.35's per-frame entry point: `App::ui` (not the classic
    /// `App::update`), handed the root viewport's `egui::Ui` directly rather
    /// than a bare `&egui::Context` — `egui::Ui::ctx()` gets the `Context`
    /// back when something (like the event pump) needs to own a clone of it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.event_queue.is_none() {
            self.event_queue = Some(events::spawn_event_pump(
                self.services.events.clone(),
                ui.ctx().clone(),
            ));
        }

        self.fire_telemetry_once();
        self.drain_pending_argv_import();
        self.drain_update_notices();

        let now = Instant::now();
        if let Some(event_queue) = &self.event_queue {
            for event in events::drain(event_queue) {
                self.tasks.apply(&event, &self.services.store);
                self.react_to_event(&event, now);
            }
        }

        let custom_model_available = self.custom_model_available();
        let yt_dlp_available = self.services.capabilities.yt_dlp;

        let result = views::main_window::show(
            ui,
            &mut self.tasks,
            &mut self.table_layout,
            &mut self.pending_table_actions,
            &self.shortcuts,
            &mut self.action_state,
            now,
            custom_model_available,
            yt_dlp_available,
            CURRENT_VERSION,
        );

        if let Some(outcome) = result.dialog_outcome {
            self.apply_dialog_outcome(outcome, now);
        }
        if result.clear_confirmed {
            self.apply_clear_confirmed(now);
        }
        for action in result.toast_actions {
            self.apply_toast_action(action, now);
        }
        if let Some(outcome) = result.sheet_outcome {
            self.apply_sheet_outcome(outcome);
        }
        if let Some(url) = result.url_dialog_result {
            self.open_options_sheet(ImportRequest::Url(url));
        }

        self.drain_table_actions(now);
        self.drain_pending_import();
        self.drain_dropped_files(ui.ctx());

        // Multi-window flows (Task 8's `about_requested`/`preferences_requested`
        // stubs, wired here): a click this frame opens the corresponding real
        // OS viewport; the flag itself is one-shot (reset immediately) so it
        // never re-triggers on a later frame while the window stays open.
        if self.action_state.about_requested {
            self.action_state.about_requested = false;
            self.about_open = true;
        }
        if self.action_state.preferences_requested {
            self.action_state.preferences_requested = false;
            // Only (re)builds state on an actual open — re-triggering ⌘,/the
            // menu item while the window is already open must not clobber
            // live state (an in-flight download's `Downloading` row, a
            // half-typed custom URL, an open delete-confirm modal).
            if !self.preferences_open {
                self.preferences_state = Some(PreferencesState::new(
                    &self.services.model_root,
                    self.custom_model_url().as_deref(),
                ));
            }
            self.preferences_open = true;
        }
        if self.about_open {
            self.show_about_viewport(ui.ctx());
        }
        if self.preferences_open {
            self.show_preferences_viewport(ui.ctx());
        }

        self.persist_layout_if_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_allowed_only_for_failed_canceled_skipped() {
        assert!(retry_allowed(Status::Failed));
        assert!(retry_allowed(Status::Canceled));
        assert!(retry_allowed(Status::Skipped));
        assert!(!retry_allowed(Status::Completed));
        assert!(!retry_allowed(Status::Queued));
        assert!(!retry_allowed(Status::InProgress));
    }
}
