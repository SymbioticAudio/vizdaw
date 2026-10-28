//! Microphone permission (sub-project 5 spec §6.1): whether macOS lets
//! VizDAW open audio input, read with `+[AVCaptureDevice
//! authorizationStatusForMediaType:]`. Reading the status never shows the
//! permission prompt; only opening input for the first time does, and the
//! app's `Info.plist` carries `NSMicrophoneUsageDescription` for it.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    /// `AVMediaTypeAudio`, an `NSString`.
    static AVMediaTypeAudio: &'static AnyObject;
}

/// What macOS says about audio input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAccess {
    /// The user allowed it.
    Granted,
    /// The user (or a policy) refused it: input reads silence.
    Denied,
    /// Not asked yet: the first input open asks.
    Undetermined,
}

/// `AVAuthorizationStatus` as a [`MicAccess`]: 3 authorized, 1 restricted
/// and 2 denied, anything else not determined.
pub fn access_of(status: isize) -> MicAccess {
    match status {
        3 => MicAccess::Granted,
        1 | 2 => MicAccess::Denied,
        _ => MicAccess::Undetermined,
    }
}

/// The microphone permission now. Never asks.
pub fn microphone() -> MicAccess {
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
        return MicAccess::Undetermined;
    };
    // SAFETY: `authorizationStatusForMediaType:` is a class method taking
    // an `AVMediaType` (an NSString, here the framework's own constant)
    // and returning an `NSInteger`; it has no side effects.
    let status: isize =
        unsafe { msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio] };
    access_of(status)
}

/// Asks macOS for microphone access when it was never asked (plan 5d-1
/// Ruling 14), and waits up to `limit` for the user's answer to the
/// system prompt; the access then. Returns at once when the access is
/// already known. Blocks for as long as the user takes: never call it on
/// the main thread (the host's input thread does). Tests never call it.
pub fn request_microphone(limit: std::time::Duration) -> MicAccess {
    let now = microphone();
    if now != MicAccess::Undetermined {
        return now;
    }
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
        return now;
    };
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let tx = std::sync::Mutex::new(Some(tx));
    let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
        let sender = tx.lock().ok().and_then(|mut t| t.take());
        if let Some(sender) = sender {
            let _ = sender.send(granted.as_bool());
        }
    });
    // SAFETY: `requestAccessForMediaType:completionHandler:` is a class
    // method taking an `AVMediaType` (the framework's own NSString
    // constant) and a block it calls once with a BOOL, on any queue; the
    // block owns its sender and is retained by the framework until then.
    let _: () = unsafe {
        msg_send![cls, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*block]
    };
    match rx.recv_timeout(limit) {
        Ok(true) => MicAccess::Granted,
        Ok(false) => MicAccess::Denied,
        Err(_) => microphone(),
    }
}

/// The message arming an audio lane shows when input is refused (spec
/// §6.1).
pub const DENIED_MESSAGE: &str = "VizDAW may not use audio input. Allow it in System Settings › Privacy & Security › Microphone, then arm the lane again.";

#[cfg(test)]
mod tests {
    use super::*;

    /// The status mapping only: tests never read the real permission (it
    /// is per-app TCC state).
    #[test]
    fn statuses_map_to_access() {
        assert_eq!(access_of(0), MicAccess::Undetermined);
        assert_eq!(access_of(1), MicAccess::Denied);
        assert_eq!(access_of(2), MicAccess::Denied);
        assert_eq!(access_of(3), MicAccess::Granted);
        assert_eq!(access_of(-7), MicAccess::Undetermined);
    }
}
