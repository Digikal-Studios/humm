//! Shared HTTP client plumbing for `humm_core::openai` (design spec §2):
//! base-URL normalization, the key-carrying [`OpenAiAuth`] credential type,
//! request-failure mapping ([`ApiFailure`]), and the run-on-its-own-thread
//! cancel mechanism every blocking `openai::*` request goes through
//! (§8/ADR-022).
//!
//! [`crate::openai::validate`] is the one exception to "goes through
//! [`cancelable`]": its two probes have their own short fixed timeouts and
//! no cancel affordance in the UI (Buzz's Test button has none either), so
//! it calls `reqwest` directly.

use std::sync::mpsc;
use std::time::Duration;

use crate::cancel::CancelToken;

/// The default OpenAI API base URL, used whenever no custom base is
/// configured (`OpenAiAuth::base_url` is `None` or empty) — see
/// [`effective_base`].
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Credentials for one OpenAI-compatible endpoint: an optional custom base
/// URL and the bearer API key sent as `Authorization: Bearer <api_key>`.
///
/// The key travels only as this in-memory `String`, constructed once per
/// engine/translator/validate call and read only to build the
/// `Authorization` header (design spec §2). It is never `Debug`-printed —
/// see the manual [`std::fmt::Debug`] impl below, which exists precisely
/// because `#[derive(Debug)]` would print `api_key` verbatim in any
/// `{:?}`/`dbg!`/panic message, defeating the whole point of keeping the key
/// out of logs.
#[derive(Clone)]
pub struct OpenAiAuth {
    /// Custom base URL override (e.g. a self-hosted OpenAI-compatible
    /// server). `None` or an empty string both mean "use the default" —
    /// see [`effective_base`].
    pub base_url: Option<String>,
    /// The bearer API key.
    pub api_key: String,
}

impl std::fmt::Debug for OpenAiAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiAuth")
            .field("base_url", &self.base_url)
            .field("api_key", &"«redacted»")
            .finish()
    }
}

/// Resolves the effective API base URL from a configured
/// [`OpenAiAuth::base_url`] (design spec §2): `None` or an empty string
/// fall back to [`DEFAULT_BASE_URL`]; a custom value is used verbatim with
/// any trailing `/` trimmed.
pub fn effective_base(base_url: &Option<String>) -> String {
    match base_url.as_deref() {
        Some(base) if !base.is_empty() => base.trim_end_matches('/').to_string(),
        _ => DEFAULT_BASE_URL.to_string(),
    }
}

/// A failure from an `openai::*` blocking request.
///
/// Every variant's `detail` is built only from response status codes,
/// response bodies, and a transport error's own message — **never** from
/// the request's `Authorization` header — so a failure can never leak the
/// API key (design spec §2).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ApiFailure {
    /// The request was canceled via a [`CancelToken`] before it produced a
    /// result. Returned promptly (~100 ms poll interval) even mid-request;
    /// see [`cancelable`]'s doc comment for what happens to the underlying
    /// request thread.
    #[error("canceled")]
    Canceled,
    /// A transport-level failure (connection refused, timeout, DNS, TLS,
    /// ...) or a non-2xx HTTP response.
    #[error("OpenAI API request failed: {detail}")]
    Http {
        /// Actionable detail: the failing host/status, or the transport
        /// error's own message.
        detail: String,
    },
    /// The response was a successful HTTP status but its body didn't match
    /// the shape the caller expected (e.g. a missing expected JSON field).
    #[error("OpenAI API returned an unexpected response: {detail}")]
    Shape {
        /// Actionable detail describing the shape mismatch.
        detail: String,
    },
}

/// Runs `send` to completion on its own short-lived, named thread, and
/// blocks the calling thread on the result — polling `cancel` roughly every
/// 100 ms while it waits.
///
/// This is the mechanism behind ADR-022 (design spec §8): every blocking
/// `openai::*` HTTP call runs through here rather than being made directly
/// on the caller's thread, so a [`CancelToken::cancel`] call gets a <1 s
/// acknowledgment (UX_SPEC §4) *even mid-request* — something a plain
/// blocking `reqwest` call on the caller's own thread could never offer
/// (there is no safe way to interrupt a blocking socket read from another
/// thread).
///
/// **On cancel, the request thread is detached — not joined, not aborted.**
/// Rust has no safe mechanism to forcibly kill a thread mid-syscall, so the
/// only two honest choices are "block the caller until the request thread
/// eventually finishes" (which defeats the entire point of a prompt cancel
/// acknowledgment) or "stop waiting for it and let it run to its own
/// conclusion in the background" (chosen here). The detached thread's
/// lifetime is still bounded: `send`'s own client-level request timeout
/// guarantees it exits on its own even though nothing joins it, and its
/// eventual `tx.send(..)` against a receiver that has already been dropped
/// is a silent no-op — not a panic. In practice only one caller routes
/// through here today, `OpenAiEngine::transcribe_one`'s 600 s per-chunk
/// request (`engine.rs`); `validate`'s two probes (5 s/15 s) and the
/// translator worker's batched requests (60 s, `translate.rs`) deliberately
/// bypass this wrapper and call `reqwest` directly on their own
/// short-lived/disposable threads instead (this module's top-level doc
/// comment, and ADR-022 in `docs/design/DECISIONS.md`, explain why).
pub fn cancelable<T: Send + 'static>(
    cancel: &CancelToken,
    send: impl FnOnce() -> Result<T, ApiFailure> + Send + 'static,
) -> Result<T, ApiFailure> {
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("openai-request".to_string())
        .spawn(move || {
            let result = send();
            // The receiver may already be gone (the caller canceled and
            // returned before this send) -- expected and fine, not an
            // error worth handling.
            let _ = tx.send(result);
        })
        .expect("spawn openai request thread");

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => {
                // The request thread has already finished (it just sent
                // its result), so this join is immediate -- not an extra
                // wait on top of the timeout above.
                let _ = handle.join();
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if cancel.is_canceled() {
                    // Detach: see the doc comment above for the rationale.
                    drop(handle);
                    return Err(ApiFailure::Canceled);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The request thread ended without sending a result -- only
                // possible if it panicked. Surface that as an `Http`
                // failure rather than propagating the panic to the caller.
                let _ = handle.join();
                return Err(ApiFailure::Http {
                    detail: "OpenAI request thread ended unexpectedly".to_string(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    // -- effective_base -------------------------------------------------

    #[test]
    fn effective_base_none_is_default() {
        assert_eq!(effective_base(&None), DEFAULT_BASE_URL);
    }

    #[test]
    fn effective_base_empty_string_is_default() {
        assert_eq!(effective_base(&Some(String::new())), DEFAULT_BASE_URL);
    }

    #[test]
    fn effective_base_custom_value_used_verbatim() {
        assert_eq!(
            effective_base(&Some("http://localhost:8000/v1".to_string())),
            "http://localhost:8000/v1"
        );
    }

    #[test]
    fn effective_base_custom_value_trims_trailing_slash() {
        assert_eq!(
            effective_base(&Some("http://localhost:8000/v1/".to_string())),
            "http://localhost:8000/v1"
        );
    }

    // -- redacted Debug ---------------------------------------------------

    #[test]
    fn debug_redacts_api_key() {
        let auth = OpenAiAuth {
            base_url: None,
            api_key: "sk-super-secret-value-must-not-be-printed".to_string(),
        };

        let debug_output = format!("{auth:?}");

        assert!(
            !debug_output.contains("sk-super-secret-value-must-not-be-printed"),
            "Debug output must not contain the key material, got: {debug_output}"
        );
        assert!(debug_output.contains("«redacted»"));
        assert!(debug_output.contains("OpenAiAuth"));
    }

    // -- cancelable ---------------------------------------------------------

    #[test]
    fn cancelable_returns_promptly_on_cancel() {
        let cancel = CancelToken::new();
        let cancel_for_canceler = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancel_for_canceler.cancel();
        });

        let start = Instant::now();
        let result = cancelable(&cancel, || {
            std::thread::sleep(Duration::from_secs(5));
            Ok::<_, ApiFailure>(42)
        });
        let elapsed = start.elapsed();

        assert!(matches!(result, Err(ApiFailure::Canceled)));
        assert!(
            elapsed < Duration::from_secs(1),
            "cancel ack took too long: {elapsed:?}"
        );
    }

    #[test]
    fn cancelable_passes_through_success_and_failure() {
        let cancel = CancelToken::new();

        let ok = cancelable(&cancel, || Ok::<_, ApiFailure>(7));
        assert!(matches!(ok, Ok(7)));

        let err = cancelable(&cancel, || {
            Err::<i32, _>(ApiFailure::Http {
                detail: "boom".to_string(),
            })
        });
        assert!(matches!(err, Err(ApiFailure::Http { detail }) if detail == "boom"));
    }

    #[test]
    fn cancelable_does_not_wait_out_a_poll_tick_on_fast_completion() {
        let cancel = CancelToken::new();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_inner = Arc::clone(&flag);

        let start = Instant::now();
        let result = cancelable(&cancel, move || {
            flag_inner.store(true, Ordering::SeqCst);
            Ok::<_, ApiFailure>(())
        });
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert!(flag.load(Ordering::SeqCst));
        assert!(
            elapsed < Duration::from_millis(100),
            "a fast completion should not wait out the 100ms poll interval: {elapsed:?}"
        );
    }
}
