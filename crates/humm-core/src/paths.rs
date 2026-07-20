//! Filesystem locations for models and logs, with environment-variable
//! overrides.
//!
//! Per Global Constraint 9, env reads are encapsulated here: the public
//! functions read `HUMM_MODEL_ROOT`/`HUMM_LOG_DIR` once and delegate to a
//! pure `..._from` inner function, so tests can exercise the override/default
//! logic without mutating process environment (`std::env::set_var` is
//! `unsafe` in edition 2024).

use std::ffi::OsStr;
use std::path::PathBuf;

/// Directory holding downloaded whisper models.
///
/// Honors `HUMM_MODEL_ROOT` if set; otherwise `<data_dir>/humm/models`
/// (`docs/design/ARCHITECTURE.md` §5), where `<data_dir>` is the platform
/// data directory (macOS: `~/Library/Application Support`; Linux:
/// `$XDG_DATA_HOME` or `~/.local/share`; Windows: `%APPDATA%`).
pub fn model_root() -> PathBuf {
    model_root_from(std::env::var_os("HUMM_MODEL_ROOT").as_deref())
}

/// Directory humm writes log files to.
///
/// Honors `HUMM_LOG_DIR` if set; otherwise the platform log location: macOS
/// `~/Library/Logs/humm`, Linux `$XDG_STATE_HOME` or `~/.local/state/humm/logs`,
/// Windows `%LOCALAPPDATA%\humm\logs`.
pub fn log_dir() -> PathBuf {
    log_dir_from(std::env::var_os("HUMM_LOG_DIR").as_deref())
}

fn model_root_from(env: Option<&OsStr>) -> PathBuf {
    if let Some(value) = env {
        return PathBuf::from(value);
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_dir().join("humm").join("models"))
        .unwrap_or_else(|| PathBuf::from("humm").join("models"))
}

fn log_dir_from(env: Option<&OsStr>) -> PathBuf {
    if let Some(value) = env {
        return PathBuf::from(value);
    }
    platform_log_dir()
}

#[cfg(target_os = "macos")]
fn platform_log_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join("Library").join("Logs").join("humm"))
        .unwrap_or_else(|| PathBuf::from("humm-logs"))
}

#[cfg(target_os = "linux")]
fn platform_log_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| {
            dirs.state_dir()
                .map(PathBuf::from)
                .unwrap_or_else(|| dirs.home_dir().join(".local").join("state"))
                .join("humm")
                .join("logs")
        })
        .unwrap_or_else(|| PathBuf::from("humm-logs"))
}

#[cfg(target_os = "windows")]
fn platform_log_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("humm").join("logs"))
        .unwrap_or_else(|| PathBuf::from("humm-logs"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_log_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_dir().join("humm").join("logs"))
        .unwrap_or_else(|| PathBuf::from("humm-logs"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn model_root_override_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let override_path = dir.path().join("custom-models");
        let resolved = model_root_from(Some(override_path.as_os_str()));
        assert_eq!(resolved, override_path);
    }

    #[test]
    fn model_root_default_ends_with_humm_models() {
        let resolved = model_root_from(None);
        assert!(
            resolved.ends_with(Path::new("humm/models")),
            "expected {resolved:?} to end with humm/models"
        );
    }

    #[test]
    fn log_dir_override_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let override_path = dir.path().join("custom-logs");
        let resolved = log_dir_from(Some(override_path.as_os_str()));
        assert_eq!(resolved, override_path);
    }

    #[test]
    fn log_dir_default_matches_platform_convention() {
        let resolved = log_dir_from(None);
        #[cfg(target_os = "macos")]
        assert!(
            resolved.ends_with(Path::new("Library/Logs/humm")),
            "expected {resolved:?} to end with Library/Logs/humm"
        );
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        assert!(
            resolved.ends_with(Path::new("humm/logs")),
            "expected {resolved:?} to end with humm/logs"
        );
    }
}
