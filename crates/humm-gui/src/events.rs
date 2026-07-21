//! The event pump: moves [`TaskEvent`]s from the queue's `flume` channel
//! (which nothing on the UI thread can block-wait on without freezing the
//! window) onto a plain, `egui`-agnostic buffer that `HummApp`'s
//! `eframe::App::ui` implementation drains every frame — design spec §4.
//!
//! A dedicated `std::thread` blocks on `events.recv()` and, for each event,
//! pushes it onto a shared `Mutex<VecDeque<_>>` and calls
//! `ctx.request_repaint()` so a native window wakes promptly instead of
//! waiting for its next natural repaint. The thread exits on its own once
//! the channel disconnects (queue shutdown or every sender dropped) — no
//! explicit stop signal is needed for this scaffold.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use humm_core::queue::TaskEvent;

/// Shared buffer the pump thread appends to and `HummApp::ui` drains.
pub type SharedEventQueue = Arc<Mutex<VecDeque<TaskEvent>>>;

/// Spawns the pump thread and returns the shared buffer it feeds.
///
/// `ctx` is cloned into the thread (an `egui::Context` is cheap to clone —
/// it's an `Arc` internally) so every forwarded event can request a repaint
/// without borrowing anything from the caller.
pub fn spawn_event_pump(
    events: flume::Receiver<TaskEvent>,
    ctx: egui::Context,
) -> SharedEventQueue {
    let queue: SharedEventQueue = Arc::new(Mutex::new(VecDeque::new()));
    let queue_for_thread = Arc::clone(&queue);
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            queue_for_thread
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push_back(event);
            ctx.request_repaint();
        }
    });
    queue
}

/// Drains every event currently buffered, in arrival order.
pub fn drain(queue: &SharedEventQueue) -> Vec<TaskEvent> {
    queue
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .drain(..)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::domain::TranscriptionId;
    use humm_core::queue::TaskEventKind;
    use std::time::{Duration, Instant};

    #[test]
    fn forwarded_events_are_drained_in_order() {
        let (tx, rx) = flume::unbounded();
        let ctx = egui::Context::default();
        let queue = spawn_event_pump(rx, ctx);

        let id = TranscriptionId::new();
        tx.send(TaskEvent {
            task_id: id,
            kind: TaskEventKind::Queued,
        })
        .expect("send queued");
        tx.send(TaskEvent {
            task_id: id,
            kind: TaskEventKind::Started,
        })
        .expect("send started");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut drained = Vec::new();
        while drained.len() < 2 && Instant::now() < deadline {
            drained.extend(drain(&queue));
            if drained.len() < 2 {
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        assert_eq!(drained.len(), 2, "expected both events to be forwarded");
        assert_eq!(drained[0].kind, TaskEventKind::Queued);
        assert_eq!(drained[1].kind, TaskEventKind::Started);
    }

    #[test]
    fn drain_of_empty_queue_is_empty() {
        let (_tx, rx) = flume::unbounded();
        let queue = spawn_event_pump(rx, egui::Context::default());
        assert!(drain(&queue).is_empty());
    }
}
