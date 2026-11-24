//! End to end for the GPUI app: launches it with the control channel in a
//! throwaway home, plays the demo, saves, quits, relaunches and reopens.
//! Ignored by default: it needs a window server and a built app. Run it
//! with
//!
//! ```text
//! scripts/make-gpui-app.sh   # macOS: target/VizDAW-GPUI.app
//! VIZDAW_E2E_APP=target/VizDAW-GPUI.app cargo test -p vizctl --test e2e_gpui -- --ignored --nocapture
//! # Linux: cargo build -p vizdaw-gpui --features screenshot, then
//! # VIZDAW_E2E_APP=target/debug/vizdaw-gpui (under xvfb-run -a, WAYLAND_DISPLAY unset)
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
        App::start_with(home, demo, &[])
    }

    /// As [`App::start`], with `extra` in the app's environment too.
    fn start_with(home: &std::path::Path, demo: bool, extra: &[(&str, &str)]) -> App {
        let mut env = vec![("VIZDAW_AUDIO".to_string(), "offline".to_string())];
        env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let launched = launch(&LaunchOptions {
            app: app_path(),
            home: Some(home.to_path_buf()),
            demo,
            timeout: Duration::from_secs(180),
            env,
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

    fn wait(&mut self, window: &Value, condition: Value) -> Value {
        let mut p = condition;
        p["timeout_ms"] = json!(WAIT_MS);
        p["window"] = window.clone();
        self.call("wait", p)
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

/// Opens the demo the way the Swift tests do and waits for its document.
fn start_demo(home: &std::path::Path) -> (App, Value) {
    let mut app = App::start(home, false);
    let opened = app.call("app.new_demo", json!({}));
    let window = opened["window"].clone();
    assert!(window.as_i64().is_some_and(|w| w > 0), "{opened}");
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    (app, window)
}

/// Polls the tree until the node labelled exactly `label` exists; returns
/// it (its `id`, `value` and `focused`).
fn wait_for_node(app: &mut App, window: &Value, label: &str) -> Value {
    wait_in_tree(app, window, label, |nodes| find_node(nodes, label))
}

/// Polls the tree until the node labelled `label` has the value `value`.
fn wait_for_value(app: &mut App, window: &Value, label: &str, value: &str) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let n = wait_for_node(app, window, label);
        if n["value"].as_str() == Some(value) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{label} is not {value:?} within {WAIT_MS} ms: {n}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Polls `state.get editor` until `ok` holds; returns it.
fn wait_for_editor(
    app: &mut App,
    window: &Value,
    what: &str,
    ok: impl Fn(&Value) -> bool,
) -> Value {
    // `wait_for_state` names the target in its failure; `what` adds context.
    let _ = what;
    wait_for_state(app, window, "editor", ok)
}

/// The transport's tick, read until it is `tick`.
fn wait_for_tick(app: &mut App, window: &Value, tick: i64) {
    wait_for_state(app, window, "transport", |t| {
        t["tick"].as_i64() == Some(tick)
    });
}

/// The ID of the tree's button labelled (or titled) exactly "Play". GPUI's
/// role is `button` where the Swift app's was `AXButton`.
fn play_button(nodes: &Value) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        let exact = |k: &str| n[k].as_str() == Some("Play");
        if n["role"] == "button" && (exact("label") || exact("title")) {
            Some(n["id"].clone())
        } else {
            play_button(&n["children"])
        }
    })
}

/// Ported from `demo_play_screenshot_stop`, with its last step: List
/// (`editors.list`) opens the detail pane on the shown sequence, and
/// `wait detail_kind list` sees it.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_demo_play_screenshot_stop() {
    let home = temp_home();
    let (mut app, window) = start_demo(home.path());

    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    let song = doc["sequences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Song")
        .unwrap_or_else(|| panic!("no Song in {doc}"))["id"]
        .clone();
    assert_eq!(doc["selected_seq"], song, "Song is selected");

    // A window number that names no window is `no_window` for every
    // command (spec 5.3).
    for (cmd, p) in [
        (
            "state.get",
            json!({"target": "document", "window": 999_999_999}),
        ),
        (
            "command.run",
            json!({"id": "transport.play_stop", "window": 999_999_999}),
        ),
        ("ui.tree", json!({"window": 999_999_999})),
    ] {
        let e = app.call_err(cmd, p);
        assert_eq!(e.code, core_control::ErrorCode::NoWindow, "{cmd}: {e}");
    }

    // A command that would open a dialog fails instead, naming it.
    let e = app.call_err("command.run", json!({"id": "file.save", "window": window}));
    assert_eq!(e.dialog.as_deref(), Some("Save"), "{e}");

    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": window}),
    );
    app.wait(&window, json!({"condition": "playing"}));
    let t0 = app.call(
        "state.get",
        json!({"target": "transport", "window": window}),
    )["tick"]
        .as_i64()
        .unwrap();
    app.wait(
        &window,
        json!({"condition": "tick_at_least", "tick": t0 + 1}),
    );

    let shot = app.launched.home.join("playing.png");
    let r = app.call("ui.screenshot", json!({"window": window, "path": shot}));
    let png = std::fs::read(&shot).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "a PNG");
    assert!(r["width"].as_u64().unwrap() >= 400, "{r}");
    // The screenshot shows text: the Bar readout's box holds bright glyph
    // pixels. Without GPUI's `font-kit` feature the macOS build drew no
    // text at all while every tree-based check still passed.
    let ws = app.call("state.get", json!({"target": "windows"}));
    let frame_w = ws["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["number"] == window)
        .unwrap_or_else(|| panic!("no window {window} in {ws}"))["frame"]["width"]
        .as_f64()
        .unwrap();
    let img = image::open(&shot).unwrap().to_rgb8();
    let scale = f64::from(img.width()) / frame_w;
    let bar = wait_for_node(&mut app, &window, "Bar Readout");
    let f = &bar["frame"];
    let (x0, y0) = (f["x"].as_f64().unwrap(), f["y"].as_f64().unwrap());
    let (w, h) = (f["width"].as_f64().unwrap(), f["height"].as_f64().unwrap());
    let mut bright = 0;
    for y in (y0 * scale) as u32..((y0 + h) * scale) as u32 {
        for x in (x0 * scale) as u32..((x0 + w) * scale) as u32 {
            if x < img.width()
                && y < img.height()
                && img.get_pixel(x, y).0.iter().all(|&c| c > 0xb0)
            {
                bright += 1;
            }
        }
    }
    assert!(
        bright >= 20,
        "no text drawn in the Bar readout ({bright} bright pixels) in {}",
        shot.display()
    );

    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": window}),
    );
    app.wait(&window, json!({"condition": "stopped"}));

    let tree = app.call("ui.tree", json!({"window": window}));
    let found = vizctl::cli::find_in_tree(&tree, "Arrangement");
    assert!(
        !found["matches"].as_array().unwrap().is_empty(),
        "no Arrangement in the tree"
    );

    app.call(
        "command.run",
        json!({"id": "editors.list", "window": window}),
    );
    app.wait(&window, json!({"condition": "detail_kind", "kind": "list"}));
    app.quit();
}

/// Events into the app's own window: a click on the toolbar's Play button
/// plays, Space (the Transport menu's key) stops.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_click_play_and_press_space() {
    let home = temp_home();
    let (mut app, window) = start_demo(home.path());
    // The toolbar may come up after the window (slow runners): poll the
    // tree for the button labelled exactly "Play".
    let id = wait_in_tree(&mut app, &window, "Play button", play_button);
    app.call("ui.click", json!({"window": window, "element": id}));
    app.wait(&window, json!({"condition": "playing"}));
    app.call("ui.key", json!({"window": window, "key": "space"}));
    app.wait(&window, json!({"condition": "stopped"}));
    app.quit();
}

/// With Loop on, playing past the end of the sequence wraps to bar 1
/// instead of stopping.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_loop_wraps_at_the_end() {
    let home = temp_home();
    let (mut app, window) = start_demo(home.path());
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "window": window, "params": params}),
        )
    };
    let button = wait_for_node(&mut app, &window, "Loop")["id"].clone();
    app.call("ui.click", json!({"window": window, "element": button}));
    for _ in 0..11 {
        run(&mut app, "transport.fast_forward", json!({}));
    }
    run(&mut app, "transport.play", json!({}));
    app.wait(&window, json!({"condition": "playing"}));
    std::thread::sleep(Duration::from_millis(5000));
    let t = app.call(
        "state.get",
        json!({"target": "transport", "window": window}),
    );
    assert_eq!(t["playing"], true, "the loop ended at the end: {t}");
    app.quit();
}

/// Ported from `transport_jumps_readouts_and_fields_over_the_channel`,
/// steps 0-6. Step 7 (a typed Punch In refuses Record) is left out: Punch
/// and Record are a later slice.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_transport_jumps_readouts_and_fields_over_the_channel() {
    use core_control::ErrorCode;
    const BAR: i64 = 61_440;
    let home = temp_home();
    // The demo's shown sequence: 4/4 at 120 BPM over its first ten bars.
    let (mut app, w) = start_demo(home.path());
    let tick = |app: &mut App| {
        app.call("state.get", json!({"target": "transport", "window": w}))["tick"]
            .as_i64()
            .unwrap()
    };
    let click = |app: &mut App, label: &str, mods: Value| {
        let n = wait_for_node(app, &w, label);
        app.call(
            "ui.click",
            json!({"window": w, "element": n["id"], "modifiers": mods}),
        );
    };
    // One click in a field selects all its text, so what is typed
    // replaces it.
    let select_field = |app: &mut App, label: &str| click(app, label, json!([]));
    let no_alert = |app: &mut App| {
        let ws = app.call("state.get", json!({"target": "windows"}));
        assert_eq!(ws["alerts"].as_array().unwrap().len(), 0, "{ws}");
        let doc = app.call("state.get", json!({"target": "document", "window": w}));
        assert!(doc["alert"].is_null(), "no dialog: {doc}");
    };

    // A bar of just what this test reads, so nothing folds into the
    // overflow button in a small window.
    app.call(
        "command.run",
        json!({"id": "ui.set_transport_bar",
            "params": {"items": "cluster,bar,time,tempo,signature,position_field"}}),
    );

    // 1. The readouts at the start.
    wait_for_tick(&mut app, &w, 0);
    let t0 = app.call("state.get", json!({"target": "transport", "window": w}));
    let bbu0 = t0["position"].as_str().unwrap().to_string();
    assert_eq!(bbu0, "1.1.0", "{t0}");
    wait_for_value(&mut app, &w, "Bar Readout", &bbu0);
    wait_for_value(&mut app, &w, "Time Readout", "0:00.000");
    wait_for_value(&mut app, &w, "Tempo Readout", "120.00");
    wait_for_value(&mut app, &w, "Signature Readout", "4/4");

    // 2. A click on Fast Forward goes one bar; the command another;
    // Rewind one back; Return to Start to 0; Rewind at 0 stays there.
    click(&mut app, "Fast Forward", json!([]));
    wait_for_tick(&mut app, &w, BAR);
    wait_for_value(&mut app, &w, "Time Readout", "0:02.000");
    app.call(
        "command.run",
        json!({"id": "transport.fast_forward", "window": w}),
    );
    wait_for_tick(&mut app, &w, 2 * BAR);
    click(&mut app, "Rewind", json!([]));
    wait_for_tick(&mut app, &w, BAR);
    click(&mut app, "Return to Start", json!([]));
    wait_for_tick(&mut app, &w, 0);
    wait_for_value(&mut app, &w, "Bar Readout", &bbu0);
    click(&mut app, "Rewind", json!([]));
    assert_eq!(tick(&mut app), 0, "Rewind stops at bar 1");
    no_alert(&mut app);

    // 3. A shift-click with no marker: the status line says so, nothing moves.
    click(&mut app, "Fast Forward", json!(["shift"]));
    let e = wait_for_editor(&mut app, &w, "no marker", |e| {
        e["status"] == "No marker after the position."
    });
    assert_eq!(tick(&mut app), 0, "{e}");
    no_alert(&mut app);

    // 4. Shift-. one bar on, the keypad's minus one bar back.
    app.call(
        "ui.key",
        json!({"window": w, "key": ".", "modifiers": ["shift"]}),
    );
    wait_for_tick(&mut app, &w, BAR);
    app.call("ui.key", json!({"window": w, "key": "keypad_minus"}));
    wait_for_tick(&mut app, &w, 0);

    // 5. Hostile parameters are refused and move nothing.
    for (id, params) in [
        ("transport.rewind", json!({"bars": 2})),
        ("transport.fast_forward", json!("x")),
    ] {
        let e = app.call_err(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        );
        assert_eq!(e.code, ErrorCode::InvalidParams, "{id} {params}: {e}");
        assert_eq!(tick(&mut app), 0, "{id} {params}");
    }
    no_alert(&mut app);

    // 6. A typed position is cancelled by a click on a view that never
    // takes the keyboard, and by one on the arrangement (which does);
    // Return commits (the click selects all, so no stale digit is left).
    let old = wait_for_node(&mut app, &w, "Position")["value"].clone();
    for away in ["Bar Readout", "Arrangement"] {
        click(&mut app, "Position", json!([]));
        app.call("ui.key", json!({"window": w, "text": "5"}));
        // The typed text replaced the old one before the click away, so
        // the wait below sees the cancel and not a click that never began
        // an edit.
        wait_for_value(&mut app, &w, "Position", "5");
        click(&mut app, away, json!([]));
        let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
        loop {
            let n = wait_for_node(&mut app, &w, "Position");
            if n["value"] == old {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "a click on {away} leaves Position showing {old}: {n}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(tick(&mut app), 0, "a click on {away} commits nothing");
    }
    select_field(&mut app, "Position");
    app.call("ui.key", json!({"window": w, "text": "5\n"}));
    wait_for_tick(&mut app, &w, 4 * BAR);
    no_alert(&mut app);
    app.quit();
}

/// The ID of the first block node on track `track`: an events block
/// ("Block on <track>, …") or a reference ("Reference to … on <track>, …").
fn block_on(nodes: &Value, track: &str) -> Option<Value> {
    let on = format!(" on {track}, ");
    nodes.as_array().into_iter().flatten().find_map(|n| {
        let l = n["label"].as_str().unwrap_or("");
        if (l.starts_with("Block on ") || l.starts_with("Reference to ")) && l.contains(&on) {
            Some(n["id"].clone())
        } else {
            block_on(&n["children"], track)
        }
    })
}

/// `state.get selection` of window `w`.
fn selection(app: &mut App, w: &Value) -> Value {
    app.call("state.get", json!({"target": "selection", "window": w}))
}

/// Polls `state.get document` until its undo label is `label`.
fn wait_for_undo(app: &mut App, w: &Value, label: &str) {
    wait_for_state(app, w, "document", |d| d["undo"].as_str() == Some(label));
}

/// Blocks select by element (⇧ adds a second track), a click on empty
/// canvas by point clears the selection, a double-click opens the block in
/// the last-used editor (the Piano Roll at first), and List (⌘2) shows the
/// same block in the List.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_blocks_select_and_open_in_the_list() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    let (chords, _) = wait_in_tree(&mut app, &w, "the Chords block", |n| {
        labelled_like(n, "Block on Chords")
    });
    // Groove's block is a reference ("Reference to … on Groove, …").
    let groove = wait_in_tree(&mut app, &w, "the Groove block", |n| block_on(n, "Groove"));
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].clone();
    let chords_lane = doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "Chords")
        .map(|l| l["id"].clone())
        .expect("a Chords track");

    app.call("ui.click", json!({"window": w, "element": chords}));
    let s = selection(&mut app, &w);
    assert!(!s["events"].as_array().unwrap().is_empty(), "{s}");
    assert_eq!(
        s["lanes"],
        json!([{"seq": seq, "lane": chords_lane}]),
        "{s}"
    );
    assert!(s["time_range"].is_object(), "{s}");

    app.call(
        "ui.click",
        json!({"window": w, "element": groove, "modifiers": ["shift"]}),
    );
    let s = selection(&mut app, &w);
    assert_eq!(s["lanes"].as_array().unwrap().len(), 2, "{s}");

    // Below the two tracks the canvas is empty.
    let canvas = wait_for_node(&mut app, &w, "Arrangement");
    let f = &canvas["frame"];
    let x = f["x"].as_f64().unwrap() + 30.;
    let y = f["y"].as_f64().unwrap() + f["height"].as_f64().unwrap() - 10.;
    app.call("ui.click", json!({"window": w, "point": {"x": x, "y": y}}));
    let s = selection(&mut app, &w);
    assert!(s["lanes"].as_array().unwrap().is_empty(), "{s}");
    assert!(s["events"].as_array().unwrap().is_empty(), "{s}");
    assert!(s["time_range"].is_null(), "{s}");

    app.call(
        "ui.click",
        json!({"window": w, "element": chords, "count": 2}),
    );
    app.wait(
        &w,
        json!({"condition": "detail_kind", "kind": "piano_roll"}),
    );
    let e = app.call("state.get", json!({"target": "editor", "window": w}));
    let d = &e["detail"];
    assert_eq!(d["kind"], "piano_roll", "{e}");
    assert_eq!(d["seq"], seq, "{e}");
    assert_eq!(d["lane"], chords_lane, "{e}");
    assert!(
        d["start"].is_i64() && d["end"].is_i64(),
        "a block target: {e}"
    );
    let block = d.clone();

    app.call("command.run", json!({"id": "editors.list", "window": w}));
    app.wait(&w, json!({"condition": "detail_kind", "kind": "list"}));
    let e = app.call("state.get", json!({"target": "editor", "window": w}));
    let d = &e["detail"];
    assert_eq!(d["kind"], "list", "{e}");
    for k in ["seq", "lane", "start", "end"] {
        assert_eq!(d[k], block[k], "{k}: {e}");
    }
    app.quit();
}

/// A node's tree path: its id before the `:` (`ui.tree` is flat; a
/// child's path extends its container's, "4.3.2" → "4.3.2.0").
fn path_of(n: &Value) -> Option<&str> {
    n["id"].as_str()?.split(':').next()
}

/// Whether node `n` lies inside the container at `path`.
fn under(n: &Value, path: &str) -> bool {
    path_of(n).is_some_and(|p| p.starts_with(&format!("{path}.")))
}

/// The nodes inside the first node labelled exactly `label` (a group),
/// as a flat list.
fn children_of(nodes: &Value, label: &str) -> Option<Value> {
    let group = find_node(nodes, label)?;
    let path = path_of(&group)?.to_owned();
    let inside = nodes.as_array()?.iter().filter(|n| under(n, &path));
    Some(Value::Array(inside.cloned().collect()))
}

/// The first node of role `group` labelled exactly `label` (the bottom
/// pane, not the transport bar's toggle of the same name).
fn find_group(nodes: &Value, label: &str) -> Option<Value> {
    nodes
        .as_array()?
        .iter()
        .find(|n| n["role"] == "group" && n["label"].as_str() == Some(label))
        .cloned()
}

/// The first node labelled exactly `label` outside the group labelled
/// `outside` (the arrangement's tool bar, not the roll's).
fn find_node_outside(nodes: &Value, label: &str, outside: &str) -> Option<Value> {
    let excluded = find_group(nodes, outside).and_then(|g| path_of(&g).map(str::to_owned));
    nodes.as_array()?.iter().find_map(|n| {
        let inside = excluded.as_deref().is_some_and(|p| under(n, p));
        (!inside && n["label"].as_str() == Some(label)).then(|| n.clone())
    })
}

/// A note of the Piano Roll's canvas (List rows are also "Note …", so
/// only the "Piano Roll" group is searched): its id and label.
fn roll_note(nodes: &Value) -> Option<(Value, String)> {
    labelled_like(&children_of(nodes, "Piano Roll")?, "Note ")
}

/// The Piano Roll (⌘1) on the demo: its notes are in the tree, a note
/// clicked is the selection (with its track), Delete deletes it, Undo
/// brings it back.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_the_piano_roll_draws_selects_and_deletes() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    app.call(
        "command.run",
        json!({"id": "editors.piano_roll", "window": w}),
    );
    app.wait(
        &w,
        json!({"condition": "detail_kind", "kind": "piano_roll"}),
    );
    wait_for_node(&mut app, &w, "Piano Roll");
    let (note, label) = wait_in_tree(&mut app, &w, "a roll note", roll_note);
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].clone();
    let lanes: Vec<Value> = doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].clone())
        .collect();

    app.call("ui.click", json!({"window": w, "element": note}));
    let s = selection(&mut app, &w);
    assert_eq!(s["events"].as_array().unwrap().len(), 1, "{label}: {s}");
    let sel_lanes = s["lanes"].as_array().unwrap();
    assert_eq!(sel_lanes.len(), 1, "{label}: {s}");
    assert_eq!(sel_lanes[0]["seq"], seq, "{s}");
    assert!(lanes.contains(&sel_lanes[0]["lane"]), "{s} in {doc}");
    assert!(s["time_range"].is_null(), "{s}");
    let selected = wait_in_tree(&mut app, &w, "the clicked note", |n| {
        children_of(n, "Piano Roll").and_then(|c| find_node(&c, &label))
    });
    assert_eq!(selected["selected"], true, "{selected}");

    // The canvas has the keyboard: Delete deletes the selected note.
    app.call("ui.key", json!({"window": w, "key": "delete"}));
    wait_for_undo(&mut app, &w, "Delete Events");

    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_state(&mut app, &w, "document", |d| {
        d["undo"].as_str() != Some("Delete Events")
    });
    wait_in_tree(&mut app, &w, "the note back", |n| {
        children_of(n, "Piano Roll").and_then(|c| find_node(&c, &label))
    });
    app.quit();
}

/// The Pencil chosen in the arrangement's tool bar, a click on the Chords
/// row opens the track's Piano Roll with its notes; the Pencil stays on.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_the_pencil_opens_the_roll() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    let pencil = wait_in_tree(&mut app, &w, "the arrangement's Pencil", |n| {
        find_node_outside(n, "Pencil", "Bottom Pane")
    });
    app.call("ui.click", json!({"window": w, "element": pencil["id"]}));
    wait_for_editor(&mut app, &w, "the pencil", |e| e["tool"] == "pencil");

    // Just inside the Chords block's start: its first bar.
    let (_, chords) = wait_in_tree(&mut app, &w, "the Chords block", |n| {
        labelled_like(n, "Block on Chords")
    });
    let block = wait_for_node(&mut app, &w, &chords);
    let f = &block["frame"];
    let x = f["x"].as_f64().unwrap() + 4.;
    let y = f["y"].as_f64().unwrap() + f["height"].as_f64().unwrap() / 2.;
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].clone();
    let chords_lane = doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "Chords")
        .map(|l| l["id"].clone())
        .expect("a Chords track");

    app.call("ui.click", json!({"window": w, "point": {"x": x, "y": y}}));
    app.wait(
        &w,
        json!({"condition": "detail_kind", "kind": "piano_roll"}),
    );
    let e = app.call("state.get", json!({"target": "editor", "window": w}));
    let d = &e["detail"];
    assert_eq!(d["kind"], "piano_roll", "{e}");
    assert_eq!(d["seq"], seq, "{e}");
    assert_eq!(d["lane"], chords_lane, "{e}");
    assert!(d["start"].is_null(), "a track target: {e}");
    assert_eq!(e["tool"], "pencil", "{e}");
    let s = selection(&mut app, &w);
    assert_eq!(
        s["lanes"],
        json!([{"seq": seq, "lane": chords_lane}]),
        "{s}"
    );
    wait_in_tree(&mut app, &w, "the roll's notes", roll_note);
    app.quit();
}

/// The List over the channel: a note row's Position typed (Return edits
/// it, Return commits), Insert Event, Delete from the table, Undo, and
/// Close Detail Editor.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_the_list_edits_inserts_and_deletes() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    app.call("command.run", json!({"id": "editors.list", "window": w}));
    app.wait(&w, json!({"condition": "detail_kind", "kind": "list"}));
    wait_for_node(&mut app, &w, "Event List");
    let (row, label) = wait_in_tree(&mut app, &w, "a note row", |n| labelled_like(n, "Note "));
    app.call("ui.click", json!({"window": w, "element": row}));
    let s = selection(&mut app, &w);
    assert_eq!(s["events"].as_array().unwrap().len(), 1, "{label}: {s}");

    // Return edits the row's Position; Return commits it.
    app.call("ui.key", json!({"window": w, "key": "return"}));
    app.call("ui.key", json!({"window": w, "text": "5"}));
    app.call("ui.key", json!({"window": w, "key": "return"}));
    wait_for_undo(&mut app, &w, "Move Events");

    app.call(
        "command.run",
        json!({"id": "editors.insert_event", "window": w}),
    );
    wait_for_undo(&mut app, &w, "Insert Events");

    // The table keeps the keyboard: Delete deletes the inserted row.
    app.call("ui.key", json!({"window": w, "key": "delete"}));
    wait_for_undo(&mut app, &w, "Delete Events");

    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_undo(&mut app, &w, "Insert Events");

    app.call(
        "command.run",
        json!({"id": "editors.close_detail", "window": w}),
    );
    app.wait(&w, json!({"condition": "detail_kind", "kind": "none"}));
    app.quit();
}

/// Ported from the G-snap step of `keys_drive_step_entry_locators_and_bindings`:
/// the grid starts on quarters, and G turns it off.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_g_toggles_snap() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    let e = wait_for_editor(&mut app, &w, "snap on", |e| e["snap"]["enabled"] == true);
    assert_eq!(e["snap"]["division"], "quarter");
    app.call("ui.key", json!({"window": w, "key": "g"}));
    wait_for_editor(&mut app, &w, "snap off", |e| e["snap"]["enabled"] == false);
    app.quit();
}

// Slice 6a: the mixer over the channel. Slice 6b's groups, VCAs, sends,
// MIDI strips and the left pane's strip follow these.

/// The pane `pane` of main view `main` in a `state.get layout` answer.
fn layout_pane(layout: &Value, main: &str, pane: &str) -> Value {
    layout["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["main"] == main)
        .and_then(|v| {
            v["panes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["pane"] == pane)
        })
        .unwrap_or_else(|| panic!("no {main} {pane} in {layout}"))
        .clone()
}

/// The windows of kind `mixer`.
fn mixer_windows(app: &mut App) -> Vec<Value> {
    window_numbers(app)
        .into_iter()
        .filter(|(_, k)| k == "mixer")
        .map(|(n, _)| n)
        .collect()
}

/// Polls until a Mixer window is listed; its number.
fn wait_for_mixer_window(app: &mut App) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        if let Some(n) = mixer_windows(app).into_iter().next() {
            return n;
        }
        assert!(Instant::now() < deadline, "no mixer window opened");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Ported from `touch_writes_a_fader_over_the_channel`: the synth's fader
/// in Touch, touched, moved and released over the channel while playing,
/// lands at Stop as one "Write Automation" step in a lane "Built-in Synth
/// Fader"; hostile gestures are refused before anything changes.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_touch_writes_a_fader_over_the_channel() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["suspended"], false, "{state}");
    let synth = state["modes"][0]["strip"]
        .as_u64()
        .expect("the synth strip first");
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        )
    };
    run(
        &mut app,
        "automation.set_mode",
        json!({"strips": synth.to_string(), "mode": "touch"}),
    );
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["modes"][0]["mode"], "touch", "{state}");
    for (params, code) in [
        (
            json!({"target": "fader:999999", "phase": "touch"}),
            core_control::ErrorCode::Failed,
        ),
        (
            json!({"target": format!("fader:{synth}"), "phase": "value", "value": "nan"}),
            core_control::ErrorCode::InvalidParams,
        ),
        (
            json!({"target": "fader", "phase": "touch"}),
            core_control::ErrorCode::InvalidParams,
        ),
    ] {
        let e = app.call_err(
            "command.run",
            json!({"id": "automation.gesture", "params": params, "window": w}),
        );
        assert_eq!(e.code, code, "{e}");
    }
    run(&mut app, "transport.play_stop", json!({}));
    app.wait(&w, json!({"condition": "playing"}));
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["writing"], true, "{state}");
    let target = format!("fader:{synth}");
    run(
        &mut app,
        "automation.gesture",
        json!({"target": target, "phase": "touch", "value": "0.5"}),
    );
    for v in ["0.55", "0.6", "0.65", "0.7"] {
        std::thread::sleep(Duration::from_millis(30));
        run(
            &mut app,
            "automation.gesture",
            json!({"target": target, "phase": "value", "value": v}),
        );
    }
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["touched"][0], target, "{state}");
    assert_eq!(state["writing"], true, "{state}");
    run(
        &mut app,
        "automation.gesture",
        json!({"target": target, "phase": "release"}),
    );
    std::thread::sleep(Duration::from_millis(600));
    run(&mut app, "transport.play_stop", json!({}));
    app.wait(&w, json!({"condition": "stopped"}));
    let doc = wait_for_state(&mut app, &w, "document", |d| {
        d["undo"] == "Write Automation"
    });
    assert!(
        doc["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["name"] == "Built-in Synth Fader"),
        "{doc}"
    );
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["touched"], json!([]), "{state}");
    app.quit();
}

/// Ported from `window_views_and_panes_over_the_channel`: the defaults,
/// hostile `ui.pane` calls that change nothing and open no dialog, the
/// main-view keys, a pane set for the other view, the mixer in the bottom
/// pane and in its own window (with its own transport bar), and the
/// layout saved per document. Dropped: the bar's Record and Record
/// Options (the recording slice draws them); the Swift wait on the Mix
/// view's "Instruments" menu (6b) waits on its "Add Bus" instead.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_window_views_and_panes_over_the_channel() {
    use core_control::ErrorCode;
    let home = temp_home();
    let (mut app, window) = start_demo(home.path());
    // 1. The defaults.
    let layout =
        |app: &mut App| app.call("state.get", json!({"target": "layout", "window": window}));
    let l0 = layout(&mut app);
    assert_eq!(l0["main"], "arrange", "{l0}");
    let left = layout_pane(&l0, "arrange", "left");
    assert_eq!(
        (left["shown"].clone(), left["view"].clone()),
        (json!(true), json!("strip")),
        "{l0}"
    );
    assert_eq!(layout_pane(&l0, "arrange", "bottom")["shown"], true, "{l0}");
    assert_eq!(layout_pane(&l0, "arrange", "right")["shown"], false, "{l0}");
    for side in ["left", "bottom", "right"] {
        assert_eq!(
            layout_pane(&l0, "mix", side)["shown"],
            false,
            "mix {side}: {l0}"
        );
    }
    app.call(
        "command.run",
        json!({"id": "ui.set_transport_bar", "params": {"items": "cluster,loop,record_options,panes"},
            "window": window}),
    );
    for label in ["Loop", "Left Pane", "Bottom Pane", "Right Pane"] {
        wait_for_node(&mut app, &window, label);
    }

    // 2. Hostile calls change nothing and open no dialog.
    let w0 = window_numbers(&mut app);
    for (params, mentions) in [
        (json!({}), None),
        (json!({"pane": "left", "view": "mixer"}), None),
        (
            json!({"pane": "bottom", "view": "mixer", "main": "mix"}),
            None,
        ),
        (json!({"pane": "left", "size": "wide"}), None),
        (json!({"pane": "left"}), None),
        (
            json!({"pane": "right", "view": "piano_roll"}),
            Some("piano_roll"),
        ),
    ] {
        let e = app.call_err(
            "command.run",
            json!({"id": "ui.pane", "params": params, "window": window}),
        );
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}: {e}");
        if let Some(word) = mentions {
            assert!(e.to_string().contains(word), "{params}: {e}");
        }
    }
    let e = app.call_err("state.get", json!({"target": "panes", "window": window}));
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    assert_eq!(layout(&mut app), l0, "no layout change");
    assert_eq!(window_numbers(&mut app), w0, "no window opened");
    let w1 = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(w1["alerts"].as_array().unwrap().len(), 0, "{w1}");
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    assert!(doc["alert"].is_null(), "no dialog: {doc}");

    // 3. ⌃2 and ⌃1 on macOS. Off macOS ⌃2 is ⌘2's (the List, as the
    // keyboard has it: `menus::owner`), so the views are switched with
    // their commands there.
    let mac = cfg!(target_os = "macos");
    if mac {
        app.call(
            "ui.key",
            json!({"window": window, "key": "2", "modifiers": ["control"]}),
        );
    } else {
        app.call(
            "ui.key",
            json!({"window": window, "key": "2", "modifiers": ["control"]}),
        );
        app.wait(&window, json!({"condition": "detail_kind", "kind": "list"}));
        assert_eq!(layout(&mut app)["main"], "arrange", "⌃2 opened the List");
        app.call(
            "command.run",
            json!({"id": "ui.view_mix", "window": window}),
        );
    }
    wait_for_state(&mut app, &window, "layout", |l| l["main"] == "mix");
    // Only the mixer has Add Bus: once it is in the tree the view has
    // switched, so the absent Arrangement means something.
    wait_for_node(&mut app, &window, "Add Bus");
    let tree = app.call("ui.tree", json!({"window": window}));
    assert!(
        find_node(&tree["nodes"], "Arrangement").is_none(),
        "the Mix view has no arrangement"
    );
    if mac {
        app.call(
            "ui.key",
            json!({"window": window, "key": "1", "modifiers": ["control"]}),
        );
    } else {
        app.call(
            "command.run",
            json!({"id": "ui.view_arrange", "window": window}),
        );
    }
    wait_for_state(&mut app, &window, "layout", |l| l["main"] == "arrange");
    wait_for_node(&mut app, &window, "Arrangement");
    app.call(
        "command.run",
        json!({"id": "ui.toggle_right", "window": window}),
    );
    let l = wait_for_state(&mut app, &window, "layout", |l| {
        layout_pane(l, "arrange", "right")["shown"] == true
    });
    assert_eq!(l["main"], "arrange", "{l}");

    // 4. A pane of the other view; the main view stays.
    let r = app.call(
        "command.run",
        json!({"id": "ui.pane", "params": {"pane": "right", "view": "markers", "size": 300, "shown": true, "main": "mix"}, "window": window}),
    );
    assert_eq!(r["status"], "right markers 300 shown", "{r}");
    let l = layout(&mut app);
    assert_eq!(l["main"], "arrange", "{l}");
    let right = layout_pane(&l, "mix", "right");
    assert_eq!(
        (
            right["shown"].clone(),
            right["view"].clone(),
            right["size"].clone()
        ),
        (json!(true), json!("markers"), json!(300.0)),
        "{l}"
    );

    // 5. The mixer in arrange's bottom pane.
    app.call(
        "command.run",
        json!({"id": "ui.pane", "params": {"pane": "bottom", "view": "mixer"}, "window": window}),
    );
    let l = layout(&mut app);
    assert_eq!(layout_pane(&l, "arrange", "bottom")["view"], "mixer", "{l}");
    wait_for_node(&mut app, &window, "Add Bus");

    // 6. The Mixer window has its own transport bar.
    app.call(
        "command.run",
        json!({"id": "window.mixer", "window": window}),
    );
    let mixer = wait_for_mixer_window(&mut app);
    let id = wait_in_tree(&mut app, &mixer, "the Mixer window's Play", play_button);
    app.call("ui.click", json!({"window": mixer, "element": id}));
    app.wait(&window, json!({"condition": "playing"}));
    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": window}),
    );
    app.wait(&window, json!({"condition": "stopped"}));

    // 7. Saved per document; closing the document closes its Mixer
    // window.
    let path = home.path().join("views.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": path}, "window": window}),
    );
    app.call("command.run", json!({"id": "file.close", "window": window}));
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    while !mixer_windows(&mut app).is_empty() {
        assert!(Instant::now() < deadline, "the Mixer window stayed open");
        std::thread::sleep(Duration::from_millis(200));
    }
    let w2 = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w2, json!({"condition": "documents_at_least", "count": 1}));
    let l = app.call("state.get", json!({"target": "layout", "window": w2}));
    let right = layout_pane(&l, "mix", "right");
    assert_eq!(
        (
            right["shown"].clone(),
            right["view"].clone(),
            right["size"].clone()
        ),
        (json!(true), json!("markers"), json!(300.0)),
        "{l}"
    );
    assert_eq!(layout_pane(&l, "arrange", "bottom")["view"], "mixer", "{l}");
    app.quit();
}

/// Ported from the strip and window parts of
/// `groups_vcas_and_transport_bar_over_the_channel`: a new audio track
/// gives the mixer a third strip, and ⌥⌘M opens one Mixer window (pressed
/// again, it is brought to the front) whose tree has a fader for every
/// strip. Dropped: groups and VCAs (6b), the bar's `command:` button (the
/// GPUI bar does not draw command items yet) and the click (the
/// recording slice).
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_new_track_strips_and_the_mixer_window_over_the_channel() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    app.call("command.run", json!({"id": "track.new_audio", "window": w}));
    let m = wait_for_state(&mut app, &w, "mixer", |m| {
        m["strips"].as_array().unwrap().len() >= 3
    });
    let names: Vec<String> = m["strips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect();
    assert!(mixer_windows(&mut app).is_empty());
    for _ in 0..2 {
        app.call(
            "ui.key",
            json!({"window": w, "key": "m", "modifiers": ["option", "command"]}),
        );
    }
    let mixer = wait_for_mixer_window(&mut app);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        mixer_windows(&mut app),
        std::slice::from_ref(&mixer),
        "one Mixer window"
    );
    for name in &names {
        wait_for_node(&mut app, &mixer, &format!("Fader of {name}"));
    }
    let ws = app.call("state.get", json!({"target": "windows"}));
    let row = ws["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["number"] == mixer)
        .unwrap()
        .clone();
    assert!(
        row["title"]
            .as_str()
            .unwrap()
            .starts_with("Mixer \u{2014} "),
        "{row}"
    );
    app.quit();
}

// Slice 6b: mixer routing, groups, VCAs, MIDI strips and the left pane's
// strip over the channel, on the real core (its undo names included).

/// The strip of `state.get mixer` whose ID is `id`.
/// A level as a strip's fader reads it ("−6.0 dB", "0.0 dB", "+2.0 dB";
/// within the throw).
fn level_text(db: f64) -> String {
    let v = (db * 10.0).round() / 10.0;
    if v == 0.0 {
        return "0.0 dB".into();
    }
    let sign = if v < 0.0 { '\u{2212}' } else { '+' };
    format!("{sign}{:.1} dB", v.abs())
}

fn strip_by_id(m: &Value, id: u64) -> Value {
    m["strips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"].as_u64() == Some(id))
        .unwrap_or_else(|| panic!("no strip {id} in {m}"))
        .clone()
}

/// The strip of `state.get mixer` named `name`.
fn strip_named(m: &Value, name: &str) -> Value {
    m["strips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no strip {name:?} in {m}"))
        .clone()
}

/// Strip `id`'s fader in dB.
fn gain_of(m: &Value, id: u64) -> f64 {
    strip_by_id(m, id)["gain_db"].as_f64().unwrap()
}

/// `state.get mixer` of window `w`.
fn mixer_state(app: &mut App, w: &Value) -> Value {
    app.call("state.get", json!({"target": "mixer", "window": w}))
}

/// `command.run id params` in window `w`; the answer (`undo`, `status`).
fn run_in(app: &mut App, w: &Value, id: &str, params: Value) -> Value {
    app.call(
        "command.run",
        json!({"id": id, "params": params, "window": w}),
    )
}

/// A list of IDs, sorted (members may come in any order).
fn sorted_ids(v: &Value) -> Vec<u64> {
    let mut ids: Vec<u64> = v
        .as_array()
        .unwrap_or_else(|| panic!("not a list: {v}"))
        .iter()
        .filter_map(Value::as_u64)
        .collect();
    ids.sort_unstable();
    ids
}

fn close_to(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// Ported from the groups and VCA parts of
/// `groups_vcas_and_transport_bar_over_the_channel`, with the undo names
/// the real core gives each step: a group's members move by one dB as one
/// step ("Move Group ‹name›", undone and redone whole), `alone` moves one
/// member only, a renamed group names its moves; a VCA's offset leaves
/// the faders alone and its fader shows the level in the Mix view.
/// Not ported: the transport bar's `command:` item (the GPUI bar does not
/// draw command items yet) and the click settings (`transport.click`,
/// `set_click_level`, `set_click_mode`: the recording slice).
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_groups_and_vcas_over_the_channel() {
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    // The demo has one strip besides Master; a second track gives two.
    app.call("command.run", json!({"id": "track.new_audio", "window": w}));
    let m = wait_for_state(&mut app, &w, "mixer", |m| {
        m["strips"].as_array().unwrap().len() >= 3
    });
    let two: Vec<(u64, f64)> = m["strips"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["kind_colour"] != "master")
        .take(2)
        .map(|s| (s["id"].as_u64().unwrap(), s["gain_db"].as_f64().unwrap()))
        .collect();
    assert_eq!(two.len(), 2, "{m}");
    let (a, b) = (two[0].0, two[1].0);
    let ids = format!("{a},{b}");

    // A group of both strips: "New Group" (not the menu's "Group").
    let r = run_in(&mut app, &w, "mixer.new_group", json!({"strips": ids}));
    assert_eq!(r["undo"], "New Group", "{r}");
    let m = mixer_state(&mut app, &w);
    let groups = m["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{m}");
    let group = groups[0]["id"].clone();
    assert_eq!(groups[0]["name"], "Group 1", "{m}");
    assert_eq!(
        sorted_ids(&groups[0]["members"]),
        sorted_ids(&json!([a, b])),
        "{m}"
    );
    for id in [a, b] {
        assert_eq!(strip_by_id(&m, id)["group"], group, "{m}");
    }

    // One move moves both by the same dB, one step.
    let r = run_in(
        &mut app,
        &w,
        "mixer.set_strip",
        json!({"strip": a, "gain_db": "-3"}),
    );
    assert_eq!(r["undo"], "Move Group Group 1", "{r}");
    let m = mixer_state(&mut app, &w);
    let d0 = gain_of(&m, a) - two[0].1;
    let d1 = gain_of(&m, b) - two[1].1;
    assert!(close_to(gain_of(&m, a), -3.0), "{m}");
    assert!(close_to(d0, d1), "both moved by {d0} / {d1}: {m}");
    wait_for_undo(&mut app, &w, "Move Group Group 1");
    let moved = (gain_of(&m, a), gain_of(&m, b));

    // Undo takes both back at once; Redo moves both again.
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_undo(&mut app, &w, "New Group");
    let m = mixer_state(&mut app, &w);
    assert!(close_to(gain_of(&m, a), two[0].1), "{m}");
    assert!(close_to(gain_of(&m, b), two[1].1), "{m}");
    app.call("command.run", json!({"id": "edit.redo", "window": w}));
    wait_for_undo(&mut app, &w, "Move Group Group 1");
    let m = mixer_state(&mut app, &w);
    assert!(close_to(gain_of(&m, a), moved.0), "{m}");
    assert!(close_to(gain_of(&m, b), moved.1), "{m}");

    // `alone`: the group does not follow; the step is the command's.
    let r = run_in(
        &mut app,
        &w,
        "mixer.set_strip",
        json!({"strip": b, "gain_db": "-10", "alone": true}),
    );
    assert_eq!(r["undo"], "Set Strip", "{r}");
    let m = mixer_state(&mut app, &w);
    assert!(close_to(gain_of(&m, b), -10.0), "{m}");
    assert!(close_to(gain_of(&m, a), moved.0), "a stayed: {m}");

    // A renamed group names its moves.
    let r = run_in(
        &mut app,
        &w,
        "mixer.rename_group",
        json!({"group": group, "name": "Keys"}),
    );
    assert_eq!(r["undo"], "Rename Group", "{r}");
    let before = mixer_state(&mut app, &w);
    let r = run_in(
        &mut app,
        &w,
        "mixer.set_strip",
        json!({"strip": a, "gain_db": "-6"}),
    );
    assert_eq!(r["undo"], "Move Group Keys", "{r}");
    let m = mixer_state(&mut app, &w);
    let d0 = gain_of(&m, a) - gain_of(&before, a);
    let d1 = gain_of(&m, b) - gain_of(&before, b);
    assert!(close_to(d0, -3.0) && close_to(d0, d1), "{d0} / {d1}: {m}");

    // A VCA: the offset shows, the faders stay.
    let r = run_in(&mut app, &w, "mixer.new_vca", json!({"strips": ids}));
    assert_eq!(r["undo"], "New VCA", "{r}");
    let m = mixer_state(&mut app, &w);
    let vcas = m["vcas"].as_array().unwrap();
    assert_eq!(vcas.len(), 1, "{m}");
    let vca = vcas[0]["id"].clone();
    assert_eq!(
        sorted_ids(&vcas[0]["members"]),
        sorted_ids(&json!([a, b])),
        "{m}"
    );
    let r = run_in(
        &mut app,
        &w,
        "mixer.set_vca",
        json!({"vca": vca, "gain_db": "-6"}),
    );
    assert_eq!(r["undo"], "Set VCA", "{r}");
    let after = mixer_state(&mut app, &w);
    assert!(
        close_to(after["vcas"][0]["gain_db"].as_f64().unwrap(), -6.0),
        "{after}"
    );
    for id in [a, b] {
        let s = strip_by_id(&after, id);
        assert!(close_to(s["vca_offset_db"].as_f64().unwrap(), -6.0), "{s}");
        assert_eq!(s["vcas"], json!([vca]), "{s}");
        assert!(
            (gain_of(&after, id) - gain_of(&m, id)).abs() < 1e-6,
            "{after}"
        );
    }
    let r = run_in(
        &mut app,
        &w,
        "mixer.rename_vca",
        json!({"vca": vca, "name": "Drums"}),
    );
    assert_eq!(r["undo"], "Rename VCA", "{r}");

    // The Mix view draws the VCA's fader at its level.
    app.call("command.run", json!({"id": "ui.view_mix", "window": w}));
    wait_for_value(&mut app, &w, "Fader of VCA Drums", "\u{2212}6.0 dB");
    // A member shows its offset under its level box; its fader is still
    // its own level, not the heard one.
    wait_for_node(&mut app, &w, "VCA offset VCA \u{2212}6.0");
    let member = strip_by_id(&after, a);
    let name = member["name"].as_str().unwrap();
    wait_for_value(
        &mut app,
        &w,
        &format!("Fader of {name}"),
        &level_text(gain_of(&after, a)),
    );
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_undo(&mut app, &w, "Set VCA");
    wait_for_node(&mut app, &w, "Fader of VCA 1");
    app.quit();
}

/// Ported from the New Bus part of `default_eq_and_new_bus_over_the_channel`,
/// then the send edits on the real core: New Bus from a strip is one step
/// that takes the bus and its send together (undo and redo), a bus alone
/// is "Add Bus", the master is refused; `mixer.set_send {index: 1}` edits
/// the first send (1-based on the channel, 0-based in the FFI), and Add,
/// Set and Remove Send are each one named step. Not ported: the default
/// EQ (`plugins.default_eq` and the EQ install need the plug-in
/// catalogue, a later Plug-in Manager slice).
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_new_bus_and_sends_over_the_channel() {
    use core_control::ErrorCode;
    let home = temp_home();
    let mut app = App::start(home.path(), false);
    let w = app.command("file.new", json!({}))["window"].clone();
    wait_for_node(&mut app, &w, "Track Headers");
    app.call("command.run", json!({"id": "track.new_audio", "window": w}));
    let m = wait_for_state(&mut app, &w, "mixer", |m| {
        m["strips"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["kind_colour"] == "channel")
    });
    let strips = m["strips"].as_array().unwrap();
    let audio = strips
        .iter()
        .find(|s| s["kind_colour"] == "channel")
        .unwrap()
        .clone();
    let master = strips
        .iter()
        .find(|s| s["kind_colour"] == "master")
        .unwrap_or_else(|| panic!("no master in {m}"))
        .clone();
    let buses = |m: &Value| {
        m["strips"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["kind_colour"] == "bus")
            .count()
    };
    let buses_before = buses(&m);
    let audio_id = audio["id"].as_u64().unwrap();
    let audio_name = audio["name"].as_str().unwrap().to_string();
    let sends = |m: &Value| strip_by_id(m, audio_id)["sends"].clone();

    // New Bus from the strip: "Bus 1", one step, a send to it.
    let r = run_in(&mut app, &w, "mixer.new_bus", json!({"strip": audio_id}));
    assert_eq!(r["status"], "Bus 1", "{r}");
    assert_eq!(r["undo"], "New Bus", "{r}");
    let m = mixer_state(&mut app, &w);
    let bus1 = strip_named(&m, "Bus 1")["id"].clone();
    assert_eq!(sends(&m).as_array().unwrap().len(), 1, "{m}");
    assert_eq!(sends(&m)[0]["target"], bus1, "{m}");
    assert_eq!(sends(&m)[0]["pre_fader"], false, "{m}");

    // One Undo removes the bus and the send; Redo brings both back.
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    let m = mixer_state(&mut app, &w);
    assert_eq!(buses(&m), buses_before, "the bus is gone: {m}");
    assert_eq!(sends(&m), json!([]), "the send is gone: {m}");
    app.call("command.run", json!({"id": "edit.redo", "window": w}));
    wait_for_undo(&mut app, &w, "New Bus");
    let m = mixer_state(&mut app, &w);
    let bus1 = strip_named(&m, "Bus 1")["id"].clone();
    assert_eq!(sends(&m)[0]["target"], bus1, "the send is back: {m}");

    // Without a strip: a bus with no send, "Bus 2". The master is refused.
    let r = run_in(&mut app, &w, "mixer.new_bus", json!({}));
    assert_eq!(r["status"], "Bus 2", "{r}");
    assert_eq!(r["undo"], "Add Bus", "{r}");
    let e = app.call_err(
        "command.run",
        json!({"id": "mixer.new_bus", "params": {"strip": master["id"]}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}");
    let m = mixer_state(&mut app, &w);
    let bus2 = strip_named(&m, "Bus 2")["id"].clone();
    assert_eq!(sends(&m).as_array().unwrap().len(), 1, "{m}");

    // A second send, to Bus 2.
    let r = run_in(
        &mut app,
        &w,
        "mixer.add_send",
        json!({"strip": audio_id, "bus": bus2, "level_db": "-3"}),
    );
    assert_eq!(r["undo"], "Add Send", "{r}");
    let m = mixer_state(&mut app, &w);
    let s = sends(&m);
    assert_eq!(s.as_array().unwrap().len(), 2, "{m}");
    assert_eq!((&s[0]["target"], &s[1]["target"]), (&bus1, &bus2), "{m}");
    assert!(close_to(s[1]["level_db"].as_f64().unwrap(), -3.0), "{m}");

    // `index: 1` is the first send (to Bus 1); the second stays.
    let r = run_in(
        &mut app,
        &w,
        "mixer.set_send",
        json!({"strip": audio_id, "index": 1, "level_db": "-6"}),
    );
    assert_eq!(r["undo"], "Set Send", "{r}");
    let m = mixer_state(&mut app, &w);
    let s = sends(&m);
    assert_eq!(s[0]["target"], bus1, "{m}");
    assert!(close_to(s[0]["level_db"].as_f64().unwrap(), -6.0), "{m}");
    assert!(close_to(s[1]["level_db"].as_f64().unwrap(), -3.0), "{m}");
    run_in(
        &mut app,
        &w,
        "mixer.set_send",
        json!({"strip": audio_id, "index": 2, "pre_fader": true}),
    );
    let m = mixer_state(&mut app, &w);
    let s = sends(&m);
    assert_eq!(
        (&s[0]["pre_fader"], &s[1]["pre_fader"]),
        (&json!(false), &json!(true)),
        "{m}"
    );

    // Index 0 is the core's InvalidParams (sends count from 1 through
    // `run_mixer`); a send the strip lacks is refused too. Neither changes
    // anything.
    let e = app.call_err(
        "command.run",
        json!({"id": "mixer.set_send", "params": {"strip": audio_id, "index": 0, "level_db": "-1"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}");
    app.call_err(
        "command.run",
        json!({"id": "mixer.set_send", "params": {"strip": audio_id, "index": 3, "level_db": "-1"}, "window": w}),
    );
    assert_eq!(sends(&mixer_state(&mut app, &w)), s, "nothing changed");

    // The Mix view shows both sends' boxes.
    app.call("command.run", json!({"id": "ui.view_mix", "window": w}));
    wait_for_node(&mut app, &w, &format!("Send 1 of {audio_name}"));
    wait_for_node(&mut app, &w, &format!("Send 2 of {audio_name}"));

    // Remove Send 1: Bus 2's send is left; Undo puts Bus 1's back first.
    let r = run_in(
        &mut app,
        &w,
        "mixer.remove_send",
        json!({"strip": audio_id, "index": 1}),
    );
    assert_eq!(r["undo"], "Remove Send", "{r}");
    let m = mixer_state(&mut app, &w);
    let left = sends(&m);
    assert_eq!(left.as_array().unwrap().len(), 1, "{m}");
    assert_eq!(left[0]["target"], bus2, "{m}");
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    let m = mixer_state(&mut app, &w);
    assert_eq!(sends(&m), s, "the send is back where it was: {m}");
    app.quit();
}

/// Ported from `midi_mix_reaches_state_get`: `destination.set_midi_mix`
/// over the channel; the mix shows in `state.get document`'s
/// destinations, an absent key keeps its value and a null volume or pan
/// unsets it; one step "Set MIDI Mix"; hostile values change nothing; the
/// Mix view has the destination's MIDI strip. The Swift test adds the
/// destination with `destination.add_midi`, which the GPUI channel does
/// not run yet: the song is written with the destination already in it
/// (as `records_mpe_as_per_note_expression` writes its own). Offline:
/// nothing is sent.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_midi_mix_reaches_state_get() {
    use core_control::ErrorCode;
    use core_engine::fixtures::Builder;
    let home = temp_home();
    let mut app = App::start_with(home.path(), false, &[("VIZDAW_FAKE_SYNTH", "25")]);
    let mut b = Builder::new();
    let rack = b.dest("Rack", "Fake Synth", 3).unwrap();
    b.sequence("Song", "Keys", Some(rack)).unwrap();
    let path = home.path().join("Rack.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));

    let dest = |app: &mut App| -> Value {
        let doc = app.call("state.get", json!({"target": "document", "window": w}));
        doc["destinations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["name"] == "Rack")
            .cloned()
            .unwrap_or_else(|| panic!("no Rack destination: {doc}"))
    };
    let d = dest(&mut app);
    let id = d["id"].clone();
    assert_eq!(id.as_u64(), Some(rack.0), "{d}");
    assert_eq!(d["external"], true, "{d}");
    assert_eq!(
        (&d["midi_volume"], &d["midi_pan"], &d["midi_mute"]),
        (&Value::Null, &Value::Null, &json!(false)),
        "{d}"
    );
    let set = |app: &mut App, params: Value| -> Value {
        let mut p = params;
        p["dest"] = id.clone();
        app.call(
            "command.run",
            json!({"id": "destination.set_midi_mix", "params": p, "window": w}),
        )
    };
    let r = set(&mut app, json!({"volume": 90, "mute": true}));
    assert_eq!(r["undo"], "Set MIDI Mix", "{r}");
    let d = dest(&mut app);
    assert_eq!(
        (&d["midi_volume"], &d["midi_pan"], &d["midi_mute"]),
        (&json!(90), &Value::Null, &json!(true)),
        "{d}"
    );
    // Absent keeps volume and mute; null unsets.
    set(&mut app, json!({"pan": 20}));
    let d = dest(&mut app);
    assert_eq!(
        (&d["midi_volume"], &d["midi_pan"], &d["midi_mute"]),
        (&json!(90), &json!(20), &json!(true)),
        "{d}"
    );
    set(&mut app, json!({"volume": null, "mute": false}));
    let d = dest(&mut app);
    assert_eq!(
        (&d["midi_volume"], &d["midi_pan"], &d["midi_mute"]),
        (&Value::Null, &json!(20), &json!(false)),
        "{d}"
    );
    // Hostile values are refused and change nothing.
    for params in [
        json!({"dest": id, "volume": 128}),
        json!({"dest": id, "pan": "20"}),
        json!({"dest": id}),
        json!({"dest": 1_000_000, "volume": 1}),
    ] {
        let e = app.call_err(
            "command.run",
            json!({"id": "destination.set_midi_mix", "params": params, "window": w}),
        );
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}");
    }
    assert_eq!(dest(&mut app)["midi_pan"], 20);

    // The Mix view has Rack's MIDI strip.
    app.call("command.run", json!({"id": "ui.view_mix", "window": w}));
    wait_for_node(&mut app, &w, "Fader of Rack");
    app.quit();
}

/// The nodes inside the pane group `pane` ("Left Pane": the group, not
/// the transport bar's toggle of that name), as a flat list.
fn in_pane(nodes: &Value, pane: &str) -> Option<Value> {
    let group = find_group(nodes, pane)?;
    let path = path_of(&group)?.to_owned();
    let inside = nodes.as_array()?.iter().filter(|n| under(n, &path));
    Some(Value::Array(inside.cloned().collect()))
}

/// The labels inside pane `pane` that start with `prefix`.
fn pane_labels(nodes: &Value, pane: &str, prefix: &str) -> Vec<String> {
    in_pane(nodes, pane)
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|n| n["label"].as_str())
        .filter(|l| l.starts_with(prefix))
        .map(str::to_owned)
        .collect()
}

/// The left-pane steps (Swift `window_views_and_panes_over_the_channel`'s
/// defaults and `themes_and_the_strip_over_the_channel` 3b): the arrange
/// view's left pane shows the Channel Strip view; with nothing focused it
/// says so and draws no strip; a block clicked over the channel selects
/// it and its track, and the pane draws that track's strip (the Built-in
/// Synth's for Chords), one fader; a click on empty canvas clears the
/// selection and the note is back. The track is focused through its
/// block because a channel click on a track header row toggles its mute
/// (Swift accessibility-press parity; real mouse clicks select).
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_the_left_pane_strip_follows_the_focused_track() {
    const NOTE: &str = "Select a track to see its channel strip.";
    let home = temp_home();
    let (mut app, w) = start_demo(home.path());
    let l = app.call("state.get", json!({"target": "layout", "window": w}));
    let left = layout_pane(&l, "arrange", "left");
    assert_eq!(
        (left["shown"].clone(), left["view"].clone()),
        (json!(true), json!("strip")),
        "{l}"
    );
    let m = mixer_state(&mut app, &w);
    let synth = m["strips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind_colour"] == "instrument")
        .and_then(|s| s["name"].as_str())
        .unwrap_or_else(|| panic!("no instrument strip in {m}"))
        .to_string();

    // Nothing focused: the note, no strip.
    wait_in_tree(&mut app, &w, "the left pane's note", |n| {
        in_pane(n, "Left Pane").and_then(|p| find_node(&p, NOTE))
    });
    let tree = app.call("ui.tree", json!({"window": w}));
    assert!(
        pane_labels(&tree["nodes"], "Left Pane", "Fader of ").is_empty()
            && pane_labels(&tree["nodes"], "Left Pane", "Level of ").is_empty(),
        "no strip in the left pane: {tree}"
    );

    // The Chords block: its track is focused, the pane draws its strip.
    let (chords, _) = wait_in_tree(&mut app, &w, "the Chords block", |n| {
        labelled_like(n, "Block on Chords")
    });
    app.call("ui.click", json!({"window": w, "element": chords}));
    let s = selection(&mut app, &w);
    assert_eq!(s["lanes"].as_array().unwrap().len(), 1, "{s}");
    let fader = format!("Fader of {synth}");
    wait_in_tree(&mut app, &w, "the left pane's strip", |n| {
        in_pane(n, "Left Pane").and_then(|p| find_node(&p, &fader))
    });
    let tree = app.call("ui.tree", json!({"window": w}));
    assert_eq!(
        pane_labels(&tree["nodes"], "Left Pane", "Fader of "),
        std::slice::from_ref(&fader),
        "one strip: {tree}"
    );
    assert!(
        find_node(&in_pane(&tree["nodes"], "Left Pane").unwrap(), NOTE).is_none(),
        "no note beside the strip: {tree}"
    );

    // Empty canvas below the tracks: nothing selected, the note again.
    let canvas = wait_for_node(&mut app, &w, "Arrangement");
    let f = &canvas["frame"];
    let x = f["x"].as_f64().unwrap() + 30.;
    let y = f["y"].as_f64().unwrap() + f["height"].as_f64().unwrap() - 10.;
    app.call("ui.click", json!({"window": w, "point": {"x": x, "y": y}}));
    let s = selection(&mut app, &w);
    assert!(s["lanes"].as_array().unwrap().is_empty(), "{s}");
    wait_in_tree(&mut app, &w, "the note back", |n| {
        let p = in_pane(n, "Left Pane")?;
        let gone = pane_labels(n, "Left Pane", "Fader of ").is_empty();
        find_node(&p, NOTE).filter(|_| gone)
    });
    app.quit();
}

// ---------------------------------------------------------------------
// Files over the channel (slice 7a): ported from `e2e.rs`. Every step
// runs but the audio editor one, which waits for its slice.

/// A 48 kHz stereo float CAF of `frames` frames of a 0.25 constant (the
/// app's importer reads CAF like any other format).
fn audio_file(path: &std::path::Path, frames: usize) {
    let mut w = core_media::caf::CafWriter::create(
        path,
        48_000,
        2,
        core_model::format::BitDepth::Float32,
        core_media::caf::WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![0.25f32; 2 * frames]).unwrap();
    w.finish().unwrap();
}

/// `(rate, channels, bits, frames, 16-bit samples)` of a 16-bit PCM WAV.
fn read_wav16(path: &std::path::Path) -> (u32, u16, u16, u64, Vec<i16>) {
    let b = std::fs::read(path).unwrap();
    assert_eq!((&b[0..4], &b[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    let mut at = 12;
    let (mut rate, mut ch, mut bits, mut data) = (0, 0, 0, Vec::new());
    while at + 8 <= b.len() {
        let n = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + n];
        match &b[at..at + 4] {
            b"fmt " => {
                ch = u16::from_le_bytes(body[2..4].try_into().unwrap());
                rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
            }
            b"data" => {
                data = body
                    .chunks(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
            }
            _ => {}
        }
        at += 8 + n + (n & 1);
    }
    let frames = (data.len() / usize::from(ch.max(1))) as u64;
    (rate, ch, bits, frames, data)
}

/// The `MTrk` chunks of a Standard MIDI File (after its `MThd`), walked by
/// their lengths.
fn mtrk_chunks(bytes: &[u8]) -> usize {
    let mut at = 0usize;
    let mut n = 0;
    while let Some(head) = bytes.get(at..at + 8) {
        let len = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if &head[..4] == b"MTrk" {
            n += 1;
        }
        at += 8 + len;
    }
    n
}

/// The `.wav` files in `dir`.
fn wavs_in(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .collect();
    out.sort();
    out
}

/// Every path under `dir`, files and folders.
fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        out.push(p.clone());
        if p.is_dir() {
            walk(&p, out);
        }
    }
}

/// Ported from `audio_import_plays_and_reports_media` (README steps
/// 109–116 through the channel, plan 5a): a 44.1 kHz/16-bit
/// project stores an imported 48 kHz file in its own format; the demo
/// imports into a new lane, plays the region without disk faults, and
/// changes its format with the media converted.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_audio_import_plays_and_reports_media() {
    let tmp = temp_home();
    let mut app = App::start(tmp.path(), false);
    let src = app.launched.home.join("tone.caf");
    audio_file(&src, 96_000);

    // A new project in a chosen format.
    let made = app.call(
        "command.run",
        json!({"id": "file.new", "params": {"rate": "44100", "depth": "16"}}),
    );
    let window = made["window"].clone();
    let p = app.call("state.get", json!({"target": "project", "window": window}));
    assert_eq!(
        (p["rate"].clone(), p["depth"].clone()),
        (json!(44_100), json!("16")),
        "{p}"
    );
    let import = json!({"id": "media.import", "params": {"path": src}, "window": window});
    if cfg!(target_os = "linux") {
        // Linux gap, answered by host track H4 (Linux audio import adds a
        // resampler): the 48 kHz file cannot go into a 44.1 kHz project, so
        // the import is refused and every later step (which needs it) is
        // skipped. When H4 lands this assertion fails and forces the update.
        let e = app.call_err("command.run", import);
        assert_eq!(e.code, core_control::ErrorCode::Failed, "{e:?}");
        assert!(
            e.message.contains("resampling needs the system importer"),
            "{e:?}"
        );
        app.quit();
        return;
    }
    let r = app.call("command.run", import);
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let m = app.call("state.get", json!({"target": "media", "window": window}));
    let item = &m["items"][0];
    assert_eq!(item["name"], "tone", "{m}");
    assert_eq!(item["rate"], 44_100);
    assert_eq!(item["offline"], false);
    assert!(
        item["frames"].as_u64().unwrap().abs_diff(88_200) <= 1,
        "converted to 44.1 kHz: {item}"
    );

    // The demo: import into a new lane at bar 1 and play it.
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w2 = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w2}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let lanes_before = doc["lanes"].as_array().unwrap().len();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": src, "seq": seq, "tick": 0}, "window": w2}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let doc = app.call("state.get", json!({"target": "document", "window": w2}));
    let lanes = doc["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), lanes_before + 1, "{doc}");
    assert_eq!(lanes.last().unwrap()["regions"], 1, "{doc}");
    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": w2}),
    );
    app.wait(&w2, json!({"condition": "playing"}));
    app.wait(
        &w2,
        json!({"condition": "tick_at_least", "tick": 2 * 15_360}),
    );
    let t = app.call("state.get", json!({"target": "transport", "window": w2}));
    let faults: Vec<String> = t["faults"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        !faults.iter().any(|f| f == "stream-underrun"),
        "the region streamed in time: {t}"
    );
    assert_eq!(t["rate_mismatch"], false, "{t}");
    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": w2}),
    );
    app.wait(&w2, json!({"condition": "stopped"}));

    // Skipped: the audio editor step (a region clicked by its element,
    // `editors.audio_editor` and its controls inside the left pane) waits
    // for the Audio Editor slice; it is not in 7a.

    // Project Settings: 24-bit to float, every file converted.
    let r = app.call(
        "command.run",
        json!({"id": "file.project_settings", "params": {"depth": "32f"}, "window": w2}),
    );
    assert_eq!(r["undo"], "Change Project Format", "{r}");
    let p = app.call("state.get", json!({"target": "project", "window": w2}));
    assert_eq!(
        (p["depth"].clone(), p["mismatched"].clone()),
        (json!("32f"), json!(0)),
        "{p}"
    );
    let m = app.call("state.get", json!({"target": "media", "window": w2}));
    let tone = m["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "tone")
        .unwrap()
        .clone();
    assert_eq!(tone["offline"], false, "{m}");
    app.quit();
}

/// Ported from `regions_normalize_relink_and_trash` — its import, save and
/// relink part: Save As carries the media and the overview cache; an
/// undone import keeps its file for the redo history until another edit
/// drops it, when Save moves the orphan to the trash folder; a moved file
/// is relinked over the channel (no path names the Relink dialog, an
/// unknown item is refused) and copied back under a free name. The
/// fixtures are VizDAW CAF files, which import on Linux too (other formats
/// wait for host track H4).
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_relinks_imported_media() {
    let tmp = temp_home();
    let trash = tmp.path().join("Trash");
    let trash_env = trash.to_string_lossy().into_owned();
    let mut app = App::start_with(tmp.path(), false, &[("VIZDAW_TRASH_DIR", &trash_env)]);
    let home = app.launched.home.clone();
    // 2 s of stereo at 0.25 (−12.04 dBFS), and a short extra file.
    let tone = home.join("tone.caf");
    audio_file(&tone, 96_000);
    let extra = home.join("extra.caf");
    audio_file(&extra, 4_800);

    // The demo, with the tone in a new track at bar 1.
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": tone, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    let item = m["items"][0].clone();
    assert_eq!(
        (item["name"].clone(), item["used_percent"].clone()),
        (json!("tone"), json!(100.0)),
        "{m}"
    );
    let media = item["id"].as_u64().unwrap();

    // Skipped: selecting the region by its element and `region.normalize`
    // (its dialog, refusals and the region's gain label) wait for the
    // Region slice.

    // Save As carries the media and the overview cache (the worker writes
    // the overview after the import, so it may land a moment later).
    let pkg = home.join("Regions.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": pkg}, "window": w}),
    );
    assert!(pkg.join("Media/tone.caf").is_file());
    let caches = || {
        std::fs::read_dir(pkg.join("Media/.overviews"))
            .map(|d| {
                d.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "vzov"))
                    .count()
            })
            .unwrap_or(0)
    };
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    while caches() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(caches(), 1, "the overview came along");

    // An import, undone: redo can bring it back, so Save keeps its file.
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": extra}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    assert!(pkg.join("Media/extra.caf").is_file());
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    app.call("command.run", json!({"id": "file.save", "window": w}));
    assert!(
        pkg.join("Media/extra.caf").is_file(),
        "the redo history needs it"
    );
    // Another edit drops the redo history; Save moves the orphan to the
    // trash folder. (An autosave written meanwhile would rightly keep it,
    // so none is left lying around.) Swift's edit was `media.rename`,
    // which waits for the Media Pool slice: a new track is an edit too.
    app.call("command.run", json!({"id": "track.new_audio", "window": w}));
    let _ = std::fs::remove_dir_all(pkg.join("Autosave"));
    app.call("command.run", json!({"id": "file.save", "window": w}));
    assert!(
        !pkg.join("Media/extra.caf").exists(),
        "nothing references it"
    );
    assert!(trash.join("extra.caf").is_file(), "moved, never unlinked");

    // Relink: the tone moves out; without a path the dialog is named; the
    // relink copies it back under a free name (`tone.caf` is still the
    // undo history's).
    let moved = home.join("moved/tone.caf");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(pkg.join("Media/tone.caf"), &moved).unwrap();
    let e = app.call_err(
        "command.run",
        json!({"id": "media.relink", "params": {"media": media}, "window": w}),
    );
    assert_eq!(e.dialog.as_deref(), Some("Relink"), "{e}");
    let e = app.call_err(
        "command.run",
        json!({"id": "media.relink", "params": {"media": 999_999, "path": moved}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");
    let r = app.call(
        "command.run",
        json!({"id": "media.relink", "params": {"media": media, "path": moved}, "window": w}),
    );
    assert_eq!(r["undo"], "Relink Media", "{r}");
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    let item = &m["items"][0];
    // Swift's name was "Tone" (its `media.rename`).
    assert_eq!(
        (
            item["name"].clone(),
            item["path"].clone(),
            item["offline"].clone()
        ),
        (json!("tone"), json!("Media/tone 2.caf"), json!(false)),
        "{m}"
    );
    // Under the channel the Relink sheet never showed.
    let tree = app.call("ui.tree", json!({"window": w}));
    assert!(
        !tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["role"] == "sheet"),
        "no sheet under the channel"
    );

    // Skipped: the media pool window (`window.media_pool`) waits for the
    // Media Pool slice.
    app.quit();
}

/// Ported from `bounces_the_demo_and_checks_the_file`:
/// README steps 180–184 through the channel (plan 5f-2; spec §12.3 "bounce
/// and check the file"): a bounce of four bars to a 16-bit WAV, refused
/// over the same file without `replace`, then Add to project; hostile
/// calls refused.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_bounces_the_demo_and_checks_the_file() {
    let tmp = temp_home();
    let mut app = App::start(tmp.path(), false);
    let home = app.launched.home.clone();
    let src = home.join("tone.caf");
    audio_file(&src, 96_000);
    let demo = app.call("command.run", json!({"id": "file.create_demo_audio"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": src, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");

    // Four bars from tick 0 at 120 BPM: 16 × 15 360 ticks, 16 × 24 000 frames.
    let (end, range) = (16 * 15_360, 384_000u64);
    let mix = home.join("mix.wav");
    let params = json!({"path": mix, "bits": "16", "rate": 48_000, "range": "selection", "start": 0, "end": end});
    let r = app.call(
        "command.run",
        json!({"id": "file.bounce", "params": params.clone(), "window": w}),
    );
    let status = r["status"].as_str().unwrap_or("").to_string();
    assert!(
        status.starts_with("wrote ") && status.contains(&mix.display().to_string()),
        "{r}"
    );
    let (rate, ch, bits, frames, samples) = read_wav16(&mix);
    assert_eq!((rate, ch, bits), (48_000, 2, 16), "{status}");
    assert!(
        (range..=range + 480_000).contains(&frames),
        "{frames} frames: the range plus at most the 10 s tail (pre-flight M11)"
    );
    assert!(
        samples[..2 * 96_000].iter().any(|s| *s != 0),
        "the imported 0.25 tone sounds in the first bar"
    );
    let before = std::fs::read(&mix).unwrap();

    // The same path without `replace`: refused, the file unchanged.
    let e = app.call_err(
        "command.run",
        json!({"id": "file.bounce", "params": params, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::Failed, "{e:?}");
    assert!(e.message.contains("exists"), "{e:?}");
    assert_eq!(std::fs::read(&mix).unwrap(), before);

    // Add to project: one undo step, a lane "Mix", a Bounced item.
    let r = app.call(
        "command.run",
        json!({"id": "file.bounce", "params": {"add_to_project": true, "range": "selection", "start": 0, "end": end}, "window": w}),
    );
    assert_eq!(r["undo"], "Bounce", "{r}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(
        doc["lanes"].as_array().unwrap().last().unwrap()["name"],
        "Mix",
        "{doc}"
    );
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    assert!(
        m["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["origin"] == "bounced"),
        "{m}"
    );

    // Hostile calls.
    let e = app.call_err(
        "command.run",
        json!({"id": "file.bounce", "params": {"format": "mp3", "path": mix}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.bounce", "params": {}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::DialogRequired, "{e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.bounce", "params": {"add_to_project": false}, "window": w}),
    );
    assert_eq!(
        e.code,
        core_control::ErrorCode::DialogRequired,
        "the app's check (pre-flight M6): {e:?}"
    );
    app.quit();
}

/// Ported from `midi_files_round_trip_over_the_channel`:
/// README 257–266 through the channel (plan 6a Task 10): export the demo
/// as a MIDI file and a MIDI Clip File, import them back into the demo and
/// into a new project, and the refusals. No call opens a sheet or a panel;
/// only the new project's window appears.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_midi_files_round_trip_over_the_channel() {
    use core_control::ErrorCode;
    let tmp = temp_home();
    let mut app = App::start(tmp.path(), false);
    let home = app.launched.home.clone();
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let lanes = doc["lanes"].as_array().unwrap().clone();
    let tracks_before = lanes.len();
    // What plays is written (a reference's events too), and only tracks
    // with events make tracks again; the conductor makes none (F11).
    let midi_tracks = lanes
        .iter()
        .filter(|l| l["events"].as_u64().unwrap_or(0) + l["references"].as_u64().unwrap_or(0) > 0)
        .count();
    assert!(midi_tracks > 0, "{doc}");
    let windows = window_numbers(&mut app);

    // A dry run: a report, no file.
    let smf = home.join("demo.mid");
    let r = app.call(
        "command.run",
        json!({"id": "file.export_midi", "params": {"dry_run": true}, "window": w}),
    );
    assert!(r["report"]["summary"].is_string(), "{r}");
    assert_eq!(r["written"], json!([]), "{r}");
    assert!(!smf.exists());

    // The file, and what it wrote.
    let r = app.call(
        "command.run",
        json!({"id": "file.export_midi", "params": {"path": smf}, "window": w}),
    );
    assert_eq!(r["written"], json!([smf.display().to_string()]), "{r}");
    assert!(r["report"]["summary"].is_string(), "{r}");
    let bytes = std::fs::read(&smf).unwrap();
    assert_eq!(&bytes[..4], b"MThd");

    // One track as a MIDI Clip File.
    let clip = home.join("one.midi2");
    let lane = lanes
        .iter()
        .find(|l| l["events"].as_u64().unwrap_or(0) > 0)
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "file.export_midi_clip", "params": {"path": clip, "lanes": lane.to_string()}, "window": w}),
    );
    assert_eq!(r["written"], json!([clip.display().to_string()]), "{r}");
    assert_eq!(&std::fs::read(&clip).unwrap()[..8], b"SMF2CLIP");

    // Back into the demo at bar 9 (the demo is in 4/4): one undo step.
    let r = app.call(
        "command.run",
        json!({"id": "file.import_midi", "params": {"path": smf, "tick": 8 * 4 * 15_360}, "window": w}),
    );
    assert_eq!(r["undo"], "Import MIDI File", "{r}");
    let imported = r["report"].clone();
    assert!(imported["summary"].is_string(), "{r}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(
        doc["lanes"].as_array().unwrap().len(),
        tracks_before + midi_tracks,
        "{doc}"
    );
    let r = app.call(
        "command.run",
        json!({"id": "file.interchange_report", "window": w}),
    );
    assert_eq!(r["report"], imported, "{r}");
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "no call opened a sheet or a panel"
    );

    // As a new project: a second document, whose window is shown.
    let r = app.call(
        "command.run",
        json!({"id": "file.import_midi", "params": {"path": clip, "new_project": true}, "window": w}),
    );
    assert!(r["doc"].is_string() && r["doc"] != demo["doc"], "{r}");
    assert!(r["window"].is_i64(), "the new project's window: {r}");
    let after = window_numbers(&mut app);
    assert_eq!(after.len(), windows.len() + 1, "{after:?}");
    assert!(
        after
            .iter()
            .any(|(n, k)| *n == r["window"] && k == "document"),
        "{after:?}"
    );
    let made = app.call(
        "state.get",
        json!({"target": "document", "window": r["window"]}),
    );
    assert_eq!(made["lanes"].as_array().unwrap().len(), 1, "{made}");

    // Refusals: inside the demo's package, and no path.
    let pkg = home.join("Demo.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": pkg}, "window": w}),
    );
    assert!(pkg.is_dir());
    let inside = pkg.join("inside.mid");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_midi", "params": {"path": inside}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "{e:?}");
    assert!(!inside.exists());
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_midi", "params": {}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e:?}");
    assert!(e.message.contains("Save"), "{e:?}");
    assert_eq!(window_numbers(&mut app).len(), windows.len() + 1);
    app.quit();
}

/// Ported from `exports_for_another_daw_over_the_channel`:
/// README 267–273 through the channel (plan 6b Task 5): export the demo for
/// another DAW, re-import its MIDI and stems, export again with "Add the
/// stems to this project", and the refusals. The reply comes once the
/// `.exportProject` job ends; no call opens a sheet or a panel.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_exports_for_another_daw_over_the_channel() {
    use core_control::ErrorCode;
    let tmp = temp_home();
    let mut app = App::start(tmp.path(), false);
    let home = app.launched.home.clone();
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let windows = window_numbers(&mut app);
    let tmp = home.join("Exports");
    std::fs::create_dir(&tmp).unwrap();
    let lanes = |app: &mut App| {
        let doc = app.call("state.get", json!({"target": "document", "window": w}));
        doc["lanes"].as_array().unwrap().len()
    };

    // A dry run: a report, nothing written.
    let r = app.call(
        "command.run",
        json!({"id": "file.export_for_daw", "params": {"dry_run": true, "path": tmp}, "window": w}),
    );
    assert!(r["report"]["summary"].is_string(), "{r}");
    assert_eq!(r["written"], json!([]), "{r}");
    assert_eq!(
        std::fs::read_dir(&tmp).unwrap().count(),
        0,
        "tmp stays empty"
    );

    // The folder and its files.
    let r = app.call(
        "command.run",
        json!({"id": "file.export_for_daw", "params": {"path": tmp}, "window": w}),
    );
    assert!(r["report"]["summary"].is_string(), "{r}");
    let written = r["written"].as_array().unwrap();
    assert_eq!(written.len(), 1, "{r}");
    let folder = PathBuf::from(written[0].as_str().unwrap());
    assert_eq!(folder.parent(), Some(tmp.as_path()), "{r}");
    let name = folder.file_name().unwrap().to_str().unwrap().to_string();
    assert!(name.ends_with(" for another DAW"), "{name}");
    for f in [
        "Tempo map.mid",
        "About these files.txt",
        "MIDI/All MIDI.mid",
    ] {
        assert!(folder.join(f).is_file(), "{f} in {folder:?}");
    }
    let stems = wavs_in(&folder.join("Audio"));
    assert!(!stems.is_empty(), "at least one stem");
    let mut all = Vec::new();
    walk(&tmp, &mut all);
    assert!(
        !all.iter()
            .any(|p| p.to_string_lossy().ends_with(".partial")),
        "no partial is left: {all:?}"
    );
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "no call opened a sheet or a panel"
    );

    // The MIDI back in at the start: one track per MIDI track after the
    // conductor (F17).
    let all_midi = folder.join("MIDI/All MIDI.mid");
    let bytes = std::fs::read(&all_midi).unwrap();
    assert_eq!(&bytes[..4], b"MThd");
    let tracks = mtrk_chunks(&bytes)
        .checked_sub(1)
        .expect("a conductor track");
    assert!(tracks > 0, "{tracks}");
    let before = lanes(&mut app);
    let r = app.call(
        "command.run",
        json!({"id": "file.import_midi", "params": {"path": all_midi, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import MIDI File", "{r}");
    assert_eq!(lanes(&mut app), before + tracks);

    // And the stems: one track each.
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    if cfg!(target_os = "linux") {
        // Linux gap, answered by host track H4 (Linux audio import reads
        // WAV/AIFF and other formats, with a resampler): today the Linux
        // engine imports only VizDAW CAF files, so the WAV stem is refused
        // and the steps that need it (the stems back in, the later exports
        // and their undo labels) are skipped. When H4 lands this assertion
        // fails and forces the update.
        let e = app.call_err(
            "command.run",
            json!({"id": "media.import", "params": {"path": stems[0], "seq": seq, "tick": 0}, "window": w}),
        );
        assert_eq!(e.code, ErrorCode::Failed, "{e:?}");
        assert!(e.message.contains("not a VizDAW CAF file"), "{e:?}");
        assert_eq!(lanes(&mut app), before + tracks, "nothing was added");
    } else {
        for (k, stem) in stems.iter().enumerate() {
            let before = lanes(&mut app);
            let r = app.call(
            "command.run",
            json!({"id": "media.import", "params": {"path": stem, "seq": seq, "tick": 0}, "window": w}),
        );
            assert_eq!(r["undo"], "Import Audio", "stem {k}: {r}");
            assert_eq!(lanes(&mut app), before + 1, "stem {k}");
        }

        // Again without adding: "… 2", and the undo label stays that of the
        // last edit.
        let r = app.call(
            "command.run",
            json!({"id": "file.export_for_daw", "params": {"path": tmp}, "window": w}),
        );
        assert_eq!(r["undo"], "Import Audio", "no edit without Add: {r}");
        assert_eq!(
            PathBuf::from(r["written"][0].as_str().unwrap()),
            tmp.join(format!("{name} 2")),
            "{r}"
        );

        // Again, adding the stems: "… 3", the first folder untouched, one
        // track per stem in one undo step.
        let first: Vec<PathBuf> = {
            let mut v = Vec::new();
            walk(&folder, &mut v);
            v.sort();
            v
        };
        let before = lanes(&mut app);
        let r = app.call(
        "command.run",
        json!({"id": "file.export_for_daw", "params": {"path": tmp, "add_to_project": true}, "window": w}),
    );
        assert_eq!(r["undo"], "Add Stems", "{r}");
        let second = PathBuf::from(r["written"][0].as_str().unwrap());
        assert_eq!(second, tmp.join(format!("{name} 3")), "{r}");
        let added = wavs_in(&second.join("Audio")).len();
        assert!(added > 0, "{r}");
        assert_eq!(lanes(&mut app), before + added);
        app.call("command.run", json!({"id": "edit.undo", "window": w}));
        assert_eq!(
            lanes(&mut app),
            before,
            "one Undo removes every added track"
        );
        let mut again = Vec::new();
        walk(&folder, &mut again);
        again.sort();
        assert_eq!(again, first, "the first folder is untouched");
        let exported = r["report"].clone();
        let r = app.call(
            "command.run",
            json!({"id": "file.interchange_report", "window": w}),
        );
        assert_eq!(r["report"], exported, "{r}");
    }

    // Refusals: inside the demo's package, and no path.
    let pkg = home.join("Demo.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": pkg}, "window": w}),
    );
    assert!(pkg.is_dir());
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_for_daw", "params": {"path": pkg}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "{e:?}");
    assert!(
        e.message.contains(&pkg.display().to_string()),
        "names the path: {e:?}"
    );
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_for_daw", "params": {}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e:?}");
    assert!(e.message.contains("Save"), "{e:?}");
    assert_eq!(window_numbers(&mut app), windows);
    app.quit();
}

/// Ported from `dawproject_round_trip_over_the_channel`:
/// README 282–290 through the channel (plan 6d Task 13): hostile calls to
/// both DAWproject commands change nothing and open nothing; a dry run
/// writes nothing; the export writes a ZIP and refuses to replace it
/// without `replace`; the import makes a new project with the demo's
/// tracks and the VizDAW-origin note, and its report is the one
/// `file.interchange_report` gives. Only the new project's window appears.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_dawproject_round_trip_over_the_channel() {
    use core_control::ErrorCode;
    let tmp = temp_home();
    let mut app = App::start(tmp.path(), false);
    let home = app.launched.home.clone();
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let tracks = doc["lanes"].as_array().unwrap().len();
    assert!(tracks > 0, "{doc}");
    let windows = window_numbers(&mut app);
    let file = home.join("demo.dawproject");
    // Everything under the throwaway home, to show the dry run adds nothing.
    // The app keeps writing its own files there, so a folder that goes away
    // mid-walk is skipped.
    fn walk_quiet(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for p in rd.flatten().map(|e| e.path()) {
            out.push(p.clone());
            // lstat: a link is listed, never followed.
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) {
                walk_quiet(&p, out);
            }
        }
    }
    let listing = || {
        let mut v = Vec::new();
        walk_quiet(&home, &mut v);
        v.sort();
        v
    };

    // Hostile calls: each refused, none opens anything.
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_dawproject", "params": {"path": file, "region_gain": "half"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_dawproject", "params": {"path": "demo.dawproject"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "a relative path: {e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_dawproject", "params": {}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e:?}");
    assert!(e.message.contains("Save"), "{e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.import_dawproject", "params": {"path": file, "mode": "fuse"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}");
    assert!(!file.exists());
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "no hostile call opened a sheet, a panel or a window"
    );

    // A dry run: a report, nothing written anywhere under the home.
    let before = listing();
    let r = app.call(
        "command.run",
        json!({"id": "file.export_dawproject", "params": {"dry_run": true}, "window": w}),
    );
    assert!(r["report"]["summary"].is_string(), "{r}");
    assert_eq!(r["written"], json!([]), "{r}");
    let new: Vec<_> = listing()
        .into_iter()
        .filter(|p| !before.contains(p))
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e == "dawproject" || e == "partial")
        })
        .collect();
    assert!(new.is_empty(), "the dry run wrote {new:?}");
    assert!(!file.exists());

    // The file: a ZIP.
    let r = app.call(
        "command.run",
        json!({"id": "file.export_dawproject", "params": {"path": file}, "window": w}),
    );
    let summary = r["report"]["summary"].as_str().unwrap_or_default();
    assert!(summary.starts_with("Exported"), "{r}");
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "the export opened nothing"
    );

    // Again without `replace`: refused, naming the file, which is kept.
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_dawproject", "params": {"path": file}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "{e:?}");
    assert!(
        e.message.contains("demo.dawproject"),
        "names the file: {e:?}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), bytes, "the old file is kept");

    // Back in as a new project: its window, the demo's tracks and the note.
    let r = app.call(
        "command.run",
        json!({"id": "file.import_dawproject", "params": {"path": file, "mode": "new"}, "window": w}),
    );
    assert!(r["doc"].is_string() && r["doc"] != demo["doc"], "{r}");
    assert!(r["window"].is_i64(), "the new project's window: {r}");
    let imported = r["report"].clone();
    assert!(
        imported
            .to_string()
            .contains("This file came from a VizDAW project"),
        "{imported}"
    );
    let after = window_numbers(&mut app);
    assert_eq!(after.len(), windows.len() + 1, "{after:?}");
    assert!(
        after
            .iter()
            .any(|(n, k)| *n == r["window"] && k == "document"),
        "{after:?}"
    );
    let made = app.call(
        "state.get",
        json!({"target": "document", "window": r["window"]}),
    );
    assert_eq!(made["lanes"].as_array().unwrap().len(), tracks, "{made}");

    // The new window's report is the import's.
    let again = app.call(
        "command.run",
        json!({"id": "file.interchange_report", "window": r["window"]}),
    );
    assert_eq!(again["report"], imported, "{again}");
    app.quit();
}

/// Ported from `aaf_round_trip_over_the_channel`:
/// README 300–309 through the channel (plan 6f Task 9): hostile calls to
/// both AAF commands are refused and open nothing; the export writes the
/// "<song> AAF" folder; the import makes a new project with the demo's
/// audio tracks and the note that the file came from VizDAW, with no
/// internal error. Only the new project's window appears.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_aaf_round_trip_over_the_channel() {
    use core_control::ErrorCode;
    let tmp = temp_home();
    let mut app = App::start(tmp.path(), false);
    let home = app.launched.home.clone();
    let demo = app.call("command.run", json!({"id": "file.create_demo_audio"}));
    let w = demo["window"].clone();
    let audio = |d: &Value| {
        d["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["kind"] == "audio")
            .count()
    };
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let tracks = audio(&doc);
    assert!(tracks > 0, "the demo has an audio track: {doc}");
    let windows = window_numbers(&mut app);
    let out = home.join("aaf-out");
    std::fs::create_dir(&out).unwrap();

    // Hostile calls: no `path` needs the Save dialog; a relative one is
    // refused; neither opens anything.
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_aaf", "params": {}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e:?}");
    assert!(e.message.contains("Save"), "{e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.export_aaf", "params": {"path": "aaf-out"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "a relative path: {e:?}");
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "no hostile call opened a sheet, a panel or a window"
    );

    // The export: a folder with the AAF.
    let r = app.call(
        "command.run",
        json!({"id": "file.export_aaf", "params": {"path": out}, "window": w}),
    );
    assert!(r["report"]["summary"].is_string(), "{r}");
    let folder = PathBuf::from(r["written"][0].as_str().unwrap());
    let aaf = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "aaf"))
        .unwrap_or_else(|| panic!("no .aaf in {folder:?}: {r}"));
    assert!(aaf.is_file(), "{aaf:?}");
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "the export opened nothing"
    );

    // Back in as a new project: its window, no internal error, the note.
    let r = app.call(
        "command.run",
        json!({"id": "file.import_aaf", "params": {"path": aaf, "new_project": true}, "window": w}),
    );
    assert!(r["doc"].is_string() && r["doc"] != demo["doc"], "{r}");
    assert!(r["window"].is_i64(), "the new project's window: {r}");
    let report = r["report"].to_string();
    assert!(
        report.contains("This file came from VizDAW; AAF files open as new projects"),
        "{report}"
    );
    assert!(!report.contains("internal"), "{report}");
    let after = window_numbers(&mut app);
    assert_eq!(after.len(), windows.len() + 1, "{after:?}");
    let made = app.call(
        "state.get",
        json!({"target": "document", "window": r["window"]}),
    );
    assert_eq!(audio(&made), tracks, "{made}");
    app.quit();
}

/// Ported from `dawproject_merge_over_the_channel`:
/// README 291–299 through the channel (plan 6e Task 11): a file from
/// VizDAW without `mode` is `dialog_required` "Return"; hostile merge calls
/// are refused and open nothing; a preview and a merge of the project's own
/// unchanged export find nothing to merge, add no undo step and open no
/// window; `file.interchange_report` gives the merge's report; and 6d's
/// path (`mode: "new"`) still makes a new project.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_dawproject_merge_over_the_channel() {
    use core_control::ErrorCode;
    const NOTHING: &str = "Nothing to merge: the file matches the project.";
    let tmp = temp_home();
    let mut app = App::start(tmp.path(), false);
    let home = app.launched.home.clone();
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let tracks = doc["lanes"].as_array().unwrap().len();
    assert!(tracks > 0, "{doc}");
    let project = home.join("demo.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": project}, "window": w}),
    );
    assert!(project.join("project.json").is_file());
    let file = home.join("demo.dawproject");
    let r = app.call(
        "command.run",
        json!({"id": "file.export_dawproject", "params": {"path": file}, "window": w}),
    );
    let summary = r["report"]["summary"].as_str().unwrap_or_default();
    assert!(summary.starts_with("Exported"), "{r}");
    let windows = window_numbers(&mut app);

    // A file from VizDAW needs `mode`; hostile merge calls are refused.
    let e = app.call_err(
        "command.run",
        json!({"id": "file.import_dawproject", "params": {"path": file}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e:?}");
    assert!(e.message.contains("Return"), "{e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.import_dawproject",
               "params": {"path": file, "mode": "merge", "snapshot": "x"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}");
    assert!(e.message.contains("snapshot"), "names `snapshot`: {e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.import_dawproject",
               "params": {"path": file, "mode": "merge", "project": project}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "already open: {e:?}");
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "no refused call opened a sheet, a panel or a window"
    );

    // A preview: the file matches the project; nothing changes.
    let before = app.call("state.get", json!({"target": "document", "window": w}));
    let r = app.call(
        "command.run",
        json!({"id": "file.import_dawproject",
               "params": {"path": file, "mode": "merge", "preview": true}, "window": w}),
    );
    assert_eq!(r["report"]["summary"], NOTHING, "{r}");
    let after = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(after["undo"], before["undo"], "the preview added no step");

    // The merge: the same summary, no undo step, no window.
    let r = app.call(
        "command.run",
        json!({"id": "file.import_dawproject",
               "params": {"path": file, "mode": "merge"}, "window": w}),
    );
    assert_eq!(r["report"]["summary"], NOTHING, "{r}");
    let merged = r["report"].clone();
    let after = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(
        after["undo"], before["undo"],
        "nothing to merge adds no step"
    );
    assert_eq!(
        window_numbers(&mut app),
        windows,
        "the merge opened nothing"
    );

    // Its report is the window's last.
    let again = app.call(
        "command.run",
        json!({"id": "file.interchange_report", "window": w}),
    );
    assert_eq!(again["report"], merged, "{again}");

    // 6d's path still works: a new project with the demo's tracks.
    let r = app.call(
        "command.run",
        json!({"id": "file.import_dawproject", "params": {"path": file, "mode": "new"}, "window": w}),
    );
    assert!(r["window"].is_i64(), "the new project's window: {r}");
    let now = window_numbers(&mut app);
    assert_eq!(now.len(), windows.len() + 1, "{now:?}");
    let made = app.call(
        "state.get",
        json!({"target": "document", "window": r["window"]}),
    );
    assert_eq!(made["lanes"].as_array().unwrap().len(), tracks, "{made}");
    app.quit();
}

// Slice 8a: recording and step entry over the channel. The offline
// device's fake input and `midi.inject`: no hardware, so they run on
// Linux CI too.

/// Polls `state.get record` until `ok` holds; returns it (copied from
/// `e2e.rs`).
fn wait_for_record(app: &mut App, window: &Value, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let r = app.call("state.get", json!({"target": "record", "window": window}));
        if ok(&r) {
            return r;
        }
        assert!(
            Instant::now() < deadline,
            "record state within {WAIT_MS} ms: {r}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The track named `name` in `state.get document` (copied from `e2e.rs`).
fn lane_named(app: &mut App, window: &Value, name: &str) -> Value {
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == name)
        .unwrap_or_else(|| panic!("no track {name} in {doc}"))
        .clone()
}

/// A project of one sequence "Take" at 120 BPM, `bars` bars long: "Keys"
/// (MIDI from source "Keys" on the built-in synth) and "Mic" (fake input
/// 1, heard never). Saved at `path`.
fn record_fixture(path: &std::path::Path, bars: i64) {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
    use core_model::ids::AnyId;
    use core_model::lane::{InputChannels, LaneInput, MonitorMode};
    use core_model::sequence::SeqLength;
    use core_model::tempo::{Bpm, TempoMap, TempoPoint};
    use core_model::time::Tick;
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Take", "Keys", Some(synth)).unwrap();
    let mic = b.lane(seq, "Mic", None).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let out = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    for c in [
        LaneCmd::SetOutput {
            seq,
            lane: mic,
            output: Some(out),
        },
        LaneCmd::SetInput {
            seq,
            lane: keys,
            input: Some(LaneInput::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneCmd::SetInput {
            seq,
            lane: mic,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(0),
                return_of: None,
            }),
        },
        LaneCmd::SetMonitor {
            seq,
            lane: mic,
            monitor: MonitorMode::Off,
        },
    ] {
        b.run(Command::Lane(c)).unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetTempo {
        seq,
        tempo: TempoMap::new(vec![TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: false,
        }])
        .unwrap(),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(bars * 4 * Q)),
    }))
    .unwrap();
    core_model::file::save(&b.finish(), path).unwrap();
}

/// Ported from `records_audio_loop_takes_and_midi` (README steps 130, 134
/// and 137): the offline device's fake input records onto an armed track
/// as one undo step "Record" with a recorded media item; loop recording
/// with a keep after pass 1 and a discard after pass 2 leaves two new
/// takes; MIDI injected into an armed track while recording lands as a
/// note. No audio device is opened and no microphone permission is
/// asked.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_records_audio_loop_takes_and_midi() {
    let home = temp_home();
    let mut app = App::start_with(home.path(), false, &[("VIZDAW_AUDIO_INPUT", "fake")]);
    // Four bars at 120 BPM: eight seconds a pass, so a keep or discard
    // lands in the pass it means even on a loaded machine.
    let path = home.path().join("Rec.vizdaw");
    record_fixture(&path, 4);
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));

    // Audio: arm Mic (none armed: the last track), record to the end.
    app.command("transport.arm_previous", json!({}));
    assert_eq!(lane_named(&mut app, &w, "Mic")["arm"], true);
    app.command(
        "record.set",
        json!({"replace": true, "loop": false, "pre_roll": 0}),
    );
    app.command("transport.record", json!({}));
    wait_for_record(&mut app, &w, |r| r["recording"] == true);
    // The sequence ends after eight seconds: the pass ends by itself.
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    let item = m["items"][0].clone();
    assert_eq!(item["origin"], "recorded", "{m}");
    assert!(
        item["path"]
            .as_str()
            .unwrap()
            .starts_with("Media/Recorded/Mic "),
        "{item}"
    );
    assert_eq!(
        (item["offline"].clone(), item["rate"].clone()),
        (json!(false), json!(48_000))
    );
    assert_eq!(lane_named(&mut app, &w, "Mic")["regions"], 1);
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(input["channels"], 3, "{input}");

    // Loop: keep after pass 1, discard after pass 2, stop in pass 3.
    app.command("record.set", json!({"loop": true, "passes_as_takes": true}));
    app.command("transport.record", json!({}));
    wait_for_record(&mut app, &w, |r| r["pass"].as_u64() >= Some(2));
    app.command("record.keep_pass", json!({}));
    wait_for_record(&mut app, &w, |r| r["pass"].as_u64() >= Some(3));
    app.command("record.discard_pass", json!({}));
    // Only the total: which pass each press lands on is timing.
    let r = app.call("state.get", json!({"target": "record", "window": w}));
    assert_eq!(
        r["kept"].as_u64().unwrap() + r["discarded"].as_u64().unwrap(),
        2,
        "{r}"
    );
    // Half a second into pass 3, counted from the pass's start.
    assert_eq!(r["pass"], 3, "{r}");
    let at = r["pass_start_file_frame"].as_u64().unwrap();
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(at + 24_000));
    app.command("transport.record", json!({}));
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let lane = lane_named(&mut app, &w, "Mic");
    assert_eq!(
        (lane["takes"].clone(), lane["active_take"].clone()),
        (json!(3), json!("Take 3")),
        "{lane}"
    );

    // MIDI: arm Keys instead, record from the start, play a note in.
    app.command("record.set", json!({"loop": false}));
    app.command("transport.arm_previous", json!({}));
    assert_eq!(lane_named(&mut app, &w, "Keys")["arm"], true);
    app.command("transport.return_to_start", json!({}));
    wait_for_tick(&mut app, &w, 0);
    app.command("transport.record", json!({}));
    wait_for_record(&mut app, &w, |r| {
        r["recording"] == true && r["frames"].as_u64() >= Some(2_400)
    });
    app.command("midi.inject", json!({"port": "Keys", "bytes": "90 3c 64"}));
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(9_600));
    app.command("midi.inject", json!({"port": "Keys", "bytes": "80 3c 00"}));
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    assert_eq!(lane_named(&mut app, &w, "Keys")["events"], 1);
    app.quit();
}

/// Ported from `arpeggiates_an_injected_chord_while_recording` (spec
/// §12.3; README 239): a C major chord held for a bar into "Keys" with an
/// Up arpeggiator at 1/16 records as C, E, G repeating, each on a
/// sixteenth and half a sixteenth long. Offline device, no MIDI port.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_arpeggiates_an_injected_chord_while_recording() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::event::EventBody;
    use core_model::input_fx::{Arp, InputFx, InputFxKind};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let home = temp_home();
    let mut app = App::start(home.path(), false);
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Arp", "Keys", Some(synth)).unwrap();
    for c in [
        LaneCmd::SetInput {
            seq,
            lane: keys,
            input: Some(LaneInput::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneCmd::SetInputFx {
            seq,
            lane: keys,
            chain: vec![InputFx::new(InputFxKind::Arpeggiator(Arp::default()))],
        },
        LaneCmd::SetFlags {
            seq,
            lane: keys,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    ] {
        b.run(Command::Lane(c)).unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(8 * Q)),
    }))
    .unwrap();
    let path = home.path().join("Arp.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let lane = lane_named(&mut app, &w, "Keys");
    assert_eq!(
        lane["input_fx"][0]["kind"]["arpeggiator"]["rate"]["division"], 16,
        "{lane}"
    );
    assert_eq!(lane["arm"], true, "{lane}");
    app.command(
        "record.set",
        json!({"replace": true, "loop": false, "pre_roll": 0, "count_in": 0}),
    );
    app.command("transport.record", json!({}));
    wait_for_record(&mut app, &w, |r| {
        r["recording"] == true && r["frames"].as_u64() >= Some(24_000)
    });
    app.command(
        "midi.inject",
        json!({"port": "Keys", "bytes": "90 3c 64 90 40 64 90 43 64"}),
    );
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(24_000 * 5));
    app.command(
        "midi.inject",
        json!({"port": "Keys", "bytes": "80 3c 00 80 40 00 80 43 00"}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(input["midi_fx_dropped"], 0, "{input}");
    app.command("file.save", json!({}));
    let doc = core_model::file::load(&path).unwrap();
    let l = doc
        .sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.id == keys)
        .unwrap();
    let notes: Vec<(i64, u8, i64)> = l
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match &e.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some((e.tick.0, *pitch, duration.0)),
            _ => None,
        })
        .collect();
    assert!(notes.len() >= 12, "about a bar of sixteenths: {notes:?}");
    let first = [60u8, 64, 67]
        .iter()
        .position(|p| *p == notes[0].1)
        .expect("a chord note");
    for (k, (tick, pitch, duration)) in notes.iter().enumerate() {
        assert_eq!(*pitch, [60, 64, 67][(first + k) % 3], "{notes:?}");
        let off = tick % (Q / 4);
        assert!(
            off.min(Q / 4 - off) <= 2,
            "{tick} is off the sixteenths: {notes:?}"
        );
        assert!(
            (duration - Q / 8).abs() <= 2,
            "{duration} is not half a sixteenth: {notes:?}"
        );
    }
    app.quit();
}

/// Ported from `keys_drive_step_entry_locators_and_bindings` (README
/// steps 99–104), the step entry and locator half (the bindings file
/// waits for Keys settings; G is `gpui_g_toggles_snap`): T on, 4 an
/// eighth, a typed chord, 0 a rest, Delete takes steps back; P gives the
/// Position field the keyboard and a typed bar moves there, ⌥2 sets a
/// locator there, Return goes to the start and 2 jumps to the locator.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_keys_drive_step_entry_and_locators() {
    use core_engine::fixtures::Builder;
    const Q: i64 = 15_360;
    let home = temp_home();
    let mut app = App::start(home.path(), false);
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    b.sequence("Tune", "Keys", Some(synth)).unwrap();
    let path = home.path().join("Tune.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let window = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    let key = |app: &mut App, k: &str, mods: Value| {
        app.call(
            "ui.key",
            json!({"window": window, "key": k, "modifiers": mods}),
        );
    };

    // Step entry: T, then 4 for eighths.
    key(&mut app, "t", json!([]));
    wait_for_editor(&mut app, &window, "step entry on", |e| {
        e["step"]["value"] == "quarter"
    });
    key(&mut app, "4", json!([]));
    wait_for_editor(&mut app, &window, "eighths", |e| {
        e["step"]["value"] == "eighth"
    });
    // A chord (MIDI input's stand-in), then a rest: the position moves on.
    app.call(
        "command.run",
        json!({"id": "step.notes", "params": {"pitches": "C4 E4 G4"}, "window": window}),
    );
    wait_for_undo(&mut app, &window, "Step Entry");
    let sel = app.call(
        "state.get",
        json!({"target": "selection", "window": window}),
    );
    assert_eq!(
        sel["events"].as_array().unwrap().len(),
        3,
        "the chord is selected: {sel}"
    );
    wait_for_tick(&mut app, &window, Q / 2);
    key(&mut app, "0", json!([]));
    wait_for_editor(&mut app, &window, "a rest", |e| e["step"]["tick"] == Q);
    // Delete takes the rest back, then the chord.
    key(&mut app, "delete", json!([]));
    wait_for_editor(&mut app, &window, "the rest back", |e| {
        e["step"]["tick"] == Q / 2
    });
    key(&mut app, "delete", json!([]));
    wait_for_undo(&mut app, &window, "Delete Last Step");
    wait_for_tick(&mut app, &window, 0);
    key(&mut app, "t", json!([]));
    wait_for_editor(&mut app, &window, "step entry off", |e| e["step"].is_null());

    // P gives the Position field the keyboard, a typed bar and Return
    // move there; ⌥2 sets locator 2 there. A short bar keeps Position on
    // it (the default bar can fold it into » on a small screen).
    app.call(
        "command.run",
        json!({"id": "ui.set_transport_bar", "params": {"items": "cluster,position_field"}}),
    );
    wait_for_node(&mut app, &window, "Position");
    key(&mut app, "p", json!([]));
    app.call("ui.key", json!({"window": window, "text": "3\n"}));
    wait_for_tick(&mut app, &window, 8 * Q);
    key(&mut app, "2", json!(["option"]));
    wait_for_editor(&mut app, &window, "locator 2", |e| {
        e["transport"]["locators"][2] == 8 * Q
    });
    key(&mut app, "return", json!([]));
    wait_for_tick(&mut app, &window, 0);
    key(&mut app, "2", json!([]));
    wait_for_tick(&mut app, &window, 8 * Q);
    app.quit();
}

/// `record.recover` over the channel (slice 8a; Linux gap H4): a take
/// left in the project's `Media/Recorded` that no region uses is offered
/// at the next open, without the Recovered Recordings alert while the
/// channel is on. Trash moves it to the trash folder on macOS (the e2e's
/// own, never the real Trash); on Linux the file stays and the problem
/// line is the call's answer.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_record_recover_trash_over_the_channel() {
    let home = temp_home();
    let trash = home.path().join("Trash");
    let trash_env = trash.to_string_lossy().into_owned();
    let mut env = vec![("VIZDAW_AUDIO_INPUT", "fake")];
    // macOS: never the real Trash. Linux: no folder, so the platform's
    // (missing) Trash is what answers.
    if cfg!(target_os = "macos") {
        env.push(("VIZDAW_TRASH_DIR", &trash_env));
    }
    let mut app = App::start_with(home.path(), false, &env);
    let path = home.path().join("Left.vizdaw");
    record_fixture(&path, 1);
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let err = app.call_err(
        "command.run",
        json!({"id": "record.recover", "params": {"action": "trash"}, "window": w}),
    );
    assert_eq!(
        (err.code, err.message.as_str()),
        (
            core_control::ErrorCode::Unavailable,
            "There are no recovered recordings."
        )
    );
    // One pass to the end (two seconds), saved.
    app.command("transport.arm_previous", json!({}));
    app.command(
        "record.set",
        json!({"replace": true, "loop": false, "pre_roll": 0, "count_in": 0}),
    );
    app.command("transport.record", json!({}));
    wait_for_record(&mut app, &w, |r| r["recording"] == true);
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    app.command("file.save", json!({}));
    // Quit (closing moves unused media to the trash, and has finished
    // once the app is gone), leave a second copy of the take that nothing
    // names, then open the project in a new app.
    app.quit();
    let recorded = path.join("Media/Recorded");
    let take = std::fs::read_dir(&recorded)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "caf"))
        .unwrap_or_else(|| panic!("no take in {}", recorded.display()));
    let left = recorded.join("Left Behind.caf");
    std::fs::copy(&take, &left).unwrap();
    let mut app = App::start_with(home.path(), false, &env);
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let r = app.call("state.get", json!({"target": "record", "window": w}));
    assert_eq!(
        r["recovered"],
        json!(["Media/Recorded/Left Behind.caf"]),
        "{r}"
    );
    let shown = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(
        shown["alerts"],
        json!([]),
        "no alert under the channel: {shown}"
    );
    let d = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(d["alert"], Value::Null, "{d}");

    let answer = app.client.call(
        "command.run",
        json!({"id": "record.recover", "params": {"action": "trash"}, "window": w}),
        Duration::from_millis(WAIT_MS),
    );
    if cfg!(target_os = "macos") {
        answer.unwrap_or_else(|e| panic!("record.recover: {e:?}"));
        assert!(!left.exists(), "moved out of the project");
        assert!(
            trash.join("Left Behind.caf").is_file(),
            "into the trash folder"
        );
    } else {
        match answer {
            Err(CallError::Reply(e)) => assert_eq!(
                e.message,
                "Media/Recorded/Left Behind.caf: not moved to the Trash (this platform has no \
                 Trash support yet)"
            ),
            other => panic!("expected the problem line, got {other:?}"),
        }
        assert!(left.is_file(), "the file stays where it was");
    }
    let shown = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(shown["alerts"], json!([]), "{shown}");
    app.quit();
}
