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
    /// In release GUI mode (windows_subsystem = "windows") there is no console,
    /// so all diagnostics are written to a startup log file next to the exe
    /// (or in %TEMP% as a fallback).  Check `shadowrust-startup.log` to debug
    /// DLL loading problems.
    ///
    /// Search order:
    ///   1. `libs\` folder next to the exe  (portable zip distribution)
    ///   2. Embedded bytes extracted to `%LOCALAPPDATA%\ShadowRust\dlls`
    ///   3. DLLs next to the exe  (flat layout fallback)
    #[cfg(windows)]
    pub fn setup() {
        use std::ffi::OsStr;
        use std::io::Write as _;
        use std::os::windows::ffi::OsStrExt;
        use std::path::PathBuf;

        #[allow(unsafe_code)]
        unsafe extern "system" {
            fn SetDllDirectoryW(lp_path_name: *const u16) -> i32;
            fn LoadLibraryExW(
                lp_lib_file_name: *const u16,
                h_file: *mut std::ffi::c_void,
                dw_flags: u32,
            ) -> *mut std::ffi::c_void;
        }

        // LOAD_WITH_ALTERED_SEARCH_PATH: when loading a DLL from an absolute
        // path, Windows searches that DLL's directory first for its own
        // dependencies. This means avcodec-61.dll will find avutil-59.dll in
        // libs\ without any additional SetDllDirectoryW needed.
        const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x00000008;

        // ── Startup log (visible even in windowless GUI mode) ─────────────────
        let log_path: PathBuf = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("shadowrust-startup.log")))
            .unwrap_or_else(|| std::env::temp_dir().join("shadowrust-startup.log"));

        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .ok();

        macro_rules! dll_log {
            ($($arg:tt)*) => {
                let msg = format!($($arg)*);
                eprintln!("{msg}");
                if let Some(ref mut f) = log {
                    let _ = writeln!(f, "{msg}");
                }
            };
        }

        dll_log!("[shadowrust] dll_bundle::setup() — log: {log_path:?}");

        fn to_wide(s: &OsStr) -> Vec<u16> {
            s.encode_wide().chain(std::iter::once(0u16)).collect()
        }

        /// Set `dir` as an additional DLL search directory and pre-load every
        /// `.dll` found inside.  Returns the number of DLLs successfully loaded.
        fn preload_dlls_from(dir: &PathBuf, log: &mut Option<std::fs::File>) -> usize {
            use std::io::Write as _;

            macro_rules! plog {
                ($($arg:tt)*) => {
                    let msg = format!($($arg)*);
                    eprintln!("{msg}");
                    if let Some(ref mut f) = *log {
                        let _ = writeln!(f, "{msg}");
                    }
                };
            }

            // SetDllDirectoryW adds this folder to the standard search order so
            // transitive dependencies (avcodec → avutil, etc.) can be resolved.
            let wide_dir = to_wide(dir.as_os_str());
            let ok = unsafe { SetDllDirectoryW(wide_dir.as_ptr()) };
            if ok == 0 {
                plog!("[shadowrust] WARNING: SetDllDirectoryW({dir:?}) failed");
            } else {
                plog!("[shadowrust] SetDllDirectoryW({dir:?}) OK");
            }

            let Ok(entries) = std::fs::read_dir(dir) else {
                plog!("[shadowrust] ERROR: cannot read directory {dir:?}");
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
                fn priority(name: &str) -> u8 {
                    if name.starts_with("avutil") { 0 }
                    else if name.starts_with("swresample") { 1 }
                    else if name.starts_with("swscale") { 2 }
                    else if name.starts_with("postproc") { 3 }
                    else if name.starts_with("avcodec") { 4 }
                    else if name.starts_with("avformat") { 5 }
                    else if name.starts_with("avfilter") { 6 }
                    else if name.starts_with("avdevice") { 7 }
                    else { 8 }
                }
                priority(&na).cmp(&priority(&nb)).then(na.cmp(&nb))
            });

            plog!("[shadowrust] Found {} DLL(s) in {dir:?}", dll_paths.len());

            let mut loaded = 0usize;
            let mut failed = Vec::new();
            for path in &dll_paths {
                let wide = to_wide(path.as_os_str());
                // LOAD_WITH_ALTERED_SEARCH_PATH: Windows searches the DLL's own
                // directory (libs\) for its transitive dependencies automatically.
                let handle = unsafe {
                    LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH)
                };
                if handle.is_null() {
                    failed.push(path.file_name().unwrap().to_string_lossy().to_string());
                } else {
                    loaded += 1;
                }
            }
            if !failed.is_empty() {
                plog!(
                    "[shadowrust] WARNING: failed to load {} DLL(s): {}",
                    failed.len(),
                    failed.join(", ")
                );
            }
            plog!("[shadowrust] Loaded {loaded}/{} DLL(s) from {dir:?}", dll_paths.len());
            loaded
        }

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        // ── 1. libs\ next to the exe ─────────────────────────────────────────
        if let Some(ref exe_dir) = exe_dir {
            let libs_dir = exe_dir.join("libs");
            dll_log!("[shadowrust] Checking libs\\ at {libs_dir:?} — exists: {}", libs_dir.is_dir());
            if libs_dir.is_dir() {
                let n = preload_dlls_from(&libs_dir, &mut log);
                if n > 0 {
                    dll_log!("[shadowrust] ✓ Loaded {n} DLL(s) from libs\\ {libs_dir:?}");
                    return;
                }
                dll_log!(
                    "[shadowrust] WARNING: libs\\ found but no DLLs loaded from {libs_dir:?}"
                );
            }
        }

        // ── 2. Embedded bytes → extract then pre-load ────────────────────────
        dll_log!("[shadowrust] Embedded DLLs count: {}", BUNDLED_DLLS.len());
        if !BUNDLED_DLLS.is_empty() {
            let dll_dir = std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("ShadowRust")
                .join("dlls");

            if let Err(e) = std::fs::create_dir_all(&dll_dir) {
                dll_log!("[shadowrust] ERROR: Cannot create DLL dir {dll_dir:?}: {e}");
            } else {
                for (name, bytes) in BUNDLED_DLLS {
                    let path = dll_dir.join(name);
                    if let Err(e) = std::fs::write(&path, bytes) {
                        dll_log!("[shadowrust] ERROR: Cannot write {path:?}: {e}");
                    }
                }

                let n = preload_dlls_from(&dll_dir, &mut log);
                if n > 0 {
                    dll_log!("[shadowrust] ✓ Loaded {n} embedded DLL(s) from {dll_dir:?}");
                    return;
                }
            }
        }

        // ── 3. Flat layout: DLLs next to the exe (no libs\ subfolder) ───────
        if let Some(ref exe_dir) = exe_dir {
            let n = preload_dlls_from(exe_dir, &mut log);
            if n > 0 {
                dll_log!("[shadowrust] ✓ Loaded {n} DLL(s) from exe directory {exe_dir:?}");
                return;
            }
        }

        dll_log!(
            "[shadowrust] ERROR: No FFmpeg DLLs found anywhere. \
             Place them in libs\\ next to the exe."
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
