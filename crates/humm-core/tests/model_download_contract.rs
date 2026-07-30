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
use humm_core::error::ModelError;
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

/// Simulates the "process died after the body completed but before the
/// rename" window: a `.partial` byte-identical to the full resource. The next
/// `ensure_vad_model` sends `Range: bytes=<full-len>-`, the server answers
/// HTTP 416 (Range Not Satisfiable), and recovery must verify the `.partial`
/// against its published digest and promote it in place — never wedge in a
/// permanent 416 loop. (Finding 2.)
#[test]
#[ignore = "requires network (huggingface.co)"]
fn ensure_vad_model_recovers_from_http_416_completed_partial() {
    let dir = tempfile::tempdir().expect("tempdir");
    let downloader = ModelDownloader::new(dir.path());
    let cancel = CancelToken::new();

    // Fully download once to obtain the real, verified bytes.
    let final_path = downloader
        .ensure_vad_model(&mut |_| {}, &cancel)
        .expect("initial ensure_vad_model should succeed");
    let full_bytes = std::fs::read(&final_path).expect("read full model file");
    let expected_sha = sha256_of(&final_path);

    // Recreate the pre-rename crash state: a COMPLETE `.partial`, no final
    // file, no sentinel.
    let sentinel_path = dir.path().join(format!(
        "{}{}",
        registry::VAD_MODEL_FILE,
        registry::SENTINEL_SUFFIX
    ));
    let partial_path = dir
        .path()
        .join(format!("{}.partial", registry::VAD_MODEL_FILE));
    std::fs::write(&partial_path, &full_bytes).expect("write complete partial");
    std::fs::remove_file(&sentinel_path).expect("remove sentinel");
    std::fs::remove_file(&final_path).expect("remove final file");

    // This resume request gets a 416 (offset == resource length). Recovery
    // must verify-and-promote, not error out.
    let recovered = downloader
        .ensure_vad_model(&mut |_| {}, &cancel)
        .expect("ensure_vad_model should recover from HTTP 416, not wedge");

    assert_eq!(recovered, final_path);
    assert!(recovered.is_file(), "final file should be restored");
    assert!(sentinel_path.is_file(), "sentinel should be written");
    assert!(
        !partial_path.exists(),
        "the completed partial should have been renamed away"
    );
    assert_eq!(
        sha256_of(&recovered),
        expected_sha,
        "promoted file must be byte-identical to the verified download"
    );
}

/// Contract test for `ModelDownloader::resolve_hf_repo` and
/// `ModelDownloader::ensure_custom_model` against a fixed, real third-party
/// Hugging Face repo (F-042, `docs/analysis/PARITY_MATRIX.md`):
/// `ggerganov/whisper.cpp` itself — the exact repo humm's own runtime
/// already depends on for every ordinary ggml model download
/// (`registry::WHISPER_CPP_REPO`), so the M2 judgement that "no fixed real
/// target exists" no longer holds (see `docs/design/DECISIONS.md` ADR-008's
/// M9 addendum). `#[ignore]` as usual, and additionally gated on
/// `HUMM_HF_THIRDPARTY_CONTRACT=1` (mirrors `humm-dfn`'s
/// `HUMM_DFN_CONTRACT`/`humm-diarize`'s `HUMM_DIARIZE_CONTRACT` convention):
/// this is a second, independent gate on top of `#[ignore]`, so a plain
/// `cargo test -p humm-core -- --ignored` (this crate's own documented gate)
/// does not also have to budget this test's ~32 MB download by default.
///
/// Run with:
///
/// ```text
/// CARGO_HOME="$(git rev-parse --show-toplevel)/.cargo-home" \
///     HUMM_HF_THIRDPARTY_CONTRACT=1 cargo test -p humm-core --test model_download_contract \
///     -- --ignored third_party_hf_repo
/// ```
#[test]
#[ignore = "requires network (huggingface.co); additionally gated on HUMM_HF_THIRDPARTY_CONTRACT=1"]
fn third_party_hf_repo_resolves_and_custom_model_downloads() {
    if std::env::var("HUMM_HF_THIRDPARTY_CONTRACT").as_deref() != Ok("1") {
        // Same rationale as `humm-dfn`'s contract test: `println!`, not
        // `eprintln!`, and an early `Ok`-shaped return rather than a panic —
        // this test is already `#[ignore]`d, so it only runs under
        // `--ignored`; the second gate means it can still report a bare `ok`
        // there without ever touching huggingface.co. This line is what
        // keeps that from looking like a silently-trustworthy green run.
        println!(
            "SKIPPED (not run): third_party_hf_repo_resolves_and_custom_model_downloads — \
             HUMM_HF_THIRDPARTY_CONTRACT is not set to 1, so this reports `ok` without touching \
             huggingface.co at all (see this test's doc comment to provision it for real)"
        );
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let downloader = ModelDownloader::new(dir.path());
    let cancel = CancelToken::new();

    // --- (1) resolve_hf_repo against the live Hub API. `ggerganov/whisper.cpp`
    // publishes dozens of `ggml-*.bin` candidates (verified via the tree API
    // while writing this test, 2026-07-28: 33 `ggml-*.bin` siblings), so
    // `resolve_hf_repo` must refuse to auto-pick one — its `Unsupported`
    // error's `detail` names every candidate, which is the only
    // externally-observable proof (from outside the crate,
    // `list_hf_repo_files`/`select_ggml_candidates` are both private) that
    // the real API was fetched and filtered correctly rather than stubbed. ---
    let repo_err = downloader
        .resolve_hf_repo(registry::WHISPER_CPP_REPO, false, &mut |_| {}, &cancel)
        .expect_err(
            "ggerganov/whisper.cpp publishes many ggml-*.bin files, not one, so resolve_hf_repo \
             must return Unsupported rather than guessing",
        );
    let ModelError::Unsupported { detail } = repo_err else {
        panic!("expected ModelError::Unsupported (multiple candidates), got {repo_err:?}");
    };
    assert!(
        detail.contains("ggml-tiny.bin"),
        "the real repo's candidate list should be non-empty and include ggml-tiny.bin: {detail}"
    );

    // --- (2) ensure_custom_model against the pinned real third-party URL
    // (exact file name confirmed present via the tree API before pinning
    // it here — see this task's report). ---
    let url = format!(
        "{}/{}/resolve/main/ggml-tiny-q5_1.bin",
        registry::HF_BASE,
        registry::WHISPER_CPP_REPO
    );

    let mut updates: Vec<DownloadProgress> = Vec::new();
    let final_path = downloader
        .ensure_custom_model(&url, &mut |p| updates.push(p), &cancel)
        .expect("ensure_custom_model should download the pinned third-party artifact");

    assert!(final_path.is_file(), "model file should exist");
    let size = std::fs::metadata(&final_path)
        .expect("stat downloaded file")
        .len();
    assert!(
        (10_000_000..100_000_000).contains(&size),
        "ggml-tiny-q5_1.bin should be a plausible ~32MB file, got {size} bytes"
    );

    let sentinel_path = dir.path().join(format!(
        "{}{}",
        registry::custom_model_file_name(),
        registry::SENTINEL_SUFFIX
    ));
    assert!(sentinel_path.is_file(), "sentinel should exist");
    assert!(
        !updates.is_empty(),
        "progress callback should have fired at least once"
    );

    let mtime_after_first = mtime(&final_path);

    // --- Second call must be a no-download no-op: same technique as
    // `ensure_vad_model_downloads_verifies_and_resumes` above (empty
    // progress callback + unchanged mtime), since a real HTTP server here
    // has no request-count observation to assert against instead. ---
    let mut replay_updates: Vec<DownloadProgress> = Vec::new();
    let replay_path = downloader
        .ensure_custom_model(&url, &mut |p| replay_updates.push(p), &cancel)
        .expect("second ensure_custom_model call should hit the fast path");
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
