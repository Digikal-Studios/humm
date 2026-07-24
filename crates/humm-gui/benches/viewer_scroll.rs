//! 100 000-row transcript-editor-table scroll benchmark (M4 Task 14, F-081/
//! F-080; spec §11 acceptance gate: "viewer table ≥60 fps at 100k segments
//! (`viewer_scroll` criterion bench, `table_scroll` pattern)").
//!
//! This is the editor-table sibling of `benches/table_scroll.rs` (the M3
//! task-table scroll bench): it drives the REAL
//! `humm_gui::views::viewer_table::ui` — the segment editor table an open
//! transcript renders in Timestamps mode — over a synthetic 100 000-row
//! [`ViewerVm`], inside a headless `egui::Context`, with injected
//! `egui::Event::MouseWheel` scroll gestures sweeping the full content range
//! repeatedly. Each criterion-measured unit is exactly one frame (one
//! `Context::run_ui` call), matching how `eframe` drives the viewer viewport's
//! render closure for real.
//!
//! ## Why a separate bench from `table_scroll`
//! The two tables are different production code: the task table
//! (`task_table::ui`) renders seven fixed-height 20 px rows of task metadata;
//! the viewer editor table (`viewer_table::ui`) renders 4-text-line-tall rows
//! of `Start | End | Text | Translation` cells, each of which is an
//! *interactive* label (`interactive_cell`) rather than a plain one, and can
//! turn into an inline `TextEdit` when edited. A 100k-row scroll gate for the
//! task table says nothing about the editor table's per-row cost — so this
//! bench measures the surface the M4 viewer actually adds. Both rely on
//! `egui_extras::TableBuilder`'s row virtualization (only the ~visible rows
//! are laid out per frame); this bench confirms that holds for the heavier
//! editor rows too.
//!
//! ## Building the 100k rows
//! [`ViewerVm::open`] normally reads through a real `SqliteStore`
//! (`get_segment_rows`) after a `replace_segments` populates it — 100 000
//! rows through a transaction just to synthesize bench data, with nothing to
//! do with table *rendering*. [`ViewerVm::for_bench`] (its `#[doc(hidden)]`
//! bench-only constructor, mirroring `TasksVm::for_bench` from the M3 bench)
//! takes a pre-built `Vec<SegmentRow>` directly — every [`SegmentRow`] field
//! is `pub`, so this file builds 100 000 of them and hands them over. It
//! still exercises the real render/edit/selection code; only the
//! data-population mechanism differs from a real run. The player is left in
//! `PlayerState::Loading` (no background decode thread is spawned — this bench
//! calls only `viewer_table::ui`, which never touches the player).
//!
//! ## Driving scroll without touching production code
//! Identical technique to `table_scroll.rs`: `viewer_table::ui` builds its own
//! internal `egui_extras::TableBuilder` (id_salt [`TABLE_ID_SALT`]) and
//! discards the `ScrollAreaOutput`, so scroll is driven exactly as a real user
//! would — `egui::Event::MouseWheel` events injected through `RawInput` with
//! the pointer over the table. `egui_extras`'s internal `ScrollArea` owns the
//! resulting offset state, persisted via `egui::Context`'s `data` map. The
//! `Id`-reconstruction below (`scroll_area_state_id`) replicates the exact
//! double-`IdSalt`-prehash `egui_extras`/`egui` perform, read from the
//! vendored source — see `table_scroll.rs`'s module doc for the full
//! walk-through of that construction (and the dead ends it documents); this
//! file reuses it verbatim, only swapping the table's id_salt string.
//!
//! ## The two numbers this file reports
//! 1. Criterion's own mean/median for `viewer_scroll/viewer_scroll_100k`.
//! 2. A custom p95, printed to stdout by [`print_manual_p95`]: criterion
//!    doesn't report percentiles, and the milestone's acceptance criterion is
//!    p95 frame CPU < 16.6 ms (≈60 fps), so this file separately times 300
//!    frames by hand. Not asserted on here (a bench must never fail the build
//!    over a perf number it can only measure, not fix) — read from stdout by
//!    `docs/benchmarks/M4.md`.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};

use egui::{Event, Id, Modifiers, MouseWheelUnit, Pos2, Rect, TouchPhase, Vec2, vec2};

use humm_core::boundary::{
    AudioDecoder, AudioOutput, FillCallback, FrameStream, MediaInfo, MediaProber, OutputStream,
    Pcm, PlaybackPcm, StreamSpec, VideoStreamInfo, VideoStreamer,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::Millis;
use humm_core::error::{AudioError, MediaError};
use humm_core::secrets::MemorySecretStore;
use humm_gui::import::FileDialogs;
use humm_gui::translate_seam::ScriptedTranslateSpawner;
use humm_gui::viewer::{ViewerDeps, ViewerVm};
use humm_gui::views::viewer_table;
use humm_store::{SegmentRow, SqliteStore};

/// Duplicated from `views/viewer_table.rs`'s private `TABLE_ID_SALT` — needed
/// only so the sanity check can read back the exact persisted
/// `egui::containers::scroll_area::State` the real editor table's `ScrollArea`
/// stores (same rationale as `table_scroll.rs`'s duplicate of the task
/// table's salt; one stable literal is cheaper and less invasive than
/// exporting it from production for a bench-only need).
const TABLE_ID_SALT: &str = "humm-viewer-editor-table";

const ROW_COUNT: usize = 100_000;
/// Viewport size for the headless `egui::Context` — the viewer's own default
/// window size (`ViewerVm` geometry default 1000×800, spec §1.1 / §6), so the
/// bench renders at the size a real user actually sees.
const VIEWPORT: (f32, f32) = (1000.0, 800.0);
/// Number of wheel-scroll frames to sweep the entire content range once before
/// snapping back to the top and repeating.
const SCROLL_STEPS_PER_SWEEP: usize = 240;

/// Builds `count` synthetic segment rows with realistic timings and text —
/// contiguous non-overlapping 2 s segments, sentence-like text of varying
/// length, and (for a quarter of the rows) a non-blank translation so the
/// Translation column is present (its render cost is part of the worst case).
fn synthetic_rows(count: usize) -> Vec<SegmentRow> {
    let phrases = [
        "the quick brown fox jumps over the lazy dog",
        "and then she said something entirely unexpected to the room",
        "we should probably revisit this decision in the next quarter",
        "hello",
        "a much longer stretch of dialogue that runs on and on well past the point \
         where a subtitle line would normally wrap, exercising the text column's \
         multi-line word-wrap layout at its widest",
        "okay",
    ];
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let start = (i as i64) * 2_000;
        let text = phrases[i % phrases.len()];
        let has_tr = i % 4 == 0;
        rows.push(SegmentRow {
            id: i as i64 + 1,
            start: Millis(start),
            end: Millis(start + 1_900),
            text: format!("{text} (segment {i})"),
            translation: if has_tr {
                format!("translation for segment {i}")
            } else {
                String::new()
            },
        });
    }
    rows
}

// -- inert fake adapters: `viewer_table::ui` only touches `deps.store` when an
//    edit *commits*, which a pure scroll bench never triggers, and never
//    touches the decoder/prober/streamer/output/dialogs at all. They exist
//    only so a `ViewerDeps` can be constructed; every method is unreachable on
//    this bench's path. (Mirrors `tests/kittest_viewer.rs`'s fakes, minus the
//    behavior those tests actually need.) --

struct InertDecoder;
impl AudioDecoder for InertDecoder {
    fn decode(&self, _path: &Path, _cancel: &CancelToken) -> Result<Pcm, MediaError> {
        unreachable!("viewer_scroll never decodes")
    }
    fn decode_playback(
        &self,
        _path: &Path,
        _cancel: &CancelToken,
    ) -> Result<PlaybackPcm, MediaError> {
        unreachable!("viewer_scroll never decodes for playback")
    }
}

struct InertProber;
impl MediaProber for InertProber {
    fn probe(&self, _path: &Path) -> Result<MediaInfo, MediaError> {
        unreachable!("viewer_scroll never probes")
    }
}

struct InertStreamer;
impl VideoStreamer for InertStreamer {
    fn open(
        &self,
        _path: &Path,
        _start: Millis,
        _info: &VideoStreamInfo,
    ) -> Result<Box<dyn FrameStream>, MediaError> {
        unreachable!("viewer_scroll never streams video")
    }
}

struct InertOutput;
impl AudioOutput for InertOutput {
    fn open(
        &self,
        _spec: StreamSpec,
        _fill: FillCallback,
    ) -> Result<Box<dyn OutputStream>, AudioError> {
        unreachable!("viewer_scroll never opens an audio device")
    }
}

struct InertDialogs;
impl FileDialogs for InertDialogs {
    fn pick_files(&self, _start_dir: Option<&Path>) -> Option<Vec<PathBuf>> {
        None
    }
    fn pick_folder(&self, _start_dir: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn save_file(
        &self,
        _title: &str,
        _default_path: &Path,
        _filter_name: &str,
        _extension: &str,
    ) -> Option<PathBuf> {
        None
    }
}

/// Builds a `ViewerDeps` over a real (empty) temp-dir store and the inert
/// adapters above. The store is never queried on the scroll path (only a
/// committed edit reads/writes it), but `ViewerDeps` requires a real
/// `Arc<SqliteStore>`; the returned `TempDir` guard keeps its file alive for
/// the bench's lifetime.
fn inert_deps() -> (ViewerDeps, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(SqliteStore::open(&dir.path().join("humm.db")).expect("open store"));
    let deps = ViewerDeps {
        store,
        decoder: Arc::new(InertDecoder),
        prober: Arc::new(InertProber),
        streamer: Arc::new(InertStreamer),
        audio_output: Arc::new(InertOutput),
        dialogs: Arc::new(InertDialogs),
        secrets: Arc::new(MemorySecretStore::new()),
        translate: Arc::new(ScriptedTranslateSpawner::identity()),
    };
    (deps, dir)
}

/// Probes the row height `viewer_table::ui` renders at — it's dynamic
/// (`ui.text_style_height(&TextStyle::Body) * 4.0`, a private literal inside
/// that function, not a constant), so this file measures it once against a
/// headless context rather than hard-coding a value that could drift from
/// egui's default font metrics.
fn probe_row_height(ctx: &egui::Context) -> f32 {
    let captured = Cell::new(0.0_f32);
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        captured.set(ui.text_style_height(&egui::TextStyle::Body) * 4.0);
    });
    let h = captured.get();
    assert!(h > 0.0, "row height probe returned {h}");
    h
}

/// One rendered frame: `Context::run_ui` over a `CentralPanel` containing the
/// real `viewer_table::ui`, with a `MouseWheel` event of `delta_y` points
/// (negative scrolls down) and the pointer parked at the viewport center.
/// Returns the `CentralPanel`'s `ui.id()` (for the scroll-state id).
fn drive_one_frame(ctx: &egui::Context, vm: &mut ViewerVm, deps: &ViewerDeps, delta_y: f32) -> Id {
    let screen_rect = Rect::from_min_size(Pos2::ZERO, vec2(VIEWPORT.0, VIEWPORT.1));
    let pointer = screen_rect.center();
    let raw_input = egui::RawInput {
        screen_rect: Some(screen_rect),
        events: vec![
            Event::PointerMoved(pointer),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: Vec2::new(0.0, delta_y),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ],
        ..egui::RawInput::default()
    };

    let central_ui_id = Cell::new(None);
    let output = ctx.run_ui(raw_input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            central_ui_id.set(Some(ui.id()));
            let applied = viewer_table::ui(ui, vm, deps);
            std::hint::black_box(&applied);
        });
    });
    std::hint::black_box(output);

    central_ui_id
        .get()
        .expect("CentralPanel always runs its closure")
}

/// Reconstructs the `Id` `egui_extras::TableBuilder::body` persists its
/// internal `ScrollArea` state under — the exact double-`IdSalt`-prehash
/// construction documented at length in `table_scroll.rs::scroll_area_state_id`
/// (read from the vendored `egui_extras`/`egui` source). Only the table
/// id_salt string differs.
fn scroll_area_state_id(central_ui_id: Id) -> Id {
    let table_salt = egui::IdSalt::new(TABLE_ID_SALT);
    let table_state_id = central_ui_id.with(table_salt);
    let scroll_salt_seed = table_state_id.with("__scroll_area");
    let scroll_salt = egui::IdSalt::new(scroll_salt_seed);
    central_ui_id.with(scroll_salt)
}

/// Reads back the real `ScrollArea`'s current vertical offset (points), or
/// `None` before the table has ever scrolled.
fn current_scroll_offset(ctx: &egui::Context, central_ui_id: Id) -> Option<f32> {
    let id = scroll_area_state_id(central_ui_id);
    egui::containers::scroll_area::State::load(ctx, id).map(|state| state.offset.y)
}

/// Runs a handful of frames with the real production `viewer_table::ui` and
/// asserts the table's rendered row range genuinely advances (read back via
/// the real `ScrollArea`'s persisted offset). Panics rather than reporting
/// misleading numbers if scrolling isn't actually happening — the same "sanity
/// check run once before measurement" `table_scroll.rs` performs.
fn sanity_check_scroll_advances_rendered_range(rows: &[SegmentRow], row_height: f32) {
    let ctx = egui::Context::default();
    let (deps, _guard) = inert_deps();
    let mut vm = ViewerVm::for_bench(rows.to_vec());

    let per_step = (ROW_COUNT as f32 * row_height) / SCROLL_STEPS_PER_SWEEP as f32;

    let central_id = drive_one_frame(&ctx, &mut vm, &deps, 0.0);
    let start_offset = current_scroll_offset(&ctx, central_id).unwrap_or(0.0);
    let start_row = (start_offset / row_height).floor() as i64;

    let mut last_offset = start_offset;
    for _ in 0..30 {
        let central_id = drive_one_frame(&ctx, &mut vm, &deps, -per_step);
        last_offset = current_scroll_offset(&ctx, central_id)
            .expect("ScrollArea must have persisted state after a scroll gesture");
    }
    let end_row = (last_offset / row_height).floor() as i64;

    assert!(
        last_offset > start_offset,
        "sanity check failed: scroll offset did not advance (start={start_offset}, \
         end={last_offset}) — viewer_scroll bench numbers would be meaningless, refusing \
         to report them"
    );
    assert!(
        end_row - start_row > 100,
        "sanity check failed: rendered row range only moved from ~row {start_row} to ~row \
         {end_row} over 30 fling frames — far less than expected, table virtualization or the \
         scroll-id reconstruction may be broken"
    );
    println!(
        "sanity check OK: 30 fling frames moved the rendered range from ~row {start_row} to \
         ~row {end_row} (offset {start_offset:.0}pt -> {last_offset:.0}pt, row_height \
         {row_height:.1}pt)"
    );
}

/// The per-frame wheel delta for sweep step `i`: a steady downward fling,
/// snapping back to the top once per sweep.
fn sweep_delta(i: usize, per_step: f32, row_height: f32) -> f32 {
    if i % SCROLL_STEPS_PER_SWEEP == SCROLL_STEPS_PER_SWEEP - 1 {
        (ROW_COUNT as f32) * row_height // snap back to top (ScrollArea clamps to 0)
    } else {
        -per_step
    }
}

/// Times 300 frames by hand (no criterion) and prints mean + p95 — the
/// milestone acceptance criterion is stated as a p95, which criterion doesn't
/// report.
fn print_manual_p95(rows: &[SegmentRow], row_height: f32) {
    let ctx = egui::Context::default();
    let (deps, _guard) = inert_deps();
    let mut vm = ViewerVm::for_bench(rows.to_vec());
    let per_step = (ROW_COUNT as f32 * row_height) / SCROLL_STEPS_PER_SWEEP as f32;

    const SAMPLES: usize = 300;
    let mut durations = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let delta_y = sweep_delta(i, per_step, row_height);
        let start = Instant::now();
        drive_one_frame(&ctx, &mut vm, &deps, delta_y);
        durations.push(start.elapsed());
    }

    durations.sort();
    let mean: Duration = durations.iter().sum::<Duration>() / durations.len() as u32;
    let p95_idx = ((durations.len() as f32) * 0.95).ceil() as usize - 1;
    let p95 = durations[p95_idx.min(durations.len() - 1)];
    let max = *durations.last().expect("SAMPLES > 0");

    println!(
        "viewer_scroll_100k manual sampling ({SAMPLES} frames, no criterion): mean={mean:?} \
         p95={p95:?} max={max:?} (milestone target: p95 < 16.6ms)"
    );
    println!(
        "viewer_scroll_100k acceptance verdict: {}",
        if p95 < Duration::from_micros(16_600) {
            "PASS (p95 < 16.6ms)"
        } else {
            "FAIL (p95 >= 16.6ms) — see docs/benchmarks/M4.md for the milestone report"
        }
    );
}

fn bench_viewer_scroll(c: &mut Criterion) {
    let rows = synthetic_rows(ROW_COUNT);

    let probe_ctx = egui::Context::default();
    let row_height = probe_row_height(&probe_ctx);

    sanity_check_scroll_advances_rendered_range(&rows, row_height);

    let ctx = egui::Context::default();
    let (deps, _guard) = inert_deps();
    let mut vm = ViewerVm::for_bench(rows.clone());
    let per_step = (ROW_COUNT as f32 * row_height) / SCROLL_STEPS_PER_SWEEP as f32;
    let step = Cell::new(0usize);

    let mut group = c.benchmark_group("viewer_scroll");
    group.bench_function("viewer_scroll_100k", |b| {
        b.iter(|| {
            let i = step.get();
            step.set(i + 1);
            let delta_y = sweep_delta(i, per_step, row_height);
            drive_one_frame(&ctx, &mut vm, &deps, delta_y);
        });
    });
    group.finish();

    print_manual_p95(&rows, row_height);
}

criterion_group!(benches, bench_viewer_scroll);
criterion_main!(benches);
