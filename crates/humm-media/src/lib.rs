//! humm-media: ffmpeg/ffprobe/yt-dlp subprocess adapter implementing the
//! `AudioDecoder`, `MediaProber`, and `MediaFetcher` traits from `humm-core`.
#![forbid(unsafe_code)]

mod capabilities;
mod decode;
mod fetch;
mod probe;
mod video;

pub use capabilities::{MediaCapabilities, probe_binaries};
pub use decode::FfmpegDecoder;
pub use fetch::YtDlpFetcher;
pub use probe::FfprobeProber;
pub use video::FfmpegVideoStreamer;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use humm_core::error::MediaError;

/// Resolves a subprocess binary path: the `var` env override if set, else
/// `default` (looked up on `PATH` at spawn time).
///
/// Per Global Constraint 9, the env read is encapsulated here and delegates
/// to a pure inner function (`binary_from_env_value`) so tests exercise the
/// override/default logic without mutating process environment
/// (`std::env::set_var` is `unsafe` in edition 2024) — mirrors
/// `humm_core::paths`.
pub(crate) fn binary_from_env(var: &str, default: &str) -> PathBuf {
    binary_from_env_value(std::env::var_os(var).as_deref(), default)
}

fn binary_from_env_value(value: Option<&OsStr>, default: &str) -> PathBuf {
    value
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

/// Maps a subprocess spawn/exec error to a `MediaError`, turning "binary not
/// found" into an actionable [`MediaError::BinaryMissing`] and anything else
/// into a plain I/O error.
pub(crate) fn map_spawn_error(
    program: &str,
    env_var: &str,
    binary: &Path,
    err: std::io::Error,
) -> MediaError {
    if err.kind() == std::io::ErrorKind::NotFound {
        MediaError::BinaryMissing {
            binary: binary.display().to_string(),
            hint: format!(
                "{program} not found — install it (e.g. `brew install {program}`) or set {env_var}"
            ),
        }
    } else {
        MediaError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn binary_from_env_value_uses_override_when_present() {
        let resolved = binary_from_env_value(Some(OsStr::new("/custom/ffmpeg")), "ffmpeg");
        assert_eq!(resolved, PathBuf::from("/custom/ffmpeg"));
    }

    #[test]
    fn binary_from_env_value_falls_back_to_default_when_absent() {
        let resolved = binary_from_env_value(None, "ffmpeg");
        assert_eq!(resolved, PathBuf::from("ffmpeg"));
    }

    #[test]
    fn map_spawn_error_not_found_becomes_binary_missing_with_hint() {
        let err = map_spawn_error(
            "ffmpeg",
            "HUMM_FFMPEG",
            Path::new("/nonexistent/ffmpeg"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        match err {
            MediaError::BinaryMissing { binary, hint } => {
                assert_eq!(binary, "/nonexistent/ffmpeg");
                assert!(hint.contains("HUMM_FFMPEG"), "hint was: {hint}");
                assert!(hint.contains("ffmpeg not found"), "hint was: {hint}");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
    }

    #[test]
    fn map_spawn_error_other_kind_stays_io() {
        let err = map_spawn_error(
            "ffmpeg",
            "HUMM_FFMPEG",
            Path::new("ffmpeg"),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(matches!(err, MediaError::Io(_)));
    }
}
