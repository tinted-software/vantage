//! Vantage software rasterizer.
//!
//! Scanline rasterization with 28.4 fixed-point edge functions (mesa swrast
//! convention). All fixed-function per-pixel state — depth test, stencil,
//! blending, masks — lives here. The fragment program seam (`FragFn`)
//! computes only per-pixel color (texenv, fog, alpha-test): Phase 3 fills it
//! with pliron-interpreted (later JIT'd) code.
//!
//! Threading: single-threaded. The draw loop is structured over tiles so a
//! future threads feature can dispatch `Job`s; do not rely on global state.
#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use core::ffi::c_void;

// ============================================================================
// GL enum values (fixed by the spec; duplicated here so raster never
// depends on the GLES crate).
// ============================================================================

pub mod gl {
    // Depth/stencil comparison functions.
    pub const NEVER: u32 = 0x0200;
    pub const LESS: u32 = 0x0201;
    pub const EQUAL: u32 = 0x0202;
    pub const LEQUAL: u32 = 0x0203;
    pub const GREATER: u32 = 0x0204;
    pub const NOTEQUAL: u32 = 0x0205;
    pub const GEQUAL: u32 = 0x0206;
    pub const ALWAYS: u32 = 0x0207;

    // Blend factors.
    pub const ZERO: u32 = 0;
    pub const ONE: u32 = 1;
    pub const SRC_COLOR: u32 = 0x0300;
    pub const ONE_MINUS_SRC_COLOR: u32 = 0x0301;
    pub const SRC_ALPHA: u32 = 0x0302;
    pub const ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
    pub const DST_ALPHA: u32 = 0x0304;
    pub const ONE_MINUS_DST_ALPHA: u32 = 0x0305;
    pub const DST_COLOR: u32 = 0x0306;
    pub const ONE_MINUS_DST_COLOR: u32 = 0x0307;
    pub const SRC_ALPHA_SATURATE: u32 = 0x0308;
    pub const CONSTANT_COLOR: u32 = 0x8001;
    pub const ONE_MINUS_CONSTANT_COLOR: u32 = 0x8002;
    pub const CONSTANT_ALPHA: u32 = 0x8003;
    pub const ONE_MINUS_CONSTANT_ALPHA: u32 = 0x8004;

    // Stencil ops.
    pub const STENCIL_KEEP: u32 = 0x1E00;
    pub const STENCIL_ZERO: u32 = 0x1E01;
    pub const STENCIL_REPLACE: u32 = 0x1E02;
    pub const STENCIL_INCR: u32 = 0x1E03;
    pub const STENCIL_DECR: u32 = 0x1E04;
    pub const STENCIL_INVERT: u32 = 0x1E0A;
    pub const STENCIL_INCR_WRAP: u32 = 0x8507;
    pub const STENCIL_DECR_WRAP: u32 = 0x8508;

    // Cull / winding.
    pub const FRONT: u32 = 0x0404;
    pub const BACK: u32 = 0x0405;
    pub const FRONT_AND_BACK: u32 = 0x0408;
    pub const CW: u32 = 0x0900;
    pub const CCW: u32 = 0x0901;

    // Texture wrap / filter.
    pub const REPEAT: u32 = 0x2901;
    pub const MIRRORED_REPEAT: u32 = 0x8370;
    pub const CLAMP_TO_EDGE: u32 = 0x812F;
    pub const NEAREST: u32 = 0x2600;
    pub const LINEAR: u32 = 0x2601;

    // TexEnv modes (GLES 1.1).
    pub const TEXENV_MODULATE: u32 = 0x2100;
    pub const TEXENV_REPLACE: u32 = 0x2101;
    pub const TEXENV_DECAL: u32 = 0x2104;
    pub const TEXENV_BLEND: u32 = 0x2105;
    pub const TEXENV_ADD: u32 = 0x0104;

    // Texture formats (internal).
    pub const GL_RGBA: u32 = 0x1908;
    pub const GL_LUMINANCE: u32 = 0x1909;
    pub const GL_LUMINANCE_ALPHA: u32 = 0x190A;
}

/// Comparison of `a` against `ref` per a GL func enum. `a OP ref`.
pub fn depth_pass(func: u32, a: f32, reference: f32) -> bool {
    match func {
        gl::NEVER => false,
        gl::LESS => a < reference,
        gl::EQUAL => a == reference,
        gl::LEQUAL => a <= reference,
        gl::GREATER => a > reference,
        gl::NOTEQUAL => a != reference,
        gl::GEQUAL => a >= reference,
        gl::ALWAYS => true,
        _ => true,
    }
}

// ============================================================================
// Vertex / varying / fragment-state contract
// ============================================================================

/// Packed per-vertex input produced by `vantage-gles` (clip-space position
/// plus interpolated varyings). `bytemuck::Pod` so vertex arrays are raw
/// hal buffers.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Clip-space position (x, y, z, w). The z already carries the GL
    /// [-w, w] convention; the rasterizer remaps with `(z + w) * 0.5`
    /// exactly like the old WGSL did.
    pub pos: [f32; 4],
    /// Vertex color (pre-multiplied by lighting on CPU).
    pub color: [f32; 4],
    /// Texture coordinates for units 0 and 1.
    pub tex0: [f32; 2],
    pub tex1: [f32; 2],
    /// Fog coordinate (eye-space distance).
    pub fog: f32,
    pub _pad: f32,
}

/// Interpolated per-pixel varyings passed to the fragment function.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Varyings {
    pub color: [f32; 4],
    pub tex0: [f32; 2],
    pub tex1: [f32; 2],
    pub fog: f32,
    pub view_z: f32,
}

/// One sampled texture as seen by fragment code.
pub struct SampledTexture {
    /// RGBA8 texel data (level 0), already converted by the GLES layer.
    pub data: *const u8,
    pub data_len: usize,
    pub width: u32,
    pub height: u32,
    /// GL_LUMINANCE / GL_LUMINANCE_ALPHA / GL_RGBA expansion applied at
    /// sample time (matches what the old wgpu path uploaded).
    pub format: u32,
    pub min_filter: u32,
    pub mag_filter: u32,
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub enabled: bool,
}

impl SampledTexture {
    pub fn disabled() -> Self {
        SampledTexture {
            data: core::ptr::null(),
            data_len: 0,
            width: 0,
            height: 0,
            format: 0,
            min_filter: gl::NEAREST,
            mag_filter: gl::NEAREST,
            wrap_s: gl::REPEAT,
            wrap_t: gl::REPEAT,
            enabled: false,
        }
    }
}

/// Per-pixel fragment context: textures, texenv, fog, alpha ref.
pub struct FragState {
    /// Texture units 0..2 (GLES1 CM exposure in this driver).
    pub textures: [SampledTexture; 2],
    /// TexEnv mode per unit (GL_MODULATE / REPLACE / DECAL / BLEND / ADD).
    pub texenv_mode: [u32; 2],
    /// TexEnv blend color (GL_TEXENV_COLOR), rgba 0..1.
    pub texenv_color: [[f32; 4]; 2],
    /// Alpha test function (GLenum) and reference; func == ALWAYS disables.
    pub alpha_func: u32,
    pub alpha_ref: f32,
    /// Fog: mode 0=none, 1=linear, 2=exp, 3=exp2; start/end/density.
    pub fog_mode: u32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_density: f32,
    pub fog_color: [f32; 4],
}

/// Fragment function seam. Given the shared state and interpolated varyings,
/// writes the pre-blend BGRA8 color and returns whether the fragment passes
/// the alpha test (`false` = discard).
///
/// Invariant: this function ONLY computes color/alpha-test/fog per pixel.
/// All blending, depth and stencil handling stays in the raster core.
pub type FragFn =
    unsafe fn(ctx: *const FragState, varying: *const Varyings, out: *mut [u8; 4]) -> bool;

// ============================================================================
// Raster pipeline state
// ============================================================================

/// Full fixed-function raster state for one draw (mirrors the hal Pipeline
/// plus the state the raster core owns directly).
#[derive(Debug, Clone)]
pub struct RasterState {
    // Framebuffer targets' geometry.
    pub viewport: (i32, i32, u32, u32),
    pub depth_range: (f32, f32),
    pub scissor: Option<(i32, i32, u32, u32)>,

    // Primitive assembly.
    pub cull_mode: u32, // 0=none, GL_FRONT, GL_BACK, GL_FRONT_AND_BACK
    pub front_face_ccw: bool,
    pub shade_flat: bool,

    // Depth.
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_func: u32,

    // Stencil (single-sided: GLES 1.1 has one func/op set).
    pub stencil_test: bool,
    pub stencil_func: u32,
    pub stencil_ref: i32,
    pub stencil_func_mask: u32,
    pub stencil_write_mask: u32,
    pub stencil_fail: u32,
    pub stencil_zfail: u32,
    pub stencil_zpass: u32,

    // Blending (glBlendFuncSeparate factors as GL enums).
    pub blend_enabled: bool,
    pub src_rgb: u32,
    pub dst_rgb: u32,
    pub src_alpha: u32,
    pub dst_alpha: u32,
    pub blend_color: [f32; 4],

    // Masks.
    pub color_mask: u8, // bit0..3 = r,g,b,a

    // Polygon offset.
    pub polygon_offset_fill: bool,
    pub polygon_offset_factor: f32,
    pub polygon_offset_units: f32,

    // Points / lines.
    pub point_size: f32,
    pub line_width: f32,
}

impl Default for RasterState {
    fn default() -> Self {
        RasterState {
            viewport: (0, 0, 1, 1),
            depth_range: (0.0, 1.0),
            scissor: None,
            cull_mode: 0,
            front_face_ccw: true,
            shade_flat: false,
            depth_test: false,
            depth_write: true,
            depth_func: gl::LESS,
            stencil_test: false,
            stencil_func: gl::ALWAYS,
            stencil_ref: 0,
            stencil_func_mask: !0,
            stencil_write_mask: !0,
            stencil_fail: gl::STENCIL_KEEP,
            stencil_zfail: gl::STENCIL_KEEP,
            stencil_zpass: gl::STENCIL_KEEP,
            blend_enabled: false,
            src_rgb: gl::ONE,
            dst_rgb: gl::ZERO,
            src_alpha: gl::ONE,
            dst_alpha: gl::ZERO,
            blend_color: [0.0; 4],
            color_mask: 0x0F,
            polygon_offset_fill: false,
            polygon_offset_factor: 0.0,
            polygon_offset_units: 0.0,
            point_size: 1.0,
            line_width: 1.0,
        }
    }
}

// ============================================================================
// Targets
// ============================================================================

/// Color target: interleaved RGBA8 or BGRA8 (4 bytes/px). Depth: f32.
/// Stencil: u8.
pub struct Targets<'a> {
    pub color: &'a mut [u8],
    pub color_stride: u32, // bytes per row
    pub width: u32,
    pub height: u32,
    pub bgra_order: bool,
    pub depth: Option<&'a mut [u8]>,
    pub stencil: Option<&'a mut [u8]>,
}

// ============================================================================
// Fragment shuffling helpers
// ============================================================================

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn clamp01(x: f32) -> f32 {
    if x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

fn wrap_coord(x: f32, mode: u32) -> f32 {
    match mode {
        gl::REPEAT => x - libm::floorf(x),
        gl::MIRRORED_REPEAT => {
            let f = x - libm::floorf(x);
            if (libm::floorf(x) as i64) & 1 == 0 {
                f
            } else {
                1.0 - f
            }
        }
        _ => clamp01(x), // CLAMP_TO_EDGE
    }
}

fn texel_or_edge(tex: &SampledTexture, x: i64, y: i64) -> (i64, i64) {
    if tex.wrap_s == gl::CLAMP_TO_EDGE || tex.wrap_t == gl::CLAMP_TO_EDGE {
        (
            x.clamp(0, tex.width as i64 - 1),
            y.clamp(0, tex.height as i64 - 1),
        )
    } else {
        // REPEAT / MIRRORED_REPEAT already folded the integer part away.
        (
            x.rem_euclid(tex.width as i64),
            y.rem_euclid(tex.height as i64),
        )
    }
}

fn sample_texel(tex: &SampledTexture, u: f32, v: f32, linear: bool) -> [f32; 4] {
    if !tex.enabled || tex.width == 0 || tex.height == 0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let fu = wrap_coord(u, tex.wrap_s) * tex.width as f32;
    let fv = wrap_coord(v, tex.wrap_t) * tex.height as f32;
    let [r, g, b, a] = if linear {
        bilinear(tex, fu, fv)
    } else {
        nearest(tex, fu, fv)
    };
    expand_to_rgba(tex.format, r, g, b, a)
}

fn fetch(tex: &SampledTexture, x: i64, y: i64) -> [f32; 4] {
    let (x, y) = texel_or_edge(tex, x, y);
    let o = ((y * tex.width as i64 + x) * 4) as usize;
    if tex.data.is_null() || o + 4 > tex.data_len {
        return [0.0, 0.0, 0.0, 1.0];
    }
    unsafe {
        let p = tex.data.add(o);
        [
            *p as f32 / 255.0,
            *p.add(1) as f32 / 255.0,
            *p.add(2) as f32 / 255.0,
            *p.add(3) as f32 / 255.0,
        ]
    }
}

fn nearest(tex: &SampledTexture, fu: f32, fv: f32) -> [f32; 4] {
    let x = libm::floorf(fu - 0.5) as i64;
    let y = libm::floorf(fv - 0.5) as i64;
    fetch(tex, x, y)
}

fn bilinear(tex: &SampledTexture, fu: f32, fv: f32) -> [f32; 4] {
    let x0 = libm::floorf(fu - 0.5) as i64;
    let y0 = libm::floorf(fv - 0.5) as i64;
    let tx = fu - 0.5 - x0 as f32;
    let ty = fv - 0.5 - y0 as f32;
    let c00 = fetch(tex, x0, y0);
    let c10 = fetch(tex, x0 + 1, y0);
    let c01 = fetch(tex, x0, y0 + 1);
    let c11 = fetch(tex, x0 + 1, y0 + 1);
    let mut out = [0.0f32; 4];
    for i in 0..4 {
        let a = lerp(c00[i], c10[i], tx);
        let b = lerp(c01[i], c11[i], tx);
        out[i] = lerp(a, b, ty);
    }
    out
}

/// Expand stored channels to full RGBA per texture format.
fn expand_to_rgba(format: u32, r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
    match format {
        gl::GL_LUMINANCE => [r, r, r, 1.0],
        gl::GL_LUMINANCE_ALPHA => [r, r, r, a],
        gl::GL_RGBA => [r, g, b, a],
        _ => [r, g, b, a],
    }
}

/// TexEnv combine (GLES 1.1 §3.7.10) for one unit.
fn texenv_apply(mode: u32, tex: [f32; 4], color: [f32; 4], env: [f32; 4]) -> [f32; 4] {
    match mode {
        gl::TEXENV_REPLACE => tex,
        gl::TEXENV_MODULATE => [
            tex[0] * color[0],
            tex[1] * color[1],
            tex[2] * color[2],
            tex[3] * color[3],
        ],
        gl::TEXENV_DECAL => {
            let a = tex[3];
            [
                color[0] * (1.0 - a) + tex[0] * a,
                color[1] * (1.0 - a) + tex[1] * a,
                color[2] * (1.0 - a) + tex[2] * a,
                color[3],
            ]
        }
        gl::TEXENV_BLEND => [
            color[0] * (1.0 - tex[0]) + env[0] * tex[0],
            color[1] * (1.0 - tex[1]) + env[1] * tex[1],
            color[2] * (1.0 - tex[2]) + env[2] * tex[2],
            color[3] * tex[3],
        ],
        gl::TEXENV_ADD => [
            (color[0] + tex[0]).min(1.0),
            (color[1] + tex[1]).min(1.0),
            (color[2] + tex[2]).min(1.0),
            (color[3] * tex[3]).min(1.0),
        ],
        _ => [
            tex[0] * color[0],
            tex[1] * color[1],
            tex[2] * color[2],
            tex[3] * color[3],
        ],
    }
}

fn alpha_pass(func: u32, a: f32, reference: f32) -> bool {
    match func {
        gl::NEVER => false,
        gl::LESS => a < reference,
        gl::EQUAL => a == reference,
        gl::LEQUAL => a <= reference,
        gl::GREATER => a > reference,
        gl::NOTEQUAL => a != reference,
        gl::GEQUAL => a >= reference,
        gl::ALWAYS => true,
        _ => true,
    }
}

fn apply_fog(state: &FragState, color: &mut [f32; 4], fog: f32) {
    let f = match state.fog_mode {
        1 => {
            // LINEAR: (end - |c|) / (end - start)
            (state.fog_end - fog) / (state.fog_end - state.fog_start)
        }
        2 => libm::expf(-state.fog_density * fog), // EXP
        3 => {
            let d = state.fog_density * fog; // EXP2
            libm::expf(-(d * d))
        }
        _ => return,
    };
    let f = clamp01(f);
    for i in 0..3 {
        color[i] = color[i] * f + state.fog_color[i] * (1.0 - f);
    }
}

/// Reference scalar fragment shader (also the test oracle): texenv chain over
/// unit 0 then unit 1, fog, alpha test.
pub fn reference_frag(ctx: *const FragState, varying: *const Varyings, out: *mut [u8; 4]) -> bool {
    unsafe {
        let st = &*ctx;
        let v = &*varying;
        let mut color = v.color;
        for unit in 0..2 {
            let tex = &st.textures[unit];
            if !tex.enabled {
                continue;
            }
            let tc = if unit == 0 { v.tex0 } else { v.tex1 };
            let linear = match tex.mag_filter {
                gl::NEAREST => false,
                _ => true,
            };
            let s = sample_texel(tex, tc[0], tc[1], linear);
            color = texenv_apply(st.texenv_mode[unit], s, color, st.texenv_color[unit]);
        }
        if st.fog_mode != 0 {
            apply_fog(st, &mut color, v.fog);
        }
        if !alpha_pass(st.alpha_func, color[3], st.alpha_ref) {
            return false;
        }
        let o = &mut *out;
        // Color out is BGRA8 pre-blend.
        o[0] = (clamp01(color[2]) * 255.0 + 0.5) as u8;
        o[1] = (clamp01(color[1]) * 255.0 + 0.5) as u8;
        o[2] = (clamp01(color[0]) * 255.0 + 0.5) as u8;
        o[3] = (clamp01(color[3]) * 255.0 + 0.5) as u8;
        true
    }
}

// ============================================================================
// Rasterizer core & Per-Fragment Pipeline
// ============================================================================

pub struct PrimitiveRasterizer<'a> {
    pub state: &'a RasterState,
    pub targets: Targets<'a>,
    pub frag_fn: FragFn,
    pub frag_ctx: *const FragState,
}

impl<'a> PrimitiveRasterizer<'a> {
    pub fn new(
        state: &'a RasterState,
        targets: Targets<'a>,
        frag_fn: FragFn,
        frag_ctx: *const FragState,
    ) -> Self {
        Self {
            state,
            targets,
            frag_fn,
            frag_ctx,
        }
    }

    #[inline]
    fn test_stencil(&mut self, offset: usize) -> (bool, bool) {
        if !self.state.stencil_test {
            return (true, true);
        }
        let Some(ref mut stencil) = self.targets.stencil else {
            return (true, true);
        };
        let s_val = stencil[offset] as u32;
        let ref_val = (self.state.stencil_ref as u32) & self.state.stencil_func_mask;
        let masked_s = s_val & self.state.stencil_func_mask;

        let pass = match self.state.stencil_func {
            gl::NEVER => false,
            gl::LESS => ref_val < masked_s,
            gl::LEQUAL => ref_val <= masked_s,
            gl::EQUAL => ref_val == masked_s,
            gl::GEQUAL => ref_val >= masked_s,
            gl::GREATER => ref_val > masked_s,
            gl::NOTEQUAL => ref_val != masked_s,
            gl::ALWAYS => true,
            _ => true,
        };
        (self.state.stencil_test, pass)
    }

    #[inline]
    fn apply_stencil_op(&mut self, offset: usize, op: u32) {
        if !self.state.stencil_test {
            return;
        }
        let Some(ref mut stencil) = self.targets.stencil else {
            return;
        };
        let orig = stencil[offset];
        let ref_val = self.state.stencil_ref as u8;
        let mut new_val = match op {
            gl::STENCIL_KEEP => orig,
            gl::STENCIL_ZERO => 0,
            gl::STENCIL_REPLACE => ref_val,
            gl::STENCIL_INCR => orig.saturating_add(1),
            gl::STENCIL_DECR => orig.saturating_sub(1),
            gl::STENCIL_INVERT => !orig,
            gl::STENCIL_INCR_WRAP => orig.wrapping_add(1),
            gl::STENCIL_DECR_WRAP => orig.wrapping_sub(1),
            _ => orig,
        };
        let mask = self.state.stencil_write_mask as u8;
        new_val = (orig & !mask) | (new_val & mask);
        stencil[offset] = new_val;
    }

    #[inline]
    fn blend_channel(
        src: f32,
        dst: f32,
        sfactor: u32,
        dfactor: u32,
        src_c: f32,
        dst_c: f32,
        src_a: f32,
        dst_a: f32,
        cc: f32,
        ca: f32,
    ) -> f32 {
        let s_weight = match sfactor {
            gl::ZERO => 0.0,
            gl::ONE => 1.0,
            gl::SRC_COLOR => src_c,
            gl::ONE_MINUS_SRC_COLOR => 1.0 - src_c,
            gl::SRC_ALPHA => src_a,
            gl::ONE_MINUS_SRC_ALPHA => 1.0 - src_a,
            gl::DST_ALPHA => dst_a,
            gl::ONE_MINUS_DST_ALPHA => 1.0 - dst_a,
            gl::DST_COLOR => dst_c,
            gl::ONE_MINUS_DST_COLOR => 1.0 - dst_c,
            gl::SRC_ALPHA_SATURATE => src_a.min(1.0 - dst_a),
            gl::CONSTANT_COLOR => cc,
            gl::ONE_MINUS_CONSTANT_COLOR => 1.0 - cc,
            gl::CONSTANT_ALPHA => ca,
            gl::ONE_MINUS_CONSTANT_ALPHA => 1.0 - ca,
            _ => 1.0,
        };
        let d_weight = match dfactor {
            gl::ZERO => 0.0,
            gl::ONE => 1.0,
            gl::SRC_COLOR => src_c,
            gl::ONE_MINUS_SRC_COLOR => 1.0 - src_c,
            gl::SRC_ALPHA => src_a,
            gl::ONE_MINUS_SRC_ALPHA => 1.0 - src_a,
            gl::DST_ALPHA => dst_a,
            gl::ONE_MINUS_DST_ALPHA => 1.0 - dst_a,
            gl::DST_COLOR => dst_c,
            gl::ONE_MINUS_DST_COLOR => 1.0 - dst_c,
            gl::CONSTANT_COLOR => cc,
            gl::ONE_MINUS_CONSTANT_COLOR => 1.0 - cc,
            gl::CONSTANT_ALPHA => ca,
            gl::ONE_MINUS_CONSTANT_ALPHA => 1.0 - ca,
            _ => 0.0,
        };
        clamp01(src * s_weight + dst * d_weight)
    }

    #[inline]
    pub fn shade_and_blend_pixel(&mut self, px: i32, py: i32, z: f32, varying: &Varyings) {
        if px < 0 || py < 0 || px >= self.targets.width as i32 || py >= self.targets.height as i32 {
            return;
        }
        if let Some((sx, sy, sw, sh)) = self.state.scissor {
            if px < sx || py < sy || px >= sx + sw as i32 || py >= sy + sh as i32 {
                return;
            }
        }

        let pixel_idx = (py as usize) * (self.targets.width as usize) + (px as usize);

        // 1. Fragment Function (color, texenv, fog, alpha-test)
        let mut frag_color = [0u8; 4]; // BGRA8
        let alpha_pass = unsafe {
            (self.frag_fn)(
                self.frag_ctx,
                varying as *const Varyings,
                frag_color.as_mut_ptr() as *mut [u8; 4],
            )
        };
        if !alpha_pass {
            return;
        }

        // 2. Stencil Test
        let (stencil_enabled, stencil_pass) = self.test_stencil(pixel_idx);
        if stencil_enabled && !stencil_pass {
            self.apply_stencil_op(pixel_idx, self.state.stencil_fail);
            return;
        }

        // 3. Depth Test
        let depth_pass = if self.state.depth_test {
            if let Some(ref depth) = self.targets.depth {
                let off = pixel_idx * 4;
                let fb_z = f32::from_ne_bytes([
                    depth[off],
                    depth[off + 1],
                    depth[off + 2],
                    depth[off + 3],
                ]);
                depth_pass(self.state.depth_func, z, fb_z)
            } else {
                true
            }
        } else {
            true
        };

        if stencil_enabled {
            if !depth_pass {
                self.apply_stencil_op(pixel_idx, self.state.stencil_zfail);
                return;
            } else {
                self.apply_stencil_op(pixel_idx, self.state.stencil_zpass);
            }
        } else if !depth_pass {
            return;
        }

        // 4. Depth Write
        if self.state.depth_test && self.state.depth_write {
            if let Some(ref mut depth) = self.targets.depth {
                let off = pixel_idx * 4;
                depth[off..off + 4].copy_from_slice(&z.to_ne_bytes());
            }
        }

        // 5. Blending & Color write
        let color_off = (py as usize) * (self.targets.color_stride as usize) + (px as usize) * 4;
        let c_dst = &self.targets.color[color_off..color_off + 4];

        // frag_color is BGRA8 (o[0]=B, o[1]=G, o[2]=R, o[3]=A)
        let (src_r, src_g, src_b, src_a) = (
            frag_color[2] as f32 / 255.0,
            frag_color[1] as f32 / 255.0,
            frag_color[0] as f32 / 255.0,
            frag_color[3] as f32 / 255.0,
        );

        let (dst_r, dst_g, dst_b, dst_a) = if self.targets.bgra_order {
            (
                c_dst[2] as f32 / 255.0,
                c_dst[1] as f32 / 255.0,
                c_dst[0] as f32 / 255.0,
                c_dst[3] as f32 / 255.0,
            )
        } else {
            (
                c_dst[0] as f32 / 255.0,
                c_dst[1] as f32 / 255.0,
                c_dst[2] as f32 / 255.0,
                c_dst[3] as f32 / 255.0,
            )
        };

        let (final_r, final_g, final_b, final_a) = if self.state.blend_enabled {
            let cc = self.state.blend_color;
            let r = Self::blend_channel(
                src_r,
                dst_r,
                self.state.src_rgb,
                self.state.dst_rgb,
                src_r,
                dst_r,
                src_a,
                dst_a,
                cc[0],
                cc[3],
            );
            let g = Self::blend_channel(
                src_g,
                dst_g,
                self.state.src_rgb,
                self.state.dst_rgb,
                src_g,
                dst_g,
                src_a,
                dst_a,
                cc[1],
                cc[3],
            );
            let b = Self::blend_channel(
                src_b,
                dst_b,
                self.state.src_rgb,
                self.state.dst_rgb,
                src_b,
                dst_b,
                src_a,
                dst_a,
                cc[2],
                cc[3],
            );
            let a = Self::blend_channel(
                src_a,
                dst_a,
                self.state.src_alpha,
                self.state.dst_alpha,
                src_a,
                dst_a,
                src_a,
                dst_a,
                cc[3],
                cc[3],
            );
            (r, g, b, a)
        } else {
            (src_r, src_g, src_b, src_a)
        };

        let out_r = (final_r * 255.0 + 0.5) as u8;
        let out_g = (final_g * 255.0 + 0.5) as u8;
        let out_b = (final_b * 255.0 + 0.5) as u8;
        let out_a = (final_a * 255.0 + 0.5) as u8;

        let mask = self.state.color_mask;
        let c_out = &mut self.targets.color[color_off..color_off + 4];
        if self.targets.bgra_order {
            if mask & 1 != 0 {
                c_out[2] = out_r;
            }
            if mask & 2 != 0 {
                c_out[1] = out_g;
            }
            if mask & 4 != 0 {
                c_out[0] = out_b;
            }
            if mask & 8 != 0 {
                c_out[3] = out_a;
            }
        } else {
            if mask & 1 != 0 {
                c_out[0] = out_r;
            }
            if mask & 2 != 0 {
                c_out[1] = out_g;
            }
            if mask & 4 != 0 {
                c_out[2] = out_b;
            }
            if mask & 8 != 0 {
                c_out[3] = out_a;
            }
        }
    }

    /// Project clip coordinate (x,y,z,w) -> screen coordinate (px, py, z)
    pub fn project_vertex(&self, v: &Vertex) -> Option<([f32; 3], f32)> {
        if v.pos[3] <= 0.000001 {
            return None;
        }
        let inv_w = 1.0 / v.pos[3];
        let ndc_x = v.pos[0] * inv_w;
        let ndc_y = v.pos[1] * inv_w;
        // Remap OpenGL clip-z [-w, w] to [0, 1]
        let ndc_z = (v.pos[2] + v.pos[3]) * 0.5 * inv_w;

        let (vx, vy, vw, vh) = self.state.viewport;
        let (d_near, d_far) = self.state.depth_range;

        let screen_x = vx as f32 + (ndc_x * 0.5 + 0.5) * vw as f32;
        let screen_y = vy as f32 + (1.0 - (ndc_y * 0.5 + 0.5)) * vh as f32; // Invert Y for top-down framebuffer
        let screen_z = d_near + ndc_z * (d_far - d_near);

        Some(([screen_x, screen_y, screen_z], inv_w))
    }

    pub fn draw_triangle(&mut self, v0: &Vertex, v1: &Vertex, v2: &Vertex) {
        let (Some((p0, w0)), Some((p1, w1)), Some((p2, w2))) = (
            self.project_vertex(v0),
            self.project_vertex(v1),
            self.project_vertex(v2),
        ) else {
            return;
        };

        // 28.4 fixed-point coordinates
        let x0 = libm::roundf((p0[0] * 16.0)) as i64;
        let y0 = libm::roundf((p0[1] * 16.0)) as i64;
        let x1 = libm::roundf((p1[0] * 16.0)) as i64;
        let y1 = libm::roundf((p1[1] * 16.0)) as i64;
        let x2 = libm::roundf((p2[0] * 16.0)) as i64;
        let y2 = libm::roundf((p2[1] * 16.0)) as i64;

        // Signed area * 2 (in 24.8)
        let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
        if area == 0 {
            return;
        }

        let is_ccw = area > 0;
        let is_front = is_ccw == self.state.front_face_ccw;

        // Culling
        match self.state.cull_mode {
            gl::FRONT if is_front => return,
            gl::BACK if !is_front => return,
            gl::FRONT_AND_BACK => return,
            _ => {}
        }

        // Polygon offset
        let mut z_offset = 0.0f32;
        if self.state.polygon_offset_fill {
            let dz_dx = (p1[2] - p0[2]) / ((p1[0] - p0[0]).abs().max(1.0));
            let dz_dy = (p2[2] - p0[2]) / ((p2[1] - p0[1]).abs().max(1.0));
            let max_slope = dz_dx.abs().max(dz_dy.abs());
            z_offset = max_slope * self.state.polygon_offset_factor
                + self.state.polygon_offset_units * 0.00001;
        }

        // Bounding box
        let min_x = ((x0.min(x1).min(x2)) >> 4).max(0) as i32;
        let max_x = ((x0.max(x1).max(x2) + 15) >> 4).min(self.targets.width as i64 - 1) as i32;
        let min_y = ((y0.min(y1).min(y2)) >> 4).max(0) as i32;
        let max_y = ((y0.max(y1).max(y2) + 15) >> 4).min(self.targets.height as i64 - 1) as i32;

        let inv_area = 1.0 / (area as f32);

        // Scanline rasterization
        for py in min_y..=max_y {
            let fy = (py as i64 * 16) + 8; // pixel center in 28.4
            for px in min_x..=max_x {
                let fx = (px as i64 * 16) + 8;

                // Edge functions (top-left rule)
                let w0_edge = (x2 - x1) * (fy - y1) - (y2 - y1) * (fx - x1);
                let w1_edge = (x0 - x2) * (fy - y2) - (y0 - y2) * (fx - x2);
                let w2_edge = (x1 - x0) * (fy - y0) - (y1 - y0) * (fx - x0);

                let inside = if area > 0 {
                    w0_edge >= 0 && w1_edge >= 0 && w2_edge >= 0
                } else {
                    w0_edge <= 0 && w1_edge <= 0 && w2_edge <= 0
                };

                if !inside {
                    continue;
                }

                let l0 = (w0_edge as f32) * inv_area;
                let l1 = (w1_edge as f32) * inv_area;
                let l2 = (w2_edge as f32) * inv_area;

                let interp_z = l0 * p0[2] + l1 * p1[2] + l2 * p2[2] + z_offset;
                let interp_w = l0 * w0 + l1 * w1 + l2 * w2;
                let r_w = 1.0 / interp_w;

                // Varyings interpolation
                let varying = if self.state.shade_flat {
                    // Provoking vertex is the last vertex v2
                    Varyings {
                        color: v2.color,
                        tex0: [v2.tex0[0], v2.tex0[1]],
                        tex1: [v2.tex1[0], v2.tex1[1]],
                        fog: v2.fog,
                        view_z: p2[2],
                    }
                } else {
                    Varyings {
                        color: [
                            l0 * v0.color[0] + l1 * v1.color[0] + l2 * v2.color[0],
                            l0 * v0.color[1] + l1 * v1.color[1] + l2 * v2.color[1],
                            l0 * v0.color[2] + l1 * v1.color[2] + l2 * v2.color[2],
                            l0 * v0.color[3] + l1 * v1.color[3] + l2 * v2.color[3],
                        ],
                        // Perspective-correct texture coordinates
                        tex0: [
                            (l0 * v0.tex0[0] * w0 + l1 * v1.tex0[0] * w1 + l2 * v2.tex0[0] * w2)
                                * r_w,
                            (l0 * v0.tex0[1] * w0 + l1 * v1.tex0[1] * w1 + l2 * v2.tex0[1] * w2)
                                * r_w,
                        ],
                        tex1: [
                            (l0 * v0.tex1[0] * w0 + l1 * v1.tex1[0] * w1 + l2 * v2.tex1[0] * w2)
                                * r_w,
                            (l0 * v0.tex1[1] * w0 + l1 * v1.tex1[1] * w1 + l2 * v2.tex1[1] * w2)
                                * r_w,
                        ],
                        fog: l0 * v0.fog + l1 * v1.fog + l2 * v2.fog,
                        view_z: interp_z,
                    }
                };

                self.shade_and_blend_pixel(px, py, interp_z, &varying);
            }
        }
    }

    pub fn draw_point(&mut self, v: &Vertex) {
        let Some((p, _w)) = self.project_vertex(v) else {
            return;
        };
        let radius = (self.state.point_size * 0.5).max(0.5);
        let r2 = radius * radius;

        let min_x = libm::floorf((p[0] - radius)) as i32;
        let max_x = libm::ceilf((p[0] + radius)) as i32;
        let min_y = libm::floorf((p[1] - radius)) as i32;
        let max_y = libm::ceilf((p[1] + radius)) as i32;

        let varying = Varyings {
            color: v.color,
            tex0: v.tex0,
            tex1: v.tex1,
            fog: v.fog,
            view_z: p[2],
        };

        for py in min_y..=max_y {
            let dy = py as f32 + 0.5 - p[1];
            for px in min_x..=max_x {
                let dx = px as f32 + 0.5 - p[0];
                if dx * dx + dy * dy <= r2 {
                    self.shade_and_blend_pixel(px, py, p[2], &varying);
                }
            }
        }
    }

    pub fn draw_line(&mut self, v0: &Vertex, v1: &Vertex) {
        let (Some((p0, _)), Some((p1, _))) = (self.project_vertex(v0), self.project_vertex(v1))
        else {
            return;
        };

        let mut x0 = libm::roundf(p0[0]) as i32;
        let mut y0 = libm::roundf(p0[1]) as i32;
        let x1 = libm::roundf(p1[0]) as i32;
        let y1 = libm::roundf(p1[1]) as i32;

        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        let total_dist = libm::hypotf((x1 - x0) as f32, (y1 - y0) as f32).max(1.0);

        loop {
            let cur_dist = libm::hypotf((x0 - p0[0] as i32) as f32, (y0 - p0[1] as i32) as f32);
            let t = (cur_dist / total_dist).clamp(0.0, 1.0);
            let z = lerp(p0[2], p1[2], t);

            let varying = Varyings {
                color: [
                    lerp(v0.color[0], v1.color[0], t),
                    lerp(v0.color[1], v1.color[1], t),
                    lerp(v0.color[2], v1.color[2], t),
                    lerp(v0.color[3], v1.color[3], t),
                ],
                tex0: [
                    lerp(v0.tex0[0], v1.tex0[0], t),
                    lerp(v0.tex0[1], v1.tex0[1], t),
                ],
                tex1: [
                    lerp(v0.tex1[0], v1.tex1[0], t),
                    lerp(v0.tex1[1], v1.tex1[1], t),
                ],
                fog: lerp(v0.fog, v1.fog, t),
                view_z: z,
            };

            self.shade_and_blend_pixel(x0, y0, z, &varying);

            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triangle_depth_and_alpha() {
        let mut color = [0u8; 16 * 16 * 4];
        let mut depth = [0u8; 16 * 16 * 4];

        // Clear depth to 1.0
        for chunk in depth.chunks_exact_mut(4) {
            chunk.copy_from_slice(&1.0f32.to_ne_bytes());
        }

        let mut state = RasterState::default();
        state.viewport = (0, 0, 16, 16);
        state.depth_test = true;
        state.depth_func = gl::LEQUAL;

        let targets = Targets {
            color: &mut color,
            color_stride: 16 * 4,
            width: 16,
            height: 16,
            bgra_order: false,
            depth: Some(&mut depth),
            stencil: None,
        };

        let mut frag_state = FragState {
            textures: [SampledTexture::disabled(), SampledTexture::disabled()],
            texenv_mode: [gl::TEXENV_REPLACE, gl::TEXENV_REPLACE],
            texenv_color: [[0.0; 4]; 2],
            alpha_func: gl::GREATER,
            alpha_ref: 0.5,
            fog_mode: 0,
            fog_start: 0.0,
            fog_end: 1.0,
            fog_density: 0.0,
            fog_color: [0.0; 4],
        };

        let mut raster = PrimitiveRasterizer::new(
            &state,
            targets,
            reference_frag,
            &frag_state as *const FragState,
        );

        // Draw red triangle with alpha = 1.0 at z = 0.5 (should pass alpha > 0.5 and write depth 0.5)
        let v0 = Vertex {
            pos: [-1.0, -1.0, 0.5, 1.0],
            color: [1.0, 0.0, 0.0, 1.0],
            tex0: [0.0, 0.0],
            tex1: [0.0, 0.0],
            fog: 0.0,
            _pad: 0.0,
        };
        let v1 = Vertex {
            pos: [1.0, -1.0, 0.5, 1.0],
            color: [1.0, 0.0, 0.0, 1.0],
            tex0: [1.0, 0.0],
            tex1: [0.0, 0.0],
            fog: 0.0,
            _pad: 0.0,
        };
        let v2 = Vertex {
            pos: [0.0, 1.0, 0.5, 1.0],
            color: [1.0, 0.0, 0.0, 1.0],
            tex0: [0.5, 1.0],
            tex1: [0.0, 0.0],
            fog: 0.0,
            _pad: 0.0,
        };

        raster.draw_triangle(&v0, &v1, &v2);

        // Now draw a blue triangle with alpha = 0.2 (should be rejected by alpha test)
        let b0 = Vertex {
            pos: [-1.0, -1.0, 0.2, 1.0],
            color: [0.0, 0.0, 1.0, 0.2],
            tex0: [0.0, 0.0],
            tex1: [0.0, 0.0],
            fog: 0.0,
            _pad: 0.0,
        };
        let b1 = Vertex {
            pos: [1.0, -1.0, 0.2, 1.0],
            color: [0.0, 0.0, 1.0, 0.2],
            tex0: [1.0, 0.0],
            tex1: [0.0, 0.0],
            fog: 0.0,
            _pad: 0.0,
        };
        let b2 = Vertex {
            pos: [0.0, 1.0, 0.2, 1.0],
            color: [0.0, 0.0, 1.0, 0.2],
            tex0: [0.5, 1.0],
            tex1: [0.0, 0.0],
            fog: 0.0,
            _pad: 0.0,
        };

        raster.draw_triangle(&b0, &b1, &b2);
        drop(raster);

        for y in 0..16 {
            let mut row = alloc::string::String::new();
            for x in 0..16 {
                let idx = (y * 16 + x) * 4;
                if color[idx] > 0 {
                    row.push('R');
                } else {
                    row.push('.');
                }
            }
            // print row
        }
        let center_idx = (8 * 16 + 8) * 4;
        assert_eq!(
            color[center_idx], 255,
            "Center pixel should remain red after alpha discard"
        );
        assert_eq!(
            color[center_idx + 2],
            0,
            "Blue component should not be written"
        );
    }
}
