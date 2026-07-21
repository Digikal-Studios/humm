#!/usr/bin/env python3
"""M2 differential harness: humm vs Buzz `add` artifacts (F-045 diff note).

Standard library only; not a cargo project (`validation/*` is excluded from
the workspace, see root ``Cargo.toml``).

Ground truth for the SRT/VTT/TXT *byte-format law* is Buzz's own golden test
fixtures, ported verbatim into ``crates/humm-core/src/export.rs`` (test
module, ``tests/transcriber/transcriber_test.py:12-43`` upstream). This
script's reference formatter (``to_timestamp``/``format_txt``/``format_srt``/
``format_vtt``) is a line-for-line mirror of that Rust code, and
``run_self_tests()`` asserts it reproduces those exact golden strings before
it is trusted to judge anything else.

Two modes:

* **Full mode** -- if ``--buzz-dir`` contains real Buzz-generated TXT/SRT/VTT
  artifacts, compares humm vs Buzz per the milestone-acceptance contract:
  structural skeleton identity (both sides must parse under the same format
  law), segment counts within +/-20%, matched-by-order segment start/end
  within +/-200 ms, and transcript-text similarity (informational only --
  text may legitimately differ between engines/backends).

* **Fallback (structural) mode** -- used when no Buzz artifacts are present.
  Validates humm's own SRT/VTT/TXT output against the byte-format law only:
  each format parses, round-trips byte-identically through the reference
  formatter, and is cross-consistent with the others (same segments/timings
  across SRT and VTT, TXT reconstructable from the SRT-derived segments).
  This is the documented fallback for when Buzz could not be produced within
  the task's 30-minute time-box -- see README.md for what blocked it.

Exit code: 0 if all checks pass, 1 otherwise.
"""

from __future__ import annotations

import argparse
import difflib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# ---------------------------------------------------------------------------
# Reference formatter -- mirrors crates/humm-core/src/export.rs exactly.
# ---------------------------------------------------------------------------

DEFAULT_PARAGRAPH_SPLIT_MS = 2000


def to_timestamp(ms: int, sep: str) -> str:
    """Mirrors `export.rs::to_timestamp`. HH grows past 2 digits, never wraps."""
    hr = ms // (1000 * 60 * 60)
    rem = ms - hr * (1000 * 60 * 60)
    minute = rem // (1000 * 60)
    rem = rem - minute * (1000 * 60)
    sec = rem // 1000
    ms_part = rem - sec * 1000
    return f"{hr:02d}:{minute:02d}:{sec:02d}{sep}{ms_part:03d}"


def format_txt(segments: list[dict], key: str, paragraph_split_ms: int = DEFAULT_PARAGRAPH_SPLIT_MS) -> str:
    """Mirrors `export.rs::write_txt`."""
    out = []
    previous_end = None
    for seg in segments:
        if previous_end is not None and seg["start"] - previous_end >= paragraph_split_ms:
            out.append("\n\n")
        out.append(f"{seg[key].strip()} ")
        previous_end = seg["end"]
    return "".join(out)


def format_srt(segments: list[dict], key: str) -> str:
    """Mirrors `export.rs::write_srt`. Text is raw/untrimmed."""
    out = []
    for i, seg in enumerate(segments):
        out.append(f"{i + 1}\n")
        out.append(f"{to_timestamp(seg['start'], ',')} --> {to_timestamp(seg['end'], ',')}\n")
        out.append(f"{seg[key]}\n")
        out.append("\n")
    return "".join(out)


def format_vtt(segments: list[dict], key: str) -> str:
    """Mirrors `export.rs::write_vtt`. Text is raw/untrimmed."""
    out = ["WEBVTT\n", "\n"]
    for seg in segments:
        out.append(f"{to_timestamp(seg['start'], '.')} --> {to_timestamp(seg['end'], '.')}\n")
        out.append(f"{seg[key]}\n")
        out.append("\n")
    return "".join(out)


def run_self_tests() -> None:
    """Asserts the reference formatter reproduces Buzz's golden strings
    verbatim, ported from `export.rs`'s test module (which itself ports
    `tests/transcriber/transcriber_test.py:12-43`). If this fails, the
    reference formatter has drifted from ground truth and nothing below can
    be trusted."""

    assert to_timestamp(0, ".") == "00:00:00.000"
    assert to_timestamp(123_456_789, ".") == "34:17:36.789"

    fixture = [
        {"start": 40, "end": 299, "text": "Bien", "translation": ""},
        {"start": 299, "end": 329, "text": "venue dans", "translation": ""},
    ]
    assert format_txt(fixture, "text") == "Bien venue dans "
    assert format_srt(fixture, "text") == (
        "1\n00:00:00,040 --> 00:00:00,299\nBien\n\n"
        "2\n00:00:00,299 --> 00:00:00,329\nvenue dans\n\n"
    )
    assert format_vtt(fixture, "text") == (
        "WEBVTT\n\n00:00:00.040 --> 00:00:00.299\nBien\n\n"
        "00:00:00.299 --> 00:00:00.329\nvenue dans\n\n"
    )

    trans_fixture = [
        {"start": 40, "end": 299, "text": "Bien", "translation": "Wel"},
        {"start": 299, "end": 329, "text": "venue dans", "translation": "come in"},
    ]
    assert format_srt(trans_fixture, "translation") == (
        "1\n00:00:00,040 --> 00:00:00,299\nWel\n\n"
        "2\n00:00:00,299 --> 00:00:00,329\ncome in\n\n"
    )

    assert format_txt([], "text") == ""
    assert format_srt([], "text") == ""
    assert format_vtt([], "text") == "WEBVTT\n\n"

    untrimmed = [{"start": 0, "end": 100, "text": " hi ", "translation": ""}]
    assert format_srt(untrimmed, "text") == "1\n00:00:00,000 --> 00:00:00,100\n hi \n\n"
    assert format_txt(untrimmed, "text") == "hi "

    gap = [
        {"start": 0, "end": 1000, "text": "a", "translation": ""},
        {"start": 3000, "end": 4000, "text": "b", "translation": ""},
    ]
    assert format_txt(gap, "text", 2000) == "a \n\nb "

    nogap = [
        {"start": 0, "end": 1000, "text": "a", "translation": ""},
        {"start": 2999, "end": 4000, "text": "b", "translation": ""},
    ]
    assert format_txt(nogap, "text", 2000) == "a b "


# ---------------------------------------------------------------------------
# Parsers -- read a real SRT/VTT file back into segments, enforcing the same
# format law the reference formatter writes (sequential numbering, the
# WEBVTT header, comma vs dot separators).
# ---------------------------------------------------------------------------

_SRT_BLOCK_RE = re.compile(
    r"(?P<index>\d+)\n"
    r"(?P<start>\d{2,}:\d{2}:\d{2},\d{3}) --> (?P<end>\d{2,}:\d{2}:\d{2},\d{3})\n"
    r"(?P<text>.*?)\n\n",
    re.DOTALL,
)

_VTT_BLOCK_RE = re.compile(
    r"(?P<start>\d{2,}:\d{2}:\d{2}\.\d{3}) --> (?P<end>\d{2,}:\d{2}:\d{2}\.\d{3})\n"
    r"(?P<text>.*?)\n\n",
    re.DOTALL,
)


def _parse_ts(s: str, sep: str) -> int:
    hms, frac = s.split(sep)
    hh, mm, ss = hms.split(":")
    return int(hh) * 3_600_000 + int(mm) * 60_000 + int(ss) * 1000 + int(frac)


def parse_srt(content: str) -> list[dict]:
    segments = []
    for i, m in enumerate(_SRT_BLOCK_RE.finditer(content)):
        idx = int(m.group("index"))
        if idx != i + 1:
            raise ValueError(f"SRT numbering not sequential: expected {i + 1}, got {idx}")
        segments.append(
            {
                "start": _parse_ts(m.group("start"), ","),
                "end": _parse_ts(m.group("end"), ","),
                "text": m.group("text"),
                "translation": "",
            }
        )
    if not segments and content.strip():
        raise ValueError("SRT content is non-empty but no block matched the format law")
    return segments


def parse_vtt(content: str) -> list[dict]:
    if not content.startswith("WEBVTT\n\n"):
        raise ValueError("VTT missing 'WEBVTT\\n\\n' header")
    body = content[len("WEBVTT\n\n") :]
    segments = []
    for m in _VTT_BLOCK_RE.finditer(body):
        segments.append(
            {
                "start": _parse_ts(m.group("start"), "."),
                "end": _parse_ts(m.group("end"), "."),
                "text": m.group("text"),
                "translation": "",
            }
        )
    if not segments and body.strip():
        raise ValueError("VTT content beyond header but no block matched the format law")
    return segments


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


class Check:
    def __init__(self, name: str, passed: bool, detail: str):
        self.name = name
        self.passed = passed
        self.detail = detail

    def __str__(self) -> str:
        status = "PASS" if self.passed else "FAIL"
        return f"[{status}] {self.name}: {self.detail}"


def validate_structural(srt_content: str, vtt_content: str, txt_content: str) -> list[Check]:
    """Fallback-mode checks: humm's own artifacts vs the byte-format law."""
    checks: list[Check] = []

    srt_segments = None
    try:
        srt_segments = parse_srt(srt_content)
        checks.append(Check("srt_parses_sequential_numbering", True, f"{len(srt_segments)} segment(s)"))
    except Exception as e:  # noqa: BLE001 - report, don't crash the harness
        checks.append(Check("srt_parses_sequential_numbering", False, str(e)))

    vtt_segments = None
    try:
        vtt_segments = parse_vtt(vtt_content)
        checks.append(Check("vtt_header_and_parses", True, f"{len(vtt_segments)} segment(s)"))
    except Exception as e:  # noqa: BLE001
        checks.append(Check("vtt_header_and_parses", False, str(e)))

    if srt_segments is not None:
        roundtrip = format_srt(srt_segments, "text")
        checks.append(
            Check(
                "srt_roundtrip_byte_identical",
                roundtrip == srt_content,
                "re-formatting the parsed segments reproduces the file byte-for-byte"
                if roundtrip == srt_content
                else f"mismatch: {roundtrip!r} != {srt_content!r}",
            )
        )

    if vtt_segments is not None:
        roundtrip = format_vtt(vtt_segments, "text")
        checks.append(
            Check(
                "vtt_roundtrip_byte_identical",
                roundtrip == vtt_content,
                "re-formatting the parsed segments reproduces the file byte-for-byte"
                if roundtrip == vtt_content
                else f"mismatch: {roundtrip!r} != {vtt_content!r}",
            )
        )

    if srt_segments is not None and vtt_segments is not None:
        checks.append(
            Check(
                "srt_vtt_segment_count_match",
                len(srt_segments) == len(vtt_segments),
                f"srt={len(srt_segments)} vtt={len(vtt_segments)}",
            )
        )
        n = min(len(srt_segments), len(vtt_segments))
        mismatches = [
            i
            for i in range(n)
            if srt_segments[i]["start"] != vtt_segments[i]["start"]
            or srt_segments[i]["end"] != vtt_segments[i]["end"]
            or srt_segments[i]["text"] != vtt_segments[i]["text"]
        ]
        checks.append(
            Check(
                "srt_vtt_segments_identical",
                len(mismatches) == 0,
                "all matched segments identical across formats" if not mismatches else f"mismatched indices: {mismatches}",
            )
        )

        expected_txt = format_txt(srt_segments, "text", DEFAULT_PARAGRAPH_SPLIT_MS)
        checks.append(
            Check(
                "txt_matches_srt_derived_reconstruction",
                expected_txt == txt_content,
                "TXT reconstructed from SRT segments matches the actual TXT file byte-for-byte"
                if expected_txt == txt_content
                else f"mismatch: expected {expected_txt!r}, got {txt_content!r}",
            )
        )

    return checks


def validate_full(
    buzz_srt: str, buzz_vtt: str, buzz_txt: str, humm_srt: str, humm_vtt: str, humm_txt: str
) -> tuple[list[Check], str]:
    """Full-mode checks: humm vs real Buzz artifacts, per the milestone
    acceptance contract. Returns (gating checks, informational text-similarity
    report)."""
    checks: list[Check] = []

    buzz_segments = None
    humm_segments = None
    try:
        buzz_segments = parse_srt(buzz_srt)
        checks.append(Check("buzz_srt_structural_skeleton", True, f"{len(buzz_segments)} segment(s)"))
    except Exception as e:  # noqa: BLE001
        checks.append(Check("buzz_srt_structural_skeleton", False, str(e)))

    try:
        humm_segments = parse_srt(humm_srt)
        checks.append(Check("humm_srt_structural_skeleton", True, f"{len(humm_segments)} segment(s)"))
    except Exception as e:  # noqa: BLE001
        checks.append(Check("humm_srt_structural_skeleton", False, str(e)))

    # Same structural checks applied to VTT (header + parse), skeleton only.
    for label, content in (("buzz_vtt", buzz_vtt), ("humm_vtt", humm_vtt)):
        try:
            parse_vtt(content)
            checks.append(Check(f"{label}_structural_skeleton", True, "parses under format law"))
        except Exception as e:  # noqa: BLE001
            checks.append(Check(f"{label}_structural_skeleton", False, str(e)))

    informational = ""
    if buzz_segments is not None and humm_segments is not None:
        buzz_n, humm_n = len(buzz_segments), len(humm_segments)
        ratio_ok = buzz_n > 0 and abs(humm_n - buzz_n) / buzz_n <= 0.20
        checks.append(
            Check(
                "segment_count_within_20pct",
                ratio_ok,
                f"buzz={buzz_n} humm={humm_n} (delta {abs(humm_n - buzz_n)}, "
                f"{'within' if ratio_ok else 'EXCEEDS'} +/-20%)",
            )
        )

        n = min(buzz_n, humm_n)
        bad = []
        for i in range(n):
            ds = abs(buzz_segments[i]["start"] - humm_segments[i]["start"])
            de = abs(buzz_segments[i]["end"] - humm_segments[i]["end"])
            if ds > 200 or de > 200:
                bad.append((i, ds, de))
        checks.append(
            Check(
                "matched_segment_timing_within_200ms",
                len(bad) == 0,
                "all matched segments within tolerance" if not bad else f"out-of-tolerance indices (idx, d_start_ms, d_end_ms): {bad}",
            )
        )

        buzz_text = " ".join(s["text"].strip() for s in buzz_segments)
        humm_text = " ".join(s["text"].strip() for s in humm_segments)
        ratio = difflib.SequenceMatcher(None, buzz_text, humm_text).ratio()
        informational = f"text similarity (informational only, not gating): {ratio:.1%}"

    return checks, informational


# ---------------------------------------------------------------------------
# humm artifact generation
# ---------------------------------------------------------------------------


def generate_humm_artifacts(humm_bin: Path, audio: Path, model_size: str, out_dir: Path, force_cpu: bool) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    if force_cpu:
        env["HUMM_FORCE_CPU"] = "1"
    cmd = [str(humm_bin), "add", str(audio), "--txt", "--srt", "--vtt", "-s", model_size, "-d", str(out_dir)]
    print(f"+ {' '.join(cmd)}")
    result = subprocess.run(cmd, env=env, capture_output=True, text=True)
    sys.stderr.write(result.stderr)
    if result.returncode != 0:
        raise RuntimeError(f"humm add failed (exit {result.returncode}); see stderr above")
    return out_dir


def find_artifact_triplet(out_dir: Path) -> tuple[Path, Path, Path]:
    txts = sorted(out_dir.glob("*.txt"))
    srts = sorted(out_dir.glob("*.srt"))
    vtts = sorted(out_dir.glob("*.vtt"))
    if not (txts and srts and vtts):
        raise FileNotFoundError(
            f"expected .txt/.srt/.vtt in {out_dir}, found txt={txts} srt={srts} vtt={vtts}"
        )
    return txts[-1], srts[-1], vtts[-1]


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]

    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--humm-bin", type=Path, default=repo_root / "target/debug/humm")
    parser.add_argument(
        "--audio",
        type=Path,
        default=Path("/Users/jawad/RustroverProjects/buzz/testdata/whisper-french.mp3"),
        help="Fixture audio file, same one fed to both humm and Buzz.",
    )
    parser.add_argument("--model-size", default="tiny")
    parser.add_argument(
        "--buzz-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "buzz-artifacts",
        help="Directory with real Buzz TXT/SRT/VTT artifacts, if any were produced.",
    )
    parser.add_argument("--out-dir", type=Path, default=None, help="Where humm writes its artifacts. Default: fresh temp dir.")
    parser.add_argument("--skip-generate", action="store_true", help="Don't invoke humm; --out-dir must already hold artifacts.")
    parser.add_argument("--force-cpu", action="store_true", help="Set HUMM_FORCE_CPU=1 for the humm subprocess.")
    parser.add_argument("--archive-dir", type=Path, default=None, help="Copy the humm artifacts used for comparison here.")
    args = parser.parse_args()

    print("== self-test: reference formatter vs Buzz golden fixtures (export.rs) ==")
    run_self_tests()
    print("self-tests passed: reference formatter matches Buzz's golden strings exactly\n")

    out_dir = args.out_dir or Path(tempfile.mkdtemp(prefix="humm-diff-"))
    if not args.skip_generate:
        generate_humm_artifacts(args.humm_bin, args.audio, args.model_size, out_dir, args.force_cpu)
    humm_txt_path, humm_srt_path, humm_vtt_path = find_artifact_triplet(out_dir)

    if args.archive_dir:
        args.archive_dir.mkdir(parents=True, exist_ok=True)
        for p in (humm_txt_path, humm_srt_path, humm_vtt_path):
            shutil.copy2(p, args.archive_dir / p.name)
        print(f"archived humm artifacts to {args.archive_dir}\n")

    humm_txt = humm_txt_path.read_text()
    humm_srt = humm_srt_path.read_text()
    humm_vtt = humm_vtt_path.read_text()

    buzz_txts = sorted(args.buzz_dir.glob("*.txt")) if args.buzz_dir.is_dir() else []
    buzz_srts = sorted(args.buzz_dir.glob("*.srt")) if args.buzz_dir.is_dir() else []
    buzz_vtts = sorted(args.buzz_dir.glob("*.vtt")) if args.buzz_dir.is_dir() else []

    informational = ""
    if buzz_txts and buzz_srts and buzz_vtts:
        print(f"== FULL MODE: comparing humm vs real Buzz artifacts in {args.buzz_dir} ==")
        buzz_txt = buzz_txts[-1].read_text()
        buzz_srt = buzz_srts[-1].read_text()
        buzz_vtt = buzz_vtts[-1].read_text()
        checks, informational = validate_full(buzz_srt, buzz_vtt, buzz_txt, humm_srt, humm_vtt, humm_txt)
    else:
        print(
            "== FALLBACK (structural) MODE: no Buzz artifacts found "
            f"in {args.buzz_dir} -- validating humm's own output against the "
            "byte-format law (Buzz golden fixtures) only. See README.md for "
            "what blocked real-Buzz artifact generation. F-045 diff stays "
            "pending. ==\n"
        )
        checks = validate_structural(humm_srt, humm_vtt, humm_txt)

    for c in checks:
        print(c)
    if informational:
        print(informational)

    all_passed = all(c.passed for c in checks)
    print(f"\nresult: {'PASS' if all_passed else 'FAIL'} ({sum(c.passed for c in checks)}/{len(checks)} checks)")
    return 0 if all_passed else 1


if __name__ == "__main__":
    sys.exit(main())
