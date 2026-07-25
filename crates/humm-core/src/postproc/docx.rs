//! F-113 `export_docx` (design spec §7): after a transcription completes,
//! writes the transcript to a `.docx` file next to the source media (or in
//! a configured folder). Ported from `ExportDocxPlugin.on_complete`
//! (`export_docx/plugin.py:91-124`, whole file read for context) — the OOXML
//! writer itself lives in [`crate::export::docx::write_docx`].
//!
//! [`run`] is built and unit-tested standalone in this task (M7 Task 6) —
//! nothing calls it from the queue yet. The `on_complete` hook chain that
//! actually invokes it after a completed task (gated on
//! [`crate::settings::ExportDocxSettings::enabled`], the same way
//! [`crate::postproc::skip::check`] is gated on `cfg.skip.enabled` by its
//! *caller* rather than internally, and exactly like
//! [`crate::postproc::summary::run`]) lands in a later M7 task (design spec
//! §2.1). Fixed hook order (design spec §2.1, ledger #L2): `transcript_resizer
//! → ai_summary → export_docx` — this hook re-reads segments from the store
//! rather than taking them as a parameter (unlike `summary::run`) precisely
//! so it observes the resizer's replacement, if any, when Task 7 wires the
//! chain.

use std::path::{Path, PathBuf};

use crate::boundary::TaskStore;
use crate::domain::{Segment, TranscriptionId};
use crate::export::docx::write_docx;

use super::PostprocConfig;
use super::skip::stem_for;

/// Outcome of one [`run`] call.
///
/// A *skip* (no segments in the store) is not an error: both fields stay
/// `None` and [`Self::error`] is `None`. Only an actual failure — the store
/// read itself failing, or an I/O/zip error while creating the destination
/// directory or writing the file — populates [`Self::error`], and in that
/// case [`Self::written`] stays `None` (Buzz's `_write_docx` either fully
/// succeeds or raises, `plugin.py:113-124`; there is no partial-write case
/// to represent).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocxResult {
    /// The `.docx` file's path, if the write succeeded.
    pub written: Option<PathBuf>,
    /// A non-fatal error recorded along the way, if any.
    pub error: Option<String>,
}

/// Runs the export_docx hook for one completed task (F-113, design spec
/// §7), porting `ExportDocxPlugin.on_complete` (`export_docx/plugin.py:91-124`):
///
/// 1. **Re-read segments from the store** (`:92-94`): the same
///    [`TaskStore::segments_for`] the F-115 database check uses — deliberate,
///    per the module doc comment, so this hook sees the auto-resizer's
///    replacement segments when Task 7 wires the fixed hook order. A store
///    read failure is treated as a hook failure (recorded in
///    [`DocxResult::error`] and logged): unlike F-115's database check
///    (an optional, best-effort lookup where a store error is tolerated as
///    a miss), a failure here means the export that was supposed to happen
///    genuinely didn't, and that must be visible.
/// 2. **Empty segments → skip** (`:95-97`): Buzz's exact log line
///    ("Export to DOCX skipped: no segments"), not an error.
/// 3. **Stem** (`:99-107`): [`super::skip::stem_for`] on `task_input` — see
///    that function's doc comment for why this reaches the same practical
///    stem as Buzz's `original_file_path`-preferring logic.
/// 4. **Folder** (`:109-111`): `cfg.docx.output_folder`, trimmed, used only
///    if non-empty after trimming (Buzz's `(… or "").strip()` idiom, the
///    same trim-before-decide rule [`crate::postproc::summary::run`]'s
///    `write_summary_file` already applies) — else `task_input`'s parent
///    directory.
/// 5. **Write** (`:113-122`): `create_dir_all` the destination, then
///    `{stem}.docx` via [`write_docx`], passing `cfg.docx.include_timestamps`
///    and `cfg.paragraph_split` (design spec §7 ledger #L7: Buzz hardcodes
///    `PARAGRAPH_SPLIT_TIME = 2000`; with the paragraph-split env var unset
///    `cfg.paragraph_split` *is* `2000`, so output is Buzz-identical). Any
///    I/O or zip error is logged and recorded in [`DocxResult::error`]
///    (Buzz's blanket `except Exception`, `:123-124`), non-fatally.
pub fn run(
    cfg: &PostprocConfig,
    transcription_id: &TranscriptionId,
    task_input: &Path,
    store: &dyn TaskStore,
) -> DocxResult {
    let segments = match store.segments_for(transcription_id) {
        Ok(segments) => segments,
        Err(e) => {
            let message = format!("Export to DOCX failed: could not read segments: {e}");
            log::error!("{message}");
            return DocxResult {
                written: None,
                error: Some(message),
            };
        }
    };

    if segments.is_empty() {
        log::info!("Export to DOCX skipped: no segments");
        return DocxResult::default();
    }

    let stem = stem_for(task_input);
    let stem = if stem.is_empty() {
        "transcript".to_string()
    } else {
        stem
    };

    match write_docx_file(cfg, task_input, &stem, &segments) {
        Ok(path) => {
            log::info!("Export to DOCX written to {}", path.display());
            DocxResult {
                written: Some(path),
                error: None,
            }
        }
        Err(e) => {
            let message = format!("Export to DOCX failed: {e}");
            log::error!("{message}");
            DocxResult {
                written: None,
                error: Some(message),
            }
        }
    }
}

/// Resolves the destination folder (Buzz's `(context.config.get("output_folder")
/// or "").strip()` else `os.path.dirname(source_path) or os.getcwd()`,
/// `plugin.py:109-111`), creates it, and writes `{stem}.docx` into it.
/// Returns the written file's path.
fn write_docx_file(
    cfg: &PostprocConfig,
    task_input: &Path,
    stem: &str,
    segments: &[Segment],
) -> std::io::Result<PathBuf> {
    let trimmed_output_folder = cfg
        .docx
        .output_folder
        .as_ref()
        .map(|folder| folder.to_string_lossy().trim().to_string())
        .filter(|folder| !folder.is_empty());

    let folder = match trimmed_output_folder {
        Some(folder) => PathBuf::from(folder),
        None => task_input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    };

    std::fs::create_dir_all(&folder)?;
    let out_path = folder.join(format!("{stem}.docx"));
    let file = std::fs::File::create(&out_path)?;
    write_docx(
        file,
        stem,
        segments,
        cfg.docx.include_timestamps,
        cfg.paragraph_split,
    )?;
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Millis;
    use crate::error::StoreError;
    use crate::settings::{
        AiSummarySettings, AutoResizeSettings, ExportDocxSettings, SkipSettings,
    };
    use tempfile::TempDir;

    // -- a minimal `TaskStore` double: only `segments_for` is ever exercised
    //    by `run`. --------------------------------------------------------

    #[derive(Default)]
    struct FakeStore {
        segments: Vec<Segment>,
        fail: bool,
    }

    impl TaskStore for FakeStore {
        fn create_task(
            &self,
            _id: &TranscriptionId,
            _spec: &crate::domain::TaskSpec,
            _source: crate::domain::Source,
        ) -> Result<(), StoreError> {
            unreachable!("run() never creates tasks")
        }

        fn requeue_task(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never requeues tasks")
        }

        fn mark_started(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks started")
        }

        fn update_progress(&self, _id: &TranscriptionId, _fraction: f64) -> Result<(), StoreError> {
            unreachable!("run() never updates progress")
        }

        fn update_file(&self, _id: &TranscriptionId, _path: &Path) -> Result<(), StoreError> {
            unreachable!("run() never updates the file path")
        }

        fn mark_completed(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks completed")
        }

        fn mark_failed(&self, _id: &TranscriptionId, _error: &str) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks failed")
        }

        fn mark_canceled(&self, _id: &TranscriptionId) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks canceled")
        }

        fn mark_skipped(
            &self,
            _id: &TranscriptionId,
            _segments: &[Segment],
        ) -> Result<(), StoreError> {
            unreachable!("run() never marks tasks skipped")
        }

        fn find_completed_by_file_name(
            &self,
            _name: &str,
        ) -> Result<Option<TranscriptionId>, StoreError> {
            unreachable!("run() never looks up completed tasks by name")
        }

        fn segments_for(&self, _id: &TranscriptionId) -> Result<Vec<Segment>, StoreError> {
            if self.fail {
                return Err(StoreError::Query {
                    detail: "boom".to_string(),
                });
            }
            Ok(self.segments.clone())
        }

        fn set_task_notes(&self, _id: &TranscriptionId, _notes: &str) -> Result<(), StoreError> {
            unreachable!("run() never sets task notes")
        }
    }

    fn seg(text: &str) -> Segment {
        Segment {
            start: Millis(0),
            end: Millis(1000),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    fn base_cfg() -> PostprocConfig {
        PostprocConfig {
            skip: SkipSettings::default(),
            summary: AiSummarySettings::default(),
            summary_api_key: None,
            docx: ExportDocxSettings::default(),
            resize: AutoResizeSettings::default(),
            paragraph_split: Millis(2000),
        }
    }

    // -- empty segments: skip -------------------------------------------------

    #[test]
    fn empty_segments_skips_with_no_error() {
        let cfg = base_cfg();
        let store = FakeStore::default();

        let result = run(
            &cfg,
            &TranscriptionId::new(),
            Path::new("/tmp/clip.mp3"),
            &store,
        );

        assert_eq!(result, DocxResult::default());
    }

    // -- store read failure: recorded as an error -----------------------------

    #[test]
    fn store_read_failure_is_recorded_as_error() {
        let cfg = base_cfg();
        let store = FakeStore {
            fail: true,
            ..FakeStore::default()
        };

        let result = run(
            &cfg,
            &TranscriptionId::new(),
            Path::new("/tmp/clip.mp3"),
            &store,
        );

        assert_eq!(result.written, None);
        assert!(result.error.is_some());
    }

    // -- folder placement ------------------------------------------------------

    #[test]
    fn default_folder_writes_next_to_input() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let cfg = base_cfg();
        let store = FakeStore {
            segments: vec![seg("hello")],
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), &input, &store);

        assert_eq!(result.error, None);
        assert_eq!(result.written, Some(tmp.path().join("clip.docx")));
        assert!(tmp.path().join("clip.docx").is_file());
    }

    #[test]
    fn configured_folder_is_used_and_created() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let out_dir = tmp.path().join("out").join("nested");
        let mut cfg = base_cfg();
        cfg.docx.output_folder = Some(out_dir.clone());
        let store = FakeStore {
            segments: vec![seg("hello")],
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), &input, &store);

        assert_eq!(result.error, None);
        assert_eq!(result.written, Some(out_dir.join("clip.docx")));
        assert!(out_dir.is_dir(), "output_folder should be create_dir_all'd");
    }

    #[test]
    fn whitespace_only_output_folder_falls_back_to_next_to_input() {
        // Buzz's `(context.config.get("output_folder") or "").strip()`: a
        // whitespace-only configured folder is not used verbatim -- it
        // trims down to "" and falls back exactly like an unset folder.
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let mut cfg = base_cfg();
        cfg.docx.output_folder = Some(PathBuf::from("   "));
        let store = FakeStore {
            segments: vec![seg("hello")],
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), &input, &store);

        assert_eq!(result.error, None);
        assert_eq!(result.written, Some(tmp.path().join("clip.docx")));
    }

    #[test]
    fn stem_strips_trailing_speech_suffix() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("interview_speech.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let cfg = base_cfg();
        let store = FakeStore {
            segments: vec![seg("hello")],
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), &input, &store);

        assert_eq!(result.written, Some(tmp.path().join("interview.docx")));
    }

    #[test]
    fn empty_stem_falls_back_to_transcript() {
        // Buzz: `os.path.splitext(os.path.basename(source_path))[0] or
        // "transcript"` (`plugin.py:107`) -- a path with no usable file
        // name (here, the empty path) falls back to the literal stem
        // "transcript". A configured output folder is used so this never
        // touches the real process working directory.
        let tmp = TempDir::new().expect("tempdir");
        let mut cfg = base_cfg();
        cfg.docx.output_folder = Some(tmp.path().to_path_buf());
        let store = FakeStore {
            segments: vec![seg("hello")],
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), Path::new(""), &store);

        assert_eq!(result.written, Some(tmp.path().join("transcript.docx")));
    }

    // -- io error: unwritable target (file-as-directory trick) ----------------

    #[test]
    fn io_error_creating_folder_is_recorded() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        // A regular file where a directory is expected: `create_dir_all`
        // on a path through it fails.
        let blocker = tmp.path().join("blocker.txt");
        std::fs::write(&blocker, b"not a directory").expect("write blocker");

        let mut cfg = base_cfg();
        cfg.docx.output_folder = Some(blocker.join("nested"));
        let store = FakeStore {
            segments: vec![seg("hello")],
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), &input, &store);

        assert_eq!(result.written, None);
        assert!(result.error.is_some());
    }

    // -- include_timestamps / paragraph_split are threaded through -----------

    #[test]
    fn include_timestamps_setting_is_honored() {
        let tmp = TempDir::new().expect("tempdir");
        let input = tmp.path().join("clip.mp3");
        std::fs::write(&input, b"fake media").expect("write input");
        let mut cfg = base_cfg();
        cfg.docx.include_timestamps = true;
        let store = FakeStore {
            segments: vec![seg("hello")],
            ..FakeStore::default()
        };

        let result = run(&cfg, &TranscriptionId::new(), &input, &store);

        let path = result.written.expect("should write");
        let bytes = std::fs::read(&path).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut doc = archive.by_name("word/document.xml").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut doc, &mut xml).unwrap();
        // The stamp's arrow renders escaped (`>` -> `&gt;`, matching
        // Buzz's `escape()` call on the stamp text too -- see
        // `export::docx`'s `stamp_format_pinned_against_to_timestamp`).
        assert!(
            xml.contains("--&gt;"),
            "timestamps mode should stamp segments: {xml}"
        );
    }
}
