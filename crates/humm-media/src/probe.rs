//! ffprobe-backed [`MediaProber`] (ADR-003).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use humm_core::boundary::{MediaInfo, MediaProber, VideoStreamInfo};
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
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    #[serde(default)]
    duration: Option<String>,
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
/// converted to rounded milliseconds, falling back to the video stream's own
/// `duration` when the format-level one is absent
/// (`ffmpeg_video_player.py:49-66`). `has_video`/`video` both exclude streams
/// whose `disposition.attached_pic == 1` — embedded cover art must not count
/// as a video stream.
fn parse_probe_json(bytes: &[u8]) -> Result<MediaInfo, String> {
    let parsed: FfprobeOutput = serde_json::from_slice(bytes).map_err(|err| err.to_string())?;

    let has_audio = parsed.streams.iter().any(|s| s.codec_type == "audio");
    let video_stream = parsed.streams.iter().find(|s| {
        s.codec_type == "video" && s.disposition.as_ref().map(|d| d.attached_pic).unwrap_or(0) != 1
    });
    let has_video = video_stream.is_some();

    let video = video_stream.map(|stream| VideoStreamInfo {
        width: stream.width.unwrap_or(0),
        height: stream.height.unwrap_or(0),
        // Floor to 1.0 — Buzz applies this at the player, not the prober
        // (`ffmpeg_video_player.py:236`); folded in here since humm has no
        // separate player-info layer.
        fps: parse_frame_rate(stream.r_frame_rate.as_deref()).max(1.0),
    });

    let format_duration_secs = parsed
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(|d| d.parse::<f64>().ok());
    let duration_secs = format_duration_secs.or_else(|| {
        video_stream
            .and_then(|s| s.duration.as_deref())
            .and_then(|d| d.parse::<f64>().ok())
    });
    let duration = duration_secs.map(|secs| Millis((secs * 1000.0).round() as i64));

    Ok(MediaInfo {
        duration,
        has_audio,
        has_video,
        video,
    })
}

/// Parses ffprobe's `r_frame_rate` (`"n/d"`) into an fps float, falling back
/// to `25.0` (Buzz's default) on a missing, malformed, or zero-denominator
/// value (`ffmpeg_video_player.py:39-75`; the floor to `1.0` is applied by
/// the caller).
fn parse_frame_rate(r_frame_rate: Option<&str>) -> f64 {
    r_frame_rate
        .and_then(|rate| {
            let (n, d) = rate.split_once('/')?;
            let n: f64 = n.trim().parse().ok()?;
            let d: f64 = d.trim().parse().ok()?;
            if d == 0.0 { None } else { Some(n / d) }
        })
        .unwrap_or(25.0)
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
                video: None,
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

    // -- Video stream parsing matrix (F-083) --------------------------------

    #[test]
    fn video_stream_reports_width_height_and_fractional_fps() {
        let json = br#"{
            "streams": [{
                "codec_type": "video",
                "width": 1920,
                "height": 1080,
                "r_frame_rate": "30000/1001"
            }],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        let video = info.video.expect("expected video stream info");
        assert_eq!(video.width, 1920);
        assert_eq!(video.height, 1080);
        assert!(
            (video.fps - 29.970_029_970_029_97).abs() < 1e-9,
            "expected ~29.97 fps, got {}",
            video.fps
        );
    }

    #[test]
    fn malformed_frame_rate_falls_back_to_25_fps() {
        let json = br#"{
            "streams": [{"codec_type": "video", "r_frame_rate": "not-a-rate"}],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.video.expect("video info").fps, 25.0);
    }

    #[test]
    fn missing_frame_rate_falls_back_to_25_fps() {
        let json = br#"{
            "streams": [{"codec_type": "video"}],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.video.expect("video info").fps, 25.0);
    }

    #[test]
    fn zero_denominator_frame_rate_falls_back_to_25_fps() {
        let json = br#"{
            "streams": [{"codec_type": "video", "r_frame_rate": "0/0"}],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.video.expect("video info").fps, 25.0);
    }

    #[test]
    fn sub_one_fps_is_floored_to_1_0() {
        let json = br#"{
            "streams": [{"codec_type": "video", "r_frame_rate": "1/2"}],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.video.expect("video info").fps, 1.0);
    }

    #[test]
    fn audio_only_file_has_no_video_stream_info() {
        let json = br#"{
            "streams": [{"codec_type": "audio"}],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert!(!info.has_video);
        assert_eq!(info.video, None);
    }

    #[test]
    fn cover_art_attached_pic_stream_is_not_video_info() {
        let json = br#"{
            "streams": [{
                "codec_type": "video",
                "disposition": {"attached_pic": 1},
                "width": 500,
                "height": 500
            }],
            "format": {"duration": "1.0"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert!(!info.has_video);
        assert_eq!(info.video, None);
    }

    #[test]
    fn duration_falls_back_to_video_stream_duration_when_format_level_absent() {
        let json = br#"{
            "streams": [{
                "codec_type": "video",
                "width": 320,
                "height": 240,
                "r_frame_rate": "25/1",
                "duration": "2.000000"
            }],
            "format": {}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.duration, Some(Millis(2000)));
    }

    #[test]
    fn format_level_duration_wins_over_video_stream_duration_when_present() {
        let json = br#"{
            "streams": [{
                "codec_type": "video",
                "r_frame_rate": "25/1",
                "duration": "2.000000"
            }],
            "format": {"duration": "5.5"}
        }"#;
        let info = parse_probe_json(json).expect("should parse");
        assert_eq!(info.duration, Some(Millis(5500)));
    }

    #[test]
    fn with_binary_overrides_the_resolved_path() {
        let prober = FfprobeProber::with_binary("custom-ffprobe");
        assert_eq!(prober.binary, PathBuf::from("custom-ffprobe"));
    }
}
