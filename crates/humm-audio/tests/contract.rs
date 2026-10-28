//! Contract tests for [`CpalOutput`] and [`CpalCapture`] against real audio
//! devices.
//!
//! `#[ignore]` because they need CoreAudio (or the platform's real
//! input/output device), which the sandbox may block. Run explicitly where a
//! device is available (the machine the M3 windowed smoke ran on):
//!
//! ```text
//! cargo test -p humm-audio -- --ignored
//! ```
//!
//! These are the *only* tests that verify the cpal boundary end to end
//! (Standing Policy: an adapter's contract tests alone verify an external
//! upgrade).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use humm_audio::{CpalCapture, CpalOutput};
use humm_core::boundary::{AudioCapture, AudioOutput, MeterSnapshot, SAMPLE_RATE, StreamSpec};
use rtrb::RingBuffer;

/// Opens the default device at 44.1 kHz stereo, plays ~100 ms of a 440 Hz
/// sine, and asserts the stream opened and the `fill` callback was invoked.
#[test]
#[ignore = "requires a real audio output device (CoreAudio); run with --ignored"]
fn cpal_output_opens_and_invokes_fill() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_cb = calls.clone();

    // 440 Hz sine at 44.1 kHz, phase carried across callbacks.
    let mut phase = 0.0f32;
    let fill = Box::new(move |buf: &mut [f32]| {
        calls_in_cb.fetch_add(1, Ordering::Relaxed);
        for frame in buf.chunks_mut(2) {
            let sample = (phase * std::f32::consts::TAU).sin() * 0.2;
            phase = (phase + 440.0 / 44_100.0).fract();
            for channel in frame.iter_mut() {
                *channel = sample;
            }
        }
    });

    let output = CpalOutput::new();
    let spec = StreamSpec {
        sample_rate: 44_100,
        channels: 2,
    };

    let mut stream = output
        .open(spec, fill)
        .expect("open default output device at 44.1 kHz stereo");
    stream.play().expect("start playback");
    thread::sleep(Duration::from_millis(100));
    stream.pause().expect("pause playback");
    drop(stream);

    assert!(
        calls.load(Ordering::Relaxed) > 0,
        "fill callback was never invoked during 100 ms of playback"
    );
}

/// Opens the default input device, captures for a fixed window, and asserts:
/// samples arrived, the effective output rate is 16 kHz mono (sample count vs
/// the *capture* window within ±10%), the meter's RMS stayed finite, and
/// `stop` (dropping the handle) returned promptly — the design doc's
/// cooperative-stop requirement (§2.1: the session thread must acknowledge a
/// stop request without a perceptible delay), checked here against this
/// implementation's own promptness target rather than any externally
/// documented budget.
///
/// # Why the window is measured *after* `start()` returns
///
/// The capture rate must be checked against the time the stream was actually
/// live, not against wall-clock that includes device-open latency. `start()`
/// blocks until the owner thread has opened the CoreAudio device and called
/// `stream.play()` — and on real hardware that device open
/// (`default_input_device()` + `build_input_stream()`) can take *seconds*, is
/// highly variable run-to-run, and captures no audio. Timing the window from
/// before `start()` (as an earlier revision of this test did) counts that dead
/// startup as if it were capture time, so `16 kHz × elapsed` overshoots the
/// real sample count by however long the device took to open — the first
/// real-hardware run measured a 9 s open against a 1 s capture (F-060). The
/// honest yardstick is `16 kHz × (window measured from when the stream went
/// live)`; the residual tolerance below absorbs the small fixed effects the
/// capture path itself introduces (see the assertion).
#[test]
#[ignore = "requires a real audio input device (CoreAudio); run with --ignored"]
fn cpal_capture_streams_16khz_mono_and_stops_promptly() {
    let (sink, mut consumer) = RingBuffer::<f32>::new(SAMPLE_RATE as usize * 5);
    let meter = Arc::new(MeterSnapshot::new());

    let mut capture = CpalCapture::new();
    let handle = capture
        .start(None, sink, meter.clone())
        .expect("open default input device");
    // The stream is live once `start()` returns (the owner thread has already
    // called `stream.play()`), so the capture window starts *here* — not before
    // `start()`, whose CoreAudio device-open latency captures no audio (see the
    // test's doc comment). A 2 s window keeps the small fixed startup/tail
    // effects well under the tolerance.
    let stream_live_at = Instant::now();

    let capture_duration = Duration::from_millis(2000);
    thread::sleep(capture_duration);
    let capture_window = stream_live_at.elapsed();

    let stop_started = Instant::now();
    handle.stop();
    let stop_elapsed = stop_started.elapsed();

    let mut received = 0usize;
    while consumer.pop().is_ok() {
        received += 1;
    }

    assert!(received > 0, "no samples arrived during the capture window");

    // Sample-count-vs-capture-window tolerance. Against the honestly-measured
    // window the capture path tracks 16 kHz to well under a percent (the first
    // real-hardware run measured 0.7% over a 1 s window, F-060); ±10% is
    // generous headroom for the small fixed effects the count carries that the
    // window does not: the CoreAudio warm-up before the first callback (a few
    // ms), up to one drain poll interval of tail flushed after `stop()`, and OS
    // scheduling slop on the sleep.
    let expected = f64::from(SAMPLE_RATE) * capture_window.as_secs_f64();
    let rel_err = (received as f64 - expected).abs() / expected;
    assert!(
        rel_err < 0.10,
        "received {received} samples over a {capture_window:?} capture window; expected \
         ~{expected:.0} at 16kHz (rel err {rel_err:.3})"
    );

    let rms = meter.get_current_rms();
    assert!(rms.is_finite(), "quiet-room RMS was not finite: {rms}");

    assert!(
        stop_elapsed < Duration::from_millis(250),
        "stop() took {stop_elapsed:?}; expected well under this implementation's own \
         promptness target (a perceptible-delay-free cooperative stop, design doc §2.1)"
    );

    println!(
        "cpal_capture_streams_16khz_mono_and_stops_promptly: {received} samples over a \
         {capture_window:?} capture window (~{:.0} Hz effective), RMS {rms}, stop() took \
         {stop_elapsed:?}",
        received as f64 / capture_window.as_secs_f64()
    );
}

/// Reports the default input device's native (unnegotiated) sample rate and
/// format, then runs a short real capture — closing the carried M4
/// hardware-debt item ("does the 48 kHz resample branch actually run on this
/// machine's default mic, for real"). This machine's default mic is expected
/// to report 48 kHz; if so, `CpalCapture::start`'s fallback/resample path
/// (device default config → rubato → 16 kHz) is what served the samples
/// above, not the direct 16 kHz-mono-f32 path.
#[test]
#[ignore = "requires a real audio input device (CoreAudio); run with --ignored"]
fn cpal_capture_reports_native_device_rate() {
    let host = humm_audio::system_host().expect("audio host initializes");
    let device = host
        .default_input_device()
        .expect("a default input device is available on this machine");
    let default_config = device
        .default_input_config()
        .expect("default input device reports a default input config");
    let native_rate = default_config.sample_rate();
    let native_channels = default_config.channels();
    let native_format = default_config.sample_format();

    let supports_exact_16k_mono_f32 = device
        .supported_input_configs()
        .expect("device reports its supported input configs")
        .any(|r| {
            r.channels() == 1
                && r.sample_format() == cpal::SampleFormat::F32
                && r.contains_rate(SAMPLE_RATE)
        });

    println!(
        "cpal_capture_reports_native_device_rate: default input device native config is \
         {native_rate} Hz, {native_channels} channel(s), {native_format:?}; supports exact \
         16 kHz mono f32 = {supports_exact_16k_mono_f32} (so CpalCapture::start takes the \
         {} path)",
        if supports_exact_16k_mono_f32 {
            "direct 16kHz-mono-f32"
        } else {
            "fallback device-default + rubato resample"
        }
    );

    // Confirm the negotiated path actually runs end to end at whatever rate
    // this machine's device reports.
    let (sink, mut consumer) = RingBuffer::<f32>::new(SAMPLE_RATE as usize * 3);
    let meter = Arc::new(MeterSnapshot::new());
    let mut capture = CpalCapture::new();
    let handle = capture
        .start(None, sink, meter)
        .expect("open default input device");
    thread::sleep(Duration::from_millis(500));
    handle.stop();

    let mut received = 0usize;
    while consumer.pop().is_ok() {
        received += 1;
    }
    assert!(
        received > 0,
        "no samples arrived during the native-rate confirmation capture"
    );
    println!(
        "cpal_capture_reports_native_device_rate: {received} samples received at the \
         negotiated 16kHz-mono-f32 sink over ~500ms"
    );
}
