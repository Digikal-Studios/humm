//! The Preferences "Test" key-validation check (partial F-100, design spec
//! §7.1) — Buzz's `ValidateOpenAIApiKeyJob.run`
//! (`buzz/widgets/preferences_dialog/general_preferences_widget.py:338-385`)
//! ported byte-exact for its request shapes, timeouts, and failure
//! messages, with one deliberate deviation: Buzz's second request only
//! catches `AuthenticationError` — any other exception (a `500`, a
//! malformed body, ...) propagates uncaught and silently kills its
//! background job (`:376-385`). That bug is **not** ported: every failure
//! path here produces [`ValidateOutcome::Invalid`] with an actionable
//! message (ledger #8).

use std::time::Duration;

use super::client::{OpenAiAuth, effective_base};

/// Buzz's exact failure text for the custom-base probe
/// (`general_preferences_widget.py:368-370`), ported byte-for-byte —
/// product-name-neutral, it never names "Buzz".
const CUSTOM_BASE_PROBE_FAILURE_MESSAGE: &str = "OpenAI API returned invalid response. Please check the API url or your key. \
     Transcription and translation may still work if the API does not support key validation.";

/// Result of [`validate_key`]: either the key checked out, or it didn't —
/// with an actionable, displayable message that never contains the key
/// itself (design spec §2's key-hygiene invariant applies here too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateOutcome {
    /// The key was accepted by the endpoint.
    Valid,
    /// The key or endpoint failed validation.
    Invalid {
        /// User-displayable failure message.
        message: String,
    },
}

/// Checks whether `auth`'s API key is valid against its configured
/// endpoint, following Buzz's `ValidateOpenAIApiKeyJob.run` exactly (design
/// spec §7.1):
///
/// 1. If a custom base URL is configured (`auth.base_url` is `Some` and
///    non-empty): a raw `GET {base}/models` probe with a bearer header and
///    a 5 s timeout. A non-`200` response fails with
///    [`ValidateOutcome::Invalid`] carrying Buzz's exact
///    `CUSTOM_BASE_PROBE_FAILURE_MESSAGE`; a transport error fails with
///    that error's own message. Either failure short-circuits — step 2
///    never runs.
/// 2. Then (only if step 1 passed, or there was no custom base): `GET
///    {effective_base}/models` with a bearer header and a 15 s timeout. A
///    `2xx` response is [`ValidateOutcome::Valid`]. A `401`/`403` whose
///    body parses as JSON with an `error.message` string fails with that
///    message (`AuthenticationError.body["message"]`'s shape,
///    `general_preferences_widget.py:385`). Any other non-`2xx`, or a
///    transport error, fails with a status-derived or the transport
///    error's own message.
///
/// Unlike Buzz, every failure path here reaches [`ValidateOutcome::Invalid`]
/// rather than an uncaught exception — see the module doc comment.
///
/// This function has no cancel affordance and does not run through
/// [`super::client::cancelable`] — its two probes already have short, fixed
/// timeouts (5 s/15 s) and Buzz's Test button has no cancel button either;
/// the GUI runs this call on its own background thread (the `UpdateNotice`
/// channel pattern), same as Buzz's `QRunnable`.
pub fn validate_key(auth: &OpenAiAuth) -> ValidateOutcome {
    let has_custom_base = matches!(auth.base_url.as_deref(), Some(base) if !base.is_empty());

    if has_custom_base {
        // `has_custom_base` guarantees `base_url` is `Some` with non-empty
        // content, so this default is never actually used.
        let base = auth.base_url.as_deref().unwrap_or_default();
        if let Some(outcome) = probe_custom_base(base, &auth.api_key) {
            return outcome;
        }
    }

    check_models_endpoint(&effective_base(&auth.base_url), &auth.api_key)
}

/// Step 1: the raw custom-base probe (`:354-374`). `Some(Invalid)`
/// short-circuits [`validate_key`]; `None` means the probe passed and step
/// 2 should run.
fn probe_custom_base(base: &str, api_key: &str) -> Option<ValidateOutcome> {
    let url = models_url(base);
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(&url)
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .timeout(Duration::from_secs(5))
        .send();

    match response {
        Ok(response) if response.status() == reqwest::StatusCode::OK => None,
        Ok(_) => Some(ValidateOutcome::Invalid {
            message: CUSTOM_BASE_PROBE_FAILURE_MESSAGE.to_string(),
        }),
        Err(e) => Some(ValidateOutcome::Invalid {
            message: e.to_string(),
        }),
    }
}

/// Step 2: the real `models.list`-equivalent check (`:376-385`).
fn check_models_endpoint(base: &str, api_key: &str) -> ValidateOutcome {
    let url = models_url(base);
    let client = reqwest::blocking::Client::new();
    let response = match client
        .get(&url)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(15))
        .send()
    {
        Ok(response) => response,
        Err(e) => {
            return ValidateOutcome::Invalid {
                message: e.to_string(),
            };
        }
    };

    let status = response.status();
    if status.is_success() {
        return ValidateOutcome::Valid;
    }

    let is_auth_failure =
        status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN;
    if is_auth_failure && let Some(message) = error_message_from_body(response) {
        return ValidateOutcome::Invalid { message };
    }

    ValidateOutcome::Invalid {
        message: format!("OpenAI API returned HTTP {status}"),
    }
}

/// Extracts `error.message` from a JSON error body, if present and a
/// string (`AuthenticationError.body["message"]`'s shape,
/// `general_preferences_widget.py:385`).
fn error_message_from_body(response: reqwest::blocking::Response) -> Option<String> {
    let body: serde_json::Value = response.json().ok()?;
    body.get("error")?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

/// Joins `base` and `models`, ensuring exactly one `/` between them
/// regardless of whether `base` already ends with one (Buzz `:356-364`'s
/// "append `/` if missing, then concatenate `models`" — equivalent to
/// trim-then-join since a trailing slash is idempotent either way).
fn models_url(base: &str) -> String {
    format!("{}/models", base.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::test_server::{Canned, TestServer};

    fn auth_for(server: &TestServer, api_key: &str) -> OpenAiAuth {
        OpenAiAuth {
            base_url: Some(server.base_url()),
            api_key: api_key.to_string(),
        }
    }

    // -- non-TCP: transport failure against an unreachable loopback port --
    // (a refused outgoing connect, not a bound listener -- unaffected by
    // the sandbox's TCP-*bind* restriction, same style as
    // `live::export::tests::upload_failure_is_logged_and_never_panics`.)

    #[test]
    fn validate_key_transport_error_surfaces_error_string_not_panic() {
        let auth = OpenAiAuth {
            base_url: Some("http://127.0.0.1:1".to_string()),
            api_key: "sk-test".to_string(),
        };

        let outcome = validate_key(&auth);

        match outcome {
            ValidateOutcome::Invalid { message } => assert!(!message.is_empty()),
            ValidateOutcome::Valid => panic!("expected Invalid against an unreachable host"),
        }
    }

    #[test]
    fn models_url_joins_regardless_of_trailing_slash() {
        assert_eq!(
            models_url("http://localhost:1234"),
            "http://localhost:1234/models"
        );
        assert_eq!(
            models_url("http://localhost:1234/"),
            "http://localhost:1234/models"
        );
    }

    // -- mock-server tests (TCP) -------------------------------------------
    //
    // Bind TCP (`TestServer::start`) -- blocked under the sandbox, run with
    // it lifted (see this task's report for the full pre-existing set).

    #[test]
    fn validate_key_success_against_mock() {
        let server = TestServer::start();
        server.set_route_for_method("GET", "/models", Canned::ok(b"{\"data\":[]}".to_vec()));

        let outcome = validate_key(&auth_for(&server, "sk-test"));

        assert_eq!(outcome, ValidateOutcome::Valid);

        let requests = server.requests();
        assert_eq!(
            requests.len(),
            2,
            "a custom base probes GET /models twice (raw probe, then the real check): {requests:?}"
        );
        for request in &requests {
            assert_eq!(request.method, "GET");
            assert_eq!(request.path, "/models");
            assert_eq!(
                request.headers.get("authorization").map(String::as_str),
                Some("Bearer sk-test")
            );
        }
    }

    #[test]
    fn validate_key_custom_base_probe_failure_yields_buzz_message() {
        let server = TestServer::start();
        server.set_route_for_method("GET", "/models", Canned::status(500));

        let outcome = validate_key(&auth_for(&server, "sk-test"));

        assert_eq!(
            outcome,
            ValidateOutcome::Invalid {
                message: "OpenAI API returned invalid response. Please check the API url or \
                          your key. Transcription and translation may still work if the API \
                          does not support key validation."
                    .to_string(),
            }
        );

        // The probe failure short-circuits -- the second (real) GET never
        // happens.
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn validate_key_auth_error_surfaces_error_message_json() {
        let server = TestServer::start();
        // First GET (the raw probe) passes...
        server.set_route_for_method("GET", "/models", Canned::ok(Vec::new()));
        // ...second GET (the real check, same URL) gets a 401 with a JSON
        // error body.
        server.set_route_for_method(
            "GET",
            "/models",
            Canned::status_with_body(
                401,
                br#"{"error":{"message":"Incorrect API key provided"}}"#.to_vec(),
            ),
        );

        let outcome = validate_key(&auth_for(&server, "sk-bad"));

        assert_eq!(
            outcome,
            ValidateOutcome::Invalid {
                message: "Incorrect API key provided".to_string(),
            }
        );
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn validate_key_non_auth_failure_yields_status_derived_message() {
        let server = TestServer::start();
        server.set_route_for_method("GET", "/models", Canned::ok(Vec::new()));
        server.set_route_for_method("GET", "/models", Canned::status(500));

        let outcome = validate_key(&auth_for(&server, "sk-test"));

        match outcome {
            ValidateOutcome::Invalid { message } => {
                assert!(message.contains("500"), "message was: {message}");
            }
            ValidateOutcome::Valid => panic!("expected Invalid for a 500 response"),
        }
    }

    #[test]
    fn validate_key_401_without_json_body_falls_back_to_status_message() {
        let server = TestServer::start();
        server.set_route_for_method("GET", "/models", Canned::ok(Vec::new()));
        server.set_route_for_method("GET", "/models", Canned::status(401));

        let outcome = validate_key(&auth_for(&server, "sk-test"));

        match outcome {
            ValidateOutcome::Invalid { message } => {
                assert!(message.contains("401"), "message was: {message}");
            }
            ValidateOutcome::Valid => panic!("expected Invalid for a 401 response"),
        }
    }
}
