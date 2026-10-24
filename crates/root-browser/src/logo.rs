//! Brand mark and the three search-engine marks drawn for settings.
//!
//! `lib.rs` calls `render_root_logo` and `render_search_engine_logo` while it
//! resolves `root://logo.png` and `root://engine-*.png`. `ui.rs` and `shell.rs`
//! call the Root mark for the toolbar and the window icon. `installer.rs` calls
//! `encode_root_png` and `encode_root_ico`.
//!
//! PNG and ICO bytes go through `root-image`. The rounded mask uses
//! `root_gpu::sdf_rounded_box`. The search-engine marks are drawn here as
//! pixels. They are not loaded from image files. The engines are Google Search,
//! DuckDuckGo, and Microsoft Bing, matching `SearchEngine`.

use std::sync::OnceLock;

use crate::SearchEngine;
use root_gpu::sdf_rounded_box;
use root_image::{Image, decode_png, encode_ico, encode_png};

/// Embedded 128x128 PNG bytes of the Root Browser brand logo (`assets/root-logo.png`).
pub const LOGO_PNG_BYTES: &[u8] = include_bytes!("../../../assets/root-logo.png");

/// Decode `LOGO_PNG_BYTES` once and fade pixels outside a rounded rectangle.
fn master_logo() -> &'static Image {
    static MASTER: OnceLock<Image> = OnceLock::new();
    MASTER.get_or_init(|| {
        let mut img = decode_png(LOGO_PNG_BYTES).expect("valid embedded assets/root-logo.png");
        let w = img.width as f32;
        let h = img.height as f32;
        let hw = w * 0.5;
        let hh = h * 0.5;
        let radius = w.min(h) * 0.48;
        for py in 0..img.height {
            for px in 0..img.width {
                let rx = (px as f32 + 0.5) - hw;
                let ry = (py as f32 + 0.5) - hh;
                let d = sdf_rounded_box(rx, ry, hw - 0.5, hh - 0.5, radius);
                let alpha_mask = (0.5 - d).clamp(0.0, 1.0);
                if let Some(mut c) = img.pixel(px, py) {
                    c[3] = ((c[3] as f32) * alpha_mask).round() as u8;
                    img.set_pixel(px, py, c);
                }
            }
        }
        img
    })
}

/// Render the Root Browser logo as an anti-aliased RGBA [`Image`] of `size x size` pixels.
pub fn render_root_logo(size: u32) -> Image {
    let dim = size.clamp(12, 512);
    master_logo().resize_bilinear(dim, dim)
}

/// Encode the Root Browser logo as a clean, metadata-free PNG image (`64x64`).
pub fn encode_root_png() -> Vec<u8> {
    encode_png(&render_root_logo(64))
}

/// Encode the Root Browser logo as a standard Windows `.ico` file containing a PNG-compressed 64x64 icon.
pub fn encode_root_ico() -> Vec<u8> {
    encode_ico(&render_root_logo(64))
}

/// Render an anti-aliased SDF search engine logo (`Google`, `DuckDuckGo`, or `Bing`) of `size x size` pixels.
pub fn render_search_engine_logo(engine: SearchEngine, size: u32) -> Image {
    let dim = size.clamp(16, 128);
    let mut img = Image::new(dim, dim);
    let w = dim as f32;
    let h = dim as f32;
    let cx = w * 0.5;
    let cy = h * 0.5;
    let outer_r = w * 0.46;

    for py in 0..dim {
        for px in 0..dim {
            let x = (px as f32 + 0.5) - cx;
            let y = (py as f32 + 0.5) - cy;
            let nx = x / outer_r;
            let ny = y / outer_r;
            let dist = (nx * nx + ny * ny).sqrt();
            let badge_alpha = ((1.0 - dist) * outer_r).clamp(0.0, 1.0);
            if badge_alpha <= 0.0 {
                continue;
            }

            let (r, g, b) = match engine {
                SearchEngine::Google => {
                    // White circular badge with 4-color "G" ring and blue crossbar
                    let mut col = (248.0f32, 250.0f32, 252.0f32);
                    let ring_outer = 0.70;
                    let ring_inner = 0.42;
                    let angle_deg = ny.atan2(nx).to_degrees();
                    let in_ring = dist <= ring_outer && dist >= ring_inner;
                    let in_gap = angle_deg > -38.0 && angle_deg < 8.0 && nx > 0.0;
                    let in_bar = ny >= -0.13 && ny <= 0.14 && nx >= 0.02 && nx <= 0.68;

                    if (in_ring && !in_gap) || in_bar {
                        let glyph_col = if in_bar || (angle_deg >= -8.0 && angle_deg <= 48.0) {
                            (66.0, 133.0, 244.0) // Blue
                        } else if angle_deg > 48.0 && angle_deg <= 145.0 {
                            (52.0, 168.0, 83.0) // Green
                        } else if angle_deg > 145.0 || angle_deg < -135.0 {
                            (251.0, 188.0, 5.0) // Yellow
                        } else {
                            (234.0, 67.0, 53.0) // Red
                        };
                        col = glyph_col;
                    }
                    col
                }
                SearchEngine::DuckDuckGo => {
                    // Vibrant orange badge (#DE5833) with white duck silhouette, golden beak, and green bowtie
                    let mut col = (222.0f32, 88.0f32, 51.0f32);
                    let head_d = ((nx + 0.08).powi(2) + (ny + 0.14).powi(2)).sqrt();
                    let neck_d = ((nx + 0.05).powi(2) / 0.10 + (ny - 0.25).powi(2) / 0.18).sqrt();
                    let beak_d = ((nx - 0.26).powi(2) / 0.10 + (ny + 0.10).powi(2) / 0.035).sqrt();
                    let bow_d = ((nx + 0.04).powi(2) / 0.08 + (ny - 0.50).powi(2) / 0.03).sqrt();
                    let eye_d = ((nx + 0.02).powi(2) + (ny + 0.20).powi(2)).sqrt();

                    if head_d < 0.36 || neck_d < 1.0 {
                        col = (252.0, 253.0, 255.0);
                    }
                    if beak_d < 1.0 {
                        col = (247.0, 181.0, 41.0);
                    }
                    if bow_d < 1.0 {
                        col = (76.0, 186.0, 111.0);
                    }
                    if eye_d < 0.07 {
                        col = (26.0, 32.0, 44.0);
                    }
                    col
                }
                SearchEngine::Bing => {
                    // Teal-to-cobalt gradient badge with white geometric "b" ribbon
                    let t = ((nx + ny) * 0.35 + 0.5).clamp(0.0, 1.0);
                    let mut col = (
                        0.0 * (1.0 - t) + 0.0 * t,
                        183.0 * (1.0 - t) + 120.0 * t,
                        195.0 * (1.0 - t) + 212.0 * t,
                    );
                    let stem = (nx >= -0.34 && nx <= -0.10) && (ny >= -0.58 && ny <= 0.48);
                    let loop_d = ((nx - 0.04).powi(2) + (ny - 0.16).powi(2)).sqrt();
                    let in_loop = loop_d <= 0.42 && loop_d >= 0.18 && nx >= -0.22;
                    let wing = ny >= -0.12 && ny <= 0.12 && nx >= -0.12 && nx <= 0.36;
                    if stem || in_loop || wing {
                        col = (250.0, 253.0, 255.0);
                    }
                    col
                }
            };

            img.set_pixel(
                px,
                py,
                [
                    r.round().clamp(0.0, 255.0) as u8,
                    g.round().clamp(0.0, 255.0) as u8,
                    b.round().clamp(0.0, 255.0) as u8,
                    (badge_alpha * 255.0).round().clamp(0.0, 255.0) as u8,
                ],
            );
        }
    }
    img
}

#[cfg(test)]
mod tests {
    //! Checks the masked Root mark, the ICO header, and one opaque center pixel per engine mark.
    use super::*;
    use root_image::decode_ico;

    #[test]
    /// The masked mark, the ICO bytes, and one opaque center pixel per engine mark.
    fn renders_embedded_root_logo_and_encodes_valid_windows_ico() {
        let img = render_root_logo(48);
        assert_eq!(img.width, 48);
        assert_eq!(img.height, 48);
        // Center pixel of the logo should be opaque
        let center = img.pixel(24, 24).unwrap();
        assert_eq!(center[3], 255);
        // Corner pixel outside the squircle should be transparent
        let corner = img.pixel(0, 0).unwrap();
        assert_eq!(corner[3], 0);

        let ico = encode_root_ico();
        assert!(ico.len() > 22);
        assert_eq!(&ico[0..6], &[0, 0, 1, 0, 1, 0]);
        let decoded_ico = decode_ico(&ico).expect("valid ICO decoded by root-image");
        assert_eq!(decoded_ico.width, 64);
        assert_eq!(decoded_ico.height, 64);

        for engine in [
            SearchEngine::Google,
            SearchEngine::DuckDuckGo,
            SearchEngine::Bing,
        ] {
            let badge = render_search_engine_logo(engine, 36);
            assert_eq!(badge.width, 36);
            assert_eq!(badge.height, 36);
            assert_eq!(badge.pixel(18, 18).unwrap()[3], 255);
            assert_eq!(badge.pixel(0, 0).unwrap()[3], 0);
        }
    }
}
