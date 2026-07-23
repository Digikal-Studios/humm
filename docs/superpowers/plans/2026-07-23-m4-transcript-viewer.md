# M4 Transcript Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the transcript viewer — review, edit, search, synced audio/video playback, export, resize/merge — per `docs/superpowers/specs/2026-07-23-m4-transcript-viewer-design.md` (**the spec; read it before any task**), unlocking F-003/F-004's viewer-gated halves.

**Architecture:** Viewer windows are egui **deferred viewports** keyed by `TranscriptionId` over `Arc<Mutex<ViewerVm>>`; segments live in memory and every edit writes the store instantly. New `humm-audio` adapter (cpal + rubato varispeed `Player`, position atomics = A/V master clock); video via a new ffmpeg-rawvideo `FrameStream` adapter in humm-media; resize/merge are native core ports of srt_equalizer 0.1.10 + the stable-ts 2.19.1 regroup subset.

**Tech Stack:** existing egui/eframe/egui_extras/egui_kittest 0.35 + rfd 0.17; NEW: cpal 0.18, rubato 4.0, rtrb (humm-audio only). Reference clones (pinned): `<scratchpad>/stable-ts` (2.19.1), `<scratchpad>/srt_equalizer` (tag v0.1.10), `<scratchpad>/srt` (srt.py), where `<scratchpad>` = `/private/tmp/claude-501/-Users-jawad-RustroverProjects-humm--claude-worktrees-m4-transcript-viewer-5aa260/37a55b77-4cfe-4ba6-b15d-21eee080009b/scratchpad`.

## Global Constraints

- Every commit passes `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Commit messages: `M4: <what> (F-###[, F-###])` + `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Build env: `export CARGO_HOME="$PWD/.cargo-home"`; registry unpacking (new deps: cpal/rubato/rtrb, Task 6), local TCP test servers, GPU/Metal, and CoreAudio device opens need the sandbox lifted; audio/video device contract tests are `#[ignore]`.
- Buzz is ground truth; the spec §2 pins the behavior with `file:line` citations — **re-read the cited Buzz source before implementing each feature** (`/Users/jawad/RustroverProjects/buzz/...`). For F-086, the pinned third-party clones above are ground truth; port from source, not memory.
- No network in unit tests. humm-core keeps `#![warn(missing_docs)]`, `#![forbid(unsafe_code)]`; exhaustive matches on domain enums; `thiserror` in libs; no `unsafe` anywhere in M4 (humm-audio included).
- New UI strings: add to `crates/humm-gui/assets/i18n/en/humm.ftl` **and** `all_keys_used()` in `i18n.rs` in the same commit (test-enforced). Parity-relevant strings byte-match Buzz (spec cites each).
- Viewer content renderers are plain `fn(&mut egui::Ui, &mut ViewerVm, ...)` so kittest drives them via `Harness::new_ui_state` without a real viewport (M3 preferences precedent). All external effects behind injected `ViewerDeps` (store/decoder/prober/streamer/audio-output/dialogs) so kittest never touches devices or rfd.
- The queue owns `status` for queue-executed tasks (ADR-018); the Task 1 store APIs that write status are for resizer-created rows only (spec §4 note goes in the queue module docs in Task 12).
- Settings persistence uses the existing diff-on-change pattern (`persist_layout_if_changed`); extend, don't fork.
- PARITY_MATRIX rows move with the task that completes them; final sweep in Task 14.

---

### Task 1: Store schema v2 + segment/copy APIs (F-081/F-084/F-086 groundwork)

**Files:**
- Modify: `crates/humm-store/src/schema.rs` (v2 migration), `crates/humm-store/src/store.rs`, `crates/humm-store/src/lib.rs` (export `SegmentRow`)

**Interfaces (Produces — spec §4 verbatim):**
```rust
pub struct SegmentRow { pub id: i64, pub start: Millis, pub end: Millis, pub text: String, pub translation: String }
impl SqliteStore {
    pub fn get_segment_rows(&self, id: TranscriptionId) -> Result<Vec<SegmentRow>, StoreError>; // ORDER BY id ASC
    pub fn update_segment_times(&self, seg: i64, start: Millis, end: Millis) -> Result<(), StoreError>;
    pub fn update_segment_text(&self, seg: i64, text: &str) -> Result<(), StoreError>;
    pub fn update_segment_translation(&self, seg: i64, translation: &str) -> Result<(), StoreError>;
    pub fn replace_segments(&self, id: TranscriptionId, segs: &[Segment]) -> Result<(), StoreError>; // one txn: DELETE + INSERT, translations dropped (Buzz parity)
    pub fn copy_transcription(&self, id: TranscriptionId) -> Result<TranscriptionId, StoreError>;   // Buzz DAO :107-160: copy all columns, new uuid, time_queued=now, status='queued'
    pub fn mark_replaced_completed(&self, id: TranscriptionId, segs: &[Segment]) -> Result<(), StoreError>; // status='completed'+time_ended=now, insert segs (translation='')
    pub fn update_progress_direct(&self, id: TranscriptionId, fraction: f64) -> Result<(), StoreError>;
    pub fn set_last_export_folder(&self, id: TranscriptionId, dir: &Path) -> Result<(), StoreError>;
    pub fn last_export_folder(&self, id: TranscriptionId) -> Result<Option<PathBuf>, StoreError>;
}
```

- [ ] **Step 1: Failing tests** in `store.rs` `mod tests` (existing temp-dir pattern):
  - `migration_v1_to_v2_adds_last_export_folder` (open v1 DB → reopen → column exists, user_version==2, existing rows intact)
  - `newer_schema_version_is_refused_with_actionable_error` (set user_version=99 → open errs mentioning "newer" and the path — closes the M2-minor guard-test ticket)
  - `segment_rows_round_trip_with_ids` (mark_completed 3 segs → get_segment_rows: ids ascending, fields match)
  - `update_segment_times_text_translation_persist` (each update → re-read reflects it; others untouched)
  - `replace_segments_swaps_all_rows_in_one_txn` (replace with 2 new → old ids gone, translations empty)
  - `copy_transcription_copies_row_with_new_id_queued_now` (copy → new id ≠ old, status queued, file/options_json/name/notes copied, **no segments copied**)
  - `mark_replaced_completed_sets_status_and_inserts` / `update_progress_direct_writes_progress`
  - `last_export_folder_round_trip_and_default_none`
- [ ] **Step 2:** `cargo test -p humm-store` — new tests FAIL.
- [ ] **Step 3: Implement.** `schema.rs::migrate`: `match user_version { 0 => full DDL + set 2, 1 => ALTER TABLE transcription ADD COLUMN last_export_folder TEXT; set 2, 2 => ok, n => Err(StoreError::SchemaTooNew { found: n, supported: 2, path }) }` (new error variant, actionable message "database was written by a newer humm — upgrade humm or move <path>"). Store methods per signatures; `copy_transcription` mirrors Buzz DAO column list + humm's extra columns (`options_json`, `kind`, `last_export_folder` reset to NULL).
- [ ] **Step 4:** `cargo test -p humm-store` green; workspace gates; **Commit** `M4: store schema v2 + segment/copy APIs (F-081, F-084, F-086)`.

### Task 2: Core viewer logic — timestamp parse, edit rules, search, text views

**Files:**
- Create: `crates/humm-core/src/viewer/mod.rs`, `viewer/timestamp.rs`, `viewer/edit.rs`, `viewer/search.rs`, `viewer/text_view.rs`
- Modify: `crates/humm-core/src/lib.rs` (`pub mod viewer`)

**Interfaces (Produces):**
```rust
// timestamp.rs — Buzz transcription_segments_editor_widget.py:42-69
pub fn parse_timestamp(s: &str) -> Option<Millis>;      // HH:MM:SS[.mmm] | MM:SS[.mmm] | SS[.mmm]; int parts; None on garbage
// display = export::to_timestamp(ms, '.')

// edit.rs — spec §2.2; a pure decision, caller applies writes
pub struct SegmentTimes { pub id: i64, pub start: Millis, pub end: Millis }
pub enum Field { Start, End }
pub enum EditOutcome { Rejected, Apply { value: Millis, neighbor: Option<(i64, Field, Millis)> } }
pub fn timestamp_edit(rows: &[SegmentTimes], idx: usize, field: Field, new_value: Millis, clamp_floor_zero: bool) -> EditOutcome;
pub const NUDGE_MS: i64 = 500;

// search.rs — spec §2.3
pub const SEARCH_CAP: usize = 100;
pub const SEARCH_DEBOUNCE_MS: u64 = 300;
pub enum SearchScope<'a> { Rows { texts: … }, Text(&'a str) }   // impl detail free, but:
pub struct RowMatch { pub row: usize, pub in_translation: bool }
pub fn search_rows(rows: &[(String, String)], has_translations: bool, query: &str) -> Vec<RowMatch>; // pass1 text, pass2 translation, shared cap
pub struct TextMatch { pub start: usize, pub end: usize }       // byte offsets into the ORIGINAL string
pub fn search_text(haystack: &str, query: &str) -> Vec<TextMatch>; // char-wise case-insensitive, cap
pub fn wrap_next(current: usize, len: usize) -> usize; pub fn wrap_prev(current: usize, len: usize) -> usize;
pub fn results_label(current: usize, total: usize) -> String;   // "1 of 100+ matches"/"K of N matches"/"No matches found" — byte-exact Buzz strings

// text_view.rs — spec §2.1
pub fn text_mode_text(segments: &[Segment], paragraph_split: Millis) -> String;   // trim+" " join, "\n\n" on gap ≥ split, final trim
pub fn translation_mode_text(segments: &[Segment]) -> String;                     // flat " " join of trimmed translations (NO split — Buzz quirk)
pub fn paragraph_split_from_env() -> Millis;                                      // HUMM_PARAGRAPH_SPLIT_TIME, default 2000 (shared constant with export)
```

- [ ] **Step 1: Failing tests** (per module `mod tests`):
  - timestamp: full/partial forms, no-fraction, garbage→None, round-trip with `to_timestamp`.
  - edit (the Buzz clamp matrix, spec §2.2): start≥end rejected; end≤start rejected; start<prev.end → neighbor `(prev.id, End, new)`; end>next.start → neighbor `(next.id, Start, new)`; nudge floor `max(0,·)` only when `clamp_floor_zero`; first/last rows have no neighbor to clamp.
  - search: cap at exactly 100 across both passes; translation pass only when flag; ordering text-then-translation; duplicate row allowed; case-insensitive; `search_text` byte offsets valid on non-ASCII haystack ("Müller" match "müller"); wrap modulo incl. len 1; label strings byte-exact (`"1 of 100+ matches"`, `"3 of 7 matches"`, `"No matches found"`).
  - text_view: gap exactly at split boundary (≥, not >) breaks; below doesn't; translation join never breaks; env override respected (serial test w/ temp env guard as in existing env-reading tests).
- [ ] **Step 2:** `cargo test -p humm-core viewer::` FAIL → **Step 3: Implement** (port from the Buzz lines the spec cites; re-read them first). → **Step 4:** green + workspace gates.
- [ ] **Step 5: Commit** `M4: core viewer logic — timestamp parse, edit clamp rules, search, text views (F-081, F-082, F-080)`.

### Task 3: `resize::` — srt_equalizer port + extend endings (F-086 resize half)

**Files:**
- Create: `crates/humm-core/src/resize.rs`; `validation/m4-differential/gen_resize_fixtures.py`; `crates/humm-core/tests/fixtures/resize/*.json` (generated, committed)
- Modify: `crates/humm-core/src/lib.rs`

**Interfaces (Produces):**
```rust
pub fn equalize(segments: &[Segment], target_chars: usize) -> Vec<Segment>;
// per-segment srt_equalizer.split_subtitle(method="punctuation") in ms-float space, chained indices,
// then round_half_even + drop rows where rounded start == rounded end (Buzz on_resize_button_clicked)
pub fn extend_endings(segments: &[Segment], amount: Millis) -> Vec<Segment>; // min(end+amount, next.start); last unclamped
pub(crate) fn round_half_even(x: f64) -> i64;   // Python round()
```

- [ ] **Step 1: Reference fixtures.** Write `gen_resize_fixtures.py`: `sys.path` the pinned clones (`srt/`, `srt_equalizer/src/`), replicate Buzz's exact call (`srt.Subtitle(index=segment_id, start=ms_int, end=ms_int, content=text)`; `split_subtitle(sub, target_chars, last_index, method="punctuation")`; chain `last_index`; `round()`+drop-equal) over ≥6 fixture transcripts (short/no-punct/long-run-on/CJK punctuation/42-boundary/one-word-over-target) × target_chars {20, 42}. Output JSON `{input, target, expected:[{start,end,text}]}`. Run with system `python3` (pure stdlib + srt.py — no pip). Commit generator + outputs.
- [ ] **Step 2: Failing tests**: `equalize_matches_python_reference_fixtures` (loop all fixtures, exact match), unit cases for `split_by_punctuation` internals (punct-attach, coalesce, greedy overflow — port the exact `srt_equalizer.py:114-190` accounting incl. the trailing-space `+1` quirk), `round_half_even_matches_python` (0.5→0, 1.5→2, 2.5→2, negatives), `extend_endings_clamps_to_next_start_last_unclamped`, `equalize_passthrough_when_under_target`.
- [ ] **Step 3:** FAIL → implement (f64 ms timing: `chunk_dur = chunk_len/total_len*(end-start)` accumulated, last chunk pinned to original end) → green.
- [ ] **Step 4:** Workspace gates. **Commit** `M4: resize — srt_equalizer 0.1.10 native port + extend endings, Python-reference differential fixtures (F-086)`.

### Task 4: `regroup::` — stable-ts 2.19.1 subset port (F-086 merge half)

**Files:**
- Create: `crates/humm-core/src/regroup/mod.rs`, `regroup/parser.rs`, `regroup/ops.rs`, `regroup/words.rs`; `validation/m4-differential/gen_regroup_fixtures.py` + `stubs/{torch,tqdm,numpy}` import stubs; `crates/humm-core/tests/fixtures/regroup/*.json`
- Modify: `crates/humm-core/src/lib.rs`

**Interfaces (Produces):**
```rust
pub struct RWord { pub text: String /* incl. trailing separator */, pub start: f64, pub end: f64 } // fake-seconds = ms/100 (spec §2.6 unit quirk)
pub struct RSegment { pub words: Vec<RWord> }   // start/end derived from first/last word
pub fn words_from_segments(segments: &[Segment], language: Option<&str>) -> Vec<RSegment>;
// separator "" iff language ∈ {zh,ja,th,lo,km,my} else " "; sentence grouping: regex `.*[.!?。！？]` MATCH (anchored-prefix, not fullmatch) on each segment's text ends a group; trailing partial group kept
pub enum RegroupError { UnsupportedOp { op: String, supported: &'static [&'static str] }, BadArgs(String) }
pub fn parse_rules(algo: &str) -> Result<Vec<Rule>, RegroupError>;   // grammar: ops `_`, args `+` positional (''→default), `/` list, `*` pair; int/float by `.`-presence; ops: mg, sp, sl only (spec ledger #8)
pub fn apply(rules: &[Rule], segments: Vec<RSegment>) -> Vec<RSegment>;
pub fn regroup_segments(segments: &[Segment], language: Option<&str>, algo: &str) -> Result<Vec<Segment>, RegroupError>;
// full Buzz pipeline: words → WhisperResult-equivalent normalization (remove_no_word_segments; force_order=False, check_sorted=False) → apply → ms via (x*100) trunc-toward-zero
pub fn default_algo(merge_by_gap: bool, gap: &str, split_by_punct: bool, punct: &str, split_by_len: bool, len: &str) -> String; // Buzz string builder incl. `++{len}+1` only when mg AND sl
pub fn algo_from_env_or(built: String) -> String;   // HUMM_MERGE_REGROUP_RULE override
```

- [ ] **Step 1: Trace the pinned source** (`<scratchpad>/stable-ts/stable_whisper/result.py`): `parse_regroup_algo` (:2980), `str_to_valid_type` (utils.py:20), `merge_by_gap` (:1642) + `get_gap_indices` (:1353) + `_merge_segments` (:1496) (max_words/max_chars/is_sum_max gating, lock, newline), `split_by_punctuation` (:1689) + `Segment.get_punctuation_indices` (:729) (str = ends-with; `(a,b)` = word-ends-a + next-word-starts-b) + `_split_segments` (:1446), `split_by_length` (:1896) + `get_length_indices` (:788) (even-split math), `WhisperResult.__init__` (:930) + `remove_no_word_segments`, `Segment` word-derived start/end + `char_count`. Record any behavior the spec didn't capture as notes in the module docs.
- [ ] **Step 2: Reference fixtures.** `gen_regroup_fixtures.py`: stub `torch`/`tqdm`/`torchaudio` (+ `numpy` minimal) modules on `sys.path`, import `stable_whisper.result.WhisperResult` from the pinned clone, build results from word-dicts exactly as Buzz's `get_transcript` does (`transcription_resizer_widget.py:52-93`), run `.regroup(algo)` for: Buzz's default UI string `mg=0.2++42+1_sp=.* /./. /。/?/? /？/!/! /！/,/, _sl=42`, mg-only, sp-only, sl-only, mg+sl-no-sp, CJK no-separator input, no-sentence-punct input (single trailing group). ≥8 fixtures, JSON with fake-seconds in/out. If stubbing fails after a time-boxed attempt (≤ half a day), fall back to hand-traced expected outputs from the source and record the gap for the milestone report (spec §5.5).
- [ ] **Step 3: Failing tests**: `regroup_matches_python_reference_fixtures`; parser unit tests (arg positional skip `''`, pair parsing `.* ` → `('.', ' ')`, int/float coercion, unsupported op error names op + supported set); per-op unit tests (gap merge at ≤ boundary; is_sum_max semantics; punct split str vs pair; length even-split); `words_from_segments` grouping tests (sentence-end regex contains-match, CJK separator, trailing group); `default_algo` builder matrix (the 3-checkbox × strings truth table incl. `++42+1` conditional); `algo_from_env_or` override; ms round-trip truncation (`int(x*100)` toward zero).
- [ ] **Step 4:** FAIL → implement → green → workspace gates.
- [ ] **Step 5: Commit** `M4: regroup — stable-ts 2.19.1 mg/sp/sl subset port + parser + reference fixtures (F-086)`.

### Task 5: Media boundaries — video probe fields, playback decode, FrameStream

**Files:**
- Modify: `crates/humm-core/src/boundary.rs` (+ errors), `crates/humm-media/src/probe.rs`, `crates/humm-media/src/decode.rs`, `crates/humm-media/src/lib.rs`
- Create: `crates/humm-media/src/video.rs`; extend `crates/humm-media/tests/contract.rs`

**Interfaces (Produces — spec §5.1/§5.3):**
```rust
// core boundary.rs
pub struct VideoStreamInfo { pub width: u32, pub height: u32, pub fps: f64 }
pub struct MediaInfo { …existing…, pub video: Option<VideoStreamInfo> }   // has_video stays (cover-art rule unchanged)
pub struct PlaybackPcm { /* Arc<[f32]> interleaved */ pub channels: u16, pub sample_rate: u32 }  // + samples(), frames(), duration()->Millis
pub trait AudioDecoder: …existing… {
    fn decode_playback(&self, path: &Path, cancel: &CancelToken) -> Result<PlaybackPcm, MediaError>;
}
pub struct VideoFrame { pub ts: Millis, pub rgba: Vec<u8>, pub width: u32, pub height: u32 }
pub trait VideoStreamer: Send + Sync { fn open(&self, path: &Path, start: Millis, info: &VideoStreamInfo) -> Result<Box<dyn FrameStream>, MediaError>; }
pub trait FrameStream: Send { fn frame_for(&mut self, position: Millis) -> Option<VideoFrame>; fn seek(&mut self, position: Millis) -> Result<(), MediaError>; }
```

- [ ] **Step 1: Failing unit tests.** probe: extend `parse_probe_json` tests — video stream `width/height/r_frame_rate "30000/1001"` → fps 29.97…, malformed rate → 25.0, fps floor 1.0, audio-only → `video: None`, duration falls back to video-stream duration when format-level absent (`ffmpeg_video_player.py:49-66`). video.rs: the **frame-selection rule is pure** — factor it as `pub(crate) fn select_frame(buf: &mut VecDeque<(Millis, Frame)>, position: Millis, frame_ms: f64) -> Option<Frame>` (drain entries `< position − frame_ms` while len>1; pop-return front if `ts ≤ position + frame_ms`; else None) and unit-test it with synthetic buffers (Buzz `ffmpeg_video_player.py:177-200` exact).
- [ ] **Step 2:** Implement probe fields + `decode_playback` (`ffmpeg -nostdin -i <p> -f f32le -ac 2 -ar 44100 -loglevel error -`, same chunked-read/kill-on-cancel/stderr-drain pattern as `decode`) + `FfmpegVideoStream` (reader thread, ≤60-frame deque, ts synthesized `start + n·1000/fps`, `-ss <s> -i <p> -f rawvideo -pix_fmt rgba -s WxH -r <fps> -an -`; `seek` = kill+respawn; Drop kills). All existing `AudioDecoder` impls in tests gain the new method (fake: return small stereo buffer).
- [ ] **Step 3: Contract tests** (`#[ignore]`, need ffmpeg): generate fixtures in a tempdir via ffmpeg (`-f lavfi -i testsrc2=duration=2:rate=25 -f lavfi -i sine=duration=2 …` mp4; `sine`-only wav): `decode_playback_stereo_44100_matches_duration` (frames ≈ duration±1%, channels 2, rate 44100), `video_stream_yields_monotonic_frames_with_dimensions` (~50 frames, w/h match probe, ts monotonic ascending from 0), `video_seek_restarts_at_position` (seek 1000 → first ts ≥ 1000−frame_ms), `probe_reports_video_info` (25 fps, 320×240 default testsrc2 size … assert probed values).
- [ ] **Step 4:** Unit green sandboxed; contract green sandbox-lifted (`cargo test -p humm-media -- --ignored`). Workspace gates. **Commit** `M4: media — video probe fields, playback decode, ffmpeg rawvideo FrameStream (F-083)`.

### Task 6: `humm-audio` crate — AudioOutput boundary + varispeed Player

**Files:**
- Create: `crates/humm-audio/Cargo.toml`, `src/lib.rs`, `src/output.rs` (CpalOutput), `src/player.rs`; `crates/humm-audio/tests/contract.rs`
- Modify: root `Cargo.toml` (member + `[workspace.dependencies]` cpal = "0.18", rubato = "4", rtrb = "0.3"), `crates/humm-core/src/boundary.rs` (+ `AudioError` in errors)

**Interfaces (Produces — spec §5.2):**
```rust
// humm-core
pub struct StreamSpec { pub sample_rate: u32, pub channels: u16 }
pub trait AudioOutput: Send + Sync { fn open(&self, spec: StreamSpec, fill: Box<dyn FnMut(&mut [f32]) + Send>) -> Result<Box<dyn OutputStream>, AudioError>; }
pub trait OutputStream: Send { fn play(&mut self) -> Result<(), AudioError>; fn pause(&mut self) -> Result<(), AudioError>; }
// humm-audio
pub struct CpalOutput;                       // impl AudioOutput; device-rate negotiation + adapter-internal resample if 44.1k unsupported (ADR-005)
pub struct Player { … }
impl Player {
    pub fn new(pcm: PlaybackPcm, output: Arc<dyn AudioOutput>) -> Result<Self, AudioError>;
    pub fn play(&self); pub fn pause(&self); pub fn toggle(&self);
    pub fn seek(&self, pos: Millis); pub fn set_speed(&self, speed: f32 /* clamped 0.1..=5.0 */);
    pub fn set_loop(&self, range: Option<(Millis, Millis)>);
    pub fn position(&self) -> Millis; pub fn duration(&self) -> Millis;
    pub fn is_playing(&self) -> bool; pub fn speed(&self) -> f32;
}   // Drop stops the stream
```
**Contract pinned by tests:** position = source-frame counter (atomic) advancing as the fill callback consumes; speed `s` consumes `s` source frames per output frame (varispeed; rubato engaged only at s≠1.0 — 1.0 is the exact-copy fast path); loop range wraps **sample-exactly** at `end`→`start` inside the feed; seek clamps to [0, duration]; end-of-media → is_playing()=false, position=duration.

- [ ] **Step 1: Failing unit tests** (`player.rs mod tests`, `MockOutput` capturing the fill closure; the test pulls N frames and asserts): `position_advances_with_frames_consumed`, `seek_moves_position_and_playback_origin`, `loop_wraps_exactly_at_end_boundary` (pull past the boundary in one fill call; wrapped mid-buffer), `speed_two_consumes_double_source_frames` (±1 frame tolerance for resampler priming), `speed_clamped_to_bounds` (0.01→0.1, 9.0→5.0), `pause_stops_frame_consumption`, `end_of_media_stops_and_pins_position`, `set_loop_none_clears_wrap`.
- [ ] **Step 2:** New crate skeleton compiles in workspace (sandbox-lift for registry unpack once) → tests FAIL → implement `Player` (feeder logic runs inside the fill callback: copy/resample from `Arc<[f32]>` at cursor; atomics for position/speed/loop/playing; no allocation in the steady-state fill path at speed 1.0) → green.
- [ ] **Step 3: CpalOutput** + contract test (`#[ignore]`): open default device at 44.1k stereo, play 100 ms sine, assert stream opened + fill called (count atomic). Run sandbox-lifted where CoreAudio permits; else document in the task report (M3 GPU precedent).
- [ ] **Step 4:** Workspace gates (workspace-wide fmt/clippy incl. new crate). **Commit** `M4: humm-audio — cpal AudioOutput adapter + varispeed Player with sample-exact loop (F-083, ADR-005/ADR-019)`.

### Task 7: Viewer window skeleton — deferred viewports, open paths, view modes (F-080 + F-003/F-004 unlock)

**Files:**
- Create: `crates/humm-gui/src/viewer.rs` (ViewerVm + deps), `crates/humm-gui/src/views/viewer.rs`
- Modify: `crates/humm-gui/src/app.rs` (viewers map, OpenSelected, deferred viewports, AppEvent channel), `crates/humm-gui/src/views/task_table.rs` (double-click/Return gating emits OpenSelected), `crates/humm-core/src/settings.rs` (`ViewerSettings`), `humm.ftl` + `i18n.rs`, `crates/humm-gui/tests/kittest_viewer.rs` (new)

**Interfaces:**
- Consumes: Task 1 `get_segment_rows`/`SegmentRow`; Task 2 `viewer::text_view`; Task 5/6 types (playback deferred to Task 10 — this task stubs `PlayerState::Unavailable`).
- Produces:
```rust
pub struct ViewerDeps { pub store: Arc<SqliteStore>, pub decoder: Arc<dyn AudioDecoder>, pub prober: Arc<dyn MediaProber>,
                        pub streamer: Arc<dyn VideoStreamer>, pub audio_output: Arc<dyn AudioOutput>, pub dialogs: Arc<dyn FileDialogs> }
pub enum ViewMode { Timestamps, Text, Translation }        // default Timestamps
pub struct ViewerVm { pub id: TranscriptionId, pub title: String, pub rows: Vec<SegmentRow>, pub has_translations: bool,
                      pub mode: ViewMode, pub close_requested: bool, /* search/edit/playback state in later tasks */ }
impl ViewerVm { pub fn open(id, deps, settings) -> Result<Self, StoreError>; }
pub fn viewer_ui(ui: &mut egui::Ui, vm: &mut ViewerVm, deps: &ViewerDeps, shortcuts: &ShortcutMap, settings: &Mutex<Settings>);
// app.rs
// viewers: BTreeMap<TranscriptionId, Arc<Mutex<ViewerVm>>>; open_viewer(id) opens-or-focuses (ViewportCommand::Focus);
// AppEvent enum + std::sync::mpsc channel field (Sender clonable into workers): AppEvent::TranscriptionsUpdated(TranscriptionId)
pub fn can_open(status: Status) -> bool;   // Completed | Skipped
```
- Settings: `ViewerSettings` per spec §6 (all fields + defaults; `playback_rate` clamped on load; `geometry: Option<ViewerGeometry>`).

- [ ] **Step 1: Failing tests.** Unit (`viewer.rs`): `can_open_matrix` (6 statuses), `open_loads_rows_and_flags_translations`, `title_is_file_basename`, mode-switch text content via `text_view` (already unit-tested in core — here assert VM wiring picks translation text). Settings: `viewer_settings_defaults_match_buzz` + `playback_rate_clamped_on_load`. kittest (`kittest_viewer.rs`, `Harness::new_ui_state` over `viewer_ui` with a seeded temp store + fake deps): `timestamps_mode_shows_segment_table_placeholder_rows`, `text_mode_shows_paragraph_joined_text`, `translation_menu_entry_hidden_without_translations`, `empty_transcript_shows_explanatory_state` (string: `viewer-empty = No segments — the input may be silence-only`).
- [ ] **Step 2:** FAIL → implement: `ViewerVm::open` (store read, `has_translations = any non-blank`), `viewer_ui` (toolbar View/Export-stub/Translate-disabled+tooltip/Resize-stub/Identify-Speakers-disabled+tooltip/Find-stub per spec §2.1 — Export/Resize/Find gain function in Tasks 9/11/12; this task renders them disabled with `viewer-*-pending` tooltips only where the feature is later-task, NOT for M6/M8 which get their permanent tooltips now), central content per mode (Timestamps = placeholder frame this task; table lands in Task 8), deferred viewport plumbing in app.rs (`show_viewport_deferred`, close-request harvest, geometry persist on close, focus-on-reopen), `OpenSelected` handler + per-row double-click gate + Return path in task_table (gate `can_open` on every path — spec ledger #13), retire `toolbar-open-transcript-tooltip` string → new enablement-rule tooltip text.
- [ ] **Step 3:** kittest green; workspace gates. Matrix: F-003/F-004 rows gain the unlock note (still gui-manual pending windowed run); F-080 → in-progress.
- [ ] **Step 4: Commit** `M4: viewer window skeleton — deferred viewports, open/focus paths, view modes, settings (F-080, F-003, F-004)`.

### Task 8: Segment editor table (F-081 GUI)

**Files:**
- Create: `crates/humm-gui/src/views/viewer_table.rs`
- Modify: `crates/humm-gui/src/viewer.rs` (edit state), `views/viewer.rs` (embed), `humm.ftl`/`i18n.rs` (column headers `viewer-col-start/end/text/translation`, autosave `viewer-saved = edited · saved`), `tests/kittest_viewer.rs`

**Interfaces:**
- Consumes: Task 2 `timestamp_edit`/`parse_timestamp`/`NUDGE_MS`, Task 1 update APIs.
- Produces: `viewer_table::ui(ui, vm, deps) -> Vec<EditApplied>`; `ViewerVm` gains `selected: Option<usize>`, `editing: Option<CellEditor { row, col, buffer, last_valid_ms }>`, `saved_flash: Option<Instant>`; `pub fn apply_timestamp_edit(vm, deps, idx, field, new_value, floor) -> bool` (runs core rule → store writes for value + neighbor → memory → flash; store `Err` → flash replaced by unsaved-warning toast per ADR-018) and `pub fn apply_text_edit(vm, deps, idx, col, text)` (translation edits recompute `has_translations`).
- Table: egui_extras virtualized, fixed row height = 4 × text line height, columns Start(120)|End(120)|Text|Translation(hidden unless has_translations), single-row selection, double-click or Return starts editing the focused cell; timestamp editor filters `[0-9:.]`, `+`/`-` keys nudge the **editor buffer** ±500 ms (floor 0), commit on Enter/Tab/focus-out (garbage → revert to `last_valid_ms`); text editor multi-line, Tab/Enter/**Esc** commit (spec ledger #6).
- Nudge shortcuts (⌘←/⌘→/⌘⇧←/⌘⇧→) act on `selected` with fresh values (floor-zero path), consumed only inside the viewer.

- [ ] **Step 1: Failing tests.** Unit: `apply_timestamp_edit_writes_value_and_clamped_neighbor_to_store` (seeded temp store; assert both rows updated in store AND memory), `apply_text_edit_translation_unhides_translation_column`, `nudge_uses_fresh_values_after_prior_edit`, editor-buffer nudge floor. kittest: `edit_survives_reopen` (drive a timestamp commit via the editing seam → new `ViewerVm::open` on same store → value persisted — **the milestone acceptance "edits survive restart" automated half**), `table_renders_100k_rows_without_panic` (smoke; perf bench is Task 14), `esc_commits_text_edit`.
- [ ] **Step 2:** FAIL → implement → green (typing-into-TextEdit via kittest is timing-fragile per M3 precedent — drive commits through the editing-state seam and assert the rendered result; document on the test).
- [ ] **Step 3:** Workspace gates. Matrix F-081 → implemented. **Commit** `M4: segment editor — inline timestamp/text editing, overlap clamping, instant persist (F-081)`.

### Task 9: Search bar (F-082 GUI)

**Files:**
- Create: `crates/humm-gui/src/views/viewer_search.rs`
- Modify: `viewer.rs` (SearchState), `views/viewer.rs`, `humm.ftl`/`i18n.rs` (`viewer-find`, `viewer-find-placeholder`, `viewer-find-clear`, nav tooltips — label strings come from core `results_label`), `tests/kittest_viewer.rs`

**Interfaces:**
- Consumes: Task 2 `search_rows`/`search_text`/`wrap_*`/`results_label`/`SEARCH_DEBOUNCE_MS`.
- Produces: `SearchState { visible, query, results: SearchResults, current: usize, last_edit: Option<Instant> }` on ViewerVm; `viewer_search::ui(...)`; debounce driven by frame time: pending when `last_edit.elapsed() ≥ 300ms` **and** trimmed len ≥ 2 at fire time (schedule `request_repaint_after` to guarantee the fire frame); mode switches re-run active search; Esc-in-input hides bar and clears; ⌘F toggle+focus+select-all; ⌘Return/⇧Return wrap navigation (also while input focused); visibility persisted (`find_widget_visible`).
- Timestamps-mode match → select+center-scroll row (`scroll_to_row(.., Align::Center)`); Text-mode match → highlight via layouter background section + scroll from galley offset.

- [ ] **Step 1: Failing tests.** Unit: `debounce_fires_only_after_300ms_and_2_chars` (injected clock), `mode_switch_reruns_search`, `esc_clears_and_hides`, `clear_keeps_bar_and_focus_flag`. kittest: `search_selects_matching_row_and_shows_label` (seeded rows, drive query via state seam + frames; assert `3 of 7 matches`-shaped label text), `nav_wraps_modulo`, `translation_matches_append_after_text_matches`.
- [ ] **Step 2:** FAIL → implement → green → workspace gates. Matrix F-082 → implemented.
- [ ] **Step 3: Commit** `M4: transcript search — debounced find bar, capped results, wrap navigation (F-082)`.

### Task 10: Playback wiring — audio UI, loop/follow/speed, video pane (F-083 GUI)

**Files:**
- Create: `crates/humm-gui/src/views/playback_controls.rs`
- Modify: `viewer.rs` (PlayerState, video state, drift log), `views/viewer.rs` (player row, current-segment frame, splitter), `humm.ftl`/`i18n.rs` (`viewer-loop-segment`, `viewer-follow-audio`, `viewer-speed`, `viewer-scroll-to-current`, `viewer-playback-controls`, tooltips — Buzz strings per spec §2.1), `tests/kittest_viewer.rs`, `docs/HUMM_SURFACE.md` (smoke hook)

**Interfaces:**
- Consumes: Task 5 (`decode_playback`, `VideoStreamer`, `select_frame` semantics), Task 6 `Player`.
- Produces on ViewerVm: `player: PlayerState { Loading | Ready(Player) | Failed(String) }` (decode on background thread at open; `Failed` renders disabled controls + tooltip naming the cause); `video: Option<VideoState { stream, texture, last_frame_ts }>`; `is_video` by extension set `{mp4 mov mkv avi m4v webm ogm wmv}` (spec §2.5); splitter with per-kind fraction persistence; playback-controls bar per spec §2.1 (auto-show on play in Timestamps mode; ⌘⌥P toggle; visibility persisted); loop toggle → `player.set_loop(selected segment)` + Buzz's re-enable/jump semantics; follow-audio toggle + position-driven row highlight (binary search — reuse `rows` sorted order); scroll-to-current (⌘G); speed combo presets `["0.5x","0.75x","1x","1.25x","1.5x","2x"]` + free text + `−/+` 0.05 steps + `%.2fx` echo + global `playback_rate` persist; scrubber (drag=live seek; release outside loop range ±2000 ms clears range); time label `HH:MM:SS / HH:MM:SS`; current-segment frame (segment text at position, 3-line scroll area); segment click → seek-if-outside + loop-range update; ⌘P/⌘⇧P wired; while playing `request_repaint_after(16ms)`; video: frame_for(audio position) → `ColorImage` → `TextureHandle` (audio clock master), seek propagates, `HUMM_SMOKE_AV_DRIFT_LOG=<path>` appends `audio_ms,frame_ts_ms` lines each frame when set.
- Pure helpers unit-testable: `speed_from_text("1.5x"|"2"|garbage) -> Option<f32>` (clamp), `time_label(pos, dur) -> String`, `segment_at(rows, pos) -> Option<usize>` (binary search, `[start,end)`), `scrub_release_clears_loop(pos, range) -> bool` (±2000 ms rule).

- [ ] **Step 1: Failing unit tests** for the pure helpers above + `loop_toggle_reenable_jumps_when_playing_outside_range` (state-machine level with MockOutput-backed Player) + `follow_audio_highlights_containing_segment` + `drift_log_writes_pairs_when_env_set` (temp file).
- [ ] **Step 2: kittest** (fake deps: decoder returns 1 s stereo silence, MockOutput, scripted FrameStream): `player_failed_state_disables_controls_with_tooltip`, `controls_bar_appears_on_play_in_timestamps_mode`, `speed_combo_echoes_clamped_value`.
- [ ] **Step 3:** FAIL → implement → green → workspace gates. Matrix F-083 → implemented (windowed A/V run pending Task 14). Update `docs/HUMM_SURFACE.md` §5 smoke hooks (`HUMM_SMOKE_AV_DRIFT_LOG`).
- [ ] **Step 4: Commit** `M4: synced playback — audio player UI, loop/follow/speed, video pane with audio-master clock (F-083)`.

### Task 11: Export menu (F-084)

**Files:**
- Create: `crates/humm-gui/src/views/export_menu.rs`
- Modify: `crates/humm-gui/src/import.rs` (`FileDialogs::save_file(&self, title: &str, default_path: &Path, filter_name: &str, extension: &str) -> Option<PathBuf>` + Rfd impl; test fake), `viewer.rs`, `views/viewer.rs`, `humm.ftl`/`i18n.rs` (`export-menu-entry = {format} - {kind}` shaped keys; toast `export-written`), `tests/kittest_viewer.rs`

**Interfaces:**
- Consumes: core `export::{write_format, output_file_path, TextKey, DEFAULT_EXPORT_TEMPLATE}`; Task 1 `set_last_export_folder`/`last_export_folder`.
- Produces: Export toolbar menu — 6 entries `{TXT,SRT,VTT} × {Text, Translation}` labeled `"SRT - Text"` etc., Translation entries visible iff `has_translations`; flow per spec §2.4 (default filename via template engine with the row's task/language/model columns — `model_display_columns` exists; default dir = `last_export_folder` else input-file dir; save dialog via seam; write via `write_format` from **in-memory rows** mapped to `Segment`; persist folder; success toast with path).

- [ ] **Step 1: Failing tests.** Unit: `default_export_path_uses_last_folder_then_input_dir` (temp store), `export_writes_via_shared_writer` (**snapshot-pinning acceptance**: for a fixture transcript, all 6 combos' bytes == direct `write_format` output — shared-code assertion), `translation_export_uses_translation_key`. kittest: `translation_entries_hidden_without_translations`, `export_entry_writes_file_and_toasts` (fake dialog returns temp path).
- [ ] **Step 2:** FAIL → implement → green → workspace gates. Matrix F-084 → implemented (snapshot half satisfied; gui-manual pending).
- [ ] **Step 3: Commit** `M4: viewer export menu — format × text/translation via shared M1 writers, per-transcript folder memory (F-084)`.

### Task 12: Resizer window (F-086 GUI)

**Files:**
- Create: `crates/humm-gui/src/views/resizer.rs`
- Modify: `crates/humm-gui/src/app.rs` (resizer viewports map + AppEvent handling: refresh/insert TasksVm row, close originating viewer + resizer), `viewer.rs` (Resize button opens it), `crates/humm-core/src/queue.rs` (ADR-018 scope doc note + enqueue-failure comment — ticket 6), `humm.ftl`/`i18n.rs` (resizer-* strings, Buzz-byte-exact labels per spec §2.6), `tests/kittest_viewer.rs`
- Settings: `viewer.resizer_create_new_transcript` (Task 7 added the field).

**Interfaces:**
- Consumes: Task 3 `equalize`/`extend_endings`, Task 4 `regroup_segments`/`default_algo`/`algo_from_env_or`, Task 1 `copy_transcription`/`mark_replaced_completed`/`replace_segments`/`update_progress_direct`.
- Produces: `ResizerVm { source: TranscriptionId, create_new: bool, extend_amount: String, target_chars: u32, merge_by_gap: (bool, String), split_by_punct: (bool, String), split_by_len: (bool, String), word_timestamps: bool, running: bool }`; `resizer::ui(...)`; section enablement per gate (`word_timestamps` from `load_task_spec` else `spec_from_columns`); Extend/Resize apply synchronously (fast); Merge runs on a `std::thread` (build words from **store** rows, regroup, then save) sending `AppEvent::TranscriptionsUpdated(new_or_same_id)`; save semantics per spec §2.6 (create-new: copy → [merge: progress 0.0 visible first] → mark_replaced_completed; in-place: replace_segments); on the app receiving the event: TasksVm refresh/insert from store + close resizer + originating viewer.
- `pub fn parse_extend_amount(s: &str) -> Millis` (unparseable → 200 ms — Buzz default fallback).

- [ ] **Step 1: Failing tests.** Unit: `parse_extend_amount_fallback`, `merge_gate_from_options_json_and_column_fallback` (temp store, both row shapes), `save_create_new_copies_then_completes` / `save_in_place_replaces` (temp store; assert row + segment outcomes incl. translations dropped), `regroup_string_built_matches_buzz_truth_table` (delegates to Task 4's `default_algo` — wiring assertion). kittest: `resize_button_disabled_with_word_timestamps` (+ inverse for merge), `resize_end_to_end_creates_new_completed_row` (seeded store → drive resize via seam → new row Completed with equalized segments; assert originating-viewer close flag).
- [ ] **Step 2:** FAIL → implement → green → workspace gates. Matrix F-086 → implemented (diff fixtures from Tasks 3/4 count toward `diff`; note inline).
- [ ] **Step 3: Commit** `M4: resizer window — extend/resize/merge with create-new-or-in-place save, viewer close-on-update (F-086)`.

### Task 13: Carried tickets — download mutex, TasksVm resort-skip, GUI minors

**Files:**
- Modify: `crates/humm-core/src/models/download.rs`, `crates/humm-gui/src/viewmodel.rs`, `crates/humm-gui/src/models_ui.rs`, `crates/humm-gui/src/main.rs`, kittest test files (sentinel paths → tempdirs)

- [ ] **Step 1: Per-artifact download mutex** (spec §8.1). Failing test `concurrent_ensure_of_same_artifact_downloads_once` (two threads × `ensure_artifact` same final path against the existing TestServer; server hit-count == 1 full download, both `Ok`; needs sandbox-lift like other TCP tests). Implement: process-wide `static ARTIFACT_LOCKS: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>`; guard held across partial→verify→promote; second comer re-checks the sentinel under the lock.
- [ ] **Step 2: `TasksVm::mutate` resort-skip** (spec §8.2). Failing tests `mutate_skips_resort_when_sort_key_unchanged` (sort by DateAdded; progress mutation keeps a deliberately-shuffled-stable order observable via a probe — implement by comparing the active key's projection before/after `f`) and `mutate_resorts_when_key_changes`. Implement in `mutate` (`viewmodel.rs:527`).
- [ ] **Step 3: Minors.** models_ui custom-URL persists on `lost_focus` only (kittest/unit per current coverage pattern); `main.rs` argv import filters leading-`-` args (unit test on the extraction fn); kittest sentinel paths (`/unused-settings-path`) → tempdirs.
- [ ] **Step 4:** Workspace gates. **Commit** `M4: carried tickets — per-artifact download mutex, TasksVm resort-skip, GUI minors (F-043, F-003)`.

### Task 14: Benches, A/V acceptance, gui-manual v2, matrix + report

**Files:**
- Create: `crates/humm-gui/benches/viewer_scroll.rs`, `docs/gui-manual/M4.md`, `docs/benchmarks/M4.md`
- Modify: root/humm-gui `Cargo.toml` (bench decl), `docs/analysis/PARITY_MATRIX.md`, `docs/design/ARCHITECTURE.md` (§2/§3/§7 as-built amendments, spec §10), `docs/design/UX_SPEC.md` (J-6 shipped ✚s), `docs/design/DECISIONS.md` (**ADR-019** playback architecture, **ADR-020** resize/merge port — full context/options/decision/validation text from spec §5/§9), `docs/HUMM_SURFACE.md` (env vars, `[viewer]` settings, schema v2)

- [ ] **Step 1: `viewer_scroll` bench** — clone `table_scroll.rs` pattern over `viewer_table::ui` with 100k `SegmentRow`s; record p95 frame time; gate ≥60 fps.
- [ ] **Step 2: Transcription-path guard** — run `cargo bench -p humm-whisper e2e` (sandbox-lifted, real model) and compare against `docs/benchmarks/M3.md`; regression = stop and fix before proceeding (mission rule).
- [ ] **Step 3: A/V drift acceptance** — generate a 10-min fixture (`ffmpeg -f lavfi -i testsrc2=duration=600:rate=25 -f lavfi -i sine=frequency=440:duration=600 -shortest out.mp4`); windowed run on the unsandboxed machine with `HUMM_SMOKE_AV_DRIFT_LOG` + `HUMM_SMOKE_AUTO_CLOSE_FRAMES` hooks (M3 smoke pattern; add a `HUMM_SMOKE_OPEN_VIEWER=<file>` hook if needed — env-gated, documented in HUMM_SURFACE §5); parse log: p95 |audio − frame_ts| ≤ 50 ms over the full run. Record numbers in `docs/benchmarks/M4.md`.
- [ ] **Step 4: gui-manual v2** — write `docs/gui-manual/M4.md` covering every F-080..F-086 gui-manual clause (`[kittest: <test>]` tags where automated; `[manual]` for pointer/windowed items), execute the windowed run (open-from-table, mode switches, editing round-trip, search, loop/follow/speed audible+visible, video sync spot-check, export dialog, resize/merge flows, multi-viewer + focus-dedupe), record the execution record section (M3 format), fold in M3's deferred pointer-interactive list where M4 surfaces overlap.
- [ ] **Step 5: Matrix + report.** PARITY_MATRIX final sweep: F-080..F-084, F-086 to their earned statuses with full evidence notes (verified only where the stated method fully ran — expected: F-081/F-082 verified (unit+kittest+manual), F-084 verified (snapshot+manual), F-080/F-083 implemented-or-verified per manual outcome, F-086 implemented + diff note (or verified if the Python harness ran end-to-end)); F-003/F-004 note the unlock. Milestone report (final reply + `docs/benchmarks/M4.md` cross-ref): matrix status table, bench numbers (viewer_scroll, e2e guard, A/V p95), deviations ledger → ADR refs, analysis corrections made, **F-045 real-differential still environment-blocked** restated.
- [ ] **Step 6:** Workspace gates. **Commit** `M4: benches, A/V acceptance, gui-manual v2, parity matrix + milestone report (F-080..F-086)`.

---

## Self-review notes (against the M4 spec)

- Spec coverage: §1 architecture → Tasks 5-7; §2.1 → 7/10; §2.2 → 2/8; §2.3 → 2/9; §2.4 → 11; §2.5 → 5/6/10; §2.6 → 3/4/12; §3 → 7; §4 → 1; §5 → 3/4/5/6; §6 → 7; §7 shortcuts land with their owning views (7/8/9/10); §8 tickets → 13 (+6 comment in 12); §9/§10 ledger+docs → 14 (ADR-019/020). No gaps found.
- Type consistency: `SegmentRow`/`ViewerDeps`/`PlayerState`/`StreamSpec` names match across tasks; `timestamp_edit` consumed in Task 8 as defined in Task 2; `default_algo` in 12 as defined in 4.
- Placeholders: none — every step names concrete tests/strings/commands; behavioral detail is pinned by spec §-references + Buzz `file:line` (the repo's established plan convention; implementers must read the spec §s cited by their task).
