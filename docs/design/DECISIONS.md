# humm — Architecture Decision Records

Format: context → options → decision → rationale → validation. Versions checked against crates.io 2026-07-20. Standing Policy defaults each get an ADR with their validation plan/result.

---

## ADR-001 — Cargo workspace with adapter crates per non-Rust boundary
**Options:** single crate with modules; workspace (core + adapters + thin binaries).
**Decision:** workspace as laid out in `ARCHITECTURE.md` §1.
**Rationale:** Standing Policy isolation rules; independent compile units (whisper.cpp C build isolated); per-adapter contract-test suites make external upgrades verifiable alone. Pure-Rust services stay in core — adapter crates are for non-Rust boundaries only.

## ADR-002 — Whisper inference via `whisper-rs` (0.16.0), validated against `candle`
**Options:** whisper-rs (FFI to whisper.cpp); candle (pure-Rust HF); burn; onnxruntime whisper.
**Decision:** `whisper-rs` with `metal` + `coreml` features on Apple Silicon (pre-approved default).
**Rationale:** whisper.cpp is the performance reference for local Whisper on Apple hardware (Metal + CoreML-encoder); ggml model ecosystem gives Buzz-model compatibility (same files Buzz downloads, F-042/043); bundled/vendored C pinned by crate version (Policy rule 4).
**Verified during design:** whisper-rs 0.16.0 exposes the full VAD API (`set_vad_model_path`, `enable_vad`, `WhisperVadParams`, `whisper_vad.rs`) — F-045 Silero VAD parity needs no extra dependency. Also exposes abort callback (cooperative cancel) and segment callbacks (streaming sink).
**Validation plan (M1, required):** benchmark vs candle's whisper example on the same ggml/safetensors model (small + large-v3-turbo) over `buzz/testdata/audio-long.mp3`: wall-clock, peak RSS, WER-proxy (diff vs reference transcript). Numbers recorded here. Abort if candle wins decisively (not expected on Metal).

## ADR-003 — Media decode/probe via ffmpeg/ffprobe subprocesses
**Options:** libav* bindings (ffmpeg-next); gstreamer; symphonia (pure Rust); ffmpeg CLI subprocess.
**Decision:** subprocess (pre-approved): decode `ffmpeg -i <in> -f f32le -ac 1 -ar 16000 -` → stdout pipe; probe `ffprobe -print_format json -show_format -show_streams`; video `-f rawvideo -pix_fmt rgba` stream.
**Rationale:** CLI surface stable for decades vs ABI churn in libav; one-shot batch decode makes process overhead (~10 ms) immaterial; symphonia lacks full container coverage (wmv/avi/ogm).
**Note:** Buzz pipes s16le then converts to f32 (`whisper_audio.py:19`); humm pipes f32le directly — identical result, one fewer conversion.
**Validation:** contract tests decode golden files (each supported extension) asserting sample-count/duration/PSNR vs known PCM; startup probe caches version+capabilities; decode-throughput criterion bench. Deviate to bindings only if the subprocess path benchmarks as a real bottleneck (not expected).

## ADR-004 — Persistence via `rusqlite` (0.40, `bundled`)
**Options:** rusqlite; sqlx; sea-orm; sled/redb; flat files.
**Decision:** rusqlite bundled (pre-approved), WAL mode, humm-owned schema = Buzz-superset + `options_json` + `kind` (live sessions).
**Rationale:** synchronous fits the single-writer queue; bundled SQLite pinned via Cargo (Policy rule 4); Buzz-column compatibility enables the approved one-time `Buzz.sqlite` import (Q-6). No ORM — repository pattern with hand-written SQL, unit-tested.
**Validation:** repository unit tests incl. migration tests; import test against a fixture Buzz.sqlite generated from Buzz's own schema.sql.

## ADR-005 — Audio capture via `cpal` (0.18) + resampling via `rubato` (4.0)
**Options:** cpal; PortAudio bindings; platform APIs directly.
**Decision:** cpal (pre-approved, the unavoidable OS boundary) with `rtrb` lock-free SPSC ring buffer; request 16 kHz mono, fall back to device default + rubato resample inside the adapter (parity with Buzz's fallback, `recording_transcriber.py:372`).
**Rationale:** maintained pure-Rust OS-API wrapper; no allocation/locking in the RT callback; rubato is the de-facto SIMD resampler.
**Validation:** contract test with a synthesized loopback (and `mock` capture impl for CI); resample quality test (sine sweep RMS error); chunker unit tests port Buzz's silence-cut behavior.

## ADR-006 — GUI framework: **egui/eframe 0.35**
**Needs from `USER_JOURNEYS.md`:** 5+ real OS windows (main/viewer/recording/preferences/presentation), virtualized editable segment table (100k rows), custom painting (audio meter, presentation view), streamed video frames as textures, synced playback UI, native file dialogs, drag-drop, 21 rebindable shortcuts, i18n, background-task progress without jank, accessibility.
**Options considered:**
- **iced 0.14** — Elm model is clean for this app-shape; but multi-window is young, table virtualization and custom-widget ergonomics lag, accessibility incomplete, slower release cadence.
- **Slint 1.17** — polished, good a11y, royalty-free desktop license; but DSL split poorly fits our churn-heavy custom surfaces (meter, table-editor, video), and the license story (GPL/royalty-free/commercial triad) is a needless liability for a personal OSS-adjacent app.
- **gpui** — Zed-proven performance; but API-unstable, sparsely documented, effectively in-tree for Zed; too risky as a foundation others must build on for years.
- **egui/eframe 0.35** — immediate mode; deferred viewports = real OS windows; `egui_extras` virtualized tables; custom painting and per-frame texture upload are first-class (exactly our meter/video needs); AccessKit integrated; `rfd` native dialogs; drag-drop supported; huge ecosystem/velocity.
**Decision:** **egui/eframe**.
**Rationale (criteria-only — requirement fit, then performance):**
- *Requirement coverage:* egui is the only candidate strong on **all four hard requirements simultaneously** — mature real-OS-window multi-viewport support, virtualized tables proven at 100k+ rows (`egui_extras`), first-class custom painting (audio meter at capture rate), and per-frame texture streaming (video). iced is weakest on virtualized tables and has the youngest multi-window; Slint's declarative DSL puts our three custom-drawn surfaces (meter, segment-table editor, video canvas) behind its canvas/image-buffer escape hatches — more boundary-crossing on exactly the surfaces we redraw most; gpui covers the requirements but its API instability, sparse documentation, and minimal accessibility story put *functional parity delivery* (value #1) at risk.
- *Performance (value #2):* on this app's GUI hot paths — segment-table scrolling, ~60 Hz meter repaint while recording, video frame upload — egui and gpui are the two strongest candidates; all four clear 60 fps for forms-and-tables workloads, and GUI rendering is not humm's bottleneck (inference/decode dominate). Idle cost, the classic immediate-mode objection, is eliminated by repaint-on-event (idle ≈ 0 CPU). gpui's raw-throughput edge does not convert into user-visible speed here, and it charges for that edge in parity risk.
- *Remaining criteria:* accessibility — egui (AccessKit integrated) and Slint lead; iced incomplete; gpui minimal. Native file dialogs — equal (`rfd`). Platform look-and-feel — Slint leads, but the parity baseline (Buzz on Qt/Fusion) is itself non-native, so this criterion cannot outweigh the requirement gaps above.
**Rejected:** gpui (perf-per-risk fails value #1), Slint (custom-surface friction; look-and-feel edge irrelevant to parity), iced (table/multi-window/a11y gaps).
**Validation:** M3 exit criteria include a 100k-segment table scroll benchmark (≥ 60 fps sustained) and a multi-window smoke test on macOS; if egui misses these targets the ADR reopens with the measured numbers.

## ADR-007 — CLI: `clap` v4 (derive), genuinely headless — logged deviation
**Options:** clap; lexopt/pico-args; reproduce Buzz's Qt parser semantics.
**Decision:** clap v4 derive. Grammar: flag *spelling* parity with Buzz's `add` (every flag in `EXTERNAL_SURFACE.md` §2 identical). Deviations (this ADR): (1) **headless** — no window, no display server needed (Buzz drives a hidden GUI); (2) real exit codes: `0` all succeeded, `1` usage/config error, `2` ≥1 task failed; (3) `--json` event-per-line output; (4) stderr progress bars (TTY-only). `--hide-gui` accepted as a no-op with a deprecation note (parity of acceptance, not behavior).
**Rationale:** headless CLI is the mission's explicit outcome-parity case; clap gives `--help` quality and env-var integration per project conventions.
**Validation:** CLI integration tests (invoke binary, assert files/exit codes/JSON schema); differential test vs Buzz `add` on the same file for produced artifacts.

## ADR-008 — Backend consolidation (pre-approved deviation, recorded)
Buzz exposes five backends (`FEATURE_INVENTORY.md` F-040/F-041). humm ships **two**: local whisper-rs + OpenAI-compatible API.
- **CTranslate2/faster-whisper: not ported** (Standing Policy directive).
- openai-whisper (.pt) and in-process HF Transformers: superseded — same user outcome (local Whisper inference) served faster by whisper-rs; ggml model set covers all Buzz sizes incl. lumii + custom (F-042).
- Custom HF Whisper-family repos: supported by fetching/converting ggml equivalents (approved Q-2). **Non-Whisper HF models (MMS/wav2vec2): out of scope** (approved Q-2); the MMS ISO-639-3 language UI is therefore dropped.
- Live whisper.cpp: in-process via whisper-rs streaming — **no `whisper-server` subprocess** (Buzz's localhost:3003 hack disappears; hallucination guards become native params).

## ADR-009 — Telemetry: PostHog via raw capture API (user-approved design)
Decision recorded from `../OPEN_QUESTIONS.md` Q-1 (approved 2026-07-20): PostHog `POST /capture/`, own key via build-time `HUMM_POSTHOG_KEY` (absent ⇒ noop), `app_launched` parity event, persisted anonymous uuid, fire-and-forget post-UI task, `HUMM_DISABLE_TELEMETRY` env + Preferences toggle, debug builds noop. `Telemetry` trait in core; zero new dependencies (reqwest already present).

## ADR-010 — Plugin system: built-ins become native; extensibility mechanism replaced
(Approved Q-4.) Buzz's Python-in-process plugin system (pip installs, 4 hooks) is not portable and is a security liability Buzz itself warns about. humm: the six built-ins are first-class features (F-111..F-116 rows retained); the *mechanism* for third parties is deferred — if demanded, a subprocess hook contract (JSON on stdio at the same 4 lifecycle points) is the sketched design. No dynamic code loading in-process, ever.

## ADR-011 — Deferred ML strategies (approved Q-3/Q-5)
- **Speech extraction (F-027):** `SpeechIsolator` trait now; M8 impl via `ort` + exported htdemucs ONNX if quality/perf validate, else optional `demucs` subprocess with graceful absence. Note: `ort` is still 2.0-rc (no stable on crates.io) — acceptable for a late milestone; re-evaluate at M8.
- **Speaker identification (F-087):** `Diarizer` trait now; M8 impl via `ort` (segmentation+embedding ONNX models + clustering in Rust). Different models than Buzz's NeMo stack — outcome-level parity (speakers detected, named, merged), verified by diff on speaker-count/boundary sanity, not identical labels.
- **Noise reduction (F-112):** DeepFilterNet's reference implementation is itself Rust (`df` crates) — native integration expected cheap; validate at M8.

## ADR-012 — Settings: TOML file (deviation from QSettings)
**Decision:** serde-typed TOML at the platform config dir; atomic writes; no QSettings migration (approved Q-6); secrets in OS keyring via `keyring` (4.x) behind `SecretStore`.
**Rationale:** QSettings is a Qt-ism (registry/plist/ini per-OS); a typed struct with in-code defaults is the Rust shape, diffable and user-editable. All Buzz setting *semantics* (keys catalogued in `EXTERNAL_SURFACE.md` §3.6) map onto the struct.

## ADR-013 — Concurrency: threads + channels for pipeline, one tokio runtime for I/O
As `ARCHITECTURE.md` §3. Justification per subsystem recorded there; the deliberate choice is *not* to make core all-async: inference is blocking-by-nature, and an async facade would infect every consumer for zero throughput gain.

## ADR-014 — i18n: Fluent
**Options:** gettext-rs (could consume Buzz's .po); fluent; rust-i18n.
**Decision:** `fluent` with English baseline; stable string keys from M3; locale ports are a late-milestone mechanical task (Buzz's 15 locales are the parity target, F-008).
**Rationale:** gettext-rs binds C gettext (an avoidable non-Rust boundary for a solved-in-Rust problem); Buzz's .po strings won't match humm's UI copy anyway (UX_SPEC redesigns flows), so .po reuse is illusory.

## ADR-015 — Update check: notify-only (approved Q-7)
GitHub Releases `releases/latest` JSON compare + toolbar notice linking to the release page. No installer download/launch in v1. Disable env: `HUMM_DISABLE_UPDATE_CHECK`. Same ≤7-day cadence.

## ADR-016 — URL import via `yt-dlp` subprocess (pre-approved conditional, confirmed)
Phase 0 confirmed Buzz uses yt-dlp (F-022). humm: `MediaFetcher` adapter invoking `yt-dlp` with `--print`-based machine-readable progress, `bestaudio/best`, cookie file passthrough (`HUMM_DOWNLOAD_COOKIEFILE`), startup probe, graceful absence (URL import disabled with actionable message when binary missing).
