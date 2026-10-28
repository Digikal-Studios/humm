# Cross-change review 2

Reviewed the combined product tree `0d2de407246a7b214215c1c2596c67e640507a88` against the previous combined PASS `1bfd98699dd015ca02c57266d75779a28e905f07` on `audit/architecture-review`. Worktree: `/workspace/.worktrees/humm-audit`. No product source was edited.

HEAD while this was written is `b3e8099a4cbb1e455cee9b9ce41f070fdb2ba6c1`. Two commits sit after the product tip, and both stay in `docs/review`:

- `0d9978ef435517b8990a196ab260d327870fe7c2` changes `docs/review/patch-review-watch-skip-cli.md`
- `b3e8099a4cbb1e455cee9b9ce41f070fdb2ba6c1` changes `docs/review/ledger.md`

`git diff 0d2de407 HEAD -- . ':(exclude)docs/review'` is empty. `0d2de407` is the product tip.

## Verdict

**PASS** for `0d2de407246a7b214215c1c2596c67e640507a88`.

No substantiated blocking interaction. Each passed fix is the tree that was passed. None of them undoes another, and none of them undoes the combined PASS at `1bfd986`. The superseded SHAs stay **REQUEST_CHANGES** and are not the effective code.

## Presence

Product commits after `1bfd986`, in order: `4c2de50`, `4a7333c`, `dae4379`, `c083075`, `fd7f3aa`, `d00bab7`, `561aa9b`, `95d9809`, `0d2de40`. Docs commits in that range do not change product code.

For each passed SHA below, the files that SHA introduced are unchanged at `0d2de407` (`git diff <sha> 0d2de407 -- <path>` empty), except where a later passed commit edited the same file. Those later edits are listed and do not remove the earlier behavior.

| Fix | Passed SHA | Still that tree |
|---|---|---|
| P-006 URL titles | `4c2de50c0e7d555a3dbfd0f95c0f5213d17fc90c` | `fetch.rs`, `export/mod.rs` match the SHA. Later edits to `queue.rs`, `pipeline.rs`, `domain.rs`, `store.rs`, and `export_menu.rs` still store the raw title, keep the CLI label as `Input::display_name`, and pass `display_name` into `output_file_path` |
| P-012 model download | `4a7333cf4164fcd0c6f33df9b0fa37b4632e6362` | `crates/humm-core/src/models/download.rs` matches the SHA. Nothing after it edits that file |
| Trailing U+FFFD | `dae43793b8c371120291bf35e4fb7aa013b76935` | `crates/humm-whisper/src/engine.rs` matches the SHA |
| URL skip | `fd7f3aaba96c9a8e62c35f9a3398b7f888f79265` | `skip.rs` and `boundary.rs` match the SHA. Later `queue.rs` still passes `Some(url)` only for `Input::Url`. Later `store.rs` only adds `has_blocking_task_for_file` and test-struct fields |
| CLI `--identify-speakers` | `95d980990f916b12129080b68e64d0139703e253` | `humm-cli`, `diarize.rs`, `pipeline.rs`, and `humm-diarize` match the SHA. `0d2de40` does not touch them |
| Folder watch settle | `0d2de407246a7b214215c1c2596c67e640507a88` | This is the product tip |

`d00bab7d9e00f052562e61e875b9a5d689ef1d57` stays **REQUEST_CHANGES**. It is an ancestor. The effective CLI is `95d9809`: `validate_add_args` returns `None` for bare `--identify-speakers` and still rejects `--speaker-count` and `--speaker-diarizer` (`run.rs`). `execute` builds `OrtDiarizer` after `ensure_diarization_models` and stores it on `RunnerDeps.diarizer`. `run_file_job` calls `identify_with_diarizer` when `identify_speakers` is set (`pipeline.rs`).

`c0830754f7e3ffbe30ce52f7947e13cc931c44e5` and `561aa9b7a78baeb631d0b34481aae58d18882893` stay **REQUEST_CHANGES**. They are ancestors. The effective watcher is `0d2de40`: `poll_settled` puts a settled path back into `SettleSet` when `get_task` is missing or errors, with `since` already 30s in the past, and it does not record `SeenSet` on that arm (`folder_watch.rs`). `seen.record` runs only after that row exists.

The earlier combined PASS is still the tree, on the paths this range did not have to edit:

- P-001. `extract_zip_archive` is not in the `download.rs` diff. It still does not call `ZipArchive::extract`. Direct `zip` stays 8.6.0. The transitive `zip` 6.0.0 entry is still not the extract path.
- P-002. Lockfile: `rustls` 0.23.46, `rustls-webpki` 0.103.16, `webpki-roots` 1.0.10. The only `Cargo.lock` change in this range is `humm-cli` depending on `humm-diarize`.
- P-003. `install_ctrlc` still calls `try_emergency_stop` and then `process::exit(130)` (`run.rs`). `fetch.rs` still calls `spawn_own_process_group` and `track_process` after spawn. `queue.rs` still registers a new isolated file and a denoised file with `keep_denoised`. `abort.rs` and `demucs.rs` are not in the diff.
- P-004. `timestamp.rs` is not in the diff.
- P-005. The three extension arrays are still the 8 + 8 Buzz suffixes, including `m4b`, `aac`, and `m4v`.
- P-007. `words.rs` matches `89489dac2ff22c8fc3543cc2ccdafb51faddbe73`. `push_lossy_final_word` still strips a trailing U+FFFD and then trims. `segment_text` is the separate path from `dae4379`.
- P-009 and P-010. `OPEN_QUESTIONS.md`, `DECISIONS.md`, `FEATURE_INVENTORY.md`, and `EXTERNAL_SURFACE.md` are not in the product diff.

## Interactions

P-006 does not undo P-003. The fetch rewrite still registers the yt-dlp process group after spawn, and the title record is the one `--print`. `output_file_path` still runs the title through `safe_filename_component` and `confine_export_file_name` before the export is written. Speaker labeling in `run_file_job` happens before that export and does not replace `display_name`.

URL skip does not undo P-006. `check` returns on `source_url` before result-file search (`skip.rs`). `run_job` passes the URL only for `Input::Url`, after `fetch_url` has stored the title and turned the path into `source.<ext>`. A skip hit returns before `run_file_job`, so that task is not diarized and is not exported again. That is the skip contract, not the CLI flag leaving the process.

The bare `--identify-speakers` flag does not exit before the diarizer. `validate_add_args` does not reject it. `ensure_cli_diarizer` runs inside `execute`, and `run_file_job` receives `self.diarizer`. `--speaker-count` and `--speaker-diarizer` still return `usage_fail` before `execute`. That rejection is the passed `95d9809` behavior. A diarization-model download error still fails the run before the queue starts; `95d9809` already accepted that, and the download goes through `ensure_artifact`.

Folder watch does not mark a path seen before its row exists. `scan` still does not update `SeenSet`. `rescan_and_enqueue` stages and does not record. `poll_settled` records only after `get_task` returns a row. A missing row is reinserted with the 30s clock already elapsed, so the next poll can adopt it. `has_blocking_task_for_file` still ignores `canceled`. A file that disappears during the wait is `Abandon`, not `seen.record`.

P-012 does not reintroduce delete-on-every-416. `recover_from_range_not_satisfiable` uses `?` on `verify_partial_is_complete` before `remove_and_warn` or `download_fresh`. `verify_or_warn` returns the lookup `Err` before the rename. `find_sha256` returns `Err` for a body that is not a JSON array. `on_range_not_satisfiable` still maps `Some(false)` and `None` to `RestartFresh`, which is the split `4a7333c` passed. `ensure_diarization_models` calls that same `ensure_artifact`. `coreml_failure_action` still degrades only on `ModelError::Http`.

CoreML is not dropped on macOS. The workspace `speakrs` features are still `openblas-system` and `coreml`. `humm-diarize` uses `speakrs.workspace = true` under `cfg(target_os = "macos")` and the openblas-only pin under `cfg(not(target_os = "macos"))`. `humm-whisper` still enables `whisper-rs` `metal` and `coreml`. The lockfile `speakrs` 0.5.0 entry still depends on `objc2-core-ml`. No file after `95d9809` edits those manifests.

## Deferred, not implemented

- P-008. `custom_model_file_name` is still `ggml-custom.bin` (`registry.rs`). That file is not in the product diff. It was not renamed to `ggml-model-whisper-custom.bin`.
- FunASR. No engine, crate, or dependency was added. The named exception from `66613c0` is untouched.
- MSDD and Sortformer. The CLI can parse `msdd` and `sortformer` and then rejects them. `Diarizer::diarize` still takes PCM only. `OrtDiarizer` still calls the speakrs pipeline. No NeMo engine was added.
