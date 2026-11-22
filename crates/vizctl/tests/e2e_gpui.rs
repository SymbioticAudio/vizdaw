//! End to end for the GPUI app: launches it with the control channel in a
//! throwaway home, plays the demo, saves, quits, relaunches and reopens.
//! Ignored by default: it needs a window server and a built app. Run it
//! with
//!
//! ```text
//! scripts/make-gpui-app.sh   # macOS: target/VizDAW-GPUI.app
//! VIZDAW_E2E_APP=target/VizDAW-GPUI.app cargo test -p vizctl --test e2e_gpui -- --ignored --nocapture
//! # Linux: VIZDAW_E2E_APP=target/debug/vizdaw-gpui (under xvfb-run -a, WAYLAND_DISPLAY unset)
//! ```
//!
//! The launch helper is copied from `e2e.rs` (only what is needed), not
//! shared. Playing is audible for a moment.

use core_control::client::{CallError, Client};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use vizctl::launch::{LaunchOptions, Launched, launch, log_tail};

const WAIT_MS: u64 = 60_000;

struct App {
    launched: Launched,
    client: Client,
}

fn app_path() -> PathBuf {
    let app =
        std::env::var("VIZDAW_E2E_APP").expect("set VIZDAW_E2E_APP to the GPUI .app or its binary");
    let p = PathBuf::from(app);
    if p.is_absolute() {
        p
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(p)
    }
}

impl App {
    fn start(home: &std::path::Path, demo: bool) -> App {
        let launched = launch(&LaunchOptions {
            app: app_path(),
            home: Some(home.to_path_buf()),
            demo,
            timeout: Duration::from_secs(180),
            env: vec![("VIZDAW_AUDIO".to_string(), "offline".to_string())],
        })
        .unwrap_or_else(|e| panic!("launch: {e}"));
        let client = Client::connect(&launched.socket).unwrap();
        App { launched, client }
    }

    fn call(&mut self, cmd: &str, params: Value) -> Value {
        let timeout = Duration::from_millis(WAIT_MS);
        self.client
            .call(cmd, params.clone(), timeout)
            .unwrap_or_else(|e| {
                panic!(
                    "{cmd} {params}: {e}\nlog:\n{}",
                    log_tail(&self.launched.log, 40)
                )
            })
    }

    fn call_err(&mut self, cmd: &str, params: Value) -> core_control::ControlError {
        match self
            .client
            .call(cmd, params, Duration::from_millis(WAIT_MS))
        {
            Err(CallError::Reply(e)) => e,
            other => panic!("{cmd}: expected an error, got {other:?}"),
        }
    }

    fn command(&mut self, id: &str, params: Value) -> Value {
        self.call("command.run", json!({"id": id, "params": params}))
    }

    fn quit(mut self) {
        assert_eq!(
            self.call("app.quit", json!({"discard": true}))["quitting"],
            true
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(Some(status)) = self.launched.child.try_wait() {
                assert!(status.success(), "the app exited with {status}");
                return;
            }
            if Instant::now() > deadline {
                let _ = self.launched.child.kill();
                panic!(
                    "the app did not quit; log:\n{}",
                    log_tail(&self.launched.log, 40)
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn temp_home() -> tempfile::TempDir {
    let home = tempfile::Builder::new()
        .prefix("vzg")
        .tempdir_in("/tmp")
        .unwrap();
    // The temp dir takes the process umask (0775 under Ubuntu's 002);
    // launch wants a home nobody else can write to.
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    home
}

/// The first node (depth first) whose label is exactly `label`.
fn find_node(nodes: &Value, label: &str) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        if n["label"].as_str() == Some(label) {
            Some(n.clone())
        } else {
            find_node(&n["children"], label)
        }
    })
}

/// The first node whose label starts with `prefix`: its id and label.
fn labelled_like(nodes: &Value, prefix: &str) -> Option<(Value, String)> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        let l = n["label"].as_str().unwrap_or("");
        if l.starts_with(prefix) {
            Some((n["id"].clone(), l.to_string()))
        } else {
            labelled_like(&n["children"], prefix)
        }
    })
}

/// Polls the window's tree until `find` returns something.
fn wait_in_tree<T>(
    app: &mut App,
    window: &Value,
    what: &str,
    find: impl Fn(&Value) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(found) = find(&tree["nodes"]) {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "no {what} within {WAIT_MS} ms in {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Polls `state.get target` until `ok` holds.
fn wait_for_state(
    app: &mut App,
    window: &Value,
    target: &str,
    ok: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let r = app.call("state.get", json!({"target": target, "window": window}));
        if ok(&r) {
            return r;
        }
        assert!(
            Instant::now() < deadline,
            "{target} state within {WAIT_MS} ms: {r}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The tracks of `state.get document`, as (kind, name).
fn track_list(app: &mut App, window: &Value) -> Vec<(String, String)> {
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["kind"].as_str().unwrap_or("?").to_string(),
                l["name"].as_str().unwrap_or("?").to_string(),
            )
        })
        .collect()
}

/// The windows' numbers and kinds.
fn window_numbers(app: &mut App) -> Vec<(Value, Value)> {
    let w = app.call("state.get", json!({"target": "windows"}));
    w["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["number"].clone(), x["kind"].clone()))
        .collect()
}

#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_demo_plays_saves_and_reopens() {
    let home = temp_home();
    let saved = home.path().join("t.vizdaw");

    let mut app = App::start(home.path(), true);
    app.command("transport.play_stop", json!({}));
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let t = app.call("state.get", json!({"target": "transport"}));
        if t["playing"] == true {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "never started playing: {t}\nlog:\n{}",
            log_tail(&app.launched.log, 40)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // Stopped at once: the speakers are on.
    app.command("transport.play_stop", json!({}));
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    while app.call("state.get", json!({"target": "transport"}))["playing"] != false {
        assert!(Instant::now() < deadline, "never stopped playing");
        std::thread::sleep(Duration::from_millis(100));
    }

    app.command("file.save_as", json!({"path": saved}));
    assert!(saved.exists(), "{} was not written", saved.display());
    app.quit();

    let mut app = App::start(home.path(), false);
    app.call("app.open", json!({"path": saved}));
    let windows = app.call("state.get", json!({"target": "windows"}));
    assert!(
        windows["windows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["title"] == "t"),
        "no window named t in {windows}"
    );
    app.quit();
}

/// Ported from `new_project_tracks_over_the_channel`: a new song opens with
/// "Sequence 1" and no tracks; the Add Track channel click only sets the
/// status; the three New Track commands, Undo, Duplicate, hostile parameters.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_new_project_tracks_over_the_channel() {
    use core_control::ErrorCode;
    let home = temp_home();
    let mut app = App::start(home.path(), false);
    let made = app.command("file.new", json!({}));
    let window = made["window"].clone();
    wait_in_tree(&mut app, &window, "Track Headers", |n| {
        find_node(n, "Track Headers")
    });

    // The start state: Sequence 1, selected, no tracks, and the add row.
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    let seqs = doc["sequences"].as_array().unwrap();
    assert_eq!(seqs.len(), 1, "{doc}");
    assert_eq!(seqs[0]["name"], "Sequence 1", "{doc}");
    assert_eq!(doc["selected_seq"], seqs[0]["id"], "{doc}");
    assert!(track_list(&mut app, &window).is_empty(), "{doc}");
    let add = wait_in_tree(&mut app, &window, "Add Track", |n| {
        find_node(n, "Add Track")
    });
    // A channel click on Add Track opens no menu: the status says what to
    // run instead, and the app answers at once.
    app.call("ui.click", json!({"window": window, "element": add["id"]}));
    let editor = wait_for_state(&mut app, &window, "editor", |e| {
        e["status"]
            .as_str()
            .is_some_and(|s| s.starts_with("Add Track opens a menu"))
    });
    assert_eq!(
        editor["status"],
        "Add Track opens a menu: use track.new_audio, track.new_instrument or track.new_midi over the control channel."
    );
    let t = Instant::now();
    app.call("state.get", json!({"target": "document", "window": window}));
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "no modal menu holds the main thread"
    );
    // Dropped: the header column / Add Track row pixel checks (no ui.screenshot in GPUI yet).

    // The three kinds, below one another.
    for id in ["track.new_audio", "track.new_instrument", "track.new_midi"] {
        app.call("command.run", json!({"id": id, "window": window}));
    }
    let tracks = track_list(&mut app, &window);
    let kinds: Vec<&str> = tracks.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(kinds, ["audio", "instrument", "midi"], "{tracks:?}");
    for prefix in [
        "Audio track, Audio 1",
        "Instrument track, Inst 1",
        "MIDI track, MIDI 1",
    ] {
        wait_in_tree(&mut app, &window, prefix, |n| labelled_like(n, prefix));
    }
    // Dropped: the first row's separator-line pixel check (no ui.screenshot in GPUI yet).

    // Undo removes the last one; Duplicate copies the first.
    app.call("command.run", json!({"id": "edit.undo", "window": window}));
    let tracks = track_list(&mut app, &window);
    assert_eq!(tracks.len(), 2, "{tracks:?}");
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    let first = doc["lanes"][0]["id"].clone();
    app.call(
        "command.run",
        json!({"id": "track.duplicate", "params": {"lane": first}, "window": window}),
    );
    let tracks = track_list(&mut app, &window);
    assert_eq!(tracks.len(), 3, "{tracks:?}");
    assert_eq!(tracks[1].1, "Audio 1 copy", "{tracks:?}");

    // Hostile parameters change nothing.
    let before = track_list(&mut app, &window);
    let e = app.call_err(
        "command.run",
        json!({"id": "track.new_audio", "params": {"seq": -1}, "window": window}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    assert_eq!(track_list(&mut app, &window), before);
    let e = app.call_err(
        "command.run",
        json!({"id": "track.new_audio", "params": {"seq": 999_999}, "window": window}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "{e}");
    assert_eq!(track_list(&mut app, &window), before);
    let e = app.call_err(
        "command.run",
        json!({"id": "track.rename", "window": window}),
    );
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e}");
    assert_eq!(track_list(&mut app, &window), before);
    let e = app.call_err(
        "command.run",
        json!({"id": "track.rename", "params": {"name": "   "}, "window": window}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "{e}");
    assert_eq!(track_list(&mut app, &window), before);
    // Dropped: the ui.screenshot `live: "yes"` invalid-params check (no ui.screenshot in GPUI yet).
    app.quit();
}

/// Ported from `control_close_of_a_dirty_song_does_not_prompt`: `file.close`
/// over the channel closes a song with unsaved changes, untitled or saved,
/// at once.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_control_close_of_a_dirty_song_does_not_prompt() {
    use core_control::ErrorCode;
    let home = temp_home();
    let mut app = App::start(home.path(), false);
    let saved = home.path().join("Dirty.vizdaw");
    for save in [false, true] {
        let w = app.command("file.new", json!({}))["window"].clone();
        if save {
            app.call(
                "command.run",
                json!({"id": "file.save", "params": {"path": saved.display().to_string()}, "window": w}),
            );
        }
        // An edit: the song now has unsaved changes.
        app.call("command.run", json!({"id": "track.new_midi", "window": w}));
        let d = app.call("state.get", json!({"target": "document", "window": w}));
        assert_eq!(d["dirty"], true, "{d}");
        let before = window_numbers(&mut app);
        assert!(before.iter().any(|(n, _)| *n == w), "{before:?}");

        // The call returns within the normal timeout: no prompt waits.
        let started = Instant::now();
        let r = app.client.call(
            "command.run",
            json!({"id": "file.close", "window": w}),
            Duration::from_millis(WAIT_MS),
        );
        assert!(
            r.is_ok(),
            "file.close (saved: {save}): {r:?}\nlog:\n{}",
            log_tail(&app.launched.log, 40)
        );
        assert!(
            started.elapsed() < Duration::from_millis(WAIT_MS),
            "{:?}",
            started.elapsed()
        );

        // The window goes (a condition with a timeout, not a sleep).
        let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
        loop {
            let now = window_numbers(&mut app);
            if now.len() < before.len() && now.iter().all(|(n, _)| *n != w) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the window stayed (saved: {save}): {now:?}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let e = app.call_err("state.get", json!({"target": "document", "window": w}));
        assert_eq!(e.code, ErrorCode::NoWindow, "{e}");
    }
    // Quitting meets no prompt either.
    app.quit();
}

/// Ported from `app_quit_refuses_while_a_song_is_unsaved`: `app.quit` without
/// `discard` replies `unsaved_changes` naming the song and the app keeps
/// running; with `discard: true` it quits.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_app_quit_refuses_while_a_song_is_unsaved() {
    use core_control::ErrorCode;
    let home = temp_home();
    let mut app = App::start(home.path(), false);
    let w = app.command("file.new", json!({}))["window"].clone();
    app.call("command.run", json!({"id": "track.new_midi", "window": w}));
    let d = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(d["dirty"], true, "{d}");
    let title = d["title"].as_str().unwrap().to_string();
    for params in [json!({}), json!({"discard": false})] {
        let e = app.call_err("app.quit", params);
        assert_eq!(e.code, ErrorCode::UnsavedChanges, "{e}");
        assert!(e.message.contains(&title), "names {title:?}: {e}");
    }
    // Still running, the song still open.
    app.call("ping", json!({}));
    let d = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(d["dirty"], true, "{d}");
    // `discard: true` quits (App::quit sends it).
    app.quit();
}
