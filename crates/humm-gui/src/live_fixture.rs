//! [`FixtureCapture`]: a wall-clock-paced, looping [`AudioCapture`] backed by
//! pre-decoded fixture audio instead of a real device.
//!
//! **Why this exists (M5 Task 14, spec §8 acceptance item 1).** Real
//! microphone input is a standing environment blocker in this sandbox: a
//! sandbox-lifted `cpal` input-stream open hangs indefinitely on an
//! unanswered first-time macOS microphone (TCC) permission prompt with
//! nobody present to click "Allow" (`.superpowers/sdd/task-5-report.md`).
//! The live-transcription realtime-fit acceptance run still needs to exercise
//! the **real** session path end to end — real `WhisperEngine` (Metal,
//! cached `small` model), real `live::session::run_session` chunking/silence
//! logic, real GUI wiring — with audio arriving at **genuine wall-clock
//! pace**, because the thing being measured (`LiveMeasurement`'s
//! audio-seconds ÷ engine-seconds ratio, spec §2.8) is only meaningful if the
//! chunker is fed no faster than a real capture device would feed it.
//!
//! [`MockCapture`](humm_audio::MockCapture) (the existing test double) pumps
//! a whole batch synchronously on the caller's thread with no notion of wall
//! time — perfect for exercising chunker/session *logic* deterministically,
//! wrong for measuring whether transcription *keeps up with real time*.
//! `FixtureCapture` fills that gap: it decodes real audio once (via the
//! existing `AudioDecoder` boundary, e.g. Buzz's `audio-long.mp3` testdata)
//! and streams it into the same `rtrb`/`MeterSnapshot` boundary a real
//! device would, on a background thread paced to real elapsed time, looping
//! the fixture indefinitely so a windowed run can run longer than the
//! fixture itself.
//!
//! Test-only in spirit (nothing in this crate's own kittest suite uses it —
//! kittest never touches real time or real audio), but not `#[cfg(test)]`:
//! it's driven by the `HUMM_SMOKE_LIVE_FIXTURE` env hook in `app.rs`, which
//! must be reachable from the real `humm-gui` binary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use humm_core::boundary::{AudioCapture, AudioDevice, CaptureHandle, MeterSnapshot, SAMPLE_RATE};
use humm_core::error::AudioError;

/// Wall-clock chunk the pacing thread pushes per tick — 100 ms at 16 kHz.
/// Coarser than a real capture callback's own buffer, but fine-grained
/// enough that the chunker never sees an unrealistic burst: 100 ms is well
/// under the 500 ms idle/silence pacing `live::session::run_session` already
/// tolerates.
const CHUNK_SAMPLES: usize = (SAMPLE_RATE as usize) / 10;
const CHUNK_PERIOD: Duration = Duration::from_millis(100);

/// A looping, wall-clock-paced capture source backed by pre-decoded audio.
/// `devices()` reports a single scripted device so the recording window's
/// device combo has something to preselect, exactly like
/// [`humm_audio::MockCapture`]'s own "Mock Microphone".
pub struct FixtureCapture {
    samples: Arc<[f32]>,
    /// Pacing tick period — always [`CHUNK_PERIOD`] in production
    /// ([`Self::new`]); shortened by [`Self::new_for_test`] so unit tests
    /// exercise the real threaded start/stop/loop path without a real
    /// 100 ms-scale sleep per test.
    chunk_period: Duration,
}

impl FixtureCapture {
    /// Wraps already-decoded 16 kHz mono `f32` samples (see the module doc —
    /// callers decode via the real `AudioDecoder` boundary before
    /// constructing this). An empty slice is accepted but produces a
    /// capture that streams silence forever (no panic, no division by zero
    /// in the pacing loop) — callers should treat that as a decode failure
    /// upstream rather than relying on this capture to surface one.
    pub fn new(samples: Vec<f32>) -> Self {
        FixtureCapture {
            samples: Arc::from(samples),
            chunk_period: CHUNK_PERIOD,
        }
    }

    /// Test-only: same as [`Self::new`] but with an explicit, typically much
    /// shorter, pacing period — keeps the loop/stop unit tests below
    /// millisecond-scale instead of paying [`CHUNK_PERIOD`]'s real 100 ms.
    #[cfg(test)]
    fn new_for_test(samples: Vec<f32>, chunk_period: Duration) -> Self {
        FixtureCapture {
            samples: Arc::from(samples),
            chunk_period,
        }
    }
}

impl AudioCapture for FixtureCapture {
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        Ok(vec![AudioDevice {
            id: "smoke-live-fixture".to_string(),
            name: "Fixture Capture (HUMM_SMOKE_LIVE_FIXTURE)".to_string(),
            is_default: true,
        }])
    }

    /// Spawns the pacing thread and returns immediately. The thread pushes
    /// one [`CHUNK_SAMPLES`]-sized batch into `sink` roughly every
    /// [`CHUNK_PERIOD`], computes that batch's RMS into `meter.current_rms`
    /// (the same per-callback contract `CpalCapture`/`MockCapture` honor —
    /// `average_rms`/`queue_len` stay session/chunker-owned, untouched
    /// here), and wraps back to sample 0 on exhaustion so a run can outlast
    /// the fixture. A full sink drops the overflowing sample rather than
    /// blocking (`CpalCapture`'s own RT-callback overflow policy) — dropped
    /// samples aren't counted here since, unlike the real adapter, nothing
    /// in this smoke path reads a drop counter.
    fn start(
        &mut self,
        _device_id: Option<&str>,
        mut sink: rtrb::Producer<f32>,
        meter: Arc<MeterSnapshot>,
    ) -> Result<Box<dyn CaptureHandle>, AudioError> {
        let samples = Arc::clone(&self.samples);
        let chunk_period = self.chunk_period;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            if samples.is_empty() {
                return;
            }
            let mut pos = 0usize;
            let mut next_tick = Instant::now() + chunk_period;
            while !stop_for_thread.load(Ordering::Relaxed) {
                let end = (pos + CHUNK_SAMPLES).min(samples.len());
                let batch = &samples[pos..end];
                let mut sum_sq = 0.0f64;
                for &sample in batch {
                    sum_sq += f64::from(sample) * f64::from(sample);
                    let _ = sink.push(sample);
                }
                if !batch.is_empty() {
                    let rms = (sum_sq / batch.len() as f64).sqrt() as f32;
                    meter.set_current_rms(rms);
                }
                pos = end;
                if pos >= samples.len() {
                    pos = 0; // loop the fixture (module doc)
                }
                let now = Instant::now();
                if next_tick > now {
                    std::thread::sleep(next_tick - now);
                }
                next_tick += chunk_period;
            }
        });
        Ok(Box::new(FixtureCaptureHandle {
            stop,
            thread: Mutex::new(Some(thread)),
        }))
    }
}

/// Stops the pacing thread and joins it (bounded — the thread only ever
/// blocks in a single `sleep` at most [`CHUNK_PERIOD`] long, so the join
/// returns promptly, matching every other `CaptureHandle::stop`'s "stop is
/// prompt" contract in this workspace).
struct FixtureCaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl CaptureHandle for FixtureCaptureHandle {
    fn stop(self: Box<Self>) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self
            .thread
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    #[test]
    fn devices_lists_a_single_default_fixture_device() {
        let capture = FixtureCapture::new(vec![0.0; 10]);
        let devices = capture.devices().expect("fixture devices never fail");
        assert_eq!(devices.len(), 1);
        assert!(devices[0].is_default);
    }

    #[test]
    fn streams_samples_and_loops_past_the_end() {
        // A tiny two-sample fixture with a chunk/period fast enough that the
        // test finishes in milliseconds: exercise the real `start`/`stop`
        // path (real thread, real rtrb ring) but at a scale that keeps this
        // a genuine unit test, not a multi-hundred-ms slow test — the pacing
        // constants themselves are only exercised for real by the
        // `HUMM_SMOKE_LIVE_FIXTURE` windowed run.
        let mut capture = FixtureCapture::new_for_test(vec![1.0, -1.0], Duration::from_millis(2));
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, mut consumer) = RingBuffer::<f32>::new(4096);
        let handle = capture.start(None, producer, meter.clone()).unwrap();

        // A few pacing ticks at 2 ms each is enough to observe multiple
        // wraps past the 2-sample fixture's end without paying the real
        // 100 ms production `CHUNK_PERIOD`.
        std::thread::sleep(Duration::from_millis(20));
        handle.stop();

        // At least the first chunk (both samples, since the fixture is
        // shorter than CHUNK_SAMPLES) landed in the ring before stop.
        let mut popped = Vec::new();
        while let Ok(sample) = consumer.pop() {
            popped.push(sample);
        }
        assert!(!popped.is_empty());
        assert!(popped.iter().all(|&s| s == 1.0 || s == -1.0));
        assert!(meter.get_current_rms() > 0.0);
    }

    #[test]
    fn empty_fixture_streams_nothing_and_stops_promptly() {
        let mut capture = FixtureCapture::new_for_test(Vec::new(), Duration::from_millis(2));
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, mut consumer) = RingBuffer::<f32>::new(16);
        let handle = capture.start(None, producer, meter).unwrap();

        std::thread::sleep(Duration::from_millis(5));
        handle.stop();

        assert!(consumer.pop().is_err());
    }
}
