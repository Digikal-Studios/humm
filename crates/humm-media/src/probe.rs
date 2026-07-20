//! ffprobe-backed [`MediaProber`] (ADR-003).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use humm_core::boundary::{MediaInfo, MediaProber};
use humm_core::domain::Millis;
use humm_core::error::MediaError;
use serde::Deserialize;

use crate::{binary_from_env, map_spawn_error};

/// Probes container/stream metadata via an `ffprobe` subprocess.
pub struct FfprobeProber {
    binary: PathBuf,
}

impl FfprobeProber {
    /// Uses `HUMM_FFPROBE` if set, else `ffprobe` on `PATH`.
    pub fn new() -> Self {
        Self::with_binary(binary_from_env("HUMM_FFPROBE", "ffprobe"))
    }

    /// Uses an explicit binary path/name, bypassing `HUMM_FFPROBE`.
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        FfprobeProber {
            binary: path.into(),
        }
    }
}

impl Default for FfprobeProber {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaProber for FfprobeProber {
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        let output = Command::new(&self.binary)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| map_spawn_error("ffprobe", "HUMM_FFPROBE", &self.binary, err))?;

        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(MediaError::ProbeFailed { detail });
        }

        parse_probe_json(&output.stdout).map_err(|detail| MediaError::ProbeFailed { detail })
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: String,
    #[serde(default)]
    disposition: Option<FfprobeDisposition>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeDisposition {
    #[serde(default)]
    attached_pic: i64,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

/// Parses ffprobe's `-show_format -show_streams` JSON into `MediaInfo`.
///
/// `format.duration` is an optional *string* of seconds (ffprobe convention)
/// converted to rounded milliseconds. `has_video` excludes streams whose
/// `disposition.attached_pic == 1` — embedded cover art must not count as a
/// video stream.
fn parse_probe_json(bytes: &[u8]) -> Result<MediaInfo, String> {
    let parsed: FfprobeOutput = serde_json::from_slice(bytes).map_err(|err| err.to_string())?;

    let has_audio = parsed.streams.iter().any(|s| s.codec_type == "audio");
    let has_video = parsed.streams.iter().any(|s| {
        s.codec_type == "video" && s.disposition.as_ref().map(|d| d.attached_pic).unwrap_or(0) != 1
    });
    let duration = parsed
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok())
        .map(|secs| Millis((secs * 1000.0).round() as i64));

    Ok(MediaInfo {
        duration,
        has_audio,
        has_video,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_audio_stream_and_attached_pic_video_with_duration() {
        let json = br#"{
            "streams": [
                {"codec_type": "audio", "index": 0},
                {"codec_type": "video", "index": 1, "disposition": {"attached_pic": 1}}
            ],
            "format": {"duration": "12.345"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(
            info,
            MediaInfo {
                duration: Some(Millis(12_345)),
                has_audio: true,
                has_video: false,
            }
        );
    }

    #[test]
    fn real_video_stream_without_attached_pic_counts_as_video() {
        let json = br#"{
            "streams": [
                {"codec_type": "audio"},
                {"codec_type": "video", "disposition": {"attached_pic": 0}}
            ],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert!(info.has_video);
    }

    #[test]
    fn video_stream_without_disposition_counts_as_video() {
        let json = br#"{
            "streams": [{"codec_type": "video"}],
            "format": {}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert!(info.has_video);
    }

    #[test]
    fn missing_duration_field_is_none() {
        let json = br#"{"streams": [{"codec_type": "audio"}], "format": {}}"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.duration, None);
    }

    #[test]
    fn missing_format_object_entirely_is_none_duration() {
        let json = br#"{"streams": [{"codec_type": "audio"}]}"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.duration, None);
        assert!(info.has_audio);
        assert!(!info.has_video);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_probe_json(b"not json").is_err());
    }

    #[test]
    fn with_binary_overrides_the_resolved_path() {
        let prober = FfprobeProber::with_binary("custom-ffprobe");
        assert_eq!(prober.binary, PathBuf::from("custom-ffprobe"));
    }
}
