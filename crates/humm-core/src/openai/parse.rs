//! Pure response parsing for humm's OpenAI-compatible transcription engine
//! (design spec §3.4), ported line-for-line from Buzz's
//! `OpenAIWhisperAPIFileTranscriber`
//! (`buzz/transcriber/openai_whisper_api_file_transcriber.py:19-36,185-290`).
//!
//! Nothing here touches the network -- it is a pure `serde_json::Value ->
//! Vec<Segment>` transform, unit-tested entirely with JSON literals.
//!
//! Buzz's `get_value(segment, key, default)` (`:185-191`) distinguishes
//! attribute access (typed SDK response models) from `dict.get` (raw
//! `model_extra` fallbacks); since everything here starts life as
//! `serde_json::Value`, that distinction collapses to a plain JSON key
//! lookup with a default -- see [`num_or`] and [`str_or`] below.

use serde_json::Value;

use crate::domain::{Millis, Segment};

use super::client::ApiFailure;

/// Picks the `response_format` request parameter for `api_model`: gpt-4o
/// transcription models don't support `verbose_json`
/// (`openai_whisper_api_file_transcriber.py:196`), so they get `"json"`;
/// every other model (`whisper-1`, self-hosted whisper.cpp-server models,
/// ...) gets `"verbose_json"` to request segment/word timing.
pub fn response_format_for(api_model: &str) -> &'static str {
    if api_model.starts_with("gpt-4o") {
        "json"
    } else {
        "verbose_json"
    }
}

/// Parses an OpenAI-compatible transcription response body into
/// [`Segment`]s, porting `get_segments_for_file`'s parsing
/// (`openai_whisper_api_file_transcriber.py:217-290`) exactly, including
/// its whisper.cpp-server word-merge algorithm.
///
/// `offset_ms` is added to every timestamp produced from `seconds * 1000`
/// paths (chunked large-file transcription's chunk offset, Buzz `:177`) --
/// **except** inside the whisper.cpp branch, which never references
/// `offset_ms` in Buzz (`:238-268`); see that branch's own doc comment.
///
/// # Errors
///
/// Returns [`ApiFailure::Shape`] when `body` has neither a `segments`
/// array nor a `text` field -- the two shapes Buzz's fallback chain
/// (`:217-229`) recognizes.
pub fn parse_response(
    body: &Value,
    offset_ms: i64,
    word_timestamps: bool,
) -> Result<Vec<Segment>, ApiFailure> {
    let segments = resolve_segments(body)?;

    if !word_timestamps {
        return Ok(parse_without_word_timestamps(&segments, offset_ms));
    }

    let is_whisper_cpp = segments.first().is_some_and(is_whisper_cpp_segment);

    if is_whisper_cpp {
        Ok(parse_whisper_cpp_words(&segments))
    } else {
        Ok(parse_standard_words(&segments, offset_ms))
    }
}

/// Resolves the `segments` array per Buzz's fallback chain
/// (`:217-228`): the top-level `"segments"` array if present; else, when
/// `"text"` exists, the gpt-4o single-pseudo-segment fallback
/// (`:227-228`) carrying the top-level `"words"` along for the word-timing
/// path; else a shape error.
fn resolve_segments(body: &Value) -> Result<Vec<Value>, ApiFailure> {
    if let Some(segments) = body.get("segments").and_then(Value::as_array) {
        return Ok(segments.clone());
    }

    if let Some(text) = body.get("text") {
        let words = body.get("words").cloned().unwrap_or(Value::Null);
        return Ok(vec![serde_json::json!({
            "text": text,
            "start": 0,
            "end": 0,
            "words": words,
        })]);
    }

    let keys = body
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    Err(ApiFailure::Shape {
        detail: format!(
            "OpenAI transcription response had neither a `segments` array nor a `text` \
             field (top-level keys: [{keys}])"
        ),
    })
}

/// The non-word-timestamps path (`:280-288`): one [`Segment`] per element
/// of `segments`, with `start`/`end` defaulting to `0` when missing
/// (`get_value(segment, "start", 0)`).
fn parse_without_word_timestamps(segments: &[Value], offset_ms: i64) -> Vec<Segment> {
    segments
        .iter()
        .map(|segment| {
            let start = num_or(segment, "start", 0.0);
            let end = num_or(segment, "end", 0.0);
            Segment {
                start: Millis(scaled_ms(start, offset_ms)),
                end: Millis(scaled_ms(end, offset_ms)),
                text: str_or(segment, "text", ""),
                translation: String::new(),
            }
        })
        .collect()
}

/// Detects a whisper.cpp-server-style response: the first segment object
/// carries all three of `tokens`, `avg_logprob`, `no_speech_prob`
/// (`:234-236`). Buzz checks this with `hasattr`, which for these
/// dict/`model_extra` segments collapses to plain JSON key presence (see
/// the module doc comment).
fn is_whisper_cpp_segment(segment: &Value) -> bool {
    segment.get("tokens").is_some()
        && segment.get("avg_logprob").is_some()
        && segment.get("no_speech_prob").is_some()
}

/// The standard (non-whisper.cpp) word-timestamps path (`:270-279`): one
/// [`Segment`] per word across every segment's `"words"` array, offset
/// applied like the no-word-timestamps path.
fn parse_standard_words(segments: &[Value], offset_ms: i64) -> Vec<Segment> {
    let mut result = Vec::new();
    for segment in segments {
        for word in words_of(segment) {
            let start = num_or(word, "start", 0.0);
            let end = num_or(word, "end", 0.0);
            result.push(Segment {
                start: Millis(scaled_ms(start, offset_ms)),
                end: Millis(scaled_ms(end, offset_ms)),
                text: str_or(word, "word", ""),
                translation: String::new(),
            });
        }
    }
    result
}

/// The whisper.cpp-server word-merge path (`:238-268`, `append_segment`
/// `:19-36`): whisper.cpp splits text across word tokens more finely than
/// "one word per token" (leading-space/comma markers, and occasionally a
/// multi-byte UTF-8 character split across two tokens -- whisper.cpp issue
/// #1798), so Buzz re-merges them into text-run segments via a byte
/// buffer.
///
/// **No `offset_ms` is applied anywhere in this branch** -- Buzz's
/// `append_segment` call sites at `:250-268` never reference `offset_ms`
/// (unlike the standard-word and no-word-timestamps paths at `:275-276`
/// and `:283-284`, which do). This is very likely an oversight in Buzz
/// rather than a deliberate choice -- whisper.cpp-server responses are the
/// only source that hits this branch, and Buzz's chunked-transcription
/// offset (`:177`) would silently fail to apply to them -- but ground
/// truth is ground truth: this port preserves the quirk exactly rather
/// than "fixing" behavior the spec didn't ask us to change. See this
/// task's report for the citation.
///
/// `txt_buffer`/`txt_start`/`txt_end` are deliberately **not** reset
/// between segments (Buzz declares them once, before the outer `for
/// segment in segments:` loop at `:239-241`, and never clears them inside
/// it) -- only reassigned by the space/comma branches when a flush
/// succeeds. This port keeps that same scoping.
fn parse_whisper_cpp_words(segments: &[Value]) -> Vec<Segment> {
    let mut result = Vec::new();
    let mut buffer: Vec<u8> = Vec::new();
    let mut buf_start = 0.0_f64;
    let mut buf_end = 0.0_f64;

    for segment in segments {
        for word in words_of(segment) {
            let text = word.get("word").and_then(Value::as_str).unwrap_or("");
            let txt = text.as_bytes();
            let start = num_or(word, "start", 0.0);
            let end = num_or(word, "end", 0.0);

            if txt.starts_with(b" ") && append_segment(&mut result, &buffer, buf_start, buf_end) {
                buffer = txt.to_vec();
                buf_start = start;
                buf_end = end;
                continue;
            }

            if txt.starts_with(b", ") {
                buffer.push(b',');
                append_segment(&mut result, &buffer, buf_start, buf_end);
                buffer = strip_leading_commas(txt);
                buf_start = start;
                buf_end = end;
                continue;
            }

            buffer.extend_from_slice(txt);
            buf_end = end;
        }

        // Append the last segment (Buzz `:267-268` -- this flush is
        // *inside* the outer per-segment loop, not after it).
        append_segment(&mut result, &buffer, buf_start, buf_end);
    }

    result
}

/// Ports `append_segment` (`:19-36`) exactly: an empty buffer is a no-op
/// success (nothing to append, not an error); a non-empty buffer that
/// decodes as UTF-8 is pushed as a [`Segment`] (centiseconds `* 10` to ms,
/// no offset -- see [`parse_whisper_cpp_words`]); a non-empty buffer that
/// fails to decode returns `false` **without mutating `result` or
/// `txt`** -- the caller's control flow (not this function) decides
/// whether to keep accumulating into the same buffer, matching Buzz's
/// `try/except UnicodeDecodeError` guard against the multi-byte-character
/// split described in whisper.cpp issue #1798.
fn append_segment(
    result: &mut Vec<Segment>,
    txt: &[u8],
    start_centis: f64,
    end_centis: f64,
) -> bool {
    if txt.is_empty() {
        return true;
    }

    match std::str::from_utf8(txt) {
        Ok(text) => {
            result.push(Segment {
                start: Millis((start_centis * 10.0) as i64),
                end: Millis((end_centis * 10.0) as i64),
                text: text.to_string(),
                translation: String::new(),
            });
            true
        }
        Err(_) => false,
    }
}

/// `bytes.lstrip(b',')` (`:259`): strips every leading comma byte (`b','`
/// only -- not the space that typically follows it in the `b", "` arm),
/// stopping at the first non-comma byte.
fn strip_leading_commas(bytes: &[u8]) -> Vec<u8> {
    let first_non_comma = bytes.iter().position(|&b| b != b',').unwrap_or(bytes.len());
    bytes[first_non_comma..].to_vec()
}

/// A segment/word object's `"words"` array, or empty when absent
/// (`get_value(segment, "words")` with no default is `None` in Buzz, which
/// would raise iterating `for word in None`; this port degrades to "no
/// words" instead of crashing, per this codebase's unhappy-path
/// conventions -- not exercised by any response a real API sends).
fn words_of(segment: &Value) -> impl Iterator<Item = &Value> {
    segment
        .get("words")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// `get_value(obj, key, default)` collapsed to a JSON numeric lookup
/// (module doc comment): `obj[key]` as `f64` if present and numeric, else
/// `default`.
fn num_or(obj: &Value, key: &str, default: f64) -> f64 {
    obj.get(key).and_then(Value::as_f64).unwrap_or(default)
}

/// `get_value(obj, key, default)` collapsed to a JSON string lookup: `obj[key]`
/// as a string if present and a string, else `default`.
fn str_or(obj: &Value, key: &str, default: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

/// `int(seconds * 1000 + offset_ms)` ported literally, operator order
/// included: Python's `int()` truncates toward zero (`f64::trunc` here),
/// and it is applied to the **whole sum**, not to `seconds * 1000` alone
/// (`openai_whisper_api_file_transcriber.py:275-276,283-284`). The
/// distinction matters when `seconds` is negative and adding `offset_ms`
/// flips the sum's sign: truncating before vs. after the addition can
/// differ by up to 1ms in that case, since "toward zero" means opposite
/// directions on either side of zero.
fn scaled_ms(seconds: f64, offset_ms: i64) -> i64 {
    (seconds * 1000.0 + offset_ms as f64).trunc() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- response_format_for -----------------------------------------------

    #[test]
    fn response_format_for_gpt4o_models_is_json() {
        assert_eq!(response_format_for("gpt-4o-transcribe"), "json");
        assert_eq!(response_format_for("gpt-4o-mini-transcribe"), "json");
        assert_eq!(response_format_for("gpt-4o"), "json");
    }

    #[test]
    fn response_format_for_other_models_is_verbose_json() {
        assert_eq!(response_format_for("whisper-1"), "verbose_json");
        // Not a `gpt-4o*` prefix match -- "gpt-4o" is strictly longer than
        // "gpt-4", so "gpt-4" is not itself a match.
        assert_eq!(response_format_for("gpt-4"), "verbose_json");
        assert_eq!(
            response_format_for("custom-whisper-cpp-model"),
            "verbose_json"
        );
    }

    // -- resolve_segments / no-word-timestamps path --------------------------

    #[test]
    fn verbose_json_basic_two_segments_truncates_fractional_seconds() {
        let body = json!({
            "segments": [
                {"start": 1.9995, "end": 3.0, "text": "first"},
                {"start": 3.0, "end": 4.0, "text": "second"},
            ]
        });

        let segments = parse_response(&body, 0, false).expect("parses");

        assert_eq!(segments.len(), 2);
        // 1.9995 * 1000 = 1999.5 -- truncates to 1999, *not* rounds to 2000.
        assert_eq!(segments[0].start, Millis(1999));
        assert_eq!(segments[0].end, Millis(3000));
        assert_eq!(segments[0].text, "first");
        assert_eq!(segments[0].translation, "");
        assert_eq!(segments[1].start, Millis(3000));
        assert_eq!(segments[1].end, Millis(4000));
        assert_eq!(segments[1].text, "second");
    }

    #[test]
    fn offset_ms_is_added_to_segment_times() {
        let body = json!({
            "segments": [{"start": 1.0, "end": 2.0, "text": "hi"}]
        });

        let segments = parse_response(&body, 30_000, false).expect("parses");

        assert_eq!(segments[0].start, Millis(31_000));
        assert_eq!(segments[0].end, Millis(32_000));
    }

    #[test]
    fn negative_start_truncates_toward_zero() {
        let body = json!({
            "segments": [{"start": -0.5, "end": 0.0, "text": "hi"}]
        });

        let segments = parse_response(&body, 0, false).expect("parses");

        // Python: int(-0.5 * 1000) == -500 (truncation toward zero, not
        // floor -- floor would give -500 too here, but the point this
        // pins is that it's *not* -1000 from a floor-style round-down).
        assert_eq!(segments[0].start, Millis(-500));
    }

    #[test]
    fn scaled_ms_adds_before_truncating_not_after() {
        // Pins the operator order from `scaled_ms`'s own doc comment:
        // Buzz computes `int(seconds * 1000 + offset_ms)`, i.e. the sum is
        // truncated as a whole, not `int(seconds * 1000) + offset_ms`.
        // `start` here is -0.0005s, i.e. -0.5ms once scaled -- a value
        // with a fractional millisecond so the two orders can actually
        // diverge (an integral `seconds * 1000` like -500 wouldn't show
        // anything: trunc is a no-op on it either way). The naive
        // "truncate first" order gives `trunc(-0.5) + 30000 == 0 + 30000
        // == 30000`; Buzz's actual order gives `trunc(-0.5 + 30000) ==
        // trunc(29999.5) == 29999` -- one less, because "-0.5" truncates
        // toward zero from *below* zero while "29999.5" truncates toward
        // zero from *above* it.
        let body = json!({
            "segments": [{"start": -0.0005, "end": 0.0, "text": "hi"}]
        });

        let segments = parse_response(&body, 30_000, false).expect("parses");

        assert_eq!(segments[0].start, Millis(29_999));
    }

    #[test]
    fn gpt4o_text_only_response_is_single_pseudo_segment() {
        let body = json!({"text": "hello world"});

        let segments = parse_response(&body, 0, false).expect("parses");

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start, Millis(0));
        assert_eq!(segments[0].end, Millis(0));
        assert_eq!(segments[0].text, "hello world");
        assert_eq!(segments[0].translation, "");
    }

    #[test]
    fn missing_start_and_end_default_to_zero() {
        let body = json!({"segments": [{"text": "hi"}]});

        let segments = parse_response(&body, 0, false).expect("parses");

        assert_eq!(segments[0].start, Millis(0));
        assert_eq!(segments[0].end, Millis(0));
    }

    #[test]
    fn unknown_shape_is_a_shape_error() {
        let body = json!({"unexpected": "shape"});

        let result = parse_response(&body, 0, false);

        match result {
            Err(ApiFailure::Shape { detail }) => {
                assert!(detail.contains("segments"), "detail was: {detail}");
                assert!(detail.contains("text"), "detail was: {detail}");
            }
            other => panic!("expected Shape error, got {other:?}"),
        }
    }

    // -- standard word-timestamps branch -------------------------------------

    #[test]
    fn standard_word_branch_one_segment_per_word_with_offset() {
        let body = json!({
            "segments": [{
                "start": 0.0,
                "end": 2.0,
                "text": "Hello world",
                "words": [
                    {"word": "Hello", "start": 0.5, "end": 1.0},
                    {"word": "world", "start": 1.0, "end": 1.5},
                ]
            }]
        });

        let segments = parse_response(&body, 30_000, true).expect("parses");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start, Millis(30_500));
        assert_eq!(segments[0].end, Millis(31_000));
        assert_eq!(segments[0].text, "Hello");
        assert_eq!(segments[1].start, Millis(31_000));
        assert_eq!(segments[1].end, Millis(31_500));
        assert_eq!(segments[1].text, "world");
    }

    // -- whisper.cpp word-merge branch ---------------------------------------

    fn wcpp_segment(words: Value) -> Value {
        json!({
            "tokens": [1, 2, 3],
            "avg_logprob": -0.2,
            "no_speech_prob": 0.01,
            "words": words,
        })
    }

    #[test]
    fn wcpp_branch_merges_words_into_text_runs_with_centisecond_times() {
        // "Hello" (no leading space -- first token of the segment) then
        // " wor" + "ld" merge into " world" (continuation tokens, no
        // leading space), then " next" starts a new run.
        let body = json!({
            "segments": [wcpp_segment(json!([
                {"word": "Hello", "start": 0, "end": 30},
                {"word": " wor", "start": 30, "end": 50},
                {"word": "ld", "start": 50, "end": 70},
                {"word": " next", "start": 70, "end": 100},
            ]))]
        });

        let segments = parse_response(&body, 0, true).expect("parses");

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text, "Hello");
        assert_eq!(segments[0].start, Millis(0));
        assert_eq!(segments[0].end, Millis(300)); // 30 centis * 10
        assert_eq!(segments[1].text, " world");
        assert_eq!(segments[1].start, Millis(300));
        assert_eq!(segments[1].end, Millis(700)); // 70 centis * 10
        assert_eq!(segments[2].text, " next");
        assert_eq!(segments[2].start, Millis(700));
        assert_eq!(segments[2].end, Millis(1000)); // 100 centis * 10
    }

    #[test]
    fn wcpp_branch_comma_arm_appends_comma_then_flushes_and_strips() {
        let body = json!({
            "segments": [wcpp_segment(json!([
                {"word": "Hi", "start": 0, "end": 20},
                {"word": ", there", "start": 20, "end": 50},
            ]))]
        });

        let segments = parse_response(&body, 0, true).expect("parses");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Hi,");
        assert_eq!(segments[0].start, Millis(0));
        assert_eq!(segments[0].end, Millis(200));
        // Only the comma is stripped by `lstrip(b',')` -- the space stays.
        assert_eq!(segments[1].text, " there");
        assert_eq!(segments[1].start, Millis(200));
        assert_eq!(segments[1].end, Millis(500));
    }

    #[test]
    fn wcpp_branch_empty_word_and_empty_buffer_flush_is_a_no_op() {
        let body = json!({
            "segments": [wcpp_segment(json!([
                {"word": "", "start": 0, "end": 10},
            ]))]
        });

        let segments = parse_response(&body, 0, true).expect("parses");

        assert!(
            segments.is_empty(),
            "an all-empty word buffer should flush to nothing, got {segments:?}"
        );
    }

    #[test]
    fn wcpp_branch_offset_ms_is_not_applied() {
        // Pins the quirk documented on `parse_whisper_cpp_words`: unlike
        // every other path in this module, the whisper.cpp branch never
        // references `offset_ms` in Buzz (`:250-268`). A large offset here
        // must produce the *same* times as offset 0 would.
        let body = json!({
            "segments": [wcpp_segment(json!([
                {"word": "Hello", "start": 0, "end": 50},
            ]))]
        });

        let segments = parse_response(&body, 30_000, true).expect("parses");

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start, Millis(0));
        assert_eq!(segments[0].end, Millis(500));
    }

    // -- append_segment's decode-failure guard, exercised directly ----------
    //
    // Every `"word"` field reaching `parse_whisper_cpp_words` comes from
    // `serde_json::Value::as_str`, which -- unlike Python's `str` -- is a
    // Rust `&str` and therefore *always* already-valid, complete UTF-8; no
    // sequence of valid complete UTF-8 byte strings can concatenate into an
    // invalid one (UTF-8 is self-synchronizing). So the decode-failure
    // guard Buzz added for whisper.cpp issue #1798 (a token whose raw bytes
    // are only *part* of a multi-byte character) cannot be reached by
    // constructing JSON literals in Rust the way it can from whisper.cpp's
    // raw wire bytes in Python. These tests instead exercise
    // `append_segment` directly with raw byte slices -- valid `&[u8]`,
    // deliberately not valid UTF-8 on their own -- which is the exact unit
    // Buzz's `append_segment` (`:19-36`) operates on, and is the most
    // faithful reproduction of "two word entries whose bytes split a
    // multi-byte character" available under Rust's `String` invariants.

    #[test]
    fn append_segment_keeps_partial_multibyte_char_undecoded() {
        let mut result = Vec::new();

        // 0xC3 alone is the leading byte of a 2-byte UTF-8 sequence (the
        // first half of 'é' = 0xC3 0xA9) -- invalid on its own.
        let flushed = append_segment(&mut result, &[0xC3], 0.0, 5.0);

        assert!(!flushed, "a lone leading byte must not decode");
        assert!(result.is_empty());
    }

    #[test]
    fn append_segment_succeeds_once_the_split_character_is_rejoined() {
        let mut result = Vec::new();

        // Simulates the caller's "keep accumulating" behavior: the first
        // word's bytes failed to flush (previous test), so they stay in
        // the buffer and the second word's bytes are appended to it.
        let mut buffer = vec![0xC3];
        buffer.extend_from_slice(&[0xA9]);

        let flushed = append_segment(&mut result, &buffer, 0.0, 5.0);

        assert!(flushed);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text, "é");
        assert_eq!(result[0].start, Millis(0));
        assert_eq!(result[0].end, Millis(50)); // 5 centis * 10
    }

    #[test]
    fn append_segment_empty_buffer_is_a_no_op_success() {
        let mut result = Vec::new();

        let flushed = append_segment(&mut result, b"", 1.0, 2.0);

        assert!(flushed);
        assert!(result.is_empty());
    }

    // -- strip_leading_commas -------------------------------------------------

    #[test]
    fn strip_leading_commas_strips_only_commas_not_the_following_space() {
        assert_eq!(strip_leading_commas(b",, there"), b" there".to_vec());
        assert_eq!(strip_leading_commas(b"no commas"), b"no commas".to_vec());
        assert_eq!(strip_leading_commas(b",,,"), Vec::<u8>::new());
    }
}
