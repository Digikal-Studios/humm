# Buzz — User Journeys

Step-by-step narratives of what the user does and sees, launch → outcome, from the source (files cited in `FEATURE_INVENTORY.md` by ID). ⚠ marks friction/latency/decision points — these are design opportunities for humm, not constraints.

## J-1: First-run file transcription (GUI)

1. Launch Buzz → main window with an empty task table. ⚠ Cold start is slow: Python + Qt + torch imports (seconds).
2. Ctrl+O (or toolbar/drag-drop/File menu) → native file picker filtered to media types; multi-select OK.
3. Options window opens: model type+size, task, language (default Detect), word timings, extract speech, export formats, Advanced (prompt, AI translation). Previous choices restored. ⚠ First-time users face ~8 decisions; defaults (Whisper/tiny) work but are quality-poor; no guidance inline (the "what model?" FAQ link lives in Preferences → Models).
4. Run → if model missing, modal download dialog with progress/ETA (determinate for Whisper .pt, ⚠ indeterminate spinner for HF-hub downloads — no ETA, cancel only). ⚠ Download is modal-per-window, blocking that import.
5. Window closes; task appears QUEUED then IN_PROGRESS with % + ETA in the table. UI stays responsive (worker thread + child process).
6. On COMPLETED: selected formats auto-write next to the input (templated filename). Double-click row → viewer.
7. Failure modes: FAILED status shows error text inline in the table row. ⚠ Errors can be cryptic (raw exception strings).

## J-2: CLI batch transcription

1. `buzz add -m whispercpp -s small --srt --txt -d out/ *.mp3` (or URLs).
2. Missing model downloads synchronously (progress on stderr). ⚠ GUI still launches — CLI is not headless; `--hide-gui` merely hides the window; needs a display server. ⚠ Exit code is 0/1 only; no per-file status output; no machine-readable summary.
3. Tasks run sequentially; app quits when done; exports land in `-d`.

## J-3: URL transcription

1. Ctrl+U → paste URL → validated → options window (same as J-1 from step 3).
2. Task shows download progress first (yt-dlp), then transcribes. ⚠ Site auth requires an env-var cookie file — invisible in UI.

## J-4: Watched folder ("drop files, get subtitles")

1. Preferences → Folder Watch: enable, pick input/output folders, configure the embedded options form once, OK.
2. Any media dropped into the folder tree auto-queues with those options; output mirrors subfolder structure; optionally deletes source. Runs whenever the app is open. ⚠ Only while the app runs (no daemon); config buried in Preferences.

## J-5: Live recording / captioning

1. Ctrl+R → Live Recording window: device (default preselected), meter, options (model etc.), Record.
2. If model missing → download dialog first. ⚠ Live models must be small enough for realtime; nothing warns the user before they pick large-v3.
3. Speech appears in ~3.5-5 s chunks in the chosen mode (append below/above, or append-and-correct sliding merge). Silence is skipped. ⚠ Latency is inherent to chunking; whisper.cpp path adds a local-server spawn on first use.
4. Optional: AI translation panel; live export to TXT/CSV; presentation window (big text, fullscreen, themed) for e.g. lectures/services; per-utterance POST to a URL (env-only config ⚠).
5. Stop → Copy button for whole transcript. ⚠ Live sessions are NOT saved to the task table/DB — closing loses the text unless exported/copied (only file-transcription tasks persist).

## J-6: Review, correct & export a transcript

1. Double-click a completed task → viewer: segment table + synced player (video shows frames), current line highlighted, Follow Audio, loop segment, speed control.
2. Edit text/translation inline; nudge start/end (±0.5 s keys or timestamp edit, overlap-clamped); changes save to DB instantly.
3. Ctrl+F search; Ctrl+E/T/L switch text/timestamps/translation views.
4. Export menu → format × text/translation → save dialog. DOCX if plugin enabled.
5. Optional Resize flow: fit-lines-to-42-chars or merge/split rules (word-timing dependent ⚠ — options change based on how the transcript was made) → runs as a new task.

## J-7: Speaker identification

1. Viewer → Identify Speakers (not on Intel macOS): Step 1 runs diarization with progress (minutes; GPU helps). ⚠ Very heavy — NeMo+torch; model downloads on first use; cancel supported.
2. Step 2: name each detected speaker (play a sample to identify voices), optionally merge consecutive same-speaker sentences → labels written into transcript.

## J-8: Translate a transcript with an LLM

1. Configure once: Preferences → General → OpenAI key (+ Test), optionally custom base URL (local LLMs OK).
2. Either per-task (Advanced → enable AI translation + model + instructions) for live translation during transcription, or post-hoc Translate button in the viewer. ⚠ Prompt defaults to English→Spanish example; model id must be typed by hand (`add-model-id-here` placeholder); errors surface as dialogs.
3. Translation column fills in as responses arrive; exportable per format; live mode writes a `.translated` file.

## J-9: Models housekeeping

1. Preferences → Models: pick group (Whisper/Whisper.cpp/Faster Whisper), see Downloaded vs Available; download with progress; delete; open file location. Custom: ggml URL (whisper.cpp) or HF repo id (faster-whisper). ⚠ HF-type and API backends have no management UI; cache can also be redirected via env only (`BUZZ_MODEL_ROOT`).

## J-10: Recover / redo work

1. Restart app → history intact; anything mid-flight shows CANCELED (never auto-resumed ⚠).
2. Context menu → Restart Transcription on failed/canceled/skipped rows. ⚠ Restart silently loses initial prompt / LLM options / API token (not persisted in DB) — a real parity trap.
3. Clear History removes selected terminal rows (confirm dialog). ⚠ Ctrl+S ("Clear History") is a surprising binding.

## J-11: Customize the app

1. Preferences: UI language (restart required ⚠), font size (live), shortcuts (per-action editor + reset), GPU toggles.
2. Plugins dialog: enable/disable/order/install-from-zip-URL (full-access warning), per-plugin settings forms.

## Journey-level observations for humm

- The five-backend choice (J-1 step 3) exists to trade speed/quality/compat — humm's single fast local backend + API collapses this to model-size choice only.
- Modal, per-window model downloads (J-1, J-5) could be background + non-blocking with the task queued behind the download.
- Live sessions not persisting (J-5) is Buzz's biggest data-loss trap; cheap to fix in humm's design.
- Restart losing options (J-10) argues for persisting the full options struct with each task.
- CLI headlessness (J-2) is the clearest structural win available to humm.
