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
    /// Strategy: call `SetDllDirectoryW` to add the `libs\` folder to the
    /// standard Windows DLL search path, then pre-load every DLL via
    /// `LoadLibraryW` with full path.  `SetDllDirectoryW` is critical: when
    /// loading e.g. `avcodec-61.dll`, its dependency `avutil-59.dll` is
    /// resolved through the standard search which now includes `libs\`.
    ///
    /// Search order:
    ///   1. `libs\` folder next to the exe  (portable zip distribution)
    ///   2. Embedded bytes extracted to `%LOCALAPPDATA%\ShadowRust\dlls`
    ///   3. DLLs next to the exe  (flat layout fallback)
    #[cfg(windows)]
    pub fn setup() {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::path::PathBuf;

        #[allow(unsafe_code)]
        unsafe extern "system" {
            fn SetDllDirectoryW(lp_path_name: *const u16) -> i32;
            fn LoadLibraryW(lp_lib_file_name: *const u16) -> *mut std::ffi::c_void;
        }

        fn to_wide(s: &OsStr) -> Vec<u16> {
            s.encode_wide().chain(std::iter::once(0u16)).collect()
        }

        /// Set `dir` as an additional DLL search directory and pre-load every
        /// `.dll` found inside.  Returns the number of DLLs successfully loaded.
        fn preload_dlls_from(dir: &PathBuf) -> usize {
            // SetDllDirectoryW inserts this directory into the standard DLL
            // search order, right after the application directory.  This is
            // essential for transitive dependencies between FFmpeg DLLs
            // (avcodec → avutil, swscale → avutil, …).
            let wide_dir = to_wide(dir.as_os_str());
            let ok = unsafe { SetDllDirectoryW(wide_dir.as_ptr()) };
            if ok == 0 {
                eprintln!(
                    "[shadowrust] WARNING: SetDllDirectoryW failed for {:?}",
                    dir
                );
            }

            let Ok(entries) = std::fs::read_dir(dir) else {
                eprintln!("[shadowrust] WARNING: cannot read directory {:?}", dir);
                return 0;
            };

            // Collect DLL paths and sort so that avutil (no deps) is loaded
            // before avcodec (depends on avutil), etc.
            let mut dll_paths: Vec<_> = entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .ends_with(".dll")
                })
                .map(|e| e.path())
                .collect();
            dll_paths.sort_by(|a, b| {
                let na = a.file_name().unwrap().to_string_lossy().to_lowercase();
                let nb = b.file_name().unwrap().to_string_lossy().to_lowercase();
                // Load avutil first (everything depends on it), then sw*, then the rest
                fn priority(name: &str) -> u8 {
                    if name.starts_with("avutil") {
                        0
                    } else if name.starts_with("swresample") {
                        1
                    } else if name.starts_with("swscale") {
                        2
                    } else if name.starts_with("postproc") {
                        3
                    } else if name.starts_with("avcodec") {
                        4
                    } else if name.starts_with("avformat") {
                        5
                    } else if name.starts_with("avfilter") {
                        6
                    } else if name.starts_with("avdevice") {
                        7
                    } else {
                        8
                    }
                }
                priority(&na).cmp(&priority(&nb)).then(na.cmp(&nb))
            });

            let mut loaded = 0usize;
            let mut failed = Vec::new();
            for path in &dll_paths {
                let wide = to_wide(path.as_os_str());
                let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
                if handle.is_null() {
                    failed.push(path.file_name().unwrap().to_string_lossy().to_string());
                } else {
                    loaded += 1;
                }
            }
            if !failed.is_empty() {
                eprintln!(
                    "[shadowrust] WARNING: failed to load {} DLL(s): {}",
                    failed.len(),
                    failed.join(", ")
                );
            }
            loaded
        }

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        // ── 1. libs\ next to the exe ─────────────────────────────────────────
        if let Some(ref exe_dir) = exe_dir {
            let libs_dir = exe_dir.join("libs");
            if libs_dir.is_dir() {
                let n = preload_dlls_from(&libs_dir);
                if n > 0 {
                    eprintln!("[shadowrust] Loaded {n} DLL(s) from {libs_dir:?}");
                    return;
                }
                eprintln!(
                    "[shadowrust] WARNING: libs\\ folder found but no DLLs loaded from {libs_dir:?}"
                );
            }
        }

        // ── 2. Embedded bytes → extract then pre-load ────────────────────────
        if !BUNDLED_DLLS.is_empty() {
            let dll_dir = std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("ShadowRust")
                .join("dlls");

            if let Err(e) = std::fs::create_dir_all(&dll_dir) {
                eprintln!("[shadowrust] ERROR: Cannot create DLL dir {dll_dir:?}: {e}");
            } else {
                for (name, bytes) in BUNDLED_DLLS {
                    let path = dll_dir.join(name);
                    if let Err(e) = std::fs::write(&path, bytes) {
                        eprintln!("[shadowrust] ERROR: Cannot write {path:?}: {e}");
                    }
                }

                let n = preload_dlls_from(&dll_dir);
                if n > 0 {
                    eprintln!("[shadowrust] Loaded {n} embedded DLL(s) from {dll_dir:?}");
                    return;
                }
            }
        }

        // ── 3. Flat layout: DLLs next to the exe (no libs\ subfolder) ───────
        if let Some(ref exe_dir) = exe_dir {
            let n = preload_dlls_from(exe_dir);
            if n > 0 {
                eprintln!("[shadowrust] Loaded {n} DLL(s) from exe directory {exe_dir:?}");
                return;
            }
        }

        eprintln!(
            "[shadowrust] WARNING: No FFmpeg DLLs found — \
             place them in libs\\ next to the exe or set FFMPEG_DIR at build time."
        );
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
