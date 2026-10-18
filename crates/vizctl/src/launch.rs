//! `vizctl launch`: starts VizDAW with the control channel on, in a
//! throwaway home (control spec §12): its own `HOME`, support folder
//! (`VIZDAW_SUPPORT_DIR`), private socket directory and log, no plugin
//! scan. Waits until the channel answers.

use core_control::client::Client;
use core_control::server::MAX_SOCKET_PATH;
use serde_json::json;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// What to launch and where.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// `VizDAW.app` or its executable.
    pub app: PathBuf,
    /// The throwaway home (made if missing); None: a new one in `/tmp`.
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
            let _ = c.call("quit", json!({}), Duration::from_secs(5));
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

fn private_dir(p: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)
        .map_err(|e| format!("cannot create {}: {e}", p.display()))
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
    let home = match &o.home {
        Some(h) => h.clone(),
        None => {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            PathBuf::from(format!("/tmp/vizctl-{}-{nanos}", std::process::id()))
        }
    };
    private_dir(&home)?;
    let ctl = home.join("ctl");
    private_dir(&ctl)?;
    let support = home.join("support");
    private_dir(&support)?;
    let socket = ctl.join("vizdaw.sock");
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(format!(
            "{} is longer than {MAX_SOCKET_PATH} bytes; pass a shorter --home",
            socket.display()
        ));
    }
    let log = home.join("vizdaw.log");
    let out =
        fs::File::create(&log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
    let err = out
        .try_clone()
        .map_err(|e| format!("cannot open {}: {e}", log.display()))?;
    let mut cmd = Command::new(&exe);
    cmd.arg("--control")
        .arg(&socket)
        .arg("--no-scan")
        .args(["-ApplePersistenceIgnoreState", "YES"])
        .env("HOME", &home)
        .env("VIZDAW_SUPPORT_DIR", &support)
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
