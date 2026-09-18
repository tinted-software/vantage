//! Vantage EGL 1.4 implementation and C ABI exports.
//!
//! This is the only crate in the workspace with a C ABI (`cdylib`/`staticlib`
//! + cargo-c capi metadata); it links the GLES entry points from
//! `vantage-gles` and the EGL logic here into `libvantage.so`.
#![allow(non_snake_case, non_camel_case_types)]
#![no_std]
extern crate alloc;

pub mod egl;
pub mod gl_reexports;
pub mod glvnd;
pub mod platform;

pub use crate::egl::*;
pub use vantage_gles::types::*;

use core::ffi::{c_char, c_void};
pub use vantage_gles::*;

/// Freestanding support (no `std`): the cdylib still needs a panic handler
/// and a global allocator to link. A fixed-heap bump allocator is provided
/// as the baseline; real freestanding deployments link their own.
// MISSING: freestanding allocator policy — this is a compile-enable baseline.
#[cfg(not(feature = "std"))]
pub mod freestanding {
    extern crate alloc;
    use alloc::alloc::Layout;
    use core::alloc::GlobalAlloc;
    use core::sync::atomic::{AtomicUsize, Ordering};

    const HEAP_SIZE: usize = 16 * 1024 * 1024;
    static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
    static OFFSET: AtomicUsize = AtomicUsize::new(0);

    pub struct Bump;
    unsafe impl GlobalAlloc for Bump {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let align = layout.align().max(1);
            let mut off = OFFSET.load(Ordering::Relaxed);
            let base = (&raw const HEAP) as usize;
            loop {
                let aligned = (base + off + align - 1) & !(align - 1);
                let next = aligned - base + layout.size();
                if next > HEAP_SIZE {
                    return core::ptr::null_mut();
                }
                if OFFSET
                    .compare_exchange_weak(off, next, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    return aligned as *mut u8;
                }
                off = OFFSET.load(Ordering::Relaxed);
            }
        }
        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
    }

    #[global_allocator]
    static A: Bump = Bump;

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        loop {
            core::hint::spin_loop();
        }
    }
}

// ============================================================================
// EGL Exports
// ============================================================================

// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn eglGetDisplay(display_id: NativeDisplayType) -> EGLDisplay {
    egl_get_display(display_id)
}

#[no_mangle]
pub unsafe extern "C" fn eglInitialize(
    dpy: EGLDisplay,
    major: *mut EGLint,
    minor: *mut EGLint,
) -> EGLBoolean {
    egl_initialize(dpy, major, minor)
}

#[no_mangle]
pub unsafe extern "C" fn eglTerminate(dpy: EGLDisplay) -> EGLBoolean {
    egl_terminate(dpy)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetConfigs(
    dpy: EGLDisplay,
    configs: *mut EGLConfig,
    config_size: EGLint,
    num_config: *mut EGLint,
) -> EGLBoolean {
    egl_get_configs(dpy, configs, config_size, num_config)
}

#[no_mangle]
pub unsafe extern "C" fn eglChooseConfig(
    dpy: EGLDisplay,
    attrib_list: *const EGLint,
    configs: *mut EGLConfig,
    config_size: EGLint,
    num_config: *mut EGLint,
) -> EGLBoolean {
    egl_choose_config(dpy, attrib_list, configs, config_size, num_config)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetConfigAttrib(
    dpy: EGLDisplay,
    config: EGLConfig,
    attribute: EGLint,
    value: *mut EGLint,
) -> EGLBoolean {
    egl_get_config_attrib(dpy, config, attribute, value)
}

#[no_mangle]
pub unsafe extern "C" fn eglCreateWindowSurface(
    dpy: EGLDisplay,
    config: EGLConfig,
    win: NativeWindowType,
    attrib_list: *const EGLint,
) -> EGLSurface {
    egl_create_window_surface(dpy, config, win, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn angle_wgpu_create_native_window_surface(
    dpy: EGLDisplay,
    _config: EGLConfig,
    native: *const AngleWgpuNativeWindow,
) -> EGLSurface {
    egl_create_native_window_surface(dpy, native)
}

#[no_mangle]
pub unsafe extern "C" fn eglCreatePixmapSurface(
    _dpy: EGLDisplay,
    _config: EGLConfig,
    _pixmap: NativePixmapType,
    _attrib_list: *const EGLint,
) -> EGLSurface {
    egl::set_egl_error(EGL_BAD_PARAMETER);
    EGL_NO_SURFACE
}

#[no_mangle]
pub unsafe extern "C" fn eglBindAPI(_api: EGLenum) -> EGLBoolean {
    EGL_TRUE
}

#[no_mangle]
pub unsafe extern "C" fn eglCreatePbufferSurface(
    dpy: EGLDisplay,
    config: EGLConfig,
    attrib_list: *const EGLint,
) -> EGLSurface {
    egl_create_pbuffer_surface(dpy, config, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean {
    egl_destroy_surface(dpy, surface)
}

#[no_mangle]
pub unsafe extern "C" fn eglCreateContext(
    dpy: EGLDisplay,
    config: EGLConfig,
    share_context: EGLContext,
    attrib_list: *const EGLint,
) -> EGLContext {
    egl_create_context(dpy, config, share_context, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean {
    egl_destroy_context(dpy, ctx)
}

#[no_mangle]
pub unsafe extern "C" fn eglMakeCurrent(
    dpy: EGLDisplay,
    draw: EGLSurface,
    read: EGLSurface,
    ctx: EGLContext,
) -> EGLBoolean {
    egl_make_current(dpy, draw, read, ctx)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetCurrentContext() -> EGLContext {
    egl_get_current_context()
}

#[no_mangle]
pub unsafe extern "C" fn eglGetCurrentSurface(readdraw: EGLint) -> EGLSurface {
    egl_get_current_surface(readdraw)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetCurrentDisplay() -> EGLDisplay {
    egl_get_current_display()
}

#[no_mangle]
pub unsafe extern "C" fn eglQuerySurface(
    dpy: EGLDisplay,
    surface: EGLSurface,
    attribute: EGLint,
    value: *mut EGLint,
) -> EGLBoolean {
    egl_query_surface(dpy, surface, attribute, value)
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapBuffers(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean {
    egl_swap_buffers(dpy, surface)
}

#[no_mangle]
pub unsafe extern "C" fn angle_wgpu_resize_surface(
    surface: EGLSurface,
    width: u32,
    height: u32,
) -> EGLBoolean {
    egl_resize_surface(surface, width, height)
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapInterval(dpy: EGLDisplay, interval: EGLint) -> EGLBoolean {
    egl_swap_interval(dpy, interval)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetError() -> EGLint {
    egl_get_error()
}

#[no_mangle]
pub unsafe extern "C" fn eglQueryString(dpy: EGLDisplay, name: EGLint) -> *const c_char {
    egl_query_string(dpy, name)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetProcAddress(
    procname: *const c_char,
) -> __eglMustCastToProperFunctionPointerType {
    egl_get_proc_address(procname)
}

pub fn get_gl_proc_address(name: &str) -> __eglMustCastToProperFunctionPointerType {
    let ptr = match name {
        "glMatrixMode" => glMatrixMode as *const (),
        "glLoadIdentity" => glLoadIdentity as *const (),
        "glPushMatrix" => glPushMatrix as *const (),
        "glPopMatrix" => glPopMatrix as *const (),
        "glTranslatef" => glTranslatef as *const (),
        "glRotatef" => glRotatef as *const (),
        "glScalef" => glScalef as *const (),
        "glOrtho" => glOrtho as *const (),
        "glOrthof" => glOrthof as *const (),
        "glFrustum" => glFrustum as *const (),
        "glFrustumf" => glFrustumf as *const (),
        "glMultMatrixf" => glMultMatrixf as *const (),
        "glLoadMatrixf" => glLoadMatrixf as *const (),
        "glEnableClientState" => glEnableClientState as *const (),
        "glDisableClientState" => glDisableClientState as *const (),
        "glVertexPointer" => glVertexPointer as *const (),
        "glTexCoordPointer" => glTexCoordPointer as *const (),
        "glColorPointer" => glColorPointer as *const (),
        "glNormalPointer" => glNormalPointer as *const (),
        "glClientActiveTexture" => glClientActiveTexture as *const (),
        "glDrawArrays" => glDrawArrays as *const (),
        "glDrawElements" => glDrawElements as *const (),
        "glBegin" => glBegin as *const (),
        "glEnd" => glEnd as *const (),
        "glVertex3f" => glVertex3f as *const (),
        "glVertex2f" => glVertex2f as *const (),
        "glTexCoord2f" => glTexCoord2f as *const (),
        "glColor4f" => glColor4f as *const (),
        "glColor3f" => glColor3f as *const (),
        "glColor4ub" => glColor4ub as *const (),
        "glNormal3f" => glNormal3f as *const (),
        "glGenLists" => glGenLists as *const (),
        "glDeleteLists" => glDeleteLists as *const (),
        "glNewList" => glNewList as *const (),
        "glEndList" => glEndList as *const (),
        "glCallList" => glCallList as *const (),
        "glCallLists" => glCallLists as *const (),
        "glGenTextures" => glGenTextures as *const (),
        "glDeleteTextures" => glDeleteTextures as *const (),
        "glBindTexture" => glBindTexture as *const (),
        "glTexImage2D" => glTexImage2D as *const (),
        "glTexSubImage2D" => glTexSubImage2D as *const (),
        "glTexParameteri" => glTexParameteri as *const (),
        "glTexParameterf" => glTexParameterf as *const (),
        "glActiveTexture" => glActiveTexture as *const (),
        "glEnable" => glEnable as *const (),
        "glDisable" => glDisable as *const (),
        "glIsEnabled" => glIsEnabled as *const (),
        "glAlphaFunc" => glAlphaFunc as *const (),
        "glBlendFunc" => glBlendFunc as *const (),
        "glBlendColor" => glBlendColor as *const (),
        "glDepthFunc" => glDepthFunc as *const (),
        "glDepthMask" => glDepthMask as *const (),
        "glColorMask" => glColorMask as *const (),
        "glCullFace" => glCullFace as *const (),
        "glFrontFace" => glFrontFace as *const (),
        "glPolygonOffset" => glPolygonOffset as *const (),
        "glLineWidth" => glLineWidth as *const (),
        "glPointSize" => glPointSize as *const (),
        "glShadeModel" => glShadeModel as *const (),
        "glFogf" => glFogf as *const (),
        "glFogfv" => glFogfv as *const (),
        "glFogi" => glFogi as *const (),
        "glFogx" => glFogx as *const (),
        "glFogxv" => glFogxv as *const (),
        "glHint" => glHint as *const (),
        "glDepthRangef" => glDepthRangef as *const (),
        "glDepthRange" => glDepthRange as *const (),
        "glDepthRangex" => glDepthRangex as *const (),
        "glLightf" => glLightf as *const (),
        "glLightfv" => glLightfv as *const (),
        "glLightModelfv" => glLightModelfv as *const (),
        "glStencilFunc" => glStencilFunc as *const (),
        "glStencilMask" => glStencilMask as *const (),
        "glStencilOp" => glStencilOp as *const (),
        "glViewport" => glViewport as *const (),
        "glScissor" => glScissor as *const (),
        "glClearColor" => glClearColor as *const (),
        "glClearDepthf" => glClearDepthf as *const (),
        "glClear" => glClear as *const (),
        "glReadPixels" => glReadPixels as *const (),
        "glFlush" => glFlush as *const (),
        "glFinish" => glFinish as *const (),
        "glGetIntegerv" => glGetIntegerv as *const (),
        "glGetFloatv" => glGetFloatv as *const (),
        "glGetBooleanv" => glGetBooleanv as *const (),
        "glGetString" => glGetString as *const (),
        "glGetError" => glGetError as *const (),
        "glCreateShader" => glCreateShader as *const (),
        "glShaderSource" => glShaderSource as *const (),
        "glCompileShader" => glCompileShader as *const (),
        "glGetShaderiv" => glGetShaderiv as *const (),
        "glDeleteShader" => glDeleteShader as *const (),
        "glCreateProgram" => glCreateProgram as *const (),
        "glAttachShader" => glAttachShader as *const (),
        "glLinkProgram" => glLinkProgram as *const (),
        "glGetProgramiv" => glGetProgramiv as *const (),
        "glUseProgram" => glUseProgram as *const (),
        "glDeleteProgram" => glDeleteProgram as *const (),
        "glGenBuffers" => glGenBuffers as *const (),
        "glBindBuffer" => glBindBuffer as *const (),
        "glBufferData" => glBufferData as *const (),
        "glDeleteBuffers" => glDeleteBuffers as *const (),
        "glGenFramebuffers" => glGenFramebuffers as *const (),
        "glBindFramebuffer" => glBindFramebuffer as *const (),
        "glFramebufferTexture2D" => glFramebufferTexture2D as *const (),
        "glDeleteFramebuffers" => glDeleteFramebuffers as *const (),
        "eglGetDisplay" => eglGetDisplay as *const (),
        "eglInitialize" => eglInitialize as *const (),
        "eglTerminate" => eglTerminate as *const (),
        "eglChooseConfig" => eglChooseConfig as *const (),
        "eglCreateWindowSurface" => eglCreateWindowSurface as *const (),
        "eglCreatePixmapSurface" => eglCreatePixmapSurface as *const (),
        "eglCreateContext" => eglCreateContext as *const (),
        "eglMakeCurrent" => eglMakeCurrent as *const (),
        "eglSwapInterval" => eglSwapInterval as *const (),
        "eglQueryString" => eglQueryString as *const (),
        "eglGetProcAddress" => eglGetProcAddress as *const (),
        _ => core::ptr::null(),
    };

    if ptr.is_null() {
        None
    } else {
        Some(unsafe { core::mem::transmute(ptr) })
    }
}

#[used]
static GLES_EXPORT_KEEP: [unsafe extern "C" fn(); 1] =
    [unsafe { core::mem::transmute(vantage_gles::glMatrixMode as *const ()) }];
