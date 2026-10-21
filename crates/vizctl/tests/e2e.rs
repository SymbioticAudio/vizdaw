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
        App::start_with(&[])
    }

    /// [`App::start`] with more environment (plan 5c: the fake input).
    fn start_with(extra: &[(&str, &str)]) -> App {
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
        // Media retention moves files here, never to the real Trash (plan 5b).
        let trash = home.join("Trash").display().to_string();
        let launched = launch(&LaunchOptions {
            app,
            home: Some(home),
            demo: false,
            timeout: Duration::from_secs(180),
            env: [
                ("VIZDAW_AUDIO".to_string(), "offline".to_string()),
                ("VIZDAW_TRASH_DIR".to_string(), trash),
            ]
            .into_iter()
            .chain(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())))
            .collect(),
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

/// README steps 109–116 through the channel (plan 5a): a 44.1 kHz/16-bit
/// project stores an imported 48 kHz file in its own format; the demo
/// imports into a new lane, plays the region without disk faults, and
/// changes its format with the media converted.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn audio_import_plays_and_reports_media() {
    let mut app = App::start();
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
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": src}, "window": window}),
    );
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

/// The first node whose label starts with `prefix` and contains `needle`:
/// its ID and label.
fn labelled_like(nodes: &Value, prefix: &str, needle: &str) -> Option<(Value, String)> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        let l = n["label"].as_str().unwrap_or("");
        if l.starts_with(prefix) && l.contains(needle) {
            Some((n["id"].clone(), l.to_string()))
        } else {
            labelled_like(&n["children"], prefix, needle)
        }
    })
}

/// Polls the window's tree until such a node exists.
fn wait_for_label_like(
    app: &mut App,
    window: &Value,
    prefix: &str,
    needle: &str,
) -> (Value, String) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(found) = labelled_like(&tree["nodes"], prefix, needle) {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "no {prefix:?}…{needle:?} within {WAIT_MS} ms in {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// README steps 121, 123–125 and 127 through the channel (plan 5b):
/// a region selected by its element is normalized (hostile levels are
/// refused, a missing one names the dialog); Save As carries the
/// overview cache; an import undone and replaced by another edit goes to
/// the trash folder on save, not before; a moved file is relinked and
/// copied back under a free name; the media pool window opens by ID.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn regions_normalize_relink_and_trash() {
    let mut app = App::start();
    let home = app.launched.home.clone();
    let trash = home.join("Trash");
    // 2 s of stereo at 0.25 (−12.04 dBFS), and a short extra file.
    let tone = home.join("tone.caf");
    audio_file(&tone, 96_000);
    let extra = home.join("extra.caf");
    audio_file(&extra, 4_800);

    // The demo, with the tone in a new lane at bar 1.
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

    // Select the region by its element.
    let (region, label) = wait_for_label_like(&mut app, &w, "Region tone on ", "gain 0.0 dB");
    app.call("ui.click", json!({"window": w, "element": region}));
    let sel = app.call("state.get", json!({"target": "selection", "window": w}));
    assert_eq!(sel["events"].as_array().unwrap().len(), 1, "{label}: {sel}");

    // Normalize: no level names the dialog; nonsense is refused; -1 dB.
    let e = app.call_err(
        "command.run",
        json!({"id": "region.normalize", "window": w}),
    );
    assert_eq!(e.dialog.as_deref(), Some("Normalize"), "{e}");
    for bad in [json!("loud"), json!("nan"), json!("1"), json!(-1)] {
        let e = app.call_err(
            "command.run",
            json!({"id": "region.normalize", "params": {"peak_db": bad}, "window": w}),
        );
        assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{bad}: {e}");
    }
    let r = app.call(
        "command.run",
        json!({"id": "region.normalize", "params": {"peak_db": "-1"}, "window": w}),
    );
    assert_eq!(r["undo"], "Normalize", "{r}");
    // −1 − (−12.04) = +11.04 dB.
    wait_for_label_like(&mut app, &w, "Region tone on ", "gain +11.0 dB");

    // Save As carries the media and the overview cache.
    let pkg = home.join("Regions.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": pkg}, "window": w}),
    );
    assert!(pkg.join("Media/tone.caf").is_file());
    let caches = std::fs::read_dir(pkg.join("Media/.overviews"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "vzov"))
        .count();
    assert_eq!(caches, 1, "the overview came along");

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
    // so none is left lying around.)
    let r = app.call(
        "command.run",
        json!({"id": "media.rename", "params": {"media": media, "name": "Tone"}, "window": w}),
    );
    assert_eq!(r["undo"], "Rename Media", "{r}");
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
    assert_eq!(
        (
            item["name"].clone(),
            item["path"].clone(),
            item["offline"].clone()
        ),
        (json!("Tone"), json!("Media/tone 2.caf"), json!(false)),
        "{m}"
    );

    // The media pool window, by ID.
    app.call(
        "command.run",
        json!({"id": "window.media_pool", "window": w}),
    );
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let ws = app.call("state.get", json!({"target": "windows"}));
        if ws["windows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["kind"] == "media_pool")
        {
            break;
        }
        assert!(Instant::now() < deadline, "no media pool window in {ws}");
        std::thread::sleep(Duration::from_millis(200));
    }
    app.quit();
}

/// Polls `state.get record` until `ok` holds; returns it.
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

/// The lane named `name` in `state.get document`.
fn lane_named(app: &mut App, window: &Value, name: &str) -> Value {
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == name)
        .unwrap_or_else(|| panic!("no lane {name} in {doc}"))
        .clone()
}

/// README steps 130, 134 and 137 through the channel (plan 5c): the offline
/// device's fake input records onto an armed lane as one undo step
/// "Record" with a recorded media item; loop recording with a keep after
/// pass 1 and a discard after pass 2 leaves two new takes; MIDI injected
/// into an armed lane while recording lands as a note. No audio device
/// is opened and no microphone permission is asked.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn records_audio_loop_takes_and_midi() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
    use core_model::ids::AnyId;
    use core_model::lane::{InputChannels, LaneInput, MonitorMode};
    use core_model::sequence::SeqLength;
    use core_model::tempo::{Bpm, TempoMap, TempoPoint};
    use core_model::time::Tick;
    let mut app = App::start_with(&[("VIZDAW_AUDIO_INPUT", "fake")]);
    // One bar at 120 BPM (two seconds, so a keep or discard lands in the
    // pass it means even on a loaded machine): "Keys" (MIDI from source
    // "Keys" on the built-in synth) and "Mic" (fake input 1, heard never).
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
        length: SeqLength::Locked(Tick(4 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Rec.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));

    // Audio: arm Mic (none armed: the last lane), record to the end.
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    assert_eq!(lane_named(&mut app, &w, "Mic")["arm"], true);
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"replace": true, "loop": false, "pre_roll": 0}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == true);
    // The sequence ends after two seconds: the pass ends by itself.
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
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"loop": true, "passes_as_takes": true}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["pass"].as_u64() >= Some(2));
    app.call(
        "command.run",
        json!({"id": "record.keep_pass", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["pass"].as_u64() >= Some(3));
    app.call(
        "command.run",
        json!({"id": "record.discard_pass", "window": w}),
    );
    // Only the total: which pass each press lands on is timing (M14).
    let r = app.call("state.get", json!({"target": "record", "window": w}));
    assert_eq!(
        r["kept"].as_u64().unwrap() + r["discarded"].as_u64().unwrap(),
        2,
        "{r}"
    );
    // Half a second into pass 3, so it is heard (past the output latency)
    // and becomes a take; it lasts two seconds.
    let at = r["frames"].as_u64().unwrap();
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(at + 24_000));
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let lane = lane_named(&mut app, &w, "Mic");
    assert_eq!(
        (lane["takes"].clone(), lane["active_take"].clone()),
        (json!(3), json!("Take 3")),
        "{lane}"
    );

    // MIDI: arm Keys instead, record, play a note in.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"loop": false}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    assert_eq!(lane_named(&mut app, &w, "Keys")["arm"], true);
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| {
        r["recording"] == true && r["frames"].as_u64() >= Some(2_400)
    });
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "90 3c 64"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(9_600));
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "80 3c 00"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    assert_eq!(lane_named(&mut app, &w, "Keys")["events"], 1);
    app.quit();
}
