//! Error type shared by the renderer / EGL layers.
//!
//! Replaces ad-hoc `String` errors and `eprintln!` diagnostics: failures are
//! surfaced as typed values which callers translate into EGL/GL error codes
//! at the FFI boundary (`eglGetError` / `glGetError`).

use core::fmt;

/// Errors produced by renderer and surface creation/present paths.
#[derive(Debug)]
pub enum AngleWgpuError {
    /// Failed to create a `wgpu` surface from the given native window.
    CreateSurface(wgpu::CreateSurfaceError),
    /// No compatible adapter was found.
    RequestAdapter(wgpu::RequestAdapterError),
    /// Logical device creation failed.
    RequestDevice(wgpu::RequestDeviceError),
    /// No swapchain format available for the surface.
    NoSurfaceFormats,
    /// Acquiring the next swapchain texture failed.
    SurfaceAcquire(&'static str),
    /// Renderer has no surface configured for this operation.
    NoSurface,
}

impl fmt::Display for AngleWgpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AngleWgpuError::CreateSurface(e) => write!(f, "create_surface failed: {e}"),
            AngleWgpuError::RequestAdapter(e) => write!(f, "request_adapter failed: {e:?}"),
            AngleWgpuError::RequestDevice(e) => write!(f, "request_device failed: {e}"),
            AngleWgpuError::NoSurfaceFormats => {
                write!(f, "surface reported no supported formats")
            }
            AngleWgpuError::SurfaceAcquire(msg) => {
                write!(f, "surface texture acquire failed: {msg}")
            }
            AngleWgpuError::NoSurface => write!(f, "no surface configured"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for AngleWgpuError {}

impl From<wgpu::CreateSurfaceError> for AngleWgpuError {
    fn from(e: wgpu::CreateSurfaceError) -> Self {
        Self::CreateSurface(e)
    }
}

impl From<wgpu::RequestAdapterError> for AngleWgpuError {
    fn from(e: wgpu::RequestAdapterError) -> Self {
        Self::RequestAdapter(e)
    }
}

impl From<wgpu::RequestDeviceError> for AngleWgpuError {
    fn from(e: wgpu::RequestDeviceError) -> Self {
        Self::RequestDevice(e)
    }
}
