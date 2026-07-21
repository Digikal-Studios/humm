//! The humm settings file: a typed, serde-driven TOML document at
//! `paths::settings_path()` (ADR-012).
//!
//! Buzz stored settings in `QSettings` (a Qt-ism: registry on Windows,
//! `.plist` on macOS, `.conf` on Linux — see `buzz/settings/settings.py` and
//! `docs/analysis/EXTERNAL_SURFACE.md` §3.6 for the full key catalog). humm
//! ports the *semantics* of the settings this milestone actually consumes,
//! not the QSettings mechanism itself: a plain TOML file that's diffable and
//! hand-editable. Every field is optional and defaulted so a partial or
//! hand-edited file still loads, and unknown keys are ignored (no
//! `deny_unknown_fields`) so older files stay forward-compatible as fields
//! are added.
//!
//! `Settings::load`/`Settings::save` are the only I/O in this module.
//! Loading a missing file is not an error (first run); loading a file that
//! exists but can't be read or parsed is — humm never silently discards a
//! user's settings. Saving is atomic: write a sibling temp file, then
//! `rename` it over the target, so a crash mid-write never corrupts the
//! previous file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The settings file schema, rooted at `paths::settings_path()`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Model-selection settings.
    pub models: ModelSettings,
    /// URL-import download settings.
    pub downloads: DownloadSettings,
    /// Update-check settings.
    pub updates: UpdateSettings,
    /// GUI-persisted task-table layout.
    pub gui: GuiSettings,
    /// Remembered per-import file-transcription options.
    pub file_transcriber: FileTranscriberSettings,
    /// Telemetry opt-out and anonymous identifier.
    pub privacy: PrivacySettings,
}

/// Settings governing model selection (`buzz/model_loader.py` custom-model
/// path; `docs/analysis/FEATURE_INVENTORY.md` F-042/F-043).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSettings {
    /// URL to download a user-supplied custom ggml model from. Consumed by
    /// `-s custom` when no model is already cached.
    pub custom_model_url: Option<String>,
}

/// Settings governing URL-import downloads (F-022).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    /// Cookie file passed through to `yt-dlp` for authenticated downloads
    /// (`HUMM_DOWNLOAD_COOKIEFILE` env var takes precedence over this).
    pub cookie_file: Option<PathBuf>,
}

/// Settings governing the update-check feature (F-006, ADR-015).
///
/// `#[derive(Default)]` is deliberately not used here: `check_enabled` must
/// default to `true` (checking is on unless the user turns it off), which
/// `bool`'s derived `Default` (`false`) would get backwards. See
/// [`UpdateSettings::check_enabled`]'s doc comment for the full
/// missing-key-vs-missing-section contract this manual impl (paired with
/// the field's `#[serde(default = "default_true")]`) provides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Timestamp of the last update check, as a naive-local ISO-8601 string
    /// in the exact format `%Y-%m-%dT%H:%M:%S%.f` (no timezone offset/`Z`
    /// suffix) — the same format `humm-store`'s `now_iso()` writes and
    /// Buzz's `datetime.now().isoformat()` produces. This is a load-bearing
    /// format contract: [`crate::update::should_check`] parses this field
    /// with that exact format string and treats anything else (including a
    /// valid-but-differently-shaped RFC3339 timestamp with a `Z`/offset) as
    /// unparseable, which makes it fall back to "check is due". A caller
    /// that persists `last_check` must write it with `now_iso()` (or an
    /// equivalent `%Y-%m-%dT%H:%M:%S%.f` formatter), not `to_rfc3339()` or
    /// any timezone-aware formatter.
    pub last_check: Option<String>,
    /// Latest version reported by the last successful update check.
    pub available_version: Option<String>,
    /// Whether the update check runs at all — the user's Preferences toggle
    /// feeding [`crate::update::should_check`]'s `disabled` parameter
    /// alongside `HUMM_DISABLE_UPDATE_CHECK`.
    ///
    /// Defaults to `true` (checking on) whether the `[updates]` section is
    /// absent entirely, present but missing this key, or present with any
    /// other key set — only an explicit `check_enabled = false` turns it
    /// off. The `#[serde(default = "default_true")]` on this field handles
    /// the "section present, key absent" case; the container-level
    /// `#[serde(default)]` plus this struct's manual `Default` impl (which
    /// also sets `true`) handles "section absent entirely" — both paths are
    /// exercised directly by this module's tests rather than relying on
    /// either mechanism alone.
    #[serde(default = "default_true")]
    pub check_enabled: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings {
            last_check: None,
            available_version: None,
            check_enabled: true,
        }
    }
}

/// Settings governing the GUI's persisted task-table layout (F-003):
/// visible/hidden columns, column order, and sort state, all restored on
/// next launch. `None` in any field means "use the built-in default layout"
/// — this section only exists once the user has customized something.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiSettings {
    /// Column ids currently visible, in no particular order (display order
    /// comes from [`GuiSettings::task_table_order`]).
    pub task_table_visible: Option<Vec<String>>,
    /// Column ids in display order (visibility is separate — see
    /// [`GuiSettings::task_table_visible`]).
    pub task_table_order: Option<Vec<String>>,
    /// Id of the column the task table is currently sorted by.
    pub task_table_sort_column: Option<String>,
    /// Whether the task table's sort is descending (`true`) or ascending
    /// (`false`).
    pub task_table_sort_descending: Option<bool>,
}

/// Remembered file-transcription options from the last import (F-020,
/// F-023): Buzz's `file-transcriber/*` `QSettings` group
/// (`EXTERNAL_SURFACE.md` §3.6), restored to pre-fill the next options
/// window. Every field is optional — a first run has none of these set —
/// and, where the underlying value is a domain enum, the field holds its
/// wire string (that type's `as_str()`/`code()` value, e.g. `"translate"`,
/// `"large-v3-turbo"`), not the enum itself, matching how the rest of this
/// settings file favors plain, hand-editable values over typed enums.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileTranscriberSettings {
    /// Directory the last file-import dialog was opened in.
    pub last_import_folder: Option<PathBuf>,
    /// Last-selected language code (`crate::language::Language::code`), or
    /// `None` for auto-detect.
    pub language: Option<String>,
    /// Last-selected task (`crate::domain::TaskKind::as_str`).
    pub task: Option<String>,
    /// Last-selected model size (`crate::domain::ModelSize::as_str`).
    pub model_size: Option<String>,
    /// Last-entered custom Hugging Face repo id, if the Hugging Face model
    /// type was last selected.
    pub hf_repo: Option<String>,
    /// Whether word-level timestamps were last enabled.
    pub word_timestamps: Option<bool>,
    /// Last-entered initial prompt text.
    pub initial_prompt: Option<String>,
    /// Last-selected export formats (`crate::domain::OutputFormat::as_str`
    /// values).
    pub export_formats: Option<Vec<String>>,
}

/// Settings governing telemetry opt-out and the anonymous analytics
/// identifier (F-007, ADR-009).
///
/// `#[derive(Default)]` is deliberately not used: `telemetry_enabled` must
/// default to `true` (Buzz parity — telemetry is on unless the user or an
/// env var opts out), which `bool`'s derived `Default` (`false`) would get
/// backwards. Same manual-`Default`-plus-field-default pattern as
/// [`UpdateSettings::check_enabled`]; see that field's doc comment for the
/// full missing-key-vs-missing-section contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacySettings {
    /// Whether the user has opted in to telemetry — one of the four inputs
    /// to [`crate::telemetry::telemetry_enabled`]. Defaults to `true`
    /// whether the `[privacy]` section is absent, present but missing this
    /// key, or present with only `user_identifier` set; only an explicit
    /// `telemetry_enabled = false` turns it off.
    #[serde(default = "default_true")]
    pub telemetry_enabled: bool,
    /// Persisted anonymous uuid4 used as the PostHog `distinct_id` — Buzz's
    /// `user-identifier` `QSettings` key (`EXTERNAL_SURFACE.md` §3.6),
    /// minted once and reused for the lifetime of the install.
    pub user_identifier: Option<String>,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        PrivacySettings {
            telemetry_enabled: true,
            user_identifier: None,
        }
    }
}

/// Shared `#[serde(default = "default_true")]` helper for settings fields
/// that must default to `true` when their key is absent from an otherwise-
/// present section (see [`UpdateSettings::check_enabled`] and
/// [`PrivacySettings::telemetry_enabled`]).
fn default_true() -> bool {
    true
}

impl Settings {
    /// Loads settings from `path`.
    ///
    /// A missing file is not an error: it returns `Settings::default()` (the
    /// expected state on first run, before any settings have been saved). A
    /// file that exists but can't be read (permissions, I/O error) or can't
    /// be parsed as TOML returns `Err` — humm never silently falls back to
    /// defaults over a user's existing settings.
    pub fn load(path: &Path) -> Result<Settings, SettingsError> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default());
            }
            Err(source) => {
                return Err(SettingsError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&content).map_err(|error| SettingsError::Parse {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })
    }

    /// Saves settings to `path`, creating parent directories if needed.
    ///
    /// The write is atomic: the serialized TOML goes to a temp file in the
    /// same directory (`<file_name>.tmp-<pid>`), which is then renamed over
    /// `path`. On any platform where rename is atomic within a directory,
    /// concurrent readers see either the old file or the new one, never a
    /// partial write, and no temp file is left behind on success.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let body = toml::to_string_pretty(self).map_err(|error| SettingsError::Parse {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;

        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|source| SettingsError::Io {
            path: parent.to_path_buf(),
            source,
        })?;

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "settings.toml".to_string());
        let tmp_path = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

        std::fs::write(&tmp_path, body).map_err(|source| SettingsError::Io {
            path: tmp_path.clone(),
            source,
        })?;
        std::fs::rename(&tmp_path, path).map_err(|source| SettingsError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }
}

/// Errors from loading or saving the settings file.
///
/// Deliberately standalone rather than a `CoreError` variant: settings I/O
/// is a leaf concern of binaries (CLI/GUI startup), not something that
/// crosses the transcription pipeline.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    /// An I/O error occurred at `path` (permissions, disk full, etc.) — not
    /// including a missing file on load, which is not an error.
    #[error("settings I/O error at {}: {source}", path.display())]
    Io {
        /// The path being read or written.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The settings file's contents could not be interpreted as TOML,
    /// either decoding an existing file or encoding one to save.
    #[error("failed to parse settings at {}: {detail}", path.display())]
    Parse {
        /// The file that failed to (de)serialize.
        path: PathBuf,
        /// Diagnostic detail from the TOML (de)serializer.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{Settings, SettingsError};
    use std::path::Path;

    #[test]
    fn load_missing_path_returns_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        let loaded = Settings::load(&path).expect("load missing file");
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn save_then_load_round_trips_all_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        let mut settings = Settings::default();
        settings.models.custom_model_url = Some("https://example.com/model.bin".to_string());
        settings.downloads.cookie_file = Some(Path::new("/tmp/cookies.txt").to_path_buf());
        // Canonical `last_check` format (`%Y-%m-%dT%H:%M:%S%.f`, no zone) —
        // see the field's doc comment: `update::should_check` parses this
        // exact shape, so the test data must model the real contract rather
        // than an RFC3339 `Z`-suffixed value that would silently fail it.
        settings.updates.last_check = Some("2026-07-21T00:00:00".to_string());
        settings.updates.available_version = Some("1.2.3".to_string());
        settings.updates.check_enabled = false;
        settings.gui.task_table_visible = Some(vec!["file_name".to_string(), "status".to_string()]);
        settings.gui.task_table_order = Some(vec!["status".to_string(), "file_name".to_string()]);
        settings.gui.task_table_sort_column = Some("date_added".to_string());
        settings.gui.task_table_sort_descending = Some(true);
        settings.file_transcriber.last_import_folder =
            Some(Path::new("/tmp/imports").to_path_buf());
        settings.file_transcriber.language = Some("en".to_string());
        settings.file_transcriber.task = Some("translate".to_string());
        settings.file_transcriber.model_size = Some("large-v3-turbo".to_string());
        settings.file_transcriber.hf_repo = Some("openai/whisper-tiny".to_string());
        settings.file_transcriber.word_timestamps = Some(true);
        settings.file_transcriber.initial_prompt = Some("proper nouns: Buzz, humm".to_string());
        settings.file_transcriber.export_formats = Some(vec!["srt".to_string(), "vtt".to_string()]);
        settings.privacy.telemetry_enabled = false;
        settings.privacy.user_identifier = Some("11111111-2222-3333-4444-555555555555".to_string());

        settings.save(&path).expect("save");
        let loaded = Settings::load(&path).expect("load");
        assert_eq!(loaded, settings);
    }

    #[test]
    fn check_enabled_defaults_true_when_updates_section_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "").expect("write empty file");

        let loaded = Settings::load(&path).expect("load");
        assert!(loaded.updates.check_enabled);
    }

    #[test]
    fn check_enabled_defaults_true_when_section_present_key_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "[updates]\nlast_check = \"2026-07-21T00:00:00\"\n")
            .expect("write file");

        let loaded = Settings::load(&path).expect("load");
        assert!(loaded.updates.check_enabled);
    }

    #[test]
    fn check_enabled_explicit_false_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "[updates]\ncheck_enabled = false\n").expect("write file");

        let loaded = Settings::load(&path).expect("load");
        assert!(!loaded.updates.check_enabled);
    }

    #[test]
    fn telemetry_enabled_defaults_true_when_privacy_section_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "").expect("write empty file");

        let loaded = Settings::load(&path).expect("load");
        assert!(loaded.privacy.telemetry_enabled);
    }

    #[test]
    fn telemetry_enabled_defaults_true_when_section_present_key_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "[privacy]\nuser_identifier = \"11111111-2222-3333-4444-555555555555\"\n",
        )
        .expect("write file");

        let loaded = Settings::load(&path).expect("load");
        assert!(loaded.privacy.telemetry_enabled);
    }

    #[test]
    fn telemetry_enabled_explicit_false_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "[privacy]\ntelemetry_enabled = false\n").expect("write file");

        let loaded = Settings::load(&path).expect("load");
        assert!(!loaded.privacy.telemetry_enabled);
    }

    #[test]
    fn corrupt_file_returns_err_mentioning_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "not = [valid toml").expect("write corrupt file");

        let err = Settings::load(&path).expect_err("corrupt file must error");
        let message = err.to_string();
        assert!(
            message.contains(path.to_str().unwrap()),
            "expected error to mention path, got: {message}"
        );
    }

    #[test]
    fn save_creates_parent_dirs_and_leaves_no_tmp_sibling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("dir").join("settings.toml");

        Settings::default().save(&path).expect("save");

        assert!(path.exists(), "settings file should exist");
        let parent = path.parent().unwrap();
        let leftover_tmp: Vec<_> = std::fs::read_dir(parent)
            .expect("read parent dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftover_tmp.is_empty(),
            "expected no tmp siblings, found {leftover_tmp:?}"
        );
    }

    #[test]
    fn unknown_toml_keys_are_ignored_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            r#"
            [models]
            custom_model_url = "https://example.com/model.bin"
            some_future_field = "ignored"

            [some_unknown_section]
            key = "value"
            "#,
        )
        .expect("write file with unknown keys");

        let loaded = Settings::load(&path).expect("load file with unknown keys");
        assert_eq!(
            loaded.models.custom_model_url,
            Some("https://example.com/model.bin".to_string())
        );
    }

    #[test]
    fn settings_error_parse_mentions_path() {
        let path = Path::new("/tmp/whatever/settings.toml");
        let err = SettingsError::Parse {
            path: path.to_path_buf(),
            detail: "bad token".to_string(),
        };
        assert!(err.to_string().contains("bad token"));
    }
}
