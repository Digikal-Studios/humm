//! Version/availability probing for the ffmpeg/ffprobe binaries.
//!
//! Never a hard error: missing or unusable binaries degrade to `None`
//! fields (Standing Policy in `CLAUDE.md`: missing binaries degrade
//! features gracefully, never crash).

use std::path::Path;
use std::process::{Command, Stdio};

use crate::binary_from_env;

/// Availability/version of the ffmpeg and ffprobe binaries as actually
/// found on this machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaCapabilities {
    /// Parsed `ffmpeg -version` string (e.g. `"8.1.1"`), or `None` if
    /// ffmpeg is missing or its version output couldn't be parsed.
    pub ffmpeg_version: Option<String>,
    /// Parsed `ffprobe -version` string, or `None` if ffprobe is missing or
    /// its version output couldn't be parsed.
    pub ffprobe_version: Option<String>,
}

/// Probes for ffmpeg/ffprobe availability and version. Never errors:
/// missing or unusable binaries simply produce `None` fields.
pub fn probe_binaries() -> MediaCapabilities {
    MediaCapabilities {
        ffmpeg_version: binary_version(&binary_from_env("HUMM_FFMPEG", "ffmpeg")),
        ffprobe_version: binary_version(&binary_from_env("HUMM_FFPROBE", "ffprobe")),
    }
}

fn binary_version(binary: &Path) -> Option<String> {
    let output = Command::new(binary)
        .arg("-version")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;
    parse_version_line(first_line)
}

/// Parses the version token out of a `<binary> -version` first line, e.g.
/// `"ffmpeg version 8.1.1 Copyright (c) 2000-2026 the FFmpeg developers"` →
/// `Some("8.1.1")`.
fn parse_version_line(first_line: &str) -> Option<String> {
    let mut parts = first_line.split_whitespace();
    parts.next()?; // program name ("ffmpeg"/"ffprobe")
    if parts.next()? != "version" {
        return None;
    }
    parts.next().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_ffmpeg_version_line() {
        assert_eq!(
            parse_version_line(
                "ffmpeg version 8.1.1 Copyright (c) 2000-2026 the FFmpeg developers"
            ),
            Some("8.1.1".to_string())
        );
    }

    #[test]
    fn parses_ffprobe_version_line() {
        assert_eq!(
            parse_version_line(
                "ffprobe version 8.1.1 Copyright (c) 2007-2026 the FFmpeg developers"
            ),
            Some("8.1.1".to_string())
        );
    }

    #[test]
    fn unrecognized_line_yields_none() {
        assert_eq!(parse_version_line("not a version line"), None);
    }

    #[test]
    fn empty_line_yields_none() {
        assert_eq!(parse_version_line(""), None);
    }

    #[test]
    fn missing_binary_yields_none_not_error() {
        assert_eq!(
            binary_version(Path::new("/nonexistent/ffmpeg-for-test")),
            None
        );
    }
}
