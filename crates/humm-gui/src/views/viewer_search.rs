//! The transcript search bar (F-082): "Find:" label + input + prev/next
//! buttons + Clear + results label, rendered under the viewer toolbar
//! (design spec §2.3). All matching rules live in `humm_core::viewer::
//! search`; this module only turns egui input (keystrokes, shortcuts, frame
//! timing) into calls against [`ViewerVm::search`]'s plain methods, and
//! renders its state — the mode-specific match application (Timestamps →
//! select+center-scroll the editor table row; Text/Translation → highlight a
//! byte range and scroll to it).
//!
//! Ported from Buzz's `create_search_bar`/`on_search_text_changed`/
//! `update_search_ui`/`clear_search`/`hide_search_bar`/`focus_search_input`/
//! `toggle_search_bar_visibility`/`eventFilter`
//! (`transcription_viewer_widget.py:372-434,774-966,1025-1076`).

use std::time::Instant;

use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Key, Modifiers, TextFormat};

use humm_core::viewer::search::results_label;

use crate::i18n::{tr, tr_args};
use crate::shortcuts::{ShortcutAction, ShortcutMap};
use crate::viewer::{DebounceDecision, SearchResults, ViewerVm, debounce_decision};

/// Extra horizontal room reserved for the scroll area's own scrollbar when
/// pre-laying-out text to estimate a scroll offset (see
/// [`approximate_scroll_offset`]) — the real render happens inside a
/// [`egui::ScrollArea`], whose content width is a few points narrower than
/// `ui.available_width()`.
const SCROLLBAR_ALLOWANCE: f32 = 16.0;

/// The toolbar's Find toggle button: mirrors [`ViewerVm::search`]'s
/// visibility (Buzz `find_button`, a checkable `QToolButton`). Toggling goes
/// through the same [`toggle_visibility`] as ⌘F, so the two stay in sync.
pub fn toolbar_button(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    let button = egui::Button::new(tr("viewer-find")).selected(vm.search.visible);
    if ui.add(button).clicked() {
        toggle_visibility(vm);
    }
}

/// Consumes ⌘F (toggle+focus+select-all), ⌘Return (next) and ⇧Return (prev)
/// from *this* viewport's input (design spec §7: viewer shortcuts act only
/// inside the focused viewer). Independent of the bar's visibility: ⌘F must
/// work to *open* a hidden bar, and Buzz wires next/prev as viewer-scoped
/// `QShortcut`s that are simply no-ops without results
/// (`search_next_if_results`/`search_previous_if_results`) rather than being
/// gated on the bar being shown.
pub fn consume_shortcuts(ui: &egui::Ui, vm: &mut ViewerVm, shortcuts: &ShortcutMap) {
    let (toggle, next, prev) = ui.input_mut(|input| {
        (
            input.consume_shortcut(&shortcuts.get(ShortcutAction::SearchTranscript)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::SearchNext)),
            input.consume_shortcut(&shortcuts.get(ShortcutAction::SearchPrevious)),
        )
    });
    if toggle {
        toggle_visibility(vm);
    }
    if next {
        vm.search_next();
    }
    if prev {
        vm.search_prev();
    }
}

/// Toggles the bar open/closed (Buzz `toggle_search_bar_visibility`): closed
/// → open+focus+select-all ([`SearchState::show`](crate::viewer::SearchState::show));
/// open → hidden+cleared ([`SearchState::hide`](crate::viewer::SearchState::hide)).
fn toggle_visibility(vm: &mut ViewerVm) {
    if vm.search.visible {
        vm.search.hide();
    } else {
        vm.search.show();
    }
}

/// Renders the bar itself — label/input/prev/next/Clear/results label — when
/// [`ViewerVm::search`] is visible; a no-op otherwise. Drives the debounce
/// timer every frame the bar is shown, so a fired search happens even with
/// no further input this frame (`request_repaint_after`).
pub fn ui(ui: &mut egui::Ui, vm: &mut ViewerVm, shortcuts: &ShortcutMap) {
    if !vm.search.visible {
        return;
    }

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("viewer-find-label")).strong());

        let mut output = egui::TextEdit::singleline(&mut vm.search.query)
            .hint_text(tr("viewer-find-placeholder"))
            .desired_width(300.0)
            .show(ui);

        if vm.search.focus_pending {
            output.response.request_focus();
            let end = CCursor::new(vm.search.query.chars().count());
            output
                .state
                .cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
            output.state.store(ui.ctx(), output.response.id);
            vm.search.focus_pending = false;
        }

        if output.response.changed() {
            if vm.search.query.trim().is_empty() {
                // Buzz `on_search_text_changed`'s empty-text branch: stop any
                // pending debounce and clear immediately — no 300ms wait for
                // an empty query.
                vm.search.clear();
            } else {
                vm.search.last_edit = Some(Instant::now());
            }
        }

        // Plain Enter while the input has focus = next match (Buzz
        // `search_input.returnPressed.connect(self.search_next)`, which only
        // fires while the `QLineEdit` itself holds keyboard focus).
        if output.response.has_focus()
            && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter))
        {
            vm.search_next();
        }

        let has_results = !vm.search.results.is_empty();
        let prev_tooltip = tr_args(
            "viewer-find-prev-tooltip",
            &[(
                "shortcut",
                &ui.ctx()
                    .format_shortcut(&shortcuts.get(ShortcutAction::SearchPrevious)),
            )],
        );
        if ui
            .add_enabled(has_results, egui::Button::new("\u{2191}"))
            .on_hover_text(prev_tooltip)
            .clicked()
        {
            vm.search_prev();
        }

        let next_tooltip = tr_args(
            "viewer-find-next-tooltip",
            &[(
                "shortcut",
                &ui.ctx()
                    .format_shortcut(&shortcuts.get(ShortcutAction::SearchNext)),
            )],
        );
        if ui
            .add_enabled(has_results, egui::Button::new("\u{2193}"))
            .on_hover_text(next_tooltip)
            .clicked()
        {
            vm.search_next();
        }

        if ui.button(tr("viewer-find-clear")).clicked() {
            vm.search.clear();
        }

        if vm.search.searched {
            let label = results_label(vm.search.current, vm.search.results.len());
            ui.label(egui::RichText::new(label).weak());
        }
    });

    drive_debounce(ui, vm);
}

/// Checks the pending debounce ([`debounce_decision`]) every frame the bar
/// is shown: reschedules a repaint while waiting, runs the search once the
/// delay has elapsed and the query still has ≥2 trimmed characters, and
/// otherwise (elapsed but too short) simply consumes the pending edit
/// without touching the (possibly stale) existing results — Buzz parity.
fn drive_debounce(ui: &egui::Ui, vm: &mut ViewerVm) {
    let Some(last_edit) = vm.search.last_edit else {
        return;
    };
    let elapsed = Instant::now().saturating_duration_since(last_edit);
    let trimmed_chars = vm.search.query.trim().chars().count();
    match debounce_decision(elapsed, trimmed_chars) {
        DebounceDecision::Pending { remaining } => {
            ui.ctx().request_repaint_after(remaining);
        }
        DebounceDecision::Ready { should_search } => {
            vm.search.last_edit = None;
            if should_search {
                vm.run_search();
            }
        }
    }
}

/// Consumes [`ViewerVm::search`]'s one-shot row-scroll latch for Timestamps
/// mode: when armed and the current result is a row match, selects that row
/// and returns its index for [`crate::views::viewer_table`]'s one-shot
/// `TableBuilder::scroll_to_row`. A no-op (returns `None`) otherwise — in
/// particular, a latch armed by a Text/Translation-mode search is simply
/// left for [`consume_text_scroll`] to pick up instead.
pub fn take_pending_row_scroll(vm: &mut ViewerVm) -> Option<usize> {
    if !vm.search.scroll_pending {
        return None;
    }
    let SearchResults::Rows(rows) = &vm.search.results else {
        return None;
    };
    let row = rows.get(vm.search.current)?.row;
    vm.search.scroll_pending = false;
    vm.selected = Some(row);
    Some(row)
}

/// The current Text/Translation-mode match's byte range, if any — the range
/// [`render_highlighted_text`] paints a background over every frame (not a
/// one-shot: the highlight must stay visible until the match changes or the
/// search is cleared, unlike the scroll, which fires once).
fn current_text_match(vm: &ViewerVm) -> Option<(usize, usize)> {
    let SearchResults::Text(matches) = &vm.search.results else {
        return None;
    };
    matches.get(vm.search.current).map(|m| (m.start, m.end))
}

/// Renders `vm.text_body` (Text/Translation mode) in a read-only, selectable,
/// scrollable area, with the current search match (if any) highlighted via a
/// background-colored [`LayoutJob`] section, and — the one-shot part —
/// scrolls to it the frame a fresh search or navigation lands on it
/// (consuming [`ViewerVm::search`]'s scroll latch).
pub fn render_highlighted_text(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    let highlight = current_text_match(vm);
    let should_scroll = vm.search.scroll_pending && highlight.is_some();

    let mut scroll_area = egui::ScrollArea::vertical().auto_shrink([false, false]);
    if should_scroll
        && let Some((start, _end)) = highlight
        && let Some(offset) = approximate_scroll_offset(ui, &vm.text_body, start)
    {
        scroll_area = scroll_area.vertical_scroll_offset(offset);
    }

    scroll_area.show(ui, |ui| {
        let job = build_layout_job(ui, &vm.text_body, highlight);
        ui.add(egui::Label::new(job).selectable(true));
    });

    if should_scroll {
        vm.search.scroll_pending = false;
    }
}

/// Builds the display [`LayoutJob`] for `text`: plain everywhere, except
/// `highlight` (a byte range, always a valid slice bound — `humm_core::
/// viewer::search::search_text`'s own contract) gets the theme's selection
/// colors as a background, so the match reads as highlighted regardless of
/// light/dark mode.
fn build_layout_job(ui: &egui::Ui, text: &str, highlight: Option<(usize, usize)>) -> LayoutJob {
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let text_color = ui.visuals().text_color();
    let plain = TextFormat {
        font_id: font_id.clone(),
        color: text_color,
        ..Default::default()
    };

    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();

    let Some((start, end)) = highlight else {
        job.append(text, 0.0, plain);
        return job;
    };

    let marked = TextFormat {
        font_id,
        color: ui.visuals().selection.stroke.color,
        background: ui.visuals().selection.bg_fill,
        ..Default::default()
    };
    if start > 0 {
        job.append(&text[..start], 0.0, plain.clone());
    }
    job.append(&text[start..end], 0.0, marked);
    if end < text.len() {
        job.append(&text[end..], 0.0, plain);
    }
    job
}

/// An approximate scroll offset that brings the byte offset `start_byte` in
/// `text` into view, centered in the currently available viewport height.
///
/// Not pixel-exact: it lays `text` out once, up front (at
/// `ui.available_width()` minus [`SCROLLBAR_ALLOWANCE`]), to locate the
/// match's row via [`epaint::Galley::pos_from_cursor`] — a *second* layout
/// pass, independent of the one [`build_layout_job`]/[`egui::ScrollArea`]
/// does for the actual render, whose final wrapped width can differ by a few
/// points (the scrollbar's exact reserved width isn't known until the
/// [`egui::ScrollArea`] itself runs). Design spec §2.3 accepts this
/// approximation explicitly ("an approximate scroll... is acceptable") —
/// only the *highlight* ([`build_layout_job`]) must be byte-exact.
fn approximate_scroll_offset(ui: &egui::Ui, text: &str, start_byte: usize) -> Option<f32> {
    if text.is_empty() {
        return None;
    }
    let font_id = egui::TextStyle::Body.resolve(ui.style());
    let mut job = LayoutJob::single_section(
        text.to_string(),
        TextFormat {
            font_id,
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    job.wrap.max_width = (ui.available_width() - SCROLLBAR_ALLOWANCE).max(1.0);
    let galley = ui.painter().layout_job(job);

    let char_index = text.get(..start_byte)?.chars().count();
    let match_top = galley.pos_from_cursor(CCursor::new(char_index)).top();

    let viewport_height = ui.available_height();
    let content_height = galley.rect.height();
    let max_offset = (content_height - viewport_height).max(0.0);
    Some((match_top - viewport_height / 2.0).clamp(0.0, max_offset))
}
