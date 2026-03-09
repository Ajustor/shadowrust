//! Prevents the OS from sleeping or dimming the display while the app runs.
//!
//! - Windows : `SetThreadExecutionState` (kernel32)
//! - macOS   : spawns `caffeinate -i -d` as a child process
//! - Linux   : D-Bus call to `org.freedesktop.ScreenSaver.Inhibit`

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub struct SleepInhibitor {
    #[cfg(target_os = "windows")]
    _prev: u32,
    #[cfg(target_os = "macos")]
    _child: Option<std::process::Child>,
    #[cfg(target_os = "linux")]
    _cookie: Option<u32>,
}

// ── Public API ────────────────────────────────────────────────────────────────

impl SleepInhibitor {
    /// Acquires a sleep-prevention lock. Automatically released on drop.
    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        {
            let _prev = windows::inhibit();
            log::info!("Sleep inhibited (SetThreadExecutionState)");
            SleepInhibitor { _prev }
        }

        #[cfg(target_os = "macos")]
        {
            let _child = macos::spawn_caffeinate();
            if _child.is_some() {
                log::info!("Sleep inhibited (caffeinate)");
            }
            SleepInhibitor { _child }
        }

        #[cfg(target_os = "linux")]
        {
            let _cookie = linux::dbus_inhibit();
            SleepInhibitor { _cookie }
        }
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        windows::release();

        #[cfg(target_os = "macos")]
        if let Some(ref mut child) = self._child {
            let _ = child.wait();
        }

        #[cfg(target_os = "linux")]
        if let Some(cookie) = self._cookie {
            linux::dbus_uninhibit(cookie);
            log::info!("Sleep inhibit released (D-Bus cookie {cookie})");
        }
    }
}

impl Default for SleepInhibitor {
    fn default() -> Self {
        Self::new()
    }
}
