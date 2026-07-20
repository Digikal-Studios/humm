//! Contract test for `humm_core::models::download::ModelDownloader` against
//! the real Silero VAD artifact on huggingface.co (the smallest real
//! artifact the registry knows about). `#[ignore]`; run with:
//!
//! ```text
//! CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
//!     cargo test -p humm-core -- --ignored
//! ```

use std::time::SystemTime;

use humm_core::cancel::CancelToken;
use humm_core::models::download::{DownloadProgress, ModelDownloader};
use humm_core::models::registry;

/// Downloads the real Silero VAD model twice (fresh, then a fast-path
/// no-network replay), then exercises resume-from-truncated-partial and
/// confirms the resumed file's SHA256 matches the first download's.
#[test]
#[ignore = "requires network (huggingface.co)"]
fn ensure_vad_model_downloads_verifies_and_resumes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let downloader = ModelDownloader::new(dir.path());
    let cancel = CancelToken::new();

    // --- Fresh download ---
    let mut updates: Vec<DownloadProgress> = Vec::new();
    let final_path = downloader
        .ensure_vad_model(&mut |p| updates.push(p), &cancel)
        .expect("first ensure_vad_model should succeed");

    assert!(final_path.is_file(), "model file should exist");
    let sentinel_path = dir.path().join(format!(
        "{}{}",
        registry::VAD_MODEL_FILE,
        registry::SENTINEL_SUFFIX
    ));
    assert!(sentinel_path.is_file(), "sentinel should exist");
    assert!(
        !updates.is_empty(),
        "progress callback should have fired at least once"
    );
    assert!(
        updates
            .windows(2)
            .all(|w| w[1].downloaded >= w[0].downloaded),
        "downloaded should be monotonically increasing across updates: {updates:?}"
    );
    assert!(
        updates.last().unwrap().downloaded > 0,
        "final progress update should report nonzero bytes downloaded"
    );

    let first_sha256 = sha256_of(&final_path);
    let mtime_after_first = mtime(&final_path);

    // --- Replay: sentinel present, must be instant and network-free ---
    let mut replay_updates: Vec<DownloadProgress> = Vec::new();
    let replay_path = downloader
        .ensure_vad_model(&mut |p| replay_updates.push(p), &cancel)
        .expect("second ensure_vad_model should succeed via fast path");
    assert_eq!(replay_path, final_path);
    assert!(
        replay_updates.is_empty(),
        "fast path should not touch the network or report progress"
    );
    assert_eq!(
        mtime(&final_path),
        mtime_after_first,
        "fast-path replay must not rewrite the file (mtime unchanged)"
    );

    // --- Resume: delete sentinel, truncate the model file to half, rename
    // it to `.partial`, then ensure_vad_model should resume and produce a
    // file whose SHA256 matches the first download's. ---
    std::fs::remove_file(&sentinel_path).expect("remove sentinel");
    let full_bytes = std::fs::read(&final_path).expect("read full model file");
    let half_len = full_bytes.len() / 2;
    assert!(half_len > 0, "downloaded model should be nonempty");
    let half_bytes = &full_bytes[..half_len];

    let partial_path = dir
        .path()
        .join(format!("{}.partial", registry::VAD_MODEL_FILE));
    std::fs::write(&partial_path, half_bytes).expect("write truncated partial");
    std::fs::remove_file(&final_path).expect("remove final so only the partial remains");

    let mut resume_updates: Vec<DownloadProgress> = Vec::new();
    let resumed_path = downloader
        .ensure_vad_model(&mut |p| resume_updates.push(p), &cancel)
        .expect("resumed ensure_vad_model should succeed");

    assert_eq!(resumed_path, final_path);
    assert!(resumed_path.is_file());
    assert!(sentinel_path.is_file(), "sentinel should be rewritten");
    assert!(
        !resume_updates.is_empty(),
        "resume should still report progress"
    );
    assert!(
        resume_updates.first().unwrap().downloaded as usize >= half_len,
        "first progress update after resume should already reflect the resumed offset: {:?}",
        resume_updates.first()
    );

    let resumed_sha256 = sha256_of(&resumed_path);
    assert_eq!(
        resumed_sha256, first_sha256,
        "resumed download must produce byte-identical content to the fresh download"
    );
}

fn sha256_of(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).expect("read file for hashing");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

fn mtime(path: &std::path::Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .expect("read mtime")
}
