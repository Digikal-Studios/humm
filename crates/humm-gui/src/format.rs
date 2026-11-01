//! Status/ETA/date display formatting for the task table (F-003, F-025 GUI
//! half, F-008). Ported from `transcription_tasks_table_widget.py:67-107`
//! (status text + ETA) and `:624-634` (`format_timedelta`). The English
//! catalog keeps Buzz's literals byte-exact, including the em dash
//! (`\u{2014}`) in the in-progress/ETA string; other catalogs translate the
//! static words.
//!
//! Percent, ETA, and duration are interpolated with `format!`, not Fluent
//! placeables. Fluent wraps substituted placeables in Unicode bidi-isolation
//! marks by default (`i18n.rs`'s `tr_args_substitutes_named_placeables`
//! test), which would break the English byte-exact contract. Static labels
//! (`Queued`, `Failed`, `Downloading model`, …) go through [`tr`].

use std::path::PathBuf;

use chrono::NaiveDateTime;

use humm_core::domain::Status;
use humm_core::format::format_eta;

use crate::i18n::tr;
use crate::viewmodel::{Phase, RowVm};

/// The task store's naive-local ISO-8601 timestamp format
/// (`humm_store::store::now_iso`): `%Y-%m-%dT%H:%M:%S%.f`.
const ISO_WITH_FRACTION: &str = "%Y-%m-%dT%H:%M:%S%.f";
/// Fallback for a timestamp with no fractional-seconds component. Never
/// written by `now_iso`, but accepted defensively so a hand-authored test
/// fixture (or a hypothetical future writer) doesn't fail to parse.
const ISO_WITHOUT_FRACTION: &str = "%Y-%m-%dT%H:%M:%S";

/// Parses one of the task store's naive-local ISO-8601 timestamps. `None` if
/// `iso` is empty or matches neither accepted format — a display helper must
/// degrade gracefully on a corrupt/foreign row, never panic.
fn parse_iso(iso: &str) -> Option<NaiveDateTime> {
    if iso.is_empty() {
        return None;
    }
    NaiveDateTime::parse_from_str(iso, ISO_WITH_FRACTION)
        .or_else(|_| NaiveDateTime::parse_from_str(iso, ISO_WITHOUT_FRACTION))
        .ok()
}

/// Formats a real elapsed-seconds duration the way Buzz's `format_timedelta`
/// does (`transcription_tasks_table_widget.py:624-634`): `"Ss"` / `"Mm Ss"` /
/// `"Hh Mm Ss"`, dropping only *leading* zero units — a zero minutes/seconds
/// component that follows a non-zero hour/minute is still printed, e.g.
/// `"1h 0m 5s"`.
///
/// Deviation from Buzz (documented, not a bug): Buzz derives this from
/// `datetime.timedelta.seconds`, which wraps at 24 hours (a transcription
/// that somehow ran over a day would show a nonsensical duration); this
/// takes the real elapsed seconds with no wrap. Negative input (clock skew
/// between `time_started`/`time_ended`) clamps to zero rather than emitting a
/// negative-looking string.
pub fn format_timedelta(secs: i64) -> String {
    let secs = secs.max(0);
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// Formats an ISO timestamp for the Date Added / Date Completed columns:
/// `"%Y-%m-%d %H:%M:%S"`. Empty and unparseable input both render as an
/// empty string (Buzz: "strftime ... if non-empty" — a missing timestamp is
/// simply blank, not an error).
pub fn date_text(iso: &str) -> String {
    match parse_iso(iso) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// Rounds a `0.0..=1.0` fraction to an integer percent (Buzz's `.0%` format
/// spec).
fn percent(fraction: f32) -> i64 {
    (f64::from(fraction) * 100.0).round() as i64
}

/// The exact Buzz status-column text for one row
/// (`transcription_tasks_table_widget.py:67-107`), with humm's F-025 phase
/// overrides (`docs/design/UX_SPEC.md` §J-1): while `status` is `InProgress`,
/// a non-`None` `phase` replaces the normal "In Progress ..." text with a
/// download-phase message instead.
///
/// `now` is a parameter (rather than read internally via
/// `chrono::Local::now()`) so the in-progress/ETA branch is deterministic
/// and unit-testable against synthetic timestamps.
///
/// Status-column text. Static words are catalog keys so a non-English locale
/// translates them (F-008). The English values are Buzz's literals, and the
/// percent / ETA / duration are `format!`'d so Fluent's bidi isolates never
/// land in the string.
///
/// [`Phase::Isolating`] and [`Phase::ReducingNoise`] are humm phases with no
/// Buzz original (`docs/design/UX_SPEC.md`'s ✚ phase list). Reducing noise is
/// indeterminate: the reducer has no fraction.
pub fn status_text(row: &RowVm, now: NaiveDateTime) -> String {
    if row.status == Status::InProgress {
        match row.phase {
            Phase::Model(Some(fraction)) => {
                return format!(
                    "{} ({}%)",
                    tr("status-downloading-model"),
                    percent(fraction)
                );
            }
            Phase::Model(None) => {
                return format!("{}\u{2026}", tr("status-downloading-model"));
            }
            Phase::Media(fraction) => {
                return format!(
                    "{} ({}%)",
                    tr("status-downloading-media"),
                    percent(fraction)
                );
            }
            Phase::Isolating => return tr("status-isolating-speech"),
            Phase::ReducingNoise => return tr("status-reducing-noise"),
            Phase::None => {}
        }
    }

    match row.status {
        Status::Queued => tr("status-queued"),
        Status::InProgress => in_progress_text(row, now),
        Status::Completed => completed_text(row),
        Status::Failed => format!(
            "{} ({})",
            tr("status-failed"),
            row.error.as_deref().unwrap_or("")
        ),
        Status::Canceled => tr("status-canceled"),
        Status::Skipped => tr("status-skipped"),
    }
}

/// `"In Progress (NN%)"`, or `"In Progress (NN%) \u{2014} ETA: <eta>"` once
/// `humm_core::format::format_eta`'s guards (>2s elapsed, `0 < p < 1`) are
/// satisfied. The em dash is U+2014, surrounded by spaces, matching Buzz
/// byte-for-byte in English. `ETA` is its own catalog word; the duration
/// text stays `format_eta`'s Buzz spelling.
fn in_progress_text(row: &RowVm, now: NaiveDateTime) -> String {
    let pct = percent(row.progress);
    let label = tr("status-in-progress");
    let eta = row
        .time_started
        .as_deref()
        .and_then(parse_iso)
        .and_then(|started| {
            let elapsed_secs = (now - started).num_milliseconds() as f64 / 1000.0;
            format_eta(elapsed_secs, row.progress)
        });
    match eta {
        Some(eta) => format!("{label} ({pct}%) \u{2014} {}: {eta}", tr("status-eta")),
        None => format!("{label} ({pct}%)"),
    }
}

/// `"Completed"`, or `"Completed (<format_timedelta>)"` when both
/// `time_started` and `time_ended` are present and parse.
fn completed_text(row: &RowVm) -> String {
    let label = tr("status-completed");
    let duration = row
        .time_started
        .as_deref()
        .zip(row.time_ended.as_deref())
        .and_then(|(started, ended)| Some((parse_iso(started)?, parse_iso(ended)?)))
        .map(|(started, ended)| (ended - started).num_seconds());
    match duration {
        Some(secs) => format!("{label} ({})", format_timedelta(secs)),
        None => label,
    }
}

/// Where a non-empty export list landed. One file: its full path. Several
/// files that share a parent: that directory. Otherwise each distinct parent,
/// one per line — never a single directory that does not hold every file.
pub fn written_location(paths: &[PathBuf]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let head = tr("toast-written-to");
    if paths.len() == 1 {
        return Some(format!("{head} {}", paths[0].display()));
    }
    let mut parents: Vec<String> = paths
        .iter()
        .map(|path| {
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(|parent| parent.display().to_string())
                .unwrap_or_else(|| ".".to_string())
        })
        .collect();
    parents.sort();
    parents.dedup();
    if parents.len() == 1 {
        return Some(format!("{head} {}", parents[0]));
    }
    let mut message = head;
    for parent in parents {
        message.push('\n');
        message.push_str(&parent);
    }
    Some(message)
}

/// The completion toast. An empty `written` list keeps the plain
/// "Transcription complete" line. A skipped noise-reduction pass appends
/// its notice without changing the task's completed status.
pub fn completion_message(written: &[PathBuf], reduction_skipped: bool) -> String {
    let mut message = if written.is_empty() {
        tr("toast-completed")
    } else {
        format!(
            "{} \u{2014} {} {}",
            tr("toast-completed"),
            written.len(),
            tr("toast-completed-suffix")
        )
    };
    if let Some(location) = written_location(written) {
        message.push('\n');
        message.push_str(&location);
    }
    if reduction_skipped {
        message.push('\n');
        message.push_str(&tr("toast-reduction-skipped"));
    }
    message
}

/// Info toast for files a later post-processing hook wrote. `None` when
/// `written` is empty — the completion toast already covered the task.
pub fn hook_files_message(written: &[PathBuf]) -> Option<String> {
    let location = written_location(written)?;
    Some(format!("{}\n{location}", tr("toast-postproc-written")))
}

/// Info toast listing post-hook errors. `None` when there are none. The
/// task stays completed; this does not fail it.
pub fn hook_errors_message(errors: &[String]) -> Option<String> {
    if errors.is_empty() {
        return None;
    }
    let mut message = tr("toast-postproc-errors");
    for error in errors {
        message.push('\n');
        message.push_str(error);
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewmodel::ColumnId;
    use humm_core::domain::TranscriptionId;
    use pretty_assertions::assert_eq;

    /// Locks the locale so a parallel test's `set_locale` cannot change the
    /// English byte-exact strings these cases pin. Not used by the isolating
    /// test, which already holds the same non-reentrant lock.
    fn status(row: &RowVm, now: NaiveDateTime) -> String {
        let _guard = crate::i18n::lock_locale_for_test();
        status_text(row, now)
    }

    fn base_row(status: Status) -> RowVm {
        RowVm {
            id: TranscriptionId::new(),
            display_name: "clip.wav".to_string(),
            model_text: "Whisper.cpp (small)".to_string(),
            task_text: "Transcribe".to_string(),
            status,
            progress: 0.0,
            phase: Phase::None,
            error: None,
            time_queued: "2026-07-21T09:00:00.000000".to_string(),
            time_started: None,
            time_ended: None,
            notes: None,
            file: None,
            url: None,
        }
    }

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, ISO_WITH_FRACTION).expect("valid fixture timestamp")
    }

    // -- format_timedelta -----------------------------------------------

    #[test]
    fn format_timedelta_seconds_only() {
        assert_eq!(format_timedelta(5), "5s");
    }

    #[test]
    fn format_timedelta_minutes_and_seconds() {
        assert_eq!(format_timedelta(185), "3m 5s");
    }

    #[test]
    fn format_timedelta_hours_minutes_seconds() {
        assert_eq!(format_timedelta(3725), "1h 2m 5s");
    }

    #[test]
    fn format_timedelta_drops_only_leading_zero_units() {
        // A non-zero hour keeps the (zero) minute and second components.
        assert_eq!(format_timedelta(3600), "1h 0m 0s");
    }

    #[test]
    fn format_timedelta_clamps_negative_to_zero() {
        assert_eq!(format_timedelta(-5), "0s");
    }

    // -- date_text --------------------------------------------------------

    #[test]
    fn date_text_formats_iso_with_fraction() {
        assert_eq!(
            date_text("2026-07-21T09:30:05.123456"),
            "2026-07-21 09:30:05"
        );
    }

    #[test]
    fn date_text_empty_input_is_empty() {
        assert_eq!(date_text(""), "");
    }

    #[test]
    fn date_text_unparseable_input_is_empty() {
        assert_eq!(date_text("not-a-timestamp"), "");
    }

    // -- status_text: terminal / simple statuses ---------------------------

    #[test]
    fn status_text_queued() {
        assert_eq!(
            status(&base_row(Status::Queued), dt("2026-07-21T09:00:00.0")),
            "Queued"
        );
    }

    #[test]
    fn status_text_canceled() {
        assert_eq!(
            status(&base_row(Status::Canceled), dt("2026-07-21T09:00:00.0")),
            "Canceled"
        );
    }

    #[test]
    fn status_text_skipped() {
        assert_eq!(
            status(&base_row(Status::Skipped), dt("2026-07-21T09:00:00.0")),
            "Skipped"
        );
    }

    #[test]
    fn status_text_failed_includes_error_message() {
        let mut row = base_row(Status::Failed);
        row.error = Some("ffmpeg not found".to_string());
        assert_eq!(
            status(&row, dt("2026-07-21T09:00:00.0")),
            "Failed (ffmpeg not found)"
        );
    }

    #[test]
    fn status_text_failed_with_no_message_renders_empty_parens() {
        let row = base_row(Status::Failed);
        assert_eq!(status(&row, dt("2026-07-21T09:00:00.0")), "Failed ()");
    }

    // -- status_text: in-progress / ETA guards -----------------------------

    #[test]
    fn status_text_in_progress_no_started_time_omits_eta() {
        let mut row = base_row(Status::InProgress);
        row.progress = 0.42;
        assert_eq!(
            status(&row, dt("2026-07-21T09:00:10.0")),
            "In Progress (42%)"
        );
    }

    #[test]
    fn status_text_in_progress_under_two_seconds_elapsed_omits_eta() {
        let mut row = base_row(Status::InProgress);
        row.progress = 0.5;
        row.time_started = Some("2026-07-21T09:00:00.0".to_string());
        // Only 1 second elapsed: format_eta's own >2s guard suppresses ETA.
        assert_eq!(
            status(&row, dt("2026-07-21T09:00:01.0")),
            "In Progress (50%)"
        );
    }

    #[test]
    fn status_text_in_progress_zero_progress_omits_eta() {
        let mut row = base_row(Status::InProgress);
        row.progress = 0.0;
        row.time_started = Some("2026-07-21T09:00:00.0".to_string());
        assert_eq!(
            status(&row, dt("2026-07-21T09:01:00.0")),
            "In Progress (0%)"
        );
    }

    #[test]
    fn status_text_in_progress_full_progress_omits_eta() {
        let mut row = base_row(Status::InProgress);
        row.progress = 1.0;
        row.time_started = Some("2026-07-21T09:00:00.0".to_string());
        assert_eq!(
            status(&row, dt("2026-07-21T09:01:00.0")),
            "In Progress (100%)"
        );
    }

    #[test]
    fn status_text_in_progress_with_eta_matches_buzz_format_with_em_dash() {
        let mut row = base_row(Status::InProgress);
        // p = 0.5 makes remaining == elapsed exactly (no floating-point
        // rounding risk): (1 - 0.5) * elapsed / 0.5 == elapsed.
        row.progress = 0.5;
        row.time_started = Some("2026-07-21T09:00:00.0".to_string());
        // 222s elapsed == 3m 42s, matching the brief's illustrative ETA text.
        let now = dt("2026-07-21T09:03:42.0");

        let text = status(&row, now);
        assert_eq!(text, "In Progress (50%) \u{2014} ETA: 3m 42s");
        assert!(
            text.contains('\u{2014}'),
            "expected a literal em dash (U+2014)"
        );
    }

    // -- status_text: F-025 phase overrides ---------------------------------

    #[test]
    fn status_text_model_download_phase_with_known_fraction() {
        let mut row = base_row(Status::InProgress);
        row.phase = Phase::Model(Some(0.3));
        assert_eq!(
            status(&row, dt("2026-07-21T09:00:00.0")),
            "Downloading model (30%)"
        );
    }

    #[test]
    fn status_text_model_download_phase_without_total_omits_percent() {
        let mut row = base_row(Status::InProgress);
        row.phase = Phase::Model(None);
        assert_eq!(
            status(&row, dt("2026-07-21T09:00:00.0")),
            "Downloading model\u{2026}"
        );
    }

    #[test]
    fn status_text_media_download_phase() {
        let mut row = base_row(Status::InProgress);
        row.phase = Phase::Media(0.75);
        assert_eq!(
            status(&row, dt("2026-07-21T09:00:00.0")),
            "Downloading media (75%)"
        );
    }

    #[test]
    fn status_text_isolating_speech_phase() {
        // Locale-independent by construction: compares against `tr` calling
        // the same key, rather than a hardcoded English string, since this
        // (unlike every other branch in this file) is a translated humm
        // addition, not a byte-exact Buzz string. This test never calls
        // `set_locale` itself, but `status_text`'s `Phase::Isolating` arm
        // and the right-hand `tr` call below are two separate reads of
        // process-wide locale state, so it still needs
        // `lock_locale_for_test`'s guard against a concurrently running
        // test's in-flight locale switch landing between them (its own doc
        // comment says so explicitly).
        let _guard = crate::i18n::lock_locale_for_test();
        let mut row = base_row(Status::InProgress);
        row.phase = Phase::Isolating;
        assert_eq!(
            status_text(&row, dt("2026-07-21T09:00:00.0")),
            crate::i18n::tr("status-isolating-speech")
        );
        row.phase = Phase::ReducingNoise;
        assert_eq!(
            status_text(&row, dt("2026-07-21T09:00:00.0")),
            crate::i18n::tr("status-reducing-noise")
        );
    }

    #[test]
    fn status_text_phase_ignored_once_status_leaves_in_progress() {
        // A stale phase must not leak into a terminal status's text (apply()
        // is documented to clear it, but status_text itself must also not
        // render it for a non-InProgress row, belt-and-braces).
        let mut row = base_row(Status::Completed);
        row.phase = Phase::Media(0.75);
        assert_eq!(status(&row, dt("2026-07-21T09:00:00.0")), "Completed");
    }

    // -- status_text: completed duration ------------------------------------

    #[test]
    fn status_text_completed_with_duration() {
        let mut row = base_row(Status::Completed);
        row.time_started = Some("2026-07-21T09:00:00.0".to_string());
        row.time_ended = Some("2026-07-21T09:03:05.0".to_string());
        assert_eq!(
            status(&row, dt("2026-07-21T09:03:05.0")),
            "Completed (3m 5s)"
        );
    }

    #[test]
    fn status_text_completed_without_timestamps_omits_duration() {
        let row = base_row(Status::Completed);
        assert_eq!(status(&row, dt("2026-07-21T09:00:00.0")), "Completed");
    }

    #[test]
    fn completion_message_empty_written_stays_the_plain_toast() {
        let _guard = crate::i18n::lock_locale_for_test();
        assert_eq!(completion_message(&[], false), "Transcription complete");
    }

    #[test]
    fn completion_message_names_a_single_path_and_distinct_parents() {
        let _guard = crate::i18n::lock_locale_for_test();
        assert_eq!(
            completion_message(&[PathBuf::from("/media/1/a.srt")], false),
            "Transcription complete \u{2014} 1 file(s) written\nWritten to /media/1/a.srt"
        );
        assert_eq!(
            completion_message(
                &[
                    PathBuf::from("/media/1/a.srt"),
                    PathBuf::from("/media/1/a.txt"),
                ],
                false
            ),
            "Transcription complete \u{2014} 2 file(s) written\nWritten to /media/1"
        );
        let split = completion_message(
            &[PathBuf::from("/a/x.srt"), PathBuf::from("/b/y.srt")],
            false,
        );
        assert!(
            split.contains("\n/a\n/b") || split.contains("\n/b\n/a"),
            "distinct parents are listed, got {split:?}"
        );
        assert!(
            !split.contains("Written to /a/x.srt"),
            "a mixed list must not pretend one file's path holds every file"
        );
    }

    #[test]
    fn completion_message_appends_a_skipped_reduction_without_hiding_the_toast() {
        let _guard = crate::i18n::lock_locale_for_test();
        let text = completion_message(&[], true);
        assert_eq!(
            text,
            "Transcription complete\nNoise reduction did not run; transcription continued."
        );
    }

    #[test]
    fn hook_messages_skip_empty_lists_and_keep_errors_non_failing() {
        let _guard = crate::i18n::lock_locale_for_test();
        assert!(hook_files_message(&[]).is_none());
        assert!(hook_errors_message(&[]).is_none());
        let errors = hook_errors_message(&["summary: timed out".to_string()]).expect("notice");
        assert!(errors.contains("summary: timed out"));
        assert!(errors.contains("still complete"));
    }

    // Sanity: ColumnId is used elsewhere in this crate's tests via this
    // module's `use` — referenced here so `cargo fmt`/clippy don't flag an
    // unused import if a future edit trims the other tests' usage.
    #[test]
    fn column_id_all_is_seven_columns_for_the_table_this_module_formats() {
        assert_eq!(ColumnId::all().len(), 7);
    }
}
