//! Schema v1 DDL and the migration that applies it.
//!
//! `transcription` and `transcription_segment` reproduce Buzz's
//! `buzz/schema.sql` verbatim (column-for-column), so a future one-time
//! `Buzz.sqlite` import (Q-6) can map rows directly. `options_json` and
//! `kind` are humm's own additions (`docs/design/ARCHITECTURE.md` §5).

use rusqlite::Connection;

use humm_core::error::StoreError;

/// The schema v1 DDL, applied once when `PRAGMA user_version` is `0`.
const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS transcription (
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
CREATE TABLE IF NOT EXISTS transcription_segment (
    id INTEGER PRIMARY KEY, end_time INT DEFAULT 0, start_time INT DEFAULT 0,
    text TEXT NOT NULL, translation TEXT DEFAULT '',
    transcription_id TEXT,
    FOREIGN KEY (transcription_id) REFERENCES transcription(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_transcription_id ON transcription_segment(transcription_id);
CREATE TABLE IF NOT EXISTS humm_meta (key TEXT PRIMARY KEY, value TEXT);
";

/// Migrates `conn` to schema v1 if needed.
///
/// Reads `PRAGMA user_version`: `0` (a fresh database) executes
/// [`SCHEMA_V1`] and sets the version to `1`; `1` (already migrated) is a
/// no-op. Any other value is rejected as [`StoreError::Corrupt`] naming the
/// version — a forward-compat guard against an older humm binary opening a
/// database written by a newer one.
pub(crate) fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|err| StoreError::Query {
            detail: format!("reading schema version: {err}"),
        })?;

    match version {
        0 => {
            conn.execute_batch(SCHEMA_V1)
                .map_err(|err| StoreError::Query {
                    detail: format!("applying schema v1: {err}"),
                })?;
            conn.execute_batch("PRAGMA user_version = 1;")
                .map_err(|err| StoreError::Query {
                    detail: format!("setting schema version: {err}"),
                })?;
            Ok(())
        }
        1 => Ok(()),
        other => Err(StoreError::Corrupt {
            detail: format!("unsupported schema version {other} (expected 0 or 1)"),
        }),
    }
}
