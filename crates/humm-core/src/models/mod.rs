//! Model registry and downloader for whisper.cpp ggml models, the optional
//! CoreML encoder, and the Silero VAD model (F-042, F-043).
//!
//! `registry` is pure name/URL/path-mapping logic; `download` performs the
//! actual network I/O (resume, streaming, SHA256 verification, CoreML zip
//! extraction) via [`download::ModelDownloader`].

pub mod download;
pub mod registry;
