//! Contract tests for `humm-media` against real `ffmpeg`/`ffprobe`/`yt-dlp`
//! binaries and Buzz's test fixtures. All `#[ignore]`; run with:
//!
//! ```text
//! CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
//!     cargo test -p humm-media -- --ignored
//! ```

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use humm_core::boundary::{
    AudioDecoder, AudioEncoder, FetchProgress, MediaFetcher, MediaProber, Pcm, SAMPLE_RATE,
    SpeechIsolator, VideoStreamer,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::Millis;
use humm_core::error::MediaError;
use humm_media::{
    DemucsIsolator, FfmpegDecoder, FfmpegVideoStreamer, FfprobeProber, YtDlpFetcher, probe_binaries,
};

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

/// Synthesizes a mono 16 kHz `f32` sine [`Pcm`] directly in memory, for the
/// `AudioEncoder` contract tests below — these don't need a Buzz fixture or
/// an on-disk audio file, just decoded PCM shaped the way the transcription
/// pipeline would hand it to `encode_mp3`.
fn synth_sine_pcm(freq_hz: f64, seconds: f64, amplitude: f64) -> Pcm {
    let num_samples = (seconds * f64::from(SAMPLE_RATE)).round() as usize;
    let samples: Vec<f32> = (0..num_samples)
        .map(|i| {
            let t = i as f64 / f64::from(SAMPLE_RATE);
            (amplitude * (2.0 * std::f64::consts::PI * freq_hz * t).sin()) as f32
        })
        .collect();
    Pcm::new(samples)
}

/// Resolves the ffmpeg binary the same way `FfmpegDecoder::new`/
/// `FfmpegVideoStreamer::new` do (`HUMM_FFMPEG` override, else `ffmpeg` on
/// `PATH`), for generating fixtures directly via a raw subprocess.
fn ffmpeg_binary() -> PathBuf {
    std::env::var_os("HUMM_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// Generates a short H.264 + AAC mp4 fixture entirely from ffmpeg's `lavfi`
/// test sources — `testsrc2` (default 320×240) for video, `sine` for audio
/// — so dimensions/fps/duration are exactly known without shipping a binary
/// fixture. Used by the video/probe contract tests below.
fn write_video_fixture(dir: &Path, duration_secs: u32, fps: u32) -> PathBuf {
    write_video_fixture_at_rate(dir, duration_secs, &fps.to_string())
}

/// As [`write_video_fixture`], but `rate` is an arbitrary ffmpeg rate
/// expression rather than a plain integer — `testsrc2`'s `rate` option is an
/// `AVRational`, so a fraction such as `"30000/1001"` (NTSC's 29.97 fps)
/// works exactly like `-r 30000/1001` would on the command line, and stamps
/// the encoded stream's `r_frame_rate` with that exact fraction (verified
/// against a real ffmpeg/ffprobe round-trip: `ffprobe -show_entries
/// stream=r_frame_rate` on a `rate=30000/1001` fixture reports
/// `r_frame_rate=30000/1001`, not a rounded decimal). Used by the
/// fractional-fps spot-check (M4 hardware debt): 29.97 is the most common
/// non-integer fps in real-world footage, and `FfprobeProber::parse_frame_rate`
/// (`probe.rs`) is only unit-tested against a synthetic mocked
/// `r_frame_rate`; this closes the loop against a genuine ffmpeg-encoded
/// fixture.
fn write_video_fixture_at_rate(dir: &Path, duration_secs: u32, rate: &str) -> PathBuf {
    let path = dir.join("fixture.mp4");
    let status = Command::new(ffmpeg_binary())
        .args(["-y", "-f", "lavfi"])
        .arg("-i")
        .arg(format!("testsrc2=duration={duration_secs}:rate={rate}"))
        .args(["-f", "lavfi"])
        .arg("-i")
        .arg(format!("sine=duration={duration_secs}"))
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .args(["-c:a", "aac", "-shortest"])
        .args(["-loglevel", "error"])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn ffmpeg to generate video fixture");
    assert!(status.success(), "ffmpeg video fixture generation failed");
    path
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
fn decode_mono_at_48k_matches_requested_rate_and_duration() {
    // `decode_mono_at` generalizes `decode` beyond the fixed 16 kHz
    // `AudioDecoder::decode` uses — `humm-dfn` (F-112) calls it at 48 kHz,
    // DeepFilterNet3's own expected rate. Source is a 22.05 kHz mono sine,
    // deliberately not 48 kHz, so a pass just means ffmpeg actually
    // resampled rather than passing the source through unchanged.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sine.wav");
    write_sine_wav(&path, 440.0, 1.0, 22_050, 0.5);

    let decoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    let pcm = decoder
        .decode_mono_at(&path, 48_000, &cancel)
        .expect("decode_mono_at should succeed");

    assert!(
        (47_500..=48_500).contains(&pcm.len()),
        "expected ~48000 samples at 48kHz for 1s of audio, got {}",
        pcm.len()
    );
    assert!(
        pcm.samples().iter().all(|s| s.is_finite()),
        "decoded samples must all be finite"
    );
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

/// Genuinely exercises the mid-decode kill()/wait()/stderr-join path.
/// Racing a fixed sleep against ffmpeg's full-speed decode is flaky (fast
/// hardware finishes the 120 s mp3 in ~100 ms), so a wrapper script injects
/// `-re` to throttle ffmpeg's input reads to realtime — the same
/// throttle-the-producer approach as `fetch_cancel_kills_download_mid_transfer`.
/// Canceling 200 ms in is then guaranteed to land while `decode()` is still
/// in its stdout read loop, on any hardware.
#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
#[cfg(unix)]
fn cancel_kills_decode_mid_read() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = dir.path().join("ffmpeg-realtime.sh");
    // `exec` keeps ffmpeg in the wrapper's own PID so `child.kill()` reaches
    // it directly; `${HUMM_FFMPEG:-ffmpeg}` matches `FfmpegDecoder::new()`'s
    // binary resolution.
    std::fs::write(
        &wrapper,
        "#!/bin/sh\nexec \"${HUMM_FFMPEG:-ffmpeg}\" -re \"$@\"\n",
    )
    .expect("write wrapper script");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("chmod wrapper script");

    let path = testdata().join("audio-long.mp3");
    let cancel = CancelToken::new();
    let cancel_from_main = cancel.clone();

    let start = Instant::now();
    let handle = std::thread::spawn(move || {
        let decoder = FfmpegDecoder::with_binary(&wrapper);
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

// --- `decode_playback`/video `FrameStream` contract tests (F-083) ---------

#[test]
#[ignore = "requires ffmpeg"]
fn decode_playback_stereo_44100_matches_duration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sine.wav");
    // Source is mono at a non-44.1kHz rate on purpose: decode_playback must
    // resample/upmix to stereo 44.1 kHz regardless of the source format
    // (`sounddevice_player.py:28-35`).
    write_sine_wav(&path, 440.0, 2.0, 22_050, 0.5);

    let decoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    let pcm = decoder
        .decode_playback(&path, &cancel)
        .expect("decode_playback should succeed");

    assert_eq!(pcm.channels, 2);
    assert_eq!(pcm.sample_rate, 44_100);
    assert_eq!(
        pcm.samples().len(),
        pcm.frames() * 2,
        "interleaved stereo: samples().len() == frames() * channels"
    );
    assert!(
        pcm.samples().iter().all(|s| s.is_finite()),
        "decoded samples must all be finite"
    );

    let expected_frames = (2.0 * 44_100.0) as i64;
    let actual_frames = pcm.frames() as i64;
    let tolerance = (expected_frames as f64 * 0.01).ceil() as i64;
    assert!(
        (actual_frames - expected_frames).abs() <= tolerance,
        "expected ~{expected_frames} frames (±1%), got {actual_frames}"
    );

    let expected_ms = 2000i64;
    let actual_ms = pcm.duration().0;
    assert!(
        (actual_ms - expected_ms).abs() <= 20,
        "expected ~{expected_ms}ms duration, got {actual_ms}ms"
    );
}

#[test]
#[ignore = "requires ffmpeg"]
fn video_stream_yields_monotonic_frames_with_dimensions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_video_fixture(dir.path(), 2, 25);

    let prober = FfprobeProber::new();
    let info = prober.probe(&path).expect("probe should succeed");
    let video_info = info.video.expect("expected a video stream");

    let streamer = FfmpegVideoStreamer::new();
    let mut stream = streamer
        .open(&path, Millis(0), &video_info)
        .expect("open should succeed");

    let frame_ms = (1000.0 / video_info.fps).round() as i64;
    let mut frames = Vec::new();
    let mut position = Millis(0);
    let deadline = Instant::now() + Duration::from_secs(20);
    while frames.len() < 50 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for frames; got {} of 50",
            frames.len()
        );
        match stream.frame_for(position) {
            Some(frame) => {
                position = Millis(position.0 + frame_ms);
                frames.push(frame);
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }

    for frame in &frames {
        assert_eq!(frame.width, video_info.width);
        assert_eq!(frame.height, video_info.height);
        assert_eq!(
            frame.rgba.len(),
            (video_info.width * video_info.height * 4) as usize,
            "rgba buffer should be width*height*4 bytes"
        );
    }

    let timestamps: Vec<i64> = frames.iter().map(|f| f.ts.0).collect();
    assert_eq!(timestamps[0], 0, "first frame's ts should be 0");
    for pair in timestamps.windows(2) {
        assert!(
            pair[1] > pair[0],
            "timestamps should be strictly ascending: {timestamps:?}"
        );
    }
}

#[test]
#[ignore = "requires ffmpeg"]
fn video_seek_restarts_at_position() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_video_fixture(dir.path(), 2, 25);

    let prober = FfprobeProber::new();
    let info = prober.probe(&path).expect("probe should succeed");
    let video_info = info.video.expect("expected a video stream");
    let frame_ms = 1000.0 / video_info.fps;

    let streamer = FfmpegVideoStreamer::new();
    let mut stream = streamer
        .open(&path, Millis(0), &video_info)
        .expect("open should succeed");

    stream.seek(Millis(1000)).expect("seek should succeed");

    let deadline = Instant::now() + Duration::from_secs(20);
    let frame = loop {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a frame after seek"
        );
        if let Some(frame) = stream.frame_for(Millis(1000)) {
            break frame;
        }
        thread::sleep(Duration::from_millis(5));
    };

    assert!(
        (frame.ts.0 as f64) >= 1000.0 - frame_ms,
        "expected first frame after seek(1000) to have ts >= {}, got {}",
        1000.0 - frame_ms,
        frame.ts.0
    );
}

#[test]
#[ignore = "requires ffmpeg"]
fn probe_reports_video_info() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_video_fixture(dir.path(), 2, 25);

    let prober = FfprobeProber::new();
    let info = prober.probe(&path).expect("probe should succeed");

    assert!(info.has_video, "expected a video stream");
    assert!(info.has_audio, "expected an audio stream");
    let video = info.video.expect("expected video stream info");
    // testsrc2's default size is 320x240.
    assert_eq!(video.width, 320);
    assert_eq!(video.height, 240);
    assert!(
        (video.fps - 25.0).abs() < 1e-6,
        "expected 25 fps, got {}",
        video.fps
    );
    assert!(
        info.duration.expect("duration should be known").0 > 0,
        "duration should be positive"
    );
}

/// Fractional-fps spot-check (M4 hardware debt, carried to M5): every prior
/// contract test in this file generates its fixture at an integer fps
/// (`25`), so `FfprobeProber::parse_frame_rate`'s `"n/d"` division only ever
/// got exercised against a *mocked* fractional `r_frame_rate` in
/// `probe.rs`'s unit tests, never a genuine ffmpeg-encoded stream. This
/// fixture is generated at NTSC's 30000/1001 (≈29.97 fps) — the most common
/// non-integer fps in real-world footage — and proves two things end to end
/// against real binaries: (1) ffprobe reports `r_frame_rate` as the exact
/// fraction `"30000/1001"` for this fixture and `FfprobeProber` divides it
/// into the expected ≈29.970_03 fps float, and (2) `FfmpegVideoStreamer`
/// still streams frames with strictly monotonic timestamps at that rate
/// (the same assertion `video_stream_yields_monotonic_frames_with_dimensions`
/// makes at 25 fps) — i.e. the fractional rate doesn't confuse the decode
/// pacing (`frame_for`'s frame-boundary math) the way it might if some path
/// silently rounded fps to an integer.
#[test]
#[ignore = "requires ffmpeg"]
fn probe_and_stream_fractional_29_97_fps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_video_fixture_at_rate(dir.path(), 2, "30000/1001");

    let prober = FfprobeProber::new();
    let info = prober.probe(&path).expect("probe should succeed");
    let video_info = info.video.expect("expected a video stream");

    // 30000/1001 = 29.970029970029969... — assert the parsed float matches
    // that division exactly, not some rounded stand-in like 29.97 or 30.0.
    let expected_fps = 30_000.0 / 1001.0;
    assert!(
        (video_info.fps - expected_fps).abs() < 1e-9,
        "expected ~{expected_fps} fps (30000/1001), got {}",
        video_info.fps
    );
    assert!(
        (video_info.fps - 30.0).abs() > 0.01,
        "29.97 fps must not have been rounded up to a flat 30 fps"
    );

    let streamer = FfmpegVideoStreamer::new();
    let mut stream = streamer
        .open(&path, Millis(0), &video_info)
        .expect("open should succeed");

    let frame_ms = (1000.0 / video_info.fps).round() as i64;
    let mut frames = Vec::new();
    let mut position = Millis(0);
    let deadline = Instant::now() + Duration::from_secs(20);
    while frames.len() < 50 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for frames; got {} of 50",
            frames.len()
        );
        match stream.frame_for(position) {
            Some(frame) => {
                position = Millis(position.0 + frame_ms);
                frames.push(frame);
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
    }

    let timestamps: Vec<i64> = frames.iter().map(|f| f.ts.0).collect();
    assert_eq!(timestamps[0], 0, "first frame's ts should be 0");
    for pair in timestamps.windows(2) {
        assert!(
            pair[1] > pair[0],
            "timestamps should be strictly ascending at 29.97 fps: {timestamps:?}"
        );
    }
}

// --- `AudioEncoder` contract tests (F-041, M6 design spec §3.1) -----------

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn encode_mp3_duration_matches_pcm() {
    let pcm = synth_sine_pcm(440.0, 3.0, 0.5);
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("encoded.mp3");

    let encoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    encoder
        .encode_mp3(&pcm, &dest, &cancel)
        .expect("encode should succeed");

    let prober = FfprobeProber::new();
    let info = prober.probe(&dest).expect("probe should succeed");
    let duration = info.duration.expect("duration should be known");

    let expected_ms = 3000i64;
    assert!(
        (duration.0 - expected_ms).abs() <= 100,
        "expected ~{expected_ms}ms duration (±100ms), got {}ms",
        duration.0
    );
}

#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn encode_mp3_round_trips_through_decode() {
    let pcm = synth_sine_pcm(440.0, 3.0, 0.5);
    let input_len = pcm.len();
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("encoded.mp3");

    let encoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    encoder
        .encode_mp3(&pcm, &dest, &cancel)
        .expect("encode should succeed");

    let decoder = FfmpegDecoder::new();
    let decoded = decoder
        .decode(&dest, &cancel)
        .expect("decode should succeed");

    let tolerance = (input_len as f64 * 0.05).ceil() as i64;
    let diff = (decoded.len() as i64 - input_len as i64).abs();
    assert!(
        diff <= tolerance,
        "expected sample count within ±5% of {input_len}, got {} (diff {diff}, tolerance {tolerance})",
        decoded.len()
    );
}

/// The pre-canceled fast path: a token already canceled before
/// `encode_mp3()` is called short-circuits before ffmpeg is even spawned
/// (mirrors `cancel_kills_decode_pre_canceled_fast_path`).
#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
fn encode_mp3_precanceled_returns_immediately() {
    let pcm = synth_sine_pcm(440.0, 3.0, 0.5);
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("encoded.mp3");

    let encoder = FfmpegDecoder::new();
    let cancel = CancelToken::new();
    cancel.cancel();

    let start = Instant::now();
    let err = encoder
        .encode_mp3(&pcm, &dest, &cancel)
        .expect_err("pre-canceled encode should be canceled");
    let elapsed = start.elapsed();

    assert!(matches!(err, MediaError::Canceled));
    assert!(
        elapsed < Duration::from_secs(2),
        "cancellation took too long: {elapsed:?}"
    );
    assert!(
        !dest.exists() || std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) == 0,
        "dest should be absent or empty after a pre-canceled encode, found {dest:?}"
    );
}

/// Genuinely exercises the mid-encode `kill()`/`wait()`/thread-join path
/// (mirrors `cancel_kills_decode_mid_read`'s and
/// `fetch_cancel_kills_download_mid_transfer`'s throttle-the-producer
/// technique): a wrapper script injects `-re` before the real ffmpeg's args,
/// so it consumes the piped PCM at realtime rate instead of racing through
/// the whole ~60 s clip in a fraction of a second. Canceling 300 ms in is
/// then guaranteed to land while `encode_mp3()` is still in its
/// `Child::try_wait` poll loop, on any hardware.
#[test]
#[ignore = "requires ffmpeg + Buzz testdata"]
#[cfg(unix)]
fn encode_mp3_cancel_kills_mid_encode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let wrapper = dir.path().join("ffmpeg-realtime.sh");
    // `exec` keeps ffmpeg in the wrapper's own PID so `child.kill()` reaches
    // it directly; `${HUMM_FFMPEG:-ffmpeg}` matches `FfmpegDecoder::new()`'s
    // binary resolution.
    std::fs::write(
        &wrapper,
        "#!/bin/sh\nexec \"${HUMM_FFMPEG:-ffmpeg}\" -re \"$@\"\n",
    )
    .expect("write wrapper script");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("chmod wrapper script");

    let pcm = synth_sine_pcm(440.0, 60.0, 0.5);
    let dest = dir.path().join("encoded.mp3");
    let cancel = CancelToken::new();
    let cancel_from_main = cancel.clone();

    let start = Instant::now();
    let handle = thread::spawn(move || {
        let encoder = FfmpegDecoder::with_binary(&wrapper);
        encoder.encode_mp3(&pcm, &dest, &cancel)
    });

    thread::sleep(Duration::from_millis(300));
    cancel_from_main.cancel();

    let result = handle.join().expect("encode thread should not panic");
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(MediaError::Canceled)),
        "expected Canceled, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
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
        .expect("fetch should succeed")
        .path;

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

// -- DemucsIsolator (F-027, ADR-023) -----------------------------------------
//
// Gated on `HUMM_DEMUCS_CONTRACT=1` in *addition* to `#[ignore]`: unlike
// ffmpeg/yt-dlp (assumed present in any dev environment able to build this
// workspace), a real `demucs` install is a heavy, ML-specific optional
// dependency (PyTorch, a first-run ~80MB model download) that most machines
// running a plain `cargo test -p humm-media -- --ignored` won't have — this
// extra gate keeps that default `--ignored` run from failing here.
//
// Provisioning (`validation/m8-spikes/README.md`, "Spike C"): bare
// `uvx demucs` fails on the M8 spike's reference machine because demucs
// 4.1.0 pulls in a `sphn` (Rust/PyO3) dependency needing a Rust target that
// machine's `rustup` doesn't have — pin the classic pre-`sphn` release
// instead, exactly as the spike did:
//
// ```text
// uvx --from demucs==4.0.1 --with "numpy<2" demucs \
//     --two-stems=vocals -n htdemucs --mp3 -o <scratch> \
//     --filename "{stem}.{ext}" <input>
// ```
//
// Point `HUMM_DEMUCS` at a tiny wrapper script that `exec`s that pinned
// `uvx` invocation (or an equivalent `pipx install demucs==4.0.1`/
// `pip install "demucs==4.0.1" "numpy<2"` install with plain `demucs` on
// `PATH`, in which case `HUMM_DEMUCS` can be left unset), then:
//
// ```text
// CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
//     HUMM_DEMUCS_CONTRACT=1 HUMM_DEMUCS=/path/to/demucs-wrapper.sh \
//     cargo test -p humm-media -- --ignored real_demucs
// ```
//
// Budget ~60s wall for the one-time cold-cache model download (Spike C); a
// warm-cache run took ~7s for the 8.517s `whisper-french.mp3` fixture.
#[test]
#[ignore = "requires a real demucs install (set HUMM_DEMUCS_CONTRACT=1); see doc comment for provisioning"]
fn real_demucs_isolates_vocals_from_whisper_french() {
    // Skip (not panic/fail) when the extra gate isn't set — this test is
    // still `#[ignore]`d like every other contract test in this file, but
    // CLAUDE.md's documented `cargo test -p humm-media -- --ignored` gate
    // (needing only ffmpeg/yt-dlp) must stay green even when someone runs
    // exactly that command without also provisioning a real demucs (task-5
    // review, Important 2 — the original `panic!` here made that documented
    // command always fail this one test).
    if std::env::var("HUMM_DEMUCS_CONTRACT").as_deref() != Ok("1") {
        // `println!`, not `eprintln!` — visible under `--nocapture` so this
        // gate being skipped can never be mistaken for it having run and
        // passed (Standing Policy rule 5: this command's output must stay
        // trustworthy).
        println!(
            "SKIPPED (not run): real_demucs_isolates_vocals_from_whisper_french — \
             HUMM_DEMUCS_CONTRACT is not set to 1, so this reports `ok` without \
             touching demucs at all — provision a real demucs install first \
             (see this test's doc comment) and point HUMM_DEMUCS at it if it \
             isn't a plain `demucs` on PATH"
        );
        return;
    }

    // Copied into a scratch dir so the isolated `_speech.mp3` derivative
    // never lands beside the shared Buzz testdata fixtures.
    let input_dir = tempfile::tempdir().expect("tempdir");
    let input = input_dir.path().join("whisper-french.mp3");
    std::fs::copy(testdata().join("whisper-french.mp3"), &input).expect("copy fixture");

    let prober = FfprobeProber::new();
    let input_duration = prober
        .probe(&input)
        .expect("probe input")
        .duration
        .expect("input has a known duration");

    let isolator = DemucsIsolator::new();
    assert!(
        isolator.available(),
        "HUMM_DEMUCS_CONTRACT=1 but demucs isn't available — check HUMM_DEMUCS/PATH"
    );

    let cancel = CancelToken::new();
    let start = Instant::now();
    let result = isolator
        .isolate(&input, &cancel)
        .expect("isolate should succeed with a real demucs install");
    println!("demucs isolation took {:?}", start.elapsed());

    assert_eq!(result, input_dir.path().join("whisper-french_speech.mp3"));
    assert!(result.is_file(), "expected {} to exist", result.display());

    // "Decodable audio of ~equal duration": probing (not just a raw
    // ffmpeg decode) proves the output is a genuine, readable audio
    // container, and its duration should match the input within a couple
    // of seconds (Spike C observed 8.568s output vs. 8.517s input — normal
    // MP3 encoder padding, not a truncation/corruption).
    let output_duration = prober
        .probe(&result)
        .expect("probe isolated output")
        .duration
        .expect("output has a known duration");
    let diff_ms = (output_duration.0 - input_duration.0).abs();
    assert!(
        diff_ms < 2000,
        "expected isolated output duration ({output_duration:?}) within 2s of \
         input duration ({input_duration:?})"
    );
}
