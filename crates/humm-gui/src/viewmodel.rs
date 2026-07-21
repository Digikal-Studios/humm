//! Task-table view-model skeleton (F-003).
//!
//! `HummApp` does not construct one of these yet — the table view (sorting,
//! selection, per-event row updates) is a later M3 task. This module exists
//! now so its shape (and the module path) is settled before the table lands.

use humm_store::TaskRow;

/// Placeholder for the task-table view-model: cached rows in display order.
/// Fleshed out (sorting, selection, event application per
/// `docs/superpowers/specs/2026-07-21-m3-gui-shell-design.md` §4) once the
/// table view itself is implemented.
#[derive(Debug, Default)]
pub struct TasksVm {
    /// Cached rows, in display order. Empty until a later task populates it
    /// from `SqliteStore::list_tasks` and applies `TaskEvent`s.
    pub rows: Vec<TaskRow>,
}
