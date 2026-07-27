//! L15 provenance test (M8 Task 12, spec §7.5): a local mock HTTP server
//! proving [`ApiLiveEngineSession::transcribe_live`] (`live_session.rs:273-
//! 301` — the GUI-side field-mapping wrapper around
//! [`humm_core::openai::LiveApiTranscriber::transcribe_chunk`]) threads a
//! session's `LiveOptions.language`/`.task` through to the right endpoint
//! and multipart form field. This is the specific hop M7 ledger #L15 named:
//! `humm-core`'s own `openai::live` mock-server tests already cover
//! `transcribe_chunk` itself (called directly, never through this wrapper),
//! so they cannot catch a bug in the wrapper's own field mapping (wrong
//! struct field read, a swapped argument, a hardcoded task) — only a test
//! that actually drives the wrapper, as this one does, can.
//!
//! Two chunks, one session (`ApiLiveEngineSession::transcribe_live` takes
//! its `LiveOptions` per call, so one instance covers both):
//! 1. `LiveOptions { language: Some(French), task: Transcribe }` — must hit
//!    `/audio/transcriptions` with a `language=fr` form field.
//! 2. `LiveOptions { language: None, task: Translate }` — must hit
//!    `/audio/translations` with **no** `language` field at all (Buzz's own
//!    `translations.create(**options)` never receives one either).
//!
//! **Binds TCP — humm-gui's FIRST TCP-bind test.** `humm_core::models::
//! test_server::TestServer` is `#[cfg(test)] pub(crate)` and unreachable
//! from this external test crate, and Global Constraint 2 forbids a new
//! dependency (`httpmock`/`wiremock`), so this is a hand-rolled, minimal
//! `std::net` server — the same shape as `humm-core`'s own hand-rolled
//! server, just narrower (no route table, no `Range` support: this test
//! only needs "record the request, answer 200"). Sandboxed environments
//! block `TcpListener::bind`, so this test fails there exactly like
//! `humm-core`'s 68 and `humm-cli`'s 1 existing TCP-bind tests — see this
//! task's report for the exact updated count.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use humm_core::cancel::CancelToken;
use humm_core::domain::{DEFAULT_TEMPERATURE, TaskKind};
use humm_core::language::Language;
use humm_core::openai::{OpenAiAuth, OpenAiEngineConfig};
use humm_gui::live_session::{ApiLiveEngineSession, LiveEngineSession};
use humm_whisper::LiveOptions;

/// One recorded request: method, request-target path, and the raw
/// multipart body — enough to assert the endpoint routing and (via a
/// byte-subsequence search, the same technique `humm-core`'s own
/// `openai::live` mock-server tests use for an identical multipart body)
/// the `language` form field's presence or absence.
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    body: Vec<u8>,
}

/// A hand-rolled mock server that answers exactly `expected` requests with
/// `200 {"text":"ok"}` each, recording every request first. Its accept
/// thread naturally exits once `expected` connections have been handled —
/// no shutdown flag needed, since this test knows exactly how many requests
/// it will make.
struct MockServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Recorded>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    fn start(expected: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("read local_addr");
        let requests: Arc<Mutex<Vec<Recorded>>> = Arc::new(Mutex::new(Vec::new()));
        let requests_bg = Arc::clone(&requests);

        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(expected) {
                let Ok(mut stream) = stream else { continue };
                if let Some(recorded) = read_request(&mut stream) {
                    requests_bg
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(recorded);
                }
                write_ok_json(&mut stream, br#"{"text":"ok"}"#);
            }
        });

        MockServer {
            addr,
            requests,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Reads one HTTP/1.1 request off `stream`: the request line (method +
/// path), headers (only `content-length` is consulted), then exactly that
/// many body bytes. Scoped so the shared borrow of `stream` (via
/// `try_clone`) ends before the caller writes the response through its own
/// `&mut TcpStream` — mirrors `humm-core`'s own `models::test_server`
/// `handle_connection`.
fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return None;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 && reader.read_exact(&mut body).is_err() {
        body.clear();
    }

    Some(Recorded { method, path, body })
}

fn write_ok_json(stream: &mut TcpStream, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// Byte-subsequence search — a multipart body is arbitrary binary, so this
/// can't be a `str` substring check. Ported verbatim from `humm-core`'s own
/// `openai::live` mock-server tests' identical helper.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[test]
fn session_threads_task_and_language_to_the_right_endpoint_and_form_field() {
    let server = MockServer::start(2);

    let mut session = ApiLiveEngineSession::new(OpenAiEngineConfig {
        auth: OpenAiAuth {
            base_url: Some(server.base_url()),
            api_key: "sk-test-provenance-key".to_string(),
        },
        api_model: "whisper-1".to_string(),
    });
    let cancel = CancelToken::new();
    let samples = [0.0f32; 1600];

    // Chunk 1: French transcription -> /audio/transcriptions, language=fr.
    let french = Language::parse("fr").expect("\"fr\" is a known language code");
    let transcribe_opts = LiveOptions {
        language: Some(french),
        task: TaskKind::Transcribe,
        temperature: DEFAULT_TEMPERATURE.to_vec(),
        threads: None,
    };
    let result = session.transcribe_live(&samples, &transcribe_opts, "", &cancel);
    assert_eq!(result.expect("ok"), "ok");

    // Chunk 2: no forced language, translate -> /audio/translations, no
    // language field at all.
    let translate_opts = LiveOptions {
        language: None,
        task: TaskKind::Translate,
        temperature: DEFAULT_TEMPERATURE.to_vec(),
        threads: None,
    };
    let result = session.transcribe_live(&samples, &translate_opts, "", &cancel);
    assert_eq!(result.expect("ok"), "ok");

    let requests = server.requests();
    assert_eq!(requests.len(), 2, "both chunks must reach the server");

    assert_eq!(requests[0].method, "POST");
    assert_eq!(
        requests[0].path, "/audio/transcriptions",
        "Transcribe must hit the transcriptions endpoint"
    );
    assert!(
        contains(&requests[0].body, b"name=\"language\""),
        "a forced language on Transcribe must carry a language form field"
    );
    assert!(
        contains(&requests[0].body, b"fr"),
        "the language field's value must be the French code"
    );

    assert_eq!(requests[1].method, "POST");
    assert_eq!(
        requests[1].path, "/audio/translations",
        "Translate must hit the translations endpoint"
    );
    assert!(
        !contains(&requests[1].body, b"name=\"language\""),
        "the translations endpoint must never receive a language field \
         (Buzz's own translations.create(**options) has no language kwarg)"
    );
}
