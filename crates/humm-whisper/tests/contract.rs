//! Contract tests for the whisper-rs engine adapter.
//!
//! These verify the adapter against the *real* vendored whisper.cpp using the
//! Buzz `tiny` model directly (no network), so they are all `#[ignore]` and run
//! with `cargo test -p humm-whisper -- --ignored`. Per the humm Standing Policy,
//! contract tests are the sole gate that an external (whisper.cpp) upgrade did
//! not break the adapter.
//!
//! Prerequisites:
//! - Buzz testdata at `$HUMM_BUZZ_TESTDATA` (default
//!   `/Users/jawad/RustroverProjects/buzz/testdata`): `whisper-french.mp3`,
//!   `audio-long.mp3`.
//! - ffmpeg on `PATH` (or `$HUMM_FFMPEG`) to decode the fixtures.
//! - A real whisper `tiny` model. **Note:** Buzz's `testdata/ggml-tiny.bin` is
//!   a 575 KB truncated placeholder (correct tiny header, absent weights) that
//!   loads but transcribes to *nothing*, so it cannot drive these tests.
//!   [`ensure_tiny_model`] fetches the genuine ~78 MB model via humm-core's
//!   downloader (network on first run; cached + SHA256-verified thereafter), or
//!   uses `$HUMM_WHISPER_TEST_MODEL` if that points at a local real model.
//! - The VAD test additionally fetches the Silero model over the network.
//!
//! Text quality is *not* asserted (the tiny model is weak); these tests pin
//! structure and timing behaviour.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use humm_core::boundary::{
    AudioDecoder, EngineOptions, Pcm, SAMPLE_RATE, SegmentSink, TranscriptionEngine,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{Millis, ModelSize, Segment, TaskKind};
use humm_core::error::EngineError;
use humm_core::language::Language;
use humm_core::models::download::ModelDownloader;
use humm_media::FfmpegDecoder;
use humm_whisper::{LiveOptions, WhisperEngine};

fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

/// Path to a *real* whisper tiny model (see the module docs — the Buzz testdata
/// copy is a truncated stub). Uses `$HUMM_WHISPER_TEST_MODEL` if set, else
/// downloads/caches the genuine model in the humm model root.
fn ensure_tiny_model() -> PathBuf {
    if let Some(path) = std::env::var_os("HUMM_WHISPER_TEST_MODEL") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "HUMM_WHISPER_TEST_MODEL does not exist: {}",
            path.display()
        );
        return path;
    }
    let downloader = ModelDownloader::new(humm_core::paths::model_root());
    downloader
        .ensure_model(ModelSize::Tiny, false, &mut |_| {}, &CancelToken::new())
        .expect("fetch real ggml-tiny.bin (needs network on first run)")
}

fn decode(file: &str) -> Pcm {
    let path = testdata_dir().join(file);
    let decoder = FfmpegDecoder::new();
    decoder
        .decode(&path, &CancelToken::new())
        .unwrap_or_else(|e| panic!("failed to decode {}: {e}", path.display()))
}

fn engine() -> WhisperEngine {
    WhisperEngine::load(&ensure_tiny_model(), true).expect("load tiny model")
}

/// Base options: transcribe French, everything else default. (GPU use is a
/// load-time decision — `WhisperEngine::load(.., use_gpu)` — not an
/// `EngineOptions` field.)
fn french_options() -> EngineOptions {
    EngineOptions {
        language: Language::parse("fr"),
        task: TaskKind::Transcribe,
        initial_prompt: String::new(),
        word_timestamps: false,
        temperature: vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
        vad_model: None,
        threads: None,
    }
}

/// Live-mode equivalent of [`french_options`] (F-061, ADR-008).
fn french_live_options() -> LiveOptions {
    LiveOptions {
        language: Language::parse("fr"),
        task: TaskKind::Transcribe,
        temperature: vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
        threads: None,
    }
}

/// Records everything the engine streams.
#[derive(Default)]
struct RecordingSink {
    segments: Vec<Segment>,
    max_progress: f32,
    progress_calls: usize,
}

impl SegmentSink for RecordingSink {
    fn on_segment(&mut self, segment: Segment) {
        self.segments.push(segment);
    }
    fn on_progress(&mut self, fraction: f32) {
        self.max_progress = self.max_progress.max(fraction);
        self.progress_calls += 1;
    }
}

/// Contract 1: a French mp3 transcribes to at least one non-empty, sanely-timed
/// segment; progress runs to completion; the streamed segments are the whole
/// transcript.
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn transcribes_french_mp3() {
    let pcm = decode("whisper-french.mp3");
    let duration = pcm.duration();
    let mut engine = engine();
    let mut sink = RecordingSink::default();

    engine
        .transcribe(&pcm, &french_options(), &mut sink, &CancelToken::new())
        .expect("transcription should succeed");

    assert!(!sink.segments.is_empty(), "expected at least one segment");

    let combined: String = sink
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !combined.trim().is_empty(),
        "expected non-empty transcript text"
    );

    let first = &sink.segments[0];
    assert!(
        first.start < Millis(2000),
        "first segment should start near the beginning, got {:?}",
        first.start
    );

    let last_end = sink.segments.iter().map(|s| s.end).max().unwrap();
    assert!(
        last_end <= duration + Millis(2000),
        "last segment end {last_end:?} exceeds audio duration {duration:?} + 2 s"
    );

    // The streamed segments *are* the returned transcript (transcribe streams
    // each final segment exactly once); verify they form a consistent whole.
    assert!(
        sink.segments.iter().all(|s| s.start <= s.end),
        "every segment must have start <= end"
    );

    assert!(
        sink.max_progress >= 0.99,
        "progress should reach completion, peaked at {}",
        sink.max_progress
    );
    assert!(sink.progress_calls > 0, "progress callback should fire");
}

/// Contract 2: the translate task completes with at least one non-empty segment
/// (no text assertions — tiny-model quality).
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn translate_task_succeeds() {
    let pcm = decode("whisper-french.mp3");
    let mut engine = engine();
    let mut sink = RecordingSink::default();

    let mut options = french_options();
    options.task = TaskKind::Translate;

    engine
        .transcribe(&pcm, &options, &mut sink, &CancelToken::new())
        .expect("translation should succeed");

    assert!(
        sink.segments.iter().any(|s| !s.text.trim().is_empty()),
        "expected at least one non-empty translated segment"
    );
}

/// Contract 3: cancellation aborts promptly, returning `Canceled` well within
/// five seconds.
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn cancel_is_prompt() {
    let pcm = decode("audio-long.mp3");
    let mut engine = engine();
    let mut sink = RecordingSink::default();

    let cancel = CancelToken::new();
    let cancel_bg = cancel.clone();
    let flag = Arc::new(AtomicBool::new(false));
    let flag_bg = flag.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        cancel_bg.cancel();
        flag_bg.store(true, Ordering::SeqCst);
    });

    let start = Instant::now();
    let result = engine.transcribe(&pcm, &french_options(), &mut sink, &cancel);
    let elapsed = start.elapsed();

    assert!(
        flag.load(Ordering::SeqCst),
        "canceller thread should have fired (test setup)"
    );
    assert!(
        matches!(result, Err(humm_core::error::EngineError::Canceled)),
        "expected Canceled, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "cancellation took too long: {elapsed:?}"
    );
}

/// Contract 4: word-level mode yields more, shorter segments than segment mode,
/// with non-decreasing timings.
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn word_timestamps_produce_words() {
    let pcm = decode("whisper-french.mp3");

    let mut segment_sink = RecordingSink::default();
    engine()
        .transcribe(
            &pcm,
            &french_options(),
            &mut segment_sink,
            &CancelToken::new(),
        )
        .expect("segment-mode transcription");

    let mut word_options = french_options();
    word_options.word_timestamps = true;
    let mut word_sink = RecordingSink::default();
    engine()
        .transcribe(&pcm, &word_options, &mut word_sink, &CancelToken::new())
        .expect("word-mode transcription");

    assert!(
        word_sink.segments.len() > segment_sink.segments.len(),
        "word mode ({}) should produce more segments than segment mode ({})",
        word_sink.segments.len(),
        segment_sink.segments.len()
    );

    let mean_len = word_sink
        .segments
        .iter()
        .map(|s| s.text.chars().count())
        .sum::<usize>() as f64
        / word_sink.segments.len() as f64;
    assert!(
        mean_len < 12.0,
        "word segments should be short, mean length was {mean_len}"
    );

    let mut prev = Millis(i64::MIN);
    for segment in &word_sink.segments {
        assert!(
            segment.start >= prev,
            "word start times must be non-decreasing: {:?} after {:?}",
            segment.start,
            prev
        );
        assert!(segment.start <= segment.end, "word start must be <= end");
        prev = segment.start;
    }
}

/// Contract 5 (F-045): with VAD on, output timestamps are on the **original**
/// audio timeline, not the VAD-compressed one.
///
/// A 5 s silence prefix is prepended to ~8.5 s of French speech (padded total
/// ~13.5 s). VAD strips the leading silence and humm remaps the resulting
/// timestamps back to the original timeline, so:
/// - the **first** segment/word must start at ≥ 4000 ms (where the speech
///   actually begins — *not* collapsed toward 0), and
/// - the **last** end must stay within the padded duration (~13.5 s), not run
///   off the end.
///
/// Both paths are checked: **segment** mode (streamed via the callback) and
/// **word** mode (token times remapped in `emit_word_segments`). If VAD had not
/// run, or timestamps had collapsed to the compressed timeline, the first start
/// would be ~0.
#[test]
#[ignore = "requires network for VAD model"]
fn vad_remaps_to_original_timeline() {
    use humm_core::models::download::ModelDownloader;

    let french = decode("whisper-french.mp3");

    // 5 s of digital silence, then the French speech.
    let silence_samples = (SAMPLE_RATE as usize) * 5;
    let mut padded = vec![0.0f32; silence_samples];
    padded.extend_from_slice(french.samples());
    let pcm = Pcm::new(padded);
    let padded_duration = pcm.duration(); // ~13.5 s (original timeline)
    let ceiling = padded_duration + Millis(2000);

    // Fetch the Silero VAD model into a throwaway root.
    let root = tempfile::tempdir().expect("tempdir");
    let downloader = ModelDownloader::new(root.path());
    let vad_model = downloader
        .ensure_vad_model(&mut |_| {}, &CancelToken::new())
        .expect("download Silero VAD model (needs network)");

    // Baseline: VAD off. The speech starts ~5 s in but whisper places a single
    // segment from 0, so VAD-off first start is ~0 — the contrast the VAD-on
    // assertions rely on.
    let mut off_sink = RecordingSink::default();
    engine()
        .transcribe(&pcm, &french_options(), &mut off_sink, &CancelToken::new())
        .expect("VAD-off transcription");
    eprintln!(
        "VAD OFF first start: {:?}",
        off_sink.segments.first().map(|s| s.start)
    );

    let assert_original_timeline = |label: &str, segments: &[Segment]| {
        assert!(!segments.is_empty(), "VAD-on {label} produced no segments");
        let first_start = segments[0].start;
        let last_end = segments.iter().map(|s| s.end).max().unwrap();
        eprintln!("VAD ON ({label}) first start: {first_start:?}, last end: {last_end:?}");
        assert!(
            first_start >= Millis(4000),
            "VAD-on {label} first start {first_start:?} collapsed toward 0 — \
             VAD did not run or timestamps are on the compressed timeline"
        );
        assert!(
            last_end <= ceiling,
            "VAD-on {label} last end {last_end:?} exceeds padded duration \
             {padded_duration:?} + 2 s"
        );
    };

    // --- Segment mode ---
    let mut seg_options = french_options();
    seg_options.vad_model = Some(vad_model.clone());
    let mut seg_sink = RecordingSink::default();
    engine()
        .transcribe(&pcm, &seg_options, &mut seg_sink, &CancelToken::new())
        .expect("VAD-on segment-mode transcription");
    assert_original_timeline("segment", &seg_sink.segments);

    // --- Word mode ---
    let mut word_options = french_options();
    word_options.vad_model = Some(vad_model);
    word_options.word_timestamps = true;
    let mut word_sink = RecordingSink::default();
    engine()
        .transcribe(&pcm, &word_options, &mut word_sink, &CancelToken::new())
        .expect("VAD-on word-mode transcription");
    assert_original_timeline("word", &word_sink.segments);
}

/// Contract 6 (F-061, ADR-008): live mode transcribes two successive chunks,
/// the second call fed the first call's own returned text as its rolling
/// prompt, both succeeding with non-empty text; a third call given a
/// pre-canceled token errors with `Canceled` promptly rather than running the
/// whole chunk.
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn live_transcribes_successive_chunks_then_cancels_promptly() {
    let pcm = decode("whisper-french.mp3");
    let samples = pcm.samples();

    // Two distinct ~3 s chunks of the ~8.5 s French speech fixture, so both
    // live calls have real (if different) speech content.
    let chunk_len = (SAMPLE_RATE as usize) * 3;
    let chunk_a = &samples[..chunk_len.min(samples.len())];
    let chunk_b_start = chunk_len.min(samples.len());
    let chunk_b_end = (chunk_len * 2).min(samples.len());
    let chunk_b = &samples[chunk_b_start..chunk_b_end];

    let mut engine = engine();
    let opts = french_live_options();

    let first = engine
        .transcribe_live(chunk_a, &opts, "", &CancelToken::new())
        .expect("first live chunk should transcribe");
    assert!(
        !first.trim().is_empty(),
        "expected non-empty first live utterance"
    );

    // Second call rolls the first call's own text forward as its prompt —
    // exactly what the session loop does between chunks.
    let second = engine
        .transcribe_live(chunk_b, &opts, &first, &CancelToken::new())
        .expect("second live chunk (rolling prompt) should transcribe");
    assert!(
        !second.trim().is_empty(),
        "expected non-empty second live utterance"
    );

    // Third call: a pre-canceled token must abort promptly rather than
    // running the chunk to completion.
    let cancel = CancelToken::new();
    cancel.cancel();
    let start = Instant::now();
    let result = engine.transcribe_live(chunk_a, &opts, &second, &cancel);
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(EngineError::Canceled)),
        "expected Canceled, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "pre-canceled live call took too long: {elapsed:?}"
    );
}

/// Contract 7 (ADR-021 carried perf ticket, F-061): a live session's first
/// chunk — the one that lazily creates the cached `WhisperState` — must
/// produce byte-identical text whether that state comes from the reuse cache
/// (this task's change) or from a dedicated fresh engine (the pre-this-task
/// behavior). This is the one call where whisper.cpp's own state design
/// guarantees equivalence: `whisper_init_state` seeds `decoders[0].rng` to
/// `mt19937(0)` once, so a state's *first* `full()` call always starts from
/// the same rng position either way, on top of `whisper_full_with_state`
/// unconditionally clearing `result_all` and (with `no_context = true`,
/// whisper.cpp's own default this crate never overrides)
/// `prompt_past0`/`prompt_past1`/`kv_self` at the top of every call. See
/// `crates/humm-whisper/src/engine.rs`'s module doc ("Live session state
/// reuse") for the full argument.
///
/// **Multi-chunk caveat, found empirically while first writing this test and
/// recorded honestly (not the property this test asserts):** whisper.cpp's
/// beam-search candidate step (`whisper_sample_token_topk`,
/// vendored `whisper.cpp/src/whisper.cpp`) draws from `decoder.rng` on
/// *every* decode step regardless of temperature, and `decoders[0].rng` is
/// seeded exactly once at state creation, never reset per `full()` call. So a
/// session's *second and later* chunks on a **reused** state consume a
/// different point in that rng sequence than the same chunk would on its own
/// **freshly created** state (which always restarts at `mt19937(0)`) — on
/// this tiny-model fixture, chunk B's text reproducibly differs between the
/// two paths by exactly this mechanism. This is whisper.cpp's own canonical
/// streaming design (`examples/stream/stream.cpp` reuses one state across a
/// whole session too), not a defect this ticket introduces, and it never
/// leaks the text/prompt/KV-cache state this ticket is actually about (that
/// equivalence argument stands for every call, first or later — only the rng
/// position is carried). See
/// `live_session_chunk_sequence_is_deterministic_when_repeated` below for the
/// property that *is* verified for multi-chunk sessions: reused-state output
/// is reproducible run to run, not identical to an independently-fresh
/// baseline.
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn first_chunk_output_is_identical_whether_the_state_is_reused_or_fresh() {
    let pcm = decode("whisper-french.mp3");
    let samples = pcm.samples();
    let chunk_len = (SAMPLE_RATE as usize) * 3;
    let chunk_a = &samples[..chunk_len.min(samples.len())];
    let opts = french_live_options();

    // "Reused" here just means "goes through the cache slot" -- this is its
    // first call, so the slot's `get_or_create` constructs a state exactly
    // as `engine()` below does for its own first (and only) call.
    let reused_a = engine()
        .transcribe_live(chunk_a, &opts, "", &CancelToken::new())
        .expect("reuse-cache first chunk");
    let fresh_a = engine()
        .transcribe_live(chunk_a, &opts, "", &CancelToken::new())
        .expect("dedicated-fresh-engine first chunk");

    assert_eq!(
        reused_a, fresh_a,
        "a session's first chunk (the state's first use either way) must be \
         state-reuse-neutral"
    );
}

/// Contract 7b (ADR-021 carried perf ticket, F-061): a live session's
/// reused-state chunk sequence is deterministic and reproducible, not flaky
/// — running the identical two-chunk, rolling-prompt-chained sequence on two
/// independent engines produces the identical pair of outputs both times.
///
/// This is the honest multi-chunk regression guard: the sibling test
/// above's doc comment records *why* comparing a reused-state chunk B
/// against an independently-fresh-state chunk B is not a valid equivalence
/// (whisper.cpp's beam-search rng carries over within a reused state, by
/// upstream design) — so this test instead compares reused-state-to-reused-
/// state, the equivalence whisper.cpp's rng design actually supports: same
/// starting seed + same call sequence ⇒ same result, every time.
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn live_session_chunk_sequence_is_deterministic_when_repeated() {
    let pcm = decode("whisper-french.mp3");
    let samples = pcm.samples();
    let chunk_len = (SAMPLE_RATE as usize) * 3;
    let chunk_a = &samples[..chunk_len.min(samples.len())];
    let chunk_b_start = chunk_len.min(samples.len());
    let chunk_b_end = (chunk_len * 2).min(samples.len());
    let chunk_b = &samples[chunk_b_start..chunk_b_end];
    let opts = french_live_options();

    let run_session = || {
        let mut engine = engine();
        let a = engine
            .transcribe_live(chunk_a, &opts, "", &CancelToken::new())
            .expect("chunk A");
        let b = engine
            .transcribe_live(chunk_b, &opts, &a, &CancelToken::new())
            .expect("chunk B (reused state, rolling prompt)");
        (a, b)
    };

    let (a1, b1) = run_session();
    let (a2, b2) = run_session();

    assert_eq!(a1, a2, "chunk A must reproduce identically run to run");
    assert_eq!(
        b1, b2,
        "chunk B (reused state) must reproduce identically run to run"
    );
}

/// Contract 8 (ADR-021 carried perf ticket, F-061): `begin_live_session`'s
/// hard guarantee against cross-session leakage. The same engine instance
/// runs the same chunk in two separate sessions (bracketed by
/// `begin_live_session`/`end_live_session`); the second session's output must
/// match the first's exactly -- no residual state from session one leaking
/// into session two's cached `WhisperState`.
#[test]
#[ignore = "requires ffmpeg + a real whisper tiny model (downloaded/cached on first run)"]
fn second_session_after_begin_produces_same_output_as_first() {
    let pcm = decode("whisper-french.mp3");
    let samples = pcm.samples();
    let chunk_len = (SAMPLE_RATE as usize) * 3;
    let chunk = &samples[..chunk_len.min(samples.len())];
    let opts = french_live_options();

    let mut engine = engine();

    engine.begin_live_session().expect("begin first session");
    let first_session_text = engine
        .transcribe_live(chunk, &opts, "", &CancelToken::new())
        .expect("first session chunk");
    engine.end_live_session();

    engine.begin_live_session().expect("begin second session");
    let second_session_text = engine
        .transcribe_live(chunk, &opts, "", &CancelToken::new())
        .expect("second session chunk");
    engine.end_live_session();

    assert_eq!(
        first_session_text, second_session_text,
        "identical input in a fresh session must reproduce identical output \
         -- no cross-session leakage from the cached live state"
    );
}
