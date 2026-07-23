//! The playback-controls bar (F-083, spec §2.1): Loop Segment / Follow Audio
//! toggles, an editable speed combo with `−`/`+` steppers, and a Scroll to
//! Current button. Hidden by default; auto-shown when playback starts in
//! Timestamps mode and toggled with ⌘⌥P (both handled in
//! [`crate::views::viewer`]). This module is a pure render of [`ViewerVm`]'s
//! playback state plus the speed-parsing rules; every mutation routes through a
//! [`ViewerVm`] method so the setting persistence and player calls live in one
//! place.
//!
//! Ported from Buzz's `create_loop_controls` / `on_speed_changed` /
//! `increase_speed` / `decrease_speed`
//! (`transcription_viewer_widget.py:436-528,706-772`).

use egui::RichText;

use crate::i18n::tr;
use crate::viewer::ViewerVm;

/// The speed-combo preset entries (Buzz `speed_combo.addItems`).
const SPEED_PRESETS: [&str; 6] = ["0.5x", "0.75x", "1x", "1.25x", "1.5x", "2x"];
/// The `−`/`+` button step (Buzz `increase_speed`/`decrease_speed`).
const SPEED_STEP: f32 = 0.05;

/// Renders the playback-controls bar into `ui`. Assumes the caller already
/// decided the bar is visible (spec §2.1 auto-show / ⌘⌥P toggle).
pub fn ui(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(tr("viewer-playback-controls")).strong());

        // Loop Segment.
        let mut looping = vm.segment_looping_enabled;
        if ui
            .checkbox(&mut looping, tr("viewer-loop-segment"))
            .on_hover_text(tr("viewer-loop-segment-tooltip"))
            .changed()
        {
            vm.set_loop_enabled(looping);
        }

        // Follow Audio.
        let mut follow = vm.follow_audio_enabled;
        if ui
            .checkbox(&mut follow, tr("viewer-follow-audio"))
            .on_hover_text(tr("viewer-follow-audio-tooltip"))
            .changed()
        {
            vm.follow_audio_enabled = follow;
            // Buzz `on_follow_audio_toggle_changed`: enabling gives immediate
            // feedback by scrolling to the current position.
            if follow {
                vm.scroll_to_current();
            }
        }

        ui.separator();

        // Speed.
        ui.label(RichText::new(tr("viewer-speed")).strong());
        speed_combo(ui, vm);
        if ui.small_button("−").clicked() {
            step_speed(vm, -SPEED_STEP);
        }
        if ui.small_button("+").clicked() {
            step_speed(vm, SPEED_STEP);
        }

        ui.separator();

        // Scroll to Current.
        if ui
            .button(tr("viewer-scroll-to-current"))
            .on_hover_text(tr("viewer-scroll-to-current-tooltip"))
            .clicked()
        {
            vm.scroll_to_current();
        }
    });
}

/// The editable speed combo: a free-text field (committed on focus-out, the
/// custom-URL-field precedent — carried ticket #3) plus a preset dropdown.
fn speed_combo(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    let response = ui.add(egui::TextEdit::singleline(&mut vm.speed_text).desired_width(52.0));
    if response.lost_focus() {
        commit_speed_text(vm);
    }
    ui.menu_button("\u{25be}", |ui| {
        for preset in SPEED_PRESETS {
            if ui.button(preset).clicked() {
                if let Some((value, display)) = apply_speed_input(preset) {
                    vm.set_speed(value, display);
                }
                ui.close();
            }
        }
    });
}

/// Commits the free-text speed buffer (Buzz `on_speed_changed`): apply the
/// parsed+clamped value and its echo, or revert the buffer to the current rate
/// when the text is unparseable.
pub fn commit_speed_text(vm: &mut ViewerVm) {
    match apply_speed_input(&vm.speed_text) {
        Some((value, display)) => vm.set_speed(value, display),
        None => vm.speed_text = format!("{:.2}x", vm.playback_rate),
    }
}

/// Steps the speed by `delta` and echoes the clamped result as `"%.2fx"` (Buzz
/// `set_speed`).
fn step_speed(vm: &mut ViewerVm, delta: f32) {
    let value = (vm.playback_rate + delta).clamp(0.1, 5.0);
    vm.set_speed(value, format!("{value:.2}x"));
}

/// Parses a speed-combo string to a clamped multiplier (Buzz `on_speed_changed`:
/// strip `x`, `float(...)`, clamp `0.1..=5.0`); `None` when unparseable — the
/// caller reverts. Trailing `x` is optional (`"1.5x"`, `"2"`, `"0.75x"` all
/// parse; `"fast"` does not).
pub fn speed_from_text(text: &str) -> Option<f32> {
    let clean: String = text.chars().filter(|&c| c != 'x').collect();
    let value: f32 = clean.trim().parse().ok()?;
    value.is_finite().then(|| value.clamp(0.1, 5.0))
}

/// Parses `raw` and computes its display echo (Buzz's `on_speed_changed` echo
/// rule): a value the caller applies plus the text to show — the raw text when
/// it already ended in `x`, else the clamped value reformatted as `"%.2fx"`.
/// `None` (unparseable) tells the caller to revert.
pub fn apply_speed_input(raw: &str) -> Option<(f32, String)> {
    let value = speed_from_text(raw)?;
    let display = if raw.ends_with('x') {
        raw.to_string()
    } else {
        format!("{value:.2}x")
    };
    Some((value, display))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn speed_from_text_parses_with_and_without_trailing_x() {
        assert_eq!(speed_from_text("1.5x"), Some(1.5));
        assert_eq!(speed_from_text("2"), Some(2.0));
        assert_eq!(speed_from_text("0.75x"), Some(0.75));
        assert_eq!(speed_from_text("  1x "), Some(1.0));
    }

    #[test]
    fn speed_from_text_clamps_to_bounds() {
        assert_eq!(speed_from_text("9"), Some(5.0));
        assert_eq!(speed_from_text("0.01"), Some(0.1));
        assert_eq!(speed_from_text("10x"), Some(5.0));
    }

    #[test]
    fn speed_from_text_rejects_garbage() {
        assert_eq!(speed_from_text("fast"), None);
        assert_eq!(speed_from_text(""), None);
        assert_eq!(speed_from_text("x"), None);
    }

    #[test]
    fn apply_speed_input_echoes_clamped_value_when_no_trailing_x() {
        // No trailing 'x' → reformat as the clamped "%.2fx".
        assert_eq!(apply_speed_input("9"), Some((5.0, "5.00x".to_string())));
        assert_eq!(apply_speed_input("2"), Some((2.0, "2.00x".to_string())));
        assert_eq!(apply_speed_input("0.05"), Some((0.1, "0.10x".to_string())));
    }

    #[test]
    fn apply_speed_input_keeps_raw_text_when_trailing_x_present() {
        // Buzz keeps the user's text verbatim when it already ended with 'x'.
        assert_eq!(apply_speed_input("1.5x"), Some((1.5, "1.5x".to_string())));
        assert_eq!(apply_speed_input("2x"), Some((2.0, "2x".to_string())));
    }

    #[test]
    fn apply_speed_input_rejects_garbage() {
        assert_eq!(apply_speed_input("nope"), None);
    }
}
