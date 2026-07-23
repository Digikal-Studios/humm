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
use std::time::{Duration, Instant};

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
use humm_core::viewer::search::{RowMatch, SEARCH_DEBOUNCE_MS, results_label};
use humm_gui::import::FileDialogs;
use humm_gui::shortcuts::ShortcutMap;
use humm_gui::viewer::{CellEditor, EditCol, SearchResults, ViewerDeps, ViewerVm};
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
    settings_path: PathBuf,
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
    let settings_path = dir.path().join("settings.toml");
    (
        Fixture {
            vm,
            deps,
            shortcuts: ShortcutMap::from_settings(&std::collections::HashMap::new()),
            settings: Mutex::new(settings),
            settings_path,
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
                &fixture.settings_path,
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

// -- F-082 search bar (Task 9) -------------------------------------------

/// Opens the find bar and arms a query so the very next `harness.run()`
/// crosses the debounce window on real elapsed time — the "state seam"
/// (`fixture.vm.search.query`/`last_edit` are plain `pub` fields) rather than
/// driving simulated keystrokes or sleeping the test.
fn arm_search(fixture: &mut Fixture, query: &str) {
    fixture.vm.search.visible = true;
    fixture.vm.search.query = query.to_string();
    fixture.vm.search.last_edit =
        Some(Instant::now() - Duration::from_millis(SEARCH_DEBOUNCE_MS + 50));
}

#[test]
fn search_selects_matching_row_and_shows_label() {
    let (mut fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[
            seg(0, 1000, "alpha", ""),
            seg(1000, 2000, "beta needle here", ""),
            seg(2000, 3000, "gamma", ""),
        ],
    );
    arm_search(&mut fixture, "needle");

    let mut harness = build_harness(fixture);
    harness.run(); // crosses the (already past-due) debounce this frame

    assert_eq!(
        harness.state().vm.search.results,
        SearchResults::Rows(vec![RowMatch {
            row: 1,
            in_translation: false
        }])
    );
    assert_eq!(
        harness.state().vm.selected,
        Some(1),
        "the matching row is selected"
    );
    let expected_label = results_label(0, 1);
    harness.get_by_label_contains(&expected_label);
}

#[test]
fn nav_wraps_modulo() {
    let (mut fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[
            seg(0, 1000, "cat", ""),
            seg(1000, 2000, "cat", ""),
            seg(2000, 3000, "cat", ""),
        ],
    );
    arm_search(&mut fixture, "cat");

    let mut harness = build_harness(fixture);
    harness.run();
    assert_eq!(harness.state().vm.search.results.len(), 3);
    assert_eq!(harness.state().vm.search.current, 0);

    harness.get_by_label("\u{2193}").click(); // next
    harness.run();
    assert_eq!(harness.state().vm.search.current, 1);

    harness.get_by_label("\u{2193}").click();
    harness.run();
    harness.get_by_label("\u{2193}").click();
    harness.run();
    assert_eq!(harness.state().vm.search.current, 0, "next wraps modulo");

    harness.get_by_label("\u{2191}").click(); // prev
    harness.run();
    assert_eq!(harness.state().vm.search.current, 2, "prev wraps modulo");
}

#[test]
fn translation_matches_append_after_text_matches() {
    let (mut fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[
            seg(0, 1000, "no match here", "needle"),
            seg(1000, 2000, "needle text", "no match"),
        ],
    );
    arm_search(&mut fixture, "needle");

    let mut harness = build_harness(fixture);
    harness.run();

    let SearchResults::Rows(matches) = &harness.state().vm.search.results else {
        panic!("Timestamps mode must produce Row-kind results");
    };
    assert_eq!(
        matches,
        &vec![
            RowMatch {
                row: 1,
                in_translation: false
            },
            RowMatch {
                row: 0,
                in_translation: true
            },
        ],
        "text matches (row 1) come before the translation match (row 0)"
    );
}

// -- Review fix round 1 regressions --------------------------------------

/// Defect 1 (critical, spec §2.3 / Buzz `eventFilter`'s `Key_Escape` branch,
/// `transcription_viewer_widget.py:1064-1076`): Esc while the search input
/// has focus must hide the bar *and* clear the search. Probe-confirmed
/// broken before the fix — egui's singleline `TextEdit` surrenders focus for
/// an unfiltered Escape *before* any widget even renders this frame
/// (`Memory::Focus::begin_pass`), so a naive `response.has_focus()` check
/// after `.show()` never sees it; `response.lost_focus()` is the
/// transition-aware fix.
#[test]
fn esc_in_focused_input_hides_and_clears_bar() {
    let (mut fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[seg(0, 1000, "needle here", ""), seg(1000, 2000, "x", "")],
    );
    arm_search(&mut fixture, "needle");
    let mut harness = build_harness(fixture);
    harness.run(); // crosses the debounce, bar shows with results

    harness.get(By::new().role(Role::TextInput)).focus();
    harness.run(); // the focus request takes effect

    harness.key_press(egui::Key::Escape);
    harness.run();

    assert!(
        !harness.state().vm.search.visible,
        "Esc in the focused input must hide the bar"
    );
    assert!(
        harness.state().vm.search.query.is_empty(),
        "Esc in the focused input must clear the query"
    );
}

/// Companion to the Escape fix: plain Enter while the input has focus must
/// still advance to the next match (Buzz `returnPressed.connect(search_
/// next)`) — the identical `has_focus()`-after-`.show()` mechanics broke
/// this path too (probe-confirmed before the fix: a real Enter keypress
/// never advanced `current`), fixed by the same `lost_focus()` idiom.
#[test]
fn enter_in_focused_input_advances_to_next_match() {
    let (mut fixture, _dir) = fixture(
        "/tmp/a.wav",
        &[
            seg(0, 1000, "needle here", ""),
            seg(1000, 2000, "needle there", ""),
        ],
    );
    arm_search(&mut fixture, "needle");
    let mut harness = build_harness(fixture);
    harness.run();
    assert_eq!(harness.state().vm.search.current, 0);

    harness.get(By::new().role(Role::TextInput)).focus();
    harness.run();

    harness.key_press(egui::Key::Enter);
    harness.run();

    assert_eq!(
        harness.state().vm.search.current,
        1,
        "Enter in the focused input must advance to the next match"
    );
}

/// Defect 2 (important): the find-bar visibility persist must cover the ⌘F
/// path too, not just the toolbar-button click — `viewer_ui` must snapshot
/// `vm.search.visible` *before* any shortcut consumption can mutate it, or
/// the diff-on-change check compares a value against itself and never saves.
#[test]
fn cmd_f_toggle_persists_find_visibility() {
    let (fixture, _dir) = fixture("/tmp/a.wav", &[seg(0, 1000, "hello", "")]);
    assert!(
        !fixture.vm.search.visible,
        "find bar starts hidden by default"
    );
    let mut harness = build_harness(fixture);

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    harness.run();

    assert!(
        harness.state().vm.search.visible,
        "⌘F must open the find bar"
    );
    let persisted = harness
        .state()
        .settings
        .lock()
        .expect("settings lock")
        .viewer
        .find_widget_visible;
    assert!(
        persisted,
        "⌘F must persist find_widget_visible, not just the toolbar-button path"
    );
}
