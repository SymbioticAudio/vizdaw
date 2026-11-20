//! The connection to VizDAW's Co-Producer socket (plan 8a Rulings 10 and
//! P7). A call connects when it has no live connection. It connects and
//! sends again once, and only when connecting failed or the peer had
//! already closed before any byte of the request was written: once any of
//! it was written, VizDAW may run it, so a failed write, a dropped connection or a
//! timeout is that call's answer and the call is never sent again (the
//! socket refuses a line cut off before its newline, but one whose
//! newline went out may have run). The next call reconnects. Before
//! connecting, the socket's folder must be a folder (not a link) of this
//! user's, writable by no one else.

use core_control::ControlError;
use core_control::client::{CallError, Client, SendError};
use core_control::protocol::{RequestId, request_line};
use serde_json::Value;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The socket's name in the support folder.
pub const SOCKET_NAME: &str = "co-producer.sock";
/// The file VizDAW leaves in the support folder while Co-Producer is off.
pub const OFF_MARKER: &str = "co-producer.off";
/// How long a call waits: above the app's 60 s, and above the 605 s it
/// gives a job (an export or a bounce), so a long job is not cut off.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(660);

/// How long the check of an idle connection waits for its end.
const PROBE: Duration = Duration::from_millis(1);

/// Why a call has no result.
#[derive(Debug)]
pub enum AppError {
    /// Nothing listens: VizDAW is not running, or Co-Producer is off.
    Unreachable,
    /// The socket's folder is not private to this user: not connected.
    Unsafe(String),
    /// Part of the request was written and then writing failed. It may
    /// run, so it is not sent again.
    Unsent(io::Error),
    /// VizDAW answered with an error.
    Refused(ControlError),
    /// The request was written and then the connection ended or timed
    /// out. It may have run, so it is not sent again.
    Lost(io::Error),
    /// The answer was not a reply.
    Garbled(String),
}

/// The connection.
pub struct App {
    support: PathBuf,
    conn: Option<Client>,
    next: i64,
    timeout: Duration,
}

impl App {
    /// The app whose support folder is `support`.
    pub fn new(support: &Path) -> App {
        App {
            support: support.to_path_buf(),
            conn: None,
            next: 1,
            timeout: CALL_TIMEOUT,
        }
    }

    /// The support folder.
    pub fn support(&self) -> &Path {
        &self.support
    }

    /// Ruling 10's sentence for a VizDAW that cannot be reached.
    pub fn unreachable_message(&self) -> &'static str {
        if std::fs::symlink_metadata(self.support.join(OFF_MARKER)).is_ok() {
            "Co-Producer is turned off in VizDAW's Settings."
        } else {
            "VizDAW is not running, or Co-Producer is off in its Settings."
        }
    }

    /// The text a tool answers for `e`.
    pub fn message(&self, e: &AppError) -> String {
        match e {
            AppError::Unreachable => self.unreachable_message().to_string(),
            AppError::Unsafe(why) => format!(
                "vizdaw-mcp will not connect to VizDAW: {why}, so another user could stand in for it."
            ),
            AppError::Unsent(e) => format!(
                "{} Sending the command failed partway ({e}); it may have run: read VizDAW's state before trying again.",
                self.unreachable_message()
            ),
            AppError::Refused(e) => e.to_string(),
            AppError::Lost(e) => format!(
                "The connection to VizDAW ended before it answered ({e}). The command may have run: read VizDAW's state before trying again."
            ),
            AppError::Garbled(m) => format!("VizDAW's answer could not be read ({m})."),
        }
    }

    /// Whether the kept connection is still open. Nothing is in flight on
    /// it, so any line or end of input means it is finished with.
    fn alive(conn: &mut Client) -> bool {
        let probe = RequestId::Text("vizdaw-mcp-probe".into());
        match conn.read_reply_for(&probe, PROBE) {
            Err(CallError::Io(e)) => matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ),
            _ => false,
        }
    }

    /// Why the socket's folder is not safe to connect in; Ok(false) when
    /// it does not exist (VizDAW never ran with Co-Producer on).
    fn folder_is_safe(&self) -> Result<bool, AppError> {
        support_is_safe(&self.support)
    }

    fn connect(&self) -> Result<Client, AppError> {
        if !self.folder_is_safe()? {
            return Err(AppError::Unreachable);
        }
        Client::connect(&self.support.join(SOCKET_NAME)).map_err(|_| AppError::Unreachable)
    }

    /// Sends `cmd` with `params` and waits for its result.
    pub fn call(&mut self, cmd: &str, params: Value) -> Result<Value, AppError> {
        for attempt in 0..2 {
            let last = attempt == 1;
            if let Some(c) = self.conn.as_mut() {
                if !App::alive(c) {
                    self.conn = None;
                }
            }
            let mut conn = match self.conn.take() {
                Some(c) => c,
                None => match self.connect() {
                    Ok(c) => c,
                    Err(AppError::Unreachable) if !last => continue,
                    Err(e) => return Err(e),
                },
            };
            let id = RequestId::Num(self.next);
            self.next = self.next.wrapping_add(1);
            if let Err(e) = conn.send_counted(&request_line(&id, cmd, &params)) {
                if may_resend(&e) {
                    if last {
                        return Err(AppError::Unreachable);
                    }
                    continue;
                }
                return Err(AppError::Unsent(e.error));
            }
            // Written: whatever happens now, this call is not sent again.
            return match conn.read_reply_for(&id, self.timeout) {
                Ok(v) => {
                    self.conn = Some(conn);
                    Ok(v)
                }
                Err(CallError::Reply(e)) => {
                    self.conn = Some(conn);
                    Err(AppError::Refused(e))
                }
                Err(CallError::Io(e)) => Err(AppError::Lost(e)),
                Err(CallError::Protocol(m)) => Err(AppError::Garbled(m)),
            };
        }
        Err(AppError::Unreachable)
    }
}

/// Why `support` is not safe to connect in or read a screenshot from:
/// it must be a folder (not a link) of this user's, writable by no one
/// else, whoever made it. Ok(false) when it does not exist.
pub(crate) fn support_is_safe(support: &Path) -> Result<bool, AppError> {
    match my_uid() {
        Ok(uid) => folder_check(support, uid),
        Err(e) => Err(AppError::Unsafe(format!(
            "this user cannot be told from others ({e})"
        ))),
    }
}

/// [`support_is_safe`] for the user `uid`.
pub(crate) fn folder_check(support: &Path, uid: u32) -> Result<bool, AppError> {
    // A final `/` or `/.` makes the system follow a final link, which
    // `symlink_metadata` would not see.
    if !core_control::co_producer::is_normal(support) {
        return Err(AppError::Unsafe(format!(
            "{} is not in normal form (no `.` part, no doubled `/`, no `/` at the end)",
            support.display()
        )));
    }
    let m = match std::fs::symlink_metadata(support) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(AppError::Unsafe(format!("its folder cannot be read ({e})"))),
    };
    let shown = support.display();
    if m.file_type().is_symlink() || !m.is_dir() {
        return Err(AppError::Unsafe(format!("{shown} is not a folder")));
    }
    if m.uid() != uid {
        return Err(AppError::Unsafe(format!("{shown} belongs to another user")));
    }
    if m.mode() & 0o022 != 0 {
        return Err(AppError::Unsafe(format!(
            "{shown} can be written by other users (mode {:o})",
            m.mode() & 0o777
        )));
    }
    Ok(true)
}

/// Whether a failed send may go again on a new connection: only when the
/// peer had already closed and not one byte of this request was written.
pub fn may_resend(e: &SendError) -> bool {
    e.written == 0
        && matches!(
            e.error.kind(),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::NotConnected
        )
}

/// This process's user, from a file it makes (std has no `getuid`, and
/// unsafe code is forbidden). Read once.
pub(crate) fn my_uid() -> io::Result<u32> {
    static UID: std::sync::OnceLock<Result<u32, String>> = std::sync::OnceLock::new();
    UID.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let probe =
            std::env::temp_dir().join(format!("vizdaw-mcp-uid-{}-{nanos}", std::process::id()));
        let made = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .and_then(|f| f.metadata());
        let _ = std::fs::remove_file(&probe);
        made.map(|m| m.uid()).map_err(|e| e.to_string())
    })
    .clone()
    .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn private_tmp() -> tempfile::TempDir {
        let d = tempfile::Builder::new()
            .prefix("vzm")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    #[test]
    fn only_a_closed_peer_with_nothing_written_is_resent() {
        let e = |kind, written| SendError {
            error: io::Error::from(kind),
            written,
        };
        assert!(may_resend(&e(io::ErrorKind::BrokenPipe, 0)));
        assert!(may_resend(&e(io::ErrorKind::ConnectionReset, 0)));
        assert!(may_resend(&e(io::ErrorKind::NotConnected, 0)));
        assert!(!may_resend(&e(io::ErrorKind::BrokenPipe, 1)));
        assert!(!may_resend(&e(io::ErrorKind::ConnectionReset, 4096)));
        assert!(!may_resend(&e(io::ErrorKind::OutOfMemory, 0)));
        assert!(!may_resend(&e(io::ErrorKind::TimedOut, 0)));
        assert!(!may_resend(&e(io::ErrorKind::WriteZero, 0)));
    }

    /// A peer that reads the first bytes of a long request and closes
    /// mid-write: the call fails and is never sent on a second connection.
    #[test]
    fn a_call_that_failed_mid_write_is_not_sent_again() {
        let d = private_tmp();
        let listener = UnixListener::bind(d.path().join(SOCKET_NAME)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let peer = {
            let (accepted, stop) = (Arc::clone(&accepted), Arc::clone(&stop));
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut s, _)) => {
                            accepted.fetch_add(1, Ordering::SeqCst);
                            s.set_nonblocking(false).unwrap();
                            let mut first = [0u8; 100];
                            s.read_exact(&mut first).unwrap();
                            // Closing with unread data resets the writer.
                        }
                        Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
                    }
                }
            })
        };
        let mut app = App::new(d.path());
        let big = "x".repeat(8 << 20);
        let r = app.call(
            "command.run",
            serde_json::json!({"id": "track.new_audio", "pad": big}),
        );
        assert!(matches!(r, Err(AppError::Unsent(_))), "{r:?}");
        assert!(app.message(&r.unwrap_err()).contains("may have run"));
        std::thread::sleep(std::time::Duration::from_millis(200));
        stop.store(true, Ordering::SeqCst);
        peer.join().unwrap();
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            1,
            "sent on one connection only"
        );
    }

    /// A support folder of another user's is never connected in, nor read
    /// a screenshot from, though vizdaw-mcp did not make it (release gate
    /// Task 5). Tests cannot give a folder away, so the check is asked
    /// about a uid that is not its owner's.
    #[test]
    fn foreign_owned_support_folder_is_refused() {
        let d = private_tmp();
        let owner = std::fs::symlink_metadata(d.path()).unwrap().uid();
        assert!(matches!(folder_check(d.path(), owner), Ok(true)));
        match folder_check(d.path(), owner.wrapping_add(1)) {
            Err(AppError::Unsafe(why)) => assert!(why.contains("another user"), "{why}"),
            other => panic!("{other:?}"),
        }
        // `/` belongs to the system's own user, never to a test's.
        if my_uid().unwrap() != 0 {
            match folder_check(Path::new("/"), my_uid().unwrap()) {
                Err(AppError::Unsafe(_)) => {}
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn a_folder_others_can_write_is_refused() {
        let d = private_tmp();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
        let mut app = App::new(d.path());
        let e = app.call("ping", serde_json::json!({})).unwrap_err();
        assert!(matches!(e, AppError::Unsafe(_)), "{e:?}");
        assert!(
            app.message(&e).contains("other users"),
            "{}",
            app.message(&e)
        );
        let link = private_tmp();
        let at = link.path().join("support");
        std::os::unix::fs::symlink(d.path(), &at).unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let e = App::new(&at)
            .call("ping", serde_json::json!({}))
            .unwrap_err();
        assert!(matches!(e, AppError::Unsafe(_)), "{e:?}");
        assert!(matches!(
            App::new(&d.path().join("missing")).call("ping", serde_json::json!({})),
            Err(AppError::Unreachable)
        ));
        assert_eq!(
            my_uid().unwrap(),
            std::fs::metadata(d.path()).unwrap().uid()
        );
    }
}
