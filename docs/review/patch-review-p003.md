# Patch review: P-003

Reviewed `43ea48433baff21dcf4076e7e0cbdbc48a17f4bc` on `audit/architecture-review`. Product baseline remains `125a54da3d284ba3be651b4c99a4456ce56cd76a`. No product source was edited. Line numbers below are that commit (`git diff 43ea484 HEAD -- crates` is only a later P-001 edit in `download.rs`).

## Verdict

**REQUEST_CHANGES** for `43ea48433baff21dcf4076e7e0cbdbc48a17f4bc`.

The second Ctrl-C path matches the accepted design. `try_emergency_stop` takes the registry with `try_lock`, SIGKILLs registered groups without waiting, deletes speech temps and the demucs scratch directory, and leaves a denoised file registered with `keep`. The handler does not lock the queue handle, and it still exits 130 when the lock is busy. The user's source path is not registered.

The new `humm-media` test calls that process-global killer while other lib tests still have live registrations. Default `cargo test -p humm-media --lib` then SIGKILLs those stubs. That is a failing commit gate.

## Blocking

`try_emergency_stop` (`crates/humm-core/src/abort.rs` lines 121–134) signals every pid in the process-global registry and deletes every temp that is not marked keep. `DemucsIsolator::isolate` registers its child after spawn (`crates/humm-media/src/demucs.rs` line 191). `emergency_stop_kills_grandchild_and_respects_keep_denoised_file` (line 728) calls `try_emergency_stop(kill_tracked_groups)` inside the same `humm-media` lib test binary as the other demucs tests. Lib tests share one process and run in parallel.

Reproduced here with `cargo +stable test -p humm-media --lib` (rustc 1.99.1):

- Attempt 7 of 20 failed. `isolate_renames_vocals_next_to_input` panicked at `demucs.rs:502` with `demucs exited with signal: 9 (SIGKILL)`. `isolate_kill_on_cancel_acks_promptly` panicked at `demucs.rs:543` because the result was not `MediaError::Canceled`. In that same run, `emergency_stop_kills_grandchild_and_respects_keep_denoised_file` passed.
- A `demucs::tests` filter failed the same two tests on attempt 2, with the same signal 9 text.

`isolate` registers the child before `wait_checking_cancel`. An outside `SIGKILL` makes `try_wait` return a killed status. The success path then returns `IsolationFailed`, which is what `isolate_renames_vocals_next_to_input` printed. The cancel test expects `Canceled`, so the same early kill fails it too.

The lead's three-name filter passed once in this review: `emergency_stop_kills_grandchild_and_respects_keep_denoised_file`, `isolate_kill_on_cancel_acks_promptly`, and `cancel_kills_grandchild_process_not_just_direct_child`. `humm-core` `abort::` was 4 passed, and `queue::tests::denoised_output_kept_when_keep_denoised_file_set` passed. That filter never starts `isolate_renames_vocals_next_to_input`, and one green run does not show the race. `YtDlpFetcher::fetch` registers its child the same way (`crates/humm-media/src/fetch.rs` line 141), so `cancel_kills_grandchild_process_not_just_direct_child` sits in the same blast radius. It did not fail in the captured run.

`cargo test --workspace` is a commit gate. This commit makes the `humm-media` lib suite flaky. The registry has to stay process-global for the real handler. The test that calls `try_emergency_stop` has to stop sharing that killer with other live registrations.

## What holds

`install_ctrlc` (`crates/humm-cli/src/run.rs` lines 787–795) does not lock `shared` on the second signal. `ctrlc` 3.5.2 runs the closure on a dedicated thread (`set_handler_inner` in ctrlc 3.5.2 `src/lib.rs`), so the registry mutex is not taken inside the libc signal handler. A busy or poisoned `try_lock` returns false, and the handler still calls `std::process::exit(130)`.

`kill_tracked_groups` (`crates/humm-media/src/lib.rs` line 120) sends `SIGKILL` to `-(pid as i32)`. That is the same group target as `signal_process_group`. On this machine, `killpg` of an `sh -c 'sleep 30'` session leader removes the `sleep` grandchild. `SIGKILL` of only the shell does not: `sleep` stays, reparented to pid 1, in that group (observed shell 39228, sleep 39229).

Speech is registered only when isolation returns a path different from the input (`crates/humm-core/src/queue.rs` lines 1416–1421). The denoised registration passes `guard.keep_denoised` (lines 1483–1486). The scratch directory is `track_temp_dir(scratch.path())` (`demucs.rs` line 192). The source path is not registered. `denoised_output_kept_when_keep_denoised_file_set` still passes. It covers `TempAudioGuard`'s `Drop`, which is the normal return path, and it does not call `try_emergency_stop`.

## Nonblocking

The new media test does not observe the grandchild. The command is `sh -c "sleep 30"` (`demucs.rs` line 743). The loop at lines 757–762 `try_wait`s that shell and panics with "grandchild still running". A `kill_tracked_groups` that signaled the positive pid would still pass, because the shell's `try_wait` succeeds while `sleep` remains. The function in this commit signals the group.

The rustdoc immediately above `kill_tracked_groups` is the previous `kill_process_group` comment with the new paragraph appended (`lib.rs` lines 91–111). `kill_process_group` (line 129) now has no docs. The upper paragraph still describes `child` and a `Child::kill()` fallback. `kill_tracked_groups` does not call `Child::kill()`.

## Uncertain

`isolate` holds `TrackedProcess` until the function returns (`demucs.rs` line 191). `wait_checking_cancel` has already reaped the child (line 207) before `move_file` (line 245). The cross-device copy writes `{file_name}.tmp-{pid}` next to the destination (`demucs.rs` line 377) and does not register that path. A second Ctrl-C during that copy skips `Drop` and can leave the sibling temp. The still-registered pgid is the recycled-pgid case named in the design, and only after the group is empty. Pid allocation is cyclic, so reuse during one copy needs a full pid-space wrap. This review did not run that window.

The `humm-core` abort tests call the same global `try_emergency_stop`, which deletes every temp that is not marked keep. `extract_speech_swaps_decode_input_and_emits_phase` registers a speech file and requires it to exist for the decode step. Ten combined runs of `abort::`, `extract_speech`, `isolation_output`, and `denoised_output` (9 tests) all passed. No core failure showed up.

## Follow-up `1bfd986`

The verdict on `43ea48433baff21dcf4076e7e0cbdbc48a17f4bc` stays **REQUEST_CHANGES**.

**PASS** for `1bfd98699dd015ca02c57266d75779a28e905f07`.

`try_emergency_stop` still stops every registration. It calls `stop_locked` with both filters `|_| true` (`abort.rs` lines 135–140). The handler is unchanged and still calls that function (`run.rs` line 794). `try_emergency_stop_registered` (lines 148–163) is the only narrowed path. Its only caller is the humm-media test, which passes `_group.registration_id()` and the three temp ids it just took (`demucs.rs` lines 754–762). Ids come from one monotonic counter and are not reused, so those four ids are not another test's registrations. `kill_tracked_groups` still signals only the pids it is given. The asserts after the call are the same: speech and scratch gone, denoised and source still there, child reaped within 2 seconds.

`cargo +stable test -p humm-media --lib` on this commit: 35 runs, zero `signal: 9`. Two runs failed `isolate_renames_vocals_next_to_input` with `ExecutableFileBusy` (`Text file busy`) at the stub `spawn`, before that child is registered. The same error hit `nonzero_exit_yields_actionable_error` on run 20 of 20 with `--skip emergency_stop_kills_grandchild` (78 tests, that test filtered out). That spawn race is not this commit. `humm-core` `abort::` plus `denoised_output_kept_when_keep_denoised_file_set` was 5 passed. The grandchild check still `try_wait`s only the shell. That was nonblocking on `43ea484` and this commit does not change it.
