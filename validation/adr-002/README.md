# ADR-002 validation benchmark — whisper-rs vs candle (M1)

This directory records the empirical validation that ADR-002 (`docs/design/DECISIONS.md`)
requires: the pre-approved `whisper-rs` default is confirmed only if it is not
*decisively* beaten by pure-Rust `candle` on the same audio and model sizes.
The summary table and verdict live in ADR-002's **Validation result (M1)**
subsection; this file is the full, reproducible protocol.

**Verdict (see ADR-002):** whisper-rs wins on every axis at both sizes — ~1.9×
faster wall-clock, 1.75–2.2× lower peak RSS, and a *more complete* transcript.
The pre-approved default stands.

This is a plain protocol + scripts, **not** a cargo project (nothing here is
built by `cargo`), so it adds no workspace member. `validation/*` is listed in
the workspace `Cargo.toml` `exclude` as future-proofing.

---

## Environment

| | |
|---|---|
| Host | Apple M4, 10 cores (4 performance + 6 efficiency), 32 GiB RAM (34 359 738 368 B) |
| OS | macOS 26.5.2 (build 25F84) |
| rustc | 1.96.1 (31fca3adb 2026-06-26) |
| humm | this worktree, `cargo build --release -p humm-cli`, `[profile.release]` = `lto="thin"`, `codegen-units=1` |
| candle | `github.com/huggingface/candle` tag **0.11.0** (commit `31f35b147389700ed2a178ee66a91c3cc25cc80d`) |
| GPU path | both sides on **Metal**. humm: whisper-rs `metal`+`coreml` (the real product path — CoreML encoder auto-downloaded on Apple Silicon). candle: `--features metal` (default device = Metal unless `--cpu`). |
| Measurement | `/usr/bin/time -l` → wall (`real`) + `maximum resident set size` (bytes). 1 warm-up + 2 measured runs per config; a separate confounded run 0 fills the model cache. |

> **Sandbox note.** Every command that loads a Metal model was run with this
> coding harness's Bash sandbox *disabled*. Under the sandbox the GPU path
> reproducibly SEGFAULTs inside ggml's Metal allocator (`MTLCreateSystemDefaultDevice`
> init failing → null deref) — an artifact of the sandbox, not a humm/whisper-rs/ggml
> bug. Root-caused in `docs/benchmarks/M1.md` ("Environment finding"). Builds
> stayed sandboxed; only the runs (and model downloads to huggingface.co, which
> the sandbox network allowlist blocks) were unsandboxed.

## Fairness decisions

- **Same input, both sides.** One file, decoded once (below), consumed by both.
- **Same measurement scope, both sides.** Wall-clock is *model-load-included*
  (process start → transcript printed/written), and RSS is process peak. Run 0
  (first invocation: model download + CoreML first-compile on the humm side)
  is **excluded** from the table on both sides — candle's run 0 is likewise
  inflated by its HF download — so only warm, disk-cached, steady-state runs are
  compared.
- **Weights: full (un-quantized) on both sides.**
  - humm loads the standard whisper.cpp **ggml** model (`ggml-small.bin`,
    `ggml-large-v3-turbo.bin`) — **F16** weights, exactly the files Buzz downloads.
  - candle's `whisper` example loads full **safetensors** at `whisper::DTYPE`
    = **F32** (`candle-transformers/src/models/whisper/mod.rs`). This is candle's
    *default* and, for these sizes, its *only* option: the example's `--quantized`
    (GGUF) path is `unimplemented!("no quantized support for {:?}")` for every
    size except tiny (`main.rs:695`). So "full weights vs full weights" is the
    fair and forced choice; candle carries F32 (2× the precision/footprint of
    humm's F16), which if anything gives candle the heavier configuration on RSS.
    We did **not** use `--quantized` (would compare humm-full against
    candle-quantized — apples to oranges, and unavailable anyway).
- **candle side = candle's reference `whisper` example**, which is exactly what
  ADR-002's validation plan names ("candle's whisper example"). It is a
  *minimalist* reference decoder, not a production transcriber — see the quality
  section for what that costs it.

## Protocol & commands

All paths below use `SCRATCH` = the session scratchpad and `WAV` = the shared input.

### Step 1 — prepare input (shared by both sides)

```bash
ffmpeg -y -i /Users/jawad/RustroverProjects/buzz/testdata/audio-long.mp3 \
  -ac 1 -ar 16000 "$SCRATCH/audio-long-16k.wav"
ffprobe -v error -show_entries format=duration -of csv=p=0 "$SCRATCH/audio-long-16k.wav"
# -> 119.994563   (mono, 16 kHz, pcm_s16le; ~2 min)
```

### Step 2 — whisper-rs side (the real humm product path)

```bash
cargo build --release -p humm-cli   # CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home"
BIN=target/release/humm
# run 0 downloads ggml + CoreML encoder and compiles CoreML on first load (confounded, excluded)
for run in run0 warmup m1 m2; do
  /usr/bin/time -l "$BIN" add "$WAV" --txt -s small -l en -d "$SCRATCH/out-wr-small" \
    2> "$SCRATCH/results/wr-small-$run.stderr" 1>/dev/null
done
# repeat with -s large-v3-turbo
```

humm writes `<name> (transcribed on <ts>).txt` (Buzz's naming convention).
On Apple Silicon it downloads and uses the CoreML encoder + Metal by default
(`humm-cli/src/run.rs`: `coreml_encoder()` = `cfg!(macos+aarch64)`, `use_gpu`
defaults on) — this is the shipped behavior, so it is what we measure.

### Step 3 — candle side (candle's whisper example, Metal)

```bash
git clone --depth 1 --branch 0.11.0 https://github.com/huggingface/candle "$SCRATCH/candle"
# Build fix (upstream dep churn, see below): pin the audio-loader dep to symphonia 0.5.4
sed -i '' 's/symphonia = { version = "0.6.0"/symphonia = { version = "0.5.4"/' \
  "$SCRATCH/candle/candle-examples/Cargo.toml"
( cd "$SCRATCH/candle" && CARGO_HOME="$SCRATCH/cargo-home" \
  cargo build --release --features metal,symphonia --example whisper )
BIN="$SCRATCH/candle/target/release/examples/whisper"
for run in run0 warmup m1 m2; do
  /usr/bin/time -l "$BIN" --model small --input "$WAV" --language en --task transcribe \
    1> "$SCRATCH/results/candle-small-$run.stdout" 2> "$SCRATCH/results/candle-small-$run.stderr"
done
# repeat with --model large-v3-turbo
```

Flags verified against `candle-examples/examples/whisper/{README.md,main.rs}`:
`--model` accepts `small` and `large-v3-turbo` (the 0.11.0 `WhichModel` enum
supports turbo → `openai/whisper-large-v3-turbo`, even though the README's model
list wasn't updated); `--input` takes a 16 kHz wav; default device is Metal
(feature `metal`, no `--cpu`); the example downloads its own safetensors from HF.

#### candle build fix (one line, recorded for honesty)

At tag 0.11.0 the example **fails to build** out of the box: `candle-examples`
pins `symphonia = "0.6.0"`, but its `src/audio.rs` uses symphonia's 0.5-era
module paths (`symphonia::core::{sample,conv,probe}`), which 0.6.0 reorganized —
`error[E0433]: could not find 'sample' in 'core'` (×6). This is upstream
**dependency churn**, not a candle-inference problem. The fix was two sequential
steps, both required: (1) the `sed` above edits `candle-examples/Cargo.toml`'s
requirement from `"0.6.0"` to `"0.5.4"` (the version the code was written for),
then (2) `cargo update -p symphonia --precise 0.5.4` rewrites `Cargo.lock` to
that exact version — the operative resolution that makes the module paths
compile. candle's whisper model/inference code is untouched. With that the
example builds and runs cleanly (attempt 2 of the ADR's 3-attempt budget). No
other candle source was modified.

### Step 4 — large-v3-turbo

Both sides ran turbo with no size-specific problems (disk headroom was ample;
started at 43 GiB free, ended at 34 GiB after all downloads: humm 487 MB ggml +
1.6 GB ggml + CoreML zips; candle ~967 MB + ~3.2 GB F32 safetensors). No skips.

### Step 5 — quality proxy

No ground-truth reference exists, so quality is a **pairwise agreement** proxy:
normalize both transcripts (lowercase, strip punctuation, collapse whitespace)
and take word-level Levenshtein distance; agreement = `1 − dist/max(len_a,len_b)`.
Script: [`quality_proxy.py`](./quality_proxy.py) (it also extracts candle's text
out of its windowed `A.As-B.Bs: <text>` stdout and prints 3 sample sentences per
side). Run:

```bash
python3 quality_proxy.py small           wr-small-transcript.txt  candle-small-transcript.raw
python3 quality_proxy.py large-v3-turbo  wr-turbo-transcript.txt  candle-turbo-transcript.raw
```

---

## Results

### Raw `/usr/bin/time -l` (wall `real`, peak RSS)

```
config                 real (s)   maximum resident set size
--------------------   --------   -------------------------
wr-small   run0         45.97      1104117760  (1053 MiB)   <- download+CoreML compile+infer (excluded)
wr-small   warmup        6.16       902660096  ( 861 MiB)
wr-small   m1            8.18       902135808  ( 860 MiB)
wr-small   m2            5.66       903446528  ( 862 MiB)
wr-turbo   run0        124.11      4373250048  (4171 MiB)   <- download+CoreML compile+infer (excluded)
wr-turbo   warmup        8.17      1909276672  (1821 MiB)
wr-turbo   m1            8.69      1908899840  (1820 MiB)
wr-turbo   m2            7.68      1908883456  (1820 MiB)
candle-small run0       52.62      2027732992  (1934 MiB)   <- HF download+infer (excluded)
candle-small warmup     13.14      2020950016  (1927 MiB)
candle-small m1         13.26      2020966400  (1927 MiB)
candle-small m2         13.14      2020720640  (1927 MiB)
candle-turbo run0       72.22      3294806016  (3142 MiB)   <- HF download+infer (excluded)
candle-turbo warmup     15.93      3341139968  (3186 MiB)
candle-turbo m1         15.63      3341221888  (3186 MiB)
candle-turbo m2         16.52      3341041664  (3186 MiB)
```

### Steady-state comparison (mean of m1+m2)

| Model | Engine | Wall (s) | Realtime (120 s audio) | Peak RSS | Weights |
|---|---|---:|---:|---:|---|
| small | **whisper-rs** | **6.92** | **17.3×** | **861 MiB** | ggml F16 |
| small | candle | 13.20 | 9.1× | 1927 MiB | safetensors F32 |
| large-v3-turbo | **whisper-rs** | **8.18** | **14.7×** | **1820 MiB** | ggml F16 |
| large-v3-turbo | candle | 16.07 | 7.5× | 3186 MiB | safetensors F32 |

**whisper-rs is faster and lighter at both sizes:** wall-clock **1.91×** (small)
/ **1.96×** (turbo) faster; peak RSS **2.24×** (small) / **1.75×** (turbo) lower.

### Quality proxy (pairwise agreement)

| Model | whisper-rs words | candle words | word edit dist. | agreement |
|---|---:|---:|---:|---:|
| small | 227 | 136 | 140 | 38.3 % |
| large-v3-turbo | 243 | 128 | 155 | 36.2 % |

The low agreement is **not** two good transcripts disagreeing word-for-word —
it is candle's minimalist example emitting **~half the words** with language
instability. `audio-long.mp3` is a French-language podcast (both sides were
given `-l en`, which neither strictly honors). Findings:

- **candle loses whole 30 s of content, via two distinct mechanisms visible in
  its stdout — not for lack of a no-speech threshold (the example *has* one).**
  (i) An outright **no-speech skip**: the example's threshold at `main.rs:449`
  (`dr.no_speech_prob > NO_SPEECH_THRESHOLD && dr.avg_logprob < LOGPROB_THRESHOLD`)
  fires on the padded tail window and prints the `no speech detected, skipping
  13500` line quoted above (`no_speech_prob: 0.94`), dropping that window
  (`continue`, `main.rs:451`). (ii) **Degenerate/empty windows**: a mid-file
  30 s window (candle-small 60–90 s, candle-turbo 90–120 s) decodes to just a
  header and an empty `0.0s-0.0s:` segment with no text. Neither has a recovery
  path — the temperature-fallback loop (`decode_with_fallback`, `main.rs:256`)
  is short-circuited once a window is classed no-speech (`main.rs:267` returns
  early), and the example never conditions on previously-decoded text across
  windows. whisper.cpp/whisper-rs carries prompt context across windows and does
  not discard a window's audio wholesale, so it produced a continuous transcript
  over the full 2 minutes.
- **candle flip-flops language per window** — it transcribes some 30 s windows
  in French and *translates* others to English, even under `--task transcribe`.
  whisper-rs stayed in French (turbo perfectly; small had one mid-file en slip).
- **candle-turbo hallucinated a tail** — "Thank you." for a phantom 120–135 s
  window past the 120 s audio (its mel padding created an extra window).
- **Where both transcribe the same span, they agree near-verbatim**, e.g. both:
  *"Dans chaque épisode, j'essaye/essaie de vous proposer des sujets différents
  à explorer."* — confirming the *engines* agree; the divergence is candle
  example-level decoding robustness, not the candle library's model math.

**Sample sentences (eyeball check, 3 per side).** Generated by
`quality_proxy.py`; `timestamps` mode is on by default in candle 0.11.0
(`--timestamps` `default_value_t = true`), which is why candle's output is
windowed.

*small — whisper-rs:*
1. "Bienvenue dans Passerelle, un podcast pensé pour éveiller la curiosité des apprenantes et des apprenants de français."
2. "Je m'appelle Émilie et cette semaine, comme d'habitude, je vous invite à prendre quelques minutes pour qu'on réfléchisse ensemble à une question."
3. "Dans chaque épisode, j'essaye de vous proposer des sujets différents à explorer."

*small — candle:*
1. "A podcast designed to awaken the curiosity of French learners."
2. "My name is Emily and this week, as usual, I invite you to learn French."
3. "Dans chaque épisode, j'essaie de vous proposer des sujets différents à explorer."

*large-v3-turbo — whisper-rs:*
1. "Bienvenue dans Passerelle, un podcast pensé pour éveiller la curiosité des apprenantes et des apprenants de français."
2. "Je m'appelle Émilie et cette semaine, comme d'habitude, je vous invite à prendre quelques minutes pour qu'on réfléchisse ensemble à une question."
3. "Dans chaque épisode, j'essaye de vous proposer des sujets différents à explorer."

*large-v3-turbo — candle:*
1. "Bienvenue dans Passerelle, un podcast pensé pour éveiller la curiosité des apprenantes et des apprenants de français."
2. "quelques minutes pour qu'on réfléchisse ensemble à une question."
3. "Dans chaque épisode, j'essaye de vous proposer des sujets différents à explorer."

The samples show the same story: turbo converges (both open with the identical
French sentence); small diverges because candle renders the intro as an English
*translation*. Net: neither engine is *higher* quality; whisper-rs is *more
complete*. Quality gives no reason to abandon the default; speed and memory
decisively favor it.

### CoreML first-load note (risk-register item)

First-load overhead is real but was **confounded by the model download** and
could not be cleanly isolated: the humm run 0 (small 45.97 s, turbo 124.11 s)
bundles model download (487 MB / 1.6 GB) + CoreML-zip download + CoreML ANE
first-compile + inference, versus ~6–8 s steady state. whisper.cpp's compiled
CoreML encoder is cached after first load, so the penalty is one-time; the
comparison table uses only post-cache steady-state runs (candle's run 0 is
likewise download-inflated and likewise excluded — symmetric). whisper.cpp's
CoreML log lines are not visible because humm routes ggml logs through
`whisper_rs::ggml_logging_hook` at WARN+ (only the "tensor API disabled for
pre-M5" notice surfaced). Takeaway for later milestones: a model's *first-ever*
transcription pays a one-time CoreML compile; keep it off the UI hot path.

## Files

- `quality_proxy.py` — normalizer + word-level edit distance + sample-sentence extractor.
- Raw run logs (`/usr/bin/time -l` stderr) and transcripts were produced in the
  session scratchpad by the commands above; the numbers are transcribed verbatim
  into this file and into ADR-002. Re-running the protocol regenerates them.
