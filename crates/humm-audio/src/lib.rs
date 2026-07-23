//! `humm-audio` — the audio-output adapter crate (ADR-005).
//!
//! Two pieces sit behind `humm-core`'s [`AudioOutput`]/[`OutputStream`]
//! boundary:
//!
//! - [`CpalOutput`] — the cpal output-device boundary. It opens the system
//!   default output device, negotiates a supported configuration, and (per
//!   ADR-005) resamples with rubato *inside the adapter* when the device
//!   cannot run at the caller's requested rate, so the `fill` contract — the
//!   [`StreamSpec`] rate the caller produces frames at — is preserved.
//! - [`Player`] — the varispeed playback engine that drives a decoded
//!   [`PlaybackPcm`] through an [`AudioOutput`]. Its source-frame position
//!   counter is the viewer's A/V master clock. It is generic over
//!   `dyn AudioOutput`, so unit tests run it against a mock output with a
//!   simulated clock and no real audio thread.
//!
//! This crate touches an OS boundary but, like the rest of humm outside
//! `humm-whisper`, contains no `unsafe` (project rule for M4).
//!
//! [`AudioOutput`]: humm_core::boundary::AudioOutput
//! [`OutputStream`]: humm_core::boundary::OutputStream
//! [`StreamSpec`]: humm_core::boundary::StreamSpec
//! [`PlaybackPcm`]: humm_core::boundary::PlaybackPcm
#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod output;
mod player;

pub use output::CpalOutput;
pub use player::Player;
