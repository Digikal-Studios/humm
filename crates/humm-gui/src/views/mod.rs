//! View rendering functions. Each submodule renders one region/window; none
//! own state beyond what's passed in (`ARCHITECTURE.md` §7 — view-models and
//! services hold state, views are pure render functions of it).

pub mod main_window;
pub mod task_table;
