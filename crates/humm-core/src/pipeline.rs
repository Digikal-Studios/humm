//! The file-transcription pipeline: probe → decode → transcribe → export.
//!
//! Ported from Buzz's `FileTranscriber.run` (`buzz/transcriber/file_transcriber.py:38-73`):
//! transcribe, strip each segment's text, then write one output file per
//! requested format. This module wires the boundary traits together but
//! names no adapter — it is pure core, exercised in tests with in-module
//! fakes only.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::boundary::{AudioDecoder, EngineOptions, MediaProber, SegmentSink, TranscriptionEngine};
use crate::cancel::CancelToken;
use crate::domain::{JobSpec, Segment};
use crate::error::{CoreError, ExportError, MediaError};
use crate::export::{TextKey, output_file_path, write_format};
use crate::openai::{LlmEndpointSource, TranslatorConfig, TranslatorHandle};

/// The result of a successful [`run_file_job`]: the (trimmed) segments
/// produced by transcription, and the paths written, in `job.output.formats`
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct JobOutcome {
    /// Transcribed segments, with `text` trimmed (`translation` untouched).
    pub segments: Vec<Segment>,
    /// Paths written, one per requested output format, in format order.
    pub written: Vec<PathBuf>,
}

/// Engine-side tuning knobs that aren't part of [`crate::domain::TranscriptionOptions`]
/// (which mirrors Buzz's persisted, restart-safe job options) — these are
/// runtime/environment concerns supplied by the caller instead.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineTuning {
    /// VAD model path. `Some` enables voice-activity detection (F-045);
    /// `None` (the default) disables it.
    pub vad_model: Option<PathBuf>,
    /// Whether to use GPU acceleration (Metal/CoreML). Defaults to `true`.
    ///
    /// Not read by [`run_file_job`]: the engine is already constructed by
    /// the time it reaches this function, and GPU/CPU selection happens at
    /// engine *load*, not per job. The binary (`humm-cli`'s `EngineFactory`)
    /// resolves this value itself and passes it directly to
    /// `WhisperEngine::load`. This field exists so callers have one place to
    /// carry the resolved value alongside the other tuning knobs; it is
    /// otherwise inert here.
    pub use_gpu: bool,
    /// Thread count override; `None` (the default) uses the adapter's
    /// default.
    pub threads: Option<u32>,
}

impl Default for EngineTuning {
    fn default() -> Self {
        EngineTuning {
            vad_model: None,
            use_gpu: true,
            threads: None,
        }
    }
}

/// Collected `(segment ordinal, translation)` results from the per-task LLM
/// translator's callback thread (F-031), merged onto the segments after
/// transcription succeeds. Shared (via [`Arc`]) between [`run_file_job`] and
/// the translator's `on_result` callback, which runs on the worker thread.
type TranslationResults = Arc<Mutex<Vec<(i64, String)>>>;

/// Forwards every segment/progress update to `observer` while also
/// collecting the segments for the final [`JobOutcome`].
///
/// Trims `segment.text` as each segment arrives (Buzz strips text once, in
/// `run()`, after transcription completes and before either the `completed`
/// signal or the export loop see it — `file_transcriber.py:51-52`; trimming
/// here, at the single point both the observer and the collected `Vec`
/// derive from, reproduces that "every consumer sees stripped text" contract
/// without a second pass over the collected segments).
struct TeeSink<'a> {
    observer: &'a mut dyn SegmentSink,
    segments: Vec<Segment>,
    /// When `Some`, each trimmed segment's text is additionally enqueued for
    /// LLM translation (F-031), tagged with its arrival ordinal as the id so
    /// [`run_file_job`]'s post-transcription merge can map each result back to
    /// `segments[ordinal].translation`. `None` (the common case: no LLM
    /// translation requested, or no endpoint resolved) makes this a no-op —
    /// the sink then behaves byte-for-byte as it did before F-031.
    translator: Option<&'a TranslatorHandle>,
}

impl SegmentSink for TeeSink<'_> {
    fn on_segment(&mut self, mut segment: Segment) {
        segment.text = segment.text.trim().to_string();
        self.observer.on_segment(segment.clone());
        if let Some(translator) = self.translator {
            // Ordinal = arrival index (the length *before* this push); the
            // merge step keys each translation on it.
            translator.enqueue(segment.text.clone(), Some(self.segments.len() as i64));
        }
        self.segments.push(segment);
    }

    fn on_progress(&mut self, fraction: f32) {
        self.observer.on_progress(fraction);
    }
}

/// Runs one file-transcription job end to end: validates the input exists,
/// probes it (soft-fail), decodes to PCM, transcribes, and exports every
/// requested format.
///
/// Ported from Buzz's `FileTranscriber.run` (`file_transcriber.py:38-73`).
/// One deliberate deviation: humm creates the output directory if it's
/// missing (`create_dir_all`) instead of failing, since Buzz's `write_output`
/// simply fails when `-d` names a nonexistent directory.
///
/// # LLM translation (F-031, design spec §6)
///
/// When `job.options.llm` is `Some` **and** `llm` resolves an endpoint (a key
/// is available), each produced segment's text is enqueued to a per-task
/// [`TranslatorHandle`] as it is transcribed; after transcription succeeds the
/// worker is stopped (a bounded join), and each translation is merged onto its
/// segment's `translation` field before export/return. Absent either the
/// option or a resolvable endpoint, translation is inert: no worker, no
/// channel, no allocation (design spec §6's zero-overhead-when-disabled) —
/// humm's ✚ over Buzz, whose "Enable AI translation" checkbox is dead on the
/// file path (design spec §1). Buzz wires no translator into file
/// transcription at all; the batching/prompt/error machinery reused here is
/// ported byte-faithfully from `translator.py` (deviations ledger #1).
// One argument past clippy's default limit: `llm` extends the existing
// seven-parameter boundary-injection signature (job/tuning + the
// decoder/prober/engine/observer/cancel collaborators the caller owns), which
// there is no cohesive struct to fold into without obscuring the call sites.
#[allow(clippy::too_many_arguments)]
pub fn run_file_job(
    job: &JobSpec,
    tuning: &EngineTuning,
    decoder: &dyn AudioDecoder,
    prober: &dyn MediaProber,
    engine: &mut dyn TranscriptionEngine,
    observer: &mut dyn SegmentSink,
    cancel: &CancelToken,
    llm: Option<&dyn LlmEndpointSource>,
) -> Result<JobOutcome, CoreError> {
    if !job.input.exists() {
        return Err(CoreError::InputMissing {
            path: job.input.clone(),
        });
    }

    match prober.probe(&job.input) {
        Ok(info) if !info.has_audio => {
            return Err(MediaError::NoAudioStream {
                path: job.input.clone(),
            }
            .into());
        }
        Ok(_) => {}
        Err(err) => {
            // A probe error is non-fatal: ffmpeg's decode is the
            // authoritative failure source (Buzz never probes before
            // decode). Log and continue.
            log::warn!(
                "probe failed for {}: {err} — continuing, decode is authoritative",
                job.input.display()
            );
        }
    }

    if cancel.is_canceled() {
        return Err(CoreError::Canceled);
    }

    let pcm = decoder.decode(&job.input, cancel)?;
    if pcm.is_empty() {
        return Err(CoreError::NoAudioDecoded);
    }

    let engine_options = EngineOptions {
        language: job.options.language,
        task: job.options.task,
        initial_prompt: job.options.initial_prompt.clone(),
        word_timestamps: job.options.word_timestamps,
        temperature: job.options.temperature.clone(),
        vad_model: tuning.vad_model.clone(),
        threads: tuning.threads,
    };

    // F-031: optionally spin up a per-task LLM translator. A worker is spawned
    // only when the task opted in (`job.options.llm`) *and* the source resolves
    // an endpoint (a key is available); otherwise the whole feature stays inert
    // — no thread, no channel, no allocation. `translation_results` collects
    // `(ordinal, translation)` pairs from the worker's callback thread.
    let (translator, translation_results): (Option<TranslatorHandle>, Option<TranslationResults>) =
        match (&job.options.llm, llm) {
            (Some(llm_opts), Some(source)) => match source.resolve() {
                Some(endpoint) => {
                    let results: TranslationResults =
                        Arc::new(Mutex::new(Vec::<(i64, String)>::new()));
                    let sink_results = Arc::clone(&results);
                    let on_result: Box<dyn FnMut(String, Option<i64>) + Send> =
                        Box::new(move |translation, id| {
                            if let Some(id) = id {
                                sink_results
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .push((id, translation));
                            }
                        });
                    let handle = TranslatorHandle::spawn(
                        TranslatorConfig {
                            auth: endpoint.auth,
                            model: llm_opts.model.clone(),
                            prompt: llm_opts.prompt.clone(),
                        },
                        on_result,
                    );
                    (Some(handle), Some(results))
                }
                None => {
                    log::warn!(
                        "LLM translation enabled but no API key/endpoint available — \
                     task will complete without translations"
                    );
                    (None, None)
                }
            },
            _ => (None, None),
        };

    let (transcribe_result, mut segments) = {
        let mut tee = TeeSink {
            observer,
            segments: Vec::new(),
            translator: translator.as_ref(),
        };
        let result = engine.transcribe(&pcm, &engine_options, &mut tee, cancel);
        (result, tee.segments)
    };

    if let Err(err) = transcribe_result {
        // Transcription failed: abandon any spawned translator without blocking
        // (its queued items are moot) and propagate the error unchanged.
        if let Some(handle) = translator {
            handle.stop_detached();
        }
        return Err(err.into());
    }

    // Drain and merge LLM translations (if a translator ran). `stop()` joins
    // the worker, which drains a FIFO queue in batches of at most
    // `BATCH_SIZE` (10) items, each dispatched as one request bounded by
    // `REQUEST_TIMEOUT` (60 s) — so the worst-case join is ceil(N/10) x 60 s
    // for N items already queued when `stop()` is called, not a flat 60 s.
    // This is accepted rather than hardened because the backlog at stop-time
    // is normally at most one batch (batches are dispatched concurrently
    // with the transcription that keeps enqueuing them, so the queue rarely
    // grows past a single in-flight batch by the time transcription
    // finishes and this drain runs); a poll-and-check-cancel join loop
    // instead of a single blocking join is an M7 hardening candidate if that
    // assumption ever proves wrong (design spec §4.2/§6).
    if let Some(handle) = translator {
        // A cancel that has *already* landed is honored without waiting on the
        // drain at all: detach the worker and bail (design spec §6 / ADR-022's
        // "stop_detached + Canceled").
        if cancel.is_canceled() {
            handle.stop_detached();
            return Err(CoreError::Canceled);
        }
        handle.stop();
        // A cancel that landed *during* the (bounded) drain is honored here, at
        // the post-join check; the collected translations are then discarded.
        if cancel.is_canceled() {
            return Err(CoreError::Canceled);
        }
        if let Some(results) = &translation_results {
            for (id, translation) in results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
            {
                // Empty (errored/missing-marker) translations leave the
                // segment's `translation` as "" — parity with Buzz's error→""
                // degradation (`translator.py`).
                if !translation.is_empty()
                    && let Some(segment) = segments.get_mut(*id as usize)
                {
                    segment.translation = translation.clone();
                }
            }
        }
    }

    let now = chrono::Local::now().naive_local();
    let mut written = Vec::with_capacity(job.output.formats.len());
    for format in &job.output.formats {
        if cancel.is_canceled() {
            return Err(CoreError::Canceled);
        }

        let path = output_file_path(
            &job.input,
            job.options.task,
            job.options.language,
            job.options.model.size,
            *format,
            job.output.directory.as_deref(),
            job.output.file_name_template.as_deref(),
            now,
        );

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| ExportError::Io {
                path: path.clone(),
                source,
            })?;
        }

        let mut file = std::fs::File::create(&path).map_err(|source| ExportError::Io {
            path: path.clone(),
            source,
        })?;
        write_format(
            *format,
            &segments,
            TextKey::Text,
            job.output.paragraph_split,
            &mut file,
        )
        .map_err(|source| ExportError::Io {
            path: path.clone(),
            source,
        })?;

        written.push(path);
    }

    Ok(JobOutcome { segments, written })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::{EngineCaps, MediaInfo, Pcm, PlaybackPcm};
    use crate::domain::{Millis, OutputFormat, OutputSpec, TranscriptionOptions};
    use crate::error::EngineError;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -- Fakes ---------------------------------------------------------

    struct FakeDecoder {
        pcm: Pcm,
        calls: AtomicUsize,
    }

    impl FakeDecoder {
        fn new(pcm: Pcm) -> Self {
            FakeDecoder {
                pcm,
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl AudioDecoder for FakeDecoder {
        fn decode(&self, _path: &Path, _cancel: &CancelToken) -> Result<Pcm, MediaError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.pcm.clone())
        }

        fn decode_playback(
            &self,
            _path: &Path,
            _cancel: &CancelToken,
        ) -> Result<PlaybackPcm, MediaError> {
            Ok(PlaybackPcm::new(vec![0.0; 4], 2, 44_100))
        }
    }

    enum ProbeBehavior {
        Info(MediaInfo),
        Error,
    }

    struct FakeProber {
        behavior: ProbeBehavior,
    }

    impl MediaProber for FakeProber {
        fn probe(&self, _path: &Path) -> Result<MediaInfo, MediaError> {
            match &self.behavior {
                ProbeBehavior::Info(info) => Ok(info.clone()),
                ProbeBehavior::Error => Err(MediaError::ProbeFailed {
                    detail: "boom".into(),
                }),
            }
        }
    }

    fn prober_with_audio() -> FakeProber {
        FakeProber {
            behavior: ProbeBehavior::Info(MediaInfo {
                duration: Some(Millis(329)),
                has_audio: true,
                has_video: false,
                video: None,
            }),
        }
    }

    enum EngineBehavior {
        EmitFixtureSegments,
        Fail,
    }

    struct FakeEngine {
        behavior: EngineBehavior,
    }

    impl TranscriptionEngine for FakeEngine {
        fn transcribe(
            &mut self,
            _audio: &Pcm,
            _options: &EngineOptions,
            sink: &mut dyn SegmentSink,
            _cancel: &CancelToken,
        ) -> Result<(), EngineError> {
            match self.behavior {
                EngineBehavior::EmitFixtureSegments => {
                    sink.on_progress(0.5);
                    sink.on_segment(Segment {
                        start: Millis(40),
                        end: Millis(299),
                        text: " Bien ".into(),
                        translation: String::new(),
                    });
                    sink.on_segment(Segment {
                        start: Millis(299),
                        end: Millis(329),
                        text: "venue dans".into(),
                        translation: String::new(),
                    });
                    sink.on_progress(1.0);
                    Ok(())
                }
                EngineBehavior::Fail => Err(EngineError::Inference {
                    detail: "boom".into(),
                }),
            }
        }

        fn capabilities(&self) -> EngineCaps {
            EngineCaps {
                word_timestamps: false,
                translate: false,
                vad: false,
            }
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        segments: Vec<Segment>,
        progress: Vec<f32>,
    }

    impl SegmentSink for RecordingSink {
        fn on_segment(&mut self, segment: Segment) {
            self.segments.push(segment);
        }

        fn on_progress(&mut self, fraction: f32) {
            self.progress.push(fraction);
        }
    }

    struct NullSink;

    impl SegmentSink for NullSink {
        fn on_segment(&mut self, _segment: Segment) {}
        fn on_progress(&mut self, _fraction: f32) {}
    }

    /// Observer that trips a shared [`CancelToken`] once progress reaches
    /// `trip_at`. Used to simulate a cancel that lands between transcription
    /// finishing (the engine's final `on_progress(1.0)`) and the export loop.
    struct CancelOnProgress {
        token: CancelToken,
        trip_at: f32,
    }

    impl SegmentSink for CancelOnProgress {
        fn on_segment(&mut self, _segment: Segment) {}
        fn on_progress(&mut self, fraction: f32) {
            if fraction >= self.trip_at {
                self.token.cancel();
            }
        }
    }

    fn job_with(
        input: PathBuf,
        formats: BTreeSet<OutputFormat>,
        directory: Option<PathBuf>,
        file_name_template: Option<String>,
    ) -> JobSpec {
        JobSpec {
            input,
            options: TranscriptionOptions::default(),
            output: OutputSpec {
                formats,
                directory,
                file_name_template,
                paragraph_split: Millis(2000),
            },
        }
    }

    fn touch(path: &Path) {
        std::fs::write(path, b"fake media bytes").unwrap();
    }

    // -- Tests -----------------------------------------------------------

    #[test]
    fn happy_path_writes_txt_and_srt_and_trims_outcome_text() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        let formats: BTreeSet<OutputFormat> =
            [OutputFormat::Txt, OutputFormat::Srt].into_iter().collect();
        let job = job_with(
            input.clone(),
            formats,
            Some(tmp.path().to_path_buf()),
            Some("{{ input_file_name }}".to_string()),
        );

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();

        let outcome = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        )
        .unwrap();

        assert_eq!(outcome.segments.len(), 2);
        assert_eq!(outcome.segments[0].text, "Bien");
        assert_eq!(outcome.segments[1].text, "venue dans");

        let txt_path = tmp.path().join("input.txt");
        let srt_path = tmp.path().join("input.srt");
        assert_eq!(outcome.written, vec![txt_path.clone(), srt_path.clone()]);

        let txt = std::fs::read_to_string(&txt_path).unwrap();
        assert_eq!(txt, "Bien venue dans ");

        let srt = std::fs::read_to_string(&srt_path).unwrap();
        assert_eq!(
            srt,
            "1\n00:00:00,040 --> 00:00:00,299\nBien\n\n2\n00:00:00,299 --> 00:00:00,329\nvenue dans\n\n"
        );
    }

    #[test]
    fn empty_formats_write_no_files() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        let job = job_with(input, BTreeSet::new(), Some(tmp.path().to_path_buf()), None);

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();

        let outcome = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        )
        .unwrap();

        assert!(outcome.written.is_empty());
        assert_eq!(outcome.segments.len(), 2);
    }

    #[test]
    fn engine_error_propagates_and_writes_zero_files() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        let formats: BTreeSet<OutputFormat> =
            [OutputFormat::Txt, OutputFormat::Srt].into_iter().collect();
        let job = job_with(
            input,
            formats,
            Some(tmp.path().to_path_buf()),
            Some("{{ input_file_name }}".to_string()),
        );

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::Fail,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();

        let result = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        );

        assert!(matches!(
            result,
            Err(CoreError::Engine(EngineError::Inference { .. }))
        ));

        let entries: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        // Only the input file we created ourselves — no exports.
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn pre_canceled_token_stops_before_decode() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        let job = job_with(input, BTreeSet::new(), Some(tmp.path().to_path_buf()), None);

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();
        cancel.cancel();

        let result = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        );

        assert!(matches!(result, Err(CoreError::Canceled)));
        assert_eq!(decoder.call_count(), 0);
    }

    #[test]
    fn missing_input_path_is_input_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("missing.mp3");

        let job = job_with(
            input.clone(),
            BTreeSet::new(),
            Some(tmp.path().to_path_buf()),
            None,
        );

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();

        let result = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        );

        match result {
            Err(CoreError::InputMissing { path }) => assert_eq!(path, input),
            other => panic!("expected InputMissing, got {other:?}"),
        }
    }

    #[test]
    fn prober_reports_no_audio_stream_is_fatal_but_probe_error_is_soft() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        // has_audio: false -> fatal NoAudioStream.
        let job = job_with(
            input.clone(),
            BTreeSet::new(),
            Some(tmp.path().to_path_buf()),
            None,
        );
        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let no_audio_prober = FakeProber {
            behavior: ProbeBehavior::Info(MediaInfo {
                duration: Some(Millis(0)),
                has_audio: false,
                has_video: false,
                video: None,
            }),
        };
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();

        let result = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &no_audio_prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        );
        assert!(matches!(
            result,
            Err(CoreError::Media(MediaError::NoAudioStream { .. }))
        ));
        assert_eq!(decoder.call_count(), 0);

        // Prober error -> soft fail, decode is authoritative, job succeeds.
        let erroring_prober = FakeProber {
            behavior: ProbeBehavior::Error,
        };
        let decoder2 = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let mut engine2 = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer2 = NullSink;
        let cancel2 = CancelToken::new();

        let result2 = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder2,
            &erroring_prober,
            &mut engine2,
            &mut observer2,
            &cancel2,
            None,
        );
        assert!(result2.is_ok());
        assert_eq!(decoder2.call_count(), 1);
    }

    #[test]
    fn observer_receives_same_segments_as_outcome_plus_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        let job = job_with(input, BTreeSet::new(), Some(tmp.path().to_path_buf()), None);

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = RecordingSink::default();
        let cancel = CancelToken::new();

        let outcome = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        )
        .unwrap();

        assert_eq!(observer.segments, outcome.segments);
        assert_eq!(observer.progress, vec![0.5, 1.0]);
    }

    // -- Extra coverage beyond the brief's 7 cases ------------------------

    #[test]
    fn empty_pcm_is_no_audio_decoded() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        let job = job_with(input, BTreeSet::new(), Some(tmp.path().to_path_buf()), None);

        let decoder = FakeDecoder::new(Pcm::new(vec![]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();

        let result = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        );

        assert!(matches!(result, Err(CoreError::NoAudioDecoded)));
    }

    #[test]
    fn creates_missing_output_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);
        let nested = tmp.path().join("nested").join("out");

        let formats: BTreeSet<OutputFormat> = [OutputFormat::Txt].into_iter().collect();
        let job = job_with(input, formats, Some(nested.clone()), None);

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();

        let outcome = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        )
        .unwrap();

        assert_eq!(outcome.written.len(), 1);
        assert!(outcome.written[0].exists());
        assert!(nested.is_dir());
    }

    // -- Mid-export cancel interleaving (M1-review carry-over ticket) ------
    //
    // From `humm-m2-tickets` ("Bundle with M2's Ctrl-C/cancel work"): the
    // mid-export cancel interleaving for `run_file_job` was verified correct by
    // inspection but left untested in M1. These two tests close that gap.

    #[test]
    fn cancel_between_transcribe_and_export_writes_zero_files() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        // Two formats requested — the cancel must abort before writing any.
        let formats: BTreeSet<OutputFormat> =
            [OutputFormat::Txt, OutputFormat::Srt].into_iter().collect();
        let job = job_with(
            input,
            formats,
            Some(tmp.path().to_path_buf()),
            Some("{{ input_file_name }}".to_string()),
        );

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let cancel = CancelToken::new();
        // Trips on the engine's final on_progress(1.0): transcription has
        // finished, the export loop has not started.
        let mut observer = CancelOnProgress {
            token: cancel.clone(),
            trip_at: 1.0,
        };

        let result = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        );

        assert!(matches!(result, Err(CoreError::Canceled)));
        // Only the input file we created ourselves — no exports.
        let entries: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn pre_canceled_token_with_formats_requested_writes_zero_files() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.mp3");
        touch(&input);

        // Same invariant asserted directly (the M1 pre-cancel test used an
        // empty format set): a token canceled before the loop writes nothing.
        let formats: BTreeSet<OutputFormat> =
            [OutputFormat::Txt, OutputFormat::Srt, OutputFormat::Vtt]
                .into_iter()
                .collect();
        let job = job_with(
            input,
            formats,
            Some(tmp.path().to_path_buf()),
            Some("{{ input_file_name }}".to_string()),
        );

        let decoder = FakeDecoder::new(Pcm::new(vec![0.0; 16_000]));
        let prober = prober_with_audio();
        let mut engine = FakeEngine {
            behavior: EngineBehavior::EmitFixtureSegments,
        };
        let mut observer = NullSink;
        let cancel = CancelToken::new();
        cancel.cancel();

        let result = run_file_job(
            &job,
            &EngineTuning::default(),
            &decoder,
            &prober,
            &mut engine,
            &mut observer,
            &cancel,
            None,
        );

        assert!(matches!(result, Err(CoreError::Canceled)));
        assert_eq!(decoder.call_count(), 0);
        let entries: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }
}
