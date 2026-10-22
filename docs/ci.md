# Shared Apple CI

The owner approved moving the native CI baseline from the hosted macOS image to
the Mac Studio's macOS 27 on 2026-10-22. This does not establish compatibility with
older macOS versions. The job selects organization group `apple-ci` and labels
`[self-hosted, macOS, ARM64, digikal-apple]`, with a 120-minute bound and no hosted
Apple fallback. Existing `cargo fmt --check`, full-workspace Clippy and
`cargo test --workspace` gates remain unchanged. Rust selects stable through the
job environment without changing the shared user's default or self-updating rustup.

## Host prerequisites

Provision full Xcode and Homebrew `openblas`, `libomp`, `cmake`, `ffmpeg` and
`pkgconf` outside jobs. The ffmpeg formula supplies both ffmpeg and ffprobe.
OpenBLAS and libomp are build/runtime requirements (ADR-024), including absolute
Homebrew dylib references. Do not force-link the keg-only formulae or replace
working toolchains. `scripts/ci_prepare.py` fails clearly if required commands or
dylibs are missing. It adds OpenBLAS's pkg-config directory only when ordinary
discovery cannot find it; no global compiler, linker or developer-directory change
is made. First compilation also needs approved dependency downloads, including the
pinned ONNX runtime; ordinary tests do not download real transcription models.

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
alone does not create one of those events. A push to a nondefault migration branch
does not trigger the existing push filter; opening its PR executes the PR workflow,
and merging executes the merged main workflow. Review future workflow and trigger
changes before pushing or opening PRs. Keep fork PR execution disabled and review
all contributors before sharing this persistent personal-account host.

Record final tested commit, run URL and actual runner assignment from GitHub job
records. An enabled Actions setting or prepared YAML alone proves no execution.
Historical refs can still contain hosted Apple selectors; runner-group access does
not rewrite workflows. Roll back through reviewed workflow changes; preserve the
owner's desired Actions policy rather than silently restoring a hosted fallback.
