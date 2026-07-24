//! Transcript-viewer view-model and dependency bundle (F-080).
//!
//! [`ViewerVm`] is the per-window state one open transcript's viewer holds:
//! its in-memory segment rows (loaded once at open — design spec §1.2), the
//! active [`ViewMode`], the translation-available flag that gates the
//! Translation view/column/export entries, and the close-request flag the app
//! harvests each frame to tear the window down. [`ViewerDeps`] is the bundle
//! of `Arc`'d adapter handles the window's render closure needs; it is built
//! once (real adapters in `app.rs`, fakes in the kittest harness) and shared
//! across every open viewer.
//!
//! This module is state + store I/O and the playback view-model. It is almost
//! entirely egui-free; the one exception is [`VideoState::texture`] — a live
//! [`egui::TextureHandle`] that must persist on the VM across frames so the
//! video pane reuses one GPU allocation (Task 10, spec §2.5). The window chrome
//! lives in [`crate::views::viewer`], the editor table in
//! [`crate::views::viewer_table`], and the playback-controls bar in
//! [`crate::views::playback_controls`]. Task 8 adds the segment-editing state
//! ([`CellEditor`], [`ViewerVm::selected`]/[`ViewerVm::editing`]) and the
//! store-backed commit rules
//! ([`ViewerVm::apply_timestamp_edit`]/[`ViewerVm::apply_text_edit`]); Task 10
//! adds the synced-playback state ([`PlayerState`], [`VideoState`], loop/follow/
//! speed) whose master clock is [`humm_audio::Player::position`].
//!
//! **Store-failure surfacing (ADR-018):** the app's toast stack lives on
//! `HummApp` and is unreachable from a viewer's deferred-viewport render
//! closure (which captures only `Arc`-shared state — Task 7's handoff note). A
//! failed edit write therefore raises [`ViewerVm::unsaved`], which the viewer
//! renders as its own inline warning banner rather than an app toast — the
//! "inline banner is acceptable" fallback the task brief allows, keeping the
//! edit in memory and never crashing.
//!
//! Ported from Buzz's `TranscriptionViewerWidget.__init__`/
//! `_load_segments_and_settings`/`reset_view`
//! (`transcription_viewer_widget.py:78-143,1078-1145`).

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use humm_audio::Player;
use humm_core::boundary::{
    AudioDecoder, AudioOutput, FrameStream, MediaProber, PlaybackPcm, VideoStreamInfo,
    VideoStreamer,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{Millis, Segment, Status, TranscriptionId};
use humm_core::error::StoreError;
use humm_core::settings::Settings;
use humm_core::viewer::edit::{EditOutcome, Field, SegmentTimes, timestamp_edit};
use humm_core::viewer::search::{
    RowMatch, SEARCH_DEBOUNCE_MS, TextMatch, search_rows, search_text, wrap_next, wrap_prev,
};
use humm_core::viewer::text_view;
use humm_core::viewer::timestamp::parse_timestamp;
use humm_store::{SegmentRow, SqliteStore};

use crate::import::FileDialogs;

/// The bundle of adapter handles a viewer window's render closure needs.
///
/// Every field is an `Arc`'d trait object (or `Arc<SqliteStore>`) so the whole
/// bundle is cheap to clone into each deferred-viewport callback and satisfies
/// egui's `Send + Sync + 'static` requirement on that callback. The real set
/// (ffmpeg decoder/prober/video-streamer, cpal output, rfd dialogs) is built
/// in `app.rs`; the kittest harness substitutes fakes so `viewer_ui`/`ViewerVm`
/// are drivable headless without touching any OS boundary.
#[derive(Clone)]
pub struct ViewerDeps {
    /// The shared persistent store — segment reads at open, edit/export/resize
    /// writes in later tasks.
    pub store: Arc<SqliteStore>,
    /// Decodes the media to whole-file playback PCM (Task 10).
    pub decoder: Arc<dyn AudioDecoder>,
    /// Probes media for duration and video-stream dimensions (Task 10).
    pub prober: Arc<dyn MediaProber>,
    /// Opens the video frame stream for video media (Task 10).
    pub streamer: Arc<dyn VideoStreamer>,
    /// Opens the audio output device stream (Task 10).
    pub audio_output: Arc<dyn AudioOutput>,
    /// Native save dialog for viewer exports (Task 11).
    pub dialogs: Arc<dyn FileDialogs>,
}

/// The three transcript views (`transcription_view_mode_tool_button.py`,
/// `ViewMode`). Default is [`ViewMode::Timestamps`] — the editor table + media
/// player (`transcription_viewer_widget.py:102`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Editor table + media player + current-segment frame.
    #[default]
    Timestamps,
    /// Read-only paragraph-joined recognized text.
    Text,
    /// Read-only space-joined translation text (visible only when the
    /// transcript has translations).
    Translation,
}

/// The audio/video player's lifecycle (F-083, spec §2.5). The media is decoded
/// on a background thread at viewer open ([`ViewerVm::ensure_player_started`]);
/// the row renders a disabled "loading" affordance until the [`Player`] is
/// built ([`Self::Ready`]) or the decode fails ([`Self::Failed`], whose reason
/// names the cause — file missing / decode error — in the disabled-controls
/// tooltip). Everything else (table/edit/search/export) works regardless of
/// which state this is in.
pub enum PlayerState {
    /// Decode is in flight on the background thread; controls are disabled.
    Loading,
    /// The playback engine is live — its [`Player::position`] is the A/V
    /// master clock the scrubber, follow-audio, and video pane all read.
    Ready(Player),
    /// Decode (or output-stream open) failed; the string names the cause for
    /// the disabled-controls tooltip.
    Failed(String),
}

/// Live video-pane state (spec §2.5): the frame stream (audio-clock-driven),
/// the reused GPU texture, and the timestamp of the last frame uploaded (for
/// the drift log and the "keep last frame on a gap" rule). Present only for
/// video media whose stream opened successfully; audio-only transcripts leave
/// [`ViewerVm::video`] `None`.
pub struct VideoState {
    /// The open frame stream; `frame_for(audio position)` picks the frame to
    /// show each repaint, `seek` respawns it on a scrub/jump.
    pub stream: Box<dyn FrameStream>,
    /// The GPU texture the pane draws — reused across frames via
    /// [`egui::TextureHandle::set`] so a steady 25–60 fps upload never
    /// reallocates. `None` until the first frame is uploaded.
    pub texture: Option<egui::TextureHandle>,
    /// Presentation timestamp of the frame currently in [`Self::texture`];
    /// `None` before the first upload. Recorded against the audio position in
    /// the A/V drift log.
    pub last_frame_ts: Option<Millis>,
}

/// The result the background decode thread sends back: the decoded playback
/// PCM (or a stringified decode error) plus, for video media, the probed
/// stream dimensions used to open the frame stream on the UI thread.
struct PlaybackLoad {
    /// Decoded whole-file playback PCM, or the decode error's message.
    pcm: Result<PlaybackPcm, String>,
    /// Probed video-stream dimensions, `Some` only when the file is video and
    /// probing succeeded; the UI thread opens the [`FrameStream`] from it.
    video_info: Option<VideoStreamInfo>,
}

/// The env-gated A/V drift log (`HUMM_SMOKE_AV_DRIFT_LOG`, spec §2.5 acceptance
/// instrumentation): appends one `audio_ms,frame_ts_ms` line per rendered video
/// frame so the windowed 10-min ±50 ms p95 check (Task 14) has ground-truth
/// pairs. A buffered writer flushed on drop; absent unless the env var points
/// at a writable path and the media is video.
pub(crate) struct DriftLog {
    writer: BufWriter<std::fs::File>,
}

impl DriftLog {
    /// Opens (create-or-append) a drift log at `path`, or `None` if it cannot
    /// be opened. Factored from [`Self::from_env`] so tests exercise the
    /// write/format/flush path against a temp file without mutating the
    /// process environment (the `paragraph_split_from_env_value` convention —
    /// `std::env::set_var` is `unsafe` in edition 2024).
    pub(crate) fn open(path: &Path) -> Option<Self> {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(|file| DriftLog {
                writer: BufWriter::new(file),
            })
    }

    /// Opens the drift log named by `HUMM_SMOKE_AV_DRIFT_LOG`, or `None` when
    /// the env var is unset/empty or the path cannot be opened.
    fn from_env() -> Option<Self> {
        let path = std::env::var_os("HUMM_SMOKE_AV_DRIFT_LOG")?;
        if path.is_empty() {
            return None;
        }
        Self::open(Path::new(&path))
    }

    /// Appends one `audio_ms,frame_ts_ms` line (best-effort; a write error is
    /// dropped rather than crashing playback — this is test-only instrumentation).
    pub(crate) fn record(&mut self, audio_ms: i64, frame_ts_ms: i64) {
        let _ = writeln!(self.writer, "{audio_ms},{frame_ts_ms}");
    }
}

impl Drop for DriftLog {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}

/// Which of the four editor columns a cell editor targets (design spec §2.2:
/// Start | End | Text | Translation). Start/End edit timestamps; Text/
/// Translation edit free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCol {
    /// The segment's start timestamp.
    Start,
    /// The segment's end timestamp.
    End,
    /// The recognized text.
    Text,
    /// The translated text.
    Translation,
}

impl EditCol {
    /// The core [`Field`] this column maps its edit rule onto, or `None` for
    /// the two free-text columns (which never go through [`timestamp_edit`]).
    pub fn as_time_field(self) -> Option<Field> {
        match self {
            EditCol::Start => Some(Field::Start),
            EditCol::End => Some(Field::End),
            EditCol::Text | EditCol::Translation => None,
        }
    }

    /// Whether this column edits a timestamp (Start/End) rather than text.
    pub fn is_timestamp(self) -> bool {
        self.as_time_field().is_some()
    }
}

/// The single active inline cell editor, when the user is editing one cell.
///
/// Only ever one cell edits at a time (Buzz opens exactly one delegate editor),
/// so the whole table's editing state is this one optional value on
/// [`ViewerVm`] — every other cell renders as a plain label, which is what
/// keeps a 100k-row table cheap (design spec §2.2: "only the actively-edited
/// cell instantiates a `TextEdit`").
#[derive(Debug, Clone)]
pub struct CellEditor {
    /// Index into [`ViewerVm::rows`] of the row being edited.
    pub row: usize,
    /// Which column of that row is being edited.
    pub col: EditCol,
    /// The live editor text (the `TextEdit` buffer): filtered to `[0-9:.]` for
    /// timestamp columns (Buzz `QRegularExpressionValidator` parity), free for
    /// text columns.
    pub buffer: String,
    /// The last valid millisecond value for a timestamp column — the value a
    /// garbage buffer reverts to on commit (Buzz `TimeStampLineEdit`'s
    /// `_milliseconds`); also the base the in-editor `+`/`-` keys nudge from.
    /// Unused for text columns.
    pub last_valid_ms: Millis,
    /// Transient one-shot focus latch: true on the frame the editor opens so
    /// the table grabs keyboard focus exactly once. A UI detail, not part of
    /// the editor's semantic state — hence `pub(crate)`, set by [`Self::new`]
    /// and cleared by the table on first render.
    pub(crate) just_opened: bool,
}

impl CellEditor {
    /// Opens an editor for `col` on `row`, seeded with `buffer` and (for
    /// timestamp columns) a `last_valid_ms` revert target. The focus latch is
    /// armed so the table focuses the new `TextEdit` on its first frame.
    pub fn new(row: usize, col: EditCol, buffer: String, last_valid_ms: Millis) -> Self {
        CellEditor {
            row,
            col,
            buffer,
            last_valid_ms,
            just_opened: true,
        }
    }
}

/// The active search's result set (F-082, design spec §2.3): which pass
/// produced it determines how a match is applied. Timestamps mode searches
/// segment rows ([`search_rows`]) → [`RowMatch`]es select+center-scroll a
/// table row; Text/Translation mode searches the joined display text
/// ([`search_text`]) → [`TextMatch`]es highlight a byte range and scroll to
/// it. [`ViewerVm::run_search`] is the only place a variant is chosen (by
/// [`ViewMode`]), so a view never needs to guess which kind is live for the
/// current mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchResults {
    /// Timestamps-mode matches (row index + which column matched).
    Rows(Vec<RowMatch>),
    /// Text/Translation-mode matches (byte ranges into
    /// [`ViewerVm::text_body`]).
    Text(Vec<TextMatch>),
}

impl SearchResults {
    /// Number of matches, regardless of kind — the `total` [`humm_core::
    /// viewer::search::results_label`] wants.
    pub fn len(&self) -> usize {
        match self {
            SearchResults::Rows(rows) => rows.len(),
            SearchResults::Text(texts) => texts.len(),
        }
    }

    /// Whether there are no matches.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for SearchResults {
    /// An empty row-kind result set — the shape before any search has run
    /// (Timestamps is the viewer's default [`ViewMode`]).
    fn default() -> Self {
        SearchResults::Rows(Vec::new())
    }
}

/// The outcome of checking a pending search debounce this frame ([`debounce_decision`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebounceDecision {
    /// Not yet [`SEARCH_DEBOUNCE_MS`] since the last query edit — the caller
    /// should schedule a repaint for `remaining` so the fire frame happens
    /// even with no further input (`ctx.request_repaint_after`).
    Pending {
        /// Time left before the debounce is due.
        remaining: Duration,
    },
    /// The debounce delay has elapsed: fire only when the trimmed query has
    /// at least two characters *right now* — Buzz starts its timer only at
    /// ≥2 chars but never cancels a pending timer when the query shrinks
    /// back down before it fires (design spec §2.3, ledger #5); checking the
    /// length here, at fire time, instead of at edit time makes that
    /// deterministic rather than racy.
    Ready {
        /// Whether the search should actually run.
        should_search: bool,
    },
}

/// The pure debounce-gate rule behind [`DebounceDecision`]: a function of how
/// long it has been since the last query edit and how many (trimmed)
/// characters the query currently has — no clock, no sleeping, trivially
/// unit-testable with constructed [`Duration`]s.
pub fn debounce_decision(elapsed: Duration, trimmed_query_chars: usize) -> DebounceDecision {
    let debounce = Duration::from_millis(SEARCH_DEBOUNCE_MS);
    if elapsed < debounce {
        DebounceDecision::Pending {
            remaining: debounce - elapsed,
        }
    } else {
        DebounceDecision::Ready {
            should_search: trimmed_query_chars >= 2,
        }
    }
}

/// The transcript search bar's state (F-082, design spec §2.3). Lives on
/// [`ViewerVm`] so the semantic state transitions (open/close, clear, run,
/// navigate) are plain, unit-testable methods here; [`crate::views::
/// viewer_search`] only renders it and turns egui input (keystrokes,
/// shortcuts, frame timing) into calls against them — no matching rule lives
/// in the GUI (`humm_core::viewer::search` owns all of that).
///
/// Ported from Buzz's `search_text`/`search_results`/`current_search_index`/
/// `search_debounce_timer` instance attributes
/// (`transcription_viewer_widget.py:123-129`).
#[derive(Debug, Clone)]
pub struct SearchState {
    /// Whether the find bar is shown.
    pub visible: bool,
    /// The input field's live text.
    pub query: String,
    /// The current search's results — empty (and [`Self::searched`] false)
    /// before any search has run.
    pub results: SearchResults,
    /// Index of the highlighted match into [`Self::results`] (0-based).
    pub current: usize,
    /// When the query last changed, for the debounce gate
    /// ([`debounce_decision`]); `None` when no search is pending.
    pub last_edit: Option<Instant>,
    /// Whether [`ViewerVm::run_search`] has produced a result set since the
    /// bar was last opened/cleared — distinguishes "never searched yet"
    /// (blank results label) from "searched and found nothing" (Buzz's
    /// `"No matches found"`), since both states have empty `results`.
    pub searched: bool,
    /// One-shot latch: true when the bar's input should grab keyboard focus
    /// and select all its text on its next render (⌘F open, Clear button) —
    /// transient UI state, not semantic search state (the
    /// [`CellEditor::just_opened`] pattern).
    pub(crate) focus_pending: bool,
    /// One-shot latch: true when the current match changed (fresh search,
    /// next/prev navigation) and the mode's view (table or text body) must
    /// select/scroll to it on its next render. Cleared by whichever view
    /// consumes it.
    pub(crate) scroll_pending: bool,
}

impl SearchState {
    /// A search bar seeded from the persisted
    /// `settings.viewer.find_widget_visible` value at viewer-open time (Buzz
    /// `restore_ui_state`).
    pub fn new(visible: bool) -> Self {
        SearchState {
            visible,
            query: String::new(),
            results: SearchResults::default(),
            current: 0,
            last_edit: None,
            searched: false,
            focus_pending: false,
            scroll_pending: false,
        }
    }

    /// Opens the bar and arms the focus+select-all latch (Buzz
    /// `show_search_bar`/`focus_search_input`'s "show" branch). The query
    /// itself is untouched — it is already empty after any prior
    /// [`Self::hide`], and toggling back open mid-session never happens
    /// without a hide/clear between (Buzz parity).
    pub fn show(&mut self) {
        self.visible = true;
        self.focus_pending = true;
    }

    /// Hides the bar and clears the search entirely (Buzz `hide_search_bar`,
    /// which calls `clear_search()` before hiding). Unlike [`Self::clear`],
    /// this does *not* re-arm the focus latch — the input is about to be
    /// hidden, not refocused.
    pub fn hide(&mut self) {
        self.query.clear();
        self.results = SearchResults::default();
        self.current = 0;
        self.last_edit = None;
        self.searched = false;
        self.visible = false;
        self.focus_pending = false;
        self.scroll_pending = false;
    }

    /// Clears the query/results/highlight but keeps the bar visible and
    /// re-focuses the input (Buzz `clear_search`) — shared by the Clear
    /// button and by the query naturally emptying while typing
    /// (`on_search_text_changed`'s empty-text branch).
    pub fn clear(&mut self) {
        self.query.clear();
        self.results = SearchResults::default();
        self.current = 0;
        self.last_edit = None;
        self.searched = false;
        self.focus_pending = true;
        self.scroll_pending = false;
    }
}

/// One open transcript viewer's state.
///
/// Segments are loaded into [`Self::rows`] once at [`Self::open`] and every
/// read (table, text views, search, export) hits memory — nothing else mutates
/// a transcript's segments while its viewer is open in M4 (design spec §1.2).
pub struct ViewerVm {
    /// The transcript this window views.
    pub id: TranscriptionId,
    /// Window title = the input file's basename (Buzz `file_path_as_title`).
    pub title: String,
    /// Every segment row, in ascending id order (display order).
    pub rows: Vec<SegmentRow>,
    /// Whether any segment carries a non-blank translation — gates the
    /// Translation view/column/export entries. Recomputed when a translation
    /// cell edit lands (Task 8+).
    pub has_translations: bool,
    /// The active view mode.
    pub mode: ViewMode,
    /// Cached display text for [`ViewMode::Text`]/[`ViewMode::Translation`],
    /// recomputed on every [`Self::set_mode`] (Buzz recomputes it in
    /// `reset_view` on mode change, not per frame). Empty in Timestamps mode.
    pub text_body: String,
    /// Set true by the render closure when the OS window's close is requested;
    /// the app harvests it each frame to persist geometry and drop the window.
    pub close_requested: bool,
    /// Set true when the transcript is re-opened while its window already
    /// exists (deviation #1: focus the existing window instead of duplicating
    /// it). The render closure sends `ViewportCommand::Focus` and clears it.
    pub focus_requested: bool,
    /// One-shot latch set by the Resize toolbar button (F-086): the app
    /// harvests it each frame to open (or focus) this transcript's resizer
    /// window, then clears it (Buzz `on_resize_button_clicked`). The viewer's
    /// render closure only *requests* it — opening the resizer is an app-level
    /// action (it owns the resizer-window map), the same intent-latch shape as
    /// [`Self::close_requested`]/[`Self::focus_requested`].
    pub resize_requested: bool,
    /// The shared viewer geometry captured from settings at open, applied to
    /// this window's viewport builder for its whole lifetime (Buzz applies
    /// geometry once, at window open — `load_geometry`). Captured per-VM rather
    /// than re-read from settings each frame so that another viewer closing
    /// (which rewrites the shared geometry) never snaps this already-open
    /// window to the closer's size.
    pub initial_geometry: Option<humm_core::settings::ViewerGeometry>,
    /// Latest window geometry observed by the render closure (position/size),
    /// persisted as the shared viewer geometry when this window closes.
    /// `None` until the closure has seen a reported geometry at least once.
    pub observed_geometry: Option<humm_core::settings::ViewerGeometry>,
    /// Persisted-at-open playback-controls-bar visibility (Buzz
    /// `_load_segments_and_settings`); rendered by Task 10.
    pub playback_controls_visible: bool,
    /// Persisted-at-open find-bar visibility; rendered by Task 9.
    pub find_widget_visible: bool,
    /// Persisted-at-open loop-selected-segment toggle (Loop Segment checkbox).
    pub segment_looping_enabled: bool,
    /// Persisted-at-open follow-audio toggle (Follow Audio checkbox): when on,
    /// the playhead's segment is selected+centered as playback advances.
    pub follow_audio_enabled: bool,
    /// The input media file (from the task row), decoded for playback and, for
    /// video, probed + streamed. `None` for a since-deleted / URL-only row
    /// (defensive — the open gate only fires for a Completed/Skipped row, which
    /// has a file).
    pub file: Option<PathBuf>,
    /// The audio/video player's lifecycle state (F-083).
    pub player: PlayerState,
    /// Live video-pane state, `Some` only for playable video media (spec §2.5).
    pub video: Option<VideoState>,
    /// Current speed multiplier (Buzz `AUDIO_PLAYBACK_RATE`, persisted global),
    /// applied live to the player; seeded from `settings.viewer.playback_rate`.
    pub playback_rate: f32,
    /// The speed combo's editable-text buffer, seeded `"%.2fx"` from
    /// [`Self::playback_rate`] (Buzz `initialize_speed_control`).
    pub speed_text: String,
    /// The active loop range (segment `[start, end]`) when looping is engaged,
    /// mirrored into the player; the scrub-release ±2000 ms clear rule and the
    /// re-enable/jump logic read it (Buzz `range_ms`).
    pub loop_range: Option<(Millis, Millis)>,
    /// Persisted-at-open table/media splitter fraction for audio media (table's
    /// share of the vertical space; Buzz `audio_splitter_fraction`).
    pub audio_splitter_fraction: f32,
    /// Persisted-at-open table/media splitter fraction for video media.
    pub video_splitter_fraction: f32,
    /// Text of the segment under the playhead (or the selected segment), shown
    /// in the bottom current-segment frame; empty hides the frame.
    pub current_segment_text: String,
    /// One-shot: set to a row index when a genuine pointer click selected a
    /// table row this frame, consumed by the player's `on_segment_selected`
    /// (seek-if-outside + loop-range) logic so search/follow programmatic
    /// selection never triggers a seek.
    pub(crate) segment_click: Option<usize>,
    /// One-shot: a row the table should center-scroll to next frame, set by
    /// follow-audio / scroll-to-current / loop re-enable (the playback analogue
    /// of the search scroll latch).
    pub(crate) playback_scroll: Option<usize>,
    /// Whether the player was playing on the previous frame — drives the
    /// auto-show-controls-on-play transition (Buzz `on_audio_playback_state_changed`).
    was_playing: bool,
    /// The last position [`Self::sync_playback_position`] ran against, so the
    /// follow/frame update only runs when the playhead actually moved (Buzz's
    /// `positionChanged` signal semantics).
    last_synced_pos: Option<Millis>,
    /// Whether the background decode thread has been spawned (once per viewer).
    player_started: bool,
    /// Receiver for the background decode thread's [`PlaybackLoad`]; `Some`
    /// while [`PlayerState::Loading`].
    player_rx: Option<Receiver<PlaybackLoad>>,
    /// Cancels the background decode if the viewer closes mid-decode.
    decode_cancel: CancelToken,
    /// The env-gated A/V drift log (`HUMM_SMOKE_AV_DRIFT_LOG`), opened once the
    /// video stream is live and the env var is set.
    drift_log: Option<DriftLog>,
    /// Test-only (`HUMM_SMOKE_OPEN_VIEWER`, `app.rs::SmokeHooks`): when set by
    /// the smoke hook at open, the viewer auto-starts playback once the player
    /// is [`PlayerState::Ready`] and requests a normal window close once
    /// playback reaches end-of-media — the deferred-viewport analogue of
    /// `HUMM_SMOKE_OPEN_WINDOWS`, driving the windowed A/V-drift acceptance run
    /// (Task 14) without OS input injection. Never set on any production path
    /// (both constructors default it `false`).
    pub smoke_autoplay: bool,
    /// One-shot guard for [`Self::smoke_autoplay`]'s play-once: `true` after
    /// playback has been started, so the auto-close end-of-media check runs
    /// only after playback actually began (never on the initial `pos == 0`).
    smoke_play_started: bool,
    /// The single selected editor-table row, if any (Buzz single-row
    /// selection). The nudge shortcuts (⌘←/⌘→/⌘⇧←/⌘⇧→) act on this row.
    pub selected: Option<usize>,
    /// The active inline cell editor, if the user is editing one cell.
    pub editing: Option<CellEditor>,
    /// Set to `Some(now)` on every successful store write; drives the
    /// "edited · saved" autosave indicator, which shows for ~1.5 s then clears
    /// itself (design spec §2.2 / UX_SPEC J-6).
    pub saved_flash: Option<Instant>,
    /// Set true when a store write for a committed edit failed: the edit stays
    /// in memory (never lost) but is flagged unsaved, surfaced as an inline
    /// warning banner in the viewer (ADR-018 consistency — the app's toast
    /// stack lives on `HummApp` and is unreachable from a deferred-viewport
    /// callback, so the viewer carries its own banner; see the module note).
    /// Cleared by the next successful write.
    pub unsaved: bool,
    /// The transcript search bar's state (F-082).
    pub search: SearchState,
    /// Set to `Some((now, path))` by [`crate::views::export_menu`] on a
    /// successful export write: drives a self-clearing "written to `path`"
    /// flash next to the autosave indicator (design spec §2.4 / UX_SPEC J-6),
    /// the same `Instant`-timed mechanism as [`Self::saved_flash`]. Cleared by
    /// [`crate::views::export_menu`]'s own flash renderer once its duration
    /// elapses, or overwritten by the next successful export.
    pub export_flash: Option<(Instant, PathBuf)>,
    /// Set `true` when an export's file write failed: surfaced as a fixed,
    /// i18n'd (`viewer-export-failed`) inline warning banner — the exact same
    /// "banner, not toast" precedent [`Self::unsaved`] set (ADR-018: the
    /// app's toast stack is unreachable from a deferred-viewport render
    /// closure), and the same "the stored flag is a plain bool, the actual
    /// path/`io::Error` diagnostic is `log::warn!`-ed at the failure site,
    /// never rendered verbatim" shape `Self::unsaved` uses. Cleared by the
    /// next export attempt, success or failure.
    pub export_error: bool,
}

impl ViewerVm {
    /// Loads `id`'s transcript into a fresh viewer view-model: reads its
    /// segment rows once, derives the window title from the input file's
    /// basename, computes [`Self::has_translations`], and seeds the persisted
    /// UI toggles from `settings.viewer`. The initial mode is always
    /// [`ViewMode::Timestamps`] (Buzz sets `view_mode = TIMESTAMPS` before
    /// loading, `transcription_viewer_widget.py:102`).
    ///
    /// A store read error propagates; a since-deleted transcript (no task row,
    /// no segments) yields an empty-title, zero-row viewer rather than an error
    /// — the open paths only ever call this for a row they just saw completed,
    /// so that case is defensive, not expected.
    pub fn open(
        id: TranscriptionId,
        deps: &ViewerDeps,
        settings: &Settings,
    ) -> Result<Self, StoreError> {
        let rows = deps.store.get_segment_rows(id)?;
        let file = deps.store.get_task(&id)?.and_then(|task| task.file);
        let title = file.as_deref().map(file_basename).unwrap_or_default();
        let has_translations = rows.iter().any(|row| !row.translation.trim().is_empty());
        let playback_rate = settings.viewer.playback_rate;
        Ok(ViewerVm {
            id,
            title,
            rows,
            has_translations,
            mode: ViewMode::Timestamps,
            text_body: String::new(),
            close_requested: false,
            focus_requested: false,
            resize_requested: false,
            initial_geometry: settings.viewer.geometry,
            observed_geometry: None,
            playback_controls_visible: settings.viewer.playback_controls_visible,
            find_widget_visible: settings.viewer.find_widget_visible,
            segment_looping_enabled: settings.viewer.segment_looping_enabled,
            follow_audio_enabled: settings.viewer.follow_audio_enabled,
            file,
            player: PlayerState::Loading,
            video: None,
            playback_rate,
            speed_text: format!("{playback_rate:.2}x"),
            loop_range: None,
            audio_splitter_fraction: settings.viewer.audio_splitter_fraction,
            video_splitter_fraction: settings.viewer.video_splitter_fraction,
            current_segment_text: String::new(),
            segment_click: None,
            playback_scroll: None,
            was_playing: false,
            last_synced_pos: None,
            player_started: false,
            player_rx: None,
            decode_cancel: CancelToken::new(),
            drift_log: None,
            smoke_autoplay: false,
            smoke_play_started: false,
            selected: None,
            editing: None,
            saved_flash: None,
            unsaved: false,
            search: SearchState::new(settings.viewer.find_widget_visible),
            export_flash: None,
            export_error: false,
        })
    }

    /// Builds a [`ViewerVm`] directly from a pre-built segment-row list, with
    /// every UI toggle at its default and the player left in
    /// [`PlayerState::Loading`] (no background decode is spawned — nothing
    /// calls [`Self::ensure_player_started`]), bypassing [`Self::open`]'s
    /// store round-trip entirely.
    ///
    /// **Bench-only.** Not a production constructor: it exists so
    /// `benches/viewer_scroll.rs` can synthesize a 100 000-row editor-table
    /// view-model cheaply (every [`SegmentRow`] field is already `pub`, so
    /// the bench builds them directly) without paying for a 100 000-row
    /// `replace_segments` + `get_segment_rows` store round-trip just to
    /// populate bench data, and without duplicating this struct's private
    /// fields in the bench target. `#[doc(hidden)]` and the `_bench` name
    /// keep it out of the crate's normal public-API surface and obvious at
    /// every call site that it isn't one. Mirrors
    /// [`crate::viewmodel::TasksVm::for_bench`] (M3 `table_scroll` precedent).
    #[doc(hidden)]
    pub fn for_bench(rows: Vec<SegmentRow>) -> Self {
        let has_translations = rows.iter().any(|row| !row.translation.trim().is_empty());
        ViewerVm {
            id: TranscriptionId::new(),
            title: "bench".to_string(),
            rows,
            has_translations,
            mode: ViewMode::Timestamps,
            text_body: String::new(),
            close_requested: false,
            focus_requested: false,
            resize_requested: false,
            initial_geometry: None,
            observed_geometry: None,
            playback_controls_visible: false,
            find_widget_visible: false,
            segment_looping_enabled: false,
            follow_audio_enabled: false,
            file: None,
            player: PlayerState::Loading,
            video: None,
            playback_rate: 1.0,
            speed_text: "1.00x".to_string(),
            loop_range: None,
            audio_splitter_fraction: 0.95,
            video_splitter_fraction: 0.8,
            current_segment_text: String::new(),
            segment_click: None,
            playback_scroll: None,
            was_playing: false,
            last_synced_pos: None,
            player_started: false,
            player_rx: None,
            decode_cancel: CancelToken::new(),
            drift_log: None,
            smoke_autoplay: false,
            smoke_play_started: false,
            selected: None,
            editing: None,
            saved_flash: None,
            unsaved: false,
            search: SearchState::new(false),
            export_flash: None,
            export_error: false,
        }
    }

    /// Switches the active view mode and recomputes [`Self::text_body`] for the
    /// Text/Translation modes (Buzz `on_view_mode_changed` → `reset_view`).
    ///
    /// A [`ViewMode::Translation`] request is honored only when
    /// [`Self::has_translations`] — the menu entry is hidden otherwise, but the
    /// ⌘L shortcut is also gated at the call site, so this guard is defensive.
    pub fn set_mode(&mut self, mode: ViewMode) {
        if mode == ViewMode::Translation && !self.has_translations {
            return;
        }
        self.mode = mode;
        self.refresh_text_body();
        // Buzz `on_view_mode_changed` (`transcription_viewer_widget.py:
        // 1143-1145`): re-run the active search against the new mode's
        // haystack whenever the query is non-empty. This runs immediately,
        // with no debounce and no 2-char gate — those are keystroke-stream
        // concerns (design spec §2.3); a mode switch is a single discrete
        // action, ported from Buzz's own untamed `if self.search_text:`
        // check (which itself has no length gate).
        if !self.search.query.trim().is_empty() {
            self.run_search();
        }
    }

    /// Recomputes [`Self::text_body`] from the in-memory rows for the current
    /// mode. Timestamps mode has no combined text (the editor table renders the
    /// rows directly), so it clears the cache.
    fn refresh_text_body(&mut self) {
        self.text_body = match self.mode {
            ViewMode::Timestamps => String::new(),
            ViewMode::Text => {
                text_view::text_mode_text(&self.segments(), text_view::paragraph_split_from_env())
            }
            ViewMode::Translation => text_view::translation_mode_text(&self.segments()),
        };
    }

    /// The in-memory rows as plain [`Segment`]s (dropping the row id), for the
    /// core `text_view` builders and (Task 11) `views::export_menu`'s writers
    /// — the same in-memory mapping either way (design spec §2.4: export
    /// reads live rows, not a fresh store query). Rebuilt on each call — an
    /// infrequent, user-initiated event, never per frame.
    pub(crate) fn segments(&self) -> Vec<Segment> {
        self.rows
            .iter()
            .map(|row| Segment {
                start: row.start,
                end: row.end,
                text: row.text.clone(),
                translation: row.translation.clone(),
            })
            .collect()
    }

    /// Re-runs [`Self::search`] against the current [`Self::mode`]'s
    /// haystack (Buzz `perform_search`, `transcription_viewer_widget.py:
    /// 787-797`): Timestamps mode searches the in-memory rows
    /// ([`search_rows`]), Text/Translation mode searches the cached
    /// [`Self::text_body`] ([`search_text`]) — both share `humm_core`'s
    /// case-insensitive, [`humm_core::viewer::search::SEARCH_CAP`]-capped
    /// matching, this method only picks which one applies. Resets the
    /// highlighted match to the first result and — when there is at least
    /// one — arms the one-shot scroll latch so the mode's view selects/
    /// scrolls to it on its next render.
    pub fn run_search(&mut self) {
        let query = self.search.query.trim().to_string();
        let results = match self.mode {
            ViewMode::Timestamps => {
                let rows: Vec<(String, String)> = self
                    .rows
                    .iter()
                    .map(|row| (row.text.clone(), row.translation.clone()))
                    .collect();
                SearchResults::Rows(search_rows(&rows, self.has_translations, &query))
            }
            ViewMode::Text | ViewMode::Translation => {
                SearchResults::Text(search_text(&self.text_body, &query))
            }
        };
        self.search.current = 0;
        self.search.searched = true;
        self.search.scroll_pending = !results.is_empty();
        self.search.results = results;
    }

    /// Advances to the next match, wrapping ([`wrap_next`]; Buzz
    /// `search_next`/`search_next_if_results`). A no-op with no results.
    pub fn search_next(&mut self) {
        let total = self.search.results.len();
        if total == 0 {
            return;
        }
        self.search.current = wrap_next(self.search.current, total);
        self.search.scroll_pending = true;
    }

    /// Retreats to the previous match, wrapping ([`wrap_prev`]; Buzz
    /// `search_previous`/`search_previous_if_results`). A no-op with no
    /// results.
    pub fn search_prev(&mut self) {
        let total = self.search.results.len();
        if total == 0 {
            return;
        }
        self.search.current = wrap_prev(self.search.current, total);
        self.search.scroll_pending = true;
    }

    /// Commits a proposed new timestamp for `rows[idx]`'s `field`, running the
    /// pure core clamp/reject rule ([`timestamp_edit`]) over a live slice view
    /// of the in-memory rows, then — on `Apply` — persisting the value row (and
    /// the clamped neighbor, when the edit crossed a boundary) to the store and
    /// mutating memory to match. Returns whether an edit landed (`false` = the
    /// core rule rejected it: `start >= end` / `end <= start`; nothing written,
    /// no flash — Buzz's silent no-op).
    ///
    /// `floor` mirrors the core rule's `max(0, new_value)` clamp: the nudge
    /// path (⌘-arrows and the in-editor `+`/`-` keys) passes `true`; a direct
    /// cell commit passes `false` (Buzz `setModelData` never floors —
    /// `viewer::edit`'s doc).
    ///
    /// Store-write outcome (ADR-018): on success, `saved_flash` fires and the
    /// unsaved flag clears; on failure, the memory mutation is kept (the edit
    /// is never lost) and [`Self::unsaved`] is raised so the viewer surfaces
    /// its inline warning banner.
    pub fn apply_timestamp_edit(
        &mut self,
        deps: &ViewerDeps,
        idx: usize,
        field: Field,
        new_value: Millis,
        floor: bool,
    ) -> bool {
        let times: Vec<SegmentTimes> = self
            .rows
            .iter()
            .map(|row| SegmentTimes {
                id: row.id,
                start: row.start,
                end: row.end,
            })
            .collect();

        let (value, neighbor) = match timestamp_edit(&times, idx, field, new_value, floor) {
            EditOutcome::Rejected => return false,
            EditOutcome::Apply { value, neighbor } => (value, neighbor),
        };

        // Mutate the edited row in memory.
        match field {
            Field::Start => self.rows[idx].start = value,
            Field::End => self.rows[idx].end = value,
        }

        // Mutate the neighbor row in memory, if the edit crossed its boundary.
        // A Start edit only ever clamps the previous row (`idx - 1`); an End
        // edit only ever clamps the next row (`idx + 1`) — the two adjacencies
        // `timestamp_edit` can return.
        let neighbor_idx = neighbor.map(|(neighbor_id, neighbor_field, neighbor_value)| {
            let neighbor_idx = match field {
                Field::Start => idx - 1,
                Field::End => idx + 1,
            };
            debug_assert_eq!(self.rows[neighbor_idx].id, neighbor_id);
            match neighbor_field {
                Field::Start => self.rows[neighbor_idx].start = neighbor_value,
                Field::End => self.rows[neighbor_idx].end = neighbor_value,
            }
            neighbor_idx
        });

        // Persist: the value row (both times read back from memory) and, when
        // present, the clamped neighbor row. `id`/`start`/`end` are all `Copy`,
        // so this reads them out without cloning the row's text.
        let (edited_id, edited_start, edited_end) = {
            let row = &self.rows[idx];
            (row.id, row.start, row.end)
        };
        let mut ok = self.write_times(deps, edited_id, edited_start, edited_end);
        if let Some(neighbor_idx) = neighbor_idx {
            let (neighbor_id, neighbor_start, neighbor_end) = {
                let row = &self.rows[neighbor_idx];
                (row.id, row.start, row.end)
            };
            ok &= self.write_times(deps, neighbor_id, neighbor_start, neighbor_end);
        }
        self.record_save(ok);
        true
    }

    /// The nudge-shortcut path (⌘←/⌘→/⌘⇧←/⌘⇧→, design spec §2.2): shifts the
    /// *selected* row's `field` by `delta` ms, reading its current value fresh
    /// from memory so a second nudge in a row compounds on the first (floors at
    /// zero via the core rule). Returns whether an edit landed; a no-op with
    /// nothing selected.
    pub fn nudge_selected(&mut self, deps: &ViewerDeps, field: Field, delta: i64) -> bool {
        let Some(idx) = self.selected else {
            return false;
        };
        if idx >= self.rows.len() {
            return false;
        }
        let base = match field {
            Field::Start => self.rows[idx].start,
            Field::End => self.rows[idx].end,
        };
        self.apply_timestamp_edit(deps, idx, field, Millis(base.0 + delta), true)
    }

    /// Commits a Text or Translation cell edit: stores `text` as-typed (Buzz
    /// trims nothing) and mutates memory. A Translation edit whose text is
    /// non-blank raises [`Self::has_translations`] (unhiding the Translation
    /// view/column/export entries — spec ledger #7; never lowered, matching
    /// Buzz's `update_translation`). Store-write outcome handled as in
    /// [`Self::apply_timestamp_edit`].
    ///
    /// `col` must be [`EditCol::Text`] or [`EditCol::Translation`]; a timestamp
    /// column is a no-op (those commit through [`Self::apply_timestamp_edit`]).
    pub fn apply_text_edit(&mut self, deps: &ViewerDeps, idx: usize, col: EditCol, text: String) {
        let id = self.rows[idx].id;
        let ok = match col {
            EditCol::Text => {
                self.rows[idx].text = text.clone();
                self.write_result(deps.store.update_segment_text(id, &text))
            }
            EditCol::Translation => {
                self.rows[idx].translation = text.clone();
                if !text.trim().is_empty() {
                    self.has_translations = true;
                }
                self.write_result(deps.store.update_segment_translation(id, &text))
            }
            EditCol::Start | EditCol::End => return,
        };
        self.record_save(ok);
    }

    /// Playback hook — spec §2.2 `on_timestamp_being_edited`: while a Start/End
    /// cell editor is open, an active loop range follows the value in the editor
    /// buffer *live* (before commit). Returns the edited row, its [`Field`], and
    /// the buffer's current parse (floored at zero), or `None` when no timestamp
    /// cell is being edited. Consumed by [`Self::refresh_loop_range`], which
    /// applies this live buffer value to the armed loop bound each frame.
    pub fn editing_timestamp_preview(&self) -> Option<(usize, Field, Millis)> {
        let editor = self.editing.as_ref()?;
        let field = editor.col.as_time_field()?;
        let ms = parse_timestamp(&editor.buffer).unwrap_or(editor.last_valid_ms);
        Some((editor.row, field, Millis(ms.0.max(0))))
    }

    /// Persists one row's `start`/`end`, logging (not propagating) a store
    /// error and reporting success so the caller can flag an unsaved edit.
    fn write_times(&self, deps: &ViewerDeps, id: i64, start: Millis, end: Millis) -> bool {
        self.write_result(deps.store.update_segment_times(id, start, end))
    }

    /// Collapses a store write `Result` to a success bool, logging any error.
    fn write_result(&self, result: Result<(), StoreError>) -> bool {
        if let Err(err) = result {
            log::warn!("viewer edit: store write failed for {}: {err}", self.id);
            false
        } else {
            true
        }
    }

    /// Records a store-write outcome: success arms the autosave flash and
    /// clears the unsaved flag; failure keeps the (already-applied) memory edit
    /// and raises the unsaved flag for the warning banner.
    fn record_save(&mut self, ok: bool) {
        if ok {
            self.saved_flash = Some(Instant::now());
            self.unsaved = false;
        } else {
            self.saved_flash = None;
            self.unsaved = true;
        }
    }

    // -- Playback (F-083, spec §2.5) --------------------------------------

    /// Spawns the background decode (once) the first time the viewer renders —
    /// Buzz builds the media player at viewer open. Runs `decode_playback` (and,
    /// for video, `probe`) off the UI thread and sends the result back through
    /// [`Self::player_rx`]; [`Self::poll_player_load`] finishes the build on the
    /// UI thread. A `None` file (no media) fails the player immediately.
    pub fn ensure_player_started(&mut self, deps: &ViewerDeps, ctx: &egui::Context) {
        if self.player_started {
            return;
        }
        self.player_started = true;
        // Only the initial `Loading` state triggers a decode; a player already
        // resolved before the first render (a future retry path, or a test
        // seam that sets `player` directly) is left untouched.
        if !matches!(self.player, PlayerState::Loading) {
            return;
        }

        let Some(path) = self.file.clone() else {
            self.player = PlayerState::Failed("no media file for this transcript".to_string());
            return;
        };

        let decoder = Arc::clone(&deps.decoder);
        let prober = Arc::clone(&deps.prober);
        let cancel = self.decode_cancel.clone();
        let want_video = is_video(&path);
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.player_rx = Some(rx);

        std::thread::spawn(move || {
            let pcm = decoder
                .decode_playback(&path, &cancel)
                .map_err(|err| err.to_string());
            // Video info is best-effort — a probe failure degrades to
            // audio-only playback rather than failing the whole player.
            let video_info = if want_video {
                prober.probe(&path).ok().and_then(|info| info.video)
            } else {
                None
            };
            // The receiver may be gone if the viewer closed mid-decode; that is
            // fine (the send simply fails). Request a repaint so the load is
            // picked up promptly.
            let _ = tx.send(PlaybackLoad { pcm, video_info });
            ctx.request_repaint();
        });
    }

    /// Polls the background decode: on completion, builds the [`Player`] (and,
    /// for video, opens the frame stream + drift log) or records the failure.
    /// A no-op while still decoding.
    pub fn poll_player_load(&mut self, deps: &ViewerDeps) {
        let Some(rx) = &self.player_rx else {
            return;
        };
        let load = match rx.try_recv() {
            Ok(load) => load,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.player_rx = None;
                self.player = PlayerState::Failed("decode thread ended unexpectedly".to_string());
                return;
            }
        };
        self.player_rx = None;

        let pcm = match load.pcm {
            Ok(pcm) => pcm,
            Err(msg) => {
                self.player = PlayerState::Failed(msg);
                return;
            }
        };
        let player = match Player::new(pcm, Arc::clone(&deps.audio_output)) {
            Ok(player) => player,
            Err(err) => {
                self.player = PlayerState::Failed(err.to_string());
                return;
            }
        };
        player.set_speed(self.playback_rate);

        // Open the video frame stream from the probed info, if any. A stream
        // open error degrades to audio-only (no crash), matching the
        // graceful-degradation rule.
        if let (Some(info), Some(path)) = (load.video_info, self.file.clone()) {
            match deps.streamer.open(&path, Millis(0), &info) {
                Ok(stream) => {
                    self.video = Some(VideoState {
                        stream,
                        texture: None,
                        last_frame_ts: None,
                    });
                    self.drift_log = DriftLog::from_env();
                }
                Err(err) => log::warn!("viewer: could not open video stream: {err}"),
            }
        }

        self.player = PlayerState::Ready(player);
    }

    /// The player's current position, or `None` when it is not [`Ready`](PlayerState::Ready).
    pub fn player_position(&self) -> Option<Millis> {
        match &self.player {
            PlayerState::Ready(player) => Some(player.position()),
            _ => None,
        }
    }

    /// Whether the player is [`Ready`](PlayerState::Ready) and playing.
    pub fn is_playing(&self) -> bool {
        matches!(&self.player, PlayerState::Ready(player) if player.is_playing())
    }

    /// Toggles play/pause (⌘P). A no-op unless the player is ready.
    pub fn toggle_play(&mut self) {
        if let PlayerState::Ready(player) = &self.player {
            player.toggle();
        }
    }

    /// Test-only autoplay/auto-close driver for the `HUMM_SMOKE_OPEN_VIEWER`
    /// windowed A/V-drift acceptance run (Task 14). A no-op unless
    /// [`Self::smoke_autoplay`] was armed by the smoke hook at open.
    ///
    /// Called once per rendered frame (from [`crate::views::viewer::viewer_ui`],
    /// before the mode dispatch so it runs regardless of segment count/mode):
    /// once the player is [`PlayerState::Ready`] it starts playback exactly as
    /// a ⌘P press would (`Player::play`); once the position reaches
    /// end-of-media (the player pins its position at `duration` and stops on
    /// EOM) it sets [`Self::close_requested`], the same signal the OS window's
    /// close button raises — the parent frame harvests it and drops the VM,
    /// flushing the drift log via its `Drop`. Rides playback's own 16 ms
    /// repaint cadence, so no extra repaint plumbing is needed here.
    pub fn drive_smoke_playback(&mut self) {
        if !self.smoke_autoplay {
            return;
        }
        let (should_start, reached_end) = match &self.player {
            PlayerState::Ready(_) if !self.smoke_play_started => (true, false),
            PlayerState::Ready(player) => (false, player.position() >= player.duration()),
            _ => return,
        };
        if should_start {
            if let PlayerState::Ready(player) = &self.player {
                player.play();
            }
            self.smoke_play_started = true;
        } else if reached_end {
            self.close_requested = true;
        }
    }

    /// Seeks only the audio master clock (no video respawn). Used for the
    /// live-drag scrubber path, where seeking the video stream every frame would
    /// kill+respawn the ffmpeg subprocess dozens of times a second; the video
    /// stream is resynced once on drag release via [`Self::seek_playback`].
    pub fn seek_audio(&self, pos: Millis) {
        if let PlayerState::Ready(player) = &self.player {
            player.seek(pos);
        }
    }

    /// Seeks audio (the master clock) and respawns the video stream at `pos`
    /// (Buzz's kill+restart seek). A no-op on the player when not ready; the
    /// video stream still follows so a scrub before decode completes is a
    /// no-op rather than a desync.
    pub fn seek_playback(&mut self, pos: Millis) {
        self.seek_audio(pos);
        if let Some(video) = &mut self.video
            && let Err(err) = video.stream.seek(pos)
        {
            log::warn!("viewer: video seek failed: {err}");
        }
    }

    /// Pushes a loop range (or clears it) onto the player. A no-op when not ready.
    fn set_player_loop(&self, range: Option<(Millis, Millis)>) {
        if let PlayerState::Ready(player) = &self.player {
            player.set_loop(range);
        }
    }

    /// On scrubber release, clears the loop range when the release landed more
    /// than 2000 ms outside it (Buzz `on_slider_moved`, [`scrub_release_clears_loop`]).
    pub fn scrub_release(&mut self, pos: Millis) {
        if let Some(range) = self.loop_range
            && scrub_release_clears_loop(pos, range)
        {
            self.loop_range = None;
            self.set_player_loop(None);
        }
    }

    /// Replays the selected segment (⌘⇧P, Buzz `replay_current_segment`): seek
    /// to its start and play if paused. A no-op with nothing selected.
    pub fn replay_current_segment(&mut self) {
        let Some(idx) = self.selected else {
            return;
        };
        if idx >= self.rows.len() {
            return;
        }
        let start = self.rows[idx].start;
        self.seek_playback(start);
        if let PlayerState::Ready(player) = &self.player
            && !player.is_playing()
        {
            player.play();
        }
    }

    /// Applies a Loop Segment toggle (Buzz `on_loop_toggle_changed`): enabling
    /// with a selected segment sets the loop to it, re-highlights it, and — if
    /// playing outside the range — jumps to its start; disabling clears the
    /// loop.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        self.segment_looping_enabled = enabled;
        if !enabled {
            self.loop_range = None;
            self.set_player_loop(None);
            return;
        }
        let Some(idx) = self.selected else {
            return;
        };
        if idx >= self.rows.len() {
            return;
        }
        let (start, end) = (self.rows[idx].start, self.rows[idx].end);
        self.loop_range = Some((start, end));
        self.playback_scroll = Some(idx);
        self.set_player_loop(Some((start, end)));
        // If playing and outside the freshly-set range, jump to its start.
        if self.is_playing()
            && let Some(pos) = self.player_position()
            && (pos < start || pos > end)
        {
            self.seek_playback(start);
        }
    }

    /// Handles a genuine segment-row click (Buzz `on_segment_selected`): shows
    /// its text in the frame; seeks to its start if the playhead is outside
    /// `[start, end]`; sets the loop range when looping, else arms a scroll to
    /// the row.
    fn on_segment_selected(&mut self, idx: usize) {
        if idx >= self.rows.len() {
            return;
        }
        let (start, end) = (self.rows[idx].start, self.rows[idx].end);
        self.selected = Some(idx);
        self.current_segment_text = self.rows[idx].text.clone();

        if let Some(pos) = self.player_position()
            && (pos < start || pos > end)
        {
            self.seek_playback(start);
        }
        if self.segment_looping_enabled {
            self.loop_range = Some((start, end));
            self.set_player_loop(Some((start, end)));
        } else {
            self.playback_scroll = Some(idx);
        }
    }

    /// Consumes a pending pointer-click segment selection (set by the editor
    /// table) and runs the [`Self::on_segment_selected`] side effects. Called
    /// after the table renders so the click's selection is already applied.
    pub fn take_segment_click(&mut self) {
        if let Some(idx) = self.segment_click.take() {
            self.on_segment_selected(idx);
        }
    }

    /// Position-driven per-frame update (Buzz `on_audio_player_position_ms_changed`):
    /// refreshes the current-segment frame text and, when following audio,
    /// selects+scrolls the playhead's segment (unless looping pins the selection
    /// to the looped segment). Only runs when the position actually moved.
    pub fn sync_playback_position(&mut self, pos: Millis) {
        if self.last_synced_pos == Some(pos) {
            return;
        }
        self.last_synced_pos = Some(pos);

        let Some(idx) = segment_at(&self.rows, pos) else {
            return;
        };
        let text = &self.rows[idx].text;
        if self.current_segment_text != *text {
            self.current_segment_text = text.clone();
        }

        if !self.follow_audio_enabled {
            // Follow off: keep the selected row highlighted (no forced scroll —
            // Buzz re-scrolls to the selected row every tick, which fights a
            // user's own scrolling; not ported — deviation recorded in the M4
            // milestone ledger sweep, Task 14).
            return;
        }
        // Follow on: select+scroll the playhead's segment, unless looping pins
        // the selection to the looped segment and the playhead is still inside it.
        let pinned =
            self.segment_looping_enabled && self.selected.is_some() && self.selected == Some(idx);
        if !pinned {
            self.selected = Some(idx);
            self.playback_scroll = Some(idx);
        }
    }

    /// Scroll-to-current (⌘G / button, Buzz `on_scroll_to_current_button_clicked`,
    /// simplified per spec ledger #11): select+scroll the segment containing the
    /// playhead, else row 0. A no-op when the player is not ready.
    pub fn scroll_to_current(&mut self) {
        let Some(pos) = self.player_position() else {
            return;
        };
        if self.rows.is_empty() {
            return;
        }
        let idx = segment_at(&self.rows, pos).unwrap_or(0);
        self.selected = Some(idx);
        self.playback_scroll = Some(idx);
        self.current_segment_text = self.rows[idx].text.clone();
    }

    /// Sets the playback speed live and persists it (caller writes settings):
    /// clamps to `0.1..=5.0`, applies it to the player, and stores the value +
    /// its `"%.2fx"` echo in [`Self::speed_text`] when `echo` is set (the
    /// `−`/`+` button path always echoes; the combo path echoes only when the
    /// typed text lacked a trailing `x`).
    pub fn set_speed(&mut self, speed: f32, display: String) {
        let clamped = if speed.is_finite() {
            speed.clamp(0.1, 5.0)
        } else {
            self.playback_rate
        };
        self.playback_rate = clamped;
        self.speed_text = display;
        if let PlayerState::Ready(player) = &self.player {
            player.set_speed(clamped);
        }
    }

    /// Keeps the player's loop range *following* the selected segment while a
    /// loop is **already armed** — after a committed timestamp edit and *live*
    /// while a Start/End cell of the looped segment is being edited (Buzz
    /// `on_timestamp_being_edited` + the `_adjust_segment_timestamp` loop-range
    /// refresh). This is a maintenance pass, **never** an arming path: arming is
    /// owned by [`Self::on_segment_selected`] / [`Self::set_loop_enabled`], and
    /// disarming by [`Self::scrub_release`] / disabling the loop. Gating on
    /// `loop_range.is_some()` is what lets a scrub-release escape survive — the
    /// cleared range is not silently re-armed one frame later. Cheap enough to
    /// run every frame (two atomic stores).
    pub fn refresh_loop_range(&mut self) {
        // Only follow an armed range; do not arm a cleared one.
        if !self.segment_looping_enabled || self.loop_range.is_none() {
            return;
        }
        let Some(idx) = self.selected else {
            return;
        };
        if idx >= self.rows.len() {
            return;
        }
        let (mut start, mut end) = (self.rows[idx].start, self.rows[idx].end);
        // Live-follow the in-editor buffer before commit, if the open editor is
        // a timestamp cell of the selected (looped) segment.
        if let Some((row, field, ms)) = self.editing_timestamp_preview()
            && row == idx
        {
            match field {
                Field::Start => start = ms,
                Field::End => end = ms,
            }
        }
        if start < end {
            self.loop_range = Some((start, end));
            self.set_player_loop(Some((start, end)));
        }
    }

    /// Auto-shows the playback-controls bar the first frame playback starts in
    /// Timestamps mode (Buzz `on_audio_playback_state_changed`). The newly-set
    /// `playback_controls_visible` is persisted by the frame's diff-on-change
    /// `persist_playback_settings_if_changed`, not by this call.
    pub fn update_controls_autoshow(&mut self) {
        let playing = self.is_playing();
        let just_shown = playing
            && !self.was_playing
            && self.mode == ViewMode::Timestamps
            && !self.playback_controls_visible;
        if just_shown {
            self.playback_controls_visible = true;
        }
        self.was_playing = playing;
    }

    /// Records one `audio_ms,frame_ts_ms` drift-log line for the frame just
    /// rendered, when the env-gated log is open and a video frame is showing.
    pub fn record_drift(&mut self, audio_ms: Millis) {
        if let (Some(log), Some(video)) = (&mut self.drift_log, &self.video)
            && let Some(ts) = video.last_frame_ts
        {
            log.record(audio_ms.0, ts.0);
        }
    }
}

impl Drop for ViewerVm {
    fn drop(&mut self) {
        // Stop an in-flight background decode (the thread will find the token
        // canceled or its send target gone); the `Player`/`FrameStream`/
        // `DriftLog` each stop/flush via their own `Drop`.
        self.decode_cancel.cancel();
    }
}

/// Whether a transcript in `status` can be opened in the viewer: Buzz's gate
/// (`main_window.py:359-364`, `can_open_transcript`) — only completed or
/// skipped transcripts have segments to view.
pub fn can_open(status: Status) -> bool {
    matches!(status, Status::Completed | Status::Skipped)
}

/// The window title for a media file path: its basename (Buzz
/// `paths.file_path_as_title` = `os.path.basename`). A path with no final
/// component (e.g. a bare root) yields an empty string.
fn file_basename(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The current-segment display timestamp helper the viewer table/frame reuse:
/// `HH:MM:SS.mmm` (design spec §2.2). Kept here so later tasks share one
/// formatter with the editor.
pub fn display_timestamp(ms: Millis) -> String {
    humm_core::export::to_timestamp(ms, '.')
}

/// The video-extension set that puts the viewer in video mode (spec §2.5,
/// Buzz `is_video_file` / `file_transcriber.py:242`): case-insensitive match on
/// `{mp4 mov mkv avi m4v webm ogm wmv}`.
pub fn is_video(path: &Path) -> bool {
    const VIDEO_EXTS: [&str; 8] = ["mp4", "mov", "mkv", "avi", "m4v", "webm", "ogm", "wmv"];
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| VIDEO_EXTS.contains(&ext.as_str()))
}

/// The player row's time label — `HH:MM:SS / HH:MM:SS` (Buzz `update_time_label`:
/// `QTime(0,0).addMSecs(...).toString()`). Negative inputs floor at zero.
pub fn time_label(pos: Millis, dur: Millis) -> String {
    format!("{} / {}", hms(pos), hms(dur))
}

/// Formats a millisecond value as `HH:MM:SS` (zero-padded, floored at zero).
fn hms(ms: Millis) -> String {
    let total_secs = ms.0.max(0) / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

/// Binary-searches `rows` (ascending start order) for the segment containing
/// `pos` under half-open `[start, end)` semantics (Buzz `find_segment_index_at`),
/// or `None` when `pos` falls in no segment (a gap, or past the last end).
pub fn segment_at(rows: &[SegmentRow], pos: Millis) -> Option<usize> {
    // The number of rows whose start is `<= pos`; the candidate is the last of
    // them (the greatest start not after `pos`).
    let count = rows.partition_point(|row| row.start <= pos);
    if count == 0 {
        return None;
    }
    let idx = count - 1;
    (pos < rows[idx].end).then_some(idx)
}

/// Whether releasing the scrubber at `pos` should clear the loop `range` — when
/// the release lands more than 2000 ms outside `[start, end]` (Buzz
/// `on_slider_moved`: `pos < start-2000 || pos > end+2000`).
pub fn scrub_release_clears_loop(pos: Millis, range: (Millis, Millis)) -> bool {
    let (start, end) = range;
    pos.0 < start.0 - 2000 || pos.0 > end.0 + 2000
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::boundary::{
        FillCallback, FrameStream, MediaInfo, OutputStream, Pcm, PlaybackPcm, StreamSpec,
        TaskStore, VideoStreamInfo,
    };
    use humm_core::cancel::CancelToken;
    use humm_core::domain::{
        Backend, Input, ModelSize, ModelSpec, OutputSpec, Source, TaskKind, TaskSpec,
        TranscriptionOptions,
    };
    use humm_core::error::{AudioError, MediaError};
    use humm_core::viewer::edit::NUDGE_MS;
    use std::path::{Path, PathBuf};

    // -- fake adapters: never exercised by these VM tests, but ViewerDeps needs
    //    a full set (the real ones are all ZST-ish, but staying test-local keeps
    //    this crate's non-test code from depending on fakes). ------------------

    struct FakeDecoder;
    impl AudioDecoder for FakeDecoder {
        fn decode(&self, _path: &Path, _cancel: &CancelToken) -> Result<Pcm, MediaError> {
            unreachable!("no VM test decodes")
        }
        fn decode_playback(
            &self,
            _path: &Path,
            _cancel: &CancelToken,
        ) -> Result<PlaybackPcm, MediaError> {
            unreachable!("no VM test decodes")
        }
    }

    struct FakeProber;
    impl MediaProber for FakeProber {
        fn probe(&self, _path: &Path) -> Result<MediaInfo, MediaError> {
            unreachable!("no VM test probes")
        }
    }

    struct FakeStreamer;
    impl VideoStreamer for FakeStreamer {
        fn open(
            &self,
            _path: &Path,
            _start: Millis,
            _info: &VideoStreamInfo,
        ) -> Result<Box<dyn FrameStream>, MediaError> {
            unreachable!("no VM test streams video")
        }
    }

    struct FakeOutput;
    impl AudioOutput for FakeOutput {
        fn open(
            &self,
            _spec: StreamSpec,
            _fill: FillCallback,
        ) -> Result<Box<dyn OutputStream>, AudioError> {
            unreachable!("no VM test opens audio")
        }
    }

    struct FakeDialogs;
    impl FileDialogs for FakeDialogs {
        fn pick_files(&self, _start_dir: Option<&Path>) -> Option<Vec<PathBuf>> {
            None
        }
        fn pick_folder(&self, _start_dir: Option<&Path>) -> Option<PathBuf> {
            None
        }
        fn save_file(
            &self,
            _title: &str,
            _default_path: &Path,
            _filter_name: &str,
            _extension: &str,
        ) -> Option<PathBuf> {
            None
        }
    }

    fn deps(store: Arc<SqliteStore>) -> ViewerDeps {
        ViewerDeps {
            store,
            decoder: Arc::new(FakeDecoder),
            prober: Arc::new(FakeProber),
            streamer: Arc::new(FakeStreamer),
            audio_output: Arc::new(FakeOutput),
            dialogs: Arc::new(FakeDialogs),
        }
    }

    fn seg(start: i64, end: i64, text: &str, translation: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.into(),
            translation: translation.into(),
        }
    }

    fn spec_for(file: &str) -> TaskSpec {
        TaskSpec {
            input: Input::Path(PathBuf::from(file)),
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
                temperature: vec![0.0],
                llm: None,
            },
            output: OutputSpec::default(),
        }
    }

    /// Creates a completed transcription with `file` as its input and `segs`
    /// as its segments, returning its id.
    fn seed(store: &SqliteStore, file: &str, segs: &[Segment]) -> TranscriptionId {
        let id = TranscriptionId::new();
        store
            .create_task(&id, &spec_for(file), Source::FileImport)
            .expect("create task");
        store.mark_completed(&id, segs).expect("mark completed");
        id
    }

    #[test]
    fn can_open_matrix_matches_buzz_gate() {
        assert!(can_open(Status::Completed));
        assert!(can_open(Status::Skipped));
        assert!(!can_open(Status::Queued));
        assert!(!can_open(Status::InProgress));
        assert!(!can_open(Status::Failed));
        assert!(!can_open(Status::Canceled));
    }

    #[test]
    fn open_loads_rows_and_flags_translations() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let with_tr = seed(
            &store,
            "/tmp/a.wav",
            &[seg(0, 1000, "hello", ""), seg(1000, 2000, "world", "hola")],
        );
        let without_tr = seed(
            &store,
            "/tmp/b.wav",
            &[seg(0, 1000, "hello", ""), seg(1000, 2000, "world", "   ")],
        );
        let deps = deps(Arc::clone(&store));
        let settings = Settings::default();

        let vm = ViewerVm::open(with_tr, &deps, &settings).expect("open");
        assert_eq!(vm.rows.len(), 2);
        assert_eq!(vm.rows[0].text, "hello");
        assert!(
            vm.has_translations,
            "a non-blank translation must set has_translations"
        );
        assert_eq!(vm.mode, ViewMode::Timestamps);

        let vm2 = ViewerVm::open(without_tr, &deps, &settings).expect("open");
        assert!(
            !vm2.has_translations,
            "only whitespace translations must not count as translations"
        );
    }

    #[test]
    fn title_is_file_basename() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/some/nested/dir/My Recording.mp3",
            &[seg(0, 1, "x", "")],
        );
        let deps = deps(Arc::clone(&store));
        let vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        assert_eq!(vm.title, "My Recording.mp3");
    }

    #[test]
    fn open_seeds_persisted_toggles_from_settings() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(&store, "/tmp/a.wav", &[seg(0, 1, "x", "")]);
        let deps = deps(Arc::clone(&store));
        let mut settings = Settings::default();
        settings.viewer.find_widget_visible = true;
        settings.viewer.playback_controls_visible = true;
        settings.viewer.segment_looping_enabled = true;

        let vm = ViewerVm::open(id, &deps, &settings).expect("open");
        assert!(vm.find_widget_visible);
        assert!(vm.playback_controls_visible);
        assert!(vm.segment_looping_enabled);
    }

    #[test]
    fn set_mode_text_builds_paragraph_joined_body() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[
                seg(0, 1000, "hello", "hola"),
                seg(1000, 2000, "world", "mundo"),
            ],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        vm.set_mode(ViewMode::Text);
        // Gap (1000-1000 = 0) < 2000 default split → single space join.
        assert_eq!(vm.text_body, "hello world");

        vm.set_mode(ViewMode::Translation);
        assert_eq!(vm.text_body, "hola mundo");

        vm.set_mode(ViewMode::Timestamps);
        assert_eq!(vm.text_body, "");
    }

    #[test]
    fn set_mode_translation_ignored_without_translations() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(&store, "/tmp/a.wav", &[seg(0, 1000, "hello", "")]);
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        vm.set_mode(ViewMode::Translation);
        // Guard holds: mode stays Timestamps, body stays empty.
        assert_eq!(vm.mode, ViewMode::Timestamps);
        assert_eq!(vm.text_body, "");
    }

    /// The [`SegmentRow::id`]s the store issued for a freshly-seeded transcript,
    /// in ascending order — the handles the edit APIs address rows by.
    fn row_ids(store: &SqliteStore, id: TranscriptionId) -> Vec<i64> {
        store
            .get_segment_rows(id)
            .expect("rows")
            .into_iter()
            .map(|row| row.id)
            .collect()
    }

    #[test]
    fn apply_timestamp_edit_writes_value_and_clamped_neighbor_to_store() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        // Two abutting segments; editing row 1's start below row 0's end must
        // clamp row 0's end to match (the overlap rule).
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[seg(0, 1000, "a", ""), seg(1000, 2000, "b", "")],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        // Drop row 1's start to 800: below row 0's end (1000) → row 0's end is
        // pulled back to 800 too.
        let landed = vm.apply_timestamp_edit(&deps, 1, Field::Start, Millis(800), false);
        assert!(landed, "a valid start edit must land");

        // Memory: both rows moved.
        assert_eq!(vm.rows[1].start, Millis(800));
        assert_eq!(vm.rows[0].end, Millis(800));
        // A successful write arms the flash and leaves nothing unsaved.
        assert!(vm.saved_flash.is_some());
        assert!(!vm.unsaved);

        // Store: both rows persisted — proven by a fresh read, not memory.
        let persisted = store.get_segment_rows(id).expect("reread");
        assert_eq!(persisted[0].end, Millis(800), "neighbor end persisted");
        assert_eq!(persisted[1].start, Millis(800), "value start persisted");
        // The untouched fields are intact.
        assert_eq!(persisted[0].start, Millis(0));
        assert_eq!(persisted[1].end, Millis(2000));
    }

    #[test]
    fn apply_timestamp_edit_rejected_leaves_store_and_flash_untouched() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(&store, "/tmp/a.wav", &[seg(0, 1000, "a", "")]);
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        // start >= end → rejected: no write, no flash, memory unchanged.
        let landed = vm.apply_timestamp_edit(&deps, 0, Field::Start, Millis(1000), false);
        assert!(!landed);
        assert_eq!(vm.rows[0].start, Millis(0));
        assert!(vm.saved_flash.is_none());
        assert_eq!(
            store.get_segment_rows(id).expect("reread")[0].start,
            Millis(0)
        );
    }

    #[test]
    fn apply_text_edit_translation_unhides_translation_column() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        // No translations at load → the Translation column/menu/export are
        // hidden.
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[seg(0, 1000, "hello", ""), seg(1000, 2000, "world", "")],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        assert!(!vm.has_translations);

        vm.apply_text_edit(&deps, 0, EditCol::Translation, "hola".to_string());

        // The flag flips → the Translation column unhides.
        assert!(vm.has_translations);
        assert_eq!(vm.rows[0].translation, "hola");
        // Persisted (fresh read).
        let ids = row_ids(&store, id);
        let persisted = store.get_segment_rows(id).expect("reread");
        assert_eq!(persisted[0].translation, "hola");
        assert_eq!(persisted[0].id, ids[0]);
        // A blank translation edit never lowers the flag (Buzz parity).
        vm.apply_text_edit(&deps, 0, EditCol::Translation, "   ".to_string());
        assert!(vm.has_translations, "flag is never lowered once raised");
    }

    #[test]
    fn apply_text_edit_stores_text_as_typed_without_trimming() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(&store, "/tmp/a.wav", &[seg(0, 1000, "hello", "")]);
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        vm.apply_text_edit(&deps, 0, EditCol::Text, "  padded  ".to_string());
        assert_eq!(vm.rows[0].text, "  padded  ");
        assert_eq!(
            store.get_segment_rows(id).expect("reread")[0].text,
            "  padded  ",
            "Buzz stores text as-typed — no trim"
        );
    }

    #[test]
    fn nudge_uses_fresh_values_after_prior_edit() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        // One wide segment so two +500 ms end nudges both stay valid.
        let id = seed(&store, "/tmp/a.wav", &[seg(0, 1000, "a", "")]);
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        vm.selected = Some(0);

        // First nudge: end 1000 → 1500.
        assert!(vm.nudge_selected(&deps, Field::End, NUDGE_MS));
        assert_eq!(vm.rows[0].end, Millis(1500));
        // Second nudge reads the *updated* 1500, not the stale 1000 → 2000.
        assert!(vm.nudge_selected(&deps, Field::End, NUDGE_MS));
        assert_eq!(vm.rows[0].end, Millis(2000));

        // And the fresh value is what reached the store.
        assert_eq!(
            store.get_segment_rows(id).expect("reread")[0].end,
            Millis(2000)
        );
    }

    #[test]
    fn nudge_start_floors_at_zero() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(&store, "/tmp/a.wav", &[seg(300, 5000, "a", "")]);
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        vm.selected = Some(0);

        // start 300 − 500 = −200 → floored to 0 (nudge path, floor = true).
        assert!(vm.nudge_selected(&deps, Field::Start, -NUDGE_MS));
        assert_eq!(vm.rows[0].start, Millis(0));
    }

    #[test]
    fn nudge_without_selection_is_a_noop() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(&store, "/tmp/a.wav", &[seg(0, 1000, "a", "")]);
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        assert!(vm.selected.is_none());
        assert!(!vm.nudge_selected(&deps, Field::End, NUDGE_MS));
        assert_eq!(vm.rows[0].end, Millis(1000));
    }

    // -- F-082 search (Task 9) ------------------------------------------

    #[test]
    fn debounce_fires_only_after_300ms_and_2_chars() {
        // Under the debounce window → always pending, regardless of length,
        // and the caller must reschedule for exactly what's left.
        assert_eq!(
            debounce_decision(Duration::from_millis(299), 5),
            DebounceDecision::Pending {
                remaining: Duration::from_millis(1)
            }
        );
        assert_eq!(
            debounce_decision(Duration::from_millis(0), 5),
            DebounceDecision::Pending {
                remaining: Duration::from_millis(SEARCH_DEBOUNCE_MS)
            }
        );

        // At/after the window, but under 2 trimmed chars → ready, but the
        // deterministic length gate says don't search (ledger #5).
        assert_eq!(
            debounce_decision(Duration::from_millis(300), 0),
            DebounceDecision::Ready {
                should_search: false
            }
        );
        assert_eq!(
            debounce_decision(Duration::from_millis(500), 1),
            DebounceDecision::Ready {
                should_search: false
            }
        );

        // At/after the window with ≥2 chars → ready and should search.
        assert_eq!(
            debounce_decision(Duration::from_millis(300), 2),
            DebounceDecision::Ready {
                should_search: true
            }
        );
        assert_eq!(
            debounce_decision(Duration::from_secs(5), 10),
            DebounceDecision::Ready {
                should_search: true
            }
        );
    }

    #[test]
    fn mode_switch_reruns_search() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[
                seg(0, 1000, "cat sat", "gato"),
                seg(1000, 2000, "dog ran", "perro"),
            ],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        vm.search.query = "cat".to_string();
        vm.run_search();
        assert_eq!(
            vm.search.results,
            SearchResults::Rows(vec![RowMatch {
                row: 0,
                in_translation: false
            }])
        );

        // Switching to Text mode re-runs the *same* query against the new
        // mode's haystack ("cat sat dog ran") — same one match, but now a
        // Text-kind (byte-range) result, not a Row-kind one.
        vm.set_mode(ViewMode::Text);
        assert_eq!(vm.mode, ViewMode::Text);
        match &vm.search.results {
            SearchResults::Text(matches) => {
                assert_eq!(matches.len(), 1);
                assert_eq!(&vm.text_body[matches[0].start..matches[0].end], "cat");
            }
            SearchResults::Rows(_) => panic!("Text mode must produce Text-kind results"),
        }

        // Switching mode with an empty query is a no-op on the search state
        // (Buzz's `if self.search_text:` guard).
        vm.search.query.clear();
        vm.search.results = SearchResults::Rows(vec![]);
        vm.set_mode(ViewMode::Timestamps);
        assert_eq!(vm.search.results, SearchResults::Rows(vec![]));
    }

    #[test]
    fn esc_clears_and_hides() {
        let mut search = SearchState::new(true);
        search.query = "needle".to_string();
        search.results = SearchResults::Rows(vec![RowMatch {
            row: 3,
            in_translation: false,
        }]);
        search.current = 1;
        search.searched = true;
        search.last_edit = Some(Instant::now());

        search.hide();

        assert!(!search.visible, "Esc hides the bar");
        assert!(search.query.is_empty(), "Esc clears the query");
        assert!(search.results.is_empty(), "Esc clears the results");
        assert_eq!(search.current, 0);
        assert!(!search.searched);
        assert!(search.last_edit.is_none(), "no debounce stays pending");
        assert!(
            !search.focus_pending,
            "the input is being hidden, not refocused"
        );
    }

    #[test]
    fn clear_keeps_bar_and_focus_flag() {
        let mut search = SearchState::new(true);
        search.query = "needle".to_string();
        search.results = SearchResults::Text(vec![TextMatch { start: 0, end: 6 }]);
        search.current = 0;
        search.searched = true;
        search.focus_pending = false;

        search.clear();

        assert!(search.visible, "Clear must not hide the bar");
        assert!(search.query.is_empty());
        assert!(search.results.is_empty());
        assert_eq!(search.current, 0);
        assert!(!search.searched);
        assert!(
            search.focus_pending,
            "Clear re-focuses the input (Buzz `clear_search`'s `setFocus()`)"
        );
    }

    // -- F-083 playback: pure helpers ------------------------------------

    fn seg_row(start: i64, end: i64, text: &str) -> SegmentRow {
        SegmentRow {
            id: 0,
            start: Millis(start),
            end: Millis(end),
            text: text.into(),
            translation: String::new(),
        }
    }

    #[test]
    fn time_label_formats_hh_mm_ss_and_floors_negatives() {
        assert_eq!(time_label(Millis(0), Millis(0)), "00:00:00 / 00:00:00");
        // 1h 1m 1s / 2h 2m 2s.
        assert_eq!(
            time_label(Millis(3_661_000), Millis(7_322_000)),
            "01:01:01 / 02:02:02"
        );
        assert_eq!(time_label(Millis(-5), Millis(1000)), "00:00:00 / 00:00:01");
    }

    #[test]
    fn segment_at_binary_search_half_open_intervals() {
        let rows = vec![
            seg_row(0, 1000, "a"),
            seg_row(1000, 2000, "b"),
            seg_row(2000, 3000, "c"),
        ];
        assert_eq!(segment_at(&rows, Millis(0)), Some(0));
        assert_eq!(segment_at(&rows, Millis(999)), Some(0));
        // Half-open: 1000 belongs to the next segment, not the previous.
        assert_eq!(segment_at(&rows, Millis(1000)), Some(1));
        assert_eq!(segment_at(&rows, Millis(1500)), Some(1));
        assert_eq!(segment_at(&rows, Millis(2999)), Some(2));
        // Past the last end → no segment.
        assert_eq!(segment_at(&rows, Millis(3000)), None);
    }

    #[test]
    fn segment_at_returns_none_in_a_gap_or_empty() {
        let gapped = vec![seg_row(0, 1000, "a"), seg_row(2000, 3000, "c")];
        assert_eq!(segment_at(&gapped, Millis(1500)), None);
        assert_eq!(segment_at(&[], Millis(0)), None);
    }

    #[test]
    fn scrub_release_clears_loop_only_more_than_2000ms_outside() {
        let range = (Millis(5000), Millis(7000));
        assert!(!scrub_release_clears_loop(Millis(6000), range), "inside");
        assert!(
            !scrub_release_clears_loop(Millis(3000), range),
            "exactly -2000"
        );
        assert!(
            scrub_release_clears_loop(Millis(2999), range),
            "more than 2000 below"
        );
        assert!(
            !scrub_release_clears_loop(Millis(9000), range),
            "exactly +2000"
        );
        assert!(
            scrub_release_clears_loop(Millis(9001), range),
            "more than 2000 above"
        );
    }

    #[test]
    fn is_video_matches_the_extension_set_case_insensitively() {
        for ext in ["mp4", "mov", "mkv", "avi", "m4v", "webm", "ogm", "wmv"] {
            assert!(is_video(Path::new(&format!("clip.{ext}"))), "{ext}");
            assert!(
                is_video(Path::new(&format!("clip.{}", ext.to_uppercase()))),
                "{ext} uppercase"
            );
        }
        for ext in ["wav", "mp3", "m4a", "flac", "ogg"] {
            assert!(!is_video(Path::new(&format!("song.{ext}"))), "{ext}");
        }
        assert!(!is_video(Path::new("no-extension")));
    }

    #[test]
    fn drift_log_writes_pairs_when_env_set() {
        // Exercises the write/format/flush path against a temp file — the
        // `paragraph_split_from_env_value` convention of injecting the value
        // rather than mutating the (edition-2024-`unsafe`) process env.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("drift.csv");
        {
            let mut log = DriftLog::open(&path).expect("open drift log");
            log.record(10, 5);
            log.record(1234, 1200);
        } // drop flushes the buffered writer
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "10,5\n1234,1200\n"
        );
    }

    // -- F-083 playback: state machine (MockOutput-backed Player) ---------

    /// A test [`AudioOutput`] that captures the `fill` closure so the test can
    /// pull buffers synchronously and advance the master clock — the gui-side
    /// analogue of humm-audio's test-internal `MockOutput`.
    struct CapturingOutput {
        slot: Arc<std::sync::Mutex<Option<humm_core::boundary::FillCallback>>>,
    }
    struct NullStream;
    impl humm_core::boundary::OutputStream for NullStream {
        fn play(&mut self) -> Result<(), humm_core::error::AudioError> {
            Ok(())
        }
        fn pause(&mut self) -> Result<(), humm_core::error::AudioError> {
            Ok(())
        }
    }
    impl AudioOutput for CapturingOutput {
        fn open(
            &self,
            _spec: humm_core::boundary::StreamSpec,
            fill: humm_core::boundary::FillCallback,
        ) -> Result<Box<dyn humm_core::boundary::OutputStream>, humm_core::error::AudioError>
        {
            *self.slot.lock().unwrap() = Some(fill);
            Ok(Box::new(NullStream))
        }
    }

    /// Builds a ready [`Player`] over a `frames`-ms ramp (mono, 1 kHz → 1 frame
    /// = 1 ms) and returns it with its captured `fill` closure.
    fn ready_player(frames: usize) -> (Player, humm_core::boundary::FillCallback) {
        let samples: Vec<f32> = (0..frames).map(|i| i as f32).collect();
        let pcm = PlaybackPcm::new(samples, 1, 1000);
        let slot = Arc::new(std::sync::Mutex::new(None));
        let output = CapturingOutput {
            slot: Arc::clone(&slot),
        };
        let player = Player::new(pcm, Arc::new(output)).expect("player opens");
        let fill = slot.lock().unwrap().take().expect("fill captured");
        (player, fill)
    }

    #[test]
    fn loop_toggle_reenable_jumps_when_playing_outside_range() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[
                seg(0, 1000, "a", ""),
                seg(1000, 2000, "b", ""),
                seg(2000, 3000, "c", ""),
            ],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        let (player, mut fill) = ready_player(5000);
        player.play();
        // Advance the playhead well past the future loop's end (2000 ms).
        let mut buf = vec![0.0f32; 3000];
        fill(&mut buf);
        vm.player = PlayerState::Ready(player);
        vm.selected = Some(1); // segment [1000, 2000]
        assert!(
            vm.player_position().expect("ready").0 >= 2000,
            "playhead must be outside the loop range before re-enable"
        );

        // Re-enable looping: the range is set and, since we're playing outside
        // it, the playhead jumps to the loop start.
        vm.set_loop_enabled(true);
        assert_eq!(vm.loop_range, Some((Millis(1000), Millis(2000))));
        assert_eq!(
            vm.player_position(),
            Some(Millis(1000)),
            "re-enabling the loop while playing outside it jumps to the start"
        );
        assert_eq!(vm.playback_scroll, Some(1));
    }

    #[test]
    fn follow_audio_highlights_containing_segment() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[
                seg(0, 1000, "a", ""),
                seg(1000, 2000, "b", ""),
                seg(2000, 3000, "c", ""),
            ],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");

        let (player, _fill) = ready_player(5000);
        vm.player = PlayerState::Ready(player);
        vm.follow_audio_enabled = true;
        vm.segment_looping_enabled = false;

        // A position inside the middle segment selects+scrolls that row and
        // updates the current-segment frame text.
        vm.sync_playback_position(Millis(1500));
        assert_eq!(vm.selected, Some(1));
        assert_eq!(vm.playback_scroll, Some(1));
        assert_eq!(vm.current_segment_text, "b");

        // Advancing into the last segment follows the playhead there.
        vm.sync_playback_position(Millis(2500));
        assert_eq!(vm.selected, Some(2));
        assert_eq!(vm.current_segment_text, "c");
    }

    #[test]
    fn follow_audio_off_keeps_selection() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[seg(0, 1000, "a", ""), seg(1000, 2000, "b", "")],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        let (player, _fill) = ready_player(5000);
        vm.player = PlayerState::Ready(player);
        vm.follow_audio_enabled = false;
        vm.selected = Some(0);

        // Position moves into segment 1, but follow is off → selection unchanged.
        vm.sync_playback_position(Millis(1500));
        assert_eq!(vm.selected, Some(0), "follow off keeps the selected row");
        assert!(vm.playback_scroll.is_none());
        // The current-segment frame text still tracks the playhead (Buzz updates
        // it regardless of follow).
        assert_eq!(vm.current_segment_text, "b");
    }

    #[test]
    fn segment_click_seeks_when_playhead_outside_and_sets_loop() {
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[seg(0, 1000, "a", ""), seg(2000, 3000, "c", "")],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        let (player, _fill) = ready_player(5000);
        vm.player = PlayerState::Ready(player);
        vm.segment_looping_enabled = true;

        // Playhead at 0 (inside segment 0); clicking segment 1 ([2000,3000])
        // is outside → seek to its start, and looping → set the loop range.
        vm.segment_click = Some(1);
        vm.take_segment_click();
        assert_eq!(vm.selected, Some(1));
        assert_eq!(vm.current_segment_text, "c");
        assert_eq!(vm.player_position(), Some(Millis(2000)));
        assert_eq!(vm.loop_range, Some((Millis(2000), Millis(3000))));
    }

    #[test]
    fn scrub_release_escape_survives_frame_maintenance() {
        // Review fix round 1 (F-1): the per-frame `refresh_loop_range`
        // maintenance pass must only FOLLOW an armed loop, never re-arm one that
        // `scrub_release` cleared — otherwise a release far outside the range is
        // undone one frame later and the playhead snaps back into the loop.
        let store = Arc::new(SqliteStore::open_in_memory().expect("store"));
        let id = seed(
            &store,
            "/tmp/a.wav",
            &[
                seg(0, 1000, "a", ""),
                seg(1000, 2000, "b", ""),
                seg(2000, 3000, "c", ""),
            ],
        );
        let deps = deps(Arc::clone(&store));
        let mut vm = ViewerVm::open(id, &deps, &Settings::default()).expect("open");
        let (player, mut fill) = ready_player(6000);
        vm.player = PlayerState::Ready(player);

        // Arm a loop on segment 1 ([1000, 2000]).
        vm.selected = Some(1);
        vm.set_loop_enabled(true);
        assert_eq!(vm.loop_range, Some((Millis(1000), Millis(2000))));

        // Release the scrubber far outside the range (> 2000 ms past the end):
        // the loop clears.
        vm.scrub_release(Millis(5000));
        assert_eq!(vm.loop_range, None, "release far outside clears the loop");

        // The frame-maintenance pass must NOT re-arm the cleared loop.
        vm.refresh_loop_range();
        assert_eq!(
            vm.loop_range, None,
            "frame maintenance must not re-arm a cleared loop"
        );

        // The player's own loop is cleared too: playing across the old end runs
        // straight through without wrapping back to the old start.
        if let PlayerState::Ready(player) = &vm.player {
            player.seek(Millis(1900));
            player.play();
        }
        let mut buf = vec![0.0f32; 400];
        fill(&mut buf);
        assert!(
            vm.player_position().expect("ready").0 >= 2200,
            "the playhead ran past the old loop end without wrapping (player \
             loop cleared); position was {:?}",
            vm.player_position()
        );
    }
}
