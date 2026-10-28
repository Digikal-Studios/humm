//! OS name and install hints that differ between macOS, Linux, and Windows.
//!
//! Every function takes the OS string (or other plain data) as an argument
//! so the Windows and Linux cases run in tests on any host. Callers pass
//! [`std::env::consts::OS`] at the edge.

/// Python `platform.system()` spelling for a Rust [`std::env::consts::OS`]
/// value. Buzz telemetry sends `"Darwin"`, `"Linux"`, and `"Windows"`.
/// Rust's constant is `"macos"`, `"linux"`, and `"windows"`.
pub fn platform_system_name(os: &str) -> &str {
    match os {
        "macos" => "Darwin",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
}

/// Actionable install clause for a missing subprocess, keyed by Rust's
/// [`std::env::consts::OS`] (`"macos"`, `"linux"`, `"windows"`).
///
/// `demucs` is a pip package on every OS. ffmpeg and yt-dlp use the package
/// manager that actually exists on that OS. ffprobe ships inside the ffmpeg
/// package, so its hint says so.
pub fn subprocess_install_hint(os: &str, tool: &str) -> &'static str {
    match (os, tool) {
        ("macos", "ffmpeg") => "install it (e.g. `brew install ffmpeg`)",
        ("macos", "ffprobe") => "install it (e.g. `brew install ffmpeg` — ffprobe ships inside it)",
        ("macos", "yt-dlp") => "install it (e.g. `brew install yt-dlp`)",
        ("linux", "ffmpeg") => "install it (e.g. `sudo apt install ffmpeg`)",
        ("linux", "ffprobe") => {
            "install it (e.g. `sudo apt install ffmpeg` — ffprobe ships inside it)"
        }
        ("linux", "yt-dlp") => "install it (e.g. `sudo apt install yt-dlp`)",
        ("windows", "ffmpeg") => "install it (e.g. `winget install Gyan.FFmpeg`)",
        ("windows", "ffprobe") => {
            "install it (e.g. `winget install Gyan.FFmpeg` — ffprobe ships inside it)"
        }
        ("windows", "yt-dlp") => "install it (e.g. `winget install yt-dlp.yt-dlp`)",
        (_, "demucs") => "install it (e.g. `pipx install demucs`)",
        _ => "install it and ensure it is on PATH",
    }
}

/// `cmd /C ver` text parsed into the two strings Buzz's `platform.release()`
/// and `platform.version()` send on Windows.
///
/// `version` is the bracketed `major.minor.build` string. `release` is
/// `"11"` when that build is 22000 or newer (Windows 11 still reports
/// version 10.0), otherwise the major number (`"10"`, `"6"`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVer {
    /// `platform.release()` analogue (`"11"`, `"10"`, `"6"`).
    pub release: String,
    /// `platform.version()` analogue (`"10.0.22631.3880"`).
    pub version: String,
}

/// Parses `ver` output. `None` when the `[Version …]` token is absent.
pub fn parse_windows_ver(output: &str) -> Option<WindowsVer> {
    let marker = "[Version ";
    let start = output.find(marker)? + marker.len();
    let rest = output.get(start..)?;
    let end = rest.find(']')?;
    let version = rest[..end].trim();
    if version.is_empty() || !version.chars().next()?.is_ascii_digit() {
        return None;
    }
    Some(WindowsVer {
        release: windows_release_name(version),
        version: version.to_string(),
    })
}

fn windows_release_name(version: &str) -> String {
    let mut parts = version.split('.');
    let major: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let _minor = parts.next();
    let build: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    if major >= 10 && build >= 22_000 {
        "11".to_string()
    } else if major > 0 {
        major.to_string()
    } else {
        version.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn platform_system_name_matches_python_platform_system() {
        assert_eq!(platform_system_name("macos"), "Darwin");
        assert_eq!(platform_system_name("linux"), "Linux");
        assert_eq!(platform_system_name("windows"), "Windows");
        assert_eq!(platform_system_name("freebsd"), "freebsd");
    }

    #[test]
    fn install_hints_are_per_os_and_demucs_is_pip_everywhere() {
        assert_eq!(
            subprocess_install_hint("macos", "ffmpeg"),
            "install it (e.g. `brew install ffmpeg`)"
        );
        assert_eq!(
            subprocess_install_hint("linux", "yt-dlp"),
            "install it (e.g. `sudo apt install yt-dlp`)"
        );
        assert_eq!(
            subprocess_install_hint("windows", "ffmpeg"),
            "install it (e.g. `winget install Gyan.FFmpeg`)"
        );
        assert!(subprocess_install_hint("windows", "ffprobe").contains("ffprobe ships inside it"));
        assert_eq!(
            subprocess_install_hint("linux", "demucs"),
            "install it (e.g. `pipx install demucs`)"
        );
        assert_eq!(
            subprocess_install_hint("windows", "demucs"),
            "install it (e.g. `pipx install demucs`)"
        );
        assert!(!subprocess_install_hint("linux", "ffmpeg").contains("brew"));
        assert!(!subprocess_install_hint("windows", "yt-dlp").contains("brew"));
    }

    #[test]
    fn parse_windows_ver_splits_release_and_version() {
        let win11 = parse_windows_ver("\r\nMicrosoft Windows [Version 10.0.22631.3880]\r\n")
            .expect("win11");
        assert_eq!(win11.release, "11");
        assert_eq!(win11.version, "10.0.22631.3880");

        let win10 = parse_windows_ver("Microsoft Windows [Version 10.0.19041.1]").expect("win10");
        assert_eq!(win10.release, "10");
        assert_eq!(win10.version, "10.0.19041.1");

        let win7 = parse_windows_ver("Microsoft Windows [Version 6.1.7601]").expect("win7");
        assert_eq!(win7.release, "6");
        assert_eq!(win7.version, "6.1.7601");

        assert_eq!(parse_windows_ver("no version here"), None);
        assert_eq!(parse_windows_ver("[Version ]"), None);
    }
}
