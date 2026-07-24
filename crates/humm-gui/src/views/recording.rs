//! The live-recording window's chrome (F-060, spec §2.1): a plain-fn
//! renderer (`recording_ui`) over a bare `egui::Ui`, following the viewer/
//! resizer windows' own "render only sets intents on the VM, `app.rs` reads
//! them back" shape.
//!
//! Layout order ports `RecordingTranscriberWidget._build_layout`
//! (`recording_transcriber_widget.py:196-224`): options group, microphone
//! row, realtime-fit warning (spec §2.8, rendered just above the meter/record
//! row per its own prose even though the design doc lists it last), meter +
//! Record/Stop row, presentation-bar placeholder (Task 12 owns the real bar),
//! transcription panel, copy-bar placeholder (ditto). The translation panel
//! is never rendered — F-063 is M6, and spec §2.1.6 says the panel itself is
//! absent this milestone, not merely disabled.

use std::collections::BTreeMap;

use crate::i18n::{tr, tr_args};
use crate::recording::{RecordingDeps, RecordingStatus, RecordingVm, realtime_fit_warning_visible};
use crate::views::audio_meter::audio_meter;
use crate::views::options_sheet::{
    render_language_combo, render_model_combo, render_task_combo, title_case,
};

/// Buzz's own Apple-Silicon gate for the model-load-time info tooltip
/// (`transcription_options_group_box.py:106`: `platform.system() == "Darwin"
/// and platform.machine() == "arm64"`). `cfg!` is the correct Rust analogue
/// here — humm ships one binary per target, so a compile-time check for "is
/// this build Apple Silicon macOS" is equivalent to Buzz's runtime check on
/// the machine it's actually running on.
fn apple_silicon_load_note_applicable() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

/// Renders the recording window's full content.
///
/// `custom_model_available`/`measured_speed` are precomputed by `app.rs` from
/// `Services.settings` (this module never locks `Settings` itself, matching
/// `options_sheet::ui`'s own `custom_model_available: bool` convention).
pub fn recording_ui(
    ui: &mut egui::Ui,
    vm: &mut RecordingVm,
    deps: &RecordingDeps,
    custom_model_available: bool,
    measured_speed: &BTreeMap<String, f32>,
) {
    let stopped = vm.status == RecordingStatus::Stopped;

    ui.add_enabled_ui(stopped, |ui| {
        render_model_combo(
            ui,
            "humm-recording-model-size",
            &mut vm.options.model_size,
            custom_model_available,
        );
        if apple_silicon_load_note_applicable() {
            ui.label("\u{24d8}")
                .on_hover_text(tr("recording-model-load-note"));
        }
        render_task_combo(ui, "humm-recording-task", &mut vm.options.task);
        render_language_combo(ui, "humm-recording-language", &mut vm.options.language);
        if ui.button(tr("recording-advanced-button")).clicked() {
            vm.advanced_requested = true;
        }

        ui.separator();

        ui.horizontal(|ui| {
            ui.label(tr("recording-microphone-label"));
            let devices = vm.devices.clone();
            let current_name = vm
                .selected_device
                .as_ref()
                .and_then(|id| devices.iter().find(|device| &device.id == id))
                .map(|device| device.name.clone())
                .unwrap_or_default();
            let mut clicked_id: Option<String> = None;
            egui::ComboBox::from_id_salt("humm-recording-device")
                .selected_text(current_name)
                .show_ui(ui, |ui| {
                    for device in &devices {
                        let is_selected = vm.selected_device.as_deref() == Some(device.id.as_str());
                        if ui.selectable_label(is_selected, &device.name).clicked() {
                            clicked_id = Some(device.id.clone());
                        }
                    }
                });
            if let Some(id) = clicked_id {
                vm.on_device_changed(id, deps);
            }
        });
    });

    ui.add_space(4.0);

    if realtime_fit_warning_visible(measured_speed, vm.options.model_size) {
        let ratio = measured_speed
            .get(vm.options.model_size.as_str())
            .copied()
            .unwrap_or(0.0);
        let size_text = title_case(vm.options.model_size.as_str());
        let ratio_text = format!("{ratio:.1}");
        ui.colored_label(
            ui.visuals().warn_fg_color,
            tr_args(
                "recording-realtime-warning",
                &[("size", size_text.as_str()), ("ratio", ratio_text.as_str())],
            ),
        );
    }

    // Buzz's `record_button_layout` (`_build_layout`): the meter stretches,
    // the Record/Stop button keeps its own natural (fixed) width — egui has
    // no automatic flex layout, so a right-to-left row is the idiomatic
    // equivalent: the button is placed first (ending up rightmost, its own
    // natural width), then the meter fills whatever's left via
    // `ui.available_width()`, which by then correctly excludes the button's
    // space. A plain left-to-right row would instead let the meter's
    // `ui.available_width()` claim the *entire* row before the button is
    // ever laid out, pushing the button off past the visible/interactive
    // area.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = match vm.status {
                RecordingStatus::Stopped => tr("recording-record-button"),
                RecordingStatus::Recording => tr("recording-stop-button"),
            };
            if ui.button(label).clicked() {
                vm.on_record_clicked(deps);
            }
            audio_meter(ui, &vm.meter, &mut vm.meter_current_amplitude);
        });
    });

    // Presentation options bar (spec §2.6): hidden until recording starts.
    // Placeholder only this task — Task 12 owns the real controls.
    if vm.presentation_bar_visible {
        ui.horizontal(|ui| {
            ui.weak(tr("recording-presentation-bar-placeholder"));
        });
    }

    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("humm-recording-panel-scroll")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut vm.panel_text)
                    .hint_text(tr("recording-panel-placeholder"))
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        });

    // Translation panel: not rendered (F-063 is M6, spec §2.1.6).

    // Copy actions bar (spec §2.7): hidden until a recording stops.
    // Placeholder only this task — Task 12 owns the real controls.
    if vm.copy_bar_visible {
        ui.horizontal(|ui| {
            ui.weak(tr("recording-copy-bar-placeholder"));
        });
    }
}
