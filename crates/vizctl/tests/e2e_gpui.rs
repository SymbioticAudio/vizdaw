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
