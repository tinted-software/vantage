//! EGL 1.4 API implementation and context/surface management.
#![allow(unused_imports, dead_code)]
use crate::display_list::DisplayListRegistry;
use crate::error::AngleWgpuError;
use crate::gl_context::GlContext;
use crate::glvnd::{current_native_platform, native_display_ptr, set_platform, NativePlatform};
use crate::renderer::WgpuRenderer;
use crate::sync::Mutex;
use crate::texture::TextureManager;
use crate::types::*;
use alloc::collections::BTreeMap as HashMap;
use alloc::sync::Arc;
use core::ffi::{c_void, CStr};
use core::future::Future;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, Win32WindowHandle, WindowHandle,
    WindowsDisplayHandle, XlibDisplayHandle, XlibWindowHandle,
};
#[derive(Clone, Copy, Debug)]
struct ForeignNativeWindow {
    kind: u32,
    display: *mut c_void,
    window: u64,
    screen: i32,
}

unsafe impl Send for ForeignNativeWindow {}
unsafe impl Sync for ForeignNativeWindow {}

impl HasDisplayHandle for ForeignNativeWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = match self.kind {
            ANGLE_WGPU_NATIVE_X11 => RawDisplayHandle::Xlib(XlibDisplayHandle::new(
                NonNull::new(self.display),
                self.screen,
            )),
            ANGLE_WGPU_NATIVE_WAYLAND => {
                let display = NonNull::new(self.display).ok_or(HandleError::Unavailable)?;
                RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display))
            }
            ANGLE_WGPU_NATIVE_WIN32 => RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
            _ => return Err(HandleError::NotSupported),
        };
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

impl HasWindowHandle for ForeignNativeWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = match self.kind {
            ANGLE_WGPU_NATIVE_X11 => RawWindowHandle::Xlib(XlibWindowHandle::new(self.window)),
            ANGLE_WGPU_NATIVE_WAYLAND => {
                let surface =
                    NonNull::new(self.window as *mut c_void).ok_or(HandleError::Unavailable)?;
                RawWindowHandle::Wayland(WaylandWindowHandle::new(surface))
            }
            ANGLE_WGPU_NATIVE_WIN32 => {
                let hwnd =
                    NonNull::new(self.window as *mut c_void).ok_or(HandleError::Unavailable)?;
                let hinstance = NonNull::new(self.display).ok_or(HandleError::Unavailable)?;
                let hwnd_nz: core::num::NonZeroIsize =
                    core::num::NonZeroIsize::new(hwnd.as_ptr() as isize)
                        .ok_or(HandleError::Unavailable)?;
                let hinst_nz: core::num::NonZeroIsize =
                    core::num::NonZeroIsize::new(hinstance.as_ptr() as isize)
                        .ok_or(HandleError::Unavailable)?;
                let mut handle = Win32WindowHandle::new(hwnd_nz);
                handle.hinstance = Some(hinst_nz);
                RawWindowHandle::Win32(handle)
            }
            _ => return Err(HandleError::NotSupported),
        };
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

pub fn block_on<F: Future>(mut future: F) -> F::Output {
    use core::pin::Pin;
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop_clone(_: *const ()) -> RawWaker {
        noop_raw_waker()
    }
    fn noop(_: *const ()) {}
    fn noop_raw_waker() -> RawWaker {
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        RawWaker::new(core::ptr::null(), &VTABLE)
    }

    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    let mut future = unsafe { Pin::new_unchecked(&mut future) };

    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(result) => return result,
            // no_std: no scheduler to yield to; busy-wait until another
            // thread completes the future.
            Poll::Pending => core::hint::spin_loop(),
        }
    }
}
pub struct EglSurfaceState {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub native_window: NativeWindowType,
    pub renderer: Option<Arc<Mutex<WgpuRenderer>>>,
}

pub struct EglContextState {
    pub id: u32,
    pub gl_context: Arc<Mutex<GlContext>>,
}

pub struct EglDisplayState {
    pub initialized: bool,
    pub surfaces: HashMap<u32, Arc<Mutex<EglSurfaceState>>>,
    pub contexts: HashMap<u32, Arc<Mutex<EglContextState>>>,
    pub shared_display_lists: Arc<DisplayListRegistry>,
    next_surface_id: AtomicU32,
    next_context_id: AtomicU32,
}

impl Default for EglDisplayState {
    fn default() -> Self {
        Self {
            initialized: false,
            surfaces: HashMap::new(),
            contexts: HashMap::new(),
            shared_display_lists: Arc::new(DisplayListRegistry::new()),
            next_surface_id: AtomicU32::new(1),
            next_context_id: AtomicU32::new(1),
        }
    }
}
impl EglDisplayState {
    pub fn allocate_surface_id(&self) -> u32 {
        self.next_surface_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn allocate_context_id(&self) -> u32 {
        self.next_context_id.fetch_add(1, Ordering::SeqCst)
    }
}

unsafe impl Send for EglSurfaceState {}
unsafe impl Sync for EglSurfaceState {}
unsafe impl Send for EglContextState {}
unsafe impl Sync for EglContextState {}
unsafe impl Send for EglDisplayState {}
unsafe impl Sync for EglDisplayState {}

// Global display singleton
static GLOBAL_DISPLAY: Mutex<Option<Arc<Mutex<EglDisplayState>>>> = Mutex::new(None);
static LAST_EGL_ERROR: Mutex<EGLint> = Mutex::new(EGL_SUCCESS);

// The game spawns chunk-rebuild worker threads that issue GL calls (via
// display-list compilation) without ever calling `eglMakeCurrent` on that
// thread themselves - real consoles this was ported from share one implicit
// GL context across threads. Desktop EGL is strictly per-thread-current, so
// without this fallback every GL call from those threads silently no-ops
// (see `with_context`), and displays lists built there are never actually
// created: `glCallList` on the render thread then calls empty/nonexistent
// lists, so terrain compiles but never draws. This app only ever creates a
// single GL context, so falling back to "whichever context was last made
// current on any thread" is safe and matches the game's actual usage.
static LAST_ACTIVE_CONTEXT: Mutex<Option<Arc<Mutex<GlContext>>>> = Mutex::new(None);

// no_std note: `std::thread_local!` is unavailable, and the target usage
// (a single implicit GL context shared across threads, as on the consoles
// this was ported from) makes globally-current state the correct semantics
// anyway. Both slots are plain lock-protected globals.
pub static CURRENT_CONTEXT: Mutex<Option<Arc<Mutex<GlContext>>>> = Mutex::new(None);
pub static CURRENT_SURFACE: Mutex<Option<Arc<Mutex<EglSurfaceState>>>> = Mutex::new(None);

pub fn get_current_gl_context() -> Option<Arc<Mutex<GlContext>>> {
    if let Some(ctx) = CURRENT_CONTEXT.lock().clone() {
        return Some(ctx);
    }
    LAST_ACTIVE_CONTEXT.lock().clone()
}

pub fn set_egl_error(err: EGLint) {
    *LAST_EGL_ERROR.lock() = err;
}

pub fn get_or_create_display() -> Arc<Mutex<EglDisplayState>> {
    let mut gd = GLOBAL_DISPLAY.lock();
    if gd.is_none() {
        *gd = Some(Arc::new(Mutex::new(EglDisplayState::default())));
    }
    gd.as_ref().unwrap().clone()
}

// ============================================================================
// EGL C API
// ============================================================================

pub unsafe fn egl_get_display(display_id: NativeDisplayType) -> EGLDisplay {
    // Direct-link clients (no glvnd) pass the native display handle here:
    // an X11 `Display*`/Wayland `wl_display*`, or EGL_DEFAULT_DISPLAY (null).
    // Record it so window-surface creation can build a real wgpu surface.
    let native = display_id as *mut c_void;
    if !native.is_null() || current_native_platform() == NativePlatform::None {
        // Platform 0 lets set_platform pick from the environment.
        set_platform(0, native);
    }
    let dpy = get_or_create_display();
    Arc::into_raw(dpy) as EGLDisplay
}

pub unsafe fn egl_initialize(
    dpy: EGLDisplay,
    major: *mut EGLint,
    minor: *mut EGLint,
) -> EGLBoolean {
    if dpy.is_null() {
        set_egl_error(EGL_BAD_DISPLAY);
        return EGL_FALSE;
    }

    let dpy_arc = Arc::from_raw(dpy as *const Mutex<EglDisplayState>);
    let res = {
        let mut d = dpy_arc.lock();
        d.initialized = true;
        if !major.is_null() {
            *major = 1;
        }
        if !minor.is_null() {
            *minor = 4;
        }
        EGL_TRUE
    };
    core::mem::forget(dpy_arc);
    res
}

pub unsafe fn egl_terminate(dpy: EGLDisplay) -> EGLBoolean {
    if dpy.is_null() {
        set_egl_error(EGL_BAD_DISPLAY);
        return EGL_FALSE;
    }

    let dpy_arc = Arc::from_raw(dpy as *const Mutex<EglDisplayState>);
    {
        let mut d = dpy_arc.lock();
        d.initialized = false;
        d.contexts.clear();
        d.surfaces.clear();
    }
    core::mem::forget(dpy_arc);
    EGL_TRUE
}

pub unsafe fn egl_get_configs(
    _dpy: EGLDisplay,
    configs: *mut EGLConfig,
    config_size: EGLint,
    num_config: *mut EGLint,
) -> EGLBoolean {
    if !num_config.is_null() {
        *num_config = 1;
    }
    if !configs.is_null() && config_size > 0 {
        *configs = 1 as EGLConfig;
    }
    EGL_TRUE
}

pub unsafe fn egl_choose_config(
    _dpy: EGLDisplay,
    attrib_list: *const EGLint,
    configs: *mut EGLConfig,
    config_size: EGLint,
    num_config: *mut EGLint,
) -> EGLBoolean {
    if num_config.is_null() {
        set_egl_error(EGL_BAD_PARAMETER);
        return EGL_FALSE;
    }

    // The single exported config. Values must stay consistent with
    // `egl_get_config_attrib`, which is the query side of the same contract.
    const CONFIG_ID: EGLint = 1;
    const SURFACE_TYPE_VAL: EGLint = EGL_WINDOW_BIT | EGL_PBUFFER_BIT;
    const RENDERABLE_TYPE_VAL: EGLint = EGL_OPENGL_ES_BIT | EGL_OPENGL_ES2_BIT;
    // Match kind: 0 = exact, 1 = at-least (config value >= requested),
    // 2 = bitmask (every requested bit set in the config's value).
    const CONFIG_ATTRIBS: &[(EGLint, EGLint, u8)] = &[
        (EGL_BUFFER_SIZE, 32, 1),
        (EGL_RED_SIZE, 8, 1),
        (EGL_GREEN_SIZE, 8, 1),
        (EGL_BLUE_SIZE, 8, 1),
        (EGL_ALPHA_SIZE, 8, 1),
        (EGL_DEPTH_SIZE, 24, 1),
        (EGL_STENCIL_SIZE, 8, 1),
        (EGL_SAMPLES, 0, 1),
        (EGL_SAMPLE_BUFFERS, 0, 1),
        (EGL_LUMINANCE_SIZE, 0, 1),
        (EGL_ALPHA_MASK_SIZE, 0, 1),
        (EGL_MAX_PBUFFER_WIDTH, 16384, 1),
        (EGL_MAX_PBUFFER_HEIGHT, 16384, 1),
        (EGL_MAX_PBUFFER_PIXELS, 16384 * 16384, 1),
        (EGL_LEVEL, 0, 0),
        (EGL_CONFIG_CAVEAT, EGL_NONE, 0),
        (EGL_CONFIG_ID, CONFIG_ID, 0),
        (EGL_COLOR_BUFFER_TYPE, EGL_RGB_BUFFER, 0),
        (EGL_NATIVE_RENDERABLE, EGL_FALSE as EGLint, 0),
        (EGL_NATIVE_VISUAL_ID, 0, 0),
        (EGL_NATIVE_VISUAL_TYPE, 0, 0),
        (EGL_BIND_TO_TEXTURE_RGB, EGL_TRUE as EGLint, 0),
        (EGL_BIND_TO_TEXTURE_RGBA, EGL_TRUE as EGLint, 0),
        (EGL_SURFACE_TYPE, SURFACE_TYPE_VAL, 2),
        (EGL_RENDERABLE_TYPE, RENDERABLE_TYPE_VAL, 2),
        (EGL_CONFORMANT, RENDERABLE_TYPE_VAL, 2),
    ];

    if !attrib_list.is_null() {
        let mut ptr = attrib_list;
        while *ptr != EGL_NONE {
            let attr = *ptr;
            let want = *ptr.add(1);
            ptr = ptr.add(2);
            if attr == EGL_DONT_CARE || want == EGL_DONT_CARE {
                continue;
            }
            let Some(&(_, have, kind)) = CONFIG_ATTRIBS.iter().find(|&&(a, _, _)| a == attr) else {
                set_egl_error(EGL_BAD_ATTRIBUTE);
                *num_config = 0;
                return EGL_FALSE;
            };
            let ok = match kind {
                1 => have >= want,
                2 => (have & want) == want,
                _ => have == want,
            };
            if !ok {
                *num_config = 0;
                return EGL_TRUE;
            }
        }
    }

    *num_config = 1;
    if !configs.is_null() && config_size > 0 {
        *configs = CONFIG_ID as EGLConfig;
    }
    EGL_TRUE
}
static STRING_VENDOR: &[u8] = b"angle_wgpu\0";
static STRING_VERSION: &[u8] = b"1.4 angle_wgpu EGL 1.4\0";
static STRING_CLIENT_APIS: &[u8] = b"OpenGL_ES \0";
static STRING_EXTENSIONS: &[u8] = b"EGL_KHR_create_context EGL_KHR_surfaceless_context \
EGL_KHR_image_base EGL_KHR_fence_sync EGL_KHR_wait_sync EGL_KHR_get_all_proc_addresses\0";
static CLIENT_EXTENSIONS: &[u8] = b"EGL_EXT_platform_base EGL_EXT_platform_x11 \
EGL_EXT_platform_wayland EGL_MESA_platform_surfaceless \
EGL_KHR_client_get_all_proc_addresses\0";

/// Returns a static NUL-terminated string, or null for unknown queries.
pub unsafe fn egl_query_string(dpy: EGLDisplay, name: EGLint) -> *const core::ffi::c_char {
    let bytes: &[u8] = match name {
        EGL_VENDOR => STRING_VENDOR,
        EGL_VERSION => STRING_VERSION,
        EGL_CLIENT_APIS => STRING_CLIENT_APIS,
        // Client extensions are display-independent.
        EGL_EXTENSIONS if dpy.is_null() => CLIENT_EXTENSIONS,
        EGL_EXTENSIONS => STRING_EXTENSIONS,
        _ => return core::ptr::null(),
    };
    bytes.as_ptr() as *const core::ffi::c_char
}

pub unsafe fn egl_get_config_attrib(
    _dpy: EGLDisplay,
    _config: EGLConfig,
    attribute: EGLint,
    value: *mut EGLint,
) -> EGLBoolean {
    if value.is_null() {
        return EGL_FALSE;
    }

    match attribute {
        EGL_BUFFER_SIZE => *value = 32,
        EGL_RED_SIZE => *value = 8,
        EGL_GREEN_SIZE => *value = 8,
        EGL_BLUE_SIZE => *value = 8,
        EGL_ALPHA_SIZE => *value = 8,
        EGL_DEPTH_SIZE => *value = 24,
        EGL_STENCIL_SIZE => *value = 8,
        EGL_SURFACE_TYPE => *value = EGL_WINDOW_BIT | EGL_PBUFFER_BIT,
        EGL_RENDERABLE_TYPE => *value = EGL_OPENGL_ES_BIT | EGL_OPENGL_ES2_BIT,
        EGL_NATIVE_VISUAL_ID => *value = x11_default_visual_id(),
        _ => *value = 0,
    }
    EGL_TRUE
}

/// Best-effort XVisualID of the default visual for the current native X11
/// display, resolved through libX11 at runtime (no hard dependency).
/// Returns 0 when unavailable or not running on X11.
fn x11_default_visual_id() -> EGLint {
    if current_native_platform() != NativePlatform::X11 {
        return 0;
    }
    let dpy = native_display_ptr();
    if dpy.is_null() {
        return 0;
    }

    const RTLD_LAZY: i32 = 1;
    let lib = unsafe { libloading::Library::new("libX11.so.6") };
    let lib = match lib {
        Ok(l) => l,
        Err(_) => return 0,
    };

    type XDefaultScreenFn = unsafe extern "C" fn(*mut c_void) -> i32;
    type XDefaultVisualFn = unsafe extern "C" fn(*mut c_void, i32) -> *mut c_void;

    unsafe {
        let screen: libloading::Symbol<XDefaultScreenFn> = match lib.get(b"XDefaultScreen\0") {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let visual_fn: libloading::Symbol<XDefaultVisualFn> = match lib.get(b"XDefaultVisual\0") {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let vis = visual_fn(dpy, screen(dpy));
        if vis.is_null() {
            return 0;
        }
        // struct Visual { XExtData *ext_data; VisualID visualid; ... }
        // visualid is the member right after the ext_data pointer.
        *(vis as *const usize).add(1) as EGLint
    }
}
pub unsafe fn egl_create_window_surface(
    dpy: EGLDisplay,
    _config: EGLConfig,
    win: NativeWindowType,
    attrib_list: *const EGLint,
) -> EGLSurface {
    let mut width = 1280;
    let mut height = 720;

    if !attrib_list.is_null() {
        let mut ptr = attrib_list;
        while *ptr != EGL_NONE {
            let attr = *ptr;
            let val = *ptr.add(1);
            if attr == EGL_WIDTH {
                width = val.max(1) as u32;
            } else if attr == EGL_HEIGHT {
                height = val.max(1) as u32;
            }
            ptr = ptr.add(2);
        }
    }

    let dpy_arc = if !dpy.is_null() {
        Arc::from_raw(dpy as *const Mutex<EglDisplayState>)
    } else {
        get_or_create_display()
    };

    let id = {
        let d = dpy_arc.lock();
        d.next_surface_id.fetch_add(1, Ordering::SeqCst)
    };

    // A real on-screen window: when the display came through glvnd we know
    // the window system (and its Display*/wl_surface*) from
    // `getPlatformDisplay`, so build a wgpu surface that actually presents.
    // Without that context (e.g. the pbuffer path passes win == null), fall
    // back to a headless offscreen renderer.
    let renderer = if (win as usize) != 0 && native_display_ptr() != core::ptr::null_mut() {
        let kind = match current_native_platform() {
            NativePlatform::X11 => ANGLE_WGPU_NATIVE_X11,
            NativePlatform::Wayland => ANGLE_WGPU_NATIVE_WAYLAND,
            NativePlatform::None => 0,
        };
        let foreign = ForeignNativeWindow {
            kind,
            display: native_display_ptr(),
            window: win as u64,
            screen: 0,
        };
        match renderer_from_native_window(foreign, width.max(1), height.max(1)) {
            Ok(r) => r,
            Err(_e) => {
                set_egl_error(EGL_BAD_ALLOC);
                return EGL_NO_SURFACE;
            }
        }
    } else {
        // renderer nothing would ever present. The error is retrievable via
        // `eglGetError` (EGL_BAD_ALLOC).
        match block_on(WgpuRenderer::new_headless(width, height)) {
            Ok(r) => Arc::new(Mutex::new(r)),
            Err(_e) => {
                set_egl_error(EGL_BAD_ALLOC);
                return EGL_NO_SURFACE;
            }
        }
    };

    let surface_state = Arc::new(Mutex::new(EglSurfaceState {
        id,
        width,
        height,
        native_window: win,
        renderer: Some(renderer),
    }));

    {
        let mut d = dpy_arc.lock();
        d.surfaces.insert(id, surface_state.clone());
    }

    if !dpy.is_null() {
        core::mem::forget(dpy_arc);
    }

    Arc::into_raw(surface_state) as EGLSurface
}

fn renderer_from_native_window(
    native: ForeignNativeWindow,
    width: u32,
    height: u32,
) -> Result<Arc<Mutex<WgpuRenderer>>, AngleWgpuError> {
    use alloc::boxed::Box;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
        Box::new(native),
    ));

    let surface = instance
        .create_surface(native)
        .map_err(AngleWgpuError::CreateSurface)?;

    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(AngleWgpuError::RequestAdapter)?;

    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("angle_wgpu Native Window Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::default(),
        default_queue: wgpu::QueueDescriptor::default(),
    }))
    .map_err(AngleWgpuError::RequestDevice)?;

    let renderer = WgpuRenderer::new_with_surface(
        instance,
        adapter,
        device,
        queue,
        surface,
        width.max(1),
        height.max(1),
    )?;

    Ok(Arc::new(Mutex::new(renderer)))
}

pub unsafe fn egl_create_native_window_surface(
    dpy: EGLDisplay,
    native: *const AngleWgpuNativeWindow,
) -> EGLSurface {
    if native.is_null() {
        set_egl_error(EGL_BAD_NATIVE_WINDOW);
        return EGL_NO_SURFACE;
    }
    let n = *native;
    if n.kind != ANGLE_WGPU_NATIVE_X11
        && n.kind != ANGLE_WGPU_NATIVE_WAYLAND
        && n.kind != ANGLE_WGPU_NATIVE_WIN32
    {
        set_egl_error(EGL_BAD_PARAMETER);
        return EGL_NO_SURFACE;
    }

    // SDL demo originally left width/height at 0, which made the renderer
    // configure a 1x1 swapchain: the 800x600 X11 window then composites as
    // mostly transparent until a resize reconfigures it to the real size.
    // Fall back to the window's actual size-derived default when the caller
    // did not provide one, so the first frame already matches the window.
    let width = if n.width == 0 { 800 } else { n.width }.max(1);
    let height = if n.height == 0 { 600 } else { n.height }.max(1);
    let foreign = ForeignNativeWindow {
        kind: n.kind,
        display: n.display,
        window: n.window,
        screen: n.screen,
    };
    let renderer = match renderer_from_native_window(foreign, width, height) {
        Ok(r) => r,
        // Surface creation failures are surfaced to the caller via the EGL
        // error state (retrievable with `eglGetError`); the underlying
        // `AngleWgpuError` carries the wgpu-level details.
        Err(_e) => {
            set_egl_error(EGL_BAD_NATIVE_WINDOW);
            return EGL_NO_SURFACE;
        }
    };

    let dpy_arc = if !dpy.is_null() {
        Arc::from_raw(dpy as *const Mutex<EglDisplayState>)
    } else {
        get_or_create_display()
    };

    let id = {
        let d = dpy_arc.lock();
        d.next_surface_id.fetch_add(1, Ordering::SeqCst)
    };

    let surface_state = Arc::new(Mutex::new(EglSurfaceState {
        id,
        width,
        height,
        native_window: n.window as NativeWindowType,
        renderer: Some(renderer),
    }));

    {
        let mut d = dpy_arc.lock();
        d.surfaces.insert(id, surface_state.clone());
    }

    if !dpy.is_null() {
        core::mem::forget(dpy_arc);
    }

    Arc::into_raw(surface_state) as EGLSurface
}

pub unsafe fn egl_create_pbuffer_surface(
    dpy: EGLDisplay,
    _config: EGLConfig,
    attrib_list: *const EGLint,
) -> EGLSurface {
    egl_create_window_surface(dpy, _config, core::mem::zeroed(), attrib_list)
}

pub unsafe fn egl_destroy_surface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean {
    if surface.is_null() {
        return EGL_TRUE;
    }
    let surf_arc = Arc::from_raw(surface as *const Mutex<EglSurfaceState>);
    let id = surf_arc.lock().id;

    if !dpy.is_null() {
        let dpy_arc = Arc::from_raw(dpy as *const Mutex<EglDisplayState>);
        dpy_arc.lock().surfaces.remove(&id);
        core::mem::forget(dpy_arc);
    }

    EGL_TRUE
}

pub unsafe fn egl_create_context(
    dpy: EGLDisplay,
    _config: EGLConfig,
    _share_context: EGLContext,
    _attrib_list: *const EGLint,
) -> EGLContext {
    let dpy_arc = if !dpy.is_null() {
        Arc::from_raw(dpy as *const Mutex<EglDisplayState>)
    } else {
        get_or_create_display()
    };

    let (id, shared_lists) = {
        let d = dpy_arc.lock();
        (
            d.next_context_id.fetch_add(1, Ordering::SeqCst),
            d.shared_display_lists.clone(),
        )
    };

    let gl_ctx = Arc::new(Mutex::new(GlContext::new(
        id,
        None,
        TextureManager::new(),
        shared_lists,
    )));
    let ctx_state = Arc::new(Mutex::new(EglContextState {
        id,
        gl_context: gl_ctx,
    }));

    {
        let mut d = dpy_arc.lock();
        d.contexts.insert(id, ctx_state.clone());
    }

    if !dpy.is_null() {
        core::mem::forget(dpy_arc);
    }

    Arc::into_raw(ctx_state) as EGLContext
}

pub unsafe fn egl_destroy_context(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean {
    if ctx.is_null() {
        return EGL_TRUE;
    }
    let ctx_arc = Arc::from_raw(ctx as *const Mutex<EglContextState>);
    let id = ctx_arc.lock().id;

    if !dpy.is_null() {
        let dpy_arc = Arc::from_raw(dpy as *const Mutex<EglDisplayState>);
        dpy_arc.lock().contexts.remove(&id);
        core::mem::forget(dpy_arc);
    }

    EGL_TRUE
}

pub unsafe fn egl_make_current(
    _dpy: EGLDisplay,
    draw: EGLSurface,
    _read: EGLSurface,
    ctx: EGLContext,
) -> EGLBoolean {
    if ctx.is_null() {
        *CURRENT_CONTEXT.lock() = None;
        *CURRENT_SURFACE.lock() = None;
        return EGL_TRUE;
    }

    let ctx_arc = Arc::from_raw(ctx as *const Mutex<EglContextState>);
    let gl_ctx = ctx_arc.lock().gl_context.clone();
    core::mem::forget(ctx_arc);

    if !draw.is_null() {
        let surf_arc = Arc::from_raw(draw as *const Mutex<EglSurfaceState>);
        let (w, h, renderer) = {
            let s = surf_arc.lock();
            (s.width, s.height, s.renderer.clone())
        };
        core::mem::forget(surf_arc.clone());

        {
            let mut gl = gl_ctx.lock();
            gl.renderer = renderer;
            gl.viewport = (0, 0, w as i32, h as i32);
            gl.scissor = (0, 0, w as i32, h as i32);
        }

        *CURRENT_SURFACE.lock() = Some(surf_arc);
    }

    *LAST_ACTIVE_CONTEXT.lock() = Some(gl_ctx.clone());
    *CURRENT_CONTEXT.lock() = Some(gl_ctx);
    EGL_TRUE
}

pub unsafe fn egl_get_current_context() -> EGLContext {
    let cur = CURRENT_CONTEXT.lock();
    match cur.as_ref() {
        Some(ctx) => Arc::as_ptr(ctx) as EGLContext,
        None => EGL_NO_CONTEXT,
    }
}

pub unsafe fn egl_get_current_surface(_readdraw: EGLint) -> EGLSurface {
    let cur = CURRENT_SURFACE.lock();
    match cur.as_ref() {
        Some(surf) => Arc::as_ptr(surf) as EGLSurface,
        None => EGL_NO_SURFACE,
    }
}

pub unsafe fn egl_get_current_display() -> EGLDisplay {
    let dpy = get_or_create_display();
    Arc::as_ptr(&dpy) as EGLDisplay
}

pub unsafe fn egl_query_surface(
    _dpy: EGLDisplay,
    surface: EGLSurface,
    attribute: EGLint,
    value: *mut EGLint,
) -> EGLBoolean {
    if surface.is_null() || value.is_null() {
        return EGL_FALSE;
    }

    let surf_arc = Arc::from_raw(surface as *const Mutex<EglSurfaceState>);
    {
        let s = surf_arc.lock();
        match attribute {
            EGL_WIDTH => *value = s.width as EGLint,
            EGL_HEIGHT => *value = s.height as EGLint,
            _ => *value = 0,
        }
    }
    core::mem::forget(surf_arc);
    EGL_TRUE
}

pub unsafe fn egl_swap_buffers(_dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean {
    if surface.is_null() {
        return EGL_FALSE;
    }

    let surf_arc = Arc::from_raw(surface as *const Mutex<EglSurfaceState>);
    let renderer = surf_arc.lock().renderer.clone();
    core::mem::forget(surf_arc);

    if let Some(r) = renderer {
        let mut rend = r.lock();
        if rend.swap_buffers().is_err() {
            set_egl_error(EGL_BAD_ACCESS);
            return EGL_FALSE;
        }
    }
    EGL_TRUE
}

pub unsafe fn egl_resize_surface(surface: EGLSurface, width: u32, height: u32) -> EGLBoolean {
    if surface.is_null() {
        return EGL_FALSE;
    }
    let w = width.max(1);
    let h = height.max(1);
    let surf_arc = Arc::from_raw(surface as *const Mutex<EglSurfaceState>);
    {
        let mut s = surf_arc.lock();
        s.width = w;
        s.height = h;
        if let Some(r) = &s.renderer {
            r.lock().resize(w, h);
        }
    }

    let is_current = CURRENT_SURFACE
        .lock()
        .as_ref()
        .map(|s| Arc::ptr_eq(s, &surf_arc))
        .unwrap_or(false);
    if is_current {
        if let Some(ctx) = CURRENT_CONTEXT.lock().as_ref() {
            let mut gl = ctx.lock();
            gl.viewport = (0, 0, w as i32, h as i32);
            gl.scissor = (0, 0, w as i32, h as i32);
        }
    }

    core::mem::forget(surf_arc);
    EGL_TRUE
}

pub unsafe fn egl_swap_interval(_dpy: EGLDisplay, interval: EGLint) -> EGLBoolean {
    if interval != 0 && interval != 1 {
        set_egl_error(EGL_BAD_PARAMETER);
        return EGL_FALSE;
    }

    let Some(surface) = CURRENT_SURFACE.lock().clone() else {
        set_egl_error(EGL_BAD_SURFACE);
        return EGL_FALSE;
    };
    let Some(renderer) = surface.lock().renderer.clone() else {
        set_egl_error(EGL_BAD_SURFACE);
        return EGL_FALSE;
    };
    renderer.lock().set_swap_interval(interval);
    EGL_TRUE
}

pub unsafe fn egl_get_error() -> EGLint {
    let mut err = LAST_EGL_ERROR.lock();
    let res = *err;
    *err = EGL_SUCCESS;
    res
}

pub unsafe fn egl_get_proc_address(
    procname: *const core::ffi::c_char,
) -> __eglMustCastToProperFunctionPointerType {
    if procname.is_null() {
        return None;
    }
    let Ok(name) = CStr::from_ptr(procname).to_str() else {
        return None;
    };

    crate::get_gl_proc_address(name)
}
