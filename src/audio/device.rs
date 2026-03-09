use cpal::traits::{DeviceTrait, HostTrait};

/// Find an input device matching the given name hint, or return the default.
pub(crate) fn find_input_device(
    host: &cpal::Host,
    name_hint: Option<&str>,
) -> Option<cpal::Device> {
    if let Some(hint) = name_hint {
        let hint_lower = hint.to_lowercase();
        if let Ok(mut devs) = host.input_devices() {
            let matched = devs.find(|d| {
                d.name()
                    .map(|n| n.to_lowercase().contains(&hint_lower))
                    .unwrap_or(false)
            });
            if let Some(dev) = matched {
                return Some(dev);
            }
        }
    }
    host.default_input_device()
}

/// List all available audio input device names.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}
