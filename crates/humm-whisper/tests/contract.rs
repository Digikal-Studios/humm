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
