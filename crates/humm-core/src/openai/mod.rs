//! humm's OpenAI-compatible HTTP service (design spec §2): shared client
//! plumbing ([`client`]), the Preferences "Test" key-validation check
//! ([`validate`]), the pure response-parsing half of the transcription engine
//! ([`parse`], including its whisper.cpp-server word-merge port), and the
//! engine itself ([`engine`], the OpenAI-compatible API file-transcription
//! backend — F-041, design spec §3), and the LLM translator ([`translate`],
//! F-031/F-063/F-085, design spec §4.1-§4.3 — batch protocol, worker
//! thread, and endpoint resolution).
//!
//! The API key travels as an in-memory `String` inside these types only:
//! constructor argument -> `Authorization` header. It is never
//! `Debug`-printed, logged, serialized, or placed in a subprocess argv
//! (design spec §2).

pub mod client;
pub mod engine;
pub mod parse;
pub mod translate;
pub mod validate;

pub use client::{ApiFailure, DEFAULT_BASE_URL, OpenAiAuth, cancelable, effective_base};
pub use engine::{MAX_CHUNK_SIZE, OpenAiEngine, OpenAiEngineConfig, chunk_plan};
pub use parse::{parse_response, response_format_for};
pub use translate::{
    BATCH_SIZE, ENV_BASE, ENV_KEY, LlmEndpoint, TranslatorConfig, TranslatorHandle,
    batch_system_prompt, combine_numbered, parse_batch_response, resolve_endpoint,
};
pub use validate::{ValidateOutcome, validate_key};
