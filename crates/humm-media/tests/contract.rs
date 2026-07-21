//! Contract tests for `humm-media` against real `ffmpeg`/`ffprobe`
//! binaries and Buzz's test fixtures. All `#[ignore]`; run with:
//!
//! ```text
//! CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
//!     cargo test -p humm-media -- --ignored
//! ```

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use humm_core::boundary::{AudioDecoder, FetchProgress, MediaFetcher, MediaProber};
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;
use humm_media::{FfmpegDecoder, FfprobeProber, YtDlpFetcher, probe_binaries};

/// Root of Buzz's test fixtures, honoring `HUMM_BUZZ_TESTDATA` (Global
/// Constraint 4).
fn testdata() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

/// Writes a canonical 44-byte-header 16-bit PCM mono WAV containing a sine
/// wave, for decode tests that don't depend on Buzz fixtures.
fn write_sine_wav(path: &Path, freq_hz: f64, seconds: f64, sample_rate: u32, amplitude: f64) {
    let num_samples = (seconds * sample_rate as f64).round() as u32;
    let mut samples = Vec::with_capacity(num_samples as usize * 2);
    for i in 0..num_samples {
        let t = f64::from(i) / f64::from(sample_rate);
        let value = amplitude * (2.0 * std::f64::consts::PI * freq_hz * t).sin();
        let sample_i16 = (value * f64::from(i16::MAX)).round() as i16;
        samples.extend_from_slice(&sample_i16.to_le_bytes());
    }

    let data_len = samples.len() as u32;
    let byte_rate = sample_rate * 2; // mono, 16-bit
    let block_align: u16 = 2;
    let bits_per_sample: u16 = 16;

    let mut buf = Vec::with_capacity(44 + samples.len());
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    buf.extend_from_slice(&samples);

    std::fs::write(path, buf).expect("write sine wav");
}

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn decode_french_mp3_matches_probed_duration() {
    let path = testdata().join("whisper-french.mp3");
    let prober = FfprobeProber::new();
    let info = prober.probe(&path).expect("probe should succeed");
    let duration = info.duration.expect("duration should be known");

    let decoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    let pcm = decoder
        .decode(&path, &cancel)
        .expect("decode should succeed");

    let expected_samples = duration.0 * 16; // 16 samples/ms at 16 kHz
    let actual = pcm.len() as i64;
    let tolerance = (expected_samples as f64 * 0.02).ceil() as i64;
    assert!(
        (actual - expected_samples).abs() <= tolerance,
        "expected ~{expected_samples} samples (±2%), got {actual}"
    );
    assert!(
        pcm.samples().iter().all(|s| s.is_finite()),
        "decoded samples must all be finite"
    );
}

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn decode_synthesized_sine_wav() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sine.wav");
    write_sine_wav(&path, 440.0, 1.0, 16_000, 0.5);

    let decoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    let pcm = decoder
        .decode(&path, &cancel)
        .expect("decode should succeed");

    assert!(
        (15_900..=16_100).contains(&pcm.len()),
        "expected ~16000 samples, got {}",
        pcm.len()
    );

    let n = pcm.len();
    let mut sum_sq_err = 0.0f64;
    for (i, sample) in pcm.samples().iter().enumerate() {
        let t = i as f64 / 16_000.0;
        let expected = 0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin();
        let err = f64::from(*sample) - expected;
        sum_sq_err += err * err;
    }
    let rms_error = (sum_sq_err / n as f64).sqrt();
    assert!(rms_error < 0.02, "RMS error too high: {rms_error}");
}

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn probe_mp3() {
    let path = testdata().join("whisper-french.mp3");
    let prober = FfprobeProber::new();
    let info = prober.probe(&path).expect("probe should succeed");

    assert!(info.has_audio, "expected an audio stream");
    assert!(!info.has_video, "mp3 should have no video stream");
    assert!(
        info.duration.expect("duration should be known").0 > 0,
        "duration should be positive"
    );
}

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn decode_corrupt_input_reports_stderr() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.mp3");
    std::fs::write(&path, b"this is not media").expect("write bad file");

    let decoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    let err = decoder
        .decode(&path, &cancel)
        .expect_err("corrupt input should fail to decode");

    match err {
        MediaError::DecodeFailed { detail } => {
            assert!(!detail.is_empty(), "stderr detail should not be empty");
        }
        other => panic!("expected DecodeFailed, got {other:?}"),
    }
}

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn missing_binary_actionable() {
    let decoder = FfmpegDecoder::with_binary("/nonexistent/ffmpeg");
    let cancel = CancelToken::new();
    let err = decoder
        .decode(Path::new("/irrelevant"), &cancel)
        .expect_err("missing binary should fail");

    match err {
        MediaError::BinaryMissing { hint, .. } => {
            assert!(hint.contains("HUMM_FFMPEG"), "hint was: {hint}");
        }
        other => panic!("expected BinaryMissing, got {other:?}"),
    }
}

/// Genuinely exercises the mid-decode kill()/wait()/stderr-join path:
/// `audio-long.mp3` decodes in a few seconds of wall time (ffmpeg is far
/// faster than real-time for a 120 s mp3, but not instant), so canceling
/// from another thread after a short delay lands while `decode()` is still
/// blocked in the stdout read loop, not before ffmpeg is even spawned.
#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn cancel_kills_decode_mid_read() {
    let path = testdata().join("audio-long.mp3");
    let cancel = CancelToken::new();
    let cancel_from_main = cancel.clone();

    let start = Instant::now();
    let handle = std::thread::spawn(move || {
        let decoder = FfmpegDecoder::new();
        decoder.decode(&path, &cancel)
    });

    std::thread::sleep(Duration::from_millis(200));
    cancel_from_main.cancel();

    let result = handle.join().expect("decode thread should not panic");
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(MediaError::Canceled)),
        "expected Canceled, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "cancellation took too long: {elapsed:?}"
    );
}

/// The pre-canceled fast path (superset of the brief): a token already
/// canceled before `decode()` is called short-circuits before ffmpeg is
/// even spawned.
#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn cancel_kills_decode_pre_canceled_fast_path() {
    let path = testdata().join("audio-long.mp3");
    let decoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    cancel.cancel();

    let start = Instant::now();
    let err = decoder
        .decode(&path, &cancel)
        .expect_err("pre-canceled decode should be canceled");
    let elapsed = start.elapsed();

    assert!(matches!(err, MediaError::Canceled));
    assert!(
        elapsed < Duration::from_secs(2),
        "cancellation took too long: {elapsed:?}"
    );
}

// --- yt-dlp `MediaFetcher` contract tests (F-022, ADR-016) ---
//
// These need no network: a hand-rolled `std::net::TcpListener` HTTP server
// below serves Buzz's local test fixtures on `127.0.0.1`, and yt-dlp's
// generic extractor fetches directly from it (no site-specific extractor,
// no real internet access). They do need the real `yt-dlp` binary installed.

/// A tiny GET-only HTTP/1.1 file server for contract tests: binds an
/// ephemeral localhost port and serves one fixed byte buffer for every
/// request it receives, ignoring the request path/method (good enough
/// for "some URL yt-dlp's generic extractor can fetch" — yt-dlp performs a
/// webpage-sniff GET followed by the real download GET, and this server
/// answers both identically). Can optionally throttle the body into small,
/// delayed chunks to keep a download "in flight" long enough for the
/// cancellation test to interrupt it mid-transfer.
struct TestFileServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TestFileServer {
    /// Serves `body` in full for every request.
    fn serve(body: Vec<u8>) -> Self {
        Self::spawn(body, None)
    }

    /// Serves `body` in `chunk_size`-byte pieces, sleeping `delay` between
    /// each piece, so a large body takes a while to fully transmit.
    fn serve_throttled(body: Vec<u8>, chunk_size: usize, delay: Duration) -> Self {
        Self::spawn(body, Some((chunk_size, delay)))
    }

    fn spawn(body: Vec<u8>, throttle: Option<(usize, Duration)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        let addr = listener.local_addr().expect("read local_addr");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_accept = Arc::clone(&stop);
        let body = Arc::new(body);

        let handle = thread::spawn(move || {
            while !stop_accept.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Accepted sockets can inherit the listener's
                        // non-blocking flag on some platforms (observed on
                        // macOS); `handle_connection` uses blocking
                        // read/write calls, so force blocking mode back on
                        // or a large `write_all` can fail early with
                        // `WouldBlock` once the send buffer fills,
                        // truncating the response.
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        let body = Arc::clone(&body);
                        thread::spawn(move || handle_connection(stream, &body, throttle));
                    }
                    Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        TestFileServer {
            addr,
            stop,
            handle: Some(handle),
        }
    }

    /// A full `http://127.0.0.1:<port><path>` URL string for this server.
    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

impl Drop for TestFileServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Handles one HTTP connection: discards the request line/headers (method
/// and path are irrelevant — there's only one file to serve), then writes a
/// `200 OK` response with `Content-Length` and the body, optionally
/// throttled.
fn handle_connection(stream: TcpStream, body: &[u8], throttle: Option<(usize, Duration)>) {
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return, // connection closed before headers ended
            Ok(_) if line == "\r\n" || line == "\n" => break,
            Ok(_) => continue,
            Err(_) => return,
        }
    }

    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if writer.write_all(header.as_bytes()).is_err() {
        return;
    }

    match throttle {
        None => {
            let _ = writer.write_all(body);
        }
        Some((chunk_size, delay)) => {
            for chunk in body.chunks(chunk_size.max(1)) {
                if writer.write_all(chunk).is_err() {
                    return;
                }
                thread::sleep(delay);
            }
        }
    }
    let _ = writer.flush();
}

#[test]
#[ignore = "requires yt-dlp; serves Buzz testdata over a local HTTP server, no network"]
fn fetch_downloads_file_and_reports_progress() {
    let bytes = std::fs::read(testdata().join("whisper-french.mp3")).expect("read fixture");
    let server = TestFileServer::serve(bytes);
    let url = url::Url::parse(&server.url("/whisper-french.mp3")).expect("valid url");

    let dest_dir = tempfile::tempdir().expect("tempdir");
    let fetcher = YtDlpFetcher::new(None);
    let cancel = CancelToken::new();
    let mut progress_calls = 0u32;

    let path = fetcher
        .fetch(
            &url,
            dest_dir.path(),
            &mut |_update: FetchProgress| progress_calls += 1,
            &cancel,
        )
        .expect("fetch should succeed");

    assert!(path.exists(), "downloaded file should exist at {path:?}");
    assert!(
        path.starts_with(dest_dir.path()),
        "downloaded file should live under dest_dir, got {path:?}"
    );
    assert!(
        progress_calls >= 1,
        "expected at least one progress callback"
    );
}

/// Genuinely exercises the mid-download kill()/wait()/reader-thread-join
/// path: the server throttles `audio-long.mp3` into small, delayed chunks
/// so the download is still in flight when cancellation trips from another
/// thread after a short delay.
#[test]
#[ignore = "requires yt-dlp; serves Buzz testdata over a local HTTP server, no network"]
fn fetch_cancel_kills_download_mid_transfer() {
    let bytes = std::fs::read(testdata().join("audio-long.mp3")).expect("read fixture");
    let server = TestFileServer::serve_throttled(bytes, 4096, Duration::from_millis(20));
    let url = url::Url::parse(&server.url("/audio-long.mp3")).expect("valid url");

    let dest_dir = tempfile::tempdir().expect("tempdir");
    let cancel = CancelToken::new();
    let cancel_from_main = cancel.clone();

    let start = Instant::now();
    let handle = thread::spawn(move || {
        let fetcher = YtDlpFetcher::new(None);
        let mut noop = |_: FetchProgress| {};
        fetcher.fetch(&url, dest_dir.path(), &mut noop, &cancel)
    });

    thread::sleep(Duration::from_millis(300));
    cancel_from_main.cancel();

    let result = handle.join().expect("fetch thread should not panic");
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(MediaError::Canceled)),
        "expected Canceled, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "cancellation took too long: {elapsed:?}"
    );
}

#[test]
#[ignore = "requires yt-dlp"]
fn yt_dlp_capability_probe_reports_version_when_present() {
    let caps = probe_binaries();
    assert!(
        caps.yt_dlp.is_some(),
        "expected yt-dlp version to be detected via `yt-dlp --version`"
    );
}
