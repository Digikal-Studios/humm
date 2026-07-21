//! [`SqliteStore`]: the [`TaskStore`] implementation, plus the read-side
//! repository methods the queue and CLI/GUI use to display task history.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use humm_core::boundary::TaskStore;
use humm_core::domain::{Input, Millis, Segment, Source, Status, TaskSpec, TranscriptionId};
use humm_core::error::StoreError;

use crate::schema::migrate;

const SELECT_TASK_COLUMNS: &str = "id, status, source, file, url, progress, error_message, \
     time_queued, time_started, time_ended, name, notes";

/// The `humm_meta` key marking a completed [`SqliteStore::import_buzz_db`]
/// run.
const BUZZ_IMPORT_DONE_KEY: &str = "buzz_import_done";

/// Outcome of [`SqliteStore::import_buzz_db`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuzzImportOutcome {
    /// `true` if the import was a no-op because a previous run already
    /// completed it (`humm_meta['buzz_import_done']` was already set) — the
    /// Buzz file was not opened, and `tasks`/`segments` are `0`.
    pub skipped: bool,
    /// Number of `transcription` rows newly copied from the Buzz database
    /// this run. Rows whose `id` already existed in this store are not
    /// counted (and are not re-copied).
    pub tasks: usize,
    /// Number of `transcription_segment` rows newly copied this run,
    /// restricted to the transcriptions actually imported: a pre-existing
    /// transcription id's Buzz segments are never copied, even though that
    /// id itself was simply skipped as a duplicate rather than an error.
    pub segments: usize,
}

/// A row from the `transcription` table: a task's metadata, not its
/// segments (see [`SqliteStore::get_segments`]).
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    /// The task's id.
    pub id: TranscriptionId,
    /// Current lifecycle status.
    pub status: Status,
    /// Where the task's input came from. `None` when the stored value is
    /// `NULL` or not a recognized [`Source`] (e.g. an imported foreign row) —
    /// such rows must not fail to load.
    pub source: Option<Source>,
    /// Resolved local file path, if any (`NULL` for a not-yet-downloaded
    /// [`Input::Url`] task).
    pub file: Option<PathBuf>,
    /// Source URL, set only for [`Input::Url`] tasks.
    pub url: Option<String>,
    /// Fraction complete, in `0.0..=1.0`.
    pub progress: f64,
    /// Error message; set only when `status` is [`Status::Failed`].
    pub error_message: Option<String>,
    /// When the task was queued, as a naive-local ISO-8601 string
    /// (`%Y-%m-%dT%H:%M:%S%.f`, matching Buzz's `datetime.now().isoformat()`).
    pub time_queued: String,
    /// When the task started running, if it has.
    pub time_started: Option<String>,
    /// When the task finished (completed/failed/canceled), if it has.
    pub time_ended: Option<String>,
    /// User-assigned name. Always `None` in M2 (rename ships in M3).
    pub name: Option<String>,
    /// User-assigned notes. Always `None` in M2 (notes ship in M3).
    pub notes: Option<String>,
}

/// The raw, unparsed column values behind a [`TaskRow`] — an intermediate
/// step so column-fetch failures (`rusqlite::Error`) and domain-parsing
/// failures (unrecognized status/id) map through their own error paths.
struct RawTaskRow {
    id: String,
    status: String,
    source: Option<String>,
    file: Option<String>,
    url: Option<String>,
    progress: f64,
    error_message: Option<String>,
    time_queued: String,
    time_started: Option<String>,
    time_ended: Option<String>,
    name: Option<String>,
    notes: Option<String>,
}

fn row_to_raw(row: &rusqlite::Row) -> rusqlite::Result<RawTaskRow> {
    Ok(RawTaskRow {
        id: row.get(0)?,
        status: row.get(1)?,
        source: row.get(2)?,
        file: row.get(3)?,
        url: row.get(4)?,
        progress: row.get(5)?,
        error_message: row.get(6)?,
        time_queued: row.get(7)?,
        time_started: row.get(8)?,
        time_ended: row.get(9)?,
        name: row.get(10)?,
        notes: row.get(11)?,
    })
}

impl TryFrom<RawTaskRow> for TaskRow {
    type Error = StoreError;

    fn try_from(raw: RawTaskRow) -> Result<Self, StoreError> {
        let id = TranscriptionId::parse(&raw.id).ok_or_else(|| StoreError::Corrupt {
            detail: format!("invalid task id {:?}", raw.id),
        })?;
        let status = Status::from_str(&raw.status).map_err(|err| StoreError::Corrupt {
            detail: format!("invalid status {:?} for task {}: {err}", raw.status, raw.id),
        })?;
        // A NULL or unrecognized source string maps to `None` rather than an
        // error: imported/foreign rows must load, not fail.
        let source = raw.source.as_deref().and_then(|s| Source::from_str(s).ok());

        Ok(TaskRow {
            id,
            status,
            source,
            file: raw.file.map(PathBuf::from),
            url: raw.url,
            progress: raw.progress,
            error_message: raw.error_message,
            time_queued: raw.time_queued,
            time_started: raw.time_started,
            time_ended: raw.time_ended,
            name: raw.name,
            notes: raw.notes,
        })
    }
}

/// Naive-local ISO-8601 timestamp for "now", matching Buzz's
/// `datetime.now().isoformat()` (`%Y-%m-%dT%H:%M:%S%.f`).
fn now_iso() -> String {
    chrono::Local::now()
        .naive_local()
        .format("%Y-%m-%dT%H:%M:%S%.f")
        .to_string()
}

fn query_error(err: rusqlite::Error) -> StoreError {
    StoreError::Query {
        detail: err.to_string(),
    }
}

/// Builds the read-only, immutable `file:` URI used to `ATTACH` a Buzz
/// database, guaranteeing humm can never write to it. Percent-encodes `%`,
/// `?`, and `#` only when present — those are the characters that would
/// otherwise be parsed as `file:` URI query-string syntax; real Buzz paths
/// (`user_data_dir("Buzz")/Buzz.sqlite`) never contain them in practice, so a
/// full RFC 3986 encoder would be overkill here.
fn buzz_attach_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if raw.contains('%') || raw.contains('?') || raw.contains('#') {
        let encoded = raw
            .replace('%', "%25")
            .replace('?', "%3F")
            .replace('#', "%23");
        format!("file:{encoded}?mode=ro&immutable=1")
    } else {
        format!("file:{raw}?mode=ro&immutable=1")
    }
}

/// SQLite-backed [`TaskStore`] implementation (ADR-004). Schema v1 is a
/// Buzz-superset of `transcription`/`transcription_segment` plus humm's
/// `options_json`/`kind` additions (see `crate::schema`).
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Opens (creating if necessary) the database at `path`: creates the
    /// parent directory, enables WAL mode and foreign keys, and migrates the
    /// schema to the current version. Calling this twice on the same path is
    /// idempotent.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(|err| StoreError::Open {
            path: path.to_path_buf(),
            detail: err.to_string(),
        })?;
        Self::configure(conn)
    }

    /// Opens an in-memory database (tests only). Same pragmas/migration as
    /// [`Self::open`], except WAL mode is meaningless for `:memory:`
    /// databases — callers must not assert it there.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(query_error)?;
        Self::configure(conn)
    }

    fn configure(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(query_error)?;
        migrate(&conn)?;
        Ok(SqliteStore {
            conn: Mutex::new(conn),
        })
    }

    /// Locks the connection mutex, mapping poisoning (a prior panic while
    /// held) to [`StoreError::Query`] rather than panicking.
    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.conn.lock().map_err(|_| StoreError::Query {
            detail: "database connection mutex poisoned".to_string(),
        })
    }

    /// Flips every `queued`/`in_progress` task to `canceled`, setting
    /// `time_ended`. Call once at startup: a crashed or killed process
    /// leaves no task actually running, so any row still `queued` or
    /// `in_progress` is stale (F-029/F-001 edge). Returns the number of rows
    /// flipped. Ported from Buzz's
    /// `mark_in_progress_and_queued_transcriptions_as_canceled`
    /// (`buzz/db/helpers.py:79-89`).
    pub fn cancel_stale_tasks(&self) -> Result<usize, StoreError> {
        let now = now_iso();
        let conn = self.lock()?;
        let affected = conn
            .execute(
                "UPDATE transcription SET status = ?1, time_ended = ?2 \
                 WHERE status = ?3 OR status = ?4",
                params![
                    Status::Canceled.as_str(),
                    now,
                    Status::Queued.as_str(),
                    Status::InProgress.as_str(),
                ],
            )
            .map_err(query_error)?;
        Ok(affected)
    }

    /// `true` once [`Self::import_buzz_db`] has completed successfully at
    /// least once (`humm_meta['buzz_import_done']` is set).
    pub fn buzz_import_done(&self) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM humm_meta WHERE key = ?1",
                params![BUZZ_IMPORT_DONE_KEY],
                |row| row.get(0),
            )
            .optional()
            .map_err(query_error)?;
        Ok(value.is_some())
    }

    /// One-time import of a Buzz `Buzz.sqlite` database (Q-6, F-029): copies
    /// every `transcription` row not already present by `id`, plus each
    /// newly-copied row's `transcription_segment` rows, then records
    /// `humm_meta['buzz_import_done']` so later calls are a no-op
    /// (`skipped: true` — `buzz_db` is not even opened).
    ///
    /// `buzz_db` is `ATTACH`ed read-only/immutable (humm can never write to
    /// a file Buzz might still own) and the copy runs in one transaction
    /// with the completion marker: a failure partway through leaves neither
    /// partial rows nor the marker, so the next launch retries from
    /// scratch. The `ATTACH` is always balanced with a `DETACH` before this
    /// returns, whether or not the import itself succeeded.
    ///
    /// Imported rows keep their Buzz `status` verbatim — a Buzz
    /// `in_progress` (or `queued`) row imports as-is, not `canceled`.
    /// Callers must run [`Self::cancel_stale_tasks`] *after* this (as
    /// already happens at every startup) so an imported stale row gets
    /// flipped like any other; this method does not do that itself.
    /// `options_json` is left `NULL` (imported rows restart from column
    /// data, exactly as Buzz's own queued/in-progress rows did) and `kind`
    /// takes its schema default (`'file'`).
    ///
    /// Failures are the caller's to log — non-fatal by contract
    /// (`docs/design/ARCHITECTURE.md` §5).
    pub fn import_buzz_db(&self, buzz_db: &Path) -> Result<BuzzImportOutcome, StoreError> {
        if self.buzz_import_done()? {
            return Ok(BuzzImportOutcome {
                skipped: true,
                tasks: 0,
                segments: 0,
            });
        }

        let mut conn = self.lock()?;
        let uri = buzz_attach_uri(buzz_db);
        conn.execute("ATTACH DATABASE ?1 AS buzz", params![uri])
            .map_err(query_error)?;

        let outcome = Self::run_buzz_import(&mut conn);

        // Balance the ATTACH above regardless of how the import went: a
        // failed import must not leave `buzz` attached to this long-lived
        // connection (the next retry would then fail to re-attach it).
        let detach = conn
            .execute_batch("DETACH DATABASE buzz;")
            .map_err(query_error);

        match outcome {
            Ok(ok) => {
                detach?;
                Ok(ok)
            }
            Err(err) => Err(err),
        }
    }

    /// The `ATTACH`ed-and-locked half of [`Self::import_buzz_db`]: runs the
    /// copy in one transaction (marker write included) and commits, or (via
    /// the transaction's drop-triggered rollback on an `Err` return) leaves
    /// no trace.
    fn run_buzz_import(conn: &mut Connection) -> Result<BuzzImportOutcome, StoreError> {
        let tx = conn.transaction().map_err(query_error)?;

        // Snapshot which ids are new *before* inserting: once the insert
        // below runs, `NOT IN (SELECT id FROM transcription)` would no
        // longer identify them (they'd already be present), and segments
        // must be restricted to exactly this set — a pre-existing
        // transcription id's segments are never copied, even though the id
        // itself is just skipped as a duplicate.
        let new_ids: Vec<String> = {
            let mut stmt = tx
                .prepare(
                    "SELECT id FROM buzz.transcription \
                     WHERE id NOT IN (SELECT id FROM transcription)",
                )
                .map_err(query_error)?;
            stmt.query_map([], |row| row.get(0))
                .map_err(query_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(query_error)?
        };

        let tasks = tx
            .execute(
                "INSERT INTO transcription (
                    id, error_message, export_formats, file, output_folder, progress,
                    language, model_type, source, status, task, time_ended, time_queued,
                    time_started, url, whisper_model_size, hugging_face_model_id,
                    word_level_timings, extract_speech, name, notes
                )
                SELECT
                    id, error_message, export_formats, file, output_folder, progress,
                    language, model_type, source, status, task, time_ended, time_queued,
                    time_started, url, whisper_model_size, hugging_face_model_id,
                    word_level_timings, extract_speech, name, notes
                FROM buzz.transcription
                WHERE id NOT IN (SELECT id FROM transcription)",
                [],
            )
            .map_err(query_error)?;

        let segments = if new_ids.is_empty() {
            0
        } else {
            let placeholders = vec!["?"; new_ids.len()].join(", ");
            let sql = format!(
                "INSERT INTO transcription_segment \
                 (end_time, start_time, text, translation, transcription_id) \
                 SELECT end_time, start_time, text, translation, transcription_id \
                 FROM buzz.transcription_segment \
                 WHERE transcription_id IN ({placeholders})"
            );
            tx.execute(&sql, params_from_iter(new_ids.iter()))
                .map_err(query_error)?
        };

        tx.execute(
            "INSERT INTO humm_meta (key, value) VALUES (?1, ?2)",
            params![BUZZ_IMPORT_DONE_KEY, now_iso()],
        )
        .map_err(query_error)?;

        tx.commit().map_err(query_error)?;

        Ok(BuzzImportOutcome {
            skipped: false,
            tasks,
            segments,
        })
    }

    /// Fetches one task's row by id; `Ok(None)` if it doesn't exist.
    pub fn get_task(&self, id: &TranscriptionId) -> Result<Option<TaskRow>, StoreError> {
        let conn = self.lock()?;
        let raw = conn
            .query_row(
                &format!("SELECT {SELECT_TASK_COLUMNS} FROM transcription WHERE id = ?1"),
                params![id.to_string()],
                row_to_raw,
            )
            .optional()
            .map_err(query_error)?;
        raw.map(TaskRow::try_from).transpose()
    }

    /// Lists all tasks, most recently queued first (`time_queued DESC`).
    pub fn list_tasks(&self) -> Result<Vec<TaskRow>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {SELECT_TASK_COLUMNS} FROM transcription ORDER BY time_queued DESC"
            ))
            .map_err(query_error)?;
        let rows = stmt.query_map([], row_to_raw).map_err(query_error)?;
        let raw_rows = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(query_error)?;
        raw_rows.into_iter().map(TaskRow::try_from).collect()
    }

    /// Loads and deserializes the persisted [`TaskSpec`] for `id`.
    /// `Ok(None)` if the task doesn't exist or its `options_json` column is
    /// `NULL` (e.g. an imported foreign row).
    pub fn load_task_spec(&self, id: &TranscriptionId) -> Result<Option<TaskSpec>, StoreError> {
        let conn = self.lock()?;
        let json: Option<String> = conn
            .query_row(
                "SELECT options_json FROM transcription WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(query_error)?
            .flatten();
        drop(conn);

        match json {
            Some(json) => {
                serde_json::from_str(&json)
                    .map(Some)
                    .map_err(|err| StoreError::Corrupt {
                        detail: format!("invalid options_json for task {id}: {err}"),
                    })
            }
            None => Ok(None),
        }
    }

    /// Fetches all segments for `id`, in insertion order.
    pub fn get_segments(&self, id: &TranscriptionId) -> Result<Vec<Segment>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT start_time, end_time, text, translation FROM transcription_segment \
                 WHERE transcription_id = ?1 ORDER BY id ASC",
            )
            .map_err(query_error)?;
        let rows = stmt
            .query_map(params![id.to_string()], |row| {
                Ok(Segment {
                    start: Millis(row.get(0)?),
                    end: Millis(row.get(1)?),
                    text: row.get(2)?,
                    translation: row.get(3)?,
                })
            })
            .map_err(query_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(query_error)
    }
}

impl TaskStore for SqliteStore {
    fn create_task(
        &self,
        id: &TranscriptionId,
        spec: &TaskSpec,
        source: Source,
    ) -> Result<(), StoreError> {
        let now = now_iso();
        let export_formats = spec
            .output
            .formats
            .iter()
            .map(|format| format.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let (file, url) = match &spec.input {
            Input::Path(path) => (Some(path.display().to_string()), None),
            Input::Url(url) => (None, Some(url.to_string())),
        };
        let model_type = if spec.options.model.hf_repo.is_some() {
            "Hugging Face"
        } else {
            "Whisper.cpp"
        };
        let language = spec.options.language.map(|lang| lang.code().to_string());
        let output_folder = spec
            .output
            .directory
            .as_ref()
            .map(|dir| dir.display().to_string());
        let options_json = serde_json::to_string(spec).map_err(|err| StoreError::Query {
            detail: format!("serializing task spec: {err}"),
        })?;

        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO transcription (
                id, export_formats, file, output_folder, language, model_type,
                source, status, task, time_queued, url, whisper_model_size,
                hugging_face_model_id, word_level_timings, extract_speech,
                name, notes, options_json, kind
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                NULL, NULL, ?16, 'file'
            )",
            params![
                id.to_string(),
                export_formats,
                file,
                output_folder,
                language,
                model_type,
                source.as_str(),
                Status::Queued.as_str(),
                spec.options.task.as_str(),
                now,
                url,
                spec.options.model.size.as_str(),
                spec.options.model.hf_repo,
                spec.options.word_timestamps,
                // No `TaskSpec` field carries this yet — Demucs speech
                // extraction (F-027) is a later milestone; every task is
                // created with `extract_speech = 0` until it exists.
                false,
                options_json,
            ],
        )
        .map_err(query_error)?;
        Ok(())
    }

    fn mark_started(&self, id: &TranscriptionId) -> Result<(), StoreError> {
        let now = now_iso();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET status = ?1, time_started = ?2 WHERE id = ?3",
            params![Status::InProgress.as_str(), now, id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
    }

    fn update_progress(&self, id: &TranscriptionId, fraction: f64) -> Result<(), StoreError> {
        let clamped = fraction.clamp(0.0, 1.0);
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET status = ?1, progress = ?2 WHERE id = ?3",
            params![Status::InProgress.as_str(), clamped, id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
    }

    fn update_file(&self, id: &TranscriptionId, path: &Path) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET file = ?1 WHERE id = ?2",
            params![path.display().to_string(), id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
    }

    fn mark_completed(&self, id: &TranscriptionId, segments: &[Segment]) -> Result<(), StoreError> {
        let now = now_iso();
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(query_error)?;
        tx.execute(
            "UPDATE transcription SET status = ?1, time_ended = ?2 WHERE id = ?3",
            params![Status::Completed.as_str(), now, id.to_string()],
        )
        .map_err(query_error)?;
        for segment in segments {
            tx.execute(
                "INSERT INTO transcription_segment \
                 (start_time, end_time, text, translation, transcription_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    segment.start.0,
                    segment.end.0,
                    segment.text,
                    segment.translation,
                    id.to_string(),
                ],
            )
            .map_err(query_error)?;
        }
        tx.commit().map_err(query_error)?;
        Ok(())
    }

    fn mark_failed(&self, id: &TranscriptionId, error: &str) -> Result<(), StoreError> {
        let now = now_iso();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET status = ?1, time_ended = ?2, error_message = ?3 \
             WHERE id = ?4",
            params![Status::Failed.as_str(), now, error, id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
    }

    fn mark_canceled(&self, id: &TranscriptionId) -> Result<(), StoreError> {
        let now = now_iso();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET status = ?1, time_ended = ?2 WHERE id = ?3",
            params![Status::Canceled.as_str(), now, id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::{
        DEFAULT_TEMPERATURE, ModelSize, ModelSpec, OutputFormat, OutputSpec, TaskKind,
        TranscriptionOptions,
    };
    use humm_core::language::Language;
    use pretty_assertions::assert_eq;

    fn sample_task_spec() -> TaskSpec {
        TaskSpec {
            input: Input::Path(PathBuf::from("/tmp/in.wav")),
            options: TranscriptionOptions {
                language: Language::parse("en"),
                task: TaskKind::Transcribe,
                model: ModelSpec {
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                word_timestamps: false,
                initial_prompt: String::new(),
                temperature: DEFAULT_TEMPERATURE.to_vec(),
            },
            output: OutputSpec {
                formats: [OutputFormat::Txt, OutputFormat::Srt].into_iter().collect(),
                directory: Some(PathBuf::from("/tmp/out")),
                file_name_template: None,
                paragraph_split: Millis(2000),
            },
        }
    }

    /// Every raw column of the `transcription` table, fetched directly
    /// (bypassing [`TaskRow`], which intentionally exposes only a subset) so
    /// the create-task fill contract can be asserted column-by-column.
    struct FullRow {
        error_message: Option<String>,
        export_formats: Option<String>,
        file: Option<String>,
        output_folder: Option<String>,
        progress: f64,
        language: Option<String>,
        model_type: Option<String>,
        source: Option<String>,
        status: Option<String>,
        task: Option<String>,
        time_ended: Option<String>,
        time_queued: String,
        time_started: Option<String>,
        url: Option<String>,
        whisper_model_size: Option<String>,
        hugging_face_model_id: Option<String>,
        word_level_timings: bool,
        extract_speech: bool,
        name: Option<String>,
        notes: Option<String>,
        options_json: Option<String>,
        kind: String,
    }

    fn fetch_full_row(store: &SqliteStore, id: &TranscriptionId) -> FullRow {
        let conn = store.conn.lock().expect("lock");
        conn.query_row(
            "SELECT error_message, export_formats, file, output_folder, progress, language, \
             model_type, source, status, task, time_ended, time_queued, time_started, url, \
             whisper_model_size, hugging_face_model_id, word_level_timings, extract_speech, \
             name, notes, options_json, kind FROM transcription WHERE id = ?1",
            params![id.to_string()],
            |row| {
                Ok(FullRow {
                    error_message: row.get(0)?,
                    export_formats: row.get(1)?,
                    file: row.get(2)?,
                    output_folder: row.get(3)?,
                    progress: row.get(4)?,
                    language: row.get(5)?,
                    model_type: row.get(6)?,
                    source: row.get(7)?,
                    status: row.get(8)?,
                    task: row.get(9)?,
                    time_ended: row.get(10)?,
                    time_queued: row.get(11)?,
                    time_started: row.get(12)?,
                    url: row.get(13)?,
                    whisper_model_size: row.get(14)?,
                    hugging_face_model_id: row.get(15)?,
                    word_level_timings: row.get(16)?,
                    extract_speech: row.get(17)?,
                    name: row.get(18)?,
                    notes: row.get(19)?,
                    options_json: row.get(20)?,
                    kind: row.get(21)?,
                })
            },
        )
        .expect("row present")
    }

    #[test]
    fn create_task_fills_every_column_per_contract() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let spec = sample_task_spec();
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        let row = fetch_full_row(&store, &id);
        assert_eq!(row.error_message, None);
        assert_eq!(row.export_formats.as_deref(), Some("txt, srt"));
        assert_eq!(row.file.as_deref(), Some("/tmp/in.wav"));
        assert_eq!(row.output_folder.as_deref(), Some("/tmp/out"));
        assert_eq!(row.progress, 0.0);
        assert_eq!(row.language.as_deref(), Some("en"));
        assert_eq!(row.model_type.as_deref(), Some("Whisper.cpp"));
        assert_eq!(row.source.as_deref(), Some("file_import"));
        assert_eq!(row.status.as_deref(), Some("queued"));
        assert_eq!(row.task.as_deref(), Some("transcribe"));
        assert_eq!(row.time_ended, None);
        assert!(!row.time_queued.is_empty());
        assert_eq!(row.time_started, None);
        assert_eq!(row.url, None);
        assert_eq!(row.whisper_model_size.as_deref(), Some("small"));
        assert_eq!(row.hugging_face_model_id, None);
        assert!(!row.word_level_timings);
        assert!(!row.extract_speech);
        assert_eq!(row.name, None);
        assert_eq!(row.notes, None);
        assert_eq!(row.kind, "file");

        let round_tripped: TaskSpec =
            serde_json::from_str(&row.options_json.expect("options_json set")).expect("parses");
        assert_eq!(round_tripped, spec);
    }

    #[test]
    fn model_type_is_hugging_face_when_hf_repo_is_some() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_task_spec();
        spec.options.model.hf_repo = Some("openai/whisper-large-v3-turbo".to_string());
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        let row = fetch_full_row(&store, &id);
        assert_eq!(row.model_type.as_deref(), Some("Hugging Face"));
        assert_eq!(
            row.hugging_face_model_id.as_deref(),
            Some("openai/whisper-large-v3-turbo")
        );
    }

    #[test]
    fn language_column_is_null_when_not_set() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_task_spec();
        spec.options.language = None;
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        let row = fetch_full_row(&store, &id);
        assert_eq!(row.language, None);
    }

    #[test]
    fn url_input_task_sets_url_not_file_then_update_file_sets_file() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_task_spec();
        spec.input = Input::Url(url::Url::parse("https://example.com/a.mp3").expect("url"));
        store
            .create_task(&id, &spec, Source::UrlImport)
            .expect("create");

        let row = fetch_full_row(&store, &id);
        assert_eq!(row.file, None);
        assert_eq!(row.url.as_deref(), Some("https://example.com/a.mp3"));

        store
            .update_file(&id, Path::new("/resolved/a.mp3"))
            .expect("update_file");
        let row = fetch_full_row(&store, &id);
        assert_eq!(row.file.as_deref(), Some("/resolved/a.mp3"));
        // update_file must not disturb url.
        assert_eq!(row.url.as_deref(), Some("https://example.com/a.mp3"));
    }

    #[test]
    fn full_lifecycle_queued_started_progress_completed() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.status, Status::Queued);
        assert!(row.time_started.is_none());
        assert!(row.time_ended.is_none());

        store.mark_started(&id).expect("started");
        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.status, Status::InProgress);
        assert!(row.time_started.is_some());

        store.update_progress(&id, 0.5).expect("progress");
        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.status, Status::InProgress);
        assert_eq!(row.progress, 0.5);

        let segments = vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "hello".to_string(),
                translation: String::new(),
            },
            Segment {
                start: Millis(1000),
                end: Millis(2000),
                text: "world".to_string(),
                translation: String::new(),
            },
        ];
        store.mark_completed(&id, &segments).expect("completed");
        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.status, Status::Completed);
        assert!(row.time_ended.is_some());

        let stored_segments = store.get_segments(&id).expect("segments");
        assert_eq!(stored_segments, segments);
    }

    #[test]
    fn update_progress_clamps_out_of_range_fractions() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        store.update_progress(&id, 1.5).expect("progress high");
        assert_eq!(store.get_task(&id).unwrap().unwrap().progress, 1.0);

        store.update_progress(&id, -0.5).expect("progress low");
        assert_eq!(store.get_task(&id).unwrap().unwrap().progress, 0.0);
    }

    #[test]
    fn failed_sets_error_message_canceled_sets_time_ended_no_segments() {
        let store = SqliteStore::open_in_memory().expect("open");

        let failed_id = TranscriptionId::new();
        store
            .create_task(&failed_id, &sample_task_spec(), Source::FileImport)
            .expect("create");
        store.mark_failed(&failed_id, "boom").expect("failed");
        let row = store.get_task(&failed_id).expect("get").expect("present");
        assert_eq!(row.status, Status::Failed);
        assert_eq!(row.error_message.as_deref(), Some("boom"));
        assert!(row.time_ended.is_some());
        assert!(store.get_segments(&failed_id).expect("segments").is_empty());

        let canceled_id = TranscriptionId::new();
        store
            .create_task(&canceled_id, &sample_task_spec(), Source::FileImport)
            .expect("create");
        store.mark_canceled(&canceled_id).expect("canceled");
        let row = store.get_task(&canceled_id).expect("get").expect("present");
        assert_eq!(row.status, Status::Canceled);
        assert!(row.time_ended.is_some());
        assert!(
            store
                .get_segments(&canceled_id)
                .expect("segments")
                .is_empty()
        );
    }

    #[test]
    fn cancel_stale_tasks_flips_queued_and_in_progress_not_completed() {
        let store = SqliteStore::open_in_memory().expect("open");

        let queued_id = TranscriptionId::new();
        store
            .create_task(&queued_id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        let in_progress_id = TranscriptionId::new();
        store
            .create_task(&in_progress_id, &sample_task_spec(), Source::FileImport)
            .expect("create");
        store.mark_started(&in_progress_id).expect("started");

        let completed_id = TranscriptionId::new();
        store
            .create_task(&completed_id, &sample_task_spec(), Source::FileImport)
            .expect("create");
        store.mark_completed(&completed_id, &[]).expect("completed");

        let flipped = store.cancel_stale_tasks().expect("cancel_stale");
        assert_eq!(flipped, 2);

        assert_eq!(
            store.get_task(&queued_id).unwrap().unwrap().status,
            Status::Canceled
        );
        assert_eq!(
            store.get_task(&in_progress_id).unwrap().unwrap().status,
            Status::Canceled
        );
        assert_eq!(
            store.get_task(&completed_id).unwrap().unwrap().status,
            Status::Completed
        );
    }

    #[test]
    fn load_task_spec_round_trips_and_null_options_json_is_none() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let spec = sample_task_spec();
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        let loaded = store.load_task_spec(&id).expect("load").expect("present");
        assert_eq!(loaded, spec);

        // A foreign/imported row with no `options_json` (column omitted, so
        // NULL) must round-trip to `Ok(None)`, not error.
        let foreign_id = TranscriptionId::new();
        {
            let conn = store.conn.lock().expect("lock");
            conn.execute(
                "INSERT INTO transcription (id, status, time_queued, kind) \
                 VALUES (?1, 'completed', ?2, 'file')",
                params![foreign_id.to_string(), "2020-01-01T00:00:00"],
            )
            .expect("insert foreign row");
        }
        assert_eq!(store.load_task_spec(&foreign_id).expect("load"), None);
    }

    #[test]
    fn get_task_returns_none_source_for_unrecognized_or_null_source() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        {
            let conn = store.conn.lock().expect("lock");
            conn.execute(
                "INSERT INTO transcription (id, status, source, time_queued, kind) \
                 VALUES (?1, 'completed', 'some_legacy_source', ?2, 'file')",
                params![id.to_string(), "2020-01-01T00:00:00"],
            )
            .expect("insert foreign row");
        }
        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.source, None);
    }

    #[test]
    fn open_twice_on_same_path_is_idempotent_and_enables_wal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("humm.sqlite");

        {
            let store = SqliteStore::open(&db_path).expect("first open");
            let id = TranscriptionId::new();
            store
                .create_task(&id, &sample_task_spec(), Source::FileImport)
                .expect("create");
        }

        let store = SqliteStore::open(&db_path).expect("second open");
        let tasks = store.list_tasks().expect("list");
        assert_eq!(tasks.len(), 1);

        let conn = store.conn.lock().expect("lock");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal_mode");
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn list_tasks_orders_by_time_queued_descending() {
        let store = SqliteStore::open_in_memory().expect("open");
        let first = TranscriptionId::new();
        store
            .create_task(&first, &sample_task_spec(), Source::FileImport)
            .expect("create first");
        let second = TranscriptionId::new();
        store
            .create_task(&second, &sample_task_spec(), Source::FileImport)
            .expect("create second");

        // Force a deterministic ordering independent of wall-clock
        // resolution: rewrite time_queued directly.
        {
            let conn = store.conn.lock().expect("lock");
            conn.execute(
                "UPDATE transcription SET time_queued = '2020-01-01T00:00:00' WHERE id = ?1",
                params![first.to_string()],
            )
            .expect("update first");
            conn.execute(
                "UPDATE transcription SET time_queued = '2021-01-01T00:00:00' WHERE id = ?1",
                params![second.to_string()],
            )
            .expect("update second");
        }

        let tasks = store.list_tasks().expect("list");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, second);
        assert_eq!(tasks[1].id, first);
    }
}
