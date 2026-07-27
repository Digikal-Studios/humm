# M8 — Heavy ML + mission closing: design

Milestone 8 (FINAL): F-027 speech isolation, F-087 speaker identification,
F-112 noise reduction, F-114 enhanced language detection, plus the closing
sweep (CJK font, 11 remaining locales, F-044 windowed pass, store/live
hardening) — ending with the all-rows-verified parity sweep and the
mission-closing Buzz-vs-humm benchmark report.

Buzz ground truth re-read for this design (implement from source, not
memory): `file_transcriber_queue_worker.py` (whole file — the demucs
pre-pass at `:55-108/:151-206/:276-350`, hook dispatch `:208-229`, cancel
`:369-381`, cleanup `:406-416`), `plugins/deep_filter_net/plugin.py`,
`plugins/enhanced_language_detection/plugin.py`, `transcriber/whisper_cpp.py`
`detect_language` (`:403+`), `transcriber/transcriber.py:228-265`
(`get_output_file_path`, `_speech` strip), `plugins/manager.py:107-119`
(`run_before_transcription` path-swap contract),
`widgets/transcription_viewer/speaker_identification_widget.py` (whole file).
The four ML submodules (`demucs_repo`, `whisper_diarization`,
`ctc_forced_aligner`, `deepmultilingualpunctuation`) are **not initialized**
in the Buzz checkout (`git submodule status` shows all `-`); their behavior is
taken from Buzz's own call sites plus upstream documentation, which suffices
because ADR-011 fixes F-027/F-087 at outcome-level parity, not
implementation parity.

---

## 0. Scope

**In:** the four ML rows (F-027/F-087/F-112/F-114) with the
`before_transcription` queue seam M7 deferred; the closing-sweep debt from
`humm-m8-tickets`: CJK font (F-008 gate) + the Preferences tab-strip
overflow ride-along, the 11 remaining locale catalogs, store
mutex-poisoning recovery, live-API tolerate-N-consecutive-chunk-failures
(L17 refinement), the L15 `ApiLiveEngineSession` provenance test; then the
acceptance phase: F-044 windowed interactive pass, outcome-level diffs vs
Buzz on fixture media, graceful-absence checks, transcription-path bench
vs a same-day M7 checkout, gui-manual v6, the FINAL parity-matrix sweep
(target: all rows `verified`, environment-blocked exceptions named), and
the mission-closing full benchmark report vs Buzz itself (startup, e2e,
memory).

**Out / honest exceptions:** the F-031/F-111 real-chat contracts remain
env-gated — probed again at sweep time (`HUMM_LLM_TEST_BASE`, ollama,
LM Studio); as of design day none exists on this machine (probe recorded
2026-07-26: env unset, no ollama binary, no LM Studio app). If still
absent at the sweep they are named exceptions, never fudged. Buzz's
third-party plugin *mechanism* stays replaced per ADR-010 (no new work).
Carried M6 cosmetics ("Model:" label placement, encode-error prefix
wording, eye-toggle mid-edit commit) are close-or-carry: attempted only if
trivially cheap during the sweep, otherwise named in the final report's
remaining-debt list.

---

## 1. Strategy re-evaluation (the mandated first step) — evidence and verdicts

ADR-011 deferred three strategies to an M8-entry re-evaluation. Evidence
gathered 2026-07-26:

**`ort` release status.** Still pre-release: `2.0.0-rc.12` (2026-03-05);
`max_stable_version: null`; every 1.x yanked. Cadence rc.10 (2025-06) →
rc.11 (2026-01) → rc.12 (2026-03); 14.0M downloads; actively maintained
(pykeio/ort). Verdict: *usable but only with an exact `=2.0.0-rc.12` pin
and confined to one adapter crate*; anything that can avoid ort should.

**F-027 (htdemucs-ONNX arm).** The ONNX arm fails viability before any
benchmark:
1. No canonical hosted htdemucs ONNX artifact exists. The first working
   full export (StemSplit `demucs-onnx`, 2026-05) *requires local torch at
   export time*; earlier attempts (sevagh/demucs.onnx, Mixxx GSoC 2025)
   confirm the export blockers (complex64 STFT, `aten::_native_multi_head_attention`).
2. Every working export moves STFT/iSTFT **outside** the graph — the Rust
   side would have to reimplement demucs's STFT, overlap-add segmenting,
   shift trick, and normalization numerics; a large, quality-risky port.
3. Upstream `facebookresearch/demucs` is **archived** (2025-01-01;
   low-maintenance fork `adefossez/demucs`): the model is frozen and no
   upstream ONNX publication is coming.
4. ort itself is still rc (above).

Verdict: **demucs subprocess** — ADR-011's sketched fallback ("else
optional `demucs` subprocess with graceful absence"). This is also the
*closest shape to Buzz itself*, which runs demucs as Python in a killable
child process; humm runs the `demucs` CLI as a killable subprocess and
degrades gracefully when the binary is absent. §3.

**F-087 (ort diarization arm).** Viable, with strong precedent: the exact
ADR-011 pipeline (segmentation ONNX + embedding ONNX + clustering in Rust)
exists as maintained Rust crates — **speakrs 0.5** (crates.io, Apache-2.0,
full pyannote `community-1`-style pipeline: segmentation, powerset decode,
overlap-add, binarization, embeddings, VBx clustering; ort *or native
CoreML*; models from HF `avencera/speakrs-models` with a documented
offline dir override) and **pyannote-rs 0.3.x** (MIT; segmentation-3.0 +
wespeaker-voxceleb-resnet34-LM + cosine clustering). Verdict: **adopt the
ort arm via one of these crates behind humm's `Diarizer` trait** in an
isolated adapter crate; a validation spike picks speakrs (primary) vs
pyannote-rs (fallback) empirically; in-house reimplementation only if both
fail the spike. §6.

**F-112 (`df` crates).** The crates.io `deep_filter` release is stale
(0.2.5, 2022 — DeepFilterNet1-era), **but** the repo (Rikorose/
DeepFilterNet, MIT/Apache-2.0) is releasing: latest tag **v0.5.6
(2024-08-31)**, `libDF` package `deep_filter` 0.5.x with a `tract` feature
(tract-onnx ^0.19.4 — pure-Rust inference; **correction, Task 2/19**: this
line originally read `^0.21.4`, which is what the repo's *default branch*
declares — the pinned v0.5.6 tag itself declares `^0.19.4`, and that is what
humm builds against. DECISIONS.md's ADR-011 addendum was corrected when the
spike found it; this sentence is corrected here) and a `default-model` feature
embedding the DeepFilterNet3 model. Verdict: **git-pin `deep_filter` at
the v0.5.6 rev with `tract` + `default-model`** — pure-Rust inference, no
ort, no model download, no absence path. If the pinned build fails the
spike, fallback: vendor `libDF`'s tract path into the adapter crate. §4.

**ADR updates:** ADR-011 gets this evidence + verdicts appended (same
commit as this spec). The demucs subprocess is a new non-Rust boundary →
new **ADR-023**; ort + the diarization model artifacts → new **ADR-024**;
the deep_filter git-pin → recorded in the ADR-011 addendum (pure Rust, no
new non-Rust boundary; the embedded ONNX bytes ride the pinned crate).

---

## 2. New boundary traits and adapter crates

Core (`humm-core/src/boundary.rs` or a new `ml.rs` module) defines the
deferred-ML traits sketched in ARCHITECTURE §2, now concrete:

```rust
/// F-027. Impl: humm-media `DemucsIsolator` (subprocess).
pub trait SpeechIsolator: Send + Sync {
    /// Writes `<input_stem>_speech.mp3` next to `input`; returns its path.
    fn isolate(&self, input: &Path, cancel: &CancelToken)
        -> Result<PathBuf, MediaError>;
    /// Cheap availability probe (binary present?) for capability gating.
    fn available(&self) -> bool;
}

/// F-112. Impl: humm-dfn `DfNoiseReducer` (deep_filter/tract, model embedded).
pub trait NoiseReducer: Send + Sync {
    /// Writes `<input_stem>_DeepFilterNet3.wav` next to `input`; returns its path.
    fn reduce(&self, input: &Path, cancel: &CancelToken)
        -> Result<PathBuf, MediaError>;
}

/// F-114. Impl: humm-whisper `WhisperLangDetector`.
pub trait LanguageDetector: Send + Sync {
    /// Detects the spoken language of `input` using `model` (a ggml path).
    fn detect(&self, input: &Path, model: &Path, cancel: &CancelToken)
        -> Result<Option<Language>, EngineError>;
}

/// F-087. Impl: humm-diarize `OrtDiarizer` (speakrs/pyannote-rs).
pub trait Diarizer: Send + Sync {
    fn diarize(&self, input: &Path, progress: &mut dyn FnMut(DiarizeProgress),
               cancel: &CancelToken) -> Result<Vec<SpeakerTurn>, DiarizeError>;
}
pub struct SpeakerTurn { pub start: Millis, pub end: Millis, pub speaker: usize }
```

**As-built correction (Tasks 4/9, recorded in the Task 19 sweep).** The two
sketches above that take a *file path* actually take already-decoded audio,
because both call sites already hold a decoded `Pcm` and re-decoding would
double the ffmpeg cost for no gain:

```rust
fn detect(&self, audio: &Pcm, model: &Path, cancel: &CancelToken)      // F-114
    -> Result<Option<Language>, EngineError>;
fn diarize(&self, audio: &Pcm, progress: &mut dyn FnMut(DiarizeProgress),
           cancel: &CancelToken) -> Result<Vec<SpeakerTurn>, DiarizeError>; // F-087
```

`SpeechIsolator`/`NoiseReducer` are as sketched (`&Path` → `PathBuf`): they
are file-to-file transforms whose output *is* a file the decoder then reads.
`SpeakerTurn` additionally carries two documented contracts the sketch did
not state: implementations must return turns sorted by `start`, and turns
from *different* speakers may overlap in time (`humm_core::diarize`).

Crates: `DemucsIsolator` joins **humm-media** (the subprocess adapter crate
— same probe/kill conventions as ffmpeg/yt-dlp). **humm-dfn** (new) holds
the deep_filter dependency. **humm-diarize** (new) holds ort/speakrs.
`WhisperLangDetector` joins **humm-whisper** (whisper-rs is already there).
Core depends on none of them (dependency rule intact); `RunnerDeps` grows

```rust
pub isolator: Option<Arc<dyn SpeechIsolator>>,
pub reducer:  Option<Arc<dyn NoiseReducer>>,
pub lang_detector: Option<Arc<dyn LanguageDetector>>,
```

(`Diarizer` is viewer-side, not queue-side — §6.) `None` = seam never
runs, the `postproc: None` precedent.

---

## 3. F-027 — Speech isolation (demucs subprocess)

**Subprocess contract** (stable, long-established CLI flags only):
`demucs --two-stems=vocals -n htdemucs --mp3 -o <scratch> --filename "{stem}.{ext}" <input>`
→ `<scratch>/htdemucs/vocals.mp3`, renamed to `<input_stem>_speech.mp3`
next to the input (Buzz's naming, `file_transcriber_queue_worker.py:191`).
Binary discovery: `HUMM_DEMUCS` env override, else `PATH`. No `-d` flag:
demucs's own default (cuda-if-available, else cpu) is exactly Buzz's device
choice (`:183-188`). Progress: demucs emits only human-oriented tqdm on
stderr — Standing Policy rule 3 forbids parsing it, so the phase is
**indeterminate** (deviation from Buzz's fraction, which came from the
in-process Python API callback; ledger #1). Cancellation: kill the child —
SIGTERM, 5 s grace, SIGKILL (Buzz's `terminate→kill` parity, `:339-350`).

**Pipeline placement** (`Runner::run_job`): after `check_skip`, before
`ensure_artifacts`:

1. check_skip runs on the **original** input. Buzz's accidental order
   (extract first, then check_skip against the `_speech` path — which can
   never match the DB) makes skip+extract mutually broken; humm keeps its
   existing skip-first order. Deviation, ledger #2.
2. If `options.extract_speech` and an isolator is wired: emit the new
   phase event, run `isolate`, and use the returned path as the **decode
   input only** — `job.input` (export naming, DB row, viewer) stays the
   original. Buzz preserves the original in the DB too (insert happens
   before extraction; the swap is in-memory), so this is parity, not
   deviation.
3. `no-audio` degrade: if demucs exits non-zero and `prober.probe(input)`
   says `!has_audio`, log + proceed with the original file (Buzz's
   `IndexError → "no_audio"` arm, `:80-83`). Any other failure fails the
   task with an actionable message naming demucs and the
   first-run-model-download possibility (Buzz's error string semantics,
   `:198`).
4. Cleanup: `_speech.mp3` is deleted on **every** terminal path
   (completed/failed/canceled). Buzz deletes only on completion
   (`:411-416`) and leaks it on error/cancel; humm cleans up always
   (improvement, ledger #3).

**Export naming:** `output_file_path` additionally ports Buzz's
`_speech`-suffix strip (`transcriber.py:238-240`) — byte-parity for the
edge where a user's own file is literally named `*_speech.*`. Folder-watch
already skips `_speech.mp3` (M7-ported skip rules; re-verified by test).

**Events/UI:** new `TaskEventKind::PhaseChanged { phase: TaskPhase }` with
`TaskPhase::IsolatingSpeech` (extensible; additive `--json`
`{"event":"phase",...}` documented in HUMM_SURFACE.md); GUI `Phase` gains
the variant, status text "isolating speech" (+4 ftl catalogs — now 15).
CLI: `-e` goes live (validate-fail fast with an actionable install hint
when demucs is absent); GUI options sheet: the checkbox goes live, bound
to `TranscriptionOptions.extract_speech` (new field, `#[serde(default)]`
for pre-M8 `options_json` rows) + `FileTranscriberSettings` persistence;
disabled-with-tooltip only when demucs is genuinely absent (Capabilities
pattern). Store: the schema's inert `extract_speech` column starts being
written from the real option.

**Unhappy paths:** binary missing (checkbox disabled + tooltip; CLI
fail-fast), model download inside demucs fails (actionable task failure),
silent video (degrade to original), cancel mid-separation (child killed,
prompt ack), disk full (subprocess failure surfaces actionably).

**Verification** (`contract + diff`): contract tests (`#[ignore]`,
demucs via `uvx`/pip, sandbox-lifted): vocals file produced, naming, kill-
on-cancel, absence degrade. Differential vs Buzz mechanism: same demucs
model family (htdemucs), `_speech.mp3` naming byte-equal, export names
strip the suffix identically. Outcome diff on fixture media at acceptance:
speech isolated (output audibly voice-dominant / spectral check), task
completes, exports named from the original stem.

---

## 4. F-112 — Noise reduction (DeepFilterNet, native)

**Dependency:** `deep_filter` git-pinned to the v0.5.6 rev, features
`["tract", "default-model"]` — pure-Rust tract inference, DeepFilterNet3
model embedded. New adapter-style crate **humm-dfn** implements
`NoiseReducer`. Validation spike up front: build + denoise a fixture; if
the pinned rev fails to build on the current toolchain, fallback = vendor
libDF's tract path (MIT/Apache-2.0) into humm-dfn.

**Behavior** (`plugin.py` parity): decode input to DFN's sample rate
(48 kHz mono — via existing ffmpeg decode path with a rate parameter, or
libDF's own loader if the spike shows it's suitable), enhance, write
`<input_stem>_DeepFilterNet3.wav` next to the input (`plugin.py:48`),
swap the **decode input** for transcription. Failure → log + proceed with
the original audio (Buzz returns `None` on exception, `:58-60`) — noise
reduction never fails a task.

**Cleanup** (`plugin.py:62-72` parity of intent): the
`_DeepFilterNet3.wav` is deleted on **every** terminal path unless
`keep_denoised_file` — the same all-terminals rule as `_speech.mp3`
(ledger #3; Buzz deletes only in its on_complete hook and leaks the wav
on failure/cancel). `keep_denoised_file = true` leaves it next to the
input as a user artifact (Buzz's `config_fields` semantics).

**Deviation (ledger #4):** Buzz's in-place `task.file_path` swap leaks
`_DeepFilterNet3` into export names and leaves the DB row pointing at a
file its own default config then deletes (viewer playback breaks). humm's
decode-input-only swap keeps exports/DB/viewer on the original file;
`keep_denoised_file = true` still delivers the wav next to the input.

**Config:** `PostprocSettings` gains `noise_reduction:
NoiseReductionSettings { enabled: bool /* default false, Buzz plugins
default-disabled */, keep_denoised_file: bool /* default false */ }`; one
new CollapsingHeader section in Preferences → Post-processing (ADR-010's
config home), Buzz's `config_fields` labels ported verbatim.

**Ordering with F-027:** isolation first, then noise reduction (chained on
the isolated file — Buzz's plugin order is user-configurable; humm fixes
isolation → reduce → detect and documents it, the ADR-010 fixed-order
convention; ledger #5).

**Verification** (`per-strategy` → `unit + contract`): unit (naming,
cleanup, settings gating, failure-degrade); contract (`#[ignore]`): real
model denoises a noisy fixture — assert output exists, same duration
±tolerance, and RMS noise-floor reduction on a silence-plus-noise band;
graceful failure on a corrupt input.

---

## 5. F-114 — Enhanced language detection (native)

**Mechanism:** `WhisperLangDetector` (humm-whisper) — whisper-rs 0.16 has
the standalone API: create context on the chosen ggml model, create state,
`pcm_to_mel`, `WhisperState::lang_detect(0, threads)` →
`(lang_id, probs)`, `whisper_rs::get_lang_str(id)` →
`humm_core::Language::parse`. Decode via the existing `AudioDecoder`
(16 kHz mono — Buzz converts non-native formats through ffmpeg the same
way, `whisper_cpp.py:415-427`). Context/state are created per call and
dropped (Buzz spawns a fresh `whisper-cli --detect-language` process per
detection — same cost shape).

**Model selection** (`plugin.py:117-169` parity): largest *downloaded*
ggml size wins (rank by Buzz's `WHISPER_MODEL_SIZES` order; `Custom` and
`Lumii` excluded, `:131`); if none and `download_tiny_if_missing`
(default true): download tiny via the existing `ModelDownloader`
(progress → `ModelDownloadProgress` events); download failure → log +
skip detection (Buzz `:164-167`).

**Hook semantics** (`plugin.py:59-115` parity): runs in the
before-transcription chain when enabled AND the task's language is unset
**or explicitly English** — Buzz's quirk (`"en"` is its no-selection
default, so an explicit English pick still triggers detection and can be
overridden by it, `:64-72`). Ported faithfully + ledgered (#6): the
feature is default-off, and outcome parity beats second-guessing. Runs for
all backends including OpenAI-API tasks (Buzz does; detection is local
whisper regardless). Detected language is written to (a) the job's
effective `EngineOptions.language`, (b) the `{{ language }}` export-name
token (threaded through `output_file_path`), and (c) the store row
(`update_transcription_language`-equivalent store method — added if
absent) so later viewer exports see it (`plugin.py:104-112`). Detection
returning nothing → proceed on auto (never fails the task).

**Config:** `PostprocSettings.language_detection:
LangDetectSettings { enabled: bool /* default false */,
download_tiny_if_missing: bool /* default true */ }` + prefs section with
Buzz's labels.

**The seam (M7 spec §0 debt):** `run_job` gains one fixed-order
pre-transcription block — isolation (F-027, option-gated) → noise
reduction (F-112) → language detection (F-114) — each hook
`catch_unwind`-contained exactly like the on_complete chain (a panicking
pre-hook logs, is skipped, and the task proceeds on the original
audio/language), mirroring `run_on_complete_chain`'s shape.

**As-built correction (Task 4, recorded in the Task 19 sweep): isolation is
deliberately exempt from that containment.** Reduction (F-112) and detection
(F-114) *are* individually `catch_unwind`-contained and degrade exactly as
written above, because Buzz's own plugin semantics for them are
degrade-on-exception (`plugin.py:58-60`, `:93-95`). Isolation is not: Buzz
fails the task when demucs raises anything other than its `no_audio`
`IndexError`, so humm keeps that failure semantic and lets an isolation
panic reach `run_task`'s existing outer `catch_unwind` — which fails the
task with the panic message instead of silently transcribing audio the user
asked to have cleaned. Same-shaped code, deliberately different blast radius;
the comment at `queue.rs`'s isolation block states this at the call site.

**Verification** (`per-strategy` → `unit + contract`): unit (gating
matrix incl. the en-quirk, model-selection ranking with fake registry
state, write-back threading); contract (`#[ignore]`, real tiny model):
`whisper-french.mp3` detects `fr`, export name carries `fr`, store row
updated.

---

## 6. F-087 — Speaker identification (ort diarization + J-7 two-step UI)

**Adapter:** new crate **humm-diarize** implementing `Diarizer` via
**speakrs** (primary; exact-pin) or **pyannote-rs** (fallback) — spike
decides on: builds with our toolchain, runs the 2-speaker fixture sanely
(≥2 speakers, boundaries within ±2 s of the known splice points),
CoreML/CPU perf sanity. ort pinned `=2.0.0-rc.12` (transitively or
directly), confined to this crate (ADR-024). Buzz's NeMo/MSDD +
ctc-forced-aligner + punctuation stack is **not** ported — ADR-011 fixed
outcome-level parity: speakers detected, nameable, mergeable; diffable on
speaker-count/boundary sanity, never identical labels.

**Models:** ensured through the F-043 pattern — `ensure_artifact` (resume,
sentinel, per-path lock) into `model_root()/diarization/`, from the pinned
HF URLs of whichever crate the spike selects; the crate is pointed at that
directory (offline-dir override). Graceful absence: the Speaker-ID window
offers the download with determinate progress + cancel before Step 1 runs
(J-7 "progress states with cancel"); no network + no models → actionable
message, never a crash — the `ensure_vad` degrade philosophy.

**Turns → outcome mapping** (core, `humm_core::diarize` module, pure +
unit-tested): each transcript segment is assigned the speaker with maximal
temporal overlap (no overlap → nearest turn); speakers are relabeled
"Speaker 0..N" in order of first appearance (Buzz's label shape). Merge
option: consecutive same-speaker segments merge (end extended, texts
joined with a space) — `speaker_identification_widget.py:743-773` parity.
Saved segments read `"{name}: {text}"` (`:757`), timestamps in ms. No
schema change: Buzz encodes speakers into segment text, so does humm.

**Two-step window (J-7, UX_SPEC):** new deferred viewport keyed
`("speakers", id)` following the resizer precedent (`open_speakers`,
`speakers: BTreeMap<TranscriptionId, Arc<Mutex<SpeakerIdVm>>>`, repaint
fan-out, viewer-close cleanup). Opened by the viewer's **Identify
Speakers** button (stub goes live; "lands in M8" tooltip retired). Buzz's
hide-on-Intel-macOS gate is not ported — humm builds/targets Apple
Silicon and the gate guards a torch limitation humm doesn't have.
- Gate: audio file must exist (`:514-518` parity — "Audio file not found",
  Identify disabled).
- **Step 1 Identify:** worker thread — ensure models (progress) → diarize
  (progress states as labeled stages with a progress bar, Buzz's "N/8 …"
  shape adapted to humm's real stages) → map to segments. Cancel button:
  cooperative `CancelToken`, checked between stages and inside the
  windowed diarization loop; "Cancelled" state resets the UI (`:616-624`).
  Errors reset Step 1 with the message shown (`:642-646`).
- **Step 2 Name speakers:** one row per unique speaker — editable name
  prefilled "Speaker N" + **Play sample**: a random segment of that
  speaker via the existing humm-audio playback engine, stopped after
  min(10 s, sample length). Buzz arms its stop-timer with the segment's
  *absolute end time* capped at 10 s (`:725`) — an accident that can cut
  samples short or overrun into the next speaker; humm plays up to 10 s of
  the actual sample (outcome fix, ledger #7).
- **Save:** `store.copy_transcription(source)` +
  `mark_replaced_completed(new_id, labeled_segments)` →
  `AppEvent::TranscriptionsUpdated` (main table refresh) → window closes.
  Buzz parity: results land in a **new** transcription row; the original
  is untouched (`:784-800`).

**Verification** (`contract + diff`): unit (overlap mapping, relabeling,
merge, name substitution); kittest (window states: gate, progress,
step-2 rows from an injected result, save path through a scripted
Diarizer double); contract (`#[ignore]`, real models, sandbox-lifted):
synthesized 2-speaker fixture (ffmpeg splice of `audio-long.mp3` +
`whisper-french.mp3` voices) → ≥2 speakers, boundary within ±2 s of the
splice; outcome diff at acceptance per ADR-011 (count/boundary sanity vs
the fixture's construction, explicitly not label-equality vs Buzz).

---

## 7. Closing sweep

**7.1 CJK font (first — user-visible today).** Bundle **Noto Sans SC**
(static Regular, OFL, ~10 MB) into humm-gui assets; `HummApp::new` sets
`FontDefinitions` appending it as fallback to both Proportional and
Monospace families. Covers zh_CN, zh_TW, ja_JP han + kana + CJK
punctuation (Buzz's default `。？！` under any locale). Documented
divergence: TC/JP-preferred han variants render with SC-preferred glyphs
(same codepoints; Buzz-on-Qt used region-correct system fonts) — ledger
#8. Verification: unit glyph-coverage test (ab_glyph glyph lookup ≠ 0 for
sample strings from each CJK catalog) + windowed screenshot in gui-manual
v6 (gates F-008 `verified`). Ride-along: fix the M7-noted tab-strip
overflow at font_size 24 (wrap the Preferences tab strip).

**7.2 Eleven locales (mechanical, de/es/zh precedent).** ca_ES, da_DK, nl,
it_IT, ja_JP, lv_LV, pl_PL, pt_BR, ru, uk_UA, zh_TW (**corrected, Task
13/19**: this list was originally written alphabetized, which put `nl` after
`lv_LV`; Buzz's real `ui_locales` order — the order the registry and the
combo actually use — is `en_US, ca_ES, da_DK, nl, de_DE, es_ES, it_IT,
ja_JP, lv_LV, pl_PL, pt_BR, ru, uk_UA, zh_CN, zh_TW`, so `nl` sits third,
right after `da_DK`. Code, catalogs and `EXTERNAL_SURFACE.md` follow the
real order and an order-pin test enforces it) — each: one
`humm.ftl` (full 340+ keys), one `AVAILABLE_LOCALES` entry, Buzz-`.po`
glossary reuse where the string matches (~67 % precedent), capability-
parity divergences documented in catalog-header comments, per-locale
completeness tests auto-cover. Registry order stays Buzz's `ui_locales`
order. (Buzz ships corrupted/doubled msgstrs in some catalogs — corrected
as in M7, documented per locale.)

**7.3 Store mutex-poisoning recovery.** `SqliteStore::lock` switches from
poison→`StoreError::Query` to the workspace `lock_recover` convention
(`unwrap_or_else(|p| p.into_inner())` — rusqlite `Connection` is safe to
reuse after a panicked holder; an interrupted transaction rolls back on
drop). A test poisons the mutex via a caught panic and proves later writes
succeed.

**7.4 Live-API tolerate-N (L17 refinement).** `run_session`'s engine-error
arm tolerates **3 consecutive** chunk failures: each failure emits a new
non-terminal `LiveEvent::ChunkSkipped { detail }` (chunk's text dropped —
never Buzz's fake `"Error: …"` transcript text), a success resets the
counter, the 3rd consecutive failure emits the existing terminal
`Error` + stop. N=3 rationale: one transient 429/500 no longer kills a
session Buzz would survive, while a sustained outage (≥15 s at 5 s chunks)
still fails loud instead of silently eating audio. GUI/CLI surfaces show a
non-fatal warning per skip. Ledger #9 (supersedes L17's fail-on-first).

**7.5 L15 provenance test.** A mock-server test at the
`ApiLiveEngineSession` seam: scripted local endpoint asserts the request
carries the session's `task` (endpoint routing transcriptions vs
translations) and `language` form field exactly as `RecordingOptions`
specified. The test lives in **humm-gui** (it exercises the GUI-side
`ApiLiveEngineSession::transcribe_live` field mapping — the hop L15
names) — humm-gui's first TCP-bind test; the documented sandbox baseline
becomes 67 humm-core + 1 humm-cli + 1 humm-gui.

**7.6 F-044 windowed interactive pass** (acceptance phase, gui-manual v6):
Delete + confirm modal, Show file location, a live download with cancel,
speed-badge tooltip, Lumii row under lv_LV — the never-windowed set.

---

## 8. Cross-cutting surface changes

- `TaskEventKind::PhaseChanged { phase: TaskPhase }` (additive; CLI
  `--json` documented in HUMM_SURFACE.md §3; exhaustive matches in
  `run.rs`/`viewmodel.rs` updated).
- `TranscriptionOptions.extract_speech: bool` (`#[serde(default)]`;
  options_json backward-compat; store column now written; Buzz-import
  already maps it).
- `PostprocSettings` + prefs Post-processing tab gain `noise_reduction`
  and `language_detection` sections (manual `Default` where any field
  defaults true).
- New ftl keys ×15 catalogs by milestone end; `all_keys_used()` extended
  in the same commits; three "arrives later" stubs retired
  (`options-extract-speech-tooltip`, `viewer-identify-speakers-tooltip`,
  CLI `-e` usage-fail) with their kittest/cli-int assertions updated.
- New crates humm-dfn, humm-diarize join workspace lint/profile settings;
  clippy/fmt/test gates unchanged.
- Wiring: the GUI wires all three `RunnerDeps` seams; the CLI wires only
  the isolator (`-e`) — the F-112/F-114 pre-hooks follow the existing
  `postproc: None` CLI stance (GUI-configured hooks are GUI-only; the
  M7-documented deviation extends to the pre-chain — Buzz's CLI runs
  plugins only because it drives a hidden GUI).

## 9. Verification map (matrix methods)

| Row | Method | This design's realization |
|---|---|---|
| F-027 | contract + diff | §3: demucs contract tests (`#[ignore]`), naming/strip diffs vs Buzz sources, outcome diff on fixture |
| F-087 | contract + diff | §6: real-model contract on 2-speaker fixture; outcome diff = count/boundary sanity (ADR-011) |
| F-112 | unit + contract | §4 (matrix method updated from `per-strategy` in the same commit) |
| F-114 | unit + contract | §5 (ditto) |
| F-008 | unit + gui-manual | §7.1 font + §7.2 catalogs; windowed CJK screenshot in v6 |
| F-044 | gui-manual | §7.6 windowed pass |
| F-002 | cli-int | `-e` goes live; cli-int updated |
| F-031/F-111 | contract | env-gated; probe at sweep; honest exception if still absent |

Environment: sandboxed TCP-bind baseline 67 humm-core + 1 humm-cli grows
only with documented cause (§7.5 adds humm-gui's first — 67+1+1);
GPU/Metal/windowed/demucs/
diarization-model runs are sandbox-lifted; cargo-home seeding per the M7
recipe; ort build-time binary fetch needs one lifted build (recorded).

## 10. Acceptance & mission close

1. Outcome-level diffs vs Buzz on fixture media (speech isolated, speakers
   found/nameable/mergeable, noise reduced, languages detected).
2. Graceful-absence checks: no demucs, no diarization models, corrupt
   inputs — degrade with actionable messages, everything else works.
3. Transcription-path bench vs a **same-day M7 checkout** (interleaved
   pairs, `docs/benchmarks/M7.md` methodology) — no regression, or it
   blocks.
4. gui-manual v6 (`docs/gui-manual/M8.md`): new-feature checks + §7.6 +
   CJK screenshot + carried-deferred items where pointer automation
   reaches them.
5. FINAL parity sweep: every row to `verified` or a named
   environment-blocked exception (candidates: F-031/F-111 real-chat).
6. Mission-closing benchmark vs **Buzz itself**: one-time sandbox-lifted
   `uv sync` in the Buzz checkout (torch/PyQt6, ~10 GB of the 274 GiB
   free); then startup-to-interactive, e2e file transcription (same
   fixture + model size), peak memory — humm vs Buzz, per docs/MISSION.md. If
   Buzz's env cannot be built, the report says so explicitly and ships
   with whatever subset ran — never fabricated numbers.
7. Milestone report + mission-completion summary: final matrix state,
   Buzz-vs-humm numbers, the complete deviations ledger, remaining debt.

## 11. Deviations ledger (M8)

1. Isolation progress is indeterminate (subprocess tqdm is not
   machine-readable; Buzz had an in-process callback).
2. check_skip runs before isolation (Buzz's order breaks skip+extract).
3. `_speech.mp3` cleaned up on all terminal paths (Buzz leaks on
   error/cancel).
4. Noise reduction never renames exports/DB rows (Buzz's path swap leaks
   `_DeepFilterNet3` into export names and breaks viewer playback after
   cleanup).
5. Pre-hook order fixed isolation → reduce → detect (Buzz: user-orderable
   plugins).
6. F-114's explicit-English-still-detects quirk ported faithfully.
7. Play-sample plays up to 10 s of the sample (Buzz arms the stop timer
   with the absolute end time).
8. CJK rendering uses SC-preferred glyphs for zh_TW/ja_JP (bundled
   pan-CJK font vs Qt system fonts).
9. Live-API chunk errors: tolerate 3 consecutive, then fail loud
   (supersedes M7 L17 fail-on-first; Buzz tolerates infinitely by
   injecting error text into the transcript).
10. F-027/F-087 are outcome-level parity by decided strategy (ADR-011):
    demucs subprocess instead of vendored-fork Python API; ort/community-1
    diarization instead of NeMo MSDD + forced alignment + punctuation.
11. F-087's cancel granularity is coarser than §6's own sketch
    ("checked between stages and inside the windowed diarization loop"):
    `speakrs::OwnedDiarizationPipeline::run` is a single monolithic call with
    no intermediate hook, so `humm-diarize`'s `OrtDiarizer` can only check
    `cancel` before and after that call, not inside it — a cancel arriving
    mid-run resolves at the call boundary (~9-10s worst case on the
    two-speaker fixture, ADR-024's Task 9 addendum), not sub-second like the
    rest of the codebase's cancellation points.

*Entries 12-14 were added by the Task 19 closing sweep, from divergences
found and ledgered during implementation.*

12. **`HUMM_FORCE_CPU` does not reach the isolation step.** Buzz checks
    `BUZZ_FORCE_CPU` before its own cuda-else-cpu fallback and passes the
    resulting device to demucs; ADR-023 forbids the `-d` flag entirely (so
    that demucs's own default device rule — the same cuda-else-cpu rule —
    stays the single source of truth), which means humm's `HUMM_FORCE_CPU`
    is honored by whisper inference but is a no-op for the demucs
    subprocess specifically. No practical divergence on this project's
    target hardware (no CUDA ⇒ both sides pick CPU anyway); recorded
    because the env var's documented scope is otherwise "all inference".
13. **The Speaker-ID window title is the task's *display name*, not the
    source file's basename.** Buzz titles it `file_path_as_title(path)` —
    always the basename. humm reuses `display_name_for`, which prefers a
    user-set row `name` (from Rename) and falls back to the basename, so an
    un-renamed task matches Buzz exactly and a renamed one shows the name
    the user chose. Deliberate: within humm, that row *is* how the user
    refers to the transcript everywhere else.
14. **`<stem>_speech.mp3` overwrites an existing file of that name** — the
    destination path is fixed (Buzz's naming), and the final `rename`
    clobbers a pre-existing sibling. Buzz parity: its own
    `file_transcriber_queue_worker.py:191` writes the same fixed path with
    the same effect. Not "fixed" with a uniquifying suffix precisely
    because that would diverge from Buzz's naming, which F-027's export-name
    `_speech`-strip logic depends on.

## 12. Risks

- **speakrs/pyannote-rs spike fails** → in-house minimal ort pipeline
  (segmentation-3.0 + wespeaker + cosine clustering — pyannote-rs's shape,
  ~1-2 kLOC) — schedule risk, not viability risk.
- **deep_filter v0.5.6 doesn't build** → vendor libDF tract path.
- **ort rc pin churn** → exact `=2.0.0-rc.12`, one crate, contract tests
  alone re-validate an upgrade (Standing Policy rule 5).
- **Buzz env for the closing benchmark fails to build** → report the
  subset honestly (§10.6).
- **demucs absent at acceptance** → `uvx`-provisioned for the contract
  run (sandbox-lifted); absence path itself is a tested feature.
