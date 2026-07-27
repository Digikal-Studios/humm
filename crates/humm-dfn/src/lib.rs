//! `humm-dfn`: pure-Rust DeepFilterNet3 noise-reduction adapter implementing
//! `humm_core::boundary::NoiseReducer` (F-112, M8 design spec §4).
//!
//! **Adapter composition, not core:** this crate depends on `humm-media`
//! (`FfmpegDecoder::decode_mono_at`) as well as `humm-core` — an adapter
//! crate consuming another adapter crate, not core, which still imports
//! neither (`docs/design/ARCHITECTURE.md`'s dependency rule; the diagram
//! itself gets this note in the Task 19 doc sweep, not here). Composing this
//! way avoids duplicating ffmpeg subprocess/cancellation/binary-resolution
//! logic that already lives in `humm-media`.
//!
//! **The model:** `deep_filter` is git-pinned (`validation/m8-spikes/README.md`
//! Spike A / ADR-011 addendum) with `default-features = false, features =
//! ["tract", "default-model"]` — pure `tract` (ONNX-in-Rust) inference, no
//! `ort`, no subprocess. `default-model` embeds the DeepFilterNet3 weights
//! via `include_bytes!` at compile time, so there is no model download and
//! no "model missing" unhappy path to handle here (unlike every other M8
//! ML feature).
//!
//! **Ported from Buzz** (`plugins/deep_filter_net/plugin.py`, whole file):
//! output naming (`<input_stem>_DeepFilterNet3.wav`, `:48`) and the
//! failure-never-fails-the-task contract (`:58-60`) — the latter is the
//! *caller's* job (`humm_core::queue::Runner::run_job`'s `catch_unwind`-
//! contained noise-reduction step), not this crate's: [`DfNoiseReducer::reduce`]
//! always returns a `Result`, it never panics on purpose, and the queue
//! seam degrades to the prior audio on any `Err`.
//!
//! A fresh `DfTract` model is built on every [`DfNoiseReducer::reduce`]
//! call — matching Buzz's own per-task `init_df()` (`plugin.py:52`), not a
//! cached singleton. This keeps the type trivially `Send + Sync` (no shared
//! mutable model state to synchronize) and gives every call the same cost
//! shape Buzz's Python plugin has.
//!
//! **Correction to the M8 spike's recorded note (`validation/m8-spikes/
//! README.md` Spike A):** the spike claimed "Cargo's `--extern` name
//! follows the *dependent's* Cargo.toml key, not the upstream crate's
//! internal lib name" — i.e. that depending on this package under the key
//! `deep_filter` (as `Cargo.toml` here does, matching the spike-recorded
//! pin exactly) would make `deep_filter::...` the right import path.
//! Building this crate showed otherwise: `rustc -v` output for this crate
//! carries `--extern df=.../libdf-*.rmeta`, not `--extern deep_filter=...`
//! — Cargo derives the extern name from the upstream package's own `[lib]
//! name = "df"` (`libDF/Cargo.toml`) whenever the dependency isn't
//! explicitly renamed via a `package = "..."` key (which this one isn't),
//! regardless of the *dependency key* used to pull it in. So the compiler,
//! not the spike note, is the correct source here: this crate imports
//! `df::tract::{...}`, not `deep_filter::tract::{...}`, even though
//! `Cargo.toml`'s dependency key stays `deep_filter` (matching the pinned
//! spec verbatim).

#![warn(missing_docs)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{Array2, Axis};

use humm_core::boundary::NoiseReducer;
use humm_core::cancel::CancelToken;
use humm_core::error::MediaError;
use humm_core::wav::write_wav_mono_i16;
use humm_media::FfmpegDecoder;

/// Reduces background noise before transcription via an embedded
/// DeepFilterNet3 model (F-112).
pub struct DfNoiseReducer {
    decoder: Arc<FfmpegDecoder>,
}

impl DfNoiseReducer {
    /// Wraps `decoder`, reused by every [`Self::reduce`] call to decode the
    /// input at whatever sample rate the freshly built model reports
    /// (`DfTract::sr`) — DeepFilterNet3's own rate (48 kHz), not
    /// `humm_core::boundary::SAMPLE_RATE` (16 kHz, the *transcription*
    /// engine's rate), and not hardcoded here: it's read off the model
    /// itself each call.
    pub fn new(decoder: Arc<FfmpegDecoder>) -> Self {
        DfNoiseReducer { decoder }
    }
}

impl NoiseReducer for DfNoiseReducer {
    fn reduce(&self, input: &Path, cancel: &CancelToken) -> Result<PathBuf, MediaError> {
        if cancel.is_canceled() {
            return Err(MediaError::Canceled);
        }

        // `RuntimeParams::default()` == `default_with_ch(1)` (mono, no
        // post-filter, atten_lim_db=100 — the spike-recorded defaults);
        // `DfParams::default()` decodes the embedded DFN3 tar.gz (the
        // `default-model` feature's `include_bytes!`).
        let r_params = RuntimeParams::default();
        let df_params = DfParams::default();
        let mut model =
            DfTract::new(df_params, &r_params).map_err(|err| MediaError::ReductionFailed {
                detail: format!("failed to load the embedded DeepFilterNet3 model: {err}"),
            })?;

        // Decode at the model's own sample rate (48 kHz per the spike, but
        // queried here rather than assumed) — `decode_mono_at` propagates
        // `MediaError` directly, including `Canceled`/`BinaryMissing`/
        // `DecodeFailed`, all of which the queue's noise-reduction step
        // treats as a degrade-to-prior-audio signal.
        let pcm = self
            .decoder
            .decode_mono_at(input, model.sr as u32, cancel)?;

        if cancel.is_canceled() {
            return Err(MediaError::Canceled);
        }

        let enhanced_samples = denoise(&mut model, pcm.samples(), cancel)?;

        let output = denoised_output_path(input);
        write_wav_sibling(&output, &enhanced_samples, model.sr as u32)?;
        Ok(output)
    }
}

/// Runs `samples` (mono `f32` at `model.sr`) through `model`, hop-size frame
/// by hop-size frame, checking `cancel` between frames — the loop can run
/// long on big files, and this is the only place in the call chain that
/// iterates rather than making one subprocess/model call.
///
/// A canceled token mid-loop returns `Err(MediaError::Canceled)`; the queue
/// seam that calls [`DfNoiseReducer::reduce`] treats *any* `Err` as a
/// degrade-to-prior-audio signal rather than a distinct cancel path (unlike
/// the isolation step, which does distinguish cancellation) — acceptable
/// here because the pipeline's own top-level cancel check runs again
/// immediately after this step returns (`queue.rs`'s `run_job`), so a
/// cancellation noticed here is never silently lost, just classified one
/// step later than it technically could be.
///
/// The trailing partial hop (fewer than `model.hop_size` samples) is left
/// as silence rather than processed or copied through — the reference CLI
/// (`libDF/src/bin/enhance_wav.rs`, the exact pattern the M8 spike
/// recorded) does the same: `process` requires an exact `hop_size`-length
/// frame, so the reference breaks out of its loop on a shorter final chunk.
/// The output buffer is pre-sized to the *full* input length, so this only
/// silences a sub-hop remainder (well under 10ms at 48kHz) rather than
/// shortening the file.
fn denoise(
    model: &mut DfTract,
    samples: &[f32],
    cancel: &CancelToken,
) -> Result<Vec<f32>, MediaError> {
    let hop = model.hop_size;
    // Row-major (1, n) — DfTract's mono (`n_ch = 1`) shape convention.
    let noisy = Array2::from_shape_vec((1, samples.len()), samples.to_vec()).map_err(|err| {
        MediaError::ReductionFailed {
            detail: format!("failed to shape decoded PCM for DeepFilterNet: {err}"),
        }
    })?;
    let mut enhanced = Array2::<f32>::zeros((1, samples.len()));

    for (noisy_frame, enh_frame) in noisy
        .view()
        .axis_chunks_iter(Axis(1), hop)
        .zip(enhanced.view_mut().axis_chunks_iter_mut(Axis(1), hop))
    {
        if cancel.is_canceled() {
            return Err(MediaError::Canceled);
        }
        if noisy_frame.len_of(Axis(1)) < hop {
            break;
        }
        model
            .process(noisy_frame, enh_frame)
            .map_err(|err| MediaError::ReductionFailed {
                detail: format!("DeepFilterNet inference failed: {err}"),
            })?;
    }

    Ok(enhanced.row(0).to_vec())
}

/// `<input_stem>_DeepFilterNet3.wav` next to `input` (Buzz naming,
/// `plugin.py:48`).
fn denoised_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{stem}_DeepFilterNet3.wav"))
}

/// Writes `samples` (mono `f32` at `sample_rate`) to `dest` atomically: a
/// same-directory temp file, written in full, then renamed into place — the
/// Task 5 precedent (`humm_media`'s `copy_then_rename`, applied here to an
/// in-memory buffer rather than an existing file) so no partial WAV is ever
/// observable at `dest` on any failure path.
fn write_wav_sibling(dest: &Path, samples: &[f32], sample_rate: u32) -> Result<(), MediaError> {
    let mut bytes = Vec::new();
    write_wav_mono_i16(samples, sample_rate, &mut bytes).map_err(MediaError::Io)?;

    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output.wav".to_string());
    let temp = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    match std::fs::write(&temp, &bytes).and_then(|_| std::fs::rename(&temp, dest)) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&temp);
            Err(MediaError::Io(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humm_core::cancel::CancelToken;
    use pretty_assertions::assert_eq;

    /// Writes a canonical 44-byte-header 16-bit PCM mono WAV containing a
    /// sine wave — a self-contained input fixture (no Buzz testdata
    /// dependency) for these unit tests.
    fn write_sine_wav(path: &Path, freq_hz: f64, seconds: f64, sample_rate: u32, amplitude: f64) {
        let num_samples = (seconds * f64::from(sample_rate)).round() as u32;
        let mut samples = Vec::with_capacity(num_samples as usize * 2);
        for i in 0..num_samples {
            let t = f64::from(i) / f64::from(sample_rate);
            let value = amplitude * (2.0 * std::f64::consts::PI * freq_hz * t).sin();
            let sample_i16 = (value * f64::from(i16::MAX)).round() as i16;
            samples.extend_from_slice(&sample_i16.to_le_bytes());
        }

        let data_len = samples.len() as u32;
        let byte_rate = sample_rate * 2;
        let mut buf = Vec::with_capacity(44 + samples.len());
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_len).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&samples);

        std::fs::write(path, buf).expect("write sine wav fixture");
    }

    /// Reads back the handful of WAV header fields these tests need,
    /// matching the exact 44-byte no-extension-chunk layout
    /// `humm_core::wav::write_wav_mono_i16` always produces (this crate's
    /// own writer, not a general-purpose WAV parser).
    struct WavInfo {
        sample_rate: u32,
        num_samples: usize,
    }

    fn read_wav_info(path: &Path) -> WavInfo {
        let bytes = std::fs::read(path).expect("read wav output");
        assert_eq!(&bytes[0..4], b"RIFF", "not a RIFF file");
        assert_eq!(&bytes[8..12], b"WAVE", "not a WAVE file");
        assert_eq!(&bytes[12..16], b"fmt ", "missing fmt chunk");
        assert_eq!(
            &bytes[36..40],
            b"data",
            "missing data chunk immediately after fmt"
        );
        let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]);
        assert_eq!(bits_per_sample, 16, "expected 16-bit PCM");
        let data_len = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
        WavInfo {
            sample_rate,
            num_samples: data_len / 2,
        }
    }

    // This test's `reduce()` call composes `humm-media`'s `FfmpegDecoder`
    // (this crate's module doc, "Adapter composition, not core") to decode
    // the synthesized input WAV at DeepFilterNet3's own 48 kHz rate before
    // running the real embedded model — so, unlike this module's other
    // tests, it needs a real `ffmpeg` on `PATH`, same requirement
    // `humm-media`'s own `#[ignore = "requires ffmpeg"]` contract tests
    // gate on (`humm-media/tests/contract.rs`). Gated the same way here
    // rather than left ungated, so a machine without ffmpeg still gets a
    // green `cargo test --workspace` (S9, M8 final review); this is the
    // workspace's only automated real-model DFN coverage, so it stays
    // `#[ignore]`d rather than deleted — `cargo test -p humm-dfn --
    // --ignored` (with ffmpeg on `PATH`) still runs it for real.
    #[test]
    #[ignore = "requires ffmpeg on PATH (DfNoiseReducer composes humm-media's \
                FfmpegDecoder to decode the synthesized input WAV)"]
    fn reduce_writes_sibling_wav_with_expected_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("noisy-input.wav");
        // A short (0.3s) fixture keeps the real embedded-model run cheap —
        // the model itself is CPU-only and this file is tiny.
        write_sine_wav(&input, 440.0, 0.3, 16_000, 0.4);

        let decoder = Arc::new(FfmpegDecoder::new());
        let reducer = DfNoiseReducer::new(decoder);
        let cancel = CancelToken::new();

        let output = reducer
            .reduce(&input, &cancel)
            .expect("reduce should succeed against a real tiny fixture");

        assert_eq!(
            output,
            dir.path().join("noisy-input_DeepFilterNet3.wav"),
            "Buzz naming: <input_stem>_DeepFilterNet3.wav beside the input"
        );
        assert!(output.exists(), "denoised output should exist on disk");

        let info = read_wav_info(&output);
        assert_eq!(info.sample_rate, 48_000, "DeepFilterNet3's own rate");
        let expected_samples = (0.3 * 48_000.0) as usize;
        let tolerance = (expected_samples as f64 * 0.05).ceil() as usize;
        assert!(
            info.num_samples.abs_diff(expected_samples) <= tolerance,
            "expected ~{expected_samples} samples (±5%), got {}",
            info.num_samples
        );
    }

    // Also needs a real ffmpeg on PATH: without it, the spawn itself fails
    // with `MediaError::BinaryMissing` rather than the `DecodeFailed` this
    // test asserts (ffmpeg genuinely trying and failing to open a
    // nonexistent input is what produces `DecodeFailed`) — see the previous
    // test's comment.
    #[test]
    #[ignore = "requires ffmpeg on PATH (needs a real ffmpeg failing to open \
                the input, not a binary-missing spawn error)"]
    fn reduce_failure_on_missing_input_is_err() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist.wav");

        let decoder = Arc::new(FfmpegDecoder::new());
        let reducer = DfNoiseReducer::new(decoder);
        let cancel = CancelToken::new();

        let err = reducer
            .reduce(&missing, &cancel)
            .expect_err("a missing input file must not produce a denoised output");

        assert!(
            matches!(err, MediaError::DecodeFailed { .. }),
            "expected DecodeFailed (ffmpeg failing on a nonexistent input), got {err:?}"
        );
        assert!(
            !dir.path()
                .join("does-not-exist_DeepFilterNet3.wav")
                .exists(),
            "no output file should be created on a decode failure"
        );
    }

    #[test]
    fn reduce_atomic_placement_leaves_no_partial_or_temp_file_on_write_failure() {
        // Simulate an unwritable destination directory: `dest`'s parent
        // doesn't exist, so both the temp-file write and the rename must
        // fail, and nothing partial should be left anywhere observable.
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("noisy-input.wav");
        write_sine_wav(&input, 440.0, 0.2, 16_000, 0.4);

        let missing_dir = dir.path().join("does-not-exist-subdir");
        let dest = missing_dir.join("noisy-input_DeepFilterNet3.wav");

        let err = write_wav_sibling(&dest, &[0.0f32; 100], 48_000)
            .expect_err("writing into a nonexistent directory should fail");
        assert!(matches!(err, MediaError::Io(_)));
        assert!(!dest.exists());
        assert!(
            !missing_dir.exists(),
            "the write must not have created the missing parent directory as a side effect"
        );
    }

    #[test]
    fn reduce_pre_canceled_token_short_circuits_before_building_the_model() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("noisy-input.wav");
        write_sine_wav(&input, 440.0, 0.2, 16_000, 0.4);

        let decoder = Arc::new(FfmpegDecoder::new());
        let reducer = DfNoiseReducer::new(decoder);
        let cancel = CancelToken::new();
        cancel.cancel();

        let err = reducer
            .reduce(&input, &cancel)
            .expect_err("pre-canceled reduce should be canceled");
        assert!(matches!(err, MediaError::Canceled));
    }

    #[test]
    fn denoised_output_path_appends_deepfilternet3_suffix_beside_the_input() {
        assert_eq!(
            denoised_output_path(Path::new("/audio/clip.mp3")),
            PathBuf::from("/audio/clip_DeepFilterNet3.wav")
        );
        assert_eq!(
            denoised_output_path(Path::new("clip.wav")),
            PathBuf::from("clip_DeepFilterNet3.wav")
        );
    }

    #[test]
    fn write_wav_sibling_writes_a_valid_wav_and_no_leftover_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("out_DeepFilterNet3.wav");

        write_wav_sibling(&dest, &[0.0, 0.5, -0.5, 1.0], 48_000).expect("write should succeed");

        let info = read_wav_info(&dest);
        assert_eq!(info.sample_rate, 48_000);
        assert_eq!(info.num_samples, 4);

        let siblings: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            siblings.len(),
            1,
            "expected only the destination file, no leftover temp: {siblings:?}"
        );
    }
}
