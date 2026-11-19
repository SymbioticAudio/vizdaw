//! The Co-Producer gate (plan 8a Rulings 5, 6, 11 and pre-flight P1–P3):
//! command classes, the allow-list, plans, tokens, request names, the
//! never-replace path check and the filtered `commands.list`.

use core_control::co_producer::{
    self, MAX_PLAN, Output, PREVIEWABLE, Planned, admit, canonical, check_output, effective_class,
    output,
};
use core_control::protocol::{RawRequest, RequestId};
use core_control::registry::{self, ArgValue, COMMANDS, Class};
use core_control::{ControlError, ErrorCode, validate};
use serde_json::{Value, json};
use std::path::Path;

fn req(cmd: &str, params: Value) -> RawRequest {
    RawRequest {
        id: RequestId::Num(1),
        cmd: cmd.into(),
        params: params.as_object().cloned().unwrap_or_default(),
    }
}

/// Validates, then admits: the Co-Producer socket's order.
fn gate(cmd: &str, params: Value) -> Result<(), ControlError> {
    admit(&validate(&req(cmd, params))?)
}

fn refused(cmd: &str, params: Value) -> ControlError {
    match gate(cmd, params.clone()) {
        Ok(()) => panic!("{cmd} {params} was admitted"),
        Err(e) => e,
    }
}

fn assert_code(cmd: &str, params: Value, code: ErrorCode) -> String {
    let e = refused(cmd, params.clone());
    assert_eq!(e.code, code, "{cmd} {params}: {}", e.message);
    e.message
}

fn run(id: &str, params: Value) -> Value {
    json!({ "id": id, "params": params })
}

const READ: &[&str] = &[
    "transport.play_stop",
    "transport.play",
    "transport.stop",
    "transport.play_from_edit_in",
    "transport.play_from_selection",
    "transport.play_selection",
    "transport.rewind",
    "transport.fast_forward",
    "transport.return_to_start",
    "transport.go_edit_in",
    "transport.go_position",
    "transport.go_punch",
    "transport.next_marker",
    "transport.previous_marker",
    "transport.loop",
    "transport.scrub",
    "file.interchange_report",
];
const ADD: &[&str] = &[
    "track.new_audio",
    "track.new_instrument",
    "track.new_midi",
    "track.duplicate",
    "media.import",
    "file.import_midi",
    "mixer.new_bus",
    "marker.add",
    "track.new_plugin_instrument",
    "mixer.new_group",
    "mixer.new_vca",
];
const OUTWARD: &[&str] = &[
    "file.bounce",
    "file.export_midi",
    "file.export_midi_clip",
    "file.export_dawproject",
    "file.export_aaf",
    "file.export_for_daw",
];
const CHANGE: &[&str] = &[
    "track.rename",
    "mixer.set_colour",
    "track.choose_take",
    "track.rename_take",
    "edit.undo",
    "edit.redo",
    "file.save",
    "file.project_settings",
    "region.normalize",
    "region.fade_in_to_cursor",
    "region.fade_out_from_cursor",
    "region.crossfade",
    "region.set_fade_shape",
    "lane.set_input",
    "lane.set_monitor",
    "lane.set_output",
    "lane.set_input_fx",
    "lane.set_mpe",
    "lane.track_synth",
    "destination.set_midi_mix",
    "destination.set_mpe",
    "destination.set_return_latency",
    "destination.set_sound",
    "destination.add_midi",
    "automation.set_mode",
    "automation.gesture",
    "automation.suspend",
    "drum.add_row",
    "drum.split_row",
    "sequence.make_reference",
    "media.rename",
    "media.relink",
    "media.convert",
    "editors.insert_event",
    "editors.nudge_earlier",
    "editors.nudge_later",
    "editors.nudge_up",
    "editors.nudge_down",
    "marker.rename",
    "mixer.set_strip",
    "mixer.set_output",
    "mixer.add_send",
    "mixer.set_send",
    "mixer.add_insert",
    "mixer.move_insert",
    "mixer.set_bypass",
    "mixer.set_eq",
    "mixer.set_param",
    "automation.write",
    "mixer.rename_group",
    "mixer.set_group_colour",
    "mixer.add_to_group",
    "mixer.remove_from_group",
    "mixer.set_group_links",
    "mixer.rename_vca",
    "mixer.set_vca_colour",
    "mixer.assign_vca",
    "mixer.unassign_vca",
    "mixer.set_vca",
];
const REMOVE: &[&str] = &[
    "track.delete",
    "track.delete_take",
    "region.remove_fades",
    "region.remove_crossfade",
    "drum.remove_row",
    "marker.remove",
    "mixer.remove_send",
    "mixer.remove_insert",
    "mixer.remove_bus",
    "mixer.delete_group",
    "mixer.delete_vca",
];
/// Spec §6.5's refused commands (the layout `ui.*` included).
const SPEC_REFUSED: &[&str] = &[
    "media.compact",
    "region.reload_external",
    "keys.save",
    "keys.load",
    "keys.reset",
    "transport.record",
    "record.keep_pass",
    "record.discard_pass",
    "record.set",
    "record.recover",
    "file.close",
    "file.save_as",
    "plugins.install_multiq",
    "plugins.new_channel_eq",
    "audio.set",
    "ui.theme",
    "ui.view_arrange",
    "ui.view_mix",
    "ui.toggle_left",
    "ui.toggle_bottom",
    "ui.toggle_right",
    "ui.pane",
];

/// Ruling 5's class of `id`, by its lists and families.
fn listed_class(id: &str) -> Option<Class> {
    if READ.contains(&id) || id.starts_with("locator.go_") {
        Some(Class::Read)
    } else if ADD.contains(&id) {
        Some(Class::Add)
    } else if OUTWARD.contains(&id) {
        Some(Class::Outward)
    } else if CHANGE.contains(&id)
        || id.starts_with("modify.")
        || id.starts_with("automation.mode.")
        || id.starts_with("locator.set_")
        || id.starts_with("locator.clear_")
    {
        Some(Class::Change)
    } else if REMOVE.contains(&id) {
        Some(Class::Remove)
    } else {
        None
    }
}

#[test]
fn every_command_has_the_spec_class() {
    assert_eq!(
        COMMANDS.len(),
        231,
        "plan 8d adds thirteen; 8e adds automation.write; 8f adds the two time-record settings; groups-vcas-transport adds seven group commands and seven VCA commands"
    );
    for list in [READ, ADD, OUTWARD, CHANGE, REMOVE, SPEC_REFUSED] {
        for id in list {
            assert!(registry::find(id).is_some(), "{id} is not registered");
        }
    }
    for c in COMMANDS {
        let want = listed_class(c.id).unwrap_or(Class::Refused);
        assert_eq!(c.class, want, "{}", c.id);
    }
    for id in SPEC_REFUSED {
        assert_eq!(registry::find(id).unwrap().class, Class::Refused, "{id}");
    }
    // The families are whole: ten of each locator, eight Modify, six modes.
    let count = |p: &str| COMMANDS.iter().filter(|c| c.id.starts_with(p)).count();
    assert_eq!(count("locator.go_"), 10);
    assert_eq!(count("locator.set_"), 10);
    assert_eq!(count("locator.clear_"), 10);
    assert_eq!(count("automation.mode."), 6);
    assert!(count("modify.") >= 8);
    for (class, name) in [
        (Class::Read, "read"),
        (Class::Add, "add"),
        (Class::Change, "change"),
        (Class::Remove, "remove"),
        (Class::Outward, "outward"),
        (Class::Refused, "refused"),
    ] {
        assert_eq!(class.name(), name);
    }
    for id in [
        "settings.co_producer",
        "file.open_copy",
        "settings.co_producer_answer",
        "settings.co_producer_metrics",
        "settings.delete_time_records",
    ] {
        assert_eq!(registry::find(id).unwrap().class, Class::Refused, "{id}");
    }
}

#[test]
fn mixer_new_bus_with_a_strip_is_a_change() {
    let spec = registry::find("mixer.new_bus").unwrap();
    assert_eq!(effective_class(spec, &[]), Class::Add);
    assert_eq!(
        effective_class(spec, &[("strip".into(), ArgValue::Int(3))]),
        Class::Change
    );
}

/// A temp folder holding `a.mid` and `a.wav`, both real files.
fn import_files() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let mid = dir.path().join("a.mid");
    std::fs::write(&mid, b"MThd").unwrap();
    let wav = dir.path().join("a.wav");
    std::fs::write(&wav, b"RIFF").unwrap();
    let (mid, wav) = (mid.to_str().unwrap().into(), wav.to_str().unwrap().into());
    (dir, mid, wav)
}

#[test]
fn import_midi_is_refused_in_every_form() {
    // Release gate B2: Co-Producer writes no notes in 26.0.0, and an
    // import lands notes on the composer's track.
    let spec = registry::find("file.import_midi").unwrap();
    let path = ("path".to_string(), ArgValue::Text("/tmp/a.mid".into()));
    assert_eq!(
        effective_class(spec, std::slice::from_ref(&path)),
        Class::Refused
    );
    assert_eq!(effective_class(spec, &[]), Class::Refused);
    assert_eq!(
        effective_class(spec, &[path, ("new_project".into(), ArgValue::Flag(true))]),
        Class::Refused
    );
    let (_dir, mid, _) = import_files();
    for params in [
        json!({"path": mid}),
        json!({"path": mid, "seq": 1, "lane": 2, "tick": 0}),
        json!({"path": mid, "new_project": true}),
        json!({"path": mid, "new_project": false}),
    ] {
        let m = assert_code(
            "command.run",
            run("file.import_midi", params.clone()),
            ErrorCode::NotAllowed,
        );
        assert!(m.contains("cannot run file.import_midi"), "{params}: {m}");
        let e = preview(json!([{"id": "file.import_midi", "params": params}])).unwrap_err();
        assert!(
            e.message.contains("cannot run file.import_midi"),
            "{}",
            e.message
        );
    }
    // The developer channel's class is unchanged.
    assert_eq!(spec.class, Class::Add);
}

#[test]
fn media_import_onto_an_existing_track_is_refused() {
    let spec = registry::find("media.import").unwrap();
    assert_eq!(effective_class(spec, &[]), Class::Add);
    assert_eq!(
        effective_class(spec, &[("lane".into(), ArgValue::Int(0))]),
        Class::Refused
    );
    let (_dir, _, wav) = import_files();
    for params in [
        json!({"path": wav, "seq": 1, "lane": 2, "tick": 0}),
        json!({"path": wav, "lane": 0}),
        json!({"path": wav, "seq": 1, "lane": 1}),
    ] {
        let m = assert_code(
            "command.run",
            run("media.import", params.clone()),
            ErrorCode::NotAllowed,
        );
        assert!(m.contains("cannot run media.import"), "{params}: {m}");
    }
    // Without a `lane` the import goes on new tracks: still an add.
    for params in [
        json!({"path": wav}),
        json!({"path": wav, "seq": 1, "tick": 0}),
        json!({"path": wav, "lane": null}),
    ] {
        assert_eq!(
            gate("command.run", run("media.import", params.clone())),
            Ok(()),
            "{params}"
        );
    }
    // Hostile `lane`s: each is refused, none panics.
    for lane in [
        json!("2"),
        json!(1.5),
        json!(-1),
        json!(i64::MIN),
        json!(i64::MAX),
        json!(u64::MAX),
        json!(1e300),
        json!(true),
        json!([2]),
        json!({"lane": 2}),
    ] {
        let params = json!({"path": wav, "seq": 1, "lane": lane, "tick": 0});
        assert!(
            gate("command.run", run("media.import", params.clone())).is_err(),
            "{params}"
        );
    }
    assert_eq!(spec.class, Class::Add);
}

#[test]
fn track_duplicate_is_refused() {
    // A duplicate copies the composer's notes onto a new track.
    let spec = registry::find("track.duplicate").unwrap();
    assert_eq!(effective_class(spec, &[]), Class::Refused);
    for params in [json!({}), json!({"seq": 1, "lane": 2})] {
        let m = assert_code(
            "command.run",
            run("track.duplicate", params.clone()),
            ErrorCode::NotAllowed,
        );
        assert!(m.contains("cannot run track.duplicate"), "{params}: {m}");
    }
    let list = co_producer::list_json();
    for id in co_producer::REFUSED_IN_26 {
        assert!(
            list["commands"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["id"] != *id),
            "{id} is listed"
        );
    }
    assert_eq!(spec.class, Class::Add);
}

/// The adds Co-Producer may run, sorted. None of them creates notes:
/// the new tracks start empty, `media.import` (without `lane`) puts audio
/// on new tracks, `mixer.new_bus` (without `strip`) is a free bus,
/// `mixer.new_group` and `mixer.new_vca` (without `strips`) an empty group
/// or VCA, and `marker.add` is a marker. The adds that do create notes,
/// `file.import_midi` and `track.duplicate`, are refused (release gate
/// B2). A new add fails `no_add_command_writes_notes` until a reviewer
/// classifies it here or refuses it.
const NOTELESS_ADDS: &[&str] = &[
    "marker.add",
    "media.import",
    "mixer.new_bus",
    "mixer.new_group",
    "mixer.new_vca",
    "track.new_audio",
    "track.new_instrument",
    "track.new_midi",
    "track.new_plugin_instrument",
];

#[test]
fn no_add_command_writes_notes() {
    let mut adds: Vec<&str> = COMMANDS
        .iter()
        .filter(|c| effective_class(c, &[]) == Class::Add)
        .map(|c| c.id)
        .collect();
    adds.sort_unstable();
    assert_eq!(adds, NOTELESS_ADDS, "classify every new add here");
    let mut sorted = NOTELESS_ADDS.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, NOTELESS_ADDS, "keep the list sorted");
    // Every add of the registry is either one of these or refused.
    for c in COMMANDS.iter().filter(|c| c.class == Class::Add) {
        assert!(
            NOTELESS_ADDS.contains(&c.id) || co_producer::REFUSED_IN_26.contains(&c.id),
            "{}",
            c.id
        );
    }
    for id in ["file.import_midi", "track.duplicate"] {
        let spec = registry::find(id).unwrap();
        assert_eq!(effective_class(spec, &[]), Class::Refused, "{id}");
    }
}

#[test]
fn the_gate_admits_only_the_allow_list() {
    let dir = tempfile::tempdir().unwrap();
    let new_mid = dir.path().join("new.mid");
    let new_mid = new_mid.to_str().unwrap();
    let existing = dir.path().join("a.mid");
    std::fs::write(&existing, b"MThd").unwrap();
    let existing = existing.to_str().unwrap();
    for (cmd, params) in [
        ("ping", json!({})),
        ("commands.list", json!({})),
        ("state.get", json!({"target": "document"})),
        ("wait", json!({"condition": "stopped"})),
        ("command.run", json!({"id": "track.new_audio"})),
        (
            "command.run",
            run("file.export_midi", json!({"path": new_mid})),
        ),
        ("co_producer.begin", json!({"name": "x"})),
        ("co_producer.end", json!({})),
        ("co_producer.status", json!({"window": 3})),
        ("ui.screenshot", json!({})),
        ("ui.screenshot", json!({"scale": 1.0, "window": "key"})),
        ("command.run", run("transport.loop", json!({"on": true}))),
        (
            "command.run",
            run("mixer.new_bus", json!({"name": "Drums"})),
        ),
    ] {
        assert_eq!(gate(cmd, params.clone()), Ok(()), "{cmd} {params}");
    }
    for (cmd, params) in [
        ("ui.click", json!({"point": {"x": 1, "y": 2}})),
        (
            "ui.drag",
            json!({"from": {"x": 1, "y": 2}, "to": {"x": 3, "y": 4}}),
        ),
        ("ui.key", json!({"key": "space"})),
        ("ui.tree", json!({})),
        ("app.open", json!({"path": "/tmp/a.vizdaw"})),
        ("app.quit", json!({})),
        ("app.new_demo", json!({})),
        ("state.get", json!({"target": "input"})),
        ("command.run", json!({"id": "media.compact"})),
        ("command.run", json!({"id": "transport.record"})),
        (
            "command.run",
            run("keys.save", json!({"path": "/tmp/k.vizkeys"})),
        ),
        (
            "command.run",
            run("file.save_as", json!({"path": "/tmp/a.vizdaw"})),
        ),
        (
            "command.run",
            run("settings.co_producer", json!({"on": false})),
        ),
        (
            "command.run",
            run("file.open_copy", json!({"path": "/tmp/a.vizdaw"})),
        ),
        ("command.run", run("mixer.new_bus", json!({"strip": 3}))),
        ("command.run", json!({"id": "edit.undo"})),
        ("command.run", json!({"id": "file.save"})),
    ] {
        assert_code(cmd, params, ErrorCode::NotAllowed);
    }
    let m = assert_code(
        "command.run",
        run("track.delete", json!({"seq": 1, "lane": 1})),
        ErrorCode::NotAllowed,
    );
    assert!(
        m.contains("track.delete changes your work")
            && m.contains("co_producer.preview")
            && m.contains("composer's yes"),
        "{m}"
    );
    assert_code(
        "command.run",
        run("region.normalize", json!({"peak_db": "-1"})),
        ErrorCode::NotAllowed,
    );
    let m = assert_code(
        "command.run",
        run(
            "file.export_midi",
            json!({"path": new_mid, "replace": true}),
        ),
        ErrorCode::NotAllowed,
    );
    assert!(m.contains("Co-Producer never replaces a file."), "{m}");
    assert_code(
        "command.run",
        run(
            "file.import_midi",
            json!({"path": existing, "new_project": true}),
        ),
        ErrorCode::NotAllowed,
    );
    // An outward path that exists never reaches the app.
    let m = assert_code(
        "command.run",
        run("file.export_midi", json!({"path": existing})),
        ErrorCode::NotAllowed,
    );
    assert!(m.contains(existing), "{m}");
    for params in [
        json!({"path": "/tmp/shot.png"}),
        json!({"live": true}),
        json!({"scale": 1.5}),
        json!({"scale": 4.0}),
    ] {
        assert_code("ui.screenshot", params, ErrorCode::InvalidParams);
    }
}

fn preview(commands: Value) -> Result<(), ControlError> {
    gate("co_producer.preview", json!({ "commands": commands }))
}

#[test]
fn plans_check_every_command() {
    let rename = json!({"id": "track.rename", "params": {"seq": 1, "lane": 2, "name": "Vox"}});
    assert_eq!(preview(json!([rename])), Ok(()));
    match validate(&req(
        "co_producer.preview",
        json!({"commands": [rename], "window": 4}),
    ))
    .unwrap()
    {
        core_control::Command::CoProducerPreview { plan, window } => {
            assert_eq!(
                plan,
                vec![Planned {
                    id: "track.rename".into(),
                    args: vec![
                        ("seq".into(), ArgValue::Int(1)),
                        ("lane".into(), ArgValue::Int(2)),
                        ("name".into(), ArgValue::Text("Vox".into())),
                    ],
                }]
            );
            assert_eq!(window, Some(core_control::params::WindowRef::Number(4)));
        }
        other => panic!("{other:?}"),
    }
    // Ruling 6's five pass with explicit targets.
    assert_eq!(
        preview(json!([
            {"id": "track.delete", "params": {"seq": 0, "lane": 0}},
            {"id": "edit.undo"},
            {"id": "edit.redo", "params": null},
            {"id": "file.save", "params": {}},
            rename,
        ])),
        Ok(())
    );
    let many: Vec<Value> = (0..MAX_PLAN + 1)
        .map(|_| json!({"id": "edit.undo"}))
        .collect();
    let full: Vec<Value> = (0..MAX_PLAN).map(|_| json!({"id": "edit.undo"})).collect();
    assert_eq!(preview(json!(full)), Ok(()));
    let huge = "x".repeat(70_000);
    for (commands, code) in [
        (json!([]), ErrorCode::InvalidParams),
        (json!(many), ErrorCode::InvalidParams),
        (json!("edit.undo"), ErrorCode::InvalidParams),
        (json!({"id": "edit.undo"}), ErrorCode::InvalidParams),
        (json!(["edit.undo"]), ErrorCode::InvalidParams),
        (json!([{"params": {}}]), ErrorCode::InvalidParams),
        (json!([{"id": 5}]), ErrorCode::InvalidParams),
        (
            json!([{"id": "edit.undo", "extra": 1}]),
            ErrorCode::InvalidParams,
        ),
        (
            json!([{"id": "track.rename", "params": "seq=1"}]),
            ErrorCode::InvalidParams,
        ),
        (
            json!([{"id": "track.rename", "params": {"seq": 1, "lane": 2, "name": huge}}]),
            ErrorCode::InvalidParams,
        ),
        (
            json!([{"id": "track.rename", "params": {"seq": -1, "lane": 2, "name": "x"}}]),
            ErrorCode::InvalidParams,
        ),
        (
            json!([{"id": "edit.undo", "params": {"count": 2}}]),
            ErrorCode::InvalidParams,
        ),
        (json!([{"id": "file.print"}]), ErrorCode::UnknownId),
        (json!([{"id": huge}]), ErrorCode::InvalidParams),
        (json!([{"id": "track.new_audio"}]), ErrorCode::NotAllowed),
        (json!([{"id": "media.compact"}]), ErrorCode::NotAllowed),
        (
            json!([{"id": "file.export_midi", "params": {"path": "/tmp/n.mid"}}]),
            ErrorCode::NotAllowed,
        ),
    ] {
        let e = preview(commands.clone()).unwrap_err();
        assert_eq!(e.code, code, "{commands}: {}", e.message);
    }
    for params in [
        json!({"seq": 1, "name": "x"}),
        json!({"lane": 1, "name": "x"}),
        json!({"name": "x"}),
    ] {
        let e = preview(json!([{"id": "track.rename", "params": params}])).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{}", e.message);
        assert!(
            e.message.contains("name the track's `seq` and `lane`"),
            "{}",
            e.message
        );
    }
    let e = preview(json!([{"id": "track.delete", "params": {"seq": 1}}])).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams, "{}", e.message);
    let e = preview(json!([{"id": "region.normalize", "params": {"peak_db": "-1"}}])).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed);
    assert_eq!(
        e.message,
        "Co-Producer cannot preview region.normalize yet."
    );
    // Plan 8d Ruling 1: a new bus fed by a strip now previews.
    assert_eq!(
        preview(json!([{"id": "mixer.new_bus", "params": {"strip": 2}}])),
        Ok(())
    );
    let e =
        preview(json!([{"id": "file.save", "params": {"path": "/tmp/Other.vizdaw"}}])).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed);
    assert_eq!(
        e.message,
        "Co-Producer saves only over the song's own file."
    );
    // A bad command anywhere in the plan refuses the whole plan.
    let e = preview(
        json!([{"id": "edit.undo"}, {"id": "region.normalize", "params": {"peak_db": "-1"}}]),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed);
    // Apply checks its plan the same way.
    let token = "0123456789abcdef0123456789abcdef";
    assert_eq!(
        gate(
            "co_producer.apply",
            json!({"token": token, "commands": [{"id": "edit.undo"}]})
        ),
        Ok(())
    );
    let e = gate(
        "co_producer.apply",
        json!({"token": token, "commands": [{"id": "region.normalize", "params": {"peak_db": "-1"}}]}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed);
    for (cmd, params) in [
        ("co_producer.preview", json!({})),
        (
            "co_producer.preview",
            json!({"commands": [{"id": "edit.undo"}], "extra": 1}),
        ),
        (
            "co_producer.preview",
            json!({"commands": [{"id": "edit.undo"}], "window": 0}),
        ),
        (
            "co_producer.preview",
            json!({"commands": [{"id": "edit.undo"}], "window": "front"}),
        ),
        ("co_producer.apply", json!({"token": token})),
        (
            "co_producer.apply",
            json!({"commands": [{"id": "edit.undo"}]}),
        ),
    ] {
        assert_code(cmd, params, ErrorCode::InvalidParams);
    }
}

#[test]
fn apply_checks_its_token() {
    let plan = json!([{"id": "edit.undo"}]);
    let good = "0123456789abcdef0123456789abcdef";
    assert_eq!(good.len(), 32);
    for token in [
        json!(&good[..31]),
        json!(format!("{good}0")),
        json!(good.to_uppercase()),
        json!(format!("{}\0", &good[..31])),
        json!(format!("{}g", &good[..31])),
        json!(""),
        json!("x".repeat(70_000)),
        json!(12345),
        json!(true),
        json!([good]),
    ] {
        let m = assert_code(
            "co_producer.apply",
            json!({"token": token, "commands": plan}),
            ErrorCode::InvalidParams,
        );
        assert!(m.starts_with("co_producer.apply:"), "{m}");
    }
}

#[test]
fn begin_checks_its_name() {
    for name in ["x", "set up a drum bus", &"é".repeat(200), "  padded  "] {
        assert_eq!(
            gate("co_producer.begin", json!({"name": name})),
            Ok(()),
            "{name}"
        );
    }
    match validate(&req("co_producer.begin", json!({"name": "  Drum bus "}))).unwrap() {
        core_control::Command::CoProducerBegin {
            name,
            asked,
            window,
        } => {
            assert_eq!(name, "Drum bus");
            assert_eq!(asked, None);
            assert_eq!(window, None);
        }
        other => panic!("{other:?}"),
    }
    for name in [
        json!(""),
        json!("   "),
        json!("x".repeat(201)),
        json!("x".repeat(70_000)),
        json!("a\0b"),
        json!("a\nb"),
        json!("a\u{202e}b"),
        json!("a\u{2028}b"),
        json!("a\u{200f}b"),
        json!("a\u{85}b"),
        json!(5),
        json!(null),
        json!(["x"]),
    ] {
        assert_code(
            "co_producer.begin",
            json!({"name": name}),
            ErrorCode::InvalidParams,
        );
    }
    for (cmd, params) in [
        ("co_producer.begin", json!({})),
        ("co_producer.begin", json!({"name": "x", "extra": 1})),
        ("co_producer.begin", json!({"name": "x", "window": -1})),
        ("co_producer.end", json!({"name": "x"})),
        ("co_producer.end", json!({"window": 0})),
        ("co_producer.status", json!({"window": true})),
        ("co_producer.status", json!({"extra": "x"})),
    ] {
        assert_code(cmd, params, ErrorCode::InvalidParams);
    }
}

fn assert_refused(path: &Path, rule: Output) {
    let e = check_output(path, rule).unwrap_err();
    assert_eq!(
        e.code,
        ErrorCode::NotAllowed,
        "{}: {}",
        path.display(),
        e.message
    );
    assert!(
        e.message.contains(&*path.to_string_lossy()),
        "{} is not named in {:?}",
        path.display(),
        e.message
    );
}

#[test]
fn outward_paths_never_replace() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let mid = Output::NewFile(&["mid"]);
    let existing = dir.join("old.mid");
    std::fs::write(&existing, b"MThd").unwrap();
    let link = dir.join("link.mid");
    symlink(&existing, &link).unwrap();
    let dangling = dir.join("dangling.mid");
    symlink(dir.join("nowhere.mid"), &dangling).unwrap();
    let folder = dir.join("folder.mid");
    std::fs::create_dir(&folder).unwrap();
    std::fs::create_dir(dir.join("a")).unwrap();
    let package = dir.join("Song.vizdaw");
    std::fs::create_dir_all(package.join("Media")).unwrap();
    let upper = dir.join("Other.VIZDAW");
    std::fs::create_dir(&upper).unwrap();
    let into_package = dir.join("x");
    symlink(&package, &into_package).unwrap();
    let dir_link = dir.join("dirlink");
    symlink(dir, &dir_link).unwrap();
    for p in [
        existing.clone(),
        link,
        dangling,
        folder,
        dir.join("a").join("..").join("b.mid"),
        Path::new("b.mid").to_path_buf(),
        package.join("out.mid"),
        upper.join("out.mid"),
        package.join("Media").join("new.mid"),
        into_package.join("Media").join("new.mid"),
        dir_link.join("new.mid"),
        dir.join("missing").join("new.mid"),
        dir.join("new.wav"),
        dir.join(".mid"),
        dir.join(".hidden.mid"),
        existing.join("new.mid"),
        Path::new("/").to_path_buf(),
    ] {
        assert_refused(&p, mid);
    }
    assert_eq!(check_output(&dir.join("new.mid"), mid), Ok(()));
    assert_eq!(check_output(&dir.join("a").join("new.mid"), mid), Ok(()));
    let audio = Output::NewFile(&["wav", "aif", "aiff", "caf"]);
    assert_eq!(check_output(&dir.join("mix.aiff"), audio), Ok(()));
    assert_refused(&dir.join("mix.mid"), audio);
    // Into a folder: an existing, real folder outside any package.
    assert_eq!(check_output(dir, Output::IntoFolder), Ok(()));
    assert_eq!(check_output(&dir.join("a"), Output::IntoFolder), Ok(()));
    for p in [
        existing.clone(),
        dir_link.clone(),
        dir.join("missing"),
        package.clone(),
        package.join("Media"),
        into_package.join("Media"),
        upper.clone(),
        dir.join("a").join(".."),
        Path::new("a").to_path_buf(),
    ] {
        assert_refused(&p, Output::IntoFolder);
    }
    // Which rule each outward command's `path` has.
    let text = |k: &str, v: &str| (k.to_string(), ArgValue::Text(v.into()));
    assert_eq!(
        output("file.bounce", &[text("source", "stems")]),
        Some(Output::IntoFolder)
    );
    assert_eq!(output("file.export_for_daw", &[]), Some(Output::IntoFolder));
    assert_eq!(output("file.export_aaf", &[]), Some(Output::IntoFolder));
    assert!(matches!(
        output("file.bounce", &[]),
        Some(Output::NewFile(_))
    ));
    assert!(matches!(
        output("file.bounce", &[text("source", "selection")]),
        Some(Output::NewFile(_))
    ));
    assert_eq!(
        output("file.bounce", &[text("format", "caf")]),
        Some(Output::NewFile(&["caf"]))
    );
    assert_eq!(
        output("file.export_midi", &[]),
        Some(Output::NewFile(&["mid"]))
    );
    assert_eq!(
        output("file.export_dawproject", &[]),
        Some(Output::NewFile(&["dawproject"]))
    );
    // Plan 8c: every track (no `lanes`) goes into a folder; see
    // `midi_clips_for_several_tracks_go_into_a_folder`.
    assert_eq!(
        output("file.export_midi_clip", &[]),
        Some(Output::IntoFolder)
    );
    for id in [
        "track.new_audio",
        "file.import_midi",
        "media.import",
        "file.save",
    ] {
        assert_eq!(output(id, &[]), None, "{id}");
    }
    // Review I1: a path not in normal form is refused before any link or
    // "already there" check, since a final `/` or `/.` makes the system
    // follow a link (a dangling one into a package, or a folder link).
    let pkg_target = dir.join("pkg_dangle.mid");
    symlink(package.join("target.mid"), &pkg_target).unwrap();
    let with = |p: &Path, tail: &str| std::path::PathBuf::from(format!("{}{tail}", p.display()));
    for (p, rule) in [
        (with(&pkg_target, "/"), mid),
        (with(&pkg_target, "/."), mid),
        (with(&dir.join("new.mid"), "/"), mid),
        (with(&dir_link, "/"), Output::IntoFolder),
        (with(&dir_link, "/."), Output::IntoFolder),
        (with(dir, "/"), Output::IntoFolder),
        (with(dir, "/./a"), Output::IntoFolder),
        (with(dir, "//a"), Output::IntoFolder),
        (with(dir, "/a/../a"), Output::IntoFolder),
        (with(dir, "/a/../new.mid"), mid),
        (Path::new("/.").to_path_buf(), Output::IntoFolder),
        (Path::new("/.").to_path_buf(), mid),
    ] {
        let e = check_output(&p, rule).unwrap_err();
        assert_eq!(
            e.code,
            ErrorCode::NotAllowed,
            "{}: {}",
            p.display(),
            e.message
        );
        assert!(e.message.contains(&*p.to_string_lossy()), "{}", e.message);
    }
    let e = check_output(&with(&pkg_target, "/"), mid).unwrap_err();
    assert!(e.message.contains("normal form"), "{}", e.message);
    let e = gate(
        "command.run",
        run(
            "file.export_midi",
            json!({"path": format!("{}/", pkg_target.display())}),
        ),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed, "{}", e.message);
    let e = gate(
        "command.run",
        run(
            "file.export_for_daw",
            json!({"path": format!("{}/.", dir_link.display())}),
        ),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed, "{}", e.message);
    assert!(
        std::fs::symlink_metadata(package.join("target.mid")).is_err(),
        "nothing was made in the package"
    );
    // A link as the folder is refused as a link, whichever rule.
    for (p, rule) in [
        (dir_link.join("new.mid"), mid),
        (dir_link.clone(), Output::IntoFolder),
    ] {
        let e = check_output(&p, rule).unwrap_err();
        assert!(e.message.contains("is a link"), "{}", e.message);
    }
    // Through the gate: the stems folder must exist, a mix file must not.
    let d = dir.to_str().unwrap();
    assert_eq!(
        gate(
            "command.run",
            run(
                "file.bounce",
                json!({"source": "stems", "strips": "1", "path": d})
            )
        ),
        Ok(())
    );
    assert_code(
        "command.run",
        run("file.bounce", json!({"path": d})),
        ErrorCode::NotAllowed,
    );
    assert_code(
        "command.run",
        run(
            "file.export_aaf",
            json!({"path": package.to_str().unwrap()}),
        ),
        ErrorCode::NotAllowed,
    );
    assert_code(
        "command.run",
        run(
            "file.export_dawproject",
            json!({"path": package.join("x.dawproject").to_str().unwrap()}),
        ),
        ErrorCode::NotAllowed,
    );
}

#[test]
fn the_filtered_list_has_classes() {
    let list = co_producer::list_json();
    let commands = list["commands"].as_array().unwrap();
    let want = COMMANDS
        .iter()
        .filter(|c| c.class != Class::Refused && !co_producer::REFUSED_IN_26.contains(&c.id))
        .count();
    assert_eq!(commands.len(), want);
    let mut previewable = Vec::new();
    for c in commands {
        let id = c["id"].as_str().unwrap();
        let spec = registry::find(id).unwrap();
        assert_ne!(spec.class, Class::Refused, "{id}");
        assert_eq!(c["class"], spec.class.name(), "{id}");
        assert!(c["params"].is_array() && c["title"].is_string(), "{c}");
        if c["previewable"] == json!(true) {
            previewable.push(id);
        } else {
            assert_eq!(c["previewable"], json!(false), "{id}");
        }
    }
    previewable.sort_unstable();
    let mut six = PREVIEWABLE.to_vec();
    six.sort_unstable();
    assert_eq!(previewable, six);
    assert!(commands.iter().all(|c| c["id"] != "ui.theme"));
    // The developer list carries every command's class too.
    let all = registry::list_json();
    for c in all["commands"].as_array().unwrap() {
        let spec = registry::find(c["id"].as_str().unwrap()).unwrap();
        assert_eq!(c["class"], spec.class.name());
    }
}

#[test]
fn canonical_text_is_order_and_value_exact() {
    let rename = |name: &str| Planned {
        id: "track.rename".into(),
        args: vec![
            ("seq".into(), ArgValue::Int(1)),
            ("lane".into(), ArgValue::Int(2)),
            ("name".into(), ArgValue::Text(name.into())),
        ],
    };
    let undo = Planned {
        id: "edit.undo".into(),
        args: vec![],
    };
    let a = [rename("Vox"), undo.clone()];
    let b = [undo.clone(), rename("Vox")];
    let c = [rename("Vox2"), undo.clone()];
    assert_eq!(canonical(&a), canonical(&a.clone()));
    assert_ne!(canonical(&a), canonical(&b));
    assert_ne!(canonical(&a), canonical(&c));
    let flag = Planned {
        id: "x".into(),
        args: vec![("on".into(), ArgValue::Flag(true))],
    };
    let text = Planned {
        id: "x".into(),
        args: vec![("on".into(), ArgValue::Text("true".into()))],
    };
    assert_ne!(canonical(&[flag]), canonical(&[text]));
    // A name cannot forge a second command.
    let forged = [rename("Vox\"},{\"id\":\"edit.undo")];
    assert_ne!(canonical(&forged), canonical(&a));
    let v: Value = serde_json::from_str(&canonical(&a)).unwrap();
    assert_eq!(v[0]["id"], "track.rename");
}

#[test]
fn new_registry_entries_have_their_class() {
    for id in ["track.make_mine", "mixer.make_mine", "window.song_notes"] {
        assert_eq!(registry::find(id).unwrap().class, Class::Refused, "{id}");
    }
    assert_eq!(
        registry::find("mixer.set_colour").unwrap().class,
        Class::Change
    );
    let list = co_producer::list_json();
    let commands = list["commands"].as_array().unwrap();
    for id in ["track.make_mine", "mixer.make_mine", "window.song_notes"] {
        assert!(commands.iter().all(|c| c["id"] != id), "{id} is listed");
    }
    let colour = commands
        .iter()
        .find(|c| c["id"] == "mixer.set_colour")
        .expect("mixer.set_colour is listed");
    assert_eq!(colour["previewable"], json!(true));
    assert_eq!(colour["class"], "change");
}

#[test]
fn set_colour_only_through_a_preview() {
    let m = assert_code(
        "command.run",
        run("mixer.set_colour", json!({"strip": 2, "colour": "blue"})),
        ErrorCode::NotAllowed,
    );
    assert_eq!(
        m,
        "mixer.set_colour changes your work: preview it with co_producer.preview and apply it with the composer's yes."
    );
    assert_eq!(
        preview(json!([
            {"id": "mixer.set_colour", "params": {"strip": 2, "colour": "blue"}}
        ])),
        Ok(())
    );
    for colour in [
        "none", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
    ] {
        assert_eq!(
            preview(json!([
                {"id": "mixer.set_colour", "params": {"strip": 1, "colour": colour}}
            ])),
            Ok(()),
            "{colour}"
        );
    }
    for params in [
        json!({"strip": 2, "colour": "mauve"}),
        json!({"strip": 2}),
        json!({"colour": "blue"}),
        json!({"strip": 0, "colour": "blue"}),
        json!({"strip": "2", "colour": "blue"}),
        json!({"strip": 2, "colour": 3}),
        json!({"strip": 2, "colour": "blue", "extra": 1}),
    ] {
        assert_code(
            "command.run",
            run("mixer.set_colour", params.clone()),
            ErrorCode::InvalidParams,
        );
        let e = preview(json!([{"id": "mixer.set_colour", "params": params}])).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{}", e.message);
    }
}

fn export(params: Value) -> Result<(), ControlError> {
    gate("co_producer.export_folder", params)
}

#[test]
fn export_folder_parent_is_checked() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let plain = dir.join("Exports");
    std::fs::create_dir(&plain).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    let link = dir.join("link");
    symlink(&plain, &link).unwrap();
    let package = dir.join("X.vizdaw");
    std::fs::create_dir_all(package.join("Media")).unwrap();
    let upper = dir.join("x.VIZDAW");
    std::fs::create_dir_all(upper.join("sub")).unwrap();
    let linked_parent = dir.join("up");
    symlink(&upper, &linked_parent).unwrap();
    let s = |p: &Path| p.to_str().unwrap().to_string();
    for (parent, reason) in [
        ("Exports".to_string(), "absolute"),
        (s(&dir.join("Exports").join("..").join("Exports")), "`..`"),
        (s(&file), "not a folder"),
        (s(&link), "is a link"),
        (s(&package.join("Media")), "inside a project package"),
        (s(&package), "inside a project package"),
        (s(&linked_parent.join("sub")), "inside a project package"),
    ] {
        let e = export(json!({"what": "stems", "parent": parent})).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotAllowed, "{parent}: {}", e.message);
        assert!(e.message.contains(&parent), "{parent}: {}", e.message);
        assert!(e.message.contains(reason), "{parent}: {}", e.message);
    }
    assert_eq!(
        export(json!({"what": "stems", "parent": s(&plain)})),
        Ok(())
    );
    assert_eq!(export(json!({"what": "mix"})), Ok(()));
    assert_eq!(export(json!({"what": "other_daw", "window": 2})), Ok(()));
    match validate(&req(
        "co_producer.export_folder",
        json!({"what": "midi", "parent": s(&plain), "window": "key"}),
    ))
    .unwrap()
    {
        core_control::Command::CoProducerExportFolder {
            what,
            parent,
            window,
        } => {
            assert_eq!(what, core_control::request::ExportWhat::Midi);
            assert_eq!(parent.as_deref(), Some(plain.as_path()));
            assert_eq!(window, Some(core_control::params::WindowRef::Key));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn export_what_parses_and_labels() {
    use core_control::request::ExportWhat;
    for (s, what, label) in [
        ("stems", ExportWhat::Stems, "Stems"),
        ("mix", ExportWhat::Mix, "Mix"),
        ("selection", ExportWhat::Selection, "Selection"),
        ("midi", ExportWhat::Midi, "MIDI"),
        ("dawproject", ExportWhat::Dawproject, "DAWproject"),
        ("aaf", ExportWhat::Aaf, "AAF"),
        ("other_daw", ExportWhat::OtherDaw, "Other DAW"),
    ] {
        assert_eq!(ExportWhat::parse(s), Some(what), "{s}");
        assert_eq!(what.label(), label);
    }
    for s in ["", "Stems", "video", "other daw", "mix "] {
        assert_eq!(ExportWhat::parse(s), None, "{s:?}");
    }
}

#[test]
fn new_calls_refuse_hostile_input() {
    use core_control::request::{MAX_ASKED, MAX_NOTE, MAX_NOTE_LINES};
    assert_eq!((MAX_ASKED, MAX_NOTE, MAX_NOTE_LINES), (500, 2000, 20));
    let lines = |n: usize| vec!["line"; n].join("\n");
    for text in [
        json!("x"),
        json!("x".repeat(2000)),
        json!(lines(20)),
        json!("the composer wants the drums dry in the verse"),
    ] {
        assert_eq!(gate("co_producer.note", json!({"text": text})), Ok(()));
    }
    match validate(&req(
        "co_producer.note",
        json!({"text": "a\nb", "window": 4}),
    ))
    .unwrap()
    {
        core_control::Command::CoProducerNote { text, window } => {
            assert_eq!(text, "a\nb");
            assert_eq!(window, Some(core_control::params::WindowRef::Number(4)));
        }
        other => panic!("{other:?}"),
    }
    for params in [
        json!({}),
        json!({"text": ""}),
        json!({"text": "   "}),
        json!({"text": "x".repeat(2001)}),
        json!({"text": "x".repeat(70_000)}),
        json!({"text": lines(21)}),
        json!({"text": "a\0b"}),
        json!({"text": "a\rb"}),
        json!({"text": "a\tb"}),
        json!({"text": "a\u{85}b"}),
        json!({"text": "a\u{202E}b"}),
        json!({"text": "a\u{2028}b"}),
        json!({"text": "a\u{2029}b"}),
        json!({"text": "a\u{200F}b"}),
        json!({"text": 5}),
        json!({"text": null}),
        json!({"text": ["x"]}),
        json!({"text": "x", "extra": 1}),
        json!({"text": "x", "window": 0}),
    ] {
        assert_code("co_producer.note", params, ErrorCode::InvalidParams);
    }
    // `asked` on begin.
    for params in [
        json!({"name": "x", "asked": "x".repeat(501)}),
        json!({"name": "x", "asked": "x".repeat(70_000)}),
        json!({"name": "x", "asked": "a\0b"}),
        json!({"name": "x", "asked": "a\rb"}),
        json!({"name": "x", "asked": "a\u{202E}b"}),
        json!({"name": "x", "asked": "a\u{2028}b"}),
        json!({"name": "x", "asked": 5}),
        json!({"name": "x", "asked": ["x"]}),
    ] {
        assert_code("co_producer.begin", params, ErrorCode::InvalidParams);
    }
    let asked = |a: Value| match validate(&req(
        "co_producer.begin",
        json!({"name": "Drum bus", "asked": a}),
    ))
    .unwrap()
    {
        core_control::Command::CoProducerBegin { name, asked, .. } => {
            assert_eq!(name, "Drum bus");
            asked
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(asked(json!("two\nlines")), Some("two lines".into()));
    assert_eq!(asked(json!("  padded \n")), Some("padded".into()));
    assert_eq!(asked(json!("x".repeat(500))), Some("x".repeat(500)));
    assert_eq!(asked(json!(null)), None);
    assert_eq!(asked(json!("   ")), None);
    assert_eq!(
        gate(
            "co_producer.begin",
            json!({"name": "x", "asked": "put my drums on a bus"})
        ),
        Ok(())
    );
    // `export_folder`.
    for params in [
        json!({"what": "video"}),
        json!({}),
        json!({"parent": "/tmp"}),
        json!({"what": 3}),
        json!({"what": "Stems"}),
        json!({"what": "stems", "parent": "/".repeat(70_000)}),
        json!({"what": "stems", "parent": format!("/{}", "x".repeat(70_000))}),
        json!({"what": "stems", "parent": "/tmp/a\0b"}),
        json!({"what": "stems", "parent": 5}),
        json!({"what": "stems", "extra": true}),
        json!({"what": "stems", "window": -2}),
    ] {
        assert_code(
            "co_producer.export_folder",
            params,
            ErrorCode::InvalidParams,
        );
    }
    // `notes`.
    assert_eq!(gate("co_producer.notes", json!({})), Ok(()));
    assert_eq!(gate("co_producer.notes", json!({"window": "main"})), Ok(()));
    for params in [
        json!({"extra": 1}),
        json!({"window": true}),
        json!({"text": "x"}),
    ] {
        assert_code("co_producer.notes", params, ErrorCode::InvalidParams);
    }
}

#[test]
fn new_calls_only_on_the_co_producer_socket() {
    use core_control::client::{CallError, Client};
    use core_control::server::{Config, Server};
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::Builder::new()
        .prefix("vzc")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let sock = d.path().join("ctl").join("s.sock");
    let dev = Server::start(&sock, Config::default()).unwrap();
    let mut c = Client::connect(&sock).unwrap();
    let long = std::time::Duration::from_secs(20);
    for (cmd, params) in [
        ("co_producer.notes", json!({})),
        ("co_producer.note", json!({"text": "x"})),
        ("co_producer.export_folder", json!({"what": "stems"})),
    ] {
        match c.call(cmd, params, long) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd}: {e}");
                assert!(e.message.contains("only on the Co-Producer socket"), "{e}");
            }
            other => panic!("{cmd}: {other:?}"),
        }
    }
    assert!(dev.next_call().is_none(), "the app never sees them");
}

/// Plan 8b Task 6 (Ruling 11): a fake peer hook for the server.
fn fake_peer(_fd: std::os::fd::RawFd) -> Option<[u32; 8]> {
    Some([1, 2, 3, 4, 5, 6, 7, 8])
}

/// A private socket folder under `/tmp`, as `server.rs`'s tests make.
fn private_tmp() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::Builder::new()
        .prefix("vzc")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

/// Sends one `co_producer.status` on `stream` and answers it from
/// `server`; returns the call the app saw.
fn one_call(
    server: &core_control::server::Server,
    stream: &mut std::os::unix::net::UnixStream,
) -> core_control::server::Call {
    use std::io::{BufRead, BufReader, Write};
    let line =
        core_control::protocol::request_line(&RequestId::Num(1), "co_producer.status", &json!({}));
    stream.write_all(format!("{line}\n").as_bytes()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let call = loop {
        if let Some(c) = server.next_call() {
            break c;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the call never arrived"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert!(server.reply(call.token, Ok(json!({"request": null}))));
    let mut reply = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut reply)
        .unwrap();
    assert!(reply.contains("\"ok\":true"), "{reply}");
    call
}

#[test]
fn the_peer_reaches_every_call() {
    use core_control::server::{Config, Role, Server};
    use std::os::unix::net::UnixStream;
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let server = Server::start(
        &sock,
        Config {
            role: Role::CoProducer,
            peer: Some(fake_peer),
            ..Config::default()
        },
    )
    .unwrap();
    let mut first = UnixStream::connect(&sock).unwrap();
    let a = one_call(&server, &mut first);
    let b = one_call(&server, &mut first);
    assert_eq!(a.audit_token, Some([1, 2, 3, 4, 5, 6, 7, 8]));
    assert_eq!(b.audit_token, a.audit_token);
    assert_eq!(a.conn, b.conn, "one connection, one number");
    let mut second = UnixStream::connect(&sock).unwrap();
    let c = one_call(&server, &mut second);
    assert_ne!(c.conn, a.conn, "a new connection, a new number");
    assert_eq!(c.audit_token, a.audit_token);
    drop(server);

    // No hook: no token.
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let server = Server::start(
        &sock,
        Config {
            role: Role::CoProducer,
            ..Config::default()
        },
    )
    .unwrap();
    assert!(Config::default().peer.is_none());
    let mut s = UnixStream::connect(&sock).unwrap();
    assert_eq!(one_call(&server, &mut s).audit_token, None);
}

#[test]
fn apply_takes_a_client_label() {
    use core_control::request::{Command, MAX_CLIENT_LABEL};
    let token = "0123456789abcdef0123456789abcdef";
    let plan = json!([{"id": "edit.undo"}]);
    let parsed = |client: Value| {
        validate(&req(
            "co_producer.apply",
            json!({"token": token, "commands": plan, "client": client}),
        ))
    };
    match parsed(json!("  Helper Chat ")).unwrap() {
        Command::CoProducerApply { client, .. } => {
            assert_eq!(client.as_deref(), Some("Helper Chat"))
        }
        other => panic!("{other:?}"),
    }
    match validate(&req(
        "co_producer.apply",
        json!({"token": token, "commands": plan}),
    ))
    .unwrap()
    {
        Command::CoProducerApply { client, .. } => assert_eq!(client, None),
        other => panic!("{other:?}"),
    }
    assert_eq!(MAX_CLIENT_LABEL, 100);
    assert!(parsed(json!("a".repeat(100))).is_ok());
    for bad in [
        json!("a".repeat(101)),
        json!("Helper\0Chat"),
        json!("Helper \u{202E}Chat"),
        json!("Helper\nChat"),
        json!("x".repeat(70_000)),
        json!(12),
        json!(true),
        json!(["Helper Chat"]),
    ] {
        let e = parsed(bad.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{bad}: {}", e.message);
    }
    // The gate admits it as before.
    assert_eq!(
        gate(
            "co_producer.apply",
            json!({"token": token, "commands": plan, "client": "Helper Chat"})
        ),
        Ok(())
    );
}

#[test]
fn the_answer_hook_is_never_co_producers() {
    use core_control::client::{CallError, Client};
    use core_control::server::{Config, Role, Server};
    let spec = registry::find("settings.co_producer_answer").expect("the test hook is registered");
    assert_eq!(spec.class, Class::Refused);
    assert!(!spec.menu_item);
    let names: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
    assert_eq!(names, ["allow", "always", "confirm"]);
    assert!(spec.params[0].required && !spec.params[1].required && !spec.params[2].required);
    let list = co_producer::list_json();
    assert!(
        list["commands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["id"] != "settings.co_producer_answer")
    );
    assert_code(
        "command.run",
        run("settings.co_producer_answer", json!({"allow": true})),
        ErrorCode::NotAllowed,
    );
    // Through the Co-Producer socket itself.
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let server = Server::start(
        &sock,
        Config {
            role: Role::CoProducer,
            ..Config::default()
        },
    )
    .unwrap();
    let mut c = Client::connect(&sock).unwrap();
    match c.call(
        "command.run",
        run(
            "settings.co_producer_answer",
            json!({"allow": true, "always": true}),
        ),
        std::time::Duration::from_secs(20),
    ) {
        Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::NotAllowed, "{e}"),
        other => panic!("{other:?}"),
    }
    assert!(server.next_call().is_none(), "the app never sees it");
}

/// Plan 8c Rulings 2 and 4: the six chore commands.
const NOTE_CHORES: &[&str] = &[
    "modify.quantize_notes",
    "modify.transpose_notes",
    "modify.velocity_notes",
];

#[test]
fn chore_commands_have_their_class() {
    for id in NOTE_CHORES.iter().chain(&["marker.rename"]) {
        assert_eq!(registry::find(id).unwrap().class, Class::Change, "{id}");
    }
    assert_eq!(
        registry::find("marker.remove").unwrap().class,
        Class::Remove
    );
    assert_eq!(registry::find("marker.add").unwrap().class, Class::Add);
    for (id, menu, title) in [
        ("modify.quantize_notes", "Modify", "Quantize Notes"),
        ("modify.transpose_notes", "Modify", "Transpose Notes"),
        ("modify.velocity_notes", "Modify", "Change Velocity"),
        ("marker.add", "Transport", "Add Marker"),
        ("marker.rename", "Transport", "Rename Marker"),
        ("marker.remove", "Transport", "Remove Marker"),
    ] {
        let c = registry::find(id).unwrap();
        assert_eq!((c.menu, c.title), (menu, title), "{id}");
        assert!(!c.menu_item, "{id} is key-only");
        assert!(c.document, "{id} acts on a document");
        assert_eq!(c.shortcut, None, "{id}");
    }
    let list = co_producer::list_json();
    let commands = list["commands"].as_array().unwrap();
    let listed = |id: &str| {
        commands
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not listed"))
            .clone()
    };
    for id in NOTE_CHORES
        .iter()
        .chain(&["marker.rename", "marker.remove"])
    {
        assert!(PREVIEWABLE.contains(id), "{id}");
        assert_eq!(listed(id)["previewable"], json!(true), "{id}");
    }
    assert!(!PREVIEWABLE.contains(&"marker.add"));
    assert_eq!(listed("marker.add")["previewable"], json!(false));
    assert_eq!(listed("marker.add")["class"], "add");
    assert_eq!(
        PREVIEWABLE,
        &[
            "track.rename",
            "track.delete",
            "edit.undo",
            "edit.redo",
            "file.save",
            "mixer.set_colour",
            "modify.quantize_notes",
            "modify.transpose_notes",
            "modify.velocity_notes",
            "marker.rename",
            "marker.remove",
            "mixer.new_bus",
            "mixer.set_strip",
            "mixer.set_output",
            "mixer.add_send",
            "mixer.set_send",
            "mixer.remove_send",
            "mixer.add_insert",
            "mixer.remove_insert",
            "mixer.move_insert",
            "mixer.set_bypass",
            "mixer.set_eq",
            "mixer.set_param",
            "mixer.remove_bus",
            "automation.write",
            "region.set_fade_shape",
            "destination.set_sound",
            "lane.set_output",
        ]
    );
}

#[test]
fn chores_only_through_a_preview() {
    let direct = |id: &str| {
        format!(
            "{id} changes your work: preview it with co_producer.preview and apply it with the composer's yes."
        )
    };
    for (id, params) in [
        (
            "modify.transpose_notes",
            json!({"seq": 1, "lanes": "1", "semitones": 2}),
        ),
        (
            "modify.quantize_notes",
            json!({"seq": 1, "lanes": "1,2", "grid": "1/16", "strength": 50}),
        ),
        (
            "modify.velocity_notes",
            json!({"seq": 1, "lanes": "3", "op": "add", "amount": -10, "start": 0, "end": 15360}),
        ),
        (
            "marker.rename",
            json!({"seq": 1, "index": 1, "name": "Chorus"}),
        ),
        ("marker.remove", json!({"seq": 1, "index": 2})),
    ] {
        let m = assert_code(
            "command.run",
            run(id, params.clone()),
            ErrorCode::NotAllowed,
        );
        assert_eq!(m, direct(id));
        assert_eq!(
            preview(json!([{"id": id, "params": params}])),
            Ok(()),
            "{id}"
        );
    }
    assert_eq!(
        gate(
            "command.run",
            run("marker.add", json!({"seq": 1, "tick": 0, "name": "Verse"}))
        ),
        Ok(())
    );
    // A marker add needs no preview.
    let e = preview(json!([
        {"id": "marker.add", "params": {"seq": 1, "tick": 0, "name": "Verse"}}
    ]))
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed, "{}", e.message);
    // Every grid, both velocity ends and both transpose ends are admitted.
    for grid in ["1/4", "1/8", "1/16", "1/32", "1/8t", "1/16t"] {
        assert_eq!(
            preview(
                json!([{"id": "modify.quantize_notes", "params": {"seq": 1, "lanes": "1", "grid": grid}}])
            ),
            Ok(()),
            "{grid}"
        );
    }
    let grids: Vec<&str> = registry::GRIDS.iter().map(|(g, _)| *g).collect();
    assert_eq!(grids, ["1/4", "1/8", "1/16", "1/32", "1/8t", "1/16t"]);
    let ticks: Vec<i64> = registry::GRIDS.iter().map(|(_, t)| *t).collect();
    assert_eq!(ticks, [15360, 7680, 3840, 1920, 5120, 2560]);
    for (op, amount) in [
        ("set", 1),
        ("set", 127),
        ("add", -126),
        ("add", 126),
        ("add", 1),
        ("scale", 10),
        ("scale", 400),
    ] {
        assert_eq!(
            preview(
                json!([{"id": "modify.velocity_notes", "params": {"seq": 1, "lanes": "1", "op": op, "amount": amount}}])
            ),
            Ok(()),
            "{op} {amount}"
        );
    }
    for semitones in [-48, -1, 1, 48] {
        assert_eq!(
            preview(
                json!([{"id": "modify.transpose_notes", "params": {"seq": 1, "lanes": "1", "semitones": semitones}}])
            ),
            Ok(()),
            "{semitones}"
        );
    }
    let ids = (1..=64)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        preview(
            json!([{"id": "modify.transpose_notes", "params": {"seq": 1, "lanes": ids, "semitones": 12}}])
        ),
        Ok(())
    );
}

/// Each of `params` is `invalid_params` for `id`, both run and previewed.
fn all_invalid(id: &str, cases: &[Value]) {
    for params in cases {
        assert_code(
            "command.run",
            run(id, params.clone()),
            ErrorCode::InvalidParams,
        );
        let e = preview(json!([{"id": id, "params": params}])).unwrap_err();
        assert_eq!(
            e.code,
            ErrorCode::InvalidParams,
            "{id} {params}: {}",
            e.message
        );
    }
}

/// `base` with `key` set to `value` (or removed, for null).
fn with(base: &Value, key: &str, value: Value) -> Value {
    let mut v = base.clone();
    let o = v.as_object_mut().unwrap();
    if value.is_null() {
        o.remove(key);
    } else {
        o.insert(key.into(), value);
    }
    v
}

#[test]
fn note_chores_refuse_hostile_input() {
    let quantize = json!({"seq": 1, "lanes": "1", "grid": "1/16"});
    let transpose = json!({"seq": 1, "lanes": "1", "semitones": 2});
    let velocity = json!({"seq": 1, "lanes": "1", "op": "set", "amount": 64});
    let many = (1..=65)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    for (id, base) in [
        ("modify.quantize_notes", &quantize),
        ("modify.transpose_notes", &transpose),
        ("modify.velocity_notes", &velocity),
    ] {
        let mut cases = vec![];
        for lanes in [
            Value::Null,
            json!(""),
            json!("1,1"),
            json!("0"),
            json!("a"),
            json!("1,,2"),
            json!("-1"),
            json!("+1"),
            json!("1.5"),
            json!(3),
            json!(["1"]),
            json!("99999999999999999999999"),
            json!(many),
            json!("1".repeat(70_000)),
            json!("1,\0"),
        ] {
            cases.push(with(base, "lanes", lanes));
        }
        cases.extend([
            with(base, "seq", Value::Null),
            with(base, "seq", json!(-1)),
            with(base, "seq", json!("1")),
            with(base, "start", json!(10)),
            with(&with(base, "start", json!(10)), "end", json!(10)),
            with(&with(base, "start", json!(10)), "end", json!(5)),
            with(base, "end", json!(10)),
            with(&with(base, "start", json!(-1)), "end", json!(10)),
            with(base, "extra", json!(1)),
        ]);
        all_invalid(id, &cases);
    }
    all_invalid(
        "modify.quantize_notes",
        &[
            with(&quantize, "grid", json!("1/5")),
            with(&quantize, "grid", Value::Null),
            with(&quantize, "grid", json!(16)),
            with(&quantize, "strength", json!(0)),
            with(&quantize, "strength", json!(101)),
            with(&quantize, "strength", json!("50")),
        ],
    );
    all_invalid(
        "modify.transpose_notes",
        &[
            with(&transpose, "semitones", json!(0)),
            with(&transpose, "semitones", json!(49)),
            with(&transpose, "semitones", json!(-49)),
            with(&transpose, "semitones", Value::Null),
            with(&transpose, "semitones", json!(1.5)),
        ],
    );
    let op = |o: &str, a: i64| with(&with(&velocity, "op", json!(o)), "amount", json!(a));
    all_invalid(
        "modify.velocity_notes",
        &[
            op("set", 0),
            op("set", 128),
            op("add", 0),
            op("add", 127),
            op("add", -127),
            op("scale", 9),
            op("scale", 401),
            with(&velocity, "op", json!("multiply")),
            with(&velocity, "op", Value::Null),
            with(&velocity, "amount", Value::Null),
            with(&velocity, "amount", json!(i64::MAX)),
        ],
    );
    // The message names the rule broken.
    for (lanes, rule) in [
        ("1,1", "twice"),
        ("0", "1"),
        ("a", "not an ID"),
        ("", "1 to 64"),
    ] {
        let m = assert_code(
            "command.run",
            run(
                "modify.transpose_notes",
                with(&transpose, "lanes", json!(lanes)),
            ),
            ErrorCode::InvalidParams,
        );
        assert!(m.contains("lanes"), "{lanes:?}: {m}");
        assert!(m.contains(rule), "{lanes:?}: {m}");
    }
    let m = assert_code(
        "command.run",
        run(
            "modify.transpose_notes",
            with(&transpose, "lanes", json!(many)),
        ),
        ErrorCode::InvalidParams,
    );
    assert!(m.contains("64"), "{m}");
}

#[test]
fn id_lists_parse() {
    use co_producer::id_list;
    assert_eq!(id_list("1,2,3"), Ok(vec![1, 2, 3]));
    assert_eq!(id_list("7"), Ok(vec![7]));
    assert_eq!(id_list(" 4 , 2"), Ok(vec![4, 2]));
    let ids = (1..=64)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(id_list(&ids).unwrap().len(), 64);
    for bad in [
        "",
        " ",
        "1,1",
        "0",
        "a",
        "1,",
        ",1",
        "1,,2",
        "-1",
        "+1",
        "1 2",
        "1.0",
        "0x1",
        "99999999999999999999999",
        "\u{0661}",
    ] {
        assert!(id_list(bad).is_err(), "{bad:?}");
    }
    assert!(id_list(&format!("{ids},65")).is_err());
    assert!(id_list(&"1,".repeat(70_000)).is_err());
}

#[test]
fn marker_commands_refuse_hostile_input() {
    use core_control::request::MAX_MARKER_NAME;
    assert_eq!(MAX_MARKER_NAME, 64);
    let add = json!({"seq": 1, "tick": 0, "name": "Verse"});
    let rename = json!({"seq": 1, "index": 1, "name": "Verse"});
    let remove = json!({"seq": 1, "index": 1});
    assert_eq!(
        gate(
            "command.run",
            run("marker.add", with(&add, "name", json!("x".repeat(64))))
        ),
        Ok(())
    );
    assert_eq!(
        gate(
            "command.run",
            run("marker.add", with(&add, "name", json!("  Bridge 2 ")))
        ),
        Ok(())
    );
    let names = [
        json!(""),
        json!("   "),
        json!("x".repeat(65)),
        json!("x".repeat(70_000)),
        json!("a\0b"),
        json!("a\nb"),
        json!("a\u{202E}b"),
        json!("a\u{2028}b"),
        json!("a\u{200F}b"),
        json!(5),
        Value::Null,
    ];
    for (id, base) in [("marker.add", &add), ("marker.rename", &rename)] {
        let mut cases: Vec<Value> = names
            .iter()
            .map(|n| with(base, "name", n.clone()))
            .collect();
        cases.extend([
            with(base, "seq", json!(-1)),
            with(base, "seq", Value::Null),
            with(base, "extra", json!(true)),
        ]);
        if id == "marker.add" {
            cases.extend([
                with(base, "tick", json!(-1)),
                with(base, "tick", Value::Null),
                with(base, "tick", json!("0")),
                with(base, "index", json!(1)),
            ]);
        } else {
            cases.extend([
                with(base, "index", json!(0)),
                with(base, "index", Value::Null),
                with(base, "tick", json!(0)),
            ]);
        }
        // marker.add is class add: its refusals come from validation alone.
        for params in &cases {
            assert_code(
                "command.run",
                run(id, params.clone()),
                ErrorCode::InvalidParams,
            );
        }
        if id == "marker.rename" {
            all_invalid(id, &cases);
        }
    }
    all_invalid(
        "marker.remove",
        &[
            with(&remove, "index", json!(0)),
            with(&remove, "index", json!(-1)),
            with(&remove, "index", Value::Null),
            with(&remove, "seq", Value::Null),
            with(&remove, "name", json!("x")),
            with(&remove, "index", json!("1")),
        ],
    );
    // The stored name is trimmed.
    match validate(&req(
        "command.run",
        run("marker.add", with(&add, "name", json!("  Bridge  "))),
    ))
    .unwrap()
    {
        core_control::Command::CommandRun { args, .. } => {
            assert!(
                args.contains(&("name".to_string(), ArgValue::Text("Bridge".into()))),
                "{args:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

fn chore_export(params: Value) -> Result<(), ControlError> {
    gate("co_producer.export", params)
}

#[test]
fn export_refuses_hostile_input() {
    use std::os::unix::fs::symlink;
    for (params, sentence) in [
        (
            json!({"what": "selection"}),
            Some("Bounce a selection with export_folder and file_bounce."),
        ),
        (json!({"what": "video"}), None),
        (json!({}), None),
        (json!({"strips": "1"}), None),
        (json!({"what": 3}), None),
        (json!({"what": "Stems"}), None),
        (
            json!({"what": "dawproject", "strips": "1,2"}),
            Some("strips is only for stems and other_daw."),
        ),
        (
            json!({"what": "mix", "strips": "1"}),
            Some("strips is only for stems and other_daw."),
        ),
        (json!({"what": "stems", "strips": "1,1"}), None),
        (json!({"what": "stems", "strips": "0"}), None),
        (json!({"what": "stems", "strips": ""}), None),
        (json!({"what": "stems", "strips": "a"}), None),
        (json!({"what": "stems", "strips": [1, 2]}), None),
        (
            json!({"what": "stems", "strips": "1,".repeat(35_000)}),
            None,
        ),
        (
            json!({"what": "stems", "parent": format!("/{}", "x".repeat(70_000))}),
            None,
        ),
        (json!({"what": "stems", "parent": "/tmp/a\0b"}), None),
        (json!({"what": "stems", "parent": ""}), None),
        (json!({"what": "stems", "parent": 5}), None),
        (json!({"what": "stems", "extra": 1}), None),
        (json!({"what": "stems", "window": 0}), None),
    ] {
        let m = assert_code(
            "co_producer.export",
            params.clone(),
            ErrorCode::InvalidParams,
        );
        if let Some(s) = sentence {
            assert!(m.contains(s), "{params}: {m}");
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let plain = dir.join("Exports");
    std::fs::create_dir(&plain).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    let link = dir.join("link");
    symlink(&plain, &link).unwrap();
    let package = dir.join("X.vizdaw");
    std::fs::create_dir_all(package.join("Media")).unwrap();
    let s = |p: &Path| p.to_str().unwrap().to_string();
    for (parent, reason) in [
        ("Exports".to_string(), "absolute"),
        (s(&dir.join("Exports").join("..").join("Exports")), "`..`"),
        (s(&file), "not a folder"),
        (s(&link), "is a link"),
        (s(&package.join("Media")), "inside a project package"),
        (s(&package), "inside a project package"),
    ] {
        let e = chore_export(json!({"what": "stems", "parent": parent})).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotAllowed, "{parent}: {}", e.message);
        assert!(e.message.contains(&parent), "{parent}: {}", e.message);
        assert!(e.message.contains(reason), "{parent}: {}", e.message);
    }
    for params in [
        json!({"what": "stems", "parent": s(&plain)}),
        json!({"what": "stems"}),
        json!({"what": "stems", "strips": "2,3"}),
        json!({"what": "other_daw", "strips": "1", "parent": s(&plain), "window": 2}),
        json!({"what": "mix"}),
        json!({"what": "midi"}),
        json!({"what": "dawproject"}),
        json!({"what": "aaf"}),
        json!({"what": "other_daw"}),
    ] {
        assert_eq!(chore_export(params.clone()), Ok(()), "{params}");
    }
}

#[test]
fn export_only_on_the_co_producer_socket() {
    use core_control::client::{CallError, Client};
    use core_control::server::{Config, Server};
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let dev = Server::start(&sock, Config::default()).unwrap();
    let mut c = Client::connect(&sock).unwrap();
    match c.call(
        "co_producer.export",
        json!({"what": "stems"}),
        std::time::Duration::from_secs(20),
    ) {
        Err(CallError::Reply(e)) => {
            assert_eq!(e.code, ErrorCode::NotAllowed, "{e}");
            assert!(e.message.contains("only on the Co-Producer socket"), "{e}");
        }
        other => panic!("{other:?}"),
    }
    assert!(dev.next_call().is_none(), "the app never sees it");
}

#[test]
fn midi_clips_for_several_tracks_go_into_a_folder() {
    let lanes = |l: &str| vec![("lanes".to_string(), ArgValue::Text(l.into()))];
    assert_eq!(
        output("file.export_midi_clip", &lanes("3")),
        Some(Output::NewFile(&["midi2"]))
    );
    assert_eq!(
        output("file.export_midi_clip", &lanes("3,4")),
        Some(Output::IntoFolder)
    );
    assert_eq!(
        output("file.export_midi_clip", &[]),
        Some(Output::IntoFolder)
    );
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("Clips");
    std::fs::create_dir(&folder).unwrap();
    let file = tmp.path().join("a.midi2");
    std::fs::write(&file, b"x").unwrap();
    let path = |p: &Path| p.to_str().unwrap().to_string();
    assert_eq!(
        gate(
            "command.run",
            run(
                "file.export_midi_clip",
                json!({"path": path(&folder), "lanes": "3,4"})
            )
        ),
        Ok(())
    );
    assert_eq!(
        gate(
            "command.run",
            run("file.export_midi_clip", json!({"path": path(&folder)}))
        ),
        Ok(())
    );
    for params in [
        json!({"path": path(&file), "lanes": "3,4"}),
        json!({"path": path(&file), "lanes": "3"}),
        json!({"path": path(&folder), "lanes": "3"}),
    ] {
        assert_code(
            "command.run",
            run("file.export_midi_clip", params),
            ErrorCode::NotAllowed,
        );
    }
    assert_eq!(
        gate(
            "command.run",
            run(
                "file.export_midi_clip",
                json!({"path": path(&tmp.path().join("new.midi2")), "lanes": "3"})
            )
        ),
        Ok(())
    );
}

/// Plan 8d Ruling 1: the twelve mixer edits, each with valid params.
fn mixer_edits() -> Vec<(&'static str, Value)> {
    vec![
        ("mixer.set_strip", json!({"strip": 2, "gain_db": "-6"})),
        ("mixer.set_output", json!({"strip": 2, "bus": 3})),
        ("mixer.add_send", json!({"strip": 2, "bus": 3})),
        (
            "mixer.set_send",
            json!({"strip": 2, "index": 1, "level_db": "-3"}),
        ),
        ("mixer.remove_send", json!({"strip": 2, "index": 1})),
        ("mixer.add_insert", json!({"strip": 2, "plugin": "multi-q"})),
        ("mixer.remove_insert", json!({"slot": 4})),
        ("mixer.move_insert", json!({"slot": 4, "index": 1})),
        ("mixer.set_bypass", json!({"slot": 4, "bypass": true})),
        (
            "mixer.set_eq",
            json!({"strip": 2, "plugin": "clap:vizdaw.test.eq"}),
        ),
        (
            "mixer.set_param",
            json!({"slot": 4, "param": 0, "value": "0.5"}),
        ),
        ("mixer.remove_bus", json!({"strip": 3})),
    ]
}

#[test]
fn mixer_commands_have_their_class() {
    assert_eq!(registry::MIXER_IDS.len(), 13);
    assert_eq!(registry::PLUGIN_REF_MAX, 512);
    let edits = mixer_edits();
    let ids: Vec<&str> = edits.iter().map(|(id, _)| *id).collect();
    let mut all = ids.clone();
    all.push("track.new_plugin_instrument");
    assert_eq!(registry::MIXER_IDS, &all[..]);
    for id in registry::MIXER_IDS {
        let c = registry::find(id).unwrap_or_else(|| panic!("{id} is not registered"));
        let want = match *id {
            "mixer.remove_send" | "mixer.remove_insert" | "mixer.remove_bus" => Class::Remove,
            "track.new_plugin_instrument" => Class::Add,
            _ => Class::Change,
        };
        assert_eq!(c.class, want, "{id}");
        assert!(!c.menu_item, "{id} is key-only");
        assert!(c.document, "{id} acts on a document");
        assert_eq!(c.shortcut, None, "{id}");
        let menu = if *id == "track.new_plugin_instrument" {
            "Track"
        } else {
            "Mixer"
        };
        assert_eq!(c.menu, menu, "{id}");
    }
    for (id, title) in [
        ("mixer.set_strip", "Set Strip"),
        ("mixer.set_output", "Set Strip Output"),
        ("mixer.add_send", "Add Send"),
        ("mixer.set_send", "Set Send"),
        ("mixer.remove_send", "Remove Send"),
        ("mixer.add_insert", "Add Insert"),
        ("mixer.remove_insert", "Remove Insert"),
        ("mixer.move_insert", "Move Insert"),
        ("mixer.set_bypass", "Bypass Insert"),
        ("mixer.set_eq", "Set EQ"),
        ("mixer.set_param", "Set Plug-in Parameter"),
        ("mixer.remove_bus", "Remove Bus"),
        (
            "track.new_plugin_instrument",
            "New Plug-in Instrument Track",
        ),
    ] {
        assert_eq!(registry::find(id).unwrap().title, title, "{id}");
    }
    let list = co_producer::list_json();
    let commands = list["commands"].as_array().unwrap();
    let listed = |id: &str| {
        commands
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not listed"))
            .clone()
    };
    for id in registry::MIXER_IDS {
        let c = listed(id);
        let previewable = *id != "track.new_plugin_instrument";
        assert_eq!(c["previewable"], json!(previewable), "{id}");
        assert_eq!(PREVIEWABLE.contains(id), previewable, "{id}");
    }
    assert_eq!(listed("track.new_plugin_instrument")["class"], "add");
    let bus = listed("mixer.new_bus");
    assert_eq!(bus["previewable"], json!(true));
    let spec = registry::find("mixer.new_bus").unwrap();
    assert_eq!(effective_class(spec, &[]), Class::Add);
    assert_eq!(
        effective_class(spec, &[("strip".into(), ArgValue::Int(2))]),
        Class::Change
    );
}

#[test]
fn mixer_changes_only_through_a_preview() {
    let direct = |id: &str| {
        format!(
            "{id} changes your work: preview it with co_producer.preview and apply it with the composer's yes."
        )
    };
    let mut edits = mixer_edits();
    edits.push(("mixer.new_bus", json!({"strip": 2})));
    edits.push(("mixer.new_bus", json!({"strip": 2, "name": "Drums"})));
    for (id, params) in &edits {
        let m = assert_code(
            "command.run",
            run(id, params.clone()),
            ErrorCode::NotAllowed,
        );
        assert_eq!(m, direct(id));
        assert_eq!(
            preview(json!([{"id": id, "params": params}])),
            Ok(()),
            "{id} {params}"
        );
    }
    // One plan holding all of them.
    let plan: Vec<Value> = edits
        .iter()
        .map(|(id, params)| json!({"id": id, "params": params}))
        .collect();
    assert_eq!(preview(Value::Array(plan.clone())), Ok(()));
    assert_eq!(
        gate(
            "co_producer.apply",
            json!({"token": "0123456789abcdef0123456789abcdef", "commands": plan})
        ),
        Ok(())
    );
    // The adds run directly.
    for (id, params) in [
        (
            "track.new_plugin_instrument",
            json!({"plugin": "clap:vizdaw.test.synth4"}),
        ),
        (
            "track.new_plugin_instrument",
            json!({"plugin": "built-in-synth", "seq": 1, "after": 2}),
        ),
        ("mixer.new_bus", json!({"name": "Drums"})),
        ("mixer.new_bus", json!({})),
    ] {
        assert_eq!(gate("command.run", run(id, params.clone())), Ok(()), "{id}");
    }
    // An add needs no preview: a free bus and a new instrument track.
    for (id, params) in [
        ("mixer.new_bus", json!({"name": "Drums"})),
        (
            "track.new_plugin_instrument",
            json!({"plugin": "clap:vizdaw.test.synth4"}),
        ),
    ] {
        let e = preview(json!([{"id": id, "params": params}])).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotAllowed, "{id}: {}", e.message);
        assert_eq!(
            e.message,
            format!("{id} needs no preview: run it with command.run.")
        );
    }
}

#[test]
fn mixer_commands_refuse_hostile_input() {
    let long = "1".repeat(70_000);
    let big = json!(1u64 << 53);
    let edits = mixer_edits();
    // A missing, zero, negative, text or too-large target.
    for (id, base) in &edits {
        let target = if base.get("strip").is_some() {
            "strip"
        } else {
            "slot"
        };
        let mut cases = vec![with(base, target, Value::Null)];
        for bad in [json!(0), json!(-1), json!("2"), big.clone(), json!(2.5)] {
            cases.push(with(base, target, bad));
        }
        cases.push(with(base, "extra", json!(1)));
        all_invalid(id, &cases);
    }
    // Strip, slot and index at 2^53 - 1 still pass the registry.
    let top = json!((1u64 << 53) - 1);
    assert_eq!(
        preview(json!([{"id": "mixer.remove_insert", "params": {"slot": top}}])),
        Ok(())
    );
    let strip = json!({"strip": 2});
    let mut gains = vec![
        strip.clone(),
        with(&strip, "pan", json!(101)),
        with(&strip, "pan", json!(-101)),
        with(&strip, "pan", json!("10")),
        with(&strip, "mute", json!("true")),
        with(&strip, "gain_db", json!(-6)),
    ];
    for bad in [
        "13",
        "-145",
        "1e3",
        "NaN",
        "inf",
        "+1",
        "0x10",
        "1,5",
        "",
        " 1",
        "1.",
        ".5",
        "-",
        "1234567",
        "1.1234567",
        "١",
    ] {
        gains.push(with(&strip, "gain_db", json!(bad)));
    }
    gains.push(with(&strip, "gain_db", json!(long)));
    gains.push(with(&strip, "gain_db", json!("-6\u{0}")));
    all_invalid("mixer.set_strip", &gains);
    // Each of the four alone, and the ends of both ranges, pass.
    for params in [
        json!({"strip": 2, "gain_db": "-144"}),
        json!({"strip": 2, "gain_db": "12"}),
        json!({"strip": 2, "gain_db": "-0.5"}),
        json!({"strip": 2, "pan": -100}),
        json!({"strip": 2, "pan": 100}),
        json!({"strip": 2, "mute": false}),
        json!({"strip": 2, "solo": true}),
    ] {
        assert_eq!(
            preview(json!([{"id": "mixer.set_strip", "params": params}])),
            Ok(()),
            "{params}"
        );
    }
    all_invalid(
        "mixer.set_output",
        &[
            json!({"strip": 2}),
            json!({"strip": 2, "bus": "master"}),
            json!({"strip": 2, "bus": 0}),
            json!({"strip": 2, "bus": true}),
        ],
    );
    assert_eq!(
        preview(json!([{"id": "mixer.set_output", "params": {"strip": 2, "bus": null}}])),
        Ok(()),
        "null is master"
    );
    all_invalid(
        "mixer.add_send",
        &[
            json!({"strip": 2}),
            json!({"strip": 2, "bus": 0}),
            json!({"strip": 2, "bus": null}),
            json!({"strip": 2, "bus": 3, "level_db": "13"}),
            json!({"strip": 2, "bus": 3, "level_db": "NaN"}),
            json!({"strip": 2, "bus": 3, "pre_fader": 1}),
        ],
    );
    all_invalid(
        "mixer.set_send",
        &[
            json!({"strip": 2, "index": 1}),
            json!({"strip": 2, "index": 0, "enabled": true}),
            json!({"strip": 2, "index": -1, "enabled": true}),
            json!({"strip": 2, "enabled": true}),
            json!({"strip": 2, "index": 1, "level_db": "1e3"}),
        ],
    );
    for params in [
        json!({"strip": 2, "index": 1, "enabled": false}),
        json!({"strip": 2, "index": 1, "pre_fader": true}),
    ] {
        assert_eq!(
            preview(json!([{"id": "mixer.set_send", "params": params}])),
            Ok(()),
            "{params}"
        );
    }
    all_invalid(
        "mixer.remove_send",
        &[json!({"strip": 2}), json!({"strip": 2, "index": 0})],
    );
    all_invalid(
        "mixer.move_insert",
        &[json!({"slot": 4}), json!({"slot": 4, "index": 0})],
    );
    all_invalid(
        "mixer.add_insert",
        &[json!({"strip": 2, "plugin": "multi-q", "index": 0})],
    );
    all_invalid(
        "mixer.set_bypass",
        &[json!({"slot": 4}), json!({"slot": 4, "bypass": "yes"})],
    );
    // Plug-in references (Ruling 3's shape).
    let bad_refs = [
        json!(""),
        json!("clap:"),
        json!("au:"),
        json!("vst:x"),
        json!("multi-q\u{0}"),
        json!("au:a\u{202E}b"),
        json!("clap:a\nb"),
        json!("Multi-Q"),
        json!(" multi-q"),
        json!(format!("clap:{}", "x".repeat(508))),
        json!("x".repeat(70_000)),
        json!(7),
    ];
    assert_eq!(format!("clap:{}", "x".repeat(508)).chars().count(), 513);
    for (id, base) in [
        ("mixer.add_insert", json!({"strip": 2, "plugin": "multi-q"})),
        ("mixer.set_eq", json!({"strip": 2, "plugin": "multi-q"})),
        (
            "track.new_plugin_instrument",
            json!({"plugin": "built-in-synth"}),
        ),
    ] {
        let mut cases: Vec<Value> = bad_refs
            .iter()
            .map(|r| with(&base, "plugin", r.clone()))
            .collect();
        cases.push(with(&base, "plugin", Value::Null));
        for (case, label) in cases.iter().zip(0..) {
            let e = validate(&req("command.run", run(id, case.clone()))).unwrap_err();
            assert_eq!(
                e.code,
                ErrorCode::InvalidParams,
                "{id} #{label}: {}",
                e.message
            );
        }
    }
    for (id, base) in [
        ("mixer.add_insert", json!({"strip": 2})),
        ("mixer.set_eq", json!({"strip": 2})),
    ] {
        for good in [
            "multi-q",
            "clap:vizdaw.test.eq",
            "au:aufx/Test/Viz",
            &format!("clap:{}", "x".repeat(507)),
        ] {
            assert_eq!(
                preview(json!([{"id": id, "params": with(&base, "plugin", json!(good))}])),
                Ok(()),
                "{id} {good}"
            );
        }
        // The Built-in Synth is an instrument, never an insert or the EQ.
        all_invalid(id, &[with(&base, "plugin", json!("built-in-synth"))]);
    }
    for good in [
        "multi-q",
        "built-in-synth",
        "clap:vizdaw.test.synth4",
        "au:aumu/Test/Viz",
    ] {
        assert_eq!(
            gate(
                "command.run",
                run("track.new_plugin_instrument", json!({"plugin": good}))
            ),
            Ok(()),
            "{good}"
        );
    }
    // A plug-in parameter.
    all_invalid(
        "mixer.set_param",
        &[
            json!({"slot": 4, "param": 0, "value": "NaN"}),
            json!({"slot": 4, "param": 0, "value": "1e3"}),
            json!({"slot": 4, "param": 0, "value": 0.5}),
            json!({"slot": 4, "param": 0}),
            json!({"slot": 4, "value": "0.5"}),
            json!({"slot": 4, "param": -1, "value": "0.5"}),
            json!({"slot": 4, "param": "0", "value": "0.5"}),
            json!({"slot": 4, "param": 1u64 << 32, "value": "0.5"}),
        ],
    );
    assert_eq!(
        preview(
            json!([{"id": "mixer.set_param", "params": {"slot": 4, "param": 12, "value": "-999999.5"}}])
        ),
        Ok(())
    );
    // track.new_plugin_instrument's own params.
    let e = validate(&req(
        "command.run",
        run("track.new_plugin_instrument", json!({})),
    ))
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams, "{}", e.message);
    // An unknown param on any of the thirteen.
    for id in registry::MIXER_IDS {
        let e = validate(&req("command.run", run(id, json!({"colour": "red"})))).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{id}: {}", e.message);
    }
    for (id, base) in &edits {
        let e = validate(&req("command.run", run(id, with(base, "window", json!(1))))).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{id}: {}", e.message);
    }
}

#[test]
fn plugin_calls_only_on_the_co_producer_socket() {
    use core_control::client::{CallError, Client};
    use core_control::server::{Config, Server};
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let dev = Server::start(&sock, Config::default()).unwrap();
    let mut c = Client::connect(&sock).unwrap();
    for (cmd, params) in [
        ("co_producer.plugins", json!({})),
        ("co_producer.plugins", json!({"kind": "effect"})),
        ("co_producer.params", json!({"slot": 4})),
    ] {
        match c.call(cmd, params, std::time::Duration::from_secs(20)) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd}: {e}");
                assert!(e.message.contains("only on the Co-Producer socket"), "{e}");
            }
            other => panic!("{cmd}: {other:?}"),
        }
    }
    assert!(dev.next_call().is_none(), "the app never sees them");
    // The gate admits both, with no request open: they only read.
    for (cmd, params) in [
        ("co_producer.plugins", json!({})),
        ("co_producer.plugins", json!({"kind": "instrument"})),
        ("co_producer.plugins", json!({"kind": "effect"})),
        ("co_producer.params", json!({"slot": 4})),
        ("co_producer.params", json!({"slot": 4, "window": "key"})),
    ] {
        assert_eq!(gate(cmd, params.clone()), Ok(()), "{cmd} {params}");
        assert!(
            validate(&req(cmd, params)).unwrap().is_co_producer(),
            "{cmd}"
        );
    }
}

#[test]
fn plugin_calls_refuse_hostile_input() {
    for (cmd, params) in [
        ("co_producer.plugins", json!({"kind": "synth"})),
        ("co_producer.plugins", json!({"kind": "Effect"})),
        ("co_producer.plugins", json!({"kind": 1})),
        ("co_producer.plugins", json!({"kind": "effect\u{0}"})),
        ("co_producer.plugins", json!({"kind": "x".repeat(70_000)})),
        ("co_producer.plugins", json!({"extra": 1})),
        ("co_producer.plugins", json!({"window": 1})),
        ("co_producer.params", json!({})),
        ("co_producer.params", json!({"slot": 0})),
        ("co_producer.params", json!({"slot": -1})),
        ("co_producer.params", json!({"slot": "a"})),
        ("co_producer.params", json!({"slot": "4"})),
        ("co_producer.params", json!({"slot": 1u64 << 53})),
        ("co_producer.params", json!({"slot": 4, "window": 0})),
        ("co_producer.params", json!({"slot": 4, "extra": 1})),
    ] {
        let e = validate(&req(cmd, params.clone())).unwrap_err();
        assert_eq!(
            e.code,
            ErrorCode::InvalidParams,
            "{cmd} {params}: {}",
            e.message
        );
    }
}

/// A valid `automation.write`: a fade over bar 1 to 2 on strip 3.
fn fade() -> Value {
    json!({"seq": 1, "target": "fader:3", "start": 0, "end": 61440, "points": "0:0 61440:-144"})
}

#[test]
fn creative_commands_have_their_class() {
    assert_eq!(registry::CREATIVE_IDS, &["automation.write"]);
    let c = registry::find("automation.write").unwrap();
    assert_eq!(c.class, Class::Change);
    assert_eq!(c.title, "Write Automation");
    assert_eq!(c.menu, "Mixer");
    assert!(!c.menu_item, "key-only");
    assert!(c.document);
    assert_eq!(c.shortcut, None);
    assert!(PREVIEWABLE.contains(&"automation.write"));
    assert_eq!(
        PREVIEWABLE.len(),
        28,
        "8e adds automation.write; Task 6b adds the three release-fixes changes"
    );
    let list = co_producer::list_json();
    let listed = list["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "automation.write")
        .expect("listed")
        .clone();
    assert_eq!(listed["previewable"], json!(true));
    assert_eq!(listed["class"], "change");
}

#[test]
fn co_producer_writes_no_notes() {
    // Owner, 2026-11-16: no Co-Producer notes in 1.0.
    assert_eq!(registry::CREATIVE_IDS, &["automation.write"]);
    for id in ["midi.new_part", "midi.add_notes"] {
        assert!(registry::find(id).is_none(), "{id} is registered");
        assert!(!PREVIEWABLE.contains(&id), "{id}");
        let params = json!({"seq": 1, "lane": 2, "notes": "0:60:3840:100"});
        let e = refused("command.run", run(id, params.clone()));
        assert_eq!(e.code, ErrorCode::UnknownId, "{id}: {}", e.message);
        let e = preview(json!([{"id": id, "params": params}])).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnknownId, "{id}: {}", e.message);
        // As any unregistered id is.
        let other = refused("command.run", run("midi.no_such", json!({})));
        assert_eq!(e.code, other.code);
    }
    assert!(
        !COMMANDS
            .iter()
            .any(|c| c.id.starts_with("midi.") && c.id != "midi.inject"),
        "no midi.* writer"
    );
}

#[test]
fn creative_changes_only_through_a_preview() {
    let m = assert_code(
        "command.run",
        run("automation.write", fade()),
        ErrorCode::NotAllowed,
    );
    assert_eq!(
        m,
        "automation.write changes your work: preview it with co_producer.preview and apply it with the composer's yes."
    );
    let plan = json!([{"id": "automation.write", "params": fade()}]);
    assert_eq!(preview(plan.clone()), Ok(()));
    assert_eq!(
        gate(
            "co_producer.apply",
            json!({"token": "0123456789abcdef0123456789abcdef", "commands": plan})
        ),
        Ok(())
    );
    // A new song's first sequence is 0.
    let main = json!([{"id": "automation.write", "params": with(&fade(), "seq", json!(0))}]);
    assert_eq!(preview(main), Ok(()));
    // Every target form, and an empty list that clears the range.
    for target in [
        "fader:3",
        "pan:3",
        "mute:3",
        "solo:3",
        "send:3:1",
        "param:9:4",
    ] {
        let p = with(&with(&fade(), "target", json!(target)), "points", json!(""));
        assert_eq!(
            preview(json!([{"id": "automation.write", "params": p}])),
            Ok(()),
            "{target}"
        );
    }
}

#[test]
fn creative_commands_refuse_hostile_input() {
    let base = fade();
    let mut cases = vec![];
    for seq in [
        Value::Null,
        json!(-1),
        json!("2"),
        json!(1u64 << 53),
        json!(1.5),
    ] {
        cases.push(with(&base, "seq", seq));
    }
    for (key, value) in [
        ("end", json!(0)),
        ("end", Value::Null),
        ("start", Value::Null),
        ("start", json!(-1)),
        ("start", json!("0")),
        ("target", json!("fader:x")),
        ("target", json!("vel:1")),
        ("target", json!("fader:")),
        ("target", json!("fader:3:1")),
        ("target", json!("send:3")),
        ("target", json!("send:3:0")),
        ("target", json!("param:9")),
        ("target", json!("fader:-3")),
        ("target", json!("fader: 3")),
        ("target", json!("fader:3\u{0}")),
        ("target", json!("")),
        ("target", json!(3)),
        ("target", Value::Null),
        ("target", json!("1".repeat(70_000))),
        ("target", json!(format!("fader:{}", "1".repeat(70)))),
        ("points", Value::Null),
        ("points", json!(1)),
        ("points", json!("1".repeat(32_769))),
        ("points", json!("0:1e3")),
        ("points", json!("61441:0")),
        ("points", json!("0:0 0:1")),
        ("extra", json!(1)),
    ] {
        cases.push(with(&base, key, value));
    }
    // `end` equal to `start`, and before it.
    cases.push(json!({"seq": 1, "target": "fader:3", "start": 3840, "end": 3840, "points": ""}));
    cases.push(json!({"seq": 1, "target": "fader:3", "start": 3840, "end": 0, "points": ""}));
    all_invalid("automation.write", &cases);
    // A point list near its byte limit, but of too many points.
    let many: Vec<String> = (0..=1024).map(|i| format!("{i}:0")).collect();
    all_invalid(
        "automation.write",
        &[with(
            &with(&base, "points", json!(many.join(" "))),
            "end",
            json!(2000),
        )],
    );
    // 1024 points of long values fit the list's 32 768 bytes.
    let full: Vec<String> = (0..1024)
        .map(|i| format!("{}:-123456.123456", i * 60))
        .collect();
    let text = full.join(" ");
    assert!(text.len() > 4096 && text.len() <= registry::LIST_TEXT_MAX);
    assert_eq!(
        preview(json!([{"id": "automation.write", "params": with(&base, "points", json!(text))}])),
        Ok(())
    );
}

#[test]
fn starter_and_reads_only_on_the_co_producer_socket() {
    use core_control::client::{CallError, Client};
    use core_control::server::{Config, Server};
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let dev = Server::start(&sock, Config::default()).unwrap();
    let mut c = Client::connect(&sock).unwrap();
    let starter = json!({"spec": {"title": "Demo", "tempo": "120", "meter": "4/4",
        "tracks": [{"name": "Keys", "kind": "instrument"}]}});
    let calls = [
        ("co_producer.starter", starter),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "start": 0, "end": 61440}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "fader:3", "start": 0, "end": 61440}),
        ),
    ];
    for (cmd, params) in &calls {
        match c.call(cmd, params.clone(), std::time::Duration::from_secs(20)) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{cmd}: {e}");
                assert!(e.message.contains("only on the Co-Producer socket"), "{e}");
            }
            other => panic!("{cmd}: {other:?}"),
        }
    }
    assert!(dev.next_call().is_none(), "the app never sees them");
    // The gate admits each with no request open.
    let mut admitted = calls.to_vec();
    admitted.extend([
        ("co_producer.midi_notes", json!({"seq": 1, "lane": 2})),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "window": "key"}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "param:9:4", "start": 0, "end": 1, "window": 3}),
        ),
        // A new song's first sequence and track are 0.
        ("co_producer.midi_notes", json!({"seq": 0, "lane": 0})),
        (
            "co_producer.automation",
            json!({"seq": 0, "target": "fader:0", "start": 0, "end": 1}),
        ),
    ]);
    for (cmd, params) in admitted {
        assert_eq!(gate(cmd, params.clone()), Ok(()), "{cmd} {params}");
        assert!(
            validate(&req(cmd, params)).unwrap().is_co_producer(),
            "{cmd}"
        );
    }
    // The gate checks the whole spec, so the app never sees a bad one.
    let e = refused(
        "co_producer.starter",
        json!({"spec": {"title": "Demo", "tempo": "120", "meter": "4/3",
            "tracks": [{"name": "Keys", "kind": "instrument"}]}}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams, "{}", e.message);
    let e = refused(
        "co_producer.starter",
        json!({"spec": {"title": "Demo", "tempo": "120", "meter": "4/4", "extra": 1,
            "tracks": [{"name": "Keys", "kind": "instrument"}]}}),
    );
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert!(
        e.message
            .ends_with("Unknown key extra in the starter project."),
        "{}",
        e.message
    );
    for params in [
        json!({}),
        json!({"spec": "x"}),
        json!({"spec": []}),
        json!({"spec": {"title": "Demo"}, "window": 1}),
        json!({"spec": {}, "extra": 1}),
    ] {
        let e = refused("co_producer.starter", params.clone());
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}: {}", e.message);
    }
}

#[test]
fn reads_refuse_hostile_input() {
    let long = "1".repeat(70_000);
    for (cmd, params) in [
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "start": 0}),
        ),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "end": 10}),
        ),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "start": 10, "end": 10}),
        ),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "start": 10, "end": 5}),
        ),
        ("co_producer.midi_notes", json!({"lane": 2})),
        ("co_producer.midi_notes", json!({"seq": 1})),
        ("co_producer.midi_notes", json!({"seq": -1, "lane": 2})),
        ("co_producer.midi_notes", json!({"seq": 1, "lane": -1})),
        ("co_producer.midi_notes", json!({"seq": "2", "lane": 2})),
        ("co_producer.midi_notes", json!({"seq": 1, "lane": "2"})),
        (
            "co_producer.midi_notes",
            json!({"seq": 1u64 << 53, "lane": 2}),
        ),
        ("co_producer.midi_notes", json!({"seq": "1", "lane": 2})),
        ("co_producer.midi_notes", json!({"seq": 1, "lane": -2})),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 1u64 << 53}),
        ),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "extra": 1}),
        ),
        (
            "co_producer.midi_notes",
            json!({"seq": 1, "lane": 2, "window": long}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "start": 0, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "fader:3", "start": 10, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "fader:3", "start": 10, "end": 0}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "fader:3", "start": 0}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "fader:3", "start": 0, "end": 10, "extra": 1}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "vel:1", "start": 0, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": long, "start": 0, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1, "target": "fader:3\u{0}", "start": 0, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"seq": -1, "target": "fader:3", "start": 0, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"seq": "2", "target": "fader:3", "start": 0, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"seq": 1u64 << 53, "target": "fader:3", "start": 0, "end": 10}),
        ),
        (
            "co_producer.automation",
            json!({"target": "fader:3", "start": 0, "end": 10}),
        ),
    ] {
        let e = validate(&req(cmd, params.clone())).unwrap_err();
        assert_eq!(
            e.code,
            ErrorCode::InvalidParams,
            "{cmd} {params}: {}",
            e.message
        );
    }
}

/// Plan 8e Task 6b: the release-fixes changes, each with valid explicit
/// targets.
fn fixes_changes() -> Vec<(&'static str, Value)> {
    vec![
        (
            "region.set_fade_shape",
            json!({"seq": 1, "lane": 2, "event": 3, "fade": "in", "shape": "s_curve"}),
        ),
        ("destination.set_sound", json!({"dest": 4, "sound": "pad"})),
        ("lane.set_output", json!({"seq": 1, "lane": 2, "output": 5})),
        (
            "lane.set_output",
            json!({"seq": 0, "lane": 0, "output": null}),
        ),
    ]
}

#[test]
fn fixes_commands_have_their_class() {
    for id in [
        "region.set_fade_shape",
        "destination.set_sound",
        "lane.set_output",
    ] {
        let c = registry::find(id).unwrap();
        assert_eq!(c.class, Class::Change, "{id}");
        assert!(PREVIEWABLE.contains(&id), "{id}");
    }
    // App-wide, as 8a Ruling 5's refused list: the channel's touches are
    // not the song's.
    let c = registry::find("automation.release_all").unwrap();
    assert_eq!(c.class, Class::Refused);
    assert!(!PREVIEWABLE.contains(&"automation.release_all"));
    let m = assert_code(
        "command.run",
        run("automation.release_all", json!({})),
        ErrorCode::NotAllowed,
    );
    assert_eq!(m, "Co-Producer cannot run automation.release_all.");
    let e = preview(json!([{"id": "automation.release_all", "params": {}}])).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotAllowed);
    assert_eq!(e.message, "Co-Producer cannot run automation.release_all.");
    let list = co_producer::list_json();
    let commands = list["commands"].as_array().unwrap();
    assert!(commands.iter().all(|c| c["id"] != "automation.release_all"));
    for id in [
        "region.set_fade_shape",
        "destination.set_sound",
        "lane.set_output",
    ] {
        let c = commands.iter().find(|c| c["id"] == id).unwrap();
        assert_eq!(c["previewable"], json!(true), "{id}");
        assert_eq!(c["class"], "change", "{id}");
    }
}

#[test]
fn fixes_changes_only_through_a_preview() {
    for (id, params) in fixes_changes() {
        let m = assert_code(
            "command.run",
            run(id, params.clone()),
            ErrorCode::NotAllowed,
        );
        assert_eq!(
            m,
            format!(
                "{id} changes your work: preview it with co_producer.preview and apply it with the composer's yes."
            )
        );
        assert_eq!(
            preview(json!([{"id": id, "params": params}])),
            Ok(()),
            "{id} {params}"
        );
    }
    let plan: Vec<Value> = fixes_changes()
        .into_iter()
        .map(|(id, params)| json!({"id": id, "params": params}))
        .collect();
    assert_eq!(preview(Value::Array(plan.clone())), Ok(()));
    assert_eq!(
        gate(
            "co_producer.apply",
            json!({"token": "0123456789abcdef0123456789abcdef", "commands": plan})
        ),
        Ok(())
    );
    // A plan names its targets: the selection is the composer's, not
    // Co-Producer's.
    for (id, params, want) in [
        (
            "region.set_fade_shape",
            json!({"fade": "in", "shape": "linear"}),
            "region.set_fade_shape: name the region's `seq`, `lane` and `event`",
        ),
        (
            "region.set_fade_shape",
            json!({"seq": 1, "fade": "in", "shape": "linear"}),
            "region.set_fade_shape: name the region's `seq`, `lane` and `event`",
        ),
        (
            "lane.set_output",
            json!({"output": 3}),
            "lane.set_output: name the track's `seq` and `lane`",
        ),
        (
            "lane.set_output",
            json!({"lane": 2, "output": 3}),
            "lane.set_output: name the track's `seq` and `lane`",
        ),
    ] {
        let e = preview(json!([{"id": id, "params": params}])).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{id} {params}");
        assert!(e.message.contains(want), "{id} {params}: {}", e.message);
    }
}

#[test]
fn fixes_commands_refuse_hostile_input() {
    let long = json!("x".repeat(70_000));
    let fade = json!({"seq": 1, "lane": 2, "event": 3, "fade": "in", "shape": "s_curve"});
    let mut cases = vec![];
    for (key, value) in [
        ("fade", Value::Null),
        ("shape", Value::Null),
        ("fade", json!("middle")),
        ("fade", json!("In")),
        ("fade", json!(1)),
        ("fade", json!("in\u{0}")),
        ("shape", json!("cubic")),
        ("shape", json!("s_curve\u{0}")),
        ("shape", long.clone()),
        ("event", json!(-1)),
        ("event", json!("3")),
        ("event", json!(1u64 << 63)),
        ("seq", json!(1.5)),
        ("lane", json!(true)),
        ("extra", json!(1)),
    ] {
        cases.push(with(&fade, key, value));
    }
    all_invalid("region.set_fade_shape", &cases);
    let sound = json!({"dest": 4, "sound": "pad"});
    let mut cases = vec![];
    for (key, value) in [
        ("dest", Value::Null),
        ("sound", Value::Null),
        ("dest", json!("4")),
        ("dest", json!(-1)),
        ("dest", json!(1u64 << 63)),
        ("sound", json!("Pad")),
        ("sound", json!("kazoo")),
        ("sound", json!("pad\u{0}")),
        ("sound", json!(3)),
        ("sound", long.clone()),
        ("extra", json!(1)),
    ] {
        cases.push(with(&sound, key, value));
    }
    all_invalid("destination.set_sound", &cases);
    let output = json!({"seq": 1, "lane": 2, "output": 5});
    let mut cases = vec![];
    for (key, value) in [
        ("output", Value::Null),
        ("output", json!("5")),
        ("output", json!(-1)),
        ("output", json!(true)),
        ("output", json!(1e30)),
        ("output", json!("a\u{0}b")),
        ("output", long.clone()),
        ("seq", json!(-1)),
        ("lane", json!("2")),
        ("bus", json!(1)),
    ] {
        cases.push(with(&output, key, value));
    }
    all_invalid("lane.set_output", &cases);
    // A strip, a slot or a bus is not a destination or an output: the
    // wrong key is refused, as any unknown key.
    all_invalid(
        "destination.set_sound",
        &[json!({"strip": 4, "sound": "pad"})],
    );
    all_invalid(
        "lane.set_output",
        &[json!({"seq": 1, "lane": 2, "strip": 5})],
    );
}

/// Plan 8f Task 1: sends `cmd` on `stream` and answers it from `server`;
/// returns the call the app saw.
fn app_sees(
    server: &core_control::server::Server,
    stream: &mut std::os::unix::net::UnixStream,
    cmd: &str,
    params: Value,
) -> core_control::server::Call {
    use std::io::{BufRead, BufReader, Write};
    let line = core_control::protocol::request_line(&RequestId::Num(1), cmd, &params);
    stream.write_all(format!("{line}\n").as_bytes()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let call = loop {
        if let Some(c) = server.next_call() {
            break c;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{cmd} {params} never reached the app"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert!(server.reply(call.token, Ok(json!({}))));
    let mut reply = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut reply)
        .unwrap();
    assert!(reply.contains("\"ok\":true"), "{reply}");
    call
}

/// Plan 8f Ruling 1: only the composer turns counting on or off, or
/// deletes the records.
#[test]
fn time_settings_are_never_co_producers() {
    use core_control::client::{CallError, Client};
    use core_control::server::{Config, Role, Server};
    use std::os::unix::net::UnixStream;
    assert_eq!(registry::METRICS_SETTING, "settings.co_producer_metrics");
    assert_eq!(
        registry::DELETE_TIME_RECORDS,
        "settings.delete_time_records"
    );
    let cases = [
        (registry::METRICS_SETTING, json!({"on": true})),
        (registry::DELETE_TIME_RECORDS, json!({})),
    ];
    for (id, _) in &cases {
        let spec = registry::find(id).unwrap_or_else(|| panic!("{id} is not registered"));
        assert_eq!(spec.class, Class::Refused, "{id}");
        assert_eq!(effective_class(spec, &[]), Class::Refused, "{id}");
    }
    let list = co_producer::list_json();
    let listed = list["commands"].as_array().unwrap();
    for (id, _) in &cases {
        assert!(listed.iter().all(|c| c["id"] != *id), "{id} is listed");
    }
    for (id, params) in &cases {
        assert_code(
            "command.run",
            run(id, params.clone()),
            ErrorCode::NotAllowed,
        );
    }
    // Through the Co-Producer socket itself: the app never sees them.
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let server = Server::start(
        &sock,
        Config {
            role: Role::CoProducer,
            ..Config::default()
        },
    )
    .unwrap();
    let mut c = Client::connect(&sock).unwrap();
    for (id, params) in &cases {
        match c.call(
            "command.run",
            run(id, params.clone()),
            std::time::Duration::from_secs(20),
        ) {
            Err(CallError::Reply(e)) => assert_eq!(e.code, ErrorCode::NotAllowed, "{id}: {e}"),
            other => panic!("{id}: {other:?}"),
        }
    }
    assert!(server.next_call().is_none(), "the app never sees them");
    drop(server);
    // On the developer socket both reach the app.
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let dev = Server::start(&sock, Config::default()).unwrap();
    let mut s = UnixStream::connect(&sock).unwrap();
    for (id, params) in &cases {
        let call = app_sees(&dev, &mut s, "command.run", run(id, params.clone()));
        match call.command {
            core_control::Command::CommandRun { id: got, .. } => assert_eq!(got, *id),
            other => panic!("{id}: {other:?}"),
        }
    }
}

/// Plan 8f Ruling 6: the read is the Co-Producer socket's alone.
#[test]
fn metrics_reads_only_on_the_co_producer_socket() {
    use core_control::client::{CallError, Client};
    use core_control::server::{Config, Server};
    let d = private_tmp();
    let sock = d.path().join("ctl").join("s.sock");
    let dev = Server::start(&sock, Config::default()).unwrap();
    let mut c = Client::connect(&sock).unwrap();
    for params in [json!({}), json!({"days": 7})] {
        match c.call(
            "co_producer.metrics",
            params.clone(),
            std::time::Duration::from_secs(20),
        ) {
            Err(CallError::Reply(e)) => {
                assert_eq!(e.code, ErrorCode::NotAllowed, "{params}: {e}");
                assert!(e.message.contains("only on the Co-Producer socket"), "{e}");
            }
            other => panic!("{params}: {other:?}"),
        }
    }
    assert!(dev.next_call().is_none(), "the app never sees it");
    // The gate admits it with no request open: it only reads.
    for params in [
        json!({}),
        json!({"days": 7, "window": 3}),
        json!({"days": 1}),
        json!({"days": 3650}),
        json!({"window": "key"}),
        json!({"window": "main"}),
    ] {
        assert_eq!(
            gate("co_producer.metrics", params.clone()),
            Ok(()),
            "{params}"
        );
        assert!(
            validate(&req("co_producer.metrics", params))
                .unwrap()
                .is_co_producer()
        );
    }
}

/// Plan 8f Task 1: every hostile `co_producer.metrics` is `invalid_params`.
#[test]
fn metrics_read_refuses_hostile_input() {
    use core_control::request::METRICS_DAYS_MAX;
    assert_eq!(METRICS_DAYS_MAX, 3650);
    let long = json!("x".repeat(70_000));
    let mut cases = vec![];
    for days in [
        json!(0),
        json!(3651),
        json!(-1),
        json!("7"),
        json!(7.5),
        Value::Null,
        json!(1u64 << 53),
        json!(true),
        json!([7]),
        json!("7\u{0}"),
        long.clone(),
    ] {
        cases.push(json!({ "days": days }));
    }
    for window in [
        json!("Key"),
        json!("3"),
        json!(0),
        json!(-1),
        json!(1.5),
        json!(true),
        json!("key\u{0}"),
        json!(1u64 << 63),
        long.clone(),
    ] {
        cases.push(json!({ "window": window }));
    }
    cases.push(json!({"extra": 1}));
    cases.push(json!({"days": 7, "song": "x"}));
    for params in cases {
        let e = refused("co_producer.metrics", params.clone());
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}: {}", e.message);
    }
}
