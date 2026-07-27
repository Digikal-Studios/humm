//! The transcript **resizer** window (F-086, design spec §2.6): a separate
//! window — opened by the viewer's Resize toolbar button — that extends,
//! resizes, or merges a completed transcript's segments and saves the result
//! either as a fresh transcript ("Create new transcript") or in place.
//!
//! [`ResizerVm`] is the per-window state (one per source transcript); this
//! module renders it ([`ui`]) and owns the pure input rules
//! ([`parse_extend_amount`], [`ResizerVm::build_algo`]) and the save semantics
//! ([`save_segments`]). It is deliberately app-free: like the viewer's chrome,
//! the render function only *sets intents* on the VM (`pending_action`,
//! `close_requested`) that `app.rs` harvests each frame — the harvest is what
//! reads the store, runs [`humm_core::resize`]/[`humm_core::regroup`], writes
//! the result, refreshes the task table, and closes the originating viewer +
//! this window (Buzz `transcriptions_updated_signal` → viewer `close`,
//! `transcription_viewer_widget.py:1333`). Extend/Resize run synchronously
//! (fast, pure); Merge runs on a background thread (its regroup is heavier and
//! its output is only meaningful with word-level timings) and the window hides
//! while it runs.
//!
//! Ported from Buzz's `TranscriptionResizerWidget`
//! (`buzz/widgets/transcription_viewer/transcription_resizer_widget.py`):
//! sections/labels/defaults/enablement (`:183-330`), extend (`:398-428`),
//! resize (`:360-396`), the merge regroup-string build (`:442-459`), and
//! `save_segments`/`on_transcription_completed` (`:337-352,479-489`).

use std::path::Path;
use std::sync::Mutex;

use egui::RichText;
use humm_core::domain::{Millis, Segment, TranscriptionId};
use humm_core::error::StoreError;
use humm_core::regroup::{default_algo, override_algo};
use humm_core::settings::Settings;
use humm_store::SqliteStore;

use crate::i18n::tr;
use crate::viewer::ViewerDeps;

/// Buzz's default "Split by punctuation" field value, byte-exact
/// (`transcription_resizer_widget.py:302`) — including its trailing space. A
/// stable-ts regroup `sp=` list of `str`/`pair` splitters (`". "`, `"? "`,
/// `", "`, and the CJK full-width variants).
const DEFAULT_PUNCT: &str = ".* /./. /。/?/? /？/!/! /！/,/, ";

/// Which resize operation the user triggered this frame — set on
/// [`ResizerVm::pending_action`] by [`ui`], harvested and executed by
/// `app.rs` (which owns the store, the task table, and the viewer/resizer
/// window maps). Extend/Resize apply synchronously; Merge starts a worker
/// thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizerAction {
    /// "Extend endings" — push each segment's end out by the parsed amount,
    /// clamped to the next segment's start ([`humm_core::resize::extend_endings`]).
    Extend,
    /// "Resize" — split each segment to at most `target_chars`
    /// ([`humm_core::resize::equalize`]).
    Resize,
    /// "Merge" — regroup by gap/punctuation/length
    /// ([`humm_core::regroup::regroup_segments`]).
    Merge,
}

/// One resizer window's state (F-086). The first nine fields are the resizer's
/// form model per the task brief; the rest are window-lifecycle plumbing the
/// deferred-viewport host in `app.rs` reads/writes (the same shape
/// [`crate::viewer::ViewerVm`] uses for its own window).
pub struct ResizerVm {
    /// The transcript this resizer operates on (the "source"). Its viewer is
    /// the "originating" window that closes when an operation lands.
    pub source: TranscriptionId,
    /// "Create new transcript" checkbox (persisted to
    /// `settings.viewer.resizer_create_new_transcript`): save as a fresh
    /// transcript rather than replacing the source's segments in place.
    pub create_new: bool,
    /// "Extend endings by up to (seconds)" field (default `"0.2"`; unparseable
    /// → 0.2 s, [`parse_extend_amount`]).
    pub extend_amount: String,
    /// "Desired subtitle length" spin value (Buzz 1–100, default 42).
    pub target_chars: u32,
    /// "Merge by gap" checkbox + its gap-seconds field (default `(true, "0.2")`).
    pub merge_by_gap: (bool, String),
    /// "Split by punctuation" checkbox + its punctuation-list field
    /// (default `(true, DEFAULT_PUNCT)`).
    pub split_by_punct: (bool, String),
    /// "Split by max length" checkbox + its length field (default `(true, "42")`).
    pub split_by_len: (bool, String),
    /// Whether the source was transcribed with word-level timings: gates the
    /// Resize section (enabled iff **off**) and the Merge section (enabled iff
    /// **on**). Read from `load_task_spec` else `spec_from_columns`.
    pub word_timestamps: bool,
    /// Whether a Merge worker is currently running (the window hides while it
    /// is; a `RegroupError` re-shows it with [`Self::error`] set).
    pub running: bool,

    /// Window title — the source file's basename (Buzz `file_path_as_title`).
    pub(crate) title: String,
    /// The source transcript's language code, captured at open (for the merge
    /// pipeline's non-space-language separator rule). `None` when unset.
    pub(crate) language: Option<String>,
    /// A resizer error to surface as an inline banner: an unsupported regroup
    /// op (via `HUMM_MERGE_REGROUP_RULE`) or a store save failure on Extend/
    /// Resize/Merge (final review, F-086). Already the full localized message
    /// (label + detail) — the caller that knows which failure occurred picks
    /// the label at set time, so [`show_error_banner`] just displays it
    /// verbatim. Cleared when a new operation starts.
    pub(crate) error: Option<String>,
    /// Set by the render closure when the OS window's close is requested; the
    /// app harvests it each frame to drop this window (and, unlike the viewer,
    /// nothing else — closing a resizer never closes its viewer).
    pub(crate) close_requested: bool,
    /// Set when the source is re-resized while this window already exists
    /// (focus it instead of duplicating — the viewer's deviation #1 shape).
    pub(crate) focus_requested: bool,
    /// One-shot intent latch: which action button was clicked this frame, taken
    /// by the app's harvest. `None` most frames.
    pub(crate) pending_action: Option<ResizerAction>,
    /// Tracks the OS window's shown/hidden state so the closure only sends a
    /// `ViewportCommand::Visible` on an actual transition (hide on merge start,
    /// re-show on merge error).
    pub(crate) window_visible: bool,
}

impl ResizerVm {
    /// Opens a resizer view-model for `source`: derives the window title from
    /// the source file's basename, seeds "Create new transcript" from
    /// `settings.viewer.resizer_create_new_transcript`, and reads the
    /// word-timestamps gate + language from the source's persisted options
    /// (`load_task_spec`, falling back to `spec_from_columns` for an imported
    /// row). Every other field takes its Buzz default.
    pub fn open(
        source: TranscriptionId,
        deps: &ViewerDeps,
        settings: &Settings,
    ) -> Result<Self, StoreError> {
        // Word-timestamps gate + language: the full persisted spec when present
        // (every humm-created row), else the tolerant raw-column fallback (an
        // imported Buzz row with a NULL `options_json` still carries
        // `word_level_timings`/`language`).
        let spec = match deps.store.load_task_spec(&source)? {
            Some(spec) => Some(spec),
            None => deps.store.spec_from_columns(&source)?,
        };
        let word_timestamps = spec
            .as_ref()
            .map(|spec| spec.options.word_timestamps)
            .unwrap_or(false);
        let language = spec
            .as_ref()
            .and_then(|spec| spec.options.language)
            .map(|lang| lang.code().to_string());

        let file = deps.store.get_task(&source)?.and_then(|task| task.file);
        let title = file.as_deref().map(file_basename).unwrap_or_default();

        Ok(ResizerVm {
            source,
            create_new: settings.viewer.resizer_create_new_transcript,
            extend_amount: "0.2".to_string(),
            target_chars: 42,
            merge_by_gap: (true, "0.2".to_string()),
            split_by_punct: (true, DEFAULT_PUNCT.to_string()),
            split_by_len: (true, "42".to_string()),
            word_timestamps,
            running: false,
            title,
            language,
            error: None,
            close_requested: false,
            focus_requested: false,
            pending_action: None,
            window_visible: true,
        })
    }

    /// The recorded error banner text (final review, F-086), for an
    /// integration test to assert on across the crate boundary — `error`
    /// itself is `pub(crate)` since only `app.rs`'s harvest sets it.
    pub fn error_for_test(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Builds Buzz's regroup algorithm string from the three merge
    /// checkboxes/fields ([`default_algo`], `transcription_resizer_widget.py:
    /// 442-459`) — *before* the `HUMM_MERGE_REGROUP_RULE` override. A thin
    /// wiring shim so the resizer's form state feeds the core builder verbatim;
    /// kept env-independent so the truth-table unit test is deterministic. The
    /// override is applied by [`merge_algo_for`], which is what the merge worker
    /// actually runs.
    pub fn build_algo(&self) -> String {
        default_algo(
            self.merge_by_gap.0,
            &self.merge_by_gap.1,
            self.split_by_punct.0,
            &self.split_by_punct.1,
            self.split_by_len.0,
            &self.split_by_len.1,
        )
    }
}

/// The regroup string the merge worker actually runs (design spec §2.6 —
/// binding): the resizer's built algorithm ([`ResizerVm::build_algo`]) with
/// Buzz's `HUMM_MERGE_REGROUP_RULE` override applied — `os.getenv(
/// "BUZZ_MERGE_REGROUP_RULE", regroup_string)`
/// (`transcription_resizer_widget.py:459`, humm's env-var rename). Reads the
/// env once and delegates the "override wins when set" rule to core's
/// [`override_algo`] (the same value-threaded core
/// [`humm_core::regroup::algo_from_env_or`] uses) rather than re-deriving it
/// here — reading the env in this thin wrapper (instead of calling
/// `algo_from_env_or` directly) keeps the GUI able to unit-test the rule by
/// threading explicit values (`humm-gui` uses no `unsafe`, and
/// `std::env::set_var` is `unsafe` in edition 2024).
pub fn merge_algo_for(vm: &ResizerVm) -> String {
    override_algo(
        vm.build_algo(),
        std::env::var("HUMM_MERGE_REGROUP_RULE").ok().as_deref(),
    )
}

/// Parses the "Extend endings by up to (seconds)" field to a millisecond
/// amount, Buzz-exact (`on_extend_button_clicked`,
/// `transcription_resizer_widget.py:398-405`): `float(text)` seconds →
/// `int(seconds * 1000)` ms, with an unparseable (or non-finite) value falling
/// back to Buzz's `0.2` s default (= 200 ms). Whitespace is trimmed first
/// (Python's `float()` strips surrounding whitespace); the `* 1000` truncation
/// toward zero matches Python's `int()`.
pub fn parse_extend_amount(s: &str) -> Millis {
    let seconds = s
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .unwrap_or(0.2);
    Millis((seconds * 1000.0) as i64)
}

/// The resize/merge save path (Buzz `save_segments` /
/// `on_transcription_completed`, `transcription_resizer_widget.py:337-352,
/// 479-489`): with "Create new transcript" on, copy the source row to a fresh
/// id and mark it completed with `segs`; otherwise replace the source's
/// segments in place. Either way translations are dropped (the store's
/// `mark_replaced_completed`/`replace_segments` insert an empty translation —
/// Buzz parity). Returns the *affected* id (the new copy, or the source).
pub fn save_segments(
    store: &SqliteStore,
    source: TranscriptionId,
    create_new: bool,
    segs: &[Segment],
) -> Result<TranscriptionId, StoreError> {
    if create_new {
        let new_id = store.copy_transcription(source)?;
        store.mark_replaced_completed(new_id, segs)?;
        Ok(new_id)
    } else {
        store.replace_segments(source, segs)?;
        Ok(source)
    }
}

/// Renders one resizer window's content into `ui`. A pure render of
/// [`ResizerVm`] (plus the checkbox-persist side effect): action clicks set
/// [`ResizerVm::pending_action`] for the app to harvest; the "Create new
/// transcript" toggle persists immediately. `settings`/`settings_path` are for
/// that one persist, mirroring `viewer_ui`'s find-bar persistence.
pub fn ui(ui: &mut egui::Ui, vm: &mut ResizerVm, settings: &Mutex<Settings>, settings_path: &Path) {
    // While a merge worker runs, the OS window is hidden (`ViewportCommand::
    // Visible(false)` in the host closure); render a plain busy label in case
    // the hide has not taken effect yet, and touch no form state.
    if vm.running {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.label(tr("resizer-merging"));
        });
        return;
    }

    show_error_banner(ui, vm);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            settings_section(ui, vm, settings, settings_path);
            ui.add_space(8.0);
            extend_section(ui, vm);
            ui.add_space(8.0);
            resize_section(ui, vm);
            ui.add_space(8.0);
            merge_section(ui, vm);
        });
}

/// A bold section heading (Buzz sets each section's `QLabel` font weight bold).
fn heading(ui: &mut egui::Ui, key: &str) {
    ui.label(RichText::new(tr(key)).strong());
}

/// "Settings" section: the persisted "Create new transcript" checkbox.
fn settings_section(
    ui: &mut egui::Ui,
    vm: &mut ResizerVm,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    heading(ui, "resizer-settings");
    ui.group(|ui| {
        let mut create_new = vm.create_new;
        if ui
            .checkbox(&mut create_new, tr("resizer-create-new"))
            .changed()
        {
            vm.create_new = create_new;
            persist_create_new(create_new, settings, settings_path);
        }
    });
}

/// "Extend end time" section (always enabled): amount field + Extend button.
fn extend_section(ui: &mut egui::Ui, vm: &mut ResizerVm) {
    heading(ui, "resizer-extend-heading");
    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("resizer-extend-label"));
            ui.add(egui::TextEdit::singleline(&mut vm.extend_amount).desired_width(60.0));
            if ui.button(tr("resizer-extend-button")).clicked() {
                vm.pending_action = Some(ResizerAction::Extend);
            }
        });
    });
}

/// "Resize Options" section: enabled only when word timestamps were **off**
/// (Buzz `resize_group_box.setEnabled(word_level_timings != 1)` + tooltip).
fn resize_section(ui: &mut egui::Ui, vm: &mut ResizerVm) {
    heading(ui, "resizer-resize-heading");
    let enabled = !vm.word_timestamps;
    let group = ui.add_enabled_ui(enabled, |ui| {
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("resizer-resize-label"));
                ui.add(egui::DragValue::new(&mut vm.target_chars).range(1..=100));
                if ui.button(tr("resizer-resize-button")).clicked() {
                    vm.pending_action = Some(ResizerAction::Resize);
                }
            });
        });
    });
    if !enabled {
        group
            .response
            .on_hover_text(tr("resizer-resize-disabled-tooltip"));
    }
}

/// "Merge Options" section: enabled only when word timestamps were **on**
/// (Buzz `merge_options_group_box.setEnabled(word_level_timings == 1)` +
/// tooltip). Three checkbox+field rows and the Merge button.
fn merge_section(ui: &mut egui::Ui, vm: &mut ResizerVm) {
    heading(ui, "resizer-merge-heading");
    let enabled = vm.word_timestamps;
    let group = ui.add_enabled_ui(enabled, |ui| {
        ui.group(|ui| {
            merge_row(ui, "resizer-merge-by-gap", &mut vm.merge_by_gap);
            merge_row(ui, "resizer-split-by-punctuation", &mut vm.split_by_punct);
            merge_row(ui, "resizer-split-by-max-length", &mut vm.split_by_len);
            if ui.button(tr("resizer-merge-button")).clicked() {
                vm.pending_action = Some(ResizerAction::Merge);
            }
        });
    });
    if !enabled {
        group
            .response
            .on_hover_text(tr("resizer-merge-disabled-tooltip"));
    }
}

/// One merge checkbox + its adjoining text field.
fn merge_row(ui: &mut egui::Ui, label_key: &str, field: &mut (bool, String)) {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut field.0, tr(label_key));
        ui.add(egui::TextEdit::singleline(&mut field.1).desired_width(180.0));
    });
}

/// The inline error banner (a `RegroupError` from an unsupported op via
/// `HUMM_MERGE_REGROUP_RULE`, or a store save failure on Extend/Resize/Merge,
/// final review F-086): the same "banner, not toast" shape the viewer uses (a
/// deferred viewport cannot reach the app's toast stack). `vm.error` is
/// already the full localized message — set at the call site that knows which
/// failure occurred — so this just displays it.
fn show_error_banner(ui: &mut egui::Ui, vm: &ResizerVm) {
    let Some(message) = &vm.error else {
        return;
    };
    let color = ui.visuals().warn_fg_color;
    ui.colored_label(color, message);
    ui.separator();
}

/// Persists `create_new` to `settings.viewer.resizer_create_new_transcript`
/// (Buzz `on_create_new_transcript_toggled`) — the diff-on-change,
/// non-fatal-on-save-failure contract shared by every other viewer setting.
fn persist_create_new(create_new: bool, settings: &Mutex<Settings>, settings_path: &Path) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.viewer.resizer_create_new_transcript == create_new {
        return;
    }
    guard.viewer.resizer_create_new_transcript = create_new;
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the resizer create-new preference: {err}");
    }
}

/// The window title for a media file path: its basename (Buzz
/// `paths.file_path_as_title` = `os.path.basename`).
fn file_basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::boundary::TaskStore;
    use humm_core::domain::{
        Backend, DEFAULT_TEMPERATURE, Input, ModelSize, ModelSpec, OutputSpec, Source, Status,
        TaskKind, TaskSpec, TranscriptionOptions,
    };
    use humm_core::language::Language;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn seg(start: i64, end: i64, text: &str, translation: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.into(),
            translation: translation.into(),
        }
    }

    fn spec_with(word_timestamps: bool, language: Option<Language>) -> TaskSpec {
        TaskSpec {
            input: Input::Path(PathBuf::from("/tmp/clip.wav")),
            options: TranscriptionOptions {
                language,
                task: TaskKind::Transcribe,
                model: ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                word_timestamps,
                initial_prompt: String::new(),
                temperature: DEFAULT_TEMPERATURE.to_vec(),
                llm: None,
            },
            output: OutputSpec::default(),
        }
    }

    // -- parse_extend_amount ------------------------------------------------

    #[test]
    fn parse_extend_amount_fallback() {
        // Unparseable → Buzz's 0.2 s default = 200 ms.
        assert_eq!(parse_extend_amount(""), Millis(200));
        assert_eq!(parse_extend_amount("abc"), Millis(200));
        assert_eq!(parse_extend_amount("  "), Millis(200));
        assert_eq!(parse_extend_amount("inf"), Millis(200));
        // Parseable seconds → int(seconds * 1000) ms (truncating toward zero).
        assert_eq!(parse_extend_amount("0.2"), Millis(200));
        assert_eq!(parse_extend_amount("0.5"), Millis(500));
        assert_eq!(parse_extend_amount(" 1 "), Millis(1000));
        assert_eq!(parse_extend_amount("1.9995"), Millis(1999));
    }

    // -- word-timestamps gate (both row shapes) -----------------------------

    /// Minimal Buzz schema (verbatim subset of the store crate's own import
    /// fixture) — just enough for `import_buzz_db` to attach and copy from, so
    /// the imported row has a genuinely `NULL` `options_json` (the
    /// column-fallback path), exactly like a migrated Buzz history.
    const BUZZ_FIXTURE_SCHEMA: &str = "
        CREATE TABLE transcription (
            id TEXT PRIMARY KEY, error_message TEXT, export_formats TEXT, file TEXT,
            output_folder TEXT, progress DOUBLE PRECISION DEFAULT 0.0, language TEXT,
            model_type TEXT, source TEXT, status TEXT, task TEXT, time_ended TIMESTAMP,
            time_queued TIMESTAMP NOT NULL, time_started TIMESTAMP, url TEXT,
            whisper_model_size TEXT, hugging_face_model_id TEXT,
            word_level_timings BOOLEAN DEFAULT FALSE, extract_speech BOOLEAN DEFAULT FALSE,
            name TEXT, notes TEXT
        );
        CREATE TABLE transcription_segment (
            id INTEGER PRIMARY KEY, end_time INT DEFAULT 0, start_time INT DEFAULT 0,
            text TEXT NOT NULL, translation TEXT DEFAULT '', transcription_id TEXT,
            FOREIGN KEY (transcription_id) REFERENCES transcription(id) ON DELETE CASCADE
        );
    ";

    fn import_foreign_row(store: &SqliteStore, path: &Path, id: &str, word_level_timings: bool) {
        let conn = rusqlite::Connection::open(path).expect("open fixture db");
        conn.execute_batch(BUZZ_FIXTURE_SCHEMA)
            .expect("apply buzz schema");
        conn.execute(
            "INSERT INTO transcription (
                id, file, model_type, source, status, task, time_queued,
                language, whisper_model_size, word_level_timings, extract_speech
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id,
                "/buzz/foreign.wav",
                "Whisper.cpp",
                "file_import",
                "completed",
                "transcribe",
                "2024-01-01T10:00:00.000000",
                "en",
                "small",
                word_level_timings,
                false,
            ],
        )
        .expect("insert foreign row");
        drop(conn);
        store.import_buzz_db(path).expect("import_buzz_db");
    }

    #[test]
    fn merge_gate_from_options_json_and_column_fallback() {
        // Shape 1: a humm-created row with a persisted spec (options_json).
        let store = SqliteStore::open_in_memory().expect("open");
        let wt_id = TranscriptionId::new();
        store
            .create_task(
                &wt_id,
                &spec_with(true, Language::parse("en")),
                Source::FileImport,
            )
            .expect("create wt");
        let no_wt_id = TranscriptionId::new();
        store
            .create_task(&no_wt_id, &spec_with(false, None), Source::FileImport)
            .expect("create no-wt");

        // Read the gate via the same load_task_spec-then-spec_from_columns
        // resolution ResizerVm::open uses (asserted directly on the store to
        // avoid needing a full ViewerDeps here — the kittest covers open()).
        assert!(
            gate_word_timestamps(&store, wt_id),
            "options_json word_timestamps=true"
        );
        assert!(
            !gate_word_timestamps(&store, no_wt_id),
            "options_json word_timestamps=false"
        );

        // Shape 2: an imported Buzz row (options_json NULL) — the fallback path.
        let dir = tempfile::tempdir().expect("tempdir");
        let fixture = dir.path().join("Buzz.sqlite");
        let foreign = "11111111-1111-1111-1111-111111111111";
        import_foreign_row(&store, &fixture, foreign, true);
        let foreign_id = TranscriptionId::parse(foreign).expect("uuid");
        // Premise: options_json really is NULL for the imported row.
        assert_eq!(store.load_task_spec(&foreign_id).expect("load"), None);
        assert!(
            gate_word_timestamps(&store, foreign_id),
            "column-fallback word_level_timings=true"
        );
    }

    /// The exact word-timestamps resolution `ResizerVm::open` performs:
    /// `load_task_spec` else `spec_from_columns`.
    fn gate_word_timestamps(store: &SqliteStore, id: TranscriptionId) -> bool {
        let spec = match store.load_task_spec(&id).expect("load") {
            Some(spec) => Some(spec),
            None => store.spec_from_columns(&id).expect("columns"),
        };
        spec.map(|spec| spec.options.word_timestamps)
            .unwrap_or(false)
    }

    // -- save semantics -----------------------------------------------------

    fn seed_source(store: &SqliteStore) -> TranscriptionId {
        let id = TranscriptionId::new();
        store
            .create_task(&id, &spec_with(false, None), Source::FileImport)
            .expect("create");
        store
            .mark_completed(
                &id,
                &[
                    seg(0, 1000, "hello", "hola"),
                    seg(1000, 2000, "world", "mundo"),
                ],
            )
            .expect("mark completed");
        id
    }

    #[test]
    fn save_create_new_copies_then_completes() {
        let store = SqliteStore::open_in_memory().expect("open");
        let source = seed_source(&store);
        let out = vec![seg(0, 1500, "hello world", "")];

        let affected = save_segments(&store, source, true, &out).expect("save");
        assert_ne!(affected, source, "create-new mints a fresh id");

        // The new row is Completed with exactly the resized segments and no
        // translations (dropped on insert).
        let new_row = store.get_task(&affected).expect("get").expect("row");
        assert_eq!(new_row.status, Status::Completed);
        let new_segs = store.get_segment_rows(affected).expect("segments");
        assert_eq!(new_segs.len(), 1);
        assert_eq!(new_segs[0].start, Millis(0));
        assert_eq!(new_segs[0].end, Millis(1500));
        assert_eq!(new_segs[0].text, "hello world");
        assert_eq!(new_segs[0].translation, "");

        // The source row is untouched (still two segments, translations intact).
        let src_segs = store.get_segment_rows(source).expect("source segments");
        assert_eq!(src_segs.len(), 2);
        assert_eq!(src_segs[0].translation, "hola");
    }

    #[test]
    fn save_in_place_replaces() {
        let store = SqliteStore::open_in_memory().expect("open");
        let source = seed_source(&store);
        let out = vec![seg(0, 500, "hi", ""), seg(500, 900, "there", "")];

        let affected = save_segments(&store, source, false, &out).expect("save");
        assert_eq!(affected, source, "in-place returns the source id");

        // The source's segments are replaced and translations dropped.
        let segs = store.get_segment_rows(source).expect("segments");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].text, "hi");
        assert_eq!(segs[1].text, "there");
        for row in &segs {
            assert_eq!(row.translation, "");
        }
    }

    // -- regroup-string wiring (Buzz truth table) ---------------------------

    fn vm_for_algo(gap: bool, punct: bool, len: bool) -> ResizerVm {
        ResizerVm {
            source: TranscriptionId::new(),
            create_new: true,
            extend_amount: "0.2".to_string(),
            target_chars: 42,
            merge_by_gap: (gap, "0.2".to_string()),
            split_by_punct: (punct, DEFAULT_PUNCT.to_string()),
            split_by_len: (len, "42".to_string()),
            word_timestamps: true,
            running: false,
            title: String::new(),
            language: None,
            error: None,
            close_requested: false,
            focus_requested: false,
            pending_action: None,
            window_visible: true,
        }
    }

    #[test]
    fn regroup_string_built_matches_buzz_truth_table() {
        // All three checked (Buzz's default UI string).
        assert_eq!(
            vm_for_algo(true, true, true).build_algo(),
            format!("mg=0.2++42+1_sp={DEFAULT_PUNCT}_sl=42")
        );
        // Gap only.
        assert_eq!(vm_for_algo(true, false, false).build_algo(), "mg=0.2");
        // Gap + length (no punctuation): the `++42+1` suffix rides on `mg`.
        assert_eq!(
            vm_for_algo(true, false, true).build_algo(),
            "mg=0.2++42+1_sl=42"
        );
        // Gap + punctuation (no length): no `++` suffix.
        assert_eq!(
            vm_for_algo(true, true, false).build_algo(),
            format!("mg=0.2_sp={DEFAULT_PUNCT}")
        );
        // Punctuation only.
        assert_eq!(
            vm_for_algo(false, true, false).build_algo(),
            format!("sp={DEFAULT_PUNCT}")
        );
        // Length only.
        assert_eq!(vm_for_algo(false, false, true).build_algo(), "sl=42");
        // Nothing checked → empty (regroup no-op).
        assert_eq!(vm_for_algo(false, false, false).build_algo(), "");
    }

    // -- HUMM_MERGE_REGROUP_RULE override wiring -----------------------------

    #[test]
    fn override_algo_wins_when_set_even_empty() {
        // Delegated straight to core's `override_algo` (M4 final-review finding
        // 6): no GUI-local copy of the "set wins, even empty" rule survives.
        let built = "mg=0.2++42+1_sl=42".to_string();
        // No override → the built string passes through unchanged.
        assert_eq!(override_algo(built.clone(), None), built);
        // A set override replaces it entirely (Buzz `os.getenv(name, default)`).
        assert_eq!(override_algo(built.clone(), Some("da=1")), "da=1");
        // An *empty* override still wins (set-but-empty is "set").
        assert_eq!(override_algo(built, Some("")), "");
    }

    #[test]
    fn merge_algo_for_matches_core_override_algo_threaded_explicitly() {
        // Pins env-override equivalence between the GUI path (`merge_algo_for`,
        // which reads the real process env once) and core's value-threaded
        // `override_algo` fed the same built string and an explicit `None` —
        // i.e. "no override set" — standing in for the env read. Guarded by a
        // value read (never a mutation: humm-gui uses no `unsafe`, and
        // `std::env::set_var` is `unsafe` in edition 2024 — Global Constraint
        // 9's no-env-mutation-in-tests convention).
        if std::env::var_os("HUMM_MERGE_REGROUP_RULE").is_some() {
            return;
        }
        let vm = vm_for_algo(true, true, true);
        assert_eq!(merge_algo_for(&vm), override_algo(vm.build_algo(), None));
        // And unchanged from build_algo() itself when nothing overrides it.
        assert_eq!(merge_algo_for(&vm), vm.build_algo());
    }

    #[test]
    fn env_supplied_unsupported_op_surfaces_as_regroup_error() {
        // The value-threaded drive of the merge path (spec §2.6): an override
        // string with an unsupported op (`da`) flows through the same
        // resolution the app uses (core's `override_algo`) and then errors in
        // the regrouper — which the merge worker turns into the resizer's
        // error banner (start_merge's Err arm sets `vm.error`).
        let vm = vm_for_algo(true, true, true);
        let algo = override_algo(vm.build_algo(), Some("da=1"));
        assert_eq!(algo, "da=1", "the override string reaches the merge path");

        let segs = [seg(0, 1000, "hello world.", "")];
        let result = humm_core::regroup::regroup_segments(&segs, None, &algo);
        assert!(
            matches!(
                result,
                Err(humm_core::regroup::RegroupError::UnsupportedOp { .. })
            ),
            "an unsupported override op surfaces as a RegroupError (→ the resizer error banner)"
        );
    }

    /// The other half of the reachability chain: a `RegroupError` recorded on
    /// the VM (start_merge's Err arm sets `vm.error`) renders as the inline
    /// merge-error banner — the "banner, not toast" surfacing a deferred
    /// viewport requires.
    #[test]
    fn error_banner_renders_the_recorded_merge_error() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        // This test never calls `set_locale` itself, but it builds `vm.error`
        // through `crate::i18n::tr` and then queries the banner by its
        // English wording, so it can still observe another concurrently
        // running test's locale switch mid-flight without this guard --
        // `lock_locale_for_test`'s own doc comment now says so explicitly.
        let _guard = crate::i18n::lock_locale_for_test();
        let mut vm = vm_for_algo(true, true, true);
        vm.error = Some(format!(
            "{} {}",
            tr("resizer-merge-error"),
            "unsupported regroup op \"da\""
        ));
        let state = (
            vm,
            Mutex::new(Settings::default()),
            PathBuf::from("/unused"),
        );

        let mut harness = Harness::new_ui_state(
            |ui: &mut egui::Ui, state: &mut (ResizerVm, Mutex<Settings>, PathBuf)| {
                super::ui(ui, &mut state.0, &state.1, &state.2);
            },
            state,
        );
        harness.run();

        harness.get_by_label_contains("The merge could not run");
        harness.get_by_label_contains("unsupported regroup op");
    }
}
