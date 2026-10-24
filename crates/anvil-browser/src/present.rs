//! Windows present of the CPU RGBA framebuffer.
//!
//! The page is rasterized by `anvil-raster` before this module runs. Direct3D 11
//! uploads those pixels and draws them with the compiled `compositor.hlsl`
//! atlas path. The SDF loops in `anvil-raster` are not replaced. Video frames
//! stay on the CPU. If the device or the shader compile fails, the caller
//! copies the same buffer with GDI. Linux does not use this module.

#![cfg(windows)]

use std::ffi::c_void;
use std::ptr::null_mut;

use anvil_gpu::COMPOSITOR_HLSL;

const D3D11_SDK_VERSION: u32 = 7;
const DRIVER_HARDWARE: u32 = 1;
const DRIVER_WARP: u32 = 5;
const FORMAT_RGBA: u32 = 28;
const FORMAT_RG_F32: u32 = 16;
const FORMAT_RGBA_F32: u32 = 2;
const USAGE_DEFAULT: u32 = 0;
const USAGE_STAGING: u32 = 3;
const BIND_VERTEX: u32 = 1;
const BIND_CONSTANT: u32 = 4;
const BIND_SHADER: u32 = 8;
const BIND_RENDER_TARGET: u32 = 32;
const CPU_READ: u32 = 0x20000;
const TOPOLOGY_TRIANGLE: u32 = 4;
const MAP_READ: u32 = 1;

/// How the window copies the CPU buffer onto the client area.
pub enum PresentPath {
    /// The first paint has not tried Direct3D yet.
    Untried,
    /// Direct3D 11 is presenting the CPU buffer.
    Gpu(WindowPresenter),
    /// Direct3D was unavailable. The caller uses GDI.
    Gdi,
}

impl PresentPath {
    /// Upload `rgba` and present it. False means the caller should use GDI.
    ///
    /// A failed first attempt stays on GDI for later paints.
    pub fn paint(&mut self, hwnd: *mut c_void, rgba: &[u8], width: u32, height: u32) -> bool {
        if width == 0 || height == 0 || rgba.len() < (width as usize) * (height as usize) * 4 {
            return false;
        }
        if matches!(self, Self::Untried) {
            *self = match WindowPresenter::open(hwnd, width, height) {
                Ok(presenter) => Self::Gpu(presenter),
                Err(_) => Self::Gdi,
            };
        }
        let Self::Gpu(presenter) = self else {
            return false;
        };
        if presenter.present(rgba, width, height).is_ok() {
            true
        } else {
            *self = Self::Gdi;
            false
        }
    }
}

/// Swap chain and the compiled compositor shaders for one window.
pub struct WindowPresenter {
    device: *mut c_void,
    context: *mut c_void,
    swap: *mut c_void,
    rtv: *mut c_void,
    pipe: GpuPipe,
    atlas: *mut c_void,
    atlas_srv: *mut c_void,
    width: u32,
    height: u32,
}

impl Drop for WindowPresenter {
    fn drop(&mut self) {
        unsafe {
            com_release(self.atlas_srv);
            com_release(self.atlas);
            com_release(self.rtv);
            self.pipe.release();
            com_release(self.swap);
            com_release(self.context);
            com_release(self.device);
        }
    }
}

impl WindowPresenter {
    /// Create a device, compile `compositor.hlsl`, and bind a swap chain to `hwnd`.
    fn open(hwnd: *mut c_void, width: u32, height: u32) -> Result<Self, String> {
        let api = gpu_api()?;
        let (device, context, swap) = create_device_and_swap(&api, hwnd, width, height)?;
        let pipe = match GpuPipe::new(device, context, &api) {
            Ok(pipe) => pipe,
            Err(err) => {
                unsafe {
                    com_release(swap);
                    com_release(context);
                    com_release(device);
                }
                return Err(err);
            }
        };
        let rtv = match back_buffer_rtv(device, swap) {
            Ok(rtv) => rtv,
            Err(err) => {
                unsafe {
                    pipe.release();
                    com_release(swap);
                    com_release(context);
                    com_release(device);
                }
                return Err(err);
            }
        };
        Ok(Self {
            device,
            context,
            swap,
            rtv,
            pipe,
            atlas: null_mut(),
            atlas_srv: null_mut(),
            width,
            height,
        })
    }

    /// Upload `rgba`, draw it through the compositor, and present.
    fn present(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
        if width != self.width || height != self.height {
            self.resize(width, height)?;
        }
        let opaque = opaque_rgba(rgba, width, height);
        self.ensure_atlas(width, height)?;
        unsafe {
            update_subresource(self.context, self.atlas, &opaque, width * 4);
            draw_quad(
                self.context,
                &self.pipe,
                self.rtv,
                self.atlas_srv,
                width,
                height,
            )?;
            let present: unsafe extern "system" fn(*mut c_void, u32, u32) -> i32 =
                com_fn(self.swap, 8);
            hr(present(self.swap, 0, 0), "Present")?;
        }
        Ok(())
    }

    /// Release the current view, resize the swap chain, and bind the new buffer.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        unsafe {
            com_release(self.rtv);
            self.rtv = null_mut();
            com_release(self.atlas_srv);
            self.atlas_srv = null_mut();
            com_release(self.atlas);
            self.atlas = null_mut();
            let resize: unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> i32 =
                com_fn(self.swap, 13);
            hr(
                resize(self.swap, 1, width, height, FORMAT_RGBA, 0),
                "ResizeBuffers",
            )?;
            self.rtv = back_buffer_rtv(self.device, self.swap)?;
        }
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Keep one RGBA texture matching the window. Recreated after a resize.
    fn ensure_atlas(&mut self, width: u32, height: u32) -> Result<(), String> {
        if !self.atlas.is_null() {
            return Ok(());
        }
        let (tex, srv) = shader_texture(self.device, width, height, BIND_SHADER, USAGE_DEFAULT, 0)?;
        self.atlas = tex;
        self.atlas_srv = srv;
        Ok(())
    }
}

/// Shaders, layout, and sampler shared by the window and the offscreen test.
struct GpuPipe {
    vs: *mut c_void,
    ps: *mut c_void,
    layout: *mut c_void,
    sampler: *mut c_void,
    raster: *mut c_void,
    cbuffer: *mut c_void,
    luma_srv: *mut c_void,
    chroma_srv: *mut c_void,
    luma: *mut c_void,
    chroma: *mut c_void,
}

impl GpuPipe {
    fn new(device: *mut c_void, _context: *mut c_void, api: &GpuApi) -> Result<Self, String> {
        let (vs_bytes, ps_bytes) = compile_compositor(&api.compile)?;
        unsafe {
            let vs = create_shader(device, 12, &vs_bytes)?;
            let ps = create_shader(device, 15, &ps_bytes)?;
            let layout = match create_layout(device, &vs_bytes) {
                Ok(layout) => layout,
                Err(err) => {
                    com_release(vs);
                    com_release(ps);
                    return Err(err);
                }
            };
            let sampler = match create_sampler(device) {
                Ok(sampler) => sampler,
                Err(err) => {
                    com_release(layout);
                    com_release(vs);
                    com_release(ps);
                    return Err(err);
                }
            };
            let raster = match create_rasterizer(device) {
                Ok(raster) => raster,
                Err(err) => {
                    com_release(sampler);
                    com_release(layout);
                    com_release(vs);
                    com_release(ps);
                    return Err(err);
                }
            };
            let constants = [1.0_f32, 1.0, 0.0, 0.0];
            let cbuffer =
                match create_buffer(device, 16, BIND_CONSTANT, Some(bytemuck_f32(&constants))) {
                    Ok(buf) => buf,
                    Err(err) => {
                        com_release(raster);
                        com_release(sampler);
                        com_release(layout);
                        com_release(vs);
                        com_release(ps);
                        return Err(err);
                    }
                };
            let (luma, luma_srv) = match shader_texture(device, 1, 1, BIND_SHADER, USAGE_DEFAULT, 0)
            {
                Ok(pair) => pair,
                Err(err) => {
                    com_release(cbuffer);
                    com_release(raster);
                    com_release(sampler);
                    com_release(layout);
                    com_release(vs);
                    com_release(ps);
                    return Err(err);
                }
            };
            let (chroma, chroma_srv) =
                match shader_texture(device, 1, 1, BIND_SHADER, USAGE_DEFAULT, 0) {
                    Ok(pair) => pair,
                    Err(err) => {
                        com_release(luma_srv);
                        com_release(luma);
                        com_release(cbuffer);
                        com_release(raster);
                        com_release(sampler);
                        com_release(layout);
                        com_release(vs);
                        com_release(ps);
                        return Err(err);
                    }
                };
            Ok(Self {
                vs,
                ps,
                layout,
                sampler,
                raster,
                cbuffer,
                luma_srv,
                chroma_srv,
                luma,
                chroma,
            })
        }
    }

    unsafe fn release(&self) {
        unsafe {
            com_release(self.chroma_srv);
            com_release(self.luma_srv);
            com_release(self.chroma);
            com_release(self.luma);
            com_release(self.cbuffer);
            com_release(self.raster);
            com_release(self.sampler);
            com_release(self.layout);
            com_release(self.ps);
            com_release(self.vs);
        }
    }
}

/// Draw `rgba` into an offscreen target and return the top-down RGBA result.
///
/// Used by the present test. The window path uses the same shader and the same
/// upload. The source is still a CPU buffer.
pub fn sample_cpu_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    if rgba.len() < (width as usize) * (height as usize) * 4 || width == 0 || height == 0 {
        return Err("rgba buffer is shorter than width times height".to_string());
    }
    let api = gpu_api()?;
    let (device, context) = create_device(&api)?;
    let pipe = match GpuPipe::new(device, context, &api) {
        Ok(pipe) => pipe,
        Err(err) => {
            unsafe {
                com_release(context);
                com_release(device);
            }
            return Err(err);
        }
    };
    let (color, _color_srv) =
        match shader_texture(device, width, height, BIND_RENDER_TARGET, USAGE_DEFAULT, 0) {
            Ok(pair) => pair,
            Err(err) => {
                unsafe {
                    pipe.release();
                    com_release(context);
                    com_release(device);
                }
                return Err(err);
            }
        };
    let rtv = match render_target_view(device, color) {
        Ok(rtv) => rtv,
        Err(err) => {
            unsafe {
                com_release(color);
                pipe.release();
                com_release(context);
                com_release(device);
            }
            return Err(err);
        }
    };
    let (atlas, atlas_srv) =
        match shader_texture(device, width, height, BIND_SHADER, USAGE_DEFAULT, 0) {
            Ok(pair) => pair,
            Err(err) => {
                unsafe {
                    com_release(rtv);
                    com_release(color);
                    pipe.release();
                    com_release(context);
                    com_release(device);
                }
                return Err(err);
            }
        };
    let opaque = opaque_rgba(rgba, width, height);
    let readback = unsafe {
        update_subresource(context, atlas, &opaque, width * 4);
        let drawn = draw_quad(context, &pipe, rtv, atlas_srv, width, height);
        let copied = drawn.and_then(|_| copy_texture_rgba(device, context, color, width, height));
        com_release(atlas_srv);
        com_release(atlas);
        com_release(rtv);
        com_release(color);
        pipe.release();
        com_release(context);
        com_release(device);
        copied
    };
    readback
}

/// Compile `VSMain` and `PSMain` from the embedded compositor source.
fn compile_compositor(compile: &D3dCompile) -> Result<(Vec<u8>, Vec<u8>), String> {
    let vs = compile_one(compile, "VSMain", "vs_5_0")?;
    let ps = compile_one(compile, "PSMain", "ps_5_0")?;
    if vs.is_empty() || ps.is_empty() {
        return Err("compositor bytecode was empty".to_string());
    }
    Ok((vs, ps))
}

fn compile_one(compile: &D3dCompile, entry: &str, target: &str) -> Result<Vec<u8>, String> {
    let entry = std::ffi::CString::new(entry).map_err(|_| "shader entry".to_string())?;
    let target = std::ffi::CString::new(target).map_err(|_| "shader target".to_string())?;
    let name = c"compositor.hlsl";
    let mut code = null_mut();
    let mut errors = null_mut();
    let hr_code = unsafe {
        (compile.func)(
            COMPOSITOR_HLSL.as_ptr(),
            COMPOSITOR_HLSL.len(),
            name.as_ptr(),
            null_mut(),
            null_mut(),
            entry.as_ptr(),
            target.as_ptr(),
            0,
            0,
            &mut code,
            &mut errors,
        )
    };
    if hr_code < 0 {
        let message = blob_text(errors);
        unsafe {
            com_release(errors);
            com_release(code);
        }
        return Err(format!("compositor compile failed: {message}"));
    }
    let bytes = blob_bytes(code);
    unsafe {
        com_release(errors);
        com_release(code);
    }
    Ok(bytes)
}

/// Copy `rgba` and force alpha to 255 so the swap chain stays opaque.
fn opaque_rgba(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = rgba[..(width as usize) * (height as usize) * 4].to_vec();
    for pixel in out.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    out
}

fn bytemuck_f32(values: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), values.len() * 4) }
}

struct D3dCompile {
    func: unsafe extern "system" fn(
        *const u8,
        usize,
        *const i8,
        *const c_void,
        *const c_void,
        *const i8,
        *const i8,
        u32,
        u32,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> i32,
}

struct GpuApi {
    create_device: unsafe extern "system" fn(
        *mut c_void,
        u32,
        *mut c_void,
        u32,
        *const u32,
        u32,
        u32,
        *mut *mut c_void,
        *mut u32,
        *mut *mut c_void,
    ) -> i32,
    create_swap: unsafe extern "system" fn(
        *mut c_void,
        u32,
        *mut c_void,
        u32,
        *const u32,
        u32,
        u32,
        *const SwapDesc,
        *mut *mut c_void,
        *mut *mut c_void,
        *mut u32,
        *mut *mut c_void,
    ) -> i32,
    compile: D3dCompile,
}

#[repr(C)]
struct Rational {
    numerator: u32,
    denominator: u32,
}

#[repr(C)]
struct ModeDesc {
    width: u32,
    height: u32,
    refresh: Rational,
    format: u32,
    scanline: u32,
    scaling: u32,
}

#[repr(C)]
struct SampleDesc {
    count: u32,
    quality: u32,
}

#[repr(C)]
struct SwapDesc {
    buffer: ModeDesc,
    sample: SampleDesc,
    usage: u32,
    count: u32,
    window: *mut c_void,
    windowed: i32,
    effect: u32,
    flags: u32,
}

#[repr(C)]
struct TexDesc {
    width: u32,
    height: u32,
    mips: u32,
    array: u32,
    format: u32,
    sample: SampleDesc,
    usage: u32,
    bind: u32,
    cpu: u32,
    misc: u32,
}

#[repr(C)]
struct BufferDesc {
    bytes: u32,
    usage: u32,
    bind: u32,
    cpu: u32,
    misc: u32,
    stride: u32,
}

#[repr(C)]
struct Subresource {
    data: *const c_void,
    pitch: u32,
    slice: u32,
}

#[repr(C)]
struct InputElement {
    name: *const i8,
    index: u32,
    format: u32,
    slot: u32,
    offset: u32,
    class: u32,
    step: u32,
}

#[repr(C)]
struct SamplerDesc {
    filter: u32,
    address_u: u32,
    address_v: u32,
    address_w: u32,
    mip_bias: f32,
    anisotropy: u32,
    comparison: u32,
    border: [f32; 4],
    min_lod: f32,
    max_lod: f32,
}

#[repr(C)]
struct RasterDesc {
    fill: u32,
    cull: u32,
    front_ccw: i32,
    depth_bias: i32,
    bias_clamp: f32,
    slope: f32,
    depth_clip: i32,
    scissor: i32,
    multisample: i32,
    line_aa: i32,
}

#[repr(C)]
struct D3dViewport {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    min_depth: f32,
    max_depth: f32,
}

#[repr(C)]
struct Mapped {
    data: *mut c_void,
    row: u32,
    depth: u32,
}

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const IID_TEXTURE2D: Guid = Guid {
    data1: 0x6f15_aaf2,
    data2: 0xd208,
    data3: 0x4e89,
    data4: [0x9a, 0xb4, 0x48, 0x95, 0x35, 0xd3, 0x4f, 0x9c],
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const i8) -> *mut c_void;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn gpu_api() -> Result<GpuApi, String> {
    unsafe {
        let d3d = LoadLibraryW(wide("d3d11.dll").as_ptr());
        let compiler = LoadLibraryW(wide("d3dcompiler_47.dll").as_ptr());
        if d3d.is_null() || compiler.is_null() {
            return Err("Direct3D 11 was not available".to_string());
        }
        let create_device = GetProcAddress(d3d, c"D3D11CreateDevice".as_ptr());
        let create_swap = GetProcAddress(d3d, c"D3D11CreateDeviceAndSwapChain".as_ptr());
        let compile = GetProcAddress(compiler, c"D3DCompile".as_ptr());
        if create_device.is_null() || create_swap.is_null() || compile.is_null() {
            return Err("Direct3D entry points were missing".to_string());
        }
        Ok(GpuApi {
            create_device: std::mem::transmute(create_device),
            create_swap: std::mem::transmute(create_swap),
            compile: D3dCompile {
                func: std::mem::transmute(compile),
            },
        })
    }
}

fn create_device(api: &GpuApi) -> Result<(*mut c_void, *mut c_void), String> {
    let levels = [0xb000_u32, 0xa000];
    for driver in [DRIVER_HARDWARE, DRIVER_WARP] {
        let mut device = null_mut();
        let mut context = null_mut();
        let mut level = 0_u32;
        let code = unsafe {
            (api.create_device)(
                null_mut(),
                driver,
                null_mut(),
                0,
                levels.as_ptr(),
                levels.len() as u32,
                D3D11_SDK_VERSION,
                &mut device,
                &mut level,
                &mut context,
            )
        };
        if code >= 0 && !device.is_null() && !context.is_null() {
            return Ok((device, context));
        }
        unsafe {
            com_release(context);
            com_release(device);
        }
    }
    Err("could not create a Direct3D 11 device".to_string())
}

fn create_device_and_swap(
    api: &GpuApi,
    hwnd: *mut c_void,
    width: u32,
    height: u32,
) -> Result<(*mut c_void, *mut c_void, *mut c_void), String> {
    let levels = [0xb000_u32, 0xa000];
    let desc = SwapDesc {
        buffer: ModeDesc {
            width,
            height,
            refresh: Rational {
                numerator: 0,
                denominator: 1,
            },
            format: FORMAT_RGBA,
            scanline: 0,
            scaling: 0,
        },
        sample: SampleDesc {
            count: 1,
            quality: 0,
        },
        usage: 32,
        count: 1,
        window: hwnd,
        windowed: 1,
        effect: 0,
        flags: 0,
    };
    for driver in [DRIVER_HARDWARE, DRIVER_WARP] {
        let mut device = null_mut();
        let mut context = null_mut();
        let mut swap = null_mut();
        let mut level = 0_u32;
        let code = unsafe {
            (api.create_swap)(
                null_mut(),
                driver,
                null_mut(),
                0,
                levels.as_ptr(),
                levels.len() as u32,
                D3D11_SDK_VERSION,
                &desc,
                &mut swap,
                &mut device,
                &mut level,
                &mut context,
            )
        };
        if code >= 0 && !device.is_null() && !context.is_null() && !swap.is_null() {
            return Ok((device, context, swap));
        }
        unsafe {
            com_release(swap);
            com_release(context);
            com_release(device);
        }
    }
    Err("could not create a Direct3D 11 swap chain".to_string())
}

fn hr(code: i32, what: &str) -> Result<(), String> {
    if code >= 0 {
        Ok(())
    } else {
        Err(format!("{what} failed ({code})"))
    }
}

unsafe fn com_fn<T>(ptr: *mut c_void, index: isize) -> T {
    let table = unsafe { *(ptr as *const *const usize) };
    let addr = unsafe { *table.offset(index) };
    unsafe { std::mem::transmute_copy(&addr) }
}

unsafe fn com_release(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let release: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { com_fn(ptr, 2) };
    unsafe { release(ptr) };
}

fn blob_bytes(blob: *mut c_void) -> Vec<u8> {
    if blob.is_null() {
        return Vec::new();
    }
    unsafe {
        let pointer: unsafe extern "system" fn(*mut c_void) -> *mut u8 = com_fn(blob, 3);
        let size: unsafe extern "system" fn(*mut c_void) -> usize = com_fn(blob, 4);
        let ptr = pointer(blob);
        let len = size(blob);
        if ptr.is_null() || len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(ptr, len).to_vec()
        }
    }
}

fn blob_text(blob: *mut c_void) -> String {
    let bytes = blob_bytes(blob);
    String::from_utf8_lossy(&bytes).trim().to_string()
}

unsafe fn create_shader(
    device: *mut c_void,
    index: isize,
    bytecode: &[u8],
) -> Result<*mut c_void, String> {
    let create: unsafe extern "system" fn(
        *mut c_void,
        *const u8,
        usize,
        *mut c_void,
        *mut *mut c_void,
    ) -> i32 = unsafe { com_fn(device, index) };
    let mut shader = null_mut();
    hr(
        unsafe {
            create(
                device,
                bytecode.as_ptr(),
                bytecode.len(),
                null_mut(),
                &mut shader,
            )
        },
        "CreateShader",
    )?;
    Ok(shader)
}

unsafe fn create_layout(device: *mut c_void, vs: &[u8]) -> Result<*mut c_void, String> {
    let elements = [
        element(c"POSITION".as_ptr(), 0, FORMAT_RG_F32, 0),
        element(c"TEXCOORD".as_ptr(), 0, FORMAT_RGBA_F32, 8),
        element(c"TEXCOORD".as_ptr(), 1, FORMAT_RGBA_F32, 24),
        element(c"COLOR".as_ptr(), 0, FORMAT_RGBA_F32, 40),
        element(c"COLOR".as_ptr(), 1, FORMAT_RGBA_F32, 56),
        element(c"COLOR".as_ptr(), 2, FORMAT_RGBA_F32, 72),
        element(c"TEXCOORD".as_ptr(), 2, FORMAT_RGBA_F32, 88),
        element(c"TEXCOORD".as_ptr(), 3, FORMAT_RGBA_F32, 104),
        element(c"TEXCOORD".as_ptr(), 4, FORMAT_RGBA_F32, 120),
    ];
    let create: unsafe extern "system" fn(
        *mut c_void,
        *const InputElement,
        u32,
        *const u8,
        usize,
        *mut *mut c_void,
    ) -> i32 = unsafe { com_fn(device, 11) };
    let mut layout = null_mut();
    hr(
        unsafe {
            create(
                device,
                elements.as_ptr(),
                elements.len() as u32,
                vs.as_ptr(),
                vs.len(),
                &mut layout,
            )
        },
        "CreateInputLayout",
    )?;
    Ok(layout)
}

fn element(name: *const i8, index: u32, format: u32, offset: u32) -> InputElement {
    InputElement {
        name,
        index,
        format,
        slot: 0,
        offset,
        class: 0,
        step: 0,
    }
}

unsafe fn create_sampler(device: *mut c_void) -> Result<*mut c_void, String> {
    let desc = SamplerDesc {
        filter: 0,
        address_u: 3,
        address_v: 3,
        address_w: 3,
        mip_bias: 0.0,
        anisotropy: 1,
        comparison: 1,
        border: [0.0; 4],
        min_lod: 0.0,
        max_lod: 0.0,
    };
    let create: unsafe extern "system" fn(
        *mut c_void,
        *const SamplerDesc,
        *mut *mut c_void,
    ) -> i32 = unsafe { com_fn(device, 23) };
    let mut sampler = null_mut();
    hr(
        unsafe { create(device, &desc, &mut sampler) },
        "CreateSamplerState",
    )?;
    Ok(sampler)
}

unsafe fn create_rasterizer(device: *mut c_void) -> Result<*mut c_void, String> {
    let desc = RasterDesc {
        fill: 3,
        cull: 1,
        front_ccw: 0,
        depth_bias: 0,
        bias_clamp: 0.0,
        slope: 0.0,
        depth_clip: 1,
        scissor: 0,
        multisample: 0,
        line_aa: 0,
    };
    let create: unsafe extern "system" fn(*mut c_void, *const RasterDesc, *mut *mut c_void) -> i32 =
        unsafe { com_fn(device, 22) };
    let mut state = null_mut();
    hr(
        unsafe { create(device, &desc, &mut state) },
        "CreateRasterizerState",
    )?;
    Ok(state)
}

unsafe fn create_buffer(
    device: *mut c_void,
    bytes: u32,
    bind: u32,
    initial: Option<&[u8]>,
) -> Result<*mut c_void, String> {
    let desc = BufferDesc {
        bytes,
        usage: USAGE_DEFAULT,
        bind,
        cpu: 0,
        misc: 0,
        stride: 0,
    };
    let sub = initial.map(|data| Subresource {
        data: data.as_ptr().cast(),
        pitch: 0,
        slice: 0,
    });
    let create: unsafe extern "system" fn(
        *mut c_void,
        *const BufferDesc,
        *const Subresource,
        *mut *mut c_void,
    ) -> i32 = unsafe { com_fn(device, 3) };
    let mut buffer = null_mut();
    hr(
        unsafe {
            create(
                device,
                &desc,
                sub.as_ref()
                    .map(|s| s as *const Subresource)
                    .unwrap_or(std::ptr::null()),
                &mut buffer,
            )
        },
        "CreateBuffer",
    )?;
    Ok(buffer)
}

fn shader_texture(
    device: *mut c_void,
    width: u32,
    height: u32,
    bind: u32,
    usage: u32,
    cpu: u32,
) -> Result<(*mut c_void, *mut c_void), String> {
    let desc = TexDesc {
        width,
        height,
        mips: 1,
        array: 1,
        format: FORMAT_RGBA,
        sample: SampleDesc {
            count: 1,
            quality: 0,
        },
        usage,
        bind,
        cpu,
        misc: 0,
    };
    let tex = unsafe {
        let create: unsafe extern "system" fn(
            *mut c_void,
            *const TexDesc,
            *const Subresource,
            *mut *mut c_void,
        ) -> i32 = com_fn(device, 5);
        let mut tex = null_mut();
        hr(
            create(device, &desc, std::ptr::null(), &mut tex),
            "CreateTexture2D",
        )?;
        tex
    };
    if bind & BIND_SHADER == 0 {
        return Ok((tex, null_mut()));
    }
    match unsafe { shader_view(device, tex) } {
        Ok(view) => Ok((tex, view)),
        Err(err) => {
            unsafe { com_release(tex) };
            Err(err)
        }
    }
}

unsafe fn shader_view(device: *mut c_void, tex: *mut c_void) -> Result<*mut c_void, String> {
    let create: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const c_void,
        *mut *mut c_void,
    ) -> i32 = unsafe { com_fn(device, 7) };
    let mut view = null_mut();
    hr(
        unsafe { create(device, tex, std::ptr::null(), &mut view) },
        "CreateShaderResourceView",
    )?;
    Ok(view)
}

fn render_target_view(device: *mut c_void, tex: *mut c_void) -> Result<*mut c_void, String> {
    unsafe {
        let create: unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            *const c_void,
            *mut *mut c_void,
        ) -> i32 = com_fn(device, 9);
        let mut view = null_mut();
        hr(
            create(device, tex, std::ptr::null(), &mut view),
            "CreateRenderTargetView",
        )?;
        Ok(view)
    }
}

fn back_buffer_rtv(device: *mut c_void, swap: *mut c_void) -> Result<*mut c_void, String> {
    unsafe {
        let get_buffer: unsafe extern "system" fn(
            *mut c_void,
            u32,
            *const Guid,
            *mut *mut c_void,
        ) -> i32 = com_fn(swap, 9);
        let mut tex = null_mut();
        hr(get_buffer(swap, 0, &IID_TEXTURE2D, &mut tex), "GetBuffer")?;
        let view = match render_target_view(device, tex) {
            Ok(view) => view,
            Err(err) => {
                com_release(tex);
                return Err(err);
            }
        };
        com_release(tex);
        Ok(view)
    }
}

unsafe fn update_subresource(context: *mut c_void, resource: *mut c_void, rgba: &[u8], pitch: u32) {
    let update: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        u32,
        *const c_void,
        *const c_void,
        u32,
        u32,
    ) = unsafe { com_fn(context, 48) };
    unsafe {
        update(
            context,
            resource,
            0,
            std::ptr::null(),
            rgba.as_ptr().cast(),
            pitch,
            0,
        );
    }
}

unsafe fn draw_quad(
    context: *mut c_void,
    pipe: &GpuPipe,
    rtv: *mut c_void,
    atlas: *mut c_void,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let vertices = quad_vertices(width, height);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            vertices.as_ptr().cast::<u8>(),
            std::mem::size_of_val(&vertices),
        )
    };
    // The device pointer is the context's device. CreateBuffer is on the device,
    // which the caller stored beside the context. The vertex buffer is created
    // through the immediate context's device via a stored pipe field... The
    // pipe does not keep the device. Create the buffer with the context's
    // GetDevice (ID3D11DeviceChild slot 3).
    let device = unsafe { device_from_context(context)? };
    let vbuf = unsafe { create_buffer(device, bytes.len() as u32, BIND_VERTEX, Some(bytes))? };
    unsafe { com_release(device) };
    let constants = [width as f32, height as f32, 0.0_f32, 0.0];
    unsafe { update_subresource(context, pipe.cbuffer, bytemuck_f32(&constants), 16) };

    unsafe {
        let ia_layout: unsafe extern "system" fn(*mut c_void, *mut c_void) = com_fn(context, 17);
        ia_layout(context, pipe.layout);
        let stride = 136_u32;
        let offset = 0_u32;
        let ia_vb: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            *const *mut c_void,
            *const u32,
            *const u32,
        ) = com_fn(context, 18);
        ia_vb(context, 0, 1, &vbuf, &stride, &offset);
        let topology: unsafe extern "system" fn(*mut c_void, u32) = com_fn(context, 24);
        topology(context, TOPOLOGY_TRIANGLE);
        let vs: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, u32) =
            com_fn(context, 11);
        vs(context, pipe.vs, null_mut(), 0);
        let ps: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, u32) =
            com_fn(context, 9);
        ps(context, pipe.ps, null_mut(), 0);
        let vs_cb: unsafe extern "system" fn(*mut c_void, u32, u32, *const *mut c_void) =
            com_fn(context, 7);
        vs_cb(context, 0, 1, &pipe.cbuffer);
        let ps_cb: unsafe extern "system" fn(*mut c_void, u32, u32, *const *mut c_void) =
            com_fn(context, 16);
        ps_cb(context, 0, 1, &pipe.cbuffer);
        let resources = [atlas, pipe.luma_srv, pipe.chroma_srv];
        let ps_sr: unsafe extern "system" fn(*mut c_void, u32, u32, *const *mut c_void) =
            com_fn(context, 8);
        ps_sr(context, 0, 3, resources.as_ptr());
        let ps_sam: unsafe extern "system" fn(*mut c_void, u32, u32, *const *mut c_void) =
            com_fn(context, 10);
        ps_sam(context, 0, 1, &pipe.sampler);
        let rs: unsafe extern "system" fn(*mut c_void, *mut c_void) = com_fn(context, 43);
        rs(context, pipe.raster);
        let viewport = D3dViewport {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let viewports: unsafe extern "system" fn(*mut c_void, u32, *const D3dViewport) =
            com_fn(context, 44);
        viewports(context, 1, &viewport);
        let om: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void, *mut c_void) =
            com_fn(context, 33);
        om(context, 1, &rtv, null_mut());
        let clear: unsafe extern "system" fn(*mut c_void, *mut c_void, *const f32) =
            com_fn(context, 50);
        let magenta = [1.0_f32, 0.0, 1.0, 1.0];
        clear(context, rtv, magenta.as_ptr());
        let draw: unsafe extern "system" fn(*mut c_void, u32, u32) = com_fn(context, 13);
        draw(context, 6, 0);
        let empty: *mut c_void = null_mut();
        ps_sr(context, 0, 3, [empty, empty, empty].as_ptr());
        om(context, 0, &empty, null_mut());
        com_release(vbuf);
    }
    Ok(())
}

unsafe fn device_from_context(context: *mut c_void) -> Result<*mut c_void, String> {
    let get_device: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) =
        unsafe { com_fn(context, 3) };
    let mut device = null_mut();
    unsafe { get_device(context, &mut device) };
    if device.is_null() {
        Err("the context had no device".to_string())
    } else {
        Ok(device)
    }
}

#[repr(C)]
struct Vertex {
    local: [f32; 2],
    rect: [f32; 4],
    clip: [f32; 4],
    fill: [f32; 4],
    fill_end: [f32; 4],
    border: [f32; 4],
    uv: [f32; 4],
    params: [f32; 4],
    params2: [f32; 4],
}

fn quad_vertices(width: u32, height: u32) -> [Vertex; 6] {
    let rect = [-2.0, -2.0, width as f32 + 4.0, height as f32 + 4.0];
    let clip = [0.0, 0.0, width as f32, height as f32];
    let fill = [1.0, 1.0, 1.0, 1.0];
    let uv = [0.0, 0.0, 1.0, 1.0];
    // Atlas sample, solid fill. NV12 (params.z > 1.5) is not selected.
    let params = [0.0, 0.0, 1.0, 0.0];
    let zero = [0.0; 4];
    let corner = |x: f32, y: f32| Vertex {
        local: [x, y],
        rect,
        clip,
        fill,
        fill_end: zero,
        border: zero,
        uv,
        params,
        params2: zero,
    };
    [
        corner(0.0, 0.0),
        corner(1.0, 0.0),
        corner(0.0, 1.0),
        corner(1.0, 0.0),
        corner(1.0, 1.0),
        corner(0.0, 1.0),
    ]
}

unsafe fn copy_texture_rgba(
    device: *mut c_void,
    context: *mut c_void,
    source: *mut c_void,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let desc = TexDesc {
        width,
        height,
        mips: 1,
        array: 1,
        format: FORMAT_RGBA,
        sample: SampleDesc {
            count: 1,
            quality: 0,
        },
        usage: USAGE_STAGING,
        bind: 0,
        cpu: CPU_READ,
        misc: 0,
    };
    let staging = unsafe {
        let create: unsafe extern "system" fn(
            *mut c_void,
            *const TexDesc,
            *const Subresource,
            *mut *mut c_void,
        ) -> i32 = com_fn(device, 5);
        let mut tex = null_mut();
        hr(
            create(device, &desc, std::ptr::null(), &mut tex),
            "CreateTexture2D",
        )?;
        tex
    };
    unsafe {
        let copy: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) =
            com_fn(context, 47);
        copy(context, staging, source);
        let map: unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            u32,
            u32,
            u32,
            *mut Mapped,
        ) -> i32 = com_fn(context, 14);
        let mut mapped = Mapped {
            data: null_mut(),
            row: 0,
            depth: 0,
        };
        let code = map(context, staging, 0, MAP_READ, 0, &mut mapped);
        if code < 0 || mapped.data.is_null() {
            com_release(staging);
            return Err(format!("Map failed ({code})"));
        }
        let mut out = vec![0_u8; (width as usize) * (height as usize) * 4];
        let row_bytes = (width as usize) * 4;
        for y in 0..height as usize {
            let src = mapped.data.add(y * mapped.row as usize);
            let dst = y * row_bytes;
            std::ptr::copy_nonoverlapping(src.cast::<u8>(), out.as_mut_ptr().add(dst), row_bytes);
        }
        let unmap: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) = com_fn(context, 15);
        unmap(context, staging, 0);
        com_release(staging);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_rgba_survives_a_d3d11_present() {
        let width = 8_u32;
        let height = 8_u32;
        let mut rgba = vec![0_u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                if x < 4 {
                    rgba[i] = 255;
                    rgba[i + 3] = 255;
                } else {
                    rgba[i + 2] = 255;
                    rgba[i + 3] = 128;
                }
            }
        }
        let out = sample_cpu_rgba(&rgba, width, height).expect("d3d11 present");
        let pixel = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            [out[i], out[i + 1], out[i + 2], out[i + 3]]
        };
        assert_eq!(pixel(1, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(6, 1), [0, 0, 255, 255]);
    }
}
