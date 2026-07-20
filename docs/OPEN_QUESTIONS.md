# humm — Open Questions

Questions that cannot be answered from the Buzz source alone, or that require a product decision from the user. Never invent behavior — items live here until resolved. Resolution recorded inline + reflected into the affected docs/ADRs.

## Q-1 — Telemetry parity
Buzz sends a PostHog `app_launched` event with a hardcoded project key and persisted per-user UUID (`buzz/widgets/application.py:80-93`). Porting Buzz's key is wrong (it's their project); shipping our own telemetry is a product decision.
**User decision (2026-07-20): humm WILL include telemetry.** Proposed design (awaiting user approval, to be formalized as an ADR in Phase 1 `DECISIONS.md`):
- Backend: **PostHog** (own project key), via its stable `POST /capture/` HTTP API using the app's existing HTTP client — no SDK dependency.
- Shape (parity with Buzz): `app_launched` event; properties = app version, OS, OS version, arch, locale; `distinct_id` = anonymous uuid4 persisted in settings on first run.
- Architecture: `Telemetry` trait in core, `PostHogTelemetry` + `NoopTelemetry` impls; fire-and-forget background task after UI shows; short timeout, no retries, silent drop offline; debug builds default to noop.
- Key injection: build-time env (`HUMM_POSTHOG_KEY`); absent key compiles to noop — no hardcoded personal key in the repo (fixes Q-8 for telemetry).
- Opt-out: `HUMM_DISABLE_TELEMETRY` env (parity) **plus** a visible Preferences toggle (improvement over Buzz's env-only opt-out).
— *Status: RESOLVED — design approved by user 2026-07-20*

## Q-2 — Hugging Face backend scope (incl. MMS)
Buzz's HF backend runs arbitrary Transformers ASR models — Whisper-family AND non-Whisper (MMS/wav2vec2 with ISO-639-3 languages, `buzz/transformers_whisper.py`). whisper-rs covers Whisper-family only. Outcome-level parity ("transcribe with a custom HF model") for non-Whisper models would need candle/ort or a deliberate scope cut.
**Proposed default:** support custom *Whisper-family* HF models by converting/fetching ggml equivalents; declare non-Whisper (MMS) out of scope in the ADR unless the user needs it. — *Status: RESOLVED — proposed default approved by user 2026-07-20*

## Q-3 — Speaker identification strategy
Buzz's diarization = NeMo MSDD + ctc-forced-aligner + punctuation model (three vendored submodules + torch; `speaker_identification_widget.py`). No mature pure-Rust equivalent. Options: (a) ONNX pipeline via `ort` (e.g. pyannote/silero embeddings + clustering — different models than Buzz, comparable outcome), (b) external subprocess tool, (c) defer to a later milestone, (d) drop.
**Proposed default:** (a) with (c) sequencing — design the `Diarizer` trait boundary now, implement via `ort` in a late milestone. — *Status: RESOLVED — proposed default approved by user 2026-07-20*

## Q-4 — Plugin system parity
Buzz plugins are Python-in-process with pip-dependency installation and 4 lifecycle hooks (`buzz/plugins/`). In-process Python is architecturally alien to a Rust app. The six built-ins can be native humm features. True third-party extensibility could be: subprocess/JSON-RPC hooks, WASM, or dropped.
**Proposed default:** implement the six built-ins natively (they're in the parity matrix individually); log an ADR that the *extensibility mechanism* is replaced (e.g. subprocess hook contract), pending user confirmation. — *Status: RESOLVED — proposed default approved by user 2026-07-20*

## Q-5 — Speech extraction (Demucs) strategy
Demucs is a PyTorch model (vendored `demucs_repo`). Rust options: ONNX export via `ort`, or ship a small helper (e.g. `demucs` CLI via `uvx`) as an optional subprocess with graceful absence.
**Proposed default:** `ort` with an exported htdemucs ONNX if quality/perf validate; else optional subprocess. Trait boundary (`SpeechIsolator`) either way. — *Status: RESOLVED — proposed default approved by user 2026-07-20*

## Q-6 — Buzz data migration
Should humm read the user's existing `Buzz.sqlite` (and QSettings/keyring) to migrate history/settings on first run? Schema is fully documented (EXTERNAL_SURFACE §3.5-3.7).
**Proposed default:** yes for transcription history (one-time import); no for QSettings (fresh defaults). — *Status: RESOLVED — proposed default approved by user 2026-07-20*

## Q-7 — In-app self-update
Buzz downloads and launches installers on Windows/macOS (F-006). humm's distribution story (cargo? notarized bundles? homebrew?) determines whether self-update is meaningful.
**Proposed default:** version check + notify with release link; no self-install in v1. — *Status: RESOLVED — proposed default approved by user 2026-07-20*

## Q-8 — Update/telemetry/upload endpoints in EXTERNAL_SURFACE are Buzz's
`version_info.json` URL, PostHog key, docs links are chidiwilliams/buzz property; humm needs its own or none. Follows Q-1/Q-7. — *Status: RESOLVED — humm uses its own PostHog key (Q-1) and check+notify update (Q-7); Buzz endpoints not reused*

## Q-9 — `local_whisper_cpp_server_transcriber.py` is dead code
Marked "Currently unused, but kept for future reference" in source. Treating as non-feature (not in parity matrix). Flag only if user disagrees. — *Status: resolved-by-default (excluded)*

## Q-10 — Live-session persistence
Buzz does not save live-recording transcripts to the DB (J-5 ⚠). Parity says "don't have to"; UX_SPEC will likely add saving as an improvement. Confirm this addition is wanted. — *Status: RESOLVED — improvement approved by user 2026-07-20 (design in Phase 1 UX_SPEC)*
