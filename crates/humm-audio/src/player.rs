//! The varispeed playback [`Player`]: the viewer's A/V master clock.
//!
//! # Model
//!
//! The player owns a whole-file, in-memory [`PlaybackPcm`] (interleaved `f32`)
//! and a source-frame *position* counter. All playback state lives in atomics
//! inside a single [`Shared`] block shared between the UI thread (which calls
//! [`Player`]'s methods) and the audio thread (which pulls the `fill`
//! callback). The `fill` callback reads directly from the `Arc<[f32]>` PCM at
//! the current cursor — no decoding, no I/O, so there is no producer thread or
//! ring buffer between them (that pattern is for I/O-bound producers; whole
//! file in memory is a pure memory read).
//!
//! ## Position is the master clock
//!
//! `position` is a source-frame index (an [`AtomicU64`]). Each `fill` call
//! advances it as it consumes source frames; [`Player::position`] derives
//! milliseconds as `frames * 1000 / sample_rate`. The viewer reads this to
//! drive the scrubber, follow-audio highlight, and — crucially — the video
//! clock (audio is the single master clock, spec §2.5 / ARCHITECTURE §7).
//!
//! ## Varispeed
//!
//! At speed `s`, the cursor advances `s` source frames per output frame, so a
//! second of output consumes `s` seconds of source — pitch shifts with rate,
//! the standard scrubbing trade-off (spec deviations ledger #2; Buzz's
//! sounddevice path had no working speed at all). Speed `1.0` is a distinct
//! *bit-exact copy* fast path with no interpolation. For `s != 1.0` the cursor
//! is fractional and each output frame is a linear interpolation between the
//! two straddling source frames.
//!
//! **Why linear interpolation here, not rubato.** The pinned contract is
//! *sample-exact* frame accounting: after emitting `N` output frames at speed
//! `s`, the position has advanced exactly `N * s` source frames (the
//! `speed_two_consumes_double_source_frames` test pins this to ±1 frame). A
//! priming resampler (rubato's sinc/FFT engines) consumes a *fixed extra*
//! delay-line worth of input on top of `N * s`, which would put the position —
//! and therefore the loop-wrap boundary and the video clock keyed off it — off
//! by that constant, breaking A/V sync and the loop contract. Rubato is used
//! where it is both correct and required: the device-rate negotiation *inside*
//! [`crate::CpalOutput`] (ADR-005), a fixed-ratio resample that never feeds the
//! master clock. See the task-6 report for the full rationale.
//!
//! ## Loop
//!
//! The loop range is two source-frame bounds (two [`AtomicI64`]s, `-1` =
//! disabled). Crossing `end` mid-fill wraps to `start` *within the same fill
//! call*, sample-exactly (spec deviations ledger #3; Buzz loops ~50 ms early
//! off a 16 ms poll). Two independent atomics can be read torn while the UI
//! thread updates them; a torn read yields either the old or new bound for one
//! `~10 ms` buffer and is made harmless by validating `start < end` before
//! using the range (an invalid pair simply disables looping for that one
//! buffer — corrected on the next call). This is cheaper than a mutex and,
//! unlike a mutex, can never block the real-time thread.
//!
//! [`PlaybackPcm`]: humm_core::boundary::PlaybackPcm

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use humm_core::boundary::{AudioOutput, FillCallback, OutputStream, PlaybackPcm, StreamSpec};
use humm_core::domain::Millis;
use humm_core::error::AudioError;

/// Minimum allowed playback speed (spec §2.1, `set_speed` clamp).
const SPEED_MIN: f32 = 0.1;
/// Maximum allowed playback speed.
const SPEED_MAX: f32 = 5.0;

/// Sentinel stored in the loop-bound atomics meaning "no loop set".
const NO_LOOP: i64 = -1;

/// Playback state shared between the UI thread and the audio (`fill`) thread.
///
/// Every field is an atomic (or immutable): no locks are taken on the
/// real-time path.
struct Shared {
    /// The decoded source audio; read-only after construction. Held whole (its
    /// internal `Arc<[f32]>` is never copied — moving the `PlaybackPcm` in is a
    /// cheap `Arc` transfer, not a duplication of the gigabyte-scale buffer).
    pcm: PlaybackPcm,
    /// Interleaved channel count (frames = `samples.len() / channels`).
    channels: usize,
    /// Source sample rate in Hz (used to convert frames ↔ milliseconds).
    sample_rate: u32,
    /// Total source frames.
    total_frames: u64,
    /// Current playback position, as a source-frame index. The master clock.
    position: AtomicU64,
    /// Whether the `fill` callback is consuming frames (vs. emitting silence).
    playing: AtomicBool,
    /// Playback speed as `f32` bits (see [`f32::to_bits`]).
    speed_bits: AtomicU32,
    /// Loop start frame, or [`NO_LOOP`].
    loop_start: AtomicI64,
    /// Loop end frame (exclusive), or [`NO_LOOP`].
    loop_end: AtomicI64,
}

impl Shared {
    fn speed(&self) -> f32 {
        f32::from_bits(self.speed_bits.load(Ordering::Relaxed))
    }

    /// Reads the loop bounds as a validated `(start, end)` frame pair, or
    /// `None` when looping is disabled or the (possibly torn) pair is invalid.
    fn loop_range(&self) -> Option<(u64, u64)> {
        let start = self.loop_start.load(Ordering::Relaxed);
        let end = self.loop_end.load(Ordering::Relaxed);
        if start < 0 || end < 0 {
            return None;
        }
        let (start, end) = (start as u64, end as u64);
        // Guard torn reads and degenerate ranges: an invalid pair disables the
        // loop for this one buffer rather than looping forever or panicking.
        if start < end && start < self.total_frames {
            Some((start, end.min(self.total_frames)))
        } else {
            None
        }
    }

    /// Converts a source-frame index to milliseconds (floored).
    fn frames_to_millis(&self, frames: u64) -> Millis {
        if self.sample_rate == 0 {
            return Millis(0);
        }
        Millis((frames as i64 * 1000) / i64::from(self.sample_rate))
    }

    /// Converts a millisecond position to a source-frame index, clamped to
    /// `[0, total_frames]`.
    fn millis_to_frames(&self, ms: Millis) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        let raw = (ms.0.max(0) as i128 * i128::from(self.sample_rate)) / 1000;
        (raw.max(0) as u64).min(self.total_frames)
    }
}

/// The varispeed audio playback engine (spec §5.2, F-083).
///
/// Owns a decoded [`PlaybackPcm`] and an open [`OutputStream`]. Every method
/// takes `&self` and is callable from the UI thread while the audio thread
/// pulls the `fill` callback; playback state is coordinated through atomics in
/// a shared block. Dropping the player stops the stream.
///
/// The [`OutputStream`] is behind a [`Mutex`] only so play/pause can be `&self`
/// (the trait's control methods need `&mut`); that mutex is taken **only** on
/// the UI-thread control path, never on the real-time `fill` path (which
/// touches only `shared`), so it can never block audio.
pub struct Player {
    shared: Arc<Shared>,
    stream: Mutex<Box<dyn OutputStream>>,
    duration: Millis,
}

impl Player {
    /// Opens `output` at `pcm`'s native rate/channels and builds the playback
    /// engine over `pcm`. The player starts **paused** at position `0`, speed
    /// `1.0`, with no loop. Call [`Player::play`] to begin.
    ///
    /// Opening the output can fail (no device, unbuildable stream); the error
    /// is surfaced so the viewer can degrade to a disabled player (spec §2.1).
    pub fn new(pcm: PlaybackPcm, output: Arc<dyn AudioOutput>) -> Result<Self, AudioError> {
        let channels_u16 = pcm.channels;
        let channels = usize::from(channels_u16);
        let sample_rate = pcm.sample_rate;
        let total_frames = pcm.frames() as u64;
        let duration = pcm.duration();

        let shared = Arc::new(Shared {
            pcm,
            channels,
            sample_rate,
            total_frames,
            position: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            speed_bits: AtomicU32::new(1.0f32.to_bits()),
            loop_start: AtomicI64::new(NO_LOOP),
            loop_end: AtomicI64::new(NO_LOOP),
        });

        let spec = StreamSpec {
            sample_rate,
            channels: channels_u16,
        };
        let fill = make_fill(shared.clone());
        let stream = output.open(spec, fill)?;

        Ok(Player {
            shared,
            stream: Mutex::new(stream),
            duration,
        })
    }

    /// Starts (or resumes) playback from the current position.
    pub fn play(&self) {
        self.shared.playing.store(true, Ordering::Relaxed);
        // Best-effort: a stream-control failure leaves the logical state
        // playing so the mock/test path and any resilient backend still run;
        // real backends that fail here surface it on the next open. A poisoned
        // lock (a panic while another control call held it) is likewise
        // ignored rather than propagated into the UI.
        if let Ok(mut stream) = self.stream.lock() {
            let _ = stream.play();
        }
    }

    /// Pauses playback. Frame consumption stops and the position holds still.
    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        if let Ok(mut stream) = self.stream.lock() {
            let _ = stream.pause();
        }
    }

    /// Toggles between playing and paused.
    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Seeks to `pos`, clamped to `[0, duration]`. Takes effect on the next
    /// `fill` call (the audio thread reconciles its cursor with this atomic).
    pub fn seek(&self, pos: Millis) {
        let frame = self.shared.millis_to_frames(pos);
        self.shared.position.store(frame, Ordering::Relaxed);
    }

    /// Sets the playback speed, clamped to `0.1..=5.0` (spec §2.1). Engages the
    /// varispeed path for any `speed != 1.0`.
    pub fn set_speed(&self, speed: f32) {
        let clamped = if speed.is_nan() {
            1.0
        } else {
            speed.clamp(SPEED_MIN, SPEED_MAX)
        };
        self.shared
            .speed_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    /// Sets or clears the loop range. `Some((start, end))` makes the feeder
    /// wrap `end → start` sample-exactly; `None` clears it. Bounds are clamped
    /// to `[0, duration]`; a range whose start is not strictly before its end
    /// clears the loop.
    pub fn set_loop(&self, range: Option<(Millis, Millis)>) {
        match range {
            Some((start, end)) => {
                let start_f = self.shared.millis_to_frames(start);
                let end_f = self.shared.millis_to_frames(end);
                if start_f < end_f {
                    // Publish end before start so a concurrent `fill` that
                    // observes the new start also observes an end at least as
                    // large (the torn-read guard tolerates the reverse too).
                    self.shared.loop_end.store(end_f as i64, Ordering::Relaxed);
                    self.shared
                        .loop_start
                        .store(start_f as i64, Ordering::Relaxed);
                } else {
                    self.clear_loop_atomics();
                }
            }
            None => self.clear_loop_atomics(),
        }
    }

    fn clear_loop_atomics(&self) {
        self.shared.loop_start.store(NO_LOOP, Ordering::Relaxed);
        self.shared.loop_end.store(NO_LOOP, Ordering::Relaxed);
    }

    /// The current playback position in milliseconds (the master clock).
    pub fn position(&self) -> Millis {
        let frames = self.shared.position.load(Ordering::Relaxed);
        self.shared.frames_to_millis(frames)
    }

    /// The total media duration in milliseconds.
    pub fn duration(&self) -> Millis {
        self.duration
    }

    /// Whether playback is currently consuming frames. Flips to `false` on its
    /// own when playback reaches end-of-media.
    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    /// The current playback speed.
    pub fn speed(&self) -> f32 {
        self.shared.speed()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Stop consuming and stop the underlying stream; dropping `stream`
        // then releases the device.
        self.shared.playing.store(false, Ordering::Relaxed);
        if let Ok(mut stream) = self.stream.lock() {
            let _ = stream.pause();
        }
    }
}

/// Builds the real-time `fill` closure over `shared`.
///
/// The closure owns two pieces of *local* (audio-thread-only) state — a
/// fractional cursor and the last position it published — so it can carry
/// sub-frame precision across calls (the atomic `position` holds only whole
/// frames) and detect a UI-thread seek (the atomic changing out from under it).
/// No heap state is captured, so there is no allocation on any fill path.
fn make_fill(shared: Arc<Shared>) -> FillCallback {
    // Fractional source-frame cursor; the atomic holds only its floored value.
    let mut cursor: f64 = 0.0;
    // Last whole-frame value this closure published to `position`, so a
    // mismatch on entry means the UI thread seeked and we must resync.
    let mut last_published: u64 = 0;

    Box::new(move |out: &mut [f32]| {
        let ch = shared.channels;
        if ch == 0 || shared.total_frames == 0 {
            out.fill(0.0);
            shared.playing.store(false, Ordering::Relaxed);
            return;
        }

        // Resync the local cursor if the UI thread moved the position (seek).
        let published = shared.position.load(Ordering::Relaxed);
        if published != last_published {
            cursor = published as f64;
        }

        if !shared.playing.load(Ordering::Relaxed) {
            // Paused: emit silence and do not advance. Position holds still.
            out.fill(0.0);
            return;
        }

        let speed = f64::from(shared.speed()).max(f64::from(SPEED_MIN));
        let loop_range = shared.loop_range();
        let total = shared.total_frames;
        let frame_count = out.len() / ch;
        let exact_copy = (speed - 1.0).abs() < f64::EPSILON;
        // Immutable borrow of the shared, read-only sample buffer.
        let samples = shared.pcm.samples();

        for i in 0..frame_count {
            // Apply the loop wrap *before* reading, so crossing `end` mid-fill
            // resumes at `start` within this same call, sample-exactly.
            if let Some((lo, hi)) = loop_range
                && cursor >= hi as f64
            {
                let len = (hi - lo) as f64;
                cursor = lo as f64 + (cursor - lo as f64).rem_euclid(len);
            }

            let base = cursor as u64;
            if base >= total {
                // End of media: zero the rest, stop, and pin to the end.
                for s in &mut out[i * ch..] {
                    *s = 0.0;
                }
                cursor = total as f64;
                shared.playing.store(false, Ordering::Relaxed);
                break;
            }

            let frame = &mut out[i * ch..i * ch + ch];
            if exact_copy {
                let src = base as usize * ch;
                frame.copy_from_slice(&samples[src..src + ch]);
            } else {
                // Linear interpolation between `base` and `base + 1` (the next
                // frame is clamped to the last frame at the tail).
                let frac = (cursor - base as f64) as f32;
                let next = (base + 1).min(total - 1) as usize;
                let a = base as usize * ch;
                let b = next * ch;
                for (c, s) in frame.iter_mut().enumerate() {
                    let s0 = samples[a + c];
                    let s1 = samples[b + c];
                    *s = s0 + (s1 - s0) * frac;
                }
            }

            cursor += speed;
        }

        // Publish the whole-frame position for the UI thread.
        let published = (cursor as u64).min(total);
        shared.position.store(published, Ordering::Relaxed);
        last_published = published;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The GUI holds each viewer's [`Player`] inside an `Arc<Mutex<ViewerVm>>`,
    /// which requires `Player: Send`. This fails to compile if that regresses.
    const _: fn() = || {
        fn assert_send<T: Send>() {}
        assert_send::<Player>();
    };

    /// A mock [`AudioOutput`] that captures the `fill` closure so the test can
    /// pull buffers synchronously — no real audio thread, a simulated clock.
    struct MockOutput {
        captured: Arc<Mutex<Option<FillCallback>>>,
    }

    struct MockStream;
    impl OutputStream for MockStream {
        fn play(&mut self) -> Result<(), AudioError> {
            Ok(())
        }
        fn pause(&mut self) -> Result<(), AudioError> {
            Ok(())
        }
    }

    impl AudioOutput for MockOutput {
        fn open(
            &self,
            _spec: StreamSpec,
            fill: FillCallback,
        ) -> Result<Box<dyn OutputStream>, AudioError> {
            *self.captured.lock().unwrap() = Some(fill);
            Ok(Box::new(MockStream))
        }
    }

    /// A test harness holding the player plus the captured fill closure.
    struct Harness {
        player: Player,
        fill: FillCallback,
        channels: usize,
    }

    impl Harness {
        /// Builds a player over a ramp PCM (`samples[i] = i as f32`, per
        /// interleaved sample) at `sample_rate = 1000` so 1 frame = 1 ms.
        fn ramp(frames: usize, channels: u16) -> Harness {
            let ch = usize::from(channels);
            let samples: Vec<f32> = (0..frames * ch).map(|i| i as f32).collect();
            let pcm = PlaybackPcm::new(samples, channels, 1000);
            let captured = Arc::new(Mutex::new(None));
            let output = MockOutput {
                captured: captured.clone(),
            };
            let player = Player::new(pcm, Arc::new(output)).unwrap();
            let fill = captured.lock().unwrap().take().expect("fill captured");
            Harness {
                player,
                fill,
                channels: ch,
            }
        }

        /// Pulls `frames` output frames and returns the interleaved buffer.
        fn pull(&mut self, frames: usize) -> Vec<f32> {
            let mut buf = vec![-1.0f32; frames * self.channels];
            (self.fill)(&mut buf);
            buf
        }
    }

    #[test]
    fn position_advances_with_frames_consumed() {
        let mut h = Harness::ramp(500, 1);
        h.player.play();

        h.pull(100);
        assert_eq!(h.player.position(), Millis(100));

        h.pull(100);
        assert_eq!(h.player.position(), Millis(200));
    }

    #[test]
    fn seek_moves_position_and_playback_origin() {
        let mut h = Harness::ramp(500, 1);
        h.player.seek(Millis(300));
        assert_eq!(h.player.position(), Millis(300));

        h.player.play();
        let out = h.pull(50);
        assert_eq!(h.player.position(), Millis(350));
        // Playback origin moved: the first frame read is source frame 300.
        assert_eq!(out[0], 300.0);
        assert_eq!(out[49], 349.0);
    }

    #[test]
    fn loop_wraps_exactly_at_end_boundary() {
        let mut h = Harness::ramp(500, 1);
        h.player.set_loop(Some((Millis(100), Millis(200))));
        h.player.seek(Millis(190));
        h.player.play();

        // Pull 20 frames in one call: 190..200 then wrap to 100..110.
        let out = h.pull(20);
        assert_eq!(h.player.position(), Millis(110));

        let expected: Vec<f32> = (190..200).chain(100..110).map(|i| i as f32).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn speed_two_consumes_double_source_frames() {
        let mut h = Harness::ramp(2000, 1);
        h.player.set_speed(2.0);
        h.player.play();

        h.pull(100);
        // 100 output frames at speed 2.0 → ~200 source frames consumed.
        assert!(
            (h.player.position().0 - 200).abs() <= 1,
            "position was {:?}",
            h.player.position()
        );
    }

    #[test]
    fn speed_clamped_to_bounds() {
        let h = Harness::ramp(10, 1);
        h.player.set_speed(0.01);
        assert_eq!(h.player.speed(), 0.1);

        h.player.set_speed(9.0);
        assert_eq!(h.player.speed(), 5.0);

        h.player.set_speed(2.5);
        assert_eq!(h.player.speed(), 2.5);
    }

    #[test]
    fn pause_stops_frame_consumption() {
        let mut h = Harness::ramp(500, 1);
        h.player.play();
        h.pull(50);
        assert_eq!(h.player.position(), Millis(50));

        h.player.pause();
        let out = h.pull(50);
        // Position held; buffer is silence.
        assert_eq!(h.player.position(), Millis(50));
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn end_of_media_stops_and_pins_position() {
        let mut h = Harness::ramp(100, 1);
        h.player.play();

        // Ask for more frames than exist.
        let out = h.pull(150);
        assert_eq!(h.player.position(), h.player.duration());
        assert_eq!(h.player.position(), Millis(100));
        assert!(!h.player.is_playing());
        // First 100 are real samples; the tail is silence.
        assert_eq!(out[99], 99.0);
        assert!(out[100..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn set_loop_none_clears_wrap() {
        let mut h = Harness::ramp(500, 1);
        h.player.set_loop(Some((Millis(100), Millis(200))));
        h.player.set_loop(None);
        h.player.seek(Millis(190));
        h.player.play();

        // With the loop cleared, playback runs straight through 200.
        let out = h.pull(20);
        assert_eq!(h.player.position(), Millis(210));
        let expected: Vec<f32> = (190..210).map(|i| i as f32).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn stereo_exact_copy_preserves_interleaving() {
        let mut h = Harness::ramp(100, 2);
        h.player.play();
        let out = h.pull(10);
        // 10 stereo frames = 20 interleaved samples, a straight ramp copy.
        let expected: Vec<f32> = (0..20).map(|i| i as f32).collect();
        assert_eq!(out, expected);
        assert_eq!(h.player.position(), Millis(10));
    }

    #[test]
    fn toggle_flips_playing_state() {
        let h = Harness::ramp(100, 1);
        assert!(!h.player.is_playing());
        h.player.toggle();
        assert!(h.player.is_playing());
        h.player.toggle();
        assert!(!h.player.is_playing());
    }
}
