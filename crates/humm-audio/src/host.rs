//! Opens a cpal host without going through [`cpal::default_host`].
//!
//! `default_host` panics when the compiled-in default cannot be constructed
//! (`expect` on ALSA on Linux, WASAPI on Windows, CoreAudio on macOS).
//! [`cpal::host_from_id`] returns that failure as a `Result`. Walking
//! [`cpal::available_hosts`] keeps a missing audio stack as
//! [`AudioError::DeviceUnavailable`] instead of taking down the GUI.

use cpal::traits::HostTrait;
use humm_core::error::AudioError;

/// The first host cpal reports as available that actually initializes.
///
/// Host order is cpal's: PipeWire, PulseAudio, JACK, then ALSA on Linux
/// when those features are compiled in (this crate uses cpal's default
/// features, so Linux is ALSA); WASAPI on Windows; CoreAudio on macOS.
pub fn system_host() -> Result<cpal::Host, AudioError> {
    let mut detail = String::from("no audio host could be initialized");
    let hosts = cpal::available_hosts();
    if hosts.is_empty() {
        return Err(AudioError::DeviceUnavailable { detail });
    }
    for id in hosts {
        match cpal::host_from_id(id) {
            Ok(host) => return Ok(host),
            Err(err) => {
                detail = format!("{id:?} host unavailable: {err}");
            }
        }
    }
    Err(AudioError::DeviceUnavailable { detail })
}

/// Names of the input devices on `host`, marking the default.
pub fn input_device_list(
    host: &cpal::Host,
) -> Result<Vec<humm_core::boundary::AudioDevice>, AudioError> {
    let default_name = host.default_input_device().map(|device| device.to_string());
    let devices = host
        .input_devices()
        .map_err(|err| AudioError::DeviceUnavailable {
            detail: format!("could not enumerate input devices: {err}"),
        })?;
    Ok(devices
        .map(|device| {
            let name = device.to_string();
            let is_default = default_name.as_deref() == Some(name.as_str());
            humm_core::boundary::AudioDevice {
                id: name.clone(),
                name,
                is_default,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_the_system_host_returns_a_result() {
        // `Ok` when a host initializes, `Err` when it does not. Either one
        // passes, including a missing ALSA, WASAPI, or CoreAudio device.
        // No capture or playback stream is opened. A green result is not
        // device coverage.
        match system_host() {
            Ok(host) => {
                if let Ok(devices) = input_device_list(&host) {
                    let defaults = devices.iter().filter(|device| device.is_default).count();
                    assert!(defaults <= 1, "{devices:?}");
                }
            }
            Err(err) => {
                let detail = err.to_string();
                assert!(
                    detail.contains("audio")
                        || detail.contains("host")
                        || detail.contains("device"),
                    "{detail}"
                );
            }
        }
    }
}
