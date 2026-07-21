# M3 — GUI shell (import → queue → progress → done) — Design

Status: approved for implementation (autonomous session; user pre-authorized the
brainstorm → plan → subagent-execution process in the milestone kickoff prompt).
Scope source: `docs/design/IMPLEMENTATION_PLAN.md` §M3. Behavior source: Buzz
source re-read 2026-07-21 (condensed notes in the session scratchpad; every
behavioral claim below was verified against `file:line` in Buzz v1.4.5).

Parity IDs in scope: F-001, F-003, F-004, F-005 (defaults), F-007, F-008
(scaffold), F-020, F-021, F-022 (GUI half), F-023, F-025, F-026 (full), F-044
(basic download/delete), plus the F-006 gui-manual half (toolbar notice).
Carried M2 tickets folded in: queue hardening (catch_unwind, stale canceled-set
id, explicit store-write-failure policy), single-instance handling, and
`HUMM_DISABLE_UPDATE_CHECK` actually read by the update-notify wiring.

## 1. New crate: `humm-gui`

eframe/egui 0.35 (ADR-006; version re-verified on crates.io 2026-07-21).
Binary → adapters → core, same dependency rule as `humm-cli`.

New dependencies (all pure Rust — no Standing-Policy ADR needed): `eframe`
0.35, `egui_extras` 0.35 (virtualized table), `rfd` 0.17 (native dialogs),
`fluent` 0.17 + `unic-langid` 0.9 (ADR-014), `open` 5 (reveal-in-Finder /
release page), `regex` 1 (Buzz's URL-validation regex, ported verbatim),
`fd-lock` 4 (single-instance; also added to `humm-cli`). Dev-dep:
`egui_kittest` 0.35 (AccessKit-driven GUI tests; also used by the scroll
bench harness).

Module layout:

```
crates/humm-gui/src/
  main.rs          # arg files (Finder open-with parity), locks, services, run_native
  app.rs           # HummApp: eframe::App; owns view state, drains events
  services.rs      # settings + store + locks + queue spawn + update check + telemetry
  events.rs        # subscriber thread → shared queue + request_repaint
  viewmodel.rs     # TasksVm: rows, sorting, selection, event application
  i18n.rs          # fluent bundle (en), t!() helper           (F-008 scaffold)
  shortcuts.rs     # 20 Buzz-default shortcuts + parser        (F-005 defaults)
  format.rs        # status text, ETA, timedelta (Buzz formats) (F-025)
  views/
    main_window.rs # menu bar, toolbar, empty-state hero, drop overlay, toasts, banner
    task_table.rs  # virtualized table, headers/sort, context menus  (F-003)
    options_sheet.rs # per-import options sheet                 (F-023)
    url_dialog.rs  # Import URL                                 (F-022)
    rename_notes.rs# rename + notes modals                      (F-026)
    preferences.rs # Preferences window: Models + Privacy tabs  (F-044, ADR-009)
    about.rs       # About window: version, manual check, show logs
    update_notice.rs # toolbar chip + update dialog             (F-006)
```

`HummApp` follows ARCHITECTURE §7: view-models hold UI state only; domain
state lives in core/store; repaint only on events (`request_repaint` from the
subscriber thread) — idle CPU ≈ 0.

## 2. Core/queue changes (humm-core, humm-store)

### 2.1 Non-modal model acquisition (UX §J-1 ✚)

The CLI resolves models synchronously before enqueueing (Buzz `cli.py:219-221`
parity) and keeps doing so. The GUI must never block imports on a download, so
model acquisition moves *into the task run* for the GUI:

- New core trait:
  ```rust
  pub trait ArtifactEnsurer: Send + Sync {
      fn ensure_model(&self, model: &ModelSpec,
          progress: &mut dyn FnMut(DownloadProgress), cancel: &CancelToken)
          -> Result<PathBuf, CoreError>;
      fn ensure_vad(&self, progress: &mut dyn FnMut(DownloadProgress),
          cancel: &CancelToken) -> Option<PathBuf>;   // failure degrades, never fails
  }
  ```
- `RunnerDeps.artifacts: Option<Arc<dyn ArtifactEnsurer>>`. When present, the
  runner (a) ensures the model at the start of each job, emitting the new
  event kind below; (b) re-resolves `tuning.vad_model` per job via
  `ensure_vad` (cached file = instant; failure = VAD silently off, logged —
  same degradation contract as the CLI's `ensure_vad`). When absent
  (CLI), behavior is byte-for-byte unchanged.
- `EngineFactory::create` gains the resolved path:
  `create(&mut self, model: &ModelSpec, resolved: Option<&Path>)`. The CLI
  factory ignores `resolved` (it pre-resolved); the GUI factory requires it.
- New event kind:
  `TaskEventKind::ModelDownloadProgress { file: String, downloaded: u64, total: Option<u64> }`
  (per-task, between `Started` and `Progress`). The GUI renders it as the
  "Downloading model (NN%)" phase in the task's own row; queued tasks behind
  it show plain `Queued`. This satisfies §J-1 ✚'s guarantees (absent model
  never blocks imports; download progress visible in the table); the
  UX_SPEC's "Waiting for model…" *label* is realized as the running task's
  download phase — UX_SPEC.md will be updated to the as-built wording (living
  doc, same commit).
- Engine cache behavior is unchanged (keyed on `ModelSpec`); ensure is a
  sentinel check when the artifact is already cached.

### 2.2 Queue hardening (carried tickets)

- **catch_unwind around the job** — `run_task` wraps `run_job` in
  `std::panic::catch_unwind(AssertUnwindSafe(..))`. A panic maps to
  `Terminal::Failed("internal error: task panicked: <payload> — see the log file")`
  and discards `engine_cache` (an engine that panicked is not reused). The
  queue keeps running; unit-tested with a panicking fake engine.
- **Stale canceled-set id** — `run_task` removes the finished task's id from
  the canceled-set after its terminal write, so canceling the *current* task
  no longer leaks one id forever. Unit test: cancel current task → set is
  empty after it terminates (asserted via a subsequent same-id retry running
  normally).
- **Store-write-failure policy (explicit)** — decision: a store write failure
  still never aborts a run (finishing the transcription beats a DB hiccup;
  the terminal state is decided by the pipeline outcome). What changes: every
  failed store write now *emits*
  `TaskEventKind::StoreWriteFailed { operation: String, detail: String }`
  in addition to the log line. Consumers: GUI shows a persistent dismissible
  banner ("humm couldn't record history updates — exports still complete;
  see the log file"); CLI human mode prints one stderr warning per failure;
  `--json` emits `{"event":"store_write_failed","task_id":…,"operation":…,"detail":…}`
  (additive; HUMM_SURFACE §3 updated). The policy is documented in
  `queue.rs`'s module docs and recorded as ADR-018.

### 2.3 Retry / rename / notes / delete (F-026 completion)

- `TaskStore` gains `requeue_task(&id)` — Buzz's
  `reset_transcription_for_restart` (`transcription_dao.py:353-370`): same
  row, `status='queued'`, `progress=0`, `time_started/time_ended/error_message`
  NULL. New `Command::Retry { id, spec }` + `QueueHandle::retry(id, spec)`:
  the runner calls `requeue_task` instead of `create_task`, then emits
  `Queued` and appends to the FIFO.
- Retry spec source: `SqliteStore::load_task_spec` (full persisted options —
  fixes Buzz's restart-loses-options trap, UX §J-10 ✚). For imported Buzz
  rows (`options_json` NULL), a new
  `SqliteStore::spec_from_columns(&id)` rebuilds the spec from row columns
  exactly as Buzz's `_restart_transcription_task` does (language, task, size
  with tiny fallback, word timings, export formats; prompt/LLM fields default)
  — Buzz-parity restart for Buzz-imported history.
- GUI gating mirrors Buzz: Restart offered only for `failed`/`canceled`/
  `skipped` (completed is *not* restartable — `transcription_tasks_table_widget.py:696-725`).
- `SqliteStore` (not the `TaskStore` trait — these are UI concerns, not queue
  transitions): `set_task_name(&id, &str)`, `set_task_notes(&id, &str)`,
  `delete_tasks(&[TranscriptionId])` (single DELETE; segments cascade via the
  existing FK + `foreign_keys=ON`).
- Deletion is gated to terminal statuses **everywhere** (toolbar and Delete
  key). Buzz's Delete-key path bypasses its own gate and can delete an
  in-progress row mid-run with no cancel (`main_window.py:232-255` reached
  from `keyPressEvent:587-600`) — that is a defect, not a behavior to port;
  deviation noted in the matrix row.

### 2.4 Single-instance handling (carried ticket, ADR-017)

Two `fd-lock` files in `paths::data_dir()`:

- `instance.lock` — **stale-flip right.** Every humm process (CLI `add`, GUI)
  at startup: try-exclusive; success ⇒ no other humm process is alive ⇒ run
  `cancel_stale_tasks()`, then downgrade to shared (held for process
  lifetime). Failure ⇒ acquire shared and **skip** the stale flip (another
  process is alive; its `in_progress` rows are real). This closes the
  HUMM_SURFACE §7 race (second process flipping the first's live row) without
  forbidding concurrent `humm add` — both processes keep working, each on its
  own rows. (The downgrade gap — drop write, take read — is benign: a process
  that wins exclusive in that gap flips only rows that predate both.)
- `gui.lock` — GUI exclusivity. Second GUI launch: native message dialog
  ("humm is already running.") and exit 0. Buzz has no guard (F-001) — logged
  as a ✚ deviation; a second Buzz instance silently corrupts shared state.

Implementation lives in `humm-core::paths`/new `humm-core::instance` (pure
std/fd-lock, no adapter needed), consumed by both binaries; HUMM_SURFACE §7
updated in the same commit.

### 2.5 Telemetry (F-007, ADR-009)

- `humm-core::telemetry`: `trait Telemetry { fn capture(&self, event: Event) }`,
  `Event::app_launched(props)` with Buzz's property keys
  (`application.py:80-93`): `app` (humm version), `system`, `release`,
  `machine`, `version` (OS build via `sw_vers -productVersion`, best-effort),
  `locale` (from `LC_ALL`/`LANG`). `PostHogTelemetry` posts
  `https://us.i.posthog.com/capture/` with humm's own key; `NoopTelemetry`
  otherwise.
- Noop when any of: `HUMM_POSTHOG_KEY` absent at build (`option_env!`), debug
  build, `HUMM_DISABLE_TELEMETRY` env set, or the Privacy toggle off.
  Fire-and-forget thread, short timeout, no retries, silent offline drop.
- `distinct_id`: `settings.privacy.user_identifier` — uuid4 generated and
  persisted on first GUI run (Buzz `user-identifier` parity).
- Unit tests: payload shape, every noop gate. No network in tests.

### 2.6 Settings additions (`humm-core::settings`, ADR-012)

```toml
[gui]                      # window/table chrome
task_table_visible = [..]  # column ids; file_name not hideable
task_table_order  = [..]
task_table_sort   = { column = "date_added", descending = true }

[file_transcriber]         # sticky options-sheet state (Buzz file_transcriber/*)
last_import_folder, language, task, model_size, hf_repo,
word_timestamps, initial_prompt, export_formats = ["srt", ...]

[privacy]
telemetry_enabled = true   # default on (Buzz parity: opt-out)
user_identifier   = "<uuid4>"

[updates]                  # existing last_check/available_version, plus:
check_enabled = true
```

Column *widths* persist through eframe's own persistence (egui memory,
`persistence_path` = `config_dir()/gui.ron`), which also covers window
geometry (F-001). Buzz persisted widths explicitly; same user outcome.

## 3. GUI feature designs (Buzz-sourced specifics)

### 3.1 F-001 launch
Window title "humm"; default size 1240×600 (Buzz `main_window.py:550-558`);
geometry restored via eframe persistence; theme follows the system
(dark-mode parity — Buzz detects once at startup, humm tracks live via egui;
strictly better). Startup order mirrors the CLI: settings → locks → store →
Buzz import (once) → stale flip (lock-gated) → queue spawn → update check →
telemetry. Any init failure renders an error screen with the actionable
cause + a Quit button — never a panic. Files passed as argv are treated as
an import (Finder "open with" ≈ drag-drop path).

### 3.2 F-003 table
Columns exactly Buzz's (`transcription_tasks_table_widget.py:109-183`):
File Name/URL (400, not hideable; shows `name` else file basename else URL),
Model (180), Task (120, "Translate to English"/"Transcribe"), Status (180),
Date Completed (180, `%Y-%m-%d %H:%M:%S`), Date Added (180), Notes (300).
Default sort Date Added desc; click-to-sort with indicator; sort persisted.
Header right-click: "Reset Column Order" + visibility toggles (file_name
excluded). Row right-click: Restart (status-gated) · Rename · Add/Edit Notes.
Keys: Delete → clear-selected (confirm); Ctrl/⌘+C → copy file path or URL
per selected row, newline-joined (Buzz concatenates raw `file`+`url` with a
literal Python `None` when absent — defect, not ported); Return → viewer
(M4; disabled with tooltip in M3). Multi-select (click / ⌘ / shift).
Rendering: `egui_extras::TableBuilder` with `body.rows(...)` virtualization —
only visible rows are laid out; the 100k bench (§5) guards it.

### 3.3 F-004 toolbar & menus
Toolbar (icon buttons, tooltips show shortcut): Record (disabled, "arrives
with live recording") · New File Transcription · New URL Transcription ·
Open Transcript (disabled in M3, "viewer arrives in the next milestone") ·
Cancel Transcription · Clear History · [Update Available chip, hidden].
Enablement rules verbatim from `main_window.py:350-382`: Open ⇔ all selected
∈ {completed, skipped}; Cancel ⇔ all ∈ {in_progress, queued}; Clear ⇔ all ∈
{completed, failed, canceled, skipped}; empty selection disables all three.
Menus (in-window egui bar — egui provides no native macOS menu bar; parity
of function, noted): File → Import File… (⌘O), Import URL… (⌘U), Import
Folder…; Help → About humm, Help (online docs), Preferences… (⌘,). Buzz's
"Plugins…" entry is not ported (ADR-010).

### 3.4 F-020/F-021/F-022 import paths
- File dialog: `rfd`, filter groups exactly Buzz's four
  (`transcriber.py:222-225`, 13 extensions + All files); starts at persisted
  `last_import_folder`; persists the picked file's parent.
- Drag-drop: egui `raw_input.dropped_files`, no extension filter (Buzz
  parity), hover overlay; empty-table hero "Drop media here, or ⌘O" ✚.
- URL dialog: Buzz's regex ported verbatim (`import_url_dialog.py:12-14`),
  full-match; invalid → inline "The URL you entered is invalid." (sheet stays
  open — egui modal instead of a nested error box).
- Folder import: `rfd` directory picker → recursive walk (std, no new dep),
  `ext.to_lowercase() ∈` the 13-extension set, hidden files included (Buzz
  `os.walk` parity), zero matches → silent return, persists the folder itself.
- All import paths converge on the options sheet.

### 3.5 F-023 options sheet
egui modal sheet over the main window (UX §1 ✚, replaces Buzz's orphan
window). Contents (Buzz `transcription_options_group_box.py` /
`file_transcription_form_widget.py`, collapsed per ADR-008 — no backend
combo; model-size choice only):
- Header: import summary ("3 files" with expandable list, or the URL) + the
  one-line last-used summary ("Whisper · small · auto language · SRT+TXT") ✚.
- Model size combo: 12 sizes title-cased; "Custom" appears when
  `models.custom_model_url` is set (Buzz gate parity); "Lumii" hidden outside
  lv locale (Buzz gate; M3 is en-only ⇒ hidden).
- Task combo: "Translate to English", "Transcribe" (Buzz order).
- Language combo: "Detect Language" + favorites from
  `HUMM_FAVORITE_LANGUAGES` (comma codes, separators) + all languages
  alphabetical by English name (`languages_combo_box.py` parity;
  `Language::all()`).
- "Word-level timings" checkbox; "Extract speech" checkbox disabled with
  tooltip (M8); "Export:" TXT/SRT/VTT checkboxes.
- Advanced (collapsing): "Initial Prompt" multiline; "Hugging Face model"
  free-text (maps to `ModelSpec::hf_repo`; plain field — Buzz's live
  HF-autocomplete deferred, noted as a deviation pending M7 polish). Buzz's
  AI-translation fields arrive with F-031 (M6), OpenAI key field with M6.
- Run (default, Enter) enqueues one task per file / one URL task with shared
  options (`main_window.py:207-230` parity), persists sticky state, closes.
  No model pre-resolution — absent models download inside the task (§2.1).
  Esc/Cancel closes without enqueueing (sticky state still saved — Buzz
  saves on close, `file_transcriber_widget.py` closeEvent parity).

### 3.6 F-025 progress & ETA (GUI half)
Status texts verbatim from `format_record_status_text` (:67-107):
`Queued` · `In Progress (NN%)` · `In Progress (NN%) — ETA: 3m 42s` (formula
`remaining = elapsed×(1−p)/p`, only when elapsed > 2 s and 0<p<1; `Xm Ys` /
`Ys`) · `Completed (3m 5s)` (timedelta from real elapsed seconds — Buzz's
`delta.seconds` wraps at 24 h, defect not ported) · `Failed (<message>)` ·
`Canceled` · `Skipped`. Phase prefixes ✚ (UX §J-1): "Downloading model
(NN%)" from `ModelDownloadProgress`, "Downloading media (NN%)" from
`DownloadProgress` (Buzz's table never showed download progress — its
handler is a no-op TODO, `main_window.py:429-433`). The CLI's `format_eta`
moves to `humm-core` (shared with the GUI, exact behavior pinned by existing
tests). In-progress rows repaint on events; a 1 s repaint tick runs only
while a task is in progress (ETA/elapsed advance; idle stays 0).

### 3.7 F-026 actions (GUI)
Cancel: toolbar/⌘X → `handle.cancel(id)` per selected (prompt ack via token).
Restart: context menu on failed/canceled/skipped → §2.3 retry. Rename: modal,
title "Rename Transcription", prefilled with name/url/basename, trimmed
non-empty saves. Notes: modal multiline, empty allowed; double-click Notes
cell also opens it (Buzz parity). Clear History: toolbar/⌘S/Delete-key →
confirm "Are you sure you want to delete the selected transcription(s)?
This action cannot be undone." (Ok/Cancel) → delete selected rows only.
Terminal-status gating everywhere (§2.3 deviation note).

### 3.8 F-006 GUI half (ADR-015) + About
Startup wiring in `services.rs`: construct always; skip the check when
`HUMM_DISABLE_UPDATE_CHECK` is non-empty (env read exactly at the Buzz gate
point, `main_window.py:569` parity — the carried ticket) or the Privacy
update toggle is off or `should_check` says not due. Background thread:
`check_for_update(VERSION_INFO_URL, CARGO_PKG_VERSION)`; persist
`last_check` (now, `%Y-%m-%dT%H:%M:%S%.f`) *even on error* (Buzz writes
before its error branch); on newer → persist `available_version`, show
toolbar chip; on up-to-date → clear `available_version`. Chip click → update
dialog: current vs new version, release notes (plain text scroll — no
markdown renderer dep; deviation noted), "Open release page" via `open`
(notify-only, ADR-015). About window: version, manual "Check for updates"
(same logic, bypasses the 7-day gate like Buzz's About) → "You're up to
date!" or the update dialog (humm uses one code path — Buzz's second
GitHub-API path + naive string compare is a defect, not ported), "Show logs"
→ opens `paths::log_dir()`.

### 3.9 F-044 basic (Preferences → Models)
Preferences window (real OS window via deferred viewport — multi-window
smoke target), min 650×500, tabs **Models** and **Privacy** (General/
Shortcuts/Folder Watch arrive M7). Models tab, collapsed to the single
whisper-rs backend (ADR-008 — no group combo): two sections "Downloaded" /
"Available for Download" listing sizes (custom row per §3.5 gate; lumii
lv-gated); per row: disk size when downloaded; actions Download (background
thread, inline progress + cancel, via `ModelDownloader::resolve_model`),
Delete (confirm "Delete Model" / "Are you sure you want to delete the
selected model?"; removes ggml + sentinel + CoreML encoder dir), Show file
location (reveal in Finder). Custom ggml URL field persists
`models.custom_model_url`. Buzz's FAQ link points at Buzz's own docs — not
ported.

### 3.10 Privacy tab (stub, ADR-009 / UX §J-11)
Telemetry toggle + plain-English list of exactly what one `app_launched`
event contains (the six property keys, spelled out); update-check toggle.
Both persist immediately (Buzz preferences apply live).

### 3.11 F-005 defaults + F-008 scaffold
`shortcuts.rs`: all **20** Buzz actions (source count; docs said 21 —
analysis correction in FEATURE_INVENTORY/UX_SPEC/EXTERNAL_SURFACE, same
commit) with Buzz's default sequences; parser "Ctrl+Shift+P" →
`egui::KeyboardShortcut` (Ctrl ⇒ ⌘ on macOS, Buzz/Qt parity), unit-tested
(the F-005 matrix method). Overrides read from `[shortcuts]` settings map
(editor UI is M7). M3 binds: ⌘O, ⌘U, ⌘,, ⌘S (confirm-guarded Clear —
UX §3), ⌘X; viewer/record keys defined but dormant.
`i18n.rs`: fluent, embedded `en/humm.ftl`, every UI string keyed; unit test
loads the catalog and resolves every key the code references (F-008 matrix
method: unit catalog load). Locale negotiation lands with locale ports (M7+).

## 4. Event flow

`services.rs` spawns the queue with real adapters (ffmpeg/ffprobe/yt-dlp
probed at startup → `Capabilities` gates URL-import UI with tooltip, Policy
rule 6). A subscriber thread moves `TaskEvent`s into `Mutex<Vec<TaskEvent>>`
and calls `ctx.request_repaint()`. `HummApp::update` drains, applies to
`TasksVm` (Queued → `store.get_task(id)` insert; progress/download/model
events → in-place; terminal → row refresh from store + toast). Toasts:
completed ("Transcription complete — N files written"), failed (one-line
cause; click → detail popover with full error + log link + Retry, UX §J-1).

## 5. Verification

- **Unit/integration**: queue (retry, panic containment, canceled-set,
  StoreWriteFailed, ensurer events, VAD degradation), store (requeue, delete
  cascade, name/notes, spec_from_columns), settings round-trip, telemetry
  gates/payload, shortcut parser, URL regex port (accept/reject vectors),
  status/ETA formatting vs Buzz strings, sort comparators.
- **egui_kittest**: empty-state hero; import → sheet → Run → row appears
  (fake engine factory + temp store); selection-based enablement matrix;
  context-menu gating; rename/notes/clear flows; update chip on injected
  result; options-sheet stickiness. These double as the automatable half of
  the gui-manual checklist.
- **Benches**: new `humm-gui` criterion bench — headless egui Context
  rendering the real task-table at 100k rows across ≥120 scrolled frames;
  acceptance p95 frame CPU < 16.6 ms (≥60 fps; GPU excluded — CPU is the
  egui-controllable cost, noted in the report). Existing decode/e2e/export
  benches re-run — no transcription-path regression allowed.
- **gui-manual v1**: `docs/gui-manual/M3.md` checklist (per-feature steps +
  expected), each item marked kittest-automated or manually executed; J-1
  end-to-end on a real file with a real model (unsandboxed run), multi-window
  smoke (main + Preferences + About as OS windows) on macOS.

## 6. Deviations ledger (for matrix/ADR notes)

1. In-window menu bar, not native macOS menu bar (egui limitation; function
   parity).
2. Delete/Clear gated to terminal statuses on every path (Buzz's Delete-key
   gate bypass is a defect).
3. Copy cell: path-or-URL, newline-joined (Buzz emits literal "None"
   artifacts).
4. Completed-duration uses real elapsed time (Buzz wraps at 24 h).
5. Restart reuses persisted full options (UX ✚; Buzz loses prompt/LLM/token).
6. Non-modal model downloads as in-task phase (UX §J-1 ✚; UX_SPEC wording
   updated to as-built).
7. HF model field is plain text in M3 (Buzz has live autocomplete).
8. Release notes rendered as plain text (no markdown dep).
9. Single-instance guard added (ADR-017; Buzz has none).
10. About's manual check reuses the one update code path (Buzz's second
    endpoint + naive compare is a defect).
11. Buzz "Plugins…" menu not ported (ADR-010); Record/Open Transcript
    present-but-disabled until M5/M4.
12. `humm-gui` accepts file arguments as an import request (Finder
    open-with; superset, drag-drop-equivalent).

## 7. Analysis corrections queued

- F-005: Buzz defines **20** shortcuts, not 21 (recount at implementation
  time from `shortcut.py`) → FEATURE_INVENTORY, UX_SPEC §3, EXTERNAL_SURFACE.
- HUMM_SURFACE §7: rewrite after ADR-017 lands (concurrent `humm add` now
  supported without the stale-flip race).
- UX_SPEC §J-1: "Waiting for model…" → as-built phase wording (§2.1).
