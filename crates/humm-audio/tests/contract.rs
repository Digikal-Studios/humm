//! Contract test for [`CpalOutput`] against a real audio output device.
//!
//! `#[ignore]` because it needs CoreAudio (or the platform's real output
//! device), which the sandbox may block. Run explicitly where a device is
//! available (the machine the M3 windowed smoke ran on):
//!
//! ```text
//! cargo test -p humm-audio -- --ignored
//! ```
//!
//! It is the *only* test that verifies the cpal boundary end to end (Standing
//! Policy: an adapter's contract tests alone verify an external upgrade).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use humm_audio::CpalOutput;
use humm_core::boundary::{AudioOutput, StreamSpec};

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
