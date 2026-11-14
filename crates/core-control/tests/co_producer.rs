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
    "edit.undo",
    "edit.redo",
    "file.save",
    "file.project_settings",
    "region.normalize",
    "region.fade_in_to_cursor",
    "region.fade_out_from_cursor",
    "region.crossfade",
    "lane.set_input",
    "lane.set_monitor",
    "lane.set_input_fx",
    "lane.set_mpe",
    "lane.track_synth",
    "destination.set_midi_mix",
    "destination.set_mpe",
    "destination.set_return_latency",
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
];
const REMOVE: &[&str] = &[
    "track.delete",
    "region.remove_fades",
    "region.remove_crossfade",
    "drum.remove_row",
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
    assert_eq!(COMMANDS.len(), 183);
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
    for id in ["settings.co_producer", "file.open_copy"] {
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

#[test]
fn import_midi_into_a_new_project_is_refused() {
    let spec = registry::find("file.import_midi").unwrap();
    let path = ("path".to_string(), ArgValue::Text("/tmp/a.mid".into()));
    assert_eq!(
        effective_class(spec, std::slice::from_ref(&path)),
        Class::Add
    );
    assert_eq!(
        effective_class(
            spec,
            &[path.clone(), ("new_project".into(), ArgValue::Flag(false))]
        ),
        Class::Add
    );
    assert_eq!(
        effective_class(spec, &[path, ("new_project".into(), ArgValue::Flag(true))]),
        Class::Refused
    );
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
        (
            "command.run",
            run("file.import_midi", json!({"path": existing})),
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
    let e = preview(json!([{"id": "mixer.new_bus", "params": {"strip": 2}}])).unwrap_err();
    assert_eq!(e.message, "Co-Producer cannot preview mixer.new_bus yet.");
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
        core_control::Command::CoProducerBegin { name, window } => {
            assert_eq!(name, "Drum bus");
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
    assert!(matches!(
        output("file.export_midi_clip", &[]),
        Some(Output::NewFile(_))
    ));
    for id in [
        "track.new_audio",
        "file.import_midi",
        "media.import",
        "file.save",
    ] {
        assert_eq!(output(id, &[]), None, "{id}");
    }
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
        .filter(|c| c.class != Class::Refused)
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
    let mut five = PREVIEWABLE.to_vec();
    five.sort_unstable();
    assert_eq!(previewable, five);
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
