# DIG-14: Scan and report: humm

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (2026-10-03 final delta (records exported from 2026-10-03T16:42:39Z)) | done |
| Priority | medium |
| Owner (assignee) | TechLead |
| Created by | TechLead |
| Paperclip project | humm |
| Primary repository archive | humm |
| Parent | [DIG-4](DIG-4.md) |
| Created / updated | 2026-09-25T13:52:12.464Z / 2026-09-25T14:00:04.549Z |
| Completed / cancelled | 2026-09-25T14:00:04.381Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `dca0f1f2-2b88-4104-a5ec-619469e5158f` |

## Description

##### Goal
Produce the status report for the Digikal codebase **humm**. This is one of 12 per-repo children of [DIG-4](DIG-4.md).

##### Repository
- URL: `https://repo.digikal.com/jawad/humm.git` (self-hosted Forgejo, private)
- Branch to scan: `main`
- Plain `git clone` over HTTPS works from the runner (host credential store). No token handling needed. If it ever fails, `$FORGEJO_TOKEN` with username `jawad` is the fallback.

##### Seed inventory (already verified this run, 2026-09-25)
Rust workspace (`crates/`, `docs/`, `validation/`, `.github/`). 244 commits, last commit 2026-07-28 (sandbox config allowing loopback binds and pypi/huggingface network access — suggests an ML/Python-adjacent component). Check for a Python or model-inference boundary.

##### Method
Read-only. Clone into `$PAPERCLIP_SCRATCH_DIR`, do not build, install, run, or modify anything, and never commit to or reformat the repo. Follow the discovery method in your instructions:

1. Inventory: top-level layout, manifests, engine/framework markers, build and CI config, docs, licenses.
2. Architecture: main modules, data flow, external services, persistence, networking, save systems, content pipelines.
3. State of completion: what runs end to end, what is stubbed, what is half-built. Cite file paths.
4. Intent: TODO/FIXME, roadmap files, design docs, changelogs, recent commit history, branch names.
5. Risks: debt, missing tests, fragile areas, licensing, committed secrets (report the path and kind, NEVER the value).
6. Open questions for the founder, phrased so a yes/no or short answer resolves them.

##### Deliverable
An issue document with key `report` on THIS issue, containing:

- One-paragraph summary a stranger could read and correctly say what the project is.
- Stack and engine, with versions where found (cite the manifest/toolchain file).
- Architecture overview: main modules and their responsibilities.
- Three lists — **Built**, **In progress**, **Planned** — every item citing a file path or commit/branch as evidence.
- Risks and debt, ranked.
- Open questions for the founder.

Then post a short comment on this issue pointing at the report and give a one-line state verdict (e.g. "active development", "dormant baseline", "artifact archive"). Also name, with evidence, any specialist expertise this codebase needs (engine, backend, platform, tooling) — [DIG-4](DIG-4.md) rolls that up for the CEO's team expansion proposal.

##### Done when
The `report` document exists and the summary comment is posted. Then set this issue to `done` — [DIG-4](DIG-4.md) is blocked on it and wakes when all 12 children are done. No separate review needed on this child.

##### Not your call here
Game design, UI, and UX judgments belong to the Game & Experience Designer ([DIG-7](DIG-7.md)). Note them as observations; do not adjudicate them.


## Documents (latest revision)

<a id="document-report"></a>

### Document `report`: humm — Status Report

Revision 1 (id `50b99136-adcc-4860-b62d-edd9dc4b27df`), updated 2026-09-25T13:59:39.052Z by TechLead.

#### humm — Status Report

**Repo:** `https://repo.digikal.com/jawad/humm.git` · branch `main` @ `0864269`
**Scanned:** 2026-09-25, read-only, from the workspace checkout. 244 commits, 399 tracked files, ~106k lines of Rust across 9 crates, ~12.9k lines of Markdown under `docs/`. Nothing was built, installed, run, or modified.

---

##### 1. Summary

humm is a **macOS desktop + CLI audio/video transcription application written in Rust** — a from-scratch reimplementation ("port") of **Buzz**, the Python/PyQt6 Whisper GUI. It transcribes local media files, URLs (via `yt-dlp`), watched folders, and live microphone input using whisper.cpp (Metal + Core ML) or an OpenAI-compatible HTTP API; it stores transcripts in SQLite, offers a transcript viewer/editor with synced A/V playback, exports TXT/SRT/VTT/CSV/DOCX, and layers on ML post-processing: Demucs speech isolation, DeepFilterNet3 noise reduction, speaker diarization with a naming UI, LLM translation, and AI summaries. It ships a GUI binary (`humm-gui`, egui/eframe) and a headless CLI (`humm`, clap) over one shared core library. The project was executed as a single disciplined 8-day mission (2026-07-20 → 2026-07-28) against an explicit governing brief (`docs/MISSION.md`) that defined "done" as **every row of a 50-row parity matrix verified**. That bar was reached: `docs/analysis/PARITY_MATRIX.md` records a final state of **46 verified / 4 named, blocker-documented exceptions**, with a mission-closing benchmark against a from-source Buzz build (`docs/benchmarks/MISSION.md`). It has not been touched in the ~2 months since. **This is a completed, self-declared-done project, not work in flight.**

---

##### 2. Stack and versions

All versions below are from `Cargo.toml` (workspace `[workspace.dependencies]`) unless noted; `Cargo.lock` is committed (778 resolved packages).

| Layer | Choice | Version / evidence |
|---|---|---|
| Language | Rust, **edition 2024**, resolver 3 | `Cargo.toml:14` (`edition = "2024"`), `Cargo.toml:3` |
| Toolchain pin | **none** — no `rust-toolchain.toml`, no `rust-version` in any manifest | CI uses `dtolnay/rust-toolchain@stable` (`.github/workflows/ci.yml`) |
| Inference | `whisper-rs` → vendored whisper.cpp, Metal + Core ML | `whisper-rs = "0.16"`; ADR-002 (`docs/design/DECISIONS.md:12`) records the validation benchmark vs `candle` |
| GUI | `egui` / `eframe` **0.35** (+ `egui_extras`, `egui_kittest`, `rfd 0.17`) | ADR-006 (`DECISIONS.md:52`) |
| CLI | `clap` 4 (derive) | ADR-007 (`DECISIONS.md:67`) |
| Media | **ffmpeg / ffprobe / yt-dlp / demucs as subprocesses** — no libav bindings | ADR-003, ADR-016, ADR-023 |
| Persistence | `rusqlite 0.40` (`bundled` SQLite), WAL | ADR-004 |
| Secrets | `keyring 4` | ADR-012 |
| Audio | `cpal 0.18` capture/playback + `rubato 4` resample + `rtrb 0.3` lock-free ring | ADR-005, ADR-021 |
| Noise reduction | `deep_filter`, **git-pinned** to `Rikorose/DeepFilterNet` rev `978576aa` (tag v0.5.6), `tract` backend, model embedded via `include_bytes!` | `Cargo.toml` `[workspace.dependencies]`; crates.io release rejected as stale |
| Diarization | `speakrs =0.5.0` + `ort =2.0.0-rc.12` (ONNX Runtime FFI), `openblas-system`, `coreml` | ADR-024 (`DECISIONS.md:152`) |
| i18n | `fluent 0.17`, **15 locales shipped** | ADR-014; locale commits `c25daa8`, `61def69`, `1b3f1bb`, `191081d` |
| Telemetry | PostHog raw `/capture/` API, key injected at build time via `option_env!("HUMM_POSTHOG_KEY")` | ADR-009; `crates/humm-core/src/telemetry.rs`, `crates/humm-gui/src/services.rs:856` |
| Tests | 1976 `#[test]` fns, 70 `#[ignore]`d (contract/heavy), `insta` snapshots, `criterion` benches, `egui_kittest` GUI tests | `crates/`, counted by grep |
| Crate versions | every crate `0.1.0` — never bumped | `crates/*/Cargo.toml:3` |
| License | `license = "MIT"` declared in `[workspace.package]`; **no `LICENSE` file exists in the repo** | `Cargo.toml:15`; `find -iname 'LICENSE*'` returns only `validation/*/README.md` hits |

**Notably absent:** no root `README.md`, no `LICENSE`, no packaging/bundling config, no release workflow, no Dockerfile, no Python package in the product (the `.py` files under `validation/` are dev-time fixture generators and Buzz-side differential harnesses only).

###### The Python / ML boundary (the seed question)

The `pypi`/`files.pythonhosted.org`/`huggingface.co` allowlist and loopback-bind entries in the final commit `0864269` are in `.claude/settings.json` — an **AI-coding-agent sandbox config**, not product config. The real boundaries behind those domains are:

- **Python, subprocess-only, optional:** `demucs` (F-027) is invoked as an external CLI (`crates/humm-media/src/demucs.rs`, `HUMM_DEMUCS` override, `pipx install demucs` hint). It is *not* a dependency — absence degrades the feature with an actionable message. Nothing else in the shipping product is Python.
- **`huggingface.co`:** runtime model downloads — ggml Whisper weights and Core ML encoder zips from `ggerganov/whisper.cpp`, Silero VAD from `ggml-org/whisper-vad`, diarization models from `avencera/speakrs-models` (~60 MB on first diarization run). `crates/humm-core/src/models/download.rs`, `crates/humm-diarize/src/lib.rs`.
- **`pypi` + loopback:** dev/benchmark only — `uv sync` of a real Buzz checkout for the closing differential and benchmark (`docs/benchmarks/MISSION.md` §1), plus mock OpenAI servers bound to `127.0.0.1` in tests (`validation/m6-real-api/oai_shim.py`).
- **In-process ONNX:** `ort` (ONNX Runtime, C++ FFI) for diarization and `tract` (pure Rust) for DeepFilterNet3. ONNX Runtime binaries are fetched from `cdn.pyke.io` at build time by `ort`'s `download-binaries` feature.

---

##### 3. Architecture

Cargo workspace, strict one-direction dependency rule: **binaries → adapters → core**, core imports no adapter and no UI. Design of record: `docs/design/ARCHITECTURE.md`.

| Crate | LOC | Responsibility |
|---|---|---|
| `humm-core` | 37.4k | **The library.** Domain model (`Millis`, `Segment`, `JobSpec`, `TranscriptionOptions`), all boundary traits, the task queue + event bus, export writers (TXT/SRT/VTT/DOCX), model registry + downloader, OpenAI-compatible transcription engine, LLM translator, regroup/resize engine, live-session state machine, post-processing chain, settings, telemetry, update check. `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`, zero UI deps. |
| `humm-gui` | 50.4k | egui/eframe binary. Main window + virtualized task table, options sheet, six-tab Preferences, transcript viewer (deferred viewports, A/V playback, segment editor, search, export menu), recording + presentation windows, Speaker-ID two-step window, models UI, folder-watch wiring, i18n. |
| `humm-media` | 3.9k | **Adapter:** ffmpeg decode/encode, ffprobe JSON probe, rawvideo frame streaming, `yt-dlp` fetch, `demucs` isolation. Owns all subprocess management, process-group kill, binary probing. |
| `humm-whisper` | 2.7k | **Adapter:** whisper-rs FFI. The **only** crate with `unsafe` (7 sites in `src/engine.rs`: C callback trampolines for progress/segment/abort). Also Silero VAD and the standalone language detector. |
| `humm-store` | 4.1k | **Adapter:** rusqlite repository, schema, one-time Buzz.sqlite import, keyring secrets. |
| `humm-audio` | 2.9k | **Adapter:** cpal capture + playback, rubato resampling, mock impl for CI. |
| `humm-dfn` | 0.6k | **Adapter (M8):** DeepFilterNet3 via `deep_filter`+tract. Composes `humm-media` for decode/encode — an adapter consuming a peer adapter, explicitly sanctioned in `ARCHITECTURE.md:32`. |
| `humm-diarize` | 0.5k | **Adapter (M8):** speaker diarization. The only crate naming `ort`/`speakrs`. |
| `humm-cli` | 3.5k | clap binary, genuinely headless (Buzz's CLI drove a hidden GUI — logged deviation, ADR-007). `--json` event stream, real exit codes (0/1/2). |

**Boundary traits** (all defined in `humm-core`, `src/boundary.rs` + `src/diarize.rs`): `AudioDecoder`, `AudioEncoder`, `MediaProber`, `VideoStreamer`/`FrameStream`, `AudioOutput`/`OutputStream`, `AudioCapture`/`CaptureHandle`, `MediaFetcher`, `TranscriptionEngine`, `SegmentSink`, `SpeechIsolator`, `NoiseReducer`, `LanguageDetector`, `Diarizer`, `SecretStore`, `Telemetry`. Every one has a contract-test suite in its adapter crate, so an external upgrade is re-validated by those tests alone (Standing Policy rule 5).

**Concurrency** (deliberately per-subsystem, `ARCHITECTURE.md` §3 — **no tokio anywhere**, the design-time async facade was dropped as unnecessary):
- Task queue: one `TaskRunner` thread owning a FIFO, commands in over `flume`, `TaskEvent`s out over broadcast. One transcription at a time (parity; GPU contention).
- Inference: one dedicated OS thread per task; cancel via whisper.cpp's abort callback polling a `CancelToken`.
- Live capture: cpal RT callback → `rtrb` lock-free SPSC ring → one session thread running the chunker/silence-cut state machine. Never allocates in the audio callback.
- Playback: cpal fill callback reads whole-file in-memory PCM; position published as one packed `{generation, frame}` `AtomicU64` — the A/V master clock; seeks by a single CAS.
- HTTP: `reqwest::blocking` on whichever thread wants it; cancel-during-request via `openai::client::cancelable` (run on a short-lived thread, detached on cancel — ADR-022).
- GUI: eframe event loop, `request_repaint()` on event arrival, ~zero idle CPU.

**Data flow.** `TaskEvent { task_id, kind: Queued | DownloadProgress | Started | SegmentAdded | Progress | Completed | Failed | Canceled | Skipped | PostProcessed | PhaseChanged }` is the single source of truth, consumed by the GUI table, the CLI renderer, exporters, and the store. The full `JobSpec` is persisted with the queue record — a deliberate structural fix for a Buzz bug where restart lost task options.

**The pre-transcription seam** (`queue::Runner::run_job`, fixed order, each independently gated): speech isolation (failure **fails** the task, Buzz parity) → noise reduction (failure degrades, `catch_unwind`-contained) → language detection (failure degrades to auto) → transcription. `TempAudioGuard` removes every derivative file on every exit path including panic unwind. Diarization is *not* in this chain — it runs on demand from the Speaker-ID window.

**Persistence:** SQLite (WAL) with a humm-owned schema that is a superset of Buzz's, plus `options_json` and a `kind` column admitting live-session records. Settings are a typed serde struct → TOML at the platform config dir, atomic temp+rename write. Secrets in keyring. Models under `<data_dir>/humm/models/` with `.humm_complete` sentinels, resume, and SHA256 verification. Observable contract documented in `docs/HUMM_SURFACE.md` (CLI grammar, `--json` schema, exit codes, 40+ `HUMM_*` env vars, file locations, first-run migration).

---

##### 4. Built

Everything here is `verified` in `docs/analysis/PARITY_MATRIX.md` — meaning the row's own stated verification method (unit / snapshot / cli-int / contract / diff / gui-manual) was actually executed and passed, with the evidence cited inline in the row.

- **File → transcript, headless.** `humm add <file> --srt --txt -s small`, ffmpeg decode → Silero VAD → whisper.cpp (Metal+Core ML) → exports. Snapshot-tested against Buzz's own golden SRT/VTT fixtures ported verbatim. `crates/humm-cli/tests/cli.rs`, `crates/humm-core/src/export/`, F-002/F-009/F-028/F-040/F-045.
- **Full CLI grammar** with Buzz flag spelling preserved, plus `--json` (one event object per line), real exit codes, stderr progress, Ctrl-C cancel. `docs/HUMM_SURFACE.md` §1–§4.
- **Durable task queue.** FIFO, one-at-a-time, full status machine, cancel/restart, kill-9 survivability with startup stale-flip, SQLite persistence, one-time Buzz.sqlite import. F-024/F-026/F-029; `crates/humm-core/src/queue.rs`, `crates/humm-store/tests/buzz_import.rs`.
- **Model management.** ggml sizes incl. Lumii, custom ggml URL, HF Whisper-family repo resolution, resumable download, SHA256, Core ML zip handling, sentinel layout, GUI download/delete/reveal. F-042 (partial, see exceptions)/F-043/F-044.
- **URL and folder import.** `yt-dlp` adapter with progress + mid-transfer cancel (contract-tested against the real binary), recursive folder scan, `notify`-based folder watch with Buzz's skip rules, structure mirroring, delete/move source. F-022/F-030.
- **GUI shell.** Virtualized task table (100k segments at 60 fps — `crates/humm-gui/benches/table_scroll.rs`), toolbar/menus with selection-based enablement, 20 customizable shortcuts, options sheet, drag-and-drop (windowed OS-level drop verified in `docs/gui-manual/M8.md` §13.1), six-tab Preferences, toasts, About, update notice. F-001/F-003/F-004/F-005/F-020/F-021/F-023/F-100/F-101.
- **Transcript viewer.** Deferred-viewport windows, view modes, persisted layout, inline segment editor (±500 ms nudges, overlap clamp, instant save), debounced search, synced audio+video playback with loop/follow/varispeed (A/V drift acceptance run windowed), export menu, resize/merge. F-080–F-084, F-086.
- **Live recording.** Device selection, RMS meter, real-time chunking with silence cuts and rolling prompt, three display modes including append-and-correct merge, live TXT/CSV export + upload URL, presentation window, clipboard copy, and live sessions persisted to the DB (an approved improvement over Buzz, Q-10). F-060–F-065.
- **Cloud paths.** OpenAI-compatible transcription engine with >25 MiB chunking, custom base URL, key in keyring, Test button; LLM translator implementing Buzz's batch-of-10 `[N]` protocol byte-faithfully; viewer Translate; live translation panel. F-041 verified; F-031/F-111 implemented (see exceptions).
- **The six Buzz plugins as native features** — no plugin mechanism was ported (ADR-010). ai_summary, DOCX export, skip-already-transcribed, transcript_resizer, DeepFilterNet3 noise reduction, enhanced language detection. F-110/F-112/F-113/F-114/F-115/F-116.
- **Heavy ML (M8).** Demucs speech isolation via subprocess with graceful absence; DeepFilterNet3 with the model embedded at compile time (no download, no absence path); diarization via `speakrs`/`ort` behind a two-step Speaker-ID window (found/named/merged, outcome-diffed on a constructed two-speaker fixture); whisper-rs standalone language detection writing back before transcription. F-027/F-087/F-112/F-114. Raw evidence: `validation/m8-acceptance/` (43 screenshots, astats measurements, contract-suite log).
- **15 UI locales**, with a bundled CJK font closing the tofu-glyph gap. F-008; commits `c25daa8`, `61def69`, `1b3f1bb`, `191081d`.
- **Quality gates in CI** on macOS: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. `.github/workflows/ci.yml`.
- **Benchmark evidence vs Buzz itself**, same machine, same model weights (md5-matched on both sides), same VAD model, same Metal+Core ML backend: startup-to-interactive **6.19×** faster (0.55 s vs 3.42 s), e2e transcription **1.83×** pooled / **1.47×** outlier-excluded, idle GUI RSS **5.50×** lighter (114 MB vs 630 MB), peak RSS under load a **tie** (+0.8%). `docs/benchmarks/MISSION.md`.

---

##### 5. In progress

**Nothing is in flight.** There is one branch (`main`), no open feature branches, no WIP commits, zero `TODO`/`FIXME`/`todo!()`/`unimplemented!()` anywhere in `crates/` (grep-verified). The last commit is a sandbox-config chore. The four parity rows that are not `verified` are **not** unfinished code — all four are `implemented` with the code complete and the *verification* blocked (`docs/analysis/PARITY_MATRIX.md` FINAL-state table):

| Row | Code state | What is blocked |
|---|---|---|
| F-006 Update check & notify | Check logic implemented + 18 unit tests | The update-*available* dialog can't be shown: `update::VERSION_INFO_URL` is a compile-time constant with no env override and no humm release is published there. The real startup check ran windowed and degraded silently on 404. |
| F-031 LLM translation | `openai::translate` complete, mock-server tested | No OpenAI-compatible **chat** endpoint on the dev machine. Closer is committed and runnable: `real_chat_api_single_and_batch_round_trip`, `#[ignore]` + env-gated `HUMM_LLM_TEST_BASE`/`_KEY`/`_MODEL`. |
| F-111 Plugin: ai_summary | `postproc::summary` complete | F-031's twin — same missing endpoint. `real_summary_api_round_trip`, same gate. |
| F-042 Model sizes & custom models | Complete + unit tested | Standing design decision since M2: no contract test drives an *arbitrary* custom URL or third-party HF repo because no stable real target exists for the suite to depend on. Unchanged by design. |

---

##### 6. Planned

There is **no roadmap, backlog, or issue tracker in the repo**, and that is deliberate: `docs/design/IMPLEMENTATION_PLAN.md:52` carries an explicit exit note — *"M8 shipped, 2026-07-27 — this is the last milestone."* M1–M8 are all closed with per-milestone benchmark and GUI-manual records. What exists instead is **named, scoped debt** that the project documented rather than fixed. These are the de-facto backlog:

1. **Packaging / distribution** — the entire story is missing. No `.app` bundle, no notarization, no installer, no Homebrew formula, no release CI job. ADR-024 (`DECISIONS.md:184`) names the hard blocker: bundling the OpenBLAS dylib or switching to `@rpath`-relative linking is *"packaging debt, not solved by this ADR."* Q-7 (`docs/OPEN_QUESTIONS.md:36`) deferred self-update to *"version check + notify … no self-install in v1."*
2. **Close the F-006 loop** — publish a `version_info.json` payload (or add an env override) so the update-available dialog is reachable and its row can flip.
3. **CLI post-processing parity gap** — deviations ledger item 16, M8 design spec (`docs/superpowers/specs/2026-07-26-m8-heavy-ml-closing-design.md:660`): `RunnerDeps.reducer`/`lang_detector` are `None` on the CLI, so a CLI user who enables `noise_reduction` or `language_detection` in `settings.toml` gets silence — no effect, no error. Explicitly *"a real CLI/GUI parity gap … closing it is scoped debt."*
4. **Second-Ctrl-C orphan** — `DECISIONS.md:150`: `run.rs::install_ctrlc` calls `std::process::exit(130)` on a second SIGINT, bypassing Drop and the cancel-and-wait path; because demucs/yt-dlp are detached into their own process groups by then, nothing signals them and they survive as orphans. Named as *"a real, currently-unfixed gap"*; the fix (a pgid registry the exit handler signals) is scoped.
5. **Non-macOS support** — `crates/humm-core/src/paths.rs` has real `linux`/`windows` branches, but nothing else does: CI is `macos-latest` only, Core ML gating is `cfg!(all(target_os = "macos", target_arch = "aarch64"))`, and diarization requires Homebrew OpenBLAS. Never claimed as supported; never tested either.
6. **F-031/F-111 verification** — point the env-gated tests at any real OpenAI-compatible chat endpoint and both rows close. Zero code work.

---

##### 7. Risks and debt, ranked

1. **The app will not launch on a machine without Homebrew OpenBLAS at the exact Homebrew prefix.** `speakrs`'s `openblas-system` feature links `/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib` by **absolute path** — verified by `otool -L` and recorded in ADR-024 (`DECISIONS.md:184`). The dynamic linker fails at process start, *before* any of humm's own graceful-absence handling runs. This makes the product currently undistributable to anyone who is not a developer with the right `brew install openblas libomp`. Highest-severity item, and honestly documented rather than hidden.
2. **No distribution artifact at all.** 106k lines of verified application with no way to hand it to a user. Combined with #1, "shipping humm" is a genuine unstarted project, not a packaging afternoon.
3. **All verification evidence is single-machine and partly irreproducible.** Every benchmark, differential, and GUI-manual run is from one Apple M4 / macOS 26.5.2. 33 files (`docs/`, `crates/*/tests/`, `validation/`) reference absolute paths under `/Users/jawad/RustroverProjects/buzz` — the Buzz checkout is the fixture source for differentials, `HUMM_BUZZ_TESTDATA` (24 references), and `crates/humm-whisper/tests/contract.rs`. A new engineer on a clean machine cannot reproduce the differential or bench evidence, and some contract tests will skip. *Change-cost / build-and-run-reproducibility lens: the unit and snapshot suites are portable; the parity evidence is not.*
4. **Contract tests never run in CI, by design.** `.github/workflows/ci.yml` runs only fmt/clippy/`cargo test --workspace`. All 70 `#[ignore]`d tests — every real-external-binary assertion for ffmpeg, yt-dlp, demucs, whisper-rs, DeepFilterNet, diarization, keyring — are local-only, and several are double-gated behind env flags (`HUMM_DEMUCS_CONTRACT`, `HUMM_DFN_CONTRACT`, `HUMM_DIARIZE_CONTRACT`, `HUMM_LLM_TEST_*`). *Test-and-verification-surface lens:* the boundary layer that Standing Policy rule 5 makes solely responsible for validating an external upgrade is exactly the layer CI does not exercise. An ffmpeg or `ort` bump would pass CI green and break at runtime.
5. **No toolchain pin.** Edition 2024 + resolver 3 needs Rust ≥ 1.85, but there is no `rust-toolchain.toml` and no `rust-version` field; CI floats on `stable`. A future stable release that changes a lint can turn `-D warnings` red with no source change. Cheap to fix, currently unfixed.
6. **Pre-release and git dependencies in the dependency graph.** `ort =2.0.0-rc.12` (a release candidate, exact-pinned only because the M8 final review noticed `speakrs`'s caret requirement wasn't actually enforcing the ADR's claim — commit `8417fc5`) and `deep_filter` pinned to a **git rev** rather than a published crate. Both are load-bearing for M8 features. `ort` additionally fetches prebuilt ONNX Runtime binaries from `cdn.pyke.io` at build time, so builds are not network-hermetic. Standing risk register (`IMPLEMENTATION_PLAN.md:60`) flagged the `ort` rc at design time; it is still an rc.
7. **Licensing is unsettled.** `Cargo.toml:15` declares `license = "MIT"` but the repo contains **no `LICENSE` file** and no attribution or NOTICE for Buzz, from which golden test fixtures were ported verbatim (`docs/design/ARCHITECTURE.md:246` — *"snapshot-tested against Buzz's golden strings … ported verbatim"*). Third-party licenses for `deep_filter`, `speakrs`, the embedded DeepFilterNet3 model weights, and the bundled CJK font are not recorded anywhere. Needs a legal pass before any distribution. *(Reported as a gap; I did not attempt to determine the upstream licenses.)*
8. **No `README.md`.** Onboarding depends entirely on `CLAUDE.md` (written for AI coding agents) plus 12.9k lines of `docs/`. Excellent for depth, poor for first contact: there is no single page telling a new engineer what the app is, how to build it, or how to run it.
9. **Residual GUI defects and unverifiable surface.** `docs/gui-manual/M8.md` §8/§9 names three found defects (two fixed and re-verified in §12; the third, `⌘,` not opening Preferences, was root-caused in commit `d44a82e` to a system Input Sources hotkey capturing it pre-app — resolved-external, not a humm defect) plus a nine-item "still carried" list of things never exercised windowed: native dialog *contents*, credential entry + Test, real-microphone sessions with human speech, all audibility judgments, inline viewer pointer flows, header-context-menu column visibility (carried since M3), and task %/ETA on a task slow enough to observe. **These are player/user-facing observations only — UI/UX adjudication belongs to the Game & Experience Designer ([DIG-7](DIG-7.md)), not to me.**
10. **Corrupt-input error text leaks internals to the UI.** `docs/gui-manual/M8.md` §8.4: the Status column and the stored row get ffmpeg's raw stderr including a pointer value and a "Last message repeated 73 times" counter. Cosmetic, noted, unfixed.
11. **Bus factor / dormancy.** 244 commits compressed into 9 days by what the commit trail and `CLAUDE.md` show to be a single AI-assisted author, then two months of silence. The documentation is the mitigation and it is unusually strong — but nobody else has ever built, run, or modified this codebase.

###### Not risks — verified clean

- **No committed secrets.** Every `sk-…`/`phc_…` literal in `crates/` is a deliberate test placeholder, and the ones in `humm-core/src/secrets.rs:257` and `openai/client.rs:256` are literally named `"[REDACTED:openai_key]"` — fixtures for tests that assert redaction. The PostHog key is build-time-injected via `option_env!`; absent key compiles to a no-op (`crates/humm-gui/src/services.rs:856`). `.claude/settings.local.json` is gitignored. Nothing to rotate.
- **`unsafe` discipline holds.** `#![forbid(unsafe_code)]` in core and 5 of 7 other crates; the 7 `unsafe` sites are all in `crates/humm-whisper/src/engine.rs` (C callback trampolines, documented) with one in a GUI test helper. Exactly as `ARCHITECTURE.md:30` specifies.
- **`unwrap`/`expect` discipline holds.** 86 occurrences in all production code across 106k lines (excluding `tests/`, `benches/`, and inline `#[cfg(test)]` modules).
- **Separation of concerns is real, not aspirational.** The `binaries → adapters → core` rule is enforced by the crate graph itself; `humm-core` cannot name ffmpeg, SQLite, cpal, or `ort` even by accident. This is the single best structural property of the codebase, and it is why swapping the Demucs strategy (ONNX → subprocess) and the diarization crate late in M8 cost one adapter each instead of a refactor.

---

##### 8. Open questions for the founder

1. **Is humm meant to ship to users, or was it an engineering exercise?** If it ships, packaging + the OpenBLAS linkage (risks #1, #2) is the next project and needs a person; if not, the repo is complete as an artifact and should be labeled that way.
2. **macOS-only, or is Linux/Windows in scope?** `paths.rs` has the branches, nothing else does. A yes means a real cross-platform port of Core ML gating, OpenBLAS, and the CI matrix.
3. **Should I add a `LICENSE` file, and does Buzz attribution need a legal review?** Cargo declares MIT, the repo has no license text, and Buzz's golden fixtures were ported verbatim. A short answer here unblocks any distribution.
4. **Do you have (or can you stand up) an OpenAI-compatible chat endpoint we can point `HUMM_LLM_TEST_BASE` at?** Yes → F-031 and F-111 flip to `verified` for the cost of one test run, taking the matrix to 48/50.
5. **Do you want the three named debt items fixed** (CLI post-proc parity, second-Ctrl-C orphan, update-check env override)? All three are small, all three are already specified in the docs; they are the only known functional gaps in the product.
6. **Should CI gain a contract-test job** (a runner with ffmpeg/yt-dlp/demucs, or a nightly), given that contract tests are the designed-in mechanism for validating external upgrades and currently never run automatically?
7. **Is the `/Users/jawad/RustroverProjects/buzz` checkout preserved?** Several tests and every differential/benchmark record depend on it. If it is gone, that evidence is frozen and unrepeatable.
8. **Do you want a `README.md`?** I can write one from the existing docs without touching anything else.

---

##### 9. Specialist expertise this codebase needs

Ranked, with evidence, for [DIG-4](DIG-4.md)'s roll-up:

1. **macOS platform / release engineering — the clear #1 need.** Evidence: ADR-024's absolute-path OpenBLAS linkage means the GUI *cannot launch* off the dev machine (`docs/design/DECISIONS.md:184`, explicitly deferred as "packaging debt"); there is no `.app` bundle, no notarization, no signing, no installer, no release job in `.github/workflows/ci.yml`; the M7/M8 GUI-manual runs record an *unsigned dev binary* blocking keychain reads (`docs/gui-manual/M8.md` §9). This is the single thing standing between a finished application and a shippable one.
2. **Build / CI / test infrastructure engineer.** Evidence: 70 `#[ignore]`d contract tests that CI never runs, and which Standing Policy rule 5 designates as the *sole* validator of an external upgrade; no toolchain pin; a git-rev dependency and an `ort` release candidate that fetches binaries from `cdn.pyke.io` at build time; 33 files hard-coding `/Users/jawad/...` fixture paths. Someone who makes the verification suite portable and automatic.
3. **Rust systems engineer (audio/ML boundaries) for maintenance.** Evidence: the load-bearing FFI and real-time surfaces — `crates/humm-whisper/src/engine.rs` (the only `unsafe`, C trampolines), the `rtrb`/cpal RT path in `humm-audio`, the packed-atomic A/V master clock (ADR-019), `ort`/`speakrs` in `humm-diarize`. Bus factor is currently 1 and the codebase is dormant; nobody but the original author has ever built it.
4. **Not needed:** no backend/service engineer (there is no server — all HTTP is outbound to third-party APIs), and no game-engine expertise (this is a desktop utility, unrelated to Digikal's game work).

**Cross-cutting note for [DIG-4](DIG-4.md):** humm is the strongest engineering-process artifact in the portfolio scanned so far — the parity matrix, ADR log, per-milestone benchmark records, and honest deviations ledgers are a template worth reusing. It is also, in stack terms, an outlier: a Rust desktop/ML application inside a game-development studio. Whether it is a product, a tooling investment, or a finished experiment is a founder call (open question #1), and it materially changes whether hires #1 and #2 above are worth making.


## Comments (1)

<a id="comment-38b745ed-6df9-449b-9d1c-dc0193070e2e"></a>

### 2026-09-25T14:00:04.515Z — TechLead

Comment id `38b745ed-6df9-449b-9d1c-dc0193070e2e`; written by run `f191bad4-a625-4caa-bb87-d79ca092c80e`.

##### Scan complete — report posted

Report: [DIG-14 → `report`](DIG-14.md#document-report)

**State verdict: dormant — completed and self-declared done.** 244 commits over 9 days (2026-07-20 → 2026-07-28), then silence. `docs/design/IMPLEMENTATION_PLAN.md:52` carries an explicit exit note: *"M8 shipped — this is the last milestone."* Single branch, no WIP, zero `TODO`/`FIXME`/`todo!()` in 106k lines of Rust.

**What it is:** a macOS desktop + CLI transcription app in Rust — a full reimplementation of Buzz (Python/PyQt6 Whisper GUI). 9-crate workspace: whisper.cpp via FFI (Metal + Core ML), ffmpeg/yt-dlp/demucs as subprocesses, SQLite, egui GUI, plus Demucs speech isolation, DeepFilterNet3 denoise, `ort` speaker diarization, LLM translation. 1976 tests. `docs/analysis/PARITY_MATRIX.md` final state: **46 of 50 rows `verified`, 4 named blocker-documented exceptions** (3 are environment blocks with committed runnable closers, 1 a standing design decision) — no unfinished code, only blocked verification.

Benchmarked against a from-source Buzz on identical model weights: **6.19× faster startup, 5.50× lighter idle memory, 1.47–1.83× faster end-to-end transcription, peak RSS a tie** (`docs/benchmarks/MISSION.md` — reported as five readings rather than one flattering headline).

**Top risks (full ranking in the report):**

1. **It cannot launch off the dev machine.** `speakrs`'s `openblas-system` links `/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib` by absolute path; the dynamic linker fails at process start, before any graceful-absence handling. ADR-024 (`docs/design/DECISIONS.md:184`) names this as unfixed packaging debt.
2. **No distribution artifact at all** — no bundle, signing, notarization, installer, or release job in `.github/workflows/ci.yml`.
3. **Contract tests never run in CI** — all 70 `#[ignore]`d boundary tests are local-only, yet Standing Policy rule 5 makes them the *sole* validator of an ffmpeg/`ort`/whisper.cpp upgrade. An external bump passes CI green and breaks at runtime.
4. **Licensing unsettled** — `Cargo.toml:15` declares MIT, but there is no `LICENSE` file and no Buzz attribution, despite golden test fixtures ported verbatim. Needs a legal pass before any distribution.
5. **Bus factor 1**, and evidence is single-machine: 33 files hard-code `/Users/jawad/RustroverProjects/buzz` fixture paths.

**Verified clean:** no committed secrets (every `sk-…` literal is a redaction-test placeholder; the PostHog key is build-time-injected and compiles to a no-op when absent). `unsafe` confined to 7 documented FFI trampolines in `humm-whisper`. 86 `unwrap`/`expect` in 106k lines of production code.

**Specialist expertise needed** (for [DIG-4](DIG-4.md)'s roll-up):

1. **macOS platform / release engineering** — clear #1. Evidence: risks 1 and 2 above, plus unsigned-dev-binary keychain blocks recorded in `docs/gui-manual/M8.md` §9. This is all that stands between a finished app and a shippable one.
2. **Build / CI / test infrastructure** — evidence: risk 3, no toolchain pin (edition 2024 with no `rust-version`), a git-rev dependency (`deep_filter`) and an `ort` release candidate that fetches binaries at build time.
3. **Rust systems engineer (audio/ML boundaries)** for maintenance — the FFI trampolines, the `rtrb`/cpal real-time path, and the packed-atomic A/V master clock (ADR-019) are load-bearing and have exactly one person who has ever built them.
4. **Not needed:** no backend engineer (there is no server) and no game-engine expertise.

**Cross-cutting note:** humm is the strongest engineering-process artifact scanned so far — parity matrix, ADR log, per-milestone benchmarks, honest deviations ledgers. It is also a stack outlier in a game studio. Whether it is a product, a tooling investment, or a finished experiment is a founder call (open question #1 in the report) and it decides whether hires 1 and 2 are worth making.

8 open questions for the founder are at the end of the report; #4 is free money — pointing `HUMM_LLM_TEST_BASE` at any OpenAI-compatible chat endpoint flips two rows and takes the matrix to 48/50 with zero code work.

Read-only throughout: cloned checkout only, nothing built, run, or modified. Marking done — [DIG-4](DIG-4.md) is blocked on this.


## Agent runs (2)

Every Paperclip heartbeat run recorded against this issue. A run's results are the comments, documents, attachments and commits it wrote (comment headers above name the run). Raw run transcripts are not archived, except the runs listed with a transcript copy below; see [inventory.md](../inventory.md#exclusions).

| Run | Agent | Status | Started | Finished | Log bytes | Transcript copy |
|---|---|---|---|---|---|---|
| `b4a08a36-da99-4ac3-bfa6-7ca007337339` | TechLead | succeeded | 2026-09-25T13:50:17.311Z | 2026-09-25T13:53:09.618Z | 161784 | — |
| `f191bad4-a625-4caa-bb87-d79ca092c80e` | TechLead | succeeded | 2026-09-25T13:52:12.743Z | 2026-09-25T14:00:18.660Z | 158025 | — |
