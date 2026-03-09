/// Generate the ShadowRust application icon as raw RGBA pixels.
///
/// Design: dark circular background, white play arrow (capture),
/// and a red record dot (bottom-right) with a subtle white ring.
/// Looks clean at all sizes from 16×16 to 256×256.
pub fn make_icon_rgba(size: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let cx = s * 0.5;
    let cy = s * 0.5;
    let outer_r = s * 0.47;

    // Play triangle (right-pointing, centered slightly left)
    let tri_cx = cx - s * 0.025;
    let tri_cy = cy;
    let tri_r = s * 0.20;
    // Tip (right), top-left, bottom-left
    let (tx1, ty1) = (tri_cx + tri_r, tri_cy);
    let (tx2, ty2) = (tri_cx - tri_r * 0.65, tri_cy - tri_r * 0.88);
    let (tx3, ty3) = (tri_cx - tri_r * 0.65, tri_cy + tri_r * 0.88);

    // Record dot (red circle, bottom-right quadrant)
    let dot_cx = cx + s * 0.285;
    let dot_cy = cy + s * 0.285;
    let dot_r = s * 0.155;
    // White ring around the dot
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
                // Transparent outside the circular boundary
                continue;
            }

            // Smooth outer edge (anti-alias)
            let edge_aa = ((outer_r - d) / 1.5_f32).clamp(0.0, 1.0);

            // Background: dark navy with a subtle radial gradient
            let t = (d / outer_r).clamp(0.0, 1.0);
            let mut r = lerp(15.0, 30.0, t) as u8;
            let mut g = lerp(20.0, 35.0, t) as u8;
            let mut b = lerp(45.0, 72.0, t) as u8;

            // White play triangle
            if in_triangle(x, y, tx1, ty1, tx2, ty2, tx3, ty3) {
                r = 240;
                g = 240;
                b = 240;
            }

            // White ring around record dot (drawn before the dot so the dot sits on top)
            let dd = dist(x, y, dot_cx, dot_cy);
            if dd >= ring_inner && dd <= ring_outer {
                let ring_t = ((dd - ring_inner) / (ring_outer - ring_inner)).clamp(0.0, 1.0);
                let ring_aa = 1.0 - (ring_t * 2.0 - 1.0).abs();
                r = lerp(r as f32, 255.0, ring_aa * 0.9) as u8;
                g = lerp(g as f32, 255.0, ring_aa * 0.9) as u8;
                b = lerp(b as f32, 255.0, ring_aa * 0.9) as u8;
            }

            // Red record dot
            if dd <= dot_r {
                let dot_aa = ((dot_r - dd) / 1.5_f32).clamp(0.0, 1.0);
                // Slight radial highlight (lighter red in top-left area)
                let hl_dist = dist(x, y, dot_cx - dot_r * 0.28, dot_cy - dot_r * 0.28);
                let hl = (1.0 - (hl_dist / (dot_r * 0.9)).clamp(0.0, 1.0)) * 0.35;
                let dot_r_val = lerp(220.0, 255.0, hl);
                let dot_g_val = lerp(50.0, 100.0, hl);
                let dot_b_val = lerp(50.0, 80.0, hl);
                r = lerp(r as f32, dot_r_val, dot_aa) as u8;
                g = lerp(g as f32, dot_g_val, dot_aa) as u8;
                b = lerp(b as f32, dot_b_val, dot_aa) as u8;
            }

            buf[idx] = r;
            buf[idx + 1] = g;
            buf[idx + 2] = b;
            buf[idx + 3] = (edge_aa * 255.0) as u8;
        }
    }
    buf
}

fn dist(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    (dx * dx + dy * dy).sqrt()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Returns true if point (px, py) is inside the triangle defined by v1, v2, v3.
fn in_triangle(
    px: f32,
    py: f32,
    v1x: f32,
    v1y: f32,
    v2x: f32,
    v2y: f32,
    v3x: f32,
    v3y: f32,
) -> bool {
    let sign = |ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32| -> f32 {
        (ax - cx) * (by - cy) - (bx - cx) * (ay - cy)
    };
    let d1 = sign(px, py, v1x, v1y, v2x, v2y);
    let d2 = sign(px, py, v2x, v2y, v3x, v3y);
    let d3 = sign(px, py, v3x, v3y, v1x, v1y);
    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(has_neg && has_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_icon_rgba_correct_size() {
        let buf = make_icon_rgba(64);
        assert_eq!(buf.len(), 64 * 64 * 4);
    }

    #[test]
    fn test_make_icon_rgba_256() {
        let buf = make_icon_rgba(256);
        assert_eq!(buf.len(), 256 * 256 * 4);
    }

    #[test]
    fn test_make_icon_rgba_not_all_zeros() {
        let buf = make_icon_rgba(32);
        // The icon should have some non-zero pixels
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_corner_pixels_transparent() {
        let size = 64u32;
        let buf = make_icon_rgba(size);
        // Top-left corner (0,0) should be transparent (outside circle)
        let idx = 0usize;
        assert_eq!(buf[idx + 3], 0, "top-left corner alpha should be 0");
    }

    #[test]
    fn test_center_pixel_opaque() {
        let size = 64u32;
        let buf = make_icon_rgba(size);
        // Center pixel should be opaque (inside circle)
        let cx = size / 2;
        let cy = size / 2;
        let idx = ((cy * size + cx) * 4) as usize;
        assert!(buf[idx + 3] > 200, "center pixel alpha should be high");
    }

    #[test]
    fn test_lerp_basic() {
        assert!((lerp(0.0, 10.0, 0.5) - 5.0).abs() < f32::EPSILON);
        assert!((lerp(0.0, 10.0, 0.0) - 0.0).abs() < f32::EPSILON);
        assert!((lerp(0.0, 10.0, 1.0) - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_lerp_clamped() {
        // t > 1.0 should clamp to b
        assert!((lerp(0.0, 10.0, 2.0) - 10.0).abs() < f32::EPSILON);
        // t < 0.0 should clamp to a
        assert!((lerp(0.0, 10.0, -1.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_dist() {
        assert!((dist(0.0, 0.0, 3.0, 4.0) - 5.0).abs() < 0.001);
        assert!((dist(1.0, 1.0, 1.0, 1.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_in_triangle_inside() {
        assert!(in_triangle(1.0, 1.0, 0.0, 0.0, 3.0, 0.0, 0.0, 3.0));
    }

    #[test]
    fn test_in_triangle_outside() {
        assert!(!in_triangle(5.0, 5.0, 0.0, 0.0, 3.0, 0.0, 0.0, 3.0));
    }

    #[test]
    fn test_in_triangle_on_edge() {
        // Point on the edge should be considered inside
        assert!(in_triangle(1.5, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 3.0));
    }
}
