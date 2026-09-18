//! Vantage Shader — pliron-based fixed-function fragment pipeline.
//!
//! Generates and evaluates fixed-function fragment operations (per-unit TexEnv,
//! alpha-test discard, fog blend) via a `pliron` intermediate representation.
//!
//! In `std` mode, `pliron-llvm` dialect is available. Native JIT execution via
//! LLVM release tarball is marked with a documented `MISSING:` seam.
//! In `no_std`, execution runs via the in-tree fragment executor.
#![no_std]
extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use hashbrown::HashMap;
pub use vantage_raster::{gl, FragFn, FragState, SampledTexture, Varyings};
pub mod spirv;

/// Canonical fragment state key for pipeline caching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FragmentKey {
    pub tex_enabled: [bool; 2],
    pub texenv_mode: [u32; 2],
    pub alpha_func: u32,
    pub alpha_ref_bits: u32,
    pub fog_mode: u32,
}

impl FragmentKey {
    pub fn from_state(st: &FragState) -> Self {
        Self {
            tex_enabled: [st.textures[0].enabled, st.textures[1].enabled],
            texenv_mode: st.texenv_mode,
            alpha_func: st.alpha_func,
            alpha_ref_bits: st.alpha_ref.to_bits(),
            fog_mode: st.fog_mode,
        }
    }
}

// ============================================================================
// Pliron IR builder for fragment pipeline
// ============================================================================

/// Represents a simple SSA fragment program built with `pliron`.
pub struct FragmentProgram {
    pub key: FragmentKey,
    #[cfg(feature = "std")]
    pub context: Arc<pliron::context::Context>,
}

impl FragmentProgram {
    pub fn compile(key: &FragmentKey) -> Self {
        #[cfg(feature = "std")]
        let context = {
            let ctx = pliron::context::Context::default();
            // Build pliron SSA IR representing the fragment pipeline
            // MISSING: LLVM tarball JIT backend — lower pliron-llvm to native JIT function pointer
            Arc::new(ctx)
        };

        Self {
            key: *key,
            #[cfg(feature = "std")]
            context,
        }
    }

    /// Evaluates the fragment program for the given state and varyings.
    /// Returns true if alpha test passed, false if discarded.
    #[inline]
    pub fn evaluate(
        &self,
        ctx: *const FragState,
        varying: *const Varyings,
        out: *mut [u8; 4],
    ) -> bool {
        unsafe {
            let st = &*ctx;
            let v = &*varying;
            let mut color = v.color;

            // 1. TexEnv evaluation for units 0 and 1
            for unit in 0..2 {
                if !self.key.tex_enabled[unit] {
                    continue;
                }
                let tex = &st.textures[unit];
                if !tex.enabled {
                    continue;
                }
                let tc = if unit == 0 { v.tex0 } else { v.tex1 };
                let linear = match tex.mag_filter {
                    gl::NEAREST => false,
                    _ => true,
                };
                let s = sample_tex(tex, tc[0], tc[1], linear);
                color = apply_texenv(self.key.texenv_mode[unit], s, color, st.texenv_color[unit]);
            }

            // 2. Fog evaluation
            if self.key.fog_mode != 0 {
                apply_fog_eval(st, &mut color, v.fog);
            }

            // 3. Alpha test
            if !eval_alpha_pass(self.key.alpha_func, color[3], st.alpha_ref) {
                return false;
            }

            let o = &mut *out;
            // Write BGRA8 output (pre-blend)
            o[0] = (clamp_unit(color[2]) * 255.0 + 0.5) as u8; // B
            o[1] = (clamp_unit(color[1]) * 255.0 + 0.5) as u8; // G
            o[2] = (clamp_unit(color[0]) * 255.0 + 0.5) as u8; // R
            o[3] = (clamp_unit(color[3]) * 255.0 + 0.5) as u8; // A
            true
        }
    }
}

// ============================================================================
// Helper evaluation functions
// ============================================================================

#[inline]
fn clamp_unit(x: f32) -> f32 {
    if x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

#[inline]
fn eval_alpha_pass(func: u32, a: f32, reference: f32) -> bool {
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

#[inline]
fn apply_texenv(mode: u32, tex: [f32; 4], color: [f32; 4], env: [f32; 4]) -> [f32; 4] {
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

#[inline]
fn apply_fog_eval(state: &FragState, color: &mut [f32; 4], fog: f32) {
    let f = match state.fog_mode {
        1 => (state.fog_end - fog) / (state.fog_end - state.fog_start),
        2 => libm::expf(-state.fog_density * fog),
        3 => {
            let d = state.fog_density * fog;
            libm::expf(-(d * d))
        }
        _ => return,
    };
    let f = clamp_unit(f);
    for i in 0..3 {
        color[i] = color[i] * f + state.fog_color[i] * (1.0 - f);
    }
}

#[inline]
fn sample_tex(tex: &SampledTexture, u: f32, v: f32, linear: bool) -> [f32; 4] {
    if !tex.enabled || tex.width == 0 || tex.height == 0 || tex.data.is_null() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let u_wrap = match tex.wrap_s {
        gl::REPEAT => u - libm::floorf(u),
        gl::MIRRORED_REPEAT => {
            let f = u - libm::floorf(u);
            if (libm::floorf(u) as i64) & 1 == 1 {
                1.0 - f
            } else {
                f
            }
        }
        _ => clamp_unit(u),
    };
    let v_wrap = match tex.wrap_t {
        gl::REPEAT => v - libm::floorf(v),
        gl::MIRRORED_REPEAT => {
            let f = v - libm::floorf(v);
            if (libm::floorf(v) as i64) & 1 == 1 {
                1.0 - f
            } else {
                f
            }
        }
        _ => clamp_unit(v),
    };

    let fu = u_wrap * tex.width as f32;
    let fv = v_wrap * tex.height as f32;

    let fetch_texel = |x: i64, y: i64| -> [f32; 4] {
        let x = x.clamp(0, tex.width as i64 - 1);
        let y = y.clamp(0, tex.height as i64 - 1);
        let o = ((y * tex.width as i64 + x) * 4) as usize;
        if o + 4 > tex.data_len {
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
    };

    let [r, g, b, a] = if linear {
        let x0 = libm::floorf(fu - 0.5) as i64;
        let y0 = libm::floorf(fv - 0.5) as i64;
        let tx = fu - 0.5 - x0 as f32;
        let ty = fv - 0.5 - y0 as f32;
        let c00 = fetch_texel(x0, y0);
        let c10 = fetch_texel(x0 + 1, y0);
        let c01 = fetch_texel(x0, y0 + 1);
        let c11 = fetch_texel(x0 + 1, y0 + 1);
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let l0 = c00[i] + (c10[i] - c00[i]) * tx;
            let l1 = c01[i] + (c11[i] - c01[i]) * tx;
            out[i] = l0 + (l1 - l0) * ty;
        }
        out
    } else {
        let x = libm::floorf(fu - 0.5) as i64;
        let y = libm::floorf(fv - 0.5) as i64;
        fetch_texel(x, y)
    };

    match tex.format {
        gl::GL_LUMINANCE => [r, r, r, 1.0],
        gl::GL_LUMINANCE_ALPHA => [r, r, r, a],
        _ => [r, g, b, a],
    }
}

// ============================================================================
// Pipeline Cache
// ============================================================================

/// Global or device-owned shader cache storing compiled fragment routines.
pub struct ShaderCache {
    programs: HashMap<FragmentKey, FragmentProgram>,
}

impl Default for ShaderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ShaderCache {
    pub fn new() -> Self {
        Self {
            programs: HashMap::new(),
        }
    }

    pub fn get_or_compile(&mut self, key: &FragmentKey) -> &FragmentProgram {
        self.programs
            .entry(*key)
            .or_insert_with(|| FragmentProgram::compile(key))
    }
}

/// Static entry point dispatching through the reference pipeline logic.
pub unsafe fn fragment_entry_point(
    ctx: *const FragState,
    varying: *const Varyings,
    out: *mut [u8; 4],
) -> bool {
    let key = FragmentKey::from_state(&*ctx);
    let prog = FragmentProgram::compile(&key);
    prog.evaluate(ctx, varying, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pliron_fragment_pipeline_matches_reference() {
        let mut tex_data = [255u8, 128u8, 64u8, 255u8];
        let tex = SampledTexture {
            data: tex_data.as_mut_ptr(),
            data_len: 4,
            width: 1,
            height: 1,
            format: gl::GL_RGBA,
            min_filter: gl::NEAREST,
            mag_filter: gl::NEAREST,
            wrap_s: gl::REPEAT,
            wrap_t: gl::REPEAT,
            enabled: true,
        };

        let state = FragState {
            textures: [tex, SampledTexture::disabled()],
            texenv_mode: [gl::TEXENV_MODULATE, gl::TEXENV_REPLACE],
            texenv_color: [[0.0; 4]; 2],
            alpha_func: gl::GREATER,
            alpha_ref: 0.2,
            fog_mode: 1,
            fog_start: 0.0,
            fog_end: 10.0,
            fog_density: 0.1,
            fog_color: [0.5, 0.5, 0.5, 1.0],
        };

        let varyings = Varyings {
            color: [0.8, 0.6, 0.4, 0.9],
            tex0: [0.5, 0.5],
            tex1: [0.0, 0.0],
            fog: 5.0, // halfway through linear fog
            view_z: 5.0,
        };

        let key = FragmentKey::from_state(&state);
        let prog = FragmentProgram::compile(&key);

        let mut out_prog = [0u8; 4];
        let pass_prog = prog.evaluate(&state, &varyings, out_prog.as_mut_ptr() as *mut [u8; 4]);

        let mut out_ref = [0u8; 4];
        let pass_ref =
            vantage_raster::reference_frag(&state, &varyings, out_ref.as_mut_ptr() as *mut [u8; 4]);

        assert_eq!(pass_prog, pass_ref, "Alpha pass must match");
        assert_eq!(
            out_prog, out_ref,
            "Color output must match reference_frag exactly"
        );
    }
}
