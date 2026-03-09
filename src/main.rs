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

    /// Load FFmpeg DLLs before any delay-load stub fires.
    ///
    /// Strategy: pre-load every DLL by its **full absolute path** via
    /// `LoadLibraryExW`. Windows caches loaded modules by name, so when the
    /// delay-load stub later calls `LoadLibrary("avcodec-61.dll")` it gets the
    /// already-loaded module — no search-path games needed.
    ///
    /// Search order:
    ///   1. `libs\` folder next to the exe  (zip distribution)
    ///   2. Embedded bytes extracted to `%LOCALAPPDATA%\ShadowRust\dlls`
    #[cfg(windows)]
    pub fn setup() {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::path::PathBuf;

        #[allow(unsafe_code)]
        unsafe extern "system" {
            fn LoadLibraryExW(
                lp_lib_file_name: *const u16,
                h_file: *mut std::ffi::c_void,
                dw_flags: u32,
            ) -> *mut std::ffi::c_void;
        }

        fn to_wide(s: &OsStr) -> Vec<u16> {
            s.encode_wide().chain(std::iter::once(0u16)).collect()
        }

        fn preload_dlls_from(dir: &PathBuf) -> usize {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return 0;
            };
            let mut loaded = 0usize;
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_lowercase();
                if !name_str.ends_with(".dll") {
                    continue;
                }
                let full = dir.join(&name);
                let wide = to_wide(full.as_os_str());
                // LOAD_WITH_ALTERED_SEARCH_PATH = 0x8 — uses the DLL's own
                // directory as base for any of its own imports.
                let handle = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x8) };
                if handle.is_null() {
                    eprintln!("[shadowrust] WARNING: could not load {:?}", full);
                } else {
                    loaded += 1;
                }
            }
            loaded
        }

        // ── 1. libs\ next to the exe ─────────────────────────────────────────
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let libs_dir = exe_dir.join("libs");
                if libs_dir.is_dir() {
                    let n = preload_dlls_from(&libs_dir);
                    if n > 0 {
                        eprintln!("[shadowrust] Loaded {n} FFmpeg DLL(s) from {libs_dir:?}");
                        return;
                    }
                }
            }
        }

        // ── 2. Embedded bytes → extract then pre-load ────────────────────────
        if BUNDLED_DLLS.is_empty() {
            eprintln!(
                "[shadowrust] WARNING: No FFmpeg DLLs in libs\\ and none embedded — \
                 recording will not be available."
            );
            return;
        }

        let dll_dir = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("ShadowRust")
            .join("dlls");

        if let Err(e) = std::fs::create_dir_all(&dll_dir) {
            eprintln!("[shadowrust] ERROR: Cannot create DLL dir {dll_dir:?}: {e}");
            return;
        }

        for (name, bytes) in BUNDLED_DLLS {
            let path = dll_dir.join(name);
            if let Err(e) = std::fs::write(&path, bytes) {
                eprintln!("[shadowrust] ERROR: Cannot write {path:?}: {e}");
            }
        }

        let n = preload_dlls_from(&dll_dir);
        eprintln!("[shadowrust] Loaded {n} embedded FFmpeg DLL(s) from {dll_dir:?}");
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
