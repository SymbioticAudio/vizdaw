//! The GUI kind's editor (Linux, CLAP `gui` embedded in X11): a real child
//! window under the host's parent, drawn through the plug-in's own X
//! connection, as DPF and JUCE editors do. In `gui.create` it connects to
//! the X server and registers a host timer and the connection's descriptor
//! (the host's run-loop table must already be there). The descriptor's
//! callbacks read the X events; the timer's redraw the window and, the first
//! time after `show`, ask the host to grow it by [`GUI_GROW`] points.

use crate::plugin::MainThread;
use crate::{
    BASE_SIZE, GUI_COLOR, GUI_GROW, GUI_TIMER_MS, PARAM_GUI_CREATE_FD, PARAM_GUI_CREATE_TIMER,
    PARAM_GUI_DRAWS, PARAM_GUI_FD_CALLS, PARAM_GUI_HEIGHT, PARAM_GUI_OPEN, PARAM_GUI_PARENT,
    PARAM_GUI_RESIZE_ASKED, PARAM_GUI_SCALE, PARAM_GUI_TIMER_CALLS, PARAM_GUI_WIDTH,
    PARAM_GUI_WINDOW,
};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize, HostGui, Window};
use clack_extensions::posix_fd::{FdFlags, HostPosixFd};
use clack_plugin::prelude::PluginError;
use std::os::unix::io::{AsRawFd, RawFd};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, Rectangle,
    WindowClass,
};
use x11rb::rust_connection::RustConnection;

/// What the GUI kind's editor holds and has seen (main thread only).
pub struct Gui {
    conn: Option<RustConnection>,
    /// Its window and graphics context, once parented.
    window: u32,
    gc: u32,
    parent: u32,
    shown: bool,
    scale: f64,
    /// In physical pixels.
    size: (u32, u32),
    timer: Option<u32>,
    fd: Option<RawFd>,
    create_timer: f64,
    create_fd: f64,
    resize_asked: f64,
    draws: u32,
    timer_calls: u32,
    fd_calls: u32,
}

impl Default for Gui {
    fn default() -> Gui {
        Gui {
            conn: None,
            window: 0,
            gc: 0,
            parent: 0,
            shown: false,
            scale: 0.0,
            size: BASE_SIZE,
            timer: None,
            fd: None,
            create_timer: -2.0,
            create_fd: -2.0,
            resize_asked: -1.0,
            draws: 0,
            timer_calls: 0,
            fd_calls: 0,
        }
    }
}

fn x_err<E>(_: E) -> PluginError {
    PluginError::Message("X11 request failed")
}

fn px(points: u32, scale: f64) -> u32 {
    (f64::from(points) * scale).round() as u32
}

fn embedded_x11(cfg: &GuiConfiguration) -> bool {
    !cfg.is_floating && cfg.api_type == GuiApiType::X11
}

impl Gui {
    fn scale(&self) -> f64 {
        if self.scale > 0.0 { self.scale } else { 1.0 }
    }

    /// Reads every queued X event; true if one asks for a redraw.
    fn drain(&mut self) -> bool {
        let Some(conn) = &self.conn else {
            return false;
        };
        let mut redraw = false;
        while let Ok(Some(ev)) = conn.poll_for_event() {
            redraw |= matches!(ev, Event::Expose(_) | Event::ConfigureNotify(_));
        }
        redraw
    }

    /// Fills the window with [`GUI_COLOR`].
    fn draw(&mut self) {
        let Some(conn) = &self.conn else {
            return;
        };
        if self.window == 0 || !self.shown {
            return;
        }
        let rect = Rectangle {
            x: 0,
            y: 0,
            width: self.size.0.min(u32::from(u16::MAX)) as u16,
            height: self.size.1.min(u32::from(u16::MAX)) as u16,
        };
        if conn
            .poly_fill_rectangle(self.window, self.gc, &[rect])
            .is_ok()
        {
            self.draws += 1;
        }
        let _ = conn.flush();
    }

    /// Frees the window and the connection (the descriptor closes with
    /// it).
    fn tear_down(&mut self) {
        if let Some(conn) = self.conn.take() {
            if self.window != 0 {
                let _ = conn.free_gc(self.gc);
                let _ = conn.destroy_window(self.window);
            }
            let _ = conn.flush();
        }
        self.window = 0;
        self.gc = 0;
        self.shown = false;
    }
}

impl MainThread<'_> {
    pub(crate) fn gui_readout(&self, id: u32) -> Option<f64> {
        let g = self.gui.borrow();
        Some(match id {
            PARAM_GUI_SCALE => g.scale,
            PARAM_GUI_WIDTH => f64::from(g.size.0),
            PARAM_GUI_HEIGHT => f64::from(g.size.1),
            PARAM_GUI_OPEN => f64::from(u8::from(g.conn.is_some())),
            PARAM_GUI_CREATE_TIMER => g.create_timer,
            PARAM_GUI_CREATE_FD => g.create_fd,
            PARAM_GUI_WINDOW => f64::from(g.window),
            PARAM_GUI_PARENT => f64::from(g.parent),
            PARAM_GUI_DRAWS => f64::from(g.draws),
            PARAM_GUI_TIMER_CALLS => f64::from(g.timer_calls),
            PARAM_GUI_FD_CALLS => f64::from(g.fd_calls),
            PARAM_GUI_RESIZE_ASKED => g.resize_asked,
            _ => return None,
        })
    }

    pub(crate) fn gui_supported(&self, cfg: &GuiConfiguration) -> bool {
        embedded_x11(cfg)
    }

    pub(crate) fn gui_create(&self, cfg: &GuiConfiguration) -> Result<(), PluginError> {
        if !embedded_x11(cfg) {
            return Err(PluginError::Message("embedded X11 only"));
        }
        self.gui_destroy();
        let (conn, _screen) =
            RustConnection::connect(None).map_err(|_| PluginError::Message("no X display"))?;
        let fd = conn.stream().as_raw_fd();
        // Both before the window exists, as DPF and JUCE editors do.
        let timer = self.register_timer(GUI_TIMER_MS);
        let fd_ok = self.host.get_extension::<HostPosixFd>().is_some_and(|x| {
            x.register_fd(&self.host, fd, FdFlags::READ | FdFlags::ERROR)
                .is_ok()
        });
        *self.gui.borrow_mut() = Gui {
            conn: Some(conn),
            timer,
            fd: fd_ok.then_some(fd),
            create_timer: timer.map_or(-1.0, f64::from),
            create_fd: f64::from(u8::from(fd_ok)),
            ..Gui::default()
        };
        Ok(())
    }

    pub(crate) fn gui_destroy(&self) {
        let (timer, fd) = {
            let mut g = self.gui.borrow_mut();
            g.tear_down();
            (g.timer.take(), g.fd.take())
        };
        if let Some(t) = timer {
            self.unregister_timer(t);
        }
        if let (Some(fd), Some(x)) = (fd, self.host.get_extension::<HostPosixFd>()) {
            let _ = x.unregister_fd(&self.host, fd);
        }
    }

    pub(crate) fn gui_set_scale(&self, scale: f64) -> Result<(), PluginError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(PluginError::Message("bad scale"));
        }
        let mut g = self.gui.borrow_mut();
        if g.window != 0 {
            return Err(PluginError::Message("already parented"));
        }
        g.scale = scale;
        g.size = (px(BASE_SIZE.0, scale), px(BASE_SIZE.1, scale));
        Ok(())
    }

    pub(crate) fn gui_get_size(&self) -> Option<GuiSize> {
        let g = self.gui.borrow();
        Some(GuiSize {
            width: g.size.0,
            height: g.size.1,
        })
    }

    pub(crate) fn gui_set_parent(&self, window: Window) -> Result<(), PluginError> {
        let parent = window
            .as_x11_handle()
            .ok_or(PluginError::Message("not an X11 window"))?;
        let parent = u32::try_from(parent).map_err(x_err)?;
        let mut g = self.gui.borrow_mut();
        let g = &mut *g;
        let conn = g.conn.as_ref().ok_or(PluginError::Message("not created"))?;
        if g.window != 0 {
            return Err(PluginError::Message("already parented"));
        }
        let wid = conn.generate_id().map_err(x_err)?;
        let gc = conn.generate_id().map_err(x_err)?;
        let (w, h) = g.size;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            wid,
            parent,
            0,
            0,
            w.clamp(1, u32::from(u16::MAX)) as u16,
            h.clamp(1, u32::from(u16::MAX)) as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new()
                .background_pixel(0)
                .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY),
        )
        .map_err(x_err)?
        .check()
        .map_err(|_| PluginError::Message("bad parent window"))?;
        conn.create_gc(gc, wid, &CreateGCAux::new().foreground(GUI_COLOR))
            .map_err(x_err)?;
        conn.flush().map_err(x_err)?;
        g.window = wid;
        g.gc = gc;
        g.parent = parent;
        Ok(())
    }

    pub(crate) fn gui_show(&self, show: bool) -> Result<(), PluginError> {
        let mut g = self.gui.borrow_mut();
        let window = g.window;
        let conn = g.conn.as_ref().ok_or(PluginError::Message("not created"))?;
        if window == 0 {
            return Err(PluginError::Message("no parent"));
        }
        if show {
            conn.map_window(window).map_err(x_err)?;
        } else {
            conn.unmap_window(window).map_err(x_err)?;
        }
        conn.flush().map_err(x_err)?;
        g.shown = show;
        Ok(())
    }

    /// The timer: reads events, asks once to grow, redraws.
    pub(crate) fn gui_on_timer(&self, id: u32) {
        let ask = {
            let mut g = self.gui.borrow_mut();
            if g.timer != Some(id) {
                return;
            }
            g.timer_calls += 1;
            g.drain();
            (g.shown && g.resize_asked < 0.0).then(|| {
                let s = g.scale();
                (g.size.0 + px(GUI_GROW.0, s), g.size.1 + px(GUI_GROW.1, s))
            })
        };
        if let Some((w, h)) = ask {
            // No borrow held: the host may call back into the editor.
            let ok = self
                .host
                .get_extension::<HostGui>()
                .is_some_and(|x| x.request_resize(&self.host, w, h).is_ok());
            let mut g = self.gui.borrow_mut();
            g.resize_asked = f64::from(u8::from(ok));
            if ok {
                // A fixed-size editor takes the size it asked for itself.
                if let Some(conn) = &g.conn {
                    let _ = conn
                        .configure_window(g.window, &ConfigureWindowAux::new().width(w).height(h));
                }
                g.size = (w, h);
            }
        }
        self.gui.borrow_mut().draw();
    }

    /// The X connection's descriptor: reads the events, redraws on expose.
    pub(crate) fn gui_on_fd(&self, fd: i32) {
        let mut g = self.gui.borrow_mut();
        if g.fd != Some(fd) {
            return;
        }
        g.fd_calls += 1;
        if g.drain() {
            g.draw();
        }
    }
}
