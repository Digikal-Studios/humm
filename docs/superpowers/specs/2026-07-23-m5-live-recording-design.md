# M5 — Live recording — Design

Parity IDs: **F-060** (recording window), **F-061** (realtime transcription loop), **F-062** (display modes incl. append-and-correct merge), **F-064** (live export TXT/CSV + upload URL), **F-065** (presentation window & clipboard copy), plus the **Q-10 ✚** live-session persistence and the **F-004** Record-button/Ctrl+R enablement carried from M3. **F-063 (live translation) is M6** — its affordance renders disabled with a tooltip; nothing else of it is built.

Ground truth re-read for this design (implementers re-read again per task): `buzz/transcriber/recording_transcriber.py` (561 ln), `buzz/widgets/recording_transcriber_widget.py` (1257 ln), `buzz/widgets/audio_devices_combo_box.py`, `buzz/widgets/audio_meter_widget.py`, `buzz/widgets/record_button.py`, `buzz/recording.py` (RecordingAmplitudeListener), `buzz/settings/recording_transcriber_mode.py`, `buzz/widgets/presentation_window.py`, `buzz/widgets/transcriber/advanced_settings_dialog.py:123-255` (recording section), `buzz/widgets/transcriber/transcription_options_group_box.py` (with `show_recording_settings=True`), `buzz/settings/settings.py:170-174` (default export template).

Governing ADRs: **ADR-005** (cpal + rtrb + rubato capture, device-default-rate fallback), **ADR-008** (live streaming **in-process** via whisper-rs — no `whisper-server` subprocess; hallucination guards become native params), **ADR-019** (`humm-audio` is the existing cpal adapter crate this milestone extends).

---

## 1. Architecture overview

New surfaces, by crate (dependency rule unchanged: binaries → adapters → core):

- **`humm-core`** (pure, testable — new `live::` module tree):
  - `live::chunker` — the sample-accumulation + silence-cut + skip/drop state machine ported from `recording_transcriber.py` (§2.2). Pure: samples in, `ChunkDecision`s out; no threads, no audio APIs.
  - `live::merge` — `find_common_part`, `merge_text_no_overlap`, the append-and-correct merge pipeline, `filter_text`, the sentence-space regex (§2.3).
  - `live::export` — live TXT/CSV writers + retry policy + filename templating + the upload-URL client (§2.4).
  - `live::session` — the recording-session orchestration loop (generic over the boundary traits so it unit-tests with fakes; §3).
  - `boundary` additions: `AudioCapture` trait + `AudioDevice`/`CaptureHandle` types (§5).
  - `settings` additions: `RecordingSettings` + `PresentationSettings` (§6).
  - `store`-facing repository additions live in humm-store (§4) behind existing trait/API conventions.
- **`humm-audio`** (existing adapter crate, ADR-019/ADR-005): `CpalCapture` implementing core `AudioCapture` — cpal input stream, rtrb SPSC ring out of the RT callback, downmix-to-mono + rubato resample-to-16k inside the adapter, device enumeration with default preselection. `MockCapture` (feeds scripted samples through the same ring at a scripted pace) ships here as a public test-support type for CI and GUI tests.
- **`humm-whisper`**: live-mode parameter set on the existing engine (ADR-008 native hallucination guards; §2.2.4).
- **`humm-store`**: live-session rows (`Source::LiveRecording`): insert-at-start / stream-segments / complete-at-stop APIs + the stale-flip special case (§4).
- **`humm-gui`**: `recording::` module (`RecordingVm`, `views/recording.rs` window, `views/audio_meter.rs` painted widget, `views/recording_advanced.rs` sheet, `views/presentation.rs` window), F-004 Record/Ctrl+R enablement, main-table row lifecycle wiring.

### 1.1 Threading model (per ARCHITECTURE §3)

```
cpal RT callback ──rtrb ring──▶ chunker/session thread ──mpsc events──▶ GUI (repaint on event)
     (adapter)                   (owns engine, runs loop)
     │ RMS per callback ──▶ AtomicU32 snapshot ──▶ meter widget (read per repaint)
```

- The RT callback does **no allocation and no locking**: downmix + resample state is preallocated in the adapter; samples push into the rtrb producer (overflow = samples dropped, counted); per-callback RMS stores into an `AtomicU32` (f32 bits).
- One **session thread** per active recording: drains the ring into the Buzz-equivalent accumulation queue, runs the chunk loop (§2.2), calls the engine synchronously, sends `LiveEvent`s over an mpsc channel the GUI drains each frame (same shape as M4's `AppEvent` pattern). `ctx.request_repaint()` on event send.
- **Meter-only mode** (window open, not recording): the same capture adapter runs with the session thread in a drain-and-discard loop that only feeds the RMS atomics (Buzz's `RecordingAmplitudeListener` equivalent, incl. its 1 s accumulation-average). Starting a real recording stops the meter-only capture first (Buzz: two streams on one device are avoided, `recording_transcriber_widget.py:696-698`).

## 2. Feature behavior (Buzz-exact unless flagged)

### 2.1 F-060 recording window

Single instance, opened by toolbar Record or Ctrl+R (F-004 unlock — the M3 button loses its "arrives in a later milestone" tooltip and enables unconditionally, Buzz parity). Deferred viewport keyed `ViewportId::from_hash_of(("recording",))`, default size 700×600 (Buzz `resize(700,600)`), title "Live Recording". Re-invoking focuses the live window (M4 viewer precedent, deviation ledger #1).

Layout top-to-bottom (Buzz `_build_layout`):
1. **Options group** — model size combo (reuses the options-sheet size list; humm has one backend per ADR-008 so Buzz's model-*type* combo has no equivalent — deviation already established in F-023), task combo, language combo, Advanced… button. First-use default model resolution matches the file options sheet (UX_SPEC §6); thereafter sticky (§6). On Apple Silicon the size row carries Buzz's info tooltip ("First time use of a model may take up to several minutest to load." — byte-exact incl. Buzz's typo).
2. **Microphone row** — "Microphone:" label + device combo listing input devices, **system default preselected**; fallback = first input device (`audio_devices_combo_box.py:46-54`). Device change while stopped restarts the meter-only capture on the new device.
3. **Meter + Record button row** — custom painted meter (§2.5) + Record/Stop toggle button.
4. **Presentation options bar** (§2.6) — hidden until recording starts.
5. **Transcription panel** — read-only text area, placeholder "Click Record to begin..." (cleared once a model is loaded, Buzz `reset_recording_controls`).
6. **Translation panel** — **not built** (F-063 is M6). The advanced sheet's "Enable AI translation" checkbox renders **disabled with tooltip** "Live translation arrives with the API backend milestone" (M3 Record-button pattern).
7. **Copy actions bar** (§2.7) — hidden until a recording stops.
8. **✚ Realtime-fit warning** — inline caution label above the Record button when the selected model's measured live speed on this machine is `< 1.0× realtime` (§2.8). No measurement yet ⇒ no warning.

**Record click (stopped→recording):** stop + detach meter-only capture, reset meter, clear both panels, disable options group + device combo + mic label, show presentation bar, hide copy bar, then start the session (§3). Model absent ⇒ download first with progress + cancel shown **inline in the window** (non-modal — humm's M3 J-1 ✚ pattern; Buzz used a modal `ModelDownloadProgressDialog`); cancel or failure returns the window to stopped state with the Buzz error-dialog text ("An error occurred while starting a new recording:" + cause + "Please check your audio devices or check the application logs for more information.").

**Stop click (recording→stopped):** signal cooperative stop; Record button disabled until the session thread acknowledges (Buzz disables until `finished`); then stopped state: options re-enabled, presentation bar hidden, copy bar shown, meter-only capture restarts, session persisted (§4).

**Window close:** if recording, defer the close until the session thread finishes (Buzz `closeEvent` two-phase), then persist sticky prefs (§6) and close. Presentation window closes with it.

### 2.2 F-061 realtime transcription loop (port of `recording_transcriber.py`)

All constants Buzz-verbatim:

- `SAMPLE_RATE` = 16 000. Capture requests 16 k; on device refusal the adapter captures at the device default rate and **resamples to 16 k inside the adapter** (ADR-005) — so unlike Buzz, the chunker always sees 16 k and `n_batch_samples` never varies by device. (Buzz ran the whole loop at the device rate and fed device-rate audio to the model — its WAV-header path made the server resample, its native path simply mis-fed; humm's adapter-side resample achieves the *documented intent* at one place. Recorded in the ledger.)
- `n_batch_samples` = 5 s × rate; `keep_sample_seconds` = 0.15. In **append-and-correct** mode: `n_batch_samples` = `transcription_step` (default 3.5 s, range 2.0–5.0) × rate; `keep_sample_seconds` = 1.5.
- `max_queue_size` = 3 × `n_batch_samples`; the accumulation queue **drops incoming samples** while full (Buzz drops in the callback; humm drops at ring-drain into the accumulation queue — same observable policy: audio is dropped when > 3 batches behind).
- Loop: when queue ≥ `n_batch_samples`: `cut = find_silence_cut_point(queue[..n_batch], rate)`; extract `samples = queue[..cut]`; advance queue by `cut` (append modes) or `cut − keep_samples` (append-and-correct — the 1.5 s keep-back overlap that feeds the merge); publish batch RMS (meter "Average volume") + queue size (meter "Queue"); **skip** the batch when its RMS `< silence_threshold` (default 0.0025); else transcribe with the rolling prompt: `initial_prompt` starts as the option value and becomes each result's text (Buzz's native-Whisper path; its whisper-server path dropped the rolling prompt on the floor — the analysis-documented behavior is rolling, kept). Emit the text.
- `find_silence_cut_point` port: search window = last 1.5 s, RMS windows of 0.02 s, threshold = 0.5 × mean window RMS, rightmost quiet window's midpoint, fallback = full batch. Byte-exact math (same window count arithmetic `(len − window) / window`, midpoint `search_start + i·window + window/2`).
- Timing: Buzz's `time.sleep(0.5)` idle/skip pacing becomes a 500 ms parked wait on the session thread (same cadence, interruptible by stop).

**2.2.4 Engine (ADR-008).** In-process whisper-rs; the engine loads **once per session** and transcribes each chunk with a live parameter set mirroring Buzz's `whisper-server` flags natively: `n_max_text_ctx = 8` (`--max-context 8`), `suppress_nst` + `entropy_thold 2.8` (already engine defaults), no timestamps needed (segment texts joined, `" "`-separated per Buzz's server-response join), language/task/temperature per options, **no VAD**, word timestamps forced off. Exposed as an explicit live-params constructor in `humm-whisper` (not a bool smuggled through `EngineOptions` — file and live paths stay independently readable). Cancellation via the existing abort callback checking the session's `CancelToken`.

### 2.3 F-062 display modes

`RecordingTranscriberMode` = `AppendBelow` / `AppendAbove` / `AppendAndCorrect` (persisted as index, default 0 = AppendBelow — Buzz parity; UX_SPEC §6's "default Append and correct" applies only until Buzz's own persisted-index parity kicks in — i.e. humm defaults the *stored index* to Buzz's default 0; the UX_SPEC line is corrected this milestone, see §9).

Every arriving text passes `filter_text` first: trim; return empty (⇒ ignored) unless it contains a `\w` char (Unicode word character, Buzz regex parity).

- **Append below**: panel text += separator (when non-empty) + text; cursor pinned to end.
- **Append above**: panel text = text + separator + panel text; cursor pinned to start.
- **Append and correct** (`process_transcription_merge` port):
  - `transcripts.push(text)`; for each adjacent pair, `find_common_part` (longest common substring via DP, returned only when ≥ 5 chars) trims `texts[i]` to end at the common part and `texts[i+1]` to start at it; the last pair's common length feeds hide-unconfirmed.
  - **Hide unconfirmed** (default true): the display copy of the last text is truncated to the confirmed overlap length; no overlap ⇒ the last text is dropped from display entirely.
  - Merge: fold `merge_text_no_overlap` (longest suffix-of-A = prefix-of-B overlap elision) over the display list; then the sentence-space regex `([.!?。！？])([A-Z])` → `\1 \2`.
  - Panel shows the merged text wholesale (cursor to end).

Unit-test target: Buzz-derived cases for `find_common_part` (incl. the ≥ 5 gate), `merge_text_no_overlap`, and full `process_transcription_merge` sequences (overlapping chunk texts, hide-unconfirmed on/off, no-overlap drop, sentence-space fix) — vectors derived by hand-executing Buzz's Python on representative inputs (and cross-checked with the real Python via the local interpreter where feasible).

### 2.4 F-064 live export & upload

Settings-gated (`export_enabled`, default false). On each session start (`setup_for_export` port):

- Filename = template (custom `export_file_name` setting, else default `{{ input_file_name }} ({{ task }}d on {{ date_time }})`) with `{{ input_file_name }}` → `"live recording"`, `{{ task }}`/`{{ language }}`/`{{ model_type }}`/`{{ model_size }}`/`{{ date_time }}` (format `%d-%b-%Y %H-%M-%S`) substituted, + `.txt`/`.csv`. `model_type` renders `"whispercpp"`-equivalent humm wording — humm has one live backend; use `"whisper"` (documented).
- `.translated` sibling path computed; both files **truncated if they already exist** at session start (Buzz parity — translation file included even though M6 owns writing it).
- Export folder must exist, else export silently disables for the session (Buzz parity).
- **TXT**: append mode = write `text + separator`, then if `max_entries > 0` re-read, split on separator, keep last N, rewrite. Prepend mode (append-above) = read, `text + separator + existing`, cap first N, rewrite. Append-and-correct = whole-file rewrite with the merged text.
- **CSV**: single-row, utterance-per-column: read existing row, append (below) / prepend (above) column, cap to last/first N, rewrite whole file with `utf-8-sig` BOM. Append-and-correct = truncate then write one column containing the merged text.
- **Retries**: 5 attempts, 200 ms delay, on the locked-file error class (Windows parity; on Unix the first attempt effectively always wins) — port the retry loop verbatim; other I/O errors log-and-give-up.
- **Upload** (✚ + parity): effective URL = `HUMM_UPLOAD_URL` env if set, else the advanced-section field (✚ — Buzz env-only; env wins, documented). On session start: `POST {"kind":"clear","text":""}`. Per utterance: `POST {"kind":"transcript","text":<raw filtered text>,"mode":"replace"|"append"}` (replace iff append-and-correct), `Content-Type: application/json`, 15 s timeout, failures logged never surfaced. Sent from the session thread (blocking reqwest — never the UI thread).

Verification: snapshot tests for TXT (all three modes × separator × max-entries) and CSV (BOM byte, quoting via the `csv` crate round-trip against Buzz-shaped fixtures); upload tested against a local `TestServer` asserting body shapes.

### 2.5 Audio meter (F-060)

Custom painted widget, port of `audio_meter_widget.py`: fixed height 56, bars 2 px wide / 1 px margin drawn symmetrically from the horizontal center, bar count = half-width / 3, bar `i` active when `(current_amplitude − 0.00005) × 10 > i / n_bars`; active/inactive colors per light/dark theme (`#555`/`#BBB` light, `#999`/`#555` dark); below the bars a centered caption `"Average volume: {avg:.4}  Queue: {n}"`. `current_amplitude = max(new, current × 0.95)` decay smoothing; reset zeroes all three.

Data feed: `MeterSnapshot` — three atomics (current RMS per callback; average = 1 s-accumulated RMS in meter-only mode, batch RMS while recording — Buzz parity via `RecordingAmplitudeListener` vs the transcriber's signals; queue size in samples). The window requests repaint at ~30 ms cadence while visible (meter animates at capture rate without event-per-callback churn; acceptance: no jank).

### 2.6 F-065 presentation window

Separate deferred viewport (Buzz: separate `QWidget` window), 800×600, title "Live Transcript Presentation", opened from the presentation bar's "Show in new window" button; re-click focuses. Content = the transcription panel's current text, big-type.

- **Bar controls** (visible while recording): show-window button · "Text Size:" spinbox 10–100 pt (persisted, 100 ms-debounced) · "Theme" combo Light/Dark/Custom (persisted) · text-color + bg-color pickers (visible only on Custom; egui color picker replaces `QColorDialog` — mechanism deviation, same capability) · fullscreen button (enabled only while the window is open).
- Colors: Light = #000 on #FFF, Dark = #FFF on #000, Custom = persisted hex pair.
- **Fullscreen**: `ViewportCommand::Fullscreen`; **Esc exits fullscreen** (key handled in the presentation viewport).
- **User style override** (parity of capability, mechanism deviation — egui renders no HTML/CSS): if `<config_dir>/humm/presentation_style.css` exists, parse the properties Buzz's own default stylesheet uses — `color`, `background-color`, `font-size` — and apply them over the theme; unrecognized properties are ignored with one log line. Documented in HUMM_SURFACE §6.
- Live updates: transcript text mirrors the panel on every accepted utterance (replace-wholesale, matching Buzz's `setHtml` of the full text).

### 2.7 Clipboard copy (F-065)

Copy bar (shown when stopped): right-aligned "Copy" button → whole panel text (trimmed) to the clipboard via egui's clipboard; button relabels "Copied!" for 2 s (or "Nothing to copy!" for 1.5 s when empty), then reverts. (Buzz's "Copy failed" arms are Qt-object-lifetime checks with no egui analogue — egui's copy command cannot fail observably; documented.)

### 2.8 ✚ Realtime-fit warning + measurement

- Per session, the loop records `Σ audio-seconds transcribed / Σ wall-seconds spent in engine calls` (skipped-silence batches excluded); at stop, the ratio is stored per model size in `RecordingSettings::measured_speed` (last-run-wins).
- The window shows the inline caution ("<size> transcribed at N.N× realtime last run — live transcription may fall behind") when the selected size's stored ratio < 1.0. No entry ⇒ no warning (first run is the measurement).
- The M1 benchmark's small-model figure (17.3× RT on this machine) predicts comfortable headroom; the acceptance run (§8) turns that prediction into a measured, windowed live number.

## 3. Session orchestration (`live::session`)

`RecordingSession` (core) is generic over `AudioCapture` + a live-transcribe closure/trait + a `LiveEventSink`, so the whole loop unit-tests with `MockCapture` + a scripted fake engine:

```
LiveEvent { Text(String), MeterBatch { rms, queue_len }, ModelDownload(f32), Error(String), Stopped }
```

GUI mapping: `Text` → display-mode application (§2.3) + persistence (§4) + live export/upload (§2.4) + presentation mirror; `Error` → Buzz's error dialog text + stopped state; `Stopped` → stopped-state UI + session finalization. Stop is a `CancelToken`; in-flight engine calls abort via the existing callback; the session thread acknowledges by sending `Stopped`.

## 4. Persistence (Q-10 ✚ — approved improvement; Buzz persists nothing)

- **At record start** (after the model is ready, before the first chunk): insert a `transcription` row — `Source::LiveRecording`, status `InProgress`, name `"Live recording <%d-%b-%Y %H-%M-%S>"` (the export template's own datetime format, so table row and any exported file read alike; visible in the main table immediately via `AppEvent::TranscriptionsUpdated`), full `TranscriptionOptions` persisted (word timestamps off).
- **Per accepted utterance**: append one segment row `[chunk_start_ms, chunk_end_ms]` (stream-position timeline: batch start/end relative to record start) with the **raw filtered utterance text** — raw chunks, not the merged view: the merge is a display transform; segments keep the honest timeline the viewer/export machinery is built on. (Append-and-correct's panel text and the persisted segments therefore differ by design; the viewer's Text mode reads close to the append-below view. Ledger entry.)
- **At stop**: mark Completed (+ Date Completed). Row now opens in the M4 viewer via every existing open path (F-003/F-004 gates already admit Completed).
- **Crash-safety**: segments are written as they arrive, so a killed process loses at most the in-flight chunk. The startup stale-flip (`cancel_stale_tasks`) gains a special case: stale `InProgress` rows with `Source::LiveRecording` flip to **Completed**, not Canceled — a live session's partial transcript is finished work worth opening, and Canceled would lock it behind the viewer's Completed/Skipped gate. (Store unit tests pin both arms.)
- Store writes follow ADR-018: failures log + surface via the existing `StoreWriteFailed` machinery; the session itself never aborts on a store error.

## 5. Boundary additions (`humm-core::boundary`)

```rust
pub struct AudioDevice { pub id: String, pub name: String, pub is_default: bool }
pub trait AudioCapture: Send {
    fn devices(&self) -> Result<Vec<AudioDevice>, AudioError>;
    /// 16 kHz mono f32 guaranteed at the producer (resample/downmix inside
    /// the adapter, ADR-005). `meter` receives per-callback RMS.
    fn start(&mut self, device_id: Option<&str>, sink: rtrb::Producer<f32>,
             meter: Arc<MeterSnapshot>) -> Result<Box<dyn CaptureHandle>, AudioError>;
}
pub trait CaptureHandle: Send { fn stop(self: Box<Self>); }
```

- `rtrb` is a pure-Rust SPSC ring (workspace dep already reserved) — core naming it is fine (Standing Policy's adapter rule targets non-Rust boundaries); the trait stays mock-implementable in-core.
- Device ids are stable name-based strings (cpal exposes names; indices are not stable across enumerations). Default = cpal's `default_input_device()`, fallback first device (Buzz combo parity).
- `CpalCapture` obligations: request 16 k mono f32 input config; if unsupported, open the device's default config and downmix (channel-average) + rubato-resample to 16 k in the drain path — never in the RT callback beyond preallocated state; f32/i16/u16 sample formats accepted (converted); ring capacity ≈ 1 s at 16 k with drop-count logging on overflow.
- Contract tests (`#[ignore]`, real default device): stream opens, samples arrive at 16 k mono within tolerance, RMS of a quiet room is finite, stop joins cleanly. The resample branch gets a real run on this machine's 48 kHz default mic (covers the carried M4 hardware-debt item).

## 6. Settings (`humm-core::settings`)

`RecordingSettings` (sticky `recording-transcriber/*` parity — saved on window close like Buzz, except the fields Buzz saves immediately, which humm also saves immediately: mode, hide_unconfirmed, export_*): `language`, `task`, `model_size`, `initial_prompt`, `silence_threshold` (default 0.0025), `line_separator` (default `"\n\n"`; the advanced sheet edits it escaped, `\n` ⇄ literal, `unicode_escape` parity), `transcription_step` (default 3.5), `mode` (index, default 0), `hide_unconfirmed` (default true), `export_enabled` (false), `export_folder`, `export_file_name`, `export_file_type` (`"txt"`), `export_max_entries` (0 = all, range 0–99), `upload_url` (✚), `measured_speed` (map size → f32, ✚). `PresentationSettings`: `text_size` (24, range 10–100), `theme` (`"light"`), `text_color` (`"#000000"`), `background_color` (`"#FFFFFF"`).

## 7. What is deliberately NOT in M5

- F-063 live translation (M6): no translator, no translation panel, no `.translated` writes; the advanced sheet's LLM section renders disabled + tooltip.
- Buzz's four extra live backends (server/faster-whisper/HF/OpenAI-API): ADR-008 consolidation — the OpenAI-API live path arrives with M6's engine work if parity demands it (matrix note stays on F-041).
- Preferences-level shortcut rebinding of Ctrl+R (M7, F-005 editor half).

## 8. Acceptance (from IMPLEMENTATION_PLAN §M5 + brief)

1. Live transcription with `small` sustains realtime on this machine — **measured, windowed run** (real mic or loopback fixture; report the §2.8 ratio).
2. Append-and-correct merge passes unit tests on Buzz-derived cases.
3. Live TXT/CSV exports match documented formats (snapshot).
4. A closed recording window loses nothing: the session row survives restart and opens in the viewer (store-reopen automated test + windowed check).
5. Meter repaints at capture rate without jank (windowed).
6. No transcription-path bench regression (criterion suite).
7. gui-manual checklist v3 executed.

## 9. Analysis corrections & ledger candidates (finalized in the milestone docs sweep)

- UX_SPEC §6 "Live mode default Append and correct" contradicts Buzz's stored-index default 0 (Append below) — corrected to parity default with the ✚ dropped (Buzz persists last choice anyway).
- Ledger: rolling-prompt fidelity (Buzz's live whisper-server path silently ignored the rolling prompt; humm implements the documented rolling behavior). Adapter-side 16 k resample vs Buzz's device-rate loop. Merged-display vs raw-segment persistence (§4). Stale-flip live-rows-complete special case (§4). Clipboard cannot-fail arms not ported (§2.7). CSS-file subset mechanism (§2.6).

## Carried tickets folded into M5 (from `humm-m5-tickets`)

- `merge_algo_from` (resizer.rs) delegation to core's env-override rule — cleanup task.
- GUI queue-path paragraph-split env unification (`options_sheet::build_specs`) — cleanup task (HUMM_SURFACE §5 known-gap paragraph retired).
- 48 kHz-device resample-branch real run (§5 contract test) + fractional-fps (29.97) video spot-check — hardware-debt task.
- F-045 real-Buzz differential: one more time-boxed attempt with the anaconda python3 lead from M4; remaining blockers stay visible in the milestone report.

## 10. Deviations ledger (M5, → matrix notes / ADR-021), finalized in Task 14's docs sweep

Same shape as M4's own ledger (§9 of `2026-07-23-m4-transcript-viewer-design.md`).

| # | Deviation | Kind |
|---|---|---|
| 1 | Rolling prompt fed forward per live chunk (each call's own returned text becomes the next call's `initial_prompt`) | parity realization — Buzz's native path rolls the prompt too; only its separate `whisper-server` subprocess path silently dropped it (dead code humm doesn't reproduce, ADR-008) |
| 2 | Capture adapter always resamples to 16 kHz internally (ADR-005); the chunker never sees a device-rate batch | mechanics — Buzz ran the whole loop at the device's native rate when 16 kHz wasn't offered, so `n_batch_samples` varied by device; humm's adapter-side resample makes that variance disappear before the chunker ever runs |
| 3 | Append-and-correct's negative-slice keep-back (`queue[cut - keep_samples:]` when `cut < keep_samples`) ported as Python's real signed-slice semantics (retains the last `keep_samples - cut` elements of the **pre-drain** queue, clamped to the whole queue when shorter), not a "clamp to zero" simplification | faithful port of an undocumented-looking corner (Task 1 review finding 1) |
| 4 | `Err(EngineError::Canceled)` from an in-flight live engine call (i.e. every ordinary Stop landing mid-batch) is a benign loop exit — no `Error` event, straight to the unconditional `Stopped` | parity — Buzz's own `recording_transcriber.py:356-359` draws the identical `is_running` distinction before deciding whether a caught exception is a real error; **empirically confirmed** during the Task 14 windowed acceptance run: `whisper_full_with_state: failed to decode` fired in whisper.cpp's own logger at the exact Stop instant, and the session row still completed with no `error_message` |
| 5 | `LiveEvent::Stopped` carries the session's `LiveMeasurement` directly, one payload, rather than a separate stop signal + measurement pair | mechanics deviation from §3's own sketch (Task 8) — one less event type to thread through the GUI drain for no lost information |
| 6 | Capture teardown ordering is enforced, not just documented: `Stopped` is sent only *after* `capture_handle.stop()` returns, so `RecordingVm::finish_stopping`'s meter-only-capture restart can never race the just-finished session's own stream tearing down | hardening (Task 11 review finding 1) — Buzz has no analogous race (its `RecordingAmplitudeListener` and transcriber are separate Qt objects with their own lifetimes) |
| 7 | Live utterances persist to the store **raw** (post-`filter_text`, pre-merge); the append-and-correct panel/export/upload views apply the merge on top of that same raw text at display/write time | design (spec §4) — the merge is a display/export transform, so the segment timeline stays the honest stream-position record the viewer/export machinery already assumes; the panel and the persisted segments can therefore read differently in append-and-correct mode, by design |
| 8 | Startup `cancel_stale_tasks` flips a stale `InProgress` **live-recording** row to `Completed`, not `Canceled` (every other stale row still flips to `Canceled`) | design (spec §4, Q-10) — a killed live session's partial transcript is finished work worth opening in the viewer, and `Canceled` would lock it behind the viewer's Completed/Skipped-only gate |
| 9 | Clipboard "Copy failed" arms (Buzz's Qt-object-lifetime checks around `QClipboard`) are not ported — `egui::Context::copy_text` cannot fail observably | mechanics — nothing to port; only the success/"nothing to copy" arms exist |
| 10 | Presentation-window user stylesheet override understands a three-property subset (`color`/`background-color`/`font-size`) of Buzz's full CSS, from `<config_dir>/presentation_style.css` (not Buzz's OS cache-dir `presentation_window_style.css`) | mechanism deviation (spec §2.6) — egui has no HTML/CSS renderer; the subset covers every declaration Buzz's own generated stylesheet actually emits |

New ADR: **ADR-021** — live-recording session architecture (in-process streaming loop as ADR-008's follow-through, adapter-side 16 kHz capture, session/capture meter-ownership split, raw-segment persistence + live-aware stale-flip, teardown-ordering + panic-recovery hardening, measured-speed realtime warning, CSS-subset presentation override).
