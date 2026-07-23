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

### J-5 Live recording
- Parity: device picker, meter, model options, 3 display modes, live TXT/CSV export, upload URL, presentation window, copy button.
- ✚ **Sessions persist**: a live session is a first-class record (`Source::LiveRecording`, approved Q-10) — closing the window loses nothing; it appears in the main table and opens in the viewer.
- ✚ Realtime-fit warning: picking a model whose measured speed < realtime on this machine shows an inline caution before recording starts (uses M1 benchmark data).
- ✚ Upload URL moves from env-only to the live window's advanced section (env still honored).

### J-6 Viewer — parity on view modes, inline editing (±500 ms nudges, overlap clamping), search, synced audio/video, loop/follow/speed, export menu. ✚ Autosave indicator ("edited · saved") since edits are instant-persisted; ✚ export remembers last folder per transcript. **Shipped M4** (F-080..F-086): both ✚s are live — every committed edit persists synchronously and flashes the "edited · saved" indicator (`ViewerVm::saved_flash`), and the viewer export menu defaults its directory to the per-transcript `last_export_folder` (schema v2 column, `HUMM_SURFACE.md` §6.1) falling back to the input file's own directory. Synced playback uses a single audio master clock (ADR-019); speed control audibly works (linear-interpolation varispeed) where Buzz's was mute (ledger #2).

### J-7 Speaker ID — same two-step flow (identify → name with play-sample), late milestone (ADR-011). Progress states with cancel; results diffable.

### J-8 LLM translation — parity flows (per-task live + post-hoc). ✚ The AI-model field is a combo seeded with common ids (still free-text); ✚ prompt default is a neutral "translate to {language}" template, not Buzz's English→Spanish sample.

### J-9 Models — parity management (download/delete/reveal, custom ggml URL & HF repo id). ✚ Each size row shows disk size and a measured speed badge once benchmarked on this machine.

### J-10 Recovery — history persists; interrupted tasks marked `Canceled` with one-click Retry (full options). ✚ Optional "resume interrupted tasks on launch" toggle (default off = parity).

### J-11 Customization — Preferences parity + **Privacy tab** ✚: telemetry toggle (ADR-009) with a plain-English list of exactly what one event contains; update-check toggle.

## 3. Keyboard shortcuts

Buzz's 20 defaults preserved verbatim (recounted from `buzz/settings/shortcut.py` at M3 implementation time — an earlier analysis pass overcounted at 21; muscle-memory parity) — including Ctrl+S = Clear History (surprising, but rebindable; a confirm dialog guards it). All rebindable in Preferences → Shortcuts with conflict detection ✚ and Reset to Defaults. macOS renders ⌘ equivalents.

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
- Live mode default `Append and correct` (Buzz's most useful mode) — parity-compatible since Buzz also persists last choice.
