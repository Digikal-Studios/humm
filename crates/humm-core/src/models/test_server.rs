//! Hand-rolled, single-threaded HTTP test server for
//! [`super::download::ModelDownloader`]'s tests (`#[cfg(test)]` only —
//! Global Constraint 2 forbids a new dependency, so this is plain
//! `std::net`, not a crate like `httpmock`/`wiremock`).
//!
//! Supports exactly what the downloader needs to be exercised offline: `GET`
//! requests, optional `Range: bytes=N-` resume (answered with `206` +
//! `Content-Range`, or a forced `416`), per-path canned responses the test
//! configures up front (and can reconfigure mid-test, e.g. for the
//! placement-failure-then-redownload scenario), and a request log the test
//! asserts on to pin "a fetch was attempted" (F-043 ticket a).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// One recorded request: enough for a test to assert "a GET for this path
/// happened" (F-043 ticket a's pinning acceptance) and whether it carried a
/// `Range` header (resume vs. fresh).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRecord {
    /// HTTP method (`"GET"` for everything this server's callers send).
    pub method: String,
    /// Request-target path (e.g. `/repo/resolve/main/ggml-small.bin`).
    pub path: String,
    /// The raw `Range` header value, if the request sent one (e.g.
    /// `Some("bytes=100-")`).
    pub range: Option<String>,
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

/// A local HTTP server for exercising [`super::download::ModelDownloader`]
/// against canned, per-path responses instead of the real network.
///
/// Binds `127.0.0.1:0` (an OS-assigned free port) on construction and serves
/// requests on a background thread until dropped.
pub struct TestServer {
    addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Canned>>>,
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
        let requests: Arc<Mutex<Vec<RequestRecord>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let routes_bg = Arc::clone(&routes);
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
                        handle_connection(stream, &routes_bg, &requests_bg);
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

    /// Configures (or replaces) the canned response for `path`. Callable
    /// mid-test to change what a path serves between two downloader calls
    /// (e.g. corrupt zip, then a good one).
    pub fn set_route(&self, path: impl Into<String>, canned: Canned) {
        self.routes
            .lock()
            .expect("routes lock")
            .insert(path.into(), canned);
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
    requests: &Arc<Mutex<Vec<RequestRecord>>>,
) {
    let mut request_line = String::new();
    let mut range: Option<String> = None;
    {
        // Scoped so the shared borrow of `stream` ends before we write the
        // response through an exclusive one.
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some(value) = trimmed
                .strip_prefix("Range:")
                .or_else(|| trimmed.strip_prefix("range:"))
            {
                range = Some(value.trim().to_string());
            }
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    requests.lock().expect("requests lock").push(RequestRecord {
        method,
        path: path.clone(),
        range: range.clone(),
    });

    let canned = routes.lock().expect("routes lock").get(&path).cloned();

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
}
