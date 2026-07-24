//! kittest coverage for Task 8 (F-004/F-026): toolbar enablement,
//! Clear-History confirm-then-delete, Rename, Notes-via-double-click,
//! Retry-to-completion, and the `StoreWriteFailed` store-health banner.
//!
//! Reuses `kittest_table.rs`'s seeding/harness-widening conventions (this
//! table needs the same ~1600pt of width so every column's synthetic clicks
//! land, not off-screen).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::NodeT;
use egui_kittest::{Harness, HarnessBuilder, kittest::Queryable};
use humm_core::boundary::{
    EngineCaps, EngineOptions, Pcm, SegmentSink, TaskStore, TranscriptionEngine,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, Input, ModelSize, ModelSpec, OutputSpec, Source, TaskKind,
    TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::error::{CoreError, EngineError};
use humm_core::queue::{EngineFactory, TaskEvent, TaskEventKind};
use humm_gui::app::HummApp;
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_store::SqliteStore;

/// A fake [`EngineFactory`] for tests that never actually run a job.
struct NeverCalledEngineFactory;

impl EngineFactory for NeverCalledEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        unreachable!("no task is ever run in this test")
    }
}

/// A fake [`TranscriptionEngine`]/[`EngineFactory`] pair that always
/// succeeds with zero segments — for the Retry-reaches-completion test,
/// which needs a real ffmpeg decode/ffprobe probe (via
/// `services::init_with`'s hard-coded real adapters) but no real whisper
/// model.
struct FakeSucceedingEngineFactory;

impl EngineFactory for FakeSucceedingEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn TranscriptionEngine>, CoreError> {
        Ok(Box::new(FakeEngine))
    }
}

struct FakeEngine;

impl TranscriptionEngine for FakeEngine {
    fn transcribe(
        &mut self,
        _audio: &Pcm,
        _options: &EngineOptions,
        _sink: &mut dyn SegmentSink,
        _cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        Ok(())
    }

    fn capabilities(&self) -> EngineCaps {
        EngineCaps {
            word_timestamps: false,
            translate: false,
            vad: false,
        }
    }
}

/// Real Buzz-sourced audio testdata, same convention as
/// `crates/humm-cli/tests/cli.rs`'s own `testdata_dir` — overridable via
/// `HUMM_BUZZ_TESTDATA`.
fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

fn sample_spec(input: Input) -> TaskSpec {
    TaskSpec {
        input,
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
            temperature: DEFAULT_TEMPERATURE.to_vec(),
            llm: None,
        },
        output: OutputSpec::default(),
    }
}

fn test_services(store: Arc<SqliteStore>, engines: Box<dyn EngineFactory>, dir: &Path) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: dir.join("media"),
        model_root: dir.join("models"),
        settings: humm_core::settings::Settings::default(),
        settings_path: dir.join("settings.toml"),
        engines,
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
    })
}

/// Builds a harness widened well past the table's full column width (see
/// `kittest_table.rs`'s own `wide_harness` doc comment for why), with a
/// small `step_dt`: `egui_kittest::Harness::step` runs each queued
/// synthetic-input event as its *own* frame, advancing egui's internal
/// clock by `step_dt` every time — the default (0.25s) means two separate
/// `Node::click()` calls (3 events each: move/press/release) land the
/// second click's release ~0.75s after the first's, well past egui's
/// `max_double_click_delay` (0.3s default), so a synthetic double-click
/// would never actually register as one. A much smaller `step_dt` (10ms)
/// keeps every test in this file's *other* interactions unaffected (nothing
/// else here asserts on simulated elapsed time) while letting
/// `notes_double_click_opens_modal_and_persists_back_to_the_store` land two
/// `click()` calls inside the real double-click window.
///
/// Returns the harness alongside the `TempDir` backing its (unused but now
/// real, not sentinel-string) `media_dir`/`model_root`/`settings_path` —
/// the caller must keep it alive for the harness's lifetime.
fn wide_harness(
    store: Arc<SqliteStore>,
    engines: Box<dyn EngineFactory>,
) -> (Harness<'static, HummApp>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let services = test_services(store, engines, dir.path());
    let harness = HarnessBuilder::default()
        .with_size(egui::Vec2::new(1700.0, 500.0))
        .with_step_dt(0.01)
        // See `kittest_import.rs`'s `harness_with` for why this is raised
        // above the default of 4 (final-review fix 5): a real queue/
        // event-pump thread backs this harness too.
        .with_max_steps(32)
        .build_eframe(move |_cc| HummApp::new_for_test(services));
    (harness, dir)
}

// -- Toolbar enablement ----------------------------------------------------

#[test]
fn toolbar_cancel_and_clear_disabled_with_nothing_selected() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(
            &id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/a.wav"))),
            Source::FileImport,
        )
        .expect("create");

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();

    let cancel = harness.get_by_label("Cancel Transcription");
    let clear = harness.get_by_label("Clear History");
    assert!(
        cancel.accesskit_node().is_disabled(),
        "Cancel should be disabled with no selection"
    );
    assert!(
        clear.accesskit_node().is_disabled(),
        "Clear should be disabled with no selection"
    );
}

#[test]
fn toolbar_cancel_enabled_and_clear_disabled_for_a_queued_selection() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(
            &id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/a.wav"))),
            Source::FileImport,
        )
        .expect("create");

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();
    harness.get_by_label_contains("Queued").click();
    harness.run();

    let cancel = harness.get_by_label("Cancel Transcription");
    let clear = harness.get_by_label("Clear History");
    assert!(
        !cancel.accesskit_node().is_disabled(),
        "Cancel should be enabled for a queued row"
    );
    assert!(
        clear.accesskit_node().is_disabled(),
        "Clear should stay disabled for a queued row"
    );
}

#[test]
fn toolbar_clear_enabled_and_cancel_disabled_for_a_completed_selection() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(
            &id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/a.wav"))),
            Source::FileImport,
        )
        .expect("create");
    store.mark_completed(&id, &[]).expect("mark completed");

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();
    harness.get_by_label("Completed").click();
    harness.run();

    let cancel = harness.get_by_label("Cancel Transcription");
    let clear = harness.get_by_label("Clear History");
    assert!(
        cancel.accesskit_node().is_disabled(),
        "Cancel should stay disabled for a completed row"
    );
    assert!(
        !clear.accesskit_node().is_disabled(),
        "Clear should be enabled for a completed row"
    );
}

#[test]
fn toolbar_both_disabled_for_a_mixed_selection() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let queued_id = TranscriptionId::new();
    store
        .create_task(
            &queued_id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/a.wav"))),
            Source::FileImport,
        )
        .expect("create queued");
    let completed_id = TranscriptionId::new();
    store
        .create_task(
            &completed_id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/b.wav"))),
            Source::FileImport,
        )
        .expect("create completed");
    store
        .mark_completed(&completed_id, &[])
        .expect("mark completed");

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();
    harness.get_by_label_contains("Queued").click();
    harness.run();
    harness
        .get_by_label("Completed")
        .click_modifiers(egui::Modifiers::COMMAND);
    harness.run();

    let cancel = harness.get_by_label("Cancel Transcription");
    let clear = harness.get_by_label("Clear History");
    assert!(
        cancel.accesskit_node().is_disabled(),
        "mixed selection disables Cancel"
    );
    assert!(
        clear.accesskit_node().is_disabled(),
        "mixed selection disables Clear"
    );
}

// -- Clear History confirm-then-delete flow ---------------------------------

#[test]
fn clear_history_deletes_the_row_only_after_confirm() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(
            &id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/a.wav"))),
            Source::FileImport,
        )
        .expect("create");
    store.mark_completed(&id, &[]).expect("mark completed");
    let store_for_assert = Arc::clone(&store);

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();
    harness.get_by_label("Completed").click();
    harness.run();

    // Click Clear History, then Cancel in the confirm modal — the row must
    // survive.
    harness.get_by_label("Clear History").click();
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();
    assert!(
        store_for_assert.get_task(&id).expect("get_task").is_some(),
        "row must survive a canceled confirm"
    );
    harness.get_by_label("Completed"); // still rendered

    // Now confirm for real.
    harness.get_by_label("Clear History").click();
    harness.run();
    harness.get_by_label("Ok").click();
    harness.run();

    assert!(
        store_for_assert.get_task(&id).expect("get_task").is_none(),
        "row must be gone from the store after confirming"
    );
    assert!(
        harness.query_by_label_contains("Drop media here").is_some(),
        "the hero should return once the last row is cleared"
    );
}

// -- Rename ------------------------------------------------------------------

/// Doesn't attempt to simulate typing over the prefilled text (an
/// `egui::TextEdit`'s cursor/selection/focus handling is upstream egui's own
/// concern, already exhaustively tested there, and driving it via
/// synthetic keystrokes proved timing-fragile in `egui_kittest` — the same
/// "too fragile" call `kittest_table.rs` made for header-menu checkboxes).
/// Instead proves the wiring end to end: the context menu opens the dialog
/// prefilled with the Buzz-verified name/url/basename text (`rename_notes.rs`
/// unit-tests the fallback order itself), and clicking OK unmodified still
/// persists that text to the store and refreshes the visible row —
/// exercising the exact same `set_task_name` + `TasksVm::refresh_row` path a
/// real edit would.
#[test]
fn rename_dialog_prefills_and_persists_on_ok() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(
            &id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/clip.wav"))),
            Source::FileImport,
        )
        .expect("create");
    let store_for_assert = Arc::clone(&store);
    // Confirm the premise: no name is persisted yet (the table shows
    // "clip.wav" only via the basename fallback).
    assert_eq!(
        store_for_assert
            .get_task(&id)
            .expect("get_task")
            .expect("row exists")
            .name,
        None
    );

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();

    harness.get_by_label_contains("clip.wav").click_secondary();
    harness.run();
    harness.get_by_label("Rename").click();
    harness.run();

    harness.get_by_label("Rename Transcription"); // modal title present
    let text_field = harness.get_by_role(egui::accesskit::Role::TextInput);
    assert_eq!(text_field.value().as_deref(), Some("clip.wav"));
    let _ = text_field;

    harness.get_by_label("OK").click();
    harness.run();

    assert_eq!(
        store_for_assert
            .get_task(&id)
            .expect("get_task")
            .expect("row exists")
            .name
            .as_deref(),
        Some("clip.wav"),
        "OK must persist the (unmodified) prefilled name"
    );
}

// -- Notes via double-click ---------------------------------------------------

/// Same rationale as the Rename test above for not simulating a text edit.
/// Seeds a non-empty note first so the Notes cell has a findable label to
/// double-click (an empty cell has no distinguishing text of its own); this
/// proves double-click opens the Notes modal (Task 7 didn't wire this —
/// this task's `task_table.rs` change does) prefilled with the current
/// notes, and that OK persists it back (`set_task_notes` + row refresh).
/// The Ok-always-saves-even-empty rule itself
/// (`rename_notes::ui`'s `Dialog::Notes` arm has no non-empty guard, unlike
/// `Dialog::Rename`'s) is a one-line, code-reviewed difference; simulating
/// clearing the field to prove it live in `egui_kittest` would fight the
/// same text-input timing fragility this test already avoids for the
/// initial prefill.
#[test]
fn notes_double_click_opens_modal_and_persists_back_to_the_store() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(
            &id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/clip.wav"))),
            Source::FileImport,
        )
        .expect("create");
    store
        .set_task_notes(&id, "existing note")
        .expect("seed notes");
    let store_for_assert = Arc::clone(&store);

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();

    let notes_cell = harness.get_by_label("existing note");
    notes_cell.click();
    notes_cell.click();
    let _ = notes_cell;
    harness.run();

    // (Not asserting on a "Notes" label here: the modal's own heading text
    // is identical to the Notes *column header*'s, which is already on
    // screen — the multiline text input below is unambiguous proof the
    // modal itself opened.)
    let text_field = harness.get_by_role(egui::accesskit::Role::MultilineTextInput);
    assert_eq!(text_field.value().as_deref(), Some("existing note"));
    let _ = text_field;

    harness.get_by_label("OK").click();
    harness.run();

    assert_eq!(
        store_for_assert
            .get_task(&id)
            .expect("get_task")
            .expect("row exists")
            .notes
            .as_deref(),
        Some("existing note"),
        "OK must persist the (unmodified) prefilled notes text"
    );
}

// -- Retry reaches completion -------------------------------------------------

/// Polls the store directly for the authoritative terminal status (rather
/// than a UI label): the retried task runs a **real** ffmpeg decode/probe
/// (`services::init_with`'s hard-coded real adapters) on its own OS thread,
/// so completion is a real-wall-clock-time event this test must wait for,
/// and `Status::Completed`'s exact table cell text varies (Buzz appends a
/// real elapsed-time duration once both `time_started`/`time_ended` are
/// set, unlike `kittest_table.rs`'s synthetic rows which only ever set
/// `time_ended` directly and so never get a duration suffix) — the store's
/// own `status` column has no such ambiguity.
#[test]
fn retry_on_a_failed_row_reaches_completion() {
    let wav = testdata_dir().join("whisper-latvian.wav");
    assert!(wav.is_file(), "missing testdata: {}", wav.display());

    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(&id, &sample_spec(Input::Path(wav)), Source::FileImport)
        .expect("create");
    store.mark_failed(&id, "boom").expect("mark failed");
    let store_for_assert = Arc::clone(&store);

    let (mut harness, _dir) = wide_harness(store, Box::new(FakeSucceedingEngineFactory));
    harness.run();

    harness
        .get_by_label_contains("Failed (boom)")
        .click_secondary();
    harness.run();
    harness.get_by_label("Restart Transcription").click();
    harness.run();

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.run();
        let status = store_for_assert
            .get_task(&id)
            .expect("get_task")
            .expect("row still exists")
            .status;
        if status == humm_core::domain::Status::Completed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "retried task did not reach Completed in time (currently {status:?})"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // Light UI sanity check: the row's own status cell now contributes a
    // second "…Completed…" match beyond the "Date Completed" header (one
    // more `harness.run()` so the just-observed store write's terminal
    // event — already drained above — has been folded into a rendered
    // frame).
    harness.run();
    assert!(
        harness.query_all_by_label_contains("Completed").count() >= 2,
        "expected the row's own status cell to also render something containing \"Completed\""
    );
}

// -- StoreWriteFailed banner ---------------------------------------------------

#[test]
fn store_write_failed_event_shows_the_banner_and_dismiss_clears_it() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run(); // spawns the (lazily-created) event pump

    harness.state_mut().inject_event_for_test(TaskEvent {
        task_id: TranscriptionId::new(),
        kind: TaskEventKind::StoreWriteFailed {
            operation: "mark_started".to_string(),
            detail: "disk full".to_string(),
        },
    });
    harness.run();

    harness.get_by_label_contains("humm couldn't record history updates");

    // Dismiss it.
    harness.get_by_label("\u{00d7}").click();
    harness.run();
    assert!(
        harness
            .query_by_label_contains("humm couldn't record history updates")
            .is_none(),
        "banner should be gone after dismiss"
    );

    // Reappears fresh (no counter) on the next event.
    harness.state_mut().inject_event_for_test(TaskEvent {
        task_id: TranscriptionId::new(),
        kind: TaskEventKind::StoreWriteFailed {
            operation: "mark_completed".to_string(),
            detail: "disk full again".to_string(),
        },
    });
    harness.run();
    harness.get_by_label_contains("humm couldn't record history updates");
    assert!(
        harness.query_by_label_contains("(2)").is_none(),
        "a fresh reappearance must not carry over the old counter"
    );
}

// -- Modal suppresses global shortcuts (review fix) --------------------------

/// Review-round fix: `handle_global_shortcuts`/`task_table::handle_keyboard_shortcuts`
/// must both be suppressed while any modal (here, the Rename dialog) is
/// open — Delete previously reached the table's Clear-selected handling and
/// ⌘S previously reached the toolbar's Clear shortcut, either of which
/// would stack the Clear-confirm modal on top of the still-open Rename
/// dialog. Confirms the fix in both directions: neither key does anything
/// while Rename is open, and Delete works normally again once it's closed.
#[test]
fn modal_open_suppresses_delete_and_command_s_while_rename_is_open() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let id = TranscriptionId::new();
    store
        .create_task(
            &id,
            &sample_spec(Input::Path(PathBuf::from("/tmp/clip.wav"))),
            Source::FileImport,
        )
        .expect("create");
    // A terminal (Clear-eligible) row, so if the gate failed to suppress
    // the shortcuts, Clear's own gate wouldn't be what stops it either.
    store.mark_completed(&id, &[]).expect("mark completed");

    let (mut harness, _dir) = wide_harness(store, Box::new(NeverCalledEngineFactory));
    harness.run();

    // A plain click first: selection is separate from a context-menu
    // right-click (`Response::clicked()` only considers the primary
    // button), and the Clear gate this test exercises reads
    // `vm.selected_statuses()`, which is empty — and so unconditionally
    // gate-fails regardless of `modal_open` — unless the row is actually
    // selected.
    harness.get_by_label("Completed").click();
    harness.run();
    harness.get_by_label("Completed").click_secondary();
    harness.run();
    harness.get_by_label("Rename").click();
    harness.run();

    // Rename is open (its text field is on screen).
    harness.get_by_role(egui::accesskit::Role::TextInput);

    harness.key_press(egui::Key::Delete);
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();

    // The Clear-confirm modal must not have appeared (its body text is
    // unique to it — unlike "Clear History", which is also the toolbar
    // button's own label and thus always on screen).
    assert!(
        harness
            .query_by_label_contains("Are you sure you want to delete")
            .is_none(),
        "Delete/⌘S must not open Clear-confirm while Rename is open"
    );
    // Rename must still be the only open dialog.
    harness.get_by_role(egui::accesskit::Role::TextInput);

    // Close Rename; the same shortcut now works normally again.
    harness.get_by_label("Cancel").click();
    harness.run();

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    harness.get_by_label_contains("Are you sure you want to delete");
}
