//! kittest coverage for the Speaker-ID window (F-087, J-7, Task 11).
//!
//! The window is a deferred OS viewport (`app.rs::HummApp::
//! show_speaker_viewports`) an `egui_kittest` `Harness` cannot reach into —
//! the same constraint the viewer/resizer windows carry. So, exactly like
//! `kittest_viewer.rs` does for `viewer_ui`, most of this file drives
//! `views::speaker_id::ui` directly over a bare `egui::Ui`
//! (`Harness::new_ui_state`) against a hermetic [`SpeakerIdDeps`] (a fake
//! decoder/audio-output plus a [`ScriptedDiarizeService`]); the two tests
//! that need real app-level wiring (the window-open gate, and Save's
//! `close_source: false` event) go through `HummApp::new_for_test`'s own
//! `open_speakers_for_test`/`run_speaker_save_for_test` seams instead.
//!
//! Genuinely untestable here (documented honestly, not glossed over): the
//! real OS Speaker-ID viewport itself (`show_speaker_viewports`'s deferred-
//! viewport plumbing, focus/close-request wiring) and the sample-preview
//! `humm_audio::Player`'s actual audio output — both need a windowed run
//! (`docs/gui-manual`), the same carve-out every other deferred-viewport
//! window in this codebase already has.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{By, NodeT, Queryable};
use humm_core::boundary::{
    AudioDecoder, AudioOutput, FillCallback, OutputStream, Pcm, PlaybackPcm, StreamSpec, TaskStore,
};
use humm_core::cancel::CancelToken;
use humm_core::diarize::SpeakerTurn;
use humm_core::domain::{
    Backend, Input, Millis, ModelSize, ModelSpec, OutputSpec, Segment, Source, TaskKind, TaskSpec,
    TranscriptionId, TranscriptionOptions,
};
use humm_core::error::{AudioError, CoreError, MediaError};
use humm_core::queue::EngineFactory;
use humm_core::settings::Settings;
use humm_gui::app::HummApp;
use humm_gui::i18n::tr;
use humm_gui::services::{self, Capabilities, Services, TestDeps};
use humm_gui::speaker_id::{
    DiarizeService, ReadyState, ScriptedDiarizeService, SpeakerIdDeps, SpeakerIdStage, SpeakerIdVm,
};
use humm_gui::views::speaker_id::ui as speaker_id_ui;
use humm_store::SqliteStore;

// -- fakes: mirrors `kittest_viewer.rs`'s own `FakeDecoder`/`FakeOutput` ----
//    (decode() is exercised here, unlike that file's, since the Identify
//    worker actually calls it).

struct FakeDecoder;
impl AudioDecoder for FakeDecoder {
    fn decode(&self, _path: &Path, _cancel: &CancelToken) -> Result<Pcm, MediaError> {
        // A second of mono silence at 16 kHz — plenty for `assign_speakers`
        // to run against; the scripted `DiarizeService` never actually reads
        // it.
        Ok(Pcm::new(vec![0.0; 16_000]))
    }
    fn decode_playback(
        &self,
        _path: &Path,
        _cancel: &CancelToken,
    ) -> Result<PlaybackPcm, MediaError> {
        Ok(PlaybackPcm::new(vec![0.0; 44_100 * 2], 2, 44_100))
    }
}

struct FakeOutput;
struct NullStream;
impl OutputStream for NullStream {
    fn play(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn pause(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
}
impl AudioOutput for FakeOutput {
    fn open(
        &self,
        _spec: StreamSpec,
        _fill: FillCallback,
    ) -> Result<Box<dyn OutputStream>, AudioError> {
        Ok(Box::new(NullStream))
    }
}

fn seg(start: i64, end: i64, text: &str) -> Segment {
    Segment {
        start: Millis(start),
        end: Millis(end),
        text: text.to_string(),
        translation: String::new(),
    }
}

fn turn(start: i64, end: i64, speaker: usize) -> SpeakerTurn {
    SpeakerTurn {
        start: Millis(start),
        end: Millis(end),
        speaker,
    }
}

fn spec_for(file: &Path) -> TaskSpec {
    TaskSpec {
        input: Input::Path(file.to_path_buf()),
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
            temperature: vec![0.0],
            llm: None,
            extract_speech: false,
        },
        output: OutputSpec::default(),
    }
}

/// The render fixture: a loaded VM plus everything `views::speaker_id::ui`
/// needs.
struct Fixture {
    vm: SpeakerIdVm,
    deps: SpeakerIdDeps,
}

fn build_harness(fixture: Fixture) -> Harness<'static, Fixture> {
    Harness::new_ui_state(
        |ui, fixture: &mut Fixture| {
            speaker_id_ui(ui, &mut fixture.vm, &fixture.deps);
        },
        fixture,
    )
}

/// Seeds a temp-dir store with one `Completed` transcription (`segs`) and
/// opens a `SpeakerIdVm` over it, wired to `diarize` as its diarization seam.
/// `audio_exists` controls whether the source file is actually written to
/// disk — `SpeakerIdVm::open`'s `MissingAudio` gate reads real
/// `Path::is_file`, so this is how a test drives either side of that gate.
fn fixture_with(
    audio_exists: bool,
    segs: &[Segment],
    diarize: impl DiarizeService + 'static,
) -> (Fixture, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open(&dir.path().join("humm.db")).expect("open store"));
    let audio_path = dir.path().join("audio.wav");
    if audio_exists {
        std::fs::write(&audio_path, b"fake-audio-bytes").expect("write fake audio file");
    }
    let id = TranscriptionId::new();
    store
        .create_task(&id, &spec_for(&audio_path), Source::FileImport)
        .expect("create task");
    store.mark_completed(&id, segs).expect("mark completed");

    let deps = SpeakerIdDeps {
        store: Arc::clone(&store),
        decoder: Arc::new(FakeDecoder),
        audio_output: Arc::new(FakeOutput),
        diarize: Arc::new(diarize) as Arc<dyn DiarizeService>,
    };
    let vm = SpeakerIdVm::open(id, &deps).expect("open speaker-id vm");
    (Fixture { vm, deps }, dir)
}

/// Polls `check` until it returns `true`, or panics naming `what` after 5s —
/// the same deadline-loop idiom `kittest_viewer.rs`'s
/// `merge_save_failure_surfaces_error_and_keeps_windows` uses for its own
/// background-worker assertions.
fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if check() {
            return;
        }
        assert!(Instant::now() < deadline, "{what} did not happen in time");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// -- button_opens_window_and_missing_audio_gates --------------------------

/// A fake [`EngineFactory`] for an app-level speaker-id test that never runs
/// a job (every seeded row is already `Completed`; Identify/Save never touch
/// the queue).
struct NeverCalledEngineFactory;
impl EngineFactory for NeverCalledEngineFactory {
    fn create(
        &mut self,
        _model: &ModelSpec,
        _resolved: Option<&Path>,
    ) -> Result<Box<dyn humm_core::boundary::TranscriptionEngine>, CoreError> {
        unreachable!("a speaker-id test never enqueues a task")
    }
}

fn speaker_test_services(store: Arc<SqliteStore>, dir: &Path) -> Services {
    services::init_with(TestDeps {
        store,
        media_dir: dir.join("media"),
        model_root: dir.join("models"),
        settings: Settings::default(),
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

/// Opening a Speaker-ID window (the app-level effect the viewer's now-live
/// Identify Speakers toolbar button triggers, `open_speakers`) always
/// succeeds: with an existing audio file the window starts `Idle` (Identify
/// clickable); with a missing one it starts `MissingAudio` (Buzz `:514-518`
/// — Identify disabled), never refusing to open at all.
#[test]
fn button_opens_window_and_missing_audio_gates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open(&dir.path().join("humm.db")).expect("open store"));

    let with_audio = TranscriptionId::new();
    let audio_path = dir.path().join("has-audio.wav");
    std::fs::write(&audio_path, b"fake-audio-bytes").expect("write fake audio file");
    store
        .create_task(&with_audio, &spec_for(&audio_path), Source::FileImport)
        .expect("create task (with audio)");
    store
        .mark_completed(&with_audio, &[seg(0, 1000, "hello")])
        .expect("mark completed (with audio)");

    let missing_audio = TranscriptionId::new();
    let missing_path = dir.path().join("does-not-exist.wav");
    store
        .create_task(&missing_audio, &spec_for(&missing_path), Source::FileImport)
        .expect("create task (missing audio)");
    store
        .mark_completed(&missing_audio, &[seg(0, 1000, "hello")])
        .expect("mark completed (missing audio)");

    let mut app = HummApp::new_for_test(speaker_test_services(Arc::clone(&store), dir.path()));

    app.open_speakers_for_test(with_audio);
    assert!(app.is_speakers_open_for_test(with_audio));
    {
        let vm_arc = app.speaker_vm_for_test(with_audio).expect("window is open");
        let vm = vm_arc.lock().expect("lock vm");
        assert_eq!(vm.stage, SpeakerIdStage::Idle);
        assert!(
            vm.can_identify(),
            "Identify is clickable when the audio file exists"
        );
    }

    app.open_speakers_for_test(missing_audio);
    assert!(app.is_speakers_open_for_test(missing_audio));
    {
        let vm_arc = app
            .speaker_vm_for_test(missing_audio)
            .expect("window is open");
        let vm = vm_arc.lock().expect("lock vm");
        assert_eq!(vm.stage, SpeakerIdStage::MissingAudio);
        assert!(
            !vm.can_identify(),
            "Identify is disabled when the audio file is missing (Buzz :514-518)"
        );
    }

    // Re-opening an already-open window focuses it instead of duplicating
    // (the viewer/resizer deviation #1 shape) — still exactly one window.
    app.open_speakers_for_test(with_audio);
    assert!(app.is_speakers_open_for_test(with_audio));
}

// -- identify_runs_to_ready_with_speaker_rows ------------------------------

/// A scripted 2-speaker Identify run reaches `Ready` with two prefilled
/// "Speaker N" rows, the merge checkbox checked, and Save enabled — driven
/// all the way through the rendered window (not just the vm's own state).
#[test]
fn identify_runs_to_ready_with_speaker_rows() {
    let segments = [seg(0, 1000, "hello"), seg(1000, 2000, "world")];
    let turns = vec![turn(0, 1000, 5), turn(1000, 2000, 9)];
    let (mut fixture, _dir) = fixture_with(true, &segments, ScriptedDiarizeService::ready(turns));

    let ctx = egui::Context::default();
    let source = fixture.vm.source;
    fixture.vm.start_identify(&fixture.deps, &ctx);
    assert!(
        matches!(fixture.vm.stage, SpeakerIdStage::Running { .. }),
        "Running is installed synchronously on click, before the worker produces anything"
    );

    wait_until("identify to reach Ready", || {
        fixture.vm.poll_identify();
        !matches!(fixture.vm.stage, SpeakerIdStage::Running { .. })
    });

    match &fixture.vm.stage {
        SpeakerIdStage::Ready(ready) => {
            assert_eq!(
                ready.names,
                vec!["Speaker 0".to_string(), "Speaker 1".to_string()]
            );
            assert_eq!(ready.speakers, vec![0, 1]);
            assert!(ready.merge, "merge defaults checked (Buzz :574)");
        }
        other => panic!("expected Ready, got {other:?}"),
    }

    // The full start_identify -> worker thread -> set_stage chain must have
    // requested a repaint of *this window's own* deferred viewport (the
    // same `("speakers", id)` hash `app.rs::show_speaker_viewports` builds
    // its `ViewportId` from) at least once along the way — not just the
    // root viewport a bare `request_repaint()` would hit from a background
    // thread. Complements `speaker_id::tests::set_stage_requests_repaint_
    // of_the_given_viewport_not_root`'s narrower, `set_stage`-only check
    // with an end-to-end one through the real worker thread.
    let viewport_id = egui::ViewportId::from_hash_of(("speakers", source));
    assert!(
        ctx.has_requested_repaint_for(&viewport_id),
        "the Identify run must have requested a repaint of the speaker-id window's own viewport"
    );

    let mut harness = build_harness(fixture);
    harness.run();

    // The name rows are `TextEdit`s: their text lives in AccessKit's `value`,
    // not `label` (`label_contains` would never match a `TextEdit`'s content
    // — confirmed by first running this against the real render and reading
    // the dumped tree: `role: TextInput, value: "Speaker 0"`, no `label`
    // field at all). `role(TextInput)` disambiguates from the `TextRun`
    // child AccessKit also emits carrying the same `value`.
    harness.get(By::new().role(Role::TextInput).value("Speaker 0"));
    harness.get(By::new().role(Role::TextInput).value("Speaker 1"));
    assert!(
        !harness
            .get_by_label(&tr("speaker-id-save-button"))
            .accesskit_node()
            .is_disabled(),
        "Save is enabled once Step 1 reaches Ready"
    );
}

// -- cancel_mid_run_resets_to_idle -----------------------------------------

/// Cancel acks in the UI immediately (button disabled, "Cancelling…" label)
/// while the scripted worker is still genuinely blocked on a channel; once
/// released, the (discarded) result resolves the window back to `Idle` —
/// never `Ready`, even though the scripted service is told to succeed.
#[test]
fn cancel_mid_run_resets_to_idle() {
    let (tx, rx) = mpsc::channel::<()>();
    let scripted = ScriptedDiarizeService::blocking(rx, vec![turn(0, 1000, 0)]);
    let (mut fixture, _dir) = fixture_with(true, &[seg(0, 1000, "hello")], scripted);

    fixture
        .vm
        .start_identify(&fixture.deps, &egui::Context::default());
    assert!(matches!(fixture.vm.stage, SpeakerIdStage::Running { .. }));

    fixture.vm.cancel_identify();
    assert!(fixture.vm.cancel_requested_for_test());
    assert_eq!(
        fixture.vm.stage,
        SpeakerIdStage::Running {
            label: tr("speaker-id-cancelling"),
            fraction: None,
        },
        "the ack is immediate — the worker is still blocked on `rx` at this point"
    );

    // Let the (doomed) worker finish.
    tx.send(()).expect("release the blocked worker");

    wait_until("cancel to resolve to Idle", || {
        fixture.vm.poll_identify();
        !matches!(fixture.vm.stage, SpeakerIdStage::Running { .. })
    });
    assert_eq!(fixture.vm.stage, SpeakerIdStage::Idle);
    assert!(!fixture.vm.cancel_requested_for_test());
    assert!(
        fixture.vm.can_identify(),
        "Identify is clickable again after a cancel"
    );
}

// -- dropping_the_window_cancels_the_in_flight_identify_run ----------------

/// Closing the window (dropping the vm) cancels an in-flight Identify run —
/// Buzz `closeEvent` does the same (`:806-817`, stops the identification
/// thread and the media player). Without `impl Drop for SpeakerIdVm`, a
/// closed window would leave the worker thread (blocked here on the
/// scripted service's channel, standing in for a real ~10s `OrtDiarizer`
/// run) running to completion for nothing — and a close-then-reopen would
/// leave two concurrent pipelines running against the same transcript.
#[test]
fn dropping_the_window_cancels_the_in_flight_identify_run() {
    let (_tx, rx) = mpsc::channel::<()>();
    let scripted = ScriptedDiarizeService::blocking(rx, vec![turn(0, 1000, 0)]);
    let (fixture, _dir) = fixture_with(true, &[seg(0, 1000, "hello")], scripted);
    let Fixture { mut vm, deps } = fixture;

    vm.start_identify(&deps, &egui::Context::default());
    assert!(matches!(vm.stage, SpeakerIdStage::Running { .. }));

    let run_cancel = vm.run_cancel_token_for_test().expect("a run is in flight");
    assert!(!run_cancel.is_canceled());

    drop(vm); // the window closing

    assert!(
        run_cancel.is_canceled(),
        "closing the window must cancel the in-flight Identify run (Buzz closeEvent :806-817)"
    );
    // `_tx` drops at the end of this function, unblocking the scripted
    // worker's `recv()` so the orphaned thread finishes promptly rather
    // than lingering for the rest of the test process's life.
}

// -- error_shows_message_and_resets -----------------------------------------

/// A failed Identify run surfaces the adapter's message and resets Step 1 to
/// clickable again (humm's own "never silent" terminal state — Buzz's own
/// `on_identification_error` shows nothing at all, `speaker_identification_
/// widget.py:642-646`).
#[test]
fn error_shows_message_and_resets() {
    let (mut fixture, _dir) = fixture_with(
        true,
        &[seg(0, 1000, "hello")],
        ScriptedDiarizeService::failing("diarization exploded"),
    );

    fixture
        .vm
        .start_identify(&fixture.deps, &egui::Context::default());
    wait_until("identify to fail", || {
        fixture.vm.poll_identify();
        !matches!(fixture.vm.stage, SpeakerIdStage::Running { .. })
    });

    assert_eq!(
        fixture.vm.stage,
        SpeakerIdStage::Error("diarization exploded".to_string())
    );
    assert!(
        fixture.vm.can_identify(),
        "Identify is clickable again after an error"
    );

    let mut harness = build_harness(fixture);
    harness.run();
    harness.get_by_label_contains("diarization exploded");
    assert!(
        !harness
            .get_by_label(&tr("speaker-id-identify-button"))
            .accesskit_node()
            .is_disabled(),
        "Identify re-enables after the error"
    );
}

// -- save_writes_copied_transcription_with_labels --------------------------

/// Save copies the source into a **new** Completed transcript with labeled
/// (and, where names match, merged) segments, refreshes the task table for
/// that new row, and — the F-087 interface note — leaves the source viewer
/// untouched (`close_source: false`, unlike the resizer's own save path).
#[test]
fn save_writes_copied_transcription_with_labels() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open(&dir.path().join("humm.db")).expect("open store"));
    let source = TranscriptionId::new();
    let audio_path = dir.path().join("audio.wav");
    std::fs::write(&audio_path, b"fake-audio-bytes").expect("write fake audio file");
    store
        .create_task(&source, &spec_for(&audio_path), Source::FileImport)
        .expect("create source");
    let original_segs = vec![
        seg(0, 1000, "hello"),
        seg(1000, 2000, "there"),
        seg(2000, 3000, "goodbye"),
    ];
    store
        .mark_completed(&source, &original_segs)
        .expect("mark completed");

    let mut app = HummApp::new_for_test(speaker_test_services(Arc::clone(&store), dir.path()));
    app.open_viewer_for_test(source);
    app.open_speakers_for_test(source);
    assert!(app.is_viewer_open_for_test(source));
    assert!(app.is_speakers_open_for_test(source));

    // Force Step 1's outcome directly (Save cares only about `Ready`'s
    // contents, not how it was reached) — two of three segments share
    // "Alice" after naming, so `merge: true` must join them.
    {
        let vm_arc = app.speaker_vm_for_test(source).expect("window is open");
        let mut vm = vm_arc.lock().expect("lock vm");
        vm.stage = SpeakerIdStage::Ready(ReadyState {
            segments: original_segs.to_vec(),
            speakers: vec![0, 0, 1],
            names: vec!["Alice".to_string(), "Bob".to_string()],
            merge: true,
        });
    }

    let new_id = app
        .run_speaker_save_for_test(source)
        .expect("save succeeds and returns the new transcript's id");
    assert_ne!(new_id, source, "Save always creates a new copy (Buzz :784)");

    // The window closes; the source viewer does not (close_source: false).
    assert!(!app.is_speakers_open_for_test(source));
    assert!(
        app.is_viewer_open_for_test(source),
        "the source viewer is untouched by a speaker-id save"
    );

    // The table gained the new row.
    assert!(app.has_task_row_for_test(new_id));

    // The source transcript's own segments are untouched.
    let source_segs: Vec<Segment> = store
        .get_segment_rows(source)
        .expect("source segments")
        .into_iter()
        .map(|row| Segment {
            start: row.start,
            end: row.end,
            text: row.text,
            translation: row.translation,
        })
        .collect();
    assert_eq!(
        source_segs, original_segs,
        "the source row is never modified"
    );

    // The new row carries the labeled, merged segments.
    let new_segs: Vec<Segment> = store
        .get_segment_rows(new_id)
        .expect("new segments")
        .into_iter()
        .map(|row| Segment {
            start: row.start,
            end: row.end,
            text: row.text,
            translation: row.translation,
        })
        .collect();
    assert_eq!(
        new_segs,
        vec![
            Segment {
                start: Millis(0),
                end: Millis(2000),
                text: "Alice: hello there".to_string(),
                translation: String::new(),
            },
            Segment {
                start: Millis(2000),
                end: Millis(3000),
                text: "Bob: goodbye".to_string(),
                translation: String::new(),
            },
        ]
    );
}
