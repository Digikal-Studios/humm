//! Export writers and filename templating.
//!
//! Ported from Buzz's `write_output`/`to_timestamp`
//! (`buzz/transcriber/file_transcriber.py:185-239`) and
//! `get_output_file_path` (`buzz/transcriber/transcriber.py:228-265`).
//! The fixture strings in the test module are byte-law: they are Buzz's own
//! golden tests (`tests/transcriber/transcriber_test.py:12-43`), ported
//! verbatim.

pub mod docx;

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::domain::{Millis, ModelSize, OutputFormat, Segment, TaskKind};
use crate::language::Language;

/// Which segment field a writer reads: the recognized text, or (when
/// translation was requested) the translated text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKey {
    /// Read [`Segment::text`].
    Text,
    /// Read [`Segment::translation`].
    Translation,
}

impl TextKey {
    fn select(self, segment: &Segment) -> &str {
        match self {
            TextKey::Text => &segment.text,
            TextKey::Translation => &segment.translation,
        }
    }
}

/// Formats a millisecond timestamp as `HH:MM:SS<sep>mmm`.
///
/// Hours are zero-padded to at least 2 digits but grow past that for long
/// durations (e.g. `123_456_789` ms formats as `"34:17:36.789"`, not
/// `"34"` wrapping). Ported verbatim from Buzz's `to_timestamp`
/// (`file_transcriber.py:232-239`).
pub fn to_timestamp(ms: Millis, ms_separator: char) -> String {
    let total = ms.0;
    let hr = total / (1000 * 60 * 60);
    let rem = total - hr * (1000 * 60 * 60);
    let min = rem / (1000 * 60);
    let rem = rem - min * (1000 * 60);
    let sec = rem / 1000;
    let ms_part = rem - sec * 1000;
    format!("{hr:02}:{min:02}:{sec:02}{ms_separator}{ms_part:03}")
}

/// Writes segments as plain text.
///
/// Ported from Buzz's `write_output` TXT branch (`file_transcriber.py:199-211`).
/// Consecutive segments are joined by a single trailing space; a `"\n\n"`
/// paragraph break is inserted whenever the gap since the previous
/// segment's end is at least `paragraph_split`. Each segment's text is
/// trimmed before being written (Buzz's `.strip()`); the trailing space
/// after the last segment is part of the contract, not a bug.
pub fn write_txt(
    segments: &[Segment],
    key: TextKey,
    paragraph_split: Millis,
    w: &mut impl Write,
) -> std::io::Result<()> {
    let mut previous_end: Option<Millis> = None;
    for segment in segments {
        if let Some(prev_end) = previous_end
            && segment.start - prev_end >= paragraph_split
        {
            w.write_all(b"\n\n")?;
        }
        write!(w, "{} ", key.select(segment).trim())?;
        previous_end = Some(segment.end);
    }
    Ok(())
}

/// Writes segments as SubRip (`.srt`) subtitles.
///
/// Ported from Buzz's `write_output` SRT branch (`file_transcriber.py:221-227`).
/// Text is written raw (untrimmed) — Buzz strips segment text upstream at
/// task completion, which humm's pipeline replicates (Task 7).
pub fn write_srt(segments: &[Segment], key: TextKey, w: &mut impl Write) -> std::io::Result<()> {
    for (i, segment) in segments.iter().enumerate() {
        writeln!(w, "{}", i + 1)?;
        writeln!(
            w,
            "{} --> {}",
            to_timestamp(segment.start, ','),
            to_timestamp(segment.end, ',')
        )?;
        writeln!(w, "{}", key.select(segment))?;
        writeln!(w)?;
    }
    Ok(())
}

/// Writes segments as WebVTT (`.vtt`) subtitles.
///
/// Ported from Buzz's `write_output` VTT branch (`file_transcriber.py:213-219`).
/// Always emits the `WEBVTT` header (followed by a blank line), even for
/// zero segments. Text is written raw (untrimmed), like [`write_srt`].
pub fn write_vtt(segments: &[Segment], key: TextKey, w: &mut impl Write) -> std::io::Result<()> {
    writeln!(w, "WEBVTT")?;
    writeln!(w)?;
    for segment in segments {
        writeln!(
            w,
            "{} --> {}",
            to_timestamp(segment.start, '.'),
            to_timestamp(segment.end, '.')
        )?;
        writeln!(w, "{}", key.select(segment))?;
        writeln!(w)?;
    }
    Ok(())
}

/// Dispatches to [`write_txt`], [`write_srt`], or [`write_vtt`] by `format`.
pub fn write_format(
    format: OutputFormat,
    segments: &[Segment],
    key: TextKey,
    paragraph_split: Millis,
    w: &mut impl Write,
) -> std::io::Result<()> {
    match format {
        OutputFormat::Txt => write_txt(segments, key, paragraph_split, w),
        OutputFormat::Srt => write_srt(segments, key, w),
        OutputFormat::Vtt => write_vtt(segments, key, w),
    }
}

/// Default export filename template.
///
/// Matches Buzz's stored default (`Settings.get_default_export_file_template`,
/// `settings.py:170-174`).
pub const DEFAULT_EXPORT_TEMPLATE: &str = "{{ input_file_name }} ({{ task }}d on {{ date_time }})";

/// Default gap (ms) between segments that starts a new paragraph in TXT
/// export.
///
/// Matches Buzz's `BUZZ_PARAGRAPH_SPLIT_TIME` default
/// (`file_transcriber.py:203`).
pub const DEFAULT_PARAGRAPH_SPLIT: Millis = Millis(2000);

/// Builds the output file path for one export, applying filename templating.
///
/// Ported from Buzz's `get_output_file_path` (`transcriber.py:250-289`). The
/// URL title (`display_name`) or else the input's file stem is
/// `{{ input_file_name }}`, with a trailing `"_speech"` stripped if present,
/// then [`safe_filename_component`]. `template`
/// falls back to [`DEFAULT_EXPORT_TEMPLATE`]; `output_directory` falls back
/// to the input's parent directory.
///
/// `{{ model_type }}` always renders the literal `"Whisper.cpp"`: humm has a
/// single local inference backend (whisper-rs, itself a whisper.cpp
/// binding), which corresponds to Buzz's `ModelType.WHISPER_CPP` (wire value
/// `"Whisper.cpp"`, `buzz/model_loader.py:179`). Buzz's other backends
/// (`WHISPER`, `FASTER_WHISPER`, `HUGGING_FACE`, `OPEN_AI_WHISPER_API`) are
/// not ported (ADR-008), so no other `{{ model_type }}` value is possible.
///
/// `display_name`, when non-empty, is the URL title (Buzz's `display_name`)
/// and replaces the input stem as `{{ input_file_name }}`. It is not joined
/// raw: [`safe_filename_component`] runs first, then
/// [`confine_export_file_name`] keeps the finished name a single relative
/// component so `Path::join` cannot treat a disk prefix as a new root.
#[allow(clippy::too_many_arguments)]
pub fn output_file_path(
    input: &Path,
    task: TaskKind,
    language: Option<Language>,
    model_size: ModelSize,
    format: OutputFormat,
    output_directory: Option<&Path>,
    template: Option<&str>,
    now: chrono::NaiveDateTime,
    display_name: Option<&str>,
) -> PathBuf {
    let stem_owned = input
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let chosen = display_name
        .filter(|name| !name.is_empty())
        .unwrap_or(stem_owned.as_str());
    // Buzz strips `_speech` before sanitizing, on the title or the stem.
    let without_speech = chosen.strip_suffix("_speech").unwrap_or(chosen);
    let input_file_name = safe_filename_component(without_speech);

    let template = template.unwrap_or(DEFAULT_EXPORT_TEMPLATE);
    let date_time = now.format("%d-%b-%Y %H-%M-%S").to_string();

    let file_name = template
        .replace("{{ input_file_name }}", &input_file_name)
        .replace("{{ task }}", task.as_str())
        .replace("{{ language }}", language.map(Language::code).unwrap_or(""))
        .replace("{{ model_type }}", "Whisper.cpp")
        .replace("{{ model_size }}", model_size.as_str())
        .replace("{{ date_time }}", &date_time)
        + "."
        + format.extension();
    let file_name = confine_export_file_name(&file_name);

    let directory = output_directory.map(Path::to_path_buf).unwrap_or_else(|| {
        input
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf()
    });

    directory.join(file_name)
}

/// One portable filename component, matching Buzz `safe_filename_component`
/// (`paths.py`).
///
/// `<>:"/\|?*` and ASCII controls become `_`. One trailing space or dot
/// becomes `#`. An empty result becomes `transcript`. Windows device names
/// (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`), matched on the
/// stem before the first dot, gain a leading `_`.
pub fn safe_filename_component(name: &str) -> String {
    let mut sanitized: String = name
        .chars()
        .map(|c| {
            if is_windows_invalid_filename_char(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    if sanitized.ends_with(' ') || sanitized.ends_with('.') {
        sanitized.pop();
        sanitized.push('#');
    }
    if sanitized.is_empty() {
        return "transcript".to_string();
    }
    let stem = sanitized.split('.').next().unwrap_or(sanitized.as_str());
    let stem = stem.trim_end_matches([' ', '.']);
    if is_windows_reserved(stem) {
        sanitized.insert(0, '_');
    }
    sanitized
}

/// The finished export name, after templating, as one relative component.
///
/// Buzz's sanitizer already turns `C:/x` into `C__x` (both `:` and `/` are
/// in the invalid class) and `CON` into `_CON`. This repeats that pass when
/// the templated name would still be a Windows disk prefix, a separator, a
/// parent segment, or a reserved device, so `Path::join` keeps the file
/// inside the output directory on Windows as well as on Unix.
pub fn confine_export_file_name(name: &str) -> String {
    if export_file_name_stays_inside(name) {
        name.to_string()
    } else {
        safe_filename_component(name)
    }
}

fn is_windows_invalid_filename_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c <= '\u{1f}'
}

fn is_windows_reserved(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}

/// `true` when `name` is one relative component `Path::join` will not use
/// as a new root. A Windows disk prefix (`C:`) counts even on Unix, where
/// `Path` does not treat it as a prefix.
fn export_file_name_stays_inside(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.contains(['/', '\\', ':']) || name.chars().any(|c| c <= '\u{1f}') {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name);
    let stem = stem.trim_end_matches([' ', '.']);
    !is_windows_reserved(stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    // Ported verbatim from Buzz tests/transcriber/transcriber_test.py:12-43
    #[test]
    fn to_timestamp_matches_buzz() {
        assert_eq!(to_timestamp(Millis(0), '.'), "00:00:00.000");
        assert_eq!(to_timestamp(Millis(123_456_789), '.'), "34:17:36.789");
    }

    fn fixture() -> Vec<Segment> {
        vec![
            Segment {
                start: Millis(40),
                end: Millis(299),
                text: "Bien".into(),
                translation: String::new(),
            },
            Segment {
                start: Millis(299),
                end: Millis(329),
                text: "venue dans".into(),
                translation: String::new(),
            },
        ]
    }

    #[test]
    fn txt_matches_buzz_golden() {
        let mut buf = Vec::new();
        write_txt(&fixture(), TextKey::Text, DEFAULT_PARAGRAPH_SPLIT, &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "Bien venue dans ");
    }

    #[test]
    fn srt_matches_buzz_golden() {
        let mut buf = Vec::new();
        write_srt(&fixture(), TextKey::Text, &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "1\n00:00:00,040 --> 00:00:00,299\nBien\n\n2\n00:00:00,299 --> 00:00:00,329\nvenue dans\n\n"
        );
    }

    #[test]
    fn vtt_matches_buzz_golden() {
        let mut buf = Vec::new();
        write_vtt(&fixture(), TextKey::Text, &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "WEBVTT\n\n00:00:00.040 --> 00:00:00.299\nBien\n\n00:00:00.299 --> 00:00:00.329\nvenue dans\n\n"
        );
    }

    #[test]
    fn txt_paragraph_split_inserted_when_gap_meets_threshold() {
        let segments = vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "a".into(),
                translation: String::new(),
            },
            Segment {
                start: Millis(3000),
                end: Millis(4000),
                text: "b".into(),
                translation: String::new(),
            },
        ];
        let mut buf = Vec::new();
        write_txt(&segments, TextKey::Text, Millis(2000), &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a \n\nb ");
    }

    #[test]
    fn txt_paragraph_split_not_inserted_below_threshold() {
        let segments = vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "a".into(),
                translation: String::new(),
            },
            Segment {
                start: Millis(2999),
                end: Millis(4000),
                text: "b".into(),
                translation: String::new(),
            },
        ];
        let mut buf = Vec::new();
        write_txt(&segments, TextKey::Text, Millis(2000), &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "a b ");
    }

    #[test]
    fn translation_key_selects_translation_field() {
        let segments = vec![
            Segment {
                start: Millis(40),
                end: Millis(299),
                text: "Bien".into(),
                translation: "Wel".into(),
            },
            Segment {
                start: Millis(299),
                end: Millis(329),
                text: "venue dans".into(),
                translation: "come in".into(),
            },
        ];
        let mut buf = Vec::new();
        write_srt(&segments, TextKey::Translation, &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "1\n00:00:00,040 --> 00:00:00,299\nWel\n\n2\n00:00:00,299 --> 00:00:00,329\ncome in\n\n"
        );
    }

    #[test]
    fn empty_segments_produce_format_specific_empty_output() {
        let empty: Vec<Segment> = Vec::new();

        let mut txt = Vec::new();
        write_txt(&empty, TextKey::Text, DEFAULT_PARAGRAPH_SPLIT, &mut txt).unwrap();
        assert_eq!(String::from_utf8(txt).unwrap(), "");

        let mut srt = Vec::new();
        write_srt(&empty, TextKey::Text, &mut srt).unwrap();
        assert_eq!(String::from_utf8(srt).unwrap(), "");

        let mut vtt = Vec::new();
        write_vtt(&empty, TextKey::Text, &mut vtt).unwrap();
        assert_eq!(String::from_utf8(vtt).unwrap(), "WEBVTT\n\n");
    }

    #[test]
    fn srt_writes_untrimmed_text_raw_while_txt_trims() {
        let segments = vec![Segment {
            start: Millis(0),
            end: Millis(100),
            text: " hi ".into(),
            translation: String::new(),
        }];

        let mut srt = Vec::new();
        write_srt(&segments, TextKey::Text, &mut srt).unwrap();
        assert_eq!(
            String::from_utf8(srt).unwrap(),
            "1\n00:00:00,000 --> 00:00:00,100\n hi \n\n"
        );

        let mut txt = Vec::new();
        write_txt(&segments, TextKey::Text, DEFAULT_PARAGRAPH_SPLIT, &mut txt).unwrap();
        assert_eq!(String::from_utf8(txt).unwrap(), "hi ");
    }

    fn fixed_now() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 20)
            .unwrap()
            .and_hms_opt(14, 3, 5)
            .unwrap()
    }

    #[test]
    fn output_file_path_default_template_strips_speech_suffix() {
        let path = output_file_path(
            Path::new("/tmp/interview_speech.mp3"),
            TaskKind::Translate,
            None,
            ModelSize::Small,
            OutputFormat::Srt,
            None,
            None,
            fixed_now(),
            None,
        );
        assert_eq!(
            path,
            Path::new("/tmp/interview (translated on 20-Jul-2026 14-03-05).srt")
        );
    }

    #[test]
    fn output_file_path_custom_template_with_language_and_directory() {
        let path = output_file_path(
            Path::new("/tmp/interview_speech.mp3"),
            TaskKind::Transcribe,
            Language::parse("fr"),
            ModelSize::Small,
            OutputFormat::Txt,
            Some(Path::new("/out")),
            Some("{{ input_file_name }}-{{ model_size }}-{{ language }}"),
            fixed_now(),
            None,
        );
        assert_eq!(path, Path::new("/out/interview-small-fr.txt"));
    }

    #[test]
    fn output_file_path_strips_speech_suffix() {
        let path = output_file_path(
            Path::new("/tmp/interview_speech.mp3"),
            TaskKind::Transcribe,
            None,
            ModelSize::Small,
            OutputFormat::Txt,
            None,
            Some("{{ input_file_name }}"),
            fixed_now(),
            None,
        );
        assert_eq!(path, Path::new("/tmp/interview.txt"));
    }

    #[test]
    fn output_file_path_strips_speech_suffix_for_a_user_file_literally_named_x_speech() {
        // Byte-parity edge case (Buzz `transcriber.py:238-240`): the strip
        // is unconditional on the suffix alone, not on provenance — a
        // user's own file that just happens to be named `*_speech.*` gets
        // the same single strip as a demucs-produced isolation output.
        //
        // The input deliberately has *two* trailing `_speech` occurrences
        // (`interview_speech_speech`) so a single-strip implementation and a
        // loop-until-no-more-suffix implementation disagree on the result —
        // a single `my_song_speech` -> `my_song` case can't tell them apart
        // (review Minor 7). `output_file_path` strips exactly once
        // (`strip_suffix`, not a `while` loop), so only the trailing
        // occurrence goes: `interview_speech_speech` -> `interview_speech`.
        let path = output_file_path(
            Path::new("/tmp/interview_speech_speech.mp3"),
            TaskKind::Transcribe,
            None,
            ModelSize::Small,
            OutputFormat::Txt,
            None,
            Some("{{ input_file_name }}"),
            fixed_now(),
            None,
        );
        assert_eq!(path, Path::new("/tmp/interview_speech.txt"));
    }

    #[test]
    fn output_file_path_model_type_token_renders_whisper_cpp() {
        let path = output_file_path(
            Path::new("/tmp/interview.mp3"),
            TaskKind::Transcribe,
            None,
            ModelSize::Small,
            OutputFormat::Txt,
            None,
            Some("{{ model_type }}"),
            fixed_now(),
            None,
        );
        assert_eq!(path, Path::new("/tmp/Whisper.cpp.txt"));
    }

    #[test]
    fn safe_filename_component_matches_buzz() {
        let cases = [
            ("中文 | QVD-123.", "中文 _ QVD-123#"),
            ("foo:bar?.wav", "foo_bar_.wav"),
            ("trailing-dot.", "trailing-dot#"),
            ("trailing-space ", "trailing-space#"),
            ("emoji-🚀", "emoji-🚀"),
            ("CON", "_CON"),
            ("CON.txt", "_CON.txt"),
            ("AC/DC", "AC_DC"),
            ("../x", ".._x"),
            ("..", ".#"),
            ("C:/x", "C__x"),
            ("", "transcript"),
        ];
        for (name, expected) in cases {
            assert_eq!(safe_filename_component(name), expected, "{name}");
        }
    }

    #[test]
    fn confine_export_file_name_rejects_a_windows_disk_prefix_and_reserved_device() {
        assert_eq!(confine_export_file_name("C:_x.srt"), "C__x.srt");
        assert_eq!(confine_export_file_name("CON.srt"), "_CON.srt");
        assert_eq!(
            confine_export_file_name("interview (translated on 20-Jul-2026 14-03-05).srt"),
            "interview (translated on 20-Jul-2026 14-03-05).srt"
        );
    }

    #[test]
    fn url_titles_stay_a_single_file_inside_the_output_directory() {
        let cases = [
            ("AC/DC", "AC_DC.srt"),
            ("../x", ".._x.srt"),
            ("..", ".#.srt"),
            ("C:/x", "C__x.srt"),
            ("CON", "_CON.srt"),
            ("C:/x ", "C__x#.srt"),
        ];
        let dir = Path::new("/out");
        for (title, file_name) in cases {
            let path = output_file_path(
                Path::new("/tmp/media/id/source.mp3"),
                TaskKind::Transcribe,
                None,
                ModelSize::Small,
                OutputFormat::Srt,
                Some(dir),
                Some("{{ input_file_name }}"),
                fixed_now(),
                Some(title),
            );
            assert_eq!(path, dir.join(file_name), "{title}");
            assert_eq!(path.parent(), Some(dir), "{title}");
            let name = path.file_name().unwrap().to_string_lossy();
            assert!(!name.contains(':'), "{title} -> {name}");
            assert!(!name.contains('/'), "{title}");
            assert!(!name.contains('\\'), "{title}");
        }
    }

    #[test]
    fn write_format_dispatches_by_format() {
        let segments = fixture();

        let mut txt = Vec::new();
        write_format(
            OutputFormat::Txt,
            &segments,
            TextKey::Text,
            DEFAULT_PARAGRAPH_SPLIT,
            &mut txt,
        )
        .unwrap();
        assert_eq!(String::from_utf8(txt).unwrap(), "Bien venue dans ");

        let mut vtt = Vec::new();
        write_format(
            OutputFormat::Vtt,
            &segments,
            TextKey::Text,
            DEFAULT_PARAGRAPH_SPLIT,
            &mut vtt,
        )
        .unwrap();
        assert!(String::from_utf8(vtt).unwrap().starts_with("WEBVTT\n\n"));
    }

    /// 5 synthetic segments: a paragraph gap (seg 2 -> seg 3), a translation
    /// track, and multi-line / untrimmed text — rendered in all three
    /// formats concatenated, to guard drift beyond the tiny Buzz goldens.
    fn snapshot_fixture() -> Vec<Segment> {
        vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "Hello world".into(),
                translation: "Salut monde".into(),
            },
            Segment {
                start: Millis(1200),
                end: Millis(2000),
                text: "second segment".into(),
                translation: "deuxième segment".into(),
            },
            Segment {
                start: Millis(5000),
                end: Millis(6000),
                text: "third\nmultiline segment".into(),
                translation: "troisième\nmultiligne".into(),
            },
            Segment {
                start: Millis(6100),
                end: Millis(7000),
                text: "  padded text  ".into(),
                translation: "  texte rembourré  ".into(),
            },
            Segment {
                start: Millis(7000),
                end: Millis(8000),
                text: "final segment".into(),
                translation: "segment final".into(),
            },
        ]
    }

    #[test]
    fn export_formats_snapshot() {
        let segments = snapshot_fixture();
        let mut out = String::new();

        let mut txt = Vec::new();
        write_txt(&segments, TextKey::Text, DEFAULT_PARAGRAPH_SPLIT, &mut txt).unwrap();
        out.push_str("== TXT ==\n");
        out.push_str(&String::from_utf8(txt).unwrap());
        out.push('\n');

        let mut srt = Vec::new();
        write_srt(&segments, TextKey::Text, &mut srt).unwrap();
        out.push_str("== SRT ==\n");
        out.push_str(&String::from_utf8(srt).unwrap());

        let mut vtt = Vec::new();
        write_vtt(&segments, TextKey::Translation, &mut vtt).unwrap();
        out.push_str("== VTT (translation) ==\n");
        out.push_str(&String::from_utf8(vtt).unwrap());

        insta::assert_snapshot!(out);
    }
}
