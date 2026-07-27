//! The Speaker-ID window's chrome (F-087, J-7): Step 1 (Identify/Cancel +
//! progress), Step 2 (name/play-sample rows + merge checkbox), Save. A pure
//! render function of [`SpeakerIdVm`] — window/viewport plumbing is
//! `app.rs`'s job (the same "view is app-free" shape `views::resizer`/
//! `views::viewer` use), so `tests/kittest_speaker_id.rs` drives it directly
//! over a bare `egui::Ui` (`Harness::new_ui_state`).
//!
//! Ported from Buzz's `SpeakerIdentificationWidget` layout
//! (`_create_step_1_group`/`_create_step_2_group`/`_create_save_section`,
//! `speaker_identification_widget.py:489-584`).

use egui::RichText;

use crate::i18n::tr;
use crate::speaker_id::{SpeakerIdDeps, SpeakerIdStage, SpeakerIdVm};

/// Renders one Speaker-ID window's content into `ui`. Polls the in-flight
/// Identify run and the sample player once per call (the same
/// "poll-then-render" convention `views::viewer::viewer_ui` uses for its own
/// player), and keeps repainting while a sample is playing so
/// [`SpeakerIdVm::tick_sample_playback`]'s stop-at-10s check actually fires
/// promptly rather than waiting for unrelated input.
pub fn ui(ui: &mut egui::Ui, vm: &mut SpeakerIdVm, deps: &SpeakerIdDeps) {
    // Derived from `ui.ctx()` (never taken as a separate parameter) — the
    // same convention `views::viewer::viewer_ui` uses, so `app.rs`'s
    // `egui::CentralPanel::default().show(ctx, |ui| { .. })` closure never
    // needs to borrow the outer `ctx` a second time alongside `ui`.
    let ctx = ui.ctx().clone();

    vm.poll_identify();
    vm.poll_sample_player(deps);
    vm.tick_sample_playback();
    if vm.sample_is_playing() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            show_step1(ui, vm, deps, &ctx);
            ui.add_space(8.0);
            show_step2(ui, vm, deps, &ctx);
        });
}

/// A bold section heading (Buzz sets each section's `QLabel` font weight
/// bold, `:490-494,543-546`).
fn heading(ui: &mut egui::Ui, key: &str) {
    ui.label(RichText::new(tr(key)).strong());
}

/// "Step 1: Identify speakers": the gate label, progress bar, and the
/// Identify/Cancel button pair (only one of which shows at a time — Buzz
/// toggles `setVisible` on the same pair, `:594-596,628-630`).
fn show_step1(ui: &mut egui::Ui, vm: &mut SpeakerIdVm, deps: &SpeakerIdDeps, ctx: &egui::Context) {
    heading(ui, "speaker-id-step1-heading");
    ui.group(|ui| {
        let label = match &vm.stage {
            SpeakerIdStage::Idle | SpeakerIdStage::Ready(_) | SpeakerIdStage::Error(_) => {
                tr("speaker-id-ready-label")
            }
            SpeakerIdStage::MissingAudio => tr("speaker-id-missing-audio-label"),
            SpeakerIdStage::Running { label, .. } => label.clone(),
        };
        ui.label(label);

        match &vm.stage {
            SpeakerIdStage::Running {
                fraction: Some(fraction),
                ..
            } => {
                ui.add(egui::ProgressBar::new(*fraction).show_percentage());
            }
            SpeakerIdStage::Running { fraction: None, .. } => {
                // Indeterminate: the current stage has no measurable
                // fraction (decoding, or the single monolithic diarize
                // call) — an animated bar, never a fabricated one
                // (`models_ui.rs`'s own idiom for the exact same case).
                ui.add(egui::ProgressBar::new(0.0).animate(true));
            }
            _ => {
                ui.add(egui::ProgressBar::new(0.0));
            }
        }

        if let SpeakerIdStage::Error(message) = &vm.stage {
            ui.colored_label(ui.visuals().warn_fg_color, message);
        }

        ui.horizontal(|ui| {
            if matches!(vm.stage, SpeakerIdStage::Running { .. }) {
                if ui
                    .add_enabled(
                        !vm.cancel_requested,
                        egui::Button::new(tr("speaker-id-cancel-button")),
                    )
                    .clicked()
                {
                    vm.cancel_identify();
                }
            } else if ui
                .add_enabled(
                    vm.can_identify(),
                    egui::Button::new(tr("speaker-id-identify-button")),
                )
                .clicked()
            {
                vm.start_identify(deps, ctx);
            }
        });
    });
}

/// "Step 2: Name speakers": one row per unique speaker (editable name +
/// Play sample), the merge checkbox, and Save — enabled only once Step 1 is
/// `Ready` (Buzz gates the whole group + checkbox + Save button on
/// `progress == 8`, `:658-661`).
fn show_step2(ui: &mut egui::Ui, vm: &mut SpeakerIdVm, deps: &SpeakerIdDeps, ctx: &egui::Context) {
    heading(ui, "speaker-id-step2-heading");
    let ready = matches!(vm.stage, SpeakerIdStage::Ready(_));
    let mut play_idx: Option<usize> = None;
    let mut merge_toggled: Option<bool> = None;

    ui.add_enabled_ui(ready, |ui| {
        ui.group(|ui| {
            if let SpeakerIdStage::Ready(state) = &mut vm.stage {
                for (idx, name) in state.names.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(name).desired_width(200.0));
                        if ui.button(tr("speaker-id-play-sample")).clicked() {
                            play_idx = Some(idx);
                        }
                    });
                }
            }

            let mut merge = match &vm.stage {
                SpeakerIdStage::Ready(state) => state.merge,
                _ => true,
            };
            if ui
                .checkbox(&mut merge, tr("speaker-id-merge-checkbox"))
                .changed()
            {
                merge_toggled = Some(merge);
            }
        });

        if let Some(message) = vm.sample_error() {
            ui.colored_label(ui.visuals().warn_fg_color, message);
        }

        if let Some(message) = vm.save_error_for_test() {
            ui.colored_label(ui.visuals().warn_fg_color, message);
        }

        if ui.button(tr("speaker-id-save-button")).clicked() {
            vm.save_requested = true;
        }
    });

    // Applied after the closure above so nothing double-borrows `vm.stage`
    // (the closure already holds it mutably while rendering).
    if let Some(idx) = play_idx {
        vm.play_sample(deps, idx, ctx);
    }
    if let Some(merge) = merge_toggled
        && let SpeakerIdStage::Ready(state) = &mut vm.stage
    {
        state.merge = merge;
    }
}
