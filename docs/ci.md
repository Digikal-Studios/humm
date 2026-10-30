# macOS CI gates

The `gates` job runs on GitHub-hosted `macos-14` (Apple Silicon), with a
120-minute bound. It no longer targets organization runner group `apple-ci` /
label `digikal-apple`: that self-hosted runner is not registered for this
repository, so jobs queued forever. Existing `cargo fmt --check`, full-workspace
Clippy and `cargo test --workspace` gates remain unchanged. Rust is installed
with `dtolnay/rust-toolchain@stable` (rustfmt + clippy); `RUSTUP_TOOLCHAIN`
keeps the job on stable.

## Host prerequisites

Homebrew must be available (it is on GitHub-hosted macOS images). Required
formulae are `openblas`, `libomp`, `cmake`, `ffmpeg` and `pkgconf`. The ffmpeg
formula supplies both ffmpeg and ffprobe. OpenBLAS and libomp are build/runtime
requirements (ADR-024), including absolute Homebrew dylib references.

`scripts/ci_prepare.py` installs any of those formulae that are missing, then
asserts the commands and dylibs. It adds OpenBLAS's pkg-config directory only
when ordinary discovery cannot find it; no global compiler, linker or
developer-directory change is made. First compilation also needs approved
dependency downloads, including the pinned ONNX runtime; ordinary tests do not
download real transcription models.

## Fixtures and coverage

Nonignored CLI, GUI and folder-watch tests need real ffmpeg decoding and the Buzz
stub tiny model. The preparation script downloads exactly two public files from
[official Buzz v1.4.5](https://github.com/chidiwilliams/buzz/tree/aaf98634cd354be207e00cc5aaa1bfbd9e19cf22/testdata),
matching `docs/analysis/FEATURE_INVENTORY.md`'s reference version:

| File | Bytes | Git blob SHA-1 |
| --- | ---: | --- |
| `ggml-tiny.bin` | 575451 | `1351aeba9b17d782f15baf46716c34dcfba0a30c` |
| `whisper-latvian.wav` | 230172 | `d62d5937d0d62c748572d7bc3a35c88fa51db853` |

The 805623-byte download is bounded and verified against each Git object hash,
including the Git blob header. Fixtures live in a unique directory under
`RUNNER_TEMP`; `HUMM_BUZZ_TESTDATA` selects it. No personal checkout path is needed,
and the runner removes temporary files after the job. These are the existing
ordinary-test fixtures, not replacement data or a real Whisper model. Full model,
external API and heavier adapter acceptance tests retain their existing `#[ignore]`
and explicit environment gates; this migration does not claim new acceptance evidence.

## Triggers, trust and validation

This workflow triggers only on pull requests and pushes to `main`; it has no
schedule, dispatch or deployment/publication step. Enabling repository Actions
alone does not create one of those events. A push to a nondefault branch does not
trigger the existing push filter; opening its PR executes the PR workflow, and
merging executes the merged main workflow. Review future workflow and trigger
changes before pushing or opening PRs.

Record final tested commit, run URL and actual runner assignment from GitHub job
records. An enabled Actions setting or prepared YAML alone proves no execution.
Roll back through reviewed workflow changes.

## Hosted library matrix

`hosted-libraries` covers the four native hosts outside `gates`:
`windows-latest` (Windows x64), `ubuntu-latest` (Linux x64), `ubuntu-24.04-arm`
(Linux ARM64), and `windows-11-arm` (Windows ARM64). macOS ARM stays on `gates`
(`macos-14`), which is the job that installs Homebrew OpenBLAS, libomp, cmake,
and ffmpeg. This matrix has no macOS cell, because that cell would skip those
formulae and duplicate `gates` with a weaker toolchain.

Each cell runs:

```bash
cargo clippy -p humm-core -p humm-media -p humm-audio --all-targets -- -D warnings
cargo test --locked -p humm-core -p humm-media -p humm-audio
```

That clippy command is what lints `cfg(windows)` and `cfg(target_os = "linux")`
items. `gates` on `macos-14` compiles the macOS configuration of those crates
instead. The library cells leave `humm-gui`, `humm-whisper`, and `humm-diarize`
unbuilt. Ignored contract tests stay skipped, so ffmpeg, yt-dlp, and model
downloads stay out of this job. Linux cells install `libasound2-dev` so cpal
can link ALSA.

A green Windows cell runs the live `cmd /C ver` check: `kernel_release` and
`os_version` must match `parse_windows_ver` of that output, and Windows does
not consult `uname`. The `taskkill` test checks the argument vector only. Audio
device contract tests stay ignored, and `opening_the_system_host_returns_a_result`
accepts a missing host. The green cell is library compile, lint, and unit-test
evidence plus that `ver` probe. It is not a WASAPI stream and not a live
process-tree kill.

## Release artifacts

`.github/workflows/release.yml` builds release archives on GitHub-hosted
runners only (`macos-14`, `ubuntu-24.04`, `ubuntu-24.04-arm`, `windows-latest`,
`windows-11-arm`). It is separate from the `CI` workflow:

| Event | What runs |
| --- | --- |
| `pull_request` touching the release workflow / `scripts/package_release.py` / this doc | Linux x86_64 **dry-run** only (build, bundle, smoke). No GitHub Release. |
| `push` of tag `v*` | Full matrix → **draft** release (never mints tags; never auto-publishes). |
| `workflow_dispatch` on `main` with an **existing** tag | Full matrix → draft release for that tag. Refuses other branches and refuses to create tags. Publishing the draft is a human step. |

Each archive is produced by `scripts/package_release.py`, which always
bundles OpenBLAS load-time libraries beside the binaries, rewrites macOS
install names / Linux `$ORIGIN` RPATH, and fail-closes on `otool` / `ldd` /
smoke failures. An ONNX Runtime shared library is bundled only when the
binaries' loader dependencies mention it (otherwise skipped — Linux often
links `libonnxruntime.a` statically); `CONTENTS.txt` /
`RUNTIME.txt` record which. GUI cells fail the job if `humm-gui` does not
build; Windows ARM64 is explicitly `windows-aarch64-cli`.

Windows release cells install vcpkg `openblas` **and** `clapack` for the
triplet: stock vcpkg OpenBLAS is BLAS-only (`BUILD_WITHOUT_LAPACK=ON`), so
`ndarray-linalg`/`lax` needs `lapack.lib` + `libf2c.lib` linked via
`RUSTFLAGS` (same as the Windows ARM64 production smoke). Packaging copies
`openblas*.dll`, `lapack*.dll`, and `libf2c*.dll` into the archive.

On `windows-11-arm`, bindgen needs a native ARM64 `libclang`. The workflow
installs the LLVM **woa64** package via `KyleMayes/install-llvm-action`
(`arch: arm64`) into a **non-default** directory (not
`C:\Program Files\LLVM`, which holds the image's x86-64 LLVM) and sets
`LIBCLANG_PATH` to that install's `bin` directory. It does **not** pass
`env: true` on that action — that would export `CC`/`CXX=clang` and break
`whisper-rs-sys` CMake (GNU `-fPIC` into the MSVC generator).

Packaging argv is built as one bash array so macOS bash 3.2 + `set -u`
does not trip on empty optional flags. The shipped Rust toolchain is pinned
to `1.99.1`. See the README Download section for the public story.
