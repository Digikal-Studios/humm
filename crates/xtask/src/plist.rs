//! Renders `Contents/Info.plist` for the assembled `humm.app` bundle
//! (Task 10 brief, spec §1.3). No signing/dylib work happens here — this
//! module only produces the plist XML string; [`crate::bundle::assemble`]
//! writes it to disk.
//!
//! Values are the literal ones the brief and the M9 design spec (§1.3)
//! pin: `CFBundleIdentifier com.digikal.humm`, `CFBundleExecutable
//! humm-gui` (the CLI binary also ships in `Contents/MacOS/` for power
//! users to symlink onto `PATH`, but the *bundle's* executable — the one
//! Launch Services runs on a Finder double-click — is the GUI),
//! `CFBundlePackageType APPL`, `NSHighResolutionCapable`, and
//! `NSMicrophoneUsageDescription` (required the moment recording moves
//! into a signed/sandboxed bundle — the dev binary run from a terminal
//! inherited the terminal's own mic grant, so this never had to exist
//! before M9).

/// `CFBundleIdentifier` (task brief, verbatim).
pub const BUNDLE_IDENTIFIER: &str = "com.digikal.humm";

/// `CFBundleName`/`CFBundleDisplayName` (task brief, verbatim).
pub const BUNDLE_NAME: &str = "humm";

/// `CFBundleExecutable` — the GUI binary; Launch Services runs this on a
/// Finder double-click of the `.app`.
pub const BUNDLE_EXECUTABLE: &str = "humm-gui";

/// `NSMicrophoneUsageDescription` (task brief, verbatim copy from the plan
/// — do not reword; it is user-facing consent text).
pub const MIC_USAGE_DESCRIPTION: &str = "humm records audio for live transcription.";

/// `LSMinimumSystemVersion`. Not guessed: discovered by building both
/// `humm-gui` and `humm` in release mode and reading `LC_BUILD_VERSION`'s
/// `minos` field off the real binaries via `otool -l` (Task 10 environment
/// note item 3) — both report `minos 11.0` (`sdk 26.5`, platform 1 =
/// macOS), so that is the toolchain's actual deployment target, not a
/// guess. See ADR-025 for the exact command/output this was read from.
pub const MIN_SYSTEM_VERSION: &str = "11.0";

/// Renders the full `Info.plist` XML for version `version` (the workspace
/// version — callers pass `env!("CARGO_PKG_VERSION")` of xtask itself,
/// which shares `[workspace.package].version` with every other crate, so
/// this string is self-updating across releases with no separate constant
/// to maintain).
///
/// `CFBundleShortVersionString` and `CFBundleVersion` both get `version`:
/// Buzz/humm don't distinguish marketing version from build number, so a
/// single value serves both keys (matches Buzz's own `Info.plist`
/// generation, one version string throughout).
pub fn render(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>{BUNDLE_IDENTIFIER}</string>
    <key>CFBundleName</key>
    <string>{BUNDLE_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>{BUNDLE_NAME}</string>
    <key>CFBundleExecutable</key>
    <string>{BUNDLE_EXECUTABLE}</string>
    <key>CFBundleIconFile</key>
    <string>humm.icns</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>{MIC_USAGE_DESCRIPTION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>{MIN_SYSTEM_VERSION}</string>
</dict>
</plist>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contains_bundle_id_version_and_mic_usage() {
        let xml = render("9.9.9");

        assert!(
            xml.contains("<string>com.digikal.humm</string>"),
            "missing CFBundleIdentifier value: {xml}"
        );
        assert!(
            xml.contains("<key>CFBundleExecutable</key>\n    <string>humm-gui</string>"),
            "CFBundleExecutable must be humm-gui: {xml}"
        );
        assert!(
            xml.contains("<key>CFBundlePackageType</key>\n    <string>APPL</string>"),
            "missing CFBundlePackageType APPL: {xml}"
        );
        assert!(
            xml.contains("<key>NSHighResolutionCapable</key>\n    <true/>"),
            "missing NSHighResolutionCapable: {xml}"
        );
        // Version must appear for both the short version and the build
        // version keys — two occurrences, not one.
        assert_eq!(
            xml.matches("<string>9.9.9</string>").count(),
            2,
            "expected version 9.9.9 to fill both CFBundleShortVersionString \
             and CFBundleVersion: {xml}"
        );
        assert!(
            xml.contains(
                "<key>NSMicrophoneUsageDescription</key>\n    \
                 <string>humm records audio for live transcription.</string>"
            ),
            "NSMicrophoneUsageDescription must be the exact approved copy: {xml}"
        );
    }
}
