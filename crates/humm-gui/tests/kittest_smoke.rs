//! kittest smoke test (Task 6 brief, Step 4): a fresh, empty store shows the
//! empty-state hero, and the window renders for several frames without
//! panicking.
//!
//! Adapted from the brief's sketch to the real `egui_kittest` 0.35 API:
//! - `Harness::new_eframe` matches the brief verbatim.
//! - `Queryable::get_by_label_contains` returns the matched node directly
//!   (not `Option`) and panics on its own if nothing matches — so there is
//!   no `.unwrap()` to call; the panic-on-miss behavior is exactly the
//!   assertion the brief's `.unwrap()` was reaching for.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::{Harness, kittest::Queryable};
use humm_core::domain::ModelSpec;
use humm_core::error::CoreError;
use humm_core::queue::EngineFactory;
use humm_gui::app::HummApp;
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_store::SqliteStore;

/// A fake [`EngineFactory`] for tests that never actually run a job: this
/// smoke test only renders the empty-state hero, so nothing ever calls
/// `create`.
struct NeverCalledEngineFactory;

impl EngineFactory for NeverCalledEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn humm_core::boundary::TranscriptionEngine>, CoreError> {
        unreachable!("no task is ever enqueued in this smoke test")
    }
}

/// Builds [`Services`] from a fresh in-memory store — no real settings file,
/// database, or subprocess is touched. `_cc` is accepted only to match
/// `egui_kittest::Harness::new_eframe`'s builder-closure shape (a later task
/// may want it, e.g. to seed egui memory); it's unused here.
fn test_services(_cc: &eframe::CreationContext<'_>) -> Services {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open in-memory store"));
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

#[test]
fn app_shows_empty_state_hero_on_fresh_store() {
    let mut harness = Harness::new_eframe(|cc| HummApp::new_for_test(test_services(cc)));
    harness.run();
    harness.get_by_label_contains("Drop media here");
}

#[test]
fn app_renders_several_frames_without_panicking() {
    let mut harness = Harness::new_eframe(|cc| HummApp::new_for_test(test_services(cc)));
    for _ in 0..3 {
        harness.step();
    }
    // Still there after repeated frames — proves nothing panicked and the
    // hero isn't a one-shot render artifact.
    harness.get_by_label_contains("Drop media here");
}
