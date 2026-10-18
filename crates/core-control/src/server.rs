//! The control socket (spec §4.2, §8): a Unix domain socket in a private
//! directory, one thread accepting and one thread per connection. A
//! connection reads a line, validates it, answers `commands.list` and
//! `ping` itself and queues anything else as a [`Call`] for the app,
//! which takes calls with [`Server::next_call`] on its main thread and
//! answers with [`Server::reply`]. A call the app does not answer in time
//! is answered `timeout`; the app's late answer is dropped.
//!
//! Nothing here panics on input: every line becomes a reply.

use crate::error::{ControlError, ErrorCode};
use crate::protocol::{MAX_LINE, RequestId, read_request, reply_line};
use crate::registry;
use crate::request::{Command, validate};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// The longest socket path macOS accepts (`sun_path` is 104 bytes with
/// its terminating NUL).
pub const MAX_SOCKET_PATH: usize = 103;

/// Limits and timeouts.
#[derive(Debug, Clone)]
pub struct Config {
    /// Connections served at once; more get one `unavailable` line.
    pub max_connections: usize,
    /// How long a call may wait for the app.
    pub app_timeout: Duration,
    /// Added to a wait's own timeout before the server gives up on it.
    pub wait_grace: Duration,
    /// A connection idle this long (mid-line or between lines) is closed.
    pub idle_timeout: Duration,
    /// A reply the client does not read within this is dropped with
    /// the connection.
    pub write_timeout: Duration,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_connections: 4,
            app_timeout: Duration::from_secs(60),
            wait_grace: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(300),
            write_timeout: Duration::from_secs(30),
        }
    }
}

/// A request for the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// Answer with this.
    pub token: u64,
    /// What to do.
    pub command: Command,
}

type Outcome = Result<Value, ControlError>;

struct Shared {
    config: Config,
    path: PathBuf,
    dir: PathBuf,
    queue: Mutex<VecDeque<Call>>,
    waiting: Mutex<HashMap<u64, Sender<Outcome>>>,
    streams: Mutex<HashMap<u64, UnixStream>>,
    next_token: AtomicU64,
    next_conn: AtomicU64,
    shots: AtomicU64,
    connections: AtomicUsize,
    stopping: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why the server did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError(pub String);

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "control channel: {}", self.0)
    }
}

impl std::error::Error for StartError {}

/// The listening control channel. Dropping it stops it.
pub struct Server {
    shared: Arc<Shared>,
    acceptor: Option<JoinHandle<()>>,
}

/// Checks (or makes) the socket's private directory and clears a stale
/// socket; returns the directory.
fn prepare(path: &Path) -> Result<PathBuf, StartError> {
    let fail = |m: String| Err(StartError(m));
    if !path.is_absolute() {
        return fail(format!("{} is not an absolute path", path.display()));
    }
    let len = path.as_os_str().len();
    if len > MAX_SOCKET_PATH {
        return fail(format!(
            "{} is {len} bytes long; a socket path may have at most {MAX_SOCKET_PATH}",
            path.display()
        ));
    }
    let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return fail(format!("{} has no directory", path.display()));
    };
    match fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(|e| StartError(format!("cannot create {}: {e}", dir.display())))?;
        }
        Err(e) => return fail(format!("cannot read {}: {e}", dir.display())),
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return fail(format!("{} is not a directory", dir.display()));
        }
        Ok(_) => {}
    }
    let mode = fs::metadata(dir)
        .map_err(|e| StartError(format!("cannot read {}: {e}", dir.display())))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return fail(format!(
            "{} is open to other users (mode {:o}); the socket needs a private directory (chmod 700)",
            dir.display(),
            mode & 0o777
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_socket() => {
            if UnixStream::connect(path).is_ok() {
                return fail(format!("another VizDAW is listening on {}", path.display()));
            }
            fs::remove_file(path).map_err(|e| {
                StartError(format!("cannot remove the stale {}: {e}", path.display()))
            })?;
        }
        Ok(_) => return fail(format!("{} exists and is not a socket", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return fail(format!("cannot read {}: {e}", path.display())),
    }
    Ok(dir.to_path_buf())
}

impl Server {
    /// Listens on `path` (spec §8): the directory is created 0700 or must
    /// already be private and owned by this user; a stale socket is
    /// removed; the socket is made 0600.
    pub fn start(path: &Path, config: Config) -> Result<Server, StartError> {
        let dir = prepare(path)?;
        let listener = UnixListener::bind(path)
            .map_err(|e| StartError(format!("cannot listen on {}: {e}", path.display())))?;
        let fail = |m: String| {
            let _ = fs::remove_file(path);
            Err(StartError(m))
        };
        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            return fail(format!("cannot make {} private: {e}", path.display()));
        }
        // The socket was made by this process: its owner is this user.
        match (fs::metadata(path), fs::metadata(&dir)) {
            (Ok(s), Ok(d)) if s.uid() == d.uid() => {}
            (Ok(_), Ok(_)) => return fail(format!("{} belongs to another user", dir.display())),
            (Err(e), _) | (_, Err(e)) => {
                return fail(format!("cannot read {}: {e}", dir.display()));
            }
        }
        let shared = Arc::new(Shared {
            config,
            path: path.to_path_buf(),
            dir,
            queue: Mutex::default(),
            waiting: Mutex::default(),
            streams: Mutex::default(),
            next_token: AtomicU64::new(1),
            next_conn: AtomicU64::new(1),
            shots: AtomicU64::new(0),
            connections: AtomicUsize::new(0),
            stopping: AtomicBool::new(false),
        });
        let s = Arc::clone(&shared);
        let acceptor = std::thread::Builder::new()
            .name("vizdaw-control".into())
            .spawn(move || accept(&s, &listener))
            .map_err(|e| StartError(format!("cannot start the control thread: {e}")))?;
        Ok(Server {
            shared,
            acceptor: Some(acceptor),
        })
    }

    /// The socket's path.
    pub fn path(&self) -> &Path {
        &self.shared.path
    }

    /// The socket's (private) directory.
    pub fn dir(&self) -> &Path {
        &self.shared.dir
    }

    /// The oldest call nobody answered yet (calls whose client gave up are
    /// skipped).
    pub fn next_call(&self) -> Option<Call> {
        let mut q = lock(&self.shared.queue);
        while let Some(c) = q.pop_front() {
            if self.is_waiting(c.token) {
                return Some(c);
            }
        }
        None
    }

    /// Answers call `token`; false when nobody waits for it any more (it
    /// timed out or its client left).
    pub fn reply(&self, token: u64, outcome: Result<Value, ControlError>) -> bool {
        let Some(tx) = lock(&self.shared.waiting).remove(&token) else {
            return false;
        };
        tx.send(outcome).is_ok()
    }

    /// Whether a client still waits for call `token`.
    pub fn is_waiting(&self, token: u64) -> bool {
        lock(&self.shared.waiting).contains_key(&token)
    }

    /// A new screenshot path in the socket's directory.
    pub fn screenshot_path(&self) -> PathBuf {
        let n = self.shared.shots.fetch_add(1, Ordering::Relaxed) + 1;
        self.shared.dir.join(format!("screenshot-{n}.png"))
    }

    /// Stops: pending calls are answered `shutting_down`, connections
    /// close, the socket file goes. Idempotent.
    pub fn stop(&mut self) {
        let s = &self.shared;
        if s.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        let waiting: Vec<_> = lock(&s.waiting).drain().collect();
        for (_, tx) in waiting {
            let _ = tx.send(Err(ControlError::new(
                ErrorCode::ShuttingDown,
                "VizDAW is quitting",
            )));
        }
        lock(&s.queue).clear();
        // The acceptor polls the flag; then end every connection's read.
        if let Some(h) = self.acceptor.take() {
            let _ = h.join();
        }
        for (_, st) in lock(&s.streams).drain() {
            let _ = st.shutdown(std::net::Shutdown::Read);
        }
        let _ = fs::remove_file(&s.path);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept(s: &Arc<Shared>, listener: &UnixListener) {
    // Non-blocking so `stop` never depends on the socket path to wake us.
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    loop {
        if s.stopping.load(Ordering::SeqCst) {
            return;
        }
        let stream = match listener.accept() {
            Ok((st, _)) => st,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let n = s.connections.fetch_add(1, Ordering::SeqCst);
        if n >= s.config.max_connections {
            s.connections.fetch_sub(1, Ordering::SeqCst);
            let e = ControlError::new(
                ErrorCode::Unavailable,
                format!(
                    "too many control connections (at most {})",
                    s.config.max_connections
                ),
            );
            let mut w = stream;
            let _ = w.set_write_timeout(Some(Duration::from_secs(1)));
            let _ = writeln!(w, "{}", reply_line(None, &Err(e)));
            continue;
        }
        let conn = s.next_conn.fetch_add(1, Ordering::Relaxed);
        if let Ok(c) = stream.try_clone() {
            lock(&s.streams).insert(conn, c);
        }
        let s2 = Arc::clone(s);
        let spawned = std::thread::Builder::new()
            .name("vizdaw-control-conn".into())
            .spawn(move || {
                serve(&s2, stream);
                lock(&s2.streams).remove(&conn);
                s2.connections.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            lock(&s.streams).remove(&conn);
            s.connections.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A line read with a length cap.
enum Line {
    Full(Vec<u8>),
    TooLong,
}

/// Reads up to a newline; a line over `max` bytes is consumed and
/// reported as [`Line::TooLong`]. None at end of input.
fn read_line_capped(r: &mut impl BufRead, max: usize) -> io::Result<Option<Line>> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let (used, done, eof) = {
            let available = match r.fill_buf() {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                (0, true, true)
            } else {
                match available.iter().position(|&b| b == b'\n') {
                    Some(i) => {
                        if !too_long {
                            buf.extend_from_slice(available.get(..i).unwrap_or(&[]));
                        }
                        (i + 1, true, false)
                    }
                    None => {
                        if !too_long {
                            buf.extend_from_slice(available);
                        }
                        (available.len(), false, false)
                    }
                }
            }
        };
        r.consume(used);
        if buf.len() > max {
            too_long = true;
            buf = Vec::new();
        }
        if done {
            if eof && buf.is_empty() && !too_long {
                return Ok(None);
            }
            return Ok(Some(if too_long {
                Line::TooLong
            } else {
                Line::Full(buf)
            }));
        }
    }
}

fn serve(s: &Arc<Shared>, stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(s.config.idle_timeout));
    let _ = stream.set_write_timeout(Some(s.config.write_timeout));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::with_capacity(64 * 1024, read_half);
    let mut writer = stream;
    loop {
        let (id, outcome) = match read_line_capped(&mut reader, MAX_LINE) {
            Ok(None) | Err(_) => return,
            Ok(Some(Line::TooLong)) => (
                None,
                Err(ControlError::new(
                    ErrorCode::ParseError,
                    format!("request line longer than {MAX_LINE} bytes"),
                )),
            ),
            Ok(Some(Line::Full(line))) => {
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                handle(s, &line)
            }
        };
        let text = reply_line(id.as_ref(), &outcome);
        if writer
            .write_all(text.as_bytes())
            .and_then(|()| writer.write_all(b"\n"))
            .and_then(|()| writer.flush())
            .is_err()
        {
            return;
        }
        if s.stopping.load(Ordering::SeqCst) {
            return;
        }
    }
}

fn handle(s: &Arc<Shared>, line: &[u8]) -> (Option<RequestId>, Result<Value, ControlError>) {
    let raw = match read_request(line) {
        Ok(r) => r,
        Err(e) => return (e.id, Err(e.error)),
    };
    let id = Some(raw.id.clone());
    if s.stopping.load(Ordering::SeqCst) {
        return (
            id,
            Err(ControlError::new(
                ErrorCode::ShuttingDown,
                "VizDAW is quitting",
            )),
        );
    }
    let command = match validate(&raw) {
        Ok(c) => c,
        Err(e) => return (id, Err(e)),
    };
    match command {
        Command::CommandsList => return (id, Ok(registry::list_json())),
        Command::Ping => return (id, Ok(json!({ "pong": true }))),
        _ => {}
    }
    let timeout = match &command {
        Command::Wait { timeout_ms, .. } => {
            Duration::from_millis(*timeout_ms) + s.config.wait_grace
        }
        _ => s.config.app_timeout,
    };
    let token = s.next_token.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    lock(&s.waiting).insert(token, tx);
    if s.stopping.load(Ordering::SeqCst) {
        // `stop` drained the waiters before this one was added.
        lock(&s.waiting).remove(&token);
        return (
            id,
            Err(ControlError::new(
                ErrorCode::ShuttingDown,
                "VizDAW is quitting",
            )),
        );
    }
    lock(&s.queue).push_back(Call { token, command });
    let outcome = rx.recv_timeout(timeout).unwrap_or_else(|_| {
        Err(ControlError::new(
            ErrorCode::Timeout,
            format!(
                "VizDAW did not answer within {} s (is its main thread blocked?)",
                timeout.as_secs()
            ),
        ))
    });
    lock(&s.waiting).remove(&token);
    (id, outcome)
}
