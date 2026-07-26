//! Domain error types.
//!
//! Every enum here is `thiserror`-derived and `#[non_exhaustive]` so adapters
//! and later milestones can add variants without a breaking change. Every
//! variant carries actionable context (`docs/design/ARCHITECTURE.md` §6):
//! messages should tell the user what's wrong *and* what to do about it.

use std::path::PathBuf;

/// Errors from decoding, probing, or otherwise touching media files (the
/// ffmpeg/ffprobe subprocess boundary).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MediaError {
    /// A required external binary (ffmpeg, ffprobe, yt-dlp) was not found.
    #[error("{binary} not found — {hint}")]
    BinaryMissing {
        /// Name of the missing binary.
        binary: String,
        /// Actionable remediation (install command, env var to set).
        hint: String,
    },
    /// The decoder subprocess failed to produce PCM.
    #[error("decode failed: {detail}")]
    DecodeFailed {
        /// Diagnostic detail (stderr excerpt, exit code).
        detail: String,
    },
    /// The encoder subprocess failed to produce a compressed audio file.
    #[error("audio encode failed: {detail}")]
    EncodeFailed {
        /// Diagnostic detail — the adapter names its tool here (e.g.
        /// "ffmpeg exited with status ...") since this message is
        /// tool-agnostic (mirrors `DecodeFailed`/`ProbeFailed`/`FetchFailed`).
        detail: String,
    },
    /// The prober subprocess failed to report media metadata.
    #[error("probe failed: {detail}")]
    ProbeFailed {
        /// Diagnostic detail.
        detail: String,
    },
    /// A remote media download (yt-dlp) failed.
    #[error("download failed: {detail}")]
    FetchFailed {
        /// Diagnostic detail (yt-dlp stderr excerpt, exit code).
        detail: String,
    },
    /// The input file has no audio stream to decode.
    #[error("no audio stream in {}", path.display())]
    NoAudioStream {
        /// The file that was probed.
        path: PathBuf,
    },
    /// The demucs speech-isolation subprocess failed to produce a vocals
    /// stem (F-027, ADR-023).
    #[error("demucs speech isolation failed: {detail}")]
    IsolationFailed {
        /// Diagnostic detail — stderr excerpt/exit status, plus an
        /// actionable note that demucs may need to download its model on
        /// first run (Buzz's own task-error text,
        /// `file_transcriber_queue_worker.py:198`).
        detail: String,
    },
    /// The `humm-dfn` DeepFilterNet3 noise-reduction pass failed to build
    /// or run its model (F-112). Never surfaces as a task failure on its
    /// own — the queue's noise-reduction step logs this and degrades to
    /// the prior audio (Buzz parity, `plugin.py:58-60`).
    #[error("DeepFilterNet noise reduction failed: {detail}")]
    ReductionFailed {
        /// Diagnostic detail from the underlying `deep_filter`/`tract`
        /// error (model load or per-frame inference failure).
        detail: String,
    },
    /// Underlying I/O error.
    #[error(transparent)]
    Io(
        /// The underlying I/O error.
        #[from]
        std::io::Error,
    ),
    /// The operation was canceled via `CancelToken`.
    #[error("canceled")]
    Canceled,
}

/// Errors from the whisper inference engine boundary.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The model file could not be loaded.
    #[error("failed to load model at {}: {detail}", path.display())]
    ModelLoad {
        /// Path to the model file.
        path: PathBuf,
        /// Diagnostic detail.
        detail: String,
    },
    /// Inference failed mid-run.
    #[error("inference failed: {detail}")]
    Inference {
        /// Diagnostic detail.
        detail: String,
    },
    /// A cloud-API transcription backend failed: an HTTP/transport error, a
    /// non-2xx response, a response whose body didn't match any expected
    /// shape, or a failure preparing the upload
    /// (`OpenAiEngine`, design spec §2.2/§3).
    ///
    /// `detail` is actionable — it names the endpoint's host and (for HTTP
    /// responses) the status plus a short body excerpt, and suggests
    /// checking the API key/base URL. It **never** contains the API key or
    /// the request's `Authorization` header (design spec §2's key-hygiene
    /// invariant).
    #[error("OpenAI API backend error: {detail}")]
    Api {
        /// Actionable detail: endpoint host + status + body excerpt. Never
        /// the API key or request headers.
        detail: String,
    },
    /// The operation was canceled via `CancelToken`.
    #[error("canceled")]
    Canceled,
}

/// Errors from the audio device boundary (cpal input capture and output
/// playback — ADR-005).
///
/// Implemented against by `humm-audio`'s `CpalCapture` (live recording) and
/// `CpalOutput` (playback); the varispeed `Player` surfaces these when opening
/// or controlling its output stream, and live recording surfaces them when
/// opening or controlling input. A missing or unusable device degrades
/// gracefully rather than crashing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AudioError {
    /// No usable audio output device is available (none present, or the
    /// default device could not be acquired).
    #[error("no audio output device available — {detail}")]
    DeviceUnavailable {
        /// Diagnostic detail (host/device error, or a hint to check the
        /// system's audio output selection).
        detail: String,
    },
    /// The output stream could not be built at any usable configuration.
    #[error("failed to build audio output stream: {detail}")]
    StreamBuild {
        /// Diagnostic detail (cpal build error, unsupported format).
        detail: String,
    },
    /// Starting or pausing playback on the stream failed.
    #[error("failed to control audio output stream: {detail}")]
    StreamPlay {
        /// Diagnostic detail (cpal play/pause error).
        detail: String,
    },
    /// A capture caller requested a specific input device by id, and no
    /// currently enumerated input device matches it (disconnected, renamed,
    /// or never existed).
    #[error("audio input device not found: {id}")]
    DeviceNotFound {
        /// The requested device id that could not be matched.
        id: String,
    },
}

/// Errors from the model registry/downloader.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModelError {
    /// The HTTP request for a model download failed.
    #[error("download failed: {detail}")]
    Http {
        /// Diagnostic detail.
        detail: String,
    },
    /// The downloaded file failed checksum verification.
    #[error("checksum mismatch for {file}")]
    ChecksumMismatch {
        /// The file that failed verification.
        file: String,
    },
    /// The requested model/backend combination isn't supported.
    #[error("unsupported: {detail}")]
    Unsupported {
        /// Diagnostic detail.
        detail: String,
    },
    /// Underlying I/O error.
    #[error(transparent)]
    Io(
        /// The underlying I/O error.
        #[from]
        std::io::Error,
    ),
    /// The operation was canceled via `CancelToken`.
    #[error("canceled")]
    Canceled,
}

/// Errors from writing export files (TXT/SRT/VTT).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// Writing an export file failed.
    #[error("failed to write {}: {source}", path.display())]
    Io {
        /// The file that failed to write.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

/// Errors from the task-store (SQLite persistence) boundary.
///
/// Implemented by `humm-store`'s `SqliteStore`
/// (`docs/design/ARCHITECTURE.md` §5, ADR-004).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The database file could not be opened or created.
    #[error("failed to open database at {}: {detail}", path.display())]
    Open {
        /// Path to the database file.
        path: PathBuf,
        /// Diagnostic detail.
        detail: String,
    },
    /// A SQL statement (query, insert, update, migration pragma, ...) failed,
    /// or the connection mutex was poisoned by a prior panic.
    #[error("database query failed: {detail}")]
    Query {
        /// Diagnostic detail.
        detail: String,
    },
    /// The database's schema contains data that cannot be interpreted (e.g.
    /// malformed persisted JSON).
    #[error("corrupt or unsupported database: {detail}")]
    Corrupt {
        /// Diagnostic detail.
        detail: String,
    },
    /// The database's `PRAGMA user_version` is higher than this build knows
    /// how to migrate — it was written by a newer humm. Distinct from
    /// [`StoreError::Corrupt`] so callers/tests can match it precisely.
    #[error(
        "database was written by a newer humm — upgrade humm or move {} \
         (schema version {found}, this build supports up to {supported})",
        path.display()
    )]
    SchemaTooNew {
        /// The schema version found in the database.
        found: i64,
        /// The highest schema version this build supports.
        supported: i64,
        /// Path to the database file.
        path: PathBuf,
    },
    /// Underlying I/O error (e.g. creating the database's parent directory).
    #[error(transparent)]
    Io(
        /// The underlying I/O error.
        #[from]
        std::io::Error,
    ),
}

/// The top-level error composing every core error type, returned by the
/// transcription pipeline.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// A media (decode/probe) error.
    #[error(transparent)]
    Media(#[from] MediaError),
    /// A transcription engine error.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// A model registry/download error.
    #[error(transparent)]
    Model(#[from] ModelError),
    /// An export-writing error.
    #[error(transparent)]
    Export(#[from] ExportError),
    /// A task-store (persistence) error.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The job's input path does not exist.
    #[error("input not found: {}", path.display())]
    InputMissing {
        /// The path that was expected to exist.
        path: PathBuf,
    },
    /// Decoding produced no audio samples.
    #[error("no audio decoded")]
    NoAudioDecoded,
    /// The operation was canceled via `CancelToken`.
    #[error("canceled")]
    Canceled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_error_binary_missing_renders_hint() {
        let err = MediaError::BinaryMissing {
            binary: "ffmpeg".to_string(),
            hint: "install ffmpeg or set HUMM_FFMPEG".to_string(),
        };
        assert!(
            err.to_string()
                .contains("install ffmpeg or set HUMM_FFMPEG")
        );
    }

    #[test]
    fn engine_error_model_load_renders_detail() {
        let err = EngineError::ModelLoad {
            path: PathBuf::from("/models/ggml-small.bin"),
            detail: "truncated file".to_string(),
        };
        assert!(err.to_string().contains("truncated file"));
    }

    #[test]
    fn model_error_checksum_mismatch_renders_file() {
        let err = ModelError::ChecksumMismatch {
            file: "ggml-small.bin".to_string(),
        };
        assert!(err.to_string().contains("ggml-small.bin"));
    }

    #[test]
    fn export_error_io_renders_path_and_source() {
        let source = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = ExportError::Io {
            path: PathBuf::from("/out/transcript.srt"),
            source,
        };
        let message = err.to_string();
        assert!(message.contains("transcript.srt"));
        assert!(message.contains("denied"));
    }

    #[test]
    fn core_error_input_missing_renders_path() {
        let err = CoreError::InputMissing {
            path: PathBuf::from("/audio/missing.mp3"),
        };
        assert!(err.to_string().contains("missing.mp3"));
    }

    #[test]
    fn audio_error_device_unavailable_renders_detail() {
        let err = AudioError::DeviceUnavailable {
            detail: "no default output device".to_string(),
        };
        assert!(err.to_string().contains("no default output device"));
    }

    #[test]
    fn audio_error_stream_build_renders_detail() {
        let err = AudioError::StreamBuild {
            detail: "format not supported".to_string(),
        };
        let message = err.to_string();
        assert!(message.contains("build"));
        assert!(message.contains("format not supported"));
    }

    #[test]
    fn audio_error_device_not_found_renders_id() {
        let err = AudioError::DeviceNotFound {
            id: "USB Microphone".to_string(),
        };
        let message = err.to_string();
        assert!(message.contains("not found"));
        assert!(message.contains("USB Microphone"));
    }

    #[test]
    fn store_error_open_renders_path_and_detail() {
        let err = StoreError::Open {
            path: PathBuf::from("/data/humm.sqlite"),
            detail: "permission denied".to_string(),
        };
        let message = err.to_string();
        assert!(message.contains("humm.sqlite"));
        assert!(message.contains("permission denied"));
    }

    #[test]
    fn store_error_corrupt_renders_detail() {
        let err = StoreError::Corrupt {
            detail: "unsupported schema version 7".to_string(),
        };
        assert!(err.to_string().contains("unsupported schema version 7"));
    }

    #[test]
    fn core_error_wraps_store_error() {
        let err: CoreError = StoreError::Query {
            detail: "boom".to_string(),
        }
        .into();
        assert!(matches!(err, CoreError::Store(_)));
        assert!(err.to_string().contains("boom"));
    }
}
