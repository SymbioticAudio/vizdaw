//! Moving a file to the Trash (sub-project 5 spec §6.5; plan 5b
//! Ruling 11): `-[NSFileManager trashItemAtURL:resultingItemURL:error:]`,
//! so a media file VizDAW no longer needs stays recoverable.

use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::{CFString, CFURL};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

// `NSFileManager` lives in Foundation.
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

/// Moves the file at `path` to the Trash of its volume. The error names
/// the path and, when Foundation gives one, its `NSError`'s
/// `localizedDescription` (preflight M14).
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{}: no such file", path.display()));
    }
    let cls = AnyClass::get(c"NSFileManager").ok_or("Foundation is not loaded")?;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` is a valid buffer of the given length.
    let url = unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .ok_or_else(|| format!("{}: not a file URL", path.display()))?;
    // A CFURL is toll-free bridged to an NSURL.
    // SAFETY: `url` is a live CFURL for the whole call.
    let ns_url: &AnyObject = unsafe { &*(&*url as *const CFURL).cast::<AnyObject>() };
    // The file manager's autoreleased objects (the error among them) are
    // released when the pool drains, on whichever thread this runs.
    autoreleasepool(|_| {
        let mut error: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: `defaultManager` returns the shared file manager; the
        // call takes a valid NSURL, a null `resultingItemURL` (unwanted)
        // and a pointer to `error`, which outlives the call.
        let ok: bool = unsafe {
            let fm: Option<Retained<AnyObject>> = msg_send![cls, defaultManager];
            let fm = fm.ok_or("no file manager")?;
            msg_send![
                &*fm,
                trashItemAtURL: ns_url,
                resultingItemURL: std::ptr::null_mut::<*mut AnyObject>(),
                error: &raw mut error
            ]
        };
        if ok {
            return Ok(());
        }
        let why = if error.is_null() {
            None
        } else {
            // SAFETY: on failure `error` is an autoreleased NSError, alive
            // until the pool drains; `localizedDescription` returns an
            // NSString, toll-free bridged to a CFString.
            unsafe {
                let d: Option<Retained<AnyObject>> = msg_send![error, localizedDescription];
                d.map(|d| (*(&*d as *const AnyObject).cast::<CFString>()).to_string())
            }
        };
        Err(match why {
            Some(w) => format!("{}: the Trash refused it ({w})", path.display()),
            None => format!("{}: the Trash refused it", path.display()),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_fails_before_reaching_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let e = move_to_trash(&dir.path().join("nothing.caf")).unwrap_err();
        assert!(e.contains("no such file"), "{e}");
    }

    /// Moves a uniquely named empty file to the real Trash: run by hand
    /// (README step 125), never on CI.
    #[test]
    #[ignore = "moves a temp file to the user's real Trash"]
    fn a_file_goes_to_the_real_trash() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir
            .path()
            .join(format!("vizdaw-trash-check-{}.txt", std::process::id()));
        std::fs::write(&f, "VizDAW Trash check; safe to delete").unwrap();
        move_to_trash(&f).unwrap();
        assert!(!f.exists());
    }
}
