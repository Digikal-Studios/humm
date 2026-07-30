//! Generates the humm app icon (Task 10 brief, spec §1.3): an original
//! mark — a rounded-square background plus a symmetric row of waveform
//! bars, flat colors only, no text — rendered at every size `iconutil`
//! needs for an `.icns`. **Not** Buzz's icon (a red rounded square with
//! concentric white/red rings around a glossy sphere, `buzz-icon-1024.png`)
//! or any derivative of it: different palette, different motif, no
//! gradients.
//!
//! Deliberately deterministic: every color and every coordinate below is a
//! fixed constant or a pure function of `size`, with no randomness
//! anywhere, so re-running [`generate_iconset`] byte-for-byte reproduces
//! the committed `assets/icon/humm.icns` (the brief's reproducibility
//! requirement — the generator is committed alongside the output, and
//! `bundle` never regenerates it).
//!
//! This module only writes PNGs to a directory in the standard macOS
//! iconset layout (`icon_<size>x<size>[@2x].png`); running `iconutil -c
//! icns` over that directory is `main.rs`'s job (an Apple-CLI subprocess
//! call, kept out of this pure-Rust, unit-testable module and out of the
//! test suite entirely — Task 10 environment note: unit tests never invoke
//! Apple CLIs).

use std::io;
use std::path::Path;

/// `(filename, pixel size)` for the standard macOS iconset: the six base
/// sizes the brief names (16/32/64/128/256/512) plus their `@2x` Retina
/// counterparts — `iconutil`'s expected naming convention
/// (`icon_<box>x<box>[@2x].png`, where an `@2x` file is rendered at twice
/// the box size).
const ICONSET_ENTRIES: &[(&str, u32)] = &[
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

/// Flat background color (deep teal) — deliberately not Buzz's red.
const BACKGROUND: [u8; 3] = [0x1b, 0x3a, 0x4b];
/// Flat waveform-bar color (warm amber).
const BAR_COLOR: [u8; 3] = [0xf2, 0xc9, 0x4c];

/// Bar heights as a fraction of the usable interior height, one per bar,
/// left to right — a fixed, symmetric waveform silhouette (not sampled
/// from real audio; this is a mark, not a data visualization).
const BAR_HEIGHT_FRACTIONS: [f64; 7] = [0.30, 0.55, 0.85, 1.00, 0.85, 0.55, 0.30];

/// Renders the full iconset (every entry in [`ICONSET_ENTRIES`]) as PNGs
/// into `dir`, creating it if needed.
pub fn generate_iconset(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, size) in ICONSET_ENTRIES {
        let rgba = render_mark(*size);
        write_png(&dir.join(name), *size, &rgba)?;
    }
    Ok(())
}

/// Renders the mark at `size` × `size` as tightly packed RGBA8 (row-major,
/// no row padding — exactly what [`write_png`] expects).
fn render_mark(size: u32) -> Vec<u8> {
    let n = size as i64;
    // Corner radius proportional to size, matching macOS's own "squircle"
    // icon convention loosely (not exact — this is an original mark, not a
    // template trace).
    let radius = (n / 5).max(1);

    let margin = (n / 6).max(1);
    let usable = (n - margin * 2).max(1);
    let bar_count = BAR_HEIGHT_FRACTIONS.len() as i64;
    // Bars and gaps alternate, one more bar than gap: `2*bar_count - 1`
    // equal units span the usable width.
    let unit = (usable / (bar_count * 2 - 1)).max(1);

    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];
    for y in 0..n {
        for x in 0..n {
            let idx = ((y * n + x) * 4) as usize;
            if !inside_rounded_square(x, y, n, radius) {
                continue; // stays fully transparent
            }

            let mut rgb = BACKGROUND;
            for (i, fraction) in BAR_HEIGHT_FRACTIONS.iter().enumerate() {
                let bar_x0 = margin + (i as i64) * unit * 2;
                let bar_x1 = bar_x0 + unit;
                if x < bar_x0 || x >= bar_x1 {
                    continue;
                }
                let bar_h = ((usable as f64) * fraction).round() as i64;
                let bar_y0 = margin + (usable - bar_h) / 2;
                let bar_y1 = bar_y0 + bar_h;
                if y >= bar_y0 && y < bar_y1 {
                    rgb = BAR_COLOR;
                }
                break;
            }

            pixels[idx] = rgb[0];
            pixels[idx + 1] = rgb[1];
            pixels[idx + 2] = rgb[2];
            pixels[idx + 3] = 0xff;
        }
    }
    pixels
}

/// Whether `(x, y)` (0-based, `y` down) falls inside an `n`×`n` square with
/// corners rounded to radius `r`: clamp the point into the inset rectangle
/// (shrunk by `r` on every side) and test the distance to that clamped
/// point against `r`. Points already inside the inset rectangle clamp to
/// themselves (distance 0, always inside); points in a flat edge region
/// clamp along one axis only (trivially inside); only the four corner
/// regions get a real circular cut.
fn inside_rounded_square(x: i64, y: i64, n: i64, r: i64) -> bool {
    if x < 0 || y < 0 || x >= n || y >= n {
        return false;
    }
    let cx = x.clamp(r, n - 1 - r);
    let cy = y.clamp(r, n - 1 - r);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

fn write_png(path: &Path, size: u32, rgba: &[u8]) -> io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(io::BufWriter::new(file), size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| io::Error::other(e.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Pure-Rust determinism check (no `iconutil`, no filesystem): two
    /// independent renders at the same size must be byte-for-byte
    /// identical, and different pixels must actually appear (background vs
    /// bar), proving the mark isn't accidentally a single flat color.
    #[test]
    fn render_mark_is_deterministic_and_has_both_colors() {
        let first = render_mark(128);
        let second = render_mark(128);
        assert_eq!(first, second);

        let mut colors = std::collections::HashSet::new();
        for px in first.chunks_exact(4) {
            if px[3] != 0 {
                colors.insert([px[0], px[1], px[2]]);
            }
        }
        assert!(
            colors.contains(&BACKGROUND),
            "expected the background color to appear"
        );
        assert!(
            colors.contains(&BAR_COLOR),
            "expected the bar color to appear"
        );
    }
}
