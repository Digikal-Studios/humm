//! Live **translation** application (F-063, spec §7.4), ported from Buzz's
//! `RecordingTranscriberWidget`
//! (`buzz/widgets/recording_transcriber_widget.py`):
//!
//! - [`strip_newlines`] -- `:773-775` (the `strip_newlines` static method).
//! - [`TranslationApplier`] -- `on_next_translation` (`:1037-1081`), the
//!   translation-side twin of `on_next_transcription`. It is the mirror of
//!   the transcript-side `SessionApplier` (humm-gui `live_session.rs`), reusing
//!   the exact same [`PanelText`]/[`MergeState`] display machinery
//!   ([`crate::live::merge`]) and the exact same [`ExportSink`] writers
//!   ([`crate::live::export`]) -- only the emptiness rule, the
//!   [`strip_newlines`] step, and the target export file differ.
//!
//! ## What the transcript side does that the translation side does NOT
//!
//! `on_next_transcription` runs `filter_text` first (`:964`) and skips on an
//! empty *filtered* result. `on_next_translation` does **no** `filter_text`:
//! it skips only on `len(text) == 0` (`:1038-1039`) -- a raw string that is
//! all whitespace, or all punctuation, is still applied. [`TranslationApplier`]
//! reproduces exactly that (`raw.is_empty()`), so a `" "` translation lands
//! in the panel/export where the same transcript would have been dropped.
//!
//! There is also no upload for translations (Buzz's `on_next_translation`
//! has no `requests.post`), and -- unlike Buzz, which persists nothing live
//! at all -- humm persists the translation onto the segment row (Q-10 ✚,
//! ledger #14). That store write, like the enqueue, is the caller's concern
//! (the GUI wiring, a later task): this applier is the pure panel + export
//! consumer, exactly as [`crate::live::merge`]/[`crate::live::export`] are
//! pure of the store/upload/thread plumbing.

use crate::live::RecordingMode;
use crate::live::export::ExportSink;
use crate::live::merge::{MergeState, PanelText};

/// Python's `os.linesep`: the OS-native line separator, `"\r\n"` on Windows
/// and `"\n"` on every Unix (macOS included). [`strip_newlines`] replaces
/// both `"\r\n"` and bare `"\n"` with this, so the two are identical on
/// Unix (an effective `"\r\n" -> "\n"` collapse) and only diverge on the
/// Windows build -- where the two-step order Buzz uses matters (see
/// [`strip_newlines`]).
#[cfg(windows)]
const OS_LINESEP: &str = "\r\n";
/// See the Windows arm.
#[cfg(not(windows))]
const OS_LINESEP: &str = "\n";

/// Ported byte-for-byte from Buzz's `strip_newlines` (`:773-775`):
///
/// ```python
/// return text.replace('\r\n', os.linesep).replace('\n', os.linesep)
/// ```
///
/// Two replaces, **in this order**: first every `"\r\n"` becomes
/// [`OS_LINESEP`], then every remaining bare `"\n"` becomes [`OS_LINESEP`].
/// The order is load-bearing on Windows (where `os.linesep == "\r\n"`): the
/// second replace also rewrites the `"\n"` inside the `"\r\n"` the first
/// step just produced, so a lone input `"\r\n"` ends up `"\r\r\n"` there --
/// Buzz's real behavior, faithfully reproduced. On Unix (`os.linesep ==
/// "\n"`) the second replace is a no-op and the net effect is simply
/// `"\r\n" -> "\n"`, with bare `"\n"` unchanged.
pub fn strip_newlines(text: &str) -> String {
    replace_newlines(text, OS_LINESEP)
}

/// Buzz's two-step newline rewrite with an explicit `os.linesep`.
///
/// `linesep == "\n"` is every Unix, including macOS. `linesep == "\r\n"` is
/// Windows, and the second replace rewrites the `\n` inside the `\r\n` the
/// first step just wrote, so an input `"\r\n"` becomes `"\r\r\n"`.
pub fn replace_newlines(text: &str, linesep: &str) -> String {
    text.replace("\r\n", linesep).replace('\n', linesep)
}

/// The live-translation consumer: the translation-side twin of the
/// transcript-side `SessionApplier` (humm-gui `live_session.rs`), owning one
/// session's translation [`PanelText`] and append-and-correct [`MergeState`]
/// and applying each incoming translation to both the display panel and the
/// `.translated` export sibling.
///
/// Ported from `on_next_translation` (`:1037-1081`). Buzz's `self.translations`
/// list (the append-and-correct history it feeds to
/// `process_transcription_merge`, `:1081`) is owned here by [`MergeState`]
/// -- which encapsulates that very `Vec` and its cross-call trimming --
/// exactly as the transcript side's `SessionApplier` owns its
/// `self.transcripts` through a [`MergeState`] rather than a bare `Vec`.
pub struct TranslationApplier {
    /// The session's display mode -- selects panel placement and which
    /// export writer runs, and is the mode passed to
    /// [`ExportSink::on_translation_utterance`].
    mode: RecordingMode,
    /// The append-below/append-above panel separator (Buzz's
    /// `transcription_options.line_separator`, `:1044,1057`). Unused in
    /// append-and-correct (Buzz never reads `line_separator` there).
    separator: String,
    /// Append-and-correct's hide-unconfirmed setting, forwarded to
    /// [`MergeState::push`].
    hide_unconfirmed: bool,
    /// The translation display panel (Buzz's `translation_text_box`).
    panel: PanelText,
    /// The append-and-correct rolling merge over the translations history
    /// (Buzz's `self.translations` + `process_transcription_merge`).
    merge: MergeState,
}

impl TranslationApplier {
    /// Builds a fresh applier (empty panel, empty merge state) for one
    /// session -- a new Record click always starts from clean state, exactly
    /// like the transcript-side `SessionApplier` (spec §7.4 lifecycle).
    pub fn new(mode: RecordingMode, separator: String, hide_unconfirmed: bool) -> Self {
        TranslationApplier {
            mode,
            separator,
            hide_unconfirmed,
            panel: PanelText::new(),
            merge: MergeState::new(),
        }
    }

    /// The translation panel's current whole text.
    pub fn panel(&self) -> &str {
        self.panel.as_str()
    }

    /// Applies one incoming translation `raw`, returning the panel's new
    /// whole text -- or `None`, with **no** side effects at all, when `raw`
    /// is empty (Buzz `:1038-1039`: `if len(text) == 0: return`; note there
    /// is deliberately no `filter_text` here, unlike the transcript side --
    /// see the module doc).
    ///
    /// Otherwise, dispatched by [`RecordingMode`] exactly as Buzz's
    /// `on_next_translation` (`:1041-1081`):
    ///
    /// - **Append-below** (`:1041-1052`) / **append-above** (`:1054-1078`):
    ///   the **panel** receives `strip_newlines(raw)` with separator
    ///   placement per mode ([`PanelText::apply`]); the **export** receives
    ///   the **raw** (un-stripped) text via
    ///   [`ExportSink::on_translation_utterance`] -- Buzz strips only for the
    ///   text box (`insertPlainText(self.strip_newlines(text))`, `:1045,
    ///   1056`) and writes the untouched `text` to the export file (`:1050,
    ///   1052,1070,1078`).
    /// - **Append-and-correct** (`:1080-1081`): `raw` (un-stripped -- Buzz
    ///   passes `text` straight to `process_transcription_merge`) is pushed
    ///   onto [`MergeState`]; the merged result becomes the whole panel and
    ///   is whole-file-rewritten to the export via
    ///   [`ExportSink::on_translation_merged`].
    ///
    /// `export` is `None` when live export is off for this session; the
    /// panel is still updated and returned either way.
    pub fn apply(&mut self, raw: &str, export: Option<&ExportSink>) -> Option<String> {
        if raw.is_empty() {
            return None;
        }

        match self.mode {
            RecordingMode::AppendBelow | RecordingMode::AppendAbove => {
                self.panel
                    .apply(self.mode, &strip_newlines(raw), &self.separator);
                if let Some(export) = export {
                    export.on_translation_utterance(raw, self.mode);
                }
            }
            RecordingMode::AppendAndCorrect => {
                let merged = self.merge.push(raw, self.hide_unconfirmed);
                self.panel.apply(self.mode, &merged, &self.separator);
                if let Some(export) = export {
                    export.on_translation_merged(&merged);
                }
            }
        }

        Some(self.panel.as_str().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TaskKind;
    use crate::live::{ExportFileType, RealSleeper};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    // -- strip_newlines --------------------------------------------------------

    #[test]
    fn strip_newlines_collapses_crlf_to_os_linesep() {
        // Unix: the first replace turns "\r\n" into "\n"; the second is a no-op.
        // Windows: the first replace writes "\r\n" back, and the second
        // replace rewrites that "\n", so one input pair becomes "\r\r\n".
        let expected = if cfg!(windows) { "a\r\r\nb" } else { "a\nb" };
        assert_eq!(strip_newlines("a\r\nb"), expected);
    }

    #[test]
    fn strip_newlines_maps_bare_lf_to_os_linesep() {
        assert_eq!(strip_newlines("a\nb"), format!("a{OS_LINESEP}b"));
    }

    #[test]
    fn strip_newlines_handles_crlf_and_lf_mixed() {
        // Each "\r\n" and each bare "\n" goes through both replaces. On
        // Windows a "\r\n" becomes "\r\r\n" and a bare "\n" becomes "\r\n".
        // On Unix both become "\n".
        let expected = if cfg!(windows) {
            "one\r\r\ntwo\r\nthree\r\r\nfour"
        } else {
            "one\ntwo\nthree\nfour"
        };
        assert_eq!(strip_newlines("one\r\ntwo\nthree\r\nfour"), expected);
    }

    #[test]
    fn windows_linesep_rewrites_crlf_into_cr_crlf() {
        // The Windows result of Buzz's two replaces. Tested here so a Linux
        // or macOS runner still locks the order; `strip_newlines` itself
        // uses the host separator.
        assert_eq!(replace_newlines("a\r\nb", "\r\n"), "a\r\r\nb");
        assert_eq!(replace_newlines("a\nb", "\r\n"), "a\r\nb");
        assert_eq!(replace_newlines("a\r\nb", "\n"), "a\nb");
    }

    #[test]
    fn strip_newlines_two_step_order_removes_the_cr_of_a_crlf() {
        // Unix: the first replace consumes the whole "\r\n", so the result
        // is "x\ny" rather than a stranded "\r". Windows runs the same two
        // replaces and the second one rewrites the "\n" just written, so
        // the result is "x\r\r\ny".
        let expected = if cfg!(windows) { "x\r\r\ny" } else { "x\ny" };
        assert_eq!(strip_newlines("x\r\ny"), expected);
        #[cfg(not(windows))]
        assert!(!strip_newlines("x\r\ny").contains('\r'));
    }

    #[test]
    fn strip_newlines_leaves_text_without_newlines_untouched() {
        assert_eq!(
            strip_newlines("plain text no breaks"),
            "plain text no breaks"
        );
    }

    // -- apply: emptiness rule (len==0 only, no filter_text) -------------------

    #[test]
    fn apply_empty_raw_returns_none() {
        let mut applier =
            TranslationApplier::new(RecordingMode::AppendBelow, "\n\n".to_string(), false);
        assert_eq!(applier.apply("", None), None);
        assert_eq!(applier.panel(), "");
    }

    #[test]
    fn apply_whitespace_only_raw_is_not_skipped_unlike_the_transcript_side() {
        // Buzz on_next_translation checks only len(text)==0 -- it does NOT
        // filter_text, so a whitespace-only translation IS applied (the
        // transcript side, which filters, would have dropped it).
        let mut applier =
            TranslationApplier::new(RecordingMode::AppendBelow, "\n\n".to_string(), false);
        assert_eq!(applier.apply("   ", None), Some("   ".to_string()));
        assert_eq!(applier.panel(), "   ");
    }

    #[test]
    fn apply_empty_raw_writes_nothing_to_export() {
        let dir = tempfile::tempdir().unwrap();
        let sink = txt_sink(dir.path(), RecordingMode::AppendBelow, 0);

        assert_eq!(
            applier(RecordingMode::AppendBelow).apply("", Some(&sink)),
            None
        );

        assert!(!sink.translated_path().exists());
        assert!(!sink.transcript_path().exists());
    }

    // -- apply: append-below panel placement -----------------------------------

    #[test]
    fn apply_append_below_first_has_no_separator_second_gets_one() {
        let mut applier =
            TranslationApplier::new(RecordingMode::AppendBelow, "\n\n".to_string(), false);

        assert_eq!(applier.apply("hello", None), Some("hello".to_string()));
        assert_eq!(
            applier.apply("world", None),
            Some("hello\n\nworld".to_string())
        );
        assert_eq!(applier.panel(), "hello\n\nworld");
    }

    #[test]
    fn apply_append_below_panel_gets_stripped_newlines() {
        // The panel runs Buzz's two replaces. Unix drops the "\r". Windows
        // turns the same "\r\n" into "\r\r\n".
        let mut applier =
            TranslationApplier::new(RecordingMode::AppendBelow, "\n\n".to_string(), false);
        let expected = if cfg!(windows) {
            "line1\r\r\nline2"
        } else {
            "line1\nline2"
        };
        assert_eq!(
            applier.apply("line1\r\nline2", None),
            Some(expected.to_string())
        );
    }

    // -- apply: append-above panel placement -----------------------------------

    #[test]
    fn apply_append_above_prepends_with_trailing_separator() {
        let mut applier =
            TranslationApplier::new(RecordingMode::AppendAbove, "\n\n".to_string(), false);

        // Buzz inserts text then separator unconditionally, even on the
        // first (empty-panel) call -- a trailing separator remains (:1055-1058).
        assert_eq!(applier.apply("hello", None), Some("hello\n\n".to_string()));
        assert_eq!(
            applier.apply("world", None),
            Some("world\n\nhello\n\n".to_string())
        );
    }

    // -- apply: append-and-correct merge (hand-computed against live::merge) ----

    #[test]
    fn apply_append_and_correct_merges_two_utterances_hide_off() {
        let mut applier =
            TranslationApplier::new(RecordingMode::AppendAndCorrect, "\n\n".to_string(), false);

        // Same overlap case pinned in live::merge's own tests: the two
        // chunks share the >=5-char common part "there general", so the
        // second utterance merges into the first rather than concatenating.
        assert_eq!(
            applier.apply("hello there general", None),
            Some("hello there general".to_string())
        );
        assert_eq!(
            applier.apply("there general kenobi", None),
            Some("hello there general kenobi".to_string())
        );
    }

    #[test]
    fn apply_append_and_correct_hide_unconfirmed_truncates_last_to_overlap() {
        let mut applier =
            TranslationApplier::new(RecordingMode::AppendAndCorrect, "\n\n".to_string(), true);

        applier.apply("hello there general", None);
        // With hide_unconfirmed, the last chunk is shown only up to its
        // confirmed overlap ("there general"), so the merged panel stops at
        // "hello there general".
        assert_eq!(
            applier.apply("there general kenobi", None),
            Some("hello there general".to_string())
        );
    }

    // -- export bytes: all writes hit .translated, never the transcript path ---

    fn applier(mode: RecordingMode) -> TranslationApplier {
        TranslationApplier::new(mode, "\n\n".to_string(), false)
    }

    fn txt_sink(folder: &std::path::Path, mode: RecordingMode, max_entries: usize) -> ExportSink {
        ExportSink::setup_with_sleeper(
            folder,
            ExportFileType::Txt,
            max_entries,
            "\n\n",
            mode,
            None,
            TaskKind::Translate,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        )
    }

    fn csv_sink(folder: &std::path::Path, mode: RecordingMode, max_entries: usize) -> ExportSink {
        ExportSink::setup_with_sleeper(
            folder,
            ExportFileType::Csv,
            max_entries,
            "\n\n",
            mode,
            None,
            TaskKind::Translate,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        )
    }

    fn fixed_now() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 24)
            .unwrap()
            .and_hms_opt(14, 3, 59)
            .unwrap()
    }

    #[test]
    fn export_txt_append_two_utterances_exact_bytes_only_translated() {
        let dir = tempfile::tempdir().unwrap();
        let sink = txt_sink(dir.path(), RecordingMode::AppendBelow, 0);
        let mut app = applier(RecordingMode::AppendBelow);

        app.apply("hello", Some(&sink));
        app.apply("world", Some(&sink));

        assert_eq!(
            std::fs::read_to_string(sink.translated_path()).unwrap(),
            "hello\n\nworld\n\n"
        );
        // The transcript file is the transcript side's alone -- a translation
        // write must never touch it.
        assert!(!sink.transcript_path().exists());
    }

    #[test]
    fn export_txt_append_writes_raw_not_stripped_text() {
        // The panel strips "\r\n"; the export keeps it raw (Buzz :1045 vs
        // :1052). Feed a "\r\n" and prove the file carries the raw bytes.
        let dir = tempfile::tempdir().unwrap();
        let sink = txt_sink(dir.path(), RecordingMode::AppendBelow, 0);
        let mut app = applier(RecordingMode::AppendBelow);

        let panel = app.apply("a\r\nb", Some(&sink)).unwrap();

        let stripped = if cfg!(windows) { "a\r\r\nb" } else { "a\nb" };
        assert_eq!(panel, stripped); // two-step strip in the panel
        assert_eq!(
            std::fs::read_to_string(sink.translated_path()).unwrap(),
            "a\r\nb\n\n" // raw in the export (the "\r" survives)
        );
    }

    #[test]
    fn export_txt_prepend_two_utterances_only_translated() {
        let dir = tempfile::tempdir().unwrap();
        let sink = txt_sink(dir.path(), RecordingMode::AppendAbove, 0);
        let mut app = applier(RecordingMode::AppendAbove);

        app.apply("a", Some(&sink));
        app.apply("b", Some(&sink));

        assert_eq!(
            std::fs::read_to_string(sink.translated_path()).unwrap(),
            "b\n\na\n\n"
        );
        assert!(!sink.transcript_path().exists());
    }

    #[test]
    fn export_csv_append_single_row_caps_to_last_n_only_translated() {
        let dir = tempfile::tempdir().unwrap();
        let sink = csv_sink(dir.path(), RecordingMode::AppendBelow, 2);
        let mut app = applier(RecordingMode::AppendBelow);

        app.apply("a", Some(&sink));
        app.apply("b", Some(&sink));
        app.apply("c", Some(&sink));

        let bytes = std::fs::read(sink.translated_path()).unwrap();
        // utf-8-sig BOM on the append path (Buzz parity, transcript side too).
        assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF]);
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(text.as_bytes());
        let row: Vec<String> = reader
            .records()
            .next()
            .unwrap()
            .unwrap()
            .iter()
            .map(str::to_string)
            .collect();
        assert_eq!(row, vec!["b".to_string(), "c".to_string()]);
        assert!(!sink.transcript_path().exists());
    }

    #[test]
    fn export_and_correct_whole_file_rewrite_only_translated() {
        let dir = tempfile::tempdir().unwrap();
        let sink = txt_sink(dir.path(), RecordingMode::AppendAndCorrect, 0);
        let mut app = applier(RecordingMode::AppendAndCorrect);

        // Two merges: the second whole-file-rewrites the first (no
        // separator suffix -- append-and-correct's export tail).
        app.apply("first draft text", Some(&sink));
        let second = app
            .apply("draft text corrected fully", Some(&sink))
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(sink.translated_path()).unwrap(),
            second
        );
        assert!(!sink.transcript_path().exists());
    }

    #[test]
    fn export_disabled_sink_writes_nothing() {
        // Sink over a non-existent folder -> ExportSink::enabled() == false;
        // every translated writer is a no-op.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        let sink = txt_sink(&missing, RecordingMode::AppendBelow, 0);
        assert!(!sink.enabled());

        let mut app = applier(RecordingMode::AppendBelow);
        app.apply("hello", Some(&sink));

        assert!(!sink.translated_path().exists());
        assert!(!sink.transcript_path().exists());
    }

    // -- snapshot --------------------------------------------------------------

    #[test]
    fn translated_txt_append_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let sink = txt_sink(dir.path(), RecordingMode::AppendBelow, 0);
        let mut app = applier(RecordingMode::AppendBelow);

        app.apply("first translation", Some(&sink));
        app.apply("second translation", Some(&sink));
        app.apply("third translation", Some(&sink));

        insta::assert_snapshot!(std::fs::read_to_string(sink.translated_path()).unwrap());
    }
}
