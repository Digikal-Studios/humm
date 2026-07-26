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
| `-m`, `--model-type` | `whisper` \| `whispercpp` \| `huggingface` \| `fasterwhisper` \| `openaiapi` | `whisper` | Backend selector. `whisper`/`whispercpp`/`fasterwhisper` all map to the one local whisper-rs backend (ADR-008). `huggingface` resolves `--hfid`. `openaiapi` runs `humm_core::openai::OpenAiEngine` (F-041, M6 Task 11) — no local model/VAD resolution, no `model_download_progress` events. Access token: `--openai-token` if non-empty, else the OS keyring (`SecretKey::OpenAiApiKey`, §6), else exit 1 with Buzz's exact `"No OpenAI access token found"` (`cli.py:235`) before any other work runs — a config error, not a usage error (no `--help` hint). Base URL and API model come from `settings.toml`'s `[openai]` section (`base_url`, default `https://api.openai.com/v1`; `api_model`, default `"whisper-1"`), read fresh at task-engine-construction time. |
| `-s`, `--model-size` | `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`, `medium.en`, `large`, `large-v2`, `large-v3`, `large-v3-turbo`, `custom`, `lumii` | `small` | Model size. `custom` requires `models.custom_model_url` in settings. (Default is `small`, not Buzz's `tiny` — UX_SPEC §6.) |
| `--hfid` | `<id>` | — | Hugging Face repo id. Required with `-m huggingface`; ignored otherwise. |
| `-l`, `--language` | `<code>` | `""` (auto-detect) | ISO language code, or empty to auto-detect. |
| `-p`, `--prompt` | `<text>` | `""` | Initial prompt passed to the model. |
| `-w`, `--word-timestamps` | flag | off | Emit word-level timestamps. |
| `-e`, `--extract-speech` | flag | off | Not yet available (exit 1). |
| `--openai-token` | `<token>` | — | The OpenAI access token, used only with `-m openaiapi` (see above for the full resolution order). Accepted-and-ignored with any other `-m` value (Buzz parity). |
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
{"event":"skipped","task_id":"550e8400-e29b-41d4-a716-446655440000"}
{"event":"post_processed","task_id":"550e8400-e29b-41d4-a716-446655440000","notes_updated":true,"segments_replaced":false,"written":["/out.docx"],"errors":[]}
{"event":"phase","task_id":"550e8400-e29b-41d4-a716-446655440000","phase":"isolating_speech"}
```

Per task the order is `queued` → `started` → (`download_progress`* for URL
inputs) → (`progress`/`segment`*) → one of `completed`/`failed`/`canceled` — or
`queued` → `canceled`/`failed` if it never starts. `model_download_progress`
events (if any) precede all per-task events. `total` may be `null` when the
size is unknown. `store_write_failed` is out of band and additive (ADR-018): a
history write (`operation` names the store method, e.g. `mark_started`) failed
and is logged, but the run is unaffected — it may appear anywhere in a task's
sequence and never changes it.

`skipped` (F-115, additive) is a terminal-state alternative to `completed`: a
task whose input was found already transcribed never actually ran, and its
found segments become its transcript (`TaskStore::mark_skipped`) — it counts
toward exit-code success exactly like `completed` (§4; Buzz treats a skip as
completion too, `main_window.py:435-443`). `post_processed` (F-110/F-111/
F-113/F-115/F-116, additive) always **follows** a `completed` event (never a
`skipped` one) and is emitted once, only when at least one post-processing
hook is enabled; `errors` lists one message per hook failure (a hook failure
never fails the task). **Both events now have real producers** (M7 Tasks 4–7
landed the features these were reserved for; this paragraph previously said
they did not): `Runner::run_job` returns `Terminal::Skipped(segments)` when
F-115's pre-hook finds the input already transcribed, and
`run_on_complete_chain` emits the single `PostProcessed` event after the
resize/summary/docx chain. Both are reachable from the CLI as well as the GUI
— `humm-cli`'s `CliPostprocSource` snapshots `settings.postproc` and hands it
to the queue, so a `settings.toml` with a hook enabled produces these events
from `humm add` (there is deliberately no CLI *flag* for post-processing;
settings are the only switch). Post-hooks are off by default, so a default
configuration emits neither.

`phase` (F-027, M8, additive) reports a mid-run phase transition that isn't a
plain progress fraction (`TaskEventKind::PhaseChanged`): today the only value
`phase` ever carries is `"isolating_speech"` (demucs speech isolation running
before transcription), `snake_case` of the corresponding `TaskPhase` variant —
the field is extensible to future phases without a schema change. It has no
producer yet as of this commit: `TaskEventKind::PhaseChanged` exists in
`humm-core::queue` and is wired into `--json`/human-mode rendering, but the
demucs pre-pass that actually emits it (`run_job`'s before-transcription seam,
gated on `TranscriptionOptions::extract_speech`) lands in a later M8 task —
the same "vocabulary now, producer later" shape `skipped`/`post_processed` had
in M6 before M7 wired their hooks (see the paragraph above). Once wired it
will appear anywhere between `started` and the terminal event, never terminal
itself; human mode renders it as a progress-style line (`<input>  isolating
speech`), so `--quiet` suppresses it exactly like `download_progress`/
`progress`.

LLM translation (F-031) adds **no new `--json` event** in M6 — the schema
above is unchanged: `humm add` has no LLM-translation flag (`humm-cli`'s
grammar has no equivalent of the options sheet's/recording window's "Enable
AI translation" toggle), so there is no CLI path that enables the feature at
all this milestone. `openai::translate::LlmEndpointSource`'s own doc comment
states this explicitly: "the CLI supplies no source at all in M6 (`None` →
translation is simply never attempted, Buzz's CLI having no LLM-translation
path either)" — an absence, not an omission: the same out-of-band-and-additive
spirit as `store_write_failed` above (ADR-018), just with nothing to add.

Human mode instead prints, to **stderr**, a per-task progress line
`<input>  NN% — ETA: <eta>` (ETA rendered `NNm NNs` or `NNs`, Buzz's formula),
a URL download phase line `downloading <url>  NN%`, and (once a later M8 task
wires a producer) a `phase` line `<input>  isolating speech`; written export
paths go to **stdout** (one per line); failures print `error: <input>: <cause>` to
stderr; a `store_write_failed` event prints `warning: history not recorded
(<operation>): <detail>` to stderr. A `skipped` event's terminal line is
`<input>  skipped (already transcribed)`, to stderr, in the same slot a
`completed`/`failed` terminal line occupies. A `post_processed` event prints
nothing unless its `errors` array is non-empty, in which case each prints
`warning: post-processing: <cause>` to stderr. `--quiet` suppresses the
progress and warning lines only (`post_processed`'s warnings included;
`skipped`'s terminal line is not suppressed, same as `completed`/`failed`).

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
| `HUMM_DISABLE_KEYRING` | Any non-empty value makes the `-m openaiapi` token-resolution keyring lookup report "no entry" without ever opening a real `keyring::Entry` (M6 Task 11, F-002/F-041). The real OS keyring is in play for real users — this is a test-only kill-switch, same shape as `HUMM_DISABLE_VAD` (used by hermetic `humm-cli` tests so they never touch the real OS keychain). |
| `HUMM_TRANSLATION_API_BASE_URL` | Overrides the LLM-translation endpoint's base URL, taking precedence over `settings.openai.base_url` (F-031, design spec §4.3). Buzz's equivalent is `BUZZ_TRANSLATION_API_BASE_URl` — note the trailing lowercase `l`: that typo is upstream Buzz's own (`translator.py:44`), not a porting error. humm spells it correctly (`URL`) since the `BUZZ_`→`HUMM_` prefix rename already breaks exact-name migration; documented here so a user grepping either spelling finds the mapping. |
| `HUMM_TRANSLATION_API_KEY` | Overrides the LLM-translation endpoint's API key, taking precedence over the OS-keyring value (F-031, design spec §4.3). Buzz's equivalent is `BUZZ_TRANSLATION_API_KEY` (`translator.py:47`, no typo). |
| `HUMM_PARAGRAPH_SPLIT_TIME` | Millisecond gap between segments that starts a new paragraph (default `2000`), resolved everywhere through the one shared helper, `text_view::paragraph_split_from_env()`. `humm-cli`'s `run.rs` and the GUI's queue path (`options_sheet::build_specs`, the spec a Run click enqueues) both call it once at spec-build time, into `OutputSpec.paragraph_split`; the **viewer's** Text-mode display and its own TXT export (F-084) call it fresh on every use instead, since those redraw/re-export live rather than building a spec up front. One helper, four call sites — an override can never make a queue-enqueued task's auto-written TXT, the on-screen Text-mode view, and a later manual TXT export of the same transcript disagree (Buzz `write_output` reads `BUZZ_PARAGRAPH_SPLIT_TIME` fresh too). Translation mode never paragraph-splits (Buzz quirk). |
| `HUMM_DOWNLOAD_COOKIEFILE` | Cookie file passed to yt-dlp for authenticated URL downloads. Takes precedence over `settings.downloads.cookie_file`. |
| `HUMM_FFMPEG` / `HUMM_FFPROBE` / `HUMM_YTDLP` | Override the ffmpeg / ffprobe / yt-dlp binary path (else looked up on `PATH`). |
| `HUMM_BUZZ_SQLITE` | Override the one-time Buzz-import source database. Used only if the path exists — point it at a nonexistent path to disable the import (M2 addition; used by hermetic tests). |
| `HUMM_BUZZ_TESTDATA`, `HUMM_WHISPER_TEST_MODEL` | Test-only: locate Buzz fixtures / a real tiny model for `#[ignore]` tests. |

### 5.1 `humm-gui`-only environment variables

The CLI-shared variables above apply to `humm-gui` too (it calls the same
`humm-core::paths`/`humm-core::env` functions). These are GUI-specific:
`HUMM_FAVORITE_LANGUAGES` from M3 Task 9 (import paths + options sheet); the
telemetry/update-check/PostHog three from M3 Task 11 (preferences/update/
telemetry wiring); `HUMM_MERGE_REGROUP_RULE` from M4 Task 12 (resizer); the
`HUMM_SMOKE_*` group (test-only); `HUMM_UPLOAD_URL` from M5 (live export/
upload, F-064).

| Variable | Effect |
|----------|--------|
| `HUMM_DISABLE_TELEMETRY` | Any non-empty value disables the one `app_launched` telemetry event (F-007, ADR-009) — Buzz's `BUZZ_DISABLE_TELEMETRY` parity. An unset or set-but-empty value leaves telemetry on (subject to the other gates below). |
| `HUMM_DISABLE_UPDATE_CHECK` | Any non-empty value disables the *startup* background update check (F-006, ADR-015) — read exactly where Buzz's own `BUZZ_DISABLE_UPDATE_CHECK` gate sits (`main_window.py:569`): the checker/channel is still constructed unconditionally, only the network call is skipped. Does **not** gate the About window's manual "Check for updates" (an explicit user click bypasses every startup gate, including this one). |
| `HUMM_POSTHOG_KEY` | Build-time only (`option_env!`, not read at runtime): humm's own PostHog project key. Absent at build time compiles telemetry down to a permanent no-op — there is no way to enable it at runtime without rebuilding. |
| `HUMM_FAVORITE_LANGUAGES` | Comma-separated language codes (Buzz: `BUZZ_FAVORITE_LANGUAGES`) shown as a separator-wrapped "favorites" block at the top of the options sheet's language combo (F-023); invalid codes are silently dropped. Unset or empty renders no favorites block at all. |
| `HUMM_MERGE_REGROUP_RULE` | Overrides the regroup algorithm string the resizer's **Merge** builds from its checkboxes (F-086, ADR-020) — a stable-ts-style `mg`/`sp`/`sl` op string (e.g. `mg=0.2++42+1_sp=…_sl=42`). Only `mg`/`sp`/`sl` are supported; any other op yields an actionable `UnsupportedOp` error naming the op and the supported set. Timings use Buzz's decisecond quirk (`mg=0.2` ⇒ merge gaps ≤ 20 ms real). Unset ⇒ the checkbox-built string is used. |
| `HUMM_UPLOAD_URL` | Overrides the live-recording export's upload URL (F-064, spec §2.4, ✚ — Buzz has no advanced-sheet field for this, only the env var; humm adds the field, but the env still wins per that same ✚ decision). Effective value = `HUMM_UPLOAD_URL` if set and non-empty, else `settings.recording.upload_url` (the advanced sheet's "Upload URL:" field, M5 Task 12), else no upload for the session (`humm_core::live::effective_upload_url`). Read once per session start, not per-frame. |
| `HUMM_SMOKE_OPEN_WINDOWS` / `HUMM_SMOKE_AUTO_CLOSE_FRAMES` | Test-only, unstable — smoke-test hooks, not user surface. Drive the `docs/gui-manual/M3.md` windowed multi-window/clean-shutdown smoke under a controller with no OS input-injection access (`app.rs`'s `SmokeHooks`): the first opens the About/Preferences viewports on the first frame; the second requests a normal window close after that many frames. |
| `HUMM_SMOKE_OPEN_VIEWER` | Test-only, unstable — A/V-drift acceptance hook, not user surface (`app.rs`'s `SmokeHooks`, F-083). Set to a media-file path: on the first frame humm seeds a completed transcript row for it, opens its viewer through the normal open path, auto-starts playback once the player is ready, and requests a clean app shutdown once playback reaches end-of-media (`ViewerVm::smoke_autoplay`/`drive_smoke_playback`) — the deferred-viewport analogue of `HUMM_SMOKE_OPEN_WINDOWS`, so the windowed 10-min A/V-drift run (`docs/gui-manual/M4.md`, `docs/benchmarks/M4.md`) runs without OS input injection. Pair with `HUMM_SMOKE_AV_DRIFT_LOG` to capture the pairs. Unset or empty ⇒ inert. |
| `HUMM_SMOKE_AV_DRIFT_LOG` | Test-only, unstable — A/V-sync acceptance instrumentation, not user surface (F-083, `viewer.rs`'s `DriftLog`). When set to a writable file path and the open transcript is video, the viewer appends one `audio_ms,frame_ts_ms` CSV line per rendered video frame (audio position = master clock vs. the displayed frame's presentation timestamp). Feeds the windowed 10-min ±50 ms p95 drift check in the Task 14 gui-manual run over a generated `testsrc2`+`sine` fixture. Unset or empty ⇒ no log. |
| `HUMM_SMOKE_LIVE_FIXTURE` | Test-only, unstable — live realtime-fit acceptance hook, not user surface (M5 Task 14, spec §8 item 1, `app.rs`'s `SmokeHooks`). Set to a media-file path: on construction humm decodes it once via the real `AudioDecoder` (ffmpeg) and swaps the recording window's capture adapter for `humm_gui::live_fixture::FixtureCapture` (a wall-clock-paced, looping stand-in feeding the same `rtrb`/`MeterSnapshot` boundary a real device would) instead of `CpalCapture` — the workaround for this environment's standing microphone-TCC blocker (`.superpowers/sdd/task-5-report.md`), so the acceptance run still exercises the real recording window, the real `WhisperEngine`, and real fixture audio, just not a real microphone. On the first frame the hook opens the recording window, forces the model size to `small`, and clicks Record; after `HUMM_SMOKE_LIVE_SECONDS` it clicks Stop, then closes the window and the app once the session's `Stopped` acknowledgement lands. A decode failure logs a warning, falls back to the real `CpalCapture`, and leaves the hook permanently inert for that run (never blocks ordinary use). Unset or empty ⇒ inert. |
| `HUMM_SMOKE_LIVE_SECONDS` | Test-only, unstable — companion to `HUMM_SMOKE_LIVE_FIXTURE` (M5 Task 14): how many seconds the hook records before clicking Stop. Parsed as a positive `u32`; unset, empty, zero, or unparseable falls back to 200 s (long enough to be a genuine "windowed multi-minute run" and to loop past a ~2-minute fixture at least once). Ignored when `HUMM_SMOKE_LIVE_FIXTURE` is unset. |
| `HUMM_SMOKE_METER_FRAME_LOG` | Test-only, unstable — meter-repaint-cadence acceptance instrumentation, not user surface (M5 Task 14, spec §8 item 5, `recording.rs`'s `MeterFrameLog`). When set to a writable file path, the recording window appends one epoch-millisecond line every time it actually redraws while `RecordingStatus::Recording` (the meter is animating) — consecutive-line deltas are the observed frame-to-frame interval, the same after-the-run-parse shape as `HUMM_SMOKE_AV_DRIFT_LOG`. Unset or empty ⇒ no log. |

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
| Presentation-window CSS override (`humm-gui` only, F-065, M5 Task 12) | `~/Library/Application Support/humm/presentation_style.css` (optional — absent by default) | `HUMM_CONFIG_DIR` |
| OpenAI API key (F-041/F-031, ADR-012) | OS keyring/credential store (macOS Keychain / Windows Credential Manager / *nix Secret Service), service `"humm"`, entry `"OpenAI API key"` — never `settings.toml`, the DB, or logs (`crates/humm-store/src/secrets.rs`'s `KeyringSecretStore`). Deliberately its own service string, distinct from Buzz's `"Buzz"`: a fresh humm setup never reads a key Buzz already stored. | Not overridable by env for real use; `-m openaiapi`'s hermetic cli-int tests use `HUMM_DISABLE_KEYRING` (§5) instead of touching this store at all. |
| AI Summary API key (F-111, M7, ADR-012) | Same keyring, same service `"humm"`, **separate entry `"AI Summary API key"`** — the summarization hook's endpoint is independently configurable from the transcription one, so it gets its own credential rather than sharing. Buzz stored its equivalent under a plugin-scoped name (`plugin:ai_summary:api_key`); humm addresses it through the same `SecretKey::entry_name` scheme as every other key (ledger #L9). Never `settings.toml`, the DB, or logs. | Same as above: no env override for real use. |

The presentation-window CSS override is a **subset-parser mechanism deviation**
from Buzz (spec §2.6, `views/presentation.rs::parse_presentation_css`): Buzz's
`presentation_window.py` renders the transcript via Qt's `QTextBrowser::setHtml`
and looks for a full user stylesheet at
`user_cache_dir("Buzz")/presentation_window_style.css`; egui has no HTML/CSS
renderer, so humm paints the text directly and only understands the three
declarations Buzz's own generated stylesheet actually uses — `color`,
`background-color`, `font-size` (a `pt` suffix) — from a file at humm's
**config** dir (not cache dir — the one settings-adjacent, user-discoverable
directory this port already uses for every other per-user file) named
`presentation_style.css` (not `presentation_window_style.css`). Unrecognized
declarations are ignored and logged once; a file present but with nothing
recognized, or unreadable, leaves the theme unchanged.

Linux/Windows follow the platform config/data/state conventions
(`directories` crate); see `crates/humm-core/src/paths.rs`.

### 6.1 Viewer settings (`[viewer]`) & database schema v2

The transcript viewer (F-080..F-086, M4) adds a `[viewer]` table to
`settings.toml` (all defaults = Buzz defaults; saved via the existing
diff-on-change pattern):

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `segment_looping_enabled` | bool | `false` | Loop Segment checkbox. |
| `playback_controls_visible` | bool | `false` | Playback-controls bar visibility. |
| `find_widget_visible` | bool | `false` | Find bar visibility. |
| `follow_audio_enabled` | bool | `false` | Follow Audio checkbox. |
| `playback_rate` | f32 | `1.0` | Global playback speed (Buzz `AUDIO_PLAYBACK_RATE`); clamped `0.1..=5.0` on load. |
| `audio_splitter_fraction` | f32 | `0.95` | Table/media split for audio media (950:50). |
| `video_splitter_fraction` | f32 | `0.8` | Table/media split for video media (800:200). |
| `resizer_create_new_transcript` | bool | `true` | Resizer "Create new transcript" checkbox. |
| `geometry` | `Option<ViewerGeometry { x?, y?, w, h }>` | absent → 1000×800 (min 800×500) | One shared viewer-window geometry, last-close-wins across all open viewers. |

The per-transcript **last export folder** is *not* a setting — it lives in the
store (schema v2, below), keyed by transcript id.

**Database schema v2** (`humm.sqlite`, gated by `PRAGMA user_version`):
migration v1→v2 is forward-only and adds one column,
`ALTER TABLE transcription ADD COLUMN last_export_folder TEXT` — the
per-transcript directory the viewer's export dialog defaults to (F-084, UX J-6).
A v2 binary opening a v1 database migrates it in place; a database whose
`user_version` is **newer than 2** is refused with an actionable error rather
than opened (forward-compat guard). Resizer-created rows
(`copy_transcription`/`mark_replaced_completed`/`update_progress_direct`) are
the only writers of `status`/`progress` outside the queue — those rows never
enter the queue, so ADR-018's "queue owns status transitions" invariant (scoped
to queue-executed tasks) is preserved.

### 6.2 Recording & presentation settings (`[recording]`/`[presentation]`)

Live recording (F-060..F-065, M5) adds two `settings.toml` sections, saved via
the same diff-on-change pattern as `[viewer]` — `mode`/`hide_unconfirmed`/every
export field/`upload_url` save **immediately** (Buzz's own `on_*_changed`
handlers write straight through); `language`/`task`/`model_size`/`backend`/
`initial_prompt`/`silence_threshold`/`line_separator`/`transcription_step` are
mutated live on the shared `RecordingOptions` object and only persisted when
the recording window closes (Buzz's `_do_close`/`closeEvent` save, spec §6).
`backend` (F-061b, M7 Task 18) joins this close-persisted group rather than
saving immediately: Buzz's own model-*type* combo (the closest analogue) has
no immediate-save handler either — it's written only inside `_do_close`
(`recording_transcriber_widget.py:1231-1233`), alongside language/task/model/
initial-prompt, so humm's `backend` field follows the identical save-timing
split.

`[recording]`:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `language` / `task` / `model_size` / `initial_prompt` | `Option<String>` | `None` | Sticky options-group prefill, same resolution rule as the file-transcriber sheet. |
| `backend` (M7, F-061b) | `Option<String>` | `None` | Last-selected live-session backend wire string (e.g. `"openai_api"`); `None` = the default local whisper.cpp backend. Close-persisted (see above), same vocabulary as `file_transcriber.backend`. |
| `silence_threshold` | f32 | `0.0025` | RMS floor below which a batch is skipped. Clamped `0.0..=1.0` on load. |
| `line_separator` | String | `"\n\n"` | Separator between append-below/append-above utterances (stored literal; the advanced sheet edits it escaped). |
| `transcription_step` | f64 | `3.5` | Append-and-correct chunk size (seconds). Clamped `2.0..=5.0` on load. |
| `mode` | usize | `0` | Display mode index (0 = Append below, 1 = Append above, 2 = Append and correct — Buzz `RecordingTranscriberMode` declaration order; UX_SPEC §6). |
| `hide_unconfirmed` | bool | `true` | Hide the last (unconfirmed) utterance's tail in append-and-correct mode. |
| `export_enabled` | bool | `false` | Live TXT/CSV export on/off. |
| `export_folder` / `export_file_name` | `Option<...>` | `None` | Export directory / custom filename template; `None` folder disables export for the session even if `export_enabled`. |
| `export_file_type` | String | `"txt"` | `"txt"` or `"csv"`. |
| `export_max_entries` | u32 | `0` | `0` = keep all entries; else cap, range `0..=99` (clamped on load). |
| `upload_url` | `Option<String>` | `None` | ✚ Advanced-sheet upload URL field; `HUMM_UPLOAD_URL` env still wins (§5.1). |
| `measured_speed` | `BTreeMap<String, f32>` | `{}` | ✚ Per-model-size measured realtime ratio (spec §2.8), keyed by model-size wire string, last-run-wins; a value `< 1.0` drives the inline realtime-fit warning. API-backend sessions (M7, F-061b) key their own measurement under `Backend::OpenAiApi`'s wire string (`"openai_api"`) instead of a model size — a slow network run must never poison (or be poisoned by) a local model size's own numbers (ledger #L14). |
| `enable_llm_translation` (M6, F-063) | bool | `false` | Whether live AI translation is enabled. Persisted on window close alongside `llm_model`/`llm_prompt` (design spec §7.4). |
| `llm_model` (M6, F-063) | `Option<String>` | `None` | Last-entered LLM translation model id, if live AI translation was configured. |
| `llm_prompt` (M6, F-063) | `Option<String>` | `None` | Last-entered LLM translation prompt, if live AI translation was configured. |

`[presentation]`:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `text_size` | u32 | `24` | Presentation-window font size (pt). Clamped `10..=100` on load; the bar's spinbox range is the same. |
| `theme` | String | `"light"` | `"light"` / `"dark"` / `"custom"`. |
| `text_color` / `background_color` | String (hex) | `"#000000"` / `"#FFFFFF"` | Custom-theme colors; invalid hex falls back to these defaults (✚, Buzz has no such fallback). |

The presentation window's user-stylesheet override
(`<config_dir>/presentation_style.css`, §6 above) is a file, not a setting —
listed in the file-locations table.

### 6.3 File-transcriber backend & LLM-translation settings (`[file_transcriber]`, M6 additions)

`FileTranscriberSettings` (F-020/F-023) already backs the options sheet's
sticky prefill (`last_import_folder`/`language`/`task`/`model_size`/`hf_repo`/
`word_timestamps`/`initial_prompt`/`export_formats`); M6 adds these four
fields for the backend selector and per-task LLM translation (F-041/F-031):

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `backend` | `Option<String>` | `None` | Last-selected backend wire string (e.g. `"openai_api"`); `None` = the default local whisper.cpp backend (F-041). |
| `enable_llm_translation` | bool | `false` | Whether AI translation was last enabled in the options sheet (F-031). |
| `llm_model` | `Option<String>` | `None` | Last-entered LLM translation model id, if AI translation was configured. |
| `llm_prompt` | `Option<String>` | `None` | Last-entered LLM translation prompt, if AI translation was configured. |
| `extract_speech` (M8) | bool | `false` | Whether speech isolation (F-027, demucs) was last enabled in the options sheet — the sticky counterpart of `TranscriptionOptions::extract_speech`. The options-sheet checkbox itself is wired to this field in a later M8 task. |

### 6.4 OpenAI-compatible API backend settings (`[openai]`, M6, F-041/F-031)

Read fresh at task-engine-construction time (§1); humm's mapping of Buzz's
`transcriber/custom-openai-base-url` and `transcriber/openai-api-model`
`QSettings` keys. Deliberately has **no API key field** — the key lives only
in the OS keyring via `SecretStore` (§6's keyring row above, ADR-012), never
in this plain TOML file.

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `base_url` | `Option<String>` | `None` | Base URL for the OpenAI-compatible API. `None` uses the client's built-in default, `https://api.openai.com/v1`. |
| `api_model` | String | `"whisper-1"` | Model id used for the OpenAI-compatible transcription endpoint. |

### 6.5 Folder-watch settings (`[folder_watch]`, F-030, M7 Tasks 1/9/15)

Ported from Buzz's `folder-watch/*` `QSettings` group (`main_window.py:167-183`,
design spec §3); configured from the Preferences → Folder Watch tab
(`views/prefs_folder_watch.rs`, Task 15). Watching is off, and every directory
is unset, by default. `enabled`/`delete_processed_files` write through on
checkbox click; `input_dir`/`output_dir` commit on focus-out/Enter (empty →
`None`); `options`' fields commit together as one `SheetState::write_to` call
whenever the embedded auto-task options form changes (§6.3's table above is
that same `FileTranscriberSettings` shape, reused wholesale —
`last_import_folder` is simply unused in this context). No field here can ever
carry API-key material: `FileTranscriberSettings` has no key-shaped field at
all, so even the embedded form's Api Key row (F-041) only ever reaches the OS
keyring (§6's keyring row above), never this table.

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `enabled` | bool | `false` | Whether the folder watcher runs at all. |
| `input_dir` | `Option<PathBuf>` | `None` | Directory scanned for new media files (Buzz's `input_folder`) — the watch's *identity*: changing this restarts the OS-level watch subscription while already running (`ReconcileAction::Restart`); the enabled/disabled transition itself starts or stops the watcher instead (`Start`/`Stop`, not `Restart` — `humm_gui::folder_watch::reconcile_folder_watch`). |
| `output_dir` | `Option<PathBuf>` | `None` | Directory mirrored transcription outputs are written under (Buzz's `output_directory`) — part of the job *template*: changing this updates the running watcher in place, no restart. |
| `delete_processed_files` | bool | `false` | Whether a completed source file is deleted (`true`) or moved into `output_dir` (`false`) after transcription (design spec §3.4) — part of the job template. **Disposition semantics**: Buzz always removes the source from the input tree one way or the other (`file_transcriber.py:155-170`) — this checkbox only chooses delete vs. move; the tab's own caption states this plainly (`prefs-fw-disposition-note`), which Buzz's UI never did. |
| `options` | `FileTranscriberSettings` | see §6.3 | Per-auto-task file-transcription options applied to every watched file (Buzz's `file_transcription_options` sub-group) — part of the job template. |

**Template-override note** (M7 Task 11 review fix, carried here): a watched
file's `OutputSpec.file_name_template` is **not** read from
`options.default_export_template` (that field is part of this per-auto-task
snapshot but nothing in the UI ever writes it) — it's threaded in as an
explicit override from the **global**
`settings.file_transcriber.default_export_template` (§6.3, Preferences →
General) instead, so a General-tab edit to the default export template also
reaches folder-watch tasks without needing its own per-watch setting. This
keeps that field correctly on the *template* half of the watch-identity/
job-template split (`humm_gui::folder_watch`'s own module doc): a Preferences
→ General edit resolves `ReconcileAction::UpdateTemplate`, never `Restart`.

### 6.6 Remaining M7 settings groups (`[ui]`, `[shortcuts]`, `[postproc]`, `recording.backend`)

The other tables M7 added. All are written through on commit by the
Preferences window — there is no OK/Cancel step anywhere (ledger #L8).

**`[ui]`** (F-008/F-100, Preferences → General):

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `locale` | `Option<String>` | `None` | Buzz locale code (e.g. `"de_DE"`) to force the UI into. `None` follows the system locale. Negotiated through Buzz's own fallback chain — exact code → language prefix → `en_US` — so an unshipped code degrades to English rather than failing. Applies **live**: changing it re-renders every open window on the next frame, with no restart hint (ledger #L10). |
| `font_size` | `Option<u32>` | `None` | App-wide text-style size in points. `None` uses egui's built-in default. **Clamped to 8–32 by `Settings::load`**, so a hand-edited out-of-range value is corrected on read rather than honored. |

**`[shortcuts]`** (F-005, Preferences → Shortcuts): a flat
`HashMap<String, String>` of **overrides only** — an action bound to its
default is absent from the file entirely, and clearing a row deletes its
entry. Keys are Buzz's action names (`OPEN_IMPORT_WINDOW`, `CLEAR_HISTORY`, …);
values are Buzz's wire notation, which uses `Ctrl` for the command modifier on
every platform (`"Ctrl+Shift+K"` renders as `⌘⇧K` on macOS). Conflicting
bindings are **allowed and persisted** — the editor warns on both colliding
rows and does not reject the assignment.

**`[recording]`** gains one key beyond §6.2's table:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `backend` | `Option<String>` | `None` (⇒ local Whisper) | Which engine live recording uses — `"whisper"` or `"openai_api"` (F-061b). Saved on window close, not on change (Buzz's own model-type combo has no immediate-save handler either). With `"openai_api"`, Record refuses to start at all unless an OpenAI key resolves from the keyring, failing inline with an actionable message before any session thread spawns (ledger #L18 ✚ — Buzz has no such precheck and surfaces a bad key as fake transcript text). |

**`[postproc]`** (F-110's four natives, Preferences → Post-processing).
**Every hook is disabled by default**, matching Buzz, and a disabled hook
costs a boolean check. Sub-tables, in the fixed execution order they run in
(ledger #L2 — skip is the only *pre*-hook; the rest run after export):

| Sub-table | Feature | Keys |
|-----|-----|-----|
| `skip_already_transcribed` | F-115 | `enabled` (false), `check_result_files` (**true**), `check_database` (false) |
| `transcript_resizer` | F-116 | `enabled` (false), `merge_by_gap`, `merge_gap_seconds`, `split_by_punctuation`, `punctuation`, `split_by_max_length`, `max_length` |
| `ai_summary` | F-111 | `enabled` (false), `api_url`, `model`, `prompt`, `save_to_notes`, `save_to_file`, `output_folder` — **the key is not here**, it is the keyring entry in §6's table |
| `export_docx` | F-113 | `enabled` (false), `output_folder`, `include_timestamps` |
| `noise_reduction` (M8) | F-112 | `enabled` (false), `keep_denoised_file` (false) — whether the DeepFilterNet-denoised `<input_stem>_DeepFilterNet3.wav` is kept next to the input rather than cleaned up |
| `language_detection` (M8) | F-114 | `enabled` (false), `download_tiny_if_missing` (**true**) — whether the `tiny` model is fetched automatically when no already-downloaded model is available to detect with |

A hook failure is logged and reported in the `post_processed` event's `errors`
array (§3); it **never** fails the transcription. Both binaries read this same
table — the CLI has no post-processing flags, so `settings.toml` is the only
switch (§3). `noise_reduction`/`language_detection` are vocabulary as of this
commit — a later M8 task wires their before-transcription hooks into
`run_job`.

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
