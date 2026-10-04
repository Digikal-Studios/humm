# CLAUDE.md — humm

humm is a Rust port of Buzz (`/Users/jawad/RustroverProjects/buzz`) with **100% functional parity** and **decisively better performance** — in that order. The Buzz *source code* is the sole ground truth for behavior; never rely on prior knowledge of Buzz or assumptions about transcription apps.

## Read before coding

1. `docs/analysis/PARITY_MATRIX.md` — the project spine. Done = every row `verified`, or a named, blocker-documented exception; final state: 46 verified / 4 exceptions.
2. `docs/analysis/FEATURE_INVENTORY.md` — feature IDs (F-###) with Buzz source refs. **Re-read the cited Buzz files before implementing a feature** — implement from source, not memory.
3. `docs/design/ARCHITECTURE.md`, `docs/design/DECISIONS.md` (ADRs), `docs/design/UX_SPEC.md`, `docs/design/IMPLEMENTATION_PLAN.md` (current milestone).
4. `docs/OPEN_QUESTIONS.md` — never invent behavior; log genuine ambiguity there.

## Commands

```bash
brew install openblas libomp                             # macOS build AND RUNTIME prereq (humm-diarize
                                                         #  speakrs -> ndarray-linalg openblas-system links
                                                         #  Homebrew's dylib by absolute path — humm-gui
                                                         #  fails to launch at all without it, ADR-024)
cargo fmt --check                                        # formatting gate
cargo clippy --workspace --all-targets -- -D warnings    # lint gate
cargo test --workspace                                   # unit + integration + snapshot
cargo test -p humm-media -- --ignored                    # contract tests (need ffmpeg/yt-dlp)
cargo bench                                              # criterion suite (decode/resample/e2e/export)
cargo run -p humm-cli -- add <file> --srt                # CLI smoke
cargo run -p humm-gui                                    # GUI
```

Adapter contract tests that need a heavier external are double-gated (`#[ignore]` **and** an env flag), so the line above stays runnable with only ffmpeg/yt-dlp installed: `HUMM_DEMUCS_CONTRACT=1` (real demucs, F-027), `HUMM_DFN_CONTRACT=1` (`-p humm-dfn`, F-112), `HUMM_DIARIZE_CONTRACT=1` (`-p humm-diarize`, downloads ~60 MB of models on first run, F-087), `HUMM_LLM_TEST_BASE`/`_KEY`/`_MODEL` (a real OpenAI-compatible chat endpoint, F-031/F-111).

Every commit must pass the first three. Commit at coherent boundaries; messages reference milestone + feature IDs (e.g. `M4: segment editor overlap clamping (F-081)`).

## Workspace

`crates/humm-core` (domain library, `#![warn(missing_docs)]`, zero UI deps, no `unsafe`) · `humm-media` (ffmpeg/ffprobe/yt-dlp/demucs subprocess adapters) · `humm-whisper` (whisper-rs FFI; the only crate allowed `unsafe`, documented) · `humm-audio` (cpal+rubato) · `humm-store` (rusqlite+keyring) · `humm-dfn` (M8: DeepFilterNet noise reduction via the pinned `deep_filter`/tract crate; composes `humm-media` for decode/encode) · `humm-diarize` (M8: speaker diarization; the only crate naming `ort`/`speakrs`) · `humm-cli` · `humm-gui` (egui/eframe). Binaries → adapters → core; core imports no adapter.

## Standing Policy (dependencies & non-Rust boundaries) — binding

Full text: `docs/MISSION.md` (the governing mission prompt — phases, gates, Standing Policy, quality bar), mirrored by ADRs in `docs/design/DECISIONS.md`. Short form:
- Non-Rust components only behind core-defined **traits**, in isolated **adapter crates**; subprocesses speak stable machine-readable interfaces only (raw PCM pipes, `-print_format json`); FFI is vendored/pinned via Cargo; every adapter has **contract tests** that alone verify an external upgrade; missing binaries degrade features gracefully, never crash.
- Approved: whisper-rs (Metal+CoreML), ffmpeg subprocess, rusqlite bundled, cpal, yt-dlp subprocess. Not ported: CTranslate2 (ADR-008). New non-Rust deps require an ADR.

## Conventions

- Errors: `thiserror` enums in libraries, `anyhow` in binaries; no `unwrap`/`expect` outside tests and documented-impossible states. Actionable messages ("X missing — do Y").
- Style: ownership-driven design, newtypes (`Millis`, ids), iterators over index loops, exhaustive `match` on domain enums, builders where API evolution warrants.
- Tests as you go: unit (core logic), snapshot (`insta`) for export formats — Buzz's golden fixtures are ported in `humm-core` tests, CLI integration (invoke the binary), contract (adapters). Differential vs Buzz where practical: structure/formats must match, transcript text may differ.
- Long work never blocks UI; cancellation cooperative and prompt; progress real, not cosmetic.
- Unhappy paths from `FEATURE_INVENTORY.md` are requirements: missing model/binary/device, corrupt input, no network, disk full.

## Process rules

- **Update `docs/analysis/PARITY_MATRIX.md` with every merged milestone** (`not-started → in-progress → implemented → verified`; `verified` only after the stated verification method actually ran and passed; stubs never advance past `in-progress`).
- Phase 0/1 docs are living: implementation discovering an analysis error fixes the doc in the same commit.
- Benchmark regressions on the transcription path are failures to fix, not notes to log.
- End each milestone with a report: matrix status, benchmark numbers, deviations (with ADR refs), analysis corrections.

## Paperclip shutdown: how to resume

Paperclip stopped running this repository's work on 2026-10-03 and the project is frozen. Read [`RESUME.md`](RESUME.md) first. It gives the posture, how to recover source, WIP and plans from the [shutdown archive](docs/paperclip/shutdown-2026-10-03/README.md), which extra branches a full check needs, and where the canonical knowledge base lives.

## Digikal knowledge base

Digikal keeps studio-wide context in a private knowledge base,
<https://repo.digikal.com/jawad/digikal-knowledge-base>. It has a page set for this project: the
[overview](https://repo.digikal.com/jawad/digikal-knowledge-base/src/branch/main/knowledge/projects/humm/README.md), with `architecture.md`,
`status.md`, `decisions.md`, `design.md` and `glossary.md` in the same folder.
It also holds the domain guidance that applies across projects. Start at the
[index](https://repo.digikal.com/jawad/digikal-knowledge-base/src/branch/main/knowledge/README.md). The knowledge base's
[`AGENTS.md`](https://repo.digikal.com/jawad/digikal-knowledge-base/src/branch/main/AGENTS.md) is the short form of the rules below.

**Scope.** This repository's own instructions and its verified source keep
their authority here. A knowledge-base page is a summary. It does not override
them, and where the two disagree, trust the verified source and report the
difference.

**Get it.** Read it in the Forgejo web view, or clone it next to this checkout:

```sh
git clone https://repo.digikal.com/jawad/digikal-knowledge-base.git
```

Use your own Forgejo account, or the Git credential helper you already use for
`repo.digikal.com`. The repository is private. If you cannot read it, say so in
your task, ask the repository owner on Forgejo for access, and work from this
repository's own documentation meanwhile. Never put a token or password in a
clone URL, a file, a commit or an issue, and do not rely on a host-local path
to reach the knowledge base.

**Before you work.** Read this project's overview, then the pages your task
touches: `architecture.md` before changing code, `status.md` for current risks
and blockers, `decisions.md` before reversing a choice, and `design.md` for
player- or user-facing work. Read the
[domain hubs](https://repo.digikal.com/jawad/digikal-knowledge-base/src/branch/main/knowledge/README.md#domains) that apply, and any studio
decision (ADR) those pages link to.

**Judge what you read.** Each page's front matter has `confidence`,
`last_reviewed` and `sources`. Check an `assumed` claim before you rely on it.
Check a `reported` claim too when your change depends on it. If a page cites an
older commit of this repository and the code has moved on, the code wins.

**After you work.** If your work produces a decision, a status change, or a
discovery that contradicts a page, propose a sourced update in the knowledge
base. Follow its [`CONTRIBUTING.md`](https://repo.digikal.com/jawad/digikal-knowledge-base/src/branch/main/CONTRIBUTING.md) and
[page conventions](https://repo.digikal.com/jawad/digikal-knowledge-base/src/branch/main/knowledge/CONVENTIONS.md): use a branch in the
knowledge-base repository, give a retrievable source for every changed claim,
make sure `python3 tools/check_kb.py` passes, and open a pull request that is
reviewed before it merges. Do not copy knowledge-base pages into this
repository. If you cannot make the change yourself, record the page path, the
claim, what you found and the source in your task handoff, so the page owner
can make it.
