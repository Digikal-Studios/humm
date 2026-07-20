# CLAUDE.md — humm

humm is a Rust port of Buzz (`/Users/jawad/RustroverProjects/buzz`) with **100% functional parity** and **decisively better performance** — in that order. The Buzz *source code* is the sole ground truth for behavior; never rely on prior knowledge of Buzz or assumptions about transcription apps.

## Read before coding

1. `docs/analysis/PARITY_MATRIX.md` — the project spine. Done = every row `verified`.
2. `docs/analysis/FEATURE_INVENTORY.md` — feature IDs (F-###) with Buzz source refs. **Re-read the cited Buzz files before implementing a feature** — implement from source, not memory.
3. `docs/design/ARCHITECTURE.md`, `docs/design/DECISIONS.md` (ADRs), `docs/design/UX_SPEC.md`, `docs/design/IMPLEMENTATION_PLAN.md` (current milestone).
4. `docs/OPEN_QUESTIONS.md` — never invent behavior; log genuine ambiguity there.

## Commands

```bash
cargo fmt --check                                        # formatting gate
cargo clippy --workspace --all-targets -- -D warnings    # lint gate
cargo test --workspace                                   # unit + integration + snapshot
cargo test -p humm-media -- --ignored                    # contract tests (need ffmpeg/yt-dlp)
cargo bench                                              # criterion suite (decode/resample/e2e/export)
cargo run -p humm-cli -- add <file> --srt                # CLI smoke
cargo run -p humm-gui                                    # GUI
```

Every commit must pass the first three. Commit at coherent boundaries; messages reference milestone + feature IDs (e.g. `M4: segment editor overlap clamping (F-081)`).

## Workspace

`crates/humm-core` (domain library, `#![warn(missing_docs)]`, zero UI deps, no `unsafe`) · `humm-media` (ffmpeg/ffprobe/yt-dlp subprocess adapters) · `humm-whisper` (whisper-rs FFI; the only crate allowed `unsafe`, documented) · `humm-audio` (cpal+rubato) · `humm-store` (rusqlite+keyring) · `humm-cli` · `humm-gui` (egui/eframe). Binaries → adapters → core; core imports no adapter.

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
