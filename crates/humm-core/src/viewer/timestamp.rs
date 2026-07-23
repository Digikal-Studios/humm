//! Timestamp string parsing for the segment editor's Start/End cells.
//!
//! Ported from Buzz's `parse_timestamp`
//! (`transcription_segments_editor_widget.py:42-69`). Display uses
//! [`crate::export::to_timestamp`] with `.` as the separator; this module
//! is the inverse — parsing what the user typed back into milliseconds.

use crate::domain::Millis;

/// Parses a timestamp string typed into a segment editor cell.
///
/// Accepts `HH:MM:SS[.mmm]`, `MM:SS[.mmm]`, or `SS[.mmm]` — the fractional
/// milliseconds part is optional in all three forms. Every hour/minute/
/// second/millisecond part must parse as an integer; anything else (wrong
/// colon-part count, non-numeric part) yields `None`. Ported verbatim from
/// Buzz's `parse_timestamp` (`transcription_segments_editor_widget.py:42-69`),
/// including its permissiveness (no upper bound on minutes/seconds, and a
/// seconds part with more than one `.` silently keeps only the first
/// fractional group — Buzz's `seconds_parts[1]` never checks `len() > 2`).
pub fn parse_timestamp(s: &str) -> Option<Millis> {
    let parts: Vec<&str> = s.trim().split(':').collect();

    let (hours, minutes, seconds_part) = match parts.as_slice() {
        [h, m, sec] => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?, *sec),
        [m, sec] => (0, m.parse::<i64>().ok()?, *sec),
        [sec] => (0, 0, *sec),
        _ => return None,
    };

    let seconds_parts: Vec<&str> = seconds_part.split('.').collect();
    let seconds = seconds_parts.first()?.parse::<i64>().ok()?;
    let milliseconds = if seconds_parts.len() > 1 {
        seconds_parts[1].parse::<i64>().ok()?
    } else {
        0
    };

    Some(Millis(
        hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + milliseconds,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::to_timestamp;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_full_hh_mm_ss_with_fraction() {
        assert_eq!(parse_timestamp("01:02:03.456"), Some(Millis(3_723_456)));
    }

    #[test]
    fn parses_mm_ss_with_fraction() {
        assert_eq!(parse_timestamp("02:03.456"), Some(Millis(123_456)));
    }

    #[test]
    fn parses_ss_with_fraction() {
        assert_eq!(parse_timestamp("3.456"), Some(Millis(3_456)));
    }

    #[test]
    fn parses_without_fraction_defaults_to_zero_ms() {
        assert_eq!(parse_timestamp("01:02:03"), Some(Millis(3_723_000)));
        assert_eq!(parse_timestamp("02:03"), Some(Millis(123_000)));
        assert_eq!(parse_timestamp("3"), Some(Millis(3_000)));
    }

    #[test]
    fn garbage_input_returns_none() {
        assert_eq!(parse_timestamp("abc"), None);
        assert_eq!(parse_timestamp(""), None);
        assert_eq!(parse_timestamp("1:2:3:4"), None);
        assert_eq!(parse_timestamp("12:"), None);
        assert_eq!(parse_timestamp(":"), None);
    }

    #[test]
    fn trailing_extra_fraction_groups_are_ignored_not_rejected() {
        // Buzz's `seconds_parts[1]` never validates `len(seconds_parts) > 2` —
        // a stray extra `.` group is silently dropped, not an error.
        assert_eq!(parse_timestamp("1.2.3"), Some(Millis(1_002)));
    }

    #[test]
    fn round_trips_with_to_timestamp() {
        for ms in [0i64, 999, 1_000, 59_999, 3_723_456, 123_456_789] {
            let displayed = to_timestamp(Millis(ms), '.');
            assert_eq!(
                parse_timestamp(&displayed),
                Some(Millis(ms)),
                "displayed: {displayed}"
            );
        }
    }
}
