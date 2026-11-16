//! The connection to VizDAW's Co-Producer socket (plan 8a Rulings 10 and
//! P7). A call connects when it has no live connection. It connects and
//! sends again once, and only when connecting or writing the request
//! failed: once a request was written, VizDAW may have run it, so a
//! dropped connection or a timeout is that call's answer and the call is
//! never sent again. The next call reconnects.

use core_control::ControlError;
use core_control::client::{CallError, Client};
use core_control::protocol::{RequestId, request_line};
use serde_json::Value;
use std::io;
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

    /// Sends `cmd` with `params` and waits for its result.
    pub fn call(&mut self, cmd: &str, params: Value) -> Result<Value, AppError> {
        for attempt in 0..2 {
            let last = attempt == 1;
            if let Some(c) = self.conn.as_mut() {
                if !App::alive(c) {
                    self.conn = None;
                }
            }
            let conn = match self.conn.take() {
                Some(c) => c,
                None => match Client::connect(&self.support.join(SOCKET_NAME)) {
                    Ok(c) => c,
                    Err(_) if !last => continue,
                    Err(_) => return Err(AppError::Unreachable),
                },
            };
            let mut conn = conn;
            let id = RequestId::Num(self.next);
            self.next = self.next.wrapping_add(1);
            if conn.send_line(&request_line(&id, cmd, &params)).is_err() {
                if last {
                    return Err(AppError::Unreachable);
                }
                continue;
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
