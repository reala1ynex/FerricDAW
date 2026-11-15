//! Milestone 10: multi-channel device matrix + WASAPI exclusive / ASIO driver selection.

use cpal::traits::{DeviceTrait, HostTrait};

/// Audio host / share-mode selection for the studio engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioDriverKind {
    /// Default Windows Shared WASAPI via `cpal` (safe, higher latency).
    #[default]
    WasapiShared,
    /// Exclusive-mode WASAPI when the device allows it (lower latency, locks the device).
    WasapiExclusive,
    /// ASIO host when an ASIO driver is installed (ASIO4ALL / interface OEM).
    Asio,
}

impl AudioDriverKind {
    pub const ALL: &'static [AudioDriverKind] = &[
        AudioDriverKind::WasapiShared,
        AudioDriverKind::WasapiExclusive,
        AudioDriverKind::Asio,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AudioDriverKind::WasapiShared => "WASAPI Shared",
            AudioDriverKind::WasapiExclusive => "WASAPI Exclusive",
            // Portable builds ship without cpal's `asio` feature — selection still
            // attempts ASIO host discovery and falls back with an honesty note.
            AudioDriverKind::Asio => "ASIO (WASAPI if unavailable)",
        }
    }

    pub fn hover_hint(self) -> &'static str {
        match self {
            AudioDriverKind::WasapiShared => {
                "Default shared-mode WASAPI — safe, plays nice with other apps"
            }
            AudioDriverKind::WasapiExclusive => {
                "True exclusive-mode WASAPI — locks the output device for lower latency. Falls back to Shared if the device rejects exclusive."
            }
            AudioDriverKind::Asio => {
                "Requests the ASIO cpal host when this build includes it and ASIO4ALL / an interface driver is installed. Portable Windows releases usually fall back to WASAPI Shared — watch the amber honesty note after switching."
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub input_channels: u16,
    pub output_channels: u16,
    pub default_sample_rate: u32,
    pub min_buffer: u32,
    pub max_buffer: u32,
}

/// Interleaved multi-channel hardware input frame (up to 16 channels).
#[derive(Debug, Clone, Copy)]
pub struct HwMultiFrame {
    pub channels: [f32; 16],
    pub count: u8,
}

impl Default for HwMultiFrame {
    fn default() -> Self {
        Self {
            channels: [0.0; 16],
            count: 0,
        }
    }
}

impl HwMultiFrame {
    #[inline]
    pub fn get(self, ch: u8) -> f32 {
        if (ch as usize) < self.count as usize {
            self.channels[ch as usize]
        } else if self.count > 0 {
            self.channels[0]
        } else {
            0.0
        }
    }

    #[inline]
    pub fn stereo_pair(self, pair: u8) -> (f32, f32) {
        let l = self.get(pair.saturating_mul(2));
        let r = self.get(pair.saturating_mul(2).saturating_add(1));
        (l, r)
    }
}

pub fn list_audio_device_matrix() -> Vec<AudioDeviceInfo> {
    let mut out = Vec::new();
    let host = cpal::default_host();
    if let Ok(devs) = host.devices() {
        for d in devs {
            let Ok(name) = d.name() else { continue };
            let mut info = AudioDeviceInfo {
                name,
                input_channels: 0,
                output_channels: 0,
                default_sample_rate: 48_000,
                min_buffer: 64,
                max_buffer: 2048,
            };
            if let Ok(cfg) = d.default_input_config() {
                info.input_channels = cfg.channels();
                info.default_sample_rate = cfg.sample_rate().0;
                match cfg.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => {
                        info.min_buffer = *min;
                        info.max_buffer = *max;
                    }
                    cpal::SupportedBufferSize::Unknown => {}
                }
            }
            if let Ok(cfg) = d.default_output_config() {
                info.output_channels = cfg.channels();
                info.default_sample_rate = cfg.sample_rate().0;
                match cfg.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => {
                        info.min_buffer = info.min_buffer.min(*min);
                        info.max_buffer = info.max_buffer.max(*max);
                    }
                    cpal::SupportedBufferSize::Unknown => {}
                }
            }
            if info.input_channels > 0 || info.output_channels > 0 {
                out.push(info);
            }
        }
    }
    out
}

/// Result of resolving a cpal host for the selected driver.
pub struct ResolvedAudioHost {
    pub host: cpal::Host,
    /// Set when the requested driver could not be opened and we fell back.
    pub fallback_note: Option<&'static str>,
}

/// Prefer an ASIO `cpal` host when the user selected ASIO; otherwise default host.
pub fn host_for_driver(kind: AudioDriverKind) -> ResolvedAudioHost {
    match kind {
        AudioDriverKind::Asio => {
            for id in cpal::available_hosts() {
                // ASIO host id is feature-gated; match by debug name so builds without `asio` still compile.
                if format!("{id:?}").to_lowercase().contains("asio") {
                    if let Ok(host) = cpal::host_from_id(id) {
                        return ResolvedAudioHost {
                            host,
                            fallback_note: None,
                        };
                    }
                }
            }
            ResolvedAudioHost {
                host: cpal::default_host(),
                fallback_note: Some(
                    "ASIO host not found — using WASAPI Shared. Install ASIO4ALL or your interface driver.",
                ),
            }
        }
        // Exclusive is opened via wasapi_exclusive COM path; cpal host is still used for shared input.
        AudioDriverKind::WasapiExclusive => ResolvedAudioHost {
            host: cpal::default_host(),
            fallback_note: None,
        },
        AudioDriverKind::WasapiShared => ResolvedAudioHost {
            host: cpal::default_host(),
            fallback_note: None,
        },
    }
}

/// Best-effort exclusive hint: clamp to the smallest supported fixed buffer when Exclusive is selected.
pub fn exclusive_buffer_hint(kind: AudioDriverKind, requested: Option<u32>, min_buf: u32) -> Option<u32> {
    match kind {
        AudioDriverKind::WasapiExclusive | AudioDriverKind::Asio => {
            Some(requested.unwrap_or(min_buf.max(64)).max(32))
        }
        AudioDriverKind::WasapiShared => requested,
    }
}
