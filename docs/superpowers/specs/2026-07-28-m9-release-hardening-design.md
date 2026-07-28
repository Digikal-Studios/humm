# M9 — Release & hardening: design

Milestone 9 (post-mission): make humm shippable and close the debt
register. The parity mission closed at M8 (46 verified / 4 named
exceptions); M9 targets a real published release (flipping F-006 and
unblocking the M7 keychain-ACL item), the robustness debt from the M8
final review (§D.5b), the named test debt, closure of the remaining
matrix exceptions (target: 50/50 or explicitly-permanent only), and a
hold/bump verdict on the three watched dependencies.

Decisions made with Jawad at brainstorm (2026-07-28), each binding for
this spec:

1. **Signing: plan both arms.** Whether a Developer ID certificate is
   available is not yet known. The pipeline treats signing as a pluggable
   final step: everything up to an ad-hoc-signed bundle works with no
   credentials; Developer ID + notarization slots in when Jawad confirms
   an identity. **STOP-gate (charter): Claude never touches the keychain
   or certificates — all credential-adjacent steps are run by Jawad.**
2. **Version: 1.0.0.** The parity mission is complete; this is what 1.0
   means. First release ever (no tags exist; workspace was 0.1.0).
3. **Distribution: full public GitHub mirror at `digikal/humm`** —
   supersedes the charter's `repo.digikal.com` release target
   (Jawad-approved deviation, 2026-07-28). The Forgejo repo stays
   private and push-mirrors source+tags to GitHub; releases are created
   on GitHub (mirroring does not carry release objects). The
   update-check URL moves to
   `https://github.com/digikal/humm/releases/latest/download/version_info.json`.
4. **F-031/F-111: install ollama + a small model** and run the two
   real-chat contracts against it.
5. **F-042: close via a double-gated contract test** against
   `ggerganov/whisper.cpp` (see §5.2), not a permanent exception.
6. **Pipeline shape: `cargo xtask`** (approach A) — pure-Rust release
   tooling, Apple's own CLIs as subprocesses, one new ADR.

---

## 0. Scope

**In:** the release arm end to end (bundle, dylib closure, sign,
notarize-if-available, DMG, GitHub release + `version_info.json`,
LICENSE + NOTICE, OMP-warning silencing, F-006 verification); the six
robustness items (pgid registry, store transaction, `move_file`
back-port, stderr caps, DfNoiseReducer streaming, `requeue_task`); the
three test-debt items (non-gated diarize coverage, `kittest_phases`
flake, `available()` staleness); exception closure (F-031, F-111,
F-042, F-006); dependency-watch verdicts (ort, deep_filter, speakrs);
absorption of the banner-flake fix (`3514857` from worktree
`elated-austin-1982b0`); bench guard vs a same-day M8 checkout; matrix
updates with evidence per row; milestone report.

**Out:** any feature work; CTranslate2 (ADR-008, unchanged); Windows or
x86_64 macOS artifacts (this machine builds `aarch64-apple-darwin` only —
`download_urls` publishes `macos_arm` alone, which `update.rs` already
treats as the only live arch key); auto-update *installation* (ADR-015
is notify-only, unchanged); carried cosmetic Minors from §D.5b item 13
(close-or-carry: only if trivially cheap in passing, otherwise named in
the final report).

---

## 1. Release arm (F-006)

### 1.1 Version + endpoint changes (code)

- Workspace `Cargo.toml` version → `1.0.0` (all crates inherit).
- `humm-core/src/update.rs::VERSION_INFO_URL` →
  `https://github.com/digikal/humm/releases/latest/download/version_info.json`;
  ADR-015's endpoint note updated in the same commit; any test pinning
  the old URL updated.
- `LICENSE` file added: MIT, Jawad Alnajjar — materializing the
  `license = "MIT"` already declared in `Cargo.toml`. No new decision.

### 1.2 Mirror + publishing setup (Jawad-run, Claude-instructed)

- Jawad creates public GitHub repo `digikal/humm` and configures a
  Forgejo push mirror (Settings → Repository → Mirror) from the private
  `jawad/humm`; Claude supplies exact steps and verifies the mirror
  syncs (source + tags visible on GitHub).
- Jawad installs `gh` (`brew install gh`) and runs `gh auth login` (or
  exports `GITHUB_TOKEN`). Claude drives `gh release create` with it but
  never handles the token's storage.

### 1.3 `xtask` bundler (new workspace member; ADR-025)

`crates/xtask`, a dev-only binary never shipped or published (excluded
from any distribution artifact; it exists only for `cargo xtask …`).
Subcommands:

- **`cargo xtask bundle [--identity "Developer ID Application: …"]`**
  1. `cargo build --release` for `humm-gui` + `humm`.
  2. Assemble `target/bundle/humm.app`:
     - `Contents/MacOS/humm-gui` (CFBundleExecutable), `Contents/MacOS/humm`
       (CLI, so power users can symlink it onto PATH; documented in
       release notes).
     - `Contents/Resources/humm.icns` — new original icon (committed
       asset under `assets/`; generated once by a small script also
       committed; **not** Buzz's icon or any derivative of it).
     - `Contents/Resources/NOTICE` (§1.6), `Contents/Resources/LICENSE`.
     - `Info.plist`: `CFBundleIdentifier com.digikal.humm`,
       `CFBundleName humm`, `CFBundleShortVersionString`/`CFBundleVersion
       1.0.0` (read from the workspace version, not hard-coded),
       `CFBundlePackageType APPL`, `NSHighResolutionCapable`,
       `LSMinimumSystemVersion` (determined at implementation from the
       toolchain's deployment target; recorded in ADR-025), and
       **`NSMicrophoneUsageDescription`** — required for live recording
       in a bundled app; the dev binary inherited the terminal's mic
       grant so this never surfaced before.
  3. **Dylib closure**: walk `otool -L` transitively from both binaries;
     every non-system dylib (`/opt/homebrew/...`: libopenblas →
     libgfortran → libomp → libgcc_s etc., whatever the walk actually
     finds) is copied into `Contents/Frameworks/`, load commands
     rewritten via `install_name_tool` to
     `@executable_path/../Frameworks/<name>`, each modified Mach-O
     re-signed (ad-hoc at minimum — mandatory on arm64 after
     `install_name_tool`). Post-condition asserted by the tool itself:
     re-walk `otool -L` over everything in the bundle and fail if any
     `/opt/homebrew` (or other non-system absolute) reference remains.
     The otool-output parser and closure-walk logic are plain Rust
     functions, unit-tested on canned `otool -L` output.
  4. **Sign**: `--identity` absent → ad-hoc (`codesign -s -`) over
     dylibs then app (inside-out). Present → `codesign
     --options runtime` with the given identity. Identity discovery,
     creation, and any keychain unlock are Jawad's steps.
  5. **DMG**: `hdiutil create` with the app + an `/Applications`
     symlink → `target/bundle/humm-<version>-macos-arm64.dmg`.
- **`cargo xtask notarize --profile <keychain-profile>`** (only if a
  Developer ID landed): wraps `xcrun notarytool submit --wait` +
  `xcrun stapler staple` on the DMG. The keychain profile is created by
  Jawad (`xcrun notarytool store-credentials`) — STOP-gate.
- **`cargo xtask version-info --notes <file>`**: emits
  `version_info.json` exactly matching `update.rs`'s parsed shape:
  `{"version": "<workspace version>", "release_notes": "<notes>",
  "download_urls": {"macos_arm":
  ["https://github.com/digikal/humm/releases/download/v<version>/humm-<version>-macos-arm64.dmg"]}}`.

**ADR-025** records: the xtask pattern; Apple CLIs (`codesign`,
`notarytool`, `stapler`, `hdiutil`, `install_name_tool`, `otool`,
`iconutil`) as the unavoidable non-Rust boundary for macOS
distribution (OS-vendor tools, versioned with the OS — treated like
ffmpeg-class subprocess adapters but build-time only); the **Accelerate
evaluation verdict**: rejected because speakrs 0.5.0 exposes only
`openblas-static`/`openblas-system`/`intel-mkl` backends and upstream
`ndarray-linalg` has no Accelerate feature — switching would mean
maintaining forks of two crates, strictly worse than bundling one dylib
closure. Cross-referenced as an ADR-024 addendum ("packaging debt"
paragraph gets a "closed by ADR-025/M9" note).

### 1.4 OMP stderr warning

Root-cause first (systematic-debugging): the known shape is an OpenMP
runtime-collision message on stderr at GUI launch (Homebrew libomp
via OpenBLAS, plus whatever OpenMP runtime ONNX Runtime carries).
Fix at the link/bundle level so exactly one OpenMP runtime loads from
the bundle; an environment-variable suppression (`KMP_*`) is acceptable
only if the root cause genuinely demands it, and then documented in
ADR-025 with the reason. Acceptance: launching the installed bundle
produces a clean stderr.

### 1.5 Keychain / live-API keyring (M7 blocker)

With the installed bundle, exercise the GUI live-API keyring path
(store a key from Preferences, restart, use it): under a real Developer
ID signature the ACL is stable across rebuilds and the path verifies
fully; under ad-hoc the identity changes per build — record the actual
observed prompt/behavior honestly in the report and gui-manual. Either
way the *installed-bundle* behavior gets documented for the first time.

### 1.6 NOTICE (Apache-2.0 §4 + general attribution)

One `NOTICE` file at repo root (copied into the bundle): Buzz (MIT —
behavioral ground truth; its golden fixtures are ported into humm's
tests), whisper.cpp + ggml (MIT), whisper-rs, speakrs (Apache-2.0), ONNX Runtime (MIT), ort (MIT/Apache-2.0),
deep_filter/DeepFilterNet code + DFN3 model weights (verify the
weights' actual license from the upstream repo), pyannote
segmentation-3.0 and wespeaker embedding models as redistributed via
`avencera/speakrs-models` (verify each model's license from its actual
HF card, not memory), Silero VAD model, Noto Sans SC (OFL 1.1),
OpenBLAS (BSD-3), libgfortran (GPL + GCC runtime-library exception),
libomp (Apache-2.0 w/ LLVM exception), egui/eframe (MIT/Apache-2.0),
plus a generated full crate-license summary (`cargo license` or
equivalent) as an appendix or sibling file. Every license claim in the
file is verified against the upstream source during implementation —
no from-memory license assertions.

### 1.7 Publish + F-006 verification

1. Tag `v1.0.0` on the merge commit, push to Forgejo, confirm the
   mirror carries it to GitHub.
2. `gh release create v1.0.0 --repo digikal/humm` with assets: the DMG,
   `version_info.json`, and release notes (drafted from the milestone
   report; notes the CLI-inside-bundle symlink trick and the
   right-click-Open caveat if the ad-hoc arm shipped).
3. Acceptance run on this machine (windowed, sandbox-lifted as needed):
   - `curl` the public `releases/latest/download/version_info.json` URL
     unauthenticated → exact published bytes (proves the private-repo
     404 problem is gone).
   - Download the DMG fresh from the public URL, mount, drag to
     `/Applications`, launch **from /Applications** — proves the dylib
     closure resolves with no Homebrew dependency (verified on this
     machine by the otool post-condition in §1.3; a truly
     OpenBLAS-less machine is not available, stated honestly).
   - Clean stderr (§1.4); mic permission prompt appears with the
     Info.plist usage string when live recording is first used.
   - Keyring path per §1.5.
   - **Update dialog, both directions**: the installed 1.0.0 checks the
     real URL and correctly shows nothing (equal version); a
     pre-version-bump 0.1.0 dev binary (built from the version-bump
     commit's parent) sees the real published payload and shows the
     genuine update-available dialog. Both recorded with screenshots in
     gui-manual/M9.md. → **F-006 flips to `verified`.**

---

## 2. Robustness (each lands with its regression test)

1. **pgid registry + `kill_all_children`** (`humm-media` +
   both binaries): `spawn_own_process_group` registers the child's pgid
   in a process-global registry; the reap path unregisters. New
   `kill_all_registered_groups()`: SIGKILL every registered group
   (second-Ctrl-C is an emergency exit — no TERM-then-KILL escalation,
   matching the existing immediate-SIGKILL cancel semantics for
   yt-dlp). Called from `humm-cli`'s `install_ctrlc` second-SIGINT arm
   before `std::process::exit(130)`, and from the GUI's shutdown path —
   in the GUI case as the last-resort sweep *after* the existing
   cooperative cancellation has had its normal chance, never instead of
   it.
   Tests: unit (stub `sh`-with-grandchild child, registry kill,
   grandchild provably dead — mirroring
   `cancel_kills_grandchild_process_not_just_direct_child`); cli-int
   (two real SIGINTs against the binary running a stub long-running
   "demucs", assert the stub's process group is dead after exit).
   Closes §D.5b items 2 and 4 (the "other spots may assume single-PID"
   audit rides along: grep every `kill`/signal site, fix or document).
2. **Atomic copy+complete** (`humm-store`): new store method wrapping
   `copy_transcription` + `mark_replaced_completed` in one rusqlite
   transaction; the three call sites (speaker-id save-as-copy, resizer
   ×2) switch to it. Test: make the second half fail (e.g. bogus id)
   and assert rollback leaves no orphan `Queued` row; plus a success
   test pinning identical end-state to the old two-call sequence.
3. **`move_file` back-port** (folder-watch): `copy_then_rename` moves
   from `humm-media` to `humm-core` (pure fs logic, no adapter deps;
   core stays adapter-free — the *dependency* direction is
   adapters-import-core, which this respects), `humm-media` re-exports
   or imports it; `pipeline.rs::move_file`'s cross-device fallback uses
   it. Existing unit tests move with the helper; new test drives
   `move_file`'s fallback via an injected copy failure. Also closes the
   §D.5b item-13 "move_file duplication across humm-media adapters"
   Minor.
4. **Bounded stderr in decode/encode** (`humm-media`): the
   `STDERR_TAIL_CAP` bounded-tail pattern from `demucs.rs` extracted to
   a shared helper used by `decode.rs`, `encode.rs`, and `demucs.rs`
   (closing the item-13 stderr-duplication Minor in the same motion).
   Test: stub program emitting oversized stderr; assert the captured
   tail is capped and marked truncated.
5. **Stream `DfNoiseReducer`'s output** (`humm-dfn`): today the reducer
   accumulates the full processed signal (~4× decoded-audio copies,
   ~5.5 GB peak for 2 h media); rework the process loop to write
   chunk-wise to the encoder/output as frames complete. Correctness:
   byte-equivalence of the streamed vs pre-change output on a small
   fixture (golden file generated from the current code before the
   change). The memory win is measured once (peak-RSS comparison on a
   long fixture) and recorded in the report — not asserted in a test.
6. **`requeue_task` segment append** (pre-existing bug): investigate
   first — known shape is `requeue_task` clearing no segments while
   `mark_completed` INSERTs. Fix at the store layer (requeue clears the
   task's segments, or completion replaces them — implementer picks
   after reading both call sites; the *behavior* contract is: a
   Skipped-then-retried task ends with exactly the retry's segments).
   Regression test: complete with segments → requeue → re-complete →
   segment set equals the second run's alone.

---

## 3. Test debt

1. **Non-gated `humm-diarize` coverage**: unit tests exercising the
   exact speakrs API surface `OrtDiarizer` uses, without models —
   `OwnedDiarizationPipeline::from_dir` on a missing dir and on a dir
   with garbage files (error shapes), config/type construction
   (`ExecutionMode`, clustering config), so a speakrs upgrade that
   breaks the API fails plain `cargo test --workspace` at compile time
   or immediately at runtime. Stated honestly in the module doc: this
   seam catches *breaking* upgrades; a silent inference-quality change
   still needs the gated 58 MB contract test.
2. **`kittest_phases` one-frame-lag flake**: systematic-debugging
   first — reproduce under load (the known trigger), identify the
   frame-timing assumption, fix with a bounded run-until-condition
   helper (no sleeps, no unbounded loops); if the root cause turns out
   different, fix that. Exit criterion: the previously-flaky test runs
   clean N=15 consecutive times under parallel full-suite load
   (matching the banner-fix verification bar).
3. **`DemucsIsolator::available()` staleness**: the process-lifetime
   `OnceLock` cache in `services.rs` means demucs installed mid-session
   isn't seen until restart. Replace with re-probe-on-options-sheet-open
   (an explicit user-visible moment, keeping steady-state cheap) —
   confirm at implementation why the cache exists (probe cost) and
   preserve CLI fast-fail behavior. Unit test pins the re-probe-on-open
   semantics.

**Banner-flake absorption**: cherry-pick `3514857` ("hold locale test
lock in tr-reading GUI tests") from worktree `elated-austin-1982b0`
early in the milestone, before any task touches GUI tests. The
`elated-austin-1982b0` worktree + branch are retired after absorption
(Jawad confirms before removal).

---

## 4. Exceptions closure

### 4.1 F-031 + F-111 (real-chat contracts)

`brew install ollama`; `ollama pull` a small chat model
(llama3.2:1b-class, ~1.3 GB); serve; run both contracts:
`HUMM_LLM_TEST_BASE=http://localhost:11434/v1`,
`HUMM_LLM_TEST_KEY=<any non-empty>`, `HUMM_LLM_TEST_MODEL=<model>` →
`real_chat_api_single_and_batch_round_trip` (F-031) and
`real_summary_api_round_trip` (F-111). Record full command + output in
the matrix rows; both flip to `verified`. Network fetches (ghcr.io,
ollama.com) need sandbox lifts, surfaced individually at execution.
Ollama remains installed afterwards (Jawad's earlier approval; flagged
again at close in case he wants it removed).

### 4.2 F-042 (custom-model contract)

The M2 standing judgement ("no fixed third-party target exists") has
aged out of being literally true: humm's runtime already depends on
`ggerganov/whisper.cpp` on HuggingFace for every model download — a
fixed, stable, third-party target. New contract test in `humm-core`
(double-gated: `#[ignore]` + `HUMM_HF_THIRDPARTY_CONTRACT=1`) driving
the row's two distinctive surfaces against the live network:
`resolve_hf_repo("ggerganov/whisper.cpp")` (candidate discovery against
the real tree API) and `ensure_custom_model` with a real small artifact
from that repo (a quantized tiny, ~32 MB) — asserting the file lands,
sentinel written, resume/verify semantics per the existing
`ensure_artifact` contract precedent. Run once at M9 acceptance; row
flips to `verified` with the run recorded. The old judgement survives
where it was right: nothing enters the default suite, so no third
party's repo schedule can break `cargo test --workspace`. Matrix
prose + an ADR-008 addendum note record the ruling change.

### 4.3 F-006

Closed by §1.7. Matrix target after §4: **50/50 verified**, or failing
a blocked arm (e.g. notarization unavailable), the residue named with
the same discipline as M8.

---

## 5. Dependency watch (evaluate, don't blindly bump)

One task, three verdicts, written into the milestone report:

- **ort** (pinned `=2.0.0-rc.12`): check for rc.13+/GA; read the
  changelog/diff for the API surfaces humm-diarize + speakrs touch.
- **deep_filter** (git-pinned v0.5.6): check upstream past the pin.
- **speakrs** (pinned `=0.5.0`): check >0.5.0; note whether any newer
  version changes the linalg feature set (an Accelerate backend
  appearing upstream would reopen §1.3's bundling trade-off — watch
  explicitly for it).

Rule (Standing Policy 5): a bump happens only if it buys something
concrete AND the adapter's contract tests alone re-validate it — which
means any bump commits to running the gated contracts (diarize:
`HUMM_DIARIZE_CONTRACT=1`; dfn: `HUMM_DFN_CONTRACT=1`) as part of the
bump commit's evidence. "Hold all three pins, reasons stated" is an
acceptable and expected outcome.

---

## 6. Process & acceptance

- **Execution model**: written implementation plan
  (superpowers:writing-plans) → subagent-driven execution with per-task
  review, as M2–M8; task briefs/reports under `.superpowers/sdd-m9/`
  (gitignored, same convention as M8's).
- **Every commit**: `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace` at **zero
  failures** (the proven sandboxed baseline — any failure is real).
  Commit messages reference M9 + feature/debt IDs.
- **Bench guard**: same-day interleaved e2e pairs (≥3, alternating
  order) of M9 vs an M8-tip checkout (`0864269`), release builds, the
  MISSION.md §3 method (same fixture, `/usr/bin/time -l`). Regression
  on the transcription path is a failure to fix, not a note.
- **Matrix**: every touched row updated with evidence; final sweep
  states all 50 rows.
- **Milestone report**: benchmarks in `docs/benchmarks/M9.md`; windowed
  evidence in `docs/gui-manual/M9.md`; final all-rows state + genuinely
  remaining debt in the closing task report, mirrored into the
  milestone-close commit message and the project memory.
- **STOP-gates (consolidated)**: keychain/certificate/notarization
  credentials (Jawad runs those steps); GitHub repo creation + Forgejo
  mirror config + `gh auth` (Jawad); any sandbox lift (surfaced
  per-event as usual); `elated-austin-1982b0` worktree removal (Jawad
  confirms).

---

## 7. Risks

- **Notarization unknowns**: if no Developer ID exists, the ad-hoc arm
  ships (right-click-Open caveat in release notes) and the
  keyring-ACL verification records ad-hoc behavior; the matrix names
  the notarization residue honestly. The pipeline is built so a later
  cert requires only re-running `bundle --identity` + `notarize`.
- **ONNX Runtime dylib surprise**: if the built GUI links a pyke
  `libonnxruntime.dylib` (not static), the closure walk in §1.3 picks
  it up like any other non-system dylib — the design already covers it;
  flagged so nobody assumes static linking without checking `otool`.
- **OMP warning root cause deeper than expected** (e.g. genuinely two
  runtimes both required): documented fallback is the env-var
  suppression path with ADR justification (§1.4).
- **Ollama contract quality**: a 1B model may answer the translation
  contract oddly; the contracts assert protocol round-trip shape, not
  answer quality — if a test's assertions turn out to require real
  semantic quality, escalate model size rather than weakening the test.
- **DfNoiseReducer streaming regression risk**: guarded by the golden
  byte-equivalence fixture (§2.5) and the existing gated DFN contract.
- **Bench-guard noise**: the M8 checkout is built once, pairs
  interleaved same-session per MISSION.md discipline; outliers flagged
  per the M7/M8 convention rather than blended.
