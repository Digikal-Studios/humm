//! Wires the GUI's runtime services: settings, store, the task queue, and
//! environment capabilities (design spec §3.1/§4).
//!
//! [`init`] mirrors `humm-cli`'s startup order
//! (`crates/humm-cli/src/run.rs::execute`): settings → store → one-time Buzz
//! import → instance-lock-gated stale-task flip → queue spawn → (if due) the
//! background update check (F-006, ADR-015, [`maybe_spawn_startup_update_check`]).
//! The GUI additionally probes `ffmpeg`/`ffprobe`/`yt-dlp` availability
//! ([`Capabilities`]) so later tasks can gate URL-import UI with a tooltip
//! (Policy rule 6). The one-per-launch telemetry event (F-007, ADR-009) is
//! deliberately *not* fired from here — see [`fire_launch_telemetry`]'s doc
//! comment for why.
//!
//! Unlike the CLI, the GUI never resolves a model before enqueueing — model
//! acquisition happens *inside* the task run instead, via
//! [`crate::engine_factory::DownloaderEnsurer`] (`RunnerDeps::artifacts`), so
//! an absent model never blocks other imports (design spec §2.1); the engine
//! itself is built by [`crate::engine_factory::GuiEngineFactory`] from
//! whatever path that ensurer just resolved.
//!
//! **Shutdown policy** (`HummApp::on_exit`, [`Services`]'s own doc for the
//! mechanics): close cancels in-flight work and joins the runner; the
//! stale-flip remains the backstop for SIGKILL.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::NaiveDateTime;

use humm_core::boundary::{
    AudioEncoder, LanguageDetector, MediaFetcher, NoiseReducer, SpeechIsolator, TaskStore,
};
use humm_core::env::{threads_from_env, use_gpu_from_env};
use humm_core::error::StoreError;
use humm_core::openai::LlmEndpointSource;
use humm_core::paths;
use humm_core::pipeline::EngineTuning;
use humm_core::postproc::{PostprocConfig, PostprocConfigSource};
use humm_core::queue::{
    ArtifactEnsurer, EngineFactory, QueueHandle, RunnerDeps, TaskEvent, spawn_task_runner,
};
use humm_core::secrets::{MemorySecretStore, SecretKey, SecretStore};
use humm_core::settings::{Settings, SettingsError};
use humm_core::telemetry::{Event, NoopTelemetry, PostHogTelemetry, Telemetry, telemetry_enabled};
use humm_core::update::{self, UpdateInfo};
use humm_core::viewer::text_view::paragraph_split_from_env;
use humm_dfn::DfNoiseReducer;
use humm_media::{
    DemucsIsolator, FfmpegDecoder, FfprobeProber, MediaCapabilities, YtDlpFetcher, probe_binaries,
};
use humm_store::{KeyringSecretStore, SqliteStore};
use humm_whisper::WhisperLangDetector;

use crate::engine_factory::{DownloaderEnsurer, GuiEngineFactory};
use crate::i18n::locale_from_env_vars;
use crate::llm_source::GuiLlmSource;
use crate::speaker_id::{DiarizeService, RealDiarizeService, ScriptedDiarizeService};
use crate::translate_seam::{RealTranslateSpawner, ScriptedTranslateSpawner, TranslateSpawner};
use crate::views::prefs_general::{ValidateFn, real_validate_fn, stub_validate_fn};

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
    /// Whether `demucs` was found (F-027 speech isolation,
    /// [`humm_media::DemucsIsolator::available`]). Gates the options sheet's
    /// "Extract speech" checkbox the same way `yt_dlp` gates URL import — set
    /// in [`init`] from the same isolator instance wired into
    /// `RunnerDeps::isolator`, so the probe and the actual runner see
    /// identical availability. Not derived from [`MediaCapabilities`]
    /// (unlike the other three fields): demucs is a separate, much slower
    /// probe (its own doc: spawning it just to check presence would pull in
    /// PyTorch) so it's set explicitly in [`init`] rather than folded into
    /// the `From<&MediaCapabilities>` impl below.
    pub demucs: bool,
}

impl From<&MediaCapabilities> for Capabilities {
    fn from(media: &MediaCapabilities) -> Self {
        Capabilities {
            ffmpeg: media.ffmpeg_version.is_some(),
            ffprobe: media.ffprobe_version.is_some(),
            yt_dlp: media.yt_dlp.is_some(),
            // Not derivable from `MediaCapabilities` (see `demucs`'s own doc)
            // — `init` overwrites this with the real probe right after
            // converting; `false` here is only ever observed by a caller
            // that skips that step entirely (none currently does).
            demucs: false,
        }
    }
}

/// The outcome of one update check (startup or manual — both funnel through
/// [`run_update_check`]), sent to `HummApp` over [`Services::update_rx`].
///
/// A startup check that finds nothing new (`UpToDate`) or fails
/// (`CheckFailed`) is silently logged by `HummApp` (Buzz's own startup path
/// has no UI for either case); the About window's manual check — which
/// reuses this exact same code path (binding decision: "one code path") —
/// surfaces both instead ("You're up to date!" / "Update check failed — see
/// the log file"). `HummApp` tells the two apart via its own
/// `about_state.checking` flag (set right before spawning a manual check),
/// not anything carried on this enum itself.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateNotice {
    /// A newer version is available.
    Available(UpdateInfo),
    /// The current version is already the latest.
    UpToDate,
    /// The check itself failed (network/parse error) — the detail is
    /// [`update::UpdateError`]'s `Display` text.
    CheckFailed(String),
}

/// Everything `HummApp` needs to run. Constructed once at startup, by
/// [`init`] (real adapters) or [`init_with`] (fakes, for kittest).
///
/// ## Shutdown policy
///
/// Closing the app cancels every in-flight/pending task and joins the queue's
/// runner thread before the process exits: `HummApp::on_exit` takes `queue`
/// (hence [`Option`] rather than a bare [`QueueHandle`] — `eframe::App::
/// on_exit` only hands back `&mut self`, so the handle must be *taken*, not
/// consumed by value from a borrow) and runs `queue.cancel_all()` then
/// `queue.finish_within(SHUTDOWN_DRAIN_TIMEOUT)` (`app.rs`'s
/// `shutdown_queue`; ADR-022). Cancellation is prompt by design (a task's
/// cancel token is checked cooperatively at every phase boundary — decode,
/// download, engine inference — and every blocking `openai::*` HTTP call
/// cancels in under a second via `cancelable`'s request-thread detach), so
/// this is a *graceful, bounded* shutdown: the store gets each in-flight
/// task's terminal `canceled` write before the process exits, not a bare
/// `SIGKILL` mid-write. `finish_within`'s own timeout is the last-resort
/// backstop within that: if a wedged engine ever ignores cancellation, the
/// runner thread is deliberately leaked (logged) and the process exits
/// anyway rather than hanging forever — see `QueueHandle::finish_within`'s
/// doc comment for why that's sound. The stale-task flip (`apply_stale_flip`,
/// next launch) remains the sole backstop for the case this policy can't
/// cover at all — a `SIGKILL`/crash that never reaches `on_exit`.
pub struct Services {
    /// Loaded settings, shared so views can read/mutate and persist changes
    /// (`Settings::save` at `settings_path`).
    pub settings: Arc<Mutex<Settings>>,
    /// Where `settings` was loaded from and is saved back to.
    pub settings_path: PathBuf,
    /// The persistent task store.
    pub store: Arc<SqliteStore>,
    /// Handle to the running task queue (enqueue/cancel/retry/subscribe).
    /// `Option` so `HummApp::on_exit` can `take()` it for the shutdown
    /// sequence (see the struct doc's "Shutdown policy" section) — `None`
    /// only after that has already happened, which only occurs once, right
    /// before the process exits. [`Self::queue`] is the normal-operation
    /// accessor; nothing on the ordinary running-app path should ever see
    /// `None` here.
    pub queue: Option<QueueHandle>,
    /// Task lifecycle events, drained by the event-pump thread (`events.rs`).
    pub events: flume::Receiver<TaskEvent>,
    /// ffmpeg/ffprobe/yt-dlp availability.
    pub capabilities: Capabilities,
    /// Root directory for downloaded whisper models.
    pub model_root: PathBuf,
    /// Update-check notices: [`init`]/[`init_with`] spawn the startup check
    /// (when due) against this channel's sender; `HummApp` drains
    /// `update_rx` every frame.
    pub update_rx: flume::Receiver<UpdateNotice>,
    /// The other end of `update_rx` — kept so `HummApp` can also spawn the
    /// About window's manual check (which reuses [`run_update_check`]) onto
    /// the same channel the startup check reports over.
    pub update_tx: flume::Sender<UpdateNotice>,
    /// The OpenAI-API-key secret store (F-041/F-031, ADR-012): the OS keyring
    /// in production ([`init`]), an in-memory double in tests ([`init_with`]).
    /// The M6 GUI surfaces reach it through `HummApp`'s owned `Services` — the
    /// Preferences → General key field (Task 13) and the viewer/recording
    /// Translate gates (Tasks 15/17) read/write it here, the same way
    /// `HummApp` already reaches [`Self::store`]/[`Self::settings`].
    pub secrets: Arc<dyn SecretStore>,
    /// The LLM-translation worker seam (F-031/F-063/F-085, design spec
    /// §7.3/§7.4): [`crate::translate_seam::RealTranslateSpawner`] in
    /// production ([`init`]), a scripted double in tests ([`init_with`]). The
    /// viewer's Translate button (Task 15) and the recording window's
    /// translation panel (Task 17) spawn workers through this, reached — like
    /// [`Self::secrets`] — via `HummApp`'s owned `Services`.
    pub translate: Arc<dyn TranslateSpawner>,
    /// The Preferences General tab's Test-button seam (partial F-100, design
    /// spec section 7.1, Task 13): `real_validate_fn()` in production
    /// ([`init`]), a scripted/stub [`ValidateFn`] in tests ([`init_with`]) so
    /// a kittest click on Test never issues a real HTTP request.
    pub validate: ValidateFn,
    /// The Speaker-ID window's diarization seam (F-087, `speaker_id`'s module
    /// doc): a [`RealDiarizeService`] wrapping `ModelDownloader` + a
    /// freshly-built `OrtDiarizer` per call in production ([`init`]), a
    /// [`ScriptedDiarizeService`] double in tests ([`init_with`]) so a
    /// kittest never touches the network or `ort`. Threaded into
    /// `HummApp`'s `speaker_deps` (`app.rs`) alongside `viewer_deps`'s own
    /// decoder/audio-output `Arc`s.
    pub diarize: Arc<dyn DiarizeService>,
}

impl Services {
    /// The running queue handle, for every ordinary (non-shutdown) caller.
    ///
    /// Panics if called after [`Self::queue`]'s handle has already been
    /// taken for shutdown (`HummApp::on_exit`) — that only happens once, in
    /// the window between the app closing and the process actually exiting,
    /// during which nothing should still be calling this.
    pub fn queue(&self) -> &QueueHandle {
        self.queue
            .as_ref()
            .expect("Services::queue called after the queue was taken for shutdown")
    }
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

/// The GUI's [`PostprocConfigSource`] (design spec §2.4): reads
/// `settings.postproc` fresh from the shared live [`Settings`] on every
/// [`PostprocConfigSource::snapshot`] call, so a Preferences → Post-
/// processing edit made mid-run is honored by the very next task — mirrors
/// [`crate::llm_source::GuiLlmSource`]'s "read fresh, per task" contract.
/// The AI-summary hook's API key is only resolved from `secrets` when
/// `ai_summary.enabled` is set, so an ordinary run with every hook off never
/// touches the keyring at all.
struct GuiPostprocSource {
    /// The shared live settings — `postproc` is cloned fresh on every
    /// snapshot.
    settings: Arc<Mutex<Settings>>,
    /// The secret store the AI-summary API key is read from (the OS keyring
    /// in production, an in-memory double in tests).
    secrets: Arc<dyn SecretStore>,
}

impl PostprocConfigSource for GuiPostprocSource {
    fn snapshot(&self) -> PostprocConfig {
        let postproc = {
            let guard = self
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.postproc.clone()
        };
        let summary_api_key = if postproc.ai_summary.enabled {
            match self.secrets.get(SecretKey::AiSummaryApiKey) {
                Ok(Some(key)) if !key.is_empty() => Some(key),
                Ok(_) => None,
                Err(err) => {
                    log::warn!("keyring lookup for AI Summary API key failed: {err}");
                    None
                }
            }
        } else {
            None
        };
        PostprocConfig {
            skip: postproc.skip_already_transcribed,
            summary: postproc.ai_summary,
            summary_api_key,
            docx: postproc.export_docx,
            resize: postproc.transcript_resizer,
            paragraph_split: paragraph_split_from_env(),
            noise_reduction: postproc.noise_reduction,
            language_detection: postproc.language_detection,
        }
    }
}

/// Builds [`Services`] with real adapters, mirroring the CLI's startup order
/// (`crates/humm-cli/src/run.rs::execute`): settings → store → one-time Buzz
/// import → instance-lock-gated stale-task flip → queue spawn → (if due)
/// background update check (F-006/ADR-015). The user-facing telemetry launch
/// event (F-007/ADR-009) is deliberately *not* fired here — [`HummApp`]
/// fires it once from its first `eframe::App::ui` frame instead (see
/// [`fire_launch_telemetry`]'s doc comment for why).
///
/// `sole_instance` is threaded in from `main.rs`'s
/// `humm_core::instance::acquire_instance_lock` call — the lock guard itself
/// is held by `main` for the process lifetime; this function only needs the
/// *decision* that call already made (ADR-017).
///
/// [`HummApp`]: crate::app::HummApp
pub fn init(sole_instance: bool) -> Result<Services, InitError> {
    let settings_path = paths::settings_path();
    // Wrapped in `Arc<Mutex<_>>` immediately (rather than at the end, as
    // before Task 11): the background update-check thread spawned below
    // needs to share this exact settings handle with the rest of `Services`,
    // not a separate copy that would silently diverge.
    let settings = Arc::new(Mutex::new(Settings::load(&settings_path)?));

    let store = Arc::new(SqliteStore::open(&paths::db_path())?);
    import_buzz_history(&store);
    apply_stale_flip(&store, sole_instance);

    let media_caps = probe_binaries();
    let mut capabilities = Capabilities::from(&media_caps);

    // The one isolator instance both probes availability (`capabilities.demucs`,
    // gating the options sheet's checkbox) and is wired into `RunnerDeps`
    // below — a single `available()` cache so the capability the checkbox
    // renders against and the isolator the queue actually runs are never out
    // of sync (design spec §3/§8).
    let isolator = Arc::new(DemucsIsolator::new());
    capabilities.demucs = isolator.available();

    let decoder = Arc::new(FfmpegDecoder::new());
    // The reducer wired into `RunnerDeps` below (F-112) reuses this same
    // `FfmpegDecoder` instance for its own `decode_mono_at` call — cloned
    // (cheap, `Arc`) before `decoder` moves into the `RunnerDeps` literal
    // (where it coerces to `Arc<dyn AudioDecoder>`), same "share, don't
    // duplicate the adapter" pattern as `isolator` above.
    let reducer = Arc::new(DfNoiseReducer::new(Arc::clone(&decoder)));
    let prober = Arc::new(FfprobeProber::new());
    let fetcher = {
        let guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        build_fetcher(&guard, &media_caps)
    };
    let model_root = paths::model_root();

    let use_gpu = use_gpu_from_env(std::env::var("HUMM_FORCE_CPU").ok().as_deref());
    let ensurer = DownloaderEnsurer {
        root: model_root.clone(),
        // Shared, not snapshotted: `DownloaderEnsurer` reads
        // `models.custom_model_url` fresh from this same handle on every
        // `ensure_model` call, so a URL set in the Models tab after this
        // point is still honored (final-review fix 1).
        settings: Arc::clone(&settings),
        // Mirrors the CLI's `coreml_encoder()` exactly (`humm-cli/src/run.rs`).
        coreml: cfg!(all(target_os = "macos", target_arch = "aarch64")),
    };

    // The OS-keyring secret store, built once and shared by every consumer of
    // the OpenAI API key: the engine factory's `Backend::OpenAiApi` arm, the
    // per-task LLM endpoint source, and (Task 13) the Preferences key field —
    // all through the one `Arc` (ADR-012, design spec §5).
    let secrets: Arc<dyn SecretStore> = Arc::new(KeyringSecretStore::new());

    let deps = RunnerDeps {
        store: Arc::clone(&store) as Arc<dyn TaskStore>,
        decoder,
        prober,
        fetcher,
        engines: Box::new(GuiEngineFactory {
            use_gpu,
            settings: Arc::clone(&settings),
            secrets: Arc::clone(&secrets),
            // The ffmpeg mp3 encoder every `Backend::OpenAiApi` engine uploads
            // through (design spec §3.1) — a stateless handle, so a dedicated
            // instance here is as cheap as sharing the decoder's.
            encoder: Arc::new(FfmpegDecoder::new()) as Arc<dyn AudioEncoder>,
        }),
        tuning: EngineTuning {
            // No fixed VAD path up front: `DownloaderEnsurer::ensure_vad`
            // resolves it per job, since a GUI run has no synchronous
            // pre-enqueue resolution step to do it once (design spec §2.1).
            vad_model: None,
            use_gpu,
            threads: threads_from_env(std::env::var("HUMM_WHISPER_N_THREADS").ok().as_deref()),
        },
        media_dir: paths::media_dir(),
        model_root: model_root.clone(),
        // The GUI never resolves a model before enqueueing — this is what
        // makes that safe: the runner ensures the model/VAD as the task's
        // own first phase (module doc, `queue.rs`).
        artifacts: Some(Arc::new(ensurer) as Arc<dyn ArtifactEnsurer>),
        // The GUI's env→settings→keyring `LlmEndpointSource` (F-031, design
        // spec §6): resolves the per-task LLM-translation endpoint at task
        // start, so a file task whose options carry `llm` is translated when a
        // key is available and silently skips translation otherwise.
        llm: Some(Arc::new(GuiLlmSource {
            settings: Arc::clone(&settings),
            secrets: Arc::clone(&secrets),
        }) as Arc<dyn LlmEndpointSource>),
        // The GUI's check_skip seam config source (F-115, design spec
        // §2.4): reads `settings.postproc` fresh per task, resolving the
        // AI-summary key from the same shared keyring `secrets` handle.
        postproc: Some(Arc::new(GuiPostprocSource {
            settings: Arc::clone(&settings),
            secrets: Arc::clone(&secrets),
        }) as Arc<dyn PostprocConfigSource>),
        // M8 pre-transcription seam (F-027): the same isolator instance
        // `capabilities.demucs` was just probed from, above — a task only
        // ever runs it when its own `options.extract_speech` is set
        // (`queue::Runner::run_job`'s gate), so this is wired unconditionally
        // rather than behind `if capabilities.demucs` (an absent binary still
        // degrades per-task, via `SpeechIsolator::isolate`'s own error path,
        // not by leaving the seam `None`).
        isolator: Some(isolator as Arc<dyn SpeechIsolator>),
        // F-112: wired unconditionally too, same reasoning as `isolator`
        // above — but for a different reason underneath. `deep_filter`'s
        // `default-model` feature embeds the DeepFilterNet3 weights at
        // compile time (no download, no "binary missing" absence path), so
        // there is no availability probe to gate on in the first place;
        // enabling/disabling the hook is entirely `PostprocConfig::
        // noise_reduction.enabled`'s job (`queue::Runner::run_job`'s own
        // gate), not this wiring site's.
        reducer: Some(reducer as Arc<dyn NoiseReducer>),
        // F-114: wired unconditionally too — same reasoning again. Whether
        // detection actually runs for a given task is entirely
        // `PostprocConfig::language_detection.enabled` + the en-quirk gate in
        // `queue::Runner::run_job`/`postproc::langdetect::should_detect`, not
        // this wiring site's job. `WhisperLangDetector` needs no availability
        // probe either (whisper-rs is always linked in; the model it runs
        // against is resolved per-job by `postproc::langdetect::select_model`
        // / the tiny-download fallback, both already inside `run_job`).
        // `settings` is shared, not snapshotted: `WhisperLangDetector` reads
        // `models.force_cpu` fresh on every `detect()` call (review fix —
        // mirrors `DownloaderEnsurer`'s own live-settings-read contract just
        // above, and `GuiEngineFactory::create_whisper`'s), so a Preferences
        // "Disable GPU" toggle is honored by the very next detection with no
        // restart, same as it already is for the main transcription engine.
        lang_detector: Some(
            Arc::new(WhisperLangDetector::new(use_gpu, Arc::clone(&settings)))
                as Arc<dyn LanguageDetector>,
        ),
    };
    let queue = spawn_task_runner(deps);
    let events = queue.subscribe();

    let (update_tx, update_rx) = flume::unbounded();
    maybe_spawn_startup_update_check(&settings, &settings_path, update_tx.clone());

    // Built from a clone *before* `model_root` moves into the `Services`
    // literal's own `model_root` field below.
    let diarize = Arc::new(RealDiarizeService::new(model_root.clone())) as Arc<dyn DiarizeService>;

    Ok(Services {
        settings,
        settings_path,
        store,
        queue: Some(queue),
        events,
        capabilities,
        model_root,
        update_rx,
        update_tx,
        secrets,
        translate: Arc::new(RealTranslateSpawner) as Arc<dyn TranslateSpawner>,
        validate: real_validate_fn(),
        diarize,
    })
}

/// Spawns the background update check (F-006, `update_checker.py:69-91`
/// parity) iff [`update_check_due`] says one is due right now — reading
/// `HUMM_DISABLE_UPDATE_CHECK` and `settings.updates.{check_enabled,
/// last_check}` exactly once. The env var is read *here*, not inside
/// [`run_update_check`] itself: Buzz constructs its checker and connects its
/// signal unconditionally, gating only the network call
/// (`main_window.py:569`) — mirrored here by always building `Services`'
/// update channel (`init`, above) and only conditionally spawning the thread
/// that would actually touch the network.
fn maybe_spawn_startup_update_check(
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    tx: flume::Sender<UpdateNotice>,
) {
    let env = std::env::var("HUMM_DISABLE_UPDATE_CHECK").ok();
    let (check_enabled, last_check) = {
        let guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            guard.updates.check_enabled,
            guard.updates.last_check.clone(),
        )
    };
    let now = chrono::Local::now().naive_local();
    if !update_check_due(env.as_deref(), check_enabled, last_check.as_deref(), now) {
        return;
    }

    let settings = Arc::clone(settings);
    let settings_path = settings_path.to_path_buf();
    std::thread::spawn(move || {
        run_update_check(
            &settings,
            &settings_path,
            &tx,
            update::VERSION_INFO_URL,
            env!("CARGO_PKG_VERSION"),
        );
    });
}

/// Whether an update check should run right now (F-006, ADR-015): the
/// `HUMM_DISABLE_UPDATE_CHECK` env var (any non-empty value disables — Buzz's
/// `BUZZ_DISABLE_UPDATE_CHECK` parity), the user's Preferences → Privacy
/// "Check for updates automatically" toggle, and the ≤7-day cadence
/// ([`update::should_check`]), all AND'd together. Pure and independently
/// testable — the actual network call/thread spawn is
/// [`maybe_spawn_startup_update_check`]'s job, not this function's.
///
/// This is the *startup* gate only. The About window's manual "Check for
/// updates" bypasses every input here — including the env var, since a
/// manual click is an explicit user action, not the automatic background
/// check Buzz's/humm's disable knobs exist to silence (see
/// [`run_update_check`]'s doc comment for the shared code path both funnel
/// through).
pub fn update_check_due(
    env: Option<&str>,
    check_enabled: bool,
    last_check: Option<&str>,
    now: NaiveDateTime,
) -> bool {
    let env_disables = env.is_some_and(|value| !value.is_empty());
    !env_disables && check_enabled && update::should_check(last_check, now, false)
}

/// Runs one update check against `url`/`current_version` and reports the
/// outcome both ways: persisted into `settings` (`updates.last_check`
/// unconditionally — written *before* anything else, `%Y-%m-%dT%H:%M:%S%.f`,
/// even on a failed fetch, matching Buzz's `_on_reply_finished`
/// (`update_checker.py:82-85`); `updates.available_version` only on success:
/// the newer version string, or `""` on "already up to date" — again Buzz
/// parity, `update_checker.py:120-125`) and sent as an [`UpdateNotice`] over
/// `tx`.
///
/// The single code path both the startup check
/// ([`maybe_spawn_startup_update_check`]) and the About window's manual
/// check share (binding decision: humm unifies Buzz's two separate update
/// endpoints/logics into this one). Callers that don't want to block their
/// own thread (both of the above) spawn this into its own
/// `std::thread::spawn`; this function itself performs the (blocking)
/// network call directly on whatever thread calls it.
pub(crate) fn run_update_check(
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    tx: &flume::Sender<UpdateNotice>,
    url: &str,
    current_version: &str,
) {
    let outcome = update::check_for_update(url, current_version);
    let now_iso = chrono::Local::now()
        .naive_local()
        .format("%Y-%m-%dT%H:%M:%S%.f")
        .to_string();

    let notice = match &outcome {
        Ok(Some(info)) => UpdateNotice::Available(info.clone()),
        Ok(None) => UpdateNotice::UpToDate,
        Err(err) => UpdateNotice::CheckFailed(err.to_string()),
    };

    {
        let mut guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.updates.last_check = Some(now_iso);
        match &outcome {
            Ok(Some(info)) => guard.updates.available_version = Some(info.version.clone()),
            Ok(None) => guard.updates.available_version = Some(String::new()),
            // Buzz leaves `available-version` untouched on a failed fetch
            // (`update_checker.py:87-91`: log + return, no further writes).
            Err(_) => {}
        }
        if let Err(err) = guard.save(settings_path) {
            log::warn!("could not persist the update-check result: {err}");
        }
    }

    if tx.send(notice).is_err() {
        log::debug!("update-check receiver already dropped; discarding the result");
    }
}

/// Whether the toolbar chip should show *at startup*, before any fresh check
/// this session has completed — Buzz-equivalent persistence across launches
/// (binding decision): `settings.updates.available_version` from a *previous*
/// session's successful check is still newer than `current_version`.
///
/// `stored_available_version` is `None` (never checked) or `Some("")`
/// (last check found nothing newer) both resolve to `false`, same as any
/// stored value [`update::is_newer_version`] can't parse as numeric (a
/// corrupted/hand-edited settings file) — "no evidence of an update" is the
/// only safe default. A caller that finds this `true` has only a version
/// string to show, not release notes/download URLs (those aren't persisted);
/// a fresh check backfills them once it completes.
pub fn chip_visible_on_startup(
    current_version: &str,
    stored_available_version: Option<&str>,
) -> bool {
    match stored_available_version {
        Some(version) if !version.is_empty() => update::is_newer_version(current_version, version),
        _ => false,
    }
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
    /// to simulate ffmpeg/yt-dlp being present). Intentional test/production
    /// asymmetry for `demucs`: a test can set `capabilities.demucs = true` to
    /// exercise the options-sheet checkbox's enabled-state UI gating (Task 6
    /// kittests do exactly this) while [`init_with`]'s own `RunnerDeps`
    /// always wires `isolator: None` regardless — no kittest here drives a
    /// real `SpeechIsolator` run end-to-end (that's `humm_core::queue`'s
    /// scripted-`ScriptedIsolator` test double's job); the two are unrelated
    /// knobs the real [`init`] happens to keep in sync (same isolator
    /// instance) but a test fixture is free not to.
    pub capabilities: Capabilities,
    /// Artifact ensurer, if the test needs one (`None` — the common case —
    /// means every enqueued task already carries a runnable engine/model, as
    /// every kittest fixture before this task assumed; a phase test scripts
    /// a fake here to drive `ModelDownloadProgress` events through the real
    /// queue).
    pub artifacts: Option<Arc<dyn ArtifactEnsurer>>,
    /// The OpenAI-API-key secret store — always an in-memory double in tests
    /// (`Arc::new(MemorySecretStore::new())` from [`default_test_secrets`]) so
    /// no test ever touches the real OS keychain (design spec §5). A test that
    /// needs a pre-seeded key clones the concrete `MemorySecretStore` before
    /// boxing it here and seeds it directly.
    pub secrets: Arc<dyn SecretStore>,
    /// The LLM-translation seam — a [`ScriptedTranslateSpawner`] test double by
    /// default ([`default_test_translate`]); a test that asserts on the
    /// translation flow injects its own scripted spawner (holding onto its
    /// [`ScriptedTranslateSpawner::state`]) here.
    pub translate: Arc<dyn TranslateSpawner>,
    /// The per-task LLM endpoint source (F-031). `None` by default — no
    /// existing kittest enables per-segment file-path translation, so leaving
    /// it unset keeps the file path's "Enable AI translation" option inert
    /// exactly as before; a test exercising F-031 injects a
    /// [`GuiLlmSource`]/fixed source pointed at a mock endpoint.
    pub llm: Option<Arc<dyn LlmEndpointSource>>,
    /// The Preferences → General tab's Test-button seam — [`stub_validate_fn`]
    /// by default ([`default_test_validate`]), which never runs a real check;
    /// a test exercising the Test button injects its own scripted
    /// `Arc::new(|_auth| ValidateOutcome::...)` here.
    pub validate: ValidateFn,
}

/// The default test secret store: a fresh, empty in-memory
/// [`MemorySecretStore`] — never the real OS keyring (design spec §5). Exposed
/// (not `#[cfg(test)]`) so the `tests/` kittest harnesses, which link the
/// library's normal build, can fill [`TestDeps::secrets`] without importing
/// the core secrets module themselves.
pub fn default_test_secrets() -> Arc<dyn SecretStore> {
    Arc::new(MemorySecretStore::new())
}

/// The default test translate seam: a [`ScriptedTranslateSpawner`] with an
/// identity mapper — neutral (its output equals its input) for harnesses that
/// spawn a translator but don't assert on the text. A test that wants a
/// visible transform or to inspect recorded calls constructs its own
/// [`ScriptedTranslateSpawner`] and injects it instead.
pub fn default_test_translate() -> Arc<dyn TranslateSpawner> {
    Arc::new(ScriptedTranslateSpawner::identity())
}

/// The default test [`ValidateFn`]: [`stub_validate_fn`] — never touches the
/// network, always reports `Invalid` with a message that would fail any
/// assertion actually depending on the real check. A test exercising the
/// Preferences General tab's Test button injects its own scripted fn instead.
pub fn default_test_validate() -> ValidateFn {
    stub_validate_fn()
}

/// The default test diarization seam: a [`ScriptedDiarizeService`] that
/// succeeds instantly with zero speaker turns — neutral for harnesses that
/// never open the Speaker-ID window through a real `HummApp`.
///
/// Not a [`TestDeps`] field (unlike `translate`/`validate`): every kittest
/// task's `SpeakerIdVm`/`views::speaker_id::ui` coverage builds its own
/// [`crate::speaker_id::SpeakerIdDeps`] directly (the exact same
/// "bypass `HummApp`'s wiring, construct the window's deps locally" shape
/// `tests/kittest_viewer.rs`'s own `fake_deps` already uses for `ViewerDeps`)
/// — so, unlike `translate`, no existing or brief-listed kittest needs a
/// *scripted* diarizer reachable through `Services`/`HummApp::new_for_test`
/// itself. Threading one more required field through [`TestDeps`] would
/// touch every one of this crate's ~20 existing `TestDeps { .. }` literals
/// for no test that actually needs it; adding it later, when a test genuinely
/// does, is one field plus that test's own call sites — not this one's.
pub fn default_test_diarize() -> Arc<dyn DiarizeService> {
    Arc::new(ScriptedDiarizeService::ready(Vec::new()))
}

/// Builds [`Services`] from caller-supplied fakes/temp state — no real
/// settings file, database, or subprocess is ever touched. Used by kittest
/// harnesses (this task's smoke test, and every later kittest task that
/// constructs `HummApp` via [`crate::app::HummApp::new_for_test`]).
///
/// Deliberately never spawns the real startup update-check thread real
/// [`init`] does (Global Constraint 3: no network in tests) — a test that
/// wants to exercise the update-chip/dialog path drives it hermetically
/// instead, by sending straight into [`Services::update_tx`] (exposed for
/// exactly this).
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
        model_root: deps.model_root.clone(),
        artifacts: deps.artifacts,
        llm: deps.llm,
        // No existing kittest exercises the check_skip seam or any other
        // post-processing hook — `None` keeps every task running through
        // the engine exactly as before this field existed (see
        // `queue::RunnerDeps::postproc`'s doc comment).
        postproc: None,
        // M8 pre-transcription seam: no existing kittest exercises it yet
        // (Tasks 5-8 add the adapters + their own scripted-double kittests).
        // `None` keeps every task running exactly as before these fields
        // existed.
        isolator: None,
        reducer: None,
        lang_detector: None,
    };
    let queue = spawn_task_runner(runner_deps);
    let events = queue.subscribe();
    let (update_tx, update_rx) = flume::unbounded();

    Services {
        settings: Arc::new(Mutex::new(deps.settings)),
        settings_path: deps.settings_path,
        store: deps.store,
        queue: Some(queue),
        events,
        capabilities: deps.capabilities,
        model_root: deps.model_root,
        update_rx,
        update_tx,
        secrets: deps.secrets,
        translate: deps.translate,
        validate: deps.validate,
        diarize: default_test_diarize(),
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

/// Fires humm's one `app_launched` telemetry event (F-007, ADR-009),
/// exactly once per launch. Called from `crate::app::HummApp`'s
/// `eframe::App::ui` on its first frame rather than from [`init`]:
/// `services::init` runs before any
/// window exists, and firing telemetry is explicitly *not* on that critical
/// path (design spec §3.1 wants < 300 ms cold-launch-to-interactive) — a
/// first-frame call keeps it off startup's own latency budget even though
/// [`Telemetry::capture`] is itself already fire-and-forget (spawns its own
/// thread), because this function also does the (cheap, but still
/// synchronous-on-the-caller's-thread) settings lock/save for
/// `user_identifier`.
///
/// `settings.privacy.user_identifier` is ensured (minted + persisted via a
/// fresh uuid4, Buzz's `Settings.get_user_identifier()` parity) *unconditionally*
/// — even when telemetry itself ends up disabled below — matching Buzz's own
/// identifier lifecycle (it's a general persisted identifier, not something
/// that only exists while telemetry is on).
pub fn fire_launch_telemetry(services: &Services) {
    let user_identifier = ensure_user_identifier(&services.settings, &services.settings_path);

    let settings_enabled = {
        let guard = services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.privacy.telemetry_enabled
    };
    let build_key = option_env!("HUMM_POSTHOG_KEY");
    let enabled = telemetry_enabled(
        build_key,
        cfg!(debug_assertions),
        std::env::var("HUMM_DISABLE_TELEMETRY").ok().as_deref(),
        settings_enabled,
    );

    let telemetry: Box<dyn Telemetry> = if enabled {
        Box::new(PostHogTelemetry::new(
            build_key.unwrap_or_default().to_string(),
            user_identifier,
        ))
    } else {
        Box::new(NoopTelemetry)
    };

    let locale = locale_from_env_vars(
        std::env::var("LC_ALL").ok().as_deref(),
        std::env::var("LANG").ok().as_deref(),
    );
    let event = Event::app_launched(
        env!("CARGO_PKG_VERSION"),
        locale.as_deref(),
        humm_core::telemetry::kernel_release().as_deref(),
        humm_core::telemetry::macos_version().as_deref(),
    );
    telemetry.capture(event);
}

/// Returns `settings.privacy.user_identifier`, minting and persisting a
/// fresh uuid4 if it was `None` (first run). The save failure path is
/// logged, not fatal (same non-fatal-settings-write contract as every other
/// persist in this crate) — the freshly-minted id is still returned and used
/// for this launch's event either way, it just won't survive to the next
/// one if the write failed.
fn ensure_user_identifier(settings: &Arc<Mutex<Settings>>, settings_path: &Path) -> String {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(id) = &guard.privacy.user_identifier {
        return id.clone();
    }
    let id = uuid::Uuid::new_v4().to_string();
    guard.privacy.user_identifier = Some(id.clone());
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the telemetry user identifier: {err}");
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::boundary::TranscriptionEngine;
    use humm_core::domain::{ModelSpec, TranscriptionId};
    use humm_core::error::CoreError;
    use pretty_assertions::assert_eq;
    use std::time::{Duration, Instant};

    fn test_deps(store: Arc<SqliteStore>, dir: &Path) -> TestDeps {
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
            // Still a real tempdir path (not a sentinel string) so nothing
            // silently degrades if that ever changes.
            media_dir: dir.join("media"),
            model_root: dir.join("models"),
            settings: Settings::default(),
            settings_path: dir.join("settings.toml"),
            engines: Box::new(FailingFactory),
            fetcher: None,
            capabilities: Capabilities::default(),
            artifacts: None,
            secrets: default_test_secrets(),
            translate: default_test_translate(),
            llm: None,
            validate: default_test_validate(),
        }
    }

    #[test]
    fn init_with_builds_a_working_queue_and_subscription() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
        let dir = tempfile::tempdir().expect("tempdir");
        let mut services = init_with(test_deps(Arc::clone(&store), dir.path()));

        let id = TranscriptionId::new();
        let spec = humm_core::domain::TaskSpec {
            input: humm_core::domain::Input::Path(PathBuf::from("/does/not/matter.wav")),
            options: humm_core::domain::TranscriptionOptions::default(),
            output: humm_core::domain::OutputSpec::default(),
        };
        services
            .queue()
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
        let _ = services.queue.take().expect("queue present").finish();
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
                demucs: false,
            }
        );
    }

    // -- update_check_due ----------------------------------------------------

    fn fixed_now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 21)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn days_ago(days: i64) -> String {
        (fixed_now() - chrono::Duration::days(days))
            .format("%Y-%m-%dT%H:%M:%S%.f")
            .to_string()
    }

    #[test]
    fn update_check_due_env_set_disables_even_when_otherwise_due() {
        assert!(!update_check_due(Some("1"), true, None, fixed_now()));
        assert!(!update_check_due(
            Some("anything"),
            true,
            Some(days_ago(30).as_str()),
            fixed_now()
        ));
    }

    #[test]
    fn update_check_due_env_unset_or_empty_does_not_disable() {
        assert!(update_check_due(None, true, None, fixed_now()));
        assert!(update_check_due(Some(""), true, None, fixed_now()));
    }

    #[test]
    fn update_check_due_toggle_off_disables_even_when_otherwise_due() {
        assert!(!update_check_due(None, false, None, fixed_now()));
    }

    #[test]
    fn update_check_due_true_when_never_checked() {
        assert!(update_check_due(None, true, None, fixed_now()));
    }

    #[test]
    fn update_check_due_false_within_the_seven_day_window() {
        let last_check = days_ago(6);
        assert!(!update_check_due(
            None,
            true,
            Some(last_check.as_str()),
            fixed_now()
        ));
    }

    #[test]
    fn update_check_due_true_past_the_seven_day_window() {
        let last_check = days_ago(8);
        assert!(update_check_due(
            None,
            true,
            Some(last_check.as_str()),
            fixed_now()
        ));
    }

    // -- chip_visible_on_startup ----------------------------------------------

    #[test]
    fn chip_visible_when_stored_version_is_newer() {
        assert!(chip_visible_on_startup("1.4.5", Some("1.5.0")));
    }

    #[test]
    fn chip_hidden_when_stored_version_is_older_or_equal() {
        assert!(!chip_visible_on_startup("1.5.0", Some("1.4.5")));
        assert!(!chip_visible_on_startup("1.5.0", Some("1.5.0")));
    }

    #[test]
    fn chip_hidden_when_stored_version_is_empty() {
        assert!(!chip_visible_on_startup("1.5.0", Some("")));
    }

    #[test]
    fn chip_hidden_when_never_checked() {
        assert!(!chip_visible_on_startup("1.5.0", None));
    }

    #[test]
    fn chip_hidden_when_stored_version_is_garbage() {
        assert!(!chip_visible_on_startup("1.5.0", Some("not-a-version")));
    }

    // -- locale_from_env_vars --------------------------------------------------

    #[test]
    fn locale_strips_the_codeset_suffix() {
        assert_eq!(
            locale_from_env_vars(Some("en_US.UTF-8"), None),
            Some("en_US".to_string())
        );
    }

    #[test]
    fn locale_lc_all_wins_over_lang() {
        assert_eq!(
            locale_from_env_vars(Some("fr_FR.UTF-8"), Some("en_US.UTF-8")),
            Some("fr_FR".to_string())
        );
    }

    #[test]
    fn locale_falls_back_to_lang_when_lc_all_unset() {
        assert_eq!(
            locale_from_env_vars(None, Some("de_DE.UTF-8")),
            Some("de_DE".to_string())
        );
    }

    #[test]
    fn locale_none_when_both_unset() {
        assert_eq!(locale_from_env_vars(None, None), None);
    }

    #[test]
    fn locale_none_when_both_blank() {
        assert_eq!(locale_from_env_vars(Some(""), Some("   ")), None);
    }

    #[test]
    fn locale_without_a_codeset_suffix_passes_through() {
        assert_eq!(locale_from_env_vars(Some("C"), None), Some("C".to_string()));
    }

    // -- run_update_check -------------------------------------------------------

    #[test]
    fn run_update_check_persists_last_check_and_sends_check_failed_on_a_bad_url() {
        // No network call ever succeeds against this URL (invalid scheme,
        // never resolves) — this proves the "persist last_check even on
        // error" contract without depending on network availability in CI.
        let settings = Arc::new(Mutex::new(Settings::default()));
        let dir = tempfile::tempdir().expect("tempdir");
        let settings_path = dir.path().join("settings.toml");
        let (tx, rx) = flume::unbounded();

        run_update_check(&settings, &settings_path, &tx, "not-a-valid-url", "1.0.0");

        let notice = rx.try_recv().expect("a notice was sent");
        assert!(matches!(notice, UpdateNotice::CheckFailed(_)));

        let guard = settings.lock().unwrap();
        assert!(
            guard.updates.last_check.is_some(),
            "last_check must be persisted even when the fetch fails"
        );
        assert_eq!(
            guard.updates.available_version, None,
            "available_version must be left untouched on a failed fetch"
        );
    }
}
