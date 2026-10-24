//! CPU RGBA framebuffer and signed-distance rasterizer.
//!
//! `anvil-browser` calls [`rasterize`] with a `anvil-paint` display list, then
//! blits the pixels into the window. The buffer is top-down RGBA8 in this
//! process. Nothing here uploads a draw to a GPU.
//!
//! Rounded boxes, box shadows, and linear gradients call the C++20 scanline
//! and compositor helpers when `anvil_cpp_accel` is linked, or fall back to the
//! CPU helpers in `anvil-gpu` (`sdf_rounded_box`, `sdf_box_shadow_alpha`, and
//! `sample_linear_gradient`). Images are `anvil-image` buffers. `draw_image`
//! resizes with that crate's bilinear filter and samples the result. This
//! crate does not call `anvil-video`. A video frame is drawn only after the
//! browser has already copied the surface into an `Image` on the display list.
//!
//! Text on Windows tries Win32 GDI in `font` and, if that returns false, the
//! built-in 5x7 bitmap. Text on Linux loads DejaVu Sans or Liberation Sans
//! through `anvil-ttf` when one of those files parses. Otherwise the same
//! bitmap font is used.

// Windows only. GDI text. Linux does not compile this module.
#[cfg(windows)]
mod font;

#[cfg(not(anvil_cpp_accel))]
use anvil_gpu::{GpuColor, sample_linear_gradient, sdf_box_shadow_alpha, sdf_rounded_box};
use anvil_image::Image;
use anvil_paint::{DisplayItem, DisplayList};
pub use anvil_style::Color;
use anvil_ttf::TtfFont;

// Scanline and compositor entry points from anvil_raster_accel.cpp and
// anvil_raster_compositor.cpp. Present only when build.rs set anvil_cpp_accel.
#[cfg(anvil_cpp_accel)]
unsafe extern "C" {
    // Must return 16 or is_cpp_raster_kernel_active reports the kernel as off.
    fn anvil_cpp_raster_kernel_version() -> u32;
    // Top-down RGBA to bottom-up BGRA. Used by Framebuffer::to_bgra_bottom_up.
    fn anvil_cpp_rgba_to_bgra_bottom_up(
        src_rgba: *const u8,
        dst_bgra: *mut u8,
        width: u32,
        height: u32,
    );
    // Opaque or blended axis-aligned fill. The clip stack is applied in Rust first.
    fn anvil_cpp_fill_rect_rgba(
        pixels: *mut u8,
        fb_width: u32,
        fb_height: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    );
    // Rounded fill plus optional border. Its SDF is local to the C++ file.
    fn anvil_cpp_fill_rounded_rect_sdf(
        pixels: *mut u8,
        fb_width: u32,
        fb_height: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        border_width: f32,
        fill_r: u8,
        fill_g: u8,
        fill_b: u8,
        fill_a: u8,
        border_r: u8,
        border_g: u8,
        border_b: u8,
        border_a: u8,
    );
    // Analytical SDF box-shadow scanline pass.
    fn anvil_cpp_fill_box_shadow_sdf(
        pixels: *mut u8,
        fb_width: u32,
        fb_height: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        cx: f32,
        cy: f32,
        half_w: f32,
        half_h: f32,
        radius: f32,
        blur: f32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    );
    // Rounded-rect two-stop linear gradient scanline pass.
    fn anvil_cpp_fill_linear_gradient_sdf(
        pixels: *mut u8,
        fb_width: u32,
        fb_height: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        border_width: f32,
        angle_deg: f32,
        start_r: u8,
        start_g: u8,
        start_b: u8,
        start_a: u8,
        end_r: u8,
        end_g: u8,
        end_b: u8,
        end_a: u8,
        border_r: u8,
        border_g: u8,
        border_b: u8,
        border_a: u8,
    );
    // 8-bit coverage glyph bitmap blender.
    fn anvil_cpp_blend_glyph_coverage(
        pixels: *mut u8,
        fb_width: u32,
        fb_height: u32,
        clip_x0: i32,
        clip_y0: i32,
        clip_x1: i32,
        clip_y1: i32,
        dst_x: i32,
        dst_y: i32,
        coverage: *const u8,
        glyph_w: u32,
        glyph_h: u32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    );
    // Clipped RGBA image blitter with optional rounded-corner SDF mask.
    fn anvil_cpp_blit_rgba_clipped(
        dst_pixels: *mut u8,
        fb_width: u32,
        fb_height: u32,
        clip_x0: i32,
        clip_y0: i32,
        clip_x1: i32,
        clip_y1: i32,
        base_x: i32,
        base_y: i32,
        src_rgba: *const u8,
        src_w: u32,
        src_h: u32,
        border_radius: f32,
    );
    // Semi-planar NV12 (Y + UV) to RGBA8 BT.709 color conversion.
    fn anvil_cpp_nv12_to_rgba_bt709(
        y_plane: *const u8,
        uv_plane: *const u8,
        dst_rgba: *mut u8,
        width: u32,
        height: u32,
    );
}

/// `true` only when `anvil_raster_accel.cpp` is linked and its version symbol is 16.
/// A build that skipped the compiler returns `false`.
pub fn is_cpp_raster_kernel_active() -> bool {
    #[cfg(anvil_cpp_accel)]
    {
        unsafe { anvil_cpp_raster_kernel_version() == 16 }
    }
    #[cfg(not(anvil_cpp_accel))]
    {
        false
    }
}

/// Convert a semi-planar `NV12` (`Y` plane + interleaved `UV` plane) surface into top-down `RGBA8`
/// using BT.709 coefficients (accelerated via C++20 when `anvil_cpp_accel` is linked).
pub fn convert_nv12_to_rgba_bt709(
    y_plane: &[u8],
    uv_plane: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let uv_stride = ((width as usize) + 1) & !1;
    let uv_required = ((height as usize).div_ceil(2)).saturating_mul(uv_stride);
    if width == 0 || height == 0 || y_plane.len() < pixel_count || uv_plane.len() < uv_required {
        return Vec::new();
    }
    let mut out = vec![0u8; pixel_count * 4];
    #[cfg(anvil_cpp_accel)]
    unsafe {
        anvil_cpp_nv12_to_rgba_bt709(
            y_plane.as_ptr(),
            uv_plane.as_ptr(),
            out.as_mut_ptr(),
            width,
            height,
        );
        return out;
    }
    #[cfg(not(anvil_cpp_accel))]
    {
        for py in 0..(height as usize) {
            let uv_row = (py / 2) * uv_stride;
            for px in 0..(width as usize) {
                let y_val = i32::from(y_plane[py * (width as usize) + px]);
                let uv_idx = uv_row + (px & !1);
                let u_val = i32::from(uv_plane[uv_idx]) - 128;
                let v_val = i32::from(uv_plane[uv_idx + 1]) - 128;
                let c = (y_val - 16).max(0) * 298;
                let r = ((c + 459 * v_val + 128) >> 8).clamp(0, 255) as u8;
                let g = ((c - 55 * u_val - 136 * v_val + 128) >> 8).clamp(0, 255) as u8;
                let b = ((c + 541 * u_val + 128) >> 8).clamp(0, 255) as u8;
                let idx = (py * (width as usize) + px) * 4;
                out[idx] = r;
                out[idx + 1] = g;
                out[idx + 2] = b;
                out[idx + 3] = 255;
            }
        }
        out
    }
}

/// Clear color when the display list does not start with a full-width background rect.
const PAPER: Color = Color {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};

/// Top-down RGBA8 buffer plus a clip stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Framebuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    clips: Vec<(i32, i32, i32, i32)>,
}

impl Framebuffer {
    /// Allocate `width * height` pixels filled with `clear`. The clip stack starts empty.
    pub fn new(width: u32, height: u32, clear: Color) -> Self {
        let count = (width as usize).saturating_mul(height as usize);
        let mut pixels = Vec::with_capacity(count * 4);
        for _ in 0..count {
            pixels.extend_from_slice(&[clear.r, clear.g, clear.b, clear.a]);
        }
        Self {
            width,
            height,
            pixels,
            clips: Vec::new(),
        }
    }

    /// Intersect a new clip with the framebuffer and with the clip already on top.
    pub fn push_clip(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let mut x0 = (x.floor() as i32).max(0);
        let mut y0 = (y.floor() as i32).max(0);
        let mut x1 = ((x + width.max(0.0)).ceil() as i32).min(self.width as i32);
        let mut y1 = ((y + height.max(0.0)).ceil() as i32).min(self.height as i32);
        if let Some(&(px0, py0, px1, py1)) = self.clips.last() {
            x0 = x0.max(px0);
            y0 = y0.max(py0);
            x1 = x1.min(px1);
            y1 = y1.min(py1);
        }
        self.clips.push((x0, y0, x1, y1));
    }

    /// Drop the top clip. A pop on an empty stack does nothing.
    pub fn pop_clip(&mut self) {
        self.clips.pop();
    }

    /// Buffer width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Buffer height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Raw top-down RGBA bytes. Length is `width * height * 4`.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Read one pixel. `None` when `x` or `y` is outside the buffer. Clips are ignored.
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Some(Color {
            r: self.pixels[offset],
            g: self.pixels[offset + 1],
            b: self.pixels[offset + 2],
            a: self.pixels[offset + 3],
        })
    }

    /// Blend `color` at an integer pixel.
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || color.a == 0 {
            return;
        }
        if let Some(&(cx0, cy0, cx1, cy1)) = self.clips.last() {
            if x < cx0 || x >= cx1 || y < cy0 || y >= cy1 {
                return;
            }
        }
        let (x, y) = (x as u32, y as u32);
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        if color.a == 255 {
            self.pixels[offset] = color.r;
            self.pixels[offset + 1] = color.g;
            self.pixels[offset + 2] = color.b;
            self.pixels[offset + 3] = 255;
            return;
        }
        let alpha = color.a as u16;
        let inv = 255 - alpha;
        self.pixels[offset] =
            ((color.r as u16 * alpha + self.pixels[offset] as u16 * inv) / 255) as u8;
        self.pixels[offset + 1] =
            ((color.g as u16 * alpha + self.pixels[offset + 1] as u16 * inv) / 255) as u8;
        self.pixels[offset + 2] =
            ((color.b as u16 * alpha + self.pixels[offset + 2] as u16 * inv) / 255) as u8;
        self.pixels[offset + 3] = 255;
    }

    /// Current clip, or the whole buffer when the clip stack is empty.
    fn active_clip_bounds(&self) -> (i32, i32, i32, i32) {
        self.clips
            .last()
            .copied()
            .unwrap_or((0, 0, self.width as i32, self.height as i32))
    }

    /// Inclusive-exclusive pixel span for a rectangle after buffer and clip limits.
    fn clipped_span(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Option<(i32, i32, i32, i32)> {
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let mut x0 = (x.floor() as i32).max(0);
        let mut y0 = (y.floor() as i32).max(0);
        let mut x1 = ((x + width).ceil() as i32).min(self.width as i32);
        let mut y1 = ((y + height).ceil() as i32).min(self.height as i32);
        if let Some(&(cx0, cy0, cx1, cy1)) = self.clips.last() {
            x0 = x0.max(cx0);
            y0 = y0.max(cy0);
            x1 = x1.min(cx1);
            y1 = y1.min(cy1);
        }
        (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
    }

    /// Fill one clipped span on the CPU. Tests use this to compare with the C++ fill.
    #[cfg(any(test, not(anvil_cpp_accel)))]
    fn fill_span_cpu(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        for py in y0..y1 {
            for px in x0..x1 {
                self.set_pixel(px, py, color);
            }
        }
    }

    /// Fill an axis-aligned rectangle, clipped to the buffer and to the top clip.
    pub fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        if color.a == 0 {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.clipped_span(x, y, width, height) else {
            return;
        };
        #[cfg(anvil_cpp_accel)]
        unsafe {
            anvil_cpp_fill_rect_rgba(
                self.pixels.as_mut_ptr(),
                self.width,
                self.height,
                x0,
                y0,
                x1,
                y1,
                color.r,
                color.g,
                color.b,
                color.a,
            );
            return;
        }
        #[cfg(not(anvil_cpp_accel))]
        self.fill_span_cpu(x0, y0, x1, y1, color);
    }

    /// Same rectangle as [`Self::fill_rect`], always on the CPU loop.
    #[cfg(test)]
    fn fill_rect_cpu(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        if color.a == 0 {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.clipped_span(x, y, width, height) else {
            return;
        };
        self.fill_span_cpu(x0, y0, x1, y1, color);
    }

    /// Fill a rounded rectangle and blend a border when `border_width` is positive.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        border_width: f32,
        fill_color: Color,
        border_color: Color,
    ) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let mut x0 = (x.floor() as i32).max(0);
        let mut y0 = (y.floor() as i32).max(0);
        let mut x1 = ((x + width).ceil() as i32).min(self.width as i32);
        let mut y1 = ((y + height).ceil() as i32).min(self.height as i32);
        if let Some(&(cx0, cy0, cx1, cy1)) = self.clips.last() {
            x0 = x0.max(cx0);
            y0 = y0.max(cy0);
            x1 = x1.min(cx1);
            y1 = y1.min(cy1);
        }
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        #[cfg(anvil_cpp_accel)]
        unsafe {
            anvil_cpp_fill_rounded_rect_sdf(
                self.pixels.as_mut_ptr(),
                self.width,
                self.height,
                x0,
                y0,
                x1,
                y1,
                x,
                y,
                width,
                height,
                radius,
                border_width,
                fill_color.r,
                fill_color.g,
                fill_color.b,
                fill_color.a,
                border_color.r,
                border_color.g,
                border_color.b,
                border_color.a,
            );
            return;
        }
        #[cfg(not(anvil_cpp_accel))]
        {
            let half_w = width * 0.5;
            let half_h = height * 0.5;
            let cx = x + half_w;
            let cy = y + half_h;

            for py in y0..y1 {
                for px in x0..x1 {
                    let sx = px as f32 + 0.5;
                    let sy = py as f32 + 0.5;
                    let dist = sdf_rounded_box(sx - cx, sy - cy, half_w, half_h, radius);
                    let outer_alpha = (0.5 - dist).clamp(0.0, 1.0);
                    if outer_alpha <= 0.0 {
                        continue;
                    }
                    let mut c = fill_color;
                    if border_width > 0.0 {
                        let inner_dist = dist + border_width;
                        let border_mix = (0.5 + inner_dist).clamp(0.0, 1.0);
                        let inv = 1.0 - border_mix;
                        c = Color {
                            r: ((c.r as f32) * inv + (border_color.r as f32) * border_mix).round()
                                as u8,
                            g: ((c.g as f32) * inv + (border_color.g as f32) * border_mix).round()
                                as u8,
                            b: ((c.b as f32) * inv + (border_color.b as f32) * border_mix).round()
                                as u8,
                            a: ((c.a as f32) * inv + (border_color.a as f32) * border_mix).round()
                                as u8,
                        };
                    }
                    c.a = ((c.a as f32) * outer_alpha).round() as u8;
                    self.set_pixel(px, py, c);
                }
            }
        }
    }

    /// Draw a box shadow using the C++20 SDF shadow kernel when linked or `anvil-gpu`'s SDF fallback.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_box_shadow(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        offset_x: f32,
        offset_y: f32,
        blur_radius: f32,
        spread_radius: f32,
        radius: f32,
        color: Color,
    ) {
        if width <= 0.0 || height <= 0.0 || color.a == 0 {
            return;
        }
        let blur = blur_radius.max(0.0);
        let sx_rect = x + offset_x - spread_radius;
        let sy_rect = y + offset_y - spread_radius;
        let sw = (width + spread_radius * 2.0).max(1.0);
        let sh = (height + spread_radius * 2.0).max(1.0);
        let sr = (radius + spread_radius).max(0.0);

        let mut x0 = ((sx_rect - blur).floor() as i32).max(0);
        let mut y0 = ((sy_rect - blur).floor() as i32).max(0);
        let mut x1 = ((sx_rect + sw + blur).ceil() as i32).min(self.width as i32);
        let mut y1 = ((sy_rect + sh + blur).ceil() as i32).min(self.height as i32);
        if let Some(&(cx0, cy0, cx1, cy1)) = self.clips.last() {
            x0 = x0.max(cx0);
            y0 = y0.max(cy0);
            x1 = x1.min(cx1);
            y1 = y1.min(cy1);
        }
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let half_w = sw * 0.5;
        let half_h = sh * 0.5;
        let cx = sx_rect + half_w;
        let cy = sy_rect + half_h;

        #[cfg(anvil_cpp_accel)]
        unsafe {
            anvil_cpp_fill_box_shadow_sdf(
                self.pixels.as_mut_ptr(),
                self.width,
                self.height,
                x0,
                y0,
                x1,
                y1,
                cx,
                cy,
                half_w,
                half_h,
                sr,
                blur,
                color.r,
                color.g,
                color.b,
                color.a,
            );
            return;
        }
        #[cfg(not(anvil_cpp_accel))]
        for py in y0..y1 {
            for px in x0..x1 {
                let sample_x = px as f32 + 0.5;
                let sample_y = py as f32 + 0.5;
                let dist = sdf_rounded_box(sample_x - cx, sample_y - cy, half_w, half_h, sr);
                let alpha = sdf_box_shadow_alpha(dist, blur);
                if alpha <= 0.0 {
                    continue;
                }
                let mut c = color;
                c.a = ((c.a as f32) * alpha).round() as u8;
                self.set_pixel(px, py, c);
            }
        }
    }

    /// Fill a rounded rectangle with a two-stop linear gradient.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_linear_gradient_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        border_width: f32,
        angle_deg: f32,
        start_color: Color,
        end_color: Color,
        border_color: Color,
    ) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let mut x0 = (x.floor() as i32).max(0);
        let mut y0 = (y.floor() as i32).max(0);
        let mut x1 = ((x + width).ceil() as i32).min(self.width as i32);
        let mut y1 = ((y + height).ceil() as i32).min(self.height as i32);
        if let Some(&(cx0, cy0, cx1, cy1)) = self.clips.last() {
            x0 = x0.max(cx0);
            y0 = y0.max(cy0);
            x1 = x1.min(cx1);
            y1 = y1.min(cy1);
        }
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        #[cfg(anvil_cpp_accel)]
        unsafe {
            anvil_cpp_fill_linear_gradient_sdf(
                self.pixels.as_mut_ptr(),
                self.width,
                self.height,
                x0,
                y0,
                x1,
                y1,
                x,
                y,
                width,
                height,
                radius,
                border_width,
                angle_deg,
                start_color.r,
                start_color.g,
                start_color.b,
                start_color.a,
                end_color.r,
                end_color.g,
                end_color.b,
                end_color.a,
                border_color.r,
                border_color.g,
                border_color.b,
                border_color.a,
            );
            return;
        }
        #[cfg(not(anvil_cpp_accel))]
        {
            let half_w = width * 0.5;
            let half_h = height * 0.5;
            let cx = x + half_w;
            let cy = y + half_h;
            let gpu_start =
                GpuColor::rgba(start_color.r, start_color.g, start_color.b, start_color.a);
            let gpu_end = GpuColor::rgba(end_color.r, end_color.g, end_color.b, end_color.a);

            for py in y0..y1 {
                for px in x0..x1 {
                    let sx = px as f32 + 0.5;
                    let sy = py as f32 + 0.5;
                    let rel_x = sx - cx;
                    let rel_y = sy - cy;
                    let dist = sdf_rounded_box(rel_x, rel_y, half_w, half_h, radius);
                    let outer_alpha = (0.5 - dist).clamp(0.0, 1.0);
                    if outer_alpha <= 0.0 {
                        continue;
                    }
                    let grad = sample_linear_gradient(
                        rel_x, rel_y, half_w, half_h, angle_deg, gpu_start, gpu_end,
                    );
                    let mut c = Color {
                        r: grad.r,
                        g: grad.g,
                        b: grad.b,
                        a: grad.a,
                    };
                    if border_width > 0.0 {
                        let inner_dist = dist + border_width;
                        let border_mix = (0.5 + inner_dist).clamp(0.0, 1.0);
                        let inv = 1.0 - border_mix;
                        c = Color {
                            r: ((c.r as f32) * inv + (border_color.r as f32) * border_mix).round()
                                as u8,
                            g: ((c.g as f32) * inv + (border_color.g as f32) * border_mix).round()
                                as u8,
                            b: ((c.b as f32) * inv + (border_color.b as f32) * border_mix).round()
                                as u8,
                            a: ((c.a as f32) * inv + (border_color.a as f32) * border_mix).round()
                                as u8,
                        };
                    }
                    c.a = ((c.a as f32) * outer_alpha).round() as u8;
                    self.set_pixel(px, py, c);
                }
            }
        }
    }

    /// Stamp one `anvil-ttf` glyph and return its advance in pixels.
    pub fn draw_ttf_glyph(
        &mut self,
        x: f32,
        y: f32,
        ch: char,
        font_size: f32,
        color: Color,
        font: &TtfFont,
    ) -> Option<f32> {
        let bmp = font.rasterize_glyph(ch, font_size)?;
        let baseline_y = y + (font.ascender as f32) * font_size / (font.units_per_em.max(1) as f32);
        let gx0 = (x.round() as i32) + bmp.bearing_x;
        let gy0 = (baseline_y.round() as i32) - bmp.bearing_y;

        #[cfg(anvil_cpp_accel)]
        unsafe {
            let (cx0, cy0, cx1, cy1) = self.active_clip_bounds();
            anvil_cpp_blend_glyph_coverage(
                self.pixels.as_mut_ptr(),
                self.width,
                self.height,
                cx0,
                cy0,
                cx1,
                cy1,
                gx0,
                gy0,
                bmp.coverage.as_ptr(),
                bmp.width,
                bmp.height,
                color.r,
                color.g,
                color.b,
                color.a,
            );
            return Some(bmp.advance_px);
        }
        #[cfg(not(anvil_cpp_accel))]
        {
            for py in 0..bmp.height {
                for px in 0..bmp.width {
                    let cov = bmp.coverage[(py * bmp.width + px) as usize];
                    if cov == 0 {
                        continue;
                    }
                    let mut c = color;
                    c.a = ((u16::from(c.a) * u16::from(cov) + 127) / 255) as u8;
                    self.set_pixel(gx0 + px as i32, gy0 + py as i32, c);
                }
            }
            Some(bmp.advance_px)
        }
    }

    /// Sample a `anvil-image` buffer into the rectangle.
    pub fn draw_image(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        border_radius: f32,
        image: &Image,
    ) {
        if width <= 0.0 || height <= 0.0 || image.width == 0 || image.height == 0 {
            return;
        }
        let dst_w = width.round().max(1.0) as u32;
        let dst_h = height.round().max(1.0) as u32;
        let scaled = image.resize_bilinear(dst_w, dst_h);
        let base_x = x.round() as i32;
        let base_y = y.round() as i32;

        #[cfg(anvil_cpp_accel)]
        unsafe {
            let (cx0, cy0, cx1, cy1) = self.active_clip_bounds();
            anvil_cpp_blit_rgba_clipped(
                self.pixels.as_mut_ptr(),
                self.width,
                self.height,
                cx0,
                cy0,
                cx1,
                cy1,
                base_x,
                base_y,
                scaled.pixels.as_ptr(),
                dst_w,
                dst_h,
                border_radius,
            );
            return;
        }
        #[cfg(not(anvil_cpp_accel))]
        {
            let half_w = (dst_w as f32) * 0.5;
            let half_h = (dst_h as f32) * 0.5;

            for iy in 0..dst_h {
                for ix in 0..dst_w {
                    let Some(rgba) = scaled.pixel(ix, iy) else {
                        continue;
                    };
                    let mut alpha = rgba[3] as f32;
                    if border_radius > 0.0 {
                        let dist = sdf_rounded_box(
                            (ix as f32 + 0.5) - half_w,
                            (iy as f32 + 0.5) - half_h,
                            half_w,
                            half_h,
                            border_radius,
                        );
                        let mask = (0.5 - dist).clamp(0.0, 1.0);
                        alpha *= mask;
                    }
                    if alpha <= 0.5 {
                        continue;
                    }
                    self.set_pixel(
                        base_x + ix as i32,
                        base_y + iy as i32,
                        Color {
                            r: rgba[0],
                            g: rgba[1],
                            b: rgba[2],
                            a: alpha.round() as u8,
                        },
                    );
                }
            }
        }
    }

    /// Draw a run of text. Non-positive size or zero alpha returns immediately.
    pub fn draw_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, color: Color) {
        if font_size <= 0.0 || color.a == 0 {
            return;
        }
        #[cfg(windows)]
        if font::draw_system_text(self, x, y, text, font_size, color) {
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some(sys_font) = linux_system_ttf_font() {
            self.draw_ttf_text(x, y, text, font_size, color, sys_font);
            return;
        }
        self.draw_bitmap_text(x, y, text, font_size, color);
    }

    /// Walk `text` with a parsed `anvil-ttf` font.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn draw_ttf_text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font_size: f32,
        color: Color,
        font: &TtfFont,
    ) {
        if font.units_per_em == 0 {
            self.draw_bitmap_text(x, y, text, font_size, color);
            return;
        }
        let scale = font_size / f32::from(font.units_per_em);
        let baseline = y + f32::from(font.ascender) * scale;
        let mut pen = x;
        for ch in text.chars() {
            if let Some(bmp) = font.rasterize_glyph(ch, font_size) {
                let origin_x = pen.round() as i32 + bmp.bearing_x;
                let width = bmp.width as usize;
                for row in 0..bmp.height {
                    let screen_y = baseline.round() as i32 + bmp.bearing_y - row as i32;
                    for col in 0..bmp.width {
                        let cov = bmp.coverage[row as usize * width + col as usize];
                        if cov == 0 {
                            continue;
                        }
                        let a = ((u16::from(color.a) * u16::from(cov)) / 255) as u8;
                        self.set_pixel(
                            origin_x + col as i32,
                            screen_y,
                            Color {
                                r: color.r,
                                g: color.g,
                                b: color.b,
                                a,
                            },
                        );
                    }
                }
            }
            pen += font.glyph_advance_px(ch, font_size);
        }
    }

    /// Draw with the built-in 5x7 bitmap from `glyph`.
    pub fn draw_bitmap_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, color: Color) {
        if font_size <= 0.0 || color.a == 0 {
            return;
        }
        let scale = ((font_size / 12.0).floor() as i32).max(1);
        let advance = font_size * 0.5;
        let glyph_height = 7 * scale;
        let line_height = (font_size * 1.25).round() as i32;
        let pad_y = ((line_height - glyph_height) / 2).max(0);
        let base_y = y.round() as i32 + pad_y;

        for (index, ch) in text.chars().enumerate() {
            let char_x = (x + index as f32 * advance).round() as i32;
            let rows = glyph(ch);
            for (row_idx, row_bits) in rows.iter().copied().enumerate() {
                for col_idx in 0..5 {
                    if (row_bits >> (4 - col_idx)) & 1 == 1 {
                        let gx = char_x + col_idx * scale;
                        let gy = base_y + (row_idx as i32) * scale;
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.set_pixel(gx + sx, gy + sy, color);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Convert top-down RGBA scanlines into bottom-up BGRA scanlines for GDI.
    pub fn to_bgra_bottom_up(&self) -> Vec<u8> {
        let mut out = vec![0_u8; self.pixels.len()];
        #[cfg(anvil_cpp_accel)]
        unsafe {
            anvil_cpp_rgba_to_bgra_bottom_up(
                self.pixels.as_ptr(),
                out.as_mut_ptr(),
                self.width,
                self.height,
            );
            return out;
        }
        #[cfg(not(anvil_cpp_accel))]
        {
            let stride = (self.width as usize) * 4;
            for row in 0..(self.height as usize) {
                let src_row = (self.height as usize - 1 - row) * stride;
                let dst_row = row * stride;
                for col in 0..(self.width as usize) {
                    let s = src_row + col * 4;
                    let d = dst_row + col * 4;
                    out[d] = self.pixels[s + 2];
                    out[d + 1] = self.pixels[s + 1];
                    out[d + 2] = self.pixels[s];
                    out[d + 3] = self.pixels[s + 3];
                }
            }
            out
        }
    }

    /// Convert top-down RGBA scanlines into top-down BGRA scanlines for X11 (`ZPixmap` 32-bit).
    pub fn to_bgra_top_down(&self) -> Vec<u8> {
        let mut out = vec![0_u8; self.pixels.len()];
        for (dst, src) in out.chunks_exact_mut(4).zip(self.pixels.chunks_exact(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        out
    }

    /// Encode the framebuffer as a binary PPM (`P6`) image.
    pub fn to_ppm(&self) -> Vec<u8> {
        let header = format!("P6\n{} {}\n255\n", self.width, self.height);
        let mut out =
            Vec::with_capacity(header.len() + (self.width as usize) * (self.height as usize) * 3);
        out.extend_from_slice(header.as_bytes());
        for chunk in self.pixels.chunks_exact(4) {
            out.push(chunk[0]);
            out.push(chunk[1]);
            out.push(chunk[2]);
        }
        out
    }
}

/// Paint `list` into a new framebuffer.
pub fn rasterize(list: &DisplayList, width: u32, height: u32, scroll_y: f32) -> Framebuffer {
    let canvas_bg = match list.items.first() {
        Some(DisplayItem::Rect {
            x,
            y,
            width: rw,
            height: rh,
            color,
        }) if *x <= 0.0 && *y <= 0.0 && *rw >= (width as f32) * 0.9 && *rh >= 100.0 => *color,
        _ => PAPER,
    };
    let mut fb = Framebuffer::new(width, height, canvas_bg);
    let mut fixed_depth = 0usize;
    for item in &list.items {
        let dy = if fixed_depth > 0 { 0.0 } else { scroll_y };
        match item {
            DisplayItem::PushClip {
                x,
                y,
                width,
                height,
            } => {
                fb.push_clip(*x, *y - dy, *width, *height);
            }
            DisplayItem::PopClip => {
                fb.pop_clip();
            }
            DisplayItem::PushFixed => {
                fixed_depth += 1;
            }
            DisplayItem::PopFixed => {
                fixed_depth = fixed_depth.saturating_sub(1);
            }
            DisplayItem::BoxShadow {
                x,
                y,
                width,
                height,
                offset_x,
                offset_y,
                blur_radius,
                spread_radius,
                radius,
                color,
            } => {
                fb.fill_box_shadow(
                    *x,
                    *y - dy,
                    *width,
                    *height,
                    *offset_x,
                    *offset_y,
                    *blur_radius,
                    *spread_radius,
                    *radius,
                    *color,
                );
            }
            DisplayItem::LinearGradientRect {
                x,
                y,
                width,
                height,
                radius,
                border_width,
                angle_deg,
                start_color,
                end_color,
                border_color,
            } => {
                fb.fill_linear_gradient_rect(
                    *x,
                    *y - dy,
                    *width,
                    *height,
                    *radius,
                    *border_width,
                    *angle_deg,
                    *start_color,
                    *end_color,
                    *border_color,
                );
            }
            DisplayItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                fb.fill_rect(*x, *y - dy, *width, *height, *color);
            }
            DisplayItem::RoundedRect {
                x,
                y,
                width,
                height,
                radius,
                border_width,
                fill_color,
                border_color,
            } => {
                fb.fill_rounded_rect(
                    *x,
                    *y - dy,
                    *width,
                    *height,
                    *radius,
                    *border_width,
                    *fill_color,
                    *border_color,
                );
            }
            DisplayItem::Image {
                x,
                y,
                width,
                height,
                border_radius,
                image,
            } => {
                fb.draw_image(*x, *y - dy, *width, *height, *border_radius, image);
            }
            DisplayItem::Text {
                x,
                y,
                text,
                color,
                font_size,
            } => {
                fb.draw_text(*x, *y - dy, text, *font_size, *color);
            }
        }
    }
    fb
}

#[cfg(target_os = "linux")]
fn linux_system_ttf_font() -> Option<&'static TtfFont> {
    use std::sync::OnceLock;
    static SYSTEM_FONT: OnceLock<Option<TtfFont>> = OnceLock::new();
    SYSTEM_FONT
        .get_or_init(|| {
            const CANDIDATES: &[&str] = &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ];
            for path in CANDIDATES {
                if let Ok(bytes) = std::fs::read(path) {
                    if let Ok(font) = TtfFont::parse(&bytes) {
                        return Some(font);
                    }
                }
            }
            None
        })
        .as_ref()
}

fn glyph(ch: char) -> [u8; 7] {
    match ch {
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '"' => [0x0a, 0x0a, 0x0a, 0x00, 0x00, 0x00, 0x00],
        '#' => [0x0a, 0x1f, 0x0a, 0x0a, 0x1f, 0x0a, 0x00],
        '$' => [0x04, 0x0f, 0x14, 0x0e, 0x05, 0x1e, 0x04],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        '&' => [0x0c, 0x12, 0x14, 0x08, 0x15, 0x12, 0x0d],
        '\'' => [0x04, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '*' => [0x00, 0x04, 0x15, 0x0e, 0x15, 0x04, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1f, 0x04, 0x04, 0x00],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x08],
        '-' => [0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x04],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x06, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        ':' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00],
        ';' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x08],
        '<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        '=' => [0x00, 0x00, 0x1f, 0x00, 0x1f, 0x00, 0x00],
        '>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        '?' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        '@' => [0x0e, 0x11, 0x17, 0x15, 0x17, 0x10, 0x0e],
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        'D' => [0x1c, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1c],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        '[' => [0x0e, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0e],
        '\\' => [0x10, 0x08, 0x08, 0x04, 0x02, 0x02, 0x01],
        ']' => [0x0e, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0e],
        '^' => [0x04, 0x0a, 0x11, 0x00, 0x00, 0x00, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f],
        '`' => [0x08, 0x04, 0x02, 0x00, 0x00, 0x00, 0x00],
        'a' => [0x00, 0x00, 0x0e, 0x01, 0x0f, 0x11, 0x0f],
        'b' => [0x10, 0x10, 0x1e, 0x11, 0x11, 0x11, 0x1e],
        'c' => [0x00, 0x00, 0x0e, 0x10, 0x10, 0x11, 0x0e],
        'd' => [0x01, 0x01, 0x0f, 0x11, 0x11, 0x11, 0x0f],
        'e' => [0x00, 0x00, 0x0e, 0x11, 0x1f, 0x10, 0x0e],
        'f' => [0x06, 0x08, 0x1e, 0x08, 0x08, 0x08, 0x08],
        'g' => [0x00, 0x0f, 0x11, 0x11, 0x0f, 0x01, 0x0e],
        'h' => [0x10, 0x10, 0x1e, 0x11, 0x11, 0x11, 0x11],
        'i' => [0x04, 0x00, 0x0c, 0x04, 0x04, 0x04, 0x0e],
        'j' => [0x02, 0x00, 0x06, 0x02, 0x02, 0x12, 0x0c],
        'k' => [0x10, 0x10, 0x12, 0x14, 0x18, 0x14, 0x12],
        'l' => [0x0c, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'm' => [0x00, 0x00, 0x1a, 0x15, 0x15, 0x11, 0x11],
        'n' => [0x00, 0x00, 0x1e, 0x11, 0x11, 0x11, 0x11],
        'o' => [0x00, 0x00, 0x0e, 0x11, 0x11, 0x11, 0x0e],
        'p' => [0x00, 0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10],
        'q' => [0x00, 0x0f, 0x11, 0x11, 0x0f, 0x01, 0x01],
        'r' => [0x00, 0x00, 0x16, 0x19, 0x10, 0x10, 0x10],
        's' => [0x00, 0x00, 0x0f, 0x10, 0x0e, 0x01, 0x1e],
        't' => [0x08, 0x08, 0x1e, 0x08, 0x08, 0x09, 0x06],
        'u' => [0x00, 0x00, 0x11, 0x11, 0x11, 0x13, 0x0d],
        'v' => [0x00, 0x00, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'w' => [0x00, 0x00, 0x11, 0x11, 0x15, 0x15, 0x0a],
        'x' => [0x00, 0x00, 0x11, 0x0a, 0x04, 0x0a, 0x11],
        'y' => [0x00, 0x11, 0x11, 0x11, 0x0f, 0x01, 0x0e],
        'z' => [0x00, 0x00, 0x1f, 0x02, 0x04, 0x08, 0x1f],
        '{' => [0x02, 0x04, 0x04, 0x08, 0x04, 0x04, 0x02],
        '|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        '}' => [0x08, 0x04, 0x04, 0x02, 0x04, 0x04, 0x08],
        '~' => [0x00, 0x00, 0x08, 0x15, 0x02, 0x00, 0x00],
        _ => [0x1f, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1f],
    }
}

/// Fill, scroll, clip, fixed layers, shadows, gradients, blits, and NV12 conversion tests.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_a_rectangle_and_leaves_outside_pixels_alone() {
        let red = Color {
            r: 220,
            g: 30,
            b: 40,
            a: 255,
        };
        let list = DisplayList {
            items: vec![DisplayItem::Rect {
                x: 2.0,
                y: 3.0,
                width: 4.0,
                height: 5.0,
                color: red,
            }],
        };
        let fb = rasterize(&list, 16, 16, 0.0);
        assert_eq!(fb.pixel(2, 3), Some(red));
        assert_eq!(fb.pixel(5, 7), Some(red));
        assert_eq!(fb.pixel(1, 3), Some(PAPER));
        assert_eq!(fb.pixel(6, 7), Some(PAPER));
    }

    #[test]
    fn draws_glyph_pixels_and_respects_vertical_scroll() {
        let ink = Color {
            r: 10,
            g: 20,
            b: 30,
            a: 255,
        };
        let list = DisplayList {
            items: vec![DisplayItem::Text {
                x: 4.0,
                y: 20.0,
                text: "Anvil".to_string(),
                color: ink,
                font_size: 16.0,
            }],
        };
        let scrolled = rasterize(&list, 64, 32, 16.0);
        let lit = (0..32)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .any(|(x, y)| scrolled.pixel(x, y) == Some(ink));
        assert!(lit);

        let bgra = scrolled.to_bgra_bottom_up();
        assert_eq!(bgra.len(), 64 * 32 * 4);
    }

    #[test]
    fn clips_overflow_and_keeps_fixed_items_pinned_on_scroll() {
        let blue = Color {
            r: 20,
            g: 100,
            b: 220,
            a: 255,
        };
        let gold = Color {
            r: 232,
            g: 195,
            b: 122,
            a: 255,
        };
        let list = DisplayList {
            items: vec![
                DisplayItem::PushClip {
                    x: 0.0,
                    y: 0.0,
                    width: 4.0,
                    height: 4.0,
                },
                DisplayItem::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                    color: blue,
                },
                DisplayItem::PopClip,
                DisplayItem::PushFixed,
                DisplayItem::Rect {
                    x: 8.0,
                    y: 2.0,
                    width: 4.0,
                    height: 4.0,
                    color: gold,
                },
                DisplayItem::PopFixed,
            ],
        };
        let fb = rasterize(&list, 16, 16, 50.0);
        assert_eq!(fb.pixel(9, 3), Some(gold));
        let fb_no_scroll = rasterize(&list, 16, 16, 0.0);
        assert_eq!(fb_no_scroll.pixel(2, 2), Some(blue));
        assert_eq!(fb_no_scroll.pixel(6, 6), Some(PAPER));
    }

    #[test]
    fn renders_box_shadows_and_linear_gradients() {
        let list = DisplayList {
            items: vec![
                DisplayItem::BoxShadow {
                    x: 4.0,
                    y: 4.0,
                    width: 8.0,
                    height: 8.0,
                    offset_x: 0.0,
                    offset_y: 2.0,
                    blur_radius: 4.0,
                    spread_radius: 0.0,
                    radius: 2.0,
                    color: Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 180,
                    },
                },
                DisplayItem::LinearGradientRect {
                    x: 0.0,
                    y: 0.0,
                    width: 16.0,
                    height: 8.0,
                    radius: 0.0,
                    border_width: 0.0,
                    angle_deg: 90.0,
                    start_color: Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    end_color: Color {
                        r: 0,
                        g: 0,
                        b: 255,
                        a: 255,
                    },
                    border_color: Color::BLACK,
                },
            ],
        };
        let fb = rasterize(&list, 20, 20, 0.0);
        let left = fb.pixel(1, 4).unwrap();
        let right = fb.pixel(14, 4).unwrap();
        assert!(left.r > right.r);
        assert!(right.b > left.b);
        let shadow_px = fb.pixel(8, 13).unwrap();
        assert!(shadow_px.r < 255);
    }

    #[test]
    fn converts_nv12_bt709_and_blits_clipped_images() {
        let y_plane = [200u8; 16];
        let uv_plane = [128u8; 8];
        let rgba = convert_nv12_to_rgba_bt709(&y_plane, &uv_plane, 4, 4);
        assert_eq!(rgba.len(), 4 * 4 * 4);
        assert!(rgba[0] > 180 && rgba[3] == 255);

        let img = Image::from_rgba(4, 4, rgba).unwrap();
        let mut fb = Framebuffer::new(8, 8, Color::BLACK);
        fb.push_clip(2.0, 2.0, 2.0, 2.0);
        fb.draw_image(0.0, 0.0, 4.0, 4.0, 0.0, &img);
        fb.pop_clip();
        assert_eq!(fb.pixel(1, 1), Some(Color::BLACK));
        assert!(fb.pixel(2, 2).unwrap().r > 180);
    }

    #[test]
    fn length_and_flex_rectangles_match_on_the_cpu_and_cpp_fills() {
        use anvil_dom::parse_html;
        use anvil_layout::layout_document;
        use anvil_paint::paint;
        use anvil_style::{compute_styles, parse_css};

        let html = "<style>
                html, body, div { margin: 0; padding: 0; }
                html { font-size: 20px; }
                #host { width: 200px; height: 100px; background: #111111; }
                #pct { width: 50%; height: 40%; background: #ff0000; }
                #em { font-size: 1.5rem; width: 2em; height: 10px; background: #00ff00; }
                #row { display: flex; flex-wrap: wrap; width: 100px; background: #0000ff; }
                #row div { width: 40px; height: 10px; }
                #a { background: #aa0000; }
                #b { background: #00aa00; }
                #c { background: #0000aa; }
                #grow { display: flex; width: 100px; height: 10px; }
                #g1 { width: 20px; height: 10px; flex-grow: 1; background: #abcdef; }
                #g2 { width: 20px; height: 10px; flex-grow: 2; background: #fedcba; }
             </style>
             <div id=\"host\"><div id=\"pct\"></div><div id=\"em\"></div></div>
             <div id=\"row\"><div id=\"a\"></div><div id=\"b\"></div><div id=\"c\"></div></div>
             <div id=\"grow\"><div id=\"g1\"></div><div id=\"g2\"></div></div>";
        let doc = parse_html(html);
        let styles = compute_styles(&doc, &parse_css(&doc.embedded_css()));
        let tree = layout_document(&doc, &styles, 200.0);
        let list = paint(&tree);
        let painted = rasterize(&list, 200, 160, 0.0);
        let mut cpu = Framebuffer::new(200, 160, PAPER);
        for item in &list.items {
            if let DisplayItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } = item
            {
                cpu.fill_rect_cpu(*x, *y, *width, *height, *color);
            }
        }

        fn find<'a>(
            doc: &anvil_dom::Document,
            tree: &'a anvil_layout::LayoutBox,
            id: &str,
        ) -> &'a anvil_layout::LayoutBox {
            if doc.get(tree.node).data().attr("id") == Some(id) {
                return tree;
            }
            for child in &tree.children {
                if doc.get(child.node).data().attr("id") == Some(id) {
                    return child;
                }
                let nested = find(doc, child, id);
                if doc.get(nested.node).data().attr("id") == Some(id) {
                    return nested;
                }
            }
            tree
        }

        let expect = [
            ("pct", Color::parse("#ff0000").unwrap()),
            ("em", Color::parse("#00ff00").unwrap()),
            ("a", Color::parse("#aa0000").unwrap()),
            ("b", Color::parse("#00aa00").unwrap()),
            ("c", Color::parse("#0000aa").unwrap()),
            ("g1", Color::parse("#abcdef").unwrap()),
            ("g2", Color::parse("#fedcba").unwrap()),
        ];
        for (id, color) in expect {
            let rect = find(&doc, &tree, id).rect;
            let x = (rect.x + rect.width * 0.5).floor() as u32;
            let y = (rect.y + rect.height * 0.5).floor() as u32;
            assert_eq!(painted.pixel(x, y), Some(color), "{id}");
            assert_eq!(cpu.pixel(x, y), painted.pixel(x, y), "{id}");
        }
    }
}
