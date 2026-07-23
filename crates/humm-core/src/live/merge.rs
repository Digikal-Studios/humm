//! Append-and-correct text merge + display-mode panel application, ported
//! from Buzz's `RecordingTranscriberWidget`
//! (`buzz/widgets/recording_transcriber_widget.py`):
//!
//! - [`filter_text`] -- `:778-784` (`filter_text` static method) plus the
//!   `REAL_CHARS_REGEX` compiled at `:59`.
//! - [`find_common_part`] -- `:884-904` (DP longest-common-substring, `>= 5`
//!   gate).
//! - [`merge_text_no_overlap`] -- `:906-913` (longest suffix-of-A =
//!   prefix-of-B overlap elision).
//! - [`no_space_between_sentences`] -- the `NO_SPACE_BETWEEN_SENTENCES`
//!   regex compiled at `:60` and applied at `:950`.
//! - [`MergeState::push`] -- `:915-961` (`process_transcription_merge`),
//!   reproducing the trim-adjacent-pairs / hide-unconfirmed / merge-fold /
//!   sentence-space pipeline. `self.texts`/`self.hide_unconfirmed` become
//!   `MergeState`'s stored field and a per-call parameter respectively; the
//!   `text_box`/`export_file` GUI and export side effects are the caller's
//!   concern (`live::export`, a later task; the GUI itself), not this pure
//!   function's -- `push` returns the merged display string in their place.
//! - [`PanelText::apply`] -- the append-below/append-above branches of
//!   `on_next_transcription` (`:963-1013`); append-and-correct's branch just
//!   calls `process_transcription_merge` and hands its result to the panel,
//!   which is exactly what a caller does with [`MergeState::push`]'s return
//!   value here too.
//!
//! **Not ported**: `strip_newlines` (`:773-775`). Buzz applies it only to
//! translation text (`on_next_translation`, not `on_next_transcription`) --
//! translation is M6 (F-063); this module is the transcript path only, so
//! there is nothing here for it to apply to yet. Revisit when `live::merge`
//! (or a sibling) grows a translation-panel counterpart.
//!
//! ## Python `str` indexing is by code point, not byte
//!
//! Every Buzz function ported here indexes/slices Python `str` values, which
//! are sequences of Unicode code points -- `text1[i-1]`, `text1[-i:]`,
//! `text.rfind(sub)`/`text.find(sub)` (returning a code-point start index)
//! all operate at that granularity. Rust `&str` indexing is byte-offset
//! based, so every port here works over `Vec<char>` (or char-index helpers)
//! rather than raw byte slicing -- `char` is not a perfect stand-in for a
//! Python code point either (a `char` is a Unicode scalar value, which
//! excludes surrogate halves Python can technically represent in narrow
//! builds, but matches CPython 3's normal flexible-representation `str`
//! for every fixture and realistic transcript text used here). The
//! differential fixtures include a CJK/punctuation case
//! (`merge-cjk-unicode-sequence.json`) specifically to pin this.
//!
//! ## `\w`/`.strip()` Unicode divergence (documented, not fixed)
//!
//! [`filter_text`] uses `str::trim`, whose notion of whitespace is Rust's
//! `char::is_whitespace` (the Unicode `White_Space` property). Python's
//! `str.strip()` uses `str.isspace()`, a very similar but not
//! byte-for-byte-identical Unicode whitespace set. No practical transcript
//! text (ASR output, in any language) is expected to differ under the two
//! definitions; this is noted rather than chased further. The regex crate's
//! `\w` (used by both [`filter_text`]'s gate and unaffected by this note) is
//! Unicode-aware by default and matches Python `re`'s Unicode-default `\w`
//! for the practical cases this module is exercised against.

use std::sync::OnceLock;

use regex::Regex;

use crate::live::RecordingMode;

/// Buzz's `REAL_CHARS_REGEX` (`:59`): matches a single Unicode word
/// character. Compiled once and reused -- the pattern is fixed, not user
/// input.
fn real_chars_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\w").expect(r"\w is a valid regex"))
}

/// Buzz's `NO_SPACE_BETWEEN_SENTENCES` (`:60`): a sentence-ending
/// punctuation mark (ASCII or CJK) directly followed by an uppercase Latin
/// letter, with no space between them.
fn no_space_between_sentences_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"([.!?。！？])([A-Z])").expect("NO_SPACE_BETWEEN_SENTENCES is a valid regex")
    })
}

/// Ported from `filter_text` (`:778-784`): trims `text`, then discards it
/// (returns `""`) unless what remains contains at least one Unicode word
/// character. See the module doc's `\w`/`.strip()` note for the one
/// documented divergence from Python's exact semantics.
pub fn filter_text(text: &str) -> String {
    let trimmed = text.trim();
    if !real_chars_regex().is_match(trimmed) {
        return String::new();
    }
    trimmed.to_string()
}

/// Ported from `find_common_part` (`:884-904`): the dynamic-programming
/// longest-common-substring between `text1` and `text2`, by Unicode code
/// point. Returns `""` unless the match is at least 5 code points long.
///
/// Buzz's iteration order matters for tie-breaking: the outer loop runs `i`
/// over `text1`'s code points (1-indexed), the inner loop `j` over `text2`'s,
/// and a candidate only replaces the running best on a **strictly greater**
/// length (`lcsuff[i][j] > max_len`) -- so among multiple maximal-length
/// common substrings, the first one reached in that row-major `(i, j)`
/// order wins (equivalently, the smallest `end_index`/earliest-ending match
/// in `text1`). This port preserves that exact iteration and comparison.
pub fn find_common_part(text1: &str, text2: &str) -> String {
    let chars1: Vec<char> = text1.chars().collect();
    let chars2: Vec<char> = text2.chars().collect();
    let len1 = chars1.len();
    let len2 = chars2.len();

    let mut max_len = 0usize;
    let mut end_index = 0usize;

    // lcsuff[i][j]: length of the longest common suffix of chars1[..i] and
    // chars2[..j] (1-indexed against the code-point vectors, matching
    // Buzz's 1-indexed `range(1, len+1)` loops over `text1`/`text2`).
    let mut lcsuff = vec![vec![0usize; len2 + 1]; len1 + 1];

    for i in 1..=len1 {
        for j in 1..=len2 {
            if chars1[i - 1] == chars2[j - 1] {
                lcsuff[i][j] = lcsuff[i - 1][j - 1] + 1;
                if lcsuff[i][j] > max_len {
                    max_len = lcsuff[i][j];
                    end_index = i;
                }
            } else {
                lcsuff[i][j] = 0;
            }
        }
    }

    let common_part: String = chars1[end_index - max_len..end_index].iter().collect();

    if common_part.chars().count() >= 5 {
        common_part
    } else {
        String::new()
    }
}

/// Ported from `merge_text_no_overlap` (`:906-913`): finds the longest `i`
/// (by Unicode code point, `1..=min(len(text1), len(text2))`) such that
/// `text1`'s last `i` code points equal `text2`'s first `i` code points --
/// the loop keeps overwriting `overlap_start` on every match, so the
/// *largest* such `i` wins, not the first -- then returns `text1` with that
/// overlapping prefix of `text2` elided.
pub fn merge_text_no_overlap(text1: &str, text2: &str) -> String {
    let chars1: Vec<char> = text1.chars().collect();
    let chars2: Vec<char> = text2.chars().collect();
    let min_len = chars1.len().min(chars2.len());

    let mut overlap_start = 0usize;
    for i in 1..=min_len {
        let suffix = &chars1[chars1.len() - i..];
        let prefix = &chars2[..i];
        if suffix == prefix {
            overlap_start = i;
        }
    }

    let mut result = text1.to_string();
    result.extend(&chars2[overlap_start..]);
    result
}

/// Ported from Buzz's `NO_SPACE_BETWEEN_SENTENCES.sub(r'\1 \2', ...)`
/// (`:950`): inserts a space between a sentence-ending punctuation mark and
/// a directly-following uppercase Latin letter.
pub fn no_space_between_sentences(text: &str) -> String {
    no_space_between_sentences_regex()
        .replace_all(text, "$1 $2")
        .into_owned()
}

/// The append-and-correct mode's rolling merge state: Buzz's `self.texts`
/// list (`process_transcription_merge`, `:915-961`), reproduced exactly
/// including its cross-call mutation of already-pushed entries.
#[derive(Debug, Clone, Default)]
pub struct MergeState {
    texts: Vec<String>,
}

impl MergeState {
    /// An empty merge state (Buzz: a freshly-started recording's
    /// `self.transcripts = []`).
    pub fn new() -> Self {
        MergeState::default()
    }

    /// The current raw (trimmed-by-overlap) stored texts, in push order --
    /// exposed for tests pinning the exact cross-call mutation Buzz performs
    /// on `self.texts`; production callers only need [`Self::push`]'s
    /// return value.
    pub fn texts(&self) -> &[String] {
        &self.texts
    }

    /// Ported from `process_transcription_merge` (`:915-961`). Appends
    /// `text` to the stored texts, then:
    ///
    /// 1. For each adjacent pair `(texts[i], texts[i+1])` in the *entire*
    ///    stored list (every call re-walks from the start -- Buzz's own
    ///    `for i in range(len(texts) - 1)` does this unconditionally),
    ///    [`find_common_part`] is computed; a non-empty result trims
    ///    `texts[i]` to end at that common part's **last** occurrence
    ///    within it (`rfind`) and `texts[i+1]` to start at its **first**
    ///    occurrence within it (`find`) -- mutating the stored entries in
    ///    place, so earlier trims compound across calls.
    /// 2. `last_common_length` is set only by the *final* pair
    ///    (`i == texts.len() - 2`): `Some(common_part's length)` when that
    ///    pair had a common part, `None` when it did not (explicitly reset,
    ///    not left over from a previous call).
    /// 3. When `hide_unconfirmed` and there is more than one stored text, a
    ///    **display copy** (the stored texts themselves are never truncated
    ///    for this) has its last entry truncated to `last_common_length`
    ///    code points, or dropped entirely when `last_common_length` is
    ///    `None`.
    /// 4. The display list is folded left-to-right through
    ///    [`merge_text_no_overlap`] starting from `""`, then
    ///    [`no_space_between_sentences`] is applied once to the result,
    ///    which is returned.
    pub fn push(&mut self, text: &str, hide_unconfirmed: bool) -> String {
        self.texts.push(text.to_string());

        // `len(texts) - 1` in Buzz; safe unsigned subtraction since we just
        // pushed, so `self.texts.len() >= 1`.
        let pair_count = self.texts.len() - 1;
        let mut last_common_length: Option<usize> = None;

        for i in 0..pair_count {
            let common_part = find_common_part(&self.texts[i], &self.texts[i + 1]);
            if !common_part.is_empty() {
                let common_chars: Vec<char> = common_part.chars().collect();
                let common_length = common_chars.len();

                let chars_i: Vec<char> = self.texts[i].chars().collect();
                let rfind_index = char_rfind(&chars_i, &common_chars).expect(
                    "common_part is a substring of texts[i] by construction \
                     (find_common_part's DP matched it against texts[i] itself)",
                );
                self.texts[i] = chars_i[..rfind_index + common_length].iter().collect();

                let chars_next: Vec<char> = self.texts[i + 1].chars().collect();
                let find_index = char_find(&chars_next, &common_chars).expect(
                    "common_part is a substring of texts[i + 1] by construction \
                     (find_common_part's DP matched it against texts[i + 1] itself)",
                );
                self.texts[i + 1] = chars_next[find_index..].iter().collect();

                if i == pair_count - 1 {
                    last_common_length = Some(common_length);
                }
            } else if i == pair_count - 1 {
                last_common_length = None;
            }
        }

        let mut display_texts = self.texts.clone();
        if hide_unconfirmed && display_texts.len() > 1 {
            match last_common_length {
                Some(keep) => {
                    let last = display_texts
                        .last_mut()
                        .expect("display_texts.len() > 1 checked above");
                    *last = last.chars().take(keep).collect();
                }
                None => {
                    display_texts.pop();
                }
            }
        }

        let mut merged_texts = String::new();
        for text in &display_texts {
            merged_texts = merge_text_no_overlap(&merged_texts, text);
        }

        no_space_between_sentences(&merged_texts)
    }
}

/// Char-index analogue of Python's `str.find(sub)`: the code-point index of
/// `needle`'s first occurrence in `haystack`, or `None` if absent (matching
/// Python's `"".find("")  == 0` for an empty `needle`, though every call
/// site here only ever passes a non-empty one -- `find_common_part` never
/// returns a common part shorter than the 5-code-point gate).
fn char_find(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|&start| &haystack[start..start + needle.len()] == needle)
}

/// Char-index analogue of Python's `str.rfind(sub)`: the code-point index
/// of `needle`'s *last* occurrence in `haystack`, or `None` if absent
/// (matching Python's `"".rfind("") == len(haystack)` for an empty
/// `needle`, per the same never-empty-in-practice note as [`char_find`]).
fn char_rfind(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .rev()
        .find(|&start| &haystack[start..start + needle.len()] == needle)
}

/// The live transcript panel's plain text, with Buzz's append-below /
/// append-above display-mode rules as one pure mutator
/// (`on_next_transcription`, `:963-1013`'s first two branches -- GUI/export
/// side effects excluded, this only computes the resulting panel string).
/// Cursor placement (`moveCursor` to `End`/`Start`) is GUI presentation, not
/// text content, so it is not reproduced here; [`RecordingMode`] alone
/// tells the GUI which end to place the cursor at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelText(String);

impl PanelText {
    /// An empty panel (Buzz: a freshly `.clear()`ed `transcription_text_box`).
    pub fn new() -> Self {
        PanelText::default()
    }

    /// The panel's current plain text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Applies one incoming `text` under `mode`, replacing the panel
    /// content in place.
    ///
    /// - [`RecordingMode::AppendBelow`] (`:972-977`): appends `separator`
    ///   first, but only when the panel is already non-empty (Buzz: `if
    ///   len(self.transcription_text_box.toPlainText()) > 0:
    ///   insertPlainText(separator)`), then appends `text`.
    /// - [`RecordingMode::AppendAbove`] (`:985-989`): prepends `text` then
    ///   `separator` **unconditionally** -- Buzz inserts both at the
    ///   document start on every call, even the first, so an empty-panel
    ///   first call still leaves a trailing separator after `text`.
    /// - [`RecordingMode::AppendAndCorrect`] (`:1012-1013`): Buzz calls
    ///   `process_transcription_merge`, which replaces the *whole* panel
    ///   with the merged text ([`MergeState::push`]'s return value) rather
    ///   than appending anything -- so `text` here is expected to already
    ///   be that merged output, and this call becomes a plain assignment
    ///   (`separator` is unused in this branch, matching Buzz, which never
    ///   touches `line_separator` in the append-and-correct branch).
    pub fn apply(&mut self, mode: RecordingMode, text: &str, separator: &str) {
        match mode {
            RecordingMode::AppendBelow => {
                if !self.0.is_empty() {
                    self.0.push_str(separator);
                }
                self.0.push_str(text);
            }
            RecordingMode::AppendAbove => {
                let mut next = String::with_capacity(text.len() + separator.len() + self.0.len());
                next.push_str(text);
                next.push_str(separator);
                next.push_str(&self.0);
                self.0 = next;
            }
            RecordingMode::AppendAndCorrect => {
                self.0 = text.to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- filter_text --------------------------------------------------------

    #[test]
    fn filter_text_trims_surrounding_whitespace() {
        assert_eq!(filter_text("  hello world  "), "hello world");
    }

    #[test]
    fn filter_text_rejects_whitespace_only_as_empty() {
        assert_eq!(filter_text("   \t\n  "), "");
    }

    #[test]
    fn filter_text_rejects_text_with_no_word_chars_as_empty() {
        // Punctuation only, no \w char anywhere.
        assert_eq!(filter_text("... !? ..."), "");
    }

    #[test]
    fn filter_text_keeps_text_with_at_least_one_word_char() {
        assert_eq!(filter_text(" a. "), "a.");
    }

    #[test]
    fn filter_text_keeps_unicode_word_chars() {
        // CJK characters count as \w under Unicode-aware matching.
        assert_eq!(filter_text("  你好  "), "你好");
    }

    // -- find_common_part -----------------------------------------------------

    #[test]
    fn find_common_part_below_five_gate_returns_empty() {
        // Common substring "abcd" is only 4 chars -- below the >=5 gate.
        assert_eq!(find_common_part("xxxabcd", "abcdyyy"), "");
    }

    #[test]
    fn find_common_part_at_five_gate_boundary_returns_match() {
        // Common substring "abcde" is exactly 5 chars -- meets the gate.
        assert_eq!(find_common_part("xxxabcde", "abcdeyyy"), "abcde");
    }

    #[test]
    fn find_common_part_no_match_returns_empty() {
        assert_eq!(find_common_part("hello world", "xyz12345"), "");
    }

    #[test]
    fn find_common_part_prefers_first_maximal_match_on_tie() {
        // Two equal-length (5-char) common substrings exist: "aaaaa" (at
        // the start of text1) and "bbbbb" (later in text1). Buzz's
        // strictly-greater tie-break keeps the first-reached (smallest
        // end_index) one: "aaaaa".
        let text1 = "aaaaaXXXbbbbb";
        let text2 = "bbbbbYYYaaaaa";
        assert_eq!(find_common_part(text1, text2), "aaaaa");
    }

    #[test]
    fn find_common_part_operates_on_code_points_not_bytes() {
        // Multi-byte CJK characters; the common part is 5 code points, each
        // multiple UTF-8 bytes.
        let text1 = "前文你好世界後";
        let text2 = "你好世界後綴文字";
        assert_eq!(find_common_part(text1, text2), "你好世界後");
    }

    // -- merge_text_no_overlap ------------------------------------------------

    #[test]
    fn merge_text_no_overlap_elides_the_longest_suffix_prefix_overlap() {
        assert_eq!(
            merge_text_no_overlap("hello there", "there general kenobi"),
            "hello there general kenobi"
        );
    }

    #[test]
    fn merge_text_no_overlap_with_no_overlap_concatenates() {
        assert_eq!(merge_text_no_overlap("foo", "bar"), "foobar");
    }

    #[test]
    fn merge_text_no_overlap_prefers_the_longest_overlap_not_the_first() {
        // "aa" is a 2-char overlap (suffix of text1 / prefix of text2), but
        // "aaa" (the full text1) is also a suffix-of-text1/prefix-of-text2
        // match at i=3 -- the loop keeps overwriting on every match found,
        // so the largest i (3) wins, eliding the whole overlap.
        assert_eq!(merge_text_no_overlap("aaa", "aaabbb"), "aaabbb");
    }

    #[test]
    fn merge_text_no_overlap_with_empty_first_text_returns_second() {
        assert_eq!(merge_text_no_overlap("", "hello"), "hello");
    }

    #[test]
    fn merge_text_no_overlap_operates_on_code_points_not_bytes() {
        // "世界" (2 code points, 6 bytes) is the overlap.
        assert_eq!(
            merge_text_no_overlap("你好世界", "世界真美"),
            "你好世界真美"
        );
    }

    // -- no_space_between_sentences -------------------------------------------

    #[test]
    fn no_space_between_sentences_inserts_space_after_period() {
        assert_eq!(
            no_space_between_sentences("Hello world.World again"),
            "Hello world. World again"
        );
    }

    #[test]
    fn no_space_between_sentences_handles_cjk_punctuation() {
        assert_eq!(no_space_between_sentences("你好。World"), "你好。 World");
    }

    #[test]
    fn no_space_between_sentences_leaves_already_spaced_text_alone() {
        assert_eq!(no_space_between_sentences("Hello. World"), "Hello. World");
    }

    #[test]
    fn no_space_between_sentences_ignores_lowercase_following_letter() {
        assert_eq!(no_space_between_sentences("e.g.test"), "e.g.test");
    }

    // -- MergeState::push: sequences -----------------------------------------

    #[test]
    fn push_single_chunk_returns_it_unchanged() {
        let mut state = MergeState::new();
        let merged = state.push("hello there", false);
        assert_eq!(merged, "hello there");
        assert_eq!(state.texts(), &["hello there".to_string()]);
    }

    #[test]
    fn push_overlapping_chunks_merge_and_trim_stored_texts() {
        let mut state = MergeState::new();
        state.push("hello there general", false);
        let merged = state.push("there general kenobi", false);

        assert_eq!(merged, "hello there general kenobi");
        // Stored texts are trimmed at the common part boundary.
        assert_eq!(
            state.texts(),
            &[
                "hello there general".to_string(),
                "there general kenobi".to_string(),
            ]
        );
    }

    #[test]
    fn push_no_overlap_chunk_kept_when_hide_unconfirmed_off() {
        let mut state = MergeState::new();
        state.push("hello there general", false);
        let merged = state.push("completely unrelated text", false);

        // No common part >= 5 chars: both entries stay separate and are
        // concatenated (merge_text_no_overlap finds no overlap either).
        assert_eq!(merged, "hello there generalcompletely unrelated text");
    }

    #[test]
    fn push_no_overlap_chunk_dropped_when_hide_unconfirmed_on() {
        let mut state = MergeState::new();
        state.push("hello there general", false);
        let merged = state.push("completely unrelated text", true);

        // hide_unconfirmed with no common part on the last pair drops the
        // last entry from the display entirely.
        assert_eq!(merged, "hello there general");
    }

    #[test]
    fn push_hide_unconfirmed_truncates_last_to_confirmed_overlap() {
        let mut state = MergeState::new();
        state.push("hello there general", false);
        let merged = state.push("there general kenobi", true);

        // "there general" is the common part (13 chars); the last text is
        // truncated to just that confirmed portion for display.
        assert_eq!(merged, "hello there general");
    }

    #[test]
    fn push_hide_unconfirmed_toggle_only_affects_display_not_storage() {
        let mut state = MergeState::new();
        state.push("hello there general", false);
        state.push("there general kenobi", true);
        // A third push with hide_unconfirmed off again; storage from the
        // prior calls' trims persists regardless of the flag's history.
        let merged = state.push("kenobi how are you", false);

        assert_eq!(merged, "hello there general kenobi how are you");
    }

    #[test]
    fn push_first_call_never_hides_since_len_is_one() {
        let mut state = MergeState::new();
        // hide_unconfirmed=true but only one stored text: the `len() > 1`
        // guard means nothing is truncated/dropped.
        let merged = state.push("only chunk so far", true);
        assert_eq!(merged, "only chunk so far");
    }

    #[test]
    fn push_below_five_char_gate_does_not_trim_stored_texts() {
        let mut state = MergeState::new();
        state.push("abcd wxyz", false);
        // The shared "wxyz" is only 4 chars -- below find_common_part's
        // >=5 gate, so the *stored* texts are left untrimmed. The final
        // merge fold still elides the overlap via merge_text_no_overlap,
        // which has no such gate -- that's a separate, ungated step.
        let merged = state.push("wxyz efgh", false);

        assert_eq!(merged, "abcd wxyz efgh");
        assert_eq!(
            state.texts(),
            &["abcd wxyz".to_string(), "wxyz efgh".to_string()]
        );
    }

    #[test]
    fn push_applies_sentence_space_regex_to_final_merge() {
        let mut state = MergeState::new();
        state.push("Talk about testing.Test", false);
        let merged = state.push("Test again works well", false);

        // "Test" is only a 4-char common part -- below find_common_part's
        // gate, so the stored texts aren't trimmed. merge_text_no_overlap's
        // ungated fold still elides it, leaving "testing.Test" concatenated
        // into the merged string, which the sentence-space regex then
        // splits into "testing. Test".
        assert_eq!(merged, "Talk about testing. Test again works well");
    }

    #[test]
    fn push_three_chunk_sequence_compounds_trims_across_calls() {
        let mut state = MergeState::new();
        state.push("one two three four five", false);
        state.push("three four five six seven", false);
        let merged = state.push("five six seven eight nine", false);

        assert_eq!(merged, "one two three four five six seven eight nine");
    }

    // -- PanelText::apply ------------------------------------------------------

    #[test]
    fn panel_append_below_first_call_has_no_leading_separator() {
        let mut panel = PanelText::new();
        panel.apply(RecordingMode::AppendBelow, "first", "\n\n");
        assert_eq!(panel.as_str(), "first");
    }

    #[test]
    fn panel_append_below_second_call_inserts_separator() {
        let mut panel = PanelText::new();
        panel.apply(RecordingMode::AppendBelow, "first", "\n\n");
        panel.apply(RecordingMode::AppendBelow, "second", "\n\n");
        assert_eq!(panel.as_str(), "first\n\nsecond");
    }

    #[test]
    fn panel_append_above_first_call_still_appends_separator_after_text() {
        let mut panel = PanelText::new();
        panel.apply(RecordingMode::AppendAbove, "first", "\n\n");
        // Buzz inserts text then separator unconditionally, even when the
        // panel starts empty -- so a trailing separator remains.
        assert_eq!(panel.as_str(), "first\n\n");
    }

    #[test]
    fn panel_append_above_second_call_prepends_new_text_above() {
        let mut panel = PanelText::new();
        panel.apply(RecordingMode::AppendAbove, "first", "\n\n");
        panel.apply(RecordingMode::AppendAbove, "second", "\n\n");
        assert_eq!(panel.as_str(), "second\n\nfirst\n\n");
    }

    #[test]
    fn panel_append_and_correct_replaces_whole_panel() {
        let mut panel = PanelText::new();
        panel.apply(RecordingMode::AppendAndCorrect, "stale merged text", "\n\n");
        panel.apply(RecordingMode::AppendAndCorrect, "fresh merged text", "\n\n");
        assert_eq!(panel.as_str(), "fresh merged text");
    }

    // -- differential fixtures (real Python 3.9, copied verbatim algorithm) --

    #[derive(serde::Deserialize)]
    struct MergeStep {
        push: String,
        hide_unconfirmed: bool,
        texts_after: Vec<String>,
        merged: String,
    }

    #[derive(serde::Deserialize)]
    struct MergeFixture {
        name: String,
        steps: Vec<MergeStep>,
    }

    /// Every `crates/humm-core/tests/fixtures/live/merge-*.json` fixture,
    /// generated by `validation/m5-differential/gen_merge_fixtures.py`
    /// running Buzz's `find_common_part`/`merge_text_no_overlap`/
    /// `process_transcription_merge` (copied verbatim from
    /// `recording_transcriber_widget.py:59-60,778-784,884-961`) against real
    /// Python 3, over realistic chunk-sequence scenarios: overlapping ASR
    /// chunk shapes, hide-unconfirmed on/off/toggled mid-sequence, a
    /// no-overlap chunk (kept vs. dropped), CJK/unicode text exercising
    /// code-point indexing and the 。！？ punctuation class, the >=5 gate's
    /// boundary and below-boundary cases, the sentence-space regex firing
    /// on a real merge, and identical consecutive chunks.
    const MERGE_FIXTURES: &[(&str, &str)] = &[
        (
            "overlapping-transcript-realistic",
            include_str!("../../tests/fixtures/live/merge-overlapping-transcript-realistic.json"),
        ),
        (
            "overlapping-transcript-hide-unconfirmed",
            include_str!(
                "../../tests/fixtures/live/merge-overlapping-transcript-hide-unconfirmed.json"
            ),
        ),
        (
            "hide-unconfirmed-toggle-mid-sequence",
            include_str!(
                "../../tests/fixtures/live/merge-hide-unconfirmed-toggle-mid-sequence.json"
            ),
        ),
        (
            "no-overlap-chunk-kept-when-hide-off",
            include_str!(
                "../../tests/fixtures/live/merge-no-overlap-chunk-kept-when-hide-off.json"
            ),
        ),
        (
            "no-overlap-chunk-dropped-when-hide-on",
            include_str!(
                "../../tests/fixtures/live/merge-no-overlap-chunk-dropped-when-hide-on.json"
            ),
        ),
        (
            "cjk-unicode-sequence",
            include_str!("../../tests/fixtures/live/merge-cjk-unicode-sequence.json"),
        ),
        (
            "short-overlaps-below-five-gate",
            include_str!("../../tests/fixtures/live/merge-short-overlaps-below-five-gate.json"),
        ),
        (
            "exact-five-char-gate-boundary",
            include_str!("../../tests/fixtures/live/merge-exact-five-char-gate-boundary.json"),
        ),
        (
            "sentence-space-regex-trigger",
            include_str!("../../tests/fixtures/live/merge-sentence-space-regex-trigger.json"),
        ),
        (
            "identical-consecutive-chunks",
            include_str!("../../tests/fixtures/live/merge-identical-consecutive-chunks.json"),
        ),
    ];

    #[test]
    fn merge_matches_python_reference_fixtures() {
        assert_eq!(
            MERGE_FIXTURES.len(),
            10,
            "expected 10 differential fixtures"
        );

        for (name, json) in MERGE_FIXTURES {
            let fixture: MergeFixture =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("fixture {name} JSON: {e}"));
            assert_eq!(&fixture.name, name, "fixture file/name mismatch");

            let mut state = MergeState::new();
            for (i, step) in fixture.steps.iter().enumerate() {
                let merged = state.push(&step.push, step.hide_unconfirmed);
                assert_eq!(
                    merged, step.merged,
                    "fixture {name} step {i}: merged display mismatch"
                );
                assert_eq!(
                    state.texts(),
                    step.texts_after.as_slice(),
                    "fixture {name} step {i}: stored texts mismatch"
                );
            }
        }
    }
}
