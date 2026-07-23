//! [`MockCapture`]: a scripted [`AudioCapture`] implementation with no real
//! time, no threads, and no audio device.
//!
//! Construct it with a prerecorded 16 kHz mono `f32` script, [`start`] it like
//! any real [`AudioCapture`], then call [`MockCapture::pump`] to push the
//! next `n` scripted samples through the sink synchronously and update the
//! meter to match — exactly the shape `CpalCapture`'s RT callback produces,
//! but driven by hand instead of by a real capture stream. This is what
//! core-session tests, GUI tests, and CI use in place of real hardware.
//!
//! [`start`]: humm_core::boundary::AudioCapture::start

use std::sync::Arc;

use humm_core::boundary::{AudioCapture, AudioDevice, CaptureHandle, MeterSnapshot};
use humm_core::error::AudioError;

/// A scripted capture source: samples are fed through the boundary's
/// producer only when [`MockCapture::pump`] is called, and only as many as
/// requested (or fewer, once the script is exhausted). No thread, no wall
/// clock — the "capture" happens synchronously on the caller's thread.
pub struct MockCapture {
    script: Vec<f32>,
    position: usize,
    sink: Option<rtrb::Producer<f32>>,
    meter: Option<Arc<MeterSnapshot>>,
    dropped: u64,
    devices: Vec<AudioDevice>,
}

impl MockCapture {
    /// Creates a capture scripted to play back `script` (16 kHz mono `f32`)
    /// in order, one [`pump`](Self::pump) call at a time. `devices()` returns
    /// a single scripted default device ("Mock Microphone") until overridden
    /// with [`Self::with_devices`].
    pub fn new(script: Vec<f32>) -> Self {
        MockCapture {
            script,
            position: 0,
            sink: None,
            meter: None,
            dropped: 0,
            devices: vec![AudioDevice {
                id: "mock-default".to_string(),
                name: "Mock Microphone".to_string(),
                is_default: true,
            }],
        }
    }

    /// Overrides the scripted device list [`AudioCapture::devices`] returns
    /// (default: a single default-flagged "Mock Microphone").
    #[must_use]
    pub fn with_devices(mut self, devices: Vec<AudioDevice>) -> Self {
        self.devices = devices;
        self
    }

    /// Pushes the next `n` scripted samples into the sink established by
    /// [`AudioCapture::start`] (fewer once the script is exhausted), then
    /// updates the meter's current RMS over exactly those samples and its
    /// queue length to the sink's resulting occupancy — the same telemetry
    /// shape `CpalCapture`'s RT callback produces per callback.
    ///
    /// A push that overflows the sink is counted (see [`Self::dropped`])
    /// rather than panicking, mirroring `CpalCapture`'s overflow policy.
    ///
    /// Returns the number of samples actually pumped (`< n` at end of
    /// script). Panics if called before [`AudioCapture::start`].
    pub fn pump(&mut self, n: usize) -> usize {
        let sink = self
            .sink
            .as_mut()
            .expect("MockCapture::pump called before AudioCapture::start");
        let meter = self
            .meter
            .as_ref()
            .expect("MockCapture::pump called before AudioCapture::start");

        let end = (self.position + n).min(self.script.len());
        let batch = &self.script[self.position..end];
        self.position = end;

        let mut sum_sq = 0.0f64;
        for &sample in batch {
            sum_sq += f64::from(sample) * f64::from(sample);
            if sink.push(sample).is_err() {
                self.dropped += 1;
            }
        }

        if !batch.is_empty() {
            let rms = (sum_sq / batch.len() as f64).sqrt() as f32;
            meter.set_current_rms(rms);
        }
        let occupied = sink.buffer().capacity().saturating_sub(sink.slots());
        meter.set_queue_len(occupied);

        batch.len()
    }

    /// Total samples dropped so far because the sink was full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// `true` once every scripted sample has been pumped.
    pub fn is_exhausted(&self) -> bool {
        self.position >= self.script.len()
    }
}

impl AudioCapture for MockCapture {
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        Ok(self.devices.clone())
    }

    fn start(
        &mut self,
        _device_id: Option<&str>,
        sink: rtrb::Producer<f32>,
        meter: Arc<MeterSnapshot>,
    ) -> Result<Box<dyn CaptureHandle>, AudioError> {
        self.sink = Some(sink);
        self.meter = Some(meter);
        self.position = 0;
        self.dropped = 0;
        Ok(Box::new(MockCaptureHandle))
    }
}

/// No-op handle: `MockCapture` owns no real stream or thread to stop —
/// callers simply stop calling [`MockCapture::pump`].
struct MockCaptureHandle;

impl CaptureHandle for MockCaptureHandle {
    fn stop(self: Box<Self>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    #[test]
    fn devices_lists_a_default_mock_microphone() {
        let capture = MockCapture::new(vec![]);
        let devices = capture.devices().expect("mock devices never fail");
        assert_eq!(devices.len(), 1);
        assert!(devices[0].is_default);
        assert_eq!(devices[0].name, "Mock Microphone");
    }

    #[test]
    fn with_devices_overrides_the_scripted_list() {
        let capture = MockCapture::new(vec![]).with_devices(vec![
            AudioDevice {
                id: "a".to_string(),
                name: "Device A".to_string(),
                is_default: false,
            },
            AudioDevice {
                id: "b".to_string(),
                name: "Device B".to_string(),
                is_default: true,
            },
        ]);
        let devices = capture.devices().unwrap();
        assert_eq!(devices.len(), 2);
        assert!(devices[1].is_default);
    }

    #[test]
    fn pump_pushes_the_requested_count_and_advances_position() {
        let mut capture = MockCapture::new(vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, mut consumer) = RingBuffer::<f32>::new(16);
        capture.start(None, producer, meter).unwrap();

        let pumped = capture.pump(3);

        assert_eq!(pumped, 3);
        assert_eq!(consumer.pop().unwrap(), 0.1);
        assert_eq!(consumer.pop().unwrap(), 0.2);
        assert_eq!(consumer.pop().unwrap(), 0.3);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn pump_sets_current_rms_over_the_pumped_batch() {
        let mut capture = MockCapture::new(vec![0.5, -0.5, 0.5, -0.5, 999.0]);
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, _consumer) = RingBuffer::<f32>::new(16);
        capture.start(None, producer, meter.clone()).unwrap();

        // Pump only the first 4 samples (RMS = 0.5 exactly); the 999.0
        // outlier must not be included.
        capture.pump(4);

        assert!((meter.get_current_rms() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn pump_sets_queue_len_to_sink_occupancy() {
        let mut capture = MockCapture::new(vec![0.1, 0.2, 0.3]);
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, _consumer) = RingBuffer::<f32>::new(16);
        capture.start(None, producer, meter.clone()).unwrap();

        capture.pump(3);

        assert_eq!(meter.get_queue_len(), 3);
    }

    #[test]
    fn pump_past_the_end_of_script_returns_only_whats_left() {
        let mut capture = MockCapture::new(vec![0.1, 0.2]);
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, mut consumer) = RingBuffer::<f32>::new(16);
        capture.start(None, producer, meter).unwrap();

        let pumped = capture.pump(10);

        assert_eq!(pumped, 2);
        assert!(capture.is_exhausted());
        assert_eq!(consumer.pop().unwrap(), 0.1);
        assert_eq!(consumer.pop().unwrap(), 0.2);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn pump_after_exhaustion_pumps_nothing() {
        let mut capture = MockCapture::new(vec![0.1]);
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, _consumer) = RingBuffer::<f32>::new(16);
        capture.start(None, producer, meter).unwrap();

        capture.pump(1);
        let pumped = capture.pump(5);

        assert_eq!(pumped, 0);
    }

    #[test]
    fn pump_counts_sink_overflow_as_dropped() {
        let mut capture = MockCapture::new(vec![0.1, 0.2, 0.3, 0.4]);
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, mut consumer) = RingBuffer::<f32>::new(2);
        capture.start(None, producer, meter).unwrap();

        capture.pump(4);

        assert_eq!(capture.dropped(), 2);
        assert_eq!(consumer.pop().unwrap(), 0.1);
        assert_eq!(consumer.pop().unwrap(), 0.2);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn start_resets_position_and_dropped_count_for_a_restart() {
        let mut capture = MockCapture::new(vec![0.1, 0.2, 0.3]);
        let meter = Arc::new(MeterSnapshot::new());
        let (producer, _consumer) = RingBuffer::<f32>::new(16);
        capture.start(None, producer, meter.clone()).unwrap();
        capture.pump(2);
        assert!(!capture.is_exhausted());

        // Restart: a fresh sink, and pump semantics start over from sample 0.
        let (producer2, mut consumer2) = RingBuffer::<f32>::new(16);
        capture.start(None, producer2, meter).unwrap();
        assert!(!capture.is_exhausted());

        capture.pump(1);
        assert_eq!(consumer2.pop().unwrap(), 0.1);
    }

    #[test]
    #[should_panic(expected = "before AudioCapture::start")]
    fn pump_before_start_panics() {
        let mut capture = MockCapture::new(vec![0.1]);
        capture.pump(1);
    }
}
