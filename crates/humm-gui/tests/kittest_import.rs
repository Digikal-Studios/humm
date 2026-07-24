//! kittest coverage for Task 9 (F-020/F-021/F-022 GUI/F-023): the options
//! sheet's file-count summary, the Run→enqueue→sticky-prefs round trip, and
//! the URL dialog's inline-error-stays-open contract.
//!
//! rfd's own (blocking, native) file/folder dialogs cannot run inside a
//! kittest harness — every scenario here opens the options sheet directly
//! via [`HummApp::open_options_sheet_for_test`] (the same test seam pattern
//! as `HummApp::inject_event_for_test`), which is exactly the state every
//! real import path (file dialog, drag-drop, folder scan, argv, confirmed
//! URL) resolves to before the sheet ever renders.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::NodeT;
use egui_kittest::{Harness, HarnessBuilder, kittest::Queryable};
use humm_core::boundary::{EngineCaps, EngineOptions, Pcm, SegmentSink, TranscriptionEngine};
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, ModelSize, ModelSpec, OutputFormat, TaskKind,
};
use humm_core::error::{CoreError, EngineError};
use humm_core::secrets::{FailingSecretStore, MemorySecretStore, SecretKey, SecretStore};
use humm_gui::app::HummApp;
use humm_gui::import::ImportRequest;
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_store::SqliteStore;

/// A [`humm_core::queue::EngineFactory`] that always succeeds with zero
/// segments — the enqueued tasks in this file point at nonexistent paths
/// (`/no/such/a.wav`), so the real decoder/prober (`services::init_with`'s
/// hard-coded real adapters) fail the job cleanly before this engine is
/// ever exercised; it exists only so `create` itself never has to be the
/// thing that fails.
struct FakeSucceedingEngineFactory;

impl humm_core::queue::EngineFactory for FakeSucceedingEngineFactory {
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

/// Builds real [`Services`] for the harness (Task 9's original
/// `test_services`, generalized in Task 14 to accept an explicit `secrets`
/// store — the options sheet's Api Key row seeds/commits through whatever
/// store is wired here, so a test asserting on that row needs to control it
/// directly rather than always getting a fresh, empty double).
fn test_services_with_secrets(
    store: Arc<SqliteStore>,
    capabilities: Capabilities,
    dir: &Path,
    secrets: Arc<dyn SecretStore>,
) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: dir.join("media"),
        model_root: dir.join("models"),
        settings: humm_core::settings::Settings::default(),
        settings_path: dir.join("settings.toml"),
        engines: Box::new(FakeSucceedingEngineFactory),
        fetcher: None,
        capabilities,
        artifacts: None,
        secrets,
        translate: services::default_test_translate(),
        llm: None,
        validate: services::default_test_validate(),
    })
}

/// Returns the harness alongside the `TempDir` backing its (unused but now
/// real, not sentinel-string) `media_dir`/`model_root`/`settings_path` —
/// the caller must keep it alive for the harness's lifetime.
fn harness_with(
    store: Arc<SqliteStore>,
    capabilities: Capabilities,
) -> (Harness<'static, HummApp>, tempfile::TempDir) {
    harness_with_secrets(store, capabilities, services::default_test_secrets())
}

/// Like [`harness_with`], but with an explicit `secrets` store (Task 14 —
/// see [`test_services_with_secrets`]).
fn harness_with_secrets(
    store: Arc<SqliteStore>,
    capabilities: Capabilities,
    secrets: Arc<dyn SecretStore>,
) -> (Harness<'static, HummApp>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let services = test_services_with_secrets(store, capabilities, dir.path(), secrets);
    let harness = HarnessBuilder::default()
        .with_size(egui::Vec2::new(1000.0, 700.0))
        // The default `max_steps` (4) is tuned for a static UI; a real
        // `HummApp` backed by `init_with`'s live queue/event-pump thread can
        // occasionally need a couple of extra `Harness::run` steps to settle
        // under event-pump churn, which otherwise flakes with a spurious
        // "exceeded max_steps" panic (final-review fix 5).
        .with_max_steps(32)
        .build_eframe(move |_cc| HummApp::new_for_test(services));
    (harness, dir)
}

/// Polls `store` until it has exactly `expected` rows (`list_tasks`), so a
/// test can wait out the queue's own enqueue → `create_task` round trip
/// (`Command::Enqueue` is processed asynchronously on the runner thread —
/// same rationale as `kittest_actions.rs`'s own poll loop for retry).
fn wait_for_task_count(store: &SqliteStore, expected: usize) -> Vec<humm_store::TaskRow> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = store.list_tasks().expect("list_tasks");
        if rows.len() >= expected {
            return rows;
        }
        assert!(
            Instant::now() < deadline,
            "expected {expected} task row(s), only saw {} before timeout",
            rows.len()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

// -- Options sheet: file-count summary ---------------------------------------

#[test]
fn open_options_sheet_shows_the_file_count_and_extract_speech_is_disabled() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let (mut harness, _dir) = harness_with(store, Capabilities::default());
    harness.run();

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![
            PathBuf::from("/no/such/a.wav"),
            PathBuf::from("/no/such/b.wav"),
        ]));
    harness.run();

    harness.get_by_label_contains("2 files");

    let extract_speech = harness.get_by_label("Extract speech");
    assert!(
        extract_speech.accesskit_node().is_disabled(),
        "Extract speech must be disabled this milestone"
    );
}

// -- Run -> enqueue -> sticky prefs -------------------------------------------

#[test]
fn run_with_srt_checked_enqueues_full_options_closes_the_sheet_and_stays_sticky_on_reopen() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let store_for_assert = Arc::clone(&store);
    let (mut harness, _dir) = harness_with(store, Capabilities::default());
    harness.run();

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![
            PathBuf::from("/no/such/a.wav"),
            PathBuf::from("/no/such/b.wav"),
        ]));
    harness.run();

    harness.get_by_label("SRT").click();
    harness.run();
    harness.get_by_label("Run").click();
    harness.run();

    // The sheet closed: its file-count header is gone.
    assert!(
        harness.query_by_label_contains("2 files").is_none(),
        "the options sheet should have closed after Run"
    );

    let rows = wait_for_task_count(&store_for_assert, 2);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let spec = store_for_assert
            .load_task_spec(&row.id)
            .expect("load_task_spec")
            .expect("a Run-enqueued task always has a persisted options_json");
        assert_eq!(
            spec.output.formats,
            [OutputFormat::Srt].into_iter().collect(),
            "only SRT was checked"
        );
        assert_eq!(spec.options.task, TaskKind::Transcribe, "sticky default");
        assert_eq!(spec.options.language, None, "Detect Language default");
        assert_eq!(
            spec.options.model,
            ModelSpec {
                backend: Backend::Whisper,
                size: ModelSize::Small,
                hf_repo: None,
            },
            "default model (no hf_repo entered)"
        );
        assert!(!spec.options.word_timestamps);
        assert_eq!(spec.options.temperature, DEFAULT_TEMPERATURE.to_vec());
    }

    // Reopen with a different request: SRT must still be checked (sticky
    // `file_transcriber.export_formats`, written back on the previous Run).
    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![PathBuf::from("/no/such/c.wav")]));
    harness.run();
    harness.get_by_label_contains("1 file");

    let srt_checkbox = harness.get_by_label("SRT");
    assert_eq!(
        srt_checkbox.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "SRT must still be checked on reopen (sticky prefs)"
    );
}

// -- Task 14: backend selector + Api Key row + LLM translation block ---------

/// Selecting the OpenAI backend hides the size combo and shows the Api Key
/// row; switching back to Whisper restores it (design spec §7.2 deliverable
/// A). The backend combo is driven via
/// [`HummApp::options_sheet_state_mut_for_test`] rather than clicking the
/// `egui::ComboBox` itself — this workspace has no kittest precedent for
/// driving a `ComboBox` selection by click (`kittest_recording.rs`'s
/// `mode_index` seam documents the same gap for the recording window).
#[test]
fn api_backend_selection_hides_size_combo_and_shows_api_key_row() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let (mut harness, _dir) = harness_with(store, Capabilities::default());
    harness.run();

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![PathBuf::from("/no/such/a.wav")]));
    harness.run();

    // Whisper is the default backend: the size combo shows, the Api Key row
    // doesn't.
    harness.get_by_label("Model:");
    assert!(harness.query_by_label("Api Key:").is_none());

    harness
        .state_mut()
        .options_sheet_state_mut_for_test()
        .expect("sheet is open")
        .backend = Backend::OpenAiApi;
    harness.run();

    assert!(
        harness.query_by_label("Model:").is_none(),
        "the size combo must hide once the API backend is selected"
    );
    harness.get_by_label("Api Key:");

    // Switching back to Whisper restores the size combo and hides the row
    // again.
    harness
        .state_mut()
        .options_sheet_state_mut_for_test()
        .expect("sheet is open")
        .backend = Backend::Whisper;
    harness.run();

    harness.get_by_label("Model:");
    assert!(harness.query_by_label("Api Key:").is_none());
}

/// The Advanced section's Hugging Face repo field hides for the same reason
/// the size combo does (design spec §7.2 deliverable A's decision to match
/// Buzz's `reset_visible_rows` intent).
#[test]
fn api_backend_selection_hides_the_advanced_hf_repo_field() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let (mut harness, _dir) = harness_with(store, Capabilities::default());
    harness.run();

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![PathBuf::from("/no/such/a.wav")]));
    harness.run();
    harness.get_by_label("Advanced").click();
    harness.run();

    harness.get_by_label("Hugging Face model");

    harness
        .state_mut()
        .options_sheet_state_mut_for_test()
        .expect("sheet is open")
        .backend = Backend::OpenAiApi;
    harness.run();

    assert!(harness.query_by_label("Hugging Face model").is_none());
}

/// The Advanced section's "Translation settings" block enable-gates its
/// AI-model/instructions fields on the checkbox (Buzz
/// `advanced_settings_dialog.py:89-121`).
#[test]
fn llm_block_enable_gates_its_fields() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let (mut harness, _dir) = harness_with(store, Capabilities::default());
    harness.run();

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![PathBuf::from("/no/such/a.wav")]));
    harness.run();
    harness.get_by_label("Advanced").click();
    harness.run();

    let checkbox = harness.get_by_label("Enable AI translation");
    assert_eq!(
        checkbox.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False),
        "unchecked by default (sticky default, design spec §7.2)"
    );

    let ai_model_label = harness.get_by_label("AI model:");
    assert!(
        ai_model_label.accesskit_node().is_disabled(),
        "the AI-model row must render disabled while unchecked"
    );
    let instructions_label = harness.get_by_label("Instructions for AI:");
    assert!(
        instructions_label.accesskit_node().is_disabled(),
        "the instructions row must render disabled while unchecked"
    );

    harness.get_by_label("Enable AI translation").click();
    harness.run();

    assert!(
        !harness
            .get_by_label("AI model:")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !harness
            .get_by_label("Instructions for AI:")
            .accesskit_node()
            .is_disabled()
    );
}

/// Running with "Enable AI translation" checked (and the seeded defaults
/// untouched, ledger #10) enqueues a spec whose persisted options carry the
/// seeded model/prompt — and, critically, never the API key material itself
/// (design spec §5's acceptance invariant), even though a real key is
/// sitting in the secret store this whole time.
#[test]
fn run_with_llm_enabled_persists_llm_options_and_never_the_key() {
    const TEST_KEY: &str = "sk-test-dummy-should-never-leak-12345";

    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let store_for_assert = Arc::clone(&store);
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    secrets.set(SecretKey::OpenAiApiKey, TEST_KEY).unwrap();
    let (mut harness, _dir) = harness_with_secrets(store, Capabilities::default(), secrets);
    harness.run();

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![PathBuf::from("/no/such/a.wav")]));
    harness.run();
    harness.get_by_label("Advanced").click();
    harness.run();

    harness.get_by_label("Enable AI translation").click();
    harness.run();
    harness.get_by_label("Run").click();
    harness.run();

    let rows = wait_for_task_count(&store_for_assert, 1);
    let spec = store_for_assert
        .load_task_spec(&rows[0].id)
        .expect("load_task_spec")
        .expect("a Run-enqueued task always has a persisted options_json");

    // Re-serialize exactly what `SqliteStore::create_task` persists as
    // `options_json` (`serde_json::to_string(spec)` over the *whole* spec,
    // not just `spec.options`; `store.rs:1264`) and prove the key material
    // never made it in -- `LlmTranslationOptions` has structurally no key
    // field, so this also guards against a future field addition
    // accidentally carrying one.
    let options_json = serde_json::to_string(&spec).expect("spec serialize");

    let llm = spec.options.llm.expect("LLM translation was enabled");
    assert!(!llm.model.is_empty());
    assert!(!llm.prompt.is_empty());
    assert!(
        options_json.contains("\"llm\""),
        "persisted options must carry the llm key: {options_json}"
    );
    assert!(
        !options_json.contains(TEST_KEY),
        "the API key must never appear in persisted options_json: {options_json}"
    );
}

/// Committing the Api Key row against a [`SecretStore`] whose `set` always
/// fails must surface the keyring-save-failure modal
/// (`options_sheet::SheetModal::KeyringSaveFailed`, a nested `egui::Modal`
/// stacked over the sheet's own) with Buzz's exact error body. No existing
/// double could exercise this branch -- [`MemorySecretStore::set`] never
/// errs -- hence [`FailingSecretStore`], whose every `set` call fails.
///
/// Drives the real Api Key `TextEdit` (focus, type, then Tab away) rather
/// than a state seam, since the commit only fires on the widget's own
/// `lost_focus()` (`options_sheet::render_api_key_row`) -- the same
/// focus/type/Tab-away idiom `kittest_prefs.rs`'s
/// `custom_url_field_persists_on_lost_focus_not_per_keystroke` uses, safe
/// here for the same reason: with the backend switched to `OpenAiApi` and
/// Advanced left collapsed, the Api Key field is the sheet's only visible
/// text field, so `get_by_role` resolves unambiguously. Its role is
/// `PasswordInput`, not `TextInput` -- `masked_field_row` renders it masked
/// (`.password(!*visible)`) by default, since `state.api_key_visible` starts
/// `false`.
#[test]
fn api_key_commit_shows_the_keyring_failure_modal_when_the_secret_store_errors() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let secrets: Arc<dyn SecretStore> = Arc::new(FailingSecretStore::new());
    let (mut harness, _dir) = harness_with_secrets(store, Capabilities::default(), secrets);
    harness.run();

    harness
        .state_mut()
        .open_options_sheet_for_test(ImportRequest::Files(vec![PathBuf::from("/no/such/a.wav")]));
    harness.run();

    harness
        .state_mut()
        .options_sheet_state_mut_for_test()
        .expect("sheet is open")
        .backend = Backend::OpenAiApi;
    harness.run();

    harness
        .get_by_role(egui::accesskit::Role::PasswordInput)
        .focus();
    harness.run();
    harness
        .get_by_role(egui::accesskit::Role::PasswordInput)
        .type_text("sk-will-fail-to-save");
    harness.run();
    harness.key_press(egui::Key::Tab);
    harness.run();

    harness.get_by_label_contains("Unable to save OpenAI API key to keyring");
}

// -- URL dialog: invalid input stays open with the exact Buzz message --------

#[test]
fn url_dialog_invalid_input_shows_the_exact_error_and_stays_open() {
    let store = Arc::new(SqliteStore::open_in_memory().expect("open"));
    let capabilities = Capabilities {
        yt_dlp: true,
        ..Capabilities::default()
    };
    let (mut harness, _dir) = harness_with(store, capabilities);
    harness.run();

    harness.get_by_label("New URL Transcription").click();
    harness.run();

    // Ok is always enabled, even with the (default-empty) field untouched —
    // an empty string is itself one of the reject vectors. Buzz relabels
    // this dialog's button "Ok" (not "OK" — see `url-dialog-ok`'s doc
    // comment in the `.ftl`).
    harness.get_by_label("Ok").click();
    harness.run();

    harness.get_by_label_contains("The URL you entered is invalid.");
    // The dialog is still open: its text field is still on screen.
    harness.get_by_role(egui::accesskit::Role::TextInput);
}
