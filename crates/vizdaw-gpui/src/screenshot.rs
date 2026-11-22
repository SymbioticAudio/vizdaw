//! `ui.screenshot`: a PNG of a document window, written atomically.
//!
//! macOS draws the window's last frame offscreen with GPUI's
//! `Window::render_to_image`, which this GPUI pin compiles only with its
//! `test-support` feature: the crate's `screenshot` feature turns that on
//! for the app build (it adds test helpers and no behaviour; see the
//! slice 3 Task 5 report). Without the feature a macOS screenshot answers
//! `unavailable`. Linux reads the window's pixels from the X server
//! (`GetImage`, which works under XWayland and Xvfb); a native Wayland
//! window answers `unavailable`.
//!
//! Tests replace the capture with [`set_capture`] (GPUI's test platform
//! draws nothing).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use core_control::{ControlError, ErrorCode};
use gpui::{App, Global, Window};
use image::ImageEncoder as _;

/// A captured window: `width` × `height` pixels, RGBA, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Captures `window` at `scale` pixels per point (`None`: its own).
pub type Capture = dyn Fn(&mut Window, Option<f64>) -> Result<Shot, ControlError>;

struct CaptureHook(Rc<Capture>);

impl Global for CaptureHook {}

/// Why a Linux window that is not an X11 one cannot be captured.
pub const NEEDS_X11: &str = "Screenshots need an X11 window on Linux.";

/// Why a window with no native handle (GPUI's test window) cannot be
/// captured.
pub const NO_HANDLE: &str = "The window has no native handle to capture.";

/// A window handle that cannot be had: nothing to capture, `unavailable`
/// (not a failed capture; those are the X server's errors).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn handle_error(_: raw_window_handle::HandleError) -> ControlError {
    ControlError::new(ErrorCode::Unavailable, NO_HANDLE)
}

/// Replaces the platform capture (tests).
pub fn set_capture(
    cx: &mut App,
    capture: impl Fn(&mut Window, Option<f64>) -> Result<Shot, ControlError> + 'static,
) {
    cx.set_global(CaptureHook(Rc::new(capture)));
}

/// The capture in use: the test's, else the platform's.
pub(crate) fn capture_fn(cx: &App) -> Rc<Capture> {
    match cx.try_global::<CaptureHook>() {
        Some(h) => h.0.clone(),
        None => Rc::new(platform_capture),
    }
}

fn failed(message: impl Into<String>) -> ControlError {
    ControlError::new(ErrorCode::Failed, message)
}

fn not_rendered() -> ControlError {
    failed("the window could not be rendered")
}

/// The platform's capture of `window`.
fn platform_capture(window: &mut Window, scale: Option<f64>) -> Result<Shot, ControlError> {
    let shot = native_capture(window)?;
    let own = f64::from(window.scale_factor());
    match scale {
        Some(s) if (s - own).abs() > 1e-6 && own > 0.0 => rescale(&shot, s / own),
        _ => Ok(shot),
    }
}

#[cfg(all(target_os = "macos", feature = "screenshot"))]
fn native_capture(window: &mut Window) -> Result<Shot, ControlError> {
    let image = window.render_to_image().map_err(|e| {
        eprintln!("ui.screenshot: {e:#}");
        not_rendered()
    })?;
    Ok(Shot {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

#[cfg(all(target_os = "macos", not(feature = "screenshot")))]
fn native_capture(_: &mut Window) -> Result<Shot, ControlError> {
    Err(ControlError::new(
        ErrorCode::Unavailable,
        "Screenshots need the app built with the `screenshot` feature (scripts/make-gpui-app.sh \
         builds it so).",
    ))
}

#[cfg(target_os = "linux")]
fn native_capture(window: &mut Window) -> Result<Shot, ControlError> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // The trait's, not `Window::window_handle` (GPUI's window handle).
    let handle = HasWindowHandle::window_handle(&*window).map_err(handle_error)?;
    let id = match handle.as_raw() {
        RawWindowHandle::Xcb(h) => h.window.get(),
        RawWindowHandle::Xlib(h) => u32::try_from(h.window).map_err(|_| not_rendered())?,
        _ => return Err(ControlError::new(ErrorCode::Unavailable, NEEDS_X11)),
    };
    x11::capture(id)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn native_capture(_: &mut Window) -> Result<Shot, ControlError> {
    Err(ControlError::new(
        ErrorCode::Unavailable,
        "Screenshots are not available on this platform.",
    ))
}

#[cfg(target_os = "linux")]
mod x11 {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, ImageOrder};

    use super::{ControlError, Shot, not_rendered, zpixmap_to_rgba};

    /// The pixels of X11 window `id`, read over a connection of our own
    /// (`DISPLAY`, `XAUTHORITY`).
    pub(super) fn capture(id: u32) -> Result<Shot, ControlError> {
        let fail = |what: &str, e: &dyn std::fmt::Display| {
            eprintln!("ui.screenshot: {what}: {e}");
            not_rendered()
        };
        let (conn, _) = x11rb::connect(None).map_err(|e| fail("connect", &e))?;
        let geometry = conn
            .get_geometry(id)
            .map_err(|e| fail("get_geometry", &e))?
            .reply()
            .map_err(|e| fail("get_geometry", &e))?;
        let image = conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                id,
                0,
                0,
                geometry.width,
                geometry.height,
                !0,
            )
            .map_err(|e| fail("get_image", &e))?
            .reply()
            .map_err(|e| fail("get_image", &e))?;
        let setup = conn.setup();
        let format = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == image.depth)
            .ok_or_else(not_rendered)?;
        zpixmap_to_rgba(
            &image.data,
            u32::from(geometry.width),
            u32::from(geometry.height),
            format.bits_per_pixel,
            format.scanline_pad,
            setup.image_byte_order == ImageOrder::LSB_FIRST,
        )
        .ok_or_else(not_rendered)
    }
}

/// An X11 `ZPixmap` of 24-bit colour in 32-bit pixels as RGBA (opaque);
/// `None` for another pixel size or too little data.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn zpixmap_to_rgba(
    data: &[u8],
    width: u32,
    height: u32,
    bits_per_pixel: u8,
    scanline_pad: u8,
    lsb_first: bool,
) -> Option<Shot> {
    if bits_per_pixel != 32 || width == 0 || height == 0 {
        return None;
    }
    let pad = usize::from(scanline_pad.max(8));
    let row_bits = width as usize * 32;
    let stride = row_bits.div_ceil(pad) * pad / 8;
    if data.len() < stride * height as usize {
        return None;
    }
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in data.chunks_exact(stride).take(height as usize) {
        for px in row[..width as usize * 4].chunks_exact(4) {
            // A pixel is 0x00RRGGBB in the server's byte order.
            let (r, g, b) = if lsb_first {
                (px[2], px[1], px[0])
            } else {
                (px[1], px[2], px[3])
            };
            rgba.extend_from_slice(&[r, g, b, 0xff]);
        }
    }
    Some(Shot {
        width,
        height,
        rgba,
    })
}

/// `shot` resized by `factor` (a `scale` other than the window's own).
fn rescale(shot: &Shot, factor: f64) -> Result<Shot, ControlError> {
    let image = image::RgbaImage::from_raw(shot.width, shot.height, shot.rgba.clone())
        .ok_or_else(not_rendered)?;
    let width = ((f64::from(shot.width) * factor).round() as u32).max(1);
    let height = ((f64::from(shot.height) * factor).round() as u32).max(1);
    let resized =
        image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle);
    Ok(Shot {
        width,
        height,
        rgba: resized.into_raw(),
    })
}

/// Writes `shot` to `path` as a PNG: to a temporary file beside it, then
/// renamed over it, so a reader never sees half a file.
pub fn write_png(shot: &Shot, path: &Path) -> Result<(), ControlError> {
    let cannot =
        |e: &dyn std::fmt::Display| failed(format!("cannot write {}: {e}", path.display()));
    let image = image::RgbaImage::from_raw(shot.width, shot.height, shot.rgba.clone())
        .ok_or_else(not_rendered)?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            shot.width,
            shot.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| cannot(&e))?;
    let temp = temp_path(path);
    let written = std::fs::File::create(&temp)
        .and_then(|mut f| f.write_all(&png).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&temp, path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(cannot(&e));
    }
    Ok(())
}

/// A hidden file beside `path`, unique to this process.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zpixmap_reads_as_opaque_rgba() {
        // 2 × 1, LSB first: BGRX.
        let data = [0x10, 0x20, 0x30, 0x00, 0x40, 0x50, 0x60, 0x00];
        let shot = zpixmap_to_rgba(&data, 2, 1, 32, 32, true).unwrap();
        assert_eq!(shot.rgba, [0x30, 0x20, 0x10, 0xff, 0x60, 0x50, 0x40, 0xff]);
        // MSB first: XRGB.
        let shot = zpixmap_to_rgba(&data, 2, 1, 32, 32, false).unwrap();
        assert_eq!(shot.rgba[..4], [0x20, 0x30, 0x00, 0xff]);
        // Too little data, or 16-bit pixels: not read.
        assert_eq!(zpixmap_to_rgba(&data[..4], 2, 1, 32, 32, true), None);
        assert_eq!(zpixmap_to_rgba(&data, 2, 1, 16, 32, true), None);
    }

    #[test]
    fn a_window_without_a_native_handle_is_unavailable() {
        use raw_window_handle::HandleError;
        for e in [HandleError::NotSupported, HandleError::Unavailable] {
            let c = handle_error(e);
            assert_eq!(c.code, ErrorCode::Unavailable);
            assert_eq!(c.message, NO_HANDLE);
        }
    }

    #[test]
    fn a_rescaled_shot_has_the_scaled_size() {
        let shot = Shot {
            width: 4,
            height: 2,
            rgba: vec![0; 32],
        };
        let half = rescale(&shot, 0.5).unwrap();
        assert_eq!((half.width, half.height, half.rgba.len()), (2, 1, 8));
    }
}
