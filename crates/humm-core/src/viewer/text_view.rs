//! Text/Translation view text builders and the shared paragraph-split env
//! setting.
//!
//! Ported from Buzz's `reset_view` TEXT/TRANSLATION branches
//! (`transcription_viewer_widget.py:1078-1137`).

use crate::domain::{Millis, Segment};
use crate::export::{self, TextKey, write_txt};

/// Builds the Text-mode display string: segments joined `text.trim() + " "`,
/// with a `"\n\n"` paragraph break inserted whenever the gap since the
/// previous segment's end is at least `paragraph_split`, then the whole
/// result trimmed once more.
///
/// Reuses [`crate::export::write_txt`] for the join/paragraph-split logic —
/// identical to Buzz's `reset_view` TEXT branch (`:1096-1106`), which is
/// structurally the same loop as the TXT exporter (`write_output`,
/// `file_transcriber.py:199-211`) plus one final `.strip()`.
pub fn text_mode_text(segments: &[Segment], paragraph_split: Millis) -> String {
    let mut buf = Vec::new();
    write_txt(segments, TextKey::Text, paragraph_split, &mut buf)
        .expect("writing to an in-memory Vec<u8> never fails");
    String::from_utf8(buf)
        .expect("write_txt writes only segment text and ASCII separators — always valid UTF-8")
        .trim()
        .to_string()
}

/// Builds the Translation-mode display string: every segment's
/// `translation.trim()`, joined flat with a single space — **no** paragraph
/// split, unlike Text mode.
///
/// Ported verbatim from Buzz's `reset_view` TRANSLATION branch
/// (`transcription_viewer_widget.py:1118-1120`): a Buzz quirk (the same
/// gap-based break TEXT mode gets is simply never applied here) that is
/// kept for parity, not fixed.
pub fn translation_mode_text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|s| s.translation.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads the paragraph-split gap threshold from `HUMM_PARAGRAPH_SPLIT_TIME`
/// (falling back to [`export::DEFAULT_PARAGRAPH_SPLIT`] when unset or
/// unparseable) — the same constant/env var the TXT exporter uses
/// (`humm-cli`'s `add` command), so Text-mode paragraph breaks always match
/// what a TXT export of the same transcript would produce.
pub fn paragraph_split_from_env() -> Millis {
    paragraph_split_from_env_value(std::env::var("HUMM_PARAGRAPH_SPLIT_TIME").ok().as_deref())
}

fn paragraph_split_from_env_value(value: Option<&str>) -> Millis {
    value
        .and_then(|v| v.parse::<i64>().ok())
        .map(Millis)
        .unwrap_or(export::DEFAULT_PARAGRAPH_SPLIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn seg(start: i64, end: i64, text: &str, translation: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.into(),
            translation: translation.into(),
        }
    }

    #[test]
    fn text_mode_breaks_paragraph_when_gap_meets_split_exactly() {
        let segments = vec![seg(0, 1000, "a", ""), seg(3000, 4000, "b", "")]; // gap == 2000
        assert_eq!(text_mode_text(&segments, Millis(2000)), "a \n\nb");
    }

    #[test]
    fn text_mode_does_not_break_below_split() {
        let segments = vec![seg(0, 1000, "a", ""), seg(2999, 4000, "b", "")]; // gap == 1999
        assert_eq!(text_mode_text(&segments, Millis(2000)), "a b");
    }

    #[test]
    fn text_mode_trims_leading_and_trailing_whitespace() {
        let segments = vec![seg(0, 1000, "  hello  ", "")];
        assert_eq!(text_mode_text(&segments, Millis(2000)), "hello");
    }

    #[test]
    fn text_mode_empty_segments_is_empty_string() {
        assert_eq!(text_mode_text(&[], Millis(2000)), "");
    }

    #[test]
    fn translation_mode_never_breaks_regardless_of_gap() {
        let segments = vec![
            seg(0, 1000, "", "  uno "),
            seg(100_000, 101_000, "", " dos  "),
        ];
        assert_eq!(translation_mode_text(&segments), "uno dos");
    }

    #[test]
    fn paragraph_split_from_env_value_defaults_to_2000_on_absent_or_bogus() {
        assert_eq!(paragraph_split_from_env_value(None), Millis(2000));
        assert_eq!(paragraph_split_from_env_value(Some("bogus")), Millis(2000));
    }

    #[test]
    fn paragraph_split_from_env_value_parses_override() {
        assert_eq!(paragraph_split_from_env_value(Some("500")), Millis(500));
    }
}
