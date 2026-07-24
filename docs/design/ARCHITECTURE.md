# humm — Architecture

Rust-native design from first principles. Buzz defines *what* (see `../analysis/`), never *how*. No transliterated Qt signal/slot patterns, no Python class-hierarchy echoes: boundaries are traits, events are channels, state is owned.

## 1. Workspace layout

```
humm/
├── Cargo.toml                 # workspace, shared lints/profiles
├── crates/
│   ├── humm-core/             # THE library. Domain model, boundary traits, task queue,
│   │                          # export writers, model registry+downloads, OpenAI API engine,
│   │                          # LLM translator, telemetry, settings model. Zero UI deps.
│   │                          # #![warn(missing_docs)], no unsafe.
│   ├── humm-media/            # ADAPTER: ffmpeg / ffprobe / yt-dlp subprocesses
│   ├── humm-whisper/          # ADAPTER: whisper-rs FFI (Metal + CoreML), incl. Silero VAD
│   ├── humm-audio/            # ADAPTER: cpal capture/playback + rubato resampling
│   ├── humm-store/            # ADAPTER: rusqlite persistence + keyring secrets + Buzz import
│   ├── humm-cli/              # binary: headless CLI (clap)
│   └── humm-gui/              # binary: egui/eframe app
└── docs/                      # analysis/ + design/ (living documents)
```

Dependency rule (enforced by crate graph): binaries → adapters → core. Core depends on **no** adapter and no UI. Adapters implement core traits; nothing outside an adapter names ffmpeg, whisper.cpp, SQLite, PortAudio, or yt-dlp. `unsafe` is permitted only in `humm-whisper` (FFI edge), documented with safety invariants.

Why one adapter crate per boundary (not modules in core): Standing Policy isolation, independent compile units (whisper.cpp's C build doesn't rebuild on core changes), and per-adapter contract-test suites that validate an external-component upgrade in isolation.

Pure-Rust services (OpenAI API engine, translator, model downloader, telemetry) live *in core* — the Standing Policy's adapter rule targets non-Rust boundaries only; splitting HTTP code out would be ceremony.

## 2. Boundary traits (defined in `humm-core`)

```rust
/// Decode any media file to 16 kHz mono f32 PCM. Impl: ffmpeg subprocess.
pub trait AudioDecoder: Send + Sync {
    fn decode(&self, path: &Path, cancel: &CancelToken) -> Result<Pcm, MediaError>;
    /// Playback decode (M4 as-built, ADR-019). The original sketch here was a
    /// seekable, chunked `open_stream(...) -> PcmStream`; the M4 playback
    /// engine instead decodes the WHOLE file to interleaved f32 in memory —
    /// `PlaybackPcm { samples: Arc<[f32]>, channels, sample_rate }` at
    /// 44.1 kHz stereo (Buzz-equal memory, ~1.27 GB/hour) — because O(1)
    /// sample-exact seek/loop and a trivial position model matter more than
    /// bounded memory at typical media lengths (revisit for long-file use).
    fn decode_playback(&self, path: &Path, cancel: &CancelToken)
        -> Result<PlaybackPcm, MediaError>;
}

/// Probe container/stream metadata. Impl: ffprobe -print_format json.
pub trait MediaProber: Send + Sync {
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError>; // duration, has_audio, has_video, video: Option<VideoStreamInfo>
}

/// Video frame streaming for the viewer's video pane (M4 as-built, ADR-019).
/// Impl: ffmpeg `-f rawvideo -pix_fmt rgba` subprocess (Buzz used rgb24 +
/// convert, ledger #10). A reader thread fills a bounded 60-frame deque; the
/// UI picks the frame for the current AUDIO position each repaint (audio clock
/// is master — §7). `seek` = kill + respawn at the position (Buzz parity).
pub trait VideoStreamer: Send + Sync {
    fn open(&self, path: &Path, start: Millis, info: &VideoStreamInfo)
        -> Result<Box<dyn FrameStream>, MediaError>;
}
pub trait FrameStream: Send {
    fn frame_for(&mut self, position: Millis) -> Option<VideoFrame>; // None = keep last
    fn seek(&mut self, position: Millis) -> Result<(), MediaError>;
}

/// Live audio output for playback (M4 as-built, ADR-019; `humm-audio::CpalOutput`).
/// `fill` is called on cpal's RT thread; device-rate negotiation + rubato
/// resample-to-device live inside the adapter, never on the master-clock path.
pub trait AudioOutput: Send {
    fn open(&self, spec: StreamSpec, fill: FillCallback) -> Result<Box<dyn OutputStream>, AudioError>;
}
pub trait OutputStream: Send { fn play(&mut self) -> Result<(), AudioError>; fn pause(&mut self) -> Result<(), AudioError>; }

/// Fetch remote media. Impl: yt-dlp subprocess.
pub trait MediaFetcher: Send + Sync {
    fn fetch(&self, url: &Url, dest: &Path, progress: ProgressFn, cancel: &CancelToken)
        -> Result<PathBuf, MediaError>;
}

/// Speech-to-text over PCM. Impls: humm-whisper (local), core::engines::openai (API).
pub trait TranscriptionEngine: Send {
    fn transcribe(&mut self, audio: &Pcm, opts: &EngineOptions, sink: &mut dyn SegmentSink,
                  cancel: &CancelToken) -> Result<(), EngineError>;
    fn capabilities(&self) -> EngineCaps; // word_timestamps, translate, vad, streaming-friendly
}

/// Incremental results: segments arrive as produced (progress + live UI + early export).
pub trait SegmentSink {
    fn on_segment(&mut self, seg: Segment);
    fn on_progress(&mut self, fraction: f32);
}

/// Live audio capture (M5 as-built, ADR-005/ADR-021; `humm-audio::CpalCapture`).
/// `device_id` (not a whole `&AudioDevice`, the original sketch's shape) is
/// the stable name-based id `AudioCapture::devices()` itself returns —
/// `None` requests the system default. The adapter guarantees `sink` always
/// sees 16 kHz mono f32 regardless of the device's native config (ADR-005's
/// fallback path), but the two operations split across two places: the RT
/// callback itself only **converts + downmixes** (never resamples — no
/// allocation/locking there); when the device's native rate isn't already
/// 16 kHz, that converted-but-still-device-rate audio lands in a
/// preallocated intermediate ring, and a **separate drain loop on the same
/// owner thread** (not the RT callback) pulls it through rubato to produce
/// the 16 kHz the sink sees. `meter` is the shared [`MeterSnapshot`] the caller reads from every
/// repaint; ownership of its three fields is split by writer, not shared:
/// the capture adapter writes only `current_rms` per RT callback, while
/// `average_rms`/`queue_len` are the session/chunker layer's own fields
/// (Buzz's two-signal split — `RecordingAmplitudeListener` vs. the
/// transcriber's own `queue_size_changed`/average signal — realized here as
/// two independent writers instead of one shared, ambiguous owner).
pub trait AudioCapture: Send {
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioError>;
    fn start(&mut self, device_id: Option<&str>, sink: rtrb::Producer<f32>,
              meter: Arc<MeterSnapshot>) -> Result<Box<dyn CaptureHandle>, AudioError>;
}
pub trait CaptureHandle: Send { fn stop(self: Box<Self>); }

/// Deferred-ML boundaries (designed now, implemented in late milestones — Q-3/Q-5):
pub trait SpeechIsolator: Send + Sync { /* demucs-equivalent vocal isolation */ }
pub trait Diarizer: Send + Sync { /* speaker segmentation + embeddings + clustering */ }

/// Cross-cutting:
pub trait SecretStore: Send + Sync { /* keyring; get/set/delete by well-known key */ }
pub trait Telemetry: Send + Sync { fn capture(&self, event: Event); } // PostHog | Noop
```

Adapter obligations (Standing Policy): startup probe (`ffmpeg -version`, `yt-dlp --version` → capability report, actionable errors, graceful feature degradation), machine-readable interfaces only (raw PCM pipes, `-print_format json`, `--print` templates), contract tests against golden inputs.

## 3. Concurrency model (per subsystem — different needs, different tools)

| Subsystem | Mechanism | Why |
|---|---|---|
| Inference (whisper-rs) | one dedicated OS thread per running task; cancellation via whisper.cpp abort callback checking `CancelToken` | blocking, CPU/GPU-bound, long-lived; async buys nothing |
| Task queue | `TaskRunner` thread owning a FIFO (parity: one transcription at a time); commands in via `flume` channel, `TaskEvent`s out via broadcast channel | serialized execution is a feature (GPU contention); channels replace Qt signals |
| HTTP + subprocess I/O (model downloads, yt-dlp, OpenAI API, translator, telemetry, update check) | single shared **tokio** multi-thread runtime owned by `CoreServices`; `tokio::process` for subprocess streaming | genuinely concurrent I/O; reqwest streams; kill-on-cancel |
| Audio capture (M5 as-built, ADR-021) | cpal's real-time callback thread (`CpalCapture`, `humm-audio`) → lock-free SPSC ring buffer (`rtrb`) → **one session thread per active recording**, owning `humm_core::live::session::run_session` (drains the ring into `live::chunker`'s accumulation queue, runs the silence-cut/skip state machine, calls the loaded `WhisperEngine` synchronously per chunk, emits `LiveEvent`s over an `mpsc` the GUI drains each frame). While the recording window is open but **not** recording, the same capture adapter instead feeds a lighter drain-and-discard thread (`humm-gui::recording::MeterOnlyDrain`, Buzz's `RecordingAmplitudeListener` parity — 1 s RMS accumulation, no chunker) — starting a real recording tears this down first (two capture streams on one device are avoided, spec §1.1) | never allocate/lock in the audio callback; the session thread is the one place chunker state, the engine, and export/upload/store writes all live, so a Stop's cooperative cancel has exactly one thread to wait on |
| Playback (M4 as-built, ADR-019) | cpal output stream whose **fill callback reads straight from the whole-file in-memory `PlaybackPcm`** (no feeder thread / no `rtrb` ring — there is no I/O-bound producer to decouple, so all playback logic runs inside the fill closure); position published via one packed `{generation, frame}` `AtomicU64` (the A/V master clock), seeks published by a single CAS so a lost seek is impossible; **varispeed by linear interpolation** over a fractional source cursor (sample-exact frame accounting the loop-wrap + A/V clock depend on — rubato's priming delay would offset the clock, so rubato is used only for device-rate conversion inside `CpalOutput`) | sample-accurate seek/loop without a media framework; pitch-shifting varispeed that actually works (Buzz's was mute) |
| GUI | eframe event loop; subscribes to `TaskEvent` receivers; `Context::request_repaint()` on event arrival | zero idle CPU, no polling |
| CLI | blocks on the same core APIs; renders events to stderr progress + `--json` lines | identical core path as GUI (parity by construction) |

**Event bus.** `TaskEvent { task_id, kind: Queued | DownloadProgress(f32) | Started | Segment(Segment) | Progress(f32) | Completed | Failed(ErrorInfo) | Canceled }` — the single source of truth consumed by GUI table, CLI renderer, exporters, and the store (which persists state transitions). This is the Rust-shaped replacement for Buzz's Qt-signal spaghetti and fixes Buzz's restart-loses-options bug structurally: the full `JobSpec` is part of the queue record and persisted with it.

**Cancellation** is cooperative and prompt everywhere: `CancelToken` (Arc<AtomicBool> + waker) checked by the whisper abort callback, subprocess kill for ffmpeg/yt-dlp/demucs, `tokio` abort for HTTP. Every path leaves the store consistent (terminal status written before ack).

## 4. Data model (core types)

```rust
pub struct Millis(pub i64);                       // newtype, no unit confusion
pub struct TranscriptionId(Uuid);
pub struct Segment { start: Millis, end: Millis, text: String, translation: String }

pub enum TaskKind { Transcribe, Translate }        // whisper task
pub enum Status { Queued, InProgress, Completed, Failed, Canceled, Skipped }
pub enum Source { FileImport, UrlImport, FolderWatch, LiveRecording }   // + live (Q-10 improvement)

pub struct ModelSpec { backend: Backend, size: ModelSize, custom: Option<CustomModel> }
pub enum Backend { Whisper /* local via whisper-rs */, OpenAiApi }
pub enum ModelSize { Tiny, TinyEn, …, LargeV3Turbo, Custom, Lumii }     // parity set

pub struct TranscriptionOptions {                  // persisted IN FULL with every task
    language: Option<Language>, task: TaskKind, model: ModelSpec,
    word_timestamps: bool, extract_speech: bool, initial_prompt: String,
    temperature: Vec<f32>,                         // parity default six-step tuple
    llm: Option<LlmTranslationOptions>,
}
pub struct JobSpec { input: Input /* Path | Url */, options: TranscriptionOptions,
                     output: OutputSpec /* formats set, dir, name template */ }
```

Exports (`core::export`): `write_txt/srt/vtt(&[Segment], key: TextOrTranslation, w: impl Write)` — pure functions, snapshot-tested against Buzz's golden strings (`tests/transcriber/transcriber_test.py` fixtures ported verbatim). Filename templating: same six tokens, same datetime format.

## 5. State & persistence

- **DB** (humm-store, rusqlite bundled): humm-owned schema, superset of Buzz's (`transcription` + `transcription_segment` columns preserved for import) **plus** `options_json` (full `TranscriptionOptions` — fixes the restart trap) and a `kind` column admitting live-session records (Q-10). WAL mode. All access behind `TranscriptionRepository`; the queue's status transitions are the only writers of `status`.
- **Buzz import** (one-time, Q-6): open `~/.../Buzz.sqlite` read-only if present on first run, map rows/segments, mark imported. Import failures are non-fatal + logged.
- **Settings**: typed serde struct → TOML at platform config dir; atomic write (temp+rename); defaults in code (single source). No QSettings migration (approved).
- **Secrets**: `keyring` crate behind `SecretStore` ("OpenAI API key" well-known entry, parity service name semantics — humm's own service string).
- **Model cache**: `<data_dir>/humm/models/` (`HUMM_MODEL_ROOT` override), `.humm_complete` sentinels, resume + SHA256 where upstream provides digests (parity behaviors F-043).

## 6. Error handling

- Library crates: `thiserror` enums per crate (`MediaError`, `EngineError`, `StoreError`, `AudioError`, `CoreError` composing them). Every variant carries actionable context ("ffmpeg not found — install it or set HUMM_FFMPEG"). No `unwrap`/`expect` outside tests/provably-impossible states (documented).
- Binaries: `anyhow` at the edge; CLI maps errors → exit codes + stderr; GUI maps → user-facing error presentation (task row status, non-modal toasts, detail dialog with log pointer).
- Missing external binary = degraded feature, not a crash: `Capabilities` struct computed at startup (ffmpeg? yt-dlp? metal?) gates UI affordances with explanatory tooltips (Standing Policy rule 6).

## 7. GUI architecture (humm-gui, egui/eframe — ADR-006)

- One `App` struct owning: `CoreServices` handle, `TaskEvent` receiver, per-window view-model structs (`MainView`, `ViewerView(id)`, `RecordingView`, `PreferencesView`, `PresentationView`). Multi-window via egui **deferred viewports** (real OS windows).
- **Viewer windows (M4 as-built):** each open transcript is a **deferred** viewport keyed `ViewportId::from_hash_of(("viewer", id))`, its state `Arc<Mutex<ViewerVm>>` (deferred viewports require `Send + Sync` per-window state — the tradeoff paid over immediate viewports because playback windows animate at up to 60 Hz and deferred viewports give per-window repaint isolation, unlike the M3 immediate About/Preferences windows). `HummApp.viewers: BTreeMap<TranscriptionId, Arc<Mutex<ViewerVm>>>`; a re-open of the same transcript *focuses* the live window rather than duplicating it (ledger #1). Resizer windows follow the same deferred-viewport shape (`resizers` map). The resizer/merge worker emits an internal `AppEvent::TranscriptionsUpdated` that closes the originating viewer + resizer through the geometry-persisting close path.
- Immediate-mode discipline: view-models hold UI state only; domain state lives in core/store; events mutate view-models; `request_repaint` on event arrival → idle CPU ≈ 0.
- Segment table: `egui_extras` virtualized rows (100k-segment transcripts stay 60 fps — decisive perf win over Buzz's QTableView).
- Video: ffmpeg rawvideo stream → `egui::TextureHandle` upload per frame; audio clock is master, frames sync to it (same strategy as Buzz, minus Python overhead).
- Audio meter: custom painted widget fed by an atomic RMS snapshot from the capture thread.
- i18n: `fluent` catalogs, English first; string keys stable from day one so locale ports are mechanical.

## 8. CLI architecture (humm-cli, clap — ADR-007)

Headless by construction (deviation ADR-007; Buzz's CLI drives a hidden GUI). `humm add …` preserves Buzz flag spelling verbatim; adds `--json` (event-per-line), real per-run exit codes (0 all ok / 1 usage / 2 some tasks failed), and stderr progress bars. Same `CoreServices` + queue as the GUI — parity by shared implementation, verified by CLI integration tests invoking the produced binary.

## 9. Performance posture (value #2)

- Hot paths owned by native code: whisper.cpp (Metal/CoreML), ffmpeg decode, rubato SIMD resampling; zero-copy PCM (`Pcm(Arc<[f32]>)`) from decode → engine.
- Startup: no Python/Qt/torch import tax — target cold-launch < 300 ms to interactive window (vs seconds for Buzz).
- Criterion suite from M1 (decode, resample, e2e file transcription, export) tracked across milestones; regression = blocking failure per mission rules.
- Benchmarks that matter get recorded in ADRs (whisper-rs vs candle in M1; subprocess-decode overhead vs libav claim if ever challenged).
