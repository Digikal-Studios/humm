//! View rendering functions. Each submodule renders one region/window; none
//! own state beyond what's passed in (`ARCHITECTURE.md` §7 — view-models and
//! services hold state, views are pure render functions of it).

/// Wakes the ROOT viewport so `app.rs`'s harvest loop — which runs only in
/// the root's own update pass — actually gets to run.
///
/// **This is the crate-wide invariant for every child-viewport → root
/// intent**: the viewer, resizer, and Speaker-ID windows are all *deferred*
/// child viewports (`ctx.show_viewport_deferred`); a click landing directly
/// on one repaints only that child, never the root, so an intent flag/value
/// set on a VM without also calling this could sit unconsumed until
/// something unrelated happened to repaint the root — observed as a ~90 s
/// stall in the M8 windowed acceptance pass (Task 17 defect 2,
/// `docs/gui-manual/M8.md` §8.1). This is the inverse of
/// `app.rs::HummApp::repaint_other_open_viewports` (Task 11's fix,
/// root → children): here it's child → root.
///
/// Every call site that sets a root-harvested intent must call this (or
/// [`latch_root_harvested_intent`], its bool-flag convenience wrapper)
/// before returning from the click handler: `views::viewer`'s Resize/
/// Identify Speakers buttons, `views::speaker_id`'s Save button, and
/// `views::resizer`'s Extend/Resize/Merge buttons (the latter three via this
/// function directly, since `ResizerVm::pending_action` is an
/// `Option<ResizerAction>`, not a bool flag [`latch_root_harvested_intent`]
/// could take).
fn wake_root_for_harvest(ctx: &egui::Context) {
    ctx.request_repaint_of(egui::ViewportId::ROOT);
}

/// Latches a one-shot "the root viewport must harvest this" bool intent flag
/// (e.g. `ViewerVm::resize_requested`/`identify_speakers_requested`,
/// `SpeakerIdVm::save_requested`) and wakes the root via
/// [`wake_root_for_harvest`] — see that function's doc for why both steps
/// are required. A thin convenience wrapper for the common bool-flag case;
/// a call site whose intent isn't a plain bool (`views::resizer`'s
/// `Option<ResizerAction>`) calls [`wake_root_for_harvest`] directly instead,
/// right after setting its own value, so the wake-root logic itself still
/// has exactly one implementation.
fn latch_root_harvested_intent(flag: &mut bool, ctx: &egui::Context) {
    *flag = true;
    wake_root_for_harvest(ctx);
}

#[cfg(test)]
mod tests {
    use super::latch_root_harvested_intent;

    /// Task 17 defect 2 (`docs/gui-manual/M8.md` §8.1): a click that only
    /// latched `resize_requested`/`identify_speakers_requested` (a bare
    /// `vm.resize_requested = true`) left the intent stranded — `app.rs`'s
    /// harvest for both flags runs in the ROOT viewport's own update pass,
    /// but the viewer is a deferred child viewport whose own repaints never
    /// drive the root. This pins the fix directly against the helper, the
    /// same "call the pure function against a raw `egui::Context`, no `Ui`/
    /// click machinery at all" shape `speaker_id.rs`'s
    /// `set_stage_requests_repaint_of_the_given_viewport_specifically` uses
    /// (and for the identical reason: an `egui_kittest` `Harness::run()`
    /// loops steps *until the root settles*, per its own `_try_run`, so it
    /// always observes `has_requested_repaint_for(ROOT) == false` by the
    /// time it returns — through a `Harness` this fix would be
    /// unobservable no matter which side of the bug is on screen).
    ///
    /// `egui::Context::default()` seeds `ViewportId::ROOT` as already
    /// wanting its first frame (the same quirk the `speaker_id.rs` test's
    /// own comment documents for that non-root case), and empirically takes
    /// *two* settled `run_ui` passes — not one — to actually clear (checked
    /// against a standalone `egui`-only probe: pass 1 still reads `true`,
    /// pass 2 reads `false`) before the "false, then true only after the
    /// call" witness below means anything.
    #[test]
    fn latch_root_harvested_intent_requests_a_root_repaint() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let _ = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        assert!(
            !ctx.has_requested_repaint_for(&egui::ViewportId::ROOT),
            "a settled context must not already be wanting a root repaint"
        );

        let mut flag = false;
        latch_root_harvested_intent(&mut flag, &ctx);

        assert!(flag, "the intent flag itself must still be latched");
        assert!(
            ctx.has_requested_repaint_for(&egui::ViewportId::ROOT),
            "latching the intent must also request a repaint of the ROOT \
             viewport, so app.rs's harvest (which runs only in the root's \
             own update pass) actually gets to run"
        );
    }
}

pub mod about;
pub mod audio_meter;
pub mod export_menu;
pub mod main_window;
pub mod options_sheet;
pub mod playback_controls;
pub mod preferences;
pub mod prefs_feedback;
pub mod prefs_folder_watch;
pub mod prefs_general;
pub mod prefs_postproc;
pub mod prefs_shortcuts;
pub mod presentation;
pub mod recording;
pub mod recording_advanced;
pub mod rename_notes;
pub mod resizer;
pub mod secret_field;
pub mod speaker_id;
pub mod task_table;
pub mod toasts;
pub mod update_notice;
pub mod url_dialog;
pub mod viewer;
pub mod viewer_search;
pub mod viewer_table;
