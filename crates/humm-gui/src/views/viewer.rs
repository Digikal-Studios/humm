//! The transcript-viewer window chrome (F-080): toolbar (View menu + Find
//! live; the rest disabled stubs this task), the search bar
//! ([`crate::views::viewer_search`], F-082), the per-mode central content,
//! and the empty-state label.
//!
//! A pure render function of [`ViewerVm`] — no window/viewport plumbing (that
//! is `app.rs`'s deferred-viewport management), so an `egui_kittest` harness
//! can drive it directly over a bare `egui::Ui` (`Harness::new_ui_state`),
//! exactly as the M3 preferences/models views are tested.
//!
//! Ported from `transcription_viewer_widget.py` (`_setup_toolbar`,
//! `reset_view`) and `transcription_view_mode_tool_button.py` (the View menu).
//! Timestamps mode now embeds the real segment editor table
//! ([`crate::views::viewer_table`], F-081); export and resize remain disabled
//! buttons with tooltips until Tasks 11/12, and the player strip is still the
//! Task 10 stub. The viewer-level nudge shortcuts (⌘←/⌘→/⌘⇧←/⌘⇧→), the
//! "edited · saved" autosave indicator, and the unsaved-edit warning banner
//! are wired here.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use humm_core::settings::Settings;
use humm_core::viewer::edit::{Field, NUDGE_MS};

use crate::i18n::tr;
use crate::shortcuts::{ShortcutAction, ShortcutMap};
use crate::viewer::{PlayerState, ViewMode, ViewerDeps, ViewerVm};
use crate::views::{viewer_search, viewer_table};

/// How long the "edited · saved" autosave indicator stays visible after a
/// successful store write (design spec §2.2: "~1.5 s").
const SAVED_FLASH_DURATION: Duration = Duration::from_millis(1500);

/// Renders one transcript viewer's chrome into `ui`. `settings`/
/// `settings_path` — reserved by Task 7 for splitter/rate persistence, spent
/// for real this task on the find-bar visibility persist.
pub fn viewer_ui(
    ui: &mut egui::Ui,
    vm: &mut ViewerVm,
    deps: &ViewerDeps,
    shortcuts: &ShortcutMap,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    // Snapshotted before *any* call that can mutate `vm.search.visible` this
    // frame (⌘F in `consume_shortcuts`, the toolbar toggle in `show_toolbar`,
    // Esc-in-input in `viewer_search::ui`) — otherwise the diff-on-change
    // persist below would compare against a value already mutated by one of
    // those paths and silently never save (review fix round 1).
    let find_was_visible = vm.search.visible;

    consume_mode_shortcuts(ui, vm, shortcuts);
    viewer_search::consume_shortcuts(ui, vm, shortcuts);
    // Nudge shortcuts act on the selected row and are consumed only inside the
    // focused viewer viewport (design spec §7).
    consume_nudge_shortcuts(ui, vm, deps, shortcuts);

    show_toolbar(ui, vm, shortcuts);
    viewer_search::ui(ui, vm, shortcuts);
    persist_find_visibility_if_changed(vm, find_was_visible, settings, settings_path);

    show_unsaved_banner(ui, vm);
    ui.separator();

    if vm.rows.is_empty() {
        show_empty_state(ui);
        return;
    }

    match vm.mode {
        ViewMode::Timestamps => show_timestamps(ui, vm, deps),
        ViewMode::Text | ViewMode::Translation => show_text_body(ui, vm),
    }
}

/// Persists `vm.search.visible` to `settings.viewer.find_widget_visible` when
/// it changed this frame — the diff-on-change, non-fatal-on-save-failure
/// contract shared by every other viewer setting
/// (`app.rs::persist_viewer_geometry_if_changed`); `settings`/`settings_path`
/// are exactly what Task 7 reserved this parameter slot for.
fn persist_find_visibility_if_changed(
    vm: &ViewerVm,
    was_visible: bool,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    if vm.search.visible == was_visible {
        return;
    }
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.viewer.find_widget_visible == vm.search.visible {
        return;
    }
    guard.viewer.find_widget_visible = vm.search.visible;
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the find-bar visibility: {err}");
    }
}

/// Consumes the four nudge shortcuts (⌘←/⌘→ start ∓/±, ⌘⇧←/⌘⇧→ end ∓/±) from
/// *this* viewport's input, shifting the selected row's Start/End by ±500 ms
/// through the shared store-backed commit rule (floor-zero nudge path). A no-op
/// with nothing selected. Each fires at most once per frame, reading the row's
/// current value fresh (design spec §2.2).
fn consume_nudge_shortcuts(
    ui: &egui::Ui,
    vm: &mut ViewerVm,
    deps: &ViewerDeps,
    shortcuts: &ShortcutMap,
) {
    let (dec_start, inc_start, dec_end, inc_end) = ui.input_mut(|input| {
        (
            input.consume_shortcut(&shortcuts.get(ShortcutAction::DecreaseSegmentStart)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::IncreaseSegmentStart)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::DecreaseSegmentEnd)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::IncreaseSegmentEnd)),
        )
    });
    if dec_start {
        vm.nudge_selected(deps, Field::Start, -NUDGE_MS);
    }
    if inc_start {
        vm.nudge_selected(deps, Field::Start, NUDGE_MS);
    }
    if dec_end {
        vm.nudge_selected(deps, Field::End, -NUDGE_MS);
    }
    if inc_end {
        vm.nudge_selected(deps, Field::End, NUDGE_MS);
    }
}

/// The inline unsaved-edit warning banner: shown when a committed edit's store
/// write failed (the edit stays in memory but could not be persisted —
/// ADR-018). The app's toast stack is unreachable from a deferred viewport, so
/// the viewer carries its own banner (see `viewer.rs`'s module note).
fn show_unsaved_banner(ui: &mut egui::Ui, vm: &ViewerVm) {
    if !vm.unsaved {
        return;
    }
    let color = ui.visuals().warn_fg_color;
    ui.horizontal(|ui| {
        ui.colored_label(color, tr("viewer-unsaved-warning"));
    });
}

/// Consumes the three view-mode shortcuts (⌘E/⌘L/⌘T) — but only from *this*
/// viewport's input, so they act on the focused viewer alone (design spec §7).
/// ⌘L is a no-op without translations (guarded inside [`ViewerVm::set_mode`]).
fn consume_mode_shortcuts(ui: &egui::Ui, vm: &mut ViewerVm, shortcuts: &ShortcutMap) {
    let (text, translation, timestamps) = ui.input_mut(|input| {
        (
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ViewTranscriptText)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ViewTranscriptTranslation)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ViewTranscriptTimestamps)),
        )
    });
    if text {
        vm.set_mode(ViewMode::Text);
    }
    if translation {
        vm.set_mode(ViewMode::Translation);
    }
    if timestamps {
        vm.set_mode(ViewMode::Timestamps);
    }
}

/// The toolbar: View menu (live), Find (live, F-082) then the disabled-stub
/// buttons. The two permanent-tooltip buttons (Translate → M6 API backend,
/// Identify Speakers → M8) name their milestone; the two remaining
/// this-milestone buttons (Export/Resize) get a "lands later this milestone"
/// tooltip until their task enables them (Tasks 11/12).
fn show_toolbar(ui: &mut egui::Ui, vm: &mut ViewerVm, shortcuts: &ShortcutMap) {
    ui.horizontal_wrapped(|ui| {
        ui.menu_button(tr("viewer-view"), |ui| {
            if mode_menu_entry(
                ui,
                tr("viewer-mode-text"),
                shortcuts,
                ShortcutAction::ViewTranscriptText,
            ) {
                vm.set_mode(ViewMode::Text);
                ui.close();
            }
            // Translation entry is visible only when the transcript has
            // translations (Buzz `translation_action.setVisible(has_translation)`).
            if vm.has_translations
                && mode_menu_entry(
                    ui,
                    tr("viewer-mode-translation"),
                    shortcuts,
                    ShortcutAction::ViewTranscriptTranslation,
                )
            {
                vm.set_mode(ViewMode::Translation);
                ui.close();
            }
            if mode_menu_entry(
                ui,
                tr("viewer-mode-timestamps"),
                shortcuts,
                ShortcutAction::ViewTranscriptTimestamps,
            ) {
                vm.set_mode(ViewMode::Timestamps);
                ui.close();
            }
        });
        ui.separator();
        disabled_button(ui, tr("viewer-export"), tr("viewer-export-pending"));
        disabled_button(ui, tr("viewer-translate"), tr("viewer-translate-tooltip"));
        disabled_button(ui, tr("viewer-resize"), tr("viewer-resize-pending"));
        disabled_button(
            ui,
            tr("viewer-identify-speakers"),
            tr("viewer-identify-speakers-tooltip"),
        );
        viewer_search::toolbar_button(ui, vm);
        show_autosave_indicator(ui, vm);
    });
}

/// The "edited · saved" autosave indicator: flashes for [`SAVED_FLASH_DURATION`]
/// after a successful store write, then clears itself. Visibility is driven off
/// `saved_flash` + `Instant::elapsed`, with `request_repaint_after` so it clears
/// without further input (design spec §2.2 / UX_SPEC J-6).
fn show_autosave_indicator(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    let Some(flashed_at) = vm.saved_flash else {
        return;
    };
    let elapsed = flashed_at.elapsed();
    if elapsed >= SAVED_FLASH_DURATION {
        vm.saved_flash = None;
        return;
    }
    ui.separator();
    ui.label(egui::RichText::new(tr("viewer-saved")).weak());
    ui.ctx()
        .request_repaint_after(SAVED_FLASH_DURATION - elapsed);
}

/// One View-menu entry with its shortcut hint; returns whether it was clicked.
fn mode_menu_entry(
    ui: &mut egui::Ui,
    label: String,
    shortcuts: &ShortcutMap,
    action: ShortcutAction,
) -> bool {
    let hint = ui.ctx().format_shortcut(&shortcuts.get(action));
    ui.add(egui::Button::new(label).shortcut_text(hint))
        .clicked()
}

/// A permanently-disabled toolbar button carrying `tooltip` on hover.
fn disabled_button(ui: &mut egui::Ui, label: String, tooltip: String) {
    ui.add_enabled(false, egui::Button::new(label))
        .on_disabled_hover_text(tooltip);
}

/// The empty-transcript state: a centered explanatory label (UX_SPEC §1).
fn show_empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(tr("viewer-empty"));
    });
}

/// Timestamps mode: the real segment editor table (F-081,
/// [`crate::views::viewer_table`]) above the stub player strip. The table is
/// given a fixed height so the player strip keeps its space below; the table
/// itself virtualizes, so a 100k-segment transcript renders only visible rows.
fn show_timestamps(ui: &mut egui::Ui, vm: &mut ViewerVm, deps: &ViewerDeps) {
    let line_height = ui.text_style_height(&egui::TextStyle::Body);
    let available = ui.available_height();
    // Reserve a strip for the Task 10 player below the table.
    let strip = (line_height * 3.0).min(available * 0.35);
    let table_height = (available - strip).max(line_height * 4.0);

    let table_width = ui.available_width();
    let _edits = ui
        .allocate_ui(egui::vec2(table_width, table_height), |ui| {
            viewer_table::ui(ui, vm, deps)
        })
        .inner;
    // Task 10 (playback) consumes `_edits` to keep an active loop range
    // following committed Start/End boundary edits — the documented seam; no
    // player exists yet.

    show_player_strip(ui, vm);
}

/// The stub player strip (Task 10 replaces it with the real audio/video
/// player). While the player is [`PlayerState::Unavailable`] it is an inert,
/// muted placeholder — never a fake "play" affordance.
fn show_player_strip(ui: &mut egui::Ui, vm: &ViewerVm) {
    ui.separator();
    match vm.player {
        PlayerState::Unavailable => {
            ui.add_enabled(
                false,
                egui::Label::new(egui::RichText::new(tr("viewer-player-pending")).weak()),
            );
        }
    }
}

/// Text / Translation mode: the recomputed combined text in a read-only,
/// selectable, scrollable area (Buzz's read-only `text_display_box`), with
/// the active search match (if any) highlighted and scrolled to
/// (`viewer_search::render_highlighted_text`, F-082).
fn show_text_body(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    viewer_search::render_highlighted_text(ui, vm);
}
