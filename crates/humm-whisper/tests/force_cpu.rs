//! CPU loads must not probe CoreML.
//!
//! whisper.cpp 1.8.3 calls `whisper_coreml_init` from `whisper_init_state`
//! even when the context was opened with `use_gpu(false)`. On Apple Silicon
//! that probe SIGSEGVs under `HUMM_FORCE_CPU` (missing encoder and a present
//! encoder both crash). The workspace patch skips the load and logs
//! `skipping Core ML because use_gpu = 0`.
//!
//! Uses Buzz's stub `ggml-tiny.bin` (same fixture as the hermetic CLI tests)
//! plus a sibling `ggml-tiny-encoder.mlmodelc` directory, so a regression
//! that still calls `whisper_coreml_init` has a path to open.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use humm_core::boundary::{EngineOptions, Pcm, SAMPLE_RATE, SegmentSink, TranscriptionEngine};
use humm_core::cancel::CancelToken;
use humm_core::domain::{Segment, TaskKind};
use humm_core::language::Language;
use humm_whisper::WhisperEngine;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        LINES
            .lock()
            .expect("log lines")
            .push(record.args().to_string());
    }

    fn flush(&self) {}
}

static LOGGER: Capture = Capture;

fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

fn install_logger() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
}

fn logs() -> String {
    LINES.lock().expect("log lines").join("\n")
}

struct NullSink;

impl SegmentSink for NullSink {
    fn on_segment(&mut self, _segment: Segment) {}
    fn on_progress(&mut self, _fraction: f32) {}
}

fn cpu_options() -> EngineOptions {
    EngineOptions {
        language: Language::parse("en"),
        task: TaskKind::Transcribe,
        initial_prompt: String::new(),
        word_timestamps: false,
        temperature: vec![0.0],
        vad_model: None,
        threads: Some(2),
    }
}

/// Copies the stub ggml next to an encoder directory whisper would open if
/// `whisper_coreml_init` still ran.
fn stage_stub(dir: &Path) {
    let stub = testdata_dir().join("ggml-tiny.bin");
    assert!(
        stub.is_file(),
        "missing Buzz stub model fixture: {} (set HUMM_BUZZ_TESTDATA)",
        stub.display()
    );
    fs::copy(&stub, dir.join("ggml-tiny.bin")).expect("copy stub ggml");
    let encoder = dir.join("ggml-tiny-encoder.mlmodelc");
    fs::create_dir_all(&encoder).expect("create fake CoreML encoder dir");
    fs::write(encoder.join("weights.bin"), b"not a coreml model").expect("write fake encoder");
}

#[test]
fn cpu_load_skips_coreml_and_transcribes_stub() {
    install_logger();
    let dir = tempfile::tempdir().expect("tempdir");
    stage_stub(dir.path());

    let mut engine = WhisperEngine::load(&dir.path().join("ggml-tiny.bin"), false)
        .expect("CPU load of the stub model");
    let pcm = Pcm::new(vec![0.0; SAMPLE_RATE as usize]);
    engine
        .transcribe(&pcm, &cpu_options(), &mut NullSink, &CancelToken::new())
        .unwrap_or_else(|err| panic!("CPU transcribe failed: {err}\nlogs:\n{}", logs()));

    let text = logs();
    assert!(
        !text.contains("loading Core ML model"),
        "use_gpu false must not probe CoreML\n{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.contains("use gpu") && line.contains("= 0")),
        "context should record use_gpu = 0\n{text}"
    );
    #[cfg(target_os = "macos")]
    assert!(
        text.contains("skipping Core ML because use_gpu = 0"),
        "Apple Silicon CPU load should take the patched skip path\n{text}"
    );
}
