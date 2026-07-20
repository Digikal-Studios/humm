# Mission: Port Buzz to Rust ("humm") with 100% Functional Parity

You are executing a full port of **Buzz** (source: `/Users/jawad/RustroverProjects/buzz`) to a new Rust application called **humm** (target: `/Users/jawad/RustroverProjects/humm`). This is a long-running, multi-phase engineering effort. Work methodically through the phases below. Do not skip ahead, and do not write implementation code before Phase 2.

Two values govern every decision in this project, in this order:
1. **Functional parity** — the user can accomplish everything in humm that they could in Buzz.
2. **Performance** — humm should be decisively faster. Rust-first by default, but where an FFI binding or a mature external component is measurably more performant or provides format compatibility that pure Rust cannot, use it. Purity is not a goal; speed and correctness are.

**The Buzz source code is the sole ground truth for behavior.** Never rely on prior knowledge of Buzz, its README, or assumptions about what a transcription app "usually" does. If the code and your expectations disagree, the code wins. When behavior is genuinely ambiguous even after reading the source, log it as an open question rather than guessing.

---

## Standing Policy — Dependencies and Non-Rust Boundaries

This policy applies to every phase and every line of code.

### Pre-approved defaults (validate, don't re-litigate)

The following choices are approved **contingent on being genuinely the most performant option**. Confirm each empirically where feasible; if evidence contradicts a default, present the evidence and the alternative at the relevant gate rather than silently deviating.

- **Whisper inference: `whisper-rs`** (FFI to whisper.cpp), built with Metal and Core ML support on Apple Silicon. In Milestone 1, run a validation benchmark against `candle`'s Whisper implementation on a real model and a real audio file (wall-clock, peak memory, output quality) and record the numbers in the ADR.
- **Media decoding: ffmpeg, invoked as a subprocess** — not linked via libav* bindings. Rationale: the ffmpeg CLI surface has been stable for decades while the library ABI breaks across major versions, which matters under the boundary rules below; subprocess overhead is immaterial for one-shot batch decode. Deviate to bindings only with benchmark evidence that the subprocess path is a real bottleneck.
- **Persistence: `rusqlite`** with the `bundled` feature (statically linked, version pinned through Cargo).
- **Audio capture: `cpal`** (the unavoidable OS-API boundary for audio I/O).

### Conditional — resolve during Phase 0 analysis

- **VAD:** if Buzz uses Silero VAD (or any ONNX model), default to `ort` (ONNX Runtime FFI); consider `tract` only if a benchmark shows it adequate for real-time use.
- **URL import:** if Buzz supports importing from URLs, integrate `yt-dlp` as a subprocess.
- **faster-whisper backend: do not port CTranslate2.** Functional parity is defined at the outcome level — one maximally fast local inference backend plus the API backends — not at the level of reproducing Buzz's backend menu. Record this deviation as an explicit ADR.

### Boundary design rules (non-negotiable)

Every non-Rust component must be integrated so it can be **updated or replaced without breaking humm**:

1. **Trait boundary.** Each non-Rust component is accessed exclusively through a Rust trait defined in the core crate (e.g., `AudioDecoder`, `TranscriptionEngine`, `Vad`, `MediaFetcher`). Nothing outside the adapter knows what implements the trait.
2. **Adapter isolation.** Each adapter lives in its own module or crate (e.g., a media crate wrapping ffmpeg). All `unsafe`, subprocess management, output parsing, and format translation are confined there.
3. **Version tolerance for external binaries.** Interact only through stable, long-established CLI flags and machine-readable outputs (e.g., ffmpeg raw PCM to stdout via `-f f32le`; `ffprobe -print_format json`). Never parse human-oriented log text. Probe each binary at startup (version and capability check) and surface actionable errors, never crashes.
4. **Version pinning for FFI.** Prefer vendored, statically linked native code pinned by the crate version (whisper-rs bundles whisper.cpp; rusqlite bundles SQLite), so a system-level library update can never break the app. Native code versions change only through deliberate Cargo dependency updates, verified in tests.
5. **Contract tests at every boundary.** Each adapter has a test suite that exercises the real binary or library against golden inputs and asserts on outputs. Upgrading ffmpeg, whisper.cpp, or any other external component must be fully verifiable by running these tests alone.
6. **Graceful absence.** If an external binary is missing or incompatible, the dependent feature degrades with a clear, actionable message; all unrelated features keep working.

---

## Phase 0 — Deep Codebase Analysis (no code written)

Read the entire Buzz codebase. Be exhaustive: entry points, GUI code, CLI code, background workers/threads, model management, persistence, configuration, packaging, and tests. Then produce the following documents under `humm/docs/analysis/`:

1. **`FEATURE_INVENTORY.md`** — Every user-facing capability, each with a stable ID (`F-001`, `F-002`, …). For each feature record: what it does, how the user invokes it (GUI action, CLI flag, keyboard shortcut, drag-and-drop, etc.), all inputs (file formats, devices, URLs, settings), all outputs (on-screen state, exported files and their exact formats, exit codes, stdout/stderr), error and edge-case behavior, and the source files that implement it. Include non-obvious behavior: default values, persisted settings, model download/caching logic, cancellation semantics, progress reporting, concurrency (what runs in background, what blocks the UI), and platform-specific branches.
2. **`USER_JOURNEYS.md`** — For each major workflow (not each feature), a step-by-step narrative of what the user does and sees, from launch to outcome. Cover both GUI and CLI journeys. Note every point of friction, latency, or required decision in the current design — these are opportunities, not constraints.
3. **`EXTERNAL_SURFACE.md`** — The complete externally observable contract: full CLI grammar (every command, flag, alias, default, exit code), every file format read or written (with exact structure — e.g., subtitle formats, transcript exports, config files), every network endpoint or API used, model files and where they're stored, and any OS integration (audio devices, file associations, tray/menu behavior, notifications).
4. **`PARITY_MATRIX.md`** — A table derived from the feature inventory: `Feature ID | Description | Verification method | Status`. Status starts at `not-started` for every row. This file is the spine of the entire project; it will be updated continuously and the project is done only when every row is `verified`.

During analysis, also resolve the conditional items in the Standing Policy: determine whether Buzz uses VAD (and which), whether URL import exists, and exactly which inference backends Buzz exposes and what user outcomes each serves.

**Gate:** When Phase 0 is complete, stop and present a summary of findings, the feature count, the resolution of the conditional policy items, and any open questions. Wait for my confirmation before proceeding.

---

## Phase 1 — Rust-Native Architecture & Design

Design humm as a Rust application from first principles. The Python/Qt implementation tells you *what* the software must do — never *how* to structure it. Explicitly avoid transliterating: no recreating Python class hierarchies, Qt signal/slot patterns, or dynamic-typing workarounds in Rust form. Where Buzz's structure was shaped by its language's limitations, design the shape Rust would naturally produce.

Produce under `humm/docs/design/`:

1. **`ARCHITECTURE.md`** — The proposed architecture. I expect (but you should validate) a Cargo workspace along the lines of: a core library crate containing all transcription, model-management, and export logic with zero UI dependencies; adapter crates for each non-Rust boundary per the Standing Policy; a CLI crate; and a GUI crate — the binaries thin frontends over the core. Define the module boundaries, the trait definitions for each boundary, the concurrency model (async runtime vs. threads vs. channels — justify per subsystem; audio capture, inference, and UI have different needs), state management, error-handling strategy (typed errors in the library via `thiserror`, contextual handling at the binary edges), and the data model for transcripts, tasks, and settings.
2. **`DECISIONS.md`** — An ADR log. One entry per significant choice, each with options considered, tradeoffs, and rationale. The Standing Policy defaults each get an ADR recording the validation plan or result. Decisions still genuinely open and required at minimum:
   - **GUI framework.** Evaluate the serious candidates (e.g., `iced`, `egui`, `Slint`, `gpui`) against humm's actual needs from `USER_JOURNEYS.md`: long-running background tasks with progress, tables/lists of transcripts, audio level/waveform display if Buzz has one, native file dialogs, accessibility, and platform look-and-feel. Recommend one with rationale.
   - **CLI framework** and how the CLI grammar maps to `EXTERNAL_SURFACE.md` (parity of behavior is required; parity of flag spelling is preferred unless there is a strong reason to deviate, and any deviation must be logged).
   - Resampling and audio pipeline design (e.g., `rubato` for 16kHz mono conversion, buffer strategy for live capture).
   - Persistence schema: settings, transcript storage, model cache location and format — including whether to remain compatible with Buzz's on-disk data for migration.
3. **`UX_SPEC.md`** — For each user journey from Phase 0, the redesigned journey in humm. Parity means the user can accomplish everything they could in Buzz — it does not mean reproducing Buzz's friction. Where the analysis found awkward flows, design the frictionless version and note the improvement. Specify: screen/view inventory and navigation, empty states, loading and progress states, error presentation, cancellation, keyboard shortcuts, and sensible defaults that minimize required decisions. Apply the same lens to the CLI: helpful `--help` output, good error messages, progress on long operations, and clean composability (predictable exit codes, machine-readable output where useful).
4. **`IMPLEMENTATION_PLAN.md`** — Ordered implementation milestones as **vertical slices**: each milestone delivers a coherent feature group end-to-end (core logic → CLI → GUI) rather than building layers horizontally. For each milestone, list the parity-matrix IDs it covers and its acceptance criteria. Sequence so that a minimal usable transcription path exists as early as possible. **Milestone 1 must include the whisper-rs vs. candle validation benchmark** and the contract-test scaffolding for every adapter it introduces.
5. **`CLAUDE.md`** (in the repo root) — Project conventions for all future sessions: build/test/lint/bench commands, workspace layout, error-handling and naming conventions, the Standing Policy (verbatim or by pointer), the rule that `PARITY_MATRIX.md` must be updated with every merged milestone, and a pointer to the docs above.

**Gate:** Stop and present the architecture, the ADRs (the GUI framework decision, and the benchmark/validation plan for each pre-approved default), and the milestone plan. Wait for my approval before writing implementation code.

---

## Phase 2..N — Implementation by Vertical Slice

For each milestone in the approved plan:

1. Re-read the relevant feature entries and Buzz source before implementing — implement from the source's actual behavior, not from memory of Phase 0.
2. Build the slice: core-library logic first with unit tests, then CLI, then GUI.
3. Write tests as you go, not after: unit tests for core logic, integration tests for the CLI surface (invoke the binary, assert on output/exit codes/produced files), snapshot tests for export formats, and **contract tests for every non-Rust boundary** per the Standing Policy. Where practical, verify parity differentially: run Buzz and humm on the same input and compare outputs (transcript text aside — inference backends may differ — structure, timestamp formats, and file formats must match the contract in `EXTERNAL_SURFACE.md`).
4. Maintain a `criterion` benchmark suite for the hot paths (decode, resample, inference, end-to-end file transcription). Track results across milestones; a performance regression on the transcription path is a failure to fix, not a note to log.
5. Every commit must pass: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Commit at coherent boundaries with messages referencing the milestone and feature IDs.
6. Update `PARITY_MATRIX.md`: `in-progress` → `implemented` → `verified` (verified only when the stated verification method has actually been executed and passed). Never mark a stubbed, placeholder, or partially working feature as anything beyond `in-progress`.
7. At the end of each milestone, give a brief report: matrix status, benchmark results, deviations from Buzz behavior (with rationale and ADR reference), and anything discovered in the Buzz source that requires revisiting earlier analysis.

---

## Code Quality Bar (applies to every line)

- Idiomatic Rust throughout: ownership-driven design, `Result` everywhere fallible (no `unwrap`/`expect` outside tests and truly impossible states — and document why for the latter), newtypes over primitive obsession, iterators over index loops, exhaustive `match` on domain enums, `#[non_exhaustive]` and builder patterns where API evolution warrants.
- Public API of the core crate fully documented with `///` doc comments including examples; `#![warn(missing_docs)]` on the core library.
- No `unsafe` outside adapter crates, and inside them only when unavoidable at the FFI boundary — isolated, minimal, and documented with safety invariants.
- Dependencies are a liability: justify each one; prefer well-maintained, widely used crates; no dependency for what the standard library does well. The Standing Policy governs all non-Rust dependencies.
- Long-running work never blocks the UI thread; all cancellation is cooperative and prompt; progress reporting is real, not cosmetic.
- Handle the unhappy paths from `FEATURE_INVENTORY.md` explicitly — missing models, missing external binaries, no audio device, corrupt input files, no network, disk full during export.

## Standing Rules

- When uncertain about Buzz's behavior: read the source again. When still uncertain: ask me or log it in `docs/OPEN_QUESTIONS.md` — never invent behavior.
- Keep all Phase 0/1 documents current as living documents; if implementation reveals an analysis error, fix the document in the same commit.
- Definition of done for the project: every `PARITY_MATRIX.md` row is `verified`, all quality gates and contract tests pass, benchmarks show no unresolved regressions, and both binaries build and run on the target platform.
