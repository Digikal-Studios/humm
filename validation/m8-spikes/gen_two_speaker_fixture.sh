#!/usr/bin/env bash
# Regenerates crates/humm-core/tests/fixtures/diarize/two-speakers.wav.
#
# Splice: 15s of audio-long.mp3 (speaker A) + the full whisper-latvian.wav
# (speaker B) + 15s of audio-long.mp3 offset 30s (speaker A again), all
# resampled to 16kHz mono 16-bit PCM WAV. See validation/m8-spikes/README.md
# ("2-speaker fixture") for why whisper-latvian.wav is used instead of the
# whisper-french.mp3 named in the original task brief, and for the exact
# splice timestamps / expected speaker-change boundaries.
#
# Byte-reproducibility: verified byte-identical across two consecutive runs
# on this machine (ffmpeg 8.1.1, macOS aarch64). ffmpeg's mp3 decode is not
# guaranteed bit-identical across ffmpeg versions/builds (different libmp3lame/
# libavcodec MP3 decoder revisions can round differently) — decoder-version
# drift is the known way this script could reproduce a fixture that is
# *equivalent* (same splice points, same speaker content) but not
# byte-identical to the committed file. If that happens, re-run this script
# and re-commit the regenerated file; nothing downstream depends on the exact
# bytes, only on the splice timestamps recorded in the README.
#
# Usage: validation/m8-spikes/gen_two_speaker_fixture.sh [output_path]
#   HUMM_BUZZ_TESTDATA overrides the Buzz testdata root (Global Constraint 4).

set -euo pipefail

TESTDATA="${HUMM_BUZZ_TESTDATA:-/Users/jawad/RustroverProjects/buzz/testdata}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUT_PATH="${1:-$REPO_ROOT/crates/humm-core/tests/fixtures/diarize/two-speakers.wav}"

AUDIO_LONG="$TESTDATA/audio-long.mp3"
WHISPER_LATVIAN="$TESTDATA/whisper-latvian.wav"

for f in "$AUDIO_LONG" "$WHISPER_LATVIAN"; do
  if [ ! -f "$f" ]; then
    echo "error: missing Buzz testdata file: $f (set HUMM_BUZZ_TESTDATA)" >&2
    exit 1
  fi
done

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "error: ffmpeg not found on PATH" >&2
  exit 1
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/humm-two-speaker-fixture.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# Segment 1: speaker A, [0s, 15s) of audio-long.mp3
ffmpeg -y -loglevel error -i "$AUDIO_LONG" -ss 0 -t 15 \
  -ar 16000 -ac 1 -c:a pcm_s16le "$WORK/seg1.wav"

# Segment 2: speaker B, the full whisper-latvian.wav (~7.19s)
ffmpeg -y -loglevel error -i "$WHISPER_LATVIAN" \
  -ar 16000 -ac 1 -c:a pcm_s16le "$WORK/seg2.wav"

# Segment 3: speaker A again, [30s, 45s) of audio-long.mp3
ffmpeg -y -loglevel error -i "$AUDIO_LONG" -ss 30 -t 15 \
  -ar 16000 -ac 1 -c:a pcm_s16le "$WORK/seg3.wav"

printf "file 'seg1.wav'\nfile 'seg2.wav'\nfile 'seg3.wav'\n" > "$WORK/concat_list.txt"

mkdir -p "$(dirname "$OUT_PATH")"
ffmpeg -y -loglevel error -f concat -safe 0 -i "$WORK/concat_list.txt" \
  -ar 16000 -ac 1 -c:a pcm_s16le "$OUT_PATH"

echo "wrote $OUT_PATH"
ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1 "$OUT_PATH"
