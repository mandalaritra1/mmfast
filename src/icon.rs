//! The app icon and the menu-bar glyph, drawn in code (no image assets):
//! a speech bubble with three dots, on a blue tile for the app icon.

/// Coverage of the bubble shape at a point in the unit square (0 or 1).
fn bubble(x: f32, y: f32, inset: f32) -> bool {
    let (l, t, r, b) = (0.12 + inset, 0.16 + inset, 0.88 - inset, 0.70 - inset * 0.6);
    let rad = 0.15 * (1.0 - inset);
    let in_rect = {
        let cx = x.clamp(l + rad, r - rad);
        let cy = y.clamp(t + rad, b - rad);
        x >= l && x <= r && y >= t && y <= b && (x - cx).powi(2) + (y - cy).powi(2) <= rad * rad
    };
    // The tail, pointing down-left.
    let (ax, ay, bx, by, cx2, cy2) = (0.28 + inset * 0.5, b - 0.04, 0.27 + inset * 0.5, 0.88 - inset * 0.8, 0.47, b - 0.02);
    let sign = |px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32| (px - x2) * (y1 - y2) - (x1 - x2) * (py - y2);
    let d1 = sign(x, y, ax, ay, bx, by);
    let d2 = sign(x, y, bx, by, cx2, cy2);
    let d3 = sign(x, y, cx2, cy2, ax, ay);
    let in_tail = !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0));
    in_rect || in_tail
}

fn dot(x: f32, y: f32, inset: f32) -> bool {
    let cy = 0.43 - inset * 0.2;
    let r = 0.055 * (1.0 - inset);
    [0.33, 0.5, 0.67].iter().any(|cx| (x - cx).powi(2) + (y - cy).powi(2) <= r * r)
}

fn render(size: usize, pixel: impl Fn(f32, f32) -> [f32; 4]) -> Vec<u8> {
    const SS: usize = 4;
    let mut out = vec![0u8; size * size * 4];
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let c = pixel(x, y);
                    // Premultiply while averaging, so edges blend cleanly.
                    for i in 0..3 {
                        acc[i] += c[i] * c[3];
                    }
                    acc[3] += c[3];
                }
            }
            let n = (SS * SS) as f32;
            let a = acc[3] / n;
            let o = (py * size + px) * 4;
            for i in 0..3 {
                out[o + i] = if a > 0.0 { (acc[i] / n / a * 255.0) as u8 } else { 0 };
            }
            out[o + 3] = (a * 255.0) as u8;
        }
    }
    out
}

/// Black on transparent, for the macOS menu bar to recolour.
pub fn template_rgba(size: usize) -> Vec<u8> {
    render(size, |x, y| if bubble(x, y, 0.0) && !dot(x, y, 0.0) { [0.0, 0.0, 0.0, 1.0] } else { [0.0; 4] })
}

/// The app icon: a white bubble on a blue rounded tile.
pub fn app_rgba(size: usize) -> Vec<u8> {
    render(size, |x, y| {
        let m = 0.06;
        let rad = 0.2;
        let cx = x.clamp(m + rad, 1.0 - m - rad);
        let cy = y.clamp(m + rad, 1.0 - m - rad);
        let in_tile = x >= m && x <= 1.0 - m && y >= m && y <= 1.0 - m && (x - cx).powi(2) + (y - cy).powi(2) <= rad * rad;
        if !in_tile {
            return [0.0; 4];
        }
        // Scale the bubble into the tile.
        let (bx, by) = ((x - 0.5) / 0.78 + 0.5, (y - 0.5) / 0.78 + 0.53);
        if bubble(bx, by, 0.0) && !dot(bx, by, 0.0) {
            return [1.0, 1.0, 1.0, 1.0];
        }
        let t = y;
        [(0.18 + 0.1 * (1.0 - t)), (0.40 + 0.12 * (1.0 - t)), (0.86 + 0.06 * (1.0 - t)), 1.0]
    })
}

/// PNGs for an `.iconset` (the bundle script turns them into an `.icns`).
pub fn export(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (size, name) in [
        (16, "icon_16x16"),
        (32, "icon_16x16@2x"),
        (32, "icon_32x32"),
        (64, "icon_32x32@2x"),
        (128, "icon_128x128"),
        (256, "icon_128x128@2x"),
        (256, "icon_256x256"),
        (512, "icon_256x256@2x"),
        (512, "icon_512x512"),
        (1024, "icon_512x512@2x"),
    ] {
        let img = image::RgbaImage::from_raw(size as u32, size as u32, app_rgba(size)).expect("icon size");
        img.save(dir.join(format!("{name}.png"))).map_err(std::io::Error::other)?;
    }
    Ok(())
}
