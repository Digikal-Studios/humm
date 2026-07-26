# M8 — Heavy ML + mission closing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship F-027 speech isolation (demucs subprocess), F-087 speaker identification (ort diarization + J-7 two-step window), F-112 DeepFilterNet noise reduction (native tract), F-114 enhanced language detection (native whisper-rs), the `before_transcription` queue seam, the closing sweep (CJK font, 11 locales, store/live hardening), then the FINAL parity sweep and the mission-closing Buzz-vs-humm benchmark report.

**Architecture:** Core defines four new boundary traits (`SpeechIsolator`, `NoiseReducer`, `LanguageDetector`, `Diarizer`); adapters: `DemucsIsolator` in humm-media (subprocess), new crate **humm-dfn** (`deep_filter` git-pin, tract, pure Rust), new crate **humm-diarize** (ort-backed pipeline crate per the Task 2 spike), `WhisperLangDetector` in humm-whisper. `Runner::run_job` gains a fixed-order pre-transcription block (isolate → reduce → detect) after check_skip; the Speaker-ID window is a viewer-side deferred viewport that saves to a copied transcription row.

**Tech Stack:** `deep_filter` (git pin v0.5.6, features tract+default-model), speakrs 0.5 **or** pyannote-rs (exact pin, spike-decided; ort `=2.0.0-rc.12` transitively), demucs CLI (user-installed, graceful absence), Noto Sans SC (bundled OFL font). No tokio.

**The governing spec:** `docs/superpowers/specs/2026-07-26-m8-heavy-ml-closing-design.md` — every task's implementer MUST read it first (§0–§2 + the §§ their task names), plus the Buzz sources their task cites. Implement from Buzz source, not this plan's paraphrase, wherever the two could disagree.

## Global Constraints

1. Every commit passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` with `export CARGO_HOME="$PWD/.cargo-home"`. **This worktree's `.cargo-home` is unseeded**: before the first build, copy the main checkout's (`cp -R /Users/jawad/RustroverProjects/humm/.cargo-home .cargo-home`) and run one sandbox-lifted `cargo fetch` (new deps: deep_filter git pin, the diarize crate). The ort-dependent first build also fetches its runtime binaries — one sandbox-lifted `cargo build -p humm-diarize` is expected and documented (Task 9). Commit messages: `M8: <what> (F-###[, F-###])`.
2. Sandbox: GPU/Metal (SEGFAULT — never bench/e2e/windowed sandboxed), TCP binds, CoreAudio, FSEvents, and writes outside the worktree are blocked. TCP-bind-blocked baseline: **67 humm-core + 1 humm-cli**; Task 12 adds humm-gui's first (→ 67+1+1); any further growth states the new count in its commit message.
3. Network-dependent tests (real demucs, real diarization models, real chat) are `#[ignore]`/env-gated; local mocks and scripted doubles carry the sandboxed load.
4. Key hygiene rules stand (no secrets in settings/DB/logs/argv/`Serialize` types).
5. All user-visible strings via ftl with stable keys; **every en key added lands in de/es/zh-CN in the same commit** (per-locale tests enforce); after Tasks 13–15 the same rule covers all 15 catalogs (any later key addition updates all 15).
6. Never `std::env::set_var` in tests — thread values explicitly.
7. kittest cannot bind TCP or drive real dialogs/child viewports — scripted seams and `Harness::new_ui_state` fixtures.
8. New public core items need doc comments (`#![warn(missing_docs)]`).
9. Pre-transcription hooks are **off by default** (`extract_speech` per-task opt-in; F-112/F-114 settings default disabled) — the default transcription path stays diff-innocent (verified in Task 16).
10. `Terminal`/event exhaustive matches: `crates/humm-cli/src/run.rs` and `crates/humm-gui/src/viewmodel.rs` must gain arms for every new `TaskEventKind` variant in the same task that adds it.

## File structure (created/modified across the milestone)

```
crates/humm-core/src/
  boundary.rs             # +SpeechIsolator, +NoiseReducer, +LanguageDetector traits;
                          #  +TaskStore::update_language
  diarize.rs              # NEW: Diarizer trait, SpeakerTurn, DiarizeStage/Progress/Error,
                          #  assign_speakers, label_segments (pure mapping)
  domain.rs               # +TranscriptionOptions.extract_speech (#[serde(default)])
  queue.rs                # +RunnerDeps.{isolator,reducer,lang_detector},
                          #  +TaskEventKind::PhaseChanged{TaskPhase}, pre-transcription block,
                          #  temp-audio cleanup guard
  pipeline.rs             # run_file_job(+audio_input, +language_override); output naming
                          #  _speech strip + detected-language token
  settings.rs             # +NoiseReductionSettings, +LangDetectSettings (under postproc),
                          #  +FileTranscriberSettings.extract_speech
  postproc/mod.rs         # PostprocConfig +noise_reduction +language_detection
  postproc/langdetect.rs  # NEW: gating rule (None-or-English), model selection (largest
                          #  downloaded, exclude Custom/Lumii), tiny-download fallback
  models/registry.rs      # +diarization model constants (spike-pinned URLs/files)
  models/download.rs      # +ensure_diarization_models
  export.rs (or export/)  # output_file_path: _speech-suffix strip
crates/humm-store/src/store.rs   # lock() → lock_recover convention; update_language;
                                 #  insert writes real extract_speech
crates/humm-media/src/           # demucs.rs NEW: DemucsIsolator; decode_mono_at on FfmpegDecoder
crates/humm-whisper/src/         # langdetect.rs NEW: WhisperLangDetector
crates/humm-dfn/                 # NEW crate: DfNoiseReducer (deep_filter/tract)
crates/humm-diarize/             # NEW crate: OrtDiarizer (speakrs or pyannote-rs)
crates/humm-cli/src/{args,run}.rs # -e live; PhaseChanged arms; isolator wiring
crates/humm-gui/src/
  app.rs                  # FontDefinitions (CJK); speakers map + viewports; AppEvent reuse
  fonts.rs                # NEW: bundled-font FontDefinitions builder + glyph tests
  assets/fonts/           # NotoSansSC-Regular.ttf + OFL.txt
  services.rs             # capabilities.demucs, RunnerDeps wiring, DiarizeService impl
  viewmodel.rs            # Phase::Isolating + PhaseChanged arm
  views/options_sheet.rs  # extract-speech checkbox live
  views/prefs_postproc.rs # +noise-reduction + language-detection sections
  views/preferences.rs    # tab-strip wrap (font_size 24 overflow fix)
  views/speaker_id.rs     # NEW: J-7 two-step window
  speaker_id.rs           # NEW: SpeakerIdVm + worker + DiarizeService trait
  live_session.rs / recording.rs  # ChunkSkipped surfacing
  i18n.rs + assets/i18n/<11 new dirs>/humm.ftl
crates/humm-core/src/live/session.rs  # tolerate-3-consecutive + LiveEvent::ChunkSkipped
validation/m8-*/          # spike artifacts, 2-speaker fixture gen, outcome-diff scripts
docs/…                    # HUMM_SURFACE §1/§3, PARITY_MATRIX, DECISIONS (ADR results),
                          #  UX_SPEC, ARCHITECTURE §2, gui-manual/M8.md, benchmarks/M8.md
```

---

### Task 1: CJK font + Preferences tab-strip overflow (F-008 gate)

**Files:** Create `crates/humm-gui/src/fonts.rs`, `crates/humm-gui/assets/fonts/NotoSansSC-Regular.ttf` (+ `OFL.txt`); modify `crates/humm-gui/src/app.rs` (HummApp::new), `crates/humm-gui/src/main.rs` (error-app path if it renders localized text), `crates/humm-gui/src/views/preferences.rs`.
**Spec:** §7.1. **First because it is user-visible today** (zh_CN renders tofu).
**Produces:** `fonts::install(ctx: &egui::Context)` — builds `FontDefinitions::default()`, inserts `"noto-sc"` `FontData` (`include_bytes!`), appends it to the END of both `Proportional` and `Monospace` families, calls `ctx.set_fonts`. Called once from `HummApp::new`.

- [ ] Fetch the font: `NotoSansSC-Regular.ttf` (static, OFL) from the notofonts GitHub release (github.com is sandbox-allowed; use the official notofonts/noto-cjk release asset), commit with `OFL.txt` alongside. Record exact source URL + SHA256 in `fonts.rs` module doc.
- [ ] Failing tests (`fonts.rs` unit): `cjk_glyphs_resolve` — load the bundled bytes with `ab_glyph::FontRef`, assert `glyph_id(c).0 != 0` for sample chars from each CJK catalog need: `转` (zh_CN), `轉` (zh_TW), `語` + `ひ` + `カ` (ja), and `。？！`; `font_definitions_contain_noto_fallback` — `fonts::definitions()` has `"noto-sc"` last in both families.
- [ ] Implement `fonts.rs`; wire `HummApp::new`.
- [ ] Tab-strip fix: `views/preferences.rs` tab row becomes `ui.horizontal_wrapped` (all six tabs reachable at `ui.font_size = 24`); kittest: `all_six_tabs_clickable_at_font_size_24` (set font size in settings fixture, click each tab by label, assert active-tab state changes).
- [ ] Gates; commit `M8: bundle Noto Sans SC CJK fallback font + prefs tab-strip wrap (F-008, F-101)`.

### Task 2: Validation spikes — deep_filter, diarize crate, demucs, 2-speaker fixture

**Files:** Create `validation/m8-spikes/README.md` (evidence record), `validation/m8-spikes/gen_two_speaker_fixture.sh`, `crates/humm-core/tests/fixtures/diarize/two-speakers.wav` (committed if < ~2 MB, else regenerated by script); modify `docs/design/DECISIONS.md` (ADR-011 addendum results, ADR-024 spike results).
**Spec:** §1, §12. Spike scratch projects live under the session scratchpad, NOT the repo; only evidence + fixtures are committed.
**Produces (later tasks consume):** the **decided diarize crate + exact version + model URLs** (written into ADR-024 and consumed by Tasks 9–11); the **deep_filter pin verdict** (rev hash for Task 7, or the vendor fallback decision); `two-speakers.wav` — ffmpeg splice: 15 s `audio-long.mp3` + full `whisper-french.mp3` (~21 s) + 15 s `audio-long.mp3` offset 30 s, 16 kHz mono wav; splice timestamps recorded in the README (expected speaker boundaries ±2 s).

- [ ] Seed `.cargo-home` (Global Constraint 1) — copy + one lifted `cargo fetch`; verify `cargo test --workspace` baseline is green before any change (record counts).
- [ ] Spike A (deep_filter): scratch crate depending on `deep_filter = { git, rev = <v0.5.6 tag rev>, features = ["tract","default-model"] }`; build; denoise a noise-added fixture (`ffmpeg` white-noise mix of `whisper-french.mp3`); record: builds? runs? output sane (duration equal, audible content)? API entry points used (exact fn names for Task 7).
- [ ] Spike B (diarize): scratch crate for **speakrs** (primary): build (lifted for ort binaries); run on `two-speakers.wav`; criteria: builds with our toolchain; **explicit local-model-path API (no env mutation)**; ≥2 speakers with boundaries ±2 s of the splice points; runtime sane on CPU/CoreML. If any criterion fails → repeat for **pyannote-rs**; if both fail → record and adopt the in-house §12 fallback (re-plan Tasks 9–11 before proceeding — stop and surface).
- [ ] Spike C (demucs): sandbox-lifted `uvx demucs --two-stems=vocals -n htdemucs --mp3 -o <scratch> <fixture>`; record: installs? produces vocals? wall time; exact output layout (`<scratch>/htdemucs/<track>/vocals.mp3` — confirm `--filename` behavior for Task 5's rename logic).
- [ ] Generate + commit the fixture and `gen_two_speaker_fixture.sh`; write `validation/m8-spikes/README.md` (all evidence, decisions, versions); append spike results to ADR-011 addendum + ADR-024 in `DECISIONS.md`.
- [ ] Gates; commit `M8: validation spikes — diarize crate pick, deep_filter pin, demucs contract, 2-speaker fixture (F-027, F-087, F-112)`.

### Task 3: Vocabulary groundwork — options, settings, events, store

**Files:** Modify `crates/humm-core/src/domain.rs`, `crates/humm-core/src/settings.rs`, `crates/humm-core/src/queue.rs` (event enum only), `crates/humm-core/src/postproc/mod.rs`, `crates/humm-core/src/boundary.rs` (TaskStore), `crates/humm-store/src/store.rs`, `crates/humm-cli/src/run.rs`, `crates/humm-gui/src/viewmodel.rs`, `crates/humm-gui/src/services.rs` (GuiPostprocSource snapshot), `docs/HUMM_SURFACE.md` §3.
**Spec:** §2, §7.3, §8.
**Produces:**
`TranscriptionOptions.extract_speech: bool` (`#[serde(default)]`);
`FileTranscriberSettings.extract_speech: bool` (default false, persisted last choice);
`NoiseReductionSettings { enabled: bool /*false*/, keep_denoised_file: bool /*false*/ }` and `LangDetectSettings { enabled: bool /*false*/, download_tiny_if_missing: bool /*true*/ }` — new `PostprocSettings.noise_reduction` / `.language_detection` sections (manual `Default` for LangDetect per the defaults-true convention); mirrored onto `PostprocConfig` as plain copies;
`TaskEventKind::PhaseChanged { phase: TaskPhase }` + `pub enum TaskPhase { IsolatingSpeech }` (`#[non_exhaustive]` not used — exhaustive matches are the convention);
`TaskStore::update_language(&self, id: &TranscriptionId, language: &str) -> Result<(), StoreError>` (+ SqliteStore impl + every test double, compile-driven);
`SqliteStore` insert writes the row's `extract_speech` column from the real option (was hardcoded `0`, store.rs:1377);
`SqliteStore::lock` switches to the `lock_recover` convention (infallible, `unwrap_or_else(|p| p.into_inner())` — queue.rs:311-320's documented rationale), `StoreError::Query` poisoning mapping removed, call sites simplified.

- [ ] Failing tests: settings defaults + `pre_m8_settings_toml_parses_with_new_sections_defaulted` (M7-era TOML literal) + full `[postproc.noise_reduction]`/`[postproc.language_detection]` round-trip; domain `old_options_json_without_extract_speech_deserializes` (M7 JSON literal); store `update_language_persists_and_reads_back`, `insert_writes_extract_speech_column` (options with true → column 1), `store_survives_a_panicked_lock_holder` (poison via caught panic holding the guard, then a write succeeds); CLI `json_for` pin test gains `{"event":"phase","phase":"isolating_speech",…}`; human-mode renders `isolating speech` on the progress line; viewmodel `apply_phase_changed_sets_isolating_phase` (`Phase::Isolating` variant).
- [ ] Implement; `GuiPostprocSource::snapshot` copies the two new sections; HUMM_SURFACE §3 documents the additive `phase` event.
- [ ] Gates; commit `M8: options/settings/event/store vocabulary + store lock recovery (F-027, F-112, F-114)`.

### Task 4: Core ML traits + the before-transcription seam

**Files:** Modify `crates/humm-core/src/boundary.rs` (three traits), `crates/humm-core/src/queue.rs` (RunnerDeps + run_job block + cleanup guard), `crates/humm-core/src/pipeline.rs` (signature growth), `crates/humm-core/src/postproc/langdetect.rs` (new), `crates/humm-core/src/export.rs` (naming), `crates/humm-cli/src/run.rs` + `crates/humm-gui/src/services.rs` (RunnerDeps literal: `None`s for now).
**Spec:** §2, §3 (placement/order/cleanup), §5 (seam + gating + selection). **Buzz:** `file_transcriber_queue_worker.py:135-164,180-229`, `plugins/manager.py:107-119`, `plugins/enhanced_language_detection/plugin.py:59-169`, `transcriber/transcriber.py:228-265`.
**Produces:**
```rust
pub trait SpeechIsolator: Send + Sync {
    fn isolate(&self, input: &Path, cancel: &CancelToken) -> Result<PathBuf, MediaError>;
    fn available(&self) -> bool;
}
pub trait NoiseReducer: Send + Sync {
    fn reduce(&self, input: &Path, cancel: &CancelToken) -> Result<PathBuf, MediaError>;
}
pub trait LanguageDetector: Send + Sync {
    fn detect(&self, audio: &Pcm, model: &Path, cancel: &CancelToken)
        -> Result<Option<Language>, EngineError>;
}
```
`RunnerDeps.{isolator: Option<Arc<dyn SpeechIsolator>>, reducer: Option<Arc<dyn NoiseReducer>>, lang_detector: Option<Arc<dyn LanguageDetector>>}`;
`run_job` pre-transcription block (after check_skip, before ensure_artifacts): (1) isolation — gated on `spec.options.extract_speech && isolator.is_some()`; emits `PhaseChanged{IsolatingSpeech}`; failure → probe: `!has_audio` ⇒ log + proceed on original (Buzz no_audio), else `Terminal::Failed` with the actionable demucs message; (2) reduction — gated on `cfg.noise_reduction.enabled && reducer.is_some()`, `catch_unwind`-contained, failure/panic ⇒ log + proceed on prior audio; (3) detection — gated on `cfg.language_detection.enabled && lang_detector.is_some()` and `postproc::langdetect::should_detect(options.language)` (None or English — the en-quirk), model via `postproc::langdetect::select_model(model_root) -> Option<PathBuf>` (largest downloaded by size rank, `Custom`/`Lumii` excluded, file+sentinel checked) else tiny via `self.artifacts.ensure_model` when `download_tiny_if_missing` (progress → `ModelDownloadProgress`); decode current audio via `self.decoder`; `catch_unwind`-contained; on `Some(lang)`: store `update_language` + pass to run_file_job. `TempAudioGuard` (Drop) deletes isolated + denoised files on every exit path incl. panic — denoised exempted when `keep_denoised_file`;
`run_file_job(job, audio_input: &Path, language_override: Option<Language>, …)` — decode uses `audio_input`; effective language = `language_override.or(job.options.language)` feeds `EngineOptions`, the translator config, and `output_file_path`'s `{{ language }}` token; export naming stays on `job.input`;
`output_file_path` strips a trailing `_speech` from the input stem (Buzz `transcriber.py:238-240`, byte-parity).

- [ ] Failing tests (queue, scripted doubles — `ScriptedIsolator`/`ScriptedReducer`/`ScriptedDetector` fakes in queue tests): `extract_speech_swaps_decode_input_and_emits_phase` (decoder records path decoded); `isolation_failure_no_audio_proceeds_on_original`; `isolation_failure_with_audio_fails_task_actionably`; `isolation_output_deleted_on_completed_and_on_failed` (temp guard, both terminals); `reducer_failure_proceeds_on_prior_audio`; `reducer_panic_is_contained`; `detector_runs_only_when_language_none_or_english` (gating matrix: None→runs, Some(En)→runs, Some(Fr)→skips, disabled→skips); `detected_language_reaches_engine_export_name_and_store`; `detector_absent_model_downloads_tiny_when_enabled` (fake ensurer records the Tiny request); `hooks_never_run_when_deps_none`; pipeline: `run_file_job_decodes_audio_input_not_job_input`; export: `output_file_path_strips_speech_suffix` (+ the user-file-named-`x_speech` case); langdetect unit: `select_model_prefers_largest_downloaded` (fake root with tiny+small files+sentinels → small), `select_model_skips_custom_and_lumii`.
- [ ] Implement; both binaries' `RunnerDeps` literals gain explicit `isolator: None, reducer: None, lang_detector: None` (wired in Tasks 5–8).
- [ ] Gates; commit `M8: ML boundary traits + before-transcription seam in the runner (F-027, F-112, F-114)`.

### Task 5: F-027 — DemucsIsolator adapter + CLI `-e`

**Files:** Create `crates/humm-media/src/demucs.rs`; modify `crates/humm-media/src/lib.rs`, `crates/humm-media/src/ffmpeg.rs` (only if shared kill helpers move), `crates/humm-cli/src/args.rs`/`run.rs` (validate + wiring), `crates/humm-cli/tests/cli.rs`, `crates/humm-gui/src/folder_watch.rs`-adjacent test (re-verify `_speech.mp3` skip), `docs/HUMM_SURFACE.md` §1 (`-e`).
**Spec:** §3, ADR-023. **Buzz:** `file_transcriber_queue_worker.py:55-108,180-206,276-350,339-350,406-416`. **Spike C** fixed the exact output layout.
**Produces:** `DemucsIsolator::new() -> Self` (discovery: `HUMM_DEMUCS` env override else `PATH` lookup, probed lazily + cached); `impl SpeechIsolator` — command per ADR-023 into a `tempfile::tempdir` scratch, rename `vocals.mp3` → `<input_stem>_speech.mp3` beside the input (cross-device copy fallback), child waited with ~100 ms `try_wait` polls checking `cancel`, on cancel SIGTERM → 5 s → SIGKILL then `Err(MediaError::Canceled)`-equivalent; non-zero exit → `MediaError` with stderr tail + the actionable message naming demucs and the first-run model download; `available()` = binary found. CLI: `-e` validation changes from usage-fail to: demucs absent → `usage_fail("demucs not found — install it (e.g. pipx install demucs) or omit --extract-speech")`; present → `options.extract_speech = true`, isolator wired into `RunnerDeps`.

- [ ] Failing unit tests (no demucs needed): `discovery_prefers_env_override` (point `HUMM_DEMUCS` at a stub script via constructor-injected env value — never `set_var`; give `new` a `from_env(Option<OsString>)` seam), `available_false_when_binary_missing`, `isolate_renames_vocals_next_to_input` (stub `demucs` = shell script writing a fake `vocals.mp3` into the expected layout), `isolate_kill_on_cancel_acks_promptly` (stub script sleeps 30 s; cancel after 200 ms; assert return < 2 s), `nonzero_exit_yields_actionable_error`.
- [ ] `#[ignore]` contract tests (real demucs via `uvx`, sandbox-lifted, `HUMM_DEMUCS_CONTRACT=1`-gated): real vocals produced from `whisper-french.mp3`, output is decodable audio of ~equal duration.
- [ ] CLI: cli-int updates — `e_flag_without_demucs_fails_actionably` (env-scrubbed PATH), `e_flag_accepted_with_stub_demucs` (stub on PATH; task completes; `_speech.mp3` cleaned up after run); the old "not yet available" test retired.
- [ ] Folder-watch: assert the skip-rule test covering `_speech.mp3` exists (add if M7 missed it).
- [ ] Gates; commit `M8: DemucsIsolator subprocess adapter + CLI -e live (F-027, F-002)`.

### Task 6: F-027 — GUI: options sheet, capability gating, phase label

**Files:** Modify `crates/humm-gui/src/views/options_sheet.rs`, `crates/humm-gui/src/services.rs` (capabilities + wiring), `crates/humm-gui/src/format.rs` (status text), `crates/humm-gui/tests/kittest_import.rs`, ftl ×4 (en/de/es/zh-CN).
**Spec:** §3 (UI half), §8. **Buzz:** `widgets/transcriber/file_transcription_form_widget.py:54-62,104-106`.
**Produces:** the extract-speech checkbox live — bound to a real `SheetState` field, persisted to `FileTranscriberSettings.extract_speech` and into the enqueued `TranscriptionOptions`; disabled-with-actionable-tooltip **only when** demucs is absent (`services::Capabilities` gains `demucs: bool` from `DemucsIsolator::available()`); `viewmodel::Phase::Isolating` renders "isolating speech" in the status column (`format::status_text`); `RunnerDeps.isolator` wired in `services::init`; the `options-extract-speech-tooltip` "later release" copy replaced by the absence tooltip (key renamed `options-extract-speech-missing-tooltip`; `all_keys_used()` updated).

- [ ] Failing kittests: `extract_speech_checkbox_enabled_when_demucs_present` (capabilities fixture true → toggling persists the setting + lands in enqueued options — the existing run-persists pattern), `extract_speech_checkbox_disabled_with_tooltip_when_demucs_absent` (replaces the old always-disabled assertion), viewmodel/format unit: `status_text_isolating_speech`.
- [ ] Implement + ftl ×4.
- [ ] Gates; commit `M8: extract-speech options-sheet checkbox + demucs capability gating (F-027, F-023)`.

### Task 7: F-112 — humm-dfn crate + queue integration + prefs

**Files:** Create `crates/humm-dfn/` (Cargo.toml, `src/lib.rs`); modify workspace `Cargo.toml`, `crates/humm-media/src/ffmpeg.rs` (`decode_mono_at`), `crates/humm-gui/src/services.rs` (wire reducer), `crates/humm-gui/src/views/prefs_postproc.rs`, ftl ×4.
**Spec:** §4. **Buzz:** `plugins/deep_filter_net/plugin.py` (whole file). **Spike A** fixed the API entry points + rev.
**Produces:** `humm-dfn` crate: `deep_filter = { git = "https://github.com/Rikorose/DeepFilterNet", rev = "<spike-recorded v0.5.6 rev>", default-features = false, features = ["tract","default-model"] }`; `DfNoiseReducer::new(decoder: Arc<FfmpegDecoder>) -> Self` (humm-dfn depends on humm-media — adapter composition, documented in lib.rs + ARCHITECTURE sweep); `impl NoiseReducer` — `decoder.decode_mono_at(input, df_sample_rate, cancel)` → enhance via the spike-recorded deep_filter API → 16-bit PCM WAV via `humm_core::wav` → `<input_stem>_DeepFilterNet3.wav` beside input (Buzz naming `plugin.py:48`); any failure → `Err(MediaError…)` (the queue seam already degrades-and-proceeds); `FfmpegDecoder::decode_mono_at(path, sample_rate: u32, cancel) -> Result<Pcm, MediaError>` (generalizes the existing 16 k decode; existing `decode` delegates). Prefs: `prefs_postproc.rs` gains a **Noise reduction (DeepFilterNet)** CollapsingHeader — enable checkbox + "Keep denoised file after transcription" (Buzz's `config_fields` label verbatim), write-through commit fns; queue wiring: `RunnerDeps.reducer = Some(…)` in services.

- [ ] Failing unit tests: humm-dfn `reduce_writes_sibling_wav_with_expected_name` + `reduce_failure_on_missing_input_is_err` (tiny synthesized wav input; enhancement is real — the model is embedded and CPU-cheap at test length); queue (already in Task 4) covers degrade paths; prefs unit: section state commit fns persist; kittest: `noise_reduction_section_renders_and_toggles`.
- [ ] `#[ignore]` contract test: denoise the spike's noise-mixed fixture; assert duration ±5 % and RMS reduction in a known noise-only span.
- [ ] Wire services; ftl ×4; keep-file semantics test at queue level: `denoised_file_kept_when_setting_set` (guard exemption).
- [ ] Gates; commit `M8: DeepFilterNet noise reduction — humm-dfn crate + seam wiring + prefs (F-112)`.

### Task 8: F-114 — WhisperLangDetector + prefs

**Files:** Create `crates/humm-whisper/src/langdetect.rs`; modify `crates/humm-whisper/src/lib.rs`, `crates/humm-gui/src/services.rs` (wire detector), `crates/humm-gui/src/views/prefs_postproc.rs`, ftl ×4.
**Spec:** §5. **Buzz:** `plugins/enhanced_language_detection/plugin.py` (whole file), `transcriber/whisper_cpp.py:403-470`.
**Produces:** `WhisperLangDetector::new(force_cpu: bool) -> Self`; `impl LanguageDetector` — fresh `WhisperContext` on `model` (gpu per `!force_cpu`), fresh state, `pcm_to_mel`, `state.lang_detect(0, threads)`, map id via `whisper_rs::get_lang_str` → `Language::parse`; unknown/unmapped code → `Ok(None)`; context dropped per call (Buzz spawns a process per detection — same cost shape). Prefs: **Enhanced language detection** section — enable + "Download tiny model if none available" (Buzz label verbatim). Wiring: `RunnerDeps.lang_detector = Some(…)` with `force_cpu` from `settings.models.force_cpu`.

- [ ] Failing `#[ignore]` contract test (real tiny model from the humm cache, sandbox-lifted for Metal or forced-cpu to run sandboxed — force_cpu(true) keeps it sandbox-safe; verify empirically in-task and gate accordingly): `detects_french_fixture` — decode `whisper-french.mp3` via FfmpegDecoder, detect with `ggml-tiny.bin` → `Some(Language::French)`.
- [ ] Unit (no model): trait-level behavior is queue-tested (Task 4); here: `unknown_lang_id_maps_to_none` via the id→Language mapping helper extracted pure (`map_lang_id(i32) -> Option<Language>` — test the boundary ids).
- [ ] Prefs section + ftl ×4 + kittest `language_detection_section_renders_and_toggles`.
- [ ] End-to-end queue test with the real seam shape (scripted detector already in Task 4; here add cli-visible check): GUI-side integration is covered by the wiring + Task 4's `detected_language_reaches_engine_export_name_and_store`.
- [ ] Gates; commit `M8: enhanced language detection — WhisperLangDetector + prefs (F-114)`.

### Task 9: F-087 — humm-diarize adapter + model downloads

**Files:** Create `crates/humm-diarize/` (Cargo.toml, `src/lib.rs`), `crates/humm-core/src/diarize.rs` (trait + types only this task); modify workspace `Cargo.toml`, `crates/humm-core/src/models/registry.rs`, `crates/humm-core/src/models/download.rs`, `crates/humm-core/src/lib.rs`.
**Spec:** §6 (adapter + models), ADR-024. **Spike B** fixed the crate, version, API, and model URLs.
**Produces:**
```rust
// humm-core::diarize
pub struct SpeakerTurn { pub start: Millis, pub end: Millis, pub speaker: usize }
pub enum DiarizeStage { LoadingModels, Segmenting, Embedding, Clustering }
pub struct DiarizeProgress { pub stage: DiarizeStage, pub fraction: Option<f32> }
#[derive(thiserror::Error, Debug)] pub enum DiarizeError {
    #[error("diarization models missing — {detail}")] ModelsMissing { detail: String },
    #[error("diarization failed — {detail}")] Failed { detail: String },
    #[error("diarization canceled")] Canceled,
}
pub trait Diarizer: Send + Sync {
    fn diarize(&self, audio: &Pcm, progress: &mut dyn FnMut(DiarizeProgress),
               cancel: &CancelToken) -> Result<Vec<SpeakerTurn>, DiarizeError>;
}
```
`registry.rs`: `DIARIZE_DIR = "diarization"`, per-model consts (file names + pinned URLs from the spike) + `diarization_model_paths(root) -> DiarizationModelPaths { … }`; `ModelDownloader::ensure_diarization_models(&self, progress, cancel) -> Result<DiarizationModelPaths, ModelError>` — each file via the existing `ensure_artifact` (resume/sentinel/lock; `sha_repo` per what the hosting repo publishes — spike records); `humm-diarize::OrtDiarizer::new(models: DiarizationModelPaths) -> Result<Self, DiarizeError>` + `impl Diarizer` mapping the spiked crate's pipeline output to `SpeakerTurn`s (ms), emitting stage progress where the crate exposes it and checking `cancel` between windows/stages (`Canceled` on trip).

- [ ] Failing unit tests: registry path/url shape tests (mirroring the VAD consts tests); download: `ensure_diarization_models_downloads_all_and_writes_sentinels` against the existing `test_server` fixture pattern (fake artifacts; TCP-bind-blocked sandboxed — this reuses existing mock-server infra and stays within the documented humm-core count; if it must bind a new listener, state the new count in the commit).
- [ ] `#[ignore]` contract test (real models, sandbox-lifted): `two_speaker_fixture_yields_two_speakers` — decode `two-speakers.wav`, diarize, assert ≥2 speakers and a boundary within ±2 s of each splice point (from the fixture README).
- [ ] First lifted `cargo build -p humm-diarize` (ort runtime fetch) — record in the task report.
- [ ] Gates; commit `M8: humm-diarize adapter + diarization model downloads (F-087)`.

### Task 10: F-087 — core mapping: turns → labeled segments

**Files:** Modify `crates/humm-core/src/diarize.rs`.
**Spec:** §6 (mapping). **Buzz:** `speaker_identification_widget.py:727-800` (save path), `whisper_diarization` helpers' outcome shape (labels "Speaker N").
**Produces:**
```rust
/// Per-segment speaker index, relabeled 0..N by first appearance.
pub fn assign_speakers(segments: &[Segment], turns: &[SpeakerTurn]) -> Vec<usize>
/// "{name}: {text}" segments; merge joins consecutive same-speaker segments
/// (end extended, texts joined with one space).
pub fn label_segments(segments: &[Segment], speakers: &[usize],
                      names: &[String], merge: bool) -> Vec<Segment>
```
Assignment: max temporal overlap wins; zero-overlap segment → nearest turn by midpoint distance; empty `turns` → all speaker 0.

- [ ] Failing unit tests: `assign_by_max_overlap`, `zero_overlap_takes_nearest_turn`, `relabel_orders_by_first_appearance` (turns arrive 1,0 → segment speakers 0,1), `label_without_merge_prefixes_every_segment` (exact `"Alice: hello"` shape, timestamps preserved), `merge_joins_consecutive_same_speaker` (end extended, `"a b"` join, boundaries preserved across speaker changes), `merge_respects_renames` (two turns mapped to the same entered name merge — Buzz merges on the *mapped* name, `:747-751`), `empty_turns_single_speaker`.
- [ ] Implement (pure, no I/O).
- [ ] Gates; commit `M8: speaker assignment + labeling core (F-087)`.

### Task 11: F-087 — Speaker-ID window (J-7)

**Files:** Create `crates/humm-gui/src/speaker_id.rs` (vm + worker + `DiarizeService` trait), `crates/humm-gui/src/views/speaker_id.rs`, `crates/humm-gui/tests/kittest_speaker_id.rs`; modify `crates/humm-gui/src/app.rs` (map/open/show/repaint/cleanup), `crates/humm-gui/src/views/viewer.rs` (button live), `crates/humm-gui/src/services.rs` (real `DiarizeService`), ftl ×4 (or ×15 if after Tasks 13–15 — follow Global Constraint 5 at execution time).
**Spec:** §6 (window), UX_SPEC J-7. **Buzz:** `speaker_identification_widget.py` (whole file — states `:489-541`, naming rows `:663-705`, preview `:707-725`, save `:727-800`, cancel `:616-624,824-840`).
**Produces:**
```rust
pub trait DiarizeService: Send + Sync {
    fn ensure_models(&self, progress: &mut dyn FnMut(f32), cancel: &CancelToken)
        -> Result<(), String>;
    fn diarize(&self, audio: &Pcm, progress: &mut dyn FnMut(DiarizeProgress),
               cancel: &CancelToken) -> Result<Vec<SpeakerTurn>, String>;
}
```
(real impl in services over `ModelDownloader::ensure_diarization_models` + lazily-built `OrtDiarizer`; scripted impl for kittest); `SpeakerIdVm` — stages Idle / MissingAudio / Running{label, fraction} / Ready{n_speakers, names: Vec<String>, merge: bool (default true)} / Error(String); worker thread: decode (existing `AudioDecoder` from viewer deps) → ensure_models → diarize → `assign_speakers`; play-sample via a window-owned humm-audio `Player` on `decode_playback`, random segment of the chosen speaker, stopped at min(10 s, sample length); Save → `store.copy_transcription(source)` + `mark_replaced_completed(new_id, label_segments(…))` → `AppEvent::TranscriptionsUpdated { affected: new_id, source }` → window closes (source viewer stays open — only the resizer's close-source semantics differ; follow Buzz: the widget closes itself, the viewer remains); viewer toolbar button goes live (gate: audio file exists → else MissingAudio state, Buzz `:514-518`); deferred viewport keyed `("speakers", id)`, focus-if-open, cleanup on viewer close; `app.rs` repaint fan-out gains the key.
**Interface note:** `AppEvent::TranscriptionsUpdated`'s existing handler closes the *source viewer* (resizer semantics, app.rs:2215-2225). Speaker-ID must NOT close the source viewer — add a `close_source: bool` field to the event (resizer sends true, speaker-id false) and branch in `apply_transcriptions_updated`.

- [ ] Failing kittests (scripted `DiarizeService`): `button_opens_window_and_missing_audio_gates` ; `identify_runs_to_ready_with_speaker_rows` (scripted 2 speakers → two name rows prefilled "Speaker 0"/"Speaker 1", merge checked, Save enabled); `cancel_mid_run_resets_to_idle` ; `error_shows_message_and_resets`; `save_writes_copied_transcription_with_labels` (real temp store: copy row exists, segments `"Name: text"`, merge semantics applied, event refreshes the table, source viewer untouched); app-level `resizer_close_semantics_unchanged` (regression for the event-field split).
- [ ] Unit: vm state transitions; sample-selection (random segment of speaker; duration cap min(10 s, len)).
- [ ] Implement + ftl; retire `viewer-identify-speakers-tooltip` (kittest assertion updated).
- [ ] Gates; commit `M8: Speaker-ID two-step window (F-087)`.

### Task 12: Live-API hardening — tolerate-3 + L15 provenance test

**Files:** Modify `crates/humm-core/src/live/session.rs`, `crates/humm-core/src/live/mod.rs` (LiveEvent), `crates/humm-gui/src/recording.rs`/`views/recording.rs` (surfacing), create `crates/humm-gui/tests/live_api_provenance.rs`; ftl per Constraint 5; `docs/HUMM_SURFACE.md` if the event is CLI-visible (it is not — live is GUI-only).
**Spec:** §7.4, §7.5. **Ledger:** M7 L15/L17 (spec 2026-07-25 §ledger).
**Produces:** `LiveEvent::ChunkSkipped { detail: String }` (non-terminal); `run_session`: `const MAX_CONSECUTIVE_CHUNK_FAILURES: usize = 3` — engine `Err` (non-Canceled) increments, emits `ChunkSkipped`, drops the chunk's text (rolling prompt unchanged — never Buzz's fake `"Error: …"` prompt text), continues; success resets; 3rd consecutive → existing `Error` + stop; recording window shows a transient warning line on `ChunkSkipped` (label with the detail, cleared on next successful text).
**L15 test:** `live_api_provenance.rs` — a local mock server (humm-gui's FIRST TCP-bind test — commit message states the new baseline 67+1+1) scripts two chunks through `ApiLiveEngineSession::transcribe_live`: `LiveOptions{language: Some(French), task: Transcribe}` → request hits `/audio/transcriptions` with form field `language=fr`; `{language: None, task: Translate}` → `/audio/translations`, no language field.

- [ ] Failing session tests: `chunk_error_emits_chunk_skipped_and_continues` (next chunk still transcribed; prompt is the last *successful* text), `success_resets_consecutive_counter` (fail,fail,ok,fail,fail → still alive), `third_consecutive_failure_ends_with_error` ; update `engine_error_emits_error_then_stopped_once_and_ends_the_loop` (now needs three consecutive errors — rename to match).
- [ ] GUI surfacing + kittest: `chunk_skipped_shows_transient_warning`.
- [ ] The provenance test (mock server asserting path + form fields).
- [ ] Update the L17 ledger note in the M8 spec's ledger (#9 already written) — DECISIONS/spec cross-ref stays as-is.
- [ ] Gates; commit `M8: live-API tolerate-3-consecutive-chunk-failures + L15 provenance test (F-061) — humm-gui TCP-bind count now 1 (baseline 67+1+1)`.

### Task 13: Locales batch 1 — ca_ES, da_DK, it_IT, nl

**Files:** Create `crates/humm-gui/assets/i18n/{ca,da,it,nl}/humm.ftl`; modify `crates/humm-gui/src/i18n.rs` (4 registry entries + 4 `include_str!` consts).
**Spec:** §7.2. **Precedent:** M7 Task 12 (de/es/zh) — same glossary rule: for every key whose en string matches a Buzz msgid, use Buzz's `.po` msgstr from `buzz/locale/<code>/LC_MESSAGES/buzz.po` (fixing Buzz's corrupted/doubled msgstrs where present — document each in the catalog header comment); humm-only keys translated fresh; capability-parity divergences documented in header comments. Registry order stays Buzz's `ui_locales` order (en_US, ca_ES, da_DK, de_DE, es_ES, it_IT, ja_JP, lv_LV, nl, pl_PL, pt_BR, ru, uk_UA, zh_CN, zh_TW — insert each entry at its ordinal slot).
**Produces:** 4 complete catalogs (every key the en catalog has at this point), auto-covered by the per-locale completeness tests.

- [ ] Confirm the per-locale tests fail for a missing/incomplete catalog (add one entry, run, see the completeness failure) — then complete each catalog.
- [ ] `display_name` keys (`locale-name-ca-es` etc.) added to ALL existing catalogs (Constraint 5).
- [ ] Gates; commit `M8: locale catalogs ca_ES/da_DK/it_IT/nl (F-008)`.

### Task 14: Locales batch 2 — ja_JP, pl_PL, pt_BR, ru

Same shape as Task 13 for `{ja,pl,pt-BR,ru}` (dir names follow the existing zh-CN hyphenation convention — check `i18n.rs`'s dir naming and match it; Buzz codes `ja_JP,pl_PL,pt_BR,ru`). ja_JP is the CJK-font consumer — the Task 1 font must already be merged (it is, Task 1 precedes).
- [ ] Catalogs + registry + `locale-name-*` keys everywhere; gates; commit `M8: locale catalogs ja_JP/pl_PL/pt_BR/ru (F-008)`.

### Task 15: Locales batch 3 — lv_LV, uk_UA, zh_TW

Same shape for `{lv,uk,zh-TW}`. lv_LV is the Lumii-gate locale (F-044's windowed check uses it); zh_TW renders through the Task 1 font (SC-variant glyphs — the documented divergence).
- [ ] Catalogs + registry + `locale-name-*` keys everywhere; **after this task the catalog count is 15 — Constraint 5 now means ×15 for any later key**; gates; commit `M8: locale catalogs lv_LV/uk_UA/zh_TW — all 15 Buzz locales shipped (F-008)`.

### Task 16: Transcription-path benchmark vs same-day M7

**Files:** Create `docs/benchmarks/M8.md`; bench worktree at `5e70790` (M7) per `docs/benchmarks/M7.md` methodology.
**Spec:** §10.3, Constraint 9.
- [ ] Call-path innocence first: diff the default-path transcription code M7→M8 (`git diff 5e70790..HEAD -- crates/humm-core/src/pipeline.rs crates/humm-whisper/src/engine.rs` + the queue's non-hook path) — document that pre-hooks-off adds only `None`/`false` checks.
- [ ] Interleaved pairs (M7 build ↔ M8 build alternating, same fixture + model, sandbox-lifted for Metal), pooled comparison per M7.md's rules (never same-day-vs-recorded).
- [ ] Criterion suite run (decode/resample/export) — no regression.
- [ ] Write `docs/benchmarks/M8.md`; regression = stop and fix before proceeding.
- [ ] Commit `M8: transcription-path bench vs same-day M7 — no regression`.

### Task 17: Windowed acceptance — gui-manual v6 + outcome diffs + graceful absence

**Files:** Create `docs/gui-manual/M8.md`; validation artifacts under `validation/m8-acceptance/`.
**Spec:** §10.1-2/4, §7.6, §9. Sandbox-lifted, `caffeinate -dimsu`, CGEvent automation (NOT System Events click-at), `HUMM_LOG_DIR` for logs.
- [ ] New-feature windowed checks: extract-speech end-to-end (uvx demucs; `_speech.mp3` appears then is cleaned; export named from original), DFN denoise run (wav appears/cleans; keep-file variant), lang-detect run (French fixture, auto → `fr` in status/export name), Speaker-ID full J-7 journey (identify with real models → name → play sample → merge → Save → new row opens in viewer).
- [ ] CJK check: zh_CN + zh_TW + ja_JP locales render real glyphs (screenshots committed) — flips the F-008 gate.
- [ ] F-044 windowed pass: Delete + confirm modal, Show file location, live download + cancel, speed-badge tooltip, Lumii under lv_LV.
- [ ] Graceful absence: demucs absent (checkbox tooltip; CLI -e message), diarization models absent + network blocked (actionable message, no crash), corrupt input.
- [ ] Outcome diffs vs Buzz recorded (speech isolated, speakers found/nameable/mergeable, noise reduced, language detected) — `validation/m8-acceptance/README.md`.
- [ ] Sweep the remaining `implemented` matrix rows: enumerate `grep '| implemented' docs/analysis/PARITY_MATRIX.md` and execute each row's outstanding gui-manual/method bits in this same windowed session (list them explicitly in M8.md — e.g. F-003 layout persistence across restart, F-004/F-006/F-007 windowed halves, F-020..F-026 outstanding items, F-100/F-110 etc. as the matrix states them); re-probe the chat endpoint (`HUMM_LLM_TEST_BASE`/ollama/LM Studio) — if one exists, run the two real-chat contracts (flips F-031/F-111).
- [ ] Commit `M8: gui-manual v6 windowed acceptance + outcome diffs (F-027, F-087, F-112, F-114, F-008, F-044, …)`.

### Task 18: Mission-closing benchmark vs Buzz

**Files:** Create `docs/benchmarks/MISSION.md` (or §in M8.md — prefer separate); Buzz env in `/Users/jawad/RustroverProjects/buzz` (`uv sync`, sandbox-lifted, one-time; leave the checkout otherwise untouched).
**Spec:** §10.6.
- [ ] `uv sync` in the Buzz checkout (lifted; ~10 GB; record versions). If it fails after genuine effort: record exactly what failed and ship the report with the subset that ran.
- [ ] Startup-to-interactive: same method both sides (launch → first window visible via `CGWindowListCopyWindowInfo` polling, wall clock, ≥3 runs each, warm).
- [ ] E2e file transcription: same fixture (`audio-long.mp3`) + same model size (small) + whisper.cpp-class backend both sides (`buzz add --task transcribe --model-type whispercpp --model-size small --txt --hide-gui` vs `humm add … --txt -s small`), `/usr/bin/time -l` wall + peak RSS, interleaved runs.
- [ ] Memory: peak RSS from the e2e runs + idle-window RSS both apps.
- [ ] Write the report (numbers, method, caveats — RNG/transcript-text differences out of scope per diff rules); commit `M8: mission-closing benchmark vs Buzz (startup, e2e, memory)`.

### Task 19: Final docs sweep + FINAL parity sweep + milestone/mission report

**Files:** Modify `docs/analysis/PARITY_MATRIX.md` (every touched row; FINAL state), `docs/analysis/FEATURE_INVENTORY.md` (F-112/F-114 method note if changed), `docs/design/DECISIONS.md` (ADR-023/024 validation results), `docs/design/ARCHITECTURE.md` (§2 traits as-built; adapter-composition note), `docs/design/UX_SPEC.md` (J-7 shipped record), `docs/design/IMPLEMENTATION_PLAN.md` (M8 exit note), `docs/HUMM_SURFACE.md`, `CLAUDE.md` (workspace crate list), memory file update; create `.superpowers/sdd/m8-final-report.md`-equivalent summary in-session.
**Spec:** §10.5/7.
- [ ] Matrix: flip every M8 row per its executed method; run the FINAL sweep — target ALL rows `verified`; any exception (expected: F-031/F-111 real-chat if still no endpoint) is named with its exact blocker and the committed-runnable test that closes it.
- [ ] ADR-023/024 validation-result sections filled from Tasks 5/9/17 evidence.
- [ ] Milestone report + mission-completion summary (final matrix state, Buzz-vs-humm numbers, complete deviations ledger M1–M8, remaining debt) — delivered in-session and committed as the docs sweep.
- [ ] Gates; commit `M8: final docs sweep + parity matrix FINAL state + mission report`.

---

## Self-review notes (kept honest)

- Spec coverage: §1→T2/ADRs (committed with spec); §2→T4; §3→T4/T5/T6; §4→T7; §5→T4/T8; §6→T9/T10/T11; §7.1→T1; §7.2→T13-15; §7.3→T3; §7.4/7.5→T12; §7.6→T17; §8→T3/T6/T19; §9→per-task tests + T17/T19; §10→T16/T17/T18/T19; §11 ledger→enforced per task; §12 risks→T2 gates them.
- Type consistency pinned: `SpeechIsolator`/`NoiseReducer` take `&Path`, return `PathBuf`/`MediaError`; `LanguageDetector`/`Diarizer` take `&Pcm`; `DiarizeService` is the GUI seam (String errors for display); `AppEvent::TranscriptionsUpdated` gains `close_source: bool` (T11 — resizer true, speaker-id false).
- Execution order: T1→T2 first (user-visible fix, then evidence); T3→T4 before all feature tasks; T5/T6, T7, T8 independent after T4; T9→T10→T11 sequential; T12 independent; T13–T15 after feature tasks (so new keys exist); T16–T19 strictly last.
