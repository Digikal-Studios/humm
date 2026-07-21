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
    /// The prober subprocess failed to report media metadata.
    #[error("probe failed: {detail}")]
    ProbeFailed {
        /// Diagnostic detail.
        detail: String,
    },
    /// The input file has no audio stream to decode.
    #[error("no audio stream in {}", path.display())]
    NoAudioStream {
        /// The file that was probed.
        path: PathBuf,
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
    /// The operation was canceled via `CancelToken`.
    #[error("canceled")]
    Canceled,
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
    /// The database's schema is at an unrecognized or unsupported version, or
    /// otherwise contains data that cannot be interpreted (e.g. malformed
    /// persisted JSON).
    #[error("corrupt or unsupported database: {detail}")]
    Corrupt {
        /// Diagnostic detail.
        detail: String,
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
