# M5 Live Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship live recording — cpal capture, realtime in-process transcription, three display modes, live TXT/CSV export + upload URL, presentation window, clipboard copy, persisted live sessions (Q-10 ✚) — per `docs/superpowers/specs/2026-07-23-m5-live-recording-design.md` (**the spec; read it before any task**), unlocking F-004's Record button + Ctrl+R. F-063 (live translation) stays M6: disabled affordance only.

**Architecture:** capture = cpal RT callback → rtrb ring → session thread (chunker + whisper-rs engine, ADR-005/ADR-008); recording + presentation windows are deferred viewports (M4 pattern); all algorithmic behavior (chunker, silence cuts, merge, exports) is pure `humm-core::live` code ported from `recording_transcriber.py` / `recording_transcriber_widget.py` and unit-tested against Buzz-derived fixtures; live sessions persist as `Source::LiveRecording` rows opening in the M4 viewer.

**Tech Stack:** existing egui/eframe/egui_kittest 0.35, cpal 0.18, rubato 4.0; NEW workspace-dep activation: rtrb 0.3 (already pinned in `[workspace.dependencies]`). Buzz ground truth at `/Users/jawad/RustroverProjects/buzz`.

## Global Constraints

- Every commit passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Commit messages: `M5: <what> (F-###[, F-###])` + `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Build env: `export CARGO_HOME="$PWD/.cargo-home"`. Sandbox gotchas (humm-port-status memory): TCP binds, GPU/Metal/CoreML, CoreAudio device opens, and writes outside the worktree need the sandbox lifted; real-device capture + real-model live tests are `#[ignore]` contract tests; everything else runs on `MockCapture`/fakes.
- Buzz is ground truth; the spec §2 pins behavior with `file:line` citations — **re-read the cited Buzz source before implementing each feature**. Fixture vectors for the merge/chunker come from executing the genuinely-extracted Buzz functions with the local python3 (M4 `validation/m4-differential` precedent), committed + regenerable under `validation/m5-differential/`.
- No network/device access in unit tests. humm-core keeps `#![warn(missing_docs)]` + `#![forbid(unsafe_code)]`; no `unsafe` in humm-audio; `thiserror` in libs; exhaustive matches on domain enums.
- New UI strings → `humm.ftl` + `all_keys_used()` same commit; parity strings byte-match Buzz (spec cites each).
- GUI renderers are plain `fn(&mut egui::Ui, &mut RecordingVm, ...)` driven by kittest via `Harness::new_ui_state`; all external effects behind injected deps (`RecordingDeps`: capture, engine factory, store, clock, upload) so kittest never touches devices/network.
- The queue owns `status` for queue-executed tasks (ADR-018); live-session rows are written by the new store APIs only (they never go through the queue) — document in `queue.rs`'s existing scope note.
- PARITY_MATRIX rows move with the task that completes them; final sweep + report in Task 14.

---

### Task 1: Core `live::chunker` — batch/silence/drop state machine (F-061 logic half)

**Files:** Create `crates/humm-core/src/live/mod.rs`, `crates/humm-core/src/live/chunker.rs`; modify `lib.rs`.

Port `recording_transcriber.py` math verbatim (spec §2.2): `SAMPLE_RATE=16_000`; `Chunker::new(mode, transcription_step)` computing `n_batch_samples`/`keep_samples`/`max_queue_size`; `push(&[f32]) -> usize` (accepted count — drops while accumulation ≥ max_queue_size, Buzz callback-drop parity); `next_batch() -> Option<Batch { samples, start_ms, end_ms, rms, queue_len_after }>` implementing the cut/keep-back/advance rules; `find_silence_cut_point` (1.5 s search, 0.02 s windows, 0.5×mean threshold, rightmost-quiet midpoint, full-batch fallback — integer arithmetic byte-matching numpy's); `rms()` (= Buzz `amplitude`). Batch timeline (`start_ms`/`end_ms`) tracks absolute stream position for §4 persistence; keep-back overlap must NOT double-count time (start_ms of batch N+1 = cut position, not cut−keep).

- [ ] Failing unit tests first: batch-size/keep-back per mode (5 s vs step×rate/1.5 s), drop-when-3-batches-behind (push counts), silence-cut on synthetic signal (loud+quiet tail → cut at quiet-window midpoint; all-loud → full batch; sub-window region → full batch), rms values, timeline accounting incl. keep-back non-double-count, correct-mode advance = `cut − keep_samples`.
- [ ] `validation/m5-differential/gen_chunker_fixtures.py`: extract `find_silence_cut_point`+`amplitude` verbatim from Buzz source (import-free — copy the static methods into the harness at generation time, noting source lines), run over deterministic synthetic PCM vectors → committed JSON fixtures asserted exactly by Rust tests.
- [ ] Implement; gates green; **Commit** `M5: live chunker — batch/silence-cut/drop state machine (F-061)`.

### Task 2: Core `live::merge` + display modes (F-062 logic)

**Files:** Create `crates/humm-core/src/live/merge.rs`; modify `live/mod.rs`.

Port from `recording_transcriber_widget.py` (spec §2.3): `filter_text` (trim + `\w`-search — Unicode, use `regex` crate `\w` which matches Python's re Unicode default), `find_common_part` (DP longest-common-substring, ≥5 gate), `merge_text_no_overlap` (longest suffix=prefix elision), `no_space_between_sentences` (`([.!?。！？])([A-Z])` → `$1 $2`), and `MergeState { texts: Vec<String> }::push(text, hide_unconfirmed) -> String` reproducing `process_transcription_merge`'s trim-adjacent-pairs + hide-unconfirmed + fold + regex pipeline (mutating stored texts exactly as Buzz does — `rfind`/`find` semantics included). Plus `DisplayMode` application for append-below/above: `PanelText::apply(mode, text, separator)` (append+separator rules, cursor semantics are GUI-side).

- [ ] Failing unit tests first: each helper (incl. ≥5 gate boundary, no-`\w` rejection, CJK sentence punctuation), full merge sequences (overlapping chunks converge; hide-unconfirmed truncates/drops last; no-overlap chunk kept when hide off), append-above/below separator placement.
- [ ] `validation/m5-differential/gen_merge_fixtures.py`: extract the four static methods verbatim (they are Qt-free), drive `process_transcription_merge`'s algorithm over ≥8 chunk-sequence scenarios (incl. hide-unconfirmed on/off, real overlapping-transcript shapes, unicode) → committed JSON fixtures asserted exactly.
- [ ] Implement; gates green; **Commit** `M5: append-and-correct merge + display modes, Buzz-differential fixtures (F-062)`.

### Task 3: Core `live::export` — TXT/CSV/template/retry + upload client (F-064 logic)

**Files:** Create `crates/humm-core/src/live/export.rs`; modify `live/mod.rs`.

Spec §2.4: filename templating (`live_export_file_name(template, task, language, model_size, now)` — `%d-%b-%Y %H-%M-%S` via chrono-free formatting consistent with `format.rs` conventions), `write_txt_export` (append/prepend/rewrite + separator + max-entries trim, Buzz's exact split-filter-rejoin), `write_csv_export` (single-row utterance-per-column, utf-8-sig BOM preserved on read+write, append/prepend column, cap last/first N, `csv` crate), retry loop (5×200 ms on lock-class errors; injected sleeper for tests), `UploadClient` (blocking reqwest: `clear` + `transcript` bodies, 15 s timeout, log-only failures; `mode` = `"replace"` iff append-and-correct).

- [ ] Failing tests first: template substitution matrix; TXT all three modes × max-entries (0/2) × custom separator; CSV BOM byte + quoting round-trip (comma/quote/newline in utterance) + prepend cap-first-N vs append cap-last-N; retry gives up after 5 with warn; upload bodies asserted against the existing `TestServer` (sandbox-lifted set) incl. env-over-setting precedence helper (`effective_upload_url`).
- [ ] insta snapshots: one TXT (append, separator `\n\n`, 3 utterances) + one CSV (3 columns) golden.
- [ ] Implement; gates green; **Commit** `M5: live TXT/CSV export, filename template, upload client (F-064)`.

### Task 4: Core boundary — `AudioCapture` trait + `MeterSnapshot` (F-060/F-061 groundwork)

**Files:** Modify `crates/humm-core/src/boundary.rs`, `Cargo.toml` (core gains `rtrb`).

Spec §5 verbatim: `AudioDevice`, `CaptureHandle`, `AudioCapture` (rtrb producer + `Arc<MeterSnapshot>`), `MeterSnapshot` (3 atomics: current-RMS f32-bits, average-RMS, queue-len; `set_*`/`get_*` + `reset()`; current-amplitude smoothing stays GUI-side per spec §2.5). Extend `AudioError` if a capture-specific variant is needed (device-not-found naming the id, format-unsupported naming the format).

- [ ] Unit tests: MeterSnapshot round-trip/reset; trait object-safety (`Box<dyn AudioCapture>` compiles via a no-op fake).
- [ ] Gates green; **Commit** `M5: AudioCapture boundary + MeterSnapshot (F-060, F-061)`.

### Task 5: `humm-audio::CpalCapture` + `MockCapture` (ADR-005 capture half)

**Files:** Create `crates/humm-audio/src/capture.rs`, `crates/humm-audio/src/mock.rs`; modify `lib.rs`, `Cargo.toml` (+rtrb).

`CpalCapture` (spec §5): `devices()` (input devices, `is_default` from `default_input_device()` name-match; stable name-based ids), `start` negotiation — try 16 k mono f32; else device default config with per-callback downmix (channel average) + rubato fixed-ratio resample **in the drain path with preallocated state** (never allocates in the RT callback), i16/u16/f32 sample formats converted; ring overflow counted + logged; per-callback RMS → `MeterSnapshot`. `stop` joins/drops the stream promptly. `MockCapture` (pub): scripted `Vec<f32>` fed through the same producer at a scripted pace (manual `pump()` — no real time), for core-session/GUI tests and CI.

- [ ] Unit tests (device-free): downmix math, i16/u16 conversion, resampler drain produces expected sample counts for 48 k→16 k, overflow drop counting, MockCapture pumps scripted samples + RMS.
- [ ] `#[ignore]` contract tests (real default device, sandbox-lifted): stream opens + samples arrive (nonzero callback count), effective output is 16 k mono (duration-vs-sample-count tolerance), clean stop; **on this machine's 48 kHz mic the resample branch runs for real — record it in the test output + memory ticket debt (M4 hardware-debt item)**.
- [ ] Gates green (workspace incl. `-p humm-audio -- --ignored` run recorded); **Commit** `M5: cpal capture adapter + mock capture (F-060, F-061; ADR-005)`.

### Task 6: `humm-whisper` live mode (ADR-008 native hallucination guards)

**Files:** Modify `crates/humm-whisper/src/engine.rs`, `lib.rs`.

`WhisperEngine::transcribe_live(&mut self, samples: &[f32], opts: &LiveOptions) -> Result<String, EngineError>` (or equivalently a `LiveParams` constructor + thin wrapper — implementer picks the cleaner seam, spec §2.2.4): beam-search defaults, `n_max_text_ctx = 8` (Buzz `--max-context 8`), suppress_nst + entropy 2.8, language/task, rolling prompt passed per call (interior-NUL guard reused), temperature schedule head, **no VAD, no word timestamps, no progress callback**; segment texts joined `" "` after per-segment trim (match Buzz's server-JSON join observable: single-space-separated utterance text). Abort callback wired to the per-call `CancelToken`.

- [ ] Unit tests: LiveOptions→params mapping (a pure builder test mirroring `build_full_params`'s existing test pattern), join/trim rule on a fake segment list.
- [ ] `#[ignore]` contract test (real tiny model via `HUMM_WHISPER_TEST_MODEL`, sandbox-lifted): two successive `transcribe_live` calls on speech PCM return non-empty text; second call passes first call's text as prompt without error; cancel mid-call aborts.
- [ ] Gates green; **Commit** `M5: in-process live transcription params (F-061; ADR-008)`.

### Task 7: `humm-store` live-session rows + stale-flip special case (Q-10 ✚)

**Files:** Modify `crates/humm-store/src/store.rs` (+schema if any column gap surfaces — none expected: `kind` exists).

APIs (spec §4): `insert_live_session(name, options) -> TranscriptionId` (Source::LiveRecording, status InProgress, options_json full), `append_live_segment(id, start, end, text) -> Result<()>`, `complete_live_session(id) -> Result<()>` (Completed + time_ended). `cancel_stale_tasks` special case: stale InProgress live rows → **Completed** (others unchanged → Canceled).

- [ ] Failing tests first: insert/append/complete round-trip (segments ordered, viewer-shaped via `get_segment_rows`), options_json survives `load_task_spec`, stale-flip matrix (live InProgress→Completed, file InProgress→Canceled, queued unaffected rules as today), append after complete errs or is ignored (pick + pin), name stored verbatim.
- [ ] Gates green; **Commit** `M5: live-session persistence + live-aware stale flip (Q-10, F-060)`.

### Task 8: Core `live::session` — orchestration loop + realtime measurement

**Files:** Create `crates/humm-core/src/live/session.rs`; modify `live/mod.rs`.

Spec §3: `RecordingSession::run(capture, engine: impl FnMut(&[f32], &str /*prompt*/) -> Result<String, EngineError>, sink, cancel, opts)` on the caller's thread (GUI spawns it): drain ring → chunker → skip/transcribe with rolling prompt → `LiveEvent`s (`Text`, `MeterBatch`, `Error`, `Stopped`); 500 ms interruptible idle pacing; realtime measurement (Σ audio-s / Σ engine-wall-s, skipped batches excluded) returned at stop; store writes happen GUI-side off the events (keeps core session store-free). Stop semantics: cancel → in-flight engine call aborts → drain stops → `Stopped` always sent (even after `Error`).

- [ ] Failing tests first (MockCapture-free — feed the ring directly via an in-test producer, scripted fake engine): chunk cadence per mode, silence-skip (no engine call, no Text), rolling prompt (engine sees previous text), drop-behind behavior surfaces in `MeterBatch.queue_len`, error from engine → `Error` then `Stopped`, cancel mid-loop → prompt `Stopped` ≤ pacing interval, measurement math (scripted clock).
- [ ] Gates green; **Commit** `M5: live session orchestration loop (F-061)`.

### Task 9: Settings — `RecordingSettings` + `PresentationSettings`

**Files:** Modify `crates/humm-core/src/settings.rs`.

Spec §6 field list + defaults (silence 0.0025, separator `"\n\n"`, step 3.5, mode 0, hide_unconfirmed true, export_file_type "txt", max_entries 0, presentation 24 pt/"light"/#000000/#FFFFFF); `measured_speed: BTreeMap<String, f32>`; manual `Default` (non-zero defaults) + `#[serde(default)]` per the ViewerSettings pattern; step clamped 2.0–5.0 and text-size 10–100 on load (clamp-on-load precedent).

- [ ] Tests: defaults matrix, partial-file fill, clamps, round-trip.
- [ ] Gates green; **Commit** `M5: recording + presentation settings (F-060, F-065)`.

### Task 10: GUI recording window skeleton + meter + F-004 unlock

**Files:** Create `crates/humm-gui/src/recording.rs` (`RecordingVm`, `RecordingDeps`), `crates/humm-gui/src/views/recording.rs`, `crates/humm-gui/src/views/audio_meter.rs`; modify `app.rs` (viewport + open/focus), `views/main_window.rs` (Record enable), `shortcuts.rs` wiring, `humm.ftl`, `i18n.rs`.

Spec §2.1 layout (options group with size/task/language + Advanced… button stub → Task 12; mic combo default-preselected + change-restarts-meter; meter + Record/Stop; panels; hidden bars), §2.5 meter painting (bar math + caption + 0.95 smoothing GUI-side + ~30 ms repaint while visible), stopped-state meter-only capture via `RecordingDeps.capture`, §2.8 warning label (reads `measured_speed`), Record button/⌘R open-or-focus (single instance), Buzz strings byte-exact (`Live Recording`, `Microphone:`, `Record`/`Stop`, `Click Record to begin...`, `Average volume`, `Queue`). Record/Stop state machine only *toggles UI state* this task (session wiring = Task 11) behind a `cfg`-free seam.

- [ ] Unit tests: meter bar-activation math + smoothing/reset, warning-visibility matrix, device-list default preselection (fake capture), options-group prefill from `RecordingSettings`.
- [ ] kittest: window renders (panel placeholder text, disabled translation affordance tooltip per spec §2.1.6), Record click flips to recording-state controls (options disabled, presentation bar visible), meter paints without panic, toolbar Record now enabled + opens.
- [ ] Gates green; **Commit** `M5: recording window skeleton, audio meter, Record/⌘R unlock (F-060, F-004)`.

### Task 11: GUI session wiring — record flow, persistence, live export/upload (F-060/F-061/F-064 + Q-10 end-to-end)

**Files:** Modify `crates/humm-gui/src/recording.rs`, `views/recording.rs`, `app.rs`, `services.rs` (deps), `engine_factory.rs` (live engine path).

Record click: model ensure (inline progress + cancel, `DownloaderEnsurer` reuse), engine load, `insert_live_session`, spawn session thread, meter-only capture handoff (spec §2.1). Drain `LiveEvent`s each frame: `Text` → filter → display mode (Task 2 helpers) → panel + `append_live_segment` + live export writers + upload + presentation mirror; `Error` → Buzz dialog text + stopped state; `Stopped` → finalize (`complete_live_session`, measured-speed persist, copy bar, `AppEvent::TranscriptionsUpdated` at start AND stop). Close-while-recording defers (spec §2.1 two-phase). Sticky prefs saved on close (Buzz `_do_close` list) + immediate-save fields (mode/hide_unconfirmed/export_*) as they change.

- [ ] Unit tests: event-application matrix (Text/Error/Stopped effects on VM), export-path setup gating (folder missing disables), effective-upload-url precedence, sticky-prefs round trip.
- [ ] kittest (scripted deps end-to-end): scripted capture+engine drive Record→2 utterances→Stop; asserts panel text (append-below), a real store row (InProgress→Completed) with 2 segments, TXT export file bytes, upload TestServer bodies (clear + 2 transcripts) [network-free fake for kittest; TestServer variant in the sandbox-lifted set], row appears in TasksVm and `can_open` admits it; restart-survival: reopen store → row + segments intact (**acceptance #4 automated half**).
- [ ] Gates green; **Commit** `M5: live session end-to-end — record flow, persisted sessions, live export/upload (F-060, F-061, F-064, Q-10)`.

### Task 12: GUI advanced sheet + presentation window + copy bar (F-060/F-062/F-064/F-065 UI completion)

**Files:** Create `crates/humm-gui/src/views/recording_advanced.rs`, `crates/humm-gui/src/views/presentation.rs`; modify `recording.rs`, `views/recording.rs`, `humm.ftl`, `i18n.rs`, `docs/HUMM_SURFACE.md` (upload env + CSS file).

Advanced sheet (egui Modal, Buzz `advanced_settings_dialog.py:123-255` recording section): initial prompt; disabled LLM-translation block (tooltip); silence threshold spinbox (0–1, step 0.0005, 4 decimals); mode combo (mode-dependent row visibility: separator ↔ step+hide-unconfirmed); separator (escaped display `\n\n` ⇄ literal, unicode_escape parity incl. reject-invalid-keep-old); step spinbox 2.0–5.0/0.1/1 decimal; hide-unconfirmed checkbox; export block (enable/folder+browse/file name/type combo txt|csv/max entries 0–99) enable-gating; ✚ upload-URL field. Presentation window (spec §2.6): bar controls, theme/colors/size persistence + 100 ms debounce, deferred viewport, fullscreen + Esc, CSS-subset override, live text mirror. Copy bar (spec §2.7) with Copied!/Nothing-to-copy! timed relabels.

- [ ] Unit tests: separator escape round-trip + invalid-escape keep-old, mode-visibility matrix, CSS-subset parser (color/background-color/font-size, junk ignored), theme→color resolution, copy-label state machine (injected clock).
- [ ] kittest: sheet renders with mode-dependent rows, export block gates on the checkbox, presentation window mirrors panel text, copy click relabels.
- [ ] Gates green; **Commit** `M5: recording advanced settings, presentation window, clipboard copy (F-060, F-062, F-064, F-065)`.

### Task 13: Carried tickets (humm-m5-tickets)

- [ ] `merge_algo_from` (views/resizer.rs) delegates to core's `algo_from_env_or`/pub(crate) `override_algo` (final-review finding 6) — test pinning env-override equivalence.
- [ ] Queue-path paragraph-split unification: `options_sheet::build_specs` reads `HUMM_PARAGRAPH_SPLIT_TIME` at spec build (CLI parity); retire the HUMM_SURFACE §5 known-gap paragraph; test threading an explicit value (no env mutation — Global-Constraint-9 convention).
- [ ] Fractional-fps spot-check: extend the video contract test with a 29.97 fps generated fixture (`-r 30000/1001`), assert monotonic frames + fps parse (`#[ignore]`, run sandbox-lifted, record result).
- [ ] F-045 time-boxed retry (≤ 1 h): anaconda python3 against Buzz's own deps per `validation/m2-differential/README.md`; whatever the outcome, update that README + the milestone-report blocker list.
- [ ] Gates green; **Commit** `M5: carried tickets — resizer delegation, paragraph-split unification, fps spot-check, F-045 attempt (F-086, F-023, F-083, F-045)`.

### Task 14: Acceptance, benches, gui-manual v3, docs sweep, milestone report

- [ ] **Acceptance runs (sandbox-lifted, windowed where stated):** (1) live realtime with `small` — windowed session against real mic or loopback fixture, report measured ratio (spec §2.8; must sustain ≥ 1× RT); (4) closed-window/restart survival windowed check; (5) meter repaint smoothness (windowed, note frame cadence); merge/export/others are Task 2/3/11 tests re-confirmed green.
- [ ] `cargo bench` — no transcription-path regression vs `docs/benchmarks/M4.md` baselines; record `docs/benchmarks/M5.md` (+ live-loop measurement numbers).
- [ ] `docs/gui-manual/M5.md` (v3 checklist): kittest-covered items marked, windowed items executed via smoke hooks where possible (`HUMM_SMOKE_*` precedent), pointer-only items listed for the human.
- [ ] Docs sweep: PARITY_MATRIX final statuses (F-060/061/062/064/065 + F-004 note; F-063 stays not-started with M6 note); **ADR-021** (live-session architecture: in-process streaming loop, adapter-side 16 k, raw-segment persistence + stale-flip-complete, measured-speed warning) in DECISIONS.md; UX_SPEC §6 live-default correction + J-5 as-built notes; ARCHITECTURE §2 capture-trait as-built + §3 row confirmation; HUMM_SURFACE (upload env, CSS file, new settings keys, smoke hooks); OPEN_QUESTIONS Q-10 closed-as-shipped note; ledger entries per spec §9.
- [ ] Milestone report (final message): matrix status, benchmark numbers, deviations w/ ADR refs, analysis corrections, remaining environment-blocked debt.
- [ ] **Commit** `M5: acceptance + benches + gui-manual v3 + docs sweep (F-060..F-065)`.
