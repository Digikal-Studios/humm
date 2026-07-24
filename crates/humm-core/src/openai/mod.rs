//! humm's OpenAI-compatible HTTP service (design spec §2): shared client
//! plumbing ([`client`]), the Preferences "Test" key-validation check
//! ([`validate`]), the pure response-parsing half of the transcription engine
//! ([`parse`], including its whisper.cpp-server word-merge port), and the
//! engine itself ([`engine`], the OpenAI-compatible API file-transcription
//! backend — F-041, design spec §3). The LLM translator
//! (`openai::translate`, F-031) lands in a later M6 task on top of this
//! shared plumbing.
//!
//! The API key travels as an in-memory `String` inside these types only:
//! constructor argument -> `Authorization` header. It is never
//! `Debug`-printed, logged, serialized, or placed in a subprocess argv
//! (design spec §2).

pub mod client;
pub mod engine;
pub mod parse;
pub mod validate;

pub use client::{ApiFailure, DEFAULT_BASE_URL, OpenAiAuth, cancelable, effective_base};
pub use engine::{MAX_CHUNK_SIZE, OpenAiEngine, OpenAiEngineConfig, chunk_plan};
pub use parse::{parse_response, response_format_for};
pub use validate::{ValidateOutcome, validate_key};
