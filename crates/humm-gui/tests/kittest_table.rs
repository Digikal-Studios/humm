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
    DEFAULT_TEMPERATURE, Input, ModelSize, ModelSpec, OutputSpec, Source, TaskKind, TaskSpec,
    TranscriptionId, TranscriptionOptions,
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
                size: ModelSize::Small,
                hf_repo: None,
            },
            word_timestamps: false,
            initial_prompt: String::new(),
            temperature: DEFAULT_TEMPERATURE.to_vec(),
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

fn test_services(store: Arc<SqliteStore>) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: PathBuf::from("/unused-media-dir"),
        model_root: PathBuf::from("/unused-model-root"),
        settings: humm_core::settings::Settings::default(),
        settings_path: PathBuf::from("/unused-settings-path"),
        engines: Box::new(NeverCalledEngineFactory),
        fetcher: None,
        capabilities: Capabilities::default(),
    })
}

/// Builds a harness over a fresh seeded store, widened well past the
/// table's full column width (~1600pt across all seven default-width
/// columns). `egui_kittest`'s default 800x600 viewport would otherwise clip
/// the later columns (Date Added, Notes) out of a synthetic click/tap's
/// reach — every test in this file needs every column reachable, not just
/// whichever happen to fit in 800pt.
fn wide_harness() -> Harness<'static, HummApp> {
    let store = Arc::new(seeded_store());
    let mut harness = Harness::new_eframe(move |_cc| HummApp::new_for_test(test_services(store)));
    harness.set_size(egui::Vec2::new(1700.0, 400.0));
    harness
}

#[test]
fn table_shows_three_seeded_rows_with_exact_buzz_status_texts() {
    let mut harness = wide_harness();
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
    let mut harness = wide_harness();
    harness.run();

    // Default sort: Date Added, descending (Buzz parity).
    harness.get_by_label_contains("Date Added \u{25bc}");

    harness.get_by_label_contains("Date Added \u{25bc}").click();
    harness.run();

    harness.get_by_label_contains("Date Added \u{25b2}");
}

#[test]
fn row_context_menu_gates_restart_to_failed_canceled_skipped() {
    let mut harness = wide_harness();
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
    let mut harness = wide_harness();
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
