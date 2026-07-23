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

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{By, Queryable};
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
use humm_core::viewer::edit::Field;
use humm_gui::import::FileDialogs;
use humm_gui::shortcuts::ShortcutMap;
use humm_gui::viewer::{CellEditor, EditCol, ViewerDeps, ViewerVm};
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

    // Open the View menu and switch to Text mode. The editor table now renders
    // a "Text" *column header* (a Label) in Timestamps mode, so target the
    // menu entry by its Button role to avoid the collision.
    harness.get_by_label("View").click();
    harness.run();
    harness
        .get(By::new().role(Role::Button).label_contains("Text"))
        .click();
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

    // With a translation present, the menu entry appears. (The table also now
    // renders a "Translation" column header — a Label — so target the menu
    // entry by its Button role.)
    let (with_tr, _dir2) = fixture("/tmp/b.wav", &[seg(0, 1000, "hello", "hola")]);
    let mut harness2 = build_harness(with_tr);
    harness2.run();
    harness2.get_by_label("View").click();
    harness2.run();
    harness2.get(By::new().role(Role::Button).label_contains("Translation"));
}

#[test]
fn empty_transcript_shows_explanatory_state() {
    let (fixture, _dir) = fixture("/tmp/silent.wav", &[]);
    let mut harness = build_harness(fixture);
    harness.run();

    harness.get_by_label_contains("No segments");
}

/// The milestone acceptance "edits survive restart" automated half: a commit
/// driven through the editing seam is read back by a *fresh* `ViewerVm::open`
/// on the same store — proving the write reached disk, not just memory. (The
/// windowed manual restart is the other half.)
#[test]
fn edit_survives_reopen() {
    let (mut fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[seg(0, 1000, "a", ""), seg(1000, 2000, "b", "")],
    );
    let id = fixture.vm.id;

    // A timestamp commit that clamps its neighbor, plus a text commit — both
    // through the store-backed editing seam.
    assert!(
        fixture
            .vm
            .apply_timestamp_edit(&fixture.deps, 1, Field::Start, Millis(800), false),
        "the start edit must land"
    );
    fixture
        .vm
        .apply_text_edit(&fixture.deps, 0, EditCol::Text, "a-edited".to_string());

    // Reopen a fresh VM over the same store — the "restart".
    let reopened = ViewerVm::open(id, &fixture.deps, &Settings::default()).expect("reopen");
    assert_eq!(
        reopened.rows[1].start,
        Millis(800),
        "edited start survived reopen"
    );
    assert_eq!(
        reopened.rows[0].end,
        Millis(800),
        "clamped neighbor end survived reopen"
    );
    assert_eq!(
        reopened.rows[0].text, "a-edited",
        "edited text survived reopen"
    );
}

/// A 100k-segment transcript renders (several frames) without panicking — the
/// virtualized table must never materialize a per-row editor (the perf bench is
/// Task 14; this is the smoke half).
#[test]
fn table_renders_100k_rows_without_panic() {
    let segs: Vec<Segment> = (0..100_000i64)
        .map(|i| seg(i * 1000, i * 1000 + 900, "segment text", ""))
        .collect();
    let (fixture, _dir) = fixture("/tmp/big.wav", &segs);
    let mut harness = build_harness(fixture);

    // Initial render plus a couple more frames to exercise virtualization.
    harness.run();
    harness.run();
    harness.run();

    assert_eq!(harness.state().vm.rows.len(), 100_000);
}

/// Esc commits a text-cell edit (the Buzz `CustomTextEdit` quirk, spec ledger
/// #6). Typing into a `TextEdit` via kittest is timing-fragile (M3 precedent),
/// so this drives the editing-state seam: it opens a Text-cell editor with a
/// changed buffer, presses Esc, and asserts the commit landed in memory AND the
/// store, and that the editor closed.
#[test]
fn esc_commits_text_edit() {
    let (mut fixture, _dir) = fixture("/tmp/a.wav", &[seg(0, 1000, "original", "")]);
    let id = fixture.vm.id;
    fixture.vm.selected = Some(0);
    fixture.vm.editing = Some(CellEditor::new(
        0,
        EditCol::Text,
        "edited-by-esc".to_string(),
        Millis(0),
    ));

    let mut harness = build_harness(fixture);
    harness.run(); // render the open editor once (it grabs focus)

    harness.key_press(egui::Key::Escape);
    harness.run();

    assert!(
        harness.state().vm.editing.is_none(),
        "Esc closes (commits) the editor"
    );
    assert_eq!(harness.state().vm.rows[0].text, "edited-by-esc");
    let persisted = harness
        .state()
        .deps
        .store
        .get_segment_rows(id)
        .expect("reread");
    assert_eq!(
        persisted[0].text, "edited-by-esc",
        "the Esc-committed text persisted to the store"
    );
}
