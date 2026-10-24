// CPU scanline helpers for the root-raster framebuffer.
// Author: Buntos
// License: Apache-2.0
//
// Compiled alongside root_raster_compositor.cpp by crates/root-raster/build.rs
// when a C++20 compiler is on PATH, setting cfg(root_cpp_accel).

#include "root_raster_accel.hpp"

using namespace root_raster_cpp;

extern "C" {

// Rust accepts this object file only when the result is 16.
u32 root_cpp_raster_kernel_version() noexcept {
    return 16u;
}

// Top-down RGBA to bottom-up BGRA for a GDI DIB. Row 0 of dst is the last src row.
void root_cpp_rgba_to_bgra_bottom_up(
    const u8* src_rgba,
    u8* dst_bgra,
    u32 width,
    u32 height
) noexcept {
    if (!src_rgba || !dst_bgra || width == 0 || height == 0) {
        return;
    }
    const u32 stride = width * 4u;
    for (u32 row = 0; row < height; ++row) {
        const u32 src_row = (height - 1u - row) * stride;
        const u32 dst_row = row * stride;
        for (u32 col = 0; col < width; ++col) {
            const u32 s = src_row + col * 4u;
            const u32 d = dst_row + col * 4u;
            dst_bgra[d] = src_rgba[s + 2];
            dst_bgra[d + 1] = src_rgba[s + 1];
            dst_bgra[d + 2] = src_rgba[s];
            dst_bgra[d + 3] = src_rgba[s + 3];
        }
    }
}

// Fill [x0, x1) by [y0, y1) in the RGBA buffer.
void root_cpp_fill_rect_rgba(
    u8* pixels,
    u32 fb_width,
    u32 fb_height,
    i32 x0,
    i32 y0,
    i32 x1,
    i32 y1,
    u8 r,
    u8 g,
    u8 b,
    u8 a
) noexcept {
    if (!pixels || a == 0 || fb_width == 0 || fb_height == 0) {
        return;
    }
    const i32 cx0 = x0 < 0 ? 0 : x0;
    const i32 cy0 = y0 < 0 ? 0 : y0;
    const i32 cx1 = x1 > static_cast<i32>(fb_width) ? static_cast<i32>(fb_width) : x1;
    const i32 cy1 = y1 > static_cast<i32>(fb_height) ? static_cast<i32>(fb_height) : y1;
    if (cx0 >= cx1 || cy0 >= cy1) {
        return;
    }
    for (i32 py = cy0; py < cy1; ++py) {
        for (i32 px = cx0; px < cx1; ++px) {
            blend_pixel_rgba(
                pixels,
                fb_width,
                static_cast<u32>(px),
                static_cast<u32>(py),
                r,
                g,
                b,
                a
            );
        }
    }
}

// Fill a rounded rectangle, optionally mixing a border, into the RGBA buffer.
void root_cpp_fill_rounded_rect_sdf(
    u8* pixels,
    u32 fb_width,
    u32 fb_height,
    i32 x0,
    i32 y0,
    i32 x1,
    i32 y1,
    float x,
    float y,
    float width,
    float height,
    float radius,
    float border_width,
    u8 fill_r,
    u8 fill_g,
    u8 fill_b,
    u8 fill_a,
    u8 border_r,
    u8 border_g,
    u8 border_b,
    u8 border_a
) noexcept {
    if (!pixels || width <= 0.0f || height <= 0.0f || fb_width == 0 || fb_height == 0) {
        return;
    }
    const i32 cx0 = x0 < 0 ? 0 : x0;
    const i32 cy0 = y0 < 0 ? 0 : y0;
    const i32 cx1 = x1 > static_cast<i32>(fb_width) ? static_cast<i32>(fb_width) : x1;
    const i32 cy1 = y1 > static_cast<i32>(fb_height) ? static_cast<i32>(fb_height) : y1;
    if (cx0 >= cx1 || cy0 >= cy1) {
        return;
    }
    const float half_w = width * 0.5f;
    const float half_h = height * 0.5f;
    const float center_x = x + half_w;
    const float center_y = y + half_h;

    for (i32 py = cy0; py < cy1; ++py) {
        const float sy = static_cast<float>(py) + 0.5f;
        for (i32 px = cx0; px < cx1; ++px) {
            const float sx = static_cast<float>(px) + 0.5f;
            const float dist = sdf_rounded_box_cpp(sx - center_x, sy - center_y, half_w, half_h, radius);
            const float outer_alpha = f_clamp(0.5f - dist, 0.0f, 1.0f);
            if (outer_alpha <= 0.0f) {
                continue;
            }
            float cr = static_cast<float>(fill_r);
            float cg = static_cast<float>(fill_g);
            float cb = static_cast<float>(fill_b);
            float ca = static_cast<float>(fill_a);

            if (border_width > 0.0f) {
                const float inner_dist = dist + border_width;
                const float border_mix = f_clamp(0.5f + inner_dist, 0.0f, 1.0f);
                const float inv = 1.0f - border_mix;
                cr = cr * inv + static_cast<float>(border_r) * border_mix;
                cg = cg * inv + static_cast<float>(border_g) * border_mix;
                cb = cb * inv + static_cast<float>(border_b) * border_mix;
                ca = ca * inv + static_cast<float>(border_a) * border_mix;
            }
            const u8 final_a = static_cast<u8>(ca * outer_alpha + 0.5f);
            blend_pixel_rgba(
                pixels,
                fb_width,
                static_cast<u32>(px),
                static_cast<u32>(py),
                static_cast<u8>(cr + 0.5f),
                static_cast<u8>(cg + 0.5f),
                static_cast<u8>(cb + 0.5f),
                final_a
            );
        }
    }
}

} // extern "C"
