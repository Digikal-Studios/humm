# humm architecture audit

Lead-investigator pass only. No adversarial review was run in this turn. Nothing here is an approval to implement.

Baseline: `125a54da3d284ba3be651b4c99a4456ce56cd76a` on `audit/architecture-review` (worktree `/workspace/.worktrees/humm-audit`). `main` was not modified. Buzz was read from a clone outside this repo (`/tmp/audit-sources/buzz` at `298dbf003505cf945bdfd1eb8e705348b59a5b34`, 2026-10-27, “Fix for preference window size (#1677)”). It was not copied into the humm tree.

Behavior target is that Buzz `main`, not the v1.4.5 tag `aaf98634`. The two CI fixture blobs named in `docs/ci.md` are unchanged at `298dbf0` (same Git blob SHAs). The tag is still a valid fixture pin and is not a valid behavior pin.

## Assessment

The crate split still matches `docs/design/ARCHITECTURE.md`: binaries depend on adapters, adapters implement core traits, and core does not import ffmpeg, whisper, SQLite, `ort`, or `deep_filter`. The problems worth changing are a short list of reachable dependency advisories and a handful of concrete gaps against current Buzz `main`. Most of the post-v1.4.5 Buzz history was not re-derived and is not claimed as a defect.

Google's Rust style guide was applied only where a pattern is harmful. Chromium/Fuchsia rules (one file per crate, no `std::thread`, no `async`) do not fit this workspace and were not scored. `catch_unwind` around a task is the documented queue-isolation design, not a style defect.

## Findings

### F-001 — CoreML zip extraction follows symlink targets outside the destination

- Category: demonstrated defect
- Severity: high. Confidence: high
- Advisory: `RUSTSEC-2026-0368` / `GHSA-4v9h-p7m9-rh26`; `RUSTSEC-2026-0369` / `GHSA-4684-h2q9-2vcp`. Same component also matches `RUSTSEC-2026-0367` / `GHSA-vv5m-jxxh-4627` (SUID/SGID bit preservation)
- Component: direct `zip` 8.6.0, called from `crates/humm-core/src/models/download.rs` `extract_zip_archive` (`ZipArchive::extract`)
- Reachable: yes for the symlink advisories. The function extracts a downloaded CoreML encoder zip. A crafted archive can plant a symlink inside the model directory whose target is outside it. SUID preservation is not a practical escalation for an unprivileged desktop user; it is fixed by the same bump and is not an independent reason to change behavior
- `zip` 6.0.0 (via `ndarray-npy`, not humm's extract path) matches the same three IDs and is not reachable: humm never calls `extract` on that copy. Do not upgrade `ndarray-npy` or `speakrs` for this
- Also checked and not applicable: `RUSTSEC-2025-0168` / `CVE-2025-29787` is patched for both zip versions (`>= 2.3.0`)
- Action: bump the direct `zip` dependency to `>= 9.0.0` and keep `extract` for benign archives. See P-001
- Benefit: a malicious or substituted encoder zip cannot redirect later writes outside the model dir
- Risk: zip 9 may break the `extract` call. A bad bump fails the existing extract tests
- Validation: existing `extract_zip_archive_*` tests, plus a fixture whose symlink target leaves the destination and must be rejected
- Simpler alternative: leave the bump and rely on HTTPS to the constructed Hugging Face URL. That does not hold once the bytes are attacker-controlled, including via the emSign trust issue in F-002

### F-002 — HTTPS trusts a distrusted emSign root

- Category: demonstrated defect
- Severity: high. Confidence: high
- Advisory: `CVE-2026-111943`, `RUSTSEC-2026-0381` (`webpki-roots` 1.0.9, patched `>= 1.0.10`), `RUSTSEC-2026-0382` (`rustls-webpki` 0.103.13, patched `>= 0.103.16 < 0.104.0-alpha.1`)
- Reachable: yes. `humm-core` uses `reqwest` with `rustls-tls` for model downloads, OpenAI-compatible calls, telemetry, and the update check. Those clients use this root bundle and this verifier
- `RUSTSEC-2026-0383` is the same CVE in `webpki-root-certs` 1.0.9 via `ureq`. No humm source calls `ureq`. Not reachable. Do not upgrade `ureq` for it
- Action: `cargo update -p webpki-roots -p rustls-webpki` within the existing reqwest 0.12 constraints. See P-002
- Benefit: connections stop accepting chains through the published emSign intermediates
- Risk: a server that still chains through that root will fail TLS. That is the intended outcome
- Validation: lockfile versions, then `cargo test -p humm-core --lib` for the download and update units. No URL or payload change
- Simpler alternative: none that keeps the current roots

### F-003 — rustls accepts some TLS 1.3 handshake messages at the wrong encryption level

- Category: demonstrated defect
- Severity: low. Confidence: high
- Advisory: `RUSTSEC-2026-0285` / `GHSA-2mjx-qc3c-rqvc`. `rustls` 0.23.42, patched `>= 0.23.45`. Unaffected `< 0.23.13`
- Reachable: yes, on the same reqwest client as F-002. The advisory's own impact is narrow: the handshake transcript stays authenticated, so a network attacker cannot complete a modified handshake. A peer can send handshake messages that should have been encrypted
- Action: include in P-002 (`rustls >= 0.23.46`, which also covers F-004)
- Benefit: the client rejects the non-compliant record
- Risk: none expected for well-formed servers
- Validation: version check plus the existing HTTP unit tests
- Simpler alternative: skip this ID and only take the emSign bump. Not worth a second pass; 0.23.46 covers it

### F-004 — rustls can stick a later connection to a classical key share

- Category: demonstrated defect
- Severity: medium. Confidence: medium
- Advisory: `RUSTSEC-2026-0349` / `GHSA-w44r-93j7-qv28`. `rustls` 0.23.42, patched `>= 0.23.46`
- Reachable: yes if the rustls client stores the server-selected group before the server is authenticated, which is the crate's client behavior and is what reqwest uses. Impact is loss of hybrid post-quantum confidentiality on a later connection to that name, not an authentication bypass. Confidence is medium because this audit did not trace reqwest's session-store call path line by line
- Not reachable, same crate, do not treat as reasons to change behavior on their own:
  - `RUSTSEC-2026-0346` / `GHSA-c8m5-c952-6cm4`: secret leakage only if a custom `ClientSessionStore` debug-formats session values. humm does not
  - `RUSTSEC-2026-0348` / `GHSA-h2v8-394m-p7vv`: Brotli certificate decompression. The resolved `rustls` 0.23.42 dependency list has no `brotli` crate, so the feature is off
  - `RUSTSEC-2026-0347` does not apply: unaffected `< 0.23.44`, and 0.23.42 is in that range
- Action: same bump as P-002
- Simpler alternative: ship only F-002. F-004 is fixed by the same crate version, so splitting them saves nothing

### F-005 — a second Ctrl-C leaves demucs and yt-dlp process groups running

- Category: demonstrated defect
- Severity: medium. Confidence: high
- Location: `crates/humm-cli/src/run.rs` `install_ctrlc` calls `std::process::exit(130)` on the second SIGINT. `crates/humm-media/src/lib.rs` `spawn_own_process_group` detaches the child from the terminal group. ADR-023 already records this and calls it unfixed
- Consequence: the first Ctrl-C cancels through `CancelToken`. The second skips `Drop` and does not signal the detached group, so demucs, yt-dlp, or an ffmpeg grandchild can keep running and writing after the CLI has exited. The same `process::exit` skips `TempAudioGuard` (`queue.rs`), so `_speech` / denoised temp files from the in-flight job are left behind. One-shot ffmpeg decode and the video streamer kill only the direct child (`decode.rs`, `video.rs`); they do not use the process-group helper. That is a smaller gap: those commands do not themselves spawn a tool that forks grandchildren the way demucs and yt-dlp do
- Action: P-003
- Benefit: an immediate abort also stops the tools humm started
- Risk: a wrong process-group id could signal an unrelated group. The registry has to record only groups this process created
- Validation: the existing grandchild-kill unit tests, plus a second-signal test that the group is dead after exit
- Simpler alternative: leave it. ADR-023 accepted it as debt, and the second signal is documented as immediate exit. That alternative keeps orphaned compute and file writes

### F-006 — short timestamp fractions are read as whole milliseconds

- Category: demonstrated defect
- Severity: high. Confidence: high
- Location: `crates/humm-core/src/viewer/timestamp.rs` `parse_timestamp` parses the fractional group with `parse::<i64>()`. Current Buzz `buzz/widgets/transcription_viewer/transcription_segments_editor_widget.py` `parse_timestamp` does `int(fraction[:3].ljust(3, "0"))` (commit `6186d09a9f`)
- Consequence: humm reads `1.2` as 1.002 s and `1.45` as 1.045 s. Buzz `main` reads them as 1.200 s and 1.450 s. Three-digit fractions still agree, which is why the existing round-trip tests do not catch it. The module comment and `trailing_extra_fraction_groups_are_ignored_not_rejected` freeze the v1.4.5 reading
- Action: P-004
- Benefit: typed timestamps match the current editor
- Risk: a user who relied on humm's short-fraction meaning would see different edits. That meaning is the bug relative to the behavior target
- Validation: unit cases `1.2` → 1200 ms, `1.45` → 1450 ms, `1.4567` → 1456 ms, and the existing three-digit round trip
- Simpler alternative: leave the tests as the contract. That keeps humm on the pre-fix editor

### F-007 — import and folder watch omit m4b, aac, and m4v

- Category: demonstrated defect
- Severity: medium. Confidence: high
- Location: `crates/humm-core/src/folderwatch.rs` `SUPPORTED_EXTENSIONS` (13 names). Buzz `main` `buzz/transcriber/transcriber.py` `SUPPORTED_AUDIO_EXTENSIONS` / `SUPPORTED_VIDEO_EXTENSIONS` add `.m4b`, `.aac`, and `.m4v` (commit `56f1d49635`)
- Consequence: those files are skipped by folder watch and import filters even when ffmpeg can decode them
- Action: P-005
- Benefit: the allowlist matches current Buzz
- Risk: a host ffmpeg without the demuxer fails the task the same way any other undecodable file does
- Validation: folder-watch unit tests for the three suffixes, and a decode contract only if ffmpeg on CI already accepts them
- Simpler alternative: leave the list. That permanently drops formats Buzz now accepts

### F-008 — URL downloads are named from the media title

- Category: demonstrated defect
- Severity: medium. Confidence: high
- Location: `crates/humm-media/src/fetch.rs` output template `%(title)s.%(ext)s`. Buzz `main` `file_transcriber.py` `_download_from_url` uses `source.%(ext)s` and keeps the title only as `display_name` (commit `4c18d46d06`). The Buzz comment states that yt-dlp normalizes trailing dots and spaces differently on Windows
- Queue order is already the fixed Buzz order: `queue.rs` `fetch_url` runs before `SpeechIsolator::isolate`. That part is not a gap
- Action: P-006
- Benefit: the working file name no longer depends on the remote title
- Risk: callers that show the downloaded file name as the task title would lose the title unless it is stored separately
- Validation: a stub yt-dlp whose title ends in a dot or space still yields one local media file under the destination directory, and the task label still carries the title
- Simpler alternative: leave title templates and accept the Windows file-name failure Buzz already fixed

### F-009 — a word-level token buffer that is not valid UTF-8 is dropped

- Category: demonstrated defect
- Severity: medium. Confidence: high on the code difference, medium on how often users hit it
- Location: `crates/humm-whisper/src/words.rs` `append_word` emits only when `str::from_utf8` succeeds. Buzz `main` `whisper_cpp.py` still returns false from `_append_word` on `UnicodeDecodeError`, then the caller at lines 390–393 decodes with `errors="replace"`, strips `U+FFFD`, and keeps the remaining text (commit `eadf4ec352`)
- humm already reassembles split UTF-8 characters in `flush_complete_chars`, and segment text comes from whisper-rs strings rather than whisper-cli's latin-1 JSON. The remaining gap is the invalid-buffer fallback, not the whole token path
- Action: P-007
- Benefit: a split multi-byte character at a word boundary keeps the valid prefix instead of deleting the word
- Risk: tests that assert the word is absent would need to expect the prefix. Valid UTF-8 output should be unchanged
- Validation: a unit buffer ending in an incomplete UTF-8 sequence emits the preceding characters and does not emit `U+FFFD`
- Simpler alternative: keep dropping the word. That matches v1.4.5 and loses the characters Buzz `main` keeps

### F-010 — one custom ggml slot versus Buzz's custom-model list

- Category: architectural concern
- Severity: medium. Confidence: high
- Location: `crates/humm-core/src/models/registry.rs` `custom_model_file_name` returns only `ggml-custom.bin`. Buzz `main` `buzz/settings/whisper_cpp_custom_models.py` stores a JSON list of named models and still surfaces the legacy single file (commit `f70a02fcd5`)
- Consequence: a user who registers more than one local ggml model in current Buzz cannot do that in humm. The single slot still matches pre-change Buzz and ADR-008
- Action: P-008, after the smaller parity fixes. Not part of the CVE patch
- Benefit: custom-model parity with current Buzz without a second inference backend
- Risk: settings migration and the GUI model list. Getting the legacy file wrong orphans an existing download
- Validation: one legacy `ggml-custom.bin` still resolves; two registered files are selectable and are not copied onto each other
- Simpler alternative: leave the single slot and record a named exception. That is reasonable if multiple custom models are unused

### F-011 — FunASR exists on Buzz main and is out of humm's scope

- Category: architectural concern
- Severity: medium. Confidence: high that the feature exists; the remedy is a product decision
- Evidence: Buzz `main` `buzz/transformers_whisper.py` transcribes FunASR models (`is_funasr_model`, commit `851c8b3593`). humm ADR-008 / Q-2, approved against the older tree, rejects non-ggml Hugging Face models. There is no FunASR path in the crates
- Consequence: a FunASR repo id that Buzz `main` runs is unsupported in humm. Implementing it as in-process Python would break the standing policy
- Action: P-009. Do not start an engine in the implementation pass
- Simpler alternative: record a named parity exception and leave ADR-008 in force. That is the right outcome unless the behavior target is explicitly widened

### F-012 — docs still describe v1.4.5 as the behavior ground truth

- Category: architectural concern
- Severity: low. Confidence: high
- Locations: `docs/analysis/FEATURE_INVENTORY.md` and `docs/analysis/EXTERNAL_SURFACE.md` name Buzz v1.4.5 as ground truth. `docs/ci.md` and `scripts/ci_prepare.py` pin `aaf98634` for two testdata files. Those two blobs match `298dbf0`, so the fixture pin is not stale. The behavior sentences are
- The 35 commits from the tag to `298dbf0` are listed in the ledger. Only F-006 through F-011 were confirmed as concrete gaps. The rest were not claimed
- Action: P-010. Do not retarget `BUZZ_COMMIT` just to make the audit look current
- Simpler alternative: leave the analysis docs and rely on this review file. The next implementation pass would then keep citing the wrong tree

### F-013 — host ffmpeg is inside two Ubuntu tracker entries

- Category: demonstrated defect in the host tool, not in humm's sources
- Severity: medium. Confidence: medium
- Installed tool: `ffmpeg` `7:6.1.1-3ubuntu5` (`ffmpeg version 6.1.1-3ubuntu5`). OSV query for ecosystem `Ubuntu:24.04:LTS` / package `ffmpeg` / that version returned:
  - `UBUNTU-CVE-2024-32228` — summary names a HEVC SEGV in FFmpeg 7.0. The Ubuntu 24.04 package is in the affected set with no fixed event in the payload. Whether 6.1.1 actually contains that bug was not reproduced
  - `UBUNTU-CVE-2025-59734` — use-after-free write in the SANM decoder when a crafted animation uses subversion < 2. Same package entry, no fixed event in the payload
- Reachable: humm runs `ffmpeg -i <user file>` (`crates/humm-media/src/decode.rs`) with no demuxer allowlist, so a crafted file can enter whatever decoder ffmpeg probes. That is the reachability argument. It is not a proof that these two bugs fire on this build
- Fix: the distribution package, not a humm logic change. See P-011. Do not add a protocol or codec allowlist to “clear” these; that would change which files transcribe
- humm does not vendor ffmpeg. `whisper.cpp` (whisper-rs-sys 0.15.0 / whisper.cpp 1.8.3, as documented in `humm-whisper`) and `onnxruntime` (`ort` `=2.0.0-rc.12`) have empty GitHub security-advisory lists for `ggml-org/whisper.cpp` and `microsoft/onnxruntime`. That is not an NVD clearance
- Simpler alternative: already the recommendation — no product diff

### F-014 — a failed digest lookup still accepts the model file

- Category: demonstrated defect
- Severity: medium. Confidence: high
- Location: `crates/humm-core/src/models/download.rs` `verify_or_warn`. `Ok(None)` (the publisher has no digest) and `Err` (the digest request failed) both log and return `Ok(())`, then the caller renames the partial into place
- Consequence: for an official repo that does publish a digest, a failed tree-API lookup — timeout, reset, or a response the client will not trust — skips SHA-256 and keeps the bytes from the file URL. `Ok(None)` for custom URLs and repos that publish no digest is the documented no-digest path and should stay. The `Err` arm is the gap. It gets worse if F-002's trust bundle is still in use
- Action: P-012
- Benefit: a model file is not promoted when its expected digest could not be checked
- Risk: a blip talking to the Hugging Face tree API fails an otherwise finished download. The existing partial/resume path can retry
- Validation: a stub digest endpoint that returns 500 or resets must not rename the partial; a repo with no `lfs.oid` still promotes
- Simpler alternative: leave the warn-and-accept behavior so a digest outage never blocks transcription. That keeps a substituted file indistinguishable from a verified one

## Evidence reconciled after the report

Three read-only passes finished after the first commit. Their notes are in the project store, not in this worktree: `internal/audit/architecture-audit-evidence-2026-10-28.md` and `internal/architecture-audit-evidence-2026-10-28.md`. Incorporated above: temp-file leak on the second Ctrl-C (F-005) and the digest fail-open (F-014). Not promoted, because the code or an ADR already treats them as policy: ADR-018 reporting success when the history write failed; the ADR-017 lock-failure fallback; OpenAI cancel detaching the request thread (ADR-022); the untimed `Client::new()` fallback only after two timed builds fail; CLI `reducer` / `lang_detector` left `None` with an explicit comment and `HUMM_SURFACE` deviation; GUI binaries constructing adapters; English strings in kittest labels; folder-watch symlink files being eligible. None of those became a new proposal.

## Checked and not proposed

| Item | Why it is not a change |
|---|---|
| `RUSTSEC-2026-0386` `tokio` 1.53.1 | Informational unsoundness in `AsyncFd::new` and related constructors. humm does not call them. reqwest's HTTP stack does not need a tokio upgrade to clear a path humm never hits |
| `RUSTSEC-2026-0274` `rtrb` 0.3.4 | Affected functions are `ReadChunk::commit` / `commit_all` when `Drop` panics. Capture uses `Producer`/`Consumer` `push`/`pop` of `f32` only |
| `quinn-proto` 0.11.16 (`RUSTSEC-2026-0338` through `0342`) | Panic requires unordered QUIC reads. `reqwest` is built with `default-features = false` and without `http3`. The lockfile lists optional deps even when inactive (`js-sys` is listed the same way) |
| `CVE-2026-55093` / `RUSTSEC-2026-0217` `tract-nnef` 0.19.16 | `read_tensor` on attacker NNEF `.dat`. `humm-dfn` loads the git-pinned embedded ONNX model and does not take an NNEF path |
| `RUSTSEC-2026-0194` / `0195` `quick-xml` 0.39.4 | Dependents in the lockfile are `wayland-scanner` (build-time protocol XML), not user documents |
| `RUSTSEC-2026-0257` `webbrowser` 1.2.1 | humm opens the release page, logs, and model dir with the `open` crate. No `webbrowser::open` call |
| Unmaintained advisories (`paste`, `ttf-parser`, `anymap2`, `zvariant`, `zbus_names`, `zvariant_derive`) and unsound `memmap2` 0.5.10 / `event-listener` 5.4.1 | No demonstrated humm call into the vulnerable behavior. Not a license to upgrade those stacks |
| Caret-requirement advisories that failed a first parse (`ordered-float` 5.3.0, `nix` 0.31.3, `rand_core` 0.6.4 and 0.10.1, `string-interner` 0.14.0, `enum-map` 2.7.3, older `quinn` / `quinn-proto`) | Re-read: each resolved version matches a patched or unaffected range |
| Skip-already-transcribed square brackets (Buzz `053b388ae2`) | humm matches by literal prefix (`skip.rs` `candidates`), which is the safe effect of `glob.escape`. Not the glob bug |
| URL-before-isolation order (Buzz `23d848b0b6`) | Already the humm queue order |
| ADR-013 title (“one tokio runtime”) | Stale relative to `ARCHITECTURE.md`, which records that no tokio runtime was adopted. Transitive `tokio` remains. Doc drift only |
| Whole-file playback memory | Accepted in ADR-019. Not re-measured here |
| Schema `ALTER` then `PRAGMA user_version` as two statements | A crash between them could make the next open fail on a duplicate column. Not observed. Leave it |
| `unsafe` in `humm-whisper` | SAFETY comments match the whisper-rs 0.16 abort-callback workaround in ADR-002. No new evidence the callback outlives `full()` |

## Coverage

| Subsystem | Depth |
|---|---|
| Workspace layout, traits, ADRs vs crate graph | deep |
| `humm-core` queue cancel, zip extract, timestamp parser, export path, folder-watch extension list | deep |
| `humm-core` OpenAI client, translator, live session | sampled |
| `humm-store` migration, stale flip, mutex poison recovery | deep on those paths; rest sampled |
| `humm-media` argv, process groups, yt-dlp template | deep |
| `humm-whisper` unsafe and word UTF-8 | deep |
| `humm-audio` capture ring | tool-checked (`push`/`pop` of `f32`); callback body not line-audited |
| `humm-dfn`, `humm-diarize` | tool-checked and dependency-audited; numerics not re-derived |
| `humm-cli` Ctrl-C | deep |
| `humm-gui` | sampled (secret redaction, `open::that`, custom-model slot) |
| Tests that encode old Buzz | deep for timestamp; not a suite-wide pass |
| Benches, ignored contract tests | unexamined / not run |
| Buzz `main` vs tag | the 35 commit subjects were listed; behavior was read for the items in F-006–F-011 and the “not proposed” rows. Folder-watch crash and duplicate entries, resizer merge-by-gap, speaker-id CLI, preference window size, XDG secret delay, multi-file CLI, and demucs RAM were not compared |

## Baseline

| Check | Result |
|---|---|
| `git rev-parse HEAD` | `125a54da3d284ba3be651b4c99a4456ce56cd76a`, clean `main` before the worktree |
| `cargo-audit` | not installed |
| RustSec DB | cloned outside the repo at `99670cfc0bc574433034b75876a4b791ccaa2efb` (2026-10-27). Lockfile matched locally. OSV used for the key crates and for Ubuntu ffmpeg |
| Default `rustc` | 1.83.0, which cannot parse edition 2024. Not a product defect. No `rust-toolchain` file. CI installs stable |
| `cargo +stable fmt --check` | pass, rustc 1.99.1, run in the audit worktree |
| `cargo +stable clippy --workspace --all-targets -- -D warnings` | exit 101. After VM packages for OpenSSL, libstdc++, ALSA, and OpenBLAS, `speakrs` 0.5.0 stops the build: `compile_error!("the coreml feature is only supported on macOS")` at `speakrs-0.5.0/src/lib.rs:204`. The workspace enables that feature (ADR-024). CI is macOS. This Linux VM cannot finish workspace clippy or tests without changing that feature, which this audit does not do |
| `cargo test --workspace`, `cargo bench` | not run. Same `speakrs`/`coreml` build stop |
| `yt-dlp` | not on `PATH`. ffmpeg and ffprobe are |

`openssl-sys` is a build requirement of the pinned `ort` feature set, not of humm's HTTP client. This audit does not propose turning that feature off.
