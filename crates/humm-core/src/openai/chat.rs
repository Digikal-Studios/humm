//! Shared OpenAI-compatible chat-completions call (design spec §6.1):
//! `POST {base}/chat/completions` with a single system+user message pair,
//! extracted verbatim from `openai::translate`'s private
//! `post_chat_completion` so `postproc::summary` (F-111's ai_summary hook)
//! can reuse it instead of duplicating the request/response plumbing. The
//! translator's own differential-parse test suite staying green unchanged
//! is this extraction's behavior-neutrality proof — no test lives in this
//! module itself.

use serde_json::Value;

use super::client::{ApiFailure, OpenAiAuth, effective_base};

/// Issues one `POST {base}/chat/completions` request — `messages = [{role:
/// system, content: system}, {role: user, content: user}]` — and extracts
/// `choices[0].message.content`: the shape every `openai::*` chat caller
/// shares (Buzz `_translate_single`/`_translate_batch`,
/// `translator.py:62-69`/`:102-106`; `ai_summary/plugin.py:136-145`).
///
/// No retries; `client`'s own configured timeout (and nothing else) bounds
/// the request — callers each build their own client with whatever timeout
/// their caller's Buzz counterpart used (the translator's 60 s, ai_summary's
/// 120 s).
///
/// Every error path's `detail` is built only from the effective base URL,
/// the response status/body, and a transport error's own message — never
/// from `auth.api_key` — so a failure can never leak the API key (design
/// spec §2), matching [`ApiFailure`]'s own contract.
pub(crate) fn complete(
    client: &reqwest::blocking::Client,
    auth: &OpenAiAuth,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, ApiFailure> {
    let base = effective_base(&auth.base_url);
    let url = format!("{base}/chat/completions");

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });

    let response = client
        .post(&url)
        .bearer_auth(&auth.api_key)
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
