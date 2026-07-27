//! Task-table view-model (F-003): [`RowVm`] (one display row, derived from a
//! [`TaskRow`] plus its persisted [`TaskSpec`]), [`TasksVm`] (the sorted,
//! selectable collection `views::task_table` renders), and [`ColumnId`] (the
//! table's seven Buzz-parity columns —
//! `transcription_tasks_table_widget.py:109-183`).
//!
//! `TasksVm` owns two kinds of update: [`TasksVm::load`] populates it once
//! from the store at startup, and [`TasksVm::apply`] folds in each
//! [`TaskEvent`] afterward without ever re-listing the whole table. See
//! `apply`'s doc comment for exactly which event kinds touch the store and
//! which mutate in place.

use std::collections::BTreeSet;
use std::path::PathBuf;

use humm_core::domain::{ModelSpec, Status, TaskKind, TranscriptionId};
use humm_core::queue::{TaskEvent, TaskEventKind, TaskPhase};
use humm_store::{SqliteStore, TaskRow};

use crate::i18n::tr;

/// F-025's download-phase override: while a row's `status` is `InProgress`,
/// a non-`None` phase replaces the normal progress/ETA text
/// (`crate::format::status_text`).
///
/// Carries its own fraction (rather than being a bare `None`/`Model`/`Media`
/// tag) because that fraction comes from a different source per phase
/// (`ModelDownloadProgress`'s `downloaded`/`total`, only known once `total`
/// is `Some`; `DownloadProgress`'s fraction, always known when emitted) and
/// `status_text` needs it to render `"(NN%)"` — threading it back out
/// through a side channel would only recreate this same data in a clumsier
/// shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// No download in progress; render the normal status text.
    None,
    /// Downloading a model/VAD artifact. `Some(fraction)` once the artifact's
    /// total size is known; `None` renders "Downloading model…" with no
    /// percent (`TaskEventKind::ModelDownloadProgress { total: None, .. }`).
    Model(Option<f32>),
    /// Downloading a URL task's media. Always carries a known fraction —
    /// `TaskEventKind::DownloadProgress` is only ever emitted while the
    /// total size is known (`humm_core::queue` module doc).
    Media(f32),
    /// Isolating speech from background noise/music with demucs before
    /// transcription (F-027, M8, `TaskEventKind::PhaseChanged { phase:
    /// TaskPhase::IsolatingSpeech }`). No fraction: the subprocess only
    /// emits human-oriented progress on stderr (Standing Policy rule 3
    /// forbids parsing it), so this phase is indeterminate.
    Isolating,
}

/// One task-table row, derived from a [`TaskRow`] plus (when available) its
/// persisted [`humm_core::domain::TaskSpec`] — everything
/// `views::task_table` needs to render a row without touching the store
/// itself.
#[derive(Debug, Clone, PartialEq)]
pub struct RowVm {
    /// The task's id (stable across the row's lifetime).
    pub id: TranscriptionId,
    /// File Name / URL column text: `name` else the file's basename else
    /// `url` else empty (Buzz: `record.name else basename(file) else url else
    /// ""`, treating an empty `name` the same as absent — Python's `or`
    /// chain treats `""` as falsy).
    pub display_name: String,
    /// Model column text: `"Hugging Face (<repo>)"` / `"Whisper.cpp
    /// (<size>)"` for a humm-created row, or a best-effort raw-column
    /// fallback for an imported row with no persisted `TaskSpec` — see
    /// `row_vm_from_task_row`.
    pub model_text: String,
    /// Task column text: `"Transcribe"` / `"Translate to English"`.
    pub task_text: String,
    /// Current lifecycle status.
    pub status: Status,
    /// Fraction complete, `0.0..=1.0` (mirrors `TaskRow::progress`, kept as
    /// `f32` to match `TaskEventKind::Progress`'s own type).
    pub progress: f32,
    /// F-025 download-phase override (see [`Phase`]).
    pub phase: Phase,
    /// Failure message, set only when `status` is [`Status::Failed`].
    pub error: Option<String>,
    /// When the task was queued (naive-local ISO-8601).
    pub time_queued: String,
    /// When the task started running, if it has.
    pub time_started: Option<String>,
    /// When the task finished, if it has.
    pub time_ended: Option<String>,
    /// User-assigned notes.
    pub notes: Option<String>,
    /// Resolved local file path, if any (used by `copy_text` and the
    /// eventual viewer/open action).
    pub file: Option<PathBuf>,
    /// Source URL, if any.
    pub url: Option<String>,
}

/// The task table's seven Buzz-parity columns, in Buzz's own default visual
/// order (`transcription_tasks_table_widget.py:109-183`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnId {
    /// File Name / URL — 400pt, not hideable.
    FileName,
    /// Model — 180pt.
    Model,
    /// Task — 120pt.
    Task,
    /// Status — 180pt.
    Status,
    /// Date Completed — 180pt.
    DateCompleted,
    /// Date Added — 180pt.
    DateAdded,
    /// Notes — 300pt.
    Notes,
}

impl ColumnId {
    /// Every column, in Buzz's default visual order.
    pub fn all() -> [ColumnId; 7] {
        [
            ColumnId::FileName,
            ColumnId::Model,
            ColumnId::Task,
            ColumnId::Status,
            ColumnId::DateCompleted,
            ColumnId::DateAdded,
            ColumnId::Notes,
        ]
    }

    /// The stable, persisted identifier (`settings.gui.task_table_*`
    /// entries) — never changes once shipped, unlike the display label.
    pub fn id(self) -> &'static str {
        match self {
            ColumnId::FileName => "file_name",
            ColumnId::Model => "model",
            ColumnId::Task => "task",
            ColumnId::Status => "status",
            ColumnId::DateCompleted => "date_completed",
            ColumnId::DateAdded => "date_added",
            ColumnId::Notes => "notes",
        }
    }

    /// Parses [`Self::id`]'s output back into a `ColumnId`; `None` for an
    /// unrecognized string (a stale/foreign settings file entry — the caller
    /// drops it rather than failing to load).
    pub fn from_id(id: &str) -> Option<ColumnId> {
        ColumnId::all().into_iter().find(|col| col.id() == id)
    }

    /// The default column width in points (Buzz's own defaults,
    /// `column_definitions`).
    pub fn default_width(self) -> f32 {
        match self {
            ColumnId::FileName => 400.0,
            ColumnId::Model => 180.0,
            ColumnId::Task => 120.0,
            ColumnId::Status => 180.0,
            ColumnId::DateCompleted => 180.0,
            ColumnId::DateAdded => 180.0,
            ColumnId::Notes => 300.0,
        }
    }

    /// Whether the header context menu offers to hide this column. Only
    /// `FileName` is not hideable (Buzz parity).
    pub fn hideable(self) -> bool {
        !matches!(self, ColumnId::FileName)
    }

    /// The Fluent catalog key for this column's header label.
    fn label_key(self) -> &'static str {
        match self {
            ColumnId::FileName => "table-col-file-name",
            ColumnId::Model => "table-col-model",
            ColumnId::Task => "table-col-task",
            ColumnId::Status => "table-col-status",
            ColumnId::DateCompleted => "table-col-date-completed",
            ColumnId::DateAdded => "table-col-date-added",
            ColumnId::Notes => "table-col-notes",
        }
    }

    /// The column's localized header label.
    pub fn label(self) -> String {
        tr(self.label_key())
    }
}

/// An action the table view wants applied but cannot itself apply — it has
/// no access to the queue/store beyond what `TasksVm`/`format` already
/// expose. Pushed onto the `actions: &mut Vec<TableAction>` the render
/// function (`views::task_table::ui`) is handed; `app.rs` is the intended
/// consumer starting Task 8. **This task**, `app.rs` only holds or discards
/// them (documented at that call site) — nothing here is wired to the queue
/// or store yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAction {
    /// Delete key: clear (delete) every selected row.
    ClearSelected,
    /// Return key: open the selected row(s) in the (not-yet-shipped, M4)
    /// transcript viewer.
    OpenSelected,
    /// Row context menu "Restart Transcription", gated to
    /// failed/canceled/skipped rows.
    Retry(TranscriptionId),
    /// Row context menu "Rename".
    Rename(TranscriptionId),
    /// Row context menu "Add/Edit Notes".
    Notes(TranscriptionId),
    /// Toolbar "Cancel Transcription" / ⌘X: cancel every selected row whose
    /// status is queued/in-progress (Task 8, F-004/F-026). Shares this enum
    /// (rather than a separate action type) because the toolbar pushes onto
    /// the same `actions: &mut Vec<TableAction>` the table and its keyboard
    /// shortcuts already do — one accumulation point, one drain loop.
    CancelSelected,
}

/// `col`'s comparable projection of `row` — exactly the value
/// [`TasksVm::resort`]'s comparator orders by (lowercased for the free-text
/// columns so sorting is case-insensitive; verbatim for `Status` (Buzz wire
/// string) and the two date columns (ISO-8601 sorts lexically identically to
/// chronologically)). The single definition both [`TasksVm::resort`] and
/// [`TasksVm::mutate`]'s resort-skip check use, so the skip can never drift
/// from the real ordering rules (carried M3 ticket, spec §8.2).
fn sort_key(row: &RowVm, col: ColumnId) -> String {
    match col {
        ColumnId::FileName => row.display_name.to_lowercase(),
        ColumnId::Model => row.model_text.to_lowercase(),
        ColumnId::Task => row.task_text.to_lowercase(),
        ColumnId::Status => row.status.as_str().to_string(),
        ColumnId::DateCompleted => row.time_ended.as_deref().unwrap_or("").to_string(),
        ColumnId::DateAdded => row.time_queued.clone(),
        ColumnId::Notes => row.notes.as_deref().unwrap_or("").to_lowercase(),
    }
}

/// The task table's view-model: display rows in current sort order, plus
/// multi-row selection. Constructed once via [`TasksVm::load`]; kept current
/// afterward via [`TasksVm::apply`] rather than re-querying the whole table
/// on every event.
#[derive(Debug)]
pub struct TasksVm {
    rows: Vec<RowVm>,
    sort: (ColumnId, bool),
    selection: BTreeSet<TranscriptionId>,
    /// Test-only [`TasksVm::resort`] invocation counter — how
    /// `mutate_skips_resort_when_sort_key_unchanged` /
    /// `mutate_resorts_when_key_changes` observe whether [`TasksVm::mutate`]'s
    /// resort-skip actually fired, without a production-facing seam.
    #[cfg(test)]
    resort_calls: u32,
}

impl Default for TasksVm {
    /// Empty, sorted `(DateAdded, descending)` — Buzz's own default
    /// (`transcription_tasks_table_widget.py:263`,
    /// `Column.TIME_QUEUED DESC`).
    fn default() -> Self {
        TasksVm {
            rows: Vec::new(),
            sort: (ColumnId::DateAdded, true),
            selection: BTreeSet::new(),
            #[cfg(test)]
            resort_calls: 0,
        }
    }
}

impl TasksVm {
    /// Loads every task from `store`, sorted `(DateAdded, descending)`.
    pub fn load(store: &SqliteStore) -> Self {
        let rows = match store.list_tasks() {
            Ok(rows) => rows
                .into_iter()
                .map(|row| row_vm_from_task_row(store, row))
                .collect(),
            Err(err) => {
                log::warn!("TasksVm::load: list_tasks failed: {err}");
                Vec::new()
            }
        };
        let mut vm = TasksVm {
            rows,
            ..TasksVm::default()
        };
        vm.resort();
        vm
    }

    /// Builds a [`TasksVm`] directly from a pre-built row list, sorted per
    /// the built-in default (`DateAdded`, descending) — bypassing
    /// [`Self::load`]'s per-row `store.load_task_spec`/`model_display_columns`
    /// reads entirely.
    ///
    /// **Bench-only.** Not a production constructor: it exists so
    /// `benches/table_scroll.rs` can synthesize a 100 000-row view-model
    /// cheaply (every [`RowVm`] field is already `pub`, so the bench builds
    /// them directly) without either paying for 100 000 real
    /// `SqliteStore::create_task` + `load_task_spec` round trips just to
    /// populate test data, or duplicating this struct's private fields in
    /// the bench crate. `#[doc(hidden)]` and the `_bench` name keep it out
    /// of the crate's normal public-API surface and obvious at every call
    /// site that it isn't one.
    #[doc(hidden)]
    pub fn for_bench(rows: Vec<RowVm>) -> Self {
        let mut vm = TasksVm {
            rows,
            ..TasksVm::default()
        };
        vm.resort();
        vm
    }

    /// Folds one [`TaskEvent`] into the view-model, per its kind:
    ///
    /// - [`TaskEventKind::Queued`]: reads the new row from `store`
    ///   (`get_task`) and inserts it at the correct sort position.
    /// - [`TaskEventKind::Started`]: refreshes the row from `store` (only
    ///   source of the freshly-set `time_started`, which the in-progress ETA
    ///   text needs).
    /// - [`TaskEventKind::Progress`] /
    ///   [`TaskEventKind::ModelDownloadProgress`] /
    ///   [`TaskEventKind::DownloadProgress`] / [`TaskEventKind::PhaseChanged`]:
    ///   mutate the row in place — no store read. `Progress` clears `phase`;
    ///   the two download-progress kinds and `PhaseChanged` (M8, F-027) set
    ///   it.
    /// - [`TaskEventKind::SegmentAdded`]: no-op (`RowVm` doesn't track
    ///   segment text).
    /// - Terminal ([`TaskEventKind::Completed`]/[`TaskEventKind::Failed`]/
    ///   [`TaskEventKind::Canceled`]/[`TaskEventKind::Skipped`]): refreshes
    ///   the row from `store` (the authoritative terminal
    ///   `status`/`time_ended`/`error_message`) — `Skipped` needs the same
    ///   store round-trip since `mark_skipped` is what actually writes the
    ///   row's terminal state (no dispatch produces this event yet).
    /// - [`TaskEventKind::PostProcessed`]: refreshes the row from `store`
    ///   too — a hook may have updated notes or replaced segments (design
    ///   spec §2.3: "the GUI refreshes the row ... on it", mirroring Buzz's
    ///   `refresh_row`-after-hooks); this refresh alone covers the notes
    ///   column (`row_vm_from_task_row` re-reads `TaskRow::notes`). An
    ///   open viewer's own segment reload (on `segments_replaced`) is a
    ///   separate concern `HummApp::react_to_event` handles — this
    ///   view-model has no notion of open viewer windows.
    /// - [`TaskEventKind::StoreWriteFailed`]: logged, not reflected in any
    ///   row (ADR-018 — the run continues regardless of this write's
    ///   outcome, so there is no row state to correct).
    ///
    /// An event for an id this view-model has no row for (and, for `Queued`,
    /// an id `store.get_task` can't find) is dropped with a debug log rather
    /// than panicking or erroring — a page reload/detached subscriber racing
    /// the queue is expected, not exceptional.
    pub fn apply(&mut self, ev: &TaskEvent, store: &SqliteStore) {
        match &ev.kind {
            TaskEventKind::Queued => self.insert_from_store(ev.task_id, store),
            TaskEventKind::Started => self.refresh_from_store(ev.task_id, store),
            TaskEventKind::Progress(fraction) => {
                let fraction = *fraction;
                self.mutate(ev.task_id, |row| {
                    row.status = Status::InProgress;
                    row.progress = fraction;
                    row.phase = Phase::None;
                });
            }
            TaskEventKind::ModelDownloadProgress {
                downloaded, total, ..
            } => {
                let fraction = total
                    .filter(|total| *total > 0)
                    .map(|total| *downloaded as f32 / total as f32);
                self.mutate(ev.task_id, |row| {
                    row.status = Status::InProgress;
                    row.phase = Phase::Model(fraction);
                });
            }
            TaskEventKind::DownloadProgress(fraction) => {
                let fraction = *fraction;
                self.mutate(ev.task_id, |row| {
                    row.status = Status::InProgress;
                    row.phase = Phase::Media(fraction);
                });
            }
            TaskEventKind::PhaseChanged { phase } => {
                let phase = *phase;
                self.mutate(ev.task_id, |row| {
                    row.status = Status::InProgress;
                    row.phase = match phase {
                        TaskPhase::IsolatingSpeech => Phase::Isolating,
                    };
                });
            }
            TaskEventKind::SegmentAdded(_) => {}
            TaskEventKind::Completed { .. }
            | TaskEventKind::Failed { .. }
            | TaskEventKind::Canceled
            | TaskEventKind::Skipped
            | TaskEventKind::PostProcessed { .. } => {
                self.refresh_from_store(ev.task_id, store);
            }
            TaskEventKind::StoreWriteFailed { operation, detail } => {
                log::debug!(
                    "TasksVm::apply: StoreWriteFailed({operation}) for task {}: {detail} \
                     (row left as-is; the run continues regardless per ADR-018)",
                    ev.task_id
                );
            }
        }
    }

    /// Current rows, in current sort order.
    pub fn sorted_rows(&self) -> &[RowVm] {
        &self.rows
    }

    /// The current sort: which column, and whether it's descending.
    pub fn sort(&self) -> (ColumnId, bool) {
        self.sort
    }

    /// Header-click sort toggle (Buzz/Qt behavior): clicking a column that
    /// isn't already the sort key sorts by it ascending; clicking the
    /// already-active column toggles its direction.
    pub fn set_sort(&mut self, col: ColumnId) {
        if self.sort.0 == col {
            self.sort.1 = !self.sort.1;
        } else {
            self.sort = (col, false);
        }
        self.resort();
    }

    /// Sets the sort to an exact `(column, descending)` pair without the
    /// toggle logic [`Self::set_sort`] applies — for restoring a persisted
    /// sort at startup, and for "Reset Column Order"'s
    /// `(DateAdded, descending)` reset (Buzz: `reset_column_order` re-sorts
    /// `TIME_QUEUED DESC`).
    pub fn set_sort_exact(&mut self, col: ColumnId, descending: bool) {
        self.sort = (col, descending);
        self.resort();
    }

    /// The current selection, by id.
    pub fn selection(&self) -> &BTreeSet<TranscriptionId> {
        &self.selection
    }

    /// Plain click: select only `id`.
    pub fn select_only(&mut self, id: TranscriptionId) {
        self.selection.clear();
        self.selection.insert(id);
    }

    /// Cmd/Ctrl-click: toggle `id`'s membership.
    pub fn toggle_selection(&mut self, id: TranscriptionId) {
        if !self.selection.remove(&id) {
            self.selection.insert(id);
        }
    }

    /// Shift-click: select every row between `from_idx` and `to_idx`
    /// (inclusive, in current display order), in addition to whatever was
    /// already selected — matching Buzz/Qt's extend-selection behavior.
    /// Indices are clamped to the current row count.
    pub fn select_range(&mut self, from_idx: usize, to_idx: usize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let (lo, hi) = if from_idx <= to_idx {
            (from_idx, to_idx)
        } else {
            (to_idx, from_idx)
        };
        for row in &self.rows[lo.min(last)..=hi.min(last)] {
            self.selection.insert(row.id);
        }
    }

    /// Clears the selection entirely.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Re-reads `id`'s row from `store` and replaces it in place (same
    /// mechanics as the `Started`/terminal-event handler
    /// [`Self::refresh_from_store`], exposed publicly) — for GUI actions
    /// (Rename/Notes saves) that mutate a row outside the task-event stream,
    /// so the table reflects the new name/notes immediately rather than
    /// waiting for the next unrelated event.
    pub fn refresh_row(&mut self, id: TranscriptionId, store: &SqliteStore) {
        self.refresh_from_store(id, store);
    }

    /// Removes every id in `ids` from both the row list and the current
    /// selection — for Clear History, which deletes rows directly via
    /// [`SqliteStore::delete_tasks`] rather than through the task-event
    /// stream (a deleted row has no terminal event of its own to react to).
    pub fn remove(&mut self, ids: &[TranscriptionId]) {
        self.rows.retain(|row| !ids.contains(&row.id));
        self.selection.retain(|id| !ids.contains(id));
    }

    /// The lifecycle statuses of every currently-selected row — for gating
    /// toolbar/menu actions (e.g. "Cancel" enabled only when every selection
    /// member is queued/in-progress).
    pub fn selected_statuses(&self) -> Vec<Status> {
        self.rows
            .iter()
            .filter(|row| self.selection.contains(&row.id))
            .map(|row| row.status)
            .collect()
    }

    /// The selected rows' file path (else URL, else skipped), newline-joined
    /// in current display order — for the ⌘C clipboard shortcut. Deviates
    /// from Buzz's own `copy_selected_fields` (`f"{file}{url}\n"`, which
    /// concatenates *both* raw values, one of which is the literal text
    /// `"None"` when absent, since PyQt's `None` stringifies that way): humm
    /// copies whichever of file/url actually identifies the row, never the
    /// literal word "None" (buzz-notes-shell-table-actions.md §F-003:
    /// "humm: copy path-or-url sanely (micro-deviation)").
    pub fn copy_text(&self) -> String {
        self.rows
            .iter()
            .filter(|row| self.selection.contains(&row.id))
            .filter_map(|row| {
                row.file
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .or_else(|| row.url.clone())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Re-sorts `self.rows` in place per `self.sort`. Called after every
    /// mutation that can affect sort order (a fresh insert, a sort-key
    /// change, or a mutated field the current sort key reads) — simplest
    /// correct approach at this milestone's scale (personal task history,
    /// not a bulk-import workload).
    fn resort(&mut self) {
        #[cfg(test)]
        {
            self.resort_calls += 1;
        }
        let (col, descending) = self.sort;
        self.rows.sort_by(|a, b| {
            let ordering = sort_key(a, col).cmp(&sort_key(b, col));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    /// Finds `id`'s row (if any) and applies `f` to it in place. Skips the
    /// re-sort (carried M3 ticket, spec §8.2) when the active sort column's
    /// [`sort_key`] projection of the row is unchanged by `f` — most events
    /// (e.g. `Progress` under the default `DateAdded` sort) mutate fields the
    /// current sort key doesn't read at all, so re-sorting the whole table on
    /// every one is wasted work. Logs and does nothing for an id with no row
    /// — the "ignore gracefully" contract [`Self::apply`] documents.
    fn mutate(&mut self, id: TranscriptionId, f: impl FnOnce(&mut RowVm)) {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            log::debug!("TasksVm::apply: event for unknown task {id}, ignoring");
            return;
        };

        let col = self.sort.0;
        let before = sort_key(&self.rows[index], col);
        f(&mut self.rows[index]);
        let after = sort_key(&self.rows[index], col);
        if before != after {
            self.resort();
        }
    }

    /// [`TaskEventKind::Queued`]'s handler: reads the new row from `store`
    /// and inserts (or, on a duplicate delivery, replaces) it.
    fn insert_from_store(&mut self, id: TranscriptionId, store: &SqliteStore) {
        match store.get_task(&id) {
            Ok(Some(task_row)) => {
                let row_vm = row_vm_from_task_row(store, task_row);
                match self.rows.iter_mut().find(|row| row.id == id) {
                    Some(existing) => *existing = row_vm,
                    None => self.rows.push(row_vm),
                }
                self.resort();
            }
            Ok(None) => {
                log::debug!("TasksVm::apply(Queued): task {id} not found in store, ignoring");
            }
            Err(err) => {
                log::warn!("TasksVm::apply(Queued): get_task failed for {id}: {err}");
            }
        }
    }

    /// The `Started`/terminal-event handler: re-reads `id`'s row from
    /// `store` and replaces it wholesale (simpler and no less correct than
    /// patching individual fields, at this milestone's scale).
    fn refresh_from_store(&mut self, id: TranscriptionId, store: &SqliteStore) {
        match store.get_task(&id) {
            Ok(Some(task_row)) => {
                let row_vm = row_vm_from_task_row(store, task_row);
                match self.rows.iter_mut().find(|row| row.id == id) {
                    Some(existing) => *existing = row_vm,
                    None => {
                        log::debug!("TasksVm::apply: refresh for untracked task {id}, inserting");
                        self.rows.push(row_vm);
                    }
                }
                self.resort();
            }
            Ok(None) => {
                log::debug!("TasksVm::apply: refresh for task {id} not found in store, ignoring");
            }
            Err(err) => {
                log::warn!("TasksVm::apply: get_task failed for {id}: {err}");
            }
        }
    }
}

/// Builds one [`RowVm`] from a [`TaskRow`], resolving the Model/Task columns
/// via `store.load_task_spec` when possible (a humm-created row always has
/// one) and falling back to raw-column, display-only reads
/// (`store.model_display_columns`, `store.spec_from_columns`) for an
/// imported/foreign row whose `options_json` is `NULL`.
fn row_vm_from_task_row(store: &SqliteStore, row: TaskRow) -> RowVm {
    let (model_text, task_text) = model_and_task_text(store, &row.id);
    RowVm {
        id: row.id,
        display_name: display_name_for(&row),
        model_text,
        task_text,
        status: row.status,
        progress: row.progress as f32,
        phase: Phase::None,
        error: row.error_message,
        time_queued: row.time_queued,
        time_started: row.time_started,
        time_ended: row.time_ended,
        notes: row.notes,
        file: row.file,
        url: row.url,
    }
}

/// File Name / URL column text: `name` else the file's basename else `url`
/// else empty — matching Python's `record.name or basename(file) or url or
/// ""` (an empty `name`/`url` string counts as absent, same as `None`).
///
/// `pub(crate)`: also the Speaker-ID window's title fallback
/// (`speaker_id::SpeakerIdVm::open`) for a row with no file — e.g. the
/// `MissingAudio` gate itself, where an empty OS window title would
/// otherwise be a real, reachable UX gap (unlike the viewer's own
/// `file_basename`-only title, whose empty-string case is dead code in
/// practice — the viewer only ever opens for a `Completed`/`Skipped` row,
/// which always has a file).
pub(crate) fn display_name_for(row: &TaskRow) -> String {
    if let Some(name) = row.name.as_deref().filter(|s| !s.is_empty()) {
        return name.to_string();
    }
    if let Some(file) = &row.file
        && let Some(base) = file.file_name().and_then(|s| s.to_str())
    {
        return base.to_string();
    }
    if let Some(url) = row.url.as_deref().filter(|s| !s.is_empty()) {
        return url.to_string();
    }
    String::new()
}

/// The Task column's two Buzz-parity labels.
fn task_label(task: TaskKind) -> String {
    match task {
        TaskKind::Transcribe => tr("task-transcribe"),
        TaskKind::Translate => tr("task-translate"),
    }
}

/// The Model column's text for a persisted [`ModelSpec`]: `"Hugging Face
/// (<repo>)"` if an HF repo was selected, else `"Whisper.cpp (<size>)"`
/// (`ModelSize::as_str`, e.g. `"small"`) — matching what `create_task` writes
/// to the `model_type` column and Buzz's own display shape.
fn model_text_from_spec(model: &ModelSpec) -> String {
    match &model.hf_repo {
        Some(repo) => format!("Hugging Face ({repo})"),
        None => format!("Whisper.cpp ({})", model.size.as_str()),
    }
}

/// Resolves the Model/Task column text for task `id`: the persisted
/// [`humm_core::domain::TaskSpec`] when one exists (every humm-created row),
/// or a tolerant, display-only fallback built from raw columns (an
/// imported/foreign row with `options_json` `NULL`).
fn model_and_task_text(store: &SqliteStore, id: &TranscriptionId) -> (String, String) {
    match store.load_task_spec(id) {
        Ok(Some(spec)) => (
            model_text_from_spec(&spec.options.model),
            task_label(spec.options.task),
        ),
        Ok(None) => (
            model_text_fallback(store, id),
            task_text_fallback(store, id),
        ),
        Err(err) => {
            log::debug!(
                "model_and_task_text: load_task_spec failed for {id} (display fallback): {err}"
            );
            (String::new(), task_label(TaskKind::Transcribe))
        }
    }
}

/// Display-only Model column fallback for a row with no persisted
/// `TaskSpec`: the raw `model_type`/`whisper_model_size` columns, verbatim,
/// joined as `"<model_type> (<size>)"` when both are present.
fn model_text_fallback(store: &SqliteStore, id: &TranscriptionId) -> String {
    match store.model_display_columns(id) {
        Ok(Some((Some(model_type), Some(size)))) => format!("{model_type} ({size})"),
        Ok(Some((Some(model_type), None))) => model_type,
        Ok(Some((None, Some(size)))) => size,
        Ok(Some((None, None))) | Ok(None) => String::new(),
        Err(err) => {
            log::debug!("model_text_fallback: model_display_columns failed for {id}: {err}");
            String::new()
        }
    }
}

/// Display-only Task column fallback for a row with no persisted
/// `TaskSpec`: `spec_from_columns`'s own tolerant `task` parsing (already
/// defaults to `Transcribe` for an absent/unparseable column — Buzz's own
/// restart fallback).
fn task_text_fallback(store: &SqliteStore, id: &TranscriptionId) -> String {
    match store.spec_from_columns(id) {
        Ok(Some(spec)) => task_label(spec.options.task),
        Ok(None) => task_label(TaskKind::Transcribe),
        Err(err) => {
            log::debug!("task_text_fallback: spec_from_columns failed for {id}: {err}");
            task_label(TaskKind::Transcribe)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::boundary::TaskStore;
    use humm_core::domain::{
        Backend, DEFAULT_TEMPERATURE, Input, ModelSize, OutputSpec, Source, TaskSpec,
        TranscriptionOptions,
    };
    use humm_core::queue::TaskEventKind;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn sample_spec(input: Input) -> TaskSpec {
        TaskSpec {
            input,
            options: TranscriptionOptions {
                language: None,
                task: TaskKind::Transcribe,
                model: ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                word_timestamps: false,
                initial_prompt: String::new(),
                temperature: DEFAULT_TEMPERATURE.to_vec(),
                llm: None,
                extract_speech: false,
            },
            output: OutputSpec::default(),
        }
    }

    // -- ColumnId -----------------------------------------------------------

    #[test]
    fn column_id_round_trips_through_id_and_from_id() {
        for col in ColumnId::all() {
            assert_eq!(ColumnId::from_id(col.id()), Some(col));
        }
    }

    #[test]
    fn column_id_from_id_unknown_string_is_none() {
        assert_eq!(ColumnId::from_id("not-a-column"), None);
    }

    #[test]
    fn only_file_name_is_not_hideable() {
        for col in ColumnId::all() {
            assert_eq!(col.hideable(), col != ColumnId::FileName);
        }
    }

    // -- TasksVm::load / row construction ------------------------------------

    #[test]
    fn load_builds_model_and_task_text_from_persisted_spec() {
        // This test never calls `set_locale` itself, but it does assert on
        // translated text (`task_text`/`model_text` are built via `tr`), so
        // it can still observe another concurrently running test's locale
        // switch mid-flight without this guard -- `lock_locale_for_test`'s
        // own doc comment now says so explicitly.
        let _guard = crate::i18n::lock_locale_for_test();
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_spec(Input::Path(PathBuf::from("/tmp/clip.wav")));
        spec.options.task = TaskKind::Translate;
        spec.options.model = ModelSpec {
            backend: Backend::Whisper,
            size: ModelSize::Small,
            hf_repo: None,
        };
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        let vm = TasksVm::load(&store);
        assert_eq!(vm.sorted_rows().len(), 1);
        let row = &vm.sorted_rows()[0];
        assert_eq!(row.model_text, "Whisper.cpp (small)");
        assert_eq!(row.task_text, "Translate to English");
        assert_eq!(row.display_name, "clip.wav");
    }

    #[test]
    fn load_uses_hugging_face_model_text_when_hf_repo_present() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_spec(Input::Path(PathBuf::from("/tmp/clip.wav")));
        spec.options.model = ModelSpec {
            backend: Backend::Whisper,
            size: ModelSize::Small,
            hf_repo: Some("openai/whisper-large-v3-turbo".to_string()),
        };
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        let vm = TasksVm::load(&store);
        assert_eq!(
            vm.sorted_rows()[0].model_text,
            "Hugging Face (openai/whisper-large-v3-turbo)"
        );
    }

    /// Minimal Buzz schema (verbatim subset of
    /// `crates/humm-store/tests/fixtures/buzz_schema.sql`, itself copied from
    /// Buzz's own `buzz/schema.sql`) — just enough for
    /// `SqliteStore::import_buzz_db` to attach and copy from. Inlined here
    /// (rather than `include_str!`-ing the other crate's test fixture)
    /// because a cross-crate `tests/fixtures` path is a needless coupling
    /// for one small, stable schema string.
    const BUZZ_FIXTURE_SCHEMA: &str = "
        CREATE TABLE transcription (
            id TEXT PRIMARY KEY,
            error_message TEXT,
            export_formats TEXT,
            file TEXT,
            output_folder TEXT,
            progress DOUBLE PRECISION DEFAULT 0.0,
            language TEXT,
            model_type TEXT,
            source TEXT,
            status TEXT,
            task TEXT,
            time_ended TIMESTAMP,
            time_queued TIMESTAMP NOT NULL,
            time_started TIMESTAMP,
            url TEXT,
            whisper_model_size TEXT,
            hugging_face_model_id TEXT,
            word_level_timings BOOLEAN DEFAULT FALSE,
            extract_speech BOOLEAN DEFAULT FALSE,
            name TEXT,
            notes TEXT
        );
        CREATE TABLE transcription_segment (
            id INTEGER PRIMARY KEY,
            end_time INT DEFAULT 0,
            start_time INT DEFAULT 0,
            text TEXT NOT NULL,
            translation TEXT DEFAULT '',
            transcription_id TEXT,
            FOREIGN KEY (transcription_id) REFERENCES transcription(id) ON DELETE CASCADE
        );
    ";

    /// Builds a one-row Buzz-shaped fixture database at `path` and imports it
    /// into `store` via the real `import_buzz_db` path — so the resulting row
    /// has a genuinely `NULL` `options_json` (import never sets it), exactly
    /// like a real user's migrated Buzz history, rather than a hand-faked
    /// shortcut.
    fn import_one_foreign_row(store: &SqliteStore, path: &std::path::Path, id: &str) {
        let conn = rusqlite::Connection::open(path).expect("open fixture db");
        conn.execute_batch(BUZZ_FIXTURE_SCHEMA)
            .expect("apply buzz schema");
        conn.execute(
            "INSERT INTO transcription (
                id, file, model_type, source, status, task, time_queued,
                whisper_model_size, word_level_timings, extract_speech
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                "/buzz/foreign.wav",
                "Whisper.cpp",
                "file_import",
                "completed",
                "not-a-real-task",
                "2024-01-01T10:00:00.000000",
                "small",
                true,
                false,
            ],
        )
        .expect("insert foreign row");
        drop(conn);

        store.import_buzz_db(path).expect("import_buzz_db");
    }

    #[test]
    fn load_falls_back_to_raw_columns_for_an_imported_row_with_null_options_json() {
        let store = SqliteStore::open_in_memory().expect("open");
        let dir = tempfile::tempdir().expect("tempdir");
        let fixture_path = dir.path().join("Buzz.sqlite");
        let id = "11111111-1111-1111-1111-111111111111";
        import_one_foreign_row(&store, &fixture_path, id);

        // Confirm the premise: options_json really is NULL for the imported
        // row, so this test exercises the fallback path, not load_task_spec.
        let parsed_id = TranscriptionId::parse(id).expect("valid uuid");
        assert_eq!(store.load_task_spec(&parsed_id).expect("load"), None);

        let vm = TasksVm::load(&store);
        assert_eq!(vm.sorted_rows().len(), 1);
        let row = &vm.sorted_rows()[0];
        assert_eq!(row.model_text, "Whisper.cpp (small)");
        // spec_from_columns falls back to Transcribe for an unparseable task.
        assert_eq!(row.task_text, "Transcribe");
        assert_eq!(row.display_name, "foreign.wav");
    }

    // -- TasksVm::apply -------------------------------------------------------

    #[test]
    fn apply_queued_inserts_row_from_store() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        assert!(vm.sorted_rows().is_empty());

        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );

        assert_eq!(vm.sorted_rows().len(), 1);
        assert_eq!(vm.sorted_rows()[0].id, id);
        assert_eq!(vm.sorted_rows()[0].status, Status::Queued);
    }

    #[test]
    fn apply_queued_for_unknown_id_is_ignored_gracefully() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();

        vm.apply(
            &TaskEvent {
                task_id: TranscriptionId::new(),
                kind: TaskEventKind::Queued,
            },
            &store,
        );

        assert!(vm.sorted_rows().is_empty());
    }

    #[test]
    fn apply_progress_mutates_in_place_without_reading_the_store() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );

        // Delete the underlying row so any store *read* in the Progress
        // branch would fail to find it — proving the mutation below is
        // genuinely in-place and doesn't depend on the store at all.
        store.delete_tasks(&[id]).expect("delete");

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Progress(0.42),
            },
            &store,
        );

        assert_eq!(vm.sorted_rows().len(), 1);
        let row = &vm.sorted_rows()[0];
        assert_eq!(row.status, Status::InProgress);
        assert_eq!(row.progress, 0.42);
        assert_eq!(row.phase, Phase::None);
    }

    #[test]
    fn apply_progress_for_unknown_id_is_ignored_gracefully() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();

        vm.apply(
            &TaskEvent {
                task_id: TranscriptionId::new(),
                kind: TaskEventKind::Progress(0.5),
            },
            &store,
        );

        assert!(vm.sorted_rows().is_empty());
    }

    #[test]
    fn apply_model_download_progress_sets_phase_without_reading_the_store() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );
        store.delete_tasks(&[id]).expect("delete");

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::ModelDownloadProgress {
                    file: "ggml-small.bin".to_string(),
                    downloaded: 50,
                    total: Some(200),
                },
            },
            &store,
        );

        assert_eq!(vm.sorted_rows()[0].phase, Phase::Model(Some(0.25)));
        assert_eq!(vm.sorted_rows()[0].status, Status::InProgress);
    }

    #[test]
    fn apply_model_download_progress_without_total_is_phase_model_none() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::ModelDownloadProgress {
                    file: "ggml-small.bin".to_string(),
                    downloaded: 50,
                    total: None,
                },
            },
            &store,
        );

        assert_eq!(vm.sorted_rows()[0].phase, Phase::Model(None));
    }

    #[test]
    fn apply_download_progress_sets_media_phase() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Url(
            url::Url::parse("https://example.com/a.mp3").expect("url"),
        ));
        store
            .create_task(&id, &spec, Source::UrlImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::DownloadProgress(0.6),
            },
            &store,
        );

        assert_eq!(vm.sorted_rows()[0].phase, Phase::Media(0.6));
    }

    #[test]
    fn apply_phase_changed_sets_isolating_phase() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );
        // Delete the underlying row, same as `apply_progress_mutates_in_
        // place_without_reading_the_store` above — proving this is an
        // in-place mutation, not a store read.
        store.delete_tasks(&[id]).expect("delete");

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::PhaseChanged {
                    phase: TaskPhase::IsolatingSpeech,
                },
            },
            &store,
        );

        assert_eq!(vm.sorted_rows()[0].phase, Phase::Isolating);
        assert_eq!(vm.sorted_rows()[0].status, Status::InProgress);
    }

    #[test]
    fn apply_progress_clears_a_previously_set_phase() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::ModelDownloadProgress {
                    file: "ggml-small.bin".to_string(),
                    downloaded: 1,
                    total: Some(2),
                },
            },
            &store,
        );
        assert_ne!(vm.sorted_rows()[0].phase, Phase::None);

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Progress(0.1),
            },
            &store,
        );

        assert_eq!(vm.sorted_rows()[0].phase, Phase::None);
    }

    #[test]
    fn apply_completed_refreshes_row_from_store() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );

        // The Completed *event* carries no timestamps/status by itself —
        // only the store write (already performed by the runner, ahead of
        // the event, per queue.rs's own ordering contract) has them. Mutate
        // the store directly here to stand in for that runner-side write.
        store.mark_completed(&id, &[]).expect("mark completed");

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Completed { written: vec![] },
            },
            &store,
        );

        let row = &vm.sorted_rows()[0];
        assert_eq!(row.status, Status::Completed);
        assert!(row.time_ended.is_some());
    }

    #[test]
    fn apply_failed_refreshes_row_from_store() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );
        store.mark_failed(&id, "boom").expect("mark failed");

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Failed {
                    message: "boom".to_string(),
                },
            },
            &store,
        );

        let row = &vm.sorted_rows()[0];
        assert_eq!(row.status, Status::Failed);
        assert_eq!(row.error.as_deref(), Some("boom"));
    }

    #[test]
    fn apply_canceled_refreshes_row_from_store() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );
        store.mark_canceled(&id).expect("mark canceled");

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Canceled,
            },
            &store,
        );

        assert_eq!(vm.sorted_rows()[0].status, Status::Canceled);
    }

    #[test]
    fn apply_store_write_failed_is_a_pure_log_noop() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::Queued,
            },
            &store,
        );
        let before = vm.sorted_rows()[0].clone();

        vm.apply(
            &TaskEvent {
                task_id: id,
                kind: TaskEventKind::StoreWriteFailed {
                    operation: "mark_started".to_string(),
                    detail: "disk full".to_string(),
                },
            },
            &store,
        );

        assert_eq!(vm.sorted_rows()[0], before);
    }

    // -- sort -----------------------------------------------------------------

    fn vm_with_rows(rows: Vec<RowVm>) -> TasksVm {
        let mut vm = TasksVm {
            rows,
            ..TasksVm::default()
        };
        vm.resort();
        vm
    }

    fn row_for_sort(name: &str, status: Status, queued: &str, ended: &str) -> RowVm {
        RowVm {
            id: TranscriptionId::new(),
            display_name: name.to_string(),
            model_text: String::new(),
            task_text: String::new(),
            status,
            progress: 0.0,
            phase: Phase::None,
            error: None,
            time_queued: queued.to_string(),
            time_started: None,
            time_ended: if ended.is_empty() {
                None
            } else {
                Some(ended.to_string())
            },
            notes: None,
            file: None,
            url: None,
        }
    }

    #[test]
    fn default_sort_is_date_added_descending() {
        let vm = TasksVm::default();
        assert_eq!(vm.sort(), (ColumnId::DateAdded, true));
    }

    #[test]
    fn set_sort_new_column_sorts_ascending_then_toggles_on_repeat_click() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("Bravo", Status::Queued, "2026-01-01T00:00:00", ""),
            row_for_sort("alpha", Status::Queued, "2026-01-02T00:00:00", ""),
        ]);

        vm.set_sort(ColumnId::FileName);
        assert_eq!(vm.sort(), (ColumnId::FileName, false));
        assert_eq!(vm.sorted_rows()[0].display_name, "alpha");
        assert_eq!(vm.sorted_rows()[1].display_name, "Bravo");

        vm.set_sort(ColumnId::FileName);
        assert_eq!(vm.sort(), (ColumnId::FileName, true));
        assert_eq!(vm.sorted_rows()[0].display_name, "Bravo");
        assert_eq!(vm.sorted_rows()[1].display_name, "alpha");
    }

    #[test]
    fn sort_by_file_name_is_case_insensitive() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("banana", Status::Queued, "2026-01-01T00:00:00", ""),
            row_for_sort("Apple", Status::Queued, "2026-01-02T00:00:00", ""),
        ]);
        vm.set_sort_exact(ColumnId::FileName, false);
        assert_eq!(vm.sorted_rows()[0].display_name, "Apple");
        assert_eq!(vm.sorted_rows()[1].display_name, "banana");
    }

    #[test]
    fn sort_by_date_added_uses_iso_ordering() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("first", Status::Queued, "2026-01-05T00:00:00", ""),
            row_for_sort("second", Status::Queued, "2026-01-01T00:00:00", ""),
        ]);
        vm.set_sort_exact(ColumnId::DateAdded, false);
        assert_eq!(vm.sorted_rows()[0].display_name, "second");
        assert_eq!(vm.sorted_rows()[1].display_name, "first");
    }

    #[test]
    fn sort_by_date_completed_treats_missing_as_earliest() {
        let mut vm = vm_with_rows(vec![
            row_for_sort(
                "done",
                Status::Completed,
                "2026-01-01T00:00:00",
                "2026-01-02T00:00:00",
            ),
            row_for_sort("pending", Status::Queued, "2026-01-01T00:00:00", ""),
        ]);
        vm.set_sort_exact(ColumnId::DateCompleted, false);
        assert_eq!(vm.sorted_rows()[0].display_name, "pending");
        assert_eq!(vm.sorted_rows()[1].display_name, "done");
    }

    #[test]
    fn sort_by_status_uses_wire_string() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("a", Status::Queued, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Canceled, "2026-01-01T00:00:00", ""),
        ]);
        vm.set_sort_exact(ColumnId::Status, false);
        // "canceled" < "queued" lexically.
        assert_eq!(vm.sorted_rows()[0].status, Status::Canceled);
        assert_eq!(vm.sorted_rows()[1].status, Status::Queued);
    }

    // -- mutate resort-skip (carried M3 ticket, spec §8.2) -----------------

    /// Sorted by `DateAdded`, whose `sort_key` reads `time_queued` — a field
    /// `mutate`'s status/progress mutation below never touches — so the
    /// resort-skip should keep `resort()` from running at all. Observed via
    /// the `#[cfg(test)]` `resort_calls` counter (the "factor resort into a
    /// fn and count invocations" option from the task brief).
    #[test]
    fn mutate_skips_resort_when_sort_key_unchanged() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("a", Status::Queued, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Queued, "2026-01-02T00:00:00", ""),
        ]);
        vm.set_sort_exact(ColumnId::DateAdded, false);
        let id = vm.sorted_rows()[0].id;
        let resort_calls_before = vm.resort_calls;

        vm.mutate(id, |row| {
            row.status = Status::InProgress;
            row.progress = 0.5;
        });

        assert_eq!(
            vm.resort_calls, resort_calls_before,
            "DateAdded's sort key is untouched by a status/progress mutation; resort() must \
             not run"
        );
        assert_eq!(vm.sorted_rows()[0].status, Status::InProgress);
    }

    /// Sorted by `Status`: mutating the queued row's status to `Canceled`
    /// (which sorts before `Failed`) both moves it to the front and — the
    /// other half of the same guarantee — must actually invoke `resort()`
    /// (the skip must not become an accidental "never resort").
    #[test]
    fn mutate_resorts_when_key_changes() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("a", Status::Failed, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Queued, "2026-01-02T00:00:00", ""),
        ]);
        vm.set_sort_exact(ColumnId::Status, false);
        assert_eq!(vm.sorted_rows()[0].status, Status::Failed);
        let queued_id = vm.sorted_rows()[1].id;
        let resort_calls_before = vm.resort_calls;

        vm.mutate(queued_id, |row| {
            row.status = Status::Canceled;
        });

        assert!(
            vm.resort_calls > resort_calls_before,
            "Status's sort key changed; resort() must run"
        );
        assert_eq!(vm.sorted_rows()[0].status, Status::Canceled);
    }

    // -- selection --------------------------------------------------------

    #[test]
    fn select_only_replaces_the_whole_selection() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("a", Status::Queued, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Queued, "2026-01-02T00:00:00", ""),
        ]);
        let first_id = vm.sorted_rows()[0].id;
        let second_id = vm.sorted_rows()[1].id;

        vm.select_only(first_id);
        assert_eq!(vm.selection().len(), 1);
        vm.select_only(second_id);
        assert_eq!(vm.selection(), &BTreeSet::from([second_id]));
    }

    #[test]
    fn toggle_selection_adds_then_removes() {
        let mut vm = vm_with_rows(vec![row_for_sort(
            "a",
            Status::Queued,
            "2026-01-01T00:00:00",
            "",
        )]);
        let id = vm.sorted_rows()[0].id;

        vm.toggle_selection(id);
        assert!(vm.selection().contains(&id));
        vm.toggle_selection(id);
        assert!(!vm.selection().contains(&id));
    }

    #[test]
    fn select_range_selects_every_row_between_the_two_indices_inclusive() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("a", Status::Queued, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Queued, "2026-01-02T00:00:00", ""),
            row_for_sort("c", Status::Queued, "2026-01-03T00:00:00", ""),
        ]);
        vm.set_sort_exact(ColumnId::FileName, false); // deterministic a,b,c order
        let ids: Vec<_> = vm.sorted_rows().iter().map(|r| r.id).collect();

        vm.select_range(0, 2);
        assert_eq!(vm.selection().len(), 3);
        for id in ids {
            assert!(vm.selection().contains(&id));
        }
    }

    #[test]
    fn selected_statuses_reflects_only_selected_rows() {
        let mut vm = vm_with_rows(vec![
            row_for_sort("a", Status::Queued, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Failed, "2026-01-02T00:00:00", ""),
        ]);
        let failed_id = vm
            .sorted_rows()
            .iter()
            .find(|r| r.status == Status::Failed)
            .expect("failed row")
            .id;
        vm.select_only(failed_id);
        assert_eq!(vm.selected_statuses(), vec![Status::Failed]);
    }

    #[test]
    fn copy_text_joins_selected_file_paths_and_urls_newline_separated() {
        let mut rows = vec![
            row_for_sort("a", Status::Completed, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Completed, "2026-01-02T00:00:00", ""),
        ];
        rows[0].file = Some(PathBuf::from("/tmp/a.wav"));
        rows[1].url = Some("https://example.com/b.mp3".to_string());
        let mut vm = vm_with_rows(rows);
        let ids: Vec<_> = vm.sorted_rows().iter().map(|r| r.id).collect();
        for id in &ids {
            vm.toggle_selection(*id);
        }

        let text = vm.copy_text();
        assert!(text.contains("/tmp/a.wav"));
        assert!(text.contains("https://example.com/b.mp3"));
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn refresh_row_picks_up_a_store_side_name_change() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let spec = sample_spec(Input::Path(PathBuf::from("/tmp/a.wav")));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        let mut vm = TasksVm::load(&store);
        assert_eq!(vm.sorted_rows()[0].display_name, "a.wav");

        store.set_task_name(&id, "My Recording").expect("rename");
        vm.refresh_row(id, &store);

        assert_eq!(vm.sorted_rows()[0].display_name, "My Recording");
    }

    #[test]
    fn refresh_row_for_unknown_id_does_not_panic_or_insert() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut vm = TasksVm::default();
        vm.refresh_row(TranscriptionId::new(), &store);
        assert!(vm.sorted_rows().is_empty());
    }

    #[test]
    fn remove_drops_the_given_rows_and_clears_their_selection() {
        let mut rows = vec![
            row_for_sort("a", Status::Completed, "2026-01-01T00:00:00", ""),
            row_for_sort("b", Status::Completed, "2026-01-02T00:00:00", ""),
        ];
        rows.sort_by(|a, b| a.time_queued.cmp(&b.time_queued));
        let mut vm = vm_with_rows(rows);
        let ids: Vec<_> = vm.sorted_rows().iter().map(|r| r.id).collect();
        vm.select_only(ids[0]);
        vm.toggle_selection(ids[1]);
        assert_eq!(vm.selection().len(), 2);

        vm.remove(&[ids[0]]);

        assert_eq!(vm.sorted_rows().len(), 1);
        assert_eq!(vm.sorted_rows()[0].id, ids[1]);
        assert_eq!(vm.selection(), &BTreeSet::from([ids[1]]));
    }

    #[test]
    fn remove_of_unknown_id_is_a_noop() {
        let mut vm = vm_with_rows(vec![row_for_sort(
            "a",
            Status::Completed,
            "2026-01-01T00:00:00",
            "",
        )]);
        vm.remove(&[TranscriptionId::new()]);
        assert_eq!(vm.sorted_rows().len(), 1);
    }

    #[test]
    fn copy_text_excludes_unselected_rows() {
        let mut rows = vec![row_for_sort(
            "a",
            Status::Completed,
            "2026-01-01T00:00:00",
            "",
        )];
        rows[0].file = Some(PathBuf::from("/tmp/a.wav"));
        let vm = vm_with_rows(rows);
        assert_eq!(vm.copy_text(), "");
    }
}
