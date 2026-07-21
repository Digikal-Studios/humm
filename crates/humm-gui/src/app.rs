//! `HummApp`: the `eframe::App` implementation — owns the runtime
//! [`Services`], the shared event-pump buffer, and (F-003, this task) the
//! task table's view-model/layout, and renders the main window every frame
//! (design spec §1/§4).

use humm_core::settings::GuiSettings;

use crate::events::{self, SharedEventQueue};
use crate::services::Services;
use crate::viewmodel::{TableAction, TasksVm};
use crate::views;
use crate::views::task_table::TableLayout;

/// The eframe application.
///
/// `capabilities`/`model_root`/`update_rx` aren't read yet: URL-import
/// gating, model resolution, and the update-check banner are later M3 tasks.
/// They're already threaded through `services` because every later M3 task
/// builds on this same `HummApp`.
pub struct HummApp {
    services: Services,
    /// Shared buffer the event-pump thread feeds. `None` until the pump is
    /// spawned: eagerly in [`HummApp::new`] (a real `egui::Context` is
    /// available immediately from [`eframe::CreationContext`]); lazily, on
    /// the first `eframe::App::ui` call, in [`HummApp::new_for_test`] (which
    /// has no `CreationContext` to draw one from — see that constructor's
    /// doc comment).
    event_queue: Option<SharedEventQueue>,
    /// The task table's view-model (F-003): loaded once at construction
    /// (`TasksVm::load`), then kept current by folding in each drained
    /// [`humm_core::queue::TaskEvent`] (`TasksVm::apply`) rather than ever
    /// re-querying the whole table.
    tasks: TasksVm,
    /// The task table's persisted column layout (visible/order); sort state
    /// itself lives on `tasks` (`TasksVm::sort`).
    table_layout: TableLayout,
    /// The `[gui]` settings snapshot last written to disk — compared against
    /// the current `table_layout`/`tasks.sort()` snapshot every frame so a
    /// change is persisted exactly once, not re-saved on every unchanged
    /// frame.
    last_saved_gui_settings: GuiSettings,
    /// `TableAction`s the task table has emitted (retry/rename/notes/clear/
    /// open) — F-003 (this task) only *produces* these; wiring them to the
    /// queue/store/dialogs is Task 8's job. Accumulated here rather than
    /// discarded so no click is silently lost between now and then; Task 8
    /// is expected to drain this `Vec` each frame once it starts consuming
    /// them.
    pending_table_actions: Vec<TableAction>,
}

impl HummApp {
    /// Builds the app for real use (`main.rs`), spawning the event pump
    /// immediately from the window's own [`egui::Context`]
    /// (`cc.egui_ctx`).
    pub fn new(cc: &eframe::CreationContext<'_>, services: Services) -> Self {
        let event_queue = events::spawn_event_pump(services.events.clone(), cc.egui_ctx.clone());
        Self::build(services, Some(event_queue))
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
        Self::build(services, None)
    }

    /// Shared constructor body: loads the task table's view-model from
    /// `services.store` and restores its persisted layout/sort from
    /// `services.settings.gui` before the first frame ever renders.
    fn build(services: Services, event_queue: Option<SharedEventQueue>) -> Self {
        let gui_settings = {
            let settings = services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settings.gui.clone()
        };

        let mut tasks = TasksVm::load(&services.store);
        let (sort_col, sort_descending) = TableLayout::sort_from_gui_settings(&gui_settings);
        tasks.set_sort_exact(sort_col, sort_descending);

        let table_layout = TableLayout::from_gui_settings(&gui_settings);
        let last_saved_gui_settings = table_layout.to_gui_settings(tasks.sort());

        HummApp {
            services,
            event_queue,
            tasks,
            table_layout,
            last_saved_gui_settings,
            pending_table_actions: Vec::new(),
        }
    }

    /// Writes `table_layout`/`tasks.sort()` to `services.settings.gui` and
    /// persists it (`Settings::save`) if it differs from what was last
    /// saved. A save failure is logged, not fatal — the in-memory layout the
    /// user sees is unaffected either way, only next launch's restore would
    /// miss this particular change.
    fn persist_layout_if_changed(&mut self) {
        let current = self.table_layout.to_gui_settings(self.tasks.sort());
        if current == self.last_saved_gui_settings {
            return;
        }
        {
            let mut settings = self
                .services
                .settings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            settings.gui = current.clone();
            if let Err(err) = settings.save(&self.services.settings_path) {
                log::warn!("could not persist task table layout: {err}");
            }
        }
        self.last_saved_gui_settings = current;
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
            for event in events::drain(event_queue) {
                self.tasks.apply(&event, &self.services.store);
            }
        }

        views::main_window::show(
            ui,
            &mut self.tasks,
            &mut self.table_layout,
            &mut self.pending_table_actions,
        );

        self.persist_layout_if_changed();
    }
}
