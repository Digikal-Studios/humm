//! End-to-end decode + transcribe benchmark (Task 9: criterion suite + M1
//! baselines). **This is THE regression-tracked transcription path** —
//! `docs/design/DECISIONS.md` / `CLAUDE.md` treat a regression here as a
//! blocking failure, not a note to log.
//!
//! Decodes Buzz's `whisper-french.mp3` fixture via the real `ffmpeg`
//! subprocess and transcribes it with a REAL whisper `tiny` model (GPU on)
//! resolved from the default `model_root()` via
//! `models::registry::local_model_path`. This bench never downloads the
//! model itself — Buzz's testdata `ggml-tiny.bin` is a 575 KB stub that
//! loads but transcribes to zero segments, so it is never used here either.
//! Skips (printing a line, registering no measurements) when ffmpeg, the
//! Buzz testdata, or a real cached tiny model is unavailable.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

use humm_core::boundary::{AudioDecoder, EngineOptions, SegmentSink, TranscriptionEngine};
use humm_core::cancel::CancelToken;
use humm_core::domain::{ModelSize, Segment, TaskKind};
use humm_core::language::Language;
use humm_core::models::registry::local_model_path;
use humm_core::paths::model_root;
use humm_media::FfmpegDecoder;
use humm_whisper::WhisperEngine;

/// Discards every segment/progress update — this bench measures timing only.
struct NullSink;

impl SegmentSink for NullSink {
    fn on_segment(&mut self, _segment: Segment) {}
    fn on_progress(&mut self, _fraction: f32) {}
}

/// Root of Buzz's test fixtures, honoring `HUMM_BUZZ_TESTDATA` (Global
/// Constraint 4), same default as the contract tests.
fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

/// Resolves the ffmpeg binary the same way `FfmpegDecoder::new` does
/// (`HUMM_FFMPEG` override, else `ffmpeg` on `PATH`).
fn ffmpeg_binary() -> PathBuf {
    std::env::var_os("HUMM_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// `true` iff the resolved ffmpeg binary can actually be spawned.
fn ffmpeg_available() -> bool {
    Command::new(ffmpeg_binary())
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Base options: transcribe French, GPU on — matches the contract tests'
/// `french_options`.
fn french_options() -> EngineOptions {
    EngineOptions {
        language: Language::parse("fr"),
        task: TaskKind::Transcribe,
        initial_prompt: String::new(),
        word_timestamps: false,
        temperature: vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
        vad_model: None,
        threads: None,
        use_gpu: true,
    }
}

fn bench_e2e(c: &mut Criterion) {
    let audio_path = testdata_dir().join("whisper-french.mp3");

    if !ffmpeg_available() {
        println!(
            "SKIP humm-whisper e2e bench: ffmpeg not found — install it (e.g. `brew install ffmpeg`) or set HUMM_FFMPEG"
        );
        return;
    }
    if !audio_path.is_file() {
        println!(
            "SKIP humm-whisper e2e bench: Buzz testdata not found at {} — set HUMM_BUZZ_TESTDATA",
            audio_path.display()
        );
        return;
    }

    let root = model_root();
    let Some(model_path) = local_model_path(&root, ModelSize::Tiny) else {
        println!(
            "SKIP humm-whisper e2e bench: no real whisper tiny model cached at {} — this bench \
             never downloads one; run `humm add <file> -s tiny` once (or the humm-whisper \
             contract tests) to populate the cache",
            root.display()
        );
        return;
    };

    let mut engine = match WhisperEngine::load(&model_path, true) {
        Ok(engine) => engine,
        Err(err) => {
            println!("SKIP humm-whisper e2e bench: failed to load tiny model: {err}");
            return;
        }
    };

    let decoder = FfmpegDecoder::new();
    let options = french_options();

    let mut group = c.benchmark_group("e2e");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(60));
    group.bench_function("decode_transcribe/whisper-french.mp3/tiny", |b| {
        b.iter(|| {
            let cancel = CancelToken::new();
            let pcm = decoder
                .decode(std::hint::black_box(&audio_path), &cancel)
                .expect("decode should succeed");
            let mut sink = NullSink;
            engine
                .transcribe(std::hint::black_box(&pcm), &options, &mut sink, &cancel)
                .expect("transcribe should succeed");
        });
    });
    group.finish();
}

criterion_group!(benches, bench_e2e);
criterion_main!(benches);
