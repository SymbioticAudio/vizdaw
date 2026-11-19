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
        App::start_seeded(extra, |_| {})
    }

    /// [`App::start_with`], after `seed` has been given the throwaway
    /// home's support folder and before the app opens it.
    fn start_seeded(extra: &[(&str, &str)], seed: impl FnOnce(&std::path::Path)) -> App {
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
        seed(&home.path().join("support"));
        let launched = launch(&LaunchOptions {
            app,
            home: Some(home.path().to_path_buf()),
            demo: false,
            timeout: Duration::from_secs(180),
            env: [
                ("VIZDAW_AUDIO".to_string(), "offline".to_string()),
                ("VIZDAW_TRASH_DIR".to_string(), trash),
                // Plan 8b Ruling 11: approvals live in memory, never the
                // Keychain, and the sheet is answered through
                // `settings.co_producer_answer`, never a GUI.
                ("VIZDAW_TEST_HOOKS".to_string(), "1".to_string()),
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

/// Types `text` into the element labelled `label`, finding it again when
/// the view redrew between finding and typing (`no_element`): on a slow
/// runner the strip can rebuild just after the tree was read.
fn key_into_label(app: &mut App, window: &Value, label: &str, text: &str) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let id = wait_for_label(app, window, label);
        let params = json!({"window": window, "element": id, "text": text});
        let timeout = Duration::from_millis(WAIT_MS) + Duration::from_secs(60);
        match app.client.call("ui.key", params.clone(), timeout) {
            Ok(_) => return,
            Err(e) if e.to_string().contains("no_element") && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                // Where the element is and how big the window is: an
                // element whose centre is outside the window is "gone".
                let tree = app.call("ui.tree", json!({"window": window}));
                let node = find_node(&tree["nodes"], label);
                let windows = app.call("state.get", json!({"target": "windows"}));
                panic!(
                    "ui.key {params}: {e}\nnode: {node:?}\nwindows: {windows}\nVizDAW log:\n{}",
                    log_tail(&app.launched.log, 40)
                )
            }
        }
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

/// A 48 kHz float CAF of `frames` frames of `channels` channels, every
/// sample at `level` (plan 6c Task 7, F19: [`audio_file`] with a level).
fn audio_file_level(path: &std::path::Path, frames: usize, level: f32, channels: u16) {
    let mut w = core_media::caf::CafWriter::create(
        path,
        48_000,
        channels,
        core_model::format::BitDepth::Float32,
        core_media::caf::WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![level; usize::from(channels) * frames])
        .unwrap();
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
    // Four bars at 120 BPM: eight seconds a pass, so a keep or discard
    // lands in the pass it means even on a loaded machine (one bar, two
    // seconds, was not enough on a GitHub macOS runner: a discard landed
    // in the pass after). "Keys" (MIDI from source "Keys" on the built-in
    // synth) and "Mic" (fake input 1, heard never).
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
        length: SeqLength::Locked(Tick(16 * Q)),
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
    // and becomes a take; it lasts eight seconds. Counted from the pass's
    // start, not from when the discard answered, so a slow answer does not
    // push the Stop towards pass 4.
    assert_eq!(r["pass"], 3, "{r}");
    let at = r["pass_start_file_frame"].as_u64().unwrap();
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
    // From the start: the loop pass's Stop left the position wherever it
    // landed, and the MIDI pass must not get only what is left of the
    // sequence before it ends by itself.
    app.call(
        "command.run",
        json!({"id": "transport.return_to_start", "window": w}),
    );
    wait_for_tick(&mut app, &w, 0);
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

/// Clicks the number box `label` until it is editing (its node is
/// `focused`), at most three times. The channel's window is not key, so
/// the refocus symbiotic-design 0.2.2 gives a box after Esc or Return can
/// stay pending there and the next click only lands it. Callers then type
/// with no element, so a click that already edited is not repeated.
fn focus_number_box(app: &mut App, window: &Value, label: &str) {
    for _ in 0..3 {
        let el = wait_for_label(app, window, label);
        app.call("ui.click", json!({"window": window, "element": el}));
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            let tree = app.call("ui.tree", json!({"window": window}));
            if find_node(&tree["nodes"], label).is_some_and(|n| n["focused"] == true) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    panic!("{label} never started editing");
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
    // agent finds Loop, Record and the pane buttons, not only Play. The
    // window is narrower than the default bar (Task 12b folds the tail
    // into »), so a short bar goes first.
    app.call(
        "command.run",
        json!({"id": "ui.set_transport_bar", "params": {"items": "cluster,loop,record_options,panes"},
            "window": window}),
    );
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
    wait_for_label(
        &mut app,
        &window,
        "Select a track to see its channel strip.",
    );
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
    key_into_label(&mut app, &window, &format!("Level of {SYNTH}"), "-4.2\n");
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
    focus_number_box(&mut app, &window, &format!("Level of {SYNTH}"));
    let half = "9".repeat(150);
    app.call("ui.key", json!({"window": window, "text": half}));
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
    focus_number_box(&mut app, &window, &format!("Pan of {SYNTH}"));
    app.call("ui.key", json!({"window": window, "text": "L 24\n"}));
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

/// Polls the window's tree until the node labelled exactly `label`
/// exists; returns it (its `id`, `value` and `focused`).
fn wait_for_node(app: &mut App, window: &Value, label: &str) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(n) = find_node(&tree["nodes"], label) {
            return n;
        }
        assert!(
            Instant::now() < deadline,
            "no {label:?} within {WAIT_MS} ms in {tree}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
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

/// README steps 225–228 through the channel (plan 7d): the readouts, a
/// click on Fast Forward, Rewind and Return to Start, a ⇧-click with no
/// marker, ⇧. and the keypad's minus, hostile parameters, a typed field
/// cancelled by a click away, and a typed punch time that refuses Record
/// until Return sets it.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn transport_jumps_readouts_and_fields_over_the_channel() {
    use core_control::ErrorCode;
    const BAR: i64 = 61_440;
    const DRAFT: &str = "Press Return to set the punch, or Esc to discard it.";
    let mut app = App::start();
    // The demo's shown sequence: 4/4 at 120 BPM over its first ten bars.
    let w = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
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
    // One click in a field selects all its text (plan 7d final wave), so
    // what is typed replaces it.
    let select_field = |app: &mut App, label: &str| click(app, label, json!([]));
    let no_alert = |app: &mut App| {
        let ws = app.call("state.get", json!({"target": "windows"}));
        assert_eq!(ws["alerts"].as_array().unwrap().len(), 0, "{ws}");
        let doc = app.call("state.get", json!({"target": "document", "window": w}));
        assert!(doc["alert"].is_null(), "no dialog: {doc}");
    };

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

    // 3. A ⇧-click with no marker: the status line says so, nothing moves.
    click(&mut app, "Fast Forward", json!(["shift"]));
    let e = wait_for_editor(&mut app, &w, "no marker", |e| {
        e["status"] == "No marker after the position."
    });
    assert_eq!(tick(&mut app), 0, "{e}");
    no_alert(&mut app);

    // 4. ⇧. one bar on, the keypad's minus one bar back.
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

    // 7. A typed bar Punch In refuses Record (plan 7d controller ruling
    // I5, fix round 1 M4): a real click on the bar's Record keeps the
    // draft and the keyboard, and Return then sets the punch. The record
    // popover's own punch fields are out of the channel's reach: its
    // window has no title, so `state.get windows` does not list it and
    // `ui.tree` and `ui.key` cannot name it; README step 228 checks them.
    let recording = |app: &mut App| {
        app.call("state.get", json!({"target": "record", "window": w}))["recording"].clone()
    };
    let punch_in = |e: &Value| e["transport"]["punch_in"].clone();
    // The default bar has no Punch fields (Task 12b): a short bar with
    // them, so they show in this window before anything folds.
    app.call(
        "command.run",
        json!({"id": "ui.set_transport_bar",
            "params": {"items": "cluster,edit_fields,punch_fields"}}),
    );
    let before = punch_in(&app.call("state.get", json!({"target": "editor", "window": w})));
    select_field(&mut app, "Punch In");
    app.call("ui.key", json!({"window": w, "text": "3"}));
    click(&mut app, "Record", json!([]));
    wait_for_editor(&mut app, &w, "Record refused", |e| e["status"] == DRAFT);
    assert_eq!(recording(&mut app), false, "nothing records");
    let e = app.call("state.get", json!({"target": "editor", "window": w}));
    assert_eq!(punch_in(&e), before, "the punch is not set yet: {e}");
    let n = wait_for_node(&mut app, &w, "Punch In");
    assert_eq!(n["focused"], true, "Punch In keeps the keyboard: {n}");
    app.call("ui.key", json!({"window": w, "key": "return"}));
    wait_for_editor(&mut app, &w, "the punch set", |e| {
        e["transport"]["punch_in"] == 2 * BAR
    });
    // The channel's (and a surface's) transport.record refuses the same way.
    select_field(&mut app, "Punch In");
    app.call("ui.key", json!({"window": w, "text": "4"}));
    let e = app.call_err(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "{e}");
    assert_eq!(e.message, DRAFT, "{e}");
    assert_eq!(recording(&mut app), false, "nothing records");
    app.call("ui.key", json!({"window": w, "key": "escape"}));
    let n = wait_for_node(&mut app, &w, "Punch In");
    let e = app.call("state.get", json!({"target": "editor", "window": w}));
    assert_eq!(
        e["transport"]["punch_in"],
        2 * BAR,
        "Esc sets nothing: {e} {n}"
    );
    assert_eq!(tick(&mut app), 4 * BAR);
    no_alert(&mut app);
    app.quit();
}

/// One pixel of a 24- or 32-bit uncompressed BMP, as [r, g, b]. Rows are
/// stored bottom-up and padded to four bytes (Ruling 8).
fn bmp_pixel(path: &std::path::Path, x: u32, y: u32) -> [u8; 3] {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..2], b"BM", "a BMP");
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let offset = u32_at(10) as usize;
    let width = u32_at(18);
    let height = i32::from_le_bytes(b[22..26].try_into().unwrap());
    let bits = u16::from_le_bytes(b[28..30].try_into().unwrap());
    assert!(bits == 24 || bits == 32, "{bits}-bit BMP");
    // 0 is plain; 3 is "bitfields", which sips writes for 32 bits with
    // the usual B, G, R, A byte order.
    assert!(matches!(u32_at(30), 0 | 3), "compression {}", u32_at(30));
    let (w, h) = (width, height.unsigned_abs());
    assert!(x < w && y < h, "pixel {x},{y} is outside {w}x{h}");
    let row = if height > 0 { h - 1 - y } else { y } as usize;
    let stride = (w as usize * (bits as usize / 8)).div_ceil(4) * 4;
    let at = offset + row * stride + x as usize * (bits as usize / 8);
    [b[at + 2], b[at + 1], b[at]]
}

/// `ui.screenshot {}` of the window as a BMP (`sips`, a system tool).
/// Returns the BMP's path and the pixels per point.
fn window_bmp(app: &mut App, window: &Value, name: &str) -> (PathBuf, f64) {
    let png = app.launched.home.join(format!("{name}.png"));
    let bmp = app.launched.home.join(format!("{name}.bmp"));
    let r = app.call("ui.screenshot", json!({"window": window, "path": png}));
    let windows = app.call("state.get", json!({"target": "windows"}));
    let win = windows["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["number"] == *window)
        .unwrap_or_else(|| panic!("no window in {windows}"))
        .clone();
    let scale = r["width"].as_f64().unwrap() / win["frame"]["width"].as_f64().unwrap();
    let out = std::process::Command::new("sips")
        .args(["-s", "format", "bmp"])
        .arg(&png)
        .arg("--out")
        .arg(&bmp)
        .output()
        .expect("sips");
    assert!(out.status.success(), "sips: {out:?}");
    (bmp, scale)
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

/// README steps 233–237 through the channel (plan tracks): a new project
/// opens with "Sequence 1" and no tracks; the header column and the Add
/// Track row are painted (checked on the screenshot's pixels, Ruling 8);
/// the three New Track commands, Undo, Duplicate, hostile parameters; a
/// channel click on Add Track opens no menu and blocks nothing.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn new_project_tracks_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let made = app.call("command.run", json!({"id": "file.new"}));
    let window = made["window"].clone();
    wait_for_label(&mut app, &window, "Track Headers");

    // The start state: Sequence 1, selected, no tracks, and the add row.
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    let seqs = doc["sequences"].as_array().unwrap();
    assert_eq!(seqs.len(), 1, "{doc}");
    assert_eq!(seqs[0]["name"], "Sequence 1", "{doc}");
    assert_eq!(doc["selected_seq"], seqs[0]["id"], "{doc}");
    assert!(track_list(&mut app, &window).is_empty(), "{doc}");
    let add = wait_for_node(&mut app, &window, "Add Track");
    // A channel click on Add Track opens no menu: the status says what to
    // run instead, and the app answers at once. Here, before any track, so
    // the row is in view in a small window (a CI runner's 980×620).
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

    // Paint, not accessibility frames: the header column differs from the
    // canvas, and so does the Add Track row.
    let tree = app.call("ui.tree", json!({"window": window}));
    let headers = find_node(&tree["nodes"], "Track Headers").expect("the headers");
    let f = |n: &Value, k: &str| n["frame"][k].as_f64().unwrap();
    let (bmp, s) = window_bmp(&mut app, &window, "start");
    let px = |x: f64, y: f64| bmp_pixel(&bmp, (x * s) as u32, (y * s) as u32);
    let y = f(&headers, "y") + 22.0 + 40.0; // below the ruler, inside the first row
    let header = px(f(&headers, "x") + f(&headers, "width") / 2.0, y);
    let canvas = px(f(&headers, "x") + f(&headers, "width") + 40.0, y);
    assert_ne!(
        header, canvas,
        "the header column is painted unlike the canvas"
    );
    let add_px = px(f(&add, "x") + f(&add, "width") / 2.0, f(&add, "y") + 30.0);
    assert_ne!(
        add_px, canvas,
        "the Add Track row is painted unlike the canvas"
    );

    // The three kinds, below one another.
    for id in ["track.new_audio", "track.new_instrument", "track.new_midi"] {
        app.call("command.run", json!({"id": id, "window": window}));
    }
    let tracks = track_list(&mut app, &window);
    let kinds: Vec<&str> = tracks.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(kinds, ["audio", "instrument", "midi"], "{tracks:?}");
    let mut first_label = String::new();
    for prefix in [
        "Audio track, Audio 1",
        "Instrument track, Inst 1",
        "MIDI track, MIDI 1",
    ] {
        let (_, label) = wait_for_label_like(&mut app, &window, prefix, "");
        if first_label.is_empty() {
            first_label = label;
        }
    }
    let tree = app.call("ui.tree", json!({"window": window}));
    let first_row = find_node(&tree["nodes"], &first_label).expect("the first row");
    // A header row's separator line shows on the screenshot.
    let (bmp, s) = window_bmp(&mut app, &window, "three");
    let px = |x: f64, y: f64| bmp_pixel(&bmp, (x * s) as u32, (y * s) as u32);
    let x = f(&headers, "x") + f(&headers, "width") - 8.0;
    // The first row's bottom edge, from its own frame (not a fixed height).
    let row_bottom = f(&first_row, "y") + f(&first_row, "height");
    let body = px(x, row_bottom - 4.0);
    let line = (-1..=1)
        .map(|d| px(x, row_bottom + d as f64 * 0.5))
        .any(|p| p != body);
    assert!(line, "the first row's separator line is painted ({body:?})");

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

    // `live` is a boolean; the test never asks for a live capture.
    let e = app.call_err("ui.screenshot", json!({"window": window, "live": "yes"}));
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    app.quit();
}

/// The Loop command: with Loop on, playing past the end of the sequence
/// wraps to bar 1 instead of stopping.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn loop_wraps_at_the_end() {
    let mut app = App::start();
    let opened = app.call("app.new_demo", json!({}));
    let window = opened["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "window": window, "params": params}),
        )
    };
    let button = wait_for_label(&mut app, &window, "Loop");
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

/// Spec §12.3's "`midi.inject` held chord into a lane with an arpeggiator
/// while recording: check the recorded notes are the arpeggio" (plan 5g-1;
/// README 239): a C major chord held for a bar into "Keys" with an Up
/// arpeggiator at 1/16 records as C, E, G repeating, each on a sixteenth
/// and half a sixteenth long. Offline device, no MIDI port.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn arpeggiates_an_injected_chord_while_recording() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::event::EventBody;
    use core_model::input_fx::{Arp, InputFx, InputFxKind};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let mut app = App::start_with(&[]);
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
    let path = app.launched.home.join("Arp.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let lane = lane_named(&mut app, &w, "Keys");
    assert_eq!(
        lane["input_fx"][0]["kind"]["arpeggiator"]["rate"]["division"], 16,
        "{lane}"
    );
    assert_eq!(lane["arm"], true, "{lane}");
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"replace": true, "loop": false, "pre_roll": 0, "count_in": 0}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| {
        r["recording"] == true && r["frames"].as_u64() >= Some(24_000)
    });
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "90 3c 64 90 40 64 90 43 64"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(24_000 * 5));
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "80 3c 00 80 40 00 80 43 00"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(input["midi_fx_dropped"], 0, "{input}");
    app.call("command.run", json!({"id": "file.save", "window": w}));
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
        assert!((duration - Q / 8).abs() <= 2, "{duration}: {notes:?}");
    }
    app.quit();
}

/// Plan 5g-1b Task 4: `destination.set_midi_mix` over the channel; the mix
/// shows in `state.get document`'s destinations, an absent key keeps its
/// value and a null volume or pan unsets it. Offline: nothing is sent.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn midi_mix_reaches_state_get() {
    use core_control::ErrorCode;
    // The fake synth is a MIDI port to add a destination for, with no hardware.
    let mut app = App::start_with(&[("VIZDAW_FAKE_SYNTH", "25")]);
    let made = app.call("command.run", json!({"id": "file.new"}));
    let w = made["window"].clone();
    wait_for_label(&mut app, &w, "Track Headers");
    app.call(
        "command.run",
        json!({"id": "destination.add_midi", "params": {"name": "Rack", "port": "Fake Synth", "channel": 3}, "window": w}),
    );
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
    assert_eq!(d["external"], true, "{d}");
    assert_eq!(
        (&d["midi_volume"], &d["midi_pan"], &d["midi_mute"]),
        (&Value::Null, &Value::Null, &json!(false)),
        "{d}"
    );
    let set = |app: &mut App, params: Value| {
        let mut p = params;
        p["dest"] = id.clone();
        app.call(
            "command.run",
            json!({"id": "destination.set_midi_mix", "params": p, "window": w}),
        );
    };
    set(&mut app, json!({"volume": 90, "mute": true}));
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
}

/// Plan 5g-2 (spec §13 5g-2, README 252): MPE injected into an armed MPE
/// track while recording lands as notes with per-note bend and timbre and
/// poly pressure; the lane and destination settings show in
/// `state.get document`; nothing was dropped. Offline device, no port.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn records_mpe_as_per_note_expression() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::event::{EventBody, PerNoteKind};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let mut app = App::start_with(&[]);
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("MPE", "Keys", Some(synth)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("MPE Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(8 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Mpe.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    app.call("command.run", json!({"id": "lane.set_mpe", "params": {"seq": seq.0, "lane": keys.0, "zone": "lower"}, "window": w}));
    app.call("command.run", json!({"id": "destination.set_mpe", "params": {"dest": synth.0, "zone": "lower"}, "window": w}));
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(
        lane_named(&mut app, &w, "Keys")["mpe"],
        json!({"zone": "lower", "members": 15, "bend_range": 48})
    );
    let dest = doc["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "Synth")
        .cloned();
    assert_eq!(
        dest.clone().map(|d| d["mpe"]["zone"].clone()),
        Some(json!("lower")),
        "{doc}"
    );
    assert!(
        dest.is_some_and(|d| d.get("midi_volume").is_some()),
        "5g-1b's keys stay: {doc}"
    );
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    app.call("command.run", json!({"id": "record.set", "params": {"replace": true, "loop": false, "pre_roll": 0, "count_in": 0}, "window": w}));
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| {
        r["recording"] == true && r["frames"].as_u64() >= Some(24_000)
    });
    let inject = |app: &mut App, hex: &str| {
        app.call(
            "command.run",
            json!({"id": "midi.inject", "params": {"port": "MPE Keys", "bytes": hex}, "window": w}),
        );
    };
    inject(&mut app, "e1 00 50 b1 4a 30 d1 20 91 3c 64");
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(48_000));
    inject(&mut app, "e1 10 60 d1 40 b1 4a 7f");
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(72_000));
    inject(&mut app, "81 3c 00");
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(
        (
            input["per_note_dropped"].clone(),
            input["mpe_stolen"].clone()
        ),
        (json!(0), json!(0)),
        "{input}"
    );
    app.call("command.run", json!({"id": "file.save", "window": w}));
    let doc = core_model::file::load(&path).unwrap();
    let l = doc
        .sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.id == keys)
        .unwrap();
    let events: Vec<_> = l.active().unwrap().events.values().cloned().collect();
    let n = events
        .iter()
        .find(|e| matches!(e.body, EventBody::Note { pitch: 60, .. }))
        .expect("the note");
    let EventBody::Note { per_note, .. } = &n.body else {
        unreachable!()
    };
    let kinds: Vec<PerNoteKind> = per_note.iter().map(|p| p.kind).collect();
    assert!(
        kinds.contains(&PerNoteKind::PitchBend) && kinds.contains(&PerNoteKind::Registered(74)),
        "{per_note:?}"
    );
    assert!(
        per_note
            .iter()
            .any(|p| p.kind == PerNoteKind::PitchBend && p.offset.0 == 0),
        "the start bend"
    );
    assert!(
        per_note
            .iter()
            .any(|p| p.kind == PerNoteKind::PitchBend && p.offset.0 > 0),
        "the later bend"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.body, EventBody::PolyPressure { pitch: 60, .. })),
        "pressure"
    );
    assert_eq!(l.mpe.map(|m| m.members), Some(15));
    app.quit();
}

/// The windows' numbers and kinds, to show a call opened none.
fn window_numbers(app: &mut App) -> Vec<(Value, Value)> {
    let w = app.call("state.get", json!({"target": "windows"}));
    w["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["number"].clone(), x["kind"].clone()))
        .collect()
}

/// README 257–266 through the channel (plan 6a Task 10): export the demo
/// as a MIDI file and a MIDI Clip File, import them back into the demo and
/// into a new project, and the refusals. No call opens a sheet or a panel;
/// only the new project's window appears.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn midi_files_round_trip_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
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

/// README 267–273 through the channel (plan 6b Task 5): export the demo for
/// another DAW, re-import its MIDI and stems, export again with "Add the
/// stems to this project", and the refusals. The reply comes once the
/// `.exportProject` job ends; no call opens a sheet or a panel.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn exports_for_another_daw_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
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

/// README 274–281 through the channel (plan 6c Task 7): two mono regions
/// on one track, the later one starting 0.5 s into the earlier; hostile
/// `region.crossfade` calls change nothing; a 100 ms Linear crossfade,
/// heard in a bounce as the midpoint of the two levels; Remove Crossfade,
/// the default 10 ms Equal Power one and its midpoint; Undo twice; and
/// Remove Fades drops the crossfade with a status line (F1).
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn crossfades_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let home = app.launched.home.clone();
    // 1. Mono files make one track each (F20): 2 s at 0.25 and at 0.5.
    let (a_file, b_file) = (home.join("a.caf"), home.join("b.caf"));
    audio_file_level(&a_file, 96_000, 0.25, 1);
    audio_file_level(&b_file, 96_000, 0.5, 1);
    let made = app.call("command.run", json!({"id": "file.new"}));
    let w = made["window"].clone();
    wait_for_label(&mut app, &w, "Track Headers");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": a_file, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let lanes = doc["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 1, "one track for a mono file: {doc}");
    let lane = lanes[0]["id"].as_u64().unwrap();
    // 0.5 s in at 120 BPM: the overlap is 1.5 s.
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": b_file, "seq": seq, "lane": lane, "tick": 15_360}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(doc["lanes"].as_array().unwrap().len(), 1, "{doc}");
    assert_eq!(doc["lanes"][0]["regions"], 2, "{doc}");

    // 2. Each region by its element; `b` ends selected. The two overlap,
    // so they make one block, and a click on a region's body selects the
    // block: the click lands on the region's gain band instead (its top
    // edge, the later region on top), at `at` of its width, which selects
    // that region alone.
    let select = |app: &mut App, name: &str, at: f64| -> (Value, Value) {
        let (_, label) = wait_for_label_like(app, &w, &format!("Region {name} on "), "");
        let tree = app.call("ui.tree", json!({"window": w}));
        let f = find_node(&tree["nodes"], &label).expect("the region")["frame"].clone();
        let num = |k: &str| f[k].as_f64().unwrap();
        let (x, y) = (num("x") + at * num("width"), num("y") + 2.0);
        app.call("ui.click", json!({"window": w, "point": {"x": x, "y": y}}));
        let sel = app.call("state.get", json!({"target": "selection", "window": w}));
        assert_eq!(sel["events"].as_array().unwrap().len(), 1, "{label}: {sel}");
        (sel["events"][0].clone(), sel["lanes"][0]["lane"].clone())
    };
    // `a` 0.2 s in, before `b` starts; `b` 0.2 s before its end, after
    // `a` ends.
    let (a, a_lane) = select(&mut app, "a", 0.1);
    let (b, b_lane) = select(&mut app, "b", 0.9);
    assert_ne!(a, b);
    assert_eq!((a_lane, b_lane.clone()), (json!(lane), json!(lane)));
    let lane = b_lane;

    // 3. Hostile calls change nothing and open nothing.
    let undo = |app: &mut App| {
        app.call("state.get", json!({"target": "document", "window": w}))["undo"].clone()
    };
    let undo0 = undo(&mut app);
    let windows = window_numbers(&mut app);
    for params in [
        json!({"seq": seq, "lane": lane, "event": b, "length_ms": "0"}),
        json!({"seq": seq, "lane": lane, "event": b, "shape": "cubic"}),
        json!({"seq": seq}),
    ] {
        let e = app.call_err(
            "command.run",
            json!({"id": "region.crossfade", "params": params.clone(), "window": w}),
        );
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}: {e:?}");
    }
    let e = app.call_err(
        "command.run",
        json!({"id": "region.crossfade", "params": {"seq": seq, "lane": lane, "event": a}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "a starts inside nothing: {e:?}");
    // Longer than the 1.5 s overlap: refused, nothing changes.
    let e = app.call_err(
        "command.run",
        json!({"id": "region.crossfade", "params": {"seq": seq, "lane": lane, "event": b, "length_ms": "2000"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "past the overlap: {e:?}");
    assert_eq!(undo(&mut app), undo0);
    assert_eq!(window_numbers(&mut app), windows, "no sheet or panel");

    // 4. 100 ms, Linear: 4800 frames at 48 kHz, crossing `a`.
    let r = app.call(
        "command.run",
        json!({"id": "region.crossfade", "params": {"seq": seq, "lane": lane, "event": b, "length_ms": "100", "shape": "linear"}, "window": w}),
    );
    assert_eq!(r["undo"], "Crossfade", "{r}");
    app.call(
        "command.run",
        json!({"id": "editors.audio_editor", "window": w}),
    );
    let crossfade = |frames: u64, shape: &str| json!({"frames": frames, "shape": shape, "with": a});
    let s = wait_for_state(&mut app, &w, "audio_editor", |s| {
        s["region"]["event"] == b && s["region"]["crossfade"] == crossfade(4800, "linear")
    });
    assert_eq!(s["boxes"]["crossfade"], "100 ms", "{s}");

    // 5. The bounce, dither off (F18): ratios, not levels, so the strip's
    // gain does not matter.
    let bounce = |app: &mut App, name: &str| -> Vec<i32> {
        let path = home.join(name);
        let r = app.call(
            "command.run",
            json!({"id": "file.bounce", "params": {"path": path, "bits": "16", "rate": 48_000, "dither": false, "range": "selection", "start": 0, "end": 46_080}, "window": w}),
        );
        assert!(
            r["status"].as_str().unwrap_or("").starts_with("wrote "),
            "{r}"
        );
        let (rate, ch, bits, frames, samples) = read_wav16(&path);
        assert_eq!((rate, bits), (48_000, 16), "{r}");
        assert!(frames >= 72_000, "{frames}");
        let ch = usize::from(ch);
        samples.iter().step_by(ch).map(|s| i32::from(*s)).collect()
    };
    let left = bounce(&mut app, "x.wav");
    let s = |f: usize| left[f];
    let (before, mid, after) = (s(12_000), s(26_400), s(40_000));
    assert!(after > before, "b is louder: {before} {after}");
    assert!(
        (2 * mid - (before + after)).abs() <= 6,
        "Linear's midpoint {mid} is halfway between {before} and {after}"
    );

    // 6. Remove Crossfade on the selection, then the default: 10 ms
    // (480 frames), Equal Power, heard at 0.7071 of each side at its middle.
    let r = app.call(
        "command.run",
        json!({"id": "region.remove_crossfade", "window": w}),
    );
    assert_eq!(r["undo"], "Remove Crossfade", "{r}");
    let r = app.call(
        "command.run",
        json!({"id": "region.crossfade", "window": w}),
    );
    assert_eq!(r["undo"], "Crossfade", "{r}");
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        s["region"]["crossfade"] == crossfade(480, "equal_power")
    });
    let left = bounce(&mut app, "y.wav");
    let ep = left[24_240];
    let want = std::f64::consts::FRAC_1_SQRT_2 * f64::from(before + after);
    assert!(
        (f64::from(ep) - want).abs() <= 3.0,
        "Equal Power's midpoint {ep} is 0.7071 × ({before} + {after}) = {want}"
    );
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    wait_for_undo(&mut app, &w, "Crossfade");
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        s["region"]["crossfade"] == crossfade(4800, "linear")
    });

    // 7. Remove Fades on `b` takes its fade in to 0: settle drops the
    // crossfade, and the status says so (F1).
    let (again, _) = select(&mut app, "b", 0.9);
    assert_eq!(again, b);
    let r = app.call(
        "command.run",
        json!({"id": "region.remove_fades", "window": w}),
    );
    let status = r["status"].as_str().unwrap_or("");
    assert!(status.contains("Crossfade removed"), "{r}");
    let s = wait_for_state(&mut app, &w, "audio_editor", |s| {
        s["region"]["event"] == b && s["region"]["crossfade"].is_null()
    });
    assert_eq!(s["region"]["fade_in"]["frames"], 0, "{s}");
    assert_eq!(window_numbers(&mut app), windows);
    app.quit();
}

/// README 282–290 through the channel (plan 6d Task 13): hostile calls to
/// both DAWproject commands change nothing and open nothing; a dry run
/// writes nothing; the export writes a ZIP and refuses to replace it
/// without `replace`; the import makes a new project with the demo's
/// tracks and the VizDAW-origin note, and its report is the one
/// `file.interchange_report` gives. Only the new project's window appears.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn dawproject_round_trip_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
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

/// README 300–309 through the channel (plan 6f Task 9): hostile calls to
/// both AAF commands are refused and open nothing; the export writes the
/// "<song> AAF" folder; the import makes a new project with the demo's
/// audio tracks and the note that the file came from VizDAW, with no
/// internal error. Only the new project's window appears.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn aaf_round_trip_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
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

/// README 291–299 through the channel (plan 6e Task 11): a file from
/// VizDAW without `mode` is `dialog_required` "Return"; hostile merge calls
/// are refused and open nothing; a preview and a merge of the project's own
/// unchanged export find nothing to merge, add no undo step and open no
/// window; `file.interchange_report` gives the merge's report; and 6d's
/// path (`mode: "new"`) still makes a new project.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn dawproject_merge_over_the_channel() {
    use core_control::ErrorCode;
    const NOTHING: &str = "Nothing to merge: the file matches the project.";
    let mut app = App::start();
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

/// Plan 6g README steps 310, 313 and 315 through the channel: the app
/// carries Multi-Q's two bundles, the new-channel switch answers its
/// status, and "Install Multi-Q for other apps" copies the AU into the
/// throwaway home's Components folder, asks before replacing, and
/// replaces when told. Nothing reaches the real `~/Library`. The app
/// launches with `--no-scan`, so the new-track default is covered by
/// core-ffi's tests and README step 311, not here.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn multiq_install_and_switch_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let home = app.launched.home.clone();

    // The bundle holds both Multi-Q builds (step 310).
    let exe = std::env::var("VIZDAW_E2E_APP").expect("VIZDAW_E2E_APP");
    let exe = PathBuf::from(exe);
    let exe = if exe.is_absolute() {
        exe
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(exe)
    };
    let bundle = if exe.extension().is_some_and(|e| e == "app") {
        exe
    } else {
        // `.../VizDAW.app/Contents/MacOS/VizDAW`.
        exe.ancestors()
            .nth(3)
            .expect("the app bundle")
            .to_path_buf()
    };
    for name in ["Multi-Q.clap", "Multi-Q.component"] {
        let p = bundle.join("Contents/PlugIns").join(name);
        assert!(p.exists(), "{} is in the app", p.display());
    }

    // The switch answers its status both ways (step 313).
    for (on, status) in [(false, "new-channel EQ off"), (true, "new-channel EQ on")] {
        let r = app.call(
            "command.run",
            json!({"id": "plugins.new_channel_eq", "params": {"on": on}}),
        );
        assert_eq!(r["status"], status, "{r}");
    }

    // Install into the throwaway home (step 315).
    let components = home.join("Library/Audio/Plug-Ins/Components");
    let target = components.join("Multi-Q.component");
    let r = app.call(
        "command.run",
        json!({"id": "plugins.install_multiq", "params": {}}),
    );
    assert_eq!(r["status"], target.display().to_string(), "{r}");
    assert!(
        components.is_dir(),
        "the folder exists in the throwaway home"
    );
    assert!(target.exists(), "{}", target.display());

    // A second install asks; replace answers it.
    let e = app.call_err(
        "command.run",
        json!({"id": "plugins.install_multiq", "params": {}}),
    );
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e:?}");
    assert!(e.message.contains("Replace"), "{e:?}");
    let r = app.call(
        "command.run",
        json!({"id": "plugins.install_multiq", "params": {"replace": true}}),
    );
    assert_eq!(r["status"], target.display().to_string(), "{r}");
    assert!(target.exists());
    app.quit();
}

/// Plan 6g-2 README steps 319, 321 and 322 through the channel: the
/// default-EQ command answers and refuses, and New Bus from a strip is one
/// undo step that takes the bus and its send together, and back with redo.
/// The app launches with `--no-scan`, so which EQ lands is covered by
/// core-ffi's tests and the README, not here.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn default_eq_and_new_bus_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();

    // The picker's command: Multi-Q answers, an unknown plug-in is refused.
    let r = app.call(
        "command.run",
        json!({"id": "plugins.default_eq", "params": {"plugin": "multi-q"}}),
    );
    assert_eq!(r["status"], "default EQ Multi-Q", "{r}");
    let e = app.call_err(
        "command.run",
        json!({"id": "plugins.default_eq", "params": {"plugin": "clap:vizdaw.e2e.none"}}),
    );
    assert_eq!(e.code, ErrorCode::UnknownId, "{e:?}");

    // A new project with one audio track.
    let made = app.call("command.run", json!({"id": "file.new"}));
    let window = made["window"].clone();
    wait_for_label(&mut app, &window, "Track Headers");
    app.call(
        "command.run",
        json!({"id": "track.new_audio", "window": window}),
    );
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    let strips = m["strips"].as_array().unwrap();
    let audio = strips
        .iter()
        .find(|s| s["kind_colour"] == "channel")
        .unwrap_or_else(|| panic!("no audio strip in {m}"))
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
    let audio_name = audio["name"].as_str().unwrap().to_string();

    // New Bus from the strip: "Bus 1", one step, a send to it.
    let r = app.call(
        "command.run",
        json!({"id": "mixer.new_bus", "params": {"strip": audio["id"]}, "window": window}),
    );
    assert_eq!(r["status"], "Bus 1", "{r}");
    assert_eq!(r["undo"], "New Bus", "{r}");
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    let bus = strip_named(&m, "Bus 1");
    let from = strip_named(&m, &audio_name);
    assert!(
        from["sends"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["target"] == bus["id"]),
        "{from}"
    );

    // One Undo removes the bus and the send; Redo brings both back.
    app.call("command.run", json!({"id": "edit.undo", "window": window}));
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    assert_eq!(buses(&m), buses_before, "the bus is gone: {m}");
    let from = strip_named(&m, &audio_name);
    assert!(
        from["sends"].as_array().unwrap().is_empty(),
        "the send is gone: {from}"
    );
    app.call("command.run", json!({"id": "edit.redo", "window": window}));
    let m = app.call("state.get", json!({"target": "mixer", "window": window}));
    let bus = strip_named(&m, "Bus 1");
    let from = strip_named(&m, &audio_name);
    assert!(
        from["sends"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["target"] == bus["id"]),
        "the send is back: {from}"
    );

    // Without a strip: a bus with no send, named "Bus 2". The master is refused.
    let r = app.call(
        "command.run",
        json!({"id": "mixer.new_bus", "params": {}, "window": window}),
    );
    assert_eq!(r["status"], "Bus 2", "{r}");
    let e = app.call_err(
        "command.run",
        json!({"id": "mixer.new_bus", "params": {"strip": master["id"]}, "window": window}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}");
    app.quit();
}

/// The ID of the tree's button (or toggle button) labelled exactly `label`.
fn button_labelled(nodes: &Value, label: &str) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        let exact = |k: &str| n[k].as_str() == Some(label);
        if (n["role"] == "AXButton" || n["role"] == "AXCheckBox")
            && (exact("label") || exact("title"))
        {
            Some(n["id"].clone())
        } else {
            button_labelled(&n["children"], label)
        }
    })
}

/// README steps 326–331 through the channel (plan 5c-2 takes): a new file
/// records in Overdub; a MIDI loop run layers its passes onto the one take
/// and one Undo removes them; with MIDI passes as takes on, the passes make
/// takes, which `track.choose_take`, `track.rename_take` and
/// `track.delete_take` choose, rename and delete, each one undo step. The
/// offline device only: no audio hardware and no permission prompt.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn takes_and_layered_midi_loop_over_the_channel() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, SequenceCmd};
    use core_model::lane::LaneInput;
    use core_model::sequence::SeqLength;
    use core_model::tempo::{Bpm, TempoMap, TempoPoint};
    use core_model::time::Tick;
    let mut app = App::start();
    // Four bars at 120 BPM (eight seconds a pass): "Keys" takes MIDI from
    // port "Keys" on the built-in synth, with two notes in Take 1.
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Take", "Keys", Some(synth)).unwrap();
    b.notes(
        seq,
        keys,
        &[(0, 60, 0x8000, Q / 2), (8 * Q, 64, 0x8000, Q / 2)],
    )
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
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
        length: SeqLength::Locked(Tick(16 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Takes.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let lane = lane_named(&mut app, &w, "Keys");
    assert_eq!(
        (lane["takes"].clone(), lane["events"].clone()),
        (json!(1), json!(2))
    );
    let keys_id = lane["id"].clone();

    // The bar's switch reads Overdub in a new file, and a click flips it
    // (the record spec exists only while recording, so the label is what
    // shows the idle mode).
    let wait_button = |app: &mut App, label: &str| -> Value {
        let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
        loop {
            let tree = app.call("ui.tree", json!({"window": w}));
            if let Some(id) = button_labelled(&tree["nodes"], label) {
                return id;
            }
            assert!(
                Instant::now() < deadline,
                "no {label:?} button within {WAIT_MS} ms in {tree}"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    let mode = wait_button(&mut app, "Record mode: Overdub");
    app.call("ui.click", json!({"window": w, "element": mode}));
    let back = wait_button(&mut app, "Record mode: Replace");
    app.call("ui.click", json!({"window": w, "element": back}));
    wait_button(&mut app, "Record mode: Overdub");
    let record = |app: &mut App| app.call("state.get", json!({"target": "record", "window": w}));

    // One note in pass `n` (counted from 1): a note-on a little way in,
    // a note-off a little later, both inside that pass.
    let note = |app: &mut App, n: u64, pitch: &str| {
        wait_for_record(app, &w, |r| r["pass"].as_u64() >= Some(n));
        let r = record(app);
        let at = r["pass_start_file_frame"].as_u64().unwrap();
        wait_for_record(app, &w, |r| r["frames"].as_u64() >= Some(at + 2_400));
        app.call(
            "command.run",
            json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": format!("90 {pitch} 64")}, "window": w}),
        );
        wait_for_record(app, &w, |r| r["frames"].as_u64() >= Some(at + 9_600));
        app.call(
            "command.run",
            json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": format!("80 {pitch} 00")}, "window": w}),
        );
        // Half a second in: a pass cut short before it is heard makes no take.
        wait_for_record(app, &w, |r| r["frames"].as_u64() >= Some(at + 30_000));
    };
    let loop_run = |app: &mut App, as_takes: bool| {
        app.call(
            "command.run",
            json!({"id": "transport.return_to_start", "window": w}),
        );
        wait_for_tick(app, &w, 0);
        app.call(
            "command.run",
            json!({"id": "transport.record", "window": w}),
        );
        // The defaults: Overdub, and MIDI passes layered unless asked.
        let r = wait_for_record(app, &w, |r| r["spec"].is_object());
        assert_eq!(
            (
                r["spec"]["replace"].clone(),
                r["spec"]["passes_as_takes"].clone()
            ),
            (json!(false), json!(as_takes)),
            "{r}"
        );
        note(app, 1, "3e");
        note(app, 2, "40");
        app.call(
            "command.run",
            json!({"id": "transport.record", "window": w}),
        );
        wait_for_record(app, &w, |r| r["recording"] == false);
        wait_for_undo(app, &w, "Record");
    };

    // Layering: two passes, one note each, on the one take; one Undo.
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    assert_eq!(lane_named(&mut app, &w, "Keys")["arm"], true);
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"loop": true, "pre_roll": 0}, "window": w}),
    );
    loop_run(&mut app, false);
    let lane = lane_named(&mut app, &w, "Keys");
    assert_eq!(
        (lane["takes"].clone(), lane["events"].clone()),
        (json!(1), json!(4)),
        "{lane}"
    );
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    assert_eq!(lane_named(&mut app, &w, "Keys")["events"], 2);

    // Takes: with MIDI passes as takes on, two passes make Take 2 and 3.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"passes_as_takes": true}, "window": w}),
    );
    loop_run(&mut app, true);
    let lane = lane_named(&mut app, &w, "Keys");
    assert_eq!(
        (lane["takes"].clone(), lane["active_take"].clone()),
        (json!(3), json!("Take 3")),
        "{lane}"
    );
    let take = |lane: &Value, name: &str| -> Value {
        lane["take_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no take {name} in {lane}"))["id"]
            .clone()
    };
    let (t1, t3) = (take(&lane, "Take 1"), take(&lane, "Take 3"));
    let r = app.call(
        "command.run",
        json!({"id": "track.choose_take", "params": {"lane": keys_id, "take": t1}, "window": w}),
    );
    assert_eq!(r["status"], "Take 1", "{r}");
    assert_eq!(r["undo"], "Set Active Take", "{r}");
    assert_eq!(lane_named(&mut app, &w, "Keys")["events"], 2);
    let r = app.call(
        "command.run",
        json!({"id": "track.rename_take", "params": {"lane": keys_id, "take": t1, "name": "Verse"}, "window": w}),
    );
    assert_eq!(r["status"], "Verse", "{r}");
    app.call(
        "command.run",
        json!({"id": "track.delete_take", "params": {"lane": keys_id, "take": t3}, "window": w}),
    );
    assert_eq!(lane_named(&mut app, &w, "Keys")["takes"], 2);
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    assert_eq!(lane_named(&mut app, &w, "Keys")["takes"], 3);
    let e = app.call_err(
        "command.run",
        json!({"id": "track.delete_take", "params": {"lane": keys_id, "take": 999_999}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::UnknownId, "{e:?}");
    app.quit();
}

/// `vizdaw-mcp` in the app bundle, spoken to over its pipes (plan 8a).
struct McpProc {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    out: std::sync::mpsc::Receiver<String>,
    next: i64,
}

impl McpProc {
    /// Spawns it with the launch's support folder and home, and sends
    /// `initialize` with `capabilities`, then `initialized`.
    fn start(app: &App, capabilities: Value) -> McpProc {
        McpProc::start_named(app, capabilities, "e2e")
    }

    /// [`McpProc::start`] with the `clientInfo` name the host calls itself.
    fn start_named(app: &App, capabilities: Value, client: &str) -> McpProc {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let exe = std::env::var("VIZDAW_E2E_APP").expect("VIZDAW_E2E_APP");
        let exe = PathBuf::from(exe);
        let exe = if exe.is_absolute() {
            exe
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(exe)
        };
        let bundle = if exe.extension().is_some_and(|e| e == "app") {
            exe
        } else {
            exe.ancestors()
                .nth(3)
                .expect("the app bundle")
                .to_path_buf()
        };
        let mcp = bundle.join("Contents/MacOS/vizdaw-mcp");
        assert!(mcp.exists(), "{} is in the app", mcp.display());
        let mut child = Command::new(mcp)
            .env("HOME", &app.launched.home)
            .env("VIZDAW_SUPPORT_DIR", app.launched.home.join("support"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, out) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        let mut m = McpProc {
            stdin: child.stdin.take(),
            child,
            out,
            next: 1,
        };
        let r = m.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": capabilities,
                "clientInfo": {"name": client, "version": "1"}}),
        );
        assert!(r.get("error").is_none(), "{r}");
        m.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        m
    }

    fn send(&mut self, v: &Value) {
        use std::io::Write;
        let w = self.stdin.as_mut().unwrap();
        writeln!(w, "{v}").unwrap();
        w.flush().unwrap();
    }

    fn recv(&self) -> Value {
        let line = self
            .out
            .recv_timeout(Duration::from_millis(WAIT_MS))
            .expect("vizdaw-mcp answered");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"))
    }

    /// Sends a request and returns its reply.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let r = self.recv();
        assert_eq!(r["id"], id, "{r}");
        r
    }

    /// A tool call: its result (`content`, `isError`).
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let r = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert!(r.get("error").is_none(), "{name}: {r}");
        r["result"].clone()
    }

    /// A tool call that must answer without an error.
    fn ok(&mut self, name: &str, arguments: Value) -> String {
        let r = self.tool(name, arguments);
        assert_eq!(r["isError"], false, "{name}: {r}");
        mcp_text(&r)
    }
}

impl Drop for McpProc {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn mcp_text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{result}"))
        .to_string()
}

/// Waits (a condition, not a sleep) until `path` exists.
fn wait_for_path(path: &std::path::Path, present: bool) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    while path.exists() != present {
        assert!(
            Instant::now() < deadline,
            "{} did not {} within {WAIT_MS} ms",
            path.display(),
            if present { "appear" } else { "go" }
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What `settings.co_producer_answer` says when no sheet is open.
const NO_SHEET: &str = "No Co-Producer approval is waiting.";

/// One try at the test-only hook (plan 8b Ruling 11) over the developer
/// channel: the sheet's answer, or the app's refusal.
fn try_answer(app: &mut App, params: Value) -> Result<Value, core_control::ControlError> {
    match app.client.call(
        "command.run",
        json!({"id": "settings.co_producer_answer", "params": params}),
        Duration::from_secs(30),
    ) {
        Ok(v) => Ok(v),
        Err(CallError::Reply(e)) => Err(e),
        Err(e) => panic!("settings.co_producer_answer: {e:?}"),
    }
}

/// Answers VizDAW's approval sheet through the hook, never a GUI. The
/// sheet opens a moment after the apply was sent, so a "no sheet" answer
/// is retried every 100 ms for up to 10 s.
fn answer_sheet_with(app: &mut App, params: Value) -> Result<Value, core_control::ControlError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match try_answer(app, params.clone()) {
            Err(e) if e.message == NO_SHEET && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

/// [`answer_sheet_with`] for a plain Allow or Don't Allow; panics on a refusal.
fn answer_sheet(app: &mut App, allow: bool) -> Value {
    answer_sheet_with(app, json!({"allow": allow}))
        .unwrap_or_else(|e| panic!("answering the sheet: {e:?}"))
}

/// No sheet is open: the hook says so at once.
fn assert_no_sheet(app: &mut App) {
    let e = try_answer(app, json!({"allow": false})).expect_err("a sheet is open");
    assert_eq!(e.code, core_control::ErrorCode::Failed, "{e:?}");
    assert_eq!(e.message, NO_SHEET, "{e:?}");
}

impl McpProc {
    /// Sends a tool call without waiting, and returns the request's ID.
    fn send_tool(&mut self, name: &str, arguments: Value) -> i64 {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}}));
        id
    }

    /// The reply to `id`, as a tool result.
    fn reply(&self, id: i64) -> Value {
        let r = self.recv();
        assert_eq!(r["id"], id, "{r}");
        assert!(r.get("error").is_none(), "{r}");
        r["result"].clone()
    }
}

/// Plan 8a README steps 334-344 through `vizdaw-mcp`: off by default, the
/// socket and its modes, one undo step and one copy per request, the
/// token flow, the socket's refusals, elicitation, Undo from VizDAW, and
/// off again. Everything stays in the throwaway home.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_over_mcp() {
    use core_control::ErrorCode;
    use std::os::unix::fs::PermissionsExt;
    const OFF: &str = "Co-Producer is turned off in VizDAW's Settings.";
    let mut app = App::start();
    let home = app.launched.home.clone();
    let support = home.join("support");
    let sock = support.join("co-producer.sock");
    let mut mcp = McpProc::start(&app, json!({}));

    // Off on a fresh install: a tool call says so (step 334).
    let r = mcp.tool("begin_request", json!({"name": "add two tracks"}));
    assert_eq!(mcp_text(&r), OFF, "{r}");
    assert!(!sock.exists());

    // On: the socket exists at 0600 in a 0700 folder.
    let r = app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    assert!(r.is_object(), "{r}");
    wait_for_path(&sock, true);
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&sock), 0o600);
    assert_eq!(mode(&support), 0o700);

    // The song, saved so its copies have a package.
    let made = app.call("command.run", json!({"id": "file.new"}));
    let w = made["window"].clone();
    let song = home.join("Song.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": song.display().to_string()}, "window": w}),
    );
    let doc = |app: &mut App| app.call("state.get", json!({"target": "document", "window": w}));
    let lanes = |app: &mut App| -> Vec<Value> { doc(app)["lanes"].as_array().unwrap().clone() };

    // A request in the initialize era, no elicitation (step 335).
    mcp.ok("begin_request", json!({"name": "add two tracks"}));
    mcp.ok("track_new_audio", json!({"window": w}));
    mcp.ok("track_new_audio", json!({"window": w}));
    mcp.ok("end_request", json!({}));
    wait_for_undo(&mut app, &w, "Co-Producer: Add Two Tracks");
    assert_eq!(lanes(&mut app).len(), 2);
    let copies = song.join("Co-Producer/Copies");
    let count = |p: &std::path::Path| std::fs::read_dir(p).map(|d| d.count()).unwrap_or(0);
    assert_eq!(count(&copies), 1, "one copy per request");

    // The two tracks Claude added are marked and named "(Co-Producer)"
    // (plan 8b Ruling 2); renaming those needs no yes. The track the
    // composer makes over the developer channel is theirs, so changing
    // it still needs one.
    assert!(lanes(&mut app).iter().all(
        |l| l["co_producer"] == true && l["name"].as_str().unwrap().ends_with(" (Co-Producer)")
    ));
    app.call("command.run", json!({"id": "track.new_audio", "window": w}));
    let d = doc(&mut app);
    let seq = d["selected_seq"].clone();
    let first = lanes(&mut app)
        .iter()
        .find(|l| l["co_producer"] == false)
        .expect("the composer's own track")["id"]
        .clone();
    assert_eq!(lanes(&mut app).len(), 3);
    let rename =
        json!([{"id": "track.rename", "params": {"seq": seq, "lane": first, "name": "Kick"}}]);
    mcp.ok("begin_request", json!({"name": "rename the first track"}));
    let listed = mcp.ok("change", json!({"commands": rename, "window": w}));
    let token = listed
        .split_whitespace()
        .skip_while(|t| *t != "token")
        .nth(1)
        .unwrap_or_else(|| panic!("no token in {listed}"))
        .trim_end_matches(['.', ','])
        .to_string();
    assert_eq!(token.len(), 32, "{listed}");
    assert!(lanes(&mut app).iter().all(|l| l["name"] != "Kick"));
    // A bad token is refused at once, with no sheet (Ruling 11).
    let r = mcp.tool(
        "apply_change",
        json!({"token": "0".repeat(32), "commands": rename, "window": w}),
    );
    assert_eq!(r["isError"], true, "{r}");
    assert_no_sheet(&mut app);
    assert!(lanes(&mut app).iter().all(|l| l["name"] != "Kick"));
    // The composer's track needs VizDAW's yes: the sheet is answered
    // through the test hook.
    let id = mcp.send_tool(
        "apply_change",
        json!({"token": token, "commands": rename, "window": w}),
    );
    answer_sheet(&mut app, true);
    let r = mcp.reply(id);
    assert_eq!(r["isError"], false, "{r}");
    assert!(lanes(&mut app).iter().any(|l| l["name"] == "Kick"));
    let r = mcp.tool(
        "apply_change",
        json!({"token": token, "commands": rename, "window": w}),
    );
    assert_eq!(r["isError"], true, "the token is used up: {r}");
    assert_no_sheet(&mut app);
    mcp.ok("end_request", json!({}));
    wait_for_undo(&mut app, &w, "Co-Producer: Rename the First Track");

    // The socket's own gate (steps 339 and 340).
    let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
    let exists = home.join("exists.mid");
    std::fs::write(&exists, b"keep").unwrap();
    let t = Duration::from_secs(30);
    for (cmd, params) in [
        (
            "ui.click",
            json!({"window": w, "element": {"x": 1, "y": 1}}),
        ),
        ("command.run", json!({"id": "media.compact", "window": w})),
        (
            "command.run",
            json!({"id": "track.delete", "params": {"seq": seq, "lane": first}, "window": w}),
        ),
        (
            "command.run",
            json!({"id": "file.export_midi", "params": {"path": exists.display().to_string()}, "window": w}),
        ),
    ] {
        match raw.call(cmd, params.clone(), t) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd} {params}: {e:?}")
            }
            other => panic!("{cmd} {params}: expected not_allowed, got {other:?}"),
        }
    }
    assert_eq!(std::fs::read(&exists).unwrap(), b"keep");
    drop(raw);

    // A second process that can ask the composer (step 337).
    let mut ask = McpProc::start(&app, json!({"elicitation": {}}));
    let kick = first.clone();
    let delete = json!([{"id": "track.delete", "params": {"seq": seq, "lane": kick}}]);
    ask.ok("begin_request", json!({"name": "delete the kick"}));
    let answer = |ask: &mut McpProc, app: &mut App, id: i64, result: Value| -> Value {
        ask.send(
            &json!({"jsonrpc": "2.0", "id": 1000 + id, "method": "tools/call",
            "params": {"name": "change", "arguments": {"commands": delete, "window": w}}}),
        );
        let e = ask.recv();
        assert_eq!(e["method"], "elicitation/create", "{e}");
        let accepted = result["action"] == "accept";
        ask.send(&json!({"jsonrpc": "2.0", "id": e["id"], "result": result}));
        if accepted {
            // The composer said yes to Claude's question; VizDAW asks too.
            answer_sheet(app, true);
        }
        let r = ask.recv();
        assert_eq!(r["id"], 1000 + id, "{r}");
        r["result"].clone()
    };
    answer(&mut ask, &mut app, 1, json!({"action": "decline"}));
    assert_eq!(lanes(&mut app).len(), 3, "declined: the track stays");
    assert_no_sheet(&mut app);
    let r = answer(
        &mut ask,
        &mut app,
        2,
        json!({"action": "accept", "content": {"allow": true}}),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(lanes(&mut app).len(), 2, "one step deletes it");
    ask.ok("end_request", json!({}));
    wait_for_undo(&mut app, &w, "Co-Producer: Delete the Kick");
    drop(ask);

    // Undo from VizDAW brings the track back.
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    let back = lanes(&mut app);
    assert_eq!(back.len(), 3, "{back:?}");
    assert!(back.iter().any(|l| l["name"] == "Kick"), "{back:?}");

    // Off again: the next call says so, and the socket file goes.
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": false}}),
    );
    wait_for_path(&sock, false);
    let r = mcp.tool("begin_request", json!({"name": "one more"}));
    assert_eq!(mcp_text(&r), OFF, "{r}");
    drop(mcp);
    app.quit();
}

/// Today's local date, "YYYY-MM-DD", as the app writes it in an export
/// folder's name.
fn today() -> String {
    let out = std::process::Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .expect("date");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// The lane named `name`'s ID, or the first lane with `co_producer` set to
/// `mark`.
fn lane_marked(app: &mut App, window: &Value, mark: bool) -> Value {
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["co_producer"] == mark)
        .unwrap_or_else(|| panic!("no lane marked {mark} in {doc}"))
        .clone()
}

/// Plan 8b's notes file, marks, dated export folders, Make Mine, the
/// untitled song's move-in on first save, and the socket's gate, through
/// `vizdaw-mcp`. Everything stays in the throwaway home.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_librarian_over_mcp() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let home = app.launched.home.clone();
    let support = home.join("support");
    let sock = support.join("co-producer.sock");
    let day = today();
    let mut mcp = McpProc::start(&app, json!({}));
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    wait_for_path(&sock, true);

    let made = app.call("command.run", json!({"id": "file.new"}));
    let w = made["window"].clone();
    let song = home.join("Song.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": song.display().to_string()}, "window": w}),
    );
    let doc = |app: &mut App| app.call("state.get", json!({"target": "document", "window": w}));
    let lanes = |app: &mut App| -> Vec<Value> { doc(app)["lanes"].as_array().unwrap().clone() };

    // The first request: the notes come back with the first answer, and
    // one entry is written when it ends (README steps 1-2).
    let begun = mcp.ok(
        "begin_request",
        json!({"name": "add a bus", "asked": "give me a reverb bus", "window": w}),
    );
    assert!(begun.contains("The song's notes (Notes.md):"), "{begun}");
    assert!(begun.contains("# Song"), "{begun}");
    mcp.ok("track_new_audio", json!({"window": w}));
    mcp.ok("add_note", json!({"text": "## fake\nkeep it dark"}));
    mcp.ok("end_request", json!({}));
    wait_for_undo(&mut app, &w, "Co-Producer: Add a Bus");
    let notes_file = song.join("Notes.md");
    let text = std::fs::read_to_string(&notes_file).unwrap();
    let heading = text
        .lines()
        .find(|l| l.starts_with(&format!("## {day} ")))
        .unwrap_or_else(|| panic!("no entry for {day} in {text}"));
    assert!(heading.ends_with("— Add a Bus"), "{heading}");
    for line in [
        "- Asked: \"give me a reverb bus\"",
        "- Note: ## fake",
        "- Note: keep it dark",
        "- Undo step: \"Co-Producer: Add a Bus\"",
    ] {
        assert!(
            text.lines().any(|l| l == line),
            "{line:?} missing in {text}"
        );
    }
    assert_eq!(
        text.lines().filter(|l| l.starts_with("## ")).count(),
        1,
        "the note's own `##` never makes a heading: {text}"
    );
    assert_eq!(mcp.ok("read_notes", json!({"window": w})), text);
    let made_lane = lane_marked(&mut app, &w, true);
    assert!(
        made_lane["name"]
            .as_str()
            .unwrap()
            .ends_with(" (Co-Producer)"),
        "{made_lane}"
    );

    // Tidying its own track needs no yes: colour, then delete.
    let seq = doc(&mut app)["selected_seq"].clone();
    let strip = app.call("state.get", json!({"target": "mixer", "window": w}))["strips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["co_producer"] == true)
        .unwrap_or_else(|| panic!("no marked strip"))["id"]
        .clone();
    mcp.ok("begin_request", json!({"name": "tidy up", "window": w}));
    let r = mcp.ok(
        "change",
        json!({"commands": [
            {"id": "mixer.set_colour", "params": {"strip": strip, "colour": "red"}},
            {"id": "track.delete", "params": {"seq": seq, "lane": made_lane["id"]}},
        ], "window": w}),
    );
    assert!(!r.contains("token"), "applied at once: {r}");
    assert!(lanes(&mut app).is_empty(), "the track is gone");
    mcp.ok("end_request", json!({}));

    // Dated export folders: the parent given, then remembered.
    mcp.ok(
        "begin_request",
        json!({"name": "export stems", "window": w}),
    );
    let first = mcp.ok(
        "export_folder",
        json!({"what": "stems", "parent": home.display().to_string()}),
    );
    assert_eq!(
        first,
        home.join(format!("Song — Stems — {day}"))
            .display()
            .to_string()
    );
    assert!(std::path::Path::new(&first).is_dir());
    let second = mcp.ok("export_folder", json!({"what": "stems"}));
    assert_eq!(second, format!("{first} 2"));
    assert!(std::path::Path::new(&second).is_dir());
    let r = mcp.tool(
        "export_folder",
        json!({"what": "midi", "parent": song.display().to_string()}),
    );
    assert_eq!(r["isError"], true, "inside the package: {r}");
    mcp.ok("end_request", json!({}));

    // Make Mine and Song Notes over the developer channel.
    mcp.ok("begin_request", json!({"name": "one more", "window": w}));
    mcp.ok("track_new_audio", json!({"window": w}));
    mcp.ok("end_request", json!({}));
    let theirs = lane_marked(&mut app, &w, true);
    assert!(theirs["name"].as_str().unwrap().ends_with(" (Co-Producer)"));
    app.call(
        "command.run",
        json!({"id": "track.make_mine", "params": {"seq": seq, "lane": theirs["id"]}, "window": w}),
    );
    let mine = lanes(&mut app)
        .into_iter()
        .find(|l| l["id"] == theirs["id"])
        .unwrap();
    assert_eq!(mine["co_producer"], false, "{mine}");
    assert!(
        !mine["name"].as_str().unwrap().contains("(Co-Producer)"),
        "{mine}"
    );
    let ran = app.call(
        "command.run",
        json!({"id": "window.song_notes", "window": w}),
    );
    assert_eq!(ran["status"], notes_file.display().to_string(), "{ran}");
    assert!(notes_file.exists());

    // The Co-Producer socket's gate.
    let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
    let t = Duration::from_secs(30);
    for (cmd, params) in [
        (
            "command.run",
            json!({"id": "track.make_mine", "params": {"seq": seq, "lane": theirs["id"]}, "window": w}),
        ),
        (
            "command.run",
            json!({"id": "window.song_notes", "window": w}),
        ),
        (
            "command.run",
            json!({"id": "mixer.set_colour", "params": {"strip": strip, "colour": "blue"}, "window": w}),
        ),
    ] {
        match raw.call(cmd, params.clone(), t) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd} {params}: {e:?}")
            }
            other => panic!("{cmd} {params}: expected not_allowed, got {other:?}"),
        }
    }
    drop(raw);

    // An untitled song: notes and a copy wait in a stand-in folder, and
    // move into the package on its first save.
    let made = app.call("command.run", json!({"id": "file.new"}));
    let w2 = made["window"].clone();
    mcp.ok(
        "begin_request",
        json!({"name": "add a track", "window": w2}),
    );
    mcp.ok("track_new_audio", json!({"window": w2}));
    mcp.ok("end_request", json!({}));
    let untitled = support.join("Co-Producer/Untitled");
    wait_for_path(&untitled, true);
    let stand_ins: Vec<_> = std::fs::read_dir(&untitled)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(stand_ins.len(), 1, "{stand_ins:?}");
    let stand_in = &stand_ins[0];
    assert!(stand_in.join("Notes.md").is_file());
    let count = |p: &std::path::Path| std::fs::read_dir(p).map(|d| d.count()).unwrap_or(0);
    assert_eq!(count(&stand_in.join("Co-Producer/Copies")), 1, "one copy");
    let second_song = home.join("Second.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": second_song.display().to_string()}, "window": w2}),
    );
    assert!(second_song.join("Notes.md").is_file());
    assert_eq!(count(&second_song.join("Co-Producer/Copies")), 1);
    wait_for_path(stand_in, false);
    drop(mcp);
    app.quit();
}

/// Plan 8b Ruling 11 against the built app: VizDAW's own approval sheet,
/// answered through the test hook, and the approvals it keeps in memory.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_approval_over_mcp() {
    const DENIED: &str = "The composer did not allow this in VizDAW.";
    const WARNING: &str = "can't check which program";
    let mut app = App::start_with(&[("VIZDAW_APPROVAL_WAIT_MS", "2000")]);
    let home = app.launched.home.clone();
    let sock = home.join("support").join("co-producer.sock");
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    wait_for_path(&sock, true);
    let made = app.call("command.run", json!({"id": "file.new"}));
    let w = made["window"].clone();
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": home.join("Song.vizdaw").display().to_string()}, "window": w}),
    );
    // The composer's own track, made over the developer channel.
    app.call("command.run", json!({"id": "track.new_audio", "window": w}));
    let seq =
        app.call("state.get", json!({"target": "document", "window": w}))["selected_seq"].clone();
    let mine = lane_marked(&mut app, &w, false);
    let lane = mine["id"].clone();
    let name_of = |app: &mut App| -> String {
        let d = app.call("state.get", json!({"target": "document", "window": w}));
        d["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == lane)
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let original = name_of(&mut app);
    let rename = |name: &str| json!([{"id": "track.rename", "params": {"seq": seq, "lane": lane, "name": name}}]);
    let t = Duration::from_secs(60);
    let me = std::env::current_exe().unwrap();

    // A raw client on the Co-Producer socket, no click: Don't Allow.
    let raw_apply = |app: &mut App, name: &str, allow: bool| -> Value {
        let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
        raw.call(
            "co_producer.begin",
            json!({"name": "rename", "window": w}),
            t,
        )
        .unwrap();
        let pre = raw
            .call(
                "co_producer.preview",
                json!({"commands": rename(name), "window": w}),
                t,
            )
            .unwrap();
        assert_eq!(pre["needs_yes"], true, "{pre}");
        let token = pre["token"].as_str().unwrap().to_string();
        let commands = rename(name);
        let window = w.clone();
        let used = token.clone();
        let worker = std::thread::spawn(move || {
            let r = raw.call(
                "co_producer.apply",
                json!({"token": token, "commands": commands, "window": window}),
                Duration::from_secs(90),
            );
            (raw, r)
        });
        let asked = answer_sheet(app, allow);
        let (mut raw, r) = worker.join().unwrap();
        if allow {
            r.unwrap();
        } else {
            match r {
                Err(CallError::Reply(e)) => assert_eq!(e.message, DENIED, "{e:?}"),
                other => panic!("expected the no, got {other:?}"),
            }
            // The used token is refused at once, with no sheet.
            let again = raw.call(
                "co_producer.apply",
                json!({"token": used, "commands": rename(name), "window": w}),
                t,
            );
            assert!(matches!(again, Err(CallError::Reply(_))), "{again:?}");
            assert_no_sheet(app);
        }
        raw.call("co_producer.end", json!({"window": w}), t)
            .unwrap();
        asked
    };
    let asked = raw_apply(&mut app, "Raw", false);
    assert_eq!(asked["kind"], "unverified", "{asked}");
    assert_eq!(
        std::path::Path::new(asked["path"].as_str().unwrap()).file_name(),
        me.file_name(),
        "the test binary's path: {asked}"
    );
    assert!(
        asked["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["change"].as_str().unwrap_or("").contains("Raw")),
        "the rename is listed: {asked}"
    );
    assert_eq!(name_of(&mut app), original, "the track keeps its name");

    // A forged label asks for the same rename; always allow needs the
    // warning confirmed.
    let mut helper = McpProc::start_named(&app, json!({}), "Helper Chat");
    let ask = |p: &mut McpProc, name: &str| -> i64 {
        p.ok("begin_request", json!({"name": "rename", "window": w}));
        let listed = p.ok("change", json!({"commands": rename(name), "window": w}));
        let token = listed
            .split_whitespace()
            .skip_while(|t| *t != "token")
            .nth(1)
            .unwrap_or_else(|| panic!("no token in {listed}"))
            .trim_end_matches(['.', ','])
            .to_string();
        p.send_tool(
            "apply_change",
            json!({"token": token, "commands": rename(name), "window": w}),
        )
    };
    let id = ask(&mut helper, "Snare");
    let e = answer_sheet_with(&mut app, json!({"allow": true, "always": true}))
        .expect_err("the warning comes first");
    assert_eq!(e.code, core_control::ErrorCode::NotAllowed, "{e:?}");
    assert!(e.message.contains(WARNING), "{e:?}");
    assert_eq!(
        name_of(&mut app),
        original,
        "nothing applied; the sheet stays"
    );
    let asked = answer_sheet_with(
        &mut app,
        json!({"allow": true, "always": true, "confirm": true}),
    )
    .expect("confirmed");
    assert_eq!(asked["kind"], "unverified", "{asked}");
    assert_eq!(asked["label"], "Helper Chat", "{asked}");
    assert_eq!(helper.reply(id)["isError"], false);
    assert_eq!(name_of(&mut app), "Snare");
    helper.ok("end_request", json!({}));

    // That unverified approval is used, and contained to its own key.
    let id = ask(&mut helper, "Tom");
    assert_eq!(helper.reply(id)["isError"], false, "applies with no sheet");
    assert_no_sheet(&mut app);
    assert_eq!(name_of(&mut app), "Tom");
    helper.ok("end_request", json!({}));
    let mut other = McpProc::start_named(&app, json!({}), "Other Chat");
    let id = ask(&mut other, "Floor");
    let asked = answer_sheet(&mut app, false);
    assert_eq!(asked["label"], "Other Chat", "{asked}");
    assert_eq!(other.reply(id)["isError"], true);
    other.ok("end_request", json!({}));
    let asked = raw_apply(&mut app, "Raw2", false);
    assert_eq!(asked["kind"], "unverified", "{asked}");
    assert_eq!(name_of(&mut app), "Tom");

    // No yes needed, no sheet: Claude renames a track it added.
    other.ok("begin_request", json!({"name": "pad", "window": w}));
    other.ok("track_new_audio", json!({"window": w}));
    let added = lane_marked(&mut app, &w, true);
    let r = other.ok(
        "change",
        json!({"commands": [{"id": "track.rename",
            "params": {"seq": seq, "lane": added["id"], "name": "Pad"}}], "window": w}),
    );
    assert!(!r.contains("token"), "applied at once: {r}");
    assert_no_sheet(&mut app);
    assert!(
        lane_marked(&mut app, &w, true)["name"]
            .as_str()
            .unwrap()
            .starts_with("Pad")
    );
    other.ok("end_request", json!({}));

    // Unanswered: the shortened wait is a no, and nothing changes.
    let id = ask(&mut other, "Never");
    let begun = Instant::now();
    let r = other.reply(id);
    let waited = begun.elapsed();
    assert_eq!(r["isError"], true, "{r}");
    assert!(mcp_text(&r).contains(DENIED), "{r}");
    assert!(
        waited >= Duration::from_millis(1500) && waited < Duration::from_secs(30),
        "about 2 s: {waited:?}"
    );
    assert_eq!(name_of(&mut app), "Tom");
    other.ok("end_request", json!({}));

    drop(helper);
    drop(other);
    let left = std::process::Command::new("pgrep")
        .args(["-lf", "vizdaw-mcp"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&left.stdout).trim().is_empty(),
        "{}",
        String::from_utf8_lossy(&left.stdout)
    );
    app.quit();
}

/// Plan 8c README steps 383-393 through `vizdaw-mcp`: one-call exports into
/// dated folders, the note and marker chores with the composer's yes, a
/// bounce placed on new tracks, the track list view, and the socket's gate.
/// Everything stays in the throwaway home.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_chores_over_mcp() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let home = app.launched.home.clone();
    let sock = home.join("support").join("co-producer.sock");
    let day = today();
    let mut mcp = McpProc::start(
        &app,
        json!({"elicitation": {},
            "extensions": {"io.modelcontextprotocol/ui": {"mimeTypes": ["text/html;profile=mcp-app"]}}}),
    );
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    wait_for_path(&sock, true);
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let song = home.join("Song.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": song.display().to_string()}, "window": w}),
    );
    let doc = |app: &mut App| app.call("state.get", json!({"target": "document", "window": w}));
    let lanes = |app: &mut App| -> Vec<Value> { doc(app)["lanes"].as_array().unwrap().clone() };
    let seq = doc(&mut app)["selected_seq"].clone();
    let demo_tracks = lanes(&mut app).len();
    let names = |dir: &std::path::Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    let notes_file = song.join("Notes.md");

    // "Export all tracks as a DAWproject" (README step 383).
    mcp.ok(
        "begin_request",
        json!({"name": "export all tracks as a dawproject",
            "asked": "export all tracks as a dawproject", "window": w}),
    );
    let said = mcp.ok(
        "export",
        json!({"what": "dawproject", "parent": home.display().to_string()}),
    );
    mcp.ok("end_request", json!({}));
    let dp_folder = home.join(format!("Song \u{2014} DAWproject \u{2014} {day}"));
    let dp = dp_folder.join("Song.dawproject");
    assert!(std::fs::metadata(&dp).unwrap().len() > 0, "{dp:?}");
    assert!(said.contains(&dp.display().to_string()), "{said}");
    let text = std::fs::read_to_string(&notes_file).unwrap();
    assert!(
        text.lines()
            .any(|l| l.starts_with("- Files: ") && l.contains("Song.dawproject")),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l == format!("- Export folder: {}", home.display())),
        "{text}"
    );

    // "Bounce to stems": no parent, twice (README step 384).
    let before = names(&home);
    mcp.ok("begin_request", json!({"name": "bounce to stems"}));
    let one = mcp.tool("export", json!({"what": "stems"}));
    assert_eq!(one["isError"], false, "{one}");
    assert_eq!(one["structuredContent"]["view"], "export", "{one}");
    mcp.ok("export", json!({"what": "stems"}));
    mcp.ok("end_request", json!({}));
    let stems = home.join(format!("Song \u{2014} Stems \u{2014} {day}"));
    let stems2 = home.join(format!("Song \u{2014} Stems \u{2014} {day} 2"));
    assert!(stems.is_dir() && stems2.is_dir());
    let after = names(&home);
    let new: Vec<_> = after.iter().filter(|n| !before.contains(n)).collect();
    assert_eq!(new.len(), 2, "only the two stems folders are new: {new:?}");
    assert!(
        before.iter().all(|n| after.contains(n)),
        "nothing else went: {before:?} {after:?}"
    );
    let audio = |dir: &std::path::Path| -> usize {
        names(dir)
            .into_iter()
            .filter(|n| {
                [".wav", ".aif", ".aiff", ".caf"]
                    .iter()
                    .any(|e| n.ends_with(e))
            })
            .count()
    };
    assert!(audio(&stems) > 0);
    assert_eq!(audio(&stems), names(&stems).len(), "audio only");
    assert_eq!(names(&stems2).len(), names(&stems).len());
    // `default_strips`' count is what the For Another DAW export writes.
    let probe = home.join("Probe");
    std::fs::create_dir(&probe).unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "file.export_for_daw", "params": {"path": probe}, "window": w}),
    );
    let folder = PathBuf::from(r["written"][0].as_str().unwrap());
    assert_eq!(wavs_in(&folder.join("Audio")).len(), names(&stems).len());

    // A note chore with a yes (README steps 387-388).
    let track = lanes(&mut app)
        .into_iter()
        .find(|l| l["events"].as_u64().unwrap_or(0) > 0)
        .unwrap_or_else(|| panic!("a demo MIDI track with notes: {}", doc(&mut app)["lanes"]));
    let transpose = json!([{"id": "modify.transpose_notes",
        "params": {"seq": seq, "lanes": track["id"].to_string(), "semitones": 2}}]);
    mcp.ok("begin_request", json!({"name": "transpose the bass"}));
    let ask = |mcp: &mut McpProc, app: &mut App, n: i64, commands: &Value, result: Value| {
        mcp.send(
            &json!({"jsonrpc": "2.0", "id": 2000 + n, "method": "tools/call",
            "params": {"name": "change", "arguments": {"commands": commands, "window": w}}}),
        );
        let e = mcp.recv();
        assert_eq!(e["method"], "elicitation/create", "{e}");
        let message = e["params"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let accepted = result["action"] == "accept";
        mcp.send(&json!({"jsonrpc": "2.0", "id": e["id"], "result": result}));
        if accepted {
            answer_sheet(app, true);
        }
        let r = mcp.recv();
        assert_eq!(r["id"], 2000 + n, "{r}");
        (message, r["result"].clone())
    };
    let v0 = doc(&mut app)["version"].clone();
    let (message, r) = ask(
        &mut mcp,
        &mut app,
        1,
        &transpose,
        json!({"action": "decline"}),
    );
    let line = message
        .lines()
        .find(|l| l.starts_with("- Track "))
        .unwrap_or_else(|| panic!("no track line in {message}"));
    assert!(line.ends_with(" notes changed (pitch)"), "{message}");
    assert_eq!(
        mcp_text(&r),
        "The composer said no. Nothing changed.",
        "{r}"
    );
    assert_eq!(r["structuredContent"]["state"], "said_no", "{r}");
    assert_eq!(doc(&mut app)["version"], v0, "nothing changed");
    assert_no_sheet(&mut app);
    let (_, r) = ask(
        &mut mcp,
        &mut app,
        2,
        &transpose,
        json!({"action": "accept", "content": {"allow": true}}),
    );
    assert_eq!(r["isError"], false, "{r}");
    mcp.ok("end_request", json!({}));
    wait_for_undo(&mut app, &w, "Co-Producer: Transpose the Bass");

    // Markers (README step 389).
    mcp.ok("begin_request", json!({"name": "mark the song"}));
    mcp.ok(
        "marker_add",
        json!({"seq": seq, "tick": 0, "name": "Intro", "window": w}),
    );
    let rename = json!([{"id": "marker.rename",
        "params": {"seq": seq, "index": 1, "name": "Top"}}]);
    let (_, r) = ask(
        &mut mcp,
        &mut app,
        3,
        &rename,
        json!({"action": "accept", "content": {"allow": true}}),
    );
    assert_eq!(r["isError"], false, "{r}");
    mcp.ok("end_request", json!({}));
    let markers = doc(&mut app)["markers"].clone();
    assert_eq!(markers[0]["name"], "Top", "{markers}");

    // A bounce placed on new tracks (README step 390).
    let strip =
        app.call("state.get", json!({"target": "mixer", "window": w}))["strips"][0]["id"].clone();
    let tracks_before = lanes(&mut app).len();
    mcp.ok(
        "begin_request",
        json!({"name": "bounce a stem onto a track"}),
    );
    mcp.ok(
        "file_bounce",
        json!({"source": "stems", "strips": strip.to_string(),
            "path": home.display().to_string(), "add_to_project": true, "window": w}),
    );
    mcp.ok("end_request", json!({}));
    let now = lanes(&mut app);
    assert_eq!(now.len(), tracks_before + 1, "{now:?}");
    let bounced = now.iter().find(|l| l["co_producer"] == true).unwrap();
    assert!(
        bounced["name"]
            .as_str()
            .unwrap()
            .ends_with(" (Co-Producer)"),
        "{bounced}"
    );
    // The track list view, with the bounced track marked.
    let listed = mcp.request("tools/list", json!({}));
    let show = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "show_tracks")
        .unwrap_or_else(|| panic!("no show_tracks in {listed}"))
        .clone();
    assert_eq!(
        show["_meta"]["ui"]["resourceUri"], "ui://vizdaw/tracks",
        "{show}"
    );
    let page = mcp.request("resources/read", json!({"uri": "ui://vizdaw/tracks"}));
    let html = page["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(html.starts_with("<!DOCTYPE html>"), "{page}");
    let shown = mcp.tool("show_tracks", json!({}));
    let rows = shown["structuredContent"]["tracks"].as_array().unwrap();
    assert_eq!(rows.len(), demo_tracks + 1, "{shown}");
    assert!(rows.iter().any(|t| t["co_producer"] == true), "{shown}");
    // One Undo from VizDAW takes the new track away with its request.
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    assert_eq!(lanes(&mut app).len(), tracks_before);

    // The gate (README step 393): a raw client on the socket.
    let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
    let t = Duration::from_secs(30);
    let two = lanes(&mut app)
        .iter()
        .take(2)
        .map(|l| l["id"].to_string())
        .collect::<Vec<_>>()
        .join(",");
    let exists = home.join("Existing.mid");
    std::fs::write(&exists, b"keep").unwrap();
    for (cmd, params) in [
        (
            "command.run",
            json!({"id": "modify.quantize_notes",
                "params": {"seq": seq, "lanes": "1", "grid": "1/16"}, "window": w}),
        ),
        (
            "command.run",
            json!({"id": "marker.remove", "params": {"seq": seq, "index": 1}, "window": w}),
        ),
        (
            "co_producer.export",
            json!({"what": "midi", "parent": song.display().to_string(), "window": w}),
        ),
        (
            "command.run",
            json!({"id": "file.export_midi_clip",
                "params": {"lanes": two, "path": exists.display().to_string()}, "window": w}),
        ),
    ] {
        match raw.call(cmd, params.clone(), t) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd} {params}: {e:?}")
            }
            other => panic!("{cmd} {params}: expected not_allowed, got {other:?}"),
        }
    }
    assert_eq!(std::fs::read(&exists).unwrap(), b"keep");
    drop(raw);
    drop(mcp);
    app.quit();
}

/// An app whose catalog holds the bundle's Multi-Q, as a scan would record
/// it (the app launches with `--no-scan`, so its catalog starts empty).
fn app_with_multiq() -> App {
    // The app launches with `--no-scan`, so its catalog is empty. Seed the
    // throwaway home's catalog with the bundle's Multi-Q, as a scan would
    // record it, so the built-in EQ is usable. Nothing is loaded here.
    let bundle = {
        let p = PathBuf::from(std::env::var("VIZDAW_E2E_APP").expect("VIZDAW_E2E_APP"));
        let p = if p.is_absolute() {
            p
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(p)
        };
        if p.extension().is_some_and(|e| e == "app") {
            p
        } else {
            p.ancestors().nth(3).expect("the app bundle").to_path_buf()
        }
    };
    App::start_seeded(&[], |support| {
        use core_host::catalog::{Catalog, CatalogEntry, ScanStatus, Source};
        use core_model::mixer::{PluginFormat, PluginRef};
        // The path as the app sees its own: not canonical, so it stays
        // under `bundled::dir()`, which is built from the launch path.
        let clap = bundle.join("Contents/PlugIns/Multi-Q.clap");
        assert!(clap.exists(), "{} is in the app", clap.display());
        let src = Source::clap_bundle(&clap);
        let entry = CatalogEntry {
            source: src.clone(),
            plugin: PluginRef {
                format: PluginFormat::Clap,
                id: core_host::bundled::MULTIQ_CLAP_ID.into(),
                name: "Multi-Q".into(),
                vendor: "Dusk Audio".into(),
                version: "1".into(),
            },
            kind: Some(core_host::PluginKind::Effect),
            info: None,
            status: ScanStatus::Ok,
        };
        let mut catalog = Catalog::new();
        catalog.record(&src, vec![entry]);
        catalog
            .save(&support.join("plugins.json"))
            .expect("the seeded catalog");
    })
}

/// README steps for plan 8d through `vizdaw-mcp`: the plug-in list, a drum
/// bus set up with a decline then an accept, a plug-in parameter, the
/// composer's fader, a plug-in instrument track, the mixer view, and the
/// Co-Producer socket's gate.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_mixer_over_mcp() {
    use core_control::ErrorCode;
    let mut app = app_with_multiq();
    let home = app.launched.home.clone();
    let sock = home.join("support").join("co-producer.sock");
    let mut mcp = McpProc::start(
        &app,
        json!({"elicitation": {},
            "extensions": {"io.modelcontextprotocol/ui": {"mimeTypes": ["text/html;profile=mcp-app"]}}}),
    );
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    wait_for_path(&sock, true);
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let song = home.join("Song.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": song.display().to_string()}, "window": w}),
    );
    // The demo has one strip of its own; a second track gives A and B.
    app.call("command.run", json!({"id": "track.new_audio", "window": w}));
    let mixer = |app: &mut App| app.call("state.get", json!({"target": "mixer", "window": w}));
    let strips = mixer(&mut app)["strips"].as_array().unwrap().clone();
    let mine: Vec<&Value> = strips
        .iter()
        .filter(|s| {
            matches!(s["kind_colour"].as_str(), Some("channel" | "instrument"))
                && s["co_producer"] != true
        })
        .take(2)
        .collect();
    assert_eq!(mine.len(), 2, "two demo strips in {strips:?}");
    let (a, b) = (
        mine[0]["id"].as_u64().unwrap(),
        mine[1]["id"].as_u64().unwrap(),
    );
    let (a_out, b_out) = (mine[0]["output"].clone(), mine[1]["output"].clone());
    let names = [
        mine[0]["name"].as_str().unwrap().to_string(),
        mine[1]["name"].as_str().unwrap().to_string(),
    ];
    let strip_of = |m: &Value, id: u64| -> Value {
        m["strips"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("no strip {id} in {m}"))
            .clone()
    };

    // The plug-in list.
    let effects = mcp.ok("list_plugins", json!({"kind": "effect"}));
    assert!(
        effects
            .lines()
            .any(|l| l == "- multi-q: Multi-Q, built in, effect"),
        "{effects}"
    );
    assert!(
        !effects.contains("MULTIQ_CLAP_ID") && !effects.contains("MULTIQ_AU_ID"),
        "{effects}"
    );
    let instruments = mcp.ok("list_plugins", json!({"kind": "instrument"}));
    assert!(
        instruments
            .lines()
            .any(|l| l == "- built-in-synth: Built-in Synth, built in, instrument"),
        "{instruments}"
    );

    // "Set up a drum bus".
    let ask = |mcp: &mut McpProc, app: &mut App, n: i64, commands: &Value, result: Value| {
        mcp.send(
            &json!({"jsonrpc": "2.0", "id": 3000 + n, "method": "tools/call",
            "params": {"name": "change", "arguments": {"commands": commands, "window": w}}}),
        );
        let e = mcp.recv();
        assert_eq!(e["method"], "elicitation/create", "{e}");
        let message = e["params"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let accepted = result["action"] == "accept";
        mcp.send(&json!({"jsonrpc": "2.0", "id": e["id"], "result": result}));
        if accepted {
            answer_sheet(app, true);
        }
        let r = mcp.recv();
        assert_eq!(r["id"], 3000 + n, "{r}");
        (message, r["result"].clone())
    };
    mcp.ok(
        "begin_request",
        json!({"name": "set up a drum bus", "asked": "put my drums on a bus"}),
    );
    mcp.ok("mixer_new_bus", json!({"name": "Drums", "window": w}));
    let shown = mcp.ok("show_mixer", json!({}));
    let bus_line = shown
        .lines()
        .find(|l| l.contains("Drums (Co-Producer)"))
        .unwrap_or_else(|| panic!("no Drums bus in {shown}"));
    assert!(
        bus_line.ends_with(" \u{2014} added by Co-Producer"),
        "{bus_line}"
    );
    let bus: u64 = bus_line
        .trim_start_matches("- [")
        .split(']')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let r = mcp.tool(
        "change",
        json!({"commands": [{"id": "mixer.add_insert",
            "params": {"strip": bus, "plugin": "multi-q"}}], "window": w}),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(
        r["structuredContent"]["state"], "applied_without_asking",
        "{r}"
    );
    let route = json!([
        {"id": "mixer.set_output", "params": {"strip": a, "bus": bus}},
        {"id": "mixer.set_output", "params": {"strip": b, "bus": bus}},
    ]);
    let (message, r) = ask(&mut mcp, &mut app, 1, &route, json!({"action": "decline"}));
    for name in &names {
        assert!(
            message.contains(&format!(
                "- Strip {name}: output Master \u{2192} Drums (Co-Producer)"
            )),
            "{message}"
        );
    }
    assert_eq!(
        mcp_text(&r),
        "The composer said no. Nothing changed.",
        "{r}"
    );
    let m = mixer(&mut app);
    assert_eq!(strip_of(&m, a)["output"], a_out, "{m}");
    assert_eq!(strip_of(&m, b)["output"], b_out, "{m}");
    let (_, r) = ask(
        &mut mcp,
        &mut app,
        2,
        &route,
        json!({"action": "accept", "content": {"allow": true}}),
    );
    assert_eq!(r["isError"], false, "{r}");
    mcp.ok("end_request", json!({}));
    wait_for_undo(&mut app, &w, "Co-Producer: Set Up a Drum Bus");
    let m = mixer(&mut app);
    assert_eq!(strip_of(&m, bus)["inserts"][0]["plugin"], "multi-q", "{m}");
    assert_eq!(strip_of(&m, a)["output"], bus, "{m}");
    assert_eq!(strip_of(&m, b)["output"], bus, "{m}");
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    let m = mixer(&mut app);
    assert!(
        m["strips"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["id"] != bus),
        "the bus is gone: {m}"
    );
    assert_eq!(strip_of(&m, a)["output"], a_out, "{m}");
    assert_eq!(strip_of(&m, b)["output"], b_out, "{m}");

    // A plug-in parameter on Co-Producer's own bus, after Redo.
    app.call("command.run", json!({"id": "edit.redo", "window": w}));
    let m = mixer(&mut app);
    let slot = strip_of(&m, bus)["inserts"][0]["slot"].as_u64().unwrap();
    mcp.ok("begin_request", json!({"name": "tune the drum bus"}));
    let deadline = Instant::now() + Duration::from_secs(10);
    let params = loop {
        let text = mcp.ok("plugin_params", json!({"slot": slot}));
        if !text.contains("is not running yet") {
            break text;
        }
        assert!(Instant::now() < deadline, "the plug-in never ran: {text}");
        std::thread::sleep(Duration::from_millis(200));
    };
    let (param, min) = params
        .lines()
        .filter(|l| l.starts_with("- ") && !l.contains("whole values"))
        .find_map(|l| {
            let id: u64 = l[2..].split(':').next()?.parse().ok()?;
            let range = l.rsplit_once(" (")?.1.strip_suffix(')')?;
            let min = range.split(" to ").next()?.to_string();
            Some((id, min))
        })
        .unwrap_or_else(|| panic!("no continuous parameter in {params}"));
    let r = mcp.tool(
        "change",
        json!({"commands": [{"id": "mixer.set_param",
            "params": {"slot": slot, "param": param, "value": min}}], "window": w}),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(
        r["structuredContent"]["state"], "applied_without_asking",
        "{r}"
    );
    let after = mcp.ok("plugin_params", json!({"slot": slot}));
    let line = after
        .lines()
        .find(|l| l.starts_with(&format!("- {param}: ")))
        .unwrap_or_else(|| panic!("no parameter {param} in {after}"));
    assert!(line.contains(&format!(" = {min} (")), "{line}");

    // The composer's fader: it asks.
    let fader = json!([{"id": "mixer.set_strip", "params": {"strip": a, "gain_db": "-6"}}]);
    let (message, r) = ask(
        &mut mcp,
        &mut app,
        3,
        &fader,
        json!({"action": "accept", "content": {"allow": true}}),
    );
    assert!(
        message.contains("volume ") && message.contains("\u{2192} \u{2212}6.0 dB"),
        "{message}"
    );
    assert_eq!(r["isError"], false, "{r}");

    // An instrument track: the Built-in Synth shares one strip, so the
    // track shows in the document's tracks, named with the mark.
    let tracks = |app: &mut App| -> Vec<Value> {
        app.call("state.get", json!({"target": "document", "window": w}))["lanes"]
            .as_array()
            .unwrap()
            .clone()
    };
    let before = tracks(&mut app).len();
    mcp.ok(
        "track_new_plugin_instrument",
        json!({"plugin": "built-in-synth", "window": w}),
    );
    let now = tracks(&mut app);
    assert_eq!(now.len(), before + 1, "{now:?}");
    assert!(
        now.iter().any(|t| t["co_producer"] == true
            && t["name"]
                .as_str()
                .is_some_and(|n| n.ends_with(" (Co-Producer)"))),
        "{now:?}"
    );
    mcp.ok("end_request", json!({}));

    // The mixer view.
    let listed = mcp.request("tools/list", json!({}));
    let show = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "show_mixer")
        .unwrap_or_else(|| panic!("no show_mixer in {listed}"))
        .clone();
    assert_eq!(
        show["_meta"]["ui"]["resourceUri"], "ui://vizdaw/mixer",
        "{show}"
    );
    let page = mcp.request("resources/read", json!({"uri": "ui://vizdaw/mixer"}));
    let html = page["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(html.starts_with("<!DOCTYPE html>"), "{page}");
    let shown = mcp.tool("show_mixer", json!({}));
    let rows = shown["structuredContent"]["strips"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|s| s["id"] == bus && s["co_producer"] == true),
        "{shown}"
    );

    // The gate: a raw client on the Co-Producer socket.
    let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
    let t = Duration::from_secs(30);
    for (cmd, params) in [
        (
            "command.run",
            json!({"id": "mixer.set_output", "params": {"strip": a, "bus": null}, "window": w}),
        ),
        (
            "command.run",
            json!({"id": "mixer.new_bus", "params": {"strip": a}, "window": w}),
        ),
    ] {
        match raw.call(cmd, params.clone(), t) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd} {params}: {e:?}")
            }
            other => panic!("{cmd} {params}: expected not_allowed, got {other:?}"),
        }
    }
    raw.call("co_producer.begin", json!({"name": "gate", "window": w}), t)
        .unwrap();
    match raw.call(
        "co_producer.preview",
        json!({"commands": [{"id": "mixer.add_insert",
            "params": {"strip": a, "plugin": "clap:vizdaw.e2e.none"}}], "window": w}),
        t,
    ) {
        Err(CallError::Reply(e)) => {
            assert_eq!(e.code, ErrorCode::Failed, "{e:?}");
            assert!(
                e.message
                    .contains("No installed plug-in is clap:vizdaw.e2e.none."),
                "{e:?}"
            );
        }
        other => panic!("expected failed, got {other:?}"),
    }
    drop(raw);
    drop(mcp);
    app.quit();
}

/// README steps for plan 8e through `vizdaw-mcp`: the recipes, a starter
/// project in a new window, automation on Co-Producer's own bus (no
/// question) and on the composer's strip (a decline, then a yes), notes
/// read and never written, the new audio track's own strip, a bus the
/// composer feeds asking before it is muted, and the Co-Producer
/// socket's gate.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_creative_over_mcp() {
    use core_control::ErrorCode;
    let mut app = app_with_multiq();
    let home = app.launched.home.clone();
    let sock = home.join("support").join("co-producer.sock");
    let mut mcp = McpProc::start(
        &app,
        json!({"elicitation": {},
            "extensions": {"io.modelcontextprotocol/ui": {"mimeTypes": ["text/html;profile=mcp-app"]}}}),
    );
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    wait_for_path(&sock, true);
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let demo_w = demo["window"].clone();
    let song = home.join("Song.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": song.display().to_string()}, "window": demo_w}),
    );
    let doc = |app: &mut App, w: &Value| {
        app.call("state.get", json!({"target": "document", "window": w}))
    };
    let mixer =
        |app: &mut App, w: &Value| app.call("state.get", json!({"target": "mixer", "window": w}));
    let strip_named = |m: &Value, name: &str| -> Value {
        m["strips"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap_or_else(|| panic!("no strip {name} in {m}"))
            .clone()
    };
    let demo_doc = doc(&mut app, &demo_w);
    let seq = demo_doc["selected_seq"].clone();
    // An instrument track P with notes, and its strip.
    let p_lane = demo_doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["kind"] == "instrument" && l["events"].as_u64().unwrap_or(0) > 0)
        .unwrap_or_else(|| panic!("no instrument track with notes in {demo_doc}"))
        .clone();
    let p_events = p_lane["events"].clone();
    let p_strip = {
        let m = mixer(&mut app, &demo_w);
        let strips = m["strips"].as_array().unwrap();
        strips
            .iter()
            .find(|s| s["name"] == p_lane["name"] && s["co_producer"] != true)
            .or_else(|| {
                strips
                    .iter()
                    .find(|s| s["kind_colour"] == "instrument" && s["co_producer"] != true)
            })
            .unwrap_or_else(|| panic!("no strip for the track in {m}"))["id"]
            .as_u64()
            .unwrap()
    };

    // The recipes: six, and a prompt that takes a style.
    let recipes = mcp.ok("recipes", json!({}));
    for name in [
        "starter_project",
        "mix_drums",
        "shared_reverb",
        "parallel_compression",
        "delay_into_reverb",
        "automation_moves",
    ] {
        assert!(recipes.contains(name), "{name} in {recipes}");
    }
    assert!(!recipes.contains("new_part"), "{recipes}");
    let prompts = mcp.request("prompts/list", json!({}));
    let starter_prompt = prompts["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "starter_project")
        .unwrap_or_else(|| panic!("no starter_project prompt in {prompts}"))
        .clone();
    assert_eq!(
        starter_prompt["arguments"][0]["name"], "style",
        "{starter_prompt}"
    );

    // Automation on the composer's strip, in the demo window: it asks.
    let points = "0:-144 61440:0";
    let write = |seq: &Value, target: &str, window: &Value| {
        json!({"commands": [{"id": "automation.write",
            "params": {"seq": seq, "target": target, "start": 0, "end": 61440, "points": points}}],
            "window": window})
    };
    let ask = |mcp: &mut McpProc, app: &mut App, n: i64, args: &Value, result: Value| {
        mcp.send(
            &json!({"jsonrpc": "2.0", "id": 4000 + n, "method": "tools/call",
            "params": {"name": "change", "arguments": args}}),
        );
        let e = mcp.recv();
        assert_eq!(e["method"], "elicitation/create", "{e}");
        let message = e["params"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let accepted = result["action"] == "accept";
        mcp.send(&json!({"jsonrpc": "2.0", "id": e["id"], "result": result}));
        if accepted {
            answer_sheet(app, true);
        }
        let r = mcp.recv();
        assert_eq!(r["id"], 4000 + n, "{r}");
        (message, r["result"].clone())
    };
    let p_target = format!("fader:{p_strip}");
    let on_p = write(&seq, &p_target, &demo_w);
    mcp.ok("begin_request", json!({"name": "ride the lead"}));
    let (message, r) = ask(&mut mcp, &mut app, 1, &on_p, json!({"action": "decline"}));
    assert!(
        message.contains("fader automation 0 points \u{2192} 2 points"),
        "{message}"
    );
    assert_eq!(
        mcp_text(&r),
        "The composer said no. Nothing changed.",
        "{r}"
    );
    let curve = |mcp: &mut McpProc| {
        mcp.ok(
            "preview_automation",
            json!({"seq": seq, "target": p_target, "start": 0, "end": 61440,
                "points": points, "window": demo_w}),
        )
    };
    assert!(curve(&mut mcp).contains("now 0 points"), "nothing written");
    let (_, r) = ask(
        &mut mcp,
        &mut app,
        2,
        &on_p,
        json!({"action": "accept", "content": {"allow": true}}),
    );
    assert_eq!(r["isError"], false, "{r}");
    mcp.ok("end_request", json!({}));
    wait_for_undo(&mut app, &demo_w, "Co-Producer: Ride the Lead");
    assert!(curve(&mut mcp).contains("now 2 points"), "written");

    // Notes are read, never written.
    let read = mcp.ok(
        "read_midi_notes",
        json!({"seq": seq, "lane": p_lane["id"], "window": demo_w}),
    );
    assert!(read.contains(':'), "{read}");
    assert_eq!(
        lane_named(&mut app, &demo_w, p_lane["name"].as_str().unwrap())["events"],
        p_events
    );
    let listed = mcp.request("tools/list", json!({}));
    for t in listed["result"]["tools"].as_array().unwrap() {
        let name = t["name"].as_str().unwrap_or_default();
        assert!(
            name != "midi_new_part" && name != "midi_add_notes",
            "{name}"
        );
    }

    // One undo takes back the request that held the accepted write.
    app.call("command.run", json!({"id": "edit.undo", "window": demo_w}));
    mcp.ok("begin_request", json!({"name": "look"}));
    assert!(curve(&mut mcp).contains("now 0 points"), "undone");
    mcp.ok("end_request", json!({}));

    // Each new audio track gets its own strip, and deleting it takes the
    // strip in the same undo step (plan 8e Task 6c).
    let strips = |app: &mut App| mixer(app, &demo_w)["strips"].as_array().unwrap().len();
    let lane_ids = |app: &mut App| -> Vec<u64> {
        doc(app, &demo_w)["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|l| l["id"].as_u64())
            .collect()
    };
    let (strips0, lanes0) = (strips(&mut app), lane_ids(&mut app));
    app.call(
        "command.run",
        json!({"id": "track.new_audio", "window": demo_w}),
    );
    assert_eq!(strips(&mut app), strips0 + 1, "its own strip");
    let new_lane = lane_ids(&mut app)
        .into_iter()
        .find(|id| !lanes0.contains(id))
        .expect("the new track");
    app.call(
        "command.run",
        json!({"id": "track.delete", "params": {"seq": seq, "lane": new_lane}, "window": demo_w}),
    );
    assert_eq!(strips(&mut app), strips0, "its strip went with it");
    app.call("command.run", json!({"id": "edit.undo", "window": demo_w}));
    assert_eq!(strips(&mut app), strips0 + 1, "one undo brings both back");

    // A bus the composer's strip feeds asks before it is muted.
    mcp.ok("begin_request", json!({"name": "echo bus"}));
    mcp.ok("mixer_new_bus", json!({"name": "Echo", "window": demo_w}));
    let echo = strip_named(&mixer(&mut app, &demo_w), "Echo (Co-Producer)")["id"]
        .as_u64()
        .unwrap();
    let route = json!({"commands": [{"id": "mixer.set_output",
        "params": {"strip": p_strip, "bus": echo}}], "window": demo_w});
    let (_, r) = ask(
        &mut mcp,
        &mut app,
        3,
        &route,
        json!({"action": "accept", "content": {"allow": true}}),
    );
    assert_eq!(r["isError"], false, "{r}");
    let mute = json!({"commands": [{"id": "mixer.set_strip",
        "params": {"strip": echo, "mute": true}}], "window": demo_w});
    let (message, r) = ask(&mut mcp, &mut app, 4, &mute, json!({"action": "decline"}));
    assert!(message.contains("silent through it"), "{message}");
    assert_eq!(
        mcp_text(&r),
        "The composer said no. Nothing changed.",
        "{r}"
    );
    mcp.ok("end_request", json!({}));

    // A starter project, in a new window.
    let spec = json!({
        "title": "Synthwave",
        "tempo": "118",
        "meter": "4/4",
        "markers": [
            {"bar": 1, "name": "Intro"}, {"bar": 9, "name": "Verse"},
            {"bar": 17, "name": "Chorus"}, {"bar": 33, "name": "Bridge"},
            {"bar": 41, "name": "Outro"}
        ],
        "buses": [
            {"name": "Reverb", "effects": ["clap:vizdaw.e2e.none"]},
            {"name": "Delay", "output": "Reverb"}
        ],
        "tracks": [
            {"name": "Drums", "kind": "instrument"},
            {"name": "Bass", "kind": "instrument", "plugin": "clap:vizdaw.e2e.none"},
            {"name": "Pad", "kind": "instrument",
             "sends": [{"bus": "Reverb", "level_db": "-12"}]},
            {"name": "Lead", "kind": "instrument"},
            {"name": "Arp", "kind": "instrument"}
        ]
    });
    let demo_undo = doc(&mut app, &demo_w)["undo"].clone();
    let windows_before = window_numbers(&mut app).len();
    mcp.ok("begin_request", json!({"name": "start synthwave"}));
    // The starter ends its own request: the new song holds its undo step.
    let made = mcp.ok("starter_project", json!({"spec": spec}));
    let w = {
        let n: u64 = made
            .split("in window ")
            .nth(1)
            .unwrap_or_else(|| panic!("no window in {made}"))
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap();
        json!(n)
    };
    assert!(
        made.contains("Bass: ") && made.contains("Built-in Synth"),
        "{made}"
    );
    assert!(
        made.contains("Reverb: ") && made.contains("Multi-Q"),
        "{made}"
    );
    assert_eq!(window_numbers(&mut app).len(), windows_before + 1);
    wait_for_undo(&mut app, &w, "Co-Producer: Start Synthwave");
    let wd = doc(&mut app, &w);
    assert_eq!(wd["markers"].as_array().unwrap().len(), 5, "{wd}");
    let tracks: Vec<&Value> = wd["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| {
            l["name"]
                .as_str()
                .is_some_and(|n| n.ends_with(" (Co-Producer)"))
        })
        .collect();
    assert_eq!(tracks.len(), 5, "{wd}");
    for t in &tracks {
        assert_eq!(t["events"], 0, "{t}");
    }
    assert_eq!(
        doc(&mut app, &demo_w)["undo"],
        demo_undo,
        "the demo's history"
    );
    // Tempo and meter, from the song as saved.
    let w_path = home.join("Synthwave.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": w_path.display().to_string()}, "window": w}),
    );
    {
        use core_model::meter::Meter;
        use core_model::tempo::Bpm;
        let saved = core_model::file::load(&w_path).expect("the saved starter");
        let q = saved.sequences().values().next().expect("a sequence");
        let tempo: Vec<Bpm> = q.tempo.points().iter().map(|p| p.bpm).collect();
        assert_eq!(tempo, vec![Bpm(118_000)]);
        let meter: Vec<Meter> = q.meter.entries().iter().map(|e| e.meter).collect();
        assert_eq!(meter, vec![Meter { num: 4, den: 4 }]);
    }
    let m = mixer(&mut app, &w);
    let reverb = strip_named(&m, "Reverb (Co-Producer)");
    let delay = strip_named(&m, "Delay (Co-Producer)");
    assert_eq!(delay["output"], reverb["id"], "{m}");
    // The fallback is the bus's own Multi-Q EQ, not a second insert.
    assert_eq!(reverb["eq"]["plugin"], "multi-q", "{m}");
    assert_eq!(reverb["inserts"], json!([]), "{m}");
    let reverb_id = reverb["id"].as_u64().unwrap();
    let w_seq = wd["selected_seq"].clone();

    // Automation on Co-Producer's own bus, in W: no question. The request
    // names W's window, as the starter's answer says (plan 8e final
    // review Minor 6), so the composer's focus does not matter.
    assert!(
        made.contains(&format!("pass window {w} to begin_request")),
        "{made}"
    );
    let target = format!("fader:{reverb_id}");
    mcp.ok(
        "begin_request",
        json!({"name": "fade the reverb in", "window": w}),
    );
    let r = mcp.tool(
        "preview_automation",
        json!({"seq": w_seq, "target": target, "start": 0, "end": 61440,
            "points": points, "window": w}),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert!(mcp_text(&r).contains("planned 2 points"), "{r}");
    assert_eq!(r["structuredContent"]["view"], "automation", "{r}");
    let r = mcp.tool("change", write(&w_seq, &target, &w));
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(
        r["structuredContent"]["state"], "applied_without_asking",
        "{r}"
    );
    mcp.ok("end_request", json!({}));

    // The gate: a raw client on the Co-Producer socket.
    let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
    let t = Duration::from_secs(30);
    let params = json!({"id": "automation.write", "window": demo_w,
        "params": {"seq": seq, "target": p_target, "start": 0, "end": 61440, "points": points}});
    match raw.call("command.run", params, t) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::NotAllowed, "{e:?}"),
        other => panic!("expected not_allowed, got {other:?}"),
    }
    for id in ["midi.new_part", "midi.add_notes"] {
        match raw.call(
            "command.run",
            json!({"id": id, "params": {}, "window": demo_w}),
            t,
        ) {
            Err(CallError::Reply(e)) => assert!(
                matches!(e.code, ErrorCode::UnknownCommand | ErrorCode::UnknownId),
                "{id}: {e:?}"
            ),
            other => panic!("{id}: expected a refusal, got {other:?}"),
        }
    }
    let windows = window_numbers(&mut app).len();
    let cycle = json!({"spec": {
        "title": "Loop",
        "buses": [{"name": "A", "output": "B"}, {"name": "B", "output": "A"}],
        "tracks": [{"name": "Keys", "kind": "instrument"}]
    }});
    match raw.call("co_producer.starter", cycle, t) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}"),
        other => panic!("expected invalid_params, got {other:?}"),
    }
    assert_eq!(window_numbers(&mut app).len(), windows, "no new window");
    drop(raw);
    drop(mcp);
    app.quit();
}

/// README steps 368 to 382 through the channel (plan release-fixes): a
/// region's fade shape, a track's output, a Built-in Synth's sound, a
/// held channel touch released, and the Mix view refusing nudges.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn release_fixes_over_the_channel() {
    use core_control::ErrorCode;
    use core_engine::fixtures::Builder;
    use core_model::command::{Command, LaneCmd, RoutingCmd};
    use core_model::event::{EventBody, Fade, FadeShape};
    use core_model::ids::AnyId;
    let mut app = App::start();
    // One audio track "Tone" with a region (10 ms = 480 frames fade-out)
    // on output "Audio 1", a second output "Audio 2", a MIDI track on an
    // external destination, and an instrument track on the Built-in Synth.
    let mut b = Builder::new();
    let synth = b.dest("Built-in Synth", "builtin:synth", 0).unwrap();
    let ext = b.dest("Hardware", "hw:synth", 0).unwrap();
    let (seq, _) = b.sequence("Song", "Piano", Some(synth)).unwrap();
    let midi = b.lane(seq, "Pads", Some(ext)).unwrap();
    let mut outputs = Vec::new();
    for (name, reference) in [("Audio 1", "audio:1-2"), ("Audio 2", "audio:3-4")] {
        let r = b
            .run(Command::Routing(RoutingCmd::AddOutput {
                name: name.into(),
                reference: reference.into(),
            }))
            .unwrap();
        outputs.push(
            r.created
                .iter()
                .find_map(|id| match id {
                    AnyId::Output(o) => Some(o.0),
                    _ => None,
                })
                .unwrap(),
        );
    }
    let media = b.media("Media/tone.wav", 96_000, 1).unwrap();
    let tone = b.lane(seq, "Tone", None).unwrap();
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane: tone,
        output: b.doc().outputs().keys().next().copied(),
    }))
    .unwrap();
    let region = b
        .event(
            seq,
            tone,
            0,
            EventBody::AudioRegion {
                media,
                source_offset: 0,
                length: 96_000,
                gain_db: 0.0,
                fade_in: Fade::default(),
                fade_out: Fade {
                    length_frames: 480,
                    shape: FadeShape::Linear,
                },
                name: Some("Tone".into()),
                crossfade: false,
            },
        )
        .unwrap();
    let path = app.launched.home.join("ReleaseFixes.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        )
    };
    let fail = |app: &mut App, id: &str, params: Value| {
        app.call_err(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        )
    };
    let (seq, tone, midi, region) = (seq.0, tone.0, midi.0, region.0);

    // 1. The fade-out's shape: "Fade Out: S-Curve", in the editor's
    // state, and Undo brings the old shape back.
    let (el, label) = wait_for_label_like(&mut app, &w, "Region Tone on ", "");
    app.call("ui.click", json!({"window": w, "element": el}));
    run(&mut app, "editors.audio_editor", json!({}));
    let fade_out = |app: &mut App| {
        wait_for_state(app, &w, "audio_editor", |s| s["region"]["event"] == region)["region"]
            ["fade_out"]
            .clone()
    };
    assert_eq!(
        fade_out(&mut app),
        json!({"frames": 480, "shape": "linear"}),
        "{label}"
    );
    let r = run(
        &mut app,
        "region.set_fade_shape",
        json!({"seq": seq, "lane": tone, "event": region, "fade": "out", "shape": "s_curve"}),
    );
    assert_eq!(r["status"], "Fade Out: S-Curve", "{r}");
    assert_eq!(
        fade_out(&mut app),
        json!({"frames": 480, "shape": "s_curve"})
    );
    run(&mut app, "edit.undo", json!({}));
    wait_for_state(&mut app, &w, "audio_editor", |s| {
        s["region"]["fade_out"]["shape"] == "linear"
    });

    // 2. A track's output: the second path, then none; a MIDI track fails.
    let audio_out = |app: &mut App, name: &str| lane_named(app, &w, name)["audio_out"].clone();
    assert_eq!(audio_out(&mut app, "Tone"), json!(outputs[0]));
    run(
        &mut app,
        "lane.set_output",
        json!({"seq": seq, "lane": tone, "output": outputs[1]}),
    );
    assert_eq!(audio_out(&mut app, "Tone"), json!(outputs[1]));
    let r = run(
        &mut app,
        "lane.set_output",
        json!({"seq": seq, "lane": tone, "output": null}),
    );
    assert_eq!(r["status"], "No Output", "{r}");
    assert_eq!(audio_out(&mut app, "Tone"), Value::Null);
    let e = fail(
        &mut app,
        "lane.set_output",
        json!({"seq": seq, "lane": midi, "output": outputs[0]}),
    );
    assert_eq!(e.code, ErrorCode::Failed, "{e:?}");

    // 3. The Built-in Synth's sound: Drums, then Undo.
    let sound = |app: &mut App| {
        let doc = app.call("state.get", json!({"target": "document", "window": w}));
        doc["destinations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["name"] == "Built-in Synth")
            .unwrap_or_else(|| panic!("no Built-in Synth in {doc}"))["sound"]
            .clone()
    };
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let dest = doc["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "Built-in Synth")
        .unwrap()["id"]
        .clone();
    let before = sound(&mut app);
    let r = run(
        &mut app,
        "destination.set_sound",
        json!({"dest": dest, "sound": "drums"}),
    );
    assert_eq!(r["status"], "Drums", "{r}");
    assert_eq!(sound(&mut app), json!("drums"));
    run(&mut app, "edit.undo", json!({}));
    assert_eq!(sound(&mut app), before);

    // 4. A touch with no release, on a stopped transport, then released.
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    let strip = state["modes"][0]["strip"].as_u64().expect("a strip");
    run(
        &mut app,
        "automation.gesture",
        json!({"target": format!("fader:{strip}"), "phase": "touch", "value": "0.5"}),
    );
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["touched"].as_array().unwrap().len(), 1, "{state}");
    let r = run(&mut app, "automation.release_all", json!({}));
    assert_eq!(r["status"], "Released 1 touch.", "{r}");
    let state = app.call("state.get", json!({"target": "automation", "window": w}));
    assert_eq!(state["touched"], json!([]), "{state}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let step = doc["undo"].clone();
    assert!(step.is_string(), "the touch is one undo step: {doc}");
    run(&mut app, "edit.undo", json!({}));
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(doc["redo"], step, "{doc}");

    // 5. The Mix view with no editor refuses a nudge; Arrange restores it.
    run(&mut app, "ui.view_mix", json!({}));
    let e = fail(&mut app, "editors.nudge_later", json!({}));
    assert_eq!(e.code, ErrorCode::Unavailable, "{e:?}");
    assert!(e.message.contains("The Mix view shows no editor"), "{e:?}");
    run(&mut app, "ui.view_arrange", json!({}));
    let r = run(&mut app, "editors.nudge_later", json!({}));
    assert_eq!(r["undo"], "Nudge Events", "Arrange restores the nudge: {r}");
    app.quit();
}

/// README steps for plan 8f through `vizdaw-mcp`: time counting is off
/// until the composer turns it on, counts transport time and friction with
/// Co-Producer off from reading it, keeps a private file with nothing of
/// the song's inside, stops sampling when the switch goes off, is read only
/// on the Co-Producer socket, and Delete All removes it.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_time_over_mcp() {
    use core_control::ErrorCode;
    use std::os::unix::fs::PermissionsExt;
    let mut app = App::start_with(&[("VIZDAW_METRICS_FAST", "1")]);
    let home = app.launched.home.clone();
    let support = home.join("support");
    let metrics = support.join("Metrics");
    let time_json = metrics.join("time.json");
    let sock = support.join("co-producer.sock");
    let mut mcp = McpProc::start(
        &app,
        json!({"elicitation": {},
            "extensions": {"io.modelcontextprotocol/ui": {"mimeTypes": ["text/html;profile=mcp-app"]}}}),
    );
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    wait_for_path(&sock, true);
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let song = home.join("Song.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save", "params": {"path": song.display().to_string()}, "window": w}),
    );
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        )
    };
    let play_for = |app: &mut App, secs: u64| {
        run(app, "transport.play", json!({}));
        std::thread::sleep(Duration::from_secs(secs));
        run(app, "transport.stop", json!({}));
    };
    let counting = |app: &mut App, on: bool| {
        app.call(
            "command.run",
            json!({"id": "settings.co_producer_metrics", "params": {"on": on}}),
        )
    };
    let t = Duration::from_secs(30);

    // Off by default: nothing is written and nothing is counted.
    play_for(&mut app, 3);
    assert!(!metrics.exists(), "no Metrics folder while counting is off");
    let off_text = mcp.ok("time_report", json!({}));
    assert!(off_text.starts_with("Time counting is off."), "{off_text}");

    // Counting: three fast blocks of play, then six of one task and undos.
    counting(&mut app, true);
    play_for(&mut app, 6);
    for _ in 0..6 {
        run(&mut app, "track.new_audio", json!({}));
    }
    for _ in 0..5 {
        run(&mut app, "edit.undo", json!({}));
    }
    // A host that asks for MCP Apps gets the text and the data of the
    // "time" view although no ui:// resource exists for it.
    let shown = mcp.tool("time_report", json!({}));
    assert_eq!(shown["isError"], false, "{shown}");
    assert_eq!(shown["structuredContent"]["view"], "time", "{shown}");
    assert!(shown["_meta"]["ui"].is_null(), "{shown}");
    let text = mcp_text(&shown);
    assert!(text.contains("- Song: "), "{text}");
    let bursts = text
        .split_once("undo burst")
        .and_then(|(head, _)| head.trim_end().rsplit(' ').next())
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("an undo burst count in {text}"));
    assert!(bursts >= 1, "{text}");
    assert!(
        text.contains("repeated tasks: ") && !text.contains("repeated tasks: none"),
        "{text}"
    );
    let ui = mcp.request("resources/read", json!({"uri": "ui://vizdaw/time"}));
    assert!(ui.get("error").is_some(), "no time view resource: {ui}");
    // The developer channel cannot read the records: that is Co-Producer's.
    let e = app.call_err("co_producer.metrics", json!({}));
    assert_eq!(e.code, ErrorCode::NotAllowed, "{e:?}");

    // Private: modes, and nothing of the song's in the file.
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&support), 0o700);
    assert_eq!(mode(&metrics), 0o700);
    assert_eq!(mode(&time_json), 0o600);
    let bytes = std::fs::read(&time_json).unwrap();
    let body = String::from_utf8(bytes.clone()).expect("UTF-8");
    assert!(!body.contains('/'), "no path in {body}");
    assert!(!body.contains(".vizdaw"), "no file name in {body}");
    let demo_doc = app.call("state.get", json!({"target": "document", "window": w}));
    for lane in demo_doc["lanes"].as_array().unwrap() {
        let name = lane["name"].as_str().unwrap_or_default();
        if name.len() > 2 {
            assert!(!body.contains(name), "track name {name} in {body}");
        }
    }
    let store: Value = serde_json::from_str(&body).unwrap();
    let mut keys: Vec<&str> = store
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["since", "songs", "untitled", "version"], "{body}");
    assert_eq!(store["version"], 1, "{body}");
    assert_eq!(store["songs"].as_object().unwrap().len(), 1, "{body}");

    // Only the composer: the Co-Producer socket cannot change the settings.
    let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
    for (id, params) in [
        ("settings.delete_time_records", json!({})),
        ("settings.co_producer_metrics", json!({"on": false})),
    ] {
        match raw.call("command.run", json!({"id": id, "params": params}), t) {
            Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::NotAllowed, "{id}: {e:?}"),
            other => panic!("{id}: expected not_allowed, got {other:?}"),
        }
    }
    assert!(time_json.exists(), "the refusals deleted nothing");
    drop(raw);

    // Switching off stops the sampling: the file does not move while play
    // goes on, and the earlier records still read.
    counting(&mut app, false);
    let frozen = std::fs::read(&time_json).unwrap();
    let frozen_at = std::fs::metadata(&time_json).unwrap().modified().unwrap();
    play_for(&mut app, 4);
    assert_eq!(
        std::fs::read(&time_json).unwrap(),
        frozen,
        "sampling stopped"
    );
    assert_eq!(
        std::fs::metadata(&time_json).unwrap().modified().unwrap(),
        frozen_at
    );
    let earlier = mcp.ok("time_report", json!({}));
    assert!(earlier.contains("Counting is off now"), "{earlier}");

    // Delete all, over the developer channel.
    counting(&mut app, true);
    app.call(
        "command.run",
        json!({"id": "settings.delete_time_records", "params": {}}),
    );
    wait_for_path(&metrics, false);
    assert_eq!(
        mcp.ok("time_report", json!({})),
        "No time has been counted yet."
    );

    // Off again: nothing is written.
    counting(&mut app, false);
    play_for(&mut app, 3);
    assert!(!metrics.exists(), "no Metrics folder after switching off");
    drop(mcp);
    app.quit();
}

/// Release gate B2 (Task 2): in 26.0.0 Co-Producer writes no MIDI notes,
/// so `file.import_midi` is refused in every form. Through `vizdaw-mcp` it
/// is no tool and not on the command list, and the Co-Producer socket
/// itself refuses it. No track, note or window appears.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn co_producer_import_midi_is_refused() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let home = app.launched.home.clone();
    let sock = home.join("support/co-producer.sock");
    app.call(
        "command.run",
        json!({"id": "settings.co_producer", "params": {"on": true}}),
    );
    wait_for_path(&sock, true);

    // A real MIDI file, written by the developer channel from the demo,
    // so a refusal is the gate's and not the file's.
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let smf = home.join("demo.mid");
    app.call(
        "command.run",
        json!({"id": "file.export_midi", "params": {"path": smf}, "window": w}),
    );
    assert_eq!(&std::fs::read(&smf).unwrap()[..4], b"MThd");
    let doc = |app: &mut App| app.call("state.get", json!({"target": "document", "window": w}));
    let before = doc(&mut app);
    let tracks = |d: &Value| d["lanes"].as_array().unwrap().len();
    let events = |d: &Value| -> u64 {
        d["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["events"].as_u64().unwrap_or(0))
            .sum()
    };
    let windows = window_numbers(&mut app);
    let path = smf.display().to_string();
    let forms = [
        json!({"path": path}),
        json!({"path": path, "tick": 0}),
        json!({"path": path, "new_project": true}),
        json!({"path": path, "new_project": false}),
    ];

    // Through vizdaw-mcp: not a tool, not on the list, not a change.
    let mut mcp = McpProc::start(&app, json!({}));
    let listed = mcp.request("tools/list", json!({}));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed}"))
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(!names.contains(&"file_import_midi"), "{names:?}");
    mcp.ok("begin_request", json!({"name": "import a MIDI file"}));
    for params in &forms {
        let r = mcp.request(
            "tools/call",
            json!({"name": "file_import_midi", "arguments": params}),
        );
        assert!(r.get("error").is_some(), "no such tool: {r}");
        let r = mcp.tool(
            "change",
            json!({"commands": [{"id": "file.import_midi", "params": params}], "window": w}),
        );
        assert_eq!(r["isError"], true, "{r}");
        assert!(
            mcp_text(&r).contains("Co-Producer has no command file.import_midi"),
            "{r}"
        );
    }
    mcp.ok("end_request", json!({}));
    drop(mcp);

    // The Co-Producer socket's own gate, past vizdaw-mcp.
    let mut raw = Client::connect(&sock).expect("the Co-Producer socket");
    for params in &forms {
        for (cmd, q) in [
            (
                "command.run",
                json!({"id": "file.import_midi", "params": params, "window": w}),
            ),
            (
                "co_producer.preview",
                json!({"commands": [{"id": "file.import_midi", "params": params}], "window": w}),
            ),
        ] {
            match raw.call(cmd, q.clone(), Duration::from_secs(30)) {
                Err(CallError::Reply(e)) => {
                    if cmd == "command.run" {
                        assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd} {q}: {e:?}");
                    }
                    assert!(
                        e.message.contains("cannot run file.import_midi"),
                        "{cmd} {q}: {e:?}"
                    );
                }
                other => panic!("{cmd} {q}: expected a refusal, got {other:?}"),
            }
        }
    }
    drop(raw);
    assert_no_sheet(&mut app);

    // Nothing was written: the same tracks and notes, and no new window.
    let after = doc(&mut app);
    assert_eq!(tracks(&after), tracks(&before), "{after}");
    assert_eq!(events(&after), events(&before), "{after}");
    assert_eq!(after["undo"], before["undo"], "no undo step");
    assert_eq!(window_numbers(&mut app), windows, "no new window");
    app.quit();
}

/// Release gate Task 3: the save prompts are the user's only. `file.close`
/// over the control channel closes a song with unsaved changes, untitled
/// or saved, at once: no prompt holds the call, and the window goes.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn control_close_of_a_dirty_song_does_not_prompt() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let home = app.launched.home.clone();
    let saved = home.join("Dirty.vizdaw");
    for save in [false, true] {
        let w = app.call("command.run", json!({"id": "file.new"}))["window"].clone();
        app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
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
            "file.close (saved: {save}): {r:?}\nVizDAW log:\n{}",
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

/// README 439–456 through the channel (plan groups-vcas-transport Task 13):
/// a group moves its members by one dB and undoes as one step, a VCA's
/// offset leaves the faders alone, the transport bar takes a Command
/// button that opens a mixer window, resets, and the click settings show
/// in the transport state (a level above 0 dB is refused).
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn groups_vcas_and_transport_bar_over_the_channel() {
    use core_control::ErrorCode;
    let mut app = App::start();
    let w = app.call("app.new_demo", json!({}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));
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
    let ids = format!("{},{}", two[0].0, two[1].0);
    let run = |app: &mut App, id: &str, params: Value| {
        app.call(
            "command.run",
            json!({"id": id, "params": params, "window": w}),
        )
    };
    let gain = |m: &Value, id: u64| -> f64 {
        m["strips"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"].as_u64() == Some(id))
            .unwrap_or_else(|| panic!("no strip {id} in {m}"))["gain_db"]
            .as_f64()
            .unwrap()
    };

    // A group of both strips; one move moves both by the same dB.
    run(&mut app, "mixer.new_group", json!({"strips": ids}));
    run(
        &mut app,
        "mixer.set_strip",
        json!({"strip": two[0].0, "gain_db": "-3"}),
    );
    let m = app.call("state.get", json!({"target": "mixer", "window": w}));
    let d0 = gain(&m, two[0].0) - two[0].1;
    let d1 = gain(&m, two[1].0) - two[1].1;
    assert!((gain(&m, two[0].0) + 3.0).abs() < 1e-3, "{m}");
    assert!((d0 - d1).abs() < 1e-3, "both moved by {d0} / {d1}: {m}");
    assert_eq!(m["groups"].as_array().unwrap().len(), 1, "{m}");
    wait_for_undo(&mut app, &w, "Move Group Group 1");

    // A VCA: the offset shows, the faders stay.
    run(&mut app, "mixer.new_vca", json!({"strips": ids}));
    run(
        &mut app,
        "mixer.set_vca",
        json!({"vca": 1, "gain_db": "-6"}),
    );
    let after = app.call("state.get", json!({"target": "mixer", "window": w}));
    for (id, _) in &two {
        let s = after["strips"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"].as_u64() == Some(*id))
            .unwrap();
        assert!(
            (s["vca_offset_db"].as_f64().unwrap() + 6.0).abs() < 1e-3,
            "{s}"
        );
        assert!((gain(&after, *id) - gain(&m, *id)).abs() < 1e-6, "{after}");
    }

    // The bar: the cluster and one Command button for Mixer; a click on
    // the button opens a mixer window; Reset puts the default back.
    let mixers = |app: &mut App| {
        window_numbers(app)
            .iter()
            .filter(|(_, k)| k == "mixer")
            .count()
    };
    let before = mixers(&mut app);
    app.call(
        "command.run",
        json!({"id": "ui.set_transport_bar",
            "params": {"items": "cluster,command:window.mixer"}, "window": w}),
    );
    let button = wait_for_label(&mut app, &w, "Mixer");
    app.call("ui.click", json!({"window": w, "element": button}));
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    while mixers(&mut app) <= before {
        assert!(Instant::now() < deadline, "no mixer window opened");
        std::thread::sleep(Duration::from_millis(200));
    }
    app.call(
        "command.run",
        json!({"id": "ui.reset_transport_bar", "window": w}),
    );

    // The click: its settings show in the transport state.
    run(&mut app, "transport.click", json!({"on": true}));
    run(
        &mut app,
        "transport.set_click_level",
        json!({"level_db": "-12"}),
    );
    run(
        &mut app,
        "transport.set_click_mode",
        json!({"mode": "record_only"}),
    );
    let t = app.call("state.get", json!({"target": "transport", "window": w}));
    assert_eq!(t["click"]["on"], true, "{t}");
    assert!(
        (t["click"]["level_db"].as_f64().unwrap() + 12.0).abs() < 1e-3,
        "{t}"
    );
    assert_eq!(t["click"]["mode"], "record_only", "{t}");
    let e = app.call_err(
        "command.run",
        json!({"id": "transport.set_click_level", "params": {"level_db": "6"}, "window": w}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
    app.quit();
}
