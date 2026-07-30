//! Renders `version_info.json` (Task 10 brief, spec §1.3), consumed at
//! runtime by [`humm_core::update::parse_version_info`] — the shape is
//! pinned by that parser, not chosen freestanding here, so this module
//! builds a `serde_json::Value` and serializes it rather than
//! string-formatting JSON by hand (the brief's explicit requirement: "so
//! escaping is real, never string-formatted JSON").

/// Renders the `version_info.json` body for `version`, with `notes` as the
/// release notes and a single macOS arm64 DMG download URL following
/// humm's release-asset naming convention
/// (`humm-<version>-macos-arm64.dmg`, published at
/// `github.com/digikal/humm/releases/download/v<version>/`).
///
/// Schema (matches `humm_core::update::parse_version_info`'s
/// `VersionInfoJson`/`DownloadUrlValue`):
///
/// ```json
/// {
///   "version": "<version>",
///   "release_notes": "<notes>",
///   "download_urls": {
///     "macos_arm": ["https://github.com/digikal/humm/releases/download/v<version>/humm-<version>-macos-arm64.dmg"]
///   }
/// }
/// ```
pub fn render(version: &str, notes: &str) -> String {
    let dmg_url = format!(
        "https://github.com/digikal/humm/releases/download/v{version}/humm-{version}-macos-arm64.dmg"
    );

    let manifest = serde_json::json!({
        "version": version,
        "release_notes": notes,
        "download_urls": {
            "macos_arm": [dmg_url],
        },
    });

    serde_json::to_string_pretty(&manifest)
        .expect("a serde_json::Value built from strings always serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn version_info_round_trips_through_update_parser() {
        let json = render("9.9.9", "notes");

        let info = humm_core::update::parse_version_info(&json, "1.0.0", "macos_arm")
            .expect("valid json")
            .expect("9.9.9 is newer than 1.0.0");

        assert_eq!(info.version, "9.9.9");
        assert_eq!(info.release_notes, "notes");
        assert_eq!(
            info.download_urls,
            vec![
                "https://github.com/digikal/humm/releases/download/v9.9.9/humm-9.9.9-macos-arm64.dmg"
                    .to_string()
            ]
        );
    }
}
