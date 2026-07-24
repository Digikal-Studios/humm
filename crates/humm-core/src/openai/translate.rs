//! LLM-translation batch protocol (design spec §4.1, F-031) — the pure,
//! network-free half of `humm_core::openai::translate`, ported line-for-line
//! from Buzz's `Translator._translate_batch`/`_parse_batch_response`
//! (`buzz/translator.py:82-145`). The worker thread that drains a channel of
//! transcript items, groups them into batches, and issues the actual HTTP
//! requests (Buzz `:147-186`) is a separate, later task built on top of
//! these functions — nothing here spawns a thread or touches the network.
//!
//! # Differential fixtures
//!
//! `parse_batch_response`'s fidelity to Buzz's `re.split(r'\[(\d+)\]\s*',
//! response)` + dict-loop semantics is not assumed — it is checked against
//! Buzz's own unmodified `_parse_batch_response` static method, run under a
//! stub-imported CPython (`validation/m6-differential/
//! gen_batch_parse_fixtures.py`), via the fixtures under
//! `crates/humm-core/tests/fixtures/batch_parse/` (see the `tests` module
//! below).

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

/// Buzz's fixed translation-queue batch size (`translator.py:17`): the
/// worker drains up to this many queued items into a single numbered-batch
/// request before falling back to per-item requests for the remainder.
pub const BATCH_SIZE: usize = 10;

/// Combines `items` into Buzz's numbered-batch user-message body
/// (`translator.py:85-88`): `"[1] {items[0]}\n[2] {items[1]}\n..."`,
/// 1-based, one item per line, joined with a single `\n`. An empty `items`
/// slice yields an empty string (Buzz's `"\n".join([])`).
pub fn combine_numbered(items: &[&str]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(i, text)| format!("[{}] {text}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builds Buzz's exact batch system prompt (`translator.py:90-96`): the
/// user's configured translation instruction (`user_prompt`), followed by a
/// fixed explanation of the numbered-batch response format the model must
/// echo back, mentioning how many texts (`n`) to expect. Ported as a single
/// `format!` matching Buzz's four concatenated f-string literals
/// byte-for-byte, including the exact placement of every `\n` and the
/// single space joining "texts." to "Process" and "above" to "and".
pub fn batch_system_prompt(user_prompt: &str, n: usize) -> String {
    format!(
        "{user_prompt}\n\nYou will receive {n} numbered texts. Process each one \
         separately according to the instruction above and return them in the \
         exact same numbered format, e.g.:\n[1] processed text\n[2] processed text"
    )
}

/// The compiled marker regex, built once. Buzz: `re.split(r'\[(\d+)\]\s*',
/// response)`. The `regex` crate's `\d` is Unicode-decimal-digit-aware by
/// default (same character class as Python `re`'s `\d` on a `str` pattern),
/// so the *split boundaries* this produces match Python's exactly, including
/// for non-ASCII digit markers — see [`digits_to_u64`]'s doc comment for the
/// one place that stops being true (converting the captured digits to a
/// number).
fn marker_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[(\d+)\]\s*").expect("marker regex is a static literal"))
}

/// Reproduces Python's `re.split(r'\[(\d+)\]\s*', response)` with its single
/// capturing group: an interleaved list of the literal text between matches
/// and each match's captured digit run — e.g. for `"[1] a[2] b"` this
/// returns `["", "1", "a", "2", "b"]` (matching CPython's own `re.split`
/// exactly, verified against every fixture in this module's `tests`).
///
/// This list is always `1 + 2*N` elements long for `N` matches: a leading
/// (possibly empty) piece of text before the first match, then one
/// `(captured digits, following text)` pair per match, where "following
/// text" runs up to the *next* match or, for the last match, to the end of
/// the string. Critically, `re.split` always appends that final trailing
/// piece even when a match ends exactly at the end of the input (an empty
/// string, not nothing) — see [`parse_batch_response`]'s doc comment for why
/// this matters.
fn split_markers(response: &str) -> Vec<&str> {
    let re = marker_regex();
    let mut parts = Vec::new();
    let mut last_end = 0;
    for caps in re.captures_iter(response) {
        let whole = caps
            .get(0)
            .expect("group 0 (the full match) is always present");
        parts.push(&response[last_end..whole.start()]);
        let digits = caps
            .get(1)
            .expect("capture group 1 always participates for a matched \\d+");
        parts.push(digits.as_str());
        last_end = whole.end();
    }
    parts.push(&response[last_end..]);
    parts
}

/// Parses a run of decimal-digit characters (as matched by the marker
/// regex's `\d+`) into a `u64`, folding each `char` through
/// [`char::to_digit`] (radix 10). Returns `None` if any character in the
/// run isn't ASCII `0`-`9`.
///
/// Two **documented micro-deviations** from Buzz here (design spec §4.1
/// explicitly allows both — a real OpenAI-compatible LLM instructed with an
/// ASCII `"[1] processed text"` example is not expected to emit either):
///
/// - **Overflow.** Python's `int()` is arbitrary-precision; this folds with
///   `saturating_mul`/`saturating_add`, capping at `u64::MAX`. A marker
///   number that large can never equal a real `1..=expected` batch index
///   anyway (`BATCH_SIZE` is 10), so the practical behavior — the marker
///   never matches an output slot — is unaffected.
/// - **Non-ASCII Unicode decimal digits** (e.g. Arabic-Indic `٣`, U+0663).
///   Python's `\d`/`int()` accept them (`int("٣") == 3`) because CPython's
///   `int()` consults each character's Unicode numeric value. Rust's
///   `char::to_digit` does not: empirically, `'٣'.to_digit(10)` returns
///   `None` even though `'٣'.is_numeric()` is `true` — there is no
///   equivalent in `std` without pulling in a Unicode-data crate for this
///   one edge case. The marker regex itself still identifies the correct
///   split boundary (the `regex` crate's Unicode-mode `\d+` matches the same
///   character class Python's `\d` does, so surrounding text is never
///   misattributed) — only the marker's *number* fails to parse, and
///   [`parse_batch_response`] silently skips it, exactly as an overflowed
///   number would be skipped. This is intentionally **not** covered by a
///   differential fixture (it would assert Rust and Python disagree, which
///   defeats the point of a parity fixture) — see the `digit_folding`
///   sub-module in `tests` for a Rust-only regression test of this exact
///   behavior instead.
fn digits_to_u64(digits: &str) -> Option<u64> {
    let mut acc: u64 = 0;
    for c in digits.chars() {
        let d = c.to_digit(10)?;
        acc = acc.saturating_mul(10).saturating_add(u64::from(d));
    }
    Some(acc)
}

/// Parses a numbered batch response like `"[1] text\n[2] text"` into a
/// `Vec` of `expected` translations, ordered `1..=expected`, ported exactly
/// from `Translator._parse_batch_response` (`translator.py:130-145`):
///
/// 1. Split `response` on the marker regex (see [`split_markers`]).
/// 2. Walk the resulting `(digits, text)` pairs — Buzz: `for i in range(1,
///    len(parts) - 1, 2)`. Each pair's digits are parsed to a number (see
///    [`digits_to_u64`]) and its text is `.strip()`-trimmed (Python
///    Unicode-whitespace trim, matched here by [`str::trim`], which trims
///    the same `White_Space`-property character set) and stored in a map
///    keyed by that number; a repeated number overwrites the earlier entry
///    ("last occurrence wins" — Buzz never special-cases duplicates, a plain
///    dict assignment does this for free).
/// 3. Build the result as `[map.get(1), map.get(2), ..., map.get(expected)]`,
///    defaulting any missing (or unparseable-number) entry to `""`.
///
/// **On the `range(1, len(parts) - 1, 2)` bound — a discrepancy this port's
/// own differential fixtures caught in the task brief's hand-derivation.**
/// One might expect this bound to *drop* a trailing captured number that has
/// no following text (e.g. a response ending exactly at a bare `"...[3]"`
/// with nothing after it) — reasoning that `re.split` would then yield an
/// even-length `parts` (`[..., "3"]`, no final empty string), making the
/// last `(digits, text)` pair fall outside `range(1, len(parts) - 1, 2)`.
/// **This reasoning is wrong**, verified against real CPython: `re.split`
/// with exactly one capturing group *always* appends a trailing string after
/// the last match, even when the match consumes the string's final
/// character (`re.split(r'\[(\d+)\]\s*', "[3]") == ['', '3', '']`, not
/// `['', '3']`). So `len(parts)` is always `1 + 2*N` for `N` matches — odd,
/// never even — and `range(1, len(parts) - 1, 2)` always walks *every*
/// matched pair. A bare trailing marker maps its number to `""`, it is never
/// silently dropped by the range bound itself. This port reflects that
/// verified behavior (see [`split_markers`]'s "always `1 + 2*N`" invariant,
/// which this function's `while i + 1 < parts.len()` loop relies on to
/// process every pair `split_markers` produced); the
/// `duplicate-bare-trailing-marker-overwrites` fixture exercises the one
/// place this is externally observable (last-occurrence-wins overwriting a
/// real value with the bare marker's `""`).
pub fn parse_batch_response(response: &str, expected: usize) -> Vec<String> {
    let parts = split_markers(response);

    let mut translations: HashMap<u64, String> = HashMap::new();
    let mut i = 1usize;
    while i + 1 < parts.len() {
        if let Some(num) = digits_to_u64(parts[i]) {
            translations.insert(num, parts[i + 1].trim().to_string());
        }
        i += 2;
    }

    (1..=expected as u64)
        .map(|i| translations.get(&i).cloned().unwrap_or_default())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------- combine_numbered

    #[test]
    fn combine_numbered_shape() {
        assert_eq!(
            combine_numbered(&["hello", "world", "foo"]),
            "[1] hello\n[2] world\n[3] foo"
        );
    }

    #[test]
    fn combine_numbered_single_item() {
        assert_eq!(combine_numbered(&["only"]), "[1] only");
    }

    #[test]
    fn combine_numbered_empty() {
        assert_eq!(combine_numbered(&[]), "");
    }

    // ------------------------------------------------------ batch_system_prompt

    #[test]
    fn batch_system_prompt_byte_shape() {
        assert_eq!(
            batch_system_prompt("Translate to French.", 2),
            "Translate to French.\n\nYou will receive 2 numbered texts. Process \
             each one separately according to the instruction above and return \
             them in the exact same numbered format, e.g.:\n[1] processed text\n\
             [2] processed text"
        );
    }

    // ----------------------------------------------------- parse_batch_response

    #[test]
    fn parse_happy_three_item() {
        assert_eq!(
            parse_batch_response("[1] Hello\n[2] World\n[3] Foo", 3),
            vec!["Hello", "World", "Foo"]
        );
    }

    #[test]
    fn parse_missing_number_becomes_empty() {
        assert_eq!(
            parse_batch_response("[1] Hello\n[3] Foo", 3),
            vec!["Hello", "", "Foo"]
        );
    }

    #[test]
    fn parse_duplicates_last_wins() {
        assert_eq!(
            parse_batch_response("[1] First\n[1] Second", 1),
            vec!["Second"]
        );
    }

    #[test]
    fn parse_preamble_prose_discarded() {
        assert_eq!(
            parse_batch_response("Here you go:\n[1] Hello\n[2] World", 2),
            vec!["Hello", "World"]
        );
    }

    #[test]
    fn parse_no_markers_all_empty() {
        assert_eq!(
            parse_batch_response("just plain text, no markers", 3),
            vec!["", "", ""]
        );
    }

    #[test]
    fn parse_extra_marker_beyond_expected_ignored() {
        assert_eq!(
            parse_batch_response("[1] Hello\n[9] Ignored", 1),
            vec!["Hello"]
        );
    }

    #[test]
    fn parse_whitespace_variants_after_marker() {
        // No space, several spaces, and a bare newline before the text --
        // `\s*` after the marker consumes all three.
        assert_eq!(
            parse_batch_response("[1]Tight\n[2]   Loose\n[3]\nOnNewline", 3),
            vec!["Tight", "Loose", "OnNewline"]
        );
    }

    #[test]
    fn parse_strip_trims_trailing_newline() {
        assert_eq!(
            parse_batch_response("[1] Hello world\n", 1),
            vec!["Hello world"]
        );
    }

    #[test]
    fn parse_strip_trims_leading_and_trailing_whitespace() {
        assert_eq!(
            parse_batch_response("[1]   padded on both sides   \n[2] next", 2),
            vec!["padded on both sides", "next"]
        );
    }

    #[test]
    fn parse_multiline_entry_preserved_internally() {
        assert_eq!(
            parse_batch_response("[1] Line one\nLine two\n[2] Second", 2),
            vec!["Line one\nLine two", "Second"]
        );
    }

    #[test]
    fn parse_empty_response_all_empty() {
        assert_eq!(parse_batch_response("", 2), vec!["", ""]);
    }

    #[test]
    fn parse_expected_zero_yields_empty_vec() {
        assert_eq!(parse_batch_response("[1] Hello", 0), Vec::<String>::new());
    }

    #[test]
    fn parse_out_of_order_numbers() {
        assert_eq!(
            parse_batch_response("[2] Second\n[1] First", 2),
            vec!["First", "Second"]
        );
    }

    /// Rust-only regression test for [`digits_to_u64`]'s documented
    /// non-ASCII-digit deviation — deliberately *not* a differential
    /// fixture, since Python parses `٣` (Arabic-Indic 3) as `3` and this
    /// port intentionally does not; asserting fixture parity here would
    /// assert the two disagree, which is not what fixtures are for.
    mod digit_folding {
        use super::*;

        #[test]
        fn ascii_marker_parses_normally() {
            assert_eq!(parse_batch_response("[7] seven", 7)[6], "seven");
        }

        #[test]
        fn non_ascii_unicode_digit_marker_is_skipped_not_matched() {
            // "٣" is Arabic-Indic digit three (U+0663). Python's `\d`/`int()`
            // would parse this marker as index 3; this port's
            // `char::to_digit` cannot (see `digits_to_u64`'s doc comment), so
            // the marker is treated as unmatched and every requested index
            // comes back empty.
            let response = "[\u{0663}] should not be attributed to index 3";
            assert_eq!(parse_batch_response(response, 3), vec!["", "", ""]);
        }

        #[test]
        fn non_ascii_digit_still_recognized_as_a_marker_boundary_by_the_regex() {
            // Confirms the doc comment's narrower claim directly: the
            // `regex` crate's Unicode-mode `\d+` DOES match "٣" as a digit
            // run (same character class Python's `\d` matches), so the text
            // is still split away from the bracket -- only the *numeric*
            // conversion (`digits_to_u64`) fails afterwards. If the regex
            // instead failed to recognize "٣" as `\d`, this whole string
            // would fall into `split_markers`' single leading "before" part
            // untouched, which would also produce all-empty output for a
            // different reason -- so this test inspects `split_markers`
            // directly rather than relying on `parse_batch_response`'s
            // output alone to distinguish the two.
            let parts = split_markers("[\u{0663}] text");
            assert_eq!(parts, vec!["", "\u{0663}", "text"]);
            assert!(digits_to_u64(parts[1]).is_none());
        }
    }

    // --------------------------------------------------- differential fixtures

    #[derive(serde::Deserialize)]
    struct Fixture {
        #[allow(dead_code)]
        name: String,
        response: String,
        expected_count: usize,
        parsed: Vec<String>,
    }

    /// Every `crates/humm-core/tests/fixtures/batch_parse/*.json` fixture,
    /// generated by stub-importing Buzz's own unmodified
    /// `Translator._parse_batch_response`
    /// (`validation/m6-differential/gen_batch_parse_fixtures.py`).
    const FIXTURES: &[(&str, &str)] = &[
        (
            "well-formed-three",
            include_str!("../../tests/fixtures/batch_parse/well-formed-three.json"),
        ),
        (
            "missing-middle-number",
            include_str!("../../tests/fixtures/batch_parse/missing-middle-number.json"),
        ),
        (
            "out-of-order-numbers",
            include_str!("../../tests/fixtures/batch_parse/out-of-order-numbers.json"),
        ),
        (
            "duplicate-last-wins",
            include_str!("../../tests/fixtures/batch_parse/duplicate-last-wins.json"),
        ),
        (
            "preamble-discarded",
            include_str!("../../tests/fixtures/batch_parse/preamble-discarded.json"),
        ),
        (
            "trailing-garbage-appended",
            include_str!("../../tests/fixtures/batch_parse/trailing-garbage-appended.json"),
        ),
        (
            "no-markers",
            include_str!("../../tests/fixtures/batch_parse/no-markers.json"),
        ),
        (
            "whitespace-variants",
            include_str!("../../tests/fixtures/batch_parse/whitespace-variants.json"),
        ),
        (
            "multiline-entry-preserved",
            include_str!("../../tests/fixtures/batch_parse/multiline-entry-preserved.json"),
        ),
        (
            "cjk-text",
            include_str!("../../tests/fixtures/batch_parse/cjk-text.json"),
        ),
        (
            "expected-count-larger-than-provided",
            include_str!(
                "../../tests/fixtures/batch_parse/expected-count-larger-than-provided.json"
            ),
        ),
        (
            "duplicate-bare-trailing-marker-overwrites",
            include_str!(
                "../../tests/fixtures/batch_parse/duplicate-bare-trailing-marker-overwrites.json"
            ),
        ),
        (
            "strip-trims-trailing-newline",
            include_str!("../../tests/fixtures/batch_parse/strip-trims-trailing-newline.json"),
        ),
    ];

    #[test]
    fn parse_batch_response_matches_python_reference_fixtures() {
        assert!(FIXTURES.len() >= 12, "brief requires ~12 fixture cases");

        for (name, json) in FIXTURES {
            let fx: Fixture =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("fixture {name} JSON: {e}"));

            let actual = parse_batch_response(&fx.response, fx.expected_count);

            assert_eq!(
                actual, fx.parsed,
                "fixture {name} mismatch (response={:?}, expected_count={})",
                fx.response, fx.expected_count
            );
        }
    }
}
