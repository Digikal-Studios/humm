# humm

Offline transcription and translation for media files — a desktop app and a
command-line tool.

## Based on Buzz

humm is an **independent Rust port** of
[Buzz](https://github.com/chidiwilliams/buzz) by Chidi Williams.

- Upstream: <https://github.com/chidiwilliams/buzz>
- humm is **unofficial** and **not affiliated with** the Buzz project or its
  authors.
- Behavior aims to match Buzz **within named carve-outs** below; this tree is
  a from-scratch Rust implementation under the same [MIT License](LICENSE).
  See [NOTICE](NOTICE) for attribution. Do not claim “100% Buzz `main` parity”
  without that list.

## Install

Pre-built installers are not published yet. Build from a checkout with a
recent Rust toolchain.

**Prerequisites (all platforms):** [ffmpeg](https://ffmpeg.org/) and
[yt-dlp](https://github.com/yt-dlp/yt-dlp) on `PATH` for media decode and URL
import. Speaker diarization needs OpenBLAS.

| Platform | Extra packages |
| -------- | -------------- |
| **macOS** | `brew install openblas libomp ffmpeg yt-dlp` (OpenBLAS + libomp are required to launch `humm-gui`) |
| **Linux** | `ffmpeg`, `yt-dlp`, `pkg-config`, OpenBLAS (`libopenblas-dev`), ALSA (`libasound2-dev`); a desktop portal or zenity for file dialogs |
| **Windows** | ffmpeg and yt-dlp (e.g. `winget install Gyan.FFmpeg` and `winget install yt-dlp.yt-dlp`); OpenBLAS via vcpkg for diarization (`vcpkg install openblas`) |

```bash
cargo run -p humm-cli -- add <file> --srt
cargo run -p humm-gui
```

Optional tools for specific features: `demucs` (speech isolation), and a
working OpenBLAS install for speaker diarization.

## What works today

- Local Whisper transcription and translation via `whisper-rs` (CLI and GUI)
- File and URL import (yt-dlp), folder import, drag-and-drop in the GUI
- Exports: plain text, SubRip (`.srt`), WebVTT (`.vtt`)
- Task queue, history, preferences, and **all 17** Buzz `main` UI locales
  (including French `fr` and Turkish `tr_TR`)
- Optional speech isolation (`demucs`), noise reduction (DeepFilterNet), and
  speaker identification via ONNX/`speakrs` (not Buzz’s NeMo stack) when those
  dependencies are available
- macOS, Linux, and Windows builds (platform audio and subprocess paths)

The M8 parity matrix is **46 verified / 4 named environment exceptions** of 50
rows (`docs/analysis/PARITY_MATRIX.md`). That is not the same as “100% of
current Buzz `main`.” Deliberate non-goals and remaining honesty gaps are
listed next and in [`docs/review/buzz-main-gaps.md`](docs/review/buzz-main-gaps.md).
Release binaries are a separate follow-up (HUM-26 / HUM-27).

## Named exceptions (not silent gaps)

These are intentional carve-outs or known remaining work. Public “parity” or
“production ready” claims must name them.

| Exception | What Buzz `main` has | What humm does |
| --- | --- | --- |
| **NeMo MSDD / Sortformer / `--speaker-count`** | `--identify-speakers` plus `--speaker-diarizer msdd\|sortformer` and optional `--speaker-count`; folder-watch identify/diarizer/count/merge controls | Bare `--identify-speakers` (and the GUI Speaker-ID window) run `OrtDiarizer`/`speakrs` (ADR-024). `--speaker-count` and `--speaker-diarizer` are **rejected** with a usage error. Folder watch does not expose NeMo engine knobs. |
| **FunASR** | FunASR models on the Hugging Face path (`851c8b3`) | Out of scope (OPEN_QUESTIONS Q-2, ADR-008). No FunASR engine, in-process Python, or `transformers` dependency. |
| **transformers / Parakeet / MMS / Faster-Whisper** | Hugging Face transformers backends and related model families | Out of scope under ADR-008. Local inference is whisper-rs only. |
| **Multiple custom Whisper.cpp models** | JSON list of custom ggml models (`f70a02f`) | One custom slot, `ggml-custom.bin` (ADR-008 / ledger P-008). Not renamed to Buzz’s `ggml-model-whisper-custom.bin` (would orphan caches). |
| **Update payload / installables** | Buzz publishes releases consumers can check | Matrix F-006 stays `implemented` until a real `version_info.json` and installable assets exist. The check degrades on 404; it does not show “Available” against an empty release channel. |
| **Hardcoded English status cells** | Task status text goes through gettext | Column headers are Fluent; task-table status strings in `crates/humm-gui/src/format.rs` (`Queued`, `In Progress…`, etc.) are still English literals. Remaining i18n work. |

Also out of scope as Buzz-internal or toolkit-specific (documented in the gap
walk, not product bugs): demucs windowed in-process RAM streaming, PortAudio
device re-init, Qt XDG secret-portal freeze workarounds, and per-segment
plugin `review_reason` without a plugin host (ADR-010).

## For developers

Start with the design and analysis docs — not historical recovery notes:

- [`docs/design/ARCHITECTURE.md`](docs/design/ARCHITECTURE.md) — crate layout and boundaries
- [`docs/design/IMPLEMENTATION_PLAN.md`](docs/design/IMPLEMENTATION_PLAN.md) — milestones
- [`docs/analysis/FEATURE_INVENTORY.md`](docs/analysis/FEATURE_INVENTORY.md) — feature IDs (F-###)
- [`docs/analysis/PARITY_MATRIX.md`](docs/analysis/PARITY_MATRIX.md) — parity status
- [`docs/review/buzz-main-gaps.md`](docs/review/buzz-main-gaps.md) — Buzz `main` after v1.4.5 + carve-outs
- [`docs/HUMM_SURFACE.md`](docs/HUMM_SURFACE.md) — CLI grammar and contracts
- [`docs/ci.md`](docs/ci.md) — CI expectations

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p humm-media -- --ignored   # needs ffmpeg and yt-dlp
```

Heavier ignored adapter checks stay off unless an environment variable is
set (`HUMM_DEMUCS_CONTRACT`, `HUMM_DFN_CONTRACT`, `HUMM_DIARIZE_CONTRACT`, or
`HUMM_LLM_TEST_BASE` / `_KEY` / `_MODEL`).

### Crates

- `humm-core` — domain types, export, transcription pipeline
- `humm-media` — ffmpeg, ffprobe, yt-dlp, demucs adapters
- `humm-whisper` — Whisper via whisper-rs
- `humm-audio` — capture, playback, resample
- `humm-store` — SQLite history and OS keyring secrets
- `humm-dfn` — DeepFilterNet noise reduction
- `humm-diarize` — speaker diarization
- `humm-cli` — `humm` command
- `humm-gui` — desktop app (egui / eframe)

## License

[MIT](LICENSE) — same terms as Buzz. Copyright notices for Chidi Williams
(Buzz) and Digikal Studios / Jawad (humm) are in `LICENSE`. Third-party /
Buzz attribution is summarized in [NOTICE](NOTICE).

## Security

See [SECURITY.md](SECURITY.md) for how to report vulnerabilities.
