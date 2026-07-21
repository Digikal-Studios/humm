//! `HummApp`: the `eframe::App` implementation — owns the runtime
//! [`Services`] and the shared event-pump buffer, and renders the main
//! window every frame (design spec §1/§4).

use crate::events::{self, SharedEventQueue};
use crate::services::Services;
use crate::views;

/// The eframe application.
///
/// Most of `services`' fields (`store`, `queue`, `settings`, `capabilities`,
/// `model_root`, `update_rx`) aren't read yet: this task renders only the
/// empty-state hero (`views::main_window`). They're already threaded through
/// here because every later M3 task builds on this same `HummApp` (the
/// import flow reads `capabilities`/`queue`, the task table reads `store`,
/// preferences read/write `settings`, ...).
pub struct HummApp {
    services: Services,
    /// Shared buffer the event-pump thread feeds. `None` until the pump is
    /// spawned: eagerly in [`HummApp::new`] (a real `egui::Context` is
    /// available immediately from [`eframe::CreationContext`]); lazily, on
    /// the first `eframe::App::ui` call, in [`HummApp::new_for_test`] (which
    /// has no `CreationContext` to draw one from — see that constructor's
    /// doc comment).
    event_queue: Option<SharedEventQueue>,
}

impl HummApp {
    /// Builds the app for real use (`main.rs`), spawning the event pump
    /// immediately from the window's own [`egui::Context`]
    /// (`cc.egui_ctx`).
    pub fn new(cc: &eframe::CreationContext<'_>, services: Services) -> Self {
        let event_queue = events::spawn_event_pump(services.events.clone(), cc.egui_ctx.clone());
        HummApp {
            services,
            event_queue: Some(event_queue),
        }
    }

    /// Builds the app directly from injected [`Services`], for kittest
    /// harnesses. Every later kittest task constructs `HummApp` this way.
    ///
    /// Unlike [`HummApp::new`], no [`eframe::CreationContext`] is available
    /// at this call site (the brief's constructor signature takes only
    /// `services`), so the event pump is instead spawned lazily on the first
    /// `eframe::App::ui` call, using the `egui::Context` that call
    /// receives — for an `egui_kittest` harness, that is the *same*
    /// `Context` its own `CreationContext` carries, so the net effect is
    /// identical; it just can't be captured before the first frame runs.
    pub fn new_for_test(services: Services) -> Self {
        HummApp {
            services,
            event_queue: None,
        }
    }
}

impl eframe::App for HummApp {
    /// eframe 0.35's per-frame entry point: `App::ui` (not the classic
    /// `App::update`), handed the root viewport's `egui::Ui` directly rather
    /// than a bare `&egui::Context` — `egui::Ui::ctx()` gets the `Context`
    /// back when something (like the event pump) needs to own a clone of it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.event_queue.is_none() {
            self.event_queue = Some(events::spawn_event_pump(
                self.services.events.clone(),
                ui.ctx().clone(),
            ));
        }
        if let Some(event_queue) = &self.event_queue {
            for _event in events::drain(event_queue) {
                // Applying events to a view-model (row insert/update, toasts)
                // lands with the task table (F-003, a later task); this task
                // only drains the buffer so it never grows unbounded.
            }
        }

        views::main_window::show(ui);
    }
}
