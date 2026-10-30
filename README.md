# humm

humm is an independent Rust port of [Buzz](https://github.com/chidiwilliams/buzz). Buzz is a local application for transcribing and translating audio. humm is not an official Buzz project.

The `humm` command transcribes or translates media files and can write plain text, SubRip (`.srt`), or WebVTT (`.vtt`). The desktop app is `humm-gui`. Transcription uses Whisper models through the `whisper-rs` crate.

## License

humm uses the [MIT License](LICENSE), the same license as Buzz. Buzz's copyright notice is kept in that file, with a short note that humm is a Rust port.

Copyright (c) 2022 Chidi Williams

## Build and run

Run these from a checkout of this repository.

Speaker diarization uses OpenBLAS. On macOS that is Homebrew's library, linked by absolute path, plus libomp, and the GUI does not start without them. CoreML stays enabled on macOS only.

```bash
brew install openblas libomp ffmpeg yt-dlp
```

On Linux, ffmpeg, yt-dlp, pkg-config, and a system OpenBLAS are required. libomp is not; distribution OpenBLAS uses libgomp. Native file dialogs need a desktop portal or zenity. Audio capture uses ALSA.

```bash
sudo apt install ffmpeg yt-dlp pkg-config libopenblas-dev libasound2-dev
```

On Windows, install ffmpeg and yt-dlp (`winget install Gyan.FFmpeg`, `winget install yt-dlp.yt-dlp`). MSVC builds of speaker diarization need OpenBLAS from vcpkg (`vcpkg install openblas`). CoreML is not compiled into Windows or Linux builds. Running the GUI, WASAPI, and process-tree cancellation still needs a Windows machine.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p humm-media -- --ignored
cargo bench
cargo run -p humm-cli -- add <file> --srt
cargo run -p humm-gui
```

`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` are the checks each change is expected to pass.

`cargo test -p humm-media -- --ignored` needs `ffmpeg` and `yt-dlp`. Heavier ignored adapter checks stay off unless an environment variable is set, so that command still runs with only those two tools:

- `HUMM_DEMUCS_CONTRACT=1` runs the real demucs check.
- `HUMM_DFN_CONTRACT=1 cargo test -p humm-dfn -- --ignored` runs the DeepFilterNet check.
- `HUMM_DIARIZE_CONTRACT=1 cargo test -p humm-diarize -- --ignored` runs the diarization check. The first run downloads about 60 MB of models.
- `HUMM_LLM_TEST_BASE`, `HUMM_LLM_TEST_KEY`, and `HUMM_LLM_TEST_MODEL` point at a real OpenAI-compatible chat endpoint.

## Crates

- `humm-core` holds domain types, export, and the transcription pipeline.
- `humm-media` adapts ffmpeg, ffprobe, yt-dlp, and demucs.
- `humm-whisper` runs Whisper through whisper-rs.
- `humm-audio` plays, captures, and resamples audio.
- `humm-store` stores transcription tasks in SQLite and secrets in the system keyring.
- `humm-dfn` reduces noise with DeepFilterNet.
- `humm-diarize` identifies speakers.
- `humm-cli` is the `humm` command.
- `humm-gui` is the desktop app, built with egui and eframe.
