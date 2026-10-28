//! kittest coverage for the task table (F-003, Task 7 brief Step 4): seeds a
//! temp store with three rows (completed/failed/queued), drives the real
//! `HummApp` through `egui_kittest`, and asserts the exact Buzz status
//! texts are visible, that a header click toggles the sort indicator, and
//! that the row context menu gates "Restart Transcription" correctly.
//!
//! Not covered here (documented deviation, per the brief's own escape
//! hatch): toggling a column's visibility via the *header* context menu.
//! Opening that popup and then finding checkbox nodes inside it proved too
//! timing-fragile to drive reliably through `egui_kittest`'s synthetic input
//! queue across the popup's own extra frame; `TableLayout`'s visibility
//! toggle is instead unit-tested directly in
//! `crates/humm-gui/src/views/task_table.rs` (`toggle_visible_*` tests),
//! which exercises the exact same code path this menu's checkboxes call.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::{Harness, kittest::Queryable};
use humm_core::boundary::TaskStore;
use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, Input, ModelSize, ModelSpec, OutputSpec, Source, TaskKind,
    TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::error::CoreError;
use humm_core::queue::EngineFactory;
use humm_gui::app::HummApp;
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_store::SqliteStore;

/// A fake [`EngineFactory`] for tests that never actually run a job: every
/// seeded row is already in a terminal (or queued-but-never-dequeued) state,
/// so nothing here ever calls `create`.
struct NeverCalledEngineFactory;

impl EngineFactory for NeverCalledEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn humm_core::boundary::TranscriptionEngine>, CoreError> {
        unreachable!("no task is ever run in this test")
    }
}

fn sample_spec(name_hint: &str) -> TaskSpec {
    TaskSpec {
        input: Input::Path(PathBuf::from(format!("/tmp/{name_hint}.wav"))),
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
            extract_speech: false,
            identify_speakers: false,
        },
        output: OutputSpec::default(),
    }
}

/// Seeds an in-memory store with three rows: completed, failed, and queued
/// (seeded directly via `create_task`, matching how a freshly-enqueued task
/// is immediately `queued` — Buzz/humm parity, no extra transition needed).
fn seeded_store() -> SqliteStore {
    let store = SqliteStore::open_in_memory().expect("open in-memory store");

    let completed_id = TranscriptionId::new();
    store
        .create_task(&completed_id, &sample_spec("alpha"), Source::FileImport)
        .expect("create completed");
    store
        .mark_completed(&completed_id, &[])
        .expect("mark completed");

    let failed_id = TranscriptionId::new();
    store
        .create_task(&failed_id, &sample_spec("bravo"), Source::FileImport)
        .expect("create failed");
    store
        .mark_failed(&failed_id, "ffmpeg not found")
        .expect("mark failed");

    let queued_id = TranscriptionId::new();
    store
        .create_task(&queued_id, &sample_spec("charlie"), Source::FileImport)
        .expect("create queued");

    store
}

fn test_services(store: Arc<SqliteStore>, dir: &Path) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: dir.join("media"),
        model_root: dir.join("models"),
        settings: humm_core::settings::Settings::default(),
        settings_path: dir.join("settings.toml"),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
        artifacts: None,
        secrets: services::default_test_secrets(),
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    })
}

/// Builds a harness over a fresh seeded store, widened well past the
/// table's full column width (~1600pt across all seven default-width
/// columns). `egui_kittest`'s default 800x600 viewport would otherwise clip
/// the later columns (Date Added, Notes) out of a synthetic click/tap's
/// reach — every test in this file needs every column reachable, not just
/// whichever happen to fit in 800pt.
///
/// Returns the harness alongside the `TempDir` backing its (unused but now
/// real, not sentinel-string) `media_dir`/`model_root`/`settings_path` —
/// the caller must keep it alive for the harness's lifetime.
fn wide_harness() -> (Harness<'static, HummApp>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(seeded_store());
    let services = test_services(store, dir.path());
    let mut harness = Harness::new_eframe(move |_cc| HummApp::new_for_test(services));
    harness.set_size(egui::Vec2::new(1700.0, 400.0));
    (harness, dir)
}

#[test]
fn table_shows_three_seeded_rows_with_exact_buzz_status_texts() {
    let (mut harness, _dir) = wide_harness();
    harness.run();

    // Byte-exact Buzz status strings (format.rs's own unit tests pin the
    // formatting rules; this proves they actually reach the rendered UI).
    // "Completed" is matched exactly (not `_contains`): the Date Completed
    // header label itself contains that substring, so `_contains` would
    // ambiguously match both nodes.
    harness.get_by_label_contains("Queued");
    harness.get_by_label("Completed");
    harness.get_by_label_contains("Failed (ffmpeg not found)");

    // The empty-state hero must not still be showing now that rows exist.
    assert!(
        harness.query_by_label_contains("Drop media here").is_none(),
        "hero should be replaced by the table once rows are present"
    );
}

#[test]
fn header_click_toggles_the_sort_indicator() {
    let (mut harness, _dir) = wide_harness();
    harness.run();

    // Default sort: Date Added, descending (Buzz parity).
    harness.get_by_label_contains("Date Added \u{25bc}");

    harness.get_by_label_contains("Date Added \u{25bc}").click();
    harness.run();

    harness.get_by_label_contains("Date Added \u{25b2}");
}

#[test]
fn row_context_menu_gates_restart_to_failed_canceled_skipped() {
    let (mut harness, _dir) = wide_harness();
    harness.run();

    // Right-click the Failed row's status cell: Restart Transcription must
    // be offered (Buzz gate: failed/canceled/skipped only).
    harness
        .get_by_label_contains("Failed (ffmpeg not found)")
        .click_secondary();
    harness.run();
    harness.get_by_label_contains("Restart Transcription");
}

#[test]
fn row_context_menu_on_completed_row_does_not_offer_restart() {
    let (mut harness, _dir) = wide_harness();
    harness.run();

    // Completed is NOT in Buzz's restartable set — right-clicking a
    // completed row's status cell must not offer "Restart Transcription".
    // Exact match: "Date Completed" (the header) also contains "Completed".
    harness.get_by_label("Completed").click_secondary();
    harness.run();
    assert!(
        harness
            .query_by_label_contains("Restart Transcription")
            .is_none(),
        "a completed row must not offer Restart Transcription"
    );
}

/// Review-fix coverage (design spec §2.6, resizable/persisted column
/// widths): the table's columns are now `.resizable(true)` and its
/// `TableBuilder` carries an explicit `id_salt`, with `egui_extras`'s
/// `"serde"` feature on so its internal `TableState` persists through
/// `egui::Memory` — this proves that setup renders and keeps rendering
/// correctly across several frames without panicking, and that the same
/// three rows stay visible throughout (the layout the user sees doesn't
/// shift or corrupt itself from having resizable/persisted state wired in).
///
/// What this does *not* (and, within `egui_kittest`, cannot) prove: an
/// actual gui.ron round-trip across a real process restart — `Harness`
/// never calls `eframe::App::save`/serializes to disk, so real
/// width-survives-a-restart coverage is an acceptance-level concern, not a
/// kittest one. The persistence *mechanism itself* (egui_extras writing
/// through `Memory::data.insert_persisted`, which eframe's `persistence`
/// feature is what serializes to disk) is verified by reading the vendored
/// `egui_extras` source directly (see `ui`'s doc comment in
/// `task_table.rs`), not by a test.
#[test]
fn table_with_resizable_persisted_columns_renders_several_frames_without_panicking() {
    let (mut harness, _dir) = wide_harness();
    for _ in 0..5 {
        harness.step();
    }

    harness.get_by_label_contains("Queued");
    harness.get_by_label("Completed");
    harness.get_by_label_contains("Failed (ffmpeg not found)");
}

/// "Reset Column Order" (header context menu) exercises the same
/// `TableLayout::reset` → `take_pending_width_reset` → `TableBuilder::reset`
/// path the review-fix added for clearing egui_extras's persisted column
/// widths — this proves that path runs without panicking and that the sort
/// genuinely returns to the Buzz default (`DateAdded`, descending) after
/// having been changed, end to end through the real app.
#[test]
fn reset_column_order_restores_default_sort_without_panicking() {
    let (mut harness, _dir) = wide_harness();
    harness.run();

    // Move off the default sort first, so the reset below is a real change.
    harness.get_by_label_contains("File Name / URL").click();
    harness.run();
    harness.get_by_label_contains("File Name / URL \u{25b2}");

    // Right-click the header, then click "Reset Column Order".
    harness
        .get_by_label_contains("File Name / URL \u{25b2}")
        .click_secondary();
    harness.run();
    harness.get_by_label_contains("Reset Column Order").click();
    harness.run();

    // Back to Buzz's default: Date Added, descending.
    harness.get_by_label_contains("Date Added \u{25bc}");
}
