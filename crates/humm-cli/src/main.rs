//! `humm`: local-first speech transcription CLI (M1 ships `add` only).

mod args;
mod logging;
mod run;

use std::process::ExitCode;

use clap::{CommandFactory, Parser};

fn main() -> ExitCode {
    // Logging initializes first, before any argument validation, so every
    // invocation — including early usage-error exits — leaves a trace
    // (F-009).
    let log_dir = humm_core::paths::log_dir();
    logging::init(&log_dir);
    log::debug!("humm starting (log_dir={})", log_dir.display());
    // Detached ffmpeg/ffprobe/yt-dlp/demucs children do not receive SIGHUP
    // or a Windows console-close. Install before any of them are spawned.
    humm_media::install_terminal_cleanup();

    let cli = match args::Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => return exit_code_for_parse_error(err),
    };

    match cli.command {
        Some(args::Command::Add(add_args)) => run::run_add(add_args),
        None => {
            // ADR-007 deviation: humm is headless (Buzz would launch the
            // GUI), so a bare `humm` invocation prints help to stderr and
            // exits 1 rather than doing nothing useful.
            let mut command = args::Cli::command();
            let _ = command.write_long_help(&mut std::io::stderr());
            eprintln!();
            ExitCode::from(1)
        }
    }
}

/// Maps a clap parse error to a process exit code: `0` for `--help`/
/// `--version` (not really errors), `1` for everything else — ADR-007
/// reserves clap's own default exit code (2) for `humm add` task failures,
/// not usage errors.
fn exit_code_for_parse_error(err: clap::Error) -> ExitCode {
    use clap::error::ErrorKind;

    let _ = err.print();
    match err.kind() {
        ErrorKind::DisplayHelp
        | ErrorKind::DisplayVersion
        | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => ExitCode::from(0),
        _ => ExitCode::from(1),
    }
}
