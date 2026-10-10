//! libmpv loaded at run time (no link-time dependency, so cross-builds need no mpv SDK). Video is drawn by
//! mpv's render API into our own GL context; the UI draws on top.
use libloading::Library;
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::ptr::null_mut;

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct GlInitParams {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    ctx: *mut c_void,
}
#[repr(C)]
struct GlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}
#[repr(C)]
pub struct Event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

/// `mpv_event.data` for `MPV_EVENT_END_FILE` (7).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EndFileEvent {
    pub reason: c_int,
    pub error: c_int,
}

/// An event as surfaced to the player: its id plus any end-of-file payload.
pub struct MpvEvent {
    pub id: c_int,
    pub end_file: Option<EndFileEvent>,
}

pub const EV_END_FILE: c_int = 7;
pub const END_FILE_REASON_EOF: c_int = 0;
pub const END_FILE_REASON_ERROR: c_int = 4;

const P_API_TYPE: c_int = 1;
const P_GL_INIT: c_int = 2;
const P_GL_FBO: c_int = 3;
const P_FLIP_Y: c_int = 4;

type GetProc = Box<dyn Fn(&CStr) -> *const c_void>;

struct Fns {
    create: unsafe extern "C" fn() -> *mut c_void,
    initialize: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_option_string: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(*mut c_void, *mut *const c_char) -> c_int,
    get_property_string: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
    error_string: unsafe extern "C" fn(c_int) -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    wait_event: unsafe extern "C" fn(*mut c_void, c_double) -> *mut Event,
    terminate_destroy: unsafe extern "C" fn(*mut c_void),
    rc_create: unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut RenderParam) -> c_int,
    rc_update: unsafe extern "C" fn(*mut c_void) -> c_int,
    rc_render: unsafe extern "C" fn(*mut c_void, *mut RenderParam) -> c_int,
    rc_free: unsafe extern "C" fn(*mut c_void),
}

pub struct Mpv {
    _lib: Library,
    f: Fns,
    handle: *mut c_void,
    rc: *mut c_void,
    _get_proc: Box<GetProc>,
}

const LIB_NAMES: &[&str] = if cfg!(windows) {
    &["libmpv-2.dll", "mpv-2.dll", "mpv-1.dll"]
} else if cfg!(target_os = "macos") {
    &["libmpv.2.dylib", "libmpv.dylib", "@executable_path/../Frameworks/libmpv.2.dylib"]
} else {
    &["libmpv.so.2", "libmpv.so.1", "libmpv.so"]
};

unsafe extern "C" fn get_proc_trampoline(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let f = &*(ctx as *const GetProc);
    f(CStr::from_ptr(name)) as *mut c_void
}

impl Mpv {
    /// `get_proc` resolves GL function names in the current context. Call with that context current.
    pub fn new(get_proc: GetProc, opts: &[(&str, &str)]) -> Result<Mpv, String> {
        let lib = LIB_NAMES
            .iter()
            .find_map(|n| unsafe { Library::new(n).ok() })
            .ok_or_else(|| format!("libmpv not found (tried {LIB_NAMES:?})"))?;
        unsafe {
            macro_rules! sym {
                ($n:literal) => {
                    *lib.get(concat!($n, "\0").as_bytes()).map_err(|e| format!("{}: {e}", $n))?
                };
            }
            let f = Fns {
                create: sym!("mpv_create"),
                initialize: sym!("mpv_initialize"),
                set_option_string: sym!("mpv_set_option_string"),
                command: sym!("mpv_command"),
                get_property_string: sym!("mpv_get_property_string"),
                error_string: sym!("mpv_error_string"),
                free: sym!("mpv_free"),
                wait_event: sym!("mpv_wait_event"),
                terminate_destroy: sym!("mpv_terminate_destroy"),
                rc_create: sym!("mpv_render_context_create"),
                rc_update: sym!("mpv_render_context_update"),
                rc_render: sym!("mpv_render_context_render"),
                rc_free: sym!("mpv_render_context_free"),
            };
            let handle = (f.create)();
            if handle.is_null() {
                return Err("mpv_create failed".into());
            }
            for (k, v) in [("vo", "libmpv"), ("terminal", "no"), ("idle", "yes")].iter().chain(opts) {
                let (k, v) = (CString::new(*k).unwrap(), CString::new(*v).unwrap());
                (f.set_option_string)(handle, k.as_ptr(), v.as_ptr());
            }
            if (f.initialize)(handle) < 0 {
                return Err("mpv_initialize failed".into());
            }
            let get_proc: Box<GetProc> = Box::new(get_proc);
            let mut init = GlInitParams {
                get_proc_address: get_proc_trampoline,
                ctx: &*get_proc as *const GetProc as *mut c_void,
            };
            let api = CString::new("opengl").unwrap();
            let mut params = [
                RenderParam { kind: P_API_TYPE, data: api.as_ptr() as *mut c_void },
                RenderParam { kind: P_GL_INIT, data: &mut init as *mut _ as *mut c_void },
                RenderParam { kind: 0, data: null_mut() },
            ];
            let mut rc = null_mut();
            let e = (f.rc_create)(&mut rc, handle, params.as_mut_ptr());
            if e < 0 {
                (f.terminate_destroy)(handle);
                return Err(format!("mpv_render_context_create failed ({e})"));
            }
            Ok(Mpv { _lib: lib, f, handle, rc, _get_proc: get_proc })
        }
    }

    pub fn command(&self, args: &[&str]) -> i32 {
        let cs: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
        let mut ptrs: Vec<*const c_char> = cs.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        unsafe { (self.f.command)(self.handle, ptrs.as_mut_ptr()) }
    }

    pub fn get(&self, name: &str) -> Option<String> {
        let n = CString::new(name).unwrap();
        unsafe {
            let p = (self.f.get_property_string)(self.handle, n.as_ptr());
            if p.is_null() {
                return None;
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f.free)(p as *mut c_void);
            Some(s)
        }
    }

    /// A human-readable error for an mpv error code (END_FILE reason = error).
    pub fn error_string(&self, code: c_int) -> Option<String> {
        unsafe {
            let p = (self.f.error_string)(code);
            if p.is_null() {
                return None;
            }
            let s = CStr::from_ptr(p).to_string_lossy().into_owned();
            (self.f.free)(p as *mut c_void);
            Some(s)
        }
    }

    /// Drains pending events; returns their ids (e.g. 7 = END_FILE, 8 = FILE_LOADED) plus the
    /// end-of-file reason/error when present.
    pub fn events(&self) -> Vec<MpvEvent> {
        let mut out = vec![];
        loop {
            let e = unsafe { &*(self.f.wait_event)(self.handle, 0.0) };
            if e.event_id == 0 {
                return out;
            }
            let end_file =
                if e.event_id == EV_END_FILE { Some(unsafe { *(e.data as *const EndFileEvent) }) } else { None };
            out.push(MpvEvent { id: e.event_id, end_file });
        }
    }

    /// Render the current video frame into framebuffer `fbo` (0 = window). Advances the render
    /// context first so playback progresses.
    pub fn render(&self, fbo: i32, w: i32, h: i32) {
        unsafe {
            (self.f.rc_update)(self.rc);
        }
        let mut fb = GlFbo { fbo, w, h, internal_format: 0 };
        let mut flip: c_int = if fbo == 0 { 1 } else { 0 };
        let mut params = [
            RenderParam { kind: P_GL_FBO, data: &mut fb as *mut _ as *mut c_void },
            RenderParam { kind: P_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
            RenderParam { kind: 0, data: null_mut() },
        ];
        unsafe { (self.f.rc_render)(self.rc, params.as_mut_ptr()) };
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe {
            (self.f.rc_free)(self.rc);
            (self.f.terminate_destroy)(self.handle);
        }
    }
}
