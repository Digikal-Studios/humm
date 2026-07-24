//! The live-recording window's chrome (F-060, spec §2.1): a plain-fn
//! renderer (`recording_ui`) over a bare `egui::Ui`, following the viewer/
//! resizer windows' own "render only sets intents on the VM, `app.rs` reads
//! them back" shape.
//!
//! Layout order ports `RecordingTranscriberWidget._build_layout`
//! (`recording_transcriber_widget.py:196-224`): options group, microphone
//! row, realtime-fit warning (spec §2.8, rendered just above the meter/record
//! row per its own prose even though the design doc lists it last), meter +
//! Record/Stop row, presentation options bar (`views::presentation::bar_ui`,
//! F-065 §2.6, Task 12), transcription panel, copy bar
//! ([`copy_click_outcome`]/[`CopyLabelState`] below, F-065 §2.7, Task 12),
//! then the advanced settings sheet (`views::recording_advanced::ui`, an
//! `egui::Modal` overlay, Task 12) and the error-dialog modal, both rendered
//! last so they paint over everything above. The translation panel is never
//! rendered — F-063 is M6, and spec §2.1.6 says the panel itself is absent
//! this milestone, not merely disabled.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use humm_core::settings::Settings;

use crate::i18n::{tr, tr_args};
use crate::live_session::PrepUpdate;
use crate::recording::{RecordingDeps, RecordingStatus, RecordingVm, realtime_fit_warning_visible};
use crate::views::audio_meter::audio_meter;
use crate::views::options_sheet::{
    render_language_combo, render_model_combo, render_task_combo, title_case,
};
use crate::views::presentation;
use crate::views::recording_advanced;

/// The Copy button's label/timer state (F-065 §2.7, task brief). Ported from
/// Buzz `on_copy_transcript_clicked` (`recording_transcriber_widget.py:359-391`):
/// `Copied!` for 2 s after a non-empty copy, `Nothing to copy!` for 1.5 s
/// after an empty one, then reverts to `Copy`. Buzz's "Copy failed" arms
/// (`QApplication`/clipboard-object-lifetime checks) have no egui analogue —
/// `egui::Context::copy_text` cannot fail observably — so they are not
/// ported (spec §2.7, documented deviation).
///
/// `Copied`/`NothingToCopy` carry the `Instant` they were entered at rather
/// than calling `Instant::now()` internally, so [`copy_click_outcome`] and
/// [`CopyLabelState::revert_if_expired`] are unit-testable with a
/// constructed clock (`views/toasts.rs`'s own "injected clock" precedent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyLabelState {
    /// Showing "Copy".
    #[default]
    Idle,
    /// Showing "Copied!" since the carried instant.
    Copied(Instant),
    /// Showing "Nothing to copy!" since the carried instant.
    NothingToCopy(Instant),
}

impl CopyLabelState {
    /// How long "Copied!" stays shown (Buzz `QTimer.singleShot(2000, ...)`).
    pub const COPIED_DURATION: Duration = Duration::from_millis(2000);
    /// How long "Nothing to copy!" stays shown (Buzz
    /// `QTimer.singleShot(1500, ...)`).
    pub const NOTHING_DURATION: Duration = Duration::from_millis(1500);

    /// Reverts to [`Self::Idle`] once `now` is past this state's own
    /// duration; otherwise returns `self` unchanged.
    pub fn revert_if_expired(self, now: Instant) -> Self {
        match self {
            CopyLabelState::Copied(at)
                if now.saturating_duration_since(at) >= Self::COPIED_DURATION =>
            {
                CopyLabelState::Idle
            }
            CopyLabelState::NothingToCopy(at)
                if now.saturating_duration_since(at) >= Self::NOTHING_DURATION =>
            {
                CopyLabelState::Idle
            }
            other => other,
        }
    }

    /// Time remaining until this state would auto-revert as of `now` —
    /// `None` while [`Self::Idle`] (nothing pending). Drives the render
    /// fn's `request_repaint_after`, mirroring `views/toasts.rs`'s
    /// `ToastStack::next_expiry`.
    pub fn remaining(self, now: Instant) -> Option<Duration> {
        match self {
            CopyLabelState::Idle => None,
            CopyLabelState::Copied(at) => {
                Some(Self::COPIED_DURATION.saturating_sub(now.saturating_duration_since(at)))
            }
            CopyLabelState::NothingToCopy(at) => {
                Some(Self::NOTHING_DURATION.saturating_sub(now.saturating_duration_since(at)))
            }
        }
    }

    /// The i18n key for this state's button label.
    pub fn label_key(self) -> &'static str {
        match self {
            CopyLabelState::Idle => "recording-copy-button",
            CopyLabelState::Copied(_) => "recording-copy-copied",
            CopyLabelState::NothingToCopy(_) => "recording-copy-nothing",
        }
    }
}

/// The pure decision behind a Copy button click (Buzz
/// `on_copy_transcript_clicked`, `:359-391`): trims `panel_text`; empty ⇒
/// [`CopyLabelState::NothingToCopy`] and nothing to place on the clipboard;
/// else [`CopyLabelState::Copied`] and the trimmed text to copy.
pub fn copy_click_outcome(panel_text: &str, now: Instant) -> (CopyLabelState, Option<String>) {
    let trimmed = panel_text.trim();
    if trimmed.is_empty() {
        (CopyLabelState::NothingToCopy(now), None)
    } else {
        (CopyLabelState::Copied(now), Some(trimmed.to_string()))
    }
}

/// Formats a [`PrepUpdate`] into the label the inline progress row shows
/// (spec §2.1: "shown inline in the window", non-modal — the humm M3 J-1 ✚
/// pattern, unlike Buzz's modal `ModelDownloadProgressDialog`).
fn prep_label(prep: &PrepUpdate) -> String {
    match prep {
        PrepUpdate::Downloading {
            downloaded,
            total: Some(total),
        } if *total > 0 => {
            let pct = (*downloaded as f64 / *total as f64 * 100.0).clamp(0.0, 100.0);
            tr_args(
                "recording-model-downloading",
                &[("percent", &format!("{pct:.0}"))],
            )
        }
        PrepUpdate::Downloading { .. } => tr("recording-model-downloading-indeterminate"),
        PrepUpdate::LoadingEngine => tr("recording-model-loading"),
    }
}

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
/// `Services.settings` (this module never locks `Settings` itself for
/// *those two* reads, matching `options_sheet::ui`'s own
/// `custom_model_available: bool` convention). `settings`/`settings_path`
/// are threaded through to the advanced sheet and the presentation bar
/// instead (Task 12), which — unlike the rest of this module — *do* need
/// live read/write access for their own immediate-save fields
/// (`views/resizer.rs::ui`'s identical `settings: &Mutex<Settings>,
/// settings_path: &Path` convention).
#[allow(clippy::too_many_arguments)]
pub fn recording_ui(
    ui: &mut egui::Ui,
    vm: &mut RecordingVm,
    deps: &RecordingDeps,
    custom_model_available: bool,
    measured_speed: &BTreeMap<String, f32>,
    settings: &Mutex<Settings>,
    settings_path: &Path,
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
                RecordingStatus::Recording | RecordingStatus::Stopping => {
                    tr("recording-stop-button")
                }
            };
            // Buzz `stop_recording`: the button is disabled until the
            // session thread's `finished`/`error` acknowledgement arrives
            // (`RecordingStatus::Stopping`) — clicking it again while
            // stopping would double-latch a stop intent nothing consumes.
            let enabled = vm.status != RecordingStatus::Stopping;
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                vm.on_record_clicked(deps);
            }
            audio_meter(ui, &vm.meter, &mut vm.meter_current_amplitude);
        });
    });

    // Model-download/engine-load inline progress (spec §2.1): shown
    // non-modally in the window itself while a session is starting up.
    // The Record/Stop button above doubles as Cancel (Buzz's separate
    // dialog Cancel button and this button both end up signaling the same
    // cooperative stop — mechanism deviation, documented).
    if let Some(prep) = &vm.prep {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(prep_label(prep));
        });
    }

    // Presentation options bar (spec §2.6): hidden until recording starts.
    if vm.presentation_bar_visible {
        presentation::bar_ui(ui, &mut vm.presentation, settings, settings_path);
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
    if vm.copy_bar_visible {
        copy_bar_ui(ui, vm);
    }

    // Advanced settings sheet (spec §2.1.6/§2.3, Task 12): a no-op modal
    // while closed. Rendered last so it overlays everything above, like the
    // error-dialog modal just below it.
    recording_advanced::ui(
        ui.ctx(),
        &mut vm.advanced_requested,
        &mut vm.advanced_sheet,
        &mut vm.options,
        settings,
        settings_path,
        deps.dialogs.as_ref(),
    );

    // Buzz's error dialog (`QMessageBox.critical`, byte-exact text) --
    // egui's non-native `egui::Modal` equivalent (`models_ui::ui`'s own
    // download-error modal is the precedent this mirrors).
    if vm.error_dialog.is_some() {
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("humm-recording-error")).show(ui.ctx(), |ui| {
            ui.label(vm.error_dialog.as_deref().unwrap_or_default());
            if ui.button(tr("dialog-ok")).clicked() {
                close = true;
            }
        });
        if close || modal.should_close() {
            vm.error_dialog = None;
        }
    }
}

/// Renders the copy bar: a right-aligned button showing
/// [`CopyLabelState::label_key`], tooltip byte-exact to Buzz
/// (`copy_transcript_button.setToolTip`, `:353`). Advances/reverts
/// `vm.copy_label`'s timer every frame it's visible, scheduling exactly one
/// repaint for the pending revert (`views/toasts.rs`'s identical pattern).
fn copy_bar_ui(ui: &mut egui::Ui, vm: &mut RecordingVm) {
    let now = Instant::now();
    vm.copy_label = vm.copy_label.revert_if_expired(now);
    if let Some(remaining) = vm.copy_label.remaining(now) {
        ui.ctx().request_repaint_after(remaining);
    }
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clicked = ui
                .button(tr(vm.copy_label.label_key()))
                .on_hover_text(tr("recording-copy-tooltip"))
                .clicked();
            if clicked {
                let (state, to_copy) = copy_click_outcome(&vm.panel_text, now);
                vm.copy_label = state;
                if let Some(text) = to_copy {
                    ui.ctx().copy_text(text);
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- copy_click_outcome ------------------------------------------------------

    #[test]
    fn a_non_empty_panel_copies_the_trimmed_text() {
        let now = Instant::now();
        let (state, copied) = copy_click_outcome("  hello world  \n", now);
        assert_eq!(state, CopyLabelState::Copied(now));
        assert_eq!(copied, Some("hello world".to_string()));
    }

    #[test]
    fn an_empty_panel_copies_nothing() {
        let now = Instant::now();
        let (state, copied) = copy_click_outcome("   \n  ", now);
        assert_eq!(state, CopyLabelState::NothingToCopy(now));
        assert_eq!(copied, None);
    }

    // -- CopyLabelState state machine (injected clock) ---------------------------

    #[test]
    fn idle_to_copied_to_revert() {
        let t0 = Instant::now();
        let mut state = CopyLabelState::Idle;
        assert_eq!(state.label_key(), "recording-copy-button");

        let (next, _) = copy_click_outcome("some text", t0);
        state = next;
        assert_eq!(state, CopyLabelState::Copied(t0));
        assert_eq!(state.label_key(), "recording-copy-copied");

        // Still within the 2s window: stays Copied.
        let mid = t0 + Duration::from_millis(1999);
        assert_eq!(state.revert_if_expired(mid), CopyLabelState::Copied(t0));

        // At/after 2s: reverts.
        let after = t0 + CopyLabelState::COPIED_DURATION;
        assert_eq!(state.revert_if_expired(after), CopyLabelState::Idle);
    }

    #[test]
    fn empty_to_nothing_to_copy_to_revert() {
        let t0 = Instant::now();
        let (state, _) = copy_click_outcome("", t0);
        assert_eq!(state, CopyLabelState::NothingToCopy(t0));
        assert_eq!(state.label_key(), "recording-copy-nothing");

        let mid = t0 + Duration::from_millis(1499);
        assert_eq!(
            state.revert_if_expired(mid),
            CopyLabelState::NothingToCopy(t0)
        );

        let after = t0 + CopyLabelState::NOTHING_DURATION;
        assert_eq!(state.revert_if_expired(after), CopyLabelState::Idle);
    }

    #[test]
    fn idle_never_reports_a_remaining_duration() {
        assert_eq!(CopyLabelState::Idle.remaining(Instant::now()), None);
    }

    #[test]
    fn remaining_counts_down_and_saturates_at_zero() {
        let t0 = Instant::now();
        let state = CopyLabelState::Copied(t0);
        let remaining = state.remaining(t0 + Duration::from_millis(500)).unwrap();
        assert_eq!(remaining, Duration::from_millis(1500));

        // Past expiry: saturates rather than underflowing.
        let remaining = state
            .remaining(t0 + CopyLabelState::COPIED_DURATION + Duration::from_secs(1))
            .unwrap();
        assert_eq!(remaining, Duration::ZERO);
    }
}
