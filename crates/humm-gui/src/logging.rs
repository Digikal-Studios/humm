//! Hand-rolled `log::Log` sink (F-009), same shape as `humm-cli`'s
//! (`crates/humm-cli/src/logging.rs`): DEBUG+ appended to
//! `<log_dir>/logs.txt`, WARN+ mirrored to stderr.
//!
//! Duplicated rather than shared: the CLI's logger module is crate-private
//! (`humm-cli` has no library target for `humm-gui` to depend on), and this
//! task is scoped to the GUI scaffold, not a `humm-cli` refactor. Noted as a
//! concern for the final review — a small shared `humm-logging` crate (or
//! moving this into `humm-core`) would remove the duplication if it's judged
//! worth the churn.
//!
//! [`init`] must run first thing in `main`, before any lock/settings/store
//! work, so every code path — including an early single-instance exit —
//! leaves a trace.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct FileAndStderrLogger {
    file: Option<Mutex<File>>,
}

impl Log for FileAndStderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = format_line(record);

        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = file.write_all(line.as_bytes());
        }

        if record.level() <= Level::Warn {
            eprint!("{line}");
        }
    }

    fn flush(&self) {
        if let Some(file) = &self.file
            && let Ok(mut file) = file.lock()
        {
            let _ = file.flush();
        }
    }
}

/// Formats one log line: `{RFC3339 time} {level} {module}:{line} — {message}\n`.
fn format_line(record: &Record) -> String {
    format!(
        "{} {} {}:{} — {}\n",
        chrono::Local::now().to_rfc3339(),
        record.level(),
        record.module_path().unwrap_or("?"),
        record.line().unwrap_or(0),
        record.args(),
    )
}

/// Initializes the global logger: DEBUG+ to `<log_dir>/logs.txt` (creating
/// the directory, appending across runs), WARN+ also mirrored to stderr.
///
/// If the log file can't be opened, prints one warning to stderr and
/// continues with stderr-only logging rather than failing the GUI.
pub fn init(log_dir: &Path) {
    let file = match open_log_file(log_dir) {
        Ok(file) => Some(file),
        Err(err) => {
            eprintln!("warning: {err:#}; logging to stderr only");
            None
        }
    };

    let logger = FileAndStderrLogger {
        file: file.map(Mutex::new),
    };

    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

fn open_log_file(log_dir: &Path) -> anyhow::Result<File> {
    use anyhow::Context;
    std::fs::create_dir_all(log_dir)
        .with_context(|| format!("creating log directory {}", log_dir.display()))?;
    let path = log_dir.join("logs.txt");
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening log file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_log_file_creates_missing_directory_and_appends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_dir = dir.path().join("nested").join("logs");

        let mut file = open_log_file(&log_dir).expect("open log file");
        writeln!(file, "first line").unwrap();
        drop(file);

        let mut file = open_log_file(&log_dir).expect("reopen log file");
        writeln!(file, "second line").unwrap();
        drop(file);

        let contents = std::fs::read_to_string(log_dir.join("logs.txt")).unwrap();
        assert_eq!(contents, "first line\nsecond line\n");
    }

    #[test]
    fn open_log_file_fails_when_parent_is_a_file_not_a_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let log_dir = blocker.join("logs");

        assert!(open_log_file(&log_dir).is_err());
    }
}
