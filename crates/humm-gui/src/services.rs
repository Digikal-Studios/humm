//! Wires the GUI's runtime services: settings, store, the task queue, and
//! environment capabilities (design spec §3.1/§4).
//!
//! [`init`] mirrors `humm-cli`'s startup order
//! (`crates/humm-cli/src/run.rs::execute`): settings → store → one-time Buzz
//! import → instance-lock-gated stale-task flip → queue spawn. The GUI
//! additionally probes `ffmpeg`/`ffprobe`/`yt-dlp` availability
//! ([`Capabilities`]) so later tasks can gate URL-import UI with a tooltip
//! (Policy rule 6), and stubs the update-check notice channel
//! ([`Services::update_rx`]) for Task 11 to fill in.
//!
//! Unlike the CLI, the GUI never resolves a model before enqueueing — model
//! acquisition is meant to happen *inside* the task run so an absent model
//! never blocks other imports (design spec §2.1). That machinery
//! (`ArtifactEnsurer`) lands with the real engine wiring later in M3; until
//! then [`init`] configures a placeholder [`EngineFactory`] whose `create`
//! always fails actionably, so an accepted-but-unwireable transcription task
//! fails cleanly instead of silently doing nothing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use humm_core::boundary::{MediaFetcher, TaskStore, TranscriptionEngine};
use humm_core::domain::ModelSpec;
use humm_core::error::{CoreError, EngineError, StoreError};
use humm_core::paths;
use humm_core::pipeline::EngineTuning;
use humm_core::queue::{EngineFactory, QueueHandle, RunnerDeps, TaskEvent, spawn_task_runner};
use humm_core::settings::{Settings, SettingsError};
use humm_media::{FfmpegDecoder, FfprobeProber, MediaCapabilities, YtDlpFetcher, probe_binaries};
use humm_store::SqliteStore;

/// ffmpeg/ffprobe/yt-dlp availability, probed once at startup.
///
/// Plain booleans (rather than [`humm_media::MediaCapabilities`]'s version
/// strings) because the views only ever need "is this feature available",
/// e.g. to gray out URL import with a tooltip when `yt_dlp` is `false`
/// (Policy rule 6: missing binaries degrade gracefully, never crash).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// Whether ffmpeg was found (media decode).
    pub ffmpeg: bool,
    /// Whether ffprobe was found (media probing).
    pub ffprobe: bool,
    /// Whether yt-dlp was found (URL import).
    pub yt_dlp: bool,
}

impl From<&MediaCapabilities> for Capabilities {
    fn from(media: &MediaCapabilities) -> Self {
        Capabilities {
            ffmpeg: media.ffmpeg_version.is_some(),
            ffprobe: media.ffprobe_version.is_some(),
            yt_dlp: media.yt_dlp.is_some(),
        }
    }
}

/// A pending update notice from the background update-check thread (Task
/// 11 wires the sender; this channel exists now so [`Services`] and
/// `HummApp` already have somewhere to receive it from).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateNotice {
    /// The newer version string the update check reported.
    pub version: String,
}

/// Everything `HummApp` needs to run. Constructed once at startup, by
/// [`init`] (real adapters) or [`init_with`] (fakes, for kittest).
pub struct Services {
    /// Loaded settings, shared so views can read/mutate and persist changes
    /// (`Settings::save` at `settings_path`).
    pub settings: Arc<Mutex<Settings>>,
    /// Where `settings` was loaded from and is saved back to.
    pub settings_path: PathBuf,
    /// The persistent task store.
    pub store: Arc<SqliteStore>,
    /// Handle to the running task queue (enqueue/cancel/retry/subscribe).
    pub queue: QueueHandle,
    /// Task lifecycle events, drained by the event-pump thread (`events.rs`).
    pub events: flume::Receiver<TaskEvent>,
    /// ffmpeg/ffprobe/yt-dlp availability.
    pub capabilities: Capabilities,
    /// Root directory for downloaded whisper models.
    pub model_root: PathBuf,
    /// Update-check notices. Empty (no sender) until Task 11 wires the
    /// background check; receiving from it simply never resolves until then.
    pub update_rx: flume::Receiver<UpdateNotice>,
}

/// Errors that can prevent the GUI from starting, rendered by `main.rs`'s
/// minimal error app (design spec §3.1: never a panic).
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// Settings failed to load (corrupt file, I/O error).
    #[error("could not load settings: {0}")]
    Settings(#[from] SettingsError),
    /// The store's database failed to open.
    #[error("could not open the database: {0}")]
    Store(#[from] StoreError),
}

/// Builds [`Services`] with real adapters, mirroring the CLI's startup order
/// (`crates/humm-cli/src/run.rs::execute`): settings → store → one-time Buzz
/// import → instance-lock-gated stale-task flip → queue spawn.
///
/// `sole_instance` is threaded in from `main.rs`'s
/// `humm_core::instance::acquire_instance_lock` call — the lock guard itself
/// is held by `main` for the process lifetime; this function only needs the
/// *decision* that call already made (ADR-017).
pub fn init(sole_instance: bool) -> Result<Services, InitError> {
    let settings_path = paths::settings_path();
    let settings = Settings::load(&settings_path)?;

    let store = Arc::new(SqliteStore::open(&paths::db_path())?);
    import_buzz_history(&store);
    apply_stale_flip(&store, sole_instance);

    let media_caps = probe_binaries();
    let capabilities = Capabilities::from(&media_caps);

    let decoder = Arc::new(FfmpegDecoder::new());
    let prober = Arc::new(FfprobeProber::new());
    let fetcher = build_fetcher(&settings, &media_caps);
    let model_root = paths::model_root();

    let deps = RunnerDeps {
        store: Arc::clone(&store) as Arc<dyn TaskStore>,
        decoder,
        prober,
        fetcher,
        engines: Box::new(UnwiredEngineFactory),
        tuning: EngineTuning::default(),
        media_dir: paths::media_dir(),
        // No ArtifactEnsurer yet: the real engine/model wiring (and the
        // non-modal download machinery it enables, design spec §2.1) lands
        // later in M3. Until then `UnwiredEngineFactory` fails every job
        // actionably rather than silently doing nothing.
        artifacts: None,
    };
    let queue = spawn_task_runner(deps);
    let events = queue.subscribe();

    // No sender is ever constructed for `update_rx` this task — Task 11
    // wires the real background check and its sender. Receiving from an
    // orphaned channel just never resolves, which is exactly "no update
    // notices yet".
    let (_update_tx, update_rx) = flume::unbounded();

    Ok(Services {
        settings: Arc::new(Mutex::new(settings)),
        settings_path,
        store,
        queue,
        events,
        capabilities,
        model_root,
        update_rx,
    })
}

/// Test-only inputs for [`init_with`]: a caller-supplied store/engine
/// factory/fetcher (so a kittest task can script transcription outcomes
/// hermetically) plus the directories/settings a real [`init`] would have
/// resolved from the environment.
pub struct TestDeps {
    /// The (already-opened, typically temp-file-or-in-memory) store.
    pub store: Arc<SqliteStore>,
    /// Directory URL-import downloads would be written under.
    pub media_dir: PathBuf,
    /// Directory downloaded models would live under (unused unless a test's
    /// engine factory reads it).
    pub model_root: PathBuf,
    /// Starting settings (defaults to `Settings::default()`).
    pub settings: Settings,
    /// Where `settings` "would" be saved — a temp path, never a real one.
    pub settings_path: PathBuf,
    /// The engine factory the runner uses — a fake, scripted per test.
    pub engines: Box<dyn EngineFactory>,
    /// URL fetcher, if the test needs one (`None` makes URL tasks fail
    /// actionably, same as a real machine with no yt-dlp).
    pub fetcher: Option<Arc<dyn MediaFetcher>>,
    /// Reported capabilities (defaults to all-`false`; a test overrides this
    /// to simulate ffmpeg/yt-dlp being present).
    pub capabilities: Capabilities,
}

/// Builds [`Services`] from caller-supplied fakes/temp state — no real
/// settings file, database, or subprocess is ever touched. Used by kittest
/// harnesses (this task's smoke test, and every later kittest task that
/// constructs `HummApp` via [`crate::app::HummApp::new_for_test`]).
pub fn init_with(deps: TestDeps) -> Services {
    let decoder = Arc::new(FfmpegDecoder::new());
    let prober = Arc::new(FfprobeProber::new());

    let runner_deps = RunnerDeps {
        store: Arc::clone(&deps.store) as Arc<dyn TaskStore>,
        decoder,
        prober,
        fetcher: deps.fetcher,
        engines: deps.engines,
        tuning: EngineTuning::default(),
        media_dir: deps.media_dir,
        artifacts: None,
    };
    let queue = spawn_task_runner(runner_deps);
    let events = queue.subscribe();
    let (_update_tx, update_rx) = flume::unbounded();

    Services {
        settings: Arc::new(Mutex::new(deps.settings)),
        settings_path: deps.settings_path,
        store: deps.store,
        queue,
        events,
        capabilities: deps.capabilities,
        model_root: deps.model_root,
        update_rx,
    }
}

/// Runs the one-time Buzz import when a Buzz database is present and the
/// import hasn't already completed. Non-fatal by contract: failures are
/// logged, never surfaced (same as `humm-cli`'s `import_buzz_history`). Must
/// run *before* the stale-task flip so an imported stale row is flipped like
/// any other.
fn import_buzz_history(store: &SqliteStore) {
    match store.buzz_import_done() {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            log::warn!("could not check Buzz-import state (skipping import): {err}");
            return;
        }
    }
    let Some(buzz_db) = paths::buzz_sqlite_path() else {
        return;
    };
    match store.import_buzz_db(&buzz_db) {
        Ok(outcome) => log::info!(
            "imported Buzz history from {}: {} task(s), {} segment(s)",
            buzz_db.display(),
            outcome.tasks,
            outcome.segments
        ),
        Err(err) => log::warn!("Buzz import failed (continuing without it): {err}"),
    }
}

/// Runs `cancel_stale_tasks` only when `sole_instance` says this process won
/// the exclusive instance-lock claim at startup (ADR-017) — otherwise a
/// second, still-running humm process's genuine `in_progress` rows must not
/// be flipped out from under it (`docs/HUMM_SURFACE.md` §7).
fn apply_stale_flip(store: &SqliteStore, sole_instance: bool) {
    if !sole_instance {
        log::info!("another humm process is running — skipping stale-task recovery");
        return;
    }
    match store.cancel_stale_tasks() {
        Ok(0) => {}
        Ok(n) => log::info!("recovered {n} stale task(s) from a prior run as canceled"),
        Err(err) => log::warn!("cancel_stale_tasks failed (continuing): {err}"),
    }
}

/// Builds the URL fetcher when yt-dlp is available, else `None` (URL tasks
/// then fail actionably — the queue's own `YTDLP_MISSING` message). Cookie
/// file resolves `HUMM_DOWNLOAD_COOKIEFILE` (env wins) then
/// `settings.downloads.cookie_file` — same precedence as the CLI.
fn build_fetcher(settings: &Settings, caps: &MediaCapabilities) -> Option<Arc<dyn MediaFetcher>> {
    caps.yt_dlp.as_ref()?;
    let cookie_file = std::env::var_os("HUMM_DOWNLOAD_COOKIEFILE")
        .map(PathBuf::from)
        .or_else(|| settings.downloads.cookie_file.clone());
    Some(Arc::new(YtDlpFetcher::new(cookie_file)))
}

/// The [`EngineFactory`] `init` configures until the real whisper-rs wiring
/// lands later in M3: every `create` call fails with an actionable
/// [`CoreError`] rather than silently doing nothing, so an accepted
/// transcription task still reaches a clean `Failed` terminal state instead
/// of hanging or panicking.
struct UnwiredEngineFactory;

impl EngineFactory for UnwiredEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        Err(CoreError::Engine(EngineError::Inference {
            detail: "the transcription engine isn't wired up yet in this build of humm — engine \
                     wiring lands later in this milestone; imports are accepted but will fail \
                     until then"
                .to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::{ModelSize, TranscriptionId};
    use humm_core::error::CoreError;
    use pretty_assertions::assert_eq;
    use std::time::{Duration, Instant};

    #[test]
    fn unwired_engine_factory_fails_actionably() {
        let mut factory = UnwiredEngineFactory;
        let model = ModelSpec {
            size: ModelSize::Small,
            hf_repo: None,
        };
        // `Box<dyn TranscriptionEngine>` isn't `Debug`, so `Result::expect_err`
        // (which requires the `Ok` side to be `Debug`) doesn't apply here.
        let err = match factory.create(&model, None) {
            Ok(_) => panic!("expected UnwiredEngineFactory::create to fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("not wired up") || err.to_string().contains("isn't wired"),
            "expected an actionable engine-not-wired message, got: {err}"
        );
    }

    fn test_deps(store: Arc<SqliteStore>) -> TestDeps {
        struct FailingFactory;
        impl EngineFactory for FailingFactory {
            fn create(
                &mut self,
                _model: &ModelSpec,
                _resolved: Option<&Path>,
            ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
                Err(CoreError::Canceled)
            }
        }
        TestDeps {
            store,
            // Unused by this test: the enqueued task is a `Input::Path`, and
            // `media_dir` is only ever touched by the runner's URL-fetch path.
            media_dir: PathBuf::from("/unused-media-dir"),
            model_root: PathBuf::from("/unused-model-root"),
            settings: Settings::default(),
            settings_path: PathBuf::from("/unused-settings-path"),
            engines: Box::new(FailingFactory),
            fetcher: None,
            capabilities: Capabilities::default(),
        }
    }

    #[test]
    fn init_with_builds_a_working_queue_and_subscription() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
        let services = init_with(test_deps(Arc::clone(&store)));

        let id = TranscriptionId::new();
        let spec = humm_core::domain::TaskSpec {
            input: humm_core::domain::Input::Path(PathBuf::from("/does/not/matter.wav")),
            options: humm_core::domain::TranscriptionOptions::default(),
            output: humm_core::domain::OutputSpec::default(),
        };
        services
            .queue
            .enqueue(id, spec, humm_core::domain::Source::FileImport);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_queued = false;
        while Instant::now() < deadline {
            match services.events.recv_timeout(Duration::from_millis(200)) {
                Ok(event) if event.task_id == id => {
                    if matches!(event.kind, humm_core::queue::TaskEventKind::Queued) {
                        saw_queued = true;
                    }
                    if !matches!(
                        event.kind,
                        humm_core::queue::TaskEventKind::Queued
                            | humm_core::queue::TaskEventKind::Started
                    ) {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_queued, "expected a Queued event for the enqueued task");
        let _ = services.queue.finish();
    }

    #[test]
    fn capabilities_from_media_capabilities_maps_presence_to_booleans() {
        let media = MediaCapabilities {
            ffmpeg_version: Some("8.1.1".to_string()),
            ffprobe_version: None,
            yt_dlp: Some("2026.03.17".to_string()),
        };
        let caps = Capabilities::from(&media);
        assert_eq!(
            caps,
            Capabilities {
                ffmpeg: true,
                ffprobe: false,
                yt_dlp: true,
            }
        );
    }
}
