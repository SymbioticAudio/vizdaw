//! The socket peer's audit token (plan 8b Task 6, Ruling 11): read with
//! `LOCAL_PEERTOKEN` on a connected local socket.
#![cfg(target_os = "macos")]

use platform_macos::peer::audit_token;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

#[test]
fn peer_token_names_this_process() {
    let (a, b) = UnixStream::pair().unwrap();
    for end in [&a, &b] {
        let token = audit_token(end.as_raw_fd()).expect("a connected socket has a peer token");
        // audit_token_to_pid: the pid is word 5.
        assert_eq!(token[5], std::process::id());
    }
}

#[test]
fn not_a_socket_has_no_token() {
    let f = tempfile::tempfile().unwrap();
    assert_eq!(audit_token(f.as_raw_fd()), None);
    assert_eq!(audit_token(-1), None);
    // A socket with no peer.
    let d = tempfile::Builder::new()
        .prefix("vzp")
        .tempdir_in("/tmp")
        .unwrap();
    let l = std::os::unix::net::UnixListener::bind(d.path().join("s")).unwrap();
    assert_eq!(audit_token(l.as_raw_fd()), None);
}
