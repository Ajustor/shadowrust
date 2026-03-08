use std::path::{Path, PathBuf};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir);

    if target_os == "windows" {
        bundle_ffmpeg_dlls(out_path);
    } else {
        write_empty_bundle(out_path);
    }
}

/// On Windows targets: find FFmpeg DLLs in $FFMPEG_DIR/bin, embed them via
/// include_bytes! and add /DELAYLOAD linker flags so Windows resolves the
/// DLLs lazily (after our main() has had a chance to call SetDllDirectoryW).
fn bundle_ffmpeg_dlls(out_path: &Path) {
    let ffmpeg_dir = std::env::var("FFMPEG_DIR").unwrap_or_default();

    if ffmpeg_dir.is_empty() {
        eprintln!(
            "cargo:warning=FFMPEG_DIR not set — FFmpeg DLLs will NOT be embedded. Make sure they are on PATH at runtime."
        );
        write_empty_bundle(out_path);
        return;
    }

    let bin_dir = Path::new(&ffmpeg_dir).join("bin");
    let dlls = find_ffmpeg_dlls(&bin_dir);

    if dlls.is_empty() {
        eprintln!("cargo:warning=No FFmpeg DLLs found in {bin_dir:?} — they will not be embedded.");
        write_empty_bundle(out_path);
        return;
    }

    // /DELAYLOAD makes the Windows PE loader resolve these DLLs on first call
    // instead of at process start, giving main() time to call SetDllDirectoryW.
    // delayimp.lib provides the delay-load stub runtime.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("msvc") {
        for dll in &dlls {
            let name = dll.file_name().unwrap().to_string_lossy();
            println!("cargo:rustc-link-arg=/DELAYLOAD:{name}");
        }
        println!("cargo:rustc-link-lib=delayimp");
    }

    // Generate src that embeds every DLL as a byte slice.
    let mut code =
        String::from("#[allow(dead_code)]\npub static BUNDLED_DLLS: &[(&str, &[u8])] = &[\n");
    for dll in &dlls {
        let name = dll.file_name().unwrap().to_string_lossy();
        // Use forward slashes so the path is valid in both Rust string and
        // include_bytes! regardless of host quoting rules.
        let forward = dll.to_str().unwrap().replace('\\', "/");
        code += &format!("    ({name:?}, include_bytes!({forward:?})),\n");
    }
    code += "];\n";

    std::fs::write(out_path.join("dlls.rs"), code).expect("write dlls.rs");

    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    println!("cargo:rerun-if-changed={}", bin_dir.display());
}

fn write_empty_bundle(out_path: &Path) {
    std::fs::write(
        out_path.join("dlls.rs"),
        "#[allow(dead_code)]\npub static BUNDLED_DLLS: &[(&str, &[u8])] = &[];\n",
    )
    .expect("write dlls.rs");
}

/// Collect only the FFmpeg DLLs actually needed for our use case:
/// avcodec (H.264 encode), avformat (MP4 mux), avutil (shared utils),
/// swscale (RGBA→YUV pixel conversion), swresample (sample-rate conversion).
/// avdevice / avfilter / postproc are excluded — we use nokhwa for capture.
fn find_ffmpeg_dlls(bin_dir: &Path) -> Vec<PathBuf> {
    const NEEDED: &[&str] = &["avcodec", "avformat", "avutil", "swscale", "swresample"];

    match std::fs::read_dir(bin_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy().to_lowercase();
                n.ends_with(".dll") && NEEDED.iter().any(|p| n.starts_with(p))
            })
            .map(|e| e.path())
            .collect(),
        Err(err) => {
            eprintln!("cargo:warning=Cannot read {bin_dir:?}: {err}");
            vec![]
        }
    }
}
