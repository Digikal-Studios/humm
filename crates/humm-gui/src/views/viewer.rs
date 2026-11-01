//! The transcript-viewer window chrome (F-080): a fully-live toolbar (View
//! menu, Export, Translate, Resize, Identify Speakers, Find), the search bar
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
//! Timestamps mode embeds the real segment editor table
//! ([`crate::views::viewer_table`], F-081) and the synced media player (F-083)
//! in a vertical splitter: the audio/video player row, the playback-controls
//! bar ([`crate::views::playback_controls`]), and the current-segment frame,
//! all driven off the [`ViewerVm`]'s player (the audio master clock). Export
//! is a live menu ([`crate::views::export_menu`], F-084); Resize opens the
//! resizer window (F-086 — the button latches `resize_requested`, which
//! `app.rs` harvests to open/focus the resizer); Identify Speakers opens the
//! Speaker-ID window the same way (F-087, `identify_speakers_requested`,
//! `crate::speaker_id`'s module doc). Translate is live (F-085,
//! Task 15, design spec §7.3): the toolbar button drives [`ViewerVm::
//! on_translate_clicked`]'s Buzz-ported gate order, [`show_translate_modals`]
//! renders whichever gate modal it armed, and [`viewer_ui`] harvests arrived
//! translations every frame. The viewer-level nudge shortcuts
//! (⌘←/⌘→/⌘⇧←/⌘⇧→), the playback shortcuts (⌘P/⌘⇧P/⌘⌥P/⌘G), the
//! "edited · saved" autosave indicator, and the unsaved-edit warning banner
//! are wired here.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use humm_core::domain::Millis;
use humm_core::settings::Settings;
use humm_core::viewer::edit::{Field, NUDGE_MS};

use crate::i18n::tr;
use crate::shortcuts::{ShortcutAction, ShortcutMap};
use crate::viewer::{PlayerState, TranslateModal, ViewMode, ViewerDeps, ViewerVm, time_label};
use crate::views::options_sheet::render_llm_model_prompt_fields;
use crate::views::{export_menu, playback_controls, viewer_search, viewer_table};

/// Repaint cadence while playing (spec §2.5): ~60 Hz so the scrubber,
/// follow-audio highlight, and video frame keep up with the master clock.
const PLAYING_REPAINT: Duration = Duration::from_millis(16);
/// The draggable splitter handle's thickness.
const SPLITTER_HANDLE_H: f32 = 6.0;

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

    // Kick off the background decode on the first frame and finish the player
    // build once it lands (spec §2.5: decode off the UI thread, degrade on
    // failure). Both are cheap no-ops once the player is Ready/Failed.
    vm.ensure_player_started(deps, ui.ctx());
    vm.poll_player_load(deps);
    // Drain any translations the (lazily-spawned) translator has produced
    // since the last frame (design spec §7.3, the `player_rx` pattern) — a
    // no-op before the first Translate click ever spawns a worker.
    vm.harvest_translations(deps);
    // Test-only A/V-drift acceptance driver (`HUMM_SMOKE_OPEN_VIEWER`): starts
    // playback once Ready and closes the window at end-of-media. A no-op on
    // every production path (`smoke_autoplay` defaults false). Runs before the
    // mode dispatch / empty-rows return so it fires regardless of either.
    vm.drive_smoke_playback();

    consume_mode_shortcuts(ui, vm, shortcuts);
    viewer_search::consume_shortcuts(ui, vm, shortcuts);
    // Nudge + playback shortcuts act only inside the focused viewer viewport
    // (design spec §7).
    consume_nudge_shortcuts(ui, vm, deps, shortcuts);
    consume_playback_shortcuts(ui, vm, shortcuts);

    show_toolbar(ui, vm, deps, shortcuts, settings);
    show_translate_modals(ui, vm, deps, settings, settings_path);
    viewer_search::ui(ui, vm, shortcuts);
    persist_find_visibility_if_changed(vm, find_was_visible, settings, settings_path);
    persist_playback_settings_if_changed(vm, settings, settings_path);

    show_unsaved_banner(ui, vm);
    export_menu::show_export_error_banner(ui, vm);
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

/// Persists the playback-related viewer settings (loop / follow / controls
/// visibility / global playback rate / per-kind splitter fractions) whenever the
/// VM's live values diverge from what is stored — the same diff-on-change,
/// non-fatal-on-save-failure contract as the geometry/find persists. Floats are
/// compared bit-exactly (they only ever change to a value the VM computed, not
/// through arithmetic drift), sidestepping `clippy::float_cmp`.
fn persist_playback_settings_if_changed(
    vm: &ViewerVm,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let viewer = &guard.viewer;
    let unchanged = viewer.segment_looping_enabled == vm.segment_looping_enabled
        && viewer.follow_audio_enabled == vm.follow_audio_enabled
        && viewer.playback_controls_visible == vm.playback_controls_visible
        && viewer.playback_rate.to_bits() == vm.playback_rate.to_bits()
        && viewer.audio_splitter_fraction.to_bits() == vm.audio_splitter_fraction.to_bits()
        && viewer.video_splitter_fraction.to_bits() == vm.video_splitter_fraction.to_bits();
    if unchanged {
        return;
    }
    let viewer = &mut guard.viewer;
    viewer.segment_looping_enabled = vm.segment_looping_enabled;
    viewer.follow_audio_enabled = vm.follow_audio_enabled;
    viewer.playback_controls_visible = vm.playback_controls_visible;
    viewer.playback_rate = vm.playback_rate;
    viewer.audio_splitter_fraction = vm.audio_splitter_fraction;
    viewer.video_splitter_fraction = vm.video_splitter_fraction;
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the viewer playback settings: {err}");
    }
}

/// Consumes the four playback shortcuts from *this* viewport's input (design
/// spec §7): ⌘P play/pause, ⌘⇧P replay segment, ⌘⌥P toggle the controls bar,
/// ⌘G scroll-to-current. Each is a no-op when the player is not ready.
fn consume_playback_shortcuts(ui: &egui::Ui, vm: &mut ViewerVm, shortcuts: &ShortcutMap) {
    let (play_pause, replay, toggle_controls, scroll_to_current) = ui.input_mut(|input| {
        (
            input.consume_shortcut(&shortcuts.get(ShortcutAction::PlayPauseAudio)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ReplayCurrentSegment)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::TogglePlaybackControls)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::ScrollToCurrentText)),
        )
    });
    if play_pause {
        vm.toggle_play();
    }
    if replay {
        vm.replay_current_segment();
    }
    if toggle_controls {
        vm.playback_controls_visible = !vm.playback_controls_visible;
    }
    if scroll_to_current {
        vm.scroll_to_current();
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

/// The toolbar: View menu, Export menu, Translate, Resize, Identify
/// Speakers, and Find — all live. Identify Speakers (F-087) is the last of
/// these to go live: its own window's `MissingAudio` gate handles the "no
/// audio file" case that Buzz's toolbar-button-disable did
/// (`transcription_viewer_widget.py:514-518`), so the toolbar button here is
/// unconditionally enabled — see `speaker_id`'s module doc for why the gate
/// belongs to the window, not this click.
fn show_toolbar(
    ui: &mut egui::Ui,
    vm: &mut ViewerVm,
    deps: &ViewerDeps,
    shortcuts: &ShortcutMap,
    settings: &Mutex<Settings>,
) {
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
        export_menu::toolbar_menu(ui, vm, deps);
        // Translate (F-085, design spec §7.3): a click runs the Buzz-ported
        // gate order (`ViewerVm::on_translate_clicked`) — never blocking, the
        // click only arms a modal or enqueues onto the (lazily-spawned)
        // translator. `settings` is snapshotted here, before the call, so no
        // lock is held across `on_translate_clicked`'s potential keyring read
        // (the `GuiLlmSource::resolve_with_env` discipline).
        if ui.button(tr("viewer-translate")).clicked() {
            let snapshot = settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            vm.on_translate_clicked(deps, &snapshot, ui.ctx());
        }
        // Resize (F-086): opens this transcript's resizer window. Enabled this
        // task; the click only latches `resize_requested`, which `app.rs`
        // harvests to open/focus the resizer (Buzz `on_resize_button_clicked`).
        if ui.button(tr("viewer-resize")).clicked() {
            super::latch_root_harvested_intent(&mut vm.resize_requested, ui.ctx());
        }
        // Identify Speakers (F-087): opens this transcript's Speaker-ID
        // window. Always enabled — a missing audio file gates the window's
        // own Identify button, not this click (`speaker_id`'s module doc).
        if ui.button(tr("viewer-identify-speakers")).clicked() {
            super::latch_root_harvested_intent(&mut vm.identify_speakers_requested, ui.ctx());
        }
        viewer_search::toolbar_button(ui, vm);
        show_autosave_indicator(ui, vm);
        export_menu::show_export_flash(ui, vm);
    });
}

/// Renders whichever Translate gate modal [`ViewerVm::on_translate_clicked`]
/// armed this frame (F-085, design spec §7.3) — a no-op while
/// `vm.translate_modal` is `None`.
///
/// * [`TranslateModal::ApiKeyRequired`]: Buzz's exact "API Key Required" /
///   "Please enter OpenAI API Key in preferences" info modal
///   (`transcription_viewer_widget.py:1303-1306`) — dismiss is its only
///   action.
/// * [`TranslateModal::Settings`]: the AI-settings modal — the same
///   model-presets/instructions field shapes as the options sheet's LLM
///   block ([`render_llm_model_prompt_fields`], Task 14). OK writes the live
///   buffers back to sticky `settings.file_transcriber` and re-runs the gate
///   (Buzz's dialog-accept → `run_translation`); Cancel discards them. Both
///   close the modal.
fn show_translate_modals(
    ui: &mut egui::Ui,
    vm: &mut ViewerVm,
    deps: &ViewerDeps,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    match &mut vm.translate_modal {
        None => {}
        Some(TranslateModal::ApiKeyRequired) => {
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("humm-viewer-translate-key-modal")).show(
                ui.ctx(),
                |ui| {
                    ui.heading(tr("viewer-translate-key-required-title"));
                    ui.label(tr("viewer-translate-key-required-body"));
                    if ui.button(tr("dialog-ok")).clicked() {
                        close = true;
                    }
                },
            );
            if close || modal.should_close() {
                vm.translate_modal = None;
            }
        }
        Some(TranslateModal::Settings { model, prompt }) => {
            let mut ok_clicked = false;
            let mut cancel_clicked = false;
            let modal = egui::Modal::new(egui::Id::new("humm-viewer-translate-settings-modal"))
                .show(ui.ctx(), |ui| {
                    ui.heading(tr("viewer-translate-settings-title"));
                    render_llm_model_prompt_fields(
                        ui,
                        "humm-viewer-translate-model-presets",
                        model,
                        prompt,
                    );
                    ui.horizontal(|ui| {
                        if ui.button(tr("dialog-ok")).clicked() {
                            ok_clicked = true;
                        }
                        if ui.button(tr("dialog-cancel")).clicked() {
                            cancel_clicked = true;
                        }
                    });
                });
            if ok_clicked {
                let model = model.clone();
                let prompt = prompt.clone();
                let snapshot = {
                    let mut guard = settings
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.file_transcriber.llm_model = Some(model);
                    guard.file_transcriber.llm_prompt = Some(prompt);
                    if let Err(err) = guard.save(settings_path) {
                        log::warn!("could not persist the viewer AI-translation settings: {err}");
                    }
                    guard.clone()
                };
                vm.translate_modal = None;
                // Re-runs the whole gate (Buzz's dialog-accept →
                // `run_translation`): gate 1 (keyring) trivially still
                // passes, gate 2 (configured) now passes with what was just
                // written, so this proceeds straight to the run.
                vm.on_translate_clicked(deps, &snapshot, ui.ctx());
            } else if cancel_clicked || modal.should_close() {
                vm.translate_modal = None;
            }
        }
    }
}

/// The "edited · saved" autosave indicator: flashes for [`SAVED_FLASH_DURATION`]
/// after a successful store write, then clears itself. Visibility is driven off
/// `saved_flash` + `Instant::elapsed`, with `request_repaint_after` so it clears
/// without further input (design spec §2.2 / UX_SPEC J-6).
fn show_autosave_indicator(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    if let Some(text) = vm.timestamp_notice_text() {
        ui.separator();
        ui.label(egui::RichText::new(text).weak());
    }
    let Some(flashed_at) = vm.saved_flash else {
        return;
    };
    let elapsed = flashed_at.elapsed();
    if elapsed >= SAVED_FLASH_DURATION {
        vm.saved_flash = None;
        return;
    }
    ui.separator();
    ui.label(egui::RichText::new(vm.saved_flash_text()).weak());
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

/// The empty-transcript state: a centered explanatory label (UX_SPEC §1).
fn show_empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(tr("viewer-empty"));
    });
}

/// Timestamps mode (F-081 + F-083): the editor table and media player in a
/// vertical splitter, then the playback-controls bar (when visible) and the
/// current-segment frame. The audio player's position is the A/V master clock;
/// this drives the per-frame follow-audio highlight, loop-range refresh, and
/// video-frame selection, and requests a ~60 Hz repaint while playing.
fn show_timestamps(ui: &mut egui::Ui, vm: &mut ViewerVm, deps: &ViewerDeps) {
    // Auto-show the controls bar the frame playback starts (before we reserve
    // its space below).
    vm.update_controls_autoshow();

    // Position-driven per-frame updates (current-segment text + follow-audio).
    // Only while playing — Buzz's position poll runs solely during playback
    // (`_poll_timer` stops when paused), so a paused player never spontaneously
    // populates the frame or moves the selection; a segment click still does.
    if vm.is_playing()
        && let Some(pos) = vm.player_position()
    {
        vm.sync_playback_position(pos);
    }
    // Keep an active loop range following the selected segment (committed and
    // live-in-editor timestamp edits).
    vm.refresh_loop_range();

    let line_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing = ui.spacing().item_spacing.y;

    // Reserve space at the bottom for the controls bar and current-segment
    // frame; the splitter (table + media) fills the rest. These are estimates —
    // egui uses each widget's natural height, so a small mismatch only shifts
    // the split by a few px, self-correcting next frame.
    let controls_reserve = if vm.playback_controls_visible {
        line_height + spacing * 2.0
    } else {
        0.0
    };
    let frame_reserve = if vm.current_segment_text.is_empty() {
        0.0
    } else {
        line_height * 3.0 * 1.25 + spacing * 3.0
    };
    let reserved = controls_reserve + frame_reserve;
    let splitter_h = (ui.available_height() - reserved).max(line_height * 6.0);

    let width = ui.available_width();
    ui.allocate_ui(egui::vec2(width, splitter_h), |ui| {
        show_splitter(ui, vm, deps);
    });

    // Consume a genuine pointer segment-click's side effects (seek-if-outside +
    // loop range) now that the table has applied the selection.
    vm.take_segment_click();

    if vm.playback_controls_visible {
        ui.separator();
        playback_controls::ui(ui, vm);
    }

    show_current_segment_frame(ui, vm);

    // Drift log (env-gated) + the playing repaint cadence.
    if let Some(pos) = vm.player_position() {
        vm.record_drift(pos);
    }
    if vm.is_playing() {
        ui.ctx().request_repaint_after(PLAYING_REPAINT);
    }
}

/// The vertical splitter: the editor table on top, the media pane (video +
/// player row) below, with a draggable handle between them whose fraction
/// (table's share of the height) is persisted per media kind on drag. A simple
/// hand-rolled splitter (egui has no built-in): the handle is a thin
/// drag-sensed strip; dragging adjusts the live fraction, which the
/// diff-on-change persist writes back to settings.
fn show_splitter(ui: &mut egui::Ui, vm: &mut ViewerVm, deps: &ViewerDeps) {
    let is_video = vm.video.is_some();
    let line_height = ui.text_style_height(&egui::TextStyle::Body);
    // The media pane's minimum: a video pane needs room above its controls row.
    let min_media = if is_video { 140.0 } else { line_height * 2.0 };
    let min_table = line_height * 4.0;

    let total = ui.available_height();
    let usable = (total - SPLITTER_HANDLE_H).max(min_table + min_media);
    let fraction = if is_video {
        vm.video_splitter_fraction
    } else {
        vm.audio_splitter_fraction
    };
    let table_h = (usable * fraction).clamp(min_table, usable - min_media);
    let media_h = usable - table_h;
    let width = ui.available_width();

    ui.allocate_ui(egui::vec2(width, table_h), |ui| {
        let _edits = viewer_table::ui(ui, vm, deps);
        // `_edits` (committed Start/End edits) already feed the loop range via
        // `refresh_loop_range`, run each frame in `show_timestamps`.
    });

    // The drag handle.
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, SPLITTER_HANDLE_H), egui::Sense::drag());
    let color = if response.hovered() || response.dragged() {
        ui.visuals().widgets.active.bg_fill
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(2.0, color),
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.dragged() && usable > 0.0 {
        let new_fraction = ((table_h + response.drag_delta().y) / usable).clamp(0.05, 0.95);
        if is_video {
            vm.video_splitter_fraction = new_fraction;
        } else {
            vm.audio_splitter_fraction = new_fraction;
        }
    }

    ui.allocate_ui(egui::vec2(width, media_h.max(min_media)), |ui| {
        show_media(ui, vm);
    });
}

/// The media pane: the video image (when the media is video) above the player
/// row. Audio-only media renders just the player row.
fn show_media(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    if vm.video.is_some() {
        let player_row_h = 44.0;
        let pane_h = (ui.available_height() - player_row_h).max(60.0);
        let width = ui.available_width();
        ui.allocate_ui(egui::vec2(width, pane_h), |ui| show_video_pane(ui, vm));
    }
    show_player_row(ui, vm);
}

/// Renders the video pane: pulls the frame for the current audio position
/// (audio is the master clock), uploads it into the reused texture (or keeps the
/// last frame when none is ready), and draws it black-boxed, aspect-preserved.
fn show_video_pane(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    let pos = vm.player_position().unwrap_or(Millis(0));
    let avail = ui.available_size();
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, egui::Color32::BLACK);

    let Some(video) = &mut vm.video else {
        return;
    };
    if let Some(frame) = video.stream.frame_for(pos) {
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &frame.rgba,
        );
        match &mut video.texture {
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            None => {
                video.texture = Some(ui.ctx().load_texture(
                    "viewer-video",
                    image,
                    egui::TextureOptions::LINEAR,
                ))
            }
        }
        video.last_frame_ts = Some(frame.ts);
    }
    if let Some(texture) = &video.texture {
        let sized = egui::load::SizedTexture::from_handle(texture);
        ui.centered_and_justified(|ui| {
            ui.add(
                egui::Image::new(sized)
                    .maintain_aspect_ratio(true)
                    .max_size(avail),
            );
        });
    }
}

/// The player row: play/pause button, live-seek scrubber, and the
/// `HH:MM:SS / HH:MM:SS` time label. Disabled (with a cause-naming tooltip) when
/// the player is loading or failed (spec §2.1 unhappy path). Intents are
/// collected during render and applied to `vm` after, so the immutable player
/// borrow never conflicts with the mutating seek/toggle calls.
fn show_player_row(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    ui.separator();
    let mut toggle = false;
    let mut seek_to: Option<Millis> = None;
    let mut release_at: Option<Millis> = None;

    match &vm.player {
        PlayerState::Loading => {
            ui.add_enabled(
                false,
                egui::Label::new(egui::RichText::new(tr("viewer-player-loading")).weak()),
            );
        }
        PlayerState::Failed(reason) => {
            ui.horizontal(|ui| {
                ui.add_enabled(false, egui::Button::new("\u{25b6}"));
                ui.label(format!("{}: {reason}", tr("viewer-player-failed")));
            });
        }
        PlayerState::Ready(player) => {
            let duration = player.duration();
            let position = player.position();
            let playing = player.is_playing();
            ui.horizontal(|ui| {
                let icon = if playing { "\u{23f8}" } else { "\u{25b6}" };
                if ui.button(icon).clicked() {
                    toggle = true;
                }
                let mut value = position.0;
                let response = ui.add(
                    egui::Slider::new(&mut value, 0..=duration.0.max(1))
                        .show_value(false)
                        .trailing_fill(true),
                );
                if response.dragged() {
                    // Live drag: seek audio only (see below).
                    seek_to = Some(Millis(value));
                } else if response.changed() {
                    // A discrete click/keyboard change (no drag in flight) is a
                    // one-shot seek — resync the video too.
                    release_at = Some(Millis(value));
                }
                if response.drag_stopped() {
                    release_at = Some(Millis(value));
                }
                ui.label(time_label(position, duration));
            });
        }
    }

    if toggle {
        vm.toggle_play();
    }
    // Live drag seeks only the audio master clock (cheap atomic); the video
    // stream is respawned once on release, not per drag frame.
    if let Some(pos) = seek_to {
        vm.seek_audio(pos);
    }
    if let Some(pos) = release_at {
        vm.seek_playback(pos);
        vm.scrub_release(pos);
    }
}

/// The current-segment frame at the bottom (Timestamps mode only): the text of
/// the segment under the playhead (or the selected segment), up to ~3 lines in
/// a scroll area. Hidden when empty (Buzz `current_segment_frame`).
fn show_current_segment_frame(ui: &mut egui::Ui, vm: &ViewerVm) {
    if vm.current_segment_text.is_empty() {
        return;
    }
    let line_height = ui.text_style_height(&egui::TextStyle::Body);
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("viewer-current-segment-frame")
        .max_height(line_height * 3.0 * 1.25)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.label(&vm.current_segment_text);
        });
}

/// Text / Translation mode: the recomputed combined text in a read-only,
/// selectable, scrollable area (Buzz's read-only `text_display_box`), with
/// the active search match (if any) highlighted and scrolled to
/// (`viewer_search::render_highlighted_text`, F-082).
fn show_text_body(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    viewer_search::render_highlighted_text(ui, vm);
}
