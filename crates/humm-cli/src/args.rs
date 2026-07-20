//! Command-line argument grammar for `humm`.
//!
//! Grammar and flag spellings are Buzz-verbatim for every flag M1 ships
//! (`buzz/cli.py:52-138`, `docs/analysis/EXTERNAL_SURFACE.md` §2): `-t/--task`,
//! `-s/--model-size`, `-l/--language`, `-p/--prompt`, `-w/--word-timestamps`,
//! `--srt`/`--vtt`/`--txt`, `-d/--output-directory`, plus file positionals.
//! Buzz flags arriving in M2/M6 — `-m/--model-type`, `--hfid`,
//! `-e/--extract-speech`, `--openai-token`, `--hide-gui`, URL positionals,
//! `--json` — are deliberately absent
//! (`docs/superpowers/plans/2026-07-20-m1-headless-core-path.md` Task 8).
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

use clap::{Args, Parser, Subcommand};
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

    /// Model size.
    #[arg(
        short = 's',
        long = "model-size",
        default_value = "small",
        value_parser = clap::value_parser!(ModelSize)
    )]
    pub model_size: ModelSize,

    /// Language code. Leave empty to auto-detect the language.
    #[arg(short = 'l', long = "language", default_value = "")]
    pub language: String,

    /// Initial prompt.
    #[arg(short = 'p', long = "prompt", default_value = "")]
    pub prompt: String,

    /// Generate word-level timestamps.
    #[arg(short = 'w', long = "word-timestamps")]
    pub word_timestamps: bool,

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

    /// Input file paths.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,
}
