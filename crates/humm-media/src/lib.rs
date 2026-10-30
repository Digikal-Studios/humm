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
#[cfg(any(windows, test))]
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Once;

use humm_core::error::MediaError;

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

/// Directories Rust's Windows `Command` searches for a bare program name,
/// in order: the application directory, `System32`, the Windows directory,
/// then `PATH`.
#[cfg(any(windows, test))]
pub(crate) struct WindowsSearch<'a> {
    /// Directory containing the running executable (`std::env::current_exe`).
    pub app_dir: Option<&'a Path>,
    /// `GetSystemDirectoryW`, approximated as `%SystemRoot%\System32` at the
    /// call site.
    pub system_dir: Option<&'a Path>,
    /// `GetWindowsDirectoryW` / `%SystemRoot%`.
    pub windows_dir: Option<&'a Path>,
    /// The process `PATH`. Empty entries are skipped.
    pub path: Option<&'a OsStr>,
}

/// File name `Command` looks for. A name with no `.` gets `.exe` appended.
/// `.bat` and `.cmd` are never invented: Rust's resolver does not walk
/// `PATHEXT`, and a batch file would be reparsed by `cmd.exe`.
#[cfg(any(windows, test))]
pub(crate) fn windows_candidate_name(name: &OsStr) -> OsString {
    if name.as_encoded_bytes().contains(&b'.') {
        name.to_os_string()
    } else {
        let mut owned = name.to_os_string();
        owned.push(".exe");
        owned
    }
}

/// Finds `name` the way Rust 1.85 `std::sys::pal::windows::process::search_paths`
/// does for a bare file name. Empty `PATH` entries are not the current
/// directory. The first existing candidate wins.
#[cfg(any(windows, test))]
pub(crate) fn windows_resolve_bare(
    name: &OsStr,
    search: &WindowsSearch<'_>,
    mut exists: impl FnMut(&Path) -> bool,
) -> Option<PathBuf> {
    let candidate = windows_candidate_name(name);
    let mut found = None;
    let mut consider = |dir: &Path| {
        if found.is_some() {
            return;
        }
        let full = dir.join(&candidate);
        if exists(&full) {
            found = Some(full);
        }
    };
    if let Some(dir) = search.app_dir {
        consider(dir);
    }
    if let Some(dir) = search.system_dir {
        consider(dir);
    }
    if let Some(dir) = search.windows_dir {
        consider(dir);
    }
    if let Some(path) = search.path {
        for dir in std::env::split_paths(path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            consider(&dir);
        }
    }
    found
}

/// Explicit Windows path that does not already name an extension: the `.exe`
/// sibling when `exe_sibling_is_file`, otherwise `path` unchanged. A path
/// whose file name contains `.` (including `.exe`, `.bat`, `.cmd`) is left
/// alone so this helper never prefers a batch file over `ffmpeg.exe`.
#[cfg(any(windows, test))]
pub(crate) fn explicit_windows_program(path: &Path, exe_sibling_is_file: bool) -> PathBuf {
    if file_name_contains_dot(path) || !exe_sibling_is_file {
        path.to_path_buf()
    } else {
        append_literal_exe(path)
    }
}

#[cfg(any(windows, test))]
fn file_name_contains_dot(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_encoded_bytes().contains(&b'.'))
}

#[cfg(any(windows, test))]
fn append_literal_exe(path: &Path) -> PathBuf {
    let mut owned = path.as_os_str().to_os_string();
    owned.push(".exe");
    PathBuf::from(owned)
}

/// A program name with no directory component. `Command` resolves these
/// itself.
fn is_bare_command(binary: &Path) -> bool {
    let mut components = binary.components();
    match components.next() {
        Some(Component::Normal(_)) => components.next().is_none(),
        _ => false,
    }
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

/// Resolves `binary` the way [`Command`] will run it.
///
/// A bare name is returned unchanged so `Command` searches: on Windows that
/// is the application directory, `System32`, the Windows directory, then
/// `PATH`, appending `.exe` only and skipping empty `PATH` entries. An
/// explicit path with no extension prefers an existing `.exe` sibling, then
/// the path itself. `.bat` and `.cmd` are not candidates this function
/// invents.
pub(crate) fn resolve_executable(binary: &Path) -> PathBuf {
    if is_bare_command(binary) {
        return binary.to_path_buf();
    }
    #[cfg(windows)]
    {
        explicit_windows_program(binary, append_literal_exe(binary).is_file())
    }
    #[cfg(not(windows))]
    {
        binary.to_path_buf()
    }
}

/// Whether `binary` can be executed without spawning it. Bare names follow
/// the same rules as [`resolve_executable`] / `Command`. Explicit paths are
/// checked on disk (Unix: executable bit; Windows: the `.exe` sibling, then
/// the path itself).
pub(crate) fn executable_exists(binary: &Path) -> bool {
    if is_bare_command(binary) {
        bare_command_exists(binary)
    } else {
        explicit_program_exists(binary)
    }
}

fn bare_command_exists(name: &Path) -> bool {
    #[cfg(windows)]
    {
        let app = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        let windows = std::env::var_os("SystemRoot").map(PathBuf::from);
        let system = windows.as_ref().map(|root| root.join("System32"));
        let path = std::env::var_os("PATH");
        windows_resolve_bare(
            name.as_os_str(),
            &WindowsSearch {
                app_dir: app.as_deref(),
                system_dir: system.as_deref(),
                windows_dir: windows.as_deref(),
                path: path.as_deref(),
            },
            is_executable_file,
        )
        .is_some()
    }
    #[cfg(not(windows))]
    {
        let Some(path) = std::env::var_os("PATH") else {
            return false;
        };
        unix_find_bare(name, &path).is_some()
    }
}

/// Exact-name `PATH` search. Empty components stay relative (the current
/// directory), matching Unix `Command`.
#[cfg(any(not(windows), test))]
fn unix_find_bare(name: &Path, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|dir| {
        let full = dir.join(name);
        if is_executable_file(&full) {
            Some(full)
        } else {
            None
        }
    })
}

fn explicit_program_exists(binary: &Path) -> bool {
    #[cfg(windows)]
    {
        if !file_name_contains_dot(binary) && is_executable_file(&append_literal_exe(binary)) {
            return true;
        }
        is_executable_file(binary)
    }
    #[cfg(not(windows))]
    {
        is_executable_file(binary)
    }
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

/// Builds a [`Command`] for `binary` and applies [`configure_subprocess`].
///
/// The child is detached (new process group on Unix, `CREATE_NO_WINDOW` on
/// Windows). Spawn it with [`spawn_prepared`] or [`output_prepared`] so the
/// abort registry can kill it on a second Ctrl-C, `SIGHUP`, or console close.
pub(crate) fn prepare_command(binary: &Path) -> (Command, PathBuf) {
    let resolved = resolve_executable(binary);
    let mut cmd = Command::new(&resolved);
    configure_subprocess(&mut cmd);
    (cmd, resolved)
}

/// A child started by [`spawn_prepared`]. Dropping the registration removes
/// the pid from the abort registry; do that only after `wait`.
pub(crate) struct Spawned {
    /// The live child.
    pub child: Child,
    track: humm_core::abort::TrackedProcess,
}

impl Spawned {
    /// Splits the child from its registry guard. Keep the guard until `wait`.
    pub(crate) fn into_parts(self) -> (Child, humm_core::abort::TrackedProcess) {
        (self.child, self.track)
    }

    /// Registry id for [`humm_core::abort::try_emergency_stop_registered`].
    ///
    /// Only the Unix emergency-stop tests call this. A Windows test build
    /// does not, and `dead_code` is denied there.
    #[cfg(all(test, unix))]
    pub(crate) fn registration_id(&self) -> u64 {
        self.track.registration_id()
    }
}

/// Spawns `cmd`, puts the child in the kill-on-close job on Windows, and
/// registers its pid. `cmd` should already have been through
/// [`configure_subprocess`] ([`prepare_command`] does that).
pub(crate) fn spawn_prepared(cmd: &mut Command) -> std::io::Result<Spawned> {
    let child = cmd.spawn()?;
    assign_kill_on_close(&child);
    let track = humm_core::abort::track_process(child.id());
    Ok(Spawned { child, track })
}

/// [`Command::output`], but the child is tracked for the whole wait.
/// Stdout and stderr are piped. Stdin is whatever `cmd` already has.
pub(crate) fn output_prepared(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    let child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    assign_kill_on_close(&child);
    let _track = humm_core::abort::track_process(child.id());
    child.wait_with_output()
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
        cmd.creation_flags(humm_core::platform::WINDOWS_CREATE_NO_WINDOW);
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

/// SIGKILLs each process-group leader in `pids` and does not wait.
///
/// `process_group(0)` makes the child's pid the group id, so the signal
/// goes to `-pid`. On Windows each pid is `taskkill /F /T /PID` (no wait:
/// this runs while the abort registry lock is held). Leaders that have
/// already been waited and unregistered are not in `pids`.
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

/// Kills `child` and descendants.
///
/// On Unix that is `SIGKILL` to the process group (`process_group(0)` makes
/// the pid the group id), then `Child::kill` as a fallback. On Windows it
/// is `taskkill /F /T` followed by `Child::kill`. Group kill matters because
/// a single-pid kill leaves a grandchild (demucs under a shell, yt-dlp's
/// ffmpeg) holding the pipes open.
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
    let mut cmd = Command::new("taskkill");
    cmd.args(&args);
    configure_subprocess(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Not tracked: this runs while the abort registry lock is held.
    let Ok(mut child) = cmd.spawn() else {
        return;
    };
    assign_kill_on_close(&child);
    if wait {
        let _ = child.wait();
    }
}

/// Arranges for detached media children to die with this process.
///
/// On Unix, `SIGHUP` (the terminal closing) runs the same emergency stop as
/// a second Ctrl-C: every pid registered via [`spawn_prepared`] /
/// [`output_prepared`] is group-killed, then this process exits `129`.
/// Those children are in their own process group, so the hangup would
/// otherwise not reach them.
///
/// On Windows, children are placed in a job with `KILL_ON_JOB_CLOSE`.
/// Closing the console, `process::exit`, or a crash closes that handle and
/// the system kills the tree. `CREATE_NO_WINDOW` keeps them off the
/// console, so `CTRL_CLOSE_EVENT` would not reach them on its own.
///
/// Safe to call more than once. A failure to install is logged and does not
/// stop startup.
pub fn install_terminal_cleanup() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        #[cfg(unix)]
        if let Err(err) = install_sighup_handler() {
            log::warn!("could not install SIGHUP cleanup ({err})");
        }
        #[cfg(windows)]
        {
            // Arm the job before the first child. Spawn also creates it
            // lazily if this was not called.
            let _ = kill_on_close_job();
        }
    });
}

#[cfg(unix)]
fn install_sighup_handler() -> std::io::Result<()> {
    let mut signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGHUP])?;
    std::thread::Builder::new()
        .name("humm-sighup".to_string())
        .spawn(move || {
            // One hangup, then exit. `process::exit` does not return, so
            // this is not a loop.
            if signals.forever().next().is_some() {
                let _ = humm_core::abort::try_emergency_stop(kill_tracked_groups);
                // 128 + SIGHUP(1). Default disposition is to terminate; do
                // that after the children have been signaled. `exit` skips
                // `Drop`, which is why the registry kill ran first.
                std::process::exit(129);
            }
        })?;
    Ok(())
}

fn assign_kill_on_close(child: &Child) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let Some(job) = kill_on_close_job() else {
            return;
        };
        let handle = child.as_raw_handle() as isize;
        if let Err(err) = job.assign_process(handle) {
            log::warn!(
                "could not assign pid {} to the kill-on-close job ({err:?})",
                child.id()
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = child;
    }
}

#[cfg(windows)]
fn kill_on_close_job() -> Option<&'static win32job::Job> {
    use std::sync::OnceLock;
    static JOB: OnceLock<Option<win32job::Job>> = OnceLock::new();
    JOB.get_or_init(|| {
        let mut info = win32job::ExtendedLimitInfo::new();
        info.limit_kill_on_job_close();
        match win32job::Job::create_with_limit_info(&info) {
            Ok(job) => Some(job),
            Err(err) => {
                log::warn!(
                    "could not create a kill-on-close job ({err:?}); detached children rely on the abort registry"
                );
                None
            }
        }
    })
    .as_ref()
}

/// Retries [`humm_core::abort::try_emergency_stop_registered`] while another
/// test holds the process-global registry lock. Unix tests only: the callers
/// spawn a process group, which Windows Clippy does not build.
#[cfg(all(test, unix))]
pub(crate) fn emergency_stop_registered_for_test(groups: &[u64], temps: &[u64]) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(5) {
        if humm_core::abort::try_emergency_stop_registered(groups, temps, kill_tracked_groups) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    false
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

    /// Argument vector only. A green run has not spawned `taskkill` or killed
    /// a process tree.
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
    fn bare_name_is_left_for_command_to_resolve() {
        assert_eq!(
            resolve_executable(Path::new("ffmpeg")),
            PathBuf::from("ffmpeg")
        );
        assert_eq!(
            resolve_executable(Path::new("yt-dlp")),
            PathBuf::from("yt-dlp")
        );
        assert_eq!(
            resolve_executable(Path::new("ffmpeg.bat")),
            PathBuf::from("ffmpeg.bat")
        );
    }

    #[test]
    fn windows_candidate_appends_exe_only_when_the_name_has_no_dot() {
        assert_eq!(
            windows_candidate_name(OsStr::new("ffmpeg")),
            OsString::from("ffmpeg.exe")
        );
        assert_eq!(
            windows_candidate_name(OsStr::new("yt-dlp")),
            OsString::from("yt-dlp.exe")
        );
        assert_eq!(
            windows_candidate_name(OsStr::new("ffmpeg.exe")),
            OsString::from("ffmpeg.exe")
        );
        assert_eq!(
            windows_candidate_name(OsStr::new("ffmpeg.bat")),
            OsString::from("ffmpeg.bat")
        );
        assert_eq!(
            windows_candidate_name(OsStr::new("ffmpeg.cmd")),
            OsString::from("ffmpeg.cmd")
        );
    }

    #[test]
    fn windows_search_prefers_later_exe_over_earlier_bat_and_skips_empty_path() {
        let earlier = tempfile::tempdir().expect("tempdir");
        let later = tempfile::tempdir().expect("tempdir");
        std::fs::write(earlier.path().join("ffmpeg.bat"), b"bat").expect("bat");
        std::fs::write(earlier.path().join("ffmpeg.cmd"), b"cmd").expect("cmd");
        let exe = later.path().join("ffmpeg.exe");
        std::fs::write(&exe, b"exe").expect("exe");
        // Leading and middle empty entries are the current directory for a
        // naive join. Command skips them.
        let path =
            std::env::join_paths([Path::new(""), earlier.path(), Path::new(""), later.path()])
                .expect("join PATH");
        let mut seen = Vec::new();
        let found = windows_resolve_bare(
            OsStr::new("ffmpeg"),
            &WindowsSearch {
                app_dir: None,
                system_dir: None,
                windows_dir: None,
                path: Some(&path),
            },
            |candidate| {
                seen.push(candidate.to_path_buf());
                candidate.is_file()
            },
        );
        assert_eq!(found, Some(exe));
        assert!(
            seen.iter().all(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                    && path
                        .parent()
                        .is_some_and(|parent| !parent.as_os_str().is_empty())
            }),
            "searched {seen:?}"
        );
    }

    #[test]
    fn windows_search_checks_the_application_directory_before_path() {
        let app = tempfile::tempdir().expect("app");
        let path_dir = tempfile::tempdir().expect("path");
        let app_exe = app.path().join("ffmpeg.exe");
        std::fs::write(&app_exe, b"app").expect("app exe");
        std::fs::write(path_dir.path().join("ffmpeg.exe"), b"path").expect("path exe");
        let path = std::env::join_paths([path_dir.path()]).expect("join PATH");
        let found = windows_resolve_bare(
            OsStr::new("ffmpeg"),
            &WindowsSearch {
                app_dir: Some(app.path()),
                system_dir: None,
                windows_dir: None,
                path: Some(&path),
            },
            |candidate| candidate.is_file(),
        );
        assert_eq!(found, Some(app_exe));
    }

    #[test]
    fn explicit_windows_path_uses_the_exe_sibling_before_the_extensionless_name() {
        assert_eq!(
            explicit_windows_program(Path::new("C:/Tools/ffmpeg"), true),
            PathBuf::from("C:/Tools/ffmpeg.exe")
        );
        assert_eq!(
            explicit_windows_program(Path::new("C:/Tools/ffmpeg"), false),
            PathBuf::from("C:/Tools/ffmpeg")
        );
        assert_eq!(
            explicit_windows_program(Path::new("C:/Tools/ffmpeg.exe"), true),
            PathBuf::from("C:/Tools/ffmpeg.exe")
        );
        // Already has an extension: not rewritten into a batch name, and not
        // given another suffix by this helper.
        assert_eq!(
            explicit_windows_program(Path::new("C:/Tools/ffmpeg.bat"), false),
            PathBuf::from("C:/Tools/ffmpeg.bat")
        );
    }

    #[test]
    fn exact_name_lookup_does_not_invent_an_exe_or_bat_sibling() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("ffmpeg.exe"), b"exe").expect("exe");
        std::fs::write(dir.path().join("ffmpeg.bat"), b"bat").expect("bat");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["ffmpeg.exe", "ffmpeg.bat"] {
                let path = dir.path().join(name);
                let mut perms = std::fs::metadata(&path).expect("meta").permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&path, perms).expect("chmod");
            }
        }
        let path = std::env::join_paths([dir.path()]).expect("join PATH");
        assert_eq!(unix_find_bare(Path::new("ffmpeg"), &path), None);
    }

    #[cfg(unix)]
    #[test]
    fn emergency_stop_kills_a_detached_child() {
        use std::thread;
        use std::time::{Duration, Instant};

        let (mut cmd, _) = prepare_command(Path::new("sleep"));
        cmd.arg("30");
        let mut spawned = spawn_prepared(&mut cmd).expect("spawn sleep");
        let id = spawned.registration_id();
        let start = Instant::now();
        assert!(
            emergency_stop_registered_for_test(&[id], &[]),
            "registry lock stayed busy"
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match spawned.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                Ok(None) => panic!("detached sleep still running after emergency stop"),
                Err(err) => panic!("try_wait: {err}"),
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "kill waited {:?}",
            start.elapsed()
        );
    }

    /// The child process installs the SIGHUP handler, spawns a detached
    /// `sleep`, and waits. The parent sends SIGHUP and checks that sleep died
    /// with the handler, not only the harness.
    #[cfg(unix)]
    #[test]
    fn sighup_kills_a_detached_child() {
        use std::process::Stdio;
        use std::thread;
        use std::time::{Duration, Instant};

        if std::env::var_os("HUMM_SIGHUP_PROBE").is_some() {
            install_terminal_cleanup();
            let (mut cmd, _) = prepare_command(Path::new("sleep"));
            cmd.arg("60");
            let mut spawned = spawn_prepared(&mut cmd).expect("spawn sleep");
            let pid_path = std::env::var("HUMM_SLEEP_PID").expect("pid path");
            std::fs::write(&pid_path, spawned.child.id().to_string()).expect("write pid");
            // Hold the registry guard until the handler exits the process.
            thread::sleep(Duration::from_secs(30));
            let _ = spawned.child.wait();
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let pid_path = dir.path().join("sleep.pid");
        let exe = std::env::current_exe().expect("test exe");
        let mut child = Command::new(exe);
        child
            .env("HUMM_SIGHUP_PROBE", "1")
            .env("HUMM_SLEEP_PID", &pid_path)
            .args([
                "--exact",
                "--test-threads=1",
                "tests::sighup_kills_a_detached_child",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = child.spawn().expect("spawn probe");

        let deadline = Instant::now() + Duration::from_secs(10);
        let sleep_pid = loop {
            if let Ok(text) = std::fs::read_to_string(&pid_path)
                && let Ok(pid) = text.trim().parse::<u32>()
            {
                break pid;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let output = child.wait_with_output();
                let detail = output.map(|out| {
                    format!(
                        "status {:?} stdout {} stderr {}",
                        out.status,
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr)
                    )
                });
                panic!("probe did not record the sleep pid: {detail:?}");
            }
            thread::sleep(Duration::from_millis(20));
        };

        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(child.id() as i32),
            nix::sys::signal::Signal::SIGHUP,
        )
        .expect("SIGHUP");

        let exit_deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < exit_deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = Command::new("kill")
                        .args(["-9", &sleep_pid.to_string()])
                        .status();
                    panic!("SIGHUP handler did not exit the probe");
                }
                Err(err) => panic!("try_wait probe: {err}"),
            }
        };
        let kill_status = Command::new("kill")
            .args(["-0", &sleep_pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .expect("kill -0");
        if kill_status.success() {
            let _ = Command::new("kill")
                .args(["-9", &sleep_pid.to_string()])
                .status();
            panic!("detached sleep {sleep_pid} survived SIGHUP (probe status {status})");
        }
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
