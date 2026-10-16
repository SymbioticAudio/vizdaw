//! The scanner protocol (spec §8): one `vizdaw-scan` process per plugin,
//! so a crash or a hang costs only that process; its outcome becomes a
//! catalog entry (a crash, non-zero exit or timeout blocklists the plugin).

use crate::catalog::{Catalog, CatalogEntry, ScanStatus, Source};
use crate::info::{PluginInfo, PluginKind};
use core_model::mixer::{PluginFormat, PluginRef};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Time a CLAP or AU v2 scan may take (spec §8).
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Time an AU v3 scan may take.
pub const TIMEOUT_AU_V3: Duration = Duration::from_secs(30);
/// Scans run at once.
pub const PARALLEL: usize = 4;

/// What `vizdaw-scan` prints on success (one JSON line on stdout).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanReply {
    /// `--list`: the plugins of a source.
    Listed(Vec<(PluginRef, PluginKind)>),
    /// `--id`: the plugin loaded, activated at 48 kHz / 512, processed one
    /// silent block and deactivated.
    Scanned(PluginInfo),
    /// It cannot be checked outside the app (AU v3), and why.
    NotValidated(String),
}

/// How one scan ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ScanOutcome {
    /// The scanner answered.
    Done(ScanReply),
    /// The scanner died from a signal (`None`: unknown).
    Crashed(Option<i32>),
    /// It took longer than allowed and was killed.
    TimedOut,
    /// It exited with an error message.
    Failed(String),
}

impl ScanOutcome {
    /// The blocklist reason (spec §8), when this blocklists the plugin.
    pub fn reason(&self) -> Option<String> {
        match self {
            ScanOutcome::Done(_) => None,
            ScanOutcome::Crashed(Some(sig)) => Some(format!("crashed (signal {sig})")),
            ScanOutcome::Crashed(None) => Some("crashed".to_string()),
            ScanOutcome::TimedOut => Some("timed out".to_string()),
            ScanOutcome::Failed(m) => Some(format!("failed: {m}")),
        }
    }
}

/// One scan to run.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanRequest {
    /// Format.
    pub format: PluginFormat,
    /// Source path (CLAP bundle; ignored for AU).
    pub path: PathBuf,
    /// The plugin to scan, or `None` to list the source.
    pub id: Option<String>,
    /// Time allowed.
    pub timeout: Duration,
}

impl ScanRequest {
    /// The command-line arguments of `vizdaw-scan` for this request.
    pub fn args(&self) -> Vec<String> {
        let mut a = vec![
            "--format".to_string(),
            match self.format {
                PluginFormat::Clap => "clap".to_string(),
                PluginFormat::Au => "au".to_string(),
            },
            "--path".to_string(),
            self.path.to_string_lossy().into_owned(),
        ];
        match &self.id {
            Some(id) => {
                a.push("--id".into());
                a.push(id.clone());
            }
            None => a.push("--list".into()),
        }
        a
    }
}

fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

/// Runs `exe` (the `vizdaw-scan` binary) for `req` and waits for it, at
/// most `req.timeout`.
pub fn run(exe: &Path, req: &ScanRequest) -> ScanOutcome {
    let child = Command::new(exe)
        .args(req.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return ScanOutcome::Failed(format!("cannot start the scanner: {e}")),
    };
    // Drain both pipes on their own threads: a large reply must never
    // block the child (which would look like a hang).
    let drain = |p: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut p) = p {
                let _ = p.read_to_string(&mut s);
            }
            s
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() >= req.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return ScanOutcome::Failed(e.to_string()),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    let Some(status) = status else {
        return ScanOutcome::TimedOut;
    };
    if let Some(sig) = signal_of(&status) {
        return ScanOutcome::Crashed(Some(sig));
    }
    if !status.success() {
        let msg = stderr.trim();
        return ScanOutcome::Failed(if msg.is_empty() {
            format!("exit status {status}")
        } else {
            msg.lines().last().unwrap_or(msg).to_string()
        });
    }
    match stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(serde_json::from_str::<ScanReply>)
    {
        Some(Ok(r)) => ScanOutcome::Done(r),
        Some(Err(e)) => ScanOutcome::Failed(format!("unreadable reply: {e}")),
        None => ScanOutcome::Failed("no reply".to_string()),
    }
}

/// Scans every plugin of `source`: lists it, then scans each plugin on its
/// own. A source that cannot be listed gives one blocklisted entry with an
/// empty id.
pub fn scan_source(exe: &Path, source: &Source, timeout: Duration) -> Vec<CatalogEntry> {
    let list = run(
        exe,
        &ScanRequest {
            format: source.format,
            path: source.path.clone(),
            id: None,
            timeout,
        },
    );
    let plugins = match list {
        ScanOutcome::Done(ScanReply::Listed(p)) => p,
        other => {
            let reason = other
                .reason()
                .unwrap_or_else(|| "failed: unexpected reply".to_string());
            return vec![CatalogEntry {
                source: source.clone(),
                plugin: PluginRef {
                    format: source.format,
                    id: String::new(),
                    name: source
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    vendor: String::new(),
                    version: source.version.clone(),
                },
                kind: None,
                info: None,
                status: ScanStatus::Blocklisted { reason },
            }];
        }
    };
    plugins
        .into_iter()
        .map(|(plugin, kind)| {
            let outcome = run(
                exe,
                &ScanRequest {
                    format: source.format,
                    path: source.path.clone(),
                    id: Some(plugin.id.clone()),
                    timeout,
                },
            );
            let (info, status) = match outcome {
                ScanOutcome::Done(ScanReply::Scanned(info)) => (Some(info), ScanStatus::Ok),
                ScanOutcome::Done(ScanReply::NotValidated(_)) => (None, ScanStatus::NotValidated),
                ScanOutcome::Done(ScanReply::Listed(_)) => (
                    None,
                    ScanStatus::Blocklisted {
                        reason: "failed: unexpected reply".into(),
                    },
                ),
                other => (
                    None,
                    ScanStatus::Blocklisted {
                        reason: other.reason().unwrap_or_default(),
                    },
                ),
            };
            CatalogEntry {
                source: source.clone(),
                kind: info.as_ref().map(|i| i.kind).or(Some(kind)),
                plugin: info.as_ref().map_or(plugin, |i| i.plugin.clone()),
                info,
                status,
            }
        })
        .collect()
}

/// Scans `sources` with up to [`PARALLEL`] scanner processes at a time and
/// records the results in `catalog`. Returns the sources scanned.
pub fn scan_all(
    catalog: &mut Catalog,
    exe: &Path,
    sources: &[Source],
    timeout: impl Fn(&Source) -> Duration + Sync,
) -> usize {
    let results: Vec<(Source, Vec<CatalogEntry>)> = std::thread::scope(|s| {
        let mut out = Vec::new();
        for chunk in sources.chunks(PARALLEL.max(1)) {
            let handles: Vec<_> = chunk
                .iter()
                .map(|src| {
                    let t = &timeout;
                    s.spawn(move || (src.clone(), scan_source(exe, src, t(src))))
                })
                .collect();
            for h in handles {
                if let Ok(r) = h.join() {
                    out.push(r);
                }
            }
        }
        out
    });
    let n = results.len();
    for (src, entries) in results {
        catalog.record(&src, entries);
    }
    n
}

/// Every CLAP bundle in the standard folders, as sources.
pub fn discover_clap() -> Vec<Source> {
    crate::clap::find_bundles(&crate::clap::search_paths())
        .iter()
        .map(|p| Source::clap_bundle(p))
        .collect()
}
