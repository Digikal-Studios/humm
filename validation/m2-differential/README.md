# M2 differential harness — humm vs Buzz `add` artifacts (F-045 diff note)

Milestone-2 acceptance item: "differential check vs Buzz `add` artifacts."
`docs/analysis/PARITY_MATRIX.md` F-045 note: humm's VAD timestamps are
**sample-exact**, vs Buzz's per-segment **linear rescale** — the diff must
tolerance-compare timestamps, not byte-compare them.

Like `validation/adr-002/`, this is a plain protocol + script, **not** a
cargo project — nothing here is built by `cargo`. `validation/*` is listed in
the workspace `Cargo.toml` `exclude`.

**Result of the original (M2) run: fallback path (structural mode).** Real
Buzz artifacts could not be produced within the task's 30-minute time-box
(see "Step 1" below for exactly what blocked it). Per the task brief's
documented fallback, humm's own `add` output was validated against the
byte-format law captured in Buzz's golden test fixtures (already ported
verbatim into `crates/humm-core/src/export.rs`). **All 7 structural checks
passed.** F-045 diff (real Buzz-vs-humm timestamp/segment-count comparison)
stayed pending after this run — the reason is recorded below, not glossed
over.

**Update, M5 Task 13 (2026-07-24): the real diff now ran, via a different
route than the one time-boxed here — see "Step 4" below.** The anaconda
python3.9 retry this task was asked to attempt is a dead end for Buzz's *own
package* (two independent, decisive blockers, not just the old llvmlite
one — detailed in Step 4a). But bypassing Buzz's Python packaging entirely —
building the pinned `whisper.cpp` commit directly (outside the Buzz
checkout) and genuinely importing (not reimplementing) Buzz's own
`whisper_cpp.py` VAD-parsing functions via a stub-`sys.modules` technique —
produced real Buzz-side VAD reference output on the M2 fixture, which was
then compared against humm's own output on the same fixture. **Segment-level
result: exact match on segment count (1 vs 1) and start (100 vs 100 ms), 30
ms end delta (8360 vs 8390 ms) — well inside the ±200 ms / ±20 % milestone
tolerance.** See Step 4 for the full method, evidence, and the word-level
divergence pattern that motivated F-045's "sample-exact vs linear rescale"
note in the first place. `docs/analysis/PARITY_MATRIX.md`'s F-045 row is
updated accordingly.

---

## Environment

| | |
|---|---|
| Host | same machine as M1's ADR-002 validation (see `validation/adr-002/README.md`) |
| humm | this worktree, `CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home"`, `cargo build -p humm-cli` (debug), binary at `target/debug/humm` |
| Fixture audio | `/Users/jawad/RustroverProjects/buzz/testdata/whisper-french.mp3` (per task brief; **not** the `ggml-tiny.bin` stub) |
| Model size | `tiny`, `HUMM_MODEL_ROOT` pointed at a session scratch dir (not the real `~/Library/Application Support` cache) |
| GPU/sandbox | ran with the coding harness's Bash sandbox lifted, `HUMM_FORCE_CPU=1` (Metal init still probed under CPU mode; matches M1's documented sandbox-Metal-segfault workaround in `docs/benchmarks/M1.md`) |
| Buzz venv | **none pre-existing** on this machine — checked `buzz/pyproject.toml`/`.python-version`, `~/Library/Caches/pypoetry/virtualenvs`, and searched the filesystem for any `.venv`/`venv` before starting; found nothing. Unlike M1's ADR-002 validation (candle git clone + build), there was no reusable Buzz Python environment to fall back on. |

## Step 1 — attempt Buzz artifact generation (time-boxed, ~30 min)

Target command (per brief): a Python 3.12 venv for Buzz +
`QT_QPA_PLATFORM=offscreen python main.py add <testdata>/whisper-french.mp3 --txt --srt --vtt -s tiny -m whispercpp -d <out>`.

**What was found, in order, before stopping:**

1. No existing venv/poetry env for Buzz anywhere on the machine (checked
   `~/Library/Caches/pypoetry/virtualenvs`, `find ... -iname ".venv"/"venv"`
   under `$HOME` and under `buzz/`). `.python-version` pins `3.12`;
   `/usr/local/bin/python3.12` and `uv` are both present.
2. Even the `whispercpp` (`-m whispercpp`) code path is **not** lightweight.
   `buzz/cli.py` imports `buzz.transcriber.transcriber`, which pulls in
   `buzz/transcriber/whisper_file_transcriber.py`. That module does, at
   **module import time** (before any backend is selected at runtime):
   `from buzz import cuda_setup`, `import torch`, `import faster_whisper`,
   `import whisper` (openai-whisper), `import stable_whisper`, plus `av` and
   `PyQt6`. So running `buzz add -m whispercpp` still requires installing
   Buzz's *entire* heavy dependency graph (torch, faster-whisper,
   openai-whisper, stable-ts, transformers, accelerate, nemo-toolkit,
   demucs, PyQt6, …) from `pyproject.toml` — there is no reduced/minimal
   install for the whisper.cpp-only path.
3. The `whisper.cpp` git submodule is **uninitialized**
   (`git submodule status` shows a leading `-`, empty working directory).
   `buzz/transcriber/whisper_cpp.py` expects a pre-built CLI binary at
   `buzz/whisper_cpp/<whisper-cli-executable>`, produced by the Makefile's
   `buzz/whisper_cpp:` target (`cmake -S whisper.cpp -B whisper.cpp/build …`
   + `cmake --build … --config Release`). This is a from-scratch C++ build
   step on top of the Python env, not yet attempted.
4. Ran `uv pip install -e .` (venv created at a scratch path, sandbox lifted
   because `pip`/`uv` need `pypi.org`, which the coding-harness sandbox
   network allowlist blocks). It failed building **`llvmlite` 0.48.0**
   (pulled in via `openai-whisper` → `numba` → `llvmlite`):

   ```
   subprocess.CalledProcessError: Command
   ['.../bin/python', '.../llvmlite/0.48.0/.../src/ffi/build.py']
   returned non-zero exit status 1.
   hint: `llvmlite` (v0.48.0) was included because `buzz-captions` (v1.4.5)
   depends on `openai-whisper` (v20250625) which depends on `numba`
   (v0.66.0) which depends on `llvmlite`
   ```

   `llvmlite`'s build needs an `llvm-config` matching its supported LLVM
   version. This machine has a homebrew `llvm` *formula directory*
   (`/opt/homebrew/opt/llvm`) but no working `llvm-config` binary in it —
   the install is broken/incomplete. Fixing this would mean repairing (or
   reinstalling) the homebrew LLVM toolchain, a real and open-ended detour.

**Decision: stop.** Between the missing venv, the submodule needing a fresh
C++ build, the forced full heavy-dependency install (not just a
`whispercpp`-only slice), and a genuine broken-toolchain build failure on the
very first dependency resolution attempt, getting Buzz to actually run
`add` on this machine was clearly going to take meaningfully longer than the
~30-minute budget once you account for: repairing/reinstalling LLVM,
re-running `uv pip install -e .` to completion (a multi-GB, many-package
resolution), initializing + building the `whisper.cpp` submodule, and only
then downloading Buzz's own tiny ggml model on first run. This was stopped
after diagnosing the blockers concretely (not a vague "it didn't work") and
well inside the time-box — no partial venv or build artifacts were left
behind (the scratch venv was deleted after the failure was captured above).

Per the brief, this executes the documented fallback: compare humm's
artifacts against the byte-format law already ported from Buzz's own golden
test fixtures, structural-only, with the real Buzz-vs-humm numeric
comparison (segment counts, timestamp tolerance, text similarity) marked
pending.

`validation/m2-differential/buzz-artifacts/` was **not created** — there is
nothing genuine to put in it. (Fabricating placeholder Buzz output would
violate the "documented failure over a fudged comparison" rule this task is
explicitly bound by.)

## Step 2 — humm artifact generation

```bash
export CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home"
cargo build -p humm-cli

export HUMM_MODEL_ROOT=<scratch dir>   # avoid touching the real model cache
export HUMM_FORCE_CPU=1                # + Bash sandbox lifted (Metal-under-sandbox segfault, see docs/benchmarks/M1.md)
target/debug/humm add /Users/jawad/RustroverProjects/buzz/testdata/whisper-french.mp3 \
  --txt --srt --vtt -s tiny -d <out-dir>
```

This is exactly what `diff_artifacts.py` runs (it invokes the binary itself
by default — see below). On this fixture, the `tiny` model produces a
**single segment** covering the whole ~8.4 s clip:

```
1
00:00:00,100 --> 00:00:08,390
Bienvenu dans PASRELE, un podcast, pensé pour évêyer la curiosité des apprenances et des apprenances de français.
```

A copy of the generated `.txt`/`.srt`/`.vtt` (from the run recorded below) is
committed at `validation/m2-differential/humm-artifacts/` as evidence; it is
not required by the script (which regenerates fresh output each run) but
lets a reviewer inspect exactly what was produced without rebuilding
anything.

## Step 3 — compare script + results

`diff_artifacts.py` (stdlib-only Python, standalone — not a workspace
member) implements the **full milestone-acceptance contract** so it is ready
to use as-is the next time someone has a working Buzz environment:

- **Full mode** (if `--buzz-dir` has real Buzz TXT/SRT/VTT artifacts):
  structural-skeleton parse of both sides under the shared format law,
  segment counts within ±20 %, matched-by-order segment start/end within
  ±200 ms, transcript-text similarity via `difflib` (printed, informational
  only — text may legitimately differ between engines).
- **Fallback (structural) mode** (used this run, since `buzz-artifacts/` is
  empty): validates humm's own SRT/VTT/TXT against the byte-format law only.
  The script's reference formatter (`to_timestamp`/`format_txt`/`format_srt`/
  `format_vtt`) is a line-for-line mirror of `crates/humm-core/src/export.rs`,
  and `run_self_tests()` first asserts it reproduces Buzz's own golden
  strings (ported into `export.rs`'s test module from
  `tests/transcriber/transcriber_test.py:12-43` upstream) *exactly*, before
  trusting it to judge humm's real output. It then: parses humm's SRT (checks
  sequential `1, 2, 3…` numbering) and VTT (checks the `WEBVTT\n\n` header);
  round-trips each parsed segment list back through the reference formatter
  and requires byte-for-byte identity with the original file; and
  cross-checks that SRT and VTT agree on segment count/timing/text, and that
  TXT is exactly what `write_txt`'s paragraph-split/trim rule would produce
  from the SRT-derived segments.

Script always exits **nonzero** on any structural/tolerance failure (verified
against a deliberately corrupted SRT — numbering `2` instead of `1` — during
development; it failed the `srt_parses_sequential_numbering` check and
returned exit code 1, as expected).

### Run + result (this task, structural fallback mode)

```bash
python3.12 validation/m2-differential/diff_artifacts.py --force-cpu \
  --archive-dir validation/m2-differential/humm-artifacts
```

```
== self-test: reference formatter vs Buzz golden fixtures (export.rs) ==
self-tests passed: reference formatter matches Buzz's golden strings exactly

== FALLBACK (structural) MODE: no Buzz artifacts found in .../buzz-artifacts
   -- validating humm's own output against the byte-format law (Buzz golden
   fixtures) only. F-045 diff stays pending. ==

[PASS] srt_parses_sequential_numbering: 1 segment(s)
[PASS] vtt_header_and_parses: 1 segment(s)
[PASS] srt_roundtrip_byte_identical: re-formatting the parsed segments reproduces the file byte-for-byte
[PASS] vtt_roundtrip_byte_identical: re-formatting the parsed segments reproduces the file byte-for-byte
[PASS] srt_vtt_segment_count_match: srt=1 vtt=1
[PASS] srt_vtt_segments_identical: all matched segments identical across formats
[PASS] txt_matches_srt_derived_reconstruction: TXT reconstructed from SRT segments matches the actual TXT file byte-for-byte

result: PASS (7/7 checks)
```

Exit code: `0`.

| Format | Result |
|---|---|
| TXT | PASS (structural: matches SRT-derived reconstruction, byte-for-byte) |
| SRT | PASS (structural: sequential numbering, comma separator, round-trips byte-identical) |
| VTT | PASS (structural: `WEBVTT` header, dot separator, round-trips byte-identical) |
| Segment count vs Buzz (±20%) | **N/A — no Buzz artifacts this run (resolved below, Step 4)** |
| Matched-segment timing vs Buzz (±200 ms) | **N/A — no Buzz artifacts this run (resolved below, Step 4)** |
| Text similarity vs Buzz | **N/A — no Buzz artifacts this run** |

## Step 4 — F-045 time-boxed retry (M5 Task 13, dated 2026-07-24)

Carried-ticket retry per `humm-m5-tickets` memory: "M4 proved the local
anaconda python3 (3.9, numpy 1.21.5) runs the *stable-ts* reference harness
genuinely (`validation/m4-differential/`) — the same interpreter might get
further with Buzz's own deps than the M2 attempt did; worth one more
time-boxed try." Time-boxed to ~1 hour of wall effort per the task brief.

### Step 4a — the directed retry: anaconda python3.9 against Buzz's own `pip install`

Located the interpreter: `/Users/jawad/opt/anaconda3/bin/python3` —
`Python 3.9.13`, `numpy 1.21.5`, `pip 22.2.2` (matches the memory note
exactly; `/opt/anaconda3` does not exist on this machine, the real path is
`~/opt/anaconda3`).

**This interpreter cannot be used to `pip install` Buzz's own package at
all, for two independent, decisive reasons — neither is the M2 llvmlite
build failure, and neither depends on which Python version is used beyond
the first:**

1. **Hard Python-version gate.** `buzz/pyproject.toml:7`:
   `requires-python = ">=3.12,<3.13"`. Python 3.9.13 fails that constraint
   outright — pip rejects the install before dependency resolution even
   starts, so the interpreter's *own* numpy/pip health (the thing the
   anaconda lead was banking on) never becomes relevant. `uv`'s `.python-version`
   pin (`3.12`) confirms the same constraint from the other tool.
2. **The install itself always triggers a from-scratch C++ build,
   independent of Python.** `buzz/pyproject.toml:181-185`:
   `[tool.hatch.build.hooks.custom]` + `build-system.requires = [...,
   "cmake>=4.2.0,<5", ...]`. `buzz/hatch_build.py`'s `CustomBuildHook.
   initialize()` unconditionally (for every build `version`, not just
   wheel builds) shells out to `subprocess.run(["make",
   "buzz/whisper_cpp"], ...)` — i.e. *any* `pip install -e .` (or `uv pip
   install -e .`) on Buzz's own package runs the exact cmake+whisper.cpp
   C++ build the original M2 log flagged as "not yet attempted" (M2 log's
   blocker #3). This was previously assumed to be a separate, deferred
   step; reading `hatch_build.py` this run shows it is actually the very
   first thing a real install attempt would hit, before most of Buzz's
   Python dependencies even finish resolving.

Both are verifiable by reading the two files above (no install attempt
needed to confirm #1 — pip's `requires-python` check is unconditional and
version-only; #2 is read directly from the hook source, not inferred).
Given both are structural (file-format-level) blockers rather than
environment-repair problems, spending the remaining time-box actually
running `pip install` with this interpreter would reproduce a predictable,
uninformative rejection — so the retry pivoted (Step 4b) rather than
burning the full hour confirming what the source already shows.

**Conclusion for the *directed* retry: still blocked**, now for a reason
unrelated to the LLVM/llvmlite toolchain the M2 log identified — the anaconda
interpreter is simply the wrong Python version for Buzz's own package, and
even the right version would still need cmake + a from-scratch whisper.cpp
build to get past `pip install` at all.

### Step 4b — a route that actually works: build whisper.cpp directly, run Buzz's own VAD-parsing code without installing Buzz

Buzz's F-045 path (`buzz/transcriber/whisper_cpp.py`, re-read for this task)
does not need Buzz's Python *package* installed at all to produce genuine
Buzz-side reference output — the VAD detection itself happens entirely
inside the `whisper-cli` C++ binary (`--vad --vad-model
ggml-silero-v6.2.0.bin`), and Buzz's own offset-remap code
(`_make_offset_mapper`, `_parse_segment_timings`,
`_parse_word_level_timings`) is ~200 lines of pure Python with no heavy
imports at the *function* level — the heavy imports (`torch`,
`faster-whisper`, `PyQt6`, …) only exist because `whisper_cpp.py`'s three
`from buzz... import ...` lines pull in Buzz's `transcriber.py`/
`file_transcriber.py`, which transitively chain into the rest of the
package. This mirrors exactly the technique
`validation/m4-differential/gen_regroup_fixtures.py` already used
successfully for stable-ts's regroup math: stub only the names actually
needed at import time, then load the real source file unmodified.

**Per the task's hard rule, the Buzz checkout
(`/Users/jawad/RustroverProjects/buzz`) was only ever read** — `whisper.cpp`
was cloned and built in a scratch directory outside the checkout entirely,
never inside `buzz/whisper.cpp/` or any venv under `buzz/`.

1. **Build `whisper-cli` from Buzz's exact pinned commit, outside the Buzz
   checkout.** `buzz/.gitmodules` pins `whisper.cpp` to
   `https://github.com/ggerganov/whisper.cpp` at
   `23ee03506a91ac3d3f0071b40e66a430eebdfa1d` (tag `v1.8.6`; confirmed via
   `git submodule status`, which shows the submodule as uninitialized —
   leading `-` — exactly as the M2 log found). Cloned that exact commit into
   a scratch dir (`git clone --no-checkout` + `git fetch --depth 1 origin
   <sha>` + `git checkout FETCH_HEAD`, entirely under this session's
   scratchpad, `github.com` being on the sandbox's network allowlist) and
   built with `cmake -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=ON
   -DGGML_METAL=0 -DWHISPER_COREML=0 -DGGML_NATIVE=OFF` (Buzz's own Makefile
   uses `-DWHISPER_COREML=1` on Apple Silicon; CoreML was dropped here for
   build speed inside the time-box — it only affects which compute backend
   the *encoder* runs on, not the VAD/offset-remap math under test, so this
   is a build-time convenience, not a divergence from Buzz's behavior).
   `cmake --build build -j --config Release --target whisper-cli` completed
   in well under the time-box; `whisper-cli` built cleanly, no errors.
2. **Run it with Buzz's exact VAD command-line flags**
   (`WhisperCpp._build_command`, `whisper_cpp.py:169-190`):
   `--model ggml-tiny.bin --language fr --print-progress --suppress-nst
   --max-context 0 --entropy-thold 2.8 --output-json-full --threads 4 -f
   whisper-french.mp3 --vad --vad-model ggml-silero-v6.2.0.bin --no-gpu`,
   on the **same fixture the M2 run used**
   (`buzz/testdata/whisper-french.mp3`). Both the `tiny` whisper model and
   the `ggml-silero-v6.2.0.bin` VAD model were already present in humm's own
   model cache (`~/Library/Application Support/humm/models/`, downloaded by
   an earlier humm run) — no `huggingface.co` fetch was needed. Exit code
   0; real whisper.cpp VAD engaged and logged genuinely
   (`whisper_vad_segments_from_probs: Final speech segments after
   filtering: 2`, merged to the JSON's single decoded segment). Raw output
   saved: `f045-vad-diff/whisper-cli-output.json`.
3. **Run Buzz's own, unmodified `whisper_cpp.py` against that JSON** via
   `f045-vad-diff/run_buzz_side.py`: pre-seeds `sys.modules` with minimal
   stand-ins for the four names `whisper_cpp.py` imports at module load
   (`buzz.assets.APP_BASE_DIR`; `buzz.transcriber.transcriber.{Segment,
   Task, FileTranscriptionTask}` — `Segment`/`Task` copied verbatim,
   field-for-field, from `transcriber.py:18-34`; `buzz.transcriber.
   file_transcriber.app_env`), none of which the parsing functions under
   test actually touch at call time — then loads
   `buzz/transcriber/whisper_cpp.py` **directly from its real path via
   `importlib`**, so every line that runs is Buzz's genuine, unmodified
   source, not a reimplementation. Calls the real
   `WhisperCpp._parse_segment_timings` (Buzz's default, no-word-timestamps
   path — does not call the remap function at all, since sentence-level
   offsets come straight from whisper-cli's own JSON) and the real
   `WhisperCpp._parse_word_level_timings` (which does call
   `_make_offset_mapper`'s per-segment linear rescale on every token).
   Output saved: `f045-vad-diff/buzz_side_segments.json`.
4. **Ran humm's own CLI on the identical fixture**: `cargo build -p
   humm-cli` (this worktree), then `humm add whisper-french.mp3 -m
   whispercpp -s tiny -l fr --srt` (segment-level) and the same with `-w`
   (word-level) — `HUMM_FORCE_CPU=1`, sandbox-lifted for the run itself (the
   documented Metal-under-sandbox / model-cache-write gotcha, `docs/
   benchmarks/M1.md`). VAD is on by default in humm (`HUMM_DISABLE_VAD`
   unset); `-e`/`--extract-speech` is a *different*, not-yet-implemented
   Buzz feature (vocal-isolation preprocessing) and was correctly left off.
   Output saved: `f045-vad-diff/humm_segment_level.srt`,
   `f045-vad-diff/humm_word_level.srt`.

**Segment-level result (the milestone's actual acceptance criterion —
segment count ±20 %, matched-segment start/end ±200 ms):**

| | Buzz (genuine, via whisper-cli + real `_parse_segment_timings`) | humm |
|---|---|---|
| Segment count | 1 | 1 |
| Start | 100 ms | 100 ms (Δ 0 ms) |
| End | 8360 ms | 8390 ms (Δ 30 ms) |

**PASS.** Segment count matches exactly; both boundaries are well inside the
±200 ms tolerance (0 ms and 30 ms deltas against a 200 ms budget).

**Word-level result (informative — F-045's "sample-exact vs linear rescale"
note, not a formally toleranced milestone criterion):** both sides produce
exactly 17 words for the same audio. The first word's start (Δ 10 ms) and
the last word's end (Δ 30 ms) track the segment-level numbers closely, but
interior word boundaries diverge by up to ~320 ms word-to-word — consistent
with the PARITY_MATRIX note this task set out to check: Buzz's
`_make_offset_mapper` places each token by a single linear rescale across
the whole VAD-compressed segment span, while humm's per-word timestamps come
from a different underlying decode path entirely (word text itself also
differs slightly between runs — "Bienvenu"/"PASRELE"/"évêyer" on humm's side
vs "Bienvenue"/"Passrail"/"aider" on the genuine whisper-cli run — both are
the same `tiny` model's known low-accuracy French output, not a formatting
bug; a byte-identical transcript was never the acceptance bar, see
`docs/analysis/PARITY_MATRIX.md`'s F-028 note). This word-level divergence
was already anticipated by F-045's existing note and is not itself a
regression; it's recorded here because it's the first genuine numeric
evidence of it, rather than a design-doc assertion.

Artifacts saved under `f045-vad-diff/` (see Files below); the scratch
`whisper.cpp` clone/build itself was not committed (multi-hundred-MB build
tree, fully reproducible from the pinned commit above).

## What this does and does not establish

**Established (M2 run):** humm's TXT/SRT/VTT writers, exercised end-to-end
through the real store-backed `humm add` CLI (Task 11's build) on a real
audio fixture, produce output that obeys the exact byte-format law Buzz's
own golden tests define — numbering, arrow lines, separators (`,` for SRT /
`.` for VTT), the `WEBVTT` header, blank-line block separators, and the TXT
trim/paragraph-split rule — and are mutually consistent across all three
formats for the same underlying segments.

**Established (M5 Task 13 retry, Step 4):** a real numeric comparison of
humm's VAD segment boundaries/count against genuine Buzz-side VAD reference
output (real `whisper-cli --vad` JSON + Buzz's own unmodified
`_parse_segment_timings`/`_make_offset_mapper` code, run without installing
Buzz's Python package) on the same fixture the M2 run used. Segment count
and boundary-timing tolerance (±20 % / ±200 ms) **PASS** on this fixture;
the word-level linear-rescale-vs-sample-exact divergence the parity note
anticipated is now backed by real evidence rather than design-doc reasoning
alone.

**Not established:** this is one fixture with a single VAD-detected
transcription segment — the M4-style multi-fixture, multi-segment breadth
`validation/m4-differential/` achieved for the regroup math has not been
replicated here (out of the ~1 hour time-box for this retry); a segment
count-and-boundary check on a longer, multi-segment recording, and a
formally toleranced (not just qualitative) word-level check, remain open if
deeper F-045 confidence is wanted later. `docs/analysis/PARITY_MATRIX.md`'s
F-045 row is updated to reflect what actually ran (see that file for the
exact wording), not blanket-flipped past what this evidence supports.

## Files

- `README.md` — this file.
- `diff_artifacts.py` — the compare script (stdlib-only, standalone).
- `humm-artifacts/` — humm's generated TXT/SRT/VTT from the M2 run recorded
  above, committed as evidence.
- `buzz-artifacts/` — intentionally absent (see Step 1; the M5 retry's real
  Buzz-side output lives in `f045-vad-diff/` instead, since it was produced
  by a different method than the TXT/SRT/VTT-file comparison this directory
  expects — see Step 4).
- `f045-vad-diff/` — M5 Task 13's real VAD differential (Step 4):
  `whisper-cli-output.json` (raw genuine whisper-cli `--vad
  --output-json-full` output), `run_buzz_side.py` (the stub-import script,
  reproducible/rerunnable), `buzz_side_segments.json` (its output — genuine
  Buzz-code-computed segment/word timings), `humm_segment_level.srt` /
  `humm_word_level.srt` (humm's own output on the identical fixture).
