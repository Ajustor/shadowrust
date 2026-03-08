use std::path::{Path, PathBuf};

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir);

    // Generate the application icon as an ICO file and embed it into the
    // Windows EXE via winres. On other platforms only the DLL bundle step runs.
    if target_os == "windows" {
        let ico_path = out_path.join("shadowrust.ico");
        let rgba = render_icon(256);
        std::fs::write(&ico_path, encode_ico(&rgba, 256)).expect("write shadowrust.ico");

        // winres embeds the .ico into the PE binary (shows in Explorer / taskbar).
        let mut res = winres::WindowsResource::new();
        res.set_icon(ico_path.to_str().unwrap());
        res.compile().expect("compile Windows resources");

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

// ── Icon generation ───────────────────────────────────────────────────────────
// Mirrors src/icon.rs — duplicated here so build.rs stays dependency-free.

/// Render the ShadowRust icon as RGBA pixels (dark bg + play arrow + red dot).
fn render_icon(size: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let cx = s * 0.5;
    let cy = s * 0.5;
    let outer_r = s * 0.47;

    let tri_cx = cx - s * 0.025;
    let tri_cy = cy;
    let tri_r = s * 0.20;
    let (tx1, ty1) = (tri_cx + tri_r, tri_cy);
    let (tx2, ty2) = (tri_cx - tri_r * 0.65, tri_cy - tri_r * 0.88);
    let (tx3, ty3) = (tri_cx - tri_r * 0.65, tri_cy + tri_r * 0.88);

    let dot_cx = cx + s * 0.285;
    let dot_cy = cy + s * 0.285;
    let dot_r = s * 0.155;
    let ring_inner = dot_r + s * 0.012;
    let ring_outer = dot_r + s * 0.038;

    for row in 0..size {
        for col in 0..size {
            let idx = ((row * size + col) * 4) as usize;
            let x = col as f32 + 0.5;
            let y = row as f32 + 0.5;
            let dx = x - cx;
            let dy = y - cy;
            let d = (dx * dx + dy * dy).sqrt();

            if d > outer_r {
                continue;
            }
            let edge_aa = ((outer_r - d) / 1.5_f32).clamp(0.0, 1.0);
            let t = (d / outer_r).clamp(0.0, 1.0);
            let mut r = lrp(15.0, 30.0, t) as u8;
            let mut g = lrp(20.0, 35.0, t) as u8;
            let mut b = lrp(45.0, 72.0, t) as u8;

            if in_tri(x, y, tx1, ty1, tx2, ty2, tx3, ty3) {
                r = 240;
                g = 240;
                b = 240;
            }

            let dd = idist(x, y, dot_cx, dot_cy);
            if dd >= ring_inner && dd <= ring_outer {
                let rt = ((dd - ring_inner) / (ring_outer - ring_inner)).clamp(0.0, 1.0);
                let ra = 1.0 - (rt * 2.0 - 1.0).abs();
                r = lrp(r as f32, 255.0, ra * 0.9) as u8;
                g = lrp(g as f32, 255.0, ra * 0.9) as u8;
                b = lrp(b as f32, 255.0, ra * 0.9) as u8;
            }
            if dd <= dot_r {
                let da = ((dot_r - dd) / 1.5_f32).clamp(0.0, 1.0);
                let hl = (1.0 - (idist(x, y, dot_cx - dot_r * 0.28, dot_cy - dot_r * 0.28)
                    / (dot_r * 0.9))
                    .clamp(0.0, 1.0))
                    * 0.35;
                r = lrp(r as f32, lrp(220.0, 255.0, hl), da) as u8;
                g = lrp(g as f32, lrp(50.0, 100.0, hl), da) as u8;
                b = lrp(b as f32, lrp(50.0, 80.0, hl), da) as u8;
            }

            buf[idx] = r;
            buf[idx + 1] = g;
            buf[idx + 2] = b;
            buf[idx + 3] = (edge_aa * 255.0) as u8;
        }
    }
    buf
}

fn idist(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    (dx * dx + dy * dy).sqrt()
}

fn lrp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn in_tri(
    px: f32, py: f32,
    v1x: f32, v1y: f32,
    v2x: f32, v2y: f32,
    v3x: f32, v3y: f32,
) -> bool {
    let sg = |ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32| -> f32 {
        (ax - cx) * (by - cy) - (bx - cx) * (ay - cy)
    };
    let d1 = sg(px, py, v1x, v1y, v2x, v2y);
    let d2 = sg(px, py, v2x, v2y, v3x, v3y);
    let d3 = sg(px, py, v3x, v3y, v1x, v1y);
    let neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(neg && pos)
}

/// Encode RGBA pixels as a Windows ICO file (BMP-based, 32-bit BGRA).
///
/// The ICO format stores pixels bottom-up in BGRA order and includes an AND
/// mask (all zeros = fully opaque, transparency comes from the alpha channel).
fn encode_ico(rgba: &[u8], size: u32) -> Vec<u8> {
    let w = size as i32;
    let h = size as i32;
    let pixel_data_len = (w * h * 4) as usize;
    // AND mask: 1 bit per pixel, rows padded to DWORD (4-byte) boundary
    let mask_row_bytes = ((size + 31) / 32 * 4) as usize;
    let mask_len = mask_row_bytes * size as usize;
    let bmp_len = 40 + pixel_data_len + mask_len;

    let mut ico = Vec::with_capacity(6 + 16 + bmp_len);

    // ICONDIR
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type: ICO
    ico.extend_from_slice(&1u16.to_le_bytes()); // count: 1 image

    // ICONDIRENTRY
    let s8 = if size >= 256 { 0u8 } else { size as u8 };
    ico.push(s8); // width  (0 = 256)
    ico.push(s8); // height (0 = 256)
    ico.push(0); // color count (true color)
    ico.push(0); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
    ico.extend_from_slice(&(bmp_len as u32).to_le_bytes()); // bytes in resource
    ico.extend_from_slice(&22u32.to_le_bytes()); // image offset (6 + 16 = 22)

    // BITMAPINFOHEADER (40 bytes)
    ico.extend_from_slice(&40u32.to_le_bytes());
    ico.extend_from_slice(&w.to_le_bytes());
    ico.extend_from_slice(&(h * 2).to_le_bytes()); // biHeight * 2 per ICO spec
    ico.extend_from_slice(&1u16.to_le_bytes()); // planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
    ico.extend_from_slice(&0u32.to_le_bytes()); // compression (BI_RGB)
    ico.extend_from_slice(&0u32.to_le_bytes()); // image size (can be 0)
    ico.extend_from_slice(&0i32.to_le_bytes()); // X pixels/meter
    ico.extend_from_slice(&0i32.to_le_bytes()); // Y pixels/meter
    ico.extend_from_slice(&0u32.to_le_bytes()); // colors used
    ico.extend_from_slice(&0u32.to_le_bytes()); // colors important

    // Pixel data: BGRA, bottom-up (last row of image first)
    for row in (0..size).rev() {
        for col in 0..size {
            let src = ((row * size + col) * 4) as usize;
            ico.push(rgba[src + 2]); // B
            ico.push(rgba[src + 1]); // G
            ico.push(rgba[src]); // R
            ico.push(rgba[src + 3]); // A
        }
    }

    // AND mask: all zeros (alpha channel carries transparency)
    ico.extend(std::iter::repeat(0u8).take(mask_len));

    ico
}
