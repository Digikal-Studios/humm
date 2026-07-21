//! `humm-gui` binary entry point. The module tree (`app`, `services`,
//! `events`, `i18n`, `shortcuts`, `viewmodel`, `views`, `logging`) lives in
//! `lib.rs` so `tests/kittest_smoke.rs` can build a real [`HummApp`]
//! in-process; this file only wires launch order.
//!
//! Launch order (design spec §3.1): parse argv as a pending import → acquire
//! the GUI single-instance lock (second launch → native dialog + exit) →
//! acquire the instance lock (decides the stale-task-flip, applied inside
//! `services::init`) → build [`Services`] (which also spawns the background
//! update check, F-006, when one is due — `services::init`'s own doc
//! comment) → `eframe::run_native`. The one-per-launch telemetry event
//! (F-007, ADR-009) deliberately isn't fired here either — `HummApp` fires
//! it from its first `eframe::App::ui` frame instead, off this function's
//! own critical path (see `services::fire_launch_telemetry`'s doc comment).
//! Any failure up to and including `services::init` renders a minimal error
//! app with the actionable cause and a Quit button — never a panic.

use std::path::PathBuf;

use humm_gui::app::HummApp;
use humm_gui::logging;
use humm_gui::services::{self, InitError, Services};

fn main() -> eframe::Result {
    let log_dir = humm_core::paths::log_dir();
    logging::init(&log_dir);
    log::debug!("humm-gui starting (log_dir={})", log_dir.display());

    // Files passed as argv (Finder "open with" parity — deviation #12 in the
    // design spec): opened as the options sheet's request on the app's
    // first frame (`HummApp::drain_pending_argv_import`).
    let pending_import: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if !pending_import.is_empty() {
        log::info!(
            "{} file(s) passed on the command line; opening the import options sheet",
            pending_import.len()
        );
    }

    let data_dir = humm_core::paths::data_dir();

    // GUI exclusivity (ADR-017): a second GUI launch shows a native dialog
    // and exits cleanly instead of racing the first over shared UI state.
    // Bound (not `let _ = ..`) so the claim is held for the rest of `main`.
    let _gui_lock = match humm_core::instance::acquire_gui_lock(&data_dir) {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => {
            show_already_running_dialog();
            return Ok(());
        }
        Err(err) => {
            log::warn!(
                "could not acquire the GUI lock ({err}); continuing without single-instance \
                 protection"
            );
            None
        }
    };

    // Single-instance stale-flip gate (ADR-017, `docs/HUMM_SURFACE.md` §7):
    // only a process that wins the exclusive claim is provably the sole live
    // humm process, so only it may treat every `in_progress`/`queued` row as
    // stale. `_instance_lock` is bound (not `let _ = ..`) so the resulting
    // shared claim is held for the rest of `main`. A lock I/O error falls
    // back to "flip anyway" — the pre-ADR-017 behavior — rather than
    // bricking the GUI.
    let (_instance_lock, sole_instance) =
        match humm_core::instance::acquire_instance_lock(&data_dir) {
            Ok(lock) => (Some(lock.guard), lock.sole_instance),
            Err(err) => {
                log::warn!(
                    "could not acquire the single-instance lock ({err}); falling back to \
                 unconditional stale-task recovery"
                );
                (None, true)
            }
        };

    match services::init(sole_instance) {
        Ok(services) => run_app(services, pending_import),
        Err(err) => run_error_app(err),
    }
}

/// Shows the native "already running" dialog (design spec §3.1 / ADR-017):
/// Buzz has no such guard, so this is a humm-only addition.
fn show_already_running_dialog() {
    rfd::MessageDialog::new()
        .set_title("humm")
        .set_description("humm is already running.")
        .set_level(rfd::MessageLevel::Info)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// The real window: 1240×600, titled "humm", geometry/egui-memory persisted
/// to `config_dir()/gui.ron` (design spec §3.1, F-001). `argv_import` is
/// threaded straight through to [`HummApp::new`].
fn run_app(services: Services, argv_import: Vec<PathBuf>) -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1240.0, 600.0])
        .with_title("humm");
    let options = eframe::NativeOptions {
        viewport,
        persistence_path: Some(humm_core::paths::config_dir().join("gui.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "humm",
        options,
        Box::new(move |cc| Ok(Box::new(HummApp::new(cc, services, argv_import)))),
    )
}

/// A minimal fallback window shown when [`services::init`] fails: the
/// actionable cause, plus a Quit button — never a panic (design spec §3.1).
fn run_error_app(err: InitError) -> eframe::Result {
    log::error!("humm-gui failed to start: {err}");
    let message = err.to_string();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([480.0, 220.0])
            .with_title("humm"),
        ..Default::default()
    };
    eframe::run_native(
        "humm",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ErrorApp {
                message: message.clone(),
            }))
        }),
    )
}

/// The fallback app [`run_error_app`] runs: shows the cause, offers Quit.
struct ErrorApp {
    message: String,
}

impl eframe::App for ErrorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("humm couldn't start");
            ui.label(&self.message);
            if ui.button("Quit").clicked() {
                std::process::exit(1);
            }
        });
    }
}
