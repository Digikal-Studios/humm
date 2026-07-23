//! The realtime sample-accumulation / silence-cut / callback-drop state
//! machine, ported verbatim from Buzz's `RecordingTranscriber`
//! (`buzz/transcriber/recording_transcriber.py`):
//!
//! - Batch sizing: lines 63-71 (`__init__`).
//! - Loop / cut / advance: lines 108-134 (`start`, the `while self.is_running`
//!   body).
//! - Callback drop policy: lines 388-397 (`stream_callback`).
//! - `find_silence_cut_point` + `amplitude`: lines 399-433.
//!
//! Pure: samples in, [`Batch`]es out. No threads, no audio APIs, no I/O --
//! `live::session` (a later task) owns the thread, the engine call, and the
//! silence-threshold skip decision (this module reports RMS; it does not
//! gate on it, matching Buzz's split between the loop and
//! `transcription_options.silence_threshold`).

use crate::boundary::SAMPLE_RATE;
use crate::domain::Millis;
use crate::live::RecordingMode;

/// `find_silence_cut_point`'s search window, in seconds
/// (`recording_transcriber.py:401`, `search_seconds` default).
const SEARCH_SECONDS: f64 = 1.5;
/// `find_silence_cut_point`'s RMS window, in seconds
/// (`recording_transcriber.py:401`, `window_seconds` default).
const WINDOW_SECONDS: f64 = 0.02;
/// `find_silence_cut_point`'s quiet-window threshold ratio, relative to the
/// search region's mean window RMS (`recording_transcriber.py:402`,
/// `silence_ratio` default).
const SILENCE_RATIO: f64 = 0.5;

/// One cut-ready slice of accumulated audio, ready to transcribe.
///
/// Emitted by [`Chunker::next_batch`]. `start_ms`/`end_ms` track this
/// crate's own stream-position timeline (`docs/OPEN_QUESTIONS.md` Q-10 --
/// Buzz persists nothing, so there is no Buzz field to port this against):
/// see [`Chunker::next_batch`]'s doc comment for the non-double-counting
/// rule that governs them across the append-and-correct keep-back overlap.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    /// The audio to transcribe: `queue[..cut]` at the moment this batch was
    /// cut (Buzz's `samples`, `recording_transcriber.py:114`).
    pub samples: Vec<f32>,
    /// Start of this batch on the absolute recording timeline.
    pub start_ms: Millis,
    /// End of this batch on the absolute recording timeline: always
    /// `start_ms + duration(cut samples)`, independent of keep-back overlap
    /// (see [`Chunker::next_batch`]).
    pub end_ms: Millis,
    /// RMS of `samples` (Buzz's `amplitude`, computed over the *cut* region,
    /// not the full `n_batch_samples` search window --
    /// `recording_transcriber.py:121`). The session loop (a later task)
    /// compares this against `silence_threshold` to decide whether to skip
    /// transcribing; this module does not gate on it.
    pub rms: f64,
    /// Remaining buffered sample count after this batch was cut and the
    /// queue advanced (Buzz's `queue_size_changed` payload,
    /// `recording_transcriber.py:123`).
    pub queue_len_after: usize,
}

/// The sample-accumulation queue plus the batch/silence-cut/drop state
/// machine. Owns no audio device, no thread -- samples arrive via
/// [`Self::push`], cut-ready batches leave via [`Self::next_batch`].
pub struct Chunker {
    mode: RecordingMode,
    n_batch_samples: usize,
    keep_samples: usize,
    max_queue_size: usize,
    queue: Vec<f32>,
    /// Absolute stream position, in samples, of the next batch's `start_ms`
    /// marker. Advances by exactly `cut` every batch regardless of mode --
    /// see [`Self::next_batch`].
    position_samples: u64,
}

impl Chunker {
    /// Builds a chunker for `mode`, sized per Buzz's `__init__`
    /// (`recording_transcriber.py:63-71`):
    ///
    /// - `AppendBelow`/`AppendAbove`: `n_batch_samples = 5s * SAMPLE_RATE`,
    ///   `keep_samples = 0.15s * SAMPLE_RATE`. `transcription_step` is
    ///   ignored (Buzz only reads it in append-and-correct mode).
    /// - `AppendAndCorrect`: `n_batch_samples = transcription_step *
    ///   SAMPLE_RATE`, `keep_samples = 1.5s * SAMPLE_RATE`.
    ///
    /// `max_queue_size = 3 * n_batch_samples` in every mode (Buzz: "pause
    /// queueing if more than 3 batches behind").
    pub fn new(mode: RecordingMode, transcription_step: f64) -> Self {
        let rate = f64::from(SAMPLE_RATE);
        let (n_batch_samples, keep_sample_seconds) = if mode == RecordingMode::AppendAndCorrect {
            ((transcription_step * rate) as usize, 1.5)
        } else {
            ((5.0 * rate) as usize, 0.15)
        };
        let keep_samples = (keep_sample_seconds * rate) as usize;
        let max_queue_size = 3 * n_batch_samples;

        Chunker {
            mode,
            n_batch_samples,
            keep_samples,
            max_queue_size,
            queue: Vec::new(),
            position_samples: 0,
        }
    }

    /// `n_batch_samples` this chunker was sized with (a batch is cut once
    /// the queue holds at least this many samples).
    pub fn n_batch_samples(&self) -> usize {
        self.n_batch_samples
    }

    /// `keep_samples` this chunker was sized with (only consumed by
    /// [`Self::next_batch`] in [`RecordingMode::AppendAndCorrect`]).
    pub fn keep_samples(&self) -> usize {
        self.keep_samples
    }

    /// `max_queue_size` this chunker was sized with ([`Self::push`] drops
    /// incoming samples once the queue reaches this size).
    pub fn max_queue_size(&self) -> usize {
        self.max_queue_size
    }

    /// Currently buffered sample count.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Appends `samples` to the accumulation queue, or drops them entirely,
    /// Buzz callback-drop parity (`stream_callback`,
    /// `recording_transcriber.py:388-397`): `if self.queue.size <
    /// self.max_queue_size: self.queue = np.append(self.queue, chunk)`.
    ///
    /// The accept/drop decision is made once, against the queue size
    /// *before* this call -- a single push can carry the queue past
    /// `max_queue_size` (matching Buzz, whose callback appends the whole
    /// incoming block unconditionally once admitted); only a push that
    /// starts with the queue already at or above `max_queue_size` is
    /// dropped. Returns the number of samples accepted: `samples.len()` or
    /// `0`.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        if self.queue.len() >= self.max_queue_size {
            return 0;
        }
        self.queue.extend_from_slice(samples);
        samples.len()
    }

    /// Cuts and returns the next ready batch, or `None` if fewer than
    /// `n_batch_samples` are buffered (Buzz: `if self.queue.size >=
    /// self.n_batch_samples: ... else: time.sleep(0.5)` --
    /// `recording_transcriber.py:109-133`; the 500ms idle pacing is a
    /// session-thread concern, not this pure function's).
    ///
    /// Cut/advance/timeline rule:
    /// 1. `cut = find_silence_cut_point(queue[..n_batch_samples], rate)`.
    /// 2. `samples = queue[..cut]` becomes the batch.
    /// 3. The **queue** (this chunker's internal buffer) drains by `cut` in
    ///    `AppendBelow`/`AppendAbove`, or by `cut - keep_samples` in
    ///    `AppendAndCorrect` (Buzz: `self.queue = self.queue[cut -
    ///    keep_samples:]` -- the keep-back overlap later tasks feed to the
    ///    append-and-correct text merge). `cut < keep_samples` is clamped to
    ///    a zero drain rather than underflowing (Buzz's own Python would
    ///    slice with a negative index here, reading from the *end* of
    ///    whatever the queue happens to hold at that instant -- a
    ///    timing-dependent, undocumented corner even in Buzz; this chunker
    ///    defines it as "keep everything" instead of replicating that
    ///    ambiguity).
    /// 4. The **timeline** (`start_ms`/`end_ms`) advances by `cut` in every
    ///    mode, never by `cut - keep_samples`: batch *N*'s `[start_ms,
    ///    end_ms)` covers exactly `cut` samples' worth of time, and batch
    ///    *N+1*'s `start_ms` equals batch *N*'s `end_ms` -- even though, in
    ///    `AppendAndCorrect` mode, batch *N+1*'s `samples` physically
    ///    re-include the `keep_samples`-long tail of batch *N*. This is
    ///    deliberate: the keep-back overlap exists to feed the text-level
    ///    merge (`find_common_part`, a later task), which trims the
    ///    duplicated *text* back out; double-counting that overlap on the
    ///    *timeline* as well (used for `docs/OPEN_QUESTIONS.md` Q-10's
    ///    persisted-segment timeline) would make adjacent stored segments
    ///    overlap in time, which nothing downstream expects.
    pub fn next_batch(&mut self) -> Option<Batch> {
        if self.queue.len() < self.n_batch_samples {
            return None;
        }

        let cut = find_silence_cut_point(&self.queue[..self.n_batch_samples], SAMPLE_RATE);
        let samples = self.queue[..cut].to_vec();

        let drained = if self.mode == RecordingMode::AppendAndCorrect {
            cut.saturating_sub(self.keep_samples)
        } else {
            cut
        };
        self.queue.drain(..drained);

        let batch_rms = rms(&samples);
        let start_ms = millis_from_samples(self.position_samples);
        self.position_samples += cut as u64;
        let end_ms = millis_from_samples(self.position_samples);

        Some(Batch {
            samples,
            start_ms,
            end_ms,
            rms: batch_rms,
            queue_len_after: self.queue.len(),
        })
    }
}

/// `Millis` implied by `n` samples at [`SAMPLE_RATE`], floored to the
/// nearest millisecond (same formula as `boundary::Pcm::duration`).
fn millis_from_samples(n: u64) -> Millis {
    Millis((n as i64 * 1000) / i64::from(SAMPLE_RATE))
}

/// Ported verbatim from Buzz's `RecordingTranscriber.find_silence_cut_point`
/// static method (`recording_transcriber.py:399-429`). Returns the index of
/// the last quiet point in `samples`: scans `0.02s`-wide RMS windows
/// backward through the final `1.5s` of `samples`, returning the midpoint of
/// the rightmost window whose RMS is below `0.5 * mean(window RMS)` over
/// that search region. Falls back to `samples.len()` (Buzz: "no quiet window
/// found", or the search region has fewer than two full windows) when no
/// such window exists.
///
/// Window/search-region arithmetic matches Buzz's integer truncation
/// (`int(seconds * sample_rate)`, `(len(region) - window) // window`)
/// exactly; RMS is computed in `f64` from the `f32` input (see
/// `validation/m5-differential/gen_chunker_fixtures.py`'s numeric-parity
/// note -- Buzz's own `np.mean`/`np.sqrt` on a `float32` array stay
/// `float32`-precision internally, a ~1e-7-relative difference from `f64`
/// that is immaterial to which window is selected given the threshold's 2x
/// margin, and which the differential fixture harness reproduces by
/// generating its reference values in `float64` throughout).
pub fn find_silence_cut_point(samples: &[f32], sample_rate: u32) -> usize {
    let rate = f64::from(sample_rate);
    let window = (WINDOW_SECONDS * rate) as usize;
    let search_span = (SEARCH_SECONDS * rate) as i64;
    let search_start = ((samples.len() as i64) - search_span).max(0) as usize;
    let region = &samples[search_start..];

    if window == 0 || region.len() < window {
        return samples.len();
    }
    let n_windows = (region.len() - window) / window;
    if n_windows < 1 {
        return samples.len();
    }

    let energies: Vec<f64> = (0..n_windows)
        .map(|i| rms(&region[i * window..(i + 1) * window]))
        .collect();
    let mean_energy: f64 = energies.iter().sum::<f64>() / n_windows as f64;
    let threshold = SILENCE_RATIO * mean_energy;

    for i in (0..n_windows).rev() {
        if energies[i] < threshold {
            return search_start + i * window + window / 2;
        }
    }
    samples.len()
}

/// Ported verbatim from Buzz's `RecordingTranscriber.amplitude` static
/// method (`recording_transcriber.py:431-433`): `sqrt(mean(arr**2))`, in
/// `f64` from `f32` input (see [`find_silence_cut_point`]'s doc comment).
/// `0.0` for an empty slice (Buzz's `np.mean([])` produces `NaN` with a
/// runtime warning; this chunker never calls `rms` on an empty batch in
/// practice -- `next_batch`'s cut is always `> 0` -- but a defined,
/// non-NaN answer is a safer contract than silently propagating one).
pub fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum_sq / samples.len() as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn approx_eq(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{a} !~= {b} (eps {eps})");
    }

    // -- Chunker::new batch sizing per mode -------------------------------

    #[test]
    fn new_computes_batch_size_for_append_modes_5s_150ms_keep() {
        let c = Chunker::new(RecordingMode::AppendBelow, 3.5);
        assert_eq!(c.n_batch_samples(), 80_000);
        assert_eq!(c.keep_samples(), 2_400);
        assert_eq!(c.max_queue_size(), 240_000);

        // transcription_step is ignored outside AppendAndCorrect.
        let c2 = Chunker::new(RecordingMode::AppendAbove, 2.0);
        assert_eq!(c2.n_batch_samples(), 80_000);
        assert_eq!(c2.keep_samples(), 2_400);
        assert_eq!(c2.max_queue_size(), 240_000);
    }

    #[test]
    fn new_computes_batch_size_for_append_and_correct_from_transcription_step() {
        let c = Chunker::new(RecordingMode::AppendAndCorrect, 3.5);
        assert_eq!(c.n_batch_samples(), 56_000);
        assert_eq!(c.keep_samples(), 24_000);
        assert_eq!(c.max_queue_size(), 168_000);

        let c2 = Chunker::new(RecordingMode::AppendAndCorrect, 2.0);
        assert_eq!(c2.n_batch_samples(), 32_000);
        assert_eq!(c2.keep_samples(), 24_000); // fixed 1.5s regardless of step
        assert_eq!(c2.max_queue_size(), 96_000);

        let c3 = Chunker::new(RecordingMode::AppendAndCorrect, 5.0);
        assert_eq!(c3.n_batch_samples(), 80_000);
        assert_eq!(c3.max_queue_size(), 240_000);
    }

    // -- push / drop-when-3-batches-behind --------------------------------

    #[test]
    fn push_accepts_until_queue_reaches_max_then_drops_entirely() {
        let mut c = Chunker::new(RecordingMode::AppendBelow, 3.5);
        assert_eq!(c.max_queue_size(), 240_000);
        let chunk = vec![0.0f32; 100_000];

        assert_eq!(c.push(&chunk), 100_000);
        assert_eq!(c.queue_len(), 100_000);

        assert_eq!(c.push(&chunk), 100_000);
        assert_eq!(c.queue_len(), 200_000);

        // 200_000 < 240_000: still accepted, even though the queue ends up
        // past max_queue_size (Buzz checks size *before* appending).
        assert_eq!(c.push(&chunk), 100_000);
        assert_eq!(c.queue_len(), 300_000);

        // 300_000 >= 240_000: dropped entirely.
        assert_eq!(c.push(&chunk), 0);
        assert_eq!(c.queue_len(), 300_000);
    }

    #[test]
    fn push_accepts_a_chunk_smaller_than_the_remaining_headroom_without_partial_accept() {
        let mut c = Chunker::new(RecordingMode::AppendBelow, 3.5);
        let chunk = vec![0.5f32; 10];
        assert_eq!(c.push(&chunk), 10);
        assert_eq!(c.queue_len(), 10);
    }

    // -- next_batch: not enough buffered yet -------------------------------

    #[test]
    fn next_batch_is_none_below_n_batch_samples() {
        let mut c = Chunker::new(RecordingMode::AppendAndCorrect, 3.5);
        assert_eq!(c.n_batch_samples(), 56_000);
        c.push(&vec![0.5f32; 55_999]);
        assert!(c.next_batch().is_none());
    }

    // -- find_silence_cut_point: hand-derived synthetic signals -----------

    #[test]
    fn find_silence_cut_point_cuts_at_rightmost_quiet_window_midpoint() {
        // 4.0s @ 16kHz: loud [0, 56_000), quiet [56_000, 64_000). The quiet
        // tail crosses fully into the 1.5s search region (samples
        // [40_000, 64_000)), so the rightmost quiet window (i=73 of 74) is
        // the very last one; cut = 40_000 + 73*320 + 160 = 63_520.
        let mut samples = vec![0.5f32; 56_000];
        samples.extend(vec![0.0001f32; 8_000]);
        assert_eq!(samples.len(), 64_000);

        assert_eq!(find_silence_cut_point(&samples, 16_000), 63_520);
    }

    #[test]
    fn find_silence_cut_point_skips_a_loud_tail_to_find_an_earlier_quiet_window() {
        // 4.0s @ 16kHz: loud [0, 40_000) (outside the search region),
        // quiet [40_000, 48_000), loud again [48_000, 64_000). The search
        // region is [40_000, 64_000); its rightmost quiet window is at
        // region-relative index 24 (of 74): cut = 40_000 + 24*320 + 160 =
        // 47_840, inside the quiet band, correctly skipping the loud tail.
        let mut samples = vec![0.5f32; 40_000];
        samples.extend(vec![0.0001f32; 8_000]);
        samples.extend(vec![0.5f32; 16_000]);
        assert_eq!(samples.len(), 64_000);

        assert_eq!(find_silence_cut_point(&samples, 16_000), 47_840);
    }

    #[test]
    fn find_silence_cut_point_falls_back_to_full_batch_when_uniformly_loud() {
        // 3.0s @ 16kHz, uniform amplitude: every window's RMS equals the
        // mean exactly, so none is below the 0.5x threshold -- no cut
        // found, fallback to len(samples).
        let samples = vec![0.5f32; 48_000];
        assert_eq!(find_silence_cut_point(&samples, 16_000), 48_000);
    }

    #[test]
    fn find_silence_cut_point_falls_back_when_shorter_than_one_window() {
        // 100 samples @ 16kHz is shorter than the 320-sample window itself.
        let samples = vec![0.0f32; 100];
        assert_eq!(find_silence_cut_point(&samples, 16_000), 100);
    }

    #[test]
    fn find_silence_cut_point_falls_back_when_region_has_less_than_two_windows() {
        // 400 samples: >= one window (320) but < two (640), so
        // n_windows = (400-320)//320 = 0, the "< 1" fallback path distinct
        // from the shorter-than-one-window early return above.
        let samples = vec![0.0f32; 400];
        assert_eq!(find_silence_cut_point(&samples, 16_000), 400);
    }

    // -- rms / amplitude ----------------------------------------------------

    #[test]
    fn rms_of_empty_slice_is_zero_not_nan() {
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn rms_of_constant_signal_is_its_absolute_value() {
        assert_eq!(rms(&[0.5, 0.5, 0.5, 0.5]), 0.5);
        assert_eq!(rms(&[-0.5, -0.5, -0.5, -0.5]), 0.5);
        assert_eq!(rms(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn rms_of_mixed_signal_matches_sqrt_mean_square() {
        // mean(9 + 16) / 2 = 12.5; sqrt(12.5) = 3.5355339059327378...
        approx_eq(rms(&[3.0, 4.0]), 3.5355339059327378, 1e-12);
    }

    // -- Timeline accounting: keep-back must not double-count -------------

    #[test]
    fn append_and_correct_advances_queue_by_cut_minus_keep_samples() {
        let mut c = Chunker::new(RecordingMode::AppendAndCorrect, 3.5);
        assert_eq!(c.n_batch_samples(), 56_000);
        assert_eq!(c.keep_samples(), 24_000);

        // Uniformly loud throughout: find_silence_cut_point always falls
        // back to the full n_batch_samples region, so `cut` is pinned at
        // 56_000 for every batch, making the arithmetic hand-verifiable.
        c.push(&vec![0.5f32; 86_000]);

        let b1 = c.next_batch().expect("batch 1");
        assert_eq!(b1.samples.len(), 56_000);
        assert_eq!(b1.rms, 0.5);
        // drained = cut - keep_samples = 56_000 - 24_000 = 32_000.
        assert_eq!(b1.queue_len_after, 86_000 - 32_000);
        assert_eq!(c.queue_len(), 54_000);
        assert_eq!(b1.start_ms, Millis(0));
        assert_eq!(b1.end_ms, Millis(3_500)); // 56_000 samples @ 16kHz

        // Not enough buffered yet for batch 2 (54_000 < 56_000).
        assert!(c.next_batch().is_none());
        c.push(&vec![0.5f32; 2_000]);
        assert_eq!(c.queue_len(), 56_000);

        let b2 = c.next_batch().expect("batch 2");
        assert_eq!(b2.samples.len(), 56_000);
        assert_eq!(b2.queue_len_after, 56_000 - 32_000);
        assert_eq!(c.queue_len(), 24_000);

        // The non-double-counting rule: batch 2 starts exactly where batch
        // 1 ended, even though batch 2's *samples* physically re-include
        // batch 1's 24_000-sample keep-back tail.
        assert_eq!(b2.start_ms, b1.end_ms);
        assert_eq!(b2.start_ms, Millis(3_500));
        assert_eq!(b2.end_ms, Millis(7_000));
    }

    #[test]
    fn append_below_advances_queue_by_cut_with_no_keep_back() {
        let mut c = Chunker::new(RecordingMode::AppendBelow, 3.5);
        assert_eq!(c.n_batch_samples(), 80_000);

        c.push(&vec![0.5f32; 90_000]);
        let b1 = c.next_batch().expect("batch 1");
        assert_eq!(b1.samples.len(), 80_000);
        assert_eq!(b1.queue_len_after, 90_000 - 80_000);
        assert_eq!(c.queue_len(), 10_000);
        assert_eq!(b1.start_ms, Millis(0));
        assert_eq!(b1.end_ms, Millis(5_000)); // 80_000 samples @ 16kHz

        c.push(&vec![0.5f32; 70_000]);
        assert_eq!(c.queue_len(), 80_000);
        let b2 = c.next_batch().expect("batch 2");
        assert_eq!(b2.queue_len_after, 0);
        assert_eq!(b2.start_ms, b1.end_ms);
        assert_eq!(b2.end_ms, Millis(10_000));
    }

    #[test]
    fn append_and_correct_clamps_drain_to_zero_when_cut_is_smaller_than_keep_samples() {
        let mut c = Chunker::new(RecordingMode::AppendAndCorrect, 2.0);
        assert_eq!(c.n_batch_samples(), 32_000);
        assert_eq!(c.keep_samples(), 24_000);

        // The 1.5s search region is samples[8_000, 32_000). Its very first
        // window (region-relative index 0 of 74) is quiet; every other
        // window is loud -- so find_silence_cut_point's backward scan only
        // matches at i=0, returning an early cut (8_160) well below
        // keep_samples (24_000).
        let mut samples = vec![0.5f32; 8_000]; // outside the search region
        samples.extend(vec![0.0001f32; 320]); // region window 0: quiet
        samples.extend(vec![0.5f32; 23_680]); // region windows 1..73: loud
        assert_eq!(samples.len(), 32_000);

        c.push(&samples);
        let b = c.next_batch().expect("batch");
        assert_eq!(b.samples.len(), 8_160);

        // Drain clamps to zero rather than underflowing (Buzz's own Python
        // would slice with a negative index here -- see next_batch's doc
        // comment): the whole queue is retained, not partially consumed.
        assert_eq!(b.queue_len_after, 32_000);
        assert_eq!(c.queue_len(), 32_000);

        // The timeline still advances by `cut` alone, per the
        // non-double-counting rule, independent of how much (if anything)
        // was drained from the queue.
        assert_eq!(b.start_ms, Millis(0));
        assert_eq!(b.end_ms, Millis(510)); // 8_160 samples @ 16kHz
    }

    // -- differential fixtures (real numpy 1.21.5, copied verbatim math) --

    #[derive(serde::Deserialize)]
    struct ChunkerFixture {
        sample_rate: u32,
        samples: Vec<f64>,
        expected_cut: usize,
        expected_rms: f64,
    }

    /// Every `crates/humm-core/tests/fixtures/live/*.json` fixture,
    /// generated by running the real numpy 1.21.5 (`validation/
    /// m5-differential/gen_chunker_fixtures.py`) over Buzz's
    /// `find_silence_cut_point`/`amplitude` static methods, copied verbatim
    /// from `recording_transcriber.py:399-433`, against deterministic
    /// closed-form synthetic vectors.
    const FIXTURES: &[(&str, &str)] = &[
        (
            "loud-then-quiet-tail",
            include_str!("../../tests/fixtures/live/loud-then-quiet-tail.json"),
        ),
        (
            "quiet-then-loud-tail",
            include_str!("../../tests/fixtures/live/quiet-then-loud-tail.json"),
        ),
        (
            "three-tier-mixed",
            include_str!("../../tests/fixtures/live/three-tier-mixed.json"),
        ),
        (
            "all-loud-fallback",
            include_str!("../../tests/fixtures/live/all-loud-fallback.json"),
        ),
        (
            "sub-window-fallback",
            include_str!("../../tests/fixtures/live/sub-window-fallback.json"),
        ),
        (
            "less-than-two-windows-fallback",
            include_str!("../../tests/fixtures/live/less-than-two-windows-fallback.json"),
        ),
    ];

    #[test]
    fn chunker_matches_python_reference_fixtures() {
        assert_eq!(FIXTURES.len(), 6, "expected 6 differential fixtures");

        for (name, json) in FIXTURES {
            let fixture: ChunkerFixture =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("fixture {name} JSON: {e}"));

            // Fixture values are Python float64; cast to f32 to simulate the
            // real audio-sample representation `find_silence_cut_point`/
            // `rms` actually receive (see this module's numeric-parity
            // note for why this f32-rounding is immaterial to the outcome
            // given the fixtures' amplitude margins).
            let samples: Vec<f32> = fixture.samples.iter().map(|&s| s as f32).collect();

            let actual_cut = find_silence_cut_point(&samples, fixture.sample_rate);
            assert_eq!(
                actual_cut, fixture.expected_cut,
                "fixture {name}: cut point mismatch"
            );

            let actual_rms = rms(&samples);
            approx_eq(actual_rms, fixture.expected_rms, 1e-6);
        }
    }
}
