//! Command validation, the registry and wait conditions (spec §5.5, §6).

use core_control::ErrorCode;
use core_control::params::WindowRef;
use core_control::protocol::{RawRequest, RequestId};
use core_control::registry::{self, ArgValue, COMMANDS};
use core_control::request::{
    Button, COMMAND_NAMES, Command, CrashAnswer, Pointer, StateTarget, validate,
};
use core_control::wait::{self, DocFacts, Facts, WaitCondition};
use serde_json::{Value, json};

fn req(cmd: &str, params: Value) -> RawRequest {
    RawRequest {
        id: RequestId::Num(1),
        cmd: cmd.into(),
        params: params.as_object().cloned().unwrap_or_default(),
    }
}

fn ok(cmd: &str, params: Value) -> Command {
    validate(&req(cmd, params)).unwrap()
}

fn err(cmd: &str, params: Value) -> (ErrorCode, String) {
    let e = validate(&req(cmd, params)).unwrap_err();
    (e.code, e.message)
}

#[test]
fn every_command_validates_its_minimal_form() {
    let tmp = std::env::temp_dir();
    let shot = tmp.join("x.png");
    let minimal = [
        ("commands.list", json!({})),
        ("command.run", json!({"id": "transport.play_stop"})),
        ("state.get", json!({"target": "windows"})),
        ("ui.tree", json!({})),
        ("ui.click", json!({"point": {"x": 1, "y": 2}})),
        (
            "ui.drag",
            json!({"from": {"x": 1, "y": 2}, "to": "0.1:00000000"}),
        ),
        ("ui.key", json!({"key": "space"})),
        (
            "ui.screenshot",
            json!({"path": shot.to_str().unwrap(), "live": false}),
        ),
        ("app.new_demo", json!({})),
        ("app.open", json!({"path": "/tmp/a.vizdaw"})),
        ("app.quit", json!({})),
        ("wait", json!({"condition": "playing"})),
        ("ping", json!({})),
    ];
    assert_eq!(minimal.len(), COMMAND_NAMES.len());
    for (cmd, params) in minimal {
        assert!(
            validate(&req(cmd, params.clone())).is_ok(),
            "{cmd} {params}"
        );
    }
}

#[test]
fn unknown_commands_and_parameters_are_refused() {
    assert_eq!(err("state.set", json!({})).0, ErrorCode::UnknownCommand);
    assert_eq!(
        err("app.quit", json!({"now": true})).0,
        ErrorCode::InvalidParams
    );
    let (code, msg) = err("state.get", json!({"target": "mixers"}));
    assert_eq!(code, ErrorCode::InvalidParams);
    assert!(msg.starts_with("state.get: "), "{msg}");
}

#[test]
fn state_and_tree_read_their_window() {
    assert_eq!(
        ok("state.get", json!({"target": "transport", "window": 42})),
        Command::StateGet {
            target: StateTarget::Transport,
            window: Some(WindowRef::Number(42))
        }
    );
    assert_eq!(
        ok("ui.tree", json!({"window": "key", "max_depth": 3})),
        Command::UiTree {
            window: Some(WindowRef::Key),
            max_depth: 3
        }
    );
    assert_eq!(
        err("state.get", json!({"target": "transport", "window": 0})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err(
            "state.get",
            json!({"target": "transport", "window": "front"})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.tree", json!({"max_depth": 0})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn clicks_take_an_element_or_a_point() {
    match ok(
        "ui.click",
        json!({"element": "0.2:0000abcd", "button": "right", "count": 2, "modifiers": ["option"]}),
    ) {
        Command::UiClick {
            at: Pointer::Element(e),
            button,
            count,
            modifiers,
            ..
        } => {
            assert_eq!(e.path, vec![0, 2]);
            assert_eq!((button, count, modifiers.option), (Button::Right, 2, true));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(err("ui.click", json!({})).0, ErrorCode::InvalidParams);
    assert_eq!(
        err(
            "ui.click",
            json!({"element": "0:00000000", "point": {"x": 0, "y": 0}})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.click", json!({"point": {"x": 1e9, "y": 0}})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.click", json!({"point": {"x": 1, "y": 2, "z": 3}})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.click", json!({"point": {"x": 1, "y": 2}, "count": 4})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.drag", json!({"from": "nope", "to": {"x": 0, "y": 0}})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn keys_take_a_key_or_text() {
    match ok(
        "ui.key",
        json!({"key": "z", "modifiers": ["command", "shift"]}),
    ) {
        Command::UiKey { strokes, .. } => {
            assert_eq!(strokes.len(), 1);
            assert!(strokes[0].modifiers.command && strokes[0].modifiers.shift);
        }
        other => panic!("{other:?}"),
    }
    match ok("ui.key", json!({"text": "Kit"})) {
        Command::UiKey { strokes, .. } => assert_eq!(strokes.len(), 3),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err("ui.key", json!({"text": "a", "modifiers": ["command"]})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.key", json!({"key": "a", "text": "b"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(err("ui.key", json!({})).0, ErrorCode::InvalidParams);
}

#[test]
fn paths_are_absolute_and_screenshots_need_an_existing_folder() {
    assert_eq!(
        err("app.open", json!({"path": "rel.vizdaw"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("app.open", json!({"path": "/tmp/a.txt"})).0,
        ErrorCode::InvalidParams
    );
    let (code, _) = err("app.open", json!({}));
    assert_eq!(code, ErrorCode::DialogRequired, "no path: the Open dialog");
    match ok(
        "app.open",
        json!({"path": "/tmp/a.vizdaw", "plugins": "off", "restore_autosave": false}),
    ) {
        Command::AppOpen { answers, .. } => {
            assert_eq!(answers.plugins, Some(CrashAnswer::Off));
            assert_eq!(answers.restore_autosave, Some(false));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err("ui.screenshot", json!({"path": "/no/such/folder/x.png"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.screenshot", json!({"scale": 9})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn screenshot_live_is_a_bool() {
    let live = |params: Value| match ok("ui.screenshot", params) {
        Command::UiScreenshot { live, .. } => live,
        other => panic!("{other:?}"),
    };
    assert!(live(json!({"live": true})));
    assert!(!live(json!({"live": false})));
    assert!(!live(json!({})));
    assert_eq!(
        err("ui.screenshot", json!({"live": "yes"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("ui.screenshot", json!({"live": 1})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn waits_take_their_own_parameters_and_a_bounded_timeout() {
    assert_eq!(
        ok(
            "wait",
            json!({"condition": "tick_at_least", "tick": 15360, "timeout_ms": 60000})
        ),
        Command::Wait {
            condition: WaitCondition::TickAtLeast(15360),
            window: None,
            timeout_ms: 60_000
        }
    );
    match ok("wait", json!({"condition": "stopped"})) {
        Command::Wait { timeout_ms, .. } => assert_eq!(timeout_ms, wait::DEFAULT_TIMEOUT_MS),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err("wait", json!({"condition": "playing", "tick": 3})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("wait", json!({"condition": "tick_at_least"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err(
            "wait",
            json!({"condition": "playing", "timeout_ms": 600_001})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("wait", json!({"condition": "detail_kind", "kind": "mixer"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("wait", json!({"condition": "window_titled", "title": ""})).0,
        ErrorCode::InvalidParams
    );
}

#[test]
fn the_drum_menu_commands_are_registered() {
    let drum: Vec<(&str, &str, bool)> = COMMANDS
        .iter()
        .filter(|c| c.menu == "Drum")
        .map(|c| (c.id, c.title, !c.params.is_empty()))
        .collect();
    assert_eq!(
        drum,
        vec![
            ("drum.add_row", "Add Drum Row", false),
            ("drum.show_velocity", "Show Velocity", true),
            ("drum.split_row", "Split Row to Own Track", false),
            ("drum.remove_row", "Remove Drum Row", false),
            ("drum.timeline", "Timeline View", true),
        ]
    );
    assert!(
        COMMANDS
            .iter()
            .filter(|c| c.menu == "Drum")
            .all(|c| c.document && c.dialogs.is_empty())
    );
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "drum.timeline", "params": {"on": true}})
        ),
        Command::CommandRun {
            id: "drum.timeline".into(),
            args: vec![("on".into(), ArgValue::Flag(true))],
            window: None,
        }
    );
    assert_eq!(
        err(
            "command.run",
            json!({"id": "drum.add_row", "params": {"note": 36}})
        )
        .0,
        ErrorCode::InvalidParams,
        "Add Drum Row takes no parameters"
    );
}

/// Plan 7d Ruling 4: rewind and fast forward on ⇧, and ⇧. with keypad
/// twins; during step entry Keypad+ stays Tie.
#[test]
fn rewind_and_fast_forward_have_their_keys() {
    use core_control::bindings::{Chord, Keymap};
    let k = Keymap::default();
    let key = |t: &str, step: bool| k.lookup(Chord::parse(t).unwrap(), step, false);
    assert_eq!(key("⇧,", false), Some("transport.rewind"));
    assert_eq!(key("Keypad-", false), Some("transport.rewind"));
    assert_eq!(key("⇧.", false), Some("transport.fast_forward"));
    assert_eq!(key("Keypad+", false), Some("transport.fast_forward"));
    assert_eq!(key("Keypad+", true), Some("step.tie"), "step entry wins");
    for (id, title) in [
        ("transport.rewind", "Rewind"),
        ("transport.fast_forward", "Fast Forward"),
    ] {
        let c = registry::find(id).unwrap();
        assert_eq!((c.title, c.menu), (title, "Transport"));
        assert!(c.menu_item && c.document && c.yields, "{id}");
    }
    let yielding: Vec<&str> = COMMANDS.iter().filter(|c| c.yields).map(|c| c.id).collect();
    assert_eq!(
        yielding,
        [
            "transport.rewind",
            "transport.fast_forward",
            "track.new_audio",
            "track.new_instrument",
            "track.new_midi"
        ]
    );
}

/// Plan 7d hostile input: rewind and fast forward take no parameters, and
/// a misspelt id is no command.
#[test]
fn rewind_and_fast_forward_take_no_parameters() {
    for id in ["transport.rewind", "transport.fast_forward"] {
        assert_eq!(
            ok("command.run", json!({ "id": id })),
            Command::CommandRun {
                id: id.into(),
                args: vec![],
                window: None,
            }
        );
        let (code, message) = err("command.run", json!({"id": id, "params": {"bars": 2}}));
        assert_eq!(code, ErrorCode::InvalidParams, "{id}");
        assert!(
            message.ends_with(": unknown parameter \"bars\" (allowed: none)"),
            "{message}"
        );
    }
    assert_eq!(
        err("command.run", json!({"id": "transport.Rewind"})).0,
        ErrorCode::UnknownId
    );
}

#[test]
fn every_transport_key_has_a_command() {
    use core_control::bindings::{Chord, Keymap};
    let k = Keymap::default();
    let key = |t: &str| k.lookup(Chord::parse(t).unwrap(), false, false);
    for (text, id) in [
        ("Space", "transport.play_stop"),
        ("S", "transport.stop"),
        ("⌥Space", "transport.play_selection"),
        ("⇧Space", "transport.play_from_selection"),
        ("Y", "transport.play_from_edit_in"),
        ("R", "transport.record"),
        ("K", "transport.scrub"),
        ("Return", "transport.return_to_start"),
        (",", "transport.previous_marker"),
        (".", "transport.next_marker"),
        ("⇧,", "transport.rewind"),
        ("⇧.", "transport.fast_forward"),
        ("Keypad-", "transport.rewind"),
        ("Keypad+", "transport.fast_forward"),
        ("L", "transport.loop"),
        ("O", "transport.overdub"),
        ("I", "transport.punch"),
        ("G", "editors.snap"),
        ("P", "transport.go_position"),
        ("E", "transport.go_edit_in"),
        ("U", "transport.go_punch"),
        ("⌥⇧↑", "transport.arm_previous"),
        ("⌥⇧↓", "transport.arm_next"),
        ("1", "locator.go_1"),
        ("0", "locator.go_0"),
        ("⌥7", "locator.set_7"),
        ("⌥⇧9", "locator.clear_9"),
    ] {
        assert_eq!(key(text), Some(id), "{text}");
    }
    let locators: Vec<&str> = COMMANDS
        .iter()
        .filter(|c| c.menu == "Locators")
        .map(|c| c.id)
        .collect();
    assert_eq!(locators.len(), 30);
    assert!(
        COMMANDS
            .iter()
            .filter(|c| ["Locators", "Record", "Key Bindings"].contains(&c.menu))
            .all(|c| !c.menu_item),
        "no menu items: keys, the Keys settings and the channel"
    );
    let keys: Vec<&str> = COMMANDS
        .iter()
        .filter(|c| c.id.starts_with("keys."))
        .map(|c| c.id)
        .collect();
    assert_eq!(keys, vec!["keys.save", "keys.load", "keys.reset"]);
    assert!(
        COMMANDS
            .iter()
            .filter(|c| c.id.starts_with("keys."))
            .all(|c| !c.document)
    );
    let e = err("command.run", json!({"id": "keys.save"}));
    assert_eq!(e.0, ErrorCode::DialogRequired);
    assert_eq!(
        err(
            "command.run",
            json!({"id": "keys.load", "params": {"path": "/tmp/k.json"}})
        )
        .0,
        ErrorCode::InvalidParams,
        "a .vizkeys file"
    );
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "transport.punch", "params": {"on": true}})
        ),
        Command::CommandRun {
            id: "transport.punch".into(),
            args: vec![("on".into(), ArgValue::Flag(true))],
            window: None,
        }
    );
}

#[test]
fn step_entry_keys_are_live_only_in_step_entry() {
    use core_control::bindings::{Chord, Keymap};
    use core_control::registry::KeyContext;
    let k = Keymap::default();
    let key =
        |t: &str, step: bool, reserve: bool| k.lookup(Chord::parse(t).unwrap(), step, reserve);
    assert_eq!(key("T", false, false), Some("step.entry"));
    for (text, id) in [
        ("1", "step.whole"),
        ("Keypad1", "step.whole"),
        ("2", "step.half"),
        ("3", "step.quarter"),
        ("4", "step.eighth"),
        ("5", "step.sixteenth"),
        ("6", "step.thirty_second"),
        (".", "step.dot"),
        ("Keypad.", "step.dot"),
        ("/", "step.triplet"),
        ("=", "step.tie"),
        ("Keypad+", "step.tie"),
        ("0", "step.rest"),
        ("Delete", "step.delete_last"),
    ] {
        assert_eq!(key(text, true, false), Some(id), "{text}");
    }
    assert_eq!(
        key("1", false, false),
        Some("locator.go_1"),
        "outside step entry"
    );
    assert_eq!(key(".", false, false), Some("transport.next_marker"));
    assert_eq!(key("Delete", false, false), None, "the editors' own key");
    assert_eq!(
        key("1", true, true),
        Some("locator.go_1"),
        "the row reserved"
    );
    assert_eq!(key("Keypad1", true, true), Some("step.whole"));
    let step: Vec<&str> = COMMANDS
        .iter()
        .filter(|c| c.context == KeyContext::StepEntry)
        .map(|c| c.id)
        .collect();
    assert_eq!(step.len(), 11);
    assert_eq!(
        err("command.run", json!({"id": "step.notes"})).0,
        ErrorCode::InvalidParams,
        "the pitches are needed"
    );
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "step.notes", "params": {"pitches": "C4 E4"}})
        ),
        Command::CommandRun {
            id: "step.notes".into(),
            args: vec![("pitches".into(), ArgValue::Text("C4 E4".into()))],
            window: None,
        }
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        170,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject; 5c-2 adds lane.track_synth, destination.set_return_latency and destination.measure_return; 5d-1 adds audio.set and destination.add_midi; 5e-1 adds automation.set_mode, automation.gesture and automation.suspend; 5e-2 adds the six automation.mode commands; 5f-1 adds transport.play, surface.add, surface.remove, surface.inject, surface.learn and surface.unlearn; 5f-2 adds file.bounce; 5f-3 adds region.open_external and region.reload_external; plan 7a: six ui.* commands; plan 7b: ui.theme; plan 7c: editors.audio_editor; plan 7d: transport.rewind and transport.fast_forward; plan tracks: track.new_audio, track.new_instrument, track.new_midi, track.duplicate, track.delete and track.rename; the Drum Grid turns drum.mark_lane's job on by itself, so it is gone (-1); 5g-1 adds lane.set_input_fx; 5g-1b adds destination.set_midi_mix; 5g-2 adds lane.set_mpe and destination.set_mpe; 6a adds file.import_midi, file.export_midi, file.export_midi_clip and file.interchange_report"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
        assert!(
            c.id.contains('.')
                && c.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'_'),
            "{}",
            c.id
        );
        for d in c.dialogs {
            for a in d.answered_by {
                assert!(
                    c.params.iter().any(|p| p.name == *a),
                    "{}: {} answers nothing",
                    c.id,
                    a
                );
            }
        }
    }
    assert_eq!(
        err("command.run", json!({"id": "file.print"})).0,
        ErrorCode::UnknownId
    );
    let (code, msg) = err("command.run", json!({"id": "file.save_as"}));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(msg.contains("\"Save\"") && msg.contains("`path`"), "{msg}");
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "file.save_as", "params": {"path": "/tmp/a.vizdaw"}, "window": 7})
        ),
        Command::CommandRun {
            id: "file.save_as".into(),
            args: vec![("path".into(), ArgValue::Text("/tmp/a.vizdaw".into()))],
            window: Some(WindowRef::Number(7)),
        }
    );
    assert_eq!(
        err(
            "command.run",
            json!({"id": "transport.loop", "params": {"on": "yes"}})
        )
        .0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err("command.run", json!({"id": "modify.template"})).0,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        err(
            "command.run",
            json!({"id": "edit.undo", "params": {"count": 2}})
        )
        .0,
        ErrorCode::InvalidParams
    );
    let list = registry::list_json();
    let first = &list["commands"][0];
    assert_eq!(first["id"], "file.new");
    assert_eq!(
        first["keys"],
        json!(["⌘N"]),
        "default keys as menus show them"
    );
    assert_eq!(first["menu_item"], true);
    assert_eq!(first["context"], "global");
    assert_eq!(list["commands"].as_array().unwrap().len(), COMMANDS.len());
    let open = list["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "file.open")
        .unwrap();
    assert_eq!(open["dialogs"][0]["answered_by"][0], "path");
    assert_eq!(open["params"][1]["values"][1], "off");
}

#[test]
fn shortcuts_name_their_menu_commands() {
    use core_control::keys::{Modifiers, named_key};
    let cmd = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    let stroke = |k: &str, m: Modifiers| registry::command_for_stroke(&named_key(k, m).unwrap());
    assert_eq!(
        stroke("space", Modifiers::default()),
        Some("transport.play_stop")
    );
    assert_eq!(
        stroke("return", Modifiers::default()),
        Some("transport.return_to_start")
    );
    assert_eq!(stroke("s", cmd), Some("file.save"));
    assert_eq!(stroke("S", cmd), Some("file.save_as"), "shift");
    assert_eq!(
        stroke(
            "1",
            Modifiers {
                option: true,
                ..cmd
            }
        ),
        Some("editors.piano_roll_window")
    );
    assert_eq!(
        stroke(
            "left",
            Modifiers {
                option: true,
                ..Modifiers::default()
            }
        ),
        Some("editors.nudge_earlier")
    );
    assert_eq!(
        stroke("x", Modifiers::default()),
        None,
        "an unbound letter is nothing"
    );
    // Global shortcuts name their commands; step entry's keys are not
    // global (plan 4f: they share keys with the locators).
    for c in COMMANDS
        .iter()
        .filter(|c| c.context == core_control::registry::KeyContext::Global)
    {
        if let Some(s) = c.shortcut {
            let k = registry::shortcut_stroke(s).unwrap_or_else(|| panic!("{}: {s}", c.id));
            assert_eq!(
                registry::command_for_stroke(&k),
                Some(c.id),
                "{s} is ambiguous"
            );
        }
    }
}

#[test]
fn wait_conditions_read_the_facts() {
    let doc = |playing, tick, kind: &str| Facts {
        document: Some(DocFacts {
            playing,
            tick,
            detail_kind: kind.into(),
        }),
        documents: 1,
        window_titles: vec!["Untitled".into(), "Mixer — Untitled".into()],
    };
    assert!(wait::met(&WaitCondition::Playing, &doc(true, 0, "none")));
    assert!(
        !wait::met(&WaitCondition::Playing, &Facts::default()),
        "no document, never met"
    );
    assert!(!wait::met(&WaitCondition::Stopped, &Facts::default()));
    assert!(wait::met(
        &WaitCondition::TickAtLeast(10),
        &doc(false, 10, "none")
    ));
    assert!(!wait::met(
        &WaitCondition::TickAtLeast(11),
        &doc(true, 10, "none")
    ));
    assert!(wait::met(
        &WaitCondition::DocumentsAtLeast(1),
        &doc(false, 0, "none")
    ));
    assert!(wait::met(
        &WaitCondition::WindowTitled("Mixer".into()),
        &doc(false, 0, "none")
    ));
    assert!(!wait::met(
        &WaitCondition::WindowGone("Mixer".into()),
        &doc(false, 0, "none")
    ));
    assert!(wait::met(
        &WaitCondition::DetailKind("list".into()),
        &doc(false, 0, "list")
    ));
    let d = wait::timeout_details(&WaitCondition::Playing, 500, &doc(false, 7, "none"));
    assert_eq!(d["facts"]["document"]["tick"], 7);
    assert_eq!(d["condition"], "playing");
}

mod fuzz {
    //! Random parameters for every command and every registry entry never
    //! panic; valid ones stay valid (spec §11).
    use super::*;
    use proptest::prelude::*;

    fn any_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::from),
            any::<i64>().prop_map(Value::from),
            (-1e12f64..1e12).prop_map(Value::from),
            ".{0,24}".prop_map(Value::from),
            Just(Value::from("0.1:00000000")),
            Just(Value::from("/tmp/a.vizdaw")),
            Just(Value::from("space")),
            Just(json!({"x": 1, "y": 2})),
            Just(json!(["command"])),
        ];
        leaf.prop_recursive(3, 32, 6, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..6).prop_map(Value::from),
                prop::collection::btree_map("[a-z_]{0,10}", inner, 0..6)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        })
    }

    /// Parameter names any command uses, so random maps hit real keys.
    fn key() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("id"),
            Just("params"),
            Just("window"),
            Just("target"),
            Just("element"),
            Just("point"),
            Just("from"),
            Just("to"),
            Just("steps"),
            Just("button"),
            Just("count"),
            Just("modifiers"),
            Just("key"),
            Just("text"),
            Just("path"),
            Just("scale"),
            Just("audio"),
            Just("plugins"),
            Just("restore_autosave"),
            Just("condition"),
            Just("tick"),
            Just("title"),
            Just("kind"),
            Just("timeout_ms"),
            Just("max_depth"),
            Just("on"),
            Just("name"),
            Just("pitches"),
        ]
        .prop_map(str::to_string)
    }

    fn params() -> impl Strategy<Value = Value> {
        prop::collection::btree_map(key(), any_json(), 0..6)
            .prop_map(|m| Value::Object(m.into_iter().collect()))
    }

    /// What validation may refuse with; anything else is a bug.
    const DOCUMENTED: [ErrorCode; 4] = [
        ErrorCode::InvalidParams,
        ErrorCode::UnknownCommand,
        ErrorCode::UnknownId,
        ErrorCode::DialogRequired,
    ];

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2048))]

        #[test]
        fn every_command_survives_random_params(i in 0..COMMAND_NAMES.len(), p in params()) {
            let cmd = COMMAND_NAMES[i];
            match validate(&req(cmd, p)) {
                Ok(_) => {}
                Err(e) => {
                    prop_assert!(DOCUMENTED.contains(&e.code), "{:?}", e.code);
                    let line = core_control::protocol::reply_line(
                        Some(&RequestId::Num(1)),
                        &Err(e),
                    );
                    prop_assert!(serde_json::from_str::<Value>(line.trim()).is_ok(), "{line}");
                }
            }
        }

        #[test]
        fn every_registry_entry_survives_random_params(i in 0..COMMANDS.len(), p in params(), w in any_json()) {
            let id = COMMANDS[i].id;
            let r = validate(&req("command.run", json!({"id": id, "params": p, "window": w})));
            if let Ok(Command::CommandRun { id: got, .. }) = r {
                prop_assert_eq!(got, id);
            }
        }
    }
}

#[test]
fn file_new_takes_a_format() {
    let Command::CommandRun { args, .. } = ok(
        "command.run",
        json!({ "id": "file.new", "params": { "rate": "44100", "depth": "16" } }),
    ) else {
        panic!()
    };
    assert_eq!(
        args,
        vec![
            ("rate".to_string(), ArgValue::Text("44100".into())),
            ("depth".to_string(), ArgValue::Text("16".into()))
        ]
    );
    let (code, msg) = err(
        "command.run",
        json!({ "id": "file.new", "params": { "rate": "22050" } }),
    );
    assert_eq!(code, ErrorCode::InvalidParams);
    assert!(msg.contains("rate"), "{msg}");
}

#[test]
fn media_import_takes_any_absolute_file_and_a_place() {
    let Command::CommandRun { args, .. } = ok(
        "command.run",
        json!({ "id": "media.import",
                "params": { "path": "/tmp/a b.wav", "seq": 1, "lane": 2, "tick": 3840 } }),
    ) else {
        panic!()
    };
    assert_eq!(
        args,
        vec![
            ("path".to_string(), ArgValue::Text("/tmp/a b.wav".into())),
            ("seq".to_string(), ArgValue::Int(1)),
            ("lane".to_string(), ArgValue::Int(2)),
            ("tick".to_string(), ArgValue::Int(3840)),
        ]
    );
    // The dialog is answered only by `path`.
    let (code, _) = err("command.run", json!({ "id": "media.import" }));
    assert_eq!(code, ErrorCode::DialogRequired);
    let (code, _) = err(
        "command.run",
        json!({ "id": "media.import", "params": { "path": "relative.wav" } }),
    );
    assert_eq!(code, ErrorCode::InvalidParams);
    let (code, _) = err(
        "command.run",
        json!({ "id": "media.import", "params": { "path": "/a.wav", "seq": -1 } }),
    );
    assert_eq!(code, ErrorCode::InvalidParams);
}

#[test]
fn project_settings_is_a_dialog_without_a_format() {
    let (code, _) = err("command.run", json!({ "id": "file.project_settings" }));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(matches!(
        ok(
            "command.run",
            json!({ "id": "file.project_settings", "params": { "depth": "32f" } })
        ),
        Command::CommandRun { .. }
    ));
    assert!(matches!(
        ok("command.run", json!({ "id": "media.convert" })),
        Command::CommandRun { .. }
    ));
}

/// Plan 5b: the media pool's and the Region menu's commands take their
/// dialogs' answers as parameters; hostile values are refused by type
/// and range before they reach the app.
#[test]
fn media_and_region_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    // Dialogs without their answers.
    for (id, dialog) in [
        ("media.relink", "Relink"),
        ("media.rename", "Rename Media"),
        ("region.normalize", "Normalize"),
    ] {
        let (code, message) = err("command.run", run(id, json!({})));
        assert_eq!(code, ErrorCode::DialogRequired, "{id}");
        assert!(message.contains(dialog), "{id}: {message}");
    }
    // The good forms.
    let Command::CommandRun { args, .. } = ok(
        "command.run",
        run(
            "media.relink",
            json!({ "media": 3, "path": "/tmp/x y.wav" }),
        ),
    ) else {
        panic!()
    };
    assert_eq!(
        args,
        vec![
            ("media".to_string(), ArgValue::Int(3)),
            ("path".to_string(), ArgValue::Text("/tmp/x y.wav".into())),
        ]
    );
    for (id, params) in [
        ("media.rename", json!({ "media": 0, "name": "Kick" })),
        ("region.normalize", json!({ "peak_db": "-0.3" })),
        ("media.compact", json!({})),
        ("window.media_pool", json!({})),
        ("region.fade_in_to_cursor", json!({})),
        ("region.fade_out_from_cursor", json!({})),
        ("region.remove_fades", json!({})),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params)),
                Command::CommandRun { .. }
            ),
            "{id}"
        );
    }
    // Hostile values.
    for (id, params) in [
        ("media.relink", json!({ "media": -1, "path": "/a.wav" })),
        (
            "media.relink",
            json!({ "media": i64::MAX, "path": "/a.wav" }),
        ),
        ("media.relink", json!({ "media": 1.5, "path": "/a.wav" })),
        ("media.relink", json!({ "media": "3", "path": "/a.wav" })),
        (
            "media.relink",
            json!({ "media": 3, "path": "relative.wav" }),
        ),
        ("media.relink", json!({ "media": 3, "path": "" })),
        ("media.relink", json!({ "media": 3, "path": 7 })),
        (
            "media.rename",
            json!({ "media": 3, "name": "x".repeat(100_000) }),
        ),
        ("media.rename", json!({ "media": 3, "name": ["x"] })),
        ("region.normalize", json!({ "peak_db": -1 })),
        ("region.normalize", json!({ "peak_db": null, "extra": 1 })),
        ("media.compact", json!({ "media": 1 })),
        ("region.remove_fades", json!({ "events": [1] })),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    // Where they appear.
    let spec = |id: &str| registry::find(id).unwrap();
    assert_eq!(
        (spec("media.relink").menu, spec("media.relink").menu_item),
        ("File", true)
    );
    assert_eq!(
        (spec("media.rename").menu, spec("media.rename").menu_item),
        ("Media", false)
    );
    assert_eq!(
        (spec("media.compact").menu, spec("media.compact").menu_item),
        ("Media", false)
    );
    assert_eq!(spec("window.media_pool").menu, "Window");
    for id in [
        "region.normalize",
        "region.fade_in_to_cursor",
        "region.fade_out_from_cursor",
        "region.remove_fades",
    ] {
        assert_eq!(
            (spec(id).menu, spec(id).menu_item, spec(id).document),
            ("Region", true, true),
            "{id}"
        );
    }
}

#[test]
fn state_get_media_and_project() {
    assert!(matches!(
        ok("state.get", json!({ "target": "media" })),
        Command::StateGet {
            target: StateTarget::Media,
            ..
        }
    ));
    assert!(matches!(
        ok("state.get", json!({ "target": "project" })),
        Command::StateGet {
            target: StateTarget::Project,
            ..
        }
    ));
}

/// Plan 5c Task 14: the record and lane commands take their parameters
/// by type and range; hostile values never reach the app.
#[test]
fn record_and_lane_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("record.set", json!({})),
        (
            "record.set",
            json!({ "replace": false, "punch": true, "loop": true, "count_in": 0, "pre_roll": 8, "passes_as_takes": false }),
        ),
        ("record.recover", json!({ "action": "import" })),
        ("record.recover", json!({ "action": "trash" })),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 3, "stereo": true }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "midi", "port": "Keys", "midi_channel": 16 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "none" }),
        ),
        (
            "lane.set_monitor",
            json!({ "seq": 1, "lane": 2, "mode": "input" }),
        ),
        (
            "midi.inject",
            json!({ "port": "Keys", "bytes": "90 3c 64" }),
        ),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    for (id, params) in [
        ("record.set", json!({ "count_in": 5 })),
        ("record.set", json!({ "pre_roll": -1 })),
        ("record.set", json!({ "loop": "yes" })),
        ("record.set", json!({ "takes": true })),
        ("record.recover", json!({})),
        ("record.recover", json!({ "action": "delete" })),
        ("lane.set_input", json!({ "lane": 2, "kind": "audio" })),
        ("lane.set_input", json!({ "seq": 1, "lane": 2 })),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "video" }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 0 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 257 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "midi", "midi_channel": 17 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": -1, "lane": 2, "kind": "none" }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "midi", "port": "x".repeat(100_000) }),
        ),
        ("lane.set_monitor", json!({ "seq": 1, "lane": 2 })),
        (
            "lane.set_monitor",
            json!({ "seq": 1, "lane": 2, "mode": "loud" }),
        ),
        ("midi.inject", json!({ "port": "Keys" })),
        ("midi.inject", json!({ "bytes": "90 3c 64" })),
        (
            "midi.inject",
            json!({ "port": "Keys", "bytes": [0x90, 60, 100] }),
        ),
        (
            "midi.inject",
            json!({ "port": "Keys", "bytes": "9".repeat(100_000) }),
        ),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    let spec = |id: &str| registry::find(id).unwrap();
    for id in [
        "record.set",
        "record.recover",
        "lane.set_input",
        "lane.set_monitor",
        "midi.inject",
    ] {
        assert!(!spec(id).menu_item && spec(id).document, "{id}");
    }
}

/// Plan 5c-2 Task 10: the synth-tracking commands, and `lane.set_input`'s
/// `return_of`, take their parameters by type and range; hostile values
/// never reach the app.
#[test]
fn synth_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("lane.track_synth", json!({ "seq": 1, "lane": 2 })),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "channel": 3, "stereo": true, "arm_midi": true, "mute_after": false }),
        ),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "channel": 256 }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": "12.5" }),
        ),
        ("destination.measure_return", json!({ "dest": 4 })),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": 90, "pan": null, "mute": true }),
        ),
        ("destination.set_midi_mix", json!({ "dest": 4, "pan": 0 })),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 3, "return_of": 4 }),
        ),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    for (id, params) in [
        ("lane.track_synth", json!({ "lane": 2 })),
        ("lane.track_synth", json!({ "seq": 1 })),
        ("lane.track_synth", json!({})),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "channel": 0 }),
        ),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "channel": 257 }),
        ),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "channel": 1.5 }),
        ),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "stereo": "yes" }),
        ),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "arm_midi": 1 }),
        ),
        ("lane.track_synth", json!({ "seq": 1, "lane": -2 })),
        ("lane.track_synth", json!({ "seq": "1", "lane": 2 })),
        ("lane.track_synth", json!({ "seq": 1, "lane": u64::MAX })),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "channels": 3 }),
        ),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "junk": "x".repeat(100_000) }),
        ),
        ("destination.set_return_latency", json!({ "dest": 4 })),
        ("destination.set_return_latency", json!({ "ms": "12" })),
        // The milliseconds are text: a JSON number (negative, above 500
        // or fractional) is the wrong type; the text's range is the
        // app's check.
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": 12.5 }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": -1 }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": 501 }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": null }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": ["12"] }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": "x".repeat(100_000) }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": -4, "ms": "12" }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": "Juno", "ms": "12" }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": "12", "wait": true }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": "12", "junk": "x".repeat(100_000) }),
        ),
        ("destination.set_midi_mix", json!({ "volume": 90 })),
        ("destination.set_midi_mix", json!({ "dest": 4 })),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": 128 }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": -1 }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": 1.5 }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": "90" }),
        ),
        ("destination.set_midi_mix", json!({ "dest": 4, "pan": 128 })),
        ("destination.set_midi_mix", json!({ "dest": 4, "pan": -1 })),
        ("destination.set_midi_mix", json!({ "dest": 4, "pan": 1.5 })),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "pan": "none" }),
        ),
        ("destination.set_midi_mix", json!({ "dest": 4, "pan": [1] })),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "mute": "yes" }),
        ),
        ("destination.set_midi_mix", json!({ "dest": 4, "mute": 1 })),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": 1, "wait": true }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": "x".repeat(100_000) }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "mute": "x".repeat(100_000) }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4, "volume": 1, "junk": "x".repeat(100_000) }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": -4, "volume": 1 }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": "Juno", "volume": 1 }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": 4.5, "volume": 1 }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": u64::MAX, "volume": 1 }),
        ),
        (
            "destination.set_midi_mix",
            json!({ "dest": null, "volume": 1 }),
        ),
        (
            "destination.measure_return",
            json!({ "dest": 4, "junk": "x".repeat(100_000) }),
        ),
        ("destination.measure_return", json!({})),
        ("destination.measure_return", json!({ "dest": "Juno" })),
        ("destination.measure_return", json!({ "dest": -1 })),
        ("destination.measure_return", json!({ "dest": 4.5 })),
        (
            "destination.measure_return",
            json!({ "dest": 4, "wait": true }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "return_of": -1 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "return_of": "Juno" }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "return_of": 1.5 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "return_of": u64::MAX }),
        ),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    let spec = |id: &str| registry::find(id).unwrap();
    for id in [
        "lane.track_synth",
        "destination.set_return_latency",
        "destination.measure_return",
        "destination.set_midi_mix",
    ] {
        assert!(!spec(id).menu_item && spec(id).document, "{id}");
        assert!(spec(id).dialogs.is_empty(), "{id} opens no dialog");
    }
    // Without `return_of`, `lane.set_input` unlinks (plan 5c-2 Task 8):
    // its help says so.
    let return_of = spec("lane.set_input")
        .params
        .iter()
        .find(|p| p.name == "return_of")
        .unwrap();
    assert!(return_of.help.contains("unlink"), "{}", return_of.help);
}

/// `destination.set_midi_mix` (plan 5g-1b Task 4): volume and pan are
/// JSON integers 0-127, or null for "unset" (which reaches the app as the
/// text `none`, since a bare null counts as absent everywhere else); an
/// absent key keeps the current value.
#[test]
fn midi_mix_params_are_checked() {
    let spec = registry::find("destination.set_midi_mix").expect("registered");
    assert!(spec.document && !spec.menu_item && spec.dialogs.is_empty());
    let names: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
    assert_eq!(names, ["dest", "volume", "pan", "mute"]);
    let args = |params: Value| match ok(
        "command.run",
        json!({ "id": "destination.set_midi_mix", "params": params }),
    ) {
        Command::CommandRun { args, .. } => args,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        args(json!({ "dest": 4, "volume": 90, "pan": null, "mute": false })),
        [
            ("dest".to_string(), ArgValue::Int(4)),
            ("volume".to_string(), ArgValue::Int(90)),
            ("pan".to_string(), ArgValue::Text("none".into())),
            ("mute".to_string(), ArgValue::Flag(false)),
        ]
    );
    assert_eq!(
        args(json!({ "dest": 4, "mute": true })),
        [
            ("dest".to_string(), ArgValue::Int(4)),
            ("mute".to_string(), ArgValue::Flag(true)),
        ]
    );
    assert_eq!(args(json!({ "dest": 4, "volume": 0, "pan": 127 })).len(), 3);
}

#[test]
fn state_get_record_and_input() {
    for (name, target) in [
        ("record", StateTarget::Record),
        ("input", StateTarget::Input),
    ] {
        match ok("state.get", json!({ "target": name })) {
            Command::StateGet { target: t, .. } => assert_eq!(t, target),
            other => panic!("{other:?}"),
        }
    }
}

/// Plan 5d-1 Task 12 (Rulings 25–26): `audio.set` and
/// `destination.add_midi` take their values by type and range; the app
/// checks the buffer size's six values and the text.
#[test]
fn audio_and_destination_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("audio.set", json!({ "software_monitoring": false })),
        (
            "audio.set",
            json!({ "buffer_frames": 256, "input_device": "", "latency_trim": -48 }),
        ),
        ("audio.set", json!({ "buffer_frames": 32 })),
        ("audio.set", json!({ "latency_trim": 192_000 })),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "Juno MIDI" }),
        ),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "Juno MIDI", "channel": 16 }),
        ),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    for (id, params) in [
        ("audio.set", json!({ "buffer_frames": 31 })),
        ("audio.set", json!({ "buffer_frames": 1025 })),
        ("audio.set", json!({ "buffer_frames": "128" })),
        ("audio.set", json!({ "buffer_frames": 128.5 })),
        ("audio.set", json!({ "software_monitoring": "off" })),
        ("audio.set", json!({ "latency_trim": 192_001 })),
        ("audio.set", json!({ "latency_trim": -192_001 })),
        ("audio.set", json!({ "input_device": 3 })),
        ("audio.set", json!({ "junk": "x".repeat(100_000) })),
        ("destination.add_midi", json!({ "name": "Juno" })),
        ("destination.add_midi", json!({ "port": "Juno MIDI" })),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "P", "channel": 0 }),
        ),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "P", "channel": 17 }),
        ),
        ("destination.add_midi", json!({ "name": 4, "port": "P" })),
        (
            "destination.add_midi",
            json!({ "name": "x".repeat(100_000), "port": "P" }),
        ),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
}

/// Plan 5e-1 Task 10: the automation commands take their parameters by
/// type and range; hostile values never reach the app.
#[test]
fn automation_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        (
            "automation.set_mode",
            json!({ "strips": "all", "mode": "touch" }),
        ),
        (
            "automation.set_mode",
            json!({ "strips": "1,2,3", "mode": "off" }),
        ),
        (
            "automation.gesture",
            json!({ "target": "fader:3", "phase": "touch" }),
        ),
        (
            "automation.gesture",
            json!({ "target": "send:3:1", "phase": "value", "value": "0.25" }),
        ),
        (
            "automation.gesture",
            json!({ "target": "param:9:100", "phase": "release" }),
        ),
        ("automation.suspend", json!({})),
        ("automation.suspend", json!({ "on": true })),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    for (id, params) in [
        ("automation.set_mode", json!({ "mode": "touch" })),
        ("automation.set_mode", json!({ "strips": "all" })),
        (
            "automation.set_mode",
            json!({ "strips": "all", "mode": "loud" }),
        ),
        (
            "automation.set_mode",
            json!({ "strips": [1, 2], "mode": "read" }),
        ),
        (
            "automation.set_mode",
            json!({ "strips": "1".repeat(100_000), "mode": "read" }),
        ),
        ("automation.gesture", json!({ "phase": "touch" })),
        ("automation.gesture", json!({ "target": "fader:3" })),
        (
            "automation.gesture",
            json!({ "target": "fader:3", "phase": "wiggle" }),
        ),
        (
            "automation.gesture",
            json!({ "target": "fader:3", "phase": "value", "value": 0.5 }),
        ),
        (
            "automation.gesture",
            json!({ "target": 3, "phase": "touch" }),
        ),
        ("automation.suspend", json!({ "on": "yes" })),
        ("automation.suspend", json!({ "now": true })),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    let spec = |id: &str| registry::find(id).unwrap();
    assert!(!spec("automation.set_mode").menu_item && spec("automation.set_mode").document);
    assert!(spec("automation.gesture").document);
    assert!(!spec("automation.suspend").document);
    assert_eq!(
        ok("state.get", json!({"target": "automation"})),
        Command::StateGet {
            target: StateTarget::Automation,
            window: None
        }
    );
}

/// Plan 5e-2 Task 4: the mode commands are Automation menu items with no
/// parameters; Suspend Automation is a menu item now.
#[test]
fn automation_menu_commands_take_no_params() {
    for m in ["off", "read", "touch", "latch", "write", "trim"] {
        let id = format!("automation.mode.{m}");
        let spec = registry::find(&id).unwrap();
        assert!(
            spec.menu_item && spec.document && spec.menu == "Automation",
            "{id}"
        );
        assert!(matches!(
            ok("command.run", json!({ "id": id })),
            Command::CommandRun { .. }
        ));
        let (code, _) = err(
            "command.run",
            json!({ "id": id, "params": { "strips": "all" } }),
        );
        assert_eq!(code, ErrorCode::InvalidParams, "{id}");
    }
    let s = registry::find("automation.suspend").unwrap();
    assert!(s.menu_item && s.menu == "Automation" && !s.document);
}

/// Plan 5f-1 Task 9: the surface commands take their parameters by type
/// and range; hostile values never reach the app.
#[test]
fn surface_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("transport.play", json!({})),
        (
            "surface.add",
            json!({ "type": "mcu", "input": "A", "output": "B" }),
        ),
        (
            "surface.add",
            json!({ "type": "mcu_xt", "input": "A", "output": "B", "position": 3 }),
        ),
        (
            "surface.add",
            json!({ "type": "hui", "input": "A", "output": "B" }),
        ),
        ("surface.remove", json!({ "surface": 1 })),
        (
            "surface.inject",
            json!({ "surface": 1, "bytes": "90 68 7f" }),
        ),
        ("surface.inject", json!({ "surface": 1, "bytes": "90 68" })),
        ("surface.learn", json!({ "target": "fader:3" })),
        ("surface.learn", json!({ "command": "transport.play_stop" })),
        ("surface.unlearn", json!({ "target": "fader:3" })),
        (
            "surface.unlearn",
            json!({ "command": "transport.play_stop" }),
        ),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    for (id, params) in [
        (
            "surface.add",
            json!({ "type": "mackie", "input": "A", "output": "B" }),
        ),
        ("surface.add", json!({ "type": "mcu", "input": "A" })),
        (
            "surface.add",
            json!({ "type": "mcu", "input": "A", "output": "B", "position": 4 }),
        ),
        (
            "surface.add",
            json!({ "type": "mcu", "input": 7, "output": "B" }),
        ),
        ("surface.remove", json!({ "surface": -1 })),
        ("surface.remove", json!({})),
        ("surface.inject", json!({ "surface": 1 })),
        (
            "surface.inject",
            json!({ "surface": 1, "bytes": "x".repeat(5000) }),
        ),
        ("surface.learn", json!({ "target": 3 })),
    ] {
        assert!(
            err("command.run", run(id, params.clone())).0 == ErrorCode::InvalidParams,
            "{id} {params}"
        );
    }
    assert_eq!(
        ok("state.get", json!({ "target": "surfaces" })),
        Command::StateGet {
            target: StateTarget::Surfaces,
            window: None
        }
    );
}

/// Plan 5f-2 Task 5: `file.bounce` takes the Bounce dialog's answers as
/// parameters; hostile values are refused by type and range before they
/// reach the app (strip lists are text here; the app checks their form).
#[test]
fn bounce_commands_check_their_params() {
    let run = |params: Value| json!({ "id": "file.bounce", "params": params });
    for good in [
        json!({ "source": "mix", "path": "/tmp/x.wav" }),
        json!({ "add_to_project": true }),
        json!({ "source": "stems", "strips": "1,2", "path": "/tmp/stems" }),
        json!({ "range": "selection", "start": 0, "end": 15360, "path": "/tmp/x.aif", "format": "aiff", "bits": "16", "rate": 44100, "dither": false }),
        json!({ "path": "/tmp/x.wav", "replace": true, "bits": "32f", "rate": 8000 }),
        // Presence answers the dialog here; the app refuses this one (pre-flight M6).
        json!({ "add_to_project": false }),
        json!({ "source": "stems", "strips": "1,,2", "path": "/tmp/stems" }),
    ] {
        assert!(
            matches!(
                ok("command.run", run(good.clone())),
                Command::CommandRun { .. }
            ),
            "{good}"
        );
    }
    for bad in [
        json!({ "path": "/tmp/x.wav", "format": "mp3" }),
        json!({ "path": "/tmp/x.wav", "bits": "8" }),
        json!({ "path": "/tmp/x.wav", "bits": 16 }),
        json!({ "path": "/tmp/x.wav", "rate": 1000 }),
        json!({ "path": "/tmp/x.wav", "rate": 384_001 }),
        json!({ "path": "relative.wav" }),
        json!({ "path": "/tmp/x.wav", "start": -1 }),
        json!({ "path": "/tmp/x.wav", "end": i64::MAX }),
        json!({ "path": "/tmp/x.wav", "source": "both" }),
        json!({ "path": "/tmp/x.wav", "dither": "yes" }),
        json!({ "path": format!("/tmp/{}.wav", "a".repeat(1100)) }),
        json!({ "path": "/tmp/x.wav", "seq": 1 }),
    ] {
        let (code, _) = err("command.run", run(bad.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
    }
    // More wrong types and values (Task 5 dispatch): each is refused here.
    for bad in [
        json!({ "path": 7 }),
        json!({ "path": "/tmp/x\u{0}.wav" }),
        json!({ "path": "/tmp/x.wav", "strips": 12 }),
        json!({ "path": "/tmp/x.wav", "strips": ["1", "2"] }),
        json!({ "path": "/tmp/x.wav", "range": "all" }),
        json!({ "path": "/tmp/x.wav", "rate": "48000" }),
        json!({ "path": "/tmp/x.wav", "rate": 48_000.5 }),
        json!({ "path": "/tmp/x.wav", "add_to_project": "true" }),
        json!({ "path": "/tmp/x.wav", "replace": 1 }),
        json!({ "path": "/tmp/x.wav", "format": "WAV" }),
    ] {
        let (code, _) = err("command.run", run(bad.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
    }
    // These pass here and are refused further in: `..` and a place inside
    // the open package by the core (Ruling 3, `hostile_bounce_requests_are_refused`),
    // the largest range by the core's length check.
    for passes in [
        json!({ "path": "/tmp/../x.wav" }),
        json!({ "range": "selection", "start": 0, "end": i64::MAX / 2, "path": "/tmp/x.wav" }),
    ] {
        assert!(
            matches!(
                ok("command.run", run(passes.clone())),
                Command::CommandRun { .. }
            ),
            "{passes}"
        );
    }
    let (code, message) = err("command.run", run(json!({})));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(message.contains("Bounce"), "{message}");
}

/// Plan 6a Task 8: the MIDI import and export commands check their
/// parameters; the exports' "Save" dialogs are not `always` (F2): a missing
/// path passes here and the app answers it, so `dry_run` needs none.
#[test]
fn interchange_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    let good_runs = [
        ("file.import_midi", json!({ "path": "/tmp/a.mid" })),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "new_project": true, "seq": 1, "lane": 2, "tick": -(1i64 << 40),
                    "channel_map": "[{\"group\":0,\"channel\":9,\"dest\":null}]",
                    "split_channels": false, "mpe": true, "use_tempo": true }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.midi2", "tick": 1i64 << 40 }),
        ),
        ("file.export_midi", json!({ "path": "/tmp/a.mid" })),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "replace": true, "seq": 1, "type": "0", "ppq": "15360",
                    "lanes": "1,2", "mpe": true, "dry_run": true }),
        ),
        ("file.export_midi", json!({ "dry_run": true })),
        ("file.export_midi", json!({})),
        ("file.export_midi_clip", json!({ "path": "/tmp/a.midi2" })),
        ("file.export_midi_clip", json!({ "dry_run": true })),
        ("file.export_midi_clip", json!({})),
        (
            "file.export_midi_clip",
            json!({ "path": "/tmp/dir", "replace": true, "seq": 1, "lanes": "1", "start": 0, "end": 1i64 << 40 }),
        ),
        ("file.interchange_report", json!({})),
    ];
    for (id, params) in good_runs {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    let bad_runs = [
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "ppq": "0" }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "ppq": "70000" }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "ppq": 960 }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "type": "2" }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "type": 1 }),
        ),
        ("file.export_midi", json!({ "path": "relative.mid" })),
        ("file.export_midi", json!({ "path": "/tmp/a.midi" })),
        ("file.export_midi", json!({ "path": "/tmp/a.wav" })),
        ("file.export_midi", json!({ "path": "/tmp/a\u{0}.mid" })),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "lanes": ["1"] }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "dry_run": "yes" }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "start": 0 }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "extra": 1 }),
        ),
        (
            "file.export_midi",
            json!({ "path": "/tmp/a.mid", "seq": -1 }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "tick": 1i64 << 41 }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "tick": -(1i64 << 41) }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "lane": -1 }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "lane": i64::MAX }),
        ),
        ("file.import_midi", json!({ "path": "relative.mid" })),
        ("file.import_midi", json!({ "path": "/tmp/a\u{0}.mid" })),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "extra": true }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "dry_run": true }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "mpe": "yes" }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "channel_map": 7 }),
        ),
        (
            "file.import_midi",
            json!({ "path": "/tmp/a.mid", "channel_map": "x".repeat(10_000) }),
        ),
        ("file.export_midi_clip", json!({ "path": "relative.midi2" })),
        (
            "file.export_midi_clip",
            json!({ "path": "/tmp/a.midi2", "start": -1 }),
        ),
        (
            "file.export_midi_clip",
            json!({ "path": "/tmp/a.midi2", "end": (1i64 << 40) + 1 }),
        ),
        (
            "file.export_midi_clip",
            json!({ "path": "/tmp/a.midi2", "ppq": "960" }),
        ),
        (
            "file.export_midi_clip",
            json!({ "path": "/tmp/a.midi2", "extra": 1 }),
        ),
        ("file.interchange_report", json!({ "path": "/tmp/a.mid" })),
    ];
    for (id, params) in bad_runs {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    let (code, message) = err("command.run", run("file.import_midi", json!({})));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(message.contains("Open"), "{message}");
    for id in ["file.export_midi", "file.export_midi_clip"] {
        let spec = registry::find(id).expect(id);
        assert_eq!(spec.dialogs.len(), 1, "{id}");
        let d = &spec.dialogs[0];
        assert!(
            d.name == "Save" && !d.always && d.answered_by == ["path"],
            "{id}"
        );
        assert_eq!(d.when, "no path and no dry_run", "{id}");
    }
    for id in [
        "file.import_midi",
        "file.export_midi",
        "file.export_midi_clip",
        "file.interchange_report",
    ] {
        let spec = registry::find(id).expect(id);
        assert!(spec.document && spec.menu == "File", "{id}");
    }
    assert!(
        registry::find("file.interchange_report")
            .unwrap()
            .params
            .is_empty()
    );
}

/// Plan 5f-3 (Ruling 5): `region.open_external` takes an optional
/// absolute `.app` path and opens no dialog for a programmatic call;
/// `region.reload_external` takes nothing.
#[test]
fn external_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for good in [
        json!({}),
        json!({ "app": "/Applications/X.app" }),
        json!({ "app": "/Applications/Sound Editor.app" }),
    ] {
        assert!(
            matches!(
                ok("command.run", run("region.open_external", good.clone())),
                Command::CommandRun { .. }
            ),
            "{good}"
        );
    }
    for bad in [
        json!({ "app": "X.app" }),
        json!({ "app": "/tmp/x.wav" }),
        json!({ "app": "/Applications/X" }),
        json!({ "app": 7 }),
        json!({ "app": "/tmp/x\u{0}.app" }),
        json!({ "app": format!("/{}.app", "a".repeat(1100)) }),
        json!({ "path": "/tmp/x.app" }),
    ] {
        let (code, _) = err("command.run", run("region.open_external", bad.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
    }
    assert!(matches!(
        ok("command.run", run("region.reload_external", json!({}))),
        Command::CommandRun { .. }
    ));
    let (code, _) = err(
        "command.run",
        run(
            "region.reload_external",
            json!({ "app": "/Applications/X.app" }),
        ),
    );
    assert_eq!(code, ErrorCode::InvalidParams, "Reload takes no parameters");
    for id in ["region.open_external", "region.reload_external"] {
        let c = COMMANDS.iter().find(|c| c.id == id).unwrap();
        assert!(c.document && c.menu == "Region" && c.menu_item, "{id}");
        assert!(
            c.dialogs.iter().all(|d| !d.always),
            "{id}: a programmatic call never needs a dialog"
        );
    }
    let open = COMMANDS
        .iter()
        .find(|c| c.id == "region.open_external")
        .unwrap();
    assert_eq!(open.dialogs[0].name, "Choose External Editor");
    assert_eq!(open.dialogs[0].answered_by, ["app"]);
}

/// Plan 5f-4 Ruling 2: `surface.add` takes the Kontrol type.
#[test]
fn kontrol_mk3_is_a_surface_type() {
    let run = |params: Value| json!({ "id": "surface.add", "params": params });
    assert!(matches!(
        ok(
            "command.run",
            run(json!({ "type": "kontrol_mk3", "input": "A", "output": "A" }))
        ),
        Command::CommandRun { .. }
    ));
    // Final review T1 m3: the Choice is exact; another casing is refused.
    let (code, _) = err(
        "command.run",
        run(json!({ "type": "Kontrol_MK3", "input": "A", "output": "A" })),
    );
    assert_eq!(code, ErrorCode::InvalidParams);
    let spec = registry::find("surface.add").unwrap();
    let help = spec.params.iter().find(|p| p.name == "type").unwrap().help;
    assert!(help.contains("kontrol_mk3"), "{help}");
}

/// Plan 7a Task 5: the view and pane commands refuse hostile params by type and range.
#[test]
fn ui_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for good in [
        json!({ "pane": "left" }),
        json!({ "pane": "bottom", "view": "piano_roll", "shown": true, "size": 300, "main": "mix" }),
        json!({ "pane": "right", "size": 0 }),
    ] {
        assert!(
            matches!(
                ok("command.run", run("ui.pane", good.clone())),
                Command::CommandRun { .. }
            ),
            "{good}"
        );
    }
    for bad in [
        json!({}),
        json!({ "pane": "top" }),
        json!({ "pane": "left", "view": "Mixer" }),
        json!({ "pane": "left", "view": 3 }),
        json!({ "pane": "left", "shown": "yes" }),
        json!({ "pane": "left", "size": 1e12 }),
        json!({ "pane": "left", "size": -1 }),
        json!({ "pane": "left", "size": "300" }),
        json!({ "pane": "left", "main": "edit" }),
        json!({ "pane": "left", "width": 300 }),
        json!({ "pane": "x".repeat(70_000) }),
    ] {
        assert_eq!(
            err("command.run", run("ui.pane", bad.clone())).0,
            ErrorCode::InvalidParams,
            "{bad}"
        );
    }
    for id in ["ui.view_arrange", "ui.view_mix"] {
        assert_eq!(
            err("command.run", run(id, json!({ "on": true }))).0,
            ErrorCode::InvalidParams,
            "{id}"
        );
    }
    for id in ["ui.toggle_left", "ui.toggle_bottom", "ui.toggle_right"] {
        assert!(matches!(
            ok("command.run", run(id, json!({ "on": false }))),
            Command::CommandRun { .. }
        ));
        assert_eq!(
            err("command.run", run(id, json!({ "on": "no" }))).0,
            ErrorCode::InvalidParams,
            "{id}"
        );
    }
    let keys: Vec<_> = [
        "ui.view_arrange",
        "ui.view_mix",
        "ui.toggle_left",
        "ui.toggle_bottom",
        "ui.toggle_right",
    ]
    .iter()
    .map(|id| COMMANDS.iter().find(|c| c.id == *id).unwrap().shortcut)
    .collect();
    assert_eq!(
        keys,
        [
            Some("⌃1"),
            Some("⌃2"),
            Some("⌃⌘←"),
            Some("⌃⌘↓"),
            Some("⌃⌘→")
        ]
    );
    assert!(
        !COMMANDS
            .iter()
            .find(|c| c.id == "ui.pane")
            .unwrap()
            .menu_item
    );
}

/// Plan 7b Task B4 (Ruling 11, controller ruling R-UT): `ui.theme` takes a
/// built-in theme's ID or a user theme's `user.` ID, and nothing else.
#[test]
fn ui_theme_checks_its_params() {
    let run = |params: Value| json!({ "id": "ui.theme", "params": params });
    for id in [
        "classic-light",
        "studio-dark",
        "flat",
        "console",
        "symbiotic",
        "eg13",
        "user.my-dark",
        "user.a",
        "user.2026-blue",
        "user.a1",
    ] {
        assert_eq!(
            ok("command.run", run(json!({ "theme": id }))),
            Command::CommandRun {
                id: "ui.theme".into(),
                args: vec![("theme".into(), ArgValue::Text(id.into()))],
                window: None,
            },
            "{id}"
        );
    }
    // The longest slug, 64 characters, is accepted.
    let longest = format!("user.{}", "a".repeat(64));
    assert!(
        matches!(
            ok("command.run", run(json!({ "theme": longest }))),
            Command::CommandRun { .. }
        ),
        "{longest}"
    );
    let long = format!("user.{}", "a".repeat(65));
    for bad in [
        json!({}),
        json!({ "theme": "svp-light" }),
        json!({ "theme": 7 }),
        json!({ "theme": "console", "extra": 1 }),
        json!({ "theme": "" }),
        json!({ "theme": "Console" }),
        json!({ "theme": "user." }),
        json!({ "theme": "user.My-Dark" }),
        json!({ "theme": "user.a/b" }),
        json!({ "theme": "user.a b" }),
        json!({ "theme": "user.-a" }),
        json!({ "theme": "user.a\u{0}" }),
        json!({ "theme": long }),
        json!({ "theme": "x".repeat(70_000) }),
        json!({ "theme": null }),
        json!({ "theme": ["flat"] }),
        json!({ "theme": "user.a-" }),
        json!({ "theme": "user.user.a" }),
        json!({ "theme": "user.café" }),
        json!({ "theme": "user.ａ" }),
        json!({ "theme": "flat " }),
        json!({ "theme": " flat" }),
        json!({ "theme": "user" }),
        json!({ "theme": "USER.a" }),
        json!({ "theme": true }),
        json!({ "theme": { "id": "flat" } }),
    ] {
        assert_eq!(
            err("command.run", run(bad.clone())).0,
            ErrorCode::InvalidParams,
            "{bad}"
        );
    }
    let spec = registry::find("ui.theme").unwrap();
    assert!(!spec.menu_item && !spec.document);
    assert_eq!((spec.menu, spec.title), ("View", "Theme"));
    let list = registry::list_json();
    let theme = list["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "ui.theme")
        .unwrap();
    assert_eq!(theme["params"][0]["type"], "choice");
    assert_eq!(theme["params"][0]["values"][5], "eg13");
    assert_eq!(theme["params"][0]["or_prefixed"], "user.");
}

#[test]
fn state_get_reads_layout() {
    assert_eq!(
        ok("state.get", json!({ "target": "layout" })),
        Command::StateGet {
            target: StateTarget::Layout,
            window: None
        }
    );
    let (code, msg) = err("state.get", json!({ "target": "panes" }));
    assert_eq!(code, ErrorCode::InvalidParams);
    assert!(msg.contains("surfaces, layout"), "{msg}");
}

/// Plan 7b Ruling 11: `state.get mixer` reads the strips; a near miss is
/// refused.
#[test]
fn mixer_state_target_parses() {
    assert_eq!(
        ok("state.get", json!({ "target": "mixer" })),
        Command::StateGet {
            target: StateTarget::Mixer,
            window: None
        }
    );
    for bad in [json!("mixers"), json!("Mixer"), json!(""), json!(3)] {
        let (code, msg) = err("state.get", json!({ "target": bad }));
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
        assert!(!msg.is_empty());
    }
    let (_, msg) = err("state.get", json!({ "target": "mixers" }));
    assert!(msg.contains("mixer or audio_editor"), "{msg}");
}

#[test]
fn audio_editor_state_target_parses() {
    assert_eq!(
        ok("state.get", json!({ "target": "audio_editor" })),
        Command::StateGet {
            target: StateTarget::AudioEditor,
            window: None
        }
    );
    for bad in [
        json!("audio-editor"),
        json!("audioEditor"),
        json!("AUDIO_EDITOR"),
        json!("audio_editor "),
        json!(""),
        json!(7),
    ] {
        let (code, msg) = err("state.get", json!({ "target": bad }));
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
        assert!(!msg.is_empty());
    }
    let (_, msg) = err("state.get", json!({ "target": "audio-editor" }));
    assert!(msg.contains("mixer or audio_editor"), "{msg}");
    // Task 5 review M1: an unknown extra param and a bad window.
    for bad in [
        json!({ "target": "audio_editor", "extra": 1 }),
        json!({ "target": "audio_editor", "window": 0 }),
        json!({ "target": "audio_editor", "window": "front" }),
        json!({ "target": "audio_editor", "window": -1 }),
    ] {
        let (code, msg) = err("state.get", bad.clone());
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
        assert!(!msg.is_empty());
    }
    assert_eq!(
        ok(
            "state.get",
            json!({ "target": "audio_editor", "window": 3 })
        ),
        Command::StateGet {
            target: StateTarget::AudioEditor,
            window: Some(WindowRef::Number(3))
        }
    );
}

#[test]
fn ui_pane_names_the_audio_editor() {
    let c = ok(
        "command.run",
        json!({ "id": "ui.pane", "params": { "pane": "bottom", "view": "audio_editor" } }),
    );
    assert!(matches!(c, Command::CommandRun { .. }), "{c:?}");
    for bad in ["audio", "Audio_Editor"] {
        let (code, msg) = err(
            "command.run",
            json!({ "id": "ui.pane", "params": { "pane": "bottom", "view": bad } }),
        );
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
        assert!(!msg.is_empty());
    }
    // Task 5 review M1: an unknown extra param, a bad window and a
    // wrong-typed view.
    for bad in [
        json!({ "id": "ui.pane", "params": { "pane": "bottom", "view": "audio_editor", "extra": 1 } }),
        json!({ "id": "ui.pane", "params": { "pane": "bottom", "view": "audio_editor" }, "window": 0 }),
        json!({ "id": "ui.pane", "params": { "pane": "bottom", "view": "audio_editor" }, "window": "front" }),
        json!({ "id": "ui.pane", "params": { "pane": "bottom", "view": 7 } }),
        json!({ "id": "ui.pane", "params": { "pane": "bottom", "view": ["audio_editor"] } }),
    ] {
        let (code, msg) = err("command.run", bad.clone());
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
        assert!(!msg.is_empty());
    }
    let c = registry::find("editors.audio_editor").unwrap();
    assert_eq!(c.shortcut, Some("⌘5"));
    assert_eq!(c.menu, "Editors");
    assert!(c.document);
}

/// Plan tracks Task 2 hostile input: the six `track.*` commands take
/// non-negative integer ids and, for rename, a string; anything else is
/// refused by the checker before it reaches the app.
#[test]
fn track_commands_refuse_hostile_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    let ids = [
        "track.new_audio",
        "track.new_instrument",
        "track.new_midi",
        "track.duplicate",
        "track.delete",
        "track.rename",
    ];
    for id in ids {
        let c = registry::find(id).unwrap();
        assert_eq!(c.menu, "Track", "{id}");
        assert!(c.menu_item && c.document, "{id}");
        for (params, word) in [
            (json!({"seq": -1}), "seq"),
            (json!({"seq": "x"}), "seq"),
            (json!({"seq": 1.5}), "seq"),
            (
                serde_json::from_str::<Value>(r#"{"seq": 18446744073709551616}"#).unwrap(),
                "seq",
            ),
            (json!({"after": -1}), "after"),
            (json!({"bogus": 1}), "bogus"),
            (json!("x"), ""),
        ] {
            let (code, message) = err("command.run", run(id, params.clone()));
            assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
            assert!(message.contains(word), "{id} {params}: {message}");
            assert!(!message.is_empty());
        }
    }
    // The checker says the same thing for the same fault on every command.
    let (_, a) = err("command.run", run("track.new_audio", json!({"seq": -1})));
    let (_, b) = err("command.run", run("track.new_midi", json!({"seq": -1})));
    assert_eq!(
        a.trim_start_matches("track.new_audio"),
        b.trim_start_matches("track.new_midi")
    );
    // A name is a string, and Rename Track is a dialog.
    let (code, _) = err("command.run", run("track.rename", json!({"name": 3})));
    assert_eq!(code, ErrorCode::InvalidParams);
    let (code, message) = err("command.run", run("track.rename", json!({})));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(message.contains("Rename Track"), "{message}");
    assert!(matches!(
        ok("command.run", run("track.rename", json!({"name": "Bass"}))),
        Command::CommandRun { .. }
    ));
    for id in &ids[..5] {
        assert!(
            matches!(
                ok("command.run", run(id, json!({}))),
                Command::CommandRun { .. }
            ),
            "{id}"
        );
    }
}

#[test]
fn input_fx_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for params in [
        json!({ "seq": 1, "lane": 2, "chain": "[]" }),
        json!({ "seq": 1, "lane": 2, "chain": "[{\"kind\": {\"transpose\": {\"semitones\": 12}}}]" }),
    ] {
        assert!(
            matches!(
                ok("command.run", run("lane.set_input_fx", params.clone())),
                Command::CommandRun { .. }
            ),
            "{params}"
        );
    }
    for params in [
        json!({}),
        json!({ "seq": 1, "lane": 2 }),
        json!({ "lane": 2, "chain": "[]" }),
        json!({ "seq": 1, "chain": "[]" }),
        json!({ "seq": 1, "lane": 2, "chain": [] }),
        json!({ "seq": 1, "lane": 2, "chain": 12 }),
        json!({ "seq": 1, "lane": 2, "chain": null }),
        json!({ "seq": -1, "lane": 2, "chain": "[]" }),
        json!({ "seq": 1, "lane": "Keys", "chain": "[]" }),
        json!({ "seq": 1, "lane": 2, "chain": "x".repeat(100_000) }),
        json!({ "seq": 1, "lane": 2, "chain": "[]", "junk": 1 }),
    ] {
        let (code, _) = err("command.run", run("lane.set_input_fx", params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{params}");
    }
}

#[test]
fn mpe_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "lower" }),
        ),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "upper", "members": 15, "bend_range": 96 }),
        ),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "off" }),
        ),
        (
            "destination.set_mpe",
            json!({ "dest": 4, "zone": "lower", "members": 1, "bend_range": 1 }),
        ),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    for (id, params) in [
        ("lane.set_mpe", json!({ "seq": 1, "lane": 2 })),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "middle" }),
        ),
        ("lane.set_mpe", json!({ "seq": 1, "lane": 2, "zone": 1 })),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "lower", "members": 0 }),
        ),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "lower", "members": 16 }),
        ),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "lower", "bend_range": 0 }),
        ),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "lower", "bend_range": 97 }),
        ),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "lower", "members": "7" }),
        ),
        ("lane.set_mpe", json!({ "lane": 2, "zone": "lower" })),
        (
            "lane.set_mpe",
            json!({ "seq": 1, "lane": 2, "zone": "lower", "junk": "x".repeat(100_000) }),
        ),
        ("destination.set_mpe", json!({ "zone": "lower" })),
        (
            "destination.set_mpe",
            json!({ "dest": -4, "zone": "lower" }),
        ),
        (
            "destination.set_mpe",
            json!({ "dest": "Synth", "zone": "lower" }),
        ),
        (
            "destination.set_mpe",
            json!({ "dest": 4, "zone": "lower", "members": 1.5 }),
        ),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
}
