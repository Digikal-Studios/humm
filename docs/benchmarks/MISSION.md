# Mission-Closing Benchmark: humm vs Buzz

Per `docs/MISSION.md` value #2 ("humm should be decisively faster") and the
M8 design spec §10.6 acceptance item 6: humm vs **Buzz itself**, not vs a
prior humm checkout. This is the number the whole port has been building
toward. Method honesty over flattering numbers: every table below is raw
runs, not just pooled means, and every caveat that could change the reading
is stated rather than smoothed over.

All runs 2026-07-27, same machine as every milestone's own benches: Apple M4
(Mac16,12) / 32 GiB / macOS 26.5.2 (25F84). `uptime` load averages 3.3–7.7
across the whole session (quiet — see §1.3 for the one exception). Metal on
both sides; sandbox lifted for every command that touches the GPU allocator
or the Buzz checkout (established M1–M8 precedent — the sandboxed shell
segfaults inside ggml's Metal allocator on model load).

## 0. Versions

| | humm | Buzz |
|---|---|---|
| Commit | `30d6eec` (2026-07-27) | `d1b11c4` (`v1.4.4-48-gd1b11c4`, 2026-07-17) |
| Engine | whisper-rs → whisper.cpp (vendored, Cargo-pinned) | `whisper-cli` — whisper.cpp submodule `23ee0350` (`v1.8.5-7-g23ee0350`), built fresh this session |
| Backend | Metal + Core ML | Metal + Core ML (confirmed live, §1.2) |
| Model | `ggml-small.bin` + `ggml-small-encoder.mlmodelc` | same repo (`ggerganov/whisper.cpp`), same files |
| VAD | Silero `ggml-silero-v6.2.0.bin` (on by default, `run.rs::ensure_vad`) | Silero `ggml-silero-v6.2.0.bin` (`--vad`, Buzz ships it next to `whisper-cli`) |
| Python / Qt / torch | — | Python 3.12.13 (arm64), PyQt6 6.9.1 (Qt 6.9.0), torch 2.8.0 |

**The two `ggml-small.bin` files are byte-identical** (`md5`
`e428ac3ab827de7b1a37328101eaf1ea` on both sides — humm's cached copy and
Buzz's freshly-downloaded one). Both sides run the same GGML weights, the
same Silero VAD model, on the same Metal+Core ML backend. **No backend
asymmetry** — the whispercpp-class path built and ran on both sides; the
Task 18 brief's torch-fallback contingency was not needed.

## 1. Environment setup (one-time, sandbox-lifted)

### 1.1 `uv sync` — hit a real architecture bug, root-caused and fixed

`uv sync` in `/Users/jawad/RustroverProjects/buzz` failed twice before
succeeding, for two independent reasons — recorded in full because both are
genuine environment findings, not incidental noise:

1. **Submodules were uninitialized** (`whisper.cpp`, `demucs_repo`,
   `whisper_diarization`, `deepmultilingualpunctuation`, `ctc_forced_aligner`
   all showed `-<sha>` in `git submodule status`). `git submodule update
   --init` for all five (network to `github.com`, already allow-listed).
2. **The `cmake` PyPI wheel silently built whisper.cpp for the wrong
   architecture.** Homebrew's `/usr/local/bin/uv` is an **x86_64** binary
   (this machine is arm64; Homebrew installed it under the Rosetta-only
   `/usr/local` prefix). Even after pinning `--python` to a genuine arm64
   CPython (`uv python install cpython-3.12.13-macos-aarch64-none`), every
   `uv sync` that actually invoked hatchling's `make buzz/whisper_cpp`
   build hook compiled with `-arch x86_64` throughout and died with `error:
   unknown target CPU 'apple-m4'` (ggml's `-march=native` resolving to an
   arm64 CPU name inside an x86_64 compile). Root cause: the `cmake` package
   on PyPI ships a **universal2** binary, and macOS's default fat-binary
   slice selection for a translated (Rosetta) process's descendants prefers
   x86_64 all the way down the tree — even through a genuinely arm64-only
   Python in between. **Fix:** bootstrap a native arm64 `uv` (`python3.12 -m
   venv` + `pip install uv`, using the arm64 CPython — `uv 0.11.32
   aarch64-apple-darwin`) and re-run the sync with that binary. First
   attempt with the arm64 `uv` built whisper.cpp correctly (Metal + Core ML,
   confirmed by the configure log: "Metal framework found" / "CoreML
   framework found", zero `-arch x86_64` anywhere) but then failed on
   `ctc_forced_aligner/setup.py: No such file` — a straightforward
   consequence of submodule init not yet having happened at that attempt;
   after all five submodules were initialized (fix 1), the full `uv sync`
   completed cleanly: `Installed 282 packages`, `buzz-captions==1.4.5`
   built and installed editable, whisper.cpp's `whisper-cli`/
   `whisper-server`/`libwhisper.dylib`/`libggml*` copied into
   `buzz/whisper_cpp/`, `ggml-silero-v6.2.0.bin` downloaded.
3. Nothing in the Buzz checkout was modified — the diff is `git status
   --short` showing only the two new, gitignored artifacts (`.venv/`,
   `buzz/whisper_cpp/*`, the five now-checked-out submodule working trees)
   plus one pre-existing untracked stray file from an earlier milestone's
   differential-testing session (`testdata/whisper-french (transcribed on
   27-Jul-2026 18-50-36).txt`) that predates this task and was left alone
   per "benchmark, don't modify."

### 1.2 Buzz launches; whispercpp backend confirmed live on Metal + Core ML

`.venv/bin/python -m buzz add --task transcribe --model-type whispercpp
--model-size small --txt --hide-gui` ran end-to-end on the first real
invocation: downloaded the small ggml model + Core ML encoder (26 s, one
-time), pip-installed `deepfilternet` on demand (Buzz's plugin
lazy-install, one-time), ran `whisper-cli` with `--vad`, wrote a `.txt`,
exited 0. A direct manual invocation of the built `whisper-cli` binary
confirms the backend live (not silently CPU-only):

```
ggml_metal_device_init: GPU name:   MTL0 (Apple M4)
whisper_init_state: loading Core ML model from '.../ggml-small-encoder.mlmodelc'
system_info: ... WHISPER : COREML = 1 | ... ACCELERATE = 1 | REPACK = 1 |
```

### 1.3 The one load exception

The `uv sync`/environment-repair phase ran under the machine's baseline
load (4–10). All benchmark runs below ran at load 3.3–7.7 (quiet), stated
per-section.

## 2. Startup-to-interactive

**Method** (identical both sides): spawn the process, record the spawn
timestamp (`date +%s.%N`), poll `CGWindowListCopyWindowInfo` (a small Swift
helper, `wait_for_window_pid.swift` — the M3/M7 smoke technique) for the
spawned PID's first on-screen, layer-0, non-trivial-size window; report
elapsed wall time. **humm**: `target/release/humm-gui` (release, isolated
`HUMM_*` dirs, reused warm across runs). **Buzz**: `.venv/bin/python -m
buzz` (no file args — opens the main window; window owner is reported as
`python`, matched by PID not name to avoid ambiguity). Both processes killed
and reaped between runs; ≥1 s settle before the next spawn. 3 interleaved
pairs, alternating which side launches first. Load 3.3–5.6 throughout.
**These are warm-launch numbers** (repeat launches on an already-used
process/model/font cache) — not cold-boot-after-reboot; humm's own <300 ms
cold-start target (§UX_SPEC) is a different, colder condition than what is
measured here and the two are not compared directly.

| Pair | Order | humm | Buzz |
|---|---|---|---|
| A | humm→buzz | 0.5524 s | 3.3741 s |
| B | buzz→humm | 0.5675 s | 3.4488 s |
| C | humm→buzz | 0.5376 s | 3.4380 s |

**Pooled: humm 0.5525 s, Buzz 3.4203 s → humm is 6.19× faster to first
visible window.** humm's three runs vary by under 30 ms; Buzz's by under
80 ms — both sides are internally consistent, so this is a clean read, not
noise.

## 3. End-to-end file transcription

**Method** (identical both sides): same fixture
(`/Users/jawad/RustroverProjects/buzz/testdata/audio-long.mp3`, 120.006 s,
`ffprobe`-confirmed), same model size (`small`), same whispercpp-class
backend, `/usr/bin/time -l` for wall clock (`real`) and peak RSS (`maximum
resident set size`) around the whole invocation. **humm**:
`target/release/humm add <file> --txt -s small -d <outdir> --quiet`, fresh
isolated `HUMM_DATA_DIR`/`HUMM_CONFIG_DIR`/`HUMM_LOG_DIR`/output dir per
run. **Buzz**: `.venv/bin/python -m buzz add --task transcribe
--model-type whispercpp --model-size small --txt --hide-gui -d <outdir>
<file>`. Both already warm (model cached, plugin pip-install already done,
page cache primed) before the first timed pair. 4 interleaved pairs (a 4th
added after pair A's Buzz reading looked like an outlier, same precedent as
M7.md §3.2 / M8.md §2.1). Load 3.3–7.7 throughout. Both sides exited 0 on
every run and wrote a 15-segment transcript (transcript text itself is out
of scope — different engines/params, per the diff rules).

| Pair | Order | humm wall | Buzz wall | humm peak RSS | Buzz peak RSS |
|---|---|---|---|---|---|
| A | humm→buzz | 8.52 s | **24.80 s** | 862.83 MB | 861.15 MB |
| B | buzz→humm | 9.02 s | 12.76 s | 866.60 MB | 856.91 MB |
| C | humm→buzz | 8.44 s | 12.76 s | 863.79 MB | 856.90 MB |
| D | buzz→humm | 8.53 s | 12.67 s | 865.29 MB | 856.75 MB |

**Pair A's Buzz reading (24.80 s) is a clear outlier** against its own
subsequent readings (12.76/12.76/12.67 s — near-identical to three decimal
places) on an otherwise-identical, already-warm invocation. Flagged rather
than blended in, per the M7/M8 convention. humm's four readings show no
comparable swing (8.44–9.02 s, a ~7% band) — the outlier is one-sided.

- **Wall clock, pooled over all 4 pairs**: humm 8.6275 s, Buzz 15.9975 s →
  **humm 1.85× faster**.
- **Wall clock, excluding pair A's Buzz outlier (pairs B–D)**: humm 8.6633 s,
  Buzz 12.7233 s → **humm 1.47× faster**. This is the more representative
  reading; both are reported rather than picking the more flattering one.
- **Peak RSS, pooled over all 4 pairs**: humm 864.63 MB, Buzz 857.93 MB —
  **essentially parity** (humm ~0.8% higher). This is the honest surprise
  of this benchmark: during active transcription, peak memory is dominated
  by the shared whisper.cpp engine's own working set (the identical ggml
  model + Metal/Core ML buffers on both sides), not by host-language
  overhead — Python/Qt/torch's own footprint does not show up as the
  *peak* because whisper.cpp's own allocation, reached later and larger,
  dominates the max on both sides. **On this metric alone, the port is not
  "decisively" better — it is a tie**, and that is reported plainly rather
  than reframed.

## 4. Idle-GUI memory

**Method**: launch, settle 10 s, sample RSS via `ps -o rss=`. Two samples
per side (cheap, per the brief).

| | humm-gui | Buzz |
|---|---|---|
| Sample 1 | 117,328 KB | 639,904 KB |
| Sample 2 | 117,088 KB | 650,144 KB |
| Mean | 114.46 MB | 630.02 MB |

**humm's idle window uses 5.50× less memory than Buzz's.** Unlike §3's
active-transcription RSS (engine-dominated, near parity), idle memory is
dominated by host-runtime overhead — Qt + torch + the Python interpreter
loaded but doing nothing, vs. egui/eframe's much smaller baseline — and
here the port's architecture shows a clear, consistent win.

## 5. Corroborating re-run (Task 16 ledger, M7-vs-M8) — skipped, load checked

The brief permits skipping this if the machine is quiet but the time is
disproportionate. Load was checked and was in fact quiet for this session
(3.3–7.7 throughout, see §1.3/§2/§3) — quiet enough that the re-run
*could* have been done. It was skipped anyway: the recipe (`docs/benchmarks/
M8.md` §4) requires standing up a fresh `git worktree` at the M7 tip, a
full release build of the `humm` CLI plus three criterion bench targets in
that separate checkout, and 9 interleaved pairs (4 criterion + 5
product-path) — a substantial rebuild for a question (does M8 regress the
transcription path vs M7?) that Task 16 already answered cleanly one commit
ago in this same milestone, with call-path-innocence proof plus two
independent measurement lines (`docs/benchmarks/M8.md` §§1–2). Re-running
it here would corroborate an already-clean, already-documented verdict, not
inform this task's actual question (humm vs Buzz). Recorded per the brief's
requirement to state the load check either way: **load was quiet; skipped
for scope/time proportionality, not because the machine was busy.**

## 6. Bottom line — does the evidence support "decisively faster"?

Per-metric, stated plainly rather than averaged into one number that would
hide the split result:

| Metric | Result | Verdict |
|---|---|---|
| Startup-to-interactive (warm) | humm 6.19× faster | **Decisive** |
| E2e wall clock (small model, whispercpp-class, pooled all 4 pairs) | humm 1.85× faster | Faster, moderate |
| E2e wall clock (outlier-pair excluded, B–D only) | humm 1.47× faster | Faster, moderate |
| Peak RSS during transcription | ~parity (humm +0.8%) | **Not decisive — a tie** |
| Idle-GUI RSS | humm 5.50× less | **Decisive** |

**Honest bottom line**: humm is decisively faster to launch and decisively
lighter at idle — both direct consequences of the architectural bet (a
native Rust/egui binary vs. a Python/Qt/torch process), and neither
metric is close. On the metric this project's own transcription-path
regression gate treats as primary — actual end-to-end transcription
time — humm is consistently faster (every pair, every reading, no sign
flips) but by a moderate 1.5–1.85×, not an order of magnitude: both sides
are running the *identical* whisper.cpp engine, the identical model
weights, and the identical VAD model on the identical GPU, so the ceiling
on how much faster the inference itself can be is inherently limited — the
speedup here is process/pipeline overhead (subprocess-per-run tqdm/plugin
machinery, Python/Qt startup cost folded into each `--hide-gui` invocation,
etc.), not a faster inference kernel, since there isn't a different kernel
to be faster than. Peak memory during active work is a tie, not a win.
Value #2 ("decisively faster") holds unambiguously for the two overhead-
bound metrics (launch, idle memory) and holds moderately, consistently, but
not dramatically for the compute-bound one (e2e transcription); it does not
hold for peak RSS under load. This is reported as five distinct readings
rather than collapsed into a single misleading "Nx faster" headline.

## 7. Reproducing

```bash
# One-time Buzz env (sandbox-lifted; requires a native-arm64 uv — see §1.1):
cd /Users/jawad/RustroverProjects/buzz
git submodule update --init
python3.12 -m venv /tmp/uv-arm64-venv && /tmp/uv-arm64-venv/bin/pip install uv
/tmp/uv-arm64-venv/bin/uv python install cpython-3.12.13-macos-aarch64-none
UV_PYTHON=$(/tmp/uv-arm64-venv/bin/uv python find cpython-3.12.13-macos-aarch64-none) \
  /tmp/uv-arm64-venv/bin/uv sync --python "$UV_PYTHON"

# Startup-to-interactive (both sides, ≥3 alternating warm runs):
#   spawn process; date +%s.%N before spawn; poll CGWindowListCopyWindowInfo
#   for the spawned PID's first on-screen layer-0 window (Swift helper).

# E2e (both sides, interleaved pairs):
/usr/bin/time -l target/release/humm add testdata/audio-long.mp3 --txt -s small -d <out> --quiet
/usr/bin/time -l .venv/bin/python -m buzz add --task transcribe --model-type whispercpp \
  --model-size small --txt --hide-gui -d <out> testdata/audio-long.mp3

# Idle RSS: launch, sleep 10, `ps -o rss= -p <pid>`.
```
