//! [`LiveTranslator`] (F-063, design spec §7.4): the recording window's
//! owner of one session's LLM-translation worker, wrapping the
//! [`crate::translate_seam::TranslateControl`] seam behind a small forwarder
//! thread.
//!
//! ## Why a forwarder thread
//!
//! The translation worker has two callers that live on *different* threads:
//! the recording window's UI thread (which forwards mid-session option edits
//! and stops the worker at window close / next Record click) and the live
//! **session** thread (which enqueues each accepted utterance's raw text —
//! Buzz `on_next_transcription:969-970`). [`TranslateControl::enqueue`]/
//! [`TranslateControl::update_options`] take `&self`, but
//! [`TranslateControl::stop`] takes `self: Box<Self>` — so the control cannot
//! simply be shared behind an `Arc` (an `Arc` can never yield the owned
//! `Box<Self>` `stop` consumes). Rather than change the seam (which the
//! scripted kittest double relies on verbatim), this module keeps the control
//! **wholly owned by one forwarder thread** and drives it through a plain
//! channel: the UI thread and the session thread each hold a cheap,
//! `Clone`-able [`std::sync::mpsc::Sender`] handle, and the forwarder thread
//! is the single owner that ever touches the control — so `enqueue`,
//! `update_options`, and the consuming `stop` all happen on that one thread,
//! serially, with no shared-ownership gymnastics.
//!
//! ## The translation-panel channel (F-063 late-arrival fix, spec §7.4)
//!
//! The worker's `on_result` callback (built in `app.rs::build_live_translation`)
//! does three things per arrived translation: persist it onto the segment row,
//! write the `.translated` export, and update the **translation panel**. The
//! first two hold their `Arc`s directly, so they land regardless of session
//! state. The panel, however, is UI state — it can only be written on the UI
//! thread. So the panel text travels through a dedicated [`mpsc`] channel whose
//! **receiver this `LiveTranslator` owns** ([`Self::panel_rx`]) and whose sender
//! the callback holds. The recording window drains it every frame
//! ([`Self::drain_panels`]) — crucially **independent of the live session**.
//!
//! This is the fix for the finding that late translations were lost after Stop:
//! the session's own `SessionMessage` channel dies with the session (its
//! receiver is dropped when `finish_live_session` sets `RecordingVm::session =
//! None`), so a panel update routed through *that* channel after Stop reached a
//! dropped receiver and vanished. This translator — and therefore this panel
//! receiver — **outlives** the session (torn down only at window close / next
//! Record click), so a translation that completes after Stop (common with real
//! network latency, the session tail) still reaches the panel while the window
//! is open.
//!
//! ## Lifecycle (Buzz `_do_close:1213-1218`, spec §7.4)
//!
//! - **Stop (of the recording, not the window)** does *not* stop the
//!   translator: queued items keep processing and late translations still
//!   land in the panel/store/export (Buzz's sentinel-after-queue semantics).
//!   The session thread's enqueue handle is simply dropped when the session
//!   ends; the worker drains whatever was already queued, and each result's
//!   panel text still arrives through [`Self::panel_rx`] (which this translator
//!   owns, so it is still alive after Stop — see the section above).
//! - **Window close / next Record click** calls [`LiveTranslator::stop`],
//!   which sends [`ForwarderMsg::Stop`] — the forwarder consumes the control
//!   via `stop` (which is `stop_detached` for the real worker, non-blocking —
//!   the UI thread never waits on an in-flight request) and exits. Dropping
//!   this `LiveTranslator` also drops [`Self::panel_rx`], so a straggler that
//!   the *previous* session's worker emits after teardown is correctly
//!   discarded rather than bleeding into the next session's panel.
//! - [`Drop`] is a backstop: it sends `Stop` too, so a `LiveTranslator` that
//!   goes away without an explicit `stop` still tears its worker down. The
//!   forwarder thread is **detached** (never joined), so no teardown path
//!   ever blocks the UI thread.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use crate::translate_seam::TranslateControl;

/// The clone-able, `Send + Sync` enqueue handle the live-session thread holds
/// (via [`crate::live_session::LiveSessionRequest::translation_enqueue`]) to
/// enqueue each accepted utterance's raw filtered text for translation, tagged
/// with the persisted segment row id. A thin closure over the forwarder
/// channel's [`Sender`] — never the control itself (see the module doc).
pub type TranslationEnqueue = Box<dyn Fn(String, Option<i64>) + Send + Sync>;

/// One message to the forwarder thread. `Enqueue`/`UpdateOptions` map 1:1 onto
/// the same-named [`TranslateControl`] calls; `Stop` consumes the control and
/// ends the thread.
enum ForwarderMsg {
    /// Enqueue one transcript for translation, echoing `id` back through the
    /// worker's `on_result` (`(text, id)`).
    Enqueue(String, Option<i64>),
    /// Swap the model/prompt for subsequent translations (`(model, prompt)`).
    UpdateOptions(String, String),
    /// Stop the worker (detached) and end the forwarder thread.
    Stop,
}

/// Owns one live-recording session's translation worker on a forwarder thread
/// (see the module doc). Held by [`crate::recording::RecordingVm`] for the
/// session's lifetime; cloned enqueue handles ([`Self::enqueue_handle`]) ride
/// to the session thread.
pub struct LiveTranslator {
    tx: Sender<ForwarderMsg>,
    /// Receives fully-applied translation-panel text (whole-panel replaces)
    /// from the worker's `on_result`, drained by the recording window every
    /// frame it is open via [`Self::drain_panels`] — **independent of the live
    /// session**. Owning it here (not on the session's `ActiveSession`, whose
    /// receiver dies at Stop) is exactly what lets a translation completing
    /// after Stop still reach the panel while the window is open (F-063, spec
    /// §7.4 — see the module doc's "translation-panel channel" section).
    panel_rx: Receiver<String>,
    /// The forwarder thread, kept only so it is not joined at construction;
    /// it is **never** joined (detached teardown — see the module doc), so a
    /// window close never blocks the UI thread.
    _thread: JoinHandle<()>,
}

impl LiveTranslator {
    /// Spawns the forwarder thread that owns `control` for the rest of the
    /// session, returning the handle the UI thread holds. The forwarder loops
    /// on the channel, driving `control` per message, until a [`ForwarderMsg::Stop`]
    /// (or the channel disconnecting — every sender dropped) — at which point
    /// it consumes the control via [`TranslateControl::stop`] and exits.
    ///
    /// `panel_rx` is the receiving end of the translation-panel channel whose
    /// sender the worker's `on_result` holds (`app.rs::build_live_translation`
    /// creates the pair). This translator owns the receiver so it outlives the
    /// session — see the module doc's "translation-panel channel" section and
    /// [`Self::drain_panels`].
    pub fn new(control: Box<dyn TranslateControl>, panel_rx: Receiver<String>) -> Self {
        let (tx, rx) = mpsc::channel::<ForwarderMsg>();
        let thread = std::thread::spawn(move || {
            // `Option` so the consuming `stop` can `take` the control out;
            // after that it's `None` and the loop has already broken.
            let mut control = Some(control);
            loop {
                match rx.recv() {
                    Ok(ForwarderMsg::Enqueue(text, id)) => {
                        if let Some(control) = &control {
                            control.enqueue(text, id);
                        }
                    }
                    Ok(ForwarderMsg::UpdateOptions(model, prompt)) => {
                        if let Some(control) = &control {
                            control.update_options(model, prompt);
                        }
                    }
                    // A `Stop`, or every sender dropped (`Err`): either way,
                    // tear the worker down (detached) and end the thread.
                    // Handling the disconnect case too guarantees the worker
                    // is never leaked even if `stop` is somehow never called.
                    Ok(ForwarderMsg::Stop) | Err(_) => {
                        if let Some(control) = control.take() {
                            control.stop();
                        }
                        break;
                    }
                }
            }
        });
        LiveTranslator {
            tx,
            panel_rx,
            _thread: thread,
        }
    }

    /// Drains every translation-panel update that has arrived since the last
    /// call — non-blocking `try_iter`, the same per-frame shape
    /// [`crate::live_session::ActiveSession`]'s own channel is drained with.
    /// Each item is a whole-panel replace (already `strip_newlines`'d,
    /// mode-dispatched, and merged by the off-UI-thread applier), so the caller
    /// applies them in order and the last one wins. Called by the recording
    /// window every frame it is open, regardless of session presence, so a
    /// translation completing after Stop still lands (F-063).
    pub fn drain_panels(&self) -> impl Iterator<Item = String> + '_ {
        self.panel_rx.try_iter()
    }

    /// A clone-able enqueue handle for the session thread: enqueues `(text,
    /// id)` for translation (Buzz `on_next_transcription:969-970` — the raw
    /// filtered utterance, tagged with its persisted segment row id). A send
    /// failure (the forwarder already stopped) is silently dropped — a stopped
    /// worker simply translates nothing more.
    pub fn enqueue_handle(&self) -> TranslationEnqueue {
        let tx = self.tx.clone();
        Box::new(move |text: String, id: Option<i64>| {
            let _ = tx.send(ForwarderMsg::Enqueue(text, id));
        })
    }

    /// Forwards a mid-session model/prompt edit to the worker (Buzz's
    /// options-changed signal → translator, `translator.py:36-38`). A no-op
    /// send failure if the worker already stopped.
    pub fn update_options(&self, model: String, prompt: String) {
        let _ = self.tx.send(ForwarderMsg::UpdateOptions(model, prompt));
    }

    /// Stops the worker (detached, non-blocking — the forwarder calls
    /// [`TranslateControl::stop`], which is `stop_detached` for the real
    /// worker) and ends the forwarder thread. Idempotent: a second `stop`
    /// (or [`Drop`] after an explicit `stop`) just fails to send and is
    /// ignored.
    pub fn stop(&self) {
        let _ = self.tx.send(ForwarderMsg::Stop);
    }
}

impl Drop for LiveTranslator {
    /// Backstop for [`Self::stop`]: a `LiveTranslator` dropped without an
    /// explicit stop still tears its worker down (the forwarder receives this
    /// `Stop`, or — if this send races the thread already exiting — the
    /// channel-disconnect arm handles it). Never joins the thread (detached).
    fn drop(&mut self) {
        let _ = self.tx.send(ForwarderMsg::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate_seam::{ScriptedTranslateSpawner, TranslateSpawner};
    use humm_core::openai::{OpenAiAuth, TranslatorConfig};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn dummy_cfg() -> TranslatorConfig {
        TranslatorConfig {
            auth: OpenAiAuth {
                base_url: Some("http://127.0.0.1:0".to_string()),
                api_key: "unused-in-scripted-tests".to_string(),
            },
            model: "gpt-4o-mini".to_string(),
            prompt: "Translate.".to_string(),
        }
    }

    /// Polls `cond` (5 ms cadence) until true or a 5 s deadline — the
    /// established background-thread wait idiom, since the forwarder runs the
    /// scripted control on its own thread (asynchronously to the test).
    fn wait_until(message: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out: {message}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn enqueue_handle_forwards_to_the_control_on_the_forwarder_thread() {
        // The scripted control invokes `on_result` synchronously from
        // `enqueue` (on the forwarder thread) — the `on_result` here only
        // pushes into a channel, never back into the control (the seam's
        // re-entrancy rule). Proves the enqueue handle reaches the control.
        type ResultLog = Arc<Mutex<Vec<(String, Option<i64>)>>>;
        let spawner = ScriptedTranslateSpawner::uppercase();
        let received: ResultLog = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let control = spawner.spawn(
            dummy_cfg(),
            Box::new(move |text, id| {
                sink.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((text, id));
            }),
        );
        let (_panel_tx, panel_rx) = mpsc::channel::<String>();
        let translator = LiveTranslator::new(control, panel_rx);

        let enqueue = translator.enqueue_handle();
        enqueue("hello".to_string(), Some(7));

        wait_until("the scripted translation to arrive", || {
            !received
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty()
        });
        assert_eq!(
            *received.lock().unwrap_or_else(|p| p.into_inner()),
            vec![("HELLO".to_string(), Some(7))]
        );
    }

    #[test]
    fn drain_panels_yields_what_the_worker_posts_to_the_panel_channel() {
        // The panel-channel path (F-063): the `on_result` posts each mapped
        // translation into the panel sender; `drain_panels` (drained by the UI
        // thread) yields it. This receiver lives with the translator, not the
        // session — the mechanism that keeps late arrivals reaching the panel.
        let (panel_tx, panel_rx) = mpsc::channel::<String>();
        let spawner = ScriptedTranslateSpawner::uppercase();
        let control = spawner.spawn(
            dummy_cfg(),
            Box::new(move |text, _id| {
                // Only a channel send — never a call back into the control
                // (the seam's re-entrancy rule).
                let _ = panel_tx.send(text);
            }),
        );
        let translator = LiveTranslator::new(control, panel_rx);

        let enqueue = translator.enqueue_handle();
        enqueue("hi".to_string(), None);

        wait_until(
            "the panel text to arrive on the translator's channel",
            || translator.drain_panels().next() == Some("HI".to_string()),
        );
    }

    #[test]
    fn update_options_and_stop_reach_the_control() {
        let spawner = ScriptedTranslateSpawner::identity();
        let state = spawner.state();
        let control = spawner.spawn(dummy_cfg(), Box::new(|_text, _id| {}));
        let (_panel_tx, panel_rx) = mpsc::channel::<String>();
        let translator = LiveTranslator::new(control, panel_rx);

        translator.update_options("gpt-4o".to_string(), "New instructions.".to_string());
        wait_until("update_options to be recorded", || {
            !state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .update_options
                .is_empty()
        });
        assert_eq!(
            state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .update_options,
            vec![("gpt-4o".to_string(), "New instructions.".to_string())]
        );

        translator.stop();
        wait_until("stop to be recorded on the control", || {
            state.lock().unwrap_or_else(|p| p.into_inner()).stopped
        });
    }

    #[test]
    fn drop_without_explicit_stop_still_stops_the_control() {
        let spawner = ScriptedTranslateSpawner::identity();
        let state = spawner.state();
        let control = spawner.spawn(dummy_cfg(), Box::new(|_text, _id| {}));
        let (_panel_tx, panel_rx) = mpsc::channel::<String>();
        let translator = LiveTranslator::new(control, panel_rx);

        drop(translator);

        wait_until("the dropped translator to stop its control", || {
            state.lock().unwrap_or_else(|p| p.into_inner()).stopped
        });
    }
}
