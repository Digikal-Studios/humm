//! Schema DDL and the migration that applies it.
//!
//! `transcription` and `transcription_segment` reproduce Buzz's
//! `buzz/schema.sql` verbatim (column-for-column), so a future one-time
//! `Buzz.sqlite` import (Q-6) can map rows directly. `options_json`, `kind`,
//! and `last_export_folder` are humm's own additions
//! (`docs/design/ARCHITECTURE.md` §5; `last_export_folder` per M4's
//! transcript viewer, UX_SPEC J-6).

use std::path::Path;

use rusqlite::Connection;

use humm_core::error::StoreError;

/// The current schema version this build understands. Bumped whenever
/// [`migrate`] gains a new arm.
const CURRENT_SCHEMA_VERSION: i64 = 2;

/// The schema v2 DDL, applied once (in full) when `PRAGMA user_version` is
/// `0` — i.e. a brand-new database skips straight to v2 rather than
/// replaying the v1-then-ALTER path a v1 database takes in [`migrate`].
const SCHEMA_V2: &str = "
CREATE TABLE IF NOT EXISTS transcription (
    id TEXT PRIMARY KEY, error_message TEXT, export_formats TEXT, file TEXT,
    output_folder TEXT, progress DOUBLE PRECISION DEFAULT 0.0, language TEXT,
    model_type TEXT, source TEXT, status TEXT, task TEXT, time_ended TIMESTAMP,
    time_queued TIMESTAMP NOT NULL, time_started TIMESTAMP, url TEXT,
    whisper_model_size TEXT, hugging_face_model_id TEXT,
    word_level_timings BOOLEAN DEFAULT FALSE, extract_speech BOOLEAN DEFAULT FALSE,
    name TEXT, notes TEXT,
    options_json TEXT,
    kind TEXT NOT NULL DEFAULT 'file',
    last_export_folder TEXT
);
CREATE TABLE IF NOT EXISTS transcription_segment (
    id INTEGER PRIMARY KEY, end_time INT DEFAULT 0, start_time INT DEFAULT 0,
    text TEXT NOT NULL, translation TEXT DEFAULT '',
    transcription_id TEXT,
    FOREIGN KEY (transcription_id) REFERENCES transcription(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_transcription_id ON transcription_segment(transcription_id);
CREATE TABLE IF NOT EXISTS humm_meta (key TEXT PRIMARY KEY, value TEXT);
";

/// Migrates `conn` (the database at `path`, used only to name it in an
/// error) to [`CURRENT_SCHEMA_VERSION`] if needed.
///
/// Reads `PRAGMA user_version`: `0` (a fresh database) executes the full
/// [`SCHEMA_V2`] DDL and sets the version to `2`; `1` (a pre-M4 database)
/// adds the `last_export_folder` column via `ALTER TABLE` and sets the
/// version to `2`; `2` (already current) is a no-op. Any higher value is
/// rejected as [`StoreError::SchemaTooNew`] — a forward-compat guard against
/// an older humm binary opening a database written by a newer one.
pub(crate) fn migrate(conn: &Connection, path: &Path) -> Result<(), StoreError> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|err| StoreError::Query {
            detail: format!("reading schema version: {err}"),
        })?;

    match version {
        0 => {
            conn.execute_batch(SCHEMA_V2)
                .map_err(|err| StoreError::Query {
                    detail: format!("applying schema v2: {err}"),
                })?;
            set_user_version(conn, CURRENT_SCHEMA_VERSION)
        }
        1 => {
            conn.execute_batch("ALTER TABLE transcription ADD COLUMN last_export_folder TEXT;")
                .map_err(|err| StoreError::Query {
                    detail: format!("migrating schema v1 to v2: {err}"),
                })?;
            set_user_version(conn, CURRENT_SCHEMA_VERSION)
        }
        2 => Ok(()),
        other => Err(StoreError::SchemaTooNew {
            found: other,
            supported: CURRENT_SCHEMA_VERSION,
            path: path.to_path_buf(),
        }),
    }
}

fn set_user_version(conn: &Connection, version: i64) -> Result<(), StoreError> {
    conn.execute_batch(&format!("PRAGMA user_version = {version};"))
        .map_err(|err| StoreError::Query {
            detail: format!("setting schema version: {err}"),
        })
}
