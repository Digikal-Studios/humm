# M6 — Cloud: OpenAI-compatible API backend + LLM translation (design spec)

Parity IDs: **F-041** (OpenAI-compatible API transcription backend), **F-031**
(LLM translation during file transcription), **F-063** (live translation
panel), **F-085** (viewer post-hoc Translate). Plus: the `-m openaiapi` /
`--openai-token` CLI arms (F-002 completion), the Preferences OpenAI block
(partial F-100), `SecretStore`/keyring (ADR-012 realized), and the carried
`QueueHandle::finish()` hard-timeout ADR.

Buzz ground truth re-read at spec time (v1.4.5 checkout,
`/Users/jawad/RustroverProjects/buzz`): `buzz/translator.py`,
`buzz/transcriber/openai_whisper_api_file_transcriber.py`,
`buzz/transcriber/transcriber.py:141-158`,
`buzz/widgets/preferences_dialog/general_preferences_widget.py`,
`buzz/widgets/transcriber/advanced_settings_dialog.py:75-121`,
`buzz/widgets/transcriber/transcription_options_group_box.py`,
`buzz/widgets/recording_transcriber_widget.py` (translation panel,
`on_next_transcription`/`on_next_translation`, `_do_close`),
`buzz/widgets/transcription_viewer/transcription_viewer_widget.py:1300-1324,1498`,
`buzz/widgets/transcription_viewer/transcription_segments_editor_widget.py:256-363`,
`buzz/store/keyring_store.py`, `buzz/cli.py:231-244`,
`buzz/widgets/presentation_window.py:138-163`, `buzz/widgets/main_window.py:323-331`.

## 0. Scope boundaries (explicit)

**In scope:** everything in §§2–9 below.

**Out of scope, stated deliberately:**
- **`-e` / `--extract-speech` stays "not yet available" (exit 1).** The
  milestone brief's HUMM_SURFACE §1 reference bundles the three backendless
  CLI arms, but `-e` is Demucs speech extraction (F-027) — an M8 heavy-ML
  feature per ADR-011 and the F-002 matrix row's own "M6/M8" split. M6 makes
  `openaiapi`/`--openai-token` real; `-e` keeps its current actionable error.
- **Live transcription via the API backend** (Buzz's per-chunk temp-WAV arm in
  `recording_transcriber.py:220-231`) is **deferred**. The recording window
  keeps its local-engine-only options group; F-061's verified scope never
  included backend selection. Logged as an M7-candidate ticket in the
  milestone report — not silently dropped. (Live *translation* — F-063 — is
  fully in scope; it is a separate LLM worker, not the transcription engine.)
- **Buzz's Linux XDG-portal secrets fallback** (`keyring_store.py:31-198`) is
  not ported. humm uses the `keyring` crate's own per-platform backends; the
  portal dance is a Buzz/Flatpak workaround with a hand-rolled XOR cipher.
  Documented as deviation (ledger #12).
- **The per-chunk `create_state()`/CoreML-reload live-engine perf ticket**
  stays carried: M6 does not modify `transcribe_live`'s engine path (the
  translation worker is additive), so per the ticket's own condition ("if M6
  touches the engine path") it remains a visible ticket, not silent debt.

## 1. The F-031 analysis correction (load-bearing discovery)

`FEATURE_INVENTORY.md` F-031 claims "each produced segment is queued to a
`Translator`" during transcription. **Buzz v1.4.5's file-transcription
pipeline never does this.** Verified exhaustively at spec time:

- The only two `Translator.enqueue` call sites in Buzz are
  `recording_transcriber_widget.py:970` (live, F-063) and
  `transcription_viewer_widget.py:1324` (viewer Translate button, F-085).
- `file_transcriber_queue_worker.py`, `whisper_file_transcriber.py`,
  `openai_whisper_api_file_transcriber.py`, and `main_window.py` contain no
  reference to the translator or the llm options.
- The file options sheet's "Enable AI translation" checkbox
  (`advanced_settings_dialog.py:80-83`) is persisted
  (`file-transcriber/enable-llm-translation`) but **consumed by nothing on
  the file path** — its only effect is as a sticky prefill for the viewer's
  own advanced dialog. Buzz's restart path even hardcodes it off
  (`transcription_tasks_table_widget.py:762`).
- Buzz's own docs (`docs/docs/usage/3_translations.md`) describe exactly two
  translation flows: live recording, and the viewer's Translate button.

**Resolution:** FEATURE_INVENTORY's F-031 row is corrected in the same commit
that lands this spec (living-docs rule). humm's F-031 ships what the brief
scopes and Buzz's UI *promises but never wired*: with AI translation enabled
in the options sheet, each produced segment is translated during the file
task and stored per segment. This is a ✚ improvement (ledger #1) — the
batching protocol, prompt shapes, and error semantics are still ported
byte-faithfully from `translator.py`, so the *machinery* is parity even
though the file-path trigger is humm-only.

## 2. Core: `humm_core::openai` (pure-Rust service in core)

Per ARCHITECTURE §1, HTTP services live in core (adapter crates are for
non-Rust boundaries). New module `crates/humm-core/src/openai/`:

- `openai::client` — shared plumbing: base-URL normalization (default
  `https://api.openai.com/v1`; user value used verbatim, trailing-`/`
  trimmed), bearer-auth header construction, `reqwest::blocking::Client`
  creation, error mapping (status + body-excerpt → actionable message that
  **never contains the key**), and the run-request-on-its-own-thread cancel
  mechanism (§8/ADR-022).
- `openai::engine` — `OpenAiEngine`, the second `TranscriptionEngine` (§3).
- `openai::translate` — the LLM translator: batch protocol + worker (§4).
- `openai::validate` — the Test-button check (§7).

The API key travels as an in-memory `String` inside these types only:
constructor argument → `Authorization` header. It is never `Debug`-printed
(key-carrying structs get manual `Debug` impls that redact), never logged,
never serialized, never placed in a subprocess argv.

### 2.1 `Backend` on `ModelSpec`

`domain.rs`: `ModelSpec` gains `backend: Backend` with
`#[serde(default)]`:

```rust
#[derive(..., Serialize, Deserialize, Default)]
pub enum Backend { #[default] Whisper, OpenAiApi }
pub struct ModelSpec { pub backend: Backend, pub size: ModelSize, pub hf_repo: Option<String> }
```

`#[serde(default)]` keeps every existing `options_json` row deserializing
(old rows → `Whisper`). The queue's engine cache is already keyed by
`ModelSpec`, so the two backends cache independently for free. The
`ArtifactEnsurer`/model-resolution phase is skipped for `OpenAiApi` (no
local model to download); `EngineFactory::create` receives
`resolved: None` for it and routes on `spec.backend`.

`TranscriptionOptions` gains `#[serde(default)] pub llm: Option<LlmTranslationOptions>`
where `LlmTranslationOptions { pub model: String, pub prompt: String }`
(presence = enabled — ARCHITECTURE §4's sketched shape). **No key field
exists on any serialized type** (Buzz needs `Exclude.ALWAYS` metadata to
keep its token out of JSON; humm makes the leak impossible structurally).

### 2.2 `EngineError` additions

`#[non_exhaustive]` already: add `Api { detail: String }` (HTTP/transport/
response-shape failures, actionable: names the endpoint's host + status,
suggests checking key/base URL) and reuse `Canceled`.

## 3. F-041 — `OpenAiEngine` (file transcription)

Construction (per task, by the binaries' `EngineFactory` impls):
`OpenAiEngine::new(cfg: OpenAiEngineConfig, encoder: Arc<dyn AudioEncoder>)`
with `OpenAiEngineConfig { base_url: Option<String>, api_key: String,
api_model: String }`. `api_model` comes from `settings.openai.api_model`
(default `"whisper-1"`), base URL from `settings.openai.base_url` — the
humm mapping of Buzz's `transcriber/openai-api-model` and
`transcriber/custom-openai-base-url` settings, read at engine-creation time
(Buzz reads them at transcriber construction: `openai_whisper_api_file_transcriber.py:41-53`).

### 3.1 `AudioEncoder` boundary (new, humm-media)

Buzz re-encodes the *original file* to mp3 via a raw ffmpeg call inside the
transcriber (`:65-94`). humm's engine receives decoded `Pcm` (16 kHz mono),
so the boundary is PCM-in:

```rust
/// Encode 16 kHz mono f32 PCM to a compressed audio file. Impl: ffmpeg subprocess.
pub trait AudioEncoder: Send + Sync {
    fn encode_mp3(&self, pcm: &Pcm, dest: &Path, cancel: &CancelToken) -> Result<(), MediaError>;
}
```

Implemented on `FfmpegDecoder` (same binary config, same spawn/cancel
helpers): `-f f32le -ar 16000 -ac 1 -i - <dest>.mp3` with PCM piped to
stdin. Contract tests in `humm-media/tests/contract.rs` (`#[ignore]`):
encode → ffprobe duration matches PCM duration ±100 ms; encode → decode
round-trip sample count within codec tolerance. Sending 16 kHz mono mp3
instead of a re-encode of the original is a mechanism deviation with
identical outcome (Whisper-family APIs downmix/resample to 16 kHz mono
anyway) and a smaller upload (ledger #2).

### 3.2 Transcribe flow (ported from `openai_whisper_api_file_transcriber.py`)

1. Encode the whole `Pcm` to a temp mp3 (temp dir; cleaned up on all paths).
2. `sink.on_progress(0.0)` (Buzz's `self.progress.emit((0, 100))`).
3. Size check: `MAX_CHUNK_SIZE = 25 * 1024 * 1024` bytes (Buzz `:127`).
   - Under: one request; stream its segments to the sink; done.
   - Over: `num_chunks = ceil(total_size / MAX)`; `chunk_duration =
     pcm_duration / num_chunks`; slice the **PCM** into equal-duration
     sample ranges and encode each slice to its own temp mp3 (mechanism
     deviation from Buzz's `-c copy` split of the encoded mp3 — sample-exact
     boundaries instead of mp3-frame-imprecise ones, one fewer subprocess
     mode; ledger #3). Per chunk: request with
     `offset_ms = chunk_index * chunk_duration_ms`, stream remapped
     segments to the sink, delete the chunk file,
     `sink.on_progress((i + 1) as f32 / num_chunks as f32)`.
4. Cancel token checked between every chunk and inside the request wait
   (§8); cancel → `EngineError::Canceled` (Buzz's `stop()` is literally
   `pass` — humm's cooperative cancel is strictly better; ledger #4).

### 3.3 The request (per file/chunk)

`POST {base}/audio/transcriptions` for `TaskKind::Transcribe`,
`POST {base}/audio/translations` for `TaskKind::Translate` (Buzz `:208-215`).
Multipart form (`reqwest` `multipart` feature added, blocking):

- `file` — the mp3 part;
- `model` — `api_model`;
- `response_format` — `"json"` if `api_model` starts with `"gpt-4o"`, else
  `"verbose_json"` (Buzz `:196`);
- `prompt` — `options.initial_prompt`;
- `language` — only for Transcribe and only when `options.language` is
  `Some` (Buzz passes `None` through the SDK, which omits it);
- `timestamp_granularities[]=word` when `options.word_timestamps` (Buzz
  `:205-206`).

Request timeout 600 s (openai-python's default, which Buzz inherits with
`max_retries=0`); no retries.

### 3.4 Response parsing (tolerant, ported fallback chain)

Parse as `serde_json::Value`, mirroring Buzz `:217-290`:

- `segments` := top-level `"segments"` array if present; else if `"text"`
  exists → the gpt-4o single-segment fallback
  `[{text, start: 0, end: 0}]`. (`model_extra` in Buzz is just "keys the
  SDK model didn't declare" — a plain JSON lookup here.)
- `words` := top-level `"words"` if present (word-granularity responses).
- **Without word timestamps:** one `Segment` per element:
  `start = int(seg.start * 1000 + offset_ms)` (f64 seconds → ms,
  truncation-toward-zero like Python `int()`, applied to the whole sum —
  corrected during Task 5 — Buzz truncates the sum), same for `end`; text
  as-is; `translation: ""`.
- **With word timestamps:** detect a whisper.cpp-server-style response —
  first segment has all three keys `tokens`, `avg_logprob`,
  `no_speech_prob` (Buzz `:234-236`):
  - **whisper.cpp branch:** the byte-buffer word-merge ported exactly
    (`append_segment` + the loop at `:238-268`): accumulate word bytes into
    a `Vec<u8>` buffer; a word starting with `b" "` flushes the buffer
    (flush = skip if empty; emit only if the buffer decodes as UTF-8, else
    **keep accumulating** — the multi-byte-character guard, whisper.cpp
    issue #1798); a word starting with `b", "` appends `b","`, flushes,
    then restarts the buffer from the comma-stripped remainder;
    otherwise append + extend `end`. Times are **centiseconds → ms
    (`* 10`)**, then `offset_ms` added — a deliberate deviation from Buzz,
    which never applies `offset_ms` in this branch; see deviation ledger
    item 16 (§11). Final flush per segment.
  - **Standard branch:** one `Segment` per word:
    `int(word.start * 1000 + offset_ms)` etc. (Buzz `:270-279`; truncation
    applies to the whole sum — corrected during Task 5 — Buzz truncates the
    sum).
- Missing/`null` `start`/`end` default to 0 via the `get_value(...,
  default)` semantics (Buzz `:281-288`).

Non-2xx or a body that fits none of the shapes → `EngineError::Api` naming
the status/URL-host and an excerpt (never echoing the request headers).

`EngineCaps` for `OpenAiEngine`: `word_timestamps: true, translate: true,
vad: false`. The pipeline's VAD phase stays local-engine-only (VAD model
resolution is skipped for `Backend::OpenAiApi`, same as model download).

## 4. LLM translator — `openai::translate` (ported from `translator.py`)

### 4.1 Pure protocol functions (unit-tested, differential-fixtured)

- `BATCH_SIZE = 10`.
- `combine_numbered(items) -> String`: `"[1] {text}\n[2] {text}..."`
  (1-based, `\n`-joined; Buzz `:85-88`).
- `batch_system_prompt(user_prompt, n) -> String`: Buzz's exact template
  (`:90-96`): `"{prompt}\n\nYou will receive {n} numbered texts. Process
  each one separately according to the instruction above and return them in
  the exact same numbered format, e.g.:\n[1] processed text\n[2] processed
  text"`.
- `parse_batch_response(response, expected) -> Vec<String>`: ports
  `re.split(r'\[(\d+)\]\s*', response)` semantics exactly (`:130-145`):
  text before the first `[N]` marker is discarded; each marker's number
  maps to the text between it and the next marker, `.strip()`-trimmed
  (trim = Unicode whitespace both ends, Python-parity); duplicate numbers →
  last occurrence wins; missing numbers → `""`; numbers beyond `expected`
  ignored; a response with no markers at all → all `""`. Marker numbers
  that overflow i64 are treated as non-matches the way Python's `int()`
  never fails on `\d+` — cap: parse as `u32`, ignore on overflow (a
  documented micro-deviation; a real LLM emitting a >4-billion marker is
  degenerate input either way). A second micro-deviation, documented during
  Task 7 rather than pre-approved here: non-ASCII Unicode decimal digits in
  markers (e.g. Arabic-Indic `٣`) are not honored, though Python's `int()`
  would parse them — an implementation decision, not spec-approved like the
  overflow cap, and regression-tested rather than differential-fixtured
  (asserting Rust/Python disagreement isn't what a parity fixture is for).

**Differential fixtures:** `validation/m6-differential/gen_batch_parse_fixtures.py`
stub-imports Buzz's own unmodified `Translator._parse_batch_response`
(anaconda python3.9; PyQt6/openai/keyring stubbed via `sys.modules`, the
established M4/M5 technique) and runs it over a fixture matrix — well-formed,
missing numbers, out-of-order, duplicates, prose preamble, trailing garbage,
no markers, `[N]` with varying whitespace, multi-line entries, CJK text —
committing JSON fixtures the Rust tests assert byte-for-byte. This is the
acceptance item "translation batching parses Buzz-style responses including
degraded/malformed ones" made differential rather than assumed.

### 4.2 The worker

`TranslatorHandle::spawn(cfg: TranslatorConfig, on_result: impl Fn(String, Option<i64>) + Send)`
→ a dedicated thread owning a `flume` receiver of:

```rust
enum TranslatorMsg { Item { text: String, id: Option<i64> },
                     UpdateOptions { model: String, prompt: String },
                     Stop }
```

Loop semantics ported from `Translator.start` (`:147-186`): block on the
first item; drain up to `BATCH_SIZE - 1` more without blocking; a `Stop`
drained mid-batch is remembered and honored **after** the batch is
processed; batch of 1 → the single-request form (system = prompt, user =
text, Buzz `:59-80`); batch of N → the numbered form; every item produces
exactly one `on_result(translation, id)` call, `""` on any error (per-item
and per-batch error semantics per Buzz: request exception or missing
choices → all-`""` for the batch, logged). `UpdateOptions` swaps
model/prompt for subsequent requests (Buzz's
`on_transcription_options_changed` signal — the live advanced sheet edits
mid-session). Requests: `POST {base}/chat/completions`, JSON body
`{model, messages: [{role: system}, {role: user}]}`, timeout **60 s**, no
retries (Buzz `:62-69`). Dropping the sender without `Stop` also ends the
worker (channel-disconnect ≡ sentinel).

### 4.3 Endpoint/key resolution (env parity)

`TranslatorConfig { base_url: Option<String>, api_key: String, model, prompt }`.
Resolution helper `openai::translate::resolve_endpoint(env, settings, secrets)`:

- base URL: **`HUMM_TRANSLATION_API_BASE_URL`** env if set, else
  `settings.openai.base_url`, else the OpenAI default. Buzz's variable is
  `BUZZ_TRANSLATION_API_BASE_URl` — **the trailing lowercase-`l` typo is
  upstream's** (`translator.py:44`). humm fixes the case since the
  `BUZZ_`→`HUMM_` rename already breaks env-name compatibility; the typo is
  documented in HUMM_SURFACE §5 so migrating users grepping either spelling
  find the mapping (ledger #5).
- key: **`HUMM_TRANSLATION_API_KEY`** env if set, else the keyring key via
  `SecretStore` (Buzz `:49-52`).

## 5. Secrets — `SecretStore` + keyring (ADR-012 realized)

Core trait (`crates/humm-core/src/secrets.rs`):

```rust
pub enum SecretKey { OpenAiApiKey }   // entry name: "OpenAI API key" (Buzz parity)
pub trait SecretStore: Send + Sync {
    fn get(&self, key: SecretKey) -> Result<Option<String>, SecretError>;
    fn set(&self, key: SecretKey, value: &str) -> Result<(), SecretError>;
    fn delete(&self, key: SecretKey) -> Result<(), SecretError>;
}
```

Impl `humm_store::KeyringSecretStore` via the `keyring` crate (version pinned
at implementation time against crates.io; ADR-012 anticipated 4.x), service
string `"humm"` (humm's own service; entry-name parity only — Buzz uses
service `"Buzz"`, so humm never reads Buzz's stored key: a fresh setup is
the documented, deliberate behavior, consistent with Q-6's "no QSettings
migration"). A `MemorySecretStore` test double lives in core
(`#[cfg(test)]`-adjacent, available to GUI/CLI tests) so no test touches the
OS keychain. One `#[ignore]` contract test in humm-store does a real
set/get/delete round-trip (run sandbox-lifted; may surface a first-use
keychain prompt — documented as a possible environment blocker like mic
TCC). Read failures degrade gracefully: `get` errors are logged and treated
as "no key" at call sites that can proceed (Buzz maps read errors to `""`),
while `set` failures surface Buzz's error dialog text ("Unable to save
OpenAI API key to keyring", `main_window.py:330`).

**The acceptance invariant** — keys appear in: OS keyring, process memory,
`Authorization` headers, and (CLI-only, user-initiated) the `--openai-token`
argv. They never appear in `settings.toml`, the DB (`options_json` has no
key field), logs (redacted `Debug`), or any subprocess argv. A grep-based
check over settings/DB/logs after the acceptance run proves it.

## 6. F-031 — per-task translation during file transcription

`run_file_job` changes (core, `pipeline.rs`):

- New optional dep threaded through `RunnerDeps` → `run_file_job`:
  `llm: Option<Arc<dyn LlmEndpointSource>>` where
  `trait LlmEndpointSource: Send + Sync { fn resolve(&self) -> Option<LlmEndpoint> }`
  (`LlmEndpoint { base_url: Option<String>, api_key: String }`). The GUI
  impl resolves env → settings → keyring at task start (Buzz reads config
  at translator construction); tests inject fixed endpoints pointed at the
  mock server. CLI passes `None` in M6 (no CLI flag enables LLM translation
  — Buzz's CLI has none either).
- When `job.options.llm` is `Some(llm_opts)` **and** the endpoint resolves
  (key present): spawn a `TranslatorHandle` for the task; the pipeline's
  `TeeSink` additionally enqueues each trimmed segment text with its
  **ordinal index** as the id. After `engine.transcribe` returns `Ok`,
  send `Stop`, then drain results (the worker's own per-request 60 s
  timeout bounds each batch) into `segments[idx].translation`; then run
  exports and return. The task's terminal `mark_completed(id, segments)`
  persists translations with zero store changes (the segment INSERT already
  writes the `translation` column). Completed means everything — including
  translations — is done (ledger #6 records the wait-for-drain decision and
  why: translator lifetime stays inside the task, the viewer sees complete
  data, cancel has one owner).
- Cancel during drain: cancel token → `Stop` + abandon the worker (detach);
  task → `Canceled` as usual.
- Endpoint unresolvable (no key anywhere) or worker spawn failure: log
  `warn`, run the task **without** translation (translation is enrichment,
  not a gate — matches Buzz's own error→`""` degradation where every
  request would fail anyway, minus the useless network attempts).
- Per-batch API failures produce `""` translations (parity); the task still
  completes. Progress display is unaffected (translation drain happens
  after `on_progress(1.0)`-equivalent segment flow; the status stays
  In Progress until exports finish, as today).

CLI `--json` gains no new event in M6 (no CLI path can enable LLM
translation; documented in HUMM_SURFACE §3's stability note).

## 7. GUI surfaces

### 7.1 Preferences → General tab (partial F-100)

New `PrefsTab::General` (listed first), containing only the OpenAI block —
the rest of F-100 (UI language, font size, export template, live-export
relocation, GPU toggles) stays M7:

- **OpenAI API key** — password-masked `TextEdit` with a show/hide eye
  toggle (Buzz `OpenAIAPIKeyLineEdit`), seeded from `SecretStore` at tab
  open, placeholder `sk-...`. Persisted to the keyring on edit-commit
  (focus-out/Enter) rather than per keystroke (ledger #7 — Buzz writes the
  keychain on every `textChanged`; same final state, far fewer keychain
  writes). On commit with a value failing Buzz's charset check
  `^[A-Za-z0-9+/=_-]*$`, show Buzz's exact warning ("Invalid API key" /
  "API supports only base64 characters (A-Za-z0-9+/=_-). Other characters
  in API key may cause errors.", `general_preferences_widget.py:261-267`) —
  warn-only, still saved (Buzz saves too). Keyring write failure → Buzz's
  "Unable to save OpenAI API key to keyring" error modal.
- **Test button** — enabled iff the key field is non-empty (Buzz `:230-231`).
  Click → background thread (the `UpdateNotice` channel pattern):
  1. If a custom base URL is set: raw `GET {base}/models` with bearer
     header, 5 s timeout; non-200 → failure modal with Buzz's exact
     "OpenAI API returned invalid response. Please check the API url or
     your key. Transcription and translation may still work if the API
     does not support key validation." text; transport error → the error's
     own string (Buzz `:354-374`).
  2. Then always: `models.list` equivalent — `GET {base-or-default}/models`,
     bearer, 15 s timeout (`openai::validate`). 2xx → success modal with
     Buzz's text ("Your API key is valid. Buzz will use this key to perform
     Whisper API transcriptions and AI translations." with Buzz→humm
     product-name substitution); auth failure → the response's
     `error.message` when parseable, else status text. Buzz's uncaught
     non-auth exception (which kills its runnable silently,
     `:376-385`) is **not** ported — every failure surfaces in the dialog
     (ledger #8).
- **OpenAI base url** — text field, placeholder `https://api.openai.com/v1`,
  saved to `settings.openai.base_url` on change (empty → `None`).
- **OpenAI API model** — text field, placeholder + default `whisper-1`,
  saved to `settings.openai.api_model`.

Matrix: F-100 → `in-progress` (OpenAI block only, M6).

### 7.2 Options sheet (F-023 extension)

- **Model row** becomes a two-part selection: a backend combo
  (`Whisper` / `OpenAI Whisper API` — humm's two shipped backends,
  ADR-008) above the existing size combo. `OpenAI Whisper API` hides the
  size combo (+ Apple-Silicon load-time tooltip) and shows an
  **Api Key:** row (Buzz `transcription_options_group_box.py:126,242-244`):
  a masked, eye-toggleable field seeded from the keyring, writing **to the
  keyring** on edit-commit — one storage, shared with Preferences (ledger
  #9: Buzz's inline field is per-task-ephemeral and silently lost on
  restart; humm's writes through, preserving the capability with one source
  of truth and keeping keys out of `options_json` structurally).
  `build_specs` maps the combo to `ModelSpec.backend`; task/language/word-
  timestamps/prompt rows are backend-independent (the API engine honors all
  of them, §3.3). Sticky-persisted like every other sheet field
  (`file_transcriber.backend` wire string).
- **Advanced section** gains the **Translation settings** block (Buzz
  `advanced_settings_dialog.py:75-121`, shown for both file and recording
  variants): "Enable AI translation" checkbox; **AI model:** — a combo-box-
  seeded free-text field (✚ UX_SPEC J-8: presets `gpt-4o-mini`, `gpt-4o`,
  `gpt-4.1-mini`, plus free entry; Buzz shows the literal placeholder text
  `add-model-id-here`); **Instructions for AI:** — multiline, default
  seeded with the neutral template
  `"You are a professional translator. Translate each text sent to you to
  {language}. Do not add any comments or notes — reply with the translation
  only."` (✚ J-8 — not Buzz's English→Spanish sample; the `{language}`
  placeholder is literal text the user edits). Unlike Buzz — whose dialog
  *displays* defaults but keeps the options empty until the field is
  touched, so an untouched dialog still fails the viewer's configured-gate
  — humm treats the seeded defaults as the real values once the sheet
  saves (ledger #10). Fields disabled while the checkbox is off (Buzz
  behavior). Persisted sticky as
  `file_transcriber.{enable_llm_translation, llm_model, llm_prompt}`;
  `build_specs` maps checked+non-empty-model → `options.llm`.
  No info-button/docs link (Buzz's links to Buzz's own docs site).

### 7.3 F-085 — viewer Translate

- The M4 disabled Translate button (`views/viewer.rs:303`) becomes live.
  Click (`ViewerVm::on_translate_clicked`, gates ported from
  `transcription_viewer_widget.py:1300-1324`):
  1. No key in the keyring → modal: title "API Key Required", body
     "Please enter OpenAI API Key in preferences" (Buzz-exact strings).
  2. `llm_model` or `llm_prompt` unconfigured (empty after the sticky
     `file_transcriber` settings + seeded defaults) → open the viewer's
     **AI translation settings** modal (model combo-seeded + instructions,
     same widgets as §7.2's block, writing the sticky settings); on OK →
     run if now configured (Buzz's dialog-accept → `run_translation`).
  3. Run: enqueue every row (`SegmentRow.id`, text) into a lazily-created
     per-viewer `TranslatorHandle` (created on first Translate click —
     ledger #11; Buzz constructs a translator thread for every viewer at
     open, even when never used). Results arrive over an `mpsc` channel
     harvested each frame (the `player_rx` pattern): non-empty translation
     → `rows[idx].translation = t`, `has_translations = true` (unhides the
     Translation column, view-mode entry, and export entries live — Buzz's
     `update_translation`/`on_translation_available`), and
     `store.update_segment_translation(id, &t)` (the existing
     `apply_text_edit` write path). Empty (error) results are skipped —
     with humm's non-blank `has_translations` semantics an all-error run
     leaves the column hidden instead of Buzz's shown-but-empty column
     (ledger #13); a partial run converges to Buzz's outcome.
  4. Translate re-click while running: enqueues again (Buzz allows it;
     the queue serializes).
- Viewer close mid-run: `Stop` + drop (Buzz `:1498`'s `translator.stop()`,
  non-blocking).

### 7.4 F-063 — live translation

**Settings/options:** `RecordingSettings` gains `enable_llm_translation:
bool` (default false), `llm_model: Option<String>`, `llm_prompt:
Option<String>`. All three live on `RecordingOptions` and are persisted on
window close (Buzz `_do_close` saves exactly these three among the
close-persisted group, `recording_transcriber_widget.py:1234-1245`) — the
same split M5 established.

**Advanced sheet:** the disabled LLM block
(`views/recording_advanced.rs:345-375`) comes alive: checkbox + combo-seeded
AI-model field + instructions field (§7.2's ✚ defaults), enabled-gating
per Buzz (`on_enable_llm_translation_changed`), mutating the live
`RecordingOptions`; mid-session edits are forwarded to a running translator
as `UpdateOptions` (Buzz's options-changed signal into the translator).

**Panel:** a second read-only panel under the transcription panel,
placeholder **"Waiting for AI translation..."**, visible iff
`enable_llm_translation` (live show/hide on toggle — Buzz `:192-193,232-233,544-547`).

**Wiring:** on Record with translation enabled, resolve the endpoint
(env → settings → keyring, §4.3); unresolvable → record proceeds without
translation + a log warning (Buzz would construct a client with an empty
key and fail every request). `SessionApplier::apply_text` enqueues the raw
filtered utterance (Buzz enqueues before mode dispatch,
`on_next_transcription:969-970`) tagged with the persisted segment row id —
`append_live_segment` changes to return `Result<i64, StoreError>`
(`last_insert_rowid`), threaded through the `LiveStore` trait.

**Consumer:** a `TranslationApplier` (core, `live::translation`) mirroring
`SessionApplier`, running on the translator's callback thread — never the
UI thread (M5's hard rule; Buzz does these writes on the GUI thread — not
ported):
- skip empty results (Buzz `on_next_translation:1038-1039`);
- persist `update_segment_translation(row_id, text)` (✚ Q-10 extension —
  Buzz persists nothing live; ledger #14);
- display dispatch by mode into its own `PanelText`/`MergeState`
  (append-below/above insert `strip_newlines(text)` = `\r\n`/`\n` →
  `os.linesep` + separator; append-and-correct feeds
  `process_transcription_merge` over a translations list — Buzz
  `:1041-1081`), the resulting panel string sent to the GUI over the
  session-message channel (`SessionMessage::TranslationPanel(String)`);
- export the **raw** text to the `.translated` sibling via `ExportSink`'s
  reserved `translated_path` (same TXT append/prepend/rewrite + CSV rules
  as the transcript side, `export_max_entries` honored — Buzz `:1048-1078`);
  no upload for translations (Buzz has none).

**Presentation window:** gains a translation region below the transcript
text (shown once a translation exists and translation is enabled — Buzz's
`translation_display`, `presentation_window.py:39-45,138-163`), painted
from the translation panel text with the same theme/size/colors.

**Lifecycle:** Stop does not kill the translator — queued items keep
processing and late translations still land in the panel/store/export
(Buzz's sentinel-after-queue semantics); window close sends `Stop` and
drops without blocking the UI (Buzz `_do_close:1213-1218`). A new Record
click tears down the old worker first (fresh applier state per session,
like `MergeState`).

### 7.5 CLI arms (F-002 completion)

`-m openaiapi` accepted: the `validate_add_args` rejection is removed.
Token resolution (Buzz `cli.py:231-235`): `--openai-token` value, else
`SecretStore` keyring read; both empty → error **"No OpenAI access token
found"**, exit 1. The CLI engine factory routes `Backend::OpenAiApi` to
`OpenAiEngine` (base URL/api-model from settings; encoder =
`FfmpegDecoder`), skipping model resolution/download and VAD for that
backend. `--openai-token` with a local `-m` stays accepted-and-ignored.
`--hide-gui`, exports, `--json`, exit codes: unchanged. cli-int tests cover
the new arms against the in-core mock (no network).

## 8. ADR-022 — cancellation & `finish()` under a network engine

Recorded as a full ADR in DECISIONS.md; decision summary:

- **`finish()` keeps its unbounded full-drain semantics** — it is the CLI's
  main execution mechanism (enqueue N, drain all); a timeout there would
  truncate legitimate work.
- **Prompt cancel becomes a property of the API client, not the queue:**
  every blocking HTTP call in `openai::*` runs on a short-lived request
  thread; the calling thread waits on a result channel with a periodic
  (~100 ms) cancel-token poll. Cancel → return `Canceled` immediately and
  **detach** the request thread (its result is dropped; the thread dies at
  its own request timeout at the latest). This gives <1 s cancel ack
  (UX_SPEC §4) even mid-request — something Buzz never had (its API
  transcriber's `stop()` is `pass`).
- **Belt-and-braces for app exit:** `QueueHandle::finish_within(timeout)` —
  used by GUI shutdown after `cancel_all()` (and by the CLI's
  second-SIGINT path) [as-built: GUI shutdown only; the CLI's second-SIGINT
  remains immediate exit(130) — ADR-022] — joins with a hard deadline; on
  expiry it logs, leaks the runner thread deliberately, and returns, so no
  engine bug can ever wedge process exit. Default GUI shutdown timeout: 5 s.

## 9. Settings, env, surface-doc updates

- `Settings` gains `openai: OpenAiSettings { base_url: Option<String>,
  api_model: String /* default "whisper-1" */ }`;
  `FileTranscriberSettings` gains `backend: Option<String>`,
  `enable_llm_translation: bool`, `llm_model: Option<String>`,
  `llm_prompt: Option<String>`; `RecordingSettings` per §7.4.
- HUMM_SURFACE: §1 flag table (`-m openaiapi` live; `--openai-token`
  documented; `-e` still exit-1), §5 env table adds
  `HUMM_TRANSLATION_API_BASE_URL` / `HUMM_TRANSLATION_API_KEY` (with the
  upstream `BUZZ_TRANSLATION_API_BASE_URl` typo note), §6 adds the keyring
  location row (service `humm`, entry "OpenAI API key"), settings tables
  extended.
- FEATURE_INVENTORY F-031 corrected (§1). ARCHITECTURE §2 annotated
  as-built (SecretStore realized, `AudioEncoder` added, `engines::openai`
  → `openai` module naming). UX_SPEC J-8 gains its "Shipped M6" paragraph.
- i18n: all new user-facing strings land in `en/humm.ftl` with stable keys;
  Buzz-byte-exact strings noted per key.

## 10. Testing & acceptance

- **Unit:** batch protocol (+ differential fixtures §4.1), response parsing
  (all branches incl. whisper.cpp word-merge, gpt-4o, malformed), chunk
  math (Buzz-formula cases around the 25 MiB boundary), engine option
  mapping, settings defaults/round-trips, endpoint resolution precedence
  (env > settings > keyring via `MemorySecretStore`), F-031 merge-by-index
  incl. failure/cancel arms, `TranslationApplier` mode matrix, viewer
  gates, `finish_within` expiry.
- **Mock-server tests** (TestServer extended: per-method canned responses +
  recorded multipart bodies): engine single-file + chunked + translations-
  endpoint + word-granularity + auth-failure; translator single/batch/
  degraded; validate-check both arms. These bind TCP → they join the
  sandbox-blocked set **with documented cause** (the 20-test baseline grows
  by exactly the enumerated new tests; count recorded in the milestone
  report).
- **Contract:** keyring round-trip (`#[ignore]`, sandbox-lifted);
  `encode_mp3` ffmpeg contract tests; **one recorded real-API run** — an
  `#[ignore]` test gated on `HUMM_OPENAI_TEST_BASE`/`HUMM_OPENAI_TEST_KEY`
  envs, with execution-time target discovery in this order: a locally
  built pinned whisper.cpp server if it exposes an OpenAI-compatible
  route (the M5 F-045 scratch-build technique), any locally installed
  OpenAI-compatible server, else a real cloud key if the user has one
  configured; if none is reachable, the test + harness are committed
  runnable and the gap is recorded as environment-blocked in the report
  (the honest F-045 precedent).
- **kittest:** options-sheet backend/LLM rows, Preferences General tab
  (key field + Test via scripted seam), viewer Translate end-to-end against
  a scripted translator (rows update live, column unhides, store rows
  written), recording advanced LLM block live, translation panel
  visibility + scripted live translation e2e (panel + `.translated` bytes +
  store row), presentation translation region.
- **gui-manual v4** (`docs/gui-manual/M6.md`): windowed checklist —
  Preferences key entry + Test against a local mock, an end-to-end
  file-task translation, viewer Translate live-arrival, live recording
  with translation panel + `.translated` export, plus the standing
  pointer-deferred list carried forward. Mic-TCC probe: attempt
  `cargo test -p humm-audio -- --ignored` early; if the TCC hang persists,
  the deferred capture items stay visibly blocked in the report.
- **Bench:** re-baseline against a **same-day M5 checkout** (`e15392c` in a
  scratch worktree) per `docs/benchmarks/M5.md` §1.2 — never against
  recorded numbers. `e2e` is the blocking gate; `export`/`decode` spot-run
  only if their source paths change (they shouldn't).
- **Key-hygiene check:** after the acceptance runs, grep `settings.toml`,
  the DB, and `logs.txt` for the test key material — zero hits required.

## 11. Deviations ledger (M6)

1. **F-031 file-path translation is humm-only** — Buzz's checkbox is dead on
   the file path (§1); humm wires it. Machinery (batching/prompts/errors)
   is ported byte-faithfully.
2. Engine uploads 16 kHz mono mp3 encoded from decoded PCM, not a re-encode
   of the original media (§3.1) — identical model-visible content.
3. >25 MiB chunking slices PCM sample-exactly instead of `-c copy` mp3
   splitting (§3.2) — same chunk-count/duration math.
4. API-path cancel is cooperative and prompt; Buzz's `stop()` is a no-op.
5. `HUMM_TRANSLATION_API_BASE_URL` fixes Buzz's `..._BASE_URl` typo (§4.3).
6. F-031 tasks wait for the translation drain before Completed (§6).
7. Preferences key persists on edit-commit, not per keystroke (§7.1).
8. Test-button failures always surface; Buzz's non-auth exceptions killed
   its worker silently (§7.1).
9. The options sheet's Api Key field writes through to the keyring; Buzz's
   was per-task-ephemeral (§7.2).
10. Seeded AI-model/prompt defaults are real values on save; Buzz's are
    display-only until touched (§7.2). Plus the two J-8 ✚s: combo-seeded
    model field, neutral default prompt.
11. Viewer translator is created lazily on first use; Buzz spawns one per
    viewer at open (§7.3).
12. Linux XDG-portal secrets fallback not ported (§0).
13. Empty (errored) translations are skipped, not written; an all-error run
    leaves the Translation column hidden rather than shown-empty (§7.3).
14. Live translations persist onto the session's segment rows (Q-10 ✚);
    Buzz persists no live data at all (§7.4). Translation-side export/store
    writes run off the UI thread (M5 rule; Buzz does them on the GUI
    thread).
15. No third-party-docs info button in the LLM block (§7.2).
16. The whisper.cpp word-merge branch applies the chunk `offset_ms`; Buzz's
    never does (`openai_whisper_api_file_transcriber.py:250-268`), silently
    corrupting the timeline for >25 MiB chunked word-timestamped runs
    against whisper.cpp-style servers — an accidental defect not ported
    (house precedent: F-026 delete-gate, ADR-016).

## 12. Carried tickets disposition

- `QueueHandle::finish()` hard-timeout ADR → **done this milestone** (§8).
- Per-chunk CoreML-reload live perf → **carried** (§0, condition not met).
- Mic-TCC capture contract tests / real-mic spot-check → probe at
  execution; run if unblocked, else remain visibly deferred (§10).
- Carry-grade minors in files M6 touches: unused pub re-exports in
  `humm-core/src/live/mod.rs:28-36` (touched by `live::translation`);
  `meter-queue` ftl key comment-block placement (ftl touched); the
  `on_device_changed` recording-branch guard test (recording.rs touched);
  M5 spec §3 `LiveEvent` sketch annotation (docs sweep). The
  `gen_merge_fixtures.py:162` dead alias is out of M6's touched set —
  carried.
