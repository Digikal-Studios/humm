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
//! This module is pure state + store I/O — no egui. The window chrome lives in
//! [`crate::views::viewer`] and the editor table in
//! [`crate::views::viewer_table`]. Playback/search state land in later M4 tasks
//! (9-12); Task 8 adds the segment-editing state ([`CellEditor`],
//! [`ViewerVm::selected`]/[`ViewerVm::editing`]) and the store-backed commit
//! rules ([`ViewerVm::apply_timestamp_edit`]/[`ViewerVm::apply_text_edit`]).
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

use std::sync::Arc;
use std::time::Instant;

use humm_core::boundary::{AudioDecoder, AudioOutput, MediaProber, VideoStreamer};
use humm_core::domain::{Millis, Segment, Status, TranscriptionId};
use humm_core::error::StoreError;
use humm_core::settings::Settings;
use humm_core::viewer::edit::{EditOutcome, Field, SegmentTimes, timestamp_edit};
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

/// Placeholder playback state for this task. The audio/video player engine and
/// its real states (loading, ready, playing, degraded) land in Task 10; the
/// window skeleton renders an inert player strip until then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerState {
    /// No player engine wired yet (this task) or the media could not be
    /// decoded (Task 10's degraded path).
    #[default]
    Unavailable,
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
    /// Persisted-at-open loop-selected-segment toggle; wired by Task 10.
    pub segment_looping_enabled: bool,
    /// Placeholder playback state (Task 10 replaces this with the real player).
    pub player: PlayerState,
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
        let title = deps
            .store
            .get_task(&id)?
            .and_then(|task| task.file)
            .map(|file| file_basename(&file))
            .unwrap_or_default();
        let has_translations = rows.iter().any(|row| !row.translation.trim().is_empty());
        Ok(ViewerVm {
            id,
            title,
            rows,
            has_translations,
            mode: ViewMode::Timestamps,
            text_body: String::new(),
            close_requested: false,
            focus_requested: false,
            initial_geometry: settings.viewer.geometry,
            observed_geometry: None,
            playback_controls_visible: settings.viewer.playback_controls_visible,
            find_widget_visible: settings.viewer.find_widget_visible,
            segment_looping_enabled: settings.viewer.segment_looping_enabled,
            player: PlayerState::Unavailable,
            selected: None,
            editing: None,
            saved_flash: None,
            unsaved: false,
        })
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
        // TODO(Task 9): re-run the active search here — Buzz's
        // `on_view_mode_changed` calls `perform_search()` when `search_text`
        // is non-empty (`transcription_viewer_widget.py:1143-1145`). No search
        // state exists yet, so there is nothing to re-run.
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
    /// core `text_view` builders. Rebuilt on each mode switch — an infrequent,
    /// user-initiated event, never per frame.
    fn segments(&self) -> Vec<Segment> {
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

    /// Task 10 (playback) hook — spec §2.2 `on_timestamp_being_edited`: while a
    /// Start/End cell editor is open, an active loop range follows the value in
    /// the editor buffer *live* (before commit). Returns the edited row, its
    /// [`Field`], and the buffer's current parse (floored at zero), or `None`
    /// when no timestamp cell is being edited. The player does not exist until
    /// Task 10, so nothing consumes this yet — it is the documented seam the
    /// loop-range live-follow wires into, kept as real (public) API rather than
    /// dead code.
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

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::boundary::{
        FillCallback, FrameStream, MediaInfo, OutputStream, Pcm, PlaybackPcm, StreamSpec,
        TaskStore, VideoStreamInfo,
    };
    use humm_core::cancel::CancelToken;
    use humm_core::domain::{
        Input, ModelSize, ModelSpec, OutputSpec, Source, TaskKind, TaskSpec, TranscriptionOptions,
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
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                word_timestamps: false,
                initial_prompt: String::new(),
                temperature: vec![0.0],
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
}
