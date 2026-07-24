# M6 — Cloud: API backend + LLM translation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the OpenAI-compatible API transcription backend (F-041), the LLM translator with its three surfaces — file-task translation (F-031), live recording (F-063), viewer Translate (F-085) — the `SecretStore`/keyring realization, the CLI `openaiapi` arms, and the carried `finish()` ADR.

**Architecture:** Everything HTTP lives in `humm_core::openai` (pure-Rust services in core, ARCHITECTURE §1); the only new non-Rust touchpoints are an `AudioEncoder` boundary implemented by the existing ffmpeg adapter and a `SecretStore` implemented by `keyring` in humm-store. The translator is one worker type reused by all three surfaces; the API key exists only in the keyring, process memory, and `Authorization` headers.

**Tech Stack:** reqwest blocking (+`multipart` feature), keyring crate, existing flume/mpsc/thread patterns. No tokio.

**The governing spec:** `docs/superpowers/specs/2026-07-24-m6-cloud-api-llm-design.md` — every task's implementer MUST read it first (at minimum §1 + the §§ their task names), plus the Buzz sources their task cites. Implement from Buzz source, not from this plan's paraphrase, wherever the two could disagree.

## Global Constraints

1. Every commit passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — run with `export CARGO_HOME="$PWD/.cargo-home"` (worktree-local; already seeded). Commit messages: `M6: <what> (F-###[, F-###])`.
2. Sandbox: GPU/Metal, TCP binds, CoreAudio, and writes outside the worktree are blocked. The pre-M6 TCP-bind-blocked baseline is **20 tests** (17 `models::` + 3 `live::export`); every new `TestServer`-using test must be added to the running count in its task's commit message ("TCP-bound test count now N") — growth is expected and documented, never silent.
3. New network-*dependent* tests (real endpoints) are `#[ignore]` contract tests; the mock server carries the sandboxed load.
4. The API key must never appear in: `settings.toml`, any DB column, log output (key-carrying structs get redacting manual `Debug` impls), subprocess argv, or any serialized options type. There is no key field on any `Serialize` type.
5. Buzz-byte-exact user-facing strings are called out per task; all user-visible strings go through `assets/i18n/en/humm.ftl` with stable keys.
6. Python-semantics ports: `int()` = truncation toward zero; `.strip()` = Unicode-whitespace trim both ends; `re.split(r'\[(\d+)\]\s*', s)` alternating-parts semantics — port these exactly (they are differential-tested).
7. `humm-gui` lib tests run in parallel: never `std::env::set_var` in a test — thread values explicitly (established convention).
8. kittest cannot bind TCP under the sandbox and cannot drive real dialogs/viewports — use the scripted seams each task defines.
9. Do not touch `transcribe_live`'s engine internals (the CoreML per-chunk ticket stays carried; spec §0).
10. New public core items need doc comments (`#![warn(missing_docs)]`).

## File structure (created/modified across the milestone)

```
crates/humm-core/src/
  openai/mod.rs            # module root + shared OpenAiConfig types
  openai/client.rs         # base-URL normalize, bearer, redacted Debug, cancelable_request
  openai/engine.rs         # OpenAiEngine (F-041)
  openai/parse.rs          # response → Vec<Segment> (all branches)
  openai/translate.rs      # batch protocol + TranslatorHandle worker + endpoint resolution
  openai/validate.rs       # Test-button key check
  secrets.rs               # SecretStore trait + SecretKey + SecretError + MemorySecretStore
  boundary.rs              # + AudioEncoder trait
  domain.rs                # + Backend, ModelSpec.backend, TranscriptionOptions.llm, LlmTranslationOptions
  settings.rs              # + OpenAiSettings; FileTranscriberSettings/RecordingSettings additions
  pipeline.rs              # F-031 fan-out/drain
  queue.rs                 # RunnerDeps.llm + encoder, backend-gated ensure, finish_within
  live/translation.rs      # TranslationApplier (F-063 core half)
  live/export.rs           # translated-path writes
  models/test_server.rs    # per-method canned + multipart-visible bodies
crates/humm-media/src/encode.rs        # FfmpegDecoder::encode_mp3 impl (+ mod wire-up in lib.rs)
crates/humm-store/src/secrets.rs       # KeyringSecretStore
crates/humm-store/src/store.rs         # append_live_segment -> i64
crates/humm-cli/src/{args,run}.rs      # openaiapi arms
crates/humm-gui/src/
  translate_seam.rs        # TranslateSpawner trait + real/scripted impls
  llm_source.rs            # GuiLlmSource (LlmEndpointSource impl)
  viewer.rs, views/viewer.rs             # F-085
  recording.rs, live_session.rs          # F-063 wiring
  views/recording_advanced.rs            # LLM block live
  views/preferences.rs, views/prefs_general.rs   # General tab
  views/options_sheet.rs                 # backend + key + LLM rows
  views/presentation.rs                  # translation region
validation/m6-differential/gen_batch_parse_fixtures.py (+ stubs/, fixtures → crates/humm-core/tests/fixtures/batch_parse/)
docs/… (HUMM_SURFACE, DECISIONS ADR-022, PARITY_MATRIX, UX_SPEC, ARCHITECTURE, gui-manual/M6.md, benchmarks/M6.md)
```

---

### Task 1: Domain + settings groundwork

**Files:** Modify `crates/humm-core/src/domain.rs`, `crates/humm-core/src/settings.rs`.
**Spec:** §2.1, §9. **Produces (later tasks consume):**
`Backend { Whisper (default), OpenAiApi }` (serde snake_case wire: `whisper` / `openai_api`);
`ModelSpec { backend: Backend, size, hf_repo }` with `#[serde(default)]` on `backend`;
`LlmTranslationOptions { model: String, prompt: String }`;
`TranscriptionOptions.llm: Option<LlmTranslationOptions>` with `#[serde(default)]`;
`OpenAiSettings { base_url: Option<String>, api_model: String /* default "whisper-1" */ }` at `settings.openai`;
`FileTranscriberSettings += backend: Option<String>, enable_llm_translation: bool (false), llm_model: Option<String>, llm_prompt: Option<String>`;
`RecordingSettings += enable_llm_translation: bool (false), llm_model: Option<String>, llm_prompt: Option<String>`.

- [ ] Failing tests first (`domain.rs` + `settings.rs` test mods): `options_json_without_backend_or_llm_deserializes_to_whisper_none` (paste a pre-M6 `TaskSpec` JSON literal); `backend_round_trips_through_options_json`; `llm_options_round_trip`; `openai_settings_default_model_is_whisper_1`; `partial_openai_section_fills_defaults`; recording/file-transcriber default tests mirroring existing ones.
- [ ] Implement; every existing test stays green (`ModelSpec` literals in tests gain `backend: Backend::Whisper` or use `..Default::default()`).
- [ ] Gates; commit `M6: Backend/LLM domain types + openai settings groundwork (F-041, F-031)`.

### Task 2: SecretStore + keyring

**Files:** Create `crates/humm-core/src/secrets.rs` (wire into `lib.rs`); create `crates/humm-store/src/secrets.rs`; modify `crates/humm-store/Cargo.toml` + workspace `Cargo.toml` (add `keyring`, current stable version — check crates.io at implementation time, pin per ADR-012).
**Spec:** §5. **Produces:**

```rust
pub enum SecretKey { OpenAiApiKey }            // entry_name() -> "OpenAI API key"
pub const SERVICE: &str = "humm";
#[derive(Debug, thiserror::Error)] pub enum SecretError { /* Backend { detail: String } */ }
pub trait SecretStore: Send + Sync {
    fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError>;
    fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError>;
    fn delete(&self, key: SecretKey) -> Result<(), SecretError>;
}
pub struct MemorySecretStore { /* Mutex<HashMap<SecretKey, String>> */ }  // NOT cfg(test): binaries' tests use it
```

`humm_store::KeyringSecretStore` maps `keyring::Entry::new(SERVICE, key.entry_name())`; `NoEntry` → `Ok(None)`; empty stored string → `Ok(Some(""))` is fine (callers treat empty as absent).

- [ ] Unit tests on `MemorySecretStore` (get-absent-None, set/get/delete round trip); `#[ignore = "touches the real OS keychain"]` contract test in `crates/humm-store/tests/keyring_contract.rs`: set → get → delete → get-None round trip under a throwaway entry value.
- [ ] Run the contract test once sandbox-lifted; if the OS blocks it (TCC-like prompt), record the exact failure in the task report — it becomes a documented environment blocker, the test stays committed.
- [ ] Gates; commit `M6: SecretStore trait + keyring impl (ADR-012)`.

### Task 3: AudioEncoder boundary + ffmpeg encode

**Files:** Modify `crates/humm-core/src/boundary.rs`; create `crates/humm-media/src/encode.rs`; modify `crates/humm-media/src/lib.rs`, `crates/humm-media/tests/contract.rs`.
**Spec:** §3.1. **Produces:**

```rust
pub trait AudioEncoder: Send + Sync {
    fn encode_mp3(&self, pcm: &Pcm, dest: &Path, cancel: &CancelToken) -> Result<(), MediaError>;
}
```

impl on `FfmpegDecoder`: `ffmpeg -nostdin -threads 0 -f f32le -ar 16000 -ac 1 -i - -loglevel error <dest>` with PCM bytes streamed to stdin from a writer thread (reuse the crate's spawn/cancel/error-mapping helpers; kill on cancel).

- [ ] Contract tests (`#[ignore = "requires ffmpeg + Buzz testdata"]`): `encode_mp3_duration_matches_pcm` (synthesized 3 s sine `Pcm` → encode → `FfprobeProber` duration within ±100 ms), `encode_mp3_round_trips_through_decode` (encode → `decode` → sample count within ±5 % of input), `encode_mp3_cancel_kills_promptly`.
- [ ] Run them sandbox-lifted (`cargo test -p humm-media -- --ignored`); gates; commit `M6: AudioEncoder boundary + ffmpeg mp3 encode (F-041)`.

### Task 4: openai::client + validate + TestServer extension

**Files:** Create `crates/humm-core/src/openai/{mod.rs,client.rs,validate.rs}`; modify `crates/humm-core/src/models/test_server.rs`, workspace `Cargo.toml` (reqwest `multipart` feature).
**Spec:** §2, §7.1 (validate flow), §8 (cancelable requests). **Produces:**

```rust
pub struct OpenAiAuth { pub base_url: Option<String>, pub api_key: String } // manual Debug: key => "«redacted»"
pub fn effective_base(base_url: &Option<String>) -> String; // default https://api.openai.com/v1, trim trailing '/'
/// Runs `send` on its own thread; polls `cancel` every ~100 ms while waiting.
/// Cancel => Err(Canceled-mapped) immediately, request thread detached.
pub fn cancelable<T: Send + 'static>(cancel: &CancelToken,
    send: impl FnOnce() -> Result<T, ApiFailure> + Send + 'static) -> Result<T, ApiFailure>;
pub enum ApiFailure { Canceled, Http { detail: String }, Shape { detail: String } }
// validate.rs — spec §7.1 exactly:
pub enum ValidateOutcome { Valid, Invalid { message: String } }
pub fn validate_key(auth: &OpenAiAuth) -> ValidateOutcome; // custom-base raw probe (5 s) then GET /models (15 s)
```

TestServer: add `Canned::for_method(method, canned)` (per-method routing; existing path-only behavior unchanged for current tests) and keep recording full bodies (multipart bodies arrive as bytes — assertions substring-match on part names/filenames).

- [ ] Unit: `effective_base` matrix (None → default; custom with/without trailing slash); redacted `Debug` test (`format!("{auth:?}")` contains no key material); `cancelable_returns_promptly_on_cancel` (send closure sleeps 5 s; pre-canceled token returns < 1 s).
- [ ] Mock-server tests (TCP): `validate_key_success_against_mock`, `validate_key_custom_base_probe_failure_yields_buzz_message` (asserts the exact "OpenAI API returned invalid response. Please check the API url or your key. …" string), `validate_key_auth_error_surfaces_error_message_json`. TestServer self-tests for per-method canned.
- [ ] Gates; commit `M6: openai client plumbing + key validation (F-041, F-100)` noting the new TCP-bound test count.

### Task 5: Response parsing (pure)

**Files:** Create `crates/humm-core/src/openai/parse.rs`.
**Spec:** §3.4 — and re-read `openai_whisper_api_file_transcriber.py:19-36,185-290` line by line before coding. **Produces:**

```rust
pub fn response_format_for(api_model: &str) -> &'static str;   // "json" iff starts_with("gpt-4o") else "verbose_json"
pub fn parse_response(body: &serde_json::Value, offset_ms: i64, word_timestamps: bool)
    -> Result<Vec<Segment>, ApiFailure>;
```

Port faithfully: segments fallback chain (`segments` key → gpt-4o `{text,0,0}` fallback when only `text` exists); top-level `words`; whisper.cpp detection = first segment object has ALL of `tokens`+`avg_logprob`+`no_speech_prob`; whisper.cpp word-merge on byte buffers (`Vec<u8>`, `String::from_utf8` guard keeps accumulating on failure; `b" "` prefix flush-then-restart, `b", "` prefix append-comma-flush-strip; centisecond→ms `*10`; empty-buffer flush = no-op success); standard word branch and no-word branch use `(seconds * 1000.0) as i64 + offset_ms` truncation; missing `start`/`end` → 0.

- [ ] Tests (all pure JSON literals, no network): verbose_json basic; offset applied; gpt-4o text-only; missing-times default 0; word branch standard; whisper.cpp branch — multi-byte UTF-8 split across words (construct tokens whose bytes split a multi-byte char so the guard path runs), comma-prefix arm, space-prefix arm, empty-buffer skip; unknown shape → `Shape` error; `response_format_for` both arms.
- [ ] Gates; commit `M6: OpenAI response parsing incl. whisper.cpp word-merge port (F-041)`.

### Task 6: OpenAiEngine (F-041)

**Files:** Create `crates/humm-core/src/openai/engine.rs`.
**Spec:** §3 whole. **Consumes:** Tasks 1–5. **Produces:**

```rust
pub struct OpenAiEngineConfig { pub auth: OpenAiAuth, pub api_model: String }
pub struct OpenAiEngine { /* cfg, encoder: Arc<dyn AudioEncoder>, client */ }
impl OpenAiEngine { pub fn new(cfg: OpenAiEngineConfig, encoder: Arc<dyn AudioEncoder>) -> Self }
impl TranscriptionEngine for OpenAiEngine { … }  // caps: word_timestamps+translate true, vad false
pub const MAX_CHUNK_SIZE: u64 = 25 * 1024 * 1024;
pub fn chunk_plan(total_size: u64, pcm_len: usize) -> Vec<Range<usize>>; // pure: Buzz ceil math over samples
```

Flow per spec §3.2/§3.3: temp-dir mp3 (tempfile-free: use `std::env::temp_dir()` + uuid names, delete on all paths incl. error/cancel via a scope guard); `sink.on_progress(0.0)`; single-vs-chunked; per-chunk multipart POST via `cancelable` (600 s client timeout); Transcribe → `/audio/transcriptions` (+`language` only when `Some`), Translate → `/audio/translations`; `prompt` = initial_prompt; `timestamp_granularities[]=word` when word_timestamps; parse → `sink.on_segment` each → chunk progress `(i+1)/n`.

- [ ] Unit (pure): `chunk_plan` matrix — under-limit → 1 range; exactly 25 MiB+1 → 2 equal-duration ranges; ranges cover the PCM exactly, no overlap (Buzz formula: `n = ceil(size/MAX)`, equal durations).
- [ ] Mock-server tests (TCP; scripted `AudioEncoder` fake writing a fixed-size file so chunking triggers without real ffmpeg): single-file happy path asserts multipart part names (`file`,`model`,`response_format`,`prompt`) + segments + progress `0.0` then completion; translate task hits `/audio/translations` and omits `language`; language omitted when `None`/sent when `Some`; word-granularity part present; chunked path → N requests with correct per-chunk offsets in returned segments + progress steps; 401 → `EngineError::Api` whose message names status + host and contains **no** key; pre-canceled token → `Canceled` fast.
- [ ] Gates; commit `M6: OpenAiEngine — API file transcription (F-041)` + TCP count.

### Task 7: Batch protocol + differential fixtures

**Files:** Create `crates/humm-core/src/openai/translate.rs` (protocol half only); create `validation/m6-differential/gen_batch_parse_fixtures.py` + `stubs/`; fixtures → `crates/humm-core/tests/fixtures/batch_parse/*.json`.
**Spec:** §4.1 — re-read `translator.py:82-145` first. **Produces:** `BATCH_SIZE`, `combine_numbered`, `batch_system_prompt`, `parse_batch_response` exactly as spec'd.

- [ ] Unit tests: happy 3-item; missing `[2]` → `""`; duplicates → last wins; preamble prose discarded; no markers → all `""`; extra `[9]` ignored; whitespace variants after `]`; strip semantics (trailing newline trimmed); multi-line entry preserved internally.
- [ ] Fixture generator: stub-import Buzz's unmodified `Translator._parse_batch_response` (anaconda `python3`; `sys.modules` stubs for `PyQt6`, `openai`, `keyring`, `buzz.locale`, `buzz.settings.settings`, `buzz.store.keyring_store`, `buzz.transcriber.transcriber`, `buzz.widgets.transcriber.advanced_settings_dialog` — the static method needs none of them at call time); ~12 cases incl. CJK; write `{response, expected_count, parsed}` JSON. A Rust test iterates every fixture asserting byte-equality.
- [ ] Run generator, commit fixtures + generator + tests; gates; commit `M6: translation batch protocol + Buzz differential fixtures (F-031)`.

### Task 8: TranslatorHandle worker + endpoint resolution

**Files:** Extend `crates/humm-core/src/openai/translate.rs`.
**Spec:** §4.2, §4.3. **Produces:**

```rust
pub struct TranslatorConfig { pub auth: OpenAiAuth, pub model: String, pub prompt: String }
pub enum TranslatorMsg { Item { text: String, id: Option<i64> },
                         UpdateOptions { model: String, prompt: String }, Stop }
pub struct TranslatorHandle { /* sender + join */ }
impl TranslatorHandle {
    pub fn spawn(cfg: TranslatorConfig, on_result: Box<dyn FnMut(String, Option<i64>) + Send>) -> Self;
    pub fn enqueue(&self, text: String, id: Option<i64>);
    pub fn update_options(&self, model: String, prompt: String);
    pub fn stop(self);          // send Stop + join (bounded by per-request 60 s timeouts)
    pub fn stop_detached(self); // send Stop, do NOT join (GUI close path)
}
pub struct LlmEndpoint { pub auth: OpenAiAuth }
pub fn resolve_endpoint(env_base: Option<String>, env_key: Option<String>,
    settings_base: &Option<String>, secrets: &dyn SecretStore) -> Option<LlmEndpoint>;
pub const ENV_BASE: &str = "HUMM_TRANSLATION_API_BASE_URL"; // Buzz: BUZZ_TRANSLATION_API_BASE_URl (upstream typo)
pub const ENV_KEY: &str  = "HUMM_TRANSLATION_API_KEY";
```

Worker loop = spec §4.2 verbatim (block-first, drain ≤9, Stop-mid-drain honored after the batch, 1→single-request form, N→numbered form, every item exactly one callback, errors→`""`, 60 s timeout, disconnect ≡ Stop). Requests via `cancelable`-style thread? No — the worker thread itself is disposable; plain blocking send with the 60 s timeout is fine (Stop is honored between requests; ADR-022's prompt-cancel guarantee is for the *task/queue* path, which drives this via `stop`/abandon).

- [ ] Mock-server tests (TCP): single item → single-form request body (system == prompt verbatim, user == text); 10 items pre-queued → exactly one batched request whose user content is `combine_numbered` output and system contains "You will receive 10 numbered texts"; degraded response (missing numbers) → `""` callbacks for the missing ids in order; HTTP 500 → all-`""` for the batch; `UpdateOptions` mid-stream → next request uses new model; Stop drains queued items first; drop-without-stop ends worker.
- [ ] Unit: `resolve_endpoint` precedence matrix (env-key wins over keyring; env-base over settings; no key anywhere → None) using `MemorySecretStore`.
- [ ] Gates; commit `M6: LLM translator worker + endpoint resolution (F-031, F-063, F-085)` + TCP count.

### Task 9: F-031 — pipeline/queue integration + backend routing

**Files:** Modify `crates/humm-core/src/pipeline.rs`, `crates/humm-core/src/queue.rs`; `crates/humm-store/src/store.rs` only if `mark_completed` drops translations (verify: its segment INSERT must write `segment.translation`).
**Spec:** §6, §2.1 (skip ensure/VAD for `OpenAiApi`). **Produces:**

```rust
pub trait LlmEndpointSource: Send + Sync { fn resolve(&self) -> Option<LlmEndpoint> }
// RunnerDeps += llm: Option<Arc<dyn LlmEndpointSource>>, encoder: Option<Arc<dyn AudioEncoder>>
// run_file_job signature += llm: Option<&dyn LlmEndpointSource>
```

Queue: for `Backend::OpenAiApi` tasks, skip `ArtifactEnsurer` model+VAD phases entirely (no download events). `EngineFactory` impls receive the spec and route on `backend` (binaries wire this in Tasks 11/12-adjacent — core's scripted test factories gain an API arm here).
Pipeline: when `job.options.llm` is `Some` and `llm.resolve()` is `Some`, spawn `TranslatorHandle`; `TeeSink` enqueues each trimmed segment with `id = Some(ordinal as i64)`; results collected into a `Mutex<Vec<(usize, String)>>` via the callback; after `engine.transcribe` Ok → `handle.stop()` (bounded join) → merge `segments[i].translation = t` → exports → return. Cancel mid-drain → `stop_detached` + `Canceled`. Endpoint `None` or spawn failure → `log::warn!`, task proceeds untranslated.

- [ ] Queue/pipeline tests (scripted engine + real translator against TestServer): `file_task_with_llm_stores_per_segment_translations` (end-to-end through the runner: enqueue → completed → store `get_segment_rows` shows translations); `llm_batch_error_yields_empty_translations_and_completed_task`; `no_key_resolves_none_task_completes_untranslated_with_warning`; `cancel_during_translation_drain_yields_canceled`; `openaiapi_task_skips_model_ensure_and_vad` (ensurer-call recorder asserts zero calls); old scripted-factory tests untouched.
- [ ] Gates; commit `M6: per-task file translation through the queue (F-031)` + TCP count.

### Task 10: ADR-022 — finish_within + the cancel story

**Files:** Modify `crates/humm-core/src/queue.rs`; `docs/design/DECISIONS.md` (append ADR-022 per spec §8, full context/options/decision/rationale/validation form).
**Produces:** `pub fn finish_within(self, timeout: Duration) -> Result<bool, CoreError>` — sends `Finish`, joins with deadline (join on a helper thread + `recv_timeout`); expiry → `log::error!`, deliberately leak the runner join handle, return `Ok(false)`. `finish()` unchanged. GUI shutdown (`services.rs` teardown) switches to `cancel_all()` + `finish_within(Duration::from_secs(5))`.

- [ ] Tests: `finish_within_returns_true_when_drained` (empty queue); `finish_within_times_out_on_wedged_engine` (scripted engine blocking on a never-signaled condvar → `Ok(false)` within ~2× the 1 s test timeout, process continues); existing finish tests green.
- [ ] Write ADR-022 into DECISIONS.md (decision summary = spec §8: unbounded `finish()` retained for CLI drain; prompt cancel via `cancelable` request threads; `finish_within` belt-and-braces; Buzz's no-op `stop()` contrast).
- [ ] Gates; commit `M6: finish_within + ADR-022 cancellation policy`.

### Task 11: CLI arms (F-002 completion)

**Files:** Modify `crates/humm-cli/src/run.rs` (+`args.rs` help text), `crates/humm-cli/tests/cli.rs`, `crates/humm-cli/Cargo.toml` (dep: humm-store already present — verify), `docs/HUMM_SURFACE.md` §1.
**Spec:** §7.5. Token: `--openai-token` else `KeyringSecretStore.get(OpenAiApiKey)` else fail `"No OpenAI access token found"` (Buzz-exact, exit 1, no `--help` hint — it's a config error). `CliEngineFactory` gains the API arm (`OpenAiEngine::new` from `settings.openai` + resolved token + `FfmpegDecoder` as encoder); model-resolution/VAD-download pre-steps in `run.rs::execute` gated to local backends. `-e` untouched (still exit 1 — spec §0). RunnerDeps.llm = None.

- [ ] cli-int tests: `openaiapi_without_token_or_keyring_fails_actionably` (assert exact message + exit 1; point `HUMM_CONFIG_DIR`/data at temp, and inject a temp-keyring? No — the CLI must not touch the real keychain in tests: give `run.rs` a `#[cfg(test)]`-reachable seam? Simplest honest seam: `HUMM_SECRETS_FILE` is NOT added — instead the test passes `--openai-token ""`? Buzz treats empty flag as unset. **Decision:** add an internal env `HUMM_DISABLE_KEYRING` (test-only, documented next to `HUMM_DISABLE_VAD` in HUMM_SURFACE §5) making the CLI's secret lookup return None — hermetic tests set it); `openaiapi_with_token_transcribes_against_mock` (settings file in temp `HUMM_CONFIG_DIR` sets `[openai] base_url` to a TestServer URL started in-test — the binary does the POST; assert exports written + exit 0 + no `model_download_progress` events in `--json`); `openaiapi_grammar_still_parses` (existing grammar tests updated — the old `not yet available` test flips to expecting success-shaped validation).
- [ ] Update HUMM_SURFACE §1 rows + §5 (new envs incl. `HUMM_DISABLE_KEYRING`, `HUMM_TRANSLATION_API_*` with the upstream-typo note).
- [ ] Gates; commit `M6: CLI openaiapi/--openai-token runtime arms (F-002, F-041)` + TCP count.

### Task 12: GUI plumbing — engine factory, llm source, translate seam

**Files:** Modify `crates/humm-gui/src/engine_factory.rs`, `crates/humm-gui/src/services.rs`; create `crates/humm-gui/src/llm_source.rs`, `crates/humm-gui/src/translate_seam.rs`.
**Spec:** §6, §7 preamble. **Produces:**

```rust
// llm_source.rs
pub struct GuiLlmSource { pub settings: Arc<Mutex<Settings>>, pub secrets: Arc<dyn SecretStore> }
impl LlmEndpointSource for GuiLlmSource { … } // env → settings.openai.base_url → keyring, via resolve_endpoint
// translate_seam.rs — the one seam both viewer + recording use; kittest injects Scripted
pub trait TranslateSpawner: Send + Sync {
    fn spawn(&self, cfg: TranslatorConfig,
             on_result: Box<dyn FnMut(String, Option<i64>) + Send>) -> Box<dyn TranslateControl>;
}
pub trait TranslateControl: Send {
    fn enqueue(&self, text: String, id: Option<i64>);
    fn update_options(&self, model: String, prompt: String);
    fn stop(self: Box<Self>);
}
pub struct RealTranslateSpawner;                       // wraps TranslatorHandle (stop_detached)
pub struct ScriptedTranslateSpawner { /* fn(text)->String mapper, synchronous callback */ }
```

`GuiEngineFactory` gains `settings: Arc<Mutex<Settings>>` + `secrets: Arc<dyn SecretStore>` + `encoder: Arc<FfmpegDecoder>`; `create` routes `Backend::OpenAiApi` → `OpenAiEngine` (key from keyring; missing → `CoreError` with the Preferences pointer: "no OpenAI API key — set one in Preferences → General"). `DownloaderEnsurer` short-circuits for API backend (no-op ensure). `services::init` wires `secrets: Arc<KeyringSecretStore>`, `RunnerDeps.llm = Some(GuiLlmSource)`, `RunnerDeps.encoder`; `TestDeps` gains injectable `secrets`/`llm`/`translate` (defaults: `MemorySecretStore`, fixed-None llm, `ScriptedTranslateSpawner`). `HummApp` holds `translate: Arc<dyn TranslateSpawner>` + `secrets` for the surfaces.

- [ ] Unit: `gui_llm_source_precedence` (explicit env args threaded — Constraint 7); `engine_factory_api_arm_missing_key_yields_actionable_error` (MemorySecretStore empty); `engine_factory_api_arm_builds_engine_with_settings` (smoke: returns Ok engine).
- [ ] Gates; commit `M6: GUI engine-factory API arm + llm/translate seams (F-041, F-031)`.

### Task 13: Preferences → General tab (partial F-100)

**Files:** Create `crates/humm-gui/src/views/prefs_general.rs`; modify `views/preferences.rs`, `assets/i18n/en/humm.ftl`, `crates/humm-gui/tests/kittest_prefs.rs`.
**Spec:** §7.1 — every Buzz string byte-exact (with Buzz→humm product-name substitution in the success message). `PrefsTab::General` first in the strip. State `GeneralTabState { key_buffer: String, key_visible: bool, dirty: bool, test_running: bool, test_rx: Option<Receiver<ValidateOutcome>>, modal: Option<PrefsModal> }`. Key field: `TextEdit::singleline().password(!key_visible)` + eye toggle button; seeded from `secrets.get` at tab construction; commit on `lost_focus() || enter` → charset check (`^[A-Za-z0-9+/=_-]*$` — regex in code, warn-modal with Buzz's exact two strings, still save) → `secrets.set` (failure → "Unable to save OpenAI API key to keyring" modal). Test button: `add_enabled(!key.is_empty() && !test_running)`; click spawns thread running `openai::validate::validate_key` (base URL read from settings at click time), result over mpsc, modal on arrival. Base-URL + api-model fields save to settings on change (empty base → None). A `ValidateFn` seam (`Arc<dyn Fn(OpenAiAuth) -> ValidateOutcome + Send + Sync>`, default = real) so kittest never networks.

- [ ] kittest: `general_tab_key_field_persists_to_secret_store` (type via state seam + commit → MemorySecretStore holds it); `test_button_disabled_without_key_enabled_with`; `test_button_success_shows_valid_modal` (scripted ValidateFn) asserting the byte-exact success string; `base_url_and_model_fields_persist_to_settings`.
- [ ] Unit: charset-regex accept/reject vectors; empty-base→None mapping.
- [ ] Gates; commit `M6: Preferences General tab — key/Test/base-url/model (F-100 partial, F-041)`.

### Task 14: Options sheet — backend, Api Key, LLM block (F-023 ext)

**Files:** Modify `crates/humm-gui/src/views/options_sheet.rs`, `assets/i18n/en/humm.ftl`, `crates/humm-gui/tests/kittest_import.rs`.
**Spec:** §7.2. Backend combo (`Whisper` / `OpenAI Whisper API`) above the size row; API selected → size combo + tooltip hidden, `Api Key:` masked row shown (seeded from secrets; commit-on-blur writes keyring — same helper as Task 13, extract it into `views/secret_field.rs` if sharing is cleaner). Advanced section gains the Translation settings block: `Enable AI translation` checkbox; `AI model:` = editable `ComboBox` seeded `["gpt-4o-mini","gpt-4o","gpt-4.1-mini"]` + free text; `Instructions for AI:` multiline seeded with the spec's neutral template (`"You are a professional translator. Translate each text sent to you to {language}. Do not add any comments or notes — reply with the translation only."`); both disabled while unchecked; seeded defaults become real values on sheet save (ledger #10). Sticky: `file_transcriber.{backend, enable_llm_translation, llm_model, llm_prompt}`. `build_specs`: backend → `ModelSpec.backend`; checked + non-empty model → `options.llm = Some(LlmTranslationOptions{model, prompt})`.

- [ ] Unit (`options_sheet::` tests): backend round-trips sticky; `build_specs_maps_api_backend`; `build_specs_maps_llm_options_when_enabled`; `llm_none_when_unchecked`; seeded-defaults-become-values-on-save.
- [ ] kittest: API backend selection hides size combo + shows key row; advanced LLM block gates on checkbox; Run with LLM enabled enqueues a spec whose persisted `options_json` contains `llm` (and no key anywhere in the JSON — assert `!json.contains(key)`).
- [ ] Gates; commit `M6: options sheet backend selector + LLM translation block (F-023, F-031, F-041)`.

### Task 15: Viewer Translate (F-085)

**Files:** Modify `crates/humm-gui/src/viewer.rs`, `crates/humm-gui/src/views/viewer.rs`, `assets/i18n/en/humm.ftl`, `crates/humm-gui/tests/kittest_viewer.rs`.
**Spec:** §7.3. `ViewerDeps += secrets: Arc<dyn SecretStore>, translate: Arc<dyn TranslateSpawner>, settings + settings_path` (already partially threaded — reuse). Translate button live; click gates in order: (1) keyring empty → modal "API Key Required"/"Please enter OpenAI API Key in preferences"; (2) sticky `file_transcriber.llm_model/llm_prompt` empty → AI-settings modal (same widgets as Task 14's block, writes sticky settings, OK → continue); (3) spawn (lazy, once) via `translate` with `TranslatorConfig { auth: resolve via GuiLlmSource-equivalent (env→settings→keyring), model, prompt }`, results into an `mpsc` harvested in `viewer_ui` each frame: non-empty → `rows[idx].translation`, `has_translations = true`, `store.update_segment_translation(row.id, &t)`; empty skipped. Enqueue all rows `(text, Some(row.id))`. Re-click enqueues again. Close → `stop()` on the control.

- [ ] Unit (`viewer::` tests, MemorySecretStore + scripted spawner): gate order (no key → modal flag; key but unconfigured → settings-modal flag; configured → spawner called once, rows enqueued in order); `translation_arrival_updates_row_store_and_flag`; `empty_translation_skipped`; `second_click_reuses_worker`.
- [ ] kittest: `translate_end_to_end_updates_rows_live` — seeded 3-row transcript, scripted spawner uppercases text; click Translate through the real button; assert Translation column unhides, store rows carry translations, view-menu Translation entry appears.
- [ ] Gates; commit `M6: viewer post-hoc Translate (F-085)`.

### Task 16: F-063 core half — TranslationApplier + .translated export + row ids

**Files:** Create `crates/humm-core/src/live/translation.rs`; modify `crates/humm-core/src/live/export.rs`, `crates/humm-core/src/live/mod.rs`, `crates/humm-store/src/store.rs` (+ `live_session.rs`'s `LiveStore` trait — note: trait lives in humm-gui; the store change is `append_live_segment -> Result<i64, StoreError>` via `last_insert_rowid`).
**Spec:** §7.4 (consumer half) — re-read `recording_transcriber_widget.py:963-1084` first. **Produces:**

```rust
// live/translation.rs
pub fn strip_newlines(text: &str) -> String;   // \r\n and \n → OS line separator (Buzz :774-775)
pub struct TranslationApplier { /* mode, separator, panel: PanelText, translations: Vec<String>,
                                   merge: MergeState, max_entries, hide_unconfirmed */ }
impl TranslationApplier {
    pub fn apply(&mut self, raw: &str, export: Option<&ExportSink>) -> Option<String>; // None if raw empty
}
// live/export.rs — translated-side writers mirroring on_utterance/on_merged, writing translated_path():
pub fn on_translation_utterance(&self, text: &str, mode: RecordingMode) -> …;
pub fn on_translation_merged(&self, merged: &str) -> …;
```

`apply`: empty → None (Buzz `:1038-1039`); append-below/above → panel gets `strip_newlines(text)` + separator placement per mode, export gets **raw** text (append/prepend/CSV rules identical to the transcript side — reuse the existing writer internals, do not duplicate format code); append-and-correct → push raw onto `translations`, run `process_transcription_merge` (same fn the transcript side uses, incl. `hide_unconfirmed`), panel = merged, export = whole-rewrite of merged. Returns the new panel string.
Store: `append_live_segment` returns the inserted row id; the `LiveStore` trait + `SessionApplier` + every test double updated (mechanical).

- [ ] Unit: `strip_newlines` both arms; mode matrix for `apply` (below appends with separator, above prepends, merge path pins against a hand-computed merge case); empty skip; export bytes per mode (temp dirs; TXT append + prepend + CSV single-row + and-correct whole-rewrite; `.translated` sibling path asserted); snapshot: `translated_txt_append_snapshot` (insta, matching the existing live-export snapshot style); `append_live_segment_returns_ascending_rowids` (humm-store test).
- [ ] Gates; commit `M6: live translation applier + .translated export + segment row ids (F-063)`.

### Task 17: F-063 GUI half — panel, advanced block, session wiring, presentation

**Files:** Modify `crates/humm-gui/src/recording.rs`, `crates/humm-gui/src/live_session.rs`, `crates/humm-gui/src/views/recording.rs`, `views/recording_advanced.rs`, `views/presentation.rs`, `app.rs` (session-message drain), `assets/i18n/en/humm.ftl`, `tests/kittest_recording.rs`, `tests/kittest_presentation.rs`.
**Spec:** §7.4 rest. `RecordingOptions += enable_llm_translation, llm_model, llm_prompt` (close-persisted, seeded from `settings.recording`; advanced sheet edits mutate live options + forward `update_options` to a running worker). Advanced sheet: replace `translation_block`'s disabled widgets with live ones (checkbox + combo-seeded model + instructions — same ✚ defaults as Task 14; retire the `recording-advanced-llm-tooltip` disabled-tooltip key). Recording window: translation panel below the transcription panel, placeholder `"Waiting for AI translation..."` (ftl key), visible iff enabled (live toggle). Record click with translation enabled: resolve endpoint (`GuiLlmSource`); None → warn-log, proceed untranslated; Some → `translate.spawn` whose `on_result` runs the `TranslationApplier` (+`update_segment_translation(row_id, …)` via a cloned store handle, skip empty) on the callback thread and sends the panel string as `SessionMessage::TranslationPanel(String)` (new variant) through a cloned session sender. `SessionApplier::apply_text` enqueues `(filtered_text, Some(row_id))` right after `append_live_segment`. GUI drain: `TranslationPanel` → `RecordingVm.translation_panel_text`. Presentation window: translation region below the transcript when `translation_panel_text` non-empty and translation enabled (same style resolution). Stop: worker keeps draining (late arrivals land while stopped); window close / next Record → `stop()` (detached) + fresh applier. `on_device_changed` recording-branch guard gets its carried unit test here (this file is in scope).

- [ ] Unit: options close-persist round trip incl. the three new fields; mid-session `update_options` forwarding (scripted control records it); panel-visibility rule; teardown-then-new-session gets a fresh worker; the carried `on_device_changed` guard test.
- [ ] kittest: advanced LLM block is now enabled and edits persist to live options; `live_translation_end_to_end` — scripted spawner echoing `"X:"+text`, scripted session drives 2 utterances → translation panel shows both (mode below, separator), `.translated` TXT bytes exact, store rows carry translations; panel hidden when disabled; presentation content includes the translation region when non-empty (Harness::new_ui_state, existing pattern).
- [ ] Gates; commit `M6: live translation panel + session/export/presentation wiring (F-063)`.

### Task 18: Docs sweep + carried minors

**Files:** `docs/HUMM_SURFACE.md` (§3 no-new-json note, §5 env table finalization, §6 keyring row + settings tables), `docs/design/ARCHITECTURE.md` (SecretStore/AudioEncoder/openai-module as-built notes), `docs/design/UX_SPEC.md` (J-8 "Shipped M6"), `docs/superpowers/specs/2026-07-23-m5-live-recording-design.md` (§3 `LiveEvent` sketch annotation), `crates/humm-core/src/live/mod.rs:28-36` (unused re-exports), `assets/i18n/en/humm.ftl` (`meter-queue` comment block), `docs/analysis/PARITY_MATRIX.md` (F-031/F-041/F-063/F-085 → implemented with method notes; F-002/F-023/F-100 row updates).
- [ ] Make each edit; gates (clippy catches the re-export removal fallout); commit `M6: docs sweep + carried minors (F-031, F-041, F-063, F-085)`.

### Task 19: Acceptance, benches, gui-manual v4, report

**Spec:** §10 whole. In order:
- [ ] **Mic-TCC probe:** `cargo test -p humm-audio -- --ignored` sandbox-lifted with a 120 s timeout; hang → still blocked (kill, record); pass → also run the deferred real-mic spot-check items from `docs/gui-manual/M5.md`.
- [ ] **Real-API run:** discovery per spec §10 (pinned whisper.cpp server scratch-build if it exposes an OpenAI route; else any local OpenAI-compatible server; else `HUMM_OPENAI_TEST_*` cloud key if configured); write the `#[ignore]` contract test either way; record run-or-blocked honestly.
- [ ] **Key hygiene:** run a GUI + CLI session with a known dummy key against the mock; grep `settings.toml`, `humm.sqlite` (via `strings`), `logs.txt` for it — zero hits.
- [ ] **Benches:** same-day M5 baseline — `git worktree add <scratch>/m5-bench e15392c`, run `e2e` there sandbox-lifted, then on M6 tip; compare same-day only (`docs/benchmarks/M5.md` §1.2 protocol); `export`/`decode` spot-run only if their sources changed. Write `docs/benchmarks/M6.md`.
- [ ] **gui-manual v4** (`docs/gui-manual/M6.md`): windowed runs via the established smoke hooks — Preferences key+Test against a local mock, file task with LLM translation end-to-end, viewer Translate live-arrival, live recording with translation panel + `.translated`; carry the pointer-deferred list forward.
- [ ] **Matrix:** flip verified rows only where the stated method actually ran; milestone report (matrix status, bench numbers, deviations with ADR refs, analysis corrections, environment-blocked debt incl. the live-API-transcription deferral ticket); memory update.
- [ ] Final gates; commit `M6: acceptance + benches + gui-manual v4 + milestone report`.

---

## Self-review checklist (done at write time)

Spec coverage: §1→Task 18 matrix note + already-committed inventory fix; §2→1,4; §3→3,5,6; §4→7,8; §5→2; §6→9; §7.1→13; §7.2→14; §7.3→15; §7.4→16,17; §7.5→11; §8→10; §9→1,11,18; §10→19; ledger items land with their owning tasks. Type names cross-checked (Backend, LlmTranslationOptions, OpenAiAuth, TranslatorConfig/Handle/Msg, LlmEndpoint(Source), TranslateSpawner/Control, ValidateOutcome, TranslationApplier). No placeholder steps: every test named, every string sourced, seams defined where kittest needs them.
