//! Compile-time layout facts for the fragment ABI.
//!
//! Compiled fragment programs (pliron -> cranelift) receive raw pointers to
//! `FragState` / `Varyings` and address fields via constant offsets. These
//! structs are `#[repr(C)]` in `vantage-raster`; the constants below are
//! derived with `offset_of!` so they cannot drift from the real layout, and
//! `layout_asserts()` pins the sizes at every entry into the backend.

use vantage_raster::{FragState, SampledTexture, Varyings};

macro_rules! off {
    ($ty:ty, $field:ident) => {
        core::mem::offset_of!($ty, $field) as u32
    };
}

/// Byte offset of a field inside `Varyings`.
pub mod varyings {
    use super::*;
    pub const COLOR: u32 = off!(Varyings, color);
    pub const TEX0: u32 = off!(Varyings, tex0);
    pub const TEX1: u32 = off!(Varyings, tex1);
    pub const FOG: u32 = off!(Varyings, fog);
    pub const SIZE: u32 = core::mem::size_of::<Varyings>() as u32;
}

/// Byte offsets inside `FragState`. `TEXTURE_STRIDE` indexes unit 0/1.
pub mod state {
    use super::*;
    pub const TEXTURES: u32 = off!(FragState, textures);
    pub const TEXTURE_STRIDE: u32 = core::mem::size_of::<SampledTexture>() as u32;
    pub const TEXENV_MODE: u32 = off!(FragState, texenv_mode);
    pub const TEXENV_COLOR: u32 = off!(FragState, texenv_color);
    pub const ALPHA_FUNC: u32 = off!(FragState, alpha_func);
    pub const ALPHA_REF: u32 = off!(FragState, alpha_ref);
    pub const FOG_MODE: u32 = off!(FragState, fog_mode);
    pub const FOG_START: u32 = off!(FragState, fog_start);
    pub const FOG_END: u32 = off!(FragState, fog_end);
    pub const FOG_DENSITY: u32 = off!(FragState, fog_density);
    pub const FOG_COLOR: u32 = off!(FragState, fog_color);
}

/// Byte offsets inside `SampledTexture` (unit base + these).
pub mod tex {
    use super::*;
    pub const DATA: u32 = off!(SampledTexture, data);
    pub const DATA_LEN: u32 = off!(SampledTexture, data_len);
    pub const WIDTH: u32 = off!(SampledTexture, width);
    pub const HEIGHT: u32 = off!(SampledTexture, height);
    pub const FORMAT: u32 = off!(SampledTexture, format);
    pub const MIN_FILTER: u32 = off!(SampledTexture, min_filter);
    pub const MAG_FILTER: u32 = off!(SampledTexture, mag_filter);
    pub const WRAP_S: u32 = off!(SampledTexture, wrap_s);
    pub const WRAP_T: u32 = off!(SampledTexture, wrap_t);
    pub const ENABLED: u32 = off!(SampledTexture, enabled);
    pub const SIZE: u32 = core::mem::size_of::<SampledTexture>() as u32;
}

/// GL enums needed by the program builder (mirrors `gl` re-exports).
pub mod gl_const {
    pub const NEAREST: u32 = vantage_raster::gl::NEAREST;
    pub const LINEAR: u32 = vantage_raster::gl::LINEAR;
    pub const REPEAT: u32 = vantage_raster::gl::REPEAT;
    pub const MIRRORED_REPEAT: u32 = vantage_raster::gl::MIRRORED_REPEAT;
    pub const CLAMP_TO_EDGE: u32 = vantage_raster::gl::CLAMP_TO_EDGE;
    pub const RGBA: u32 = vantage_raster::gl::GL_RGBA;
    pub const LUMINANCE: u32 = vantage_raster::gl::GL_LUMINANCE;
    pub const LUMINANCE_ALPHA: u32 = vantage_raster::gl::GL_LUMINANCE_ALPHA;
}

/// Panics (debug + release) if the assumed ABI ever changes shape.
pub fn layout_asserts() {
    assert_eq!(varyings::SIZE, 40, "Varyings must stay 40 bytes");
    assert_eq!(state::TEXTURES, 0, "textures must lead FragState");
    assert_eq!(tex::DATA, 0, "data must lead SampledTexture");
    assert_eq!(tex::DATA_LEN, 8, "64-bit data_len after data ptr");
}

#[cfg(test)]
mod tests {
    #[test]
    fn abi_layout_pinned() {
        super::layout_asserts();
    }
}
