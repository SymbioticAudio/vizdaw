//! CLAP editors embed with the platform's native window API (GPUI spike
//! Task 6): Cocoa on macOS, X11 on Linux (also under XWayland).
use clack_extensions::gui::GuiApiType;

#[test]
fn editor_api_is_platform_native() {
    let api = core_host::clap::editor_api();
    #[cfg(target_os = "macos")]
    assert_eq!(api, Some(GuiApiType::COCOA));
    #[cfg(target_os = "linux")]
    assert_eq!(api, Some(GuiApiType::X11));
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    assert_eq!(api, None);
}
