//! Core domain types ported from Buzz's `transcriber.py` (`Task`, `Segment`,
//! `OutputFormat`, `WhisperModelSize`, `TranscriptionOptions`), plus the
//! job/output wrappers that carry them through the pipeline. Timestamps are
//! integer milliseconds everywhere (`docs/design/ARCHITECTURE.md` §4).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;

use crate::language::Language;

/// A point in time or a duration, expressed in whole milliseconds.
///
/// Newtype to avoid unit confusion; whisper.cpp reports centiseconds, which
/// adapters convert to milliseconds at the boundary (`ms = cs * 10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Millis(
    /// The millisecond value.
    pub i64,
);

impl std::ops::Add for Millis {
    type Output = Millis;

    fn add(self, rhs: Millis) -> Millis {
        Millis(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Millis {
    type Output = Millis;

    fn sub(self, rhs: Millis) -> Millis {
        Millis(self.0 - rhs.0)
    }
}

/// Error returned when a string doesn't match any known variant of a domain
/// enum (`TaskKind`, `OutputFormat`, `ModelSize`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ParseEnumError(String);

/// A single transcribed span of audio: start/end time, recognized text, and
/// (when translation was requested) the translated text.
///
/// Ported from Buzz's `Segment` dataclass (`transcriber.py:29-34`).
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Start time of the segment.
    pub start: Millis,
    /// End time of the segment.
    pub end: Millis,
    /// Recognized text, in the source (or target, for translation) language.
    pub text: String,
    /// Translated text (English); empty unless translation was requested.
    pub translation: String,
}

/// Whisper task: transcribe in the source language, or translate to English.
///
/// Ported from Buzz's `Task` enum (`transcriber.py:18-20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Produce text in the audio's source language.
    Transcribe,
    /// Translate the audio's speech to English.
    Translate,
}

impl TaskKind {
    /// The Buzz-compatible wire value: `"transcribe"` or `"translate"`.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::Transcribe => "transcribe",
            TaskKind::Translate => "translate",
        }
    }
}

impl FromStr for TaskKind {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "transcribe" => Ok(TaskKind::Transcribe),
            "translate" => Ok(TaskKind::Translate),
            other => Err(ParseEnumError(format!("unknown task: {other:?}"))),
        }
    }
}

/// An export file format.
///
/// Ported from Buzz's `OutputFormat` enum (`transcriber.py:212-215`). Ordered
/// `Txt < Srt < Vtt` (declaration order) so it can live in a `BTreeSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutputFormat {
    /// Plain text.
    Txt,
    /// SubRip subtitles.
    Srt,
    /// WebVTT subtitles.
    Vtt,
}

impl OutputFormat {
    /// The Buzz-compatible wire value: `"txt"`, `"srt"`, or `"vtt"`.
    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Txt => "txt",
            OutputFormat::Srt => "srt",
            OutputFormat::Vtt => "vtt",
        }
    }

    /// The file extension for this format (identical to [`OutputFormat::as_str`],
    /// named separately for use when building output file names).
    pub fn extension(self) -> &'static str {
        self.as_str()
    }
}

impl FromStr for OutputFormat {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "txt" => Ok(OutputFormat::Txt),
            "srt" => Ok(OutputFormat::Srt),
            "vtt" => Ok(OutputFormat::Vtt),
            other => Err(ParseEnumError(format!("unknown output format: {other:?}"))),
        }
    }
}

/// Whisper model size (parity set), plus humm's `Custom` and `Lumii` variants.
///
/// Ported from Buzz's `WhisperModelSize` enum (`model_loader.py:128-142`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSize {
    /// `tiny`.
    Tiny,
    /// `tiny.en` (English-only).
    TinyEn,
    /// `base`.
    Base,
    /// `base.en` (English-only).
    BaseEn,
    /// `small`.
    Small,
    /// `small.en` (English-only).
    SmallEn,
    /// `medium`.
    Medium,
    /// `medium.en` (English-only).
    MediumEn,
    /// `large` (v1).
    Large,
    /// `large-v2`.
    LargeV2,
    /// `large-v3`.
    LargeV3,
    /// `large-v3-turbo`.
    LargeV3Turbo,
    /// A user-supplied custom model.
    Custom,
    /// The Lumii model.
    Lumii,
}

impl ModelSize {
    /// The Buzz-compatible wire value, e.g. `"tiny.en"`, `"large-v3-turbo"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ModelSize::Tiny => "tiny",
            ModelSize::TinyEn => "tiny.en",
            ModelSize::Base => "base",
            ModelSize::BaseEn => "base.en",
            ModelSize::Small => "small",
            ModelSize::SmallEn => "small.en",
            ModelSize::Medium => "medium",
            ModelSize::MediumEn => "medium.en",
            ModelSize::Large => "large",
            ModelSize::LargeV2 => "large-v2",
            ModelSize::LargeV3 => "large-v3",
            ModelSize::LargeV3Turbo => "large-v3-turbo",
            ModelSize::Custom => "custom",
            ModelSize::Lumii => "lumii",
        }
    }

    /// The model file name whisper.cpp expects for this size.
    ///
    /// `Large` maps to `"large-v1"` (whisper.cpp's naming for the first large
    /// model predates the `-v2`/`-v3` suffixes); every other variant matches
    /// [`ModelSize::as_str`] (`model_loader.py:149-152`).
    pub fn ggml_name(self) -> &'static str {
        match self {
            ModelSize::Large => "large-v1",
            other => other.as_str(),
        }
    }
}

impl FromStr for ModelSize {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tiny" => Ok(ModelSize::Tiny),
            "tiny.en" => Ok(ModelSize::TinyEn),
            "base" => Ok(ModelSize::Base),
            "base.en" => Ok(ModelSize::BaseEn),
            "small" => Ok(ModelSize::Small),
            "small.en" => Ok(ModelSize::SmallEn),
            "medium" => Ok(ModelSize::Medium),
            "medium.en" => Ok(ModelSize::MediumEn),
            "large" => Ok(ModelSize::Large),
            "large-v2" => Ok(ModelSize::LargeV2),
            "large-v3" => Ok(ModelSize::LargeV3),
            "large-v3-turbo" => Ok(ModelSize::LargeV3Turbo),
            "custom" => Ok(ModelSize::Custom),
            "lumii" => Ok(ModelSize::Lumii),
            other => Err(ParseEnumError(format!("unknown model size: {other:?}"))),
        }
    }
}

/// The default fallback temperature schedule whisper.cpp retries decoding at
/// when a segment fails quality checks.
///
/// Ported verbatim from Buzz's `DEFAULT_WHISPER_TEMPERATURE` (`transcriber.py:15`).
pub const DEFAULT_TEMPERATURE: [f32; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];

/// Which model to run inference with.
///
/// `backend` and `custom` fields (HF repo id, ggml URL) arrive in M2/M6
/// (ADR-008); only the size is modeled here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpec {
    /// The model size to load.
    pub size: ModelSize,
}

/// Full set of options for one transcription run, persisted in full with every
/// queued job (fixes Buzz's restart-loses-options bug structurally).
///
/// Ported from Buzz's `TranscriptionOptions` dataclass (`transcriber.py:141-158`).
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionOptions {
    /// Force a specific source language, or `None` to auto-detect.
    pub language: Option<Language>,
    /// Transcribe or translate-to-English.
    pub task: TaskKind,
    /// The model to run inference with.
    pub model: ModelSpec,
    /// Whether to request word-level timestamps.
    pub word_timestamps: bool,
    /// Optional priming text passed to the model.
    pub initial_prompt: String,
    /// Fallback temperature schedule.
    pub temperature: Vec<f32>,
}

impl Default for TranscriptionOptions {
    /// Matches Buzz's dataclass defaults (`transcriber.py:141-158`) with one
    /// deliberate deviation: the default model size is `Small`, not Buzz's
    /// `Tiny` (`UX_SPEC.md` §6 — `tiny` optimizes first-run speed at a quality
    /// cost humm judges not worth it).
    fn default() -> Self {
        TranscriptionOptions {
            language: None,
            task: TaskKind::Transcribe,
            model: ModelSpec {
                size: ModelSize::Small,
            },
            word_timestamps: false,
            initial_prompt: String::new(),
            temperature: DEFAULT_TEMPERATURE.to_vec(),
        }
    }
}

/// Where and how to write transcription results.
///
/// Ported from Buzz's `FileTranscriptionOptions.output_formats` plus the
/// filename-templating and paragraph-splitting settings threaded through
/// Buzz's exporter.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputSpec {
    /// Which formats to export.
    pub formats: BTreeSet<OutputFormat>,
    /// Output directory; `None` writes alongside the input file.
    pub directory: Option<PathBuf>,
    /// Output file name template; `None` uses the default template.
    pub file_name_template: Option<String>,
    /// Maximum gap between segments before starting a new paragraph on export.
    pub paragraph_split: Millis,
}

impl Default for OutputSpec {
    fn default() -> Self {
        OutputSpec {
            formats: BTreeSet::new(),
            directory: None,
            file_name_template: None,
            paragraph_split: Millis(2000),
        }
    }
}

/// A single unit of work: transcribe `input` per `options`, writing `output`.
#[derive(Debug, Clone, PartialEq)]
pub struct JobSpec {
    /// Path to the input media file.
    pub input: PathBuf,
    /// Transcription options for this job.
    pub options: TranscriptionOptions,
    /// Where and how to write the results.
    pub output: OutputSpec,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn millis_subtraction() {
        assert_eq!(Millis(3000) - Millis(1000), Millis(2000));
    }

    #[test]
    fn millis_addition() {
        assert_eq!(Millis(1000) + Millis(500), Millis(1500));
    }

    #[test]
    fn millis_ordering() {
        assert!(Millis(1000) < Millis(2000));
        assert!(Millis(2000) > Millis(1000));
        assert_eq!(Millis(500), Millis(500));
    }

    const ALL_TASK_KINDS: [TaskKind; 2] = [TaskKind::Transcribe, TaskKind::Translate];

    #[test]
    fn task_kind_round_trips_through_from_str() {
        for kind in ALL_TASK_KINDS {
            assert_eq!(TaskKind::from_str(kind.as_str()).unwrap(), kind);
        }
    }

    #[test]
    fn task_kind_as_str_matches_buzz_wire_values() {
        assert_eq!(TaskKind::Transcribe.as_str(), "transcribe");
        assert_eq!(TaskKind::Translate.as_str(), "translate");
    }

    #[test]
    fn task_kind_unknown_string_is_err() {
        assert!(TaskKind::from_str("bogus").is_err());
    }

    const ALL_OUTPUT_FORMATS: [OutputFormat; 3] =
        [OutputFormat::Txt, OutputFormat::Srt, OutputFormat::Vtt];

    #[test]
    fn output_format_round_trips_through_from_str() {
        for format in ALL_OUTPUT_FORMATS {
            assert_eq!(OutputFormat::from_str(format.as_str()).unwrap(), format);
            assert_eq!(OutputFormat::from_str(format.extension()).unwrap(), format);
        }
    }

    #[test]
    fn output_format_as_str_matches_buzz_wire_values() {
        assert_eq!(OutputFormat::Txt.as_str(), "txt");
        assert_eq!(OutputFormat::Srt.as_str(), "srt");
        assert_eq!(OutputFormat::Vtt.as_str(), "vtt");
    }

    #[test]
    fn output_format_unknown_string_is_err() {
        assert!(OutputFormat::from_str("bogus").is_err());
    }

    #[test]
    fn output_format_ordering() {
        assert!(OutputFormat::Txt < OutputFormat::Srt);
        assert!(OutputFormat::Srt < OutputFormat::Vtt);
        assert!(OutputFormat::Txt < OutputFormat::Vtt);
    }

    const ALL_MODEL_SIZES: [ModelSize; 14] = [
        ModelSize::Tiny,
        ModelSize::TinyEn,
        ModelSize::Base,
        ModelSize::BaseEn,
        ModelSize::Small,
        ModelSize::SmallEn,
        ModelSize::Medium,
        ModelSize::MediumEn,
        ModelSize::Large,
        ModelSize::LargeV2,
        ModelSize::LargeV3,
        ModelSize::LargeV3Turbo,
        ModelSize::Custom,
        ModelSize::Lumii,
    ];

    #[test]
    fn model_size_round_trips_through_from_str() {
        for size in ALL_MODEL_SIZES {
            assert_eq!(ModelSize::from_str(size.as_str()).unwrap(), size);
        }
    }

    #[test]
    fn model_size_as_str_matches_buzz_wire_values() {
        let expected = [
            (ModelSize::Tiny, "tiny"),
            (ModelSize::TinyEn, "tiny.en"),
            (ModelSize::Base, "base"),
            (ModelSize::BaseEn, "base.en"),
            (ModelSize::Small, "small"),
            (ModelSize::SmallEn, "small.en"),
            (ModelSize::Medium, "medium"),
            (ModelSize::MediumEn, "medium.en"),
            (ModelSize::Large, "large"),
            (ModelSize::LargeV2, "large-v2"),
            (ModelSize::LargeV3, "large-v3"),
            (ModelSize::LargeV3Turbo, "large-v3-turbo"),
            (ModelSize::Custom, "custom"),
            (ModelSize::Lumii, "lumii"),
        ];
        for (size, expected_str) in expected {
            assert_eq!(size.as_str(), expected_str);
        }
    }

    #[test]
    fn model_size_unknown_string_is_err() {
        assert!(ModelSize::from_str("bogus").is_err());
    }

    #[test]
    fn model_size_ggml_name() {
        assert_eq!(ModelSize::Large.ggml_name(), "large-v1");
        assert_eq!(ModelSize::LargeV3Turbo.ggml_name(), "large-v3-turbo");
        assert_eq!(ModelSize::Small.ggml_name(), "small");
    }

    #[test]
    fn transcription_options_default_matches_buzz_with_documented_deviation() {
        let opts = TranscriptionOptions::default();
        assert_eq!(
            opts,
            TranscriptionOptions {
                language: None,
                task: TaskKind::Transcribe,
                model: ModelSpec {
                    size: ModelSize::Small,
                },
                word_timestamps: false,
                initial_prompt: String::new(),
                temperature: DEFAULT_TEMPERATURE.to_vec(),
            }
        );
    }

    #[test]
    fn output_spec_default_is_empty_with_two_second_paragraph_split() {
        let spec = OutputSpec::default();
        assert_eq!(
            spec,
            OutputSpec {
                formats: BTreeSet::new(),
                directory: None,
                file_name_template: None,
                paragraph_split: Millis(2000),
            }
        );
    }
}
