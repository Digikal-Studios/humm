//! Contract test for `humm-diarize`'s `OrtDiarizer` against the real,
//! committed two-speaker fixture (F-087, ADR-024, `validation/m8-spikes/
//! README.md` Spike B / "2-speaker fixture"). `#[ignore]`, and additionally
//! gated on `HUMM_DIARIZE_CONTRACT=1` (mirrors `humm-dfn`'s `HUMM_DFN_CONTRACT`/
//! `humm-media`'s `HUMM_DEMUCS_CONTRACT` convention, `contract.rs`): the plain
//! documented `cargo test -p humm-diarize -- --ignored` gate must stay green
//! even for someone who hasn't provisioned the real ~58MB model download —
//! without this second gate, that command would either panic (no models, no
//! network) or silently pull 58MB over the network (models absent, network
//! present), neither of which is what "just run the ignored tests" should do.
//! This test provisions the models itself via
//! `ModelDownloader::ensure_diarization_models` against the real
//! `avencera/speakrs-models` repo if they aren't already present under
//! `model_root()/diarization/`. Run with:
//!
//! ```text
//! CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
//!     HUMM_DIARIZE_CONTRACT=1 cargo test -p humm-diarize -- --ignored
//! ```
//!
//! Set `HUMM_MODEL_ROOT` to point at an existing model cache to avoid a
//! fresh download; both the download (if needed) and the diarization run
//! itself may need the sandbox lifted (network for the download; ONNX
//! Runtime/OpenBLAS may probe outside the sandboxed write set) — this is
//! expected and recorded in this task's report.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use humm_core::boundary::Pcm;
use humm_core::cancel::CancelToken;
use humm_core::diarize::{DiarizeProgress, Diarizer, SpeakerTurn};
use humm_core::models::download::ModelDownloader;
use humm_core::paths::model_root;
use humm_diarize::OrtDiarizer;

/// The fixture committed by Task 2 (`crates/humm-core/tests/fixtures/
/// diarize/two-speakers.wav`), referenced by relative path across the crate
/// boundary — the fixture is a real ~1.2MB binary artifact, so (unlike the
/// small-string precedent elsewhere in this workspace) inlining a copy isn't
/// reasonable.
fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../humm-core/tests/fixtures/diarize/two-speakers.wav")
}

/// Splice points from `validation/m8-spikes/README.md`'s "2-speaker fixture"
/// section: `audio-long.mp3[0,15s)` + `whisper-latvian.wav` (7.1915s) +
/// `audio-long.mp3[30,45s)`, so the speaker changes at 15.000s and
/// 15.000 + 7.1915 = 22.1915s.
const SPLICE_POINTS_MS: [i64; 2] = [15_000, 22_192];

/// Reads the fixture's `data` chunk directly (already 16kHz mono 16-bit PCM
/// — matching `humm_core::boundary::SAMPLE_RATE` exactly, so no resample or
/// channel-mix is needed). Walks RIFF chunks rather than assuming a fixed
/// 44-byte header: the fixture (written by ffmpeg) has a `LIST`/`INFO`
/// chunk between `fmt ` and `data` that a fixed-offset reader would miss.
fn read_fixture_pcm(path: &Path) -> Pcm {
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!("read fixture wav at {}: {e}", path.display());
    });
    assert_eq!(&bytes[0..4], b"RIFF", "not a RIFF file");
    assert_eq!(&bytes[8..12], b"WAVE", "not a WAVE file");

    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().expect("4 bytes")) as usize;
        let body_start = pos + 8;
        if chunk_id == b"data" {
            let body = &bytes[body_start..body_start + chunk_size];
            let samples: Vec<f32> = body
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .map(|s| f32::from(s) / 32_768.0)
                .collect();
            return Pcm::new(samples);
        }
        // RIFF chunks are padded to an even byte count.
        pos = body_start + chunk_size + (chunk_size % 2);
    }
    panic!("no data chunk found in {}", path.display());
}

/// The boundary times (start of the later turn) at which the speaker
/// actually changes, in chronological order — the real claim this test
/// checks against the splice points, not merely "some turn edge is near
/// here" (which a same-speaker segmentation edge could satisfy vacuously).
/// Turns are sorted defensively (see `SpeakerTurn`'s own doc on ordering);
/// this only looks at consecutive turns in that order.
fn speaker_change_boundaries_ms(turns: &[SpeakerTurn]) -> Vec<i64> {
    let mut sorted: Vec<&SpeakerTurn> = turns.iter().collect();
    sorted.sort_by_key(|t| t.start.0);
    sorted
        .windows(2)
        .filter(|pair| pair[0].speaker != pair[1].speaker)
        .map(|pair| pair[1].start.0)
        .collect()
}

#[test]
#[ignore = "requires HUMM_DIARIZE_CONTRACT=1 (real diarization models, downloaded from \
            HuggingFace on first run, ~58MB); see this file's doc comment for provisioning"]
fn two_speaker_fixture_yields_two_speakers() {
    if std::env::var("HUMM_DIARIZE_CONTRACT").as_deref() != Ok("1") {
        eprintln!(
            "SKIP two_speaker_fixture_yields_two_speakers: HUMM_DIARIZE_CONTRACT is not \
             set to 1 (see this test's doc comment) — set it to provision the real \
             ~58MB model download and run the real pipeline"
        );
        return;
    }

    let cancel = CancelToken::new();
    let downloader = ModelDownloader::new(model_root());
    let paths = downloader
        .ensure_diarization_models(&mut |_| {}, &cancel)
        .expect("ensure_diarization_models against the real HuggingFace repo");

    let diarizer = OrtDiarizer::new(paths).expect("models should be present after ensure");

    let audio = read_fixture_pcm(&fixture_path());
    assert!(!audio.is_empty(), "fixture should decode to non-empty PCM");

    let mut stages = Vec::new();
    let start = std::time::Instant::now();
    let turns = diarizer
        .diarize(
            &audio,
            &mut |p: DiarizeProgress| stages.push(p.stage),
            &cancel,
        )
        .expect("diarize should succeed against the real fixture");
    let elapsed = start.elapsed();

    eprintln!(
        "two_speaker_fixture_yields_two_speakers: {} turns, {} stage updates, {:.2}s wall \
         (Task 11 cancel-ack budget note: a cancel arriving mid-run only resolves once this \
         call returns)",
        turns.len(),
        stages.len(),
        elapsed.as_secs_f64()
    );
    for turn in &turns {
        eprintln!(
            "  speaker {} : {:.3}s - {:.3}s",
            turn.speaker,
            turn.start.0 as f64 / 1000.0,
            turn.end.0 as f64 / 1000.0
        );
    }

    let speakers: BTreeSet<usize> = turns.iter().map(|t| t.speaker).collect();
    assert!(
        speakers.len() >= 2,
        "expected >=2 speakers, got {}: {turns:?}",
        speakers.len()
    );

    let change_boundaries = speaker_change_boundaries_ms(&turns);
    eprintln!("  speaker-change boundaries (ms): {change_boundaries:?}");
    for splice_ms in SPLICE_POINTS_MS {
        let close = change_boundaries
            .iter()
            .any(|&boundary_ms| (boundary_ms - splice_ms).abs() <= 2_000);
        assert!(
            close,
            "expected a speaker-change boundary within +/-2s of splice point {splice_ms}ms; \
             actual speaker-change boundaries: {change_boundaries:?}, turns: {turns:?}"
        );
    }
}
