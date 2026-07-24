//! humm's OpenAI-compatible HTTP service (design spec §2): shared client
//! plumbing ([`client`]), the Preferences "Test" key-validation check
//! ([`validate`]), and the pure response-parsing half of the transcription
//! engine ([`parse`]), including its whisper.cpp-server word-merge port
//! (F-041, design spec §3.4). The request-sending half of the engine
//! (`openai::engine`) and the LLM translator (`openai::translate`, F-031)
//! land in later M6 tasks on top of this shared plumbing.
//!
//! The API key travels as an in-memory `String` inside these types only:
//! constructor argument -> `Authorization` header. It is never
//! `Debug`-printed, logged, serialized, or placed in a subprocess argv
//! (design spec §2).

pub mod client;
pub mod parse;
pub mod validate;

pub use client::{ApiFailure, DEFAULT_BASE_URL, OpenAiAuth, cancelable, effective_base};
pub use parse::{parse_response, response_format_for};
pub use validate::{ValidateOutcome, validate_key};
