# humm — Implementation Plan

Vertical slices: each milestone ships a coherent feature group end-to-end (core → CLI → GUI once the GUI exists). Every commit passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. `PARITY_MATRIX.md` statuses updated with every milestone; criterion benches tracked from M1 — a transcription-path regression blocks the milestone.

Before implementing any milestone: **re-read the relevant Buzz source files** (listed per feature in `../analysis/FEATURE_INVENTORY.md`) — implement from source, not from Phase 0 memory.

---

## M1 — A file becomes a transcript (headless core path)
**Parity IDs:** F-002 (partial), F-009, F-028, F-040 (local engine), F-042 (sizes, partial), F-043 (download, partial), F-045.
**Scope:**
- Workspace scaffolding + CI quality gates; `humm-core` domain types + export writers; `humm-media` (ffmpeg/ffprobe adapter); `humm-whisper` (Metal+CoreML, VAD, abort callback, segment-callback sink); minimal model download (ggml from HF, resume, sentinel); `humm-cli` with `add` accepting files, `--model-size`, `--language`, `--task`, `--prompt`, `--word-timestamps`, `--srt/--vtt/--txt`, `--output-directory`.
- **Required: whisper-rs vs candle validation benchmark** (ADR-002) — wall-clock, peak RSS, output quality on real model + `buzz/testdata` audio; numbers recorded in DECISIONS.md.
- **Contract-test scaffolding for every adapter introduced** (ffmpeg, ffprobe, whisper-rs) + snapshot tests porting Buzz's golden SRT/VTT/timestamp fixtures + criterion suite (decode, e2e, export).
**Acceptance:** `humm add audio.mp3 --srt --txt -s small` produces exports byte-format-identical to the documented contract; golden snapshots pass; VAD on/off produces correctly-remapped timestamps; benchmark recorded; all gates green.

## M2 — Tasks are durable; models are managed; URLs import (CLI complete)
**Parity IDs:** F-022 (URL half), F-024, F-025 (CLI), F-026 (cancel), F-029, F-042/F-043 (full), F-006 (check logic only).
**Scope:** `humm-store` (schema, repository, status transitions, one-time Buzz.sqlite import per Q-6); task queue + event bus; `humm-media` yt-dlp adapter (+contract tests, graceful absence); custom models (ggml URL, HF Whisper-family repo per Q-2); full CLI grammar incl. URLs, `--json`, exit codes, Ctrl-C cancel; settings file + `HUMM_*` env surface.
**Acceptance:** kill -9 mid-task → relaunch shows Canceled with retryable full options; Buzz DB import verified against a fixture; CLI integration tests cover the grammar matrix; differential check vs Buzz `add` artifacts.

## M3 — The GUI shell (import → queue → progress → done)
**Parity IDs:** F-001, F-003, F-004, F-005 (defaults), F-007, F-008 (scaffold), F-020, F-021, F-022 (GUI), F-023, F-025, F-026 (full), F-044 (basic download/delete).
**Scope:** eframe app; main window + virtualized task table (columns/sort/context menu/persisted layout); options sheet; drag-drop; URL/folder import; non-modal model downloads (UX §J-1 ✚); cancel/retry/rename/notes/clear; telemetry (ADR-009) + Privacy tab stub; update notify (ADR-015); fluent scaffolding (en).
**Acceptance:** J-1 runs end-to-end in GUI; 100k-segment table scroll bench ≥ 60 fps; multi-window smoke on macOS; gui-manual checklist v1 executed.

## M4 — The viewer (review, edit, export)
**Parity IDs:** F-080, F-081, F-082, F-083, F-084, F-086.
**Scope:** viewer window (view modes, persisted layout); segment editor (nudges, overlap clamp, instant save); search; audio playback (cpal sink, loop/follow/speed); video (ffmpeg rawvideo → texture, A/V sync); export menu; resize/merge (line-length fit + gap-merge/punct-split rules).
**Acceptance:** edits survive restart; A/V stays in sync ±50 ms over 10 min; exports from viewer match M1 writers (shared code); resize outputs diffed against Buzz behavior on fixtures.

## M5 — Live recording
**Parity IDs:** F-060, F-061, F-062, F-064, F-065 (+Q-10 persistence ✚).
**Scope:** `humm-audio` capture (+mock impl for CI); meter; realtime loop (chunking, RMS silence cuts, rolling prompt — ported from `recording_transcriber.py` behavior); three display modes incl. append-and-correct merge (unit-tested against Buzz's merge semantics); live TXT/CSV export + upload URL; presentation window; clipboard copy; live sessions persisted.
**Acceptance:** live transcription with `small` sustains realtime on Apple Silicon; merge algorithm unit tests pass on Buzz-derived cases; CSV/TXT exports match documented formats.

## M6 — Cloud: API backend + LLM translation
**Parity IDs:** F-031, F-041, F-063, F-085.
**Scope:** OpenAI-compatible transcription engine (custom base URL, model setting, >25 MB chunking via ffmpeg, key in keyring + Test button); LLM translator (batch-of-10 `[N]` protocol, env overrides); viewer Translate; live translation panel + `.translated` export.
**Acceptance:** contract tests against a local mock server + one recorded real-API run; translation batching parses Buzz-style responses; keys never leave the keyring.

## M7 — Completeness: preferences, folder watch, native "plugins", locales
**Parity IDs:** F-005 (editor), F-008 (locale ports begin), F-030, F-044 (full), F-100, F-101, F-110 (ADR-010 resolution), F-111, F-113, F-115, F-116.
**Scope:** full Preferences (General/Models/Shortcuts+conflict detection/Folder Watch/Privacy); folder watcher (`notify`-based, Buzz skip-rules, structure mirroring, delete/move source); ai-summary, docx export, skip-already-transcribed, auto-resizer as native features; shortcut editor; first locale ports.
**Acceptance:** folder-watch integration test (drop → auto task → mirrored output); docx snapshot; per-feature gui-manual checks; parity matrix sweep — everything except M8 rows `implemented`+.

## M8 — Heavy ML: speech isolation, speaker ID, noise reduction
**Parity IDs:** F-027, F-087, F-112, F-114.
**Scope (per ADR-011):** `SpeechIsolator` via ort/htdemucs-ONNX or demucs subprocess (decide by benchmark); `Diarizer` via ort (segmentation+embeddings+clustering) behind the two-step naming UI; DeepFilterNet via native `df` crates; enhanced language detection natively.
**Acceptance:** outcome-level diffs vs Buzz on fixture media (speech isolated, speakers found/nameable/mergeable, noise reduced); graceful absence when models/`ort` unavailable; final parity-matrix sweep → all rows `verified`; full benchmark report vs Buzz (startup, e2e transcription, memory).

---

## Sequencing rationale
Minimal usable transcription exists at M1 exit (mission requirement). Store-before-GUI (M2→M3) lets the GUI bind to real persistence on day one. Viewer before live (M4→M5) because live-session persistence (Q-10) reuses the viewer. Cloud (M6) is independent and slots after the local path is trustworthy. The two genuinely-risky ML integrations are last (M8) with traits already in place, so slips can't destabilize the core product.

## Standing risk register
- `ort` still 2.0-rc at design time → re-evaluate at M8; subprocess fallbacks sketched (ADR-011).
- CoreML encoder first-load latency (Buzz shows a tooltip) → measure in M1, surface in UX if real.
- egui a11y/i18n gaps → AccessKit + fluent validated in M3, before deep GUI investment.
