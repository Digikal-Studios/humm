# Patch review: deferred P-006, P-012, and segment text

Reviewed only these SHAs on `audit/architecture-review`, via `git show`. Buzz was read from `/tmp/audit-sources/buzz` at `298dbf003505cf945bdfd1eb8e705348b59a5b34`. No product source was edited.

| ID | SHA | Verdict |
|---|---|---|
| P-006 | `4c2de50c0e7d555a3dbfd0f95c0f5213d17fc90c` | **PASS** |
| P-012 | `4a7333cf4164fcd0c6f33df9b0fa37b4632e6362` | **PASS** |
| Trailing U+FFFD | `dae43793b8c371120291bf35e4fb7aa013b76935` | **PASS** |

Re-ran on this tree with `cargo +stable`: `humm-core --lib models::download::` 49 passed; the export, queue title, and pipeline title filters passed with them; `humm-whisper --lib` 38 passed; `humm-media --lib fetch::` 15 passed. GUI was not compiled (`speakrs` `coreml` still does not build on this Linux host).

## P-006 — PASS `4c2de50c0e7d555a3dbfd0f95c0f5213d17fc90c`

The download template is `dest_dir.join("source.%(ext)s")` (`fetch.rs`). One `--print` is `after_move:humm-result:{"path":%(filepath)j,"title":%(title)j}`. yt-dlp splits that on the first `WHEN` colon (`options.py` `_dict_from_options_callback`), and the rest does not match the bare-field shorthand in `_forceprint`, so it is a real output template. `%(field)j` is `json.dumps`, which keeps a title with quotes or newlines on one line. `parse_fetch_record` accepts only a `humm-result:` JSON object, so a second plain `--print` cannot replace the path. The stub test asserts a single `--print` and `source.%(ext)s`.

`set_task_name` stores the raw title. `display_name_for` prefers that column, so the GUI label is the title. The CLI label map is filled from `Input::display_name` at enqueue (`run.rs`) and is not updated from the store. For a URL that is the URL string.

`output_file_path` runs the title or the file stem through `safe_filename_component` before the template, then `confine_export_file_name` on the finished name. Against `paths.py` in this Buzz tree, the function matches the cases that were deferred: `C:/x` → `C__x`, `CON` → `_CON`, `..` → `.#`, one trailing space or dot → `#`, and `CON`/`PRN`/`AUX`/`NUL`/`COM1`–`COM9`/`LPT1`–`LPT9` on the stem before the first dot. A name that still contains `:`, `/`, `\`, or a reserved stem is sanitized again, so `Path::join` cannot see a Windows disk prefix. The pipeline test writes `C__x.srt`, `.#.srt`, and `_CON.srt` inside the output directory. Skip still uses `job.input` (`source.<ext>`), which is the written design.

### Nonblocking

`url_task_stores_the_raw_title_and_keeps_the_url_as_the_cli_label` asserts `Input::display_name` before enqueue. It does not render a CLI progress line. The label cannot pick up the title unless `run.rs` starts reading the store name.

`Started` is emitted before `fetch_url`, and download progress does not re-read the row. The table shows the URL until the terminal refresh. The stored name is already the raw title by then.

The parent assertion runs on Linux, where `C:` is not a prefix. The tests also reject `:` in the finished name, which is the prefix Windows `Path` uses.

## P-012 — PASS `4a7333cf4164fcd0c6f33df9b0fa37b4632e6362`

`verify_partial_is_complete` no longer maps `Err` to `Ok(None)`. `recover_from_range_not_satisfiable` uses `?` before `remove_and_warn` or `download_fresh`. `verify_or_warn` returns that `Err` before the rename. `find_sha256` returns `Err` for a body that is not a JSON array; a parsed entry with no `lfs` is still `Ok(None)`.

`on_range_not_satisfiable` still maps `Some(false)` and `None` to `RestartFresh`. That is the split the last review asked for. `Ok(None)` (no `sha_repo`, or a parsed tree with no `lfs.oid`) can delete once and finish with one non-ranged GET. `ensure_custom_model_416_without_a_digest_downloads_fresh_and_finishes` covers the no-repo case. A mismatch still deletes (`ensure_ggml_checksum_mismatch_deletes_the_partial`).

`ensure_coreml_encoder_416_on_digest_lookup_error_keeps_the_partial` is two ranged probes, unchanged partial bytes, then promotion of that same partial when the digest matches. No non-ranged body GET. The old `ensure_coreml_encoder_416_after_restart_errors_without_looping` test is not in the crate.

`coreml_failure_action` is unchanged: only `ModelError::Http` degrades. A tree-API 500 is `Http`, so `resolve_model_coreml_digest_lookup_error_degrades` keeps the local ggml and sets `coreml_degraded`. `ChecksumMismatch` still propagates. That is the degrade rule the last review allowed. The M1 "log and skip" reversal is named on `verify_or_warn`. A ggml or VAD lookup error still fails that ensure.

### Nonblocking

A 416 whose parsed tree has no `lfs.oid` takes the same `RestartFresh` arm as `sha_repo: None`. There is no separate integration test for that tree shape on 416. `on_range_not_satisfiable(None)` and the no-oid promote path are both tested.

## Trailing U+FFFD — PASS `dae43793b8c371120291bf35e4fb7aa013b76935`

Buzz `_parse_segment_timings` (`whisper_cpp.py` 398–411) lossy-decodes, then `rstrip("\ufffd").strip()`. `segment_text` is `trim_end_matches('\u{FFFD}')` then `trim`. `Bridge::on_segment` uses it after the `set_segment_callback_safe_lossy` callback, and only when `word_mode` is false. `join_segment_texts` uses it for the live `to_str_lossy` join. The test keeps a leading U+FFFD, strips a trailing run, and leaves `hello\u{FFFD} ` as `hello\u{FFFD}` because the space hides the replacement until after the strip. That is Buzz's order.

`89489dac2ff22c8fc3543cc2ccdafb51faddbe73` is still an ancestor. `crates/humm-whisper/src/words.rs` has the same blob as in that commit (`de0d11d6fa855e964bc36b7caebed9c6f2d8e8f2`). `emit_word_segments` still calls `group_words` only. The final-flush word path was not edited.

### Nonblocking

A file segment whose text is only U+FFFD becomes empty and is still passed to the sink. Buzz skips an empty `segment_text`. The live join already drops empty pieces. Whitespace-only segments were already emitted empty before this commit.
