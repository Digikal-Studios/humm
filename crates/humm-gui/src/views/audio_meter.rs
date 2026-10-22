//! The live-recording audio meter (F-060, spec §2.5): a custom-painted
//! widget ported **exactly** from `buzz/widgets/audio_meter_widget.py`'s
//! paint math — bar count/positions, the activation threshold, the two
//! theme palettes, and the caption text.
//!
//! Buzz's widget owns three pieces of state directly (`current_amplitude`,
//! `average_amplitude`, `queue_size`), fed by Qt signals. humm's equivalent
//! data lives in [`humm_core::boundary::MeterSnapshot`] (shared with the
//! capture adapter / session thread, read lock-free) for `average_rms`/
//! `queue_len`, but `current_amplitude`'s `SMOOTHING_FACTOR = 0.95` decay
//! (Buzz `update_amplitude`) is purely a *display* concern with no other
//! reader, so it lives on the caller's own state (`RecordingVm::meter_current_amplitude`)
//! and is threaded in here as `current_amplitude` each repaint — this
//! module smooths it, the caller only owns storage.
//!
//! All the paint math below is a pure, `egui`-free function
//! ([`bar_active`]/[`n_bars_in_half`]/[`smoothed_amplitude`]/[`meter_caption`])
//! so the bar-activation/smoothing/caption logic is directly unit-testable
//! without a rendering context; [`audio_meter`] is the thin painted shell
//! around them.

use std::time::Duration;

use humm_core::boundary::MeterSnapshot;

use crate::i18n::tr;

/// Fixed widget height (`audio_meter_widget.py`'s `setFixedHeight(56)`).
pub const HEIGHT: f32 = 56.0;
/// Height of the bar region, from the widget's top (`self.BARS_HEIGHT = 28`).
pub const BARS_HEIGHT: f32 = 28.0;
/// Top padding subtracted from each bar's drawn height (`self.PADDING_TOP = 14`).
pub const PADDING_TOP: f32 = 14.0;
/// Bar width in points (`self.BAR_WIDTH = 2`).
pub const BAR_WIDTH: f32 = 2.0;
/// Gap between bars in points (`self.BAR_MARGIN = 1`).
pub const BAR_MARGIN: f32 = 1.0;
/// Minimum amplitude before the first bar lights up (`self.MINIMUM_AMPLITUDE = 0.00005`).
pub const MINIMUM_AMPLITUDE: f32 = 0.00005;
/// Scales the amplitude so the full bar range lights within
/// `1 / AMPLITUDE_SCALE_FACTOR` (`self.AMPLITUDE_SCALE_FACTOR = 10`).
pub const AMPLITUDE_SCALE_FACTOR: f32 = 10.0;
/// `current_amplitude`'s decay-toward-new-value factor (`self.SMOOTHING_FACTOR = 0.95`).
pub const SMOOTHING_FACTOR: f32 = 0.95;
/// Repaint cadence while the meter is visible (spec §2.5): animates at
/// capture rate without an event-per-callback repaint storm.
const REPAINT_INTERVAL: Duration = Duration::from_millis(30);

/// `int((rect.width() / 2) / (BAR_MARGIN + BAR_WIDTH))` — the number of bars
/// drawn on *each* side of the center line (`paintEvent`'s `num_bars_in_half`).
/// Truncates toward zero exactly like Python's `int()`, so a widget narrower
/// than one bar-cell simply paints no bars (no panic, no negative count).
pub fn n_bars_in_half(width: f32) -> usize {
    ((width / 2.0) / (BAR_MARGIN + BAR_WIDTH)).max(0.0) as usize
}

/// Whether bar `i` (0-indexed from the center outward) is active at
/// `current_amplitude`, verbatim from `paintEvent`'s `is_bar_active`:
/// `(current_amplitude - MINIMUM_AMPLITUDE) * AMPLITUDE_SCALE_FACTOR > i / n_bars_in_half`.
/// `n_bars_in_half == 0` (a degenerate/zero-width widget) never activates —
/// there is nothing to divide by and no bar would be drawn anyway.
pub fn bar_active(current_amplitude: f32, bar_index: usize, bars_in_half: usize) -> bool {
    if bars_in_half == 0 {
        return false;
    }
    (current_amplitude - MINIMUM_AMPLITUDE) * AMPLITUDE_SCALE_FACTOR
        > (bar_index as f32 / bars_in_half as f32)
}

/// `current_amplitude`'s per-repaint decay smoothing, verbatim from
/// `update_amplitude`: `max(new, current * SMOOTHING_FACTOR)` — a fresh loud
/// sample snaps up instantly; between loud samples, the displayed value
/// decays by 5% per repaint rather than dropping to the new (quieter) value
/// immediately.
pub fn smoothed_amplitude(new_rms: f32, current: f32) -> f32 {
    new_rms.max(current * SMOOTHING_FACTOR)
}

/// The caption text below the bars, byte-exact format from `paintEvent`:
/// `f"{average_volume_label}: {self.average_amplitude:.4f}  {queue_label}: {self.queue_size}"`
/// — note the **two spaces** between the average-volume and queue fields.
pub fn meter_caption(average_amplitude: f32, queue_size: usize) -> String {
    format!(
        "{}: {average_amplitude:.4}  {}: {queue_size}",
        tr("meter-average-volume"),
        tr("meter-queue"),
    )
}

/// The two theme palettes (`__init__`'s `if self.palette().window().color().black() > 127`),
/// keyed on `dark_mode` (the egui-native equivalent of that Qt palette
/// check, per the task brief). Returns `(inactive, active)`.
fn colors(dark_mode: bool) -> (egui::Color32, egui::Color32) {
    if dark_mode {
        (
            egui::Color32::from_rgb(0x55, 0x55, 0x55),
            egui::Color32::from_rgb(0x99, 0x99, 0x99),
        )
    } else {
        (
            egui::Color32::from_rgb(0xBB, 0xBB, 0xBB),
            egui::Color32::from_rgb(0x55, 0x55, 0x55),
        )
    }
}

/// Paints the meter: bars drawn symmetrically outward from the horizontal
/// center, then the caption centered below them (`paintEvent`, verbatim
/// math via the pure helpers above). `current_amplitude` is the caller's own
/// smoothed-display state (`RecordingVm::meter_current_amplitude`) — this
/// function updates it in place from `snapshot.get_current_rms()` every
/// call, exactly where Buzz's `update_amplitude` runs (once per repaint here,
/// once per callback there — see the module doc for why the smoothing target
/// moved).
///
/// Requests a repaint after [`REPAINT_INTERVAL`] so the meter keeps animating
/// at capture rate while the window is visible (spec §2.5) without needing a
/// repaint request per audio callback.
pub fn audio_meter(
    ui: &mut egui::Ui,
    snapshot: &MeterSnapshot,
    current_amplitude: &mut f32,
) -> egui::Response {
    *current_amplitude = smoothed_amplitude(snapshot.get_current_rms(), *current_amplitude);

    let desired_size = egui::vec2(ui.available_width().max(10.0), HEIGHT);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        let (inactive, active) = colors(ui.visuals().dark_mode);
        let bars = n_bars_in_half(rect.width());
        let painter = ui.painter();
        let center_x = rect.center().x;

        for i in 0..bars {
            let color = if bar_active(*current_amplitude, i, bars) {
                active
            } else {
                inactive
            };
            let step = (i as f32 + 1.0) * (BAR_MARGIN + BAR_WIDTH);
            let left = egui::Rect::from_min_size(
                egui::pos2(center_x - step, rect.top() + PADDING_TOP),
                egui::vec2(BAR_WIDTH, BARS_HEIGHT - PADDING_TOP),
            );
            painter.rect_filled(left, 0.0, color);

            let right_x = center_x + BAR_MARGIN + (i as f32) * (BAR_MARGIN + BAR_WIDTH);
            let right = egui::Rect::from_min_size(
                egui::pos2(right_x, rect.top() + PADDING_TOP),
                egui::vec2(BAR_WIDTH, BARS_HEIGHT - PADDING_TOP),
            );
            painter.rect_filled(right, 0.0, color);
        }

        let caption = meter_caption(snapshot.get_average_rms(), snapshot.get_queue_len());
        painter.text(
            egui::pos2(
                rect.left() + rect.width() / 2.0,
                rect.top() + BARS_HEIGHT + (rect.height() - BARS_HEIGHT) / 2.0,
            ),
            egui::Align2::CENTER_CENTER,
            caption,
            egui::FontId::default(),
            active,
        );
    }

    ui.ctx().request_repaint_after(REPAINT_INTERVAL);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- n_bars_in_half -------------------------------------------------------

    #[test]
    fn n_bars_in_half_matches_buzz_arithmetic() {
        // width=300 -> half=150 -> 150/3 = 50 bars.
        assert_eq!(n_bars_in_half(300.0), 50);
        // Truncates, doesn't round: half=151 -> 151/3 = 50.33 -> 50.
        assert_eq!(n_bars_in_half(302.0), 50);
    }

    #[test]
    fn n_bars_in_half_of_a_tiny_widget_is_zero() {
        assert_eq!(n_bars_in_half(0.0), 0);
        assert_eq!(n_bars_in_half(2.0), 0);
    }

    // -- bar_active -------------------------------------------------------------

    #[test]
    fn bar_active_matches_buzz_threshold() {
        // (0.1 - 0.00005) * 10 = 0.99995; 0/50=0, 49/50=0.98 both < threshold;
        // at i=50 boundary (out of range) not tested; check a couple of indices.
        let amplitude = 0.1_f32;
        assert!(bar_active(amplitude, 0, 50));
        assert!(bar_active(amplitude, 49, 50));
        // A near-silent amplitude lights no bars.
        assert!(!bar_active(0.0, 0, 50));
    }

    #[test]
    fn bar_active_is_false_below_minimum_amplitude() {
        // current_amplitude below MINIMUM_AMPLITUDE makes the LHS negative,
        // which can never exceed a non-negative i/n_bars ratio.
        assert!(!bar_active(0.00001, 0, 50));
    }

    #[test]
    fn bar_active_with_zero_bars_in_half_is_always_false() {
        assert!(!bar_active(1.0, 0, 0));
    }

    #[test]
    fn bar_active_full_scale_amplitude_lights_every_bar() {
        // (1.0 - 0.00005) * 10 ~= 9.9995, comfortably above every i/50 ratio
        // (max ratio 49/50 = 0.98).
        for i in 0..50 {
            assert!(
                bar_active(1.0, i, 50),
                "bar {i} should be active at full scale"
            );
        }
    }

    // -- smoothed_amplitude -------------------------------------------------------

    #[test]
    fn smoothing_snaps_up_to_a_louder_new_sample() {
        assert_eq!(smoothed_amplitude(0.5, 0.1), 0.5);
    }

    #[test]
    fn smoothing_decays_a_quieter_new_sample_by_the_factor() {
        assert_eq!(smoothed_amplitude(0.0, 0.5), 0.5 * SMOOTHING_FACTOR);
    }

    #[test]
    fn smoothing_of_zero_stays_zero() {
        assert_eq!(smoothed_amplitude(0.0, 0.0), 0.0);
    }

    // -- meter_caption -------------------------------------------------------------

    #[test]
    fn caption_has_four_decimal_places_and_two_spaces_between_fields() {
        let _locale_guard = crate::i18n::lock_locale_for_test();
        let caption = meter_caption(0.123_456_7, 42);
        assert_eq!(caption, "Average volume: 0.1235  Queue: 42");
    }

    #[test]
    fn caption_zero_values() {
        let _locale_guard = crate::i18n::lock_locale_for_test();
        assert_eq!(meter_caption(0.0, 0), "Average volume: 0.0000  Queue: 0");
    }
}
