# Buzz — Feature Inventory

Ground truth: Buzz source at `/Users/jawad/RustroverProjects/buzz` (v1.4.5). IDs are stable and referenced by `PARITY_MATRIX.md`. Format per feature: what it does / how invoked / inputs / outputs / errors & edges / source.

ID blocks: F-00x app & platform · F-02x import & file transcription · F-04x backends & models · F-06x live recording · F-08x transcript viewer · F-10x preferences · F-11x plugins.

---

## A. Application & platform

### F-001 — GUI application launch
Qt (PyQt6) desktop app. `main()` builds `Application`, parses CLI, shows `MainWindow` (task table), runs event loop. Dark mode auto-detected (`darkdetect`); font "SF Pro" on macOS, size from `font-size` setting; Fusion style on Windows; window geometry persisted (`main-window`). No single-instance guard.
- Invoke: `buzz` binary / `python main.py`.
- Outputs: main window with transcription-task table; DEBUG log at `user_log_dir("Buzz")/logs.txt`.
- Edge: startup flips stale `in_progress`/`queued` DB rows to `canceled`; one-time legacy JSON→SQLite import.
- Source: `buzz/buzz.py`, `buzz/widgets/application.py`, `buzz/widgets/main_window.py`, `buzz/db/helpers.py`.

### F-002 — CLI `add` command
Queues file/URL transcriptions from the command line into the GUI app (window optionally hidden), exits when done (`quit_on_complete=True`). Full grammar in `EXTERNAL_SURFACE.md` §2: `--task/-t`, `--model-type/-m`, `--model-size/-s`, `--hfid`, `--language/-l`, `--prompt/-p`, `--word-timestamps/-w`, `--extract-speech/-e`, `--openai-token`, `--output-directory/-d`, `--srt/--vtt/--txt`, `--hide-gui`; positionals = paths or URLs.
- Errors: no files → exit 1 with help; `huggingface` without `--hfid` → error; `openaiapi` without token in flag or keyring → error. Missing model downloads synchronously first.
- Source: `buzz/cli.py`.

### F-003 — Main window task table
`QSqlTableModel` over `transcription` table. Columns: File Name/URL, Model, Task, Status, Date Completed, Date Added, Notes — visibility/order/width/sort persisted (`transcription-tasks-table/*`), header context menu to toggle/reset. Status cell: In Progress shows `(NN%)` + ETA; Completed shows elapsed; Failed shows error message. Row context menu: Restart (failed/canceled/skipped only), Rename, Add/Edit Notes. Keys: Return=open viewer, Delete=delete, Ctrl+C=copy file name/URL.
- Source: `buzz/widgets/transcription_tasks_table_widget.py`, `buzz/widgets/main_window.py`.

### F-004 — Toolbar & menus
Icon toolbar: Record | New File Transcription, New URL Transcription, Open Transcript, Cancel Transcription, Clear History | Update Available (hidden unless found). Enable rules by selection status (Open→COMPLETED/SKIPPED; Cancel→IN_PROGRESS/QUEUED; Clear→terminal states). File menu: Import File… (Ctrl+O), Import URL… (Ctrl+U), Import Folder…; Help menu: About, Help (online docs), Plugins…, Preferences… (Ctrl+,).
- Source: `buzz/widgets/main_window_toolbar.py`, `buzz/widgets/menu_bar.py`.

### F-005 — Customizable keyboard shortcuts
20 named shortcuts with defaults (recounted from `buzz/settings/shortcut.py` at M3 implementation time — an earlier analysis pass overcounted at 21; e.g. Ctrl+R record, Ctrl+O import, Ctrl+X cancel, Ctrl+S clear history, viewer keys). Editable in Preferences → Shortcuts (QKeySequenceEdit per action, Reset to Defaults); stored as dict under `shortcuts`; re-applied live to menu/toolbar.
- Source: `buzz/settings/shortcut.py`, `buzz/settings/shortcuts.py`, `buzz/widgets/preferences_dialog/shortcuts_editor_preferences_widget.py`, `buzz/widgets/sequence_edit.py`.

### F-006 — Update check & in-app update
On startup (Windows/macOS only, ≤ once/7 days, disable `BUZZ_DISABLE_UPDATE_CHECK`): fetch `version_info.json` from GitHub latest release; toolbar "Update Available" button + dialog with release notes; Download-and-Install fetches installer, launches it (exe/DMG), quits. About dialog has manual "Check for updates".
- Source: `buzz/update_checker.py`, `buzz/widgets/update_dialog.py`, `buzz/widgets/about_dialog.py`.

### F-007 — Telemetry
PostHog `app_launched` event (app version, locale, platform triple) with persisted uuid4 `distinct_id`; opt-out via `BUZZ_DISABLE_TELEMETRY`. (Parity decision → OPEN_QUESTIONS Q-1.)
- Source: `buzz/widgets/application.py:80-93`.

### F-008 — UI localization
gettext domain `buzz`, 15 locales, UI language selectable in Preferences (restart required), default system locale, English fallback.
- Source: `buzz/locale.py`, `buzz/locale/*/LC_MESSAGES/buzz.po`, `buzz/widgets/preferences_dialog/general_preferences_widget.py`.

### F-009 — Logging
DEBUG file log `user_log_dir("Buzz")/logs.txt` (module/line/thread format); child transcription processes log there too; About → "Show logs" opens the folder.
- Source: `buzz/buzz.py`, `buzz/widgets/about_dialog.py`.

---

## B. Import & file transcription

### F-020 — Import files (dialog)
File menu / toolbar → `QFileDialog.getOpenFileNames`, filter groups Media/Audio/Video/All (`.mp3 .wav .m4a .ogg .opus .flac .mp4 .webm .ogm .mov .mkv .avi .wmv`); last-used folder persisted (`file-transcriber/last-import-folder`). Opens options window (F-023).
- Source: `buzz/widgets/main_window.py:268-281`, `buzz/transcriber/transcriber.py:222-225`.

### F-021 — Drag-and-drop import
Drop file URLs anywhere on main window → options window with those paths.
- Source: `buzz/widgets/main_window.py:69,196-205`.

### F-022 — Import URL / Import Folder
URL: dialog with regex validation ("Invalid URL" on bad input); downloaded at task run via yt-dlp (`bestaudio/best`, cookies via `BUZZ_DOWNLOAD_COOKIEFILE`), re-encoded to 16 kHz mono WAV, download progress reported (`fraction_downloaded`). Folder: directory picker, recursive walk for supported extensions, all found files → options window; no files → silently returns.
- Source: `buzz/widgets/import_url_dialog.py`, `buzz/transcriber/file_transcriber.py:75-153`, `buzz/widgets/main_window.py:288-304`.

### F-023 — Transcription options window (per import)
One window per import batch: model type/size (F-040), HF model id w/ live autocomplete (F-041), OpenAI key field (API backend), task (Transcribe / Translate to English), language (~99 + Detect; favorites via `BUZZ_FAVORITE_LANGUAGES`; MMS models swap to ISO-639-3 text input), word-level timings checkbox, extract speech checkbox (F-027), export format checkboxes TXT/SRT/VTT, Advanced dialog: initial prompt, AI-translation enable + model + instructions. Run disabled until required HF id present. All choices persisted (`file-transcriber/*`) and restored next time. Temperature exists in options dataclass (default 6-step tuple) but has **no GUI control**.
- Source: `buzz/widgets/transcriber/*.py`, `buzz/widgets/transcription_options_group_box.py` (in `widgets/transcriber/`), `buzz/widgets/transcriber/advanced_settings_dialog.py`.

### F-024 — Task queue & background execution
FIFO queue on a dedicated worker thread (`FileTranscriberQueueWorker` on `QThread`); one task at a time; per-task subprocess for local inference (spawned `multiprocessing.Process` in `WhisperFileTranscriber`) so cancel is a process kill; statuses QUEUED→IN_PROGRESS→COMPLETED/FAILED/CANCELED/SKIPPED; signals update DB rows live.
- Source: `buzz/file_transcriber_queue_worker.py`, `buzz/transcriber/whisper_file_transcriber.py`, `buzz/widgets/main_window.py:421-490`.

### F-025 — Progress & ETA
Fraction-complete per task (from segment timestamps vs audio duration in child process pipe messages), shown as `(NN%)` + computed ETA in Status column; URL download progress separate (`fraction_downloaded`). Model download progress dialog: determinate w/ ETA for Whisper `.pt`, indeterminate for HF-hub downloads.
- Source: `buzz/transcriber/whisper_file_transcriber.py`, `buzz/widgets/transcription_tasks_table_widget.py:67-107`, `buzz/widgets/model_download_progress_dialog.py`.

### F-026 — Cancel / restart / rename / notes / delete
Cancel (toolbar/Ctrl+X): terminates demucs process if running, stops transcriber, DB → canceled. Restart (context menu, failed/canceled/skipped): rebuilds task from DB row — initial prompt, OpenAI token, LLM options are NOT persisted, so restart uses defaults for those (edge!). Rename + free-text Notes columns. Clear History (Ctrl+S) / Delete key with confirmation.
- Source: `buzz/widgets/main_window.py:232-266` (cancel/delete-confirmation triggers), `buzz/widgets/transcription_tasks_table_widget.py:606-826` (delete, rename, notes, restart — corrects the original `main_window.py:727-826` citation, which pointed at the wrong file: `main_window.py` is only 589 lines and holds none of this logic), `buzz/db/dao/transcription_dao.py:269-370` (`update_transcription_name`/`update_transcription_notes`/`reset_transcription_for_restart` DAO methods, each raising on a missing row), `buzz/file_transcriber_queue_worker.py:357-381` (`add_task`'s canceled-set purge "for restart functionality" plus `cancel_task`).

### F-027 — Speech extraction (Demucs vocal isolation)
Optional pre-pass: Demucs htdemucs separates vocals → `<name>_speech.mp3`, which replaces the input for transcription; `_speech` stripped from export names; runs in its own process (killable); CLI `-e`.
- Source: `buzz/file_transcriber_queue_worker.py:55-108`, `demucs_repo` submodule.

### F-028 — Export on completion
For each selected format, auto-write TXT/SRT/VTT next to input (or `--output-directory`/folder-watch output), filename from template (tokens + `%d-%b-%Y %H-%M-%S` datetime). Exact serialization in `EXTERNAL_SURFACE.md` §3.1-3.2. TXT paragraph split on ≥`BUZZ_PARAGRAPH_SPLIT_TIME` (2000 ms) gaps.
- Source: `buzz/transcriber/file_transcriber.py:56-70,185-239`, `buzz/transcriber/transcriber.py:228-265`.

### F-029 — Task persistence
All tasks + segments in SQLite (`user_data_dir("Buzz")/Buzz.sqlite`; schema in `EXTERNAL_SURFACE.md` §3.5). History repopulates on restart; queued/in-progress are NOT auto-resumed (flipped to canceled at startup).
- Source: `buzz/db/*`, `buzz/schema.sql`.

### F-030 — Folder watch
Preferences tab: enable, input folder, output folder, delete-processed-files, plus a full embedded options form for auto tasks. Recursive `QFileSystemWatcher`; new media auto-queued as `FOLDER_WATCH` tasks; skips hidden/temp/`_speech.mp3`/seen (size+mtime)/queued files; mirrors subdir structure into output folder; optional delete or move of source after completion.
- Source: `buzz/widgets/transcription_task_folder_watcher.py`, `buzz/widgets/preferences_dialog/folder_watch_preferences_widget.py`, `buzz/transcriber/file_transcriber.py:155-170`.

### F-031 — LLM translation (during transcription)
If enabled in options: each produced segment is queued to a `Translator` calling an OpenAI-compatible `chat.completions` (model + system-prompt configurable; batched 10 segments with `[N]` markers parsed back; env overrides `BUZZ_TRANSLATION_API_*`). Translations stored per segment (DB `translation` column) and exportable.
- Source: `buzz/translator.py`, `buzz/transcriber/transcriber.py:153-155`.

---

## C. Backends & model management

### F-040 — Local inference backends (file)
`ModelType` enum: **Whisper** (openai-whisper .pt; stable-whisper used when word timings on), **Whisper.cpp** (subprocess `whisper-cli`, ggml models, Silero VAD when model file present, CoreML on Apple Silicon, Vulkan elsewhere; `BUZZ_WHISPERCPP_N_THREADS`), **Hugging Face** (transformers pipeline; Whisper-family AND MMS/wav2vec2 with ISO-639-3 languages), **Faster Whisper** (CTranslate2 `BatchedInferencePipeline`; hidden on Intel macOS). Selection per task; env defaults `BUZZ_DEFAULT_MODEL_TYPE`/`_SIZE`; `force-cpu` & `reduce-gpu-memory` (int8) settings.
- Standing Policy: humm ships ONE maximally fast local backend (whisper-rs) + API backends — deviation ADR required; HF/MMS coverage → OPEN_QUESTIONS Q-2.
- Source: `buzz/model_loader.py:177-209`, `buzz/transcriber/whisper_file_transcriber.py`, `buzz/transcriber/whisper_cpp.py`, `buzz/transformers_whisper.py`.

### F-041 — OpenAI-compatible API backend
`audio/transcriptions` / `audio/translations`; model setting default `whisper-1`; custom base URL setting; files > 25 MB re-encoded/chunked via ffmpeg + ffprobe; key from keyring (set in Preferences or options widget; "Test" button validates via `models.list()`).
- Source: `buzz/transcriber/openai_whisper_api_file_transcriber.py`, `buzz/widgets/preferences_dialog/general_preferences_widget.py`.

### F-042 — Model sizes & custom models
`WhisperModelSize`: tiny…large-v3-turbo (+.en variants), `custom` (Whisper.cpp: user ggml URL → `ggml-model-whisper-custom.bin`; Faster Whisper: arbitrary HF repo id via setting), `lumii` (Latvian, lv_LV locale only). Size→name mapping: `large`→large-v1 for whisper.cpp/faster-whisper.
- Source: `buzz/model_loader.py:128-175`.

### F-043 — Model download & cache
Cache root `user_cache_dir("Buzz")/models` (`BUZZ_MODEL_ROOT` override), `.buzz_complete` sentinels; whisper `.pt` with HTTP-Range resume + SHA256; HF-hub snapshot downloads in killable child process; CoreML encoder zip on arm64; progress dialog with cancel. Layout details `EXTERNAL_SURFACE.md` §3.8.
- Source: `buzz/model_loader.py`.

### F-044 — Model management UI
Preferences → Models: per-group (Whisper, Whisper.cpp, Faster Whisper) tree of Downloaded vs Available; Download w/ progress; Delete (confirm); Show file location; custom model inputs; "What model should I use?" FAQ link.
- Source: `buzz/widgets/preferences_dialog/models_preferences_widget.py`.

### F-045 — VAD (file transcription, whisper.cpp backend)
`whisper-cli` invoked with `--vad --vad-model ggml-silero-v6.2.0.bin` when the model file exists next to the binary (shipped at build time); token offsets rescaled back to original timeline (`_make_offset_mapper`). Absent model file → silently no VAD.
- Source: `buzz/transcriber/whisper_cpp.py:48,182,377`, `Makefile:74-112`.

---

## D. Live recording

### F-060 — Live recording window
Ctrl+R / toolbar Record. Device combo (input devices, default preselected), audio meter (avg volume + queue depth), Record/Stop toggle, same options group as files (word timings forced off) + recording-specific advanced settings; transcription + (optional) translation read-only panels; settings persisted `recording-transcriber/*` on close.
- Source: `buzz/widgets/recording_transcriber_widget.py`, `buzz/widgets/audio_devices_combo_box.py`, `buzz/widgets/audio_meter_widget.py`, `buzz/widgets/record_button.py`.

### F-061 — Real-time transcription loop
sounddevice mono float32 stream at 16 kHz (fallback to device default rate); chunks of 5 s (or `transcription_step`=3.5 s in correct-mode); silence-aware cut points (RMS scan) and skip-below-`silence_threshold` (0.0025); previous text fed as rolling initial prompt; drops audio if queue > 3 batches behind. Backends: Whisper/Faster-Whisper/HF in-process; OpenAI API per-chunk temp WAV; **Whisper.cpp spawns local `whisper-server` on 127.0.0.1:3003** and posts chunks (hallucination guards `--max-context 8 --entropy-thold 2.8 --suppress-nst`).
- Source: `buzz/transcriber/recording_transcriber.py`.

### F-062 — Display modes
`Append below` / `Append above` / `Append and correct` (sliding-window overlap merge with confirm/unconfirm split, optional "hide unconfirmed", 1.5 s keep-back).
- Source: `buzz/settings/recording_transcriber_mode.py`, `buzz/widgets/recording_transcriber_widget.py:882-961`.

### F-063 — Live translation
Same LLM translator as F-031 fed with live chunks; separate translation panel; exported to a `.translated` sibling file when live export on.
- Source: `buzz/widgets/recording_transcriber_widget.py`, `buzz/translator.py`.

### F-064 — Live export & upload
Continuous export to TXT (append/prepend + separator + optional max-entries trim) or single-row CSV (utterance-per-column, max-entries, utf-8-sig, Windows-lock retries); optional per-utterance `POST $BUZZ_UPLOAD_URL {"kind":"transcript","text","mode"}` + "clear" on start.
- Source: `buzz/widgets/recording_transcriber_widget.py:484-531,786-863,1021-1101`.

### F-065 — Presentation window & copy
While recording: presentation bar (open big-text window, size 10-100 pt, Light/Dark/Custom colors, fullscreen w/ Esc, user CSS override at `<cache>/Buzz/presentation_window_style.css`). When stopped: Copy button (whole transcript → clipboard, "Copied!" feedback).
- Source: `buzz/widgets/presentation_window.py`, `buzz/widgets/recording_transcriber_widget.py:267-357`.

---

## E. Transcript viewer

### F-080 — Viewer window
Open from table (double-click/Return/toolbar). Title = file name. View modes: Timestamps (segment table + player), Text (paragraphs, gap-split), Translation (Ctrl+E/T/L). Splitter + geometry persisted per media kind.
- Source: `buzz/widgets/transcription_viewer/transcription_viewer_widget.py`.

### F-081 — Segment editing
Editable Start/End (HH:MM:SS.mmm delegate, +/- = ±500 ms, neighbor-overlap clamping) and Text/Translation (word-wrap editor); edits persist to DB immediately.
- Source: `buzz/widgets/transcription_viewer/transcription_segments_editor_widget.py`.

### F-082 — Search
Ctrl+F bar; 300 ms debounce; text+translation; 100-result cap; N-of-M label; Ctrl+Enter / Shift+Enter next/prev; Esc closes.
- Source: `transcription_viewer_widget.py:372-1076`.

### F-083 — Synced playback (audio & video)
Audio: sounddevice engine (fallback QMediaPlayer), scrubber, persisted playback rate. Video files get an ffmpeg-decoded frame player with synced audio. Loop-segment, Follow Audio auto-scroll, speed 0.5-2× (clamp 0.1-5), current-segment frame follows position (binary search); segment click seeks. Shortcuts Ctrl+P / Ctrl+Shift+P / Ctrl+Alt+P / Ctrl+G.
- Source: `buzz/widgets/transcription_viewer/audio_player.py`, `buzz/widgets/video_player.py`, `buzz/ffmpeg_video_player.py`, `buzz/sounddevice_player.py`.

### F-084 — Export from viewer
Menu: {TXT,SRT,VTT} × {Text, Translation} (translation entries appear once translations exist) → save dialog → same writers as F-028. DOCX via plugin (F-113).
- Source: `buzz/widgets/transcription_viewer/export_transcription_menu.py`.

### F-085 — Translate (post-hoc)
Toolbar Translate: requires OpenAI key + AI model/prompt configured; enqueues all segments to Translator; translations land in DB/table live.
- Source: `transcription_viewer_widget.py:1300-1324`.

### F-086 — Resize / merge transcript
Resizer window: create-new-vs-in-place; extend end times (default +0.2 s); without word timings → target line length (srt_equalizer, default 42 chars); with word timings → merge-gap / split-punctuation / split-max-length via stable-whisper regroup rules (`BUZZ_MERGE_REGROUP_RULE` override). Runs as a new task; stable-ts VAD option present but hardcoded off (broken in frozen builds — in-code TODO).
- Source: `buzz/widgets/transcription_viewer/transcription_resizer_widget.py`, `buzz/plugins/transcript_resizer/plugin.py`.

### F-087 — Speaker identification
Viewer button (hidden on Intel macOS): Step 1 diarization (NeMo MSDD via `whisper_diarization` submodule + ctc-forced-aligner word alignment + punctuation-based realignment via deepmultilingualpunctuation, progress + cancel), Step 2 name speakers (play sample per speaker, merge-sentences option) → labels written into segments. Heavy PyTorch stack → OPEN_QUESTIONS Q-3.
- Source: `buzz/widgets/transcription_viewer/speaker_identification_widget.py`, submodules `whisper_diarization`, `ctc_forced_aligner`, `deepmultilingualpunctuation`.

---

## F. Preferences

### F-100 — General tab
UI language (restart hint), font size 8-32 (live), OpenAI key (keyring, Test button), custom base URL, API model, default export filename template, live-export enable/folder/mode, Reduce GPU RAM (int8), Disable GPU.
- Source: `buzz/widgets/preferences_dialog/general_preferences_widget.py`.

### F-101 — Models tab → F-044. Shortcuts tab → F-005. Folder Watch tab → F-030.
- Source: `buzz/widgets/preferences_dialog/preferences_dialog.py` (4 tabs, Ok/Cancel, 650×500 min).

---

## G. Plugins

### F-110 — Plugin system
Folder-per-plugin with `plugin.py` (`BuzzPlugin` subclass); hooks `before_transcription` (may swap audio path), `check_skip` (return cached segments), `after_transcription` (modify segments), `on_complete` (side effects); declarative config fields (TEXT/TEXTAREA/BOOL/PASSWORD→keyring); per-plugin locale JSON; pip-dependency auto-install to a deps dir; enable/disable + execution order persisted (`plugins`); install from `.zip` URL; management dialog with full-access warning. Post-hooks run on background thread, DB writes marshaled to main thread. (Rust plugin strategy → OPEN_QUESTIONS Q-4.)
- Source: `buzz/plugins/{base,manager,loader,post_processing}.py`, `buzz/widgets/plugins_dialog/`.

### F-111..F-116 — Built-in plugins
| ID | Plugin | Hook | Function |
|---|---|---|---|
| F-111 | ai_summary | on_complete | LLM summary of transcript (own API key/base URL/prompt/output folder) |
| F-112 | deep_filter_net | before_transcription | DeepFilterNet3 noise reduction pre-pass (pip dep `deepfilternet`) |
| F-113 | export_docx | on_complete | DOCX export |
| F-114 | enhanced_language_detection | before_transcription | improved language detection |
| F-115 | skip_already_transcribed | check_skip | skip files already in DB |
| F-116 | transcript_resizer | on_complete | auto-resize segments after completion |
- Source: `buzz/plugins/<name>/plugin.py`.

---

## Cross-cutting behaviors (apply to many features)

- **Concurrency:** UI thread never blocks — queue worker thread; local inference in killable child processes; model downloads in threads/child processes; translator on its own thread.
- **Cancellation:** cooperative + process-kill; cancel always leaves a consistent DB state.
- **Unhappy paths:** missing model → download-or-error dialog referencing `BUZZ_MODEL_ROOT`; keyring unavailable → error dialog (macOS) / portal fallback (Linux); missing ffmpeg → task failure surfaced in Status column; no audio device → device combo empty.
- **Secrets:** OpenAI key only in OS keyring (or Linux portal fallback file), never QSettings/DB.
