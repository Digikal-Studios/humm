//! Segment timestamp edit validation: the clamp/reject rules Buzz applies
//! when a Start/End cell commits, or a nudge shortcut fires.
//!
//! Ported from Buzz's `TimeStampEditorDelegate::setModelData`
//! (`transcription_segments_editor_widget.py:123-199`) and
//! `_adjust_segment_timestamp` (`transcription_viewer_widget.py:578-679`) —
//! both apply the same start/end/neighbor rules, unified here into one pure
//! decision function. `timestamp_edit` never writes anything itself; the
//! caller (the GUI's store-backed viewer model) applies the resulting
//! write(s).

use crate::domain::Millis;

/// The start/end times of one segment row, keyed by its store id — the
/// slice [`timestamp_edit`] reasons over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentTimes {
    /// Store row id.
    pub id: i64,
    /// Segment start time.
    pub start: Millis,
    /// Segment end time.
    pub end: Millis,
}

/// Which of a segment's two timestamp fields is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The segment's start time.
    Start,
    /// The segment's end time.
    End,
}

/// The result of validating one timestamp edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOutcome {
    /// The edit is invalid (`start >= end` or `end <= start`) and must be
    /// discarded — no write happens.
    Rejected,
    /// The edit is valid: write `value` to the edited row's field, and,
    /// when the edit pushed past a neighbor row's boundary, also write
    /// `neighbor` (`(neighbor row id, neighbor field, value)`) to keep rows
    /// non-overlapping.
    Apply {
        /// The value to write to the edited field.
        value: Millis,
        /// A neighbor row/field/value to also write, when the edit crossed
        /// that neighbor's boundary.
        neighbor: Option<(i64, Field, Millis)>,
    },
}

/// Nudge step for the ⌘←/⌘→/⌘⇧←/⌘⇧→ shortcuts and the in-editor `+`/`-`
/// keys: ±500 ms (`transcription_viewer_widget.py:578-591`).
pub const NUDGE_MS: i64 = 500;

/// Validates a proposed new value for `rows[idx]`'s `field`, applying
/// Buzz's clamp/reject rules.
///
/// - **Start** edit: rejected if `new_value >= rows[idx].end`; if it drops
///   below the previous row's end, that row's `End` field is also written
///   (`new_value`).
/// - **End** edit: rejected if `new_value <= rows[idx].start`; if it rises
///   above the next row's start, that row's `Start` field is also written
///   (`new_value`).
///
/// `clamp_floor_zero` mirrors the nudge path's `max(0, new_value)` floor
/// (`transcription_viewer_widget.py:628-629`; `TimeStampLineEdit`'s
/// in-editor `-` key, `transcription_segments_editor_widget.py:100-104`);
/// direct cell-edit commits (`TimeStampEditorDelegate::setModelData`) never
/// floor, so callers on that path pass `false`. The floor is applied
/// before validation, matching Buzz's order of operations.
///
/// # Panics
/// Panics if `idx >= rows.len()` — callers always index an existing,
/// currently-selected row.
pub fn timestamp_edit(
    rows: &[SegmentTimes],
    idx: usize,
    field: Field,
    new_value: Millis,
    clamp_floor_zero: bool,
) -> EditOutcome {
    let new_value = if clamp_floor_zero {
        Millis(new_value.0.max(0))
    } else {
        new_value
    };
    let row = rows[idx];

    match field {
        Field::Start => {
            if new_value >= row.end {
                return EditOutcome::Rejected;
            }
            let neighbor = if idx > 0 {
                let prev = rows[idx - 1];
                (new_value < prev.end).then_some((prev.id, Field::End, new_value))
            } else {
                None
            };
            EditOutcome::Apply {
                value: new_value,
                neighbor,
            }
        }
        Field::End => {
            if new_value <= row.start {
                return EditOutcome::Rejected;
            }
            let neighbor = if idx + 1 < rows.len() {
                let next = rows[idx + 1];
                (new_value > next.start).then_some((next.id, Field::Start, new_value))
            } else {
                None
            };
            EditOutcome::Apply {
                value: new_value,
                neighbor,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn row(id: i64, start: i64, end: i64) -> SegmentTimes {
        SegmentTimes {
            id,
            start: Millis(start),
            end: Millis(end),
        }
    }

    #[test]
    fn start_edit_rejected_when_new_value_equals_or_exceeds_end() {
        let rows = [row(1, 0, 1000)];
        assert_eq!(
            timestamp_edit(&rows, 0, Field::Start, Millis(1000), false),
            EditOutcome::Rejected
        );
        assert_eq!(
            timestamp_edit(&rows, 0, Field::Start, Millis(1500), false),
            EditOutcome::Rejected
        );
    }

    #[test]
    fn end_edit_rejected_when_new_value_equals_or_is_below_start() {
        let rows = [row(1, 500, 1000)];
        assert_eq!(
            timestamp_edit(&rows, 0, Field::End, Millis(500), false),
            EditOutcome::Rejected
        );
        assert_eq!(
            timestamp_edit(&rows, 0, Field::End, Millis(100), false),
            EditOutcome::Rejected
        );
    }

    #[test]
    fn start_edit_below_prev_end_writes_prev_end_as_neighbor() {
        let rows = [row(1, 0, 1000), row(2, 1000, 2000)];
        let outcome = timestamp_edit(&rows, 1, Field::Start, Millis(800), false);
        assert_eq!(
            outcome,
            EditOutcome::Apply {
                value: Millis(800),
                neighbor: Some((1, Field::End, Millis(800)))
            }
        );
    }

    #[test]
    fn end_edit_above_next_start_writes_next_start_as_neighbor() {
        let rows = [row(1, 0, 1000), row(2, 1000, 2000)];
        let outcome = timestamp_edit(&rows, 0, Field::End, Millis(1500), false);
        assert_eq!(
            outcome,
            EditOutcome::Apply {
                value: Millis(1500),
                neighbor: Some((2, Field::Start, Millis(1500)))
            }
        );
    }

    #[test]
    fn edits_that_do_not_cross_a_neighbor_boundary_have_no_neighbor_write() {
        let rows = [row(1, 0, 1000), row(2, 1000, 2000)];
        // Start of row 1 moves to 1200: still within [prev.end=1000, own end=2000).
        let outcome = timestamp_edit(&rows, 1, Field::Start, Millis(1200), false);
        assert_eq!(
            outcome,
            EditOutcome::Apply {
                value: Millis(1200),
                neighbor: None
            }
        );
    }

    #[test]
    fn nudge_floors_to_zero_only_when_clamp_floor_zero_is_true() {
        let rows = [row(1, 0, 1000)];

        let floored = timestamp_edit(&rows, 0, Field::Start, Millis(-500), true);
        assert_eq!(
            floored,
            EditOutcome::Apply {
                value: Millis(0),
                neighbor: None
            }
        );

        let unfloored = timestamp_edit(&rows, 0, Field::Start, Millis(-500), false);
        assert_eq!(
            unfloored,
            EditOutcome::Apply {
                value: Millis(-500),
                neighbor: None
            }
        );
    }

    #[test]
    fn first_row_start_edit_has_no_prev_neighbor_to_clamp() {
        let rows = [row(1, 500, 1000), row(2, 1000, 2000)];
        // New start (100) is below where a "previous" end would be, but
        // there is no row 0's predecessor — nothing to clamp.
        let outcome = timestamp_edit(&rows, 0, Field::Start, Millis(100), false);
        assert_eq!(
            outcome,
            EditOutcome::Apply {
                value: Millis(100),
                neighbor: None
            }
        );
    }

    #[test]
    fn last_row_end_edit_has_no_next_neighbor_to_clamp() {
        let rows = [row(1, 0, 1000), row(2, 1000, 1500)];
        let last = rows.len() - 1;
        let outcome = timestamp_edit(&rows, last, Field::End, Millis(5000), false);
        assert_eq!(
            outcome,
            EditOutcome::Apply {
                value: Millis(5000),
                neighbor: None
            }
        );
    }
}
