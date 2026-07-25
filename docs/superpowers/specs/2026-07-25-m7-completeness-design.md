# M7 — Completeness: preferences, folder watch, native "plugins", locales (design spec)

Parity IDs: **F-005** (editor half), **F-008** (locale ports begin), **F-030**, **F-044**
(full), **F-100** (complete), **F-101**, **F-110** (ADR-010 resolution), **F-111**,
**F-113**, **F-115**, **F-116** — plus, decided in §0 below, the **F-061 live-API
scope-cut closure** carried from M6.

Ground truth re-read for this spec (all from `/Users/jawad/RustroverProjects/buzz`):
`settings/shortcut.py`, `settings/shortcuts.py`,
`widgets/preferences_dialog/{preferences_dialog,general_preferences_widget,models_preferences_widget,folder_watch_preferences_widget,shortcuts_editor_preferences_widget}.py`,
`widgets/preferences_dialog/models/{preferences,folder_watch_preferences,file_transcription_preferences}.py`,
`widgets/sequence_edit.py`, `locale.py` + `locale/*/LC_MESSAGES/buzz.po`,
`widgets/transcription_task_folder_watcher.py`, `transcriber/file_transcriber.py:55-73,155-170`,
`plugins/{base,manager,loader,post_processing}.py`,
`plugins/{ai_summary,export_docx,skip_already_transcribed,transcript_resizer}/plugin.py`,
`widgets/main_window.py:167-183,435-495`, `file_transcriber_queue_worker.py:208-230`,
`db/service/transcription_service.py:62-73`, `widgets/menu_bar.py:85-102`,
`settings/settings.py` (key names + `get_default_export_file_template`).

Executors must re-read the cited Buzz files per task (CLAUDE.md rule) — this spec
records decisions and load-bearing findings, not a substitute for the source.

---

## 0. Scope boundaries (explicit)

**In scope**
1. Full Preferences window (F-100 complete, F-044 full, F-005 editor, F-030 tab,
   F-101 assembly): tabs **General · Models · Shortcuts · Folder Watch ·
   Post-processing ✚ · Privacy**.
2. Folder watcher (F-030): `notify`-based service in `humm-gui`, pure scan/skip
   logic in `humm-core`, structure mirroring, delete-or-move-source disposition in
   the pipeline, `Source::FolderWatch` producer, J-4 ✚ "Watching &lt;folder&gt;" pill
   with pause toggle.
3. The four native "plugins" (ADR-010): ai_summary (F-111), export_docx (F-113),
   skip_already_transcribed (F-115), transcript_resizer (F-116) — as first-class
   features with a Post-processing preferences tab, **no plugin mechanism of any
   kind** (F-110 resolves to "the strategy shipped").
4. First locale ports (F-008): locale infrastructure (setting, negotiation,
   **live switching ✚**) + starter catalogs **de_DE, es_ES, zh_CN**; the remaining
   11 Buzz locales recorded as mechanical follow-ups.
5. **Live transcription via the API backend** (the M6 scope cut; F-061 row note
   closure) + the ADR-021 per-chunk `create_state()`/CoreML-reload perf ticket,
   which falls due with it.
6. Carried tickets that fall due because their files are touched (§14): the
   translation-drain poll-join (pipeline.rs), the translator client-build
   fallback hardening (translate.rs).

**Decision — live-API transcription joins M7.** Rationale (criteria: parity
completeness, risk placement, cost): (a) M7's charter is "completeness" — leaving
a non-ML parity gap standing after it contradicts the milestone's purpose, and the
only later slot is M8, where risk is already concentrated in three heavy-ML
integrations; (b) the cost collapsed on inspection: `live::session` is already
engine-agnostic (`EngineFn = dyn FnMut(&[f32], &str) -> Result<String, EngineError>`,
`session.rs:160`), so the API arm is a new closure over existing M6 machinery
(`OpenAiEngine`, `AudioEncoder`), plus a backend selector the options sheet
already knows how to render (`render_backend_combo`); (c) ADR-008 kept exactly two
backends — local whisper-rs *and* the OpenAI-compatible API — and Buzz's recording
window offers the API backend (`recording_transcriber.py:220-231`), so the gap is
squarely a parity gap, not an ADR-recorded cut.

**Out of scope (unchanged M8 rows):** F-027 (Demucs), F-087 (speaker ID), F-112
(DeepFilterNet), F-114 (enhanced language detection). The `before_transcription`
hook point exists in Buzz solely for F-112/F-114 — M7 therefore implements **only
`check_skip` and `on_complete` seams**; a pre-transcription seam is added in M8 by
whichever feature first needs it, not speculatively now.

**Environment-blocked (planned honestly):** the env-gated real-chat contract
(`HUMM_LLM_TEST_BASE`, F-031) still has no local target (no ollama/LM Studio on
this machine, per the M6 record) — it stays committed-runnable and F-031 stays
`implemented`. ai_summary's real-chat contract test faces the identical gap: it
ships env-gated (`HUMM_LLM_TEST_BASE`, same variables), with local mock-server
tests carrying the actual verification load. Both remain visible in the milestone
report as environment-blocked debt.

---

## 1. Settings & surface additions (ADR-012)

New/extended `settings.toml` sections (all `#[serde(default)]`, atomic save,
clamps on load — the established pattern, `settings.rs:651-713`):

| Section | Field | Type | Default | Consumer |
|---|---|---|---|---|
| `[shortcuts]` (new, flattened map) | — | `HashMap<String, String>` keyed by `SCREAMING_SNAKE` action name | `{}` | `ShortcutMap::from_settings` — the exact map shape it already accepts (`shortcuts.rs:233`) |
| `[ui]` (new) | `locale` | `Option<String>` (Buzz locale codes, e.g. `"de_DE"`) | `None` = system locale | i18n negotiation (§9) |
| | `font_size` | `Option<u32>`, clamped `8..=32` on load | `None` = egui default | app-wide text-style scaling (§8.1) |
| `[models]` | `force_cpu` | `bool` | `false` | engine construction: `use_gpu = !(HUMM_FORCE_CPU set ∨ settings.models.force_cpu)` — env wins, read fresh per task/engine build (Buzz sets `BUZZ_FORCE_CPU` live from the checkbox; humm reads settings at construction, same next-task effect, no restart) |
| `[file_transcriber]` | `default_export_template` | `Option<String>` | `None` = `export::DEFAULT_EXPORT_TEMPLATE` | `OutputSpec.file_name_template` at every spec-build site: options-sheet `build_specs`, CLI `run.rs` (today hardcoded `None`, `run.rs:241`), viewer export menu, folder-watch emit |
| `[folder_watch]` (new) | `enabled` | `bool` | `false` | watcher service |
| | `input_dir` / `output_dir` | `Option<PathBuf>` | `None` | watcher service (Buzz `folder_watch/input_folder`, `output_directory`) |
| | `delete_processed_files` | `bool` | `false` | source disposition (§3.4) |
| | `options` | `FileTranscriberSettings` (type reuse; its `last_import_folder` is simply unused here) | default | the embedded per-auto-task options form (Buzz's `file_transcription_options` sub-group) |
| `[postproc.ai_summary]` (new) | `enabled` | `bool` | `false` (Buzz plugins default disabled, `manager.py:219`) | §6 |
| | `api_url` | `Option<String>` | `None` → `https://api.openai.com/v1` (Buzz default string) | |
| | `model` | `String` | `"gpt-4o-mini"` | |
| | `prompt` | `String` | Buzz's `DEFAULT_PROMPT` verbatim (`ai_summary/plugin.py:31-34`) | |
| | `save_to_notes` | `bool` | `true` | |
| | `save_to_file` | `bool` | `false` | |
| | `output_folder` | `Option<PathBuf>` | `None` = next to source | |
| `[postproc.export_docx]` (new) | `enabled` | `bool` | `false` | §7 |
| | `output_folder` | `Option<PathBuf>` | `None` = next to source | |
| | `include_timestamps` | `bool` | `false` | |
| `[postproc.skip_already_transcribed]` (new) | `enabled` | `bool` | `false` | §4 |
| | `check_result_files` | `bool` | `true` | |
| | `check_database` | `bool` | `false` | |
| `[postproc.transcript_resizer]` (new) | `enabled` | `bool` | `false` | §5 |
| | `merge_by_gap` / `split_by_punctuation` / `split_by_max_length` | `bool` | `true`/`true`/`true` | |
| | `merge_gap_seconds` / `punctuation` / `max_length` | `String` | `"0.2"` / Buzz `DEFAULT_PUNCTUATION` / `"42"` (TEXT fields in Buzz — kept as strings, parsed at use, Buzz-faithful) | |
| `[recording]` | `backend` | `Option<String>` (wire string, same vocabulary as `file_transcriber.backend`) | `None` = local | live-API backend selector (§10) |

**Secrets (ADR-012):** `SecretKey` gains `AiSummaryApiKey` with entry name
`"AI Summary API key"` (service stays `"humm"`). Buzz stored it as keyring secret
`plugin:ai_summary:api_key`; humm's own naming is already established as a
deliberate service/entry scheme (`secrets.rs`), ledger #L9. The key is never in
TOML/DB/logs; the Post-processing tab reuses `secret_field::masked_field_row` +
the M6 `Debug`-redaction discipline.

`HUMM_SURFACE.md` gains: the new settings tables, the `skipped` + `post_processed`
`--json` events (§11), and no new env vars (deliberate — every new knob is a
setting; existing env overrides keep their precedence).

---

## 2. Core: hook architecture (the ADR-010 "plugin" seams)

A new module **`humm_core::postproc`** owns the four features' pure logic and one
dispatch struct. No trait objects for extensibility, no registry, no ordering
state — ADR-010: these are features, not plugins.

### 2.1 The two seams

**`check_skip`** — runs in `queue.rs::run_job` after input resolution
(`queue.rs:982`) and before artifact-ensure (`queue.rs:1004`), i.e. after
`mark_started`/`Started` (Buzz also flips an already-in-progress task to skipped:
`file_transcriber_queue_worker.py:222-227`). Only `Input::Path` tasks are
checked in practice (a URL input's local file doesn't exist before fetch; Buzz's
GUI-only watcher/import paths are all local files — for `Input::Url` the check
runs after fetch against the downloaded file, matching Buzz's "file_path at check
time" semantics). Returns `Option<Vec<Segment>>`; `Some(segs)` short-circuits to
a new **`Terminal::Skipped(Vec<Segment>)`** arm.

**`on_complete`** — runs in `queue.rs::run_task`'s `Terminal::Completed` arm
**after** `mark_completed` and **after** `emit(Completed)` (Buzz order:
`process_completed` saves-as-completed, then runs `on_complete`,
`manager.py:153-164`; the table already shows Completed while hooks run). Fixed
execution order: **transcript_resizer → ai_summary → export_docx** — a deliberate
deviation from Buzz's arbitrary discovery-order + user reordering (ledger #L2):
resizer first so the docx export (which re-reads the store, `export_docx/plugin.py:92`)
reflects resized segments; summary reads the in-memory outcome segments (Buzz
passes the same in-memory list, `ai_summary/plugin.py:103`), which resizing never
textually changes.

Each hook is individually `enabled`-gated; every hook failure is contained
(logged + carried into the event, never failing the task — Buzz wraps each hook
in try/except, `manager.py:93-148`).

### 2.2 Threading decision — hooks run on the runner thread

Buzz splits: GUI runs post-hooks on a background thread (`main_window.py:459-471`),
CLI runs them synchronously (`quit_on_complete`, same lines). humm runs them
**synchronously on the queue runner thread in both binaries**. Rationale: (a) the
queue is serialized by design (one transcription at a time — parity feature), so
"the next task waits" is a bounded-delay deviation, not a UI freeze — the GUI
never blocks (events keep flowing); (b) the worst case is ai_summary's single
120 s request timeout, and every other hook is milliseconds of local work;
(c) it keeps ADR-018 store-write policy, event ordering, cancellation, and
`finish()` semantics exactly as they are — no marshaling layer (Buzz needed
`post_processing.py`'s `MainThreadInvoker` only because of Qt thread-affine DB
handles, which humm doesn't have). Recorded as ledger #L3.

Cancellation: the cancel token is checked between hooks; ai_summary's request
runs under its own 120 s client timeout and additionally routes through
`openai::client::cancelable` (unlike Buzz, whose summary request is
uninterruptible) — cancel mid-summary is acked sub-second, the surviving hooks
are skipped, and the task stays `Completed` (it already was).

### 2.3 New event + store vocabulary

- `TaskEventKind::Skipped` (additive). CLI: exhaustive matches in
  `ProgressLine::on_event` and `emit_json` are the compile-time forcing function
  (agent-verified `run.rs:708/902`). Human mode prints nothing new beyond the
  terminal line; `--json` emits `{"event":"skipped","task_id":…}`. Exit-code
  contribution: a skipped task counts as success (exit 0 arm) — Buzz's CLI treats
  skip as completion (`main_window.py:437-443`).
- `TaskEventKind::PostProcessed { notes_updated: bool, segments_replaced: bool,
  written: Vec<PathBuf>, errors: Vec<String> }` (additive) — emitted once after
  the hook chain **iff at least one hook is enabled**; the GUI refreshes the row
  (notes column, viewer segment reload) on it, mirroring Buzz's
  `refresh_row`-after-hooks. `--json`: `{"event":"post_processed",…}`.
- `TaskStore::mark_skipped(&id, &[Segment])` — status → `skipped`, `time_ended`
  set, segments inserted with `""` translations (Buzz
  `transcription_service.py:62-73`). `Status::Skipped` finally gets its producer;
  the GUI's existing `can_open` (Completed|Skipped) and `retry_allowed`
  (Failed|Canceled|Skipped) light up with no changes.
- `SqliteStore::find_completed_by_file_name(name: &str) -> Option<TranscriptionId>`
  — `WHERE file` **basename** matches and `status='completed'`, newest first
  (Buzz matches by basename: `skip_already_transcribed/plugin.py:163-166`).
  Basename matching is done in SQL on the stored path's tail; no new index (table
  sizes are personal-history scale; note in code).

### 2.4 `RunnerDeps` addition

`RunnerDeps.postproc: Option<Arc<PostprocConfigSource>>` — a small trait giving
the runner a fresh config snapshot per task (GUI: reads `Arc<Mutex<Settings>>`;
CLI: reads the loaded settings — CLI **does** run hooks, Buzz parity). `None`
(tests that predate M7) = all hooks off. The summary hook also needs the secret:
the config source resolves the key (env-free; keyring via `SecretStore`) at
snapshot time, so the runner never holds a `SecretStore`.

---

## 3. F-030 — Folder watch

### 3.1 Core logic — `humm_core::folderwatch` (pure, unit-tested)

Ported from `transcription_task_folder_watcher.py` with the store standing in for
Buzz's in-memory task dict (Buzz's own TODO: "query db instead of passing tasks",
`transcription_task_folder_watcher.py:25`):

- `SeenSet`: `HashMap<PathBuf, (u64 size, SystemTime mtime)>` — session-scoped,
  exactly Buzz's `paths_emitted` semantics (a file re-emits if its size/mtime
  changed *and* it's not already a known task).
- `fn scan(input_dir, seen: &SeenSet, known: &dyn Fn(&Path) -> bool)
  -> Vec<Candidate { path, relative_dir }>` — recursive walk skipping hidden
  directories; per-file skip rules ported verbatim
  (`transcription_task_folder_watcher.py:70-81`):
  1. hidden files (`.`-prefixed name);
  2. extension not in the 13 supported (reuse the list already ported to
     `humm-gui/src/import.rs:15-21` — **moves to `humm_core::folderwatch` so core
     owns it**; `import.rs` re-imports);
  3. temp double-extension files (secondary extension is itself a media
     extension, e.g. `.ogg.wav`);
  4. `"_speech.mp3"` contained in the file name;
  5. already a known task (store lookup by full path — new
     `SqliteStore::has_task_for_file(path)`);
  6. seen with identical (size, mtime).
- `fn mirrored_output_dir(output_root, relative_dir) -> PathBuf` — Buzz's
  relpath mirroring (`:97-107`); `create_dir_all` at emit time.

### 3.2 GUI service — `humm-gui::folder_watch` (notify-backed)

New dependency: **`notify` 8.x** (pure-Rust crate over FSEvents/inotify/
ReadDirectoryChanges — same dependency class as `keyring`/`cpal`; it lives in the
GUI binary, not core, because folder watch is a GUI-app feature in Buzz and the
CLI has no folder-watch surface; core stays OS-boundary-free). A
`FolderWatchService` owns:

- one `notify::RecommendedWatcher` in `RecursiveMode::Recursive` on `input_dir`
  (replacing Buzz's manual per-subdir `addPath` bookkeeping — same coverage, the
  library does the recursion);
- a worker thread that drains the event channel (drain-then-rescan gives natural
  event coalescing; Buzz rescans the whole tree per directory event — same
  strategy, `:53-60`), runs `folderwatch::scan`, and for each candidate:
  `create_dir_all(mirrored)`, build a `TaskSpec` from
  `settings.folder_watch.options` (same construction path as the options sheet's
  `build_specs`), `OutputSpec { directory: mirrored, source_disposition:
  Some(…) }`, and `queue.enqueue(id, spec, Source::FolderWatch)` — **the first
  `Source::FolderWatch` producer**;
- an initial full scan on start/enable (Buzz calls `find_tasks()` immediately at
  startup and on every prefs change, `main_window.py:172,183`);
- a session-scoped `paused: AtomicBool` (the pill's toggle — pausing mutes
  event handling without touching settings);
- rebuild-on-settings-change: the app compares the `[folder_watch]` snapshot
  after the Preferences viewport reports a change and tears down/rebuilds the
  service (Buzz applies folder-watch prefs only on dialog OK; humm's
  write-through tab applies on edit-commit — folder fields commit on
  focus-out/Enter like every humm path field, so no per-keystroke watcher churn).

Model downloads: Buzz downloads a missing model synchronously inside the watcher
thread before emitting (`:93-95`); humm just enqueues — the queue's existing
non-modal model-download phase (UX J-1 ✚) handles absence. No code, better
behavior; ledger #L4.

### 3.3 J-4 ✚ pill

Trailing toolbar chip after the update-notice chip (`main_window.rs:503-504`
precedent): visible iff `folder_watch.enabled` and a directory is configured;
label `Watching {folder-name}` (basename); a pause/resume toggle button flips the
service's `paused` flag; paused state renders the chip dimmed with `Paused`
suffix. Chip strings via `tr()`.

### 3.4 Source disposition (delete-or-move) — pipeline

`OutputSpec` gains `source_disposition: Option<SourceDisposition>` where
`enum SourceDisposition { Delete, MoveToOutput }` (serde-default `None`; old
`options_json` rows deserialize unchanged). After the export loop in
`pipeline.rs` (the same placement as Buzz — `file_transcriber.py:72-73` runs
`_handle_folder_watch` right after `write_output`, before any plugin hook):
`Delete` → `fs::remove_file(input)`; `MoveToOutput` → rename into
`output_dir/basename` with copy+delete fallback for cross-device moves. Errors
are logged, non-fatal (Buzz would raise out of the worker; humm treats a
disposition failure like a store-write failure — the transcription succeeded,
ADR-018 spirit). The watcher sets `Delete` when `delete_processed_files` else
`MoveToOutput` — **Buzz always removes the source from the input tree** (delete
or move, `file_transcriber.py:155-170`); the tab's checkbox chooses which, and
the Folder Watch tab copy makes that explicit (Buzz's UI never said so).

### 3.5 Verification

- `folderwatch::` unit tests: every skip rule, double-extension corner, seen-set
  re-emit on size change, mirroring math (temp dirs, no watcher).
- Integration (the plan's acceptance gate): humm-gui test with a real
  `notify` watcher over temp dirs + scripted engine — drop a fixture file →
  `wait_for` a `Source::FolderWatch` row completing → assert mirrored export
  path + source moved (and a delete-variant case). Matrix method for F-030
  corrects `unit + cli-int` → `unit + gui-int` (the CLI has no folder-watch
  surface in Buzz or humm — Phase-1 doc error, fixed in the same commit that
  lands the test).

---

## 4. F-115 — skip_already_transcribed (native)

`humm_core::postproc::skip` — port of `skip_already_transcribed/plugin.py`:

- Stem: file stem with trailing `"_speech"` stripped (`:80-82`).
- Result-file check (`check_result_files`, default on): search dirs = input's
  parent + task output dir if set (`:84-86`); per dir, per format **in Buzz's
  `OutputFormat` declaration order TXT → SRT → VTT** (humm's `OutputFormat` order
  matches), match files whose name starts with the stem and ends with the
  format extension — the faithful port of Buzz's `glob(f"{stem}*.{ext}")`
  including its prefix-overmatch quirk (`audio*.srt` also matches `audio2.srt`) —
  first parseable hit wins.
- Parsers ported verbatim: TXT → single `Segment { 0, 0, text.trim() }`, empty
  file → not a hit (`:104-106`); SRT blocks split on `\n\n+`, ≥3 lines, timestamp
  regex `(\d+:\d+:\d+,\d+) --> (\d+:\d+:\d+,\d+)` with `,`→`.` before parsing;
  VTT blocks ≥2 lines, regex `(\d+:\d+:\d+\.\d+) --> (\d+:\d+:\d+\.\d+)`;
  `h*3600+m*60+s` seconds → ms via `int()` truncation (`:155-158`); parse
  failures logged, treated as no-hit.
- DB check (`check_database`, default off): `find_completed_by_file_name(basename)`
  → copy that transcription's segments (text only, translations dropped — Buzz
  rebuilds `Segment(start,end,text)`, `:170-174`).
- Outcome: `Terminal::Skipped(segments)` → `mark_skipped` persists them → row
  opens in the viewer (Completed|Skipped gate) — Buzz behavior end to end.

Verification (`unit`): parser fixtures (SRT/VTT/TXT round-trips incl. malformed
blocks), stem/`_speech` handling, glob-order and prefix-quirk pinning, DB-hit
path against a temp store, and a queue-level test driving a scripted engine that
must **never run** when a planted sibling `.srt` exists.

---

## 5. F-116 — transcript_resizer (native auto-resize)

`humm_core::postproc::auto_resize` — port of `transcript_resizer/plugin.py` over
the ADR-020 machinery:

- Gate 1: task's `word_timestamps` on, else skip logged (`:91-98`).
- Gate 2: regroup string non-empty. Built exactly like Buzz (`:129-151`) — which
  is the **same construction** as the viewer's Merge:
  `regroup::default_algo(merge_by_gap, gap, split_by_punctuation, punctuation,
  split_by_max_length, max_length)` + `regroup::algo_from_env_or` (the
  `HUMM_MERGE_REGROUP_RULE` override, mapped from Buzz's
  `BUZZ_MERGE_REGROUP_RULE`, `:151`).
- Read segment rows from the store, run
  `regroup::regroup_segments(&segs, language, &algo)` (the sentence-end word
  reconstruction — `SENTENCE_END = .*[.!?。！？]`, non-space languages
  `zh/ja/th/lo/km/my` get `""` separator — is already inside
  `regroup::words_from_segments`'s contract; the executor verifies the ported
  reconstruction against `plugin.py:178-200` and adds any missing pieces to
  `regroup::words`), then `store.replace_segments`.
- Empty result → keep original, log (`:117-119`).
- **Dropped Buzz check, ledger #L5:** `_resolve_audio_path`'s skip-if-source-
  audio-missing (`:153-167`) exists only because stable-ts demands an audio
  argument it then ignores (`vad=False, suppress_silence=False`); humm's regroup
  is a pure function, so the auto-resizer runs even after folder-watch moved or
  deleted the source — strictly more capable, no output difference when audio
  exists.
- Ordering note (§2.1): runs **before** export_docx so the docx reflects the
  resize; on-disk SRT/VTT/TXT exports were already written pre-hook and are *not*
  rewritten — exactly Buzz's behavior (hooks run after `write_output`).

Verification (`unit + diff`): unit tests for the gates, config→algo-string
matrix pinned against hand-traced Buzz strings (incl. the `mg=…++len+1`
composition), an end-to-end store-backed test (word-timestamped rows in →
replaced rows out); the `diff` half rides on ADR-020's committed Python-reference
regroup fixtures (16 files) plus, if the reconstruction needed new code in
`words.rs`, one regenerated fixture from the existing
`validation/m4-differential` harness extended with the plugin's `get_transcript`
shape.

---

## 6. F-111 — ai_summary (native)

### 6.1 Shared chat call (core refactor)

`translate.rs`'s private `post_chat_completion` (`:545-597`) is extracted to
`openai::chat::complete(client, auth, model, system, user) -> Result<String, ApiFailure>`
(pub within core); the translator calls it unchanged (differential parse fixtures
must stay green — the refactor is behavior-neutral by test). While in the file,
the carried one-liner lands: the worker's client-build fallback
`Client::new()` (timeout-less) gains the 60 s timeout (§14).

### 6.2 The hook — `humm_core::postproc::summary`

Port of `ai_summary/plugin.py:96-168`:

- Skip (logged) when: no key configured (`:98-100`); transcript empty after
  joining non-empty segment texts with `\n` (`:103-107`).
- Request: own `reqwest::blocking::Client` with **120 s** timeout (`:144`,
  Buzz `timeout=120.0, max_retries=0` — no retries, matching reqwest's default),
  via `chat::complete`, wrapped in `openai::client::cancelable` (cancel-ack ✚
  over Buzz's uninterruptible call; ledger #L6). Endpoint: `api_url` else
  default base; key from `SecretKey::AiSummaryApiKey` — **deliberately its own
  key, not the transcription key** (Buzz's plugin likewise had its own secret).
- `save_to_notes` → `store.set_task_notes(id, summary)` (overwrite — Buzz's
  `update_transcription_notes` is an overwrite too).
- `save_to_file` → `{stem}.summary.txt` in `output_folder` else next to the
  source (`original` path preferred, `:155-163`), `create_dir_all`, write; errors
  logged, non-fatal.
- **Analysis correction (Task 5 review):** the response-content gate is two
  distinct Buzz behaviors, not one. `choices`/`message` *structurally absent*
  → error logged ("AI Summary: empty response from server"), nothing written
  (`:150-153`); `message` *present* but its `content` is empty/`None` →
  `_summarize` returns that falsy value and `on_complete`'s `if not summary:
  return` (`:111-112`) exits **silently** — no log, no error, nothing
  written. `chat::complete` maps the former to `Err(ApiFailure::Shape)`
  (logged) and the latter to `Ok("")` (silent no-op) — the same distinction
  the shared function already drew pre-M7, carried through unchanged.

### 6.3 Verification

`unit` + mock-server contract: config gating, transcript assembly, notes/file
fan-out against a temp store — network-free; plus local-mock HTTP tests
(the M6 `chat_mock`/test-server pattern) for request shape (system=prompt,
user=transcript) and error paths — these bind TCP and join the sandbox-lifted
baseline with documented cause (§12). The real-endpoint contract test ships
env-gated behind `HUMM_LLM_TEST_BASE`/`_KEY`/`_MODEL` (same variables as F-031's;
one shared harness, two callers) — environment-blocked, stays in the report.

---

## 7. F-113 — export_docx (native)

`humm_core::export::docx` (core already depends on `zip`):

- `write_docx(w: impl Write + Seek, title: &str, segments: &[Segment],
  include_timestamps: bool, paragraph_split: Millis) -> io::Result<()>` — the
  three-entry OOXML zip ported byte-for-byte from `export_docx/plugin.py:35-61,
  126-186`: `[Content_Types].xml`, `_rels/.rels`, `word/document.xml` (template
  strings verbatim, XML-escaping equivalent to Python's `saxutils.escape` +
  the `xml:space="preserve"` runs, bold-run and heading markup identical,
  deflate compression).
- Timestamps mode: `[{to_timestamp(start,'.')} --> {to_timestamp(end,'.')}] `
  as a bold run + text run per segment, empty texts skipped (`:133-144`) —
  reusing `export::to_timestamp` (executor pins the separator against Buzz's
  `to_timestamp` default).
- Paragraph mode: gap-split grouping ported from `:146-162`; **deviation ledger
  #L7**: the split threshold routes through the caller-supplied
  `paragraph_split` (the hook passes `paragraph_split_from_env()`) instead of
  Buzz's hardcoded 2000 ms — with the env unset the value *is* 2000 and output
  is Buzz-identical; this keeps HUMM_SURFACE §5's "one helper, N call sites"
  consistency promise.
- The hook (`postproc::docx`): re-reads segments from the store (`:92-94` —
  post-resize by §2.1 ordering), skips when empty, stem from the original path
  (`_speech`-aware via the same stem helper), `{stem}.docx` into `output_folder`
  else next to source.
- The viewer's export menu does **not** grow a DOCX entry: in Buzz, DOCX exists
  only as the plugin (F-084's menu is TXT/SRT/VTT × Text/Translation) — parity.

Verification (`snapshot`): insta snapshots of `document.xml` for both modes +
the zip entry list (names + order), a fixture with CJK/escaping-hostile text
(`&<>"'`), and one end-to-end hook test writing a real `.docx` through a temp
store. (A checked-in Buzz-generated fixture is unobtainable — Buzz's Python
package can't run on this machine's 3.9 — so the snapshot is seeded from the
verbatim-ported templates and the port is reviewed against the source line by
line; recorded honestly in the matrix row.)

---

## 8. Preferences (F-100 complete · F-044 full · F-005 editor · F-030 tab · F-101)

Tab strip becomes **General · Models · Shortcuts · Folder Watch ·
Post-processing ✚ · Privacy** (`PrefsTab` + the strip at `preferences.rs:36-96`).
Buzz's OK/Cancel pair is not ported — humm's write-through-on-commit pattern is
established (M3/M6); Buzz itself write-through-saves everything except the
folder-watch group (only that tab was OK-gated, `menu_bar.py:98-102`) — humm's
folder-watch tab commits on field-commit and the watcher rebuilds then (§3.2).
Ledger #L8. Buzz's Help-menu "Plugins…" item is not ported (the dialog it opened
has no successor; ADR-010) — the Post-processing tab is the four features' home.

### 8.1 General tab completion (F-100)

Rows appended to the existing OpenAI block (Buzz order preserved where the row
survives, `general_preferences_widget.py:66-225`):

1. **UI language** combo — top of the tab (Buzz row 1). Lists `en_US` + the
   ported locales (§9), display names from Buzz's `ui_locales` mapping.
   **Applies live ✚** (ledger #L10): humm's Fluent strings all resolve per frame
   (agent-verified ~100% `tr()` coverage), so switching the bundle re-renders the
   whole UI next frame — Buzz's "Restart required!" tooltip is replaced by
   nothing at all. Persists `ui.locale`.
2. **Font size** spinbox (`DragValue` 8–32, Buzz's QSpinBox range) — applies
   live by scaling every `TextStyle` in `ctx.style` proportionally from egui's
   defaults (`None`/unset = untouched defaults; applied at startup and on
   change — no per-frame set). Persists `ui.font_size`.
3. *(existing M6 OpenAI block: key + Test + base URL + api model.)*
4. **Default export file name** — text field editing
   `file_transcriber.default_export_template`, commit-on-focus-out, empty →
   `None` (default template shown as hint text). Wired into all four spec-build
   sites (§1).
5. **Disable GPU** checkbox — `models.force_cpu` (§1). Tooltip = Buzz's
   ("Set this if larger models do not fit your GPU memory and humm crashes",
   localized).

**Not ported, ledger #L11:** the live-export block (enable/folder), the
live-recording-mode combo, and Buzz's in-app note "Live recording export
settings will be moved to the Advanced Settings in the Live Recording screen in
a future version" — humm's M5 already did the move Buzz announced
(`[recording]` fields, edited in the recording window's advanced sheet); the
General rows would be a second writer for the same settings. Buzz's own note is
the cited justification.
**Not ported, ledger #L12:** "Reduce GPU RAM" (8-bit) — its only consumers are
the HF/Faster-Whisper backends ADR-008 does not port; a dead checkbox would be
surface without behavior.

### 8.2 Models tab full (F-044 + J-9 ✚)

- **Measured-speed badge ✚**: each size row shows `{ratio}× realtime` sourced
  from `settings.recording.measured_speed` (the M5 map, last-run-wins) when an
  entry exists for that size; no entry → no badge. Tooltip explains the number
  comes from this machine's live sessions.
- **Lumii gating (parity)**: `ModelSize::Lumii` appears in the list iff the
  effective UI locale is `lv_LV` (`models_preferences_widget.py:214-215`;
  Buzz additionally requires the whisper.cpp group — humm's single local backend
  *is* that group). `selectable_model_sizes` gains an `include_lumii` flag; the
  options sheet and recording window pass the same locale-derived flag (Buzz
  gates via the shared size list, so all combos follow).
- Group combo and FAQ link stay unported (ADR-008 / Buzz-docs-specific — both
  already recorded in the F-044 matrix row).

### 8.3 Shortcuts tab (F-005 editor + ✚ conflict detection)

- One row per `ShortcutAction::ALL` (20), label = the action's description key
  (new `description_key()` on the action enum → 20 new ftl strings matching
  Buzz's `shortcut.py` descriptions), field = a **sequence-capture widget**:
  click → capturing state (field shows `Press shortcut…`); next non-modifier
  keypress + held modifiers becomes the sequence (modifier-only presses ignored —
  `sequence_edit.py:26-33`); `Esc` cancels capture (and therefore can't be
  bound — Buzz's same documented limitation); a per-row clear button restores
  the default (Buzz's `setClearButtonEnabled`). Sequences render via the
  existing `ctx.format_shortcut` (⌘ on macOS).
- Capture writes `settings.shortcuts[key] = sequence` (removing the entry when
  it equals the default keeps the map minimal), saves, and bumps a
  shortcuts-generation so `HummApp` rebuilds `Arc<ShortcutMap>` next frame —
  the map is already read per frame by every consumer (main window, viewer,
  search), so **live re-apply is the rebuild** (menu hint texts update the same
  frame). Reset to Defaults clears the whole map (`shortcuts.py:20-21`).
- **Conflict detection ✚**: after any change, sequences are checked pairwise;
  every action whose sequence collides with another renders a warning icon +
  `Conflicts with {other}` caption. Non-blocking — the assignment stands (Buzz
  silently allowed conflicts; humm surfaces them without forbidding, the least
  surprising superset).
- Editable-sequence vocabulary: `parse_sequence` today accepts A–Z, `,`,
  `Return`, arrows — the capture widget emits only sequences the parser accepts,
  and grows `parse_key` with digits and common punctuation as needed (executor
  pins round-trips: captured → serialized → parsed → same `KeyboardShortcut`).

### 8.4 Folder Watch tab (F-030 UI)

Ported from `folder_watch_preferences_widget.py`: enable checkbox; input/output
folder rows (path field + native Browse via the existing `FileDialogs` seam);
"Delete processed files" checkbox; below, the **embedded auto-task options
form** — the options sheet's shared row renderers over a `SheetState` seeded
from `folder_watch.options` (`render_model_combo`/`render_task_combo`/
`render_language_combo`/word-timestamps checkbox/export-format checkboxes/
`render_backend_combo` + API-key row/LLM block — the private ones get
`pub(crate)` promotion; each renderer already takes an `id_salt` so sheet + tab
coexist). Everything except the enable checkbox is disabled while the feature is
off (`_set_settings_enabled`, `:132-138`). Tab copy states the delete-vs-move
disposition plainly (§3.4).

### 8.5 Post-processing tab ✚ (F-110/F-111/F-113/F-115/F-116 config)

Four collapsible sections in execution order (resizer, summary, docx — plus
skip-already-transcribed first, as the only pre-hook), each: enable checkbox +
its §1 config fields (Buzz's declarative `ConfigField` forms flattened into
native rows; PASSWORD → `masked_field_row` + keyring; BOOL → checkbox; TEXT →
single-line commit-on-focus-out; TEXTAREA → multiline for the summary prompt).
A header caption states hooks run after each transcription and never fail it.
No enable/ordering list, no install surface — ADR-010.

### 8.6 F-101

F-101 is the assembly row: all six tabs reachable, min-size 650×500 (already
set), kittest per new tab via the established `Harness::new_ui_state` fixture
pattern, gui-manual v5 covers the windowed pass.

---

## 9. F-008 — Locale ports begin

### 9.1 Infrastructure

- `settings.ui.locale: Option<String>` (§1) — Buzz locale codes (`de_DE` …) as
  the wire values (parity with Buzz's `ui-locale` setting semantics); `None` →
  system locale via the same env-derived detection `services::locale_from_env_vars`
  already implements for telemetry (promoted to a shared helper), fallback `en_US`.
- `i18n` grows a registry of embedded catalogs
  (`include_str!` per locale: `en`, `de`, `es`, `zh-CN`) and
  `set_locale(code: &str)` — a global `AtomicUsize` generation + selected-locale
  slot; each thread's `thread_local` bundle lazily rebuilds when its cached
  generation lags (deferred-viewport render threads included). Negotiation:
  exact Buzz-code match → language-only match → `en_US` (Buzz's gettext fallback
  chain, `locale.py:15-21`).
- Missing-key behavior unchanged (key echoes through) — and per-locale
  completeness is *tested*, so echo never ships (below).

### 9.2 Starter set + the mechanical rest

Port **de_DE, es_ES, zh_CN** now (two European + one CJK exercises the pipeline:
latin scripts, `zh` non-space rendering, placeable `{$…}` handling). Each
catalog: all 258 (± M7-added) keys translated; Buzz's corresponding `buzz.po`
serves as the reference glossary wherever humm's string matches or paraphrases a
Buzz string (~428 msgids per Buzz locale) — but humm translates **its own**
catalog (ADR-014: .po reuse is illusory; UI copy differs).

Per-locale test (extends `unit (catalog load)`): every locale's key set equals
the `en` key set exactly (both directions) and every key resolves non-empty —
the compile-time-ish guarantee that makes the remaining ports mechanical.

**Matrix semantics for F-008** (reconciling "starter set" with the M7 sweep
gate): the row flips to `implemented` when the localization *system* is complete
— negotiation, live switching, the General combo, per-locale completeness tests
— with the starter catalogs shipped; this is partial locale coverage of a fully
implemented mechanism, not a stub. `verified` stays gated on all 15 locales.
Remaining 11 (ca_ES, da_DK, it_IT, ja_JP, lv_LV, nl, pl_PL, pt_BR, ru, uk_UA,
zh_TW) are recorded in the row as "mechanical: catalog + combo entry + test, no
code" and stay visible in the milestone report.

### 9.3 Language combo + live switch

§8.1's combo lists exactly the embedded locales; switching calls `set_locale` +
persists. Live application is a ✚ (ledger #L10). The `lv_LV` Lumii gate (§8.2)
reads the *setting*, so it works even before the lv catalog is ported (parity
with Buzz, whose gate also reads the setting, not the translation files).

---

## 10. F-061 closure — live transcription via the API backend

Ported from `recording_transcriber.py:220-231` (executor re-reads; the shape:
Buzz writes each accepted chunk to a temp WAV and posts it through the OpenAI
client with the configured model, feeding the same text pipeline as local
backends).

- **Recording window options group** gains the backend selector + API-key row —
  the same promoted `render_backend_combo`/API-key renderers as the options
  sheet (M6 ledger #9/#10 ✚ carries over: key row visible only for the API
  backend, keyring-backed). Persisted `recording.backend` (§1) alongside the
  existing sticky options (window-close save group, `HUMM_SURFACE` §6.2).
- **Session wiring**: `EngineFn` stays the seam. The API arm's closure holds an
  `OpenAiEngine`-derived chunk transcriber: PCM chunk → in-memory WAV (16 kHz
  mono f32 → 16-bit PCM WAV bytes — a tiny writer in core; no ffmpeg per chunk,
  deviation ledger #L13: Buzz writes a temp WAV file via soundfile, humm builds
  the same container in memory — same bytes-on-the-wire shape, no disk churn) →
  multipart `/audio/transcriptions` with the rolling prompt as `prompt`, under
  `cancelable` + the engine's timeout. Language/task map exactly as the file
  engine's (F-041 request builder reused).
- **Realtime-fit warning**: the measured-speed map is keyed by model size — API
  sessions record under a backend-qualified key (e.g. `openai_api`) so a slow
  network doesn't poison the local sizes' numbers (✚ detail, ledger #L14).
- **ADR-021 perf ticket (falls due)**: `WhisperEngine::transcribe_live`
  (`engine.rs:155`) creates a fresh state — reloading the CoreML encoder — per
  chunk (`engine.rs:186`; ADR-021's traced mechanism). Change: the engine gains
  a live-session state handle created once per session and reused across
  `transcribe_live` calls (whisper.cpp states are reusable across `full()`
  runs); teardown with the session. Measured before/after with the existing
  `HUMM_SMOKE_LIVE_FIXTURE` acceptance path; the ratio lands in
  `docs/benchmarks/M7.md` and ADR-021 gets an addendum note. Regression guard:
  the existing session/merge unit suites must stay green (the change is inside
  the engine adapter, behind the same `EngineFn`).
- F-061's matrix row note is rewritten: scope cut closed, verification =
  existing suites + a scripted-API-arm kittest (mock server) + the fixture
  acceptance run.

---

## 11. CLI & `--json` surface (additive only)

- `{"event":"skipped","task_id":…}` and
  `{"event":"post_processed","task_id":…,"notes_updated":…,"segments_replaced":…,"written":[…],"errors":[…]}` —
  additive events, documented in `HUMM_SURFACE.md` §3 with the ADR-018-style
  "additive, may interleave" note for `post_processed` (it follows `completed`).
- Human mode: skipped prints `skipped (already transcribed)` on the task's
  line; `post_processed` prints nothing unless `errors` is non-empty
  (one stderr warning per error).
- Exit codes: skipped counts as success (§2.3). No new flags — folder watch and
  the post-hooks are settings-driven; the CLI runs hooks because the settings
  say so (Buzz CLI parity, `main_window.py:459-473`'s sync arm).

---

## 12. Testing & acceptance

**Per-feature** verification is specified in each section; cross-cutting:

- Gates on every commit: `cargo fmt --check`, `clippy -D warnings`,
  `cargo test --workspace` (sandboxed).
- **Sandbox TCP baseline** grows only with documented cause: ai_summary's
  local-mock HTTP tests and the live-API-arm mock tests join the lifted set;
  the milestone report restates the new count against M6's 50 humm-core +
  1 humm-cli.
- **Folder-watch integration test** (acceptance gate): §3.5's drop → auto task →
  mirrored output, plus the move/delete disposition assertions.
- **DOCX snapshot** (acceptance gate): §7's insta suite.
- **gui-manual v5** (`docs/gui-manual/M7.md`): per-tab windowed checks (General
  new rows incl. live locale + font size, Shortcuts capture + conflict + live
  re-apply, Folder Watch end-to-end with a real folder, Post-processing config,
  Models badge), the pill, live-API recording (mock or real endpoint as
  available), carried pointer-deferred items from v1–v4.
- **Benches**: no transcription-path regression — re-baseline against a same-day
  M6 checkout per `docs/benchmarks/M6.md`'s interleaved-pairs-under-load
  methodology; the live state-reuse change additionally reports its ratio
  improvement (§10). Post-hooks are off by default, so the bench path is
  untouched by construction — verified by call-path diff.
- **Matrix sweep** (acceptance): every row except F-027/F-087/F-112/F-114 at
  `implemented` or better; flips this milestone: F-030, F-100, F-101, F-110,
  F-111, F-113, F-115, F-116, **F-008 → `implemented`** under §9.2's recorded
  semantics (system complete, starter catalogs shipped, remaining 11 mechanical)
  (+ F-005/F-044 to full scope; F-061 note rewritten, §10).

---

## 13. Deviations ledger (M7)

| # | Deviation | Justification |
|---|---|---|
| L1 | No plugin mechanism: the four built-ins are native features configured in Preferences → Post-processing; no loader, pip installs, zip-URL install, enable-order list, or Plugins dialog/menu item | ADR-010 (approved Q-4); Buzz's own full-access warning names the liability |
| L2 | Fixed hook order (skip-check pre; resizer → summary → docx post) replaces Buzz's arbitrary discovery order + manual reordering | determinism; docx must see the resize; Buzz's default order was `os.listdir` accident |
| L3 | Post-hooks run synchronously on the queue runner thread in both binaries (Buzz: async thread in GUI, sync in CLI) | queue is serialized by design; bounded worst case (120 s summary timeout); zero marshaling machinery; ADR-018/event semantics untouched |
| L4 | Folder watcher enqueues absent-model tasks instead of downloading synchronously in the watcher | queue's non-modal model-download phase (UX J-1 ✚) already does it better |
| L5 | Auto-resizer drops Buzz's skip-if-source-audio-missing check | the check served stable-ts's unused audio argument; humm regroup is pure — resize works after folder-watch delete/move |
| L6 | ai_summary request is cancel-acknowledging (`cancelable`) | Buzz's is uninterruptible; ADR-022 machinery is free here |
| L7 | DOCX paragraph split routes through the shared paragraph-split helper (Buzz hardcodes 2000 ms in the plugin) | HUMM_SURFACE §5 one-helper consistency; identical output with env unset |
| L8 | Preferences window remains write-through-on-commit; no OK/Cancel | established humm pattern (M3/M6); Buzz itself OK-gates only the folder-watch group |
| L9 | ai_summary key in humm's own keyring scheme (service `humm`, entry `AI Summary API key`) vs Buzz's `plugin:ai_summary:api_key` | M6-established naming; fresh-setup isolation from Buzz's entries |
| L10 | UI language applies live — no restart, no "Restart required!" hint | every string resolves per frame through `tr()`; Buzz's restart was a gettext-at-import limitation |
| L11 | Live-export block + live-mode combo omitted from General | Buzz's own in-app note announces the move humm's M5 already made; two writers for one setting is a bug farm |
| L12 | "Reduce GPU RAM" (int8) omitted | consumers are the unported HF/Faster-Whisper backends (ADR-008); dead surface otherwise |
| L13 | Live-API chunks built as in-memory WAV, not temp files | same wire bytes; no disk churn; Buzz's temp file was a soundfile-API artifact |
| L14 | API live sessions record measured speed under a backend-qualified key | network speed must not poison local model-size numbers the realtime-fit warning reads |

---

## 14. Carried-ticket disposition (from `humm-m7-tickets`)

| Ticket | Disposition |
|---|---|
| Live transcription via API backend | **In scope** (§0 decision, §10) |
| Per-chunk `create_state()`/CoreML reload (ADR-021) | **Falls due** with §10; state reused per session, measured |
| F-100 General completion | In scope (§8.1) |
| Pipeline translation-drain poll-join (cancel mid-join) | **Taken** — pipeline.rs is touched anyway (§3.4): `TranslatorHandle::stop()`'s drain join gets the `finish_within`-shaped waiter (poll cancel every ~100 ms during the bounded join); the honest-bound comment updates |
| Translator client-build fallback timeout | **Taken** in §6.1's refactor commit |
| Viewer id→index map (`apply_translation_result`) | **Conditional**: taken by the first M7 task that edits `viewer.rs`; if none does, stays carried (current plan touches viewer only via shared shortcut reads — likely carried; re-checked at final review) |
| Env-gated `real_chat_api` contract / F-031 flip | Environment still lacks any OpenAI-compatible chat target — stays env-gated + `implemented`; ai_summary's twin ships the same way (§0, §6.3) |
| CLI second-SIGINT `finish_within` adoption | Not taken (ADR-022 records `exit(130)` as accepted); unchanged |
| Test-coverage minors / parity-cosmetic list ("Model:" label, encode-error prefix, eye-toggle commit) | Not in scope — carried unchanged to M8 planning |
| Manual/pointer-deferred list (gui-manual v1–v4 carries) | Folded into gui-manual v5's carried-items section (§12) |
| Dependency watch | Unchanged; **new watch entry: `notify` 8.x** (macOS FSEvents backend) added by §3.2 |
