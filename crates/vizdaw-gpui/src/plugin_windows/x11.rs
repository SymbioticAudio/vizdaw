//! The X11 requests of the Linux editor host, on one process-wide `x11rb`
//! connection of our own, made on the main thread at first use (plug-in
//! windows open and close there only). Sizes here are device pixels.
//!
//! The container is made with the root window's depth and visual, the
//! screen's default colormap and an explicit border pixel: GPUI's window
//! uses a 32-bit transparent visual of its own, and a child of another
//! depth needs both set or the server refuses it.

use std::cell::OnceCell;

use x11rb::connection::Connection;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, PropMode, Screen,
    WindowClass,
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

/// An unmapped container window inside `parent` at (`x`, `y`), `width` ×
/// `height`; its id. It shows only once [`show`] maps it (an editor is
/// open): a mapped X child hides its parent's content and takes the
/// pointer.
pub fn create_container(
    parent: u32,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
) -> Result<u32, String> {
    with(|c, screen| create_on(c, screen, parent, (x, y), (width, height)))
}

/// [`create_container`] on connection `c`.
fn create_on(
    c: &impl Connection,
    screen: &Screen,
    parent: u32,
    (x, y): (i16, i16),
    (width, height): (u16, u16),
) -> Result<u32, String> {
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
    Ok(id)
}

/// Moves and sizes `window` inside its parent, then maps it (an editor is
/// open in it).
pub fn show(window: u32, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
    with(|c, _| show_on(c, window, (x, y), (width, height)))
}

/// [`show`] on connection `c`.
fn show_on(
    c: &impl Connection,
    window: u32,
    (x, y): (i32, i32),
    (width, height): (u32, u32),
) -> Result<(), String> {
    let aux = ConfigureWindowAux::new()
        .x(x)
        .y(y)
        .width(width.max(1))
        .height(height.max(1));
    c.configure_window(window, &aux).map_err(msg)?;
    c.map_window(window).map_err(msg)?;
    Ok(())
}

/// Unmaps `window` (no editor is open in it): its parent's content shows
/// and gets the pointer again.
pub fn hide(window: u32) -> Result<(), String> {
    with(|c, _| hide_on(c, window))
}

/// [`hide`] on connection `c`.
fn hide_on(c: &impl Connection, window: u32) -> Result<(), String> {
    c.unmap_window(window).map_err(msg)?;
    Ok(())
}

/// Takes `window` out of its parent (to the root) and hides it: it
/// survives its old parent's destruction. Synchronous: GPUI destroys the
/// old parent on its own connection right after, and the server must have
/// done the reparent first (else it destroys the container with its
/// parent while the plug-in still draws in it: the BadWindow exit).
pub fn park(window: u32) -> Result<(), String> {
    with(|c, screen| park_on(c, screen.root, window))
}

/// [`park`] on connection `c`: returns once the server has done it.
fn park_on(c: &impl Connection, root: u32, window: u32) -> Result<(), String> {
    c.unmap_window(window).map_err(msg)?;
    c.reparent_window(window, root, 0, 0)
        .map_err(msg)?
        .check()
        .map_err(msg)?;
    Ok(())
}

/// Destroys `window` (and whatever is still inside it). Synchronous, as
/// [`park`]: the container is gone before GPUI destroys its window.
pub fn destroy(window: u32) -> Result<(), String> {
    with(|c, _| {
        c.destroy_window(window)
            .map_err(msg)?
            .check()
            .map_err(msg)?;
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

#[cfg(test)]
mod tests {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, MapState, WindowClass};

    /// A display to test on: `None` (skipped) without one, unless
    /// `VIZDAW_REQUIRE_DISPLAY` is set.
    fn connect() -> Option<(x11rb::rust_connection::RustConnection, usize)> {
        match x11rb::connect(None) {
            Ok(c) => Some(c),
            Err(e) => {
                assert!(
                    std::env::var_os("VIZDAW_REQUIRE_DISPLAY").is_none(),
                    "VIZDAW_REQUIRE_DISPLAY is set but there is no X display: {e}"
                );
                None
            }
        }
    }

    /// GPUI's side: a parent window on a connection of its own, destroyed
    /// right after the park with no wait. The parked container survives it
    /// (the park was done by the server before `park_on` returned).
    #[test]
    fn a_parked_container_survives_its_parent_destroyed_on_another_connection() {
        let Some((ours, n)) = connect() else {
            return;
        };
        let Some((gpui, _)) = connect() else {
            return;
        };
        let screen = &ours.setup().roots[n];
        let root = screen.root;
        let parent = gpui.generate_id().unwrap();
        gpui.create_window(
            screen.root_depth,
            parent,
            root,
            0,
            0,
            200,
            200,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
        let container = ours.generate_id().unwrap();
        ours.create_window(
            screen.root_depth,
            container,
            parent,
            0,
            37,
            100,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
        super::park_on(&ours, root, container).unwrap();
        gpui.destroy_window(parent).unwrap();
        gpui.flush().unwrap();
        // GPUI's destroy has been processed once its round trip returns.
        gpui.get_input_focus().unwrap().reply().unwrap();
        let tree = ours.query_tree(container).unwrap().reply();
        assert_eq!(tree.expect("the container is alive").parent, root);
        ours.destroy_window(container).unwrap().check().unwrap();
    }

    /// Final review I1: the container is made unmapped (the parameter
    /// list and the loading text under it show and take the clicks), is
    /// mapped at the editor's frame once an editor opens, and is unmapped
    /// again when it closes.
    #[test]
    fn the_container_is_mapped_only_while_an_editor_is_open() {
        let Some((ours, n)) = connect() else {
            return;
        };
        let Some((gpui, _)) = connect() else {
            return;
        };
        let screen = ours.setup().roots[n].clone();
        let parent = gpui.generate_id().unwrap();
        gpui.create_window(
            screen.root_depth,
            parent,
            screen.root,
            0,
            0,
            420,
            360,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
        gpui.map_window(parent).unwrap();
        gpui.flush().unwrap();
        gpui.get_input_focus().unwrap().reply().unwrap();
        let state = |w: u32| {
            ours.get_window_attributes(w)
                .unwrap()
                .reply()
                .unwrap()
                .map_state
        };
        let container = super::create_on(&ours, &screen, parent, (0, 37), (420, 323)).unwrap();
        assert_eq!(
            state(container),
            MapState::UNMAPPED,
            "hidden until an editor opens"
        );
        super::show_on(&ours, container, (0, 37), (300, 200)).unwrap();
        assert_ne!(
            state(container),
            MapState::UNMAPPED,
            "shown with the editor"
        );
        let g = ours.get_geometry(container).unwrap().reply().unwrap();
        assert_eq!((g.x, g.y, g.width, g.height), (0, 37, 300, 200));
        super::hide_on(&ours, container).unwrap();
        assert_eq!(
            state(container),
            MapState::UNMAPPED,
            "hidden after the close"
        );
        ours.destroy_window(container).unwrap().check().unwrap();
        gpui.destroy_window(parent).unwrap().check().unwrap();
    }
}
