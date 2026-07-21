# M2 differential harness — humm vs Buzz `add` artifacts (F-045 diff note)

Milestone-2 acceptance item: "differential check vs Buzz `add` artifacts."
`docs/analysis/PARITY_MATRIX.md` F-045 note: humm's VAD timestamps are
**sample-exact**, vs Buzz's per-segment **linear rescale** — the diff must
tolerance-compare timestamps, not byte-compare them.

Like `validation/adr-002/`, this is a plain protocol + script, **not** a
cargo project — nothing here is built by `cargo`. `validation/*` is listed in
the workspace `Cargo.toml` `exclude`.

**Result of this run: fallback path (structural mode).** Real Buzz artifacts
could not be produced within the task's 30-minute time-box (see "Step 1"
below for exactly what blocked it). Per the task brief's documented fallback,
humm's own `add` output was validated against the byte-format law captured in
Buzz's golden test fixtures (already ported verbatim into
`crates/humm-core/src/export.rs`). **All 7 structural checks passed.**
**F-045 diff (real Buzz-vs-humm timestamp/segment-count comparison) stays
pending** — the reason is recorded below, not glossed over.

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
| Segment count vs Buzz (±20%) | **N/A — no Buzz artifacts this run** |
| Matched-segment timing vs Buzz (±200 ms) | **N/A — no Buzz artifacts this run** |
| Text similarity vs Buzz | **N/A — no Buzz artifacts this run** |

## What this does and does not establish

**Established:** humm's TXT/SRT/VTT writers, exercised end-to-end through the
real store-backed `humm add` CLI (Task 11's build) on a real audio fixture,
produce output that obeys the exact byte-format law Buzz's own golden tests
define — numbering, arrow lines, separators (`,` for SRT / `.` for VTT), the
`WEBVTT` header, blank-line block separators, and the TXT trim/paragraph-split
rule — and are mutually consistent across all three formats for the same
underlying segments.

**Not established (F-045 diff remains pending):** a real numeric comparison
of humm's segment boundaries/count against Buzz's own `add` output on the
same file — i.e., confirmation that humm's sample-exact VAD timestamps and
Buzz's linearly-rescaled ones land within the ±200 ms/±20 % milestone
tolerance in practice. That requires an actual Buzz run, which this task's
time-box did not allow. `diff_artifacts.py --buzz-dir <dir-with-real-buzz-output>`
will run that exact comparison unmodified whenever a working Buzz environment
becomes available (repairing the LLVM toolchain for `llvmlite`/`numba`,
initializing + building the `whisper.cpp` submodule, and installing Buzz's
full `pyproject.toml` dependency set).

## Files

- `README.md` — this file.
- `diff_artifacts.py` — the compare script (stdlib-only, standalone).
- `humm-artifacts/` — humm's generated TXT/SRT/VTT from the run recorded
  above, committed as evidence.
- `buzz-artifacts/` — intentionally absent (see Step 1).
