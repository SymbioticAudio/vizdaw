//! A VST3 plug-in's editor (host track H2 Task 1): the controller's
//! `IPlugView`, embedded in the host's native view (`NSView` on macOS, an
//! X11 window on Linux), and the host's `IPlugFrame` it is given.
//!
//! The order follows the SDK's hosting rules:
//! - a view is made only to open (`createView("editor")`), and released at
//!   close; whether the controller has one this platform embeds
//!   (`isPlatformTypeSupported`) is asked once and cached, so asking for the
//!   editor keeps no view alive;
//! - open: `setFrame`, then on Linux `setContentScaleFactor` (views there
//!   count physical pixels; macOS views count points and get no scale),
//!   `attached`, `getSize`;
//! - a user resize: `checkSizeConstraint`, then `onSize`, for a view that
//!   `canResize`; a fixed-size view keeps its size;
//! - the plug-in's own `IPlugFrame::resizeView`, also from a fixed-size
//!   view and also inside `attached`, is held and applied at the next idle
//!   (`onSize` with the size it asked for, reported as
//!   `InstanceEvents::editor_resize`), never re-entering the view from its
//!   own call;
//! - close: `removed`, `setFrame(null)`, the view released, then the frame
//!   shut: whatever the view left in the run loop is dropped and later
//!   calls on the frame are refused. The instance closes its editor before
//!   the controller is terminated.
//!
//! On Linux the frame is also the view's `Linux::IRunLoop` (X11 editors
//! query it from the frame): descriptors and timers go into the instance
//! thread's run loop ([`crate::run_loop`]), each view with its own table
//! ([`RunLoopHost`]). The module's host context offers one too (`ffi.rs`):
//! VSTGUI and the SDK's timers take theirs from the factory's host context
//! and keep it for the module's life, so those registrations are the
//! module's and go when they are unregistered or the module exits, not
//! with a view. Keys are not forwarded (`onKeyDown` and friends):
//! the `NSView` takes them itself on macOS, the plug-in's own X connection
//! on Linux.

use super::ffi::sdk;
use crate::error::HostError;
use crate::instance::{ParentView, PluginEditor};
#[cfg(unix)]
use crate::run_loop::{self, FdFlags, Member, Registrations, Target};
use std::cell::Cell;
#[cfg(unix)]
use std::cell::RefCell;
#[cfg(unix)]
use std::os::unix::io::RawFd;
#[cfg(unix)]
use std::rc::Rc;
use std::thread::ThreadId;
use vst3::{Class, ComPtr, ComWrapper};

#[cfg(target_os = "linux")]
use sdk::Linux::IRunLoop;
#[cfg(unix)]
use sdk::Linux::{
    FileDescriptor, IEventHandler, IEventHandlerTrait, ITimerHandler, ITimerHandlerTrait,
    TimerInterval,
};
use sdk::Vst::{IEditController, IEditControllerTrait, ViewType};
use sdk::{
    FIDString, IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kInvalidArgument,
    kPlatformTypeNSView, kPlatformTypeX11EmbedWindowID, kResultFalse, kResultOk, kResultTrue,
    tresult,
};
#[cfg(unix)]
use vst3::ComRef;

/// The platform type views embed with here: `NSView` on macOS,
/// `X11EmbedWindowID` on Linux (under Wayland through XWayland). `None`
/// elsewhere.
fn platform_type() -> Option<FIDString> {
    if cfg!(target_os = "macos") {
        Some(kPlatformTypeNSView)
    } else if cfg!(target_os = "linux") {
        Some(kPlatformTypeX11EmbedWindowID)
    } else {
        None
    }
}

/// Whether view sizes are physical pixels here (X11), not points (Cocoa).
const PIXELS: bool = cfg!(target_os = "linux");

/// The size taken when a view does not say (`getSize` fails), in points.
const FALLBACK_SIZE: (u32, u32) = (400, 300);

/// A window scale, or 1 when it is not a usable one.
fn usable_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// A size in the view's units as logical points, rounded up so the window
/// never clips it (the CLAP host's rule).
fn to_points(size: (u32, u32), scale: f64) -> (u32, u32) {
    let f = |v: u32| ((f64::from(v) / scale).ceil() as u32).max(1);
    (f(size.0), f(size.1))
}

/// Logical points as a size in the view's units.
fn from_points(size: (u32, u32), scale: f64) -> (u32, u32) {
    let f = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    (f(size.0), f(size.1))
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

/// A rect's size, or `None` when it is empty or inverted.
fn rect_size(r: &ViewRect) -> Option<(u32, u32)> {
    let w = u32::try_from(r.right.checked_sub(r.left)?).ok()?;
    let h = u32::try_from(r.bottom.checked_sub(r.top)?).ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// The handlers a view registered with the frame's run loop, by the ids
/// and descriptors of its run-loop table (main thread).
#[cfg(unix)]
#[derive(Default)]
struct Handlers {
    timers: RefCell<Vec<(u32, ComPtr<ITimerHandler>)>>,
    fds: RefCell<Vec<(RawFd, ComPtr<IEventHandler>)>>,
}

#[cfg(unix)]
impl Target for Handlers {
    fn on_timer(&self, id: u32) {
        // No borrow held across the call: the view may unregister.
        let h = self
            .timers
            .try_borrow()
            .ok()
            .and_then(|t| t.iter().find(|(t, _)| *t == id).map(|(_, h)| h.clone()));
        if let Some(h) = h {
            // SAFETY: a live handler the view registered.
            unsafe { h.onTimer() };
        }
    }

    fn on_fd(&self, fd: RawFd, _flags: FdFlags) {
        let hs: Vec<ComPtr<IEventHandler>> = self
            .fds
            .try_borrow()
            .map(|f| {
                f.iter()
                    .filter(|(f, _)| *f == fd)
                    .map(|(_, h)| h.clone())
                    .collect()
            })
            .unwrap_or_default();
        for h in hs {
            // SAFETY: as above.
            unsafe { h.onFDIsSet(fd) };
        }
    }
}

/// One host `Linux::IRunLoop`'s registrations: its own run-loop table,
/// joined to the thread's loop ([`crate::run_loop`]) until it is shut.
/// Every call is refused off the thread that made it and once shut. The
/// frame of each open view has one (dropped with the view), and so does
/// the module's host context (dropped when the module exits).
#[cfg(unix)]
pub(crate) struct RunLoopHost {
    main: ThreadId,
    live: Cell<bool>,
    table: Rc<RefCell<Registrations>>,
    handlers: Rc<Handlers>,
    member: RefCell<Option<Member>>,
}

#[cfg(unix)]
impl RunLoopHost {
    /// A new, empty run loop on this thread.
    pub(crate) fn new() -> RunLoopHost {
        let table = Rc::new(RefCell::new(Registrations::default()));
        let handlers = Rc::new(Handlers::default());
        let target: Rc<dyn Target> = handlers.clone();
        let member = run_loop::join(Rc::clone(&table), target);
        RunLoopHost {
            main: std::thread::current().id(),
            live: Cell::new(true),
            table,
            handlers,
            member: RefCell::new(Some(member)),
        }
    }

    fn usable(&self) -> bool {
        std::thread::current().id() == self.main && self.live.get()
    }

    /// Refuses every later call, leaves the thread's loop (dropping what
    /// is still registered) and releases the handlers. Main thread.
    pub(crate) fn shut(&self) {
        if std::thread::current().id() != self.main {
            return;
        }
        self.live.set(false);
        drop(self.member.borrow_mut().take());
        let timers = std::mem::take(&mut *self.handlers.timers.borrow_mut());
        let fds = std::mem::take(&mut *self.handlers.fds.borrow_mut());
        // Released with no borrow held (a handler's release is plug-in
        // code).
        drop(timers);
        drop(fds);
    }

    /// `registerEventHandler`: the descriptor is polled for reading (and
    /// errors); one handler may watch several, several one.
    pub(crate) fn register_event_handler(
        &self,
        handler: *mut IEventHandler,
        fd: FileDescriptor,
    ) -> tresult {
        if !self.usable() {
            return kResultFalse;
        }
        // SAFETY: the plug-in's handler, or null; ours is a new reference.
        let Some(h) = (unsafe { ComRef::from_raw(handler) }).map(|r| r.to_com_ptr()) else {
            return kInvalidArgument;
        };
        let Ok(mut fds) = self.handlers.fds.try_borrow_mut() else {
            return kResultFalse;
        };
        if fds.iter().any(|(f, x)| *f == fd && x.as_ptr() == handler) {
            return kResultTrue;
        }
        if fds.iter().any(|(f, _)| *f == fd) {
            // A new registration watches a descriptor found closed again.
            if let Ok(mut t) = self.table.try_borrow_mut() {
                t.rewatch_fd(fd);
            }
        } else {
            let added = self
                .table
                .try_borrow_mut()
                .is_ok_and(|mut t| t.register_fd(fd, FdFlags::READ));
            if !added {
                return kResultFalse;
            }
        }
        fds.push((fd, h));
        kResultTrue
    }

    /// `unregisterEventHandler`: every descriptor `handler` watches.
    pub(crate) fn unregister_event_handler(&self, handler: *mut IEventHandler) -> tresult {
        if !self.usable() {
            return kResultFalse;
        }
        let gone: Vec<(RawFd, ComPtr<IEventHandler>)> = {
            let Ok(mut fds) = self.handlers.fds.try_borrow_mut() else {
                return kResultFalse;
            };
            let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut *fds)
                .into_iter()
                .partition(|(_, h)| h.as_ptr() == handler);
            *fds = kept;
            for (fd, _) in &gone {
                if !fds.iter().any(|(f, _)| f == fd) {
                    if let Ok(mut t) = self.table.try_borrow_mut() {
                        t.unregister_fd(*fd);
                    }
                }
            }
            gone
        };
        // Released with no borrow held.
        if gone.is_empty() {
            kInvalidArgument
        } else {
            kResultTrue
        }
    }

    /// `registerTimer`.
    pub(crate) fn register_timer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: TimerInterval,
    ) -> tresult {
        if !self.usable() {
            return kResultFalse;
        }
        // SAFETY: as for descriptors.
        let Some(h) = (unsafe { ComRef::from_raw(handler) }).map(|r| r.to_com_ptr()) else {
            return kInvalidArgument;
        };
        let Ok(mut timers) = self.handlers.timers.try_borrow_mut() else {
            return kResultFalse;
        };
        let period = u32::try_from(milliseconds).unwrap_or(u32::MAX);
        let Some(id) = self
            .table
            .try_borrow_mut()
            .ok()
            .and_then(|mut t| t.register_timer(period))
        else {
            return kResultFalse;
        };
        timers.push((id, h));
        kResultTrue
    }

    /// `unregisterTimer`: every timer of `handler`.
    pub(crate) fn unregister_timer(&self, handler: *mut ITimerHandler) -> tresult {
        if !self.usable() {
            return kResultFalse;
        }
        let gone: Vec<(u32, ComPtr<ITimerHandler>)> = {
            let Ok(mut timers) = self.handlers.timers.try_borrow_mut() else {
                return kResultFalse;
            };
            let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut *timers)
                .into_iter()
                .partition(|(_, h)| h.as_ptr() == handler);
            *timers = kept;
            if let Ok(mut t) = self.table.try_borrow_mut() {
                for (id, _) in &gone {
                    t.unregister_timer(*id);
                }
            }
            gone
        };
        if gone.is_empty() {
            kInvalidArgument
        } else {
            kResultTrue
        }
    }
}

/// Implements `Linux::IRunLoopTrait` for a type over its `run:
/// RunLoopHost` field.
macro_rules! run_loop_host {
    ($t:ty) => {
        impl $crate::vst3::ffi::sdk::Linux::IRunLoopTrait for $t {
            unsafe fn registerEventHandler(
                &self,
                handler: *mut $crate::vst3::ffi::sdk::Linux::IEventHandler,
                fd: $crate::vst3::ffi::sdk::Linux::FileDescriptor,
            ) -> $crate::vst3::ffi::sdk::tresult {
                self.run.register_event_handler(handler, fd)
            }

            unsafe fn unregisterEventHandler(
                &self,
                handler: *mut $crate::vst3::ffi::sdk::Linux::IEventHandler,
            ) -> $crate::vst3::ffi::sdk::tresult {
                self.run.unregister_event_handler(handler)
            }

            unsafe fn registerTimer(
                &self,
                handler: *mut $crate::vst3::ffi::sdk::Linux::ITimerHandler,
                milliseconds: $crate::vst3::ffi::sdk::Linux::TimerInterval,
            ) -> $crate::vst3::ffi::sdk::tresult {
                self.run.register_timer(handler, milliseconds)
            }

            unsafe fn unregisterTimer(
                &self,
                handler: *mut $crate::vst3::ffi::sdk::Linux::ITimerHandler,
            ) -> $crate::vst3::ffi::sdk::tresult {
                self.run.unregister_timer(handler)
            }
        }
    };
}
#[cfg(unix)]
pub(crate) use run_loop_host;

/// The host's `IPlugFrame` for one open view (and on Linux its
/// `Linux::IRunLoop`, whose registrations go with the view). Every call is
/// refused off the thread that made it and once the view is closed
/// ([`HostFrame::shut`]).
pub(crate) struct HostFrame {
    main: ThreadId,
    live: Cell<bool>,
    /// The latest `resizeView` size (the view's units), for the next idle.
    resize: Cell<Option<(u32, u32)>>,
    #[cfg(unix)]
    run: RunLoopHost,
}

impl Class for HostFrame {
    #[cfg(target_os = "linux")]
    type Interfaces = (IPlugFrame, IRunLoop);
    #[cfg(not(target_os = "linux"))]
    type Interfaces = (IPlugFrame,);
}

#[cfg(unix)]
run_loop_host!(HostFrame);

impl HostFrame {
    fn new() -> HostFrame {
        HostFrame {
            main: std::thread::current().id(),
            live: Cell::new(true),
            resize: Cell::new(None),
            #[cfg(unix)]
            run: RunLoopHost::new(),
        }
    }

    fn usable(&self) -> bool {
        std::thread::current().id() == self.main && self.live.get()
    }

    /// Takes the pending `resizeView` size.
    fn take_resize(&self) -> Option<(u32, u32)> {
        self.resize.take()
    }

    /// Ends the frame: refuses every later call, forgets a pending resize,
    /// and drops what the view left in its run loop.
    fn shut(&self) {
        self.live.set(false);
        self.resize.set(None);
        #[cfg(unix)]
        self.run.shut();
    }
}

impl IPlugFrameTrait for HostFrame {
    unsafe fn resizeView(&self, _view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if !self.usable() {
            return kResultFalse;
        }
        // SAFETY: the plug-in's rect, or null.
        let Some(size) = (unsafe { new_size.as_ref() }).and_then(rect_size) else {
            return kInvalidArgument;
        };
        self.resize.set(Some(size));
        kResultTrue
    }
}

/// An open view.
struct Open {
    view: ComPtr<IPlugView>,
    frame: ComWrapper<HostFrame>,
    /// Physical pixels per point of this view's sizes: the scale on X11,
    /// 1 on Cocoa.
    gui_scale: f64,
    /// Its size in its units, as last opened, resized or followed.
    size: (u32, u32),
}

/// A VST3 instance's editor (see the module docs). Main thread only.
pub(crate) struct Vst3Editor {
    controller: Option<ComPtr<IEditController>>,
    /// Whether the controller makes a view this platform embeds (asked
    /// once).
    available: Option<bool>,
    /// The window scale for the next open.
    scale: f64,
    open: Option<Open>,
}

impl Vst3Editor {
    /// The editor of `controller` (none: no editor).
    pub(crate) fn new(controller: Option<ComPtr<IEditController>>) -> Vst3Editor {
        Vst3Editor {
            controller,
            available: None,
            scale: 1.0,
            open: None,
        }
    }

    /// Whether the plug-in has an editor here. The first call makes a view
    /// and releases it; the answer is kept.
    pub(crate) fn available(&mut self) -> bool {
        if self.open.is_some() {
            return true;
        }
        if let Some(a) = self.available {
            return a;
        }
        let a = self.create_view().is_ok();
        self.available = Some(a);
        a
    }

    /// A new view of the controller that embeds in this platform's type.
    fn create_view(&self) -> Result<(ComPtr<IPlugView>, FIDString), HostError> {
        let unavailable = |why: &str| Err(HostError::EditorUnavailable(why.into()));
        let Some(controller) = &self.controller else {
            return unavailable("the plug-in has no controller");
        };
        let Some(t) = platform_type() else {
            return unavailable("no embedding API here");
        };
        // SAFETY: the controller's own call; the view comes with our
        // reference (or null).
        let Some(view) = (unsafe { ComPtr::from_raw(controller.createView(ViewType::kEditor)) })
        else {
            return unavailable("the plug-in made no editor view");
        };
        // SAFETY: a plain query on the live view.
        if unsafe { view.isPlatformTypeSupported(t) } != kResultTrue {
            return unavailable("the editor does not embed in this platform's windows");
        }
        Ok((view, t))
    }

    /// Applies the view's latest `resizeView` (from the instance's idle)
    /// and returns the size the window should take, in points: the view
    /// gets `onSize` with the size it asked for. Nothing without an open
    /// view, or when the view refuses its own size.
    pub(crate) fn take_resize(&mut self) -> Option<(u32, u32)> {
        let o = self.open.as_mut()?;
        let want = o.frame.take_resize()?;
        let mut r = rect(want);
        // SAFETY: the open view; the rect is ours for the call.
        if unsafe { o.view.onSize(&mut r) } != kResultOk {
            return None;
        }
        o.size = want;
        Some(to_points(want, o.gui_scale))
    }
}

impl PluginEditor for Vst3Editor {
    fn set_scale(&mut self, scale: f64) {
        self.scale = usable_scale(scale);
    }

    fn open(&mut self, parent: ParentView) -> Result<(u32, u32), HostError> {
        self.close();
        let (view, t) = self.create_view()?;
        self.available = Some(true);
        let frame = ComWrapper::new(HostFrame::new());
        let Some(fp) = frame.to_com_ptr::<IPlugFrame>() else {
            frame.shut();
            return Err(HostError::EditorUnavailable("no frame".into()));
        };
        // SAFETY: the live view takes its own reference to the frame.
        unsafe { view.setFrame(fp.as_ptr()) };
        drop(fp);
        let gui_scale = if PIXELS {
            if let Some(s) = view.cast::<IPlugViewContentScaleSupport>() {
                // SAFETY: a plain call on the live view, before
                // `attached`. A view may decline it (it measures the
                // screen itself); its pixels are still the window's.
                unsafe { s.setContentScaleFactor(self.scale as f32) };
            }
            self.scale
        } else {
            1.0
        };
        // SAFETY: `parent` is a live native view (`ParentView::new`'s
        // contract) of the type the view supports.
        let r = unsafe { view.attached(parent.0, t) };
        if r != kResultOk {
            // SAFETY: the view takes the frame back before it goes.
            unsafe { view.setFrame(std::ptr::null_mut()) };
            drop(view);
            frame.shut();
            return Err(HostError::EditorUnavailable(format!(
                "the editor did not attach ({r})"
            )));
        }
        let mut got = rect((0, 0));
        // SAFETY: the rect is ours for the call.
        let size = (unsafe { view.getSize(&mut got) } == kResultOk)
            .then(|| rect_size(&got))
            .flatten()
            .unwrap_or_else(|| from_points(FALLBACK_SIZE, gui_scale));
        self.open = Some(Open {
            view,
            frame,
            gui_scale,
            size,
        });
        Ok(to_points(size, gui_scale))
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(u32, u32), HostError> {
        let o = self
            .open
            .as_mut()
            .ok_or_else(|| HostError::EditorUnavailable("no editor is open".into()))?;
        // SAFETY: plain calls on the open view; the rects are ours.
        unsafe {
            if o.view.canResize() != kResultTrue {
                return Ok(to_points(o.size, o.gui_scale));
            }
            let asked = from_points((width, height), o.gui_scale);
            let mut r = rect(asked);
            let want = if o.view.checkSizeConstraint(&mut r) == kResultOk {
                rect_size(&r).unwrap_or(asked)
            } else {
                asked
            };
            let mut r = rect(want);
            if o.view.onSize(&mut r) != kResultOk {
                return Err(HostError::EditorUnavailable(
                    "the editor refused the size".into(),
                ));
            }
            o.size = want;
            Ok(to_points(want, o.gui_scale))
        }
    }

    fn can_resize(&mut self) -> bool {
        // SAFETY: a plain query on the open view.
        self.open
            .as_ref()
            .is_some_and(|o| unsafe { o.view.canResize() } == kResultTrue)
    }

    fn close(&mut self) {
        let Some(Open { view, frame, .. }) = self.open.take() else {
            return;
        };
        // SAFETY: the open view: detached, then given no frame.
        unsafe {
            view.removed();
            view.setFrame(std::ptr::null_mut());
        }
        drop(view);
        frame.shut();
    }
}

impl Drop for Vst3Editor {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_convert_between_physical_pixels_and_points() {
        assert_eq!(to_points((600, 400), 2.0), (300, 200));
        assert_eq!(to_points((601, 401), 2.0), (301, 201), "rounded up");
        assert_eq!(to_points((1, 1), 4.0), (1, 1), "never empty");
        assert_eq!(from_points((301, 201), 1.25), (376, 251));
        assert_eq!(from_points((0, 0), 2.0), (1, 1));
    }

    #[test]
    fn empty_and_inverted_rects_have_no_size() {
        let r = |left, top, right, bottom| ViewRect {
            left,
            top,
            right,
            bottom,
        };
        assert_eq!(rect_size(&r(0, 0, 300, 200)), Some((300, 200)));
        assert_eq!(rect_size(&r(10, 20, 310, 220)), Some((300, 200)));
        assert_eq!(rect_size(&r(0, 0, 0, 200)), None);
        assert_eq!(rect_size(&r(10, 0, 0, 200)), None);
        assert_eq!(rect_size(&r(i32::MIN, 0, i32::MAX, 1)), None);
    }

    #[test]
    fn a_shut_frame_refuses_resizes_and_forgets_the_pending_one() {
        let f = HostFrame::new();
        let mut r = rect((300, 200));
        // SAFETY: plain calls on our own frame.
        unsafe {
            assert_eq!(f.resizeView(std::ptr::null_mut(), &mut r), kResultTrue);
            let mut empty = rect((0, 0));
            assert_eq!(
                f.resizeView(std::ptr::null_mut(), &mut empty),
                kInvalidArgument
            );
            assert_eq!(
                f.resizeView(std::ptr::null_mut(), std::ptr::null_mut()),
                kInvalidArgument
            );
        }
        f.shut();
        assert_eq!(f.take_resize(), None);
        // SAFETY: as above.
        assert_eq!(
            unsafe { f.resizeView(std::ptr::null_mut(), &mut r) },
            kResultFalse
        );
    }

    #[cfg(unix)]
    mod run_loop_side {
        //! The frame's `Linux::IRunLoop` against fake handlers (compiled on
        //! every unix; the frame offers it on Linux only).
        use super::*;
        use crate::run_loop::{run_loop_fds, run_loop_tick};
        use sdk::Linux::IRunLoopTrait;
        use std::time::Instant;

        #[derive(Default)]
        struct Calls {
            timers: Cell<u32>,
            fds: Cell<u32>,
        }

        struct Tick(Rc<Calls>);
        impl Class for Tick {
            type Interfaces = (ITimerHandler,);
        }
        impl ITimerHandlerTrait for Tick {
            unsafe fn onTimer(&self) {
                self.0.timers.set(self.0.timers.get() + 1);
            }
        }

        struct Ready(Rc<Calls>);
        impl Class for Ready {
            type Interfaces = (IEventHandler,);
        }
        impl IEventHandlerTrait for Ready {
            unsafe fn onFDIsSet(&self, _fd: FileDescriptor) {
                self.0.fds.set(self.0.fds.get() + 1);
            }
        }

        #[test]
        fn registrations_reach_the_loop_and_go_when_unregistered_or_shut() {
            let calls = Rc::new(Calls::default());
            let tick = ComWrapper::new(Tick(Rc::clone(&calls)));
            let ready = ComWrapper::new(Ready(Rc::clone(&calls)));
            let t = tick.to_com_ptr::<ITimerHandler>().unwrap();
            let e = ready.to_com_ptr::<IEventHandler>().unwrap();
            let f = HostFrame::new();
            // SAFETY: plain calls on our own frame with live handlers.
            unsafe {
                assert_eq!(f.registerTimer(t.as_ptr(), 0), kResultTrue);
                assert_eq!(f.registerEventHandler(e.as_ptr(), 7), kResultTrue);
                assert_eq!(f.registerEventHandler(e.as_ptr(), 7), kResultTrue);
                assert_eq!(
                    f.registerEventHandler(std::ptr::null_mut(), 8),
                    kInvalidArgument
                );
            }
            assert_eq!(run_loop_fds(), vec![(7, FdFlags::READ)]);
            run_loop_tick(Instant::now(), &[(7, FdFlags::READ)]);
            assert_eq!(calls.timers.get(), 1);
            assert_eq!(
                calls.fds.get(),
                1,
                "one callback for a pair registered twice"
            );
            // SAFETY: as above.
            unsafe {
                assert_eq!(f.unregisterEventHandler(e.as_ptr()), kResultTrue);
                assert_eq!(f.unregisterEventHandler(e.as_ptr()), kInvalidArgument);
            }
            assert!(run_loop_fds().is_empty());
            // A timer left registered goes when the frame shuts, and so do
            // the frame's references to its handler.
            f.shut();
            run_loop_tick(Instant::now(), &[]);
            assert_eq!(calls.timers.get(), 1);
            assert!(f.run.handlers.timers.borrow().is_empty());
            // SAFETY: as above.
            unsafe {
                assert_eq!(f.registerTimer(t.as_ptr(), 0), kResultFalse, "shut");
                assert_eq!(f.unregisterTimer(t.as_ptr()), kResultFalse);
            }
        }

        #[test]
        fn calls_from_another_thread_are_refused() {
            let f = Rc::new(HostFrame::new());
            let raw = Rc::as_ptr(&f) as usize;
            let r = std::thread::spawn(move || {
                // SAFETY: only the thread check runs before refusing.
                let f = unsafe { &*(raw as *const HostFrame) };
                let mut r = rect((10, 10));
                // SAFETY: as above.
                unsafe { f.resizeView(std::ptr::null_mut(), &mut r) }
            })
            .join()
            .unwrap();
            assert_eq!(r, kResultFalse);
            assert_eq!(f.take_resize(), None);
        }
    }
}
