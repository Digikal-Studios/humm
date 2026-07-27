# humm — UX Specification

Redesign of every Phase 0 journey (`../analysis/USER_JOURNEYS.md`). Parity = the user can accomplish everything Buzz allowed — not Buzz's friction. Each ✚ marks a deliberate improvement over Buzz (traceable; the underlying capability row stays parity-governed).

## 1. Screen inventory & navigation

| Screen | Reached from | Notes |
|---|---|---|
| **Main window** | launch | task table + toolbar + menu; the hub |
| **New-transcription sheet** | import actions, drag-drop | modal *sheet over main* (not a floating orphan window ✚) |
| **Transcript viewer** | double-click/Return on row | own OS window, many may be open |
| **Live recording** | Ctrl+R | own OS window |
| **Presentation view** | from live recording | own OS window, fullscreen-capable |
| **Preferences** | Ctrl+, | tabs: General · Models · Shortcuts · Folder Watch · Privacy ✚ |
| **About / update notice** | menu / toolbar chip | notify-only updates (ADR-015) |

Empty states everywhere: fresh main table shows a drop-target hero ("Drop media here, or ⌘O") ✚; empty model list links straight to a download; viewer with zero segments explains why (e.g., silence-only input).

## 2. Journey redesigns

### J-1 File transcription
- Cold launch < 300 ms to interactive (perf value #2). Import via dialog, drag-drop, URL, or folder — all converge on the **options sheet**, prefilled from last use (parity) with a one-line summary header ("Whisper · small · auto language · SRT+TXT") so repeat runs are Enter-Enter ✚.
- **Model downloads are non-modal** ✚: choosing an absent model, the task queues normally; the model downloads as the running task's first phase, with progress in its own row; imports never block (fixes J-1 ⚠ modal downloads). All downloads (incl. HF-hub ones) show determinate progress + ETA ✚ (we control the HTTP client).
- Task rows: status chip with %, ETA, and *phase* (`downloading media / downloading model / isolating speech / transcribing / exporting`) ✚ — Buzz collapsed these into one opaque %.
- Errors: status chip `Failed` + one-line human cause; click → detail popover with full error, log-file link, and a **Retry** button. Retry reuses the *fully persisted* options (fixes Buzz's restart-loses-options trap, F-026/ARCH §4) ✚.

### J-2 CLI
- `humm add …` — Buzz flag spelling, but headless, with per-file progress bars (TTY), `--json` events for scripting, and exit codes 0/1/2 (ADR-007) ✚. `humm add --help` documents every flag with defaults shown.

### J-3 URL import — same sheet; download phase visible; cookie file configurable in Preferences → General ✚ (Buzz: env var only).

### J-4 Folder watch — parity config in Preferences; additionally the main window shows an active "Watching <folder>" pill with pause toggle ✚ (Buzz gave no ambient indication).

**Shipped M7** (F-030): live end to end, and the ✚ pill is real — confirmed in a windowed acceptance run against **production FSEvents**, not a test seam (`docs/gui-manual/M7.md` §5). Dropping a file into the watched folder produced a mirrored `.txt`/`.srt` about a second later from real inference, with the source **moved** into the output folder or **deleted** according to the setting, both variants observed; one run sat idle 35 s after boot before the drop, so the initial scan cannot explain the detection. The pill rendered as `Watching watch-in` (basename, not the full path) with a `⏸` button; a real click greyed it to `Watching watch-in — Paused` with `▶`, and a file dropped while paused genuinely sat untouched for 12 s before `▶` picked it up within a second — the pause toggle drives the running service, it does not merely relabel itself. One ✚ beyond the design: the Folder Watch tab carries a disposition caption spelling out the move-vs-delete rule, which Buzz's own UI never disclosed. Preferences → Folder Watch is the parity config half (enable checkbox gating every other field, both folder rows with native Browse, the embedded auto-task options form), §4 of the same document.

### J-5 Live recording
- Parity: device picker, meter, model options, 3 display modes, live TXT/CSV export, upload URL, presentation window, copy button.
- ✚ **Sessions persist**: a live session is a first-class record (`Source::LiveRecording`, approved Q-10) — closing the window loses nothing; it appears in the main table and opens in the viewer.
- ✚ Realtime-fit warning: picking a model whose measured speed < realtime on this machine shows an inline caution before recording starts (uses M1 benchmark data).
- ✚ Upload URL moves from env-only to the live window's advanced section (env still honored).

**Shipped M5** (F-060..F-065, Q-10): every parity item above is live — device combo (system-default preselected, meter-only capture while stopped), Record/⌘R opens the window unconditionally (F-004 unlock), the options group + recording-specific Advanced sheet (initial prompt, silence threshold, mode, transcription step, line separator, export block, upload URL), the three display modes (append below/above/and-correct, the last genuinely wired end to end through the real session), live TXT/CSV export + upload (folder-gated, retrying writer, `HUMM_UPLOAD_URL`-wins precedence), the presentation window (text size/theme/custom colors, fullscreen+Esc, the CSS-subset override file) and copy bar. Both ✚s are live and confirmed working in the M5 Task 14 windowed acceptance run (`docs/gui-manual/M5.md`): a persisted session survived a full process restart and remained openable, and the measured-speed map (`settings.recording.measured_speed`) was written after each session, the input the realtime-fit warning reads. The upload-URL ✚ is unchanged from the design (`HUMM_UPLOAD_URL` env still wins over the advanced sheet's field — `humm_core::live::effective_upload_url`).

### J-6 Viewer — parity on view modes, inline editing (±500 ms nudges, overlap clamping), search, synced audio/video, loop/follow/speed, export menu. ✚ Autosave indicator ("edited · saved") since edits are instant-persisted; ✚ export remembers last folder per transcript. **Shipped M4** (F-080..F-086): both ✚s are live — every committed edit persists synchronously and flashes the "edited · saved" indicator (`ViewerVm::saved_flash`), and the viewer export menu defaults its directory to the per-transcript `last_export_folder` (schema v2 column, `HUMM_SURFACE.md` §6.1) falling back to the input file's own directory. Synced playback uses a single audio master clock (ADR-019); speed control audibly works (linear-interpolation varispeed) where Buzz's was mute (ledger #2).

### J-7 Speaker ID — same two-step flow (identify → name with play-sample), late milestone (ADR-011). Progress states with cancel; results diffable.

**Shipped M8** (F-087, Task 11): the two-step window is live, opened by the viewer's now-real Identify Speakers toolbar button. Step 1's background worker decodes the source audio, ensures the diarization models (determinate download progress), runs diarization, and maps the result onto the transcript's segments; Step 2 shows one prefilled `"Speaker N"` row per detected speaker with a Play-sample button (a random segment of that speaker, played up to `min(10s, segment length)` — ✚ a fix over Buzz's own stop-timer bug, which arms with the segment's *absolute end time* instead, ledger #7), the merge checkbox defaulted on (Buzz parity), and Save (copies the source into a new completed transcript; the source viewer and its table row are untouched). Cancel acks in the UI immediately — button disabled, "Cancelling…" shown — even though the underlying `ort` pipeline's own cancel granularity is coarse (~10s worst case on the reference fixture, ledger #11): the run's own cancel token is the guard that keeps a stale result from ever reaching Step 2, discarding it back to the ready-to-identify state instead. Not yet exercised in a real window (kittest-only this task — the same honest carve-out F-044's own row named for its own never-windowed set): the deferred OS viewport itself and the sample-preview player's actual audio output both need a windowed pass, tracked at F-087's own matrix row.

### J-8 LLM translation — parity flows (per-task live + post-hoc). ✚ The AI-model field is a combo seeded with common ids (still free-text); ✚ prompt default is a neutral "translate to {language}" template, not Buzz's English→Spanish sample.

**Shipped M6** (F-031/F-041/F-063/F-085): all three translation flows are live. Per-task file translation is a ✚ improvement, not parity — Buzz's own "Enable AI translation" checkbox on the file options sheet (`advanced_settings_dialog.py:80-83`, persisted as `file-transcriber/enable-llm-translation`) is dead on the file path, consumed by nothing in Buzz's transcription pipeline (design spec §1, deviations ledger #1); humm wires it for real, translating and storing each produced segment during the file task, batching/prompt-shape/error-semantics still ported byte-faithfully from `translator.py` (options-sheet backend/LLM rows kittest-covered: `llm_block_enable_gates_its_fields`, `run_with_llm_enabled_persists_llm_options_and_never_the_key`). The viewer's post-hoc Translate button is parity (`kittest_viewer.rs::translate_end_to_end_updates_rows_live`), and the live-recording panel + `.translated` export sibling is parity too, now genuinely wired end to end (`kittest_recording.rs::live_translation_end_to_end_panel_export_and_store`, `late_translation_arrivals_after_stop_reach_panel_store_and_export`). Both J-8 ✚s above are live and persist for real on save (ledger #10) — Buzz's own seeded AI-model/prompt defaults stay display-only until touched. The key lives only in the OS keyring (never `settings.toml`, the DB, or logs), entered via Preferences → General with a Test button that validates it live against the configured endpoint (`kittest_prefs.rs::general_tab_key_field_persists_to_secret_store`, `test_button_success_shows_valid_modal`, `test_button_failure_shows_message`).

### J-9 Models — parity management (download/delete/reveal, custom ggml URL & HF repo id). ✚ Each size row shows disk size and a measured speed badge once benchmarked on this machine.

**Shipped M7** (F-044): the speed badge ✚ is live and was watched working in a real window (`docs/gui-manual/M7.md` §2). With measured entries seeded, the tab rendered `Tiny  12.5× realtime  74.1 MB [Delete] [Show file location]` and `Small  2.1× realtime  465.0 MB […]` — and, just as importantly, **no** badge on a size with no measurement, and **no phantom row** for the backend-qualified `openai_api` key an API live session writes (that key is a backend name, never a model size, so it can never match a size row — ledger #L14's guarantee, visible rather than argued). The badge reads `settings.recording.measured_speed`, the same map J-5's live sessions write, so it is genuinely "once benchmarked on this machine" and empty on a fresh install. Also shipped this milestone: the `Lumii` size, gated on the UI locale exactly as Buzz gates it. **Not yet exercised in a window**, and stated plainly because the journey promises them: Delete + its confirm modal, "Show file location", and a live download over the network are all kittest-covered but have never been clicked in a real window in any milestone — F-044's matrix row stays `implemented` for precisely that reason.

### J-10 Recovery — history persists; interrupted tasks marked `Canceled` with one-click Retry (full options). ✚ Optional "resume interrupted tasks on launch" toggle (default off = parity).

### J-11 Customization — Preferences parity + **Privacy tab** ✚: telemetry toggle (ADR-009) with a plain-English list of exactly what one event contains; update-check toggle.

**Shipped M7** (F-100/F-101/F-110): the Preferences window is complete — six tabs, **General · Models · Shortcuts · Folder Watch · Post-processing · Privacy** — and every one of them was opened with a real click and captured in a real 650×532 OS window whose min size the viewport honors, alongside a three-window smoke (main + Preferences + About simultaneously from one process) (`docs/gui-manual/M7.md` §7). The Privacy ✚ is unchanged and intact. Two behaviors worth naming because they are humm's, not Buzz's: **write-through on commit, no OK/Cancel** (ledger #L8 — every change this pass made was already in `settings.toml` before the window closed, and no OK or Cancel control exists on any tab), and **UI language applies live** (ledger #L10 — selecting German re-rendered the Preferences window on the next frame *and* the already-open main window with it, where Buzz showed a "Restart required!" hint). General also gained a font-size control (8–32, applied live) and a default-export-filename template that the folder-watch run demonstrably used. The sixth tab, Post-processing, is where ADR-010's four native features are configured — and it carries **no plugin surface of any kind**: no enable-order list, no install field, no Plugins dialog or menu entry, confirmed by inspecting the shipped UI. One rough edge found and recorded rather than smoothed over: at `ui.font_size = 24` the six-tab strip overflows the 650 pt minimum width (Privacy off-screen, no wrap or scroll) — recoverable by widening the window, and an M8 fix candidate.

## 3. Keyboard shortcuts

Buzz's 20 defaults preserved verbatim (recounted from `buzz/settings/shortcut.py` at M3 implementation time — an earlier analysis pass overcounted at 21; muscle-memory parity) — including Ctrl+S = Clear History (surprising, but rebindable; a confirm dialog guards it). All rebindable in Preferences → Shortcuts with conflict detection ✚ and Reset to Defaults. macOS renders ⌘ equivalents.

**Shipped M7** (F-005): the editor is live and was driven with real clicks and real keystrokes in a windowed run (`docs/gui-manual/M7.md` §3). All 20 rows render their description and current sequence in ⌘ notation; clicking a field shows `Press shortcut…`; pressing `⌘⇧K` binds it, displays `Shift+Cmd+K` the same frame, and writes `OPEN_IMPORT_WINDOW = "Ctrl+Shift+K"` to `settings.toml` (⌘ on screen, Buzz's `Ctrl` wire notation on disk — the documented mapping, observed). **Live re-apply is real and crosses windows**: immediately after that capture, the *main* window's File menu read `Import File…  Shift+Cmd+K` where it had read `Cmd+O` — no restart, and the rebuilt map reaching a different OS viewport than the one that captured the key. The conflict-detection ✚ is **non-blocking by design**: two actions sharing a sequence each get a `⚠ Conflicts with <other>` caption in warning colour and **the assignment stands** — humm warns, it does not reject. Two deliberate adaptations: Esc cancels the row capture (Buzz's Esc closed its whole modal dialog; humm's Preferences window is non-modal write-through with no Esc-close semantics — the shared invariant is that Esc can never itself be bound), and a bare modifier press is ignored via vocabulary exclusion rather than a special case, since the bindable key set never contained the physical modifier keys in the first place.

## 4. States, cancellation, progress (global rules)

- Every long operation: determinate progress where knowable, phase label, cancel affordance, and a terminal state that is *never* silent (toast + table state).
- Cancellation is prompt (< 1 s ack): abort callback for inference, process kill for ffmpeg/yt-dlp, HTTP abort for downloads; partial artifacts cleaned or clearly marked.
- Progress is real (segment-timestamp based for transcription; byte-based for downloads) — cosmetic spinners only where the upstream truly gives nothing.
- Degraded capabilities (no ffmpeg / no yt-dlp / no mic): affected controls disabled with tooltip naming the fix; everything else fully functional (Policy rule 6).

## 5. CLI UX contract

- `--help`: grouped flags, defaults inline, examples section.
- Errors: single-line cause + hint on stderr, exit 1; never a panic trace.
- Progress: per-task bar with phase (TTY); silent with `--quiet`; `--json` = one event object per line (schema documented in `EXTERNAL_SURFACE` successor doc for humm).
- Composability: exit 2 lists failed inputs on stderr; produced file paths printed to stdout on success (one per line) for piping ✚.

## 6. Defaults (minimize required decisions)

- Model default: whisper **small** (quality/speed balance; Buzz's `tiny` default optimizes first-run speed at severe quality cost — changed deliberately, noted as deviation) ✚; first-run banner offers "benchmark my machine and pick for me" ✚.
- Language auto-detect; task transcribe; exports default to SRT+TXT off (parity: none selected until chosen once, then sticky).
- Live mode default `Append below` (Buzz's own stored-index default, `RecordingTranscriberMode` index 0, `recording_transcriber_mode.py`) — this line originally read "Append and correct" during Phase 1 analysis, which contradicted Buzz's real default; corrected here in the M5 docs sweep (spec §9's analysis correction) once M5 shipped `RecordingMode::from_index`'s parity fallback. Buzz persists the last choice regardless, so this default only ever governs a fresh install's first recording — not a ✚ (a stored-index-0 default is parity, not an improvement).
