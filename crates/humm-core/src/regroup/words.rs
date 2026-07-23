//! Word/segment data model for the regroup port and the Buzz `get_transcript`
//! sentence-grouping step that builds the initial `WhisperResult`-equivalent.
//!
//! Ground truth: `transcription_resizer_widget.py:52-93`
//! (`TranscriptionWorker.get_transcript`) and stable-ts
//! `stable_whisper/result.py` (`WordTiming`, `Segment`, `WhisperResult`).

use crate::domain::{Millis, Segment};

/// Languages that don't put spaces between words — Buzz's
/// `NON_SPACE_LANGUAGES` (`transcription_resizer_widget.py:40`). For these the
/// per-word separator is `""` rather than `" "`.
const NON_SPACE_LANGUAGES: [&str; 6] = ["zh", "ja", "th", "lo", "km", "my"];

/// Whether `language` is one of the six non-spacing languages.
///
/// Buzz reads `self.transcription.language or ""` then tests membership, so
/// `None` (and `Some("")`) map to the spacing default (`" "`).
fn is_non_space_language(language: Option<&str>) -> bool {
    language.is_some_and(|l| NON_SPACE_LANGUAGES.contains(&l))
}

/// One stable-ts `WordTiming`. `text` is the word **including its trailing
/// separator** (`" "`, or `""` for non-spacing languages) exactly as Buzz
/// appends it (`buzz_segment.text + separator`); `start`/`end` are
/// *fake-seconds* (`ms / 100.0` — Buzz's unit quirk, spec §2.6).
///
/// `left_locked`/`right_locked` carry stable-ts's word-level lock flags
/// (crate-internal — the public surface is just `text`/`start`/`end`); they
/// gate later merges/splits when a `_=1` lock argument is used (see
/// [`super::ops`]). Word timings are never arithmetic-combined by the
/// mg/sp/sl subset, so `start`/`end` stay exactly `ms / 100.0` throughout
/// (stable-ts's `round(x, 3)` on load is an identity for 2-decimal
/// fake-seconds — verified over all reachable ms — so it is not reproduced).
#[derive(Debug, Clone, PartialEq)]
pub struct RWord {
    /// Word text including its trailing separator.
    pub text: String,
    /// Start time in fake-seconds (`ms / 100.0`).
    pub start: f64,
    /// End time in fake-seconds (`ms / 100.0`).
    pub end: f64,
    pub(crate) left_locked: bool,
    pub(crate) right_locked: bool,
}

impl RWord {
    /// A word with both locks clear (the state every word starts in).
    pub(crate) fn new(text: String, start: f64, end: f64) -> Self {
        RWord {
            text,
            start,
            end,
            left_locked: false,
            right_locked: false,
        }
    }

    /// `len(WordTiming)` in stable-ts = `len(self.word)` = Python `str` length
    /// = Unicode **code-point** count (`.chars().count()`, not bytes).
    pub(crate) fn char_len(&self) -> usize {
        self.text.chars().count()
    }
}

/// One stable-ts `Segment` (in the merge context, a group of words). `start`,
/// `end`, `text`, and the char/word counts are all **derived from the words**
/// exactly as stable-ts's `Segment` properties are (`result.py:414-464`).
#[derive(Debug, Clone, PartialEq)]
pub struct RSegment {
    /// The segment's words, in order.
    pub words: Vec<RWord>,
}

impl RSegment {
    /// `Segment.start` — first word's start (segments here always have words).
    pub(crate) fn start(&self) -> f64 {
        self.words.first().map_or(0.0, |w| w.start)
    }

    /// `Segment.end` — last word's end.
    pub(crate) fn end(&self) -> f64 {
        self.words.last().map_or(0.0, |w| w.end)
    }

    /// `Segment.text` — `''.join(word.word for word in words)`.
    pub(crate) fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect()
    }

    /// `Segment.char_count()` — `sum(len(w) for w in words)` (code points).
    pub(crate) fn char_count(&self) -> usize {
        self.words.iter().map(RWord::char_len).sum()
    }

    /// `Segment.word_count()` — `len(words)` (always ≥ 1 here).
    pub(crate) fn word_count(&self) -> usize {
        self.words.len()
    }

    /// `Segment.duration` — `end - start` (fake-seconds).
    pub(crate) fn duration(&self) -> f64 {
        self.end() - self.start()
    }

    /// `bool(self.words)` — whether the segment has any words.
    pub(crate) fn has_words(&self) -> bool {
        !self.words.is_empty()
    }

    /// `Segment.left_locked` — `words[0].left_locked`.
    pub(crate) fn left_locked(&self) -> bool {
        self.words.first().is_some_and(|w| w.left_locked)
    }

    /// `Segment.right_locked` — `words[-1].right_locked`.
    pub(crate) fn right_locked(&self) -> bool {
        self.words.last().is_some_and(|w| w.right_locked)
    }
}

/// `ms / 100.0` — the fake-seconds Buzz feeds stable-ts (1 fake-second =
/// 100 ms). Same IEEE-754 `f64` division Python's `start_time / 100`
/// performs, so bit-identical.
pub(crate) fn ms_to_fake(ms: Millis) -> f64 {
    ms.0 as f64 / 100.0
}

/// `int(x * 100)` — Buzz's output truncation (`int(segment.start * 100)`,
/// `transcription_resizer_widget.py:126-127`). `int()` truncates **toward
/// zero**, as do `f64::trunc` and `as i64`; because `x` is a fake-second the
/// round trip is lossy for ~6.5 % of ms values (e.g. `29 → 0.29 →
/// 28.999… → 28`), a Buzz behavior this reproduces exactly.
pub(crate) fn fake_to_ms(x: f64) -> i64 {
    (x * 100.0).trunc() as i64
}

/// Buzz's `SENTENCE_END = re.compile(r'.*[.!?。！？]')` applied with
/// `re.match` to a segment's text (`transcription_resizer_widget.py:75`).
///
/// `re.match` anchors at the start and `.` (outside the class) does **not**
/// match `\n`, so the pattern matches iff the text's first line contains at
/// least one of the six terminators `. ! ? 。 ！ ？`.
fn ends_sentence(text: &str) -> bool {
    text.split('\n')
        .next()
        .unwrap_or("")
        .chars()
        .any(|c| matches!(c, '.' | '!' | '?' | '。' | '！' | '？'))
}

/// Buzz's `get_transcript` word-building + sentence grouping
/// (`transcription_resizer_widget.py:64-93`): each input segment becomes one
/// stable-ts *word* (`text + separator`, `start`/`end` as fake-seconds), and
/// words are accumulated into a group each time the **original** segment text
/// (without the separator) matches `SENTENCE_END`; any trailing words form a
/// final partial group.
///
/// This is the initial `WhisperResult`-equivalent the mg/sp/sl operations then
/// run on. (stable-ts's `WhisperResult.__init__` also runs
/// `remove_no_word_segments`, but every group produced here has ≥ 1 word, so
/// nothing is removed.)
pub fn words_from_segments(segments: &[Segment], language: Option<&str>) -> Vec<RSegment> {
    let separator = if is_non_space_language(language) {
        ""
    } else {
        " "
    };
    let mut out: Vec<RSegment> = Vec::new();
    let mut words: Vec<RWord> = Vec::new();

    for seg in segments {
        let text = format!("{}{}", seg.text, separator);
        words.push(RWord::new(text, ms_to_fake(seg.start), ms_to_fake(seg.end)));
        if ends_sentence(&seg.text) {
            out.push(RSegment {
                words: std::mem::take(&mut words),
            });
        }
    }
    if !words.is_empty() {
        out.push(RSegment { words });
    }
    out
}
