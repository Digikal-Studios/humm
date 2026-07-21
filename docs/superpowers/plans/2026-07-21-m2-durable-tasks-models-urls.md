# M2 — Tasks are durable; models are managed; URLs import (CLI complete) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `humm add` becomes a durable, complete CLI: every task persists to a humm-owned SQLite store (kill -9 mid-task → relaunch shows Canceled with fully retryable options), a FIFO task queue with an event bus runs jobs, URLs import via a yt-dlp adapter, custom models work (ggml URL + HF Whisper-family repo id), the model downloader gains an HTTP seam + CoreML 416 recovery (M1 review tickets), update-check logic lands, and the full Buzz `add` grammar (incl. `-m`, `--hfid`, `-e`, `--openai-token`, URLs, `--json`, exit codes, Ctrl-C cancel) is CLI-integration-tested.

**Parity IDs:** F-022 (URL half), F-024, F-025 (CLI), F-026 (cancel), F-029, F-042/F-043 (full), F-006 (check logic), F-002 (full grammar).

**Architecture:** New adapter crate `humm-store` (rusqlite bundled, WAL; ADR-004) implementing a core-defined `TaskStore` trait; task queue + `TaskEvent` bus in `humm-core` (dedicated runner thread, flume channels; ARCHITECTURE §3); `MediaFetcher` trait in core + yt-dlp subprocess adapter in `humm-media` (ADR-016); settings as serde TOML (ADR-012); CLI rewired to run everything through store+queue (parity by construction with the M3 GUI).

**Tech Stack (additions):** rusqlite 0.40 (`bundled`) · uuid 1 (v4+serde) · url 2 (serde) · flume 0.11 · toml 0.9 · ctrlc 3.

## Global Constraints

Carried from M1 (all still binding) plus M2 additions. Every task's requirements implicitly include all of these.

1. **Gates — every commit must pass** (run all three before each `git commit`):
   ```bash
   export CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home"
   cargo fmt --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
2. **Sandbox:** ALL `cargo` invocations need `CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home"`. **GPU/Metal model loads SEGFAULT under sandboxed Bash** — any `#[ignore]` test or bench that loads a real whisper model must be run with the sandbox lifted (ask the orchestrator; document in your report which ignored tests you ran and how).
3. **Ground truth:** Buzz source at `/Users/jawad/RustroverProjects/buzz` is the sole authority. Each task cites exact Buzz files; **read them before implementing**. If source contradicts this plan, source wins — report the discrepancy. Genuine ambiguity → `docs/OPEN_QUESTIONS.md`, never invented behavior.
4. **Buzz testdata:** env `HUMM_BUZZ_TESTDATA`, default `/Users/jawad/RustroverProjects/buzz/testdata`. **`ggml-tiny.bin` there is a 575 KB stub** — it loads and produces zero segments; usable as a fast fake for store/queue/CLI integration tests that don't assert transcript content, NEVER for real transcription assertions.
5. **Errors:** `thiserror` in library crates, `anyhow` only in `humm-cli`. No `unwrap`/`expect` outside `#[cfg(test)]`/documented-impossible. Messages actionable ("X missing — do Y").
6. **`humm-core`:** `#![warn(missing_docs)]`, `#![forbid(unsafe_code)]`, zero UI/adapter deps. `humm-store` is an adapter: it may name rusqlite, nothing else non-Rust; `#![forbid(unsafe_code)]`.
7. **Style:** newtypes, iterators over index loops, exhaustive `match` on domain enums, doc comments on all public core/store items.
8. **Tests:** unit tests inline; tests needing external binaries/models/network are `#[ignore]` with a doc comment naming the prerequisite. No network and no real user directories in non-ignored tests — every path the code touches must be injectable (temp dirs via parameters/env seams).
9. **Env var surface (humm-owned):** M1's set (`HUMM_MODEL_ROOT`, `HUMM_FFMPEG`, `HUMM_FFPROBE`, `HUMM_LOG_DIR`, `HUMM_FORCE_CPU`, `HUMM_WHISPER_N_THREADS`, `HUMM_PARAGRAPH_SPLIT_TIME`, `HUMM_BUZZ_TESTDATA`) plus M2: `HUMM_CONFIG_DIR`, `HUMM_DATA_DIR`, `HUMM_YTDLP`, `HUMM_DOWNLOAD_COOKIEFILE`, `HUMM_DISABLE_UPDATE_CHECK`. Binaries read env; core functions take values as parameters (same seam pattern as M1's `paths`).
10. **Dependencies are a liability:** only crates named in a task may be added; extras need a one-line justification in your report. New M2 versions (verify against crates.io; adjust patch-level if resolution fails, note it): `rusqlite = { version = "0.40", features = ["bundled"] }`, `uuid = { version = "1", features = ["v4", "serde"] }`, `url = { version = "2", features = ["serde"] }`, `flume = "0.11"`, `toml = "0.9"`, `ctrlc = "3"`.
11. **Commits:** coherent boundaries, `M2: <what> (F-###[, F-###])`, Claude co-author line per harness rules.
12. **Timestamps** are integer milliseconds (`Millis(i64)`). DB timestamp strings are naive-local ISO-8601 (`%Y-%m-%dT%H:%M:%S%.f`) matching Buzz's `datetime.now().isoformat()`.
13. **Docs are living:** implementation discovering an analysis/design error fixes the doc in the same commit and says so in the report.
14. **Wire-value law:** every string that crosses a boundary (DB columns, `options_json`, `--json` events, CLI flag values) uses the exact Buzz wire values where a Buzz equivalent exists (`Status`: `queued|in_progress|completed|failed|canceled|skipped`; `Source`: `file_import|url_import|folder_watch`; `Task`: `transcribe|translate`; model sizes per `ModelSize::as_str`; `ModelType` column strings: `Whisper.cpp` / `Hugging Face`).

---

### Task 1: Core groundwork — ids, Status/Source, Input/TaskSpec, serde wire format

**Files:**
- Modify: `crates/humm-core/src/domain.rs`, `crates/humm-core/src/language.rs`, `crates/humm-core/Cargo.toml` (deps: serde, serde_json, uuid, url), `Cargo.toml` (workspace deps: uuid, url)
- Test: inline `#[cfg(test)]` in `domain.rs`/`language.rs`

**Buzz sources to read first:** `buzz/transcriber/transcriber.py:177-188` (`Status`, `Source` wire values), `buzz/cli.py:48-50` (`is_url` = scheme+netloc). Design: `docs/design/ARCHITECTURE.md` §4.

**Interfaces (Produces — later tasks use these exact names):**
```rust
// domain.rs
pub struct TranscriptionId(uuid::Uuid);
//   new() -> Self (v4); parse(&str) -> Option<Self>; Display = hyphenated lowercase;
//   Serialize/Deserialize as that string; Clone+Copy+Debug+PartialEq+Eq+Hash
pub enum Status { Queued, InProgress, Completed, Failed, Canceled, Skipped }
//   as_str(): "queued","in_progress","completed","failed","canceled","skipped"; FromStr exact inverse
pub enum Source { FileImport, UrlImport, FolderWatch, LiveRecording }
//   as_str(): "file_import","url_import","folder_watch","live_recording" (last is humm's Q-10 addition,
//   unused until M5 — document that in its doc comment); FromStr exact inverse
pub enum Input { Path(std::path::PathBuf), Url(url::Url) }
//   parse(s: &str) -> Input : url::Url::parse ok AND url.has_host() => Url, else Path (Buzz is_url = scheme+netloc)
//   display_name() -> String : file stem-less full path display for Path, full URL string for Url
pub struct TaskSpec { pub input: Input, pub options: TranscriptionOptions, pub output: OutputSpec }
// ModelSpec gains an optional HF repo id (used when the CLI passes `-m huggingface --hfid X`):
pub struct ModelSpec { pub size: ModelSize, pub hf_repo: Option<String> }   // hf_repo None for plain sizes
```
- Serde on: `Millis` (as i64), `Segment`, `TaskKind`, `OutputFormat`, `ModelSize`, `Status`, `Source`, `ModelSpec`, `TranscriptionOptions`, `OutputSpec`, `Input`, `TaskSpec`, `Language` (as its code string). Enum representations = the `as_str` wire values (via `#[serde(rename_all)]` where it matches exactly, else manual `Serialize`/`Deserialize` impls through `as_str`/`FromStr` — `ModelSize` needs manual impls because of `"tiny.en"`/`"large-v3-turbo"`). `Input` serializes tagged: `{"path": "..."}` / `{"url": "..."}`.
- `ModelSpec` change ripples: `TranscriptionOptions` construction sites (`domain.rs` Default, `humm-cli/src/run.rs`, pipeline tests) gain `hf_repo: None`.

**Steps:**

- [ ] **Step 1 (TDD): tests first.** Required cases:
  - `Status`/`Source` round-trip through `FromStr` for every variant; wire strings match Buzz exactly (assert each literal).
  - `TranscriptionId::new()` twice → distinct; `Display` round-trips through `parse`; `parse("not-a-uuid")` → `None`.
  - `Input::parse("https://example.com/a.mp3")` → `Url`; `Input::parse("ftp://host/x")` → `Url`; `Input::parse("audio.mp3")`, `Input::parse("/abs/path.mp3")`, `Input::parse("a:b")` (scheme, no host) → `Path`.
  - Serde round-trip: a full `TaskSpec` (Url input, language Some(fr), all options non-default) → JSON → back == original. Spot-assert the JSON contains `"task":"transcribe"`, `"large-v3-turbo"` style wire values (serialize `ModelSize::LargeV3Turbo` and `ModelSize::TinyEn` explicitly).
  - `Language` serializes as its bare code string (`"fr"`), deserializes back; unknown code fails to deserialize with a message naming the code.
- [ ] **Step 2:** Implement until green. Run the gates.
- [ ] **Step 3: Commit** `M2: core ids, Status/Source, Input/TaskSpec, serde wire format (F-024, F-029 groundwork)`.

---

### Task 2: Settings file (TOML) + config/data paths

**Files:**
- Create: `crates/humm-core/src/settings.rs`
- Modify: `crates/humm-core/src/paths.rs`, `crates/humm-core/src/lib.rs`, `crates/humm-core/Cargo.toml` (dep: toml), `Cargo.toml` (workspace dep: toml)

**Buzz sources to read first:** `buzz/settings/settings.py` (key catalog; humm ports *semantics* not QSettings — ADR-012), `docs/analysis/EXTERNAL_SURFACE.md` §3.6. Design: ADR-012.

**Interfaces:**
```rust
// paths.rs additions (same seam pattern as model_root: pub fn reads env, inner fn takes Option<&OsStr>)
pub fn config_dir() -> PathBuf;     // HUMM_CONFIG_DIR override, else <platform config dir>/humm
pub fn data_dir() -> PathBuf;       // HUMM_DATA_DIR override, else <platform data dir>/humm
pub fn settings_path() -> PathBuf;  // config_dir()/settings.toml
pub fn db_path() -> PathBuf;        // data_dir()/humm.sqlite
pub fn media_dir() -> PathBuf;      // data_dir()/media   (URL-import downloads, Task 6/11)
pub fn buzz_sqlite_path() -> Option<PathBuf>; // platform data root/Buzz/Buzz.sqlite (macOS:
    // ~/Library/Application Support/Buzz/Buzz.sqlite) if it exists; inner fn takes the base dir for tests

// settings.rs — serde TOML, every field optional/defaulted (#[serde(default)] on all structs;
// unknown fields tolerated for forward compat; missing file => Settings::default())
pub struct Settings { pub models: ModelSettings, pub downloads: DownloadSettings, pub updates: UpdateSettings }
pub struct ModelSettings { pub custom_model_url: Option<String> }          // consumed by `-s custom` (Task 8/11)
pub struct DownloadSettings { pub cookie_file: Option<PathBuf> }           // yt-dlp cookies (Task 6/11)
pub struct UpdateSettings { pub last_check: Option<String>, pub available_version: Option<String> } // F-006 (Task 9)
impl Settings {
    pub fn load(path: &Path) -> Result<Settings, SettingsError>;  // missing file => Ok(default);
        // corrupt TOML => Err with actionable message (never silently clobber a user's file)
    pub fn save(&self, path: &Path) -> Result<(), SettingsError>; // atomic: temp file in same dir + rename;
        // creates parent dirs
}
pub enum SettingsError { /* thiserror; Io { path, source }, Parse { path, detail } */ }
```
Add `CoreError::Settings(#[from] SettingsError)`? No — settings errors never cross the pipeline; keep `SettingsError` standalone (binaries handle it).

**Steps:**

- [ ] **Step 1 (TDD):** tests: load of missing path → defaults; save→load round-trip (all fields Some); corrupt file → `Err` mentioning the path; save creates parent dirs and leaves no `*.tmp` sibling; unknown TOML keys are ignored on load; `paths` inner fns honor overrides and defaults end with `humm` (`humm/settings.toml`, `humm/humm.sqlite`, `humm/media`); `buzz_sqlite_path` inner fn returns None when the file doesn't exist.
- [ ] **Step 2:** Implement; gates.
- [ ] **Step 3: Commit** `M2: settings file (TOML) + config/data paths (ADR-012)`.

---

### Task 3: `TaskStore` trait + humm-store crate (schema v1, repository, startup cancel-flip)

**Files:**
- Create: `crates/humm-store/Cargo.toml`, `crates/humm-store/src/lib.rs`, `crates/humm-store/src/schema.rs`, `crates/humm-store/src/store.rs`
- Modify: `Cargo.toml` (workspace member + `humm-core` path dep centralized in `[workspace.dependencies]` — the M1 cosmetic ticket: each crate then uses `humm-core.workspace = true`), `crates/humm-core/src/boundary.rs` (`TaskStore` trait), `crates/humm-core/src/error.rs` (`StoreError` + `CoreError::Store`), `crates/humm-media/Cargo.toml`+`crates/humm-whisper/Cargo.toml`+`crates/humm-cli/Cargo.toml` (switch to workspace humm-core dep)

**Buzz sources to read first:** `buzz/schema.sql` (verbatim), `buzz/db/dao/transcription_dao.py` (transition semantics + which columns each transition touches), `buzz/db/service/transcription_service.py` (segments inserted only on completed/skipped), `buzz/db/helpers.py:79-89` (startup cancel-flip). Design: `docs/design/ARCHITECTURE.md` §5, ADR-004.

**Interfaces:**
```rust
// humm-core boundary.rs — the queue's writer surface (the ONLY writer of status; ARCH §5)
pub trait TaskStore: Send + Sync {
    fn create_task(&self, id: &TranscriptionId, spec: &TaskSpec, source: Source) -> Result<(), StoreError>;
    fn mark_started(&self, id: &TranscriptionId) -> Result<(), StoreError>;
    fn update_progress(&self, id: &TranscriptionId, fraction: f64) -> Result<(), StoreError>;
    fn update_file(&self, id: &TranscriptionId, path: &Path) -> Result<(), StoreError>; // URL download resolved
    fn mark_completed(&self, id: &TranscriptionId, segments: &[Segment]) -> Result<(), StoreError>;
    fn mark_failed(&self, id: &TranscriptionId, error: &str) -> Result<(), StoreError>;
    fn mark_canceled(&self, id: &TranscriptionId) -> Result<(), StoreError>;
}
// humm-core error.rs
pub enum StoreError { Open { path: PathBuf, detail: String }, Query { detail: String },
    Corrupt { detail: String }, Io(#[from] std::io::Error) }   // #[non_exhaustive], thiserror

// humm-store store.rs
pub struct SqliteStore { /* Mutex<rusqlite::Connection> */ }
impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self, StoreError>;   // creates parent dirs; WAL; foreign_keys ON; migrates
    pub fn open_in_memory() -> Result<Self, StoreError>;    // tests
    pub fn cancel_stale_tasks(&self) -> Result<usize, StoreError>; // queued|in_progress -> canceled + time_ended (F-029/F-001 edge)
    pub fn get_task(&self, id: &TranscriptionId) -> Result<Option<TaskRow>, StoreError>;
    pub fn list_tasks(&self) -> Result<Vec<TaskRow>, StoreError>;  // time_queued DESC
    pub fn load_task_spec(&self, id: &TranscriptionId) -> Result<Option<TaskSpec>, StoreError>; // options_json round-trip
    pub fn get_segments(&self, id: &TranscriptionId) -> Result<Vec<Segment>, StoreError>;
}
impl TaskStore for SqliteStore { /* ... */ }
pub struct TaskRow { pub id: TranscriptionId, pub status: Status, pub source: Option<Source>,
    pub file: Option<PathBuf>, pub url: Option<String>, pub progress: f64,
    pub error_message: Option<String>, pub time_queued: String, pub time_started: Option<String>,
    pub time_ended: Option<String>, pub name: Option<String>, pub notes: Option<String> }
```

**Schema v1 (`schema.rs`, executed when `PRAGMA user_version` = 0, then set to 1)** — Buzz-superset (`buzz/schema.sql` verbatim columns) + humm's two additions (ARCH §5):
```sql
CREATE TABLE IF NOT EXISTS transcription (
    id TEXT PRIMARY KEY, error_message TEXT, export_formats TEXT, file TEXT,
    output_folder TEXT, progress DOUBLE PRECISION DEFAULT 0.0, language TEXT,
    model_type TEXT, source TEXT, status TEXT, task TEXT, time_ended TIMESTAMP,
    time_queued TIMESTAMP NOT NULL, time_started TIMESTAMP, url TEXT,
    whisper_model_size TEXT, hugging_face_model_id TEXT,
    word_level_timings BOOLEAN DEFAULT FALSE, extract_speech BOOLEAN DEFAULT FALSE,
    name TEXT, notes TEXT,
    options_json TEXT,                        -- humm: full TaskSpec JSON (fixes restart-loses-options)
    kind TEXT NOT NULL DEFAULT 'file'         -- humm: 'file' | 'live' (Q-10; live arrives M5)
);
CREATE TABLE IF NOT EXISTS transcription_segment (
    id INTEGER PRIMARY KEY, end_time INT DEFAULT 0, start_time INT DEFAULT 0,
    text TEXT NOT NULL, translation TEXT DEFAULT '',
    transcription_id TEXT,
    FOREIGN KEY (transcription_id) REFERENCES transcription(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_transcription_id ON transcription_segment(transcription_id);
CREATE TABLE IF NOT EXISTS humm_meta (key TEXT PRIMARY KEY, value TEXT);
```

**Column-fill contract for `create_task`** (Buzz `transcription_dao.py:18-105` semantics):
`status`='queued', `time_queued`=now (naive-local ISO, Constraint 12), `task`=`options.task.as_str()`, `language`=code or NULL, `model_type`= `"Hugging Face"` if `spec.options.model.hf_repo.is_some()` else `"Whisper.cpp"` (Buzz `ModelType.value` strings; humm's single local backend maps to Whisper.cpp — same mapping M1 chose for `{{ model_type }}`), `whisper_model_size`=`size.as_str()`, `hugging_face_model_id`=repo or NULL, `export_formats`=formats joined `", "` in BTreeSet order, `file`=path (NULL for Url), `url`=url string (NULL for Path), `output_folder`=directory or NULL, `word_level_timings`/`extract_speech` 0/1, `source`=`source.as_str()`, `name`/`notes` NULL, `options_json`=`serde_json::to_string(spec)`, `kind`='file'.

**Transition contract** (each writes exactly Buzz's columns, `transcription_dao.py:172-251`): started → status+time_started; progress → status='in_progress'+progress; completed → status+time_ended, then insert all segments (start/end/text, translation ''); failed → status+time_ended+error_message; canceled → status+time_ended. Segments only on completion (cancel/fail leave none — Buzz parity).

**Steps:**

- [ ] **Step 1:** Workspace/dep plumbing (member, `humm-core` centralized workspace dep — verify all crates still build).
- [ ] **Step 2 (TDD):** tests (all `open_in_memory` or tempdir):
  - create → row present with every column per the fill contract (assert each, incl. `options_json` parses back to the input `TaskSpec` and wire strings `"queued"`, `"Whisper.cpp"`, `"txt, srt"`).
  - Url-input task: `file` NULL, `url` set; `update_file` then sets `file`.
  - Full lifecycle queued→started→progress(0.5)→completed(2 segments): statuses/timestamps/segments correct; `get_segments` returns them in insertion order.
  - failed sets error_message; canceled sets time_ended, no segments.
  - `cancel_stale_tasks`: seed queued + in_progress + completed rows → flips exactly the first two, returns 2, completed untouched (Buzz `helpers.py:79-89`).
  - `load_task_spec` round-trips; row with `options_json` NULL → `Ok(None)`.
  - `open` twice on same path → idempotent (migration guard); WAL mode actually on (`PRAGMA journal_mode` query).
- [ ] **Step 3:** Implement; gates.
- [ ] **Step 4: Commit** `M2: humm-store crate — schema v1, repository, startup cancel-flip (F-029, F-026)`.

---

### Task 4: One-time Buzz.sqlite import (Q-6)

**Files:**
- Create: `crates/humm-store/tests/buzz_import.rs`, `crates/humm-store/tests/fixtures/buzz_schema.sql` (copied VERBATIM from `/Users/jawad/RustroverProjects/buzz/buzz/schema.sql` — cite the copy source in a header comment)
- Modify: `crates/humm-store/src/store.rs`

**Buzz sources to read first:** `buzz/schema.sql`, `buzz/db/helpers.py` (what a real Buzz DB contains), `docs/OPEN_QUESTIONS.md` Q-6 (approved: history yes, settings no). ADR-004 validation clause.

**Interfaces:**
```rust
impl SqliteStore {
    /// One-time import of a Buzz database. No-op (skipped=true) if already done.
    /// Failures are the caller's to log — non-fatal by contract (ARCH §5).
    pub fn import_buzz_db(&self, buzz_db: &Path) -> Result<BuzzImportOutcome, StoreError>;
    pub fn buzz_import_done(&self) -> Result<bool, StoreError>;
}
pub struct BuzzImportOutcome { pub skipped: bool, pub tasks: usize, pub segments: usize }
```

**Behavior contract:**
- Guard: `humm_meta['buzz_import_done']` present → `skipped: true`, no reads of the Buzz file.
- Import: `ATTACH DATABASE ?1 AS buzz` with a read-only URI (`file:<path>?mode=ro&immutable=1`); `INSERT INTO transcription (<the 21 Buzz columns>) SELECT <same> FROM buzz.transcription WHERE id NOT IN (SELECT id FROM transcription)`; segments likewise (all columns except autoincrement `id`), restricted to imported transcription ids; `DETACH`. Imported rows keep Buzz status values verbatim (a Buzz `in_progress` row imports as-is; the caller's subsequent `cancel_stale_tasks` flips it — document this ordering in the doc comment). `options_json` stays NULL (imported rows restart from column data, like Buzz did), `kind` defaults `'file'`.
- On success write `humm_meta['buzz_import_done']` = now (ISO). A failed import writes no marker (retry next launch).

**Steps:**

- [ ] **Step 1 (TDD):** integration test builds a fixture Buzz DB by executing `fixtures/buzz_schema.sql` with rusqlite, inserting: one completed row with 3 segments (incl. translation text), one failed row (error_message set), one in_progress row, one URL row (`file` NULL/`url` set, source `url_import`), one row with NULL language. Assertions: counts in outcome; columns survive verbatim (spot-check each fixture row incl. segment times/translation); second `import_buzz_db` call → `skipped: true`, counts unchanged; import into a store that already has a row with the same id → not duplicated; corrupt path → `Err`, marker absent, retry after fixing works.
- [ ] **Step 2:** Implement; gates.
- [ ] **Step 3: Commit** `M2: one-time Buzz.sqlite import (F-029, Q-6)`.

---

### Task 5: Task queue + event bus in core (F-024, F-026 cancel, F-025 fractions)

**Files:**
- Create: `crates/humm-core/src/queue.rs`
- Modify: `crates/humm-core/src/lib.rs`, `crates/humm-core/src/boundary.rs` (the `MediaFetcher` trait + `FetchProgress` are DEFINED here, in this task — Task 6 only implements them), `crates/humm-core/src/error.rs` (`MediaError::FetchFailed { detail: String }`), `crates/humm-core/src/pipeline.rs` (tests only — cancel-interleaving ticket), `crates/humm-core/Cargo.toml` (dep: flume), `Cargo.toml` (workspace dep: flume)

**Buzz sources to read first:** `buzz/file_transcriber_queue_worker.py` (FIFO, one-at-a-time, canceled-set semantics, status assignment on error containing "canceled", cleanup), `buzz/widgets/main_window.py:257-266` (cancel writes canceled to DB immediately), `buzz/transcriber/file_transcriber.py:38-73` (URL download happens at task run, inside the job). Design: ARCHITECTURE §3 (concurrency table + event bus + cancellation rules).

**Interfaces:**
```rust
// queue.rs
pub enum TaskEventKind { Queued, Started, DownloadProgress(f32), Progress(f32),
    SegmentAdded(Segment), Completed { written: Vec<PathBuf> },
    Failed { message: String }, Canceled }
pub struct TaskEvent { pub task_id: TranscriptionId, pub kind: TaskEventKind }

/// Creates one engine per model spec; the runner caches the engine while
/// consecutive tasks share an identical ModelSpec (one CLI run = one model).
pub trait EngineFactory: Send {
    fn create(&mut self, model: &ModelSpec) -> Result<Box<dyn TranscriptionEngine>, CoreError>;
}

// boundary.rs (defined HERE; humm-media implements in Task 6)
pub struct FetchProgress { pub downloaded: u64, pub total: Option<u64> }
pub trait MediaFetcher: Send + Sync {
    /// Downloads the best audio of `url` into `dest_dir`, returning the
    /// downloaded file's path. Emits progress; kills the subprocess on cancel.
    fn fetch(&self, url: &url::Url, dest_dir: &Path,
             progress: &mut dyn FnMut(FetchProgress), cancel: &CancelToken)
             -> Result<PathBuf, MediaError>;
}

pub struct RunnerDeps {
    pub store: std::sync::Arc<dyn TaskStore>,
    pub decoder: std::sync::Arc<dyn AudioDecoder>,
    pub prober: std::sync::Arc<dyn MediaProber>,
    pub fetcher: Option<std::sync::Arc<dyn MediaFetcher>>, // None => URL tasks fail actionably (Task 6 trait)
    pub engines: Box<dyn EngineFactory>,
    pub tuning: EngineTuning,
    pub media_dir: PathBuf,          // URL downloads land in media_dir/<task-id>/
}
pub fn spawn_task_runner(deps: RunnerDeps) -> QueueHandle;
pub struct QueueHandle { /* command sender + subscriber registry + JoinHandle */ }
impl QueueHandle {
    pub fn subscribe(&self) -> flume::Receiver<TaskEvent>;
    pub fn enqueue(&self, id: TranscriptionId, spec: TaskSpec, source: Source);
    pub fn cancel(&self, id: TranscriptionId);
    pub fn cancel_all(&self);
    pub fn finish(self) -> Result<(), CoreError>;  // no more enqueues; drain queue; join thread
}
```

**Behavior contract (the runner thread):**
- FIFO, strictly one task at a time (GPU contention is the feature — ARCH §3).
- On `enqueue`: runner (sole status writer) calls `store.create_task` then emits `Queued`. Store errors at any point fail the task (`mark_failed` best-effort, `Failed` event), never panic the runner.
- Per task: fresh `CancelToken`; canceled-before-start (via `cancel`/`cancel_all` while pending) → `mark_canceled` + `Canceled` event without starting (Buzz canceled-set semantics, `queue_worker.py:171-178`).
- Run: `mark_started`+`Started`; if `Input::Url`: no fetcher → fail with "yt-dlp missing — install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP"; else `fetch` into `media_dir/<id>/` emitting `DownloadProgress(fraction)` when total known (Buzz emits only then — `file_transcriber.py:172-174`), then `store.update_file(local)`. Then `run_file_job` with a sink that forwards `SegmentAdded`/`Progress` events and writes `update_progress` throttled to whole-percent changes (deviation from Buzz's every-signal write — invisible, cheaper; note in doc comment).
- Terminal: **status written to store BEFORE the terminal event is emitted** (ARCH §3 "terminal status written before ack") — completed (with segments) / failed / canceled. An engine/pipeline `CoreError::Canceled` (or `EngineError::Canceled`/`MediaError::Canceled` inside it) maps to Canceled, not Failed (Buzz's string-sniffing `queue_worker.py:388-394` becomes typed).
- `cancel(id)` for the *current* task trips its `CancelToken` (whisper abort callback + ffmpeg/yt-dlp kill paths already honor it). `cancel_all` = cancel current + all pending. `finish` processes the remaining queue then exits (CLI `quit_on_complete` parity); events to dropped receivers are discarded silently.

**Steps:**

- [ ] **Step 1 (TDD):** unit tests with in-module fakes (fake `TaskStore` recording calls with an event log + `Mutex<Vec<...>>`, fake engine blocking on a channel until released, fake fetcher, fake decoder/prober reused from pipeline-test patterns):
  - two tasks enqueued → run in FIFO order, one at a time (fake engine asserts no overlap), statuses queued→started→completed each, events in contract order, segments passed to `mark_completed`.
  - failed task (engine error) → `mark_failed` with the message, queue continues to next task.
  - cancel pending: enqueue A (blocking) + B; cancel B; release A → B gets `mark_canceled`+`Canceled`, never `Started`.
  - cancel current: A blocking in engine; `cancel(A)`; fake engine returns `EngineError::Canceled` when its token trips → `mark_canceled` called BEFORE the `Canceled` event is observable (fake store timestamps its calls; the test subscriber records receive order).
  - store-write ordering: fake store that records sequence — assert `mark_completed` precedes `Completed` event delivery.
  - Url task with `fetcher: None` → Failed with the actionable message; with fake fetcher → `DownloadProgress` events + `update_file` called with the fetched path before decode.
  - progress throttling: fake engine emits fractions 0.111→0.112→0.113→0.25 → `update_progress` called for 11% once and 25% once (2 calls).
  - `finish` drains: enqueue 2, immediately `finish()` → both complete.
- [ ] **Step 2 (M1 ticket — mid-export cancel interleaving, `pipeline.rs` tests):** a `SegmentSink` observer whose `on_progress(1.0)` trips the shared `CancelToken` (cancel lands between transcribe and the export loop) with 2 formats requested → `run_file_job` returns `CoreError::Canceled` and writes ZERO export files. Second test: a fake engine that trips cancel *after* `run_file_job` has written the first format — not reachable from inside the current loop, so instead assert the documented invariant directly: token canceled before the loop → zero files (already covered by M1's pre-cancel test — extend it to assert with formats requested, not empty set). Cite the ticket in the test doc comment.
- [ ] **Step 3:** Implement; gates.
- [ ] **Step 4: Commit** `M2: task queue + event bus, cancel semantics, cancel-interleaving tests (F-024, F-025, F-026)`.

---

### Task 6: yt-dlp MediaFetcher adapter (F-022 URL half, ADR-016)

**Files:**
- Create: `crates/humm-media/src/fetch.rs`
- Modify: `crates/humm-media/src/lib.rs`, `crates/humm-media/src/capabilities.rs` (yt-dlp probe), `crates/humm-media/tests/contract.rs`
- Note: the `MediaFetcher` trait, `FetchProgress`, and `MediaError::FetchFailed` were defined in core by Task 5 — this task implements the trait in humm-media only.

**Buzz sources to read first:** `buzz/transcriber/file_transcriber.py:75-153` (`_download_from_url`: bestaudio/best, cookiefile env, title→filename sanitization, progress hook = downloaded/total bytes), `docs/design/DECISIONS.md` ADR-016 (subprocess, `--print` machine-readable, graceful absence).

**Interfaces:**
```rust
// humm-media fetch.rs
pub struct YtDlpFetcher { /* binary path (HUMM_YTDLP override else "yt-dlp"), cookie_file: Option<PathBuf> */ }
impl YtDlpFetcher { pub fn new(cookie_file: Option<PathBuf>) -> Self; }  // env read in constructor, M1 adapter pattern
impl MediaFetcher for YtDlpFetcher { /* ... */ }
// capabilities.rs: MediaCapabilities gains `pub yt_dlp: Option<String>` (version), probed via `yt-dlp --version`
```

**Subprocess contract (machine-readable only — Standing Policy):**
```
yt-dlp <url> -f bestaudio/best --no-playlist -o <dest_dir>/%(title)s.%(ext)s \
  --newline --progress \
  --progress-template "download:humm-dl:%(progress.downloaded_bytes)s:%(progress.total_bytes)s:%(progress.total_bytes_estimate)s" \
  --print after_move:filepath --no-simulate [--cookies <cookie_file>]
```
- Parse stdout line-by-line: lines starting `humm-dl:` → progress (fields may be `NA`/non-numeric → `None`; `total` = total_bytes else estimate else None); the final path = last non-progress stdout line. Missing final path or nonzero exit → `FetchFailed` with trailing stderr (capped ~2 KB) in `detail`.
- `--no-playlist` is a deliberate deviation: Buzz's fixed `outtmpl` makes playlists overwrite themselves (accidental behavior, not a feature); document in the adapter doc comment + ADR-016 note (same commit, Constraint 13).
- Filename comes from yt-dlp's own `%(title)s` sanitization (Buzz hand-replaces `/\:*?"<>|` — same outcome class; document).
- Cancel: poll `cancel` while reading; on trip, `kill()` the child, reap, return `MediaError::Canceled`.
- Binary missing (spawn `NotFound`) → `MediaError::BinaryMissing { binary: "yt-dlp", hint: "install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP" }`.

**Steps:**

- [ ] **Step 1 (TDD, non-ignored):** progress-template line parser is a pure fn — tests: valid line, `NA` totals, estimate fallback, garbage line ignored. Absence: `YtDlpFetcher` with `HUMM_YTDLP` pointing at a nonexistent path (constructor takes the seam per M1 pattern) → `BinaryMissing` with the hint.
- [ ] **Step 2 (contract tests, `#[ignore]`, need real yt-dlp):** a ~50-line std `TcpListener` HTTP file server in the test module serves `HUMM_BUZZ_TESTDATA/whisper-french.mp3`; `fetch("http://127.0.0.1:<port>/whisper-french.mp3", tmp)` via yt-dlp's generic extractor → file exists in dest_dir, ≥1 progress callback fired, returned path is the stdout-printed one. Cancel test: trip the token mid-download (large file: serve `audio-long.mp3` with throttled writes) → `Canceled` within ~2 s, child reaped. Capability probe test: `probe_binaries().yt_dlp` is `Some` when the binary exists.
- [ ] **Step 3:** Implement; gates; run the ignored tests (`cargo test -p humm-media -- --ignored`, network-free — the server is local; yt-dlp must be installed, note if absent).
- [ ] **Step 4: Commit** `M2: yt-dlp MediaFetcher adapter + contract tests (F-022, ADR-016)`.

---

### Task 7: Downloader HTTP seam + CoreML 416 recovery + pinning tests (M1 tickets, F-043)

**Files:**
- Modify: `crates/humm-core/src/models/registry.rs` (`*_url` fns gain `_with_base` variants), `crates/humm-core/src/models/download.rs`
- Create: test HTTP server helper `#[cfg(test)]` in `crates/humm-core/src/models/test_server.rs` (declared `#[cfg(test)] mod test_server;` in `models/mod.rs`)

**Context (M1 review tickets — the acceptance criteria are theirs):** memory `humm-m2-tickets`: (a) injectable HTTP seam, then the test pinning "ggml cached + CoreML Missing + wanted → encoder fetch attempted" (final reviewer's explicit acceptance criterion); (b) CoreML zip path routed through the same `recover_from_range_not_satisfiable` logic as the ggml path; (c) placement failure after zip rename → whole-zip redownload next run.

**Interfaces:**
```rust
// registry.rs — existing fns keep their signatures (delegating with the default base)
pub const HF_BASE: &str = "https://huggingface.co";
pub fn ggml_url_with_base(base: &str, size: ModelSize) -> Result<String, ModelError>;
pub fn coreml_zip_url_with_base(base: &str, size: ModelSize) -> Result<String, ModelError>;
pub fn vad_model_url_with_base(base: &str) -> String;
// download.rs
impl ModelDownloader {
    pub fn with_hf_base(root: impl Into<PathBuf>, hf_base: impl Into<String>) -> Self; // tests; new() = HF_BASE
}
```
Every URL the downloader builds (ggml, CoreML zip, VAD, SHA-digest lookups if any) must flow through the stored base — grep for `huggingface.co` literals afterward; none may remain outside `registry.rs` defaults.

**Test server requirements (`test_server.rs`):** single-threaded std `TcpListener` accept loop on 127.0.0.1:0; per-path canned responses configured by the test (body bytes, forced status); supports GET with `Range: bytes=N-` → 206 + `Content-Range` (or forced 416); records every request (method, path, range) into an `Arc<Mutex<Vec<RequestRecord>>>` the test asserts on; shuts down when dropped.

**Steps:**

- [ ] **Step 1:** Thread the seam (mechanical refactor; existing tests stay green).
- [ ] **Step 2 (TDD, all non-network via the local server):**
  - **Pinning test (ticket a, exact acceptance):** pre-place a valid ggml + sentinel in a temp root; CoreML state `Missing`; `resolve_model(size, coreml_encoder=true, ...)` against the local server → server's request log contains a GET for `ggml-<size>-encoder.mlmodelc.zip`.
  - **CoreML 416 recovery (ticket b):** pre-place a COMPLETE encoder-zip `.partial` (a valid zip the server also has); server answers the ranged GET with 416 → `ensure_coreml_encoder` verifies/promotes (or restarts once) and the encoder directory ends `Complete`; a second 416 after restart → error, no infinite loop. Route the zip path through `recover_from_range_not_satisfiable` (refactor as needed).
  - **Placement-failure redownload (ticket c):** first run: server serves a CORRUPT zip (extraction fails) → error propagates, no `Complete` state; second run against a good zip → a fresh GET of the zip occurs (request log) and extraction completes. This pins "a failed placement doesn't wedge the cache".
  - **Happy-path seam test:** full `resolve_model` (ggml + CoreML zip both from local server, real SHA flow if M1 verifies digests — mirror whatever digest mechanism M1 implemented, serving the digest the same way) → `ResolvedModel` complete; proves future upgrades testable offline.
- [ ] **Step 3:** gates; also run the existing network contract test once (`cargo test -p humm-core --test model_download_contract -- --ignored`) to prove no regression against real huggingface.co.
- [ ] **Step 4: Commit** `M2: downloader HTTP seam, CoreML 416 recovery + pinning tests (F-043)`.

---

### Task 8: Custom models — ggml URL + HF Whisper-family repo id (F-042 full, Q-2)

**Files:**
- Modify: `crates/humm-core/src/models/registry.rs`, `crates/humm-core/src/models/download.rs`, `docs/design/DECISIONS.md` (ADR-008: record the concrete custom-model mechanics), `docs/analysis/PARITY_MATRIX.md` only if a factual note is wrong (status updates stay in Task 13)

**Buzz sources to read first:** `buzz/model_loader.py:128-155` (custom/lumii sizes), `:495-527` (custom file path `ggml-model-whisper-custom.bin`, repos), `:656-738` (`custom_model_url` download path — note: no SHA for custom), `buzz/widgets/preferences_dialog/models_preferences_widget.py` (where the custom URL comes from in Buzz — a setting, not per-task). `docs/OPEN_QUESTIONS.md` Q-2 (approved: Whisper-family via ggml equivalents; MMS out of scope).

**Interfaces:**
```rust
// registry.rs
pub fn custom_model_file_name() -> &'static str;              // "ggml-custom.bin" (ggml_file_name(Custom) already yields this)
pub fn hf_repo_model_file_name(repo: &str) -> String;         // "ggml-hf-<repo with '/'->'--'>.bin"
// download.rs
impl ModelDownloader {
    /// `-s custom`: downloads the user-configured ggml URL to the cache.
    /// No SHA256 (no upstream digest exists — Buzz doesn't verify custom either); sentinel still written.
    pub fn ensure_custom_model(&self, url: &str, progress: &mut dyn FnMut(DownloadProgress),
                               cancel: &CancelToken) -> Result<PathBuf, ModelError>;
    /// `-m huggingface --hfid <repo>`: resolves a Whisper-family HF repo to a local ggml (Q-2).
    pub fn resolve_hf_repo(&self, repo: &str, coreml_encoder: bool,
                           progress: &mut dyn FnMut(DownloadProgress), cancel: &CancelToken)
                           -> Result<ResolvedModel, ModelError>;
}
```

**`resolve_hf_repo` behavior contract (the Q-2 "fetch ggml equivalents" mechanics — record in ADR-008):**
1. `openai/whisper-<name>` where `<name>` maps to a known `ModelSize` (`tiny`, `tiny.en`, … `large-v3-turbo`; `large` → `Large`) → delegate to `resolve_model(size, coreml_encoder, ...)` — the canonical ggml equivalent of the canonical Whisper repos.
2. Otherwise GET `{hf_base}/api/models/{repo}` (HF Hub API, JSON `siblings[].rfilename`): collect files matching `ggml-*.bin` or `*.bin`; exactly one candidate → download `{hf_base}/{repo}/resolve/main/{file}` to `hf_repo_model_file_name(repo)` (no SHA — repo digests are not exposed per-file in that API; sentinel written; no CoreML — third-party repos don't publish encoder zips). Multiple candidates → `ModelError::Unsupported` listing them and telling the user to pick one via the custom-URL mechanism; zero → `Unsupported` explaining only ggml-format Whisper repos are supported and pointing at whisper.cpp's conversion docs + the custom-URL path. API failure → `ModelError::Http`.
3. MMS / non-Whisper repos naturally fall into the "zero candidates" arm — that IS the approved Q-2 scope cut; the error message must say so plainly.

**Steps:**

- [ ] **Step 1 (TDD, local server via Task 7's seam):**
  - `ensure_custom_model` downloads + resumes (interrupt once) + sentinel; second call = fast path, zero requests.
  - `resolve_hf_repo("openai/whisper-tiny", ...)` → request log shows `ggml-tiny.bin` fetched from the whisper.cpp repo path (the mapping worked); returned path = the standard tiny cache path.
  - `resolve_hf_repo` on a fake repo whose api/models JSON lists exactly `["ggml-model.bin", "README.md"]` → downloads it to `ggml-hf-<repo>--....bin`; JSON with two `.bin` files → `Unsupported` naming both; JSON with none → `Unsupported` mentioning MMS/conversion; HTTP 404 on the API → `Http`.
  - registry name fns: `hf_repo_model_file_name("acme/whisper-x")` == `"ggml-hf-acme--whisper-x.bin"`; `ggml_file_name(ModelSize::Custom)` == `"ggml-custom.bin"`.
- [ ] **Step 2:** Implement; update ADR-008 with the mechanics above (same commit); gates.
- [ ] **Step 3: Commit** `M2: custom models — ggml URL + HF repo resolution (F-042, F-043, Q-2/ADR-008)`.

---

### Task 9: Update-check logic (F-006, ADR-015)

**Files:**
- Create: `crates/humm-core/src/update.rs`
- Modify: `crates/humm-core/src/lib.rs`, `docs/design/DECISIONS.md` (ADR-015: endpoint note)

**Buzz sources to read first:** `buzz/update_checker.py` (cadence gate, ISO last-check, numeric dotted version compare with zero-padding, `version_info.json` shape: `version`/`release_notes`/`download_urls{macos_arm,macos_x86,windows_x64}`). ADR-015 (notify-only, `HUMM_DISABLE_UPDATE_CHECK`, same ≤7-day cadence).

**Interfaces:**
```rust
pub const CHECK_INTERVAL_DAYS: i64 = 7;
/// humm's own endpoint (Q-8: Buzz's endpoints are not reused). The repo is Gitea;
/// Gitea serves /releases/latest/download/<asset> like GitHub. No release exists yet —
/// note that in ADR-015 (fetches 404 harmlessly until first release).
pub const VERSION_INFO_URL: &str = "https://repo.digikal.com/jawad/humm/releases/latest/download/version_info.json";
pub struct UpdateInfo { pub version: String, pub release_notes: String, pub download_urls: Vec<String> }
pub fn is_newer_version(current: &str, remote: &str) -> bool;   // numeric dotted, zero-pad shorter; any non-numeric part => false (update_checker.py:147-163)
pub fn should_check(last_check_iso: Option<&str>, now: chrono::NaiveDateTime, disabled: bool) -> bool;
    // disabled => false; unparseable/absent last_check => true; else days-since >= 7 (update_checker.py:42-67;
    // Buzz's Windows/macOS platform gate is not ported — humm targets macOS; note in doc comment)
pub fn parse_version_info(json: &str, current_version: &str, arch_key: &str) -> Result<Option<UpdateInfo>, UpdateError>;
    // None when remote is not newer; arch_key picks download_urls["macos_arm"|"macos_x86"] (list-or-scalar tolerated)
pub fn check_for_update(url: &str, current_version: &str) -> Result<Option<UpdateInfo>, UpdateError>; // thin blocking reqwest fetch + parse; callers persist last_check/available_version via Settings (Task 2)
pub enum UpdateError { Http { detail: String }, Parse { detail: String } }  // thiserror
```

**Steps:**

- [ ] **Step 1 (TDD):** port `update_checker.py`'s compare semantics as table tests: `("1.4.5","1.4.6")→true`, `("1.4.5","1.4.5")→false`, `("1.4.5","1.5")→true` (pad), `("1.10.0","1.9.9")→false`, `("1.4.5","2.0.0-beta")→false` (non-numeric). `should_check`: disabled→false; None→true; 6 days→false; 8 days→true; garbage ISO→true. `parse_version_info`: full JSON fixture (newer) → Some with the right platform urls (scalar and list forms); equal version → None; missing keys → defaults ("" / empty list) not errors (Buzz `.get` semantics); invalid JSON → Parse err.
- [ ] **Step 2:** Implement (no test hits the network; `check_for_update` itself is a 10-line composition left uncovered or covered via the Task 7 test server). ADR-015 endpoint note. Gates.
- [ ] **Step 3: Commit** `M2: update-check logic (F-006, ADR-015)`.

---

### Task 10: CLI grammar completion (F-002)

**Files:**
- Modify: `crates/humm-cli/src/args.rs`, `crates/humm-cli/src/run.rs` (validation only — wiring is Task 11), `crates/humm-cli/tests/cli.rs`

**Buzz sources to read first:** `buzz/cli.py` (WHOLE file — option list `:52-138`, `_resolve_model` `:141-159`, hfid rule `:146-147`, token rule `:231-235`, "No input files" `:212-213`, "Invalid language option" `:226-227`), `docs/analysis/EXTERNAL_SURFACE.md` §2/§8, ADR-007 (deviations incl. `--hide-gui` no-op + real exit codes), ADR-008 (backend mapping), UX_SPEC §5 (`--quiet`, `--json`).

**Interfaces:**
```rust
// args.rs additions to AddArgs (spellings Buzz-verbatim):
#[arg(short = 'm', long = "model-type", default_value = "whisper")] pub model_type: ModelTypeArg,
#[arg(long = "hfid", value_name = "id")] pub hfid: Option<String>,
#[arg(short = 'e', long = "extract-speech")] pub extract_speech: bool,
#[arg(long = "openai-token", value_name = "token")] pub openai_token: Option<String>,
#[arg(long = "hide-gui")] pub hide_gui: bool,
#[arg(long = "json")] pub json: bool,     // ADR-007 addition
#[arg(long = "quiet")] pub quiet: bool,   // UX_SPEC §5 addition
pub files: Vec<String>,                   // was Vec<PathBuf>; positionals are paths OR URLs (Input::parse in Task 11)

pub enum ModelTypeArg { Whisper, WhisperCpp, HuggingFace, FasterWhisper, OpenAiApi }
// clap values exactly: "whisper", "whispercpp", "huggingface", "fasterwhisper", "openaiapi" (cli.py:31-36)
```

**Validation contract (in `run.rs`, before any model/store work; all exit 1 via the existing `fail` path):**
- zero positionals → `No input files` (exists).
- `-m huggingface` without `--hfid` → `--hfid is required when --model-type is huggingface` (cli.py:147 verbatim).
- `-m openaiapi` → `the OpenAI API backend is not yet available in humm — it arrives with cloud support; use a local model type meanwhile` (honest not-yet gap; F-002 matrix note in Task 13).
- `-e/--extract-speech` set → `speech extraction is not yet available in humm — it arrives in a later release` (F-027 is M8).
- `--hide-gui` → accepted, stderr note `note: --hide-gui has no effect; humm is headless (ADR-007)`, continue.
- `-m whisper|whispercpp|fasterwhisper` → all run the local whisper-rs backend (ADR-008 consolidation); `--hfid` with a non-huggingface type is ignored exactly as Buzz ignores it.
- `--openai-token` without `-m openaiapi`: accepted-and-unused (Buzz behavior).

**Steps:**

- [ ] **Step 1 (TDD, cli-int via assert_cmd — no model/network needed):** grammar matrix tests: each validation case above (exit code 1 + exact message on stderr); `--help` lists every flag incl. short forms `-m`/`-e` (snapshot-lite: assert substrings); invalid `-m bogus` → clap error, exit 1 (existing `exit_code_for_parse_error`); `--json`/`--quiet`/`--hide-gui` parse. Unit tests for any pure validation helpers.
- [ ] **Step 2:** Implement; gates.
- [ ] **Step 3: Commit** `M2: full add grammar — model-type/hfid/extract-speech/openai-token/hide-gui/json/quiet (F-002)`.

---

### Task 11: CLI wiring — store + queue + events, URLs, Ctrl-C, `--json`, progress+ETA (F-002, F-024, F-025, F-026, F-029)

**Files:**
- Modify: `crates/humm-cli/src/run.rs` (substantial rework), `crates/humm-cli/src/main.rs` (if needed), `crates/humm-cli/Cargo.toml` (deps: humm-store, ctrlc, serde_json, uuid via workspace), `crates/humm-cli/tests/cli.rs`
- Create: `docs/HUMM_SURFACE.md` (humm's observable contract — the `EXTERNAL_SURFACE.md` successor UX_SPEC §5 calls for: full CLI grammar, `--json` event schema, exit codes, env vars, file locations incl. DB/media/settings paths)

**Buzz sources to read first:** `buzz/cli.py` (model resolution ORDER: model resolved/downloaded synchronously BEFORE tasks queue — `:219-221`), `buzz/widgets/transcription_tasks_table_widget.py:67-107` (the ETA formula: `elapsed > 2s && 0<p<1` → `remaining = (1-p)*elapsed/p`, rendered `NNm NNs`/`NNs`), `buzz/widgets/main_window.py:257-266` (cancel → DB canceled immediately). ADR-007, UX_SPEC §4/§5.

**Run flow (replaces the M1 per-file loop; parity order restored — M1's "skip model download when every input is missing" fast path is removed because failed tasks now get persisted history rows like Buzz):**
1. Validation (Task 10). Parse positionals via `Input::parse`.
2. Load `Settings` (path from `paths::settings_path()`; corrupt → exit 1 actionable).
3. Open `SqliteStore` at `paths::db_path()`. First-run Buzz import: if `!buzz_import_done()` and `paths::buzz_sqlite_path()` is Some → `import_buzz_db` (log outcome; failure = `log::warn`, never fatal — Q-6). Then `cancel_stale_tasks()` (log flipped count) — order matters: imported stale rows get flipped too (Task 4 contract).
4. Resolve model synchronously with download progress (human line or `--json` `model_download_progress` events): by model type — sizes via `resolve_model`; `-s custom` → `settings.models.custom_model_url` else exit 1 `no custom model URL configured — set models.custom_model_url in <settings_path>`; `-m huggingface` → `resolve_hf_repo(hfid, ...)`. CoreML-degraded note preserved. Failures exit 1 **without** the `--help` hint (see messaging rule below).
5. Ensure VAD (existing logic).
6. Build one `TaskSpec` per input (shared options/output; `hf_repo` set when applicable). Spawn runner: `RunnerDeps { store (Arc<SqliteStore>), FfmpegDecoder, FfprobeProber, fetcher: YtDlpFetcher::new(cookie_file from HUMM_DOWNLOAD_COOKIEFILE env else settings.downloads.cookie_file) when capabilities report the binary else None, engines: CliEngineFactory (loads WhisperEngine from the resolved path; caches — the runner already reuses per equal ModelSpec), tuning, media_dir: paths::media_dir() }`.
7. Install Ctrl-C handler (`ctrlc`): first signal → `cancel_all()` + stderr `canceling… (press Ctrl-C again to exit immediately)`; second → `std::process::exit(130)`.
8. Subscribe; enqueue all (fresh `TranscriptionId` each); render events until `finish()` returns:
   - human mode: per-task progress line `<input>  NN% — ETA: NNs` (ETA via the ported pure fn `format_eta(elapsed_secs: f64, fraction: f32) -> Option<String>`, Buzz formula + rendering); URL download phase renders `downloading <url>  NN%`; completed → written paths to **stdout** one per line (existing contract); failed → `error: <input>: <cause>` on stderr. `--quiet`: no progress lines (stdout paths + stderr errors remain).
   - `--json` mode: one serde_json object per line on **stdout**, nothing else on stdout (schema below; document in `docs/HUMM_SURFACE.md`).
9. Exit code: any canceled → **130**; else any failed → **2**; else **0**. Record the 130 addition in ADR-007 (same commit, Constraint 13).

**`--json` event schema (stable from this milestone; field names exact):**
```json
{"event":"model_download_progress","file":"ggml-small.bin","downloaded":1024,"total":487000000}
{"event":"queued","task_id":"<uuid>","input":"/path/or/url"}
{"event":"started","task_id":"<uuid>"}
{"event":"download_progress","task_id":"<uuid>","fraction":0.42}
{"event":"progress","task_id":"<uuid>","fraction":0.42}
{"event":"segment","task_id":"<uuid>","start_ms":40,"end_ms":299,"text":"..."}
{"event":"completed","task_id":"<uuid>","written":["/path.srt"]}
{"event":"failed","task_id":"<uuid>","error":"..."}
{"event":"canceled","task_id":"<uuid>"}
```

**Messaging rule (M1 ticket):** the `Run \`humm add --help\` for usage.` hint appears ONLY on usage/validation errors (Task 10's cases + zero inputs + bad language). Runtime/config failures (model download, settings corrupt, engine load) print the actionable cause alone. Refactor `fail` into `usage_fail` (hint) and `run_fail` (no hint).

**Steps:**

- [ ] **Step 1 (TDD, pure units):** `format_eta`: `(1.0s, 0.5) → None` (≤2 s), `(10.0, 0.5) → Some("10s")`, `(300.0, 0.2) → Some("20m 0s")`, `(10.0, 0.0)`/`(10.0, 1.0)` → None. JSON event serialization: each `TaskEventKind` maps to the exact schema above (assert full JSON strings).
- [ ] **Step 2 (TDD, cli-int with the stub model — Constraint 4 makes this network-free):** seed a temp `HUMM_MODEL_ROOT` with the Buzz stub `ggml-tiny.bin` + its `.humm_complete` sentinel + a `.humm_no_coreml`-marked encoder state (fast path, no network); temp `HUMM_DATA_DIR`/`HUMM_CONFIG_DIR`:
  - `humm add <testdata wav> --txt -s tiny` → exit 0; DB at the temp data dir contains one `completed` row with `options_json` round-tripping to the spec, `export_formats` `"txt"`; a segments query returns 0 rows (stub transcribes to nothing) — asserted via rusqlite in the test.
  - `humm add missing.mp3 good.wav --txt -s tiny` → exit 2; TWO rows: one `failed` (error_message mentions the path), one `completed` (parity: failures are history now — Buzz creates rows for every input).
  - `--json` run → every stdout line parses as JSON; event sequence for the good file is `queued`,`started`,…,`completed`; `completed.written` files exist; nothing non-JSON on stdout.
  - relaunch cancel-flip: insert an `in_progress` row directly via rusqlite, run any `humm add` → row now `canceled` with `time_ended`.
  - `-s custom` with no settings → exit 1, message names `models.custom_model_url` and the settings path; no `--help` hint. `nonexistent-language` case keeps its hint (usage).
- [ ] **Step 3 (`#[ignore]` acceptance tests — real model + real audio; run with sandbox lifted):**
  - **kill -9 acceptance (the milestone's headline):** spawn `humm add audio-long.mp3 --txt -s tiny` (real tiny model pre-fetched via `ModelDownloader` per M1's contract-test pattern), poll the DB until the row is `in_progress`, SIGKILL the process, run `humm add` again on a trivial input → old row is `canceled`, `load_task_spec` on it returns the full original `TaskSpec` (retryable full options — the acceptance criterion verbatim).
  - **Ctrl-C:** spawn the same, wait for `in_progress`, send SIGINT → process exits 130 within ~5 s, row `canceled`.
  - **URL e2e (needs yt-dlp):** local HTTP server serving a testdata file; `humm add http://127.0.0.1:<port>/whisper-french.mp3 --txt -s tiny` → exit 0, row has `source='url_import'`, `url` set, `file` updated to a path under the media dir, export written next to it.
- [ ] **Step 4:** Implement; write `docs/HUMM_SURFACE.md` (grammar table, JSON schema, exit codes 0/1/2/130, env vars incl. the M2 additions, file locations); ADR-007 exit-130 note; gates; run Step 3's ignored tests (sandbox lifted) and report results.
- [ ] **Step 5: Commit** `M2: CLI wired to store+queue — URLs, Ctrl-C cancel, --json, ETA, exit codes (F-002, F-024, F-025, F-026, F-029)`.

---

### Task 12: Differential check vs Buzz `add` artifacts (milestone acceptance; F-045 diff note)

**Files:**
- Create: `validation/m2-differential/README.md`, `validation/m2-differential/diff_artifacts.py` (or a small Rust bin under the same dir — NOT in the workspace; `validation/*` is excluded)
- Possibly create: `validation/m2-differential/buzz-artifacts/` (generated Buzz exports, committed as fixtures)

**Context:** M2 acceptance: "differential check vs Buzz `add` artifacts". PARITY_MATRIX F-045 note: humm's VAD timestamps are sample-exact vs Buzz's per-segment linear rescale — **the diff must tolerance-compare timestamps, not byte-compare**.

**Protocol (README documents whatever actually happened — honesty over ceremony):**
1. Try to produce Buzz artifacts on this machine: a Python 3.12 venv for Buzz + `QT_QPA_PLATFORM=offscreen python main.py add <testdata>/whisper-french.mp3 --txt --srt --vtt -s tiny -m whispercpp -d <out>` (Buzz's whisper.cpp backend = closest engine match). **Time-box: if Buzz cannot be made to run within ~30 minutes of effort, stop**, record exactly what blocked it in the README, and fall back to the golden fixtures already ported from Buzz's test suite (M1 snapshots) as the structural baseline — with the gap noted in the parity matrix (F-045 diff stays pending, reason recorded).
2. Produce humm artifacts: `humm add` same file/flags/model size.
3. Compare (the script): SRT/VTT/TXT structural skeleton identical (numbering, arrow lines, header, blank-line layout — reuse the byte-format law from M1's writers); segment counts within ±20 %; for segments matched by order, start/end within ±200 ms tolerance; report transcript-text similarity (informational — text may differ per project rules). Script exits nonzero on structural/tolerance failure.
4. Record results (pass/fail per format + numbers) in the README.

**Steps:**

- [ ] **Step 1:** Attempt Buzz artifact generation (time-boxed). Whatever the outcome, commit the evidence.
- [ ] **Step 2:** Write the compare script + run it; record results.
- [ ] **Step 3: Commit** `M2: differential harness vs Buzz add artifacts (F-045 diff, milestone acceptance)`.

---

### Task 13: Benches guard, cosmetic sweep, parity matrix, milestone report

**Files:**
- Modify: `docs/analysis/PARITY_MATRIX.md`, `crates/humm-core/src/pipeline.rs` (doc line), `.github/workflows/ci.yml` (concurrency group), `docs/HUMM_SURFACE.md` (final sweep), `docs/benchmarks/M2.md` (new)

**Steps:**

- [ ] **Step 1 — benchmark regression gate:** run the criterion suite (sandbox lifted for GPU benches) and compare against `docs/benchmarks/M1.md`; a transcription-path regression is a milestone-blocking failure to fix, not a note (CLAUDE.md). Record numbers in `docs/benchmarks/M2.md` (same format as M1's).
- [ ] **Step 2 — cosmetic backlog (M1 tickets):** `EngineTuning.use_gpu` doc line ("consumed by the binary at engine load, not run_file_job"); CI concurrency group (`concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }`) fixing the push+PR double-run. (The humm-core workspace-dep centralization already landed in Task 3.)
- [ ] **Step 3 — parity matrix update** (only statuses whose stated verification method ACTUALLY ran; cite the test/bench that ran in the status note): F-002 (full grammar cli-int tested; note openaiapi/`-e` accepted-but-not-yet-runnable → stays `implemented` with the note, or `verified` for the grammar half per the row's method — follow the row's stated method), F-006 → implemented (check logic unit-tested; GUI notify M3), F-022 → URL half implemented (+contract if run), F-024 → verified if unit+cli-int both ran, F-025 → CLI half done (unit + cli-int; gui-manual pending M3), F-026 → cancel half (kill-9/Ctrl-C tests), F-029 → verified (unit + cli-int incl. startup flip + Buzz-import fixture), F-042/F-043 → full (note custom/HF mechanics + seam/416 tests), F-045 → diff row per Task 12's actual outcome.
- [ ] **Step 4 — milestone report** (final message of the branch, per CLAUDE.md): matrix status table, benchmark numbers, deviations with ADR refs (exit 130, `--no-playlist`, progress-write throttling, dropped missing-file fast path, custom-model no-SHA), analysis-doc corrections made, M1-ticket disposition (each ticket → done/where), open items for M3 (openaiapi/`-e` runtime gaps, GUI halves of F-025/F-026, whisper-rs callback-leak upstream ticket unchanged).
- [ ] **Step 5: Commit** `M2: parity matrix + benches + docs sweep` and run the full gate suite one final time.

---

## Self-review notes (against the M2 spec)

- **Spec coverage:** humm-store schema/repository/transitions/Buzz-import (Tasks 3–4) ✓; task queue + event bus (Task 5) ✓; yt-dlp adapter + contract tests + graceful absence (Task 6) ✓; custom models ggml URL + HF repo per Q-2 (Task 8) ✓; full CLI grammar incl. URLs, `--json`, exit codes, Ctrl-C (Tasks 10–11) ✓; settings file + `HUMM_*` env surface (Task 2 + Constraint 9 + HUMM_SURFACE.md) ✓; F-006 check logic (Task 9) ✓; F-042/F-043 completion incl. M1 downloader tickets (Tasks 7–8) ✓; acceptance criteria: kill -9 (Task 11 Step 3), Buzz DB import fixture (Task 4), grammar matrix cli-int (Tasks 10–11), differential check (Task 12) ✓; M1 review tickets all placed (seam/416/pinning → 7, cancel-interleaving → 5, messaging split → 11, cosmetics → 3/13) ✓.
- **Deliberate exclusions:** `-m openaiapi` and `-e` execute in M6/M8 (grammar-only here, actionable errors — honest not-yet gaps recorded in the matrix); SecretStore/keyring deferred to M6 with the API backend; restart/rename/notes/delete beyond cancel are F-026's GUI half (M3).
- **Type consistency check:** `TaskStore` names match between Task 3 (definition), Task 5 (fake), and Task 11 (`Arc<SqliteStore>`); `MediaFetcher`/`FetchProgress` are defined in Task 5 (core boundary) and implemented in Task 6 (already reflected in both tasks' Files/Interfaces blocks); `TranscriptionId`/`TaskSpec`/`Status`/`Source` (Task 1) are used with identical names in Tasks 3, 4, 5, and 11; `ensure_custom_model`/`resolve_hf_repo` (Task 8) match Task 11's model-resolution step; `Settings` field paths (`models.custom_model_url`, `downloads.cookie_file`, `updates.*`) match between Tasks 2, 8, 9, and 11.
