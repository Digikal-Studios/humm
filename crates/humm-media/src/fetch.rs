//! `yt-dlp`-backed [`MediaFetcher`] (ADR-016, F-022 URL-import half).
//!
//! Ported from the URL-download half of Buzz's `FileTranscriber`
//! (`buzz/transcriber/file_transcriber.py:75-153`, `_download_from_url`):
//! `bestaudio/best` format selection, an optional cookie file for
//! authenticated downloads, and a downloaded/total-bytes progress hook
//! (`on_download_progress`, lines 172-174). Buzz drives yt-dlp through its
//! Python API; humm instead drives the `yt-dlp` CLI as a subprocess emitting
//! only machine-readable output (Standing Policy in `CLAUDE.md`), matching
//! the `FfmpegDecoder`/`FfprobeProber` adapter pattern in this crate.
//!
//! # Deviations from Buzz
//!
//! - **`--no-playlist` (ADR-016 note).** Buzz's `_download_from_url` never
//!   disables playlist expansion, but its output template
//!   (`temp_output_path`, a single fixed path with no `%(playlist_index)s`)
//!   means a playlist URL would have every entry in the playlist silently
//!   overwrite the same file — an accident of Buzz's implementation, not an
//!   intended feature, since Buzz's UI only ever offers a single-URL import.
//!   humm passes `--no-playlist` so a playlist URL downloads only the
//!   single requested item instead of silently discarding all but the last
//!   entry.
//! - **Download name.** Buzz writes `source.%(ext)s` and keeps `info["title"]`
//!   as `display_name` (`file_transcriber.py`). humm does the same: the
//!   output template is `source.%(ext)s`, and one `--print` record carries
//!   the filepath and the raw title. Export sanitizes that title; this
//!   adapter does not.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use humm_core::boundary::{FetchProgress, FetchedMedia, MediaFetcher};
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;

use crate::{binary_from_env, map_spawn_error};

/// Prefix that marks a yt-dlp stdout line as a progress update (from this
/// module's `--progress-template`) rather than the final result record.
const PROGRESS_PREFIX: &str = "humm-dl:";

/// One stdout record: `humm-result:` plus a JSON object with `path` and
/// `title`. A single `--print`, not a second print that could be taken as
/// the path.
const RESULT_PREFIX: &str = "humm-result:";

/// `--print` template. `j` JSON-encodes each field so a title can contain
/// quotes, slashes, or newlines without becoming a second line.
const RESULT_TEMPLATE: &str = "after_move:humm-result:{\"path\":%(filepath)j,\"title\":%(title)j}";

/// How long the main thread waits for the next stdout line before
/// re-checking `cancel`. Bounds cancellation latency even when yt-dlp is
/// stalled mid-download (a throttled/hung server), since the blocking
/// stdout read happens on a separate reader thread, not here.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Bytes of trailing stderr retained for [`MediaError::FetchFailed`]
/// diagnostics, bounding memory for a chatty subprocess.
const STDERR_TAIL_CAP: usize = 2 * 1024;

/// Downloads remote media via a `yt-dlp` subprocess (ADR-016).
pub struct YtDlpFetcher {
    binary: PathBuf,
    cookie_file: Option<PathBuf>,
}

impl YtDlpFetcher {
    /// Uses `HUMM_YTDLP` if set, else `yt-dlp` on `PATH`. `cookie_file`, if
    /// `Some`, is passed to yt-dlp via `--cookies` for authenticated
    /// downloads (the caller resolves `HUMM_DOWNLOAD_COOKIEFILE`/settings
    /// and passes the result in — this constructor does not read that env
    /// var itself).
    pub fn new(cookie_file: Option<PathBuf>) -> Self {
        Self::with_binary(binary_from_env("HUMM_YTDLP", "yt-dlp"), cookie_file)
    }

    /// Uses an explicit binary path/name, bypassing `HUMM_YTDLP`. Test seam.
    pub fn with_binary(binary: impl Into<PathBuf>, cookie_file: Option<PathBuf>) -> Self {
        YtDlpFetcher {
            binary: binary.into(),
            cookie_file,
        }
    }
}

impl MediaFetcher for YtDlpFetcher {
    fn fetch(
        &self,
        url: &url::Url,
        dest_dir: &Path,
        progress: &mut dyn FnMut(FetchProgress),
        cancel: &CancelToken,
    ) -> Result<FetchedMedia, MediaError> {
        if cancel.is_canceled() {
            return Err(MediaError::Canceled);
        }

        let output_template = dest_dir.join("source.%(ext)s");
        let progress_template = format!(
            "download:{PROGRESS_PREFIX}%(progress.downloaded_bytes)s:%(progress.total_bytes)s:%(progress.total_bytes_estimate)s"
        );

        let (mut command, binary) = crate::prepare_command(&self.binary);
        command
            .arg(url.as_str())
            .args(["-f", "bestaudio/best"])
            .arg("--no-playlist")
            .arg("-o")
            .arg(&output_template)
            .args(["--newline", "--progress"])
            .arg("--progress-template")
            .arg(&progress_template)
            .args(["--print", RESULT_TEMPLATE])
            .arg("--no-simulate");
        if let Some(cookie_file) = &self.cookie_file {
            command.arg("--cookies").arg(cookie_file);
        }
        // `prepare_command` spawns into a new process group on unix — see
        // `demucs.rs`'s module doc / `crate::kill_process_group`'s doc for
        // why: yt-dlp can invoke `ffmpeg` itself as an external
        // downloader/post-processor (HLS streams, format merges, fixups),
        // and a single-PID kill would leave that descendant running, holding
        // this function's reader threads' pipes open until it exits naturally
        // on its own. On Windows the same helper hides the console window;
        // cancellation uses `taskkill /T`.

        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                map_spawn_error(
                    "yt-dlp",
                    "HUMM_YTDLP",
                    crate::install_hint("yt-dlp"),
                    &binary,
                    err,
                )
            })?;
        // Register after spawn. The lock is not held across `spawn`.
        let _group = humm_core::abort::track_process(child.id());

        let stdout = child.stdout.take().expect("stdout was piped at spawn");
        let stderr = child.stderr.take().expect("stderr was piped at spawn");

        // Reader thread feeding stdout lines over a channel: the main loop
        // below bounds its wait with `recv_timeout` so a stalled child
        // doesn't block cancellation, even though the read on this thread
        // may itself block indefinitely.
        let (tx, rx) = mpsc::channel::<String>();
        let stdout_handle = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        // stderr is drained on its own thread throughout, exactly like
        // `FfmpegDecoder::decode`: yt-dlp can block writing to a full
        // stderr pipe while we're still reading stdout, deadlocking both
        // sides if we read them sequentially instead.
        let stderr_handle = thread::spawn(move || read_stderr_tail(stderr));

        let mut fetched: Option<FetchedMedia> = None;
        loop {
            if cancel.is_canceled() {
                // Group-SIGKILL (unix) + direct `Child::kill()` fallback —
                // see the `spawn_own_process_group` call above. Keeps
                // yt-dlp's existing immediate-SIGKILL cancellation
                // semantics (no grace-period ladder, unlike `DemucsIsolator`'s
                // SIGTERM-then-SIGKILL) — just aimed at the whole group now.
                crate::kill_process_group(&mut child);
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(MediaError::Canceled);
            }
            match rx.recv_timeout(CANCEL_POLL_INTERVAL) {
                Ok(line) => {
                    if let Some(update) = parse_progress_line(&line) {
                        progress(update);
                    } else if let Some(record) = parse_fetch_record(&line) {
                        fetched = Some(record);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let _ = stdout_handle.join();
        let status = child.wait().map_err(MediaError::Io)?;
        let stderr_tail = stderr_handle.join().unwrap_or_default();

        match (status.success(), fetched) {
            (true, Some(fetched)) => Ok(fetched),
            (true, None) => Err(MediaError::FetchFailed {
                detail: if stderr_tail.is_empty() {
                    "yt-dlp exited successfully but printed no output path".to_string()
                } else {
                    stderr_tail
                },
            }),
            (false, _) => Err(MediaError::FetchFailed {
                detail: stderr_tail,
            }),
        }
    }
}

/// Parses a yt-dlp progress-template stdout line
/// (`humm-dl:<downloaded_bytes>:<total_bytes>:<total_bytes_estimate>`, per
/// this module's `--progress-template`) into a [`FetchProgress`]. Returns
/// `None` for any line that isn't `humm-dl:`-prefixed, or whose fields don't
/// parse (missing fields, non-numeric `downloaded_bytes`).
///
/// Buzz's equivalent hook (`file_transcriber.py:172-174`) divides
/// `downloaded_bytes` by `total_bytes` directly, which would panic if
/// `total_bytes` were unavailable; humm instead keeps `downloaded`/`total`
/// as separate fields so the caller decides how to handle an unknown total.
/// yt-dlp reports unavailable numeric fields as the literal string `"NA"`;
/// any field that is `NA`, empty, or otherwise fails to parse as `u64`
/// becomes `None`. `total` prefers `total_bytes`, falling back to
/// `total_bytes_estimate` (yt-dlp's estimate when the server didn't
/// advertise `Content-Length` up front) when `total_bytes` doesn't parse.
fn parse_progress_line(line: &str) -> Option<FetchProgress> {
    let fields = line.strip_prefix(PROGRESS_PREFIX)?;
    let mut parts = fields.split(':');
    let downloaded = parse_u64(parts.next()?)?;
    let total_bytes = parts.next()?;
    let total_bytes_estimate = parts.next()?;
    let total = parse_u64(total_bytes).or_else(|| parse_u64(total_bytes_estimate));
    Some(FetchProgress { downloaded, total })
}

/// Parses a single progress-template numeric field, treating `"NA"`, empty,
/// and any other non-numeric text alike as `None`.
fn parse_u64(field: &str) -> Option<u64> {
    field.trim().parse::<u64>().ok()
}

/// Parses the single `humm-result:` JSON record. Returns `None` for progress
/// lines and anything that is not that record, so a title cannot be taken
/// as the download path.
fn parse_fetch_record(line: &str) -> Option<FetchedMedia> {
    let json = line.trim().strip_prefix(RESULT_PREFIX)?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let path = value.get("path")?.as_str()?.trim();
    if path.is_empty() {
        return None;
    }
    let title = match value.get("title") {
        Some(serde_json::Value::String(title)) if !title.is_empty() => Some(title.clone()),
        _ => None,
    };
    Some(FetchedMedia {
        path: PathBuf::from(path),
        title,
    })
}

/// Reads `stderr` to completion in the background (so the child never
/// blocks writing to a full pipe), retaining only the last
/// `STDERR_TAIL_CAP` bytes for a `FetchFailed` diagnostic.
fn read_stderr_tail(mut stderr: impl Read) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stderr.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > STDERR_TAIL_CAP {
                    let excess = buf.len() - STDERR_TAIL_CAP;
                    buf.drain(0..excess);
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_valid_progress_line_with_known_total() {
        let update = parse_progress_line("humm-dl:1024:342770:NA").expect("should parse");
        assert_eq!(
            update,
            FetchProgress {
                downloaded: 1024,
                total: Some(342770),
            }
        );
    }

    #[test]
    fn na_totals_become_none() {
        let update = parse_progress_line("humm-dl:1024:NA:NA").expect("should parse");
        assert_eq!(
            update,
            FetchProgress {
                downloaded: 1024,
                total: None,
            }
        );
    }

    #[test]
    fn falls_back_to_estimate_when_total_bytes_is_na() {
        let update = parse_progress_line("humm-dl:1024:NA:500000").expect("should parse");
        assert_eq!(
            update,
            FetchProgress {
                downloaded: 1024,
                total: Some(500_000),
            }
        );
    }

    #[test]
    fn prefers_total_bytes_over_estimate_when_both_present() {
        let update = parse_progress_line("humm-dl:1024:342770:999999").expect("should parse");
        assert_eq!(update.total, Some(342_770));
    }

    #[test]
    fn garbage_line_without_prefix_is_ignored() {
        assert_eq!(parse_progress_line("/tmp/dest/audio.mp3"), None);
    }

    #[test]
    fn garbage_line_with_prefix_but_unparseable_downloaded_is_ignored() {
        assert_eq!(parse_progress_line("humm-dl:not-a-number:NA:NA"), None);
    }

    #[test]
    fn garbage_line_with_prefix_but_missing_fields_is_ignored() {
        assert_eq!(parse_progress_line("humm-dl:1024"), None);
    }

    #[test]
    fn empty_line_is_ignored() {
        assert_eq!(parse_progress_line(""), None);
    }

    #[test]
    fn fetch_record_keeps_the_raw_title_and_one_path() {
        let cases = [
            "AC/DC",
            "../x",
            "..",
            "C:/x",
            "CON",
            "title with \"quotes\"",
        ];
        for title in cases {
            let line = format!(
                "humm-result:{}",
                serde_json::json!({"path": "/tmp/media/source.mp3", "title": title})
            );
            let record = parse_fetch_record(&line).expect(title);
            assert_eq!(record.path, PathBuf::from("/tmp/media/source.mp3"));
            assert_eq!(record.title.as_deref(), Some(title));
        }
        assert!(parse_fetch_record("humm-dl:1:2:3").is_none());
        assert!(parse_fetch_record("/tmp/media/source.mp3").is_none());
        let missing =
            parse_fetch_record("humm-result:{\"path\":\"/tmp/media/source.mp3\",\"title\":null}")
                .expect("path without a title");
        assert_eq!(missing.title, None);
    }

    #[test]
    #[cfg(unix)]
    fn fetch_reads_one_result_record_and_downloads_as_source() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let stub = dir.path().join("yt-dlp-stub.sh");
        std::fs::write(
            &stub,
            r#"#!/bin/sh
o_next=0
for arg in "$@"; do
  if [ "$o_next" = 1 ]; then
    dest=$(dirname "$arg")
    printf '%s\n' "$@" > "$dest/args"
    printf 'humm-result:{"path":"%s/source.mp3","title":"AC/DC"}\n' "$dest"
    exit 0
  fi
  if [ "$arg" = "-o" ]; then
    o_next=1
  else
    o_next=0
  fi
done
exit 1
"#,
        )
        .expect("write stub");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let dest = tempfile::tempdir().expect("dest");
        let fetcher = YtDlpFetcher::with_binary(&stub, None);
        let url = url::Url::parse("https://example.com/video").expect("url");
        let cancel = CancelToken::new();
        let fetched = fetcher
            .fetch(&url, dest.path(), &mut |_| {}, &cancel)
            .expect("fetch");
        assert_eq!(fetched.path, dest.path().join("source.mp3"));
        assert_eq!(fetched.title.as_deref(), Some("AC/DC"));

        let args = std::fs::read_to_string(dest.path().join("args")).expect("args");
        assert!(args.contains("source.%(ext)s"), "{args}");
        assert!(!args.contains("%(title)s.%(ext)s"), "{args}");
        assert_eq!(args.matches("--print").count(), 1, "{args}");
        assert!(args.contains("--no-playlist"), "{args}");
    }

    #[test]
    fn with_binary_overrides_the_resolved_path() {
        let fetcher = YtDlpFetcher::with_binary("custom-yt-dlp", None);
        assert_eq!(fetcher.binary, PathBuf::from("custom-yt-dlp"));
        assert_eq!(fetcher.cookie_file, None);
    }

    #[test]
    fn with_binary_carries_cookie_file_through() {
        let cookie = PathBuf::from("/home/user/cookies.txt");
        let fetcher = YtDlpFetcher::with_binary("yt-dlp", Some(cookie.clone()));
        assert_eq!(fetcher.cookie_file, Some(cookie));
    }

    #[test]
    fn missing_binary_reports_actionable_hint() {
        let fetcher = YtDlpFetcher::with_binary("/nonexistent/yt-dlp-binary-for-test", None);
        let cancel = CancelToken::new();
        let dest = std::env::temp_dir();
        let url = url::Url::parse("https://example.com/video").expect("valid url");
        let mut calls = 0u32;
        let err = fetcher
            .fetch(&url, &dest, &mut |_| calls += 1, &cancel)
            .expect_err("missing binary should fail");

        match err {
            MediaError::BinaryMissing { hint, .. } => {
                assert!(hint.contains("HUMM_YTDLP"), "hint was: {hint}");
                assert!(hint.contains("yt-dlp not found"), "hint was: {hint}");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
        assert_eq!(
            calls, 0,
            "no progress callbacks for a binary that never spawned"
        );
    }

    #[test]
    fn pre_canceled_token_short_circuits_before_spawning() {
        let fetcher = YtDlpFetcher::with_binary("/nonexistent/yt-dlp-binary-for-test", None);
        let cancel = CancelToken::new();
        cancel.cancel();
        let dest = std::env::temp_dir();
        let url = url::Url::parse("https://example.com/video").expect("valid url");
        let err = fetcher
            .fetch(&url, &dest, &mut |_| {}, &cancel)
            .expect_err("should be canceled");
        assert!(matches!(err, MediaError::Canceled));
    }

    /// Regression test for the same bug class found and fixed in
    /// `DemucsIsolator` (`demucs.rs`'s module doc, task-5 review): a stub
    /// `yt-dlp` whose last statement is `sleep 30` forks a real grandchild
    /// process for it on this platform (not exec-tail-call optimized) — a
    /// single-PID kill would leave that grandchild running, holding
    /// `fetch`'s stdout reader thread's pipe open and blocking `join()`
    /// until it exits naturally 30s later. Asserts cancellation still acks
    /// promptly now that the child is killed by process group.
    #[test]
    #[cfg(unix)]
    fn cancel_kills_grandchild_process_not_just_direct_child() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let dir = tempfile::tempdir().expect("tempdir");
        let stub = dir.path().join("yt-dlp-stub.sh");
        std::fs::write(&stub, "#!/bin/sh\nsleep 30\n").expect("write stub script");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub script");

        let dest_dir = tempfile::tempdir().expect("tempdir");
        let cancel = CancelToken::new();
        let cancel_from_main = cancel.clone();
        let url = url::Url::parse("https://example.com/video").expect("valid url");

        let start = Instant::now();
        let handle = thread::spawn(move || {
            let fetcher = YtDlpFetcher::with_binary(&stub, None);
            let mut noop = |_: FetchProgress| {};
            fetcher.fetch(&url, dest_dir.path(), &mut noop, &cancel)
        });

        thread::sleep(Duration::from_millis(200));
        cancel_from_main.cancel();

        let result = handle.join().expect("fetch thread should not panic");
        let elapsed = start.elapsed();

        assert!(
            matches!(result, Err(MediaError::Canceled)),
            "expected Canceled, got {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "expected cancellation to ack within ~2s, took {elapsed:?}"
        );
    }
}
