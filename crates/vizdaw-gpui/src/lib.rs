//! The VizDAW GPUI application.

pub mod shell;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};

/// Open the application with one empty document window.
pub fn run() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| shell::DocumentWindow::empty(window, cx)),
        )
        .expect("open the document window");
    });
}
