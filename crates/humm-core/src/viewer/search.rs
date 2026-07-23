//! Transcript search: row-scoped find (Timestamps view) and text-scoped
//! find (Text/Translation view), plus label formatting and next/prev
//! wrap-around.
//!
//! Ported from Buzz's `perform_search`/`search_in_table`/`search_in_text`/
//! `update_search_ui`/`update_search_results_label`/`search_next`/
//! `search_previous` (`transcription_viewer_widget.py:774-934`).

/// Shared cap on the number of matches kept by either search path — Buzz's
/// `max_results = 100` (`transcription_viewer_widget.py:801`, `:826`).
pub const SEARCH_CAP: usize = 100;

/// Delay, in milliseconds, between the last query edit and running the
/// search — Buzz's `search_debounce_timer.start(300)`
/// (`transcription_viewer_widget.py:780`).
pub const SEARCH_DEBOUNCE_MS: u64 = 300;

/// One row-scoped search match (Timestamps view).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowMatch {
    /// Index into the segment rows.
    pub row: usize,
    /// Whether the match was found in the row's translation column, rather
    /// than its text column.
    pub in_translation: bool,
}

/// Searches segment rows for `query`, Timestamps-view semantics.
///
/// Two passes, sharing one [`SEARCH_CAP`]: first every row whose `text`
/// (`.0`) contains `query` case-insensitively, then — only when
/// `has_translations` — every row whose `translation` (`.1`) contains it.
/// A row can appear in both passes. Ported from `search_in_table`
/// (`transcription_viewer_widget.py:798-819`).
pub fn search_rows(
    rows: &[(String, String)],
    has_translations: bool,
    query: &str,
) -> Vec<RowMatch> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for (row, (text, _)) in rows.iter().enumerate() {
        if results.len() >= SEARCH_CAP {
            break;
        }
        if text.to_lowercase().contains(&query_lower) {
            results.push(RowMatch {
                row,
                in_translation: false,
            });
        }
    }

    if has_translations {
        for (row, (_, translation)) in rows.iter().enumerate() {
            if results.len() >= SEARCH_CAP {
                break;
            }
            if translation.to_lowercase().contains(&query_lower) {
                results.push(RowMatch {
                    row,
                    in_translation: true,
                });
            }
        }
    }

    results
}

/// One text-scoped search match (Text/Translation view): byte offsets
/// valid in the original (not lowercased) haystack string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMatch {
    /// Byte offset of the match start.
    pub start: usize,
    /// Byte offset just past the match end.
    pub end: usize,
}

/// Searches `haystack` for `query`, Text/Translation-view semantics: a
/// char-wise case-insensitive substring scan, capped at [`SEARCH_CAP`].
///
/// Ported from `search_in_text` (`transcription_viewer_widget.py:823-838`),
/// with one deliberate fix: Buzz lowercases the whole haystack then indexes
/// into the lowercased copy, which can misalign offsets when a character's
/// lowercase form has a different UTF-8 byte length than the original —
/// those offsets are then applied to the *original* string for cursor
/// placement. This scans char-by-char over the original string instead, so
/// every returned offset is always a valid slice bound on `haystack`
/// (ledger note, spec §2.3).
pub fn search_text(haystack: &str, query: &str) -> Vec<TextMatch> {
    let query_chars: Vec<char> = query.chars().collect();
    let qlen = query_chars.len();
    let mut results = Vec::new();
    if qlen == 0 {
        return results;
    }

    let hay_chars: Vec<(usize, char)> = haystack.char_indices().collect();
    if qlen > hay_chars.len() {
        return results;
    }

    for start in 0..=(hay_chars.len() - qlen) {
        if results.len() >= SEARCH_CAP {
            break;
        }
        let matches = (0..qlen).all(|k| char_eq_ci(hay_chars[start + k].1, query_chars[k]));
        if matches {
            let start_byte = hay_chars[start].0;
            let end_byte = hay_chars
                .get(start + qlen)
                .map(|&(b, _)| b)
                .unwrap_or(haystack.len());
            results.push(TextMatch {
                start: start_byte,
                end: end_byte,
            });
        }
    }

    results
}

/// Single-character case-insensitive equality via Unicode case folding
/// (`char::to_lowercase`), not ASCII-only comparison.
fn char_eq_ci(a: char, b: char) -> bool {
    a.to_lowercase().eq(b.to_lowercase())
}

/// Wraps forward to the next result index — Buzz's
/// `(current + 1) % len(results)` (`transcription_viewer_widget.py:900-901`).
/// `len == 0` returns `0` (callers never invoke this without results, per
/// Buzz's own guard in `search_next_if_results`).
pub fn wrap_next(current: usize, len: usize) -> usize {
    if len == 0 { 0 } else { (current + 1) % len }
}

/// Wraps backward to the previous result index — Buzz's
/// `(current - 1) % len(results)` (`transcription_viewer_widget.py:906-907`),
/// expressed without relying on Python's always-non-negative `%`.
pub fn wrap_prev(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (current + len - 1) % len
    }
}

/// Formats the results label, byte-exact with Buzz's `update_search_ui`
/// (`transcription_viewer_widget.py:844-852`) and
/// `update_search_results_label` (`:927-934`): `"No matches found"` when
/// empty, `"{n} of 100+ matches"` at the cap, else `"{n} of {total}
/// matches"` — `current` is the 0-based index of the highlighted match.
pub fn results_label(current: usize, total: usize) -> String {
    if total == 0 {
        return "No matches found".to_string();
    }
    if total >= SEARCH_CAP {
        format!("{} of 100+ matches", current + 1)
    } else {
        format!("{} of {total} matches", current + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn search_rows_caps_at_exactly_100_across_both_passes() {
        // 60 rows match on text, 60 more match only on translation — pass 1
        // alone is under the cap, pass 2 must stop it at exactly 100.
        let mut rows = Vec::new();
        for i in 0..60 {
            rows.push((format!("needle {i}"), String::new()));
        }
        for i in 0..60 {
            rows.push((String::new(), format!("needle {i}")));
        }
        let results = search_rows(&rows, true, "needle");
        assert_eq!(results.len(), SEARCH_CAP);
        assert_eq!(results.iter().filter(|m| !m.in_translation).count(), 60);
        assert_eq!(results.iter().filter(|m| m.in_translation).count(), 40);
    }

    #[test]
    fn search_rows_skips_translation_pass_when_flag_is_false() {
        let rows = vec![(String::new(), "needle in translation".to_string())];
        let results = search_rows(&rows, false, "needle");
        assert!(results.is_empty());
    }

    #[test]
    fn search_rows_orders_text_matches_before_translation_matches() {
        let rows = vec![
            ("no match here".to_string(), "needle".to_string()),
            ("needle".to_string(), "no match here".to_string()),
        ];
        let results = search_rows(&rows, true, "needle");
        assert_eq!(
            results,
            vec![
                RowMatch {
                    row: 1,
                    in_translation: false
                },
                RowMatch {
                    row: 0,
                    in_translation: true
                },
            ]
        );
    }

    #[test]
    fn search_rows_allows_a_row_to_match_in_both_passes() {
        let rows = vec![("cat sat".to_string(), "cat translation".to_string())];
        let results = search_rows(&rows, true, "cat");
        assert_eq!(
            results,
            vec![
                RowMatch {
                    row: 0,
                    in_translation: false
                },
                RowMatch {
                    row: 0,
                    in_translation: true
                },
            ]
        );
    }

    #[test]
    fn search_rows_is_case_insensitive() {
        let rows = vec![("A Cat Sat".to_string(), String::new())];
        let results = search_rows(&rows, false, "cAT");
        assert_eq!(
            results,
            vec![RowMatch {
                row: 0,
                in_translation: false
            }]
        );
    }

    #[test]
    fn search_text_byte_offsets_are_valid_on_non_ascii_haystack() {
        let haystack = "Müller ist hier";
        let matches = search_text(haystack, "müller");
        assert_eq!(matches.len(), 1);
        let m = matches[0];
        assert_eq!(&haystack[m.start..m.end], "Müller");
    }

    #[test]
    fn search_text_is_case_insensitive_and_capped() {
        let haystack = "a".repeat(150);
        let matches = search_text(&haystack, "a");
        assert_eq!(matches.len(), SEARCH_CAP);
    }

    #[test]
    fn search_text_finds_no_matches_for_absent_query() {
        assert!(search_text("hello world", "xyz").is_empty());
    }

    #[test]
    fn wrap_next_and_prev_are_modulo_including_len_one() {
        assert_eq!(wrap_next(0, 1), 0);
        assert_eq!(wrap_prev(0, 1), 0);
        assert_eq!(wrap_next(2, 3), 0);
        assert_eq!(wrap_prev(0, 3), 2);
        assert_eq!(wrap_next(0, 3), 1);
        assert_eq!(wrap_prev(1, 3), 0);
    }

    #[test]
    fn results_label_is_byte_exact_with_buzz() {
        assert_eq!(results_label(0, 0), "No matches found");
        assert_eq!(results_label(2, 7), "3 of 7 matches");
        assert_eq!(results_label(0, 150), "1 of 100+ matches");
        assert_eq!(results_label(99, 100), "100 of 100+ matches");
    }
}
