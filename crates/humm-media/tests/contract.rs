//! Contract tests for `humm-media` against real `ffmpeg`/`ffprobe`
//! binaries and Buzz's test fixtures. All `#[ignore]`; run with:
//!
//! ```text
//! CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
//!     cargo test -p humm-media -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use humm_core::boundary::{AudioDecoder, MediaProber};
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;
use humm_media::{FfmpegDecoder, FfprobeProber};

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

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn cancel_kills_decode() {
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
