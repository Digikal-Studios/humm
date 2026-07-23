"""Minimal ``tqdm`` stub (progress bars are unused by the regroup path).

``stable_whisper/result.py`` does ``from tqdm import tqdm`` at module load; the
regroup methods never construct a progress bar. A real tqdm is present in this
particular interpreter, but stubbing it keeps the harness runnable on a bare
system ``python3`` that lacks it. (numpy is deliberately *not* stubbed — the
regroup arithmetic needs the real thing.)
"""


class tqdm:
    def __init__(self, iterable=None, *a, **k):
        self._iterable = iterable if iterable is not None else ()

    def __iter__(self):
        return iter(self._iterable)

    def update(self, *a, **k):
        pass

    def close(self, *a, **k):
        pass

    def set_description(self, *a, **k):
        pass


def __getattr__(name):
    return object
