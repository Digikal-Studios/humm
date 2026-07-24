//! Hand-rolled, single-threaded HTTP test server for
//! [`super::download::ModelDownloader`]'s tests (`#[cfg(test)]` only —
//! Global Constraint 2 forbids a new dependency, so this is plain
//! `std::net`, not a crate like `httpmock`/`wiremock`). Also reused by
//! `live::export`'s `UploadClient` tests, which need `POST` bodies/headers
//! captured — see [`RequestRecord::headers`]/[`RequestRecord::body`].
//!
//! Supports exactly what its callers need to be exercised offline: `GET`
//! requests, optional `Range: bytes=N-` resume (answered with `206` +
//! `Content-Range`, or a forced `416`), per-path canned responses the test
//! configures up front (and can reconfigure mid-test, e.g. for the
//! placement-failure-then-redownload scenario), and a request log the test
//! asserts on to pin "a fetch was attempted" (F-043 ticket a) or to inspect
//! a `POST`'s headers/JSON/multipart body (`live::export`'s `UploadClient`
//! tests; `openai::validate`'s mock-server tests).
//!
//! Route matching is by path only via [`TestServer::set_route`], regardless
//! of method — a `Canned` response answers both a `GET` and a `POST` to the
//! same path identically, which is all the pre-M6 callers need (this
//! behavior is unchanged). [`TestServer::set_route_for_method`] (M6) adds a
//! `(method, path)`-scoped table checked first: each call *appends* to a
//! queue for its key rather than replacing, so a test can script a short
//! sequence of distinct responses to the very same endpoint (e.g.
//! `openai::validate`'s two same-URL requests against a custom base — a
//! `200` probe pass, then a `401` for the follow-up check) without any way
//! to interleave test code between the requests that trigger them; a
//! request pops the queue's front entry while more than one remains, then
//! repeats the sole remaining entry forever (so simple single-response
//! per-method routes behave like a steady-state canned response, same as
//! [`TestServer::set_route`]). A method+path combination with no queued
//! entry falls back to the path-only table, then to `404`.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// One recorded request: enough for a test to assert "a GET for this path
/// happened" (F-043 ticket a's pinning acceptance), whether it carried a
/// `Range` header (resume vs. fresh), and — for `POST` callers — its
/// headers and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRecord {
    /// HTTP method (e.g. `"GET"`, `"POST"`).
    pub method: String,
    /// Request-target path (e.g. `/repo/resolve/main/ggml-small.bin`).
    pub path: String,
    /// The raw `Range` header value, if the request sent one (e.g.
    /// `Some("bytes=100-")`).
    pub range: Option<String>,
    /// Every request header, lowercased-name -> raw value (e.g.
    /// `"content-type" -> "application/json"`).
    pub headers: HashMap<String, String>,
    /// The request body bytes, read per the `Content-Length` header (empty
    /// for a bodyless `GET`).
    pub body: Vec<u8>,
}

/// A canned response for one path, configured by the test before (or
/// during) a run.
#[derive(Debug, Clone)]
pub struct Canned {
    status: u16,
    body: Vec<u8>,
    force_416: bool,
}

impl Canned {
    /// A plain `200 OK` with `body`, honoring `Range: bytes=N-` with a `206`
    /// + `Content-Range` over the matching suffix of `body`.
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Canned {
            status: 200,
            body: body.into(),
            force_416: false,
        }
    }

    /// A fixed non-200 status with no body (e.g. `404`).
    pub fn status(status: u16) -> Self {
        Canned {
            status,
            body: Vec::new(),
            force_416: false,
        }
    }

    /// A fixed non-200 status with a response body — e.g. a JSON error
    /// payload (`{"error": {"message": "..."}}`) an `openai::validate` test
    /// asserts gets parsed out of a `401`/`403` response.
    pub fn status_with_body(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Canned {
            status,
            body: body.into(),
            force_416: false,
        }
    }

    /// Every request to this path — ranged or not — gets `416 Range Not
    /// Satisfiable`. Models the "resume offset past EOF" case the
    /// downloader's 416-recovery logic must handle without wedging.
    pub fn forced_416() -> Self {
        Canned {
            status: 416,
            body: Vec::new(),
            force_416: true,
        }
    }
}

/// Per-`(method, path)` response queues — see [`TestServer::set_route_for_method`].
type MethodRoutes = Arc<Mutex<HashMap<(String, String), VecDeque<Canned>>>>;

/// A local HTTP server for exercising [`super::download::ModelDownloader`]
/// against canned, per-path responses instead of the real network.
///
/// Binds `127.0.0.1:0` (an OS-assigned free port) on construction and serves
/// requests on a background thread until dropped.
pub struct TestServer {
    addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Canned>>>,
    method_routes: MethodRoutes,
    requests: Arc<Mutex<Vec<RequestRecord>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    /// Starts the server on a spawned thread and returns immediately.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("read local_addr");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");

        let routes: Arc<Mutex<HashMap<String, Canned>>> = Arc::new(Mutex::new(HashMap::new()));
        let method_routes: MethodRoutes = Arc::new(Mutex::new(HashMap::new()));
        let requests: Arc<Mutex<Vec<RequestRecord>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let routes_bg = Arc::clone(&routes);
        let method_routes_bg = Arc::clone(&method_routes);
        let requests_bg = Arc::clone(&requests);
        let stop_bg = Arc::clone(&stop);

        let handle = std::thread::spawn(move || {
            loop {
                if stop_bg.load(Ordering::SeqCst) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        handle_connection(stream, &routes_bg, &method_routes_bg, &requests_bg);
                        if stop_bg.load(Ordering::SeqCst) {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        TestServer {
            addr,
            routes,
            method_routes,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    /// The server's base URL (e.g. `http://127.0.0.1:54321`), suitable for
    /// [`super::download::ModelDownloader::with_hf_base`].
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Configures (or replaces) the canned response for `path`, answering
    /// every method. Callable mid-test to change what a path serves between
    /// two downloader calls (e.g. corrupt zip, then a good one).
    ///
    /// Checked only when no [`Self::set_route_for_method`] entry matches the
    /// incoming request's `(method, path)` — existing callers that only need
    /// one canned response per path are unaffected by the method-scoped
    /// table's existence.
    pub fn set_route(&self, path: impl Into<String>, canned: Canned) {
        self.routes
            .lock()
            .expect("routes lock")
            .insert(path.into(), canned);
    }

    /// Appends `canned` to the response queue for `method`+`path` (method
    /// matched case-insensitively). Checked before the path-only table from
    /// [`Self::set_route`].
    ///
    /// Calling this once behaves like a steady-state canned response (every
    /// matching request gets it, same as [`Self::set_route`]). Calling it
    /// more than once for the same `(method, path)` before the requests
    /// happen scripts a *sequence*: the first N-1 queued responses are each
    /// served exactly once, in order, and the last one then repeats forever
    /// — the mechanism `openai::validate`'s mock tests use to answer a
    /// custom base's two same-URL `GET /models` requests (probe, then real
    /// check) differently.
    pub fn set_route_for_method(
        &self,
        method: impl Into<String>,
        path: impl Into<String>,
        canned: Canned,
    ) {
        let key = (method.into().to_ascii_uppercase(), path.into());
        self.method_routes
            .lock()
            .expect("method_routes lock")
            .entry(key)
            .or_default()
            .push_back(canned);
    }

    /// A snapshot of every request received so far, in arrival order.
    pub fn requests(&self) -> Vec<RequestRecord> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Nudge the accept loop past its nonblocking poll wait so it notices
        // the stop flag promptly instead of on the next poll tick.
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    routes: &Arc<Mutex<HashMap<String, Canned>>>,
    method_routes: &MethodRoutes,
    requests: &Arc<Mutex<Vec<RequestRecord>>>,
) {
    let mut request_line = String::new();
    let mut range: Option<String> = None;
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut body = Vec::new();
    {
        // Scoped so the shared borrow of `stream` ends before we write the
        // response through an exclusive one.
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
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
            if let Some((name, value)) = trimmed.split_once(':') {
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if name == "range" {
                    range = Some(value.clone());
                }
                if name == "content-length" {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.insert(name, value);
            }
        }
        if content_length > 0 {
            body = vec![0u8; content_length];
            if reader.read_exact(&mut body).is_err() {
                body.clear();
            }
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    requests.lock().expect("requests lock").push(RequestRecord {
        method: method.clone(),
        path: path.clone(),
        range: range.clone(),
        headers,
        body,
    });

    let canned = take_canned(&method, &path, method_routes, routes);

    let Some(canned) = canned else {
        write_response(&mut stream, 404, &[], None);
        return;
    };

    if canned.force_416 {
        write_response(&mut stream, 416, &[], None);
        return;
    }

    if canned.status != 200 {
        write_response(&mut stream, canned.status, &canned.body, None);
        return;
    }

    match range.as_deref().and_then(parse_range_start) {
        Some(start) if (start as usize) <= canned.body.len() => {
            let start = start as usize;
            let total = canned.body.len();
            let content_range = format!("bytes {start}-{}/{total}", total.saturating_sub(1));
            write_response(
                &mut stream,
                206,
                &canned.body[start..],
                Some(&content_range),
            );
        }
        Some(_) => write_response(&mut stream, 416, &[], None),
        None => write_response(&mut stream, 200, &canned.body, None),
    }
}

/// Resolves the [`Canned`] response for one incoming request: the
/// method-scoped queue for `(method, path)` first (popping its front entry
/// while more than one remains, otherwise repeating the sole remaining
/// entry), falling back to the path-only table, then `None` (→ `404`).
fn take_canned(
    method: &str,
    path: &str,
    method_routes: &MethodRoutes,
    routes: &Arc<Mutex<HashMap<String, Canned>>>,
) -> Option<Canned> {
    let key = (method.to_ascii_uppercase(), path.to_string());
    {
        let mut method_routes = method_routes.lock().expect("method_routes lock");
        if let Some(queue) = method_routes.get_mut(&key) {
            if queue.len() > 1 {
                if let Some(canned) = queue.pop_front() {
                    return Some(canned);
                }
            } else if let Some(canned) = queue.front() {
                return Some(canned.clone());
            }
        }
    }
    routes.lock().expect("routes lock").get(path).cloned()
}

/// Parses a `Range: bytes=N-` header value's start offset.
fn parse_range_start(range: &str) -> Option<u64> {
    let bytes_part = range.strip_prefix("bytes=")?;
    let start_str = bytes_part.split('-').next()?;
    start_str.parse::<u64>().ok()
}

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8], content_range: Option<&str>) {
    let reason = reason_phrase(status);
    let mut header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(cr) = content_range {
        header.push_str(&format!("Content-Range: {cr}\r\n"));
    }
    header.push_str("\r\n");
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        _ => "Status",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_full_body_and_records_request() {
        let server = TestServer::start();
        server.set_route("/thing.bin", Canned::ok(b"hello world".to_vec()));

        let url = format!("{}/thing.bin", server.base_url());
        let response = reqwest::blocking::get(&url).expect("get");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.bytes().unwrap().as_ref(), b"hello world");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/thing.bin");
        assert_eq!(requests[0].range, None);
    }

    #[test]
    fn serves_range_as_206_with_content_range() {
        let server = TestServer::start();
        server.set_route("/thing.bin", Canned::ok(b"0123456789".to_vec()));

        let client = reqwest::blocking::Client::new();
        let url = format!("{}/thing.bin", server.base_url());
        let response = client
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=5-")
            .send()
            .expect("get");

        assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .unwrap(),
            "bytes 5-9/10"
        );
        assert_eq!(response.bytes().unwrap().as_ref(), b"56789");

        let requests = server.requests();
        assert_eq!(requests[0].range.as_deref(), Some("bytes=5-"));
    }

    #[test]
    fn forced_416_ignores_configured_body() {
        let server = TestServer::start();
        server.set_route("/thing.bin", Canned::forced_416());

        let client = reqwest::blocking::Client::new();
        let url = format!("{}/thing.bin", server.base_url());
        let response = client
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=0-")
            .send()
            .expect("get");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE
        );

        // Even a non-ranged request gets the forced 416.
        let response = reqwest::blocking::get(&url).expect("get");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE
        );
    }

    #[test]
    fn unconfigured_path_is_404() {
        let server = TestServer::start();
        let url = format!("{}/nope.bin", server.base_url());
        let response = reqwest::blocking::get(&url).expect("get");
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    }

    // -- M6: set_route_for_method + multipart body capture ----------------

    #[test]
    fn method_specific_route_overrides_path_only_route() {
        let server = TestServer::start();
        server.set_route("/models", Canned::ok(b"path-only".to_vec()));
        server.set_route_for_method("GET", "/models", Canned::ok(b"method-specific".to_vec()));

        let url = format!("{}/models", server.base_url());
        let response = reqwest::blocking::get(&url).expect("get");
        assert_eq!(response.bytes().unwrap().as_ref(), b"method-specific");
    }

    #[test]
    fn method_specific_route_is_scoped_to_its_method() {
        let server = TestServer::start();
        server.set_route_for_method("GET", "/models", Canned::ok(b"get-body".to_vec()));
        server.set_route_for_method("POST", "/models", Canned::ok(b"post-body".to_vec()));

        let url = format!("{}/models", server.base_url());
        let client = reqwest::blocking::Client::new();

        let get_response = client.get(&url).send().expect("get");
        assert_eq!(get_response.bytes().unwrap().as_ref(), b"get-body");

        let post_response = client.post(&url).body(Vec::new()).send().expect("post");
        assert_eq!(post_response.bytes().unwrap().as_ref(), b"post-body");
    }

    #[test]
    fn method_without_a_route_falls_back_to_path_only_table() {
        let server = TestServer::start();
        server.set_route("/models", Canned::ok(b"any-method".to_vec()));
        server.set_route_for_method("GET", "/models", Canned::ok(b"get-only".to_vec()));

        let url = format!("{}/models", server.base_url());
        let client = reqwest::blocking::Client::new();

        // GET matches the method-specific route...
        let get_response = client.get(&url).send().expect("get");
        assert_eq!(get_response.bytes().unwrap().as_ref(), b"get-only");

        // ...POST has no method-specific route, so it falls back to the
        // unchanged path-only table.
        let post_response = client.post(&url).body(Vec::new()).send().expect("post");
        assert_eq!(post_response.bytes().unwrap().as_ref(), b"any-method");
    }

    #[test]
    fn method_route_queue_pops_in_order_then_repeats_last() {
        let server = TestServer::start();
        server.set_route_for_method("GET", "/models", Canned::status(200));
        server.set_route_for_method("GET", "/models", Canned::status(401));

        let url = format!("{}/models", server.base_url());
        let client = reqwest::blocking::Client::new();

        let first = client.get(&url).send().expect("get 1");
        assert_eq!(first.status(), reqwest::StatusCode::OK);

        let second = client.get(&url).send().expect("get 2");
        assert_eq!(second.status(), reqwest::StatusCode::UNAUTHORIZED);

        // The queue's sole remaining entry (401) repeats forever rather
        // than falling through to 404.
        let third = client.get(&url).send().expect("get 3");
        assert_eq!(third.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    /// Binds TCP (`TestServer::start`) — blocked under the sandbox, run
    /// with it lifted.
    ///
    /// Proves the request body reader (`read_exact` over raw bytes, no
    /// text-mode line handling) never mangles a `multipart/form-data`
    /// body's binary file part — the property `openai::engine`'s later
    /// mock-server tests (mp3 upload assertions) depend on.
    #[test]
    fn multipart_post_body_bytes_arrive_unmangled() {
        let server = TestServer::start();
        server.set_route("/upload", Canned::ok(Vec::new()));

        // Binary bytes that would be mangled by any CRLF-translating or
        // line-oriented body reader: raw \r, \n, NUL, and high bytes.
        let file_bytes: Vec<u8> = vec![0x00, 0x0d, 0x0a, 0xff, 0xfe, b'-', b'-', 0x01, 0x02];
        let form = reqwest::blocking::multipart::Form::new()
            .text("model", "whisper-1")
            .part(
                "file",
                reqwest::blocking::multipart::Part::bytes(file_bytes.clone())
                    .file_name("chunk.mp3")
                    .mime_str("audio/mpeg")
                    .expect("mime"),
            );

        let client = reqwest::blocking::Client::new();
        let url = format!("{}/upload", server.base_url());
        let response = client.post(&url).multipart(form).send().expect("post");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let body = &requests[0].body;

        assert!(contains_subsequence(body, b"name=\"model\""));
        assert!(contains_subsequence(body, b"whisper-1"));
        assert!(contains_subsequence(body, b"name=\"file\""));
        assert!(contains_subsequence(body, b"filename=\"chunk.mp3\""));
        assert!(
            contains_subsequence(body, &file_bytes),
            "the exact binary file part must appear byte-for-byte, unmangled"
        );
    }

    /// `body.windows(needle.len())` byte-subsequence search — the request
    /// body is arbitrary binary, so this can't be a `str` substring check.
    fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty()
            && haystack
                .windows(needle.len())
                .any(|window| window == needle)
    }
}
