//! `humm add`: resolve the model, load the engine, and run one
//! `run_file_job` per input file — the flow specified in
//! `docs/superpowers/plans/2026-07-20-m1-headless-core-path.md` Task 8.

use std::collections::BTreeSet;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use humm_core::boundary::SegmentSink;
use humm_core::cancel::CancelToken;
use humm_core::domain::{
    DEFAULT_TEMPERATURE, JobSpec, Millis, ModelSpec, OutputFormat, OutputSpec, Segment,
    TranscriptionOptions,
};
use humm_core::language::Language;
use humm_core::models::download::{DownloadProgress, ModelDownloader};
use humm_core::models::registry::{local_model_path, local_vad_model_path};
use humm_core::paths;
use humm_core::pipeline::{EngineTuning, run_file_job};
use humm_media::{FfmpegDecoder, FfprobeProber};
use humm_whisper::WhisperEngine;

use crate::args::AddArgs;

/// Runs `humm add`. Returns the process exit code per ADR-007: `0` every
/// input succeeded, `1` a usage/config error aborted the whole run before
/// any file could be processed, `2` at least one input failed but the run
/// otherwise completed.
pub fn run_add(args: AddArgs) -> ExitCode {
    if args.files.is_empty() {
        return fail("No input files");
    }

    let language = match parse_language(&args.language) {
        Ok(language) => language,
        Err(message) => return fail(message),
    };

    let mut failed: Vec<String> = Vec::new();
    let mut existing_files: Vec<PathBuf> = Vec::new();
    for file in &args.files {
        if file.exists() {
            existing_files.push(file.clone());
        } else {
            report_failure(file, "no such file");
            failed.push(file.display().to_string());
        }
    }

    // No model download if every input is missing (brief Run-flow step 1) —
    // fail fast rather than paying for a model resolution nobody needs.
    if !existing_files.is_empty() {
        match process_existing_files(&args, &existing_files, language) {
            Ok(mut job_failures) => failed.append(&mut job_failures),
            Err(exit_code) => return exit_code,
        }
    }

    if failed.is_empty() {
        ExitCode::from(0)
    } else {
        eprintln!(
            "Error: {} of {} input(s) failed:",
            failed.len(),
            args.files.len()
        );
        for input in &failed {
            eprintln!("  {input}");
        }
        ExitCode::from(2)
    }
}

/// Resolves the model, ensures VAD (best-effort), loads the engine, then runs
/// `run_file_job` for every file in `existing_files`. Returns the display
/// strings of any per-file job failures on `Ok`; returns the process exit
/// code directly (`Err`) when a config-level step (model download, engine
/// load) fails the whole run.
fn process_existing_files(
    args: &AddArgs,
    existing_files: &[PathBuf],
    language: Option<Language>,
) -> Result<Vec<String>, ExitCode> {
    let cancel = CancelToken::new();
    let model_root = paths::model_root();
    let downloader = ModelDownloader::new(model_root.clone());

    let model_path = match local_model_path(&model_root, args.model_size) {
        Some(path) => path,
        None => {
            let mut progress = ProgressLine::new();
            let mut render =
                |update: DownloadProgress| render_download_progress(&mut progress, &update);
            let result =
                downloader.ensure_model(args.model_size, coreml_encoder(), &mut render, &cancel);
            progress.finish();
            match result {
                Ok(path) => path,
                Err(err) => return Err(fail(err)),
            }
        }
    };

    let vad_model = match local_vad_model_path(&model_root) {
        Some(path) => Some(path),
        None => {
            let mut progress = ProgressLine::new();
            let mut render =
                |update: DownloadProgress| render_download_progress(&mut progress, &update);
            let result = downloader.ensure_vad_model(&mut render, &cancel);
            progress.finish();
            match result {
                Ok(path) => Some(path),
                Err(err) => {
                    // F-045 parity: an absent VAD model silently disables VAD
                    // rather than failing the run.
                    log::warn!("VAD model unavailable ({err}); continuing without VAD");
                    None
                }
            }
        }
    };

    let use_gpu = use_gpu_from_env(std::env::var("HUMM_FORCE_CPU").ok().as_deref());
    let tuning = EngineTuning {
        vad_model,
        use_gpu,
        threads: threads_from_env(std::env::var("HUMM_WHISPER_N_THREADS").ok().as_deref()),
    };

    let mut engine = match WhisperEngine::load(&model_path, use_gpu) {
        Ok(engine) => engine,
        Err(err) => return Err(fail(err)),
    };

    let decoder = FfmpegDecoder::new();
    let prober = FfprobeProber::new();

    let formats = output_formats(args);
    if formats.is_empty() {
        // Buzz parity: zero formats still transcribes, it just writes
        // nothing — but that's surprising enough to be worth a warning.
        log::warn!("no export format selected (--txt/--srt/--vtt)");
    }

    let options = TranscriptionOptions {
        language,
        task: args.task,
        model: ModelSpec {
            size: args.model_size,
        },
        word_timestamps: args.word_timestamps,
        initial_prompt: args.prompt.clone(),
        temperature: DEFAULT_TEMPERATURE.to_vec(),
    };
    let output = OutputSpec {
        formats,
        directory: args.output_directory.clone(),
        file_name_template: None,
        paragraph_split: paragraph_split_from_env(
            std::env::var("HUMM_PARAGRAPH_SPLIT_TIME").ok().as_deref(),
        ),
    };

    let mut failed = Vec::new();
    for file in existing_files {
        let job = JobSpec {
            input: file.clone(),
            options: options.clone(),
            output: output.clone(),
        };
        let mut sink = JobProgressSink::new(file);
        let result = run_file_job(
            &job,
            &tuning,
            &decoder,
            &prober,
            &mut engine,
            &mut sink,
            &cancel,
        );
        sink.finish();
        match result {
            Ok(outcome) => {
                for path in &outcome.written {
                    println!("{}", path.display());
                }
            }
            Err(err) => {
                report_failure(file, &err.to_string());
                failed.push(file.display().to_string());
            }
        }
    }

    Ok(failed)
}

fn coreml_encoder() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

fn output_formats(args: &AddArgs) -> BTreeSet<OutputFormat> {
    let mut formats = BTreeSet::new();
    if args.srt {
        formats.insert(OutputFormat::Srt);
    }
    if args.vtt {
        formats.insert(OutputFormat::Vtt);
    }
    if args.txt {
        formats.insert(OutputFormat::Txt);
    }
    formats
}

/// Empty string = auto-detect (`Ok(None)`); a non-empty, unrecognized code is
/// `cli.py`'s `"Invalid language option"` (`cli.py:226-227`).
fn parse_language(input: &str) -> Result<Option<Language>, &'static str> {
    if input.is_empty() {
        Ok(None)
    } else {
        Language::parse(input)
            .map(Some)
            .ok_or("Invalid language option")
    }
}

fn report_failure(file: &Path, cause: &str) {
    eprintln!("error: {}: {cause}", file.display());
}

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("Error: {message}");
    eprintln!("Run `humm add --help` for usage.");
    ExitCode::from(1)
}

/// `HUMM_FORCE_CPU` semantics mirror Buzz's `BUZZ_FORCE_CPU`
/// (`buzz/transcriber/whisper_cpp.py:191-193`): unset or `"false"` leaves GPU
/// use enabled; any other value forces CPU.
fn use_gpu_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some(v) if v != "false")
}

fn threads_from_env(value: Option<&str>) -> Option<u32> {
    value.and_then(|v| v.parse().ok())
}

fn paragraph_split_from_env(value: Option<&str>) -> Millis {
    value
        .and_then(|v| v.parse::<i64>().ok())
        .map(Millis)
        .unwrap_or(Millis(2000))
}

/// Throttled single-line progress renderer: rewrites the same terminal line
/// via `\r` when stderr is a TTY; emits plain, change-triggered lines
/// otherwise (brief: "`\r<file>  NN%` to stderr on TTY... plain occasional
/// lines otherwise").
struct ProgressLine {
    tty: bool,
    last_key: Option<i64>,
}

impl ProgressLine {
    fn new() -> Self {
        ProgressLine {
            tty: std::io::stderr().is_terminal(),
            last_key: None,
        }
    }

    fn update(&mut self, key: i64, text: &str) {
        if self.last_key == Some(key) {
            return;
        }
        self.last_key = Some(key);
        if self.tty {
            eprint!("\r{text}");
            let _ = std::io::stderr().flush();
        } else {
            eprintln!("{text}");
        }
    }

    fn finish(&mut self) {
        if self.tty && self.last_key.is_some() {
            eprintln!();
        }
    }
}

fn render_download_progress(progress: &mut ProgressLine, update: &DownloadProgress) {
    let downloaded_mb = update.downloaded as f64 / 1_000_000.0;
    match update.total.filter(|&total| total > 0) {
        Some(total) => {
            let percent = ((update.downloaded as f64 / total as f64) * 100.0).round() as i64;
            let total_mb = total as f64 / 1_000_000.0;
            progress.update(
                percent,
                &format!(
                    "Downloading {}  {percent}% ({downloaded_mb:.1}/{total_mb:.1} MB)",
                    update.file
                ),
            );
        }
        None => {
            let bucket = (update.downloaded / 1_000_000) as i64;
            progress.update(
                bucket,
                &format!("Downloading {}  ({downloaded_mb:.1} MB)", update.file),
            );
        }
    }
}

struct JobProgressSink<'a> {
    file: &'a Path,
    line: ProgressLine,
}

impl<'a> JobProgressSink<'a> {
    fn new(file: &'a Path) -> Self {
        JobProgressSink {
            file,
            line: ProgressLine::new(),
        }
    }

    fn finish(&mut self) {
        self.line.finish();
    }
}

impl SegmentSink for JobProgressSink<'_> {
    fn on_segment(&mut self, _segment: Segment) {}

    fn on_progress(&mut self, fraction: f32) {
        let percent = (fraction.clamp(0.0, 1.0) * 100.0).round() as i64;
        self.line
            .update(percent, &format!("{}  {percent}%", self.file.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parse_language_empty_is_auto_detect() {
        assert_eq!(parse_language(""), Ok(None));
    }

    #[test]
    fn parse_language_known_code_resolves() {
        assert_eq!(parse_language("fr"), Ok(Language::parse("fr")));
    }

    #[test]
    fn parse_language_unknown_code_is_invalid_language_option() {
        assert_eq!(parse_language("xx"), Err("Invalid language option"));
    }

    #[test]
    fn use_gpu_from_env_unset_allows_gpu() {
        assert!(use_gpu_from_env(None));
    }

    #[test]
    fn use_gpu_from_env_false_allows_gpu() {
        assert!(use_gpu_from_env(Some("false")));
    }

    #[test]
    fn use_gpu_from_env_any_other_value_forces_cpu() {
        assert!(!use_gpu_from_env(Some("true")));
        assert!(!use_gpu_from_env(Some("1")));
        assert!(!use_gpu_from_env(Some("")));
    }

    #[test]
    fn threads_from_env_parses_valid_value() {
        assert_eq!(threads_from_env(Some("4")), Some(4));
    }

    #[test]
    fn threads_from_env_none_when_unset_or_invalid() {
        assert_eq!(threads_from_env(None), None);
        assert_eq!(threads_from_env(Some("not-a-number")), None);
    }

    #[test]
    fn paragraph_split_from_env_defaults_to_2000() {
        assert_eq!(paragraph_split_from_env(None), Millis(2000));
        assert_eq!(paragraph_split_from_env(Some("bogus")), Millis(2000));
    }

    #[test]
    fn paragraph_split_from_env_parses_override() {
        assert_eq!(paragraph_split_from_env(Some("500")), Millis(500));
    }

    #[test]
    fn output_formats_collects_selected_flags_only() {
        let mut args = base_add_args();
        args.srt = true;
        args.txt = true;
        let formats = output_formats(&args);
        assert_eq!(
            formats,
            [OutputFormat::Srt, OutputFormat::Txt].into_iter().collect()
        );
    }

    #[test]
    fn output_formats_empty_when_no_flags_selected() {
        let args = base_add_args();
        assert!(output_formats(&args).is_empty());
    }

    fn base_add_args() -> AddArgs {
        AddArgs {
            task: humm_core::domain::TaskKind::Transcribe,
            model_size: humm_core::domain::ModelSize::Small,
            language: String::new(),
            prompt: String::new(),
            word_timestamps: false,
            srt: false,
            vtt: false,
            txt: false,
            output_directory: None,
            files: Vec::new(),
        }
    }
}
