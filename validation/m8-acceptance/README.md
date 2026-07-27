# M8 acceptance artifacts — outcome diffs vs Buzz, raw evidence

Companion to `docs/gui-manual/M8.md` (gui-manual v6). Everything here was
produced on 2026-07-27, Apple M4 / macOS 26.5.2, release binaries built from
this branch at `2f82a1e`, sandbox-lifted, every run under `caffeinate -dimsu`
with isolated `HUMM_DATA_DIR`/`HUMM_CONFIG_DIR`/`HUMM_LOG_DIR` (and
`HUMM_MODEL_ROOT` only where a run deliberately needed an empty or scratch
model root — otherwise the real cached models were used, exactly like a real
launch).

Contents:

| Path | What it is |
|---|---|
| `shots/01..43-*.png` | Windowed screenshots referenced by number (`[NN]`) from `docs/gui-manual/M8.md`; the filenames say what each one shows. |
| `logs/humm-media-contract-ignored.log` | The full `cargo test -p humm-media --test contract -- --ignored` run (real ffmpeg + real yt-dlp + real demucs). |
| `logs/outcome-measurements.txt` | ffmpeg `astats` numbers behind §1 and §2 below. |
| `logs/cli-extract-speech-without-demucs.log` | CLI `-e` graceful-absence message. |
| `logs/cli-corrupt-input.log` | CLI corrupt-input failure message. |

## The bar these diffs are held to (ADR-011)

F-027 and F-087 are **outcome-level** parity rows, not implementation parity:
humm runs demucs as a subprocess where Buzz vendored a fork of its Python API,
and humm diarizes with `ort`/community-1 models where Buzz drove NeMo MSDD +
forced alignment + punctuation restoration. So the diff below asserts
*counts, boundaries and artifacts* — "speech was isolated", "two speakers were
found, at the right boundary, and are nameable/mergeable", "noise went down",
"the language was detected and written back" — and never label equality or
transcript-text equality against Buzz. F-112 and F-114 are ordinary
unit+contract rows; their acceptance halves are the same outcome checks on
fixture media.

## 1. Speech isolated (F-027)

Fixture built for the run (`ffmpeg`): `whisper-french.mp3` (8.5 s of French
speech) mixed with two sustained sines — 220 Hz with a 4 Hz tremolo and
330 Hz — at 0.35 gain, as a stand-in for musical accompaniment
(`speech-plus-music.wav`).

The artifact measured is **humm's own** `_speech.mp3`, copied out of the queue
by a 0.2 s poller while the task was between isolation and cleanup — not a
hand-run demucs invocation. Producer: `humm add speech-plus-music.wav -e
-s tiny --txt` with `HUMM_DEMUCS` pointing at a wrapper for
`uvx --from demucs==4.0.1 --with "numpy<2" demucs`.

| Band | Input | humm's `_speech.mp3` | Δ |
|---|---|---|---|
| full | −25.07 dB | −26.73 dB | −1.66 dB |
| 220 Hz (tone 1, ±8 Hz) | −33.88 dB | −38.48 dB | **−4.60 dB** |
| 330 Hz (tone 2, ±8 Hz) | −32.65 dB | −45.14 dB | **−12.49 dB** |
| 3000 Hz (speech band) | −63.99 dB | −63.91 dB | +0.08 dB (unchanged) |

Both injected tones are attenuated while the speech band is left alone — the
isolated file is voice-dominant relative to its input, which is the outcome
claim. (*Task 19 sweep: the two bold Δs were each 0.01 dB off — they had been
computed from the already-rounded column values rather than from the raw
`astats` readings in `logs/outcome-measurements.txt`. Recomputed from the raw
numbers: −33.878788 → −38.483626 = −4.604838 and −32.648366 → −45.143009 =
−12.494643. The one-decimal figures quoted elsewhere, 4.6 dB and 12.5 dB, were
unaffected.*) The transcript produced *from the isolated audio* is intelligible
French: *"Bienvenue dans PassRail, un podcast, pensé pour éveiller la curiosité
des apprenances et des apprenances de français."*

Structural half (naming, containers, durations) is covered by the real-demucs
contract test in `logs/humm-media-contract-ignored.log`
(`real_demucs_isolates_vocals_from_whisper_french` — asserts the
`<stem>_speech.mp3` name, that the output probes as real audio, and that its
duration matches the input).

Windowed lifecycle evidence (shots 01–04): the options-sheet checkbox, the
`isolating speech` status text, `whisper-french_speech.mp3` appearing beside
the input at 16:14:39 and gone by 16:14:43, and the export named from the
**original** stem (`whisper-french (transcribed on …).txt/.srt`, never
`whisper-french_speech …`).

## 2. Noise reduced (F-112)

Fixture: `whisper-french.mp3` + white noise (`anoisesrc` amplitude 0.06),
mixed without normalisation. The artifact measured is humm's own
`french-noisy2_DeepFilterNet3.wav`, left on disk by the **keep-file** variant
(Preferences → Post-processing → *Keep denoised file after transcription*),
so the file is exactly what the hook handed the transcriber.

| Metric | Input | Denoised | Δ |
|---|---|---|---|
| RMS level | −22.27 dB | −24.10 dB | −1.83 dB |
| Noise floor | −22.88 dB | **−inf** | floor collapsed |
| Peak level | −4.94 dB | −5.89 dB | −0.95 dB |

Broadband floor removed while peaks (the speech) survive within 1 dB. Buzz's
own plugin makes no stronger claim than "the denoised file is what gets
transcribed"; humm additionally never lets the artifact reach an export name
or a DB row (ledger #4) — verified in the same run: the export is
`french-noisy (transcribed on …).txt` and the store row's `file` column is the
original `.wav`.

## 3. Speakers found, nameable, mergeable (F-087)

Fixture: the committed `crates/humm-core/tests/fixtures/diarize/two-speakers.wav`
— speaker A `[0, 15)`, speaker B `[15, 22.19)`, speaker A again
`[22.19, 37.19]` by construction (`validation/m8-spikes/gen_two_speaker_fixture.sh`).

The whole J-7 journey ran in a real window against the real cached diarization
models (shots 12–16):

- **Found:** Step 2 listed exactly **two** speakers (`Speaker 0`, `Speaker 1`)
  — the fixture's own count.
- **Boundary:** the saved transcript's speaker change lands at **22 210 ms**,
  against the fixture's constructed change at **22 190 ms** — i.e. on the
  transcript's own nearest segment boundary. (The 15 000 ms change is not
  observable in this transcript: whisper produced no segment before
  15 430 ms for this fixture, so there is nothing there to label. Recorded as
  observed, not claimed as a hit.)
- **Nameable:** both rows renamed by real keystrokes (`Narrator`, `Baiba`) and
  the names appear verbatim in the saved transcript's segment text.
- **Mergeable:** with *Merge speaker sentences* checked, 4 source segments
  became 2 — consecutive same-speaker segments merged, `22 210 → 37 191` in
  one row.
- **Play sample** opened a real CoreAudio output stream (`sample` showed live
  `humm-audio-output`, `cpal-coreaudio-default-output` and
  `com.apple.audio.IOThread.client` threads during the click); whether sound
  was *audible in the room* stays a human-judgment item.

Source segments (store, transcript 1) vs saved labelled transcript (2):

```
1: 15430–22210  "Mani uztrauc laikapstākļu, tapēc uz jūru es diezvai braucu."
1: 22210–27090  "Andra kāli minūte, pūrk un rēpļišis esam, aiz kādāk."
1: 27090–32900  "Dā kā kā kādāk, esmu esmu vispārībā…"
1: 32900–37191  "Mani būtu, ka esmu ir parībāk…"
2: 15430–22210  "Narrator: Mani uztrauc laikapstākļu…"
2: 22210–37191  "Baiba: Andra kāli minūte…"
```

Never claimed: that humm's speaker *labels* match Buzz's, or that the
transcript text matches — ADR-011 explicitly excludes both.

## 4. Language detected (F-114)

French fixture, options-sheet language left on **Detect Language**, the
Enhanced-language-detection hook enabled from Preferences by a real click.
Observed end to end in one windowed run:

- the store row carried `language = 'fr'` **while the task was still
  `in_progress`** — i.e. detection happened in the pre-transcription hook, not
  as a by-product of the transcription;
- with the export template set (by typing it into Preferences → General →
  *Default export file name*) to `{{ input_file_name }}-{{ language }}`, the
  export file written was **`whisper-french-fr.txt`** — the detected code
  reaching the export *name*, which is the half the F-114 matrix row said was
  still owed.

## Reproducing

```bash
export CARGO_HOME="$PWD/.cargo-home"
cargo build --release -p humm-gui -p humm-cli

# 1. Speech isolation, real demucs, artifact captured mid-run
cat > /tmp/demucs-wrapper.sh <<'EOF'
#!/bin/sh
exec uvx --from demucs==4.0.1 --with "numpy<2" demucs "$@"
EOF
chmod +x /tmp/demucs-wrapper.sh
ffmpeg -i whisper-french.mp3 -f lavfi -i "sine=frequency=220:duration=8.5,tremolo=f=4:d=0.6" \
  -f lavfi -i "sine=frequency=330:duration=8.5" \
  -filter_complex "[0:a]aresample=44100[s];[1:a][2:a]amix=inputs=2:normalize=0,volume=0.35[m];[s][m]amix=inputs=2:duration=first:normalize=0[out]" \
  -map "[out]" -ar 44100 -ac 2 speech-plus-music.wav
( while :; do [ -f speech-plus-music_speech.mp3 ] && cp speech-plus-music_speech.mp3 captured.mp3 && break; sleep 0.2; done ) &
HUMM_DEMUCS=/tmp/demucs-wrapper.sh ./target/release/humm add speech-plus-music.wav -e -s tiny --txt
ffmpeg -i captured.mp3 -af "bandpass=f=330:width_type=h:w=8,astats" -f null /dev/null   # etc.

# 2. Contract suite against the real binaries (sandbox-lifted)
HUMM_DEMUCS_CONTRACT=1 HUMM_DEMUCS=/tmp/demucs-wrapper.sh \
  cargo test -p humm-media --test contract -- --ignored --test-threads=1

# 3. Windowed runs: see docs/gui-manual/M8.md §0 for the launch recipe and
#    the CGEvent/screencapture automation used to drive them.
```
