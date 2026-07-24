//! Live recording: the realtime transcription loop ported from Buzz's
//! `recording_transcriber.py` (F-060..F-065, `docs/superpowers/specs/
//! 2026-07-23-m5-live-recording-design.md`). This module tree holds pure
//! `humm-core` domain logic -- no threads, no audio APIs; adapters
//! (`humm-audio`, `humm-whisper`) and the GUI drive it. [`export`] is the one
//! exception to "no I/O": it *is* the live-export engine, so file writes and
//! blocking HTTP POSTs are its whole job (same "services, not adapters, stay
//! in core when the boundary is pure Rust" reasoning as `humm-core::update`'s
//! blocking `reqwest` call) -- `chunker`/`merge` remain strictly I/O-free.
//!
//! - [`chunker`] -- sample accumulation, silence-aware cut points, and the
//!   callback-drop queue policy (F-061 logic half).
//! - [`merge`] -- the append-and-correct text merge (`find_common_part`,
//!   `merge_text_no_overlap`, `MergeState`) and display-mode panel
//!   application (`PanelText`), F-062.
//! - [`export`] -- the live TXT/CSV export writers, filename templating,
//!   retry policy, and the upload-URL client (F-064, spec §2.4).
//! - [`session`] -- the recording-session orchestration loop: drains the
//!   capture ring into [`chunker`], decides skip-vs-transcribe with a
//!   rolling prompt, and emits [`session::LiveEvent`]s (F-061, spec §3).
//! - [`translation`] -- the live-translation consumer (`strip_newlines` +
//!   [`translation::TranslationApplier`]): the translation-side twin of the
//!   transcript-side `SessionApplier`, dispatching each translation into its
//!   own panel/merge and the `.translated` export sibling (F-063, spec §7.4).

pub mod chunker;
pub mod export;
pub mod merge;
pub mod session;
pub mod translation;

pub use chunker::{Batch, Chunker};
pub use export::{
    ExportFileType, ExportSink, InsertMode, LIVE_DATETIME_FORMAT, RealSleeper, Sleeper,
    UploadClient, WriteMode, effective_upload_url, live_export_file_name, write_csv_export,
    write_to_export_file, write_txt_export,
};
pub use merge::{
    MergeState, PanelText, filter_text, find_common_part, merge_text_no_overlap,
    no_space_between_sentences,
};
pub use session::{
    Clock, EngineFn, LiveEvent, LiveMeasurement, RealClock, SessionConfig, run_session,
};
pub use translation::{TranslationApplier, strip_newlines};

/// Which of Buzz's three live-transcript display strategies is in effect.
///
/// Ported from Buzz's `RecordingTranscriberMode` enum
/// (`buzz/settings/recording_transcriber_mode.py`, 3 variants), persisted as
/// a plain index (`Settings.Key.RECORDING_TRANSCRIBER_MODE`, default `0`)
/// rather than by name -- `list(RecordingTranscriberMode)[index]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingMode {
    /// New text is appended below the existing transcript; cursor pinned to
    /// the end.
    AppendBelow,
    /// New text is prepended above the existing transcript; cursor pinned to
    /// the start.
    AppendAbove,
    /// Sliding-window merge: overlapping chunk text is deduplicated via
    /// `find_common_part`/`merge_text_no_overlap` (F-062, later `live::merge`).
    AppendAndCorrect,
}

impl RecordingMode {
    /// Recovers the mode from its persisted index, Buzz parity
    /// (`list(RecordingTranscriberMode)[index]`).
    ///
    /// An out-of-range index falls back to [`RecordingMode::AppendBelow`] --
    /// Buzz's own default (stored index `0`) when no setting has ever been
    /// saved, and a safe recovery for a corrupted/foreign settings value
    /// (Buzz itself would raise `IndexError` here; humm degrades rather than
    /// panicking on untrusted persisted data).
    pub fn from_index(index: usize) -> RecordingMode {
        match index {
            1 => RecordingMode::AppendAbove,
            2 => RecordingMode::AppendAndCorrect,
            _ => RecordingMode::AppendBelow,
        }
    }

    /// The index this mode is persisted as (inverse of [`Self::from_index`]).
    pub fn as_index(self) -> usize {
        match self {
            RecordingMode::AppendBelow => 0,
            RecordingMode::AppendAbove => 1,
            RecordingMode::AppendAndCorrect => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const ALL_MODES: [RecordingMode; 3] = [
        RecordingMode::AppendBelow,
        RecordingMode::AppendAbove,
        RecordingMode::AppendAndCorrect,
    ];

    #[test]
    fn as_index_matches_buzz_enum_declaration_order() {
        assert_eq!(RecordingMode::AppendBelow.as_index(), 0);
        assert_eq!(RecordingMode::AppendAbove.as_index(), 1);
        assert_eq!(RecordingMode::AppendAndCorrect.as_index(), 2);
    }

    #[test]
    fn from_index_round_trips_for_all_variants() {
        for mode in ALL_MODES {
            assert_eq!(RecordingMode::from_index(mode.as_index()), mode);
        }
    }

    #[test]
    fn from_index_zero_is_append_below_buzz_default() {
        assert_eq!(RecordingMode::from_index(0), RecordingMode::AppendBelow);
    }

    #[test]
    fn from_index_out_of_range_falls_back_to_append_below() {
        assert_eq!(RecordingMode::from_index(3), RecordingMode::AppendBelow);
        assert_eq!(
            RecordingMode::from_index(usize::MAX),
            RecordingMode::AppendBelow
        );
    }
}
