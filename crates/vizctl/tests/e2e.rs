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
    /// The throwaway home: removed when the app is dropped (declared
    /// last, so the process and the client go first).
    _home: tempfile::TempDir,
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
            .unwrap();
        // Media retention moves files here, never to the real Trash (plan 5b).
        let trash = home.path().join("Trash").display().to_string();
        let launched = launch(&LaunchOptions {
            app,
            home: Some(home.path().to_path_buf()),
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
        App {
            launched,
            client,
            _home: home,
        }
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

/// The ID of the first tree node whose label, title or value is exactly `label`.
fn labelled(nodes: &Value, label: &str) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        if ["label", "title", "value"]
            .iter()
            .any(|k| n[*k].as_str() == Some(label))
        {
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

    // Plan 7c Task 6 review I1: at the default window, with the left pane
    // shown, every control the channel drives in the audio editor (Ruling
    // 10's and the boxes Task 7 types into) lies inside its pane and the
    // window.
    let (region, _) = wait_for_label_like(&mut app, &w2, "Region tone", "");
    app.call("ui.click", json!({"window": w2, "element": region}));
    app.call(
        "command.run",
        json!({"id": "editors.audio_editor", "window": w2}),
    );
    wait_for_label(&mut app, &w2, "Audio Editor Pane");
    let windows = app.call("state.get", json!({"target": "windows"}));
    let win = windows["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["number"] == w2)
        .unwrap()
        .clone();
    let width = win["frame"]["width"].as_f64().unwrap();
    let tree = app.call("ui.tree", json!({"window": w2}));
    let pane = find_node(&tree["nodes"], "Audio Editor Pane").expect("the pane");
    let (px, pw) = (
        pane["frame"]["x"].as_f64().unwrap(),
        pane["frame"]["width"].as_f64().unwrap(),
    );
    assert!(px > 0.0, "the left pane is shown: {pane}");
    for label in [
        "Audio Editor",
        "Region Gain",
        "Fade In Length",
        "Fade Out Length",
        "Region Start",
        "Region Length",
        "Normalize Peak",
        "Normalize",
        "Zoom Out",
        "Zoom In",
        "Fit Region",
        "Shorter Waveform",
        "Taller Waveform",
    ] {
        let n = find_node(&tree["nodes"], label).unwrap_or_else(|| panic!("no {label:?}"));
        let (x, w) = (
            n["frame"]["x"].as_f64().unwrap(),
            n["frame"]["width"].as_f64().unwrap(),
        );
        assert!(
            x >= px - 0.5 && x + w <= px + pw + 0.5 && x + w <= width + 0.5,
            "{label:?} at {x}+{w} is outside the pane {px}+{pw} or the window {width}"
        );
    }

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

/// README steps 140–142 through the channel (plan 5c-2; spec §12.3 "track
/// the fake synth"): Track Synth sets up the return lane in one undo step,
/// Measure finds the fake synth's 25 ms with the transport stopped, and
/// one Record pass lands the synth's clicks on the MIDI notes that played
/// them, within a millisecond. The offline device, its fake input and the
/// fake synth: no hardware, no permission prompt.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn tracks_the_fake_synth() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
    use core_model::event::EventBody;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    // The fake synth brings the signals input by itself.
    let mut app = App::start_with(&[("VIZDAW_FAKE_SYNTH", "25")]);
    // Two bars at 120 BPM: "Part" plays "Fake Synth" at beats 1, 2 and 3.
    let mut b = Builder::new();
    let synth = b.dest("Fake Synth", "Fake Synth", 0).unwrap();
    let (seq, part) = b.sequence("Track", "Part", Some(synth)).unwrap();
    b.notes(
        seq,
        part,
        &[
            (Q, 60, 0x8000, Q / 2),
            (2 * Q, 62, 0x8000, Q / 2),
            (3 * Q, 64, 0x8000, Q / 2),
        ],
    )
    .unwrap();
    // Armed, so Track Synth disarming it is seen (pre-flight M13).
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane: part,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    b.run(Command::Routing(RoutingCmd::AddOutput {
        name: "Audio 1".into(),
        reference: "audio:1-2".into(),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(8 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Synth.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    assert_eq!(lane_named(&mut app, &w, "Part")["arm"], true);

    // Step 140: Track Synth on input 3.
    let r = app.call(
        "command.run",
        json!({"id": "lane.track_synth", "params": {"seq": seq.0, "lane": part.0, "channel": 3}, "window": w}),
    );
    assert_eq!(r["undo"], "Track Synth", "{r}");
    let rec = lane_named(&mut app, &w, "Fake Synth audio");
    assert_eq!(rec["arm"], true, "{rec}");
    assert_eq!(
        rec["input"],
        json!({"kind": "audio", "channel": 3, "stereo": false, "return_of": synth.0})
    );
    assert_eq!(lane_named(&mut app, &w, "Part")["arm"], false);

    // Step 141: Measure.
    let m = app.call(
        "command.run",
        json!({"id": "destination.measure_return", "params": {"dest": synth.0}, "window": w}),
    );
    // The fake synth answers `max(25 ms, block + offset)` late (README
    // 142): 25 ms holds because the offline device renders 512-frame
    // blocks (core-ffi `OFFLINE_BLOCK`, 10.7 ms, under the 1200 frames of
    // 25 ms) and the app's MIDI offset is 0. The offline measurement is
    // exact (1200 frames), so ±0.25 ms leaves the alignment check below
    // to catch its own error (final review findings 7 and 8).
    let ms = m["return_latency_ms"].as_f64().unwrap();
    assert!((ms - 25.0).abs() <= 0.25, "{m}");
    assert_eq!(m["onsets"], 5, "{m}");
    // A destination the document lacks is not external: refused before
    // the core sees it.
    let e = app.call_err(
        "command.run",
        json!({"id": "destination.measure_return", "params": {"dest": 999_999}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");

    // Step 142: one pass to the end of the sequence.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"replace": true, "loop": false, "pre_roll": 0}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == true);
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    app.call("command.run", json!({"id": "file.save", "window": w}));

    // Where each click landed against the note that played it: a sample
    // above 0.3 after at least 5 ms at or below it (the tone stays under
    // 0.1), on the timeline at 24 000 frames a beat.
    let doc = core_model::file::load(&path).unwrap();
    let s = doc.sequence(seq).unwrap();
    let lane = s
        .lanes
        .iter()
        .find(|l| l.name == "Fake Synth audio")
        .unwrap();
    let (tick, media, offset) = lane
        .active()
        .unwrap()
        .events
        .values()
        .find_map(|e| match e.body {
            EventBody::AudioRegion {
                media,
                source_offset,
                ..
            } => Some((e.tick.0, media, source_offset)),
            _ => None,
        })
        .expect("a recorded region");
    let item = doc.media().get(&media).unwrap();
    let reader = core_media::caf::CafReader::open(&path.join(&item.path)).unwrap();
    let info = reader.info().clone();
    assert_eq!(info.channels, 1);
    // The frames below assume 48 kHz (and 120 BPM: 24 000 frames a beat).
    assert_eq!(info.sample_rate, 48_000);
    let mut data = vec![0.0f32; info.frames as usize];
    reader.read(0, &mut data, &mut Vec::new()).unwrap();
    let clicks: Vec<i64> = core_engine::fixtures::clicks(&data, offset as usize, 0.3, 240)
        .into_iter()
        .map(|k| tick * 24_000 / Q + k as i64)
        .collect();
    assert_eq!(clicks.len(), 3, "{clicks:?}");
    for (at, note) in clicks.iter().zip([24_000i64, 48_000, 72_000]) {
        assert!(
            (at - note).abs() <= 48,
            "a click landed at frame {at}, its note at {note}"
        );
    }
    app.quit();
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

/// Plan 5d-1 through the channel (README 5d-2 steps): arming before any
/// Play claims the engine and meters the fake input; `audio.set` changes
/// the buffer size asked for and refuses one that is not offered; a
/// count-in plays before the roll with the timeline standing still; the
/// take's peak grows while it records, and its overview peaks at the fake
/// sine's −12 dBFS after Stop (5c final review M8). The offline device:
/// no hardware, no permission prompt, nothing saved.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn arms_meters_counts_in_and_draws_the_take() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
    use core_model::ids::AnyId;
    use core_model::lane::{InputChannels, LaneInput, MonitorMode};
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let mut app = App::start_with(&[("VIZDAW_AUDIO_INPUT", "fake")]);
    // Eight bars at 120 BPM; lane "Mic" records fake input 1, heard never.
    let mut b = Builder::new();
    let (seq, mic) = b.sequence("Take", "Mic", None).unwrap();
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
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(32 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Arm.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));

    // Armed before any Play: the engine is claimed and the input metered.
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    let input = wait_for_state(&mut app, &w, "input", |i| {
        i["levels"][0].as_f64().is_some_and(|l| l > 0.2)
    });
    assert_eq!(input["channels"], 3, "{input}");
    assert_eq!(
        input["levels"].as_array().map(Vec::len),
        Some(3),
        "a level per device channel: {input}"
    );
    assert_eq!(input["opening"], false, "{input}");
    let t = app.call("state.get", json!({"target": "transport", "window": w}));
    assert_eq!(t["playing"], false, "nothing played: {t}");

    // The buffer size: one of six, as asked (the offline device keeps its
    // 512-frame blocks).
    app.call(
        "command.run",
        json!({"id": "audio.set", "params": {"buffer_frames": 256}, "window": w}),
    );
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(
        (
            input["buffer_frames"].clone(),
            input["device_buffer_frames"].clone()
        ),
        (json!(256), json!(512)),
        "{input}"
    );
    let e = app.call_err(
        "command.run",
        json!({"id": "audio.set", "params": {"buffer_frames": 100}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");
    let e = app.call_err(
        "command.run",
        json!({"id": "audio.set", "params": {"junk": 1}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");

    // One bar of count-in: the timeline stands still, nothing records.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"count_in": 1, "loop": false, "replace": true}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    let t = wait_for_state(&mut app, &w, "transport", |t| t["counting_in"] == true);
    assert_eq!(t["playing"], false, "{t}");
    assert_eq!(t["tick"], json!(0), "the timeline stands at the start: {t}");
    let r = app.call("state.get", json!({"target": "record", "window": w}));
    assert_eq!(
        (r["recording"].clone(), r["frames"].clone()),
        (json!(true), json!(0)),
        "{r}"
    );
    // Then it rolls; the take's peak grows to the sine's.
    let r = wait_for_record(&mut app, &w, |r| {
        r["frames"].as_u64() >= Some(48_000) && r["lanes"][0]["peak"].as_f64() > Some(0.2)
    });
    assert!(
        (r["lanes"][0]["peak"].as_f64().unwrap() - 0.251).abs() < 0.01,
        "{r}"
    );
    assert_eq!(r["pass_start_tick"], json!(0), "{r}");
    // The buffer size stays while recording: busy, and nothing changes.
    let e = app.call_err(
        "command.run",
        json!({"id": "audio.set", "params": {"buffer_frames": 512}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::Busy, "{e}");
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(input["buffer_frames"], json!(256), "{input}");
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let m = wait_for_state(&mut app, &w, "media", |m| !m["items"][0]["peak"].is_null());
    assert!(
        (m["items"][0]["peak"].as_f64().unwrap() - 0.251).abs() < 0.01,
        "{m}"
    );
    app.quit();
}

/// Spec §12.3 through the channel (plan 5e-1): with the Built-in Synth
/// strip on Touch, a fader touched, moved and released while playing
/// lands at Stop as one "Write Automation" step in a lane "Built-in Synth
/// Fader"; hostile gestures are refused before anything changes.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn touch_writes_a_fader_over_the_channel() {
    let mut app = App::start();
    let w = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
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
    // Hostile input: refused, nothing touched.
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
    // Pre-flight C1 (Ruling 24): the first idle after Play must not end
    // the pass.
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

/// Plan 5f-1 Task 11 (spec §12.3): an MCU-compatible unit added and fed
/// through the channel (`surface.inject`, no CoreMIDI port: the offline
/// app uses the fake port transport) touches, moves and releases fader 1
/// as one "Set Fader" step, mutes strip 1, gets the ride's value back once
/// after the release and the mute LED, ignores and counts a cut message.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn drives_a_fader_and_a_button_from_an_mcu() {
    let mut app = App::start();
    // The launch window adopts an empty document (`documentForWindow`):
    // close it first, so the call below has no document to answer for.
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let ws = app.call("state.get", json!({"target": "windows"}));
        if let Some(d) = ws["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["kind"] == "document")
        {
            let n = d["number"].clone();
            app.call("command.run", json!({"id": "file.close", "window": n}));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no launch document window in {ws}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // Pre-flight M29: surfaces are app-wide, so `state.get surfaces` answers without a document.
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    let st = loop {
        let st = app.call("state.get", json!({"target": "surfaces"}));
        if st["doc"].is_null() {
            break st;
        }
        assert!(
            Instant::now() < deadline,
            "the launch document closes: {st}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        st["feedback"].as_array().is_some_and(|f| f.is_empty()),
        "{st}"
    );
    let w = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        )
    };
    for (params, code) in [
        (
            json!({"type": "mcu", "input": "", "output": "E2E Out"}),
            core_control::ErrorCode::Failed,
        ),
        (
            json!({"type": "nope", "input": "a", "output": "b"}),
            core_control::ErrorCode::InvalidParams,
        ),
    ] {
        let e = app.call_err(
            "command.run",
            json!({"id": "surface.add", "params": params, "window": w}),
        );
        assert_eq!(e.code, code, "{e}");
    }
    run(
        &mut app,
        "surface.add",
        json!({"type": "mcu", "input": "E2E In", "output": "E2E Out"}),
    );
    let st = app.call("state.get", json!({"target": "surfaces", "window": w}));
    assert_eq!(st["surfaces"][0]["connected"], true, "{st}");
    let id = st["surfaces"][0]["id"].as_u64().unwrap();
    let auto = app.call("state.get", json!({"target": "automation", "window": w}));
    let synth = auto["modes"][0]["strip"].as_u64().unwrap();
    // The synth is the demo's first strip: fader 1, mute 1.
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "90 68 7f e0 00 20"}),
    );
    let auto = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(auto["touched"][0], format!("fader:{synth}"), "{auto}");
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "90 68 00"}),
    );
    wait_for_state(&mut app, &w, "document", |d| d["undo"] == "Set Fader");
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "90 10 7f"}),
    );
    let st = wait_for_state(&mut app, &w, "surfaces", |s| {
        s["feedback"][0]["messages"]
            .as_array()
            .is_some_and(|m| m.iter().any(|x| x == "90 10 7f"))
    });
    let messages = st["feedback"][0]["messages"].as_array().unwrap();
    assert!(
        messages.iter().any(|x| x == "e0 00 20"),
        "the fader got the ride's value (0.25) once, after the release: {st}"
    );
    assert!(
        messages
            .iter()
            .all(|x| x.as_str().is_some_and(|t| !t.starts_with("d0 "))),
        "meters are counted, not listed"
    );
    let ignored = st["surfaces"][0]["ignored"].as_u64().unwrap();
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "90 68"}),
    );
    let st = app.call("state.get", json!({"target": "surfaces", "window": w}));
    assert_eq!(
        st["surfaces"][0]["ignored"].as_u64(),
        Some(ignored + 1),
        "a cut message is ignored and counted: {st}"
    );
    let e = app.call_err(
        "command.run",
        json!({"id": "surface.inject", "params": {"surface": id, "bytes": "zz"}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams);
    run(&mut app, "surface.remove", json!({"surface": id}));
    app.quit();
}

/// Plan 5f-4 Task 8 (spec §8.4): a Kontrol S-Series MK3 added and fed
/// through the channel (offline: the fake port transport) answers the
/// handshake and gets its display, rides fader 1 from knob 1 as one "Set
/// Fader" step, starts and stops the transport, sets the tempo as one "Set
/// Tempo" step (owner, 2026-11-03), and counts a cut SysEx.
/// (`messages` keeps the last 64 sent, so the identity at the head of the
/// first refresh may have scrolled out: the test waits for any NI SysEx;
/// `core-ffi/tests/kontrol.rs` pins the identity.)
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn drives_the_mixer_from_a_kontrol_mk3() {
    let mut app = App::start();
    let w = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        )
    };
    let port = "KONTROL S88 MK3 DAW";
    run(
        &mut app,
        "surface.add",
        json!({"type": "kontrol_mk3", "input": port, "output": port}),
    );
    let st = app.call("state.get", json!({"target": "surfaces", "window": w}));
    assert_eq!(st["surfaces"][0]["type"], "kontrol_mk3", "{st}");
    assert_eq!(st["surfaces"][0]["online"], false, "{st}");
    let id = st["surfaces"][0]["id"].as_u64().unwrap();
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "bf 01 04"}),
    );
    let ni = "f0 00 21 09 00 00 44 43 01 00";
    let st = wait_for_state(&mut app, &w, "surfaces", |s| {
        s["feedback"][0]["messages"].as_array().is_some_and(|m| {
            m.iter()
                .any(|x| x.as_str().is_some_and(|t| t.starts_with(ni)))
        })
    });
    assert_eq!(st["surfaces"][0]["online"], true, "{st}");
    let auto = app.call("state.get", json!({"target": "automation", "window": w}));
    let synth = auto["modes"][0]["strip"].as_u64().unwrap();
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "bf 50 05"}),
    );
    wait_for_state(&mut app, &w, "automation", |a| {
        a["touched"][0] == format!("fader:{synth}")
    });
    wait_for_state(&mut app, &w, "document", |d| d["undo"] == "Set Fader");
    // The ride ends 500 ms after the last turn; the tempo is refused while
    // its edit is open.
    wait_for_state(&mut app, &w, "automation", |a| {
        a["touched"].as_array().is_some_and(|t| t.is_empty())
    });
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "bf 10 01"}),
    );
    app.wait(&w, json!({"condition": "playing"}));
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "bf 14 01"}),
    );
    app.wait(&w, json!({"condition": "stopped"}));
    // The keyboard's tempo control: 100 BPM is 60 000 000 × 10 ns, sent as
    // 00 0e 4e 1c 00 (Ruling 5).
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "f0 00 21 09 00 00 44 43 01 00 19 00 00 00 0e 4e 1c 00 f7"}),
    );
    wait_for_state(&mut app, &w, "document", |d| d["undo"] == "Set Tempo");
    // VizDAW plays 100 BPM: no state target carries the tempo, but the
    // keyboard is sent VizDAW's own tempo after its turn (final review T8
    // M1), so the last tempo in its log is 100's.
    let tempo = format!("{ni} 19 ");
    wait_for_state(&mut app, &w, "surfaces", |s| {
        s["feedback"][0]["messages"].as_array().is_some_and(|m| {
            m.iter()
                .rev()
                .filter_map(|x| x.as_str())
                .find(|t| t.starts_with(&tempo))
                == Some("f0 00 21 09 00 00 44 43 01 00 19 00 00 00 0e 4e 1c 00 f7")
        })
    });
    let st = app.call("state.get", json!({"target": "surfaces", "window": w}));
    let ignored = st["surfaces"][0]["ignored"].as_u64().unwrap();
    run(
        &mut app,
        "surface.inject",
        json!({"surface": id, "bytes": "f0 00 21 09 00 00 44 43 01 00 48"}),
    );
    let st = app.call("state.get", json!({"target": "surfaces", "window": w}));
    assert_eq!(
        st["surfaces"][0]["ignored"].as_u64(),
        Some(ignored + 1),
        "a cut SysEx is ignored and counted: {st}"
    );
    run(&mut app, "surface.remove", json!({"surface": id}));
    app.quit();
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

/// README steps 180–184 through the channel (plan 5f-2; spec §12.3 "bounce
/// and check the file"): a bounce of four bars to a 16-bit WAV, refused
/// over the same file without `replace`, then Add to project; hostile
/// calls refused.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn bounces_the_demo_and_checks_the_file() {
    let mut app = App::start();
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

/// A 48 kHz mono 16-bit WAV of `frames` frames at `level`: an editor's
/// save (plan 5f-3).
fn wav(path: &std::path::Path, frames: usize, level: f32) {
    let fmt = core_media::export::ExportFormat {
        container: core_media::export::Container::Wav,
        depth: core_model::format::BitDepth::Int16,
        rate: 48_000,
        dither: false,
        dither_stream: 0,
    };
    let mut w = core_media::export::PcmWriter::create(path, &fmt, 1).unwrap();
    w.write(&vec![level; frames]).unwrap();
    w.finish().unwrap();
}

/// README steps 185–188 through the channel (plan 5f-3; spec §11, §12.1):
/// with `VIZDAW_EXTERNAL_EDITOR=stub`, which launches nothing, a region of
/// an imported tone opens in the "editor"; the test saves the copy as an
/// editor that replaces its file would, and the region plays the edit as
/// one "External Edit" step; a second save in place, on the file the
/// rename put there, lands too, which pins the watch re-armed on the
/// replaced file (pre-flight I8); Reload with nothing changed is
/// "unchanged"; hostile calls are refused.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn edits_a_region_through_the_stub_editor() {
    let mut app = App::start_with(&[("VIZDAW_EXTERNAL_EDITOR", "stub")]);
    let home = app.launched.home.clone();
    let tone = home.join("tone.caf");
    audio_file(&tone, 96_000);
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();

    // 1. Nothing selected: refused; a relative `app`: refused by type.
    let e = app.call_err(
        "command.run",
        json!({"id": "region.open_external", "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::Unavailable, "{e}");
    let e = app.call_err(
        "command.run",
        json!({"id": "region.open_external", "params": {"app": "relative.app"}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");

    // 2. The tone at bar 1 (the demo's own audio is offline: pre-flight
    // C1), its region selected by its element.
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": tone, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let (region, label) = wait_for_label_like(&mut app, &w, "Region tone on ", "gain 0.0 dB");
    app.call("ui.click", json!({"window": w, "element": region}));
    let sel = app.call("state.get", json!({"target": "selection", "window": w}));
    assert_eq!(sel["events"].as_array().unwrap().len(), 1, "{label}: {sel}");

    // 3. Open: the copy's path, in the scratch package in the home.
    let r = app.call(
        "command.run",
        json!({"id": "region.open_external", "window": w}),
    );
    let status = r["status"].as_str().unwrap_or("").to_string();
    let copy = PathBuf::from(
        status
            .strip_prefix("editing ")
            .unwrap_or_else(|| panic!("{r}")),
    );
    assert!(
        copy.starts_with(&home) && copy.ends_with("Media/Edits/tone edit 1.wav"),
        "{r}"
    );
    assert!(copy.is_file(), "{}", copy.display());
    let r = app.call(
        "command.run",
        json!({"id": "region.reload_external", "window": w}),
    );
    assert_eq!(
        r["status"], "unchanged",
        "right after Open (pre-flight I10): {r}"
    );

    // 4. The editor saves by replacing the file (pre-flight M11).
    let tmp = copy.with_file_name(".tone edit 1.tmp.wav");
    wav(&tmp, 24_000, 0.5);
    std::fs::rename(&tmp, &copy).unwrap();
    wait_for_undo(&mut app, &w, "External Edit");
    let edited = |m: &Value, frames: u64| {
        m["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["origin"] == "external_edit" && i["frames"] == frames)
    };
    wait_for_state(&mut app, &w, "media", |m| edited(m, 24_000));

    // 5. A second save, in place on the file the rename put there: it
    // lands only if the watch followed the rename (pre-flight I8).
    let bytes = {
        let t = home.join("short.wav");
        wav(&t, 12_000, 0.25);
        std::fs::read(&t).unwrap()
    };
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&copy)
            .unwrap();
        f.write_all(&bytes).unwrap();
        f.sync_all().unwrap();
    }
    let m = wait_for_state(&mut app, &w, "media", |m| edited(m, 12_000));
    let edits = m["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["origin"] == "external_edit")
        .count();
    assert_eq!(edits, 2, "one import per save: {m}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(doc["undo"], "External Edit", "{doc}");

    // 6. Reload with nothing changed since; an `app` that is no application.
    let r = app.call(
        "command.run",
        json!({"id": "region.reload_external", "window": w}),
    );
    assert_eq!(r["status"], "unchanged", "{r}");
    let e = app.call_err(
        "command.run",
        json!({"id": "region.open_external", "params": {"app": "/Applications/Nope-VizDAW-e2e.app"}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::Failed, "{e}");
    app.quit();
}

/// One pane of one main view in a `state.get layout` answer.
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

/// Polls `state.get layout` until `ok` holds; returns that answer.
fn wait_for_layout(app: &mut App, window: &Value, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let l = app.call("state.get", json!({"target": "layout", "window": window}));
        if ok(&l) {
            return l;
        }
        assert!(
            Instant::now() < deadline,
            "the layout did not settle within {WAIT_MS} ms: {l}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// README steps 199–206 through the channel (plan 7a): the defaults,
/// hostile `ui.pane` calls that change nothing and open no dialog, the
/// main-view keys, a pane set for the other view, the mixer in the bottom
/// pane and in its own window (with its own transport bar), and the
/// layout saved per document.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn window_views_and_panes_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
    // 1. The defaults.
    let window = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
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
    // The transport bar's controls are in the tree (final review I1): an
    // agent finds Loop, Record and the pane buttons, not only Play.
    for label in [
        "Loop",
        "Record",
        "Record Options",
        "Left Pane",
        "Bottom Pane",
        "Right Pane",
    ] {
        wait_for_label(&mut app, &window, label);
    }

    // 2. Hostile calls change nothing and open no dialog.
    let w0 = app.call("state.get", json!({"target": "windows"}));
    let numbers = |w: &Value| -> Vec<(Value, Value)> {
        w["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| (x["number"].clone(), x["kind"].clone()))
            .collect()
    };
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
    let w1 = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(numbers(&w1), numbers(&w0), "no window opened");
    assert_eq!(w1["alerts"].as_array().unwrap().len(), 0, "{w1}");
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    assert!(doc["alert"].is_null(), "no dialog: {doc}");

    // 3. ⌃2 and ⌃1.
    let tree_has = |app: &mut App, label: &str| {
        let tree = app.call("ui.tree", json!({"window": window}));
        labelled(&tree["nodes"], label).is_some()
    };
    app.call(
        "ui.key",
        json!({"window": window, "key": "2", "modifiers": ["control"]}),
    );
    wait_for_layout(&mut app, &window, |l| l["main"] == "mix");
    // Only the Mix view's mixer has the Instruments menu: once it is in the
    // tree the view has switched, so the absent Arrangement means something.
    wait_for_label(&mut app, &window, "Instruments");
    assert!(
        !tree_has(&mut app, "Arrangement"),
        "the Mix view has no arrangement"
    );
    app.call(
        "ui.key",
        json!({"window": window, "key": "1", "modifiers": ["control"]}),
    );
    wait_for_layout(&mut app, &window, |l| l["main"] == "arrange");
    wait_for_label(&mut app, &window, "Arrangement");
    app.call(
        "command.run",
        json!({"id": "ui.toggle_right", "window": window}),
    );
    let l = wait_for_layout(&mut app, &window, |l| {
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
    // The Mix view's right pane took it at once (Task 9 review M1).
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
    // The mixer bar's Add Bus is an AppKit button, in the tree (Task 9
    // fix round 1 finding, final review I1).
    wait_for_label(&mut app, &window, "Add Bus");

    // 6. The mixer window has its own transport bar.
    app.call(
        "command.run",
        json!({"id": "window.mixer", "window": window}),
    );
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    let mixer = loop {
        let ws = app.call("state.get", json!({"target": "windows"}));
        if let Some(m) = ws["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["kind"] == "mixer")
        {
            break m["number"].clone();
        }
        assert!(Instant::now() < deadline, "no mixer window in {ws}");
        std::thread::sleep(Duration::from_millis(200));
    };
    let id = loop {
        let tree = app.call("ui.tree", json!({"window": mixer}));
        if let Some(id) = play_button(&tree["nodes"]) {
            break id;
        }
        assert!(
            Instant::now() < deadline,
            "no Play in the mixer window's tree: {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    app.call("ui.click", json!({"window": mixer, "element": id}));
    app.wait(&window, json!({"condition": "playing"}));
    app.call(
        "command.run",
        json!({"id": "transport.play_stop", "window": window}),
    );
    app.wait(&window, json!({"condition": "stopped"}));

    // 7. Saved per document.
    let path = app.launched.home.join("views.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": path}, "window": window}),
    );
    app.call("command.run", json!({"id": "file.close", "window": window}));
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

/// The strip named `name` in a `state.get mixer` answer.
fn strip_named(mixer: &Value, name: &str) -> Value {
    mixer["strips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no strip {name:?} in {mixer}"))
        .clone()
}

/// README steps 207–216 through the channel (plan 7b): hostile `ui.theme`
/// and `state.get` calls that change nothing and open no dialog, a theme
/// chosen live, the mixer state's colours and EQ slot, the typed level
/// and pan on the real number boxes (Return commits, a refusal and Esc
/// change nothing, a 300-digit entry is refused, not cut), the mixer
/// window's strips, and the values saved with the project while the
/// theme stays a preference.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn themes_and_the_strip_over_the_channel() {
    use core_control::ErrorCode;
    const SYNTH: &str = "Built-in Synth";
    let mut app = App::start();
    // 1. The default theme; the windows as they are.
    let window = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    let numbers = |w: &Value| -> Vec<(Value, Value)> {
        w["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| (x["number"].clone(), x["kind"].clone()))
            .collect()
    };
    let w0 = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(w0["theme"], "classic-light", "{w0}");

    // 2. Hostile calls change nothing and open no dialog.
    for params in [
        json!({}),
        json!({"theme": "svp-light"}),
        json!({"theme": "console", "extra": 1}),
    ] {
        let e = app.call_err(
            "command.run",
            json!({"id": "ui.theme", "params": params, "window": window}),
        );
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}: {e}");
    }
    let e = app.call_err("state.get", json!({"target": "mixers", "window": window}));
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    let w1 = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(w1["theme"], "classic-light", "{w1}");
    assert_eq!(numbers(&w1), numbers(&w0), "no window opened or closed");
    assert_eq!(w1["alerts"].as_array().unwrap().len(), 0, "{w1}");
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    assert!(doc["alert"].is_null(), "no dialog: {doc}");

    // 3. Console, live; no window opens or closes.
    let r = app.call(
        "command.run",
        json!({"id": "ui.theme", "params": {"theme": "console"}, "window": window}),
    );
    assert_eq!(r["status"], "theme console", "{r}");
    let w2 = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(w2["theme"], "console", "{w2}");
    assert_eq!(numbers(&w2), numbers(&w0), "no window opened or closed");

    // 3b. Nothing is selected: the left pane says so and shows no strip.
    wait_for_label(&mut app, &window, "Select a lane to see its channel strip.");
    let tree = app.call("ui.tree", json!({"window": window}));
    assert!(
        labelled_like(&tree["nodes"], "Level of ", "").is_none(),
        "no strip in the left pane: {tree}"
    );
    app.call(
        "command.run",
        json!({"id": "ui.view_mix", "window": window}),
    );
    wait_for_state(&mut app, &window, "layout", |l| l["main"] == "mix");

    // 4. The mixer state: kind colours, no strip colours, no EQ.
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    for s in m["strips"].as_array().unwrap() {
        // B8 review m4: a kind colour that is a name, and a colour key that
        // is there and null (not missing).
        assert!(
            s["kind_colour"].as_str().is_some_and(|k| !k.is_empty()),
            "{s}"
        );
        assert_eq!(
            s.get("colour"),
            Some(&Value::Null),
            "no strip colour yet: {s}"
        );
        assert!(s["eq"].is_null(), "no EQ yet: {s}");
    }
    assert_eq!(strip_named(&m, SYNTH)["kind_colour"], "instrument", "{m}");
    assert_eq!(strip_named(&m, "Master")["kind_colour"], "master", "{m}");
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(eq) = find_node(&tree["nodes"], &format!("EQ of {SYNTH}")) {
            assert_eq!(eq["value"], "empty", "{eq}");
            break;
        }
        assert!(Instant::now() < deadline, "no EQ of {SYNTH} in {tree}");
        std::thread::sleep(Duration::from_millis(200));
    }

    // 5. The level box: one call clicks it and types.
    let level = wait_for_label(&mut app, &window, &format!("Level of {SYNTH}"));
    app.call(
        "ui.key",
        json!({"window": window, "element": level, "text": "-4.2\n"}),
    );
    wait_for_state(&mut app, &window, "mixer", |m| {
        (strip_named(m, SYNTH)["gain_db"].as_f64().unwrap() + 4.2).abs() < 1e-6
    });
    wait_for_undo(&mut app, &window, "Set Fader");
    let version = |app: &mut App| {
        app.call("state.get", json!({"target": "document", "window": window}))["version"].clone()
    };
    let v0 = version(&mut app);

    // 5b. Esc while typing changes nothing (A4 review M4).
    let level = wait_for_label(&mut app, &window, &format!("Level of {SYNTH}"));
    app.call(
        "ui.key",
        json!({"window": window, "element": level, "text": "-9"}),
    );
    // B8 review m2: the box is editing the typed draft before Esc (its
    // field has the keyboard; the node's value stays the committed level
    // until Return), so the Esc below is not tested vacuously.
    let box_node = |app: &mut App| {
        find_node(
            &app.call("ui.tree", json!({"window": window}))["nodes"],
            &format!("Level of {SYNTH}"),
        )
        .unwrap()
    };
    let editing = box_node(&mut app);
    assert_eq!(editing["focused"], true, "the box is editing -9: {editing}");
    app.call("ui.key", json!({"window": window, "key": "escape"}));
    let after = box_node(&mut app);
    assert!(after["focused"].is_null(), "Esc ended the edit: {after}");
    // Had Esc not ended the edit, this Return would commit −9.
    app.call("ui.key", json!({"window": window, "key": "return"}));
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    assert!(
        (strip_named(&m, SYNTH)["gain_db"].as_f64().unwrap() + 4.2).abs() < 1e-6,
        "Esc cancelled the typed -9: {m}"
    );
    assert_eq!(version(&mut app), v0, "Esc added no undo step");
    // 5c. A 300-digit entry (the channel's paste) is refused, not cut. A
    // `ui.key` holds at most 256 characters: two calls into the one field.
    let level = wait_for_label(&mut app, &window, &format!("Level of {SYNTH}"));
    let half = "9".repeat(150);
    app.call(
        "ui.key",
        json!({"window": window, "element": level, "text": half}),
    );
    app.call(
        "ui.key",
        json!({"window": window, "text": format!("{half}\n")}),
    );
    wait_for_state(&mut app, &window, "editor", |e| {
        e["status"] == "Type a level in dB, like \u{2212}4.2 or \u{2212}inf."
    });
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    assert!(
        (strip_named(&m, SYNTH)["gain_db"].as_f64().unwrap() + 4.2).abs() < 1e-6,
        "Esc and the refusal changed nothing: {m}"
    );
    assert_eq!(version(&mut app), v0, "no undo step was added");
    wait_for_undo(&mut app, &window, "Set Fader");

    // 6. The pan box: a refused pan, then L 24.
    let pan = wait_for_label(&mut app, &window, &format!("Pan of {SYNTH}"));
    app.call("ui.click", json!({"window": window, "element": pan}));
    app.call("ui.key", json!({"window": window, "text": "LR\n"}));
    wait_for_state(&mut app, &window, "editor", |e| {
        e["status"] == "Type a pan like L 24, C or R 10."
    });
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    assert_eq!(strip_named(&m, SYNTH)["pan"].as_f64(), Some(0.0), "{m}");
    let pan = wait_for_label(&mut app, &window, &format!("Pan of {SYNTH}"));
    app.call(
        "ui.key",
        json!({"window": window, "element": pan, "text": "L 24\n"}),
    );
    wait_for_state(&mut app, &window, "mixer", |m| {
        (strip_named(m, SYNTH)["pan"].as_f64().unwrap() + 0.24).abs() < 1e-6
    });
    // Leaving the level box committed no draft: Esc had cancelled it (and
    // since symbiotic-design 0.2.1 a blur cancels too).
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    assert!(
        (strip_named(&m, SYNTH)["gain_db"].as_f64().unwrap() + 4.2).abs() < 1e-6,
        "{m}"
    );

    // 7. The mixer window shows the same strips.
    app.call(
        "command.run",
        json!({"id": "window.mixer", "window": window}),
    );
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    let mixer = loop {
        let ws = app.call("state.get", json!({"target": "windows"}));
        if let Some(m) = ws["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["kind"] == "mixer")
        {
            break m["number"].clone();
        }
        assert!(Instant::now() < deadline, "no mixer window in {ws}");
        std::thread::sleep(Duration::from_millis(200));
    };
    wait_for_label(&mut app, &mixer, &format!("Fader of {SYNTH}"));

    // 8. The values are saved; the theme is a preference.
    let path = app.launched.home.join("strip.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": path}, "window": window}),
    );
    app.call("command.run", json!({"id": "file.close", "window": window}));
    let w3 = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w3, json!({"condition": "documents_at_least", "count": 1}));
    let m = app.call("state.get", json!({"target": "mixer", "window": w3}));
    let s = strip_named(&m, SYNTH);
    assert!((s["gain_db"].as_f64().unwrap() + 4.2).abs() < 1e-6, "{m}");
    assert!((s["pan"].as_f64().unwrap() + 0.24).abs() < 1e-6, "{m}");
    let w4 = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(w4["theme"], "console", "{w4}");
    app.quit();
}

/// Whether `label` holds the whole word `word` (split on non-letters, so
/// "Pane" and "Panel" do not count; Ruling 11, no regex crate).
fn has_word(label: &str, word: &str) -> bool {
    label.split(|c: char| !c.is_alphabetic()).any(|w| w == word)
}

/// Every label and title in `nodes` and their subtrees holding `word`.
fn labels_with_word(nodes: &Value, word: &str, out: &mut Vec<String>) {
    for n in nodes.as_array().into_iter().flatten() {
        for k in ["label", "title"] {
            if let Some(l) = n[k].as_str().filter(|l| has_word(l, word)) {
                out.push(l.to_string());
            }
        }
        labels_with_word(&n["children"], word, out);
    }
}

/// The last edit is one undo step named `label` (Task 7 review M1): one
/// Undo gives `undone` and the undo label `prev`, one Redo gives `redone`
/// and `label` again.
fn one_undo_step(
    app: &mut App,
    w: &Value,
    label: &str,
    prev: &str,
    undone: impl Fn(&Value) -> bool,
    redone: impl Fn(&Value) -> bool,
) {
    wait_for_undo(app, w, label);
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_state(app, w, "audio_editor", undone);
    wait_for_undo(app, w, prev);
    app.call("command.run", json!({"id": "edit.redo", "window": w}));
    wait_for_state(app, w, "audio_editor", redone);
    wait_for_undo(app, w, label);
}

/// The node labelled `label` inside the "Audio Editor Pane" container
/// (the arrangement has zoom buttons of its own).
fn in_editor(app: &mut App, window: &Value, label: &str) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(n) = find_node(&tree["nodes"], "Audio Editor Pane")
            .and_then(|pane| find_node(&pane["children"], label))
        {
            return n;
        }
        assert!(
            Instant::now() < deadline,
            "no {label:?} in the Audio Editor Pane within {WAIT_MS} ms: {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// README steps 217–224 through the channel (plan 7c): hostile calls that
/// change nothing, ⌘5 on the selected region, no pan in the editor, the
/// typed gain, fade, length and start, a gain drag, Normalize and its
/// undo, a zoom, and the edits and the bottom view saved.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn edits_a_region_in_the_audio_editor() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let home = app.launched.home.clone();
    // 1. 2 s of stereo at 0.25 (−12.04 dBFS) at bar 1 of the demo, its
    // region selected by its element.
    let tone = home.join("tone.caf");
    audio_file(&tone, 96_000);
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": tone, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let (region, label) = wait_for_label_like(&mut app, &w, "Region tone on ", "gain 0.0 dB");
    app.call("ui.click", json!({"window": w, "element": region}));
    let sel = app.call("state.get", json!({"target": "selection", "window": w}));
    assert_eq!(sel["events"].as_array().unwrap().len(), 1, "{label}: {sel}");
    let event = sel["events"][0].clone();

    // 2. Hostile calls change nothing and open no dialog.
    let layout =
        |app: &mut App, w: &Value| app.call("state.get", json!({"target": "layout", "window": w}));
    let l0 = layout(&mut app, &w);
    let e = app.call_err("state.get", json!({"target": "audio-editor", "window": w}));
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    let e = app.call_err(
        "command.run",
        json!({"id": "ui.pane", "params": {"pane": "left", "view": "audio_editor"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    assert_eq!(
        e.message, "audio_editor is not on the left pane's list in the arrange view.",
        "{e}"
    );
    let e = app.call_err(
        "command.run",
        json!({"id": "ui.pane", "params": {"pane": "bottom", "view": "audio"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    assert_eq!(layout(&mut app, &w), l0, "no layout change");
    let ws = app.call("state.get", json!({"target": "windows"}));
    assert_eq!(ws["alerts"].as_array().unwrap().len(), 0, "{ws}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert!(doc["alert"].is_null(), "no dialog: {doc}");

    // 3. ⌘5: the bottom pane shows the editor on the region.
    app.call(
        "command.run",
        json!({"id": "editors.audio_editor", "window": w}),
    );
    wait_for_layout(&mut app, &w, |l| {
        let b = layout_pane(l, "arrange", "bottom");
        b["view"] == "audio_editor" && b["shown"] == true
    });
    let s = wait_for_state(&mut app, &w, "audio_editor", |s| {
        s["shown"] == true && s["region"]["event"] == event
    });
    assert_eq!(
        (
            s["boxes"]["gain"].clone(),
            s["boxes"]["start"].clone(),
            s["boxes"]["length"].clone()
        ),
        (json!("0.0 dB"), json!("0:00.000"), json!("0:02.000")),
        "{s}"
    );
    // No pan anywhere in the editor (Ruling 11): "Bottom Pane" and the
    // left strip's "Pan of …" lie outside its container.
    in_editor(&mut app, &w, "Audio Editor");
    let tree = app.call("ui.tree", json!({"window": w}));
    let root = find_node(&tree["nodes"], "Audio Editor Pane").expect("the pane");
    let mut pans = Vec::new();
    labels_with_word(&json!([root]), "Pan", &mut pans);
    assert!(pans.is_empty(), "pan in the audio editor: {pans:?}");

    let region_of = |s: &Value| s["region"].clone();
    let gain = |s: &Value| s["region"]["gain_db"].as_f64().unwrap_or(f64::NAN);
    let state = |app: &mut App, w: &Value| {
        app.call("state.get", json!({"target": "audio_editor", "window": w}))
    };

    // 4. The typed gain; a refused one changes nothing.
    let id = in_editor(&mut app, &w, "Region Gain")["id"].clone();
    app.call(
        "ui.key",
        json!({"window": w, "element": id, "text": "-4.2\n"}),
    );
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        (gain(s) + 4.2).abs() < 1e-6
    });
    wait_for_undo(&mut app, &w, "Set Region Gain");
    let id = in_editor(&mut app, &w, "Region Gain")["id"].clone();
    app.call(
        "ui.key",
        json!({"window": w, "element": id, "text": "abc\n"}),
    );
    wait_for_state(&mut app, &w, "editor", |e| {
        e["status"] == "Type a gain in dB, like \u{2212}4.2 or +6."
    });
    let s = state(&mut app, &w);
    assert!((gain(&s) + 4.2).abs() < 1e-6, "{s}");

    // 5. The fade in, in ms: 250 ms at 48 kHz.
    let id = in_editor(&mut app, &w, "Fade In Length")["id"].clone();
    app.call(
        "ui.key",
        json!({"window": w, "element": id, "text": "250\n"}),
    );
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        region_of(s)["fade_in"]["frames"] == 12_000
    });
    one_undo_step(
        &mut app,
        &w,
        "Set Region Fades",
        "Set Region Gain",
        |s| region_of(s)["fade_in"]["frames"] == 0,
        |s| region_of(s)["fade_in"]["frames"] == 12_000,
    );

    // 6. The length, then the start: the end stays.
    let id = in_editor(&mut app, &w, "Region Length")["id"].clone();
    app.call(
        "ui.key",
        json!({"window": w, "element": id, "text": "1.5\n"}),
    );
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        region_of(s)["length"] == 72_000
    });
    one_undo_step(
        &mut app,
        &w,
        "Trim Region",
        "Set Region Fades",
        |s| region_of(s)["length"] == 96_000,
        |s| region_of(s)["length"] == 72_000,
    );
    let id = in_editor(&mut app, &w, "Region Start")["id"].clone();
    app.call(
        "ui.key",
        json!({"window": w, "element": id, "text": "0.5\n"}),
    );
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        region_of(s)["source_offset"] == 24_000 && region_of(s)["length"] == 48_000
    });
    // One step: a start sent as two edits would undo to a third state.
    one_undo_step(
        &mut app,
        &w,
        "Trim Region",
        "Trim Region",
        |s| region_of(s)["source_offset"] == 0 && region_of(s)["length"] == 72_000,
        |s| region_of(s)["source_offset"] == 24_000 && region_of(s)["length"] == 48_000,
    );

    // 7. Drag the gain band 20 points up: +5 dB at 0.25 dB a point.
    let handle = in_editor(&mut app, &w, "Gain Handle");
    let f = &handle["frame"];
    let (cx, cy) = (
        f["x"].as_f64().unwrap() + f["width"].as_f64().unwrap() / 2.0,
        f["y"].as_f64().unwrap() + f["height"].as_f64().unwrap() / 2.0,
    );
    app.call(
        "ui.drag",
        json!({"window": w, "from": handle["id"], "to": {"x": cx, "y": cy - 20.0}, "steps": 8}),
    );
    let s = wait_for_state(&mut app, &w, "audio_editor", |s| {
        (gain(s) - 0.8).abs() < 0.3
    });
    wait_for_undo(&mut app, &w, "Region Gain");
    let dragged = gain(&s);

    // 8. Normalize to −1 dBFS: −1 − (−12.04) = +11.04 dB; undo brings the
    // dragged gain back.
    let id = in_editor(&mut app, &w, "Normalize Peak")["id"].clone();
    app.call(
        "ui.key",
        json!({"window": w, "element": id, "text": "-1\n"}),
    );
    let id = in_editor(&mut app, &w, "Normalize")["id"].clone();
    app.call("ui.click", json!({"window": w, "element": id}));
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        (gain(s) - 11.04).abs() < 0.05
    });
    wait_for_undo(&mut app, &w, "Normalize");
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        (gain(s) - dragged).abs() < 1e-6
    });
    // A bare Return on a negative gain that is not a tenth sends nothing
    // (final review I1): Normalize to −13 dBFS gives −0.96 dB, shown as
    // "−1.0 dB"; then undo it.
    let id = in_editor(&mut app, &w, "Normalize Peak")["id"].clone();
    app.call(
        "ui.key",
        json!({"window": w, "element": id, "text": "-13\n"}),
    );
    let id = in_editor(&mut app, &w, "Normalize")["id"].clone();
    app.call("ui.click", json!({"window": w, "element": id}));
    let s = wait_for_state(&mut app, &w, "audio_editor", |s| {
        (gain(s) + 0.96).abs() < 0.05
    });
    wait_for_undo(&mut app, &w, "Normalize");
    let normalized = gain(&s);
    let id = in_editor(&mut app, &w, "Region Gain")["id"].clone();
    app.call("ui.key", json!({"window": w, "element": id, "text": "\n"}));
    std::thread::sleep(Duration::from_millis(500));
    let s = state(&mut app, &w);
    assert_eq!(gain(&s), normalized, "{s}");
    wait_for_undo(&mut app, &w, "Normalize");
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        (gain(s) - dragged).abs() < 1e-6
    });

    // 9. Zoom In halves the frames per point.
    let fpp = state(&mut app, &w)["frames_per_point"].as_f64().unwrap();
    let id = in_editor(&mut app, &w, "Zoom In")["id"].clone();
    app.call("ui.click", json!({"window": w, "element": id}));
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        (s["frames_per_point"].as_f64().unwrap_or(f64::NAN) - fpp / 2.0).abs() <= 1.0
    });

    // 10. Saved and opened again: the edits and the bottom view.
    let path = home.join("edited.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": path}, "window": w}),
    );
    app.call("command.run", json!({"id": "file.close", "window": w}));
    let w3 = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w3, json!({"condition": "documents_at_least", "count": 1}));
    let (region, _) = wait_for_label_like(&mut app, &w3, "Region tone on ", "");
    app.call("ui.click", json!({"window": w3, "element": region}));
    let s = wait_for_state(&mut app, &w3, "audio_editor", |s| !s["region"].is_null());
    let r = region_of(&s);
    assert!((gain(&s) - dragged).abs() < 1e-6, "{s}");
    assert_eq!(
        (
            r["fade_in"]["frames"].clone(),
            r["source_offset"].clone(),
            r["length"].clone()
        ),
        (json!(12_000), json!(24_000), json!(48_000)),
        "{s}"
    );
    let l = layout(&mut app, &w3);
    assert_eq!(
        layout_pane(&l, "arrange", "bottom")["view"],
        "audio_editor",
        "{l}"
    );
    app.quit();
}

/// The first tree node whose label is exactly `label`.
fn find_node(nodes: &Value, label: &str) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        if n["label"].as_str() == Some(label) {
            Some(n.clone())
        } else {
            find_node(&n["children"], label)
        }
    })
}
