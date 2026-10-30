//! F-115 `skip_already_transcribed`: skip a transcription whose result
//! already exists — as a sibling `.txt`/`.srt`/`.vtt` result file, or as a
//! completed transcription in the store with the same file basename.
//!
//! Ported from `buzz/plugins/skip_already_transcribed/plugin.py` (whole
//! file); the manager's dispatch order (`buzz/plugins/manager.py:88-105`,
//! "first non-None result wins") collapses here to a simple `if`/`if`
//! chain, since this is the only pre-run hook M7 implements (design spec §0:
//! `before_transcription` is out of scope until F-112/F-114 need it).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::boundary::TaskStore;
use crate::domain::{Millis, OutputFormat, Segment};
use crate::settings::SkipSettings;

/// Result-file formats searched, in Buzz's `OutputFormat` declaration order
/// (`buzz/transcriber/transcriber.py:212-215`; humm's [`OutputFormat`] is
/// declared `Txt < Srt < Vtt` to match, `domain.rs`).
const SEARCH_FORMATS: [OutputFormat; 3] = [OutputFormat::Txt, OutputFormat::Srt, OutputFormat::Vtt];

/// Runs the skip check for one task (design spec §4, `plugin.py:50-75`).
///
/// If [`SkipSettings::check_result_files`] is set, searches `input_path`'s
/// parent directory, then `output_dir` (if given), in that order, for a
/// matching result file (see [`find_result_file_segments`]); failing that —
/// or if that check is off — [`SkipSettings::check_database`] looks up a
/// prior `completed` task with the same file basename via `store` (see
/// [`find_db_segments`]).
///
/// Returns `None` when neither check is enabled, or both run and find
/// nothing: the caller (`queue::Runner::run_job`) then transcribes the task
/// normally. Returns `Some(segments)` the moment either check hits — the
/// task's transcript, to be persisted via `TaskStore::mark_skipped` instead
/// of running the engine.
pub fn check(
    settings: &SkipSettings,
    input_path: &Path,
    output_dir: Option<&Path>,
    store: &dyn TaskStore,
    source_url: Option<&str>,
) -> Option<Vec<Segment>> {
    // A URL download is `source.<ext>`. That name matches unrelated files, so
    // only the stored URL identifies it (Buzz `bc2882b`). Result-file search
    // is not used for a URL.
    if let Some(url) = source_url {
        if !settings.check_database {
            return None;
        }
        return find_db_segments_by_url(url, store);
    }

    if settings.check_result_files
        && let Some(segments) = find_result_file_segments(input_path, output_dir)
    {
        log::info!(
            "skip_already_transcribed: found existing result file for {}",
            basename(input_path)
        );
        return Some(segments);
    }

    if settings.check_database
        && let Some(segments) = find_db_segments(input_path, store)
    {
        log::info!(
            "skip_already_transcribed: found prior DB record for {}",
            basename(input_path)
        );
        return Some(segments);
    }

    None
}

/// `path`'s file name as a display string, or `"?"` if it has none — used
/// only for the log lines in [`check`].
fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".to_string())
}

/// The stem used to match result files and DB rows: `path`'s file stem with
/// a trailing `"_speech"` removed (`plugin.py:80-82`). Buzz's (unported,
/// F-027) speech-extraction plugin names its intermediate file
/// `{stem}_speech.mp3`; stripping the suffix here means a result produced
/// from that intermediate file still matches the original recording's stem.
///
/// `pub(super)`: also reused by [`crate::postproc::docx`] (F-113) for its
/// own stem derivation — Buzz's `export_docx` plugin instead prefers
/// `task.original_file_path` when set (`export_docx/plugin.py:102-107`),
/// which humm has no equivalent field for yet (see that module's doc
/// comment); stripping `_speech` from the same `task_input` this module
/// already receives reaches the same practical stem.
pub(super) fn stem_for(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem.strip_suffix("_speech")
        .map(str::to_string)
        .unwrap_or(stem)
}

/// Searches `input_path`'s parent directory, then `output_dir` (if any), in
/// that order — per directory, [`SEARCH_FORMATS`] in order — for a result
/// file whose name starts with the sanitized stem and ends with `.{format}`.
///
/// The stem is [`stem_for`] passed through
/// [`crate::export::safe_filename_component`], because export writes that
/// form (`what?` becomes `what_`). Buzz does the same in
/// `_find_result_file_segments` (`plugin.py`, `053b388`) and then
/// `glob.escape`s it. This search reads the directory and uses
/// [`str::starts_with`], so `[` and `]` stay literal characters. The
/// prefix-overmatch quirk is preserved: stem `"audio"` also matches
/// `audio2.srt`. Returns the first candidate (directory entries visited in
/// sorted order, for determinism) that parses to at least one segment.
fn find_result_file_segments(input_path: &Path, output_dir: Option<&Path>) -> Option<Vec<Segment>> {
    let stem = crate::export::safe_filename_component(&stem_for(input_path));

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = input_path.parent() {
        search_dirs.push(parent.to_path_buf());
    }
    if let Some(dir) = output_dir {
        search_dirs.push(dir.to_path_buf());
    }

    for dir in &search_dirs {
        if !dir.is_dir() {
            continue;
        }
        for format in SEARCH_FORMATS {
            for candidate in candidates(dir, &stem, format) {
                if let Some(segments) = parse_result_file(&candidate, format) {
                    return Some(segments);
                }
            }
        }
    }
    None
}

/// Every file directly inside `dir` whose name starts with `stem` and ends
/// with `.{format}`, in sorted order. An unreadable `dir` yields no
/// candidates (the `dir.is_dir()` guard in [`find_result_file_segments`]
/// already filters the common case; this is the belt-and-braces fallback
/// for a race — removed between the check and the read).
fn candidates(dir: &Path, stem: &str, format: OutputFormat) -> Vec<PathBuf> {
    let suffix = format!(".{}", format.as_str());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut matches: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            name.starts_with(stem) && name.ends_with(&suffix)
        })
        .collect();
    matches.sort();
    matches
}

/// Parses one candidate result file, returning `None` (no hit — the caller
/// tries the next candidate) on a read failure or a parse that yields no
/// usable transcript (an empty TXT file, or an SRT/VTT file with zero valid
/// blocks) — logged at `warn`, mirroring Buzz's broad `except Exception`
/// (`plugin.py:98-113`).
fn parse_result_file(path: &Path, format: OutputFormat) -> Option<Vec<Segment>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            log::warn!(
                "skip_already_transcribed: failed to read result file {}: {err}",
                path.display()
            );
            return None;
        }
    };
    match format {
        OutputFormat::Txt => parse_txt(&text),
        OutputFormat::Srt => non_empty(parse_srt(&text)),
        OutputFormat::Vtt => non_empty(parse_vtt(&text)),
    }
}

/// `Some(segments)` iff `segments` is non-empty — Python's `x or None`
/// idiom, ported literally (`plugin.py:108,110`).
fn non_empty(segments: Vec<Segment>) -> Option<Vec<Segment>> {
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

/// TXT parse (`plugin.py:103-106`): the whole trimmed file as one
/// `Segment { start: 0, end: 0, .. }`; an empty (or whitespace-only) file is
/// not a hit.
fn parse_txt(text: &str) -> Option<Vec<Segment>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(vec![Segment {
            start: Millis(0),
            end: Millis(0),
            text: trimmed.to_string(),
            translation: String::new(),
        }])
    }
}

/// Buzz's block splitter: one or more consecutive blank lines
/// (`re.split(r"\n\n+", text.strip())`, `plugin.py:119,139`).
fn block_split_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\n\n+").expect(r"\n\n+ is a valid regex"))
}

/// SRT timestamp line pattern (`plugin.py:124-126`): comma decimal
/// separator. Anchored at the start only (`^`, no `$`), matching Python's
/// `re.match` semantics — trailing content on the line (rare, but SRT
/// tolerates it) does not disqualify the match.
fn srt_timestamp_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"^(\d+:\d+:\d+,\d+) --> (\d+:\d+:\d+,\d+)")
            .expect("SRT timestamp regex is a static literal")
    })
}

/// VTT timestamp line pattern (`plugin.py:144-146`): dot decimal separator.
/// Anchored the same way as [`srt_timestamp_regex`].
fn vtt_timestamp_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"^(\d+:\d+:\d+\.\d+) --> (\d+:\d+:\d+\.\d+)")
            .expect("VTT timestamp regex is a static literal")
    })
}

/// SRT parse (`plugin.py:115-133`): blocks split on one-or-more blank
/// lines; a block needs at least 3 lines (index, timestamp, text...); line
/// 1 must match [`srt_timestamp_regex`] (its `,` decimal separator is
/// rewritten to `.` before [`ts_to_ms`]); the text is every line from index
/// 2 onward, joined with `\n`. A block that doesn't match is skipped, not
/// aborted — the rest of the file is still parsed.
fn parse_srt(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    for block in block_split_regex().split(text.trim()) {
        let lines: Vec<&str> = block.trim().lines().collect();
        if lines.len() < 3 {
            continue;
        }
        let Some(caps) = srt_timestamp_regex().captures(lines[1]) else {
            continue;
        };
        let (Some(start), Some(end)) = (
            ts_to_ms(&caps[1].replace(',', ".")),
            ts_to_ms(&caps[2].replace(',', ".")),
        ) else {
            continue;
        };
        segments.push(Segment {
            start: Millis(start),
            end: Millis(end),
            text: lines[2..].join("\n"),
            translation: String::new(),
        });
    }
    segments
}

/// VTT parse (`plugin.py:135-153`): blocks split the same way; a block
/// needs at least 2 lines (timestamp, text...); line 0 must match
/// [`vtt_timestamp_regex`] (no cue-index line, unlike SRT).
fn parse_vtt(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    for block in block_split_regex().split(text.trim()) {
        let lines: Vec<&str> = block.trim().lines().collect();
        if lines.len() < 2 {
            continue;
        }
        let Some(caps) = vtt_timestamp_regex().captures(lines[0]) else {
            continue;
        };
        let (Some(start), Some(end)) = (ts_to_ms(&caps[1]), ts_to_ms(&caps[2])) else {
            continue;
        };
        segments.push(Segment {
            start: Millis(start),
            end: Millis(end),
            text: lines[1..].join("\n"),
            translation: String::new(),
        });
    }
    segments
}

/// `h:m:s.frac` → milliseconds, Buzz's `_ts_to_ms` (`plugin.py:155-158`):
/// `int((h*3600 + m*60 + s) * 1000)`. Truncates toward zero rather than
/// rounding — Python's `int()` on a float truncates; `as i64` does the same
/// for the non-negative values every real timestamp produces, and the same
/// operation order (`h*3600.0 + m*60.0` is an exact integer sum in `f64`
/// for any realistic timestamp, then `+ s`, then `* 1000.0`) reproduces
/// Python's float arithmetic bit-for-bit, so the truncation lands on the
/// same value.
fn ts_to_ms(ts: &str) -> Option<i64> {
    let mut parts = ts.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(((h * 3600.0 + m * 60.0 + s) * 1000.0) as i64)
}

/// The store-backed check ([`SkipSettings::check_database`],
/// `plugin.py:160-177`): looks up a prior `completed` task whose input
/// file's basename matches `input_path`'s, and copies its segments — text
/// only, translations dropped (Buzz rebuilds `Segment(start, end, text)`
/// with no third field, `plugin.py:170-174`). A missing basename, no match,
/// or any store error is a miss (Buzz's broad `except Exception`,
/// `plugin.py:175-177`) — logged at `warn`, never surfaced as a failure.
fn find_db_segments_by_url(url: &str, store: &dyn TaskStore) -> Option<Vec<Segment>> {
    let id = match store.find_completed_by_url(url) {
        Ok(Some(id)) => id,
        Ok(None) => return None,
        Err(err) => {
            log::warn!("skip_already_transcribed: URL DB check failed: {err}");
            return None;
        }
    };
    load_segments(store, &id)
}

fn find_db_segments(input_path: &Path, store: &dyn TaskStore) -> Option<Vec<Segment>> {
    let name = input_path.file_name()?.to_string_lossy().into_owned();
    let id = match store.find_completed_by_file_name(&name) {
        Ok(Some(id)) => id,
        Ok(None) => return None,
        Err(err) => {
            log::warn!("skip_already_transcribed: DB check failed: {err}");
            return None;
        }
    };
    load_segments(store, &id)
}

fn load_segments(
    store: &dyn TaskStore,
    id: &crate::domain::TranscriptionId,
) -> Option<Vec<Segment>> {
    match store.segments_for(id) {
        Ok(segments) => Some(
            segments
                .into_iter()
                .map(|segment| Segment {
                    start: segment.start,
                    end: segment.end,
                    text: segment.text,
                    translation: String::new(),
                })
                .collect(),
        ),
        Err(err) => {
            log::warn!("skip_already_transcribed: DB check failed: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TranscriptionId;
    use crate::error::StoreError;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    // -- A minimal `TaskStore` double: only the two read methods this
    //    module calls are ever exercised; every write method is
    //    unreachable in these tests (`check`'s DB path never writes). ------

    #[derive(Default)]
    struct FakeStore {
        found: Option<TranscriptionId>,
        found_url: Option<TranscriptionId>,
        segments: Vec<Segment>,
        find_fails: bool,
        segments_fail: bool,
    }

    impl TaskStore for FakeStore {
        fn create_task(
            &self,
            _id: &TranscriptionId,
            _spec: &crate::domain::TaskSpec,
            _source: crate::domain::Source,
        ) -> Result<(), StoreError> {
            unreachable!("check() never creates tasks")
        }

        fn requeue_task(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("check() never requeues tasks")
        }

        fn mark_started(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("check() never marks tasks started")
        }

        fn update_progress(&self, _id: &TranscriptionId, _fraction: f64) -> Result<(), StoreError> {
            unreachable!("check() never updates progress")
        }

        fn update_file(&self, _id: &TranscriptionId, _path: &Path) -> Result<(), StoreError> {
            unreachable!("check() never updates the file path")
        }

        fn set_task_name(&self, _id: &TranscriptionId, _name: &str) -> Result<(), StoreError> {
            unreachable!("check() never sets the task name")
        }

        fn update_language(
            &self,
            _id: &TranscriptionId,
            _language: &str,
        ) -> Result<(), StoreError> {
            unreachable!("check() never updates the language")
        }

        fn mark_completed(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("check() never marks tasks completed")
        }

        fn mark_failed(&self, _id: &TranscriptionId, _error: &str) -> Result<(), StoreError> {
            unreachable!("check() never marks tasks failed")
        }

        fn mark_canceled(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("check() never marks tasks canceled")
        }

        fn mark_skipped(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("check() never marks tasks skipped (the caller does)")
        }

        fn find_completed_by_file_name(
            &self,
            _name: &str,
        ) -> Result<Option<TranscriptionId>, StoreError> {
            if self.find_fails {
                Err(StoreError::Query {
                    detail: "boom".to_string(),
                })
            } else {
                Ok(self.found)
            }
        }

        fn find_completed_by_url(&self, _url: &str) -> Result<Option<TranscriptionId>, StoreError> {
            if self.find_fails {
                Err(StoreError::Query {
                    detail: "boom".to_string(),
                })
            } else {
                Ok(self.found_url)
            }
        }

        fn segments_for(&self, _id: &TranscriptionId) -> Result<Vec<Segment>, StoreError> {
            if self.segments_fail {
                Err(StoreError::Query {
                    detail: "boom".to_string(),
                })
            } else {
                Ok(self.segments.clone())
            }
        }

        fn set_task_notes(&self, _id: &TranscriptionId, _notes: &str) -> Result<(), StoreError> {
            unreachable!("check() never writes notes (that's postproc::summary's job)")
        }

        fn replace_task_segments(
            &self,
            _id: &TranscriptionId,
            _segs: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("check() never replaces segments (that's postproc::auto_resize's job)")
        }
    }

    fn seg(start: i64, end: i64, text: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    fn never_touched_store() -> FakeStore {
        FakeStore::default()
    }

    fn enabled_skip(check_result_files: bool, check_database: bool) -> SkipSettings {
        SkipSettings {
            enabled: true,
            check_result_files,
            check_database,
        }
    }

    // -- stem -----------------------------------------------------------

    #[test]
    fn stem_strips_speech_suffix() {
        assert_eq!(stem_for(Path::new("/x/audio_speech.mp3")), "audio");
        assert_eq!(stem_for(Path::new("/x/audio.mp3")), "audio");
    }

    #[test]
    fn result_search_matches_the_sanitized_export_stem() {
        let tmp = TempDir::new().expect("tempdir");
        // Windows rejects `?` in a file name (os error 123). The search
        // reads the path stem, so this fixture stays a path: `what?`
        // sanitizes to `what_`, and only that result file is created.
        let input = tmp.path().join("what?.mp3");
        std::fs::write(tmp.path().join("what_.txt"), "sanitized hit").expect("write txt");

        let segments =
            find_result_file_segments(&input, None).expect("a result file should be found");
        assert_eq!(segments, vec![seg(0, 0, "sanitized hit")]);
    }

    #[test]
    fn result_search_treats_square_brackets_as_literal() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip[2].mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        std::fs::write(tmp.path().join("clip[2].txt"), "bracket hit").expect("write txt");
        std::fs::write(tmp.path().join("clip2.txt"), "character class").expect("write other");

        let segments =
            find_result_file_segments(&input, None).expect("a result file should be found");
        assert_eq!(segments, vec![seg(0, 0, "bracket hit")]);
    }

    // -- glob prefix-overmatch quirk --------------------------------------

    #[test]
    fn glob_prefix_quirk_matches_longer_stems() {
        let tmp = TempDir::new().expect("tempdir");
        std::fs::write(tmp.path().join("audio2.srt"), "not real srt").expect("write");
        let found = candidates(tmp.path(), "audio", OutputFormat::Srt);
        assert_eq!(found, vec![tmp.path().join("audio2.srt")]);
    }

    // -- format order: TXT before SRT before VTT --------------------------

    #[test]
    fn format_order_txt_wins_over_planted_srt() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        std::fs::write(tmp.path().join("clip.txt"), "txt wins").expect("write txt");
        std::fs::write(
            tmp.path().join("clip.srt"),
            "1\n00:00:00,000 --> 00:00:01,000\nsrt text\n",
        )
        .expect("write srt");

        let segments =
            find_result_file_segments(&input, None).expect("a result file should be found");
        assert_eq!(segments, vec![seg(0, 0, "txt wins")]);
    }

    // -- search-dir order: parent beats output dir ------------------------

    #[test]
    fn search_dir_order_parent_beats_output_dir() {
        let parent = TempDir::new().expect("parent tempdir");
        let out = TempDir::new().expect("output tempdir");
        let input = parent.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        std::fs::write(parent.path().join("clip.txt"), "parent hit").expect("write parent txt");
        std::fs::write(out.path().join("clip.txt"), "output hit").expect("write output txt");

        let segments = find_result_file_segments(&input, Some(out.path()))
            .expect("a result file should be found");
        assert_eq!(segments, vec![seg(0, 0, "parent hit")]);
    }

    #[test]
    fn output_dir_used_when_parent_has_no_hit() {
        let parent = TempDir::new().expect("parent tempdir");
        let out = TempDir::new().expect("output tempdir");
        let input = parent.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        std::fs::write(out.path().join("clip.txt"), "output hit").expect("write output txt");

        let segments = find_result_file_segments(&input, Some(out.path()))
            .expect("a result file should be found");
        assert_eq!(segments, vec![seg(0, 0, "output hit")]);
    }

    // -- TXT parse ---------------------------------------------------------

    #[test]
    fn txt_round_trip() {
        assert_eq!(
            parse_txt("  hello world  \n"),
            Some(vec![seg(0, 0, "hello world")])
        );
    }

    #[test]
    fn txt_empty_is_a_miss() {
        assert_eq!(parse_txt("   \n  "), None);
        assert_eq!(parse_txt(""), None);
    }

    // -- SRT parse -----------------------------------------------------

    #[test]
    fn srt_well_formed_two_blocks_round_trip() {
        let text = "1\n00:00:00,000 --> 00:00:01,500\nhello\n\n\
                     2\n00:00:01,500 --> 00:00:03,000\nworld\n";
        let segments = parse_srt(text);
        assert_eq!(
            segments,
            vec![seg(0, 1500, "hello"), seg(1500, 3000, "world")]
        );
    }

    #[test]
    fn srt_malformed_block_is_skipped_others_still_parsed() {
        let text = "1\nnot a timestamp\nbad block\n\n\
                     2\n00:00:01,500 --> 00:00:03,000\ngood block\n";
        let segments = parse_srt(text);
        assert_eq!(segments, vec![seg(1500, 3000, "good block")]);
    }

    #[test]
    fn srt_block_with_too_few_lines_is_skipped() {
        let text = "1\n00:00:00,000 --> 00:00:01,000\n";
        // Only 2 lines (index + timestamp, no text line) - Buzz requires >= 3.
        let segments = parse_srt(text);
        assert_eq!(segments, Vec::<Segment>::new());
    }

    #[test]
    fn srt_multiline_text_is_joined_with_newline() {
        let text = "1\n00:00:00,000 --> 00:00:01,000\nline one\nline two\n";
        let segments = parse_srt(text);
        assert_eq!(segments, vec![seg(0, 1000, "line one\nline two")]);
    }

    // -- VTT parse -----------------------------------------------------

    #[test]
    fn vtt_well_formed_round_trip() {
        let text = "00:00:00.000 --> 00:00:01.500\nhello\n\n00:00:01.500 --> 00:00:03.000\nworld\n";
        let segments = parse_vtt(text);
        assert_eq!(
            segments,
            vec![seg(0, 1500, "hello"), seg(1500, 3000, "world")]
        );
    }

    // -- ms parsing: comma-to-dot, exact values, truncation ---------------

    #[test]
    fn srt_comma_to_dot_ms_parsing_is_exact() {
        assert_eq!(ts_to_ms(&"00:00:01,500".replace(',', ".")), Some(1500));
    }

    #[test]
    fn ts_to_ms_truncates_fractional_milliseconds_toward_zero() {
        // 61.999s -> 61999ms exactly.
        assert_eq!(ts_to_ms("00:01:01.999"), Some(61_999));
    }

    #[test]
    fn ts_to_ms_truncation_pinned_for_near_integer_fraction() {
        // 1.9995s * 1000 = 1999.5(00...)ms; int() truncates to 1999, not 2000.
        assert_eq!(ts_to_ms("00:00:01.9995"), Some(1999));
    }

    // -- disabled-flag gates ------------------------------------------------

    #[test]
    fn check_result_files_disabled_skips_the_file_search() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        std::fs::write(tmp.path().join("clip.txt"), "would have hit").expect("write txt");

        let settings = enabled_skip(false, false);
        let result = check(&settings, &input, None, &never_touched_store(), None);
        assert_eq!(result, None);
    }

    #[test]
    fn check_database_disabled_skips_the_db_lookup() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");

        let store = FakeStore {
            found: Some(TranscriptionId::new()),
            segments: vec![seg(0, 1000, "db text")],
            ..FakeStore::default()
        };
        let settings = enabled_skip(true, false);
        // No result file planted, and check_database is off: no hit at all,
        // and the DB is never consulted (asserted implicitly - `found` above
        // would otherwise produce a hit).
        let result = check(&settings, &input, None, &store, None);
        assert_eq!(result, None);
    }

    // -- DB check: hit, translations dropped -------------------------------

    #[test]
    fn db_hit_copies_segments_and_drops_translations() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");

        let store = FakeStore {
            found: Some(TranscriptionId::new()),
            segments: vec![Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "db text".to_string(),
                translation: "translated text".to_string(),
            }],
            ..FakeStore::default()
        };
        let settings = enabled_skip(false, true);
        let result = check(&settings, &input, None, &store, None).expect("db hit expected");
        assert_eq!(result, vec![seg(0, 1000, "db text")]);
    }

    #[test]
    fn db_miss_when_no_completed_task_matches() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");

        let store = FakeStore {
            found: None,
            ..FakeStore::default()
        };
        let settings = enabled_skip(false, true);
        assert_eq!(check(&settings, &input, None, &store, None), None);
    }

    // -- store-error tolerance ----------------------------------------------

    #[test]
    fn find_completed_error_is_tolerated_as_a_miss() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");

        let store = FakeStore {
            find_fails: true,
            ..FakeStore::default()
        };
        let settings = enabled_skip(false, true);
        assert_eq!(check(&settings, &input, None, &store, None), None);
    }

    #[test]
    fn segments_for_error_is_tolerated_as_a_miss() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");

        let store = FakeStore {
            found: Some(TranscriptionId::new()),
            segments_fail: true,
            ..FakeStore::default()
        };
        let settings = enabled_skip(false, true);
        assert_eq!(check(&settings, &input, None, &store, None), None);
    }

    // -- result-file hit falls through to DB check when files miss --------

    #[test]
    fn result_file_miss_falls_through_to_db_check() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        // No sibling result file exists.

        let store = FakeStore {
            found: Some(TranscriptionId::new()),
            segments: vec![seg(0, 500, "from db")],
            ..FakeStore::default()
        };
        let settings = enabled_skip(true, true);
        let result =
            check(&settings, &input, None, &store, None).expect("db fallback hit expected");
        assert_eq!(result, vec![seg(0, 500, "from db")]);
    }

    #[test]
    fn url_task_matches_the_stored_url_not_the_download_name() {
        let store = FakeStore {
            found: Some(TranscriptionId::new()),
            found_url: Some(TranscriptionId::new()),
            segments: vec![seg(0, 1000, "from the url")],
            ..FakeStore::default()
        };
        let settings = enabled_skip(true, true);
        let downloaded = Path::new("/tmp/media/source.mp3");
        let hit = check(
            &settings,
            downloaded,
            None,
            &store,
            Some("https://example.com/watch?v=1"),
        )
        .expect("url hit");
        assert_eq!(hit, vec![seg(0, 1000, "from the url")]);

        let files_only = enabled_skip(true, false);
        assert_eq!(
            check(
                &files_only,
                downloaded,
                None,
                &store,
                Some("https://example.com/watch?v=1"),
            ),
            None,
            "a URL task does not fall through to result files"
        );
    }
}
