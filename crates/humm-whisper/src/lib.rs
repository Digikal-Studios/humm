//! humm-whisper: whisper-rs FFI adapter (Metal + CoreML) implementing the
//! `TranscriptionEngine` trait from `humm-core`. This is the sole crate in
//! the workspace permitted to use `unsafe`, and only at the FFI boundary,
//! each occurrence documented with a `// SAFETY:` invariant comment.

mod engine;
mod words;

pub use engine::WhisperEngine;
pub use words::{TokenInfo, group_words};
