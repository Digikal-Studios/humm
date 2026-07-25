//! Boundary traits and the value types that cross them.
//!
//! Adapter crates (`humm-media`, `humm-whisper`, ...) implement these traits;
//! `humm-core` never names ffmpeg, whisper.cpp, or any other non-Rust
//! component directly (`docs/design/ARCHITECTURE.md` §2).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub use rtrb;

use crate::cancel::CancelToken;
use crate::domain::{Millis, Segment, Source, TaskKind, TaskSpec, TranscriptionId};
use crate::error::{AudioError, EngineError, MediaError, StoreError};
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
    /// Dimensions/frame rate of the file's video stream, if it has one.
    /// `None` whenever `has_video` is `false` (same cover-art exclusion:
    /// an embedded `attached_pic` stream is never reported here either).
    pub video: Option<VideoStreamInfo>,
}

/// Dimensions and frame rate of a probed video stream
/// (`ffmpeg_video_player.py:39-75`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoStreamInfo {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second. Parsed from ffprobe's `r_frame_rate` (`"n/d"` →
    /// `f64`), falling back to `25.0` on a missing/malformed value and
    /// floored to `1.0` (`ffmpeg_video_player.py:61-66,236`).
    pub fps: f64,
}

/// One decoded video frame, in RGBA byte order (ADR deviation from Buzz's
/// `rgb24` — see deviations ledger #10), ready for GPU texture upload.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    /// Presentation timestamp, synthesized from the frame's ffmpeg read
    /// order as `start + n·(1000/fps)` (Buzz parity,
    /// `ffmpeg_video_player.py:120-175`).
    pub ts: Millis,
    /// Interleaved RGBA bytes, `width * height * 4` long.
    pub rgba: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

/// Decoded interleaved playback PCM, at whatever channel count/sample rate
/// the decoder produced it (unlike [`Pcm`]'s fixed mono `SAMPLE_RATE`) —
/// used for audio-player playback rather than transcription. Buzz decodes
/// playback audio to 44.1 kHz stereo (`sounddevice_player.py:28-35`);
/// [`AudioDecoder::decode_playback`] mirrors that.
///
/// Backed by `Arc<[f32]>` for the same cheap-clone reason as [`Pcm`].
#[derive(Debug, Clone)]
pub struct PlaybackPcm {
    samples: Arc<[f32]>,
    /// Interleaved channel count (e.g. `2` for stereo).
    pub channels: u16,
    /// Sample rate in Hz.
    pub sample_rate: u32,
}

impl PlaybackPcm {
    /// Wraps interleaved decoded samples at `channels`/`sample_rate`.
    pub fn new(samples: Vec<f32>, channels: u16, sample_rate: u32) -> Self {
        PlaybackPcm {
            samples: samples.into(),
            channels,
            sample_rate,
        }
    }

    /// The raw interleaved samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Number of frames (samples per channel): `samples().len() / channels`.
    /// `0` when `channels` is `0`.
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / usize::from(self.channels)
        }
    }

    /// Duration implied by [`Self::frames`] at [`Self::sample_rate`],
    /// rounded down to the nearest millisecond. `Millis(0)` when
    /// `sample_rate` is `0`.
    pub fn duration(&self) -> Millis {
        if self.sample_rate == 0 {
            return Millis(0);
        }
        Millis((self.frames() as i64 * 1000) / i64::from(self.sample_rate))
    }
}

/// The configuration an [`AudioOutput`] stream is opened at: the sample rate
/// and channel count the `fill` callback produces interleaved `f32` frames in.
///
/// Distinct from [`crate::domain::OutputFormat`]/export outputs — this is a
/// live audio-device stream spec, not a transcript file format. `humm-audio`'s
/// `Player` opens its output at the [`PlaybackPcm`]'s own rate/channels so the
/// steady-state fill at speed `1.0` is a bit-exact copy; the adapter
/// negotiates the device's real rate and resamples internally (ADR-005) so
/// this spec is preserved from the caller's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamSpec {
    /// Sample rate in Hz the `fill` callback is invoked at.
    pub sample_rate: u32,
    /// Interleaved channel count the `fill` callback writes.
    pub channels: u16,
}

/// The real-time fill callback an [`AudioOutput`] invokes to produce audio.
///
/// Called repeatedly on the audio thread with an interleaved `f32` buffer to
/// fill ([`StreamSpec::channels`] samples per frame); it must not block or
/// allocate in its steady state. Boxed so implementations can store it behind
/// the `dyn` boundary; `Send` so it can move onto the audio thread.
pub type FillCallback = Box<dyn FnMut(&mut [f32]) + Send>;

/// An audio output device the playback engine can open a stream against.
///
/// Implemented by `humm-audio`'s `CpalOutput` (the cpal boundary, ADR-005);
/// unit tests substitute a mock whose captured `fill` closure they pull
/// synchronously, so the varispeed `Player` is testable without a real audio
/// thread.
pub trait AudioOutput: Send + Sync {
    /// Opens an output stream at `spec`. `fill` is invoked repeatedly on the
    /// real-time audio thread to fill an interleaved `f32` buffer
    /// (`spec.channels` per frame); it must not block or allocate in its
    /// steady state. The returned [`OutputStream`] starts paused — call
    /// [`OutputStream::play`] to begin.
    fn open(
        &self,
        spec: StreamSpec,
        fill: FillCallback,
    ) -> Result<Box<dyn OutputStream>, AudioError>;
}

/// A handle to an open audio output stream. Dropping it stops the stream.
pub trait OutputStream: Send {
    /// Starts (or resumes) invoking the `fill` callback on the audio thread.
    fn play(&mut self) -> Result<(), AudioError>;
    /// Pauses the stream: the `fill` callback stops being invoked.
    fn pause(&mut self) -> Result<(), AudioError>;
}

/// An audio input device available for live recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    /// Stable device identifier (name-based, as exposed by the adapter like cpal).
    pub id: String,
    /// User-friendly device name.
    pub name: String,
    /// Whether this is the system's default input device.
    pub is_default: bool,
}

/// Snapshot of live audio capture metering data, shared between two
/// different writers and the UI layer, read by the UI thread without
/// locking.
///
/// The three fields have two different owners, per Buzz parity:
/// - `current_rms` is written by the **capture adapter** (`humm-audio`'s
///   `CpalCapture`/`MockCapture`) once per RT callback — this is the only
///   field the capture adapter writes.
/// - `average_rms` and `queue_len` are written by the **session/chunker
///   layer** (a later task), once per accumulation batch — Buzz's
///   transcriber emits `queue_size_changed`/its rolling average signal from
///   the chunker loop, never from the raw audio callback
///   (`recording_transcriber.py`), so the capture adapter has no business
///   writing them.
///
/// All three values use relaxed ordering because each is single-writer
/// telemetry (just two different single writers), and the meter widget reads
/// them once per display frame. Stale reads are acceptable (a frame or two of
/// lag in the displayed meters), and there is no shared mutable state to
/// synchronize.
#[derive(Debug)]
pub struct MeterSnapshot {
    /// Current RMS level as f32 bits packed in an `AtomicU32`, updated per
    /// capture callback by the capture adapter. Decode with `f32::from_bits()`.
    current_rms: AtomicU32,
    /// Average RMS level as f32 bits, accumulated over 1 second in meter-only
    /// mode or per batch while recording. Written by the session/chunker
    /// layer (a later task), not the capture adapter. Decode with
    /// `f32::from_bits()`.
    average_rms: AtomicU32,
    /// Number of samples queued in the session/chunker's accumulation queue
    /// (Buzz's transcriber `queue_size_changed`, emitted once per processed
    /// batch — `recording_transcriber.py`). Written by the session/chunker
    /// layer (a later task); the capture adapter never writes this field —
    /// it has no notion of the accumulation queue, only of the RT callback's
    /// own ring.
    queue_len: AtomicUsize,
}

impl MeterSnapshot {
    /// Creates a new snapshot, initialized to all zeros.
    pub fn new() -> Self {
        MeterSnapshot {
            current_rms: AtomicU32::new(0),
            average_rms: AtomicU32::new(0),
            queue_len: AtomicUsize::new(0),
        }
    }

    /// Sets the current RMS level (f32 encoded as bits).
    pub fn set_current_rms(&self, rms: f32) {
        self.current_rms.store(rms.to_bits(), Ordering::Relaxed);
    }

    /// Gets the current RMS level (f32 decoded from bits).
    pub fn get_current_rms(&self) -> f32 {
        f32::from_bits(self.current_rms.load(Ordering::Relaxed))
    }

    /// Sets the average RMS level (f32 encoded as bits).
    pub fn set_average_rms(&self, rms: f32) {
        self.average_rms.store(rms.to_bits(), Ordering::Relaxed);
    }

    /// Gets the average RMS level (f32 decoded from bits).
    pub fn get_average_rms(&self) -> f32 {
        f32::from_bits(self.average_rms.load(Ordering::Relaxed))
    }

    /// Sets the queue length in samples.
    pub fn set_queue_len(&self, len: usize) {
        self.queue_len.store(len, Ordering::Relaxed);
    }

    /// Gets the queue length in samples.
    pub fn get_queue_len(&self) -> usize {
        self.queue_len.load(Ordering::Relaxed)
    }

    /// Resets all metering values to zero.
    pub fn reset(&self) {
        self.current_rms.store(0, Ordering::Relaxed);
        self.average_rms.store(0, Ordering::Relaxed);
        self.queue_len.store(0, Ordering::Relaxed);
    }
}

impl Default for MeterSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Captures live audio from a system input device.
///
/// Implemented by `humm-audio`'s `CpalCapture` adapter; unit tests substitute
/// a mock. The stream produces 16 kHz mono f32 samples at the producer end of
/// an `rtrb` SPSC ring buffer (resample/downmix happen inside the adapter per
/// ADR-005).
pub trait AudioCapture: Send {
    /// Lists all available audio input devices.
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioError>;

    /// Starts capturing audio from the specified device (or the default if
    /// `device_id` is `None`). Delivers 16 kHz mono f32 samples to `sink`.
    ///
    /// The adapter resamples/downmixes as needed inside the capture callback
    /// (ADR-005) so the producer always sees 16 kHz mono f32. `meter` receives
    /// per-callback RMS (`current_rms` only — `average_rms`/`queue_len` are
    /// owned by the session/chunker layer, see [`MeterSnapshot`]'s docs).
    ///
    /// Returns a [`CaptureHandle`] that must be dropped to stop the stream.
    fn start(
        &mut self,
        device_id: Option<&str>,
        sink: rtrb::Producer<f32>,
        meter: Arc<MeterSnapshot>,
    ) -> Result<Box<dyn CaptureHandle>, AudioError>;
}

/// A handle to an open audio capture stream. Dropping it stops the stream.
pub trait CaptureHandle: Send {
    /// Stops the capture stream (called when the handle is dropped or
    /// explicitly stopped).
    fn stop(self: Box<Self>);
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

    /// Decodes `path` to whole-file interleaved playback PCM (44.1 kHz
    /// stereo — Buzz parity, `sounddevice_player.py:28-35`), checking
    /// `cancel` cooperatively. Distinct from [`Self::decode`]: this feeds
    /// the audio player, not the transcription engine.
    fn decode_playback(&self, path: &Path, cancel: &CancelToken)
    -> Result<PlaybackPcm, MediaError>;
}

/// Encodes decoded PCM to a compressed audio file. Implemented by the ffmpeg
/// subprocess adapter (`humm-media`); used by the OpenAI API transcription
/// engine (Milestone 6) to prepare an mp3 upload from already-decoded
/// [`Pcm`] instead of Buzz's re-encode of the original file
/// (`openai_whisper_api_file_transcriber.py:65-94`) — mechanism deviation
/// with identical outcome, since Whisper-family APIs downmix/resample to
/// 16 kHz mono anyway (M6 design spec §3.1, deviations ledger #2).
pub trait AudioEncoder: Send + Sync {
    /// Encodes `pcm` (16 kHz mono `f32`) to an mp3 file at `dest`, checking
    /// `cancel` cooperatively.
    fn encode_mp3(&self, pcm: &Pcm, dest: &Path, cancel: &CancelToken) -> Result<(), MediaError>;
}

/// Probes container/stream metadata. Implemented by the ffprobe subprocess
/// adapter.
pub trait MediaProber: Send + Sync {
    /// Probes `path` for duration and stream presence.
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError>;
}

/// Opens a decoded video frame stream. Implemented by the ffmpeg rawvideo
/// subprocess adapter (`ffmpeg_video_player.py:82-175`).
pub trait VideoStreamer: Send + Sync {
    /// Opens `path`'s video stream starting at `start`, reading frames
    /// according to `info` (as previously reported by [`MediaProber`]).
    fn open(
        &self,
        path: &Path,
        start: Millis,
        info: &VideoStreamInfo,
    ) -> Result<Box<dyn FrameStream>, MediaError>;
}

/// A live decoded-frame stream from an open [`VideoStreamer`], polled by
/// display timer.
pub trait FrameStream: Send {
    /// Best frame for `position` per Buzz's drain-stale rule
    /// (`ffmpeg_video_player.py:177-200`); `None` means keep showing the
    /// last frame returned (no fresh frame is ready yet, or none ever
    /// will be for this position).
    fn frame_for(&mut self, position: Millis) -> Option<VideoFrame>;

    /// Restarts decoding at `position` (kill the current subprocess and
    /// respawn one seeked there — Buzz has no separate seek path, just
    /// `start()` again, `ffmpeg_video_player.py:264-271`).
    fn seek(&mut self, position: Millis) -> Result<(), MediaError>;
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

    /// Marks the task `skipped`, sets `time_ended`, and persists `segments`
    /// as the task's transcript with every translation forced to `""` —
    /// mirrors [`Self::mark_completed`]'s shape, but for a task whose input
    /// was found already transcribed rather than one that actually ran
    /// (Buzz's `update_transcription_as_skipped`,
    /// `transcription_service.py:62-73`, which hardcodes
    /// `translation=''` regardless of what the segment carries).
    /// `Status::Skipped` is a terminal state alternative to `Completed`, not
    /// a failure: Buzz counts a skip toward "quit on complete"
    /// (`main_window.py:435-443`).
    fn mark_skipped(&self, id: &TranscriptionId, segments: &[Segment]) -> Result<(), StoreError>;

    /// Finds the newest (`time_ended` descending) `completed` task whose
    /// input file's basename matches `name` — the trait-level counterpart
    /// of `SqliteStore::find_completed_transcription_by_basename`
    /// (distinctly named, not reused, so the trait impl's delegation can
    /// never become accidental recursion), needed so
    /// [`crate::postproc::skip::check`]'s database check (F-115, design
    /// spec §2.3/§4) can reach it through the same store handle
    /// [`crate::queue::RunnerDeps::store`] already holds, without
    /// `humm-core` depending on `humm-store` for the concrete type.
    fn find_completed_by_file_name(
        &self,
        name: &str,
    ) -> Result<Option<TranscriptionId>, StoreError>;

    /// Fetches every persisted segment for `id`, in insertion order — the
    /// trait-level counterpart of `SqliteStore::get_segments`, used by the
    /// same F-115 database check to copy a prior completed transcription's
    /// segments. Returns whatever is actually stored (including any
    /// translation); the hook itself is what drops translations, not this
    /// method.
    fn segments_for(&self, id: &TranscriptionId) -> Result<Vec<Segment>, StoreError>;
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

    #[test]
    fn meter_snapshot_set_get_current_rms() {
        let meter = MeterSnapshot::new();
        let rms = 0.5_f32;
        meter.set_current_rms(rms);
        assert_eq!(meter.get_current_rms(), rms);
    }

    #[test]
    fn meter_snapshot_set_get_average_rms() {
        let meter = MeterSnapshot::new();
        let rms = 0.75_f32;
        meter.set_average_rms(rms);
        assert_eq!(meter.get_average_rms(), rms);
    }

    #[test]
    fn meter_snapshot_set_get_queue_len() {
        let meter = MeterSnapshot::new();
        let len = 1024_usize;
        meter.set_queue_len(len);
        assert_eq!(meter.get_queue_len(), len);
    }

    #[test]
    fn meter_snapshot_f32_bit_exactness() {
        let meter = MeterSnapshot::new();
        let special_values = [
            0.0_f32,
            -0.0_f32,
            1.0_f32,
            -1.0_f32,
            0.5_f32,
            f32::MIN_POSITIVE,
            f32::MAX,
            f32::NAN,
        ];
        for val in &special_values {
            meter.set_current_rms(*val);
            let retrieved = meter.get_current_rms();
            assert_eq!(retrieved.to_bits(), val.to_bits());
        }
    }

    #[test]
    fn meter_snapshot_reset() {
        let meter = MeterSnapshot::new();
        meter.set_current_rms(0.5);
        meter.set_average_rms(0.75);
        meter.set_queue_len(1024);

        meter.reset();

        assert_eq!(meter.get_current_rms(), 0.0);
        assert_eq!(meter.get_average_rms(), 0.0);
        assert_eq!(meter.get_queue_len(), 0);
    }

    #[test]
    fn meter_snapshot_default() {
        let meter = MeterSnapshot::default();
        assert_eq!(meter.get_current_rms(), 0.0);
        assert_eq!(meter.get_average_rms(), 0.0);
        assert_eq!(meter.get_queue_len(), 0);
    }

    #[test]
    fn audio_capture_trait_object_safety() {
        // No-op fake implementations to verify object-safety.
        struct FakeCapture;

        impl AudioCapture for FakeCapture {
            fn devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
                Ok(vec![])
            }

            fn start(
                &mut self,
                _device_id: Option<&str>,
                _sink: rtrb::Producer<f32>,
                _meter: Arc<MeterSnapshot>,
            ) -> Result<Box<dyn CaptureHandle>, AudioError> {
                Ok(Box::new(FakeHandle))
            }
        }

        struct FakeHandle;

        impl CaptureHandle for FakeHandle {
            fn stop(self: Box<Self>) {}
        }

        // Should compile: trait is object-safe.
        let _capture: Box<dyn AudioCapture> = Box::new(FakeCapture);
    }
}
