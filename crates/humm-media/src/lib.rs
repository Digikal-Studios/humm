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
pub(crate) fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    signal_process_group(child, nix::sys::signal::Signal::SIGKILL);
    let _ = child.kill();
}

/// Process-group ids ([`spawn_own_process_group`]'s children — pgid equals
/// pid, since each is spawned as its own group leader) currently registered
/// for [`kill_all_registered_process_groups`] to reach in an emergency exit
/// that bypasses this crate's own normal cancel-then-terminate machinery
/// entirely (`humm-cli`'s second-Ctrl-C `std::process::exit` arm, `humm-gui`'s
/// `on_exit` last-resort sweep after cooperative shutdown — D5b-2/D5b-4, M9).
/// Lock poisoning is recovered the same way every other `Mutex` in this
/// workspace is (`humm_core::models::download`'s `ARTIFACT_LOCKS` convention)
/// — a panic while holding this lock would be a bug elsewhere, not a reason
/// to also fail the (already-emergency) caller.
static PROCESS_GROUPS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<i32>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Registers `child`'s process group (spawned via [`spawn_own_process_group`],
/// so its pgid is its own pid) in the process-wide emergency-kill registry,
/// returning a Drop-guard ([`GroupRegistration`]) that unregisters it again.
/// Adapters (`demucs.rs`, `fetch.rs`) bind the guard to a local that lives for
/// the child's whole lifetime — from just after spawn until it's fully
/// reaped on every return path, including a panic unwind — so
/// [`kill_all_registered_process_groups`] can always reach a still-running
/// child, and never a stale/already-reaped one.
pub(crate) fn register_process_group(child: &std::process::Child) -> GroupRegistration {
    let pgid = child.id() as i32;
    let mut groups = PROCESS_GROUPS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    groups.insert(pgid);
    GroupRegistration { pgid }
}

/// Drop-guard returned by [`register_process_group`]; removes its pgid from
/// the registry on drop, however the caller's scope ends (normal return,
/// early `?`, or panic unwind).
pub(crate) struct GroupRegistration {
    pgid: i32,
}

impl Drop for GroupRegistration {
    fn drop(&mut self) {
        let mut groups = PROCESS_GROUPS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        groups.remove(&self.pgid);
    }
}

/// Emergency last-resort sweep: SIGKILLs every still-registered process
/// group and drains the registry. For callers that bypass this crate's own
/// cooperative cancel-then-terminate machinery entirely and cannot afford to
/// wait for it:
///
/// - `humm-cli`'s `install_ctrlc` second-signal arm, immediately before its
///   `std::process::exit(130)` — the first Ctrl-C already asked every task
///   to cancel cooperatively; a user impatient enough to press Ctrl-C again
///   gets an immediate exit, but every registered group is still reaped
///   first so nothing is orphaned (reparented to init, invisible to the
///   terminal that started it — `demucs.rs`'s module doc explains why
///   these groups are detached from the terminal's foreground group in the
///   first place, so a second Ctrl-C at the terminal never reaches them
///   directly).
/// - `humm-gui`'s `on_exit`, *after* `shutdown_queue`'s own bounded,
///   graceful drain has had its normal chance — this is a belt-and-braces
///   final sweep for whatever `shutdown_queue`'s timeout left running, never
///   a substitute for it.
///
/// SIGKILL directly (no SIGTERM grace period, unlike `demucs.rs`'s own
/// cancellation ladder): both callers are themselves emergency/last-resort
/// exits with no more time to spend waiting, and group-wide immediate
/// SIGKILL already matches `YtDlpFetcher`'s existing cancel semantics
/// (`kill_process_group`'s doc).
///
/// Each `kill` error (almost always `ESRCH` — the group already exited
/// through the normal cooperative path first) is silently ignored: there is
/// nothing actionable left to do with it, since the caller is already on
/// its way out regardless.
pub fn kill_all_registered_process_groups() {
    let pgids: Vec<i32> = {
        let mut groups = PROCESS_GROUPS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        groups.drain().collect()
    };
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        for pgid in pgids {
            let _ = kill(Pid::from_raw(-pgid), Signal::SIGKILL);
        }
    }
    // Non-unix: `spawn_own_process_group` never actually creates a separate
    // process group to begin with (that function's own doc comment), so
    // there is no group here to SIGKILL beyond what each adapter's own
    // `Child::kill()` already reaches on its normal cancellation path — the
    // registry is still drained above for consistency, but no signal is
    // sent.
    #[cfg(not(unix))]
    let _ = pgids;
}

/// Test-only guard against a genuine cross-test hazard: [`PROCESS_GROUPS`]
/// is one process-wide static shared by every test in this crate's test
/// binary, and `cargo test` runs them concurrently by default. A test that
/// calls [`kill_all_registered_process_groups`] drains and SIGKILLs
/// *every* currently-registered pgid — including one a wholly unrelated,
/// concurrently-running test just registered for its own (still wanted)
/// child. Every test that spawns a [`spawn_own_process_group`] child holds
/// a `read()` guard for the child's whole lifetime (so normal spawning
/// tests still run concurrently with each other, unaffected); every test
/// that calls `kill_all_registered_process_groups` holds a `write()` guard
/// instead, which only proceeds once no other such test has a child
/// in flight — making the "kill everything" tests properly exclusive
/// without serializing the whole suite.
#[cfg(test)]
pub(crate) static PROCESS_GROUP_TEST_LOCK: std::sync::RwLock<()> = std::sync::RwLock::new(());

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

    // -- pgid registry (D5b-2/D5b-4, M9) -------------------------------------

    #[cfg(unix)]
    fn is_alive(pid: i32) -> bool {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        kill(Pid::from_raw(pid), None).is_ok()
    }

    #[cfg(unix)]
    fn wait_until_dead(pid: i32, timeout: std::time::Duration) {
        let start = std::time::Instant::now();
        loop {
            if !is_alive(pid) {
                return;
            }
            assert!(
                start.elapsed() < timeout,
                "pid {pid} still alive after {timeout:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Mirrors `fetch.rs::cancel_kills_grandchild_process_not_just_direct_child`'s
    /// stub technique: a `/bin/sh` script backgrounds a genuine `sleep 30`
    /// child (a real grandchild of the registered process, not just the
    /// shell itself) and writes that grandchild's pid to a file before
    /// blocking on `wait` — proving `kill_all_registered_process_groups`
    /// reaches the *whole* group, not just the direct child whose `Child`
    /// handle was registered.
    #[cfg(unix)]
    #[test]
    fn kill_all_registered_groups_kills_grandchildren() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        // Exclusive: this test's own `kill_all_registered_process_groups`
        // call must not collaterally SIGKILL some other, concurrently
        // running test's still-wanted child (`PROCESS_GROUP_TEST_LOCK`'s
        // doc comment explains the hazard).
        let _serialize = PROCESS_GROUP_TEST_LOCK
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let dir = tempfile::tempdir().expect("tempdir");
        let grandchild_pid_file = dir.path().join("grandchild.pid");
        let script = dir.path().join("stub.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\necho $! > {}\nwait\n",
                grandchild_pid_file.display()
            ),
        )
        .expect("write stub script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub script");

        let mut cmd = Command::new(&script);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn_own_process_group(&mut cmd);
        let mut child = cmd.spawn().expect("spawn stub script");
        let child_pid = child.id() as i32;

        // Wait for the grandchild pid file to appear — proves the
        // background `sleep 30` is actually up before we try to kill it.
        let deadline = Instant::now() + Duration::from_secs(2);
        let grandchild_pid: i32 = loop {
            if let Ok(contents) = std::fs::read_to_string(&grandchild_pid_file)
                && let Ok(pid) = contents.trim().parse()
            {
                break pid;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the grandchild pid file"
            );
            std::thread::sleep(Duration::from_millis(20));
        };

        let _registration = register_process_group(&child);
        kill_all_registered_process_groups();

        // `child_pid` is reaped through `child.try_wait()` (mutable access
        // to our own `Child`), not the raw `kill(pid, 0)` probe
        // `wait_until_dead`/`is_alive` use below: a killed child we
        // ourselves haven't `wait()`-ed on yet is a zombie, and `kill(pid,
        // 0)` reports a zombie as "alive" (it still holds a process-table
        // entry) until *someone* reaps it — which for our own direct child
        // is us. The grandchild has no such wrinkle: its real parent (the
        // now-dead shell) is gone, so once killed it's reparented to init,
        // which reaps it on our behalf — a third party's `kill(pid, 0)`
        // then correctly reports it gone.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(err) => panic!("try_wait failed: {err}"),
            }
            assert!(
                Instant::now() < deadline,
                "direct child (pid {child_pid}) still alive after 2s"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        wait_until_dead(grandchild_pid, Duration::from_secs(2));
    }

    /// A dropped [`GroupRegistration`] unregisters its pgid — proving
    /// `kill_all_registered_process_groups` never reaches a child whose
    /// normal reap (or early return) already dropped the guard, i.e. it
    /// only ever sweeps groups that are still actually outstanding.
    #[cfg(unix)]
    #[test]
    fn dropped_registration_unregisters() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};

        // Exclusive — same reason as `kill_all_registered_groups_kills_grandchildren`.
        let _serialize = PROCESS_GROUP_TEST_LOCK
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("stub.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").expect("write stub script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub script");

        let mut cmd = Command::new(&script);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn_own_process_group(&mut cmd);
        let mut child = cmd.spawn().expect("spawn stub script");
        let pid = child.id() as i32;

        let registration = register_process_group(&child);
        drop(registration);

        kill_all_registered_process_groups();

        assert!(
            is_alive(pid),
            "child should still be alive: kill_all must not reach a group whose \
             registration was already dropped"
        );

        // Clean up directly — this is no longer the (dropped) guard's job.
        kill_process_group(&mut child);
        let _ = child.wait();
    }
}
