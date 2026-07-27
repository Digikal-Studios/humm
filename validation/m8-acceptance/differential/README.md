# M8 Task 19b — humm vs Buzz artifact differential (F-028, F-040)

Closes the `diff` half of both `docs/analysis/PARITY_MATRIX.md` rows F-028
("Export on completion") and F-040 ("Local inference backend(s)"). Both rows
named the identical unrun work — "run `buzz add` + `humm add` on one fixture
and diff structure/format" — and Task 18 (`docs/benchmarks/MISSION.md`) had
already built a genuine native-arm64 Buzz environment at
`/Users/jawad/RustroverProjects/buzz/.venv`, so this task's job was simply to
run it and record the comparison neither Task 17 (windowed GUI) nor Task 18
(performance) owned.

**Method, per CLAUDE.md ADR-011 and this row's own scope:** compare
structure/format only — cue numbering, arrow format, timestamp
precision/format, blank-line separators, WEBVTT header, TXT
paragraph-splitting, encoding/BOM/line endings, output file naming. Never
compare transcript text (the two engines' segmentation and word choice are
expected to diverge — see `docs/benchmarks/MISSION.md` §0 for why: both run
whisper.cpp/Metal/Core ML on byte-identical model + VAD weights, but humm
calls whisper-rs's Rust API while Buzz shells out to the `whisper-cli`
binary with its own CLI-level decode parameters, so segment boundaries are
not expected to match even though the underlying engine is the same
whisper.cpp).

## Environment

| | |
|---|---|
| Date | 2026-07-27 |
| humm | this worktree, `cargo build --release` binaries (`target/release/humm`), `CARGO_HOME="$PWD/.cargo-home"`, sandbox lifted (Metal buffer allocation fails under the sandboxed environment — confirmed directly: `ggml_metal_buffer_init: error: failed to allocate buffer` — and succeeds once the Bash sandbox is disabled) |
| humm env | `HUMM_DATA_DIR`/`HUMM_CONFIG_DIR`/`HUMM_LOG_DIR` pointed at a session scratch dir (not the real `~/Library` cache); `HUMM_MODEL_ROOT` left unset (real cached `ggml-small.bin` + Core ML encoder, already present from prior tasks) |
| Buzz | `/Users/jawad/RustroverProjects/buzz`, `.venv/bin/python -m buzz`, built by Task 18 (native arm64 `uv`, whisper.cpp submodule `23ee0350`, `whisper-cli` with Metal + Core ML) |
| Model | `small`, whispercpp model type on both sides — same `ggml-small.bin` (md5 `e428ac3ab827de7b1a37328101eaf1ea`, confirmed byte-identical across both caches in Task 18's report) and same Silero VAD `ggml-silero-v6.2.0.bin` (md5 `ee99234b74067018b10d324cc8dc8119`) |
| Fixtures | `/Users/jawad/RustroverProjects/buzz/testdata/whisper-french.mp3` (8.4 s) and `audio-long.mp3` (120 s) |

## Commands run

```
# humm
export CARGO_HOME="$PWD/.cargo-home"
export HUMM_DATA_DIR=<scratch>/data HUMM_CONFIG_DIR=<scratch>/config HUMM_LOG_DIR=<scratch>/logs
target/release/humm add <fixture> --srt --vtt --txt -s small -m whispercpp \
    -d validation/m8-acceptance/differential/humm-out

# Buzz
cd /Users/jawad/RustroverProjects/buzz
.venv/bin/python -m buzz add --task transcribe --model-type whispercpp --model-size small \
    --srt --vtt --txt --hide-gui \
    -d <humm-repo>/validation/m8-acceptance/differential/buzz-out <fixture>
```

Both ran end to end, exit 0, no errors. humm: 1.94 s (whisper-french.mp3),
9.59 s (audio-long.mp3). Buzz: 14.46 s (whisper-french.mp3, includes ~7 s of
Qt/PyQt6 startup), 11.61 s (audio-long.mp3).

Raw artifacts are kept in `humm-out/` and `buzz-out/` (six files each — one
`.srt`/`.vtt`/`.txt` per fixture); `logs/srt-diff.log` holds the raw `diff -u`
of the SRT files for both fixtures (text differences included for
completeness — not evidence of a defect, per the "never compare transcript
text" rule above).

## Findings

### 1. File naming — the six template tokens + datetime format

Both sides use the identical default template
`"{{ input_file_name }} ({{ task }}d on {{ date_time }})"` — humm's
`crates/humm-core/src/export/mod.rs::DEFAULT_EXPORT_TEMPLATE` doc-comments
this as matching Buzz's `Settings.get_default_export_file_template`
(`settings.py:170-174`) verbatim, and the produced filenames confirm it byte
for byte modulo the timestamp itself:

```
buzz:  whisper-french (transcribed on 27-Jul-2026 20-50-28).srt
humm:  whisper-french (transcribed on 27-Jul-2026 20-50-07).srt
```

`{{ task }}` → `transcribe` + `d` = `transcribed`, `{{ date_time }}` →
`%d-%b-%Y %H-%M-%S` (both sides use this exact `strftime`/`chrono` pattern —
confirmed in source on both, `transcriber.py:238` vs `export/mod.rs`'s
`now.format("%d-%b-%Y %H-%M-%S")`). **Parity-equal.**

### 2. SRT structure

| Aspect | Buzz | humm | Verdict |
|---|---|---|---|
| Cue numbering | sequential `1`, `2`, … | sequential `1`, `2`, … | parity-equal |
| Arrow format | ` --> ` | ` --> ` | parity-equal |
| Timestamp format | `HH:MM:SS,mmm` (comma, zero-padded) | `HH:MM:SS,mmm` | parity-equal |
| Blank-line cue separator | one blank line between cues, trailing `\n\n` | same | parity-equal |
| Segment count (whisper-french.mp3) | 2 | 1 | **inference difference, not format** — see note below |
| Segment count (audio-long.mp3) | 15 | 25 | **inference difference, not format** — see note below |

**Note on segment-count divergence.** humm's whisper-rs call and Buzz's
`whisper-cli` invocation (`--language auto --print-progress --suppress-nst
--max-context 0 --entropy-thold 2.8 --output-json-full --threads 5 --vad
--vad-model …`) are two different call surfaces into the same whisper.cpp
core — one via the Rust FFI bindings' own default decode parameters, one via
the CLI's own flag defaults — and segment/cue boundaries are an emergent
property of the decoder's internal VAD-boundary and no-speech/entropy
heuristics, not of the export formatter. This is exactly the kind of
divergence CLAUDE.md's "transcript text may differ" rule anticipates one
level up (segmentation is a decode-time artifact, same family as word
choice). The **format law that wraps whatever segments the engine produces**
is what F-028's row is about, and it matches exactly. humm's own trailing
segment on audio-long.mp3 (`00:01:59,590 --> 00:01:59,994 A l'chape...`, a
394 ms fragment) is a decode-quality curiosity worth a future look but is
transcript content, not structure.

### 3. VTT structure

`WEBVTT\n\n` header present exactly once, at the top, on both sides (grep
count 1/1 on both files). Cue timestamp format `HH:MM:SS.mmm` (dot, not
comma) on both sides — correctly distinct from SRT's comma. Same blank-line
cue separation as SRT. **Parity-equal.**

### 4. TXT structure

Both sides produce a single space-joined paragraph (no cue numbers, no
timestamps) for both fixtures — neither fixture crosses the 2000 ms
inter-segment gap that triggers a paragraph break (`DEFAULT_PARAGRAPH_SPLIT`
in `export/mod.rs`, ported from Buzz's `BUZZ_PARAGRAPH_SPLIT_TIME` /
`file_transcriber.py:203`), so the split rule itself is not newly exercised
by this differential — it was already covered by F-028's `snapshot` half
(Buzz's golden fixtures ported into `crates/humm-core` since M1) and is
unchanged by this run. What *is* newly confirmed here: both sides' TXT
output ends in the same way — a **trailing space with no trailing newline**
after the final segment's text (confirmed via `od -An -tx1`: both end
`2e 20` — period, space, nothing else). **Parity-equal.**

### 5. Encoding / BOM / line endings

- No UTF-8 BOM on either side (`head -c 3` on the TXT files: `Bie`/`bie`,
  plain ASCII start, no `ef bb bf`).
- Non-ASCII (`é` = `c3 a9`, `ç` = `c3 87`) encoded identically as UTF-8 on
  both sides.
- No `\r` anywhere in any SRT file on either side (`grep -c $'\r'` → 0/0) —
  LF-only line endings on both. **Parity-equal.**

## Verdict

Every structural/format aspect in this row's stated diff scope — naming
template, cue numbering, arrow format, timestamp precision/format,
blank-line separators, WEBVTT header, TXT paragraph-joining and its
end-of-file byte shape, encoding, BOM, line endings — is **parity-equal**
between a real Buzz artifact and a real humm artifact produced from the
same input, same model, same VAD. No defect found. The only divergence
(segment/cue count) is a decode-time artifact of two different call
surfaces into whisper.cpp, explicitly out of this row's comparison scope
(transcript-adjacent, not format-law), and is the same category of expected
difference CLAUDE.md already carves out for transcript text.

This run also satisfies F-040's `diff` half: both backends are confirmed
whisper.cpp (Metal + Core ML, Task 18's md5-verified identical weights), and
both produced a complete, well-formed transcript (SRT/VTT/TXT, exit 0, no
errors) from the same input on this run — the "local-backend outcome" F-040
asks for.

## Files

- `humm-out/` — humm's SRT/VTT/TXT for both fixtures.
- `buzz-out/` — Buzz's SRT/VTT/TXT for both fixtures.
- `logs/srt-diff.log` — raw `diff -u` of both fixtures' SRT files (buzz →
  humm), kept for completeness; the text hunks are expected divergence, not
  defects.
