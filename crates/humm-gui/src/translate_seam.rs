//! The single translate seam both M6 GUI surfaces drive (F-031/F-063/F-085,
//! design spec §7.3/§7.4): the viewer's Translate button (Task 15) and the
//! live-recording window's translation panel (Task 17).
//!
//! Both surfaces spawn an LLM translator, enqueue transcripts, forward live
//! option edits, and stop on close — but the *only* concrete translator is
//! [`humm_core::openai::TranslatorHandle`], which needs a live HTTP endpoint.
//! So this module factors the surface-facing contract into two object-safe
//! traits — [`TranslateSpawner`] (create a worker) and [`TranslateControl`]
//! (drive one) — with a real implementation over `TranslatorHandle`
//! ([`RealTranslateSpawner`]) and a hermetic, network-free test double
//! ([`ScriptedTranslateSpawner`]) an `egui_kittest` harness injects to script
//! translations synchronously, exactly the way `services::TestDeps` already
//! injects a fake `EngineFactory`/`ArtifactEnsurer`.
//!
//! # Interior mutability
//!
//! The result callback (this module's [`OnResult`] alias) is an `FnMut`, since
//! the real worker invokes it repeatedly and it typically mutates captured
//! state. But [`TranslateControl::enqueue`] takes `&self` (a control is shared
//! behind the surface's own state), so the scripted double — which must call
//! that same `FnMut` *synchronously* from `enqueue` — wraps it in a [`Mutex`]
//! to get interior mutability. The real implementation has no such need: it
//! hands the `FnMut` straight to the [`humm_core::openai::TranslatorHandle`]
//! worker thread, which owns it outright.

use std::sync::{Arc, Mutex};

use humm_core::openai::{TranslatorConfig, TranslatorHandle};

/// The per-item result callback a spawned worker invokes:
/// `on_result(translation, id)`, with `id` echoed from the
/// [`TranslateControl::enqueue`] call that produced it. Aliased to keep the
/// seam's signatures clippy-quiet (`clippy::type_complexity`), the same alias
/// `openai::translate`'s own tests use for this exact callback shape.
type OnResult = Box<dyn FnMut(String, Option<i64>) + Send>;

/// Spawns an LLM-translation worker for one surface (viewer or recording
/// window). The one seam a kittest harness swaps to run a translation flow
/// end to end without touching the network.
pub trait TranslateSpawner: Send + Sync {
    /// Spawns a worker from `cfg`, reporting each translation back through
    /// `on_result(translation, id)` — the `id` echoed unchanged from the
    /// [`TranslateControl::enqueue`] call that produced it. Returns the
    /// [`TranslateControl`] the surface drives.
    fn spawn(&self, cfg: TranslatorConfig, on_result: OnResult) -> Box<dyn TranslateControl>;
}

/// A running translation worker's control surface (F-031/F-063/F-085): enqueue
/// transcripts, swap the model/prompt mid-session, and stop.
pub trait TranslateControl: Send {
    /// Queues one transcript for translation, echoing `id` back through the
    /// spawner's `on_result` callback when its translation lands.
    fn enqueue(&self, text: String, id: Option<i64>);

    /// Swaps the model/prompt used for subsequent translations (the live
    /// advanced-settings sheet's options-changed edit, Task 17).
    fn update_options(&self, model: String, prompt: String);

    /// Stops the worker. Consumes the control (`self: Box<Self>`) so no
    /// further [`Self::enqueue`]/[`Self::update_options`] call is possible
    /// after stop.
    fn stop(self: Box<Self>);
}

/// The production [`TranslateSpawner`]: every worker is a real
/// [`TranslatorHandle`] issuing `/chat/completions` requests.
pub struct RealTranslateSpawner;

impl TranslateSpawner for RealTranslateSpawner {
    fn spawn(&self, cfg: TranslatorConfig, on_result: OnResult) -> Box<dyn TranslateControl> {
        Box::new(RealTranslateControl {
            handle: TranslatorHandle::spawn(cfg, on_result),
        })
    }
}

/// A [`TranslateControl`] over a live [`TranslatorHandle`].
struct RealTranslateControl {
    handle: TranslatorHandle,
}

impl TranslateControl for RealTranslateControl {
    fn enqueue(&self, text: String, id: Option<i64>) {
        self.handle.enqueue(text, id);
    }

    fn update_options(&self, model: String, prompt: String) {
        self.handle.update_options(model, prompt);
    }

    fn stop(self: Box<Self>) {
        // `stop_detached` (not `stop`): the GUI close path must return
        // promptly, never block the UI thread on an in-flight request's
        // timeout (design spec §7.3/§7.4 lifecycle; `TranslatorHandle`'s own
        // `stop`/`stop_detached` doc comments). The worker still drains and
        // reports everything queued before the `Stop` it just sent.
        let RealTranslateControl { handle } = *self;
        handle.stop_detached();
    }
}

/// A mapper from a source transcript to its scripted "translation".
type Mapper = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// What a [`ScriptedTranslateSpawner`]'s controls recorded, shared back to the
/// test through [`ScriptedTranslateSpawner::state`] for inspection.
#[derive(Debug, Default)]
pub struct ScriptedState {
    /// Every `(model, prompt)` passed to [`TranslateControl::update_options`],
    /// in call order.
    pub update_options: Vec<(String, String)>,
    /// Whether [`TranslateControl::stop`] was called on a spawned control.
    pub stopped: bool,
}

/// A hermetic [`TranslateSpawner`] test double (kittest, design spec §10): each
/// spawned control translates via a caller-supplied `mapper` and invokes
/// `on_result` **synchronously on the enqueueing thread**, so a test observes
/// the translation the instant it enqueues — no worker thread, no channel, no
/// network. `update_options`/`stop` calls are recorded into shared
/// [`ScriptedState`] a test reads via [`Self::state`].
pub struct ScriptedTranslateSpawner {
    mapper: Mapper,
    state: Arc<Mutex<ScriptedState>>,
}

impl ScriptedTranslateSpawner {
    /// Builds a spawner whose controls translate each text through `mapper`.
    pub fn new(mapper: Mapper) -> Self {
        ScriptedTranslateSpawner {
            mapper,
            state: Arc::new(Mutex::new(ScriptedState::default())),
        }
    }

    /// A spawner whose "translation" is the input uppercased — a visible,
    /// order-preserving transform that makes a scripted round-trip easy to
    /// assert.
    pub fn uppercase() -> Self {
        Self::new(Arc::new(|text: &str| text.to_uppercase()))
    }

    /// A spawner whose "translation" is the input unchanged — the neutral
    /// default for harnesses that spawn a translator but don't assert on its
    /// output.
    pub fn identity() -> Self {
        Self::new(Arc::new(|text: &str| text.to_string()))
    }

    /// The shared state its spawned controls record into — a test clones this
    /// before spawning (or reuses this same spawner afterward) to inspect the
    /// recorded `update_options`/`stop` calls.
    pub fn state(&self) -> Arc<Mutex<ScriptedState>> {
        Arc::clone(&self.state)
    }
}

impl TranslateSpawner for ScriptedTranslateSpawner {
    fn spawn(&self, _cfg: TranslatorConfig, on_result: OnResult) -> Box<dyn TranslateControl> {
        Box::new(ScriptedTranslateControl {
            mapper: Arc::clone(&self.mapper),
            on_result: Mutex::new(on_result),
            state: Arc::clone(&self.state),
        })
    }
}

/// A [`TranslateControl`] that maps each enqueued text and calls `on_result`
/// synchronously (see the module doc's "Interior mutability" note for why the
/// `FnMut` is behind a [`Mutex`]).
struct ScriptedTranslateControl {
    mapper: Mapper,
    on_result: Mutex<OnResult>,
    state: Arc<Mutex<ScriptedState>>,
}

impl TranslateControl for ScriptedTranslateControl {
    fn enqueue(&self, text: String, id: Option<i64>) {
        let translated = (self.mapper)(&text);
        let mut on_result = self
            .on_result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        on_result(translated, id);
    }

    fn update_options(&self, model: String, prompt: String) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .update_options
            .push((model, prompt));
    }

    fn stop(self: Box<Self>) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .stopped = true;
    }
}

/// The shared state a [`DeferredScriptedTranslateSpawner`]'s control and its
/// test-held [`DeferredScriptedTranslateSpawner::flush`] both touch: the queue
/// of not-yet-translated `(text, id)` pairs and the spawned worker's
/// `on_result` (captured at spawn). Behind an [`Arc<Mutex<..>>`] so the test
/// thread (which calls `flush`) and the forwarder thread (which calls
/// `enqueue`) can both reach it.
#[derive(Default)]
struct DeferredShared {
    /// `(text, id)` pairs enqueued but not yet flushed, in enqueue order.
    queue: Vec<(String, Option<i64>)>,
    /// The spawned worker's result callback, set at [`TranslateSpawner::spawn`]
    /// (`None` until a control has been spawned). `flush` invokes it for each
    /// queued item.
    on_result: Option<OnResult>,
}

/// A [`TranslateSpawner`] test double like [`ScriptedTranslateSpawner`], except
/// its controls **defer** every translation instead of running it synchronously
/// from `enqueue`: each `enqueue` records `(text, id)` into a shared queue, and
/// the callback fires only when a test calls [`Self::flush`] at a moment of its
/// choosing. This is what lets a kittest exercise the **late-arrival** path
/// (design spec §7.4, F-063): translations that complete *after* the live
/// session's Stop must still reach the panel/store/export while the recording
/// window is open — a flow the synchronous [`ScriptedTranslateSpawner`] (which
/// translates the instant it enqueues, always mid-session) structurally cannot
/// reproduce.
///
/// Re-entrancy: `flush` invokes `on_result` on the **calling** thread, holding
/// only its own shared lock; the callback obeys the same rule the production
/// worker thread does — it only sends into channels / does direct store+applier
/// work, never a call back into the control — so nothing re-enters this
/// spawner's lock. `update_options`/`stop` are recorded into a separate
/// [`ScriptedState`] a test reads via [`Self::state`], exactly as the
/// synchronous double does.
pub struct DeferredScriptedTranslateSpawner {
    mapper: Mapper,
    shared: Arc<Mutex<DeferredShared>>,
    state: Arc<Mutex<ScriptedState>>,
}

impl DeferredScriptedTranslateSpawner {
    /// Builds a deferred spawner whose controls will translate each queued text
    /// through `mapper` when [`Self::flush`] fires.
    pub fn new(mapper: Mapper) -> Self {
        DeferredScriptedTranslateSpawner {
            mapper,
            shared: Arc::new(Mutex::new(DeferredShared::default())),
            state: Arc::new(Mutex::new(ScriptedState::default())),
        }
    }

    /// The shared `update_options`/`stop` record its spawned control writes into
    /// (see [`ScriptedTranslateSpawner::state`]).
    pub fn state(&self) -> Arc<Mutex<ScriptedState>> {
        Arc::clone(&self.state)
    }

    /// How many `(text, id)` pairs are queued but not yet flushed. A test polls
    /// this to know every enqueue has reached the control (enqueues arrive
    /// asynchronously, via the [`crate::live_translation::LiveTranslator`]
    /// forwarder thread) before calling [`Self::flush`].
    pub fn pending_len(&self) -> usize {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .queue
            .len()
    }

    /// Invokes the spawned worker's `on_result` for every queued `(text, id)`
    /// (each `text` mapped through this spawner's `mapper`), in enqueue order,
    /// on the calling thread — then clears the queue. Returns how many items
    /// were flushed. A no-op returning `0` if no control has been spawned yet.
    pub fn flush(&self) -> usize {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let DeferredShared { queue, on_result } = &mut *shared;
        let Some(callback) = on_result.as_mut() else {
            return 0;
        };
        let items = std::mem::take(queue);
        let flushed = items.len();
        for (text, id) in items {
            let translated = (self.mapper)(&text);
            callback(translated, id);
        }
        flushed
    }
}

impl TranslateSpawner for DeferredScriptedTranslateSpawner {
    fn spawn(&self, _cfg: TranslatorConfig, on_result: OnResult) -> Box<dyn TranslateControl> {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .on_result = Some(on_result);
        Box::new(DeferredScriptedTranslateControl {
            shared: Arc::clone(&self.shared),
            state: Arc::clone(&self.state),
        })
    }
}

/// A [`TranslateControl`] whose `enqueue` **queues** `(text, id)` (rather than
/// translating synchronously) for a later [`DeferredScriptedTranslateSpawner::flush`].
struct DeferredScriptedTranslateControl {
    shared: Arc<Mutex<DeferredShared>>,
    state: Arc<Mutex<ScriptedState>>,
}

impl TranslateControl for DeferredScriptedTranslateControl {
    fn enqueue(&self, text: String, id: Option<i64>) {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .queue
            .push((text, id));
    }

    fn update_options(&self, model: String, prompt: String) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .update_options
            .push((model, prompt));
    }

    fn stop(self: Box<Self>) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .stopped = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::openai::OpenAiAuth;

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

    #[test]
    fn scripted_enqueue_invokes_on_result_synchronously_with_same_id() {
        let spawner = ScriptedTranslateSpawner::uppercase();
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let control = spawner.spawn(
            dummy_cfg(),
            Box::new(move |text, id| {
                sink.lock().unwrap().push((text, id));
            }),
        );

        control.enqueue("hello".to_string(), Some(42));

        // Synchronous: the callback has already run by the time `enqueue`
        // returns, with the mapped text and the *same* id.
        assert_eq!(
            *received.lock().unwrap(),
            vec![("HELLO".to_string(), Some(42))]
        );
    }

    #[test]
    fn scripted_records_update_options_and_stop() {
        let spawner = ScriptedTranslateSpawner::identity();
        let control = spawner.spawn(dummy_cfg(), Box::new(|_text, _id| {}));

        control.update_options("gpt-4o".to_string(), "New instructions.".to_string());
        control.update_options("gpt-4.1-mini".to_string(), "Newer.".to_string());
        control.stop();

        let state = spawner.state();
        let guard = state.lock().unwrap();
        assert_eq!(
            guard.update_options,
            vec![
                ("gpt-4o".to_string(), "New instructions.".to_string()),
                ("gpt-4.1-mini".to_string(), "Newer.".to_string()),
            ]
        );
        assert!(guard.stopped, "stop must be recorded");
    }

    #[test]
    fn deferred_enqueue_queues_without_calling_on_result_until_flush() {
        let spawner =
            DeferredScriptedTranslateSpawner::new(Arc::new(|text: &str| format!("X:{text}")));
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let control = spawner.spawn(
            dummy_cfg(),
            Box::new(move |text, id| {
                sink.lock().unwrap().push((text, id));
            }),
        );

        control.enqueue("one".to_string(), Some(1));
        control.enqueue("two".to_string(), Some(2));

        // Deferred: nothing has fired yet; both items are queued.
        assert!(
            received.lock().unwrap().is_empty(),
            "no translation fires before flush"
        );
        assert_eq!(spawner.pending_len(), 2, "both enqueues are queued");

        let flushed = spawner.flush();

        assert_eq!(flushed, 2);
        assert_eq!(spawner.pending_len(), 0, "flush drains the queue");
        assert_eq!(
            *received.lock().unwrap(),
            vec![
                ("X:one".to_string(), Some(1)),
                ("X:two".to_string(), Some(2))
            ],
            "flush invokes on_result for every queued item, mapped and in order"
        );
    }

    #[test]
    fn deferred_records_update_options_and_stop_and_flush_without_control_is_noop() {
        let spawner = DeferredScriptedTranslateSpawner::new(Arc::new(|t: &str| t.to_string()));
        // No control spawned yet: flush is a no-op.
        assert_eq!(spawner.flush(), 0, "flush before any spawn does nothing");

        let control = spawner.spawn(dummy_cfg(), Box::new(|_text, _id| {}));
        control.update_options("gpt-4o".to_string(), "New.".to_string());
        control.stop();

        let state = spawner.state();
        let guard = state.lock().unwrap();
        assert_eq!(
            guard.update_options,
            vec![("gpt-4o".to_string(), "New.".to_string())]
        );
        assert!(guard.stopped, "stop is recorded");
    }

    /// Compile-level + prompt-return proof that [`RealTranslateSpawner`]
    /// satisfies the seam, with **no network touched**: `spawn` builds a
    /// [`TranslatorHandle`] whose worker blocks on its first `recv` (no item
    /// is ever enqueued, so no request is ever issued — see
    /// `translate::worker_loop`), and `stop` sends `Stop` and returns without
    /// joining (`stop_detached`), so this returns promptly. Deliberately binds
    /// no TCP, so it stays in the sandbox-green set.
    #[test]
    fn real_spawner_spawn_then_stop_makes_no_request_and_returns_promptly() {
        let spawner = RealTranslateSpawner;
        let control = spawner.spawn(dummy_cfg(), Box::new(|_text, _id| {}));
        // No `enqueue` — the worker never leaves its blocking receive, so no
        // request is attempted against the unroutable base URL above.
        control.stop();
    }
}
