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
        ("ui.screenshot", json!({"path": shot.to_str().unwrap()})),
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
    let (code, msg) = err("state.get", json!({"target": "mixer"}));
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
            ("drum.mark_lane", "Drum Lane", true),
            ("drum.add_row", "Add Drum Row", false),
            ("drum.show_velocity", "Show Velocity", true),
            ("drum.split_row", "Split Row to Own Lane", false),
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
        119,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four"
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
