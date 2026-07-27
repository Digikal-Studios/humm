//! `HummApp`: the `eframe::App` implementation — owns the runtime
//! [`Services`], the shared event-pump buffer, the task table's
//! view-model/layout, and (F-004/F-026, this task) the toolbar/menu/dialog/
//! toast/banner state, rendering the main window every frame (design spec
//! §1/§4).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use humm_core::boundary::{AudioCapture, AudioDecoder, TaskStore};
use humm_core::cancel::CancelToken;
use humm_core::domain::{Backend, ModelSize, ModelSpec, Segment, Source, Status, TranscriptionId};
use humm_core::env::use_gpu_from_env;
use humm_core::error::{CoreError, EngineError};
use humm_core::folderwatch::SeenSet;
use humm_core::live::{
    ExportSink, RecordingMode, SessionConfig, TranslationApplier, effective_upload_url,
};
use humm_core::models::download::DownloadProgress;
use humm_core::openai::{
    ENV_BASE, ENV_KEY, OpenAiEngineConfig, TranslatorConfig, resolve_endpoint,
};
use humm_core::queue::{ArtifactEnsurer, QueueHandle, TaskEvent, TaskEventKind};
use humm_core::settings::{FileTranscriberSettings, GuiSettings, Settings, ViewerGeometry};
use humm_media::{FfmpegDecoder, FfmpegVideoStreamer, FfprobeProber};
use humm_whisper::LiveOptions;

use crate::engine_factory::{DownloaderEnsurer, resolve_openai_config};
use crate::events::{self, SharedEventQueue};
use crate::folder_watch::{FolderWatchService, FolderWatchSnapshot, ReconcileAction};
use crate::fonts;
use crate::i18n::{self, tr};
use crate::import::{FileDialogs, ImportRequest, RfdFileDialogs, scan_folder};
use crate::live_fixture::FixtureCapture;
use crate::live_session::{
    self, LiveEngineFactory, LiveEngineSession, LiveSessionRequest, LiveStore, LiveUpload,
    SessionMessage, UploadFactory,
};
use crate::live_translation::LiveTranslator;
use crate::recording::{RecordingDeps, RecordingStatus, RecordingVm, measured_speed_key};
use crate::services::{self, Services, UpdateNotice};
use crate::shortcuts::ShortcutMap;
use crate::speaker_id::{self, SpeakerIdDeps, SpeakerIdStage, SpeakerIdVm};
use crate::viewer::{ViewerDeps, ViewerVm};
use crate::viewmodel::{TableAction, TasksVm};
use crate::views;
use crate::views::about::{self, AboutCheckResult, AboutState};
use crate::views::main_window::{
    ActionState, FolderWatchPillState, PendingImportRequest, toolbar_enablement,
};
use crate::views::options_sheet::{OptionsSheet, SheetOutcome, SheetState};
use crate::views::preferences::{self, PreferencesState};
use crate::views::prefs_general;
use crate::views::rename_notes::{self, DialogOutcome};
use crate::views::resizer::{self, ResizerAction, ResizerVm};
use crate::views::task_table::TableLayout;
use crate::views::toasts::{ToastAction, ToastKind};
use crate::views::update_notice::UpdateNoticeState;
use crate::views::url_dialog::UrlDialog;

/// The version string every version-facing view (the update dialog's
/// "Current version: …", the About window's "Version …") shows — the crate's
/// own `Cargo.toml` version at build time, the same value `services.rs`
/// feeds [`humm_core::update::check_for_update`] as `current_version`.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A GUI-internal event a background worker sends back to the app (design spec
/// §1.2). Distinct from [`humm_core::queue::TaskEvent`] (the queue's own
/// lifecycle stream): this carries viewer-side effects that must run on the UI
/// thread. Its producers are the resize/merge feature (F-086, the merge
/// worker thread; the synchronous extend/resize path applies the same effect
/// inline via [`HummApp::apply_transcriptions_updated`]) and the Speaker-ID
/// window's Save (F-087, always synchronous — see `speaker_id`'s module doc).
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// A resize/merge/speaker-id save produced or replaced a transcript's
    /// segments: refresh (or insert) the `affected` transcript's table row,
    /// then — only when `close_source` is set — close the `source`
    /// transcript's viewer *and* its resizer.
    ///
    /// The resizer (Buzz `transcriptions_updated_signal` → viewer `close`,
    /// `transcription_viewer_widget.py:1333`; humm also closes the resizer)
    /// always sends `close_source: true`. The Speaker-ID window
    /// (F-087) always sends `false`: Buzz's own widget only closes itself
    /// (`speaker_identification_widget.py:800`) — the viewer closing too, in
    /// Buzz, is an accident of a *different*, shared wire (see `speaker_id`'s
    /// module doc's Save section for the full story) that this explicit field
    /// deliberately does not reproduce.
    ///
    /// `affected == source` for an in-place save; `affected` is the new
    /// copy's id for a "Create new transcript" (or speaker-id) save.
    TranscriptionsUpdated {
        /// The transcript whose table row must be refreshed/inserted.
        affected: TranscriptionId,
        /// The source transcript whose viewer (and resizer, if
        /// `close_source`) opened the operation.
        source: TranscriptionId,
        /// Whether `source`'s viewer (and resizer) must close.
        close_source: bool,
    },
}

/// The eframe application.
pub struct HummApp {
    services: Services,
    /// Shared buffer the event-pump thread feeds. `None` until the pump is
    /// spawned: eagerly in [`HummApp::new`] (a real `egui::Context` is
    /// available immediately from [`eframe::CreationContext`]); lazily, on
    /// the first `eframe::App::ui` call, in [`HummApp::new_for_test`] (which
    /// has no `CreationContext` to draw one from — see that constructor's
    /// doc comment).
    event_queue: Option<SharedEventQueue>,
    /// The task table's view-model (F-003): loaded once at construction
    /// (`TasksVm::load`), then kept current by folding in each drained
    /// [`humm_core::queue::TaskEvent`] (`TasksVm::apply`) rather than ever
    /// re-querying the whole table.
    tasks: TasksVm,
    /// The task table's persisted column layout (visible/order); sort state
    /// itself lives on `tasks` (`TasksVm::sort`).
    table_layout: TableLayout,
    /// The `[gui]` settings snapshot last written to disk — compared against
    /// the current `table_layout`/`tasks.sort()` snapshot every frame so a
    /// change is persisted exactly once, not re-saved on every unchanged
    /// frame.
    last_saved_gui_settings: GuiSettings,
    /// `TableAction`s the task table/toolbar/shortcuts have emitted
    /// (retry/rename/notes/clear/open/cancel) this frame — drained every
    /// frame by [`Self::drain_table_actions`], *after* `main_window::show`
    /// returns (so actions pushed by this same frame's clicks are the ones
    /// executed, not last frame's).
    pending_table_actions: Vec<TableAction>,
    /// The resolved keyboard-shortcut bindings: Buzz's F-005 defaults with
    /// `settings.shortcuts` overrides applied (`ShortcutMap::from_settings`),
    /// seeded at startup and rebuilt whenever [`Self::shortcuts_generation`]
    /// moves (the shortcuts-editor tab's own write path,
    /// `views/prefs_shortcuts.rs`, Task 14). `Arc` so it can be cloned into
    /// each viewer's deferred-viewport render closure (which egui requires
    /// be `Send + Sync + 'static`) — every consumer (main window, viewer,
    /// search) reads this fresh every frame rather than caching it, so a
    /// rebuilt `Arc` reaches all of them the very next frame with no
    /// per-consumer wiring (`show_viewer_viewports`/`show_resizer_viewports`
    /// re-`Arc::clone` this field into their deferred closures every single
    /// call, and `views::main_window::show` takes it by reference directly —
    /// see [`Self::rebuild_shortcuts_if_generation_changed`]'s own doc for
    /// the rebuild trigger).
    shortcuts: Arc<ShortcutMap>,
    /// Bumped by the shortcuts-editor tab (`views/prefs_shortcuts.rs`'s
    /// `commit_capture`/`commit_clear`/`commit_reset_all`) every time
    /// `settings.shortcuts` changes. Read-and-compared-against
    /// [`Self::last_shortcuts_generation`] once per frame
    /// ([`Self::rebuild_shortcuts_if_generation_changed`]) to decide whether
    /// [`Self::shortcuts`] needs rebuilding. Both the write (inside the
    /// Preferences window, an *immediate* viewport rendered inline on the
    /// main UI thread) and the read happen on the same thread, so this is
    /// never actually contended — the `Arc<AtomicUsize>` (rather than a bare
    /// `AtomicUsize` field) exists only because `views::prefs_shortcuts::ui`
    /// needs its own clone-able handle to bump it, the same reason
    /// `services.update_tx`-style channels get cloned into view code
    /// elsewhere in this module.
    shortcuts_generation: Arc<AtomicUsize>,
    /// The last [`Self::shortcuts_generation`] value [`Self::shortcuts`] was
    /// rebuilt from — starts at `0`, matching the counter's own initial
    /// value, so the first frame never spuriously rebuilds.
    last_shortcuts_generation: usize,
    /// Open transcript viewers (F-080), one per transcript, keyed by id — each
    /// a deferred egui viewport (`Self::show_viewer_viewports`). The `Arc<
    /// Mutex<_>>` is the shared handle the deferred render closure locks; the
    /// app harvests each viewer's `close_requested` flag every frame and drops
    /// the entry (which drops the VM) when it is set.
    viewers: BTreeMap<TranscriptionId, Arc<Mutex<ViewerVm>>>,
    /// Open resizer windows (F-086), one per **source** transcript, keyed by
    /// that source id — each a deferred egui viewport
    /// (`Self::show_resizer_viewports`). Opened by a viewer's Resize button;
    /// closed when the operation lands, when the user closes the window, or
    /// when the originating viewer closes (Buzz `closeEvent` closes its
    /// resizer). The `Arc<Mutex<_>>` is shared with both the deferred render
    /// closure and (for merge) the worker thread.
    resizers: BTreeMap<TranscriptionId, Arc<Mutex<ResizerVm>>>,
    /// Open Speaker-ID windows (F-087), one per **source** transcript, keyed
    /// by that source id — each a deferred egui viewport
    /// (`Self::show_speaker_viewports`). Opened by a viewer's Identify
    /// Speakers button; closed on save, when the user closes the window, or
    /// when the originating viewer closes (Buzz `closeEvent` closes its
    /// speaker-id dialog, `transcription_viewer_widget.py:1495-1496`). The
    /// `Arc<Mutex<_>>` is shared with the deferred render closure and the
    /// background Identify worker.
    speakers: BTreeMap<TranscriptionId, Arc<Mutex<SpeakerIdVm>>>,
    /// The adapter bundle every viewer window shares (built once from the real
    /// ffmpeg/cpal/rfd adapters). Cloned into each deferred-viewport closure.
    viewer_deps: Arc<ViewerDeps>,
    /// The Speaker-ID window's adapter bundle (F-087): reuses `viewer_deps`'s
    /// own decoder/audio-output `Arc`s (this window decodes/plays back the
    /// same source file a viewer would) plus `services.store` and the
    /// diarization seam ([`Services::diarize`] — real in production, a
    /// scripted double in tests).
    speaker_deps: Arc<SpeakerIdDeps>,
    /// The live-recording window (F-060), single instance — `None` until
    /// opened via the toolbar Record button or Ctrl+R
    /// (`self.action_state.record_requested`), then a deferred egui viewport
    /// (`Self::show_recording_viewport`), mirroring the viewer/resizer
    /// windows' own `Arc<Mutex<_>>` shape.
    recording: Option<Arc<Mutex<RecordingVm>>>,
    /// The recording window's adapter bundle (F-060): the audio-capture
    /// seam, real (`humm_audio::CpalCapture`) in [`Self::new`], scripted
    /// (`humm_audio::MockCapture`) in [`Self::new_for_test`] — a real
    /// `CpalCapture::start` opens an actual input stream, which no test in
    /// this workspace may ever touch (the same "fakeable in tests" rule
    /// `viewer_deps`'s decoder/prober/streamer/audio_output already follow,
    /// just injected at construction here since [`AudioCapture::start`]
    /// needs `&mut self`).
    recording_deps: Arc<RecordingDeps>,
    /// Whether the live-recording session's engine load may use GPU
    /// (Metal/CoreML) acceleration — resolved once at startup from
    /// `HUMM_FORCE_CPU` (`Self::new`, mirroring `services::init`'s own
    /// `use_gpu_from_env`; always `false` in [`Self::new_for_test`], which
    /// never reads real process env — final-review fix 8's own rule applied
    /// here too).
    use_gpu: bool,
    /// GUI-internal event channel (design spec §1.2): resize/merge workers
    /// (Task 12) send [`AppEvent`]s that the app drains each frame. The sender
    /// is clonable into those workers; dormant until Task 12 wires a sender.
    app_event_tx: mpsc::Sender<AppEvent>,
    /// The receiving end of [`Self::app_event_tx`], drained each frame.
    app_event_rx: mpsc::Receiver<AppEvent>,
    /// Toolbar/menu/dialog/toast/banner state (F-004/F-026): see
    /// [`ActionState`]'s own field docs.
    action_state: ActionState,
    /// Seam over the (blocking, native) file/folder pickers (F-020/F-022) —
    /// always [`RfdFileDialogs`] in both real and kittest construction;
    /// nothing in this milestone's kittest coverage drives it (the sheet
    /// itself is opened directly via [`HummApp::open_options_sheet_for_test`]
    /// instead), but the seam exists so a future test can inject a fake
    /// without touching call sites.
    dialogs: Box<dyn FileDialogs>,
    /// Files passed as argv (Finder "open with" parity, design spec
    /// deviation #12) — opened as the options sheet's request on the first
    /// `eframe::App::ui` call, then cleared so it never re-triggers. Empty
    /// (the common case) is a permanent no-op.
    pending_argv_import: Vec<PathBuf>,
    /// Whether [`services::fire_launch_telemetry`] has already run this
    /// launch — set on the very first `eframe::App::ui` call so the one
    /// `app_launched` event fires exactly once (F-007, ADR-009), never
    /// again after.
    telemetry_fired: bool,
    /// Whether the About window (a real OS viewport,
    /// [`Self::show_about_viewport`]) is currently open.
    about_open: bool,
    /// The About window's own state (manual-check in-flight flag + last
    /// result) — persists across the window closing/reopening within one
    /// launch, same as Buzz's dialog would if kept alive.
    about_state: AboutState,
    /// Whether the Preferences window (a real OS viewport,
    /// [`Self::show_preferences_viewport`]) is currently open.
    preferences_open: bool,
    /// The Preferences window's state, rebuilt fresh (Models tab re-scanned
    /// from disk) each time it's opened; `None` while closed.
    preferences_state: Option<PreferencesState>,
    /// Environment-gated smoke-test hooks (`HUMM_SMOKE_OPEN_WINDOWS`/
    /// `HUMM_SMOKE_AUTO_CLOSE_FRAMES`/`HUMM_SMOKE_OPEN_VIEWER`, see
    /// [`SmokeHooks`]) — read once from real process env in [`Self::new`]
    /// (never in [`Self::new_for_test`], which always passes
    /// [`SmokeHooks::default`] — final-review fix 8, so a kittest harness can
    /// never pick one up by accident) and threaded into [`Self::build`], never
    /// re-read per frame. Exist solely so the `gui-manual` windowed smokes
    /// (`docs/gui-manual/M3.md` item 35's multi-window smoke;
    /// `docs/gui-manual/M4.md`'s A/V-drift run) can run under a controller that
    /// cannot inject OS-level input (Accessibility permissions unavailable in
    /// the automation environment); test-only, unstable, documented as such in
    /// `docs/HUMM_SURFACE.md` §5.1.
    smoke_hooks: SmokeHooks,
    /// One-shot guard for `smoke_hooks.open_windows`'s effect — applies on
    /// the first `eframe::App::ui` call only, same once-guard shape as
    /// `telemetry_fired`.
    smoke_windows_opened: bool,
    /// Frames rendered since construction; only consulted when
    /// `smoke_hooks.auto_close_frames` is `Some`, to know when that many
    /// frames have elapsed and shutdown should be requested.
    smoke_frame_count: u32,
    /// One-shot guard for `smoke_hooks.open_viewer`'s effect — seeds+opens the
    /// smoke viewer on the first `eframe::App::ui` call only.
    smoke_viewer_opened: bool,
    /// The transcript id the `HUMM_SMOKE_OPEN_VIEWER` hook seeded and opened;
    /// `Some` once armed, used to detect when its window has auto-closed at
    /// end-of-media so the app itself can then shut down cleanly.
    smoke_viewer_id: Option<TranscriptionId>,
    /// `HUMM_SMOKE_LIVE_FIXTURE`'s own state machine
    /// ([`HummApp::apply_smoke_live_fixture_tick`], spec §8 acceptance item
    /// 1): `None` until the hook opens the recording window and clicks
    /// Record (the recording window's own `RecordingVm` — reachable via
    /// `self.recording` — is the source of truth for whether a session is
    /// still active; this only remembers *when* Record was clicked and
    /// whether Stop has been latched yet).
    smoke_live_fixture_state: Option<SmokeLiveFixtureState>,
    /// The running queue handle, `Arc`-wrapped so the folder-watch service's
    /// worker thread can share it (F-030): `humm_core::queue::QueueHandle`
    /// deliberately has no `Clone` impl (it uniquely owns the runner
    /// thread's `JoinHandle` — only one thing may ever call `finish`/
    /// `finish_within` on it), so rather than each ordinary call site
    /// borrowing `&QueueHandle` through `Services::queue()` (as before this
    /// task), `Self::build` takes `services.queue` out exactly once and
    /// wraps it here; [`Self::queue`] is the accessor every ordinary call
    /// site now uses instead of `self.services.queue()`. `Option` for the
    /// same reason as `Services::queue`: `on_exit` takes it to reclaim sole
    /// ownership (`Arc::try_unwrap`) for [`shutdown_queue`], which only
    /// succeeds once `folder_watch` (stopped first) has dropped its own
    /// clone — see [`Self::on_exit`].
    queue: Option<Arc<QueueHandle>>,
    /// The folder-watch service (F-030, design spec §3.2) — `None` while
    /// disabled/unconfigured, or (transiently, logged) after a `start()`
    /// failure. Reconciled against live settings every frame
    /// ([`Self::sync_folder_watch`]) and stopped before the queue on
    /// shutdown ([`Self::on_exit`]), mirroring Buzz's own teardown order
    /// (`main_window.py:500-507` disconnects the folder watcher before
    /// `closeEvent` does anything else).
    folder_watch: Option<FolderWatchService>,
    /// The `[folder_watch]` snapshot [`Self::sync_folder_watch`] last acted
    /// on — *not* only when `folder_watch` is actually running: a `Stop`, a
    /// no-op (still-inactive settings), and a failed `start()` all cache
    /// their snapshot here too, so the very next frame's reconcile sees
    /// "already handled, nothing changed" (`ReconcileAction::NoChange`)
    /// instead of retrying (or re-logging a misconfiguration warning) every
    /// single frame at UI frame rate. Retrying only happens once this
    /// differs from a fresh snapshot again (e.g. the user fixes a bad path).
    folder_watch_snapshot: Option<FolderWatchSnapshot>,
    /// The folder-watch service's `SeenSet` (Buzz's `paths_emitted`),
    /// `Arc<Mutex<_>>`-shared across every `start()`/restart for this
    /// process's whole lifetime — constructed exactly once, in
    /// [`Self::build`], and never reset. Buzz's own `paths_emitted` lives on
    /// the one `TranscriptionTaskFolderWatcher` instance `MainWindow`
    /// constructs once and reuses across every `set_preferences` call
    /// (`main_window.py:167-183`); only a full app restart clears it. If
    /// humm instead gave each `start()` a *fresh* `SeenSet` (as an earlier
    /// revision of this module did), a prefs edit landing in the narrow
    /// window between a file being enqueued (`QueueHandle::enqueue` is
    /// fire-and-forget — the runner's own `create_task` hasn't necessarily
    /// run yet, so `store.has_task_for_file` can still say "not known") and
    /// that enqueue's `create_task` actually landing could double-enqueue
    /// the same file across the resulting restart; keeping the *same*
    /// `SeenSet` closes that window regardless of store timing, since rule 6
    /// (`humm_core::folderwatch::scan`, "seen at this exact size/mtime")
    /// still fires. Proven end to end by
    /// `tests/folder_watch_int.rs::seen_set_survives_a_rebuild_so_a_recently_enqueued_file_is_not_double_enqueued`.
    folder_watch_seen: Arc<Mutex<SeenSet>>,
}

impl HummApp {
    /// Builds the app for real use (`main.rs`), spawning the event pump
    /// immediately from the window's own [`egui::Context`]
    /// (`cc.egui_ctx`). `argv_import` is `main.rs`'s parsed command-line
    /// file paths (Finder "open with" parity) — opened as the options
    /// sheet's request on the first frame; an empty `Vec` (the common case)
    /// is a no-op.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        services: Services,
        argv_import: Vec<PathBuf>,
    ) -> Self {
        // CJK fallback font (F-008 gate, design spec §7.1): installed first,
        // before any locale/font-size setup below reads or writes
        // `cc.egui_ctx`'s style — without this, the zh_CN/zh_TW/ja_JP
        // catalogs (and Buzz's own `。？！` default punctuation under any
        // locale) render as tofu (`□`), since egui's compiled-in default
        // fonts carry no Han/Kana glyphs. See `fonts.rs`'s module doc for
        // the bundled font, its source, and the documented SC-glyph-shape
        // divergence for TC/JP text.
        fonts::install(&cc.egui_ctx);

        // Locale negotiation (F-008a, design spec §9.1): resolved exactly
        // once here, after `services::init` has already loaded
        // `settings.toml` but before this constructor returns an app for
        // eframe to ever paint a first frame — so that very first frame
        // already renders through the negotiated bundle. Buzz's own
        // equivalent (`locale.py:15-16`) reads `QLocale().name()`, which on
        // macOS surfaces the OS's System Settings › Language & Region
        // preference regardless of how the app was launched; humm's
        // env-based fallback (`i18n::locale_from_env_vars`) only sees
        // `LC_ALL`/`LANG`, which a Finder/Dock-launched macOS GUI process
        // essentially never has set (documented platform difference,
        // design spec §9.1) — in practice this resolves to `en_US` on a
        // typical macOS launch whenever `settings.ui.locale` is also unset.
        //
        // That deviation is now user-visible, where before the F-008b
        // catalogs landed it was inert: with `de_DE`/`es_ES`/`zh_CN`
        // embedded, a macOS user whose OS language is German gets English on
        // first launch, because the env vars Buzz's `QLocale()` doesn't need
        // simply aren't there. The Preferences › General language combo is
        // the working path around it — one explicit pick persists
        // `settings.ui.locale`, which takes precedence here forever after.
        // Closing the gap properly needs a native macOS locale lookup
        // (`NSLocale.preferredLanguages`); tracked as the §9.1 platform
        // deviation, not fixed here.
        let (requested_locale, startup_font_size) = {
            let settings = services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (settings.ui.locale.clone(), settings.ui.font_size)
        };
        let system_locale = i18n::locale_from_env_vars(
            std::env::var("LC_ALL").ok().as_deref(),
            std::env::var("LANG").ok().as_deref(),
        );
        let negotiated = i18n::negotiate(requested_locale.as_deref().or(system_locale.as_deref()));
        i18n::set_locale(negotiated.buzz_code);

        // Font-size application (F-100, design spec §8.1): same "resolved
        // once here, before the first frame paints" contract as the locale
        // negotiation just above — `cc.egui_ctx` is the one handle every
        // later `apply_font_size` call (the General tab's own DragValue/
        // checkbox, `views::prefs_general::ui`) also mutates through.
        prefs_general::apply_font_size(&cc.egui_ctx, startup_font_size);

        let event_queue = events::spawn_event_pump(services.events.clone(), cc.egui_ctx.clone());
        // Smoke-test hooks (see `SmokeHooks`'s doc comment): read from real
        // process env only on this, the real construction path — never in
        // `new_for_test`, so a kittest harness can never accidentally pick
        // up a `HUMM_SMOKE_*` var some other test (or the developer's own
        // shell) happened to leave set (final-review fix 8).
        let smoke_hooks = smoke_hooks_from_env(
            std::env::var("HUMM_SMOKE_OPEN_WINDOWS").ok().as_deref(),
            std::env::var("HUMM_SMOKE_AUTO_CLOSE_FRAMES")
                .ok()
                .as_deref(),
            std::env::var("HUMM_SMOKE_OPEN_VIEWER").ok().as_deref(),
            std::env::var("HUMM_SMOKE_LIVE_FIXTURE").ok().as_deref(),
            std::env::var("HUMM_SMOKE_LIVE_SECONDS").ok().as_deref(),
        );
        let use_gpu = use_gpu_from_env(std::env::var("HUMM_FORCE_CPU").ok().as_deref());
        // Mirrors `services::init`'s own `DownloaderEnsurer` construction
        // (`root`/`settings`/`coreml`) exactly — the queue's instance is
        // already consumed into its `RunnerDeps` and not re-exposed from
        // `Services`, so the recording window gets its own, otherwise
        // identical, instance.
        let model_ensurer = Arc::new(DownloaderEnsurer {
            root: services.model_root.clone(),
            settings: Arc::clone(&services.settings),
            coreml: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        });
        // `HUMM_SMOKE_LIVE_FIXTURE` (M5 Task 14, spec §8 item 1): swap the
        // real `CpalCapture` for a `FixtureCapture` decoded from the given
        // media path — see that module's doc for why (a real mic open hangs
        // on an unanswered TCC prompt in this environment; the live
        // realtime-fit acceptance run needs *some* wall-clock-paced audio
        // source feeding the real session). Everything else about this
        // build stays the real production path (real model ensurer, real
        // engine factory, real store) — only the capture adapter changes.
        // A decode failure (bad path, no ffmpeg) logs and falls back to the
        // real `CpalCapture`, leaving the app otherwise unaffected; the
        // corresponding `apply_smoke_live_fixture_tick` no-ops whenever
        // `smoke_hooks.live_fixture` is `None`, which
        // `smoke_hooks_from_env`'s own contract guarantees here too (a
        // failed decode never sets it, see below).
        let mut smoke_hooks = smoke_hooks;
        let capture: Box<dyn AudioCapture> = match &smoke_hooks.live_fixture {
            Some(path) => match FfmpegDecoder::new().decode(path, &CancelToken::new()) {
                Ok(pcm) => {
                    let samples = pcm.samples().to_vec();
                    log::info!(
                        "HUMM_SMOKE_LIVE_FIXTURE: decoded {} samples (~{:.1}s at 16 kHz) from {}",
                        samples.len(),
                        samples.len() as f64 / f64::from(humm_core::boundary::SAMPLE_RATE),
                        path.display()
                    );
                    Box::new(FixtureCapture::new(samples))
                }
                Err(err) => {
                    log::warn!(
                        "HUMM_SMOKE_LIVE_FIXTURE: could not decode {}: {err} — falling back to \
                         the real microphone capture (the smoke hook will not fire)",
                        path.display()
                    );
                    smoke_hooks.live_fixture = None;
                    Box::new(humm_audio::CpalCapture::new())
                }
            },
            None => Box::new(humm_audio::CpalCapture::new()),
        };
        Self::build(
            services,
            Some(event_queue),
            argv_import,
            smoke_hooks,
            capture,
            model_ensurer,
            Arc::new(live_session::RealLiveEngineFactory) as Arc<dyn LiveEngineFactory>,
            Arc::new(live_session::RealUploadFactory) as Arc<dyn UploadFactory>,
            use_gpu,
        )
    }

    /// Builds the app directly from injected [`Services`], for kittest
    /// harnesses. Every later kittest task constructs `HummApp` this way.
    ///
    /// Unlike [`HummApp::new`], no [`eframe::CreationContext`] is available
    /// at this call site (the brief's constructor signature takes only
    /// `services`), so the event pump is instead spawned lazily on the first
    /// `eframe::App::ui` call, using the `egui::Context` that call
    /// receives — for an `egui_kittest` harness, that is the *same*
    /// `Context` its own `CreationContext` carries, so the net effect is
    /// identical; it just can't be captured before the first frame runs.
    ///
    /// Never carries a pending argv import (kittest harnesses have no real
    /// argv to speak of) — a test that wants to exercise the argv-import
    /// path opens the sheet directly via
    /// [`HummApp::open_options_sheet_for_test`], which is exactly what the
    /// real argv path itself resolves to on its first frame.
    pub fn new_for_test(services: Services) -> Self {
        // Never reads `HUMM_SMOKE_*` (final-review fix 8): a test harness
        // always gets the hooks' inert default, regardless of what happens
        // to be set in the process environment this test runs under.
        //
        // Never gets a real `CpalCapture` either, for the same reason: a
        // real capture's `start` opens an actual input stream, which no
        // kittest may touch (see `HummApp::recording_deps`'s own doc
        // comment). `MockCapture::new(Vec::new())` is a scripted, threadless,
        // hardware-free stand-in that still lists its own default "Mock
        // Microphone" device, so device-selection/meter-only-capture logic
        // is still exercised end to end.
        // Never a real `LiveEngineFactory`/`UploadFactory`/`ArtifactEnsurer`
        // either (`Unwired*` below): a kittest file that opens the recording
        // window but never clicks Record is unaffected; one that clicks
        // Record without first calling `set_recording_deps_for_test` gets an
        // actionable "not wired for this test" error/panic instead of
        // silently reaching the real network (Global Constraint 3).
        Self::build(
            services,
            None,
            Vec::new(),
            SmokeHooks::default(),
            Box::new(humm_audio::MockCapture::new(Vec::new())),
            Arc::new(UnwiredModelEnsurer) as Arc<dyn ArtifactEnsurer>,
            Arc::new(UnwiredLiveEngineFactory) as Arc<dyn LiveEngineFactory>,
            Arc::new(UnwiredUploadFactory) as Arc<dyn UploadFactory>,
            false,
        )
    }

    /// Test-only seam: replaces the recording window's adapter bundle
    /// (`self.recording_deps`) wholesale — the hook a kittest file uses to
    /// script a full live session (`crate::recording::RecordingDeps`'s
    /// `store`/`model_ensurer`/`engine_factory`/`upload_factory`, plus a
    /// scripted `capture`) without touching real hardware, disk models, or
    /// the network. Must be called before [`Self::open_recording_window_for_test`]
    /// (or the toolbar/Ctrl+R equivalent) — swapping it out from under an
    /// already-open window is unsupported (no open window holds this Arc
    /// across the swap in any test in this workspace).
    pub fn set_recording_deps_for_test(&mut self, deps: RecordingDeps) {
        self.recording_deps = Arc::new(deps);
    }

    /// Shared constructor body: loads the task table's view-model from
    /// `services.store` and restores its persisted layout/sort from
    /// `services.settings.gui` before the first frame ever renders.
    #[allow(clippy::too_many_arguments)]
    fn build(
        mut services: Services,
        event_queue: Option<SharedEventQueue>,
        pending_argv_import: Vec<PathBuf>,
        smoke_hooks: SmokeHooks,
        capture: Box<dyn AudioCapture>,
        model_ensurer: Arc<dyn ArtifactEnsurer>,
        engine_factory: Arc<dyn LiveEngineFactory>,
        upload_factory: Arc<dyn UploadFactory>,
        use_gpu: bool,
    ) -> Self {
        // Taken out of `services` exactly once, here, and `Arc`-wrapped —
        // see `Self::queue`'s field doc for why. Every later ordinary call
        // site uses `Self::queue`, never `services.queue()` again.
        let queue = services.queue.take().map(Arc::new);

        let (gui_settings, stored_available_version, shortcut_overrides) = {
            let settings = services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                settings.gui.clone(),
                settings.updates.available_version.clone(),
                settings.shortcuts.clone(),
            )
        };

        let mut tasks = TasksVm::load(&services.store);
        let (sort_col, sort_descending) = TableLayout::sort_from_gui_settings(&gui_settings);
        tasks.set_sort_exact(sort_col, sort_descending);

        let table_layout = TableLayout::from_gui_settings(&gui_settings);
        let last_saved_gui_settings = table_layout.to_gui_settings(tasks.sort());

        // Seed the resolved map from whatever `[shortcuts]` overrides are
        // already on disk (Task 14: the shortcuts-editor tab writes this
        // same `settings.shortcuts` map at runtime; an empty map — the
        // common case, nothing customized yet — resolves every action to
        // its Buzz default, `ShortcutMap::from_settings`'s own contract).
        let shortcuts = Arc::new(ShortcutMap::from_settings(&shortcut_overrides));
        let shortcuts_generation = Arc::new(AtomicUsize::new(0));

        // The viewer adapter bundle (F-080): all real adapters are stateless
        // handles that resolve their OS resources lazily (cpal at stream
        // open, ffmpeg per subprocess), so constructing them here — including
        // in the kittest path — touches no device or binary until a viewer
        // actually plays/decodes something (which no test does; the viewer
        // kittest drives `viewer_ui` with fakes directly).
        let viewer_deps = Arc::new(ViewerDeps {
            store: Arc::clone(&services.store),
            decoder: Arc::new(FfmpegDecoder::new()),
            prober: Arc::new(FfprobeProber::new()),
            streamer: Arc::new(FfmpegVideoStreamer::new()),
            audio_output: Arc::new(humm_audio::CpalOutput::new()),
            dialogs: Arc::new(RfdFileDialogs),
            // The Translate button's gate/spawn seam (F-085, design spec
            // §7.3): the same `secrets`/`translate` `Services` already
            // threads to the Preferences key field and the file-import
            // pipeline's LLM endpoint (real keyring + real translator worker
            // in `init`, in-memory/scripted doubles in `init_with`).
            secrets: Arc::clone(&services.secrets),
            translate: Arc::clone(&services.translate),
            // The Export menu's `default_export_template` read (F-100,
            // design spec §8.1) — the same shared, live handle every other
            // settings-reading view uses, so a Preferences edit is honored by
            // the very next export.
            settings: Arc::clone(&services.settings),
        });
        // The Speaker-ID window's adapter bundle (F-087): shares `viewer_deps`'s
        // own decoder/audio-output `Arc`s (see `speaker_deps`'s field doc) plus
        // `services.diarize` — real in production, scripted in kittest.
        let speaker_deps = Arc::new(SpeakerIdDeps {
            store: Arc::clone(&services.store),
            decoder: Arc::clone(&viewer_deps.decoder),
            audio_output: Arc::clone(&viewer_deps.audio_output),
            diarize: Arc::clone(&services.diarize),
        });
        // The recording window's adapter bundle (F-060/F-061/F-064, Q-10):
        // `capture`/`model_ensurer`/`engine_factory`/`upload_factory` are
        // real or scripted per the constructor that called `build`
        // (`Self::new`/`Self::new_for_test`) — see `recording_deps`'s own
        // field doc. `store` is always the *real* `services.store` (already
        // a real, often temp-file, `SqliteStore` even under `new_for_test`
        // — see `services::TestDeps`).
        let recording_deps = Arc::new(RecordingDeps {
            capture: Mutex::new(capture),
            store: Arc::clone(&services.store) as Arc<dyn LiveStore>,
            model_ensurer,
            engine_factory,
            upload_factory,
            dialogs: Arc::new(RfdFileDialogs),
            secrets: Arc::clone(&services.secrets),
        });
        let (app_event_tx, app_event_rx) = mpsc::channel();

        // Chip persistence across sessions (binding decision, Buzz-equivalent):
        // a stale-but-still-newer `available_version` from a previous
        // session's check shows the chip immediately, before this session's
        // own check (if any) has even run. Only the version string survives
        // a restart — release notes/download URLs aren't persisted, so the
        // dialog shows an empty notes section until a fresh check backfills
        // them.
        let update_notice = if services::chip_visible_on_startup(
            CURRENT_VERSION,
            stored_available_version.as_deref(),
        ) {
            UpdateNoticeState {
                info: Some(humm_core::update::UpdateInfo {
                    version: stored_available_version.unwrap_or_default(),
                    release_notes: String::new(),
                    download_urls: Vec::new(),
                }),
                dialog_open: false,
            }
        } else {
            UpdateNoticeState::default()
        };

        HummApp {
            services,
            event_queue,
            tasks,
            table_layout,
            last_saved_gui_settings,
            pending_table_actions: Vec::new(),
            shortcuts,
            shortcuts_generation,
            last_shortcuts_generation: 0,
            viewers: BTreeMap::new(),
            resizers: BTreeMap::new(),
            speakers: BTreeMap::new(),
            viewer_deps,
            speaker_deps,
            recording: None,
            recording_deps,
            use_gpu,
            app_event_tx,
            app_event_rx,
            action_state: ActionState {
                update_notice,
                ..ActionState::default()
            },
            dialogs: Box::new(RfdFileDialogs),
            pending_argv_import,
            telemetry_fired: false,
            about_open: false,
            about_state: AboutState::default(),
            preferences_open: false,
            preferences_state: None,
            smoke_hooks,
            smoke_windows_opened: false,
            smoke_frame_count: 0,
            smoke_viewer_opened: false,
            smoke_viewer_id: None,
            smoke_live_fixture_state: None,
            queue,
            folder_watch: None,
            folder_watch_snapshot: None,
            folder_watch_seen: Arc::new(Mutex::new(SeenSet::new())),
        }
    }

    /// Writes `table_layout`/`tasks.sort()` to `services.settings.gui` and
    /// persists it (`Settings::save`) if it differs from what was last
    /// saved. A save failure is logged, not fatal — the in-memory layout the
    /// user sees is unaffected either way, only next launch's restore would
    /// miss this particular change.
    fn persist_layout_if_changed(&mut self) {
        let current = self.table_layout.to_gui_settings(self.tasks.sort());
        if current == self.last_saved_gui_settings {
            return;
        }
        {
            let mut settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settings.gui = current.clone();
            if let Err(err) = settings.save(&self.services.settings_path) {
                log::warn!("could not persist task table layout: {err}");
            }
        }
        self.last_saved_gui_settings = current;
    }

    /// Test-only seam: injects `event` directly into the event-pump buffer,
    /// as if the real queue's runner thread had emitted it. Lets a kittest
    /// harness exercise event-driven UI reactions (the `StoreWriteFailed`
    /// banner, in particular) deterministically, without racing the real
    /// queue/runner threads for a precise failure-injection point — the
    /// same "plain `pub fn`, not `#[cfg(test)]`" pattern as
    /// [`Self::new_for_test`] (an integration test in `tests/` links the
    /// library's normal, non-`cfg(test)` build, so a `#[cfg(test)]` method
    /// wouldn't even be visible to it). A no-op before the event pump
    /// exists (i.e. before the first `eframe::App::ui` call) — harmless,
    /// but there is nothing to inject into yet.
    pub fn inject_event_for_test(&mut self, event: TaskEvent) {
        if let Some(queue) = &self.event_queue {
            queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push_back(event);
        }
    }

    /// Test-only seam: opens the options sheet directly with `request`, as
    /// if the corresponding dialog/drag-drop/argv flow had already produced
    /// it. Lets a kittest harness exercise the sheet (and Run→enqueue→
    /// sticky-prefs flow) without ever driving a real, blocking rfd dialog —
    /// same "plain `pub fn`, not `#[cfg(test)]`" pattern as
    /// [`Self::inject_event_for_test`].
    pub fn open_options_sheet_for_test(&mut self, request: ImportRequest) {
        self.open_options_sheet(request);
    }

    /// Test-only seam: mutably borrows the open options sheet's live
    /// [`SheetState`], if one is open — lets a kittest harness force a
    /// backend-combo selection directly (F-041, Task 14). Driving an
    /// `egui::ComboBox` selection via clicks has no kittest precedent in
    /// this workspace (`kittest_recording.rs`'s own `mode_index` seam
    /// documents the same gap for the recording window's advanced sheet);
    /// this mirrors that technique for the options sheet.
    pub fn options_sheet_state_mut_for_test(&mut self) -> Option<&mut SheetState> {
        self.action_state
            .options_sheet
            .as_mut()
            .map(|sheet| &mut sheet.state)
    }

    /// Whether `settings.models.custom_model_url` is set — gates the
    /// options sheet's model-size combo's "Custom" entry (F-023).
    fn custom_model_available(&self) -> bool {
        self.custom_model_url().is_some()
    }

    /// Whether Buzz's Lumii (Latvian whisper.cpp) model size is currently
    /// selectable — gates every model-size combo's "Lumii" entry (options
    /// sheet, recording window, and the Preferences → Models tab; F-044/J-9,
    /// design spec §8.2). Delegates the actual comparison to
    /// [`crate::i18n::lumii_locale_requested`]; this method's only job is
    /// gathering its two raw inputs (`settings.ui.locale`, and the
    /// `LC_ALL`/`LANG` system-locale fallback) exactly the way `Self::new`'s
    /// own startup locale negotiation does.
    fn lumii_gate(&self) -> bool {
        let system_locale = i18n::locale_from_env_vars(
            std::env::var("LC_ALL").ok().as_deref(),
            std::env::var("LANG").ok().as_deref(),
        );
        let settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        i18n::lumii_locale_requested(&settings, system_locale.as_deref())
    }

    /// The current `settings.models.custom_model_url` value — the Models
    /// tab's own custom-URL row gate (F-044) needs the value itself, not
    /// just [`Self::custom_model_available`]'s boolean.
    fn custom_model_url(&self) -> Option<String> {
        let settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.models.custom_model_url.clone()
    }

    /// A full clone of the live settings, snapshotted at the moment the
    /// Preferences window opens (`PreferencesState::new`'s own contract:
    /// every tab's fields are seeded once, at open time, not live-read
    /// per frame). Renamed from the M6-era `openai_settings_snapshot` (which
    /// returned only `settings.openai`) once the General tab grew rows
    /// seeded from `settings.ui`/`settings.file_transcriber`/
    /// `settings.models` too (F-100, design spec §8.1).
    fn settings_snapshot(&self) -> Settings {
        let settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.clone()
    }

    /// `settings.models.force_cpu`, read fresh (design spec §8.1: "Disable
    /// GPU" persists live, no restart) — the setting-half of the live-
    /// recording session's own `use_gpu` resolution
    /// ([`Self::start_live_session`]'s `LiveSessionRequest.use_gpu`), which
    /// combines this with `self.use_gpu`'s startup-cached, env-only half
    /// (`HUMM_FORCE_CPU` doesn't change mid-process, so caching it is safe;
    /// the setting can change any time Preferences is open, so it isn't).
    fn force_cpu_setting(&self) -> bool {
        let settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.models.force_cpu
    }

    /// The running queue handle, for every ordinary (non-shutdown) caller —
    /// the `Arc`-wrapped replacement for `Services::queue()` (see
    /// `Self::queue` field's own doc for why). Panics if called after the
    /// queue was taken for shutdown, mirroring `Services::queue`'s own
    /// contract exactly.
    fn queue(&self) -> &QueueHandle {
        self.queue
            .as_deref()
            .expect("HummApp::queue called after the queue was taken for shutdown")
    }

    /// Re-derives the `[folder_watch]` settings snapshot and starts/
    /// restarts/updates-in-place/stops [`Self::folder_watch`] to match
    /// (F-030, design spec §3.2). Run every frame (`eframe::App::ui`):
    /// building [`FolderWatchSnapshot`] is a small, bounded clone of
    /// `settings.folder_watch` (a few `String`/`Option<String>`/`Vec<String>`
    /// fields) taken under the settings lock — not literally free, but cheap
    /// enough to redo every frame — so this covers both "startup" (the very
    /// first frame, `self.folder_watch` starts `None`) and "a Preferences
    /// edit landed since last frame" with the exact same code path — no
    /// separate one-shot "apply on window close" hook to keep in sync, per
    /// humm's established write-through-on-commit pattern (ledger #L8 in the
    /// design spec).
    ///
    /// `self.folder_watch_snapshot` caches the *last snapshot this function
    /// actually reacted to* — regardless of whether that reaction was
    /// starting, restarting, updating the template in place, stopping, or
    /// even doing nothing because settings were already inactive. This
    /// serves two purposes: a `start()` failure (e.g. a configured directory
    /// doesn't exist) doesn't spin — the next frame sees an unchanged
    /// snapshot and takes `ReconcileAction::NoChange`, only retrying once the
    /// user actually changes something — and an "enabled but not fully
    /// configured" misconfiguration is logged exactly once per distinct
    /// snapshot, not every frame.
    fn sync_folder_watch(&mut self, ctx: &egui::Context) {
        let snapshot = {
            let settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // The *global* `file_transcriber.default_export_template`
            // (Preferences → General) threads in here, not `folder_watch.
            // options.default_export_template` — review fix (M7 Task 11
            // follow-up), see `FolderWatchSnapshot::default_export_template`'s
            // own doc for why.
            FolderWatchSnapshot::from_settings(
                &settings.folder_watch,
                settings.file_transcriber.default_export_template.clone(),
            )
        };
        let changed = self.folder_watch_snapshot.as_ref() != Some(&snapshot);

        let action = crate::folder_watch::reconcile_folder_watch(
            &snapshot,
            self.folder_watch_snapshot.as_ref(),
            self.folder_watch.is_some(),
        );

        match action {
            ReconcileAction::NoChange => {}
            ReconcileAction::Stop => {
                if let Some(service) = self.folder_watch.take() {
                    service.stop();
                }
            }
            ReconcileAction::UpdateTemplate => {
                // The watcher itself (identity: `input_dir`) is untouched —
                // only the job template (`output_dir`/`delete_processed_files`/
                // `options`/`default_export_template`) changed, so the running
                // worker just picks up the new template on its next rescan; no
                // watcher teardown/
                // rebuild, no lost `SeenSet` progress, no churn (this is
                // exactly what keeps a future per-keystroke options-form edit
                // — Task 15 — from restarting the FSEvents/PollWatcher
                // subscription on every character).
                if let Some(service) = &self.folder_watch {
                    service.update_template(&snapshot);
                }
            }
            ReconcileAction::Start | ReconcileAction::Restart => {
                if let Some(service) = self.folder_watch.take() {
                    service.stop();
                }
                if let Some(queue) = self.queue.as_ref().map(Arc::clone) {
                    match FolderWatchService::start(
                        snapshot.clone(),
                        queue,
                        Arc::clone(&self.services.store),
                        ctx.clone(),
                        Arc::clone(&self.folder_watch_seen),
                    ) {
                        Ok(service) => self.folder_watch = Some(service),
                        Err(err) => {
                            log::warn!("could not start the folder-watch service: {err}");
                            // `snapshot` is still cached below (`if changed`)
                            // even though `start()` failed — a retry only
                            // happens once the `[folder_watch]` snapshot
                            // itself changes again (e.g. the user picks a
                            // different directory in Preferences); simply
                            // repairing whatever made `start()` fail on disk
                            // (recreating a deleted `input_dir`, fixing a
                            // permission) does *not* resurrect the watcher
                            // this session on its own. This matches Buzz:
                            // its own recovery path is also only
                            // `set_preferences` (a fresh Preferences save),
                            // never a live filesystem re-check.
                        }
                    }
                }
                // `self.queue` is only ever `None` once shutdown has already
                // taken it (`on_exit` stops `folder_watch` first, so this
                // frame should never actually run again afterward) —
                // defensive no-op otherwise.
            }
        }

        if changed {
            if snapshot.enabled && !snapshot.is_active() {
                log::info!(
                    "folder watch is enabled but not fully configured (input and/or output \
                     directory not set) — nothing will be watched until both are set"
                );
            }
            self.folder_watch_snapshot = Some(snapshot);
        }
    }

    /// The current folder-watch toolbar pill state (design spec §3.3), or
    /// `None` to hide it entirely — `Some` iff a service is actually
    /// running, which already implies `folder_watch_snapshot.is_active()`
    /// (`Self::sync_folder_watch` only ever sets both together).
    fn folder_watch_pill(&self) -> Option<FolderWatchPillState> {
        let service = self.folder_watch.as_ref()?;
        let folder_name = self
            .folder_watch_snapshot
            .as_ref()
            .and_then(FolderWatchSnapshot::pill_folder_name)?;
        Some(FolderWatchPillState {
            folder_name,
            paused: service.is_paused(),
        })
    }

    /// The folder-watch pill's pause/resume button was clicked this frame
    /// (design spec §3.3): flips the running service's pause flag. A no-op
    /// if the service isn't running (e.g. a click landed the same frame
    /// `sync_folder_watch` stopped it — harmless either way, the pill itself
    /// wouldn't have rendered a toggle button with nothing to toggle).
    fn toggle_folder_watch_pause(&mut self) {
        if let Some(service) = &self.folder_watch {
            service.pause(!service.is_paused());
        }
    }

    /// Opens the options sheet for `request`, prefilled from the current
    /// `settings.file_transcriber`/`settings.models` snapshot.
    fn open_options_sheet(&mut self, request: ImportRequest) {
        let prefill: FileTranscriberSettings = {
            let settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settings.file_transcriber.clone()
        };
        let custom_model_available = self.custom_model_available();
        self.action_state.options_sheet = Some(OptionsSheet::new(
            request,
            &prefill,
            custom_model_available,
            self.services.secrets.as_ref(),
        ));
    }

    /// Applies the options sheet's close outcome (`views::main_window::show`
    /// already rendered it this frame): persists the sticky prefs
    /// unconditionally (Buzz's `closeEvent` parity — Run *and* Cancel/
    /// Escape/backdrop-click all save), then, for `Run`, enqueues every
    /// spec with a fresh id.
    fn apply_sheet_outcome(&mut self, outcome: SheetOutcome) {
        match outcome {
            SheetOutcome::Run { specs, sticky } => {
                self.persist_sticky_prefs(&sticky);
                for (id, spec, source) in specs {
                    self.queue().enqueue(id, spec, source);
                }
            }
            SheetOutcome::Cancelled { sticky } => self.persist_sticky_prefs(&sticky),
        }
    }

    /// Writes `sticky` into `settings.file_transcriber` and persists it. A
    /// save failure is logged, not fatal (same non-fatal-settings-write
    /// contract as [`Self::persist_layout_if_changed`]/
    /// [`Self::persist_last_import_folder`]).
    fn persist_sticky_prefs(&mut self, sticky: &SheetState) {
        let mut settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sticky.write_to(&mut settings.file_transcriber);
        if let Err(err) = settings.save(&self.services.settings_path) {
            log::warn!("could not persist file-transcriber options: {err}");
        }
    }

    /// Persists `folder` as `settings.file_transcriber.last_import_folder`
    /// (Buzz parity: persisted at dialog time, independent of the sheet's
    /// own sticky write-back). A no-op if `folder` is `None` (the file
    /// dialog's pick had no parent directory — practically unreachable, but
    /// harmless either way).
    fn persist_last_import_folder(&mut self, folder: Option<&Path>) {
        let Some(folder) = folder else {
            return;
        };
        let mut settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.file_transcriber.last_import_folder = Some(folder.to_path_buf());
        if let Err(err) = settings.save(&self.services.settings_path) {
            log::warn!("could not persist the last import folder: {err}");
        }
    }

    /// The last-used import folder, if any, to seed the file/folder dialog's
    /// starting directory.
    fn last_import_folder(&self) -> Option<PathBuf> {
        let settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        settings.file_transcriber.last_import_folder.clone()
    }

    /// Drains `action_state.pending_import` (set by the toolbar/File-menu/
    /// shortcuts this frame's `main_window::show` call) and dispatches the
    /// corresponding import flow. Always resets it to `None` first — a
    /// canceled dialog must not re-trigger itself on the very next frame.
    /// Guarded by `!modal_open()` (symmetry with
    /// [`Self::drain_dropped_files`]'s own guard): a pending import queued
    /// the same frame some other modal is already up (e.g. Clear-confirm)
    /// must not silently clobber it with a freshly-opened dialog/sheet.
    fn drain_pending_import(&mut self) {
        let request = std::mem::take(&mut self.action_state.pending_import);
        if self.action_state.modal_open() {
            return;
        }
        match request {
            PendingImportRequest::None => {}
            PendingImportRequest::File => self.handle_file_import_request(),
            PendingImportRequest::Url => {
                self.action_state.url_dialog = UrlDialog::open();
            }
            PendingImportRequest::Folder => self.handle_folder_import_request(),
        }
    }

    /// "New File Transcription" / File → "Import File…" / ⌘O
    /// (`main_window.py:268-281`): opens the (blocking) file dialog seeded
    /// with the last import folder, persists the parent of the first pick,
    /// and opens the options sheet with every picked file. A cancel (`None`)
    /// or an empty pick is a silent no-op.
    fn handle_file_import_request(&mut self) {
        let start_dir = self.last_import_folder();
        let Some(paths) = self.dialogs.pick_files(start_dir.as_deref()) else {
            return;
        };
        if paths.is_empty() {
            return;
        }
        self.persist_last_import_folder(paths[0].parent());
        self.open_options_sheet(ImportRequest::Files(paths));
    }

    /// File → "Import Folder…" (`main_window.py:288-304`): opens the
    /// (blocking) folder dialog seeded with the last import folder,
    /// persists the chosen folder itself, recursively scans it
    /// (`import::scan_folder`), and opens the options sheet with whatever
    /// matched. Zero matches is a silent no-op (Buzz parity) — same for a
    /// cancel (`None`).
    fn handle_folder_import_request(&mut self) {
        let start_dir = self.last_import_folder();
        let Some(folder) = self.dialogs.pick_folder(start_dir.as_deref()) else {
            return;
        };
        self.persist_last_import_folder(Some(&folder));
        let files = scan_folder(&folder);
        if files.is_empty() {
            return;
        }
        self.open_options_sheet(ImportRequest::Files(files));
    }

    /// Drag-and-drop (F-021, `main_window.py:196-205`): reads
    /// `egui::RawInput::dropped_files` (no extension filtering — Buzz
    /// parity) and, if any files were dropped this frame, opens the options
    /// sheet with them. Guarded by `!modal_open()` so a drop landing while
    /// some other modal (e.g. Clear-confirm) is already up doesn't silently
    /// clobber it with a freshly-opened sheet — a humm-only defensive
    /// addition (Buzz's own top-level-window drop target has no equivalent
    /// modal to protect).
    fn drain_dropped_files(&mut self, ctx: &egui::Context) {
        if self.action_state.modal_open() {
            return;
        }
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        self.open_options_sheet(ImportRequest::Files(dropped));
    }

    /// Consumes `pending_argv_import` (Finder "open with" parity) on
    /// whichever frame first calls this — always the very first
    /// `eframe::App::ui` call, since it's cleared unconditionally the first
    /// time regardless of whether it was empty. An empty `Vec` (the common
    /// case: no argv files) never opens a sheet.
    fn drain_pending_argv_import(&mut self) {
        let paths = std::mem::take(&mut self.pending_argv_import);
        if paths.is_empty() {
            return;
        }
        self.open_options_sheet(ImportRequest::Files(paths));
    }

    /// Reacts to one drained event with a toast/banner side effect, keyed on
    /// `event.kind`. `TasksVm::apply` (already called for every event, just
    /// before this) owns row state; this owns only the toast stack and the
    /// store-health banner, neither of which lives on any row.
    ///
    /// Chosen over teaching `TasksVm::apply` to return a toast-driving
    /// value (the brief's other offered seam): every payload this method
    /// needs (`task_id`, `written`/`message`) is already fully available on
    /// `event` right here, in the same loop that calls `apply` — threading
    /// the same data back out through `apply`'s return value would only
    /// duplicate it for no benefit, and keeps `TasksVm`'s own tests free of
    /// toast/banner concerns that have nothing to do with row state.
    fn react_to_event(&mut self, event: &TaskEvent, now: Instant) {
        match &event.kind {
            TaskEventKind::Completed { written } => {
                let message = if written.is_empty() {
                    tr("toast-completed")
                } else {
                    format!(
                        "{} \u{2014} {} {}",
                        tr("toast-completed"),
                        written.len(),
                        tr("toast-completed-suffix")
                    )
                };
                self.action_state
                    .toasts
                    .push(ToastKind::Success, message, now);
            }
            TaskEventKind::Failed { message } => {
                let first_line = message.lines().next().unwrap_or(message).to_string();
                self.action_state.toasts.push(
                    ToastKind::TaskFailed {
                        task_id: event.task_id,
                        full_message: message.clone(),
                    },
                    first_line,
                    now,
                );
            }
            TaskEventKind::Canceled => {
                self.action_state
                    .toasts
                    .push(ToastKind::Info, tr("toast-canceled"), now);
            }
            TaskEventKind::StoreWriteFailed { .. } => {
                self.action_state.banner.on_store_write_failed();
            }
            // `TasksVm::apply` (just before this call) already refreshed the
            // row from the store, covering the notes column; this covers
            // the one thing a row refresh can't: an *open viewer* still
            // showing the pre-resize segments in memory (design spec §2.3).
            TaskEventKind::PostProcessed { .. }
                if postprocessed_should_reload_viewer(&event.kind) =>
            {
                self.reload_viewer_segments_if_open(event.task_id);
            }
            _ => {}
        }
    }

    /// Reloads `id`'s open viewer's segments from the store, if one is
    /// open — a no-op otherwise (most completed tasks have no viewer open
    /// at all). Driven by [`TaskEventKind::PostProcessed`]'s
    /// `segments_replaced` flag via [`postprocessed_should_reload_viewer`]:
    /// the auto-resizer hook can replace a task's persisted segments after
    /// transcription completes, and an already-open viewer would otherwise
    /// keep showing the pre-resize rows until closed and reopened.
    fn reload_viewer_segments_if_open(&mut self, id: TranscriptionId) {
        if let Some(vm_arc) = self.viewers.get(&id) {
            let mut vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.reload_segments(&self.services.store);
        }
    }

    /// Applies a confirmed Rename/Notes dialog outcome: writes the store,
    /// then refreshes the row so the table reflects it immediately. A store
    /// write failure is logged and surfaced as an error toast (never a
    /// panic, never silently dropped) — the dialog has already closed by
    /// this point either way.
    fn apply_dialog_outcome(&mut self, outcome: DialogOutcome, now: Instant) {
        let store = Arc::clone(&self.services.store);
        match outcome {
            DialogOutcome::Rename { id, name } => match store.set_task_name(&id, &name) {
                Ok(()) => self.tasks.refresh_row(id, &store),
                Err(err) => {
                    log::warn!("set_task_name failed for {id}: {err}");
                    self.push_error_toast(format!("{}: {err}", tr("rename-error")), now);
                }
            },
            DialogOutcome::Notes { id, text } => match store.set_task_notes(&id, &text) {
                Ok(()) => self.tasks.refresh_row(id, &store),
                Err(err) => {
                    log::warn!("set_task_notes failed for {id}: {err}");
                    self.push_error_toast(format!("{}: {err}", tr("notes-error")), now);
                }
            },
        }
    }

    /// Applies the Clear-History confirm modal's Ok click: deletes every
    /// currently-selected row from the store, then drops them from the
    /// view-model (which also clears their selection). A store failure is
    /// logged and surfaced as an error toast; the view-model is left
    /// unchanged in that case (the rows are still there — because they're
    /// still in the store).
    ///
    /// Re-checks the terminal-status gate here too (the same
    /// `toolbar_enablement(..).clear` rule `handle_clear_selected` already
    /// checked before ever opening the confirm modal) — defense in depth:
    /// the modal is open for as long as the user takes to click Ok/Cancel,
    /// and task events keep flowing into `TasksVm` regardless of whether a
    /// modal is up, so a selected row could in principle transition to a
    /// non-terminal status in that window. Failing the gate silently
    /// aborts the delete (nothing selected is safe to delete anymore) —
    /// no toast, matching `handle_clear_selected`'s own silent-ignore
    /// contract for a gate that fails.
    fn apply_clear_confirmed(&mut self, now: Instant) {
        if !toolbar_enablement(&self.tasks.selected_statuses()).clear {
            return;
        }
        let ids: Vec<TranscriptionId> = self.tasks.selection().iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        match self.services.store.delete_tasks(&ids) {
            Ok(_) => self.tasks.remove(&ids),
            Err(err) => {
                log::warn!("delete_tasks failed: {err}");
                self.push_error_toast(format!("{}: {err}", tr("clear-error")), now);
            }
        }
    }

    /// Applies a toast/detail-popover action.
    fn apply_toast_action(&mut self, action: ToastAction, now: Instant) {
        match action {
            ToastAction::Retry(id) => self.handle_retry(id, now),
            ToastAction::OpenLogFolder => {
                if let Err(err) = open::that(humm_core::paths::log_dir()) {
                    log::warn!("could not open the log folder: {err}");
                }
            }
        }
    }

    /// Drains this frame's `pending_table_actions` (Task 7's accumulator —
    /// the table's context menu/keyboard shortcuts and the toolbar/global
    /// shortcuts all push onto the same `Vec`), executing each.
    fn drain_table_actions(&mut self, now: Instant) {
        let actions = std::mem::take(&mut self.pending_table_actions);
        for action in actions {
            match action {
                TableAction::ClearSelected => self.handle_clear_selected(),
                TableAction::OpenSelected => self.handle_open_selected(now),
                TableAction::Retry(id) => self.handle_retry(id, now),
                TableAction::Rename(id) => self.open_rename_dialog(id),
                TableAction::Notes(id) => self.open_notes_dialog(id),
                TableAction::CancelSelected => self.handle_cancel_selected(),
            }
        }
    }

    /// Toolbar "Cancel Transcription" / ⌘X: `queue.cancel(id)` for every
    /// selected row whose status is queued/in-progress — filtered here
    /// (rather than gated all-or-nothing) so a selection the toolbar would
    /// have disabled entirely still cancels whichever of its rows *are*
    /// cancelable if reached via a path that doesn't re-check the toolbar's
    /// own enablement (defense in depth). Never writes the store directly:
    /// the queue owns status, and a canceled row flips via the ordinary
    /// `Canceled` event.
    fn handle_cancel_selected(&mut self) {
        let ids: Vec<TranscriptionId> = self
            .tasks
            .sorted_rows()
            .iter()
            .filter(|row| self.tasks.selection().contains(&row.id))
            .filter(|row| matches!(row.status, Status::InProgress | Status::Queued))
            .map(|row| row.id)
            .collect();
        for id in ids {
            self.queue().cancel(id);
        }
    }

    /// Toolbar "Clear History" / ⌘S / Delete key: gates to a non-empty,
    /// all-terminal-status selection (the same rule
    /// [`toolbar_enablement`] uses to gray out the toolbar button) and, if
    /// it passes, opens the confirm modal. A selection that fails the gate
    /// is silently ignored on every path, including the Delete key —
    /// deviation §6.2: Buzz's own Delete-key handler bypasses this gate
    /// (a defect, not ported).
    fn handle_clear_selected(&mut self) {
        if toolbar_enablement(&self.tasks.selected_statuses()).clear {
            self.action_state.clear_confirm_open = true;
        }
    }

    /// Retry: reachable from the row context menu's "Restart Transcription"
    /// and a failed-transcription toast's detail-popover Retry button.
    /// Re-checks the row's current status before doing anything (defense in
    /// depth — the queue itself has no terminal-status guard, and the
    /// row's status can change between the click that offered Retry and
    /// this handler actually running).
    ///
    /// Spec source: `load_task_spec` (the full persisted options); on
    /// `Ok(None)` (an imported/foreign row with no `options_json`), falls
    /// back to `spec_from_columns`. Any error from `load_task_spec` itself,
    /// or an `Ok(None)`/error from the fallback, surfaces the toast
    /// "Could not restart: saved options are unavailable" — the row is left
    /// exactly as it was (still failed/canceled/skipped, retryable again
    /// later).
    fn handle_retry(&mut self, id: TranscriptionId, now: Instant) {
        let Some(row) = self.tasks.sorted_rows().iter().find(|row| row.id == id) else {
            return;
        };
        if !retry_allowed(row.status) {
            return;
        }

        let store = Arc::clone(&self.services.store);
        let spec = match store.load_task_spec(&id) {
            Ok(Some(spec)) => Ok(Some(spec)),
            Ok(None) => store.spec_from_columns(&id),
            Err(err) => Err(err),
        };
        match spec {
            Ok(Some(spec)) => self.queue().retry(id, spec),
            Ok(None) | Err(_) => {
                self.push_error_toast(tr("retry-error-unavailable"), now);
            }
        }
    }

    /// Row context menu "Rename": prefills from the row's *current* store
    /// columns (not the view-model's `display_name`, which uses a
    /// different fallback order — see [`rename_notes::rename_prefill`]'s
    /// doc comment) and opens the modal. A store-read failure or a
    /// since-deleted row silently does nothing (no dialog opens).
    fn open_rename_dialog(&mut self, id: TranscriptionId) {
        let Ok(Some(row)) = self.services.store.get_task(&id) else {
            return;
        };
        let text = rename_notes::rename_prefill(
            row.name.as_deref(),
            row.url.as_deref(),
            row.file.as_deref(),
        );
        self.action_state.dialog = rename_notes::Dialog::Rename { id, text };
    }

    /// Row context menu "Add/Edit Notes" (or a Notes-cell double-click):
    /// prefills from the view-model's current notes text and opens the
    /// modal. A since-removed row silently does nothing.
    fn open_notes_dialog(&mut self, id: TranscriptionId) {
        let Some(row) = self.tasks.sorted_rows().iter().find(|row| row.id == id) else {
            return;
        };
        let text = row.notes.clone().unwrap_or_default();
        self.action_state.dialog = rename_notes::Dialog::Notes { id, text };
    }

    /// Pushes a plain (non-`TaskFailed`, non-clickable) error toast.
    fn push_error_toast(&mut self, message: String, now: Instant) {
        self.action_state
            .toasts
            .push(ToastKind::Error, message, now);
    }

    /// Fires the one-per-launch telemetry event (F-007, ADR-009) on the
    /// first call only — every later call is a no-op. See
    /// [`services::fire_launch_telemetry`]'s doc comment for why this lives
    /// on the first UI frame rather than in `services::init`.
    fn fire_telemetry_once(&mut self) {
        if self.telemetry_fired {
            return;
        }
        self.telemetry_fired = true;
        services::fire_launch_telemetry(&self.services);
    }

    /// Applies `smoke_hooks.open_windows`'s effect on the first
    /// `eframe::App::ui` call only — every later call is a no-op, the same
    /// once-guard shape as [`Self::fire_telemetry_once`]. Sets exactly the
    /// same `action_state.about_requested`/`preferences_requested` flags the
    /// Help-menu clicks / ⌘, shortcut set
    /// (`views::main_window::show_menu_bar`/`handle_shortcuts`) — there is
    /// no parallel window-opening code path here; the very same drain in
    /// [`eframe::App::ui`] that already turns those flags into
    /// `about_open`/`preferences_open` for the real menu path does the rest.
    fn apply_smoke_open_windows_once(&mut self) {
        if self.smoke_windows_opened {
            return;
        }
        self.smoke_windows_opened = true;
        if self.smoke_hooks.open_windows {
            self.action_state.about_requested = true;
            self.action_state.preferences_requested = true;
        }
    }

    /// Counts this frame against `smoke_hooks.auto_close_frames` and, once
    /// the count reaches it, requests a normal shutdown
    /// (`egui::ViewportCommand::Close`) — the same close path a user
    /// clicking the window's close button takes, so `eframe`'s own
    /// persistence (`gui.ron`) still runs. A no-op whenever the hook is
    /// unset (the default, non-smoke case).
    ///
    /// While active and short of the target, also calls
    /// `ctx.request_repaint()`: egui only calls `eframe::App::ui` again on
    /// its own when there's real work to do (input, an animation, an
    /// explicit repaint request), so an otherwise-idle app would never
    /// advance `smoke_frame_count` and the hook would stall forever short of
    /// its target. This deliberately defeats humm's normal zero-idle-CPU
    /// behavior — but only for the duration this hook is itself active,
    /// i.e. only when `HUMM_SMOKE_AUTO_CLOSE_FRAMES` is set.
    fn apply_smoke_auto_close(&mut self, ctx: &egui::Context) {
        let Some(target_frames) = self.smoke_hooks.auto_close_frames else {
            return;
        };
        self.smoke_frame_count += 1;
        if self.smoke_frame_count >= target_frames {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }

    /// Applies `smoke_hooks.open_viewer`'s effect on the first `eframe::App::ui`
    /// call only (the once-guard shape of [`Self::apply_smoke_open_windows_once`]):
    /// seeds a **completed** transcript row for the configured media path into
    /// the real store (`create_task` + `mark_completed`, a handful of
    /// placeholder segments so the Timestamps table + video pane render — the
    /// transcript text is irrelevant to an A/V-sync measurement), opens its
    /// viewer through the exact same [`Self::open_viewer`] path a table
    /// double-click takes, and arms [`ViewerVm::smoke_autoplay`] so the viewer
    /// self-drives playback + auto-close (the deferred viewport owns its own
    /// repaint cadence). Test-only; a no-op whenever the hook is unset.
    fn apply_smoke_open_viewer_once(&mut self, now: Instant) {
        if self.smoke_viewer_opened {
            return;
        }
        self.smoke_viewer_opened = true;
        let Some(path) = self.smoke_hooks.open_viewer.clone() else {
            return;
        };
        let id = TranscriptionId::new();
        let spec = smoke_viewer_spec(&path);
        let seeded = self
            .services
            .store
            .create_task(&id, &spec, Source::FileImport)
            .and_then(|()| {
                self.services
                    .store
                    .mark_completed(&id, &smoke_viewer_segments())
            });
        if let Err(err) = seeded {
            log::warn!("HUMM_SMOKE_OPEN_VIEWER: could not seed transcript for {path:?}: {err}");
            return;
        }
        self.open_viewer(id, now);
        if let Some(vm_arc) = self.viewers.get(&id) {
            vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .smoke_autoplay = true;
            self.smoke_viewer_id = Some(id);
            log::info!("HUMM_SMOKE_OPEN_VIEWER: opened autoplay viewer {id} for {path:?}");
        }
    }

    /// Keeps the root frame advancing while the `HUMM_SMOKE_OPEN_VIEWER` viewer
    /// is open (deferred viewports repaint on their own schedule, so the root
    /// would otherwise go idle and never harvest the child's end-of-media
    /// `close_requested`), and — once that window has auto-closed — requests a
    /// normal app shutdown so the process exits cleanly with the drift log
    /// flushed (`ViewerVm`'s `Drop`). A no-op whenever the hook is unset.
    fn apply_smoke_viewer_tick(&mut self, ctx: &egui::Context) {
        let Some(id) = self.smoke_viewer_id else {
            return;
        };
        if self.viewers.contains_key(&id) {
            ctx.request_repaint();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Drives the `HUMM_SMOKE_LIVE_FIXTURE` acceptance hook (spec §8 item 1,
    /// M5 Task 14): a no-op whenever `smoke_hooks.live_fixture` is `None`
    /// (unset, or a decode failure already cleared it in [`Self::new`]).
    /// Otherwise, a small state machine — mirrors
    /// [`Self::apply_smoke_open_viewer_once`]'s one-shot-then-poll shape,
    /// but this hook has three phases instead of two because a live
    /// recording session, unlike opening a viewer, needs an explicit
    /// Stop before it can close:
    ///
    /// 1. **First call**: opens the recording window exactly like the
    ///    toolbar Record button / Ctrl+R would
    ///    ([`Self::open_recording_window`]), forces the model size to
    ///    [`ModelSize::Small`] (the acceptance run's own requirement,
    ///    spec §8 item 1 — independent of whatever `settings.recording`
    ///    happens to have persisted), then calls
    ///    [`crate::recording::RecordingVm::on_record_clicked`] — the exact
    ///    function the real button's click handler calls — which latches
    ///    `StartRequested`; [`Self::harvest_recording_intent`] (already
    ///    called every root frame) spawns the real session moments later.
    /// 2. **While recording**: once `smoke_hooks.live_fixture_seconds` has
    ///    elapsed since the click, calls `on_record_clicked` a second time
    ///    (Recording → Stopping, the same function a Stop click uses) —
    ///    exactly one stop request is ever latched
    ///    (`SmokeLiveFixtureState::StopRequested` guards it).
    /// 3. **Waiting for Stopped**: once the real session's `Stopped`
    ///    message has flowed through [`Self::finish_live_session`] and
    ///    restored [`crate::recording::RecordingStatus::Stopped`], sets
    ///    `close_requested` directly (the same field a real window-close
    ///    while stopped would set) so [`Self::harvest_recording_close`]
    ///    (already called every root frame) persists sticky prefs and drops
    ///    the window on its own next pass; once `self.recording` is gone,
    ///    requests a normal root-viewport close so the process exits
    ///    cleanly, matching [`Self::apply_smoke_viewer_tick`]'s own
    ///    "wait for the child to actually close, then close the root"
    ///    shape.
    ///
    /// `ctx.request_repaint()` is called on every branch short of the final
    /// close (same rationale as [`Self::apply_smoke_auto_close`]'s own doc
    /// comment: this deliberately defeats zero-idle-CPU, but only while this
    /// hook is active).
    fn apply_smoke_live_fixture_tick(&mut self, ctx: &egui::Context) {
        let Some(seconds) = (self.smoke_hooks.live_fixture.is_some())
            .then_some(self.smoke_hooks.live_fixture_seconds)
        else {
            return;
        };

        let Some(state) = self.smoke_live_fixture_state else {
            self.open_recording_window();
            let Some(vm_arc) = self.recording.clone() else {
                log::warn!("HUMM_SMOKE_LIVE_FIXTURE: recording window failed to open");
                return;
            };
            {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                vm.options.model_size = ModelSize::Small;
                vm.on_record_clicked(&self.recording_deps);
                // Also opens the presentation window as a real OS window for
                // the run's duration (F-065 windowed evidence, spec §8 item
                // 1's neighboring acceptance concern) — equivalent to a
                // "Show" click on the presentation bar, which only renders
                // while recording (`presentation_bar_visible`, just set true
                // by `on_record_clicked` above).
                vm.presentation.open = true;
            }
            log::info!("HUMM_SMOKE_LIVE_FIXTURE: Record clicked (small model), {seconds}s window");
            self.smoke_live_fixture_state = Some(SmokeLiveFixtureState {
                started: Instant::now(),
                stop_requested: false,
            });
            ctx.request_repaint();
            return;
        };

        let Some(vm_arc) = self.recording.clone() else {
            // The window has already closed (the third phase below just
            // finished it) — request the app shutdown that phase deferred
            // to this next tick.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };

        if !state.stop_requested {
            if state.started.elapsed() >= Duration::from_secs(u64::from(seconds)) {
                log::info!("HUMM_SMOKE_LIVE_FIXTURE: {seconds}s window elapsed, requesting stop");
                vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .on_record_clicked(&self.recording_deps);
                self.smoke_live_fixture_state = Some(SmokeLiveFixtureState {
                    stop_requested: true,
                    ..state
                });
            }
            ctx.request_repaint();
            return;
        }

        let stopped = vm_arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .status
            == RecordingStatus::Stopped;
        if stopped {
            log::info!("HUMM_SMOKE_LIVE_FIXTURE: session stopped, closing recording window");
            vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .close_requested = true;
        }
        ctx.request_repaint();
    }

    /// Drains `services.update_rx` and applies every [`UpdateNotice`] this
    /// frame: `Available` always updates the toolbar chip (whichever check
    /// — startup or the About window's manual one — found it); `UpToDate`
    /// always clears the chip (binding decision: "clear when a check says
    /// up-to-date"), regardless of source. Which UI surfaces which notice
    /// beyond that is decided by `self.about_state.checking`: while a manual
    /// check is in flight, this frame's notice *is* that check's result
    /// (`about_state.checking` is cleared here and the About window's inline
    /// label/the update dialog reflects it); otherwise it's the startup
    /// check's own (silent, log-only) result.
    ///
    /// Simplification (documented): if a startup check and a manual check
    /// somehow overlap in the same frame window, this can't tell their
    /// notices apart and may attribute one to the other. Both come from
    /// `run_update_check`'s exact same shape either way, and the startup
    /// check only ever fires once, very early in the process's life, so in
    /// practice the two paths don't actually race within a session.
    fn drain_update_notices(&mut self) {
        for notice in self.services.update_rx.try_iter().collect::<Vec<_>>() {
            let awaiting_manual = self.about_state.checking;
            match notice {
                UpdateNotice::Available(info) => {
                    self.action_state.update_notice.info = Some(info);
                    if awaiting_manual {
                        self.about_state.checking = false;
                        self.about_state.result = None;
                        self.action_state.update_notice.dialog_open = true;
                    }
                }
                UpdateNotice::UpToDate => {
                    self.action_state.update_notice.info = None;
                    if awaiting_manual {
                        self.about_state.checking = false;
                        self.about_state.result = Some(AboutCheckResult::UpToDate);
                    } else {
                        log::debug!("startup update check: already up to date");
                    }
                }
                UpdateNotice::CheckFailed(detail) => {
                    if awaiting_manual {
                        self.about_state.checking = false;
                        self.about_state.result = Some(AboutCheckResult::Failed(detail));
                    } else {
                        log::debug!("startup update check failed: {detail}");
                    }
                }
            }
        }
    }

    /// Spawns the About window's manual "Check for updates" — bypasses
    /// every startup gate (the ≤7-day cadence, the Privacy toggle, and the
    /// `HUMM_DISABLE_UPDATE_CHECK` env var: binding decision, an explicit
    /// user click), but funnels through the exact same
    /// [`services::run_update_check`] the startup path uses, onto the same
    /// [`Services::update_tx`] channel.
    fn spawn_manual_update_check(&self) {
        let settings = Arc::clone(&self.services.settings);
        let settings_path = self.services.settings_path.clone();
        let tx = self.services.update_tx.clone();
        std::thread::spawn(move || {
            services::run_update_check(
                &settings,
                &settings_path,
                &tx,
                humm_core::update::VERSION_INFO_URL,
                CURRENT_VERSION,
            );
        });
    }

    /// Opens/keeps open the About window (Help → "About humm") as a real OS
    /// viewport — an *immediate* viewport (see `views::about`'s module doc
    /// for the immediate-vs-deferred tradeoff). [`about::ui`] itself is the
    /// plain, kittest-drivable function; this wrapper is the (manually
    /// verified — no display in this sandboxed environment)
    /// viewport-plumbing part.
    fn show_about_viewport(&mut self, ctx: &egui::Context) {
        let about_state = &self.about_state;
        let mut action = about::AboutAction::default();
        let mut close_requested = false;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("humm-about-viewport"),
            egui::ViewportBuilder::default()
                .with_title(tr("about-app-name"))
                .with_inner_size([380.0, 240.0])
                .with_resizable(false),
            |ui, _class| {
                egui::CentralPanel::default().show(ui, |ui| {
                    action = about::ui(ui, CURRENT_VERSION, about_state);
                });
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    close_requested = true;
                }
            },
        );

        if action.check_clicked && !self.about_state.checking {
            self.about_state.checking = true;
            self.about_state.result = None;
            self.spawn_manual_update_check();
        }
        if action.show_logs_clicked
            && let Err(err) = open::that(humm_core::paths::log_dir())
        {
            log::warn!("could not open the log folder: {err}");
        }
        if action.close_clicked || close_requested {
            self.about_open = false;
        }
    }

    /// Opens/keeps open the Preferences window (⌘,/Help → "Preferences…")
    /// as a real OS viewport, min 650×500 (binding decision). Same
    /// immediate-viewport shape as [`Self::show_about_viewport`]; see
    /// `views::preferences`'s module doc for the tradeoff this implies.
    ///
    /// Review fix (M7 Task 11 follow-up, repaint gap): `ui.locale`/
    /// `ui.font_size` are compared before and after the `preferences::ui`
    /// call, and [`Self::repaint_other_open_viewports`] runs whenever either
    /// changed — the General tab's language combo/font-size row already
    /// apply live process-wide (`i18n::set_locale`/`prefs_general::
    /// apply_font_size` touch global state every `tr()`/style lookup reads),
    /// but an *idle* deferred viewport (a viewer, resizer, the recording
    /// window, the presentation window) only repaints on its own input or
    /// timer — without this, one of those windows sitting open and untouched
    /// would keep rendering the old locale/font scale until the user
    /// happened to interact with it, even though the very next frame it
    /// *does* repaint would already show the change.
    fn show_preferences_viewport(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.preferences_state else {
            return;
        };
        let settings = &self.services.settings;
        let settings_path = &self.services.settings_path;
        let model_root = &self.services.model_root;
        let secrets = &self.services.secrets;
        let validate = &self.services.validate;
        let shortcuts_generation = &self.shortcuts_generation;
        let current_shortcuts = &self.shortcuts;
        let dialogs = self.dialogs.as_ref();
        let mut close_requested = false;

        let appearance_before = ui_appearance_snapshot(settings);

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("humm-preferences-viewport"),
            egui::ViewportBuilder::default()
                .with_title(tr("prefs-title"))
                .with_inner_size([650.0, 500.0])
                .with_min_inner_size([650.0, 500.0]),
            |ui, _class| {
                egui::CentralPanel::default().show(ui, |ui| {
                    preferences::ui(
                        ui,
                        state,
                        model_root,
                        settings,
                        settings_path,
                        secrets,
                        validate,
                        shortcuts_generation,
                        current_shortcuts,
                        dialogs,
                    );
                });
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    close_requested = true;
                }
            },
        );

        let appearance_after = ui_appearance_snapshot(settings);
        if ui_appearance_changed(&appearance_before, &appearance_after) {
            self.repaint_other_open_viewports(ctx);
        }

        if close_requested {
            self.preferences_open = false;
            self.preferences_state = None;
        }
    }

    /// Requests a repaint of every other open deferred viewport — every
    /// open viewer/resizer, the live-recording window, and (if it's also
    /// open) the presentation window — see [`Self::show_preferences_viewport`]'s
    /// own doc for why. Harmless to call for a viewport id that isn't
    /// actually being shown this frame: `egui::Context::request_repaint_of`
    /// only affects a viewport that's currently registered via
    /// `show_viewport_deferred`/`_immediate`, so a request against one that
    /// isn't (this method itself only requests the presentation window's id
    /// when [`crate::views::presentation::PresentationVm::open`] agrees it
    /// should be) is simply never consumed.
    fn repaint_other_open_viewports(&self, ctx: &egui::Context) {
        for id in self.viewers.keys() {
            ctx.request_repaint_of(egui::ViewportId::from_hash_of(("viewer", *id)));
        }
        for id in self.resizers.keys() {
            ctx.request_repaint_of(egui::ViewportId::from_hash_of(("resizer", *id)));
        }
        for id in self.speakers.keys() {
            ctx.request_repaint_of(egui::ViewportId::from_hash_of(("speakers", *id)));
        }
        if let Some(vm_arc) = &self.recording {
            ctx.request_repaint_of(egui::ViewportId::from_hash_of(("recording",)));
            let presentation_open = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .presentation
                .open;
            if presentation_open {
                ctx.request_repaint_of(egui::ViewportId::from_hash_of(("presentation",)));
            }
        }
    }

    /// Toolbar "Open Transcript" / Return / double-click →
    /// [`TableAction::OpenSelected`]: opens (or focuses) a viewer for every
    /// selected row, gated to a non-empty, all-openable selection — the same
    /// [`toolbar_enablement`]`.open` rule the toolbar button enables on, so
    /// Return mirrors the toolbar and double-click (which first collapsed the
    /// selection to its one row) reduces to a per-row gate (design spec §2.1,
    /// ledger #13). A selection that fails the gate is a silent no-op on every
    /// path — Buzz's Return-key bypass of its own gate is deliberately not
    /// ported.
    fn handle_open_selected(&mut self, now: Instant) {
        if !toolbar_enablement(&self.tasks.selected_statuses()).open {
            return;
        }
        let ids: Vec<TranscriptionId> = self.tasks.selection().iter().copied().collect();
        for id in ids {
            self.open_viewer(id, now);
        }
    }

    /// Opens a transcript viewer for `id`, or focuses the existing window if
    /// one is already open (deviation #1: no duplicate editors). A store read
    /// failure surfaces an error toast and opens nothing — the row is left
    /// exactly as it was.
    fn open_viewer(&mut self, id: TranscriptionId, now: Instant) {
        if let Some(existing) = self.viewers.get(&id) {
            existing
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .focus_requested = true;
            return;
        }
        let opened = {
            let settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            ViewerVm::open(id, &self.viewer_deps, &settings)
        };
        match opened {
            Ok(vm) => {
                self.viewers.insert(id, Arc::new(Mutex::new(vm)));
            }
            Err(err) => {
                log::warn!("could not open transcript viewer for {id}: {err}");
                self.push_error_toast(format!("{}: {err}", tr("viewer-open-error")), now);
            }
        }
    }

    /// Opens a resizer window for `source`, or focuses the existing one if this
    /// transcript already has a resizer open (one resizer per source — the
    /// viewer's own no-duplicate deviation #1 shape). Triggered by a viewer's
    /// Resize button (Buzz `on_resize_button_clicked`). A store-read failure at
    /// open is logged and opens nothing.
    fn open_resizer(&mut self, source: TranscriptionId) {
        if let Some(existing) = self.resizers.get(&source) {
            existing
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .focus_requested = true;
            return;
        }
        let opened = {
            let settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            ResizerVm::open(source, &self.viewer_deps, &settings)
        };
        match opened {
            Ok(vm) => {
                self.resizers.insert(source, Arc::new(Mutex::new(vm)));
            }
            Err(err) => log::warn!("could not open transcript resizer for {source}: {err}"),
        }
    }

    /// Opens a Speaker-ID window for `source`, or focuses the existing one if
    /// this transcript already has one open (one per source — the
    /// viewer/resizer deviation #1 shape). Triggered by a viewer's Identify
    /// Speakers button, now live (Buzz `on_speaker_identification_button_
    /// clicked`; humm's Intel-Mac gate is not ported — humm targets Apple
    /// Silicon only). The window always opens: a missing audio file gates
    /// only the Identify button inside it ([`SpeakerIdVm::open`]'s own
    /// `MissingAudio` stage, Buzz `:514-518`), not the window itself. A
    /// store-read failure at open is logged and opens nothing.
    fn open_speakers(&mut self, source: TranscriptionId) {
        if let Some(existing) = self.speakers.get(&source) {
            existing
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .focus_requested = true;
            return;
        }
        match SpeakerIdVm::open(source, &self.speaker_deps) {
            Ok(vm) => {
                self.speakers.insert(source, Arc::new(Mutex::new(vm)));
            }
            Err(err) => log::warn!("could not open speaker-id window for {source}: {err}"),
        }
    }

    /// Opens the live-recording window (F-060/F-004), or focuses it if
    /// already open (single instance — the viewer/resizer windows' own
    /// deviation #1 shape). Triggered by the toolbar Record button or Ctrl+R
    /// (`action_state.record_requested`, drained in [`eframe::App::ui`]).
    /// [`RecordingVm::open`] never fails outright (Buzz parity: the window
    /// always opens, even with no input devices — see its own doc comment).
    fn open_recording_window(&mut self) {
        if let Some(existing) = &self.recording {
            existing
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .focus_requested = true;
            return;
        }
        let vm = {
            let settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            RecordingVm::open(&self.recording_deps, &settings)
        };
        self.recording = Some(Arc::new(Mutex::new(vm)));
    }

    /// Executes one resizer action harvested from a resizer window (F-086).
    /// Extend/Resize run synchronously (fast, pure) and return the affected id;
    /// Merge starts a worker thread and returns `None` (its completion arrives
    /// later as an [`AppEvent`]). The return value is for the test seam — the
    /// harvest loop discards it.
    fn apply_resizer_action(
        &mut self,
        source: TranscriptionId,
        action: ResizerAction,
        now: Instant,
        ctx: &egui::Context,
    ) -> Option<TranscriptionId> {
        let (create_new, extend_amount, target_chars, algo, language) = {
            let vm = self
                .resizers
                .get(&source)?
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                vm.create_new,
                vm.extend_amount.clone(),
                vm.target_chars,
                // The regroup string with the HUMM_MERGE_REGROUP_RULE override
                // applied (spec §2.6) — `build_algo` alone is the pre-override
                // string.
                resizer::merge_algo_for(&vm),
                vm.language.clone(),
            )
        };

        match action {
            ResizerAction::Extend => {
                let segs = self.read_source_segments(source)?;
                let out = humm_core::resize::extend_endings(
                    &segs,
                    resizer::parse_extend_amount(&extend_amount),
                );
                self.finish_sync_resize(source, create_new, &out, ctx)
            }
            ResizerAction::Resize => {
                let segs = self.read_source_segments(source)?;
                let out = humm_core::resize::equalize(&segs, target_chars as usize);
                self.finish_sync_resize(source, create_new, &out, ctx)
            }
            ResizerAction::Merge => {
                self.start_merge(source, create_new, algo, language, ctx, now);
                None
            }
        }
    }

    /// Re-reads `source`'s current segments from the store (Buzz re-queries at
    /// each button click, `get_transcription_segments`) as plain [`Segment`]s.
    /// A read failure is logged and yields `None` — the caller aborts the
    /// operation rather than saving over the transcript with nothing.
    fn read_source_segments(&self, source: TranscriptionId) -> Option<Vec<Segment>> {
        match self.services.store.get_segment_rows(source) {
            Ok(rows) => Some(
                rows.into_iter()
                    .map(|row| Segment {
                        start: row.start,
                        end: row.end,
                        text: row.text,
                        translation: row.translation,
                    })
                    .collect(),
            ),
            Err(err) => {
                log::warn!("resizer: could not read segments for {source}: {err}");
                None
            }
        }
    }

    /// The synchronous save+refresh tail shared by Extend and Resize: save
    /// `out` (create-new copy → complete, or in-place replace) and, on success,
    /// apply the [`AppEvent::TranscriptionsUpdated`] effect inline (refresh the
    /// table row, close the source's viewer + resizer). A save failure instead
    /// mirrors the merge worker's `RegroupError` arm (final review, F-086):
    /// the error lands on the resizer's own banner, `running` is cleared, and
    /// neither the viewer nor the resizer closes — a silently-dropped edit
    /// would otherwise look identical to a saved one. Returns the affected id
    /// on success.
    fn finish_sync_resize(
        &mut self,
        source: TranscriptionId,
        create_new: bool,
        out: &[Segment],
        ctx: &egui::Context,
    ) -> Option<TranscriptionId> {
        match resizer::save_segments(&self.services.store, source, create_new, out) {
            Ok(affected) => {
                self.apply_transcriptions_updated(affected, source, true);
                Some(affected)
            }
            Err(err) => {
                log::warn!("resizer: could not save resized segments for {source}: {err}");
                if let Some(vm_arc) = self.resizers.get(&source) {
                    let mut vm = vm_arc
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    vm.error = Some(format!("{} {err}", tr("resizer-save-error")));
                    vm.running = false;
                }
                ctx.request_repaint();
                None
            }
        }
    }

    /// Starts a merge on a background thread (Buzz `on_merge_button_clicked` +
    /// its `TranscriptionWorker`): for "Create new transcript", copy the source
    /// row and mark it visible at 0 % *first* (so the new row appears in the
    /// table immediately — Buzz's first `transcriptions_updated_signal.emit`),
    /// then hide the resizer window (`running = true`) and spawn a thread that
    /// reads the source segments, regroups them, saves, and posts
    /// [`AppEvent::TranscriptionsUpdated`]. A `RegroupError` (an unsupported op
    /// via `HUMM_MERGE_REGROUP_RULE`) or a save failure (final review, F-086)
    /// is instead surfaced on the resizer's own error banner and re-shows the
    /// window — neither ever posts `TranscriptionsUpdated`, so the viewer and
    /// resizer stay open on any failure.
    fn start_merge(
        &mut self,
        source: TranscriptionId,
        create_new: bool,
        algo: String,
        language: Option<String>,
        ctx: &egui::Context,
        now: Instant,
    ) {
        // Defensive ordering: confirm the resizer window still exists *before*
        // any create-new copy. If it had vanished, a copy made here would leave
        // a permanently-`queued` row nothing ever completes (the worker's
        // completion path routes through this window).
        if !self.resizers.contains_key(&source) {
            return;
        }

        let store = Arc::clone(&self.services.store);
        let affected = if create_new {
            match store.copy_transcription(source) {
                Ok(new_id) => {
                    if let Err(err) = store.update_progress_direct(new_id, 0.0) {
                        log::warn!("resizer: update_progress_direct failed for {new_id}: {err}");
                    }
                    // Make the freshly-copied row visible in the table right away
                    // (it is inserted because it is not yet tracked).
                    self.tasks.refresh_row(new_id, &store);
                    new_id
                }
                Err(err) => {
                    log::warn!("resizer: copy_transcription failed for {source}: {err}");
                    self.push_error_toast(format!("{}: {err}", tr("resizer-merge-error")), now);
                    return;
                }
            }
        } else {
            source
        };

        let Some(vm_arc) = self.resizers.get(&source) else {
            return;
        };
        {
            let mut vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.error = None;
            vm.running = true;
        }
        let vm_arc = Arc::clone(vm_arc);
        let tx = self.app_event_tx.clone();
        let ctx = ctx.clone();

        std::thread::spawn(move || {
            let segs: Vec<Segment> = match store.get_segment_rows(source) {
                Ok(rows) => rows
                    .into_iter()
                    .map(|row| Segment {
                        start: row.start,
                        end: row.end,
                        text: row.text,
                        translation: row.translation,
                    })
                    .collect(),
                Err(err) => {
                    log::warn!("resizer merge: could not read segments for {source}: {err}");
                    let mut vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
                    vm.error = Some(format!("{} {err}", tr("resizer-merge-error")));
                    vm.running = false;
                    ctx.request_repaint();
                    return;
                }
            };

            match humm_core::regroup::regroup_segments(&segs, language.as_deref(), &algo) {
                Ok(out) => {
                    let saved = if create_new {
                        store.mark_replaced_completed(affected, &out)
                    } else {
                        store.replace_segments(source, &out)
                    };
                    match saved {
                        Ok(()) => {
                            let _ = tx.send(AppEvent::TranscriptionsUpdated {
                                affected,
                                source,
                                close_source: true,
                            });
                        }
                        Err(err) => {
                            // Save failed (final review, F-086): surface it on
                            // the resizer's own banner exactly like the
                            // RegroupError arm below rather than sending the
                            // success-shaped `TranscriptionsUpdated` — that
                            // would close the viewer/resizer over a merge that
                            // never actually landed. For "Create new
                            // transcript", the copy row (`affected`) was
                            // already inserted and shown at 0 % before this
                            // thread started (`self.tasks.refresh_row` above);
                            // it is left exactly as-is (still `Queued`) rather
                            // than refreshed again, since nothing about it
                            // changed here — if the user never retries, the
                            // next launch's stale-task sweep flips it to
                            // `Canceled` (§21 semantics).
                            log::warn!(
                                "resizer merge: could not save segments for {affected}: {err}"
                            );
                            let mut vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
                            vm.error = Some(format!("{} {err}", tr("resizer-save-error")));
                            vm.running = false;
                        }
                    }
                    ctx.request_repaint();
                }
                Err(err) => {
                    // Unsupported regroup op (via HUMM_MERGE_REGROUP_RULE): show
                    // the actionable error on the resizer and re-show its window.
                    let mut vm = vm_arc.lock().unwrap_or_else(|p| p.into_inner());
                    vm.error = Some(format!("{} {err}", tr("resizer-merge-error")));
                    vm.running = false;
                    ctx.request_repaint();
                }
            }
        });
    }

    /// Applies the [`AppEvent::TranscriptionsUpdated`] effect: refresh (or
    /// insert) the `affected` transcript's table row, then — only when
    /// `close_source` is set — close the `source`'s viewer and its resizer
    /// (Buzz `transcriptions_updated` → viewer `close`, F-086's resizer;
    /// F-087's Speaker-ID window always passes `false` — see the
    /// `AppEvent::TranscriptionsUpdated` variant doc). Shared by the
    /// synchronous resize/extend and speaker-id-save paths and the merge
    /// worker's drained event.
    fn apply_transcriptions_updated(
        &mut self,
        affected: TranscriptionId,
        source: TranscriptionId,
        close_source: bool,
    ) {
        // `refresh_row` re-reads the row and replaces it in place, or inserts it
        // when untracked (a create-new copy) — so one call covers both shapes.
        self.tasks.refresh_row(affected, &self.services.store);
        if close_source {
            self.close_viewer(source);
        }
    }

    /// Closes `id`'s viewer window (persisting its geometry through the shared
    /// last-close-wins path — the watch-item fix: the dormant `AppEvent` path
    /// previously dropped a viewer without persisting geometry) and, with it,
    /// `id`'s resizer and Speaker-ID windows (Buzz `closeEvent` closes both
    /// child dialogs, `transcription_viewer_widget.py:1492-1496`). A no-op
    /// for an id with none of the three open.
    fn close_viewer(&mut self, id: TranscriptionId) {
        if let Some(vm_arc) = self.viewers.remove(&id) {
            let geometry = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .observed_geometry;
            self.persist_viewer_geometry_if_changed(geometry);
        }
        // Closing a viewer closes its resizer and speaker-id window too
        // (Buzz closeEvent parity).
        self.resizers.remove(&id);
        self.speakers.remove(&id);
    }

    /// Renders every open viewer as a deferred egui viewport
    /// (`ctx.show_viewport_deferred`, ARCHITECTURE §7 / design spec §1.1), then
    /// harvests close requests: a viewer whose render closure set
    /// `close_requested` this frame has its geometry persisted (shared,
    /// last-close-wins) and its entry dropped — dropping the VM (and, from
    /// Task 10 on, stopping its playback via `Drop`). Re-opening the same
    /// transcript focuses the live window instead of duplicating it.
    ///
    /// The deferred callback must be `Send + Sync + 'static`, so it captures
    /// only `Arc`-shared state; per-window state lives behind the viewer's
    /// `Arc<Mutex<ViewerVm>>`. This viewport plumbing is verifiable only in a
    /// windowed run (an `egui_kittest` `Harness` sees only its own root
    /// viewport) — the same constraint the About/Preferences immediate
    /// viewports carry; `viewer_ui` itself is kittest-driven directly.
    fn show_viewer_viewports(&mut self, ctx: &egui::Context) {
        if self.viewers.is_empty() {
            return;
        }
        for (id, vm_arc) in &self.viewers {
            let (title, geometry) = {
                let vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                (vm.title.clone(), vm.initial_geometry)
            };
            let builder = viewer_viewport_builder(&title, geometry);

            let vm_for_cb = Arc::clone(vm_arc);
            let deps = Arc::clone(&self.viewer_deps);
            let shortcuts = Arc::clone(&self.shortcuts);
            let settings = Arc::clone(&self.services.settings);
            let settings_path = self.services.settings_path.clone();

            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of(("viewer", *id)),
                builder,
                move |ctx, _class| {
                    let mut vm = vm_for_cb
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    egui::CentralPanel::default().show(ctx, |ui| {
                        crate::views::viewer::viewer_ui(
                            ui,
                            &mut vm,
                            &deps,
                            &shortcuts,
                            &settings,
                            &settings_path,
                        );
                    });

                    // Record the live geometry for persist-on-close, and honor
                    // a pending focus / close request.
                    let (outer, inner) = ctx
                        .input(|input| (input.viewport().outer_rect, input.viewport().inner_rect));
                    if let Some(inner) = inner {
                        vm.observed_geometry = Some(ViewerGeometry {
                            x: outer.map(|rect| rect.min.x),
                            y: outer.map(|rect| rect.min.y),
                            w: inner.width(),
                            h: inner.height(),
                        });
                    }
                    if std::mem::take(&mut vm.focus_requested) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    if ctx.input(|input| input.viewport().close_requested()) {
                        vm.close_requested = true;
                    }
                },
            );
        }

        // Harvest Resize-button requests (open/focus the source's resizer)
        // before the close harvest, so a viewer that requests a resize and is
        // then closed the same frame still gets its resizer opened first.
        let resize_reqs: Vec<TranscriptionId> = self
            .viewers
            .iter()
            .filter_map(|(id, vm_arc)| {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                std::mem::take(&mut vm.resize_requested).then_some(*id)
            })
            .collect();
        for id in resize_reqs {
            self.open_resizer(id);
        }

        // Harvest Identify Speakers requests (F-087), same ordering rationale
        // as the resize harvest just above.
        let speaker_reqs: Vec<TranscriptionId> = self
            .viewers
            .iter()
            .filter_map(|(id, vm_arc)| {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                std::mem::take(&mut vm.identify_speakers_requested).then_some(*id)
            })
            .collect();
        for id in speaker_reqs {
            self.open_speakers(id);
        }

        let closing: Vec<TranscriptionId> = self
            .viewers
            .iter()
            .filter(|(_, vm_arc)| {
                vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .close_requested
            })
            .map(|(id, _)| *id)
            .collect();
        for id in closing {
            self.close_viewer(id);
        }
    }

    /// Renders every open resizer as a deferred egui viewport (mirroring
    /// [`Self::show_viewer_viewports`], F-086 / design spec §2.6), then harvests
    /// each window's intents: a queued action (Extend/Resize/Merge button) is
    /// executed via [`Self::apply_resizer_action`], and a close request drops
    /// the window. While a merge runs (`running`), the window is hidden
    /// (`ViewportCommand::Visible(false)`) — Buzz's `self.hide()`.
    ///
    /// The deferred callback captures only `Arc`-shared state (the same
    /// `Send + Sync + 'static` constraint the viewer viewports carry); this
    /// viewport plumbing is verifiable only in a windowed run — `resizer::ui`
    /// itself and the action-apply seam are kittest-driven directly.
    fn show_resizer_viewports(&mut self, ctx: &egui::Context, now: Instant) {
        if self.resizers.is_empty() {
            return;
        }
        for (source, vm_arc) in &self.resizers {
            let title = {
                vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .title
                    .clone()
            };
            let builder = resizer_viewport_builder(&title);

            let vm_for_cb = Arc::clone(vm_arc);
            let settings = Arc::clone(&self.services.settings);
            let settings_path = self.services.settings_path.clone();

            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of(("resizer", *source)),
                builder,
                move |ctx, _class| {
                    let mut vm = vm_for_cb
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    // The window hides while a merge runs and re-shows on error;
                    // only send the command on an actual transition.
                    let want_visible = !vm.running;
                    if vm.window_visible != want_visible {
                        vm.window_visible = want_visible;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(want_visible));
                    }
                    egui::CentralPanel::default().show(ctx, |ui| {
                        crate::views::resizer::ui(ui, &mut vm, &settings, &settings_path);
                    });
                    if std::mem::take(&mut vm.focus_requested) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    if ctx.input(|input| input.viewport().close_requested()) {
                        vm.close_requested = true;
                    }
                },
            );
        }

        // Harvest queued actions (one at most per window per frame), then run
        // them — collected first so executing one (which can drop a resizer)
        // never mutates the map mid-iteration.
        let actions: Vec<(TranscriptionId, ResizerAction)> = self
            .resizers
            .iter()
            .filter_map(|(source, vm_arc)| {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                vm.pending_action.take().map(|action| (*source, action))
            })
            .collect();
        for (source, action) in actions {
            self.apply_resizer_action(source, action, now, ctx);
        }

        let closing: Vec<TranscriptionId> = self
            .resizers
            .iter()
            .filter(|(_, vm_arc)| {
                vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .close_requested
            })
            .map(|(id, _)| *id)
            .collect();
        for id in closing {
            self.resizers.remove(&id);
        }
    }

    /// Renders every open Speaker-ID window as a deferred egui viewport
    /// (mirroring [`Self::show_resizer_viewports`], F-087), then harvests each
    /// window's intents: a Save click runs [`Self::apply_speaker_save`]
    /// (closing the window on success), and a close request drops it. Unlike
    /// the resizer's Merge, an in-flight Identify never hides this window —
    /// Buzz's own widget keeps rendering progress in place while its worker
    /// runs (`:593-661`), so there is no `window_visible` bookkeeping here.
    ///
    /// The deferred callback captures only `Arc`-shared state (the same
    /// `Send + Sync + 'static` constraint the viewer/resizer viewports
    /// carry); this viewport plumbing is verifiable only in a windowed run —
    /// `views::speaker_id::ui` and the save-apply seam are kittest-driven
    /// directly.
    fn show_speaker_viewports(&mut self, ctx: &egui::Context) {
        if self.speakers.is_empty() {
            return;
        }
        for (source, vm_arc) in &self.speakers {
            let title = {
                vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .title
                    .clone()
            };
            let builder = speaker_viewport_builder(&title);

            let vm_for_cb = Arc::clone(vm_arc);
            let deps = Arc::clone(&self.speaker_deps);

            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of(("speakers", *source)),
                builder,
                move |ctx, _class| {
                    let mut vm = vm_for_cb
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    egui::CentralPanel::default().show(ctx, |ui| {
                        crate::views::speaker_id::ui(ui, &mut vm, &deps);
                    });
                    if std::mem::take(&mut vm.focus_requested) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    if ctx.input(|input| input.viewport().close_requested()) {
                        vm.close_requested = true;
                    }
                },
            );
        }

        // Harvest Save clicks (one at most per window per frame) before the
        // close harvest, mirroring the resizer's own action-then-close order.
        let saves: Vec<TranscriptionId> = self
            .speakers
            .iter()
            .filter_map(|(source, vm_arc)| {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                std::mem::take(&mut vm.save_requested).then_some(*source)
            })
            .collect();
        for source in saves {
            self.apply_speaker_save(source);
        }

        let closing: Vec<TranscriptionId> = self
            .speakers
            .iter()
            .filter(|(_, vm_arc)| {
                vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .close_requested
            })
            .map(|(id, _)| *id)
            .collect();
        for id in closing {
            self.speakers.remove(&id);
        }
    }

    /// Executes one Speaker-ID Save click (F-087, [`speaker_id::save`]):
    /// reads the window's current `Ready` state, labels + copies + completes
    /// it, then — on success — applies `AppEvent::TranscriptionsUpdated`
    /// inline with `close_source: false` (refresh the table row; the source
    /// viewer stays open, per this window's own module doc) and closes this
    /// window. A failure records the message on `vm.save_error` (a test
    /// reads it via [`SpeakerIdVm::save_error_for_test`]) and leaves the
    /// window — and its still-`Ready` Step 2 form — open, the same
    /// "error is a side channel, not a state replacement" shape
    /// [`Self::finish_sync_resize`]'s failure arm uses. A no-op (defensive)
    /// if the window vanished, or isn't `Ready`, before this ran. Returns the
    /// new transcript's id on success (the resizer's own
    /// `apply_resizer_action` return-value convention — the real harvest
    /// discards it, [`Self::run_speaker_save_for_test`] doesn't).
    fn apply_speaker_save(&mut self, source: TranscriptionId) -> Option<TranscriptionId> {
        let vm_arc = self.speakers.get(&source)?;
        let ready = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &vm.stage {
                SpeakerIdStage::Ready(ready) => ready.clone(),
                _ => return None,
            }
        };

        match speaker_id::save(&self.services.store, source, &ready) {
            Ok(new_id) => {
                self.apply_transcriptions_updated(new_id, source, false);
                self.speakers.remove(&source);
                Some(new_id)
            }
            Err(err) => {
                log::warn!("speaker-id: could not save labeled segments for {source}: {err}");
                if let Some(vm_arc) = self.speakers.get(&source) {
                    let mut vm = vm_arc
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    vm.save_error = Some(format!("{} {err}", tr("speaker-id-save-error")));
                }
                None
            }
        }
    }

    /// Renders the live-recording window as a deferred egui viewport (F-060,
    /// mirroring [`Self::show_viewer_viewports`]/[`Self::show_resizer_viewports`]),
    /// a no-op while [`Self::recording`] is `None`. Reads
    /// `settings.models.custom_model_url`/`settings.recording.measured_speed`,
    /// plus the Lumii locale gate (F-044/J-9, design spec §8.2,
    /// [`crate::i18n::lumii_locale_requested`]), fresh every render (all
    /// cheap, read-only per-frame lookups —
    /// [`crate::views::recording::recording_ui`] never locks `Settings`
    /// itself, matching `options_sheet::ui`'s own convention).
    ///
    /// Close-while-recording is the spec §2.1 / Buzz `closeEvent` two-phase
    /// deferral: a close request while [`RecordingStatus::Stopped`] latches
    /// `close_requested` immediately for [`Self::harvest_recording_close`];
    /// while [`RecordingStatus::Recording`] it instead latches
    /// [`RecordingVm::pending_close`], moves to
    /// [`RecordingStatus::Stopping`], and latches a stop intent (the exact
    /// same intent a Stop click would) — `Self::harvest_recording_intent`
    /// cancels the active session, and once its `Stopped` message arrives
    /// (`Self::drain_live_session`), `pending_close` makes that handler set
    /// `close_requested` instead of restoring the stopped-state UI. While
    /// already [`RecordingStatus::Stopping`], a close request just latches
    /// `pending_close` (the stop is already in flight). Every branch that
    /// doesn't allow an immediate close sends `CancelClose` so the OS window
    /// itself never disappears out from under the deferred logic.
    fn show_recording_viewport(&mut self, ctx: &egui::Context) {
        let Some(vm_arc) = &self.recording else {
            return;
        };
        let vm_for_cb = Arc::clone(vm_arc);
        let deps = Arc::clone(&self.recording_deps);
        let settings = Arc::clone(&self.services.settings);
        let settings_path = self.services.settings_path.clone();

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(("recording",)),
            egui::ViewportBuilder::default()
                .with_title(tr("recording-window-title"))
                .with_inner_size([700.0, 600.0]),
            move |ctx, _class| {
                let mut vm = vm_for_cb
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let (custom_model_available, measured_speed, include_lumii) = {
                    let system_locale = i18n::locale_from_env_vars(
                        std::env::var("LC_ALL").ok().as_deref(),
                        std::env::var("LANG").ok().as_deref(),
                    );
                    let settings = settings
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    (
                        settings.models.custom_model_url.is_some(),
                        settings.recording.measured_speed.clone(),
                        i18n::lumii_locale_requested(&settings, system_locale.as_deref()),
                    )
                };
                egui::CentralPanel::default().show(ctx, |ui| {
                    crate::views::recording::recording_ui(
                        ui,
                        &mut vm,
                        &deps,
                        custom_model_available,
                        include_lumii,
                        &measured_speed,
                        &settings,
                        &settings_path,
                    );
                });
                // `HUMM_SMOKE_METER_FRAME_LOG` acceptance instrumentation
                // (M5 Task 14 item 5): one sample per actual redraw of this
                // deferred viewport — a no-op unless the hook is armed (see
                // `RecordingVm::record_meter_frame`'s own doc).
                vm.record_meter_frame();
                if std::mem::take(&mut vm.focus_requested) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                if ctx.input(|input| input.viewport().close_requested()) {
                    match vm.status {
                        RecordingStatus::Stopped => {
                            vm.close_requested = true;
                        }
                        RecordingStatus::Recording => {
                            vm.pending_close = true;
                            vm.status = RecordingStatus::Stopping;
                            vm.pending_intent =
                                Some(crate::recording::RecordingIntent::StopRequested);
                            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        }
                        RecordingStatus::Stopping => {
                            vm.pending_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        }
                    }
                }
            },
        );
    }

    /// Renders the presentation window as a deferred egui viewport (F-065,
    /// spec §2.6), mirroring [`Self::show_recording_viewport`] — a no-op
    /// while [`Self::recording`] is `None` **or** its
    /// [`crate::views::presentation::PresentationVm::open`] is false. This
    /// double gate is also how the presentation window closes when the
    /// recording window does (Buzz `_do_close`, `:1204-1206`): once
    /// `self.recording` is dropped by [`Self::harvest_recording_close`],
    /// this method's early return simply stops calling
    /// `show_viewport_deferred` for it, and egui/eframe treat an
    /// unrequested deferred viewport as closed — no explicit teardown call
    /// needed here.
    ///
    /// Owns every call that must run from *this* viewport's own deferred
    /// closure rather than the recording window's (`views::presentation`'s
    /// module doc): reading/toggling OS fullscreen state
    /// (`ViewportCommand::Fullscreen`) and Esc-exits-fullscreen-only
    /// (Buzz `keyPressEvent`, `presentation_window.py:172-179`).
    ///
    /// The CSS override is **not** reloaded from disk every frame (review
    /// finding — this closure runs at the presentation window's own repaint
    /// rate, up to ~30/frame/s while recording, and `load_css_override` is an
    /// `exists`+`read`+`parse` syscall round-trip): it's cached on
    /// `vm.presentation.css_override` and only recomputed when
    /// `vm.presentation.css_dirty` is set — that field's own doc lists the
    /// exact trigger set (window open, theme change, debounced text-size/color
    /// save), matching every point Buzz's own `PresentationWindow.load_settings()`
    /// (which re-reads the CSS file as a side effect) is called from.
    fn show_presentation_viewport(&mut self, ctx: &egui::Context) {
        let Some(vm_arc) = &self.recording else {
            return;
        };
        let open = vm_arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .presentation
            .open;
        if !open {
            return;
        }
        let vm_for_cb = Arc::clone(vm_arc);

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(("presentation",)),
            egui::ViewportBuilder::default()
                .with_title(tr("presentation-window-title"))
                .with_inner_size([800.0, 600.0]),
            move |ctx, _class| {
                let mut vm = vm_for_cb
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if std::mem::take(&mut vm.presentation.css_dirty) {
                    vm.presentation.css_override = crate::views::presentation::load_css_override();
                }
                let style = crate::views::presentation::effective_style(
                    &vm.presentation,
                    vm.presentation.css_override.as_ref(),
                );
                let text = vm.panel_text.clone();
                // F-063: the presentation window shows a translation region
                // below the transcript once a translation exists and live
                // translation is enabled (Buzz's `translation_display`, shown
                // only when non-empty). `window_ui` itself no-ops on `None`/
                // empty, so the gate here is just the enable flag.
                let translation = if vm.options.enable_llm_translation {
                    Some(vm.translation_panel_text.clone())
                } else {
                    None
                };
                crate::views::presentation::window_ui(ctx, &text, translation.as_deref(), &style);

                if std::mem::take(&mut vm.presentation.focus_requested) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                if std::mem::take(&mut vm.presentation.fullscreen_toggle_requested) {
                    let currently_fullscreen = ctx
                        .input(|input| input.viewport().fullscreen)
                        .unwrap_or(false);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!currently_fullscreen));
                }
                let is_fullscreen = ctx
                    .input(|input| input.viewport().fullscreen)
                    .unwrap_or(false);
                if is_fullscreen && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
                if ctx.input(|input| input.viewport().close_requested()) {
                    vm.presentation.open = false;
                }
            },
        );
    }

    /// Harvests a stopped-state close request from the recording window
    /// (Buzz `_do_close`): persists the sticky prefs
    /// ([`RecordingVm::close`] writes `settings.recording`'s language/task/
    /// model/initial-prompt fields) and drops the window. A save failure is
    /// logged, not fatal (the same non-fatal-settings-write contract as
    /// [`Self::persist_layout_if_changed`]).
    fn harvest_recording_close(&mut self) {
        let Some(vm_arc) = &self.recording else {
            return;
        };
        let should_close = vm_arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .close_requested;
        if !should_close {
            return;
        }
        if let Some(vm_arc) = self.recording.take() {
            let mut vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.close(&mut settings.recording);
            // Review fix: the whole window (and its embedded presentation
            // bar) is disappearing, so any presentation edit still sitting
            // in the ~100 ms debounce window must be flushed here too, or
            // it's lost — piggybacks on the unconditional `settings.save`
            // just below (`RecordingVm::close`'s own doc comment). The
            // returned bool is ignored on purpose: this save already runs
            // unconditionally regardless of whether anything was pending.
            let _ = vm
                .presentation
                .flush_pending_save(&mut settings.presentation);
            if let Err(err) = settings.save(&self.services.settings_path) {
                log::warn!("could not persist recording sticky prefs: {err}");
            }
        }
    }

    /// Harvests `RecordingVm::pending_intent` — the click seam Task 10 left
    /// (`recording.rs`'s module doc), now consumed for real: `StartRequested`
    /// (Stopped→Recording, including the deferred-close-triggered path,
    /// `Self::show_recording_viewport`) builds a [`LiveSessionRequest`] from
    /// the window's live options + `settings.recording` and spawns the
    /// session; `StopRequested` (Recording→Stopping) cancels the active
    /// session's cooperative-stop token — a no-op if none is active (a Stop
    /// intent latched before `StartRequested` was ever harvested, which
    /// [`RecordingVm::on_record_clicked`]'s own state machine makes
    /// unreachable, but defensive regardless).
    fn harvest_recording_intent(&mut self, ctx: &egui::Context) {
        let Some(vm_arc) = self.recording.clone() else {
            return;
        };
        let intent = {
            let mut vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.pending_intent.take()
        };
        match intent {
            Some(crate::recording::RecordingIntent::StartRequested) => {
                self.start_live_session(&vm_arc, ctx);
            }
            Some(crate::recording::RecordingIntent::StopRequested) => {
                let vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(session) = &vm.session {
                    session.cancel.cancel();
                }
            }
            None => {}
        }
    }

    /// Builds a [`LiveSessionRequest`] from `vm`'s live options snapshot and
    /// the current `settings.recording`/`HUMM_UPLOAD_URL`, and spawns it
    /// (`crate::live_session::spawn_live_session`). The live options — model
    /// size/task/language/prompt, silence threshold, line separator,
    /// transcription step, and the selected device — come from the **VM**
    /// (`RecordingOptions`, mutated live by both the options group and the
    /// Task 12 advanced sheet), not `settings.recording` directly: they
    /// reflect live, possibly-just-edited selections, and `settings.recording`
    /// is only the sticky *prefill/save* target for those same fields (Buzz:
    /// `self.transcription_options` is the shared live object both the group
    /// box and `AdvancedSettingsDialog` mutate in place; matches
    /// `recording_transcriber_widget.py:667-673`'s own split). Mode/hide-
    /// unconfirmed/export/upload-url are the advanced sheet's *immediate*-save
    /// fields instead (`views::recording_advanced`'s own module doc) — those
    /// genuinely do come from `recording_settings` below, since `settings.recording`
    /// already holds their current value the instant they change.
    fn start_live_session(&mut self, vm_arc: &Arc<Mutex<RecordingVm>>, ctx: &egui::Context) {
        let (device_id, options, meter) = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                vm.selected_device.clone(),
                vm.options.clone(),
                Arc::clone(&vm.meter),
            )
        };

        // F-061b: the API backend needs a resolvable key *before* the
        // session is ever allowed to start -- a deliberate, documented
        // improvement over Buzz, whose own live-API arm has no precheck at
        // all and instead surfaces a bad/missing key as fake "Error: ..."
        // transcript text on the very first chunk
        // (`recording_transcriber.py:355-359`; see the task report for the
        // full source read). Resolved fresh here (keyring + `settings.openai`,
        // via the shared `resolve_openai_config`), matching the "read fresh
        // at record-start" contract `Self::force_cpu_setting`/
        // `GuiEngineFactory::create_openai` already use. Deliberately runs
        // **before** the previous session's translator is torn down below
        // (review fix): an aborted click must be side-effect-free, leaving
        // any still-running translator from an earlier session untouched. A
        // missing/empty key aborts the click inline: the optimistic
        // Stopped->Recording UI flip `RecordingVm::on_record_clicked`
        // already made is reverted via the same `flush_finish_stopping`
        // every other early-Stop path uses, and no session thread is ever
        // spawned.
        let api_config = if options.backend == Backend::OpenAiApi {
            match resolve_openai_config(
                &self.services.settings,
                self.recording_deps.secrets.as_ref(),
            ) {
                Ok(cfg) => Some(cfg),
                Err(()) => {
                    let deps = Arc::clone(&self.recording_deps);
                    let mut vm = vm_arc
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    vm.error_dialog = Some(tr("viewer-translate-key-required-body"));
                    self.flush_finish_stopping(&mut vm, &deps);
                    ctx.request_repaint();
                    return;
                }
            }
        } else {
            None
        };

        // A new Record click tears down the previous session's translator
        // (fresh worker + applier per session, spec §7.4 lifecycle) -- only
        // reached once the precheck above has passed.
        let old_translator = {
            let mut vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.live_translator.take()
        };
        if let Some(old) = old_translator {
            old.stop();
        }

        let recording_settings = {
            let settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settings.recording.clone()
        };

        let model = ModelSpec {
            // F-061b: the live session's actual selected backend -- the
            // resolved `api_config` above (`Some` only when `options.backend
            // == Backend::OpenAiApi`) is `run_live_session`'s own signal to
            // skip model resolution/download and `WhisperEngine::load`
            // entirely, so this field only drives the persisted
            // `options_snapshot` below for the API backend, not the ensure/
            // load path.
            backend: options.backend,
            size: options.model_size,
            hf_repo: None,
        };
        let live_options = LiveOptions {
            language: options.language,
            task: options.task,
            temperature: humm_core::domain::DEFAULT_TEMPERATURE.to_vec(),
            threads: None,
        };
        let mode = RecordingMode::from_index(recording_settings.mode);
        // silence_threshold/transcription_step/initial_prompt come from the
        // *live* `options` snapshot, not `recording_settings` — Buzz parity
        // (Task 12): the advanced sheet mutates the same in-memory object
        // this reads, so an edit made before this Record click already
        // applies, even if the window hasn't closed (and thus hasn't
        // persisted) yet. `separator` below is the same live-edit field
        // (`crate::recording::RecordingOptions`'s own doc comment).
        let session_config = SessionConfig {
            mode,
            transcription_step_secs: options.transcription_step,
            silence_threshold: options.silence_threshold,
            initial_prompt: options.initial_prompt.clone(),
        };
        // Build the export sink here (UI thread) and wrap it in an `Arc` so the
        // F-063 translation callback can share the *same* instance — one setup,
        // one filename timestamp for both the transcript and its `.translated`
        // sibling (`LiveSessionRequest::export_sink`'s doc). Matches Buzz's own
        // GUI-thread `_init_export_settings`.
        let export_sink: Option<Arc<ExportSink>> =
            live_session::export_request_from_settings(&recording_settings).map(|cfg| {
                Arc::new(ExportSink::setup(
                    &cfg.folder,
                    cfg.file_type,
                    cfg.max_entries,
                    &options.line_separator,
                    mode,
                    cfg.template.as_deref(),
                    options.task,
                    options.language.map(|l| l.code()),
                    model.size.as_str(),
                    chrono::Local::now().naive_local(),
                ))
            });
        let upload_url = effective_upload_url(
            std::env::var("HUMM_UPLOAD_URL").ok().as_deref(),
            recording_settings.upload_url.as_deref(),
        );
        let session_name = live_session::live_session_name(chrono::Local::now().naive_local());

        // F-063 live-translation wiring: build the worker (and its enqueue
        // handle) iff translation is enabled, a model is configured, and an
        // endpoint resolves — otherwise record proceeds untranslated with a
        // warning (Buzz would construct a client with an empty key and fail
        // every request). The translation-panel text does **not** ride the
        // `SessionMessage` channel below: that channel dies with the session
        // (its receiver is dropped at Stop), which would silently swallow any
        // translation completing after Stop. Instead the worker posts panel
        // text into a dedicated channel the `LiveTranslator` owns, drained by
        // the recording window every frame it is open — see
        // `Self::build_live_translation` and `LiveTranslator::drain_panels`.
        let (session_tx, session_rx) = mpsc::channel::<SessionMessage>();
        let (translation_enqueue, live_translator, llm_snapshot) = self.build_live_translation(
            &options,
            mode,
            &recording_settings,
            export_sink.clone(),
            ctx,
        );

        let options_snapshot = humm_core::domain::TranscriptionOptions {
            language: options.language,
            task: options.task,
            model: model.clone(),
            word_timestamps: false,
            initial_prompt: options.initial_prompt.clone(),
            temperature: humm_core::domain::DEFAULT_TEMPERATURE.to_vec(),
            // Record what translation this session actually ran with (the same
            // checked+non-empty-model gate the file path uses), so the
            // persisted options honestly reflect the run.
            llm: llm_snapshot,
            // Speech isolation (F-027) is a file-transcription-only feature
            // (Buzz has no demucs pre-pass for live recording either) —
            // always off for a recording session's persisted snapshot.
            extract_speech: false,
        };

        let request = LiveSessionRequest {
            device_id,
            model,
            // `self.use_gpu` (env-only, cached at startup) AND a fresh
            // `settings.models.force_cpu` read (design spec §8.1) — see
            // `Self::force_cpu_setting`'s doc for why the two halves are
            // combined here rather than both cached.
            use_gpu: self.use_gpu && !self.force_cpu_setting(),
            live_options,
            session_config,
            separator: options.line_separator.clone(),
            hide_unconfirmed: recording_settings.hide_unconfirmed,
            options_snapshot,
            session_name,
            export_sink,
            upload_url,
            meter,
            translation_enqueue,
            api_config,
        };

        let session = live_session::spawn_live_session(
            Arc::clone(&self.recording_deps),
            request,
            session_tx,
            session_rx,
        );
        let mut vm = vm_arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        vm.session = Some(session);
        vm.live_translator = live_translator;
        ctx.request_repaint();
    }

    /// Builds this session's live-translation worker (F-063, spec §7.4),
    /// returning `(enqueue_handle, live_translator, llm_snapshot)`. All three
    /// are `None` when translation is off, the AI model is unconfigured (empty
    /// once trimmed — a warning is logged), or no LLM endpoint resolves (env →
    /// `settings.openai.base_url` → keyring — the same precedence
    /// [`crate::llm_source::GuiLlmSource`] and the viewer's Translate use); in
    /// every such case the recording simply proceeds untranslated.
    ///
    /// The worker's `on_result` callback runs on the translator's own worker
    /// thread (never the UI thread — M5's hard rule): for each arrived
    /// translation it applies the [`TranslationApplier`] (panel + `.translated`
    /// export), persists the raw translation onto the segment row when a row id
    /// is known, and posts the new translation-panel text into a dedicated
    /// panel channel the returned [`LiveTranslator`] owns (drained by the
    /// recording window via [`LiveTranslator::drain_panels`]). Routing the
    /// panel through the translator's own channel — not the session's
    /// `SessionMessage` channel, whose receiver is dropped at Stop — is what
    /// makes a translation completing *after* Stop still reach the panel while
    /// the window is open (F-063, spec §7.4). It only ever pushes into channels
    /// / does direct work — it **never** calls back into the translate control
    /// (the scripted seam invokes it synchronously from `enqueue`; re-entering
    /// the control there would deadlock).
    fn build_live_translation(
        &self,
        options: &crate::recording::RecordingOptions,
        mode: RecordingMode,
        recording_settings: &humm_core::settings::RecordingSettings,
        export_sink: Option<Arc<ExportSink>>,
        ctx: &egui::Context,
    ) -> (
        Option<crate::live_translation::TranslationEnqueue>,
        Option<LiveTranslator>,
        Option<humm_core::domain::LlmTranslationOptions>,
    ) {
        if !options.enable_llm_translation {
            return (None, None, None);
        }
        let model = options.llm_model.trim().to_string();
        if model.is_empty() {
            log::warn!(
                "live translation enabled but no AI model configured — recording without translation"
            );
            return (None, None, None);
        }

        let endpoint = {
            let env_base = std::env::var(ENV_BASE).ok();
            let env_key = std::env::var(ENV_KEY).ok();
            let settings_base = {
                let settings = self
                    .services
                    .settings
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                settings.openai.base_url.clone()
            };
            resolve_endpoint(
                env_base,
                env_key,
                &settings_base,
                self.services.secrets.as_ref(),
            )
        };
        let Some(endpoint) = endpoint else {
            log::warn!(
                "live translation enabled but no LLM endpoint resolvable (no key in env/keyring) — recording without translation"
            );
            return (None, None, None);
        };

        let prompt = options.llm_prompt.clone();
        let cfg = TranslatorConfig {
            auth: endpoint.auth,
            model: model.clone(),
            prompt: prompt.clone(),
        };

        // The applier + shared export sink + store handle + panel sender +
        // repaint handle the worker thread's `on_result` owns for the session's
        // lifetime. `TranslationApplier` is the translation-side twin of the
        // transcript `SessionApplier`, seeded with the same mode/separator/
        // hide-unconfirmed values (spec §7.4). `panel_tx`/`panel_rx` is the
        // dedicated translation-panel channel: the callback holds the sender,
        // the returned `LiveTranslator` owns the receiver (so it outlives the
        // session — see this method's doc and `LiveTranslator::drain_panels`).
        let mut applier = TranslationApplier::new(
            mode,
            options.line_separator.clone(),
            recording_settings.hide_unconfirmed,
        );
        let store = Arc::clone(&self.services.store);
        let (panel_tx, panel_rx) = mpsc::channel::<String>();
        let repaint_ctx = ctx.clone();
        let on_result: Box<dyn FnMut(String, Option<i64>) + Send> =
            Box::new(move |raw: String, id: Option<i64>| {
                // Runs on the translator worker thread. Only channel sends /
                // direct applier+store work here — never a call back into the
                // control (seam re-entrancy rule).
                let Some(panel) = applier.apply(&raw, export_sink.as_deref()) else {
                    // Empty translation: no store write, no panel (Buzz
                    // `on_next_translation:1038-1039`).
                    return;
                };
                if let Some(row_id) = id
                    && let Err(err) = store.update_segment_translation(row_id, &raw)
                {
                    log::warn!("live translation: store write failed for segment {row_id}: {err}");
                }
                let _ = panel_tx.send(panel);
                repaint_ctx.request_repaint();
            });

        let control = self.services.translate.spawn(cfg, on_result);
        let translator = LiveTranslator::new(control, panel_rx);
        let enqueue = translator.enqueue_handle();
        let llm_snapshot = humm_core::domain::LlmTranslationOptions { model, prompt };
        (Some(enqueue), Some(translator), Some(llm_snapshot))
    }

    /// Drains the active session's [`SessionMessage`] channel (non-blocking
    /// `try_iter`, mirroring `models_ui::poll_downloads`'s own per-frame poll
    /// shape) and applies each message — see `crate::live_session`'s module
    /// doc for what each variant does and doesn't carry (in particular: no
    /// store write happens here, only the one `TasksVm::refresh_row` *read*).
    /// Then drains the F-063 translation-panel channel — which is **not** gated
    /// on an active session: its receiver lives on the [`LiveTranslator`], which
    /// outlives the session (Stop drops the session and its `rx`, but not the
    /// translator), so a translation completing after Stop still updates the
    /// panel while the window is open (spec §7.4). Finally, regardless of
    /// whether any message arrived this frame, checks for a crashed session
    /// ([`Self::recover_crashed_session`]) — that check must not be gated on
    /// "some message arrived", since a crash is exactly the scenario where *no*
    /// message (in particular, no `Stopped`) will ever arrive again.
    fn drain_live_session(&mut self, ctx: &egui::Context) {
        let Some(vm_arc) = self.recording.clone() else {
            return;
        };
        let messages: Vec<SessionMessage> = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &vm.session {
                Some(session) => session.rx.try_iter().collect(),
                None => Vec::new(),
            }
        };
        if !messages.is_empty() {
            for message in messages {
                self.apply_session_message(&vm_arc, message);
            }
            ctx.request_repaint();
        }
        self.drain_translation_panel(&vm_arc, ctx);
        self.recover_crashed_session(&vm_arc, ctx);
    }

    /// Drains the live-translation panel channel and applies the latest
    /// whole-panel replace to [`RecordingVm::translation_panel_text`] (F-063).
    /// Independent of [`RecordingVm::session`] on purpose: the channel's
    /// receiver belongs to the [`LiveTranslator`] (`RecordingVm::live_translator`),
    /// which is torn down only at window close / next Record click — never at
    /// Stop. Draining it every frame the window is open is what lets a
    /// translation completing after the session's Stop (common with real
    /// network latency, the session tail) still reach the panel. Each drained
    /// item is a whole-panel replace already applied off-thread by the
    /// [`humm_core::live::translation::TranslationApplier`], so the last one
    /// this frame wins.
    fn drain_translation_panel(&self, vm_arc: &Arc<Mutex<RecordingVm>>, ctx: &egui::Context) {
        let latest = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &vm.live_translator {
                Some(translator) => translator.drain_panels().last(),
                None => None,
            }
        };
        if let Some(text) = latest {
            let mut vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.translation_panel_text = text;
            ctx.request_repaint();
        }
    }

    /// Applies one already-drained [`SessionMessage`] to `vm_arc`/app state.
    /// The single place either [`Self::drain_live_session`]'s normal
    /// per-frame poll *or* [`Self::recover_crashed_session`]'s own re-drain
    /// (a session thread that has finished can still have its final message
    /// sitting undrained — see that function's doc for why) applies a
    /// message, so the two callers can never diverge on what a given
    /// message means.
    fn apply_session_message(&mut self, vm_arc: &Arc<Mutex<RecordingVm>>, message: SessionMessage) {
        match message {
            SessionMessage::Preparing(update) => {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                vm.prep = Some(update);
            }
            SessionMessage::TableRefresh(id) => {
                {
                    let mut vm = vm_arc
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    // Idempotent: the same id arrives once at insert and
                    // once at completion. Exists only for
                    // `Self::recover_crashed_session` (a crashed session
                    // never reaches the completion send, so this is the
                    // only way that fallback learns which row to
                    // complete).
                    vm.session_id = Some(id);
                }
                self.tasks.refresh_row(id, &self.services.store);
            }
            SessionMessage::StoreWriteFailed { operation, detail } => {
                log::warn!("live session store write failed ({operation}): {detail}");
                self.action_state.banner.on_store_write_failed();
            }
            SessionMessage::Ready => {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                // Buzz `on_model_loaded` -> `reset_recording_controls()`:
                // clears the progress row once the model/capture are
                // actually ready, not on the first utterance (which
                // silence could delay indefinitely).
                vm.prep = None;
            }
            SessionMessage::PanelText(text) => {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                vm.panel_text = text;
            }
            SessionMessage::Error(cause) => {
                let mut vm = vm_arc
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                vm.error_dialog = Some(live_session::recording_error_dialog_text(&cause));
            }
            SessionMessage::Stopped { measurement } => {
                self.finish_live_session(vm_arc, measurement);
            }
        }
    }

    /// Liveness fallback for a session thread that panicked: normally the
    /// thread always sends exactly one `Stopped` before exiting
    /// (`live_session::run_live_session`'s own contract), which is what
    /// clears [`RecordingVm::session`]. If the thread's `JoinHandle` reports
    /// [`JoinHandle::is_finished`] while [`RecordingVm::session`] is still
    /// `Some` — i.e. it exited *without* ever sending `Stopped` — that
    /// contract was broken by a panic, not a normal return, and the window
    /// would otherwise be stranded in `Recording`/`Stopping` forever (the
    /// close veto never lifts, nothing ever restores the stopped-state UI).
    ///
    /// **Re-drain before declaring a crash (review fix — false-positive
    /// race).** [`Self::drain_live_session`]'s own `try_iter()` snapshot and
    /// this function's `is_finished()` check are two separate, unsynchronized
    /// reads a frame apart: a *healthy* session's very last `sink(Stopped)`
    /// call and its thread's subsequent return can both land in that gap —
    /// the message is sent but not yet in this frame's drained snapshot, and
    /// by the time this function checks, the thread has already exited. Read
    /// naively, that looks identical to a crash. The fix exploits a real
    /// ordering guarantee instead of guessing: `run_live_session` calls
    /// `sink(Stopped)` — which fully hands the message to the `mpsc` channel
    /// before returning — strictly before its enclosing thread function can
    /// return, which is strictly before the OS can mark the thread finished,
    /// which is strictly before *this* function (running on a different
    /// thread, at some later point) can observe [`JoinHandle::is_finished`]
    /// return `true`. So once `is_finished()` is `true` here, any message
    /// that thread ever sent — in particular its final `Stopped` — is
    /// *already* fully published to the channel and guaranteed visible to a
    /// `try_iter()` call made now, even though it wasn't yet visible to last
    /// frame's earlier snapshot. This function therefore re-drains once
    /// before concluding anything crashed; if that re-drain yields anything
    /// at all, it is applied through the exact same [`Self::apply_session_message`]
    /// every normal message goes through, and this function returns without
    /// ever touching `error_dialog` — a healthy stop must never be
    /// misreported as a crash (and must never lose that run's
    /// `Stopped{measurement}`, which is what feeds §2.8's measured-speed
    /// persistence in [`Self::finish_live_session`]). Only a genuinely empty
    /// re-drain — no message left behind at all — proceeds to the recovery
    /// below. See `app.rs`'s `crash_recovery_tests::redrain_wins_over_crash_declaration_for_a_finished_thread_with_a_queued_stopped`
    /// for the deterministic (non-racy) test of this exact branch.
    ///
    /// Recovery (only once the re-drain above found nothing): best-effort
    /// `complete_live_session` for whatever the session's own id turned out
    /// to be (Q-10 — a partial transcript is still finished work worth
    /// opening; ADR-018 surfaces a failure here the same way every other
    /// store write does), refresh its table row, surface a dedicated (not
    /// Buzz-parity — this is a humm-only failure mode Buzz's own threading
    /// model can't produce) error dialog, drop the stale `session` handle,
    /// and finish exactly like a normal `Stopped` would
    /// ([`Self::finish_live_session`]'s own `pending_close`-or-
    /// `finish_stopping` split) — which is what actually lifts the close
    /// veto.
    ///
    /// Note: a real `CpalCapture`'s audio stream has no owner left to stop
    /// it once its thread is gone — this fallback cannot clean that up, only
    /// recover the UI/store from the crash. Recorded as a known limitation
    /// (task report), not fixed here.
    fn recover_crashed_session(&mut self, vm_arc: &Arc<Mutex<RecordingVm>>, ctx: &egui::Context) {
        let finished = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &vm.session {
                Some(session) => session
                    .thread
                    .as_ref()
                    .is_some_and(std::thread::JoinHandle::is_finished),
                None => false,
            }
        };
        if !finished {
            return;
        }

        // Re-drain: see this function's own doc for why `is_finished()`
        // guarantees any last-moment message (in particular a healthy
        // session's final `Stopped`) is visible here even when it wasn't
        // yet visible to `Self::drain_live_session`'s own snapshot this
        // frame.
        let late_messages: Vec<SessionMessage> = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &vm.session {
                Some(session) => session.rx.try_iter().collect(),
                None => Vec::new(),
            }
        };
        if !late_messages.is_empty() {
            for message in late_messages {
                self.apply_session_message(vm_arc, message);
            }
            ctx.request_repaint();
            return;
        }

        let session_id = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.session_id
        };
        if let Some(id) = session_id {
            match self.recording_deps.store.complete_live_session(id) {
                Ok(()) => self.tasks.refresh_row(id, &self.services.store),
                Err(err) => {
                    log::warn!("crashed live session {id}: could not mark completed: {err}");
                    self.action_state.banner.on_store_write_failed();
                }
            }
        }

        let mut vm = vm_arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        vm.session = None;
        vm.error_dialog = Some(tr("recording-thread-crashed"));
        if vm.pending_close {
            vm.close_requested = true;
        } else {
            let deps = Arc::clone(&self.recording_deps);
            self.flush_finish_stopping(&mut vm, &deps);
        }
        ctx.request_repaint();
    }

    /// The session thread's terminal `Stopped` message (Buzz
    /// `on_transcriber_finished`/`on_transcriber_error` ->
    /// `set_recording_status_stopped`): persists this session's measured
    /// realtime ratio (spec §2.8, last-run-wins per model size — a save
    /// failure is logged, not fatal, same non-fatal-settings-write contract
    /// as [`Self::persist_layout_if_changed`]), drops the [`ActiveSession`]
    /// handle (detaching its thread — see the handle's own doc for why this
    /// is never a blocking `.join()`), then either finishes closing the
    /// window ([`RecordingVm::pending_close`] was set — a deferred close,
    /// `Self::show_recording_viewport`) or restores the ordinary
    /// stopped-state UI ([`RecordingVm::finish_stopping`]).
    ///
    /// [`ActiveSession`]: crate::live_session::ActiveSession
    fn finish_live_session(
        &mut self,
        vm_arc: &Arc<Mutex<RecordingVm>>,
        measurement: Option<humm_core::live::LiveMeasurement>,
    ) {
        let measured_key = {
            let vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            measured_speed_key(vm.options.backend, vm.options.model_size)
        };
        if let Some(ratio) = measurement.and_then(|m| m.realtime_ratio()) {
            let mut settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            live_session::record_measured_speed(
                &mut settings.recording.measured_speed,
                measured_key,
                ratio,
            );
            if let Err(err) = settings.save(&self.services.settings_path) {
                log::warn!("could not persist measured live-transcription speed: {err}");
            }
        }

        let mut vm = vm_arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        vm.session = None;
        if vm.pending_close {
            vm.close_requested = true;
        } else {
            let deps = Arc::clone(&self.recording_deps);
            self.flush_finish_stopping(&mut vm, &deps);
        }
    }

    /// Calls [`RecordingVm::finish_stopping`] and persists
    /// `settings.presentation` iff it actually flushed a pending debounced
    /// text-size/color edit (review fix — `finish_stopping` hides the
    /// presentation bar, the only place that normally drives that debounce,
    /// so a pending edit must be flushed right here or it's lost). Shared by
    /// both terminal-session paths that call `finish_stopping`
    /// ([`Self::finish_live_session`]'s healthy stop and
    /// [`Self::recover_crashed_session`]'s crash recovery) so the
    /// flush-then-maybe-save sequence is written once. `vm` is already the
    /// caller's own locked `RecordingVm` guard (not re-locked here); this
    /// method only locks `self.services.settings`, mirroring every other
    /// `RecordingVm`-then-`Settings` lock ordering in this file.
    fn flush_finish_stopping(&self, vm: &mut RecordingVm, deps: &RecordingDeps) {
        let mut settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let flushed = vm.finish_stopping(deps, &mut settings.presentation);
        if flushed && let Err(err) = settings.save(&self.services.settings_path) {
            log::warn!("could not persist a pending presentation-settings edit: {err}");
        }
    }

    /// Persists `geometry` as the shared viewer geometry
    /// (`settings.viewer.geometry`) if it differs from what is stored — the
    /// same diff-on-change, non-fatal-on-save-failure contract as
    /// [`Self::persist_layout_if_changed`]. `None` (the closure never observed
    /// a geometry) is a no-op.
    fn persist_viewer_geometry_if_changed(&mut self, geometry: Option<ViewerGeometry>) {
        let Some(geometry) = geometry else {
            return;
        };
        let mut settings = self
            .services
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if settings.viewer.geometry == Some(geometry) {
            return;
        }
        settings.viewer.geometry = Some(geometry);
        if let Err(err) = settings.save(&self.services.settings_path) {
            log::warn!("could not persist viewer geometry: {err}");
        }
    }

    /// Drains [`Self::app_event_rx`] and applies every [`AppEvent`] (design
    /// spec §1.2): the merge worker's completion event refreshes/inserts the
    /// affected table row and closes the source's viewer + resizer through the
    /// geometry-persisting close path (the watch-item fix).
    fn drain_app_events(&mut self) {
        let events: Vec<AppEvent> = self.app_event_rx.try_iter().collect();
        for event in events {
            match event {
                AppEvent::TranscriptionsUpdated {
                    affected,
                    source,
                    close_source,
                } => {
                    self.apply_transcriptions_updated(affected, source, close_source);
                }
            }
        }
    }

    /// A clone of the GUI-internal [`AppEvent`] sender, to hand to a background
    /// worker (the resize/merge worker). Keeps [`Self::app_event_tx`] a live
    /// producer endpoint that other spawn sites can reuse.
    pub fn app_event_sender(&self) -> mpsc::Sender<AppEvent> {
        self.app_event_tx.clone()
    }

    /// Rebuilds [`Self::shortcuts`] from the live `settings.shortcuts`
    /// overrides when [`Self::shortcuts_generation`] has moved since the last
    /// rebuild (the shortcuts-editor tab's write path bumps it on every
    /// commit — `views/prefs_shortcuts.rs`'s module doc has the full
    /// handshake). A no-op most frames (the common case: nothing changed) —
    /// one atomic load plus an integer compare. Called once per frame, before
    /// anything renders, so every consumer that reads `self.shortcuts` this
    /// frame (the main window directly, viewers/resizers via their own
    /// per-frame `Arc::clone`) already sees the rebuilt map, not a
    /// stale one.
    fn rebuild_shortcuts_if_generation_changed(&mut self) {
        let current_generation = self.shortcuts_generation.load(Ordering::Relaxed);
        if current_generation == self.last_shortcuts_generation {
            return;
        }
        self.last_shortcuts_generation = current_generation;
        let overrides = {
            let guard = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.shortcuts.clone()
        };
        self.shortcuts = Arc::new(ShortcutMap::from_settings(&overrides));
    }

    // -- Test seams (F-086 resizer) -----------------------------------------
    // The resizer window is a deferred OS viewport an `egui_kittest` Harness
    // cannot reach into, so these plain `pub fn`s (the `new_for_test`/
    // `open_options_sheet_for_test` pattern) let an integration test drive the
    // open → action → close flow directly against a shared store.

    /// Opens (or focuses) a viewer for `id` — the originating window a resizer
    /// closes on completion.
    pub fn open_viewer_for_test(&mut self, id: TranscriptionId) {
        self.open_viewer(id, Instant::now());
    }

    /// Opens (or focuses) a resizer window for `source`.
    pub fn open_resizer_for_test(&mut self, source: TranscriptionId) {
        self.open_resizer(source);
    }

    /// The live resizer view-model for `source`, so a test can set its form
    /// fields (`create_new`, `target_chars`, …) before driving an action.
    pub fn resizer_for_test(&self, source: TranscriptionId) -> Option<Arc<Mutex<ResizerVm>>> {
        self.resizers.get(&source).map(Arc::clone)
    }

    /// Runs one resizer action as the harvest would, returning the affected id
    /// for the synchronous (Extend/Resize) paths (`None` for Merge).
    pub fn run_resizer_action_for_test(
        &mut self,
        source: TranscriptionId,
        action: ResizerAction,
    ) -> Option<TranscriptionId> {
        self.apply_resizer_action(source, action, Instant::now(), &egui::Context::default())
    }

    /// Whether a viewer window is open for `id`.
    pub fn is_viewer_open_for_test(&self, id: TranscriptionId) -> bool {
        self.viewers.contains_key(&id)
    }

    /// Whether a resizer window is open for `source`.
    pub fn is_resizer_open_for_test(&self, source: TranscriptionId) -> bool {
        self.resizers.contains_key(&source)
    }

    /// Whether the task table's view-model currently has a row for `id`.
    pub fn has_task_row_for_test(&self, id: TranscriptionId) -> bool {
        self.tasks.sorted_rows().iter().any(|row| row.id == id)
    }

    // -- Test seams (F-087 Speaker-ID window) --------------------------------
    // The Speaker-ID window is a deferred OS viewport an `egui_kittest`
    // Harness cannot reach into, so these plain `pub fn`s (the resizer's own
    // precedent) let an integration test drive open → Identify/Cancel → Save
    // → close directly. Identify/Cancel/poll themselves need no seam here —
    // `SpeakerIdVm`'s own methods are already public, so a test drives them
    // straight off the `Arc<Mutex<SpeakerIdVm>>` [`Self::speaker_vm_for_test`]
    // returns.

    /// Opens (or focuses) a Speaker-ID window for `source` — the originating
    /// viewer a save closes only via the shared table refresh, never the
    /// viewer itself (`close_source: false`, this module's `AppEvent` doc).
    pub fn open_speakers_for_test(&mut self, source: TranscriptionId) {
        self.open_speakers(source);
    }

    /// The live Speaker-ID view-model for `source`, so a test can drive
    /// `start_identify`/`cancel_identify`/`poll_identify` directly, or set
    /// `Ready`'s `names`/`merge` before saving.
    pub fn speaker_vm_for_test(&self, source: TranscriptionId) -> Option<Arc<Mutex<SpeakerIdVm>>> {
        self.speakers.get(&source).map(Arc::clone)
    }

    /// Whether a Speaker-ID window is open for `source`.
    pub fn is_speakers_open_for_test(&self, source: TranscriptionId) -> bool {
        self.speakers.contains_key(&source)
    }

    /// Runs the Save harvest for `source`'s open Speaker-ID window exactly as
    /// [`Self::show_speaker_viewports`] would on a real click — labels +
    /// copies + completes, applies `TranscriptionsUpdated { close_source:
    /// false, .. }` on success (closing this window and returning the new
    /// transcript's id), or records the failure on `save_error_for_test`
    /// (leaving the window open, returning `None`) on failure.
    pub fn run_speaker_save_for_test(
        &mut self,
        source: TranscriptionId,
    ) -> Option<TranscriptionId> {
        self.apply_speaker_save(source)
    }

    /// The app's currently-resolved shortcut map (Task 14) — so a test can
    /// assert `settings.shortcuts` overrides were actually seeded at
    /// startup, or that a shortcuts-editor commit's generation bump was
    /// picked up by [`Self::rebuild_shortcuts_if_generation_changed`] on the
    /// next `ui()` call.
    pub fn shortcuts_for_test(&self) -> Arc<ShortcutMap> {
        Arc::clone(&self.shortcuts)
    }

    // -- Test seams (F-060 recording window) ---------------------------------
    // The recording window is a deferred OS viewport an `egui_kittest` Harness
    // cannot reach into (the viewer/resizer windows' own constraint); these
    // seams let an integration test drive open/focus/close without touching
    // real OS viewport plumbing. `HummApp::new_for_test` always wires a
    // `MockCapture` (never `CpalCapture`), so exercising these seams never
    // touches real audio hardware.

    /// Opens (or focuses) the live-recording window — the same effect the
    /// toolbar Record button / Ctrl+R has once `action_state.record_requested`
    /// is drained.
    pub fn open_recording_window_for_test(&mut self) {
        self.open_recording_window();
    }

    /// The live recording view-model, so a test can drive its Record/Stop
    /// click or read its state directly. `None` while the window is closed.
    pub fn recording_vm_for_test(&self) -> Option<Arc<Mutex<RecordingVm>>> {
        self.recording.as_ref().map(Arc::clone)
    }

    /// Whether the recording window is currently open.
    pub fn is_recording_window_open_for_test(&self) -> bool {
        self.recording.is_some()
    }

    /// The recording window's adapter bundle (Task 11) — so a scripted
    /// end-to-end test can drive `RecordingVm::on_record_clicked` directly
    /// with the exact same `RecordingDeps` `app.rs`'s own harvest uses (the
    /// recording window's own Record/Stop button lives inside a deferred
    /// viewport a kittest `Harness` cannot click; see the module doc).
    pub fn recording_deps_for_test(&self) -> Arc<RecordingDeps> {
        Arc::clone(&self.recording_deps)
    }

    /// The task table's own row for `id`, as the running app currently
    /// holds it (`self.tasks`, kept live by [`Self::drain_live_session`]'s
    /// `SessionMessage::TableRefresh` handling — *not* a fresh read from the
    /// store) — so a test can assert the live-session `TableRefresh` wiring
    /// itself, rather than only ever re-deriving `TasksVm` from the store
    /// independently of the running app.
    pub fn tasks_row_for_test(&self, id: TranscriptionId) -> Option<crate::viewmodel::RowVm> {
        self.tasks
            .sorted_rows()
            .iter()
            .find(|row| row.id == id)
            .cloned()
    }
}

/// Buzz's Restart gate (`transcription_tasks_table_widget.py:696-725`):
/// only failed/canceled/skipped rows may be retried — a completed row is
/// never restartable. Pure and free of any `HummApp` state so the gate
/// itself is directly unit-testable, independent of
/// [`HummApp::handle_retry`]'s store/queue plumbing.
fn retry_allowed(status: Status) -> bool {
    matches!(status, Status::Failed | Status::Canceled | Status::Skipped)
}

/// Whether a drained [`TaskEventKind`] should reload an open viewer's
/// segments (design spec §2.3: the auto-resizer hook can replace a task's
/// persisted segments after transcription completes, and an already-open
/// viewer must not keep showing the stale, pre-resize rows). Pure and free
/// of any `HummApp` state — `egui_kittest` cannot drive a real deferred
/// viewport, so this is the seam that makes the *decision* testable without
/// one; [`HummApp::reload_viewer_segments_if_open`] is the (untested here)
/// side-effecting half that actually touches a `ViewerVm` when this returns
/// `true`.
fn postprocessed_should_reload_viewer(kind: &TaskEventKind) -> bool {
    matches!(
        kind,
        TaskEventKind::PostProcessed {
            segments_replaced: true,
            ..
        }
    )
}

/// `(ui.locale, ui.font_size)`, the two settings a Preferences edit can
/// change that every other open window must repaint to reflect
/// ([`HummApp::show_preferences_viewport`]'s own doc) — read under one lock
/// so the pair is a consistent snapshot from a single point in time.
fn ui_appearance_snapshot(settings: &Mutex<Settings>) -> (Option<String>, Option<u32>) {
    let guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (guard.ui.locale.clone(), guard.ui.font_size)
}

/// Whether two [`ui_appearance_snapshot`] values differ — pure and free of
/// any `HummApp`/`egui::Context` state so the decision itself is directly
/// unit-testable, independent of [`HummApp::show_preferences_viewport`]'s
/// viewport plumbing (mirrors [`retry_allowed`]/
/// [`postprocessed_should_reload_viewer`]'s own "pure decision, `HummApp`
/// only acts on it" split above).
fn ui_appearance_changed(
    before: &(Option<String>, Option<u32>),
    after: &(Option<String>, Option<u32>),
) -> bool {
    before != after
}

/// The [`egui::ViewportBuilder`] for a viewer window: `title`, min size
/// 800×500, and either the captured shared geometry (`geometry`, position
/// applied only when both coordinates are known) or the 1000×800 default
/// (design spec §1.1/§6, `transcription_viewer_widget.py:92-93,1522-1531`).
///
/// `geometry` is the per-VM value captured at open ([`ViewerVm::initial_geometry`]),
/// not a live settings read: a constant-per-window builder means egui only
/// sizes the window once, so a *later* viewer closing (which rewrites the
/// shared geometry) never re-snaps this already-open window.
fn viewer_viewport_builder(title: &str, geometry: Option<ViewerGeometry>) -> egui::ViewportBuilder {
    let mut builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_min_inner_size([800.0, 500.0]);
    match geometry {
        Some(geometry) => {
            builder = builder.with_inner_size([geometry.w, geometry.h]);
            if let (Some(x), Some(y)) = (geometry.x, geometry.y) {
                builder = builder.with_position([x, y]);
            }
        }
        None => {
            builder = builder.with_inner_size([1000.0, 800.0]);
        }
    }
    builder
}

/// The [`egui::ViewportBuilder`] for a resizer window: `title` (the source
/// file's basename), min size 600×300 (Buzz `setMinimumWidth(600)` /
/// `setMinimumHeight(300)`), default size 600×480. Unlike the viewer, the
/// resizer persists no geometry (Buzz's `TranscriptionResizerWidget` has no
/// `load_geometry`/`save_geometry`).
fn resizer_viewport_builder(title: &str) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_min_inner_size([600.0, 300.0])
        .with_inner_size([600.0, 480.0])
}

/// The [`egui::ViewportBuilder`] for a Speaker-ID window: `title` (the
/// source file's basename), min size 650×400 (Buzz `setMinimumWidth(650)` /
/// `setMinimumHeight(400)`, `speaker_identification_widget.py:475-476`),
/// default size 650×520 (a little taller than the minimum so Step 1, Step 2,
/// and Save all fit without scrolling for a typical few-speaker result).
fn speaker_viewport_builder(title: &str) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_min_inner_size([650.0, 400.0])
        .with_inner_size([650.0, 520.0])
}

/// [`Self::new_for_test`]'s default `recording_deps.model_ensurer` — every
/// call fails actionably (never touches the network) so a kittest file that
/// never scripts a live session (nearly all of them) is unaffected, and one
/// that clicks Record without first calling
/// [`HummApp::set_recording_deps_for_test`] gets a clear, actionable error
/// instead of silently reaching the real network (Global Constraint 3).
struct UnwiredModelEnsurer;

impl ArtifactEnsurer for UnwiredModelEnsurer {
    fn ensure_model(
        &self,
        _model: &ModelSpec,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Result<PathBuf, CoreError> {
        Err(CoreError::Engine(EngineError::ModelLoad {
            path: PathBuf::from("(recording session not wired for this test)"),
            detail: "call HummApp::set_recording_deps_for_test to script a live session"
                .to_string(),
        }))
    }

    fn ensure_vad(
        &self,
        _progress: &mut dyn FnMut(DownloadProgress),
        _cancel: &CancelToken,
    ) -> Option<PathBuf> {
        None
    }
}

/// [`Self::new_for_test`]'s default `recording_deps.engine_factory` — see
/// [`UnwiredModelEnsurer`]'s doc for the rationale (same "actionable error,
/// never touches anything real" contract; engine load never reaches this
/// stand-in unless a model somehow resolved without a scripted
/// `model_ensurer`, which is itself unreachable by construction here).
struct UnwiredLiveEngineFactory;

impl LiveEngineFactory for UnwiredLiveEngineFactory {
    fn load(
        &self,
        _model_path: &Path,
        _use_gpu: bool,
    ) -> Result<Box<dyn LiveEngineSession>, EngineError> {
        Err(EngineError::ModelLoad {
            path: PathBuf::from("(recording session not wired for this test)"),
            detail: "call HummApp::set_recording_deps_for_test to script a live session"
                .to_string(),
        })
    }

    /// Overrides [`LiveEngineFactory::load_api`]'s default `unreachable!`
    /// panic (review fix, task 18) with the same actionable-error contract
    /// `load` above already gives: rather than a hard panic, a test that
    /// somehow reaches the API backend without scripting
    /// `HummApp::set_recording_deps_for_test` gets a clean session error
    /// (`SessionMessage::Error` + `Stopped`) carrying the identical message.
    /// `load_api` itself cannot fail (no `Result`), so the failure is
    /// deferred one step, onto the returned stand-in session's first
    /// `transcribe_live` call.
    fn load_api(&self, _cfg: OpenAiEngineConfig) -> Box<dyn LiveEngineSession> {
        Box::new(UnwiredApiEngineSession)
    }
}

/// [`UnwiredLiveEngineFactory::load_api`]'s stand-in session — see that
/// method's own doc for why this exists instead of inheriting the trait's
/// default panic.
struct UnwiredApiEngineSession;

impl LiveEngineSession for UnwiredApiEngineSession {
    fn transcribe_live(
        &mut self,
        _samples: &[f32],
        _opts: &LiveOptions,
        _prompt: &str,
        _cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        Err(EngineError::ModelLoad {
            path: PathBuf::from("(recording session not wired for this test)"),
            detail: "call HummApp::set_recording_deps_for_test to script a live session"
                .to_string(),
        })
    }
}

/// [`Self::new_for_test`]'s default `recording_deps.upload_factory` — unlike
/// [`UnwiredModelEnsurer`]/[`UnwiredLiveEngineFactory`], [`UploadFactory::build`]
/// cannot return an error (upload failures are logged-and-swallowed by
/// design, `humm_core::live::UploadClient`'s own contract), and it is only
/// ever reached when a session's *effective* upload URL is `Some` — never
/// true by default (`Settings::default()`'s `recording.upload_url` is
/// `None`, and `HUMM_UPLOAD_URL` is never set in this workspace's test
/// environment) — so panicking here is the same "prove no test in this
/// workspace accidentally reaches it" contract
/// `tests/kittest_recording.rs`'s own `NeverCalledEngineFactory` already
/// uses for the *queue's* engine factory.
struct UnwiredUploadFactory;

impl UploadFactory for UnwiredUploadFactory {
    fn build(&self, _url: String) -> Box<dyn LiveUpload> {
        unreachable!(
            "recording upload requested without HummApp::set_recording_deps_for_test — no \
             test's settings configure an upload_url by default"
        )
    }
}

/// Default `HUMM_SMOKE_LIVE_FIXTURE` recording window, used whenever
/// `HUMM_SMOKE_LIVE_SECONDS` is unset/invalid: long enough to be a genuine
/// "windowed multi-minute run" (spec §8 acceptance item 1) and, at the
/// fixture pacing thread's real wall-clock rate, to loop past a ~2-minute
/// fixture (Buzz's `audio-long.mp3`) at least once.
const DEFAULT_LIVE_FIXTURE_SECONDS: u32 = 200;

/// Environment-gated smoke-test hooks (see the `smoke_hooks` field doc on
/// [`HummApp`]): parsed once, at construction, from
/// `HUMM_SMOKE_OPEN_WINDOWS`/`HUMM_SMOKE_AUTO_CLOSE_FRAMES`/
/// `HUMM_SMOKE_OPEN_VIEWER`/`HUMM_SMOKE_LIVE_FIXTURE`/
/// `HUMM_SMOKE_LIVE_SECONDS`. Exist solely so the `gui-manual` windowed
/// smokes (`docs/gui-manual/M3.md` item 35, `docs/gui-manual/M4.md`'s
/// A/V-drift run, `docs/gui-manual/M5.md`'s live realtime-fit run) can run
/// without OS input injection — test-only, unstable, documented as such in
/// `docs/HUMM_SURFACE.md` §5.1.
///
/// See also [`SmokeLiveFixtureState`], the small state machine
/// [`HummApp::apply_smoke_live_fixture_tick`] drives once `live_fixture`
/// below is `Some`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SmokeHooks {
    /// `HUMM_SMOKE_OPEN_WINDOWS` was set to any non-empty value.
    open_windows: bool,
    /// `HUMM_SMOKE_AUTO_CLOSE_FRAMES` parsed as a positive `u32`; `None`
    /// when the var is unset, empty, zero, or fails to parse as a `u32`.
    auto_close_frames: Option<u32>,
    /// `HUMM_SMOKE_OPEN_VIEWER` = a media path: on the first frame, seed a
    /// completed transcript row for it, open its viewer, arm autoplay, and
    /// close the app once playback finishes (the A/V-drift acceptance run,
    /// F-083). `None` when unset or empty.
    open_viewer: Option<PathBuf>,
    /// `HUMM_SMOKE_LIVE_FIXTURE` = a media path: the live realtime-fit
    /// acceptance hook (spec §8 item 1, M5 Task 14). `None` when unset,
    /// empty, **or** the path failed to decode at construction (`Self::new`
    /// clears it on a decode error rather than leaving a dangling hook that
    /// would never fire) — so by the time [`HummApp::apply_smoke_live_fixture_tick`]
    /// ever sees `Some`, the paired `FixtureCapture` is already wired into
    /// `recording_deps.capture` with real, decoded samples.
    live_fixture: Option<PathBuf>,
    /// `HUMM_SMOKE_LIVE_SECONDS` parsed as a positive `u32`, the recording
    /// window's duration before the hook auto-stops it —
    /// [`DEFAULT_LIVE_FIXTURE_SECONDS`] when unset, empty, zero, or
    /// unparseable. Only consulted when `live_fixture` is `Some`.
    live_fixture_seconds: u32,
}

/// Pure parse of the five smoke-hook env values into a [`SmokeHooks`]. Takes
/// `Option<&str>` (the already-`std::env::var(..).ok()`'d value) rather than
/// reading the environment itself, so it's directly unit-testable without
/// mutating real process-global env state.
fn smoke_hooks_from_env(
    open: Option<&str>,
    close: Option<&str>,
    open_viewer: Option<&str>,
    live_fixture: Option<&str>,
    live_fixture_seconds: Option<&str>,
) -> SmokeHooks {
    let open_windows = open.is_some_and(|value| !value.is_empty());
    let auto_close_frames = close
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|&frames| frames > 0);
    let open_viewer = open_viewer
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let live_fixture = live_fixture
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let live_fixture_seconds = live_fixture_seconds
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|&secs| secs > 0)
        .unwrap_or(DEFAULT_LIVE_FIXTURE_SECONDS);
    SmokeHooks {
        open_windows,
        auto_close_frames,
        open_viewer,
        live_fixture,
        live_fixture_seconds,
    }
}

/// The `HUMM_SMOKE_LIVE_FIXTURE` hook's own progress
/// ([`HummApp::apply_smoke_live_fixture_tick`]): `started` is when
/// `on_record_clicked` was called (Record), so the tick knows when
/// `smoke_hooks.live_fixture_seconds` has elapsed; `stop_requested` guards
/// against latching a second Stop intent while waiting for the session's
/// `Stopped` acknowledgement. `Copy` (both fields are) so the tick can read
/// it out of `Option<SmokeLiveFixtureState>` without borrowing `self`.
#[derive(Debug, Clone, Copy)]
struct SmokeLiveFixtureState {
    started: Instant,
    stop_requested: bool,
}

/// Builds a minimal file-import [`humm_core::domain::TaskSpec`] for the
/// `HUMM_SMOKE_OPEN_VIEWER` media path — default options, no export formats;
/// only the input path is load-bearing (the viewer decodes it for playback).
/// Test-only (see [`HummApp::apply_smoke_open_viewer_once`]).
fn smoke_viewer_spec(path: &Path) -> humm_core::domain::TaskSpec {
    humm_core::domain::TaskSpec {
        input: humm_core::domain::Input::Path(path.to_path_buf()),
        options: humm_core::domain::TranscriptionOptions::default(),
        output: humm_core::domain::OutputSpec::default(),
    }
}

/// Placeholder segments spanning ~10 min so the `HUMM_SMOKE_OPEN_VIEWER` viewer
/// renders a non-empty editor table (and, for video, the video pane) in
/// Timestamps mode — the A/V-drift acceptance run measures playback sync, not
/// transcript content, so the exact text/timings are irrelevant. Test-only.
fn smoke_viewer_segments() -> Vec<Segment> {
    (0..20)
        .map(|i| {
            let start = i * 30_000;
            Segment {
                start: humm_core::domain::Millis(start),
                end: humm_core::domain::Millis(start + 29_000),
                text: format!("smoke segment {i}"),
                translation: String::new(),
            }
        })
        .collect()
}

impl eframe::App for HummApp {
    /// eframe 0.35's per-frame entry point: `App::ui` (not the classic
    /// `App::update`), handed the root viewport's `egui::Ui` directly rather
    /// than a bare `&egui::Context` — `egui::Ui::ctx()` gets the `Context`
    /// back when something (like the event pump) needs to own a clone of it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.event_queue.is_none() {
            self.event_queue = Some(events::spawn_event_pump(
                self.services.events.clone(),
                ui.ctx().clone(),
            ));
        }

        self.rebuild_shortcuts_if_generation_changed();
        self.fire_telemetry_once();
        self.apply_smoke_open_windows_once();
        self.apply_smoke_open_viewer_once(Instant::now());
        self.apply_smoke_auto_close(ui.ctx());
        self.apply_smoke_viewer_tick(ui.ctx());
        self.apply_smoke_live_fixture_tick(ui.ctx());
        self.drain_pending_argv_import();
        self.drain_update_notices();
        self.drain_app_events();
        self.sync_folder_watch(ui.ctx());

        let now = Instant::now();
        if let Some(event_queue) = &self.event_queue {
            for event in events::drain(event_queue) {
                self.tasks.apply(&event, &self.services.store);
                self.react_to_event(&event, now);
            }
        }

        let custom_model_available = self.custom_model_available();
        let include_lumii = self.lumii_gate();
        let yt_dlp_available = self.services.capabilities.yt_dlp;
        let demucs_available = self.services.capabilities.demucs;
        let folder_watch_pill = self.folder_watch_pill();

        let result = views::main_window::show(
            ui,
            &mut self.tasks,
            &mut self.table_layout,
            &mut self.pending_table_actions,
            &self.shortcuts,
            &mut self.action_state,
            now,
            custom_model_available,
            include_lumii,
            yt_dlp_available,
            demucs_available,
            CURRENT_VERSION,
            &self.services.secrets,
            folder_watch_pill.as_ref(),
        );

        if let Some(outcome) = result.dialog_outcome {
            self.apply_dialog_outcome(outcome, now);
        }
        if result.clear_confirmed {
            self.apply_clear_confirmed(now);
        }
        for action in result.toast_actions {
            self.apply_toast_action(action, now);
        }
        if let Some(outcome) = result.sheet_outcome {
            self.apply_sheet_outcome(outcome);
        }
        if let Some(url) = result.url_dialog_result {
            self.open_options_sheet(ImportRequest::Url(url));
        }
        if result.folder_watch_pause_toggled {
            self.toggle_folder_watch_pause();
        }

        self.drain_table_actions(now);
        self.drain_pending_import();
        self.drain_dropped_files(ui.ctx());

        // Multi-window flows (Task 8's `about_requested`/`preferences_requested`
        // stubs, wired here): a click this frame opens the corresponding real
        // OS viewport; the flag itself is one-shot (reset immediately) so it
        // never re-triggers on a later frame while the window stays open.
        if self.action_state.about_requested {
            self.action_state.about_requested = false;
            self.about_open = true;
        }
        if self.action_state.preferences_requested {
            self.action_state.preferences_requested = false;
            // Only (re)builds state on an actual open — re-triggering ⌘,/the
            // menu item while the window is already open must not clobber
            // live state (an in-flight download's `Downloading` row, a
            // half-typed custom URL, an open delete-confirm modal).
            if !self.preferences_open {
                let settings_snapshot = self.settings_snapshot();
                self.preferences_state = Some(PreferencesState::new(
                    &self.services.model_root,
                    self.custom_model_url().as_deref(),
                    &self.services.secrets,
                    &settings_snapshot,
                ));
            }
            self.preferences_open = true;
        }
        if self.action_state.record_requested {
            self.action_state.record_requested = false;
            self.open_recording_window();
        }
        if self.about_open {
            self.show_about_viewport(ui.ctx());
        }
        if self.preferences_open {
            self.show_preferences_viewport(ui.ctx());
        }
        self.show_viewer_viewports(ui.ctx());
        self.show_resizer_viewports(ui.ctx(), now);
        self.show_speaker_viewports(ui.ctx());
        self.show_recording_viewport(ui.ctx());
        self.show_presentation_viewport(ui.ctx());
        self.harvest_recording_intent(ui.ctx());
        self.drain_live_session(ui.ctx());
        self.harvest_recording_close();

        self.persist_layout_if_changed();
    }

    /// Called once on shutdown (`eframe::App::on_exit`, non-`glow` build —
    /// `humm-gui` doesn't enable the `glow` feature, so this is the
    /// zero-argument signature). Stops the folder-watch service **before**
    /// touching the queue (Buzz's own teardown order,
    /// `main_window.py:500-507`: the folder watcher is disconnected before
    /// `closeEvent` does anything else) — this also drops that service's own
    /// `Arc<QueueHandle>` clone, which is why the `Arc::try_unwrap` below can
    /// actually succeed. Then performs the graceful, bounded shutdown
    /// `services.rs`'s `Services` struct doc documents: cancel every in-
    /// flight/pending task, then wait — up to [`SHUTDOWN_DRAIN_TIMEOUT`]
    /// (ADR-022) — for the runner to actually drain and stop, so each
    /// canceled task's terminal store write lands before the process exits
    /// rather than being cut off mid-write by a bare process kill. A no-op
    /// if the queue was somehow already taken (defensive only — `on_exit`
    /// fires at most once per process).
    fn on_exit(&mut self) {
        if let Some(service) = self.folder_watch.take() {
            service.stop();
        }
        self.folder_watch_snapshot = None;
        if let Some(queue) = self.queue.take() {
            match Arc::try_unwrap(queue) {
                Ok(queue) => shutdown_queue(queue),
                Err(_still_shared) => log::error!(
                    "queue handle was still shared at shutdown (folder-watch service not fully \
                     stopped?) — skipping the graceful drain"
                ),
            }
        }
    }
}

/// The GUI shutdown timeout for [`shutdown_queue`]'s `finish_within` call
/// (ADR-022, design spec §8) — belt-and-braces only: cancellation is prompt
/// by design, so 5 s is ample margin for a well-behaved runner to actually
/// drain, not a budget shutdown expects to spend.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// The shutdown sequence itself, factored out of [`HummApp::on_exit`] so it
/// has its own directly callable, directly testable seam: `cancel_all()`
/// trips every in-flight/pending task's cancel token and marks every pending
/// task canceled without ever starting it; `finish_within` then signals "no
/// more enqueues" and blocks — up to [`SHUTDOWN_DRAIN_TIMEOUT`] — until the
/// runner thread has drained (including writing the now-canceled in-flight
/// task's terminal store row) and stopped.
///
/// This is `finish_within`, not the unbounded `finish()`: ADR-022's
/// belt-and-braces for app exit. Ordinary cancellation is prompt (a task's
/// cancel token is checked cooperatively at every phase boundary, and every
/// blocking `openai::*` HTTP call cancels in under a second via
/// `cancelable`'s request-thread detach — design spec §8), so the drain
/// should finish almost immediately; the timeout exists only to guarantee
/// process exit is never held hostage by an engine bug that ignores
/// cancellation. `Ok(true)` (drained in time) is silent, same as `finish()`
/// used to be; `Ok(false)` (still running at the deadline) is logged — the
/// runner thread is deliberately leaked at that point (see
/// `QueueHandle::finish_within`'s own doc comment for why leaking is
/// sound) and the process exits anyway.
fn shutdown_queue(queue: QueueHandle) {
    queue.cancel_all();
    match queue.finish_within(SHUTDOWN_DRAIN_TIMEOUT) {
        Ok(true) => {}
        Ok(false) => log::error!(
            "queue did not drain within {} s at shutdown — exiting anyway",
            SHUTDOWN_DRAIN_TIMEOUT.as_secs()
        ),
        Err(err) => log::error!("queue shutdown sequence failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_allowed_only_for_failed_canceled_skipped() {
        assert!(retry_allowed(Status::Failed));
        assert!(retry_allowed(Status::Canceled));
        assert!(retry_allowed(Status::Skipped));
        assert!(!retry_allowed(Status::Completed));
        assert!(!retry_allowed(Status::Queued));
        assert!(!retry_allowed(Status::InProgress));
    }

    #[test]
    fn postprocessed_should_reload_viewer_only_when_segments_replaced() {
        assert!(postprocessed_should_reload_viewer(
            &TaskEventKind::PostProcessed {
                notes_updated: false,
                segments_replaced: true,
                written: vec![],
                errors: vec![],
            }
        ));
        assert!(!postprocessed_should_reload_viewer(
            &TaskEventKind::PostProcessed {
                notes_updated: true,
                segments_replaced: false,
                written: vec![PathBuf::from("/tmp/x.docx")],
                errors: vec!["boom".to_string()],
            }
        ));
    }

    #[test]
    fn postprocessed_should_reload_viewer_ignores_other_event_kinds() {
        assert!(!postprocessed_should_reload_viewer(&TaskEventKind::Started));
        assert!(!postprocessed_should_reload_viewer(&TaskEventKind::Skipped));
        assert!(!postprocessed_should_reload_viewer(
            &TaskEventKind::Completed { written: vec![] }
        ));
    }

    // -- ui_appearance_changed / ui_appearance_snapshot (repaint-gap fix) --------

    #[test]
    fn ui_appearance_changed_is_false_for_an_identical_snapshot() {
        let snap = (Some("de_DE".to_string()), Some(20));
        assert!(!ui_appearance_changed(&snap, &snap.clone()));
        let none_snap = (None, None);
        assert!(!ui_appearance_changed(&none_snap, &none_snap));
    }

    #[test]
    fn ui_appearance_changed_true_when_locale_differs() {
        let before = (Some("en_US".to_string()), Some(14));
        let after = (Some("de_DE".to_string()), Some(14));
        assert!(ui_appearance_changed(&before, &after));
    }

    #[test]
    fn ui_appearance_changed_true_when_font_size_differs() {
        let before = (None, Some(14));
        let after = (None, Some(20));
        assert!(ui_appearance_changed(&before, &after));
    }

    #[test]
    fn ui_appearance_changed_true_when_font_size_toggles_custom_off() {
        // Unchecking "Custom font size" clears the setting to `None` --
        // this must count as a change too, not just a numeric edit.
        let before = (None, Some(20));
        let after = (None, None);
        assert!(ui_appearance_changed(&before, &after));
    }

    #[test]
    fn ui_appearance_snapshot_reads_both_fields_under_one_lock() {
        let mut settings = Settings::default();
        settings.ui.locale = Some("es_ES".to_string());
        settings.ui.font_size = Some(18);
        let settings = Mutex::new(settings);

        assert_eq!(
            ui_appearance_snapshot(&settings),
            (Some("es_ES".to_string()), Some(18))
        );
    }

    #[test]
    fn smoke_hooks_from_env_open_nonempty_sets_open_windows_true() {
        assert!(smoke_hooks_from_env(Some("1"), None, None, None, None).open_windows);
        assert!(smoke_hooks_from_env(Some("anything"), None, None, None, None).open_windows);
    }

    #[test]
    fn smoke_hooks_from_env_open_empty_or_absent_leaves_open_windows_false() {
        assert!(!smoke_hooks_from_env(None, None, None, None, None).open_windows);
        assert!(!smoke_hooks_from_env(Some(""), None, None, None, None).open_windows);
    }

    #[test]
    fn smoke_hooks_from_env_close_positive_integer_parses_to_some() {
        assert_eq!(
            smoke_hooks_from_env(None, Some("5"), None, None, None).auto_close_frames,
            Some(5)
        );
        assert_eq!(
            smoke_hooks_from_env(None, Some("1"), None, None, None).auto_close_frames,
            Some(1)
        );
    }

    #[test]
    fn smoke_hooks_from_env_close_zero_is_none() {
        assert_eq!(
            smoke_hooks_from_env(None, Some("0"), None, None, None).auto_close_frames,
            None
        );
    }

    #[test]
    fn smoke_hooks_from_env_close_garbage_is_none() {
        assert_eq!(
            smoke_hooks_from_env(None, Some("not-a-number"), None, None, None).auto_close_frames,
            None
        );
    }

    #[test]
    fn smoke_hooks_from_env_close_absent_or_empty_is_none() {
        assert_eq!(
            smoke_hooks_from_env(None, None, None, None, None).auto_close_frames,
            None
        );
        assert_eq!(
            smoke_hooks_from_env(None, Some(""), None, None, None).auto_close_frames,
            None
        );
    }

    #[test]
    fn smoke_hooks_from_env_open_viewer_nonempty_is_some_path_empty_is_none() {
        assert_eq!(
            smoke_hooks_from_env(None, None, Some("/tmp/av-drift.mp4"), None, None).open_viewer,
            Some(PathBuf::from("/tmp/av-drift.mp4"))
        );
        assert_eq!(
            smoke_hooks_from_env(None, None, Some(""), None, None).open_viewer,
            None
        );
        assert_eq!(
            smoke_hooks_from_env(None, None, None, None, None).open_viewer,
            None
        );
    }

    #[test]
    fn smoke_hooks_from_env_live_fixture_nonempty_is_some_path_empty_is_none() {
        assert_eq!(
            smoke_hooks_from_env(None, None, None, Some("/tmp/audio-long.mp3"), None).live_fixture,
            Some(PathBuf::from("/tmp/audio-long.mp3"))
        );
        assert_eq!(
            smoke_hooks_from_env(None, None, None, Some(""), None).live_fixture,
            None
        );
        assert_eq!(
            smoke_hooks_from_env(None, None, None, None, None).live_fixture,
            None
        );
    }

    #[test]
    fn smoke_hooks_from_env_live_fixture_seconds_defaults_and_parses() {
        assert_eq!(
            smoke_hooks_from_env(None, None, None, None, None).live_fixture_seconds,
            DEFAULT_LIVE_FIXTURE_SECONDS
        );
        assert_eq!(
            smoke_hooks_from_env(None, None, None, None, Some("0")).live_fixture_seconds,
            DEFAULT_LIVE_FIXTURE_SECONDS
        );
        assert_eq!(
            smoke_hooks_from_env(None, None, None, None, Some("not-a-number")).live_fixture_seconds,
            DEFAULT_LIVE_FIXTURE_SECONDS
        );
        assert_eq!(
            smoke_hooks_from_env(None, None, None, None, Some("45")).live_fixture_seconds,
            45
        );
    }
}

/// Covers [`shutdown_queue`] (final-review fix 4) end to end against a real
/// [`crate::services::Services`] built by [`services::init_with`]: `Services`
/// hard-codes the real ffmpeg/ffprobe decoder/prober (only `engines` is
/// fakeable via `TestDeps`, same constraint `kittest_phases.rs`'s own module
/// doc explains), so the enqueued task's input must be genuinely decodable —
/// this reuses the same real Buzz-sourced test WAV (`HUMM_BUZZ_TESTDATA`
/// override) every other GUI test in this workspace that reaches the decode
/// step already depends on — rather than exercising a fake pipeline that
/// would never actually reach the engine.
#[cfg(test)]
mod shutdown_tests {
    use super::*;
    use crate::services::{self, Capabilities, TestDeps};
    use humm_core::boundary::{EngineCaps, EngineOptions, Pcm, SegmentSink, TranscriptionEngine};
    use humm_core::cancel::CancelToken;
    use humm_core::domain::{Input, ModelSpec, OutputSpec, Source, TaskSpec, TranscriptionOptions};
    use humm_core::error::{CoreError, EngineError};
    use humm_core::queue::EngineFactory;
    use humm_store::SqliteStore;
    use std::time::Duration;

    fn testdata_wav() -> PathBuf {
        std::env::var_os("HUMM_BUZZ_TESTDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
            .join("whisper-latvian.wav")
    }

    /// An engine that signals it has started, then busy-polls `cancel` until
    /// tripped, returning `Canceled` — proving `shutdown_queue`'s
    /// `cancel_all()` reaches an already-in-flight job promptly, with no
    /// separate rendezvous "release" signal needed (unlike `queue.rs`'s own
    /// gate-based tests, which need to control exactly when a *scripted*
    /// result becomes available; here the only thing under test is whether
    /// cancellation itself is observed, so polling the token directly is
    /// both simpler and just as deterministic — the loop cannot exit any
    /// other way).
    struct BlockingEngine {
        entered_tx: flume::Sender<()>,
    }

    impl TranscriptionEngine for BlockingEngine {
        fn transcribe(
            &mut self,
            _audio: &Pcm,
            _options: &EngineOptions,
            _sink: &mut dyn SegmentSink,
            cancel: &CancelToken,
        ) -> Result<(), EngineError> {
            let _ = self.entered_tx.send(());
            while !cancel.is_canceled() {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(EngineError::Canceled)
        }

        fn capabilities(&self) -> EngineCaps {
            EngineCaps {
                word_timestamps: false,
                translate: false,
                vad: false,
            }
        }
    }

    struct BlockingFactory {
        entered_tx: flume::Sender<()>,
    }

    impl EngineFactory for BlockingFactory {
        fn create(
            &mut self,
            _model: &ModelSpec,
            _resolved: Option<&Path>,
        ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
            Ok(Box::new(BlockingEngine {
                entered_tx: self.entered_tx.clone(),
            }))
        }
    }

    #[test]
    fn shutdown_queue_cancels_the_in_flight_task_and_joins_the_runner() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
        let (entered_tx, entered_rx) = flume::bounded(1);
        let media_dir = tempfile::tempdir().expect("media tempdir");

        let mut services = services::init_with(TestDeps {
            store: Arc::clone(&store),
            media_dir: media_dir.path().to_path_buf(),
            model_root: media_dir.path().join("models"),
            settings: humm_core::settings::Settings::default(),
            settings_path: media_dir.path().join("settings.toml"),
            engines: Box::new(BlockingFactory { entered_tx }),
            fetcher: None,
            capabilities: Capabilities::default(),
            artifacts: None,
            secrets: services::default_test_secrets(),
            translate: services::default_test_translate(),
            llm: None,
            validate: services::default_test_validate(),
        });

        let id = TranscriptionId::new();
        let spec = TaskSpec {
            input: Input::Path(testdata_wav()),
            options: TranscriptionOptions::default(),
            output: OutputSpec::default(),
        };
        services.queue().enqueue(id, spec, Source::FileImport);

        entered_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the engine must have started before shutdown runs");

        let queue = services
            .queue
            .take()
            .expect("queue present before shutdown");
        shutdown_queue(queue);

        let row = store
            .get_task(&id)
            .expect("get_task")
            .expect("the task row still exists");
        assert_eq!(
            row.status,
            Status::Canceled,
            "shutdown must cancel the in-flight task and write its terminal status \
             before the runner is considered joined"
        );
    }
}

/// Covers [`HummApp::recover_crashed_session`]'s re-drain fix: a *finished*
/// session thread whose final message (`Stopped`, or in principle `Error`)
/// is still sitting undrained must be treated as a normal completion, never
/// a crash. The real production scenario is racy (the message-send and the
/// thread's own OS-level exit both landing in the narrow gap between
/// `HummApp::drain_live_session`'s snapshot and this fallback's
/// `is_finished()` check) — deliberately reproducing that exact interleaving
/// would make the test itself racy/flaky. Instead this drives
/// `recover_crashed_session` directly with the *end state* the race
/// produces (a `JoinHandle` that is already finished, and a message already
/// sitting in the channel) — fully deterministic, and it exercises the
/// precise branch the fix added: no synthetic timing, no sleeping, just the
/// two facts `recover_crashed_session`'s own doc comment reasons about.
#[cfg(test)]
mod crash_recovery_tests {
    use super::*;
    use crate::live_session::ActiveSession;
    use crate::services::{self, Capabilities, TestDeps};
    use humm_core::domain::ModelSpec;
    use humm_core::error::CoreError;
    use humm_core::queue::EngineFactory;
    use humm_store::SqliteStore;

    /// No task is ever enqueued in this module's tests (the queue is
    /// entirely unrelated to the live-session recovery path under test).
    struct UnreachableEngineFactory;

    impl EngineFactory for UnreachableEngineFactory {
        fn create(
            &mut self,
            _model: &ModelSpec,
            _resolved: Option<&Path>,
        ) -> Result<Box<dyn humm_core::boundary::TranscriptionEngine>, CoreError> {
            unreachable!("no task is ever enqueued in this module's tests")
        }
    }

    /// Builds a bare `HummApp` for this module's tests. `media_dir`/
    /// `model_root`/`settings_path` are never-created placeholder paths —
    /// this module never enqueues a real task or saves settings (the one
    /// `Stopped` message it drives has `measurement: None`, so
    /// `finish_live_session`'s own `settings.save` is never reached), so
    /// nothing ever reads or writes under them.
    fn test_app() -> HummApp {
        let store = Arc::new(SqliteStore::open_in_memory().expect("open store"));
        let placeholder = PathBuf::from("/humm-gui-crash-recovery-test-unused");
        let services = services::init_with(TestDeps {
            store,
            media_dir: placeholder.join("media"),
            model_root: placeholder.join("models"),
            settings: humm_core::settings::Settings::default(),
            settings_path: placeholder.join("settings.toml"),
            engines: Box::new(UnreachableEngineFactory),
            fetcher: None,
            capabilities: Capabilities::default(),
            artifacts: None,
            secrets: services::default_test_secrets(),
            translate: services::default_test_translate(),
            llm: None,
            validate: services::default_test_validate(),
        });
        HummApp::new_for_test(services)
    }

    #[test]
    fn redrain_wins_over_crash_declaration_for_a_finished_thread_with_a_queued_stopped() {
        let mut app = test_app();
        app.open_recording_window_for_test();
        let vm_arc = app
            .recording_vm_for_test()
            .expect("the window is open, so a VM exists");

        // The end state the real race produces: the thread already sent its
        // final message (so it's sitting in `rx`, undrained) *and* has
        // already exited (`is_finished() == true`) — deterministically
        // constructed, not raced into.
        let (tx, rx) = mpsc::channel();
        tx.send(SessionMessage::Stopped { measurement: None })
            .expect("send into an unbounded channel with a live receiver cannot fail");
        let handle = std::thread::spawn(|| {});
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while !handle.is_finished() {
            assert!(
                Instant::now() < deadline,
                "a trivial no-op thread should finish almost instantly"
            );
            std::hint::spin_loop();
        }

        {
            let mut vm = vm_arc
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            vm.status = RecordingStatus::Stopping;
            vm.session = Some(ActiveSession {
                cancel: CancelToken::new(),
                rx,
                thread: Some(handle),
            });
        }

        // Exercise `recover_crashed_session` directly (not via
        // `drain_live_session`'s own first drain) — this *is* "a finished
        // thread whose message is still queued" the review asked for,
        // isolated from the (unrelated, already-covered-elsewhere) normal
        // per-frame drain path.
        app.recover_crashed_session(&vm_arc, &egui::Context::default());

        let vm = vm_arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            vm.status,
            RecordingStatus::Stopped,
            "the re-drained Stopped message must win — normal finish, not a crash"
        );
        assert!(
            vm.error_dialog.is_none(),
            "a healthy stop must never surface the crash dialog, got {:?}",
            vm.error_dialog
        );
        assert!(
            vm.copy_bar_visible,
            "finish_stopping's own normal-stop UI restore ran (copy bar shows)"
        );
        assert!(
            vm.session.is_none(),
            "the (now-consumed) session handle must be cleared either way"
        );
    }
}
