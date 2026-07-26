//! ffmpeg-backed [`AudioDecoder`] (ADR-003).
//!
//! Command shape ported from `buzz/whisper_audio.py:39-50`. Deliberate
//! deviation (ADR-003): humm pipes `f32le` straight from ffmpeg's stdout
//! instead of Buzz's `s16le` + manual `/32768.0` conversion — same result,
//! one fewer conversion.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use humm_core::boundary::{AudioDecoder, Pcm, PlaybackPcm};
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;

use crate::{binary_from_env, map_spawn_error};

/// Bytes read from ffmpeg's stdout per chunk while streaming decoded PCM.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Decodes media files to mono 16 kHz `f32` PCM via an `ffmpeg` subprocess.
pub struct FfmpegDecoder {
    binary: PathBuf,
}

impl FfmpegDecoder {
    /// Uses `HUMM_FFMPEG` if set, else `ffmpeg` on `PATH`.
    pub fn new() -> Self {
        Self::with_binary(binary_from_env("HUMM_FFMPEG", "ffmpeg"))
    }

    /// Uses an explicit binary path/name, bypassing `HUMM_FFMPEG`.
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        FfmpegDecoder {
            binary: path.into(),
        }
    }

    /// The resolved ffmpeg binary path/name, for other adapters in this
    /// crate (`encode.rs`'s `AudioEncoder` impl) that reuse the same binary
    /// resolution rather than duplicating `new`/`with_binary`.
    pub(crate) fn binary(&self) -> &Path {
        &self.binary
    }
}

impl Default for FfmpegDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioDecoder for FfmpegDecoder {
    fn decode(&self, path: &Path, cancel: &CancelToken) -> Result<Pcm, MediaError> {
        // `whisper_audio.py:39-50`, f32le per ADR-003.
        let mut cmd = Command::new(&self.binary);
        cmd.arg("-nostdin")
            .args(["-threads", "0"])
            .arg("-i")
            .arg(path)
            .args(["-f", "f32le"])
            .args(["-ac", "1"])
            .args(["-acodec", "pcm_f32le"])
            .args(["-ar", "16000"])
            .args(["-loglevel", "error"])
            .arg("-");

        let raw = run_ffmpeg_pcm(cmd, &self.binary, cancel)?;
        Ok(Pcm::new(bytes_to_f32(&raw)))
    }

    fn decode_playback(
        &self,
        path: &Path,
        cancel: &CancelToken,
    ) -> Result<PlaybackPcm, MediaError> {
        // `sounddevice_player.py:28-35`: playback is decoded to 44.1 kHz
        // stereo (distinct from `decode`'s mono 16 kHz for the engine).
        let mut cmd = Command::new(&self.binary);
        cmd.arg("-nostdin")
            .arg("-i")
            .arg(path)
            .args(["-f", "f32le"])
            .args(["-ac", "2"])
            .args(["-ar", "44100"])
            .args(["-loglevel", "error"])
            .arg("-");

        let raw = run_ffmpeg_pcm(cmd, &self.binary, cancel)?;
        Ok(PlaybackPcm::new(bytes_to_f32(&raw), 2, 44_100))
    }
}

/// Spawns `cmd` (already configured with every ffmpeg argument except
/// stdio), streams its stdout to completion checking `cancel`
/// cooperatively, and returns the raw decoded bytes. Shared by
/// [`AudioDecoder::decode`] and [`AudioDecoder::decode_playback`], which
/// differ only in ffmpeg's argument list.
fn run_ffmpeg_pcm(
    mut cmd: Command,
    binary: &Path,
    cancel: &CancelToken,
) -> Result<Vec<u8>, MediaError> {
    if cancel.is_canceled() {
        return Err(MediaError::Canceled);
    }

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            map_spawn_error(
                "ffmpeg",
                "HUMM_FFMPEG",
                "install it (e.g. `brew install ffmpeg`)",
                binary,
                err,
            )
        })?;

    let mut stdout = child.stdout.take().expect("stdout was piped at spawn");
    let mut stderr = child.stderr.take().expect("stderr was piped at spawn");

    // Drain stderr on a separate thread: ffmpeg can block writing to a
    // full stderr pipe while we're still reading stdout, deadlocking
    // both sides if we read them sequentially instead.
    let stderr_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let mut raw = Vec::new();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        if cancel.is_canceled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_handle.join();
            return Err(MediaError::Canceled);
        }
        let n = match stdout.read(&mut chunk) {
            Ok(n) => n,
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stderr_handle.join();
                return Err(MediaError::Io(err));
            }
        };
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
    }

    let status = child.wait().map_err(MediaError::Io)?;
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
        return Err(MediaError::DecodeFailed { detail });
    }

    Ok(raw)
}

/// Converts little-endian `f32` PCM bytes to samples. A trailing partial
/// sample (byte length not a multiple of 4) is discarded with a warning.
fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    let remainder = bytes.len() % 4;
    if remainder != 0 {
        log::warn!(
            "decoded PCM byte length {} is not a multiple of 4; discarding trailing {remainder} byte(s)",
            bytes.len()
        );
    }
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn four_bytes_decode_to_one_sample() {
        assert_eq!(bytes_to_f32(&[0, 0, 128, 63]), vec![1.0f32]);
    }

    #[test]
    fn five_bytes_decode_to_one_sample_and_discard_tail() {
        assert_eq!(bytes_to_f32(&[0, 0, 128, 63, 99]), vec![1.0f32]);
    }

    #[test]
    fn empty_bytes_decode_to_no_samples() {
        assert!(bytes_to_f32(&[]).is_empty());
    }

    #[test]
    fn multiple_samples_round_trip_in_order() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&(-0.5f32).to_le_bytes());
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        assert_eq!(bytes_to_f32(&bytes), vec![1.0f32, -0.5f32, 0.0f32]);
    }

    #[test]
    fn with_binary_overrides_the_resolved_path() {
        let decoder = FfmpegDecoder::with_binary("custom-ffmpeg");
        assert_eq!(decoder.binary, PathBuf::from("custom-ffmpeg"));
    }

    #[test]
    fn pre_canceled_token_short_circuits_before_spawning() {
        // A binary that doesn't exist would normally fail with BinaryMissing;
        // a pre-canceled token must win the race and never spawn at all.
        let decoder = FfmpegDecoder::with_binary("/nonexistent/ffmpeg-binary-for-test");
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = decoder
            .decode(Path::new("/irrelevant"), &cancel)
            .expect_err("should be canceled");
        assert!(matches!(err, MediaError::Canceled));
    }

    #[test]
    fn decode_playback_pre_canceled_token_short_circuits_before_spawning() {
        let decoder = FfmpegDecoder::with_binary("/nonexistent/ffmpeg-binary-for-test");
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = decoder
            .decode_playback(Path::new("/irrelevant"), &cancel)
            .expect_err("should be canceled");
        assert!(matches!(err, MediaError::Canceled));
    }
}
