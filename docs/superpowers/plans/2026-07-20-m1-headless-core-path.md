# M1 — A file becomes a transcript (headless core path) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `humm add audio.mp3 --srt --txt -s small` transcribes a local media file with whisper-rs (Metal+CoreML) and writes Buzz-byte-format-identical TXT/SRT/VTT exports — plus the ADR-002 whisper-rs-vs-candle validation benchmark, contract tests for every adapter introduced, ported golden snapshots, and a criterion suite.

**Architecture:** Cargo workspace per `docs/design/ARCHITECTURE.md`: `humm-core` (domain types, boundary traits, export writers, model registry/downloader, pipeline — zero UI deps, no unsafe, `#![warn(missing_docs)]`), `humm-media` (ffmpeg/ffprobe subprocess adapter), `humm-whisper` (whisper-rs FFI adapter — the only crate allowed `unsafe`), `humm-cli` (clap binary `humm`). Binaries → adapters → core; core imports no adapter.

**Tech Stack:** Rust edition 2024 · whisper-rs 0.16 (metal+coreml on macOS) · clap 4 derive · thiserror 2 / anyhow 1 · chrono 0.4 · reqwest (blocking, rustls) · sha2 · zip · directories · insta · criterion · assert_cmd.

## Global Constraints

Copied from `CLAUDE.md`, `docs/MISSION.md`, and the ADRs — every task's requirements implicitly include all of these.

1. **Gates — every commit must pass** (run all three before each `git commit`):
   ```bash
   export CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home"
   cargo fmt --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
2. **Sandbox:** ALL `cargo` invocations must run with `CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home"` (the default `~/.cargo` is not writable in this environment; the redirected home is gitignored). If a cargo command fails with "Operation not permitted" on `~/.cargo`, you forgot this.
3. **Ground truth:** the Buzz source at `/Users/jawad/RustroverProjects/buzz` is the sole authority on behavior. Each task cites the exact Buzz files; **read them before implementing** — implement from source, not from this plan's paraphrase. If source contradicts this plan, the source wins; report the discrepancy in your report. Never invent behavior; genuine ambiguity goes to `docs/OPEN_QUESTIONS.md`.
4. **Buzz testdata** for contract tests: env `HUMM_BUZZ_TESTDATA`, default `/Users/jawad/RustroverProjects/buzz/testdata` (contains `whisper-french.mp3`, `whisper-latvian.wav`, `audio-long.mp3`, `ggml-tiny.bin`).
5. **Errors:** `thiserror` enums in library crates, `anyhow` only in `humm-cli`. No `unwrap`/`expect` outside `#[cfg(test)]` and documented-impossible states. Every error message actionable ("X missing — do Y").
6. **`humm-core`:** `#![warn(missing_docs)]`, `#![forbid(unsafe_code)]`, zero UI/adapter deps. `unsafe` permitted only in `humm-whisper`, and only if unavoidable, with a `// SAFETY:` invariant comment.
7. **Style:** newtypes (`Millis`), iterators over index loops, exhaustive `match` on domain enums, no `pub` fields without reason. Doc comments on all public core items.
8. **Tests:** unit tests inline in the module; tests needing external binaries, models, or network are `#[ignore]` with a doc comment naming the prerequisite. No network in non-ignored tests.
9. **Env var surface (humm-owned names):** `HUMM_MODEL_ROOT`, `HUMM_FFMPEG`, `HUMM_FFPROBE`, `HUMM_LOG_DIR`, `HUMM_FORCE_CPU`, `HUMM_WHISPER_N_THREADS`, `HUMM_PARAGRAPH_SPLIT_TIME`, `HUMM_BUZZ_TESTDATA`. Binaries read env; core functions take values as parameters. Exceptions (encapsulated env reads, unit-tested via parameterized inner functions): the `paths` helpers (`HUMM_MODEL_ROOT`, `HUMM_LOG_DIR`) and adapter binary-path constructors (`FfmpegDecoder::new` / `FfprobeProber::new` read `HUMM_FFMPEG`/`HUMM_FFPROBE`).
10. **Dependencies are a liability:** only the crates named in a task may be added. Any extra dependency needs a one-line justification in your report. Versions: whisper-rs `0.16`, clap `4` (derive), thiserror `2`, anyhow `1`, chrono `0.4` (default-features = false, features `["clock", "std"]`), serde `1` + serde_json `1`, directories `6`, reqwest `0.12` (default-features = false, features `["blocking", "rustls-tls"]`), sha2 `0.10`, zip (latest stable, default-features = false, features `["deflate"]`), tempfile `3`, insta `1`, pretty_assertions `1`, criterion (latest stable, `harness = false`), assert_cmd `2`, predicates `3`.
11. **Commits:** at coherent boundaries, message format `M1: <what> (F-###[, F-###])`, ending with the Claude co-author line per harness rules.
12. **Timestamps are integer milliseconds** everywhere (`Millis(i64)`); whisper.cpp reports centiseconds — convert at the adapter edge (`ms = cs * 10`).
13. Docs are living: if implementation reveals an error in `docs/analysis/*` or `docs/design/*`, fix the doc in the same commit and say so in your report.

---

### Task 1: Workspace scaffolding + quality gates

**Files:**
- Create: `Cargo.toml` (workspace root), `crates/humm-core/Cargo.toml`, `crates/humm-core/src/lib.rs`, `crates/humm-media/Cargo.toml`, `crates/humm-media/src/lib.rs`, `crates/humm-whisper/Cargo.toml`, `crates/humm-whisper/src/lib.rs`, `crates/humm-cli/Cargo.toml`, `crates/humm-cli/src/main.rs`, `.github/workflows/ci.yml`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: a compiling 4-crate workspace all later tasks build inside. Crate names exactly `humm-core`, `humm-media`, `humm-whisper`, `humm-cli`; the CLI binary is named `humm`.

**Steps:**

- [ ] **Step 1: Workspace root `Cargo.toml`.** Members `crates/humm-core`, `crates/humm-media`, `crates/humm-whisper`, `crates/humm-cli`; `resolver = "3"`. A `[workspace.package]` block with `edition = "2024"`, `license = "MIT"`, `repository = "https://repo.digikal.com/jawad/humm"`. A `[workspace.dependencies]` table declaring the versions from Global Constraint 10 (only the ones used by M1 crates; each crate opts in with `dep.workspace = true`). `[profile.release]` with `lto = "thin"`, `codegen-units = 1`. Do NOT add `humm-audio`/`humm-store`/`humm-gui` yet — they join the workspace in M2/M3/M5 (deliberate YAGNI deviation from the ARCHITECTURE §1 listing; the layout doc describes the end state).
- [ ] **Step 2: Crate skeletons.**
  - `humm-core/src/lib.rs`: crate-level doc comment (`//! humm domain library …`), `#![warn(missing_docs)]`, `#![forbid(unsafe_code)]`. Library crates get `[lints.rust] missing_docs = "warn"`? No — keep lint attributes in source (visible), no `[lints]` tables.
  - `humm-media/src/lib.rs`, `humm-whisper/src/lib.rs`: doc comment + `#![forbid(unsafe_code)]` for humm-media; humm-whisper gets a doc comment noting it is the sole crate permitted `unsafe` (no forbid).
  - `humm-media` and `humm-whisper` depend on `humm-core` (path + workspace version).
  - `humm-cli/Cargo.toml`: `[[bin]] name = "humm"`, `path = "src/main.rs"`; depends on all three libs. `main.rs`: `fn main() { println!("humm"); }` placeholder (replaced in Task 8).
- [ ] **Step 3: `.gitignore`:** append `/target`, `/.cargo-home`, `/.superpowers/`.
- [ ] **Step 4: CI workflow** `.github/workflows/ci.yml` (Gitea-Actions-compatible GH syntax): one job on `macos-latest` running the three gate commands (plain `cargo`, no CARGO_HOME override — that is a local-sandbox concern only). Note in a YAML comment that contract tests are `#[ignore]` and not run in CI.
- [ ] **Step 5: Run the gates** (Global Constraint 1). Expected: all pass trivially (`cargo test` reports 0 tests).
- [ ] **Step 6: Commit** `M1: workspace scaffolding + quality gates`.

---

### Task 2: humm-core — domain model, errors, cancellation, boundary traits, paths

**Files:**
- Create: `crates/humm-core/src/domain.rs`, `crates/humm-core/src/language.rs`, `crates/humm-core/src/cancel.rs`, `crates/humm-core/src/error.rs`, `crates/humm-core/src/boundary.rs`, `crates/humm-core/src/paths.rs`
- Modify: `crates/humm-core/src/lib.rs` (`pub mod` for each), `crates/humm-core/Cargo.toml` (deps: thiserror, directories; dev-deps: pretty_assertions, tempfile)

**Buzz sources to read first:** `buzz/transcriber/transcriber.py` (lines 15–225: `DEFAULT_WHISPER_TEMPERATURE`, `Task`, `Segment`, `LANGUAGES`, `TranscriptionOptions`, `OutputFormat`), `buzz/model_loader.py` lines 128–155 (`WhisperModelSize`, ggml size mapping). Design contract: `docs/design/ARCHITECTURE.md` §2 (traits) and §4 (data model).

**Interfaces (Produces — later tasks use these exact names):**
```rust
// domain.rs
pub struct Millis(pub i64);            // Copy+Clone+Debug+PartialEq+Eq+PartialOrd+Ord+Hash, impl Add/Sub -> Millis
pub struct Segment { pub start: Millis, pub end: Millis, pub text: String, pub translation: String }
pub enum TaskKind { Transcribe, Translate }        // as_str() -> "transcribe" | "translate"; FromStr accepting exactly those
pub enum OutputFormat { Txt, Srt, Vtt }            // as_str()/extension() -> "txt" | "srt" | "vtt"; Ord (Txt<Srt<Vtt)
pub enum ModelSize { Tiny, TinyEn, Base, BaseEn, Small, SmallEn, Medium, MediumEn,
                     Large, LargeV2, LargeV3, LargeV3Turbo, Custom, Lumii }
//   as_str(): "tiny","tiny.en","base","base.en","small","small.en","medium","medium.en",
//             "large","large-v2","large-v3","large-v3-turbo","custom","lumii"  (FromStr = exact inverse)
//   ggml_name(): Large => "large-v1", else as_str()      [model_loader.py:149-152]
pub struct ModelSpec { pub size: ModelSize }       // backend/custom fields arrive in M2/M6 (ADR-008)
pub struct TranscriptionOptions { pub language: Option<Language>, pub task: TaskKind,
    pub model: ModelSpec, pub word_timestamps: bool, pub initial_prompt: String,
    pub temperature: Vec<f32> }                    // Default: None/Transcribe/small/false/""/DEFAULT_TEMPERATURE
pub const DEFAULT_TEMPERATURE: [f32; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];   // transcriber.py:15
pub struct OutputSpec { pub formats: std::collections::BTreeSet<OutputFormat>,
    pub directory: Option<std::path::PathBuf>, pub file_name_template: Option<String>,
    pub paragraph_split: Millis }                  // Default: empty/None/None/Millis(2000)
pub struct JobSpec { pub input: std::path::PathBuf, pub options: TranscriptionOptions, pub output: OutputSpec }

// language.rs
pub struct Language(/* private */);                // Copy; code() -> &'static str; english_name() -> &'static str
impl Language { pub fn parse(code: &str) -> Option<Language>; pub fn all() -> &'static [Language]; }

// cancel.rs
pub struct CancelToken(/* Arc<AtomicBool> */);     // Clone; new(); cancel(); is_canceled() -> bool

// error.rs — thiserror enums, all #[non_exhaustive]
pub enum MediaError { BinaryMissing { binary: String, hint: String }, DecodeFailed { detail: String },
    ProbeFailed { detail: String }, NoAudioStream { path: std::path::PathBuf },
    Io(#[from] std::io::Error), Canceled }
pub enum EngineError { ModelLoad { path: std::path::PathBuf, detail: String },
    Inference { detail: String }, Canceled }
pub enum ModelError { Http { detail: String }, ChecksumMismatch { file: String },
    Unsupported { detail: String }, Io(#[from] std::io::Error), Canceled }
pub enum ExportError { Io { path: std::path::PathBuf, source: std::io::Error } }
pub enum CoreError { Media(#[from] MediaError), Engine(#[from] EngineError),
    Model(#[from] ModelError), Export(#[from] ExportError),
    InputMissing { path: std::path::PathBuf }, NoAudioDecoded, Canceled }

// boundary.rs
pub const SAMPLE_RATE: u32 = 16_000;
pub struct Pcm { /* samples: Arc<[f32]> */ }       // new(Vec<f32>); samples() -> &[f32]; len(); is_empty(); duration() -> Millis
pub struct MediaInfo { pub duration: Option<Millis>, pub has_audio: bool, pub has_video: bool }
pub struct EngineOptions { pub language: Option<Language>, pub task: TaskKind,
    pub initial_prompt: String, pub word_timestamps: bool, pub temperature: Vec<f32>,
    pub vad_model: Option<std::path::PathBuf>,      // Some => VAD on (F-045); None => off
    pub threads: Option<u32>, pub use_gpu: bool }   // threads None => adapter default; use_gpu default true
pub struct EngineCaps { pub word_timestamps: bool, pub translate: bool, pub vad: bool }
pub trait AudioDecoder: Send + Sync {
    fn decode(&self, path: &std::path::Path, cancel: &CancelToken) -> Result<Pcm, MediaError>;
}
pub trait MediaProber: Send + Sync {
    fn probe(&self, path: &std::path::Path) -> Result<MediaInfo, MediaError>;
}
pub trait SegmentSink {
    fn on_segment(&mut self, segment: Segment);
    fn on_progress(&mut self, fraction: f32);       // 0.0..=1.0
}
pub trait TranscriptionEngine: Send {
    fn transcribe(&mut self, audio: &Pcm, options: &EngineOptions, sink: &mut dyn SegmentSink,
                  cancel: &CancelToken) -> Result<(), EngineError>;
    fn capabilities(&self) -> EngineCaps;
}

// paths.rs
pub fn model_root() -> std::path::PathBuf;          // HUMM_MODEL_ROOT override, else <data_dir>/humm/models (ARCH §5)
pub fn log_dir() -> std::path::PathBuf;             // HUMM_LOG_DIR override, else macOS ~/Library/Logs/humm,
                                                    // Linux $XDG_STATE_HOME|~/.local/state/humm/logs, Windows %LOCALAPPDATA%\humm\logs
```
Notes: `ARCHITECTURE.md` §2/§4 also lists `TranscriptionId`, `Status`, `Source`, `MediaFetcher`, `AudioCapture`, `PcmStream`, `SecretStore`, `Telemetry` — those belong to M2+ slices; do NOT add them now (YAGNI; the architecture doc describes the end state).

**Steps:**

- [ ] **Step 1 (TDD): write unit tests first, module by module, then implement until green.** Required test cases:
  - `Millis` arithmetic: `Millis(3000) - Millis(1000) == Millis(2000)`; ordering.
  - `TaskKind`/`OutputFormat`/`ModelSize`: `as_str` round-trips through `FromStr` for every variant (iterate an `ALL` const); unknown string → `Err`. `ModelSize::Large.ggml_name() == "large-v1"`; `ModelSize::LargeV3Turbo.ggml_name() == "large-v3-turbo"`.
  - `Language`: port the FULL 100-entry table from `buzz/transcriber/transcriber.py:37-138` verbatim (code + English name, e.g. `("en","English") … ("yue","Cantonese")`). Tests: `Language::all().len() == 100`; `parse("en").unwrap().english_name() == "English"`; `parse("haw")` ok; `parse("xx").is_none()`; every entry's `code()` parses back to itself.
  - `TranscriptionOptions::default()` matches Buzz defaults (`transcriber.py:141-158`): language None, task Transcribe, word_timestamps false, prompt "", temperature == DEFAULT_TEMPERATURE; model size Small (humm deviation, UX_SPEC §6 — document it in the doc comment).
  - `Pcm`: `Pcm::new(vec![0.0; 16_000]).duration() == Millis(1000)`; `duration` rounds down (`16_015` samples → `Millis(1000)`); empty → `Millis(0)`.
  - `CancelToken`: fresh token not canceled; after `cancel()` all clones report canceled.
  - `paths`: implement as pure inner fns taking the env value as `Option<&std::ffi::OsStr>` (e.g. `fn model_root_from(env: Option<&OsStr>) -> PathBuf`) so tests never mutate process env (`std::env::set_var` is unsafe in edition 2024). Tests: override respected; default ends with `humm/models`.
  - Error `Display` messages: `MediaError::BinaryMissing { binary: "ffmpeg", hint: … }` renders the hint; spot-check one variant per enum.
- [ ] **Step 2: Run the gates.** `missing_docs` warnings count as clippy failures — document every public item.
- [ ] **Step 3: Commit** `M1: core domain model, boundary traits, errors (F-040, F-042 groundwork)`.

---

### Task 3: humm-core — export writers + filename templating (F-028)

**Files:**
- Create: `crates/humm-core/src/export.rs`
- Modify: `crates/humm-core/src/lib.rs`, `crates/humm-core/Cargo.toml` (dep: chrono; dev-dep: insta)

**Buzz sources to read first:** `buzz/transcriber/file_transcriber.py:185-239` (`write_output`, `to_timestamp`), `buzz/transcriber/transcriber.py:228-265` (`get_output_file_path`), `tests/transcriber/transcriber_test.py` (golden fixtures), `buzz/settings/settings.py` (default template, line ~170). Contract summary: `docs/analysis/EXTERNAL_SURFACE.md` §3.1–3.2.

**Interfaces:**
- Consumes: `Millis`, `Segment`, `TaskKind`, `OutputFormat`, `ModelSize`, `Language` (Task 2).
- Produces:
```rust
pub fn to_timestamp(ms: Millis, ms_separator: char) -> String;
pub enum TextKey { Text, Translation }
pub fn write_txt(segments: &[Segment], key: TextKey, paragraph_split: Millis,
                 w: &mut impl std::io::Write) -> std::io::Result<()>;
pub fn write_srt(segments: &[Segment], key: TextKey, w: &mut impl std::io::Write) -> std::io::Result<()>;
pub fn write_vtt(segments: &[Segment], key: TextKey, w: &mut impl std::io::Write) -> std::io::Result<()>;
pub fn write_format(format: OutputFormat, segments: &[Segment], key: TextKey,
                    paragraph_split: Millis, w: &mut impl std::io::Write) -> std::io::Result<()>;
pub const DEFAULT_EXPORT_TEMPLATE: &str = "{{ input_file_name }} ({{ task }}d on {{ date_time }})";
pub const DEFAULT_PARAGRAPH_SPLIT: Millis = Millis(2000);
pub fn output_file_path(input: &std::path::Path, task: TaskKind, language: Option<Language>,
    model_size: ModelSize, format: OutputFormat, output_directory: Option<&std::path::Path>,
    template: Option<&str>, now: chrono::NaiveDateTime) -> std::path::PathBuf;
```

**Behavior contract (port exactly — the fixtures are byte-law):**
- `to_timestamp`: `{hr:02}:{min:02}:{sec:02}{sep}{ms:03}` — hours zero-padded to 2 but growing beyond (`123456789` → `34:17:36.789`).
- TXT: for each segment, if a previous segment exists and `seg.start - prev.end >= paragraph_split`, append `"\n\n"`; then append `seg.text.trim()` + one space. Trailing space at EOF is part of the contract. TXT trims each segment's text; SRT/VTT write the text RAW (untrimmed) — Buzz strips upstream at task completion, which humm's pipeline replicates (Task 7).
- SRT: `{i+1}\n{ts(start, ',')} --> {ts(end, ',')}\n{text}\n\n`. VTT: header `WEBVTT\n\n`, then `{ts(start, '.')} --> {ts(end, '.')}\n{text}\n\n` (no index).
- `TextKey::Translation` selects `segment.translation` in every writer.
- `output_file_path`: stem of input; strip a trailing `_speech` (7 chars) if present; token replacement on the template — `{{ input_file_name }}`, `{{ task }}` (`as_str()`), `{{ language }}` (code or `""`), `{{ model_type }}` → literal `"Whisper.cpp"` (humm's single local backend maps to Buzz's whispercpp naming; ADR-008), `{{ model_size }}` (`as_str()`), `{{ date_time }}` (`now.format("%d-%b-%Y %H-%M-%S")`); append `.{ext}`; directory = `output_directory` or the input's parent.

**Steps:**

- [ ] **Step 1 (TDD): golden tests first** (write, watch fail, then implement):
```rust
// Ported verbatim from Buzz tests/transcriber/transcriber_test.py:12-43
#[test] fn to_timestamp_matches_buzz() {
    assert_eq!(to_timestamp(Millis(0), '.'), "00:00:00.000");
    assert_eq!(to_timestamp(Millis(123_456_789), '.'), "34:17:36.789");
}
// fixture segments
fn fixture() -> Vec<Segment> { vec![
    Segment { start: Millis(40), end: Millis(299), text: "Bien".into(), translation: String::new() },
    Segment { start: Millis(299), end: Millis(329), text: "venue dans".into(), translation: String::new() },
]}
#[test] fn txt_matches_buzz_golden()  { /* == "Bien venue dans " */ }
#[test] fn srt_matches_buzz_golden()  { /* == "1\n00:00:00,040 --> 00:00:00,299\nBien\n\n2\n00:00:00,299 --> 00:00:00,329\nvenue dans\n\n" */ }
#[test] fn vtt_matches_buzz_golden()  { /* == "WEBVTT\n\n00:00:00.040 --> 00:00:00.299\nBien\n\n00:00:00.299 --> 00:00:00.329\nvenue dans\n\n" */ }
```
  Additional required tests:
  - paragraph split: `[(0,1000,"a"), (3000,4000,"b")]` with split 2000 → `"a \n\nb "`; `[(0,1000,"a"), (2999,4000,"b")]` → `"a b "` (gap 1999 < 2000).
  - translation key: fixture with translations `"Wel"`/`"come in"` → SRT from `TextKey::Translation` carries the translations.
  - empty segments: TXT `""`, SRT `""`, VTT `"WEBVTT\n\n"`.
  - untrimmed text passes through raw in SRT (`" hi "` stays `" hi "`), but TXT trims it.
  - `output_file_path`: input `/tmp/interview_speech.mp3`, task Translate, default template, `now = 2026-07-20 14:03:05`, format Srt, no dir → `/tmp/interview (translated on 20-Jul-2026 14-03-05).srt` (`_speech` stripped). Custom template `"{{ input_file_name }}-{{ model_size }}-{{ language }}"` with language `Some(fr)`, size Small, format Txt, dir `/out` → `/out/interview-small-fr.txt`. `{{ model_type }}` renders `Whisper.cpp`.
  - one `insta` snapshot: 5 synthetic segments (with a paragraph gap, a translation, multi-line text) rendered in all three formats concatenated — guards future drift beyond the tiny goldens.
- [ ] **Step 2: implement** until green. `write_format` dispatches; writers take `impl Write` (files come later — pipeline owns file creation).
- [ ] **Step 3: Run the gates. Commit** `M1: export writers + filename templating, Buzz goldens ported (F-028)`.

---

### Task 4: humm-media — ffmpeg decode + ffprobe adapter (ADR-003)

**Files:**
- Create: `crates/humm-media/src/decode.rs`, `crates/humm-media/src/probe.rs`, `crates/humm-media/src/capabilities.rs`, `crates/humm-media/tests/contract.rs`
- Modify: `crates/humm-media/src/lib.rs`, `crates/humm-media/Cargo.toml` (deps: humm-core, serde, serde_json, log; dev-deps: tempfile, pretty_assertions)

**Buzz sources to read first:** `buzz/whisper_audio.py` (decode subprocess contract), ADR-003 in `docs/design/DECISIONS.md` (humm pipes `f32le` directly instead of Buzz's `s16le`+convert — same result, one fewer conversion).

**Interfaces:**
- Consumes: `AudioDecoder`, `MediaProber`, `Pcm`, `MediaInfo`, `MediaError`, `CancelToken`, `Millis` (Task 2).
- Produces:
```rust
pub struct FfmpegDecoder { /* binary: PathBuf */ }
impl FfmpegDecoder { pub fn new() -> Self;                       // env HUMM_FFMPEG else "ffmpeg"
                     pub fn with_binary(path: impl Into<std::path::PathBuf>) -> Self; }
impl humm_core::boundary::AudioDecoder for FfmpegDecoder { … }
pub struct FfprobeProber { /* binary: PathBuf */ }               // env HUMM_FFPROBE else "ffprobe"; same ctors
impl humm_core::boundary::MediaProber for FfprobeProber { … }
pub struct MediaCapabilities { pub ffmpeg_version: Option<String>, pub ffprobe_version: Option<String> }
pub fn probe_binaries() -> MediaCapabilities;                    // never errors; None = missing/unusable
```

**Behavior contract:**
- Decode command (from `whisper_audio.py:39-50`, f32le per ADR-003): `ffmpeg -nostdin -threads 0 -i <path> -f f32le -ac 1 -acodec pcm_f32le -ar 16000 -loglevel error -` → stdout. Stream stdout in chunks (read loop, e.g. 64 KiB buffer) into a `Vec<u8>`; drain stderr concurrently (thread) to avoid pipe deadlock; check `cancel.is_canceled()` between reads → `kill()` the child, return `MediaError::Canceled`. Non-zero exit → `DecodeFailed { detail: <trimmed stderr> }`. Spawn `ErrorKind::NotFound` → `BinaryMissing { binary, hint: "ffmpeg not found — install it (e.g. `brew install ffmpeg`) or set HUMM_FFMPEG" }`. Bytes → f32 via `chunks_exact(4)` + `f32::from_le_bytes`; a non-multiple-of-4 tail is discarded with a `log::warn!`.
- Probe: `ffprobe -v error -print_format json -show_format -show_streams <path>`; parse with serde structs (`format.duration` is an optional STRING of seconds → `Millis((secs * 1000.0).round())`; `has_audio` = any stream `codec_type == "audio"`; `has_video` = any `codec_type == "video"` whose `disposition.attached_pic != 1` — cover-art must not count as video). Parse failure → `ProbeFailed`.
- `probe_binaries`: run `<bin> -version`, parse leading `ffmpeg version <X>` token from the first line.

**Steps:**

- [ ] **Step 1 (TDD, unit — no binary needed):** pure-function tests: bytes→f32 conversion (`[0u8,0,128,63]` → `[1.0f32]`; 5-byte input → 1 sample + warn path); version-line parse (`"ffmpeg version 8.1.1 Copyright …"` → `"8.1.1"`); ffprobe JSON parse from an inline JSON literal containing one audio stream, one `attached_pic` video stream, `format.duration = "12.345"` → `MediaInfo { duration: Some(Millis(12345)), has_audio: true, has_video: false }`; missing duration → `None`.
- [ ] **Step 2: implement** decoder/prober/capabilities until unit tests green.
- [ ] **Step 3: contract tests** in `tests/contract.rs`, ALL `#[ignore = "requires ffmpeg + Buzz testdata"]`, using a `testdata()` helper reading `HUMM_BUZZ_TESTDATA` (Global Constraint 4):
  1. `decode_french_mp3_matches_probed_duration`: probe duration `d`, decode, assert `samples.len()` within ±2% of `d.0 * 16` and all samples finite.
  2. `decode_synthesized_sine_wav`: write a 16-bit PCM WAV (44-byte canonical header, hand-rolled in the test) of 1 s 440 Hz at 16 kHz mono into a tempdir; decode; assert `len` in `15_900..=16_100` and RMS error vs `sin(2π·440·t)` (amplitude 0.5) `< 0.02`.
  3. `probe_mp3`: `whisper-french.mp3` → `has_audio`, `!has_video`, `duration > 0`.
  4. `decode_corrupt_input_reports_stderr`: temp file `bad.mp3` containing `b"this is not media"` → `DecodeFailed` with non-empty detail.
  5. `missing_binary_actionable`: `FfmpegDecoder::with_binary("/nonexistent/ffmpeg")` → `BinaryMissing` whose Display mentions `HUMM_FFMPEG`.
  6. `cancel_kills_decode`: pre-canceled token + `audio-long.mp3` → returns `Canceled` in well under the file's decode time (< 2 s).
- [ ] **Step 4: run contract tests** once for real: `CARGO_HOME=… cargo test -p humm-media -- --ignored` — must pass on this machine (ffmpeg is installed). Then the gates. **Commit** `M1: humm-media ffmpeg/ffprobe adapter + contract tests (F-028 pipeline groundwork, ADR-003)`.

---

### Task 5: humm-core — model registry + downloader (F-042 partial, F-043 partial)

**Files:**
- Create: `crates/humm-core/src/models/mod.rs`, `crates/humm-core/src/models/registry.rs`, `crates/humm-core/src/models/download.rs`
- Modify: `crates/humm-core/src/lib.rs`, `crates/humm-core/Cargo.toml` (deps: reqwest, sha2, zip, serde, serde_json, log)

**Buzz sources to read first:** `buzz/model_loader.py` lines 119–155 (root/sizes/sentinel), 491–527 (repos + file naming), 670–738 (whisper.cpp download incl. CoreML zip extraction), 800–1031 (resume/SHA256/temp-file algorithm). Contract summary: `EXTERNAL_SURFACE.md` §3.8/§4; humm layout decision: `ARCHITECTURE.md` §5 ("Model cache").

**Interfaces:**
- Consumes: `ModelSize`, `ModelError`, `CancelToken`, `paths::model_root` (Task 2).
- Produces:
```rust
// registry.rs
pub const WHISPER_CPP_REPO: &str = "ggerganov/whisper.cpp";
pub const LUMII_REPO: &str = "RaivisDejus/whisper.cpp-lv";       // model_loader.py:491-492
pub const VAD_MODEL_FILE: &str = "ggml-silero-v6.2.0.bin";
pub const VAD_MODEL_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";
pub fn ggml_file_name(size: ModelSize) -> String;                 // "ggml-{ggml_name}.bin"
pub fn ggml_url(size: ModelSize) -> Result<String, ModelError>;   // https://huggingface.co/{repo}/resolve/main/{file}; Custom => Unsupported("custom model URLs arrive with M2")
pub fn coreml_zip_url(size: ModelSize) -> Result<String, ModelError>; // "…/ggml-{n}-encoder.mlmodelc.zip"
pub fn local_model_path(root: &std::path::Path, size: ModelSize) -> Option<std::path::PathBuf>;
    // Some iff <root>/<ggml_file> exists AND "<file>.humm_complete" sentinel exists (flat humm-owned layout, ARCH §5)
pub fn local_vad_model_path(root: &std::path::Path) -> Option<std::path::PathBuf>;   // same sentinel rule
pub const SENTINEL_SUFFIX: &str = ".humm_complete";

// download.rs
pub struct DownloadProgress { pub file: String, pub downloaded: u64, pub total: Option<u64> }
pub struct ModelDownloader { /* reqwest::blocking::Client, root: PathBuf */ }
impl ModelDownloader {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self;
    pub fn ensure_model(&self, size: ModelSize, coreml_encoder: bool,
        progress: &mut dyn FnMut(DownloadProgress), cancel: &CancelToken)
        -> Result<std::path::PathBuf, ModelError>;                // returns ggml path
    pub fn ensure_vad_model(&self, progress: &mut dyn FnMut(DownloadProgress),
        cancel: &CancelToken) -> Result<std::path::PathBuf, ModelError>;
}
```

**Behavior contract:**
- Layout: flat files directly under the root — `ggml-small.bin` + `ggml-small.bin.humm_complete`, extracted `ggml-small-encoder.mlmodelc/` (dir; completeness marker: file `.humm_complete` INSIDE the dir), `ggml-silero-v6.2.0.bin` + sentinel. This is humm-owned (ARCH §5) — deliberately NOT Buzz's HF-hub snapshot layout; the parity behaviors are the sentinel/resume/SHA256 semantics (F-043), not the directory shape.
- Per-artifact download algorithm (Buzz semantics, `model_loader.py:800-1031`, simplified to one temp-file scheme):
  1. final + sentinel present → done (no network).
  2. Work file is `<final>.partial`. If it exists with `len > 0`, send `Range: bytes=<len>-`; HTTP 206 → append; HTTP 200 → truncate and restart. No partial → plain GET.
  3. Stream in chunks (8 KiB); after each chunk call `progress` (downloaded = resumed offset + written, total from `Content-Length`/`Content-Range` when present) and check `cancel` → keep `.partial` for future resume, return `ModelError::Canceled`.
  4. On completion: fetch expected SHA256 from the HF API — `GET https://huggingface.co/api/models/{repo}/tree/main` (JSON array; the entry for the file has `lfs.oid` = sha256). If obtainable, hash the file (sha2, streamed 8 KiB) and compare; mismatch → delete `.partial` and final, `ChecksumMismatch`. If the API call fails, log a warning and skip verification ("SHA256 where upstream provides digests", F-043).
  5. Rename `.partial` → final, write the sentinel (empty file). Sentinel written LAST.
- `ensure_model(size, coreml_encoder=true, …)` also downloads + extracts the CoreML encoder zip when the platform warrants it (caller passes `cfg!(all(target_os="macos", target_arch="aarch64"))`): download zip via the same algorithm (no sentinel for the zip), extract with the `zip` crate to a temp dir under the root, remove `__MACOSX`, then if the temp contains exactly one directory move it as `<root>/ggml-<n>-encoder.mlmodelc`, else move the contents into that dir (`model_loader.py:706-735`); write the in-dir sentinel; delete the zip. HTTP 404 on the zip → log + continue without CoreML (lumii has none; graceful degradation).
- Custom size → `Unsupported` with an actionable "custom model URLs arrive with M2" message. Lumii uses `LUMII_REPO`.

**Steps:**

- [ ] **Step 1 (TDD, unit — no network):** tests: `ggml_file_name(Large) == "ggml-large-v1.bin"`; `ggml_url(Small)` exact string; `ggml_url(Lumii)` uses `RaivisDejus/whisper.cpp-lv`; `ggml_url(Custom)` is `Unsupported`; `coreml_zip_url(Small)` exact; `local_model_path` in a tempdir: file only → `None`, file+sentinel → `Some`; sha256 helper on a temp file with a precomputed hash (hash of `b"humm"` = `895e40a…` — compute the real constant with `printf 'humm' | shasum -a 256` and inline it); HF tree JSON parse from an inline literal (`[{"path":"ggml-tiny.bin","lfs":{"oid":"abc…"}}]`) → oid lookup; zip extraction: build an in-memory zip (zip crate writer) containing `__MACOSX/junk` + `ggml-x-encoder.mlmodelc/model.mil` → extract → nested-dir handling produces `<root>/ggml-x-encoder.mlmodelc/model.mil`, no `__MACOSX`.
- [ ] **Step 2: implement** registry + downloader until green. Factor the resume decision (`fn resume_plan(partial_len: Option<u64>) -> Option<String /* Range header */>`) and the completion steps as testable pure pieces.
- [ ] **Step 3: contract test** `tests/model_download_contract.rs`, `#[ignore = "requires network (huggingface.co)"]`: into a tempdir root, `ensure_vad_model` (smallest real artifact) → file + sentinel exist, progress was called with increasing `downloaded`; call again → completes instantly without network (assert file mtime unchanged); then delete the sentinel, truncate the model file to half, rename it to `.partial` → `ensure_vad_model` resumes and the final SHA256 matches the first download's.
- [ ] **Step 4: run** `cargo test -p humm-core -- --ignored` once for real (network permitting — if the sandbox blocks huggingface.co, escalate per harness rules rather than skipping). Then the gates. **Commit** `M1: model registry + downloader — sentinels, resume, SHA256, CoreML zip (F-042, F-043)`.

---

### Task 6: humm-whisper — whisper-rs engine adapter (F-040, F-045)

**Files:**
- Create: `crates/humm-whisper/src/engine.rs`, `crates/humm-whisper/src/words.rs`, `crates/humm-whisper/tests/contract.rs`
- Modify: `crates/humm-whisper/src/lib.rs`, `crates/humm-whisper/Cargo.toml`

**Buzz sources to read first:** `buzz/transcriber/whisper_cpp.py` — `_build_command` (lines 167–196: the exact whisper flags Buzz passes), `_make_offset_mapper` (48–74), `_parse_word_level_timings` (240–319), `_flush_complete_chars`/`_append_word` (77–125), `transcribe` (358–400). Also ADR-002 in `DECISIONS.md`. **Primary API reference: docs.rs/whisper-rs/0.16.0 — verify every method name against the real crate; do not trust this plan's guesses about whisper-rs API names.** The vendored whisper.cpp source lives in the crate's build tree; consult `examples/cli/cli.cpp` there for whisper-cli's default sampling parameters.

**Environment prerequisite:** whisper-rs-sys builds whisper.cpp via CMake. If `cmake` is missing, STOP and report BLOCKED (the controller will install it) — do not attempt to install system packages yourself.

**Interfaces:**
- Consumes: `TranscriptionEngine`, `SegmentSink`, `EngineOptions`, `EngineCaps`, `EngineError`, `Pcm`, `Segment`, `Millis`, `CancelToken` (Task 2).
- Produces:
```rust
pub struct WhisperEngine { /* ctx + model path */ }
impl WhisperEngine {
    /// Loads a ggml model. use_gpu=false forces CPU (HUMM_FORCE_CPU is read by the CLI, not here).
    pub fn load(model_path: &std::path::Path, use_gpu: bool) -> Result<Self, humm_core::error::EngineError>;
}
impl humm_core::boundary::TranscriptionEngine for WhisperEngine { … }
// words.rs — pure, unit-testable word/char grouping (no FFI):
pub struct TokenInfo { pub bytes: Vec<u8>, pub start: Millis, pub end: Millis, pub p: f32 }
pub fn group_words(segment_tokens: &[TokenInfo], language: Option<&str>) -> Vec<Segment>;
```
- Cargo: `whisper-rs = { workspace = true }`; on macOS enable features via `[target.'cfg(target_os = "macos")'.dependencies] whisper-rs = { workspace = true, features = ["metal", "coreml"] }`.

**Behavior contract:**
- `load`: missing file → `ModelLoad { path, detail: "model file missing — download it first or set HUMM_MODEL_ROOT" }`; whisper-rs init failure → `ModelLoad` with the library's message. With the `coreml` feature, whisper.cpp auto-discovers `ggml-<n>-encoder.mlmodelc` next to the ggml file and falls back gracefully when absent — rely on that (matches Task 5 layout).
- `transcribe` FullParams (parity with Buzz's whisper-cli invocation, `whisper_cpp.py:167-196`):
  - Sampling strategy: whatever whisper-cli defaults to in the VENDORED whisper.cpp (check `cli.cpp`; historically beam search, beam_size 5) — record what you found in a code comment with the whisper.cpp version.
  - `language`: `Some(code)` or `"auto"` when `options.language` is None (`whisper_cpp.py:361-365`); `translate` = task Translate; `initial_prompt` when non-empty; temperature: initial = `temperature[0]`, increment = `temperature[1] - temperature[0]` when len ≥ 2 else leave the whisper default; `suppress_nst` / suppress-non-speech-tokens = true; `entropy_thold` = 2.8; `n_max_text_ctx` = 0 (Buzz's `--max-context 0`); threads = `options.threads` else `max(1, available_parallelism/2)` (Buzz: `(cpu_count or 8)//2`); disable all print flags.
  - Progress: wire the whisper progress callback → `sink.on_progress(pct as f32 / 100.0)`.
  - Streaming: wire the new-segment callback so `sink.on_segment` fires as segments are produced (segment text + `t0/t1` centiseconds × 10 → `Millis`). The final returned state must equal what streamed.
  - Cancellation: wire the abort callback to `cancel.is_canceled()`; when whisper aborts and the token is set, return `EngineError::Canceled` (not `Inference`).
  - Word timestamps (`options.word_timestamps`): enable token-level timestamps; after the run, for each segment collect `TokenInfo` (raw token TEXT BYTES — token pieces can split multi-byte UTF-8 chars, which is why `bytes` not `String`; token t0/t1 ×10; token probability) and emit `group_words(...)` output instead of segment-level segments.
  - VAD (`options.vad_model = Some(path)`): enable whisper-rs 0.16's VAD (`enable_vad`/`set_vad_model_path`/VAD params — verify names on docs.rs), leaving VAD tuning params at library defaults (Buzz passes only `--vad --vad-model`). **Empirical requirement (F-045):** with VAD on, output timestamps must be on the ORIGINAL audio timeline. Verify with the contract test below; if the vendored whisper.cpp does not remap (tokens/segments come back on the VAD-compressed timeline), port Buzz's `_make_offset_mapper` (`whisper_cpp.py:48-74`) scaling into the adapter. Record which case you hit in your report.
- `group_words` (exact port of `_parse_word_level_timings`, `whisper_cpp.py:240-319`):
  - Skip tokens whose text starts with `[_` and empty tokens.
  - Space-delimited languages: skip tokens with `p < 0.01`; a token starting with `b" "` flushes the current word and starts a new one; a token starting with `b", "` appends `,` to the current word, flushes it, and starts the new word from the token minus the leading comma; otherwise append to the current word (extending its end time). Flush trailing word. Word text = trimmed UTF-8 decode of the buffer (skip the word if the buffer is not valid UTF-8 after the run, matching `_append_word`).
  - Non-space languages `{zh, ja, th, lo, km, my}` (`whisper_cpp.py:244`): emit one Segment PER CHARACTER; buffer token bytes and flush every complete UTF-8 char (1–4 bytes) with the span of the tokens that contributed (`_flush_complete_chars`); no `p` filter on this path.
  - All emitted segments have `translation: ""`.
- `capabilities()`: all three true.

**Steps:**

- [ ] **Step 1 (TDD, pure):** `words.rs` unit tests first — no FFI, no model:
  1. `[" Bien"(40,299,p=1)," venue"(299,320,1)," dans"(320,329,1)]` → 3 words `Bien/venue/dans` with those spans.
  2. comma rule: `[" Hi"(0,10,1), ", there"(10,20,1)]` → `Hi,`(0,10), `there`(10,20).
  3. `p < 0.01` token skipped (space-language path).
  4. `[_TT_50]`-style tokens skipped.
  5. UTF-8 split across tokens: `é` as `[0xC3]` then `[0xA9]` in consecutive tokens within one word → single word `é`.
  6. Japanese: tokens carrying `こんにちは` split mid-char across token boundaries → 5 per-char segments, times spanning contributing tokens.
- [ ] **Step 2: implement `words.rs`** until green (this compiles without whisper-rs).
- [ ] **Step 3: add whisper-rs + implement `engine.rs`** against docs.rs/whisper-rs/0.16.0. `cargo build -p humm-whisper` must succeed with Metal+CoreML on this Mac.
- [ ] **Step 4: contract tests** (`tests/contract.rs`, all `#[ignore]`; dev-dep on humm-media for decoding; model = `<testdata>/ggml-tiny.bin` directly — no network):
  1. `transcribes_french_mp3`: decode `whisper-french.mp3`, language `fr` → ≥1 segment, non-empty trimmed text, `start < Millis(2000)`, `end ≤ duration + 2 s`; streamed segments == returned segments; progress reached ≥ 0.99 at least once.
  2. `translate_task_succeeds`: task Translate → completes with ≥1 non-empty segment (tiny-model quality — no text assertions).
  3. `cancel_is_prompt`: spawn cancel after 200 ms on `audio-long.mp3` → returns `Canceled` in < 5 s.
  4. `word_timestamps_produce_words`: word mode on french → more segments than segment mode, mean text length < 12 chars, times monotonically non-decreasing.
  5. `vad_remaps_to_original_timeline` (`#[ignore = "requires network for VAD model"]`): get the silero model via `ModelDownloader::ensure_vad_model` into a temp root; build a Pcm of 5 s of zeros + the decoded french samples; VAD OFF → record first-segment start; VAD ON → first segment start ≥ `Millis(4000)` (i.e. remapped — NOT collapsed toward 0) and last end ≤ padded duration + 2 s.
- [ ] **Step 5: run** `cargo test -p humm-whisper -- --ignored` for real (test 5 may need network escalation). Then the gates. **Commit** `M1: whisper-rs engine adapter — Metal+CoreML, streaming sink, abort, VAD, word timings (F-040, F-045)`.

---

### Task 7: humm-core — file-transcription pipeline

**Files:**
- Create: `crates/humm-core/src/pipeline.rs`
- Modify: `crates/humm-core/src/lib.rs`

**Buzz sources to read first:** `buzz/transcriber/file_transcriber.py:38-73` (`FileTranscriber.run`: transcribe → strip each segment's text → export loop).

**Interfaces:**
- Consumes (exact signatures from Tasks 2–3): the Task 2 types/traits (`JobSpec`, `TranscriptionOptions`, `OutputSpec`, `EngineOptions`, `AudioDecoder::decode(&self, &Path, &CancelToken) -> Result<Pcm, MediaError>`, `MediaProber::probe(&self, &Path) -> Result<MediaInfo, MediaError>`, `TranscriptionEngine::transcribe(&mut self, &Pcm, &EngineOptions, &mut dyn SegmentSink, &CancelToken) -> Result<(), EngineError>`, `SegmentSink`) and the Task 3 exports (`write_format(OutputFormat, &[Segment], TextKey, Millis, &mut impl Write) -> io::Result<()>`, `output_file_path(input, task, language, model_size, format, output_directory, template, now) -> PathBuf`).
- Produces:
```rust
pub struct JobOutcome { pub segments: Vec<Segment>, pub written: Vec<std::path::PathBuf> }
pub struct EngineTuning { pub vad_model: Option<std::path::PathBuf>, pub use_gpu: bool,
                          pub threads: Option<u32> }   // Default: None/true/None
pub fn run_file_job(job: &JobSpec, tuning: &EngineTuning, decoder: &dyn AudioDecoder,
    prober: &dyn MediaProber, engine: &mut dyn TranscriptionEngine,
    observer: &mut dyn SegmentSink, cancel: &CancelToken) -> Result<JobOutcome, CoreError>;
```

**Behavior contract:**
1. `job.input` missing → `CoreError::InputMissing` (actionable Display naming the path).
2. Probe: `has_audio == false` → `MediaError::NoAudioStream`. A probe ERROR is non-fatal (log warn, continue) — ffmpeg's decode is the authoritative failure source (Buzz never probes before decode).
3. Decode (cancel-aware). Empty PCM → `CoreError::NoAudioDecoded`.
4. Build `EngineOptions` here: language/task/prompt/word_timestamps/temperature from `job.options`; `vad_model`/`use_gpu`/`threads` from `tuning`.
5. Transcribe through a tee sink: forwards `on_segment`/`on_progress` to `observer` AND collects segments.
6. Normalize: `text = text.trim()` in place for every segment (`file_transcriber.py:51-52`); translations untouched.
7. Export: for each format in `job.output.formats` (BTreeSet order): path via `output_file_path(…, now = chrono::Local::now().naive_local())`; `create_dir_all` on the parent if absent (humm improvement — Buzz fails on a missing `-d` directory; note as deviation in the doc comment); write with the Task 3 writer; io error → `ExportError::Io { path, source }`. Check cancel before each format; canceled → return `CoreError::Canceled` (already-written files stay — matches kill semantics).
8. Return outcome with segments + written paths in format order.

**Steps:**

- [ ] **Step 1 (TDD):** in-module fakes: `FakeDecoder` (fixed `Pcm`, records call count), `FakeProber` (configurable `MediaInfo` / error), `FakeEngine` (emits segments `" Bien "` (40–299) and `"venue dans"` (299–329) + progress 0.5/1.0), `NullSink`/`RecordingSink`. Tests:
  1. happy path with formats `{Txt, Srt}`, template override `Some("{{ input_file_name }}")`, tempdir output → outcome text trimmed (`"Bien"`); files `input.txt` == `"Bien venue dans "`, `input.srt` == the Task 3 golden; `written` = `[…txt, …srt]` (BTreeSet order Txt<Srt<Vtt).
  2. empty `formats` → no files, `written` empty, Ok.
  3. engine `Inference` error → propagated, zero files written.
  4. pre-canceled token → `Canceled`, decoder never called.
  5. missing input path → `InputMissing`.
  6. prober `has_audio: false` → `NoAudioStream`; prober returning `Err` while decode succeeds → Ok (probe soft-fails).
  7. observer received the same segments the outcome carries plus progress values.
- [ ] **Step 2: implement** until green. Gates. **Commit** `M1: core file-transcription pipeline (F-028, F-002 groundwork)`.

---

### Task 8: humm-cli — `add` (F-002 partial, F-009 logging)

**Files:**
- Create: `crates/humm-cli/src/args.rs`, `crates/humm-cli/src/logging.rs`, `crates/humm-cli/src/run.rs`, `crates/humm-cli/tests/cli.rs`
- Modify: `crates/humm-cli/src/main.rs`, `crates/humm-cli/Cargo.toml` (deps: clap {derive}, anyhow, log, chrono, humm-core, humm-media, humm-whisper; dev-deps: assert_cmd, predicates, tempfile)

**Buzz sources to read first:** `buzz/cli.py` (whole file — grammar, defaults, error strings, model resolution order), `EXTERNAL_SURFACE.md` §2/§8, ADR-007, UX_SPEC §5 (CLI contract) + §6 (default model **small** — deliberate deviation from Buzz's tiny, already approved in UX_SPEC).

**Interfaces:**
- Consumes (exact signatures from earlier tasks):
  - Task 2: `Language::parse(&str) -> Option<Language>`, `TaskKind`/`ModelSize`/`OutputFormat` `FromStr`, `TranscriptionOptions`, `OutputSpec`, `JobSpec`, `CancelToken`, `paths::log_dir()`, `paths::model_root()`.
  - Task 5: `models::local_model_path(&Path, ModelSize) -> Option<PathBuf>`, `models::local_vad_model_path(&Path) -> Option<PathBuf>`, `ModelDownloader::new(root)`, `.ensure_model(ModelSize, coreml_encoder: bool, &mut dyn FnMut(DownloadProgress), &CancelToken) -> Result<PathBuf, ModelError>` (pass `coreml_encoder = cfg!(all(target_os = "macos", target_arch = "aarch64"))`), `.ensure_vad_model(…)` same shape.
  - Task 6: `WhisperEngine::load(&Path, use_gpu: bool) -> Result<WhisperEngine, EngineError>`.
  - Task 7: `run_file_job(&JobSpec, &EngineTuning, &dyn AudioDecoder, &dyn MediaProber, &mut dyn TranscriptionEngine, &mut dyn SegmentSink, &CancelToken) -> Result<JobOutcome, CoreError>`, `EngineTuning { vad_model, use_gpu, threads }`.
  - Task 4: `FfmpegDecoder::new()`, `FfprobeProber::new()`.
- Produces: the `humm` binary. M1 grammar (exact Buzz spellings, `cli.py:52-138`):
  ```
  humm add [-t|--task transcribe|translate] [-s|--model-size <size>] [-l|--language <code>]
           [-p|--prompt <text>] [-w|--word-timestamps] [--srt] [--vtt] [--txt]
           [-d|--output-directory <dir>] <file> [file …]
  ```
  Buzz flags NOT in M1 (arrive M2/M6 — do not add them): `-m/--model-type`, `--hfid`, `-e/--extract-speech`, `--openai-token`, `--hide-gui`, URLs as positionals, `--json`.

**Behavior contract:**
- Defaults: task `transcribe`; model-size **`small`** (UX_SPEC §6 deviation — Buzz defaults `tiny`); language empty = auto-detect; word-timestamps off. `--help` (exit 0) shows every default.
- Validation (all exit **1**, single-line stderr message + a "run `humm add --help`" hint; use `Cli::try_parse` and map clap errors to exit 1 — clap's default exit 2 conflicts with ADR-007 where 2 = task failures):
  - zero positional files → `Error: No input files` (`cli.py:212-213`).
  - unknown language → `Error: Invalid language option` (`cli.py:226-227`).
  - invalid `--task`/`--model-size` value → clap value-enum error, exit 1.
  - `-s custom` → exit 1 with the M2 message from `ModelError::Unsupported`.
- Logging (F-009): initialize FIRST (before arg validation): a hand-rolled `log::Log` impl (no new deps) writing DEBUG+ to `paths::log_dir()/logs.txt` (create dirs; append) with format `{RFC3339 time} {level} {module}:{line} — {message}`, and WARN+ to stderr. Log-file init failure must not crash the CLI (fall back to stderr-only with one warning).
- Run flow (`run.rs`):
  1. Check inputs: files that don't exist are recorded as failed tasks up front (no model download if ALL inputs are missing — fail fast with exit 2).
  2. Resolve model (`cli.py:141-159` order: download-if-absent, synchronously, before tasks): `local_model_path` else `ModelDownloader::ensure_model` with a `\r`-rewriting stderr progress line (`Downloading ggml-small.bin  42% (198.4/466.2 MB)`) when stderr is a TTY (`std::io::IsTerminal`), plain occasional lines otherwise. Download/config failure → exit 1.
  3. Ensure VAD model (best-effort): `local_vad_model_path` else try `ensure_vad_model`; on failure `log::warn!` and continue WITHOUT VAD (F-045 parity: absent model → silently no VAD).
  4. `WhisperEngine::load(model_path, use_gpu = env HUMM_FORCE_CPU unset/false)`; `HUMM_WHISPER_N_THREADS` parsed → `EngineTuning.threads`.
  5. For each existing input sequentially (one at a time — queue parity): build `JobSpec` (formats from `--srt/--vtt/--txt`; `OutputSpec.paragraph_split` from `HUMM_PARAGRAPH_SPLIT_TIME` else 2000; template None = default), run `run_file_job` with a sink rendering `\r<file>  NN%` to stderr on TTY. Success → print each written path to stdout, one per line (UX_SPEC §5 composability). Failure → `error: <input>: <cause>` on stderr, continue with the next file.
  6. Zero formats selected → still transcribe (Buzz parity), print nothing to stdout, log a warning `no export format selected (--txt/--srt/--vtt)`.
  7. Exit code: 0 all succeeded; 2 if ≥1 task failed, with a final stderr summary listing failed inputs (ADR-007, UX_SPEC §5).
- `humm` with no subcommand → clap help to stderr, exit 1 (headless; deviation ADR-007 — Buzz would launch the GUI).

**Steps:**

- [ ] **Step 1 (TDD): integration tests** in `tests/cli.rs` with `assert_cmd` (every invocation sets `HUMM_LOG_DIR` to a tempdir):
  1. `no_subcommand_exits_1`, 2. `add_without_files_says_no_input_files` (exit 1, stderr contains `No input files`), 3. `invalid_language_rejected` (`-l xx` → exit 1, `Invalid language option`), 4. `invalid_task_rejected` (`-t foo` → 1), 5. `invalid_size_rejected` (`-s foo` → 1), 6. `custom_size_points_to_m2` (`-s custom` with an existing dummy input file → exit 1, message mentions M2/custom), 7. `missing_inputs_exit_2_without_model` (`humm add /nope.mp3 --txt` + `HUMM_MODEL_ROOT` pointing at an EMPTY tempdir → exit 2, stderr names `/nope.mp3`; proves no download attempted since exit fast — also assert stderr does NOT contain "Downloading"), 8. `help_lists_flags_and_defaults` (`add --help` → 0; contains `--word-timestamps`, `--model-size`, `small`), 9. `log_file_written` (after case 2, `<HUMM_LOG_DIR>/logs.txt` exists, non-empty — F-009 cli-int verification).
- [ ] **Step 2: implement** args/logging/run until 1–9 green (they run in the normal suite — no model, no network, no ffmpeg needed).
- [ ] **Step 3: e2e integration test** `add_transcribes_and_exports` — `#[ignore = "requires ffmpeg + Buzz testdata"]`: tempdir as `HUMM_MODEL_ROOT`; copy `<testdata>/ggml-tiny.bin` to `<root>/ggml-tiny.bin` + create `ggml-tiny.bin.humm_complete`; run `humm add <testdata>/whisper-french.mp3 --srt --txt -s tiny -l fr -d <temp_out>` → exit 0; exactly one `.srt` + one `.txt` in `<temp_out>` matching `whisper-french (transcribed on *)`; SRT starts `1\n00:`; TXT non-empty; stdout = the two paths. Run it for real: `cargo test -p humm-cli -- --ignored`.
- [ ] **Step 4: gates. Commit** `M1: humm-cli add — grammar, exit codes, logging, e2e (F-002, F-009)`.

---

### Task 9: Criterion suite (decode, export, e2e) + baseline record

**Files:**
- Create: `crates/humm-core/benches/export.rs`, `crates/humm-media/benches/decode.rs`, `crates/humm-whisper/benches/e2e.rs`, `docs/benchmarks/M1.md`
- Modify: the three `Cargo.toml`s (`[dev-dependencies] criterion`, `[[bench]] harness = false`)

**Interfaces:** consumes public APIs only.

**Steps:**

- [ ] **Step 1: export bench** — build 10 000 synthetic segments once; bench `write_srt`, `write_vtt`, `write_txt` into a reused `Vec<u8>`.
- [ ] **Step 2: decode bench** — `FfmpegDecoder` on `<testdata>/whisper-french.mp3`; if ffmpeg or testdata is absent, print a skip line and return without registering the group (bench must not panic on other machines). `sample_size(10)`.
- [ ] **Step 3: e2e bench** — decode + `WhisperEngine` transcribe of `whisper-french.mp3` with `<testdata>/ggml-tiny.bin` (GPU on), `sample_size(10)`, `measurement_time(60 s)`; same skip guard. This is THE regression-tracked transcription path.
- [ ] **Step 4: run** `CARGO_HOME=… cargo bench` and write `docs/benchmarks/M1.md`: host line (chip, RAM, macOS), table of median times for each bench, and the note that these are the M1 baselines future milestones diff against (regression = blocking failure per CLAUDE.md).
- [ ] **Step 5: gates (benches compile under `clippy --all-targets`). Commit** `M1: criterion suite + baselines (decode, e2e, export)`.

---

### Task 10: ADR-002 validation benchmark — whisper-rs vs candle

**Files:**
- Create: `validation/adr-002/` (standalone cargo project, NOT a workspace member — add `"validation/*"` to workspace `exclude`), with `src/main.rs` (a small runner) or plain shell steps documented in `validation/adr-002/README.md`
- Modify: `docs/design/DECISIONS.md` (ADR-002 gains a "**Validation result (M1)**" subsection), workspace `Cargo.toml` (exclude)

**Protocol (record every number in DECISIONS.md):**

- [ ] **Step 1: prepare input** — `ffmpeg -i <testdata>/audio-long.mp3 -ac 1 -ar 16000 <scratch>/audio-long-16k.wav` (both contenders consume the same file; also note its duration via ffprobe).
- [ ] **Step 2: whisper-rs side = the real product path.** Download `small` via humm (`humm add … -s small` triggers it) then measure: `/usr/bin/time -l target/release/humm add <scratch>/audio-long-16k.wav --txt -s small -l en -d <out1>` (release build; run once as warm-up, then 2 measured runs; record wall clock and `maximum resident set size`). Keep the TXT output.
- [ ] **Step 3: candle side.** Shallow-clone `https://github.com/huggingface/candle` at its latest release tag into `<scratch>/candle`; run its whisper example with Metal: `cargo run --release --features metal --example whisper -- --model small --input <scratch>/audio-long-16k.wav` (verify the example's actual flags from its README/source; it downloads the HF model itself). Same warm-up + 2 measured runs under `/usr/bin/time -l`; capture the transcript from stdout.
- [ ] **Step 4: repeat both sides with `large-v3-turbo`** (ggml ~1.6 GB, candle side comparable). If a side cannot run this size (disk/time/example limitations), record exactly why instead of numbers.
- [ ] **Step 5: quality proxy.** Normalize both transcripts (lowercase, strip punctuation, collapse whitespace) and compute word-level edit distance between them (a ~30-line function in the runner); report similarity % and eyeball 3 sample sentences from each for sanity. (No ground-truth reference exists; pairwise agreement + inspection is the WER proxy.)
- [ ] **Step 6: write DECISIONS.md** — table: model size × {wall s, peak RSS MB, transcript agreement %} for both engines + the verdict sentence (whisper-rs confirmed, or evidence presented). Note CoreML first-load compile latency if observed (risk-register item).
- [ ] **Step 7: commit** `M1: ADR-002 validation benchmark — whisper-rs vs candle numbers recorded`. The `validation/` project must not break the workspace gates (it is excluded; still leave it warning-free).

**Notes:** needs huggingface.co network (model downloads, several GB total) — escalate sandbox per harness rules when the download is blocked. candle API churn is a real risk: if the example fails to build at the pinned tag, try the latest tag; if it still fails, record the failure evidence in the ADR (the pre-approved default stands unless candle demonstrably wins).

---

### Task 11: Parity matrix + docs sweep

**Files:**
- Modify: `docs/analysis/PARITY_MATRIX.md`, plus any analysis/design doc corrections accumulated in task reports.

**Steps:**

- [ ] **Step 1: status updates** (statuses reflect what verification ACTUALLY ran — `verified` requires the row's full stated method):
  - F-002 → `in-progress` (partial grammar; cli-int ran for the M1 subset).
  - F-009 → `verified` (cli-int executed and passed).
  - F-028 → `implemented` (snapshot ✅; `diff` half runs with M2's differential harness — note inline).
  - F-040 → `implemented` (contract ✅ + bench ✅; diff pending) — add inline note "whisper-rs engine, ADR-002 validated M1, ADR-008 consolidation".
  - F-042 → `in-progress` (sizes + lumii done; custom/HF-repo in M2).
  - F-043 → `in-progress` (ggml download/resume/SHA256/sentinel/CoreML done; `.pt`/HF-hub layouts are N/A per ADR-008 — note inline; custom-URL download M2).
  - F-045 → `implemented` (contract ✅; diff vs Buzz pending).
- [ ] **Step 2:** sweep task reports for discovered analysis/design errors; fix those docs now if any slipped past their task's commit.
- [ ] **Step 3: gates (docs-only, still run). Commit** `M1: parity matrix update + docs sweep (F-002, F-009, F-028, F-040, F-042, F-043, F-045)`.
