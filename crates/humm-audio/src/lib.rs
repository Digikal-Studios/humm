//! `humm-audio` — the cpal audio adapter crate (ADR-005): output, capture,
//! and a scripted capture mock.
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
//! - [`CpalCapture`] — the cpal input-device boundary for live recording
//!   (M5, F-060/F-061). Negotiates 16 kHz mono `f32` directly where the
//!   device supports it, else falls back to the device's default
//!   configuration with adapter-side downmix + rubato resample — never in
//!   the RT callback (see the `capture` module docs).
//! - [`MockCapture`] — a scripted, threadless `AudioCapture` stand-in for
//!   core-session tests, GUI tests, and CI.
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

mod capture;
mod mock;
mod output;
mod player;

pub use capture::CpalCapture;
pub use mock::MockCapture;
pub use output::CpalOutput;
pub use player::Player;
