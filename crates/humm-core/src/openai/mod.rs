//! humm's OpenAI-compatible HTTP service (design spec §2): shared client
//! plumbing ([`client`]) and the Preferences "Test" key-validation check
//! ([`validate`]). The transcription engine (`openai::engine`, F-041) and
//! LLM translator (`openai::translate`, F-031) land in later M6 tasks on
//! top of this shared plumbing.
//!
//! The API key travels as an in-memory `String` inside these types only:
//! constructor argument -> `Authorization` header. It is never
//! `Debug`-printed, logged, serialized, or placed in a subprocess argv
//! (design spec §2).

pub mod client;
pub mod validate;

pub use client::{ApiFailure, DEFAULT_BASE_URL, OpenAiAuth, cancelable, effective_base};
pub use validate::{ValidateOutcome, validate_key};
