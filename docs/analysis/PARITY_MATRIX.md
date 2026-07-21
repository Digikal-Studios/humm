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
| F-002 | CLI `add` (full grammar parity; headless is an approved-pending deviation) | cli-int | in-progress (M1 grammar subset — `-t/-s/-l/-p/-w/--srt/--vtt/--txt/-d` + positionals — implemented and cli-int tested, Task 8; URL import, `-m`, `--hfid`, `-e`, `--openai-token`, `--json` deferred to M2+) |
| F-003 | Task table (columns, status text, %/ETA, context actions, persistence of layout) | gui-manual | not-started |
| F-004 | Toolbar & menus with selection-based enablement | gui-manual | not-started |
| F-005 | Customizable shortcuts (21 actions, defaults, live re-apply) | gui-manual + unit (parser) | not-started |
| F-006 | Update check & notify (install flow = deviation candidate) | unit (version compare) + gui-manual | not-started |
| F-007 | Telemetry (parity decision open — Q-1) | n/a until decided | not-started |
| F-008 | UI localization (15 locales) | unit (catalog load) + gui-manual | not-started |
| F-009 | File logging | cli-int | verified (cli-int `log_file_written` executed and passed, Task 8) |
| F-020 | Import files dialog (filters, last-folder) | gui-manual | not-started |
| F-021 | Drag-and-drop import | gui-manual | not-started |
| F-022 | Import URL (yt-dlp) & Import Folder (recursive scan) | contract (yt-dlp) + cli-int + gui-manual | not-started |
| F-023 | Options window (all options, persistence, conditional visibility) | gui-manual + unit (options model) | not-started |
| F-024 | Task queue, background execution, statuses | unit + cli-int | not-started |
| F-025 | Progress & ETA (task, download) | unit + gui-manual | not-started |
| F-026 | Cancel / restart / rename / notes / delete | unit + gui-manual | not-started |
| F-027 | Speech extraction (Demucs) — strategy Q-5 | contract + diff | not-started |
| F-028 | Export on completion (TXT/SRT/VTT, naming template) | snapshot + diff | implemented (snapshot ✅ — Buzz golden fixtures ported + insta snapshot, Task 3; `diff` half runs with M2's differential harness) |
| F-029 | Task persistence (SQLite, startup cancel-flip) | unit + cli-int | not-started |
| F-030 | Folder watch (filters, mirroring, delete/move source) | unit + cli-int | not-started |
| F-031 | LLM translation during transcription (batching, storage) | unit (mock server) + contract | not-started |
| F-040 | Local inference backend(s) — whisper-rs consolidation (ADR; Q-2 for HF/MMS) | contract + diff + bench | implemented (whisper-rs engine, ADR-002 validated M1, ADR-008 consolidation — whisper-rs adapter + 4 contract tests pass; bench baseline recorded in docs/benchmarks/M1.md; ADR-002 whisper-rs-vs-candle validation recorded in validation/adr-002/ + DECISIONS.md — whisper-rs 1.9× faster, 1.75–2.2× less RSS, more complete; diff pending) |
| F-041 | OpenAI-compatible API backend (chunking >25MB, base URL, key) | contract (mock + real) | not-started |
| F-042 | Model sizes & custom models (ggml URL, HF repo id, lumii) | unit + contract | in-progress (ggml sizes incl. lumii implemented + unit tested, Task 5; custom-URL/HF-repo id support deferred to M2) |
| F-043 | Model download & cache (resume, SHA256, sentinels, layout) | unit + contract | in-progress (download/resume/SHA256/sentinel/CoreML zip implemented + unit tested (Task 5) and verified against real huggingface.co network in a contract test (Task 5); `.pt`/HF-hub layouts are N/A per ADR-008 backend consolidation; custom-URL download deferred to M2) |
| F-044 | Model management UI (download/delete/show location) | gui-manual | not-started |
| F-045 | Silero VAD on file transcription (offset remap) | contract + diff | implemented (manual VAD via WhisperVadContext + VadRemap, incl. VAD remap empirics, Task 6; contract test passes — first segment ≥ speech onset on original timeline; diff vs Buzz pending; VAD timestamps sample-exact (more accurate than Buzz's linear rescale) — M2 diff harness must tolerance-compare) |
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
