# Adversarial review of the accepted patches

Reviewed the commits below on `audit/architecture-review`. HEAD while this was written is `22c4049d2e2fd972c7fa7348448adfad81145dd2`. Commits after `66613c07` change `crates/humm-whisper/src/words.rs`, the analysis headers, and review docs. They do not change the files in these patches, and `crates/humm-media` is still identical to `125a54da3d284ba3be651b4c99a4456ce56cd76a`. Buzz was read at `/tmp/audit-sources/buzz` `298dbf003505cf945bdfd1eb8e705348b59a5b34`. No product file was left modified.

| ID | Commit | Verdict |
|---|---|---|
| P-001 | `e6637e08483ff8c00669fa1874cc40f41b6bcb00` | REQUEST_CHANGES |
| P-001 follow-up | `7d887ded7323356af790a33c323576327f55b4b1` | REQUEST_CHANGES |
| P-001 revision | `1954ffc4305c5847f7da3fe0037459932e2c47cf` | PASS |
| P-002 | `8e2244670c6a4692094f6584cb584e0a6a501628` | PASS |
| P-004 | `423ec1fd2160dedc2579469c5fcee6f08fe74db9` | PASS |
| P-005 | `4c1ac2b4d1670f9440d7d0683e7ff648061f3576` | PASS |
| P-009 | `66613c07a5516e0cc0209176e45b62ca58eed29d` | PASS |
| P-011 | no commit; `crates/humm-media` identical to `125a54da3d284ba3be651b4c99a4456ce56cd76a` | PASS |

## P-001 — REQUEST_CHANGES

`e6637e08483ff8c00669fa1874cc40f41b6bcb00`

Blocking. The new extractor still writes a file outside the destination for a symlink chain whose lexical form stays inside. zip stays 8.6.0, and `ZipArchive::extract` is no longer called. That does not close the hole, because `proposed_symlink_escapes` cancels `..` with `PathBuf::pop` before it knows whether the cancelled component is a symlink.

`lexical_normalize` in `crates/humm-core/src/models/download.rs` treats every `ParentDir` as a pop. `proposed_symlink_escapes` then accepts the result when it `starts_with` the extract directory. `File::create` and `create_dir_all` follow the real chain.

This archive does it:

1. symlink `a/b/missing` → `..`
2. symlink `a/b/link` → `missing/../../outside`
3. file `a/b/link/pwned` with body `owned`

Link 2 normalizes to `<dest>/a/outside`, so both links are created. I ran `extract_zip_archive` on that zip under `cargo +stable test` (rustc 1.99, Linux). It returned `Ok(())`. `<parent>/outside/pwned` contained `owned`. The link left behind was `missing/../../outside`. The probe was not committed.

The committed test `extract_zip_archive_does_not_create_an_escaping_symlink` uses a top-level target `../outside`. That shape is rejected, and I re-ran it: pass. `place_coreml_contents_does_not_follow_an_escaping_symlink` is the same direct target: pass. Those two results cannot see this archive. `cargo test -p humm-core --lib models::download::` passing is the same gap.

`place_coreml_contents` only calls `symlink_target_escapes` on the immediate `read_dir` children. This archive's top entry is the real directory `a`, so that loop does not see `a/b/link`. `ensure_coreml_encoder` calls `place_coreml_contents` only after extract returns `Ok`, so this zip is a successful extract. The outside file is already written by then. The nested link is inside the directory that gets renamed to the model dir.

What has to be true: a `..` that follows a component that is not already a real, non-symlink directory is rejected, both when the link is created and when a later entry is written. For the archive above, extract returns an error, `a/b/link` is absent, and `outside` is empty. Refusing every symlink entry meets that bar. Nothing in the tree shows a CoreML zip that needs a symlink kept.

Calling `ZipArchive::extract` on zip 9.0.0 or 9.0.1 is not a substitute. [GHSA-x2x8-gq67-9mq6](https://github.com/zip-rs/zip2/security/advisories/GHSA-x2x8-gq67-9mq6) (patched 9.0.2) is the same class: lexical cancellation of `..` across a component that a later entry can create as a symlink. Their example is `l → x/../outside` followed by `x → .`. I did not run 9.0.2.

Archive unix modes are not applied. `File::create` leaves SUID/SGID unset, so that part of `RUSTSEC-2026-0367` is closed here and is not the objection. `docx` still only reads zip entries. The direct dependency remains zip 8.6.0 (`Cargo.toml`, lockfile package `zip` 8.6.0). The write above is the defect.

The follow-up `7d887ded7323356af790a33c323576327f55b4b1` is a separate verdict below. This SHA stays REQUEST_CHANGES.

## P-002 — PASS

`8e2244670c6a4692094f6584cb584e0a6a501628`

No material objection. The lockfile resolves a single `rustls` 0.23.46, `rustls-webpki` 0.103.16, and `webpki-roots` 1.0.10. `cargo +stable tree -p humm-core -i rustls -i rustls-webpki -i webpki-roots` shows those versions on reqwest 0.12.28 / hyper-rustls. `ureq` is still 3.3.0. `webpki-root-certs` is still 1.0.9.

`webpki-roots` 1.0.10's source has no `emSign ECC Root CA - G3`. That label is present in 1.0.9. That is the root `RUSTSEC-2026-0381` / `CVE-2026-111943` names, and the advisory's patched floor is `>= 1.0.10`. rustls 0.23.46 is the release that includes the 0.23.45 encryption-level fix and `GHSA-w44r-93j7-qv28`.

`ort-sys` 2.0.0-rc.12 with `tls-native` sets `RootCerts::PlatformVerifier` and `TlsProvider::NativeTls` (`build/download/mod.rs`). ureq's native-tls `PlatformVerifier` arm uses the OS roots and does not load `webpki_root_certs::TLS_SERVER_ROOT_CERTS`. Leaving `webpki-root-certs` 1.0.9 in the unused `WebPki` arm is consistent with that. That crate's 1.0.9 source still contains G3. It is not the reqwest trust path.

The download and update tests speak HTTP to `TestServer`. They do not show that G3 is gone. The crate source and the tree do. I did not re-run those tests.

Nonblocking: `webpki-roots` 1.0.11 (2026-10-23) removes `emSign Root CA - C1` and `emSign ECC Root CA - C3`, which are still in the locked 1.0.10. That is a later CCADB update (Mozilla bug 2083709), not the G3 floor in `RUSTSEC-2026-0381`. The same lockfile change moves `tempfile` 3.27.0 from `getrandom` 0.4.3 to 0.3.4. Both satisfy that crate's `>=0.3, <0.5`. It is outside the TLS advisories.

## P-004 — PASS

`423ec1fd2160dedc2579469c5fcee6f08fe74db9`

No material objection. Buzz `parse_timestamp` (`transcription_segments_editor_widget.py` lines 88–89 at `298dbf0`) is `int(fraction[:3].ljust(3, "0"))`, and a `ValueError` returns `None`. The new parser takes the first fractional group, keeps at most three characters, and right-pads with zeros.

`1.2` is 1200 ms, `1.45` is 1450, `1.4567` is 1456, `1.` is 1000 (empty fraction pads to `000`), and `1.2.3` is 1200 (fraction `2` pads to 200, plus one second). `to_timestamp` is unchanged, so stored rows stay integer milliseconds until a cell is edited. `cargo +stable test -p humm-core --lib timestamp::` — 8 passed, including those cases.

Nonblocking: `fraction[..fraction.len().min(3)]` slices bytes. A multibyte character that straddles byte 3 panics (`1.abé`). Buzz slices by code point and turns a bad `int` into `None`. The timestamp editor strips the buffer to `[0-9:.]` before parse (`views/viewer_table.rs`, `render_timestamp_editor`), which is Buzz's validator, so the viewer path does not feed that string.

## P-005 — PASS

`4c1ac2b4d1670f9440d7d0683e7ff648061f3576`

No material objection. Buzz `transcriber.py` lines 225–229 are `.mp3 .wav .m4a .m4b .aac .ogg .opus .flac` and `.mp4 .m4v .webm .ogm .mov .mkv .avi .wmv`. The three handwritten arrays in `folderwatch.rs` are those suffixes, without dots, audio then video. `SUPPORTED_EXTENSIONS` is the concatenation. `has_supported_extension` still compares case-insensitively, and folder import uses it, so the dialog filters and the scan share one list.

`humm-gui` `is_video` already lists `m4v` and does not list `m4b` or `aac`. Playback classification was left alone. `cargo +stable test -p humm-core --lib folderwatch::` — 18 passed, including `m4b_aac_and_m4v_are_supported`.

The GUI assertion `supported_extensions_match_buzz_main_lowercase_no_dot` matches those three arrays. It did not compile here: `speakrs` 0.5.0 `coreml` is macOS-only, and the feature was not turned off. The assertion is a literal compare against the constants above. ffmpeg still decides whether a queued file decodes.

## P-009 — PASS

`66613c07a5516e0cc0209176e45b62ca58eed29d`

No material objection. The diff is two documentation sentences. No engine, crate, or dependency is added. Q-2 and ADR-008 still keep non-Whisper Hugging Face models out of scope, and they name FunASR against Buzz `298dbf003505cf945bdfd1eb8e705348b59a5b34`.

That tree has the feature. `model_loader.py` defines `is_fun_asr_model`, and `transformers_whisper.py` has the property `is_fun_asr_model`. GitHub commit `851c8b359343db8c9460aa15ee59a40f2ab66aa8` is "Adding support for FunASR (#1651)" and introduces `def is_fun_asr_model`. The same name is in the `298dbf0` tree.

Nonblocking: both new sentences say `is_funasr_model`. That identifier is not in either file. The file paths and the two commits are enough to find the feature; the symbol a later audit will grep for is `is_fun_asr_model`.

## P-011 — PASS

No implementation commit. `git diff 125a54da3d284ba3be651b4c99a4456ce56cd76a HEAD -- crates/humm-media` is empty. Leaving `UBUNTU-CVE-2024-32228` and `UBUNTU-CVE-2025-59734` unchanged in humm is still justified.

This host is `ffmpeg` `6.1.1-3ubuntu5`. humm does not vendor ffmpeg. `decode.rs` `decode_mono_at` runs `ffmpeg -nostdin -threads 0 -i <path> -f f32le -ac 1 -acodec pcm_f32le -ar <rate> -loglevel error -`. There is no demuxer or codec allowlist, so a crafted file reaches whatever this build probes.

Ubuntu's current pages:

- `CVE-2024-32228`: the upstream text names a HEVC SEGV in FFmpeg 7.0 at `hevc_frame_end`. Ubuntu 24.04 is "Ignored, changes too intrusive". Ubuntu 22.04 is "Not affected". The 7.0 sentence is not a clearance of this package.
- `CVE-2025-59734`: SANM use-after-free, and the writeup says probing a sanm file is enough. The OSV affected list includes `7:6.1.1-3ubuntu5`. Ubuntu 24.04 is "Ignored, changes too intrusive". Upstream points at FFmpeg 8.0.

Neither bug was reproduced here. The fix Ubuntu declined is in ffmpeg. An extension allowlist does not stop the CLI from passing a path to `-i`, and it was not shown to skip HEVC or SANM probing. No `crates/humm-media` change is the right humm patch.

Uncertain, and not a reason to patch humm: whether 6.1.1's HEVC decoder contains the 7.0 SEGV. Ubuntu did not mark 24.04 "Not affected", and this review did not run a sample.

## P-001 follow-up — REQUEST_CHANGES

`7d887ded7323356af790a33c323576327f55b4b1`

Blocking. The original chain is rejected when `missing` is created first. The same names in the other order still create the escaping link, return success, and install it.

`extract_zip_archive_rejects_symlink_chain_that_escapes_via_dotdot` does build the archive from the previous review: symlink `a/b/missing` → `..`, then symlink `a/b/link` → `missing/../../outside`, then file `a/b/link/pwned`. I ran that test: pass. `missing` exists before `link` is checked, `apply_components` follows it, and `..` is applied to the resolved directory, so `link` is not created. zip stays 8.6.0. That part of the claim is true.

`apply_components` still treats a missing component as a real directory. On `NotFound` it sets `current` to that name (`download.rs` around the `NotFound` arm), and `ParentDir` then pops it (`pop_inside`). A `..` after a name that does not exist yet is cancelled lexically. A later entry can create that name as a symlink. The earlier link is not checked again.

This archive is the same chain with the entries swapped, and no file entry after them:

1. symlink `a/b/link` → `missing/../../outside`
2. symlink `a/b/missing` → `..`

While `missing` is absent, `missing/../../outside` from `a/b` walks to `<dest>/a/outside`, which is inside `<dest>`. Both links are created. `extract_zip_archive` returns `Ok(())`. Writing `a/b/link/pwned` then creates `<parent>/outside/pwned` with body `owned`. `place_coreml_contents` returns `Ok` and the placed model directory contains `b/link` → `missing/../../outside`. `place_coreml_contents` still only runs `symlink_target_escapes` on the immediate children of the extract directory, so the real directory `a` hides the nested link.

I ran that zip through `extract_zip_archive` and `place_coreml_contents` under `cargo +stable test` on Linux. The probe was not committed. The new test cannot see it: its zip writer adds `missing` before `link`.

A `..` after a component that does not exist has to be an escape, not a pop. For the swapped archive, extract must not return success, `a/b/link` must be absent, and `outside` must stay empty. `place_coreml_contents` must not install that link. The committed test has to build this order, not only the order where `missing` already exists.

The revision `1954ffc4305c5847f7da3fe0037459932e2c47cf` is a separate verdict below. This SHA stays REQUEST_CHANGES.

## P-001 revision — PASS

`1954ffc4305c5847f7da3fe0037459932e2c47cf`

No material objection. `..` after a component that does not exist yet is now an escape. `apply_components` stats `current` before `pop_inside`. `NotFound`, or anything that is not a directory, returns escape instead of popping that name. zip stays 8.6.0. `ZipArchive::extract` is still not called.

`extract_zip_archive_rejects_symlink_chain_when_dotdot_precedes_the_link` builds the swapped archive: `a/b/link` → `missing/../../outside`, then `a/b/missing` → `..`, then file `a/b/link/pwned`. It requires an error, `a/b/link` absent, `outside` empty, and `place_coreml_contents` not installing `b/link`. The earlier test still builds `missing` first. I ran both: pass.

I also extracted these orders on Linux and looked for a symlink whose target is absolute outside the extract directory, a symlink that canonicalizes outside it, or a write through a symlink that lands outside it: `x/../outside` before and after `x` → `.`; `link` → `a/b` then `a` → `../..`; a real directory `foo` then `foo/../../../outside`; `a/up` → `..` then `up/../../outside`; an absolute `/tmp/...` target; `pivot/x` before `pivot` → `../../outside`; `missing/foo/../../../outside` before `missing` → `..`; shallow `../outside`; and `a/b/link` → `../../../outside` after real `a/b`. None of them created an outside write. The probe was not committed.

A symlink whose target contains no `..` and no absolute path stays inside at creation, and a later entry cannot retarget it. A later entry that would point that name outside is rejected on its own target. That closes the ordering hole from `7d887ded`.

Nonblocking: the same rule rejects `..` through a name that a later entry creates as a real directory, even when the resolved path would stay inside. Nothing in the tree shows a CoreML zip that needs that shape.
