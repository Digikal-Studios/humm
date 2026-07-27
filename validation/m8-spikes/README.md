# M8 validation spikes — deep_filter pin, diarize crate pick, demucs contract, 2-speaker fixture

Task 2 of Milestone 8. Pins the ML-strategy details ADR-011's addendum and
ADR-023/ADR-024 leave to "a validation spike decides": the exact
`deep_filter` git rev, the diarize crate (`speakrs` vs `pyannote-rs`), the
`demucs` subprocess contract, and the 2-speaker test fixture consumed by
`humm-diarize`'s contract tests (Tasks 9–11).

This is a plain evidence record with throwaway scratch crates (built under
the session scratchpad, **not** in this repo — nothing here is a cargo
workspace member), following the `validation/adr-002` precedent. Only this
README, `gen_two_speaker_fixture.sh`, and the fixture itself
(`crates/humm-core/tests/fixtures/diarize/two-speakers.wav`) are committed
deliverables; the DECISIONS.md appendices carry the verdicts forward.

## Environment

| | |
|---|---|
| Host | Apple Silicon (arm64/aarch64-apple-darwin), macOS 26.5.2 (build 25F84) |
| rustc / cargo | 1.97.1 |
| ffmpeg / ffprobe | 8.1.1, on PATH |
| uv / uvx | 0.11.19 (Homebrew build, reports as `x86_64-apple-darwin` — runs under Rosetta) |
| Sandbox | commands here run inside this coding harness's Bash sandbox by default (network allowlist: crates.io, github.com only). Steps that need the wider network (git deps resolved through crates.io are fine; `ort`'s prebuilt-binary CDN `cdn.pyke.io`, HuggingFace `huggingface.co`, PyPI, `dl.fbaipublicfiles.com`) are called out per-spike below as "sandbox-lifted". |

---

## Spike A — `deep_filter` (F-112 noise reduction)

**Pin decided:** repo `Rikorose/DeepFilterNet`, tag `v0.5.6`, which resolves
to commit **`978576aa8400552a4ce9730838c635aa30db5e61`** (lightweight tag —
the tag ref *is* the commit; confirmed via `git ls-remote`). The `libDF`
package is named `deep_filter` (crate/extern name `deep_filter`, even though
its own internal `[lib] name = "df"` — Cargo's `--extern` name follows the
*dependent's* Cargo.toml key, not the upstream crate's internal lib name;
verified by building both ways, see below).

```toml
deep_filter = { git = "https://github.com/Rikorose/DeepFilterNet.git", rev = "978576aa8400552a4ce9730838c635aa30db5e61", features = ["tract", "default-model"] }
```

**Correction to the pre-spike research note:** at the pinned `v0.5.6` tag,
`libDF/Cargo.toml` pins `tract-core`/`tract-onnx`/`tract-pulse`/`tract-hir`
at **`^0.19.4`** (resolved to `0.19.16`), not `^0.21.4` as assumed going in —
`0.21.4` is apparently only true of the unreleased `main` branch
(`0.5.7-pre`). Recorded here so Task 7 doesn't re-derive it; this is a
**pure-Rust dependency, no ort, no model download** as designed.

**Build:** cold `cargo build --release` (crates.io + the git dep, both on
the sandbox's allowlist — no sandbox lift needed) compiled tract-onnx +
deep_filter + all transitive deps from scratch in **85.8s** (measured with
`time`). Builds clean, no warnings surfaced as errors.

**API entry points (`deep_filter::tract`, for Task 7):**

```rust
use deep_filter::tract::{DfParams, DfTract, RuntimeParams};

let r_params = RuntimeParams::default();           // default_with_ch(1); post_filter off, atten_lim_db=100
let df_params = DfParams::default();               // requires feature "default-model"; embeds DeepFilterNet3 tar.gz via include_bytes!
let mut model = DfTract::new(df_params, &r_params)?;

// model.sr == 48000, model.hop_size == 480, model.fft_size == 960, model.lookahead == 2
// Caller resamples input to 48kHz mono f32 and chunks it into model.hop_size-sample frames:
for (noisy_frame, mut enh_frame) in /* ArrayView2<f32> chunks of shape [1, hop_size] */ {
    model.process(noisy_frame, enh_frame.view_mut())?;   // -> Result<f32> (per-frame local SNR estimate)
}
```

`DfParams::new(tar_file: PathBuf)` loads a model tar.gz from disk instead of
the embedded default (not needed — `default-model` embeds DFN3 in the
binary, matching ADR-011's "no model download" verdict). `RuntimeParams`
has builder methods `with_post_filter(beta)`, `with_atten_lim(db)`,
`with_thresholds(min, max_erb, max_df)`, `with_mask_reduce(ReduceMask)` for
tuning. The reference CLI (`libDF/src/bin/enhance_wav.rs`) is the exact
frame-chunking pattern shown above — resample to `model.sr`, iterate
`noisy.axis_chunks_iter(Axis(1), model.hop_size)`, call `process` per chunk,
optionally slice off `model.fft_size - model.hop_size + model.lookahead *
model.hop_size` samples of STFT/lookahead delay from the front if exact
alignment matters.

**Denoise run (sanity, not a quality benchmark):** built a noisy fixture —
`whisper-french.mp3` (Buzz testdata, actual duration 8.517s — see fixture
section below for why "~21s" in the pre-spike research was wrong) resampled
to 48kHz mono, mixed with `ffmpeg`'s `anoisesrc=color=white:amplitude=0.05`.
Ran the model:

```
model sr=48000 hop_size=480 fft_size=960 lookahead=2
input sr=48000 channels=1 bits=16
processed 851 frames of hop_size 480
```

Wall time **0.28s** for 8.517s of audio (RTF ≈ 0.033, ~30× realtime on CPU,
single-threaded, release build). Output duration matches input (minus a
sub-hop trailing remainder, expected — the reference CLI drops any final
partial hop too). `ffmpeg -af volumedetect` sanity: noisy input
`mean_volume=-31.0dB max=-13.8dB`; denoised output `mean_volume=-33.0dB
max=-14.9dB` — comparable levels, not collapsed to silence, audible content
preserved.

**Verdict:** confirmed as designed. Git-pin `deep_filter` at
`978576aa8400552a4ce9730838c635aa30db5e61` (tag `v0.5.6`), features
`["tract", "default-model"]`. No fallback needed.

---

## Spike B — diarize crate pick (F-087 speaker identification)

### Criterion 1 — builds with our toolchain

**speakrs 0.5.0** (crates.io, Apache-2.0; repo `avencera/speakrs`, no git
tags — crates.io release is HEAD of `master` at publish time). **Builds**,
with two deviations from the crate's own defaults, both required by this
machine being Apple Silicon (`aarch64-apple-darwin`, not `x86_64`):

1. Default features pull `ndarray-linalg` with `openblas-static`
   (`ndarray-linalg-default` → `openblas-static` on non-x86_64), which
   compiles OpenBLAS from source via `openblas-src`. This **failed**: the
   from-source build's own self-test binaries fail to link —
   `ld: library 'gfortran' not found` — because Homebrew's `gfortran`
   (`/opt/homebrew/Cellar/gcc/16.1.0/lib/gcc/16/libgfortran.dylib`) isn't on
   the linker's default search path. Fixed by switching to
   `default-features = false, features = ["online", "openblas-system",
   "coreml"]`, which links Homebrew's prebuilt OpenBLAS directly via
   `brew --prefix` (needs `brew`, `openblas`, and `libomp` installed — all
   present here). **Record for Task 9/CI:** on aarch64 macOS, prefer
   `openblas-system` unless the build environment is known to have a
   working from-source OpenBLAS toolchain; document the Homebrew
   dependency (`brew install openblas libomp`).
2. `ort` (`=2.0.0-rc.12`, matching ADR-024's exact pin) needs its prebuilt
   runtime binary from `cdn.pyke.io` at build time — **sandbox-lifted**
   for this fetch only (the coding harness's default network allowlist
   doesn't include `cdn.pyke.io`); this matches ADR-024's "one sandbox-
   lifted fetch, like the cargo-home seeding recipe" build note.

With those two adjustments, `cargo build --release` succeeds
(`speakrs = { version = "0.5", default-features = false, features =
["online", "openblas-system", "coreml"] }`).

**pyannote-rs 0.3.4** (crates.io, MIT; repo `thewh1teagle/pyannote-rs`).
**Fails to build as published**: its `Cargo.toml` declares `ort =
"2.0.0-rc.10"`, but Cargo's pre-release matching resolves that requirement
to the newest compatible pre-release on the same `2.0.0` core, i.e.
**`2.0.0-rc.12`** — and pyannote-rs 0.3.4 does not compile against rc.12:
`ort::Error<SessionBuilder>` in rc.12 is no longer `Send + Sync`
(`Box<dyn Operator>` isn't `Sync`), which breaks pyannote-rs's `?`-based
error conversion into `eyre::Report` (29 compile errors, all the same root
cause). **Workaround:** pin `ort = "=2.0.0-rc.10"` explicitly in the
dependent crate to force the version pyannote-rs was actually tested
against; then it builds (also needed the same sandbox-lifted `cdn.pyke.io`
fetch, this time for `ort-sys 2.0.0-rc.10`'s binary).

**Verdict on criterion 1:** both build, but only with adjustments; speakrs's
are environment-portability fixes (BLAS backend selection), pyannote-rs's
is a **real upstream incompatibility** with the ADR-024-mandated exact `ort`
pin (`=2.0.0-rc.12`) — pyannote-rs 0.3.4 cannot be used at that exact ort
version without patching pyannote-rs itself. This alone weighs toward
speakrs.

### Criterion 2 — explicit local-model-path API (no env mutation)

**speakrs wins decisively — better than the pre-spike research assumed.**
`speakrs::OwnedDiarizationPipeline::from_dir(models_dir: impl Into<PathBuf>,
mode: ExecutionMode) -> Result<Self, PipelineError>` takes the directory
**directly as an argument** — no `SPEAKRS_MODELS_DIR` env var involved at
all for the offline path (that env var only matters for the separate
`ModelManager::new()` *online* download path, which speakrs also offers via
`from_pretrained(mode)` / `ModelManager::with_cache_dir(dir)`). This is
exactly the "explicit local-model-path API usable without process-global
env mutation" the brief asked for, and it composes cleanly with humm's
`ensure_artifact` → `model_root()/diarization/` → pass the path to
`from_dir` — no env mutation anywhere, so no conflict with humm's
tests-forbid-`set_var` rule.

pyannote-rs's public API (`EmbeddingExtractor::new(path)`,
`pyannote_rs::get_segments(samples, sr, seg_model_path)`) also takes
explicit paths — it would also pass this criterion — but it's moot given
criterion 1's build incompatibility and criteria 3/4 below.

### Criterion 3 — 2-speaker fixture sanity (≥2 speakers, boundaries ±2s of splice points)

This is where the spike found a real problem — **not** in either crate, but
in the *fixture material* the brief specified. Full account, because Tasks
9/11 need to know exactly what was tried and why the committed fixture
differs from the brief's literal text.

**First attempt — brief's literal fixture** (`audio-long.mp3[0,15)` +
`whisper-french.mp3` full + `audio-long.mp3[30,45)`, 16kHz mono):

- **speakrs**, `ExecutionMode::Cpu`, default `AhcConfig` (threshold 0.6):
  detects **1 speaker** across the whole 38.5s clip.
- Swept `AhcConfig.threshold` (the AHC dendrogram merge-distance knob) at
  0.01, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4, 0.45, 0.5 — **every value
  produced the identical 1-speaker result.** A threshold of 0.01 should
  force AHC to split almost anything into separate clusters, so this rules
  out "the default threshold merges them" as the explanation.
- **Positive control:** ran the exact same binary against speakrs's own
  bundled multi-speaker test fixture (`fixtures/test.wav` from the
  `avencera/speakrs` repo) — correctly detects **2 speakers** with sane
  boundaries, both at default threshold and forced to threshold=4.0
  (which should force AHC's *initial* guess to a single cluster). VBx
  still recovers 2 speakers there. This proves the pipeline itself works
  and that VBx doesn't blindly defer to AHC's initial cluster count — so
  the 1-speaker result on the brief's fixture is not a config or tool
  defect.
- **Window-length hypothesis, tested and ruled out:** `whisper-french.mp3`
  is actually **8.517s** long, not "~21s" as the pre-spike research
  assumed (verified with `ffprobe`) — shorter than speakrs's fixed 10s
  segmentation window, so no single analysis window ever sees "pure"
  speaker-B audio. Looped `whisper-french.mp3` back-to-back to 17.03s
  (comfortably over the 10s window) and reran: **still 1 speaker.** This
  rules out window length as the (sole) cause.
- **pyannote-rs**, same fixture, default 0.5 cosine threshold: detects
  **4** "speakers" (`speaker = 1/2/3/4`), none of them a clean 2-way split
  aligned with the true splice points, and one label (`1`) reappears
  discontinuously (17.0–20.1s, then again at 34.9–35.5s) — its simpler
  online-cosine-match design (exactly as speakrs's own docs describe: VAD
  → per-segment embedding → greedy nearest-neighbor match, no global
  refinement) is *more* fragile here, not less.

**Conclusion:** with the literal brief fixture, **both** candidate crates
fail criterion 3 — but the evidence above (positive control passing
cleanly, AHC-threshold invariance, window-length ruled out) points at the
*fixture's source material*, not the tools: `audio-long.mp3` and
`whisper-french.mp3` are both clean-studio male French speech, and under
this embedding model they are apparently not separable as distinct
speakers (whether they are literally the same speaker or merely
very similar-timbre voices in similar recording conditions was not
determined — that would need ground truth about the fixtures' provenance,
which doesn't exist in Buzz's checkout).

**Diagnostic swap, then adopted as the fixture:** substituted Buzz's other
bundled non-English sample, `whisper-latvian.wav` (7.1915s, WAV not MP3),
for the middle segment, keeping everything else identical. Result:

```
2.427 - 2.765   SPEAKER_00
15.387 - 15.860 SPEAKER_01
16.231 - 18.543 SPEAKER_01
18.644 - 21.614 SPEAKER_01
22.171 - 22.542 SPEAKER_00
22.711 - 27.065 SPEAKER_00
28.077 - 32.785 SPEAKER_00
33.764 - 34.338 SPEAKER_00
34.743 - 37.122 SPEAKER_00
unique speakers: 2
```

Splice points are 15.000s and 22.1915s (15 + 7.1915). Detected speaker
changes: **15.387s** (0.39s off) and **22.171s** (0.02s off) — both
comfortably inside the ±2s tolerance, with the correct 2-speaker count.
**This is the fixture committed** (`crates/humm-core/tests/fixtures/
diarize/two-speakers.wav`) and what `gen_two_speaker_fixture.sh` produces
— see "2-speaker fixture" section below for the deviation this represents
from the brief's literal text, and why it's the right call.

### Criterion 4 — runtime sane on CPU (and CoreML if trivial)

speakrs, `ExecutionMode::Cpu`, on the final 37.19s two-speaker fixture:
model load **69ms**, full diarization run **9.8s** (RTF ≈ 0.26, ~3.8×
realtime, single CPU run, release build, cold-not-warmed process). Sane for
an interactive "Identify Speakers" step with a progress UI (J-7). CoreML
(`ExecutionMode::CoreMl`) is feature-available (`features = ["coreml"]`
compiled and linked cleanly) but not exercised end-to-end in this spike —
Task 9/11 should benchmark it (speakrs's own published VoxConverse numbers
claim CoreML is dramatically faster than CPU on Apple Silicon, ~500×
realtime).

### Model artifacts (for Task 9's `ensure_artifact`)

speakrs `ExecutionMode::Cpu` requires exactly these files, all from HF repo
**`avencera/speakrs-models`**, resolved via
`https://huggingface.co/avencera/speakrs-models/resolve/main/<filename>`
(standard HF `resolve/main` URLs, public repo, no auth; confirmed via
`curl -I`, which 302-redirects to a signed CDN URL that 200s):

| file | size |
|---|---|
| `segmentation-3.0.onnx` | 5,916,308 B |
| `wespeaker-voxceleb-resnet34.onnx` | 26,894,815 B |
| `wespeaker-voxceleb-resnet34.onnx.data` | 26,673,152 B |
| `plda_lda.npy` | 131,200 B |
| `plda_tr.npy` | 131,200 B |
| `plda_mu.npy` | 1,152 B |
| `plda_psi.npy` | 1,152 B |
| `plda_mean1.npy` | 2,176 B |
| `plda_mean2.npy` | 640 B |
| `wespeaker-voxceleb-resnet34.min_num_samples.txt` | 4 B |

Total **59,751,799 B = 59.75 MB (56.98 MiB)** — the sum of the table above.
(*Corrected in the Task 19 sweep: this line read "≈ 58MB", which is the MiB
figure rounded and then labelled MB; ADR-024 carried the same number and is
corrected with it.*) Fetched all 10 via `speakrs::models::ModelManager` (an
HF-hub-cache-backed convenience the crate exposes) in **127.8s** cold on
this network. **Confirmed layout:** `ModelBundle::from_dir(dir)` just joins
these exact filenames flat onto `dir` — no subdirectories, no manifest file,
symlinks-vs-real-files doesn't matter (verified: copied the HF cache's
symlinked snapshot into a plain flat directory of regular files and
`from_dir` loaded it identically). **Task 9 action:** `ensure_artifact` can
download these 10 files directly by URL into `model_root()/diarization/`
(flat, no subfolder) and hand that path straight to `from_dir` —
`ExecutionMode::Cpu` needs only these 10; CoreML/CUDA modes need
additional split/`.mlmodelc` files (see `speakrs`'s `src/models.rs`
`required_files()` if Task 9/11 pursue CoreML model provisioning too).

### Verdict

**speakrs 0.5, pinned exactly**, is the diarize crate for `humm-diarize`,
via `OwnedDiarizationPipeline::from_dir`. Wins criteria 1 (builds, with
noted `openblas-system` + sandbox-lifted-fetch adjustments), 2 (explicit
path API, no env mutation — better than assumed), 4 (sane CPU runtime).
Criterion 3 required a fixture-material correction (documented above and in
the fixture section) rather than a crate-selection change — both crates hit
the same wall on the brief's literal fixture, and the diagnostic swap
proves the *tool* works once given separable voices. `pyannote-rs` is
**not** selected: it fails to build against the ADR-024-mandated exact
`ort =2.0.0-rc.12` pin without an unplanned extra pin, and is noisier
(4 spurious speakers) on the same audio where speakrs at least degrades
gracefully to under-count rather than over-fragment.

**Cargo dependency for `humm-diarize` (Task 9):**

```toml
speakrs = { version = "=0.5.0", default-features = false, features = ["online", "openblas-system", "coreml"] }
```

(pin exact per ADR-024; `openblas-system` needs `brew install openblas
libomp` documented as a build prerequisite on macOS; `coreml` only compiles
on macOS and is additive — CPU mode still works with it enabled).

---

## Spike C — `demucs` subprocess contract (F-027 speech isolation)

**Sandbox note:** `uvx`'s cache init fails sandboxed
(`~/.cache/uv/sdists-v9/.git: Operation not permitted`) exactly as
predicted — every command in this section ran sandbox-lifted.

**`uvx demucs` (bare, latest = 4.1.0) fails to install** in this
environment: demucs 4.1.0 added a dependency on `sphn` (a Rust/PyO3 audio
I/O crate), and this `uv` binary is an `x86_64-apple-darwin` Homebrew build
(runs under Rosetta) that selects an x86_64 CPython, forcing `sphn`'s
`maturin`/cargo build to target `x86_64-apple-darwin` — a Rust target not
installed via `rustup` on this machine (`error[E0463]: can't find crate for
'core'`). **This is an environment-provisioning fact about this spike
machine's `uv` install, not a fact about humm** — humm's own
`DemucsIsolator` never invokes `uvx`; it only looks for an already-installed
`demucs` binary (`HUMM_DEMUCS` env, else `PATH`), per ADR-023. Recorded
because Task 5/11's own contract-test setup will hit the same thing if it
provisions `demucs` via bare `uvx demucs`.

**Working install used for this spike:**
`uvx --from demucs==4.0.1 --with "numpy<2" demucs ...` — pins the
classic (pre-`sphn`) demucs release Buzz itself vendors a version close to,
and constrains `numpy<2` (otherwise a NumPy-2/torch-built-against-NumPy-1
ABI mismatch crashes `AudioFile.read()` with `RuntimeError: Numpy is not
available`, since `uv`'s resolver otherwise picks NumPy 2.2.6 for this
Python/torch combination).

**Command run** (matches ADR-023's contract exactly):

```
demucs --two-stems=vocals -n htdemucs --mp3 -o <scratch> --filename "{stem}.{ext}" <input.mp3>
```

Input: `whisper-french.mp3` (8.517s).

**First run** (cold, model not cached): **60.2s** wall total — includes
downloading the `htdemucs` hybrid-transformer checkpoint (**80.2MB**) from
`https://dl.fbaipublicfiles.com/demucs/hybrid_transformer/955717e8-8726e21a.th`
to `~/.cache/torch/hub/checkpoints/` (first-run-only; PyTorch Hub's
standard cache, outside humm's `model_root()` — demucs manages this
itself, not humm). **Second run** (model cached): **7.2s** wall total for
the separation of an 8.517s clip.

**Output layout — confirmed exactly as ADR-023/the plan expects:**

```
<scratch>/htdemucs/vocals.mp3
<scratch>/htdemucs/no_vocals.mp3
```

No per-track subdirectory — because `--filename "{stem}.{ext}"` omits
`{track}`, demucs doesn't nest a folder named after the input file (its
*default* filename pattern, `"{track}/{stem}.{ext}"`, would have; the
`--filename` override is exactly what avoids that, confirming Task 5's
rename-logic assumption). Both output files are valid MP3s, duration
matches input (8.568s vs 8.517s input — normal MP3 encoder padding), and
`ffmpeg -af volumedetect` on `vocals.mp3` shows `mean_volume=-26.5dB
max_volume=-6.6dB` — audible, not silent or corrupted.

**Device:** no `-d` flag passed; demucs 4.0.1's own default is "cuda if
available else cpu" (no MPS auto-detection in this version's `--help`) —
ran CPU automatically on this machine (no CUDA), matching ADR-023's "no
`-d`, cuda-else-cpu is Buzz's own device rule."

**Verdict:** confirmed as designed. `DemucsIsolator`'s subprocess contract
(command shape, output layout, discovery via `HUMM_DEMUCS`/`PATH`) needs no
changes. Record for Task 5/11's contract-test harness: provision `demucs`
via a pinned install (`uvx --from demucs==4.0.1 --with "numpy<2" demucs`,
or an equivalent `pip install "demucs==4.0.1" "numpy<2"`), not bare
`uvx demucs`, to avoid the `sphn`/Rust-target failure mode above; the first
contract-test run on a fresh machine should budget ~60s for the one-time
80MB `htdemucs` checkpoint download.

---

## 2-speaker fixture (`crates/humm-core/tests/fixtures/diarize/two-speakers.wav`)

**Deviation from the task brief, with evidence (see Spike B, criterion 3,
above for the full investigation):** the brief specifies splicing in the
*full* `whisper-french.mp3` as the middle "different speaker" segment. That
file is actually 8.517s (not "~21s" as the pre-spike research assumed), and
— independent of that duration correction — neither candidate diarize
crate can tell it apart from `audio-long.mp3`'s speaker under embedding
comparison (confirmed with an AHC-threshold sweep from 0.01–0.6, a
window-length test, and a positive control proving the tooling itself
works). The fixture actually committed uses **`whisper-latvian.wav`**
(also genuine Buzz testdata, 7.1915s, WAV) in place of `whisper-french.mp3`
for the middle segment — this is the "implementation discovering an
analysis error fixes the doc in the same commit" case from this repo's
process rules: shipping the brief's literal fixture would silently hand
Tasks 9/11 a diarization contract-test fixture that cannot pass its own
stated acceptance criterion, for reasons that have nothing to do with
`humm-diarize`'s correctness.

**Splice** (`validation/m8-spikes/gen_two_speaker_fixture.sh`):

| segment | source | range | duration | speaker |
|---|---|---|---|---|
| 1 | `audio-long.mp3` | `[0s, 15s)` | 15.000s | A |
| 2 | `whisper-latvian.wav` | full file | 7.1915s | B |
| 3 | `audio-long.mp3` | `[30s, 45s)` | 15.000s | A (again) |

Output: 16kHz mono, 16-bit PCM WAV, **37.1915s** total, **1,190,206 bytes**
(≈1.14MB, under the ~2MB target). **Expected speaker-change boundaries:**
**15.0s** and **22.1915s** (±2s tolerance per the brief). Measured
detection (speakrs, `ExecutionMode::Cpu`, default config): **15.387s**
(0.39s off) and **22.171s** (0.02s off) — both well inside tolerance, 2
speakers detected, matching the splice structure.

**Byte-reproducibility:** `gen_two_speaker_fixture.sh` regenerates the
committed file **byte-identically** on this machine (verified via `cmp` —
running the script twice, and running it against the already-committed
fixture, both produced 0-diff output). This holds because every step is a
deterministic `ffmpeg` transcode/concat with fixed flags; the caveat is
`ffmpeg`'s MP3 *decode* path (`audio-long.mp3` is MP3 source) — different
`ffmpeg`/`libavcodec` builds could in principle decode MP3 samples with
different rounding, which would change the regenerated WAV's bytes without
changing its content or splice timing. Documented in the script's header
comment: if a future `ffmpeg` upgrade produces a non-identical-but-
equivalent file, re-run the script and re-commit — nothing downstream
depends on exact bytes, only on the splice timestamps above.

**Regenerate:** `validation/m8-spikes/gen_two_speaker_fixture.sh
[output_path]` (defaults to the committed fixture path; honors
`HUMM_BUZZ_TESTDATA` per Global Constraint 4).
