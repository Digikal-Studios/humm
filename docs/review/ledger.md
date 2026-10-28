# Change and review ledger

Continuation state for the audit at `125a54da3d284ba3be651b4c99a4456ce56cd76a`. Worktree: `/workspace/.worktrees/humm-audit`, branch `audit/architecture-review`. Report: `docs/review/architecture-audit.md`.

All proposals are **design-unreviewed**. Do not implement from this file until the coordinator returns a reviewed set. Do not push this branch as part of the audit.

## Baseline commands

- Confirmed HEAD `125a54da3d284ba3be651b4c99a4456ce56cd76a`, subject `test: isolate remaining translated GUI assertions (#2)`, tracking `origin/main`. Working tree of `main` was clean. Review files exist only on this branch's worktree.
- `cargo-audit`: absent. Advisory source: RustSec DB `99670cfc0bc574433034b75876a4b791ccaa2efb` plus OSV (`api.osv.dev`) for the crates called out in the report and for Ubuntu ffmpeg.
- `cargo +stable fmt --check` in the worktree: exit 0. Toolchain `rustc 1.99.1 (58cf8f9c3 2026-10-13)`. Default toolchain 1.83.0 cannot parse edition 2024.
- Clippy, first attempt: exit 101, `openssl-sys` 0.9.117, `Package 'openssl' not found`. Environment package `libssl-dev` installed afterward. Retry: see the status line at the bottom of this file.
- `cargo test --workspace`: not run.
- `cargo bench`: not run.
- Host `ffmpeg` `6.1.1-3ubuntu5`. `yt-dlp` absent.
- Buzz `main` `298dbf003505cf945bdfd1eb8e705348b59a5b34` read from `/tmp/audit-sources/buzz` (not vendored). Tag `v1.4.5` is `aaf98634cd354be207e00cc5aaa1bfbd9e19cf22`. Compare is 35 commits ahead, 0 behind.
- Fixture blobs at that main commit match `docs/ci.md`: `ggml-tiny.bin` `1351aeba9b17d782f15baf46716c34dcfba0a30c` (575451 bytes), `whisper-latvian.wav` `d62d5937d0d62c748572d7bc3a35c88fa51db853` (230172 bytes).

## Buzz commits since the tag (not all are findings)

`35c381c4cb` Python upgrade (#1594); `56f1d49635` m4b/m4v/aac and folder drop (#1595); `9775971fad` French translation (#1598); `4c18d46d06` URL trailing-dot titles on Windows (#1602); `98b7993dce` Parakeet / MP4 (#1604); `2af05d4555` multi-file CLI (#1609); `710870dc61` demucs RAM (#1611); `f70a02fcd5` multiple custom Whisper.cpp models (#1614); `2d7d9dd795` per-segment plugin review reasons (#1600); `221659720d` folder watch and speaker-id option (#1617); `7d1929c588` unbundle CUDA (#1439); `81fdc26293` Orukeet on the HF backend (#1623); `a61bc18175` close shortcut (#1624); `d1c5646bd6` stream download progress (#1625); `1cc58f4dd1` speaker-id playback (#1627); `4e3eb6305a` recording transcriber (#1629); `838a191089` GPU docs (#1634); `8d0812a243` speaker-id CLI (#1640); `100589fd8c` concurrent speaker-id (#1641); `5082ca4e13` PyPI demucs (#1650); `851c8b3593` FunASR (#1651); `d05a48a7df` folder-watch unreadable path (#1653); `c02e63f60a` `/SUPPRESSMSGBOXES` (#1654); `c6c0a69571` resizer merge-by-gap (#1658); `f1b943ccd3` XDG secret-portal freeze (#1663); `053b388ae2` skip brackets (#1659); `6186d09a9f` timestamp fractions (#1660); `23d848b0b6` URL before speech extraction (#1667); `ee0118213c` CUDA prompt on deleted window (#1668); `efd6e2d796` lazy ML imports (#1669); `95c91e4d6d` Turkish (#1670); `bc2882b1d9` skip imports (#1672); `40f6a7812e` folder-watch duplicates on network shares (#1673); `eadf4ec352` malformed UTF-8 transcripts (#1674); `298dbf0035` preference window size (#1677).

Confirmed gaps: #1595, #1602, #1614, #1651, #1660, #1674. Confirmed already aligned: #1659 (literal prefix, not glob), #1667 (fetch then isolate). Not compared: the other subjects.

## Coverage map

- deep: architecture boundary, queue cancel and URL-then-isolate order, zip extract, timestamp parser, extension allowlist, media process groups, yt-dlp template, whisper `unsafe` and `append_word`, store migrate/poison/stale flip, CLI second SIGINT, RustSec lockfile match
- sampled: OpenAI cancel/translate, live session, GUI secret field and `open::that`, export filename construction
- tool-checked: `rtrb` call sites, `ureq` absence in humm sources, ort/whisper GitHub advisory endpoints, OSV ffmpeg, lockfile feature implications for brotli and http3
- unexamined: bench runs, ignored adapter contracts, `humm-audio` callback body, diarize/dfn numerics, most GUI view files, Buzz commits listed above as not compared

## Finding index

| ID | Category | Sev | Conf | Proposal |
|---|---|---|---|---|
| F-001 | demonstrated defect | high | high | P-001 |
| F-002 | demonstrated defect | high | high | P-002 |
| F-003 | demonstrated defect | low | high | P-002 |
| F-004 | demonstrated defect | medium | medium | P-002 |
| F-005 | demonstrated defect | medium | high | P-003 |
| F-006 | demonstrated defect | high | high | P-004 |
| F-007 | demonstrated defect | medium | high | P-005 |
| F-008 | demonstrated defect | medium | high | P-006 |
| F-009 | demonstrated defect | medium | high (code) / medium (frequency) | P-007 |
| F-010 | architectural concern | medium | high | P-008 |
| F-011 | architectural concern | medium | high | P-009 |
| F-012 | architectural concern | low | high | P-010 |
| F-013 | demonstrated defect (host ffmpeg) | medium | medium | P-011 |
| F-014 | demonstrated defect | medium | high | P-012 |

## Proposals

Review `a329e4e` accepted P-001, P-002, P-004, P-005, P-009, and P-011. Those are implemented on this branch except P-011, which is a non-change. P-003, P-006, P-007, P-010, and P-012 are revised below and not implemented. P-008 stays deferred, with the review's constraint added. None of the revised text has been re-reviewed.

### P-001 — confine zip 8.6.0 symlink extraction (implemented)

- Implemented in `e6637e08483ff8c00669fa1874cc40f41b6bcb00`. zip stays 8.6.0. `extract_zip_archive` does not call `ZipArchive::extract`; an escaping symlink is not created, and archive unix modes are not applied. `place_coreml_contents` uses `symlink_metadata` and refuses a link whose target leaves the extract directory, including before `remove_dir_all` and the sentinel write.
- Tests: `extract_zip_archive_does_not_create_an_escaping_symlink`, `place_coreml_contents_does_not_follow_an_escaping_symlink`. `cargo test -p humm-core --lib models::download::` — 43 passed.

### P-002 — patch the rustls trust path (implemented)

- Implemented in `8e2244670c6a4692094f6584cb584e0a6a501628`. Lockfile: `rustls` 0.23.46, `rustls-webpki` 0.103.16, `webpki-roots` 1.0.10. `ureq` stayed 3.3.0. `webpki-root-certs` stayed 1.0.9.
- `cargo tree -p humm-core -i rustls -i rustls-webpki -i webpki-roots` shows those versions on the reqwest path. `cargo test -p humm-core --lib update::` — 18 passed. `models::download::` — 43 passed. Those tests use plain HTTP and do not prove the emSign root is gone.

### P-003 — second Ctrl-C: groups, temp files, and a non-blocking handler (revised, not implemented)

- Objective: a second SIGINT stops demucs, yt-dlp, and their grandchildren, and does not leave `_speech` / denoised temp files or the demucs scratch directory behind.
- Findings: F-005
- Why the previous design is not enough: signaling the process group and then calling `std::process::exit(130)` still skips `Drop`. `TempAudioGuard` deletes those files only in `Drop`. `DemucsIsolator`'s `tempfile::TempDir` is the same class of leak. A test that only checks the grandchild is dead passes with the files still on disk.
- Design for the next review: before `process::exit`, delete the temp paths this process still owns. Do that explicitly. Do not rely on `Drop` after `exit`. Record process-group ids in something the `ctrlc` thread can read without waiting (`Atomic` slots, or `try_lock` and exit 130 if the lock is busy). Do not take a mutex the worker holds across `spawn`; that turns the second Ctrl-C into a stuck handler, which is worse than today's exit. Signal only ids whose `Child` has not been `wait`ed. Do not join the children. The first signal stays `cancel_all`. `decode.rs`, `encode.rs`, and `video.rs` stay a direct `child.kill()`; those commands are a single ffmpeg, not a shell.
- Contracts: CLI exit code 130. First Ctrl-C still cancels cooperatively.
- Alternatives: leave ADR-023's debt, including the temp files.
- Risks: a recycled pgid, and deleting a path that is no longer the job's temp file. Only paths this process created, and only groups it has not reaped.
- Acceptance: a stub grandchild is dead after the second SIGINT. `_speech` and the demucs scratch directory are gone. The denoised file is gone only when `keep_denoised_file` is false; when that setting is true the file the user asked to keep is still there (`TempAudioGuard` / `denoised_output_kept_when_keep_denoised_file_set`). Do not register the user's source path. `SIGKILL` the group before deleting, so a live child cannot recreate the file, and do not wait for it. Temp-path deletion is not unix-only. When the registry lock is busy, exit 130 without cleanup rather than blocking.
- Verification: `humm-media` grandchild unit tests; a temp-file assertion for `_speech`, the scratch dir, and both values of `keep_denoised_file`; a test that the handler does not wait on the worker's mutex.

### P-004 — parse timestamp fractions as Buzz main does (implemented)

- Implemented in `423ec1fd2160dedc2579469c5fcee6f08fe74db9`. `1.2` is 1200 ms, `1.2.3` is 1200 ms (fraction `2` padded to 200, plus one second), `1.` is 1000 ms. `to_timestamp` is unchanged.
- `cargo test -p humm-core --lib viewer::timestamp` — 8 passed.

### P-005 — allow m4b, aac, and m4v (implemented)

- Implemented in `4c1ac2b4d1670f9440d7d0683e7ff648061f3576`. All three handwritten arrays in `folderwatch.rs` gained the suffixes, in Buzz's order. `SUPPORTED_EXTENSIONS` is still handwritten, not derived. `humm-gui` `import.rs` `supported_extensions_match_buzz_main_lowercase_no_dot` asserts all three. `viewer.rs` `is_video` was not given a fourth list.
- `cargo test -p humm-core --lib folderwatch::` — 18 passed, including `m4b_aac_and_m4v_are_supported`. `cargo test -p humm-gui --lib import::tests::supported_extensions_match_buzz_main_lowercase_no_dot` did not run: `speakrs` 0.5.0 `coreml` does not compile on Linux. `coreml` was not disabled.

### P-006 — URL download name, GUI label, and export stem (revised, not implemented)

- Objective: the on-disk download name is not the media title, and the title still reaches the GUI label and the export stem the way Buzz uses `display_name`.
- Findings: F-008
- Why the previous design is not enough: `source.%(ext)s` is the right template, and `media_dir/<id>/` already keeps tasks from colliding. humm does not have Buzz's `display_name`. `create_task` inserts URL rows with `name` NULL. `display_name_for` is `name`, else the file basename, else the URL, so after `update_file` the GUI label is the downloaded filename. Changing only the template makes that label `source.<ext>` and makes `{{ input_file_name }}` export as `source`, because `output_file_path` uses `job.input`'s stem. The CLI label is `Input::display_name`, which for a URL is the URL string, not the title. Keeping the CLI label is not a title regression. `fetch` treats the last non-progress stdout line as the path. A second `--print` of the title overwrites that path unless the parser changes.
- Design for the next review: one machine-readable yt-dlp record carries path and title. Do not add a second `--print` that shares the path line. Store the raw title with `set_task_name` (or an equivalent `TaskStore` method; `set_task_name` is not on the trait today) so the GUI label is the title. The CLI label stays the URL. For a URL task, `{{ input_file_name }}` uses that title only after the same sanitizing Buzz applies in `safe_filename_component` (`paths.py`): `/` and `\` become `_`, and a trailing space or dot becomes `#`. `output_file_path` must not `join` the raw title. `check_skip` / `stem_for` still sees `job.input`, which will be `source.<ext>`, so skip looks for `source*` and will not see a title-named export. Buzz has the same split (`audio.wav` versus `display_name`). Leave skip on the file stem unless a later change points it at the sanitized title. Keep `--no-playlist`, cookies, and the process-group kill.
- Contracts: `MediaFetcher::fetch`, task `name` (raw title), GUI `display_name_for`, CLI URL label (stays the URL), `output_file_path` for URL jobs (sanitized stem only).
- Alternatives: keep `%(title)s.%(ext)s` and accept the Windows trailing-dot failure.
- Risks: export names of existing URL tasks change only for new downloads. A sanitizer that is not applied before `Path::join` writes outside the output directory.
- Acceptance: a title ending in `.` or ` ` does not create that download path; the GUI label is the raw title; the CLI progress label remains the URL; stdout still yields exactly one path line. A title containing `/` or `..` (for example `AC/DC` or `../x`) does not add a path component: the export file stays inside the output directory, and the stem has those characters replaced the way `safe_filename_component` does. Cancel still kills the process group.
- Verification: `humm-media` fetch unit tests with a stub that prints one path line, plus a queue or pipeline test whose title contains `/` or `..` and whose export path is still inside the output directory.

### P-007 — final-flush UTF-8 prefix only (implemented)

- Implemented in `89489dac2ff22c8fc3543cc2ccdafb51faddbe73`. The fallback is only the final flush: lossy-decode, `trim_end_matches('\u{FFFD}')`, then `trim`. Mid-loop flushes still drop an invalid buffer. `flush_complete_chars` is unchanged.
- `cargo test -p humm-whisper --lib words::` — 8 passed, including `final_flush_strips_only_a_trailing_replacement_character` (`hello\xff` → `hello`, `\xffhello` → `\u{FFFD}hello`, and `hello\xff` plus ` world` → only `world`).

### P-008 — more than one custom ggml model (deferred, not implemented)

- Still deferred. Buzz `whisper_cpp_custom_models.py` is a JSON list plus legacy id `legacy-custom` and filename `ggml-model-whisper-custom.bin`. humm's slot is `ggml-custom.bin`, which is this project's file, not that legacy name. Do not rename the cache file to Buzz's name; that orphans existing downloads. A later design has to re-read the Python module. The `{id, name, path}` sketch dropped `source_url`. ADR-008 still describes one custom URL slot. Not part of the CVE or extension edits.

### P-009 — FunASR named exception (implemented)

- Implemented in `66613c07a5516e0cc0209176e45b62ca58eed29d`. `docs/OPEN_QUESTIONS.md` Q-2 and ADR-008 name FunASR as out of scope against Buzz `main` `298dbf003505cf945bdfd1eb8e705348b59a5b34` (`is_funasr_model`, commit `851c8b3593`). No engine, crate, or dependency was added.

### P-010 — say what the behavior pin is, without restamping the inventory (implemented)

- Implemented in `bd2c7d720c12dc4ac745189b34cc64fde938b623`. `FEATURE_INVENTORY.md` and `EXTERNAL_SURFACE.md` keep their v1.4.5 ground-truth lines. A following sentence names Buzz `main` `298dbf003505cf945bdfd1eb8e705348b59a5b34` as the behavior target and says those bodies were not re-read against it. Feature rows are unchanged. `scripts/ci_prepare.py` was not edited.

### P-011 — do not change humm to clear host ffmpeg CVEs (accepted, no code)

- No `crates/humm-media` diff. `UBUNTU-CVE-2024-32228` and `UBUNTU-CVE-2025-59734` stay recorded as unconfirmed host advisories for ffmpeg `6.1.1-3ubuntu5`. The 2024 summary names FFmpeg 7.0. Neither bug was reproduced. F-013 is not a demonstrated defect on this build. An argv or codec allowlist is still rejected.

### P-012 — fail closed on a digest lookup error, without a re-download loop (revised, not implemented)

- Objective: do not promote a model file when its digest lookup fails, and do not delete a complete partial and download it again on the next attempt.
- Findings: F-014
- Why the previous design is not enough: `verify_or_warn` turns both `Ok(None)` and `Err` into success, then `download_body` renames the partial. The M1 plan documented the skip: "If the API call fails, log a warning and skip verification" (`docs/superpowers/plans/2026-07-20-m1-headless-core-path.md`). Reversing that is a deliberate change and has to be named as one. `verify_partial_is_complete` maps `Err` to `Ok(None)`, and `on_range_not_satisfiable` treats `None` as `RestartFresh` (delete the partial, download again). After a fail-closed first attempt the partial is still there, the next `ensure_artifact` sends `Range: bytes=<len>-`, a 416 comes back, the lookup fails again, and the whole file is fetched again. A test that only checks the first 500 does not catch that. `find_sha256` uses `serde_json::from_str(...).ok()?`, so HTTP 200 with a non-JSON body is `Ok(None)` and would still promote. `coreml_failure_action` maps every `ModelError::Http` to degrade: a tree-API 500 on the encoder zip does not fail `resolve_model`. ggml and VAD do fail the ensure. A diarization sidecar with no `lfs.oid` must still promote. Custom URLs pass `sha_repo: None` and never enter `verify_or_warn`.
- Design for the next review: lookup `Err` fails that ensure and does not delete a complete partial. `verify_partial_is_complete` must not treat that same `Err` as "restart fresh". `Ok(None)` after a parsed tree that lacks `lfs.oid` still promotes. A 200 body that does not parse is `Err`, not `Ok(None)`. Say explicitly that CoreML HTTP errors still degrade and that this does not fail the ggml ensure. Record the reversal of the M1 skip.
- Contracts: `ensure_artifact`, the 416 recovery arm, `find_sha256`, `coreml_failure_action`.
- Alternatives: keep warn-and-accept, matching the M1 plan, so a digest outage never blocks or re-downloads.
- Risks: a tree-API blip fails the ggml or VAD ensure until the lookup works. The partial must still be reusable.
- Acceptance: a digest lookup error does not rename the partial and does not download the whole file again. A second `ensure_artifact` may send `Range: bytes=<len>-` and may receive 416; that probe is allowed. 416 must not take the `RestartFresh` arm (no delete of the partial, no second non-ranged body GET). When a later lookup returns `Some(true)`, that same partial is still promoted. A parsed tree with no `lfs.oid` still promotes. Malformed JSON on HTTP 200 does not promote. A CoreML encoder digest failure still degrades rather than failing the ggml model. A mismatched digest still deletes the partial.
- Verification: replace `ensure_coreml_encoder_416_after_restart_errors_without_looping`'s delete-and-re-download assertion. The 416 case is one ranged probe, no second body download, and the partial still present. The double-call 500 case must not count every file-URL GET as a failure: assert the partial's bytes are unchanged and that no non-ranged GET follows the 416, not "a single file GET". `cargo test -p humm-core --lib models::download` covers those rewritten tests. Do not require the old 416 test to keep passing unchanged.

## Evidence reconciled

Read-only notes from the later passes live in the project store (`internal/audit/architecture-audit-evidence-2026-10-28.md`, `internal/architecture-audit-evidence-2026-10-28.md`). F-005 now includes the `TempAudioGuard` leak. F-014 / P-012 is the only new proposal. ADR-018, ADR-022 detach, CLI `reducer: None`, and the GUI adapter wiring were confirmed and left as documented policy.

## Open questions

- FunASR: in scope or a named exception? P-009. Blocked on review, not on code.
- Multiple custom models: implement P-008 or write the exception? Not blocked for the CVE and timestamp work.
- Second Ctrl-C orphans: fix (P-003) or keep ADR-023's debt?
- `UBUNTU-CVE-2024-32228` on ffmpeg 6.1.1 specifically: tracker says the Ubuntu 24.04 package is affected; the summary names 7.0. Not reproduced.
- Uncompared Buzz commits in the list above. Do not file them as gaps without reading both sides.
- Clippy on this Linux VM stops at `speakrs`'s macOS-only `coreml` feature (ledger, Clippy status). Stable fmt passed. Do not disable `coreml` to get a green Linux log.

## Style notes kept out of the finding list

`panic::catch_unwind` in the queue stays. The style guide discourages it; humm uses it so one task panic cannot kill the runner, which `ARCHITECTURE.md` requires. No `unsafe` expansion is proposed. No formatting churn.

## Clippy status

`cargo +stable clippy --workspace --all-targets -- -D warnings`, rustc 1.99.1, `CARGO_BUILD_JOBS=2`, worktree target dir:

1. Exit 101. `openssl-sys` 0.9.117: no `openssl.pc`. `libssl-dev` installed in the VM only.
2. Exit 101. `whisper-rs-sys` 0.15.0 cmake: `/usr/bin/ld: cannot find -lstdc++`. `g++` installed in the VM only.
3. Exit 101. `alsa-sys` 0.4.0: no `alsa.pc` (cpal on Linux; CI is macOS). `libasound2-dev` and `libopenblas-dev` installed in the VM only.
4. Exit 101 after `libasound2-dev` and `libopenblas-dev`. `speakrs` 0.5.0: `the coreml feature is only supported on macOS` (`speakrs-0.5.0/src/lib.rs:204`). The workspace Cargo.toml turns that feature on (ADR-024). Further Linux retries would require disabling it. Not done.

`cargo test --workspace` was not started. Do not treat the Linux stop as a product failure and do not drop `coreml` to make this VM pass.
