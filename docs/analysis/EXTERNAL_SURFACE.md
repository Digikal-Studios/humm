# Buzz — External Surface (Observable Contract)

Source of truth: Buzz source at `/Users/jawad/RustroverProjects/buzz` (package `buzz-captions` v1.4.5, Python >=3.12,<3.13, PyQt6 6.9.1). Every claim cites the implementing file. This document defines the contract humm must honor (or deliberately deviate from, with an ADR).

## 1. Process model & entry points

- Entry points `main.py`, `buzz/__main__.py`, and the `buzz` console script all call `buzz.buzz.main()` (`buzz/buzz.py`).
- Buzz is **always a Qt GUI app**. `main()` constructs `Application(sys.argv)`, runs `parse_command_line(app)`, then `app.show_main_window()` and `app.exec()`. The CLI does not run headless — it queues tasks into the same GUI application (optionally hiding the window with `--hide-gui`).
- Bootstrap side effects (`buzz/buzz.py`): sets `HF_HOME` (from `BUZZ_MODEL_ROOT` else `user_cache_dir("Buzz")`), configures DEBUG logging to `user_log_dir("Buzz")/logs.txt`, Linux forces `multiprocessing.set_start_method("spawn")`, Windows PATH/DLL setup.

**humm note:** humm's CLI should be genuinely headless (parity of outcome, better design); parity requirement is the *behavior* — `add` queues transcriptions that run to completion and export.

## 2. CLI grammar (`buzz/cli.py`, Qt `QCommandLineParser`)

One subcommand: **`add`**. No args → GUI launches normally. Built-ins: `--help`/`-h`, `--version`/`-v`.

```
buzz add [options] [file file file ...]
```

| Option | Long/short | Default | Values / notes |
|---|---|---|---|
| Task | `--task`, `-t` | `transcribe` | `translate`, `transcribe` |
| Model type | `--model-type`, `-m` | `whisper` | `whisper`, `whispercpp`, `huggingface`, `fasterwhisper`, `openaiapi` |
| Model size | `--model-size`, `-s` | `tiny` | `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`, `medium.en`, `large`, `large-v2`, `large-v3`, `large-v3-turbo`, `custom`, `lumii` |
| HF model ID | `--hfid` | — | required iff `--model-type huggingface` |
| Language | `--language`, `-l` | `""` | ISO code from Whisper language table; empty = auto-detect |
| Initial prompt | `--prompt`, `-p` | `""` | |
| Word timestamps | `--word-timestamps`, `-w` | off | flag |
| Extract speech | `--extract-speech`, `-e` | off | flag (Demucs vocal isolation pre-pass) |
| OpenAI token | `--openai-token` | keyring | `openaiapi` only; falls back to OS keyring; error if neither |
| Output dir | `--output-directory`, `-d` | input file's dir | |
| Export SRT | `--srt` | off | flag |
| Export VTT | `--vtt` | off | flag |
| Export TXT | `--txt` | off | flag |
| Hide GUI | `--hide-gui` | off | window hidden while tasks run |

- Positionals: file paths **or URLs** (`is_url()` = has scheme+netloc → task source `URL_IMPORT`).
- Zero files → `CommandLineError("No input files")` → message + help on stderr, `sys.exit(1)`.
- Model resolution: downloads the model synchronously if not cached (`_resolve_model` → `ModelDownloader.run()`).
- Tasks queue with `quit_on_complete=True` — app exits when CLI-queued tasks finish.

## 3. Files written / read

### 3.1 Transcript exports (batch, `write_output()` in `buzz/transcriber/file_transcriber.py:185-229`)

Formats: `OutputFormat` = **TXT, SRT, VTT** (`buzz/transcriber/transcriber.py:212`). Each can be serialized from `text` or `translation` (`segment_key` param). Timestamp formatter `to_timestamp(ms, ms_separator)` → `HH:MM:SS{sep}mmm` (zero-padded, hours 2+ digits).

- **TXT**: `segment.text.strip() + " "` concatenated; a paragraph break (`\n\n`) is inserted when `segment.start − prev.end ≥ BUZZ_PARAGRAPH_SPLIT_TIME` (env, default **2000 ms**).
- **SRT**: `{i+1}\nHH:MM:SS,mmm --> HH:MM:SS,mmm\n{text}\n\n` (comma separator).
- **VTT**: header `WEBVTT\n\n`, then `HH:MM:SS.mmm --> HH:MM:SS.mmm\n{text}\n\n` (dot separator).

Golden fixtures exist inline in `tests/transcriber/transcriber_test.py:14-28` (exact SRT/VTT strings) — port these as humm snapshot tests.

### 3.2 Export file naming (`get_output_file_path`, `transcriber.py:228-265`)

Template tokens: `{{ input_file_name }}`, `{{ task }}`, `{{ language }}`, `{{ model_type }}`, `{{ model_size }}`, `{{ date_time }}` (format `%d-%b-%Y %H-%M-%S`). Default template: `"{{ input_file_name }} ({{ task }}d on {{ date_time }})"` (`settings.py:170`). A trailing `_speech` suffix (from speech extraction) is stripped from the input name. Default directory: `output_directory or dirname(input)`. Extension appended as `.{txt|srt|vtt}`.

### 3.3 Live-recording exports (`buzz/widgets/recording_transcriber_widget.py`)

Settings-driven continuous export to **TXT or CSV** during live transcription:
- TXT (`write_txt_export`, line 839): append / prepend / overwrite modes with `line_separator`, optional `max_entries` cap (keeps last N in append mode, first N in prepend).
- CSV (`write_csv_export`, line 804): a **single-row CSV**, each new utterance appended as a new column, `max_entries` keeps the last N columns; `utf-8-sig` encoding; 5 retries with 0.2s delay for Windows file locks.
- Optional HTTP upload of each utterance: `POST {BUZZ_UPLOAD_URL}` JSON `{"kind":"transcript","text":...,"mode":"append|replace"}` (line 1022; only when env is set).

### 3.4 DOCX export (plugin `buzz/plugins/export_docx/plugin.py`)

Adds a DOCX exporter to the transcription viewer (reuses `to_timestamp`).

### 3.5 SQLite database (`buzz/db/`)

Location: `user_data_dir("Buzz")/Buzz.sqlite`. Opened with stdlib sqlite3 for migration, then Qt QSQLITE with `PRAGMA foreign_keys = ON`. Schema (`buzz/schema.sql`, verbatim):

```sql
CREATE TABLE transcription (
    id TEXT PRIMARY KEY,
    error_message TEXT,
    export_formats TEXT,
    file TEXT,
    output_folder TEXT,
    progress DOUBLE PRECISION DEFAULT 0.0,
    language TEXT,
    model_type TEXT,
    source TEXT,
    status TEXT,
    task TEXT,
    time_ended TIMESTAMP,
    time_queued TIMESTAMP NOT NULL,
    time_started TIMESTAMP,
    url TEXT,
    whisper_model_size TEXT,
    hugging_face_model_id TEXT,
    word_level_timings BOOLEAN DEFAULT FALSE,
    extract_speech BOOLEAN DEFAULT FALSE,
    name TEXT,
    notes TEXT
);

CREATE TABLE transcription_segment (
    id INTEGER PRIMARY KEY,
    end_time INT DEFAULT 0,
    start_time INT DEFAULT 0,
    text TEXT NOT NULL,
    translation TEXT DEFAULT '',
    transcription_id TEXT,
    FOREIGN KEY (transcription_id) REFERENCES transcription(id) ON DELETE CASCADE
);
CREATE INDEX idx_transcription_id ON transcription_segment(transcription_id);
```

- `id` is a UUID string; segment times are integer **milliseconds**; segments carry both `text` and `translation`.
- Migration: declarative diff (`buzz/db/migrator.py`, Stb-tester `dumb_migrate_db`) — pristine in-memory DB from `schema.sql`, ALTER live DB to match.
- Startup: `in_progress`/`queued` rows are flipped to `canceled` (`db/helpers.py`); one-time import of the legacy JSON cache (`copy_transcriptions_from_json_to_sqlite`).
- **humm migration question**: whether to read `Buzz.sqlite` for user data migration → see OPEN_QUESTIONS.

### 3.6 Settings (Qt `QSettings`, org `"Buzz"`, app `""`)

Native backend per OS: macOS `~/Library/Preferences/*.Buzz.plist`, Linux `~/.config/Buzz/Buzz.conf`, Windows registry. Wrapper `buzz/settings/settings.py` (`Settings.Key`, lines 16-94). Key groups:

- `recording-transcriber/*`: task, model, language, initial-prompt, enable-llm-translation, llm-model, llm-prompt, export-enabled, export-folder, mode, silence-threshold, line-separator, transcription-step, export-file-type, export-max-entries, export-file-name, hide-unconfirmed
- `presentation-window/*`: text-color, background-color, text-size, theme
- `file-transcriber/*`: task, model, language, initial-prompt, enable-llm-translation, llm-model, llm-prompt, word-level-timings, export-formats, last-import-folder
- `transcriber/*`: default-export-file-name, custom-openai-base-url, openai-api-model, custom-faster-whisper-id, huggingface-model-id
- `transcription-tasks-table/*`: column-visibility, column-order, column-widths, sort-state
- Top-level: `shortcuts`, `font-size`, `ui-locale`, `user-identifier` (auto-uuid4), `main-window` (geometry), `transcription-viewer`, `audio/playback-rate`, `force-cpu`, `reduce-gpu-memory`, `update/last-check`, `update/available-version`, `plugins`, `transcription-resizer/create-new-transcript`

Notable defaults: OpenAI API model `whisper-1`; custom OpenAI base URL `""` (SDK default); export template as §3.2; `force-cpu`/`reduce-gpu-memory` false.

### 3.7 Secrets (`buzz/store/keyring_store.py`)

- OS keyring, service `"Buzz"`. Static key: `"OpenAI API key"`.
- Linux sandbox fallback: XDG Desktop Portal master secret + PBKDF2-HMAC-SHA256 (100k iters) per-key derivation, XOR+base64 values in `user_data_dir("Buzz")/.secrets.json` chmod 0600 (in-code noted as weak).
- Dynamic plugin secrets: `plugin:<id>:<field>` names (e.g., ai_summary API key).

### 3.8 Model storage (`buzz/model_loader.py`)

Root: `user_cache_dir("Buzz")/models`, override `BUZZ_MODEL_ROOT` (line 119-122). `.buzz_complete` sentinel marks finished downloads. Layout by backend:

- OpenAI Whisper `.pt`: `<root>/whisper/` — downloaded from `whisper._MODELS` URLs with HTTP-Range resume + SHA256 check.
- whisper.cpp ggml `.bin`: HF hub cache layout under `<root>` (`snapshot_download`, repo `ggerganov/whisper.cpp`; Latvian `lumii` from `RaivisDejus/whisper.cpp-lv`); custom model at `<root>/ggml-model-whisper-custom.bin`; macOS arm64 also CoreML encoder `ggml-<size>-encoder.mlmodelc` (zip extracted).
- faster-whisper CTranslate2: HF repos `Systran/faster-whisper-<size>`; turbo `mobiuslabsgmbh/faster-whisper-large-v3-turbo`; or custom HF id.
- Hugging Face models: `snapshot_download` with allow-patterns (safetensors/bin/tokenizer/config).
- Silero VAD `ggml-silero-v6.2.0.bin`: shipped **next to the whisper-cli binary** at build time (Makefile), not in the model cache.

### 3.9 Logs

`user_log_dir("Buzz")/logs.txt`, DEBUG level, format includes module/line/thread (`buzz/buzz.py`). Child processes also log there (whisper_file_transcriber).

## 4. Network endpoints

| Purpose | Endpoint | Notes |
|---|---|---|
| Model downloads (whisper .pt) | `whisper._MODELS[size]` public Azure blob URLs | resume + SHA256 |
| Model downloads (ggml/CT2/HF) | huggingface.co via `huggingface_hub` | repos listed in §3.8 |
| Silero VAD (build-time) | `https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin` | Makefile |
| OpenAI transcription/translation API | configurable base URL (`transcriber/custom-openai-base-url` setting, default `https://api.openai.com`), `audio/transcriptions`, `audio/translations` | model default `whisper-1`; files >25 MB are chunked; `max_retries=0` |
| LLM translation | same OpenAI-compatible client; `chat.completions` | env overrides `BUZZ_TRANSLATION_API_BASE_URl` (sic), `BUZZ_TRANSLATION_API_KEY` |
| Local whisper.cpp server (live) | `http://127.0.0.1:3003` (`recording_transcriber.py`), unused variant on `:3000` | spawned `whisper-server` subprocess |
| Update check | `https://github.com/chidiwilliams/buzz/releases/latest/download/version_info.json` | ≤ every 7 days; Windows/macOS only; disable `BUZZ_DISABLE_UPDATE_CHECK` |
| Telemetry | PostHog `https://us.i.posthog.com`, event `app_launched` | hardcoded project key; disable `BUZZ_DISABLE_TELEMETRY`; distinct_id = persisted uuid |
| URL import | arbitrary media URLs via `yt-dlp` (`bestaudio/best`) | cookies via `BUZZ_DOWNLOAD_COOKIEFILE` |
| Live transcript upload | `POST $BUZZ_UPLOAD_URL` | opt-in via env |

**humm note:** telemetry parity is a product decision, not a technical requirement → OPEN_QUESTIONS.

## 5. Environment variables

`BUZZ_MODEL_ROOT`, `HF_HOME` (derived), `BUZZ_DEFAULT_MODEL_TYPE`, `BUZZ_DEFAULT_MODEL_SIZE`, `BUZZ_FORCE_CPU`, `BUZZ_REDUCE_GPU_MEMORY`, `BUZZ_WHISPERCPP_N_THREADS`, `BUZZ_DISABLE_TELEMETRY`, `BUZZ_DISABLE_UPDATE_CHECK`, `BUZZ_DOWNLOAD_COOKIEFILE`, `BUZZ_UPLOAD_URL`, `BUZZ_PARAGRAPH_SPLIT_TIME` (default 2000), `BUZZ_TRANSLATION_API_BASE_URl` (typo is in Buzz), `BUZZ_TRANSLATION_API_KEY`, `BUZZ_FAVORITE_LANGUAGES`, `BUZZ_MERGE_REGROUP_RULE`, `BUZZ_ICON_PATH`, `BUZZ_LARGE_ICON_PATH`.

## 6. OS integration

- **Drag & drop** onto main window (file URLs → import) — `main_window.py:69,196-204`.
- **Folder watch**: recursive `QFileSystemWatcher` over a configured input dir; new media files auto-queued (`Source.FOLDER_WATCH`); skips hidden files, temp `.ogg.wav`, `_speech.mp3`, already-seen (size+mtime) files; preserves subdir structure in output dir; optional delete/move of source after transcription (`widgets/transcription_task_folder_watcher.py`, `file_transcriber.py:155-170`).
- **Audio devices**: PortAudio via `sounddevice`; input devices with `max_input_channels > 0`; default input preselected.
- **Supported media extensions** (import dialog filter + folder watch): `.mp3 .wav .m4a .ogg .opus .flac .mp4 .webm .ogm .mov .mkv .avi .wmv`.
- **No system tray**, no native OS notifications (in-window QMessageBox only).
- File associations: none registered in code (packaging-level `Buzz.desktop`, Inno Setup, snap/flatpak/appimage).
- Dark mode: `darkdetect` dependency (used for theme detection).

## 7. i18n

- UI: GNU gettext, domain `buzz`, locale dir `buzz/locale`; UI language from setting `ui-locale` (default system `QLocale().name()`), fallback English. 15 languages, in `ui_locales`' own dict order (`general_preferences_widget.py:34-50` — this is also the Preferences language combo's render order, not just an unordered set): en_US, ca_ES, da_DK, nl, de_DE, es_ES, it_IT, ja_JP, lv_LV, pl_PL, pt_BR, ru, uk_UA, zh_CN, zh_TW (`.po` compiled to `.mo` at build).
- Transcription languages: ~100-entry Whisper map in `transcriber.py:37-138` (distinct from UI locales); `BUZZ_FAVORITE_LANGUAGES` pins favorites.

## 8. Exit codes

- CLI parse/validation error → stderr message + help, exit **1** (`cli.py` CommandLineError handler).
- Normal GUI exit → Qt `app.exec()` return (0).
- `--help`/`--version` → Qt parser built-in behavior, exit 0.
- No dedicated per-task failure exit codes observed; CLI-queued tasks report failure in-GUI (with `quit_on_complete`). **humm note:** headless CLI should define real exit codes (deviation to log).
