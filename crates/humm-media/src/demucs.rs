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
//! [`crate::signal_process_group`]/[`crate::kill_process_group`] helpers.
//! Windows has no SIGTERM. A pip or cmd launcher is often the direct child,
//! and `Child::kill` stops only that process, so cancellation calls
//! [`crate::kill_process_group`] (`taskkill /F /T`) immediately. Waiting for
//! the launcher to exit and then returning would leave the Python
//! interpreter running.
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
//! **The second Ctrl-C does not unwind.** `humm-cli`'s handler calls
//! [`humm_core::abort::try_emergency_stop`] before `std::process::exit`,
//! which SIGKILLs every detached media child (`prepare_command` registers
//! the process group at spawn, including this one) and deletes
//! the scratch directory without waiting and without running `Drop`. The
//! user's source file is not registered. A denoised file is registered
//! separately and kept when the user asked to keep it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use humm_core::boundary::SpeechIsolator;
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;

use crate::map_spawn_error;

/// How long the wait loop sleeps between `try_wait`/`cancel` polls — bounds
/// cancellation latency the same way `YtDlpFetcher`'s `CANCEL_POLL_INTERVAL`
/// does.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Grace period between SIGTERM and SIGKILL (Buzz's `process.join(timeout=5)`,
/// `file_transcriber_queue_worker.py:344`).
const TERM_GRACE: Duration = Duration::from_secs(5);

/// Bytes of trailing stderr retained for an actionable [`MediaError::IsolationFailed`]
/// diagnostic, bounding memory for demucs's chatty tqdm progress output.
const STDERR_TAIL_CAP: usize = 2 * 1024;

/// Isolates speech via a `demucs` subprocess (F-027, ADR-023).
pub struct DemucsIsolator {
    binary: PathBuf,
    /// Lazily computed, cached availability probe (`available()`'s doc:
    /// "cheap" — demucs's own startup pulls in PyTorch and is far too slow
    /// to spawn just to check whether it's installed, unlike ffmpeg/yt-dlp's
    /// quick `-version`/`--version` in `capabilities.rs`), so this checks the
    /// filesystem directly instead of spawning anything.
    available: OnceLock<bool>,
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
        DemucsIsolator {
            binary,
            available: OnceLock::new(),
        }
    }

    /// Uses an explicit binary path/name, bypassing `HUMM_DEMUCS` entirely.
    /// Test seam for constructing against a stub script by direct path.
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        DemucsIsolator {
            binary: path.into(),
            available: OnceLock::new(),
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

        let (mut cmd, binary) = crate::prepare_command(&self.binary);
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
        // `prepare_command` puts the child in its own process group on unix
        // (see this module's doc comment) and sets CREATE_NO_WINDOW on Windows.

        let spawned = crate::spawn_prepared(&mut cmd).map_err(|err| {
            map_spawn_error(
                "demucs",
                "HUMM_DEMUCS",
                crate::install_hint("demucs"),
                &binary,
                err,
            )
        })?;
        // `spawn_prepared` registers the process group. The lock is not held
        // across `spawn`. The guard must outlive `wait`.
        let (mut child, _group) = spawned.into_parts();
        let _scratch_track = humm_core::abort::track_temp_dir(scratch.path());

        let mut stdout = child.stdout.take().expect("stdout was piped at spawn");
        let mut stderr = child.stderr.take().expect("stderr was piped at spawn");

        // Drain both pipes on background threads: demucs's tqdm progress
        // goes to stderr and could in principle write enough to stdout too
        // that either pipe fills and blocks demucs mid-write while we wait
        // on the other — same deadlock hazard `FfmpegDecoder`/`YtDlpFetcher`
        // guard against. stdout content is discarded to a sink rather than
        // an unbounded growing buffer (there's nothing in it worth keeping,
        // unlike stderr's bounded diagnostic tail below).
        let stdout_handle = thread::spawn(move || std::io::copy(&mut stdout, &mut std::io::sink()));
        let stderr_handle = thread::spawn(move || read_stderr_tail(&mut stderr));

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
        *self
            .available
            .get_or_init(|| binary_is_available(&self.binary))
    }
}

/// Waits for `child` to exit, polling `cancel` every [`POLL_INTERVAL`]
/// instead of blocking on `wait()` — the only way to react to cancellation
/// promptly without a blocking-forever `wait()` call. On cancellation: sends
/// SIGTERM to the process group (unix), or [`crate::kill_process_group`]
/// (Windows `taskkill /F /T`, the same tree kill decode/fetch/video use),
/// then polls for up to [`TERM_GRACE`] before escalating to a hard kill,
/// reaping either way. Buzz parity:
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
    // Tree kill, not `Child::kill`. A Windows pip/cmd shim exits on
    // TerminateProcess while the Python child keeps running, and
    // `terminate_then_kill` returns as soon as this direct child is reaped.
    crate::kill_process_group(child);
}

/// Reads `stderr` to completion in the background (so demucs never blocks
/// writing to a full pipe), retaining only the last `STDERR_TAIL_CAP` bytes
/// for an `IsolationFailed` diagnostic — same shape as `fetch.rs`'s
/// `read_stderr_tail`.
fn read_stderr_tail(stderr: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stderr.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > STDERR_TAIL_CAP {
                    let excess = buf.len() - STDERR_TAIL_CAP;
                    buf.drain(0..excess);
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).trim().to_string()
}

/// Renames `from` to `to`, falling back to [`copy_then_rename`] when the
/// rename fails (`fs::rename` cannot cross filesystem/device boundaries —
/// demucs's scratch tempdir and the input's own directory are very often on
/// different filesystems, e.g. `/tmp` vs. a user media directory).
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => copy_then_rename(from, to),
    }
}

/// Cross-device fallback for [`move_file`]: copies `from` to a sibling temp
/// file in **`to`'s own directory** (same filesystem as the eventual
/// destination), then `rename`s that temp file into place as the one
/// atomic last step — a same-filesystem rename can't itself fail with a
/// cross-device error, and rename is atomic, so `to` either doesn't exist
/// yet or is fully, correctly present; never partial.
///
/// Review fix (task-5 Important 1): the original version called
/// `fs::copy(from, to)` directly, writing straight to the final
/// destination. A copy failing partway (disk full, I/O error) could leave a
/// partial file *at* `to`; worse, if the copy fully succeeded but the
/// following `remove_file(from)` failed, the function still returned `Err`
/// even though `to` now held a complete file — the caller's `?` propagates
/// that error, so `isolate` returns `Err` and the queue's cleanup guard
/// never learns `to`'s path, permanently orphaning it. This version never
/// writes to `to` directly: the temp file is removed on *any* failure
/// (copy or rename), and `to` only ever comes into existence via one atomic
/// `rename` once the copy is already known-complete. Mirrors
/// `Settings::save`'s `<file_name>.tmp-<pid>` atomic-write convention
/// (`settings.rs`) — same-directory temp name, rename-into-place last.
fn copy_then_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let parent = to.parent().unwrap_or_else(|| Path::new("."));
    let file_name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let temp = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    match std::fs::copy(from, &temp).and_then(|_| std::fs::rename(&temp, to)) {
        Ok(_) => {
            // `from` lives in the isolator's own ephemeral scratch tempdir
            // (deleted wholesale on drop regardless of what happens here) —
            // a failure removing it doesn't undo the now-complete,
            // correctly-placed `to`, so it isn't treated as this
            // function's own failure.
            let _ = std::fs::remove_file(from);
            Ok(())
        }
        Err(err) => {
            // Never leave a partial/orphaned file behind: on *any* failure
            // (the copy itself, or the rename-into-place), the temp file is
            // removed and `to` is never touched at all.
            let _ = std::fs::remove_file(&temp);
            Err(err)
        }
    }
}

/// Checks whether `binary` can be executed, without spawning it. An explicit
/// path (more than one component, e.g. an absolute `HUMM_DEMUCS` override)
/// is checked for existence directly. A bare command name (the default
/// `"demucs"`) follows `Command`: on Unix, `PATH` (an empty component is the
/// current directory); on Windows, the application directory, `System32`,
/// the Windows directory, then `PATH`, appending `.exe` only and skipping
/// empty `PATH` entries. `.bat` and `.cmd` are not candidates.
fn binary_is_available(binary: &Path) -> bool {
    crate::executable_exists(binary)
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

    #[test]
    #[cfg(unix)]
    fn isolate_renames_vocals_next_to_input() {
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
                // Demucs is a pip/pipx install on every OS. The ffmpeg/yt-dlp
                // hints name Homebrew, apt, or winget instead.
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

    #[test]
    fn copy_then_rename_places_file_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("source.mp3");
        let to = dir.path().join("dest.mp3");
        std::fs::write(&from, b"payload").expect("write source");

        copy_then_rename(&from, &to).expect("copy_then_rename should succeed");

        assert_eq!(std::fs::read_to_string(&to).unwrap(), "payload");
        assert!(
            !from.exists(),
            "source should be removed after a successful move"
        );
        // No leftover temp file beside the destination.
        let siblings: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            siblings.len(),
            1,
            "expected only the destination file, no leftover temp: {siblings:?}"
        );
    }

    #[test]
    fn copy_then_rename_cleans_up_temp_on_copy_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Source doesn't exist: the initial `fs::copy` fails immediately,
        // before any temp file is even created.
        let from = dir.path().join("does-not-exist.mp3");
        let to = dir.path().join("dest.mp3");

        let err = copy_then_rename(&from, &to).expect_err("missing source should fail the copy");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        let siblings: Vec<_> = std::fs::read_dir(dir.path()).expect("read_dir").collect();
        assert!(
            siblings.is_empty(),
            "expected no leftover files after a failed copy: {siblings:?}"
        );
    }

    #[test]
    fn copy_then_rename_cleans_up_temp_on_rename_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("source.mp3");
        std::fs::write(&from, b"payload").expect("write source");
        // `to` is an existing directory: a file can't be renamed onto a
        // directory, giving a copy-succeeds/rename-fails case.
        let to = dir.path().join("dest-is-a-dir");
        std::fs::create_dir(&to).expect("create directory at dest path");

        let result = copy_then_rename(&from, &to);
        assert!(
            result.is_err(),
            "renaming a file onto an existing directory should fail"
        );

        // Regardless of the exact (platform-dependent) error kind: no
        // leftover temp file, and `to` is still the empty directory it was
        // (never touched).
        let siblings: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            siblings.len(),
            2,
            "expected only source + the dest dir, no leftover temp: {siblings:?}"
        );
        assert!(
            to.is_dir(),
            "dest path should still be the untouched directory"
        );
    }

    /// Second Ctrl-C path: SIGKILL the group without waiting, delete speech
    /// and the scratch dir, and leave a denoised file the user asked to keep
    /// plus the untracked source.
    #[test]
    #[cfg(unix)]
    fn emergency_stop_kills_grandchild_and_respects_keep_denoised_file() {
        use std::process::Command;

        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("clip.mp3");
        let speech = dir.path().join("clip_speech.mp3");
        let denoised = dir.path().join("clip_DeepFilterNet3.wav");
        let scratch = dir.path().join("scratch");
        std::fs::write(&source, b"source").expect("source");
        std::fs::write(&speech, b"speech").expect("speech");
        std::fs::write(&denoised, b"denoised").expect("denoised");
        std::fs::create_dir(&scratch).expect("scratch");
        std::fs::write(scratch.join("partial"), b"partial").expect("partial");

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 30");
        crate::spawn_own_process_group(&mut cmd);
        let mut child = cmd.spawn().expect("spawn sleep");
        let _group = humm_core::abort::track_process(child.id());
        let _speech_track = humm_core::abort::track_temp_file(&speech, false);
        let _scratch_track = humm_core::abort::track_temp_dir(&scratch);
        let _denoised_track = humm_core::abort::track_temp_file(&denoised, true);

        let start = Instant::now();
        // Only this test's registrations. The process-global stop would
        // SIGKILL demucs stubs other lib tests still have registered.
        assert!(
            crate::emergency_stop_registered_for_test(
                &[_group.registration_id()],
                &[
                    _speech_track.registration_id(),
                    _scratch_track.registration_id(),
                    _denoised_track.registration_id(),
                ],
            ),
            "registry lock stayed busy"
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => panic!("grandchild still running after SIGKILL"),
                Err(err) => panic!("try_wait: {err}"),
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "kill waited {:?}",
            start.elapsed()
        );
        assert!(!speech.exists());
        assert!(!scratch.exists());
        assert!(denoised.exists());
        assert!(source.exists());
    }
}
