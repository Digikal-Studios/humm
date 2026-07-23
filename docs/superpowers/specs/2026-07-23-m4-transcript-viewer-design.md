# M4 — Transcript viewer (review, edit, export) — Design

Parity IDs: **F-080** (viewer window), **F-081** (segment editing), **F-082** (search), **F-083** (synced audio/video playback), **F-084** (export from viewer), **F-086** (resize/merge). Unlocks the viewer-gated halves of **F-003** (Return/double-click-to-open) and **F-004** (toolbar Open Transcript). Carried tickets from M3 (`humm-m4-tickets` memory) folded in (§8).

Ground truth re-read for this design (implementers re-read again per task): `transcription_viewer_widget.py` (1575 ln), `transcription_segments_editor_widget.py`, `transcription_view_mode_tool_button.py`, `export_transcription_menu.py`, `widgets/audio_player.py`, `sounddevice_player.py`, `widgets/video_player.py`, `ffmpeg_video_player.py`, `transcription_resizer_widget.py`, `plugins/transcript_resizer/plugin.py`, `main_window.py:333-411`, `db/service/transcription_service.py`, `db/dao/transcription_dao.py:107-160`, `transcriber/file_transcriber.py:185-245`, `transcriber/transcriber.py:212-266`, `settings/shortcut.py:22-36`. Third-party reference sources pinned by Buzz's `uv.lock` and cloned for the port: **stable-ts 2.19.1**, **srt_equalizer 0.1.10** (tag `v0.1.10`), **srt 3.5.3** (scratchpad clones; the plan records exact paths).

---

## 1. Architecture overview

New surfaces, by crate (dependency rule unchanged: binaries → adapters → core):

- **`humm-core`** (pure, testable): `viewer::search` (debounced-search rules as pure functions), `viewer::timestamp` (HH:MM:SS.mmm parse — display reuses `export::to_timestamp`), `viewer::edit` (overlap-clamp + nudge rules), `viewer::text_view` (TEXT/TRANSLATION mode text builders), `resize::` (srt_equalizer port + extend-endings), `regroup::` (stable-ts subset port), boundary additions (§5): `PlaybackPcm`, `AudioDecoder::decode_playback`, `AudioOutput` trait, `FrameStream`/`VideoStreamer` traits, `MediaInfo` video fields.
- **`humm-audio`** (NEW adapter crate, ADR-005 deps cpal 0.18 + rubato 4.0 + rtrb): `CpalOutput` implementing core `AudioOutput`; `Player` (feeder thread, varispeed, loop, position atomics) generic over `dyn AudioOutput` so unit tests run against a mock output with a simulated clock. Only crate besides humm-whisper touching an OS boundary; no `unsafe`.
- **`humm-media`**: `FfmpegDecoder::decode_playback` (f32le **stereo 44.1 kHz** interleaved, whole-file — §5.1); `FfprobeProber` extended to fill `MediaInfo.video: Option<VideoStreamInfo>`; `FfmpegFrameStream` implementing core `FrameStream` (rawvideo RGBA subprocess, §5.3).
- **`humm-store`**: schema **v2** migration + segment-row APIs + `copy_transcription` + `replace_segments` (§4).
- **`humm-gui`**: `viewer::` module tree (`ViewerVm`, `views/viewer.rs` window, `views/viewer_table.rs` editor table, `views/viewer_search.rs`, `views/playback_controls.rs`, `views/export_menu.rs`, `views/resizer.rs`), deferred-viewport management keyed by `TranscriptionId`, app wiring (open paths, `AppEvent` channel), settings persistence.

### 1.1 Viewer windows: deferred viewports

Buzz opens each viewer as a real OS window; many may be open at once (`main_window.py:402-411` re-assigns one field but old windows stay alive). humm uses **egui deferred viewports** (ARCHITECTURE §7) — one per open transcript, keyed `ViewportId::from_hash_of(("viewer", id))`.

- Options considered: (a) immediate viewports (M3's About/Preferences pattern) — simplest, but every child repaint forces a parent pass, and playback repaints at up to 60 Hz per window; with several viewers open the whole window tree repaints continuously. (b) **deferred viewports** — per-window repaint isolation, the architecture-stated choice; cost: per-window state must be `Send + Sync` (`Arc<Mutex<ViewerVm>>`). Decision: **(b)**; the M3 immediate-viewport tradeoff was explicitly justified by "non-animating settings window", which playback windows are not.
- `HummApp.viewers: BTreeMap<TranscriptionId, Arc<Mutex<ViewerVm>>>`. The deferred callback locks the VM, renders, and returns; closes are signalled via `ViewportCommand::Close` / close-requested flag read back by the app each frame (dropping the entry drops the VM, stopping playback via `Drop`).
- **Same-transcript re-open focuses the existing window** instead of opening a duplicate (`ViewportCommand::Focus`). Deviation ✚ (Buzz opens independent duplicate editors whose models go stale against each other); recorded in the ledger (§9).
- Viewer geometry: one shared persisted geometry for all viewers (Buzz stores a single `transcription_viewer/geometry`), min size 800×500, default 1000×800 (`transcription_viewer_widget.py:92-93`, `load_geometry`). Per-media-kind persistence applies to the **splitter** only (§2.1).

### 1.2 Threading & data flow

- **Segments in memory.** `ViewerVm` loads `Vec<SegmentRow>` (id, start, end, text, translation) once at open. All reads (table, search, text views, export, resize inputs) hit memory; every committed edit writes the store synchronously (fast single-row UPDATE) and mutates memory. Safe because nothing else mutates segments while a viewer is open in M4 (translator is M6; resize closes the viewer on completion — §3.6). 100k rows ≈ a few MB.
- **Store sharing.** The store handle is shared with the deferred callbacks (`Arc`; internal connection synchronization as it exists today — the queue runner keeps its own path unchanged). Viewer writes are row-level UPDATEs on `transcription_segment` and never touch `status` (queue-owned, ADR-018 boundary respected).
- **Playback threads.** Per playing viewer: cpal callback thread (pops ring) + feeder thread (reads in-memory PCM at the speed-adjusted rate through rubato, pushes ring, updates position atomic, enforces loop range). UI reads position atomics; while playing the viewer requests repaint every 16 ms (`request_repaint_after`), else on demand (idle ≈ 0 unchanged).
- **Video thread.** Frame-reader thread owns the ffmpeg rawvideo subprocess, fills a bounded frame deque (60 frames, Buzz's `MAX_BUFFER`); UI picks the frame for the current **audio** position each repaint (audio clock is master — ARCHITECTURE §7) and uploads an `egui::TextureHandle`.
- **`AppEvent` channel** (new, GUI-internal): `TranscriptionsUpdated(TranscriptionId)` sent by resize/merge workers → app refreshes/inserts the affected `TasksVm` row from the store, closes the originating viewer + resizer (Buzz: `transcriptions_updated_signal` → viewer `close`, `transcription_viewer_widget.py:1333`).

---

## 2. Feature behavior (Buzz-exact unless flagged)

### 2.1 F-080 viewer window

- **Open paths** (unlocks F-003/F-004): table double-click on a row, Return on selection, toolbar Open Transcript. Gate `Status::Completed | Skipped` **on every path** — per-row for double-click (`main_window.py:396-400`), all-selected for the toolbar (`:350-364`; enablement already shipped in M3's `toolbar_enablement`), and Return mirrors the toolbar rule. Buzz's Return key bypasses the gate entirely (`transcription_tasks_table_widget.py:587-589` emits unconditionally → `open_transcript_viewer` opens any status) — same defect class as the M3 Delete-key gate bypass, deliberately not ported (ledger #13). Multi-selection Return/toolbar opens one window per selected row.
- Title = `basename(file)` (`paths.py::file_path_as_title`); rows with no file (URL rows pre-download) can't be Completed, so no special case.
- **View modes** (`ViewMode::{Timestamps, Text, Translation}`, default Timestamps):
  - *Timestamps*: editor table + media player in a vertical splitter (drag handle), playback-controls bar (when visible), current-segment frame at bottom.
  - *Text*: read-only text box; segments joined `text.trim() + " "`, `"\n\n"` inserted when `start − prev_end ≥ paragraph_split` (env `HUMM_PARAGRAPH_SPLIT_TIME`, default 2000 ms — same constant the TXT writer uses); final `.trim()`. Table/player/controls hidden.
  - *Translation*: read-only text box; translations joined `translation.trim()` with `" "` — **no paragraph split** (Buzz quirk, `transcription_viewer_widget.py:1118-1133` — kept).
  - View toolbar button opens a menu: Text (⌘E) / Translation (⌘L, **visible only when `has_translations`**) / Timestamps (⌘T). `has_translations` = any segment translation non-blank at load; recomputed when a translation cell edit lands (unhides Translation mode/column/export entries — micro-✚, Buzz needs reopen).
  - Mode switch re-runs an active search (`on_view_mode_changed`).
- **Toolbar**: View · Export (§2.4) · Translate (**disabled + tooltip** "arrives with the API backend milestone" — F-085 is M6; M3's Record-button pattern) · Resize (§2.6) · Identify Speakers (**disabled + tooltip**, M8) · Find (checkable toggle).
- **Splitter** persisted **per media kind**: `video_splitter_fraction` (default 800:200) / `audio_splitter_fraction` (default 950:50) (`load_splitter_sizes`). Saved on drag (100 ms debounce parity is unnecessary in immediate mode — saved on drag-release/geometry change via the settings-diff pattern).
- **Persisted viewer state** (§6): loop toggle, follow-audio toggle, playback-controls visibility, find-bar visibility, playback rate (global, `AUDIO_PLAYBACK_RATE` parity), viewer geometry.
- **Playback controls bar** (hidden by default; auto-shows when playback starts in Timestamps mode, `on_audio_playback_state_changed`): Loop Segment checkbox · Follow Audio checkbox · Speed editable combo (`0.5x 0.75x 1x 1.25x 1.5x 2x` presets, free text, clamp 0.1–5.0, `%.2fx` echo) · `−`/`+` 0.05 steps · Scroll to Current button. ⌘⌥P toggles bar visibility.
- **Current-segment frame** (bottom): shows the segment text at the audio position (or the selected segment on click), ~3 lines, scrollable, hidden outside Timestamps mode.
- **Follow audio**: on position change, binary-search the segment containing the position; if follow enabled → select+center-scroll that row (unless looping pins the selection); if disabled → keep the selected row highlighted. **Scroll to Current** (⌘G): jump to the segment containing the current position (linear-scan fallback-to-row-0 semantics simplified to: select+scroll the containing segment, else row 0 — the Buzz double-highlight workaround dance at `:1427-1437` is a Qt scroll quirk, not behavior; noted in ledger).
- **Segment click** (row selection): show its text in the frame; if position outside `[start,end)` → seek to start; if loop enabled → loop range = segment (`on_segment_selected`). Clicking also plays if paused? — **No**: Buzz's `on_transcript_segment_clicked` (auto-play) is connected to a signal that is never emitted in v1.4.5 (dead path); selection alone does not start playback. Only Play/⌘P starts playback.
- **Close**: stops playback, closes a child resizer window, persists state. ⌘⇧P replay-current-segment, ⌘P play/pause wired per shortcut table (§7).
- **Unhappy paths**: file missing/undecodable → player row shows disabled controls + tooltip naming the file problem; table/edit/search/export/resize all still work. Zero segments → empty-state label ("No segments — the input may be silence-only") per UX_SPEC §1.

### 2.2 F-081 segment editing

Editor table (egui_extras virtualized, fixed row height = 4 text lines — Buzz `init_row_height`): columns **Start | End | Text | Translation** (display order; Buzz swaps its odd internal END-before-START order back to this), Start/End width 120, text columns split the remainder (translation column hidden unless `has_translations`). Single-row selection.

- **Timestamps** display `HH:MM:SS.mmm` (`export::to_timestamp` with `'.'`). Edit begins on double-click or Return on the focused cell. The editor is a single-line field accepting only `[0-9:.]`; **`+`/`-` keys nudge the edited value ±500 ms in-editor** (floor 0); parse accepts `HH:MM:SS.mmm` / `MM:SS.mmm` / `SS.mmm` (fraction optional); unparseable input reverts to the last valid value on commit (focus-out).
- **Commit validation** (`TimeStampEditorDelegate::setModelData` + `_adjust_segment_timestamp` — both ported into one core rule set, `viewer::edit`):
  - Start edit: reject (no-op) if `new ≥ end`; clamp `max(0, new)` (nudge path); if `new < prev.end` → **set `prev.end = new`** (neighbor row written too).
  - End edit: reject if `new ≤ start`; if `new > next.start` → **set `next.start = new`**.
- **Nudge shortcuts** (viewer-level, on the *selected* row, reading fresh current values): ⌘← / ⌘→ start ∓/± 500 ms, ⌘⇧← / ⌘⇧→ end ∓/± 500 ms; same validation/clamping; loop range refreshed if looping.
- **Text/Translation cells**: multi-line word-wrap editor; Tab/Enter/**Esc** commit (Buzz's `CustomTextEdit` commits on Esc via focus-out — quirk kept, ledger note); commit on focus loss.
- **Instant persist**: every commit → store UPDATE (both rows when a neighbor was clamped) + in-memory mutation + autosave indicator flash "edited · saved" (UX_SPEC J-6 ✚). Store write failure → toast + banner path consistent with ADR-018 (edit stays in memory, flagged unsaved).
- Timestamp edits while looping update the loop range live (`on_timestamp_being_edited` → live range follow while the editor is open).

### 2.3 F-082 search

Find bar under the toolbar (visibility persisted): input, ↑ ↓ buttons (disabled without results), Clear, "N of M matches" label.

- ⌘F toggles bar + focuses + selects-all input; Esc (in input) hides bar and clears; Find toolbar button mirrors state.
- **Debounce**: search runs 300 ms after the last edit, only when `query.trim().len() ≥ 2` **at fire time** (Buzz starts the timer only at ≥2 chars but never cancels a pending timer when the query shrinks to 1 — a race we make deterministic; ledger note). Empty query → results cleared, bar stays.
- Case-insensitive substring (char-wise case-insensitive matcher over the original text so highlight offsets are always valid; Unicode multi-char folds diverge from Python `str.lower()` only in exotic cases — ledger note).
- **Timestamps mode**: pass 1 over segment `text` (row matches), then, if `has_translations`, pass 2 over `translation` — appended after all text matches, shared **100-result cap** (a row can appear twice). Match = select+center-scroll row.
- **Text/Translation mode**: substring positions in the joined display text, cap 100; match = highlight range + scroll to it (layouter-highlight + galley-offset scroll).
- Label: `1 of N matches` on fresh search; `K of N matches` while navigating; `… of 100+ matches` at the cap; `No matches found` otherwise. **Next/prev wrap modulo** (⌘Return / ⇧Return, also from the input; Return in input = next). Clear: input/results/highlight cleared, bar stays, input focused.
- Search over segments is pure core code (`viewer::search`) — unit-tested per the matrix method (`unit + gui-manual`).

### 2.4 F-084 export from viewer

Export toolbar button opens a menu of `{TXT, SRT, VTT} × {Text, Translation}` — 6 entries, `"SRT - Text"` label shape, Translation entries **visible only when `has_translations`**. Selecting one:

1. Default filename from the shared template engine (`export::output_file_path` — same tokens + `%d-%b-%Y %H-%M-%S` datetime), default directory = **per-transcript last export folder** (✚ UX J-6, §4 schema v2) falling back to the input file's directory.
2. Native save dialog (extend the `FileDialogs` seam with `save_file(...)` so kittest never drives rfd) with `*.{ext}` filter; cancel = no-op.
3. Write via the **shared M1 writers** (`export::write_format` with `TextKey::{Text, Translation}` from in-memory segments — exactly the code path the queue's export uses; acceptance pins snapshot equality).
4. Persist the chosen folder for this transcript; success toast with the written path.

### 2.5 F-083 synced playback

**Audio** (§5.1-5.2 for the engine): scrubber (drag = live seek; releasing outside a loop range ±2000 ms clears the range — `on_slider_moved`), play/pause button, `position / duration` time label (`H:MM:SS`), speed (§2.1 controls). Loop range: when enabled + segment selected, feeder wraps `end → start` **sample-exactly** (Buzz's poll loops 50 ms early — mechanics improvement, outcome parity, ledger). `set_range` outside-position jump-to-start parity. Playback rate persists globally and applies live.

**Video** (`is_video` by extension set `{mp4 mov mkv avi m4v webm ogm wmv}` — `file_transcriber.py:242`): video pane above the controls inside the splitter (per-kind splitter default 800:200). Frames: core `FrameStream` impl spawns `ffmpeg -ss <pos> -i <file> -f rawvideo -pix_fmt rgba -s WxH -r <fps> -an -`; reader thread buffers ≤60 `(ts, frame)`; UI requests the frame for the **audio clock** position with Buzz's drain-stale/one-frame-tolerance rule; stale-drain keeps `last_frame` visible on gaps. Seek = kill + respawn at position (Buzz parity). fps/width/height from the extended prober; fps floor 1.0, default 25 when probing fails. A/V sync is structural (single audio master clock; no Buzz-style 200 ms drift-correction polling needed — that corrected *two independent clocks*, which humm doesn't have).

**Acceptance instrumentation**: env-gated drift log (`HUMM_SMOKE_AV_DRIFT_LOG`) records `(audio_pos, frame_ts)` pairs each frame; the 10-min ±50 ms p95 check runs windowed on the unsandboxed machine (M3 smoke-hook pattern) over a generated `testsrc2`+`sine` fixture.

### 2.6 F-086 resize / merge

Separate window (deferred viewport, one per source transcript) opened by the Resize toolbar button; title = file basename; sections per `transcription_resizer_widget.py`:

- **Settings**: "Create new transcript" checkbox, persisted (`resizer_create_new_transcript`, default **true**).
- **Extend end time** (always enabled): "Extend endings by up to (seconds)" text field default `0.2` (unparseable → 0.2); `new_end = min(end + amount_ms, next.start)`, last segment unclamped; then save (below).
- **Resize** (enabled iff **word timestamps off**; tooltip otherwise): "Desired subtitle length" spin 1–100 default 42 + Resize button → srt_equalizer port (§5.4): per input segment `split_subtitle(method="punctuation")` in **milliseconds** with proportional float timing, chained `start_from_index`; output drops zero-duration rows (`round(start) == round(end)`, Python banker's rounding); save.
- **Merge** (enabled iff **word timestamps on**): Merge-by-gap checkbox + field `0.2`; Split-by-punctuation checkbox + field `.* /./. /。/?/? /？/!/! /！/,/, `; Split-by-max-length checkbox + field `42`. Regroup string built exactly as Buzz (`mg=<gap>` [`++<len>+1` iff max-length also on] `_sp=<punct>` `_sl=<len>`), overridable by **`HUMM_MERGE_REGROUP_RULE`**; runs on a worker thread; window hides while running; on completion → save → `TranscriptionsUpdated` → resizer *and* originating viewer close.
- Word-timestamps gate source: `load_task_spec(...).options.word_timestamps`, falling back to `spec_from_columns` (imported Buzz rows carry `word_level_timings`).
- **Save semantics** (Buzz DAO parity): create-new → `copy_transcription` (row copy, new id, `time_queued = now`, status Queued — for merge, progress 0.0 is written first so the row is visible while the worker runs) then `mark_replaced_completed(new_id, segments)` (status Completed + insert segments, **translations dropped** — parity); in-place → `replace_segments(id, segments)` (delete + reinsert, translations dropped, id sequence fresh).
- Merge pipeline (§5.5 for the port): DB segments → "words" (`text + sep`, sep `""` for languages `{zh ja th lo km my}` else `" "`; language from the transcription row) → sentence groups via `.*[.!?。！？]` regex-match on each segment text → stable-ts `WhisperResult` → `regroup(string)` → segments → ms. **Unit quirk preserved**: Buzz feeds `ms / 100` as "seconds" (1 fake-second = 100 ms real), so default `mg=0.2` merges gaps ≤ 20 ms real and `sl` splits are character-based (unaffected); output `int(x * 100)` truncation back to ms. Byte-exact port target.

---

## 3. F-003/F-004 unlock wiring

`TableAction::OpenSelected` (currently the app.rs no-op) → for each selected id with `Status::Completed | Skipped`: open/focus viewer. Double-click on a table row emits `OpenSelected` for that row (gate per-row, silent no-op otherwise). Toolbar Open Transcript button: enablement already correct (M3); action now emits `OpenSelected`. The `toolbar-open-transcript-tooltip` i18n string ("arrives in the next milestone") is retired; disabled-state tooltip becomes the selection-rule explanation.

---

## 4. humm-store: schema v2 + segment APIs

**Migration v1→v2** (gated `PRAGMA user_version`): `ALTER TABLE transcription ADD COLUMN last_export_folder TEXT;`. Forward-only; v2 code opening a v1 DB migrates; `user_version > 2` → refuse with actionable error (adds the M2-minor guard test: unknown-newer-version refusal).

New APIs (all unit-tested, Buzz-DAO-shaped):

```rust
pub struct SegmentRow { pub id: i64, pub start: Millis, pub end: Millis, pub text: String, pub translation: String }
fn get_segment_rows(&self, id: TranscriptionId) -> Result<Vec<SegmentRow>, StoreError>;   // ORDER BY id ASC
fn update_segment_times(&self, seg: i64, start: Millis, end: Millis) -> Result<(), StoreError>;
fn update_segment_text(&self, seg: i64, text: &str) -> Result<(), StoreError>;
fn update_segment_translation(&self, seg: i64, translation: &str) -> Result<(), StoreError>;
fn replace_segments(&self, id: TranscriptionId, segs: &[Segment]) -> Result<(), StoreError>;  // txn: DELETE + INSERT
fn copy_transcription(&self, id: TranscriptionId) -> Result<TranscriptionId, StoreError>;      // row copy: new id, time_queued=now, status=queued, progress untouched-then-overwritten by caller
fn mark_replaced_completed(&self, id: TranscriptionId, segs: &[Segment]) -> Result<(), StoreError>; // status=completed(+time_ended), insert segs
fn update_progress_direct(&self, id: TranscriptionId, fraction: f64) -> Result<(), StoreError>;     // resize-merge visible row; NOT a queue path
fn set_last_export_folder(&self, id: TranscriptionId, dir: &Path) -> Result<(), StoreError>;
fn last_export_folder(&self, id: TranscriptionId) -> Result<Option<PathBuf>, StoreError>;
```

Status-write boundary note: `copy_transcription`/`mark_replaced_completed`/`update_progress_direct` write `status`/`progress` for **resizer-created rows only**, which never enter the queue; ADR-018's "queue owns status transitions" invariant is scoped to queue-executed tasks (doc note added to the queue module).

---

## 5. New boundaries & engines

### 5.1 Playback decode (humm-core boundary + humm-media impl)

```rust
pub struct PlaybackPcm { samples: Arc<[f32]>, /* interleaved */ pub channels: u16, pub sample_rate: u32 }
pub trait AudioDecoder { …existing decode…; fn decode_playback(&self, path: &Path, cancel: &CancelToken) -> Result<PlaybackPcm, MediaError>; }
```

`FfmpegDecoder::decode_playback`: `ffmpeg -nostdin -i <path> -f f32le -ac 2 -ar 44100 -loglevel error -` (Buzz decodes to 44.1 kHz stereo WAV — `sounddevice_player.py:31`), same streaming-read/kill-on-cancel pattern as `decode`.

**Whole-file in-memory** (Buzz-parity approach) rather than a seekable `PcmStream`: sample-exact seek/loop with zero latency and a trivial position model; memory ≈ 1.27 GB/hour — identical to Buzz's numpy buffer. A streaming decoder would add a seek-respawn state machine for no user-visible win at typical media lengths; revisit if long-file use appears (ARCHITECTURE §2's `open_stream` sketch is amended to record this — §10). Decode runs on a background thread at viewer open (player shows "loading…" until ready; failure → degraded player, §2.1).

### 5.2 `humm-audio`: `AudioOutput` + `Player`

```rust
// humm-core
pub trait AudioOutput: Send {
    /// Open an output stream at (sample_rate, channels); `fill` is called on the RT thread.
    fn open(&self, spec: StreamSpec, fill: Box<dyn FnMut(&mut [f32]) + Send>) -> Result<Box<dyn OutputStream>, AudioError>;
    // StreamSpec { sample_rate: u32, channels: u16 } — new type; NOT domain::OutputSpec (export outputs)
}
pub trait OutputStream: Send { fn play(&mut self); fn pause(&mut self); }
```

`humm-audio::CpalOutput` implements it (default device, f32, device-rate negotiation with rubato resample-to-device inside the adapter per ADR-005 if 44.1 kHz unsupported). `humm-audio::Player` owns: `PlaybackPcm`, position (atomic source-frame counter — **the A/V master clock**), `loop_range: Option<(Millis, Millis)>`, `speed: f32` (0.1–5.0), play state. Feeder pulls source frames at `speed` ratio through rubato (varispeed: pitch shifts with rate, the standard scrubbing tradeoff; Buzz's sounddevice path had **no working speed at all** — its rate call landed on a muted QMediaPlayer — so parity is to the *feature* (Qt fallback path) not the defect; ledger). Loop enforcement in the feeder: crossing `end` wraps to `start` sample-exactly. Public surface: `play/pause/toggle/seek(ms)/set_speed/set_loop/clear_loop/position_ms/duration_ms/is_playing`, all callable from the UI thread; `Drop` stops the stream.

Unit tests drive `Player` against a `MockOutput` whose `fill` is pulled by the test at simulated rates: position advance, seek, loop wrap at exact boundary, speed-ratio frame consumption, pause/resume, end-of-media stop. cpal itself gets an `--ignored` contract test (device open + 100 ms sine) since the sandbox may block CoreAudio (run where the M3 windowed smoke ran).

### 5.3 Video boundary (humm-core trait + humm-media impl)

```rust
pub struct VideoStreamInfo { pub width: u32, pub height: u32, pub fps: f64 }   // MediaInfo.video: Option<VideoStreamInfo>
pub struct VideoFrame { pub ts: Millis, pub rgba: Vec<u8>, pub width: u32, pub height: u32 }
pub trait VideoStreamer: Send + Sync {
    fn open(&self, path: &Path, start: Millis, info: &VideoStreamInfo) -> Result<Box<dyn FrameStream>, MediaError>;
}
pub trait FrameStream: Send {
    /// Best frame for `position` per Buzz's drain-stale rule; None = keep last.
    fn frame_for(&mut self, position: Millis) -> Option<VideoFrame>;
    fn seek(&mut self, position: Millis) -> Result<(), MediaError>;   // kill + respawn
}
```

`FfprobeProber` gains stream parsing for `width/height/r_frame_rate` (`"n/d"` → f64, fallback 25.0; duration fallback to the video stream's duration when format-level is absent — `ffmpeg_video_player.py:49-66`). Pixel format **rgba** (egui-native upload; Buzz used rgb24 + a QImage conversion — same frames, one less conversion, ADR-003's own precedent). Frame timestamps synthesized `start + n·(1000/fps)` (Buzz parity). Contract test (`--ignored`, needs ffmpeg): generated 2 s `testsrc2` fixture → open, read ~50 frames, assert dimensions/monotonic ts/seek restart.

### 5.4 `resize::` port (srt_equalizer 0.1.10 + extend)

Pure functions in humm-core over `&[Segment]` (ms f64 internally):

- `split_subtitle` "punctuation" method: regex-split on `([.,!?])` keeping delimiters; strip; skip empties; chunk > target → greedy word-split (with Buzz/srt_equalizer's exact `len(current)+len(word)+1 > target` accounting incl. trailing-space quirks); attach bare punctuation to the previous chunk; coalesce adjacent chunks while `≤ target`; ≤-target inputs pass through unchanged. Proportional timing: `chunk_dur = chunk_len / total_len · (end − start)` accumulated in f64, last chunk pinned to original end.
- Output mapping: `round_half_even(start/end)` (Python `round`) → drop rows where they collapse equal.
- `extend_endings(segments, amount_ms)` per §2.6.
- Verification (`unit + diff`): unit tests port the library's own doctests/cases plus edge cases; **differential fixtures generated by running the real Python libs locally** — srt_equalizer + srt are pure Python (stdlib-only), so a harness script runs Buzz's exact call pattern (`srt.Subtitle` with raw-ms ints, `method="punctuation"`, chained indices) over fixture transcripts and pins expected outputs the Rust tests assert against.

### 5.5 `regroup::` port (stable-ts 2.19.1 subset)

Data model: `RWord { text: String /* includes trailing separator */, start: f64, end: f64 /* fake-seconds = ms/100 */ }`, `RSegment { words: Vec<RWord> }`; built from DB segments per §2.6. Port scope = everything reachable from Buzz's UI + parser:

- **Parser** (`parse_regroup_algo` grammar): ops split `_`, args `+` positional, `/` list, `*` tuple-in-list, `''`→None (skip → default), int/float coercion (`.`-presence rule). Supported ops: `mg` (merge_by_gap: min_gap, max_words, max_chars, is_sum_max, lock, newline), `sp` (split_by_punctuation: punctuation list of str|pair, lock, newline, min_words/min_chars/min_dur, ignore_special_periods), `sl` (split_by_length: max_chars, max_words, even_split, force_len, lock, include_lock, newline, ignore_special_periods). Any other op key → actionable error naming the op and the supported set (`HUMM_MERGE_REGROUP_RULE` reachable; Buzz would accept ~20 ops — scope cut recorded in ledger; the UI never emits others).
- **Semantics ported from source** (implementer traces these exact functions in the pinned clone): `WhisperResult.__init__` normalization (Segment from words; `remove_no_word_segments`; `force_order=False`, `check_sorted=False`), `get_gap_indices`/`get_punctuation_indices` (str = word-ends-with; `(a,b)` pair = word-ends-a + next-starts-b)/`get_length_indices` (even-split math), `_merge_segments` (max_words/max_chars/is_sum_max gating, lock rules), `_split_segments` (lock + newline + special-periods handling as reached with defaults), segment `start/end` derived from first/last word.
- Buzz's default UI string exercised end-to-end: `mg=0.2++42+1_sp=.* /./. /。/?/? /？/!/! /！/,/, _sl=42`.
- Verification (`unit + diff`): unit tests per op; **differential fixtures** via a stub-import harness running the *genuine* stable-ts result.py locally (torch/numpy/tqdm stubbed — the regroup path is pure Python; spike confirmed the import graph is the only obstacle). If stubbing proves unstable, fallback = hand-traced expected outputs from the pinned source with the harness gap reported in the milestone report (F-045-style transparency).

---

## 6. Settings additions (`humm-core::settings`)

```toml
[viewer]                                   # all defaults = Buzz defaults
segment_looping_enabled = false
playback_controls_visible = false
find_widget_visible = false
follow_audio_enabled = false
playback_rate = 1.0                        # clamp 0.1..=5.0 on load
audio_splitter_fraction = 0.95             # per-media-kind (950:50)
video_splitter_fraction = 0.8              # (800:200)
resizer_create_new_transcript = true
# viewer geometry: Option<ViewerGeometry { x: Option<f32>, y: Option<f32>, w: f32, h: f32 }>,
# absent until first close; shared across viewer windows, last-close wins; default 1000×800 (min 800×500)
```

Saved via the existing diff-on-change pattern (`persist_layout_if_changed` generalized). Per-transcript last export folder lives in the store (§4), not settings.

## 7. Shortcuts & i18n

All 14 dormant viewer shortcuts (already defined M3, Buzz-verbatim) become live **inside the focused viewer viewport only**: ⌘E/⌘L/⌘T view modes, ⌘F search, ⌘Return/⇧Return next/prev, ⌘G scroll-to-current, ⌘P play/pause, ⌘⇧P replay segment, ⌘⌥P toggle controls, ⌘←/⌘→/⌘⇧←/⌘⇧→ nudges. Consumption via `ShortcutMap` + `consume_shortcut` in the viewer callback (M3 pattern). New i18n keys (viewer-*, resizer-*, export-*) added to `humm.ftl` + `all_keys_used()` in the same commits; user-facing strings byte-match Buzz where the string is parity-relevant (e.g. "No matches found", " of ", " matches", resizer labels).

## 8. Carried tickets folded in

1. **Per-artifact download mutex** (core `models/download.rs`): process-wide registry (`Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>`); `ensure_artifact` holds the per-final-path lock across the whole partial→verify→promote sequence, so prefs-tab + in-task ensure of the same artifact serialize (second comer finds the sentinel). Unit test: two threads ensure the same artifact against the test server; exactly one download, both succeed.
2. **`TasksVm::mutate` resort-skip**: capture the active sort key's projection before/after `f`; skip `resort()` when unchanged (progress/phase events under default `DateAdded` sort stop resorting). Unit tests: mutation not touching the key skips; touching resorts.
3. **Models-tab custom-URL field**: persist on `lost_focus` (not per keystroke).
4. **argv import**: filter leading-`-` args before treating as paths (main.rs).
5. **kittest sentinel paths** (`/unused-settings-path`) → tempdirs.
6. **Enqueue-failure mark_failed comment** (queue.rs, no-op row update — comment only).
7. **F-045 real differential** stays environment-blocked → restated in the M4 milestone report.
8. Not in M4 (recorded): `QueueHandle::finish()` timeout ADR (M6, next engine), logging.rs dedup (third binary), column drag-reorder egui gap (matrix note stands), gui-manual pointer-interactive human spot-checks.

## 9. Deviations ledger (→ matrix notes / ADRs)

| # | Deviation | Kind |
|---|---|---|
| 1 | Same-transcript open focuses the existing viewer instead of duplicating | ✚ improvement (stale-model hazard in Buzz) |
| 2 | Speed control audibly works (rubato varispeed); Buzz's sounddevice path silently dropped rate | defect not ported (M3 Delete-gate precedent) |
| 3 | Loop wraps sample-exactly at `end`; Buzz loops 50 ms early off a 16 ms poll | mechanics; outcome parity |
| 4 | A/V sync via single audio master clock; Buzz drift-corrected two clocks every 100–200 ms | mechanics (ARCH §7) |
| 5 | Search debounce deterministic (≥2-char check at fire time); Buzz has a 1-char race window | defect not ported |
| 6 | Esc in a text-cell editor **commits** (Buzz behavior) — kept despite being surprising | parity quirk, documented |
| 7 | Translation view/column/export unhide immediately after a manual translation edit (Buzz needs reopen) | ✚ micro |
| 8 | Regroup parser supports the UI-reachable op set (`mg`/`sp`/`sl`) + actionable error for the rest of stable-ts's ~20 ops via `HUMM_MERGE_REGROUP_RULE` | scope cut (ADR) |
| 9 | Playback decode is whole-file in-memory (Buzz-equal memory), not streaming | ADR (ARCH §2 amendment) |
| 10 | Video frames requested as rgba (Buzz rgb24 + convert) | mechanics |
| 11 | Buzz's Scroll-to-Current double-highlight Qt workaround not reproduced | mechanics |
| 12 | Resizer-created rows get status writes outside the queue (Buzz DAO parity); ADR-018 scope note | doc note |
| 13 | Return-to-open gated to Completed/Skipped like every other path; Buzz's Return bypasses its own gate (`transcription_tasks_table_widget.py:587-589`) | defect not ported (M3 Delete-key precedent) |

New ADRs: **ADR-019** playback architecture (in-memory decode, cpal+rubato varispeed, audio-master A/V sync) · **ADR-020** resize/merge native port (pinned reference versions, unit quirk preservation, parser scope cut, differential-fixture strategy).

## 10. Doc updates in-milestone

ARCHITECTURE §2 (`open_stream` sketch → `decode_playback` + `VideoStreamer` as-built), §3 playback row (feeder/rubato specifics), §7 (viewer = deferred viewports as-built); UX_SPEC J-6 (autosave indicator shipped, per-transcript export folder shipped); HUMM_SURFACE (new env: `HUMM_MERGE_REGROUP_RULE`, `HUMM_PARAGRAPH_SPLIT_TIME` shared with viewer TEXT mode, smoke hook `HUMM_SMOKE_AV_DRIFT_LOG`; settings `[viewer]`; schema v2). FEATURE_INVENTORY citation corrections if implementation finds drift (living-docs rule).

## 11. Verification & acceptance

Per-feature methods match the matrix rows:

- **F-080** gui-manual: kittest where drivable (open-from-table via seam, mode switches, translation-gated menu items, empty-state) + windowed manual v2 checklist.
- **F-081** unit (core edit rules: clamp matrix, nudge floor/reject cases, parse round-trip) + gui-manual (kittest: edit-commit persists through a real store; restart survival via store reopen).
- **F-082** unit (search rules: cap, ordering, two-pass translation append, wrap navigation, debounce gate) + gui-manual (kittest: bar toggle, label text).
- **F-083** gui-manual + engine unit tests (§5.2) + `--ignored` contract tests (cpal device, ffmpeg video stream) + windowed A/V drift run (±50 ms p95 / 10 min).
- **F-084** snapshot (viewer export byte-equals `write_format` output for all 6 combos over a fixture — shared-code assertion) + gui-manual.
- **F-086** unit (both ports) + **diff** (Python reference harnesses §5.4/§5.5 on fixture transcripts incl. a Buzz-imported one).
- **Acceptance gates** (before merge): edits survive restart (kittest store-reopen + manual); A/V ±50 ms/10 min; exports match M1 writers (snapshot); resize diff green; **viewer table ≥60 fps at 100k segments** (`viewer_scroll` criterion bench, `table_scroll` pattern); **no transcription-path bench regression** (`e2e` bench compare); gui-manual v2 executed and recorded in `docs/gui-manual/M4.md`.
- PARITY_MATRIX updated with every merged task group; milestone report at close (matrix status, bench numbers, deviations w/ ADR refs, analysis corrections, F-045 blocker status).
