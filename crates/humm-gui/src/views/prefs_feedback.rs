//! Session-only confirmation for Preferences write-through saves.
//!
//! Preferences has no Ok/Cancel (UX_SPEC J-11). A successful discrete commit
//! (checkbox, combo, focus-out) flashes "Saved". A live control such as the
//! font-size drag does not, so a tick does not strobe. A failed `save` is
//! shown on the tab; the log line stays too.
//!
//! The state lives in a thread-local because the commit helpers are also
//! called from unit tests that do not own the Preferences window. The UI
//! thread is the only one that both commits and renders the tab, so the
//! flash the window draws is the one that commit just recorded.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use crate::i18n::tr;

/// How long the "Saved" flash stays up.
pub const SAVED_FLASH: Duration = Duration::from_millis(1500);

#[derive(Default)]
struct Feedback {
    saved_at: Option<Instant>,
    error: Option<String>,
}

thread_local! {
    static FEEDBACK: RefCell<Feedback> = RefCell::new(Feedback::default());
}

/// Records a settings-file save. `flash` is for a discrete commit. A live
/// slider passes `false`: success clears any previous flash and any error,
/// and does not show "Saved".
pub fn record_save(result: Result<(), impl std::fmt::Display>, flash: bool, log_context: &str) {
    match result {
        Ok(()) => FEEDBACK.with(|cell| {
            let mut feedback = cell.borrow_mut();
            feedback.error = None;
            feedback.saved_at = flash.then(Instant::now);
        }),
        Err(err) => {
            log::warn!("{log_context}: {err}");
            FEEDBACK.with(|cell| {
                let mut feedback = cell.borrow_mut();
                feedback.saved_at = None;
                feedback.error = Some(err.to_string());
            });
        }
    }
}

/// Drops the "Saved" flash when the user starts another edit, before that
/// edit has committed.
pub fn clear_flash() {
    FEEDBACK.with(|cell| cell.borrow_mut().saved_at = None);
}

/// Draws the current flash or save error at the top of the open tab.
pub fn show(ui: &mut egui::Ui) {
    let (error, saved_at) = FEEDBACK.with(|cell| {
        let mut feedback = cell.borrow_mut();
        if feedback
            .saved_at
            .is_some_and(|at| at.elapsed() >= SAVED_FLASH)
        {
            feedback.saved_at = None;
        }
        (feedback.error.clone(), feedback.saved_at)
    });
    if let Some(error) = error {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{} {error}", tr("prefs-save-failed")),
        );
    } else if let Some(saved_at) = saved_at {
        ui.label(tr("prefs-saved"));
        ui.ctx()
            .request_repaint_after(SAVED_FLASH.saturating_sub(saved_at.elapsed()));
    }
}
