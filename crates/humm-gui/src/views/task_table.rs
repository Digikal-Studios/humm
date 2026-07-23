//! The virtualized task table (F-003):
//! `transcription_tasks_table_widget.py` ported to an
//! `egui_extras::TableBuilder`. This module owns only *rendering* and
//! *layout* — the data lives in [`crate::viewmodel::TasksVm`]; anything the
//! view can't decide for itself (retry/rename/notes/clear/open) is pushed
//! onto the caller-supplied `actions: &mut Vec<TableAction>` rather than
//! acted on here (`app.rs` is the intended consumer, starting Task 8 — see
//! its call site's doc comment for this task's interim handling).

use std::time::Duration;

use egui::{Key, Modifiers, Sense};
use egui_extras::{Column, TableBuilder};

use humm_core::domain::{Status, TranscriptionId};
use humm_core::settings::GuiSettings;

use crate::format;
use crate::i18n::tr;
use crate::viewmodel::{ColumnId, RowVm, TableAction, TasksVm};

/// Stable `TableBuilder::id_salt` for the (single, app-wide) task table.
///
/// `egui_extras`'s own default (`"__table_state"`) would already be stable
/// across restarts here in practice (it's a fixed constant, and the table's
/// enclosing `Ui` — the one `CentralPanel` this app ever shows — gets a
/// deterministic auto-id since it's unconditionally the first and only
/// thing added to the root `Ui` every frame). Explicit anyway: a reader
/// (or a future refactor that adds a sibling panel before this one, subtly
/// changing that auto-id) shouldn't have to reason about that chain to
/// know the persisted column widths (design spec §2.6) survive a restart.
const TABLE_ID_SALT: &str = "humm-task-table";

/// Persisted column chrome for the task table: visible columns, their
/// display order, and (design spec §2.6) resizable column widths
/// (`settings.gui.task_table_visible`/`task_table_order`). Sort state lives
/// on [`TasksVm`] itself (`TasksVm::sort`), not here — see
/// [`Self::to_gui_settings`] for how the two recombine into one `[gui]`
/// settings snapshot.
///
/// Column *widths* are not tracked as a field here at all: they're owned
/// entirely by `egui_extras::TableBuilder`'s own internal state, persisted
/// through `egui::Memory`'s `data` store (which is itself serialized into
/// `gui.ron` by eframe's `persistence` feature) once `egui_extras`'s
/// `"serde"` feature is enabled (it is, workspace-wide) — see [`ui`]'s doc
/// comment for how [`TABLE_ID_SALT`] keys that persisted entry, and
/// [`Self::take_pending_width_reset`] for how "Reset Column Order" clears
/// it.
///
/// [`Self::anchor`] is the shift-click range-selection anchor: transient UI
/// state that must survive across frames (so a later shift-click can extend
/// from it) but must never be persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct TableLayout {
    order: Vec<ColumnId>,
    visible: Vec<ColumnId>,
    /// Row index (in current sort order) of the last plain click, if any —
    /// the shift-click range-selection anchor.
    anchor: Option<usize>,
    /// Set by [`Self::reset`]; consumed (cleared) by
    /// [`Self::take_pending_width_reset`] at the top of the next [`ui`]
    /// call, which then clears `egui_extras`'s own persisted column-width
    /// state for [`TABLE_ID_SALT`] before this frame's `TableBuilder` is
    /// built. This has to be one frame delayed: `TableBuilder::reset`
    /// needs a live `TableBuilder` instance, which doesn't exist yet
    /// inside the header context-menu closure that requests the reset —
    /// imperceptible to a human, and consistent with other
    /// one-frame-delayed effects immediate-mode UIs commonly have.
    pending_width_reset: bool,
}

impl Default for TableLayout {
    fn default() -> Self {
        TableLayout {
            order: ColumnId::all().to_vec(),
            visible: ColumnId::all().to_vec(),
            anchor: None,
            pending_width_reset: false,
        }
    }
}

impl TableLayout {
    /// Restores a [`TableLayout`] from persisted settings, falling back to
    /// the built-in default for any field that's absent, empty, or contains
    /// only unrecognized column ids (a stale/foreign settings file must
    /// never leave the table with zero columns). `FileName` is always kept
    /// visible regardless of what was persisted (it is never hideable).
    pub fn from_gui_settings(gui: &GuiSettings) -> Self {
        let mut layout = TableLayout::default();

        if let Some(persisted) = &gui.task_table_order {
            let mut order: Vec<ColumnId> = persisted
                .iter()
                .filter_map(|id| ColumnId::from_id(id))
                .collect();
            if !order.is_empty() {
                // A column added to `ColumnId::all()` after this settings
                // file was written wouldn't appear in `persisted` — append
                // it at the end rather than silently dropping it.
                for col in ColumnId::all() {
                    if !order.contains(&col) {
                        order.push(col);
                    }
                }
                layout.order = order;
            }
        }

        if let Some(persisted) = &gui.task_table_visible {
            let mut visible: Vec<ColumnId> = persisted
                .iter()
                .filter_map(|id| ColumnId::from_id(id))
                .collect();
            // Every entry unrecognized (a foreign/corrupt settings file) —
            // fall back to the built-in default rather than an all-hidden
            // table.
            if !visible.is_empty() {
                if !visible.contains(&ColumnId::FileName) {
                    visible.push(ColumnId::FileName);
                }
                layout.visible = visible;
            }
        }

        layout
    }

    /// Builds the `[gui]` settings snapshot for this layout plus the given
    /// (externally-owned, on `TasksVm`) sort state — the shape
    /// [`Self::from_gui_settings`] reads back.
    pub fn to_gui_settings(&self, sort: (ColumnId, bool)) -> GuiSettings {
        GuiSettings {
            task_table_visible: Some(
                self.visible
                    .iter()
                    .map(|col| col.id().to_string())
                    .collect(),
            ),
            task_table_order: Some(self.order.iter().map(|col| col.id().to_string()).collect()),
            task_table_sort_column: Some(sort.0.id().to_string()),
            task_table_sort_descending: Some(sort.1),
        }
    }

    /// The persisted sort restored from `gui`, or the built-in default
    /// `(DateAdded, descending)` if absent/unrecognized.
    pub fn sort_from_gui_settings(gui: &GuiSettings) -> (ColumnId, bool) {
        let col = gui
            .task_table_sort_column
            .as_deref()
            .and_then(ColumnId::from_id)
            .unwrap_or(ColumnId::DateAdded);
        let descending = gui.task_table_sort_descending.unwrap_or(true);
        (col, descending)
    }

    /// Whether `col` is currently visible. `FileName` is always visible.
    pub fn is_visible(&self, col: ColumnId) -> bool {
        col == ColumnId::FileName || self.visible.contains(&col)
    }

    /// Toggles `col`'s visibility. A no-op for `FileName` (never hideable).
    pub fn toggle_visible(&mut self, col: ColumnId) {
        if !col.hideable() {
            return;
        }
        match self.visible.iter().position(|c| *c == col) {
            Some(pos) => {
                self.visible.remove(pos);
            }
            None => self.visible.push(col),
        }
    }

    /// "Reset Column Order": restores the default order and un-hides every
    /// column, and requests that `egui_extras`'s own persisted column-width
    /// state be cleared too (Buzz's `reset_column_order` resets widths
    /// alongside order/visibility — `transcription_tasks_table_widget.py:
    /// 454-500`; the accompanying sort reset is the caller's job — see
    /// [`TasksVm::set_sort_exact`]). The width part of the reset takes
    /// effect one frame later — see [`Self::pending_width_reset`]'s doc.
    pub fn reset(&mut self) {
        self.order = ColumnId::all().to_vec();
        self.visible = ColumnId::all().to_vec();
        self.pending_width_reset = true;
    }

    /// Consumes (clears) the pending width-reset flag [`Self::reset`] set,
    /// returning whether one was pending. `ui` calls this once per frame,
    /// at the very top, before building that frame's `TableBuilder`.
    fn take_pending_width_reset(&mut self) -> bool {
        std::mem::take(&mut self.pending_width_reset)
    }

    /// Visible columns, in display order — the single list both the header
    /// and every row must iterate identically (an `egui_extras::TableRow`
    /// panics if more cells are added than columns were pre-allocated).
    fn visible_in_order(&self) -> Vec<ColumnId> {
        self.order
            .iter()
            .copied()
            .filter(|col| self.is_visible(*col))
            .collect()
    }
}

/// Renders the virtualized task table: sortable, resizable header with a
/// persisted column layout, multi-select rows with Buzz-parity status/ETA
/// text, header/row context menus, and the Delete/⌘C/Return shortcuts.
///
/// Column widths are resizable (design spec §2.6) and persist across
/// restarts: `TableBuilder::id_salt(TABLE_ID_SALT)` gives `egui_extras` a
/// stable key for its internal `TableState`, which — with `egui_extras`'s
/// `"serde"` feature enabled workspace-wide — stores that state via
/// `ui.data_mut(|d| d.insert_persisted(..))` (`egui_extras-0.35.0/src/
/// table.rs:576-660`, vendored source). `insert_persisted`/`get_persisted`
/// go through `egui::Memory`'s `data` map, which is exactly what eframe's
/// `persistence` feature serializes into `gui.ron` on shutdown and restores
/// on the next launch (`main.rs`'s `persistence_path: config_dir()/gui.ron`)
/// — so resized widths survive a restart with no code in this crate reading
/// or writing them directly.
///
/// `modal_open` (F-004 review fix): `true` while any modal (Rename/Notes,
/// Clear-confirm, a toast's error detail) has the user's attention —
/// suppresses the Delete/⌘C/Return handling below so a modal's own text
/// field owns keyboard input instead of the table underneath it silently
/// acting on the still-selected row (see
/// `main_window::ActionState::modal_open`'s doc comment for the concrete
/// collisions this fixes).
pub fn ui(
    ui: &mut egui::Ui,
    vm: &mut TasksVm,
    layout: &mut TableLayout,
    actions: &mut Vec<TableAction>,
    modal_open: bool,
) {
    let modifiers = ui.input(|i| i.modifiers);
    if !modal_open {
        handle_keyboard_shortcuts(ui, vm, actions);
    }

    let visible_columns = layout.visible_in_order();

    // Only an in-progress row's status text (elapsed-time ETA) goes stale
    // between real events, so only schedule the tick when one exists — an
    // idle table (nothing in-progress) requests nothing extra.
    if vm
        .sorted_rows()
        .iter()
        .any(|row| row.status == Status::InProgress)
    {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    let mut builder = TableBuilder::new(ui)
        .id_salt(TABLE_ID_SALT)
        .striped(true)
        .sense(Sense::click())
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    if layout.take_pending_width_reset() {
        // Clears egui_extras's persisted TableState for TABLE_ID_SALT so
        // the next `TableState::load` recomputes fresh default widths from
        // this frame's `Column::initial` values, instead of reusing
        // whatever was resized (and persisted) before "Reset Column Order"
        // was clicked.
        builder.reset();
    }
    for col in &visible_columns {
        builder = builder.column(
            Column::initial(col.default_width())
                .clip(true)
                .at_least(40.0),
        );
    }

    let now = chrono::Local::now().naive_local();
    let sort = vm.sort();

    let mut header_union: Option<egui::Response> = None;
    let table = builder.header(22.0, |mut header| {
        for col in &visible_columns {
            let label = header_label(*col, sort);
            let mut cell_response = None;
            header.col(|ui| {
                cell_response = Some(interactive_cell(ui, "sort-click", label));
            });
            if let Some(response) = &cell_response
                && response.clicked()
            {
                vm.set_sort(*col);
            }
            header_union = union_opt(header_union, cell_response);
        }

        if let Some(union) = header_union {
            union.context_menu(|ui| {
                if ui.button(tr("table-menu-reset-column-order")).clicked() {
                    layout.reset();
                    vm.set_sort_exact(ColumnId::DateAdded, true);
                    ui.close();
                }
                ui.separator();
                for col in ColumnId::all() {
                    if !col.hideable() {
                        continue;
                    }
                    let mut visible = layout.is_visible(col);
                    if ui.checkbox(&mut visible, col.label()).changed() {
                        layout.toggle_visible(col);
                    }
                }
            });
        }
    });

    table.body(|body| {
        let total_rows = vm.sorted_rows().len();
        body.rows(20.0, total_rows, |mut row| {
            let idx = row.index();
            let row_id = vm.sorted_rows()[idx].id;
            let row_status = vm.sorted_rows()[idx].status;
            let selected = vm.selection().contains(&row_id);
            row.set_selected(selected);

            let mut row_union: Option<egui::Response> = None;
            let mut notes_cell_response: Option<egui::Response> = None;
            for col in &visible_columns {
                let text = cell_text(vm.sorted_rows(), idx, *col, now);
                let mut cell_response = None;
                row.col(|ui| {
                    cell_response = Some(interactive_cell(ui, "row-click", text));
                });
                if *col == ColumnId::Notes {
                    notes_cell_response = cell_response.clone();
                }
                row_union = union_opt(row_union, cell_response);
            }

            // Double-click on the Notes cell opens the Notes modal (Buzz
            // parity, `transcription_tasks_table_widget.py:576-585`'s
            // double-click dispatch) — checked against the Notes column's
            // *own* response, not the row-wide union, so double-clicking
            // some other cell doesn't also open Notes.
            let notes_double_clicked = notes_cell_response
                .as_ref()
                .is_some_and(egui::Response::double_clicked);
            if notes_double_clicked {
                actions.push(TableAction::Notes(row_id));
            }

            let Some(response) = row_union else {
                return;
            };
            if response.clicked() {
                apply_row_click(vm, layout, idx, row_id, modifiers);
            }
            // Double-click on any non-Notes cell opens that row in the
            // transcript viewer (Buzz `on_table_double_clicked`,
            // `main_window.py:396-400`). Selecting just this row first makes
            // the shared `OpenSelected` handler's all-selected `can_open` gate
            // reduce to a per-row gate on exactly this row (design spec §2.1:
            // double-click gates per-row). A non-openable row is a silent
            // no-op in that handler.
            if response.double_clicked() && !notes_double_clicked {
                vm.select_only(row_id);
                actions.push(TableAction::OpenSelected);
            }
            response.context_menu(|ui| {
                if matches!(
                    row_status,
                    Status::Failed | Status::Canceled | Status::Skipped
                ) {
                    if ui.button(tr("table-menu-restart")).clicked() {
                        actions.push(TableAction::Retry(row_id));
                        ui.close();
                    }
                    ui.separator();
                }
                if ui.button(tr("table-menu-rename")).clicked() {
                    actions.push(TableAction::Rename(row_id));
                    ui.close();
                }
                if ui.button(tr("table-menu-notes")).clicked() {
                    actions.push(TableAction::Notes(row_id));
                    ui.close();
                }
            });
        });
    });
}

/// Delete/⌘C/Return, gated on a non-empty selection (mirroring Buzz's own
/// enablement — none of these three do anything with nothing selected).
fn handle_keyboard_shortcuts(ui: &egui::Ui, vm: &TasksVm, actions: &mut Vec<TableAction>) {
    if vm.selection().is_empty() {
        return;
    }
    let (delete, copy, open) = ui.input(|i| {
        (
            i.key_pressed(Key::Delete),
            i.modifiers.command && i.key_pressed(Key::C),
            i.key_pressed(Key::Enter),
        )
    });
    if delete {
        actions.push(TableAction::ClearSelected);
    }
    if copy {
        ui.ctx().copy_text(vm.copy_text());
    }
    if open {
        actions.push(TableAction::OpenSelected);
    }
}

/// Applies Buzz/Qt-style click selection semantics: plain click selects only
/// this row (and sets it as the shift-click anchor); Cmd/Ctrl-click toggles
/// it; Shift-click extends the selection from the last anchor to this row.
fn apply_row_click(
    vm: &mut TasksVm,
    layout: &mut TableLayout,
    idx: usize,
    row_id: TranscriptionId,
    modifiers: Modifiers,
) {
    if modifiers.shift
        && let Some(anchor) = layout.anchor
    {
        vm.select_range(anchor, idx);
        return;
    }
    if modifiers.command {
        vm.toggle_selection(row_id);
    } else {
        vm.select_only(row_id);
    }
    layout.anchor = Some(idx);
}

/// Renders `text` and returns a *directly-captured* full-cell click response.
///
/// `egui_extras` 0.35's `TableRow::col`/`TableRow::response` return a
/// `Response` built from `Ui::response()`, which is documented to reflect
/// "last frame's widget rect" — in practice this cell-wrapper response never
/// observes a click at all (proven via a standalone repro against this
/// exact `egui`/`egui_extras`/`egui_kittest` version combination: the
/// wrapper's `clicked()` stayed `false` across 20+ settled frames after a
/// real click, while a response obtained by calling `Ui::interact` directly
/// inside the cell closure correctly reported it). This helper is the
/// workaround: it renders the label, then calls `ui.interact` over the
/// cell's own `max_rect` with a stable, salt-based id, and returns *that*
/// response — the only one this table's click/sort/context-menu handling can
/// trust. `id_salt` only needs to be unique *within one cell* (row/column
/// identity already comes from the table's own per-cell id salting), so a
/// short fixed string per call site (`"sort-click"`, `"row-click"`) is
/// enough.
fn interactive_cell(ui: &mut egui::Ui, id_salt: &str, text: String) -> egui::Response {
    ui.label(text);
    let rect = ui.max_rect();
    let id = ui.id().with(id_salt);
    ui.interact(rect, id, Sense::click())
}

/// Folds `next` into `acc` via [`egui::Response::union`], the same
/// "accumulate a row's clickable area across its cells" pattern
/// `egui_extras::TableRow` itself uses internally.
fn union_opt(acc: Option<egui::Response>, next: Option<egui::Response>) -> Option<egui::Response> {
    match (acc, next) {
        (Some(acc), Some(next)) => Some(acc.union(next)),
        (Some(acc), None) => Some(acc),
        (None, next) => next,
    }
}

/// The header label for `col`, with a sort-direction arrow appended when
/// `col` is the current sort key.
fn header_label(col: ColumnId, sort: (ColumnId, bool)) -> String {
    let label = col.label();
    if sort.0 == col {
        let arrow = if sort.1 { "\u{25bc}" } else { "\u{25b2}" };
        format!("{label} {arrow}")
    } else {
        label
    }
}

/// The display text for one cell.
fn cell_text(rows: &[RowVm], idx: usize, col: ColumnId, now: chrono::NaiveDateTime) -> String {
    let row = &rows[idx];
    match col {
        ColumnId::FileName => row.display_name.clone(),
        ColumnId::Model => row.model_text.clone(),
        ColumnId::Task => row.task_text.clone(),
        ColumnId::Status => format::status_text(row, now),
        ColumnId::DateCompleted => row
            .time_ended
            .as_deref()
            .map(format::date_text)
            .unwrap_or_default(),
        ColumnId::DateAdded => format::date_text(&row.time_queued),
        ColumnId::Notes => row.notes.clone().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui_settings(
        visible: Option<Vec<&str>>,
        order: Option<Vec<&str>>,
        sort_col: Option<&str>,
        sort_desc: Option<bool>,
    ) -> GuiSettings {
        GuiSettings {
            task_table_visible: visible.map(|v| v.into_iter().map(str::to_string).collect()),
            task_table_order: order.map(|v| v.into_iter().map(str::to_string).collect()),
            task_table_sort_column: sort_col.map(str::to_string),
            task_table_sort_descending: sort_desc,
        }
    }

    #[test]
    fn default_layout_shows_every_column_in_buzz_order() {
        let layout = TableLayout::default();
        for col in ColumnId::all() {
            assert!(layout.is_visible(col));
        }
        assert_eq!(layout.visible_in_order(), ColumnId::all().to_vec());
    }

    #[test]
    fn toggle_visible_hides_and_reshows_a_hideable_column() {
        let mut layout = TableLayout::default();
        assert!(layout.is_visible(ColumnId::Notes));
        layout.toggle_visible(ColumnId::Notes);
        assert!(!layout.is_visible(ColumnId::Notes));
        layout.toggle_visible(ColumnId::Notes);
        assert!(layout.is_visible(ColumnId::Notes));
    }

    #[test]
    fn toggle_visible_is_a_noop_for_file_name() {
        let mut layout = TableLayout::default();
        layout.toggle_visible(ColumnId::FileName);
        assert!(layout.is_visible(ColumnId::FileName));
    }

    #[test]
    fn hidden_column_is_excluded_from_visible_in_order_but_others_remain() {
        let mut layout = TableLayout::default();
        layout.toggle_visible(ColumnId::Notes);
        let visible = layout.visible_in_order();
        assert!(!visible.contains(&ColumnId::Notes));
        assert_eq!(visible.len(), ColumnId::all().len() - 1);
    }

    #[test]
    fn reset_restores_default_order_and_visibility() {
        let mut layout = TableLayout::default();
        layout.toggle_visible(ColumnId::Notes);
        layout.order.swap(0, 1);
        layout.reset();
        assert_eq!(layout.order, ColumnId::all().to_vec());
        assert!(layout.is_visible(ColumnId::Notes));
    }

    #[test]
    fn reset_requests_a_pending_width_reset_consumed_exactly_once() {
        let mut layout = TableLayout::default();
        // Nothing pending on a fresh layout.
        assert!(!layout.take_pending_width_reset());

        layout.reset();
        // The request is still there until something consumes it...
        assert!(layout.take_pending_width_reset());
        // ...and consuming it clears it, so a second read (the next frame
        // that *didn't* also click Reset) sees nothing pending.
        assert!(!layout.take_pending_width_reset());
    }

    #[test]
    fn from_gui_settings_round_trips_through_to_gui_settings() {
        let mut layout = TableLayout::default();
        layout.toggle_visible(ColumnId::Notes);
        layout.order.swap(0, 2);
        let sort = (ColumnId::Status, false);

        let settings = layout.to_gui_settings(sort);
        let restored = TableLayout::from_gui_settings(&settings);
        let restored_sort = TableLayout::sort_from_gui_settings(&settings);

        assert_eq!(restored.order, layout.order);
        assert_eq!(restored.visible, layout.visible);
        assert_eq!(restored_sort, sort);
    }

    #[test]
    fn from_gui_settings_falls_back_to_defaults_when_absent() {
        let settings = gui_settings(None, None, None, None);
        let layout = TableLayout::from_gui_settings(&settings);
        assert_eq!(layout.order, ColumnId::all().to_vec());
        assert_eq!(layout.visible, ColumnId::all().to_vec());
        assert_eq!(
            TableLayout::sort_from_gui_settings(&settings),
            (ColumnId::DateAdded, true)
        );
    }

    #[test]
    fn from_gui_settings_ignores_unrecognized_column_ids_and_falls_back() {
        let settings = gui_settings(
            Some(vec!["not-a-real-column"]),
            Some(vec!["also-not-real"]),
            Some("nope"),
            Some(false),
        );
        let layout = TableLayout::from_gui_settings(&settings);
        // Every entry was unrecognized, so both lists fall back to the
        // built-in default rather than ending up empty.
        assert_eq!(layout.order, ColumnId::all().to_vec());
        assert_eq!(layout.visible, ColumnId::all().to_vec());
        // An unrecognized sort column falls back to the default column, but
        // the (independently valid) descending flag is still honored.
        assert_eq!(
            TableLayout::sort_from_gui_settings(&settings),
            (ColumnId::DateAdded, false)
        );
    }

    #[test]
    fn from_gui_settings_always_keeps_file_name_visible() {
        let settings = gui_settings(
            Some(vec!["model", "status"]), // file_name deliberately omitted
            None,
            None,
            None,
        );
        let layout = TableLayout::from_gui_settings(&settings);
        assert!(layout.is_visible(ColumnId::FileName));
    }

    #[test]
    fn from_gui_settings_appends_a_column_missing_from_a_stale_persisted_order() {
        // Simulates a settings file written before `Notes` existed.
        let ids_without_notes: Vec<&str> = ColumnId::all()
            .into_iter()
            .filter(|c| *c != ColumnId::Notes)
            .map(ColumnId::id)
            .collect();
        let settings = gui_settings(None, Some(ids_without_notes), None, None);
        let layout = TableLayout::from_gui_settings(&settings);
        assert!(layout.order.contains(&ColumnId::Notes));
        assert_eq!(layout.order.last(), Some(&ColumnId::Notes));
    }

    #[test]
    fn header_label_appends_no_arrow_for_a_non_sorted_column() {
        let label = header_label(ColumnId::Model, (ColumnId::DateAdded, true));
        assert_eq!(label, ColumnId::Model.label());
    }

    #[test]
    fn header_label_appends_descending_arrow() {
        let label = header_label(ColumnId::DateAdded, (ColumnId::DateAdded, true));
        assert!(label.ends_with('\u{25bc}'));
    }

    #[test]
    fn header_label_appends_ascending_arrow() {
        let label = header_label(ColumnId::DateAdded, (ColumnId::DateAdded, false));
        assert!(label.ends_with('\u{25b2}'));
    }

    #[test]
    fn plain_click_selects_only_that_row_and_sets_anchor() {
        let mut vm = TasksVm::default();
        let mut layout = TableLayout::default();
        let id = TranscriptionId::new();

        apply_row_click(&mut vm, &mut layout, 3, id, Modifiers::NONE);

        assert_eq!(vm.selection().len(), 1);
        assert!(vm.selection().contains(&id));
        assert_eq!(layout.anchor, Some(3));
    }

    #[test]
    fn command_click_toggles_without_clearing_other_selection() {
        let mut vm = TasksVm::default();
        let mut layout = TableLayout::default();
        let first = TranscriptionId::new();
        let second = TranscriptionId::new();
        vm.select_only(first);

        apply_row_click(&mut vm, &mut layout, 1, second, Modifiers::COMMAND);

        assert!(vm.selection().contains(&first));
        assert!(vm.selection().contains(&second));
    }
}
