"""Minimal ``torchaudio`` stub (absent, unused by the regroup path).

Only needed so ``stable_whisper.audio.utils``'s ``import torchaudio`` resolves;
that module is itself stubbed out of the import graph in
``gen_regroup_fixtures.py``, so this is belt-and-suspenders.
"""


def __getattr__(name):
    return object
