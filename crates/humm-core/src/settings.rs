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

use std::collections::BTreeMap;
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
    /// Transcript-viewer window state (F-080): persisted toggles, playback
    /// rate, splitter fractions, and shared window geometry.
    pub viewer: ViewerSettings,
    /// Live recording transcriber settings (F-060, F-061): sticky options and
    /// export configuration.
    pub recording: RecordingSettings,
    /// Live presentation window settings (F-065): display style preferences.
    pub presentation: PresentationSettings,
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

/// Transcript-viewer window state (F-080, design spec §6), ported from Buzz's
/// `transcription_viewer/*` `QSettings` group (`transcription_viewer_widget.py`
/// `_load_segments_and_settings`/`load_geometry`/`load_splitter_sizes`, and the
/// global `AUDIO_PLAYBACK_RATE` key). Every field defaults to Buzz's own
/// default, so a first run (or a partial/hand-edited file) matches Buzz.
///
/// `#[derive(Default)]` is deliberately not used: several fields have non-zero
/// defaults (`resizer_create_new_transcript = true`, `playback_rate = 1.0`,
/// the two splitter fractions) that the derived all-zero `Default` would get
/// wrong. The manual [`Default`] impl below is the single source of truth for
/// those defaults; the container-level `#[serde(default)]` fills any missing
/// field from it, and the per-field `#[serde(default = "…")]` attributes make
/// the "section present, key absent" case explicit too (same belt-and-braces
/// pattern as [`UpdateSettings`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewerSettings {
    /// Whether loop-the-selected-segment playback is enabled.
    pub segment_looping_enabled: bool,
    /// Whether the playback-controls bar is shown (it also auto-shows when
    /// playback starts in Timestamps mode).
    pub playback_controls_visible: bool,
    /// Whether the find bar is shown.
    pub find_widget_visible: bool,
    /// Whether follow-audio (auto-select the segment at the playhead) is on.
    pub follow_audio_enabled: bool,
    /// Global playback speed multiplier (Buzz `AUDIO_PLAYBACK_RATE`). Clamped
    /// to `0.1..=5.0` when the settings file is loaded
    /// ([`ViewerSettings::clamp_playback_rate`]); a non-finite stored value
    /// resets to [`ViewerSettings::DEFAULT_PLAYBACK_RATE`].
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f32,
    /// Vertical-splitter fraction given to the editor table (vs. the player)
    /// for **audio** media — Buzz's 950:50 default.
    #[serde(default = "default_audio_splitter_fraction")]
    pub audio_splitter_fraction: f32,
    /// Vertical-splitter fraction given to the editor table (vs. the player)
    /// for **video** media — Buzz's 800:200 default.
    #[serde(default = "default_video_splitter_fraction")]
    pub video_splitter_fraction: f32,
    /// Whether the resize/merge window creates a new transcript (vs. editing
    /// in place). Buzz default: `true`.
    #[serde(default = "default_true")]
    pub resizer_create_new_transcript: bool,
    /// Shared viewer-window geometry (Buzz stores a single
    /// `transcription_viewer/geometry`): absent until the first viewer window
    /// closes, then last-close-wins across all open viewers.
    pub geometry: Option<ViewerGeometry>,
}

impl ViewerSettings {
    /// Lower bound on [`ViewerSettings::playback_rate`] (Buzz's speed clamp,
    /// `transcription_viewer_widget.py` speed-combo validation).
    pub const MIN_PLAYBACK_RATE: f32 = 0.1;
    /// Upper bound on [`ViewerSettings::playback_rate`].
    pub const MAX_PLAYBACK_RATE: f32 = 5.0;
    /// The default (and reset) playback rate.
    pub const DEFAULT_PLAYBACK_RATE: f32 = 1.0;

    /// Clamps a stored playback rate into `[MIN, MAX]`, resetting a non-finite
    /// value (`NaN`/`±inf` from a hand-edited file) to the default rather than
    /// propagating it. Applied by [`Settings::load`] so downstream readers
    /// never see an out-of-range or non-finite rate.
    pub fn clamp_playback_rate(rate: f32) -> f32 {
        if rate.is_finite() {
            rate.clamp(Self::MIN_PLAYBACK_RATE, Self::MAX_PLAYBACK_RATE)
        } else {
            Self::DEFAULT_PLAYBACK_RATE
        }
    }
}

impl Default for ViewerSettings {
    fn default() -> Self {
        ViewerSettings {
            segment_looping_enabled: false,
            playback_controls_visible: false,
            find_widget_visible: false,
            follow_audio_enabled: false,
            playback_rate: Self::DEFAULT_PLAYBACK_RATE,
            audio_splitter_fraction: 0.95,
            video_splitter_fraction: 0.8,
            resizer_create_new_transcript: true,
            geometry: None,
        }
    }
}

fn default_playback_rate() -> f32 {
    ViewerSettings::DEFAULT_PLAYBACK_RATE
}

fn default_audio_splitter_fraction() -> f32 {
    0.95
}

fn default_video_splitter_fraction() -> f32 {
    0.8
}

/// Shared viewer-window geometry (design spec §6): optional saved position
/// (`x`/`y` may be absent even when a size is known — e.g. a compositor that
/// doesn't report window position) and a size. Default size is 1000×800 with a
/// min of 800×500, applied by the GUI when this is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewerGeometry {
    /// Window x position in points, if known.
    pub x: Option<f32>,
    /// Window y position in points, if known.
    pub y: Option<f32>,
    /// Window width in points.
    pub w: f32,
    /// Window height in points.
    pub h: f32,
}

/// Live recording transcriber settings (F-060, F-061, F-064), ported from
/// Buzz's `recording-transcriber/*` `QSettings` group (F-060/F-061: model
/// options, chunking, display mode, hide_unconfirmed; F-064: export
/// configuration and upload URL). Sticky options are saved when the recording
/// window closes (Buzz parity, except Buzz saves export fields immediately).
///
/// `#[derive(Default)]` is deliberately not used: several fields have
/// non-zero defaults that would be lost. The manual [`Default`] impl below is
/// the single source of truth for those defaults; container-level
/// `#[serde(default)]` fills any missing field from it, and per-field
/// `#[serde(default = "…")]` attributes make "section present, key absent"
/// cases explicit (belt-and-braces pattern matching [`ViewerSettings`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingSettings {
    /// Last-selected language code (`crate::language::Language::code`), or
    /// `None` for auto-detect.
    pub language: Option<String>,
    /// Last-selected task (`crate::domain::TaskKind::as_str`).
    pub task: Option<String>,
    /// Last-selected model size (`crate::domain::ModelSize::as_str`).
    pub model_size: Option<String>,
    /// Last-entered custom initial prompt text.
    pub initial_prompt: Option<String>,
    /// Silence threshold for skipping batches (RMS). Clamped to the valid
    /// range `0.0..=1.0` when the settings file is loaded; a non-finite
    /// stored value resets to [`RecordingSettings::DEFAULT_SILENCE_THRESHOLD`].
    #[serde(default = "default_silence_threshold")]
    pub silence_threshold: f32,
    /// Line separator used in append modes (stored as literal, not escaped
    /// — the GUI sheet handles escaping/unescaping). Default is `"\n\n"`.
    #[serde(default = "default_line_separator")]
    pub line_separator: String,
    /// Chunk size in seconds for append-and-correct mode (rolling-prompt path).
    /// Clamped to `2.0..=5.0` when the settings file is loaded; a non-finite
    /// stored value resets to [`RecordingSettings::DEFAULT_TRANSCRIPTION_STEP`].
    #[serde(default = "default_transcription_step")]
    pub transcription_step: f64,
    /// Display mode index: 0 = AppendBelow, 1 = AppendAbove, 2 = AppendAndCorrect
    /// (Buzz parity: stored as index, not enum).
    #[serde(default = "default_mode")]
    pub mode: usize,
    /// Whether to hide unconfirmed text (last utterance) in append-and-correct
    /// mode. Default is `true`.
    #[serde(default = "default_true")]
    pub hide_unconfirmed: bool,
    /// Whether live export to TXT/CSV is enabled.
    pub export_enabled: bool,
    /// Directory to write exported files to.
    pub export_folder: Option<PathBuf>,
    /// Custom filename template ({{placeholders}}) for exports.
    pub export_file_name: Option<String>,
    /// Export file type: `"txt"` or `"csv"`. Default is `"txt"`.
    #[serde(default = "default_export_file_type")]
    pub export_file_type: String,
    /// Maximum number of entries to keep in the export file (0 = all,
    /// range 0–99). Clamped to `0..=99` when the settings file is loaded.
    #[serde(default = "default_export_max_entries")]
    pub export_max_entries: u32,
    /// Upload URL for realtime transcript posting (✚ humm addition, ADR-018).
    /// Overridden by `HUMM_UPLOAD_URL` env var if set.
    pub upload_url: Option<String>,
    /// Measured realtime speed per model size, persisted across sessions
    /// (✚ humm addition, ADR-018). Keyed by model-size wire string;
    /// a value < 1.0 triggers the realtime-fit warning.
    #[serde(default = "default_measured_speed")]
    pub measured_speed: BTreeMap<String, f32>,
}

impl RecordingSettings {
    /// Minimum transcription step (seconds).
    pub const MIN_TRANSCRIPTION_STEP: f64 = 2.0;
    /// Maximum transcription step (seconds).
    pub const MAX_TRANSCRIPTION_STEP: f64 = 5.0;
    /// Default transcription step (seconds, append-and-correct mode).
    pub const DEFAULT_TRANSCRIPTION_STEP: f64 = 3.5;
    /// Default silence threshold (RMS).
    pub const DEFAULT_SILENCE_THRESHOLD: f32 = 0.0025;
    /// Default line separator.
    pub const DEFAULT_LINE_SEPARATOR: &'static str = "\n\n";
    /// Default mode (index).
    pub const DEFAULT_MODE: usize = 0;
    /// Default export file type.
    pub const DEFAULT_EXPORT_FILE_TYPE: &'static str = "txt";
    /// Default max export entries.
    pub const DEFAULT_EXPORT_MAX_ENTRIES: u32 = 0;

    /// Clamps stored recording settings into their valid ranges on load,
    /// resetting non-finite values (`NaN`/`±inf` from a hand-edited file)
    /// to defaults rather than propagating them. Applied by [`Settings::load`]
    /// so downstream readers never see out-of-range or non-finite values.
    pub fn clamp(&mut self) {
        if self.transcription_step.is_finite() {
            self.transcription_step = self
                .transcription_step
                .clamp(Self::MIN_TRANSCRIPTION_STEP, Self::MAX_TRANSCRIPTION_STEP);
        } else {
            self.transcription_step = Self::DEFAULT_TRANSCRIPTION_STEP;
        }

        if self.silence_threshold.is_finite() {
            self.silence_threshold = self.silence_threshold.clamp(0.0, 1.0);
        } else {
            self.silence_threshold = Self::DEFAULT_SILENCE_THRESHOLD;
        }

        self.export_max_entries = self.export_max_entries.min(99);
    }
}

impl Default for RecordingSettings {
    fn default() -> Self {
        RecordingSettings {
            language: None,
            task: None,
            model_size: None,
            initial_prompt: None,
            silence_threshold: Self::DEFAULT_SILENCE_THRESHOLD,
            line_separator: Self::DEFAULT_LINE_SEPARATOR.to_string(),
            transcription_step: Self::DEFAULT_TRANSCRIPTION_STEP,
            mode: Self::DEFAULT_MODE,
            hide_unconfirmed: true,
            export_enabled: false,
            export_folder: None,
            export_file_name: None,
            export_file_type: Self::DEFAULT_EXPORT_FILE_TYPE.to_string(),
            export_max_entries: Self::DEFAULT_EXPORT_MAX_ENTRIES,
            upload_url: None,
            measured_speed: BTreeMap::new(),
        }
    }
}

/// Live presentation window settings (F-065), ported from Buzz's
/// `presentation-window/*` `QSettings` group. Display preferences for the
/// live-transcript presentation window: text size, theme, and colors.
///
/// `#[derive(Default)]` is deliberately not used: all fields have non-zero
/// defaults. The manual [`Default`] impl below is the single source of truth;
/// container-level `#[serde(default)]` fills any missing field from it, and
/// per-field `#[serde(default = "…")]` attributes make "section present,
/// key absent" cases explicit (belt-and-braces pattern).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresentationSettings {
    /// Font size in points, range 10–100. Clamped to this range when the
    /// settings file is loaded. Default is 24.
    #[serde(default = "default_presentation_text_size")]
    pub text_size: u32,
    /// Theme name: `"light"`, `"dark"`, or `"custom"`. Default is `"light"`.
    #[serde(default = "default_presentation_theme")]
    pub theme: String,
    /// Text color in hex format `#RRGGBB`. Default is `"#000000"` (black).
    #[serde(default = "default_presentation_text_color")]
    pub text_color: String,
    /// Background color in hex format `#RRGGBB`. Default is `"#FFFFFF"` (white).
    #[serde(default = "default_presentation_background_color")]
    pub background_color: String,
}

impl PresentationSettings {
    /// Minimum text size (points).
    pub const MIN_TEXT_SIZE: u32 = 10;
    /// Maximum text size (points).
    pub const MAX_TEXT_SIZE: u32 = 100;
    /// Default text size (points).
    pub const DEFAULT_TEXT_SIZE: u32 = 24;
    /// Default theme.
    pub const DEFAULT_THEME: &'static str = "light";
    /// Default text color.
    pub const DEFAULT_TEXT_COLOR: &'static str = "#000000";
    /// Default background color.
    pub const DEFAULT_BACKGROUND_COLOR: &'static str = "#FFFFFF";

    /// Clamps stored presentation settings into their valid ranges on load.
    /// Applied by [`Settings::load`].
    pub fn clamp(&mut self) {
        self.text_size = self
            .text_size
            .clamp(Self::MIN_TEXT_SIZE, Self::MAX_TEXT_SIZE);
    }
}

impl Default for PresentationSettings {
    fn default() -> Self {
        PresentationSettings {
            text_size: Self::DEFAULT_TEXT_SIZE,
            theme: Self::DEFAULT_THEME.to_string(),
            text_color: Self::DEFAULT_TEXT_COLOR.to_string(),
            background_color: Self::DEFAULT_BACKGROUND_COLOR.to_string(),
        }
    }
}

fn default_silence_threshold() -> f32 {
    RecordingSettings::DEFAULT_SILENCE_THRESHOLD
}

fn default_line_separator() -> String {
    RecordingSettings::DEFAULT_LINE_SEPARATOR.to_string()
}

fn default_transcription_step() -> f64 {
    RecordingSettings::DEFAULT_TRANSCRIPTION_STEP
}

fn default_mode() -> usize {
    RecordingSettings::DEFAULT_MODE
}

fn default_export_file_type() -> String {
    RecordingSettings::DEFAULT_EXPORT_FILE_TYPE.to_string()
}

fn default_export_max_entries() -> u32 {
    RecordingSettings::DEFAULT_EXPORT_MAX_ENTRIES
}

fn default_measured_speed() -> BTreeMap<String, f32> {
    BTreeMap::new()
}

fn default_presentation_text_size() -> u32 {
    PresentationSettings::DEFAULT_TEXT_SIZE
}

fn default_presentation_theme() -> String {
    PresentationSettings::DEFAULT_THEME.to_string()
}

fn default_presentation_text_color() -> String {
    PresentationSettings::DEFAULT_TEXT_COLOR.to_string()
}

fn default_presentation_background_color() -> String {
    PresentationSettings::DEFAULT_BACKGROUND_COLOR.to_string()
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
        let mut settings: Settings =
            toml::from_str(&content).map_err(|error| SettingsError::Parse {
                path: path.to_path_buf(),
                detail: error.to_string(),
            })?;
        // Normalize persisted settings into their valid ranges on load (design
        // spec §6): hand-edited or corrupt files must never hand downstream
        // out-of-range or non-finite values.
        settings.viewer.playback_rate =
            ViewerSettings::clamp_playback_rate(settings.viewer.playback_rate);
        settings.recording.clamp();
        settings.presentation.clamp();
        Ok(settings)
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
    use super::{
        PresentationSettings, RecordingSettings, Settings, SettingsError, ViewerGeometry,
        ViewerSettings,
    };
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
        settings.viewer.segment_looping_enabled = true;
        settings.viewer.playback_controls_visible = true;
        settings.viewer.find_widget_visible = true;
        settings.viewer.follow_audio_enabled = true;
        // In-range so load-time clamping leaves it untouched (the clamp path is
        // exercised separately in `playback_rate_clamped_on_load`).
        settings.viewer.playback_rate = 1.5;
        settings.viewer.audio_splitter_fraction = 0.9;
        settings.viewer.video_splitter_fraction = 0.75;
        settings.viewer.resizer_create_new_transcript = false;
        settings.viewer.geometry = Some(ViewerGeometry {
            x: Some(100.0),
            y: Some(50.0),
            w: 1100.0,
            h: 850.0,
        });

        settings.save(&path).expect("save");
        let loaded = Settings::load(&path).expect("load");
        assert_eq!(loaded, settings);
    }

    #[test]
    fn viewer_settings_defaults_match_buzz() {
        // Buzz defaults (design spec §6): every toggle off, rate 1.0, splitters
        // 0.95 (audio 950:50) / 0.8 (video 800:200), create-new on, no geometry.
        let viewer = ViewerSettings::default();
        assert!(!viewer.segment_looping_enabled);
        assert!(!viewer.playback_controls_visible);
        assert!(!viewer.find_widget_visible);
        assert!(!viewer.follow_audio_enabled);
        assert_eq!(viewer.playback_rate, 1.0);
        assert_eq!(viewer.audio_splitter_fraction, 0.95);
        assert_eq!(viewer.video_splitter_fraction, 0.8);
        assert!(viewer.resizer_create_new_transcript);
        assert_eq!(viewer.geometry, None);
        // Missing `[viewer]` section entirely also yields exactly these.
        assert_eq!(Settings::default().viewer, viewer);
    }

    #[test]
    fn viewer_defaults_apply_when_section_present_but_partial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        // Only one key set — every other viewer field must still take its Buzz
        // default, not the type's zero value (create-new true, rate 1.0, …).
        std::fs::write(&path, "[viewer]\nfind_widget_visible = true\n").expect("write file");

        let loaded = Settings::load(&path).expect("load");
        assert!(loaded.viewer.find_widget_visible);
        assert!(loaded.viewer.resizer_create_new_transcript);
        assert_eq!(loaded.viewer.playback_rate, 1.0);
        assert_eq!(loaded.viewer.audio_splitter_fraction, 0.95);
        assert_eq!(loaded.viewer.video_splitter_fraction, 0.8);
    }

    #[test]
    fn playback_rate_clamped_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        // Above the max → clamped down to 5.0.
        std::fs::write(&path, "[viewer]\nplayback_rate = 10.0\n").expect("write file");
        assert_eq!(
            Settings::load(&path).expect("load").viewer.playback_rate,
            5.0
        );

        // Below the min → clamped up to 0.1.
        std::fs::write(&path, "[viewer]\nplayback_rate = 0.01\n").expect("write file");
        assert_eq!(
            Settings::load(&path).expect("load").viewer.playback_rate,
            0.1
        );

        // Non-finite → reset to the default 1.0.
        std::fs::write(&path, "[viewer]\nplayback_rate = nan\n").expect("write file");
        assert_eq!(
            Settings::load(&path).expect("load").viewer.playback_rate,
            1.0
        );
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

    // RecordingSettings tests

    #[test]
    fn recording_settings_defaults_match_spec() {
        let rec = RecordingSettings::default();
        assert_eq!(rec.silence_threshold, 0.0025);
        assert_eq!(rec.line_separator, "\n\n");
        assert_eq!(rec.transcription_step, 3.5);
        assert_eq!(rec.mode, 0);
        assert!(rec.hide_unconfirmed);
        assert!(!rec.export_enabled);
        assert_eq!(rec.export_file_type, "txt");
        assert_eq!(rec.export_max_entries, 0);
        assert!(rec.language.is_none());
        assert!(rec.task.is_none());
        assert!(rec.model_size.is_none());
        assert!(rec.initial_prompt.is_none());
        assert!(rec.export_folder.is_none());
        assert!(rec.export_file_name.is_none());
        assert!(rec.upload_url.is_none());
        assert!(rec.measured_speed.is_empty());
    }

    #[test]
    fn recording_defaults_apply_when_section_present_but_partial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "[recording]\nlanguage = \"es\"\nsilence_threshold = 0.005\n",
        )
        .expect("write file");

        let loaded = Settings::load(&path).expect("load");
        assert_eq!(loaded.recording.language, Some("es".to_string()));
        assert_eq!(loaded.recording.silence_threshold, 0.005);
        // Other fields take their defaults
        assert_eq!(loaded.recording.transcription_step, 3.5);
        assert_eq!(loaded.recording.line_separator, "\n\n");
        assert_eq!(loaded.recording.mode, 0);
        assert!(loaded.recording.hide_unconfirmed);
        assert_eq!(loaded.recording.export_file_type, "txt");
    }

    #[test]
    fn recording_transcription_step_clamped_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        // Below the min → clamped up to 2.0.
        std::fs::write(&path, "[recording]\ntranscription_step = 1.0\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .transcription_step,
            2.0
        );

        // Above the max → clamped down to 5.0.
        std::fs::write(&path, "[recording]\ntranscription_step = 9.9\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .transcription_step,
            5.0
        );

        // Non-finite → reset to the default 3.5.
        std::fs::write(&path, "[recording]\ntranscription_step = nan\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .transcription_step,
            3.5
        );
    }

    #[test]
    fn recording_silence_threshold_clamped_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        // Below zero → clamped to 0.0.
        std::fs::write(&path, "[recording]\nsilence_threshold = -0.5\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .silence_threshold,
            0.0
        );

        // Above 1.0 → clamped to 1.0.
        std::fs::write(&path, "[recording]\nsilence_threshold = 5.0\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .silence_threshold,
            1.0
        );

        // Non-finite → reset to the default 0.0025.
        std::fs::write(&path, "[recording]\nsilence_threshold = inf\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .silence_threshold,
            0.0025
        );
    }

    #[test]
    fn recording_export_max_entries_clamped_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        // Above the max → clamped to 99.
        std::fs::write(&path, "[recording]\nexport_max_entries = 250\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .export_max_entries,
            99
        );

        // Within range → unchanged.
        std::fs::write(&path, "[recording]\nexport_max_entries = 50\n").expect("write file");
        assert_eq!(
            Settings::load(&path)
                .expect("load")
                .recording
                .export_max_entries,
            50
        );
    }

    #[test]
    fn recording_round_trip_all_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        let mut settings = Settings::default();
        settings.recording.language = Some("fr".to_string());
        settings.recording.task = Some("translate".to_string());
        settings.recording.model_size = Some("large-v3".to_string());
        settings.recording.initial_prompt = Some("proper nouns: Buzz".to_string());
        settings.recording.silence_threshold = 0.003;
        settings.recording.line_separator = "\n".to_string();
        settings.recording.transcription_step = 4.0;
        settings.recording.mode = 2;
        settings.recording.hide_unconfirmed = false;
        settings.recording.export_enabled = true;
        settings.recording.export_folder = Some(Path::new("/tmp/exports").to_path_buf());
        settings.recording.export_file_name = Some("my_export_{{ date_time }}".to_string());
        settings.recording.export_file_type = "csv".to_string();
        settings.recording.export_max_entries = 25;
        settings.recording.upload_url = Some("https://example.com/upload".to_string());
        settings
            .recording
            .measured_speed
            .insert("small".to_string(), 2.5);
        settings
            .recording
            .measured_speed
            .insert("base".to_string(), 1.1);

        settings.save(&path).expect("save");
        let loaded = Settings::load(&path).expect("load");

        assert_eq!(loaded.recording.language, Some("fr".to_string()));
        assert_eq!(loaded.recording.task, Some("translate".to_string()));
        assert_eq!(loaded.recording.model_size, Some("large-v3".to_string()));
        assert_eq!(
            loaded.recording.initial_prompt,
            Some("proper nouns: Buzz".to_string())
        );
        assert_eq!(loaded.recording.silence_threshold, 0.003);
        assert_eq!(loaded.recording.line_separator, "\n");
        assert_eq!(loaded.recording.transcription_step, 4.0);
        assert_eq!(loaded.recording.mode, 2);
        assert!(!loaded.recording.hide_unconfirmed);
        assert!(loaded.recording.export_enabled);
        assert_eq!(
            loaded.recording.export_folder,
            Some(Path::new("/tmp/exports").to_path_buf())
        );
        assert_eq!(
            loaded.recording.export_file_name,
            Some("my_export_{{ date_time }}".to_string())
        );
        assert_eq!(loaded.recording.export_file_type, "csv");
        assert_eq!(loaded.recording.export_max_entries, 25);
        assert_eq!(
            loaded.recording.upload_url,
            Some("https://example.com/upload".to_string())
        );
        assert_eq!(loaded.recording.measured_speed.get("small"), Some(&2.5));
        assert_eq!(loaded.recording.measured_speed.get("base"), Some(&1.1));
    }

    // PresentationSettings tests

    #[test]
    fn presentation_settings_defaults_match_spec() {
        let pres = PresentationSettings::default();
        assert_eq!(pres.text_size, 24);
        assert_eq!(pres.theme, "light");
        assert_eq!(pres.text_color, "#000000");
        assert_eq!(pres.background_color, "#FFFFFF");
    }

    #[test]
    fn presentation_defaults_apply_when_section_present_but_partial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "[presentation]\ntheme = \"dark\"\n").expect("write file");

        let loaded = Settings::load(&path).expect("load");
        assert_eq!(loaded.presentation.theme, "dark");
        // Other fields take their defaults
        assert_eq!(loaded.presentation.text_size, 24);
        assert_eq!(loaded.presentation.text_color, "#000000");
        assert_eq!(loaded.presentation.background_color, "#FFFFFF");
    }

    #[test]
    fn presentation_text_size_clamped_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        // Below the min → clamped up to 10.
        std::fs::write(&path, "[presentation]\ntext_size = 5\n").expect("write file");
        assert_eq!(
            Settings::load(&path).expect("load").presentation.text_size,
            10
        );

        // Above the max → clamped down to 100.
        std::fs::write(&path, "[presentation]\ntext_size = 500\n").expect("write file");
        assert_eq!(
            Settings::load(&path).expect("load").presentation.text_size,
            100
        );

        // Within range → unchanged.
        std::fs::write(&path, "[presentation]\ntext_size = 50\n").expect("write file");
        assert_eq!(
            Settings::load(&path).expect("load").presentation.text_size,
            50
        );
    }

    #[test]
    fn presentation_round_trip_all_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        let mut settings = Settings::default();
        settings.presentation.text_size = 48;
        settings.presentation.theme = "dark".to_string();
        settings.presentation.text_color = "#FFFFFF".to_string();
        settings.presentation.background_color = "#1a1a1a".to_string();

        settings.save(&path).expect("save");
        let loaded = Settings::load(&path).expect("load");

        assert_eq!(loaded.presentation.text_size, 48);
        assert_eq!(loaded.presentation.theme, "dark");
        assert_eq!(loaded.presentation.text_color, "#FFFFFF");
        assert_eq!(loaded.presentation.background_color, "#1a1a1a");
    }

    #[test]
    fn recording_and_presentation_sections_absent_give_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "").expect("write empty file");

        let loaded = Settings::load(&path).expect("load");
        assert_eq!(loaded.recording, RecordingSettings::default());
        assert_eq!(loaded.presentation, PresentationSettings::default());
    }
}
