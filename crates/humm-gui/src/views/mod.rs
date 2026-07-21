//! View rendering functions. Each submodule renders one region/window; none
//! own state beyond what's passed in (`ARCHITECTURE.md` §7 — view-models and
//! services hold state, views are pure render functions of it).

pub mod main_window;
pub mod options_sheet;
pub mod rename_notes;
pub mod task_table;
pub mod toasts;
pub mod url_dialog;
