//! A fake VizDAW for `vizdaw-mcp`'s tests: a real `core_control::Server`
//! with `Role::CoProducer` (so the socket's own gate and validation run),
//! listening at `<support>/co-producer.sock` in a private temp folder
//! under `/tmp`. A thread answers its calls, from a script first and
//! otherwise with canned results. Nothing here touches the real
//! Application Support folder or a real VizDAW.
#![allow(dead_code)]

use core_control::server::{Config, Role, Server};
use core_control::{Command, ControlError};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A 1×1 PNG.
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// A private support folder under `/tmp` (socket paths are short).
pub fn support_dir() -> tempfile::TempDir {
    let d = tempfile::Builder::new()
        .prefix("vzm")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

/// The Co-Producer socket in `support`.
pub fn socket(support: &Path) -> PathBuf {
    support.join("co-producer.sock")
}

/// What the fake answers the next call with, instead of its canned result.
#[derive(Debug, Clone)]
pub enum Answer {
    /// This result.
    Ok(Value),
    /// This error.
    Err(ControlError),
    /// A screenshot answer naming this path; nothing is written.
    ScreenshotAt(PathBuf),
    /// This result, after this long (a call that keeps the server waiting).
    After(Duration, Value),
}

/// The fake app.
pub struct FakeApp {
    server: Arc<Mutex<Server>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    seen: Arc<Mutex<Vec<Command>>>,
    script: Arc<Mutex<VecDeque<Answer>>>,
}

/// The token the fake's `n`th preview (from 1) answers.
pub fn preview_token(n: usize) -> String {
    format!("{:032x}", 0xfeed_0000_usize + n)
}

/// The two items the fake previews for any plan but `[edit.undo]`.
pub fn two_items() -> Value {
    json!([
        {"thing": "Track 2, Vox", "change": "deleted", "composers_work": true},
        {"thing": "Track 3, Bass", "change": "renamed to \"Low\"", "composers_work": true},
    ])
}

/// `previews` counts the previews seen so far, this one included.
fn canned(server: &Server, c: &Command, previews: usize) -> Value {
    match c {
        Command::CommandRun { id, .. } => json!({ "status": format!("ran {id}") }),
        Command::StateGet { target, .. } => json!({ "target": format!("{target:?}") }),
        Command::CoProducerBegin { name, .. } => json!({ "begun": name, "doc": "d1" }),
        Command::CoProducerNotes { .. } => json!({
            "doc": "d1",
            "text": "Keep the drums dry.\n",
            "truncated": false,
            "path": "/x/Song.vizdaw/Notes.md",
        }),
        Command::CoProducerNote { .. } => json!({ "doc": "d1", "request": null }),
        Command::CoProducerExportFolder { .. } => {
            json!({ "path": "/x/Song \u{2014} Stems \u{2014} 2026-11-16" })
        }
        Command::CoProducerExport { .. } => json!({
            "doc": "d1",
            "folder": "/x/Song \u{2014} Stems \u{2014} 2026-11-16",
            "written": ["/x/\u{2026}/Kick.wav", "/x/\u{2026}/Bass.wav"],
            "summary": null,
        }),
        Command::CoProducerEnd { .. } => json!({ "ended": true }),
        Command::CoProducerStatus { .. } => json!({ "request": null }),
        Command::CoProducerPreview { plan, .. } => {
            let undo_only = plan.len() == 1 && plan[0].id == "edit.undo";
            if undo_only {
                json!({
                    "doc": 1,
                    "token": preview_token(previews),
                    "needs_yes": false,
                    "items": [{"thing": "Undo", "change": "Co-Producer: Add a Bus", "composers_work": false}],
                })
            } else {
                json!({ "doc": 1, "token": preview_token(previews), "needs_yes": true, "items": two_items() })
            }
        }
        Command::CoProducerApply { plan, .. } => json!({
            "doc": 1,
            "results": plan.iter().map(|p| json!({"id": p.id, "status": "ran"})).collect::<Vec<_>>(),
            "undo_label": "Co-Producer: Tidy Up",
        }),
        Command::CoProducerStarter { .. } => json!({
            "doc": "d2", "window": 2, "title": "Song", "undo_label": "Co-Producer: Start Song",
            "tracks": [{"name": "Keys", "plays": "Built-in Synth"}], "buses": [], "lines": [],
        }),
        Command::CoProducerMidiNotes { seq, lane, .. } => json!({
            "doc": "d1", "seq": seq, "lane": lane, "name": "Keys",
            "notes": [[0, 60, 3840, 100]], "truncated": false,
        }),
        Command::CoProducerAutomation { target, .. } => json!({
            "doc": "d1", "target": target, "target_name": "Keys fader", "unit": "dB",
            "min": -144.0, "max": 12.0, "start_bar": 1, "end_bar": 2,
            "points": [[0, -6.0], [15360, -3.0], [30720, 0.0]], "truncated": false,
        }),
        Command::UiScreenshot { .. } => {
            let path = server.screenshot_path();
            std::fs::write(&path, PNG).unwrap();
            json!({ "path": path })
        }
        _ => json!({ "ok": true }),
    }
}

impl FakeApp {
    /// Listens at `support`'s Co-Producer socket and answers.
    pub fn start(support: &Path) -> FakeApp {
        let config = Config {
            role: Role::CoProducer,
            ..Config::default()
        };
        let server = Arc::new(Mutex::new(Server::start(&socket(support), config).unwrap()));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let script = Arc::new(Mutex::new(VecDeque::new()));
        let thread = {
            let (server, stop, seen, script) = (
                Arc::clone(&server),
                Arc::clone(&stop),
                Arc::clone(&seen),
                Arc::clone(&script),
            );
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let s = server.lock().unwrap();
                    let Some(call) = s.next_call() else {
                        drop(s);
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    };
                    let previews = {
                        let mut seen = seen.lock().unwrap();
                        seen.push(call.command.clone());
                        seen.iter()
                            .filter(|c| matches!(c, Command::CoProducerPreview { .. }))
                            .count()
                    };
                    let outcome = match script.lock().unwrap().pop_front() {
                        Some(Answer::Ok(v)) => Ok(v),
                        Some(Answer::Err(e)) => Err(e),
                        Some(Answer::ScreenshotAt(p)) => Ok(json!({ "path": p })),
                        Some(Answer::After(d, v)) => {
                            thread::sleep(d);
                            Ok(v)
                        }
                        None => Ok(canned(&s, &call.command, previews)),
                    };
                    s.reply(call.token, outcome);
                }
            })
        };
        FakeApp {
            server,
            stop,
            thread: Some(thread),
            seen,
            script,
        }
    }

    /// Answers the next call with `a`.
    pub fn script(&self, a: Answer) {
        self.script.lock().unwrap().push_back(a);
    }

    /// Every call that reached the app, in order.
    pub fn seen(&self) -> Vec<Command> {
        self.seen.lock().unwrap().clone()
    }

    /// The `co_producer.apply` calls that reached the app.
    pub fn applies(&self) -> Vec<Command> {
        self.seen()
            .into_iter()
            .filter(|c| matches!(c, Command::CoProducerApply { .. }))
            .collect()
    }

    /// The `command.run` calls that reached the app.
    pub fn runs(&self) -> Vec<Command> {
        self.seen()
            .into_iter()
            .filter(|c| matches!(c, Command::CommandRun { .. }))
            .collect()
    }

    /// Stops answering and closes the socket (its connections drop).
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
        self.server.lock().unwrap().stop();
    }
}

impl Drop for FakeApp {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A peer at the Co-Producer socket that reads one line from each
/// connection and closes it without answering, as an app that went away
/// mid-call would.
pub struct DroppingApp {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    lines: Arc<Mutex<Vec<String>>>,
}

impl DroppingApp {
    /// Listens at `support`'s Co-Producer socket.
    pub fn start(support: &Path) -> DroppingApp {
        let path = socket(support);
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let lines = Arc::new(Mutex::new(Vec::new()));
        let thread = {
            let (stop, lines) = (Arc::clone(&stop), Arc::clone(&lines));
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false).unwrap();
                            stream
                                .set_read_timeout(Some(Duration::from_secs(20)))
                                .unwrap();
                            let mut line = String::new();
                            let _ = BufReader::new(&stream).read_line(&mut line);
                            if !line.is_empty() {
                                lines.lock().unwrap().push(line);
                            }
                            drop(stream);
                        }
                        Err(_) => thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        DroppingApp {
            path,
            stop,
            thread: Some(thread),
            lines,
        }
    }

    /// Every line it read.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    /// Stops listening and removes the socket.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for DroppingApp {
    fn drop(&mut self) {
        self.stop();
    }
}
