//! Word- and character-level timing reconstruction from a whisper segment's
//! token stream.
//!
//! Pure, FFI-free port of Buzz's `_parse_word_level_timings`,
//! `_flush_complete_chars`, and `_append_word`
//! (`buzz/transcriber/whisper_cpp.py:77-125,240-319`). The engine adapter
//! ([`crate::engine`]) collects each whisper segment's tokens into
//! [`TokenInfo`]s — already converted to the intended timeline in
//! milliseconds — and calls [`group_words`] to turn them into the per-word
//! (or, for scriptio-continua languages, per-character) [`Segment`]s Buzz's
//! word-level-timing mode produces.

use humm_core::domain::{Millis, Segment};

/// One whisper token's raw output for a single segment.
///
/// `bytes` holds the token's raw text bytes rather than a `String` because
/// whisper's tokenizer can split a multi-byte UTF-8 character across two
/// tokens (`buzz/transcriber/whisper_cpp.py` reconstructs the same way, via
/// `latin-1` round-tripping); [`group_words`] reassembles complete characters
/// from the byte stream. `start`/`end` are already in the intended timeline
/// (VAD remapping, if any, is applied by the caller). `p` is the token
/// probability whisper reports.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenInfo {
    /// Raw token text bytes (may split a multi-byte UTF-8 char).
    pub bytes: Vec<u8>,
    /// Token start time.
    pub start: Millis,
    /// Token end time.
    pub end: Millis,
    /// Token probability, as reported by whisper.
    pub p: f32,
}

/// Languages whisper writes without spaces between words (scriptio continua);
/// for these, word-level mode emits one [`Segment`] per character
/// (`buzz/transcriber/whisper_cpp.py:244`).
const NON_SPACE_LANGUAGES: [&str; 6] = ["zh", "ja", "th", "lo", "km", "my"];

/// Groups one segment's `segment_tokens` into word (or per-character)
/// [`Segment`]s.
///
/// `language` is the source-language code (e.g. `"fr"`); `None` takes the
/// space-delimited path. Exact port of Buzz's `_parse_word_level_timings`
/// (`buzz/transcriber/whisper_cpp.py:240-319`): for space-delimited
/// languages, tokens are joined into words on leading-space boundaries with a
/// low-probability filter; for the non-space languages in
/// [`NON_SPACE_LANGUAGES`], each complete UTF-8 character becomes its own
/// segment with no probability filter. Every emitted segment has an empty
/// `translation`.
pub fn group_words(segment_tokens: &[TokenInfo], language: Option<&str>) -> Vec<Segment> {
    let is_non_space = language.is_some_and(|code| NON_SPACE_LANGUAGES.contains(&code));
    if is_non_space {
        group_characters(segment_tokens)
    } else {
        group_space_delimited(segment_tokens)
    }
}

/// Space-delimited path (`buzz/transcriber/whisper_cpp.py:280-317`): joins
/// tokens into words on leading-space boundaries, handling the special
/// leading-comma case, skipping bracket-special and empty tokens as well as
/// low-probability (`p < 0.01`) tokens.
fn group_space_delimited(tokens: &[TokenInfo]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut word_buffer: Vec<u8> = Vec::new();
    let mut word_start = Millis(0);
    let mut word_end = Millis(0);

    for token in tokens {
        if is_special_or_empty(&token.bytes) {
            continue;
        }
        if token.p < 0.01 {
            continue;
        }

        if token.bytes.starts_with(b" ") && !word_buffer.is_empty() {
            // Leading space starts a new word; flush the previous one.
            append_word(&word_buffer, word_start, word_end, &mut segments);
            word_buffer = token.bytes.clone();
            word_start = token.start;
            word_end = token.end;
        } else if token.bytes.starts_with(b", ") {
            // ", ..." attaches the comma to the current word, flushes it, and
            // starts a new word from the remainder (leading commas stripped).
            word_buffer.push(b',');
            append_word(&word_buffer, word_start, word_end, &mut segments);
            word_buffer = lstrip_commas(&token.bytes);
            word_start = token.start;
            word_end = token.end;
        } else {
            if word_buffer.is_empty() {
                word_start = token.start;
            }
            word_buffer.extend_from_slice(&token.bytes);
            word_end = token.end;
        }
    }
    // Mid-loop flushes ignore a `false` return and drop the word (Buzz
    // lines 374 and 380). Only the segment's final buffer gets the
    // lossy prefix (`whisper_cpp.py` around line 391).
    if !append_word(&word_buffer, word_start, word_end, &mut segments) {
        push_lossy_final_word(&word_buffer, word_start, word_end, &mut segments);
    }
    segments
}

/// Non-space (scriptio continua) path
/// (`buzz/transcriber/whisper_cpp.py:251-279`): buffers token bytes and emits
/// one segment per complete UTF-8 character as it becomes available, with no
/// probability filter. A character's span is the start/end of the tokens that
/// contributed to it, following Buzz's `char_start`/`char_end` bookkeeping.
fn group_characters(tokens: &[TokenInfo]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut char_buffer: Vec<u8> = Vec::new();
    let mut char_start = Millis(0);
    let mut char_end = Millis(0);

    for token in tokens {
        if is_special_or_empty(&token.bytes) {
            continue;
        }

        if char_buffer.is_empty() {
            char_start = token.start;
        }
        char_buffer.extend_from_slice(&token.bytes);
        char_end = token.end;

        char_buffer = flush_complete_chars(&char_buffer, char_start, char_end, &mut segments);

        if char_buffer.is_empty() {
            char_start = token.end;
        }
    }
    if !char_buffer.is_empty() {
        flush_complete_chars(&char_buffer, char_start, char_end, &mut segments);
    }
    segments
}

/// Whether a token is skipped on both paths: a whisper special token (text
/// starts with `[_`, e.g. `[_TT_50]`) or an empty token
/// (`buzz/transcriber/whisper_cpp.py:259,288`).
fn is_special_or_empty(bytes: &[u8]) -> bool {
    bytes.is_empty() || bytes.starts_with(b"[_")
}

/// Strips leading `,` bytes, mirroring Python's `bytes.lstrip(b",")`.
fn lstrip_commas(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().position(|&b| b != b',').unwrap_or(bytes.len());
    bytes[start..].to_vec()
}

/// Decodes `buffer` as UTF-8, trims it, and — if non-empty — pushes a word
/// segment (`buzz/transcriber/whisper_cpp.py` `_append_word`).
///
/// Returns `true` when the buffer was empty or valid UTF-8 (even if `trim`
/// left nothing to emit). Returns `false` when the bytes are not valid UTF-8,
/// and emits nothing. Callers that flush because the next token starts a new
/// word ignore `false` and drop the word. Only [`push_lossy_final_word`]
/// recovers a prefix, and only for the segment's last buffer.
fn append_word(buffer: &[u8], start: Millis, end: Millis, segments: &mut Vec<Segment>) -> bool {
    if buffer.is_empty() {
        return true;
    }
    let Ok(text) = std::str::from_utf8(buffer) else {
        return false;
    };
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        segments.push(Segment {
            start,
            end,
            text: trimmed.to_string(),
            translation: String::new(),
        });
    }
    true
}

/// Buzz's final-flush fallback (`whisper_cpp.py` around line 391):
/// `decode(errors="replace").rstrip("\ufffd").strip()`.
///
/// `trim_end_matches` removes a trailing run of U+FFFD only. A replacement
/// at the start or in the middle stays. Not used for mid-word flushes.
fn push_lossy_final_word(buffer: &[u8], start: Millis, end: Millis, segments: &mut Vec<Segment>) {
    let lossy = String::from_utf8_lossy(buffer);
    let text = lossy.trim_end_matches('\u{FFFD}').trim();
    if text.is_empty() {
        return;
    }
    segments.push(Segment {
        start,
        end,
        text: text.to_string(),
        translation: String::new(),
    });
}

/// Extracts every complete UTF-8 character from the front of `buffer`, pushing
/// each non-whitespace character as its own segment spanning `start..end`, and
/// returns the trailing bytes that don't yet form a complete character (an
/// exact port of Buzz's `_flush_complete_chars`,
/// `buzz/transcriber/whisper_cpp.py:77-104`). Because the minimal byte length
/// that decodes as valid UTF-8 always yields exactly one character, each
/// successful decode contributes a single character.
fn flush_complete_chars(
    buffer: &[u8],
    start: Millis,
    end: Millis,
    segments: &mut Vec<Segment>,
) -> Vec<u8> {
    let mut pos = 0;
    while pos < buffer.len() {
        // Buzz's `range(1, min(5, len - pos + 1))`: try 1..=min(4, len-pos).
        let upper = std::cmp::min(5, buffer.len() - pos + 1);
        let mut advanced = false;
        for char_len in 1..upper {
            match std::str::from_utf8(&buffer[pos..pos + char_len]) {
                Ok(text) => {
                    if !text.trim().is_empty() {
                        segments.push(Segment {
                            start,
                            end,
                            text: text.to_string(),
                            translation: String::new(),
                        });
                    }
                    pos += char_len;
                    advanced = true;
                    break;
                }
                Err(_) => {
                    // A 4-byte prefix that still isn't valid, or bytes that run
                    // to the end of the buffer, are incomplete/invalid: keep
                    // them for the next token.
                    if char_len == 4 || pos + char_len >= buffer.len() {
                        return buffer[pos..].to_vec();
                    }
                }
            }
        }
        if !advanced {
            return buffer[pos..].to_vec();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn tok(bytes: &[u8], start: i64, end: i64, p: f32) -> TokenInfo {
        TokenInfo {
            bytes: bytes.to_vec(),
            start: Millis(start),
            end: Millis(end),
            p,
        }
    }

    fn word(text: &str, start: i64, end: i64) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    // Case 1: three space-prefixed tokens become three words, each keeping its
    // own token span.
    #[test]
    fn splits_space_prefixed_tokens_into_words() {
        let tokens = [
            tok(b" Bien", 40, 299, 1.0),
            tok(b" venue", 299, 320, 1.0),
            tok(b" dans", 320, 329, 1.0),
        ];
        assert_eq!(
            group_words(&tokens, Some("fr")),
            vec![
                word("Bien", 40, 299),
                word("venue", 299, 320),
                word("dans", 320, 329),
            ]
        );
    }

    // Case 2: a token starting with ", " appends the comma to the current word,
    // flushes it, and starts a new word from the remainder.
    #[test]
    fn comma_token_attaches_comma_then_starts_new_word() {
        let tokens = [tok(b" Hi", 0, 10, 1.0), tok(b", there", 10, 20, 1.0)];
        assert_eq!(
            group_words(&tokens, Some("en")),
            vec![word("Hi,", 0, 10), word("there", 10, 20)]
        );
    }

    // Case 3: a low-probability token (p < 0.01) is skipped on the
    // space-delimited path.
    #[test]
    fn low_probability_token_is_skipped() {
        let tokens = [
            tok(b" a", 0, 10, 1.0),
            tok(b" b", 10, 20, 0.005),
            tok(b" c", 20, 30, 1.0),
        ];
        assert_eq!(
            group_words(&tokens, Some("en")),
            vec![word("a", 0, 10), word("c", 20, 30)]
        );
    }

    // Case 4: special tokens whose text starts with "[_" are skipped.
    #[test]
    fn special_bracket_tokens_are_skipped() {
        let tokens = [
            tok(b" Hi", 0, 10, 1.0),
            tok(b"[_TT_50]", 10, 15, 1.0),
            tok(b" world", 15, 25, 1.0),
        ];
        assert_eq!(
            group_words(&tokens, Some("en")),
            vec![word("Hi", 0, 10), word("world", 15, 25)]
        );
    }

    // Case 5: a multi-byte UTF-8 char split across two consecutive tokens
    // (é = 0xC3 0xA9) reassembles into one word spanning both tokens.
    #[test]
    fn utf8_char_split_across_tokens_reassembles() {
        let tokens = [tok(&[0xC3], 0, 10, 1.0), tok(&[0xA9], 10, 20, 1.0)];
        assert_eq!(group_words(&tokens, Some("fr")), vec![word("é", 0, 20)]);
    }

    // Case 6: a non-space language (ja) with each character split into
    // single-byte tokens yields one segment per complete character, each
    // spanning the tokens that contributed to it.
    #[test]
    fn japanese_emits_one_segment_per_character() {
        // こんにちは = E3 81 93 | E3 82 93 | E3 81 AB | E3 81 A1 | E3 81 AF
        let bytes: [u8; 15] = [
            0xE3, 0x81, 0x93, 0xE3, 0x82, 0x93, 0xE3, 0x81, 0xAB, 0xE3, 0x81, 0xA1, 0xE3, 0x81,
            0xAF,
        ];
        let tokens: Vec<TokenInfo> = bytes
            .iter()
            .enumerate()
            .map(|(i, &b)| tok(&[b], (i as i64) * 10, (i as i64 + 1) * 10, 1.0))
            .collect();
        assert_eq!(
            group_words(&tokens, Some("ja")),
            vec![
                word("こ", 0, 30),
                word("ん", 30, 60),
                word("に", 60, 90),
                word("ち", 90, 120),
                word("は", 120, 150),
            ]
        );
    }

    /// Three cases in one test so a global strip inside every `append_word`
    /// cannot pass. The fallback is only the segment's last buffer.
    #[test]
    fn final_flush_strips_only_a_trailing_replacement_character() {
        // Trailing incomplete byte: keep the prefix, drop the trailing U+FFFD.
        assert_eq!(
            group_words(&[tok(b"hello\xff", 0, 10, 1.0)], Some("en")),
            vec![word("hello", 0, 10)]
        );
        // Leading invalid byte stays. Stripping every U+FFFD would yield "hello".
        assert_eq!(
            group_words(&[tok(b"\xffhello", 0, 10, 1.0)], Some("en")),
            vec![word("\u{FFFD}hello", 0, 10)]
        );
        // The next token starts with a space, so the bad buffer is a mid-loop
        // flush and is dropped. A fallback on that flush would also emit "hello".
        assert_eq!(
            group_words(
                &[tok(b"hello\xff", 0, 10, 1.0), tok(b" world", 10, 20, 1.0),],
                Some("en"),
            ),
            vec![word("world", 10, 20)]
        );
    }

    // The non-space path applies no probability filter (unlike the
    // space-delimited path): a low-p single-byte-per-char token still counts.
    #[test]
    fn non_space_path_has_no_probability_filter() {
        // 世 = E4 B8 96, three single-byte tokens, middle one low-p.
        let tokens = [
            tok(&[0xE4], 0, 10, 0.001),
            tok(&[0xB8], 10, 20, 0.001),
            tok(&[0x96], 20, 30, 0.001),
        ];
        assert_eq!(group_words(&tokens, Some("zh")), vec![word("世", 0, 30)]);
    }
}
