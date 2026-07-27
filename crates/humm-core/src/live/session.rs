//! The live-recording orchestration loop, ported from Buzz's
//! `RecordingTranscriber.start`'s `while self.is_running` body
//! (`recording_transcriber.py:100-160`, spec §3).
//!
//! This module owns the *thread-body* concern: draining the capture ring,
//! driving [`crate::live::chunker::Chunker`] (which owns all the Buzz batch
//! math -- this module never re-derives it), deciding skip-vs-transcribe,
//! and calling the injected engine closure with the rolling prompt. It does
//! no I/O and names no adapter: the ring consumer, the engine, and the
//! sink are all supplied by the caller (`humm-gui`, on a thread it spawns
//! when a recording starts).
//!
//! Two testability seams keep [`run_session`] fully exercisable with fakes,
//! never sleeping in a test:
//!
//! - [`crate::live::export::Sleeper`] -- reused verbatim from the export
//!   module (its own doc comment already anticipated this: "the session
//!   thread uses `RealSleeper`"). Backs the 500 ms idle/skip pacing
//!   (Buzz's `time.sleep(0.5)`), sliced into small increments so a
//!   mid-wait cancel is observed promptly rather than after the full
//!   500 ms -- see [`idle_pause`].
//! - [`Clock`] -- new here. Times each engine call for the §2.8 realtime
//!   measurement. Deliberately narrower than a general "what time is it"
//!   abstraction (which would need fragile now()/now() bookkeeping to
//!   fake): it wraps the call itself, so a test double can hand back a
//!   scripted duration decoupled from how long the (possibly-instant, fake)
//!   engine closure actually took.
//!
//! `LiveEvent::Stopped` carries the session's [`LiveMeasurement`] directly,
//! where spec §3's sketch shows the stop signal and the stop-time
//! measurement as two separate things. Folding them into one field is the
//! idiomatic Rust shape here: exactly one terminal event, with its payload,
//! rather than making the GUI correlate two independent arrivals. No
//! information is lost -- documented deviation.

use std::time::{Duration, Instant};

use crate::boundary::{MeterSnapshot, SAMPLE_RATE};
use crate::cancel::CancelToken;
use crate::domain::Millis;
use crate::error::EngineError;
use crate::live::RecordingMode;
use crate::live::chunker::Chunker;
use crate::live::export::Sleeper;

/// Idle/skip pacing duration -- Buzz's `time.sleep(0.5)`
/// (`recording_transcriber.py:110,132`).
const IDLE_PAUSE: Duration = Duration::from_millis(500);

/// The slice [`IDLE_PAUSE`] is split into so a mid-wait cancel is observed
/// within one slice instead of the full 500 ms. Production-only latency
/// (at most this much extra delay before `Stopped`); invisible to tests,
/// whose injected [`Sleeper`] never really blocks.
const PACE_SLICE: Duration = Duration::from_millis(50);

/// How many **consecutive** non-canceled engine-call failures a session
/// tolerates before ending in a terminal [`LiveEvent::Error`] (spec §7.4,
/// ledger #9 -- supersedes M7's L17 fail-on-first ruling). Each tolerated
/// failure emits a non-terminal [`LiveEvent::ChunkSkipped`] instead and the
/// loop continues; a success resets the count to zero.
///
/// N=3 rationale (spec §7.4): one transient 429/500 no longer kills a
/// session Buzz would survive, while a sustained outage (>= 15 s at this
/// mode's ~5 s chunk cadence) still fails loud rather than silently eating
/// audio forever.
const MAX_CONSECUTIVE_CHUNK_FAILURES: usize = 3;

/// One update emitted by [`run_session`] as the live loop progresses.
///
/// Buzz's transcriber exposes these as five separate Qt signals
/// (`transcription`, `average_amplitude_changed` + `queue_size_changed`,
/// `error`, `finished`); this enum folds them into one Rust sum type the
/// GUI matches over in a single place.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    /// One accepted utterance, with its position on the recording's
    /// stream-position timeline (persistence needs `start`/`end` to write
    /// the segment row, spec §4). Emitted even when `text` is empty --
    /// filtering (`filter_text`) and merge (F-062) are GUI-side transforms
    /// on the display copy, not this loop's job (Buzz filters in the
    /// widget, not the transcriber).
    Text {
        /// The engine's raw output text for this batch.
        text: String,
        /// Start of this batch on the recording's timeline.
        start: Millis,
        /// End of this batch on the recording's timeline.
        end: Millis,
    },
    /// Per-batch meter data -- Buzz's `average_amplitude_changed` +
    /// `queue_size_changed`, emitted together from the same loop iteration
    /// (`recording_transcriber.py:122-123`). Emitted for every cut batch,
    /// whether it goes on to be skipped or transcribed.
    MeterBatch {
        /// RMS of the batch that was just cut.
        rms: f32,
        /// Samples remaining in the accumulation queue after the cut.
        queue_len: usize,
    },
    /// A live chunk's engine call failed for a genuine reason (not a
    /// cancellation), but the session tolerates it: fewer than
    /// [`MAX_CONSECUTIVE_CHUNK_FAILURES`] consecutive failures have
    /// happened, so the loop continues instead of ending (spec §7.4, ledger
    /// #9 -- supersedes M7's L17 fail-on-first ruling). The chunk's text is
    /// dropped entirely -- the rolling prompt stays whatever the last
    /// *successful* call produced (or `initial_prompt`, if none yet has) --
    /// never Buzz's own behavior of feeding a fake `"Error: ..."` string
    /// back in as the next chunk's prompt
    /// (`recording_transcriber.py:355-359`, M7 ledger #L17). A subsequent
    /// success resets the consecutive-failure count to zero; the
    /// [`MAX_CONSECUTIVE_CHUNK_FAILURES`]th consecutive failure instead
    /// emits [`LiveEvent::Error`] and ends the loop.
    ChunkSkipped {
        /// The underlying `EngineError`'s message (e.g. `"inference failed:
        /// ..."`, `"OpenAI API backend error: ..."`).
        detail: String,
    },
    /// The engine call failed for a genuine reason (not a cancellation --
    /// `Err(`[`EngineError::Canceled`]`)` ends the loop silently instead, no
    /// `Error` event, so a plain Stop landing mid-call never surfaces as an
    /// error) **and** this was the [`MAX_CONSECUTIVE_CHUNK_FAILURES`]th
    /// consecutive such failure -- fewer than that tolerates the failure via
    /// [`LiveEvent::ChunkSkipped`] instead. The loop has already stopped
    /// (Buzz: `result is None: return`); a [`LiveEvent::Stopped`] follows
    /// immediately.
    Error(String),
    /// The loop has exited -- via cancellation (including a canceled
    /// in-flight engine call), or after a genuine [`LiveEvent::Error`] --
    /// and will not run again. Emitted exactly once, always last.
    Stopped {
        /// This session's realtime measurement (spec §2.8), or `None` if
        /// no batch was ever successfully transcribed this session.
        measurement: Option<LiveMeasurement>,
    },
}

/// A session's realtime-transcription measurement (spec §2.8): total audio
/// handed to the engine versus total wall time spent inside the engine
/// producing it. Skipped (silence) batches contribute to neither sum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveMeasurement {
    /// Total seconds of audio transcribed across every successful engine
    /// call this session.
    pub audio_secs: f64,
    /// Total wall-clock seconds spent inside those engine calls.
    pub engine_secs: f64,
}

impl LiveMeasurement {
    /// `audio_secs / engine_secs` -- seconds of audio the engine processes
    /// per wall second; `> 1.0` is comfortably realtime. `None` when
    /// `engine_secs` is `0.0` (the ratio is undefined, not infinite).
    pub fn realtime_ratio(&self) -> Option<f64> {
        if self.engine_secs == 0.0 {
            None
        } else {
            Some(self.audio_secs / self.engine_secs)
        }
    }
}

/// Per-session configuration [`run_session`] is driven by -- the subset of
/// `RecordingSettings` (spec §6) the loop itself reads; display-mode
/// application, persistence, and export are GUI-side concerns that read the
/// same settings independently.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionConfig {
    /// Which display-mode batch sizing the [`Chunker`] uses (spec §2.2).
    pub mode: RecordingMode,
    /// `AppendAndCorrect`'s batch step, in seconds. Ignored in the two
    /// append modes (see [`Chunker::new`]).
    pub transcription_step_secs: f64,
    /// A batch whose RMS falls below this is skipped: no engine call, no
    /// `Text` event (Buzz default `0.0025`).
    pub silence_threshold: f32,
    /// The rolling prompt's starting value (Buzz's
    /// `transcription_options.initial_prompt`). Every engine call after the
    /// first sees the previous call's output instead.
    pub initial_prompt: String,
}

/// One live-transcription engine call: raw batch samples + the rolling
/// prompt in, transcribed text or an [`EngineError`] out. Named here purely
/// to keep [`run_session`]'s signature from spelling out the multi-argument
/// `dyn FnMut` type inline (clippy's `type_complexity`); [`Clock::timed`]
/// wraps one *call* to this (a zero-argument closure), which is a distinct,
/// simpler type of its own.
///
/// Carries an explicit lifetime parameter -- a bare `dyn FnMut(..)` type
/// alias defaults its trait-object bound to `'static`, which would forbid
/// exactly the short-lived, locals-borrowing closures the caller (and every
/// test here) needs to pass.
pub type EngineFn<'a> = dyn FnMut(&[f32], &str) -> Result<String, EngineError> + 'a;

/// Wall-clock timing seam for the §2.8 realtime measurement: production
/// times each engine call via [`std::time::Instant`]; tests substitute a
/// scripted duration per call. Kept separate from the idle-pause
/// [`Sleeper`] seam -- this measures work, that one delays it.
pub trait Clock: Send + Sync {
    /// Calls `f`, returning its result alongside the wall-clock duration it
    /// took -- or, in a test double, a scripted duration decoupled from how
    /// long `f` actually ran.
    fn timed(
        &self,
        f: &mut dyn FnMut() -> Result<String, EngineError>,
    ) -> (Result<String, EngineError>, Duration);
}

/// The production [`Clock`]: wraps [`std::time::Instant`].
#[derive(Debug, Default, Clone, Copy)]
pub struct RealClock;

impl Clock for RealClock {
    fn timed(
        &self,
        f: &mut dyn FnMut() -> Result<String, EngineError>,
    ) -> (Result<String, EngineError>, Duration) {
        let start = Instant::now();
        let result = f();
        (result, start.elapsed())
    }
}

/// Drains every sample currently available from `consumer` into `chunker`
/// via a single [`Chunker::push`] call -- mirroring Buzz's audio callback,
/// which appends one whole chunk per invocation rather than sample by
/// sample (the accept/drop decision is made once against the queue's
/// pre-call length, see [`Chunker::push`]'s doc comment). A no-op when
/// nothing is queued.
fn drain_ring(consumer: &mut rtrb::Consumer<f32>, chunker: &mut Chunker) {
    let mut drained = Vec::new();
    while let Ok(sample) = consumer.pop() {
        drained.push(sample);
    }
    if !drained.is_empty() {
        chunker.push(&drained);
    }
}

/// Buzz's `time.sleep(0.5)` idle/skip pacing, sliced into [`PACE_SLICE`]
/// increments through the injected [`Sleeper`] so a mid-wait
/// [`CancelToken::cancel`] is observed within one slice instead of the full
/// [`IDLE_PAUSE`].
fn idle_pause(sleeper: &dyn Sleeper, cancel: &CancelToken) {
    let mut waited = Duration::ZERO;
    while waited < IDLE_PAUSE && !cancel.is_canceled() {
        sleeper.sleep(PACE_SLICE);
        waited += PACE_SLICE;
    }
}

/// Runs the live transcription loop until `cancel` is tripped or the engine
/// errors, emitting [`LiveEvent`]s to `sink` as it goes. Blocks the calling
/// thread for the whole session -- intended to run on its own thread,
/// spawned by the GUI when a recording starts.
///
/// Port of Buzz's `RecordingTranscriber.start`'s `while self.is_running`
/// body (`recording_transcriber.py:100-160`). Each iteration:
///
/// 1. Drains everything currently buffered in `consumer` into the
///    [`Chunker`] (drop counting is the chunker's own job, not this loop's
///    -- see [`Chunker::push`]).
/// 2. If a batch is ready: emits [`LiveEvent::MeterBatch`] and writes
///    `meter`'s `average_rms`/`queue_len` unconditionally, then either
///    skips it (RMS below `config.silence_threshold`: no engine call, no
///    `Text`, idle-paused same as an empty queue) or transcribes it with
///    the rolling prompt (starts at `config.initial_prompt`, becomes each
///    successful call's output -- a tolerated failure never changes it,
///    see [`LiveEvent::ChunkSkipped`]). A genuine engine error tolerates up
///    to [`MAX_CONSECUTIVE_CHUNK_FAILURES`] - 1 consecutive failures via
///    [`LiveEvent::ChunkSkipped`] (loop continues, chunk's text dropped);
///    the [`MAX_CONSECUTIVE_CHUNK_FAILURES`]th consecutive failure instead
///    emits [`LiveEvent::Error`] and ends the loop (Buzz: `result is None:
///    return`) -- but `Err(`[`EngineError::Canceled`]`)` ends the loop
///    *silently*, no `Error`/`ChunkSkipped` event, regardless of the
///    consecutive-failure count: that variant is what a canceled in-flight
///    call returns (see below), and a plain Stop must not look like a
///    failure to the GUI. Buzz draws the same distinction
///    (`recording_transcriber.py:356-359`: `if self.is_running: ...emit
///    error...` else swallow it).
/// 3. If no batch is ready: idle-pauses the same way.
///
/// Whatever ends the loop -- cancellation or an error -- a single
/// [`LiveEvent::Stopped`] always follows, carrying this session's
/// [`LiveMeasurement`] if at least one batch was successfully transcribed.
///
/// `clock` and `sleeper` are the two testability seams (see the module
/// doc comment). Production wires [`RealClock`] and
/// [`crate::live::export::RealSleeper`]; tests substitute fakes so the
/// whole loop runs with no real waiting and fully scripted timing.
///
/// Cancellation is checked between iterations (top of the loop, and inside
/// [`idle_pause`]'s slices) -- this loop cannot preempt an `engine` call
/// that is already in progress, since it blocks synchronously on it like
/// any other function call. Spec §3's "in-flight engine call aborts" stop
/// semantic is therefore the *caller's* responsibility: `engine`'s closure
/// is expected to close over the same `cancel` token and forward it to the
/// underlying `TranscriptionEngine::transcribe`'s abort callback (spec
/// §2.2.4), so a long inference genuinely stops early. Once that call
/// returns (aborted or not), this loop's very next cancel check -- at the
/// top of the `while` -- guarantees no further batch is drained or
/// transcribed.
///
/// Eight collaborators is exactly the shape of "generic over every boundary
/// this loop touches" the spec calls for (capture ring, engine, meter,
/// event sink, cancellation, config, and the two testability seams) --
/// bundling them into a struct would just move the same fields one level
/// out without reducing what a caller has to supply.
#[allow(clippy::too_many_arguments)]
pub fn run_session(
    mut consumer: rtrb::Consumer<f32>,
    engine: &mut EngineFn<'_>,
    meter: &MeterSnapshot,
    sink: &dyn Fn(LiveEvent),
    cancel: &CancelToken,
    config: &SessionConfig,
    clock: &dyn Clock,
    sleeper: &dyn Sleeper,
) {
    let mut chunker = Chunker::new(config.mode, config.transcription_step_secs);
    let mut prompt = config.initial_prompt.clone();
    let mut audio_secs = 0.0_f64;
    let mut engine_secs = 0.0_f64;
    let mut transcribed_calls = 0_usize;
    // Consecutive (non-canceled) engine-call failures since the last
    // success -- spec §7.4's tolerate-N counter. Reset on every `Ok`;
    // never touched by `Err(EngineError::Canceled)`, which ends the loop
    // outright before this counter would matter.
    let mut consecutive_failures = 0_usize;

    while !cancel.is_canceled() {
        drain_ring(&mut consumer, &mut chunker);

        let Some(batch) = chunker.next_batch() else {
            idle_pause(sleeper, cancel);
            continue;
        };

        sink(LiveEvent::MeterBatch {
            rms: batch.rms as f32,
            queue_len: batch.queue_len_after,
        });
        meter.set_average_rms(batch.rms as f32);
        meter.set_queue_len(batch.queue_len_after);

        if batch.rms < f64::from(config.silence_threshold) {
            idle_pause(sleeper, cancel);
            continue;
        }

        let mut call = || engine(&batch.samples, prompt.as_str());
        let (result, elapsed) = clock.timed(&mut call);

        match result {
            Ok(text) => {
                audio_secs += batch.samples.len() as f64 / f64::from(SAMPLE_RATE);
                engine_secs += elapsed.as_secs_f64();
                transcribed_calls += 1;
                consecutive_failures = 0;
                prompt = text.clone();
                sink(LiveEvent::Text {
                    text,
                    start: batch.start_ms,
                    end: batch.end_ms,
                });
            }
            // A cancel-caused failure is not a genuine error -- Buzz's own
            // transcriber distinguishes the two the same way
            // (`recording_transcriber.py:356-359`: `if self.is_running:
            // ...error...` else an empty, silent result). `EngineError` is
            // `#[non_exhaustive]`, so this still needs a catch-all arm for
            // any future variant, which is treated as a genuine error.
            Err(EngineError::Canceled) => break,
            // Tolerate-N (spec §7.4, ledger #9): the chunk's own text is
            // dropped -- `prompt` is deliberately left untouched here, so
            // the next call still sees the last *successful* output (or
            // `initial_prompt`), never a fake error string injected as
            // prompt text (M7 ledger #L17).
            Err(err) => {
                consecutive_failures += 1;
                if consecutive_failures >= MAX_CONSECUTIVE_CHUNK_FAILURES {
                    sink(LiveEvent::Error(err.to_string()));
                    break;
                }
                sink(LiveEvent::ChunkSkipped {
                    detail: err.to_string(),
                });
            }
        }
    }

    let measurement = (transcribed_calls > 0).then_some(LiveMeasurement {
        audio_secs,
        engine_secs,
    });
    sink(LiveEvent::Stopped { measurement });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ring(capacity: usize) -> (rtrb::Producer<f32>, rtrb::Consumer<f32>) {
        rtrb::RingBuffer::<f32>::new(capacity)
    }

    fn config(mode: RecordingMode, transcription_step_secs: f64) -> SessionConfig {
        SessionConfig {
            mode,
            transcription_step_secs,
            silence_threshold: 0.0025,
            initial_prompt: String::new(),
        }
    }

    /// A [`Sleeper`] that never really blocks -- for tests whose pacing
    /// isn't the thing under test (a scripted [`CancelToken`]/engine/sink
    /// always ends the loop deterministically instead).
    #[derive(Debug, Default, Clone, Copy)]
    struct NoopSleeper;

    impl Sleeper for NoopSleeper {
        fn sleep(&self, _duration: Duration) {}
    }

    /// A [`Sleeper`] that trips `cancel` on its `threshold`th call --
    /// simulates a user-initiated stop landing mid idle-pause.
    struct CancelAfterNSleeper {
        calls: AtomicUsize,
        threshold: usize,
        cancel: CancelToken,
    }

    impl CancelAfterNSleeper {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Sleeper for CancelAfterNSleeper {
        fn sleep(&self, _duration: Duration) {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= self.threshold {
                self.cancel.cancel();
            }
        }
    }

    /// A [`Clock`] that hands back durations from a fixed script, one per
    /// call, ignoring how long `f` actually took -- so measurement math is
    /// exercised without any real timing.
    struct ScriptedClock {
        durations: Mutex<VecDeque<Duration>>,
    }

    impl ScriptedClock {
        fn new(durations: Vec<Duration>) -> Self {
            ScriptedClock {
                durations: Mutex::new(durations.into_iter().collect()),
            }
        }
    }

    impl Clock for ScriptedClock {
        fn timed(
            &self,
            f: &mut dyn FnMut() -> Result<String, EngineError>,
        ) -> (Result<String, EngineError>, Duration) {
            let result = f();
            let elapsed = self
                .durations
                .lock()
                .expect("scripted clock mutex")
                .pop_front()
                .unwrap_or(Duration::ZERO);
            (result, elapsed)
        }
    }

    fn stopped_measurement(events: &[LiveEvent]) -> Option<LiveMeasurement> {
        match events.last() {
            Some(LiveEvent::Stopped { measurement }) => *measurement,
            other => panic!("expected Stopped as the last event, got {other:?}"),
        }
    }

    // -- realtime_ratio: pure math, no loop involved -----------------------

    #[test]
    fn realtime_ratio_computes_audio_over_engine_seconds() {
        let m = LiveMeasurement {
            audio_secs: 10.0,
            engine_secs: 5.0,
        };
        assert_eq!(m.realtime_ratio(), Some(2.0));
    }

    #[test]
    fn realtime_ratio_none_when_engine_secs_is_zero() {
        let m = LiveMeasurement {
            audio_secs: 5.0,
            engine_secs: 0.0,
        };
        assert_eq!(m.realtime_ratio(), None);
    }

    // -- RealClock: isolated sanity check (not part of the no-sleep loop --

    #[test]
    fn real_clock_measures_wall_time_of_the_call() {
        let clock = RealClock;
        let mut call = || {
            std::thread::sleep(Duration::from_millis(5));
            Ok("done".to_string())
        };
        let (result, elapsed) = clock.timed(&mut call);
        assert_eq!(result.unwrap(), "done");
        assert!(elapsed >= Duration::from_millis(5));
    }

    // -- chunk cadence per mode ---------------------------------------------

    #[test]
    fn append_below_cuts_5s_batches_engine_receives_n_batch_samples_each_call() {
        let (mut producer, consumer) = ring(200_000);
        // 160_000 uniform-loud samples: two 80_000-sample (5s @ 16kHz)
        // batches, no silence-driven early cut (find_silence_cut_point
        // falls back to the full batch on a uniform signal).
        for _ in 0..160_000 {
            producer.push(0.5).unwrap();
        }

        let mut call_sizes: Vec<usize> = Vec::new();
        let mut engine = |samples: &[f32], _prompt: &str| {
            call_sizes.push(samples.len());
            Ok(format!("t{}", call_sizes.len()))
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let text_count = Cell::new(0usize);
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::Text { .. }) {
                text_count.set(text_count.get() + 1);
                if text_count.get() >= 2 {
                    cancel_for_sink.cancel();
                }
            }
        };

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(call_sizes, vec![80_000, 80_000]);
    }

    #[test]
    fn append_and_correct_cuts_step_sized_batches() {
        let (mut producer, consumer) = ring(100_000);
        // transcription_step = 2.0s => n_batch_samples = 32_000, distinct
        // from the append modes' fixed 80_000 -- proves cadence is mode/step
        // driven, not a hardcoded constant.
        for _ in 0..70_000 {
            producer.push(0.5).unwrap();
        }

        let mut call_sizes: Vec<usize> = Vec::new();
        let mut engine = |samples: &[f32], _prompt: &str| {
            call_sizes.push(samples.len());
            Ok(format!("t{}", call_sizes.len()))
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let text_count = Cell::new(0usize);
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::Text { .. }) {
                text_count.set(text_count.get() + 1);
                if text_count.get() >= 2 {
                    cancel_for_sink.cancel();
                }
            }
        };

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendAndCorrect, 2.0);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(call_sizes, vec![32_000, 32_000]);
    }

    // -- silence skip --------------------------------------------------------

    #[test]
    fn quiet_batch_is_skipped_no_engine_call_but_meter_batch_still_emitted() {
        let (mut producer, consumer) = ring(100_000);
        for _ in 0..80_000 {
            producer.push(0.0001).unwrap();
        }

        let mut engine_calls = 0usize;
        let mut engine = |_samples: &[f32], _prompt: &str| {
            engine_calls += 1;
            Ok(String::new())
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::MeterBatch { .. }) {
                cancel_for_sink.cancel();
            }
            events.borrow_mut().push(event);
        };

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(engine_calls, 0);
        let events = events.into_inner();
        assert_eq!(
            events,
            vec![
                LiveEvent::MeterBatch {
                    rms: 0.0001,
                    queue_len: 0
                },
                LiveEvent::Stopped { measurement: None },
            ]
        );
    }

    // -- rolling prompt -------------------------------------------------------

    #[test]
    fn rolling_prompt_first_call_sees_initial_prompt_second_sees_first_output() {
        let (mut producer, consumer) = ring(200_000);
        for _ in 0..160_000 {
            producer.push(0.5).unwrap();
        }

        let mut seen_prompts: Vec<String> = Vec::new();
        let mut engine = |_samples: &[f32], prompt: &str| {
            seen_prompts.push(prompt.to_string());
            Ok(format!("out{}", seen_prompts.len()))
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let text_count = Cell::new(0usize);
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::Text { .. }) {
                text_count.set(text_count.get() + 1);
                if text_count.get() >= 2 {
                    cancel_for_sink.cancel();
                }
            }
        };

        let meter = MeterSnapshot::new();
        let mut cfg = config(RecordingMode::AppendBelow, 3.5);
        cfg.initial_prompt = "seed".to_string();

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(seen_prompts, vec!["seed".to_string(), "out1".to_string()]);
    }

    // -- drop-behind surfaces via MeterBatch.queue_len -----------------------

    #[test]
    fn drop_behind_while_a_call_is_in_flight_surfaces_in_meter_batch_queue_len() {
        let (mut producer, consumer) = ring(350_000);
        // AppendBelow: n_batch_samples = 80_000, max_queue_size = 240_000.
        for _ in 0..320_000 {
            producer.push(0.5).unwrap();
        }

        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let text_count = Cell::new(0usize);
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::Text { .. }) {
                text_count.set(text_count.get() + 1);
                if text_count.get() >= 2 {
                    cancel_for_sink.cancel();
                }
            }
            events.borrow_mut().push(event);
        };

        // The engine closure owns the producer: on its first call it pushes
        // 10_000 more samples into the ring, simulating audio that arrived
        // while the (first) transcription was "in flight". At that moment
        // the chunker's queue sits exactly at max_queue_size (240_000), so
        // the next drain-into-push call must drop the whole 10_000 rather
        // than admit it.
        let mut calls = 0usize;
        let mut engine = move |_samples: &[f32], _prompt: &str| {
            calls += 1;
            if calls == 1 {
                for _ in 0..10_000 {
                    producer.push(0.5).ok();
                }
            }
            Ok(format!("t{calls}"))
        };

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        let events = events.into_inner();
        let meter_queue_lens: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                LiveEvent::MeterBatch { queue_len, .. } => Some(*queue_len),
                _ => None,
            })
            .collect();

        // Batch 1: 320_000 - 80_000 = 240_000 (== max_queue_size exactly).
        // Batch 2: had the dropped 10_000 samples been admitted, this would
        // read 170_000; because they were dropped, it reads 160_000.
        assert_eq!(meter_queue_lens, vec![240_000, 160_000]);
    }

    // -- tolerate-3 consecutive chunk failures (spec §7.4, ledger #9) --------

    /// The `MAX_CONSECUTIVE_CHUNK_FAILURES`th (3rd) *consecutive* engine
    /// failure -- not the 1st -- is what ends the loop: the first two are
    /// tolerated as [`LiveEvent::ChunkSkipped`], and only the third emits
    /// the terminal [`LiveEvent::Error`] followed by exactly one
    /// [`LiveEvent::Stopped`]. Supersedes the pre-M8 fail-on-first behavior
    /// this test used to assert (M7 ledger #L17).
    #[test]
    fn third_consecutive_failure_ends_with_error() {
        let (mut producer, consumer) = ring(300_000);
        // Three consecutive 80_000-sample (5s @ 16kHz) batches -- one per
        // engine call, all uniform-loud so none is silence-skipped.
        for _ in 0..240_000 {
            producer.push(0.5).unwrap();
        }

        let mut engine_calls = 0usize;
        let mut engine = |_samples: &[f32], _prompt: &str| {
            engine_calls += 1;
            Err(EngineError::Inference {
                detail: "boom".to_string(),
            })
        };

        let cancel = CancelToken::new();
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| events.borrow_mut().push(event);

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(
            engine_calls, 3,
            "the loop must keep calling the engine through the first two tolerated failures"
        );
        let events = events.into_inner();
        assert_eq!(
            events,
            vec![
                LiveEvent::MeterBatch {
                    rms: 0.5,
                    queue_len: 160_000
                },
                LiveEvent::ChunkSkipped {
                    detail: "inference failed: boom".to_string()
                },
                LiveEvent::MeterBatch {
                    rms: 0.5,
                    queue_len: 80_000
                },
                LiveEvent::ChunkSkipped {
                    detail: "inference failed: boom".to_string()
                },
                LiveEvent::MeterBatch {
                    rms: 0.5,
                    queue_len: 0
                },
                LiveEvent::Error("inference failed: boom".to_string()),
                LiveEvent::Stopped { measurement: None },
            ],
            "only the 3rd consecutive failure may emit Error, and Stopped must follow exactly once"
        );
    }

    /// A tolerated failure (fewer than 3 consecutive) drops that chunk's
    /// text but does not end the session: the very next batch is still
    /// handed to the engine, and its rolling prompt is the last
    /// *successful* call's output -- never anything derived from the
    /// skipped chunk (M7 ledger #L17's "no fake error-text prompt
    /// injection" ruling carried forward).
    #[test]
    fn chunk_error_emits_chunk_skipped_and_continues() {
        let (mut producer, consumer) = ring(300_000);
        // Three consecutive batches: success, a tolerated failure, success
        // again.
        for _ in 0..240_000 {
            producer.push(0.5).unwrap();
        }

        let mut call_num = 0usize;
        let mut seen_prompts: Vec<String> = Vec::new();
        let mut engine = |_samples: &[f32], prompt: &str| {
            call_num += 1;
            seen_prompts.push(prompt.to_string());
            match call_num {
                1 => Ok("first".to_string()),
                2 => Err(EngineError::Inference {
                    detail: "boom".to_string(),
                }),
                _ => Ok("third".to_string()),
            }
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let text_count = Cell::new(0usize);
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::Text { .. }) {
                text_count.set(text_count.get() + 1);
                if text_count.get() >= 2 {
                    cancel_for_sink.cancel();
                }
            }
            events.borrow_mut().push(event);
        };

        let clock = ScriptedClock::new(vec![
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(0.3),
            Duration::from_secs_f64(4.0),
        ]);
        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &clock,
            &NoopSleeper,
        );

        assert_eq!(
            call_num, 3,
            "the chunk after a tolerated failure must still be transcribed"
        );
        // Every call after the first success sees that success's text as
        // its prompt -- including the call right after the skipped chunk,
        // proving the failure never became (or corrupted) the rolling
        // prompt.
        assert_eq!(
            seen_prompts,
            vec!["".to_string(), "first".to_string(), "first".to_string()]
        );

        let events = events.into_inner();
        assert_eq!(
            events,
            vec![
                LiveEvent::MeterBatch {
                    rms: 0.5,
                    queue_len: 160_000
                },
                LiveEvent::Text {
                    text: "first".to_string(),
                    start: Millis(0),
                    end: Millis(5_000),
                },
                LiveEvent::MeterBatch {
                    rms: 0.5,
                    queue_len: 80_000
                },
                LiveEvent::ChunkSkipped {
                    detail: "inference failed: boom".to_string()
                },
                LiveEvent::MeterBatch {
                    rms: 0.5,
                    queue_len: 0
                },
                LiveEvent::Text {
                    text: "third".to_string(),
                    start: Millis(10_000),
                    end: Millis(15_000),
                },
                LiveEvent::Stopped {
                    measurement: Some(LiveMeasurement {
                        audio_secs: 10.0,
                        engine_secs: 5.0,
                    }),
                },
            ]
        );
    }

    /// fail, fail, ok, fail, fail -- the middle success must reset the
    /// consecutive-failure count, so the trailing two failures (only 2
    /// consecutive, never reaching 3) must not end the session.
    #[test]
    fn success_resets_consecutive_counter() {
        let (mut producer, consumer) = ring(500_000);
        for _ in 0..400_000 {
            producer.push(0.5).unwrap();
        }

        let mut call_num = 0usize;
        let mut engine = |_samples: &[f32], _prompt: &str| {
            call_num += 1;
            if call_num == 3 {
                Ok("ok".to_string())
            } else {
                Err(EngineError::Inference {
                    detail: format!("boom{call_num}"),
                })
            }
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let skip_count = Cell::new(0usize);
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::ChunkSkipped { .. }) {
                skip_count.set(skip_count.get() + 1);
                // The 4th ChunkSkipped is this run's 5th (and last)
                // engine call -- cancel right after it so the loop ends
                // deterministically instead of idle-pausing forever once
                // the ring runs dry.
                if skip_count.get() >= 4 {
                    cancel_for_sink.cancel();
                }
            }
            events.borrow_mut().push(event);
        };

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(
            call_num, 5,
            "a mid-run success must reset the streak so trailing failures don't end the session early"
        );
        let events = events.into_inner();
        assert!(
            !events.iter().any(|e| matches!(e, LiveEvent::Error(_))),
            "only 2 consecutive trailing failures -- must never reach the terminal Error: {events:?}"
        );
        let skip_details: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                LiveEvent::ChunkSkipped { detail } => Some(detail.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            skip_details,
            vec![
                "inference failed: boom1",
                "inference failed: boom2",
                "inference failed: boom4",
                "inference failed: boom5",
            ]
        );
        assert!(
            matches!(events.last(), Some(LiveEvent::Stopped { .. })),
            "a canceled (not failed) end must still emit Stopped last: {events:?}"
        );
    }

    /// A canceled in-flight engine call (`EngineError::Canceled` -- what
    /// `WhisperEngine::transcribe_live` returns whenever the shared
    /// `CancelToken` fires mid-call, i.e. on every ordinary Stop that lands
    /// while a batch is transcribing) must end the loop *silently*: no
    /// `Error` event, just the unconditional `Stopped`. Buzz draws the same
    /// distinction (`recording_transcriber.py:356-359`).
    #[test]
    fn engine_canceled_error_ends_the_loop_without_an_error_event() {
        let (mut producer, consumer) = ring(100_000);
        for _ in 0..80_000 {
            producer.push(0.5).unwrap();
        }

        let mut engine_calls = 0usize;
        let mut engine = |_samples: &[f32], _prompt: &str| {
            engine_calls += 1;
            Err(EngineError::Canceled)
        };

        let cancel = CancelToken::new();
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| events.borrow_mut().push(event);

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(engine_calls, 1);
        let events = events.into_inner();
        assert!(
            !events.iter().any(|e| matches!(e, LiveEvent::Error(_))),
            "a canceled in-flight call must not surface as Error: {events:?}"
        );
        assert_eq!(
            events,
            vec![
                LiveEvent::MeterBatch {
                    rms: 0.5,
                    queue_len: 0
                },
                LiveEvent::Stopped { measurement: None },
            ]
        );
    }

    // -- cancel mid-loop -> Stopped promptly, bounded pacer ticks ------------

    #[test]
    fn cancel_during_idle_pause_stops_promptly_with_bounded_pacer_ticks_and_no_engine_calls() {
        let (_producer, consumer) = ring(16); // never fed: the queue never
        // reaches n_batch_samples, so the loop only ever idle-pauses.

        let mut engine_calls = 0usize;
        let mut engine = |_samples: &[f32], _prompt: &str| {
            engine_calls += 1;
            Ok(String::new())
        };

        let cancel = CancelToken::new();
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| events.borrow_mut().push(event);

        let sleeper = CancelAfterNSleeper {
            calls: AtomicUsize::new(0),
            threshold: 3,
            cancel: cancel.clone(),
        };

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &sleeper,
        );

        assert_eq!(engine_calls, 0);
        assert_eq!(sleeper.calls(), 3, "should stop pacing as soon as canceled");
        assert_eq!(
            events.into_inner(),
            vec![LiveEvent::Stopped { measurement: None }]
        );
    }

    // -- measurement math -----------------------------------------------------

    #[test]
    fn measurement_sums_audio_and_engine_seconds_across_transcribed_batches() {
        let (mut producer, consumer) = ring(200_000);
        for _ in 0..160_000 {
            producer.push(0.5).unwrap();
        }

        let mut engine = |_samples: &[f32], _prompt: &str| Ok("t".to_string());

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let text_count = Cell::new(0usize);
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::Text { .. }) {
                text_count.set(text_count.get() + 1);
                if text_count.get() >= 2 {
                    cancel_for_sink.cancel();
                }
            }
            events.borrow_mut().push(event);
        };

        let clock = ScriptedClock::new(vec![
            Duration::from_secs_f64(1.0),
            Duration::from_secs_f64(4.0),
        ]);
        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &clock,
            &NoopSleeper,
        );

        // Two 80_000-sample (5.0s @ 16kHz) batches: audio_secs = 10.0;
        // scripted engine wall time = 1.0 + 4.0 = 5.0; ratio = 2.0.
        let measurement = stopped_measurement(&events.into_inner()).expect("measurement");
        assert_eq!(measurement.audio_secs, 10.0);
        assert_eq!(measurement.engine_secs, 5.0);
        assert_eq!(measurement.realtime_ratio(), Some(2.0));
    }

    #[test]
    fn measurement_is_none_when_nothing_was_ever_transcribed() {
        let (mut producer, consumer) = ring(100_000);
        for _ in 0..80_000 {
            producer.push(0.0001).unwrap(); // below silence_threshold
        }

        let mut engine_calls = 0usize;
        let mut engine = |_samples: &[f32], _prompt: &str| {
            engine_calls += 1;
            Ok(String::new())
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::MeterBatch { .. }) {
                cancel_for_sink.cancel();
            }
            events.borrow_mut().push(event);
        };

        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &RealClock,
            &NoopSleeper,
        );

        assert_eq!(engine_calls, 0);
        assert_eq!(stopped_measurement(&events.into_inner()), None);
    }

    #[test]
    fn measurement_excludes_skipped_batches_from_audio_secs() {
        let (mut producer, consumer) = ring(200_000);
        // First batch: 80_000 quiet samples (skipped). Second: 80_000 loud
        // samples (transcribed). Only the second should count toward
        // audio_secs.
        for _ in 0..80_000 {
            producer.push(0.0001).unwrap();
        }
        for _ in 0..80_000 {
            producer.push(0.5).unwrap();
        }

        let mut engine_calls = 0usize;
        let mut engine = |_samples: &[f32], _prompt: &str| {
            engine_calls += 1;
            Ok("t".to_string())
        };

        let cancel = CancelToken::new();
        let cancel_for_sink = cancel.clone();
        let events: RefCell<Vec<LiveEvent>> = RefCell::new(Vec::new());
        let sink = |event: LiveEvent| {
            if matches!(event, LiveEvent::Text { .. }) {
                cancel_for_sink.cancel();
            }
            events.borrow_mut().push(event);
        };

        let clock = ScriptedClock::new(vec![Duration::from_secs_f64(2.5)]);
        let meter = MeterSnapshot::new();
        let cfg = config(RecordingMode::AppendBelow, 3.5);

        run_session(
            consumer,
            &mut engine,
            &meter,
            &sink,
            &cancel,
            &cfg,
            &clock,
            &NoopSleeper,
        );

        assert_eq!(engine_calls, 1);
        let measurement = stopped_measurement(&events.into_inner()).expect("measurement");
        assert_eq!(measurement.audio_secs, 5.0);
        assert_eq!(measurement.engine_secs, 2.5);
    }
}
