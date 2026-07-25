//! View rendering functions. Each submodule renders one region/window; none
//! own state beyond what's passed in (`ARCHITECTURE.md` §7 — view-models and
//! services hold state, views are pure render functions of it).

pub mod about;
pub mod audio_meter;
pub mod export_menu;
pub mod main_window;
pub mod options_sheet;
pub mod playback_controls;
pub mod preferences;
pub mod prefs_folder_watch;
pub mod prefs_general;
pub mod prefs_shortcuts;
pub mod presentation;
pub mod recording;
pub mod recording_advanced;
pub mod rename_notes;
pub mod resizer;
pub mod secret_field;
pub mod task_table;
pub mod toasts;
pub mod update_notice;
pub mod url_dialog;
pub mod viewer;
pub mod viewer_search;
pub mod viewer_table;
