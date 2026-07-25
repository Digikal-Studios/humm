//! [`SqliteStore`]: the [`TaskStore`] implementation, plus the read-side
//! repository methods the queue and CLI/GUI use to display task history.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};

use std::collections::BTreeSet;

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use humm_core::boundary::TaskStore;
use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, Input, Millis, ModelSize, ModelSpec, OutputFormat, OutputSpec,
    Segment, Source, Status, TaskKind, TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::error::StoreError;
use humm_core::language::Language;

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

/// The raw `(model_type, whisper_model_size)` column pair
/// [`SqliteStore::model_display_columns`] returns — display-only, see that
/// method's doc comment.
pub type ModelDisplayColumns = (Option<String>, Option<String>);

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

/// A row from the `transcription_segment` table, id included — the viewer
/// (M4) edits/reorders individual segments by their row id, unlike
/// [`SqliteStore::get_segments`]'s plain [`Segment`] list.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentRow {
    /// The segment's `transcription_segment.id` (its SQLite row id) — the
    /// handle [`SqliteStore::update_segment_times`],
    /// [`SqliteStore::update_segment_text`], and
    /// [`SqliteStore::update_segment_translation`] address a row by.
    pub id: i64,
    /// Start time of the segment.
    pub start: Millis,
    /// End time of the segment.
    pub end: Millis,
    /// Recognized text.
    pub text: String,
    /// Translated text (English); empty unless translation was requested.
    pub translation: String,
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

/// Placeholder [`Input`] embedded in a live session's persisted [`TaskSpec`]
/// (`options_json`, see [`SqliteStore::insert_live_session`]). A live session
/// has no file or URL to transcribe — its `file`/`url` *columns* are `NULL`
/// — but [`SqliteStore::load_task_spec`] deserializes `options_json` back
/// into a whole `TaskSpec`, so a filler `input` keeps that round-trip
/// working like any other row's. Every current reader of a loaded `TaskSpec`
/// (`export_menu`, the resizer's word-timestamps/language gate, the retry
/// path) only ever reads `.options`; nothing must treat this filler as a
/// real path.
fn live_session_input_placeholder() -> Input {
    Input::Path(PathBuf::new())
}

fn query_error(err: rusqlite::Error) -> StoreError {
    StoreError::Query {
        detail: err.to_string(),
    }
}

/// Interprets a raw SQLite value from a `BOOLEAN`-affinity column (
/// `word_level_timings`, `extract_speech`) as a bool, tolerating every
/// storage class such a value might arrive in: humm's own
/// [`SqliteStore::create_task`] binds a Rust `bool`, which rusqlite/SQLite
/// store as `INTEGER` `0`/`1`; a real Buzz install's own restart path
/// instead reads this column back as a Python `str` and compares it to
/// `"1"` (`transcription_tasks_table_widget.py:758`) — so a `"1"`/`"true"`
/// text value, however it got onto a row, is also truthy here. Anything
/// else (`NULL`, `0`, `"0"`, a blob) is `false`.
fn parse_loose_bool(value: &Value) -> bool {
    match value {
        Value::Integer(i) => *i != 0,
        Value::Real(r) => *r != 0.0,
        Value::Text(s) => s == "1" || s.eq_ignore_ascii_case("true"),
        Value::Null | Value::Blob(_) => false,
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

/// Folds a `DETACH DATABASE buzz` failure into an already-failed import's
/// error, so a caller that only logs the returned [`StoreError`] still
/// learns that the `buzz` alias may still be attached to this connection —
/// same-process retries would then fail at `ATTACH` with a confusing
/// "already in use" error rather than the original problem repeating.
/// `StoreError` is `#[non_exhaustive]`, so unrecognized/future variants fall
/// through the wildcard arm as a [`StoreError::Query`] carrying the original
/// error's `Display` text plus the appended detail — nothing is lost.
fn augment_with_detach_failure(err: StoreError, detach_err: StoreError) -> StoreError {
    let suffix = format!(
        "; additionally, DETACH DATABASE buzz failed ({detach_err}) — the \
         alias may still be attached and retries in this process may fail"
    );
    match err {
        StoreError::Open { path, detail } => StoreError::Open {
            path,
            detail: format!("{detail}{suffix}"),
        },
        StoreError::Query { detail } => StoreError::Query {
            detail: format!("{detail}{suffix}"),
        },
        StoreError::Corrupt { detail } => StoreError::Corrupt {
            detail: format!("{detail}{suffix}"),
        },
        other => StoreError::Query {
            detail: format!("{other}{suffix}"),
        },
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
        Self::configure(conn, path)
    }

    /// Opens an in-memory database (tests only). Same pragmas/migration as
    /// [`Self::open`], except WAL mode is meaningless for `:memory:`
    /// databases — callers must not assert it there.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(query_error)?;
        Self::configure(conn, Path::new(":memory:"))
    }

    /// `path` is used only to name the database in a
    /// [`StoreError::SchemaTooNew`] error message — `:memory:` databases
    /// pass a placeholder since they have no real path.
    fn configure(conn: Connection, path: &Path) -> Result<Self, StoreError> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(query_error)?;
        migrate(&conn, path)?;
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
    ///
    /// **Live-recording special case** (Q-10 ✚, `docs/superpowers/specs/
    /// 2026-07-23-m5-live-recording-design.md` §4): a stale `in_progress`
    /// row with `kind = "live_recording"` (see [`Self::insert_live_session`])
    /// flips to **`completed`** instead — a killed process loses at most the
    /// in-flight chunk, and the partial transcript already on disk is
    /// finished work worth opening in the M4 viewer; `canceled` would lock
    /// it behind the viewer's Completed/Skipped-only open gate. Every other
    /// row (any `queued` row, or an `in_progress` row of any other `kind`)
    /// keeps today's exact behavior. The live pass runs first so the
    /// unchanged second query — matching on `status` alone — naturally
    /// excludes the rows it already flipped to `completed`, with no `kind`
    /// filter needed there.
    pub fn cancel_stale_tasks(&self) -> Result<usize, StoreError> {
        let now = now_iso();
        let conn = self.lock()?;

        let live_completed = conn
            .execute(
                "UPDATE transcription SET status = ?1, time_ended = COALESCE(time_ended, ?2) \
                 WHERE status = ?3 AND kind = ?4",
                params![
                    Status::Completed.as_str(),
                    now,
                    Status::InProgress.as_str(),
                    Source::LiveRecording.as_str(),
                ],
            )
            .map_err(query_error)?;

        let other_canceled = conn
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

        Ok(live_completed + other_canceled)
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
            // The import already failed; if DETACH *also* failed, don't
            // silently drop that — `buzz` may still be attached to this
            // long-lived connection, so same-process retries could fail on
            // `ATTACH` with a confusing "already in use" error. Fold both
            // failures into one error rather than picking just one.
            Err(err) => Err(match detach {
                Ok(()) => err,
                Err(detach_err) => augment_with_detach_failure(err, detach_err),
            }),
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
            // One bound parameter per imported id. SQLite caps bound
            // parameters per statement (`SQLITE_LIMIT_VARIABLE_NUMBER`: 999
            // on SQLite < 3.32, 32766 on the bundled current version) — an
            // enormous Buzz history (999+/32766+ *new* rows in one import)
            // would fail this statement. Accepted for personal-scale
            // histories; not worth a batching scheme for this one-time,
            // one-user import.
            let placeholders = vec!["?"; new_ids.len()].join(", ");
            let sql = format!(
                "INSERT INTO transcription_segment \
                 (end_time, start_time, text, translation, transcription_id) \
                 SELECT end_time, start_time, text, translation, transcription_id \
                 FROM buzz.transcription_segment \
                 WHERE transcription_id IN ({placeholders}) \
                 ORDER BY id"
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

    /// Finds the newest (`time_ended` descending) `completed` task whose
    /// `file` column's **basename** matches `name` — Buzz's
    /// `find_completed_transcription_by_filename`
    /// (`skip_already_transcribed/plugin.py:163-166` calls it with
    /// `os.path.basename(file_path)`; the DAO itself matches `file` against
    /// `%/<name>` or an exact `<name>`, ordered `time_ended DESC`,
    /// `transcription_dao.py:336-351` — ordering by completion time, not
    /// queue time, so a task queued earlier but finishing later still wins).
    ///
    /// The basename comparison is done on the `file` column's *tail* via
    /// `substr`/`length` rather than a `LIKE` pattern built from `name` — an
    /// equivalent exact-tail check, so a filename containing SQL wildcard
    /// characters (`%`, `_`) is compared literally instead of being
    /// reinterpreted as a pattern: the trailing `length(name)` characters of
    /// `file` must equal `name` exactly, and either `file` is exactly `name`
    /// (no directory component at all) or the character immediately before
    /// that tail is a path separator (`/` or `\`, so a Windows-produced row
    /// still basename-matches on any host). No index is added on `file` for
    /// this lookup — deliberately: this table is
    /// personal-transcription-history scale (a full scan of a few thousand
    /// rows costs nothing worth an index for).
    pub fn find_completed_transcription_by_basename(
        &self,
        name: &str,
    ) -> Result<Option<TranscriptionId>, StoreError> {
        let conn = self.lock()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT id FROM transcription \
                 WHERE status = ?1 \
                   AND file IS NOT NULL \
                   AND length(file) >= length(?2) \
                   AND substr(file, -length(?2)) = ?2 \
                   AND (length(file) = length(?2) \
                        OR substr(file, length(file) - length(?2), 1) IN ('/', '\\')) \
                 ORDER BY time_ended DESC \
                 LIMIT 1",
                params![Status::Completed.as_str(), name],
                |row| row.get(0),
            )
            .optional()
            .map_err(query_error)?;
        raw.map(|raw| {
            TranscriptionId::parse(&raw).ok_or_else(|| StoreError::Corrupt {
                detail: format!("invalid task id {raw:?}"),
            })
        })
        .transpose()
    }

    /// Reports whether any task — regardless of status — has `file` exactly
    /// equal to `path`'s displayed form. An exact-path existence check, not
    /// a basename search (contrast
    /// [`Self::find_completed_transcription_by_basename`]).
    pub fn has_task_for_file(&self, path: &Path) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM transcription WHERE file = ?1)",
            params![path.display().to_string()],
            |row| row.get(0),
        )
        .map_err(query_error)
    }

    /// Sets a task's user-assigned display name. Ported from Buzz's
    /// `TranscriptionDAO.update_transcription_name`
    /// (`transcription_dao.py:269-284`), which raises when the row doesn't
    /// exist (`query.numRowsAffected() == 0`); this returns
    /// `Err(StoreError::Query)` in that case.
    pub fn set_task_name(&self, id: &TranscriptionId, name: &str) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let affected = conn
            .execute(
                "UPDATE transcription SET name = ?1 WHERE id = ?2",
                params![name, id.to_string()],
            )
            .map_err(query_error)?;
        if affected == 0 {
            return Err(StoreError::Query {
                detail: format!("set_task_name: no task with id {id}"),
            });
        }
        Ok(())
    }

    /// Sets a task's free-text notes. Ported from Buzz's
    /// `TranscriptionDAO.update_transcription_notes`
    /// (`transcription_dao.py:303-318`), which raises when the row doesn't
    /// exist (`query.numRowsAffected() == 0`); this returns
    /// `Err(StoreError::Query)` in that case.
    pub fn set_task_notes(&self, id: &TranscriptionId, notes: &str) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let affected = conn
            .execute(
                "UPDATE transcription SET notes = ?1 WHERE id = ?2",
                params![notes, id.to_string()],
            )
            .map_err(query_error)?;
        if affected == 0 {
            return Err(StoreError::Query {
                detail: format!("set_task_notes: no task with id {id}"),
            });
        }
        Ok(())
    }

    /// Deletes `ids` (and, via the `ON DELETE CASCADE` foreign key —
    /// active because `Self::configure` enables `PRAGMA foreign_keys=ON`
    /// — their `transcription_segment` rows). Returns the number of
    /// `transcription` rows actually removed; an id with no matching row is
    /// silently skipped rather than an error. `ids.is_empty()` is a no-op
    /// returning `Ok(0)` without touching the database.
    ///
    /// One `DELETE ... WHERE id IN (...)` statement with one bound
    /// parameter per id, the same placeholder-building pattern
    /// `Self::run_buzz_import` uses for its segment copy.
    pub fn delete_tasks(&self, ids: &[TranscriptionId]) -> Result<usize, StoreError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.lock()?;
        let placeholders = vec!["?"; ids.len()].join(", ");
        let sql = format!("DELETE FROM transcription WHERE id IN ({placeholders})");
        let id_strings: Vec<String> = ids.iter().map(TranscriptionId::to_string).collect();
        let affected = conn
            .execute(&sql, params_from_iter(id_strings.iter()))
            .map_err(query_error)?;
        Ok(affected)
    }

    /// Rebuilds a Buzz-parity [`TaskSpec`] from a task's raw columns —
    /// for rows whose `options_json` is `NULL` (an imported Buzz row, or
    /// any row a future humm version wrote without it).
    /// [`Self::load_task_spec`] is for the `options_json` round-trip; this
    /// is the column-only fallback. `Ok(None)` if `id` doesn't exist.
    ///
    /// Ported from Buzz's `_restart_transcription_task`
    /// (`transcription_tasks_table_widget.py:727-780`): every column is
    /// parsed tolerantly, with the same fallbacks Buzz applies on a
    /// restart — an unparseable/absent `language` becomes `None`, `task`
    /// becomes [`TaskKind::Transcribe`], `export_formats` entries that
    /// don't parse are dropped rather than failing the whole list, and
    /// (Buzz's own restart fallback, not this crate's
    /// `TranscriptionOptions::default()` `Small`) an unparseable/absent
    /// `whisper_model_size` becomes [`ModelSize::Tiny`].
    /// `initial_prompt`/`temperature` are not persisted as columns, so they
    /// take `TranscriptionOptions::default()`'s values — matching Buzz's
    /// restart, which hardcodes `initial_prompt=""` and never restores
    /// temperature either.
    pub fn spec_from_columns(&self, id: &TranscriptionId) -> Result<Option<TaskSpec>, StoreError> {
        let conn = self.lock()?;
        let row = conn
            .query_row(
                "SELECT file, url, language, task, whisper_model_size, \
                 hugging_face_model_id, word_level_timings, export_formats, output_folder \
                 FROM transcription WHERE id = ?1",
                params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Value>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(query_error)?;
        drop(conn);

        let Some((
            file,
            url,
            language,
            task,
            whisper_model_size,
            hugging_face_model_id,
            word_level_timings,
            export_formats,
            output_folder,
        )) = row
        else {
            return Ok(None);
        };

        // "input from url else file": the `url`/`file` columns are mutually
        // exclusive by construction (`create_task` sets exactly one), so
        // preferring `url` when present is unambiguous.
        let input = match url {
            Some(url) => Input::parse(&url),
            None => Input::Path(file.map(PathBuf::from).unwrap_or_default()),
        };

        let options = TranscriptionOptions {
            language: language.as_deref().and_then(Language::parse),
            task: task
                .as_deref()
                .and_then(|s| TaskKind::from_str(s).ok())
                .unwrap_or(TaskKind::Transcribe),
            model: ModelSpec {
                // Buzz's restart path never persisted a backend column
                // (predates F-041 entirely), so a restored row is always
                // whisper.cpp — matching Buzz's own restart, which hardcodes
                // AI-translation-equivalent settings off too (see
                // `enable_llm_translation` -> `llm: None` below).
                backend: Backend::Whisper,
                size: whisper_model_size
                    .as_deref()
                    .and_then(|s| ModelSize::from_str(s).ok())
                    .unwrap_or(ModelSize::Tiny),
                hf_repo: hugging_face_model_id,
            },
            word_timestamps: parse_loose_bool(&word_level_timings),
            initial_prompt: String::new(),
            temperature: DEFAULT_TEMPERATURE.to_vec(),
            // No `llm` column exists in the legacy per-column schema this
            // restart path reads from — matching Buzz's own restart, which
            // hardcodes AI translation off (`transcription_tasks_table_widget.py:762`).
            llm: None,
        };

        // Tolerant, comma-split (Buzz's own restart splits on `','` and
        // `.strip()`s each piece, even though `create_task` writes `", "`-
        // joined values): unrecognized entries are dropped, not an error.
        let formats: BTreeSet<OutputFormat> = export_formats
            .as_deref()
            .map(|s| {
                s.split(',')
                    .filter_map(|part| OutputFormat::from_str(part.trim()).ok())
                    .collect()
            })
            .unwrap_or_default();

        let output = OutputSpec {
            formats,
            directory: output_folder.map(PathBuf::from),
            ..OutputSpec::default()
        };

        Ok(Some(TaskSpec {
            input,
            options,
            output,
        }))
    }

    /// Reads the raw `model_type`/`whisper_model_size` columns for `id`,
    /// **display-only** (the GUI task table's Model column, for rows whose
    /// `options_json` is `NULL` — an imported/foreign row with no
    /// [`TaskSpec`] to derive a proper model description from). `Ok(None)` if
    /// `id` doesn't exist.
    ///
    /// This is *not* a substitute for [`Self::spec_from_columns`]: it hands
    /// back the two raw strings verbatim (no `ModelSize`/`ModelSpec`
    /// parsing, no Buzz-restart fallbacks) for a caller that only wants to
    /// *show* something reasonable, never to reconstruct a runnable model
    /// selection.
    pub fn model_display_columns(
        &self,
        id: &TranscriptionId,
    ) -> Result<Option<ModelDisplayColumns>, StoreError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT model_type, whisper_model_size FROM transcription WHERE id = ?1",
            params![id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(query_error)
    }

    // -- M4 viewer support: segment rows, replace/copy, export folder ----

    /// Fetches all segments for `id` as [`SegmentRow`]s (id included), in
    /// ascending id order — the order they were inserted in, and the order
    /// the viewer table displays them.
    pub fn get_segment_rows(&self, id: TranscriptionId) -> Result<Vec<SegmentRow>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, start_time, end_time, text, translation \
                 FROM transcription_segment WHERE transcription_id = ?1 ORDER BY id ASC",
            )
            .map_err(query_error)?;
        let rows = stmt
            .query_map(params![id.to_string()], |row| {
                Ok(SegmentRow {
                    id: row.get(0)?,
                    start: Millis(row.get(1)?),
                    end: Millis(row.get(2)?),
                    text: row.get(3)?,
                    translation: row.get(4)?,
                })
            })
            .map_err(query_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(query_error)
    }

    /// Updates one segment row's `start`/`end` times in place. `seg` is a
    /// `transcription_segment.id` (see [`SegmentRow::id`]), not a
    /// [`TranscriptionId`]. An unknown `seg` is a silent no-op (0 rows
    /// affected) — the viewer only ever calls this with an id it just read
    /// via [`Self::get_segment_rows`].
    pub fn update_segment_times(
        &self,
        seg: i64,
        start: Millis,
        end: Millis,
    ) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription_segment SET start_time = ?1, end_time = ?2 WHERE id = ?3",
            params![start.0, end.0, seg],
        )
        .map_err(query_error)?;
        Ok(())
    }

    /// Updates one segment row's `text` in place. See
    /// [`Self::update_segment_times`] for the `seg` id contract.
    pub fn update_segment_text(&self, seg: i64, text: &str) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription_segment SET text = ?1 WHERE id = ?2",
            params![text, seg],
        )
        .map_err(query_error)?;
        Ok(())
    }

    /// Updates one segment row's `translation` in place. See
    /// [`Self::update_segment_times`] for the `seg` id contract.
    pub fn update_segment_translation(
        &self,
        seg: i64,
        translation: &str,
    ) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription_segment SET translation = ?1 WHERE id = ?2",
            params![translation, seg],
        )
        .map_err(query_error)?;
        Ok(())
    }

    /// Replaces every `transcription_segment` row for `id` with `segs`, in
    /// one transaction (`DELETE` then re-`INSERT`): the resize/merge
    /// in-place save path (F-086). Row ids are freshly issued (the old ids
    /// are gone), and translations are dropped — Buzz parity, since resize/
    /// merge output never carries translations.
    pub fn replace_segments(
        &self,
        id: TranscriptionId,
        segs: &[Segment],
    ) -> Result<(), StoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(query_error)?;
        tx.execute(
            "DELETE FROM transcription_segment WHERE transcription_id = ?1",
            params![id.to_string()],
        )
        .map_err(query_error)?;
        for seg in segs {
            tx.execute(
                "INSERT INTO transcription_segment \
                 (start_time, end_time, text, translation, transcription_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![seg.start.0, seg.end.0, seg.text, "", id.to_string()],
            )
            .map_err(query_error)?;
        }
        tx.commit().map_err(query_error)?;
        Ok(())
    }

    /// Copies `id`'s `transcription` row to a freshly-minted id: the
    /// resize/merge "create new transcript" save path (F-086). Ported from
    /// Buzz's `TranscriptionDAO.copy_transcription`
    /// (`transcription_dao.py:107-160`): the new row gets a new
    /// [`TranscriptionId`], `time_queued = now`, `status = queued`, and
    /// copies every other Buzz column (`file`/`url`/`language`/`model_type`/
    /// `source`/`task`/`whisper_model_size`/`hugging_face_model_id`/
    /// `word_level_timings`/`extract_speech`/`name`/`notes`/
    /// `export_formats`/`output_folder`) plus humm's `options_json`/`kind`
    /// verbatim. `error_message`/`time_started`/`time_ended` are left at
    /// their column defaults (`NULL`) and `progress` at its default (`0.0`,
    /// "untouched" — a caller writes it explicitly via
    /// [`Self::update_progress_direct`] if it wants the row visible mid-run)
    /// — Buzz's own `INSERT` omits those columns too, so they never carry
    /// over from the source row. `last_export_folder` is reset to `NULL`
    /// (it's per-row state a fresh copy shouldn't inherit). No segments are
    /// copied — `transcription_segment` rows key off the old id and are
    /// never touched by this method.
    ///
    /// Errs if `id` doesn't exist (Buzz raises `"Transcription not found"`).
    pub fn copy_transcription(&self, id: TranscriptionId) -> Result<TranscriptionId, StoreError> {
        let new_id = TranscriptionId::new();
        let now = now_iso();
        let conn = self.lock()?;
        let affected = conn
            .execute(
                "INSERT INTO transcription (
                    id, export_formats, file, output_folder, language, model_type,
                    source, status, task, time_queued, url, whisper_model_size,
                    hugging_face_model_id, word_level_timings, extract_speech,
                    name, notes, options_json, kind, last_export_folder
                )
                SELECT
                    ?1, export_formats, file, output_folder, language, model_type,
                    source, ?2, task, ?3, url, whisper_model_size,
                    hugging_face_model_id, word_level_timings, extract_speech,
                    name, notes, options_json, kind, NULL
                FROM transcription WHERE id = ?4",
                params![
                    new_id.to_string(),
                    Status::Queued.as_str(),
                    now,
                    id.to_string(),
                ],
            )
            .map_err(query_error)?;
        if affected == 0 {
            return Err(StoreError::Query {
                detail: format!("copy_transcription: no task with id {id}"),
            });
        }
        Ok(new_id)
    }

    /// Marks `id` completed (status + `time_ended = now`) and inserts
    /// `segs` — the resize/merge "create new transcript" save path's second
    /// half, run after [`Self::copy_transcription`] on the new id (F-086).
    /// Translations are dropped on insert (Buzz parity, matching
    /// [`Self::replace_segments`]). One transaction.
    pub fn mark_replaced_completed(
        &self,
        id: TranscriptionId,
        segs: &[Segment],
    ) -> Result<(), StoreError> {
        let now = now_iso();
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(query_error)?;
        tx.execute(
            "UPDATE transcription SET status = ?1, time_ended = ?2 WHERE id = ?3",
            params![Status::Completed.as_str(), now, id.to_string()],
        )
        .map_err(query_error)?;
        for seg in segs {
            tx.execute(
                "INSERT INTO transcription_segment \
                 (start_time, end_time, text, translation, transcription_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![seg.start.0, seg.end.0, seg.text, "", id.to_string()],
            )
            .map_err(query_error)?;
        }
        tx.commit().map_err(query_error)?;
        Ok(())
    }

    /// Writes `fraction` (clamped `0.0..=1.0`) to `id`'s `progress` column
    /// directly, without touching `status`. Distinct from
    /// `TaskStore::update_progress` (which also flips status to
    /// `in_progress`, a queue-owned transition per ADR-018): this is the
    /// resize/merge worker's "make the row visible while it runs" path for
    /// a row that never enters the queue.
    pub fn update_progress_direct(
        &self,
        id: TranscriptionId,
        fraction: f64,
    ) -> Result<(), StoreError> {
        let clamped = fraction.clamp(0.0, 1.0);
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET progress = ?1 WHERE id = ?2",
            params![clamped, id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
    }

    /// Sets `id`'s last-used export folder (UX_SPEC J-6): the export menu's
    /// default save directory next time this transcript is exported.
    pub fn set_last_export_folder(
        &self,
        id: TranscriptionId,
        dir: &Path,
    ) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET last_export_folder = ?1 WHERE id = ?2",
            params![dir.display().to_string(), id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
    }

    /// Reads `id`'s last-used export folder. `Ok(None)` both when `id`
    /// doesn't exist and when no folder has been set yet — callers fall
    /// back to the input file's directory either way.
    pub fn last_export_folder(&self, id: TranscriptionId) -> Result<Option<PathBuf>, StoreError> {
        let conn = self.lock()?;
        let value: Option<String> = conn
            .query_row(
                "SELECT last_export_folder FROM transcription WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(query_error)?
            .flatten();
        Ok(value.map(PathBuf::from))
    }

    // -- M5 Task 7: live-session persistence (Q-10 ✚) ----------------------
    //
    // `docs/superpowers/specs/2026-07-23-m5-live-recording-design.md` §4:
    // Buzz persists nothing for a live recording; this crate makes a live
    // session a first-class row instead — insert-at-start
    // ([`Self::insert_live_session`]), stream-segments
    // ([`Self::append_live_segment`]), complete-at-stop
    // ([`Self::complete_live_session`]). These three (plus the stale-flip
    // special case in [`Self::cancel_stale_tasks`]) are called from the
    // GUI's live-session start/stop path, **not** the task queue:
    // `docs/design/ARCHITECTURE.md` §5 makes the queue's status transitions
    // the only writers of `status` for queue-executed tasks, but a live
    // session never enters the queue, so this family is the row's only
    // writer for its whole lifetime instead.

    /// Inserts a new live-recording session row: `Source::LiveRecording`,
    /// `Status::InProgress`, `time_queued = now`, `file`/`url` both `NULL`
    /// (a live session has neither — its audio is never a task input file),
    /// and `kind = "live_recording"` (the stale-flip special case in
    /// [`Self::cancel_stale_tasks`] keys off this column). `options_json`
    /// holds the full persisted [`TaskSpec`] — `options` cloned verbatim,
    /// `input` a never-read-back placeholder (`live_session_input_placeholder`),
    /// `output` default — so [`Self::load_task_spec`] round-trips a live row
    /// exactly like a file row's.
    ///
    /// `name` is stored **verbatim**: this method does no formatting of its
    /// own. The spec's naming rule (`"Live recording <%d-%b-%Y %H-%M-%S>"`)
    /// is the GUI session-start path's responsibility.
    pub fn insert_live_session(
        &self,
        name: &str,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionId, StoreError> {
        let id = TranscriptionId::new();
        let now = now_iso();
        let spec = TaskSpec {
            input: live_session_input_placeholder(),
            options: options.clone(),
            output: OutputSpec::default(),
        };
        let options_json = serde_json::to_string(&spec).map_err(|err| StoreError::Query {
            detail: format!("serializing live session options for {id}: {err}"),
        })?;
        let language = options.language.map(|lang| lang.code().to_string());
        let model_type = if options.model.hf_repo.is_some() {
            "Hugging Face"
        } else {
            "Whisper.cpp"
        };

        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO transcription (
                id, file, url, language, model_type, source, status, task,
                time_queued, whisper_model_size, hugging_face_model_id,
                word_level_timings, extract_speech, name, options_json, kind
            ) VALUES (
                ?1, NULL, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
            )",
            params![
                id.to_string(),
                language,
                model_type,
                Source::LiveRecording.as_str(),
                Status::InProgress.as_str(),
                options.task.as_str(),
                now,
                options.model.size.as_str(),
                options.model.hf_repo,
                options.word_timestamps,
                // No `TranscriptionOptions` field carries this (see
                // `create_task`'s identical comment) — always false.
                false,
                name,
                options_json,
                Source::LiveRecording.as_str(),
            ],
        )
        .map_err(query_error)?;
        Ok(id)
    }

    /// Appends one segment row for a live session — the stream-segments
    /// half of Q-10 ✚: called once per accepted utterance while the session
    /// runs, so a killed process loses at most the in-flight chunk.
    /// `translation` is always empty at insert time; live translation
    /// (M6, F-063) fills it in later via [`Self::update_segment_translation`],
    /// which is why this returns the inserted row's id
    /// ([`rusqlite::Connection::last_insert_rowid`] — the
    /// `transcription_segment.id` [`Self::update_segment_translation`] and
    /// [`SegmentRow::id`] both address by): the caller enqueues the raw
    /// utterance for translation tagged with this id, then persists the
    /// translation against it once it arrives.
    ///
    /// Errs if `id` doesn't exist, or if the row is already
    /// [`Status::Completed`] ([`Self::complete_live_session`] already ran):
    /// a correctly-behaving session never appends after completing, so this
    /// is treated as a programming error rather than silently ignored or
    /// silently accepted, per the pinned behavior in the M5 Task 7 brief.
    ///
    /// The status check and the segment insert are race-free against a
    /// concurrent call on another thread — not by a transaction, but because
    /// `Self::lock` holds `SqliteStore`'s single `Mutex<Connection>` guard
    /// across both statements for the whole method body; no other
    /// `SqliteStore` method can run against this connection in between. This
    /// invariant depends on `SqliteStore` staying one shared connection
    /// behind one mutex — a future move to per-call connections or a
    /// connection pool would need to re-establish atomicity here explicitly
    /// (e.g. an actual `BEGIN`/`COMMIT` transaction) rather than relying on
    /// the lock.
    pub fn append_live_segment(
        &self,
        id: TranscriptionId,
        start: Millis,
        end: Millis,
        text: &str,
    ) -> Result<i64, StoreError> {
        let conn = self.lock()?;
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM transcription WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(query_error)?;
        let Some(status) = status else {
            return Err(StoreError::Query {
                detail: format!("append_live_segment: no session with id {id}"),
            });
        };
        if status == Status::Completed.as_str() {
            return Err(StoreError::Query {
                detail: format!(
                    "append_live_segment: session {id} is already completed \
                     (status={status}) — a live session must not append a \
                     segment after complete_live_session has run"
                ),
            });
        }

        conn.execute(
            "INSERT INTO transcription_segment \
             (start_time, end_time, text, translation, transcription_id) \
             VALUES (?1, ?2, ?3, '', ?4)",
            params![start.0, end.0, text, id.to_string()],
        )
        .map_err(query_error)?;
        Ok(conn.last_insert_rowid())
    }

    /// Marks a live session `Completed` (+ `time_ended = now`) — the
    /// complete-at-stop half of Q-10 ✚. Once this returns, the row opens in
    /// the M4 viewer via every existing open path (F-003/F-004 gates already
    /// admit `Completed`), and any further [`Self::append_live_segment`]
    /// call for `id` errs (see its doc comment).
    pub fn complete_live_session(&self, id: TranscriptionId) -> Result<(), StoreError> {
        let now = now_iso();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcription SET status = ?1, time_ended = ?2 WHERE id = ?3",
            params![Status::Completed.as_str(), now, id.to_string()],
        )
        .map_err(query_error)?;
        Ok(())
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

    fn requeue_task(&self, id: &TranscriptionId) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let affected = conn
            .execute(
                "UPDATE transcription SET status = ?1, progress = 0.0, time_started = NULL, \
                 time_ended = NULL, error_message = NULL WHERE id = ?2",
                params![Status::Queued.as_str(), id.to_string()],
            )
            .map_err(query_error)?;
        if affected == 0 {
            return Err(StoreError::Query {
                detail: format!("requeue_task: no task with id {id}"),
            });
        }
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

    fn mark_skipped(&self, id: &TranscriptionId, segments: &[Segment]) -> Result<(), StoreError> {
        let now = now_iso();
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(query_error)?;
        tx.execute(
            "UPDATE transcription SET status = ?1, time_ended = ?2 WHERE id = ?3",
            params![Status::Skipped.as_str(), now, id.to_string()],
        )
        .map_err(query_error)?;
        for segment in segments {
            // The translation column is the literal empty string, not
            // `segment.translation` — Buzz hardcodes `translation=''` here
            // (`transcription_service.py:62-73`) regardless of what the
            // segment itself carries.
            tx.execute(
                "INSERT INTO transcription_segment \
                 (start_time, end_time, text, translation, transcription_id) \
                 VALUES (?1, ?2, ?3, '', ?4)",
                params![segment.start.0, segment.end.0, segment.text, id.to_string()],
            )
            .map_err(query_error)?;
        }
        tx.commit().map_err(query_error)?;
        Ok(())
    }

    fn find_completed_by_file_name(
        &self,
        name: &str,
    ) -> Result<Option<TranscriptionId>, StoreError> {
        // Delegates to the distinctly-named inherent method above. Naming
        // this differently from the trait method (rather than relying on
        // Rust's inherent-method-priority rule to disambiguate a same-named
        // pair) means renaming or inlining the inherent method can never
        // silently turn this into infinite recursion.
        self.find_completed_transcription_by_basename(name)
    }

    fn segments_for(&self, id: &TranscriptionId) -> Result<Vec<Segment>, StoreError> {
        self.get_segments(id)
    }

    fn set_task_notes(&self, id: &TranscriptionId, notes: &str) -> Result<(), StoreError> {
        // Rust prefers the inherent method of the same name over this trait
        // method when resolving a `self.set_task_notes(...)` call, so this
        // reaches the inherent method above, not itself -- see the trait's
        // own doc comment (`boundary.rs`) for why that's safe here, unlike
        // `find_completed_by_file_name`/`segments_for` just above.
        self.set_task_notes(id, notes)
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
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                word_timestamps: false,
                initial_prompt: String::new(),
                temperature: DEFAULT_TEMPERATURE.to_vec(),
                llm: None,
            },
            output: OutputSpec {
                formats: [OutputFormat::Txt, OutputFormat::Srt].into_iter().collect(),
                directory: Some(PathBuf::from("/tmp/out")),
                file_name_template: None,
                paragraph_split: Millis(2000),
                source_disposition: None,
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
        last_export_folder: Option<String>,
    }

    fn fetch_full_row(store: &SqliteStore, id: &TranscriptionId) -> FullRow {
        let conn = store.conn.lock().expect("lock");
        conn.query_row(
            "SELECT error_message, export_formats, file, output_folder, progress, language, \
             model_type, source, status, task, time_ended, time_queued, time_started, url, \
             whisper_model_size, hugging_face_model_id, word_level_timings, extract_speech, \
             name, notes, options_json, kind, last_export_folder FROM transcription \
             WHERE id = ?1",
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
                    last_export_folder: row.get(22)?,
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

    /// Guards F-031's linchpin: `mark_completed`'s segment `INSERT` writes the
    /// `translation` column, so a file task that produced per-segment LLM
    /// translations (design spec §6) persists them — not just the transcript.
    /// The column was always written (this is a regression guard, not a fix),
    /// but nothing previously asserted a *non-empty* translation round-trips.
    #[test]
    fn mark_completed_persists_segment_translations() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        let segments = vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "uno".to_string(),
                translation: "one".to_string(),
            },
            Segment {
                start: Millis(1000),
                end: Millis(2000),
                text: "dos".to_string(),
                // A segment whose translation failed/was skipped stays "" —
                // this must round-trip as empty, not be conflated with the
                // translated row above.
                translation: String::new(),
            },
        ];
        store.mark_completed(&id, &segments).expect("completed");

        let stored = store.get_segments(&id).expect("segments");
        assert_eq!(stored, segments);
        assert_eq!(stored[0].translation, "one");
        assert_eq!(stored[1].translation, "");
    }

    #[test]
    fn mark_skipped_sets_status_time_and_segments() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        let segments = vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "uno".to_string(),
                // Even a segment carrying a translation must be persisted
                // with an empty one: Buzz hardcodes `translation=''` for
                // skipped rows regardless of what the segment itself holds
                // (`transcription_service.py:62-73`).
                translation: "one".to_string(),
            },
            Segment {
                start: Millis(1000),
                end: Millis(2000),
                text: "dos".to_string(),
                translation: String::new(),
            },
        ];
        store.mark_skipped(&id, &segments).expect("skipped");

        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.status, Status::Skipped);
        assert!(row.time_ended.is_some());

        let stored = store.get_segments(&id).expect("segments");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].text, "uno");
        assert_eq!(stored[0].translation, "");
        assert_eq!(stored[1].text, "dos");
        assert_eq!(stored[1].translation, "");
    }

    #[test]
    fn find_completed_transcription_by_basename_matches_basename_newest_first() {
        let store = SqliteStore::open_in_memory().expect("open");

        let mut spec_a = sample_task_spec();
        spec_a.input = Input::Path(PathBuf::from("/dir/a/x.mp3"));
        let queued_earlier_finishes_later = TranscriptionId::new();
        store
            .create_task(&queued_earlier_finishes_later, &spec_a, Source::FileImport)
            .expect("create queued_earlier_finishes_later");
        store
            .mark_completed(&queued_earlier_finishes_later, &[])
            .expect("complete queued_earlier_finishes_later");

        let mut spec_b = sample_task_spec();
        spec_b.input = Input::Path(PathBuf::from("/dir/b/x.mp3"));
        let queued_later_finishes_earlier = TranscriptionId::new();
        store
            .create_task(&queued_later_finishes_earlier, &spec_b, Source::FileImport)
            .expect("create queued_later_finishes_earlier");
        store
            .mark_completed(&queued_later_finishes_earlier, &[])
            .expect("complete queued_later_finishes_earlier");

        // Force a deterministic ordering independent of wall-clock
        // resolution, and deliberately *invert* queued-vs-ended order
        // between the two rows: `queued_earlier_finishes_later` is queued
        // first but finishes last; `queued_later_finishes_earlier` is queued
        // second but finishes first. Buzz orders this lookup by `time_ended`
        // (`transcription_dao.py:336-351`), not `time_queued` — this makes
        // the test fail if the query orders by the wrong column.
        {
            let conn = store.conn.lock().expect("lock");
            conn.execute(
                "UPDATE transcription SET time_queued = '2020-01-01T00:00:00', \
                 time_ended = '2022-01-01T00:00:00' WHERE id = ?1",
                params![queued_earlier_finishes_later.to_string()],
            )
            .expect("update queued_earlier_finishes_later");
            conn.execute(
                "UPDATE transcription SET time_queued = '2021-01-01T00:00:00', \
                 time_ended = '2020-06-01T00:00:00' WHERE id = ?1",
                params![queued_later_finishes_earlier.to_string()],
            )
            .expect("update queued_later_finishes_earlier");
        }

        let found = store
            .find_completed_transcription_by_basename("x.mp3")
            .expect("query")
            .expect("found");
        assert_eq!(found, queued_earlier_finishes_later);
    }

    #[test]
    fn find_completed_ignores_non_completed() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut spec = sample_task_spec();
        spec.input = Input::Path(PathBuf::from("/dir/a/x.mp3"));
        let id = TranscriptionId::new();
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        // Still `queued` — not a match.
        assert_eq!(
            store
                .find_completed_transcription_by_basename("x.mp3")
                .expect("query"),
            None
        );

        // `failed` — still not a match.
        store.mark_failed(&id, "boom").expect("fail");
        assert_eq!(
            store
                .find_completed_transcription_by_basename("x.mp3")
                .expect("query"),
            None
        );
    }

    #[test]
    fn has_task_for_file_exact_path_only() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut spec = sample_task_spec();
        spec.input = Input::Path(PathBuf::from("/dir/a/x.mp3"));
        let id = TranscriptionId::new();
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        assert!(
            store
                .has_task_for_file(Path::new("/dir/a/x.mp3"))
                .expect("exact match")
        );
        assert!(
            !store
                .has_task_for_file(Path::new("/dir/b/x.mp3"))
                .expect("different dir doesn't match")
        );
        assert!(
            !store
                .has_task_for_file(Path::new("/dir/a/y.mp3"))
                .expect("different name doesn't match")
        );

        // "Any status" — still true once the task is terminal, not just
        // while queued.
        store.mark_failed(&id, "boom").expect("fail");
        assert!(
            store
                .has_task_for_file(Path::new("/dir/a/x.mp3"))
                .expect("still matches after status change")
        );
    }

    /// End-to-end proof that `humm_core::postproc::skip::check`'s database
    /// check (F-115) works against a *real* `SqliteStore` reached purely
    /// through the `TaskStore` trait object (exercising the
    /// `find_completed_by_file_name`/`segments_for` trait methods added for
    /// it, not just their already-tested inherent counterparts): a
    /// completed task with a different directory but the same basename is
    /// found, and its segments come back with translations forced to `""`
    /// even though the stored row carries a real one.
    #[test]
    fn skip_check_db_hit_drops_translations_through_task_store_trait() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut spec = sample_task_spec();
        spec.input = Input::Path(PathBuf::from("/dir/a/clip.mp3"));
        let id = TranscriptionId::new();
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        store
            .mark_completed(
                &id,
                &[
                    Segment {
                        start: Millis(0),
                        end: Millis(1000),
                        text: "uno".to_string(),
                        translation: "one".to_string(),
                    },
                    Segment {
                        start: Millis(1000),
                        end: Millis(2000),
                        text: "dos".to_string(),
                        translation: String::new(),
                    },
                ],
            )
            .expect("complete");

        let settings = humm_core::settings::SkipSettings {
            enabled: true,
            check_result_files: false,
            check_database: true,
        };
        // A different directory, same basename — proves the match is by
        // basename, not exact path (Buzz parity).
        let found = humm_core::postproc::skip::check(
            &settings,
            Path::new("/some/other/dir/clip.mp3"),
            None,
            &store,
        )
        .expect("db hit expected");

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].text, "uno");
        assert_eq!(found[0].translation, "");
        assert_eq!(found[1].text, "dos");
        assert_eq!(found[1].translation, "");
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

    #[test]
    fn augment_with_detach_failure_appends_detach_detail_to_the_import_error() {
        let err = StoreError::Query {
            detail: "no such table: buzz.transcription".to_string(),
        };
        let detach_err = StoreError::Query {
            detail: "database buzz is locked".to_string(),
        };

        let augmented = augment_with_detach_failure(err, detach_err);
        let StoreError::Query { detail } = augmented else {
            panic!("expected Query variant, got {augmented:?}");
        };
        assert!(detail.contains("no such table: buzz.transcription"));
        assert!(detail.contains("DETACH DATABASE buzz failed"));
        assert!(detail.contains("database buzz is locked"));
        assert!(detail.contains("may still be attached"));
    }

    #[test]
    fn augment_with_detach_failure_preserves_open_variant_and_path() {
        let err = StoreError::Open {
            path: PathBuf::from("/tmp/humm.sqlite"),
            detail: "disk full".to_string(),
        };
        let detach_err = StoreError::Corrupt {
            detail: "malformed database schema".to_string(),
        };

        let augmented = augment_with_detach_failure(err, detach_err);
        let StoreError::Open { path, detail } = augmented else {
            panic!("expected Open variant, got {augmented:?}");
        };
        assert_eq!(path, PathBuf::from("/tmp/humm.sqlite"));
        assert!(detail.starts_with("disk full"));
        assert!(detail.contains("malformed database schema"));
    }

    // -- Task 3: retry/rename/notes/delete/spec_from_columns (F-026) --------

    #[test]
    fn requeue_task_resets_status_progress_and_clears_timestamps_and_error_only() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_task_spec();
        spec.output.directory = Some(PathBuf::from("/tmp/out"));
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        store.mark_started(&id).expect("started");
        store.update_progress(&id, 0.75).expect("progress");
        store.mark_failed(&id, "boom").expect("failed");

        let before = fetch_full_row(&store, &id);
        assert_eq!(before.status.as_deref(), Some("failed"));
        assert_eq!(before.progress, 0.75);
        assert!(before.time_started.is_some());
        assert!(before.time_ended.is_some());
        assert_eq!(before.error_message.as_deref(), Some("boom"));

        store.requeue_task(&id).expect("requeue");

        let after = fetch_full_row(&store, &id);
        assert_eq!(after.status.as_deref(), Some("queued"));
        assert_eq!(after.progress, 0.0);
        assert_eq!(after.time_started, None);
        assert_eq!(after.time_ended, None);
        assert_eq!(after.error_message, None);

        // Every other column is untouched by requeue.
        assert_eq!(after.export_formats, before.export_formats);
        assert_eq!(after.file, before.file);
        assert_eq!(after.output_folder, before.output_folder);
        assert_eq!(after.language, before.language);
        assert_eq!(after.model_type, before.model_type);
        assert_eq!(after.source, before.source);
        assert_eq!(after.task, before.task);
        assert_eq!(after.time_queued, before.time_queued);
        assert_eq!(after.url, before.url);
        assert_eq!(after.whisper_model_size, before.whisper_model_size);
        assert_eq!(after.hugging_face_model_id, before.hugging_face_model_id);
        assert_eq!(after.word_level_timings, before.word_level_timings);
        assert_eq!(after.extract_speech, before.extract_speech);
        assert_eq!(after.name, before.name);
        assert_eq!(after.notes, before.notes);
        assert_eq!(after.options_json, before.options_json);
        assert_eq!(after.kind, before.kind);
    }

    #[test]
    fn requeue_task_on_missing_id_errors() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let err = store.requeue_task(&id).expect_err("missing id must error");
        assert!(matches!(err, StoreError::Query { .. }));
    }

    #[test]
    fn set_task_name_updates_name_and_errors_on_missing_id() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        store.set_task_name(&id, "My Recording").expect("set name");
        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.name.as_deref(), Some("My Recording"));

        let missing = TranscriptionId::new();
        let err = store
            .set_task_name(&missing, "x")
            .expect_err("missing id must error");
        assert!(matches!(err, StoreError::Query { .. }));
    }

    #[test]
    fn set_task_notes_updates_notes_and_errors_on_missing_id() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        store
            .set_task_notes(&id, "remember to trim the intro")
            .expect("set notes");
        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.notes.as_deref(), Some("remember to trim the intro"));

        let missing = TranscriptionId::new();
        let err = store
            .set_task_notes(&missing, "x")
            .expect_err("missing id must error");
        assert!(matches!(err, StoreError::Query { .. }));
    }

    #[test]
    fn delete_tasks_removes_rows_and_cascades_segments() {
        let store = SqliteStore::open_in_memory().expect("open");
        let a = TranscriptionId::new();
        let b = TranscriptionId::new();
        store
            .create_task(&a, &sample_task_spec(), Source::FileImport)
            .expect("create a");
        store
            .create_task(&b, &sample_task_spec(), Source::FileImport)
            .expect("create b");
        let segments = vec![Segment {
            start: Millis(0),
            end: Millis(1000),
            text: "hi".to_string(),
            translation: String::new(),
        }];
        store.mark_completed(&a, &segments).expect("complete a");
        store.mark_completed(&b, &segments).expect("complete b");
        assert_eq!(store.get_segments(&a).expect("segments a").len(), 1);

        let deleted = store.delete_tasks(&[a, b]).expect("delete");
        assert_eq!(deleted, 2);

        assert_eq!(store.get_task(&a).expect("get a"), None);
        assert_eq!(store.get_task(&b).expect("get b"), None);
        assert!(
            store
                .get_segments(&a)
                .expect("segments a after delete")
                .is_empty()
        );
        assert!(
            store
                .get_segments(&b)
                .expect("segments b after delete")
                .is_empty()
        );
    }

    #[test]
    fn delete_tasks_of_empty_slice_is_a_noop_returning_zero() {
        let store = SqliteStore::open_in_memory().expect("open");
        assert_eq!(store.delete_tasks(&[]).expect("delete empty"), 0);
    }

    #[test]
    fn delete_tasks_skips_unknown_ids_without_erroring() {
        let store = SqliteStore::open_in_memory().expect("open");
        let a = TranscriptionId::new();
        store
            .create_task(&a, &sample_task_spec(), Source::FileImport)
            .expect("create a");
        let unknown = TranscriptionId::new();

        let deleted = store.delete_tasks(&[a, unknown]).expect("delete");
        assert_eq!(deleted, 1);
        assert_eq!(store.get_task(&a).expect("get a"), None);
    }

    #[test]
    fn spec_from_columns_maps_buzz_shaped_row_with_fallbacks() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        {
            let conn = store.conn.lock().expect("lock");
            conn.execute(
                "INSERT INTO transcription (
                    id, file, url, language, task, whisper_model_size,
                    hugging_face_model_id, word_level_timings, export_formats,
                    output_folder, status, time_queued, kind
                ) VALUES (
                    ?1, ?2, NULL, ?3, ?4, ?5, NULL, ?6, ?7, ?8, 'failed', ?9, 'file'
                )",
                params![
                    id.to_string(),
                    "/buzz/audio1.mp3",
                    "not-a-real-language",
                    "not-a-real-task",
                    "not-a-real-size",
                    true,
                    "txt, bogus, srt",
                    "/buzz/out",
                    "2024-01-01T00:00:00.000000",
                ],
            )
            .expect("insert buzz-shaped row");
        }

        let spec = store
            .spec_from_columns(&id)
            .expect("spec_from_columns")
            .expect("row present");

        assert_eq!(spec.input, Input::Path(PathBuf::from("/buzz/audio1.mp3")));
        assert_eq!(spec.options.language, None);
        assert_eq!(spec.options.task, TaskKind::Transcribe);
        // Buzz's own restart fallback for an unparseable model size is
        // `Tiny`, not this crate's `TranscriptionOptions::default()` `Small`.
        assert_eq!(spec.options.model.size, ModelSize::Tiny);
        assert_eq!(spec.options.model.hf_repo, None);
        assert!(spec.options.word_timestamps);
        assert_eq!(spec.options.initial_prompt, "");
        assert_eq!(spec.options.temperature, DEFAULT_TEMPERATURE.to_vec());
        assert_eq!(
            spec.output.formats,
            [OutputFormat::Txt, OutputFormat::Srt].into_iter().collect()
        );
        assert_eq!(spec.output.directory, Some(PathBuf::from("/buzz/out")));
    }

    #[test]
    fn spec_from_columns_prefers_url_over_file_and_returns_none_for_missing_row() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_task_spec();
        spec.input = Input::Url(url::Url::parse("https://example.com/a.mp3").expect("url"));
        store
            .create_task(&id, &spec, Source::UrlImport)
            .expect("create");

        let rebuilt = store
            .spec_from_columns(&id)
            .expect("spec_from_columns")
            .expect("row present");
        assert_eq!(
            rebuilt.input,
            Input::Url(url::Url::parse("https://example.com/a.mp3").expect("url"))
        );

        assert_eq!(
            store
                .spec_from_columns(&TranscriptionId::new())
                .expect("missing"),
            None
        );
    }

    #[test]
    fn model_display_columns_reads_raw_model_type_and_size_and_none_for_missing_row() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let mut spec = sample_task_spec();
        spec.options.model.size = ModelSize::Small;
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");

        let (model_type, size) = store
            .model_display_columns(&id)
            .expect("model_display_columns")
            .expect("row present");
        assert_eq!(model_type.as_deref(), Some("Whisper.cpp"));
        assert_eq!(size.as_deref(), Some("small"));

        assert_eq!(
            store
                .model_display_columns(&TranscriptionId::new())
                .expect("missing"),
            None
        );
    }

    #[test]
    fn model_display_columns_is_none_for_null_columns_on_a_foreign_row() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        {
            let conn = store.conn.lock().expect("lock");
            conn.execute(
                "INSERT INTO transcription (id, status, time_queued, kind) \
                 VALUES (?1, 'completed', ?2, 'file')",
                params![id.to_string(), "2020-01-01T00:00:00"],
            )
            .expect("insert foreign row");
        }
        let (model_type, size) = store
            .model_display_columns(&id)
            .expect("model_display_columns")
            .expect("row present");
        assert_eq!(model_type, None);
        assert_eq!(size, None);
    }

    #[test]
    fn parse_loose_bool_treats_buzz_text_and_ints_as_truthy() {
        use rusqlite::types::Value;
        assert!(parse_loose_bool(&Value::Integer(1)));
        assert!(!parse_loose_bool(&Value::Integer(0)));
        assert!(parse_loose_bool(&Value::Text("1".to_string())));
        assert!(parse_loose_bool(&Value::Text("true".to_string())));
        assert!(!parse_loose_bool(&Value::Text("0".to_string())));
        assert!(!parse_loose_bool(&Value::Null));
    }

    // -- Task 1 (M4): schema v2 + segment/copy APIs (F-081, F-084, F-086) ---

    #[test]
    fn migration_v1_to_v2_adds_last_export_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("humm.sqlite");
        let sample_id = TranscriptionId::new();

        // Build a v1 database exactly as a pre-M4 humm build would have left
        // it: v1 DDL (no `last_export_folder` column), user_version = 1, one
        // row with data that must survive the migration untouched.
        {
            let conn = Connection::open(&db_path).expect("open raw v1 db");
            conn.execute_batch(
                "CREATE TABLE transcription (
                    id TEXT PRIMARY KEY, error_message TEXT, export_formats TEXT, file TEXT,
                    output_folder TEXT, progress DOUBLE PRECISION DEFAULT 0.0, language TEXT,
                    model_type TEXT, source TEXT, status TEXT, task TEXT, time_ended TIMESTAMP,
                    time_queued TIMESTAMP NOT NULL, time_started TIMESTAMP, url TEXT,
                    whisper_model_size TEXT, hugging_face_model_id TEXT,
                    word_level_timings BOOLEAN DEFAULT FALSE, extract_speech BOOLEAN DEFAULT FALSE,
                    name TEXT, notes TEXT,
                    options_json TEXT,
                    kind TEXT NOT NULL DEFAULT 'file'
                );
                CREATE TABLE transcription_segment (
                    id INTEGER PRIMARY KEY, end_time INT DEFAULT 0, start_time INT DEFAULT 0,
                    text TEXT NOT NULL, translation TEXT DEFAULT '',
                    transcription_id TEXT,
                    FOREIGN KEY (transcription_id) REFERENCES transcription(id) ON DELETE CASCADE
                );
                CREATE TABLE humm_meta (key TEXT PRIMARY KEY, value TEXT);
                PRAGMA user_version = 1;",
            )
            .expect("apply v1 ddl");
            conn.execute(
                "INSERT INTO transcription (id, status, time_queued, kind, file) \
                 VALUES (?1, 'completed', ?2, 'file', ?3)",
                params![sample_id.to_string(), "2020-01-01T00:00:00", "/audio.wav"],
            )
            .expect("insert v1 row");
        }

        let store = SqliteStore::open(&db_path).expect("reopen migrates to v2");

        let conn = store.conn.lock().expect("lock");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user_version");
        assert_eq!(version, 2);

        let (file, last_export_folder): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT file, last_export_folder FROM transcription WHERE id = ?1",
                params![sample_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row intact after migration");
        assert_eq!(file.as_deref(), Some("/audio.wav"));
        assert_eq!(last_export_folder, None);
    }

    #[test]
    fn newer_schema_version_is_refused_with_actionable_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("humm.sqlite");
        {
            let conn = Connection::open(&db_path).expect("open raw db");
            conn.execute_batch("PRAGMA user_version = 99;")
                .expect("set future version");
        }

        let err = SqliteStore::open(&db_path)
            .err()
            .expect("newer schema must be refused");
        let message = err.to_string();
        assert!(
            message.to_lowercase().contains("newer"),
            "message should mention 'newer': {message}"
        );
        assert!(
            message.contains(&db_path.display().to_string()),
            "message should mention the db path: {message}"
        );
        assert!(matches!(
            err,
            StoreError::SchemaTooNew {
                found: 99,
                supported: 2,
                ..
            }
        ));
    }

    #[test]
    fn segment_rows_round_trip_with_ids() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");
        let segments = vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "a".to_string(),
                translation: "ta".to_string(),
            },
            Segment {
                start: Millis(1000),
                end: Millis(2000),
                text: "b".to_string(),
                translation: "tb".to_string(),
            },
            Segment {
                start: Millis(2000),
                end: Millis(3000),
                text: "c".to_string(),
                translation: "tc".to_string(),
            },
        ];
        store.mark_completed(&id, &segments).expect("complete");

        let rows = store.get_segment_rows(id).expect("get_segment_rows");
        assert_eq!(rows.len(), 3);
        assert!(rows[0].id < rows[1].id);
        assert!(rows[1].id < rows[2].id);
        for (row, seg) in rows.iter().zip(segments.iter()) {
            assert_eq!(row.start, seg.start);
            assert_eq!(row.end, seg.end);
            assert_eq!(row.text, seg.text);
            assert_eq!(row.translation, seg.translation);
        }
    }

    #[test]
    fn update_segment_times_text_translation_persist() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");
        let segments = vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "a".to_string(),
                translation: "ta".to_string(),
            },
            Segment {
                start: Millis(1000),
                end: Millis(2000),
                text: "b".to_string(),
                translation: "tb".to_string(),
            },
        ];
        store.mark_completed(&id, &segments).expect("complete");
        let rows = store.get_segment_rows(id).expect("rows");
        let first_id = rows[0].id;
        let second_id = rows[1].id;

        store
            .update_segment_times(first_id, Millis(50), Millis(1200))
            .expect("update times");
        let rows = store.get_segment_rows(id).expect("rows after times");
        assert_eq!(rows[0].start, Millis(50));
        assert_eq!(rows[0].end, Millis(1200));
        // The other row is untouched.
        assert_eq!(rows[1].start, Millis(1000));
        assert_eq!(rows[1].end, Millis(2000));

        store
            .update_segment_text(first_id, "a-edited")
            .expect("update text");
        let rows = store.get_segment_rows(id).expect("rows after text");
        assert_eq!(rows[0].text, "a-edited");
        assert_eq!(rows[1].text, "b");

        store
            .update_segment_translation(second_id, "tb-edited")
            .expect("update translation");
        let rows = store.get_segment_rows(id).expect("rows after translation");
        assert_eq!(rows[1].translation, "tb-edited");
        // First row's translation untouched by the second row's edit.
        assert_eq!(rows[0].translation, "ta");
    }

    #[test]
    fn replace_segments_swaps_all_rows_in_one_txn() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");
        let original = vec![Segment {
            start: Millis(0),
            end: Millis(1000),
            text: "a".to_string(),
            translation: "ta".to_string(),
        }];
        store.mark_completed(&id, &original).expect("complete");
        let old_ids: Vec<i64> = store
            .get_segment_rows(id)
            .expect("rows")
            .into_iter()
            .map(|r| r.id)
            .collect();

        // A decoy transcription's segment, inserted *after* the one above,
        // keeps `transcription_segment`'s max rowid above `old_ids` across
        // the delete below — otherwise SQLite's default rowid allocation
        // (one more than the table's current max) would hand the very next
        // insert the same id the delete just freed, making a freshly-issued
        // id collide with an old one by coincidence rather than by a real
        // bug.
        let decoy_id = TranscriptionId::new();
        store
            .create_task(&decoy_id, &sample_task_spec(), Source::FileImport)
            .expect("create decoy");
        store
            .mark_completed(
                &decoy_id,
                &[Segment {
                    start: Millis(0),
                    end: Millis(1000),
                    text: "decoy".to_string(),
                    translation: String::new(),
                }],
            )
            .expect("complete decoy");

        let replacement = vec![
            Segment {
                start: Millis(0),
                end: Millis(500),
                text: "x".to_string(),
                translation: "should-drop".to_string(),
            },
            Segment {
                start: Millis(500),
                end: Millis(900),
                text: "y".to_string(),
                translation: "should-drop-2".to_string(),
            },
        ];
        store.replace_segments(id, &replacement).expect("replace");

        let rows = store.get_segment_rows(id).expect("rows after replace");
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert!(!old_ids.contains(&row.id), "row id must be freshly issued");
            assert_eq!(row.translation, "", "translations are dropped on replace");
        }
        assert_eq!(rows[0].text, "x");
        assert_eq!(rows[0].start, Millis(0));
        assert_eq!(rows[0].end, Millis(500));
        assert_eq!(rows[1].text, "y");
        assert_eq!(rows[1].start, Millis(500));
        assert_eq!(rows[1].end, Millis(900));
    }

    #[test]
    fn copy_transcription_copies_row_with_new_id_queued_now() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        let spec = sample_task_spec();
        store
            .create_task(&id, &spec, Source::FileImport)
            .expect("create");
        store.set_task_name(&id, "My Recording").expect("name");
        store.set_task_notes(&id, "notes here").expect("notes");
        store.mark_started(&id).expect("start");
        store
            .mark_completed(
                &id,
                &[Segment {
                    start: Millis(0),
                    end: Millis(1000),
                    text: "hi".to_string(),
                    translation: String::new(),
                }],
            )
            .expect("complete");

        let new_id = store.copy_transcription(id).expect("copy");
        assert_ne!(new_id, id);

        let new_row = store.get_task(&new_id).expect("get new").expect("present");
        assert_eq!(new_row.status, Status::Queued);
        assert_eq!(new_row.name.as_deref(), Some("My Recording"));
        assert_eq!(new_row.notes.as_deref(), Some("notes here"));
        assert!(new_row.time_started.is_none());
        assert!(new_row.time_ended.is_none());
        assert_eq!(new_row.progress, 0.0);
        assert!(!new_row.time_queued.is_empty());

        let original_full = fetch_full_row(&store, &id);
        let new_full = fetch_full_row(&store, &new_id);
        assert_eq!(new_full.file, original_full.file);
        assert_eq!(new_full.options_json, original_full.options_json);
        assert_eq!(new_full.kind, original_full.kind);
        assert_eq!(new_full.last_export_folder, None);

        assert!(
            store.get_segments(&new_id).expect("segments").is_empty(),
            "copy_transcription must not copy segments"
        );
    }

    #[test]
    fn mark_replaced_completed_sets_status_and_inserts() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        let segments = vec![Segment {
            start: Millis(0),
            end: Millis(500),
            text: "one".to_string(),
            translation: "should-drop".to_string(),
        }];
        store
            .mark_replaced_completed(id, &segments)
            .expect("mark replaced completed");

        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.status, Status::Completed);
        assert!(row.time_ended.is_some());

        let stored = store.get_segments(&id).expect("segments");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].text, "one");
        assert_eq!(stored[0].translation, "", "translations are dropped");
    }

    #[test]
    fn update_progress_direct_writes_progress() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        store.update_progress_direct(id, 0.42).expect("progress");
        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.progress, 0.42);
        // Unlike `TaskStore::update_progress`, this does not touch status —
        // it's the resizer-visible-row path, not a queue transition.
        assert_eq!(row.status, Status::Queued);
    }

    #[test]
    fn last_export_folder_round_trip_and_default_none() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = TranscriptionId::new();
        store
            .create_task(&id, &sample_task_spec(), Source::FileImport)
            .expect("create");

        assert_eq!(store.last_export_folder(id).expect("read default"), None);

        store
            .set_last_export_folder(id, Path::new("/exports/out"))
            .expect("set");
        assert_eq!(
            store.last_export_folder(id).expect("read after set"),
            Some(PathBuf::from("/exports/out"))
        );
    }

    // -- Task 7 (M5): live-session persistence + stale-flip (Q-10, F-060) --

    fn sample_live_options() -> TranscriptionOptions {
        TranscriptionOptions {
            language: Language::parse("en"),
            task: TaskKind::Transcribe,
            model: ModelSpec {
                backend: Backend::Whisper,
                size: ModelSize::Small,
                hf_repo: None,
            },
            word_timestamps: false,
            initial_prompt: "hello there".to_string(),
            temperature: DEFAULT_TEMPERATURE.to_vec(),
            llm: None,
        }
    }

    #[test]
    fn insert_live_session_fills_row_per_contract_and_name_verbatim() {
        let store = SqliteStore::open_in_memory().expect("open");
        let options = sample_live_options();
        let name = "Live recording 23-Jul-2026 10-00-00";

        let id = store
            .insert_live_session(name, &options)
            .expect("insert live session");

        let row = fetch_full_row(&store, &id);
        assert_eq!(row.status.as_deref(), Some("in_progress"));
        assert_eq!(row.source.as_deref(), Some("live_recording"));
        assert_eq!(row.kind, "live_recording");
        assert_eq!(row.file, None);
        assert_eq!(row.url, None);
        assert_eq!(row.name.as_deref(), Some(name));
        assert!(!row.time_queued.is_empty());
        assert_eq!(row.time_started, None);
        assert_eq!(row.time_ended, None);
        assert_eq!(row.progress, 0.0);

        let task_row = store.get_task(&id).expect("get_task").expect("present");
        assert_eq!(task_row.status, Status::InProgress);
        assert_eq!(task_row.source, Some(Source::LiveRecording));
        assert_eq!(task_row.name.as_deref(), Some(name));
        assert_eq!(task_row.file, None);
        assert_eq!(task_row.url, None);
    }

    #[test]
    fn insert_live_session_options_json_survives_load_task_spec() {
        let store = SqliteStore::open_in_memory().expect("open");
        let mut options = sample_live_options();
        options.language = Language::parse("fr");
        options.task = TaskKind::Translate;
        options.model.size = ModelSize::Medium;

        let id = store
            .insert_live_session("Live recording 23-Jul-2026 10-00-00", &options)
            .expect("insert");

        let loaded = store.load_task_spec(&id).expect("load").expect("present");
        assert_eq!(loaded.options, options);
    }

    #[test]
    fn append_live_segment_appends_ordered_and_viewer_shaped() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = store
            .insert_live_session("Live recording", &sample_live_options())
            .expect("insert");

        store
            .append_live_segment(id, Millis(0), Millis(1000), "hello")
            .expect("append 1");
        store
            .append_live_segment(id, Millis(1000), Millis(2500), "world")
            .expect("append 2");

        let rows = store.get_segment_rows(id).expect("get_segment_rows");
        assert_eq!(rows.len(), 2);
        assert!(
            rows[0].id < rows[1].id,
            "segments come back in append order"
        );
        assert_eq!(rows[0].start, Millis(0));
        assert_eq!(rows[0].end, Millis(1000));
        assert_eq!(rows[0].text, "hello");
        assert_eq!(rows[0].translation, "");
        assert_eq!(rows[1].start, Millis(1000));
        assert_eq!(rows[1].end, Millis(2500));
        assert_eq!(rows[1].text, "world");
        assert_eq!(rows[1].translation, "");
    }

    #[test]
    fn append_live_segment_returns_ascending_rowids() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = store
            .insert_live_session("Live recording", &sample_live_options())
            .expect("insert");

        let r1 = store
            .append_live_segment(id, Millis(0), Millis(1000), "one")
            .expect("append 1");
        let r2 = store
            .append_live_segment(id, Millis(1000), Millis(2000), "two")
            .expect("append 2");
        let r3 = store
            .append_live_segment(id, Millis(2000), Millis(3000), "three")
            .expect("append 3");

        assert!(
            r1 < r2 && r2 < r3,
            "row ids must be strictly ascending: {r1} {r2} {r3}"
        );

        // The returned id is exactly the `transcription_segment.id`
        // `update_segment_translation` (F-063) later addresses — so
        // `get_segment_rows` (which selects that same column) round-trips it.
        let rows = store.get_segment_rows(id).expect("get_segment_rows");
        assert_eq!(rows[0].id, r1);
        assert_eq!(rows[1].id, r2);
        assert_eq!(rows[2].id, r3);
    }

    #[test]
    fn append_live_segment_missing_id_errors() {
        let store = SqliteStore::open_in_memory().expect("open");
        let missing = TranscriptionId::new();

        let err = store
            .append_live_segment(missing, Millis(0), Millis(100), "x")
            .expect_err("missing id must error");
        assert!(matches!(err, StoreError::Query { .. }));
    }

    #[test]
    fn complete_live_session_marks_completed_with_time_ended() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = store
            .insert_live_session("Live recording", &sample_live_options())
            .expect("insert");

        store.complete_live_session(id).expect("complete");

        let row = store.get_task(&id).expect("get").expect("present");
        assert_eq!(row.status, Status::Completed);
        assert!(row.time_ended.is_some());
    }

    #[test]
    fn append_live_segment_after_complete_errors_actionably() {
        let store = SqliteStore::open_in_memory().expect("open");
        let id = store
            .insert_live_session("Live recording", &sample_live_options())
            .expect("insert");
        store.complete_live_session(id).expect("complete");

        let err = store
            .append_live_segment(id, Millis(0), Millis(1000), "too late")
            .expect_err("append after complete must error");
        assert!(matches!(err, StoreError::Query { .. }));

        let message = err.to_string();
        assert!(
            message.contains(&id.to_string()),
            "message should name the session id: {message}"
        );
        assert!(
            message.to_lowercase().contains("complet"),
            "message should name the completed status: {message}"
        );

        assert!(
            store.get_segments(&id).expect("segments").is_empty(),
            "the rejected append must not have inserted a row"
        );
    }

    #[test]
    fn cancel_stale_tasks_flips_live_in_progress_to_completed_others_unchanged() {
        let store = SqliteStore::open_in_memory().expect("open");

        // Stale live in-progress row: must flip to Completed, not Canceled.
        let live_id = store
            .insert_live_session("Live recording stale", &sample_live_options())
            .expect("insert live");

        // Stale file in-progress row: today's exact behavior (Canceled).
        let file_in_progress_id = TranscriptionId::new();
        store
            .create_task(
                &file_in_progress_id,
                &sample_task_spec(),
                Source::FileImport,
            )
            .expect("create file in-progress");
        store.mark_started(&file_in_progress_id).expect("started");

        // Queued row: unaffected by the live special case, still Canceled.
        let queued_id = TranscriptionId::new();
        store
            .create_task(&queued_id, &sample_task_spec(), Source::FileImport)
            .expect("create queued");

        // Terminal rows (file and live) must be left untouched either way.
        let completed_id = TranscriptionId::new();
        store
            .create_task(&completed_id, &sample_task_spec(), Source::FileImport)
            .expect("create completed");
        store.mark_completed(&completed_id, &[]).expect("complete");

        let live_completed_id = store
            .insert_live_session("Live recording already done", &sample_live_options())
            .expect("insert live completed");
        store
            .complete_live_session(live_completed_id)
            .expect("complete live");
        let live_completed_time_ended_before = store
            .get_task(&live_completed_id)
            .expect("get")
            .expect("present")
            .time_ended;

        let flipped = store.cancel_stale_tasks().expect("cancel_stale");
        // live_id (in_progress -> completed) + file_in_progress_id and
        // queued_id (both -> canceled) = 3; the two terminal rows are
        // untouched and so not counted.
        assert_eq!(flipped, 3);

        let live_row = store.get_task(&live_id).expect("get").expect("present");
        assert_eq!(live_row.status, Status::Completed);
        assert!(live_row.time_ended.is_some());

        assert_eq!(
            store
                .get_task(&file_in_progress_id)
                .expect("get")
                .expect("present")
                .status,
            Status::Canceled
        );
        assert_eq!(
            store
                .get_task(&queued_id)
                .expect("get")
                .expect("present")
                .status,
            Status::Canceled
        );
        assert_eq!(
            store
                .get_task(&completed_id)
                .expect("get")
                .expect("present")
                .status,
            Status::Completed
        );

        let live_completed_row = store
            .get_task(&live_completed_id)
            .expect("get")
            .expect("present");
        assert_eq!(live_completed_row.status, Status::Completed);
        assert_eq!(
            live_completed_row.time_ended,
            live_completed_time_ended_before
        );
    }
}
