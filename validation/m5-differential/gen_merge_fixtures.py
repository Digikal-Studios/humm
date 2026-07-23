#!/usr/bin/env python3
"""M5 differential harness: `live::merge`'s append-and-correct merge fixtures
(F-062).

Standard library only (`re`, `json`) -- no Buzz/PyQt import (Buzz is
unrunnable on this machine per `docs/analysis` M1 notes, and none of the
functions below touch Qt anyway); not a cargo project (`validation/*` is
excluded from the workspace, see root `Cargo.toml`).

The three static methods below (`filter_text`, `find_common_part`,
`merge_text_no_overlap`) and the two module-level regexes are copied
**verbatim** (logic unchanged, only `self` dropped since these are
`@staticmethod`s in Buzz) from
`buzz/widgets/recording_transcriber_widget.py`:

- `REAL_CHARS_REGEX` / `NO_SPACE_BETWEEN_SENTENCES` -- lines 59-60.
- `filter_text` -- lines 778-784.
- `find_common_part` -- lines 884-904.
- `merge_text_no_overlap` -- lines 906-913.

`process_transcription_merge` (lines 915-961) is a *method* touching
`self.texts`/`self.hide_unconfirmed`/`self.transcription_text_box`/
`self.transcript_export_file` and the GUI/export side effects it triggers.
`merge_step` below extracts its actual algorithm -- the trim-adjacent-pairs
loop, the hide-unconfirmed display copy, the `merge_text_no_overlap` fold,
and the final sentence-space regex substitution -- unchanged, as a plain
function over `(texts, new_text, hide_unconfirmed) -> (mutated_texts,
merged_display)`; `self.texts` becomes the `texts` parameter (mutated in
place and also returned, matching Buzz's own list-mutates-in-place
semantics), and the GUI/export writes are dropped (that's `live::export`'s
job, not this module's).

This is ground truth for `crates/humm-core/src/live/merge.rs`'s
`find_common_part`/`merge_text_no_overlap`/`MergeState::push`; the Rust
integration test `merge_matches_python_reference_fixtures` asserts byte-exact
string equality (the merged display output, and the full stored-texts array
at every step of every scenario) against every fixture this script writes.

Python `str` indexing is by Unicode code point -- exactly what these
functions rely on (`text1[i-1]`, `text1[-i:]`, `.rfind`/`.find` returning a
code-point index) -- which is why the CJK/unicode fixture scenario below
exists: it is the one differential check that actually exercises the
code-point-vs-byte distinction the Rust port must get right (see
`merge.rs`'s module doc).

Usage:
    python3 gen_merge_fixtures.py [--out-dir DIR]

Regeneration is a documented, occasional step -- the committed JSON under
`crates/humm-core/tests/fixtures/live/` is what `cargo test` actually reads.
Plain `python3` (stdlib only, no numpy needed) is sufficient here, unlike
`gen_chunker_fixtures.py`.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

# ---------------------------------------------------------------------------
# Copied verbatim from buzz/widgets/recording_transcriber_widget.py:59-60,
# 778-784, 884-913 (all @staticmethod / module-level -- `self` dropped,
# everything else byte-for-byte identical).
# ---------------------------------------------------------------------------

REAL_CHARS_REGEX = re.compile(r"\w")
NO_SPACE_BETWEEN_SENTENCES = re.compile(r"([.!?。！？])([A-Z])")


def filter_text(text: str) -> str:
    text = text.strip()

    if not REAL_CHARS_REGEX.search(text):
        return ""

    return text


def find_common_part(text1: str, text2: str) -> str:
    len1, len2 = len(text1), len(text2)
    max_len = 0
    end_index = 0

    lcsuff = [[0] * (len2 + 1) for _ in range(len1 + 1)]

    for i in range(1, len1 + 1):
        for j in range(1, len2 + 1):
            if text1[i - 1] == text2[j - 1]:
                lcsuff[i][j] = lcsuff[i - 1][j - 1] + 1
                if lcsuff[i][j] > max_len:
                    max_len = lcsuff[i][j]
                    end_index = i
            else:
                lcsuff[i][j] = 0

    common_part = text1[end_index - max_len : end_index]

    return common_part if len(common_part) >= 5 else ""


def merge_text_no_overlap(text1: str, text2: str) -> str:
    overlap_start = 0
    for i in range(1, min(len(text1), len(text2)) + 1):
        if text1[-i:] == text2[:i]:
            overlap_start = i

    return text1 + text2[overlap_start:]


# ---------------------------------------------------------------------------
# `process_transcription_merge`'s algorithm (recording_transcriber_widget.py:
# 915-961), extracted to a plain function over (texts, new_text,
# hide_unconfirmed) -- the trim loop / hide-unconfirmed / merge-fold / regex
# pipeline is unchanged; `self.texts`/`self.hide_unconfirmed`/`text_box`/
# `export_file` become `texts` (mutated + returned)/`hide_unconfirmed`/
# nothing/nothing.
# ---------------------------------------------------------------------------


def merge_step(texts: list[str], text: str, hide_unconfirmed: bool) -> tuple[list[str], str]:
    texts.append(text)

    last_common_length = None
    for i in range(len(texts) - 1):
        common_part = find_common_part(texts[i], texts[i + 1])
        if common_part:
            common_length = len(common_part)
            texts[i] = texts[i][: texts[i].rfind(common_part) + common_length]
            texts[i + 1] = texts[i + 1][texts[i + 1].find(common_part) :]
            if i == len(texts) - 2:
                last_common_length = common_length
        elif i == len(texts) - 2:
            last_common_length = None

    display_texts = list(texts)
    if hide_unconfirmed and len(display_texts) > 1:
        if last_common_length is not None:
            display_texts[-1] = display_texts[-1][:last_common_length]
        else:
            display_texts = display_texts[:-1]

    merged_texts = ""
    for t in display_texts:
        merged_texts = merge_text_no_overlap(merged_texts, t)

    merged_texts = NO_SPACE_BETWEEN_SENTENCES.sub(r"\1 \2", merged_texts)

    return texts, merged_texts


# ---------------------------------------------------------------------------
# Scenarios -- each is a named sequence of (raw_chunk, hide_unconfirmed)
# pushes. `raw_chunk` is passed through `filter_text` first (matching
# `on_next_transcription`'s `text = filter_text(text); if len(text) == 0:
# return` gate upstream of `process_transcription_merge` -- no scenario
# chunk below filters to empty, since that path is unit-tested directly in
# Rust against hand-derived cases, not via this sequence harness).
# ---------------------------------------------------------------------------

Scenario = dict


SCENARIOS: dict[str, list[tuple[str, bool]]] = {
    # Realistic overlapping ASR chunk shapes (each chunk repeats the
    # previous chunk's tail plus new material, as append-and-correct mode's
    # 1.5s keep-back would produce), hide_unconfirmed off throughout.
    "overlapping-transcript-realistic": [
        ("The quick brown fox jumps over the lazy", False),
        ("jumps over the lazy dog near the river", False),
        ("near the river bank while birds sing", False),
    ],
    # Same chunk shape, hide_unconfirmed on throughout -- exercises the
    # truncate-last-to-confirmed-overlap path on every push.
    "overlapping-transcript-hide-unconfirmed": [
        ("The quick brown fox jumps over the lazy", True),
        ("jumps over the lazy dog near the river", True),
        ("near the river bank while birds sing", True),
    ],
    # hide_unconfirmed toggled on and off across the same sequence --
    # confirms the flag only changes the display computation, never the
    # stored/trimmed texts, which keep compounding regardless.
    "hide-unconfirmed-toggle-mid-sequence": [
        ("Weather today is sunny with light wind", False),
        ("with light wind from the northwest side", True),
        ("from the northwest side bringing cool air", False),
        ("bringing cool air through the valley floor", True),
    ],
    # A chunk with no overlap at all appears mid-sequence, hide off: kept in
    # the display (plain concatenation via merge_text_no_overlap, which
    # finds no suffix/prefix overlap either).
    "no-overlap-chunk-kept-when-hide-off": [
        ("First topic discussed at length here", False),
        ("Completely unrelated second topic begins now", False),
        ("begins now and continues for a while", False),
    ],
    # Same shape, hide on: the no-overlap chunk is dropped from display
    # entirely (last_common_length is None for that pair).
    "no-overlap-chunk-dropped-when-hide-on": [
        ("First topic discussed at length here", True),
        ("Completely unrelated second topic begins now", True),
        ("begins now and continues for a while", True),
    ],
    # CJK text mixed with ASCII, exercising 。！？ sentence punctuation and
    # code-point (not byte) indexing throughout find_common_part / rfind /
    # find / merge_text_no_overlap.
    "cjk-unicode-sequence": [
        ("你好世界这是一个测试", False),
        ("这是一个测试用来验证程序", False),
        ("用来验证程序是否正确运行。Next", False),
    ],
    # Overlaps that stay just below find_common_part's >=5 code-point gate
    # throughout -- the stored texts are never trimmed, only the ungated
    # merge_text_no_overlap fold elides anything.
    "short-overlaps-below-five-gate": [
        ("alpha beta gamm", False),
        ("gamm delta epsi", False),
        ("epsi zeta eta theta", False),
    ],
    # An overlap of exactly 5 code points -- the >=5 gate's boundary.
    "exact-five-char-gate-boundary": [
        ("prefix textA", False),
        ("textA suffix", False),
    ],
    # Concatenation crosses a period directly followed by an uppercase
    # letter with no space -- triggers NO_SPACE_BETWEEN_SENTENCES on the
    # final merged output.
    "sentence-space-regex-trigger": [
        ("This is the first sentence.Next", False),
        ("Next comes the second sentence.Final", False),
        ("Final part concludes the passage", False),
    ],
    # The exact same chunk pushed twice in a row (a stalled/duplicate ASR
    # result) -- the whole text is itself the common part.
    "identical-consecutive-chunks": [
        ("Repeated phrase exactly as before", False),
        ("Repeated phrase exactly as before", False),
    ],
}


def generate(out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    for name, pushes in SCENARIOS.items():
        texts: list[str] = []
        steps = []

        for raw_chunk, hide_unconfirmed in pushes:
            chunk = filter_text(raw_chunk)
            assert chunk != "", f"scenario {name}: chunk filtered to empty, adjust fixture input"

            texts, merged = merge_step(texts, chunk, hide_unconfirmed)
            steps.append(
                {
                    "push": chunk,
                    "hide_unconfirmed": hide_unconfirmed,
                    "texts_after": list(texts),
                    "merged": merged,
                }
            )

        payload = {"name": name, "steps": steps}

        out_path = out_dir / f"merge-{name}.json"
        out_path.write_text(
            json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(f"wrote {out_path} ({len(steps)} steps)")


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=repo_root / "crates/humm-core/tests/fixtures/live",
        help="Where to write the merge-<name>.json fixtures.",
    )
    args = parser.parse_args()

    generate(args.out_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
