---
cursor:
  subagentId: "bc-5c649f91-53a5-581c-aada-d91b2b0433b7"
---

# Patch review: folder watch, URL skip, CLI speaker flags

Reviewed on `audit/architecture-review`. Buzz was read at `/tmp/audit-sources/buzz` `298dbf003505cf945bdfd1eb8e705348b59a5b34`. No product source was edited. `docs/review/patch-review-deferred.md` was not opened.

| SHA | Claim | Verdict |
|---|---|---|
| `c0830754f7e3ffbe30ce52f7947e13cc931c44e5` | Folder watch queues on first sight and adopts after 30s unchanged size and mtime | REQUEST_CHANGES |
| `fd7f3aaba96c9a8e62c35f9a3398b7f888f79265` | A URL task skips by stored URL and does not use result files | PASS |
| `d00bab7d9e00f052562e61e875b9a5d689ef1d57` | CLI speaker flags parse and are rejected because the diarizer cannot apply them | REQUEST_CHANGES |

## `c083075` — REQUEST_CHANGES

Buzz `40f6a78` (`transcription_task_folder_watcher.py`) does what the lead says about the clock. `settle_seconds = 30`, `settle_poll_interval_ms = 5000`. First sight stores `(task, stamp, time.monotonic())` and emits `task_found` (lines 113–121). `check_pending_files` (lines 139–168) discards on `OSError`, resets the clock when the stamp changes, and emits `task_settled` only when `now - stamp_since >= settle_seconds`. `main_window.py` lists the row immediately (`on_folder_watch_task_found`, lines 479–483), hands it to the worker only if it is still queued (lines 485–488), and cancels on `task_discarded` (lines 490–492). There is no fast path for a file that was already stable. `find_tasks()` at startup (`main_window.py` line 198) still waits 30s. humm's `SettleSet` matches that clock: insert does not reset an existing row, poll settles at `>= 30s`, a changed stamp resets `since`, a missing stat is discarded. `cargo +stable test -p humm-core --lib folderwatch::` — 21 passed, including the three settle tests. `adopt_runs_a_row_that_already_exists_without_creating_it` passed. Delaying an already-stable file by 30s is not a defect.

The watcher that is supposed to use that path does not build, and the rows it would write are not the Buzz rows.

### Blocking

`rescan_and_enqueue` binds `seen` inside the block that ends at `crates/humm-gui/src/folder_watch.rs` line 750, then calls `drop(seen)` at line 815. `seen` is not in scope. That is a compile error. `humm-gui` was not compiled on this commit (`speakrs` `coreml` is macOS-only). The settle loop in `run_worker` is not a program that has been built.

After that line is removed, the wiring still misses the two things `task_found` / `task_discarded` exist to do.

The table learns about a task from `TaskEventKind::Queued` (`viewmodel.rs` `TasksVm::apply`, lines 353–356). `Started` and terminal events refresh an existing row and, if none exists, insert one (`refresh_from_store`, lines 603–612). Folder watch calls `store.create_task` on the worker (line 808) and `request_repaint` (lines 817–818). It does not emit `Queued`. `request_repaint` does not reload `TasksVm`; the table is loaded once and then folded from queue events (`app.rs` around lines 587 and 4227). `QueueHandle::adopt` only pushes `PendingTask`. The runner emits `Started` after `mark_started` (`queue.rs` lines 1191–1197), so the row first appears as in progress, about 30s after the file was seen. Buzz refreshes the table at `task_found`, while the file is still copying, so the user can see it and cancel it. A cancel here cannot be issued from the table during that window. A file that disappears is `mark_canceled` in the store (`poll_settled`, lines 859–864) with no `Canceled` event either.

`has_task_for_file` is true for every status (`store.rs` lines 702–714; the test at lines 2096–2102 keeps the match after `mark_failed`). `mark_canceled` (lines 1531–1539) leaves the `file` column in place. `scan` drops a path as soon as `known` is true (`folderwatch.rs` lines 259–261), before the seen-set stamp is consulted. Buzz constructs the watcher with `tasks={}` (`main_window.py` lines 189–191) and never writes that dict again. `paths_emitted` is set only when a file settles (`transcription_task_folder_watcher.py` lines 167–168). `discard_pending_files` (lines 173–178) does not set it. The next `find_tasks` creates a new task.

humm cancels on the same occasions and then cannot create that new task:

- The file is removed before 30s and replaced. `poll` discards it, the row is canceled, the replacement is `known`, and it is never adopted. That is the copy-that-does-not-stay-put case this wait is for.
- Folder watch stops inside the 30s window. `on_exit` (`app.rs` lines 4340–4343) and a disable/restart (`sync_folder_watch`) call `stop`, which sends `Shutdown`. `discard_pending` cancels every unsettled row. The next process has an empty `SettleSet` and a canceled row, so startup `find_tasks` does not run. Existing files all enter that window at start. Quitting within 30s of launch cancels each of them, and the next launch does not transcribe them. Retry is allowed for `Canceled` and not for `Queued` (`app.rs` lines 4400–4406), so a row left queued by a crash is not even manually restartable, and the watch will not adopt it.

`crates/humm-gui/tests/folder_watch_int.rs` was not compiled. It also cannot pass against this timing. `drop_creates_a_task_and_moves_the_source_to_the_mirrored_output_dir` (line 385), `drop_with_delete_processed_files_removes_the_source` (line 451), and `paused_service_ignores_a_drop_until_resumed` (line 520) wait 30s for `Completed`. Adopt cannot happen until the stamp has been stable for 30s, and the poll is every 5s, so completion is after that deadline. `seen_set_survives_a_rebuild_so_a_recently_enqueued_file_is_not_double_enqueued` waits 10s for `SeenSet::record` (lines 605–618), which now happens only after settle, and then asserts `has_task_for_file` is still false (lines 624–628). `create_task` now runs in the watcher, on first sight, not in the runner.

What has to be true: `humm-gui` builds; a found file is a queued row in the table before it is adopted; a file that disappears, or a watch that stops, before settle does not leave a row that makes `scan` ignore the path when the file is there again; the folder-watch integration tests observe that, including the 30s wait.

### Nonblocking

The doc on `rescan_and_enqueue` (lines 716–719) still says the seen-set is recorded immediately after enqueue. Recording moved to `poll_settled`. `drop(seen)` is the leftover of the old lock.

Pausing does not freeze a settle that already started. The 5s timeout calls `poll_settled` with no `paused` check (`run_worker`, the `None` arm). Pause already meant "do not rescan", and a file enqueued under the old code was already in the runner. Not a new loss if the table shows the row and the user can cancel it.

## `fd7f3aa` — PASS

No blocking issue. Buzz `bc2882b` (`plugin.py` lines 56–68) returns before result-file search when `source == URL_IMPORT`. An empty URL or `check_database` false returns `None`. Otherwise it loads the newest `completed` row with that URL (`transcription_dao.py` `find_completed_transcription_by_url`: `status = 'completed'`, `ORDER BY time_ended DESC LIMIT 1`). File tasks still use result files, then the basename.

humm does the same split. `Runner::run_job` passes `Some(url.as_str())` only for `Input::Url`, and `None` for `Input::Path` (`queue.rs` lines 1376–1387), after the download has turned the job path into `source.<ext>`. `check` returns on that `Some` (`skip.rs` lines 49–54) and does not call `find_result_file_segments` or `find_db_segments`. `find_completed_transcription_by_url` (`store.rs` lines 678–691) is the same SQL. The file arm is the previous function, and it still clears translations in the shared `load_segments`. A file task's row stores `url = NULL` (`create_task`, lines 1373–1375), so a basename lookup is not how a URL task is found.

Re-ran: `cargo +stable test -p humm-core --lib postproc::skip::` — 23 passed. `skip_matches_a_completed_url_and_ignores_a_different_one` passed. `skip_check_db_hit_drops_translations_through_task_store_trait` passed (file task, `source_url` `None`, same basename in another directory).

### Nonblocking

`url_task_matches_the_stored_url_not_the_download_name` sets both `found` and `found_url`, and the fake store ignores the URL string. The hit assertion would also pass on the filename path. The store test is the one that separates `https://example.com/watch?v=1` from `https://example.com/other` against a row whose `file` is null. A URL hit does not log; a file hit does (`skip.rs` lines 69–72). Buzz logs the URL (`plugin.py` lines 64–67).

## `d00bab7` — REQUEST_CHANGES

Rejecting `--speaker-count` and `--speaker-diarizer` is a faithful response. Rejecting `--identify-speakers` by the same path is a stub.

Buzz `8d0812a` / `cli.py` lines 294–319 sets `identify_speakers` when any of the three options is set, parses `--speaker-count` as an int, and passes the diarizer string through. The default diarizer value is `msdd` even when the option is not set; `isSet` is what turns identification on. `speaker_identifier.py` lines 97–100 passes `num_speakers` only when `diarizer == "msdd"`.

humm's engine cannot apply either parameter. `Diarizer::diarize` (`crates/humm-core/src/diarize.rs` lines 126–131) takes `&Pcm`, a progress callback, and a cancel token. `OrtDiarizer::diarize` (`crates/humm-diarize/src/lib.rs` lines 186–190) calls `OwnedDiarizationPipeline::run(audio.samples())`. speakrs 0.5.0 defines that as `run(&mut self, audio: &[f32])` (`pipeline.rs` lines 65–66 in the crates.io source). ADR-011 and ADR-024 keep speaker identification on that pipeline and do not port NeMo MSDD or Sortformer. Running speakrs for `--speaker-diarizer sortformer` or `--speaker-count 4` would ignore the flag the user set. An error for those two is the honest outcome. No new engine is required for that conclusion.

`--identify-speakers` with no count and no diarizer does not name an algorithm. `wants_speaker_identification` (`args.rs` lines 192–194) treats it like the other two. `validate_add_args` (`run.rs` lines 133–146) then returns `usage_fail` before `execute` (`run_add` line 100). The transcription does not run. `humm add --help` prints:

- `--identify-speakers` — "Identify speakers in the audio. Buzz `8d0812a`"
- `--speaker-diarizer` — "Default when the flag is passed: `msdd`"

There is no clap default. The field is `Option`, and `--speaker-diarizer` without a value does not become `msdd`. The enum help says "Not the engine humm runs". The boolean flag's help does not say the command will exit. The GUI already identifies speakers with this diarizer (`speaker_id.rs` `RealDiarizeService::diarize`). `TranscriptionOptions` has no speaker field, and `queue.rs` never calls `diarize`, so this commit does not connect the flag to that engine. The help offers a feature, and every `add` that passes the flag exits 1. Before this commit, clap reported an unknown argument. That was the smaller surface. Leaving the flags unrecognized is enough. So is running the existing diarizer for the bare flag and rejecting only the count and the algorithm. The current shape does neither.

`cargo +stable test -p humm-cli --lib` — 41 passed, including the four new speaker tests. The lead's "14" is not the count of this binary. `cargo test -p humm-cli --test cli` failed 14 tests on a missing `/Users/jawad/RustroverProjects/buzz/testdata/ggml-tiny.bin`. `tests/cli.rs` is not in this commit. Those failures are the fixture path, not the new flags.

### Nonblocking

`speaker_flags_are_rejected_without_a_matching_engine` does not pass `--speaker-diarizer` into `validate_add_args`. The shared predicate covers it. The help text quotes a Buzz commit id at the user.

## Follow-up

The verdicts above stay on those SHAs. This pass is only the two commits that followed them.

| SHA | Claim | Verdict |
|---|---|---|
| `561aa9b7a78baeb631d0b34481aae58d18882893` | Queued is emitted before the wait, and a canceled settle no longer blocks the next scan | REQUEST_CHANGES |
| `95d980990f916b12129080b68e64d0139703e253` | Bare `--identify-speakers` runs `OrtDiarizer` after transcription | PASS |

## `561aa9b` — REQUEST_CHANGES

`QueueHandle::stage` does emit `Queued` without starting (`queue.rs` `Command::Stage`), and `has_blocking_task_for_file` ignores `canceled` (`store.rs`). `poll_settled` records `SeenSet` only on the settled arm, not on discard. `drop(seen)` is gone. On an idle runner the completion tests really wait out the 30s settle: `drop_creates_a_task_and_moves_the_source_to_the_mirrored_output_dir` finished in 30.26s, and `HUMM_BUZZ_TESTDATA=/tmp/audit-sources/buzz/testdata cargo +stable test -p humm-gui --test folder_watch_int` was 4 passed in 30.98s. `cargo +stable test -p humm-core --lib folderwatch::` was 22 passed, including `file_removed_during_settle_is_found_again`. The lead's 24 is not the count of that filter.

A clean stop can be seen again. `discard_pending` sends `Abandon`, which `write_canceled`s the row, and the next process's `cancel_stale_tasks` flips any leftover `queued` row to `canceled` before the watch starts. `has_blocking_task_for_file` then does not match, and `SeenSet` is empty in the new process.

That is not what happens when the file is sighted while the runner is already inside `run_task`.

### Blocking

`Runner::run` drains the command channel, then calls `run_task`, and does not drain again until that call returns (`queue.rs` `run`, and the drain at the start and tail of `run_task`). `stage` only sends `Command::Stage`. The row does not exist until the runner handles it.

`poll_settled` (`folder_watch.rs` lines 836–850) does not wait for that. `SettleSet::poll` removes the path once 30s have passed. The watcher then reads `get_task`. If the row is missing, `still_queued` is false, `adopt` is not called, and `seen.record` runs anyway.

Counterexample: a transcription is already running and will keep the runner inside `run_job` for more than 30s. A file appears in the watch folder. `stage` sits on the channel. At 30s the watcher records the path as seen and drops it from `SettleSet`. When the running task ends, `Stage` creates a `queued` row and emits `Queued`. Nothing adopts it. `has_blocking_task_for_file` is true for `queued`, and rule 5 of `scan` runs before the seen-set stamp, so a later size change does not create another task. The row stays queued for the rest of the session. The four integration tests do not hit this. Their runner is idle when the file is sighted, so `Stage` lands immediately, and the rebuild test holds the runner busy so the first row does not exist yet when the second watch scans.

What has to be true: a path whose row is not in the store yet stays pending, and it is not recorded as seen. `adopt` runs only after that row is `queued`.

### Nonblocking

`file_removed_during_settle_is_found_again` passes `|_| false` as `known`. It shows a discard does not record `SeenSet`. It does not call `has_blocking_task_for_file`. The store test `canceled_file_does_not_block_folder_watch` covers that query. `seen_set_survives_a_rebuild_so_a_recently_enqueued_file_is_not_double_enqueued` stops the watch while the blocker is in progress, so the second scan runs before `Stage` has inserted the first row. The module comment at the top of `folder_watch_int.rs` still describes the old double-enqueue assertion.

A user cancel during the wait only inserts the id into the runner's canceled set (`Command::Cancel`). The store stays `queued` until `adopt`, so the table keeps showing queued for the rest of the 30s. The task then ends canceled and is not transcribed.

## `95d9809` — PASS

No blocking issue. macOS was not stripped of `coreml`. `crates/humm-diarize/Cargo.toml` uses `speakrs.workspace = true` under `cfg(target_os = "macos")`, and the workspace dependency still enables `openblas-system` and `coreml` (`Cargo.toml`). `cargo +stable tree -p humm-diarize --target aarch64-apple-darwin -e features --invert speakrs` shows both features. The Linux build uses the `not(target_os = "macos")` dependency, `openblas-system` only, and the same tree on this host does not list `coreml`. `OrtDiarizer::diarize` still calls `ExecutionMode::Cpu`. That call was already CPU-only. This commit does not change it.

Bare `--identify-speakers` is no longer rejected. `validate_add_args` returns `None` for that flag and still rejects `--speaker-count` and `--speaker-diarizer` (`run.rs`). `execute` builds `OrtDiarizer` after `ensure_diarization_models` and stores it on `RunnerDeps.diarizer`. Every added task copies `TranscriptionOptions { identify_speakers: args.identify_speakers }`. `Runner::run_job` passes that diarizer into `run_file_job`. When the flag is set, `run_file_job` calls `identify_with_diarizer` on the PCM from `decoder.decode` (`pipeline.rs`). That function calls `Diarizer::diarize`. `mark_completed` and the export both use the returned segments. `cargo +stable test -p humm-cli --bin humm` — 41 passed. `identify_with_diarizer_prefixes_the_pcm_speakers` passed.

### Nonblocking

A diarize error fails the task. Buzz `identify_speakers` returns the original segments when identification fails (`speaker_identifier.py`), so the transcript is still exported. `SegmentAdded` events are emitted during `transcribe`, before the labels exist. The stored row and the export files are the labeled segments. The CLI progress callback passed to `diarize` is empty.
