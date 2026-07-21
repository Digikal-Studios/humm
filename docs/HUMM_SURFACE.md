# HUMM_SURFACE.md — humm's observable contract

The successor to `docs/analysis/EXTERNAL_SURFACE.md` (which catalogs *Buzz*'s
surface). This documents *humm*'s own CLI grammar, `--json` event schema, exit
codes, environment variables, and on-disk file locations. Stable from M2
onward: the JSON field names and event names are a compatibility contract.

Buzz-derived flag *spellings* are verbatim (ADR-007); `--json`/`--quiet` are
humm additions.

## 1. `humm add` grammar

`humm add [OPTIONS] <FILE>...` — transcribe or translate one or more local
files or URLs.

| Flag | Value | Default | Meaning |
|------|-------|---------|---------|
| `-t`, `--task` | `transcribe` \| `translate` | `transcribe` | Transcribe in-language, or translate to English. |
| `-m`, `--model-type` | `whisper` \| `whispercpp` \| `huggingface` \| `fasterwhisper` \| `openaiapi` | `whisper` | Backend selector. `whisper`/`whispercpp`/`fasterwhisper` all map to the one local whisper-rs backend (ADR-008). `huggingface` resolves `--hfid`. `openaiapi` is not yet available (exit 1). |
| `-s`, `--model-size` | `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`, `medium.en`, `large`, `large-v2`, `large-v3`, `large-v3-turbo`, `custom`, `lumii` | `small` | Model size. `custom` requires `models.custom_model_url` in settings. (Default is `small`, not Buzz's `tiny` — UX_SPEC §6.) |
| `--hfid` | `<id>` | — | Hugging Face repo id. Required with `-m huggingface`; ignored otherwise. |
| `-l`, `--language` | `<code>` | `""` (auto-detect) | ISO language code, or empty to auto-detect. |
| `-p`, `--prompt` | `<text>` | `""` | Initial prompt passed to the model. |
| `-w`, `--word-timestamps` | flag | off | Emit word-level timestamps. |
| `-e`, `--extract-speech` | flag | off | Not yet available (exit 1). |
| `--openai-token` | `<token>` | — | Accepted; used only with `-m openaiapi` (not yet available). |
| `--srt` | flag | off | Write an SRT export. |
| `--vtt` | flag | off | Write a VTT export. |
| `--txt` | flag | off | Write a TXT export. |
| `-d`, `--output-directory` | `<DIRECTORY>` | each input's own directory | Where exports are written. |
| `--hide-gui` | flag | off | No-op (humm is headless — ADR-007); prints an informational note. |
| `--json` | flag | off | Emit one JSON event object per line on stdout instead of human output (humm addition). |
| `--quiet` | flag | off | Suppress stderr progress lines (stdout paths and stderr errors still print). |
| `<FILE>...` | paths or URLs | — | One or more inputs. A string with both a scheme and a host is treated as a URL (downloaded via yt-dlp); everything else is a local path. |

Zero formats still transcribes and persists history — it just writes no export.

## 2. Voice-activity detection (VAD)

VAD is **default-on, not a flag** — there is no `--vad`/`--no-vad` switch (matching
Buzz, which has none either). Every `humm add` run tries to use Silero VAD
(F-045) when a model is obtainable:

- **Cached:** if `ggml-silero-v6.2.0.bin` is already present and complete under
  the model root, it's used immediately — no network, no delay.
- **First run:** if it's missing, humm downloads it once from
  `huggingface.co/ggml-org/whisper-vad` before the first task starts, emitting
  the same `model_download_progress` events (`--json` mode) / progress line
  (human mode) described in §3 below — this is the same mechanism Buzz gets
  for free by shipping the model at build time next to `whisper-cli`; humm
  fetches it lazily instead since there's no bundled binary to ship it beside.
- **Graceful degradation:** any fetch failure (offline, network blip, disk
  full) is logged (`log::warn!`) and the run continues **without VAD** —
  transcription still completes normally, just without the voice-activity
  offset remap. VAD is never a hard requirement for `humm add` to succeed.
- **Kill-switch:** VAD stays default-on for real users; `HUMM_DISABLE_VAD`
  (any non-empty value) is a test-only escape hatch that skips it entirely
  (see §5).

See `crates/humm-cli/src/run.rs`'s `ensure_vad()` for the implementation.

## 3. `--json` event schema

One JSON object per line on **stdout**; nothing else is written to stdout in
`--json` mode. Field names and event names are stable.

```json
{"event":"model_download_progress","file":"ggml-small.bin","downloaded":1024,"total":487000000}
{"event":"queued","task_id":"550e8400-e29b-41d4-a716-446655440000","input":"/path/or/url"}
{"event":"started","task_id":"550e8400-e29b-41d4-a716-446655440000"}
{"event":"download_progress","task_id":"550e8400-e29b-41d4-a716-446655440000","fraction":0.42}
{"event":"progress","task_id":"550e8400-e29b-41d4-a716-446655440000","fraction":0.42}
{"event":"segment","task_id":"550e8400-e29b-41d4-a716-446655440000","start_ms":40,"end_ms":299,"text":"..."}
{"event":"completed","task_id":"550e8400-e29b-41d4-a716-446655440000","written":["/path.srt"]}
{"event":"failed","task_id":"550e8400-e29b-41d4-a716-446655440000","error":"..."}
{"event":"canceled","task_id":"550e8400-e29b-41d4-a716-446655440000"}
{"event":"store_write_failed","task_id":"550e8400-e29b-41d4-a716-446655440000","operation":"mark_started","detail":"..."}
```

Per task the order is `queued` → `started` → (`download_progress`* for URL
inputs) → (`progress`/`segment`*) → one of `completed`/`failed`/`canceled` — or
`queued` → `canceled`/`failed` if it never starts. `model_download_progress`
events (if any) precede all per-task events. `total` may be `null` when the
size is unknown. `store_write_failed` is out of band and additive (ADR-018): a
history write (`operation` names the store method, e.g. `mark_started`) failed
and is logged, but the run is unaffected — it may appear anywhere in a task's
sequence and never changes it.

Human mode instead prints, to **stderr**, a per-task progress line
`<input>  NN% — ETA: <eta>` (ETA rendered `NNm NNs` or `NNs`, Buzz's formula)
and a URL download phase line `downloading <url>  NN%`; written export paths go
to **stdout** (one per line); failures print `error: <input>: <cause>` to
stderr; a `store_write_failed` event prints `warning: history not recorded
(<operation>): <detail>` to stderr. `--quiet` suppresses the progress and
warning lines only.

## 4. Exit codes

| Code | Meaning |
|------|---------|
| `0` | Every input succeeded. |
| `1` | Usage/validation error (bad flag, unknown language, missing `--hfid`, unavailable backend) **or** a config/runtime error before any task could run (corrupt settings, model download/resolution failure). Usage errors also print `Run \`humm add --help\` for usage.`; config/runtime errors print the actionable cause alone. |
| `2` | At least one task failed (and none was canceled). Failures are persisted as history rows, so `humm add missing.mp3 good.wav` yields a `failed` row and a `completed` row. |
| `130` | At least one task **in this invocation** was canceled via SIGINT/Ctrl-C (first signal cancels every task; a second exits immediately). Outranks `2`. Startup recovery flips a *prior* run's stale `in_progress`/`queued` rows to `canceled`, but that never drives the current invocation's exit code — a relaunch whose own inputs fail still exits `2`. |

## 5. Environment variables

| Variable | Effect |
|----------|--------|
| `HUMM_DATA_DIR` | Data directory (database + downloaded media). Default: platform data dir + `/humm`. |
| `HUMM_CONFIG_DIR` | Config directory (`settings.toml`). Default: platform config dir + `/humm`. |
| `HUMM_MODEL_ROOT` | Model cache directory. Default: platform data dir + `/humm/models`. |
| `HUMM_LOG_DIR` | Log directory. Default: platform log location (macOS `~/Library/Logs/humm`). |
| `HUMM_FORCE_CPU` | Any value other than unset or `false` forces CPU inference (disables Metal/CoreML). Mirrors Buzz's `BUZZ_FORCE_CPU`. |
| `HUMM_WHISPER_N_THREADS` | Whisper thread-count override (else `max(1, cpus/2)`). |
| `HUMM_DISABLE_VAD` | Any non-empty value skips VAD entirely (no cache lookup, no download; §2). VAD is default-on for real users — this is a test-only kill-switch (used by hermetic `humm-cli` tests to stay network-free). |
| `HUMM_PARAGRAPH_SPLIT_TIME` | Millisecond gap between segments that starts a new paragraph on export (default `2000`). |
| `HUMM_DOWNLOAD_COOKIEFILE` | Cookie file passed to yt-dlp for authenticated URL downloads. Takes precedence over `settings.downloads.cookie_file`. |
| `HUMM_FFMPEG` / `HUMM_FFPROBE` / `HUMM_YTDLP` | Override the ffmpeg / ffprobe / yt-dlp binary path (else looked up on `PATH`). |
| `HUMM_BUZZ_SQLITE` | Override the one-time Buzz-import source database. Used only if the path exists — point it at a nonexistent path to disable the import (M2 addition; used by hermetic tests). |
| `HUMM_BUZZ_TESTDATA`, `HUMM_WHISPER_TEST_MODEL` | Test-only: locate Buzz fixtures / a real tiny model for `#[ignore]` tests. |

### 5.1 `humm-gui`-only environment variables

The CLI-shared variables above apply to `humm-gui` too (it calls the same
`humm-core::paths`/`humm-core::env` functions). These four are GUI-specific:
`HUMM_FAVORITE_LANGUAGES` from M3 Task 9 (import paths + options sheet); the
other three from M3 Task 11 (preferences/update/telemetry wiring).

| Variable | Effect |
|----------|--------|
| `HUMM_DISABLE_TELEMETRY` | Any non-empty value disables the one `app_launched` telemetry event (F-007, ADR-009) — Buzz's `BUZZ_DISABLE_TELEMETRY` parity. An unset or set-but-empty value leaves telemetry on (subject to the other gates below). |
| `HUMM_DISABLE_UPDATE_CHECK` | Any non-empty value disables the *startup* background update check (F-006, ADR-015) — read exactly where Buzz's own `BUZZ_DISABLE_UPDATE_CHECK` gate sits (`main_window.py:569`): the checker/channel is still constructed unconditionally, only the network call is skipped. Does **not** gate the About window's manual "Check for updates" (an explicit user click bypasses every startup gate, including this one). |
| `HUMM_POSTHOG_KEY` | Build-time only (`option_env!`, not read at runtime): humm's own PostHog project key. Absent at build time compiles telemetry down to a permanent no-op — there is no way to enable it at runtime without rebuilding. |
| `HUMM_FAVORITE_LANGUAGES` | Comma-separated language codes (Buzz: `BUZZ_FAVORITE_LANGUAGES`) shown as a separator-wrapped "favorites" block at the top of the options sheet's language combo (F-023); invalid codes are silently dropped. Unset or empty renders no favorites block at all. |
| `HUMM_SMOKE_OPEN_WINDOWS` / `HUMM_SMOKE_AUTO_CLOSE_FRAMES` | Test-only, unstable — smoke-test hooks, not user surface. Drive the `docs/gui-manual/M3.md` windowed multi-window/clean-shutdown smoke under a controller with no OS input-injection access (`app.rs`'s `SmokeHooks`): the first opens the About/Preferences viewports on the first frame; the second requests a normal window close after that many frames. |

## 6. File locations (macOS defaults shown)

| What | Path | Override |
|------|------|----------|
| Settings | `~/Library/Application Support/humm/settings.toml` | `HUMM_CONFIG_DIR` |
| Database | `~/Library/Application Support/humm/humm.sqlite` | `HUMM_DATA_DIR` |
| URL-import media | `~/Library/Application Support/humm/media/<task-id>/` | `HUMM_DATA_DIR` |
| Model cache | `~/Library/Application Support/humm/models/` | `HUMM_MODEL_ROOT` |
| VAD model | `~/Library/Application Support/humm/models/ggml-silero-v6.2.0.bin` (§2) | `HUMM_MODEL_ROOT` |
| Logs | `~/Library/Logs/humm/logs.txt` | `HUMM_LOG_DIR` |
| GUI window/table state (`humm-gui` only) | `~/Library/Application Support/humm/gui.ron` | `HUMM_CONFIG_DIR` (`eframe`'s `persistence_path`, §7) |
| Single-instance stale-flip lock | `~/Library/Application Support/humm/instance.lock` | `HUMM_DATA_DIR` (§7) |
| GUI exclusivity lock (`humm-gui` only) | `~/Library/Application Support/humm/gui.lock` | `HUMM_DATA_DIR` (§7) |

Linux/Windows follow the platform config/data/state conventions
(`directories` crate); see `crates/humm-core/src/paths.rs`.

## 7. First-run migration

On first launch, if a Buzz database exists (macOS
`~/Library/Application Support/Buzz/Buzz.sqlite`, or `HUMM_BUZZ_SQLITE`) and the
import has not already run, humm copies Buzz's transcription history into its
own store once (idempotent; failures are non-fatal). Stale `in_progress`/
`queued` rows (Buzz's or humm's) are then flipped to `canceled` at every
startup so a crashed run leaves no phantom "running" task.

Running two `humm add` invocations concurrently is now supported (ADR-017): at
startup, every humm process takes a non-blocking exclusive claim on
`data_dir/instance.lock`. Whichever process wins that claim is provably the
sole live humm process, so only *it* runs the stale-flip; it then downgrades
to a shared claim (held for the rest of its run) so later `humm add`
invocations can start and proceed concurrently. A process that loses the
exclusive claim (another humm process is already alive) skips the flip
entirely and takes a shared claim instead — it never treats another process's
genuinely-running `in_progress` row as stale. A lock-file I/O failure (e.g. an
unwritable `data_dir`) falls back to the old unconditional flip rather than
failing the run.

A second, independent lock file (`data_dir/gui.lock`) exists for GUI
exclusivity — an exclusive claim on it that is never downgraded, so a second
GUI launch's attempt fails. `humm-gui`'s `main.rs` wires this end to end (M3
GUI-shell task): a failed claim shows a native `rfd::MessageDialog` ("humm is
already running.") and exits 0 before any window opens; a successful claim is
held for the process lifetime alongside `instance.lock`'s own shared claim
(the GUI participates in the `instance.lock` stale-flip protocol like any
other humm process, via the same `acquire_instance_lock` call).
