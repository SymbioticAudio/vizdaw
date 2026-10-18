//! `vizctl` arguments, exit codes and output against a fake VizDAW (a
//! `core-control` server answered by a test thread).

use core_control::Command;
use core_control::server::{Config, Server};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::process::Command as Process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use vizctl::cli::{Plan, find_in_tree, parse};
use vizctl::launch::{LaunchOptions, launch};

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn request(s: &str) -> (String, Value, Duration) {
    match parse(&args(s), Some("/tmp/x.sock".into())).unwrap() {
        Plan::Send { request, .. } => (request.cmd, request.params, request.timeout),
        other => panic!("{other:?}"),
    }
}

#[test]
fn arguments_become_requests() {
    assert_eq!(
        request("click 10 20 --right --count 2 --window 5").1,
        json!({"point": {"x": 10.0, "y": 20.0}, "button": "right", "count": 2, "window": 5})
    );
    assert_eq!(
        request("drag 0.1:0000abcd 30,40 --steps 4 --mod option").1,
        json!({"from": "0.1:0000abcd", "to": {"x": 30.0, "y": 40.0}, "steps": 4, "modifiers": ["option"]})
    );
    assert_eq!(
        request("key s --mod command,shift").1,
        json!({"key": "s", "modifiers": ["command", "shift"]})
    );
    assert_eq!(
        request("type Hello there").1,
        json!({"text": "Hello there"})
    );
    assert_eq!(
        request(r#"run file.save_as {"path":"/tmp/a.vizdaw"} --window 3"#).1,
        json!({"id": "file.save_as", "params": {"path": "/tmp/a.vizdaw"}, "window": 3})
    );
    let (cmd, p, timeout) = request("wait tick_at_least --tick 100 --timeout-ms 90000");
    assert_eq!(cmd, "wait");
    assert_eq!(
        p,
        json!({"condition": "tick_at_least", "tick": 100, "timeout_ms": 90000})
    );
    assert!(
        timeout >= Duration::from_secs(120),
        "room for the wait's own timeout"
    );
    assert_eq!(
        request("new-demo --audio --window 2").1,
        json!({"audio": true}),
        "no window for app.*"
    );
    assert_eq!(request("state windows").0, "state.get");
    assert!(matches!(parse(&args("help"), None).unwrap(), Plan::Help));
    assert!(
        parse(&args("state transport"), None)
            .unwrap_err()
            .contains("VIZDAW_CONTROL")
    );
    assert!(parse(&args("frobnicate"), Some("/tmp/x".into())).is_err());
    assert!(
        parse(&args("click 1"), Some("/tmp/x".into())).is_ok(),
        "one argument is an element ID"
    );
    assert!(parse(&args("click 1 2 3"), Some("/tmp/x".into())).is_err());
    match parse(
        &args("launch --app /x/VizDAW.app --env VIZDAW_AUDIO=offline --demo"),
        None,
    )
    .unwrap()
    {
        Plan::Launch(o) => {
            assert!(o.demo);
            assert_eq!(
                o.env,
                vec![("VIZDAW_AUDIO".to_string(), "offline".to_string())]
            );
            assert_eq!(
                vizctl::launch::executable(&o.app).to_str().unwrap(),
                "/x/VizDAW.app/Contents/MacOS/VizDAW"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn find_flattens_matching_tree_nodes() {
    let tree = json!({"window": 3, "nodes": [
        {"id": "0:00000001", "role": "AXGroup", "label": "Arrangement", "children": [
            {"id": "0.0:00000002", "role": "AXButton", "label": "Block Chords", "frame": {"x": 1}},
        ]},
        {"id": "1:00000003", "role": "AXButton", "title": "Play"},
    ]});
    let found = find_in_tree(&tree, "Chords");
    assert_eq!(found["matches"].as_array().unwrap().len(), 1);
    assert_eq!(found["matches"][0]["id"], "0.0:00000002");
    assert_eq!(
        find_in_tree(&tree, "Play")["matches"][0]["role"],
        "AXButton"
    );
}

/// A fake VizDAW on a socket in a private folder.
struct Fake {
    _dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    stop: Arc<AtomicBool>,
    app: Option<thread::JoinHandle<()>>,
}

impl Fake {
    fn new() -> Fake {
        let dir = tempfile::Builder::new()
            .prefix("vzt")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.path().join("s.sock");
        let server = Arc::new(Mutex::new(
            Server::start(&socket, Config::default()).unwrap(),
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (Arc::clone(&server), Arc::clone(&stop));
        let app = thread::spawn(move || {
            while !st.load(Ordering::SeqCst) {
                let call = s.lock().unwrap().next_call();
                let Some(c) = call else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                let outcome = match c.command {
                    Command::StateGet { .. } => Ok(json!({"playing": true, "tick": 7})),
                    _ => Err(core_control::ControlError::new(
                        core_control::ErrorCode::Unavailable,
                        "Nothing to undo",
                    )),
                };
                s.lock().unwrap().reply(c.token, outcome);
            }
        });
        Fake {
            _dir: dir,
            socket,
            stop,
            app: Some(app),
        }
    }

    fn vizctl(&self, a: &str) -> (i32, String) {
        let out = Process::new(env!("CARGO_BIN_EXE_vizctl"))
            .arg("--socket")
            .arg(&self.socket)
            .args(args(a))
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
        )
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(a) = self.app.take() {
            let _ = a.join();
        }
    }
}

#[test]
fn the_binary_prints_results_and_exits_by_outcome() {
    let f = Fake::new();
    let (code, out) = f.vizctl("state transport");
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["tick"], 7);
    let (code, out) = f.vizctl("run edit.undo");
    assert_eq!(code, 1, "VizDAW answered with an error");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["error"]["code"], "unavailable");
    let (code, out) = f.vizctl("commands");
    assert_eq!(code, 0);
    assert!(out.contains("transport.play_stop"));
    assert_eq!(f.vizctl("frobnicate").0, 2, "bad usage");
    let gone = Process::new(env!("CARGO_BIN_EXE_vizctl"))
        .args(["--socket", "/tmp/no-such-vizdaw.sock", "ping"])
        .output()
        .unwrap();
    assert_eq!(gone.status.code(), Some(3), "no connection");
}

#[test]
fn launch_reports_an_app_that_exits_early() {
    let home = tempfile::Builder::new()
        .prefix("vzl")
        .tempdir_in("/tmp")
        .unwrap();
    let e = launch(&LaunchOptions {
        app: "/usr/bin/true".into(),
        home: Some(home.path().join("h")),
        demo: false,
        timeout: Duration::from_secs(60),
        env: vec![],
    })
    .unwrap_err();
    assert!(e.contains("exited"), "{e}");
    let mode = std::fs::metadata(home.path().join("h/ctl"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "the socket folder is private");
    assert!(
        launch(&LaunchOptions {
            app: "/no/such/VizDAW.app".into(),
            home: None,
            demo: false,
            timeout: Duration::from_secs(1),
            env: vec![],
        })
        .unwrap_err()
        .contains("no VizDAW executable")
    );
}

#[test]
fn restore_autosave_takes_only_true_or_false() {
    let some = Some("/tmp/x.sock".to_string());
    for (v, want) in [("true", true), ("false", false)] {
        match parse(
            &args(&format!("open /tmp/a.vizdaw --restore-autosave {v}")),
            some.clone(),
        )
        .unwrap()
        {
            Plan::Send { request, .. } => {
                assert_eq!(request.params["restore_autosave"], json!(want));
            }
            other => panic!("{other:?}"),
        }
    }
    let bad = parse(&args("open /tmp/a.vizdaw --restore-autosave yes"), some).unwrap_err();
    assert!(bad.contains("true or false"), "{bad}");
    let out = Process::new(env!("CARGO_BIN_EXE_vizctl"))
        .args([
            "--socket",
            "/tmp/none.sock",
            "open",
            "/tmp/a.vizdaw",
            "--restore-autosave",
            "1",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "usage error");
}

#[test]
fn bad_timeouts_are_usage_errors_not_panics() {
    for bad in ["nan", "inf", "-1", "-inf"] {
        let out = Process::new(env!("CARGO_BIN_EXE_vizctl"))
            .args(["--socket", "/tmp/none.sock", "--timeout", bad, "ping"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "--timeout {bad}");
    }
    for bad in ["-1", "nan", "1.5"] {
        let out = Process::new(env!("CARGO_BIN_EXE_vizctl"))
            .args([
                "--socket",
                "/tmp/none.sock",
                "wait",
                "playing",
                "--timeout-ms",
                bad,
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "--timeout-ms {bad}");
    }
    let (_, p, t) = request("wait playing --timeout-ms 99999999999");
    assert_eq!(p["timeout_ms"], 3_600_000);
    assert!(t <= Duration::from_secs(3700));
}

#[test]
fn dropping_launched_stops_the_child_unless_detached() {
    let spawn = || Process::new("/bin/sleep").arg("60").spawn().unwrap();
    let alive = |pid: u32| {
        Process::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .output()
            .unwrap()
            .status
            .success()
    };
    let child = spawn();
    let pid = child.id();
    drop(vizctl::launch::Launched::new(
        child,
        "/tmp/none.sock".into(),
        "/tmp".into(),
        "/tmp/none.log".into(),
    ));
    assert!(!alive(pid), "killed on drop");
    let child = spawn();
    let pid = child.id();
    vizctl::launch::Launched::new(
        child,
        "/tmp/none.sock".into(),
        "/tmp".into(),
        "/tmp/n".into(),
    )
    .detach();
    assert!(alive(pid), "detached stays running");
    let _ = Process::new("/bin/kill").arg(pid.to_string()).output();
}

fn tmp(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in("/tmp")
        .unwrap()
}

fn launch_into(home: &std::path::Path) -> Result<vizctl::launch::Launched, String> {
    launch(&LaunchOptions {
        app: "/usr/bin/true".into(),
        home: Some(home.to_path_buf()),
        demo: false,
        timeout: Duration::from_secs(60),
        env: vec![],
    })
}

/// README step 82: a second launch into a home whose VizDAW still runs
/// fails, exit 3, and leaves that VizDAW's log alone.
#[test]
fn launch_refuses_a_home_whose_vizdaw_still_answers() {
    let d = tmp("vzr");
    let home = d.path().join("h");
    let socket = home.join("ctl/vizdaw.sock");
    let _running = Server::start(&socket, Config::default()).unwrap();
    let log = home.join("vizdaw.log");
    std::fs::write(&log, "the running VizDAW's log\n").unwrap();
    let e = launch_into(&home).unwrap_err();
    assert!(e.contains("another VizDAW is listening on"), "{e}");
    assert!(e.contains(socket.to_str().unwrap()), "{e}");
    let out = Process::new(env!("CARGO_BIN_EXE_vizctl"))
        .args(["launch", "--app", "/usr/bin/true", "--home"])
        .arg(&home)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("another VizDAW is listening"),
        "{out:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "the running VizDAW's log\n",
        "the running app's log is untouched"
    );
}

/// Spec §10: the home is private. An existing `--home` must be a real
/// directory of this user's that others cannot write to; nothing in it
/// may be a planted link.
#[test]
fn launch_refuses_a_home_others_could_have_planted() {
    let d = tmp("vzp");
    // A link as the home.
    let target = d.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let link = d.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let e = launch_into(&link).unwrap_err();
    assert!(e.contains("is not a directory"), "{e}");
    // A home anyone can write to.
    let open = d.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    let e = launch_into(&open).unwrap_err();
    assert!(e.contains("writable by other users"), "{e}");
    // A planted link for the socket folder inside a private home.
    let home = d.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(&target, home.join("ctl")).unwrap();
    let e = launch_into(&home).unwrap_err();
    assert!(e.contains("ctl is not a directory"), "{e}");
    // A planted link for the log: the file it points at is not truncated.
    let home2 = d.path().join("home2");
    std::fs::create_dir(&home2).unwrap();
    std::fs::set_permissions(&home2, std::fs::Permissions::from_mode(0o700)).unwrap();
    let victim = d.path().join("victim.txt");
    std::fs::write(&victim, "keep me").unwrap();
    std::os::unix::fs::symlink(&victim, home2.join("vizdaw.log")).unwrap();
    let e = launch_into(&home2).unwrap_err();
    assert!(e.contains("not a regular file"), "{e}");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me");
    // The binary exits 3 with the reason.
    let out = Process::new(env!("CARGO_BIN_EXE_vizctl"))
        .args(["launch", "--app", "/usr/bin/true", "--home"])
        .arg(&open)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
}
