//! The toast stack (F-004/F-026, Task 8): bottom-right, auto-dismissing
//! notices for terminal task events — `Completed` (success), `Failed`
//! (error, with a click-through detail popover), `Canceled` (info).
//! `StoreWriteFailed` is deliberately *not* a toast (see `app.rs`'s
//! `StoreHealthBanner`) — it needs to persist until dismissed, which a
//! 5-second auto-expiring toast cannot do.
//!
//! [`ToastStack::push`]/[`ToastStack::expire`] take an explicit `now:
//! Instant` parameter (the "injected clock" the brief asks for) rather than
//! calling `Instant::now()` internally, so a unit test can advance time
//! deterministically via `Instant::now() + Duration` without any real
//! sleeping or a trait-object clock abstraction.

use std::time::{Duration, Instant};

use humm_core::domain::{Status, TranscriptionId};

use crate::i18n::tr;
use crate::viewmodel::TasksVm;

/// How long a toast stays visible before auto-expiring.
pub const TOAST_DURATION: Duration = Duration::from_secs(5);

/// A toast's kind, driving its styling and (for [`ToastKind::TaskFailed`])
/// its click behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastKind {
    /// A task completed successfully.
    Success,
    /// A neutral notice (task canceled).
    Info,
    /// A plain error with no further detail to show (e.g. "could not
    /// restart" or "could not delete") — not clickable.
    Error,
    /// A task's `Failed` terminal event: clicking this toast opens a detail
    /// popover with the full message, an "Open log folder" button, and a
    /// Retry button gated the same way the row context menu's Restart is
    /// (UX §J-1 — Buzz has no analogous UI; this is humm's own addition).
    TaskFailed {
        /// The failed task, so Retry knows which row to restart.
        task_id: TranscriptionId,
        /// The failure's full message (the toast itself shows only its
        /// first line).
        full_message: String,
    },
}

/// One toast: its kind, always-visible message, and push time (for expiry).
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    id: u64,
    /// The toast's kind.
    pub kind: ToastKind,
    /// The always-visible message (a `TaskFailed` toast's first line only —
    /// the full text lives in `kind`, shown only in the detail popover).
    pub message: String,
    pushed_at: Instant,
}

/// The currently-open error detail popover's data, copied out of whichever
/// [`ToastKind::TaskFailed`] toast was clicked — copied rather than
/// referenced so the popover survives its originating toast's own 5-second
/// expiry (the user may click right as it's about to disappear).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDetail {
    /// The failed task.
    pub task_id: TranscriptionId,
    /// The failure's full message.
    pub full_message: String,
}

/// An action a toast/detail popover wants applied — `app.rs` is the
/// intended consumer (this module has no access to `Services`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastAction {
    /// Retry the given (failed) task — same gated flow as the row context
    /// menu's Restart.
    Retry(TranscriptionId),
    /// Open the log folder (`humm_core::paths::log_dir()`).
    OpenLogFolder,
}

/// The toast stack: active toasts (newest last) plus at most one open error
/// detail popover.
#[derive(Debug, Default)]
pub struct ToastStack {
    items: Vec<Toast>,
    next_id: u64,
    /// The currently-open `TaskFailed` detail popover, if any.
    pub detail: Option<ErrorDetail>,
}

impl ToastStack {
    /// Pushes a new toast, returning its id.
    pub fn push(&mut self, kind: ToastKind, message: String, now: Instant) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Toast {
            id,
            kind,
            message,
            pushed_at: now,
        });
        id
    }

    /// Drops every toast older than [`TOAST_DURATION`] as of `now`.
    pub fn expire(&mut self, now: Instant) {
        self.items
            .retain(|toast| now.saturating_duration_since(toast.pushed_at) < TOAST_DURATION);
    }

    /// Currently-active toasts, oldest first.
    pub fn items(&self) -> &[Toast] {
        &self.items
    }

    /// Opens the error detail popover for `id`'s toast, copying out its
    /// `TaskFailed` data. A no-op if `id` doesn't exist or isn't a
    /// `TaskFailed` toast.
    pub fn open_detail(&mut self, id: u64) {
        if let Some(Toast {
            kind:
                ToastKind::TaskFailed {
                    task_id,
                    full_message,
                },
            ..
        }) = self.items.iter().find(|t| t.id == id)
        {
            self.detail = Some(ErrorDetail {
                task_id: *task_id,
                full_message: full_message.clone(),
            });
        }
    }

    /// Closes the open error detail popover, if any.
    pub fn close_detail(&mut self) {
        self.detail = None;
    }
}

/// Renders the toast stack (bottom-right overlay) and, if open, the error
/// detail popover — returning any [`ToastAction`]s the user requested this
/// frame. Expires stale toasts (`now`) before rendering.
pub fn ui(
    ctx: &egui::Context,
    stack: &mut ToastStack,
    vm: &TasksVm,
    now: Instant,
) -> Vec<ToastAction> {
    stack.expire(now);
    let mut actions = Vec::new();
    let mut open_detail_ids = Vec::new();

    egui::Area::new(egui::Id::new("humm-toast-stack"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                for toast in stack.items() {
                    let clickable = matches!(toast.kind, ToastKind::TaskFailed { .. });
                    let frame = egui::Frame::popup(ui.style());
                    let response = frame
                        .show(ui, |ui| {
                            ui.set_min_width(220.0);
                            ui.label(&toast.message);
                        })
                        .response;
                    if clickable {
                        let response = ui.interact(
                            response.rect,
                            ui.id().with(("humm-toast-click", toast.id)),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            open_detail_ids.push(toast.id);
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        });

    // The detail-open request is collected above (inside the `Area`
    // closure, which can't itself hold `&mut stack` — `stack.items()` is
    // already borrowed immutably there) and applied here.
    for id in open_detail_ids {
        stack.open_detail(id);
    }

    if let Some(detail) = stack.detail.clone() {
        let retryable = vm
            .sorted_rows()
            .iter()
            .find(|row| row.id == detail.task_id)
            .map(|row| {
                matches!(
                    row.status,
                    Status::Failed | Status::Canceled | Status::Skipped
                )
            })
            .unwrap_or(false);

        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("humm-toast-detail")).show(ctx, |ui| {
            ui.heading(tr("toast-detail-title"));
            ui.label(&detail.full_message);
            ui.horizontal(|ui| {
                if ui.button(tr("toast-detail-open-log")).clicked() {
                    actions.push(ToastAction::OpenLogFolder);
                }
                if ui
                    .add_enabled(retryable, egui::Button::new(tr("toast-detail-retry")))
                    .clicked()
                {
                    actions.push(ToastAction::Retry(detail.task_id));
                    close = true;
                }
                if ui.button(tr("toast-detail-close")).clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            stack.close_detail();
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_assigns_increasing_ids() {
        let mut stack = ToastStack::default();
        let now = Instant::now();
        let a = stack.push(ToastKind::Info, "one".to_string(), now);
        let b = stack.push(ToastKind::Info, "two".to_string(), now);
        assert_ne!(a, b);
        assert_eq!(stack.items().len(), 2);
    }

    #[test]
    fn expire_drops_toasts_older_than_the_duration() {
        let mut stack = ToastStack::default();
        let t0 = Instant::now();
        stack.push(ToastKind::Success, "done".to_string(), t0);

        stack.expire(t0 + Duration::from_secs(3));
        assert_eq!(stack.items().len(), 1, "not yet expired");

        stack.expire(t0 + TOAST_DURATION + Duration::from_millis(1));
        assert!(stack.items().is_empty(), "should have expired");
    }

    #[test]
    fn expire_keeps_a_toast_pushed_after_an_older_one_expired() {
        let mut stack = ToastStack::default();
        let t0 = Instant::now();
        stack.push(ToastKind::Info, "old".to_string(), t0);
        let t1 = t0 + Duration::from_secs(4);
        stack.push(ToastKind::Info, "new".to_string(), t1);

        // At t0 + 6s the first toast (pushed at t0) has expired but the
        // second (pushed at t1 = t0+4s) has not.
        stack.expire(t0 + Duration::from_secs(6));
        assert_eq!(stack.items().len(), 1);
        assert_eq!(stack.items()[0].message, "new");
    }

    #[test]
    fn open_detail_copies_task_failed_data() {
        let mut stack = ToastStack::default();
        let id = TranscriptionId::new();
        let toast_id = stack.push(
            ToastKind::TaskFailed {
                task_id: id,
                full_message: "line one\nline two".to_string(),
            },
            "line one".to_string(),
            Instant::now(),
        );

        stack.open_detail(toast_id);

        assert_eq!(
            stack.detail,
            Some(ErrorDetail {
                task_id: id,
                full_message: "line one\nline two".to_string(),
            })
        );
    }

    #[test]
    fn open_detail_is_a_noop_for_a_non_task_failed_toast() {
        let mut stack = ToastStack::default();
        let toast_id = stack.push(ToastKind::Success, "done".to_string(), Instant::now());
        stack.open_detail(toast_id);
        assert!(stack.detail.is_none());
    }

    #[test]
    fn close_detail_clears_it() {
        let mut stack = ToastStack {
            detail: Some(ErrorDetail {
                task_id: TranscriptionId::new(),
                full_message: "boom".to_string(),
            }),
            ..ToastStack::default()
        };
        stack.close_detail();
        assert!(stack.detail.is_none());
    }
}
