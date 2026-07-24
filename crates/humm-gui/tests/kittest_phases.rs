//! kittest coverage for the F-025 download-phase display end to end through
//! the *real* task queue (Task 10): a scripted [`ArtifactEnsurer`] and
//! [`MediaFetcher`] each emit one progress update, gated so the test can
//! deterministically observe the row's status cell mid-phase before letting
//! the job proceed — proving `TaskEventKind::DownloadProgress` /
//! `ModelDownloadProgress` genuinely flow from the runner thread through the
//! real event pump into the rendered table (Task 7's `format.rs`/
//! `viewmodel.rs` phase text, unit-tested in isolation there, actually
//! reaches the screen here).
//!
//! Unlike every other kittest file's fakes (which return instantly), this
//! one's fetcher/ensurer block on a rendezvous [`Gate`] *after* emitting
//! their one scripted progress update — without that, the runner thread
//! could race straight through `Started` → `DownloadProgress` →
//! `ModelDownloadProgress` → `Completed` before the test ever calls
//! `harness.run()` again, collapsing every intermediate phase into a single
//! drained batch and leaving nothing to assert on.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::{Harness, HarnessBuilder, kittest::Queryable};
use humm_core::boundary::{
    EngineCaps, EngineOptions, FetchProgress, MediaFetcher, Pcm, SegmentSink, TranscriptionEngine,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, Input, ModelSize, ModelSpec, OutputSpec, Source, TaskKind,
    TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::error::{CoreError, EngineError};
use humm_core::models::download::DownloadProgress;
use humm_core::queue::{ArtifactEnsurer, EngineFactory};
use humm_gui::app::HummApp;
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_store::SqliteStore;

// -- Rendezvous gate: blocks the runner thread until the test releases it ---

/// One rendezvous point: [`Gate::wait`] (called from the runner thread)
/// signals it has been entered, then blocks until [`GateControl::release`]
/// (called from the test thread, after it has confirmed — via
/// [`GateControl::wait_entered`] — that the progress update preceding the
/// `wait` call has already been emitted).
struct Gate {
    entered_tx: flume::Sender<()>,
    proceed_rx: flume::Receiver<()>,
}

impl Gate {
    fn wait(&self) {
        let _ = self.entered_tx.send(());
        let _ = self.proceed_rx.recv();
    }
}

struct GateControl {
    entered_rx: flume::Receiver<()>,
    proceed_tx: flume::Sender<()>,
}

impl GateControl {
    fn wait_entered(&self) {
        self.entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("gate was never entered");
    }

    fn release(&self) {
        let _ = self.proceed_tx.send(());
    }
}

fn gate() -> (GateControl, Gate) {
    let (entered_tx, entered_rx) = flume::bounded(1);
    let (proceed_tx, proceed_rx) = flume::bounded(1);
    (
        GateControl {
            entered_rx,
            proceed_tx,
        },
        Gate {
            entered_tx,
            proceed_rx,
        },
    )
}

// -- Scripted fetcher/ensurer: one progress step, then block ----------------

/// Real Buzz-sourced audio testdata, same convention as
/// `kittest_actions.rs`'s own `testdata_dir` — overridable via
/// `HUMM_BUZZ_TESTDATA`. `init_with` hard-codes the *real* ffmpeg/ffprobe
/// decoder/prober (only `engines`/`fetcher`/`artifacts` are fakeable via
/// `TestDeps`), so the "downloaded" file must be genuinely decodable or the
/// job fails at the decode step rather than ever reaching `Completed`.
fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

/// Emits one `FetchProgress` at 50%, then blocks on `gate` before copying a
/// real, decodable audio file into place (see [`testdata_dir`]'s doc comment
/// for why a dummy byte string won't do).
struct GatedFetcher {
    gate: Gate,
}

impl MediaFetcher for GatedFetcher {
    fn fetch(
        &self,
        _url: &url::Url,
        dest_dir: &Path,
        progress: &mut dyn FnMut(FetchProgress),
        _cancel: &CancelToken,
    ) -> Result<PathBuf, humm_core::error::MediaError> {
        progress(FetchProgress {
            downloaded: 50,
            total: Some(100),
        });
        self.gate.wait();
        let src = testdata_dir().join("whisper-latvian.wav");
        let path = dest_dir.join("audio.wav");
        std::fs::copy(&src, &path).map_err(humm_core::error::MediaError::Io)?;
        Ok(path)
    }
}

/// Emits one `ModelDownloadProgress`-worth of `DownloadProgress` at 42%, then
/// blocks on `gate` before resolving to `model_path`. `ensure_vad` returns
/// `None` immediately (this test isn't exercising the VAD phase).
struct GatedEnsurer {
    gate: Gate,
    model_path: PathBuf,
}

impl ArtifactEnsurer for GatedEnsurer {
    fn ensure_model(
        &self,
        _model: &ModelSpec,
        progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError> {
        progress(DownloadProgress {
            file: "ggml-small.bin".to_string(),
            downloaded: 42,
            total: Some(100),
        });
        self.gate.wait();
        Ok(self.model_path.clone())
    }

    fn ensure_vad(
        &self,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Option<PathBuf> {
        None
    }
}

/// Succeeds instantly with zero segments once the engine is finally reached
/// — this test cares about the two download phases, not transcription
/// itself, and (per the task brief) a real `WhisperEngine` must never be
/// constructed/invoked under kittest (GPU init segfaults in this sandbox).
struct FakeEngine;

impl TranscriptionEngine for FakeEngine {
    fn transcribe(
        &mut self,
        _audio: &Pcm,
        _options: &EngineOptions,
        _sink: &mut dyn SegmentSink,
        _cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        Ok(())
    }

    fn capabilities(&self) -> EngineCaps {
        EngineCaps {
            word_timestamps: false,
            translate: false,
            vad: false,
        }
    }
}

struct FakeEngineFactory;

impl EngineFactory for FakeEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        assert!(
            resolved.is_some(),
            "the ensurer's resolved path must reach the engine factory"
        );
        Ok(Box::new(FakeEngine))
    }
}

fn sample_spec(input: Input) -> TaskSpec {
    TaskSpec {
        input,
        options: TranscriptionOptions {
            language: None,
            task: TaskKind::Transcribe,
            model: ModelSpec {
                backend: Backend::Whisper,
                size: ModelSize::Small,
                hf_repo: None,
            },
            word_timestamps: false,
            initial_prompt: String::new(),
            temperature: DEFAULT_TEMPERATURE.to_vec(),
            llm: None,
        },
        output: OutputSpec::default(),
    }
}

/// Widened well past the table's full column width, same convention as
/// `kittest_table.rs`'s `wide_harness` (the status cell's text must be
/// reachable by `get_by_label_contains`).
fn wide_harness(services: Services) -> Harness<'static, HummApp> {
    HarnessBuilder::default()
        .with_size(egui::Vec2::new(1700.0, 400.0))
        // See `kittest_import.rs`'s `harness_with` for why this is raised
        // above the default of 4 (final-review fix 5) — this file's whole
        // point is a real, gated queue/event-pump thread driving the UI.
        .with_max_steps(32)
        .build_eframe(move |_cc| HummApp::new_for_test(services))
}

/// Polls `harness.run()` until a node whose label contains `needle` appears,
/// or panics after a 10s deadline. The events driving this are produced on
/// the queue's own runner thread and forwarded by the event-pump thread
/// (`events.rs`), so — even past a [`GateControl::wait_entered`]
/// rendezvous, which only proves the *runner* has emitted the event — a
/// short real-wall-clock wait for the pump thread to move it onto the
/// egui-visible buffer is expected, not a bug (same pattern as
/// `kittest_actions.rs`'s `retry_on_a_failed_row_reaches_completion`).
fn wait_for_label_contains(harness: &mut Harness<'static, HummApp>, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.run();
        if harness.query_by_label_contains(needle).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a label containing {needle:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn download_phases_render_media_then_model_then_complete() {
    let wav = testdata_dir().join("whisper-latvian.wav");
    assert!(wav.is_file(), "missing testdata: {}", wav.display());

    let store = Arc::new(SqliteStore::open_in_memory().expect("open in-memory store"));
    let store_for_assert = Arc::clone(&store);
    let media_dir = tempfile::tempdir().expect("media tempdir");

    let (fetch_control, fetch_gate) = gate();
    let (model_control, model_gate) = gate();

    let services = services::init_with(TestDeps {
        store,
        media_dir: media_dir.path().to_path_buf(),
        model_root: media_dir.path().join("models"),
        settings: humm_core::settings::Settings::default(),
        settings_path: media_dir.path().join("settings.toml"),
        engines: Box::new(FakeEngineFactory),
        fetcher: Some(Arc::new(GatedFetcher { gate: fetch_gate })),
        capabilities: Capabilities::default(),
        artifacts: Some(Arc::new(GatedEnsurer {
            gate: model_gate,
            model_path: PathBuf::from("/unused/ggml-small.bin"),
        })),
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    });

    // Enqueued directly against `services.queue()` *before* `services` moves
    // into the harness below (`QueueHandle::enqueue` only needs `&self`):
    // the command channel and the runner thread already exist (`init_with`
    // spawned them), so this is queued immediately regardless of whether
    // anything has drained an event yet — `flume`'s channels are unbounded/
    // buffered, so nothing is lost waiting for the event pump to spin up on
    // the harness's first frame below.
    let id = TranscriptionId::new();
    let url: url::Url = "https://example.com/clip.mp3".parse().expect("valid url");
    services
        .queue()
        .enqueue(id, sample_spec(Input::Url(url)), Source::UrlImport);

    let mut harness = wide_harness(services);
    harness.run(); // spawns the (lazily-created) event pump

    // Phase 1: media download. The runner thread has emitted `DownloadProgress`
    // and is now blocked in `GatedFetcher::fetch`'s `gate.wait()` the moment
    // this returns.
    fetch_control.wait_entered();
    wait_for_label_contains(&mut harness, "Downloading media (50%)");
    fetch_control.release();

    // Phase 2: model download, same rendezvous.
    model_control.wait_entered();
    wait_for_label_contains(&mut harness, "Downloading model (42%)");
    model_control.release();

    // Terminal: Completed. Polls the store directly for the authoritative
    // status (not a UI label): the "Date Completed" column *header* itself
    // contains the substring "Completed" and is on screen from the very
    // first frame, so a bare `wait_for_label_contains(harness, "Completed")`
    // would trivially match it regardless of whether the row itself ever
    // reaches a terminal state (`kittest_table.rs`'s own tests hit this same
    // ambiguity and use an exact-match label for it; the store's `status`
    // column has no such ambiguity at all).
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.run();
        let status = store_for_assert
            .get_task(&id)
            .expect("get_task")
            .expect("row still exists")
            .status;
        if status == humm_core::domain::Status::Completed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "task did not reach Completed in time (currently {status:?})"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Light UI sanity check: the row's own status cell now contributes a
    // second "…Completed…" match beyond the "Date Completed" header (same
    // technique as `kittest_actions.rs`'s `retry_on_a_failed_row_reaches_completion`).
    harness.run();
    assert!(
        harness.query_all_by_label_contains("Completed").count() >= 2,
        "expected the row's own status cell to also render something containing \"Completed\""
    );
}
