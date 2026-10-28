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

## Proposals (design-unreviewed)

Implementation order after review: P-002, P-001, P-004, P-006, P-007, P-003, P-005, P-010. P-008 only with its own design. P-009 is a decision, not an engine. P-011 is an explicit non-change.

### P-001 — bump direct `zip` to >= 9.0.0

- Objective: stop `ZipArchive::extract` from creating symlinks that point outside the CoreML destination, and stop preserving SUID/SGID bits.
- Findings: F-001
- Design: change the workspace `zip` requirement to a 9.x release that contains the fixes. Keep `extract_zip_archive` if the method still exists; if 9 renamed it, call the safe equivalent and nothing else. Do not bump `zip` 6.0.0 / `ndarray-npy`.
- Contracts: `ModelDownloader` CoreML placement. Benign encoder zips must still land in `target_dir` with the sentinel written last.
- Alternatives: leave 8.6.0. Rejected for the symlink case.
- Risks: zip 9 API break. Mitigation: the existing extract tests must pass before any other zip use changes.
- Acceptance: lockfile `zip` used by `humm-core` is `>= 9.0.0`; a symlink entry targeting `../` outside dest does not create that link; a normal single-directory encoder zip still renames into place; `ndarray-npy` still resolves `zip` 6.x.
- Verification: `cargo test -p humm-core --lib models::download`.

### P-002 — patch the rustls trust path

- Objective: reject the distrusted emSign intermediates and take the other rustls fixes that ship in the same versions.
- Findings: F-002, F-003, F-004
- Design: `cargo update -p webpki-roots -p rustls-webpki -p rustls` only, staying inside reqwest 0.12's existing requirements. Targets: `webpki-roots >= 1.0.10`, `rustls-webpki >= 0.103.16`, `rustls >= 0.23.46`. If a parent requirement refuses one of those, stop and report the blocker rather than upgrading reqwest's major line or unrelated crates. Do not touch `ureq`, `tokio`, `quinn`, `rtrb`, or `tract`.
- Contracts: TLS client behavior for model download, API calls, telemetry, and update check. URLs and payloads stay the same.
- Alternatives: roots-only bump. Worse, because 0.23.46 is the same client and also covers F-003 and F-004.
- Risks: a host that still uses the emSign chain will fail closed.
- Acceptance: `cargo tree -p humm-core -i webpki-roots,rustls,rustls-webpki` shows those minimums; no feature or URL diff.
- Verification: `cargo test -p humm-core --lib` for `models::download` and `update`.

### P-003 — signal humm's process groups on the second Ctrl-C

- Objective: the second SIGINT must not leave demucs, yt-dlp, or their ffmpeg grandchildren running.
- Findings: F-005
- Design: remember each unix process group created by `spawn_own_process_group`. The second-signal path signals those groups, then exits 130. Do not join them and do not change the first-signal `cancel_all` path. Non-unix stays a direct exit.
- Contracts: CLI exit code 130. First Ctrl-C still cancels cooperatively.
- Alternatives: leave ADR-023's debt. Recorded in F-005.
- Risks: signaling a recycled pgid. Only ids this process created, and only while the child is unreaped, may be signaled.
- Acceptance: a stub grandchild that ignores the parent's death is dead after the second SIGINT; first SIGINT behavior and exit codes stay as tested.
- Verification: `humm-media` grandchild unit tests and a CLI test or a focused harness around `install_ctrlc`.

### P-004 — parse timestamp fractions as Buzz main does

- Objective: a 1- or 2-digit fraction is a decimal, padded or truncated to milliseconds.
- Findings: F-006
- Design: in `parse_timestamp`, take `fraction[:3]` and right-pad with zeros to length 3 before parsing, matching `transcription_segments_editor_widget.py` lines 88–89. Do not change colon splitting or the extra-dot rule beyond what that formula implies (`1.2.3` uses fraction `2` → 200 ms).
- Contracts: viewer start/end cell parsing. Display formatting stays `to_timestamp`.
- Alternatives: keep the v1.4.5 integer parse. Rejected; the tests are freezing the bug.
- Risks: edited times change for short fractions. That is the fix.
- Acceptance: `1.2` → 1200, `1.45` → 1450, `1.4567` → 1456, `01:02:03.456` unchanged, garbage still `None`.
- Verification: `cargo test -p humm-core --lib viewer::timestamp`.

### P-005 — allow m4b, aac, and m4v

- Objective: folder watch and import accept the extensions Buzz `main` lists.
- Findings: F-007
- Design: add `m4b` and `aac` to the audio list and `m4v` to the video list, in Buzz's order, and keep the derived `SUPPORTED_EXTENSIONS` consistent.
- Contracts: `folderwatch` allowlist and any GUI filter that copies it.
- Alternatives: leave the 13-name list.
- Risks: decode failure on a thin ffmpeg build. Existing error path.
- Acceptance: the three suffixes pass the extension predicate; an unknown suffix still fails.
- Verification: `cargo test -p humm-core --lib folderwatch`.

### P-006 — download URL media to a fixed file name

- Objective: the on-disk download name is not the media title.
- Findings: F-008
- Design: pass yt-dlp an output template equivalent to `source.%(ext)s` under the destination directory. Persist the title as the task display name using a machine-readable yt-dlp print field, not stderr scraping. Keep `--no-playlist`, cookies, and group kill as they are.
- Contracts: `MediaFetcher::fetch` return path; task label in CLI and GUI. If the title cannot be returned without a signature change, extend the trait in core and both binaries together.
- Alternatives: keep `%(title)s` and document Windows as unsupported for trailing dots.
- Risks: tests that assert the title appears in the file name.
- Acceptance: a title ending in `.` or ` ` does not create that path; the UI/CLI label still shows the title; cancel still kills the process group.
- Verification: `humm-media` fetch unit tests with a stub binary.

### P-007 — keep the valid prefix of a bad word-level UTF-8 buffer

- Objective: match Buzz `main`'s fallback when a word buffer is not valid UTF-8.
- Findings: F-009
- Design: if `from_utf8` fails, lossy-decode, strip `U+FFFD`, trim, and emit when the result is non-empty. Leave `flush_complete_chars` and valid UTF-8 words alone.
- Contracts: `group_words` / `append_word` output for word timestamps.
- Alternatives: keep dropping the word.
- Risks: a test that expected silence for an incomplete sequence.
- Acceptance: a buffer of valid UTF-8 plus one incomplete trailing byte emits the valid text and no replacement character; a fully valid buffer is unchanged.
- Verification: `cargo test -p humm-whisper --lib words`.

### P-008 — more than one custom ggml model

- Objective: parity with Buzz's custom-model registry, without a new inference backend.
- Findings: F-010
- Design: a small persisted list of `{id, name, path}` plus the existing `ggml-custom.bin` as the legacy entry, selected by id from the model control. Do not invent a second file format if TOML settings can hold the list. Re-read `whisper_cpp_custom_models.py` before coding; this proposal is not that design.
- Contracts: `ModelSize::Custom`, settings, GUI model combo, CLI custom-model flags.
- Alternatives: named exception, keep one slot.
- Risks: migrating an existing cache file. The legacy file must keep working.
- Acceptance: two distinct local files can be chosen across runs; the old single file still loads; no network fetch is added.
- Verification: registry unit tests and one GUI or CLI selection test. Do not start this in the same change as P-001/P-002.

### P-009 — decide FunASR; do not port it here

- Objective: make the scope cut explicit against Buzz `main`.
- Findings: F-011
- Design: no engine, no Python, no `transformers` dependency. After review, either record a named parity exception next to ADR-008 / Q-2, or open a separate design for a non-Python FunASR path. This proposal's only acceptable implementation is the written decision.
- Contracts: ADR-008, Q-2, Hugging Face repo resolution.
- Alternatives: start a candle/ort FunASR port now. Not justified, and it would mix a new backend into the CVE fixes.
- Risks: the exception drifts and the next audit files FunASR again. The decision text has to cite `298dbf0`.
- Acceptance: one reviewed doc update states in or out of scope. No new crate.
- Verification: the doc cites `transformers_whisper.py` FunASR entry points and ADR-008.

### P-010 — point behavior docs at Buzz main

- Objective: stop describing v1.4.5 as the behavior ground truth.
- Findings: F-012
- Design: update the ground-truth sentences in `docs/analysis/FEATURE_INVENTORY.md` and `docs/analysis/EXTERNAL_SURFACE.md`, and any spec header that says behavior was verified only against v1.4.5, to cite `298dbf003505cf945bdfd1eb8e705348b59a5b34`. Leave `scripts/ci_prepare.py` `BUZZ_COMMIT` and the two blob hashes alone unless a later check shows those blobs moved. Do not edit `CLAUDE.md`, `RESUME.md`, or `.github/workflows/ci.yml` for this.
- Contracts: none at runtime.
- Alternatives: leave the analysis docs stale.
- Risks: a wholesale doc rewrite. Only the ground-truth sentences. Do not restamp every historical ADR.
- Acceptance: those two analysis files name `298dbf0` as the behavior tree and still mention that the CI fixtures match both commits. `git diff` for `scripts/ci_prepare.py` is empty.
- Verification: the blob SHA check already done in this ledger, repeated if `BUZZ_COMMIT` is ever moved.

### P-011 — do not change humm to clear host ffmpeg CVEs

- Objective: record the host advisories without a decode-behavior change.
- Findings: F-013
- Design: no argv change, no vendored ffmpeg, no demuxer allowlist. The fix is an updated OS ffmpeg when the distributor publishes one. humm keeps ADR-003.
- Contracts: `AudioDecoder` / playback / video still call system ffmpeg.
- Alternatives: `-protocol_whitelist` or a codec allowlist. Rejected; it drops files Buzz and humm currently open, and it was not shown to fix these two CVEs.
- Risks: crafted media can still hit a vulnerable system ffmpeg.
- Acceptance: no diff under `crates/humm-media` for this proposal. The ledger keeps the two Ubuntu CVE ids.
- Verification: `git diff -- crates/humm-media` empty for the CVE pass.

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
