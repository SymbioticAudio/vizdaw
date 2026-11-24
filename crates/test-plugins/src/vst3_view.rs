//! The test VST3's editor (host track H2 Task 1): the `IPlugView` that
//! "Test View"'s controller ([`crate::VST3_VIEW_ID`]) makes for
//! `createView("editor")`. It is headless (it never touches the parent it
//! is given, so host tests run without a display) and records what the
//! host does to it in counters a test reads through
//! [`crate::Vst3Probe::view`]: `attached`/`removed`, the platform type and
//! parent, the content scale and whether it came before `attached`,
//! `setFrame`, `getSize`, `checkSizeConstraint` and `onSize`, and when the
//! view is released (on the module's sequence clock, so it orders against
//! the controller's `terminate`).
//!
//! It takes the SDK's sizes: points on macOS, physical pixels on Linux
//! ([`crate::BASE_SIZE`] times the content scale). The probe sets how views
//! made from then on behave ([`vst3_view_flags`]): resizable (clamped to
//! [`crate::MIN_SIZE`]–[`crate::MAX_SIZE`]), asking the host to resize it
//! through the frame inside `attached`, leaving its run-loop registrations
//! behind in `removed`, or (Linux) drawing a real X11 child window with the
//! GUI kind's [`crate::gui::XChild`]. The probe can also make the latest
//! view ask for a resize ([`crate::Vst3Probe::view_resize`]).
//!
//! On Linux, `attached` queries the frame for `Linux::IRunLoop`, as VSTGUI
//! does, and registers a timer ([`crate::GUI_TIMER_MS`]) and a descriptor:
//! a pipe's read end (written by [`crate::Vst3Probe::view_poke`]) for the
//! headless view, the X connection's for the X11 one. `removed`
//! unregisters both unless told not to.

use crate::vst3_module::{next_seq, sdk};
use crate::{BASE_SIZE, MAX_SIZE, MIN_SIZE};
use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_void};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use sdk::Linux::{
    IEventHandler, IEventHandlerTrait, IRunLoop, IRunLoopTrait, ITimerHandler, ITimerHandlerTrait,
};
use sdk::{
    FIDString, IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, TBool, ViewRect, char16, int16,
    kInvalidArgument, kPlatformTypeNSView, kPlatformTypeX11EmbedWindowID, kResultFalse, kResultOk,
    kResultTrue, tresult,
};

/// What views made from now on do ([`crate::Vst3Probe::set_view`]).
pub mod vst3_view_flags {
    /// `canResize` says yes; `checkSizeConstraint` clamps to
    /// [`crate::MIN_SIZE`]–[`crate::MAX_SIZE`] (else it answers its own
    /// size).
    pub const RESIZABLE: u32 = 1;
    /// Inside `attached`, asks the frame to resize it to the size given
    /// with the flags (`resizeView`), as editors that restore a zoom do.
    pub const RESIZE_IN_ATTACHED: u32 = 2;
    /// `removed` leaves its run-loop timer and descriptor registered (the
    /// host must drop them).
    pub const KEEP_RUN_LOOP: u32 = 4;
    /// Linux: draws a real X11 child window under the parent (needs a
    /// display; `attached` fails without one).
    pub const X11: u32 = 8;
    /// `isPlatformTypeSupported` says no to everything.
    pub const NO_PLATFORM: u32 = 16;
}

/// The platform type `attached` got, as [`Vst3ViewCounters::platform`]
/// reads it.
pub mod vst3_platform {
    /// `NSView`.
    pub const NS_VIEW: u64 = 1;
    /// `X11EmbedWindowID`.
    pub const X11: u64 = 2;
    /// Anything else.
    pub const OTHER: u64 = 3;
}

static FLAGS: AtomicU32 = AtomicU32::new(0);
/// The size [`vst3_view_flags::RESIZE_IN_ATTACHED`] asks for (`w << 16 | h`).
static ASK: AtomicU32 = AtomicU32::new(0);

/// Indices into [`COUNTERS`] (the order of `vizdaw_test_vst3_view_counters`).
mod vx {
    pub const CREATED: usize = 0;
    pub const RELEASES: usize = 1;
    pub const LAST_RELEASE: usize = 2;
    pub const ATTACHED: usize = 3;
    pub const LAST_ATTACHED: usize = 4;
    pub const REMOVED: usize = 5;
    pub const LAST_REMOVED: usize = 6;
    pub const PLATFORM: usize = 7;
    pub const PARENT: usize = 8;
    pub const SCALE: usize = 9;
    pub const SCALES: usize = 10;
    pub const SCALED_FIRST: usize = 11;
    pub const FRAME_SETS: usize = 12;
    pub const FRAME_CLEARS: usize = 13;
    pub const LAST_FRAME_CLEAR: usize = 14;
    pub const ON_SIZES: usize = 15;
    pub const ON_SIZE_W: usize = 16;
    pub const ON_SIZE_H: usize = 17;
    pub const CHECKS: usize = 18;
    pub const WIDTH: usize = 19;
    pub const HEIGHT: usize = 20;
    pub const ATTACH_RESIZE: usize = 21;
    pub const TIMER_REG: usize = 22;
    pub const FD_REG: usize = 23;
    pub const TIMER_CALLS: usize = 24;
    pub const FD_CALLS: usize = 25;
    pub const UNREGISTERS: usize = 26;
    pub const X_WINDOW: usize = 27;
    pub const DRAWS: usize = 28;
    pub const PLATFORM_ASKS: usize = 29;
    pub const HOST_RUN_LOOP: usize = 30;
    pub const N: usize = 31;
}

static COUNTERS: [AtomicU64; vx::N] = [const { AtomicU64::new(0) }; vx::N];

fn bump(i: usize) {
    if let Some(c) = COUNTERS.get(i) {
        c.fetch_add(1, Ordering::SeqCst);
    }
}

fn stamp(i: usize) {
    store(i, next_seq());
}

fn store(i: usize, v: u64) {
    if let Some(c) = COUNTERS.get(i) {
        c.store(v, Ordering::SeqCst);
    }
}

/// A result as a counter (two's complement).
fn store_i(i: usize, v: i64) {
    store(i, v as u64);
}

/// "Not tried" for the registration and resize results.
const NOT_TRIED: i64 = -2;
/// "The frame has no `IRunLoop`".
const NO_RUN_LOOP: i64 = -1;

/// What the view's handlers and the probe share with it (main thread).
struct State {
    flags: u32,
    /// The view's own `IPlugView` pointer (no reference), for
    /// `resizeView`.
    me: Cell<*mut IPlugView>,
    frame: RefCell<Option<ComPtr<IPlugFrame>>>,
    attached: Cell<bool>,
    /// A content scale came since the view was made (0: none).
    scale: Cell<f32>,
    /// In the platform's units (points on macOS, pixels on Linux).
    size: Cell<(u32, u32)>,
    run_loop: RefCell<Option<ComPtr<IRunLoop>>>,
    timer: RefCell<Option<ComPtr<ITimerHandler>>>,
    events: RefCell<Option<ComPtr<IEventHandler>>>,
    /// The headless view's pipe (read end registered, write end poked).
    #[cfg(target_os = "linux")]
    pipe: Cell<Option<(i32, i32)>>,
    #[cfg(target_os = "linux")]
    x: RefCell<Option<crate::gui::XChild>>,
}

thread_local! {
    /// The latest view made on this thread, for the probe's commands.
    static LATEST: RefCell<Weak<State>> = const { RefCell::new(Weak::new()) };
}

impl State {
    fn resizable(&self) -> bool {
        self.flags & vst3_view_flags::RESIZABLE != 0
    }

    fn set_size(&self, size: (u32, u32)) {
        self.size.set(size);
        store(vx::WIDTH, u64::from(size.0));
        store(vx::HEIGHT, u64::from(size.1));
        #[cfg(target_os = "linux")]
        if let Some(x) = self.x.borrow_mut().as_mut() {
            x.resize(size);
        }
    }

    /// Asks the frame for `size` (`resizeView`); the frame's result, or
    /// -1 without a frame.
    fn ask_resize(&self, size: (u32, u32)) -> i64 {
        let Some(frame) = self.frame.borrow().clone() else {
            return -1;
        };
        let mut r = rect(size);
        // SAFETY: the host's live frame; our own view pointer.
        i64::from(unsafe { frame.resizeView(self.me.get(), &mut r) })
    }

    /// Registers the timer and the descriptor with the frame's run loop
    /// (Linux), recording the results.
    fn register(self: &Rc<State>, fd: Option<i32>) {
        store_i(vx::TIMER_REG, NO_RUN_LOOP);
        store_i(vx::FD_REG, NO_RUN_LOOP);
        let Some(run_loop) = self
            .frame
            .borrow()
            .as_ref()
            .and_then(|f| f.cast::<IRunLoop>())
        else {
            return;
        };
        let timer = ComWrapper::new(Tick(Rc::downgrade(self))).to_com_ptr::<ITimerHandler>();
        if let Some(t) = &timer {
            // SAFETY: the host's live run loop; it takes its own reference.
            let r = unsafe { run_loop.registerTimer(t.as_ptr(), u64::from(crate::GUI_TIMER_MS)) };
            store_i(vx::TIMER_REG, i64::from(r));
        }
        if let Some(fd) = fd {
            let events = ComWrapper::new(Ready(Rc::downgrade(self))).to_com_ptr::<IEventHandler>();
            if let Some(e) = &events {
                // SAFETY: as above.
                let r = unsafe { run_loop.registerEventHandler(e.as_ptr(), fd) };
                store_i(vx::FD_REG, i64::from(r));
            }
            *self.events.borrow_mut() = events;
        }
        *self.timer.borrow_mut() = timer;
        *self.run_loop.borrow_mut() = Some(run_loop);
    }

    /// Unregisters what [`State::register`] registered.
    fn unregister(&self) {
        let Some(run_loop) = self.run_loop.borrow_mut().take() else {
            return;
        };
        if let Some(t) = self.timer.borrow_mut().take() {
            // SAFETY: the host's live run loop and our handler.
            if unsafe { run_loop.unregisterTimer(t.as_ptr()) } == kResultOk {
                bump(vx::UNREGISTERS);
            }
        }
        if let Some(e) = self.events.borrow_mut().take() {
            // SAFETY: as above.
            if unsafe { run_loop.unregisterEventHandler(e.as_ptr()) } == kResultOk {
                bump(vx::UNREGISTERS);
            }
        }
    }

    fn on_timer(&self) {
        bump(vx::TIMER_CALLS);
        #[cfg(target_os = "linux")]
        if let Some(x) = self.x.borrow_mut().as_mut() {
            x.drain();
            x.draw();
            store(vx::DRAWS, u64::from(x.draws()));
        }
    }

    fn on_fd(&self, _fd: i32) {
        bump(vx::FD_CALLS);
        #[cfg(target_os = "linux")]
        {
            if let Some(x) = self.x.borrow_mut().as_mut() {
                if x.drain() {
                    x.draw();
                    store(vx::DRAWS, u64::from(x.draws()));
                }
                return;
            }
            if let Some((r, _)) = self.pipe.get() {
                let mut b = [0u8; 64];
                // SAFETY: a non-blocking read into our buffer.
                unsafe { libc::read(r, b.as_mut_ptr() as *mut c_void, b.len()) };
            }
        }
    }
}

fn rect(size: (u32, u32)) -> ViewRect {
    let v = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
    ViewRect {
        left: 0,
        top: 0,
        right: v(size.0),
        bottom: v(size.1),
    }
}

fn rect_size(r: &ViewRect) -> (u32, u32) {
    let v = |n: i32| u32::try_from(n.max(0)).unwrap_or(0);
    (v(r.right - r.left), v(r.bottom - r.top))
}

/// The view's run-loop timer handler.
struct Tick(Weak<State>);

impl Class for Tick {
    type Interfaces = (ITimerHandler,);
}

impl ITimerHandlerTrait for Tick {
    unsafe fn onTimer(&self) {
        if let Some(s) = self.0.upgrade() {
            s.on_timer();
        }
    }
}

/// The view's run-loop descriptor handler.
struct Ready(Weak<State>);

impl Class for Ready {
    type Interfaces = (IEventHandler,);
}

impl IEventHandlerTrait for Ready {
    unsafe fn onFDIsSet(&self, fd: i32) {
        if let Some(s) = self.0.upgrade() {
            s.on_fd(fd);
        }
    }
}

/// The view (see the module docs).
pub(crate) struct View(Rc<State>);

impl Drop for View {
    fn drop(&mut self) {
        bump(vx::RELEASES);
        stamp(vx::LAST_RELEASE);
        let s = &self.0;
        s.frame.borrow_mut().take();
        s.run_loop.borrow_mut().take();
        s.timer.borrow_mut().take();
        s.events.borrow_mut().take();
        #[cfg(target_os = "linux")]
        {
            s.x.borrow_mut().take();
            if let Some((r, w)) = s.pipe.take() {
                // SAFETY: our own descriptors, closed once.
                unsafe {
                    libc::close(r);
                    libc::close(w);
                }
            }
        }
    }
}

impl Class for View {
    type Interfaces = (IPlugView, IPlugViewContentScaleSupport);
}

/// Records whether the factory's host context offers `Linux::IRunLoop`
/// (VSTGUI takes its run loop from there).
pub(crate) fn note_host_context(context: *mut sdk::FUnknown) {
    // SAFETY: the host's context, or null.
    let has = unsafe { ComRef::from_raw(context) }.is_some_and(|c| c.cast::<IRunLoop>().is_some());
    store(vx::HOST_RUN_LOOP, u64::from(has));
}

/// A new view (`createView("editor")`), as an owned `IPlugView` pointer.
pub(crate) fn create() -> *mut IPlugView {
    let state = Rc::new(State {
        flags: FLAGS.load(Ordering::SeqCst),
        me: Cell::new(std::ptr::null_mut()),
        frame: RefCell::new(None),
        attached: Cell::new(false),
        scale: Cell::new(0.0),
        size: Cell::new(BASE_SIZE),
        run_loop: RefCell::new(None),
        timer: RefCell::new(None),
        events: RefCell::new(None),
        #[cfg(target_os = "linux")]
        pipe: Cell::new(None),
        #[cfg(target_os = "linux")]
        x: RefCell::new(None),
    });
    let view = ComWrapper::new(View(Rc::clone(&state)));
    let Some(p) = view.to_com_ptr::<IPlugView>() else {
        return std::ptr::null_mut();
    };
    state.me.set(p.as_ptr());
    bump(vx::CREATED);
    store(vx::WIDTH, u64::from(BASE_SIZE.0));
    store(vx::HEIGHT, u64::from(BASE_SIZE.1));
    store(vx::SCALE, 0);
    store_i(vx::SCALED_FIRST, -1);
    store_i(vx::ATTACH_RESIZE, NOT_TRIED);
    store_i(vx::TIMER_REG, NOT_TRIED);
    store_i(vx::FD_REG, NOT_TRIED);
    store(vx::X_WINDOW, 0);
    let _ = LATEST.try_with(|l| *l.borrow_mut() = Rc::downgrade(&state));
    p.into_raw()
}

fn type_is(t: FIDString, want: FIDString) -> bool {
    if t.is_null() {
        return false;
    }
    // SAFETY: both are NUL-terminated strings (the host's and the SDK's).
    unsafe { CStr::from_ptr(t) == CStr::from_ptr(want) }
}

/// The platform type this build embeds in.
fn native_type() -> FIDString {
    if cfg!(target_os = "macos") {
        kPlatformTypeNSView
    } else {
        kPlatformTypeX11EmbedWindowID
    }
}

impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, t: FIDString) -> tresult {
        bump(vx::PLATFORM_ASKS);
        if self.0.flags & vst3_view_flags::NO_PLATFORM == 0 && type_is(t, native_type()) {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn attached(&self, parent: *mut c_void, t: FIDString) -> tresult {
        let s = &self.0;
        if parent.is_null() || s.attached.get() {
            return kInvalidArgument;
        }
        let platform = if type_is(t, kPlatformTypeNSView) {
            crate::vst3_platform::NS_VIEW
        } else if type_is(t, kPlatformTypeX11EmbedWindowID) {
            crate::vst3_platform::X11
        } else {
            crate::vst3_platform::OTHER
        };
        bump(vx::ATTACHED);
        stamp(vx::LAST_ATTACHED);
        store(vx::PLATFORM, platform);
        store(vx::PARENT, parent as usize as u64);
        store(vx::SCALED_FIRST, u64::from(s.scale.get() > 0.0));
        if !type_is(t, native_type()) {
            return kResultFalse;
        }
        #[allow(unused_mut)]
        let mut fd = None;
        #[cfg(target_os = "linux")]
        {
            if s.flags & vst3_view_flags::X11 != 0 {
                let Some(mut x) = crate::gui::XChild::connect(s.size.get()) else {
                    return kResultFalse;
                };
                let Ok(xid) = u32::try_from(parent as usize) else {
                    return kInvalidArgument;
                };
                if x.create(xid).is_err() || x.show(true).is_err() {
                    return kResultFalse;
                }
                store(vx::X_WINDOW, u64::from(x.window()));
                fd = Some(x.fd());
                *s.x.borrow_mut() = Some(x);
            } else {
                let mut p = [0i32; 2];
                // SAFETY: `p` has room for the two descriptors.
                if unsafe { libc::pipe(p.as_mut_ptr()) } == 0 {
                    // SAFETY: our own new descriptor.
                    unsafe { libc::fcntl(p[0], libc::F_SETFL, libc::O_NONBLOCK) };
                    s.pipe.set(Some((p[0], p[1])));
                    fd = Some(p[0]);
                }
            }
        }
        s.attached.set(true);
        s.register(fd);
        if s.flags & vst3_view_flags::RESIZE_IN_ATTACHED != 0 {
            let ask = ASK.load(Ordering::SeqCst);
            store_i(vx::ATTACH_RESIZE, s.ask_resize((ask >> 16, ask & 0xFFFF)));
        }
        kResultOk
    }

    unsafe fn removed(&self) -> tresult {
        let s = &self.0;
        if !s.attached.replace(false) {
            return kResultFalse;
        }
        bump(vx::REMOVED);
        stamp(vx::LAST_REMOVED);
        if s.flags & vst3_view_flags::KEEP_RUN_LOOP == 0 {
            s.unregister();
        }
        #[cfg(target_os = "linux")]
        s.x.borrow_mut().take();
        kResultOk
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyDown(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyUp(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        // SAFETY: the host's rect, or null.
        let Some(r) = (unsafe { size.as_mut() }) else {
            return kInvalidArgument;
        };
        *r = rect(self.0.size.get());
        kResultOk
    }

    unsafe fn onSize(&self, size: *mut ViewRect) -> tresult {
        // SAFETY: the host's rect, or null.
        let Some(r) = (unsafe { size.as_ref() }) else {
            return kInvalidArgument;
        };
        let (w, h) = rect_size(r);
        bump(vx::ON_SIZES);
        store(vx::ON_SIZE_W, u64::from(w));
        store(vx::ON_SIZE_H, u64::from(h));
        self.0.set_size((w.max(1), h.max(1)));
        kResultOk
    }

    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        // SAFETY: the host's frame, or null; `to_com_ptr` adds our
        // reference.
        let f = unsafe { ComRef::from_raw(frame) }.map(|r| r.to_com_ptr());
        if f.is_some() {
            bump(vx::FRAME_SETS);
        } else {
            bump(vx::FRAME_CLEARS);
            stamp(vx::LAST_FRAME_CLEAR);
        }
        *self.0.frame.borrow_mut() = f;
        kResultOk
    }

    unsafe fn canResize(&self) -> tresult {
        if self.0.resizable() {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn checkSizeConstraint(&self, size: *mut ViewRect) -> tresult {
        // SAFETY: the host's rect, or null.
        let Some(r) = (unsafe { size.as_mut() }) else {
            return kInvalidArgument;
        };
        bump(vx::CHECKS);
        let want = if self.0.resizable() {
            let (w, h) = rect_size(r);
            (
                w.clamp(MIN_SIZE.0, MAX_SIZE.0),
                h.clamp(MIN_SIZE.1, MAX_SIZE.1),
            )
        } else {
            self.0.size.get()
        };
        let out = rect(want);
        r.right = r.left + out.right;
        r.bottom = r.top + out.bottom;
        kResultTrue
    }
}

impl IPlugViewContentScaleSupportTrait for View {
    unsafe fn setContentScaleFactor(&self, factor: f32) -> tresult {
        if !factor.is_finite() || factor <= 0.0 {
            return kInvalidArgument;
        }
        let s = &self.0;
        bump(vx::SCALES);
        store(vx::SCALE, f64::from(factor).to_bits());
        let old = s.scale.replace(factor);
        let old = if old > 0.0 { old } else { 1.0 };
        // Its pixels follow the scale, as a real editor's would.
        let (w, h) = s.size.get();
        let f = |v: u32| (f64::from(v) * f64::from(factor) / f64::from(old)).round() as u32;
        s.set_size((f(w), f(h)));
        kResultOk
    }
}

/// What the test views have seen (see the module docs). `last_*` fields
/// are sequence numbers on the module's clock ([`crate::Vst3Counters`]),
/// 0 for never; the others describe the latest view.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vst3ViewCounters {
    /// Views made (`createView("editor")`).
    pub created: u64,
    /// Views released.
    pub releases: u64,
    pub last_release: u64,
    /// `attached` calls.
    pub attached: u64,
    pub last_attached: u64,
    /// `removed` calls that detached a view.
    pub removed: u64,
    pub last_removed: u64,
    /// The platform type of the latest `attached` ([`vst3_platform`]).
    pub platform: u64,
    /// The parent of the latest `attached` (an `NSView *` or an X window
    /// ID).
    pub parent: u64,
    /// The latest content scale (0: none since the view was made).
    pub scale: f64,
    /// `setContentScaleFactor` calls.
    pub scales: u64,
    /// At the latest `attached`: 1 if a content scale came first, 0 if
    /// not; -1 before any.
    pub scaled_first: i64,
    /// `setFrame` with a frame, and with null.
    pub frame_sets: u64,
    pub frame_clears: u64,
    pub last_frame_clear: u64,
    /// `onSize` calls and the latest size given, in the platform's units.
    pub on_sizes: u64,
    pub on_size: (u32, u32),
    /// `checkSizeConstraint` calls.
    pub checks: u64,
    /// The latest view's size in the platform's units.
    pub size: (u32, u32),
    /// What `resizeView` inside `attached` returned (-2: not tried).
    pub attach_resize: i64,
    /// What `registerTimer` and `registerEventHandler` returned (-1: the
    /// frame has no `IRunLoop`; -2: not tried).
    pub timer_registered: i64,
    pub fd_registered: i64,
    /// Run-loop callbacks.
    pub timer_calls: u64,
    pub fd_calls: u64,
    /// Successful unregister calls in `removed`.
    pub unregisters: u64,
    /// The X11 variant's window (0: none) and how often it drew.
    pub x_window: u64,
    pub draws: u64,
    /// `isPlatformTypeSupported` calls.
    pub platform_asks: u64,
    /// The latest factory host context offered `Linux::IRunLoop`.
    pub host_run_loop: bool,
}

/// Sets what views made from now on do: [`vst3_view_flags`], and the
/// size [`vst3_view_flags::RESIZE_IN_ATTACHED`] asks for.
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_set_view(flags: u32, width: u32, height: u32) {
    FLAGS.store(flags, Ordering::SeqCst);
    ASK.store(
        (width.min(0xFFFF) << 16) | height.min(0xFFFF),
        Ordering::SeqCst,
    );
}

/// Copies the view counters into `out` (`len` values, in [`vx`] order).
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_view_counters(out: *mut u64, len: usize) {
    if out.is_null() {
        return;
    }
    for (i, c) in COUNTERS.iter().enumerate().take(len) {
        // SAFETY: the caller passes room for `len` values.
        unsafe { *out.add(i) = c.load(Ordering::SeqCst) };
    }
}

/// Makes the latest view on this thread ask its frame for `width` ×
/// `height` (`resizeView`); the frame's result, or -1 without a view or
/// frame.
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_view_resize(width: u32, height: u32) -> i32 {
    let Some(s) = LATEST.try_with(|l| l.borrow().upgrade()).ok().flatten() else {
        return -1;
    };
    i32::try_from(s.ask_resize((width, height))).unwrap_or(-1)
}

/// Writes a byte into the latest headless view's pipe on this thread, so
/// its run-loop descriptor becomes readable (Linux); 1 when written, else
/// 0.
#[unsafe(no_mangle)]
pub extern "C" fn vizdaw_test_vst3_view_poke() -> i32 {
    #[cfg(target_os = "linux")]
    {
        let Some(s) = LATEST.try_with(|l| l.borrow().upgrade()).ok().flatten() else {
            return 0;
        };
        let Some((_, w)) = s.pipe.get() else {
            return 0;
        };
        // SAFETY: one byte from our buffer into our own pipe.
        return i32::from(unsafe { libc::write(w, [1u8].as_ptr() as *const c_void, 1) } == 1);
    }
    #[allow(unreachable_code)]
    0
}

impl crate::Vst3Probe {
    /// Sets what views made from now on do: [`vst3_view_flags`], and the
    /// size (the platform's units) [`vst3_view_flags::RESIZE_IN_ATTACHED`] asks
    /// for.
    pub fn set_view(&self, flags: u32, ask: (u32, u32)) {
        // SAFETY: the exported function's signature.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(u32, u32, u32)>(b"vizdaw_test_vst3_set_view\0")
        }
        .unwrap();
        f(flags, ask.0, ask.1);
    }

    /// The view counters now.
    pub fn view(&self) -> Vst3ViewCounters {
        // SAFETY: the exported function fills `len` values.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(*mut u64, usize)>(b"vizdaw_test_vst3_view_counters\0")
        }
        .unwrap();
        let mut v = [0u64; vx::N];
        f(v.as_mut_ptr(), v.len());
        Vst3ViewCounters {
            created: v[vx::CREATED],
            releases: v[vx::RELEASES],
            last_release: v[vx::LAST_RELEASE],
            attached: v[vx::ATTACHED],
            last_attached: v[vx::LAST_ATTACHED],
            removed: v[vx::REMOVED],
            last_removed: v[vx::LAST_REMOVED],
            platform: v[vx::PLATFORM],
            parent: v[vx::PARENT],
            scale: f64::from_bits(v[vx::SCALE]),
            scales: v[vx::SCALES],
            scaled_first: v[vx::SCALED_FIRST] as i64,
            frame_sets: v[vx::FRAME_SETS],
            frame_clears: v[vx::FRAME_CLEARS],
            last_frame_clear: v[vx::LAST_FRAME_CLEAR],
            on_sizes: v[vx::ON_SIZES],
            on_size: (v[vx::ON_SIZE_W] as u32, v[vx::ON_SIZE_H] as u32),
            checks: v[vx::CHECKS],
            size: (v[vx::WIDTH] as u32, v[vx::HEIGHT] as u32),
            attach_resize: v[vx::ATTACH_RESIZE] as i64,
            timer_registered: v[vx::TIMER_REG] as i64,
            fd_registered: v[vx::FD_REG] as i64,
            timer_calls: v[vx::TIMER_CALLS],
            fd_calls: v[vx::FD_CALLS],
            unregisters: v[vx::UNREGISTERS],
            x_window: v[vx::X_WINDOW],
            draws: v[vx::DRAWS],
            platform_asks: v[vx::PLATFORM_ASKS],
            host_run_loop: v[vx::HOST_RUN_LOOP] != 0,
        }
    }

    /// Makes the latest view (made on this thread) ask its frame for a new
    /// size in the platform's units; the frame's result (-1: no view or
    /// frame).
    pub fn view_resize(&self, width: u32, height: u32) -> i32 {
        // SAFETY: the exported function's signature.
        let f = unsafe {
            self.0
                .get::<extern "C" fn(u32, u32) -> i32>(b"vizdaw_test_vst3_view_resize\0")
        }
        .unwrap();
        f(width, height)
    }

    /// Makes the latest headless view's run-loop descriptor readable
    /// (Linux); false when there is none.
    pub fn view_poke(&self) -> bool {
        // SAFETY: the exported function's signature.
        let f = unsafe {
            self.0
                .get::<extern "C" fn() -> i32>(b"vizdaw_test_vst3_view_poke\0")
        }
        .unwrap();
        f() == 1
    }
}
