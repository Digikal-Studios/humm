//! Shared bounded stderr-tail capture for this crate's subprocess adapters.
//!
//! Every adapter here (`demucs.rs`, `fetch.rs`, `decode.rs`, `encode.rs`)
//! spawns a child with `stderr(Stdio::piped())` and must drain it on a
//! background thread — a subprocess can otherwise block forever writing to
//! a full pipe while the caller is still busy reading stdout or writing
//! stdin (the same deadlock hazard each of those modules' own doc comments
//! describe). Before M9 (D5b-1/D5b-5), `fetch.rs` and `demucs.rs` each kept
//! a private, near-identical bounded-tail copy, while `decode.rs` and
//! `encode.rs` read stderr with an unbounded `read_to_end` — fine for a
//! well-behaved ffmpeg, but an unbounded memory sink for anything unusually
//! chatty (or hostile) on stderr. This module unifies all four sites on one
//! implementation.

use std::io::Read;
use std::process::ChildStderr;
use std::thread::JoinHandle;

/// Bytes of trailing stderr retained for an actionable diagnostic, bounding
/// memory for a subprocess that writes more than a reasonable amount to
/// stderr (chatty progress output, a runaway loop, or worse).
const STDERR_TAIL_CAP: usize = 2 * 1024;

/// Spawns a background thread that reads `stderr` to completion — so the
/// child process never blocks writing to a full pipe — and returns a handle
/// yielding its tail: at most [`STDERR_TAIL_CAP`] bytes, keeping the *last*
/// bytes written (the most likely to contain the actual failure, e.g. an
/// exit-causing error line, past any earlier chatty progress output).
/// Trimmed either way; prefixed with `…` when the captured output had to be
/// truncated (i.e. the child wrote more than the cap).
pub(crate) fn spawn_stderr_tail(mut stderr: ChildStderr) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut truncated = false;
        loop {
            match stderr.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.len() > STDERR_TAIL_CAP {
                        truncated = true;
                        let excess = buf.len() - STDERR_TAIL_CAP;
                        buf.drain(0..excess);
                    }
                }
            }
        }
        let tail = String::from_utf8_lossy(&buf).trim().to_string();
        if truncated {
            format!("…{tail}")
        } else {
            tail
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn write_stub(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write stub script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub script");
        path
    }

    #[cfg(unix)]
    fn spawn_stub_stderr(script: &std::path::Path) -> (std::process::Child, ChildStderr) {
        use std::process::{Command, Stdio};
        let mut child = Command::new(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn stub script");
        let stderr = child.stderr.take().expect("stderr was piped at spawn");
        (child, stderr)
    }

    #[test]
    #[cfg(unix)]
    fn stderr_tail_caps_oversized_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 64 KiB of filler followed by a short, unique marker at the very
        // end — proves the *last* bytes survive truncation, not an
        // arbitrary prefix.
        let stub = write_stub(
            dir.path(),
            "big-stderr.sh",
            "yes x | head -c 65536 >&2\nprintf 'END-MARKER-42' >&2",
        );
        let (mut child, stderr) = spawn_stub_stderr(&stub);

        let tail = spawn_stderr_tail(stderr)
            .join()
            .expect("thread should not panic");
        let _ = child.wait();

        assert!(
            tail.starts_with('…'),
            "truncated output should be `…`-prefixed, got: {tail:?}"
        );
        assert!(
            tail.len() <= STDERR_TAIL_CAP + '…'.len_utf8(),
            "tail should be capped near {STDERR_TAIL_CAP} bytes, got {} bytes",
            tail.len()
        );
        assert!(
            tail.contains("END-MARKER-42"),
            "tail should contain the final bytes written, got: {tail:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn stderr_tail_passes_small_output_through() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stub = write_stub(
            dir.path(),
            "small-stderr.sh",
            "printf 'boom: small error' >&2",
        );
        let (mut child, stderr) = spawn_stub_stderr(&stub);

        let tail = spawn_stderr_tail(stderr)
            .join()
            .expect("thread should not panic");
        let _ = child.wait();

        assert_eq!(tail, "boom: small error");
        assert!(
            !tail.starts_with('…'),
            "small output should never be truncated: {tail:?}"
        );
    }
}
