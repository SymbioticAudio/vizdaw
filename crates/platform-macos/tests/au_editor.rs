//! AU editors (spec §9) against Apple's built-in units, on the process main
//! thread as AppKit requires (this test has its own `main`): AUDelay brings
//! its own Cocoa view, AUMatrixReverb gets CoreAudioKit's generic view, a
//! closed or dropped editor leaves the window, and another thread cannot
//! open one.

#[cfg(target_os = "macos")]
fn main() {
    use core_host::{ParentView, PluginInstance};
    use objc2::msg_send;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use platform_macos::au;

    fn subviews(parent: &AnyObject) -> usize {
        // SAFETY: `subviews` of a live NSView is an NSArray.
        unsafe {
            let a: Retained<AnyObject> = msg_send![parent, subviews];
            msg_send![&a, count]
        }
    }

    // The window's content view stand-in.
    let cls = AnyClass::get(c"NSView").unwrap();
    let frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 800.0,
            height: 600.0,
        },
    };
    // SAFETY: an NSView made on the main thread.
    let parent: Retained<AnyObject> = unsafe {
        let a: Allocated<AnyObject> = msg_send![cls, alloc];
        msg_send![a, initWithFrame: frame]
    };
    let raw = Retained::as_ptr(&parent).cast_mut().cast();
    // SAFETY: `parent` lives until the end of `main`, after every editor.
    let view = || unsafe { ParentView::new(raw) };

    let mut delay = au::load("aufx/dely/appl", 48_000.0, 512).unwrap();
    let (w, h) = delay.editor().unwrap().open(view()).unwrap();
    assert!(w > 50 && h > 50, "AUDelay's own view: {w}×{h}");
    assert_eq!(delay.au_editor().view_kind(), Some("cocoa"));
    assert_eq!(subviews(&parent), 1);
    assert!(!delay.editor().unwrap().can_resize());
    delay.editor().unwrap().close();
    assert_eq!(subviews(&parent), 0, "close removes the view");
    println!("au_editor: AUDelay Cocoa view {w}×{h}");

    let mut reverb = au::load("aufx/mrev/appl", 48_000.0, 512).unwrap();
    let (w, h) = reverb.editor().unwrap().open(view()).unwrap();
    assert!(w > 0 && h > 0);
    assert_eq!(reverb.au_editor().view_kind(), Some("generic"));
    assert_eq!(subviews(&parent), 1);
    drop(reverb);
    assert_eq!(subviews(&parent), 0, "dropping the unit removes its view");
    println!("au_editor: AUMatrixReverb generic view {w}×{h}");

    let addr = raw as usize;
    let off_main = std::thread::spawn(move || {
        let mut d = au::load("aufx/dely/appl", 48_000.0, 512).unwrap();
        // SAFETY: never attached: refused before the view is touched.
        let p = unsafe { ParentView::new(addr as *mut std::ffi::c_void) };
        d.editor()
            .unwrap()
            .open(p)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .join()
    .unwrap();
    assert!(
        off_main.as_ref().is_err_and(|e| e.contains("main thread")),
        "{off_main:?}"
    );
    assert_eq!(subviews(&parent), 0);
    println!("au_editor: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
