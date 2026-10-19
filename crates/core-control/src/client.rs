//! A blocking client of the control socket (`vizctl`, tests).

use crate::error::ControlError;
use crate::protocol::{RequestId, read_reply, request_line};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Why a call failed.
#[derive(Debug)]
pub enum CallError {
    /// The socket failed (no VizDAW listening, closed, timed out).
    Io(io::Error),
    /// VizDAW answered with an error.
    Reply(ControlError),
    /// The answer was not a reply.
    Protocol(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Io(e) => write!(f, "control socket: {e}"),
            CallError::Reply(e) => write!(f, "{e}"),
            CallError::Protocol(m) => write!(f, "control protocol: {m}"),
        }
    }
}

impl std::error::Error for CallError {}

/// One connection.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: i64,
}

impl Client {
    /// Connects to the socket at `path`.
    pub fn connect(path: &Path) -> io::Result<Client> {
        let writer = UnixStream::connect(path)?;
        let reader = BufReader::new(writer.try_clone()?);
        Ok(Client {
            reader,
            writer,
            next: 1,
        })
    }

    /// Sends `cmd` with `params` and waits up to `timeout` for its reply.
    pub fn call(
        &mut self,
        cmd: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        let id = RequestId::Num(self.next);
        self.next += 1;
        self.send_line(&request_line(&id, cmd, &params))?;
        self.read_reply_for(&id, timeout)
    }

    /// Sends a raw line (tests send malformed ones).
    pub fn send_line(&mut self, line: &str) -> Result<(), CallError> {
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(CallError::Io)
    }

    /// Reads replies until the one for `id` (or a reply without id, which
    /// is a connection-level error).
    pub fn read_reply_for(
        &mut self,
        id: &RequestId,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        self.writer
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
            .map_err(CallError::Io)?;
        loop {
            let line = self.read_line()?;
            let reply = read_reply(&line).map_err(CallError::Protocol)?;
            if reply.id.is_none() || reply.id.as_ref() == Some(id) {
                return reply.outcome.map_err(CallError::Reply);
            }
        }
    }

    /// The next raw line.
    pub fn read_line(&mut self) -> Result<String, CallError> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(CallError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "VizDAW closed the connection",
            ))),
            Ok(_) => Ok(line),
            Err(e) => Err(CallError::Io(e)),
        }
    }
}
