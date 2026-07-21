//! Update-check logic (F-006, ADR-015): a notify-only version check against
//! humm's own release manifest. Ported from Buzz's `update_checker.py` —
//! [`is_newer_version`] mirrors `_is_newer_version` (`update_checker.py:147-163`),
//! [`should_check`] mirrors `should_check_for_updates` (`update_checker.py:42-67`),
//! and [`parse_version_info`] mirrors the JSON handling in
//! `_on_reply_finished`/`_get_download_url` (`update_checker.py:93-145`).
//!
//! This milestone ships the pure decision logic only — no scheduling, no
//! network call wired to app startup, no GUI notice (that arrives in M3,
//! ADR-015). Callers persist `last_check`/`available_version` themselves via
//! [`crate::settings::UpdateSettings`] (Task 2); nothing in this module reads
//! or writes `Settings` — it takes and returns plain values.
//!
//! Buzz gates the whole check on `platform.system() in ("Windows", "Darwin")`
//! (`update_checker.py:44-47`), skipping Linux outright. humm targets macOS
//! only, so that platform gate is deliberately **not** ported: there is no
//! other platform for humm to run on, so a "skip on Linux" branch would be
//! dead code.

use chrono::NaiveDateTime;
use serde::Deserialize;

/// Minimum days between update checks (`update_checker.py:25`,
/// `CHECK_INTERVAL_DAYS`).
pub const CHECK_INTERVAL_DAYS: i64 = 7;

/// humm's own update-manifest endpoint (Q-8: Buzz's `chidiwilliams/buzz`
/// GitHub endpoint is not reused). humm's repo is hosted on Gitea
/// (`repo.digikal.com`), which serves `releases/latest/download/<asset>`
/// URLs the same way GitHub does. No release has been published yet, so this
/// URL currently 404s — [`check_for_update`] surfaces that as
/// [`UpdateError::Http`], which is harmless and expected until the first
/// release exists (see ADR-015's endpoint note in `docs/design/DECISIONS.md`).
pub const VERSION_INFO_URL: &str =
    "https://repo.digikal.com/jawad/humm/releases/latest/download/version_info.json";

/// A parsed update manifest describing a version newer than the caller's
/// current version (`update_checker.py`'s `UpdateInfo` dataclass,
/// `update_checker.py:14-18`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    /// The newer version string reported by the manifest.
    pub version: String,
    /// Release notes for the newer version, if published.
    pub release_notes: String,
    /// Download URL(s) for the caller's platform/arch. Always normalized to
    /// a list, whether the manifest published a single string or a list
    /// (`_get_download_url`, `update_checker.py:131-145`).
    pub download_urls: Vec<String>,
}

/// Errors from the update-check network/parse boundary. Standalone (not
/// folded into [`crate::error::CoreError`]) — update-checking is optional,
/// best-effort background behavior, not part of the transcription pipeline.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UpdateError {
    /// The manifest fetch failed at the transport level, or returned a
    /// non-success HTTP status (including the expected 404 before humm's
    /// first release is published).
    #[error("update check failed: {detail}")]
    Http {
        /// Diagnostic detail.
        detail: String,
    },
    /// The manifest body was not valid JSON.
    #[error("failed to parse update manifest: {detail}")]
    Parse {
        /// Diagnostic detail.
        detail: String,
    },
}

/// Compares `current` against `remote` as numeric dotted versions, zero-padding
/// the shorter side, exactly like Buzz's `_is_newer_version`
/// (`update_checker.py:147-163`): each dot-separated component must parse as
/// an integer; if either version has any non-numeric component (including a
/// pre-release suffix like `"2.0.0-beta"`), Buzz's `int()` raises
/// `ValueError` and it returns `False` — ported here as returning `false`
/// rather than propagating a parse error, since a malformed remote version is
/// not a hard failure, just "no update".
pub fn is_newer_version(current: &str, remote: &str) -> bool {
    let (Some(mut current_parts), Some(mut remote_parts)) = (
        parse_numeric_version(current),
        parse_numeric_version(remote),
    ) else {
        return false;
    };

    while current_parts.len() < remote_parts.len() {
        current_parts.push(0);
    }
    while remote_parts.len() < current_parts.len() {
        remote_parts.push(0);
    }

    remote_parts > current_parts
}

/// Parses a dotted version string into its integer components, or `None` if
/// any component fails to parse as an integer (mirrors Python's `int(x)`
/// raising `ValueError` on a non-numeric part).
fn parse_numeric_version(version: &str) -> Option<Vec<i64>> {
    version
        .split('.')
        .map(|part| part.parse::<i64>().ok())
        .collect()
}

/// Whether another update check should run now, given the ISO-8601 timestamp
/// of the last check (naive-local, `%Y-%m-%dT%H:%M:%S%.f` — humm's
/// convention for Buzz-equivalent timestamps, matching
/// `datetime.now().isoformat()`; see `humm-store`'s `now_iso`), `now`, and
/// whether checking is disabled.
///
/// Ports Buzz's `should_check_for_updates` (`update_checker.py:42-67`) minus
/// its Windows/macOS platform gate (see the module doc comment): `disabled`
/// always wins and returns `false`; a missing or unparseable `last_check_iso`
/// always returns `true` (Buzz's `except ValueError: pass` falls through to
/// its final `return True`); otherwise a check is due once at least
/// [`CHECK_INTERVAL_DAYS`] whole days have passed.
pub fn should_check(last_check_iso: Option<&str>, now: NaiveDateTime, disabled: bool) -> bool {
    if disabled {
        return false;
    }

    let Some(last_check_iso) = last_check_iso else {
        return true;
    };

    let Ok(last_check) = NaiveDateTime::parse_from_str(last_check_iso, "%Y-%m-%dT%H:%M:%S%.f")
    else {
        return true;
    };

    (now - last_check).num_days() >= CHECK_INTERVAL_DAYS
}

/// The shape of `version_info.json` that this module cares about
/// (`update_checker.py:93-99`). Every field defaults when absent, matching
/// Buzz's `data.get(key, default)` semantics — a manifest missing a key is
/// not a parse error.
#[derive(Debug, Deserialize)]
struct VersionInfoJson {
    #[serde(default)]
    version: String,
    #[serde(default)]
    release_notes: String,
    #[serde(default)]
    download_urls: std::collections::HashMap<String, DownloadUrlValue>,
}

/// A `download_urls` entry, tolerating either a single URL string or a list
/// of them (`_get_download_url`'s `urls if isinstance(urls, list) else
/// [urls]`, `update_checker.py:145`).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DownloadUrlValue {
    Single(String),
    Multiple(Vec<String>),
}

impl DownloadUrlValue {
    fn into_vec(self) -> Vec<String> {
        match self {
            DownloadUrlValue::Single(url) => vec![url],
            DownloadUrlValue::Multiple(urls) => urls,
        }
    }
}

/// Parses a `version_info.json` body and decides whether it describes an
/// update over `current_version`, picking `download_urls[arch_key]` for the
/// caller's platform (`arch_key` is `"macos_arm"` or `"macos_x86"` — the
/// caller selects via `cfg!(target_arch = ...)`; humm never runs on
/// Windows, so `"windows_x64"` is never a valid `arch_key` here even though
/// Buzz's manifest shape reserves it).
///
/// Returns `Ok(None)` when `remote`'s version is not newer than
/// `current_version` (equal or older) — not an error, just "nothing to do".
/// A missing `download_urls[arch_key]` defaults to an empty list rather than
/// an error (Buzz's `.get(key, [])`). Invalid JSON is [`UpdateError::Parse`].
pub fn parse_version_info(
    json: &str,
    current_version: &str,
    arch_key: &str,
) -> Result<Option<UpdateInfo>, UpdateError> {
    let mut manifest: VersionInfoJson =
        serde_json::from_str(json).map_err(|e| UpdateError::Parse {
            detail: e.to_string(),
        })?;

    if !is_newer_version(current_version, &manifest.version) {
        return Ok(None);
    }

    let download_urls = manifest
        .download_urls
        .remove(arch_key)
        .map(DownloadUrlValue::into_vec)
        .unwrap_or_default();

    Ok(Some(UpdateInfo {
        version: manifest.version,
        release_notes: manifest.release_notes,
        download_urls,
    }))
}

/// Fetches [`VERSION_INFO_URL`] (or `url`, for tests/mirrors) and parses it —
/// a thin composition of a blocking `GET` and [`parse_version_info`], with
/// `arch_key` selected via `cfg!(target_arch = ...)` (humm targets macOS
/// only: `aarch64` maps to `"macos_arm"`, anything else to `"macos_x86"`).
/// No test exercises this function over the network (Global Constraint 3);
/// its two collaborators are covered directly on string fixtures.
pub fn check_for_update(
    url: &str,
    current_version: &str,
) -> Result<Option<UpdateInfo>, UpdateError> {
    let arch_key = if cfg!(target_arch = "aarch64") {
        "macos_arm"
    } else {
        "macos_x86"
    };

    let response = reqwest::blocking::get(url).map_err(|e| UpdateError::Http {
        detail: e.to_string(),
    })?;
    if !response.status().is_success() {
        return Err(UpdateError::Http {
            detail: format!("{url}: HTTP {}", response.status()),
        });
    }
    let body = response.text().map_err(|e| UpdateError::Http {
        detail: e.to_string(),
    })?;

    parse_version_info(&body, current_version, arch_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // --- is_newer_version: update_checker.py:147-163 table ---

    #[test]
    fn is_newer_version_patch_bump_is_newer() {
        assert!(is_newer_version("1.4.5", "1.4.6"));
    }

    #[test]
    fn is_newer_version_equal_is_not_newer() {
        assert!(!is_newer_version("1.4.5", "1.4.5"));
    }

    #[test]
    fn is_newer_version_shorter_remote_is_zero_padded() {
        // "1.5" pads to "1.5.0", which is still newer than "1.4.5".
        assert!(is_newer_version("1.4.5", "1.5"));
    }

    #[test]
    fn is_newer_version_numeric_comparison_not_lexicographic() {
        // "1.10.0" > "1.9.9" numerically, but lexicographic string
        // comparison would get this backwards ("1.10" < "1.9").
        assert!(!is_newer_version("1.10.0", "1.9.9"));
    }

    #[test]
    fn is_newer_version_non_numeric_component_is_never_newer() {
        // Buzz's int() raises ValueError on "0-beta" -> caught -> False.
        assert!(!is_newer_version("1.4.5", "2.0.0-beta"));
    }

    // --- should_check: update_checker.py:42-67 ---

    fn fixed_now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 21)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn days_ago(days: i64) -> String {
        (fixed_now() - chrono::Duration::days(days))
            .format("%Y-%m-%dT%H:%M:%S%.f")
            .to_string()
    }

    #[test]
    fn should_check_false_when_disabled_regardless_of_last_check() {
        assert!(!should_check(None, fixed_now(), true));
        assert!(!should_check(
            Some(days_ago(30).as_str()),
            fixed_now(),
            true
        ));
    }

    #[test]
    fn should_check_true_when_never_checked() {
        assert!(should_check(None, fixed_now(), false));
    }

    #[test]
    fn should_check_false_when_checked_six_days_ago() {
        let last_check = days_ago(6);
        assert!(!should_check(Some(last_check.as_str()), fixed_now(), false));
    }

    #[test]
    fn should_check_true_when_checked_eight_days_ago() {
        let last_check = days_ago(8);
        assert!(should_check(Some(last_check.as_str()), fixed_now(), false));
    }

    #[test]
    fn should_check_true_on_unparseable_last_check() {
        // Buzz: invalid ISO format -> ValueError -> caught -> falls through
        // to `return True`.
        assert!(should_check(Some("not-a-date"), fixed_now(), false));
    }

    // --- parse_version_info: update_checker.py:93-145 ---

    #[test]
    fn parse_version_info_newer_with_scalar_download_url() {
        let json = r#"{
            "version": "2.0.0",
            "release_notes": "Bug fixes",
            "download_urls": {
                "macos_arm": "https://example.com/humm-2.0.0-arm64.dmg",
                "macos_x86": "https://example.com/humm-2.0.0-x86_64.dmg"
            }
        }"#;

        let info = parse_version_info(json, "1.9.0", "macos_arm")
            .expect("valid json")
            .expect("newer version");

        assert_eq!(info.version, "2.0.0");
        assert_eq!(info.release_notes, "Bug fixes");
        assert_eq!(
            info.download_urls,
            vec!["https://example.com/humm-2.0.0-arm64.dmg".to_string()]
        );
    }

    #[test]
    fn parse_version_info_newer_with_list_download_url() {
        let json = r#"{
            "version": "2.0.0",
            "release_notes": "Bug fixes",
            "download_urls": {
                "macos_arm": [
                    "https://example.com/humm-2.0.0-arm64.dmg",
                    "https://mirror.example.com/humm-2.0.0-arm64.dmg"
                ]
            }
        }"#;

        let info = parse_version_info(json, "1.9.0", "macos_arm")
            .expect("valid json")
            .expect("newer version");

        assert_eq!(
            info.download_urls,
            vec![
                "https://example.com/humm-2.0.0-arm64.dmg".to_string(),
                "https://mirror.example.com/humm-2.0.0-arm64.dmg".to_string(),
            ]
        );
    }

    #[test]
    fn parse_version_info_equal_version_is_none() {
        let json = r#"{"version": "1.9.0", "release_notes": "", "download_urls": {}}"#;
        assert_eq!(
            parse_version_info(json, "1.9.0", "macos_arm").unwrap(),
            None
        );
    }

    #[test]
    fn parse_version_info_older_version_is_none() {
        let json = r#"{"version": "1.0.0", "release_notes": "", "download_urls": {}}"#;
        assert_eq!(
            parse_version_info(json, "1.9.0", "macos_arm").unwrap(),
            None
        );
    }

    #[test]
    fn parse_version_info_missing_keys_default_instead_of_erroring() {
        // Only "version" present: release_notes, download_urls both absent.
        let json = r#"{"version": "2.0.0"}"#;
        let info = parse_version_info(json, "1.9.0", "macos_arm")
            .expect("valid json")
            .expect("newer version");

        assert_eq!(info.version, "2.0.0");
        assert_eq!(info.release_notes, "");
        assert_eq!(info.download_urls, Vec::<String>::new());
    }

    #[test]
    fn parse_version_info_missing_arch_key_defaults_to_empty_list() {
        let json = r#"{
            "version": "2.0.0",
            "release_notes": "",
            "download_urls": {"macos_x86": "https://example.com/x86.dmg"}
        }"#;
        let info = parse_version_info(json, "1.9.0", "macos_arm")
            .expect("valid json")
            .expect("newer version");

        assert_eq!(info.download_urls, Vec::<String>::new());
    }

    #[test]
    fn parse_version_info_invalid_json_is_parse_error() {
        let err = parse_version_info("not json", "1.9.0", "macos_arm").unwrap_err();
        assert!(matches!(err, UpdateError::Parse { .. }));
    }
}
