# Cross-change review

Reviewed the combined product tree `1bfd98699dd015ca02c57266d75779a28e905f07` against `125a54da3d284ba3be651b4c99a4456ce56cd76a` on `audit/architecture-review`. Worktree: `/workspace/.worktrees/humm-audit`. No product source was edited.

HEAD while this was written is `d51814fcf61ba8a27e3b3098199f1e81a3a071c7`. Two commits sit after the product tip, and both stay in `docs/review`:

- `c100d996153508f0097f48e26b574f6e5615b2d4` changes `docs/review/patch-review-p003.md`
- `d51814fcf61ba8a27e3b3098199f1e81a3a071c7` changes `docs/review/ledger.md`

`git diff 1bfd986 HEAD -- . ':(exclude)docs/review'` is empty. `1bfd986` is the product tip.

## Verdict

**PASS** for `1bfd98699dd015ca02c57266d75779a28e905f07`.

No substantiated blocking interaction. Each accepted fix is still the tree that was passed, and none of them undoes another. `43ea48433baff21dcf4076e7e0cbdbc48a17f4bc` stays **REQUEST_CHANGES**; the follow-up at this tip is what the combined state includes. P-006, P-008, and P-012 were not implemented.

## Presence

No product path is touched by two different proposal IDs. For each passed SHA below, that path at `1bfd986` matches the SHA (`git diff <sha> 1bfd986 -- <path>` empty).

| ID | Passed SHA | Still that tree |
|---|---|---|
| P-001 | `1954ffc4305c5847f7da3fe0037459932e2c47cf` | `crates/humm-core/src/models/download.rs` |
| P-002 | `8e2244670c6a4692094f6584cb584e0a6a501628` | `Cargo.lock` |
| P-003 | `1bfd98699dd015ca02c57266d75779a28e905f07` | `abort.rs`, `demucs.rs`; `run.rs`, `queue.rs`, `fetch.rs`, `humm-media` `lib.rs` match `43ea484` and were not edited again |
| P-004 | `423ec1fd2160dedc2579469c5fcee6f08fe74db9` | `crates/humm-core/src/viewer/timestamp.rs` |
| P-005 | `4c1ac2b4d1670f9440d7d0683e7ff648061f3576` | `folderwatch.rs`, `humm-gui` `import.rs` |
| P-007 | `89489dac2ff22c8fc3543cc2ccdafb51faddbe73` | `crates/humm-whisper/src/words.rs` |
| P-009 | `66613c07a5516e0cc0209176e45b62ca58eed29d` | `docs/OPEN_QUESTIONS.md`, `docs/design/DECISIONS.md` |
| P-010 | `bd2c7d720c12dc4ac745189b34cc64fde938b623` | `FEATURE_INVENTORY.md`, `EXTERNAL_SURFACE.md` |
| P-011 | no commit | ffmpeg invocation sources unchanged; see below |

What the original findings asked for is still in this tree:

- P-001. `extract_zip_archive` does not call `ZipArchive::extract`. `apply_components` treats `NotFound` on `..` as an escape (`download.rs` around lines 1206–1210). Direct `zip` stays 8.6.0. `extract_zip_archive_rejects_symlink_chain_when_dotdot_precedes_the_link` is present. The transitive `zip` 6.0.0 entry is still there and is still not the extract path.
- P-002. Lockfile: `rustls` 0.23.46, `rustls-webpki` 0.103.16, `webpki-roots` 1.0.10. `ureq` 3.3.0 and `webpki-root-certs` 1.0.9 are unchanged. The whole `Cargo.lock` diff against the baseline is those three version bumps plus `tempfile`'s `getrandom` dependency line (0.4.3 to 0.3.4). No other crate moved.
- P-003. `install_ctrlc` still calls `try_emergency_stop(kill_tracked_groups)` and then `process::exit(130)` (`run.rs` lines 794–795). `try_emergency_stop` still passes `|_| true` for groups and temps (`abort.rs` lines 135–140). Speech is registered only when the isolated path is not the source (`queue.rs` around 1416–1421). The denoised registration passes `keep_denoised` (around 1483–1486). Demucs registers its child and scratch dir after spawn (`demucs.rs` lines 191–192). yt-dlp does the same for its child (`fetch.rs` line 141) after `spawn_own_process_group`. The humm-media test calls `try_emergency_stop_registered` with its own four ids (`demucs.rs` lines 754–762). Nothing else calls that function.
- P-004. `1.2` pads to 1200 ms. `to_timestamp` is not in the diff.
- P-005. The three arrays are the 8 + 8 Buzz `main` suffixes, including `m4b`, `aac`, and `m4v`. The GUI test compares those constants. `viewer` `is_video` is not in the diff.
- P-007. Space and comma flushes still ignore `append_word`'s `false`. Only the final buffer calls `push_lossy_final_word`, which strips a trailing U+FFFD and then trims.
- P-009. Two documentation sentences. No `Cargo.toml` or engine crate is in the product diff.
- P-010. One inserted sentence in each analysis header. Feature rows are not in the diff. `scripts/ci_prepare.py` is not in the diff.

## Interactions

P-002 does not relax P-001. The lockfile diff does not touch `zip`. Extraction still refuses an escaping link on the bytes that a substituted download would hand it. The trust-floor bump and the extractor are the two halves of F-001's "HTTPS is not enough" note, and both are present.

P-003 does not change the zip, digest, or timestamp paths, and those paths do not register process groups or temp files. A second Ctrl-C still deletes only registered speech, scratch, and un-kept denoised files. `.humm-coreml-extract` stays unregistered, which is the same as before P-003: `process::exit` already skipped that cleanup.

P-005 does not touch ffmpeg's argv. Folder watch will queue `m4b`, `aac`, and `m4v` into the same isolate-then-transcribe path. F-007 already stated that a host demuxer miss fails the task like any other undecodable file. That is not an ffmpeg CVE patch.

P-004 and P-007 do not share a function. `parse_timestamp` reads editor text. `group_words` writes word text. Neither commit feeds one into the other.

P-009 and P-010 name the same Buzz `main` `298dbf003505cf945bdfd1eb8e705348b59a5b34`. The inventory sentence still says the rows describe v1.4.5. That was already accepted on the P-010 review, which ran after P-005 and P-009 had landed. Nothing since then edited those sentences.

P-011's earlier "no `crates/humm-media` diff" check is not true at this tip. The diff is P-003 only: `demucs.rs`, `fetch.rs`, and `lib.rs` (`kill_tracked_groups`). `decode.rs`, `encode.rs`, and `video.rs` match the baseline. The diff does not add a demuxer or codec allowlist and does not vendor ffmpeg. `UBUNTU-CVE-2024-32228` and `UBUNTU-CVE-2025-59734` are still untouched host advisories. The process-group signal kills a yt-dlp or demucs grandchild; it does not change what ffmpeg is asked to probe. That does not undo P-011.

`tempfile`'s `getrandom` line moved inside the P-002 lockfile bump. P-003's scratch dir uses that same `tempfile` crate. The version of `tempfile` did not change, and the P-002 review already accepted the dependency line. No later commit edited `Cargo.lock`.

## Deferred, not implemented

- P-006. `fetch.rs` still passes `%(title)s.%(ext)s` (`fetch.rs` line 98). The product diff does not add `safe_filename`, a second yt-dlp record, or a title write on the URL download path. `set_task_name` remains the pre-existing rename API.
- P-008. `custom_model_file_name` is still `ggml-custom.bin` (`registry.rs` lines 101–102). `registry.rs` is not in the product diff.
- P-012. `verify_or_warn` still turns `Ok(None)` and `Err` into success (`download.rs` lines 689–698). `on_range_not_satisfiable` still maps `None` to `RestartFresh`. `find_sha256` still uses `serde_json::from_str(...).ok()?`, so a non-JSON body stays `None`. Those functions have no diff against the baseline. `coreml_failure_action` is unchanged: only `ModelError::Http` degrades.

## Not a product objection

The ledger's opening paragraph still says P-003 is not implemented. The P-003 section and this tree say otherwise. That sentence is review text. It is not a behavior change.
