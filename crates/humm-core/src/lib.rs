//! humm domain library: domain types, boundary traits, export writers, model
//! registry/downloader, and the transcription pipeline. Zero UI/adapter
//! dependencies; adapters implement the traits defined here.
#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod boundary;
pub mod cancel;
pub mod domain;
pub mod error;
pub mod export;
pub mod language;
pub mod models;
pub mod paths;
pub mod pipeline;
pub mod queue;
pub mod settings;
