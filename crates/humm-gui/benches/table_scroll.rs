//! 100 000-row task-table scroll benchmark (M3 Task 12, ADR-006's validation
//! plan: "M3 exit criteria include a 100k-segment table scroll benchmark
//! (≥ 60 fps sustained)").
//!
//! Drives the REAL `humm_gui::views::task_table::ui` — not a stand-in — over
//! a synthetic 100 000-row `TasksVm`, inside a headless `egui::Context`, with
//! injected `egui::Event::MouseWheel` scroll gestures sweeping the full
//! content range repeatedly. Each criterion-measured unit is exactly one
//! frame (one `Context::run_ui` call, matching how `eframe` drives
//! `HummApp::ui` for real).
//!
//! ## Building the 100k rows
//! `TasksVm`'s constructor normally reads through a real `SqliteStore`
//! (`TasksVm::load`), which would mean 100 000 real `create_task` +
//! `load_task_spec` round trips just to synthesize bench data — a real cost
//! with nothing to do with table *rendering*. `TasksVm::for_bench` (see its
//! doc comment in `viewmodel.rs`, `#[doc(hidden)]`) is a bench-only
//! constructor added for exactly this: every [`RowVm`] field is already
//! `pub`, so this file builds 100 000 of them directly and hands them to
//! `for_bench`, which is the cheapest *honest* path — it still exercises the
//! real sort/select/render code, only the data-population mechanism differs
//! from a real run.
//!
//! ## Driving scroll without touching production code
//! `task_table::ui` builds its own internal `egui_extras::TableBuilder` and
//! discards the `ScrollAreaOutput` it returns, so there is no plumbing to
//! *set* a scroll offset directly from outside it (and none was added here —
//! that would mean changing production code just to make it easier to
//! bench). Instead this file drives scroll exactly as a real user would:
//! `egui::Event::MouseWheel` events injected through `RawInput`, with the
//! pointer hovering over the table, exactly like `eframe`'s real winit
//! backend would deliver a trackpad/wheel gesture. `egui_extras`'s
//! `ScrollArea` (internal to `TableBuilder::body`) owns the resulting scroll
//! state the same way it does in the real app — persisted via
//! `egui::Context`'s `data` map, keyed off `ui.id().with("humm-task-table")`
//! (task_table.rs's own `TABLE_ID_SALT`, duplicated here as
//! [`TABLE_ID_SALT`] — this file's one deliberate coupling to that private
//! constant, needed only so the sanity check below can read back the exact
//! same persisted `egui::containers::scroll_area::State` egui_extras stores,
//! per its own `Table::body`/`ScrollArea::id_salt` construction
//! (`state_id.with("__scroll_area")`, vendored source read directly rather
//! than assumed) — the same technique task_table.rs's own module doc already
//! relies on to explain why column widths survive a restart.
//!
//! Per frame this file injects a large single wheel delta (content height ÷
//! [`SCROLL_STEPS_PER_SWEEP`]) rather than a realistic small trackpad notch —
//! deliberately: the goal is *coverage* (every measured frame should
//! plausibly land at a different scroll position, so the criterion samples
//! aren't all measuring the same near-top 30 rows), not a literal physical
//! gesture. [`sanity_check_scroll_advances_rendered_range`] proves this
//! actually moves the rendered row range before any measurement is trusted;
//! if it doesn't, this file panics loudly rather than silently reporting
//! numbers for a scroll that never happened.
//!
//! ## The two numbers this file reports
//! 1. Criterion's own mean/median/std-dev for `table_scroll/table_scroll_100k`
//!    (`cargo bench -p humm-gui -- table_scroll`, normal criterion report).
//! 2. A custom p95, printed to stdout by [`print_manual_p95`]: criterion
//!    doesn't report percentiles directly, and the milestone's acceptance
//!    criterion (`docs/design/DECISIONS.md` ADR-006) is p95 frame CPU
//!    < 16.6 ms (≈60 fps), so this file separately times 300 frames by hand
//!    (`std::time::Instant`, no criterion involved) after the criterion group
//!    finishes, sorts them, and prints mean/p95. This is *not* a criterion
//!    benchmark — it runs once, as a plain function call, and its numbers are
//!    read from `cargo bench`'s stdout by the milestone report, not asserted
//!    on here (a bench must never fail the build over a perf regression this
//!    file can only measure, not fix).

use std::cell::Cell;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};

use egui::{Event, Id, Modifiers, MouseWheelUnit, Pos2, Rect, TouchPhase, Vec2, vec2};

use humm_core::domain::{Status, TranscriptionId};
use humm_gui::viewmodel::{Phase, RowVm, TasksVm};
use humm_gui::views::task_table::{self, TableLayout};

/// Floor of `task_table::task_table_row_height` at the default body size
/// (`(line_height + 4).max(20)`). The bench renders with egui's default
/// font, where that floor is the row height. A larger Preferences font
/// grows the real rows; this constant is the default-font measurement.
const ROW_HEIGHT: f32 = 20.0;

/// Duplicated from `views/task_table.rs`'s private `TABLE_ID_SALT` — see the
/// module doc's "Driving scroll without touching production code" section
/// for why this file needs it (reading back the persisted
/// `egui::containers::scroll_area::State` the real table's `ScrollArea`
/// stores) and why duplicating one stable literal was judged cheaper and
/// less invasive than exporting it from production code for a bench-only
/// need.
const TABLE_ID_SALT: &str = "humm-task-table";

const ROW_COUNT: usize = 100_000;
/// Viewport size for the headless `egui::Context` — the design spec's own
/// default main-window size (`main.rs`'s `ViewportBuilder::default_size`,
/// F-001), so the bench renders at the size a real user actually sees.
const VIEWPORT: (f32, f32) = (1240.0, 600.0);
/// Number of wheel-scroll frames to sweep the *entire* content range once
/// before jumping back to the top and repeating — chosen so a single sweep
/// comfortably exceeds the brief's "≥120 frames" floor. Criterion calls its
/// benchmarked closure far more than this per run (typically thousands of
/// times, auto-calibrated), so in practice many full sweeps happen across
/// one `cargo bench` invocation.
const SCROLL_STEPS_PER_SWEEP: usize = 240;

/// Builds `count` synthetic rows with mixed statuses and realistic-looking
/// strings — every [`Status`] variant appears (cycled), file/URL inputs
/// alternate, and text lengths vary the way a real mixed history would
/// (short filenames, long Hugging Face repo ids, occasional notes/errors).
fn synthetic_rows(count: usize) -> Vec<RowVm> {
    let statuses = [
        Status::Queued,
        Status::InProgress,
        Status::Completed,
        Status::Failed,
        Status::Canceled,
        Status::Skipped,
    ];
    let models = [
        "Whisper.cpp (small)",
        "Whisper.cpp (medium)",
        "Whisper.cpp (large-v3-turbo)",
        "Hugging Face (openai/whisper-large-v3-turbo)",
        "Hugging Face (distil-whisper/distil-large-v3)",
    ];
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let status = statuses[i % statuses.len()];
        let is_url = i % 5 == 0;
        let base_minute = (i % 500_000) as i64;
        let time_queued = format!(
            "2026-{:02}-{:02}T{:02}:{:02}:{:02}.000000",
            1 + (i / 40_000) % 12,
            1 + (i / 1_400) % 28,
            (base_minute / 60) % 24,
            base_minute % 60,
            i as i64 % 60
        );
        let (progress, phase) = match status {
            Status::InProgress if i % 3 == 0 => (0.42, Phase::Model(Some(0.3))),
            Status::InProgress if i % 3 == 1 => (0.65, Phase::Media(0.5)),
            Status::InProgress => (0.18, Phase::None),
            _ => (0.0, Phase::None),
        };
        rows.push(RowVm {
            id: TranscriptionId::new(),
            display_name: if is_url {
                format!("https://example.com/episode-{i:05}.mp3")
            } else {
                format!("recording_{i:05}_interview_segment.wav")
            },
            model_text: models[i % models.len()].to_string(),
            task_text: if i % 4 == 0 {
                "Translate to English".to_string()
            } else {
                "Transcribe".to_string()
            },
            status,
            progress,
            phase,
            error: matches!(status, Status::Failed)
                .then(|| format!("ffmpeg exited with status 1 (row {i})")),
            time_queued,
            time_started: (!matches!(status, Status::Queued))
                .then(|| "2026-01-01T00:00:01.000000".to_string()),
            time_ended: matches!(
                status,
                Status::Completed | Status::Failed | Status::Canceled | Status::Skipped
            )
            .then(|| "2026-01-01T00:03:45.000000".to_string()),
            notes: (i % 11 == 0).then(|| format!("Reviewed by QA on batch {}", i / 11)),
            file: (!is_url)
                .then(|| std::path::PathBuf::from(format!("/data/audio/recording_{i:05}.wav"))),
            url: is_url.then(|| format!("https://example.com/episode-{i:05}.mp3")),
        });
    }
    rows
}

/// One rendered frame: `Context::run_ui` over a `CentralPanel` containing the
/// real `task_table::ui`, with a `MouseWheel` event of `delta_y` points
/// (negative scrolls down through later rows) and the pointer parked at the
/// viewport center so the wheel event lands on the table. Returns the
/// `CentralPanel`'s own `ui.id()` (needed by the sanity check to compute the
/// scroll area's persisted-state id; unused by the criterion loop, which
/// only cares about wall time).
fn drive_one_frame(
    ctx: &egui::Context,
    vm: &mut TasksVm,
    layout: &mut TableLayout,
    delta_y: f32,
) -> Id {
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
    let mut actions = Vec::new();
    let output = ctx.run_ui(raw_input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            central_ui_id.set(Some(ui.id()));
            task_table::ui(ui, vm, layout, &mut actions, false);
        });
    });
    std::hint::black_box(output);
    std::hint::black_box(&actions);

    central_ui_id
        .get()
        .expect("CentralPanel always runs its closure")
}

/// Reconstructs the exact `Id` `egui_extras::TableBuilder::body` persists its
/// internal `ScrollArea` state under, per the vendored source
/// (`egui_extras-0.35.0/src/table.rs`):
///
/// 1. `TableBuilder::id_salt(TABLE_ID_SALT)` (called by `task_table::ui`)
///    pre-hashes the literal string into an `egui::IdSalt` *before* storing
///    it (`self.id_salt = IdSalt::new(id_salt)`) — the first thing that
///    tripped this file up: naively `.with()`-ing the raw `&str` at the
///    outer level, skipping this pre-hash, produces a *different* `Id` than
///    production actually uses, since `IdSalt::new` hashes a `&str` and an
///    already-computed `IdSalt` (a `NonZeroU64` newtype) very differently
///    even for "the same" logical salt.
/// 2. `Table::header` computes `state_id = ui.id().with(id_salt)`, `id_salt`
///    here being that pre-hashed `IdSalt` from step 1.
/// 3. `Table::body` builds its internal `ScrollArea` with
///    `.id_salt(state_id.with("__scroll_area"))` (a plain `Id::with(&str)`
///    call — no pre-hash needed for *this* `.with()`, since nothing upstream
///    of it wrapped its salt argument in `IdSalt` yet).
/// 4. `ScrollArea::id_salt`'s own setter (`egui-0.35.0/src/containers/
///    scroll_area.rs`) is `self.id_salt = Some(IdSalt::new(id_salt))` — the
///    *second* pre-hash this file has to replicate explicitly (easy to miss:
///    `Id::with` always re-hashes whatever's handed to it via its own
///    internal `IdSalt::new(&salt)` regardless of the argument's type, so
///    skipping this step doesn't fail to compile — it just silently produces
///    a *different, wrong* `Id`, which is exactly what happened on the first
///    attempt at this function: `state_count=1` but `State::load` at the
///    naively-derived id came back `None`).
/// 5. `ScrollArea::show` resolves its final `Id` via `Ui::make_persistent_id`,
///    which is `self.id.with(id_salt)` (`egui-0.35.0/src/ui.rs`) — using the
///    *same* `ui` as steps 1–2, i.e. the `CentralPanel`'s child `Ui`
///    (`central_ui_id` here), and `id_salt` being the already-pre-hashed
///    value from step 4.
fn scroll_area_state_id(central_ui_id: Id) -> Id {
    let table_salt = egui::IdSalt::new(TABLE_ID_SALT); // step 1: TableBuilder::id_salt's pre-hash
    let table_state_id = central_ui_id.with(table_salt); // step 2: Table::header's `ui.id().with(id_salt)`
    let scroll_salt_seed = table_state_id.with("__scroll_area"); // step 3
    let scroll_salt = egui::IdSalt::new(scroll_salt_seed); // step 4: ScrollArea::id_salt's pre-hash
    central_ui_id.with(scroll_salt) // step 5: Ui::make_persistent_id
}

/// Reads back the real `ScrollArea`'s current vertical offset, in points, via
/// [`scroll_area_state_id`]. `None` before the table has ever scrolled (no
/// persisted state yet).
fn current_scroll_offset(ctx: &egui::Context, central_ui_id: Id) -> Option<f32> {
    let id = scroll_area_state_id(central_ui_id);
    egui::containers::scroll_area::State::load(ctx, id).map(|state| state.offset.y)
}

/// Runs a handful of frames with the real production `task_table::ui` and
/// asserts the table's *rendered row range* — read back via the real
/// `ScrollArea`'s persisted offset, converted to an approximate first-visible
/// row index — genuinely advances. This is the "#[test]-style sanity check
/// … run once before measurement" the bench brief calls for: it is a plain
/// function call (not a `#[test]`, which would never run for a
/// `harness = false` bench target — cargo only compiles `#[cfg(test)]` items
/// when it passes `--test`, which `harness = false` targets never receive),
/// invoked once from [`main`] before the criterion group runs, and it panics
/// (rather than silently reporting misleading numbers) if scrolling isn't
/// actually happening.
fn sanity_check_scroll_advances_rendered_range(rows: &[RowVm]) {
    let ctx = egui::Context::default();
    let mut vm = TasksVm::for_bench(rows.to_vec());
    let mut layout = TableLayout::default();

    let per_step = (ROW_COUNT as f32 * ROW_HEIGHT) / SCROLL_STEPS_PER_SWEEP as f32;

    // Frame 0: establish the ScrollArea's persisted state at offset ~0.
    let central_id = drive_one_frame(&ctx, &mut vm, &mut layout, 0.0);
    let start_offset = current_scroll_offset(&ctx, central_id).unwrap_or(0.0);
    let start_row = (start_offset / ROW_HEIGHT).floor() as i64;

    // 30 frames of a sustained downward gesture (a fling, not a single
    // notch) — comfortably enough to move well past the initially-visible
    // rows even accounting for any one-frame smoothing/animation lag.
    let mut last_offset = start_offset;
    for _ in 0..30 {
        let central_id = drive_one_frame(&ctx, &mut vm, &mut layout, -per_step);
        last_offset = current_scroll_offset(&ctx, central_id)
            .expect("ScrollArea must have persisted state after a scroll gesture");
    }
    let end_row = (last_offset / ROW_HEIGHT).floor() as i64;

    assert!(
        last_offset > start_offset,
        "sanity check failed: scroll offset did not advance (start={start_offset}, \
         end={last_offset}) — table_scroll bench numbers would be meaningless (always the \
         same ~30 top rows), refusing to report them"
    );
    assert!(
        end_row - start_row > 100,
        "sanity check failed: rendered row range only moved from ~row {start_row} to ~row \
         {end_row} over 30 fling frames — far less than expected, table virtualization or the \
         scroll-id reconstruction above may be broken"
    );
    println!(
        "sanity check OK: 30 fling frames moved the rendered range from ~row {start_row} to \
         ~row {end_row} (offset {start_offset:.0}pt -> {last_offset:.0}pt)"
    );
}

/// Times 300 frames by hand (no criterion), sweeping scroll the same way the
/// criterion benchmark does, and prints mean + p95 — criterion itself only
/// reports mean/median/std-dev, not percentiles, and the milestone's
/// acceptance criterion (ADR-006) is stated as a p95.
fn print_manual_p95(rows: &[RowVm]) {
    let ctx = egui::Context::default();
    let mut vm = TasksVm::for_bench(rows.to_vec());
    let mut layout = TableLayout::default();
    let per_step = (ROW_COUNT as f32 * ROW_HEIGHT) / SCROLL_STEPS_PER_SWEEP as f32;

    const SAMPLES: usize = 300;
    let mut durations = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let delta_y = sweep_delta(i, per_step);
        let start = Instant::now();
        drive_one_frame(&ctx, &mut vm, &mut layout, delta_y);
        durations.push(start.elapsed());
    }

    durations.sort();
    let mean: Duration = durations.iter().sum::<Duration>() / durations.len() as u32;
    let p95_idx = ((durations.len() as f32) * 0.95).ceil() as usize - 1;
    let p95 = durations[p95_idx.min(durations.len() - 1)];
    let max = *durations.last().expect("SAMPLES > 0");

    println!(
        "table_scroll_100k manual sampling ({SAMPLES} frames, no criterion): mean={mean:?} \
         p95={p95:?} max={max:?} (milestone target: p95 < 16.6ms)"
    );
    println!(
        "table_scroll_100k acceptance verdict: {}",
        if p95 < Duration::from_micros(16_600) {
            "PASS (p95 < 16.6ms)"
        } else {
            "FAIL (p95 >= 16.6ms) — see docs/benchmarks/M3.md for the milestone report"
        }
    );
}

/// The per-frame wheel delta for sweep step `i`: a steady downward fling of
/// `per_step` points, except once per [`SCROLL_STEPS_PER_SWEEP`] frames where
/// a large positive delta snaps back near the top (clamped by `ScrollArea`
/// itself) so the next sweep starts over — see the module doc's "large
/// single wheel delta" rationale.
fn sweep_delta(i: usize, per_step: f32) -> f32 {
    if i % SCROLL_STEPS_PER_SWEEP == SCROLL_STEPS_PER_SWEEP - 1 {
        (ROW_COUNT as f32) * ROW_HEIGHT // snap back to top (overshoots; ScrollArea clamps to 0)
    } else {
        -per_step
    }
}

fn bench_table_scroll(c: &mut Criterion) {
    let rows = synthetic_rows(ROW_COUNT);

    sanity_check_scroll_advances_rendered_range(&rows);

    let ctx = egui::Context::default();
    let mut vm = TasksVm::for_bench(rows.clone());
    let mut layout = TableLayout::default();
    let per_step = (ROW_COUNT as f32 * ROW_HEIGHT) / SCROLL_STEPS_PER_SWEEP as f32;
    let step = Cell::new(0usize);

    let mut group = c.benchmark_group("table_scroll");
    group.bench_function("table_scroll_100k", |b| {
        b.iter(|| {
            let i = step.get();
            step.set(i + 1);
            let delta_y = sweep_delta(i, per_step);
            drive_one_frame(&ctx, &mut vm, &mut layout, delta_y);
        });
    });
    group.finish();

    print_manual_p95(&rows);
}

criterion_group!(benches, bench_table_scroll);
criterion_main!(benches);
