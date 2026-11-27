//! `surface.*` from the control channel (Swift `SurfaceCommandsTests`,
//! plan 5f-1 Task 9): every argument is checked before the core or the
//! session is called, and the process-wide calls need no document.

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use core_control::registry::ArgValue;
use support::{FakeSession, demo_fake, editor_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::GESTURE_OPEN;
use vizdaw_app::surface_commands::{SURFACE_COMMANDS, parse_kind, run_surface};
use vizdaw_app::testing::FakeCore;
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::surfaces::SurfaceKindRow;

fn text(name: &str, v: &str) -> (String, ArgValue) {
    (name.into(), ArgValue::Text(v.into()))
}

fn int(name: &str, v: i64) -> (String, ArgValue) {
    (name.into(), ArgValue::Int(v))
}

fn open() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

#[test]
fn surface_commands_check_their_arguments() {
    let (f, mut m) = open();
    let c = FakeCore::new();
    let many = vec!["00"; 1025].join(" ");
    let rows: Vec<(&str, Vec<(String, ArgValue)>)> = vec![
        (
            "surface.inject",
            vec![int("surface", 1), text("bytes", "9g")],
        ),
        ("surface.inject", vec![int("surface", 1), text("bytes", "")]),
        ("surface.learn", vec![]),
        (
            "surface.learn",
            vec![text("target", "fader:1"), text("command", "edit.undo")],
        ),
        ("surface.learn", vec![text("target", "fader")]),
        ("surface.unlearn", vec![]),
        (
            "surface.unlearn",
            vec![text("target", "fader:1"), text("command", "edit.undo")],
        ),
        ("surface.inject", vec![text("bytes", "90 68 7f")]),
        (
            "surface.inject",
            vec![text("surface", "1"), text("bytes", "90 68 7f")],
        ),
        (
            "surface.inject",
            vec![int("surface", -1), text("bytes", "90 68 7f")],
        ),
        (
            "surface.inject",
            vec![int("surface", 4_294_967_296), text("bytes", "90 68 7f")],
        ),
        ("surface.inject", vec![int("surface", 1)]),
        (
            "surface.inject",
            vec![int("surface", 1), text("bytes", "9")],
        ),
        (
            "surface.inject",
            vec![int("surface", 1), text("bytes", "906")],
        ),
        (
            "surface.inject",
            vec![int("surface", 1), text("bytes", &many)],
        ),
        ("surface.learn", vec![text("command", "")]),
        ("surface.learn", vec![int("target", 3)]),
        ("surface.unlearn", vec![text("command", "")]),
        (
            "surface.add",
            vec![
                text("type", "mackie"),
                text("input", "In A"),
                text("output", "Out A"),
            ],
        ),
        (
            "surface.add",
            vec![text("input", "In A"), text("output", "Out A")],
        ),
        (
            "surface.add",
            vec![text("type", "mcu"), text("input", "In A")],
        ),
        (
            "surface.add",
            vec![
                text("type", "mcu"),
                int("input", 7),
                text("output", "Out A"),
            ],
        ),
        (
            "surface.add",
            vec![
                text("type", "mcu"),
                text("input", "In A"),
                text("output", "Out A"),
                int("position", 4),
            ],
        ),
        (
            "surface.add",
            vec![
                text("type", "mcu"),
                text("input", "In A"),
                text("output", "Out A"),
                int("position", -1),
            ],
        ),
        (
            "surface.add",
            vec![
                text("type", "mcu"),
                text("input", "In A"),
                text("output", "Out A"),
                text("position", "2"),
            ],
        ),
        ("surface.remove", vec![]),
        ("surface.remove", vec![int("surface", -1)]),
        ("surface.remove", vec![text("surface", "0")]),
    ];
    for (id, args) in &rows {
        let e = run_surface(id, args, Some(&mut m), &c).expect_err(&format!("{id} {args:?} ran"));
        assert_eq!(e.code, ErrorCode::InvalidParams, "{id} {args:?}: {e:?}");
        assert!(e.message.starts_with(&format!("{id}: ")), "{}", e.message);
    }
    assert!(f.surface_injections().is_empty());
    assert!(f.learn_targets().is_empty() && f.unlearn_targets().is_empty());
    assert_eq!(f.focus_calls(), 0);
    assert!(c.learned_commands().is_empty() && c.removed_command_maps().is_empty());
    assert!(c.surfaces_added().is_empty() && c.surfaces_removed().is_empty());
}

#[test]
fn the_messages_are_swifts() {
    let (_, mut m) = open();
    let c = FakeCore::new();
    let message = |id: &str, args: &[(String, ArgValue)], m: &mut DocumentModel| {
        run_surface(id, args, Some(m), &c).unwrap_err().message
    };
    assert_eq!(
        message("surface.add", &[], &mut m),
        "surface.add: `type` is mcu, mcu_xt, hui or kontrol_mk3"
    );
    assert_eq!(
        message("surface.add", &[text("type", "hui")], &mut m),
        "surface.add: `input` and `output` name the MIDI ports"
    );
    assert_eq!(
        message(
            "surface.add",
            &[
                text("type", "mcu"),
                text("input", "a"),
                text("output", "b"),
                int("position", 9)
            ],
            &mut m
        ),
        "surface.add: `position` is 0 to 3"
    );
    assert_eq!(
        message("surface.remove", &[], &mut m),
        "surface.remove: `surface` is a surface ID"
    );
    assert_eq!(
        message("surface.inject", &[int("surface", 0)], &mut m),
        "surface.inject: `bytes` are 1 to 1024 hex bytes such as \"90 68 7f\""
    );
    assert_eq!(
        message("surface.inject", &[], &mut m),
        "surface.inject: `surface` is a surface ID"
    );
    assert_eq!(
        message("surface.learn", &[], &mut m),
        "surface.learn: give exactly one of `target` and `command`"
    );
    assert_eq!(
        message("surface.unlearn", &[text("target", "x")], &mut m),
        "surface.unlearn: `target` is fader:<strip>, pan:<strip>, mute:<strip>, \
         solo:<strip>, send:<strip>:<n> (n from 1), param:<slot>:<param> or vca:<vca>"
    );
    assert_eq!(
        message("surface.learn", &[text("command", "")], &mut m),
        "surface.learn: `command` is a registry command ID"
    );
}

#[test]
fn inject_and_learn_reach_the_session() {
    let (f, mut m) = open();
    let c = FakeCore::new();
    run_surface(
        "surface.inject",
        &[int("surface", 2), text("bytes", "90 68 7f e0 00 40")],
        Some(&mut m),
        &c,
    )
    .unwrap();
    assert_eq!(
        f.surface_injections(),
        vec![(2, vec![0x90, 0x68, 0x7f, 0xe0, 0x00, 0x40])]
    );
    run_surface(
        "surface.inject",
        &[int("surface", 2), text("bytes", "90 68")],
        Some(&mut m),
        &c,
    )
    .unwrap();
    assert_eq!(
        f.surface_injections().len(),
        2,
        "a cut message is the core's to count"
    );
    run_surface(
        "surface.learn",
        &[text("target", "pan:3")],
        Some(&mut m),
        &c,
    )
    .unwrap();
    assert_eq!(
        f.learn_targets(),
        vec![AutomationTargetRow::StripPan { strip: 3 }]
    );
    let focused = f.focus_calls();
    run_surface(
        "surface.learn",
        &[text("command", "edit.undo")],
        Some(&mut m),
        &c,
    )
    .unwrap();
    assert_eq!(c.learned_commands(), vec!["edit.undo".to_owned()]);
    assert_eq!(
        f.focus_calls(),
        focused + 1,
        "the learn completes in this window's session"
    );
    run_surface(
        "surface.unlearn",
        &[text("target", "pan:3")],
        Some(&mut m),
        &c,
    )
    .unwrap();
    assert_eq!(
        f.unlearn_targets(),
        vec![AutomationTargetRow::StripPan { strip: 3 }]
    );
    run_surface(
        "surface.unlearn",
        &[text("command", "edit.undo")],
        Some(&mut m),
        &c,
    )
    .unwrap();
    assert_eq!(c.removed_command_maps(), vec!["edit.undo".to_owned()]);
    c.fail_learn_command(VizError::Model {
        message: "\u{201c}automation.gesture\u{201d} cannot be learned".into(),
    });
    let e = run_surface(
        "surface.learn",
        &[text("command", "automation.gesture")],
        Some(&mut m),
        &c,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed, "a core refusal is the call's");
    c.fail_learn_command(VizError::Busy {
        message: "busy".into(),
    });
    let e = run_surface(
        "surface.learn",
        &[text("command", "edit.undo")],
        Some(&mut m),
        &c,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
}

#[test]
fn add_and_remove_need_no_document() {
    let c = FakeCore::new();
    let r = run_surface(
        "surface.add",
        &[
            text("type", "mcu_xt"),
            text("input", "In A"),
            text("output", "Out A"),
            int("position", 2),
        ],
        None,
        &c,
    )
    .unwrap();
    assert_eq!(r.status.as_deref(), Some("surface 0"), "the ID in `status`");
    assert_eq!(r.undo, None);
    assert_eq!(
        c.surfaces_added(),
        vec![(
            SurfaceKindRow::McuXt,
            "In A".into(),
            "Out A".into(),
            Some(2)
        )]
    );
    run_surface("surface.remove", &[int("surface", 0)], None, &c).unwrap();
    assert_eq!(c.surfaces_removed(), vec![0]);
    let e = run_surface(
        "surface.inject",
        &[int("surface", 0), text("bytes", "90 68 7f")],
        None,
        &c,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NoDocument);
    assert_eq!(e.message, "No document is open.");
    // Arguments are checked before the document is looked for.
    let e = run_surface("surface.learn", &[], None, &c).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(parse_kind("hui"), Some(SurfaceKindRow::Hui));
    assert_eq!(parse_kind("mcu"), Some(SurfaceKindRow::Mcu));
    assert_eq!(parse_kind("mackie"), None);
    assert_eq!(
        SURFACE_COMMANDS,
        [
            "surface.add",
            "surface.remove",
            "surface.inject",
            "surface.learn",
            "surface.unlearn"
        ]
    );
    // A core refusal of the add is the call's failure.
    c.fail_surface_add(VizError::Model {
        message: "taken".into(),
    });
    let e = run_surface(
        "surface.add",
        &[text("type", "mcu"), text("input", "a"), text("output", "b")],
        None,
        &c,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
}

/// Plan 5f-4 Rulings 2 and 11: a keyboard and a HUI unit are banks of
/// their own, added without a position.
#[test]
fn a_kontrol_is_added_without_a_position() {
    assert_eq!(parse_kind("kontrol_mk3"), Some(SurfaceKindRow::KontrolMk3));
    let c = FakeCore::new();
    for kind in ["kontrol_mk3", "hui"] {
        run_surface(
            "surface.add",
            &[
                text("type", kind),
                text("input", kind),
                text("output", kind),
                int("position", 3),
            ],
            None,
            &c,
        )
        .unwrap();
    }
    let added = c.surfaces_added();
    assert_eq!(added[0].0, SurfaceKindRow::KontrolMk3);
    assert_eq!(added[0].3, None, "no position is passed");
    assert_eq!(added[1].0, SurfaceKindRow::Hui);
    assert_eq!(added[1].3, None);
}

#[test]
fn unlearn_waits_for_an_open_gesture() {
    let f = Arc::new(editor_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    assert!(m.begin_gesture("Move Block", vec![(3, 31)]));
    let c = FakeCore::new();
    let e = run_surface(
        "surface.unlearn",
        &[text("target", "fader:1")],
        Some(&mut m),
        &c,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, GESTURE_OPEN);
    assert!(f.unlearn_targets().is_empty());
    m.end_gesture(None);
    run_surface(
        "surface.unlearn",
        &[text("target", "fader:1")],
        Some(&mut m),
        &c,
    )
    .unwrap();
    assert_eq!(
        f.unlearn_targets(),
        vec![AutomationTargetRow::StripFader { strip: 1 }]
    );
}
