//! Core domain types ported from Buzz's `transcriber.py` (`Task`, `Segment`,
//! `OutputFormat`, `WhisperModelSize`, `TranscriptionOptions`), plus the
//! job/output wrappers that carry them through the pipeline. Timestamps are
//! integer milliseconds everywhere (`docs/design/ARCHITECTURE.md` §4).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::language::Language;

/// A point in time or a duration, expressed in whole milliseconds.
///
/// Newtype to avoid unit confusion; whisper.cpp reports centiseconds, which
/// adapters convert to milliseconds at the boundary (`ms = cs * 10`).
///
/// Serializes as a bare JSON integer (`#[serde(transparent)]`), not a
/// single-element object or array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
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
/// enum (`TaskKind`, `OutputFormat`, `ModelSize`, `Status`, `Source`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ParseEnumError(String);

/// A stable identifier for one queued/running/completed transcription task.
///
/// Wraps a random (v4) UUID — Buzz's `FileTranscriptionTask.uid`
/// (`transcriber.py:195`); Buzz also keeps a deprecated `id: int`, which humm
/// does not port. Serializes as its hyphenated lowercase string form.
///
/// `PartialOrd`/`Ord` (byte order of the underlying UUID — not creation
/// order) are derived so ids can key a `BTreeSet`/`BTreeMap`, e.g. the GUI
/// task table's multi-row selection (`humm-gui`'s `TasksVm::selection`); no
/// code relies on the ordering being anything other than an arbitrary total
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TranscriptionId(uuid::Uuid);

impl TranscriptionId {
    /// Generates a new, random (v4) id. Two calls never produce the same id.
    ///
    /// No `Default` impl is provided: `new()` mints a fresh, random identity
    /// on every call, so there is no meaningful "default" value to return
    /// (unlike `uuid::Uuid::default()`, which is the fixed nil UUID) — a
    /// `Default` impl here would either be non-deterministic (violating
    /// `T::default() == T::default()`) or silently wrong (a nil id).
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        TranscriptionId(uuid::Uuid::new_v4())
    }

    /// Parses an id from its hyphenated string form (as produced by this
    /// type's `Display`); `None` if `s` is not a valid UUID.
    pub fn parse(s: &str) -> Option<Self> {
        uuid::Uuid::parse_str(s).ok().map(TranscriptionId)
    }
}

impl std::fmt::Display for TranscriptionId {
    /// Hyphenated, lowercase form, e.g. `550e8400-e29b-41d4-a716-446655440000`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl Serialize for TranscriptionId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TranscriptionId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        TranscriptionId::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid transcription id: {s:?}")))
    }
}

/// A single transcribed span of audio: start/end time, recognized text, and
/// (when translation was requested) the translated text.
///
/// Ported from Buzz's `Segment` dataclass (`transcriber.py:29-34`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

// `ModelSize`'s wire values (`"tiny.en"`, `"large-v3-turbo"`) mix `.` and `-`
// separators that no `#[serde(rename_all = ...)]` case convention produces,
// so serialize/deserialize manually through `as_str`/`FromStr`.
impl Serialize for ModelSize {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ModelSize {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ModelSize::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Lifecycle status of a queued or running transcription task.
///
/// Ported from Buzz's `FileTranscriptionTask.Status` enum (`transcriber.py:177-183`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Waiting in the queue; not yet started.
    Queued,
    /// Currently transcribing.
    InProgress,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Canceled by the user before completion.
    Canceled,
    /// Skipped without running (e.g. its input no longer existed at run time).
    Skipped,
}

impl Status {
    /// The Buzz-compatible wire value, e.g. `"in_progress"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::InProgress => "in_progress",
            Status::Completed => "completed",
            Status::Failed => "failed",
            Status::Canceled => "canceled",
            Status::Skipped => "skipped",
        }
    }
}

impl FromStr for Status {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(Status::Queued),
            "in_progress" => Ok(Status::InProgress),
            "completed" => Ok(Status::Completed),
            "failed" => Ok(Status::Failed),
            "canceled" => Ok(Status::Canceled),
            "skipped" => Ok(Status::Skipped),
            other => Err(ParseEnumError(format!("unknown status: {other:?}"))),
        }
    }
}

/// Where a transcription task's input came from.
///
/// Ported from Buzz's `FileTranscriptionTask.Source` enum (`transcriber.py:185-188`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Imported from a local file the user picked or dropped.
    FileImport,
    /// Imported from a remote URL (e.g. a YouTube link).
    UrlImport,
    /// Picked up automatically by folder-watch.
    FolderWatch,
    /// Captured from a live microphone recording. This is humm's own
    /// addition (Q-10 in `docs/OPEN_QUESTIONS.md`), not present in Buzz;
    /// unused until live recording ships in M5.
    LiveRecording,
}

impl Source {
    /// The Buzz-compatible wire value, e.g. `"file_import"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::FileImport => "file_import",
            Source::UrlImport => "url_import",
            Source::FolderWatch => "folder_watch",
            Source::LiveRecording => "live_recording",
        }
    }
}

impl FromStr for Source {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "file_import" => Ok(Source::FileImport),
            "url_import" => Ok(Source::UrlImport),
            "folder_watch" => Ok(Source::FolderWatch),
            "live_recording" => Ok(Source::LiveRecording),
            other => Err(ParseEnumError(format!("unknown source: {other:?}"))),
        }
    }
}

/// The default fallback temperature schedule whisper.cpp retries decoding at
/// when a segment fails quality checks.
///
/// Ported verbatim from Buzz's `DEFAULT_WHISPER_TEMPERATURE` (`transcriber.py:15`).
pub const DEFAULT_TEMPERATURE: [f32; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];

/// Which transcription backend a [`ModelSpec`] runs on (F-041, design spec
/// §2.1): local whisper.cpp inference, or a remote OpenAI-compatible HTTP
/// API. The queue's engine cache is keyed by `ModelSpec` (including this
/// field), so the two backends cache their engines independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Local whisper.cpp inference via `humm-whisper`. The default: every
    /// `options_json` row persisted before M6 has no `backend` key at all,
    /// and `#[serde(default)]` on [`ModelSpec::backend`] must resolve those
    /// rows to this variant, since it was the only backend that existed
    /// then.
    #[default]
    Whisper,
    /// A remote OpenAI-compatible HTTP API (`humm_core::openai::OpenAiEngine`,
    /// landing in a later M6 task). Wire value `"openai_api"` — the derived
    /// `snake_case` rule would otherwise split `OpenAiApi` as
    /// `"open_ai_api"`, so this variant is renamed explicitly.
    #[serde(rename = "openai_api")]
    OpenAiApi,
}

impl Backend {
    /// The wire value persisted in `settings.file_transcriber.backend`
    /// (F-023 extension, design spec §7.2) — deliberately the same strings
    /// as the `#[serde]` rename above (`"whisper"`/`"openai_api"`), so the
    /// settings TOML and `options_json` never disagree on spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Whisper => "whisper",
            Backend::OpenAiApi => "openai_api",
        }
    }
}

impl FromStr for Backend {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "whisper" => Ok(Backend::Whisper),
            "openai_api" => Ok(Backend::OpenAiApi),
            other => Err(ParseEnumError(format!("unknown backend: {other:?}"))),
        }
    }
}

/// Which model to run inference with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Which backend runs inference for this spec (F-041).
    /// `#[serde(default)]` resolves every pre-M6 `options_json` row — which
    /// has no `backend` key — to [`Backend::Whisper`].
    #[serde(default)]
    pub backend: Backend,
    /// The model size to load.
    pub size: ModelSize,
    /// Hugging Face repo id, set when the model was selected by repo (`humm
    /// add -m huggingface --hfid <repo>`) rather than by [`ModelSize`];
    /// `None` for the built-in sizes.
    pub hf_repo: Option<String>,
}

/// LLM-backed translation options for a transcription task (F-031, design
/// spec §2.1). Presence of a value on [`TranscriptionOptions::llm`] means
/// "translate each produced segment with this model/prompt"; absence means
/// no translation happens. Ported from Buzz's `translator.py` batch
/// protocol (model/prompt shape only) — deliberately has no API key field:
/// the key travels only through `humm_core::openai` constructor arguments,
/// never through a serialized domain type (see design spec §2.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmTranslationOptions {
    /// The model id to request translations from (e.g. `"gpt-4o-mini"`).
    pub model: String,
    /// System/instruction prompt steering the translation.
    pub prompt: String,
}

/// Full set of options for one transcription run, persisted in full with every
/// queued job (fixes Buzz's restart-loses-options bug structurally).
///
/// Ported from Buzz's `TranscriptionOptions` dataclass (`transcriber.py:141-158`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Optional per-task LLM translation (F-031): presence means "translate
    /// every produced segment"; `None` means no translation happens.
    /// `#[serde(default)]` resolves pre-M6 `options_json` rows (no `llm`
    /// key) to `None`.
    #[serde(default)]
    pub llm: Option<LlmTranslationOptions>,
    /// Whether to isolate speech from background noise/music with demucs
    /// before transcription (F-027, design spec §3): `true` runs the
    /// decode input through a [`crate::boundary`]-defined `SpeechIsolator`
    /// first (both binaries wire a real one — see that trait's doc).
    /// `#[serde(default)]` resolves every pre-M8 `options_json` row — which
    /// has no `extract_speech` key at all — to `false`.
    #[serde(default)]
    pub extract_speech: bool,
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
                backend: Backend::Whisper,
                size: ModelSize::Small,
                hf_repo: None,
            },
            word_timestamps: false,
            initial_prompt: String::new(),
            temperature: DEFAULT_TEMPERATURE.to_vec(),
            llm: None,
            extract_speech: false,
        }
    }
}

/// What happens to a folder-watch task's source file once every requested
/// export has been written (F-030, design spec §3.4).
///
/// Ported from Buzz's `_handle_folder_watch` (`file_transcriber.py:155-170`),
/// which always removes the source from the watched input tree — either
/// deleting it outright or moving it into the output directory — driven by
/// the folder-watch tab's "delete processed files" checkbox. Buzz never
/// leaves the source in place for a folder-watch task; `None` on
/// [`OutputSpec::source_disposition`] is humm's way of saying "this job did
/// not come from folder watch" (or, pre-M7, that the field didn't exist yet),
/// not a third disposition choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDisposition {
    /// Remove the source file outright (`os.remove`, `file_transcriber.py:162`).
    Delete,
    /// Move the source file into the export directory, under its own
    /// basename (`shutil.move`, `file_transcriber.py:163-169`).
    MoveToOutput,
}

/// Where and how to write transcription results.
///
/// Ported from Buzz's `FileTranscriptionOptions.output_formats` plus the
/// filename-templating and paragraph-splitting settings threaded through
/// Buzz's exporter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputSpec {
    /// Which formats to export.
    pub formats: BTreeSet<OutputFormat>,
    /// Output directory; `None` writes alongside the input file.
    pub directory: Option<PathBuf>,
    /// Output file name template; `None` uses the default template.
    pub file_name_template: Option<String>,
    /// Maximum gap between segments before starting a new paragraph on export.
    pub paragraph_split: Millis,
    /// What to do with the input file after every requested export has been
    /// written (F-030): delete it, move it into the export directory, or (the
    /// default, `None`) leave it alone — this job isn't a folder-watch task.
    /// `#[serde(default)]` resolves every pre-M7 `options_json` row — which
    /// has no `source_disposition` key at all — to `None`.
    #[serde(default)]
    pub source_disposition: Option<SourceDisposition>,
}

impl Default for OutputSpec {
    fn default() -> Self {
        OutputSpec {
            formats: BTreeSet::new(),
            directory: None,
            file_name_template: None,
            paragraph_split: Millis(2000),
            source_disposition: None,
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
    /// Raw URL title used as `{{ input_file_name }}`. `None` for a file
    /// task, which keeps the input stem. Not a path; export sanitizes it.
    pub display_name: Option<String>,
}

/// Where a transcription task reads its media from: a local file or a remote
/// URL.
///
/// Serializes externally tagged with a lowercase key: `{"path": "..."}` or
/// `{"url": "..."}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Input {
    /// A local filesystem path.
    Path(PathBuf),
    /// A remote URL (e.g. a direct media link or a `yt-dlp`-resolvable page).
    Url(url::Url),
}

impl Input {
    /// Parses `s` as a URL only if it has both a scheme *and* a host;
    /// otherwise treats it as a filesystem path.
    ///
    /// Mirrors Buzz's `is_url` (`cli.py:48-50`), which checks
    /// `urlparse(path)` for a truthy `scheme` *and* `netloc` — so
    /// scheme-only strings like `"a:b"` (an opaque URI with no authority)
    /// fall back to `Path`, matching Buzz.
    pub fn parse(s: &str) -> Input {
        match url::Url::parse(s) {
            Ok(url) if url.has_host() => Input::Url(url),
            _ => Input::Path(PathBuf::from(s)),
        }
    }

    /// A human-readable label for this input: the full path display for
    /// [`Input::Path`] (not just the file stem), or the full URL string for
    /// [`Input::Url`].
    pub fn display_name(&self) -> String {
        match self {
            Input::Path(path) => path.display().to_string(),
            Input::Url(url) => url.to_string(),
        }
    }
}

/// A single unit of work bound for the task queue: transcribe `input` per
/// `options`, writing `output`.
///
/// Unlike [`JobSpec`] (a plain-path job the M1 pipeline runs directly),
/// `TaskSpec` accepts a [`url::Url`] input for the queued CLI/GUI task list;
/// its serde form is what the M2 SQLite store persists as `options_json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSpec {
    /// The media to transcribe: a local file or a remote URL.
    pub input: Input,
    /// Transcription options for this task.
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
                    backend: Backend::Whisper,
                    size: ModelSize::Small,
                    hf_repo: None,
                },
                word_timestamps: false,
                initial_prompt: String::new(),
                temperature: DEFAULT_TEMPERATURE.to_vec(),
                llm: None,
                extract_speech: false,
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
                source_disposition: None,
            }
        );
    }

    // -- Status / Source (F-024, F-029 groundwork) ------------------------

    const ALL_STATUSES: [Status; 6] = [
        Status::Queued,
        Status::InProgress,
        Status::Completed,
        Status::Failed,
        Status::Canceled,
        Status::Skipped,
    ];

    #[test]
    fn status_round_trips_through_from_str() {
        for status in ALL_STATUSES {
            assert_eq!(Status::from_str(status.as_str()).unwrap(), status);
        }
    }

    #[test]
    fn status_as_str_matches_buzz_wire_values() {
        assert_eq!(Status::Queued.as_str(), "queued");
        assert_eq!(Status::InProgress.as_str(), "in_progress");
        assert_eq!(Status::Completed.as_str(), "completed");
        assert_eq!(Status::Failed.as_str(), "failed");
        assert_eq!(Status::Canceled.as_str(), "canceled");
        assert_eq!(Status::Skipped.as_str(), "skipped");
    }

    #[test]
    fn status_unknown_string_is_err() {
        assert!(Status::from_str("bogus").is_err());
    }

    const ALL_SOURCES: [Source; 4] = [
        Source::FileImport,
        Source::UrlImport,
        Source::FolderWatch,
        Source::LiveRecording,
    ];

    #[test]
    fn source_round_trips_through_from_str() {
        for source in ALL_SOURCES {
            assert_eq!(Source::from_str(source.as_str()).unwrap(), source);
        }
    }

    #[test]
    fn source_as_str_matches_buzz_wire_values() {
        assert_eq!(Source::FileImport.as_str(), "file_import");
        assert_eq!(Source::UrlImport.as_str(), "url_import");
        assert_eq!(Source::FolderWatch.as_str(), "folder_watch");
        assert_eq!(Source::LiveRecording.as_str(), "live_recording");
    }

    #[test]
    fn source_unknown_string_is_err() {
        assert!(Source::from_str("bogus").is_err());
    }

    // -- Backend (F-041, design spec §2.1/§7.2) ----------------------------

    const ALL_BACKENDS: [Backend; 2] = [Backend::Whisper, Backend::OpenAiApi];

    #[test]
    fn backend_round_trips_through_from_str() {
        for backend in ALL_BACKENDS {
            assert_eq!(Backend::from_str(backend.as_str()).unwrap(), backend);
        }
    }

    #[test]
    fn backend_as_str_matches_the_serde_wire_values() {
        assert_eq!(Backend::Whisper.as_str(), "whisper");
        assert_eq!(Backend::OpenAiApi.as_str(), "openai_api");
    }

    #[test]
    fn backend_unknown_string_is_err() {
        assert!(Backend::from_str("bogus").is_err());
    }

    // -- TranscriptionId ---------------------------------------------------

    #[test]
    fn transcription_id_new_twice_are_distinct() {
        assert_ne!(TranscriptionId::new(), TranscriptionId::new());
    }

    #[test]
    fn transcription_id_display_round_trips_through_parse() {
        let id = TranscriptionId::new();
        let text = id.to_string();
        assert_eq!(TranscriptionId::parse(&text), Some(id));
    }

    #[test]
    fn transcription_id_parse_rejects_non_uuid() {
        assert_eq!(TranscriptionId::parse("not-a-uuid"), None);
    }

    // -- Input ---------------------------------------------------------

    #[test]
    fn input_parse_url_with_scheme_and_host() {
        assert_eq!(
            Input::parse("https://example.com/a.mp3"),
            Input::Url(url::Url::parse("https://example.com/a.mp3").unwrap())
        );
        assert_eq!(
            Input::parse("ftp://host/x"),
            Input::Url(url::Url::parse("ftp://host/x").unwrap())
        );
    }

    #[test]
    fn input_parse_falls_back_to_path_without_scheme_and_host() {
        assert_eq!(
            Input::parse("audio.mp3"),
            Input::Path(PathBuf::from("audio.mp3"))
        );
        assert_eq!(
            Input::parse("/abs/path.mp3"),
            Input::Path(PathBuf::from("/abs/path.mp3"))
        );
        // Scheme present, no host: Buzz's `is_url` requires both.
        assert_eq!(Input::parse("a:b"), Input::Path(PathBuf::from("a:b")));
    }

    #[test]
    fn input_display_name() {
        assert_eq!(
            Input::Path(PathBuf::from("/a/b/audio.mp3")).display_name(),
            "/a/b/audio.mp3"
        );
        let url = url::Url::parse("https://example.com/a.mp3").unwrap();
        assert_eq!(
            Input::Url(url.clone()).display_name(),
            "https://example.com/a.mp3"
        );
    }

    // -- Serde wire format --------------------------------------------------

    #[test]
    fn input_path_serializes_as_tagged_lowercase_json_and_round_trips() {
        let input = Input::Path(PathBuf::from("/x.mp3"));

        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, r#"{"path":"/x.mp3"}"#);

        let round_tripped: Input = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, input);
    }

    #[test]
    fn model_size_serializes_to_buzz_wire_values() {
        assert_eq!(
            serde_json::to_string(&ModelSize::LargeV3Turbo).unwrap(),
            "\"large-v3-turbo\""
        );
        assert_eq!(
            serde_json::to_string(&ModelSize::TinyEn).unwrap(),
            "\"tiny.en\""
        );
    }

    #[test]
    fn task_spec_round_trips_through_json() {
        let spec = TaskSpec {
            input: Input::Url(url::Url::parse("https://example.com/audio.mp3").unwrap()),
            options: TranscriptionOptions {
                language: Language::parse("fr"),
                task: TaskKind::Transcribe,
                model: ModelSpec {
                    backend: Backend::Whisper,
                    size: ModelSize::LargeV3Turbo,
                    hf_repo: Some("openai/whisper-large-v3-turbo".to_string()),
                },
                word_timestamps: true,
                initial_prompt: "hello there".to_string(),
                temperature: vec![0.1, 0.3],
                llm: None,
                extract_speech: true,
            },
            output: OutputSpec {
                formats: [OutputFormat::Srt, OutputFormat::Vtt].into_iter().collect(),
                directory: Some(PathBuf::from("/tmp/out")),
                file_name_template: Some("{{ input_file_name }}".to_string()),
                paragraph_split: Millis(1500),
                source_disposition: Some(SourceDisposition::MoveToOutput),
            },
        };

        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"task\":\"transcribe\""));
        assert!(json.contains("\"large-v3-turbo\""));
        assert!(json.contains("\"url\":\"https://example.com/audio.mp3\""));
        assert!(json.contains("\"source_disposition\":\"move_to_output\""));
        assert!(json.contains("\"extract_speech\":true"));

        let round_tripped: TaskSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, spec);
    }

    // -- Backend / LlmTranslationOptions (M6 groundwork, F-041, F-031) -----

    #[test]
    fn backend_default_is_whisper() {
        assert_eq!(Backend::default(), Backend::Whisper);
    }

    #[test]
    fn backend_serializes_to_expected_wire_values() {
        assert_eq!(
            serde_json::to_string(&Backend::Whisper).unwrap(),
            "\"whisper\""
        );
        assert_eq!(
            serde_json::to_string(&Backend::OpenAiApi).unwrap(),
            "\"openai_api\""
        );
    }

    #[test]
    fn backend_round_trips_through_options_json() {
        let mut options = TranscriptionOptions::default();
        options.model.backend = Backend::OpenAiApi;

        let json = serde_json::to_string(&options).unwrap();
        assert!(json.contains("\"backend\":\"openai_api\""));

        let round_tripped: TranscriptionOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, options);
        assert_eq!(round_tripped.model.backend, Backend::OpenAiApi);
    }

    #[test]
    fn llm_options_round_trip() {
        let options = TranscriptionOptions {
            llm: Some(LlmTranslationOptions {
                model: "gpt-4o-mini".to_string(),
                prompt: "Translate to English.".to_string(),
            }),
            ..TranscriptionOptions::default()
        };

        let json = serde_json::to_string(&options).unwrap();
        assert!(json.contains("\"model\":\"gpt-4o-mini\""));
        assert!(json.contains("\"prompt\":\"Translate to English.\""));

        let round_tripped: TranscriptionOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, options);
        assert_eq!(
            round_tripped.llm,
            Some(LlmTranslationOptions {
                model: "gpt-4o-mini".to_string(),
                prompt: "Translate to English.".to_string(),
            })
        );
    }

    #[test]
    fn transcription_options_default_has_no_llm_and_whisper_backend() {
        let opts = TranscriptionOptions::default();
        assert_eq!(opts.llm, None);
        assert_eq!(opts.model.backend, Backend::Whisper);
    }

    #[test]
    fn options_json_without_backend_or_llm_deserializes_to_whisper_none() {
        // A real pre-M6 `TaskSpec` shape (captured from this same test's
        // serializer before the `backend`/`llm` fields existed): no
        // `"backend"` key on `model`, no `"llm"` key on `options`. Every row
        // already sitting in a user's SQLite `options_json` column looks
        // like this — `#[serde(default)]` on both new fields must resolve
        // it to `Backend::Whisper` / `None`, not fail to deserialize.
        let pre_m6_json = r#"{"input":{"path":"/audio/sample.mp3"},"options":{"language":"fr","task":"transcribe","model":{"size":"small","hf_repo":null},"word_timestamps":false,"initial_prompt":"","temperature":[0.0,0.2,0.4,0.6,0.8,1.0]},"output":{"formats":["srt"],"directory":null,"file_name_template":null,"paragraph_split":2000}}"#;

        let spec: TaskSpec = serde_json::from_str(pre_m6_json).expect("pre-M6 row deserializes");
        assert_eq!(spec.options.model.backend, Backend::Whisper);
        assert_eq!(spec.options.llm, None);
    }

    // -- SourceDisposition (M7, F-030) -------------------------------------

    #[test]
    fn output_spec_default_has_no_source_disposition() {
        assert_eq!(OutputSpec::default().source_disposition, None);
    }

    #[test]
    fn source_disposition_round_trips_through_options_json() {
        for (variant, wire) in [
            (SourceDisposition::Delete, "\"delete\""),
            (SourceDisposition::MoveToOutput, "\"move_to_output\""),
        ] {
            let output = OutputSpec {
                source_disposition: Some(variant),
                ..OutputSpec::default()
            };

            let json = serde_json::to_string(&output).unwrap();
            assert!(
                json.contains(&format!("\"source_disposition\":{wire}")),
                "expected {wire} in {json}"
            );

            let round_tripped: OutputSpec = serde_json::from_str(&json).unwrap();
            assert_eq!(round_tripped, output);
            assert_eq!(round_tripped.source_disposition, Some(variant));
        }
    }

    #[test]
    fn old_options_json_without_disposition_deserializes() {
        // A real M6-era `TaskSpec` shape: `backend`/`llm` already exist (M6
        // added them — see `options_json_without_backend_or_llm_deserializes_to_whisper_none`
        // above for the pre-M6 shape), but `output` has no
        // `"source_disposition"` key at all, since M7 is what introduces it.
        // Every `options_json` row persisted before this milestone looks
        // like this — `#[serde(default)]` on the new field must resolve it
        // to `None`, not fail to deserialize.
        let m6_json = r#"{"input":{"path":"/audio/sample.mp3"},"options":{"language":"fr","task":"transcribe","model":{"backend":"whisper","size":"small","hf_repo":null},"word_timestamps":false,"initial_prompt":"","temperature":[0.0,0.2,0.4,0.6,0.8,1.0],"llm":null},"output":{"formats":["srt"],"directory":null,"file_name_template":null,"paragraph_split":2000}}"#;

        let spec: TaskSpec = serde_json::from_str(m6_json).expect("M6-era row deserializes");
        assert_eq!(spec.output.source_disposition, None);
    }

    // -- extract_speech (M8, F-027) -----------------------------------------

    #[test]
    fn old_options_json_without_extract_speech_deserializes() {
        // A real M7-era `TaskSpec` shape: `backend`/`llm` (M6) and
        // `source_disposition` (M7) already exist, but `options` has no
        // `"extract_speech"` key at all, since M8 is what introduces it.
        // Every `options_json` row persisted before this milestone looks
        // like this — `#[serde(default)]` on the new field must resolve it
        // to `false`, not fail to deserialize.
        let m7_json = r#"{"input":{"path":"/audio/sample.mp3"},"options":{"language":"fr","task":"transcribe","model":{"backend":"whisper","size":"small","hf_repo":null},"word_timestamps":false,"initial_prompt":"","temperature":[0.0,0.2,0.4,0.6,0.8,1.0],"llm":null},"output":{"formats":["srt"],"directory":null,"file_name_template":null,"paragraph_split":2000,"source_disposition":null}}"#;

        let spec: TaskSpec = serde_json::from_str(m7_json).expect("M7-era row deserializes");
        assert!(!spec.options.extract_speech);
    }
}
