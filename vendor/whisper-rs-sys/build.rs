//! Unpack pinned whisper-rs-sys 0.15.0 and compile it with one source change.
//!
//! `upstream.rs` is that crate's `build.rs` with `main` renamed to `run`.
//! It still expects to be executed with the crate root as the process
//! working directory (`./whisper.cpp`, `wrapper.h`, `src/bindings.rs`).

mod upstream;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// crates.io SHA-256 of whisper-rs-sys 0.15.0 (same value as Cargo.lock
/// before this path patch).
const CRATE_SHA256: &str = "6986c0fe081241d391f09b9a071fbcbb59720c3563628c3c829057cf69f2a56f";

/// Bump if [`COREML_INIT_WITH_GPU_GATE`] changes, so a stale OUT_DIR cmake
/// tree is rebuilt. The stamp also includes the crate digest.
const PATCH_ID: &str = "skip-coreml-when-use-gpu-false-v1";

/// The ungated CoreML block in whisper.cpp 1.8.3 `whisper_init_state`.
const COREML_INIT_UNCONDITIONAL: &str = r#"#ifdef WHISPER_USE_COREML
    const auto path_coreml = whisper_get_coreml_path_encoder(ctx->path_model);

    WHISPER_LOG_INFO("%s: loading Core ML model from '%s'\n", __func__, path_coreml.c_str());
    WHISPER_LOG_INFO("%s: first run on a device may take a while ...\n", __func__);

    state->ctx_coreml = whisper_coreml_init(path_coreml.c_str());
    if (!state->ctx_coreml) {
        WHISPER_LOG_ERROR("%s: failed to load Core ML model from '%s'\n", __func__, path_coreml.c_str());
#ifndef WHISPER_COREML_ALLOW_FALLBACK
        whisper_free_state(state);
        return nullptr;
#endif
    } else {
        WHISPER_LOG_INFO("%s: Core ML model loaded\n", __func__);
    }
#endif"#;

/// Same block, skipped when the context was opened with `use_gpu = false`.
const COREML_INIT_WITH_GPU_GATE: &str = r#"#ifdef WHISPER_USE_COREML
    // humm: use_gpu == false is HUMM_FORCE_CPU / settings.models.force_cpu.
    // whisper.cpp 1.8.3 otherwise calls whisper_coreml_init here even when
    // the ggml backend is CPU-only. That probe SIGSEGVs on Apple Silicon
    // both when the encoder is missing and when it is present. Metal is
    // already skipped in whisper_backend_init_gpu via params.use_gpu.
    if (!ctx->params.use_gpu) {
        state->ctx_coreml = nullptr;
        WHISPER_LOG_INFO("%s: skipping Core ML because use_gpu = 0\n", __func__);
    } else {
        const auto path_coreml = whisper_get_coreml_path_encoder(ctx->path_model);

        WHISPER_LOG_INFO("%s: loading Core ML model from '%s'\n", __func__, path_coreml.c_str());
        WHISPER_LOG_INFO("%s: first run on a device may take a while ...\n", __func__);

        state->ctx_coreml = whisper_coreml_init(path_coreml.c_str());
        if (!state->ctx_coreml) {
            WHISPER_LOG_ERROR("%s: failed to load Core ML model from '%s'\n", __func__, path_coreml.c_str());
#ifndef WHISPER_COREML_ALLOW_FALLBACK
            whisper_free_state(state);
            return nullptr;
#endif
        } else {
            WHISPER_LOG_INFO("%s: Core ML model loaded\n", __func__);
        }
    }
#endif"#;

fn main() {
    let src_root = prepare_patched_sources();
    env::set_current_dir(&src_root).unwrap_or_else(|err| {
        panic!(
            "failed to enter unpacked whisper-rs-sys at {}: {err}",
            src_root.display()
        )
    });
    upstream::run();
}

fn prepare_patched_sources() -> PathBuf {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=upstream.rs");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=whisper-rs-sys-0.15.0.crate");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let crate_bytes = fs::read(manifest.join("whisper-rs-sys-0.15.0.crate"))
        .expect("read vendored whisper-rs-sys-0.15.0.crate");
    let digest = sha256_hex(&crate_bytes);
    if digest != CRATE_SHA256 {
        panic!(
            "whisper-rs-sys-0.15.0.crate sha256 {digest} does not match the pinned {CRATE_SHA256}"
        );
    }

    let stamp_text = format!("{digest}\n{PATCH_ID}\n");
    let stamp_path = out.join("humm-coreml-skip.stamp");
    let unpacked = out.join("unpacked");
    let src_root = unpacked.join("whisper-rs-sys-0.15.0");
    let patched_cpp = src_root.join("whisper.cpp/src/whisper.cpp");
    let stamp_ok = fs::read_to_string(&stamp_path).ok().as_deref() == Some(stamp_text.as_str());
    if !stamp_ok || !patched_cpp.is_file() {
        let _ = fs::remove_dir_all(&unpacked);
        // upstream.rs copies into OUT_DIR/whisper.cpp only when that directory
        // is absent, and cmake reuses OUT_DIR/build. Drop both so a patch
        // change cannot keep compiling the previous whisper.cpp.
        let _ = fs::remove_dir_all(out.join("whisper.cpp"));
        let _ = fs::remove_dir_all(out.join("build"));
        fs::create_dir_all(&unpacked).expect("create unpack directory");
        unpack_crate(&crate_bytes, &unpacked);
        patch_whisper_cpp(&patched_cpp);
        fs::write(&stamp_path, &stamp_text).expect("write patch stamp");
    }
    src_root
}

fn unpack_crate(bytes: &[u8], dest: &Path) {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(dest)
        .expect("unpack whisper-rs-sys-0.15.0.crate");
}

fn patch_whisper_cpp(path: &Path) {
    let original = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!(
            "read {} to apply the CoreML use_gpu gate: {err}",
            path.display()
        )
    });
    let matches = original.matches(COREML_INIT_UNCONDITIONAL).count();
    if matches != 1 {
        panic!(
            "expected exactly one ungated CoreML init block in {}, found {matches} — \
             whisper-rs-sys 0.15.0 / whisper.cpp 1.8.3 moved; update the patch in build.rs",
            path.display()
        );
    }
    let patched = original.replacen(COREML_INIT_UNCONDITIONAL, COREML_INIT_WITH_GPU_GATE, 1);
    fs::write(path, patched)
        .unwrap_or_else(|err| panic!("write patched {}: {err}", path.display()));
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}
