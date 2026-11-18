//! A blocking client of the control socket (`vizctl`, tests).

use crate::error::ControlError;
use crate::protocol::{MAX_LINE, RequestId, read_reply, request_line};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Read, Write};
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

/// The most bytes of a line's first write (see [`Client::send_counted`]).
const FIRST_WRITE: usize = 512;

/// A failed send: the error, and how many bytes of the line went out
/// before it.
#[derive(Debug)]
pub struct SendError {
    /// What failed.
    pub error: io::Error,
    /// Bytes of the line (newline included) written before the failure.
    pub written: usize,
}

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
        self.send_counted(line).map_err(|e| CallError::Io(e.error))
    }

    /// Sends `line` and its newline as one buffer, and on failure says how
    /// many of its bytes were written first. A caller that resends must
    /// know none were: a line cut off before its newline is not run, but
    /// one whose newline went out may have been.
    pub fn send_counted(&mut self, line: &str) -> Result<(), SendError> {
        let mut buf = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        let mut written = 0;
        while let Some(rest) = buf.get(written..).filter(|r| !r.is_empty()) {
            // A blocking write that fails partway may report only the
            // error, not the bytes it sent. The first write is small
            // enough for an empty socket buffer, so it is all or nothing:
            // `written == 0` then truly means nothing went out.
            let rest = if written == 0 {
                rest.get(..FIRST_WRITE).unwrap_or(rest)
            } else {
                rest
            };
            match self.writer.write(rest) {
                Ok(0) => {
                    return Err(SendError {
                        error: io::Error::new(io::ErrorKind::WriteZero, "the socket took no bytes"),
                        written,
                    });
                }
                Ok(n) => written += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(SendError { error, written }),
            }
        }
        self.writer
            .flush()
            .map_err(|error| SendError { error, written })
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

    /// The next raw line, at most [`MAX_LINE`] bytes before its newline
    /// (the server's own cap). A longer one is refused once the cap is
    /// read, without reading the rest: the connection is then of no
    /// further use.
    pub fn read_line(&mut self) -> Result<String, CallError> {
        let mut line = Vec::new();
        let cap = u64::try_from(MAX_LINE)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        match (&mut self.reader).take(cap).read_until(b'\n', &mut line) {
            Ok(0) => Err(CallError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "VizDAW closed the connection",
            ))),
            Ok(_) if line.len() > MAX_LINE && line.last() != Some(&b'\n') => Err(
                CallError::Protocol(format!("the reply is longer than {MAX_LINE} bytes")),
            ),
            Ok(_) => String::from_utf8(line)
                .map_err(|_| CallError::Protocol("the reply is not UTF-8".into())),
            Err(e) => Err(CallError::Io(e)),
        }
    }
}
