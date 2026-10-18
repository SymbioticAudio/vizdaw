//! The control socket: private directory, stale sockets, calls, limits,
//! timeouts and shutdown (spec §4.2, §8). Success paths wait generously
//! (slow CI); only deliberate timeouts are short.

use core_control::client::{CallError, Client};
use core_control::protocol::MAX_LINE;
use core_control::server::{Config, Server};
use core_control::{Command, ErrorCode};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const LONG: Duration = Duration::from_secs(20);

/// A short private directory (socket paths are limited to 103 bytes).
fn private_dir() -> tempfile::TempDir {
    let d = tempfile::Builder::new()
        .prefix("vzc")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

fn sock(d: &tempfile::TempDir) -> PathBuf {
    d.path().join("ctl").join("s.sock")
}

/// Answers every call with `answer(command)` until `stop` is set.
fn fake_app(
    server: Arc<std::sync::Mutex<Server>>,
    stop: Arc<AtomicBool>,
    answer: impl Fn(&Command) -> Option<Value> + Send + 'static,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let call = server.lock().unwrap().next_call();
            match call {
                Some(c) => {
                    if let Some(v) = answer(&c.command) {
                        server.lock().unwrap().reply(c.token, Ok(v));
                    }
                }
                None => thread::sleep(Duration::from_millis(1)),
            }
        }
    })
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn it_listens_in_a_private_directory_and_cleans_up() {
    let d = private_dir();
    let path = sock(&d);
    let mut s = Server::start(&path, Config::default()).unwrap();
    assert_eq!(
        mode(path.parent().unwrap()),
        0o700,
        "the directory it made is private"
    );
    assert_eq!(mode(&path), 0o600, "the socket is the user's only");
    assert_eq!(s.dir(), path.parent().unwrap());
    assert!(s.screenshot_path().starts_with(path.parent().unwrap()));
    s.stop();
    assert!(!path.exists(), "the socket file goes on stop");
}

#[test]
fn unsafe_places_are_refused() {
    let d = private_dir();
    let open = d.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = Server::start(&open.join("s.sock"), Config::default())
        .err()
        .unwrap();
    assert!(e.0.contains("open to other users"), "{e}");
    assert!(Server::start(Path::new("rel/s.sock"), Config::default()).is_err());
    let long = d.path().join("x".repeat(120)).join("s.sock");
    assert!(
        Server::start(&long, Config::default())
            .err()
            .unwrap()
            .0
            .contains("at most 103")
    );
    let file = d.path().join("f.sock");
    std::fs::write(&file, b"").unwrap();
    assert!(
        Server::start(&file, Config::default())
            .err()
            .unwrap()
            .0
            .contains("not a socket")
    );
}

#[test]
fn a_stale_socket_is_replaced_but_a_live_one_is_not() {
    let d = private_dir();
    let path = sock(&d);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::set_permissions(
        path.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    drop(UnixListener::bind(&path).unwrap()); // leaves a dead socket file
    let live = Server::start(&path, Config::default()).unwrap();
    let e = Server::start(&path, Config::default()).err().unwrap();
    assert!(e.0.contains("another VizDAW"), "{e}");
    drop(live);
}

#[test]
fn calls_reach_the_app_and_local_commands_do_not() {
    let d = private_dir();
    let server = Arc::new(std::sync::Mutex::new(
        Server::start(&sock(&d), Config::default()).unwrap(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let app = fake_app(Arc::clone(&server), Arc::clone(&stop), |c| match c {
        Command::StateGet { .. } => Some(json!({"playing": false})),
        other => panic!("the app got {other:?}"),
    });
    let mut c = Client::connect(&sock(&d)).unwrap();
    assert_eq!(
        c.call("state.get", json!({"target": "transport"}), LONG)
            .unwrap(),
        json!({"playing": false})
    );
    let list = c.call("commands.list", json!({}), LONG).unwrap();
    assert!(list["commands"].as_array().unwrap().len() >= 38);
    assert_eq!(
        c.call("ping", json!({}), LONG).unwrap(),
        json!({"pong": true})
    );
    match c.call("state.get", json!({"target": "nope"}), LONG) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::InvalidParams),
        other => panic!("{other:?}"),
    }
    stop.store(true, Ordering::SeqCst);
    app.join().unwrap();
}

#[test]
fn garbage_and_overlong_lines_get_errors_and_the_connection_survives() {
    let d = private_dir();
    let _s = Server::start(&sock(&d), Config::default()).unwrap();
    let mut c = Client::connect(&sock(&d)).unwrap();
    c.send_line("this is not json").unwrap();
    let v: Value = serde_json::from_str(&c.read_line().unwrap()).unwrap();
    assert_eq!(v["error"]["code"], "parse_error");
    assert_eq!(v["id"], Value::Null);
    c.send_line(&"x".repeat(MAX_LINE + 10)).unwrap();
    let v: Value = serde_json::from_str(&c.read_line().unwrap()).unwrap();
    assert_eq!(v["error"]["code"], "parse_error");
    c.send_line("").unwrap(); // blank lines are skipped
    assert_eq!(
        c.call("ping", json!({}), LONG).unwrap(),
        json!({"pong": true})
    );
}

#[test]
fn a_call_the_app_never_answers_times_out_and_the_late_answer_is_dropped() {
    let d = private_dir();
    let config = Config {
        app_timeout: Duration::from_millis(200),
        wait_grace: Duration::from_millis(100),
        ..Config::default()
    };
    let s = Server::start(&sock(&d), config).unwrap();
    let mut c = Client::connect(&sock(&d)).unwrap();
    let t0 = Instant::now();
    match c.call("app.quit", json!({}), LONG) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::Timeout),
        other => panic!("{other:?}"),
    }
    assert!(t0.elapsed() < Duration::from_secs(10));
    // The app takes the call late: nobody waits for it any more.
    assert!(
        s.next_call().is_none(),
        "a call nobody waits for is skipped"
    );
    // A wait gets its own timeout plus the grace.
    let t0 = Instant::now();
    match c.call(
        "wait",
        json!({"condition": "playing", "timeout_ms": 300}),
        LONG,
    ) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::Timeout),
        other => panic!("{other:?}"),
    }
    assert!(
        t0.elapsed() >= Duration::from_millis(400),
        "{:?}",
        t0.elapsed()
    );
}

#[test]
fn too_many_connections_are_turned_away() {
    let d = private_dir();
    let config = Config {
        max_connections: 2,
        ..Config::default()
    };
    let _s = Server::start(&sock(&d), config).unwrap();
    let mut a = Client::connect(&sock(&d)).unwrap();
    let mut b = Client::connect(&sock(&d)).unwrap();
    // Both are served (each answers a ping), so both count.
    a.call("ping", json!({}), LONG).unwrap();
    b.call("ping", json!({}), LONG).unwrap();
    let mut c = Client::connect(&sock(&d)).unwrap();
    let v: Value = serde_json::from_str(&c.read_line().unwrap()).unwrap();
    assert_eq!(v["error"]["code"], "unavailable");
    drop(a);
    // Once a connection closes another is served (its thread ends first).
    let deadline = Instant::now() + LONG;
    loop {
        let mut d2 = Client::connect(&sock(&d)).unwrap();
        if d2.call("ping", json!({}), Duration::from_secs(5)).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "a freed slot was never reused");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn stopping_answers_pending_calls() {
    let d = private_dir();
    let mut s = Server::start(&sock(&d), Config::default()).unwrap();
    let path = sock(&d);
    let caller = thread::spawn(move || {
        let mut c = Client::connect(&path).unwrap();
        c.call("app.quit", json!({}), LONG)
    });
    let deadline = Instant::now() + LONG;
    let call = loop {
        if let Some(c) = s.next_call() {
            break c;
        }
        assert!(Instant::now() < deadline, "the call never arrived");
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(call.command, Command::AppQuit);
    s.stop();
    match caller.join().unwrap() {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::ShuttingDown),
        other => panic!("{other:?}"),
    }
    assert!(!s.reply(call.token, Ok(json!({}))), "too late");
}

#[test]
fn parallel_clients_get_their_own_replies() {
    let d = private_dir();
    let server = Arc::new(std::sync::Mutex::new(
        Server::start(&sock(&d), Config::default()).unwrap(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let app = fake_app(Arc::clone(&server), Arc::clone(&stop), |c| match c {
        Command::Wait { timeout_ms, .. } => Some(json!({"elapsed_ms": timeout_ms})),
        _ => None,
    });
    let handles: Vec<_> = (1..=3u64)
        .map(|k| {
            let path = sock(&d);
            thread::spawn(move || {
                let mut c = Client::connect(&path).unwrap();
                for _ in 0..20 {
                    let v = c
                        .call(
                            "wait",
                            json!({"condition": "playing", "timeout_ms": k}),
                            LONG,
                        )
                        .unwrap();
                    assert_eq!(v["elapsed_ms"], k);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    app.join().unwrap();
}
