//! [`WhisperEngine`]: the whisper-rs implementation of
//! [`humm_core::boundary::TranscriptionEngine`].
//!
//! Loads a ggml model into a [`WhisperContext`] (Metal + CoreML on Apple
//! Silicon, CPU elsewhere) and runs `whisper_full` per transcription, wiring
//! whisper.cpp's progress / new-segment / abort callbacks to the core's
//! [`SegmentSink`] and [`CancelToken`]. FullParams mirror Buzz's whisper-cli
//! invocation (`buzz/transcriber/whisper_cpp.py:167-196`); word-level mode
//! reconstructs per-word/character segments from token data via
//! [`crate::words::group_words`].
//!
//! ## whisper.cpp version
//! Built against the whisper.cpp vendored by whisper-rs-sys 0.15
//! (**whisper.cpp 1.8.3**); see [`whisper_rs::WHISPER_CPP_VERSION`].
//!
//! ## VAD timeline (F-045) — empirical finding
//! whisper-rs 0.16's `WhisperState::full` calls `whisper_full_with_state`, which
//! does **not** run VAD — whisper.cpp's VAD preprocessing lives only in the
//! context-level `whisper_full`, which whisper-rs does not wrap. So `enable_vad`
//! on `FullParams` is a no-op here. humm therefore runs VAD itself via the
//! standalone [`whisper_rs::WhisperVadContext`], transcribes the speech-only
//! audio, and remaps timestamps back to the original timeline
//! ([`VadRemap`]) — the parity realization of Buzz's `_make_offset_mapper`. See
//! the note on [`WhisperEngine::transcribe`].
//!
//! ## Live mode (F-061, ADR-008)
//! [`WhisperEngine::transcribe_live`] runs the same loaded context against
//! short realtime chunks with a distinct [`LiveOptions`] parameter set
//! ([`build_live_params`]) that natively mirrors the flags Buzz's
//! `whisper-server` subprocess was launched with
//! (`buzz/transcriber/recording_transcriber.py:450-554`,
//! `start_local_whisper_server`) — humm never spawns that server (ADR-008).
//! No VAD, no word timestamps, no progress/segment streaming: a live chunk is
//! a few seconds of audio and the caller wants one joined utterance string
//! back ([`join_segment_texts`]), not a stream of callbacks.
//!
//! ## Live session state reuse (ADR-021 carried perf ticket, F-061)
//! `transcribe_live` used to call `WhisperContext::create_state()` on every
//! chunk. Under the `coreml` feature this reloads the CoreML encoder model
//! from disk each time (`whisper_ctx_wrapper.rs:466` → whisper.cpp's
//! `whisper_init_state` → `whisper_coreml_init`, vendored
//! `whisper.cpp/src/whisper.cpp:3441-3446` — ADR-021's traced mechanism).
//! [`WhisperEngine`] now caches one [`whisper_rs::WhisperState`] in a
//! [`LiveStateSlot`] and reuses it across a live session's chunks;
//! [`WhisperEngine::begin_live_session`] clears the slot (the hard guarantee
//! against a stale state leaking across sessions) and
//! [`WhisperEngine::end_live_session`] drops it as best-effort cleanup. The
//! one call is still per-chunk: the rolling prompt continues to be passed
//! explicitly via `prompt`, and the abort callback wiring is still installed
//! on `params` per call, unchanged (live mode has no segment/progress
//! callbacks to begin with — see "Live mode" above).
//!
//! **Parity bonus, not just a perf fix.** Per-session state reuse also
//! matches Buzz's own live-recording behavior more closely than the
//! per-chunk-fresh-state approach this replaces did: Buzz's `whisper-server`
//! subprocess (`recording_transcriber.py:450-554`) called whisper.cpp's
//! context-level `whisper_full`, which allocates and owns **one** internal
//! state for the server process's entire lifetime — every chunk the server
//! ever transcribes runs on that same state, not a fresh one. whisper.cpp's
//! own reference realtime example, `examples/stream/stream.cpp`, does the
//! same: one `whisper_state` reused across the whole streaming session. So
//! this ticket doesn't just recover the CoreML-reload cost ADR-021 traced —
//! it also brings humm's in-process live path back in line with the
//! state-lifetime shape both Buzz's server and upstream's own streaming
//! reference use, which the per-chunk-fresh-state interim design (accepted
//! deliberately in ADR-021, for architectural simplicity) had diverged from.
//!
//! **Text/prompt/KV state never leaks between calls, reused state or not.**
//! whisper.cpp's `whisper_full_with_state` unconditionally clears
//! `state->result_all` and (with `no_context = true`, whisper.cpp's own
//! default, never overridden by
//! [`build_live_params`]/[`build_full_params`]) `state->prompt_past0`/
//! `prompt_past1` at the top of every call, and clears `state->kv_self` (the
//! decoder's self-attention cache) before decoding each segment. This is
//! true for every call, first or later, whether the `WhisperState` is fresh
//! or reused.
//!
//! **One real, upstream-inherited exception: `decoders[0].rng`.**
//! whisper.cpp's beam-search candidate step
//! (`whisper_sample_token_topk`) draws from `decoder.rng` on *every* decode
//! step regardless of temperature, and `decoders[0].rng` is seeded exactly
//! once, in `whisper_init_state` (state creation) — never reset per `full()`
//! call. So a session's second and later chunks, run on a **reused** state,
//! consume a different point in that rng sequence than the same chunk would
//! on its own **freshly created** state (which always restarts at
//! `mt19937(0)`). Confirmed empirically against the real tiny model
//! (`tests/contract.rs`'s `first_chunk_output_is_reproducible_across_engine_instances`,
//! pinned against a captured golden transcript — always matches, since a
//! state's first call always starts from the same rng position either way —
//! versus the same file's doc comment on why a *later* chunk does not, and
//! `live_session_chunk_sequence_is_deterministic_when_repeated`, which
//! verifies the property that **is** true instead: a reused-state chunk
//! sequence reproduces identically run to run). This is whisper.cpp's own
//! canonical streaming design — `examples/stream/stream.cpp` reuses one
//! state across a whole session too — not a defect this ticket introduces.
//! See `docs/design/DECISIONS.md` ADR-021's addendum for the measured
//! before/after ratio.

use std::borrow::Cow;
use std::ffi::{c_int, c_void};
use std::path::Path;
use std::sync::Once;

use humm_core::boundary::{
    EngineCaps, EngineOptions, Pcm, SAMPLE_RATE, SegmentSink, TranscriptionEngine,
};
use humm_core::cancel::CancelToken;
use humm_core::domain::{Millis, Segment, TaskKind};
use humm_core::error::EngineError;
use humm_core::language::Language;

use whisper_rs::{
    FullParams, SamplingStrategy, SegmentCallbackData, WhisperContext, WhisperContextParameters,
    WhisperState, WhisperVadContext, WhisperVadContextParams, WhisperVadParams,
};

use crate::words::{TokenInfo, group_words};

/// whisper.cpp reports timestamps in centiseconds; humm uses milliseconds
/// (`ms = cs * 10`, Global Constraint 12).
fn centiseconds_to_millis(cs: i64) -> Millis {
    Millis(cs * 10)
}

static LOG_HOOK: Once = Once::new();

/// Routes whisper.cpp/GGML logging through the `log` crate exactly once
/// (idempotent; safe to call on every load).
fn init_logging() {
    LOG_HOOK.call_once(whisper_rs::install_logging_hooks);
}

/// Parameters for a single live (in-process, already-chunked realtime)
/// transcription call (F-061, ADR-008).
///
/// Deliberately narrower than [`EngineOptions`] rather than reusing it with
/// an extra bool: live transcription has no VAD (the session loop already
/// hands over a short, silence-cut batch) and no word-level timestamps (Buzz's
/// `whisper-server` path never requested them either, `--no-timestamps`) — so
/// those fields simply don't exist here instead of being carried unused. The
/// rolling prompt is passed per call to [`WhisperEngine::transcribe_live`]
/// rather than stored on this struct, since it changes every call (the
/// previous call's own output).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveOptions {
    /// Force a specific source language, or `None` to auto-detect.
    pub language: Option<Language>,
    /// Transcribe or translate-to-English.
    pub task: TaskKind,
    /// Fallback temperature schedule (same shape as `EngineOptions::temperature`).
    pub temperature: Vec<f32>,
    /// Thread count override; `None` uses the adapter's default.
    pub threads: Option<u32>,
}

/// A lazily created, session-scoped cache slot for one long-lived value,
/// reused across calls until [`Self::clear`] resets it.
///
/// Generic and dependency-free on purpose: [`WhisperEngine`] cannot be
/// constructed in a unit test without a real ggml model on disk (see
/// `crates/humm-whisper/tests/contract.rs`'s module doc), so the
/// create-once/reuse/clear behavior this type implements is instead
/// unit-tested directly against a plain `T` (no whisper involved) —
/// `live_state_slot_creates_once_and_reuses_until_cleared` — while still
/// being the exact same code [`WhisperEngine`] runs in production.
///
/// [`WhisperEngine`] uses `LiveStateSlot<WhisperState>` to cache one live
/// session's [`whisper_rs::WhisperState`] (ADR-021's carried perf ticket,
/// F-061 — see the module doc's "Live session state reuse" section).
#[derive(Debug)]
struct LiveStateSlot<T> {
    value: Option<T>,
}

impl<T> LiveStateSlot<T> {
    fn new() -> Self {
        LiveStateSlot { value: None }
    }

    /// Drops any cached value, guaranteeing the next [`Self::get_or_create`]
    /// call constructs a fresh one. This is the "begin/end a session"
    /// boundary: no value ever survives a `clear()` call.
    fn clear(&mut self) {
        self.value = None;
    }

    /// Returns the cached value, constructing one via `create` on a cache
    /// miss (the first call, or the first call after [`Self::clear`]).
    /// Subsequent calls return the same value without invoking `create`
    /// again.
    fn get_or_create<E>(&mut self, create: impl FnOnce() -> Result<T, E>) -> Result<&mut T, E> {
        if self.value.is_none() {
            self.value = Some(create()?);
        }
        Ok(self.value.as_mut().expect("just set above"))
    }
}

/// The local whisper.cpp transcription engine.
#[derive(Debug)]
pub struct WhisperEngine {
    context: WhisperContext,
    /// The live-session [`WhisperState`], reused across a session's
    /// [`Self::transcribe_live`] calls instead of recreated per chunk
    /// (ADR-021's carried perf ticket, F-061 — see the module doc).
    /// [`Self::begin_live_session`]/[`Self::end_live_session`] clear it;
    /// `transcribe_live` creates it lazily on first use. Never touched by
    /// [`TranscriptionEngine::transcribe`] (the file path creates its own
    /// per-call state — out of this ticket's scope, unchanged).
    live_state: LiveStateSlot<WhisperState>,
}

impl WhisperEngine {
    /// Loads a ggml model from `model_path`.
    ///
    /// `use_gpu` selects Metal/CoreML acceleration (`false` forces CPU; the CLI
    /// reads `HUMM_FORCE_CPU`, not this crate). With the `coreml` feature,
    /// whisper.cpp auto-discovers `ggml-<name>-encoder.mlmodelc` next to the
    /// ggml file and falls back gracefully when it is absent.
    ///
    /// # Errors
    /// [`EngineError::ModelLoad`] if the file is missing or whisper-rs fails to
    /// initialize the context.
    pub fn load(model_path: &Path, use_gpu: bool) -> Result<Self, EngineError> {
        init_logging();

        if !model_path.is_file() {
            return Err(EngineError::ModelLoad {
                path: model_path.to_path_buf(),
                detail: "model file missing — download it first or set HUMM_MODEL_ROOT".to_string(),
            });
        }

        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);

        let context = WhisperContext::new_with_params(model_path, params).map_err(|e| {
            EngineError::ModelLoad {
                path: model_path.to_path_buf(),
                detail: e.to_string(),
            }
        })?;

        Ok(WhisperEngine {
            context,
            live_state: LiveStateSlot::new(),
        })
    }

    /// Begins a live-recording session: clears any previously cached live
    /// [`WhisperState`] so the next [`Self::transcribe_live`] call starts
    /// fresh (ADR-021's carried perf ticket, F-061). This is the hard
    /// guarantee against a stale state leaking across sessions — today's GUI
    /// wiring never reuses one `WhisperEngine` instance across sessions
    /// (`LiveEngineFactory::load` in `crates/humm-gui/src/live_session.rs`
    /// constructs a fresh engine per session), but calling this first keeps
    /// that guarantee true even if that ever changed.
    ///
    /// # Errors
    /// Never returns `Err` for this engine; the `Result` return type matches
    /// the `LiveEngineSession` trait seam other backends bracket a session
    /// through (`humm-gui`'s API-backend arm has no live state to reset, so
    /// its implementation is a no-op `Ok(())` too).
    pub fn begin_live_session(&mut self) -> Result<(), EngineError> {
        self.live_state.clear();
        Ok(())
    }

    /// Ends a live-recording session: drops the cached [`WhisperState`]
    /// promptly rather than waiting for `WhisperEngine` itself to drop.
    /// Best-effort cleanup — since a fresh `WhisperEngine` is constructed per
    /// session by the GUI (see [`Self::begin_live_session`]'s doc), this is
    /// usually redundant with the engine's own drop moments later, but it
    /// makes the "session is over" boundary explicit and frees the cached
    /// state's CoreML/Metal-backed resources as soon as the session ends
    /// rather than whenever the engine happens to fall out of scope.
    pub fn end_live_session(&mut self) {
        self.live_state.clear();
    }

    /// Transcribes one already-chunked live batch of PCM samples, returning
    /// the joined utterance text (F-061, ADR-008).
    ///
    /// This is Buzz's `_transcribe_via_api` live-chunk round trip
    /// (`buzz/transcriber/recording_transcriber.py:306-362`) reproduced
    /// in-process instead of over the whisper-server subprocess Buzz spawned:
    /// the same request shape (rolling `prompt`, no timestamps) and the same
    /// response shape (segment texts joined, see `join_segment_texts`), but
    /// via `WhisperState::full` directly rather than an HTTP call.
    ///
    /// `prompt` is the caller's rolling context — the previous call's
    /// returned text, or the session's configured initial prompt on the first
    /// call. Unlike [`TranscriptionEngine::transcribe`], there is no
    /// progress/segment streaming: a live chunk is a few seconds of audio, so
    /// the joined result is returned once the run completes.
    ///
    /// # Errors
    /// [`EngineError::Canceled`] if `cancel` fires during the run (checked
    /// the same way as [`TranscriptionEngine::transcribe`] — see the SAFETY
    /// note there for why the abort callback is wired via the raw setter);
    /// [`EngineError::Inference`] on any other whisper.cpp failure.
    pub fn transcribe_live(
        &mut self,
        samples: &[f32],
        opts: &LiveOptions,
        prompt: &str,
        cancel: &CancelToken,
    ) -> Result<String, EngineError> {
        if samples.is_empty() {
            // Mirrors `transcribe`'s empty-input guard: whisper_full errors on
            // empty input, but an empty batch is an empty transcript, not a
            // failure.
            return Ok(String::new());
        }

        let threads = opts.threads.unwrap_or_else(default_threads);
        let mut params = build_live_params(opts, prompt, threads);

        // Cooperative cancellation — identical wiring to `transcribe` (see
        // its SAFETY comment for why the raw setter is used instead of
        // whisper-rs's miscompiled `set_abort_callback_safe`).
        // SAFETY: `abort_trampoline` reads the `CancelToken` behind
        // `user_data`; `cancel` is borrowed for the whole of this call, so it
        // outlives `state.full(...)` below, the only time whisper invokes the
        // callback.
        unsafe {
            params.set_abort_callback(Some(abort_trampoline));
            params.set_abort_callback_user_data(cancel as *const CancelToken as *mut c_void);
        }

        // Reuse the cached live-session state instead of creating a fresh one
        // per chunk (ADR-021's carried perf ticket — see the module doc's
        // "Live session state reuse" section for the output-equivalence
        // argument). `context` is bound separately from `self.live_state` so
        // the closure below and `self.live_state.get_or_create`'s `&mut`
        // borrow touch disjoint fields of `self`, not the same borrow twice.
        let context = &self.context;
        let state = self
            .live_state
            .get_or_create(|| context.create_state())
            .map_err(|e| EngineError::Inference {
                detail: format!("failed to create whisper state: {e}"),
            })?;

        let run = state.full(params, samples);

        // Same convention as `transcribe`: cancellation makes whisper abort
        // with a nonzero return code, surfaced as `Canceled` regardless of
        // the specific code, whether `full` reported Ok or Err.
        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }
        run.map_err(|e| EngineError::Inference {
            detail: e.to_string(),
        })?;

        let segment_texts: Vec<Cow<'_, str>> = state
            .as_iter()
            .filter_map(|segment| segment.to_str_lossy().ok())
            .collect();
        let refs: Vec<&str> = segment_texts.iter().map(Cow::as_ref).collect();

        Ok(join_segment_texts(&refs))
    }
}

impl TranscriptionEngine for WhisperEngine {
    /// Transcribes `audio`, streaming segments and progress to `sink` and
    /// aborting promptly when `cancel` is set.
    ///
    /// ## VAD timeline (F-045) — empirical finding
    /// The vendored whisper.cpp 1.8.3 *does* remap VAD-compressed timestamps to
    /// the original timeline — but only inside the **context-level**
    /// `whisper_full` (which runs `whisper_vad`, transcribes the filtered
    /// samples, then maps segment timestamps back via
    /// `map_processed_to_original_time`; token timestamps stay raw). whisper-rs
    /// 0.16, however, only wraps `whisper_full_with_state`, which contains no VAD
    /// path at all, so `enable_vad` / `set_vad_model_path` on `FullParams` do
    /// nothing here (verified empirically: with them set, no VAD ran and output
    /// was byte-identical to VAD-off).
    ///
    /// humm therefore performs VAD itself: [`vad_filter`] runs whisper-rs's
    /// standalone Silero VAD to get the speech spans, concatenates the
    /// speech-only samples, transcribes those, and remaps every emitted segment
    /// **and** token timestamp back onto the original timeline via [`VadRemap`]
    /// (a sample-exact realization of Buzz's `_make_offset_mapper`,
    /// `buzz/transcriber/whisper_cpp.py:48-74`). Verified by the
    /// `vad_remaps_to_original_timeline` contract test.
    fn transcribe(
        &mut self,
        audio: &Pcm,
        options: &EngineOptions,
        sink: &mut dyn SegmentSink,
        cancel: &CancelToken,
    ) -> Result<(), EngineError> {
        let original = audio.samples();
        if original.is_empty() {
            // Nothing to decode; whisper_full errors on empty input. Report an
            // empty transcript rather than an inference failure.
            return Ok(());
        }

        // VAD preprocessing (F-045). See the note above: whisper-rs bypasses
        // whisper.cpp's built-in VAD, so we filter to speech ourselves and remap
        // timestamps back to the original timeline. `VadRemap::identity()` leaves
        // timings untouched when VAD is off.
        let (filtered, remap) = match &options.vad_model {
            Some(vad_path) => {
                let (filtered, remap) = vad_filter(vad_path, original, cancel)?;
                if filtered.is_empty() {
                    // No speech detected → empty transcript (mirrors whisper.cpp's
                    // `vad_samples.empty()` early return).
                    return Ok(());
                }
                (Some(filtered), remap)
            }
            None => (None, VadRemap::identity()),
        };
        let samples: &[f32] = filtered.as_deref().unwrap_or(original);

        let threads = options.threads.unwrap_or_else(default_threads);
        let mut params = build_full_params(options, threads);

        let word_mode = options.word_timestamps;
        let mut bridge = Bridge {
            sink,
            word_mode,
            remap: remap.clone(),
        };

        // SAFETY: erase the borrow lifetime so whisper-rs's `'static` callback
        // closures can hold a pointer to `bridge`. `bridge` lives on this stack
        // frame for the whole of `transcribe`; whisper.cpp invokes the closures
        // only synchronously on this thread from within `state.full(...)` below
        // and never afterwards, so the pointer is always valid when
        // dereferenced. The callbacks are non-reentrant and single-threaded, so
        // the transient `&mut Bridge` reborrows never alias.
        let bridge_ptr: *mut Bridge<'static> =
            unsafe { std::mem::transmute::<*mut Bridge<'_>, *mut Bridge<'static>>(&mut bridge) };

        let progress_ptr = bridge_ptr;
        params.set_progress_callback_safe(move |pct: i32| {
            // SAFETY: see the transmute above — valid throughout `full()`.
            unsafe { (*progress_ptr).on_progress(pct) };
        });

        let segment_ptr = bridge_ptr;
        params.set_segment_callback_safe_lossy(move |data: SegmentCallbackData| {
            // SAFETY: see the transmute above — valid throughout `full()`.
            unsafe { (*segment_ptr).on_segment(data) };
        });

        // Cooperative cancellation. whisper-rs 0.16's *safe* abort setter
        // (`set_abort_callback_safe`) is miscompiled — it instantiates its
        // trampoline with the raw closure type `F` while the user_data actually
        // points to a `Box<dyn FnMut() -> bool>` (unlike the progress/segment
        // setters, which use the boxed type), so the trampoline reinterprets a
        // fat pointer as the closure and calls garbage, intermittently aborting
        // the encoder (whisper error -6) even when nothing was cancelled. We use
        // the raw C setter with a pointer to the borrowed token instead.
        // SAFETY: `abort_trampoline` reads the `CancelToken` behind `user_data`.
        // `cancel` is borrowed for the whole of `transcribe`, so it outlives
        // `state.full(...)` — the only time whisper invokes the callback. The
        // access is a shared read (`is_canceled(&self)`), so it never aliases.
        unsafe {
            params.set_abort_callback(Some(abort_trampoline));
            params.set_abort_callback_user_data(cancel as *const CancelToken as *mut c_void);
        }

        let mut state = self
            .context
            .create_state()
            .map_err(|e| EngineError::Inference {
                detail: format!("failed to create whisper state: {e}"),
            })?;

        let run = state.full(params, samples);

        // Cancellation makes whisper abort with a nonzero return code; surface
        // it as `Canceled` regardless of the specific code (rather than
        // `Inference`), whether `full` reported Ok or Err.
        if cancel.is_canceled() {
            return Err(EngineError::Canceled);
        }
        run.map_err(|e| EngineError::Inference {
            detail: e.to_string(),
        })?;

        // Word-level mode: segment-level streaming was suppressed in the
        // callback; reconstruct per-word/character segments from token data now
        // that the run is complete and emit them (remapping to the original
        // timeline under VAD).
        if word_mode {
            emit_word_segments(&state, options, &remap, bridge.sink)?;
        }

        Ok(())
    }

    fn capabilities(&self) -> EngineCaps {
        EngineCaps {
            word_timestamps: true,
            translate: true,
            vad: true,
        }
    }
}

/// Bridges whisper.cpp's callbacks to the borrowed [`SegmentSink`]. Held on
/// `transcribe`'s stack and reached through a lifetime-erased raw pointer (the
/// closures whisper-rs stores must be `'static`).
struct Bridge<'a> {
    sink: &'a mut dyn SegmentSink,
    /// When `true`, per-segment streaming is suppressed and per-word segments
    /// are emitted after the run instead.
    word_mode: bool,
    /// Maps VAD-filtered timestamps back to the original timeline (identity when
    /// VAD is off).
    remap: VadRemap,
}

impl Bridge<'_> {
    fn on_progress(&mut self, pct: i32) {
        self.sink.on_progress((pct as f32 / 100.0).clamp(0.0, 1.0));
    }

    fn on_segment(&mut self, data: SegmentCallbackData) {
        if self.word_mode {
            return;
        }
        // Translate mode yields English directly in `text`; `translation` stays
        // empty on the segment path, matching Buzz's `_parse_segment_timings`,
        // which never fills `translation`.
        let text = data.text.trim().to_string();
        self.sink.on_segment(Segment {
            start: self
                .remap
                .remap(centiseconds_to_millis(data.start_timestamp)),
            end: self.remap.remap(centiseconds_to_millis(data.end_timestamp)),
            text,
            translation: String::new(),
        });
    }
}

/// Default whisper thread count when `options.threads` is `None`:
/// `max(1, logical_cpus / 2)`, matching Buzz's `(os.cpu_count() or 8) // 2`
/// (`buzz/transcriber/whisper_cpp.py:178`).
fn default_threads() -> u32 {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    ((logical / 2).max(1)) as u32
}

/// C trampoline for whisper's abort callback (`ggml_abort_callback`): returns
/// `true` (abort) once the [`CancelToken`] behind `user_data` is cancelled.
/// Installed via the raw setter to work around a bug in whisper-rs 0.16's
/// `set_abort_callback_safe` (see `transcribe`).
unsafe extern "C" fn abort_trampoline(user_data: *mut c_void) -> bool {
    if user_data.is_null() {
        return false;
    }
    // SAFETY: `user_data` is the `*const CancelToken` installed by `transcribe`;
    // the token is borrowed for the whole call and outlives every invocation
    // (whisper only calls this during `full()`). `is_canceled` takes `&self`, so
    // this shared read never aliases mutably.
    let cancel = unsafe { &*(user_data as *const CancelToken) };
    cancel.is_canceled()
}

/// Returns `prompt` only when it is non-empty and free of interior NUL bytes
/// — the only inputs `FullParams::set_initial_prompt` accepts without
/// panicking (it `CString`-converts them). Shared by [`build_full_params`]
/// and [`build_live_params`].
fn sanitized_initial_prompt(prompt: &str) -> Option<&str> {
    (!prompt.is_empty() && !prompt.contains('\0')).then_some(prompt)
}

/// Splits a fallback temperature schedule into whisper.cpp's `temperature`
/// (the schedule's first value) and `temperature_inc` (the gap between the
/// first two values, present only when at least two are given — otherwise
/// whisper's own default increment is left alone). Shared by
/// [`build_full_params`] and [`build_live_params`].
fn temperature_schedule_head(temperature: &[f32]) -> (Option<f32>, Option<f32>) {
    let first = temperature.first().copied();
    let inc = (temperature.len() >= 2).then(|| temperature[1] - temperature[0]);
    (first, inc)
}

/// Builds the `FullParams` mirroring Buzz's whisper-cli flags
/// (`buzz/transcriber/whisper_cpp.py:167-196`). The progress, new-segment, and
/// abort callbacks are wired separately in [`WhisperEngine::transcribe`].
fn build_full_params(options: &EngineOptions, threads: u32) -> FullParams<'static, 'static> {
    // whisper-cli defaults to beam search with beam_size 5 (whisper.cpp 1.8.3:
    // `wparams.strategy = (beam_size > 1 || use_grammar) ? BEAM_SEARCH : GREEDY`
    // with `beam_size` defaulting to 5, `patience` to -1.0). Buzz passes no
    // sampling flags, so it inherits this default.
    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });

    params.set_n_threads(threads as c_int);
    params.set_n_max_text_ctx(0); // Buzz: --max-context 0
    params.set_translate(matches!(options.task, TaskKind::Translate));

    // `Some(code)` or "auto" to auto-detect (Buzz: whisper_cpp.py:361-365).
    let language: &'static str = options.language.map_or("auto", |l| l.code());
    params.set_language(Some(language));

    // Buzz passes the initial prompt straight through; guard the (malformed)
    // interior-NUL case so `set_initial_prompt` can't panic.
    if let Some(prompt) = sanitized_initial_prompt(&options.initial_prompt) {
        params.set_initial_prompt(prompt);
    }

    // temperature[0] = initial; increment = temperature[1] - temperature[0]
    // when at least two are given, else leave whisper's default (0.2).
    let (temperature, temperature_inc) = temperature_schedule_head(&options.temperature);
    if let Some(temperature) = temperature {
        params.set_temperature(temperature);
    }
    if let Some(temperature_inc) = temperature_inc {
        params.set_temperature_inc(temperature_inc);
    }

    params.set_suppress_nst(true); // Buzz: --suppress-nst
    params.set_entropy_thold(2.8); // Buzz: --entropy-thold 2.8

    params.set_token_timestamps(options.word_timestamps);

    // No console output; progress and segments flow through callbacks.
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);

    // VAD is NOT configured here: `enable_vad`/`set_vad_model_path` on
    // `FullParams` are no-ops through whisper-rs's `whisper_full_with_state`
    // wrapper. VAD is handled by `vad_filter` + `VadRemap` in `transcribe`.

    params
}

/// Builds the `FullParams` for one live (in-process realtime) transcription
/// call, natively mirroring the hallucination guards Buzz's `whisper-server`
/// subprocess was launched with (`buzz/transcriber/recording_transcriber.py:
/// 450-554`, `start_local_whisper_server`): `--max-context 8`,
/// `--suppress-nst`, `--entropy-thold 2.8`, `--no-timestamps`, plus
/// `--translate`/`--language` from `opts` and the per-call rolling `prompt`
/// in place of Buzz's whisper-server having no initial-prompt flag at all
/// (the session loop feeds each call's own returned text back in as the next
/// call's prompt). The abort callback is wired separately in
/// [`WhisperEngine::transcribe_live`].
///
/// Kept as its own function rather than folding into [`build_full_params`]
/// behind a `live: bool` flag: the two differ in context window,
/// timestamps, and prompt source, so a merged function would need more
/// branching than the duplication it would remove — two clear functions beat
/// one flag-riddled one. [`sanitized_initial_prompt`] and
/// [`temperature_schedule_head`] are shared because that logic is identical
/// (and safety-relevant) in both.
fn build_live_params(
    opts: &LiveOptions,
    prompt: &str,
    threads: u32,
) -> FullParams<'static, 'static> {
    // Same beam-search default as file mode (Buzz's whisper-server passes no
    // sampling flags either, so it also inherits whisper.cpp's default).
    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });

    params.set_n_threads(threads as c_int);
    params.set_n_max_text_ctx(8); // Buzz: --max-context 8 (live only; file mode uses 0)
    params.set_translate(matches!(opts.task, TaskKind::Translate));

    // `Some(code)` or "auto" to auto-detect (Buzz: --language <code|auto>).
    let language: &'static str = opts.language.map_or("auto", |l| l.code());
    params.set_language(Some(language));

    // The rolling prompt is a call argument here, not an `opts` field — it
    // changes every call. Same interior-NUL guard as file mode.
    if let Some(prompt) = sanitized_initial_prompt(prompt) {
        params.set_initial_prompt(prompt);
    }

    let (temperature, temperature_inc) = temperature_schedule_head(&opts.temperature);
    if let Some(temperature) = temperature {
        params.set_temperature(temperature);
    }
    if let Some(temperature_inc) = temperature_inc {
        params.set_temperature_inc(temperature_inc);
    }

    params.set_suppress_nst(true); // Buzz: --suppress-nst
    params.set_entropy_thold(2.8); // Buzz: --entropy-thold 2.8

    // Buzz: --no-timestamps. Live mode has no word-timestamps concept at all
    // (see `LiveOptions`), so this is unconditional, not options-driven.
    params.set_token_timestamps(false);

    // No console output; live mode has no progress/segment callbacks at all
    // (the caller gets the joined text once `transcribe_live` returns).
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);

    params
}

/// Joins per-segment texts into one live utterance string: each text is
/// trimmed, empty results are dropped, and the remainder are joined with a
/// single space — the observable behavior of Buzz's whisper-server JSON
/// response join (`" ".join(segment["text"] for segment in segments)`,
/// `buzz/transcriber/recording_transcriber.py:340-345`), trimmed per segment
/// because whisper.cpp segment text carries a leading space by convention
/// (the same convention [`Bridge::on_segment`] trims for file mode).
fn join_segment_texts(texts: &[&str]) -> String {
    texts
        .iter()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// One contiguous speech region kept by VAD: its sample range in the ORIGINAL
/// audio and the range it occupies in the concatenated speech-only audio. Since
/// samples are copied 1:1 (no resampling), positions within a span map by a
/// constant offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VadSpan {
    orig_start: usize,
    orig_end: usize,
    proc_start: usize,
    proc_end: usize,
}

/// Maps a timestamp on the VAD-filtered (speech-only) timeline back to the
/// original audio timeline, using the kept speech spans — the parity
/// realization of Buzz's `_make_offset_mapper`
/// (`buzz/transcriber/whisper_cpp.py:48-74`). An empty span list is the
/// identity map (VAD off).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct VadRemap {
    spans: Vec<VadSpan>,
}

impl VadRemap {
    /// The no-op map used when VAD is disabled.
    fn identity() -> Self {
        VadRemap { spans: Vec::new() }
    }

    /// Maps a filtered-timeline time onto the original timeline. Times inside a
    /// span shift by that span's constant sample offset; times past the last
    /// span clamp to the end of the final speech region.
    fn remap(&self, t: Millis) -> Millis {
        if self.spans.is_empty() {
            return t;
        }
        let processed = millis_to_sample(t);
        for span in &self.spans {
            if processed < span.proc_end {
                let within = processed.saturating_sub(span.proc_start);
                let clamped = within.min(span.orig_end - span.orig_start);
                return sample_to_millis(span.orig_start + clamped);
            }
        }
        sample_to_millis(self.spans.last().map_or(0, |s| s.orig_end))
    }
}

/// Converts a millisecond time to a sample index at [`SAMPLE_RATE`].
fn millis_to_sample(t: Millis) -> usize {
    (t.0.max(0) * i64::from(SAMPLE_RATE) / 1000) as usize
}

/// Converts a sample index at [`SAMPLE_RATE`] to a millisecond time.
fn sample_to_millis(sample: usize) -> Millis {
    Millis(sample as i64 * 1000 / i64::from(SAMPLE_RATE))
}

/// Runs Silero VAD over `samples`, returning the concatenated speech-only audio
/// and the [`VadRemap`] that maps its timeline back to the original.
///
/// whisper-rs's `WhisperState::full` does not run whisper.cpp's built-in VAD, so
/// humm drives the standalone VAD context directly (F-045). VAD tuning and the
/// CPU VAD context are left at library defaults, matching both whisper.cpp's own
/// `whisper_vad` (`whisper_vad_default_context_params`, `use_gpu = false`) and
/// Buzz (which passes only `--vad --vad-model`).
fn vad_filter(
    vad_path: &Path,
    samples: &[f32],
    cancel: &CancelToken,
) -> Result<(Vec<f32>, VadRemap), EngineError> {
    if cancel.is_canceled() {
        return Err(EngineError::Canceled);
    }

    let path_str = vad_path.to_string_lossy();
    let mut vad = WhisperVadContext::new(path_str.as_ref(), WhisperVadContextParams::default())
        .map_err(|e| EngineError::ModelLoad {
            path: vad_path.to_path_buf(),
            detail: format!("VAD model load failed: {e}"),
        })?;

    let segments = vad
        .segments_from_samples(WhisperVadParams::default(), samples)
        .map_err(|e| EngineError::Inference {
            detail: format!("VAD failed: {e}"),
        })?;

    let mut filtered = Vec::new();
    let mut spans = Vec::new();
    for segment in segments {
        // Segment bounds are in centiseconds on the original timeline.
        let orig_start = centiseconds_f32_to_sample(segment.start).min(samples.len());
        let orig_end = centiseconds_f32_to_sample(segment.end).min(samples.len());
        if orig_start >= orig_end {
            continue;
        }
        let proc_start = filtered.len();
        filtered.extend_from_slice(&samples[orig_start..orig_end]);
        spans.push(VadSpan {
            orig_start,
            orig_end,
            proc_start,
            proc_end: filtered.len(),
        });
    }

    Ok((filtered, VadRemap { spans }))
}

/// Converts a centisecond time (whisper-rs VAD reports these as `f32`) to a
/// sample index at [`SAMPLE_RATE`].
fn centiseconds_f32_to_sample(cs: f32) -> usize {
    (f64::from(cs) / 100.0 * f64::from(SAMPLE_RATE))
        .round()
        .max(0.0) as usize
}

/// Reconstructs per-word (or per-character) segments from each finished
/// segment's token data and emits them to `sink` — the word-level-timings path
/// (`buzz/transcriber/whisper_cpp.py:240-319`), delegating the grouping to
/// [`group_words`]. Token times (raw, on the VAD-filtered timeline under VAD)
/// are remapped to the original timeline via `remap` before grouping.
fn emit_word_segments(
    state: &whisper_rs::WhisperState,
    options: &EngineOptions,
    remap: &VadRemap,
    sink: &mut dyn SegmentSink,
) -> Result<(), EngineError> {
    let language = options.language.map(|l| l.code());

    for segment in state.as_iter() {
        let n_tokens = segment.n_tokens();
        let mut tokens = Vec::with_capacity(n_tokens.max(0) as usize);
        for i in 0..n_tokens {
            let Some(token) = segment.get_token(i) else {
                continue;
            };
            // Raw token bytes (may split a multi-byte UTF-8 char across tokens);
            // `group_words` reassembles complete characters.
            let Ok(bytes) = token.to_bytes() else {
                continue;
            };
            let data = token.token_data();
            tokens.push(TokenInfo {
                bytes: bytes.to_vec(),
                start: remap.remap(centiseconds_to_millis(data.t0)),
                end: remap.remap(centiseconds_to_millis(data.t1)),
                p: token.token_probability(),
            });
        }

        for word in group_words(&tokens, language) {
            sink.on_segment(word);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn load_missing_model_is_model_load_error() {
        let err = WhisperEngine::load(&PathBuf::from("/no/such/ggml-model.bin"), false)
            .expect_err("missing model must error");
        match err {
            EngineError::ModelLoad { path, detail } => {
                assert_eq!(path, PathBuf::from("/no/such/ggml-model.bin"));
                assert!(detail.contains("model file missing"), "detail: {detail}");
            }
            other => panic!("expected ModelLoad, got {other:?}"),
        }
    }

    #[test]
    fn default_threads_is_at_least_one() {
        assert!(default_threads() >= 1);
    }

    #[test]
    fn centiseconds_convert_to_millis() {
        assert_eq!(centiseconds_to_millis(40), Millis(400));
        assert_eq!(centiseconds_to_millis(0), Millis(0));
    }

    #[test]
    fn sample_millis_round_trip() {
        assert_eq!(millis_to_sample(Millis(1000)), 16_000);
        assert_eq!(sample_to_millis(16_000), Millis(1000));
        assert_eq!(millis_to_sample(Millis(-5)), 0);
    }

    #[test]
    fn vad_remap_identity_leaves_times_unchanged() {
        let remap = VadRemap::identity();
        assert_eq!(remap.remap(Millis(1234)), Millis(1234));
    }

    #[test]
    fn vad_remap_shifts_single_span_to_original_offset() {
        // One speech span at original 5 s..10 s, filtered to 0 s..5 s.
        let remap = VadRemap {
            spans: vec![VadSpan {
                orig_start: millis_to_sample(Millis(5000)),
                orig_end: millis_to_sample(Millis(10_000)),
                proc_start: 0,
                proc_end: millis_to_sample(Millis(5000)),
            }],
        };
        // filtered 0 ms → original 5000 ms; filtered 2500 ms → original 7500 ms.
        assert_eq!(remap.remap(Millis(0)), Millis(5000));
        assert_eq!(remap.remap(Millis(2500)), Millis(7500));
        // Past the last span clamps to its original end.
        assert_eq!(remap.remap(Millis(9999)), Millis(10_000));
    }

    #[test]
    fn vad_remap_jumps_silence_gap_between_spans() {
        // Speech at original 0..1 s and 5..10 s; filtered to 0..1 s and 1..6 s.
        let remap = VadRemap {
            spans: vec![
                VadSpan {
                    orig_start: 0,
                    orig_end: millis_to_sample(Millis(1000)),
                    proc_start: 0,
                    proc_end: millis_to_sample(Millis(1000)),
                },
                VadSpan {
                    orig_start: millis_to_sample(Millis(5000)),
                    orig_end: millis_to_sample(Millis(10_000)),
                    proc_start: millis_to_sample(Millis(1000)),
                    proc_end: millis_to_sample(Millis(6000)),
                },
            ],
        };
        // Within the first span: unchanged.
        assert_eq!(remap.remap(Millis(500)), Millis(500));
        // Just past the gap: filtered 1000 ms → original 5000 ms (jumps 4 s).
        assert_eq!(remap.remap(Millis(1000)), Millis(5000));
        // Deep in the second span: filtered 3500 ms → original 7500 ms.
        assert_eq!(remap.remap(Millis(3500)), Millis(7500));
    }

    // --- Live session state-lifecycle seam (ADR-021 carried perf ticket,
    //     F-061). `WhisperEngine` cannot be built in a unit test without a
    //     real ggml model on disk (see `tests/contract.rs`'s module doc), so
    //     this exercises the exact `LiveStateSlot` machinery
    //     `WhisperEngine::begin_live_session`/`end_live_session`/
    //     `transcribe_live` delegate to, hermetically, against a plain `i32`
    //     instead of a `WhisperState`. The `#[ignore]` real-model tests in
    //     `tests/contract.rs` cover the whisper-backed end of this contract
    //     (`first_chunk_output_is_reproducible_across_engine_instances`,
    //     `live_session_chunk_sequence_is_deterministic_when_repeated`,
    //     `second_session_after_begin_produces_same_output_as_first`).

    #[test]
    fn live_state_slot_creates_once_and_reuses_until_cleared() {
        let mut slot: LiveStateSlot<i32> = LiveStateSlot::new();
        let mut creations = 0;

        // First call: cache miss, constructs via `create`.
        let first = *slot
            .get_or_create::<&'static str>(|| {
                creations += 1;
                Ok(99)
            })
            .unwrap();
        assert_eq!(first, 99);
        assert_eq!(creations, 1, "first call must construct");

        // Second call: cache hit -- reuses the same value, `create` is not
        // invoked again (mirrors consecutive `transcribe_live` calls within
        // one live session reusing one `WhisperState`).
        let second = *slot
            .get_or_create::<&'static str>(|| {
                creations += 1;
                Ok(100)
            })
            .unwrap();
        assert_eq!(second, 99, "cached value must be reused, not replaced");
        assert_eq!(creations, 1, "second call must not construct again");

        // `clear()` (what `begin_live_session`/`end_live_session` call)
        // forces the next call to construct fresh -- mirrors a new session
        // never seeing a previous session's state.
        slot.clear();
        let third = *slot
            .get_or_create::<&'static str>(|| {
                creations += 1;
                Ok(101)
            })
            .unwrap();
        assert_eq!(third, 101, "post-clear call must construct a fresh value");
        assert_eq!(creations, 2, "clear() must force reconstruction");
    }

    #[test]
    fn live_state_slot_propagates_create_error_without_caching() {
        let mut slot: LiveStateSlot<i32> = LiveStateSlot::new();

        let err = slot.get_or_create(|| Err::<i32, _>("boom")).unwrap_err();
        assert_eq!(err, "boom");

        // A later successful call still constructs (the slot was never
        // populated by the failed attempt).
        let mut creations = 0;
        let ok = *slot
            .get_or_create::<&'static str>(|| {
                creations += 1;
                Ok(7)
            })
            .unwrap();
        assert_eq!(ok, 7);
        assert_eq!(
            creations, 1,
            "a failed create must not have populated the slot -- this call \
             must still construct"
        );
    }

    // --- Live params mapping (F-061, ADR-008) ---

    fn live_options() -> LiveOptions {
        LiveOptions {
            language: Language::parse("fr"),
            task: TaskKind::Transcribe,
            temperature: vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
            threads: None,
        }
    }

    fn file_options() -> EngineOptions {
        EngineOptions {
            language: Language::parse("fr"),
            task: TaskKind::Transcribe,
            initial_prompt: String::new(),
            word_timestamps: false,
            temperature: vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
            vad_model: None,
            threads: None,
        }
    }

    #[test]
    fn live_params_use_max_context_8_not_files_0() {
        let live = format!("{:?}", build_live_params(&live_options(), "hello", 4));
        assert!(
            live.contains("n_max_text_ctx: 8"),
            "live params should carry --max-context 8: {live}"
        );

        let file = format!("{:?}", build_full_params(&file_options(), 4));
        assert!(
            file.contains("n_max_text_ctx: 0"),
            "file params should carry --max-context 0: {file}"
        );
    }

    #[test]
    fn live_params_force_token_timestamps_off() {
        let debug = format!("{:?}", build_live_params(&live_options(), "", 4));
        assert!(
            debug.contains("token_timestamps: false"),
            "live params never request word timestamps: {debug}"
        );
    }

    #[test]
    fn live_params_set_translate_from_task() {
        let mut opts = live_options();

        opts.task = TaskKind::Translate;
        let debug = format!("{:?}", build_live_params(&opts, "", 4));
        assert!(debug.contains("translate: true"), "{debug}");

        opts.task = TaskKind::Transcribe;
        let debug = format!("{:?}", build_live_params(&opts, "", 4));
        assert!(debug.contains("translate: false"), "{debug}");
    }

    #[test]
    fn live_params_keep_suppress_nst_and_entropy_thold() {
        let debug = format!("{:?}", build_live_params(&live_options(), "", 4));
        assert!(debug.contains("suppress_nst: true"), "{debug}");
        assert!(debug.contains("entropy_thold: 2.8"), "{debug}");
    }

    #[test]
    fn sanitized_initial_prompt_rejects_empty_and_interior_nul() {
        assert_eq!(sanitized_initial_prompt(""), None);
        assert_eq!(sanitized_initial_prompt("has\0nul"), None);
        assert_eq!(sanitized_initial_prompt("hello"), Some("hello"));
    }

    #[test]
    fn temperature_schedule_head_needs_two_values_for_increment() {
        assert_eq!(temperature_schedule_head(&[]), (None, None));
        assert_eq!(temperature_schedule_head(&[0.3]), (Some(0.3), None));
        assert_eq!(
            temperature_schedule_head(&[0.0, 0.2, 0.4]),
            (Some(0.0), Some(0.2))
        );
    }

    // --- Segment-text join rule (Buzz's server-response join, F-061) ---

    #[test]
    fn join_segment_texts_trims_each_and_joins_with_single_space() {
        assert_eq!(join_segment_texts(&[" Hello,", " world."]), "Hello, world.");
    }

    #[test]
    fn join_segment_texts_drops_empty_segments_after_trim() {
        assert_eq!(join_segment_texts(&["   ", " Hi "]), "Hi");
    }

    #[test]
    fn join_segment_texts_empty_input_is_empty_string() {
        assert_eq!(join_segment_texts(&[]), "");
    }
}
