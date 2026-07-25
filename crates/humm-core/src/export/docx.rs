//! F-113 `export_docx`: a byte-faithful OOXML `.docx` writer, ported from
//! `buzz/plugins/export_docx/plugin.py` (whole file read for context — the
//! three template constants at `:35-61`, `_write_docx` at `:126-169`,
//! `_run_xml`/`_paragraph_xml`/`_heading_xml` at `:172-186`).
//!
//! The `.docx` is a zip archive with exactly three entries — a minimal Word
//! document with no external dependency beyond the `zip` crate (mirroring
//! Buzz's own rationale for hand-rolling this instead of pulling in
//! `python-docx`: keeps working without extra binary dependencies).
//!
//! [`write_docx`] is built and unit/snapshot-tested standalone in this task
//! (M7 Task 6) — the post-completion hook that calls it
//! ([`crate::postproc::docx::run`]) is not yet dispatched from the queue
//! (Task 7 wires the chain, design spec §2.1).

use std::io::{Seek, Write};

use zip::write::SimpleFileOptions;

use crate::domain::{Millis, Segment};

use super::to_timestamp;

/// `[Content_Types].xml`, verbatim from `export_docx/plugin.py:35-45`.
const CONTENT_TYPES_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/>"#,
    r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    "</Types>"
);

/// `_rels/.rels`, verbatim from `export_docx/plugin.py:47-54`.
const RELS_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
    "</Relationships>"
);

/// `word/document.xml`, verbatim from `export_docx/plugin.py:56-61` — the
/// `{body}` placeholder is filled in by [`write_docx`] via
/// [`str::replace`], mirroring Buzz's own `_DOCUMENT_TEMPLATE.format(body=…)`
/// (Rust's `format!` macro requires its format string to be a literal, so a
/// runtime placeholder substitution is the faithful equivalent here).
const DOCUMENT_TEMPLATE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    "<w:body>{body}<w:sectPr/></w:body>",
    "</w:document>"
);

/// Escapes `&`, `<`, `>` in `text` for use inside an XML **text node**.
///
/// Mirrors Python's `xml.sax.saxutils.escape` with its default `entities`
/// argument — which Buzz's plugin calls unchanged (`escape(text)`,
/// `plugin.py:174,185`): only `&`, `<`, `>` are replaced. Quotes (`"`, `'`)
/// are deliberately **not** escaped here — `saxutils.escape` only escapes
/// quotes when a caller explicitly passes an `entities` map that includes
/// them (used for XML *attribute* values), which the plugin never does,
/// since every use here is inside a `<w:t>` text node, not an attribute.
/// A single left-to-right character pass (rather than sequential
/// string-replaces) also means `&` is never re-escaped after being turned
/// into `&amp;`.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// One `<w:r>` run: `<w:rPr><w:b/></w:rPr>` when `bold`, then the escaped
/// text inside a `xml:space="preserve"` `<w:t>` (`_run_xml`,
/// `plugin.py:172-174`).
fn run_xml(text: &str, bold: bool) -> String {
    let props = if bold { "<w:rPr><w:b/></w:rPr>" } else { "" };
    format!(
        r#"<w:r>{props}<w:t xml:space="preserve">{}</w:t></w:r>"#,
        escape_text(text)
    )
}

/// One `<w:p>` paragraph wrapping a sequence of `(text, bold)` runs
/// (`_paragraph_xml`, `plugin.py:177-178`).
fn paragraph_xml(runs: &[(&str, bool)]) -> String {
    let mut out = String::from("<w:p>");
    for (text, bold) in runs {
        out.push_str(&run_xml(text, *bold));
    }
    out.push_str("</w:p>");
    out
}

/// The document heading paragraph: `title`, bold, `w:sz`/`w:szCs` 36
/// (18 pt), `w:spacing w:after="200"` (`_heading_xml`, `plugin.py:181-186`).
fn heading_xml(title: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:spacing w:after="200"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="36"/><w:szCs w:val="36"/></w:rPr><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        escape_text(title)
    )
}

/// Builds the `<w:body>` inner XML: the heading paragraph, then either the
/// timestamps-mode runs or the gap-grouped paragraph-mode runs (`_write_docx`,
/// `plugin.py:131-162`).
fn build_body(
    title: &str,
    segments: &[Segment],
    include_timestamps: bool,
    paragraph_split: Millis,
) -> String {
    let mut paragraphs = vec![heading_xml(title)];

    if include_timestamps {
        // Bold `[start --> end] ` stamp (trailing space inside the bold
        // run) + a normal run with the trimmed text, one paragraph per
        // non-empty segment (`plugin.py:133-144`).
        for segment in segments {
            let text = segment.text.trim();
            if text.is_empty() {
                continue;
            }
            let stamp = format!(
                "[{} --> {}] ",
                to_timestamp(segment.start, '.'),
                to_timestamp(segment.end, '.')
            );
            paragraphs.push(paragraph_xml(&[(stamp.as_str(), true), (text, false)]));
        }
    } else {
        // Gap-grouping, ported exactly from `plugin.py:146-162`: a new
        // paragraph starts whenever the gap since the previous segment's
        // end is `>= paragraph_split` *and* the current group already has
        // text — checked *before* the current segment's text is appended.
        // An empty-text segment still advances `previous_end` (it
        // contributes no text to the group but does count toward the next
        // gap), matching Buzz's unconditional `previous_end = segment.end`
        // at the bottom of the loop body regardless of whether `text` was
        // non-empty.
        let mut current: Vec<&str> = Vec::new();
        let mut previous_end: Option<Millis> = None;
        for segment in segments {
            if let Some(prev_end) = previous_end
                && segment.start - prev_end >= paragraph_split
                && !current.is_empty()
            {
                let joined = current.join(" ");
                paragraphs.push(paragraph_xml(&[(joined.as_str(), false)]));
                current.clear();
            }
            let text = segment.text.trim();
            if !text.is_empty() {
                current.push(text);
            }
            previous_end = Some(segment.end);
        }
        if !current.is_empty() {
            let joined = current.join(" ");
            paragraphs.push(paragraph_xml(&[(joined.as_str(), false)]));
        }
    }

    paragraphs.concat()
}

/// Writes a `.docx` (a three-entry, DEFLATE-compressed OOXML zip) to `w`.
///
/// Entries are written in Buzz's exact order (`plugin.py:166-169`):
/// `[Content_Types].xml`, `_rels/.rels`, `word/document.xml`. `title` is
/// both the document heading text and (by convention of every caller) the
/// file's stem. `segments` render either as per-segment bold timestamp
/// stamps (`include_timestamps`) or grouped into paragraphs split on gaps
/// `>= paragraph_split` (see [`build_body`] for the exact semantics of
/// both modes).
///
/// Deterministic by construction: every entry's modified-time is pinned to
/// the zip epoch default (1980-01-01, [`zip::DateTime::default`]) rather
/// than the wall-clock time `zip::write::FileOptions::default()` would
/// otherwise stamp (its `last_modified_time` only reads the real clock
/// when the `zip` crate's `time` feature is enabled — humm's dependency
/// declares `default-features = false, features = ["deflate"]`, which does
/// *not* pull in `time`, so entries are already clock-free today; the
/// explicit call here is a correctness belt-and-braces against that
/// feature ever getting enabled by feature unification elsewhere in the
/// workspace). Same input therefore always produces a byte-identical zip —
/// Buzz's own `zipfile.ZipFile` has no such guarantee (it lets `zipfile`
/// stamp the real wall-clock time), which is a deliberate, harmless
/// deviation: OOXML part *structure* is what parity requires, not the
/// zip container's embedded mtime.
///
/// Zip errors (`zip::result::ZipError`) convert to [`std::io::Error`] via
/// the crate's own `From` impl.
pub fn write_docx(
    w: impl Write + Seek,
    title: &str,
    segments: &[Segment],
    include_timestamps: bool,
    paragraph_split: Millis,
) -> std::io::Result<()> {
    let body = build_body(title, segments, include_timestamps, paragraph_split);
    let document_xml = DOCUMENT_TEMPLATE.replace("{body}", &body);

    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());

    let mut writer = zip::ZipWriter::new(w);

    writer.start_file("[Content_Types].xml", options)?;
    writer.write_all(CONTENT_TYPES_XML.as_bytes())?;

    writer.start_file("_rels/.rels", options)?;
    writer.write_all(RELS_XML.as_bytes())?;

    writer.start_file("word/document.xml", options)?;
    writer.write_all(document_xml.as_bytes())?;

    writer.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn seg(start_ms: i64, end_ms: i64, text: &str) -> Segment {
        Segment {
            start: Millis(start_ms),
            end: Millis(end_ms),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    /// 4 segments spanning 3 gaps: a big gap exactly at the 2000 ms
    /// threshold (pins the `>=` boundary), a gap comfortably under it, and
    /// a normal within-paragraph pair.
    fn multi_gap_fixture() -> Vec<Segment> {
        vec![
            seg(0, 500, "Hello"),
            seg(600, 1000, "world"),
            // Gap since previous end (1000): exactly 2000 -> new paragraph.
            seg(3000, 3500, "Second"),
            seg(3600, 4000, "paragraph"),
        ]
    }

    fn write_to_vec(
        segments: &[Segment],
        include_timestamps: bool,
        paragraph_split: Millis,
    ) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        write_docx(
            &mut buf,
            "transcript",
            segments,
            include_timestamps,
            paragraph_split,
        )
        .unwrap();
        buf.into_inner()
    }

    fn document_xml_of(zip_bytes: &[u8]) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).expect("open zip");
        let mut file = archive.by_name("word/document.xml").expect("document.xml");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut file, &mut xml).expect("read document.xml");
        xml
    }

    // -- snapshot: document.xml, paragraph mode ------------------------------

    #[test]
    fn document_xml_paragraph_mode_snapshot() {
        let bytes = write_to_vec(&multi_gap_fixture(), false, Millis(2000));
        insta::assert_snapshot!(document_xml_of(&bytes));
    }

    // -- snapshot: document.xml, timestamps mode -----------------------------

    #[test]
    fn document_xml_timestamps_mode_snapshot() {
        let bytes = write_to_vec(&multi_gap_fixture(), true, Millis(2000));
        insta::assert_snapshot!(document_xml_of(&bytes));
    }

    // -- snapshot: CJK + escaping-hostile fixture -----------------------------

    #[test]
    fn document_xml_escaping_fixture_snapshot() {
        let segments = vec![
            seg(0, 500, "Tom & Jerry <says> \"hi\" it's <ok>"),
            seg(3000, 3500, "你好，世界 — 日本語のテスト"),
        ];
        let bytes = write_to_vec(&segments, false, Millis(2000));
        insta::assert_snapshot!(document_xml_of(&bytes));
    }

    #[test]
    fn escaping_fixture_quotes_survive_unescaped_in_text_node() {
        let segments = vec![seg(0, 500, "he said \"hi\" and it's fine")];
        let bytes = write_to_vec(&segments, false, Millis(2000));
        let xml = document_xml_of(&bytes);
        assert!(
            xml.contains("he said \"hi\" and it's fine"),
            "quotes/apostrophes must survive unescaped in a text node: {xml}"
        );
    }

    #[test]
    fn escaping_fixture_amp_lt_gt_are_escaped() {
        let segments = vec![seg(0, 500, "Tom & Jerry <says> hi")];
        let bytes = write_to_vec(&segments, false, Millis(2000));
        let xml = document_xml_of(&bytes);
        assert!(xml.contains("Tom &amp; Jerry &lt;says&gt; hi"));
        assert!(!xml.contains("Tom & Jerry"));
    }

    // -- constants: [Content_Types].xml + _rels/.rels ------------------------

    #[test]
    fn content_types_xml_snapshot() {
        insta::assert_snapshot!(CONTENT_TYPES_XML);
    }

    #[test]
    fn rels_xml_snapshot() {
        insta::assert_snapshot!(RELS_XML);
    }

    // -- zip entry name/order -------------------------------------------------

    #[test]
    fn zip_entry_names_and_order() {
        let bytes = write_to_vec(&multi_gap_fixture(), false, Millis(2000));
        let archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");
        let names: Vec<&str> = archive.file_names().collect();
        assert_eq!(
            names,
            vec!["[Content_Types].xml", "_rels/.rels", "word/document.xml"]
        );
    }

    // -- determinism: same input -> byte-identical output --------------------

    #[test]
    fn same_input_produces_byte_identical_zip() {
        let a = write_to_vec(&multi_gap_fixture(), false, Millis(2000));
        let b = write_to_vec(&multi_gap_fixture(), false, Millis(2000));
        assert_eq!(a, b);
    }

    // -- unit: paragraph-mode gap grouping ------------------------------------

    #[test]
    fn gap_exactly_at_threshold_starts_new_paragraph() {
        let segments = vec![seg(0, 1000, "a"), seg(3000, 4000, "b")];
        let xml = document_xml_of(&write_to_vec(&segments, false, Millis(2000)));
        // Two distinct `<w:t>` bodies -- "a" and "b" each get their own
        // paragraph, not "a b" joined into one.
        assert!(xml.contains("<w:t xml:space=\"preserve\">a</w:t>"));
        assert!(xml.contains("<w:t xml:space=\"preserve\">b</w:t>"));
        assert!(!xml.contains("a b"));
    }

    #[test]
    fn gap_just_under_threshold_does_not_split() {
        let segments = vec![seg(0, 1000, "a"), seg(2999, 4000, "b")];
        let xml = document_xml_of(&write_to_vec(&segments, false, Millis(2000)));
        assert!(xml.contains("<w:t xml:space=\"preserve\">a b</w:t>"));
    }

    #[test]
    fn empty_text_segment_updates_previous_end_but_adds_no_text() {
        // The middle segment is empty-text but spans a chunk of time; the
        // gap from *its* end (not the first segment's end) must be what
        // the next segment's split decision uses.
        let segments = vec![
            seg(0, 100, "a"),
            seg(100, 3000, ""),   // empty text, but ends at 3000
            seg(3050, 4000, "b"), // gap from 3000 is only 50ms: no split
        ];
        let xml = document_xml_of(&write_to_vec(&segments, false, Millis(2000)));
        assert!(xml.contains("<w:t xml:space=\"preserve\">a b</w:t>"));
    }

    #[test]
    fn empty_text_segment_gap_can_still_trigger_a_split_for_the_next_segment() {
        let segments = vec![
            seg(0, 100, "a"),
            seg(100, 200, ""),    // empty text, ends at 200
            seg(2300, 3000, "b"), // gap from 200 is 2100ms: split
        ];
        let xml = document_xml_of(&write_to_vec(&segments, false, Millis(2000)));
        assert!(xml.contains("<w:t xml:space=\"preserve\">a</w:t>"));
        assert!(xml.contains("<w:t xml:space=\"preserve\">b</w:t>"));
        assert!(!xml.contains("a b"));
    }

    // -- unit: timestamps-mode stamp format -----------------------------------

    #[test]
    fn timestamps_mode_skips_empty_text_segments() {
        let segments = vec![seg(0, 100, "kept"), seg(200, 300, "   ")];
        let xml = document_xml_of(&write_to_vec(&segments, true, Millis(2000)));
        assert!(xml.contains("kept"));
        // Only one stamp paragraph beyond the heading -- count `<w:p>`.
        assert_eq!(xml.matches("<w:p>").count(), 2);
    }

    #[test]
    fn stamp_format_pinned_against_to_timestamp() {
        // 1h02m03.456s -> 04m05.678s, matching `to_timestamp(_, '.')`'s
        // zero-padded `HH:MM:SS.mmm` format (`file_transcriber.py:232-239`).
        let start = Millis((3600 + 2 * 60 + 3) * 1000 + 456);
        let end = Millis(4 * 60 * 1000 + 5 * 1000 + 678);
        let segments = vec![Segment {
            start,
            end,
            text: "hi".to_string(),
            translation: String::new(),
        }];
        let xml = document_xml_of(&write_to_vec(&segments, true, Millis(2000)));
        // The stamp run is escaped like every other run (Buzz's `_run_xml`
        // calls `escape()` unconditionally on the stamp text too, so the
        // arrow's literal `>` becomes `&gt;` in the XML -- not a bug, a
        // faithful port of that quirk).
        assert!(
            xml.contains("[01:02:03.456 --&gt; 00:04:05.678] "),
            "xml was: {xml}"
        );
    }

    #[test]
    fn timestamps_mode_stamp_is_bold_text_is_not() {
        let segments = vec![seg(0, 100, "hi")];
        let xml = document_xml_of(&write_to_vec(&segments, true, Millis(2000)));
        // The stamp run carries <w:b/>, the text run does not -- verify by
        // finding the two `<w:r>` occurrences within the (single) stamp
        // paragraph.
        let stamp_run_start = xml.find("[00:00:00.000").unwrap();
        let bold_marker = xml[..stamp_run_start].rfind("<w:r>").unwrap();
        assert!(xml[bold_marker..stamp_run_start].contains("<w:b/>"));
    }

    // -- empty segments: heading only ----------------------------------------

    #[test]
    fn empty_segments_produce_heading_only_document() {
        let xml = document_xml_of(&write_to_vec(&[], false, Millis(2000)));
        assert_eq!(xml.matches("<w:p>").count(), 1, "heading paragraph only");
    }
}
