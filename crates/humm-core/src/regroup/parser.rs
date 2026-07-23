//! `parse_regroup_algo` grammar port (stable-ts `result.py:2980-3024`,
//! `str_to_valid_type` `utils.py:20-30`), scoped to the three UI-reachable ops
//! `mg`/`sp`/`sl` (spec ledger #8). Any other op key yields an actionable
//! [`RegroupError::UnsupportedOp`] naming the op and the supported set.
//!
//! Grammar (from the pinned source):
//! - ops separated by `_`; an op is `key` or `key=args`.
//! - args separated by `+`, mapped **positionally** to the method's parameters
//!   (`zip(co_varnames[1:], args)`); an empty arg (`''`) parses to `None` and
//!   is **skipped**, leaving that parameter at its Python default (so
//!   `mg=0.2++42+1` sets `min_gap`, skips `max_words`, sets `max_chars`,
//!   `is_sum_max`).
//! - `str_to_valid_type`: a `/`-containing arg becomes a **list** (each item
//!   `*`-split into an ending/beginning **pair** iff it contains `*`, e.g.
//!   `.* ` → `('.', ' ')`); otherwise `int` when it has no `.`, `float` when it
//!   does, falling back to the raw string on a parse failure.

/// The regroup ops humm implements (Buzz's resizer only ever emits these).
pub const SUPPORTED_OPS: &[&str] = &["mg", "sp", "sl"];

/// Error from parsing a regroup algorithm string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegroupError {
    /// An op key outside humm's supported subset (`mg`/`sp`/`sl`). stable-ts
    /// defines ~20 ops; humm ports only the resizer-reachable three and names
    /// both the offending op and the supported set so an operator setting
    /// `HUMM_MERGE_REGROUP_RULE` gets an actionable message.
    #[error(
        "unsupported regroup op {op:?}; humm supports only {supported:?} \
         (set via the resizer merge checkboxes or HUMM_MERGE_REGROUP_RULE)"
    )]
    UnsupportedOp {
        /// The offending op key.
        op: String,
        /// The supported op keys (`["mg", "sp", "sl"]`).
        supported: &'static [&'static str],
    },
    /// An op's arguments could not be parsed into that op's parameters (wrong
    /// type, too many, or a malformed `*`-pair / missing required punctuation).
    #[error("invalid regroup arguments: {0}")]
    BadArgs(String),
}

/// One item of a split/merge punctuation list: either a plain string
/// (word-ends-with rule) or an ending/beginning `(a, b)` pair (word-ends-with-a
/// **and** next-word-starts-with-b rule) — `Segment.get_punctuation_indices`,
/// `result.py:729-747`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PunctItem {
    /// Plain-string rule: split where a word ends with (or, past the first
    /// word, starts with) this string.
    Str(String),
    /// Pair rule `(ending, beginning)`: split between a word ending with
    /// `ending` and the next word beginning with `beginning`.
    Pair(String, String),
}

/// A parsed, typed regroup operation. Field defaults match the stable-ts method
/// signatures; a skipped (`''`) positional argument leaves its field at that
/// default.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    /// `merge_by_gap` (`result.py:1642`).
    MergeByGap {
        /// `min_gap` — merge adjacent segments whose gap ≤ this (fake-seconds).
        min_gap: f64,
        /// `max_words` — per-segment (or summed) word cap gating the merge.
        max_words: Option<i64>,
        /// `max_chars` — per-segment (or summed) char cap gating the merge.
        max_chars: Option<i64>,
        /// `is_sum_max` — apply the caps to the merged pair's sum.
        is_sum_max: bool,
        /// `lock` — lock the merged boundary against later ops.
        lock: bool,
        /// `newline` — insert `\n` between merged segments instead of a space.
        newline: bool,
    },
    /// `split_by_punctuation` (`result.py:1689`).
    SplitByPunctuation {
        /// `punctuation` — the list of string/pair rules to split at.
        punctuation: Vec<PunctItem>,
        /// `lock` — lock the split boundaries against later ops.
        lock: bool,
        /// `newline` — insert `\n` at split points instead of splitting.
        newline: bool,
        /// `min_words` — only split segments with ≥ this many words.
        min_words: Option<i64>,
        /// `min_chars` — only split segments with ≥ this many chars.
        min_chars: Option<i64>,
        /// `min_dur` — only split segments with ≥ this duration (fake-seconds).
        min_dur: Option<f64>,
        /// `ignore_special_periods` — skip splits at abbreviation-like periods.
        ignore_special_periods: bool,
    },
    /// `split_by_length` (`result.py:1896`).
    SplitByLength {
        /// `max_chars` — split segments exceeding this many chars.
        max_chars: Option<i64>,
        /// `max_words` — split segments exceeding this many words.
        max_words: Option<i64>,
        /// `even_split` — split evenly (default) vs. greedily.
        even_split: bool,
        /// `force_len` — merge-all first, then split to a constant length.
        force_len: bool,
        /// `lock` — lock the split boundaries against later ops.
        lock: bool,
        /// `include_lock` — respect prior locks when greedily splitting.
        include_lock: bool,
        /// `newline` — insert `\n` at split points instead of splitting.
        newline: bool,
        /// `ignore_special_periods` — skip splits at abbreviation-like periods.
        ignore_special_periods: bool,
    },
}

/// The `str_to_valid_type` result for one positional argument (`None` = the
/// empty `''` arg, which is skipped so the parameter keeps its default).
#[derive(Debug, Clone, PartialEq)]
enum RawArg {
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<PunctItem>),
}

/// `str_to_valid_type` (`utils.py:20-30`). Returns `Ok(None)` for the empty
/// string (Python's `None`). The `/`-list branch is checked **before** numeric
/// coercion, and list items are never numerically coerced (Python keeps them as
/// `str`/nested-`list`).
fn str_to_valid_type(val: &str) -> Result<Option<RawArg>, RegroupError> {
    if val.is_empty() {
        return Ok(None);
    }
    if val.contains('/') {
        let mut items = Vec::new();
        for a in val.split('/') {
            if a.contains('*') {
                let parts: Vec<&str> = a.split('*').collect();
                // Python stores `a.split('*')` and later unpacks `ending,
                // beginning = p`, which requires exactly two parts (a runtime
                // ValueError otherwise); humm surfaces that as an actionable
                // parse error up front.
                if parts.len() == 2 {
                    items.push(PunctItem::Pair(parts[0].to_string(), parts[1].to_string()));
                } else {
                    return Err(RegroupError::BadArgs(format!(
                        "punctuation list item {a:?} splits into {} parts on '*'; \
                         expected 1 (a string) or 2 (an ending*beginning pair)",
                        parts.len()
                    )));
                }
            } else {
                items.push(PunctItem::Str(a.to_string()));
            }
        }
        return Ok(Some(RawArg::List(items)));
    }
    if val.contains('.') {
        Ok(Some(match val.parse::<f64>() {
            Ok(f) => RawArg::Float(f),
            Err(_) => RawArg::Str(val.to_string()),
        }))
    } else {
        Ok(Some(match val.parse::<i64>() {
            Ok(i) => RawArg::Int(i),
            Err(_) => RawArg::Str(val.to_string()),
        }))
    }
}

impl RawArg {
    fn as_f64(&self, op: &str, pos: usize) -> Result<f64, RegroupError> {
        match self {
            RawArg::Int(i) => Ok(*i as f64),
            RawArg::Float(f) => Ok(*f),
            _ => Err(bad_arg(op, pos, "a number")),
        }
    }

    fn as_count(&self, op: &str, pos: usize) -> Result<i64, RegroupError> {
        match self {
            RawArg::Int(i) => Ok(*i),
            _ => Err(bad_arg(op, pos, "an integer")),
        }
    }

    /// Python truthiness of an `int`/`float` argument (`1`/`0` → `True`/
    /// `False`; any non-zero is truthy).
    fn as_bool(&self, op: &str, pos: usize) -> Result<bool, RegroupError> {
        match self {
            RawArg::Int(i) => Ok(*i != 0),
            RawArg::Float(f) => Ok(*f != 0.0),
            _ => Err(bad_arg(op, pos, "0 or 1")),
        }
    }

    /// `punctuation` accepts a `/`-list or a bare string (stable-ts wraps a
    /// lone `str` as `[str]` in `get_punctuation_indices`).
    fn as_punct(&self, op: &str, pos: usize) -> Result<Vec<PunctItem>, RegroupError> {
        match self {
            RawArg::List(items) => Ok(items.clone()),
            RawArg::Str(s) => Ok(vec![PunctItem::Str(s.clone())]),
            _ => Err(bad_arg(
                op,
                pos,
                "punctuation (a string or a '/'-separated list)",
            )),
        }
    }
}

fn bad_arg(op: &str, pos: usize, expected: &str) -> RegroupError {
    RegroupError::BadArgs(format!("{op} argument {pos} must be {expected}"))
}

/// Present, non-skipped argument at position `i` (skipped `''`/absent → `None`).
fn arg_at(args: &[Option<RawArg>], i: usize) -> Option<&RawArg> {
    args.get(i).and_then(Option::as_ref)
}

fn check_arity(op: &str, args: &[Option<RawArg>], max: usize) -> Result<(), RegroupError> {
    if args.len() > max {
        return Err(RegroupError::BadArgs(format!(
            "{op} takes at most {max} argument(s), got {}",
            args.len()
        )));
    }
    Ok(())
}

fn parse_mg(args: &[Option<RawArg>]) -> Result<Rule, RegroupError> {
    check_arity("mg", args, 6)?;
    let mut rule = Rule::MergeByGap {
        min_gap: 0.1,
        max_words: None,
        max_chars: None,
        is_sum_max: false,
        lock: false,
        newline: false,
    };
    if let Rule::MergeByGap {
        min_gap,
        max_words,
        max_chars,
        is_sum_max,
        lock,
        newline,
    } = &mut rule
    {
        if let Some(a) = arg_at(args, 0) {
            *min_gap = a.as_f64("mg", 0)?;
        }
        if let Some(a) = arg_at(args, 1) {
            *max_words = Some(a.as_count("mg", 1)?);
        }
        if let Some(a) = arg_at(args, 2) {
            *max_chars = Some(a.as_count("mg", 2)?);
        }
        if let Some(a) = arg_at(args, 3) {
            *is_sum_max = a.as_bool("mg", 3)?;
        }
        if let Some(a) = arg_at(args, 4) {
            *lock = a.as_bool("mg", 4)?;
        }
        if let Some(a) = arg_at(args, 5) {
            *newline = a.as_bool("mg", 5)?;
        }
    }
    Ok(rule)
}

fn parse_sp(args: &[Option<RawArg>]) -> Result<Rule, RegroupError> {
    check_arity("sp", args, 7)?;
    let punctuation = match arg_at(args, 0) {
        Some(a) => a.as_punct("sp", 0)?,
        None => {
            return Err(RegroupError::BadArgs(
                "sp requires a punctuation argument (e.g. sp=,/. )".into(),
            ));
        }
    };
    let mut rule = Rule::SplitByPunctuation {
        punctuation,
        lock: false,
        newline: false,
        min_words: None,
        min_chars: None,
        min_dur: None,
        ignore_special_periods: false,
    };
    if let Rule::SplitByPunctuation {
        lock,
        newline,
        min_words,
        min_chars,
        min_dur,
        ignore_special_periods,
        ..
    } = &mut rule
    {
        if let Some(a) = arg_at(args, 1) {
            *lock = a.as_bool("sp", 1)?;
        }
        if let Some(a) = arg_at(args, 2) {
            *newline = a.as_bool("sp", 2)?;
        }
        if let Some(a) = arg_at(args, 3) {
            *min_words = Some(a.as_count("sp", 3)?);
        }
        if let Some(a) = arg_at(args, 4) {
            *min_chars = Some(a.as_count("sp", 4)?);
        }
        if let Some(a) = arg_at(args, 5) {
            *min_dur = Some(a.as_f64("sp", 5)?);
        }
        if let Some(a) = arg_at(args, 6) {
            *ignore_special_periods = a.as_bool("sp", 6)?;
        }
    }
    Ok(rule)
}

fn parse_sl(args: &[Option<RawArg>]) -> Result<Rule, RegroupError> {
    check_arity("sl", args, 8)?;
    let mut rule = Rule::SplitByLength {
        max_chars: None,
        max_words: None,
        even_split: true,
        force_len: false,
        lock: false,
        include_lock: false,
        newline: false,
        ignore_special_periods: false,
    };
    if let Rule::SplitByLength {
        max_chars,
        max_words,
        even_split,
        force_len,
        lock,
        include_lock,
        newline,
        ignore_special_periods,
    } = &mut rule
    {
        // stable-ts `get_length_indices` asserts `max_chars != 0 and
        // max_words != 0`; reject a zero cap here (parse time) so `apply`
        // stays infallible.
        if let Some(a) = arg_at(args, 0) {
            let v = a.as_count("sl", 0)?;
            if v == 0 {
                return Err(RegroupError::BadArgs("sl max_chars must not be 0".into()));
            }
            *max_chars = Some(v);
        }
        if let Some(a) = arg_at(args, 1) {
            let v = a.as_count("sl", 1)?;
            if v == 0 {
                return Err(RegroupError::BadArgs("sl max_words must not be 0".into()));
            }
            *max_words = Some(v);
        }
        if let Some(a) = arg_at(args, 2) {
            *even_split = a.as_bool("sl", 2)?;
        }
        if let Some(a) = arg_at(args, 3) {
            *force_len = a.as_bool("sl", 3)?;
        }
        if let Some(a) = arg_at(args, 4) {
            *lock = a.as_bool("sl", 4)?;
        }
        if let Some(a) = arg_at(args, 5) {
            *include_lock = a.as_bool("sl", 5)?;
        }
        if let Some(a) = arg_at(args, 6) {
            *newline = a.as_bool("sl", 6)?;
        }
        if let Some(a) = arg_at(args, 7) {
            *ignore_special_periods = a.as_bool("sl", 7)?;
        }
    }
    Ok(rule)
}

/// Parse a regroup algorithm string into an ordered list of [`Rule`]s.
///
/// Ops run left-to-right in the order returned. An empty string yields an empty
/// list (a no-op regroup). Unsupported op keys → [`RegroupError::UnsupportedOp`].
pub fn parse_rules(algo: &str) -> Result<Vec<Rule>, RegroupError> {
    if algo.is_empty() {
        return Ok(Vec::new());
    }
    let mut rules = Vec::new();
    for method in algo.split('_') {
        let (key, args_str) = method.split_once('=').unwrap_or((method, ""));
        let args: Vec<Option<RawArg>> = if args_str.is_empty() {
            Vec::new()
        } else {
            args_str
                .split('+')
                .map(str_to_valid_type)
                .collect::<Result<_, _>>()?
        };
        let rule = match key {
            "mg" => parse_mg(&args)?,
            "sp" => parse_sp(&args)?,
            "sl" => parse_sl(&args)?,
            other => {
                return Err(RegroupError::UnsupportedOp {
                    op: other.to_string(),
                    supported: SUPPORTED_OPS,
                });
            }
        };
        rules.push(rule);
    }
    Ok(rules)
}
