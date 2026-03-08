//! Prevents the OS from sleeping or dimming the display while the app runs.
//!
//! - Windows : `SetThreadExecutionState` (kernel32)
//! - macOS   : spawns `caffeinate -i -d` as a child process
//! - Linux   : D-Bus call to `org.freedesktop.ScreenSaver.Inhibit`

pub struct SleepInhibitor {
    #[cfg(target_os = "windows")]
    _prev: u32,
    #[cfg(target_os = "macos")]
    _child: Option<std::process::Child>,
    #[cfg(target_os = "linux")]
    _cookie: Option<u32>,
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

    unsafe extern "system" {
        pub fn SetThreadExecutionState(esFlags: u32) -> u32;
    }

    pub fn inhibit() -> u32 {
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) }
    }
    pub fn release() {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

// ── macOS ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn spawn_caffeinate() -> Option<std::process::Child> {
    std::process::Command::new("caffeinate")
        .args(["-i", "-d"])
        .spawn()
        .map_err(|e| log::warn!("caffeinate spawn failed: {e}"))
        .ok()
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn dbus_inhibit() -> Option<u32> {
    // Try systemd-inhibit as a fallback subprocess approach (no extra deps)
    // and also attempt the D-Bus ScreenSaver interface.
    // We use std::process::Command to call dbus-send — zero extra crates.
    use std::process::Command;

    // dbus-send call: org.freedesktop.ScreenSaver Inhibit
    let out = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.Inhibit",
            "string:ShadowRust",
            "string:Capture in progress",
        ])
        .output()
        .ok()?;

    if out.status.success() {
        // Parse the uint32 cookie from the reply
        let reply = String::from_utf8_lossy(&out.stdout);
        let cookie = reply
            .split_whitespace()
            .last()
            .and_then(|s| s.parse::<u32>().ok());
        if let Some(c) = cookie {
            log::info!("Sleep inhibited via D-Bus ScreenSaver (cookie {c})");
            return Some(c);
        }
    }
    log::warn!("D-Bus ScreenSaver inhibit unavailable; sleep not prevented");
    None
}

#[cfg(target_os = "linux")]
fn dbus_uninhibit(cookie: u32) {
    let _ = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver.UnInhibit",
            &format!("uint32:{cookie}"),
        ])
        .status();
}

// ── Public API ────────────────────────────────────────────────────────────────

impl SleepInhibitor {
    /// Acquires a sleep-prevention lock. Automatically released on drop.
    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        {
            let _prev = win::inhibit();
            log::info!("Sleep inhibited (SetThreadExecutionState)");
            SleepInhibitor { _prev }
        }

        #[cfg(target_os = "macos")]
        {
            let _child = spawn_caffeinate();
            if _child.is_some() {
                log::info!("Sleep inhibited (caffeinate)");
            }
            SleepInhibitor { _child }
        }

        #[cfg(target_os = "linux")]
        {
            let _cookie = dbus_inhibit();
            SleepInhibitor { _cookie }
        }
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        win::release();

        #[cfg(target_os = "macos")]
        if let Some(ref mut child) = self._child {
            let _ = child.wait(); // will already have exited if app exits normally
        }

        #[cfg(target_os = "linux")]
        if let Some(cookie) = self._cookie {
            dbus_uninhibit(cookie);
            log::info!("Sleep inhibit released (D-Bus cookie {cookie})");
        }
    }
}

impl Default for SleepInhibitor {
    fn default() -> Self {
        Self::new()
    }
}
