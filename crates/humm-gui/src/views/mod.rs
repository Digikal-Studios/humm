//! View rendering functions. Each submodule renders one region/window; none
//! own state beyond what's passed in (`ARCHITECTURE.md` §7 — view-models and
//! services hold state, views are pure render functions of it).

pub mod about;
pub mod main_window;
pub mod options_sheet;
pub mod preferences;
pub mod rename_notes;
pub mod task_table;
pub mod toasts;
pub mod update_notice;
pub mod url_dialog;
