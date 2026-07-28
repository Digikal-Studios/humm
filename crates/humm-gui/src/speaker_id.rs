//! The Speaker-ID window's view-model, background worker, and the
//! [`DiarizeService`] boundary it drives (F-087, J-7, M8 design spec §6).
//!
//! Ported from Buzz's `SpeakerIdentificationWidget`
//! (`buzz/widgets/transcription_viewer/speaker_identification_widget.py`):
//! the gate (`:514-518`), the Identify/Cancel button pair (`:593-631`), the
//! progress label (`:648-661`), Step 2's per-speaker rows (`:663-705`), the
//! play-sample preview (`:707-725`), and Save (`:727-800`). Buzz's own
//! NeMo/MSDD + `ctc-forced-aligner` + punctuation stack is not ported
//! (ADR-011): `humm-diarize`'s `OrtDiarizer` fills the same UI slot behind
//! [`DiarizeService`], the seam this module actually drives (a thinner,
//! flat-`f32`/`String`-error shape than `humm_core::diarize::Diarizer`
//! itself — see the trait's own doc for why).
//!
//! # Window flow
//!
//! [`SpeakerIdVm::open`] never fails outright (Buzz always constructs the
//! widget): a missing/absent audio file just seeds [`SpeakerIdStage::MissingAudio`]
//! instead of [`SpeakerIdStage::Idle`] (Buzz `:514-518`, `os.path.isfile`).
//! Identify spawns a background worker
//! ([`SpeakerIdVm::start_identify`]) that decodes the source audio (16 kHz
//! mono, the existing [`humm_core::boundary::AudioDecoder::decode`] a
//! transcription task itself uses — reused here rather than a second
//! decoder seam), ensures the diarization models are present (download
//! progress, cancel-checked per file), runs [`DiarizeService::diarize`], then
//! maps the result onto the transcript's segments
//! ([`humm_core::diarize::assign_speakers`]) — landing in
//! [`SpeakerIdStage::Ready`] with one default `"Speaker N"` name per unique
//! speaker and the merge checkbox defaulted on (Buzz `:574`).
//!
//! **Stage-label honesty (binding, M8 task-11 brief):** Buzz's numbered
//! `"N/8 ..."` progress steps are not ported — this module's own
//! `"speaker-id-downloading-models"`/`"speaker-id-decoding"`/
//! `"speaker-id-identifying"` catalog keys are used instead, and every
//! [`humm_core::diarize::DiarizeStage`] variant [`DiarizeService::diarize`]'s
//! progress callback reports collapses to the single neutral
//! `"speaker-id-identifying"` label — never a per-stage name like
//! "Segmenting", which would misrepresent `speakrs`'s single monolithic call
//! as more finely observable than it is (`humm-diarize`'s own module doc:
//! only `LoadingModels`/`Segmenting` are ever emitted, both `fraction: None`).
//! The progress bar is determinate only for the model download (a real
//! byte fraction); every other stage renders an animated indeterminate bar
//! (`views::speaker_id`, the same `ProgressBar::new(0.0).animate(true)`
//! idiom `models_ui.rs` already uses) — UX_SPEC's "progress is real, never
//! cosmetic" rule applies to *withholding* a fraction just as much as to
//! fabricating one.
//!
//! # Cancel-ack mechanism (binding, documented per the task brief)
//!
//! [`OrtDiarizer::diarize`] can only observe a cancel at its own call
//! boundaries (`humm-diarize`'s module doc, deviations ledger #11) — up to
//! ~10s worst case on the two-speaker fixture. Buzz's own widget acks
//! instantly regardless (`on_cancel_button_clicked` sets "Cancelling..."
//! before it even asks the thread to stop, `:616-624`); this module does the
//! same: [`SpeakerIdVm::cancel_identify`] flips [`SpeakerIdVm::stage`] to
//! `Running { label: "Cancelling…", .. }` and disables further cancel clicks
//! *immediately*, trips the run's [`CancelToken`], and otherwise leaves the
//! background thread alone — it keeps running to completion (or its own
//! next cancel check) and writes its result into the run's shared
//! [`RunStatus`] exactly as an uncancelled run would.
//!
//! The guard against that stale result ever reaching [`SpeakerIdStage::Ready`]
//! is the run's own [`CancelToken`], not a separate generation counter:
//! [`SpeakerIdVm::poll_identify`] checks `cancel.is_canceled()` before
//! looking at the worker's `Ok`/`Err` outcome at all, and — because at most
//! one run is ever in flight (Identify is unreachable while `Running`) —
//! there is no second, *newer* run whose own result a stale counter would
//! need to be told apart from. A cancelled run's outcome, whatever it is,
//! is always discarded back to [`SpeakerIdStage::Idle`].
//!
//! **Deliberate divergence:** Buzz shows a distinct terminal `"Cancelled"`
//! label (`:623`) between the "Cancelling..." ack and resetting its
//! buttons; this module skips that middle step and resolves straight to
//! `Idle` — which `views::speaker_id::ui` renders with the same
//! `"speaker-id-ready-label"` text ("Ready to identify speakers") an
//! untouched window shows. A simplification, not an oversight: nothing in
//! the brief or UX_SPEC asks for a third transient label, and collapsing
//! "Cancelled" and "Ready to identify speakers" into one state removes a
//! stage a test would otherwise have to race against.
//!
//! # Play-sample (Buzz `:707-725`, deviations ledger #7)
//!
//! The window owns its own `humm_audio::Player`, built lazily on the
//! *first* Play-sample click (not at window open — decoding a whole file to
//! playback PCM is heavy, ~1.27 GB/hr) over
//! [`humm_core::boundary::AudioDecoder::decode_playback`], the exact same
//! background-decode-then-poll shape `crate::viewer::ViewerVm` already uses
//! for its own player ([`SpeakerIdVm::poll_sample_player`] mirrors
//! `ViewerVm::poll_player_load`). [`pick_sample`] is the pure selection
//! logic: a random segment assigned the chosen speaker, played for
//! `min(10s, segment length)`. Buzz instead arms a one-shot timer with the
//! segment's *absolute end time* (capped at 10s, `:725`) — a real bug (it
//! can cut a long segment's sample short, or run past a short segment into
//! the next speaker's audio); humm's own cap is against the *sample's own
//! length*, always exactly what it says on the tin.
//!
//! # Save (Buzz `:727-800`)
//!
//! [`save`] mirrors `views::resizer::save_segments`'s create-new shape
//! exactly (`store.copy_replaced_completed`, one transaction, D5b-3), fed by
//! [`humm_core::diarize::label_segments`] for the merge/name-prefix
//! text. It is synchronous — like the resizer's Extend/Resize tail, not its
//! background Merge — because by the time Save is clickable all the heavy
//! work already ran in Step 1; only a fast DB write remains. The interface
//! note every earlier M8 task carries forward: `AppEvent::TranscriptionsUpdated`
//! now carries `close_source`, and this window's save always sends `false`
//! — the source viewer stays open (`app.rs`'s harvest), unlike the resizer's
//! `true`. Buzz's own widget reaches the same outcome by a narrower path:
//! `SpeakerIdentificationWidget.on_save_button_clicked` only calls
//! `self.close()` (closing itself), never the viewer — the viewer closing on
//! *any* save is a side effect of a *different* wire, a shared
//! `transcriptions_updated_signal` the main window hands to every viewer,
//! which each viewer separately (re-)connects to its own `self.close` the
//! moment its Resize *or* Identify Speakers button is first clicked
//! (`transcription_viewer_widget.py:1333,1346`) — so in Buzz, saving from
//! *this* window closes the viewer only because that viewer happened to
//! wire its own close to the same shared signal, not because the speaker-id
//! save path itself asked for that. `close_source: bool` on the humm event
//! makes the same outcome an explicit, per-operation choice instead of an
//! accident of shared-signal wiring.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use humm_audio::Player;
use humm_core::boundary::{AudioDecoder, AudioOutput, Pcm};
use humm_core::cancel::CancelToken;
use humm_core::diarize::{DiarizeProgress, Diarizer, SpeakerTurn, assign_speakers, label_segments};
use humm_core::domain::{Millis, Segment, TranscriptionId};
use humm_core::error::StoreError;
use humm_core::models::download::{DownloadProgress, ModelDownloader};
use humm_core::models::registry;
use humm_diarize::OrtDiarizer;
use humm_store::SqliteStore;
use rand::Rng;

use crate::i18n::tr;
use crate::viewmodel::display_name_for;

/// A sample preview never plays more than this much of a segment (deviations
/// ledger #7): Buzz arms its stop-timer with the segment's *absolute end
/// time* capped at 10s (`speaker_identification_widget.py:725`); humm caps
/// against the segment's own length instead.
pub const SAMPLE_MAX: Millis = Millis(10_000);

// -- The DiarizeService boundary ------------------------------------------

/// The Speaker-ID window's seam onto diarization (F-087): a thinner,
/// GUI-facing shape than `humm_core::diarize::Diarizer` itself — flat `f32`
/// download progress (no per-file `DownloadProgress` detail the window has
/// any use for) and `String` errors (already-localized-enough diagnostic
/// text, the same "adapter errors become display strings at the GUI
/// boundary" convention `crate::translate_seam`'s traits use). Implemented
/// for real by [`RealDiarizeService`] (wrapping
/// [`ModelDownloader::ensure_diarization_models`] +
/// a freshly-built [`OrtDiarizer`] per call); [`ScriptedDiarizeService`] is
/// the hermetic kittest double.
pub trait DiarizeService: Send + Sync {
    /// Ensures the diarization model files are present locally, reporting a
    /// `[0.0, 1.0]` download fraction per file (no cross-file aggregation —
    /// same "one `DownloadProgress` sequence per artifact" shape
    /// [`ModelDownloader::ensure_diarization_models`] itself documents).
    fn ensure_models(
        &self,
        progress: &mut dyn FnMut(f32),
        cancel: &CancelToken,
    ) -> Result<(), String>;

    /// Runs diarization over already-decoded `audio`, reporting
    /// [`DiarizeProgress`] updates and checking `cancel` cooperatively (see
    /// this module's doc for the cancel-ack granularity this can actually
    /// offer).
    fn diarize(
        &self,
        audio: &Pcm,
        progress: &mut dyn FnMut(DiarizeProgress),
        cancel: &CancelToken,
    ) -> Result<Vec<SpeakerTurn>, String>;
}

/// The real [`DiarizeService`]: downloads via [`ModelDownloader`], then
/// builds a fresh [`OrtDiarizer`] per [`Self::diarize`] call — the same
/// "cheap to rebuild, no `&mut self` needed" choice `OrtDiarizer`'s own
/// module doc explains (~69ms on the spike's machine).
pub struct RealDiarizeService {
    downloader: ModelDownloader,
    model_root: PathBuf,
}

impl RealDiarizeService {
    /// Builds a service rooted at `model_root` (`paths::model_root()` in
    /// production, matching every other model consumer in this crate).
    pub fn new(model_root: PathBuf) -> Self {
        RealDiarizeService {
            downloader: ModelDownloader::new(model_root.clone()),
            model_root,
        }
    }
}

impl DiarizeService for RealDiarizeService {
    fn ensure_models(
        &self,
        progress: &mut dyn FnMut(f32),
        cancel: &CancelToken,
    ) -> Result<(), String> {
        let mut on_progress = |update: DownloadProgress| {
            if let Some(total) = update.total
                && total > 0
            {
                progress(update.downloaded as f32 / total as f32);
            }
        };
        self.downloader
            .ensure_diarization_models(&mut on_progress, cancel)
            .map(|_paths| ())
            .map_err(|err| err.to_string())
    }

    fn diarize(
        &self,
        audio: &Pcm,
        progress: &mut dyn FnMut(DiarizeProgress),
        cancel: &CancelToken,
    ) -> Result<Vec<SpeakerTurn>, String> {
        let paths = registry::diarization_model_paths(&self.model_root);
        let diarizer = OrtDiarizer::new(paths).map_err(|err| err.to_string())?;
        diarizer
            .diarize(audio, progress, cancel)
            .map_err(|err| err.to_string())
    }
}

/// A hermetic [`DiarizeService`] test double (kittest): [`Self::ready`]
/// succeeds instantly with a fixed turn list; [`Self::failing`] fails
/// instantly with a fixed message; [`Self::blocking`] blocks
/// [`Self::diarize`] on a test-controlled channel — the
/// `cancel_mid_run_resets_to_idle` kittest's double, letting the test click
/// Cancel *while* the background worker is genuinely still in flight before
/// releasing it.
pub struct ScriptedDiarizeService {
    turns: Vec<SpeakerTurn>,
    error: Option<String>,
    gate: Option<Mutex<Receiver<()>>>,
}

impl ScriptedDiarizeService {
    /// Succeeds instantly: `ensure_models` is a no-op, `diarize` returns
    /// `turns` straight away.
    pub fn ready(turns: Vec<SpeakerTurn>) -> Self {
        ScriptedDiarizeService {
            turns,
            error: None,
            gate: None,
        }
    }

    /// Fails instantly: `ensure_models` still succeeds (models are never the
    /// simulated failure), `diarize` returns `Err(message)`.
    pub fn failing(message: impl Into<String>) -> Self {
        ScriptedDiarizeService {
            turns: Vec::new(),
            error: Some(message.into()),
            gate: None,
        }
    }

    /// `diarize` blocks on `release.recv()` before returning — `ensure_models`
    /// still succeeds instantly. Once released, it checks `cancel` first (a
    /// canceled run gets a canceled-shaped error; the caller's
    /// [`SpeakerIdVm::poll_identify`] guard discards either outcome the
    /// instant `cancel` is tripped, so what exactly comes back after release
    /// barely matters — this keeps the double honest rather than silently
    /// returning success over a request it was asked to abandon).
    pub fn blocking(release: Receiver<()>, turns: Vec<SpeakerTurn>) -> Self {
        ScriptedDiarizeService {
            turns,
            error: None,
            gate: Some(Mutex::new(release)),
        }
    }
}

impl DiarizeService for ScriptedDiarizeService {
    fn ensure_models(
        &self,
        _progress: &mut dyn FnMut(f32),
        _cancel: &CancelToken,
    ) -> Result<(), String> {
        Ok(())
    }

    fn diarize(
        &self,
        _audio: &Pcm,
        _progress: &mut dyn FnMut(DiarizeProgress),
        cancel: &CancelToken,
    ) -> Result<Vec<SpeakerTurn>, String> {
        if let Some(gate) = &self.gate {
            let _ = gate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .recv();
        }
        if cancel.is_canceled() {
            return Err("canceled".to_string());
        }
        if let Some(message) = &self.error {
            return Err(message.clone());
        }
        Ok(self.turns.clone())
    }
}

// -- Window dependency bundle ----------------------------------------------

/// The adapter bundle the Speaker-ID window needs — the same "`Arc`'d trait
/// objects, cheap to clone into a deferred-viewport closure" shape
/// `crate::viewer::ViewerDeps` uses. `decoder`/`audio_output` are the exact
/// same `Arc`s the viewer's own deps hold (real ffmpeg/cpal in production,
/// the identical fakes in kittest) — this window decodes/plays back the
/// same source file, so there is no reason for it to own separate adapter
/// instances.
#[derive(Clone)]
pub struct SpeakerIdDeps {
    /// The shared persistent store — reads the source's file/segments at
    /// open and Identify time; Save writes the labeled copy.
    pub store: Arc<SqliteStore>,
    /// Decodes the source audio: 16 kHz mono for diarization
    /// ([`AudioDecoder::decode`]) and 44.1 kHz stereo for sample preview
    /// ([`AudioDecoder::decode_playback`]).
    pub decoder: Arc<dyn AudioDecoder>,
    /// Opens the audio output stream for the window-owned sample-preview
    /// [`Player`].
    pub audio_output: Arc<dyn AudioOutput>,
    /// The diarization seam ([`DiarizeService`]) — real in production, a
    /// [`ScriptedDiarizeService`] in kittest.
    pub diarize: Arc<dyn DiarizeService>,
}

// -- View-model -------------------------------------------------------------

/// One Speaker-ID run's outcome, once Step 1 reaches Ready (Buzz
/// `identification_result` plus its per-widget `speaker_mapping`
/// construction, `:663-705,738-740`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReadyState {
    /// The source transcript's segments, in store order — the same slice
    /// [`assign_speakers`] was run against.
    pub segments: Vec<Segment>,
    /// Per-segment relabeled speaker index, aligned 1:1 with `segments`
    /// ([`assign_speakers`]'s own contract).
    pub speakers: Vec<usize>,
    /// One editable name per unique speaker, indexed by the relabeled
    /// speaker index — prefilled `"Speaker {idx}"` (Buzz `:687-688`), sorted
    /// by that index (Step 2's row order).
    pub names: Vec<String>,
    /// "Merge speaker sentences" checkbox — default `true` (Buzz `:574`).
    pub merge: bool,
}

/// The Speaker-ID window's stage (F-087, this task's brief).
#[derive(Debug, Clone, PartialEq)]
pub enum SpeakerIdStage {
    /// Ready to click Identify (Buzz "Ready to identify speakers", `:515`).
    Idle,
    /// The source's audio file does not exist on disk (Buzz "Audio file not
    /// found", `:517` — Identify is disabled in this stage).
    MissingAudio,
    /// A background Identify run is in flight (or being canceled — see this
    /// module's doc for why "Cancelling…" is just this variant with that
    /// label rather than a separate stage).
    Running {
        /// The current stage's already-localized display text.
        label: String,
        /// `Some(fraction)` only while genuinely measurable (the model
        /// download); `None` for every other stage — an indeterminate
        /// progress bar, never a fabricated fraction.
        fraction: Option<f32>,
    },
    /// Step 1 finished: Step 2's name/merge/Save form is live.
    Ready(ReadyState),
    /// The background run failed; the message is already the adapter's own
    /// diagnostic text (no dedicated humm wrapper — Buzz has no error
    /// display of its own here at all, `on_identification_error` only resets
    /// the buttons; humm surfaces the message instead, per UX_SPEC's "never
    /// silent" terminal-state rule).
    Error(String),
}

/// Shared state a background Identify worker writes and
/// [`SpeakerIdVm::poll_identify`] reads — the same `Arc<Mutex<_>>` +
/// `ctx.request_repaint()` shape `models_ui::start_download`'s
/// `DownloadStatus` uses for its own background download.
struct RunStatus {
    label: String,
    fraction: Option<f32>,
    finished: Option<Result<ReadyState, String>>,
}

/// One in-flight (or just-canceled-but-not-yet-resolved) Identify run.
struct RunHandle {
    status: Arc<Mutex<RunStatus>>,
    cancel: CancelToken,
}

/// The sample-preview player's lifecycle — the window-owned analogue of
/// `crate::viewer::PlayerState`, except it starts `NotStarted` (decode is
/// lazy, on the first Play-sample click) rather than always `Loading` at
/// open.
enum SamplePlayerState {
    /// No Play-sample click has happened yet.
    NotStarted,
    /// Decode is in flight on the background thread.
    Loading,
    /// The playback engine is live.
    Ready(Player),
    /// Decode (or output-stream open) failed; the string names the cause.
    Failed(String),
}

/// One open Speaker-ID window's state (F-087, J-7).
pub struct SpeakerIdVm {
    /// The transcript this window operates on.
    pub source: TranscriptionId,
    /// Window title — the task's display name (`display_name_for`): the
    /// user-set row name if there is one, else the source file's basename.
    /// Buzz always uses the basename (`file_path_as_title`), so an un-renamed
    /// task matches Buzz exactly and a renamed one shows the name the user
    /// chose — deliberate, M8 design spec ledger #13.
    pub(crate) title: String,
    /// The source's input media file, `None` for a since-deleted/URL-only
    /// row (mirrors `ViewerVm::file`'s own defensive `None` case).
    file: Option<PathBuf>,
    /// The window's current stage.
    pub stage: SpeakerIdStage,
    /// A Save failure's message, kept *separate* from `stage` so it renders
    /// alongside the still-`Ready` Step 2 form instead of discarding the
    /// user's edited names/merge choice (the same shape
    /// `views::resizer::ResizerVm::error` uses for its own save-failure arm).
    /// Written by `app.rs`'s `apply_speaker_save` harvest (mirrors
    /// `ResizerVm::error`'s own `pub(crate)` visibility).
    pub(crate) save_error: Option<String>,
    /// The in-flight Identify run, if any.
    run: Option<RunHandle>,
    /// Set by [`Self::cancel_identify`]; cleared once the run's (possibly
    /// stale) result has actually been discarded. Disables the Cancel button
    /// from being clicked twice and keeps a late progress update from
    /// clobbering the "Cancelling…" label.
    pub(crate) cancel_requested: bool,
    /// The sample-preview player's lifecycle.
    sample_player: SamplePlayerState,
    /// Receiver for the background sample-decode thread; `Some` only while
    /// `sample_player` is `Loading`.
    sample_rx: Option<Receiver<Result<humm_core::boundary::PlaybackPcm, String>>>,
    /// Cancels the sample decode if the window closes mid-decode — tripped
    /// by `impl Drop for SpeakerIdVm`, below.
    sample_decode_cancel: CancelToken,
    /// A `(seek position, stop position)` a Play-sample click queued before
    /// the player finished building — applied the instant it does (by
    /// [`Self::poll_sample_player`]), or immediately if the player is
    /// already `Ready` (by [`Self::play_sample`] itself).
    pending_play: Option<(Millis, Millis)>,
    /// The position the sample player should pause at, once reached
    /// ([`Self::tick_sample_playback`]) — `None` when nothing is armed.
    stop_at: Option<Millis>,
    /// One-shot latch: the Save button was clicked this frame — `app.rs`
    /// harvests it (owns the store write + the app-level
    /// `TranscriptionsUpdated` event + the window map).
    pub(crate) save_requested: bool,
    /// Set by the render closure when the OS window's close is requested.
    pub(crate) close_requested: bool,
    /// Set when this source is re-opened while the window already exists
    /// (focus it instead of duplicating — the viewer/resizer deviation #1
    /// shape).
    pub(crate) focus_requested: bool,
}

impl SpeakerIdVm {
    /// Opens a Speaker-ID view-model for `source`: derives the window title
    /// (the task table's own `name`-else-file-basename-else-`url` fallback,
    /// [`crate::viewmodel::display_name_for`] — never a blank OS window
    /// title, including in the `MissingAudio` case, where there is no file
    /// to fall back to at all), and seeds [`SpeakerIdStage::Idle`] or
    /// [`SpeakerIdStage::MissingAudio`] from whether the source's file
    /// actually exists on disk (Buzz `:514-518`, `os.path.isfile`). Never
    /// fails outright on a missing file — only a genuine store-read error
    /// propagates (mirrors `ResizerVm::open`'s contract).
    pub fn open(source: TranscriptionId, deps: &SpeakerIdDeps) -> Result<Self, StoreError> {
        let task = deps.store.get_task(&source)?;
        let file = task.as_ref().and_then(|task| task.file.clone());
        let title = task.as_ref().map(display_name_for).unwrap_or_default();
        let stage = match &file {
            Some(path) if path.is_file() => SpeakerIdStage::Idle,
            _ => SpeakerIdStage::MissingAudio,
        };

        Ok(SpeakerIdVm {
            source,
            title,
            file,
            stage,
            save_error: None,
            run: None,
            cancel_requested: false,
            sample_player: SamplePlayerState::NotStarted,
            sample_rx: None,
            sample_decode_cancel: CancelToken::new(),
            pending_play: None,
            stop_at: None,
            save_requested: false,
            close_requested: false,
            focus_requested: false,
        })
    }

    /// The current Save-failure banner text, if any — `views::speaker_id::ui`
    /// renders this alongside the still-`Ready` Step 2 form (see the
    /// `save_error` field's own doc comment for why it's kept separate from
    /// `stage` rather than folded into it).
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// The recorded Save-failure banner text, for a test to assert on across
    /// the crate boundary — a thin alias of [`Self::save_error`] kept
    /// distinct so production rendering never calls a `_for_test`-suffixed
    /// method.
    pub fn save_error_for_test(&self) -> Option<&str> {
        self.save_error()
    }

    /// Whether a cancel has been requested for the in-flight run (see this
    /// module's doc for the ack-immediately-resolve-later mechanism) — for a
    /// test to assert on across the crate boundary.
    pub fn cancel_requested_for_test(&self) -> bool {
        self.cancel_requested
    }

    /// Whether Identify may be clicked (Buzz's `step_1_button.setEnabled`):
    /// any stage except `MissingAudio` or an in-flight `Running`.
    pub fn can_identify(&self) -> bool {
        !matches!(
            self.stage,
            SpeakerIdStage::MissingAudio | SpeakerIdStage::Running { .. }
        )
    }

    /// Starts a background Identify run (decode → ensure models → diarize →
    /// map to segments). A no-op if a run is already in flight, or if
    /// [`Self::can_identify`] would say no (defensive — the button is
    /// disabled in both cases already).
    pub fn start_identify(&mut self, deps: &SpeakerIdDeps, ctx: &egui::Context) {
        if !self.can_identify() {
            return;
        }
        let Some(path) = self.file.clone() else {
            self.stage = SpeakerIdStage::MissingAudio;
            return;
        };

        self.save_error = None;
        self.cancel_requested = false;
        let label = tr("speaker-id-decoding");
        self.stage = SpeakerIdStage::Running {
            label: label.clone(),
            fraction: None,
        };

        let status = Arc::new(Mutex::new(RunStatus {
            label,
            fraction: None,
            finished: None,
        }));
        let cancel = CancelToken::new();
        self.run = Some(RunHandle {
            status: Arc::clone(&status),
            cancel: cancel.clone(),
        });

        let source = self.source;
        let decoder = Arc::clone(&deps.decoder);
        let diarize = Arc::clone(&deps.diarize);
        let store = Arc::clone(&deps.store);
        let ctx_for_thread = ctx.clone();
        // The exact same `("speakers", id)` hash tuple `app.rs::show_speaker_
        // viewports`/`repaint_other_open_viewports` build this window's
        // `ViewportId` from — see `set_stage`'s doc for why a background
        // thread must target this id specifically, never a bare
        // `request_repaint()`.
        let viewport_id = egui::ViewportId::from_hash_of(("speakers", source));

        std::thread::spawn(move || {
            let outcome = run_identify(
                &path,
                source,
                decoder.as_ref(),
                diarize.as_ref(),
                store.as_ref(),
                &status,
                &cancel,
                &ctx_for_thread,
                viewport_id,
            );
            status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .finished = Some(outcome);
            ctx_for_thread.request_repaint_of(viewport_id);
        });
    }

    /// Requests cancellation of the in-flight run (see this module's doc for
    /// the ack-immediately-resolve-later mechanism). A no-op if nothing is
    /// running, or if already requested this run.
    pub fn cancel_identify(&mut self) {
        let Some(handle) = &self.run else { return };
        if self.cancel_requested {
            return;
        }
        handle.cancel.cancel();
        self.cancel_requested = true;
        self.stage = SpeakerIdStage::Running {
            label: tr("speaker-id-cancelling"),
            fraction: None,
        };
    }

    /// Polls the in-flight run: while running, keeps `stage`'s label/fraction
    /// current (unless a cancel is already pending — its own "Cancelling…"
    /// label must not be overwritten by a stray late progress update from the
    /// about-to-be-discarded run); once finished, resolves to `Idle` (if this
    /// run was canceled, whatever it returned), `Ready`, or `Error`. A no-op
    /// while nothing is running. Call once per frame, before rendering.
    pub fn poll_identify(&mut self) {
        let Some(handle) = &self.run else { return };
        let (label, fraction, finished) = {
            let guard = handle
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (guard.label.clone(), guard.fraction, guard.finished.clone())
        };
        if !self.cancel_requested {
            self.stage = SpeakerIdStage::Running { label, fraction };
        }
        let Some(outcome) = finished else { return };
        let canceled = handle.cancel.is_canceled();
        self.run = None;
        self.cancel_requested = false;
        self.stage = if canceled {
            SpeakerIdStage::Idle
        } else {
            match outcome {
                Ok(ready) => SpeakerIdStage::Ready(ready),
                Err(message) => SpeakerIdStage::Error(message),
            }
        };
    }

    // -- Sample preview (Buzz `:707-725`, ledger #7) -----------------------

    /// Plays a random segment of `speaker_idx` (Step 2's Play-sample button):
    /// picks a candidate via [`pick_sample`], lazily starts the sample
    /// player's background decode on the very first call, and either applies
    /// the pick immediately (player already `Ready`) or queues it for
    /// [`Self::poll_sample_player`] to apply once decode finishes. A no-op
    /// while `stage` isn't `Ready`, or if `speaker_idx` has no assigned
    /// segment (defensive — every row Step 2 renders came from `Ready`'s own
    /// `names`, whose indices always have at least one assigned segment).
    pub fn play_sample(&mut self, deps: &SpeakerIdDeps, speaker_idx: usize, ctx: &egui::Context) {
        let target = match &self.stage {
            SpeakerIdStage::Ready(ready) => {
                pick_sample(&ready.segments, &ready.speakers, speaker_idx, |n| {
                    rand::thread_rng().gen_range(0..n)
                })
            }
            _ => None,
        };
        let Some((start, duration)) = target else {
            return;
        };
        self.pending_play = Some((start, Millis(start.0 + duration.0)));
        self.ensure_sample_player_started(deps, ctx);
        self.apply_pending_play_if_ready();
    }

    /// Spawns the sample player's background decode exactly once (the first
    /// Play-sample click); a no-op on every later call regardless of outcome
    /// (`Loading`/`Ready`/`Failed` all short-circuit).
    fn ensure_sample_player_started(&mut self, deps: &SpeakerIdDeps, ctx: &egui::Context) {
        if !matches!(self.sample_player, SamplePlayerState::NotStarted) {
            return;
        }
        let Some(path) = self.file.clone() else {
            self.sample_player =
                SamplePlayerState::Failed("no media file for this transcript".to_string());
            return;
        };
        self.sample_player = SamplePlayerState::Loading;

        let decoder = Arc::clone(&deps.decoder);
        let cancel = self.sample_decode_cancel.clone();
        let ctx = ctx.clone();
        // Same reasoning as `start_identify`'s own worker (see `set_stage`'s
        // doc): a bare `request_repaint()` from this background thread would
        // target the root viewport, not this deferred window, and the
        // Loading -> Ready transition would sit unnoticed until unrelated
        // input repainted it.
        let viewport_id = egui::ViewportId::from_hash_of(("speakers", self.source));
        let (tx, rx) = mpsc::channel();
        self.sample_rx = Some(rx);

        std::thread::spawn(move || {
            let pcm = decoder
                .decode_playback(&path, &cancel)
                .map_err(|err| err.to_string());
            let _ = tx.send(pcm);
            ctx.request_repaint_of(viewport_id);
        });
    }

    /// Polls the sample player's background decode: on completion, builds
    /// the [`Player`] (or records the failure), then applies any
    /// [`Self::pending_play`] queued while it was loading. A no-op while
    /// still decoding, or before any Play-sample click has happened.
    pub fn poll_sample_player(&mut self, deps: &SpeakerIdDeps) {
        let Some(rx) = &self.sample_rx else { return };
        let pcm = match rx.try_recv() {
            Ok(pcm) => pcm,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.sample_rx = None;
                self.sample_player =
                    SamplePlayerState::Failed("decode thread ended unexpectedly".to_string());
                return;
            }
        };
        self.sample_rx = None;
        self.sample_player = match pcm {
            Ok(pcm) => match Player::new(pcm, Arc::clone(&deps.audio_output)) {
                Ok(player) => SamplePlayerState::Ready(player),
                Err(err) => SamplePlayerState::Failed(err.to_string()),
            },
            Err(msg) => SamplePlayerState::Failed(msg),
        };
        self.apply_pending_play_if_ready();
    }

    /// Applies a queued [`Self::pending_play`] if the sample player is
    /// `Ready` — seeks, plays, and arms [`Self::stop_at`]. A no-op otherwise
    /// (nothing queued, or the player isn't built yet).
    fn apply_pending_play_if_ready(&mut self) {
        let Some((start, stop_at)) = self.pending_play else {
            return;
        };
        if let SamplePlayerState::Ready(player) = &self.sample_player {
            player.seek(start);
            player.play();
            self.stop_at = Some(stop_at);
            self.pending_play = None;
        }
    }

    /// Pauses the sample player once its position reaches [`Self::stop_at`]
    /// (the "played up to `min(10s, sample length)`" cap, ledger #7's
    /// bounded-by-hand analogue of `ViewerVm::drive_smoke_playback`'s own
    /// per-frame position check). Call once per frame.
    pub fn tick_sample_playback(&mut self) {
        let SamplePlayerState::Ready(player) = &self.sample_player else {
            return;
        };
        let Some(stop_at) = self.stop_at else {
            return;
        };
        if player.is_playing() && player.position() >= stop_at {
            player.pause();
            self.stop_at = None;
        }
    }

    /// Whether the sample player is currently playing — drives the view's
    /// "keep repainting while a sample plays" cadence.
    pub fn sample_is_playing(&self) -> bool {
        matches!(&self.sample_player, SamplePlayerState::Ready(player) if player.is_playing())
    }

    /// The sample player's failure message, if decode or opening the audio
    /// output failed — for the view to surface as a small inline note near
    /// Step 2's Play-sample rows (UX_SPEC's "never silent" rule; Buzz's own
    /// `QMediaPlayer`/`sounddevice` preview has no error path of its own to
    /// port). `None` before any Play-sample click, or while still
    /// loading/already playing successfully.
    pub fn sample_error(&self) -> Option<&str> {
        match &self.sample_player {
            SamplePlayerState::Failed(message) => Some(message),
            _ => None,
        }
    }

    /// The in-flight run's cancel token, cloned — for a test to retain
    /// across a `drop`/window-close and check `is_canceled()` on afterward
    /// (the vm itself is gone by then, but a [`CancelToken`] clone shares
    /// the same underlying flag as the original). `None` while nothing is
    /// running.
    pub fn run_cancel_token_for_test(&self) -> Option<CancelToken> {
        self.run.as_ref().map(|handle| handle.cancel.clone())
    }
}

impl Drop for SpeakerIdVm {
    fn drop(&mut self) {
        // Closing the window must not leave either background thread
        // running for nothing (Buzz `closeEvent`, `speaker_identification_
        // widget.py:806-817`, stops the identify thread — `_cleanup_thread`
        // — and the media player on close). Every close path (the user's
        // OS close click, a Save success, or the originating viewer
        // closing and taking this window with it, `app.rs::close_viewer`)
        // ends the same way: the last `Arc<Mutex<SpeakerIdVm>>` clone drops
        // and this runs — the same "cleanup lives in `Drop`, not in every
        // close call site" shape `ViewerVm::drop` already uses for its own
        // background decode.
        if let Some(handle) = &self.run {
            handle.cancel.cancel();
        }
        self.sample_decode_cancel.cancel();
    }
}

/// The background Identify run's full pipeline (decode → ensure models →
/// diarize → map to segments) — a free function (not a `SpeakerIdVm` method)
/// since it runs entirely off the UI thread with no `&self` to borrow;
/// `status`/`cancel` are its only channel back to the vm. `ctx`/`viewport_id`
/// are threaded through purely so every [`set_stage`] call can request a
/// repaint of *this window specifically* — see [`set_stage`]'s own doc for
/// why a plain `ctx.request_repaint()` would not do that.
#[allow(clippy::too_many_arguments)]
fn run_identify(
    path: &Path,
    source: TranscriptionId,
    decoder: &dyn AudioDecoder,
    diarize: &dyn DiarizeService,
    store: &SqliteStore,
    status: &Mutex<RunStatus>,
    cancel: &CancelToken,
    ctx: &egui::Context,
    viewport_id: egui::ViewportId,
) -> Result<ReadyState, String> {
    if cancel.is_canceled() {
        return Err("canceled".to_string());
    }
    set_stage(status, tr("speaker-id-decoding"), None, ctx, viewport_id);
    let pcm = decoder
        .decode(path, cancel)
        .map_err(|err| err.to_string())?;

    if cancel.is_canceled() {
        return Err("canceled".to_string());
    }
    set_stage(
        status,
        tr("speaker-id-downloading-models"),
        Some(0.0),
        ctx,
        viewport_id,
    );
    diarize.ensure_models(
        &mut |fraction| {
            set_stage(
                status,
                tr("speaker-id-downloading-models"),
                Some(fraction),
                ctx,
                viewport_id,
            );
        },
        cancel,
    )?;

    if cancel.is_canceled() {
        return Err("canceled".to_string());
    }
    set_stage(status, tr("speaker-id-identifying"), None, ctx, viewport_id);
    let turns = diarize.diarize(
        &pcm,
        &mut |_progress| {
            // Every `DiarizeStage` collapses to the same neutral label — see
            // this module's doc for why surfacing per-stage names would be
            // dishonest for a single monolithic `speakrs` call.
            set_stage(status, tr("speaker-id-identifying"), None, ctx, viewport_id);
        },
        cancel,
    )?;

    if cancel.is_canceled() {
        return Err("canceled".to_string());
    }
    let segments = read_segments(store, source).map_err(|err| err.to_string())?;
    let speakers = assign_speakers(&segments, &turns);
    let n_speakers = speakers.iter().copied().max().map_or(0, |max| max + 1);
    let names = (0..n_speakers)
        .map(|idx| format!("Speaker {idx}"))
        .collect();

    Ok(ReadyState {
        segments,
        speakers,
        names,
        merge: true,
    })
}

/// Writes `label`/`fraction` into the shared run status (silently tolerating
/// a poisoned lock, the workspace `lock_recover` convention), then requests
/// a repaint of `viewport_id` specifically.
///
/// The Speaker-ID window is a *deferred* egui viewport
/// (`app.rs::show_speaker_viewports`), not the root one. A bare
/// `ctx.request_repaint()` called from a background thread resolves to
/// `egui::Context::viewport_id()`'s default — the root viewport — because
/// there is no "current viewport" outside of an actual frame; `show_
/// viewport_deferred` itself never self-schedules a repaint either. Without
/// targeting `viewport_id` explicitly via [`egui::Context::request_repaint_of`],
/// this window's progress bar/label would freeze (most visibly during the
/// determinate download stage, whose fraction updates rapidly) until some
/// unrelated input happened to repaint it — confirmed by reading `egui`
/// 0.35's own `Context::request_repaint`/`request_repaint_of` source, not
/// assumed.
fn set_stage(
    status: &Mutex<RunStatus>,
    label: String,
    fraction: Option<f32>,
    ctx: &egui::Context,
    viewport_id: egui::ViewportId,
) {
    {
        let mut guard = status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.label = label;
        guard.fraction = fraction;
    }
    ctx.request_repaint_of(viewport_id);
}

/// Reads `source`'s current segments from the store as plain [`Segment`]s —
/// the same row-to-`Segment` mapping `app.rs::read_source_segments` and
/// `ViewerVm::open` both already use.
fn read_segments(store: &SqliteStore, source: TranscriptionId) -> Result<Vec<Segment>, StoreError> {
    let rows = store.get_segment_rows(source)?;
    Ok(rows
        .into_iter()
        .map(|row| Segment {
            start: row.start,
            end: row.end,
            text: row.text,
            translation: row.translation,
        })
        .collect())
}

/// Picks a random segment assigned `speaker_idx` (Buzz `on_speaker_preview`'s
/// `random.choice` over the matching entries, `:711-714`), returning its
/// `(start, duration)` — `duration` capped at [`SAMPLE_MAX`] against the
/// segment's *own* length (ledger #7, not Buzz's absolute-end-time bug).
/// `pick_index(candidate_count)` supplies the choice: production passes a
/// real RNG, tests pass a deterministic closure. `None` when no segment in
/// `segments`/`speakers` is assigned `speaker_idx`.
pub fn pick_sample(
    segments: &[Segment],
    speakers: &[usize],
    speaker_idx: usize,
    pick_index: impl FnOnce(usize) -> usize,
) -> Option<(Millis, Millis)> {
    let candidates: Vec<&Segment> = segments
        .iter()
        .zip(speakers)
        .filter(|&(_, &speaker)| speaker == speaker_idx)
        .map(|(seg, _)| seg)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let idx = pick_index(candidates.len()).min(candidates.len() - 1);
    let segment = candidates[idx];
    let duration = (segment.end.0 - segment.start.0).clamp(0, SAMPLE_MAX.0);
    Some((segment.start, Millis(duration)))
}

/// The Save button's synchronous save path (Buzz `on_save_button_clicked`,
/// `:727-800`): labels `ready`'s segments (merge/name-prefix applied,
/// [`label_segments`]), then copies `source` to a fresh transcript already
/// completed with the labeled segments in one store transaction
/// ([`SqliteStore::copy_replaced_completed`], D5b-3) — the same call
/// `views::resizer::save_segments`'s create-new arm uses. Unlike the old
/// copy-then-complete pair this replaced, no intermediate `Queued` row is
/// ever written, so a crash mid-save can't orphan one. Returns the new
/// transcript's id; `app.rs`'s harvest sends
/// `AppEvent::TranscriptionsUpdated { close_source: false, .. }` and closes
/// this window on success, or records the error on [`SpeakerIdVm::save_error`]
/// (leaving `stage` — and the user's edited names/merge choice — untouched)
/// on failure.
pub fn save(
    store: &SqliteStore,
    source: TranscriptionId,
    ready: &ReadyState,
) -> Result<TranscriptionId, StoreError> {
    let labeled = label_segments(&ready.segments, &ready.speakers, &ready.names, ready.merge);
    store.copy_replaced_completed(source, &labeled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn seg(start: i64, end: i64, text: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    // -- pick_sample --------------------------------------------------------

    #[test]
    fn pick_sample_filters_to_the_chosen_speaker() {
        let segments = vec![
            seg(0, 1000, "a"),
            seg(1000, 2000, "b"),
            seg(2000, 3000, "c"),
        ];
        let speakers = vec![0, 1, 0];

        // Speaker 1 has exactly one candidate (segment 1) regardless of the
        // injected pick index.
        let picked = pick_sample(&segments, &speakers, 1, |n| {
            assert_eq!(n, 1, "exactly one candidate for speaker 1");
            0
        });
        assert_eq!(picked, Some((Millis(1000), Millis(1000))));
    }

    #[test]
    fn pick_sample_honors_the_injected_index() {
        let segments = vec![
            seg(0, 1000, "a"),
            seg(5000, 6000, "b"),
            seg(9000, 9500, "c"),
        ];
        let speakers = vec![0, 0, 0];

        // Three candidates for speaker 0; picking index 2 must select the
        // third one (segment "c", 9000..9500), not the first.
        let picked = pick_sample(&segments, &speakers, 0, |n| {
            assert_eq!(n, 3);
            2
        });
        assert_eq!(picked, Some((Millis(9000), Millis(500))));
    }

    #[test]
    fn pick_sample_caps_duration_at_ten_seconds() {
        let segments = vec![seg(0, 25_000, "long segment")];
        let speakers = vec![0];

        let (start, duration) = pick_sample(&segments, &speakers, 0, |_| 0).expect("one candidate");
        assert_eq!(start, Millis(0));
        assert_eq!(
            duration, SAMPLE_MAX,
            "capped at 10s, not the full 25s segment"
        );
    }

    #[test]
    fn pick_sample_under_ten_seconds_plays_the_whole_segment() {
        let segments = vec![seg(0, 3_000, "short segment")];
        let speakers = vec![0];

        let (_, duration) = pick_sample(&segments, &speakers, 0, |_| 0).expect("one candidate");
        assert_eq!(duration, Millis(3_000), "under the cap, plays in full");
    }

    #[test]
    fn pick_sample_no_candidate_is_none() {
        let segments = vec![seg(0, 1000, "a")];
        let speakers = vec![0];
        assert_eq!(pick_sample(&segments, &speakers, 7, |_| 0), None);
    }

    // -- vm state transitions -------------------------------------------------

    fn ready_state(names: &[&str]) -> ReadyState {
        ReadyState {
            segments: vec![seg(0, 1000, "hi")],
            speakers: vec![0],
            names: names.iter().map(|s| s.to_string()).collect(),
            merge: true,
        }
    }

    #[test]
    fn identify_to_ready_via_poll() {
        let status = Arc::new(Mutex::new(RunStatus {
            label: "decoding".to_string(),
            fraction: None,
            finished: None,
        }));
        let cancel = CancelToken::new();
        let mut vm = test_vm();
        vm.run = Some(RunHandle {
            status: Arc::clone(&status),
            cancel: cancel.clone(),
        });
        vm.stage = SpeakerIdStage::Running {
            label: "decoding".to_string(),
            fraction: None,
        };

        // Still running: poll keeps the label/fraction synced from the shared
        // status, and does not resolve.
        {
            let mut guard = status.lock().unwrap();
            guard.label = "identifying".to_string();
            guard.fraction = Some(0.5);
        }
        vm.poll_identify();
        assert_eq!(
            vm.stage,
            SpeakerIdStage::Running {
                label: "identifying".to_string(),
                fraction: Some(0.5),
            }
        );

        // Finished successfully: resolves to Ready.
        status.lock().unwrap().finished = Some(Ok(ready_state(&["Speaker 0"])));
        vm.poll_identify();
        assert_eq!(vm.stage, SpeakerIdStage::Ready(ready_state(&["Speaker 0"])));
        assert!(vm.run.is_none(), "the run handle is cleared once resolved");
    }

    #[test]
    fn identify_error_resolves_to_error_stage() {
        let status = Arc::new(Mutex::new(RunStatus {
            label: "decoding".to_string(),
            fraction: None,
            finished: Some(Err("boom".to_string())),
        }));
        let mut vm = test_vm();
        vm.run = Some(RunHandle {
            status,
            cancel: CancelToken::new(),
        });

        vm.poll_identify();
        assert_eq!(vm.stage, SpeakerIdStage::Error("boom".to_string()));
    }

    #[test]
    fn cancel_acks_immediately_then_resolves_to_idle_on_poll() {
        let status = Arc::new(Mutex::new(RunStatus {
            label: "decoding".to_string(),
            fraction: None,
            finished: None,
        }));
        let cancel = CancelToken::new();
        let mut vm = test_vm();
        vm.run = Some(RunHandle {
            status: Arc::clone(&status),
            cancel: cancel.clone(),
        });
        vm.stage = SpeakerIdStage::Running {
            label: "decoding".to_string(),
            fraction: None,
        };

        vm.cancel_identify();
        assert!(vm.cancel_requested);
        assert_eq!(
            vm.stage,
            SpeakerIdStage::Running {
                label: tr("speaker-id-cancelling"),
                fraction: None,
            },
            "the ack is immediate, before the worker has resolved anything"
        );

        // A stray late progress update from the doomed run must not clobber
        // the "Cancelling…" label.
        {
            let mut guard = status.lock().unwrap();
            guard.label = "identifying".to_string();
            guard.fraction = Some(0.9);
        }
        vm.poll_identify();
        assert_eq!(
            vm.stage,
            SpeakerIdStage::Running {
                label: tr("speaker-id-cancelling"),
                fraction: None,
            }
        );

        // The worker eventually resolves (even with a *successful*-shaped
        // outcome) — the cancel guard discards it back to Idle regardless.
        status.lock().unwrap().finished = Some(Ok(ready_state(&["Speaker 0"])));
        vm.poll_identify();
        assert_eq!(vm.stage, SpeakerIdStage::Idle);
        assert!(!vm.cancel_requested);
        assert!(vm.run.is_none());
    }

    #[test]
    fn can_identify_gates_on_missing_audio_and_running() {
        let mut vm = test_vm();
        assert!(vm.can_identify());

        vm.stage = SpeakerIdStage::MissingAudio;
        assert!(!vm.can_identify());

        vm.stage = SpeakerIdStage::Running {
            label: String::new(),
            fraction: None,
        };
        assert!(!vm.can_identify());

        vm.stage = SpeakerIdStage::Ready(ready_state(&["Speaker 0"]));
        assert!(
            vm.can_identify(),
            "re-running Identify from Ready is allowed (Buzz parity)"
        );
    }

    #[test]
    fn dropping_the_vm_cancels_the_in_flight_run_and_sample_decode() {
        let status = Arc::new(Mutex::new(RunStatus {
            label: "decoding".to_string(),
            fraction: None,
            finished: None,
        }));
        let run_cancel = CancelToken::new();
        let mut vm = test_vm();
        vm.run = Some(RunHandle {
            status,
            cancel: run_cancel.clone(),
        });
        let sample_cancel = vm.sample_decode_cancel.clone();

        assert!(!run_cancel.is_canceled());
        assert!(!sample_cancel.is_canceled());

        drop(vm);

        assert!(
            run_cancel.is_canceled(),
            "closing the window must cancel an in-flight Identify run \
             (Buzz closeEvent :806-817)"
        );
        assert!(
            sample_cancel.is_canceled(),
            "closing the window must also cancel an in-flight sample-preview decode"
        );
    }

    #[test]
    fn dropping_a_vm_with_no_in_flight_run_does_not_panic() {
        // No `run`, `sample_decode_cancel` never touched by a Play-sample
        // click — the common case (a window closed before ever clicking
        // Identify or Play sample) must drop cleanly.
        let vm = test_vm();
        drop(vm);
    }

    #[test]
    fn set_stage_requests_repaint_of_the_given_viewport_specifically() {
        let status = Arc::new(Mutex::new(RunStatus {
            label: String::new(),
            fraction: None,
            finished: None,
        }));
        let ctx = egui::Context::default();
        let this_window = egui::ViewportId::from_hash_of(("speakers", TranscriptionId::new()));
        let a_different_window =
            egui::ViewportId::from_hash_of(("speakers", TranscriptionId::new()));

        // Note: `egui::ViewportId::ROOT` is *not* a useful "untouched"
        // witness here — `egui::Context::default()` seeds it with an
        // implicit outstanding repaint of its own (every fresh context
        // wants its very first frame to paint), so it already reports
        // `has_requested_repaint_for == true` before `set_stage` ever runs.
        // A second, unrelated non-root id (never touched by this call) is
        // the clean negative case instead.
        assert!(!ctx.has_requested_repaint_for(&this_window));
        assert!(!ctx.has_requested_repaint_for(&a_different_window));

        set_stage(
            &status,
            "identifying".to_string(),
            Some(0.5),
            &ctx,
            this_window,
        );

        assert!(
            ctx.has_requested_repaint_for(&this_window),
            "set_stage must request a repaint of the window's own deferred \
             viewport, not rely on a bare request_repaint() (which resolves \
             to root from a background thread — see set_stage's own doc)"
        );
        assert!(
            !ctx.has_requested_repaint_for(&a_different_window),
            "must target only the given viewport, not a blanket repaint"
        );
        let guard = status.lock().unwrap();
        assert_eq!(guard.label, "identifying");
        assert_eq!(guard.fraction, Some(0.5));
    }

    /// A minimal `SpeakerIdVm` for state-transition tests that never touch
    /// the store/decoder/player (those need `SpeakerIdDeps`, exercised in
    /// `tests/kittest_speaker_id.rs` instead).
    fn test_vm() -> SpeakerIdVm {
        SpeakerIdVm {
            source: TranscriptionId::new(),
            title: String::new(),
            file: Some(PathBuf::from("/tmp/does-not-matter.wav")),
            stage: SpeakerIdStage::Idle,
            save_error: None,
            run: None,
            cancel_requested: false,
            sample_player: SamplePlayerState::NotStarted,
            sample_rx: None,
            sample_decode_cancel: CancelToken::new(),
            pending_play: None,
            stop_at: None,
            save_requested: false,
            close_requested: false,
            focus_requested: false,
        }
    }
}
