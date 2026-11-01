//! The segment editor table (F-081): a virtualized `egui_extras::TableBuilder`
//! over an open transcript's in-memory rows, with inline cell editing.
//!
//! Columns are **Start | End | Text | Translation** (display order — Buzz swaps
//! its odd internal END-before-START order back to this); Start/End are a fixed
//! 120 px, the text columns split the remainder, and Translation is hidden
//! unless the transcript [`has_translations`](ViewerVm::has_translations). Rows
//! are a fixed 4-text-line height (Buzz `init_row_height`) so the table stays
//! cheap at 100k segments — **only the one actively-edited cell instantiates a
//! `TextEdit`**; every other cell is a plain label.
//!
//! Editing is Buzz-faithful (`transcription_segments_editor_widget.py`):
//! - **Begin** on double-click of a cell, or Return on the selected row (which
//!   edits its Start cell — Buzz edits the current cell; simplified to the
//!   leftmost editable cell, since egui has no per-cell keyboard focus cursor).
//! - **Timestamp cells** (`TimeStampLineEdit`): a single-line field filtered to
//!   `[0-9:.]`; the `+`/`-` keys nudge the buffer ±500 ms (floored at 0)
//!   *before* the field consumes them; commit on Enter/Tab/focus-out; a garbage
//!   buffer reverts silently.
//! - **Text/Translation cells** (`CustomTextEdit`): a multi-line field; Enter,
//!   Tab **and Esc** all commit (Esc-commits is spec ledger #6), plus focus-out.
//!
//! Every commit routes through [`ViewerVm::apply_timestamp_edit`] /
//! [`ViewerVm::apply_text_edit`], which run the pure core clamp rule
//! (`humm_core::viewer::edit`) and persist to the store — this view never
//! re-derives an edit decision. All per-frame edit intents (select / begin /
//! commit) are collected during the render pass and applied to `vm` afterward,
//! so the cell closures never need a mutable borrow of `vm` (the M3
//! `task_table` pattern).

use egui::{Key, Modifiers, Sense};
use egui_extras::{Column, TableBuilder};

use humm_core::domain::Millis;
use humm_core::export::to_timestamp;
use humm_core::viewer::edit::NUDGE_MS;
use humm_core::viewer::timestamp::parse_timestamp;
use humm_store::SegmentRow;

use crate::i18n::tr;
use crate::viewer::{CellEditor, EditCol, ViewerDeps, ViewerVm};
use crate::views::viewer_search;

/// Stable `TableBuilder::id_salt` for the (per-viewport) editor table — keeps
/// egui_extras's internal table state distinct from the M3 task table's.
const TABLE_ID_SALT: &str = "humm-viewer-editor-table";
/// Header-row height.
const HEADER_HEIGHT: f32 = 22.0;
/// Fixed Start/End column width (Buzz `setColumnWidth(START/END, 120)`).
const TIME_COL_WIDTH: f32 = 120.0;
/// Minimum width a text column keeps when the remainder is split.
const TEXT_COL_MIN_WIDTH: f32 = 120.0;

/// One committed edit this frame, returned so the caller can react (Task 10's
/// loop-range live-follow consumes committed Start/End edits). A real public
/// signal, not dead code — the documented seam for the not-yet-built player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditApplied {
    /// The edited row's index into [`ViewerVm::rows`].
    pub row: usize,
    /// The edited column.
    pub col: EditCol,
}

/// Renders the editor table into `ui`, applying any cell edits committed this
/// frame through the store-backed [`ViewerVm`] commit methods. Returns the list
/// of edits that landed (empty on a plain-render frame).
pub fn ui(ui: &mut egui::Ui, vm: &mut ViewerVm, deps: &ViewerDeps) -> Vec<EditApplied> {
    let mut applied = Vec::new();

    let line_height = ui.text_style_height(&egui::TextStyle::Body);
    let row_height = line_height * 4.0;
    let table_height = ui.available_height().max(row_height);
    let columns = visible_columns(vm.has_translations);

    // A one-shot center-scroll requested this frame: a search match (F-082,
    // `highlight_table_match`) or a playback scroll (follow-audio /
    // scroll-to-current / loop re-enable, F-083). Search takes precedence when
    // both are pending (a search action is always the more recent intent).
    let search_scroll_row =
        viewer_search::take_pending_row_scroll(vm).or_else(|| vm.playback_scroll.take());

    // Return begins editing the selected row's Start cell when no editor is
    // open. Read before the editor is pulled out below, so a Return that
    // *commits* an open editor (handled in `render_editor`) never also opens a
    // new one.
    let mut begin_edit: Option<(usize, EditCol)> = None;
    if vm.editing.is_none()
        && let Some(selected) = vm.selected
        && selected < vm.rows.len()
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
    {
        begin_edit = Some((selected, EditCol::Start));
    }

    // Pull the active editor out so the per-cell closures can mutate its buffer
    // without reborrowing `vm` (whose rows they read immutably for labels).
    let mut editing = vm.editing.take();
    let mut click_select: Option<usize> = None;
    let mut should_commit = false;
    let row_count = vm.rows.len();

    let mut builder = TableBuilder::new(ui)
        .id_salt(TABLE_ID_SALT)
        .striped(true)
        .sense(Sense::click())
        .resizable(false)
        .max_scroll_height(table_height)
        .cell_layout(egui::Layout::left_to_right(egui::Align::TOP));
    for col in &columns {
        builder = builder.column(column_spec(*col));
    }
    if let Some(row) = search_scroll_row {
        builder = builder.scroll_to_row(row, Some(egui::Align::Center));
    }

    builder
        .header(HEADER_HEIGHT, |mut header| {
            for col in &columns {
                header.col(|ui| {
                    ui.strong(column_header(*col));
                });
            }
        })
        .body(|body| {
            body.rows(row_height, row_count, |mut row| {
                let idx = row.index();
                row.set_selected(vm.selected == Some(idx));
                for &col in &columns {
                    let editing_here = editing
                        .as_ref()
                        .is_some_and(|editor| editor.row == idx && editor.col == col);
                    // Precompute the label so the cell closure never borrows
                    // `vm` (keeps `editing`'s mutable borrow conflict-free).
                    let label = if editing_here {
                        String::new()
                    } else {
                        cell_display(&vm.rows[idx], col)
                    };
                    let mut response = None;
                    row.col(|ui| {
                        if editing_here {
                            if render_editor(ui, editing.as_mut().expect("editing cell")) {
                                should_commit = true;
                            }
                        } else {
                            response = Some(interactive_cell(ui, col, label));
                        }
                    });
                    if let Some(response) = response {
                        if response.double_clicked() {
                            begin_edit = Some((idx, col));
                            click_select = Some(idx);
                        } else if response.clicked() {
                            click_select = Some(idx);
                        }
                    }
                }
            });
        });

    // Restore the (possibly mutated) editor, then apply this frame's intents
    // against a now-freely-mutable `vm`: commit an open editor first, then
    // selection, then begin a new edit — so a double-click that both commits
    // the old editor and opens a new one lands cleanly.
    vm.editing = editing;
    if should_commit && let Some(editor) = vm.editing.take() {
        commit_editor(vm, deps, editor, &mut applied);
    }
    if let Some(idx) = click_select {
        vm.selected = Some(idx);
        // Record the genuine pointer selection for the player's segment-click
        // logic (seek-if-outside + loop range). Search/follow selection sets
        // `vm.selected` through other paths, so it never triggers a seek.
        vm.segment_click = Some(idx);
    }
    if let Some((idx, col)) = begin_edit
        && vm.editing.is_none()
        && idx < vm.rows.len()
    {
        vm.editing = Some(open_editor(&vm.rows[idx], idx, col));
        vm.selected = Some(idx);
    }

    applied
}

/// The visible columns in display order — Translation only when the transcript
/// has translations (Buzz hides it otherwise, `resizeEvent`).
fn visible_columns(has_translations: bool) -> Vec<EditCol> {
    let mut columns = vec![EditCol::Start, EditCol::End, EditCol::Text];
    if has_translations {
        columns.push(EditCol::Translation);
    }
    columns
}

/// The `egui_extras` column spec: fixed width for timestamps, remainder split
/// for the text columns.
fn column_spec(col: EditCol) -> Column {
    match col {
        EditCol::Start | EditCol::End => Column::exact(TIME_COL_WIDTH),
        EditCol::Text | EditCol::Translation => Column::remainder().at_least(TEXT_COL_MIN_WIDTH),
    }
}

/// The localized header label for a column.
fn column_header(col: EditCol) -> String {
    tr(match col {
        EditCol::Start => "viewer-col-start",
        EditCol::End => "viewer-col-end",
        EditCol::Text => "viewer-col-text",
        EditCol::Translation => "viewer-col-translation",
    })
}

/// The read-only display text for a cell: `HH:MM:SS.mmm` for timestamps, the
/// stored string for text columns.
fn cell_display(row: &SegmentRow, col: EditCol) -> String {
    match col {
        EditCol::Start => to_timestamp(row.start, '.'),
        EditCol::End => to_timestamp(row.end, '.'),
        EditCol::Text => row.text.clone(),
        EditCol::Translation => row.translation.clone(),
    }
}

/// Renders a non-editing cell's label and returns a directly-captured full-cell
/// click response (the M3 `task_table::interactive_cell` workaround: the
/// `egui_extras` cell response never observes clicks against this
/// egui/egui_extras version, so we `ui.interact` over the cell rect ourselves;
/// `id_salt` need only be unique within one cell).
fn interactive_cell(ui: &mut egui::Ui, col: EditCol, text: String) -> egui::Response {
    ui.add(egui::Label::new(text).wrap().selectable(false));
    let rect = ui.max_rect();
    let id = ui.id().with(col_salt(col));
    ui.interact(rect, id, Sense::click())
}

/// A short, per-column-stable id-salt for the cell interact id.
fn col_salt(col: EditCol) -> &'static str {
    match col {
        EditCol::Start => "cell-start",
        EditCol::End => "cell-end",
        EditCol::Text => "cell-text",
        EditCol::Translation => "cell-translation",
    }
}

/// Opens a [`CellEditor`] for `col` on `row` (index `idx`), seeding its buffer
/// from the current cell value and (for timestamps) the revert target.
fn open_editor(row: &SegmentRow, idx: usize, col: EditCol) -> CellEditor {
    match col {
        EditCol::Start => CellEditor::new(idx, col, to_timestamp(row.start, '.'), row.start),
        EditCol::End => CellEditor::new(idx, col, to_timestamp(row.end, '.'), row.end),
        EditCol::Text => CellEditor::new(idx, col, row.text.clone(), Millis(0)),
        EditCol::Translation => CellEditor::new(idx, col, row.translation.clone(), Millis(0)),
    }
}

/// Renders the active cell's editor and returns whether it should commit this
/// frame (Enter/Tab/Esc as appropriate, or focus-out).
fn render_editor(ui: &mut egui::Ui, editor: &mut CellEditor) -> bool {
    if editor.col.is_timestamp() {
        render_timestamp_editor(ui, editor)
    } else {
        render_text_editor(ui, editor)
    }
}

/// The single-line timestamp editor (`TimeStampLineEdit`): `+`/`-` nudge the
/// buffer ±500 ms (floored at 0) before the field sees them, the buffer is
/// filtered to `[0-9:.]`, and Enter/Tab/focus-out commit.
fn render_timestamp_editor(ui: &mut egui::Ui, editor: &mut CellEditor) -> bool {
    // Intercept +/- as text events before the TextEdit inserts them (Buzz
    // `keyPressEvent`), accumulating a net nudge.
    let nudge = ui.input_mut(|input| {
        let mut delta = 0i64;
        input.events.retain(|event| match event {
            egui::Event::Text(text) if text == "+" => {
                delta += NUDGE_MS;
                false
            }
            egui::Event::Text(text) if text == "-" => {
                delta -= NUDGE_MS;
                false
            }
            _ => true,
        });
        delta
    });
    if nudge != 0 {
        let nudged = nudge_buffer_ms(&editor.buffer, editor.last_valid_ms, nudge);
        editor.last_valid_ms = nudged;
        editor.buffer = to_timestamp(nudged, '.');
    }

    // Enter/Tab commit; consume them so the field neither submits nor tabs away.
    let commit_key = ui.input_mut(|input| {
        input.consume_key(Modifiers::NONE, Key::Enter)
            || input.consume_key(Modifiers::NONE, Key::Tab)
    });

    let response =
        ui.add(egui::TextEdit::singleline(&mut editor.buffer).desired_width(f32::INFINITY));
    // Strip anything outside [0-9:.] (Buzz's `^[0-9:.]*$` validator).
    if response.changed() {
        editor
            .buffer
            .retain(|c| c.is_ascii_digit() || c == ':' || c == '.');
    }
    if editor.just_opened {
        response.request_focus();
        editor.just_opened = false;
    }
    commit_key || response.lost_focus()
}

/// The multi-line text/translation editor (`CustomTextEdit`): Enter, Tab and
/// Esc all commit (Esc-commits is the Buzz quirk, spec ledger #6), plus
/// focus-out.
fn render_text_editor(ui: &mut egui::Ui, editor: &mut CellEditor) -> bool {
    // Consume all three commit keys before the field so it inserts nothing and
    // egui's own Esc handling is bypassed.
    let commit_key = ui.input_mut(|input| {
        input.consume_key(Modifiers::NONE, Key::Enter)
            || input.consume_key(Modifiers::NONE, Key::Tab)
            || input.consume_key(Modifiers::NONE, Key::Escape)
    });
    let response = ui.add(
        egui::TextEdit::multiline(&mut editor.buffer)
            .desired_width(f32::INFINITY)
            .desired_rows(3),
    );
    if editor.just_opened {
        response.request_focus();
        editor.just_opened = false;
    }
    commit_key || response.lost_focus()
}

/// Commits a finished editor: timestamp cells parse the buffer and route
/// through [`ViewerVm::apply_timestamp_edit`]. A buffer that is not a time,
/// or a range the core rule rejects, writes nothing and leaves one viewer
/// line saying the previous time was kept. Text cells store the buffer
/// as-typed via [`ViewerVm::apply_text_edit`].
fn commit_editor(
    vm: &mut ViewerVm,
    deps: &ViewerDeps,
    editor: CellEditor,
    applied: &mut Vec<EditApplied>,
) {
    if editor.row >= vm.rows.len() {
        return;
    }
    match editor.col.as_time_field() {
        Some(field) => {
            if let Some(ms) = parse_timestamp(&editor.buffer) {
                if vm.apply_timestamp_edit(deps, editor.row, field, ms, false) {
                    applied.push(EditApplied {
                        row: editor.row,
                        col: editor.col,
                    });
                }
            } else {
                vm.note_unparsed_timestamp();
            }
        }
        None => {
            let (row, col) = (editor.row, editor.col);
            vm.apply_text_edit(deps, row, col, editor.buffer);
            applied.push(EditApplied { row, col });
        }
    }
}

/// Nudges a timestamp editor buffer by `delta` ms (the in-editor `+`/`-` keys),
/// flooring at zero (Buzz `TimeStampLineEdit.keyPressEvent`'s `max(0, …)`).
/// Bases the nudge on the buffer's current parse, falling back to `last_valid`
/// when the buffer is mid-edit garbage.
pub fn nudge_buffer_ms(buffer: &str, last_valid: Millis, delta: i64) -> Millis {
    let base = parse_timestamp(buffer).unwrap_or(last_valid);
    Millis((base.0 + delta).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn nudge_buffer_adds_and_subtracts_500ms() {
        assert_eq!(
            nudge_buffer_ms("00:00:01.000", Millis(1000), NUDGE_MS),
            Millis(1500)
        );
        assert_eq!(
            nudge_buffer_ms("00:00:01.000", Millis(1000), -NUDGE_MS),
            Millis(500)
        );
    }

    #[test]
    fn nudge_buffer_floors_at_zero() {
        // 300 − 500 = −200 → floored to 0.
        assert_eq!(
            nudge_buffer_ms("00:00:00.300", Millis(300), -NUDGE_MS),
            Millis(0)
        );
        // Already at zero stays at zero.
        assert_eq!(
            nudge_buffer_ms("00:00:00.000", Millis(0), -NUDGE_MS),
            Millis(0)
        );
    }

    #[test]
    fn nudge_buffer_falls_back_to_last_valid_on_garbage() {
        // Unparseable buffer → nudge the last valid value instead.
        assert_eq!(
            nudge_buffer_ms("not-a-time", Millis(2000), NUDGE_MS),
            Millis(2500)
        );
    }

    #[test]
    fn visible_columns_gates_translation_on_has_translations() {
        assert_eq!(
            visible_columns(false),
            vec![EditCol::Start, EditCol::End, EditCol::Text]
        );
        assert_eq!(
            visible_columns(true),
            vec![
                EditCol::Start,
                EditCol::End,
                EditCol::Text,
                EditCol::Translation
            ]
        );
    }
}
