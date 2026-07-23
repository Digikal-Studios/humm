//! humm domain library: domain types, boundary traits, export writers, model
//! registry/downloader, and the transcription pipeline. Zero UI/adapter
//! dependencies; adapters implement the traits defined here.
#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod boundary;
pub mod cancel;
pub mod domain;
pub mod env;
pub mod error;
pub mod export;
pub mod format;
pub mod instance;
pub mod language;
pub mod live;
pub mod models;
pub mod paths;
pub mod pipeline;
pub mod queue;
pub mod regroup;
pub mod resize;
pub mod settings;
pub mod telemetry;
pub mod update;
pub mod viewer;
