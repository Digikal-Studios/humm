# humm ↔ Buzz Parity Matrix

The spine of the project. A row reaches `verified` only when its stated verification method has been executed and passed. Statuses: `not-started` → `in-progress` → `implemented` → `verified`. Deviations (Standing Policy or design) noted inline with their ADR once written.

Verification method legend:
- **unit** — Rust unit tests in core crate
- **snapshot** — golden-file/insta snapshot (formats ported from Buzz's inline test fixtures where they exist)
- **cli-int** — integration test invoking the humm binary, asserting output/exit codes/files
- **contract** — adapter contract test against the real external binary/library
- **diff** — differential run: Buzz vs humm on same input, compare structure/formats (not transcript text)
- **gui-manual** — scripted manual GUI checklist (automated where the chosen framework allows)

| Feature ID | Description | Verification method | Status |
|---|---|---|---|
| F-001 | GUI application launch (window, theme, geometry persistence) | gui-manual | not-started |
| F-002 | CLI `add` (full grammar parity; headless is an approved-pending deviation) | cli-int | verified (full M2 grammar — `-m/--hfid/-e/--openai-token/--hide-gui/--json/--quiet` added to the M1 subset — cli-int tested via `humm-cli/tests/cli.rs`'s grammar-matrix suite, M2 Task 10, plus URL/exit-code/Ctrl-C wiring cli-int + acceptance tests, M2 Task 11; every flag parses and every documented behavior is exercised by a passing cli-int test. Deliberate runtime gap, not a testing gap: `-m openaiapi` and `-e` are grammar-accepted and validated (actionable "not yet available" errors, cli-int tested) but have no working backend until M6/M8 — see `docs/HUMM_SURFACE.md` §1) |
| F-003 | Task table (columns, status text, %/ETA, context actions, persistence of layout) | gui-manual | not-started |
| F-004 | Toolbar & menus with selection-based enablement | gui-manual | not-started |
| F-005 | Customizable shortcuts (21 actions, defaults, live re-apply) | gui-manual + unit (parser) | not-started |
| F-006 | Update check & notify (install flow = deviation candidate) | unit (version compare) + gui-manual | implemented (check logic — `is_newer_version`, `should_check`, `parse_version_info` — implemented + unit tested, M2 Task 9, 18 tests incl. the review-round format-contract fix; `gui-manual` half — toolbar notice — not started, pending M3 GUI work) |
| F-007 | Telemetry (parity decision open — Q-1) | n/a until decided | not-started |
| F-008 | UI localization (15 locales) | unit (catalog load) + gui-manual | not-started |
| F-009 | File logging | cli-int | verified (cli-int `log_file_written` executed and passed, Task 8) |
| F-020 | Import files dialog (filters, last-folder) | gui-manual | not-started |
| F-021 | Drag-and-drop import | gui-manual | not-started |
| F-022 | Import URL (yt-dlp) & Import Folder (recursive scan) | contract (yt-dlp) + cli-int + gui-manual | implemented (URL-import half only, reconciling Task 6's early edit: `YtDlpFetcher`/`MediaFetcher` adapter + contract tests against real yt-dlp — download+progress, mid-transfer cancel, capability probe, Task 6, ADR-016 `--no-playlist` deviation noted — **and** now wired end-to-end through the CLI, cli-int/acceptance-tested sandbox-lifted (`acceptance_url_import_downloads_and_transcribes`, Task 11: real yt-dlp + a local HTTP server, asserts a `url_import`-sourced task, downloaded media under the media dir, and a completed export). Folder-import recursive scan is entirely not-started (M3); `gui-manual` not run for either half) |
| F-023 | Options window (all options, persistence, conditional visibility) | gui-manual + unit (options model) | not-started |
| F-024 | Task queue, background execution, statuses | unit + cli-int | verified (`unit`: FIFO/one-at-a-time/statuses/events/cancel/store-write-ordering/engine-caching, M2 Task 5, 13 `queue::` tests incl. the review-round `cancel_all` one-shot fix; `cli-int`: end-to-end queue wiring through the CLI — completed/failed rows, exit codes, `--json` event stream, relaunch stale-flip — M2 Task 11, 22 cli-int tests. Both halves of the stated method ran and passed) |
| F-025 | Progress & ETA (task, download) | unit + gui-manual | implemented (CLI half done: `format_eta` ported from Buzz's ETA formula + unit tested, M2 Task 11 (9 tests); task/download progress plumbing unit tested via the queue's throttling test, M2 Task 5, and additionally cli-int tested end-to-end via the `--json` `progress`/`download_progress` event stream, M2 Task 11 — beyond the row's stated `unit` requirement, which is fully satisfied either way; `gui-manual` half — on-screen %/ETA — not started, pending M3) |
| F-026 | Cancel / restart / rename / notes / delete | unit + gui-manual | implemented (cancel-only, of the five sub-features: `unit` — pending/current/`cancel_all` semantics + cancel-interleaving in `pipeline.rs`, M2 Task 5 (13 tests, incl. the review-round one-shot-`cancel_all` fix); real-process cancel additionally proven via two sandbox-lifted acceptance tests invoking the actual CLI binary, M2 Task 11 — `acceptance_kill9_leaves_recoverable_canceled_row` (SIGKILL → stale row recoverable as `canceled` on next launch) and `acceptance_ctrl_c_cancels_and_exits_130` (SIGINT → exit 130). Restart/rename/notes/delete are entirely not-started (M3, GUI-only actions in Buzz); `gui-manual` not run) |
| F-027 | Speech extraction (Demucs) — strategy Q-5 | contract + diff | not-started |
| F-028 | Export on completion (TXT/SRT/VTT, naming template) | snapshot + diff | implemented (snapshot ✅ — Buzz golden fixtures ported + insta snapshot, M1; `diff` half: M2's differential harness (Task 12) ran but real Buzz-vs-humm output comparison was blocked — see F-045's note and `validation/m2-differential/README.md`; what ran instead was a structural self-consistency check of humm's own TXT/SRT/VTT round-tripped through a Python mirror of the format law, 7/7 PASS. `diff` half against real Buzz artifacts stays pending, carried to M3) |
| F-029 | Task persistence (SQLite, startup cancel-flip) | unit + cli-int | verified (`unit`: `SqliteStore` schema/repository lifecycle, M2 Task 3 (12 tests) + one-time Buzz-import (dedup, segment scoping, corrupt-DB rollback, DETACH-error surfacing), M2 Task 4 (4 integration + 2 review-fix tests); `cli-int`: startup cancel-flip proven through the real CLI binary — `relaunch_flips_stale_in_progress_row_to_canceled` and the sandbox-lifted `acceptance_kill9_leaves_recoverable_canceled_row`, M2 Task 11. Both halves of the stated method ran and passed) |
| F-030 | Folder watch (filters, mirroring, delete/move source) | unit + cli-int | not-started |
| F-031 | LLM translation during transcription (batching, storage) | unit (mock server) + contract | not-started |
| F-040 | Local inference backend(s) — whisper-rs consolidation (ADR; Q-2 for HF/MMS) | contract + diff + bench | implemented (whisper-rs engine, ADR-002 validated M1, ADR-008 consolidation — whisper-rs adapter + 4 contract tests pass; bench baseline recorded in docs/benchmarks/M1.md; ADR-002 whisper-rs-vs-candle validation recorded in validation/adr-002/ + DECISIONS.md — whisper-rs 1.9× faster, 1.75–2.2× less RSS, more complete; diff pending) |
| F-041 | OpenAI-compatible API backend (chunking >25MB, base URL, key) | contract (mock + real) | not-started |
| F-042 | Model sizes & custom models (ggml URL, HF repo id, lumii) | unit + contract | implemented (ggml sizes incl. lumii — implemented + unit tested, M1; custom-model-URL (`ensure_custom_model`) and HF-repo-id resolution (`resolve_hf_repo`, ADR-008 "Custom-model mechanics") — implemented + unit tested against a local `TestServer`, M2 Task 8, 11 tests incl. openai-prefix delegation, single/multiple/zero-candidate resolution, 404/malformed-JSON. `contract` is satisfied for the shared download primitive both new methods call through (`ensure_artifact` — resume/verify/sentinel), re-verified against real huggingface.co after Task 7's refactor (2/2 passed); no contract test exercises an actual arbitrary custom URL or third-party HF repo against the live network (not practical — there is no fixed real target), so kept at `implemented` rather than `verified` for this row's own distinctive surface) |
| F-043 | Model download & cache (resume, SHA256, sentinels, layout) | unit + contract | verified (download/resume/SHA256/sentinel/CoreML-zip mechanics — `unit`: M1 baseline + M2 Task 7's HTTP-seam refactor and 4 new pinning tests (ticket a/b/c: CoreML-zip-fetch-when-missing, 416-recovery-promotes-verified-partial, 416-after-restart-errors-without-looping, placement-failure-redownloads) plus 5 registry `*_with_base` tests and a full offline `resolve_model` happy-path test, all `crates/humm-core/src/models/download.rs`/`registry.rs`; `contract`: `cargo test -p humm-core --test model_download_contract -- --ignored` re-run after the refactor, 2/2 passed against real huggingface.co (resume + HTTP-416-recovery-of-a-completed-partial, both exercising the exact primitive the CoreML-zip fix now shares). Both halves of the stated method ran and passed. `.pt`/HF-hub layouts N/A per ADR-008 backend consolidation) |
| F-044 | Model management UI (download/delete/show location) | gui-manual | not-started |
| F-045 | Silero VAD on file transcription (offset remap) | contract + diff | implemented (manual VAD via WhisperVadContext + VadRemap, incl. VAD remap empirics, M1; contract test passes — first segment ≥ speech onset on original timeline; first-run VAD-model download restored end-to-end in the CLI after a review-round regression catch — M2 Task 11 found the M1-parity "cache-only" reasoning was actually an F-045 regression (Buzz ships the VAD model at build time, so every Buzz user has it by default) and fixed it: `ensure_vad()` now downloads with progress + graceful degradation on failure, proven by all 4 sandbox-lifted acceptance tests incl. a real first-run VAD fetch; diff vs Buzz still pending — M2 Task 12 time-boxed a real Buzz run (blocked: no venv, whisper.cpp submodule uninitialized, broken system LLVM toolchain broke the from-scratch Python env build — see `validation/m2-differential/README.md` for the full attempt log) and executed the brief's structural-only fallback instead: humm's own TXT/SRT/VTT round-tripped byte-identically through a Python mirror of `export.rs` self-tested against Buzz's ported golden fixtures, 7/7 PASS. VAD timestamps remain sample-exact (more accurate than Buzz's linear rescale) — the real numeric diff (tolerance-compare against Buzz's own output) is the concrete carry-over item for M3) |
| F-060 | Live recording window (device, meter, options) | gui-manual | not-started |
| F-061 | Real-time transcription loop (chunking, silence cuts, rolling prompt) | unit (chunker) + gui-manual | not-started |
| F-062 | Display modes incl. append-and-correct merge | unit (merge algorithm) + gui-manual | not-started |
| F-063 | Live translation panel | gui-manual + unit | not-started |
| F-064 | Live export TXT/CSV + upload URL | unit + snapshot | not-started |
| F-065 | Presentation window & clipboard copy | gui-manual | not-started |
| F-080 | Viewer window (view modes, layout persistence) | gui-manual | not-started |
| F-081 | Segment editing (timestamps ±500ms, overlap clamp, instant save) | unit + gui-manual | not-started |
| F-082 | Transcript search (debounce, cap, next/prev) | unit + gui-manual | not-started |
| F-083 | Synced audio/video playback (loop, follow, speed) | gui-manual | not-started |
| F-084 | Export from viewer (format × text/translation) | snapshot + gui-manual | not-started |
| F-085 | Post-hoc Translate | unit (mock) + gui-manual | not-started |
| F-086 | Resize/merge transcript (line-length fit, regroup rules) | unit + diff | not-started |
| F-087 | Speaker identification — strategy Q-3 | contract + diff | not-started |
| F-100 | Preferences: General tab | gui-manual | not-started |
| F-101 | Preferences: Models/Shortcuts/Folder-Watch tabs | gui-manual | not-started |
| F-110 | Plugin system — strategy Q-4 | per-strategy | not-started |
| F-111 | Plugin: ai_summary | per-strategy | not-started |
| F-112 | Plugin: deep_filter_net | per-strategy | not-started |
| F-113 | Plugin: export_docx | snapshot | not-started |
| F-114 | Plugin: enhanced_language_detection | per-strategy | not-started |
| F-115 | Plugin: skip_already_transcribed | unit | not-started |
| F-116 | Plugin: transcript_resizer | unit + diff | not-started |

50 rows. Project definition of done: every row `verified`.
