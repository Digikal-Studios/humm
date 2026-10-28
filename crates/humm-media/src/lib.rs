//! humm-media: ffmpeg/ffprobe/yt-dlp subprocess adapter implementing the
//! `AudioDecoder`, `AudioEncoder`, `MediaProber`, and `MediaFetcher` traits
//! from `humm-core`.
#![forbid(unsafe_code)]

mod capabilities;
mod decode;
mod demucs;
mod encode;
mod fetch;
mod probe;
mod video;

pub use capabilities::{MediaCapabilities, probe_binaries};
pub use decode::FfmpegDecoder;
pub use demucs::DemucsIsolator;
pub use fetch::YtDlpFetcher;
pub use probe::FfprobeProber;
pub use video::FfmpegVideoStreamer;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use humm_core::error::MediaError;

/// Win32 `CREATE_NO_WINDOW`. A GUI parent with no console otherwise gets a
/// console window for every ffmpeg, ffprobe, yt-dlp, or demucs child.
#[cfg(any(windows, test))]
pub(crate) const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Resolves a subprocess binary path: the `var` env override if set, else
/// `default` (looked up on `PATH` at spawn time).
///
/// Per Global Constraint 9, the env read is encapsulated here and delegates
/// to a pure inner function (`binary_from_env_value`) so tests exercise the
/// override/default logic without mutating process environment
/// (`std::env::set_var` is `unsafe` in edition 2024) — mirrors
/// `humm_core::paths`.
pub(crate) fn binary_from_env(var: &str, default: &str) -> PathBuf {
    binary_from_env_value(std::env::var_os(var).as_deref(), default)
}

fn binary_from_env_value(value: Option<&OsStr>, default: &str) -> PathBuf {
    value
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

/// Maps a subprocess spawn/exec error to a `MediaError`, turning "binary not
/// found" into an actionable [`MediaError::BinaryMissing`] and anything else
/// into a plain I/O error. `install_hint` is the tool-specific remediation
/// clause (e.g. `` "install it (e.g. `brew install ffmpeg`)" ``) — callers
/// name their own install method rather than this shared helper assuming
/// one, since it isn't universal (Homebrew doesn't package `demucs`; that's
/// a `pip`/`pipx` install).
pub(crate) fn map_spawn_error(
    program: &str,
    env_var: &str,
    install_hint: &str,
    binary: &Path,
    err: std::io::Error,
) -> MediaError {
    if err.kind() == std::io::ErrorKind::NotFound {
        MediaError::BinaryMissing {
            binary: binary.display().to_string(),
            hint: format!("{program} not found — {install_hint} or set {env_var}"),
        }
    } else {
        MediaError::Io(err)
    }
}

/// Install hint for `tool` on this host (`ffmpeg`, `ffprobe`, `yt-dlp`,
/// `demucs`).
pub(crate) fn install_hint(tool: &str) -> &'static str {
    humm_core::platform::subprocess_install_hint(std::env::consts::OS, tool)
}

/// `PATHEXT` entries, each starting with `.`. The default matches a stock
/// Windows `PATHEXT` when the variable is unset. Unix passes an empty list
/// and lookup stays exact-name.
pub(crate) fn pathext_from(value: Option<&str>) -> Vec<String> {
    let Some(raw) = value else {
        return Vec::new();
    };
    raw.split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            if entry.starts_with('.') {
                entry.to_string()
            } else {
                format!(".{entry}")
            }
        })
        .collect()
}

fn executable_extensions() -> Vec<String> {
    #[cfg(windows)]
    {
        let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        pathext_from(Some(&raw))
    }
    #[cfg(not(windows))]
    {
        pathext_from(None)
    }
}

/// Names to try for `binary`. When `binary`'s file name already has an
/// extension, only that name is returned (Windows does not append `PATHEXT`
/// in that case). Otherwise each extension is appended, after the bare name.
pub(crate) fn candidate_executable_names(binary: &Path, extensions: &[String]) -> Vec<PathBuf> {
    let mut names = vec![binary.to_path_buf()];
    let Some(file_name) = binary.file_name() else {
        return names;
    };
    if Path::new(file_name).extension().is_some() {
        return names;
    }
    for ext in extensions {
        let mut owned = binary.as_os_str().to_os_string();
        owned.push(ext);
        names.push(PathBuf::from(owned));
    }
    names
}

/// `taskkill /F /T /PID <pid>`. `None` for pid 0, which is not a process.
#[cfg(any(windows, test))]
pub(crate) fn windows_tree_kill_args(pid: u32) -> Option<Vec<String>> {
    if pid == 0 {
        None
    } else {
        Some(vec![
            "/F".to_string(),
            "/T".to_string(),
            "/PID".to_string(),
            pid.to_string(),
        ])
    }
}

fn find_on_path(binary: &Path, extensions: &[String], path: Option<&OsStr>) -> Option<PathBuf> {
    let candidates = candidate_executable_names(binary, extensions);
    if binary.components().count() > 1 {
        return candidates
            .into_iter()
            .find(|candidate| is_executable_file(candidate));
    }
    let path = path?;
    for dir in std::env::split_paths(path) {
        for name in &candidates {
            let full = dir.join(name);
            if is_executable_file(&full) {
                return Some(full);
            }
        }
    }
    None
}

pub(crate) fn resolve_executable(binary: &Path) -> PathBuf {
    find_on_path(
        binary,
        &executable_extensions(),
        std::env::var_os("PATH").as_deref(),
    )
    .unwrap_or_else(|| binary.to_path_buf())
}

/// True when `binary` is an executable file, or (on Windows) a `PATHEXT`
/// sibling such as `ffmpeg.exe` for the bare name `ffmpeg`.
pub(crate) fn executable_exists(binary: &Path) -> bool {
    find_on_path(
        binary,
        &executable_extensions(),
        std::env::var_os("PATH").as_deref(),
    )
    .is_some()
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Builds a [`std::process::Command`] for `binary`, resolving Windows
/// `PATHEXT` names and applying [`configure_subprocess`].
pub(crate) fn prepare_command(binary: &Path) -> (std::process::Command, PathBuf) {
    let resolved = resolve_executable(binary);
    let mut cmd = std::process::Command::new(&resolved);
    configure_subprocess(&mut cmd);
    (cmd, resolved)
}

/// Puts `cmd`'s child in a new process group on Unix so a later
/// [`kill_process_group`] reaches descendants. No-op on other targets
/// (Windows tree kill is by pid, via `taskkill /T`).
pub(crate) fn spawn_own_process_group(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = cmd;
    }
}

/// Unix process group plus, on Windows, `CREATE_NO_WINDOW` so a console-less
/// GUI does not flash a console for every ffmpeg, ffprobe, yt-dlp, or demucs
/// child.
pub(crate) fn configure_subprocess(cmd: &mut std::process::Command) {
    spawn_own_process_group(cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(WINDOWS_CREATE_NO_WINDOW);
    }
}

/// Sends `signal` to `child`'s whole process group (unix only; `child` must
/// have been spawned via [`spawn_own_process_group`]) — see
/// [`kill_process_group`]'s doc for why the group, not just the direct
/// child, is the right target.
#[cfg(unix)]
pub(crate) fn signal_process_group(child: &std::process::Child, signal: nix::sys::signal::Signal) {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let _ = kill(Pid::from_raw(-(child.id() as i32)), signal);
}

/// SIGKILLs `child`'s whole process group (unix; `child` must have been
/// spawned via [`spawn_own_process_group`]), then also directly
/// `Child::kill()`s it as a fallback (a harmless no-op error if it's
/// already reaped) — on every other target, just the direct `Child::kill()`
/// (already the platform's immediate kill call, with no process-group
/// distinction to make there).
///
/// Group-signaling matters because a single-PID kill can leave a
/// descendant the child spawned of its own running: empirically confirmed
/// for a shell-script-under-`demucs` case (`demucs.rs`'s module doc) — a
/// killable `yt-dlp` invoking `ffmpeg` as an external downloader/
/// post-processor for HLS or fixup steps is the same shape — and that
/// descendant then keeps the killed child's stdout/stderr pipes open,
/// blocking the caller's reader-thread `join`s until it exits naturally on
/// its own.
/// SIGKILL each process-group leader in `pids` and do not wait.
///
/// `process_group(0)` makes the child's pid the group id, so the signal
/// goes to `-pid`. Leaders that have already been waited and unregistered
/// are not in `pids`. Windows runs `taskkill /F /T /PID` for each pid and
/// does not wait. Any other target is a no-op.
pub fn kill_tracked_groups(pids: &[u32]) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        for &pid in pids {
            if pid == 0 {
                continue;
            }
            let _ = kill(Pid::from_raw(-(pid as i32)), Signal::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        for &pid in pids {
            // Do not wait: this runs while the abort registry lock is held,
            // and the caller is about to exit.
            windows_kill_tree(pid, false);
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pids;
    }
}

pub(crate) fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    signal_process_group(child, nix::sys::signal::Signal::SIGKILL);
    #[cfg(windows)]
    windows_kill_tree(child.id(), true);
    let _ = child.kill();
}

/// `taskkill /F /T /PID`. `wait` blocks until taskkill exits. The
/// no-wait path is for [`kill_tracked_groups`].
#[cfg(windows)]
fn windows_kill_tree(pid: u32, wait: bool) {
    let Some(args) = windows_tree_kill_args(pid) else {
        return;
    };
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(&args);
    configure_subprocess(&mut cmd);
    if wait {
        let _ = cmd.output();
    } else {
        use std::process::Stdio;
        let _ = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn binary_from_env_value_uses_override_when_present() {
        let resolved = binary_from_env_value(Some(OsStr::new("/custom/ffmpeg")), "ffmpeg");
        assert_eq!(resolved, PathBuf::from("/custom/ffmpeg"));
    }

    #[test]
    fn binary_from_env_value_falls_back_to_default_when_absent() {
        let resolved = binary_from_env_value(None, "ffmpeg");
        assert_eq!(resolved, PathBuf::from("ffmpeg"));
    }

    #[test]
    fn map_spawn_error_not_found_becomes_binary_missing_with_hint() {
        let err = map_spawn_error(
            "ffmpeg",
            "HUMM_FFMPEG",
            "install it (e.g. `brew install ffmpeg`)",
            Path::new("/nonexistent/ffmpeg"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        match err {
            MediaError::BinaryMissing { binary, hint } => {
                assert_eq!(binary, "/nonexistent/ffmpeg");
                assert!(hint.contains("HUMM_FFMPEG"), "hint was: {hint}");
                assert!(hint.contains("ffmpeg not found"), "hint was: {hint}");
                assert!(hint.contains("brew install ffmpeg"), "hint was: {hint}");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
    }

    #[test]
    fn map_spawn_error_uses_the_caller_supplied_install_hint_verbatim() {
        // Demucs is the reason this is a parameter at all — Homebrew
        // doesn't package it (Minor 6, task-5 review).
        let err = map_spawn_error(
            "demucs",
            "HUMM_DEMUCS",
            "install it (e.g. `pipx install demucs`)",
            Path::new("/nonexistent/demucs"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        match err {
            MediaError::BinaryMissing { hint, .. } => {
                assert!(hint.contains("pipx install demucs"), "hint was: {hint}");
                assert!(!hint.contains("brew"), "hint was: {hint}");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
    }

    #[test]
    fn windows_tree_kill_is_taskkill_force_tree() {
        assert_eq!(
            windows_tree_kill_args(4242),
            Some(vec![
                "/F".to_string(),
                "/T".to_string(),
                "/PID".to_string(),
                "4242".to_string()
            ])
        );
        assert_eq!(windows_tree_kill_args(0), None);
    }

    #[test]
    fn windows_create_no_window_is_the_win32_flag() {
        assert_eq!(WINDOWS_CREATE_NO_WINDOW, 0x0800_0000);
    }

    #[test]
    fn pathext_candidates_cover_a_bare_name_and_leave_an_extension_alone() {
        let exts = pathext_from(Some(".COM;.EXE;.BAT;.CMD"));
        let names = candidate_executable_names(Path::new("yt-dlp"), &exts);
        assert_eq!(names[0], PathBuf::from("yt-dlp"));
        assert!(names.iter().any(|path| path == Path::new("yt-dlp.EXE")));
        assert_eq!(
            candidate_executable_names(Path::new("C:/bin/ffmpeg.exe"), &exts),
            vec![PathBuf::from("C:/bin/ffmpeg.exe")]
        );
        assert_eq!(
            candidate_executable_names(Path::new("C:/bin/ffmpeg"), &exts),
            vec![
                PathBuf::from("C:/bin/ffmpeg"),
                PathBuf::from("C:/bin/ffmpeg.COM"),
                PathBuf::from("C:/bin/ffmpeg.EXE"),
                PathBuf::from("C:/bin/ffmpeg.BAT"),
                PathBuf::from("C:/bin/ffmpeg.CMD"),
            ]
        );
        assert!(pathext_from(None).is_empty());
    }

    #[test]
    fn path_lookup_finds_an_exe_sibling_of_a_bare_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("ffmpeg.exe");
        std::fs::write(&exe, b"not a real binary").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exe).expect("meta").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe, perms).expect("chmod");
        }
        let exts = vec![".exe".to_string()];
        assert_eq!(
            find_on_path(Path::new("ffmpeg"), &exts, Some(dir.path().as_os_str())),
            Some(exe)
        );
        // Unix lookup passes no extensions, so `ffmpeg.exe` is not `ffmpeg`.
        assert_eq!(
            find_on_path(Path::new("ffmpeg"), &[], Some(dir.path().as_os_str())),
            None
        );
    }

    #[test]
    fn explicit_path_without_exe_finds_the_exe_sibling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("ffmpeg.exe");
        std::fs::write(&exe, b"not a real binary").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exe).expect("meta").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe, perms).expect("chmod");
        }
        let bare = dir.path().join("ffmpeg");
        assert_eq!(find_on_path(&bare, &[".exe".to_string()], None), Some(exe));
    }

    #[test]
    fn map_spawn_error_other_kind_stays_io() {
        let err = map_spawn_error(
            "ffmpeg",
            "HUMM_FFMPEG",
            "install it (e.g. `brew install ffmpeg`)",
            Path::new("ffmpeg"),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(matches!(err, MediaError::Io(_)));
    }
}
