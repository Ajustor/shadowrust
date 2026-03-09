// Hide the terminal window in release builds on Windows.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod audio;
mod capture;
mod config;
mod decode;
mod icon;
mod power;
mod record;
mod render;
mod ui;
mod updater;

use anyhow::Result;
use app::App;

// ── Bundled FFmpeg DLLs (Windows only) ───────────────────────────────────────
//
// build.rs generates dlls.rs which contains:
//   pub static BUNDLED_DLLS: &[(&str, &[u8])] = &[("avcodec-61.dll", include_bytes!(...)), ...];
//
// On non-Windows builds it is an empty slice — zero overhead.
mod dll_bundle {
    include!(concat!(env!("OUT_DIR"), "/dlls.rs"));

    /// Extract embedded FFmpeg DLLs to %LOCALAPPDATA%\ShadowRust\dlls and
    /// register that directory with Windows so it is searched before PATH.
    /// Also checks for a libs\ folder next to the exe as a first priority.
    /// Must be called before any FFmpeg function is invoked.
    #[cfg(windows)]
    pub fn setup() {
        #[allow(unsafe_code)]
        unsafe {
            use std::os::windows::ffi::OsStrExt;
            unsafe extern "system" {
                fn SetDllDirectoryW(path: *const u16) -> i32;
            }

            // ── Priority 1: libs\ folder next to the exe ──────────────────────
            // If the user extracted the zip, the DLLs are in exe_dir\libs\.
            // This is the fastest path and works without writing to AppData.
            if let Ok(exe) = std::env::current_exe() {
                if let Some(exe_dir) = exe.parent() {
                    let libs_dir = exe_dir.join("libs");
                    if libs_dir.is_dir()
                        && std::fs::read_dir(&libs_dir)
                            .map(|mut d| d.any(|e| {
                                e.ok().map(|e| {
                                    e.file_name()
                                        .to_string_lossy()
                                        .to_lowercase()
                                        .ends_with(".dll")
                                }).unwrap_or(false)
                            }))
                            .unwrap_or(false)
                    {
                        let wide: Vec<u16> = libs_dir
                            .as_os_str()
                            .encode_wide()
                            .chain(std::iter::once(0u16))
                            .collect();
                        SetDllDirectoryW(wide.as_ptr());
                        log::info!("FFmpeg DLLs loaded from {:?}", libs_dir);
                        return;
                    }
                }
            }

            // ── Priority 2: embedded DLLs → extract to LocalAppData ───────────
            if BUNDLED_DLLS.is_empty() {
                log::warn!(
                    "No FFmpeg DLLs found in libs\\ and none embedded — \
                     recording will not be available."
                );
                return;
            }

            let dll_dir = std::env::var("LOCALAPPDATA")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("ShadowRust")
                .join("dlls");

            if let Err(e) = std::fs::create_dir_all(&dll_dir) {
                log::error!("Cannot create DLL dir {dll_dir:?}: {e}");
                return;
            }

            for (name, bytes) in BUNDLED_DLLS {
                let path = dll_dir.join(name);
                // Always overwrite so updates replace stale DLLs
                if let Err(e) = std::fs::write(&path, bytes) {
                    log::error!("Cannot write {path:?}: {e}");
                }
            }

            let wide: Vec<u16> = dll_dir
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0u16))
                .collect();
            SetDllDirectoryW(wide.as_ptr());

            log::info!(
                "FFmpeg DLLs extracted to {:?} ({} files)",
                dll_dir,
                BUNDLED_DLLS.len()
            );
        }
    }

    #[cfg(not(windows))]
    pub fn setup() {}
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("ShadowRust starting…");

    // Extract embedded FFmpeg DLLs before the first FFmpeg call.
    // On non-Windows this is a no-op.
    dll_bundle::setup();

    // macOS requires requesting camera permission before any AVFoundation call.
    #[cfg(target_os = "macos")]
    {
        nokhwa::nokhwa_initialize(|granted| {
            if granted {
                log::info!("Camera permission granted");
            } else {
                log::error!("Camera permission denied — video capture will not work");
            }
        });
        // Give AVFoundation a moment to process the permission grant.
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    let event_loop = winit::event_loop::EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}
