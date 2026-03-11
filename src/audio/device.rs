use cpal::traits::{DeviceTrait, HostTrait};

/// Find an input device matching the given name hint.
///
/// - If `name_hint` is `Some(hint)` and a device matches → return it.
/// - If `name_hint` is `Some(hint)` but **no device matches** → return `None`
///   so the caller can decide not to start audio rather than accidentally
///   opening the system microphone.
/// - If `name_hint` is `None` → return the system default input device.
pub(crate) fn find_input_device(
    host: &cpal::Host,
    name_hint: Option<&str>,
) -> Option<cpal::Device> {
    match name_hint {
        Some(hint) => {
            let hint_lower = hint.to_lowercase();
            host.input_devices().ok()?.find(|d| {
                d.name()
                    .map(|n| n.to_lowercase().contains(&hint_lower))
                    .unwrap_or(false)
            })
            // No match → return None (do NOT fall back to default / mic).
        }
        None => host.default_input_device(),
    }
}

/// List all available audio input device names.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}
