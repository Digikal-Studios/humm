//! `demucs`-backed [`SpeechIsolator`] (F-027, ADR-023).
//!
//! Subprocess contract (ADR-023, confirmed by the M8 validation spike,
//! `validation/m8-spikes/README.md` "Spike C"):
//!
//! ```text
//! demucs --two-stems=vocals -n htdemucs --mp3 -o <scratch> --filename "{stem}.{ext}" <input>
//! ```
//!
//! produces `<scratch>/htdemucs/vocals.mp3` (+ `no_vocals.mp3`, ignored) —
//! the `--filename` override (literal demucs template placeholders, not
//! Rust-formatted) omits `{track}`, so demucs never nests a per-input
//! subfolder. No `-d` flag: demucs is left to its own cuda-else-cpu default.
//! **Stated honestly, not claimed as exact Buzz parity**: Buzz's own device
//! selection checks `BUZZ_FORCE_CPU` *first*, falling back to cuda-else-cpu
//! only when it's unset (`file_transcriber_queue_worker.py:183-188`);
//! ADR-023 forbids passing `-d` to this subprocess at all, so `HUMM_FORCE_CPU`
//! (which does gate the main whisper engine, `env::use_gpu_resolution`) has
//! no effect on the demucs step specifically — a `HUMM_FORCE_CPU=true` user
//! still gets CUDA-if-available for isolation alone. Not fixed here
//! (ledgered for the docs sweep's deviations list). The vocals file is
//! renamed to `<input_stem>_speech.mp3` beside `input` (Buzz naming, `:191`)
//! — **only after** a fully successful run, so no partial or failed-run
//! artifact is ever written next to the user's file; every intermediate
//! byte lives in a `tempfile::tempdir()` scratch directory that is deleted
//! (recursively, on `Drop`) on every exit path, including a cancellation or
//! a panic unwind, and the final placement itself
//! ([`copy_then_rename`]) is a same-directory copy-to-temp-then-atomic-
//! rename, so even the cross-device fallback can never leave a partial or
//! orphaned file at the destination. This is also the whole mechanism
//! behind the Task 4 review's cleanup contract ("on any `Err`, remove
//! partial/produced output") — there is nothing beside the input to remove,
//! because nothing is ever written there before success is confirmed.
//!
//! Cancellation mirrors Buzz's `_terminate_speech_extractor_process`
//! (`:339-350`): SIGTERM, a 5 s grace period (polled every ~100 ms via
//! `try_wait`, never a blocking `wait()`), then SIGKILL. `std::process::Child`
//! has no SIGTERM (`Child::kill` is always SIGKILL), so the unix path sends
//! it via `nix::sys::signal::kill` (already resolved in this workspace's
//! dependency graph via `humm-cli`'s `ctrlc`) through the shared
//! [`crate::signal_process_group`]/[`crate::kill_process_group`] helpers;
//! non-unix targets have no separate terminate/kill distinction to make
//! (Windows' own `TerminateProcess`, which `Child::kill` already calls, is
//! immediate — matching Python `multiprocessing.Process.terminate()`'s own
//! Windows behavior) so they go straight to `Child::kill()`.
//!
//! **Why the whole process group, not just the direct child** (earned
//! empirically mid-task-5, not anticipated by the spike, which never
//! exercised cancellation): a `/bin/sh` script whose last statement is
//! `sleep 30` forks a genuine child process for it on this platform (not
//! exec-tail-call optimized); signaling only the `sh` PID leaves that
//! `sleep` grandchild running for its full duration, which in turn keeps
//! the stdout/stderr pipes open and blocks this module's reader threads
//! until it exits naturally. A real `demucs` (a Python program) is equally
//! likely to have descendants of its own. [`crate::spawn_own_process_group`]
//! puts the child in a *new* process group at spawn time — which also
//! detaches it from this terminal session's foreground process group, so a
//! user's Ctrl-C no longer reaches demucs directly the way it would a
//! same-group child; the covering path is humm's own SIGINT handler (`humm-cli`'s
//! `ctrlc`-based Ctrl-C-cancels-tasks flow) flipping the task's
//! `CancelToken`, which this module's own cancellation check then reaches
//! via the explicit group-SIGTERM/SIGKILL above, not by relying on the
//! terminal to deliver a signal to demucs itself.
//!
//! **The above covers only the *first* Ctrl-C on its own** —
//! `humm-cli::run::install_ctrlc`'s second SIGINT calls
//! `std::process::exit(130)` directly, bypassing Rust's normal
//! unwind/`Drop` machinery entirely, so nothing would run the
//! cancel-and-wait path above and nothing would signal the detached
//! process group. **Closed in M9 (D5b-2/D5b-4):** every `isolate()` call
//! registers this child's group ([`crate::register_process_group`]) for
//! the duration of the spawn, and `install_ctrlc`'s second-signal arm now
//! calls [`crate::kill_all_registered_process_groups`] immediately before
//! that `exit(130)` — so even a second Ctrl-C mid-isolation reaps this
//! detached group instead of orphaning it (reparented to init) — the same
//! is true of yt-dlp (`fetch.rs`, the same `spawn_own_process_group`/
//! `kill_process_group`/`register_process_group` helpers).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use humm_core::boundary::SpeechIsolator;
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;
use humm_core::fsutil::copy_then_rename;

use crate::map_spawn_error;

/// How long the wait loop sleeps between `try_wait`/`cancel` polls — bounds
/// cancellation latency the same way `YtDlpFetcher`'s `CANCEL_POLL_INTERVAL`
/// does.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Grace period between SIGTERM and SIGKILL (Buzz's `process.join(timeout=5)`,
/// `file_transcriber_queue_worker.py:344`).
const TERM_GRACE: Duration = Duration::from_secs(5);

/// Isolates speech via a `demucs` subprocess (F-027, ADR-023).
pub struct DemucsIsolator {
    binary: PathBuf,
}

impl DemucsIsolator {
    /// Uses `HUMM_DEMUCS` if set, else `demucs` on `PATH`.
    pub fn new() -> Self {
        Self::from_env(std::env::var_os("HUMM_DEMUCS"))
    }

    /// Builds from an explicit `HUMM_DEMUCS` value (`Some` = override,
    /// `None` = fall back to `demucs` on `PATH`) without ever touching
    /// `std::env` itself — the test seam `new()` delegates to. Tests inject
    /// `Some(path_to_stub_script)`/`None` directly instead of mutating
    /// process environment (`std::env::set_var` is `unsafe` in edition
    /// 2024); mirrors `crate::binary_from_env_value`'s pure-function shape.
    pub fn from_env(override_value: Option<std::ffi::OsString>) -> Self {
        let binary = override_value
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("demucs"));
        DemucsIsolator { binary }
    }

    /// Uses an explicit binary path/name, bypassing `HUMM_DEMUCS` entirely.
    /// Test seam for constructing against a stub script by direct path.
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        DemucsIsolator {
            binary: path.into(),
        }
    }
}

impl Default for DemucsIsolator {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechIsolator for DemucsIsolator {
    fn isolate(&self, input: &Path, cancel: &CancelToken) -> Result<PathBuf, MediaError> {
        if cancel.is_canceled() {
            return Err(MediaError::Canceled);
        }

        let stem = input
            .file_stem()
            .ok_or_else(|| MediaError::IsolationFailed {
                detail: format!("input path {} has no file name", input.display()),
            })?
            .to_os_string();

        // Everything demucs writes lives here until (and only if) the whole
        // run succeeds; dropped (recursively deleted) on every return path,
        // including a panic unwind — see this module's doc comment.
        let scratch = tempfile::tempdir().map_err(MediaError::Io)?;

        let mut cmd = Command::new(&self.binary);
        cmd.arg("--two-stems=vocals")
            .args(["-n", "htdemucs"])
            .arg("--mp3")
            .arg("-o")
            .arg(scratch.path())
            .arg("--filename")
            .arg("{stem}.{ext}")
            .arg(input)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // See this module's doc comment ("why the whole process group") for
        // why the child is spawned into its own group here.
        crate::spawn_own_process_group(&mut cmd);

        let mut child = cmd.spawn().map_err(|err| {
            map_spawn_error(
                "demucs",
                "HUMM_DEMUCS",
                "install it (e.g. `pipx install demucs`)",
                &self.binary,
                err,
            )
        })?;
        // Registered for the child's whole lifetime (dropped — and thus
        // unregistered — on every return path below, including the panic
        // path, since it's a plain scope-bound local): closes the gap where
        // a second Ctrl-C's `std::process::exit` would otherwise bypass
        // this function's own cancel-then-terminate machinery entirely and
        // orphan this detached group (D5b-2, M9; see this module's doc
        // comment, "This covers only the *first* Ctrl-C").
        let _group_registration = crate::register_process_group(&child);

        let mut stdout = child.stdout.take().expect("stdout was piped at spawn");
        let stderr = child.stderr.take().expect("stderr was piped at spawn");

        // Drain both pipes on background threads: demucs's tqdm progress
        // goes to stderr and could in principle write enough to stdout too
        // that either pipe fills and blocks demucs mid-write while we wait
        // on the other — same deadlock hazard `FfmpegDecoder`/`YtDlpFetcher`
        // guard against. stdout content is discarded to a sink rather than
        // an unbounded growing buffer (there's nothing in it worth keeping,
        // unlike stderr's bounded diagnostic tail below).
        let stdout_handle = thread::spawn(move || std::io::copy(&mut stdout, &mut std::io::sink()));
        let stderr_handle = crate::stderr::spawn_stderr_tail(stderr);

        let status = match wait_checking_cancel(&mut child, cancel) {
            Ok(status) => status,
            Err(err) => {
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(err);
            }
        };

        let _ = stdout_handle.join();
        let stderr_tail = stderr_handle.join().unwrap_or_default();

        if !status.success() {
            return Err(MediaError::IsolationFailed {
                detail: format!(
                    "demucs exited with {status}: {stderr_tail} — demucs may need to \
                     download its model on first run; check your internet connection"
                ),
            });
        }

        let vocals = scratch.path().join("htdemucs").join("vocals.mp3");
        if !vocals.is_file() {
            return Err(MediaError::IsolationFailed {
                detail: format!(
                    "demucs exited successfully but {} was not produced",
                    vocals.display()
                ),
            });
        }

        let mut dest_name = stem;
        dest_name.push("_speech.mp3");
        let dest = input.with_file_name(dest_name);

        // Rename last, on confirmed full success only (this function's doc
        // comment) — `move_file`'s cross-device fallback (`copy_then_rename`)
        // is itself atomic; see that function's doc for why.
        move_file(&vocals, &dest).map_err(MediaError::Io)?;

        Ok(dest)
    }

    fn available(&self) -> bool {
        // M9 (D5b-7): re-probes the filesystem on every call — no caching.
        // `binary_is_available` is an env-override existence check or a
        // `PATH` scan (file `metadata` calls only, never a subprocess spawn
        // — see that function's own doc), cheap enough to re-run on every
        // options-sheet open (the GUI's re-probe trigger, `humm-gui`'s
        // `HummApp::open_options_sheet`) or CLI invocation (`-e` validates
        // at invocation time, already a fresh process/probe either way) —
        // an `OnceLock` here would otherwise mean `demucs` installed
        // mid-session is never seen without a restart.
        binary_is_available(&self.binary)
    }
}

/// Waits for `child` to exit, polling `cancel` every [`POLL_INTERVAL`]
/// instead of blocking on `wait()` — the only way to react to cancellation
/// promptly without a blocking-forever `wait()` call. On cancellation: sends
/// SIGTERM (unix) — or `kill()` on other targets, which is already an
/// immediate terminate there — then polls for up to [`TERM_GRACE`] before
/// escalating to a hard kill, reaping either way. Buzz parity:
/// `_terminate_speech_extractor_process` (`:339-350`).
fn wait_checking_cancel(
    child: &mut Child,
    cancel: &CancelToken,
) -> Result<std::process::ExitStatus, MediaError> {
    loop {
        if cancel.is_canceled() {
            return Err(terminate_then_kill(child));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(err) => {
                crate::kill_process_group(child);
                let _ = child.wait();
                return Err(MediaError::Io(err));
            }
        }
    }
}

/// SIGTERM (whole process group, unix — see this module's doc comment),
/// then poll for up to [`TERM_GRACE`], then [`crate::kill_process_group`] —
/// always returns `MediaError::Canceled` once the child is reaped either
/// way.
fn terminate_then_kill(child: &mut Child) -> MediaError {
    send_terminate(child);

    let deadline = Instant::now() + TERM_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return MediaError::Canceled,
            Ok(None) if Instant::now() >= deadline => break,
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(_) => break,
        }
    }

    crate::kill_process_group(child);
    let _ = child.wait();
    MediaError::Canceled
}

#[cfg(unix)]
fn send_terminate(child: &mut Child) {
    crate::signal_process_group(child, nix::sys::signal::Signal::SIGTERM);
}

#[cfg(not(unix))]
fn send_terminate(child: &mut Child) {
    // No separate terminate/kill distinction on this target — `Child::kill`
    // is already the platform's immediate-terminate call.
    let _ = child.kill();
}

/// Renames `from` to `to`, falling back to [`copy_then_rename`] (promoted to
/// `humm_core::fsutil` in M9, D5b-1/D5b-13 — see that function's doc for the
/// full atomic-placement rationale) when the rename fails (`fs::rename`
/// cannot cross filesystem/device boundaries — demucs's scratch tempdir and
/// the input's own directory are very often on different filesystems, e.g.
/// `/tmp` vs. a user media directory).
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => copy_then_rename(from, to),
    }
}

/// Checks whether `binary` can be executed, without spawning it. An explicit
/// path (more than one component, e.g. an absolute `HUMM_DEMUCS` override)
/// is checked for existence directly; a bare command name (the default
/// `"demucs"`) is looked up by scanning `PATH` for an executable file with
/// that name — the same resolution `Command::spawn` itself performs, done
/// here without actually spawning anything.
fn binary_is_available(binary: &Path) -> bool {
    if binary.components().count() > 1 {
        return is_executable_file(binary);
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| is_executable_file(&dir.join(binary))))
        .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[cfg(unix)]
    fn write_stub(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write stub script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub script");
        path
    }

    /// A stub demucs that writes the spike-confirmed layout
    /// (`<outdir>/htdemucs/vocals.mp3`) as soon as it sees `-o <dir>`,
    /// ignoring every other argument (the input path, `--two-stems=vocals`,
    /// etc.) exactly like the real binary does with its own flags.
    #[cfg(unix)]
    const STUB_WRITES_VOCALS: &str = r#"
outdir=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) outdir="$2"; shift 2 ;;
        *) shift ;;
    esac
done
mkdir -p "$outdir/htdemucs"
printf 'fake vocals data' > "$outdir/htdemucs/vocals.mp3"
printf 'fake no_vocals data' > "$outdir/htdemucs/no_vocals.mp3"
"#;

    #[test]
    fn discovery_prefers_env_override() {
        let isolator = DemucsIsolator::from_env(Some(std::ffi::OsString::from("/custom/demucs")));
        assert_eq!(isolator.binary, PathBuf::from("/custom/demucs"));
    }

    #[test]
    fn discovery_falls_back_to_path_when_env_absent() {
        let isolator = DemucsIsolator::from_env(None);
        assert_eq!(isolator.binary, PathBuf::from("demucs"));
    }

    #[test]
    fn available_false_when_binary_missing() {
        // An explicit nonexistent path exercises the "check existence
        // directly" branch deterministically, without depending on whether
        // this machine's real PATH happens to have demucs installed.
        let isolator = DemucsIsolator::with_binary("/nonexistent/demucs-for-test");
        assert!(!isolator.available());
    }

    #[test]
    #[cfg(unix)]
    fn available_true_when_stub_script_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stub = write_stub(dir.path(), "demucs", "exit 0");
        let isolator = DemucsIsolator::with_binary(&stub);
        assert!(isolator.available());
    }

    /// M9 (D5b-7): install-while-running staleness. `available()` must
    /// re-probe the filesystem on every call rather than caching its first
    /// result for the isolator's lifetime — a user who installs `demucs`
    /// mid-session (e.g. `pipx install demucs` while humm is already
    /// running) must see the capability flip without restarting. Uses the
    /// `from_env` seam (never `std::env::set_var`) to point `HUMM_DEMUCS` at
    /// a path that doesn't exist yet inside a tempdir, then creates the
    /// executable stub there after the first (false) probe.
    #[test]
    #[cfg(unix)]
    fn available_sees_binary_installed_after_first_probe() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("demucs");
        let isolator = DemucsIsolator::from_env(Some(path.clone().into_os_string()));

        assert!(
            !isolator.available(),
            "must be false before the binary exists"
        );

        write_stub(dir.path(), "demucs", "exit 0");

        assert!(
            isolator.available(),
            "install-while-running: available() must re-probe rather than reuse a cached false"
        );
    }

    #[test]
    #[cfg(unix)]
    fn isolate_renames_vocals_next_to_input() {
        // This spawns a real process-grouped child via `isolate()`
        // (registered in `crate::PROCESS_GROUPS`) — a shared read lock
        // against `crate::PROCESS_GROUP_TEST_LOCK` keeps it from being
        // collaterally SIGKILLed by a concurrently-running "kill
        // everything" registry test (that lock's own doc comment explains
        // the hazard), while still running freely alongside other normal
        // spawning tests like this one.
        let _serialize = crate::PROCESS_GROUP_TEST_LOCK
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let script_dir = tempfile::tempdir().expect("tempdir");
        let stub = write_stub(script_dir.path(), "demucs", STUB_WRITES_VOCALS);

        let input_dir = tempfile::tempdir().expect("tempdir");
        let input = input_dir.path().join("clip.mp3");
        std::fs::write(&input, b"not real media, just needs to exist").expect("write input");

        let isolator = DemucsIsolator::with_binary(&stub);
        let cancel = CancelToken::new();
        let result = isolator.isolate(&input, &cancel).expect("isolate ok");

        assert_eq!(result, input_dir.path().join("clip_speech.mp3"));
        assert_eq!(
            std::fs::read_to_string(&result).expect("read result"),
            "fake vocals data"
        );
        // Nothing else was left beside the input.
        let siblings: Vec<_> = std::fs::read_dir(input_dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            siblings.len(),
            2,
            "expected only input + _speech.mp3: {siblings:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn isolate_kill_on_cancel_acks_promptly() {
        // See `isolate_renames_vocals_next_to_input`'s comment above — same
        // shared-registry hazard, same fix.
        let _serialize = crate::PROCESS_GROUP_TEST_LOCK
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let script_dir = tempfile::tempdir().expect("tempdir");
        let stub = write_stub(script_dir.path(), "demucs", "sleep 30");

        let input_dir = tempfile::tempdir().expect("tempdir");
        let input = input_dir.path().join("clip.mp3");
        std::fs::write(&input, b"not real media, just needs to exist").expect("write input");

        let isolator = DemucsIsolator::with_binary(&stub);
        let cancel = CancelToken::new();
        let cancel_from_main = cancel.clone();

        let start = Instant::now();
        let handle = thread::spawn(move || isolator.isolate(&input, &cancel));
        thread::sleep(Duration::from_millis(200));
        cancel_from_main.cancel();

        let result = handle.join().expect("isolate thread should not panic");
        let elapsed = start.elapsed();

        assert!(matches!(result, Err(MediaError::Canceled)));
        assert!(
            elapsed < Duration::from_secs(2),
            "expected cancellation to ack within ~2s, took {elapsed:?}"
        );
        // No partial output ever landed beside the input.
        assert!(!input_dir.path().join("clip_speech.mp3").exists());
    }

    #[test]
    #[cfg(unix)]
    fn nonzero_exit_yields_actionable_error() {
        // See `isolate_renames_vocals_next_to_input`'s comment above — same
        // shared-registry hazard, same fix.
        let _serialize = crate::PROCESS_GROUP_TEST_LOCK
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let script_dir = tempfile::tempdir().expect("tempdir");
        let stub = write_stub(
            script_dir.path(),
            "demucs",
            "echo 'boom: model download failed' >&2\nexit 1",
        );

        let input_dir = tempfile::tempdir().expect("tempdir");
        let input = input_dir.path().join("clip.mp3");
        std::fs::write(&input, b"not real media, just needs to exist").expect("write input");

        let isolator = DemucsIsolator::with_binary(&stub);
        let cancel = CancelToken::new();
        let err = isolator
            .isolate(&input, &cancel)
            .expect_err("nonzero exit should fail");

        match err {
            MediaError::IsolationFailed { detail } => {
                assert!(detail.contains("demucs"), "detail was: {detail}");
                assert!(
                    detail.contains("boom: model download failed"),
                    "detail was: {detail}"
                );
                assert!(
                    detail.to_lowercase().contains("model"),
                    "expected the actionable model-download hint, detail was: {detail}"
                );
            }
            other => panic!("expected IsolationFailed, got {other:?}"),
        }
        // No partial output ever landed beside the input.
        assert!(!input_dir.path().join("clip_speech.mp3").exists());
    }

    #[test]
    fn missing_binary_reports_actionable_hint() {
        let isolator = DemucsIsolator::with_binary("/nonexistent/demucs-binary-for-test");
        let cancel = CancelToken::new();
        let input = std::env::temp_dir().join("clip.mp3");
        let err = isolator
            .isolate(&input, &cancel)
            .expect_err("missing binary should fail");
        match err {
            MediaError::BinaryMissing { hint, .. } => {
                assert!(hint.contains("HUMM_DEMUCS"), "hint was: {hint}");
                assert!(hint.contains("demucs not found"), "hint was: {hint}");
                // Minor 6 (task-5 review): demucs isn't a Homebrew package —
                // the hint must name a real install method for it, not the
                // generic `brew install <program>` every other adapter uses.
                assert!(hint.contains("pipx install demucs"), "hint was: {hint}");
                assert!(!hint.contains("brew"), "hint was: {hint}");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
    }

    #[test]
    fn pre_canceled_token_short_circuits_before_spawning() {
        let isolator = DemucsIsolator::with_binary("/nonexistent/demucs-binary-for-test");
        let cancel = CancelToken::new();
        cancel.cancel();
        let input = std::env::temp_dir().join("clip.mp3");
        let err = isolator
            .isolate(&input, &cancel)
            .expect_err("should be canceled");
        assert!(matches!(err, MediaError::Canceled));
    }

    #[test]
    fn move_file_uses_plain_rename_when_possible() {
        // Two tempdirs on the same filesystem: `fs::rename` succeeds
        // directly, so `move_file` never actually reaches `copy_then_rename`
        // here — this test is honest about testing only the primary-rename
        // branch (the review's Important 1 finding: the old version of this
        // test claimed to exercise the fallback but never did, since rename
        // across same-filesystem tempdirs just succeeds). The fallback
        // itself is tested directly, below.
        let from_dir = tempfile::tempdir().expect("tempdir");
        let to_dir = tempfile::tempdir().expect("tempdir");
        let from = from_dir.path().join("source.mp3");
        let to = to_dir.path().join("dest.mp3");
        std::fs::write(&from, b"payload").expect("write source");

        move_file(&from, &to).expect("move_file should succeed");

        assert_eq!(std::fs::read_to_string(&to).unwrap(), "payload");
        assert!(!from.exists());
    }

    // `copy_then_rename`'s own unit tests (atomic placement, temp cleanup on
    // copy/rename failure) moved to `humm_core::fsutil` alongside the
    // function itself (M9, D5b-1/D5b-13).
}
