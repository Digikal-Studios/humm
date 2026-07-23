#!/usr/bin/env python3
"""M4 differential harness: stable-ts (pinned 2.19.1) regroup fixtures (F-086).

Ground truth for ``crates/humm-core/src/regroup/`` (the merge half of the
resizer). Runs the **genuine** ``stable_whisper.result`` code from the pinned
clone against Buzz's exact merge call pattern and pins the output the Rust
port must reproduce byte-for-byte.

Buzz pipeline being replicated (``transcription_resizer_widget.py:52-132``):

    DB segments  --get_transcript-->  WhisperResult(list-of-sentence-dicts)
                 --regroup(algo)-->   segments  --int(x*100)-->  Segment(ms)

Each DB "segment" (a word-level row) becomes one stable-ts *word*
``{'word': text + sep, 'start': start_ms/100, 'end': end_ms/100}`` (fake-seconds
= ms/100 -- Buzz's unit quirk). ``sep`` is ``""`` for the six non-spacing
languages else ``" "``. Rows are accumulated into a *sentence* segment each time
``SENTENCE_END = re.compile(r'.*[.!?。！？]')`` ``.match``es the row text; the
trailing partial group is kept. ``.regroup(algo)`` then runs, and each output
segment is re-emitted as ``int(segment.start*100), int(segment.end*100)`` --
truncation toward zero, which for ~6.5% of ms values shifts the boundary by 1ms
(e.g. 29ms -> fake 0.29 -> 28.999...*100 -> int 28). That quirk is intentional
and is exactly what these fixtures capture.

Import strategy (see the ``stubs/`` dir and ``_import_stable_whisper`` below):
``stable_whisper/__init__.py`` eagerly imports whisper/torch, so we bypass it by
pre-seeding ``sys.modules['stable_whisper']`` with a synthetic package whose
``__path__`` points at the real source dir. The torch-dependent internal
submodules the regroup path never executes (``stabilization*``, ``audio*``) are
replaced with tiny sys.modules stubs; ``torch``/``torchaudio``/``tqdm`` come
from ``stubs/`` on ``sys.path``. **numpy and tqdm are real** -- the regroup math
(``get_length_indices`` even-split cumsum/argmin, ``get_gap_indices`` nonzero)
genuinely uses numpy, so it is *not* stubbed (a correction to the task brief's
"pure Python" phrasing, recorded in the M4 report's source-trace notes).

Regeneration is an occasional, documented step -- the committed JSON under
``crates/humm-core/tests/fixtures/regroup/`` is what ``cargo test`` reads.

Usage::

    python3 gen_regroup_fixtures.py \\
        --stable-ts /path/to/stable-ts   # repo root containing stable_whisper/
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import types
from pathlib import Path

# --- Buzz's get_transcript constants (transcription_resizer_widget.py) -------
SENTENCE_END = re.compile(r".*[.!?。！？]")
NON_SPACE_LANGUAGES = {"zh", "ja", "th", "lo", "km", "my"}

# Buzz's full default resizer UI regroup string (all three checkboxes on):
DEFAULT_UI = "mg=0.2++42+1_sp=.* /./. /。/?/? /？/!/! /！/,/, _sl=42"


def _import_stable_whisper(stable_ts_root: Path):
    """Import the real ``stable_whisper.result`` without running the package's
    heavy ``__init__`` or its torch/whisper dependency graph."""
    stubs_dir = Path(__file__).resolve().parent / "stubs"
    sys.path.insert(0, str(stubs_dir))  # torch / torchaudio / tqdm stubs (numpy stays real)

    pkg = types.ModuleType("stable_whisper")
    pkg.__path__ = [str(stable_ts_root / "stable_whisper")]
    sys.modules["stable_whisper"] = pkg  # bypass real __init__.py

    def stub(modname, is_pkg=False, **attrs):
        m = types.ModuleType(modname)
        if is_pkg:
            m.__path__ = []
        for k, v in attrs.items():
            setattr(m, k, v)
        sys.modules[modname] = m
        return m

    # The torch-dependent internal modules the regroup path imports but never runs.
    stub(
        "stable_whisper.stabilization",
        is_pkg=True,
        suppress_silence=lambda *a, **k: None,
        get_vad_silence_func=lambda *a, **k: None,
        VAD_SAMPLE_RATES=[16000],
    )
    stub("stable_whisper.stabilization.nonvad", audio2timings=lambda *a, **k: None)
    stub(
        "stable_whisper.stabilization.utils",
        filter_timings=lambda *a, **k: None,
        valid_ts=lambda *a, **k: None,
    )
    stub("stable_whisper.audio", is_pkg=True)
    stub("stable_whisper.audio.utils", audio_to_tensor_resample=lambda *a, **k: None)

    import stable_whisper.result as result  # noqa: E402  (path-dependent)

    return result


def build_result_dict(rows, language):
    """Buzz's ``TranscriptionWorker.get_transcript`` (lines 52-93), verbatim."""
    is_non_space = (language or "") in NON_SPACE_LANGUAGES
    separator = "" if is_non_space else " "

    segments = []
    words = []
    text = ""
    for row in rows:
        words.append(
            {
                "word": row["text"] + separator,
                "start": row["start"] / 100,
                "end": row["end"] / 100,
            }
        )
        text += row["text"] + separator
        if SENTENCE_END.match(row["text"]):
            segments.append({"text": text, "words": words})
            words = []
            text = ""
    if words:
        segments.append({"text": text, "words": words})

    return {"language": language, "segments": segments}


def run_case(result_mod, rows, language, algo):
    """Full Buzz merge pipeline for one case; returns output segment dicts."""
    result_dict = build_result_dict(rows, language)
    result = result_mod.WhisperResult(result_dict, force_order=False, check_sorted=False)
    result.regroup(algo)
    return [
        {"start": int(seg.start * 100), "end": int(seg.end * 100), "text": seg.text}
        for seg in result.segments
    ]


# ---------------------------------------------------------------------------
# Fixture cases: (name, language, algo, input word-level rows).
# Timings are DB milliseconds; text is one word-level row each. Gaps chosen so
# fake-seconds (ms/100) exercise the intended op (mg=0.2 == 20ms real gap).
# ---------------------------------------------------------------------------

def _words(*specs):
    """specs: (text, start_ms, end_ms) -> row dicts."""
    return [{"start": s, "end": e, "text": t} for (t, s, e) in specs]


CASES = []


def case(name, language, algo, rows):
    CASES.append({"name": name, "language": language, "algo": algo, "rows": rows})


# 1. Full default UI string over a multi-sentence transcript. Sentence 1 -> 2
#    gap is 10ms (<=20ms) so mg merges them; sentence 3 is long enough for sl.
case(
    "default-ui",
    None,
    DEFAULT_UI,
    _words(
        ("Hello", 0, 40),
        ("there.", 40, 90),
        ("How", 100, 140),
        ("are", 140, 180),
        ("you", 180, 220),
        ("doing", 220, 300),
        ("today?", 300, 360),
        ("I", 900, 950),
        ("am", 950, 1000),
        ("absolutely", 1000, 1200),
        ("wonderful", 1200, 1500),
        ("and", 1500, 1560),
        ("thriving.", 1560, 1800),
    ),
)

# 2. merge-by-gap only. Two sentences 10ms apart (fake 0.1 <= 0.2) -> merge;
#    third sentence 5000ms later -> stays separate.
case(
    "mg-only",
    None,
    "mg=0.2",
    _words(
        ("First", 0, 50),
        ("sentence.", 50, 120),
        ("Second", 130, 190),
        ("one.", 190, 260),
        ("Later", 5000, 5060),
        ("sentence.", 5060, 5200),
    ),
)

# 3. split-by-punctuation only (default punctuation list). Single sentence
#    (one terminal '.') with interior commas -> split at ", " boundaries.
case(
    "sp-only",
    None,
    "sp=.* /./. /。/?/? /？/!/! /！/,/, ",
    _words(
        ("Well,", 0, 60),
        ("you", 60, 100),
        ("know,", 100, 160),
        ("I", 160, 200),
        ("think", 200, 260),
        ("so,", 260, 320),
        ("maybe.", 320, 400),
    ),
)

# 4. split-by-length only. One long sentence (>42 chars incl. trailing spaces).
case(
    "sl-only",
    None,
    "sl=42",
    _words(
        ("The", 0, 40),
        ("quick", 40, 90),
        ("brown", 90, 140),
        ("fox", 140, 180),
        ("jumps", 180, 240),
        ("over", 240, 290),
        ("the", 290, 320),
        ("lazy", 320, 370),
        ("sleeping", 370, 440),
        ("dog.", 440, 500),
    ),
)

# 5. merge-by-gap + split-by-length, no punctuation split (the '++42+1' branch).
case(
    "mg-sl-no-sp",
    None,
    "mg=0.2++42+1_sl=42",
    _words(
        ("Alpha", 0, 50),
        ("beta.", 50, 110),
        ("Gamma", 120, 170),
        ("delta", 170, 220),
        ("epsilon", 220, 300),
        ("zeta.", 300, 360),
        ("Eta", 5000, 5050),
        ("theta.", 5050, 5120),
    ),
)

# 6. CJK: language 'zh' -> no separator; CJK sentence punctuation 。！？.
case(
    "cjk",
    "zh",
    DEFAULT_UI,
    _words(
        ("你", 0, 30),
        ("好", 30, 60),
        ("世界。", 60, 100),
        ("这", 110, 140),
        ("是", 140, 170),
        ("测试！", 170, 220),
        ("你", 900, 930),
        ("觉得", 930, 990),
        ("怎么样？", 990, 1080),
    ),
)

# 7. No sentence punctuation at all -> single trailing group. Default string:
#    mg no-op (1 seg), sp no-op, sl splits by length.
case(
    "no-sentence-punct",
    None,
    DEFAULT_UI,
    _words(
        ("just", 0, 40),
        ("a", 40, 60),
        ("stream", 60, 120),
        ("of", 120, 150),
        ("words", 150, 210),
        ("with", 210, 250),
        ("no", 250, 280),
        ("terminal", 280, 360),
        ("punctuation", 360, 460),
        ("anywhere", 460, 540),
    ),
)

# 8. split-by-length forcing several even splits (char_count well over 3*42).
case(
    "sl-even-multi",
    None,
    "sl=42",
    _words(
        ("Supercalifragilistic", 0, 200),
        ("expialidocious", 200, 360),
        ("pneumonoultramicroscopic", 360, 560),
        ("silicovolcanoconiosis", 560, 760),
        ("antidisestablishmentarianism", 760, 1000),
        ("floccinaucinihilipilification.", 1000, 1240),
    ),
)

# 9. merge-by-gap with is_sum_max char gating: several close sentences where
#    merging some pairs would exceed 42 chars (skipped), others fit (merged).
case(
    "mg-is-sum-max",
    None,
    "mg=0.2++42+1",
    _words(
        ("Hi.", 0, 40),
        ("Yo.", 45, 80),
        ("This", 85, 130),
        ("is", 130, 160),
        ("a", 160, 180),
        ("much", 180, 230),
        ("longer", 230, 300),
        ("sentence", 300, 380),
        ("here.", 380, 440),
        ("End.", 445, 500),
    ),
)

# 10. Interior-dot grouping + no-op single-str sp. ``sp=.* `` has no '/', so
#     str_to_valid_type returns the bare string ".* " (the '*'->pair split only
#     happens for items INSIDE a '/'-list), and ".* " matches no word -> sp is a
#     no-op. The 3-way split is entirely from SENTENCE grouping: "3.14" and
#     "so." both contain '.', so SENTENCE_END.match ends a group at each.
case(
    "interior-dot-grouping",
    None,
    "sp=.* ",
    _words(
        ("Pi", 0, 40),
        ("is", 40, 80),
        ("3.14", 80, 140),
        ("or", 140, 180),
        ("so.", 180, 240),
        ("Right", 240, 300),
        ("then.", 300, 360),
    ),
)

# 10b. '/'-list of plain strings: sp=,/,  -> [",", ", "]. Only the ", " item
#      fires (words carry a trailing separator space, so a bare "," never ends
#      a word). Exercises the '/'-list-of-strings parse distinct from the single
#      bare-string case above.
case(
    "sp-comma-list",
    None,
    "sp=,/, ",
    _words(
        ("First,", 0, 60),
        ("then", 60, 100),
        ("second,", 100, 160),
        ("and", 160, 200),
        ("third.", 200, 280),
    ),
)

# 11. Lock across ops: sl locks its split boundaries, a following mg=9999 (merge
#     everything) must respect the locks and NOT re-merge them. Compare against
#     the no-lock twin (case 12) to see the lock take effect.
case(
    "lock-chain",
    None,
    "sl=12++++1_mg=99",
    _words(
        ("aaaa", 0, 50),
        ("bbbb", 50, 100),
        ("cccc", 100, 150),
        ("dddd", 150, 200),
        ("eeee.", 200, 260),
    ),
)

# 12. No-lock twin of case 11 (sl then mg re-merges freely).
case(
    "nolock-chain",
    None,
    "sl=12_mg=99",
    _words(
        ("aaaa", 0, 50),
        ("bbbb", 50, 100),
        ("cccc", 100, 150),
        ("dddd", 150, 200),
        ("eeee.", 200, 260),
    ),
)

# 13. newline mode: split-by-punctuation (punctuation ", " so it matches words
#     that carry the trailing separator space) with newline=1 inserts '\n' into
#     the word text instead of splitting into separate segments.
case(
    "sp-newline",
    None,
    "sp=, +0+1",
    _words(
        ("one,", 0, 60),
        ("two,", 60, 120),
        ("three.", 120, 200),
    ),
)

# 14. split-by-punctuation with a min_chars gate: sp=, +0+0+0+30 -> punctuation
#     ", ", min_chars=30 (min_words=0 inactive). Only the group whose char_count
#     (incl. trailing separators) is >= 30 is split at ", "; the short group is
#     left intact. Exercises the `_over_max`/`over_max[i]` machinery.
case(
    "sp-min-chars",
    None,
    "sp=, +0+0+0+30",
    _words(
        # short sentence (< 30 chars incl. separators) -> not split
        ("Hi,", 0, 40),
        ("yes.", 40, 90),
        # long sentence (>= 30 chars) -> split at each ", "
        ("This", 100, 140),
        ("sentence,", 140, 220),
        ("with", 220, 260),
        ("several,", 260, 340),
        ("commas,", 340, 420),
        ("is", 420, 450),
        ("long.", 450, 520),
    ),
)

# 15. split-by-length with BOTH max_chars and max_words: sl=42+3 exercises the
#     even-split word-count path and the `exceed_words` re-check after the char
#     split. One long sentence with many short words.
case(
    "sl-maxwords",
    None,
    "sl=42+3",
    _words(
        ("a", 0, 30),
        ("bb", 30, 60),
        ("cc", 60, 90),
        ("dd", 90, 120),
        ("ee", 120, 150),
        ("ff", 150, 180),
        ("gg", 180, 210),
        ("hh", 210, 240),
        ("ii.", 240, 300),
    ),
)


def generate(stable_ts_root: Path, out_dir: Path) -> None:
    result_mod = _import_stable_whisper(stable_ts_root)
    out_dir.mkdir(parents=True, exist_ok=True)

    for c in CASES:
        expected = run_case(result_mod, c["rows"], c["language"], c["algo"])
        payload = {
            "name": c["name"],
            "language": c["language"],
            "algo": c["algo"],
            "input": c["rows"],
            "expected": expected,
        }
        out_path = out_dir / f"{c['name']}.json"
        out_path.write_text(
            json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(f"wrote {out_path.name}: {len(c['rows'])} row(s) -> {len(expected)} segment(s)")


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]
    default_stable_ts = Path(
        "/private/tmp/claude-501/-Users-jawad-RustroverProjects-humm--claude-worktrees-m4-transcript-viewer-5aa260"
        "/37a55b77-4cfe-4ba6-b15d-21eee080009b/scratchpad/stable-ts"
    )
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--stable-ts",
        type=Path,
        default=default_stable_ts,
        help="stable-ts repo root (the dir containing stable_whisper/). Pinned v2.19.1 clone.",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=repo_root / "crates/humm-core/tests/fixtures/regroup",
        help="Where to write <name>.json fixtures.",
    )
    args = parser.parse_args()

    if not (args.stable_ts / "stable_whisper" / "result.py").is_file():
        parser.error(f"--stable-ts {args.stable_ts} does not contain stable_whisper/result.py")

    generate(args.stable_ts, args.out_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
