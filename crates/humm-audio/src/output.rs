//! [`CpalOutput`]: the cpal output-device implementation of
//! [`AudioOutput`](humm_core::boundary::AudioOutput) (ADR-005).
//!
//! # Device-rate negotiation and internal resampling
//!
//! The caller opens a stream at a [`StreamSpec`] (rate + channels) and feeds
//! interleaved `f32` frames at that rate. Real output devices do not always
//! support the requested rate (44.1 kHz is common; many devices run natively
//! at 48 kHz). Per ADR-005 the *adapter* absorbs that mismatch: if the device
//! cannot run at `spec.sample_rate`/`spec.channels` in `f32`, [`CpalOutput`]
//! builds the stream at a supported device configuration and resamples the
//! caller's frames to the device rate *inside the callback* with rubato — so
//! the `fill` contract (frames produced at the spec rate) is preserved from the
//! caller's perspective, and a varispeed [`crate::Player`] never has to know the
//! device's real rate.
//!
//! # Why an owner thread
//!
//! `cpal::Stream` is `!Send` on CoreAudio (and several other backends), but the
//! boundary trait [`OutputStream`] must be `Send` (a [`crate::Player`] holds it
//! and is moved across threads inside the GUI). So the stream is built and held
//! entirely on a dedicated owner thread; [`CpalStream`] holds only a command
//! [`Sender`] and the thread's [`JoinHandle`], both `Send`. Play/pause are
//! messages to that thread; dropping the handle stops and joins it.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, SupportedStreamConfig, SupportedStreamConfigRange};
use humm_core::boundary::{AudioOutput, FillCallback, OutputStream, StreamSpec};
use humm_core::error::AudioError;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};

/// Fixed number of output (device-rate) frames produced per rubato chunk on
/// the resample path.
const RESAMPLE_OUT_CHUNK: usize = 1024;

/// The system default audio output device, as an [`AudioOutput`].
///
/// Opens a stream on the host's default output device, negotiating a supported
/// configuration and resampling internally when the device cannot run at the
/// requested rate (see the module docs).
#[derive(Debug, Default, Clone, Copy)]
pub struct CpalOutput;

impl CpalOutput {
    /// Creates a new output bound to the system default device (resolved lazily
    /// at [`AudioOutput::open`] time).
    pub fn new() -> Self {
        CpalOutput
    }
}

impl AudioOutput for CpalOutput {
    fn open(
        &self,
        spec: StreamSpec,
        fill: FillCallback,
    ) -> Result<Box<dyn OutputStream>, AudioError> {
        let (cmd_tx, cmd_rx) = channel::<Cmd>();
        let (ready_tx, ready_rx) = channel::<Result<(), AudioError>>();

        let handle = thread::Builder::new()
            .name("humm-audio-output".to_string())
            .spawn(move || run_output_thread(spec, fill, cmd_rx, ready_tx))
            .map_err(|e| AudioError::StreamBuild {
                detail: format!("could not spawn audio output thread: {e}"),
            })?;

        // Block until the owner thread reports whether the stream built.
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Box::new(CpalStream {
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
                    detail: "audio output thread exited before reporting readiness".to_string(),
                })
            }
        }
    }
}

/// Control messages sent from [`CpalStream`] to its owner thread.
enum Cmd {
    Play,
    Pause,
    Stop,
}

/// The `Send` handle to a running output stream: a command channel plus the
/// owner thread. Dropping it stops and joins the thread (which drops the
/// underlying `!Send` `cpal::Stream`).
struct CpalStream {
    cmd_tx: Sender<Cmd>,
    handle: Option<JoinHandle<()>>,
}

impl OutputStream for CpalStream {
    fn play(&mut self) -> Result<(), AudioError> {
        self.cmd_tx
            .send(Cmd::Play)
            .map_err(|_| AudioError::StreamPlay {
                detail: "audio output thread is no longer running".to_string(),
            })
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.cmd_tx
            .send(Cmd::Pause)
            .map_err(|_| AudioError::StreamPlay {
                detail: "audio output thread is no longer running".to_string(),
            })
    }
}

impl Drop for CpalStream {
    fn drop(&mut self) {
        // Ask the owner thread to stop, then wait for it to drop the stream.
        let _ = self.cmd_tx.send(Cmd::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Owner-thread entry point: builds the stream (which must live on this
/// thread), reports readiness, then services control commands until stopped.
fn run_output_thread(
    spec: StreamSpec,
    fill: FillCallback,
    cmd_rx: Receiver<Cmd>,
    ready_tx: Sender<Result<(), AudioError>>,
) {
    let stream = match build_stream(spec, fill) {
        Ok(stream) => {
            if ready_tx.send(Ok(())).is_err() {
                return; // opener gave up; nothing to run.
            }
            stream
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };

    loop {
        match cmd_rx.recv() {
            Ok(Cmd::Play) => {
                if let Err(e) = stream.play() {
                    log::error!("audio output play failed: {e}");
                }
            }
            Ok(Cmd::Pause) => {
                if let Err(e) = stream.pause() {
                    log::error!("audio output pause failed: {e}");
                }
            }
            // Explicit stop, or the sender was dropped: leave the loop so the
            // stream is dropped and the device released.
            Ok(Cmd::Stop) | Err(_) => break,
        }
    }
    drop(stream);
}

/// Builds a `cpal` output stream for `spec`, choosing a direct or a resampling
/// callback depending on what the default device supports. Must run on the
/// owner thread (the returned stream is `!Send`).
fn build_stream(spec: StreamSpec, fill: FillCallback) -> Result<cpal::Stream, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AudioError::DeviceUnavailable {
            detail: "no default output device — check the system's audio output selection"
                .to_string(),
        })?;

    let f32_ranges: Vec<SupportedStreamConfigRange> = device
        .supported_output_configs()
        .map_err(|e| AudioError::StreamBuild {
            detail: format!("could not query supported output configs: {e}"),
        })?
        .filter(|range| range.sample_format() == SampleFormat::F32)
        .collect();

    // 1. Direct path: an f32 config with the exact channel count that covers
    //    the requested rate. speed-1.0 playback is then a bit-exact copy all
    //    the way to the device.
    if let Some(range) = f32_ranges
        .iter()
        .copied()
        .find(|range| range.channels() == spec.channels && range.contains_rate(spec.sample_rate))
    {
        let config: StreamConfig = range.with_sample_rate(spec.sample_rate).config();
        return build_direct(&device, config, fill);
    }

    // 2. Resample path: pick a device configuration and resample to it.
    let chosen: SupportedStreamConfig = match f32_ranges
        .iter()
        .copied()
        .find(|range| range.channels() == spec.channels)
    {
        // Same channel count, different rate.
        Some(range) => pick_device_rate(range),
        // No f32 config with the requested channel count; fall back to the
        // device default (still required to be f32 — humm's player is f32).
        None => {
            let default = device
                .default_output_config()
                .map_err(|e| AudioError::StreamBuild {
                    detail: format!("no usable output configuration: {e}"),
                })?;
            if default.sample_format() != SampleFormat::F32 {
                return Err(AudioError::StreamBuild {
                    detail: format!(
                        "default output device is {:?}, not f32 — humm's player needs an f32 \
                         output device",
                        default.sample_format()
                    ),
                });
            }
            default
        }
    };

    let device_rate = chosen.sample_rate();
    let device_channels = chosen.channels();
    let config: StreamConfig = chosen.config();
    build_resampled(&device, config, spec, device_rate, device_channels, fill)
}

/// Chooses a concrete device sample rate from a supported range: a standard
/// rate (48 kHz, then 44.1 kHz) if the range covers one, else the range's
/// maximum.
fn pick_device_rate(range: SupportedStreamConfigRange) -> SupportedStreamConfig {
    range
        .try_with_standard_sample_rate()
        .unwrap_or_else(|| range.with_max_sample_rate())
}

/// Builds a stream whose callback feeds `fill` straight into the device buffer
/// (device already runs at the spec rate/channels).
fn build_direct(
    device: &cpal::Device,
    config: StreamConfig,
    mut fill: FillCallback,
) -> Result<cpal::Stream, AudioError> {
    device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _| fill(data),
            |err| log::error!("audio output stream error: {err}"),
            None,
        )
        .map_err(|e| AudioError::StreamBuild {
            detail: e.to_string(),
        })
}

/// Builds a stream whose callback resamples `fill`'s spec-rate frames up/down
/// to the device rate (and remaps channels) inside the RT callback.
fn build_resampled(
    device: &cpal::Device,
    config: StreamConfig,
    spec: StreamSpec,
    device_rate: u32,
    device_channels: u16,
    fill: FillCallback,
) -> Result<cpal::Stream, AudioError> {
    let mut state = ResampleState::new(
        spec.sample_rate,
        device_rate,
        usize::from(spec.channels),
        usize::from(device_channels),
        fill,
    )?;

    device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _| state.render(data),
            |err| log::error!("audio output stream error: {err}"),
            None,
        )
        .map_err(|e| AudioError::StreamBuild {
            detail: e.to_string(),
        })
}

/// Per-stream resampling state living inside the RT callback. All buffers are
/// pre-sized at construction; `render` performs no heap allocation.
struct ResampleState {
    resampler: Async<f32>,
    fill: FillCallback,
    spec_channels: usize,
    device_channels: usize,
    /// Interleaved spec-channel input scratch, `input_frames_max * spec_channels`.
    in_buf: Vec<f32>,
    /// Interleaved spec-channel resampled output, `RESAMPLE_OUT_CHUNK * spec_channels`.
    out_buf: Vec<f32>,
    /// Frames currently valid in `out_buf`.
    out_len_frames: usize,
    /// Frames already drained from `out_buf` into the device buffer.
    out_pos_frames: usize,
}

impl ResampleState {
    fn new(
        src_rate: u32,
        device_rate: u32,
        spec_channels: usize,
        device_channels: usize,
        fill: FillCallback,
    ) -> Result<Self, AudioError> {
        let ratio = f64::from(device_rate) / f64::from(src_rate);
        let resampler = Async::<f32>::new_poly(
            ratio,
            // Ratio is fixed (device rate never changes mid-stream); no
            // dynamic-ratio headroom needed.
            1.0,
            PolynomialDegree::Cubic,
            RESAMPLE_OUT_CHUNK,
            spec_channels,
            FixedAsync::Output,
        )
        .map_err(|e| AudioError::StreamBuild {
            detail: format!("could not build resampler ({src_rate} Hz → {device_rate} Hz): {e}"),
        })?;

        let in_capacity = resampler.input_frames_max() * spec_channels;
        Ok(ResampleState {
            resampler,
            fill,
            spec_channels,
            device_channels,
            in_buf: vec![0.0; in_capacity],
            out_buf: vec![0.0; RESAMPLE_OUT_CHUNK * spec_channels],
            out_len_frames: 0,
            out_pos_frames: 0,
        })
    }

    fn frames_available(&self) -> usize {
        self.out_len_frames - self.out_pos_frames
    }

    /// Fills one device buffer (`device_channels`-interleaved) from the
    /// resampled stream, refilling from `fill` as needed.
    fn render(&mut self, data: &mut [f32]) {
        let frames_wanted = data.len() / self.device_channels;
        let mut written = 0;

        while written < frames_wanted {
            if self.frames_available() == 0 {
                self.refill();
                // A resampler that produced nothing (shouldn't happen with a
                // fixed-output engine) must not spin the callback forever.
                if self.frames_available() == 0 {
                    break;
                }
            }

            let take = (frames_wanted - written).min(self.frames_available());
            for f in 0..take {
                let src = (self.out_pos_frames + f) * self.spec_channels;
                let dst = (written + f) * self.device_channels;
                map_frame(
                    &self.out_buf[src..src + self.spec_channels],
                    &mut data[dst..dst + self.device_channels],
                );
            }
            self.out_pos_frames += take;
            written += take;
        }

        // Zero any tail we could not fill (only on a resampler error).
        for sample in &mut data[written * self.device_channels..] {
            *sample = 0.0;
        }
    }

    /// Pulls one chunk from `fill` and resamples it into `out_buf`.
    fn refill(&mut self) {
        let need_in = self.resampler.input_frames_next();
        let in_slice = &mut self.in_buf[..need_in * self.spec_channels];
        (self.fill)(in_slice);

        // The buffer sizes are controlled entirely by this struct
        // (`in_buf`/`out_buf` sized in `new`), so these adapters cannot fail —
        // a documented-impossible state.
        let input = InterleavedSlice::new(&*in_slice, self.spec_channels, need_in)
            .expect("in_buf sized for input_frames_max");
        let mut output =
            InterleavedSlice::new_mut(&mut self.out_buf, self.spec_channels, RESAMPLE_OUT_CHUNK)
                .expect("out_buf sized for RESAMPLE_OUT_CHUNK");

        match self
            .resampler
            .process_into_buffer(&input, &mut output, None)
        {
            Ok((_in_used, out_frames)) => {
                self.out_len_frames = out_frames;
                self.out_pos_frames = 0;
            }
            Err(e) => {
                log::error!("audio resample failed: {e}");
                self.out_len_frames = 0;
                self.out_pos_frames = 0;
            }
        }
    }
}

/// Maps one interleaved source frame (`src.len()` channels) into one device
/// frame (`dst.len()` channels): identity when equal, mono→N duplication,
/// N→mono averaging, otherwise a best-effort channel-index copy.
fn map_frame(src: &[f32], dst: &mut [f32]) {
    let src_channels = src.len();
    if dst.len() == src_channels {
        dst.copy_from_slice(src);
    } else if src_channels == 1 {
        dst.fill(src[0]);
    } else if dst.len() == 1 {
        dst[0] = src.iter().sum::<f32>() / src_channels as f32;
    } else {
        for (i, d) in dst.iter_mut().enumerate() {
            *d = src[i.min(src_channels - 1)];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_frame_identity_when_channels_equal() {
        let src = [0.25, -0.5];
        let mut dst = [0.0; 2];
        map_frame(&src, &mut dst);
        assert_eq!(dst, [0.25, -0.5]);
    }

    #[test]
    fn map_frame_mono_duplicates_to_all() {
        let src = [0.4];
        let mut dst = [0.0; 3];
        map_frame(&src, &mut dst);
        assert_eq!(dst, [0.4, 0.4, 0.4]);
    }

    #[test]
    fn map_frame_downmixes_to_mono_by_average() {
        let src = [0.2, 0.6];
        let mut dst = [0.0; 1];
        map_frame(&src, &mut dst);
        assert!((dst[0] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn map_frame_widening_copies_and_repeats_last() {
        let src = [0.1, 0.2];
        let mut dst = [0.0; 4];
        map_frame(&src, &mut dst);
        assert_eq!(dst, [0.1, 0.2, 0.2, 0.2]);
    }

    #[test]
    fn resample_state_converts_rate_without_device() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

        // 44.1 kHz -> 48 kHz mono: the non-44.1kHz device branch (Finding 3
        // had no behavioral coverage for it). No real device or audio thread
        // is involved — a synthetic fill closure stands in for the player.
        let src_rate = 44_100u32;
        let device_rate = 48_000u32;
        let ratio = f64::from(device_rate) / f64::from(src_rate);

        // A synthetic 440 Hz sine, generated on demand by the fill closure;
        // also counts the total number of source frames the resampler pulled.
        let consumed = Arc::new(AtomicUsize::new(0));
        let consumed_in_fill = consumed.clone();
        let mut phase: f32 = 0.0;
        let step = 2.0 * std::f32::consts::PI * 440.0 / src_rate as f32;
        let fill: FillCallback = Box::new(move |buf: &mut [f32]| {
            for s in buf.iter_mut() {
                *s = phase.sin();
                phase += step;
            }
            consumed_in_fill.fetch_add(buf.len(), AtomicOrdering::Relaxed);
        });

        let mut state = ResampleState::new(src_rate, device_rate, 1, 1, fill)
            .expect("resampler builds for a real 44.1kHz -> 48kHz ratio");

        // Pull many device-rate buffers of varied sizes (including sizes that
        // don't evenly divide RESAMPLE_OUT_CHUNK) so internal chunk
        // boundaries are crossed repeatedly without panicking.
        let buffer_sizes = [128usize, 512, 1024, 4000, 37, 1, 900];
        let mut total_out_frames: u64 = 0;
        let mut saw_nonsilence = false;

        for _ in 0..40 {
            for &frames in &buffer_sizes {
                let mut data = vec![0.0f32; frames]; // 1 device channel
                state.render(&mut data);
                if data.iter().any(|&s| s.abs() > 1e-4) {
                    saw_nonsilence = true;
                }
                total_out_frames += frames as u64;
            }
        }

        assert!(saw_nonsilence, "resampled output must not be silent");

        // Frame-count sanity: the total source frames the resampler pulled
        // from `fill` should track the device/source ratio, modulo the
        // bounded internal buffering slop (at most ~1 refill chunk running
        // ahead of what render() has drained so far).
        let consumed_frames = consumed.load(AtomicOrdering::Relaxed) as f64;
        let expected_frames = total_out_frames as f64 / ratio;
        let rel_err = (consumed_frames - expected_frames).abs() / expected_frames;
        assert!(
            rel_err < 0.05,
            "consumed {consumed_frames} source frames for {total_out_frames} device \
             frames at ratio {ratio}; expected ~{expected_frames} (rel err {rel_err})"
        );
    }
}
