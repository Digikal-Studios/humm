//! `regroup::` — a faithful native port of the stable-ts 2.19.1 regroup subset
//! Buzz's transcript resizer "merge" feature reaches (F-086 merge half). Ground
//! truth is the pinned `stable_whisper/result.py` and Buzz's calling convention
//! in `transcription_resizer_widget.py` (re-read before changing this module).
//!
//! # Pipeline (Buzz-equivalent)
//!
//! With Buzz's fixed `transcribe_any(..., vad=False, suppress_silence=False,
//! check_sorted=False)` arguments, the merge pipeline reduces to:
//!
//! 1. **[`words_from_segments`]** — each DB segment becomes one stable-ts
//!    *word* (`text + separator`, timings as *fake-seconds* = `ms / 100.0`);
//!    words are grouped into sentence segments at every `SENTENCE_END` match
//!    (`.*[.!?。！？]`), trailing words forming a final group. (This is the
//!    `WhisperResult`-equivalent; `remove_no_word_segments` is a no-op since
//!    every group has ≥ 1 word.)
//! 2. **[`parse_rules`]** → **[`apply`]** — run the mg/sp/sl operations in
//!    order, mutating the segment list exactly as stable-ts does.
//! 3. Emit each output segment as `int(start * 100) .. int(end * 100)` ms
//!    (truncation toward zero) with `translation` dropped.
//!
//! [`regroup_segments`] performs all three; [`default_algo`] builds Buzz's
//! resizer regroup string and [`algo_from_env_or`] applies the
//! `HUMM_MERGE_REGROUP_RULE` override.
//!
//! # Source-trace notes (behaviors the spec/brief didn't fully capture)
//!
//! - **Fake-seconds truncation is lossy.** `int((ms/100.0)*100)` differs from
//!   `ms` for ~6.5 % of ms values (e.g. `29 → 0.29 → 28.999… → 28`). This is a
//!   genuine Buzz behavior and is reproduced bit-for-bit ([`words::fake_to_ms`]).
//! - **`round(x, 3)` on word load is an identity** for the 2-decimal
//!   fake-seconds Buzz feeds (verified over all reachable ms), and word timings
//!   are never arithmetic-combined by mg/sp/sl, so it is not reproduced.
//! - **The `('.',' ')` pair rule is largely inert** in Buzz's word model:
//!   words carry a *trailing* separator, so a next word never "starts with a
//!   space". The default punctuation string's real splitters are the `". "` /
//!   `"? "` / `", "` *string* rules (a word ends with `"punct "`), plus sentence
//!   grouping itself.
//! - **Sentence grouping splits at *any* `.`** (or `!?。！？`), including
//!   interior decimals like `"3.14"`, because `SENTENCE_END` is a prefix
//!   `re.match`.
//! - **mg then sp**: the default UI string merges by gap and *then* re-splits by
//!   punctuation, so on well-formed input the net effect on sentence boundaries
//!   can look like "no merge" even though the merge ran.
//! - **numpy is genuinely used** by the regroup math (`get_length_indices`
//!   even-split `cumsum`/`argmin`, `get_gap_indices` `nonzero`); the differential
//!   harness therefore uses real numpy (only torch/torchaudio are stubbed) — a
//!   correction to the brief's "pure Python" phrasing.
//! - **Output text keeps trailing separators** (and any `\n` inserted by
//!   `newline` mode): Buzz stores `segment.text` verbatim.

mod ops;
mod parser;
mod words;

pub use ops::apply;
pub use parser::{PunctItem, RegroupError, Rule, SUPPORTED_OPS, parse_rules};
pub use words::{RSegment, RWord, words_from_segments};

use crate::domain::{Millis, Segment};
use words::fake_to_ms;

/// Full Buzz merge pipeline: sentence-group `segments` into a
/// `WhisperResult`-equivalent, run the `algo` regroup operations, and emit
/// domain [`Segment`]s (ms via `int(x*100)` truncation, `translation` dropped).
///
/// Errors only from [`parse_rules`] (unsupported op / bad args); the algorithm
/// itself is infallible. An empty `algo` returns the sentence groups unchanged.
pub fn regroup_segments(
    segments: &[Segment],
    language: Option<&str>,
    algo: &str,
) -> Result<Vec<Segment>, RegroupError> {
    let rules = parse_rules(algo)?;
    let grouped = words_from_segments(segments, language);
    let out = apply(&rules, grouped);
    Ok(out
        .iter()
        .map(|rseg| Segment {
            start: Millis(fake_to_ms(rseg.start())),
            end: Millis(fake_to_ms(rseg.end())),
            text: rseg.text(),
            translation: String::new(),
        })
        .collect())
}

/// Build Buzz's resizer regroup string from the three merge checkboxes and
/// their fields (`transcription_resizer_widget.py:442-459`).
///
/// The `++{len}+1` suffix on the `mg` op (which sets `max_chars` + `is_sum_max`)
/// is appended **only when both** merge-by-gap and split-by-max-length are on,
/// exactly as Buzz builds it.
pub fn default_algo(
    merge_by_gap: bool,
    gap: &str,
    split_by_punct: bool,
    punct: &str,
    split_by_len: bool,
    len: &str,
) -> String {
    let mut s = String::new();
    if merge_by_gap {
        s.push_str("mg=");
        s.push_str(gap);
        if split_by_len {
            s.push_str("++");
            s.push_str(len);
            s.push_str("+1");
        }
    }
    if split_by_punct {
        if !s.is_empty() {
            s.push('_');
        }
        s.push_str("sp=");
        s.push_str(punct);
    }
    if split_by_len {
        if !s.is_empty() {
            s.push('_');
        }
        s.push_str("sl=");
        s.push_str(len);
    }
    s
}

/// Apply the `HUMM_MERGE_REGROUP_RULE` override to a built algorithm string,
/// mirroring Buzz's `os.getenv("BUZZ_MERGE_REGROUP_RULE", regroup_string)`
/// (`transcription_resizer_widget.py:459`): the env var wins whenever it is
/// **set** — even to an empty string.
pub fn algo_from_env_or(built: String) -> String {
    override_algo(
        built,
        std::env::var("HUMM_MERGE_REGROUP_RULE").ok().as_deref(),
    )
}

/// Pure core of [`algo_from_env_or`] (testable without touching real env):
/// `env_value` wins when present, else the `built` string is used.
fn override_algo(built: String, env_value: Option<&str>) -> String {
    match env_value {
        Some(v) => v.to_string(),
        None => built,
    }
}

#[cfg(test)]
mod tests;
