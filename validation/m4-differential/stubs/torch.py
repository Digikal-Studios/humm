"""Minimal ``torch`` stub for the M4 regroup differential harness (F-086).

The stable-ts 2.19.1 regroup code path does **not** use torch at runtime; it
only needs the ``torch`` name (and ``torch.Tensor``) resolvable for
module-load-time type annotations in ``stable_whisper/result.py`` (e.g.
``audio: Union[torch.Tensor, np.ndarray, str, bytes]`` on ``WhisperResult``'s
alignment methods, which are never called during regroup).

Real torch/torchaudio are absent from this environment and are not required to
compute regroup output. numpy and tqdm are *not* stubbed: the regroup math
(``np.cumsum``/``np.abs``/``argmin``/``.nonzero()`` in ``get_length_indices``/
``get_gap_indices``) genuinely uses real numpy, and tqdm is only pulled in for
its name. See ``gen_regroup_fixtures.py`` for the full import strategy.
"""


class Tensor:  # referenced in type annotations only, never instantiated here
    pass


def __getattr__(name):  # any other attribute access -> harmless placeholder
    return object
