//! The connected peer of a local socket (plan 8b Task 6, Ruling 11): its
//! audit token, read with `getsockopt(SOL_LOCAL, LOCAL_PEERTOKEN)`. The
//! token names the process that connected (its pid is word 5) together
//! with its pid version, so the app can ask Security which code it is
//! without racing a reused pid.

use std::os::fd::RawFd;

/// `SOL_LOCAL` (`<sys/un.h>`).
const SOL_LOCAL: i32 = 0;
/// `LOCAL_PEERTOKEN` (`<sys/un.h>`): the peer's `audit_token_t`.
const LOCAL_PEERTOKEN: i32 = 0x006;

unsafe extern "C" {
    /// `getsockopt(2)`.
    fn getsockopt(
        socket: i32,
        level: i32,
        name: i32,
        value: *mut std::ffi::c_void,
        len: *mut u32,
    ) -> i32;
}

/// The connected peer's audit token (`LOCAL_PEERTOKEN`), or None: not a
/// connected local socket, or a closed or bad descriptor.
pub fn audit_token(fd: RawFd) -> Option<[u32; 8]> {
    if fd < 0 {
        return None;
    }
    let mut token = [0u32; 8];
    let mut len = std::mem::size_of_val(&token) as u32;
    // SAFETY: `token` is a writable buffer of `len` bytes, and `len` is a
    // valid pointer to its size, both live for the call; a bad `fd` is
    // reported by the return value, never by touching memory.
    let r = unsafe {
        getsockopt(
            fd,
            SOL_LOCAL,
            LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if r != 0 || len as usize != std::mem::size_of_val(&token) {
        return None;
    }
    Some(token)
}
