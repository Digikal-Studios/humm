//! Real GUI services (M3 Task 10, F-022/F-025/F-043): the downloader-backed
//! [`ArtifactEnsurer`] ([`DownloaderEnsurer`]) and the whisper-rs
//! [`EngineFactory`] ([`GuiEngineFactory`]) that `services::init` wires into
//! [`humm_core::queue::RunnerDeps`].
//!
//! Unlike the CLI (`humm-cli/src/run.rs`'s `CliEngineFactory` +
//! `resolve_model`/`ensure_vad`), which resolves the model/VAD synchronously
//! *before* enqueueing, the GUI never blocks an import on a download
//! (design spec §2.1, `docs/design/UX_SPEC.md` §J-1): [`DownloaderEnsurer`]
//! runs *inside* the task, as the queue's own `RunnerDeps::artifacts` hook,
//! so an absent model/VAD file downloads as that task's own first phase
//! instead of blocking anything else in the queue. Its routing logic is
//! otherwise a deliberate, near-verbatim mirror of the CLI's
//! `resolve_model`/`ensure_vad` helpers — same arms, same actionable
//! messages, same "VAD failure never fails the task" contract.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use humm_core::boundary::TranscriptionEngine;
use humm_core::cancel::CancelToken;
use humm_core::domain::{ModelSize, ModelSpec};
use humm_core::env::vad_disabled_from_env;
use humm_core::error::{CoreError, EngineError, ModelError};
use humm_core::models::download::{DownloadProgress, ModelDownloader, ResolvedModel};
use humm_core::models::registry::local_vad_model_path;
use humm_core::paths;
use humm_core::queue::{ArtifactEnsurer, EngineFactory};
use humm_core::settings::Settings;

/// Seam over [`ModelDownloader`]'s network-touching methods, so
/// [`DownloaderEnsurer`]'s routing logic (which arm to call, the exact
/// arguments each gets, and the actionable-error path that never calls
/// anything) is unit-testable without ever reaching the network: tests
/// substitute a fake implementation; [`DownloaderEnsurer`] itself always
/// goes through [`RealBackend`], a thin wrapper over a real
/// [`ModelDownloader`].
///
/// Covers four of `ModelDownloader`'s five public download methods —
/// `ensure_model` (which *always* fetches the CoreML encoder unconditionally)
/// has no caller here: the GUI's routing mirrors the CLI's own
/// `resolve_model` helper (`humm-cli/src/run.rs`), which uses
/// `resolve_model`/`resolve_hf_repo` (both tolerate a CoreML transport blip
/// once the ggml itself is usable) or `ensure_custom_model`.
trait ModelBackend {
    fn resolve_model(
        &self,
        size: ModelSize,
        coreml: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<ResolvedModel, ModelError>;

    fn ensure_custom_model(
        &self,
        url: &str,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError>;

    fn resolve_hf_repo(
        &self,
        repo: &str,
        coreml: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<ResolvedModel, ModelError>;

    fn ensure_vad_model(
        &self,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError>;
}

/// The real [`ModelBackend`]: a thin, allocation-cheap wrapper — safe to
/// build fresh on every [`ArtifactEnsurer`] call rather than caching it on
/// [`DownloaderEnsurer`] itself (no network I/O happens at construction,
/// only when a method below actually runs).
struct RealBackend(ModelDownloader);

impl ModelBackend for RealBackend {
    fn resolve_model(
        &self,
        size: ModelSize,
        coreml: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<ResolvedModel, ModelError> {
        self.0.resolve_model(size, coreml, progress, cancel)
    }

    fn ensure_custom_model(
        &self,
        url: &str,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        self.0.ensure_custom_model(url, progress, cancel)
    }

    fn resolve_hf_repo(
        &self,
        repo: &str,
        coreml: bool,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<ResolvedModel, ModelError> {
        self.0.resolve_hf_repo(repo, coreml, progress, cancel)
    }

    fn ensure_vad_model(
        &self,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, ModelError> {
        self.0.ensure_vad_model(progress, cancel)
    }
}

/// [`ArtifactEnsurer`] that downloads model/VAD artifacts on demand via
/// [`ModelDownloader`], configured as `services::init`'s
/// `RunnerDeps::artifacts` — the machinery that lets the GUI enqueue a task
/// with no model resolved yet (design spec §2.1).
pub struct DownloaderEnsurer {
    /// Root directory downloaded artifacts are cached under
    /// (`paths::model_root()`).
    pub root: PathBuf,
    /// Shared handle to the live settings. `models.custom_model_url` is read
    /// fresh from this handle on every [`ArtifactEnsurer::ensure_model`]
    /// call — never snapshotted at construction — so a URL the user sets in
    /// the Models tab *after* `services::init` already built this ensurer is
    /// still honored by the very next enqueued Custom-size task. The lock is
    /// held only long enough to clone the `Option<String>`, then dropped
    /// before any network I/O starts (Standing Policy: never hold a lock
    /// across blocking work).
    pub settings: Arc<Mutex<Settings>>,
    /// Whether to fetch the CoreML encoder alongside a ggml model (Apple
    /// Silicon only — mirrors the CLI's `coreml_encoder()`,
    /// `humm-cli/src/run.rs`).
    pub coreml: bool,
}

impl DownloaderEnsurer {
    /// A fresh real backend rooted at `self.root`.
    fn backend(&self) -> RealBackend {
        RealBackend(ModelDownloader::new(self.root.clone()))
    }

    /// [`ArtifactEnsurer::ensure_model`]'s real body, taking `backend` as an
    /// explicit parameter so tests can exercise the live-settings-read
    /// wiring against a [`ModelBackend`] fake — [`Self::ensure_model`]
    /// itself always calls this with [`Self::backend`]'s real,
    /// network-touching [`RealBackend`].
    fn ensure_model_against(
        &self,
        backend: &dyn ModelBackend,
        model: &ModelSpec,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError> {
        let custom_url = {
            let guard = self
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.models.custom_model_url.clone()
        };
        route_ensure_model(
            backend,
            model,
            custom_url.as_deref(),
            self.coreml,
            progress,
            cancel,
        )
    }
}

impl ArtifactEnsurer for DownloaderEnsurer {
    fn ensure_model(
        &self,
        model: &ModelSpec,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError> {
        self.ensure_model_against(&self.backend(), model, progress, cancel)
    }

    fn ensure_vad(
        &self,
        progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken,
    ) -> Option<PathBuf> {
        let disabled = vad_disabled_from_env(std::env::var("HUMM_DISABLE_VAD").ok().as_deref());
        route_ensure_vad(&self.backend(), &self.root, disabled, progress, cancel)
    }
}

/// [`DownloaderEnsurer::ensure_model`]'s routing, extracted as a free
/// function over the [`ModelBackend`] seam so it's unit-testable against a
/// fake without ever constructing a real (network-touching)
/// [`ModelDownloader`].
///
/// Order (verbatim from the task brief): an explicit `model.hf_repo` always
/// wins (a HuggingFace-repo task, whatever `model.size` happens to carry);
/// else [`ModelSize::Custom`] routes to the user's configured custom URL,
/// failing actionably — and never touching `backend` — if none is
/// configured; else the ordinary size-indexed path.
fn route_ensure_model(
    backend: &dyn ModelBackend,
    model: &ModelSpec,
    custom_url: Option<&str>,
    coreml: bool,
    progress: &mut dyn FnMut(DownloadProgress),
    cancel: &CancelToken,
) -> Result<PathBuf, CoreError> {
    if let Some(repo) = model.hf_repo.as_deref() {
        return backend
            .resolve_hf_repo(repo, coreml, progress, cancel)
            .map(|resolved| resolved.path)
            .map_err(CoreError::from);
    }

    if model.size == ModelSize::Custom {
        let Some(url) = custom_url else {
            return Err(CoreError::Model(ModelError::Unsupported {
                detail: format!(
                    "no custom model URL configured — set models.custom_model_url in {} or the \
                     Models tab",
                    paths::settings_path().display()
                ),
            }));
        };
        return backend
            .ensure_custom_model(url, progress, cancel)
            .map_err(CoreError::from);
    }

    backend
        .resolve_model(model.size, coreml, progress, cancel)
        .map(|resolved| resolved.path)
        .map_err(CoreError::from)
}

/// [`DownloaderEnsurer::ensure_vad`]'s routing, mirroring the CLI's
/// `ensure_vad` (`humm-cli/src/run.rs:426-450`) exactly: `disabled` short-
/// circuits to `None` (the `HUMM_DISABLE_VAD` test-only kill switch, already
/// resolved by the caller so this stays testable without touching real
/// process env); else a locally-cached copy is used as-is, no network touch;
/// else a fetch is attempted, degrading to `None` (logged) on any failure —
/// VAD failure must never fail the task.
fn route_ensure_vad(
    backend: &dyn ModelBackend,
    root: &Path,
    disabled: bool,
    progress: &mut dyn FnMut(DownloadProgress),
    cancel: &CancelToken,
) -> Option<PathBuf> {
    if disabled {
        return None;
    }
    if let Some(path) = local_vad_model_path(root) {
        return Some(path);
    }
    match backend.ensure_vad_model(progress, cancel) {
        Ok(path) => Some(path),
        Err(err) => {
            log::warn!("VAD model unavailable ({err}); continuing without VAD");
            None
        }
    }
}

/// [`EngineFactory`] that loads a real [`humm_whisper::WhisperEngine`] from
/// the model path a [`DownloaderEnsurer`] already resolved for this job.
pub struct GuiEngineFactory {
    /// Whether to allow GPU (Metal/CoreML) acceleration when loading the
    /// engine — resolved once at startup from `HUMM_FORCE_CPU`
    /// (`services::init`, mirroring the CLI's own `use_gpu_from_env`).
    pub use_gpu: bool,
}

impl EngineFactory for GuiEngineFactory {
    /// `resolved` is `None` only if `RunnerDeps::artifacts` is absent —
    /// unreachable in `services::init`'s real wiring (it always configures a
    /// [`DownloaderEnsurer`]), so this is a programmer-error guard, not a
    /// normal-operation path: a `None` here means the artifact-ensure wiring
    /// itself is missing, not that a download failed (a real download
    /// failure surfaces as `ensure_model`'s own `Err`, which never reaches
    /// `create` at all — see `Runner::run_job`).
    fn create(
        &mut self,
        _model: &ModelSpec,
        resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        let Some(path) = resolved else {
            return Err(CoreError::Engine(EngineError::ModelLoad {
                path: PathBuf::from("(no model path resolved)"),
                detail: "internal error: engine requested without a resolved model path — \
                         artifact ensure was not wired"
                    .to_string(),
            }));
        };
        humm_whisper::WhisperEngine::load(path, self.use_gpu)
            .map(|engine| Box::new(engine) as Box<dyn TranscriptionEngine>)
            .map_err(CoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::Backend;
    use pretty_assertions::assert_eq;
    use std::cell::RefCell;

    /// Records which [`ModelBackend`] method(s) were called, and plays back
    /// one scripted result per method (a test sets only the field(s) its
    /// scenario expects to be reached — an unset field's method call panics,
    /// which is exactly how "never touches the backend" gets proven).
    #[derive(Default)]
    struct FakeBackend {
        calls: RefCell<Vec<String>>,
        resolve_model_result: RefCell<Option<Result<ResolvedModel, ModelError>>>,
        ensure_custom_result: RefCell<Option<Result<PathBuf, ModelError>>>,
        resolve_hf_result: RefCell<Option<Result<ResolvedModel, ModelError>>>,
        ensure_vad_result: RefCell<Option<Result<PathBuf, ModelError>>>,
    }

    impl ModelBackend for FakeBackend {
        fn resolve_model(
            &self,
            _size: ModelSize,
            _coreml: bool,
            progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Result<ResolvedModel, ModelError> {
            self.calls.borrow_mut().push("resolve_model".to_string());
            progress(DownloadProgress {
                file: "ggml-small.bin".to_string(),
                downloaded: 10,
                total: Some(20),
            });
            self.resolve_model_result
                .borrow_mut()
                .take()
                .expect("resolve_model was not scripted for this test")
        }

        fn ensure_custom_model(
            &self,
            url: &str,
            progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Result<PathBuf, ModelError> {
            self.calls
                .borrow_mut()
                .push(format!("ensure_custom_model:{url}"));
            progress(DownloadProgress {
                file: "ggml-custom.bin".to_string(),
                downloaded: 5,
                total: None,
            });
            self.ensure_custom_result
                .borrow_mut()
                .take()
                .expect("ensure_custom_model was not scripted for this test")
        }

        fn resolve_hf_repo(
            &self,
            _repo: &str,
            _coreml: bool,
            progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Result<ResolvedModel, ModelError> {
            self.calls.borrow_mut().push("resolve_hf_repo".to_string());
            progress(DownloadProgress {
                file: "ggml-hf.bin".to_string(),
                downloaded: 1,
                total: Some(2),
            });
            self.resolve_hf_result
                .borrow_mut()
                .take()
                .expect("resolve_hf_repo was not scripted for this test")
        }

        fn ensure_vad_model(
            &self,
            progress: &mut dyn FnMut(DownloadProgress),
            _cancel: &CancelToken,
        ) -> Result<PathBuf, ModelError> {
            self.calls.borrow_mut().push("ensure_vad_model".to_string());
            progress(DownloadProgress {
                file: "ggml-silero-v6.2.0.bin".to_string(),
                downloaded: 1,
                total: Some(2),
            });
            self.ensure_vad_result
                .borrow_mut()
                .take()
                .expect("ensure_vad_model was not scripted for this test")
        }
    }

    fn model(size: ModelSize, hf_repo: Option<&str>) -> ModelSpec {
        ModelSpec {
            backend: Backend::Whisper,
            size,
            hf_repo: hf_repo.map(str::to_string),
        }
    }

    // -- ensure_model routing -------------------------------------------------

    #[test]
    fn hf_repo_routes_to_resolve_hf_repo_even_with_custom_size() {
        let backend = FakeBackend::default();
        *backend.resolve_hf_result.borrow_mut() = Some(Ok(ResolvedModel {
            path: PathBuf::from("/cache/ggml-hf.bin"),
            coreml_degraded: false,
        }));
        let mut seen = Vec::new();
        let mut progress = |update: DownloadProgress| seen.push(update);

        // hf_repo wins even when size is nominally Custom.
        let spec = model(ModelSize::Custom, Some("openai/whisper-tiny"));
        let result = route_ensure_model(
            &backend,
            &spec,
            None,
            true,
            &mut progress,
            &CancelToken::new(),
        );

        assert_eq!(result.expect("ok"), PathBuf::from("/cache/ggml-hf.bin"));
        assert_eq!(*backend.calls.borrow(), vec!["resolve_hf_repo".to_string()]);
        assert_eq!(seen.len(), 1, "progress must pass through to the caller");
    }

    #[test]
    fn custom_size_with_url_routes_to_ensure_custom_model() {
        let backend = FakeBackend::default();
        *backend.ensure_custom_result.borrow_mut() =
            Some(Ok(PathBuf::from("/cache/ggml-custom.bin")));
        let mut seen = Vec::new();
        let mut progress = |update: DownloadProgress| seen.push(update);

        let spec = model(ModelSize::Custom, None);
        let result = route_ensure_model(
            &backend,
            &spec,
            Some("https://example.com/model.bin"),
            false,
            &mut progress,
            &CancelToken::new(),
        );

        assert_eq!(result.expect("ok"), PathBuf::from("/cache/ggml-custom.bin"));
        assert_eq!(
            *backend.calls.borrow(),
            vec!["ensure_custom_model:https://example.com/model.bin".to_string()]
        );
        assert_eq!(seen.len(), 1, "progress must pass through to the caller");
    }

    #[test]
    fn custom_size_without_url_fails_actionably_and_never_touches_the_backend() {
        let backend = FakeBackend::default();
        let mut progress = |_: DownloadProgress| panic!("must not be called");

        let spec = model(ModelSize::Custom, None);
        let err = route_ensure_model(
            &backend,
            &spec,
            None,
            false,
            &mut progress,
            &CancelToken::new(),
        )
        .expect_err("must fail without a configured custom URL");

        let message = err.to_string();
        assert!(
            message.contains("models.custom_model_url"),
            "message should name the setting: {message}"
        );
        assert!(
            message.contains(&paths::settings_path().display().to_string()),
            "message should name the settings path: {message}"
        );
        assert!(
            backend.calls.borrow().is_empty(),
            "the backend must never be touched when no custom URL is configured"
        );
    }

    #[test]
    fn plain_size_routes_to_resolve_model() {
        let backend = FakeBackend::default();
        *backend.resolve_model_result.borrow_mut() = Some(Ok(ResolvedModel {
            path: PathBuf::from("/cache/ggml-small.bin"),
            coreml_degraded: false,
        }));
        let mut seen = Vec::new();
        let mut progress = |update: DownloadProgress| seen.push(update);

        let spec = model(ModelSize::Small, None);
        let result = route_ensure_model(
            &backend,
            &spec,
            None,
            true,
            &mut progress,
            &CancelToken::new(),
        );

        assert_eq!(result.expect("ok"), PathBuf::from("/cache/ggml-small.bin"));
        assert_eq!(*backend.calls.borrow(), vec!["resolve_model".to_string()]);
        assert_eq!(seen.len(), 1, "progress must pass through to the caller");
    }

    #[test]
    fn resolve_model_error_propagates_as_core_error() {
        let backend = FakeBackend::default();
        *backend.resolve_model_result.borrow_mut() = Some(Err(ModelError::Http {
            detail: "connection refused".to_string(),
        }));
        let mut progress = |_: DownloadProgress| {};

        let spec = model(ModelSize::Small, None);
        let err = route_ensure_model(
            &backend,
            &spec,
            None,
            false,
            &mut progress,
            &CancelToken::new(),
        )
        .expect_err("must propagate the backend's error");
        assert!(matches!(err, CoreError::Model(ModelError::Http { .. })));
    }

    // -- DownloaderEnsurer::ensure_model: live settings read (Task: final-
    //    review fix 1) ------------------------------------------------------

    #[test]
    fn ensure_model_honors_a_custom_url_set_after_construction() {
        let backend = FakeBackend::default();
        *backend.ensure_custom_result.borrow_mut() =
            Some(Ok(PathBuf::from("/cache/ggml-late.bin")));
        let settings = Arc::new(Mutex::new(Settings::default()));
        let ensurer = DownloaderEnsurer {
            root: PathBuf::from("/unused-root"),
            settings: Arc::clone(&settings),
            coreml: false,
        };
        // The ensurer was already constructed above with no URL configured
        // — this mutates the *shared* settings handle afterward, exactly as
        // a Preferences-tab edit would between `services::init` and a later
        // `ensure_model` call.
        settings
            .lock()
            .expect("settings lock")
            .models
            .custom_model_url = Some("https://example.com/late.bin".to_string());

        let mut seen = Vec::new();
        let mut progress = |update: DownloadProgress| seen.push(update);
        let spec = model(ModelSize::Custom, None);
        let result =
            ensurer.ensure_model_against(&backend, &spec, &mut progress, &CancelToken::new());

        assert_eq!(result.expect("ok"), PathBuf::from("/cache/ggml-late.bin"));
        assert_eq!(
            *backend.calls.borrow(),
            vec!["ensure_custom_model:https://example.com/late.bin".to_string()]
        );
    }

    #[test]
    fn ensure_model_still_fails_actionably_when_the_live_url_is_unset() {
        let backend = FakeBackend::default();
        let settings = Arc::new(Mutex::new(Settings::default()));
        let ensurer = DownloaderEnsurer {
            root: PathBuf::from("/unused-root"),
            settings,
            coreml: false,
        };
        let mut progress = |_: DownloadProgress| panic!("must not be called");
        let spec = model(ModelSize::Custom, None);
        let err = ensurer
            .ensure_model_against(&backend, &spec, &mut progress, &CancelToken::new())
            .expect_err("must fail without a configured custom URL");

        let message = err.to_string();
        assert!(
            message.contains("models.custom_model_url"),
            "message should name the setting: {message}"
        );
        assert!(
            backend.calls.borrow().is_empty(),
            "the backend must never be touched when no custom URL is configured"
        );
    }

    // -- ensure_vad routing ----------------------------------------------------

    #[test]
    fn vad_disabled_returns_none_without_touching_the_backend() {
        let backend = FakeBackend::default();
        let mut progress = |_: DownloadProgress| panic!("must not be called");
        let root = tempfile::tempdir().expect("tempdir");

        let result = route_ensure_vad(
            &backend,
            root.path(),
            true,
            &mut progress,
            &CancelToken::new(),
        );

        assert_eq!(result, None);
        assert!(backend.calls.borrow().is_empty());
    }

    #[test]
    fn vad_cached_locally_returns_its_path_without_touching_the_backend() {
        let backend = FakeBackend::default();
        let mut progress = |_: DownloadProgress| panic!("must not be called");
        let root = tempfile::tempdir().expect("tempdir");
        let vad_file = root.path().join("ggml-silero-v6.2.0.bin");
        std::fs::write(&vad_file, b"fake vad model").expect("write vad file");
        std::fs::write(
            root.path().join("ggml-silero-v6.2.0.bin.humm_complete"),
            b"",
        )
        .expect("write sentinel");

        let result = route_ensure_vad(
            &backend,
            root.path(),
            false,
            &mut progress,
            &CancelToken::new(),
        );

        assert_eq!(result, Some(vad_file));
        assert!(backend.calls.borrow().is_empty());
    }

    #[test]
    fn vad_fetch_success_returns_the_downloaded_path() {
        let backend = FakeBackend::default();
        *backend.ensure_vad_result.borrow_mut() =
            Some(Ok(PathBuf::from("/cache/ggml-silero-v6.2.0.bin")));
        let mut seen = Vec::new();
        let mut progress = |update: DownloadProgress| seen.push(update);
        let root = tempfile::tempdir().expect("tempdir");

        let result = route_ensure_vad(
            &backend,
            root.path(),
            false,
            &mut progress,
            &CancelToken::new(),
        );

        assert_eq!(result, Some(PathBuf::from("/cache/ggml-silero-v6.2.0.bin")));
        assert_eq!(
            *backend.calls.borrow(),
            vec!["ensure_vad_model".to_string()]
        );
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn vad_fetch_failure_degrades_to_none_never_fails() {
        let backend = FakeBackend::default();
        *backend.ensure_vad_result.borrow_mut() = Some(Err(ModelError::Http {
            detail: "offline".to_string(),
        }));
        let mut progress = |_: DownloadProgress| {};
        let root = tempfile::tempdir().expect("tempdir");

        let result = route_ensure_vad(
            &backend,
            root.path(),
            false,
            &mut progress,
            &CancelToken::new(),
        );

        assert_eq!(result, None, "a VAD fetch failure must degrade, never fail");
        assert_eq!(
            *backend.calls.borrow(),
            vec!["ensure_vad_model".to_string()]
        );
    }

    // -- GuiEngineFactory --------------------------------------------------------

    #[test]
    fn gui_engine_factory_fails_actionably_without_a_resolved_path() {
        let mut factory = GuiEngineFactory { use_gpu: false };
        let spec = model(ModelSize::Small, None);
        let err = match factory.create(&spec, None) {
            Ok(_) => panic!("expected a failure without a resolved model path"),
            Err(err) => err,
        };
        let message = err.to_string();
        assert!(
            message.contains("artifact ensure was not wired"),
            "expected the programmer-error message, got: {message}"
        );
    }
}
