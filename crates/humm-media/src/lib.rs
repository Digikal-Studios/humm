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

/// Configures `cmd` to spawn its child into a new process group (unix only,
/// a no-op elsewhere) so a later [`kill_process_group`]/[`signal_process_group`]
/// call can reach descendants a single-PID signal would miss.
pub(crate) fn spawn_own_process_group(_cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        _cmd.process_group(0);
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
/// are not in `pids`. Non-unix has no process groups; this is a no-op there.
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
    #[cfg(not(unix))]
    {
        let _ = pids;
    }
}

pub(crate) fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    signal_process_group(child, nix::sys::signal::Signal::SIGKILL);
    let _ = child.kill();
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
