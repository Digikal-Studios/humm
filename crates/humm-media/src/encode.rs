//! ffmpeg-backed [`AudioEncoder`], implemented on [`FfmpegDecoder`] (same
//! binary config, same spawn/cancel/error-mapping helpers as `decode.rs`).
//!
//! Command shape (task-3-brief.md, binding): `ffmpeg -nostdin -threads 0 -f
//! f32le -ar 16000 -ac 1 -i - -loglevel error <dest>` — the PCM's `f32le`
//! bytes are streamed to ffmpeg's stdin from a writer thread while stdout and
//! stderr are drained on their own threads, mirroring `decode.rs`'s
//! `run_ffmpeg_pcm` (never block a pipe the child needs to keep writing to).
//! Unlike `decode`, ffmpeg here writes its output to `<dest>` rather than
//! stdout, so there's no continuous stdout stream to block a cancel-checking
//! read on; cancellation instead polls `Child::try_wait` on an interval.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use humm_core::boundary::{AudioEncoder, Pcm};
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;

use crate::decode::FfmpegDecoder;
use crate::map_spawn_error;

/// How often the cancel-poll loop checks `cancel`/`Child::try_wait` while
/// ffmpeg encodes to `dest`. Bounds cancellation latency similarly to
/// `fetch.rs`'s `CANCEL_POLL_INTERVAL`.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(50);

impl AudioEncoder for FfmpegDecoder {
    fn encode_mp3(&self, pcm: &Pcm, dest: &Path, cancel: &CancelToken) -> Result<(), MediaError> {
        if cancel.is_canceled() {
            return Err(MediaError::Canceled);
        }

        let mut cmd = Command::new(self.binary());
        cmd.args(encode_args(dest));

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                map_spawn_error(
                    "ffmpeg",
                    "HUMM_FFMPEG",
                    "install it (e.g. `brew install ffmpeg`)",
                    self.binary(),
                    err,
                )
            })?;

        let mut stdin = child.stdin.take().expect("stdin was piped at spawn");
        let mut stdout = child.stdout.take().expect("stdout was piped at spawn");
        let mut stderr = child.stderr.take().expect("stderr was piped at spawn");

        // Drain stdout on its own thread: ffmpeg is not expected to write
        // anything there (the encoded audio goes to `dest`), but draining
        // unconditionally avoids a pipe-full stall if it ever does.
        let stdout_handle = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
        });
        // Drain stderr on its own thread for the same reason `run_ffmpeg_pcm`
        // does in `decode.rs`: ffmpeg can block writing to a full stderr pipe
        // while we're still writing stdin, deadlocking both sides otherwise.
        let stderr_handle = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf);
            buf
        });

        // Writer thread streams the PCM's f32le bytes to stdin, then drops
        // it to close the pipe — ffmpeg reads `-i -` until EOF before it
        // finalizes/flushes the encoded output.
        let bytes = f32_to_bytes(pcm.samples());
        let writer_handle = thread::spawn(move || {
            let result = stdin.write_all(&bytes);
            drop(stdin);
            result
        });

        loop {
            if cancel.is_canceled() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer_handle.join();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(MediaError::Canceled);
            }

            let status = match child.try_wait() {
                Ok(Some(status)) => status,
                Ok(None) => {
                    thread::sleep(CANCEL_POLL_INTERVAL);
                    continue;
                }
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = writer_handle.join();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(MediaError::Io(err));
                }
            };

            let _ = writer_handle.join();
            let _ = stdout_handle.join();
            let stderr_bytes = stderr_handle.join().unwrap_or_default();

            if !status.success() {
                let stderr_detail = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
                // `MediaError::EncodeFailed`'s format string is tool-agnostic
                // (core has no ffmpeg-specific knowledge); the adapter names
                // the tool in `detail` itself so messages still say "ffmpeg".
                let detail = if stderr_detail.is_empty() {
                    format!("ffmpeg exited with status {status}")
                } else {
                    format!("ffmpeg exited with status {status}: {stderr_detail}")
                };
                return Err(MediaError::EncodeFailed { detail });
            }
            return Ok(());
        }
    }
}

/// Builds the ffmpeg argument list for [`AudioEncoder::encode_mp3`]:
/// `-nostdin -threads 0 -f f32le -ar 16000 -ac 1 -i - -loglevel error <dest>`
/// (task-3-brief.md, binding). Extracted so the argument order/content is
/// unit-testable without spawning a subprocess.
fn encode_args(dest: &Path) -> Vec<OsString> {
    vec![
        OsString::from("-nostdin"),
        OsString::from("-threads"),
        OsString::from("0"),
        OsString::from("-f"),
        OsString::from("f32le"),
        OsString::from("-ar"),
        OsString::from("16000"),
        OsString::from("-ac"),
        OsString::from("1"),
        OsString::from("-i"),
        OsString::from("-"),
        OsString::from("-loglevel"),
        OsString::from("error"),
        dest.as_os_str().to_os_string(),
    ]
}

/// Converts `f32` PCM samples to little-endian bytes for streaming to
/// ffmpeg's `-f f32le` stdin (the inverse of `decode.rs`'s `bytes_to_f32`).
fn f32_to_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn encode_args_matches_the_binding_command_shape() {
        let dest = Path::new("/tmp/out.mp3");
        let args = encode_args(dest);
        let expected: Vec<OsString> = [
            "-nostdin",
            "-threads",
            "0",
            "-f",
            "f32le",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-i",
            "-",
            "-loglevel",
            "error",
            "/tmp/out.mp3",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        assert_eq!(args, expected);
    }

    #[test]
    fn encode_args_carries_dest_through_unmodified() {
        let dest = Path::new("/some/weird path/dest.mp3");
        let args = encode_args(dest);
        assert_eq!(args.last(), Some(&OsString::from(dest)));
    }

    #[test]
    fn f32_to_bytes_encodes_little_endian() {
        let bytes = f32_to_bytes(&[1.0f32, -0.5, 0.0]);
        assert_eq!(bytes.len(), 12);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &(-0.5f32).to_le_bytes());
        assert_eq!(&bytes[8..12], &0.0f32.to_le_bytes());
    }

    #[test]
    fn f32_to_bytes_empty_samples_is_empty() {
        assert!(f32_to_bytes(&[]).is_empty());
    }

    #[test]
    fn pre_canceled_token_short_circuits_before_spawning() {
        // A binary that doesn't exist would normally fail with BinaryMissing;
        // a pre-canceled token must win the race and never spawn at all.
        let encoder = FfmpegDecoder::with_binary("/nonexistent/ffmpeg-binary-for-test");
        let cancel = CancelToken::new();
        cancel.cancel();
        let pcm = Pcm::new(vec![0.0; 100]);
        let dest = Path::new("/irrelevant/dest.mp3");
        let err = encoder
            .encode_mp3(&pcm, dest, &cancel)
            .expect_err("should be canceled");
        assert!(matches!(err, MediaError::Canceled));
    }
}
