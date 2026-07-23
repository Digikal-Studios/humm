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
//! [`crate::views::viewer`]. Playback/search/editing state land in later M4
//! tasks (8-12); this task ships the window skeleton, view modes, and the
//! open/focus paths only.
//!
//! Ported from Buzz's `TranscriptionViewerWidget.__init__`/
//! `_load_segments_and_settings`/`reset_view`
//! (`transcription_viewer_widget.py:78-143,1078-1145`).

use std::sync::Arc;

use humm_core::boundary::{AudioDecoder, AudioOutput, MediaProber, VideoStreamer};
use humm_core::domain::{Millis, Segment, Status, TranscriptionId};
use humm_core::error::StoreError;
use humm_core::settings::Settings;
use humm_core::viewer::text_view;
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
}
