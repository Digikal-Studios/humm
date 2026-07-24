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
