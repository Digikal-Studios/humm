# M7 — Completeness: preferences, folder watch, native "plugins", locales — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the full Preferences window (General complete, Models full, Shortcuts editor + conflict ✚, Folder Watch, Post-processing ✚, Privacy), the notify-based folder watcher with source disposition and the J-4 pill, the four ADR-010 native post-processing features (F-111/F-113/F-115/F-116) over new `check_skip`/`on_complete` seams, locale infrastructure + de/es/zh catalogs (F-008), and the F-061 live-API closure with the ADR-021 state-reuse fix.

**Architecture:** Pure feature logic lands in `humm_core::postproc` + `humm_core::folderwatch`; the queue gains a `Terminal::Skipped` arm, a post-completion hook chain on the runner thread, and two additive events. The only new dependency is `notify` (GUI binary only — folder watch is a GUI feature, as in Buzz). The four "plugins" are settings-configured native features; the live-API arm is a new `EngineFn` closure over M6's `OpenAiEngine` machinery.

**Tech Stack:** notify 8.x (humm-gui), existing zip/reqwest/fluent/keyring. No tokio, no new core OS boundaries.

**The governing spec:** `docs/superpowers/specs/2026-07-25-m7-completeness-design.md` — every task's implementer MUST read it first (at minimum §0–§2 + the §§ their task names), plus the Buzz sources their task cites. Implement from Buzz source, not from this plan's paraphrase, wherever the two could disagree.

## Global Constraints

1. Every commit passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — run with `export CARGO_HOME="$PWD/.cargo-home"` (worktree-local, already seeded and `cargo fetch`-completed). Commit messages: `M7: <what> (F-###[, F-###])`.
2. Sandbox: GPU/Metal (SEGFAULT — never bench/e2e sandboxed), TCP binds, CoreAudio, and writes outside the worktree are blocked. The M6 TCP-bind-blocked baseline is **50 humm-core + 1 humm-cli** tests; every new TCP-binding test states the new running count in its task's commit message — growth documented, never silent.
3. Network-*dependent* tests (real endpoints) are `#[ignore]`/env-gated; local mocks carry the sandboxed load.
4. Key hygiene (M6 rules stand): no API key in `settings.toml`, DB, logs, argv, or any `Serialize` type; key-carrying structs get redacting `Debug`. Applies to the new `AiSummaryApiKey` exactly as to `OpenAiApiKey`.
5. All user-visible strings go through `assets/i18n/en/humm.ftl` with stable keys; Buzz-byte-exact strings are called out per task. **New in M7: every en key added must land in de/es/zh too once Task 12 exists — tasks after 12 add all four catalogs together** (the per-locale completeness test enforces this).
6. `humm-gui` lib tests run in parallel: never `std::env::set_var` in a test — thread values explicitly.
7. kittest cannot bind TCP sandboxed and cannot drive real dialogs/child viewports — use `Harness::new_ui_state` fixtures for tab views (kittest_prefs.rs pattern) and scripted seams elsewhere.
8. New public core items need doc comments (`#![warn(missing_docs)]`).
9. Python-semantics ports stay exact: `int()` truncation, `round()` half-even, regexes verbatim.
10. Post-hooks are **off by default**; the transcription bench path must be diff-innocent (verified in Task 21).

## File structure (created/modified across the milestone)

```
crates/humm-core/src/
  settings.rs             # +[shortcuts] map, +UiSettings, +models.force_cpu,
                          #  +file_transcriber.default_export_template, +FolderWatchSettings,
                          #  +PostprocSettings{4 sub-structs}, +recording.backend
  secrets.rs              # +SecretKey::AiSummaryApiKey
  domain.rs               # +OutputSpec.source_disposition, SourceDisposition
  queue.rs                # +Terminal::Skipped, TaskEventKind::{Skipped,PostProcessed},
                          #  RunnerDeps.postproc, check_skip + on_complete dispatch
  boundary.rs             # +TaskStore::mark_skipped
  pipeline.rs             # post-export source disposition; drain poll-join
  postproc/mod.rs         # PostprocConfig snapshot + PostprocConfigSource trait + hook chain
  postproc/skip.rs        # F-115 (SRT/VTT/TXT parsers, stem rules, DB check)
  postproc/summary.rs     # F-111
  postproc/docx.rs        # F-113 hook (path/folder resolution)
  postproc/auto_resize.rs # F-116
  export/docx.rs          # write_docx (OOXML zip; export.rs becomes export/mod.rs OR docx.rs sibling — Task 6 decides, prefer export/ dir)
  folderwatch.rs          # SeenSet, scan + skip rules, mirrored_output_dir, SUPPORTED_EXTENSIONS (moves here from humm-gui)
  openai/chat.rs          # extracted chat::complete
  openai/translate.rs     # uses chat::complete; fallback-client timeout
  openai/live.rs          # F-061: per-chunk WAV transcriber (LiveApiTranscriber)
  wav.rs                  # in-memory 16-bit PCM WAV writer (tiny)
crates/humm-store/src/store.rs   # mark_skipped, find_completed_by_file_name, has_task_for_file
crates/humm-whisper/src/engine.rs # live session state reuse (ADR-021 ticket)
crates/humm-cli/src/run.rs        # Skipped/PostProcessed arms (human + --json + exit codes), template setting
crates/humm-gui/src/
  i18n.rs + assets/i18n/{de,es,zh-CN}/humm.ftl   # registry, set_locale, negotiation
  shortcuts.rs            # description_key(), parse_key growth, conflicts()
  views/prefs_general.rs  # F-100 rows
  views/prefs_shortcuts.rs   # F-005 editor tab (new)
  views/prefs_folder_watch.rs# F-030 tab (new)
  views/prefs_postproc.rs    # ✚ tab (new)
  views/preferences.rs    # 6-tab strip
  models_ui.rs            # speed badge, lumii gating
  views/options_sheet.rs  # pub(crate) promotions; selectable_model_sizes(include_lumii)
  folder_watch.rs         # FolderWatchService (notify) (new)
  views/main_window.rs    # pill chip
  views/recording.rs + recording.rs + live_session.rs  # backend selector + API arm wiring
  app.rs                  # shortcuts rebuild, font-size apply, locale init, watcher lifecycle,
                          #  PostProcessed refresh
docs/… (HUMM_SURFACE, PARITY_MATRIX, UX_SPEC, DECISIONS ADR-021 addendum, FEATURE_INVENTORY
        F-030 method fix, gui-manual/M7.md, benchmarks/M7.md)
```

---

### Task 1: Settings + secrets groundwork

**Files:** Modify `crates/humm-core/src/settings.rs`, `crates/humm-core/src/secrets.rs`.
**Spec:** §1. **Produces (later tasks consume):**
`Settings.shortcuts: HashMap<String, String>` (serde section `[shortcuts]`, default empty);
`UiSettings { locale: Option<String>, font_size: Option<u32> }` at `settings.ui` (font_size clamped `8..=32` on load — clamp method, called from `Settings::load`);
`ModelSettings.force_cpu: bool` (default false);
`FileTranscriberSettings.default_export_template: Option<String>`;
`FolderWatchSettings { enabled: bool, input_dir: Option<PathBuf>, output_dir: Option<PathBuf>, delete_processed_files: bool, options: FileTranscriberSettings }` at `settings.folder_watch`;
`PostprocSettings { ai_summary: AiSummarySettings, export_docx: ExportDocxSettings, skip_already_transcribed: SkipSettings, transcript_resizer: AutoResizeSettings }` at `settings.postproc` — field names/defaults exactly per spec §1 table (Buzz defaults; `prompt` default = Buzz `DEFAULT_PROMPT` verbatim, `punctuation` default = Buzz `DEFAULT_PUNCTUATION` verbatim from `transcript_resizer/plugin.py:37`);
`RecordingSettings.backend: Option<String>`;
`SecretKey::AiSummaryApiKey` with `entry_name() == "AI Summary API key"`.

- [ ] Failing tests first (settings test mod): defaults-per-section tests mirroring the existing shape; `font_size_clamped_on_load` (write `font_size = 99` TOML literal → 32; `4` → 8); `pre_m7_settings_toml_parses_with_all_new_sections_defaulted` (paste an M6-era TOML literal); round-trip test for a fully-populated `[postproc.*]`/`[folder_watch]` file; secrets test `ai_summary_entry_name`.
- [ ] Implement; existing tests stay green.
- [ ] Gates; commit `M7: settings + secrets groundwork (F-100, F-030, F-111, F-115, F-116, F-005, F-008)`.

### Task 2: Skipped/PostProcessed vocabulary + store methods

**Files:** Modify `crates/humm-core/src/queue.rs` (events only), `crates/humm-core/src/boundary.rs`, `crates/humm-store/src/store.rs`, `crates/humm-cli/src/run.rs`, `docs/HUMM_SURFACE.md` §3.
**Spec:** §2.3, §11. **Buzz:** `db/service/transcription_service.py:62-73`, `widgets/main_window.py:435-443`.
**Produces:** `TaskEventKind::Skipped`; `TaskEventKind::PostProcessed { notes_updated: bool, segments_replaced: bool, written: Vec<PathBuf>, errors: Vec<String> }`; `TaskStore::mark_skipped(&self, id, segments: &[Segment]) -> Result<(), StoreError>` (+ SqliteStore impl: status `skipped`, `time_ended`, segments inserted with `""` translations, one txn); `SqliteStore::find_completed_by_file_name(&self, name: &str) -> Result<Option<TranscriptionId>, StoreError>` (basename match on `file` tail, `status='completed'`, newest `time_queued` first); `SqliteStore::has_task_for_file(&self, path: &Path) -> Result<bool, StoreError>` (exact full-path match, any status).

- [ ] Failing store tests: `mark_skipped_sets_status_time_and_segments`, `find_completed_by_file_name_matches_basename_newest_first` (two rows same basename different dirs), `find_completed_ignores_non_completed`, `has_task_for_file_exact_path_only`.
- [ ] Implement store + trait (+ every `TaskStore` test double gains the method — compile-driven).
- [ ] CLI: the exhaustive matches (`ProgressLine::on_event` run.rs:708, `emit_json` run.rs:902, `json_for` helper) gain arms — human: `skipped (already transcribed)` on the task line; nothing for `PostProcessed` unless `errors` non-empty (one stderr `warning: post-processing: <err>` each); `--json`: `{"event":"skipped",…}` / `{"event":"post_processed",…}` per spec §11; exit-code: skipped counts as success. Failing test first: extend the `json_for` pin tests with both events; human-mode snapshot for skipped.
- [ ] HUMM_SURFACE §3: document both events (additive; `post_processed` follows `completed`).
- [ ] Gates; commit `M7: Skipped/PostProcessed event + store vocabulary (F-115, F-110)`.

### Task 3: Pipeline — source disposition + drain poll-join

**Files:** Modify `crates/humm-core/src/domain.rs`, `crates/humm-core/src/pipeline.rs`, `crates/humm-core/src/openai/translate.rs` (stop() only).
**Spec:** §3.4, §14 (poll-join ticket). **Buzz:** `transcriber/file_transcriber.py:55-73,155-170`.
**Produces:** `SourceDisposition { Delete, MoveToOutput }`; `OutputSpec.source_disposition: Option<SourceDisposition>` (`#[serde(default)]`); pipeline post-export disposition execution (after the export loop, pipeline.rs:293-337 region): `Delete` → `remove_file(input)`; `MoveToOutput` → rename to `<export dir>/<basename>` with copy+delete cross-device fallback; failures `log::warn!`, never fail the job. `TranslatorHandle::stop()` becomes a poll-join: waiter thread performs the join, caller loops `recv_timeout(100ms)` checking a passed `Option<&CancelToken>` — signature becomes `stop(self, cancel: Option<&CancelToken>) -> bool` (false = abandoned on cancel, worker detached); `pipeline.rs`'s drain passes the job token and the honest-bound comment is rewritten (cancel-ack now ≤ ~100 ms mid-join).

- [ ] Failing tests: `old_options_json_without_disposition_deserializes` (M6 JSON literal); pipeline-level with temp files + scripted engine: `folder_watch_delete_removes_source_after_export`, `folder_watch_move_relocates_source`, `disposition_failure_is_nonfatal` (move target dir removed pre-run); translate: `stop_with_canceled_token_returns_within_bound` (blocked worker via mock, assert < 1 s), existing `cancel_during_translation_drain_yields_canceled` updated to the new signature.
- [ ] Implement; ADR-022's drain-exception paragraph gets a one-line as-built note (Task 21 does the full doc sweep; here just the `pipeline.rs` comment).
- [ ] Gates; commit `M7: source disposition in pipeline + translator drain poll-join (F-030, F-031)`.

### Task 4: `postproc` module + check_skip seam + F-115

**Files:** Create `crates/humm-core/src/postproc/mod.rs`, `crates/humm-core/src/postproc/skip.rs`; modify `crates/humm-core/src/queue.rs`, `crates/humm-cli/src/run.rs` (config source), `crates/humm-gui/src/services.rs` (config source).
**Spec:** §2.1–§2.4, §4. **Buzz:** `plugins/skip_already_transcribed/plugin.py` (whole file), `plugins/manager.py:88-105`, `file_transcriber_queue_worker.py:208-230`.
**Produces:**
`PostprocConfig` — plain-data snapshot: `skip: SkipSettings`, `summary: AiSummarySettings + resolved key: Option<String>` (redacting `Debug`), `docx: ExportDocxSettings`, `resize: AutoResizeSettings`, `paragraph_split: Millis`;
`trait PostprocConfigSource: Send + Sync { fn snapshot(&self) -> PostprocConfig; }`;
`RunnerDeps.postproc: Option<Arc<dyn PostprocConfigSource>>` (None = all off);
`Terminal::Skipped(Vec<Segment>)` + `run_job` hook after input resolution (queue.rs:982): `postproc::skip::check(&cfg.skip, input_path, output_dir, store) -> Option<Vec<Segment>>`; `run_task` arm: `mark_skipped` + emit `Skipped`;
`skip.rs` internals per spec §4: `stem_for(path)` (`_speech` strip), result-file scan (parent + output dir, TXT→SRT→VTT order, prefix-glob quirk preserved), `parse_srt`/`parse_vtt`/`parse_txt` with Buzz's regexes verbatim, DB check via `find_completed_by_file_name`.
GUI + CLI both wire a real config source (GUI: reads `Arc<Mutex<Settings>>` + `SecretStore`; CLI: loaded settings + keyring unless `HUMM_DISABLE_KEYRING`).

- [ ] Failing unit tests: parser fixtures (well-formed SRT/VTT/TXT, malformed blocks skipped, empty TXT = miss, `,`→`.` SRT ms), `stem_strips_speech_suffix`, `glob_prefix_quirk_matches_longer_stems` (`audio2.srt` hits for stem `audio`), format-order test (planted TXT beats SRT), DB-hit test (temp store), disabled-flags tests.
- [ ] Failing queue test: scripted engine + planted sibling `.srt` → engine never runs, row `Skipped`, segments persisted, `Skipped` event emitted; and with `postproc: None` → engine runs (regression).
- [ ] Implement; gates; commit `M7: check_skip seam + skip_already_transcribed native (F-115, F-110)`.

### Task 5: openai chat extraction + F-111 ai_summary

**Files:** Create `crates/humm-core/src/openai/chat.rs`, `crates/humm-core/src/postproc/summary.rs`; modify `crates/humm-core/src/openai/translate.rs`, `crates/humm-core/src/openai/mod.rs`.
**Spec:** §6. **Buzz:** `plugins/ai_summary/plugin.py` (whole file).
**Produces:** `openai::chat::complete(client: &reqwest::blocking::Client, auth: &OpenAiAuth, model: &str, system: &str, user: &str) -> Result<String, ApiFailure>` (the extracted `post_chat_completion` body, behavior-identical — translator differential fixtures must stay green untouched); translator fallback client gains the 60 s timeout (carried one-liner); `postproc::summary::run(cfg: &PostprocConfig, outcome_segments: &[Segment], task_input: &Path, store: &dyn TaskStore-ish /* set_task_notes via a narrow trait or SqliteStore — match Task 4's store access shape */, cancel: &CancelToken) -> SummaryResult { notes_updated: bool, file_written: Option<PathBuf>, error: Option<String> }` — gates (no key / empty transcript → skip logged), 120 s client, `cancelable`-wrapped, `\n`-joined non-empty texts, notes overwrite via `set_task_notes`, `{stem}.summary.txt` placement per spec.
- [ ] Failing tests: chat extraction is covered by existing translate suite staying green; summary unit tests (gates, transcript assembly incl. empty-text segments skipped, notes write to temp store, file placement default-vs-folder, error carried not thrown); mock-server tests for request shape (`messages[0]=system prompt, messages[1]=transcript`) + non-2xx + malformed-body error paths (TCP-bound: state new count in commit message); env-gated `real_summary_api` behind `HUMM_LLM_TEST_BASE/_KEY/_MODEL` (shared harness with F-031's — factor the env-reading into a small shared test helper if trivial, else duplicate honestly).
- [ ] Implement; key-hygiene check: `PostprocConfig` `Debug` redacts; no `Serialize` on key-carrying types.
- [ ] Gates; commit `M7: chat extraction + ai_summary native (F-111)`.

### Task 6: DOCX writer + F-113 hook

**Files:** Create `crates/humm-core/src/export/docx.rs` (restructure `export.rs` → `export/mod.rs` — pure move, no signature changes), `crates/humm-core/src/postproc/docx.rs`.
**Spec:** §7. **Buzz:** `plugins/export_docx/plugin.py` (whole file); `transcriber/file_transcriber.py` `to_timestamp` (pin the separator).
**Produces:** `export::docx::write_docx(w: impl Write + Seek, title: &str, segments: &[Segment], include_timestamps: bool, paragraph_split: Millis) -> io::Result<()>` — OOXML templates verbatim from plugin.py:35-61 (`[Content_Types].xml`, `_rels/.rels`, `word/document.xml`), heading + bold-stamp + preserve-space runs, XML escaping equal to `saxutils.escape` (`& < >`; quotes escaped in attributes only — mirror exactly what escape() does on text nodes), deflate zip in that entry order; `postproc::docx::run(cfg, transcription_id, task_input, store) -> DocxResult { written: Option<PathBuf>, error: Option<String> }` — re-reads store segments, empty → skip, `{stem}.docx` per spec placement.
- [ ] Failing snapshot tests (insta): `document.xml` paragraph mode (multi-gap fixture), timestamps mode, CJK + `&<>"'` escaping fixture, zip entry-name/order test; unit: paragraph grouping vs `paragraph_split` boundary (gap == threshold starts new paragraph — Buzz `>=`), stamp format pinned against `to_timestamp(start,'.')`.
- [ ] Implement; hook e2e test through a temp store.
- [ ] Gates; commit `M7: DOCX export native + snapshots (F-113)`.

### Task 7: F-116 auto-resize + on_complete dispatch chain

**Files:** Create `crates/humm-core/src/postproc/auto_resize.rs`; modify `crates/humm-core/src/queue.rs`, `crates/humm-core/src/regroup/words.rs` (only if reconstruction verification demands), `crates/humm-gui/src/app.rs` + `viewmodel.rs` (PostProcessed refresh).
**Spec:** §2.1–§2.3, §5. **Buzz:** `plugins/transcript_resizer/plugin.py` (whole file), `plugins/manager.py:136-164`, `widgets/main_window.py:453-473`.
**Produces:** `postproc::auto_resize::run(cfg, transcription_id, options_word_timestamps: bool, language: Option<&str>, store) -> ResizeResult { replaced: bool, error: Option<String> }` — gates per spec §5, algo via `regroup::default_algo` + `regroup::algo_from_env_or`, `regroup::regroup_segments`, `store.replace_segments`; **verify** `words_from_segments` against plugin.py:178-200's reconstruction (SENTENCE_END regex, non-space separator set) and extend `words.rs` + a regenerated differential fixture only if they differ;
queue.rs `run_task` Completed arm: after `mark_completed` + `emit(Completed)`, when any post hook enabled → chain **resize → summary → docx**, cancel-token check between hooks, one `PostProcessed` event with aggregated fields; each hook error contained into `errors`;
GUI: on `PostProcessed` → refresh row (notes column) + open viewer reloads segments when `segments_replaced`.
- [ ] Failing tests: config→algo-string matrix pinned to hand-traced Buzz strings (all 8 checkbox combos incl. `mg=0.2++42+1_sp=…_sl=42` composite and empty → skip), word-timestamps gate, store e2e (word-timestamped rows → replaced rows), queue-level chain-order test (scripted store observing `replace_segments` before docx's `get_segments`), `post_processed_event_aggregates` (resize replaced + docx written + summary error), cancel-between-hooks test, GUI viewmodel test (`PostProcessed` with `segments_replaced` refreshes).
- [ ] Implement; gates; commit `M7: auto-resizer native + on_complete chain (F-116, F-110, F-111, F-113)`.

### Task 8: Folder watch core logic

**Files:** Create `crates/humm-core/src/folderwatch.rs`; modify `crates/humm-gui/src/import.rs` (re-export from core).
**Spec:** §3.1. **Buzz:** `widgets/transcription_task_folder_watcher.py` (whole file).
**Produces:** `SUPPORTED_EXTENSIONS: [&str; 13]` (moved from humm-gui; import.rs re-exports); `SeenSet` (`record(path, size, mtime)` / semantics of Buzz `paths_emitted`); `Candidate { path: PathBuf, relative_dir: PathBuf }`; `scan(input_dir: &Path, seen: &SeenSet, known: &dyn Fn(&Path) -> bool) -> Vec<Candidate>` implementing the six skip rules verbatim (hidden dirs pruned during walk, hidden files, extension, double-extension temp files, `_speech.mp3` substring, known-task, seen-unchanged); `mirrored_output_dir(output_root: &Path, relative_dir: &Path) -> PathBuf`.
- [ ] Failing tests (temp dirs, no watcher): one per skip rule, `double_extension_ogg_wav_skipped`, `speech_mp3_substring_skipped`, `hidden_directory_subtree_pruned`, `seen_same_size_mtime_not_reemitted`, `seen_size_change_reemits`, `known_task_path_skipped`, `mirroring_relative_math` (nested subdir → mirrored path; root file → output root), deterministic ordering (sorted walk).
- [ ] Implement; gates; commit `M7: folder watch core scan/skip logic (F-030)`.

### Task 9: FolderWatchService (notify) + pill + integration test

**Files:** Create `crates/humm-gui/src/folder_watch.rs`; modify `crates/humm-gui/Cargo.toml` (+`notify = "8"`, workspace dep), workspace `Cargo.toml`, `crates/humm-gui/src/app.rs`, `crates/humm-gui/src/views/main_window.rs`, `assets/i18n/en/humm.ftl`.
**Spec:** §3.2, §3.3, §3.5. **Buzz:** `widgets/transcription_task_folder_watcher.py`, `widgets/main_window.py:167-183,500-507`.
**Produces:** `FolderWatchService::start(cfg: FolderWatchSnapshot, queue: QueueHandle, store: Arc<SqliteStore>, defaults: SpecDefaults) -> FolderWatchService` — notify `RecommendedWatcher` recursive on input dir; worker thread: drain-then-rescan; per candidate `create_dir_all(mirrored)` + TaskSpec from `folder_watch.options` (reuse the options-sheet spec-build path — extract `spec_from_transcriber_settings(&FileTranscriberSettings, input, output_dir, disposition) -> TaskSpec` into a shared location if `build_specs` isn't directly callable) + `enqueue(id, spec, Source::FolderWatch)`; disposition Delete/MoveToOutput from `delete_processed_files`; initial scan on start; `pause(bool)`/`is_paused()`; `stop()` (drop watcher + join worker); app lifecycle: build when enabled at startup, rebuild on `[folder_watch]` snapshot change after prefs edits, teardown on disable/shutdown. Pill: toolbar chip after update-notice (`main_window.rs:503-504`): `Watching {name}` / dimmed `Paused` + toggle; ftl keys `folderwatch-pill-watching`, `folderwatch-pill-paused`, `folderwatch-pill-pause-tip`, `folderwatch-pill-resume-tip`.
- [ ] Failing integration test (humm-gui tests/, real notify over TempDir, TestDeps scripted engine): drop fixture media file → wait_for `Source::FolderWatch` row Completed → mirrored export exists at `out/<subdir>/…`, source **moved** to output; second case: delete variant; third: paused service ignores a drop until resumed.
- [ ] Unit tests: snapshot-change rebuild decision fn; pill visibility fn.
- [ ] Implement; gates; commit `M7: notify folder-watch service + J-4 pill + integration test (F-030)`.

### Task 10: Locale infrastructure (F-008a)

**Files:** Modify `crates/humm-gui/src/i18n.rs`, `crates/humm-gui/src/app.rs`, `crates/humm-gui/src/services.rs` (locale helper promotion), `crates/humm-gui/src/main.rs` (startup init).
**Spec:** §9.1, §9.3. **Buzz:** `locale.py`, `widgets/preferences_dialog/general_preferences_widget.py:34-50,297-303`.
**Produces:** `i18n::AVAILABLE_LOCALES: &[(&str /* buzz code */, &str /* langid */)]` (starts `[("en_US","en-US")]`, Task 12 appends); `i18n::set_locale(buzz_code: &str)` (global generation bump; per-thread bundles lazily rebuild — works across deferred-viewport render threads); `i18n::negotiate(requested: Option<&str>) -> &'static str` (exact → language-only → `en_US`, Buzz fallback chain); `i18n::display_name(code) -> String` (ftl-keyed names per Buzz's `ui_locales` dict); startup: `negotiate(settings.ui.locale.as_deref().or(system))` where system comes from the promoted `locale_from_env_vars` helper.
- [ ] Failing tests: `negotiate_exact_then_language_then_en` (e.g. request `de_AT` with only `de_DE` embedded → `de_DE`? No — Buzz language-only match means `de_AT`→`de` file; with our codes: request maps by language prefix → first embedded locale sharing it; pin that rule), `set_locale_rebuilds_bundle_on_same_thread`, `set_locale_visible_from_second_thread`, `unknown_locale_falls_back_en`.
- [ ] Implement (en-only registry — behavior unchanged until catalogs land); gates; commit `M7: locale negotiation + live switching infrastructure (F-008)`.

### Task 11: General tab completion (F-100)

**Files:** Modify `crates/humm-gui/src/views/prefs_general.rs`, `crates/humm-gui/src/app.rs` (font apply + engine force_cpu read), `crates/humm-gui/src/views/options_sheet.rs` (`build_specs` template), `crates/humm-cli/src/run.rs` (template), `crates/humm-gui/src/views/export_menu.rs` (template), `assets/i18n/en/humm.ftl`.
**Spec:** §8.1. **Buzz:** `widgets/preferences_dialog/general_preferences_widget.py` (whole file — row order, ranges, tooltips, checkbox labels byte-faithful in en).
**Produces:** General tab rows in Buzz order: UI-language combo (lists `AVAILABLE_LOCALES`, `display_name`, on change `set_locale` + persist `ui.locale` — live, no restart hint); Font-size `DragValue` 8–32 (on change: persist + `apply_font_size(ctx, Option<u32>)` — scales every `TextStyle` size proportionally from a captured pristine default set; `None` restores); existing OpenAI block unchanged in place; Default-export-file-name field (commit-on-focus-out → `file_transcriber.default_export_template`, hint = `DEFAULT_EXPORT_TEMPLATE`); Disable-GPU checkbox (`models.force_cpu`, Buzz tooltip text). Template consumers: options-sheet `build_specs`, CLI `run.rs` OutputSpec (settings-read), viewer export menu — all pass `settings.file_transcriber.default_export_template` into `file_name_template`. Engine construction (GUI `use_gpu` site + CLI): `!(env HUMM_FORCE_CPU ∨ settings.models.force_cpu)`.
- [ ] Failing tests: kittest fixture tab tests (`language_combo_lists_available_locales_and_persists`, `font_size_persists_and_clamps`, `export_template_field_persists_empty_as_none`, `force_cpu_checkbox_persists`); pure-fn test `apply_font_size_scales_all_text_styles_and_restores`; cli-int `default_export_template_setting_names_output_file`; unit `use_gpu_resolution_env_wins`.
- [ ] Implement; gates; commit `M7: General tab completion — language, font size, template, GPU toggle (F-100)`.

### Task 12: Starter locale catalogs de/es/zh (F-008b)

**Files:** Create `crates/humm-gui/assets/i18n/de/humm.ftl`, `es/humm.ftl`, `zh-CN/humm.ftl`; modify `crates/humm-gui/src/i18n.rs` (registry entries `de_DE`/`es_ES`/`zh_CN`), test mod.
**Spec:** §9.2. **Reference glossary:** `buzz/locale/{de_DE,es_ES,zh_CN}/LC_MESSAGES/buzz.po` — use Buzz's translation for any humm string that matches/paraphrases a Buzz msgid; translate the rest faithfully (formal register de, neutral es, simplified zh); keep `{$name}` placeables intact; keep product name `humm` untranslated.
**Produces:** three complete catalogs (every key in the current en catalog — run against the *post-Task-11* key set); per-locale completeness test `every_locale_has_exactly_the_en_key_set` (both directions, non-empty resolution) — the enforcement for Global Constraint 5.
- [ ] Failing test first (registry lists 4 locales; completeness test fails on empty catalogs).
- [ ] Write the three catalogs; verify zh renders without placeable breakage (`tr_args` test per locale for `recording-realtime-warning`).
- [ ] Gates; commit `M7: de/es/zh locale catalogs + completeness tests (F-008)`.

### Task 13: Models tab full (F-044 + J-9 ✚)

**Files:** Modify `crates/humm-gui/src/models_ui.rs`, `crates/humm-gui/src/views/options_sheet.rs` (`selectable_model_sizes(include_custom, include_lumii)`), call sites (`views/recording.rs`, models_ui), ftl catalogs (×4).
**Spec:** §8.2. **Buzz:** `widgets/preferences_dialog/models_preferences_widget.py:207-230` (lumii gate), UX_SPEC J-9.
**Produces:** speed badge — `ModelRow` render shows `{ratio:.1}× realtime` (+ tooltip key `models-speed-badge-tip`) when `settings.recording.measured_speed` has the size's wire key; `selectable_model_sizes` gains `include_lumii: bool` (from effective locale == `lv_LV`), threaded through models tab + options sheet + recording combo.
- [ ] Failing tests: `speed_badge_shown_when_measured` / `absent_without_entry` (kittest fixture with seeded settings map); `lumii_listed_only_under_lv_locale` (both sheet fn unit + models refresh), existing 17 models_ui tests stay green.
- [ ] Implement; gates; commit `M7: Models tab full — speed badge + lumii gating (F-044)`.

### Task 14: Shortcuts editor tab (F-005 + conflict ✚)

**Files:** Create `crates/humm-gui/src/views/prefs_shortcuts.rs`; modify `crates/humm-gui/src/shortcuts.rs`, `crates/humm-gui/src/views/preferences.rs` (tab enum + strip entry — done here for this tab), `crates/humm-gui/src/app.rs` (rebuild-on-generation), ftl ×4.
**Spec:** §8.3. **Buzz:** `settings/shortcut.py` (descriptions), `settings/shortcuts.py` (get/set/clear), `widgets/sequence_edit.py`, `widgets/preferences_dialog/shortcuts_editor_preferences_widget.py`.
**Produces:** `ShortcutAction::description_key() -> &'static str` (+20 ftl keys, en text = Buzz descriptions verbatim); `shortcuts::serialize_shortcut(&KeyboardShortcut) -> String` (round-trips through `parse_sequence`); `parse_key` grows digits `0-9` (needed for capture completeness; round-trip tested); `shortcuts::conflicts(map: &ShortcutMap) -> Vec<(ShortcutAction, ShortcutAction)>`; `ShortcutsTabState { capturing: Option<ShortcutAction> }` + `prefs_shortcuts::ui(...)`: rows per `ALL` (description + current sequence via `format_shortcut` + capture-on-click field showing `prefs-shortcuts-press` while capturing + per-row clear-to-default), capture ignores modifier-only keys, `Esc` cancels capture; write-through: set/remove `settings.shortcuts[key]` (remove when == default), save, bump `shortcuts_generation: Arc<AtomicUsize>`; Reset-to-Defaults clears map; conflict warning icon + `Conflicts with {other}` caption per colliding row (non-blocking). `HummApp`: seeds `ShortcutMap::from_settings(&settings.shortcuts)` at startup (replacing the empty map, app.rs:423) and rebuilds the `Arc` when the generation moves.
- [ ] Failing tests: unit `serialize_parse_round_trip_for_all_defaults`, `digits_parse`, `conflicts_detects_pairwise_and_ignores_self`, `override_map_minimality` (set-to-default removes entry); kittest fixture: capture flow (click field → `key_press` with modifiers → sequence persisted + map rebuilt), reset-to-defaults clears + rows show defaults, conflict caption appears when two actions share a sequence; app-level: `startup_reads_shortcut_overrides_from_settings` (settings literal → map has override).
- [ ] Implement; gates; commit `M7: Shortcuts editor tab + conflict detection + live re-apply (F-005)`.

### Task 15: Folder Watch tab (F-030c)

**Files:** Create `crates/humm-gui/src/views/prefs_folder_watch.rs`; modify `views/preferences.rs` (tab), `views/options_sheet.rs` (`pub(crate)` promotions: `render_export_checkboxes` — refactored to take the fields it needs rather than `&mut SheetState` if straightforward, else a narrow wrapper — `render_backend_combo`, `render_api_key_row`, `render_llm_translation_block`), `app.rs` (watcher rebuild signal), ftl ×4.
**Spec:** §8.4, §3.2 (rebuild-on-commit). **Buzz:** `widgets/preferences_dialog/folder_watch_preferences_widget.py` (whole file).
**Produces:** `FolderWatchTabState` (path buffers + embedded `SheetState` seeded from `folder_watch.options`); tab rows: enable checkbox / input-folder field + Browse (`FileDialogs` seam) / output-folder + Browse / delete-processed checkbox + disposition caption (`prefs-fw-disposition-note`: completed sources are moved to the output folder unless delete is checked) / embedded options form via the shared renderers (`add_enabled_ui` off the enable flag, Buzz `_set_settings_enabled`); commits write `settings.folder_watch.*` (paths commit-on-focus-out; `SheetState.write_to`-style sync for options) + set a `folder_watch_changed` flag the app consumes to rebuild the service (Task 9's lifecycle).
- [ ] Failing kittest fixture tests: enable-gates-fields, path commit persists, delete checkbox persists, embedded model/task/format edits persist into `folder_watch.options`, browse-button uses dialog seam.
- [ ] Implement; gates; commit `M7: Folder Watch preferences tab (F-030, F-101)`.

### Task 16: Post-processing tab ✚ + F-101 assembly

**Files:** Create `crates/humm-gui/src/views/prefs_postproc.rs`; modify `views/preferences.rs` (final 6-tab strip order General · Models · Shortcuts · Folder Watch · Post-processing · Privacy), ftl ×4.
**Spec:** §8.5, §8.6. **Buzz:** the four plugins' `config_fields` declarations (labels/descriptions → en ftl, byte-faithful where they're user-facing copy).
**Produces:** `PostprocTabState { summary_key_buffer, key_visible, … }` (redacting `Debug`); four collapsible sections (skip → resizer → summary → docx — pre-hook first, then execution order) each with enable + its fields (summary key via `secret_field::masked_field_row` → `SecretKey::AiSummaryApiKey`; TEXTAREA prompt = multiline `TextEdit`; folders = path field + Browse; TEXT numerics stay strings, Buzz-faithful); header caption `prefs-postproc-header` (hooks run after each transcription, never fail it).
- [ ] Failing kittest fixture tests: each section's enable + one representative field persists; summary key writes keyring double not settings; `Debug` redaction unit test; tab-strip test: all six tabs selectable (assembly = F-101).
- [ ] Implement; gates; commit `M7: Post-processing tab + six-tab assembly (F-110, F-101)`.

### Task 17: Live-API chunk transcriber (F-061a, core)

**Files:** Create `crates/humm-core/src/wav.rs`, `crates/humm-core/src/openai/live.rs`; modify `crates/humm-core/src/openai/mod.rs`, `crates/humm-core/src/settings.rs` (nothing new — `recording.backend` from Task 1).
**Spec:** §10. **Buzz:** `transcriber/recording_transcriber.py:220-231` (re-read the whole surrounding method for the exact request: model, language, prompt threading).
**Produces:** `wav::write_wav_16k_mono_i16(pcm: &[f32], w: &mut impl Write) -> io::Result<()>` (44-byte header + s16le samples, f32 clamped [-1,1] scaled); `openai::live::LiveApiTranscriber::new(cfg: OpenAiEngineConfig) -> Self` with `transcribe_chunk(&self, pcm: &[f32], prompt: &str, language: Option<&str>, cancel: &CancelToken) -> Result<String, EngineError>` — in-memory WAV → multipart to `/audio/transcriptions` (reuse the engine's request/parse builders; task = transcribe, Buzz live path), `cancelable`-wrapped, engine timeout; joined text returned (the `EngineFn` contract).
- [ ] Failing tests: WAV golden-bytes test (known 4-sample input → exact header+payload hex), clamp test; mock-server chunk test (request carries prompt + model + wav part; response text returned); cancel-mid-request test (mock hangs, cancel → prompt return).
- [ ] Implement; gates; commit `M7: live-API chunk transcriber + WAV writer (F-061, F-041)`.

### Task 18: Recording window backend selector + live-API wiring (F-061b)

**Files:** Modify `crates/humm-gui/src/views/recording.rs`, `crates/humm-gui/src/recording.rs`, `crates/humm-gui/src/live_session.rs`, `crates/humm-gui/src/app.rs`, ftl ×4.
**Spec:** §10. **Buzz:** `widgets/recording_transcriber_widget.py` (options group), `recording_transcriber.py:220-231`.
**Produces:** recording options group gains the backend combo (shared `render_backend_combo`) + API-key row (API backend only), persisted `recording.backend` in the window-close save group; session start: backend == OpenAiApi → `EngineFn` closure over `LiveApiTranscriber` (settings-fresh auth/model at record-start, keyring key; missing key → inline error before start, Buzz-parity message via ftl) instead of the whisper engine (no model resolution/download for API); measured-speed recording keys API sessions as `openai_api` (spec L14) and `realtime_fit_warning_visible` reads that key for API backend.
- [ ] Failing tests: kittest recording fixture — backend combo persists; API-arm scripted session end-to-end via a mock-server-backed `LiveApiTranscriber` (panel text + store rows; TCP count noted) or, if harness TCP is blocked in this context, a scripted `EngineFn` seam test proving the closure selection + a `#[ignore]`-free mock test at core level already covers HTTP (decide in-task, document); missing-key inline error test; measured-speed key test.
- [ ] Implement; gates; commit `M7: recording backend selector + live-API session arm (F-061)`.

### Task 19: ADR-021 ticket — live state reuse (humm-whisper)

**Files:** Modify `crates/humm-whisper/src/engine.rs`.
**Spec:** §10 (perf ticket). **Reference:** ADR-021's traced mechanism (`create_state` → CoreML reload, engine.rs:186).
**Produces:** a per-session live state: `WhisperEngine::begin_live_session(&mut self) -> Result<(), EngineError>` + `end_live_session(&mut self)` (or an RAII `LiveSessionState` owned by the engine — implementer picks the shape that satisfies borrow rules with `transcribe_live(&mut self, …)`), such that consecutive `transcribe_live` calls reuse one `WhisperState`; GUI session start/stop calls bracket it. Behavior contract unchanged: same outputs (rolling prompt is per-call), cancel semantics unchanged.
- [ ] Failing/regression tests: existing humm-whisper suite green; a unit test on the state-lifecycle seam (session begin → N transcribe_live → end; second begin works); `#[ignore]` real-model test variant if the existing ones exercise transcribe_live (run what's runnable — GPU-dependent parts stay for Task 20's lifted run).
- [ ] Implement; gates; commit `M7: reuse whisper state across live chunks — ADR-021 ticket (F-061)`.

### Task 20: Windowed acceptance — gui-manual v5 + benches + live ratio

**Files:** Create `docs/gui-manual/M7.md`, `docs/benchmarks/M7.md`.
**Spec:** §12. **Environment:** sandbox-lifted for windowed runs + benches; benches vs a same-day M6 checkout (`git worktree add` at `2327e80`) per `docs/benchmarks/M6.md` interleaved-pairs-under-load methodology.
- [ ] gui-manual v5 checklist authored + executed where automatable (smoke hooks/windowed runs): General rows live (locale switch mid-session, font size), shortcuts capture + live re-apply + conflict, folder-watch real run (real folder, real notify, watch → task → mirrored output → pill), post-processing config, models badge, live-API arm (fixture run against local mock/oai_shim if no key; else recorded real run), carried v1–v4 pointer items restated.
- [ ] Live realtime ratio before/after Task 19 measured via `HUMM_SMOKE_LIVE_FIXTURE` (lifted, small model) → `docs/benchmarks/M7.md` + ADR-021 addendum note text drafted (Task 21 lands it).
- [ ] Criterion interleaved pairs M6-vs-M7 on the transcription path (decode/e2e/export) → no regression, or fix before proceeding; call-path git-diff innocence argument recorded (post-hooks off by default).
- [ ] Commit `M7: acceptance — gui-manual v5 + benches + live ratio (F-030, F-061, …)`.

### Task 21: Docs sweep + parity matrix + milestone report

**Files:** Modify `docs/analysis/PARITY_MATRIX.md`, `docs/analysis/FEATURE_INVENTORY.md` (F-030 method note), `docs/design/DECISIONS.md` (ADR-021 addendum; ADR-010 as-built note), `docs/design/UX_SPEC.md` (J-4/J-9/§3 shipped notes), `docs/HUMM_SURFACE.md` (final read-through of M7 additions), memory files.
**Spec:** §12, §13, §14.
- [ ] Matrix flips with as-built notes: F-030/F-100/F-101/F-110/F-111/F-113/F-115/F-116 → `implemented`/`verified` per what actually ran; F-005/F-044 full-scope; F-008 → `implemented` under spec §9.2 semantics; F-061 note rewritten (scope cut closed). Verification claims only for methods that ran.
- [ ] FEATURE_INVENTORY F-030 correction (`unit + cli-int` → `unit + gui-int`) in the same commit as its justification.
- [ ] Milestone report (final message + memory update): matrix status, bench numbers, deviations ledger refs, analysis corrections, environment-blocked debt (real-chat contracts ×2, any mic/GPU-gated leftovers), remaining mechanical locales, M8 planning inputs.
- [ ] Commit `M7: docs sweep + matrix + milestone report`.

---

## Self-review notes (kept, per house style)

- Spec coverage: §1→T1; §2→T2/T4/T7; §3→T3/T8/T9/T15; §4→T4; §5→T7; §6→T5; §7→T6; §8→T11/T13/T14/T15/T16; §9→T10/T12; §10→T17/T18/T19; §11→T2; §12→T20/T21; §13/§14 threaded through named tasks. Viewer id→index carried ticket: no M7 task edits `viewer.rs` — stays carried (spec §14), re-checked at final review.
- Type consistency: `PostprocConfig`/`PostprocConfigSource` (T4) consumed by T5/T6/T7; `Candidate`/`scan` (T8) by T9; `AVAILABLE_LOCALES`/`set_locale` (T10) by T11/T12/T13; `serialize_shortcut`/`conflicts` (T14 internal); `LiveApiTranscriber::transcribe_chunk` (T17) by T18; `mark_skipped`/`find_completed_by_file_name`/`has_task_for_file` (T2) by T4/T8/T9.
- Deliberate deviations from the skill's full-code-in-steps format: this plan follows the M2–M6 house style (terse steps, spec + Buzz source as the governing detail); every implementer must read the spec §§ + Buzz files named in their task.
