//! The mg/sp/sl regroup operations, ported from stable-ts
//! `stable_whisper/result.py` (v2.19.1). Every function here mirrors a named
//! stable-ts method or helper; the doc comment on each cites the source lines.
//!
//! All operations mutate a `Vec<RSegment>` in place (stable-ts operates
//! in-place on `WhisperResult.segments`). Segment `id`s are stable-ts's only
//! post-op bookkeeping (`reassign_ids`) and equal the list position after every
//! op, so this port uses the positional index directly instead of storing ids.

use std::collections::HashSet;

use super::parser::{PunctItem, Rule};
use super::words::{RSegment, RWord};

/// Run each rule left-to-right (`WhisperResult.regroup` → `method(**kwargs)`,
/// `result.py:2972-2976`), mutating the segment list.
pub fn apply(rules: &[Rule], mut segments: Vec<RSegment>) -> Vec<RSegment> {
    for rule in rules {
        match rule {
            Rule::MergeByGap {
                min_gap,
                max_words,
                max_chars,
                is_sum_max,
                lock,
                newline,
            } => merge_by_gap(
                &mut segments,
                *min_gap,
                *max_words,
                *max_chars,
                *is_sum_max,
                *lock,
                *newline,
            ),
            Rule::SplitByPunctuation {
                punctuation,
                lock,
                newline,
                min_words,
                min_chars,
                min_dur,
                ignore_special_periods,
            } => split_by_punctuation(
                &mut segments,
                punctuation,
                *lock,
                *newline,
                *min_words,
                *min_chars,
                *min_dur,
                *ignore_special_periods,
            ),
            Rule::SplitByLength {
                max_chars,
                max_words,
                even_split,
                force_len,
                lock,
                include_lock,
                newline,
                ignore_special_periods,
            } => split_by_length(
                &mut segments,
                *max_chars,
                *max_words,
                *even_split,
                *force_len,
                *lock,
                *include_lock,
                *newline,
                *ignore_special_periods,
            ),
        }
    }
    segments
}

// --- lock-index helpers -----------------------------------------------------

/// `WhisperResult.get_locked_indices` (`result.py:1341-1345`): boundary `i`
/// (between segment `i` and `i+1`) is locked iff `segments[i+1].left_locked` or
/// `segments[i].right_locked` (note stable-ts's inverted `zip` — verified).
fn result_locked_indices(segs: &[RSegment]) -> HashSet<usize> {
    (0..segs.len().saturating_sub(1))
        .filter(|&i| segs[i + 1].left_locked() || segs[i].right_locked())
        .collect()
}

/// `Segment.get_locked_indices` (`result.py:707-711`): word-boundary `i` is
/// locked iff `words[i+1].left_locked` or `words[i].right_locked`.
fn segment_locked_indices(seg: &RSegment) -> Vec<usize> {
    let w = &seg.words;
    (0..w.len().saturating_sub(1))
        .filter(|&i| w[i + 1].left_locked || w[i].right_locked)
        .collect()
}

// --- merge ------------------------------------------------------------------

/// `WhisperResult.get_gap_indices` (merging, `result.py:1353-1359`): boundaries
/// whose inter-segment gap (`next.start - prev.end`, fake-seconds) ≤ `min_gap`,
/// minus locked boundaries.
fn get_gap_indices(segs: &[RSegment], min_gap: f64) -> Vec<usize> {
    if segs.len() < 2 {
        return Vec::new();
    }
    let locked = result_locked_indices(segs);
    (0..segs.len() - 1)
        .filter(|&i| (segs[i + 1].start() - segs[i].end()) <= min_gap && !locked.contains(&i))
        .collect()
}

/// `WhisperResult.merge_by_gap` (`result.py:1642-1687`).
fn merge_by_gap(
    segs: &mut Vec<RSegment>,
    min_gap: f64,
    max_words: Option<i64>,
    max_chars: Option<i64>,
    is_sum_max: bool,
    lock: bool,
    newline: bool,
) {
    let indices = get_gap_indices(segs, min_gap);
    merge_segments(
        segs, &indices, max_words, max_chars, is_sum_max, lock, newline,
    );
}

/// `WhisperResult._merge_segments` (`result.py:1496-1531`): walk the merge
/// boundaries **in reverse**, skipping any whose merged pair would exceed the
/// word/char caps (per-segment, or summed when `is_sum_max`), and merge the
/// rest in place.
fn merge_segments(
    segs: &mut Vec<RSegment>,
    indices: &[usize],
    max_words: Option<i64>,
    max_chars: Option<i64>,
    is_sum_max: bool,
    lock: bool,
    newline: bool,
) {
    if indices.is_empty() {
        return;
    }
    for &i in indices.iter().rev() {
        // `max_words and seg.has_words and (...)` — the word cap also requires
        // the (already-guaranteed) presence of words.
        let skip_words = truthy(max_words) && segs[i].has_words() && {
            let mw = max_words.unwrap();
            let a = segs[i].word_count() as i64;
            let b = segs[i + 1].word_count() as i64;
            if is_sum_max {
                a + b > mw
            } else {
                a > mw && b > mw
            }
        };
        let skip_chars = truthy(max_chars) && {
            let mc = max_chars.unwrap();
            let a = segs[i].char_count() as i64;
            let b = segs[i + 1].char_count() as i64;
            if is_sum_max {
                a + b > mc
            } else {
                a > mc && b > mc
            }
        };
        if skip_words || skip_chars {
            continue;
        }
        add_segments(segs, i, lock, newline);
    }
    remove_no_word_segments(segs);
}

/// `WhisperResult.add_segments(i, i+1, inplace=True)` composed with
/// `Segment.add` (`result.py:1074-1100`, `:466-493`): concatenate segment `i`'s
/// and `i+1`'s words into `i`, optionally inserting `\n` at the seam
/// (`newline`) and locking the seam (`lock`), then drop `i+1`.
fn add_segments(segs: &mut Vec<RSegment>, i: usize, lock: bool, newline: bool) {
    let first_len = segs[i].words.len();
    let mut words: Vec<RWord> = Vec::with_capacity(first_len + segs[i + 1].words.len());
    words.extend(segs[i].words.iter().cloned());
    words.extend(segs[i + 1].words.iter().cloned());

    if newline && first_len > 0 {
        let idx = first_len - 1;
        if !words[idx].text.ends_with('\n') {
            words[idx].text.push('\n');
        }
    }
    if lock && first_len > 0 {
        // lock_idx = len(first segment's words); lock its right and the next
        // word's left.
        let lock_idx = first_len;
        words[lock_idx - 1].right_locked = true;
        if lock_idx < words.len() {
            words[lock_idx].left_locked = true;
        }
    }

    segs[i] = RSegment { words };
    segs.remove(i + 1);
}

// --- split (punctuation) ----------------------------------------------------

/// `Segment.get_punctuation_indices` (splitting, `result.py:729-747`): for a
/// string rule, split after a word that **ends with** it (or, past the first
/// word, before a word that **starts with** it); for a `(ending, beginning)`
/// pair rule, split between a word ending with `ending` and the next word
/// starting with `beginning`. Locked boundaries are removed.
// The index is used arithmetically (`i - 1` / `i + 1`) to record boundaries, so
// an index range mirrors stable-ts's `enumerate(words[:-1])` most faithfully.
#[allow(clippy::needless_range_loop)]
fn punctuation_indices(seg: &RSegment, punct: &[PunctItem]) -> Vec<usize> {
    let w = &seg.words;
    if w.len() < 2 {
        return Vec::new();
    }
    let mut indices: Vec<usize> = Vec::new();
    for p in punct {
        match p {
            PunctItem::Str(s) => {
                for i in 0..w.len() - 1 {
                    if w[i].text.ends_with(s.as_str()) {
                        indices.push(i);
                    } else if i != 0 && w[i].text.starts_with(s.as_str()) {
                        indices.push(i - 1);
                    }
                }
            }
            PunctItem::Pair(ending, beginning) => {
                for i in 0..w.len() - 1 {
                    if w[i].text.ends_with(ending.as_str())
                        && w[i + 1].text.starts_with(beginning.as_str())
                    {
                        indices.push(i);
                    }
                }
            }
        }
    }
    let locked: HashSet<usize> = segment_locked_indices(seg).into_iter().collect();
    let mut out: Vec<usize> = indices
        .into_iter()
        .filter(|i| !locked.contains(i))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// `WhisperResult.split_by_punctuation` (`result.py:1689-1748`). The
/// `min_words`/`min_chars`/`min_dur` gate (`_over_max`) restricts splitting to
/// segments meeting any threshold; the eligible set is fixed up front by
/// **original** position (stable-ts keys it on segment id, which equals the
/// pre-op position — see module note), so it is evaluated once here.
#[allow(clippy::too_many_arguments)]
fn split_by_punctuation(
    segs: &mut Vec<RSegment>,
    punct: &[PunctItem],
    lock: bool,
    newline: bool,
    min_words: Option<i64>,
    min_chars: Option<i64>,
    min_dur: Option<f64>,
    ignore_special_periods: bool,
) {
    let any_min = truthy(min_words) || truthy(min_chars) || truthy_f64(min_dur);
    let over_max: Vec<bool> = if any_min {
        segs.iter()
            .map(|s| over_max(s, min_words, min_chars, min_dur))
            .collect()
    } else {
        Vec::new()
    };

    split_segments(
        segs,
        |i, seg| {
            if !any_min || over_max[i] {
                punctuation_indices(seg, punct)
            } else {
                Vec::new()
            }
        },
        lock,
        newline,
        ignore_special_periods,
    );
}

/// `_over_max` (`result.py:1724-1729`): a segment qualifies if it meets **any**
/// active threshold (`>=`). Inactive (`None`/zero) thresholds don't contribute.
fn over_max(
    seg: &RSegment,
    min_words: Option<i64>,
    min_chars: Option<i64>,
    min_dur: Option<f64>,
) -> bool {
    (truthy(min_words) && seg.word_count() as i64 >= min_words.unwrap())
        || (truthy(min_chars) && seg.char_count() as i64 >= min_chars.unwrap())
        || (truthy_f64(min_dur) && seg.duration() >= min_dur.unwrap())
}

// --- split (length) ---------------------------------------------------------

/// `WhisperResult.split_by_length` (`result.py:1896-1959`). `force_len` first
/// merges everything into one segment; `ignore_special_periods` is applied in
/// [`split_segments`] (stable-ts does **not** forward it to
/// `get_length_indices`).
#[allow(clippy::too_many_arguments)]
fn split_by_length(
    segs: &mut Vec<RSegment>,
    max_chars: Option<i64>,
    max_words: Option<i64>,
    even_split: bool,
    force_len: bool,
    lock: bool,
    include_lock: bool,
    newline: bool,
    ignore_special_periods: bool,
) {
    if force_len {
        merge_all(segs);
    }
    split_segments(
        segs,
        |_i, seg| get_length_indices(seg, max_chars, max_words, even_split, include_lock),
        lock,
        newline,
        ignore_special_periods,
    );
}

/// `Segment.get_length_indices` (`result.py:788-851`). The `even_split` (true
/// by default) path computes evenly spaced split points via
/// [`even_split_indices`]; the greedy path breaks whenever the running char/word
/// count exceeds the cap. `ignore_special_periods` isn't threaded here because
/// `split_by_length` never passes it (it handles specials in the splitter).
fn get_length_indices(
    seg: &RSegment,
    max_chars: Option<i64>,
    max_words: Option<i64>,
    even_split: bool,
    include_lock: bool,
) -> Vec<usize> {
    let n = seg.words.len();
    if !seg.has_words() || (max_chars.is_none() && max_words.is_none()) {
        return Vec::new();
    }
    // stable-ts asserts max_chars != 0 and max_words != 0; humm rejects zero
    // caps at parse time (see parser::parse_sl), so both are non-zero here.
    if n < 2 {
        return Vec::new();
    }
    let locked: Vec<usize> = if include_lock {
        segment_locked_indices(seg)
    } else {
        Vec::new()
    };

    if even_split {
        let mut indices: Vec<usize> = Vec::new();
        let char_count = if max_chars.is_none() {
            -1
        } else {
            seg.char_count() as i64
        };
        let word_count = if max_words.is_none() { -1 } else { n as i64 };
        let exceed_chars = max_chars.is_some() && char_count > max_chars.unwrap();
        let mut exceed_words = max_words.is_some() && word_count > max_words.unwrap();

        if exceed_chars {
            let mc = max_chars.unwrap();
            let splits = (char_count as f64 / mc as f64).ceil();
            let chars_per_split = char_count as f64 / splits;
            let mut cum = Vec::with_capacity(n - 1);
            let mut acc = 0f64;
            for w in &seg.words[..n - 1] {
                acc += w.char_len() as f64;
                cum.push(acc);
            }
            indices = even_split_indices(&cum, chars_per_split, splits as i64, &locked, n);
            if let Some(mw) = max_words {
                // Re-check: does any char-split piece still exceed max_words?
                let mut left = vec![0i64];
                left.extend(indices.iter().map(|&x| x as i64));
                let mut right: Vec<i64> = indices.iter().map(|&x| x as i64).collect();
                right.push(n as i64);
                exceed_words = left.iter().zip(right.iter()).any(|(&i, &j)| j - i + 1 > mw);
            }
        }

        if exceed_words {
            let mw = max_words.unwrap();
            let splits = (word_count as f64 / mw as f64).ceil();
            let words_per_split = word_count as f64 / splits;
            let cum: Vec<f64> = (1..=n).map(|x| x as f64).collect();
            indices = even_split_indices(&cum, words_per_split, splits as i64, &locked, n);
        }
        indices
    } else {
        let locked_set: HashSet<usize> = locked.into_iter().collect();
        let mut indices: Vec<usize> = Vec::new();
        let mut curr_words = 0i64;
        let mut curr_chars = 0i64;
        for (i, w) in seg.words.iter().enumerate() {
            curr_words += 1;
            curr_chars += w.char_len() as i64;
            if i != 0 {
                let over = (max_chars.is_some() && curr_chars > max_chars.unwrap())
                    || (max_words.is_some() && curr_words > max_words.unwrap());
                if over && !locked_set.contains(&(i - 1)) {
                    indices.push(i - 1);
                    curr_words = 1;
                    curr_chars = w.char_len() as i64;
                }
            }
        }
        indices
    }
}

/// `Segment._get_even_split_indices` (`result.py:760-786`): for each of
/// `1..splits`, the word-boundary index whose cumulative count is nearest to
/// `i * parts_per_split` (numpy `argmin` → **first** minimum on ties). The
/// `excluded_indices` path (reached only with `include_lock` + prior locks)
/// remaps locked positions to their successor before the search.
fn even_split_indices(
    cumsum: &[f64],
    parts_per_split: f64,
    splits: i64,
    excluded: &[usize],
    n_words: usize,
) -> Vec<usize> {
    if cumsum.is_empty() || splits < 2 {
        return Vec::new();
    }
    if excluded.is_empty() {
        (1..splits)
            .map(|i| argmin_abs(cumsum, i as f64 * parts_per_split))
            .collect()
    } else {
        let mut cs = cumsum.to_vec();
        let mut cs_idx: Vec<usize> = (0..cumsum.len()).collect();
        let max_word_i = n_words - 1;
        let mut ex: Vec<usize> = excluded.to_vec();
        ex.sort_unstable();
        ex.dedup();
        for i in ex {
            if i == max_word_i || i + 1 >= cs.len() {
                break;
            }
            cs[i] = cs[i + 1];
            cs_idx[i] = cs_idx[i + 1];
        }
        let mut indices: Vec<usize> = (1..splits)
            .map(|i| cs_idx[argmin_abs(&cs, i as f64 * parts_per_split)])
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}

/// First index minimizing `|arr[k] - target|` (numpy `argmin` tie-break: the
/// lowest index).
fn argmin_abs(arr: &[f64], target: f64) -> usize {
    let mut best = 0usize;
    let mut best_val = (arr[0] - target).abs();
    for (k, &v) in arr.iter().enumerate().skip(1) {
        let d = (v - target).abs();
        if d < best_val {
            best_val = d;
            best = k;
        }
    }
    best
}

// --- generic split driver ---------------------------------------------------

/// `WhisperResult._split_segments` (`result.py:1446-1494`): walk segments **in
/// reverse**, compute split indices for each (passing its original position so
/// `split_by_punctuation`'s min-gate can key on it), and either insert `\n` at
/// those word positions (`newline`) or split the segment into pieces —
/// optionally locking the resulting seams (`lock`).
fn split_segments<F>(
    segs: &mut Vec<RSegment>,
    get_indices: F,
    lock: bool,
    newline: bool,
    ignore_special_periods: bool,
) where
    F: Fn(usize, &RSegment) -> Vec<usize>,
{
    for i in (0..segs.len()).rev() {
        let mut indices = get_indices(i, &segs[i]);
        indices.sort_unstable();
        indices.dedup();
        if ignore_special_periods {
            remove_special_period_indices(&segs[i], &mut indices);
        }
        if indices.is_empty() {
            continue;
        }
        if newline {
            let last = segs[i].words.len() - 1;
            if *indices.last().unwrap() == last {
                indices.pop();
                if indices.is_empty() {
                    continue;
                }
            }
            for &word_idx in &indices {
                let words = &mut segs[i].words;
                if words[word_idx].text.ends_with('\n') {
                    continue;
                }
                words[word_idx].text.push('\n');
                if lock {
                    words[word_idx].right_locked = true;
                    if word_idx + 1 < words.len() {
                        words[word_idx + 1].left_locked = true;
                    }
                }
            }
        } else {
            let mut pieces = split_segment(&segs[i], indices);
            if lock {
                let n = pieces.len();
                for (k, s) in pieces.iter_mut().enumerate() {
                    if k == 0 {
                        if let Some(w) = s.words.last_mut() {
                            w.right_locked = true;
                        }
                    } else if k == n - 1 {
                        if let Some(w) = s.words.first_mut() {
                            w.left_locked = true;
                        }
                    } else {
                        if let Some(w) = s.words.last_mut() {
                            w.right_locked = true;
                        }
                        if let Some(w) = s.words.first_mut() {
                            w.left_locked = true;
                        }
                    }
                }
            }
            segs.remove(i);
            for (off, s) in pieces.into_iter().enumerate() {
                segs.insert(i + off, s);
            }
        }
    }
    remove_no_word_segments(segs);
}

/// `Segment.split` (`result.py:886-902`): partition `words` at the given
/// (sorted, unique) boundary indices — a boundary `idx` ends a piece after
/// word `idx`. Always terminates the final piece at the last word.
fn split_segment(seg: &RSegment, mut indices: Vec<usize>) -> Vec<RSegment> {
    if indices.is_empty() {
        return Vec::new();
    }
    let last = seg.words.len() - 1;
    if *indices.last().unwrap() != last {
        indices.push(last);
    }
    let mut out = Vec::new();
    let mut prev = 0usize;
    for idx in indices {
        let end = idx + 1;
        if end > prev {
            out.push(RSegment {
                words: seg.words[prev..end].to_vec(),
            });
        }
        prev = end;
    }
    out
}

/// `WhisperResult._remove_special_period_indices` (`result.py:1434-1444`): drop
/// any split index landing on an abbreviation-like word — ends with `.`, starts
/// (after trim) with `A-Z`/`0-9`, and has fewer than three characters outside
/// `[.A-Z0-9]` (e.g. `U.S.`).
fn remove_special_period_indices(seg: &RSegment, indices: &mut Vec<usize>) {
    let mut i = indices.len();
    while i > 0 {
        i -= 1;
        let word = &seg.words[indices[i]].text;
        if !word.ends_with('.') {
            continue;
        }
        let stripped = word.trim();
        let starts_alnum = matches!(
            stripped.chars().next(),
            Some(c) if c.is_ascii_uppercase() || c.is_ascii_digit()
        );
        if !starts_alnum {
            continue;
        }
        let remaining = stripped
            .chars()
            .filter(|c| !(*c == '.' || c.is_ascii_uppercase() || c.is_ascii_digit()))
            .count();
        if remaining < 3 {
            indices.remove(i);
        }
    }
}

// --- shared helpers ---------------------------------------------------------

/// `WhisperResult.remove_no_word_segments` (`result.py:1334-1339`): drop
/// segments left with no words (splits/merges here never produce empties, so
/// this is a faithful no-op in practice).
fn remove_no_word_segments(segs: &mut Vec<RSegment>) {
    segs.retain(RSegment::has_words);
}

/// `WhisperResult.merge_all_segments` (`result.py:1863-1894`, has-words path):
/// concatenate every segment's words (keeping their locks) into a single
/// segment.
fn merge_all(segs: &mut Vec<RSegment>) {
    if segs.is_empty() {
        return;
    }
    let words: Vec<RWord> = segs.iter().flat_map(|s| s.words.iter().cloned()).collect();
    *segs = vec![RSegment { words }];
}

/// Python truthiness of an `Option<i64>` count argument: active iff `Some` and
/// non-zero (`if max_chars` is false for both `None` and `0`).
fn truthy(v: Option<i64>) -> bool {
    matches!(v, Some(x) if x != 0)
}

/// Python truthiness of an `Option<f64>` argument (`min_dur`).
fn truthy_f64(v: Option<f64>) -> bool {
    matches!(v, Some(x) if x != 0.0)
}
