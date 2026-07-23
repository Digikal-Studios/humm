//! [`CpalCapture`]: the cpal input-device implementation of
//! [`AudioCapture`](humm_core::boundary::AudioCapture) (ADR-005).
//!
//! # Negotiation and the two capture paths
//!
//! [`CpalCapture::start`] first asks the device for a supported input
//! configuration that is exactly 16 kHz, mono, `f32` (`recording_transcriber.py:372-386`
//! parity: Buzz's `get_device_sample_rate` prefers Whisper's native rate when
//! the device supports it). When the device supports that exact
//! configuration, the RT callback pushes converted samples **directly** into
//! the caller's `sink` — there is nothing to resample, so no intermediate
//! ring or drain thread is created at all.
//!
//! Otherwise the adapter falls back to the device's own default input
//! configuration (any rate/channel count, `f32`/`i16`/`u16` — Buzz's own
//! fallback, `sounddevice.query_devices(...).default_samplerate`). The RT
//! callback then converts + downmixes into an **intermediate device-rate
//! ring** (preallocated at `start` time), and a poll loop on the same owner
//! thread that holds the `cpal::Stream` (see below) drains that ring through
//! a preallocated rubato resampler into the caller's `sink`. All allocation
//! (the ring, the resampler's buffers) happens once at `start` time; the RT
//! callback itself never allocates, locks, or resamples.
//!
//! # No scratch buffer for downmixing
//!
//! The RT callback does not accumulate a callback's frames into a scratch
//! `Vec` before downmixing. `cpal`'s buffer size is a per-backend negotiated
//! value the adapter cannot reliably bound ahead of time (`BufferSize::Default`
//! or a backend-chosen size within a `Range`), so a fixed-capacity scratch
//! buffer would either need an arbitrary cap (silently truncating a larger
//! callback) or a size guess. Instead [`capture_callback`] downmixes and
//! pushes one frame at a time with a running sum-of-squares for the RMS —
//! `O(1)` memory regardless of the callback's buffer length, no allocation,
//! and no per-callback size assumption.
//!
//! # Why one owner thread for both the stream and the drain loop
//!
//! Like [`crate::output::CpalOutput`], `cpal::Stream` is `!Send` on CoreAudio
//! (and other backends), but [`CaptureHandle`] must be `Send`. So the stream
//! is built and held on a dedicated owner thread. On the fallback path that
//! same thread also runs the drain loop: it is otherwise idle (parked in
//! `recv_timeout` waiting for a stop command), so reusing it to poll the
//! intermediate ring avoids spawning a second thread. A `recv_timeout` of
//! [`DRAIN_POLL_INTERVAL`] serves as both "check for a stop command" and
//! "drain what's arrived since last tick".
//!
//! # Device ids
//!
//! Device ids are the device's name (via cpal's `Display` impl) — cpal
//! device indices are not stable across enumerations, but names are what
//! Buzz's own combo box (`audio_devices_combo_box.py`) and this adapter's
//! `devices()` both key on.
//!
//! # Meter ownership: `current_rms` only
//!
//! The capture adapter writes only [`MeterSnapshot::set_current_rms`], once
//! per RT callback. It does **not** write `queue_len` (or `average_rms`):
//! Buzz's meter "Queue" value is the *transcriber's* accumulation-queue size,
//! emitted once per processed batch by the session/chunker loop
//! (`recording_transcriber.py`'s `queue_size_changed`), never a raw-callback
//! or capture-ring depth. That field belongs to the session/chunker layer (a
//! later task) — see [`MeterSnapshot`]'s own docs for the full ownership
//! split.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, SupportedStreamConfigRange};
use humm_core::boundary::{AudioCapture, AudioDevice, CaptureHandle, MeterSnapshot, SAMPLE_RATE};
use humm_core::error::AudioError;
use rtrb::{Consumer, Producer, RingBuffer};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};

/// Fixed number of input (device-rate) frames the fallback-path resampler
/// consumes per drain iteration.
const RESAMPLE_IN_CHUNK: usize = 1024;

/// How often the capture owner thread wakes to drain the intermediate ring
/// on the fallback path (and to notice a stop command promptly on either
/// path). Short enough that `stop()` acknowledges promptly — the design
/// doc's cooperative-stop requirement (§2.1: the session thread must
/// acknowledge a stop request without a perceptible delay) — this value is
/// this implementation's own promptness target, not an externally documented
/// constraint.
const DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The system's audio input devices, as an [`AudioCapture`].
///
/// Opens a stream on the requested (or system default) input device,
/// negotiating 16 kHz mono `f32` directly where supported and falling back to
/// the device's default configuration with adapter-side downmix/resample
/// otherwise (see the module docs).
#[derive(Debug, Default, Clone, Copy)]
pub struct CpalCapture;

impl CpalCapture {
    /// Creates a new capture bound to whichever device [`AudioCapture::start`]
    /// is asked for (resolved lazily at `start` time).
    pub fn new() -> Self {
        CpalCapture
    }
}

impl AudioCapture for CpalCapture {
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        let host = cpal::default_host();
        let default_name = host.default_input_device().map(|d| d.to_string());

        let devices = host
            .input_devices()
            .map_err(|e| AudioError::DeviceUnavailable {
                detail: format!("could not enumerate input devices: {e}"),
            })?;

        Ok(devices
            .map(|d| {
                let name = d.to_string();
                let is_default = default_name.as_deref() == Some(name.as_str());
                AudioDevice {
                    id: name.clone(),
                    name,
                    is_default,
                }
            })
            .collect())
    }

    fn start(
        &mut self,
        device_id: Option<&str>,
        sink: Producer<f32>,
        meter: Arc<MeterSnapshot>,
    ) -> Result<Box<dyn CaptureHandle>, AudioError> {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (ready_tx, ready_rx) = channel::<Result<(), AudioError>>();
        let device_id = device_id.map(str::to_string);

        let handle = thread::Builder::new()
            .name("humm-audio-capture".to_string())
            .spawn(move || run_capture_thread(device_id, sink, meter, cmd_rx, ready_tx))
            .map_err(|e| AudioError::StreamBuild {
                detail: format!("could not spawn audio capture thread: {e}"),
            })?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(CpalCaptureHandle {
                cmd_tx,
                handle: Some(handle),
            })),
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err(AudioError::StreamBuild {
                    detail: "audio capture thread exited before reporting readiness".to_string(),
                })
            }
        }
    }
}

/// Control messages sent from [`CpalCaptureHandle`] to its owner thread.
enum Cmd {
    Stop,
}

/// The `Send` handle to a running capture stream: a command channel plus the
/// owner thread. Dropping it stops and joins the thread (which drops the
/// underlying `!Send` `cpal::Stream`).
struct CpalCaptureHandle {
    cmd_tx: Sender<Cmd>,
    handle: Option<JoinHandle<()>>,
}

impl CaptureHandle for CpalCaptureHandle {
    fn stop(self: Box<Self>) {
        // Dropping the box runs `Drop` below, which signals and joins.
    }
}

impl Drop for CpalCaptureHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Cmd::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Owner-thread entry point: builds the stream (which must live on this
/// thread), starts it, reports readiness, then services stop commands and —
/// on the fallback path — drains the intermediate ring until stopped.
fn run_capture_thread(
    device_id: Option<String>,
    sink: Producer<f32>,
    meter: Arc<MeterSnapshot>,
    cmd_rx: Receiver<Cmd>,
    ready_tx: Sender<Result<(), AudioError>>,
) {
    let dropped = Arc::new(AtomicU64::new(0));
    let opened = match open_stream(device_id.as_deref(), sink, meter, Arc::clone(&dropped)) {
        Ok(opened) => opened,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };
    let OpenedCapture { stream, mut drain } = opened;

    if let Err(e) = stream.play() {
        let _ = ready_tx.send(Err(AudioError::StreamBuild {
            detail: format!("could not start audio capture stream: {e}"),
        }));
        return;
    }

    if ready_tx.send(Ok(())).is_err() {
        // Caller gave up before we could report readiness; nothing to run.
        return;
    }

    loop {
        match cmd_rx.recv_timeout(DRAIN_POLL_INTERVAL) {
            Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(drain) = drain.as_mut() {
                    drain.run();
                }
            }
        }
    }
    // Drop the stream *before* the final flush: dropping stops the RT
    // callback from firing again, so nothing new can land in the
    // intermediate ring after this point. Only then do we drain — that
    // guarantees the final flush empties everything that reached the ring
    // while still capturing, with no window where a callback could race the
    // flush and add a sample that never gets drained (drain-then-drop would
    // leave exactly that window open).
    drop(stream);
    if let Some(drain) = drain.as_mut() {
        drain.run();
    }

    let total_dropped = dropped.load(Ordering::Relaxed);
    if total_dropped > 0 {
        log::warn!("audio capture dropped {total_dropped} sample(s) total due to ring overflow");
    }
}

/// A built, playable capture stream plus the fallback path's drain state
/// (`None` on the direct 16 kHz-mono-f32 path, where there is nothing to
/// drain).
struct OpenedCapture {
    stream: cpal::Stream,
    drain: Option<ResampleDrain>,
}

/// Resolves the device, negotiates a configuration, and builds the stream —
/// must run on the eventual owner thread (the returned stream is `!Send`).
fn open_stream(
    device_id: Option<&str>,
    sink: Producer<f32>,
    meter: Arc<MeterSnapshot>,
    dropped: Arc<AtomicU64>,
) -> Result<OpenedCapture, AudioError> {
    let host = cpal::default_host();
    let device = resolve_device(&host, device_id)?;

    let input_configs: Vec<SupportedStreamConfigRange> = device
        .supported_input_configs()
        .map_err(|e| AudioError::StreamBuild {
            detail: format!("could not query supported input configs: {e}"),
        })?
        .collect();

    // 1. Direct path: the device supports 16 kHz mono f32 exactly (Buzz
    //    parity, `recording_transcriber.py:372-386`). Push straight into the
    //    caller's sink — nothing to resample.
    if let Some(range) = input_configs.iter().copied().find(|range| {
        range.channels() == 1
            && range.sample_format() == SampleFormat::F32
            && range.contains_rate(SAMPLE_RATE)
    }) {
        let config: StreamConfig = range.with_sample_rate(SAMPLE_RATE).config();
        let stream = build_direct_stream(&device, config, sink, meter, dropped).map_err(|e| {
            AudioError::StreamBuild {
                detail: e.to_string(),
            }
        })?;
        return Ok(OpenedCapture {
            stream,
            drain: None,
        });
    }

    // 2. Fallback path: the device's own default input config (Buzz parity —
    //    `sounddevice.query_devices(device).default_samplerate`), converted
    //    and resampled to 16 kHz mono in the drain path.
    let default_config = device
        .default_input_config()
        .map_err(|e| AudioError::StreamBuild {
            detail: format!("no usable input configuration: {e}"),
        })?;

    let format = default_config.sample_format();
    if !matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    ) {
        return Err(AudioError::StreamBuild {
            detail: format!(
                "default input device format is {format:?} — humm's capture needs f32, i16, \
                 or u16"
            ),
        });
    }

    let device_rate = default_config.sample_rate();
    let device_channels = usize::from(default_config.channels());
    let config: StreamConfig = default_config.config();

    // ~1 s of device-rate mono samples: generous headroom for the drain
    // thread's poll cadence without unbounded growth.
    let ring_capacity = (device_rate as usize).max(1);
    let (device_producer, device_consumer) = RingBuffer::<f32>::new(ring_capacity);

    let drain = ResampleDrain::new(device_consumer, sink, device_rate, Arc::clone(&dropped))?;

    let stream = build_fallback_stream(
        &device,
        config,
        format,
        device_channels,
        device_producer,
        meter,
        dropped,
    )
    .map_err(|e| AudioError::StreamBuild {
        detail: e.to_string(),
    })?;

    Ok(OpenedCapture {
        stream,
        drain: Some(drain),
    })
}

/// Resolves `device_id` to a concrete device: `None` means the system default
/// input device; `Some(id)` matches by name against every enumerated input
/// device.
fn resolve_device(host: &cpal::Host, device_id: Option<&str>) -> Result<cpal::Device, AudioError> {
    match device_id {
        None => host
            .default_input_device()
            .ok_or_else(|| AudioError::DeviceUnavailable {
                detail: "no default input device available — check the system's audio input \
                     selection"
                    .to_string(),
            }),
        Some(id) => {
            let mut devices = host
                .input_devices()
                .map_err(|e| AudioError::DeviceUnavailable {
                    detail: format!("could not enumerate input devices: {e}"),
                })?;
            devices
                .find(|d| d.to_string() == id)
                .ok_or_else(|| AudioError::DeviceNotFound { id: id.to_string() })
        }
    }
}

/// Builds the direct (16 kHz mono f32, no resample) capture stream.
fn build_direct_stream(
    device: &cpal::Device,
    config: StreamConfig,
    mut sink: Producer<f32>,
    meter: Arc<MeterSnapshot>,
    dropped: Arc<AtomicU64>,
) -> Result<cpal::Stream, cpal::Error> {
    device.build_input_stream::<f32, _, _>(
        config,
        move |data: &[f32], _| {
            capture_callback(data, 1, f32_identity, &mut sink, &meter, &dropped);
        },
        |err| log::error!("audio capture stream error: {err}"),
        None,
    )
}

/// Builds the fallback capture stream (device default config, any of
/// f32/i16/u16), pushing into the intermediate device-rate ring.
fn build_fallback_stream(
    device: &cpal::Device,
    config: StreamConfig,
    format: SampleFormat,
    channels: usize,
    mut producer: Producer<f32>,
    meter: Arc<MeterSnapshot>,
    dropped: Arc<AtomicU64>,
) -> Result<cpal::Stream, cpal::Error> {
    match format {
        SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
            config,
            move |data: &[f32], _| {
                capture_callback(
                    data,
                    channels,
                    f32_identity,
                    &mut producer,
                    &meter,
                    &dropped,
                );
            },
            |err| log::error!("audio capture stream error: {err}"),
            None,
        ),
        SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
            config,
            move |data: &[i16], _| {
                capture_callback(data, channels, i16_to_f32, &mut producer, &meter, &dropped);
            },
            |err| log::error!("audio capture stream error: {err}"),
            None,
        ),
        SampleFormat::U16 => device.build_input_stream::<u16, _, _>(
            config,
            move |data: &[u16], _| {
                capture_callback(data, channels, u16_to_f32, &mut producer, &meter, &dropped);
            },
            |err| log::error!("audio capture stream error: {err}"),
            None,
        ),
        other => unreachable!(
            "open_stream validates the default config format is f32/i16/u16 before calling \
             build_fallback_stream, got {other:?}"
        ),
    }
}

#[inline]
fn f32_identity(s: f32) -> f32 {
    s
}

/// `i16` → `f32`, matching cpal's own `dasp_sample` conversion
/// (`s as f32 / 32_768.0`, origin `0`, range `-1.0..=1.0`).
#[inline]
fn i16_to_f32(s: i16) -> f32 {
    f32::from(s) / 32_768.0
}

/// `u16` → `f32`, matching cpal's own `dasp_sample` conversion (`u16` origin
/// `32_768`, so `(s − 32_768) / 32_768`, range `-1.0..=1.0`).
#[inline]
fn u16_to_f32(s: u16) -> f32 {
    (f32::from(s) - 32_768.0) / 32_768.0
}

/// Converts one RT callback's interleaved raw samples to downmixed mono
/// `f32`, updates `meter`'s per-callback RMS, and pushes each mono sample
/// into `sink`.
///
/// Runs entirely on the audio thread: no allocation, no locking. `convert` is
/// a pure per-sample conversion ([`f32_identity`] for the direct path,
/// [`i16_to_f32`]/[`u16_to_f32`] for the fallback path's non-float formats);
/// downmixing sums the frame's channels and divides by `channels`, and the
/// RMS is accumulated as a running sum of squares rather than into a
/// scratch buffer (see the module docs). Sink overflow increments `dropped`
/// rather than panicking or blocking. Does **not** touch `queue_len` — that
/// field belongs to the session/chunker layer (see the module docs' "Meter
/// ownership" section).
fn capture_callback<S: Copy>(
    data: &[S],
    channels: usize,
    convert: impl Fn(S) -> f32,
    sink: &mut Producer<f32>,
    meter: &MeterSnapshot,
    dropped: &AtomicU64,
) {
    if channels == 0 {
        return;
    }

    let mut sum_sq = 0.0f64;
    let mut frames = 0usize;
    for frame in data.chunks_exact(channels) {
        let mono = frame.iter().copied().map(&convert).sum::<f32>() / channels as f32;
        sum_sq += f64::from(mono) * f64::from(mono);
        frames += 1;
        if sink.push(mono).is_err() {
            dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    if frames > 0 {
        let rms = (sum_sq / frames as f64).sqrt() as f32;
        meter.set_current_rms(rms);
    }
}

/// Fallback-path drain state: pops device-rate mono samples out of the
/// intermediate ring, resamples them to 16 kHz with a preallocated rubato
/// resampler, and pushes the result into the caller's sink. Lives entirely
/// off the RT callback — [`ResampleDrain::run`] is called only from the
/// capture owner thread's poll loop (or, in tests, driven directly against a
/// synthetic ring).
struct ResampleDrain {
    consumer: Consumer<f32>,
    resampler: Async<f32>,
    /// Fixed input chunk size the resampler consumes per call
    /// (`resampler.input_frames_next()`, constant since `FixedAsync::Input`
    /// fixes it and the ratio never changes mid-stream).
    chunk_frames: usize,
    in_buf: Vec<f32>,
    out_buf: Vec<f32>,
    sink: Producer<f32>,
    dropped: Arc<AtomicU64>,
}

impl ResampleDrain {
    fn new(
        consumer: Consumer<f32>,
        sink: Producer<f32>,
        device_rate: u32,
        dropped: Arc<AtomicU64>,
    ) -> Result<Self, AudioError> {
        let ratio = f64::from(SAMPLE_RATE) / f64::from(device_rate);
        let resampler = Async::<f32>::new_poly(
            ratio,
            // Ratio is fixed (the device rate never changes mid-stream); no
            // dynamic-ratio headroom needed.
            1.0,
            PolynomialDegree::Cubic,
            RESAMPLE_IN_CHUNK,
            1, // mono
            FixedAsync::Input,
        )
        .map_err(|e| AudioError::StreamBuild {
            detail: format!(
                "could not build capture resampler ({device_rate} Hz -> {SAMPLE_RATE} Hz): {e}"
            ),
        })?;

        let chunk_frames = resampler.input_frames_next();
        let out_capacity = resampler.output_frames_max();
        Ok(ResampleDrain {
            consumer,
            resampler,
            chunk_frames,
            in_buf: vec![0.0; chunk_frames],
            out_buf: vec![0.0; out_capacity],
            sink,
            dropped,
        })
    }

    /// Drains every complete input chunk currently buffered in the
    /// intermediate ring, resampling each to 16 kHz and pushing the result
    /// into the sink. Returns the number of output samples successfully
    /// pushed (for tests; the owner thread ignores it).
    fn run(&mut self) -> usize {
        let mut produced = 0;
        while self.consumer.slots() >= self.chunk_frames {
            for slot in &mut self.in_buf {
                *slot = self
                    .consumer
                    .pop()
                    .expect("checked consumer.slots() before popping");
            }

            let out_capacity = self.out_buf.len();
            let input = InterleavedSlice::new(&self.in_buf, 1, self.chunk_frames)
                .expect("in_buf sized for chunk_frames");
            let mut output = InterleavedSlice::new_mut(&mut self.out_buf, 1, out_capacity)
                .expect("out_buf sized for output_frames_max");

            match self
                .resampler
                .process_into_buffer(&input, &mut output, None)
            {
                Ok((_in_used, out_frames)) => {
                    for &sample in &self.out_buf[..out_frames] {
                        if self.sink.push(sample).is_err() {
                            self.dropped.fetch_add(1, Ordering::Relaxed);
                        } else {
                            produced += 1;
                        }
                    }
                }
                Err(e) => {
                    log::error!("audio capture resample failed: {e}");
                }
            }
        }
        produced
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- sample format conversion -------------------------------------------------

    #[test]
    fn i16_to_f32_zero_is_zero() {
        assert_eq!(i16_to_f32(0), 0.0);
    }

    #[test]
    fn i16_to_f32_min_is_minus_one() {
        assert_eq!(i16_to_f32(i16::MIN), -1.0);
    }

    #[test]
    fn i16_to_f32_max_is_just_under_one() {
        let v = i16_to_f32(i16::MAX);
        assert!((0.999_9..1.0).contains(&v), "{v}");
    }

    #[test]
    fn u16_to_f32_origin_is_zero() {
        assert_eq!(u16_to_f32(32_768), 0.0);
    }

    #[test]
    fn u16_to_f32_min_is_minus_one() {
        assert_eq!(u16_to_f32(u16::MIN), -1.0);
    }

    #[test]
    fn u16_to_f32_max_is_just_under_one() {
        let v = u16_to_f32(u16::MAX);
        assert!((0.999_9..1.0).contains(&v), "{v}");
    }

    // -- capture_callback: downmix, RMS, overflow, queue_len non-ownership -------

    fn meter_and_ring(capacity: usize) -> (MeterSnapshot, Producer<f32>, Consumer<f32>) {
        let meter = MeterSnapshot::new();
        let (producer, consumer) = RingBuffer::<f32>::new(capacity);
        (meter, producer, consumer)
    }

    #[test]
    fn capture_callback_downmixes_stereo_by_averaging() {
        let (meter, mut sink, mut consumer) = meter_and_ring(16);
        let dropped = AtomicU64::new(0);

        // Two stereo frames: (0.2, 0.6) -> 0.4, (1.0, -0.5) -> 0.25.
        let data = [0.2f32, 0.6, 1.0, -0.5];
        capture_callback(&data, 2, f32_identity, &mut sink, &meter, &dropped);

        assert!((consumer.pop().unwrap() - 0.4).abs() < 1e-6);
        assert!((consumer.pop().unwrap() - 0.25).abs() < 1e-6);
        assert!(consumer.pop().is_err());
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn capture_callback_mono_frame_is_unchanged() {
        let (meter, mut sink, mut consumer) = meter_and_ring(16);
        let dropped = AtomicU64::new(0);

        let data = [0.3f32, -0.7, 0.1];
        capture_callback(&data, 1, f32_identity, &mut sink, &meter, &dropped);

        assert_eq!(consumer.pop().unwrap(), 0.3);
        assert_eq!(consumer.pop().unwrap(), -0.7);
        assert_eq!(consumer.pop().unwrap(), 0.1);
    }

    #[test]
    fn capture_callback_converts_i16_before_downmixing() {
        let (meter, mut sink, mut consumer) = meter_and_ring(16);
        let dropped = AtomicU64::new(0);

        // One stereo i16 frame: max and min average to (approximately) 0.
        let data = [i16::MAX, i16::MIN];
        capture_callback(&data, 2, i16_to_f32, &mut sink, &meter, &dropped);

        let mono = consumer.pop().unwrap();
        assert!(mono.abs() < 1e-3, "{mono}");
    }

    #[test]
    fn capture_callback_computes_rms_over_callback_frames() {
        let (meter, mut sink, _consumer) = meter_and_ring(16);
        let dropped = AtomicU64::new(0);

        // Mono frames: 0.5, -0.5, 0.5, -0.5 -> RMS = 0.5 exactly.
        let data = [0.5f32, -0.5, 0.5, -0.5];
        capture_callback(&data, 1, f32_identity, &mut sink, &meter, &dropped);

        assert!((meter.get_current_rms() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn capture_callback_does_not_touch_queue_len() {
        // queue_len is owned by the session/chunker layer (Buzz's
        // transcriber queue_size_changed, once per processed batch), never
        // the capture adapter — see MeterSnapshot's and this module's docs.
        let (meter, mut sink, _consumer) = meter_and_ring(16);
        let dropped = AtomicU64::new(0);

        let data = [0.1f32, 0.2, 0.3, 0.4];
        capture_callback(&data, 1, f32_identity, &mut sink, &meter, &dropped);

        assert_eq!(meter.get_queue_len(), 0);
    }

    #[test]
    fn capture_callback_counts_overflow_drops() {
        let (meter, mut sink, mut consumer) = meter_and_ring(2);
        let dropped = AtomicU64::new(0);

        // 4 mono frames into a 2-slot ring: 2 pushed, 2 dropped.
        let data = [0.1f32, 0.2, 0.3, 0.4];
        capture_callback(&data, 1, f32_identity, &mut sink, &meter, &dropped);

        assert_eq!(dropped.load(Ordering::Relaxed), 2);
        assert_eq!(consumer.pop().unwrap(), 0.1);
        assert_eq!(consumer.pop().unwrap(), 0.2);
        assert!(consumer.pop().is_err());
    }

    // -- ResampleDrain: drives the drain logic against a synthetic ring ----------

    /// Fills a ring with `n` samples of a synthetic tone so resampled output
    /// is non-silent (a silence-only input could hide a broken resampler).
    fn synthetic_tone_ring(n: usize, sample_rate: u32) -> (Producer<f32>, Consumer<f32>) {
        let (mut producer, consumer) = RingBuffer::<f32>::new(n);
        let step = 2.0 * std::f32::consts::PI * 440.0 / sample_rate as f32;
        let mut phase = 0.0f32;
        for _ in 0..n {
            producer.push(phase.sin()).expect("ring sized for n");
            phase += step;
        }
        (producer, consumer)
    }

    #[test]
    fn resample_drain_48k_to_16k_produces_expected_sample_count() {
        let device_rate = 48_000u32;
        let one_second = device_rate as usize;
        let (_producer, consumer) = synthetic_tone_ring(one_second, device_rate);

        let (sink_producer, mut sink_consumer) = RingBuffer::<f32>::new(SAMPLE_RATE as usize * 2);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut drain = ResampleDrain::new(consumer, sink_producer, device_rate, dropped.clone())
            .expect("48kHz -> 16kHz is a valid resample ratio");

        let produced = drain.run();

        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        assert!(produced > 0);

        let expected = SAMPLE_RATE as f64; // 1 second of 48k in -> 1 second of 16k out.
        let rel_err = (produced as f64 - expected).abs() / expected;
        assert!(
            rel_err < 0.05,
            "produced {produced} samples for {one_second} @ {device_rate}Hz input; \
             expected ~{expected} (rel err {rel_err})"
        );

        // Confirm the pushed samples actually landed in the sink and are not
        // silent (a broken resampler could still report a plausible count).
        let mut saw_nonsilence = false;
        while let Ok(s) = sink_consumer.pop() {
            if s.abs() > 1e-4 {
                saw_nonsilence = true;
            }
        }
        assert!(saw_nonsilence);
    }

    #[test]
    fn resample_drain_ratio_one_is_near_pass_through_count() {
        // Device already at 16 kHz (but, say, stereo before downmix — this
        // struct only ever sees mono, so this covers the "no resample really
        // needed but we still built a resampler" branch of open_stream).
        let device_rate = SAMPLE_RATE;
        let n = SAMPLE_RATE as usize;
        let (_producer, consumer) = synthetic_tone_ring(n, device_rate);

        let (sink_producer, sink_consumer) = RingBuffer::<f32>::new(n * 2);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut drain = ResampleDrain::new(consumer, sink_producer, device_rate, dropped.clone())
            .expect("ratio 1.0 is a valid resample ratio");

        let produced = drain.run();

        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        let rel_err = (produced as f64 - n as f64).abs() / n as f64;
        assert!(
            rel_err < 0.05,
            "produced {produced} for {n} in at ratio 1.0"
        );
        drop(sink_consumer);
    }

    #[test]
    fn resample_drain_counts_sink_overflow_drops() {
        let device_rate = 48_000u32;
        let n = device_rate as usize;
        let (_producer, consumer) = synthetic_tone_ring(n, device_rate);

        // Sink far too small to hold a full second of 16 kHz output.
        let (sink_producer, mut sink_consumer) = RingBuffer::<f32>::new(4);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut drain = ResampleDrain::new(consumer, sink_producer, device_rate, dropped.clone())
            .expect("48kHz -> 16kHz is a valid resample ratio");

        drain.run();

        assert!(dropped.load(Ordering::Relaxed) > 0);
        // Sanity: the tiny sink still received *something*.
        assert!(sink_consumer.pop().is_ok());
    }

    #[test]
    fn resample_drain_leaves_a_partial_chunk_unconsumed() {
        // Fewer samples than one input chunk: run() must not pop anything
        // (would need to `pop().expect()` past an empty ring otherwise).
        let device_rate = 48_000u32;
        let (_producer, consumer) = synthetic_tone_ring(RESAMPLE_IN_CHUNK - 1, device_rate);

        let (sink_producer, _sink_consumer) = RingBuffer::<f32>::new(64);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut drain = ResampleDrain::new(consumer, sink_producer, device_rate, dropped.clone())
            .expect("48kHz -> 16kHz is a valid resample ratio");

        let produced = drain.run();
        assert_eq!(produced, 0);
    }
}
