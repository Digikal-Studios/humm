#!/usr/bin/env python3
"""M5 differential harness: `live::chunker`'s silence-cut/RMS fixtures (F-061).

Standard library + numpy only (no Buzz/PyQt import -- Buzz is unrunnable on
this machine per `docs/analysis` M1 notes, and neither static method touches
Qt anyway); not a cargo project (`validation/*` is excluded from the
workspace, see root `Cargo.toml`).

The two functions below are copied **verbatim** (logic unchanged, only
`self` dropped since these are `@staticmethod`s in Buzz) from
`buzz/transcriber/recording_transcriber.py:399-433`
(`RecordingTranscriber.find_silence_cut_point`, lines 399-424, and
`RecordingTranscriber.amplitude`, lines 426-427). This is ground truth for
`crates/humm-core/src/live/chunker.rs`'s `find_silence_cut_point`/`rms`; the
Rust unit test `chunker_matches_python_reference_fixtures` asserts exact
equality (cut index) / near-exact equality (RMS, see the numeric-parity note
below) against every fixture this script writes.

Numeric-parity note: Buzz's queue is a `float32` numpy array end to end, so
its real `np.mean`/`np.sqrt` calls run at `float32` precision internally
(confirmed empirically: `np.mean(np.float32([...])**2).dtype` is `float32`,
*not* auto-promoted to `float64`, contrary to a casual read of the numpy
docs). `crates/humm-core`'s Rust port deliberately computes RMS in `f64`
from `f32` samples instead (see `chunker.rs`'s doc comments) -- a ~1e-7
relative difference from Buzz's literal float32 arithmetic that is immaterial
to *which window* gets selected as the cut point, given the 2x margin the
0.5x-of-mean threshold provides against the synthetic vectors below (loud
vs. quiet blocks differ by 100-500x in amplitude, not 1e-7). To keep the
fixtures a genuine differential check of the Rust port rather than of numpy's
dtype-promotion quirk, this harness feeds the copied functions **float64**
arrays throughout (`np.asarray(samples, dtype=np.float64)`) -- i.e. it
reproduces Buzz's cut-point/RMS *math*, not its incidental float32-precision
loss.

Usage:
    python3 gen_chunker_fixtures.py [--out-dir DIR]

Regeneration is a documented, occasional step (these vectors are fully
closed-form/deterministic, not drawn from any external corpus) -- the
committed JSON under `crates/humm-core/tests/fixtures/live/` is what
`cargo test` actually reads. Requires a `python3` with numpy on `sys.path`
(this machine's default `python3` has numpy 1.21.5; if not, try
`/opt/anaconda3/bin/python3` or `conda run python3`).
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

# ---------------------------------------------------------------------------
# Copied verbatim from buzz/transcriber/recording_transcriber.py:399-433
# (RecordingTranscriber.find_silence_cut_point / .amplitude, both
# @staticmethod -- `self` dropped, everything else byte-for-byte identical).
# ---------------------------------------------------------------------------


def find_silence_cut_point(
    samples: np.ndarray,
    sample_rate: int,
    search_seconds: float = 1.5,
    window_seconds: float = 0.02,
    silence_ratio: float = 0.5,
) -> int:
    """Return index of the last quiet point in the final search_seconds of samples.

    Scans backwards through short windows; returns the midpoint of the rightmost
    window whose RMS is below silence_ratio * mean_rms of the search region.
    Falls back to len(samples) if no quiet window is found.
    """
    window = int(window_seconds * sample_rate)
    search_start = max(0, len(samples) - int(search_seconds * sample_rate))
    region = samples[search_start:]
    n_windows = (len(region) - window) // window
    if n_windows < 1:
        return len(samples)

    energies = np.array(
        [np.sqrt(np.mean(region[i * window : (i + 1) * window] ** 2)) for i in range(n_windows)]
    )
    mean_energy = energies.mean()
    threshold = silence_ratio * mean_energy

    for i in range(n_windows - 1, -1, -1):
        if energies[i] < threshold:
            cut = search_start + i * window + window // 2
            return cut

    return len(samples)


def amplitude(arr: np.ndarray):
    return float(np.sqrt(np.mean(arr**2)))


# ---------------------------------------------------------------------------
# Fixture vectors -- deterministic, closed-form (no RNG/seed needed).
# ---------------------------------------------------------------------------


def _blocks(*pieces: tuple[float, int]) -> np.ndarray:
    """Concatenates `(amplitude, count)` pieces into one float64 array."""
    return np.concatenate([np.full(count, value, dtype=np.float64) for value, count in pieces])


FIXTURES: dict[str, dict] = {
    # 1.6s @ 16kHz total: a 1_600-sample loud prefix that falls entirely
    # *outside* the 1.5s search region (`search_start` = 1_600 exactly, so
    # `region` = samples[1_600:] is the remaining 24_000 samples) -- its
    # value never matters to the output beyond its length, since the
    # algorithm never looks at anything before `search_start`; kept loud
    # only for realism. Within the region: loud 16_000, quiet 8_000.
    # The quiet tail crosses fully to the end, so every window from the
    # boundary onward is quiet -- exercises the plain "rightmost quiet
    # window is the last one" path.
    "loud-then-quiet-tail": {
        "sample_rate": 16_000,
        "samples": _blocks((0.5, 1_600), (0.5, 16_000), (0.0001, 8_000)),
    },
    # Same 1_600-sample ignored prefix; within the region: quiet 8_000, loud
    # 16_000. Exercises scanning backward *past* a loud tail to find an
    # earlier quiet window.
    "quiet-then-loud-tail": {
        "sample_rate": 16_000,
        "samples": _blocks((0.5, 1_600), (0.0001, 8_000), (0.5, 16_000)),
    },
    # Same ignored prefix; within the region: medium 8_000, quiet 16_000 --
    # three amplitude tiers (loud prefix / medium / quiet) instead of a
    # single loud/quiet binary, exercising the mean/threshold computation
    # over non-binary window energies.
    "three-tier-mixed": {
        "sample_rate": 16_000,
        "samples": _blocks((0.5, 1_600), (0.05, 8_000), (0.001, 16_000)),
    },
    # 2.0s @ 2000Hz, uniform amplitude: every window's RMS equals the region
    # mean exactly, so none is below the 0.5x threshold -- fallback to
    # len(samples).
    "all-loud-fallback": {
        "sample_rate": 2_000,
        "samples": _blocks((0.5, 4_000)),
    },
    # 15 samples @ 2000Hz: shorter than one window (window = int(0.02*2000)
    # = 40) -- fallback via the "region shorter than window" path.
    "sub-window-fallback": {
        "sample_rate": 2_000,
        "samples": _blocks((0.3, 15)),
    },
    # 60 samples @ 2000Hz: >= one window (40) but < two (80), so
    # n_windows = (60-40)//40 = 0 -- fallback via the "n_windows < 1" path,
    # distinct from the shorter-than-one-window case above.
    "less-than-two-windows-fallback": {
        "sample_rate": 2_000,
        "samples": _blocks((0.3, 60)),
    },
}


def generate(out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    for name, spec in FIXTURES.items():
        rate = spec["sample_rate"]
        samples: np.ndarray = spec["samples"]

        cut = find_silence_cut_point(samples, rate)
        rms_value = amplitude(samples)

        payload = {
            "name": name,
            "sample_rate": rate,
            "samples": samples.tolist(),
            "expected_cut": int(cut),
            "expected_rms": rms_value,
        }

        # Compact (no per-number indentation): these are data fixtures, not
        # hand-read/hand-edited files, and a handful of the vectors below run
        # to tens of thousands of samples -- pretty-printing one float per
        # line would bloat a handful of KB of real fixture data into
        # megabytes of committed JSON for no readability benefit.
        out_path = out_dir / f"{name}.json"
        out_path.write_text(json.dumps(payload, separators=(",", ":")) + "\n", encoding="utf-8")
        print(f"wrote {out_path} ({len(samples)} samples @ {rate}Hz -> cut={cut}, rms={rms_value:.6g})")


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=repo_root / "crates/humm-core/tests/fixtures/live",
        help="Where to write the <name>.json fixtures.",
    )
    args = parser.parse_args()

    generate(args.out_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
