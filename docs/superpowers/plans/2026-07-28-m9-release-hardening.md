# M9 — Release & hardening — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship humm 1.0.0 as a real installable macOS app bundle published to a public GitHub mirror (flipping F-006 and exercising the M7 keychain-ACL path), close the six robustness items and three test-debt items from the M8 debt register, close the remaining matrix exceptions (F-031/F-111 via ollama, F-042 via a gated contract), and record hold/bump verdicts on ort/deep_filter/speakrs — ending at matrix 50/50 (or explicitly-permanent exceptions only) with no transcription-path bench regression.

**Architecture:** A new dev-only `crates/xtask` workspace member owns bundle assembly (Info.plist, icon, dylib-closure copy + `install_name_tool` fixups into `Contents/Frameworks/`, pluggable ad-hoc/Developer-ID signing, `hdiutil` DMG, `version_info.json` generation) with Apple's own CLIs as the only non-Rust tools (ADR-025). Robustness fixes land store-layer (one-transaction copy+complete, requeue segment clear), humm-media-layer (pgid registry + kill-all, shared bounded stderr), core-layer (`copy_then_rename` promoted to `humm-core::fsutil`), and humm-dfn-layer (streamed output write).

**Tech Stack:** Existing workspace + `png` crate (xtask-only, icon generation). No new runtime dependencies. Apple CLIs: `codesign`, `notarytool`, `stapler`, `hdiutil`, `install_name_tool`, `otool`, `iconutil`. `gh` CLI for publishing (Jawad-authenticated). ollama (local, F-031/F-111 contracts only).

**The governing spec:** `docs/superpowers/specs/2026-07-28-m9-release-hardening-design.md` — every task's implementer MUST read it first (§0 + the §§ their task names). The M8 debt register lives at the MAIN checkout's `.superpowers/sdd-m8/task-19-report.md` §D.5b for provenance questions.

## Global Constraints

1. Every commit passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` with `export CARGO_HOME="$PWD/.cargo-home"`. **This worktree's `.cargo-home` is unseeded** — Task 1 seeds it (`cp -R /Users/jawad/RustroverProjects/humm/.cargo-home .cargo-home` + one sandbox-lifted `cargo fetch`). **The sandboxed baseline is ZERO failures** (committed `.claude/settings.json` allows loopback binds + pypi/pythonhosted/huggingface; proven 1921 passed / 0 failed at M8 close) — any test failure is real. Commit messages: `M9: <what> (F-### or D5b-item#)`.
2. Sandbox: GPU/Metal (SEGFAULT — never bench/windowed sandboxed), CoreAudio, FSEvents, writes outside the worktree are blocked. Windowed runs: sandbox lift + `caffeinate -dimsu` + CGEvent automation (never System Events click-at). `⌘,` is captured by the system Input Sources hotkey (gui-manual/M8.md §8.3) — not a humm bug.
3. **STOP-gates (Jawad-only actions; Claude never does these):** anything touching the keychain or certificates (signing identity discovery/creation, `notarytool store-credentials`); GitHub repo creation, Forgejo mirror configuration, `gh auth login`; the final `gh release create` needs Jawad's explicit go-ahead in-session; removal of the `elated-austin-1982b0` worktree.
4. Never `std::env::set_var` in tests — thread values explicitly (constructor seams).
5. New public core items need doc comments (`#![warn(missing_docs)]`); any new user-visible GUI string lands in **all 15** ftl catalogs in the same commit (none are expected this milestone — if one appears, the ×15 rule applies).
6. Buzz source remains ground truth for any behavior question; docs discovered wrong get fixed in the same commit.
7. `crates/xtask` is `publish = false`, never enters any shipped artifact, and is the only crate allowed to invoke Apple CLIs.
8. Network-dependent tests stay `#[ignore]`/env-gated; the new F-042 contract follows the existing double-gate convention.

## File structure (created/modified across the milestone)

```
LICENSE                          # NEW: MIT, Jawad Alnajjar
NOTICE                           # NEW: third-party attribution (verified licenses)
docs/THIRD_PARTY_LICENSES.md     # NEW: generated crate-license summary
Cargo.toml                       # [workspace.package] version=1.0.0, repository→GitHub,
                                 #  members += crates/xtask; humm-core dep version→1.0.0
crates/xtask/                    # NEW: bundle/notarize/version-info/gen-icon subcommands
assets/icon/humm.icns            # NEW: committed original icon (xtask gen-icon output)
crates/humm-core/src/
  fsutil.rs                      # NEW: copy_then_rename (moved from humm-media/demucs.rs)
  pipeline.rs                    # move_file fallback → fsutil::copy_then_rename
  update.rs                      # VERSION_INFO_URL → github.com/digikal/humm
  models/download.rs (tests)     # F-042 contract test (double-gated)
crates/humm-store/src/store.rs   # +copy_replaced_completed (one tx); requeue_task clears segments
crates/humm-media/src/
  lib.rs                         # pgid registry + kill_all_registered_process_groups()
  stderr.rs                      # NEW: shared bounded stderr-tail helper
  decode.rs, encode.rs           # unbounded read_to_end → bounded helper
  fetch.rs, demucs.rs            # private tail copies → shared helper; registry hooks;
                                 #  DemucsIsolator::available() re-probes (OnceLock dropped)
crates/humm-dfn/src/lib.rs       # streamed output write (header-patch temp + rename)
crates/humm-diarize/src/lib.rs   # non-gated speakrs API-surface tests
crates/humm-cli/src/run.rs       # install_ctrlc second-SIGINT → kill_all before exit(130)
crates/humm-gui/src/
  app.rs                         # on_exit → kill_all after shutdown_queue; site-3 comment
  services.rs                    # demucs capability re-probe on options-sheet open
  speaker_id.rs, views/resizer.rs  # → copy_replaced_completed
crates/humm-gui/tests/kittest_phases.rs  # flake fix (bounded run-until)
docs/design/DECISIONS.md         # ADR-025; ADR-015 endpoint note; ADR-024/ADR-008 addenda
docs/benchmarks/M9.md, docs/gui-manual/M9.md, docs/analysis/PARITY_MATRIX.md (FINAL)
CLAUDE.md, docs/design/{ARCHITECTURE,IMPLEMENTATION_PLAN}.md  # as-built sweep
```

---

### Task 1: Absorb the banner-flake fix + establish the zero-failure baseline

**Files:** cherry-pick touches `crates/humm-gui/src/views/main_window.rs`, `crates/humm-gui/src/views/resizer.rs`.
**Spec:** §3 (banner-flake absorption).

- [ ] Seed `.cargo-home` (Global Constraint 1): copy from the main checkout, then one sandbox-lifted `cargo fetch`.
- [ ] `git cherry-pick 3514857` (the commit exists in this repo's object store via the `elated-austin-1982b0` worktree's branch `claude/elated-austin-1982b0`; message "M7: hold locale test lock in tr-reading GUI tests (flake fix)" — keep it as-is, provenance intact). Do NOT remove that worktree (Jawad confirms later, Task 18).
- [ ] Run the full gates: `cargo fmt --check`, clippy, `cargo test --workspace`. Expected: **0 failures** (the proven baseline). Record exact pass counts in the task report — every later task's gate run is compared against this.
- [ ] No new commit beyond the cherry-pick.

### Task 2: Store atomicity — `copy_replaced_completed` + `requeue_task` segment clear

**Files:** Modify `crates/humm-store/src/store.rs`, `crates/humm-gui/src/speaker_id.rs` (:995-1009), `crates/humm-gui/src/views/resizer.rs` (`save_segments`, :244-258), `crates/humm-gui/src/app.rs` (comment only, `start_merge` :2188-2205).
**Spec:** §2.2, §2.6. **Debt:** D5b items 3, 12.
**Produces:** `SqliteStore::copy_replaced_completed(&self, source: TranscriptionId, segs: &[Segment]) -> Result<TranscriptionId, StoreError>` — ONE rusqlite transaction: the `copy_transcription` INSERT..SELECT shape but written directly as `Completed` with `time_ended = now` (never materializing a `Queued` intermediate), then the segment INSERTs (`translation` empty, matching `mark_replaced_completed`). `requeue_task` gains `DELETE FROM transcription_segment WHERE transcription_id = ?1` in the same transaction as its UPDATE (wrap the two statements in `conn.transaction()`).

- [ ] **Investigate first (D5b-12 is "investigate + fix"):** store-level red test `requeue_then_recomplete_leaves_only_second_run_segments` — insert task, `mark_completed` with 3 segments, `requeue_task`, `mark_completed` with 2 segments, assert `get_segment_rows` returns exactly the 2. Run: expect FAIL with 5 rows (confirms the append bug as described; if it passes, stop and re-investigate before touching code).
- [ ] Red tests for the new method: `copy_replaced_completed_inserts_completed_row_with_segments` (source untouched; new row `Completed`, `time_ended` set, `last_export_folder` NULL, segments present, translations empty); `copy_replaced_completed_bogus_source_errors_and_leaves_no_row` (row count unchanged — the atomicity claim: no `Queued` orphan can ever exist because no `Queued` row is ever written).
- [ ] Implement both store changes; update the existing `requeue_task_resets_status_progress_and_clears_timestamps_and_error_only` test (name + assertions now include segment clearing).
- [ ] Switch the two back-to-back call sites to the new method (`speaker_id.rs` save path, `resizer.rs::save_segments` create_new arm); their existing tests must stay green with unchanged observable behavior. **`app.rs::start_merge` is deliberately NOT switched** — its copy-first shape is the Buzz-parity visible-at-0% row and its crash residue is already documented as recovered by the stale-task sweep (app.rs:2267-2274); extend that comment to name this task's decision explicitly.
- [ ] Gates; commit `M9: one-transaction copy+complete + requeue segment clear (D5b-3, D5b-12)`.

### Task 3: pgid registry + kill-all on second-Ctrl-C and GUI shutdown

**Files:** Modify `crates/humm-media/src/lib.rs` (:71-118 helpers region), `crates/humm-media/src/demucs.rs`, `crates/humm-media/src/fetch.rs` (spawn/reap sites), `crates/humm-cli/src/run.rs` (`install_ctrlc` :785-802), `crates/humm-gui/src/app.rs` (`on_exit` :4340-4354), `docs/design/DECISIONS.md` (ADR-023 gap paragraph gets a "closed, M9" note).
**Spec:** §2.1. **Debt:** D5b items 2, 4.
**Produces:** in `humm-media`: `pub(crate) fn register_process_group(child: &Child) -> GroupRegistration` (a Drop-guard that unregisters; registry is a `static LazyLock<Mutex<HashSet<i32>>>` of pgids); `pub fn kill_all_registered_process_groups()` — SIGKILL each registered group via `nix::sys::signal::kill(Pid::from_raw(-pgid), SIGKILL)`, ESRCH swallowed, registry drained (SIGKILL directly: both callers are emergency exits, and group-wide immediate SIGKILL is the existing yt-dlp cancel semantic). Adapters hold the guard for the child's lifetime (demucs + fetch spawn sites).

- [ ] Red unit test (humm-media, mirrors `fetch.rs::cancel_kills_grandchild_process_not_just_direct_child`'s stub technique): `kill_all_registered_groups_kills_grandchildren` — spawn a stub `sh` script whose `sleep 30` grandchild's pid is written to a file, register, call `kill_all_registered_process_groups()`, poll `kill(pid, 0)` until ESRCH for both child and grandchild (bounded ~2 s); `dropped_registration_unregisters` — register, drop the guard, kill_all, child still alive (then clean it up).
- [ ] Implement registry + guard; hook the demucs and fetch spawn/reap sites.
- [ ] **D5b-4 audit ride-along:** grep every `signal::kill`/`Child::kill` site in the workspace; each either already signals the group, gains the registry, or gets a one-line comment stating why single-PID is correct there. List the sites + disposition in the task report.
- [ ] Wire callers: `install_ctrlc` second-signal arm calls `humm_media::kill_all_registered_process_groups()` immediately before `std::process::exit(130)`; `HummApp::on_exit` calls it after `shutdown_queue` (last-resort sweep AFTER cooperative cancellation had its chance — spec wording).
- [ ] Red cli-int test `second_sigint_kills_detached_process_group`: run `humm add <fixture> -e` with a stub demucs on PATH that writes its pgid to a temp file then sleeps 60; send SIGINT twice (first, then second after the "canceling…" line appears on stderr); assert exit code 130 AND the stub's process group is dead (poll `kill(-pgid, 0)` → ESRCH, bounded).
- [ ] Gates; commit `M9: pgid registry + kill-all on second-Ctrl-C and GUI shutdown (D5b-2, D5b-4)`.

### Task 4: Shared bounded stderr + `copy_then_rename` → core + `move_file` back-port

**Files:** Create `crates/humm-core/src/fsutil.rs`, `crates/humm-media/src/stderr.rs`; modify `crates/humm-core/src/lib.rs`, `crates/humm-core/src/pipeline.rs` (`move_file` :489), `crates/humm-media/src/{demucs.rs,fetch.rs,decode.rs,encode.rs,lib.rs}`.
**Spec:** §2.3, §2.4. **Debt:** D5b items 1, 5, 13 (stderr + move_file duplication Minors).
**Produces:** `humm_core::fsutil::copy_then_rename(from: &Path, to: &Path) -> std::io::Result<()>` — the demucs.rs:374 implementation moved verbatim (same-dir temp `.tmp-<pid>`, rename as the atomic last step, temp removed on any failure) with its 3 unit tests moved alongside; `humm-media` call sites import it (private duplicate deleted). `pipeline.rs::move_file` becomes: try `fs::rename`; on error, `copy_then_rename(from, to)` then `fs::remove_file(from)` — the composed body stays 3 lines and the risk lives in the directly-tested helper (the ADR-023 review-round precedent: the fallback helper is unit-tested directly because in-tempdir tests cannot force EXDEV). `humm_media::stderr::spawn_stderr_tail(stderr: ChildStderr) -> JoinHandle<String>` (cap 2 KiB, keeps the LAST cap bytes, `…`-prefixed when truncated — the existing fetch.rs/demucs.rs semantics) adopted by all four sites: fetch.rs:244-254 and demucs.rs:322-334 (replacing their private copies) and decode.rs:147 / encode.rs:72 (previously unbounded). **encode.rs:65 reads STDOUT (the encoded media) — that read is payload, not diagnostics: leave it unbounded and add a comment saying so.**

- [ ] Move `copy_then_rename` + its 3 tests to `fsutil.rs` (doc comments per missing_docs); mechanical re-import in demucs.rs; red test `move_file_fallback_removes_source_after_copy` — call the composed fallback arm directly (extract `fn move_file_via_copy(from, to)` so the non-EXDEV-forceable branch is testable: copy_then_rename + remove_file, asserted on a tempdir pair).
- [ ] Red test for the tail helper: `stderr_tail_caps_oversized_output` (stub emitting 64 KiB → captured tail ≤ 2 KiB + `…` prefix + contains the FINAL bytes); `stderr_tail_passes_small_output_through` (no prefix).
- [ ] Implement; adopt at all four sites; existing decode/encode error-path tests (`decode_corrupt_input_reports_stderr` etc.) stay green.
- [ ] Gates; commit `M9: shared bounded stderr tail + copy_then_rename to core + move_file back-port (D5b-1, D5b-5, D5b-13)`.

### Task 5: Stream `DfNoiseReducer`'s output write

**Files:** Modify `crates/humm-dfn/src/lib.rs` (`reduce` :85-, `enhance` :148-, `write_wav_sibling` :195-), `crates/humm-core/src/wav.rs` only if a streaming writer variant is needed there.
**Spec:** §2.5. **Debt:** D5b item 6 (memory half).
**Produces:** the enhance loop writes i16 frames incrementally to a `BufWriter` over the same-dir temp file as hops complete (WAV header written up-front from the exactly-computable output length — `n_hops × hop_size` — or seek-patched at the end if the length genuinely can't be precomputed; implementer verifies against `DfTract`'s actual output framing), then the existing atomic rename. The full-output `Vec<f32>` accumulation and the intermediate `Vec<u8>` disappear; input decode stays as-is (the spec targets the output copies). Atomic-placement semantics unchanged — the existing `reduce_atomic_placement_leaves_no_partial_or_temp_file_on_write_failure` test must stay green.

- [ ] **Golden first, BEFORE any change:** run the current code on the existing sine fixture (`write_sine_wav`, 0.3 s @ 16 kHz) and commit the output as `crates/humm-dfn/tests/golden/sine_440_denoised.wav` with a red test `streamed_output_is_byte_identical_to_pre_stream_golden` comparing `reduce()`'s output file bytes to the golden. (Deterministic: tract CPU inference, same per-sample math — if a byte diff appears from a legitimate float-path change, stop, compare samples with an epsilon, and document the finding in the task report before deciding.)
- [ ] Implement the streaming rework; all existing humm-dfn unit tests + the golden test green.
- [ ] Measure once (not a test): peak RSS via `/usr/bin/time -l` on a ~30-min synthesized fixture, before-vs-after builds, recorded in the task report + `docs/benchmarks/M9.md` later (Task 17 collects).
- [ ] Optional-if-cheap (D5b-6's other half): if the rework naturally allows reusing one `DfTract` across calls without a Mutex, note it; do NOT force it — per-call model build is the documented ADR-024-adjacent decision.
- [ ] Gates; commit `M9: stream DfNoiseReducer output write (D5b-6)`.

### Task 6: Non-gated humm-diarize speakrs-surface coverage

**Files:** Modify `crates/humm-diarize/src/lib.rs` (tests module).
**Spec:** §3.1. **Debt:** D5b item 10.
**Produces:** non-gated unit tests pinning the exact speakrs API surface `OrtDiarizer` uses, so a breaking speakrs upgrade fails plain `cargo test --workspace`: `from_dir_on_missing_dir_errors_models_missing` (`OwnedDiarizationPipeline::from_dir` on a nonexistent path → `OrtDiarizer`'s `ModelsMissing` mapping — no network, no models, must run sandboxed and fast; verify empirically that ort fails fast on missing files); `from_dir_on_garbage_files_errors` (dir with junk bytes at the expected 10 filenames); construction pins for `ExecutionMode::Cpu` and `speakrs::Segment::new(start, end, label)` field access (extending the existing :325-shape tests); a `to_segments`-path pin if that free function is callable without models (check `speakrs::segment::to_segments`'s signature at implementation; if it needs a model, pin its *signature* via a typed `let _: fn(_) -> _` binding instead — the compile failure is the test).

- [ ] Write the tests red-first where a runtime behavior is claimed (missing-dir/garbage-dir); run sandboxed, confirm fast (<5 s total).
- [ ] Module doc states the honest scope: this seam catches API/loading breaks; silent inference-quality changes still need the gated `HUMM_DIARIZE_CONTRACT=1` test.
- [ ] Gates; commit `M9: non-gated speakrs API-surface coverage in humm-diarize (D5b-10)`.

### Task 7: `DemucsIsolator::available()` re-probe (install-while-running)

**Files:** Modify `crates/humm-media/src/demucs.rs` (the probe cache), `crates/humm-gui/src/services.rs` (:316-320 capability snapshot), `crates/humm-gui/src/app.rs` (options-sheet open path).
**Spec:** §3.3. **Debt:** D5b item 7.
**Produces:** `available()` re-probes on every call (drop the `OnceLock`; the probe is an env-override check + PATH scan — file metadata only, no subprocess; verify at implementation and keep it that way); the GUI refreshes `capabilities.demucs` each time the options sheet opens (the explicit user-visible moment), reading through the same isolator instance. CLI behavior unchanged (`-e` validates at invocation time — already a fresh probe per process).

- [ ] Red unit test (humm-media): `available_sees_binary_installed_after_first_probe` — `from_env` seam pointing `HUMM_DEMUCS` at a not-yet-existing path inside a tempdir; `available()` false; create an executable stub there; `available()` true (no set_var — constructor-injected value, the existing seam).
- [ ] Red kittest (humm-gui): `options_sheet_reprobes_demucs_capability_on_open` — capabilities fixture whose isolator flips availability between two sheet opens; checkbox disabled→enabled across the reopen.
- [ ] Implement; confirm no steady-state per-frame probing (probe only on open).
- [ ] Gates; commit `M9: DemucsIsolator availability re-probe on options-sheet open (D5b-7)`.

### Task 8: `kittest_phases` one-frame-lag flake

**Files:** Modify `crates/humm-gui/tests/kittest_phases.rs` (`download_phases_render_media_then_model_then_complete` :264-; helper `wait_for_label_contains` :240-255).
**Spec:** §3.2 (systematic-debugging mandated).
**Produces:** a deterministic test. Known symptom: one-frame-lag assertion failure under parallel-suite load (M8 memory). The file's own module doc (:16-17) explains the gate-based sequencing hazard; the likely fix shape is replacing any single `harness.run()`-then-assert with the existing bounded `wait_for_label_contains`-style polling — but the root cause must be established first, not assumed.

- [ ] Reproduce: loop the test ≥25× while the full workspace suite runs concurrently (`cargo test -p humm-gui --test kittest_phases` in a shell loop against a parallel `cargo test --workspace`); record the failing assertion + frame state.
- [ ] Root-cause per superpowers:systematic-debugging (read the failure, form hypothesis, verify by instrumentation — not by adding waits blindly). If the cause is NOT the one-frame lag, fix what it actually is and say so in the report.
- [ ] Fix (bounded run-until-condition, no sleeps, no unbounded loops); then the exit criterion: **15 consecutive clean runs under full-suite load** (the banner-fix verification bar).
- [ ] Gates; commit `M9: kittest_phases determinism fix (flake root-cause: <finding>)`.

### Task 9: Release metadata — version 1.0.0, endpoint URL, LICENSE, NOTICE

**Files:** Modify `Cargo.toml` (workspace), every `crates/*/Cargo.toml` (version inheritance), `crates/humm-core/src/update.rs` (:34-35), `docs/design/DECISIONS.md` (ADR-015 endpoint note); create `LICENSE`, `NOTICE`, `docs/THIRD_PARTY_LICENSES.md`.
**Spec:** §1.1, §1.6.
**Produces:** `[workspace.package] version = "1.0.0"`; each crate `version.workspace = true`; the workspace dep table's `humm-core = { path, version = "1.0.0" }`; `repository = "https://github.com/digikal/humm"` (public-facing metadata points at the public mirror). `VERSION_INFO_URL = "https://github.com/digikal/humm/releases/latest/download/version_info.json"`. `LICENSE`: MIT, `Copyright (c) 2026 Jawad Alnajjar`. `NOTICE`: the spec §1.6 list — **every license claim verified against the upstream source in-task** (HF model cards for pyannote/wespeaker artifacts in `avencera/speakrs-models`, the DFN3 weights license from Rikorose/DeepFilterNet, OFL for Noto Sans SC, Buzz's MIT + the ported-fixtures acknowledgment, OpenBLAS BSD-3, libgfortran GCC-runtime-exception, libomp Apache-2.0-LLVM, speakrs/ort/ONNX Runtime, whisper.cpp/ggml/whisper-rs, egui/eframe) — record each verification URL in the task report. `docs/THIRD_PARTY_LICENSES.md`: generated crate summary (`cargo license` — one-time `cargo install cargo-license`, crates.io is sandbox-allowlisted; regeneration command recorded in the file header).

- [ ] Red test first: `update.rs`'s URL tests — assert the const equals the GitHub URL (adjust the existing pins :348-390 region as needed); `grep -rn "repo.digikal.com" crates/ docs/HUMM_SURFACE.md` — every code/surface-doc hit updated or justified in the report (docs/analysis history stays as-written).
- [ ] Version bump + inheritance; `cargo build` confirms `CURRENT_VERSION`/`env!("CARGO_PKG_VERSION")` sites (app.rs:69, services.rs:499,878) now report 1.0.0 (assert via `target/release/humm --version` if the CLI prints it, else a unit test on the const).
- [ ] LICENSE + NOTICE + THIRD_PARTY_LICENSES; ADR-015 endpoint note updated ("digikal/humm GitHub mirror, M9" — keep the old Forgejo note as history).
- [ ] Gates; commit `M9: v1.0.0 + GitHub release endpoint + LICENSE/NOTICE (F-006)`.

### Task 10: `crates/xtask` — skeleton, Info.plist, icon, app assembly (unsigned)

**Files:** Create `crates/xtask/{Cargo.toml,src/main.rs,src/plist.rs,src/bundle.rs,src/icon.rs}`, `assets/icon/humm.icns` (committed output), `.cargo/config.toml` (alias `xtask = "run -p xtask --"`); modify workspace `Cargo.toml` (members), `docs/design/DECISIONS.md` (ADR-025 initial entry).
**Spec:** §1.3 items 1-2 (assembly half). **ADR-025** starts here: xtask pattern, Apple CLIs as the non-Rust boundary, the Accelerate-rejection verdict (speakrs 0.5.0 has no Accelerate backend; upstream ndarray-linalg has none; forking two crates is strictly worse than bundling one dylib closure — cross-referenced from ADR-024's packaging-debt paragraph).
**Produces:** `cargo xtask bundle` (through assembly; closure/signing in Task 11): builds release `humm-gui` + `humm`, assembles `target/bundle/humm.app` — `Contents/MacOS/{humm-gui,humm}`, `Contents/Resources/{humm.icns,NOTICE,LICENSE}`, `Contents/Info.plist`. `plist::render(version: &str) -> String` — CFBundleIdentifier `com.digikal.humm`, CFBundleName/DisplayName `humm`, CFBundleExecutable `humm-gui`, CFBundleShortVersionString + CFBundleVersion from the workspace version (compile-time `env!("CARGO_PKG_VERSION")` of xtask itself — same workspace version — is acceptable and self-updating), CFBundlePackageType `APPL`, NSHighResolutionCapable, `NSMicrophoneUsageDescription` ("humm records audio for live transcription."), LSMinimumSystemVersion from the toolchain deployment target (`otool -l` `LC_BUILD_VERSION` minos of the built binary — read it, don't guess; record in ADR-025). `icon::gen` (subcommand `gen-icon`, dep `png` xtask-only): renders a simple original mark (rounded square + waveform bars) at the iconset sizes, `iconutil -c icns` assembles; output committed so `bundle` never regenerates. `version_info::render(version: &str, notes: &str) -> String` (subcommand `version-info --notes <file>`, consumed by Task 15): emits the exact `update.rs`-parsed schema — `{"version", "release_notes", "download_urls": {"macos_arm": ["https://github.com/digikal/humm/releases/download/v<version>/humm-<version>-macos-arm64.dmg"]}}` — via `serde_json` so escaping is real, never string-formatted JSON.

- [ ] Red unit tests: `plist_contains_bundle_id_version_and_mic_usage` (parse-free string asserts on `render("9.9.9")`); `bundle_layout_places_binaries_and_resources` — `bundle::assemble(&fake_bins_dir, &staging)` on dummy executable files into a tempdir, assert the full layout above; `version_info_round_trips_through_update_parser` — feed `version_info::render("9.9.9", "notes")` to `humm_core::update::parse_version_info(json, "1.0.0", "macos_arm")` and assert the parsed `UpdateInfo` carries the version, notes, and the one DMG URL (xtask dev-depends on humm-core for exactly this test).
- [ ] Implement; run `cargo xtask gen-icon` once (lift only if `iconutil` needs it), commit `assets/icon/humm.icns`; run `cargo xtask bundle` for real — assembled app exists; `target/bundle/humm.app/Contents/MacOS/humm --help` exits 0 (the CLI binary runs from inside the bundle — headless, sandbox-safe).
- [ ] ADR-025 initial entry (decision + Accelerate verdict; as-built numbers land in Task 11).
- [ ] Gates; commit `M9: xtask bundler — app assembly, Info.plist, original icon (F-006, ADR-025)`.

### Task 11: Dylib closure + ad-hoc signing + DMG + OMP silencing

**Files:** Create `crates/xtask/src/dylib.rs`, `crates/xtask/src/dmg.rs`; modify `crates/xtask/src/{main.rs,bundle.rs}`, `docs/design/DECISIONS.md` (ADR-025 as-built + OMP finding; ADR-024 addendum "closed by M9 packaging").
**Spec:** §1.3 items 3-5, §1.4.
**Produces:** `dylib::parse_otool_libs(output: &str) -> Vec<PathBuf>` (skips the self-id line, keeps non-system entries: anything not under `/usr/lib` or `/System/`); `dylib::bundle_closure(app: &Path) -> Result<ClosureReport>` — BFS from both `Contents/MacOS` binaries via `otool -L`, copy each non-system dylib into `Contents/Frameworks/`, `install_name_tool -id @rpath/<name>` on the copy, `-change <old> @executable_path/../Frameworks/<name>` in the binaries and `-change <old> @loader_path/<name>` between dylibs, ad-hoc `codesign --force -s -` each modified Mach-O (arm64 requires it after install_name_tool); post-condition method `verify_no_external_refs` re-walks everything in the bundle and errors on any remaining `/opt/homebrew`/`/usr/local` reference. `bundle` finishes: sign dylibs-then-app inside-out (`--identity` flag → real identity + `--options runtime`; absent → `-s -`); `dmg::create` — staging dir with the app + `ln -s /Applications`, `hdiutil create -format UDZO` → `target/bundle/humm-<version>-macos-arm64.dmg`. `notarize` subcommand: `xcrun notarytool submit --keychain-profile <name> --wait` + `xcrun stapler staple` (STOP-gate: profile created by Jawad; this subcommand is only ever run at his go).

- [ ] Red unit tests on canned `otool -L` output: `parse_skips_system_and_self_entries` (fixture text with `/usr/lib/libSystem`, `/System/Library/...`, `/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib`, an `@rpath/...` line → exactly the homebrew path); `closure_rewrite_plan_is_idempotent` (planning fn on already-rewritten refs → empty plan).
- [ ] Implement; run `cargo xtask bundle` for real. Record the actual discovered closure in ADR-025 (expected: libopenblas + libgfortran + libomp + libgcc_s-family from Homebrew's gfortran runtime; plus `libonnxruntime.dylib` iff ort links dynamically — the §7-risk check: `otool -L` decides, not assumption). `codesign` sandboxed if it works, one documented lift if not.
- [ ] **OMP root-cause (spec §1.4):** launch the bundled `humm-gui` (windowed sandbox lift, brief) with stderr captured; reproduce the OMP warning line; identify the colliding runtimes (`DYLD_PRINT_LIBRARIES=1` run: which libomp/libiomp instances load); fix in the closure step (single bundled OpenMP runtime, all referents rewritten to it) — env-var suppression only if the root cause genuinely demands it, documented in ADR-025 either way. Acceptance here: bundled-app launch stderr is clean.
- [ ] Launch smoke from the bundle (lifted): window appears, transcribe a short fixture end-to-end, quit cleanly.
- [ ] Gates; commit `M9: dylib closure + signing + DMG + OMP silenced (F-006, ADR-025, D5b-9, D5b-11)`.

### Task 12: Dependency watch — ort / deep_filter / speakrs verdicts

**Files:** Task report (verdicts flow into Task 18's milestone report + `docs/design/DECISIONS.md` only if a bump happens).
**Spec:** §5.

- [ ] For each: current upstream state (crates.io API / GitHub releases — both sandbox-reachable), changelog/diff read against the API surfaces humm actually touches (humm-diarize's speakrs usage incl. the Task 6 pins; ort transitively; humm-dfn's deep_filter git pin).
- [ ] Watch explicitly for: ort GA (would end rc-pin risk), a speakrs Accelerate backend (would reopen the ADR-025 bundling trade-off — note only, no action this milestone).
- [ ] Verdict per dep: hold (with reason) or bump. **A bump additionally requires, in the same commit: the gated contract suite for that adapter run and passing** (`HUMM_DIARIZE_CONTRACT=1` / `HUMM_DFN_CONTRACT=1`), per Standing Policy rule 5. Expected outcome is three holds — that is a valid result.
- [ ] Commit only if something bumped; otherwise verdicts land in the report. (If bumped: `M9: <dep> <old>-><new> — contract-revalidated`.)

### Task 13: F-042 contract test against a fixed third-party HF repo

**Files:** Modify the humm-core test file holding the `ensure_artifact` real-network contracts (locate via `grep -rn "real huggingface" crates/humm-core` — follow its file + gating conventions exactly), `docs/analysis/PARITY_MATRIX.md` (F-042 row), `docs/design/DECISIONS.md` (ADR-008 addendum note).
**Spec:** §4.2.
**Produces:** `#[ignore]` + `HUMM_HF_THIRDPARTY_CONTRACT=1`-gated test `third_party_hf_repo_resolves_and_custom_model_downloads`: (1) `resolve_hf_repo("ggerganov/whisper.cpp")` against the live tree API → candidates non-empty and containing `ggml-tiny.bin`; (2) `ensure_custom_model` against `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny-q5_1.bin` (~32 MB) into a temp `HUMM_MODEL_ROOT`-style dir → file lands, sentinel written, and a second call is a no-download no-op (the existing ensure_artifact contract shape). Ungated branch: `eprintln!` + early return (the ADR-023 convention — never a panic).

- [ ] Write the test; confirm the default suite ignores it (baseline unchanged).
- [ ] Run it for real: `HUMM_HF_THIRDPARTY_CONTRACT=1 cargo test -p humm-core -- --ignored third_party_hf_repo` (huggingface.co is sandbox-allowlisted — expect no lift; document if the CDN redirect needs one). Record full output.
- [ ] Matrix F-042 → `verified` with the run as evidence; ADR-008 addendum: the M2 judgement's "no fixed target" premise expired (humm's own runtime depends on this repo), the no-default-suite half stands.
- [ ] Gates; commit `M9: F-042 third-party HF contract — run and verified (F-042)`.

### Task 14: ollama + the two real-chat contracts (F-031, F-111)

**Files:** `docs/analysis/PARITY_MATRIX.md` (both rows); no production code.
**Spec:** §4.1.

- [ ] `brew install ollama` (sandbox lift for ghcr.io — surfaced per-event); start `ollama serve`; `ollama pull llama3.2:1b` (lift for ollama.com registry).
- [ ] Confirm the OpenAI surface: `curl http://localhost:11434/v1/models` lists the model (loopback is sandbox-allowed).
- [ ] Run both (sandboxed — loopback): `HUMM_LLM_TEST_BASE=http://localhost:11434/v1 HUMM_LLM_TEST_KEY=ollama HUMM_LLM_TEST_MODEL=llama3.2:1b cargo test -p humm-core -- --ignored real_chat_api_single_and_batch_round_trip real_summary_api_round_trip`. The contracts assert protocol round-trip shape, not answer quality; if an assertion genuinely needs semantic quality, escalate the model size (3b, then 8b) rather than weakening the test — record whichever ran.
- [ ] Matrix F-031 + F-111 → `verified`, full probe + command + output recorded in the rows (the same evidentiary shape as the M8 §7-item-60 probe, now succeeding). Ollama stays installed (Jawad's standing approval; re-flagged at Task 18).
- [ ] Gates; commit `M9: F-031/F-111 real-chat contracts run against local ollama — verified`.

### Task 15: Mirror, tag, publish the release

**Files:** none in-repo (the release itself); task report records every URL + response.
**Spec:** §1.2, §1.7. **STOP-gates throughout (Global Constraint 3).**

- [ ] **Jawad step (wait for confirmation):** create public GitHub repo `digikal/humm`; configure Forgejo push mirror (jawad/humm → GitHub, Settings → Repository → Mirror); `brew install gh` + `gh auth login` (or export `GITHUB_TOKEN`).
- [ ] **Jawad decision point — signing arm (spec decision 1):** ask whether a Developer ID Application certificate is available. If YES: Jawad runs `xcrun notarytool store-credentials` himself, supplies the identity string + profile name; rebuild `cargo xtask bundle --identity "…"` + `cargo xtask notarize --profile …` + re-verify. If NO: the ad-hoc DMG ships (release notes carry the right-click-Open caveat) and the notarization residue is named in the matrix.
- [ ] Verify the mirror synced (source + branches visible on GitHub); push tag: `git tag v1.0.0 && git push origin v1.0.0` (Forgejo), then confirm the tag mirrors (trigger a manual mirror sync in Forgejo's UI if the interval hasn't fired — Jawad clicks, or wait it out).
- [ ] `cargo xtask version-info --notes <release-notes file>` → `version_info.json` (exact `update.rs` schema; `download_urls.macos_arm` = the `releases/download/v1.0.0/humm-1.0.0-macos-arm64.dmg` URL). Draft release notes: 1.0.0 = the parity mission (46→50 verified), key numbers from MISSION.md, the CLI-in-bundle symlink tip, the Gatekeeper caveat if ad-hoc.
- [ ] **Confirm with Jawad, then:** `gh release create v1.0.0 --repo digikal/humm --title "humm 1.0.0" --notes-file <notes> humm-1.0.0-macos-arm64.dmg version_info.json`.
- [ ] Verify unauthenticated: `curl -sL https://github.com/digikal/humm/releases/latest/download/version_info.json` from a shell with no gh auth in play → exact published bytes.
- [ ] Task report; no code commit (any docs corrections ride Task 18).

### Task 16: Install-from-release acceptance + F-006 both ways + keyring

**Files:** Create `docs/gui-manual/M9.md`; `docs/analysis/PARITY_MATRIX.md` F-006 row.
**Spec:** §1.5, §1.7 acceptance list. Windowed, sandbox-lifted, `caffeinate`, CGEvents.

- [ ] Fresh download of the DMG from the public URL (curl, not a local copy); mount; drag `humm.app` to `/Applications`; launch **from /Applications**.
- [ ] Checks, each recorded with screenshot/log in `gui-manual/M9.md`: app launches with NO Homebrew OpenBLAS present in the load path (verify via `DYLD_PRINT_LIBRARIES` that every non-system dylib loads from `Contents/Frameworks/` — this machine has Homebrew installed, so proof is load-path provenance, stated honestly); stderr clean (OMP gone); mic-permission prompt shows the Info.plist string on first live-recording use; a short end-to-end transcription completes.
- [ ] **Keyring (M7 blocker):** enter a live-API key in Preferences (a dummy value), quit, relaunch from /Applications, confirm it reads back without a keychain prompt-per-launch; record the observed ACL behavior for the shipped signing arm (Developer ID: stable; ad-hoc: whatever is actually observed — honestly).
- [ ] **F-006 both directions:** (a) installed 1.0.0 → update check against the real URL → no dialog (quiet path, log-confirmed); (b) pre-bump 0.1.0 binary — `git worktree add` at Task 9's commit's parent, build `humm-gui`, run with isolated `HUMM_*` dirs → the REAL published payload produces the genuine update-available dialog (screenshot). Remove the temp worktree after.
- [ ] Matrix F-006 → `verified`; gates; commit `M9: install-from-release acceptance + F-006 verified both ways (F-006)`.

### Task 17: Bench guard — M9 vs same-day M8

**Files:** Create `docs/benchmarks/M9.md`; bench worktree at `0864269` (M8 tip).
**Spec:** §6 bench guard; method = MISSION.md §3 / M8.md precedent.

- [ ] Call-path innocence first: `git diff 0864269..HEAD -- crates/humm-core/src/pipeline.rs crates/humm-core/src/queue.rs crates/humm-whisper/` — enumerate transcription-path touches (expected: pipeline.rs move_file only — not on the transcribe path; state it).
- [ ] Fresh worktree at `0864269`, release build both sides, ≥3 interleaved e2e pairs (`humm add audio-long.mp3 --txt -s small`, `/usr/bin/time -l`, alternating order, sandbox-lifted, load recorded), plus the criterion suite (decode/resample/export) on the M9 side vs its M8 recorded numbers.
- [ ] Also fold in Task 5's before/after DFN RSS measurement + the Task 11 bundle-launch startup number (informational).
- [ ] Regression on the transcription path = stop and fix before Task 18. Write `docs/benchmarks/M9.md`; commit `M9: bench guard vs same-day M8 — no transcription-path regression`.

### Task 18: Final docs sweep + matrix FINAL + milestone report

**Files:** Modify `docs/analysis/PARITY_MATRIX.md` (FINAL all-rows state), `docs/design/DECISIONS.md` (ADR-025 completion, addenda cross-checks), `docs/design/ARCHITECTURE.md` (xtask + fsutil as-built), `docs/design/IMPLEMENTATION_PLAN.md` (M9 exit note), `CLAUDE.md` (commands: `cargo xtask bundle`; workspace list + OpenBLAS note → bundled story; new contract gate `HUMM_HF_THIRDPARTY_CONTRACT`), `docs/HUMM_SURFACE.md` (if any surface changed), memory files.
**Spec:** §6.

- [ ] Matrix FINAL sweep: target **50/50 verified**; any residue (e.g. notarization if no Developer ID existed) named with blocker + committed closer, M8-discipline.
- [ ] Milestone report (task report, mirrored into the close commit message): final all-rows state, bench numbers, the dependency-watch verdicts, deviations (charter's Forgejo-release target → GitHub mirror, Jawad-approved), remaining debt honestly (carried Minors from D5b-13 not closed in passing).
- [ ] Memory update (`humm-port-status` + retire/rewrite `humm-m8-tickets` → M9 state); **ask Jawad**: retire `elated-austin-1982b0` worktree + branch (absorbed in Task 1); ollama keep/remove.
- [ ] Gates; commit `M9: final docs sweep + parity matrix FINAL + milestone report`.

---

## Self-review notes

- **Spec coverage:** §1.1→T9; §1.2→T15; §1.3→T10/T11; §1.4→T11; §1.5→T16; §1.6→T9; §1.7→T15/T16; §2.1→T3; §2.2→T2; §2.3/2.4→T4; §2.5→T5; §2.6→T2; §3.1→T6; §3.2→T8; §3.3→T7; banner absorption→T1; §4.1→T14; §4.2→T13; §4.3→T16; §5→T12; §6→T17/T18 + Global Constraints; §7 risks→named inside T5/T11/T14/T15.
- **Type consistency:** `copy_replaced_completed(source, segs) -> Result<TranscriptionId, StoreError>` used identically in T2's three site descriptions; `kill_all_registered_process_groups()` name identical in T3's media/cli/gui wiring; `fsutil::copy_then_rename(from, to) -> io::Result<()>` in T4 both halves; xtask subcommand names (`bundle`, `notarize`, `version-info`, `gen-icon`) consistent T10/T11/T15.
- **Order:** T1 first (baseline). T2–T8 (robustness/test debt) before T9–T11 so the released binary contains every fix. T9 before T10 (NOTICE/LICENSE/version exist before bundling). T12–T14 independent, before T15 so the release commit is final. T15→T16 strictly ordered (publish then install-from-release). T17 after all code tasks; T18 last.
- **Site-3 disposition** (app.rs merge worker) is a deliberate, documented non-change — T2 records it so the debt item closes as "2 fixed, 1 by-design" rather than silently half-done.
