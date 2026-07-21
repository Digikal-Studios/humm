//! Command-line argument grammar for `humm`.
//!
//! Grammar and flag spellings are Buzz-verbatim for every flag
//! (`buzz/cli.py:52-138`, `docs/analysis/EXTERNAL_SURFACE.md` §2): `-t/--task`,
//! `-s/--model-size`, `-m/--model-type`, `--hfid`, `-l/--language`,
//! `-p/--prompt`, `-w/--word-timestamps`, `-e/--extract-speech`,
//! `--openai-token`, `--srt`/`--vtt`/`--txt`, `-d/--output-directory`,
//! `--hide-gui`, plus file positionals. `--json`/`--quiet` are humm additions
//! (ADR-007, `docs/design/UX_SPEC.md` §5).
//!
//! `run.rs::run_add` validates the semantics clap's grammar can't express
//! (hfid-required-for-huggingface, not-yet-available backends, etc. —
//! `buzz/cli.py:146-147`, `docs/analysis/EXTERNAL_SURFACE.md` §2/§8).
//!
//! Default model size is `small`, not Buzz's `tiny`
//! (`docs/design/UX_SPEC.md` §6, an approved deviation).
//!
//! `-t`/`-s` values are validated by clap itself: [`humm_core::domain::TaskKind`]
//! and [`humm_core::domain::ModelSize`] already implement `FromStr` (returning
//! [`humm_core::domain::ParseEnumError`], which implements `std::error::Error`),
//! so `clap::value_parser!` picks them up directly — an invalid value produces
//! a normal clap usage error, no bespoke validation needed here.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use humm_core::domain::{ModelSize, TaskKind};

/// `humm`: local-first speech transcription.
#[derive(Debug, Parser)]
#[command(name = "humm", version, about = "Local-first speech transcription")]
pub struct Cli {
    /// The command to run. `add` is the only M1 subcommand.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// `humm` subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Transcribe or translate one or more media files.
    Add(AddArgs),
}

/// Model backend, `buzz/cli.py:31-36` (`CommandLineModelType`) — CLI-local:
/// humm-core's `ModelSpec` has no backend field (ADR-008 maps `whisper`,
/// `whispercpp`, and `fasterwhisper` all onto the one local whisper-rs
/// backend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModelTypeArg {
    /// Local whisper-rs backend (openai-whisper-compatible ggml models).
    #[value(name = "whisper")]
    Whisper,
    /// Local whisper-rs backend (whisper.cpp-compatible ggml models).
    #[value(name = "whispercpp")]
    WhisperCpp,
    /// A Hugging Face repo id resolved via `--hfid`.
    #[value(name = "huggingface")]
    HuggingFace,
    /// Local whisper-rs backend (faster-whisper is not ported — ADR-008).
    #[value(name = "fasterwhisper")]
    FasterWhisper,
    /// OpenAI's hosted Whisper API.
    #[value(name = "openaiapi")]
    OpenAiApi,
}

/// Arguments for `humm add` (`buzz/cli.py:52-138`).
#[derive(Debug, Args)]
pub struct AddArgs {
    /// The task to perform.
    #[arg(
        short = 't',
        long = "task",
        default_value = "transcribe",
        value_parser = clap::value_parser!(TaskKind)
    )]
    pub task: TaskKind,

    /// Model type.
    #[arg(
        short = 'm',
        long = "model-type",
        default_value = "whisper",
        value_enum
    )]
    pub model_type: ModelTypeArg,

    /// Model size.
    #[arg(
        short = 's',
        long = "model-size",
        default_value = "small",
        value_parser = clap::value_parser!(ModelSize)
    )]
    pub model_size: ModelSize,

    /// Hugging Face model ID. Use only when --model-type is huggingface.
    #[arg(long = "hfid", value_name = "id")]
    pub hfid: Option<String>,

    /// Language code. Leave empty to auto-detect the language.
    #[arg(short = 'l', long = "language", default_value = "")]
    pub language: String,

    /// Initial prompt.
    #[arg(short = 'p', long = "prompt", default_value = "")]
    pub prompt: String,

    /// Generate word-level timestamps.
    #[arg(short = 'w', long = "word-timestamps")]
    pub word_timestamps: bool,

    /// Extract speech from audio before transcribing.
    #[arg(short = 'e', long = "extract-speech")]
    pub extract_speech: bool,

    /// OpenAI access token. Use only when --model-type is openaiapi.
    #[arg(long = "openai-token", value_name = "token")]
    pub openai_token: Option<String>,

    /// Output result in an SRT file.
    #[arg(long = "srt")]
    pub srt: bool,

    /// Output result in a VTT file.
    #[arg(long = "vtt")]
    pub vtt: bool,

    /// Output result in a TXT file.
    #[arg(long = "txt")]
    pub txt: bool,

    /// Output directory. Defaults to each input file's own directory.
    #[arg(short = 'd', long = "output-directory", value_name = "DIRECTORY")]
    pub output_directory: Option<PathBuf>,

    /// Hide the main application window. No-op: humm is headless (ADR-007).
    #[arg(long = "hide-gui")]
    pub hide_gui: bool,

    /// Emit one JSON event object per line instead of human-readable output.
    #[arg(long = "json")]
    pub json: bool,

    /// Suppress progress output.
    #[arg(long = "quiet")]
    pub quiet: bool,

    /// Input file paths or URLs.
    #[arg(value_name = "FILE")]
    pub files: Vec<String>,
}
