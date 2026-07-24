//! humm-store: the SQLite persistence adapter implementing
//! `humm_core::boundary::TaskStore` (ADR-004).
//!
//! Schema v2 is a Buzz-superset of `transcription`/`transcription_segment`
//! (`buzz/schema.sql`, verbatim columns) plus humm's `options_json` (the full
//! persisted [`humm_core::domain::TaskSpec`], fixing Buzz's
//! restart-loses-options bug), `kind` (admits live-session records, Q-10),
//! and `last_export_folder` (M4's transcript viewer, UX_SPEC J-6)
//! additions (`docs/design/ARCHITECTURE.md` §5). All access goes through
//! [`SqliteStore`]; no ORM or query builder — hand-written SQL, unit-tested.
#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod schema;
mod secrets;
mod store;

pub use secrets::KeyringSecretStore;
pub use store::{BuzzImportOutcome, ModelDisplayColumns, SegmentRow, SqliteStore, TaskRow};
