//! Filesystem locations for models and logs, with environment-variable
//! overrides.
//!
//! Per Global Constraint 9, env reads are encapsulated here: the public
//! functions read `HUMM_MODEL_ROOT`/`HUMM_LOG_DIR` once and delegate to a
//! pure `..._from` inner function, so tests can exercise the override/default
//! logic without mutating process environment (`std::env::set_var` is
//! `unsafe` in edition 2024).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

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

/// Directory holding humm's configuration file (`settings.toml`).
///
/// Honors `HUMM_CONFIG_DIR` if set; otherwise `<platform config dir>/humm`
/// (macOS: `~/Library/Application Support/humm`; Linux: `$XDG_CONFIG_HOME`
/// or `~/.config/humm`; Windows: `%APPDATA%\humm`).
pub fn config_dir() -> PathBuf {
    config_dir_from(std::env::var_os("HUMM_CONFIG_DIR").as_deref())
}

/// Directory holding humm's persistent data: the sqlite database and
/// downloaded media (`docs/design/ARCHITECTURE.md` §5).
///
/// Honors `HUMM_DATA_DIR` if set; otherwise `<platform data dir>/humm`
/// (macOS: `~/Library/Application Support/humm`; Linux: `$XDG_DATA_HOME` or
/// `~/.local/share/humm`; Windows: `%APPDATA%\humm`).
pub fn data_dir() -> PathBuf {
    data_dir_from(std::env::var_os("HUMM_DATA_DIR").as_deref())
}

/// Path to the settings TOML file: `config_dir()/settings.toml`.
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.toml")
}

/// Path to humm's sqlite database: `data_dir()/humm.sqlite`.
pub fn db_path() -> PathBuf {
    data_dir().join("humm.sqlite")
}

/// Directory for URL-import downloads: `data_dir()/media`.
pub fn media_dir() -> PathBuf {
    data_dir().join("media")
}

/// Path to Buzz's own sqlite database on this machine, if present.
///
/// Buzz (the app humm ports from) keeps its database at the platform
/// user-data root for app "Buzz" plus `Buzz.sqlite` (macOS:
/// `~/Library/Application Support/Buzz/Buzz.sqlite`). humm never writes to
/// this file; it only reads it, and only when a caller asks to import (a
/// later milestone). Returns `None` when the platform data directory can't
/// be determined or the file doesn't exist.
pub fn buzz_sqlite_path() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    buzz_sqlite_path_from(base.data_dir())
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

fn config_dir_from(env: Option<&OsStr>) -> PathBuf {
    if let Some(value) = env {
        return PathBuf::from(value);
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.config_dir().join("humm"))
        .unwrap_or_else(|| PathBuf::from("humm"))
}

fn data_dir_from(env: Option<&OsStr>) -> PathBuf {
    if let Some(value) = env {
        return PathBuf::from(value);
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_dir().join("humm"))
        .unwrap_or_else(|| PathBuf::from("humm"))
}

/// Given the platform's base data directory, returns Buzz's sqlite path if
/// it exists there. Takes the base dir as a parameter so tests never touch
/// real user directories.
fn buzz_sqlite_path_from(base_data_dir: &Path) -> Option<PathBuf> {
    let candidate = base_data_dir.join("Buzz").join("Buzz.sqlite");
    candidate.exists().then_some(candidate)
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

    #[test]
    fn config_dir_override_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let override_path = dir.path().join("custom-config");
        let resolved = config_dir_from(Some(override_path.as_os_str()));
        assert_eq!(resolved, override_path);
    }

    #[test]
    fn config_dir_default_ends_with_humm() {
        let resolved = config_dir_from(None);
        assert!(
            resolved.ends_with(Path::new("humm")),
            "expected {resolved:?} to end with humm"
        );
    }

    #[test]
    fn data_dir_override_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let override_path = dir.path().join("custom-data");
        let resolved = data_dir_from(Some(override_path.as_os_str()));
        assert_eq!(resolved, override_path);
    }

    #[test]
    fn data_dir_default_ends_with_humm() {
        let resolved = data_dir_from(None);
        assert!(
            resolved.ends_with(Path::new("humm")),
            "expected {resolved:?} to end with humm"
        );
    }

    #[test]
    fn settings_path_default_ends_with_humm_settings_toml() {
        let resolved = settings_path();
        assert!(
            resolved.ends_with(Path::new("humm/settings.toml")),
            "expected {resolved:?} to end with humm/settings.toml"
        );
    }

    #[test]
    fn db_path_default_ends_with_humm_sqlite() {
        let resolved = db_path();
        assert!(
            resolved.ends_with(Path::new("humm/humm.sqlite")),
            "expected {resolved:?} to end with humm/humm.sqlite"
        );
    }

    #[test]
    fn media_dir_default_ends_with_humm_media() {
        let resolved = media_dir();
        assert!(
            resolved.ends_with(Path::new("humm/media")),
            "expected {resolved:?} to end with humm/media"
        );
    }

    #[test]
    fn buzz_sqlite_path_returns_none_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = buzz_sqlite_path_from(dir.path());
        assert_eq!(resolved, None);
    }

    #[test]
    fn buzz_sqlite_path_returns_some_when_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let buzz_dir = dir.path().join("Buzz");
        std::fs::create_dir_all(&buzz_dir).expect("create Buzz dir");
        let db_path = buzz_dir.join("Buzz.sqlite");
        std::fs::write(&db_path, b"").expect("write stub db");
        let resolved = buzz_sqlite_path_from(dir.path());
        assert_eq!(resolved, Some(db_path));
    }
}
