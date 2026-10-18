//! End to end (control spec §11): launches the built VizDAW with the
//! control channel in a throwaway home and drives README steps through
//! the socket. Ignored by default: it needs a window server and a built
//! app. Run it with
//!
//! ```text
//! VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Every wait is a condition with a generous timeout (slow CI runners);
//! there is no fixed sleep, only short poll intervals.

use core_control::client::{CallError, Client};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use vizctl::launch::{LaunchOptions, Launched, launch, log_tail};

/// Generous: macos-15 runners are slow.
const WAIT_MS: u64 = 60_000;

struct App {
    launched: Launched,
    client: Client,
}

impl App {
    fn start() -> App {
        let app = std::env::var("VIZDAW_E2E_APP")
            .expect("set VIZDAW_E2E_APP to VizDAW.app or its executable");
        // Cargo runs tests in the crate's folder: a relative path is
        // relative to the workspace root, where the app is built.
        let app = {
            let p = PathBuf::from(app);
            if p.is_absolute() {
                p
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(p)
            }
        };
        let home = tempfile::Builder::new()
            .prefix("vze")
            .tempdir_in("/tmp")
            .unwrap()
            .keep();
        let launched = launch(&LaunchOptions {
            app,
            home: Some(home),
            demo: false,
            timeout: Duration::from_secs(180),
            env: vec![("VIZDAW_AUDIO".into(), "offline".into())],
        })
        .unwrap_or_else(|e| panic!("launch: {e}"));
        let client = Client::connect(&launched.socket).unwrap();
        App { launched, client }
    }

    /// Calls `cmd`; panics with the app's log on an error.
    fn call(&mut self, cmd: &str, params: Value) -> Value {
        let timeout = Duration::from_millis(WAIT_MS) + Duration::from_secs(60);
        match self.client.call(cmd, params.clone(), timeout) {
            Ok(v) => v,
            Err(e) => panic!(
                "{cmd} {params}: {e}\nVizDAW log:\n{}",
                log_tail(&self.launched.log, 40)
            ),
        }
    }

    fn call_err(&mut self, cmd: &str, params: Value) -> core_control::ControlError {
        match self.client.call(cmd, params, Duration::from_secs(120)) {
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

    /// Quits and waits (generously) for the process to end.
    fn quit(mut self) {
        assert_eq!(self.call("app.quit", json!({}))["quitting"], true);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(Some(status)) = self.launched.child.try_wait() {
                assert!(status.success(), "VizDAW exited with {status}");
                break;
            }
            if Instant::now() > deadline {
                let _ = self.launched.child.kill();
                panic!(
                    "VizDAW did not quit; log:\n{}",
                    log_tail(&self.launched.log, 40)
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!self.launched.socket.exists(), "the socket file is gone");
    }
}

/// README smoke steps 2–3 and 32 through the channel: create the demo,
/// play, check the transport moves, screenshot, stop, open the list.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn demo_play_screenshot_stop() {
    let mut app = App::start();
    let opened = app.call("app.new_demo", json!({}));
    let window = opened["window"].clone();
    assert!(window.as_i64().is_some_and(|w| w > 0), "{opened}");
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );

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
    // command (spec §5.3).
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

/// Events into VizDAW's own window: a click on the toolbar's Play button
/// plays, Space (the Transport menu's key) stops.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn click_play_and_press_space() {
    let mut app = App::start();
    let window = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    // The toolbar may come up after the window (slow runners): poll the
    // tree for the button labelled exactly "Play".
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    let id = loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(id) = play_button(&tree["nodes"]) {
            break id;
        }
        assert!(
            Instant::now() < deadline,
            "no Play button within {WAIT_MS} ms in {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    app.call("ui.click", json!({"window": window, "element": id}));
    app.wait(&window, json!({"condition": "playing"}));
    app.call("ui.key", json!({"window": window, "key": "space"}));
    app.wait(&window, json!({"condition": "stopped"}));
    app.quit();
}

/// The ID of the first tree node labelled exactly `label`.
fn labelled(nodes: &Value, label: &str) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        if n["label"].as_str() == Some(label) {
            Some(n["id"].clone())
        } else {
            labelled(&n["children"], label)
        }
    })
}

/// Polls the window's tree until a node is labelled exactly `label`.
fn wait_for_label(app: &mut App, window: &Value, label: &str) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(id) = labelled(&tree["nodes"], label) {
            return id;
        }
        assert!(
            Instant::now() < deadline,
            "no {label:?} within {WAIT_MS} ms in {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Polls `state.get document` until its undo label is `label`.
fn wait_for_undo(app: &mut App, window: &Value, label: &str) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let doc = app.call("state.get", json!({"target": "document", "window": window}));
        if doc["undo"].as_str() == Some(label) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "undo is not {label:?} within {WAIT_MS} ms: {doc}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// README steps 87–88 through the channel (plan 4e): a project whose only
/// sequence has a drum lane opens; Open Drum Grid in Window shows its
/// cells as elements; a click on an empty cell paints a hit, a click on
/// it again erases it; Drum > Add Drum Row and Timeline View run by ID.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn drum_grid_paints_and_erases_by_element() {
    use core_engine::fixtures::{Builder, Q};
    let mut app = App::start();
    let mut b = Builder::new();
    let kit = b.dest("Kit", "builtin:synth", 9).unwrap();
    let (seq, drums) = b.sequence("Beat", "Drums", Some(kit)).unwrap();
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38)])
        .unwrap();
    b.notes(seq, drums, &[(0, 36, 0x8000, Q / 4)]).unwrap();
    let path = app.launched.home.join("Beat.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();

    let window = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    // In a window of its own, where the rows have room.
    app.call(
        "command.run",
        json!({"id": "editors.drum_grid_window", "window": window}),
    );
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    let window = loop {
        let w = app.call("state.get", json!({"target": "windows"}));
        let torn = w["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["kind"] == "editor")
            .map(|w| w["number"].clone());
        if let Some(n) = torn {
            break n;
        }
        assert!(Instant::now() < deadline, "no drum grid window in {w}");
        std::thread::sleep(Duration::from_millis(200));
    };

    // The Kick's hit at bar 1 is on; the Snare's first cell is off.
    wait_for_label(&mut app, &window, "Kick, bar 1 beat 1, on, velocity 64");
    let cell = wait_for_label(&mut app, &window, "Snare, bar 1 beat 1, off");
    app.call("ui.click", json!({"window": window, "element": cell}));
    wait_for_undo(&mut app, &window, "Paint Drum Hits");
    let hit = wait_for_label(&mut app, &window, "Snare, bar 1 beat 1, on, velocity 100");
    app.call("ui.click", json!({"window": window, "element": hit}));
    wait_for_undo(&mut app, &window, "Erase Drum Hits");

    // The Drum menu by ID: a row for the next kit note, then the timeline.
    app.call(
        "command.run",
        json!({"id": "drum.add_row", "window": window}),
    );
    wait_for_undo(&mut app, &window, "Add Drum Row");
    wait_for_label(&mut app, &window, "Closed Hat, bar 1 beat 1, off");
    app.call(
        "command.run",
        json!({"id": "drum.timeline", "params": {"on": true}, "window": window}),
    );
    wait_for_label(&mut app, &window, "Kick, bar 1 beat 1, on, velocity 64");
    let tree = app.call("ui.tree", json!({"window": window}));
    assert!(
        labelled(&tree["nodes"], "Closed Hat, bar 1 beat 1, off").is_none(),
        "the timeline lists hits only"
    );
    app.quit();
}

/// Polls `state.get editor` until `ok` holds; returns it.
fn wait_for_editor(
    app: &mut App,
    window: &Value,
    what: &str,
    ok: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let e = app.call("state.get", json!({"target": "editor", "window": window}));
        if ok(&e) {
            return e;
        }
        assert!(Instant::now() < deadline, "{what} within {WAIT_MS} ms: {e}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The transport's tick, read until it is `tick`.
fn wait_for_tick(app: &mut App, window: &Value, tick: i64) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let t = app.call(
            "state.get",
            json!({"target": "transport", "window": window}),
        );
        if t["tick"].as_i64() == Some(tick) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "tick {tick} within {WAIT_MS} ms: {t}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// README steps 99–104 through the channel (plan 4f): keys by the
/// bindings drive step entry (T on, 4 an eighth, a typed chord, 0 a
/// rest, Delete takes steps back), P focuses the Position field and a
/// typed bar moves there, ⌥2 sets a locator and 2 jumps to it, a loaded
/// bindings file moves that key to X, G toggles snap.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn keys_drive_step_entry_locators_and_bindings() {
    use core_engine::fixtures::Builder;
    const Q: i64 = 15_360;
    let mut app = App::start();
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    b.sequence("Tune", "Keys", Some(synth)).unwrap();
    let path = app.launched.home.join("Tune.vizdaw");
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
    // move there; ⌥2 sets locator 2 there.
    wait_for_label(&mut app, &window, "Position");
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

    // A bindings file moves Go to Locator 2 to X.
    let keys = app.launched.home.join("mine.vizkeys");
    std::fs::write(
        &keys,
        r#"{"vizdaw_keys": 1, "bindings": {"locator.go_2": ["X"]}}"#,
    )
    .unwrap();
    app.call(
        "command.run",
        json!({"id": "keys.load", "params": {"path": keys}}),
    );
    key(&mut app, "return", json!([]));
    wait_for_tick(&mut app, &window, 0);
    key(&mut app, "2", json!([]));
    let t = app.call(
        "state.get",
        json!({"target": "transport", "window": window}),
    );
    assert_eq!(t["tick"], 0, "2 no longer goes to the locator");
    key(&mut app, "x", json!([]));
    wait_for_tick(&mut app, &window, 8 * Q);
    app.call("command.run", json!({"id": "keys.reset"}));

    // G toggles snap; the division reads snake_case.
    let e = wait_for_editor(&mut app, &window, "snap on", |e| {
        e["snap"]["enabled"] == true
    });
    assert_eq!(e["snap"]["division"], "quarter");
    key(&mut app, "g", json!([]));
    wait_for_editor(&mut app, &window, "snap off", |e| {
        e["snap"]["enabled"] == false
    });
    app.quit();
}

/// The ID of the tree's button labelled (or titled) exactly "Play".
fn play_button(nodes: &Value) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        let exact = |k: &str| n[k].as_str() == Some("Play");
        if n["role"] == "AXButton" && (exact("label") || exact("title")) {
            Some(n["id"].clone())
        } else {
            play_button(&n["children"])
        }
    })
}
