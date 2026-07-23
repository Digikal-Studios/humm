//! kittest coverage for the transcript-viewer window chrome (F-080, Task 7).
//!
//! The viewer window is a deferred OS viewport (`app.rs::HummApp::
//! show_viewer_viewports`), which an `egui_kittest` `Harness` cannot reach
//! into (it drives only its own root viewport — the same constraint the M3
//! preferences/about viewports carry). So, exactly like `kittest_prefs.rs`
//! does for the preferences tabs, this file drives the extracted plain render
//! function `views::viewer::viewer_ui` directly over a bare `egui::Ui` via
//! `Harness::new_ui_state`, proving the toolbar/view-mode/empty-state logic
//! end to end without any window plumbing.
//!
//! Segments are seeded into a temp-dir `SqliteStore` through its public APIs
//! (`create_task` + `mark_completed`), then loaded with the real
//! `ViewerVm::open`; the adapter bundle is faked (no ffmpeg/cpal/rfd — the
//! window skeleton never invokes them this task).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use humm_core::boundary::{
    AudioDecoder, AudioOutput, FillCallback, FrameStream, MediaInfo, MediaProber, OutputStream,
    Pcm, PlaybackPcm, StreamSpec, TaskStore, VideoStreamInfo, VideoStreamer,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    Input, Millis, ModelSize, ModelSpec, OutputSpec, Segment, Source, TaskKind, TaskSpec,
    TranscriptionId, TranscriptionOptions,
};
use humm_core::error::{AudioError, MediaError};
use humm_core::settings::Settings;
use humm_gui::import::FileDialogs;
use humm_gui::shortcuts::ShortcutMap;
use humm_gui::viewer::{ViewerDeps, ViewerVm};
use humm_gui::views::viewer::viewer_ui;
use humm_store::SqliteStore;

// -- fakes: viewer_ui never invokes any adapter this task, so these only need
//    to satisfy the type bounds. --------------------------------------------

struct FakeDecoder;
impl AudioDecoder for FakeDecoder {
    fn decode(&self, _path: &Path, _cancel: &CancelToken) -> Result<Pcm, MediaError> {
        unreachable!("viewer_ui never decodes")
    }
    fn decode_playback(
        &self,
        _path: &Path,
        _cancel: &CancelToken,
    ) -> Result<PlaybackPcm, MediaError> {
        unreachable!("viewer_ui never decodes")
    }
}

struct FakeProber;
impl MediaProber for FakeProber {
    fn probe(&self, _path: &Path) -> Result<MediaInfo, MediaError> {
        unreachable!("viewer_ui never probes")
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
        unreachable!("viewer_ui never streams video")
    }
}

struct FakeOutput;
impl AudioOutput for FakeOutput {
    fn open(
        &self,
        _spec: StreamSpec,
        _fill: FillCallback,
    ) -> Result<Box<dyn OutputStream>, AudioError> {
        unreachable!("viewer_ui never opens audio")
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

fn fake_deps(store: Arc<SqliteStore>) -> ViewerDeps {
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

/// The viewer render fixture: a loaded VM plus everything `viewer_ui` needs.
struct Fixture {
    vm: ViewerVm,
    deps: ViewerDeps,
    shortcuts: ShortcutMap,
    settings: Mutex<Settings>,
}

/// Seeds a temp-dir store with one completed transcription (`file`, `segs`) and
/// opens a viewer VM over it. Returns the fixture; the tempdir is leaked into
/// the store's lifetime via the returned guard.
fn fixture(file: &str, segs: &[Segment]) -> (Fixture, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open(&dir.path().join("humm.db")).expect("open store"));
    let id = TranscriptionId::new();
    store
        .create_task(&id, &spec_for(file), Source::FileImport)
        .expect("create task");
    store.mark_completed(&id, segs).expect("mark completed");

    let deps = fake_deps(Arc::clone(&store));
    let settings = Settings::default();
    let vm = ViewerVm::open(id, &deps, &settings).expect("open viewer");
    (
        Fixture {
            vm,
            deps,
            shortcuts: ShortcutMap::from_settings(&std::collections::HashMap::new()),
            settings: Mutex::new(settings),
        },
        dir,
    )
}

fn build_harness(fixture: Fixture) -> Harness<'static, Fixture> {
    Harness::new_ui_state(
        |ui, fixture: &mut Fixture| {
            viewer_ui(
                ui,
                &mut fixture.vm,
                &fixture.deps,
                &fixture.shortcuts,
                &fixture.settings,
            );
        },
        fixture,
    )
}

#[test]
fn timestamps_mode_shows_segment_table_placeholder_rows() {
    let (fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[
            seg(0, 1000, "first segment here", ""),
            seg(1000, 2000, "second segment here", ""),
        ],
    );
    let mut harness = build_harness(fixture);
    harness.run();

    // Default mode is Timestamps → the placeholder list shows one row per
    // segment, each carrying its text.
    harness.get_by_label_contains("first segment here");
    harness.get_by_label_contains("second segment here");
    // And a display timestamp for the first row (00:00:00.000).
    harness.get_by_label_contains("00:00:00.000");
}

#[test]
fn text_mode_shows_paragraph_joined_text() {
    let (fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[seg(0, 1000, "hello", ""), seg(1000, 2000, "world", "")],
    );
    let mut harness = build_harness(fixture);
    harness.run();

    // Open the View menu and switch to Text mode.
    harness.get_by_label("View").click();
    harness.run();
    harness.get_by_label_contains("Text").click();
    harness.run();

    // Segments joined with a single space (gap 0 < 2000ms split).
    harness.get_by_label_contains("hello world");
}

#[test]
fn translation_menu_entry_hidden_without_translations() {
    // No non-blank translation anywhere → the Translation entry is absent.
    let (no_tr, _dir) = fixture("/tmp/a.wav", &[seg(0, 1000, "hello", "")]);
    let mut harness = build_harness(no_tr);
    harness.run();
    harness.get_by_label("View").click();
    harness.run();

    // The menu is open (Timestamps entry visible) but Translation is not.
    harness.get_by_label_contains("Timestamps");
    assert!(
        harness.query_by_label_contains("Translation").is_none(),
        "Translation menu entry must be hidden when the transcript has no translations"
    );

    // With a translation present, the entry appears.
    let (with_tr, _dir2) = fixture("/tmp/b.wav", &[seg(0, 1000, "hello", "hola")]);
    let mut harness2 = build_harness(with_tr);
    harness2.run();
    harness2.get_by_label("View").click();
    harness2.run();
    harness2.get_by_label_contains("Translation");
}

#[test]
fn empty_transcript_shows_explanatory_state() {
    let (fixture, _dir) = fixture("/tmp/silent.wav", &[]);
    let mut harness = build_harness(fixture);
    harness.run();

    harness.get_by_label_contains("No segments");
}
