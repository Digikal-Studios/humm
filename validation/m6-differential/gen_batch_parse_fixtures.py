#!/usr/bin/env python3
"""M6 differential harness: LLM-translation batch-response parsing fixtures
(F-031, design spec §4.1).

Ground truth for ``crates/humm-core/src/openai/translate.rs``'s
``parse_batch_response``. Stub-imports Buzz's own **unmodified**
``Translator._parse_batch_response`` static method
(``/Users/jawad/RustroverProjects/buzz/buzz/translator.py:130-145``) and runs
it over a fixture matrix covering well-formed input, malformed/degenerate
input, and Unicode content, committing the results as JSON the Rust test
asserts byte-for-byte.

Import strategy (the established M4/M5 stub-import technique, see
``validation/m4-differential/gen_regroup_fixtures.py``): rather than copying
``_parse_batch_response``'s logic into this script, we load the *real*
``translator.py`` file via ``importlib.util.spec_from_file_location`` so the
ground truth is Buzz's actual source, byte-for-byte, not our transcription of
it. ``translator.py`` module-level imports pull in PyQt6, the ``openai``
SDK, and five ``buzz.*`` modules (``buzz.locale``, ``buzz.settings.settings``,
``buzz.store.keyring_store``, ``buzz.transcriber.transcriber``,
``buzz.widgets.transcriber.advanced_settings_dialog``) purely for type hints
and its ``__init__``/``_translate_single``/``_translate_batch`` methods --
none of which ``_parse_batch_response`` (a ``@staticmethod`` we call directly
on the class, without instantiating ``Translator``) touches at call time. So
each of those modules is pre-seeded into ``sys.modules`` as a tiny synthetic
stub exposing just the names ``translator.py`` imports (a name to satisfy
`from x import y`, nothing more) -- no stub *files* are needed on ``sys.path``
(unlike M4's torch/torchaudio/tqdm, which are transitively imported by real,
unstubbed submodules), so there is no ``stubs/`` directory here.

**A discrepancy this harness caught** (see the ``duplicate_bare_trailing_
marker_overwrites`` fixture and the module docstring in ``translate.rs``):
the M6 task brief's own hand-trace of the ``range(1, len(parts) - 1, 2)``
loop bound predicted that a response ending exactly at a bare trailing
``[N]`` marker (no following text) would make ``re.split`` return an
even-length ``parts`` list, silently dropping that final ``(number, text)``
pair. Empirically this is false: ``re.split`` with a single capturing group
*always* appends a trailing (possibly empty) string after the last match, so
``len(parts)`` is always odd (``1 + 2*N`` for ``N`` matches) and every
matched marker is always included in the loop -- a bare trailing ``[3]``
maps index 3 to ``""``, it does not vanish. The fixture below exercises the
one case where this is actually observable: a marker number that first
appears with real text, then reappears as a bare trailing marker -- Buzz's
"last occurrence wins" semantics overwrite the earlier text with ``""``,
which a (subtly wrong) "drop the dangling pair" implementation would get
wrong by keeping the earlier, non-empty text.

Usage::

    python3 gen_batch_parse_fixtures.py [--buzz-root /path/to/buzz] [--out-dir DIR]

Regeneration is an occasional, documented step -- the committed JSON under
``crates/humm-core/tests/fixtures/batch_parse/`` is what ``cargo test``
actually reads. Requires anaconda ``python3`` (3.9) or any stock CPython 3 --
no third-party packages are needed (``re``/``json``/``importlib`` only).
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
import types
from pathlib import Path


# ---------------------------------------------------------------------------
# Stub-import machinery: install synthetic modules for everything
# translator.py imports at module scope, then load the real file.
# ---------------------------------------------------------------------------


def _stub_module(name: str, **attrs: object) -> types.ModuleType:
    """Register a bare synthetic module under ``name`` in ``sys.modules``,
    with ``attrs`` set as module-level names. Mirrors the ``stub()`` helper
    in ``validation/m4-differential/gen_regroup_fixtures.py``."""
    module = types.ModuleType(name)
    for key, value in attrs.items():
        setattr(module, key, value)
    sys.modules[name] = module
    return module


def _install_stubs() -> None:
    """Pre-seed ``sys.modules`` with everything ``translator.py`` imports at
    module scope, so the real file loads without PyQt6/openai/keyring/the
    rest of the Buzz package tree actually being installed or importable."""

    # -- PyQt6.QtCore: `from PyQt6.QtCore import QObject, pyqtSignal` -------
    # `Translator(QObject)` is defined (class body runs `pyqtSignal(...)`
    # twice for its two class-level signals) but never instantiated here, so
    # both stubs only need to be *callable*/*subclassable*.
    class _QObject:
        def __init__(self, *args: object, **kwargs: object) -> None:
            pass

    def _pyqt_signal(*_args: object, **_kwargs: object) -> object:
        return None

    _stub_module("PyQt6")
    _stub_module("PyQt6.QtCore", QObject=_QObject, pyqtSignal=_pyqt_signal)

    # -- openai: `from openai import OpenAI, max_retries` -------------------
    class _OpenAI:
        def __init__(self, *args: object, **kwargs: object) -> None:
            pass

    _stub_module("openai", OpenAI=_OpenAI, max_retries=0)

    # -- buzz.locale: `from buzz.locale import _` ---------------------------
    _stub_module("buzz")
    _stub_module("buzz.locale", _=lambda s: s)

    # -- buzz.settings.settings: `from buzz.settings.settings import Settings`
    class _SettingsKey:
        CUSTOM_OPENAI_BASE_URL = "custom_openai_base_url"

    class _Settings:
        Key = _SettingsKey

        def value(self, key: object = None, default_value: object = None) -> object:
            return default_value

    _stub_module("buzz.settings")
    _stub_module("buzz.settings.settings", Settings=_Settings)

    # -- buzz.store.keyring_store: `from buzz.store.keyring_store import
    #    get_password, Key` ---------------------------------------------
    class _KeyringKey:
        OPENAI_API_KEY = "OpenAI API key"

    _stub_module("buzz.store")
    _stub_module(
        "buzz.store.keyring_store",
        get_password=lambda key: "",
        Key=_KeyringKey,
    )

    # -- buzz.transcriber.transcriber: `from buzz.transcriber.transcriber
    #    import TranscriptionOptions` (used only as a parameter type hint,
    #    which CPython evaluates eagerly at `def` time since translator.py
    #    has no `from __future__ import annotations`) ----------------------
    class _TranscriptionOptions:
        pass

    _stub_module("buzz.transcriber")
    _stub_module(
        "buzz.transcriber.transcriber", TranscriptionOptions=_TranscriptionOptions
    )

    # -- buzz.widgets.transcriber.advanced_settings_dialog:
    #    `from ... import AdvancedSettingsDialog` (also just a type hint) --
    class _AdvancedSettingsDialog:
        pass

    _stub_module("buzz.widgets")
    _stub_module("buzz.widgets.transcriber")
    _stub_module(
        "buzz.widgets.transcriber.advanced_settings_dialog",
        AdvancedSettingsDialog=_AdvancedSettingsDialog,
    )


def _load_translator_class(buzz_root: Path) -> type:
    """Stub-import the real, unmodified ``translator.py`` and return its
    ``Translator`` class (never instantiated -- we only call the
    ``@staticmethod _parse_batch_response`` directly on the class)."""
    translator_path = buzz_root / "buzz" / "translator.py"
    if not translator_path.is_file():
        raise SystemExit(f"--buzz-root {buzz_root} does not contain buzz/translator.py")

    _install_stubs()

    spec = importlib.util.spec_from_file_location(
        "buzz_translator_under_test", translator_path
    )
    if spec is None or spec.loader is None:
        raise SystemExit(f"could not build an import spec for {translator_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.Translator


# ---------------------------------------------------------------------------
# Fixture matrix: (name, response, expected_count). `parsed` is computed by
# calling the real, stub-imported `Translator._parse_batch_response` -- never
# hand-derived -- so this file never encodes an assumption about what Python
# does, only what the real function does.
# ---------------------------------------------------------------------------

CASES: list[tuple[str, str, int]] = [
    (
        "well-formed-three",
        "[1] Hello there\n[2] How are you\n[3] Goodbye",
        3,
    ),
    (
        "missing-middle-number",
        "[1] First item\n[3] Third item",
        3,
    ),
    (
        "out-of-order-numbers",
        "[2] Second\n[1] First\n[3] Third",
        3,
    ),
    (
        "duplicate-last-wins",
        "[1] Original text\n[1] Replacement text",
        1,
    ),
    (
        "preamble-discarded",
        "Sure, here are the translations:\n[1] Bonjour\n[2] Au revoir",
        2,
    ),
    (
        "trailing-garbage-appended",
        "[1] First\n[2] Second\nNote: this is extra commentary the model added",
        2,
    ),
    (
        "no-markers",
        "I cannot process this request right now.",
        3,
    ),
    (
        # Three whitespace shapes after the marker in one response: no
        # space, two spaces, and a bare newline before the text.
        "whitespace-variants",
        "[1]NoSpaceHere\n[2]  DoubleSpaceHere\n[3]\nNewlineAfterMarker",
        3,
    ),
    (
        "multiline-entry-preserved",
        "[1] Line one\nLine two\nLine three\n[2] Second item, single line",
        2,
    ),
    (
        "cjk-text",
        "[1] 你好，世界\n[2] これはテストです",
        2,
    ),
    (
        "expected-count-larger-than-provided",
        "[1] Only one translated item",
        3,
    ),
    (
        # The range-bound edge case (see module docstring): marker 3 first
        # appears with real text, then reappears as a bare trailing marker
        # with nothing after it. Last-occurrence-wins means index 3 ends up
        # "", not the earlier "first value for three".
        "duplicate-bare-trailing-marker-overwrites",
        "[3] first value for three\n[1] hello\n[3]",
        3,
    ),
    (
        "strip-trims-trailing-newline",
        "[1] Hello world\n",
        1,
    ),
    (
        # `\s*` after the marker also consumes a literal tab, not just
        # spaces/newlines -- covers whitespace-variants' blind spot for
        # horizontal-tab specifically.
        "marker-followed-by-tab",
        "[1]\ttabbed text\n[2] second",
        2,
    ),
]


def generate(buzz_root: Path, out_dir: Path) -> None:
    translator_cls = _load_translator_class(buzz_root)
    out_dir.mkdir(parents=True, exist_ok=True)

    for name, response, expected_count in CASES:
        parsed = translator_cls._parse_batch_response(response, expected_count)
        payload = {
            "name": name,
            "response": response,
            "expected_count": expected_count,
            "parsed": parsed,
        }
        out_path = out_dir / f"{name}.json"
        out_path.write_text(
            json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        print(f"wrote {out_path.name}: expected_count={expected_count} -> {parsed!r}")


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--buzz-root",
        type=Path,
        default=Path("/Users/jawad/RustroverProjects/buzz"),
        help="Buzz repo root (the dir containing buzz/translator.py).",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=repo_root / "crates/humm-core/tests/fixtures/batch_parse",
        help="Where to write <name>.json fixtures.",
    )
    args = parser.parse_args()

    generate(args.buzz_root, args.out_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
