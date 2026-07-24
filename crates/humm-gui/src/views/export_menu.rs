//! The transcript viewer's Export toolbar menu (F-084, design spec §2.4):
//! `{TXT, SRT, VTT} × {Text, Translation}`, 6 entries labeled Buzz's
//! `"SRT - Text"` shape, Translation entries visible only once the
//! transcript has translations. Selecting an entry resolves a default save
//! path (the shared filename-template engine, per-transcript last export
//! folder else the input file's directory), opens the native save dialog
//! through the `FileDialogs` seam (`crate::import`), and writes the
//! in-memory rows through the **exact same** `humm_core::export::write_format`
//! the queue's own export uses (`humm_core::pipeline::run_file_job`) — the
//! whole point of F-084 is that the viewer and the queue share one writer,
//! never a second implementation.
//!
//! Ported from Buzz's `ExportTranscriptionMenu`
//! (`buzz/widgets/transcription_viewer/export_transcription_menu.py`) and
//! `Transcription.get_output_file_path`
//! (`buzz/db/entity/transcription.py:44-66`, itself built on
//! `buzz/transcriber/transcriber.py:228-265`'s template engine).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use humm_core::domain::{Millis, OutputFormat, Segment, TranscriptionId, TranscriptionOptions};
use humm_core::export::{TextKey, output_file_path, write_format};
use humm_core::viewer::text_view::paragraph_split_from_env;
use humm_store::SqliteStore;

use crate::i18n::{tr, tr_args};
use crate::viewer::{ViewerDeps, ViewerVm};

/// How long the "written to `path`" flash stays visible — the same duration
/// as the editor's "edited · saved" indicator
/// (`views::viewer::SAVED_FLASH_DURATION`); kept as a separate constant
/// since the two flashes are independent state on [`ViewerVm`].
pub const EXPORT_FLASH_DURATION: Duration = Duration::from_millis(1500);

/// The three formats, in Buzz's own `OutputFormat` enum order
/// (`transcriber.py:212-215`) — `{TXT,SRT,VTT} x {Text,Translation}` builds
/// its 6 entries by crossing this list with [`TextKey::Text`] then
/// [`TextKey::Translation`], matching `ExportTranscriptionMenu.__init__`'s
/// `text_actions`-then-`translation_actions` construction order.
const FORMATS: [OutputFormat; 3] = [OutputFormat::Txt, OutputFormat::Srt, OutputFormat::Vtt];

/// Renders the toolbar's Export menu button and handles a selected entry.
/// The Translation half of the menu is entirely absent (not just disabled)
/// when `!vm.has_translations` — Buzz `translation_action.setVisible(...)`.
pub fn toolbar_menu(ui: &mut egui::Ui, vm: &mut ViewerVm, deps: &ViewerDeps) {
    ui.menu_button(tr("viewer-export"), |ui| {
        for format in FORMATS {
            if ui.button(entry_label(format, TextKey::Text)).clicked() {
                export_entry(vm, deps, format, TextKey::Text);
                ui.close();
            }
        }
        if vm.has_translations {
            for format in FORMATS {
                if ui
                    .button(entry_label(format, TextKey::Translation))
                    .clicked()
                {
                    export_entry(vm, deps, format, TextKey::Translation);
                    ui.close();
                }
            }
        }
    });
}

/// One menu entry's label: `"SRT - Text"` (Buzz
/// `f"{output_format.value.upper()} - {label}"`,
/// `export_transcription_menu.py:35,39`). `viewer-export-text`/
/// `viewer-export-translation` are their own i18n keys, not a reuse of
/// `viewer-mode-text`/`viewer-mode-translation` — the two happen to share
/// English text (Buzz itself resolves both from the same `_("Text")`/
/// `_("Translation")` gettext calls), but the export menu and the View menu
/// are independent UI surfaces that may diverge once real localization
/// lands.
fn entry_label(format: OutputFormat, key: TextKey) -> String {
    let kind = match key {
        TextKey::Text => tr("viewer-export-text"),
        TextKey::Translation => tr("viewer-export-translation"),
    };
    format!("{} - {kind}", format.as_str().to_uppercase())
}

/// Renders the "written to `path`" flash next to the toolbar's autosave
/// indicator — the export-menu analogue of `views::viewer::
/// show_autosave_indicator`. Call from the same toolbar row; a no-op once
/// [`EXPORT_FLASH_DURATION`] has elapsed since the write (self-clearing, via
/// `request_repaint_after`).
pub fn show_export_flash(ui: &mut egui::Ui, vm: &mut ViewerVm) {
    let Some((written_at, path)) = &vm.export_flash else {
        return;
    };
    let elapsed = written_at.elapsed();
    if elapsed >= EXPORT_FLASH_DURATION {
        vm.export_flash = None;
        return;
    }
    let path_text = path.display().to_string();
    let message = tr_args("export-written", &[("path", &path_text)]);
    ui.separator();
    ui.label(egui::RichText::new(message).weak());
    ui.ctx()
        .request_repaint_after(EXPORT_FLASH_DURATION - elapsed);
}

/// The inline export-error warning banner — the exact same shape as `views::
/// viewer::show_unsaved_banner` (ADR-018's "banner, not toast" precedent: the
/// app's toast stack is unreachable from a deferred-viewport render closure):
/// a fixed, i18n'd message, not the raw path/`io::Error` text — that detail
/// is logged (`log::warn!`) at the failure site in [`export_entry`] instead,
/// the same "banner shows a stable sentence, the log carries the diagnostic"
/// split `show_unsaved_banner`'s fixed `viewer-unsaved-warning` string uses.
pub fn show_export_error_banner(ui: &mut egui::Ui, vm: &ViewerVm) {
    if !vm.export_error {
        return;
    }
    let color = ui.visuals().warn_fg_color;
    ui.horizontal(|ui| {
        ui.colored_label(color, tr("viewer-export-failed"));
    });
}

/// Handles one selected export entry (Buzz `on_menu_triggered`,
/// `export_transcription_menu.py:61-95`): resolves the default save path,
/// opens the save dialog (cancel = silent no-op, Buzz `if output_file_path
/// == "": return`), writes the in-memory rows via the shared writer, and
/// records the outcome (success → flash + persisted folder; failure → error
/// banner).
///
/// A transcript with no input file (defensive — the open gate only ever
/// opens a completed/skipped row, which always has one) has nothing sensible
/// to resolve a default path or directory from, so this is a no-op, logged
/// rather than surfaced (mirrors `ViewerVm::ensure_player_started`'s own
/// no-file guard).
fn export_entry(vm: &mut ViewerVm, deps: &ViewerDeps, format: OutputFormat, key: TextKey) {
    let Some(file) = vm.file.clone() else {
        log::warn!(
            "viewer export: transcript {} has no input file, cannot export",
            vm.id
        );
        return;
    };

    let now = chrono::Local::now().naive_local();
    let default_path = default_export_path(&deps.store, vm.id, &file, format, now);

    let Some(chosen) = deps.dialogs.save_file(
        &tr("viewer-export-dialog-title"),
        &default_path,
        &tr("viewer-export-filter"),
        format.extension(),
    ) else {
        return;
    };

    let segments = vm.segments();
    // Read fresh per export, exactly like Buzz's `write_output`
    // (`file_transcriber.py:203`, the function `on_menu_triggered` calls)
    // reads `BUZZ_PARAGRAPH_SPLIT_TIME` fresh per export — see
    // `write_export_file`'s doc comment for why this must not be a
    // hardcoded constant.
    let paragraph_split = paragraph_split_from_env();
    match write_export_file(&chosen, format, key, &segments, paragraph_split) {
        Ok(()) => {
            if let Some(parent) = chosen.parent()
                && let Err(err) = deps.store.set_last_export_folder(vm.id, parent)
            {
                log::warn!(
                    "viewer export: could not persist last export folder for {}: {err}",
                    vm.id
                );
            }
            vm.export_error = false;
            vm.export_flash = Some((Instant::now(), chosen));
        }
        Err(err) => {
            log::warn!("viewer export: could not write {}: {err}", chosen.display());
            vm.export_flash = None;
            vm.export_error = true;
        }
    }
}

/// Creates (or overwrites) `path` and writes `segments` through the shared
/// `write_format` — the exact function `humm_core::pipeline::run_file_job`
/// calls for queue exports.
///
/// `paragraph_split` is a caller-supplied value, not a hardcoded constant:
/// Buzz's `write_output` (`file_transcriber.py:203`, the very function
/// `ExportTranscriptionMenu.on_menu_triggered` calls) reads
/// `BUZZ_PARAGRAPH_SPLIT_TIME` **fresh on every export**, and humm's own
/// Text-mode display (`ViewerVm::refresh_text_body`) reads
/// `text_view::paragraph_split_from_env()` the same way — [`export_entry`]
/// (this function's only caller) passes that very value through, so a
/// transcript's on-screen Text-mode paragraphing and its TXT export always
/// agree, `HUMM_PARAGRAPH_SPLIT_TIME` override or not. SRT/VTT ignore the
/// parameter entirely ([`write_format`]'s own dispatch), so this is a no-op
/// for those two formats regardless of value.
fn write_export_file(
    path: &Path,
    format: OutputFormat,
    key: TextKey,
    segments: &[Segment],
    paragraph_split: Millis,
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    write_format(format, segments, key, paragraph_split, &mut file)
}

/// Resolves task `id`'s persisted [`TranscriptionOptions`] for filename
/// templating: the full [`humm_core::domain::TaskSpec`] when one exists
/// (every humm-created row), else the tolerant raw-column fallback (an
/// imported/foreign row with `options_json` `NULL`), else
/// [`TranscriptionOptions::default`] — the same three-tier fallback
/// `viewmodel::model_and_task_text` uses for the task table's Model/Task
/// columns.
fn export_options(store: &SqliteStore, id: TranscriptionId) -> TranscriptionOptions {
    match store.load_task_spec(&id) {
        Ok(Some(spec)) => spec.options,
        Ok(None) => match store.spec_from_columns(&id) {
            Ok(Some(spec)) => spec.options,
            Ok(None) => TranscriptionOptions::default(),
            Err(err) => {
                log::debug!("viewer export: spec_from_columns failed for {id}: {err}");
                TranscriptionOptions::default()
            }
        },
        Err(err) => {
            log::debug!("viewer export: load_task_spec failed for {id}: {err}");
            TranscriptionOptions::default()
        }
    }
}

/// Resolves the default export *directory*: `id`'s last export folder
/// (UX_SPEC J-6) when one is set and still exists on disk, else `None` —
/// which [`output_file_path`] itself resolves to the input file's own
/// directory (spec §2.4 step 1's fallback), so this function only ever
/// *overrides* that built-in behavior, never duplicates it.
fn default_export_directory(store: &SqliteStore, id: TranscriptionId) -> Option<PathBuf> {
    match store.last_export_folder(id) {
        Ok(Some(dir)) if dir.is_dir() => Some(dir),
        Ok(_) => None,
        Err(err) => {
            log::debug!("viewer export: last_export_folder failed for {id}: {err}");
            None
        }
    }
}

/// Builds the default save path for one export entry: [`export_options`]'s
/// task/language/model feed the shared template engine
/// (`output_file_path`, `template = None` → `DEFAULT_EXPORT_TEMPLATE`, the
/// same resolution the queue's own export (`pipeline::run_file_job`) uses —
/// this milestone has no settings-level template override to thread through
/// either path), with [`default_export_directory`] substituted for the
/// input file's own directory when a valid last-export folder exists.
fn default_export_path(
    store: &SqliteStore,
    id: TranscriptionId,
    file: &Path,
    format: OutputFormat,
    now: chrono::NaiveDateTime,
) -> PathBuf {
    let options = export_options(store, id);
    let directory = default_export_directory(store, id);
    output_file_path(
        file,
        options.task,
        options.language,
        options.model.size,
        format,
        directory.as_deref(),
        None,
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::boundary::TaskStore;
    use humm_core::domain::{
        Backend, Input, ModelSize, ModelSpec, OutputSpec, Segment, Source, TaskKind, TaskSpec,
    };
    use humm_core::export::DEFAULT_PARAGRAPH_SPLIT;
    use humm_core::language::Language;
    use pretty_assertions::assert_eq;

    fn spec_for(file: &str) -> TaskSpec {
        TaskSpec {
            input: Input::Path(PathBuf::from(file)),
            options: TranscriptionOptions {
                language: Language::parse("fr"),
                task: TaskKind::Translate,
                model: ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::Medium,
                    hf_repo: None,
                },
                word_timestamps: false,
                initial_prompt: String::new(),
                temperature: vec![0.0],
                llm: None,
            },
            output: OutputSpec::default(),
        }
    }

    fn seed(store: &SqliteStore, file: &str) -> TranscriptionId {
        let id = TranscriptionId::new();
        store
            .create_task(&id, &spec_for(file), Source::FileImport)
            .expect("create task");
        store.mark_completed(&id, &[]).expect("mark completed");
        id
    }

    fn fixed_now() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 20)
            .unwrap()
            .and_hms_opt(14, 3, 5)
            .unwrap()
    }

    #[test]
    fn default_export_path_uses_last_folder_then_input_dir() {
        let store = SqliteStore::open_in_memory().expect("store");
        let input_dir = tempfile::tempdir().expect("input tempdir");
        let file = input_dir.path().join("interview.wav");
        let id = seed(&store, file.to_str().expect("utf8 path"));

        // No last folder set yet: falls back to the input file's own directory.
        let path = default_export_path(&store, id, &file, OutputFormat::Srt, fixed_now());
        assert_eq!(path.parent(), Some(input_dir.path()));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("interview (translated on 20-Jul-2026 14-03-05).srt")
        );

        // A last export folder that still exists on disk takes priority.
        let export_dir = tempfile::tempdir().expect("export tempdir");
        store
            .set_last_export_folder(id, export_dir.path())
            .expect("set last export folder");
        let path = default_export_path(&store, id, &file, OutputFormat::Srt, fixed_now());
        assert_eq!(path.parent(), Some(export_dir.path()));

        // A last export folder that no longer exists falls back to the input
        // file's directory again, rather than building a path into nowhere.
        drop(export_dir); // deletes the tempdir
        let path = default_export_path(&store, id, &file, OutputFormat::Srt, fixed_now());
        assert_eq!(path.parent(), Some(input_dir.path()));
    }

    fn seg(start: i64, end: i64, text: &str, translation: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.into(),
            translation: translation.into(),
        }
    }

    /// A fixture with translations, multi-line/untrimmed text, and a
    /// ≥2000 ms gap (paragraph split) — the acceptance-pinning shape the
    /// task brief calls for.
    fn fixture_segments() -> Vec<Segment> {
        vec![
            seg(0, 1000, "Hello world", "Salut monde"),
            seg(1200, 2000, "second segment", "deuxième segment"),
            seg(
                5000,
                6000,
                "third\nmultiline segment",
                "troisième\nmultiligne",
            ),
            seg(6100, 7000, "  padded text  ", "  texte rembourré  "),
        ]
    }

    /// The acceptance-pinning test: every `{TXT,SRT,VTT} x {Text,Translation}`
    /// combo this module writes is byte-identical to calling `write_format`
    /// directly — proving the viewer's export path is the shared M1 writer,
    /// not a second implementation.
    #[test]
    fn export_writes_via_shared_writer() {
        let segments = fixture_segments();
        for format in FORMATS {
            for key in [TextKey::Text, TextKey::Translation] {
                let dir = tempfile::tempdir().expect("tempdir");
                let path = dir.path().join(format!("out.{}", format.extension()));

                write_export_file(&path, format, key, &segments, DEFAULT_PARAGRAPH_SPLIT)
                    .expect("write via export_menu");
                let via_menu = std::fs::read(&path).expect("read written file");

                let mut expected = Vec::new();
                write_format(
                    format,
                    &segments,
                    key,
                    DEFAULT_PARAGRAPH_SPLIT,
                    &mut expected,
                )
                .expect("write via direct write_format call");

                assert_eq!(
                    via_menu, expected,
                    "{format:?}/{key:?}: viewer export bytes must match write_format directly"
                );
            }
        }
    }

    #[test]
    fn translation_export_uses_translation_key() {
        let segments = fixture_segments();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.srt");

        write_export_file(
            &path,
            OutputFormat::Srt,
            TextKey::Translation,
            &segments,
            DEFAULT_PARAGRAPH_SPLIT,
        )
        .expect("write");
        let written = std::fs::read_to_string(&path).expect("read");

        assert!(
            written.contains("Salut monde"),
            "translation-key export must contain translated text"
        );
        assert!(
            !written.contains("Hello world"),
            "translation-key export must not contain the recognized text"
        );
    }

    /// Review fix round 1 (Important): `write_export_file` must not hardcode
    /// [`DEFAULT_PARAGRAPH_SPLIT`] — Buzz's `write_output`
    /// (`file_transcriber.py:203`, the exact function
    /// `ExportTranscriptionMenu.on_menu_triggered` calls) reads
    /// `BUZZ_PARAGRAPH_SPLIT_TIME` fresh per export, and humm's own Text-mode
    /// display reads `text_view::paragraph_split_from_env()`; a hardcoded
    /// constant here would let a `HUMM_PARAGRAPH_SPLIT_TIME` override make the
    /// on-screen text and the exported TXT disagree. `export_entry` now
    /// resolves the split via `paragraph_split_from_env()` and threads it
    /// through as a plain argument (proven below); this test proves the
    /// *value itself* actually reaches the writer for both the menu's write
    /// path and a direct `write_format` call, rather than mutating the real
    /// process `HUMM_PARAGRAPH_SPLIT_TIME` env var — this crate's lib tests
    /// run in parallel and several *other* tests
    /// (`viewer::tests::mode_switch_reruns_search` and friends) call
    /// `paragraph_split_from_env()` too, so a real `std::env::set_var` here
    /// would be a genuine flaky-test hazard. Matches the codebase's own
    /// established convention for env-configurable behavior (`humm_core::
    /// paths`'s "Global Constraint 9" doc comment; `text_view`'s own tests
    /// exercise `paragraph_split_from_env_value`, never the real env, for the
    /// identical reason).
    #[test]
    fn export_txt_honors_paragraph_split_env() {
        // A 500 ms gap: below the default 2000 ms split (no paragraph break)
        // but at the boundary of a 500 ms override (a break) — proves the
        // supplied value, not the default, decides the outcome.
        let segments = vec![seg(0, 500, "first", ""), seg(1000, 1500, "second", "")];
        let overridden = Millis(500);

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.txt");
        write_export_file(
            &path,
            OutputFormat::Txt,
            TextKey::Text,
            &segments,
            overridden,
        )
        .expect("write via export_menu");
        let via_menu = std::fs::read(&path).expect("read written file");

        let mut expected = Vec::new();
        write_format(
            OutputFormat::Txt,
            &segments,
            TextKey::Text,
            overridden,
            &mut expected,
        )
        .expect("write via direct write_format call");
        assert_eq!(
            via_menu, expected,
            "the menu path and a direct write_format call must agree for a non-default split"
        );

        let mut at_default = Vec::new();
        write_format(
            OutputFormat::Txt,
            &segments,
            TextKey::Text,
            DEFAULT_PARAGRAPH_SPLIT,
            &mut at_default,
        )
        .expect("write via direct write_format call at the default split");
        assert_ne!(
            via_menu, at_default,
            "a non-default paragraph split must actually change the TXT export, \
             proving write_export_file does not silently ignore its argument"
        );
    }

    #[test]
    fn entry_label_matches_buzz_format_upper_dash_label_shape() {
        assert_eq!(entry_label(OutputFormat::Srt, TextKey::Text), "SRT - Text");
        assert_eq!(entry_label(OutputFormat::Txt, TextKey::Text), "TXT - Text");
        assert_eq!(entry_label(OutputFormat::Vtt, TextKey::Text), "VTT - Text");
        assert_eq!(
            entry_label(OutputFormat::Srt, TextKey::Translation),
            "SRT - Translation"
        );
    }
}
