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

## 2. `--json` event schema

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
```

Per task the order is `queued` → `started` → (`download_progress`* for URL
inputs) → (`progress`/`segment`*) → one of `completed`/`failed`/`canceled` — or
`queued` → `canceled`/`failed` if it never starts. `model_download_progress`
events (if any) precede all per-task events. `total` may be `null` when the
size is unknown.

Human mode instead prints, to **stderr**, a per-task progress line
`<input>  NN% — ETA: <eta>` (ETA rendered `NNm NNs` or `NNs`, Buzz's formula)
and a URL download phase line `downloading <url>  NN%`; written export paths go
to **stdout** (one per line); failures print `error: <input>: <cause>` to
stderr. `--quiet` suppresses the progress lines only.

## 3. Exit codes

| Code | Meaning |
|------|---------|
| `0` | Every input succeeded. |
| `1` | Usage/validation error (bad flag, unknown language, missing `--hfid`, unavailable backend) **or** a config/runtime error before any task could run (corrupt settings, model download/resolution failure). Usage errors also print `Run \`humm add --help\` for usage.`; config/runtime errors print the actionable cause alone. |
| `2` | At least one task failed (and none was canceled). Failures are persisted as history rows, so `humm add missing.mp3 good.wav` yields a `failed` row and a `completed` row. |
| `130` | At least one task was canceled — SIGINT/Ctrl-C (first signal cancels every task; a second exits immediately) or startup recovery of a stale `in_progress`/`queued` row from a crashed/killed prior run. Outranks `2`. |

## 4. Environment variables

| Variable | Effect |
|----------|--------|
| `HUMM_DATA_DIR` | Data directory (database + downloaded media). Default: platform data dir + `/humm`. |
| `HUMM_CONFIG_DIR` | Config directory (`settings.toml`). Default: platform config dir + `/humm`. |
| `HUMM_MODEL_ROOT` | Model cache directory. Default: platform data dir + `/humm/models`. |
| `HUMM_LOG_DIR` | Log directory. Default: platform log location (macOS `~/Library/Logs/humm`). |
| `HUMM_FORCE_CPU` | Any value other than unset or `false` forces CPU inference (disables Metal/CoreML). Mirrors Buzz's `BUZZ_FORCE_CPU`. |
| `HUMM_WHISPER_N_THREADS` | Whisper thread-count override (else `max(1, cpus/2)`). |
| `HUMM_PARAGRAPH_SPLIT_TIME` | Millisecond gap between segments that starts a new paragraph on export (default `2000`). |
| `HUMM_DOWNLOAD_COOKIEFILE` | Cookie file passed to yt-dlp for authenticated URL downloads. Takes precedence over `settings.downloads.cookie_file`. |
| `HUMM_FFMPEG` / `HUMM_FFPROBE` / `HUMM_YTDLP` | Override the ffmpeg / ffprobe / yt-dlp binary path (else looked up on `PATH`). |
| `HUMM_BUZZ_SQLITE` | Override the one-time Buzz-import source database. Used only if the path exists — point it at a nonexistent path to disable the import (M2 addition; used by hermetic tests). |
| `HUMM_BUZZ_TESTDATA`, `HUMM_WHISPER_TEST_MODEL` | Test-only: locate Buzz fixtures / a real tiny model for `#[ignore]` tests. |

## 5. File locations (macOS defaults shown)

| What | Path | Override |
|------|------|----------|
| Settings | `~/Library/Application Support/humm/settings.toml` | `HUMM_CONFIG_DIR` |
| Database | `~/Library/Application Support/humm/humm.sqlite` | `HUMM_DATA_DIR` |
| URL-import media | `~/Library/Application Support/humm/media/<task-id>/` | `HUMM_DATA_DIR` |
| Model cache | `~/Library/Application Support/humm/models/` | `HUMM_MODEL_ROOT` |
| Logs | `~/Library/Logs/humm/logs.txt` | `HUMM_LOG_DIR` |

Linux/Windows follow the platform config/data/state conventions
(`directories` crate); see `crates/humm-core/src/paths.rs`.

## 6. First-run migration

On first launch, if a Buzz database exists (macOS
`~/Library/Application Support/Buzz/Buzz.sqlite`, or `HUMM_BUZZ_SQLITE`) and the
import has not already run, humm copies Buzz's transcription history into its
own store once (idempotent; failures are non-fatal). Stale `in_progress`/
`queued` rows (Buzz's or humm's) are then flipped to `canceled` at every
startup so a crashed run leaves no phantom "running" task.
