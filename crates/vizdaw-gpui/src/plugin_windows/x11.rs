//! The X11 requests of the Linux editor host, on one process-wide `x11rb`
//! connection of our own, made on the main thread at first use (plug-in
//! windows open and close there only). Sizes here are device pixels.
//!
//! The container is made with the root window's depth and visual, the
//! screen's default colormap and an explicit border pixel: GPUI's window
//! uses a 32-bit transparent visual of its own, and a child of another
//! depth needs both set or the server refuses it.

use std::cell::OnceCell;

use x11rb::connection::Connection as _;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, PropMode, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

struct Conn {
    conn: RustConnection,
    screen: usize,
}

thread_local! {
    static CONN: OnceCell<Option<Conn>> = const { OnceCell::new() };
}

/// Runs `f` on the connection; an error is its message.
fn with<R>(
    f: impl FnOnce(&RustConnection, &x11rb::protocol::xproto::Screen) -> Result<R, String>,
) -> Result<R, String> {
    CONN.with(|cell| {
        let c = cell.get_or_init(|| match x11rb::connect(None) {
            Ok((conn, screen)) => Some(Conn { conn, screen }),
            Err(e) => {
                eprintln!("plug-in windows: no X11 connection: {e}");
                None
            }
        });
        let c = c.as_ref().ok_or("no X11 connection")?;
        let screen = c.conn.setup().roots.get(c.screen).ok_or("no X11 screen")?;
        let r = f(&c.conn, screen)?;
        c.conn.flush().map_err(|e| e.to_string())?;
        Ok(r)
    })
}

fn msg(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A mapped container window inside `parent` at (`x`, `y`), `width` ×
/// `height`; its id.
pub fn create_container(
    parent: u32,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
) -> Result<u32, String> {
    with(|c, screen| {
        let id = c.generate_id().map_err(msg)?;
        let aux = CreateWindowAux::new()
            .background_pixel(screen.black_pixel)
            .border_pixel(screen.black_pixel)
            .colormap(screen.default_colormap);
        c.create_window(
            screen.root_depth,
            id,
            parent,
            x,
            y,
            width.max(1),
            height.max(1),
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &aux,
        )
        .map_err(msg)?
        .check()
        .map_err(msg)?;
        c.map_window(id).map_err(msg)?;
        Ok(id)
    })
}

/// Moves and sizes `window` inside its parent.
pub fn set_frame(window: u32, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
    with(|c, _| {
        let aux = ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(width.max(1))
            .height(height.max(1));
        c.configure_window(window, &aux).map_err(msg)?;
        Ok(())
    })
}

/// Takes `window` out of its parent (to the root) and hides it: it
/// survives its old parent's destruction.
pub fn park(window: u32) -> Result<(), String> {
    with(|c, screen| {
        c.unmap_window(window).map_err(msg)?;
        c.reparent_window(window, screen.root, 0, 0).map_err(msg)?;
        Ok(())
    })
}

/// Destroys `window` (and whatever is still inside it).
pub fn destroy(window: u32) -> Result<(), String> {
    with(|c, _| {
        c.destroy_window(window).map_err(msg)?;
        Ok(())
    })
}

/// `WM_TRANSIENT_FOR` of `window` is `owner`.
pub fn set_transient_for(window: u32, owner: u32) -> Result<(), String> {
    with(|c, _| {
        c.change_property32(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_TRANSIENT_FOR,
            AtomEnum::WINDOW,
            &[owner],
        )
        .map_err(msg)?;
        Ok(())
    })
}

/// `WM_NORMAL_HINTS` of `window`: a fixed size (`min` = `max` = `size`), or
/// any size from `min` up.
pub fn set_size_hints(
    window: u32,
    resizable: bool,
    size: (i32, i32),
    min: (i32, i32),
) -> Result<(), String> {
    with(|c, _| {
        let mut hints = WmSizeHints::new();
        if resizable {
            hints.min_size = Some(min);
        } else {
            hints.min_size = Some(size);
            hints.max_size = Some(size);
        }
        hints.set_normal_hints(c, window).map_err(msg)?;
        Ok(())
    })
}
