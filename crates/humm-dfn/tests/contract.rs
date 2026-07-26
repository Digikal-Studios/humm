//! Contract test for `humm-dfn` against a real noise-mixed Buzz testdata
//! fixture (F-112). `#[ignore]`, and additionally gated on
//! `HUMM_DFN_CONTRACT=1` (mirrors `humm-media`'s `HUMM_DEMUCS_CONTRACT`
//! convention): this test builds its own noisy fixture via a real `ffmpeg`
//! subprocess and runs the full real embedded model on ~8.5s of audio,
//! meaningfully heavier than `humm-dfn`'s own tiny (0.3s) default unit
//! tests — the extra gate keeps a plain `cargo test -p humm-dfn --
//! --ignored` (which only needs ffmpeg + Buzz testdata, both already
//! assumed present) from being required to also budget this test's longer
//! wall time by default.
//!
//! Run with:
//!
//! ```text
//! CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
//!     HUMM_DFN_CONTRACT=1 cargo test -p humm-dfn -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use humm_core::boundary::NoiseReducer;
use humm_core::cancel::CancelToken;
use humm_dfn::DfNoiseReducer;
use humm_media::FfmpegDecoder;

/// Root of Buzz's test fixtures, honoring `HUMM_BUZZ_TESTDATA` (Global
/// Constraint 4) — identical convention to `humm-media`'s own contract
/// tests.
fn testdata() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

fn ffmpeg_binary() -> PathBuf {
    std::env::var_os("HUMM_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// Builds a noisy fixture at 48kHz mono: `prefix_secs` of pure white noise
/// (a known noise-only span, for the RMS-reduction assertion below),
/// followed by `speech` mixed continuously with the same white noise for
/// its whole duration (the spike's own `anoisesrc=color=white:amplitude=0.05`
/// mix, `validation/m8-spikes/README.md` Spike A) — three real `ffmpeg`
/// subprocess calls (noise prefix, mixed speech, concat), asserted to
/// succeed at each step.
fn build_noisy_fixture(dir: &Path, speech: &Path, prefix_secs: f64) -> PathBuf {
    let noise_prefix = dir.join("noise_prefix.wav");
    let status = Command::new(ffmpeg_binary())
        .args(["-y", "-f", "lavfi", "-i"])
        .arg(format!(
            "anoisesrc=color=white:amplitude=0.05:d={prefix_secs}"
        ))
        .args(["-ac", "1", "-ar", "48000"])
        .arg(&noise_prefix)
        .status()
        .expect("spawn ffmpeg (noise prefix)");
    assert!(status.success(), "ffmpeg noise-prefix generation failed");

    let mixed_speech = dir.join("mixed_speech.wav");
    let status = Command::new(ffmpeg_binary())
        .args(["-y", "-i"])
        .arg(speech)
        .args(["-f", "lavfi", "-i", "anoisesrc=color=white:amplitude=0.05"])
        .args([
            "-filter_complex",
            "[0:a]aformat=channel_layouts=mono,aresample=48000[speech];\
             [1:a]aformat=channel_layouts=mono,aresample=48000[noise];\
             [speech][noise]amix=inputs=2:duration=first:dropout_transition=0[out]",
            "-map",
            "[out]",
            "-ac",
            "1",
            "-ar",
            "48000",
        ])
        .arg(&mixed_speech)
        .status()
        .expect("spawn ffmpeg (mixed speech)");
    assert!(status.success(), "ffmpeg speech/noise mix failed");

    let combined = dir.join("noisy_input.wav");
    let status = Command::new(ffmpeg_binary())
        .args(["-y", "-i"])
        .arg(&noise_prefix)
        .args(["-i"])
        .arg(&mixed_speech)
        .args([
            "-filter_complex",
            "[0:a][1:a]concat=n=2:v=0:a=1[out]",
            "-map",
            "[out]",
            "-ac",
            "1",
            "-ar",
            "48000",
        ])
        .arg(&combined)
        .status()
        .expect("spawn ffmpeg (concat)");
    assert!(status.success(), "ffmpeg concat failed");

    combined
}

fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum_sq / samples.len() as f64).sqrt()
}

#[test]
#[ignore = "requires ffmpeg + Buzz testdata (set HUMM_DFN_CONTRACT=1); see this file's doc comment for provisioning"]
fn real_dfn_denoises_the_noise_mixed_whisper_french_fixture() {
    if std::env::var("HUMM_DFN_CONTRACT").as_deref() != Ok("1") {
        eprintln!(
            "SKIP real_dfn_denoises_the_noise_mixed_whisper_french_fixture: \
             HUMM_DFN_CONTRACT is not set to 1 (see this test's doc comment)"
        );
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let speech = testdata().join("whisper-french.mp3");
    let prefix_secs = 1.0;
    let noisy_input = build_noisy_fixture(dir.path(), &speech, prefix_secs);

    let decoder = Arc::new(FfmpegDecoder::new());
    let cancel = CancelToken::new();

    let noisy_pcm = decoder
        .decode_mono_at(&noisy_input, 48_000, &cancel)
        .expect("decode the noisy fixture");

    let reducer = DfNoiseReducer::new(Arc::clone(&decoder));
    let output = reducer
        .reduce(&noisy_input, &cancel)
        .expect("reduce should succeed against a real noisy fixture");
    assert!(output.exists(), "denoised output should exist on disk");

    let denoised_pcm = decoder
        .decode_mono_at(&output, 48_000, &cancel)
        .expect("decode the denoised output");

    // Duration ±5%.
    let expected_samples = noisy_pcm.len();
    let actual_samples = denoised_pcm.len();
    let tolerance = (expected_samples as f64 * 0.05).ceil() as usize;
    assert!(
        actual_samples.abs_diff(expected_samples) <= tolerance,
        "expected ~{expected_samples} samples (±5%), got {actual_samples}"
    );

    // RMS reduction in the known noise-only prefix span: the first
    // `prefix_secs` of the fixture is pure white noise with no speech at
    // all, so DeepFilterNet3 should suppress it far more aggressively than
    // it can the speech-plus-noise remainder.
    let prefix_samples = (prefix_secs * 48_000.0) as usize;
    let noisy_prefix_rms = rms(&noisy_pcm.samples()[..prefix_samples.min(noisy_pcm.len())]);
    let denoised_prefix_rms =
        rms(&denoised_pcm.samples()[..prefix_samples.min(denoised_pcm.len())]);

    eprintln!(
        "real_dfn_denoises_the_noise_mixed_whisper_french_fixture: \
         noisy_samples={expected_samples} denoised_samples={actual_samples} \
         noise_only_prefix_rms: noisy={noisy_prefix_rms:.5} denoised={denoised_prefix_rms:.5} \
         (ratio={:.3})",
        denoised_prefix_rms / noisy_prefix_rms
    );

    assert!(
        denoised_prefix_rms < noisy_prefix_rms * 0.5,
        "expected the noise-only prefix's RMS to drop by at least half: \
         noisy={noisy_prefix_rms:.5}, denoised={denoised_prefix_rms:.5}"
    );
}
