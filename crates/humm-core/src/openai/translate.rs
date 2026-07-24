//! LLM-translation batch protocol and worker (design spec §4.1/§4.2/§4.3,
//! F-031/F-063/F-085), ported from Buzz's `Translator` (`buzz/translator.py`):
//!
//! - The pure, network-free batch protocol (`:82-145`): [`BATCH_SIZE`],
//!   [`combine_numbered`], [`batch_system_prompt`], [`parse_batch_response`].
//! - [`TranslatorHandle`]: a dedicated worker thread that drains a channel
//!   of enqueued transcripts, batches them (up to [`BATCH_SIZE`] at a time),
//!   and issues the actual `POST {base}/chat/completions` requests
//!   (`Translator.start`, `:147-186`).
//! - [`resolve_endpoint`]: env-var/settings/keyring precedence for the
//!   endpoint credentials (`Translator.__init__`, `:36-53`).
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
use std::thread;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use crate::secrets::{SecretKey, SecretStore};

use super::client::{ApiFailure, OpenAiAuth, effective_base};

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
/// Two **documented micro-deviations** from Buzz here:
///
/// - **Overflow** — design spec §4.1 pre-approves this one explicitly (a
///   real OpenAI-compatible LLM instructed with an ASCII `"[1] processed
///   text"` example is not expected to emit a >4-billion-digit marker). The
///   spec's own phrasing frames this as "parse as `u32`, ignore on
///   overflow"; this implementation instead folds with `saturating_mul`/
///   `saturating_add` in `u64` space, capping at `u64::MAX` rather than
///   rejecting outright at `u32::MAX`. The two are observably equivalent:
///   Python's `int()` is arbitrary-precision, so any number this large is
///   already well past a real batch index, and a saturated `u64::MAX`
///   value can never equal a real `1..=expected` batch index anyway
///   (`BATCH_SIZE` is 10) — the marker simply never matches an output slot,
///   identical to being ignored outright.
/// - **Non-ASCII Unicode decimal digits** (e.g. Arabic-Indic `٣`, U+0663) —
///   an **implementation decision, regression-tested**, not a spec-approved
///   deviation like the overflow cap above (spec §4.1 pre-approves only
///   that one). Python's `int()` accepts non-ASCII decimal digits
///   (`int("٣") == 3`) because CPython's `int()` consults each character's
///   Unicode numeric value; Rust's `char::to_digit` does not: empirically,
///   `'٣'.to_digit(10)` returns `None` even though `'٣'.is_numeric()` is
///   `true` — there is no equivalent in `std` without pulling in a
///   Unicode-data crate for this one edge case. Degenerate LLM output
///   either way. The marker regex itself still identifies the correct split
///   boundary (the `regex` crate's Unicode-mode `\d+` matches the same
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
///    Unicode-whitespace trim, approximated here by [`str::trim`], which
///    trims `char::is_whitespace`'s 25-character `White_Space`-property set
///    — not quite the same set Python's `str.strip()` trims: Python's is 29
///    characters, additionally treating the four C0 control characters
///    U+001C–U+001F (the "information separator" controls) as whitespace,
///    which `char::is_whitespace` does not) and stored in a map
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

// ============================================================================
// TranslatorHandle — the worker (design spec §4.2)
// ============================================================================

/// Construction parameters for a [`TranslatorHandle`] (design spec §4.2):
/// the endpoint credentials plus the two `TranscriptionOptions` fields
/// Buzz's `Translator` reads (`llm_model`/`llm_prompt`, `translator.py:36`,
/// `:63`, `:92`).
///
/// `#[derive(Debug)]` is safe here even though `auth` carries the bearer API
/// key: a derived `Debug` impl formats each field via *that field's own*
/// `Debug` impl, and [`OpenAiAuth`] implements it by hand to redact
/// `api_key` (see its doc comment) — so `{:?}` on a `TranslatorConfig` never
/// prints the key.
#[derive(Debug)]
pub struct TranslatorConfig {
    /// Endpoint credentials: optional custom base URL + bearer API key.
    pub auth: OpenAiAuth,
    /// The chat-completion model id (Buzz `transcription_options.llm_model`).
    pub model: String,
    /// The user's translation instruction: sent verbatim as the system
    /// message for a single-item request, or prefixed onto the
    /// numbered-batch instructions (via [`batch_system_prompt`]) for a batch
    /// (Buzz `transcription_options.llm_prompt`).
    pub prompt: String,
}

/// One message on a [`TranslatorHandle`]'s internal channel.
///
/// Deliberately not `pub`: nothing outside this module needs to name it —
/// only [`TranslatorHandle`]'s own methods construct and send it, and its
/// worker loop is private too. Callers interact with the channel only
/// through [`TranslatorHandle::enqueue`]/[`TranslatorHandle::update_options`]/
/// [`TranslatorHandle::stop`]/[`TranslatorHandle::stop_detached`].
#[derive(Debug)]
enum TranslatorMsg {
    /// One transcript to translate, carrying its caller-assigned id (e.g. a
    /// segment ordinal for file-task translation; `None` when the caller has
    /// nothing to correlate against — Buzz's `Optional[int]`,
    /// `translator.py:188`).
    Item {
        /// The transcript text to translate.
        text: String,
        /// Caller-assigned correlation id, echoed back unchanged via
        /// `on_result`.
        id: Option<i64>,
    },
    /// Swaps `model`/`prompt` for every request issued after this message is
    /// processed by the worker (Buzz's `on_transcription_options_changed`
    /// signal handler, `:180-183`, fired by the live advanced-settings
    /// sheet).
    UpdateOptions {
        /// The new chat-completion model id.
        model: String,
        /// The new translation instruction.
        prompt: String,
    },
    /// Sentinel: stop the worker (Buzz's `queue.put(None)`, `:198`).
    Stop,
}

/// Per-request timeout for every `/chat/completions` call: 60 s, Buzz's
/// `timeout=60.0` (`translator.py:69`/`:106`). No retries — Buzz constructs
/// its OpenAI client with `max_retries=0` (`:53`), and this worker issues
/// requests directly via a plain `reqwest::blocking::Client` rather than
/// `openai::client::cancelable` (which exists for the *task/queue* path's
/// prompt-cancel guarantee, ADR-022; this worker's own thread is disposable
/// and `Stop` is honored between requests, bounded by this same timeout).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A running LLM-translation worker (F-031/F-063/F-085, design spec §4.2): a
/// dedicated `"humm-translator"` thread that drains a queue of enqueued
/// transcripts, batches them (up to [`BATCH_SIZE`] at a time) into
/// `/chat/completions` requests, and reports each translation back through
/// the `on_result` callback given to [`Self::spawn`].
///
/// Ported from Buzz's `Translator` (`translator.py`): construction reads the
/// endpoint once (Buzz `:36-53`; humm's equivalent is [`resolve_endpoint`],
/// run by the caller before [`Self::spawn`], since [`TranslatorConfig`]
/// takes already-resolved [`OpenAiAuth`]); `start`'s blocking-queue loop
/// (`:147-186`) is this struct's worker thread over a `flume` channel;
/// `enqueue`/`stop` (`:188-198`) are [`Self::enqueue`]/[`Self::stop`]; the
/// `advanced_settings_dialog` options-changed signal (`:33-35`, `:180-183`)
/// is [`Self::update_options`].
pub struct TranslatorHandle {
    sender: flume::Sender<TranslatorMsg>,
    join: thread::JoinHandle<()>,
}

impl TranslatorHandle {
    /// Spawns the worker on a dedicated thread named `"humm-translator"` and
    /// returns a handle to it immediately — construction never blocks on the
    /// network.
    ///
    /// `on_result` is called exactly once per enqueued item, in the order
    /// items were enqueued within their batch, always on the worker thread
    /// (never the caller's) — see the worker loop semantics documented on
    /// this struct's other methods for exactly when a batch runs.
    pub fn spawn(
        cfg: TranslatorConfig,
        on_result: Box<dyn FnMut(String, Option<i64>) + Send>,
    ) -> Self {
        let (sender, receiver) = flume::unbounded();
        let join = thread::Builder::new()
            .name("humm-translator".to_string())
            .spawn(move || worker_loop(cfg, receiver, on_result))
            .expect("spawn humm-translator thread");
        TranslatorHandle { sender, join }
    }

    /// Queues one transcript for translation (Buzz `enqueue`, `:188-192`).
    /// Returns immediately; translation happens asynchronously on the
    /// worker thread. A silent no-op if the worker has already stopped
    /// (sending into a channel whose receiver is gone).
    pub fn enqueue(&self, text: String, id: Option<i64>) {
        let _ = self.sender.send(TranslatorMsg::Item { text, id });
    }

    /// Swaps the model/prompt used by requests issued after this message is
    /// processed by the worker (Buzz's `on_transcription_options_changed`,
    /// `:180-183`).
    ///
    /// Applied strictly *between* requests in humm's channel model — either
    /// between two batches (if drained by the blocking receive that starts
    /// the next batch), or immediately upon being drained mid-batch, before
    /// that batch's own request goes out (if drained while non-blockingly
    /// filling the current batch). Buzz's Qt signal, by contrast, can land
    /// asynchronously *during* an in-flight request and only affects the
    /// *next* call to `_translate_single`/`_translate_batch` since Python
    /// has no channel to queue it on — humm's ordering is never looser than
    /// Buzz's, only ever at least as prompt.
    pub fn update_options(&self, model: String, prompt: String) {
        let _ = self
            .sender
            .send(TranslatorMsg::UpdateOptions { model, prompt });
    }

    /// Stops the worker and waits for it to exit (Buzz `stop`, `:196-198`,
    /// plus the join Buzz's caller performs on the underlying `QThread`).
    ///
    /// Sends the `Stop` sentinel, then blocks joining the worker thread. The
    /// wait is bounded: the worker honors `Stop` between requests (or after
    /// finishing a batch that had `Stop` queued mid-drain — see
    /// [`Self::update_options`]'s sibling drain semantics), and every
    /// request has its own [`REQUEST_TIMEOUT`], so this returns within
    /// roughly one in-flight request's timeout, never indefinitely.
    pub fn stop(self) {
        let _ = self.sender.send(TranslatorMsg::Stop);
        let _ = self.join.join();
    }

    /// Stops the worker without waiting for it — the GUI close path, which
    /// must return promptly rather than block the UI thread on up to
    /// [`REQUEST_TIMEOUT`].
    ///
    /// Sends the `Stop` sentinel, then drops the handle. Rust threads are
    /// detached by default (unlike C++'s `std::thread`): dropping a
    /// [`thread::JoinHandle`] without calling `join` lets the thread keep
    /// running to completion independently, so the worker still drains and
    /// reports every item queued before `Stop` — only this call's *return*
    /// doesn't wait for that, which is the whole point of this method
    /// existing alongside [`Self::stop`].
    pub fn stop_detached(self) {
        let _ = self.sender.send(TranslatorMsg::Stop);
    }
}

/// The worker thread body (Buzz `Translator.start`, `translator.py:147-186`,
/// ported exactly):
///
/// 1. **Block** on the next message. `Stop`, or the channel disconnecting
///    (every [`TranslatorHandle`] dropped without a `Stop` ever having been
///    sent — Python's `queue.Queue` has no channel-disconnect concept, so
///    this case has no exact Buzz analogue; the nearest read is "nothing
///    left to ever call `queue.put` again") — both end the worker
///    immediately. `UpdateOptions` applies the new model/prompt and loops
///    back to block again (there is no batch to run yet).
/// 2. Once an `Item` is received, **start a batch** `[item]` and drain up to
///    `BATCH_SIZE - 1` more messages *without blocking* (`try_recv`, Buzz
///    `:161-168`'s `get_nowait`/`queue.Empty`):
///    - a drained `Item` joins the batch;
///    - a drained `UpdateOptions` is applied *immediately* (mutates the
///      config, then draining continues) rather than waiting for this batch
///      to finish — see [`TranslatorHandle::update_options`]'s doc comment
///      for how this compares to Buzz's asynchronous signal delivery;
///    - a drained `Stop` is remembered (`stop_after_batch = true`) and
///      draining stops right there, without consuming anything still queued
///      behind it (Buzz `:163-166`);
///    - an empty channel just stops draining (Buzz's `queue.Empty`); a
///      disconnected channel does too — that case is instead caught by the
///      *next* iteration's blocking receive in step 1, which is where it
///      actually ends the worker.
/// 3. **Dispatch the batch:** exactly one item uses the single-request form
///    (Buzz `_translate_single`, `:59-80`); more than one uses the
///    numbered-batch form (Buzz `_translate_batch`, `:82-127`, via
///    [`batch_system_prompt`]/[`combine_numbered`]/[`parse_batch_response`]).
///    Every item in the batch gets exactly one `on_result(translation, id)`
///    call, in the batch's order (Buzz `:173-179`); a request/transport
///    error or a malformed response yields `""` for every item in that
///    batch (Buzz `:70-72`/`:107-114`), logged via `log::error!`.
/// 4. If `stop_after_batch` was set in step 2, the worker exits right after
///    dispatching (Buzz `:181-183`); otherwise it loops back to step 1.
fn worker_loop(
    mut cfg: TranslatorConfig,
    receiver: flume::Receiver<TranslatorMsg>,
    mut on_result: Box<dyn FnMut(String, Option<i64>) + Send>,
) {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());

    loop {
        // Step 1: block for the next batch's first item.
        let first = loop {
            match receiver.recv() {
                Ok(TranslatorMsg::Item { text, id }) => break Some((text, id)),
                Ok(TranslatorMsg::UpdateOptions { model, prompt }) => {
                    cfg.model = model;
                    cfg.prompt = prompt;
                }
                Ok(TranslatorMsg::Stop) | Err(_) => break None,
            }
        };
        let Some(first) = first else {
            return;
        };

        // Step 2: drain up to BATCH_SIZE - 1 more, non-blocking.
        let mut batch = vec![first];
        let mut stop_after_batch = false;
        while batch.len() < BATCH_SIZE {
            match receiver.try_recv() {
                Ok(TranslatorMsg::Item { text, id }) => batch.push((text, id)),
                Ok(TranslatorMsg::UpdateOptions { model, prompt }) => {
                    cfg.model = model;
                    cfg.prompt = prompt;
                }
                Ok(TranslatorMsg::Stop) => {
                    stop_after_batch = true;
                    break;
                }
                Err(_) => break,
            }
        }

        // Step 3: dispatch, then step 4: honor a mid-drain Stop.
        run_batch(&client, &cfg, &batch, on_result.as_mut());

        if stop_after_batch {
            return;
        }
    }
}

/// Dispatches one drained batch and reports every item's result via
/// `on_result`, in batch order (Buzz `:169-179`).
fn run_batch(
    client: &reqwest::blocking::Client,
    cfg: &TranslatorConfig,
    batch: &[(String, Option<i64>)],
    on_result: &mut dyn FnMut(String, Option<i64>),
) {
    if let [(text, id)] = batch {
        let translation = translate_single(client, cfg, text);
        on_result(translation, *id);
        return;
    }

    let texts: Vec<&str> = batch.iter().map(|(text, _)| text.as_str()).collect();
    let translations = translate_batch(client, cfg, &texts);
    for ((_, id), translation) in batch.iter().zip(translations) {
        on_result(translation, *id);
    }
}

/// The single-item request form (Buzz `_translate_single`, `:59-80`):
/// `messages = [{role: system, content: prompt}, {role: user, content:
/// text}]`. On any failure, logs (Buzz `:70-72`) and returns `""`.
fn translate_single(
    client: &reqwest::blocking::Client,
    cfg: &TranslatorConfig,
    text: &str,
) -> String {
    match post_chat_completion(client, cfg, &cfg.prompt, text) {
        Ok(content) => content,
        Err(e) => {
            log::error!("Translation error! Server response: {e}");
            String::new()
        }
    }
}

/// The numbered-batch request form (Buzz `_translate_batch`, `:82-127`):
/// system = [`batch_system_prompt`], user = [`combine_numbered`]. On any
/// failure, logs (Buzz `:107-114`) and returns `""` for every item;
/// otherwise parses the response via [`parse_batch_response`], which always
/// returns exactly `texts.len()` entries (`""` for any missing or
/// unparseable marker).
fn translate_batch(
    client: &reqwest::blocking::Client,
    cfg: &TranslatorConfig,
    texts: &[&str],
) -> Vec<String> {
    let n = texts.len();
    let system_content = batch_system_prompt(&cfg.prompt, n);
    let user_content = combine_numbered(texts);

    match post_chat_completion(client, cfg, &system_content, &user_content) {
        Ok(content) => parse_batch_response(&content, n),
        Err(e) => {
            log::error!("Batch translation error! Server response: {e}");
            vec![String::new(); n]
        }
    }
}

/// Issues one `POST {base}/chat/completions` request and extracts
/// `choices[0].message.content` — the shape both the single-item and batch
/// forms share (Buzz `:62-69`/`:102-106` build the request;
/// `completion.choices[0].message.content` extracts the result, `:76`/
/// `:120`). No retries; [`REQUEST_TIMEOUT`] is applied at the client level.
///
/// Every error path's `detail` is built only from the effective base URL,
/// the response status/body, and a transport error's own message — never
/// from the request's `Authorization` header — so a failure can never leak
/// the API key (design spec §2), matching [`ApiFailure`]'s own contract.
fn post_chat_completion(
    client: &reqwest::blocking::Client,
    cfg: &TranslatorConfig,
    system_content: &str,
    user_content: &str,
) -> Result<String, ApiFailure> {
    let base = effective_base(&cfg.auth.base_url);
    let url = format!("{base}/chat/completions");

    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [
            {"role": "system", "content": system_content},
            {"role": "user", "content": user_content},
        ],
    });

    let response = client
        .post(&url)
        .bearer_auth(&cfg.auth.api_key)
        .json(&body)
        .send()
        .map_err(|e| ApiFailure::Http {
            detail: format!("request to {base} failed: {e}"),
        })?;

    let status = response.status();
    let text = response.text().map_err(|e| ApiFailure::Http {
        detail: format!("failed reading response from {base}: {e}"),
    })?;

    if !status.is_success() {
        return Err(ApiFailure::Http {
            detail: format!("{base} returned HTTP {status}"),
        });
    }

    let value: Value = serde_json::from_str(&text).map_err(|e| ApiFailure::Shape {
        detail: format!("{base} returned a non-JSON response body (HTTP {status}): {e}"),
    })?;

    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ApiFailure::Shape {
            detail: format!("{base} response missing choices[0].message.content"),
        })
}

// ============================================================================
// Endpoint resolution (design spec §4.3)
// ============================================================================

/// humm's environment variable for a custom LLM-translation base URL
/// (design spec §4.3). Buzz's equivalent is `BUZZ_TRANSLATION_API_BASE_URl`
/// (`translator.py:44`) — note the trailing lowercase `l`: that is a typo in
/// upstream Buzz itself, not a porting error here. humm spells it correctly
/// (`URL`, all-uppercase) rather than carrying the typo forward, since the
/// `BUZZ_` → `HUMM_` prefix rename already breaks environment-variable-name
/// compatibility with Buzz — there is no exact-string migration left to
/// preserve by keeping the typo too. Documented here (and design spec §4.3 /
/// `HUMM_SURFACE` §5) so a user migrating from Buzz and grepping either
/// spelling finds this mapping.
pub const ENV_BASE: &str = "HUMM_TRANSLATION_API_BASE_URL";

/// humm's environment variable for a custom LLM-translation API key (design
/// spec §4.3). Buzz's equivalent is `BUZZ_TRANSLATION_API_KEY`
/// (`translator.py:47`).
pub const ENV_KEY: &str = "HUMM_TRANSLATION_API_KEY";

/// The resolved LLM-translation endpoint (design spec §4.3): just an
/// [`OpenAiAuth`], the same credential type `openai::engine`'s and
/// `openai::validate`'s OpenAI-compatible call sites use, so every call site
/// shares one redacting `Debug` impl and one `effective_base`/bearer-auth
/// convention.
pub struct LlmEndpoint {
    /// Endpoint credentials.
    pub auth: OpenAiAuth,
}

/// Resolves the LLM-translation endpoint from environment overrides,
/// settings, and the secret store, following Buzz's `os.getenv(VAR,
/// fallback)` precedence exactly (`translator.py:43-52`, design spec §4.3):
///
/// - **Base URL:** `env_base` if `Some` and non-empty, else
///   `settings_base.clone()` (itself possibly `None`, meaning "use the
///   client's built-in default" via `effective_base`).
/// - **API key:** `env_key` if `Some` and non-empty, else
///   `secrets.get(SecretKey::OpenAiApiKey)` — a read failure is logged and
///   treated as "no key", the same as a missing entry (Buzz's
///   `get_password` returns `""` on any failure, `keyring_store.py:60-68`,
///   an equivalent degrade). An empty stored key is likewise treated as
///   absent.
///
/// Returns `None` when no key is available from either source: there is
/// nothing an [`LlmEndpoint`] built from it could authenticate with, so
/// none is constructed — callers skip translation entirely rather than
/// firing requests doomed to fail auth (design spec §6's "endpoint
/// unresolvable" degradation).
///
/// Reads **no** environment variables itself — `env_base`/`env_key` are
/// values the caller already read via `std::env::var`. Keeping env access
/// out of this function is deliberate: tests running in parallel must never
/// race on process-global environment state, the same rule
/// `openai::validate`'s and `openai::engine`'s tests already follow.
pub fn resolve_endpoint(
    env_base: Option<String>,
    env_key: Option<String>,
    settings_base: &Option<String>,
    secrets: &dyn SecretStore,
) -> Option<LlmEndpoint> {
    let base_url = match env_base {
        Some(base) if !base.is_empty() => Some(base),
        _ => settings_base.clone(),
    };

    let api_key = match env_key {
        Some(key) if !key.is_empty() => Some(key),
        _ => match secrets.get(SecretKey::OpenAiApiKey) {
            Ok(Some(key)) if !key.is_empty() => Some(key),
            Ok(_) => None,
            Err(e) => {
                log::error!("failed reading OpenAI API key from secret store: {e}");
                None
            }
        },
    }?;

    Some(LlmEndpoint {
        auth: OpenAiAuth { base_url, api_key },
    })
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
    /// (`validation/m6-differential/gen_batch_parse_fixtures.py`), is
    /// discovered by walking the fixture directory at test time rather than
    /// being named in a hardcoded list here — a new fixture file dropped
    /// into that directory is picked up automatically, with no
    /// corresponding edit to this test required (and none to forget).
    #[test]
    fn parse_batch_response_matches_python_reference_fixtures() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/batch_parse");

        let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("reading fixture dir {}: {e}", dir.display()))
            .map(|entry| entry.expect("reading fixture dir entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        entries.sort();

        // Guards against an empty (or near-empty) directory silently passing
        // this test — the fixture matrix documented in design spec §4.1 has
        // 13 named cases as of Task 7 (12 original + the tab-whitespace case
        // added in review); this floor should only ever grow.
        assert!(
            entries.len() >= 13,
            "expected at least 13 fixture files in {}, found {}",
            dir.display(),
            entries.len()
        );

        for path in entries {
            let json = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("reading fixture {}: {e}", path.display()));
            let fx: Fixture = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("fixture {}: JSON: {e}", path.display()));

            let actual = parse_batch_response(&fx.response, fx.expected_count);

            assert_eq!(
                actual,
                fx.parsed,
                "fixture {} mismatch (response={:?}, expected_count={})",
                path.display(),
                fx.response,
                fx.expected_count
            );
        }
    }

    // ------------------------------------------------------ TranslatorConfig
    //
    // Verifies the claim in `TranslatorConfig`'s doc comment: a derived
    // `Debug` impl formats each field via that field's own `Debug`, so it
    // never bypasses `OpenAiAuth`'s hand-written redaction (see
    // `client::tests::debug_redacts_api_key` for that impl's own coverage).

    #[test]
    fn translator_config_debug_redacts_api_key_via_auths_manual_impl() {
        let cfg = TranslatorConfig {
            auth: OpenAiAuth {
                base_url: None,
                api_key: "sk-super-secret-value-must-not-be-printed".to_string(),
            },
            model: "gpt-4o-mini".to_string(),
            prompt: "Translate to French.".to_string(),
        };

        let debug_output = format!("{cfg:?}");

        assert!(
            !debug_output.contains("sk-super-secret-value-must-not-be-printed"),
            "Debug output must not contain the key material, got: {debug_output}"
        );
        assert!(debug_output.contains("«redacted»"));
        assert!(debug_output.contains("TranslatorConfig"));
    }

    // ------------------------------------------------------ resolve_endpoint
    //
    // Network-free: exercises only `resolve_endpoint`'s precedence logic
    // against a `MemorySecretStore`, no TCP involved.

    mod endpoint_resolution {
        use super::*;
        use crate::secrets::MemorySecretStore;

        #[test]
        fn env_key_wins_over_keyring() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "keyring-key").unwrap();

            let endpoint = resolve_endpoint(None, Some("env-key".to_string()), &None, &secrets)
                .expect("endpoint resolves");

            assert_eq!(endpoint.auth.api_key, "env-key");
        }

        #[test]
        fn keyring_key_used_when_no_env_key() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "keyring-key").unwrap();

            let endpoint =
                resolve_endpoint(None, None, &None, &secrets).expect("endpoint resolves");

            assert_eq!(endpoint.auth.api_key, "keyring-key");
        }

        #[test]
        fn env_base_wins_over_settings_base() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "some-key").unwrap();

            let endpoint = resolve_endpoint(
                Some("http://env-base.example".to_string()),
                None,
                &Some("http://settings-base.example".to_string()),
                &secrets,
            )
            .expect("endpoint resolves");

            assert_eq!(
                endpoint.auth.base_url,
                Some("http://env-base.example".to_string())
            );
        }

        #[test]
        fn settings_base_used_when_no_env_base() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "some-key").unwrap();

            let endpoint = resolve_endpoint(
                None,
                None,
                &Some("http://settings-base.example".to_string()),
                &secrets,
            )
            .expect("endpoint resolves");

            assert_eq!(
                endpoint.auth.base_url,
                Some("http://settings-base.example".to_string())
            );
        }

        #[test]
        fn empty_env_base_treated_as_unset_falls_back_to_settings() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "some-key").unwrap();

            let endpoint = resolve_endpoint(
                Some(String::new()),
                None,
                &Some("http://settings-base.example".to_string()),
                &secrets,
            )
            .expect("endpoint resolves");

            assert_eq!(
                endpoint.auth.base_url,
                Some("http://settings-base.example".to_string())
            );
        }

        #[test]
        fn empty_env_key_treated_as_unset_falls_back_to_keyring() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "keyring-key").unwrap();

            let endpoint = resolve_endpoint(None, Some(String::new()), &None, &secrets)
                .expect("endpoint resolves");

            assert_eq!(endpoint.auth.api_key, "keyring-key");
        }

        #[test]
        fn no_key_anywhere_is_none() {
            let secrets = MemorySecretStore::new();

            assert!(resolve_endpoint(None, None, &None, &secrets).is_none());
        }

        #[test]
        fn no_key_anywhere_is_none_even_with_a_base_configured() {
            let secrets = MemorySecretStore::new();

            assert!(
                resolve_endpoint(
                    Some("http://env-base.example".to_string()),
                    None,
                    &Some("http://settings-base.example".to_string()),
                    &secrets,
                )
                .is_none()
            );
        }

        #[test]
        fn key_without_any_base_defaults_base_url_to_none() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "some-key").unwrap();

            let endpoint =
                resolve_endpoint(None, None, &None, &secrets).expect("endpoint resolves");

            assert_eq!(endpoint.auth.base_url, None);
        }

        #[test]
        fn empty_stored_key_treated_as_absent() {
            let secrets = MemorySecretStore::new();
            secrets.set(SecretKey::OpenAiApiKey, "").unwrap();

            assert!(resolve_endpoint(None, None, &None, &secrets).is_none());
        }

        #[test]
        fn env_base_and_env_key_both_used_verbatim() {
            let secrets = MemorySecretStore::new();

            let endpoint = resolve_endpoint(
                Some("http://custom.example/v1".to_string()),
                Some("env-key".to_string()),
                &None,
                &secrets,
            )
            .expect("endpoint resolves");

            assert_eq!(endpoint.auth.api_key, "env-key");
            assert_eq!(
                endpoint.auth.base_url,
                Some("http://custom.example/v1".to_string())
            );
        }

        #[test]
        fn env_names_match_documented_humm_variables() {
            assert_eq!(ENV_BASE, "HUMM_TRANSLATION_API_BASE_URL");
            assert_eq!(ENV_KEY, "HUMM_TRANSLATION_API_KEY");
        }
    }

    // -------------------------------------------------- TranslatorHandle worker
    //
    // Binds TCP (`TestServer::start`) -- blocked under the sandbox, run with
    // it lifted (see this task's report for the full documented set).

    mod worker {
        use super::*;
        use crate::models::test_server::{Canned, TestServer};
        use std::sync::mpsc;

        const RECV_TIMEOUT: Duration = Duration::from_secs(5);

        fn cfg_for(server: &TestServer, model: &str, prompt: &str) -> TranslatorConfig {
            TranslatorConfig {
                auth: OpenAiAuth {
                    base_url: Some(server.base_url()),
                    api_key: "sk-test".to_string(),
                },
                model: model.to_string(),
                prompt: prompt.to_string(),
            }
        }

        /// A canned `{"choices":[{"message":{"content": "..."}}]}` body.
        fn choice_response(content: &str) -> Vec<u8> {
            serde_json::json!({
                "choices": [ { "message": { "content": content } } ]
            })
            .to_string()
            .into_bytes()
        }

        /// A boxed `on_result` callback that forwards every call onto an
        /// `mpsc` channel the test can `recv_timeout` on -- the synchronization
        /// primitive every test in this module uses instead of a real sleep.
        /// Boxed `on_result` callback type, factored out purely to keep
        /// [`on_result_channel`]'s signature clippy-quiet
        /// (`clippy::type_complexity`).
        type OnResult = Box<dyn FnMut(String, Option<i64>) + Send>;

        fn on_result_channel() -> (OnResult, mpsc::Receiver<(String, Option<i64>)>) {
            let (tx, rx) = mpsc::channel();
            let on_result: OnResult = Box::new(move |text, id| {
                let _ = tx.send((text, id));
            });
            (on_result, rx)
        }

        fn recv(rx: &mpsc::Receiver<(String, Option<i64>)>) -> (String, Option<i64>) {
            rx.recv_timeout(RECV_TIMEOUT)
                .expect("on_result callback within timeout")
        }

        /// Blocks on `join` from a watcher thread and reports back over a
        /// channel, bounding the wait rather than risking an indefinite hang
        /// if the worker never actually exits -- a genuine synchronization
        /// primitive, not a fixed-duration sleep.
        fn join_with_timeout(join: thread::JoinHandle<()>, timeout: Duration) -> bool {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = join.join();
                let _ = tx.send(());
            });
            rx.recv_timeout(timeout).is_ok()
        }

        #[test]
        fn single_item_sends_single_request_form() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("Bonjour")),
            );

            let cfg = cfg_for(&server, "gpt-4o-mini", "Translate to French.");
            let (on_result, rx) = on_result_channel();
            let handle = TranslatorHandle::spawn(cfg, on_result);

            handle.enqueue("Hello".to_string(), Some(1));
            assert_eq!(recv(&rx), ("Bonjour".to_string(), Some(1)));

            handle.stop();

            let requests = server.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].method, "POST");
            assert_eq!(requests[0].path, "/chat/completions");
            assert_eq!(
                requests[0].headers.get("authorization").map(String::as_str),
                Some("Bearer sk-test")
            );

            let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
            assert_eq!(body["model"], "gpt-4o-mini");
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(body["messages"][0]["content"], "Translate to French.");
            assert_eq!(body["messages"][1]["role"], "user");
            assert_eq!(body["messages"][1]["content"], "Hello");
        }

        #[test]
        fn ten_pre_queued_items_batch_into_one_numbered_request() {
            let server = TestServer::start();
            let numbered_content: String = (1..=10)
                .map(|i| format!("[{i}] out{i}"))
                .collect::<Vec<_>>()
                .join("\n");
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response(&numbered_content)),
            );

            let cfg = cfg_for(&server, "gpt-4o-mini", "Translate to French.");

            // Pre-queue all 10 items directly on a manually-built channel
            // before the worker ever runs (rather than via
            // `TranslatorHandle::spawn` + `enqueue`), so the batch is
            // deterministic instead of racing this test thread's enqueue
            // calls against the worker thread's startup -- BATCH_SIZE is 10,
            // so anything less than all 10 already queued before the first
            // drain would split across two requests.
            let (tx, rx_chan) = flume::unbounded::<TranslatorMsg>();
            let texts: Vec<String> = (1..=10).map(|i| format!("text{i}")).collect();
            for (i, text) in texts.iter().enumerate() {
                tx.send(TranslatorMsg::Item {
                    text: text.clone(),
                    id: Some(i as i64 + 1),
                })
                .expect("send pre-queued item");
            }
            drop(tx); // channel disconnects once drained -- worker exits after the batch

            let (on_result, rx) = on_result_channel();
            let join = thread::spawn(move || worker_loop(cfg, rx_chan, on_result));

            let mut results = Vec::new();
            for _ in 0..10 {
                results.push(recv(&rx));
            }
            assert!(
                join_with_timeout(join, RECV_TIMEOUT),
                "worker did not exit after its pre-queued batch and channel disconnect"
            );

            assert_eq!(results.len(), 10);
            for (i, (text, id)) in results.iter().enumerate() {
                assert_eq!(*id, Some(i as i64 + 1));
                assert_eq!(text, &format!("out{}", i + 1));
            }

            let requests = server.requests();
            assert_eq!(
                requests.len(),
                1,
                "expected exactly one batched request, got {requests:?}"
            );

            let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
            let system = body["messages"][0]["content"].as_str().unwrap();
            assert!(
                system.contains("You will receive 10 numbered texts"),
                "system prompt was: {system}"
            );
            let user = body["messages"][1]["content"].as_str().unwrap();
            let text_refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            assert_eq!(user, combine_numbered(&text_refs));
        }

        #[test]
        fn degraded_response_missing_marker_yields_empty_string_for_that_item() {
            let server = TestServer::start();
            // 3-item batch; the response omits marker [2].
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("[1] uno\n[3] tres")),
            );

            let cfg = cfg_for(&server, "gpt-4o-mini", "Translate to Spanish.");
            let (tx, rx_chan) = flume::unbounded::<TranslatorMsg>();
            for (text, id) in [("one", 1), ("two", 2), ("three", 3)] {
                tx.send(TranslatorMsg::Item {
                    text: text.to_string(),
                    id: Some(id),
                })
                .unwrap();
            }
            drop(tx);

            let (on_result, rx) = on_result_channel();
            let join = thread::spawn(move || worker_loop(cfg, rx_chan, on_result));

            let results = vec![recv(&rx), recv(&rx), recv(&rx)];
            assert!(join_with_timeout(join, RECV_TIMEOUT), "worker did not exit");

            assert_eq!(
                results,
                vec![
                    ("uno".to_string(), Some(1)),
                    (String::new(), Some(2)),
                    ("tres".to_string(), Some(3)),
                ]
            );
        }

        #[test]
        fn http_500_yields_empty_string_for_every_item_in_the_batch() {
            let server = TestServer::start();
            server.set_route_for_method("POST", "/chat/completions", Canned::status(500));

            let cfg = cfg_for(&server, "gpt-4o-mini", "Translate to German.");
            let (tx, rx_chan) = flume::unbounded::<TranslatorMsg>();
            for (text, id) in [("one", 1), ("two", 2), ("three", 3)] {
                tx.send(TranslatorMsg::Item {
                    text: text.to_string(),
                    id: Some(id),
                })
                .unwrap();
            }
            drop(tx);

            let (on_result, rx) = on_result_channel();
            let join = thread::spawn(move || worker_loop(cfg, rx_chan, on_result));

            let results = vec![recv(&rx), recv(&rx), recv(&rx)];
            assert!(join_with_timeout(join, RECV_TIMEOUT), "worker did not exit");

            assert_eq!(
                results,
                vec![
                    (String::new(), Some(1)),
                    (String::new(), Some(2)),
                    (String::new(), Some(3)),
                ]
            );
        }

        #[test]
        fn update_options_between_batches_changes_the_next_request() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("ok")),
            );

            let cfg = cfg_for(&server, "model-a", "Translate.");
            let (on_result, rx) = on_result_channel();
            let handle = TranslatorHandle::spawn(cfg, on_result);

            handle.enqueue("first".to_string(), Some(1));
            recv(&rx); // batch 1 fully processed -- the worker is back at its blocking receive

            handle.update_options("model-b".to_string(), "Translate differently.".to_string());

            handle.enqueue("second".to_string(), Some(2));
            recv(&rx); // batch 2 fully processed

            handle.stop();

            let requests = server.requests();
            assert_eq!(requests.len(), 2);

            let body0: Value = serde_json::from_slice(&requests[0].body).unwrap();
            let body1: Value = serde_json::from_slice(&requests[1].body).unwrap();
            assert_eq!(body0["model"], "model-a");
            assert_eq!(body0["messages"][0]["content"], "Translate.");
            assert_eq!(body1["model"], "model-b");
            assert_eq!(body1["messages"][0]["content"], "Translate differently.");
        }

        #[test]
        fn stop_drains_pre_queued_items_before_exiting() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("ok")),
            );

            let cfg = cfg_for(&server, "gpt-4o-mini", "Translate.");
            let (on_result, rx) = on_result_channel();
            let handle = TranslatorHandle::spawn(cfg, on_result);

            handle.enqueue("one".to_string(), Some(1));
            handle.enqueue("two".to_string(), Some(2));
            handle.enqueue("three".to_string(), Some(3));
            // `stop` sends `Stop` strictly after the three enqueues above
            // (same handle, same thread -- flume preserves per-sender send
            // order) and blocks until the worker has drained everything
            // queued ahead of it, so every callback below is available by
            // the time `stop` returns.
            handle.stop();

            let mut ids: Vec<Option<i64>> = (0..3).map(|_| recv(&rx).1).collect();
            ids.sort();
            assert_eq!(ids, vec![Some(1), Some(2), Some(3)]);
        }

        #[test]
        fn drop_without_stop_ends_the_worker() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("ok")),
            );

            let cfg = cfg_for(&server, "gpt-4o-mini", "Translate.");
            let (on_result, rx) = on_result_channel();
            let handle = TranslatorHandle::spawn(cfg, on_result);

            handle.enqueue("one".to_string(), Some(1));
            recv(&rx); // the item's request has fully completed by now

            // Simulate "drop the handle without ever calling stop()" (no
            // `Stop` message is sent): decompose the handle -- this test
            // module is a descendant of `translate`, so it can see
            // `TranslatorHandle`'s private fields -- and drop only the
            // sender, exactly what dropping the whole handle would do to
            // that field. The worker's next blocking `recv()` then sees a
            // disconnected channel and returns, per "channel disconnected ≡
            // Stop".
            let TranslatorHandle { sender, join } = handle;
            drop(sender);

            assert!(
                join_with_timeout(join, RECV_TIMEOUT),
                "worker thread did not exit after its sender was dropped"
            );
        }

        #[test]
        fn stop_detached_sends_stop_and_returns_without_the_caller_joining() {
            let server = TestServer::start();
            server.set_route_for_method(
                "POST",
                "/chat/completions",
                Canned::ok(choice_response("ok")),
            );

            let cfg = cfg_for(&server, "gpt-4o-mini", "Translate.");
            let (on_result, rx) = on_result_channel();
            let handle = TranslatorHandle::spawn(cfg, on_result);

            handle.enqueue("one".to_string(), Some(1));
            recv(&rx);

            // `stop_detached` takes `self` by value and returns `()`
            // (no `JoinHandle` handed back) -- the call completing at all,
            // without this test providing any way to block on the worker,
            // is what proves the caller isn't made to wait on it.
            handle.stop_detached();
        }
    }
}
