//! X11 MIT-SHM presentation backend for Vantage EGL.
#![allow(non_upper_case_globals, non_camel_case_types, dead_code)]

#[cfg(all(feature = "std", target_os = "linux"))]
pub mod x11_shm {
    use core::ffi::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
    use core::ptr::NonNull;

    pub type Display = c_void;
    pub type Window = c_ulong;
    pub type VisualID = c_ulong;
    pub type GC = *mut c_void;
    pub type Bool = c_int;
    pub type Status = c_int;

    pub const ZPixmap: c_int = 2;
    pub const IPC_CREAT: c_int = 0o1000;
    pub const IPC_RMID: c_int = 0;

    #[repr(C)]
    pub struct XShmSegmentInfo {
        pub shmseg: c_ulong,
        pub shmid: c_int,
        pub shmaddr: *mut c_char,
        pub readOnly: Bool,
    }

    #[repr(C)]
    pub struct XImage {
        pub width: c_int,
        pub height: c_int,
        pub xoffset: c_int,
        pub format: c_int,
        pub data: *mut c_char,
        pub byte_order: c_int,
        pub bitmap_unit: c_int,
        pub bitmap_bit_order: c_int,
        pub bitmap_pad: c_int,
        pub depth: c_int,
        pub bytes_per_line: c_int,
        pub bits_per_pixel: c_int,
        pub red_mask: c_ulong,
        pub green_mask: c_ulong,
        pub blue_mask: c_ulong,
        pub obdata: *mut c_char,
        // function pointers omitted
    }

    #[link(name = "X11")]
    extern "C" {
        pub fn XOpenDisplay(name: *const c_char) -> *mut Display;
        pub fn XCloseDisplay(dpy: *mut Display) -> c_int;
        pub fn XDefaultScreen(dpy: *mut Display) -> c_int;
        pub fn XDefaultVisual(dpy: *mut Display, screen: c_int) -> *mut c_void;
        pub fn XDefaultDepth(dpy: *mut Display, screen: c_int) -> c_int;
        pub fn XCreateGC(dpy: *mut Display, d: Window, valuemask: c_ulong, values: *mut c_void) -> GC;
        pub fn XFreeGC(dpy: *mut Display, gc: GC) -> c_int;
        pub fn XFlush(dpy: *mut Display) -> c_int;
        pub fn XSync(dpy: *mut Display, discard: Bool) -> c_int;
        pub fn XDestroyImage(ximage: *mut XImage) -> c_int;
    }

    #[link(name = "Xext")]
    extern "C" {
        pub fn XShmQueryExtension(dpy: *mut Display) -> Bool;
        pub fn XShmCreateImage(
            dpy: *mut Display,
            visual: *mut c_void,
            depth: c_uint,
            format: c_int,
            data: *mut c_char,
            shminfo: *mut XShmSegmentInfo,
            width: c_uint,
            height: c_uint,
        ) -> *mut XImage;
        pub fn XShmAttach(dpy: *mut Display, shminfo: *mut XShmSegmentInfo) -> Status;
        pub fn XShmDetach(dpy: *mut Display, shminfo: *mut XShmSegmentInfo) -> Status;
        pub fn XShmPutImage(
            dpy: *mut Display,
            d: Window,
            gc: GC,
            image: *mut XImage,
            src_x: c_int,
            src_y: c_int,
            dst_x: c_int,
            dst_y: c_int,
            src_width: c_uint,
            src_height: c_uint,
            send_event: Bool,
        ) -> Status;
    }

    pub struct X11ShmSurface {
        pub dpy: *mut Display,
        pub win: Window,
        pub gc: GC,
        pub ximage: *mut XImage,
        pub shminfo: alloc::boxed::Box<XShmSegmentInfo>,
        pub width: u32,
        pub height: u32,
        pub owns_display: bool,
    }

    unsafe impl Send for X11ShmSurface {}
    unsafe impl Sync for X11ShmSurface {}

    impl X11ShmSurface {
        pub unsafe fn new(mut dpy: *mut Display, win: Window, width: u32, height: u32) -> Result<Self, &'static str> {
            let owns_display = if dpy.is_null() {
                dpy = XOpenDisplay(core::ptr::null());
                if dpy.is_null() {
                    return Err("Failed to open X11 display");
                }
                true
            } else {
                false
            };

            if XShmQueryExtension(dpy) == 0 {
                if owns_display {
                    XCloseDisplay(dpy);
                }
                return Err("X11 MIT-SHM extension unavailable");
            }

            let screen = XDefaultScreen(dpy);
            let visual = XDefaultVisual(dpy, screen);
            let depth = XDefaultDepth(dpy, screen) as c_uint;

            let gc = XCreateGC(dpy, win, 0, core::ptr::null_mut());
            if gc.is_null() {
                if owns_display {
                    XCloseDisplay(dpy);
                }
                return Err("XCreateGC failed");
            }

            let mut shminfo = alloc::boxed::Box::new(core::mem::zeroed::<XShmSegmentInfo>());
            let ximage = XShmCreateImage(
                dpy,
                visual,
                depth,
                ZPixmap,
                core::ptr::null_mut(),
                shminfo.as_mut() as *mut XShmSegmentInfo,
                width.max(1),
                height.max(1),
            );

            if ximage.is_null() {
                XFreeGC(dpy, gc);
                if owns_display {
                    XCloseDisplay(dpy);
                }
                return Err("XShmCreateImage failed");
            }

            let size = ((*ximage).bytes_per_line * (*ximage).height) as usize;
            shminfo.shmid = libc::shmget(libc::IPC_PRIVATE, size, IPC_CREAT | 0o777);
            if shminfo.shmid < 0 {
                XDestroyImage(ximage);
                XFreeGC(dpy, gc);
                if owns_display {
                    XCloseDisplay(dpy);
                }
                return Err("shmget failed");
            }

            shminfo.shmaddr = libc::shmat(shminfo.shmid, core::ptr::null(), 0) as *mut c_char;
            if shminfo.shmaddr == (-1isize) as *mut c_char {
                libc::shmctl(shminfo.shmid, IPC_RMID, core::ptr::null_mut());
                XDestroyImage(ximage);
                XFreeGC(dpy, gc);
                if owns_display {
                    XCloseDisplay(dpy);
                }
                return Err("shmat failed");
            }

            (*ximage).data = shminfo.shmaddr;
            shminfo.readOnly = 0;

            if XShmAttach(dpy, shminfo.as_mut() as *mut XShmSegmentInfo) == 0 {
                libc::shmdt(shminfo.shmaddr as *mut c_void);
                libc::shmctl(shminfo.shmid, IPC_RMID, core::ptr::null_mut());
                XDestroyImage(ximage);
                XFreeGC(dpy, gc);
                if owns_display {
                    XCloseDisplay(dpy);
                }
                return Err("XShmAttach failed");
            }

            // Must sync with X server so it attaches the shared memory segment before we proceed
            XSync(dpy, 0);

            Ok(Self {
                dpy,
                win,
                gc,
                ximage,
                shminfo,
                width,
                height,
                owns_display,
            })
        }

        pub unsafe fn present(&mut self, src_rgba: &[u8], src_stride: usize) {
            let dst = (*self.ximage).data as *mut u8;
            let dst_stride = (*self.ximage).bytes_per_line as usize;
            let h = (self.height as usize).min((*self.ximage).height as usize);
            let w = (self.width as usize).min((*self.ximage).width as usize);

            // Copy with RGBA -> BGRA swizzle for standard X11 24/32-bit TrueColor visuals
            for y in 0..h {
                let src_row = &src_rgba[(y * src_stride)..(y * src_stride + w * 4)];
                let dst_row = core::slice::from_raw_parts_mut(dst.add(y * dst_stride), w * 4);
                for x in 0..w {
                    let so = x * 4;
                    let do_off = x * 4;
                    dst_row[do_off] = src_row[so + 2];     // B
                    dst_row[do_off + 1] = src_row[so + 1]; // G
                    dst_row[do_off + 2] = src_row[so];     // R
                    dst_row[do_off + 3] = src_row[so + 3]; // A
                }
            }

            XShmPutImage(
                self.dpy,
                self.win,
                self.gc,
                self.ximage,
                0,
                0,
                0,
                0,
                self.width,
                self.height,
                0,
            );
            XFlush(self.dpy);
        }
    }

    impl Drop for X11ShmSurface {
        fn drop(&mut self) {
            unsafe {
                XShmDetach(self.dpy, self.shminfo.as_mut() as *mut XShmSegmentInfo);
                XSync(self.dpy, 0);
                XDestroyImage(self.ximage);
                libc::shmdt(self.shminfo.shmaddr as *mut c_void);
                libc::shmctl(self.shminfo.shmid, IPC_RMID, core::ptr::null_mut());
                XFreeGC(self.dpy, self.gc);
                if self.owns_display {
                    XCloseDisplay(self.dpy);
                }
            }
        }
    }
}
