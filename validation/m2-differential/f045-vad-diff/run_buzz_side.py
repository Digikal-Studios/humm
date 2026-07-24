#!/usr/bin/env python3
"""F-045 time-boxed retry (M5 Task 13): run Buzz's *genuine, unmodified*
``buzz/transcriber/whisper_cpp.py`` VAD segment-parsing code
(``_parse_segment_timings``, ``_parse_word_level_timings``,
``_make_offset_mapper``) against a real ``whisper-cli --vad`` JSON output,
without installing Buzz's heavy Python dependency graph (torch,
faster-whisper, openai-whisper, stable-ts, PyQt6, ...) and without touching
the Buzz checkout at all.

Strategy (same "stub only the actually-needed names" technique
``validation/m4-differential/gen_regroup_fixtures.py`` used for
stable_whisper): ``buzz/transcriber/whisper_cpp.py`` only needs, at import
time, ``buzz.assets.APP_BASE_DIR``, ``buzz.transcriber.transcriber.{Segment,
Task, FileTranscriptionTask}``, and ``buzz.transcriber.file_transcriber.
app_env``. None of the functions this script calls
(``_parse_segment_timings``/``_parse_word_level_timings``/
``_make_offset_mapper``) touch any of those names at *call* time -- they're
only needed to satisfy the module's top-level ``from buzz... import ...``
statements. So: pre-seed ``sys.modules`` with tiny stub modules providing
those four names (mirroring their real shapes exactly -- ``Segment`` is a
plain ``start/end/text/translation`` dataclass, ``Task`` a two-member enum,
both copied verbatim from ``buzz/transcriber/transcriber.py:18-34``), then
load the real ``whisper_cpp.py`` file directly via ``importlib`` (bypassing
the ``buzz`` package's own ``__init__.py``/heavy imports entirely). Every
line of ``whisper_cpp.py`` itself runs completely unmodified.

Usage::

    python3 run_buzz_side.py <whisper-cli-output.json> <language>
"""
from __future__ import annotations

import dataclasses
import enum
import importlib.util
import json
import sys
import types
from pathlib import Path

BUZZ_ROOT = Path("/Users/jawad/RustroverProjects/buzz")
WHISPER_CPP_PY = BUZZ_ROOT / "buzz" / "transcriber" / "whisper_cpp.py"


def _install_stubs() -> None:
    # buzz.assets -- real module is two lines, no heavy deps, but stub it
    # anyway rather than importing the real one, to prove this script depends
    # on nothing but the one file under test.
    assets_mod = types.ModuleType("buzz.assets")
    assets_mod.APP_BASE_DIR = str(BUZZ_ROOT / "buzz")  # unused by the functions we call

    # buzz.transcriber.transcriber -- Segment/Task copied verbatim from
    # transcriber.py:18-34 (dataclass field names/defaults must match exactly
    # since whisper_cpp.py constructs Segment(start=.., end=.., text=..,
    # translation=..) by keyword).
    transcriber_mod = types.ModuleType("buzz.transcriber.transcriber")

    class Task(enum.Enum):
        TRANSLATE = "translate"
        TRANSCRIBE = "transcribe"

    @dataclasses.dataclass
    class Segment:
        start: int
        end: int
        text: str
        translation: str = ""

    class FileTranscriptionTask:  # only used as a type hint; never constructed here
        pass

    transcriber_mod.Task = Task
    transcriber_mod.Segment = Segment
    transcriber_mod.FileTranscriptionTask = FileTranscriptionTask

    # buzz.transcriber.file_transcriber -- only `app_env` is imported.
    file_transcriber_mod = types.ModuleType("buzz.transcriber.file_transcriber")
    file_transcriber_mod.app_env = {}

    buzz_pkg = types.ModuleType("buzz")
    buzz_pkg.__path__ = []  # mark as a package
    transcriber_pkg = types.ModuleType("buzz.transcriber")
    transcriber_pkg.__path__ = []

    sys.modules["buzz"] = buzz_pkg
    sys.modules["buzz.assets"] = assets_mod
    sys.modules["buzz.transcriber"] = transcriber_pkg
    sys.modules["buzz.transcriber.transcriber"] = transcriber_mod
    sys.modules["buzz.transcriber.file_transcriber"] = file_transcriber_mod


def _load_real_whisper_cpp_module():
    spec = importlib.util.spec_from_file_location(
        "buzz.transcriber.whisper_cpp", WHISPER_CPP_PY
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)  # runs the REAL, unmodified file
    return module


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <whisper-cli-output.json> <language>", file=sys.stderr)
        return 2

    json_path = Path(sys.argv[1])
    language = sys.argv[2]

    _install_stubs()
    whisper_cpp = _load_real_whisper_cpp_module()
    print(
        f"loaded genuine {WHISPER_CPP_PY} "
        f"(buzz checkout git HEAD unmodified, read-only)",
        file=sys.stderr,
    )

    with open(json_path, "r", encoding="latin-1") as f:
        result = json.load(f)

    # Segment-level path (Buzz's default -- no word-level-timings option),
    # WhisperCpp._parse_segment_timings, whisper_cpp.py:321-341. Does NOT
    # call _make_offset_mapper at all: sentence-level offsets are taken
    # directly from whisper-cli's own JSON (already on the original
    # timeline per whisper.cpp's own VAD offset restoration).
    segment_level = whisper_cpp.WhisperCpp._parse_segment_timings(result)

    # Word-level path, WhisperCpp._parse_word_level_timings,
    # whisper_cpp.py:216-319 -- DOES call _make_offset_mapper per Whisper
    # segment to place each token/word inside that segment's span by linear
    # rescale (the "per-segment linear rescale" PARITY_MATRIX's F-045 note
    # refers to).
    word_level = whisper_cpp.WhisperCpp._parse_word_level_timings(
        result, language, True
    )

    out = {
        "segment_level": [dataclasses.asdict(s) for s in segment_level],
        "word_level": [dataclasses.asdict(s) for s in word_level],
    }
    print(json.dumps(out, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
