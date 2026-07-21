//! Small, dependency-free display-string formatters shared by every humm
//! binary. Currently just [`format_eta`] (moved here from `humm-cli` so a
//! future GUI progress display can reuse the identical formula rather than
//! reimplementing or subtly diverging from it).

/// Estimated time remaining, ported from Buzz's in-progress ETA
/// (`buzz/widgets/transcription_tasks_table_widget.py:73-90`): with `elapsed >
/// 2s` and `0 < fraction < 1`, `remaining = (1 - p) * elapsed / p`, rendered
/// `NNm NNs` (≥ 60 s) or `NNs`. `None` when no meaningful estimate exists.
///
/// The remaining seconds are rounded to the nearest whole second before the
/// minute/second split — a deliberate, minor refinement over Buzz's truncation
/// that also absorbs the `f32`-fraction representation error (so, e.g.,
/// `(300.0, 0.2)` renders the intended `20m 0s`, not `19m 59s`).
pub fn format_eta(elapsed_secs: f64, fraction: f32) -> Option<String> {
    let p = f64::from(fraction);
    if elapsed_secs <= 2.0 || p <= 0.0 || p >= 1.0 {
        return None;
    }
    let remaining = ((1.0 - p) * elapsed_secs / p).round();
    let secs = remaining as i64;
    if secs >= 60 {
        Some(format!("{}m {}s", secs / 60, secs % 60))
    } else {
        Some(format!("{secs}s"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn format_eta_none_when_elapsed_under_two_seconds() {
        assert_eq!(format_eta(1.0, 0.5), None);
    }

    #[test]
    fn format_eta_seconds_only_under_a_minute() {
        assert_eq!(format_eta(10.0, 0.5), Some("10s".to_string()));
    }

    #[test]
    fn format_eta_minutes_and_seconds_over_a_minute() {
        assert_eq!(format_eta(300.0, 0.2), Some("20m 0s".to_string()));
    }

    #[test]
    fn format_eta_none_at_zero_or_full_fraction() {
        assert_eq!(format_eta(10.0, 0.0), None);
        assert_eq!(format_eta(10.0, 1.0), None);
    }
}
