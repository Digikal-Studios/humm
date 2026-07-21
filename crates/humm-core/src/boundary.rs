//! Boundary traits and the value types that cross them.
//!
//! Adapter crates (`humm-media`, `humm-whisper`, ...) implement these traits;
//! `humm-core` never names ffmpeg, whisper.cpp, or any other non-Rust
//! component directly (`docs/design/ARCHITECTURE.md` §2).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cancel::CancelToken;
use crate::domain::{Millis, Segment, Source, TaskKind, TaskSpec, TranscriptionId};
use crate::error::{EngineError, MediaError, StoreError};
use crate::language::Language;

/// Sample rate (Hz) all decoded PCM and captured audio is normalized to.
pub const SAMPLE_RATE: u32 = 16_000;

/// Decoded mono PCM audio at [`SAMPLE_RATE`], as `f32` samples in `[-1.0, 1.0]`.
///
/// Backed by `Arc<[f32]>` so handing decoded audio from decoder to engine is a
/// cheap clone, not a copy.
#[derive(Debug, Clone)]
pub struct Pcm {
    samples: Arc<[f32]>,
}

impl Pcm {
    /// Wraps decoded samples.
    pub fn new(samples: Vec<f32>) -> Self {
        Pcm {
            samples: samples.into(),
        }
    }

    /// The raw samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// `true` if there are no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Duration implied by the sample count at [`SAMPLE_RATE`], rounded down
    /// to the nearest millisecond.
    pub fn duration(&self) -> Millis {
        Millis((self.samples.len() as i64 * 1000) / i64::from(SAMPLE_RATE))
    }
}

/// Container/stream metadata reported by the media prober.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    /// Total duration, if known.
    pub duration: Option<Millis>,
    /// Whether the file has at least one audio stream.
    pub has_audio: bool,
    /// Whether the file has at least one video stream.
    pub has_video: bool,
}

/// Parameters for a single transcription run, translated from
/// [`crate::domain::TranscriptionOptions`] at the adapter boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineOptions {
    /// Force a specific source language, or `None` to auto-detect.
    pub language: Option<Language>,
    /// Transcribe or translate-to-English.
    pub task: TaskKind,
    /// Optional priming text passed to the model.
    pub initial_prompt: String,
    /// Whether to request word-level timestamps.
    pub word_timestamps: bool,
    /// Fallback temperature schedule.
    pub temperature: Vec<f32>,
    /// VAD model path. `Some` enables voice-activity detection (F-045);
    /// `None` disables it.
    pub vad_model: Option<PathBuf>,
    /// Thread count override; `None` uses the adapter's default.
    pub threads: Option<u32>,
}

/// Capabilities an engine implementation supports, for UI/CLI feature gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineCaps {
    /// Whether word-level timestamps are supported.
    pub word_timestamps: bool,
    /// Whether translate-to-English is supported.
    pub translate: bool,
    /// Whether voice-activity detection is supported.
    pub vad: bool,
}

/// Decodes media files to mono 16 kHz PCM. Implemented by the ffmpeg
/// subprocess adapter.
pub trait AudioDecoder: Send + Sync {
    /// Decodes `path` to PCM, checking `cancel` cooperatively.
    fn decode(&self, path: &Path, cancel: &CancelToken) -> Result<Pcm, MediaError>;
}

/// Probes container/stream metadata. Implemented by the ffprobe subprocess
/// adapter.
pub trait MediaProber: Send + Sync {
    /// Probes `path` for duration and stream presence.
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError>;
}

/// Progress of a media download, as reported by a [`MediaFetcher`].
///
/// Mirrors yt-dlp's `downloaded_bytes`/`total_bytes` progress hook
/// (`buzz/transcriber/file_transcriber.py:172-174`): `total` is `None` when
/// the server did not advertise a content length, in which case a fractional
/// progress cannot be computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchProgress {
    /// Bytes downloaded so far.
    pub downloaded: u64,
    /// Total bytes to download, if the source advertised a size.
    pub total: Option<u64>,
}

/// Downloads remote media to a local file. Implemented by the yt-dlp
/// subprocess adapter (`humm-media`, Task 6).
///
/// Ported from the URL-download half of Buzz's `FileTranscriber.run`
/// (`buzz/transcriber/file_transcriber.py:38-73,75-153`), where the download
/// happens inside the task run, immediately before transcription.
pub trait MediaFetcher: Send + Sync {
    /// Downloads the best audio of `url` into `dest_dir`, returning the
    /// downloaded file's path.
    ///
    /// Emits progress through `progress` (called zero or more times) and polls
    /// `cancel` cooperatively, killing the subprocess if cancellation is
    /// requested. `dest_dir` is expected to exist; the caller creates it.
    fn fetch(
        &self,
        url: &url::Url,
        dest_dir: &Path,
        progress: &mut dyn FnMut(FetchProgress),
        cancel: &CancelToken,
    ) -> Result<PathBuf, MediaError>;
}

/// Receives incremental transcription results as they're produced.
///
/// Implementations must not panic: [`Self::on_segment`] and
/// [`Self::on_progress`] may be invoked from FFI callback contexts (e.g. the
/// whisper.cpp new-segment/progress callbacks) where unwinding across the
/// foreign frame aborts the process. Handle errors by degrading gracefully
/// (drop the update, set a flag) rather than panicking.
pub trait SegmentSink {
    /// Called once per recognized segment, in order.
    fn on_segment(&mut self, segment: Segment);
    /// Called with overall progress in `0.0..=1.0`.
    fn on_progress(&mut self, fraction: f32);
}

/// Speech-to-text over decoded PCM. Implemented by `humm-whisper` (local) and,
/// later, an OpenAI API engine.
pub trait TranscriptionEngine: Send {
    /// Transcribes `audio`, emitting segments/progress to `sink`, checking
    /// `cancel` cooperatively.
    fn transcribe(
        &mut self,
        audio: &Pcm,
        options: &EngineOptions,
        sink: &mut dyn SegmentSink,
        cancel: &CancelToken,
    ) -> Result<(), EngineError>;

    /// The capabilities this engine implementation supports.
    fn capabilities(&self) -> EngineCaps;
}

/// The task queue's writer surface: the *only* writer of a task's `status`
/// (`docs/design/ARCHITECTURE.md` §5). Implemented by `humm-store`'s
/// `SqliteStore`; the queue drives a task through these transitions in order
/// (`create_task` → `mark_started` → zero or more `update_progress`/
/// `update_file` → exactly one of `mark_completed`/`mark_failed`/
/// `mark_canceled`).
pub trait TaskStore: Send + Sync {
    /// Inserts a new `queued` task row for `id`, persisting every field of
    /// `spec` (including the full `options_json` round-trip) plus `source`.
    fn create_task(
        &self,
        id: &TranscriptionId,
        spec: &TaskSpec,
        source: Source,
    ) -> Result<(), StoreError>;

    /// Resets an already-terminated task's *existing* row back to `queued`
    /// for a retry — no new row is inserted: `status` becomes `queued`,
    /// `progress` resets to `0.0`, and `time_started`/`time_ended`/
    /// `error_message` are cleared to `NULL`. Every other column (name,
    /// notes, the persisted spec, ...) is left untouched.
    ///
    /// Ported from Buzz's `TranscriptionDAO.reset_transcription_for_restart`
    /// (`transcription_dao.py:353-370`), which raises when the row doesn't
    /// exist (`query.numRowsAffected() == 0`); this returns `Err` in that
    /// case rather than silently no-oping.
    fn requeue_task(&self, id: &TranscriptionId) -> Result<(), StoreError>;

    /// Marks the task as started: `status` becomes `in_progress` and
    /// `time_started` is set to now.
    fn mark_started(&self, id: &TranscriptionId) -> Result<(), StoreError>;

    /// Records progress: `status` becomes `in_progress` and `progress` is set
    /// to `fraction`, clamped to `[0.0, 1.0]`.
    fn update_progress(&self, id: &TranscriptionId, fraction: f64) -> Result<(), StoreError>;

    /// Updates the task's resolved local file path, e.g. once a
    /// [`crate::domain::Input::Url`] task's download completes.
    fn update_file(&self, id: &TranscriptionId, path: &Path) -> Result<(), StoreError>;

    /// Marks the task `completed`, sets `time_ended`, and persists
    /// `segments` (in order) as the task's transcript.
    fn mark_completed(&self, id: &TranscriptionId, segments: &[Segment]) -> Result<(), StoreError>;

    /// Marks the task `failed`, sets `time_ended`, and records `error` as the
    /// task's `error_message`. No segments are persisted.
    fn mark_failed(&self, id: &TranscriptionId, error: &str) -> Result<(), StoreError>;

    /// Marks the task `canceled` and sets `time_ended`. No segments are
    /// persisted.
    fn mark_canceled(&self, id: &TranscriptionId) -> Result<(), StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn duration_of_one_second_at_16khz() {
        let pcm = Pcm::new(vec![0.0; 16_000]);
        assert_eq!(pcm.duration(), Millis(1000));
    }

    #[test]
    fn duration_rounds_down_to_nearest_millisecond() {
        let pcm = Pcm::new(vec![0.0; 16_015]);
        assert_eq!(pcm.duration(), Millis(1000));
    }

    #[test]
    fn duration_of_empty_pcm_is_zero() {
        let pcm = Pcm::new(vec![]);
        assert_eq!(pcm.duration(), Millis(0));
        assert!(pcm.is_empty());
        assert_eq!(pcm.len(), 0);
    }

    #[test]
    fn samples_and_len_match_input() {
        let pcm = Pcm::new(vec![0.1, 0.2, 0.3]);
        assert_eq!(pcm.len(), 3);
        assert_eq!(pcm.samples(), &[0.1, 0.2, 0.3]);
    }
}
