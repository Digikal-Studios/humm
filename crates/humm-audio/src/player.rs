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
//! off a 16 ms poll). `set_loop` writes the two bounds as independent
//! `Relaxed` stores; the C++/Rust memory model gives **no ordering
//! guarantee** between them as observed by another thread — which store a
//! concurrent `fill` sees first is unspecified, so the write order does not
//! by itself make one observation more likely than the other. Correctness
//! does not depend on any such ordering: `fill` reads both atomics once per
//! buffer and validates `start < end` (and `start < total_frames`) before
//! trusting the pair, so *every* torn combination (old/old, old/new, new/old,
//! new/new) either yields a valid range or is rejected and simply disables
//! looping for that one `~10 ms` buffer, self-correcting on the next call.
//! This is cheaper than a mutex and, unlike a mutex, can never block the
//! real-time thread.
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

/// Bits of the packed position atom given to the frame counter (the
/// remaining high bits are a wrapping generation). 2^40 frames is roughly
/// 800 years of audio at 44.1 kHz — far beyond any real recording — so the
/// truncation `pack`'s `debug_assert` guards against is unreachable in
/// practice; it exists to fail loudly in tests/debug builds if that ever
/// stops being true, at zero cost in release builds.
const FRAME_BITS: u32 = 40;
/// Mask isolating the low `FRAME_BITS` bits of a packed value.
const FRAME_MASK: u64 = (1u64 << FRAME_BITS) - 1;
/// Mask isolating the (already shifted-down) generation's low 24 bits.
const GEN_MASK: u64 = (1u64 << (u64::BITS - FRAME_BITS)) - 1;

/// Packs a `{generation, frame}` pair into the single `u64` published by
/// [`Shared::packed`]. See that field's docs for why folding both into one
/// atomic — rather than the two independent atomics used before — is what
/// actually closes the seek-loss race: a single word has no "half written"
/// state for a concurrent reader to observe.
fn pack(generation: u64, frame: u64) -> u64 {
    debug_assert!(
        frame <= FRAME_MASK,
        "frame {frame} does not fit the packed position's {FRAME_BITS}-bit field"
    );
    ((generation & GEN_MASK) << FRAME_BITS) | (frame & FRAME_MASK)
}

/// Unpacks a `(generation, frame)` pair from a value produced by [`pack`].
fn unpack(packed: u64) -> (u64, u64) {
    (packed >> FRAME_BITS, packed & FRAME_MASK)
}

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
    /// Packed `{generation: 24 bits, frame: 40 bits}` current-position atom
    /// (see [`pack`]/[`unpack`]) — the *sole* publication point for playback
    /// position, shared between the UI thread ([`Player::seek`], via
    /// `seek_to`) and the audio thread (the `fill` closure, via
    /// `try_commit`). The master clock is `unpack(packed).1`.
    ///
    /// This replaces an earlier design with two independent atomics
    /// (`position` + `seek_generation`, round 1's fix for the original
    /// unconditional-overwrite bug). That design still had a residual gap:
    /// checking the generation and storing the position were two *separate*
    /// atomic operations, so a `seek()` landing in the nanosecond window
    /// between the fill's generation check and its position store was
    /// invisible to that store and got silently overwritten anyway —
    /// closing the first hole reopened a narrower one right next to it.
    ///
    /// Packing both into one atomic and publishing exclusively through a
    /// single `compare_exchange` (`try_commit`) removes the gap entirely:
    /// the read (what the fill expects to still be true), the compute (the
    /// new frame), and the write (publish it) are bridged by one hardware
    /// CAS whose compare-and-swap is atomic as a unit. A concurrent
    /// `seek_to`'s single store either lands strictly before that CAS
    /// executes (the CAS sees it, fails, and the fill drops its stale
    /// commit) or strictly after (the CAS already succeeded with the old
    /// generation, and the seek's store — which always carries a *newer*
    /// generation — is simply the next value in the atomic's modification
    /// order and is never overwritten by anything). There is no third case:
    /// a single atomic word is never observed "half updated".
    packed: AtomicU64,
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

    /// Reads the current position frame out of the packed atomic.
    fn position_frame(&self) -> u64 {
        unpack(self.packed.load(Ordering::Relaxed)).1
    }

    /// UI-thread seek: publishes a new `{generation, frame}` pair with a
    /// single store. This is a plain load-increment-store rather than a
    /// CAS-retry loop because `Player::seek` is only ever called from the UI
    /// thread — a fetch-then-store race between two concurrent UI-thread
    /// seeks is out of scope (the GUI issues seeks from one thread; see the
    /// task-6-report fix-round-2 notes). The only other writer of `packed`
    /// is the audio thread's `try_commit`, and that writer is a CAS
    /// designed to lose to this store (its expected value goes stale the
    /// instant this store lands), so this store can never itself be lost or
    /// torn against that side.
    fn seek_to(&self, frame: u64) {
        let current_gen = unpack(self.packed.load(Ordering::Relaxed)).0;
        let next_gen = current_gen.wrapping_add(1) & GEN_MASK;
        self.packed.store(pack(next_gen, frame), Ordering::Relaxed);
    }

    /// Attempts to publish `computed_frame` as the new position, gated on
    /// nothing else having touched `packed` since the caller read
    /// `entry_packed`.
    ///
    /// `entry_packed` must be the exact packed value (generation *and*
    /// frame together) the `fill` closure read at the start of this buffer.
    /// The `compare_exchange` here expects that exact value: if it still
    /// holds, no `seek_to` landed anywhere in the read-compute-write span
    /// and the new frame is published atomically at the same generation. If
    /// it does not hold, a `seek_to` raced in somewhere in that span — the
    /// two-atomic design's residual bug was that this window used to be
    /// covered by *two* separate checks (a generation load, then later an
    /// unconditional position store), leaving a gap between them; here
    /// there is exactly one atomic operation covering the whole span, so
    /// there is no gap to land in. On failure, `seek_to`'s own store has
    /// already published the value that must win, so this stale commit is
    /// simply dropped rather than clobbering it. The caller does not need
    /// to re-read `packed` on failure: it can leave its remembered value
    /// stale, and its *next* call's entry check will notice the mismatch
    /// and resync from whatever is now published.
    fn try_commit(&self, entry_packed: u64, computed_frame: u64) -> bool {
        let (entry_gen, _) = unpack(entry_packed);
        let new_packed = pack(entry_gen, computed_frame.min(self.total_frames));
        self.packed
            .compare_exchange(
                entry_packed,
                new_packed,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
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
            packed: AtomicU64::new(pack(0, 0)),
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

    /// Seeks to `pos`, clamped to `[0, duration]`. Publishes the new
    /// position with a single atomic store (`Shared::seek_to`), packing in a
    /// bumped generation alongside it. A `fill` buffer already in flight
    /// detects this via its end-of-buffer `compare_exchange`
    /// (`Shared::try_commit`) failing, and drops its own stale commit
    /// instead of overwriting this target — losing the seek is impossible
    /// by construction, since the position is published through exactly one
    /// atomic word and exactly one CAS (see the `packed` field docs).
    pub fn seek(&self, pos: Millis) {
        let frame = self.shared.millis_to_frames(pos);
        self.shared.seek_to(frame);
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
                    // The store order between these two `Relaxed` atomics
                    // carries no observation guarantee for a concurrent
                    // `fill` — two independent `Relaxed` atomics have no
                    // relative ordering visible to a third thread under the
                    // C++/Rust memory model, regardless of which is written
                    // first here. Correctness instead comes from
                    // `Shared::loop_range`'s validation, which tolerates
                    // every torn combination of old/new start and end: an
                    // invalid pair just disables looping for one buffer,
                    // corrected on the next call.
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
        let frames = self.shared.position_frame();
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
/// fractional cursor and the packed `{generation, frame}` value it last
/// published or observed — so it can carry sub-frame precision across calls
/// (the atomic only ever holds a whole-frame position) and detect a
/// UI-thread seek. Each buffer touches `shared.packed` exactly twice: one
/// `load` at entry (to resync `cursor` if a seek landed before this call
/// started) and one `compare_exchange` at exit (`Shared::try_commit`) that
/// publishes the computed cursor *only if* nothing else wrote `packed` in
/// between. Because both the check and the publish live inside that single
/// CAS, a seek landing anywhere in the buffer — including the exact
/// nanosecond gap between a *separate* generation-check and position-store
/// that the two-atomic design (fix round 1) left open — cannot be silently
/// lost: it either lands before the CAS (which then fails, and this
/// closure's stale commit is dropped) or after it (which then simply
/// becomes the next value, never overwritten). The closure notices a
/// dropped commit on its *next* call, because its remembered
/// `last_seen_packed` will then disagree with what it reads. No heap state
/// is captured, so there is no allocation on any fill path.
fn make_fill(shared: Arc<Shared>) -> FillCallback {
    // Fractional source-frame cursor; the atomic holds only its floored value.
    let mut cursor: f64 = 0.0;
    // The packed value this closure last published (via `try_commit`) or
    // resynced from (at entry). A mismatch against a fresh
    // `shared.packed.load` means a seek landed since we last checked.
    let mut last_seen_packed: u64 = pack(0, 0);

    Box::new(move |out: &mut [f32]| {
        let ch = shared.channels;
        if ch == 0 || shared.total_frames == 0 {
            out.fill(0.0);
            shared.playing.store(false, Ordering::Relaxed);
            return;
        }

        // Resync the local cursor if the UI thread seeked before this call
        // started.
        let entry_packed = shared.packed.load(Ordering::Relaxed);
        if entry_packed != last_seen_packed {
            cursor = unpack(entry_packed).1 as f64;
            last_seen_packed = entry_packed;
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

        // Publish the computed cursor via a single compare-exchange against
        // `entry_packed` — the exact value read above. If it still matches,
        // nothing raced during this buffer and the new frame is published
        // atomically; if not, a `seek()` landed somewhere in this call and
        // already published its own target via its own store, so this stale
        // commit is dropped rather than clobbering it (`Shared::try_commit`).
        // On a dropped commit, `last_seen_packed` is deliberately left as
        // `entry_packed`, which the *next* call's entry check will find
        // stale against the current atomic and resync `cursor` from.
        let computed_frame = (cursor as u64).min(total);
        if shared.try_commit(entry_packed, computed_frame) {
            last_seen_packed = pack(unpack(entry_packed).0, computed_frame);
            cursor = computed_frame as f64;
        }
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
    fn seek_during_fill_is_not_lost() {
        // Fix round 1: a `seek()` landing between the fill's entry check and
        // its end-of-buffer position publish must not be silently clobbered
        // by that publish. Fix round 2 replaced the two-atomic
        // (position + seek_generation) scheme with one packed atomic
        // published exclusively through `Shared::try_commit`'s CAS; this
        // test keeps the original scenario green under that scheme and
        // still demonstrates, by contrast, the bug shape an unconditional
        // publish has. Both shapes are driven manually
        // (read-entry -> interleaved seek -> end-publish) so the
        // interleaving is deterministic rather than depending on real
        // thread timing.
        let total: u64 = 1000;

        // --- Contrast: an unconditional publish loses the seek. ---
        {
            let h = Harness::ramp(total as usize, 1);
            h.player.play();
            let shared = &h.player.shared;

            // 1. read-entry.
            let entry_packed = shared.packed.load(Ordering::Relaxed);
            // Playback computed cursor 250 from the frames it consumed this
            // buffer, before the seek below landed.
            let stale_computed_cursor: u64 = 250;

            // 2. interleave: a UI-thread seek lands mid-buffer.
            h.player.seek(Millis(700));
            assert_eq!(h.player.position(), Millis(700));

            // 3. end-publish, UNCONDITIONAL shape (what the pre-fix-round-1
            //    code did, and what a naive port to a single packed atomic
            //    without the CAS guard would still do): overwrite `packed`
            //    outright instead of gating it on `entry_packed` still
            //    being current. This is exactly the clobber `try_commit`
            //    exists to prevent.
            shared.packed.store(
                pack(unpack(entry_packed).0, stale_computed_cursor),
                Ordering::Relaxed,
            );
            assert_eq!(
                h.player.position(),
                Millis(250),
                "reproduces the bug shape: an unconditional publish silently \
                 loses the interleaved seek"
            );
        }

        // --- Production shape: `try_commit`'s CAS honors the newer seek. ---
        let mut h = Harness::ramp(total as usize, 1);
        h.player.play();

        // 1. read-entry.
        let entry_packed = h.player.shared.packed.load(Ordering::Relaxed);
        let stale_computed_cursor: u64 = 250;

        // 2. interleave: seek lands after entry was captured but before the
        //    fill's commit runs.
        h.player.seek(Millis(700));

        // 3. commit, production shape: the CAS is gated on `entry_packed`
        //    and fails, so the seek's target is left untouched instead of
        //    overwritten.
        let committed = h
            .player
            .shared
            .try_commit(entry_packed, stale_computed_cursor);
        assert!(
            !committed,
            "the commit must be rejected once a seek has raced in"
        );
        assert_eq!(
            h.player.position(),
            Millis(700),
            "the interleaved seek survives the fill's dropped commit"
        );

        // The real fill path converges the same way end-to-end: the next
        // buffer resumes from the seek target, not the pre-seek trajectory.
        let out = h.pull(10);
        assert_eq!(out[0], 700.0, "fill resumes playback from the seek target");
        assert_eq!(h.player.position(), Millis(710));
    }

    #[test]
    fn seek_between_generation_check_and_commit_wins() {
        // Reviewer's residual-TOCTOU repro: under the fix-round-1 scheme
        // (an independent `position` atomic and `seek_generation` atomic),
        // a seek landing strictly between the fill's generation *check* and
        // its position *store* was invisible to that store — the two ops
        // were separate atomics with no combined atomicity, so the window
        // between them, though nanoseconds wide, was real and the loss was
        // silent and permanent (the fill's own store also updated its
        // notion of "last published", so a later resync never recovered
        // the lost target either).
        //
        // The packed single-atomic scheme closes this by construction:
        // there is exactly one publication point on the fill side
        // (`Shared::try_commit`'s `compare_exchange`), and a CAS is a
        // single indivisible hardware operation — there is no "check" step
        // and "store" step for anything to land between. This test drives
        // the exact interleaving that broke the old scheme
        // (read-entry -> interleaved seek -> attempted commit)
        // deterministically and asserts the seek survives, then that the
        // very next fill call resyncs from it.
        let total: u64 = 1000;
        let mut h = Harness::ramp(total as usize, 1);
        h.player.play();
        let shared = &h.player.shared;

        // 1. `fill` reads the packed value at entry — the single load that
        //    stands in for what used to be a *separate* generation check.
        let entry_packed = shared.packed.load(Ordering::Relaxed);
        assert_eq!(unpack(entry_packed), (0, 0));

        // Playback computed cursor 250 from the frames it consumed this
        // buffer -- before the seek below lands.
        let stale_computed_cursor: u64 = 250;

        // 2. Interleave: a UI-thread seek lands strictly between the fill's
        //    entry read and its attempted commit — exactly the gap that
        //    used to sit between the old scheme's two separate atomics.
        h.player.seek(Millis(700));
        assert_eq!(h.player.position(), Millis(700));
        let after_seek_packed = shared.packed.load(Ordering::Relaxed);
        assert_ne!(
            after_seek_packed, entry_packed,
            "the seek must have published a new packed value"
        );

        // 3. `fill` attempts its commit against the *stale* entry value it
        //    captured in step 1. The CAS must fail, and must fail
        //    atomically: there is no intermediate state where a partial
        //    write could be observed by another thread.
        let committed = shared.try_commit(entry_packed, stale_computed_cursor);
        assert!(
            !committed,
            "commit must be rejected once a seek has raced in between the \
             entry read and the commit attempt"
        );

        // The seek's target must still be exactly what's published — the
        // failed CAS must not have perturbed it at all.
        assert_eq!(
            h.player.position(),
            Millis(700),
            "the interleaved seek survives the fill's dropped commit"
        );
        assert_eq!(shared.packed.load(Ordering::Relaxed), after_seek_packed);

        // 4. The next real fill call resyncs from the packed value it now
        //    observes (the seek's target), exactly as `make_fill` does when
        //    its remembered `last_seen_packed` no longer matches.
        let out = h.pull(10);
        assert_eq!(out[0], 700.0, "fill resumes playback from the seek target");
        assert_eq!(h.player.position(), Millis(710));
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
