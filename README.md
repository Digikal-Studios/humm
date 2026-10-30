# humm

Offline transcription and translation for media files — a desktop app and a
command-line tool.

## Based on Buzz

humm is an **independent Rust port** of
[Buzz](https://github.com/chidiwilliams/buzz) by Chidi Williams.

- Upstream: <https://github.com/chidiwilliams/buzz>
- humm is **unofficial** and **not affiliated with** the Buzz project or its
  authors.
- Behavior aims to match Buzz; this tree is a from-scratch Rust
  implementation under the same [MIT License](LICENSE). See [NOTICE](NOTICE)
  for attribution.

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
- Task queue, history, preferences, and localization
- Optional speech isolation (`demucs`), noise reduction (DeepFilterNet), and
  speaker diarization when those dependencies are available
- macOS, Linux, and Windows builds (platform audio and subprocess paths)

Gaps versus Buzz main and remaining environment-blocked parity rows are
tracked in `docs/analysis/PARITY_MATRIX.md`. Release binaries are a separate
follow-up.

## For developers

Start with the design and analysis docs — not historical recovery notes:

- [`docs/design/ARCHITECTURE.md`](docs/design/ARCHITECTURE.md) — crate layout and boundaries
- [`docs/design/IMPLEMENTATION_PLAN.md`](docs/design/IMPLEMENTATION_PLAN.md) — milestones
- [`docs/analysis/FEATURE_INVENTORY.md`](docs/analysis/FEATURE_INVENTORY.md) — feature IDs (F-###)
- [`docs/analysis/PARITY_MATRIX.md`](docs/analysis/PARITY_MATRIX.md) — parity status
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
