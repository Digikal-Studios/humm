# Adversarial review of the architecture proposals

Reviewed tree: `audit/architecture-review` at `07f0e2c323333d0c8bc5c598bafa370fcbbb376a`. Product code read at `125a54da3d284ba3be651b4c99a4456ce56cd76a`. Buzz read at `/tmp/audit-sources/buzz` `298dbf003505cf945bdfd1eb8e705348b59a5b34`. No product file was edited. Nothing here is an approval to implement past the disposition below.

Dispositions: **accept**, **revise**, **reject**, or **defer**. A revise means the problem is real and the written design should not be implemented unchanged.

## Dispositions

| ID | Disposition | Objection class |
|---|---|---|
| P-001 | accept | no material objection |
| P-002 | accept | no material objection |
| P-003 | revise | blocking against amended F-005 |
| P-004 | accept | no material objection |
| P-005 | accept | no material objection |
| P-006 | revise | blocking |
| P-007 | revise | blocking against the Buzz citation |
| P-008 | defer | no material objection to deferring |
| P-009 | accept | no material objection; exception only |
| P-010 | revise | blocking |
| P-011 | accept | no material objection |
| P-012 | revise | blocking |

## P-001 — accept

No material objection. `zip` 8.6.0 `ZipArchive::extract` does create a symlink whose target is attacker-controlled.

`make_symlink` in `zip` 8.6.0 (`read.rs`) UTF-8-checks the target and then calls `std::os::unix::fs::symlink` with those bytes. It does not confine the target. The crate's own `test_cannot_symlink_outside_destination` only rejects a *later file write* through an already extracted link (`symlink/dest-file`). A link entry itself is still created. `RUSTSEC-2026-0368` patched `>= 9.0.0` matches that. `ndarray-npy` 0.10.0 opens `ZipArchive` to read entries; it never calls `extract`. Leaving zip 6.0.0 alone is right.

`place_coreml_contents` (`download.rs` around the `is_dir` / `rename` / sentinel `write`) makes the link useful: `Path::is_dir` follows, `rename` of the link moves the link, and `std::fs::write` of the sentinel follows it. That is a concrete write outside the extract dir, not only a planted link.

`reqwest` is not involved in this bump. Workspace `zip = "8.6.0"` is `^8.6.0`, so 9.x requires a requirement change, as proposed. SUID/SGID (`RUSTSEC-2026-0367`) is the same bump and is not a separate behavior change for an unprivileged user.

Nonblocking: the new test has to assert the outside path was not created, not only that `extract` returned `Err`. `ZipWriter` / `FileOptions` in `build_test_zip` may not compile on zip 9; the existing extract tests are the right gate. Pin `>=9.0.0, <10` rather than an open `>=9`.

## P-002 — accept

No material objection. Lockfile versions match the advisories: `webpki-roots` 1.0.9, `rustls-webpki` 0.103.13, `rustls` 0.23.42. `reqwest` 0.12.28 allows `webpki-roots` `^1`, `rustls` `^0.23.4`, and `rustls-webpki` `^0.103.5` (via rustls). `1.0.10`, `0.103.16`, and `0.23.46` fit without a reqwest major bump. The workspace client is `default-features = false` plus `rustls-tls`, and `ModelDownloader` uses `reqwest::blocking::Client::new()`.

`ureq` 3.3.0 is in the lockfile because `ort` / `ort-sys` declare it. Runtime `ureq` in `ort` is behind `fetch-models`, which this workspace does not enable. `ort-sys`'s build download, with `tls-native`, uses `RootCerts::PlatformVerifier`, not the webpki bundle. Not updating `ureq` is correct. `cargo update -p webpki-roots` may still move the single shared 1.0.9 row if `ureq`'s optional dep allows it. That is fine.

`ClientConfig::builder_with_provider(...).with_no_client_auth()` does not disable resumption, so F-004's session store is the default client, not a custom store humm would have to opt into. Bundling `rustls >= 0.23.46` is the smaller change. F-003's impact is as narrow as the advisory says; it is not a reason to split the bump.

Nonblocking: `cargo test -p humm-core --lib` talks to `TestServer` over plain HTTP. It cannot show that the emSign root is gone. The `cargo tree` check is the verification that matters. A duplicate older `webpki-roots` outside `humm-core` would not show up in `-p humm-core`.

## P-003 — revise

Blocking against the amended finding. The process-group signal is the right fix for orphaned demucs and yt-dlp. The written design does not close the temp-file half of amended F-005, and a straightforward registry can make the second Ctrl-C hang.

`install_ctrlc` (`crates/humm-cli/src/run.rs`) calls `std::process::exit(130)` on the second SIGINT. `std::process::exit` skips `Drop` on every thread, not only the handler. `TempAudioGuard` (`crates/humm-core/src/queue.rs`) deletes `_speech` and `_DeepFilterNet3` files only in `Drop`. Signaling the process group and then exiting still leaves those files beside the user's media. `DemucsIsolator`'s `tempfile::TempDir` scratch is the same class of leak while isolation is in flight. Acceptance as written ("grandchild is dead") passes with the files still on disk.

The handler already runs on the `ctrlc` thread and, on the *first* signal, locks `shared`. A second-signal path that locks a mutex the worker holds across `spawn` turns "exit immediately" into a stuck handler: the process keeps running, which is worse than today's exit. Record pgids in something the handler can read without waiting (`Atomic` slots, or `try_lock` and exit 130 anyway). Signal only ids whose `Child` has not been `wait`ed; after `wait`, the pid can be reused. The proposal already says this. It has to be true in the synchronization, not only in the comment.

Nonblocking: `decode.rs`, `encode.rs`, and `video.rs` kill the direct child only. Their commands are `Command::new(ffmpeg)`, not a shell. Leaving them out of this proposal is fair. Do not join the children. Keep the first signal as `cancel_all`.

Uncertain: a grandchild that calls `setsid` would leave the group. Not shown for demucs or yt-dlp's ffmpeg.

## P-004 — accept

No material objection. The diagnosis matches the code and Buzz `main`.

`parse_timestamp` (`crates/humm-core/src/viewer/timestamp.rs`) parses the fractional group with `parse::<i64>()`. `1.2` becomes milliseconds `2` (total 1002). `1.45` becomes `45` (total 1045). Buzz `transcription_segments_editor_widget.py` lines 88–89 do `int(fraction[:3].ljust(3, "0"))`, so those inputs are 1200 and 1450. `to_timestamp` always prints three digits (`export/mod.rs`), so `round_trips_with_to_timestamp` never sees a short fraction. Callers are the viewer editors (`humm-gui/src/viewer.rs`, `views/viewer_table.rs`); stored rows are integer milliseconds, so old transcripts do not move until the cell is edited.

Nonblocking: the stated acceptance list does not include cases the formula changes and the current tests freeze.

- `1.2.3` is 1200 ms total (fraction `2` padded to `200`), not 200 ms total, and not today's `Millis(1_002)`. `trailing_extra_fraction_groups_are_ignored_not_rejected` will fail until it is updated. The proposal's "→ 200 ms" is easy to implement as the wrong total.
- `1.` is `""` padded to `"000"` in Python (1000 ms). Today `"".parse::<i64>()` is `None`. The listed cases (`1.2`, `1.45`, `1.4567`, `01:02:03.456`) all pass if the empty fraction is left as `None`.

Add those two assertions. Do not change `to_timestamp`.

## P-005 — accept

No material objection. Buzz `transcriber.py` `SUPPORTED_AUDIO_EXTENSIONS` / `SUPPORTED_VIDEO_EXTENSIONS` are `.mp3 .wav .m4a .m4b .aac .ogg .opus .flac` and `.mp4 .m4v .webm .ogm .mov .mkv .avi .wmv`. humm's three arrays in `folderwatch.rs` omit `m4b`, `aac`, and `m4v`. Folder watch and `humm-gui/src/import.rs` both use those arrays. The CLI does not filter on them. ffmpeg decode failure stays the existing error.

`humm-gui/src/viewer.rs` `is_video` already includes `m4v`. Playback classification is not a second hole. `m4b` and `aac` correctly stay out of that set.

Nonblocking: `SUPPORTED_EXTENSIONS` is not derived. It is a third handwritten array, and `import.rs` `supported_extensions_is_buzz_s_thirteen_lowercase_no_dot` hardcodes the old 13 names. `cargo test -p humm-core --lib folderwatch` does not compile that test. Update all three arrays, their lengths, and that GUI assertion. A change that touches only `SUPPORTED_EXTENSIONS` would pass a predicate test and leave the Audio/Video dialog filters wrong.

## P-006 — revise

Blocking. The Windows title bug is real, and `source.%(ext)s` is the right template. The design does not follow the name through the consumers that actually show it, and it can break the path parser.

Buzz (`file_transcriber.py` `_download_from_url`) downloads to `source.%(ext)s`, sets `display_name` from `info["title"]`, then sets `file_path` to a temp `audio.wav`. Export uses `display_name or basename(file_path)` (`transcriber.py` `get_output_file_path`).

humm does not. `YtDlpFetcher::fetch` returns only a `PathBuf`. `create_task` inserts URL rows with `name` NULL (`humm-store` `create_task`). `display_name_for` (`humm-gui/src/viewmodel.rs`) is `name`, else the file basename, else the URL. After `update_file`, the GUI label *is* the downloaded filename, which today is `%(title)s.%(ext)s`. CLI progress labels are `Input::display_name`, which for a URL is the URL string, not the title (`domain.rs`). Export names come from `job.input`'s stem (`pipeline.rs` calls `output_file_path` on `job.input`; the comment there says the fetched local file is that input).

So:

- Changing the template to `source.%(ext)s` without another change makes the GUI label `source.<ext>` and makes exports `source (...)`, not the title. Writing the title into the task `name` fixes the GUI label only. It does not fix `output_file_path`.
- "The CLI label still shows the title" is not the current behavior. The CLI label is the URL. Preserving it is not a title regression.
- `fetch` treats every non-progress stdout line as the path and keeps the last one (`fetch.rs` `last_line`). `--print after_move:filepath` is that line. A second `--print` of the title overwrites the path unless the parser is changed. The proposal says to use a print field and does not mention this.

Revise the design so one machine-readable record carries path and title, `set_task_name` (or equivalent) stores the title, and `{{ input_file_name }}` for a URL task uses that title the way Buzz uses `display_name`. Keep `--no-playlist` and the process-group kill. Destination is already `media_dir/<id>/`, so `source.%(ext)s` does not collide across tasks.

## P-007 — revise

Blocking against the cited Buzz lines. Keeping a valid prefix is the right direction. Doing it inside every `append_word` with a full strip of `U+FFFD` is not what `whisper_cpp.py` does.

Buzz line 391, and only that call:

```python
text = word_buffer.decode("utf-8", errors="replace").rstrip("\ufffd").strip()
```

`rstrip("\ufffd")` removes a trailing run. It does not remove a `U+FFFD` at the start or in the middle. The calls at lines 374 and 380 ignore `_append_word`'s `False` and drop the word. The non-space path never uses this fallback.

`token.to_bytes()` (`whisper-rs` 0.16) returns `CStr` bytes and does not require valid UTF-8, and `emit_word_segments` only skips a null pointer. An incomplete sequence can reach `append_word`. The gap is real. `flush_complete_chars` already reassembles a split character when the next token arrives. The fallback matters when the segment *ends* on an incomplete sequence.

A unit test of "valid prefix plus one trailing incomplete byte" passes both `trim_end_matches('\u{FFFD}')` and `replace(U+FFFD)`. It also passes if the fallback is applied on the mid-loop flushes. Those are the cases that diverge from Buzz: `b"\xffhello"` stays `"\u{FFFD}hello"` after `rstrip`, and a bad buffer flushed because the next token starts with `b" "` is dropped.

Implement the fallback only on the final flush. Use lossy decode, `trim_end_matches('\u{FFFD}')`, then `trim`, and emit only if that is non-empty. Leave `flush_complete_chars` alone.

## P-008 — defer

No material objection to deferring. Buzz `whisper_cpp_custom_models.py` really is a JSON list plus legacy id `legacy-custom` / filename `ggml-model-whisper-custom.bin`. humm's slot is `ggml-custom.bin` (`registry.rs` `custom_model_file_name`), which is this project's file, not that legacy name. ADR-008 still describes one custom URL slot. The proposal correctly says the sketch is not the design and must not ride along with the CVE edits.

Do not rename humm's cache file to Buzz's legacy name while "keeping the old file working." That orphans existing downloads. A later design has to re-read the Python module; the `{id, name, path}` sketch drops `source_url`.

## P-009 — accept

No material objection, and the only accepted outcome is a named exception. `transformers_whisper.py` on this Buzz commit has FunASR (`is_funasr_model`, commit `851c8b3593` as cited). ADR-008 and Q-2, approved 2026-07-20, keep non-Whisper Hugging Face models out of scope. An in-process Python or `transformers` port violates the standing policy. Do not open an engine in this pass. The decision text should cite `298dbf0` and those two records so the next audit does not file it again.

## P-010 — revise

Blocking. The v1.4.5 sentences are stale as a *behavior pin*, and replacing them with `298dbf0` as if the inventories had been re-read would make them false.

`FEATURE_INVENTORY.md` line 3 and `EXTERNAL_SURFACE.md` line 3 say the documents are ground truth for v1.4.5. The bodies still describe that tree: F-020 lists the old 13 extensions and cites `transcriber.py:222-225`, which on `298dbf0` has moved and grown. A header-only edit tells the next implementer that those paragraphs were checked against `298dbf0`. They were not. The ledger says the other 29 commit subjects were not compared.

The same applies to spec lines that say a claim was *verified* against v1.4.5 (for example the M3 spec header). Restamping the commit does not re-verify them. The proposal says to update those headers. Do not.

Keep `scripts/ci_prepare.py` `BUZZ_COMMIT` and the two blob hashes. That part is right: the ledger's blob SHAs match `298dbf0`.

Revise the doc edit to one sentence: behavior target is Buzz `298dbf003505cf945bdfd1eb8e705348b59a5b34`; this inventory was written against v1.4.5 `aaf98634`; known gaps are the review ledger, not a silent rewrite of every feature row. Update a feature row only when that row is actually re-read.

## P-011 — accept

No material objection. humm does not vendor ffmpeg. `decode.rs` runs `ffmpeg -i` on the user file with no demuxer allowlist, so a crafted file reaches whatever this build probes. An argv or codec allowlist would drop files that transcribe today and was not shown to fix `UBUNTU-CVE-2024-32228` or `UBUNTU-CVE-2025-59734`. No `crates/humm-media` diff is the right change.

The finding's category "demonstrated defect" is stronger than the evidence. The Ubuntu tracker lists the package; the 2024 summary names FFmpeg 7.0; this host is `6.1.1-3ubuntu5`; neither bug was reproduced. That is a reason to keep the non-change, not a reason to patch humm. Record the ids as unconfirmed host advisories.

## P-012 — revise

Blocking. Failing closed when the digest request errors is a reasonable hardening. The design as written does not match its own recovery story, and it contradicts a documented choice without saying so.

`verify_or_warn` (`download.rs`) turns both `Ok(None)` and `Err` into `Ok(())`, then `download_body` renames the partial. That `Err` arm is real. It is also what the M1 plan specified: "If the API call fails, log a warning and skip verification" (`docs/superpowers/plans/2026-07-20-m1-headless-core-path.md`, SHA step). Buzz does not have this split. Its whisper.cpp digest is `url.split("/")[-2]` before the download (`model_loader.py` `_download_whisper`), so a missing digest is not a second HTTP call after the bytes are on disk.

`verify_partial_is_complete` already maps `Err` to `Ok(None)`, and `on_range_not_satisfiable` treats `None` as `RestartFresh`: delete the partial and download again. The 416 test `ensure_coreml_encoder_416_after_restart_errors_without_looping` documents that a tree-API 404 is "unverified, restart." After P-012's first-attempt failure:

1. The body is complete, the rename did not happen, the partial remains.
2. The next `ensure_artifact` sends `Range: bytes=<len>-`.
3. A compliant server returns 416.
4. The lookup fails again, the partial is deleted, and the whole file is fetched again.
5. `verify_or_warn` fails again.

"The partial remains for resume" is true only until that second attempt. The proposed test (500 on a fresh download, partial not renamed) passes without ever calling `ensure_artifact` twice, so it does not catch the re-download. While the tree API is down, every retry pays for the full model.

`coreml_failure_action` maps every `ModelError::Http` to degrade, not to a failed ensure. A tree-API 500 on the encoder zip fails the zip promote and then `resolve_model` continues without CoreML. That matches existing transport policy. It does not match "callers already treat `ModelError` as a failed ensure" or "returns an error that names the file" at the `resolve_model` boundary. ggml and VAD do fail the ensure. The diarization sidecar with no `lfs.oid` stays `Ok(None)` and should still promote (`ensure_diarization_models`'s comment). Custom URLs pass `sha_repo: None` and never enter `verify_or_warn`.

`find_sha256` uses `serde_json::from_str(...).ok()?`. HTTP 200 with a non-JSON body is `Ok(None)`, which this proposal still promotes. A 500 is fail-closed and an error page with status 200 is not.

Revise both arms together: lookup `Err` fails the ensure, does not delete a complete partial, and `verify_partial_is_complete` must not treat that same `Err` as "restart fresh." `Ok(None)` after a parsed tree that lacks `lfs.oid` still promotes. Decide explicitly whether a 200 body that does not parse is `Err`. Add a test that calls the download twice with the tree route left at 500 and asserts a single file GET.

## Audit notes

False or over-strong claims, not extra proposals:

- F-013 is not a demonstrated ffmpeg bug on this build. P-011's non-change still stands.
- `SUPPORTED_EXTENSIONS` is not derived from the audio and video arrays.
- The CLI URL label is the URL, not the media title.
- "Callers already treat `ModelError` as a failed ensure" is false for CoreML HTTP errors.
- F-001 is not a false positive. The zip 8.6.0 docs say symlink targets are checked; `make_symlink` does not check the new link's target.
- F-006's `1.2` → 1.002 s and `1.45` → 1.045 s match the parser. Not a false positive.
- F-009's code difference is real. Frequency stays medium because only a segment that *ends* on an incomplete UTF-8 sequence hits it.

Missing from the proposals, found while checking the ones above:

- URL export stems and the yt-dlp stdout "last line is the path" protocol (P-006).
- 416 restart versus fail-closed digest lookup (P-012).
- `TempAudioGuard` and the demucs scratch dir versus `process::exit` (P-003).
- `humm-gui` import extension snapshot test (P-005).
- `find_sha256` collapsing malformed JSON to "no digest" (P-012).

Overlooked on purpose, and not cleared by this review: the Buzz subjects the ledger marks "not compared" (folder-watch crash and duplicate entries, resizer merge-by-gap, speaker-id CLI, preference window size, XDG secret delay, multi-file CLI, demucs RAM). Folder-watch `read_dir` / `metadata` failures are already skipped in `folderwatch.rs`. That is not a finding. The other subjects were not re-read here.

`encode.rs` is the same direct-child kill as `decode.rs` and `video.rs`. Not a new proposal.

Unjustified priority: F-010 and F-011 are scope decisions already recorded in ADR-008 / Q-2. Medium severity next to F-007 (files the user cannot import) overstates them. P-008 and P-009 are correctly kept off the CVE patch. They should not jump the unread Buzz fixes if someone later ranks work by the finding table alone.

`cargo test` was not run. The speakrs `coreml` stop on this Linux VM is an environment limit, not a product defect, and it does not affect `humm-core` or `humm-whisper` unit tests.

## Second pass — `d76cd1c`

Reviewed only the revised text in `docs/review/ledger.md` at `d76cd1c3d7ddcfe5206c6e55fa9d8e1cc4f827c4`. Product code is still `125a54da`. The accepted implementations (P-001, P-002, P-004, P-005, P-009, P-011) were not re-reviewed. This section supersedes the first-pass rows for P-003, P-006, P-007, P-008, P-010, and P-012.

| ID | Disposition | Earlier blocking points |
|---|---|---|
| P-003 | revise | Drop-after-exit and the handler hang are resolved. The new acceptance is not. |
| P-006 | revise | Export stem, GUI label, CLI URL label, and the second `--print` are resolved. The title is still an unsanitized path segment. |
| P-007 | accept | Resolved. No material objection. |
| P-008 | defer | Resolved. No material objection to staying deferred. |
| P-010 | accept | Resolved. No material objection. |
| P-012 | revise | The 416 rule, malformed JSON, named CoreML degrade, and the M1 reversal are in the design. The verification contradicts the 416 rule. |

### P-003 — revise

The two earlier blockers are resolved in the design. Explicit delete before `process::exit` is the right answer to `TempAudioGuard` and the demucs `TempDir`, because `exit` still will not run `Drop`. `Atomic` slots or `try_lock` then exit 130 is the right answer to the handler hang. Signaling only an unreaped child, not joining, and leaving the first signal as `cancel_all` stay correct. `decode.rs`, `encode.rs`, and `video.rs` staying on `child.kill()` is still right.

The new acceptance is wrong for one existing contract. It says the denoised path is gone after the second SIGINT. `TempAudioGuard` does not delete that path when `keep_denoised_file` is set (`queue.rs`, and `denoised_output_kept_when_keep_denoised_file_set`). That file is the artifact the user asked to keep, beside the input, not a scratch file. A second Ctrl-C after denoise has finished and while transcription is still running would remove it. An implementation that respects the setting fails this acceptance; an implementation that satisfies the acceptance breaks the setting.

Revise the acceptance: `_speech` and the demucs scratch directory are gone; a denoised file is gone only when `keep_denoised_file` is false. Do not register the user's source path. `SIGKILL` the group before deleting, so a live child cannot recreate the file, and do not wait for it. Temp-path deletion is not unix-only. When the registry lock is busy, exiting 130 without cleanup is the tradeoff the design already accepts.

### P-006 — revise

The earlier blockers are resolved. One stdout record, not a second `--print` on the path line, stops the parser from taking the title as the path (`fetch.rs` keeps the last non-progress line). `set_task_name` (or equivalent) fixes the GUI, because `display_name_for` is `name`, else the file basename. Leaving the CLI label as `Input::display_name` (the URL) is the current behavior, not a regression. Using the title for `{{ input_file_name }}` on a URL task is the Buzz `display_name` choice, and a queue or pipeline test can catch a fix that only updates the GUI.

`SqliteStore::set_task_name` is not on `TaskStore`. The queue holds `dyn TaskStore`. "Or equivalent" covers adding a trait method. That is not a new design.

What is still wrong: the title is not a filename. `output_file_path` does `directory.join(file_name)` with the stem interpolated raw (`export/mod.rs`). A title `AC/DC` or `../x` becomes another path component. Buzz runs that string through `safe_filename_component` (`paths.py`) immediately after choosing `display_name or basename`: `/` and `\` become `_`, and a trailing space or dot becomes `#`. humm has no equivalent. The acceptance title is one that ends in `.` or ` `, checked against the download path. The default export template appends more text and an extension, so that title does not prove the export path is safe. A slash passes the written acceptance and writes outside the intended directory.

Revise: the GUI `name` is the raw title; the export component is that title after the same sanitizer. Add a title containing `/` or `..` to the export assertion.

Nonblocking: `check_skip` / `stem_for` uses `job.input`. After this change that file is `source.<ext>`, so skip looks for `source*` and will not see a title-named export. Buzz has the same split (`audio.wav` versus `display_name`). Say so, or point skip at the sanitized title. The written acceptance does not run skip.

### P-007 — accept

No material objection. The earlier blocker is resolved.

The fallback is only the final flush. `trim_end_matches('\u{FFFD}')` then `trim` is `rstrip("\ufffd").strip()` (`whisper_cpp.py` line 391). `b"\xffhello"` stays `"\u{FFFD}hello"`. A bad buffer flushed because the next token starts with `b" "` stays dropped (lines 374 and 380 ignore the `False`). `flush_complete_chars` is untouched. One test with those three cases fails a global strip inside every `append_word`. That is the check the first design lacked.

### P-008 — defer

No material objection. The deferral now includes the constraint that mattered: humm's file stays `ggml-custom.bin`, Buzz's legacy name `ggml-model-whisper-custom.bin` is not a rename target, and a later design has to re-read `whisper_cpp_custom_models.py` (including `source_url`). ADR-008's one slot stands. Not part of this patch set.

### P-010 — accept

No material objection. The earlier blocker is resolved.

The design keeps v1.4.5 `aaf98634` as the inventory's written tree, names `298dbf003505cf945bdfd1eb8e705348b59a5b34` as the behavior target, says the bodies were not re-read, and refuses to restamp "verified against v1.4.5" lines. Feature rows stay. `scripts/ci_prepare.py` stays, which is right because the two fixture blobs match `298dbf0`.

Nonblocking: the sentence has to be added in `FEATURE_INVENTORY.md` and `EXTERNAL_SURFACE.md`, beside the existing ground-truth lines. A sentence that exists only in this ledger leaves those files saying they are the v1.4.5 ground truth. "Not a header swap" means do not replace that line with `298dbf0` alone.

### P-012 — revise

Three of the four earlier points are resolved in the design body. Lookup `Err` fails the ensure and does not delete a complete partial. `verify_partial_is_complete` must not treat that `Err` as `RestartFresh`. A parsed tree with no `lfs.oid` still promotes, which keeps the diarization sidecar and custom URLs (`sha_repo: None`) on the old path. A 200 body that does not parse is `Err`, so `find_sha256`'s `serde_json::from_str(...).ok()?` cannot stay as it is. The M1 sentence ("If the API call fails, log a warning and skip verification") is called out as a deliberate reversal. CoreML `ModelError::Http` still degrades and does not fail the ggml ensure. That last sentence is the right resolution of the earlier point: the defect was a design that claimed a failed ensure while `coreml_failure_action` degrades. Degrade itself matches the existing transport policy. Do not fail the ggml model because the encoder's digest lookup failed.

The verification undoes the 416 fix. `ensure_coreml_encoder_416_after_restart_errors_without_looping` still asserts that a tree-API 404 discards the partial and issues a second, non-ranged file GET (`download.rs`, two `zip_requests`, the second with `range.is_none()`). The proposal says that test still passes, and also says `Err` must not restart fresh. Both cannot be true. Update the test: one ranged probe, no second body download, partial still present.

"A single file GET" across two `ensure_artifact` calls is the other trap. A correct second attempt can `Range: bytes=<len>-`, get 416, see the lookup fail, and return without `download_fresh`. That is a second GET to the file URL and zero extra body bytes. A test that counts every GET rejects that and pushes the implementation into skipping resume entirely. Count body downloads, or assert the partial's bytes are unchanged and no non-ranged GET follows the 416. When a later lookup succeeds, `Some(true)` must still promote that same partial. The design does not say to change that arm. Leave it.
