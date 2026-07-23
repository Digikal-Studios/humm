#!/usr/bin/env python3
"""M4 differential harness: srt_equalizer (pinned 0.1.10) resize fixtures (F-086).

Standard library only, plus the two pinned Python clones on `sys.path`; not a
cargo project (`validation/*` is excluded from the workspace, see root
``Cargo.toml``).

Replicates Buzz's *exact* call pattern for the resize button
(``transcription_resizer_widget.py:360-396``, ``on_resize_button_clicked``):

* Build ``srt.Subtitle(index=<id>, start=<ms int>, end=<ms int>,
  content=<text>)`` per segment -- Buzz passes plain millisecond **ints**
  (not ``timedelta``s, despite ``srt.Subtitle``'s docstring) straight from
  its DB rows; ``srt.py``'s ``Subtitle`` does no type coercion in
  ``__init__``, so the arithmetic downstream is identical either way.
* Call ``srt_equalizer.split_subtitle(sub=sub, target_chars=target,
  start_from_index=last_index, method="punctuation")`` per segment, chaining
  ``last_index = new_subs[-1].index`` across segments (index numbering only;
  never read by any other computation in ``split_subtitle`` -- see
  ``srt_equalizer.py:37-90``).
* Round every resulting boundary with Python's own builtin ``round()``
  (round-half-to-even for floats) and drop rows where the rounded start
  equals the rounded end.

This is ground truth for ``crates/humm-core/src/resize.rs::equalize``; the
Rust unit test ``equalize_matches_python_reference_fixtures`` asserts exact
equality against every fixture this script writes.

Usage:
    python3 gen_resize_fixtures.py \\
        --srt-repo /path/to/srt \\
        --srt-equalizer-src /path/to/srt_equalizer/src

Both default to the scratch clones this task was pinned to (v0.1.10 tag);
override them if regenerating from a fresh checkout elsewhere. Regeneration
is a documented, occasional step -- these are pinned reference libraries,
not a build dependency -- so the committed JSON under
``crates/humm-core/tests/fixtures/resize/`` is what CI/`cargo test` actually
reads.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# ---------------------------------------------------------------------------
# Fixture transcripts -- >=6 categories x target_chars {20, 42}, per the task
# brief (short / no-punct / long-run-on / CJK-punctuation / 42-boundary /
# one-word-over-target).
# ---------------------------------------------------------------------------

_BOUNDARY_BASE = (
    "the quick brown fox jumps over the lazy dog and runs far into the deep "
    "dark forest without ever once stopping to look back at what it left behind"
)


def _exact_len(n: int) -> str:
    """A deterministic string of exactly `n` characters, sliced from a long
    base sentence (word boundaries don't matter here -- only the exact
    character count does, to pin the `<= target_chars` boundary check)."""
    assert n <= len(_BOUNDARY_BASE), "base sentence too short for requested length"
    return _BOUNDARY_BASE[:n]


FIXTURES: dict[str, list[dict]] = {
    "short": [
        {"start": 0, "end": 1200, "text": "Hello there."},
        {"start": 1200, "end": 2600, "text": "How are you?"},
        {"start": 2600, "end": 4000, "text": "I am fine."},
    ],
    "no-punct": [
        {
            "start": 0,
            "end": 6000,
            "text": "the quick brown fox jumps over the lazy dog while running "
            "through the forest without stopping",
        },
        {
            "start": 6000,
            "end": 11000,
            "text": "and then it kept going and going and going until it finally "
            "reached the other side of the field",
        },
    ],
    "long-run-on": [
        {
            "start": 0,
            "end": 15000,
            "text": "This is a very long run-on sentence, and it keeps going, and "
            "going, and going, without ever really stopping, which makes it "
            "perfect for testing, the coalesce and split behavior, of the "
            "punctuation splitter, under real conditions, and even more text "
            "to push well past the limit for good measure.",
        },
    ],
    "cjk-punctuation": [
        {
            "start": 0,
            "end": 9000,
            "text": "你好世界，这是一个测试句子。这句话包含中文标点符号！你觉得如何？"
            "这些标点、逗号和句号都不会被切分，因为它们不是英文标点。",
        },
        {
            "start": 9000,
            "end": 14000,
            "text": "Mixed content, with an ASCII comma, that should split, "
            "alongside 中文内容和标点。",
        },
    ],
    "42-boundary": [
        {"start": 0, "end": 2000, "text": _exact_len(19)},
        {"start": 2000, "end": 4000, "text": _exact_len(20)},
        {"start": 4000, "end": 6000, "text": _exact_len(21)},
        {"start": 6000, "end": 9000, "text": _exact_len(41)},
        {"start": 9000, "end": 12000, "text": _exact_len(42)},
        {"start": 12000, "end": 15000, "text": _exact_len(43)},
    ],
    "one-word-over-target": [
        {"start": 0, "end": 1000, "text": "Short intro."},
        {"start": 1000, "end": 5000, "text": "Pneumonoultramicroscopicsilicovolcanoconiosis."},
    ],
}

TARGETS = (20, 42)


def generate(srt_repo: Path, srt_equalizer_src: Path, out_dir: Path) -> None:
    sys.path.insert(0, str(srt_repo))
    sys.path.insert(0, str(srt_equalizer_src))
    import srt  # noqa: E402  (path-dependent import)
    from srt_equalizer import split_subtitle  # noqa: E402

    out_dir.mkdir(parents=True, exist_ok=True)

    for name, segments in FIXTURES.items():
        for target in TARGETS:
            subs = [
                srt.Subtitle(index=i + 1, start=seg["start"], end=seg["end"], content=seg["text"])
                for i, seg in enumerate(segments)
            ]

            resized_subs = []
            last_index = 0
            for sub in subs:
                new_subs = split_subtitle(
                    sub=sub, target_chars=target, start_from_index=last_index, method="punctuation"
                )
                last_index = new_subs[-1].index
                resized_subs.extend(new_subs)

            expected = [
                {"start": round(sub.start), "end": round(sub.end), "text": sub.content}
                for sub in resized_subs
                if round(sub.start) != round(sub.end)
            ]

            payload = {"input": segments, "target": target, "expected": expected}

            out_path = out_dir / f"{name}-t{target}.json"
            out_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            print(f"wrote {out_path} ({len(segments)} input seg(s) -> {len(expected)} output seg(s))")


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--srt-repo",
        type=Path,
        default=Path(
            "/private/tmp/claude-501/-Users-jawad-RustroverProjects-humm--claude-worktrees-m4-transcript-viewer-5aa260"
            "/37a55b77-4cfe-4ba6-b15d-21eee080009b/scratchpad/srt"
        ),
        help="Directory containing srt.py (the pinned `srt` clone root).",
    )
    parser.add_argument(
        "--srt-equalizer-src",
        type=Path,
        default=Path(
            "/private/tmp/claude-501/-Users-jawad-RustroverProjects-humm--claude-worktrees-m4-transcript-viewer-5aa260"
            "/37a55b77-4cfe-4ba6-b15d-21eee080009b/scratchpad/srt_equalizer/src"
        ),
        help="Directory containing the srt_equalizer/ package (pinned v0.1.10 clone's `src/`).",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=repo_root / "crates/humm-core/tests/fixtures/resize",
        help="Where to write the <name>-t<target>.json fixtures.",
    )
    args = parser.parse_args()

    if not (args.srt_repo / "srt.py").is_file():
        parser.error(f"--srt-repo {args.srt_repo} does not contain srt.py")
    if not (args.srt_equalizer_src / "srt_equalizer" / "srt_equalizer.py").is_file():
        parser.error(f"--srt-equalizer-src {args.srt_equalizer_src} does not contain srt_equalizer/srt_equalizer.py")

    generate(args.srt_repo, args.srt_equalizer_src, args.out_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
