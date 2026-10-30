//! `FfmpegDecoder` benchmark (Task 9: criterion suite + M1 baselines).
//!
//! Decodes Buzz's `whisper-french.mp3` fixture end to end via the real
//! `ffmpeg` subprocess. Skips (printing a line, registering no measurements)
//! when either `ffmpeg` or the Buzz testdata directory is unavailable — this
//! bench must never panic on a machine lacking those prerequisites.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use criterion::{Criterion, criterion_group, criterion_main};

use humm_core::boundary::AudioDecoder;
use humm_core::cancel::CancelToken;
use humm_media::FfmpegDecoder;

/// Root of Buzz's test fixtures, honoring `HUMM_BUZZ_TESTDATA` (Global
/// Constraint 4), same default as the contract tests.
fn testdata_dir() -> PathBuf {
    std::env::var_os("HUMM_BUZZ_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/jawad/RustroverProjects/buzz/testdata"))
}

/// Resolves the ffmpeg binary the same way `FfmpegDecoder::new` does
/// (`HUMM_FFMPEG` override, else `ffmpeg` on `PATH`).
fn ffmpeg_binary() -> PathBuf {
    std::env::var_os("HUMM_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// `true` iff the resolved ffmpeg binary can actually be spawned.
fn ffmpeg_available() -> bool {
    Command::new(ffmpeg_binary())
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn bench_decode(c: &mut Criterion) {
    let audio_path = testdata_dir().join("whisper-french.mp3");

    if !ffmpeg_available() {
        println!(
            "SKIP humm-media decode bench: ffmpeg not found — {} or set HUMM_FFMPEG",
            humm_core::platform::subprocess_install_hint(std::env::consts::OS, "ffmpeg")
        );
        return;
    }
    if !audio_path.is_file() {
        println!(
            "SKIP humm-media decode bench: Buzz testdata not found at {} — set HUMM_BUZZ_TESTDATA",
            audio_path.display()
        );
        return;
    }

    let decoder = FfmpegDecoder::new();

    let mut group = c.benchmark_group("decode");
    group.sample_size(10);
    group.bench_function("ffmpeg_decode/whisper-french.mp3", |b| {
        b.iter(|| {
            let cancel = CancelToken::new();
            let pcm = decoder
                .decode(std::hint::black_box(&audio_path), &cancel)
                .expect("decode should succeed");
            std::hint::black_box(pcm);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_decode);
criterion_main!(benches);
