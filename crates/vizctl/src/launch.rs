//! `vizctl launch`: starts VizDAW with the control channel on, in a
//! throwaway home (control spec §12): its own `HOME`, support folder
//! (`VIZDAW_SUPPORT_DIR`), private socket directory and log, no plugin
//! scan. Waits until the channel answers.

use core_control::client::Client;
use core_control::server::MAX_SOCKET_PATH;
use serde_json::json;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// What to launch and where.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// `VizDAW.app` or its executable.
    pub app: PathBuf,
    /// The throwaway home (made 0700 if missing; an existing one must be
    /// this user's and not writable by others); None: a new one in `/tmp`.
    pub home: Option<PathBuf>,
    /// Open the demo project at launch.
    pub demo: bool,
    /// How long to wait for the channel (slow CI: be generous).
    pub timeout: Duration,
    /// Extra environment (`VIZDAW_AUDIO=offline` on CI).
    pub env: Vec<(String, String)>,
}

/// A running VizDAW.
#[derive(Debug)]
pub struct Launched {
    /// Its process.
    pub child: Child,
    /// Its control socket.
    pub socket: PathBuf,
    /// Its throwaway home.
    pub home: PathBuf,
    /// Its stdout and stderr.
    pub log: PathBuf,
    detached: bool,
}

impl Launched {
    /// Takes over a started process: dropping the value shuts it down
    /// unless [`Launched::detach`] is called.
    pub fn new(child: Child, socket: PathBuf, home: PathBuf, log: PathBuf) -> Launched {
        Launched {
            child,
            socket,
            home,
            log,
            detached: false,
        }
    }

    /// Leaves the app running for later commands (what `vizctl launch`
    /// does); without this, dropping `Launched` quits it, then kills it.
    pub fn detach(mut self) {
        self.detached = true;
    }

    /// What `vizctl launch` prints.
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "pid": self.child.id(),
            "socket": self.socket,
            "home": self.home,
            "log": self.log,
        })
    }
}

impl Drop for Launched {
    fn drop(&mut self) {
        if self.detached {
            return;
        }
        if let Ok(mut c) = Client::connect(&self.socket) {
            let _ = c.call("app.quit", json!({}), Duration::from_secs(5));
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The executable inside `app` when it is a bundle, else `app`.
pub fn executable(app: &Path) -> PathBuf {
    if app.extension().is_some_and(|e| e == "app") {
        app.join("Contents/MacOS/VizDAW")
    } else {
        app.to_path_buf()
    }
}

/// A fresh home in `/tmp`, made 0700 by this process: `mkdir` fails when
/// the name exists, so a directory someone planted is never used.
fn fresh_home() -> Result<PathBuf, String> {
    fresh_dir(|attempt| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        PathBuf::from(format!(
            "/tmp/vizctl-{}-{nanos}-{attempt}",
            std::process::id()
        ))
    })
}

/// Makes the first of `name(0)`, `name(1)`, … that does not exist yet,
/// 0700; never reuses an existing one.
fn fresh_dir(name: impl Fn(u32) -> PathBuf) -> Result<PathBuf, String> {
    let mut last = String::new();
    for attempt in 0..100u32 {
        let p = name(attempt);
        match fs::DirBuilder::new().mode(0o700).create(&p) {
            Ok(()) => return Ok(p),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                last = format!("{} exists", p.display());
            }
            Err(e) => return Err(format!("cannot create {}: {e}", p.display())),
        }
    }
    Err(format!("cannot make a new home ({last})"))
}

/// This user's id, read from a directory made in `dir` for the purpose
/// (std has no `getuid`; the crate has no unsafe code).
fn my_uid(dir: &Path) -> Result<u32, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let probe = dir.join(format!(".vizctl-probe-{}-{nanos}", std::process::id()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&probe)
        .map_err(|e| format!("cannot check who owns {}: {e}", dir.display()))?;
    let uid = fs::symlink_metadata(&probe).map(|m| m.uid());
    let _ = fs::remove_dir(&probe);
    uid.map_err(|e| format!("cannot check who owns {}: {e}", dir.display()))
}

/// Checks that an existing `p` is a real directory (not a link) owned by
/// `uid` that nobody else can write to.
fn check_owned(p: &Path, m: &fs::Metadata, uid: u32) -> Result<(), String> {
    if m.file_type().is_symlink() || !m.is_dir() {
        return Err(format!("{} is not a directory", p.display()));
    }
    if m.uid() != uid {
        return Err(format!("{} belongs to another user", p.display()));
    }
    let mode = m.permissions().mode();
    if mode & 0o022 != 0 {
        return Err(format!(
            "{} is writable by other users (mode {:o}); use a private --home",
            p.display(),
            mode & 0o777
        ));
    }
    Ok(())
}

/// The caller's `--home`: made 0700 when missing (its parents as
/// needed); when it exists it must be this user's own directory, not
/// writable by others. Returns this user's id.
fn own_home(p: &Path) -> Result<u32, String> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            fs::DirBuilder::new()
                .mode(0o700)
                .create(p)
                .map_err(|e| format!("cannot create {}: {e}", p.display()))?;
            fs::symlink_metadata(p)
                .map(|m| m.uid())
                .map_err(|e| format!("cannot read {}: {e}", p.display()))
        }
        Err(e) => Err(format!("cannot read {}: {e}", p.display())),
        Ok(m) => {
            if m.file_type().is_symlink() || !m.is_dir() {
                return Err(format!("{} is not a directory", p.display()));
            }
            let uid = my_uid(p)?;
            check_owned(p, &m, uid)?;
            Ok(uid)
        }
    }
}

/// A folder inside the home: made 0700, or an existing one of `uid`'s
/// that nobody else can write to.
fn private_dir(p: &Path, uid: u32) -> Result<(), String> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::DirBuilder::new()
            .mode(0o700)
            .create(p)
            .map_err(|e| format!("cannot create {}: {e}", p.display())),
        Err(e) => Err(format!("cannot read {}: {e}", p.display())),
        Ok(m) => check_owned(p, &m, uid),
    }
}

/// Whether a VizDAW answers on `socket` already.
fn answers(socket: &Path) -> bool {
    fs::symlink_metadata(socket).is_ok()
        && Client::connect(socket)
            .ok()
            .and_then(|mut c| c.call("ping", json!({}), Duration::from_secs(5)).ok())
            .is_some()
}

/// The last `n` lines of the log, for errors.
pub fn log_tail(log: &Path, n: usize) -> String {
    let text = fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines
        .get(lines.len().saturating_sub(n)..)
        .unwrap_or(&[])
        .join("\n")
}

/// Launches VizDAW and waits until its channel answers `ping`.
pub fn launch(o: &LaunchOptions) -> Result<Launched, String> {
    let exe = executable(&o.app);
    if !exe.is_file() {
        return Err(format!(
            "no VizDAW executable at {} (build it with scripts/make-app.sh, or pass --app)",
            exe.display()
        ));
    }
    let (home, uid) = match &o.home {
        Some(h) => (h.clone(), own_home(h)?),
        None => {
            let h = fresh_home()?;
            let uid = fs::symlink_metadata(&h)
                .map(|m| m.uid())
                .map_err(|e| format!("cannot read {}: {e}", h.display()))?;
            (h, uid)
        }
    };
    let ctl = home.join("ctl");
    private_dir(&ctl, uid)?;
    let support = home.join("support");
    private_dir(&support, uid)?;
    let scratch = home.join("scratch");
    private_dir(&scratch, uid)?;
    let socket = ctl.join("vizdaw.sock");
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(format!(
            "{} is longer than {MAX_SOCKET_PATH} bytes; pass a shorter --home",
            socket.display()
        ));
    }
    // Before the log is touched: a VizDAW still running in this home
    // keeps its log, and its socket is not mistaken for the new app's.
    if answers(&socket) {
        return Err(format!(
            "another VizDAW is listening on {}",
            socket.display()
        ));
    }
    let log = home.join("vizdaw.log");
    if fs::symlink_metadata(&log).is_ok_and(|m| !m.is_file()) {
        return Err(format!("{} is not a regular file", log.display()));
    }
    let out =
        fs::File::create(&log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
    let err = out
        .try_clone()
        .map_err(|e| format!("cannot open {}: {e}", log.display()))?;
    let mut cmd = Command::new(&exe);
    // The defaults pair goes first: after `--no-scan` AppKit misreads it
    // and SwiftUI opens no windows at all.
    cmd.args(["-ApplePersistenceIgnoreState", "YES"])
        .arg("--control")
        .arg(&socket)
        .arg("--no-scan")
        .env("HOME", &home)
        .env("VIZDAW_SUPPORT_DIR", &support)
        // Untitled projects' scratch packages, and the lost recordings
        // the app looks for at launch, stay in this home (plan 5c Task 10).
        .env("VIZDAW_SCRATCH_DIR", &scratch)
        .env_remove("VIZDAW_CONTROL")
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    if o.demo {
        cmd.arg("--demo");
    }
    for (k, v) in &o.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let deadline = Instant::now() + o.timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "VizDAW exited ({status}) before its control channel answered; log:\n{}",
                log_tail(&log, 20)
            ));
        }
        let ready = Client::connect(&socket)
            .ok()
            .and_then(|mut c| c.call("ping", json!({}), Duration::from_secs(5)).ok());
        if ready.is_some() {
            return Ok(Launched::new(child, socket, home, log));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "VizDAW's control channel did not answer within {} s; log:\n{}",
                o.timeout.as_secs(),
                log_tail(&log, 20)
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_home_never_reuses_a_planted_directory() {
        let d = tempfile::Builder::new()
            .prefix("vzf")
            .tempdir_in("/tmp")
            .unwrap();
        let planted = d.path().join("home-0");
        fs::create_dir(&planted).unwrap();
        fs::set_permissions(&planted, fs::Permissions::from_mode(0o777)).unwrap();
        let home = fresh_dir(|n| d.path().join(format!("home-{n}"))).unwrap();
        assert_eq!(home, d.path().join("home-1"), "the planted one is skipped");
        let m = fs::symlink_metadata(&home).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o700);
        // A planted link is skipped too (mkdir does not follow it).
        std::os::unix::fs::symlink(&planted, d.path().join("link-0")).unwrap();
        let home = fresh_dir(|n| d.path().join(format!("link-{n}"))).unwrap();
        assert_eq!(home, d.path().join("link-1"));
    }

    #[test]
    fn a_fresh_home_is_in_tmp_and_private() {
        let home = fresh_home().unwrap();
        let m = fs::symlink_metadata(&home).unwrap();
        assert!(home.starts_with("/tmp"));
        assert!(m.is_dir() && !m.file_type().is_symlink());
        assert_eq!(m.permissions().mode() & 0o777, 0o700);
        fs::remove_dir(&home).unwrap();
    }
}
