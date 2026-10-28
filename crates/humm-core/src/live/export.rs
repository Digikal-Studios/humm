//! Live-recording export writers, filename templating, retry policy, and the
//! upload-URL client (F-064, spec §2.4). Ported from Buzz's
//! `RecordingTranscriberWidget`
//! (`buzz/widgets/recording_transcriber_widget.py`):
//!
//! - [`live_export_file_name`] -- `setup_for_export`'s template substitution
//!   (`:508-518`); the default template itself comes from
//!   `Settings.get_default_export_file_template` (`settings.py:170-174`),
//!   which is byte-identical to [`crate::export::DEFAULT_EXPORT_TEMPLATE`]
//!   (reused rather than redeclared).
//! - [`ExportSink`] -- the rest of `setup_for_export` (`:484-531`: folder/
//!   file-type/max-entries/template setup, `.translated` sibling path,
//!   truncate-existing-at-start, folder-must-exist-else-disable) plus the
//!   per-mode write dispatch scattered across `on_next_transcription`
//!   (`:979-1008`, including append-above's inline CSV prepend logic) and
//!   `process_transcription_merge`'s export tail (`:955-961`).
//! - [`write_to_export_file`]/[`write_txt_export`]/[`write_csv_export`] --
//!   the three static writer methods (`:786-863`), including the retry
//!   policy (5 attempts, 200 ms delay, on a lock-class error) shared by
//!   `write_to_export_file`/`read_export_file`/`write_csv_export`'s own
//!   inline retry loop (`:824-836`) -- unified here onto one retry helper
//!   (see "Divergences" below).
//! - [`UploadClient`] -- the two `requests.post` call sites: the
//!   session-start clear (`start_recording`, `:628-637`) and the
//!   per-utterance transcript upload (`on_next_transcription`, `:1021-1035`).
//! - [`effective_upload_url`] -- humm's env-wins-over-setting resolution (✚,
//!   spec §2.4); Buzz only ever reads `BUZZ_UPLOAD_URL` from the environment
//!   (`:97`) with no persisted-setting fallback at all.
//!
//! ## Divergences from Buzz (documented, not bugs)
//!
//! - **Unified retry log wording.** Buzz's `write_to_export_file`/
//!   `read_export_file` and `write_csv_export` each hand-roll their own
//!   near-identical 5×200ms retry loop with slightly different log message
//!   text ("Export write failed..." vs "CSV export write failed...").
//!   Nothing in the spec or this task's tests pins the exact string, so
//!   this port collapses all three onto one [`write_to_export_file`]/
//!   internal-read helper pair with one wording -- behavior (attempt count,
//!   delay, which error class retries vs. gives up immediately) is
//!   unchanged.
//! - **CSV read asymmetry is preserved, not fixed.** Buzz's append path
//!   (`write_csv_export`, `:806-817`) reads existing columns with a
//!   **single unretried attempt**, silently treating any I/O error
//!   (including a lock-class one) as "no existing columns" -- unlike every
//!   other read/write in this module, which retries and warns. Buzz's
//!   append-above CSV prepend path (`:994-1001`) instead calls
//!   `read_export_file` (retrying, plain UTF-8, no BOM handling) and its
//!   final write (`:1008`) uses `write_to_export_file` (plain UTF-8, no
//!   BOM) rather than `write_csv_export`'s own utf-8-sig writer -- so an
//!   append-mode CSV export file carries a BOM and a prepend-mode one does
//!   not. Both asymmetries are real Buzz behavior (not humm inventions) and
//!   are ported exactly; see [`write_csv_export`]'s doc comment.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDateTime;
use serde_json::json;

use crate::domain::TaskKind;
use crate::live::RecordingMode;

/// The `%d-%b-%Y %H-%M-%S` datetime format shared by [`live_export_file_name`]'s
/// `{{ date_time }}` substitution and (Task 11, spec §4) the live session
/// row's own name (`"Live recording <%d-%b-%Y %H-%M-%S>"`) — one literal so
/// the table row and any exported file's timestamp can never drift apart.
pub const LIVE_DATETIME_FORMAT: &str = "%d-%b-%Y %H-%M-%S";

/// Retry attempt count shared by every export read/write (Buzz's `retries: int = 5`
/// default, `:787,824,866`).
const RETRY_ATTEMPTS: u32 = 5;

/// Delay between retry attempts (Buzz's `delay: float = 0.2` default /
/// `time.sleep(0.2)`, `:787,824,866`).
const RETRY_DELAY: Duration = Duration::from_millis(200);

/// Builds the live-recording export filename, **without** an extension --
/// the caller appends `.txt`/`.csv` (see [`ExportSink::setup`], which knows
/// the configured [`ExportFileType`]).
///
/// Ported from `setup_for_export`'s template substitution (`:508-518`).
/// `template` falls back to [`crate::export::DEFAULT_EXPORT_TEMPLATE`]
/// (Buzz: `Settings.get_default_export_file_template`, `settings.py:170-174`
/// -- byte-identical to the live default, so the one constant is reused for
/// both the file-export and live-export paths). Substitutions:
///
/// - `{{ input_file_name }}` -> the literal `"live recording"` (there is no
///   input file for a live session).
/// - `{{ task }}` -> [`TaskKind::as_str`] (`"transcribe"`/`"translate"`).
///   The default template's literal `"d"` immediately follows this
///   placeholder (`"({{ task }}d on ...)"`), so substituting `"transcribe"`
///   naturally reads `"transcribed on ..."` -- that is plain string
///   substitution, not special-cased here.
/// - `{{ language }}` -> `language`, or `""` when `None`.
/// - `{{ model_type }}` -> the literal `"whisper"`. Buzz renders
///   `self.transcription_options.model.model_type.value` (its multi-backend
///   `ModelType` wire string); humm has one live backend (whisper-rs,
///   ADR-008), so this is a fixed literal rather than an enum lookup --
///   documented consolidation, spec §2.4.
/// - `{{ model_size }}` -> `model_size` verbatim (e.g. `"small"`).
/// - `{{ date_time }}` -> `now` formatted `%d-%b-%Y %H-%M-%S` (e.g.
///   `24-Jul-2026 14-03-59`), identical to
///   [`crate::export::output_file_path`]'s format string.
pub fn live_export_file_name(
    template: Option<&str>,
    task: TaskKind,
    language: Option<&str>,
    model_size: &str,
    now: NaiveDateTime,
) -> String {
    let template = template.unwrap_or(crate::export::DEFAULT_EXPORT_TEMPLATE);
    let date_time = now.format(LIVE_DATETIME_FORMAT).to_string();

    template
        .replace("{{ input_file_name }}", "live recording")
        .replace("{{ task }}", task.as_str())
        .replace("{{ language }}", language.unwrap_or(""))
        .replace("{{ model_type }}", "whisper")
        .replace("{{ model_size }}", model_size)
        .replace("{{ date_time }}", &date_time)
}

/// Which export file format [`ExportSink`] writes -- Buzz's
/// `export_file_type` setting (`"txt"`/`"csv"`, default `"txt"`), typed
/// rather than a raw string at this layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFileType {
    /// Plain text, Buzz's `write_txt_export` (`:838-863`).
    Txt,
    /// Single-row CSV, one utterance per column, Buzz's `write_csv_export`
    /// (`:803-836`) plus the inline append-above prepend logic
    /// (`:993-1008`).
    Csv,
}

impl ExportFileType {
    /// The file extension, no leading dot (matches
    /// [`crate::domain::OutputFormat::extension`]'s convention). Buzz:
    /// `ext = ".csv" if self.export_file_type == "csv" else ".txt"` (`:508`).
    pub fn extension(self) -> &'static str {
        match self {
            ExportFileType::Txt => "txt",
            ExportFileType::Csv => "csv",
        }
    }
}

/// Whether a new utterance is inserted after existing export content or
/// before it -- shared by the TXT and CSV writers.
///
/// Derived from [`RecordingMode`]: [`RecordingMode::AppendBelow`] maps to
/// [`InsertMode::Append`], [`RecordingMode::AppendAbove`] to
/// [`InsertMode::Prepend`] (`on_next_transcription`'s two branches,
/// `:972-1010`). [`RecordingMode::AppendAndCorrect`] never reaches either
/// writer through [`ExportSink::on_utterance`] -- its export write is
/// always the whole-file rewrite in [`ExportSink::on_merged`]
/// (`process_transcription_merge`, `:955-961`), so the mapping below is
/// never actually exercised for that variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertMode {
    /// New content goes after what's already there.
    Append,
    /// New content goes before what's already there.
    Prepend,
}

impl InsertMode {
    fn from_recording_mode(mode: RecordingMode) -> InsertMode {
        match mode {
            RecordingMode::AppendAbove => InsertMode::Prepend,
            RecordingMode::AppendBelow | RecordingMode::AppendAndCorrect => InsertMode::Append,
        }
    }
}

/// Injectable delay so retry-loop tests never actually sleep.
///
/// The session thread uses [`RealSleeper`]; tests use a fake that records
/// call counts/durations instead of blocking.
pub trait Sleeper: Send + Sync {
    /// Blocks for (approximately) `duration`, or -- in a test double --
    /// records that it was asked to.
    fn sleep(&self, duration: Duration);
}

/// The production [`Sleeper`]: a thin wrapper over [`std::thread::sleep`].
#[derive(Debug, Default, Clone, Copy)]
pub struct RealSleeper;

impl Sleeper for RealSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Whether a write creates-or-appends or creates-or-truncates -- Rust
/// analogue of Python's `open(path, mode)` `"a"`/`"w"` modes used throughout
/// Buzz's export writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Python `"a"`: create if absent, append if present.
    Append,
    /// Python `"w"`: create if absent, truncate to empty if present.
    Truncate,
}

fn open_for_write(path: &Path, mode: WriteMode) -> io::Result<std::fs::File> {
    match mode {
        WriteMode::Append => OpenOptions::new().create(true).append(true).open(path),
        WriteMode::Truncate => OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path),
    }
}

/// Whether `err` is the lock class Buzz retries: Python `PermissionError`.
///
/// That is [`std::io::ErrorKind::PermissionDenied`] everywhere. On Windows it
/// is also `ERROR_SHARING_VIOLATION` (32) and `ERROR_LOCK_VIOLATION` (33)
/// when the standard library leaves them as a raw OS error instead of
/// `PermissionDenied`. Those numbers are unrelated errno values on Unix, so
/// they count only when `windows` is true. `windows` is a parameter so the
/// Windows codes are tested on Linux.
pub(crate) fn is_export_lock_error(
    kind: std::io::ErrorKind,
    raw_os_error: Option<i32>,
    windows: bool,
) -> bool {
    if kind == std::io::ErrorKind::PermissionDenied {
        return true;
    }
    windows && matches!(raw_os_error, Some(32 | 33))
}

/// A lock-class error on a file is retried. A directory is not: Windows
/// reports opening one as `PermissionDenied` (`ERROR_ACCESS_DENIED`), which
/// is the same kind as a locked file, but it will not become readable by
/// waiting.
fn export_io_is_lock(path: &Path, err: &std::io::Error) -> bool {
    if path.is_dir() {
        return false;
    }
    is_export_lock_error(err.kind(), err.raw_os_error(), cfg!(windows))
}

/// Writes `content` to `path`, retrying up to [`RETRY_ATTEMPTS`] times at
/// [`RETRY_DELAY`] when the failure is a lock-class error (see
/// [`is_export_lock_error`] -- Rust's analogue of Windows' `PermissionError`,
/// which is what Buzz's retry loop specifically catches). Any other I/O
/// error logs a warning and gives up immediately without retrying;
/// exhausting every attempt on a lock error also logs a warning. Never
/// returns an error -- a failed export write is a best-effort side channel,
/// not a hard failure (Buzz: both call sites return `None` either way).
///
/// Ported from `write_to_export_file` (`:786-801`); see the module doc's
/// "Divergences" note for why this same helper also backs the CSV writer's
/// final write, rather than duplicating Buzz's separate inline loop
/// (`:824-836`).
pub fn write_to_export_file(path: &Path, content: &str, mode: WriteMode, sleeper: &dyn Sleeper) {
    for attempt in 0..RETRY_ATTEMPTS {
        match open_for_write(path, mode).and_then(|mut f| f.write_all(content.as_bytes())) {
            Ok(()) => return,
            Err(e) if export_io_is_lock(path, &e) => {
                if attempt + 1 < RETRY_ATTEMPTS {
                    sleeper.sleep(RETRY_DELAY);
                } else {
                    log::warn!(
                        "Export write failed after {RETRY_ATTEMPTS} retries: {}",
                        path.display()
                    );
                }
            }
            Err(e) => {
                log::warn!("Export write failed: {e}");
                return;
            }
        }
    }
}

/// Reads `path` as UTF-8 text, with the same retry policy as
/// [`write_to_export_file`]. Any failure (including "not found") warns and
/// returns `""` -- the caller's own `os.path.isfile` guard (ported as a
/// `path.is_file()` check at each call site) means this is normally only
/// reached when the file is known to already exist, so a warning here means
/// a genuine read failure, not a routine "doesn't exist yet".
///
/// Ported from `read_export_file` (`:865-880`). Plain UTF-8 -- no BOM
/// handling, unlike the CSV-specific readers below.
fn read_export_file(path: &Path, sleeper: &dyn Sleeper) -> String {
    for attempt in 0..RETRY_ATTEMPTS {
        match std::fs::read_to_string(path) {
            Ok(content) => return content,
            Err(e) if export_io_is_lock(path, &e) => {
                if attempt + 1 < RETRY_ATTEMPTS {
                    sleeper.sleep(RETRY_DELAY);
                } else {
                    log::warn!(
                        "Export read failed after {RETRY_ATTEMPTS} retries: {}",
                        path.display()
                    );
                }
            }
            Err(e) => {
                log::warn!("Export read failed: {e}");
                return String::new();
            }
        }
    }
    String::new()
}

/// Writes a TXT export utterance under `mode`, with `max_entries` trimming
/// and `separator`-joined entries.
///
/// Ported from `write_txt_export`'s `"a"`/`"prepend"` branches (`:838-863`).
/// The static method's `else` branch (an arbitrary passthrough `mode`
/// string used for neither call site) has no analogue here -- Buzz's own
/// append-and-correct path never calls `write_txt_export` at all, it
/// rewrites the whole file directly via `write_to_export_file(..., mode="w")`
/// (`:961`); see [`ExportSink::on_merged`].
///
/// **Append** (`:841-850`): appends `text + separator`; if `max_entries > 0`
/// *and* the file exists, re-reads the whole file, splits on `separator`
/// (dropping empty parts), and -- **only when** more than `max_entries`
/// parts remain -- rewrites the file with the last `max_entries` parts
/// rejoined (`separator`-joined, with a trailing `separator`). Below the
/// cap, nothing further happens: the plain append from the first step is
/// left as-is, un-renormalized.
///
/// **Prepend** (`:851-861`): reads existing content (`""` if the file is
/// absent), computes `text + separator + existing`. If `max_entries > 0`,
/// the result is **always** rebuilt -- split on `separator` (dropping empty
/// parts), truncated to the first `max_entries` parts if there are more,
/// then `separator`-joined with a trailing `separator` -- regardless of
/// whether truncation actually fired. This is a genuine asymmetry with the
/// append branch (which only rebuilds when the cap is exceeded), preserved
/// exactly because that is what Buzz's indentation encodes. The whole
/// result (rebuilt or not) is written with [`WriteMode::Truncate`].
pub fn write_txt_export(
    path: &Path,
    text: &str,
    mode: InsertMode,
    max_entries: usize,
    separator: &str,
    sleeper: &dyn Sleeper,
) {
    match mode {
        InsertMode::Append => {
            write_to_export_file(
                path,
                &format!("{text}{separator}"),
                WriteMode::Append,
                sleeper,
            );
            if max_entries > 0 && path.is_file() {
                let raw = read_export_file(path, sleeper);
                let mut parts: Vec<&str> = raw.split(separator).filter(|p| !p.is_empty()).collect();
                if parts.len() > max_entries {
                    let start = parts.len() - max_entries;
                    parts = parts.split_off(start);
                    write_to_export_file(
                        path,
                        &format!("{}{separator}", parts.join(separator)),
                        WriteMode::Truncate,
                        sleeper,
                    );
                }
            }
        }
        InsertMode::Prepend => {
            let existing = if path.is_file() {
                read_export_file(path, sleeper)
            } else {
                String::new()
            };
            let combined = format!("{text}{separator}{existing}");
            let new_content = if max_entries > 0 {
                let mut parts: Vec<&str> = combined
                    .split(separator)
                    .filter(|p| !p.is_empty())
                    .collect();
                if parts.len() > max_entries {
                    parts.truncate(max_entries);
                }
                format!("{}{separator}", parts.join(separator))
            } else {
                combined
            };
            write_to_export_file(path, &new_content, WriteMode::Truncate, sleeper);
        }
    }
}

/// Strips a leading UTF-8 BOM (`EF BB BF`) if present, then lossily decodes
/// the remaining bytes as UTF-8 -- Python's `encoding="utf-8-sig"` read
/// behavior. Lossy (not strict) decoding is a deliberate, minor defensive
/// divergence for malformed-byte input: Buzz's real `UnicodeDecodeError` in
/// that case is an uncaught `ValueError` (not `OSError`), so it would crash
/// rather than degrade -- not a documented, exercised behavior worth
/// reproducing.
fn strip_utf8_bom_to_string(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Parses `raw`'s first CSV record into its fields, or `[]` if `raw` is
/// blank (Python's `if raw.strip():` gate) or contains no records at all.
fn parse_csv_row(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(raw.as_bytes());
    reader
        .records()
        .next()
        .and_then(Result::ok)
        .map(|record| record.iter().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Renders `columns` as one CSV record, `\r\n`-terminated (RFC 4180 /
/// Python `csv.writer`'s default dialect -- the `csv` crate's own default
/// terminator is plain `\n`, so [`csv::Terminator::CRLF`] is set explicitly
/// to match).
fn build_csv_row(columns: &[String]) -> String {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(Vec::new());
    writer
        .write_record(columns)
        .expect("writing a CSV record to an in-memory Vec<u8> buffer cannot fail");
    writer
        .flush()
        .expect("flushing a Vec<u8>-backed CSV writer cannot fail");
    let bytes = writer
        .into_inner()
        .expect("into_inner on a flushed Vec<u8>-backed writer cannot fail");
    String::from_utf8(bytes).expect("csv writer emits valid UTF-8 given valid UTF-8 field input")
}

/// Reads the existing single CSV row from `path` for the **append** path
/// only: a single, unretried attempt (unlike every other read in this
/// module), utf-8-sig tolerant (BOM stripped if present). Any failure --
/// missing file, permission error, anything -- is swallowed silently and
/// treated as "no existing columns", with no retry and no log line. This
/// mirrors Buzz's `write_csv_export` (`:806-817`) exactly; see the module
/// doc's "Divergences" note for why this differs from the prepend path.
fn read_csv_row_no_retry(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    parse_csv_row(&strip_utf8_bom_to_string(&bytes))
}

/// Writes a CSV export utterance under `mode`, with `max_entries` trimming.
///
/// **Append** (Buzz's `write_csv_export` static method, `:803-836`): reads
/// the existing row via [`read_csv_row_no_retry`] (utf-8-sig tolerant,
/// unretried), pushes `text` as a new column, keeps only the **last**
/// `max_entries` columns when `max_entries > 0`, then rewrites the whole
/// file **with a leading utf-8-sig BOM** via the shared retrying writer
/// ([`write_to_export_file`]).
///
/// **Prepend** (the inline append-above CSV branch, `:993-1008` -- there is
/// no separate Buzz static method for this direction): reads the existing
/// row via the general retrying [`read_export_file`] (plain UTF-8, **no**
/// BOM stripping) when the file exists, inserts `text` as the new first
/// column, keeps only the **first** `max_entries` columns when
/// `max_entries > 0`, then rewrites the whole file with **no BOM**. This
/// asymmetry (BOM on append, none on prepend; unretried-silent read on
/// append, retrying-and-warning read on prepend) is real Buzz behavior, not
/// a humm simplification -- see the module doc's "Divergences" note.
pub fn write_csv_export(
    path: &Path,
    text: &str,
    mode: InsertMode,
    max_entries: usize,
    sleeper: &dyn Sleeper,
) {
    match mode {
        InsertMode::Append => {
            let mut columns = read_csv_row_no_retry(path);
            columns.push(text.to_string());
            if max_entries > 0 {
                let start = columns.len().saturating_sub(max_entries);
                columns = columns.split_off(start);
            }
            let body = build_csv_row(&columns);
            let content = format!("\u{FEFF}{body}");
            write_to_export_file(path, &content, WriteMode::Truncate, sleeper);
        }
        InsertMode::Prepend => {
            let mut columns = if path.is_file() {
                parse_csv_row(&read_export_file(path, sleeper))
            } else {
                Vec::new()
            };
            columns.insert(0, text.to_string());
            if max_entries > 0 {
                columns.truncate(max_entries);
            }
            let body = build_csv_row(&columns);
            write_to_export_file(path, &body, WriteMode::Truncate, sleeper);
        }
    }
}

/// One recording session's live-export target files and write dispatch,
/// configured once at recording start.
///
/// Ported from `setup_for_export` (`:484-531`) plus the per-mode dispatch in
/// `on_next_transcription` (`:979-1013`).
pub struct ExportSink {
    enabled: bool,
    file_type: ExportFileType,
    max_entries: usize,
    separator: String,
    insert_mode: InsertMode,
    transcript_path: PathBuf,
    translated_path: PathBuf,
    sleeper: Arc<dyn Sleeper>,
}

impl ExportSink {
    /// Configures a sink for real use ([`RealSleeper`]). See
    /// [`Self::setup_with_sleeper`] for the full behavior and parameter
    /// meanings; tests use that method directly to inject a fake sleeper.
    #[allow(clippy::too_many_arguments)]
    pub fn setup(
        folder: &Path,
        file_type: ExportFileType,
        max_entries: usize,
        separator: &str,
        mode: RecordingMode,
        template: Option<&str>,
        task: TaskKind,
        language: Option<&str>,
        model_size: &str,
        now: NaiveDateTime,
    ) -> ExportSink {
        ExportSink::setup_with_sleeper(
            folder,
            file_type,
            max_entries,
            separator,
            mode,
            template,
            task,
            language,
            model_size,
            now,
            Arc::new(RealSleeper),
        )
    }

    /// Full `setup_for_export` port (`:484-531`):
    ///
    /// 1. Builds the transcript file name via [`live_export_file_name`] plus
    ///    `.{extension}` (`:508-518`).
    /// 2. Computes the `.translated` sibling by replacing the extension in
    ///    the **whole path string** with `.translated.{extension}` (`:520,
    ///    526`) -- Python's `str.replace` (and Rust's `str::replace`, used
    ///    here) replaces every occurrence, not just a trailing one, so a
    ///    folder path that happens to also contain the extension substring
    ///    would be affected too. This is Buzz's real, unguarded string
    ///    replace, ported as-is.
    /// 3. `enabled` is `folder.is_dir()` (`:522-523`) -- computed
    ///    independently of the path computation, matching Buzz's flag-only
    ///    disable (the paths are still built and both files are still
    ///    truncated-if-present even when disabled, since a nonexistent
    ///    folder simply means neither path is ever `is_file()` true).
    /// 4. Both the transcript and translated paths are truncated
    ///    (`WriteMode::Truncate` with empty content) if they already exist
    ///    (`:528-531`) -- the translated file is truncated even though M6
    ///    owns writing translations there.
    #[allow(clippy::too_many_arguments)]
    pub fn setup_with_sleeper(
        folder: &Path,
        file_type: ExportFileType,
        max_entries: usize,
        separator: &str,
        mode: RecordingMode,
        template: Option<&str>,
        task: TaskKind,
        language: Option<&str>,
        model_size: &str,
        now: NaiveDateTime,
        sleeper: Arc<dyn Sleeper>,
    ) -> ExportSink {
        let name = live_export_file_name(template, task, language, model_size, now);
        let ext_with_dot = format!(".{}", file_type.extension());
        let file_name = format!("{name}{ext_with_dot}");
        let transcript_path = folder.join(&file_name);

        let translated_ext = format!(".translated{ext_with_dot}");
        let transcript_path_string = transcript_path.to_string_lossy().into_owned();
        let translated_path =
            PathBuf::from(transcript_path_string.replace(&ext_with_dot, &translated_ext));

        let enabled = folder.is_dir();

        for path in [&transcript_path, &translated_path] {
            if path.is_file() {
                write_to_export_file(path, "", WriteMode::Truncate, sleeper.as_ref());
            }
        }

        ExportSink {
            enabled,
            file_type,
            max_entries,
            separator: separator.to_string(),
            insert_mode: InsertMode::from_recording_mode(mode),
            transcript_path,
            translated_path,
            sleeper,
        }
    }

    /// Whether export is active for this session (`false` when the
    /// configured folder wasn't a directory at setup time).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The transcript export file's path.
    pub fn transcript_path(&self) -> &Path {
        &self.transcript_path
    }

    /// The `.translated` sibling's path -- the append-below/append-above /
    /// append-and-correct translation exports land here (F-063,
    /// [`Self::on_translation_utterance`]/[`Self::on_translation_merged`]),
    /// and it is also truncated-at-setup for parity (see [`Self::setup`]).
    pub fn translated_path(&self) -> &Path {
        &self.translated_path
    }

    /// The append-below/append-above per-utterance write (`write_txt_export`
    /// / `write_csv_export` dispatch), parameterized by target `path` and
    /// `insert_mode` so the transcript side ([`Self::on_utterance`], writing
    /// [`Self::transcript_path`]) and the translation side
    /// ([`Self::on_translation_utterance`], writing [`Self::translated_path`])
    /// share one byte-for-byte-identical implementation. No-op when
    /// [`Self::enabled`] is `false`.
    fn write_utterance_to(&self, path: &Path, text: &str, insert_mode: InsertMode) {
        if !self.enabled {
            return;
        }
        match self.file_type {
            ExportFileType::Txt => write_txt_export(
                path,
                text,
                insert_mode,
                self.max_entries,
                &self.separator,
                self.sleeper.as_ref(),
            ),
            ExportFileType::Csv => write_csv_export(
                path,
                text,
                insert_mode,
                self.max_entries,
                self.sleeper.as_ref(),
            ),
        }
    }

    /// The append-and-correct whole-file rewrite
    /// (`process_transcription_merge`'s export tail, `:955-961`),
    /// parameterized by target `path` so the transcript side
    /// ([`Self::on_merged`]) and the translation side
    /// ([`Self::on_translation_merged`]) share one implementation. No-op when
    /// [`Self::enabled`] is `false`.
    ///
    /// TXT: rewrites the whole file with `merged_text`, no separator suffix
    /// (`write_to_export_file(export_file, merged_texts, mode="w")`, `:961`).
    /// CSV: truncates the file, then writes it as a single column containing
    /// `merged_text` with `max_entries` forced to `0`
    /// (`write_to_export_file(..., mode="w")` then `write_csv_export(...,
    /// 0)`, `:958-959`).
    fn write_merged_to(&self, path: &Path, merged_text: &str) {
        if !self.enabled {
            return;
        }
        match self.file_type {
            ExportFileType::Txt => write_to_export_file(
                path,
                merged_text,
                WriteMode::Truncate,
                self.sleeper.as_ref(),
            ),
            ExportFileType::Csv => {
                write_to_export_file(path, "", WriteMode::Truncate, self.sleeper.as_ref());
                write_csv_export(
                    path,
                    merged_text,
                    InsertMode::Append,
                    0,
                    self.sleeper.as_ref(),
                );
            }
        }
    }

    /// Applies one accepted **transcript** utterance under append-below/
    /// append-above (`on_next_transcription`'s `:979-1010`), writing
    /// [`Self::transcript_path`]. No-op when [`Self::enabled`] is `false`.
    /// Not the right call for append-and-correct mode -- use
    /// [`Self::on_merged`] there instead (the caller, `live::session`, knows
    /// which mode is active and dispatches accordingly).
    pub fn on_utterance(&self, text: &str) {
        self.write_utterance_to(&self.transcript_path, text, self.insert_mode);
    }

    /// Applies the append-and-correct merged **transcript** text as a
    /// whole-file rewrite (`process_transcription_merge`'s export tail,
    /// `:955-961`), writing [`Self::transcript_path`]. See
    /// [`Self::write_merged_to`] for the TXT/CSV behavior; no-op when
    /// [`Self::enabled`] is `false`.
    pub fn on_merged(&self, merged_text: &str) {
        self.write_merged_to(&self.transcript_path, merged_text);
    }

    /// Applies one accepted **translation** utterance under append-below/
    /// append-above (`on_next_translation`'s `:1048-1078`), writing
    /// [`Self::translated_path`] -- the exact TXT append/prepend + CSV
    /// append/prepend rules as [`Self::on_utterance`], only the target file
    /// differs (F-063, spec §7.4). The write is the **raw** translation text
    /// (Buzz exports raw `text`, not `strip_newlines(text)`, `:1050,1052,
    /// 1070,1078`); newline normalization is a display-panel concern, not an
    /// export one. `mode` selects append-vs-prepend
    /// ([`InsertMode::from_recording_mode`]) -- it is the session's
    /// [`RecordingMode`], the same one this sink was set up with, threaded in
    /// by the [`crate::live::translation::TranslationApplier`] rather than
    /// re-read from `self` so the applier owns the mode decision. No-op when
    /// [`Self::enabled`] is `false`; not the right call for append-and-correct
    /// (use [`Self::on_translation_merged`]).
    pub fn on_translation_utterance(&self, text: &str, mode: RecordingMode) {
        self.write_utterance_to(
            &self.translated_path,
            text,
            InsertMode::from_recording_mode(mode),
        );
    }

    /// Applies the append-and-correct merged **translation** text as a
    /// whole-file rewrite (`process_transcription_merge` on
    /// `translation_export_file`, `:955-961` via `:1081`), writing
    /// [`Self::translated_path`]. Same TXT/CSV behavior as [`Self::on_merged`]
    /// (see [`Self::write_merged_to`]); no-op when [`Self::enabled`] is
    /// `false`.
    pub fn on_translation_merged(&self, merged: &str) {
        self.write_merged_to(&self.translated_path, merged);
    }
}

/// Blocking POST client for Buzz's live-upload sidecar protocol
/// (`start_recording`'s clear POST, `:628-637`; `on_next_transcription`'s
/// per-utterance POST, `:1021-1035`). Every failure is logged and swallowed
/// -- never surfaced as an `Err` -- exactly like Buzz's own
/// `try/except Exception as e: logging.error(...)` around both calls, since
/// the session thread must keep transcribing regardless of upload health.
pub struct UploadClient {
    url: String,
}

impl UploadClient {
    /// Request timeout (Buzz's `timeout=15`, `:634,1032`).
    const TIMEOUT: Duration = Duration::from_secs(15);

    /// Targets every POST at `url`.
    pub fn new(url: impl Into<String>) -> UploadClient {
        UploadClient { url: url.into() }
    }

    /// `POST {"kind": "clear", "text": ""}` -- sent once, at recording start
    /// (`start_recording`, `:628-637`).
    pub fn clear(&self) {
        self.post(json!({"kind": "clear", "text": ""}), "Clear upload failed");
    }

    /// `POST {"kind": "transcript", "text": text, "mode": "replace"|"append"}`
    /// (`on_next_transcription`, `:1021-1035`). `replace` corresponds to
    /// Buzz's `mode="replace" if ... == APPEND_AND_CORRECT else "append"`
    /// (`:1029`) -- the caller passes `replace = true` iff the session is in
    /// append-and-correct mode.
    pub fn transcript(&self, text: &str, replace: bool) {
        let mode = if replace { "replace" } else { "append" };
        self.post(
            json!({"kind": "transcript", "text": text, "mode": mode}),
            "Transcript upload failed",
        );
    }

    /// POSTs `body` as JSON (setting `Content-Type: application/json`,
    /// Buzz's explicit header, `:633,1031`) with [`Self::TIMEOUT`]; any
    /// failure -- including failing to build the HTTP client itself, which
    /// `reqwest::blocking::Client::builder().build()` can theoretically
    /// return (matching [`crate::telemetry::PostHogTelemetry::capture`]'s
    /// same non-panicking pattern rather than `.expect`) -- logs
    /// `"{failure_context}: {detail}"` via `log::error!` and returns.
    fn post(&self, body: serde_json::Value, failure_context: &str) {
        let Ok(client) = reqwest::blocking::Client::builder()
            .timeout(UploadClient::TIMEOUT)
            .build()
        else {
            log::error!("{failure_context}: could not build HTTP client");
            return;
        };
        if let Err(e) = client.post(&self.url).json(&body).send() {
            log::error!("{failure_context}: {e}");
        }
    }
}

/// Resolves the live-upload target URL: `env_value` (the caller's own
/// `std::env::var("HUMM_UPLOAD_URL").ok()`, following [`crate::env`]'s
/// convention of taking an already-read `Option<&str>` rather than touching
/// `std::env` here) wins over the persisted `setting` value when non-empty;
/// an empty string counts as unset in either slot.
///
/// Mirrors Buzz's own `if self.upload_url:` truthiness check (`:97,628,
/// 1022`) -- for a Python `str`, only the exact empty string is falsy, so
/// this checks emptiness only, without trimming whitespace.
///
/// humm's env-wins-over-setting precedence is a documented ✚ deviation
/// (spec §2.4): Buzz only ever reads `BUZZ_UPLOAD_URL` from the environment
/// (`:97`) and has no persisted-setting equivalent at all.
pub fn effective_upload_url(env_value: Option<&str>, setting: Option<&str>) -> Option<String> {
    [env_value, setting]
        .into_iter()
        .flatten()
        .find(|v| !v.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Default)]
    struct CountingSleeper {
        calls: AtomicU32,
    }

    impl CountingSleeper {
        fn calls(&self) -> u32 {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Sleeper for CountingSleeper {
        fn sleep(&self, _duration: Duration) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn fixed_now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 24)
            .unwrap()
            .and_hms_opt(14, 3, 59)
            .unwrap()
    }

    // -- live_export_file_name: template substitution matrix -----------------

    #[test]
    fn default_template_substitutes_task_language_model_and_date() {
        let name =
            live_export_file_name(None, TaskKind::Transcribe, Some("en"), "small", fixed_now());
        assert_eq!(name, "live recording (transcribed on 24-Jul-2026 14-03-59)");
    }

    #[test]
    fn translate_task_reads_translated_on() {
        let name = live_export_file_name(None, TaskKind::Translate, None, "base", fixed_now());
        assert_eq!(name, "live recording (translated on 24-Jul-2026 14-03-59)");
    }

    #[test]
    fn missing_language_substitutes_empty_string() {
        let name = live_export_file_name(
            Some("{{ language }}|{{ model_size }}"),
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
        );
        assert_eq!(name, "|small");
    }

    #[test]
    fn model_type_always_renders_whisper() {
        let name = live_export_file_name(
            Some("{{ model_type }}"),
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
        );
        assert_eq!(name, "whisper");
    }

    #[test]
    fn custom_template_overrides_default() {
        let name = live_export_file_name(
            Some("{{ input_file_name }}-{{ task }}-{{ date_time }}"),
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
        );
        assert_eq!(name, "live recording-transcribe-24-Jul-2026 14-03-59");
    }

    #[test]
    fn input_file_name_is_always_live_recording_literal() {
        let name = live_export_file_name(
            Some("{{ input_file_name }}"),
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
        );
        assert_eq!(name, "live recording");
    }

    // -- TXT: append mode ------------------------------------------------------

    #[test]
    fn txt_append_no_cap_keeps_growing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        write_txt_export(&path, "one", InsertMode::Append, 0, "\n\n", &sleeper);
        write_txt_export(&path, "two", InsertMode::Append, 0, "\n\n", &sleeper);
        write_txt_export(&path, "three", InsertMode::Append, 0, "\n\n", &sleeper);

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "one\n\ntwo\n\nthree\n\n"
        );
    }

    #[test]
    fn txt_append_below_cap_leaves_content_un_renormalized() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        // max_entries=5 but only 2 entries so far: below the cap, append
        // mode never re-reads/rewrites.
        write_txt_export(&path, "one", InsertMode::Append, 5, "|", &sleeper);
        write_txt_export(&path, "two", InsertMode::Append, 5, "|", &sleeper);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one|two|");
    }

    #[test]
    fn txt_append_caps_to_last_n_when_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        write_txt_export(&path, "one", InsertMode::Append, 2, "|", &sleeper);
        write_txt_export(&path, "two", InsertMode::Append, 2, "|", &sleeper);
        write_txt_export(&path, "three", InsertMode::Append, 2, "|", &sleeper);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two|three|");
    }

    // -- TXT: prepend mode -----------------------------------------------------

    #[test]
    fn txt_prepend_no_cap_keeps_raw_concatenation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        write_txt_export(&path, "one", InsertMode::Prepend, 0, "\n\n", &sleeper);
        write_txt_export(&path, "two", InsertMode::Prepend, 0, "\n\n", &sleeper);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n\none\n\n");
    }

    #[test]
    fn txt_prepend_caps_to_first_n_when_exceeded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        write_txt_export(&path, "one", InsertMode::Prepend, 2, "|", &sleeper);
        write_txt_export(&path, "two", InsertMode::Prepend, 2, "|", &sleeper);
        write_txt_export(&path, "three", InsertMode::Prepend, 2, "|", &sleeper);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "three|two|");
    }

    #[test]
    fn txt_prepend_always_renormalizes_when_max_entries_positive_even_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        // Seed a file with a stray double separator (as if hand-edited) --
        // well under the cap of 5, but max_entries > 0 still forces a
        // filter-empty-then-rejoin rebuild per Buzz's exact indentation.
        std::fs::write(&path, "one||two|").unwrap();
        write_txt_export(&path, "zero", InsertMode::Prepend, 5, "|", &sleeper);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "zero|one|two|");
    }

    // -- TXT: append-and-correct whole-file rewrite -----------------------------

    #[test]
    fn txt_append_and_correct_whole_file_rewrite_no_separator_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        std::fs::write(&path, "stale content").unwrap();
        write_to_export_file(&path, "merged text", WriteMode::Truncate, &sleeper);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "merged text");
    }

    // -- CSV: BOM + quoting round trip ------------------------------------------

    #[test]
    fn csv_append_writes_leading_utf8_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        let sleeper = CountingSleeper::default();

        write_csv_export(&path, "hello", InsertMode::Append, 0, &sleeper);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF]);
    }

    #[test]
    fn csv_prepend_writes_no_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        let sleeper = CountingSleeper::default();

        write_csv_export(&path, "hello", InsertMode::Prepend, 0, &sleeper);

        let bytes = std::fs::read(&path).unwrap();
        assert_ne!(&bytes[..3.min(bytes.len())], [0xEF, 0xBB, 0xBF]);
    }

    #[test]
    fn csv_quoting_round_trips_comma_quote_and_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        let sleeper = CountingSleeper::default();

        let tricky = "hello, \"world\"\nnewline";
        write_csv_export(&path, tricky, InsertMode::Append, 0, &sleeper);

        let bytes = std::fs::read(&path).unwrap();
        let text = strip_utf8_bom_to_string(&bytes);
        let columns = parse_csv_row(&text);
        assert_eq!(columns, vec![tricky.to_string()]);
    }

    // -- CSV: append cap-last-N vs prepend cap-first-N --------------------------

    #[test]
    fn csv_append_caps_to_last_n_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        let sleeper = CountingSleeper::default();

        write_csv_export(&path, "a", InsertMode::Append, 2, &sleeper);
        write_csv_export(&path, "b", InsertMode::Append, 2, &sleeper);
        write_csv_export(&path, "c", InsertMode::Append, 2, &sleeper);

        let bytes = std::fs::read(&path).unwrap();
        let text = strip_utf8_bom_to_string(&bytes);
        assert_eq!(parse_csv_row(&text), vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn csv_prepend_caps_to_first_n_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        let sleeper = CountingSleeper::default();

        write_csv_export(&path, "a", InsertMode::Prepend, 2, &sleeper);
        write_csv_export(&path, "b", InsertMode::Prepend, 2, &sleeper);
        write_csv_export(&path, "c", InsertMode::Prepend, 2, &sleeper);

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(parse_csv_row(&text), vec!["c".to_string(), "b".to_string()]);
    }

    // -- retry policy ------------------------------------------------------------

    #[test]
    fn write_retry_gives_up_after_five_attempts_on_permission_denied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.txt");
        std::fs::write(&path, "").unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms).unwrap();

        let sleeper = CountingSleeper::default();
        write_to_export_file(&path, "x", WriteMode::Append, &sleeper);

        // 5 attempts total -> 4 sleeps between them, then give up.
        assert_eq!(sleeper.calls(), 4);
    }

    #[test]
    fn windows_sharing_violation_is_a_lock_error_only_on_windows() {
        use std::io::ErrorKind;
        assert!(is_export_lock_error(
            ErrorKind::PermissionDenied,
            None,
            false
        ));
        assert!(is_export_lock_error(ErrorKind::Other, Some(32), true));
        assert!(is_export_lock_error(ErrorKind::Other, Some(33), true));
        // errno 32 on Unix is not a sharing violation.
        assert!(!is_export_lock_error(ErrorKind::Other, Some(32), false));
        assert!(!is_export_lock_error(ErrorKind::NotFound, None, true));
        // Opening a directory is PermissionDenied on Windows. That kind is
        // a lock for a file and is not a lock for a directory.
        assert!(is_export_lock_error(
            ErrorKind::PermissionDenied,
            Some(5),
            true
        ));
    }

    #[test]
    #[cfg(unix)]
    fn read_retry_gives_up_after_five_attempts_on_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.txt");
        std::fs::write(&path, "content").unwrap();
        // `set_readonly` only clears the write bits (still readable), so
        // clear every permission bit directly to force a real read-side
        // `PermissionDenied`. Unix-only: Windows has no mode bits, and its
        // sharing-violation codes are covered by
        // `windows_sharing_violation_is_a_lock_error_only_on_windows`.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let sleeper = CountingSleeper::default();
        let result = read_export_file(&path, &sleeper);

        assert_eq!(result, "");
        // 5 attempts total -> 4 sleeps between them, then give up.
        assert_eq!(sleeper.calls(), 4);

        // Restore read permission so tempdir cleanup can remove it without
        // relying on the "unlink doesn't need target read/write perms"
        // Unix quirk.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn read_of_directory_path_fails_immediately_without_retry() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().join("a_directory");
        std::fs::create_dir(&dir_path).unwrap();

        let sleeper = CountingSleeper::default();
        let result = read_export_file(&dir_path, &sleeper);

        // Unix reports this as `IsADirectory`. Windows reports it as
        // `PermissionDenied`, the same kind as a locked file. A directory
        // is not a lock, so neither read nor write sleeps.
        assert_eq!(result, "");
        assert_eq!(sleeper.calls(), 0);

        let write_sleeper = CountingSleeper::default();
        write_to_export_file(&dir_path, "x", WriteMode::Append, &write_sleeper);
        assert_eq!(write_sleeper.calls(), 0);
    }

    // -- effective_upload_url ----------------------------------------------------

    #[test]
    fn effective_upload_url_env_wins_over_setting() {
        assert_eq!(
            effective_upload_url(Some("http://env"), Some("http://setting")),
            Some("http://env".to_string())
        );
    }

    #[test]
    fn effective_upload_url_falls_back_to_setting_when_env_absent() {
        assert_eq!(
            effective_upload_url(None, Some("http://setting")),
            Some("http://setting".to_string())
        );
    }

    #[test]
    fn effective_upload_url_empty_env_counts_as_unset() {
        assert_eq!(
            effective_upload_url(Some(""), Some("http://setting")),
            Some("http://setting".to_string())
        );
    }

    #[test]
    fn effective_upload_url_none_when_both_absent_or_empty() {
        assert_eq!(effective_upload_url(None, None), None);
        assert_eq!(effective_upload_url(Some(""), Some("")), None);
    }

    // -- ExportSink: setup/truncate/disable --------------------------------------

    #[test]
    fn setup_disables_when_folder_is_not_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        let sink = ExportSink::setup_with_sleeper(
            &missing,
            ExportFileType::Txt,
            0,
            "\n\n",
            RecordingMode::AppendBelow,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );

        assert!(!sink.enabled());
    }

    #[test]
    fn setup_enables_when_folder_exists_and_computes_translated_sibling() {
        let dir = tempfile::tempdir().unwrap();

        let sink = ExportSink::setup_with_sleeper(
            dir.path(),
            ExportFileType::Txt,
            0,
            "\n\n",
            RecordingMode::AppendBelow,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );

        assert!(sink.enabled());
        assert_eq!(sink.transcript_path().extension().unwrap(), "txt");
        assert!(
            sink.translated_path()
                .to_string_lossy()
                .ends_with(".translated.txt")
        );
    }

    #[test]
    fn setup_truncates_existing_transcript_and_translated_files() {
        let dir = tempfile::tempdir().unwrap();

        let sink = ExportSink::setup_with_sleeper(
            dir.path(),
            ExportFileType::Txt,
            0,
            "\n\n",
            RecordingMode::AppendBelow,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );
        let transcript_path = sink.transcript_path().to_path_buf();
        let translated_path = sink.translated_path().to_path_buf();
        std::fs::write(&transcript_path, "stale transcript").unwrap();
        std::fs::write(&translated_path, "stale translation").unwrap();

        // Re-running setup (a fresh session on the same folder/template
        // inputs, e.g. within the same second) must truncate both.
        let sleeper = Arc::new(CountingSleeper::default());
        let _ = ExportSink::setup_with_sleeper(
            dir.path(),
            ExportFileType::Txt,
            0,
            "\n\n",
            RecordingMode::AppendBelow,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            sleeper,
        );

        assert_eq!(std::fs::read_to_string(&transcript_path).unwrap(), "");
        assert_eq!(std::fs::read_to_string(&translated_path).unwrap(), "");
    }

    #[test]
    fn on_utterance_no_op_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        let sink = ExportSink::setup_with_sleeper(
            &missing,
            ExportFileType::Txt,
            0,
            "\n\n",
            RecordingMode::AppendBelow,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );

        sink.on_utterance("should not be written anywhere");
        assert!(!sink.transcript_path().exists());
    }

    #[test]
    fn on_utterance_dispatches_append_txt() {
        let dir = tempfile::tempdir().unwrap();

        let sink = ExportSink::setup_with_sleeper(
            dir.path(),
            ExportFileType::Txt,
            0,
            "\n\n",
            RecordingMode::AppendBelow,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );

        sink.on_utterance("hello");
        sink.on_utterance("world");

        assert_eq!(
            std::fs::read_to_string(sink.transcript_path()).unwrap(),
            "hello\n\nworld\n\n"
        );
    }

    #[test]
    fn on_utterance_dispatches_prepend_csv() {
        let dir = tempfile::tempdir().unwrap();

        let sink = ExportSink::setup_with_sleeper(
            dir.path(),
            ExportFileType::Csv,
            0,
            "\n\n",
            RecordingMode::AppendAbove,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );

        sink.on_utterance("a");
        sink.on_utterance("b");

        let text = std::fs::read_to_string(sink.transcript_path()).unwrap();
        assert_eq!(parse_csv_row(&text), vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn on_merged_rewrites_txt_whole_file() {
        let dir = tempfile::tempdir().unwrap();

        let sink = ExportSink::setup_with_sleeper(
            dir.path(),
            ExportFileType::Txt,
            0,
            "\n\n",
            RecordingMode::AppendAndCorrect,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );

        sink.on_merged("first merge");
        sink.on_merged("second, corrected merge");

        assert_eq!(
            std::fs::read_to_string(sink.transcript_path()).unwrap(),
            "second, corrected merge"
        );
    }

    #[test]
    fn on_merged_rewrites_csv_as_single_column() {
        let dir = tempfile::tempdir().unwrap();

        let sink = ExportSink::setup_with_sleeper(
            dir.path(),
            ExportFileType::Csv,
            0,
            "\n\n",
            RecordingMode::AppendAndCorrect,
            None,
            TaskKind::Transcribe,
            None,
            "small",
            fixed_now(),
            Arc::new(RealSleeper),
        );

        sink.on_merged("first merge");
        sink.on_merged("second merge");

        let bytes = std::fs::read(sink.transcript_path()).unwrap();
        assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF]);
        let text = strip_utf8_bom_to_string(&bytes);
        assert_eq!(parse_csv_row(&text), vec!["second merge".to_string()]);
    }

    // -- insta snapshots ----------------------------------------------------------

    #[test]
    fn txt_append_snapshot_three_utterances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let sleeper = CountingSleeper::default();

        write_txt_export(
            &path,
            "first utterance",
            InsertMode::Append,
            0,
            "\n\n",
            &sleeper,
        );
        write_txt_export(
            &path,
            "second utterance",
            InsertMode::Append,
            0,
            "\n\n",
            &sleeper,
        );
        write_txt_export(
            &path,
            "third utterance",
            InsertMode::Append,
            0,
            "\n\n",
            &sleeper,
        );

        insta::assert_snapshot!(std::fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn csv_snapshot_three_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.csv");
        let sleeper = CountingSleeper::default();

        write_csv_export(&path, "first, utterance", InsertMode::Append, 0, &sleeper);
        write_csv_export(
            &path,
            "second \"quoted\" utterance",
            InsertMode::Append,
            0,
            &sleeper,
        );
        write_csv_export(&path, "third utterance", InsertMode::Append, 0, &sleeper);

        let bytes = std::fs::read(&path).unwrap();
        // Snapshot the body without the BOM (asserted separately by
        // `csv_append_writes_leading_utf8_bom`) so the golden file is clean
        // readable text.
        insta::assert_snapshot!(strip_utf8_bom_to_string(&bytes));
    }

    // -- UploadClient: bodies against TestServer ---------------------------------
    //
    // Binds TCP (`crate::models::test_server::TestServer::start`) -- blocked
    // under the sandbox, run with it lifted (see this task's report for the
    // full pre-existing set).

    #[test]
    fn upload_clear_posts_kind_clear_with_empty_text() {
        let server = crate::models::test_server::TestServer::start();
        server.set_route("/", crate::models::test_server::Canned::ok(Vec::new()));

        let client = UploadClient::new(server.base_url());
        client.clear();

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            requests[0].headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body, json!({"kind": "clear", "text": ""}));
    }

    #[test]
    fn upload_transcript_posts_append_mode_by_default() {
        let server = crate::models::test_server::TestServer::start();
        server.set_route("/", crate::models::test_server::Canned::ok(Vec::new()));

        let client = UploadClient::new(server.base_url());
        client.transcript("hello world", false);

        let requests = server.requests();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(
            body,
            json!({"kind": "transcript", "text": "hello world", "mode": "append"})
        );
    }

    #[test]
    fn upload_transcript_posts_replace_mode_for_append_and_correct() {
        let server = crate::models::test_server::TestServer::start();
        server.set_route("/", crate::models::test_server::Canned::ok(Vec::new()));

        let client = UploadClient::new(server.base_url());
        client.transcript("merged text", true);

        let requests = server.requests();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(
            body,
            json!({"kind": "transcript", "text": "merged text", "mode": "replace"})
        );
    }

    #[test]
    fn upload_failure_is_logged_and_never_panics() {
        // No server listening at all: connection refused. This must not
        // panic or return an error to the caller -- just log.
        let client = UploadClient::new("http://127.0.0.1:1");
        client.clear();
        client.transcript("text", false);
    }
}
