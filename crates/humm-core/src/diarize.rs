//! Speaker diarization boundary trait and types (F-087, M8 design spec §6,
//! ADR-024).
//!
//! Buzz's speaker identification runs NeMo's MSDD model plus
//! `ctc-forced-aligner` and a punctuation-restoration stack
//! (`speaker_identification_widget.py`) — not ported (ADR-011 outcome-level
//! parity): humm instead runs an ort-backed segmentation + embedding +
//! clustering pipeline (`humm-diarize`'s `OrtDiarizer`, implementing
//! [`Diarizer`] via the `speakrs` crate, ADR-024). This module holds only the
//! trait and its value types — pure, adapter-free, and therefore usable from
//! `humm-core`'s own unit tests without linking `ort`/`speakrs` (Standing
//! Policy adapter isolation: `humm-diarize` is the only crate naming either).
//!
//! Turning [`SpeakerTurn`]s into transcript-segment speaker labels (temporal-
//! overlap assignment, "Speaker 0..N" relabeling by first appearance,
//! consecutive-same-speaker merge) is a later task's pure-mapping addition to
//! this module — out of scope here.

use crate::boundary::Pcm;
use crate::cancel::CancelToken;
use crate::domain::Millis;

/// One detected speaker turn: a contiguous span of audio attributed to a
/// single speaker.
///
/// `speaker` is a diarizer-local, zero-based index (e.g. `speakrs`'s
/// `SPEAKER_00` -> `0`) — not yet a human-facing "Speaker N" label. Relabeling
/// by order of first appearance across a whole run (Buzz's label shape,
/// `speaker_identification_widget.py:743-773`) is the transcript-mapping
/// layer's job, not this type's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerTurn {
    /// Turn start time.
    pub start: Millis,
    /// Turn end time.
    pub end: Millis,
    /// Zero-based, diarizer-local speaker index.
    pub speaker: usize,
}

/// A coarse phase of a diarization run, for progress reporting.
///
/// Buzz's Qt progress dialog shows a numbered step count
/// (`speaker_identification_widget.py`'s "N/8 ..."); humm instead reports
/// the diarizer's *real* stages (UX_SPEC: progress must be real, not
/// cosmetic). Not every implementation can distinguish every variant here —
/// see `humm-diarize`'s `OrtDiarizer` doc for which stages it actually
/// emits and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiarizeStage {
    /// Loading the diarization models into memory (constructing inference
    /// sessions from the local model files).
    LoadingModels,
    /// Running speaker-change segmentation over the audio.
    Segmenting,
    /// Extracting per-segment speaker embeddings.
    Embedding,
    /// Clustering embeddings into speaker identities.
    Clustering,
}

/// One progress update from a [`Diarizer::diarize`] run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiarizeProgress {
    /// The stage this update describes.
    pub stage: DiarizeStage,
    /// Fractional completion within `stage`, in `[0.0, 1.0]`, when the
    /// implementation can genuinely measure it. `None` — never a fake
    /// interpolated value — when it can't (e.g. a single monolithic call
    /// with no intermediate hook).
    pub fraction: Option<f32>,
}

/// Errors from a [`Diarizer::diarize`] run.
#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    /// The diarization models are missing or incomplete locally.
    #[error("diarization models missing — {detail}")]
    ModelsMissing {
        /// Actionable detail (which file(s), and how to obtain them).
        detail: String,
    },
    /// Diarization ran but failed.
    #[error("diarization failed — {detail}")]
    Failed {
        /// Diagnostic detail from the underlying pipeline.
        detail: String,
    },
    /// The operation was canceled via `CancelToken`.
    #[error("diarization canceled")]
    Canceled,
}

/// Detects speaker turns in already-decoded audio (F-087).
///
/// Implemented by `humm-diarize`'s `OrtDiarizer` (an ort-backed segmentation,
/// embedding, and clustering pipeline via the `speakrs` crate, ADR-024).
/// Buzz's NeMo MSDD + ctc-forced-aligner + punctuation stack is not ported
/// (ADR-011 outcome-level parity: speaker count/boundaries are diffable,
/// labels never are).
pub trait Diarizer: Send + Sync {
    /// Diarizes `audio`, emitting [`DiarizeProgress`] updates to `progress`
    /// and checking `cancel` cooperatively. Returns the detected speaker
    /// turns in chronological order.
    fn diarize(
        &self,
        audio: &Pcm,
        progress: &mut dyn FnMut(DiarizeProgress),
        cancel: &CancelToken,
    ) -> Result<Vec<SpeakerTurn>, DiarizeError>;
}
