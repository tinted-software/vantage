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

use hashbrown::HashMap;
pub use vantage_raster::{gl, FragFn, FragState, SampledTexture, Varyings};
pub mod offsets;
#[cfg(feature = "spirv-frontend")]
pub mod spirv;

#[cfg(feature = "backend-llvm")]
pub mod program;

#[cfg(all(
    feature = "backend-llvm",
    any(feature = "backend-jit", feature = "backend-interp")
))]
pub mod lowering;

#[cfg(all(feature = "backend-llvm", feature = "backend-jit"))]
pub mod jit;

#[cfg(all(feature = "backend-llvm", feature = "backend-interp"))]
pub mod interp;

#[cfg(feature = "backend-llvm")]
pub use program::FragmentIr;
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
    backend: Backend,
}

/// Executable form chosen at compile time from the active backend features.
enum Backend {
    /// Portable scalar evaluation; always available, used as fallback.
    Scalar,
    /// Native machine code produced by cranelift-jit. The owning program
    /// must outlive the entry pointer (executable lives in its arena).
    #[cfg(feature = "backend-jit")]
    Jit {
        _program: jit::JitFragmentProgram,
        entry: FragFn,
    },
    /// cranelift-interpreter execution of the same lowered function.
    #[cfg(all(feature = "backend-interp", not(feature = "backend-jit")))]
    Interp(interp::InterpFragmentProgram),
}

impl Backend {
    fn compile(key: &FragmentKey) -> Self {
        #[cfg(feature = "backend-jit")]
        {
            let ir = program::build_fragment_program(key);
            match jit::JitFragmentProgram::compile(&ir) {
                Ok(p) => {
                    return Backend::Jit {
                        entry: p.as_frag_fn(),
                        _program: p,
                    };
                }
                Err(e) => panic!("JIT COMPILE FAILED: {e}"),
            }
        }
        #[cfg(not(feature = "backend-jit"))]
        {
            #[cfg(feature = "backend-interp")]
            {
                let ir = program::build_fragment_program(key);
                if ir.has_external_calls() {
                    // Interpreter cannot call host symbols (@vantage_expf).
                    return Backend::Scalar;
                }
                return Backend::Interp(interp::InterpFragmentProgram::compile(&ir));
            }
            #[cfg(not(feature = "backend-interp"))]
            Backend::Scalar
        }
    }
}

impl FragmentProgram {
    /// Name of the active execution backend, for diagnostics/tests.
    pub fn backend_name(&self) -> &'static str {
        match &self.backend {
            #[cfg(feature = "backend-jit")]
            Backend::Jit { .. } => "cranelift-jit",
            #[cfg(all(feature = "backend-interp", not(feature = "backend-jit")))]
            Backend::Interp(_) => "cranelift-interp",
            Backend::Scalar => "scalar",
        }
    }

    pub fn compile(key: &FragmentKey) -> Self {
        Self {
            key: *key,
            backend: Backend::compile(key),
        }
    }
    /// Native entry point for draw-time binding. `None` means this backend
    /// needs the generic dispatcher (scalar/interpreter).
    #[inline]
    fn direct_entry(&self) -> Option<FragFn> {
        match &self.backend {
            #[cfg(feature = "backend-jit")]
            Backend::Jit { entry, .. } => Some(*entry),
            #[cfg(all(feature = "backend-interp", not(feature = "backend-jit")))]
            Backend::Interp(_) => None,
            Backend::Scalar => None,
        }
    }

    /// Evaluates the fragment program for the given state and varyings.
    /// Returns true if alpha test passed, false if discarded.
    #[inline]
    pub unsafe fn evaluate(
        &self,
        ctx: *const FragState,
        varying: *const Varyings,
        out: *mut [u8; 4],
    ) -> bool {
        match &self.backend {
            #[cfg(feature = "backend-jit")]
            Backend::Jit { entry, .. } => unsafe { (entry)(ctx, varying, out) },
            #[cfg(all(feature = "backend-interp", not(feature = "backend-jit")))]
            Backend::Interp(p) => unsafe { p.evaluate(ctx, varying, out) },
            Backend::Scalar => unsafe { self.scalar_eval(ctx, varying, out) },
        }
    }

    /// Portable scalar evaluation of the fragment pipeline.
    unsafe fn scalar_eval(
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
                let linear = !matches!(tex.mag_filter, gl::NEAREST);
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
    x.clamp(0.0, 1.0)
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

#[inline(always)]
fn wrap_c(x: f32, mode: u32) -> f32 {
    if mode == gl::REPEAT {
        if (0.0..1.0).contains(&x) {
            x
        } else {
            let i = x as i32;
            let f = x - (i as f32);
            if f < 0.0 {
                f + 1.0
            } else {
                f
            }
        }
    } else if mode == gl::CLAMP_TO_EDGE {
        clamp_unit(x)
    } else {
        let f = if (0.0..1.0).contains(&x) {
            x
        } else {
            x - libm::floorf(x)
        };
        if (libm::floorf(x) as i64) & 1 == 0 {
            f
        } else {
            1.0 - f
        }
    }
}

#[inline(always)]
fn sample_tex(tex: &SampledTexture, u: f32, v: f32, linear: bool) -> [f32; 4] {
    if !tex.enabled || tex.width == 0 || tex.height == 0 || tex.data.is_null() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let fu = wrap_c(u, tex.wrap_s) * tex.width as f32;
    let fv = wrap_c(v, tex.wrap_t) * tex.height as f32;

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
        let x = libm::floorf(fu) as i64;
        let y = libm::floorf(fv) as i64;
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

/// Process-wide pipeline cache. Programs own their executable allocations;
/// callers may retain a JIT entry pointer after releasing the cache lock.
static PROGRAM_CACHE: spin::Mutex<Option<ShaderCache>> = spin::Mutex::new(None);

/// Select the fragment routine once per draw. This keeps key construction,
/// hashing, and the global cache lock out of the per-pixel raster loop.
pub fn fragment_function_for_state(state: &FragState) -> FragFn {
    let key = FragmentKey::from_state(state);
    let mut guard = PROGRAM_CACHE.lock();
    let prog = guard
        .get_or_insert_with(ShaderCache::new)
        .get_or_compile(&key);
    prog.direct_entry().unwrap_or(fragment_entry_point)
}

/// Generic fallback dispatcher for scalar/interpreter configurations.
/// JIT-enabled draw paths use [`fragment_function_for_state`] instead.
pub unsafe fn fragment_entry_point(
    ctx: *const FragState,
    varying: *const Varyings,
    out: *mut [u8; 4],
) -> bool {
    let key = FragmentKey::from_state(unsafe { &*ctx });
    let mut guard = PROGRAM_CACHE.lock();
    let prog = guard
        .get_or_insert_with(ShaderCache::new)
        .get_or_compile(&key);
    unsafe { prog.evaluate(ctx, varying, out) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "backend-jit")]
    #[test]
    fn test_exponential_fog_jit_matches_reference() {
        // MCPE regression: exponential fog (mode 2) is the only fragment
        // program that emits a call (@vantage_expf). The external-name
        // relocation path (ExternalName::TestCase) panicked at runtime with
        // `not implemented!()` in ModuleReloc::from_mach_reloc; linear-fog
        // coverage let it ship.
        let state = FragState {
            textures: [
                vantage_raster::SampledTexture::disabled(),
                vantage_raster::SampledTexture::disabled(),
            ],
            texenv_mode: [gl::TEXENV_REPLACE, gl::TEXENV_REPLACE],
            texenv_color: [[0.0; 4]; 2],
            alpha_func: gl::ALWAYS,
            alpha_ref: 0.0,
            fog_mode: 2,
            fog_start: 0.0,
            fog_end: 1.0,
            fog_density: 0.3,
            fog_color: [0.5, 0.5, 0.5, 1.0],
        };
        let varyings = Varyings {
            color: [0.3, 0.4, 0.5, 1.0],
            tex0: [0.0, 0.0],
            tex1: [0.0, 0.0],
            fog: 0.7,
            view_z: 1.0,
        };
        let key = FragmentKey::from_state(&state);
        let prog = FragmentProgram::compile(&key);
        assert_eq!(prog.backend_name(), "cranelift-jit");
        let mut out_prog = [0u8; 4];
        let pass_prog =
            unsafe { prog.evaluate(&state, &varyings, out_prog.as_mut_ptr() as *mut [u8; 4]) };
        let mut out_ref = [0u8; 4];
        let pass_ref = unsafe {
            vantage_raster::reference_frag(&state, &varyings, out_ref.as_mut_ptr() as *mut [u8; 4])
        };
        assert_eq!(pass_prog, pass_ref);
        assert_eq!(
            out_prog, out_ref,
            "expf call must produce native-code result"
        );
    }

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
        // The whole point of the pipeline: this must be native code, not the
        // scalar fallback (a silent jit Err would otherwise keep tests green).
        assert_eq!(prog.backend_name(), "cranelift-jit");

        let mut out_prog = [0u8; 4];
        let pass_prog =
            unsafe { prog.evaluate(&state, &varyings, out_prog.as_mut_ptr() as *mut [u8; 4]) };

        let mut out_ref = [0u8; 4];
        let pass_ref = unsafe {
            vantage_raster::reference_frag(&state, &varyings, out_ref.as_mut_ptr() as *mut [u8; 4])
        };

        assert_eq!(pass_prog, pass_ref, "Alpha pass must match");
        assert_eq!(
            out_prog, out_ref,
            "Color output must match reference_frag exactly"
        );
    }
}
