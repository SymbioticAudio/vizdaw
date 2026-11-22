//! Swift `EditorModelTests` (selection, edits, layout) and the editor
//! commands' gesture refusal.

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use core_control::registry::ArgValue;
use support::{FakeSession, block_row, editor_fake};
use vizdaw_app::editor::{EditorLayout, LightMode, Tool};
use vizdaw_app::{DocSession, DocumentModel, commands};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::selection::{SelectionRow, TickRange};

fn model(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn delete(ids: &[u64]) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Delete {
            events: ids.to_vec(),
        },
    }
}

fn held(f: &FakeSession) -> SelectionRow {
    DocSession::selection(f).unwrap()
}

/// The next command returns the fake's snapshot changed by `change`, one
/// version on.
fn next_snapshot(f: &FakeSession, change: impl FnOnce(&mut vizdaw_ffi::snapshot::DocSnapshot)) {
    let mut s = f.snap();
    s.version += 1;
    change(&mut s);
    f.bump(s);
}

#[test]
fn selecting_a_block_selects_its_events_lane_and_range() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.select_block(31, &block_row(0, 100, &[1, 2]), false);
    let s = held(&f);
    assert_eq!(s.events, [1, 2]);
    assert_eq!(s.lanes, [LaneRef { seq: 3, lane: 31 }]);
    assert_eq!(s.time_range, Some(TickRange { start: 0, end: 100 }));
    m.select_block(30, &block_row(200, 300, &[3]), true);
    let s = held(&f);
    let mut events = s.events.clone();
    events.sort();
    assert_eq!(events, [1, 2, 3]);
    assert_eq!(s.time_range, Some(TickRange { start: 0, end: 300 }));
    assert!(m.is_selected(&block_row(0, 100, &[1, 2])));
    assert!(
        !m.is_selected(&block_row(0, 100, &[])),
        "an empty block never is"
    );
    m.clear_selection();
    let s = held(&f);
    assert!(s.events.is_empty() && s.time_range.is_none());
    assert!(!m.is_selected(&block_row(0, 100, &[1, 2])));
}

#[test]
fn a_zero_length_block_selects_one_tick() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.select_block(31, &block_row(50, 50, &[1]), false);
    assert_eq!(held(&f).time_range, Some(TickRange { start: 50, end: 51 }));
}

#[test]
fn a_ruler_range_selects_every_shown_lane() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.select_range(TickRange {
        start: 0,
        end: 61_440,
    });
    let s = held(&f);
    assert_eq!(
        s.lanes,
        [LaneRef { seq: 3, lane: 30 }, LaneRef { seq: 3, lane: 31 }]
    );
    assert!(s.events.is_empty());
}

#[test]
fn an_edit_applies_its_snapshot_and_refreshes_the_selection() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    let before = m.snapshot().version;
    f.force_selection(SelectionRow {
        time_range: None,
        lanes: vec![],
        events: vec![1],
    });
    assert!(m.edit(delete(&[1])));
    assert_eq!(m.snapshot().version, before + 1);
    assert_eq!(m.selection().events, [1]);
    assert!(m.is_event_selected(1));
    assert_eq!(f.edits(), [delete(&[1])]);
}

#[test]
fn a_refused_edit_shows_a_status() {
    let f = Arc::new(editor_fake());
    f.set_edit_error(Some(VizError::Model {
        message: "would make a cycle".into(),
    }));
    let mut m = model(&f);
    assert!(!m.edit(delete(&[1])));
    assert_eq!(m.status(), Some("would make a cycle"));
    assert!(m.alert.is_none());
    m.set_status(None);
    assert_eq!(m.status(), None);
}

#[test]
fn edit_all_is_one_call_and_one_command_is_a_plain_edit() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    assert!(m.edit_all(vec![delete(&[1]), delete(&[2])], "Delete"));
    assert_eq!(
        f.edit_alls(),
        [("Delete".to_string(), vec![delete(&[1]), delete(&[2])])]
    );
    assert!(f.edits().is_empty());
    assert!(m.edit_all(vec![delete(&[3])], "Delete"));
    assert_eq!(f.edits(), [delete(&[3])]);
    assert!(m.edit_all(vec![], "Delete"), "nothing to do is done");
    f.set_call_error(
        "edit_all",
        Some(VizError::Model {
            message: "the first tempo stays".into(),
        }),
    );
    let version = m.snapshot().version;
    assert!(!m.edit_all(vec![delete(&[1]), delete(&[2])], "Delete"));
    assert_eq!(m.status(), Some("the first tempo stays"));
    assert_eq!(m.snapshot().version, version);
}

#[test]
fn a_refused_selection_is_rolled_back() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    let before = m.selection().clone();
    f.set_selection_error(Some(VizError::Model {
        message: "no".into(),
    }));
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![],
        events: vec![1],
    });
    assert_eq!(*m.selection(), before);
    assert!(!m.is_event_selected(1));
    assert_eq!(m.status(), Some("no"));
}

#[test]
fn a_refused_time_range_is_rolled_back_too() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    f.set_selection_error(Some(VizError::Model {
        message: "no".into(),
    }));
    m.set_time_range(TickRange { start: 0, end: 10 });
    assert_eq!(m.selection().time_range, None);
    assert_eq!(m.status(), Some("no"));
    assert!(m.alert.is_none());
}

#[test]
fn the_selection_is_what_the_session_kept() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    // Rust drops unknown events (retain_existing): event 99 does not exist.
    f.set_keep_selection(Some(Box::new(|mut s: SelectionRow| {
        s.events.retain(|e| *e != 99);
        s
    })));
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![],
        events: vec![1, 99],
    });
    assert_eq!(m.selection().events, [1]);
    assert!(m.is_event_selected(1));
    assert!(!m.is_event_selected(99));
    assert_eq!(m.status(), None);
}

#[test]
fn edit_warnings_show_in_the_status() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    let note = "Crossfade removed: the region no longer starts inside the one before it.";
    next_snapshot(&f, |s| s.edit_notes = vec![note.into()]);
    assert!(m.edit(delete(&[1])));
    assert_eq!(m.status(), Some(note));
    m.set_status(Some("Earlier.".into()));
    next_snapshot(&f, |s| s.edit_notes = vec![]);
    assert!(m.edit(delete(&[1])));
    assert_eq!(m.status(), Some("Earlier."));
}

#[test]
fn edits_and_steps_wait_for_an_open_drag() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    assert!(!m.gesture_open());
    assert!(m.begin_gesture("Move Block", vec![(3, 31)]));
    assert!(!m.edit(delete(&[1])));
    assert_eq!(m.status(), Some("Finish the drag first."));
    m.set_status(None);
    assert!(!m.edit_all(vec![delete(&[1]), delete(&[2])], "Delete"));
    assert_eq!(m.status(), Some("Finish the drag first."));
    assert!(f.edits().is_empty() && f.edit_alls().is_empty());

    // Undo and Redo: the menu item is disabled, the channel and the key
    // are refused.
    next_snapshot(&f, |s| {
        s.undo_label = Some("Delete Events".into());
        s.redo_label = Some("Delete Events".into());
    });
    m.end_gesture(None);
    assert!(m.edit(delete(&[1])));
    assert!(m.begin_gesture("Move Block", vec![(3, 31)]));
    for id in ["edit.undo", "edit.redo"] {
        assert!(!commands::enabled(id, &m), "{id}");
        let e = commands::run(id, &[], &mut m).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert_eq!(e.message, "Finish the drag first.");
        m.set_status(None);
        let e = commands::perform_from_menu(id, &mut m).unwrap_err();
        assert_eq!(e.message, "Finish the drag first.");
        assert_eq!(m.status(), Some("Finish the drag first."));
    }
    m.undo();
    m.redo();
    assert_eq!(f.count("undo") + f.count("redo"), 0);
    m.end_gesture(None);
    assert!(commands::enabled("edit.undo", &m));
    commands::run("edit.undo", &[], &mut m).unwrap();
    assert_eq!(f.count("undo"), 1);
}

#[test]
fn nothing_to_undo_is_said_before_the_drag() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    assert!(m.begin_gesture("Move Block", vec![(3, 31)]));
    let e = commands::run("edit.undo", &[], &mut m).unwrap_err();
    assert_eq!(e.message, "Nothing to undo.");
}

#[test]
fn the_tool_starts_as_the_arrow() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    assert_eq!(m.tool(), Tool::Arrow);
    m.set_tool(Tool::Eraser);
    assert_eq!(m.tool(), Tool::Eraser);
    let titles: Vec<_> = Tool::ALL.iter().map(|t| t.title()).collect();
    assert_eq!(titles, ["Arrow", "Pencil", "Eraser"]);
    assert_eq!(Tool::Pencil.name(), "pencil");
}

#[test]
fn the_snap_command_sets_or_toggles() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    assert!(m.layout().snap.enabled);
    assert!(commands::enabled("editors.snap", &m));
    commands::run("editors.snap", &[], &mut m).unwrap();
    assert!(!m.layout().snap.enabled, "no `on` toggles");
    let on = |v| vec![("on".to_string(), ArgValue::Flag(v))];
    commands::run("editors.snap", &on(false), &mut m).unwrap();
    assert!(!m.layout().snap.enabled);
    commands::run("editors.snap", &on(true), &mut m).unwrap();
    assert!(m.layout().snap.enabled);
    commands::perform_from_menu("editors.snap", &mut m).unwrap();
    assert!(!m.layout().snap.enabled);
    let bad = vec![("on".to_string(), ArgValue::Int(1))];
    let e = commands::run("editors.snap", &bad, &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[test]
fn the_layout_is_stored_on_save_and_read_at_open() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.update_layout(|l| {
        l.snap.enabled = false;
        l.lane_height = 60.;
        l.light_mode = LightMode::On;
    });
    assert_eq!(f.count("set_ui_layout"), 0, "a change is written on save");
    m.save_as(std::path::Path::new("/tmp/x/a.vizdaw")).unwrap();
    let calls = f.calls();
    let stored = calls.iter().position(|c| c == "set_ui_layout").unwrap();
    let saved = calls
        .iter()
        .position(|c| c.starts_with("save_as("))
        .unwrap();
    assert!(stored < saved, "stored before the song is written");
    let written = EditorLayout::decode(f.layout().as_deref());
    assert!(!written.snap.enabled);
    assert_eq!(written.lane_height, 60.);
    let reopened = model(&f);
    assert_eq!(reopened.layout(), m.layout());

    // Save in place stores it too.
    m.update_layout(|l| l.follow = false);
    assert_eq!(m.save(), vizdaw_app::SaveAction::Saved);
    assert!(!EditorLayout::decode(f.layout().as_deref()).follow);
}

#[test]
fn layout_decoding_is_tolerant() {
    assert_eq!(EditorLayout::decode(None), EditorLayout::default());
    assert_eq!(
        EditorLayout::decode(Some("garbage")),
        EditorLayout::default()
    );
    assert_eq!(EditorLayout::decode(Some("[1,2]")), EditorLayout::default());
    let partial = EditorLayout::decode(Some(r#"{"version":1,"lightMode":"on","future":42}"#));
    assert_eq!(partial.light_mode, LightMode::On);
    assert_eq!(partial.snap, Default::default());
    let wrong = EditorLayout::decode(Some(
        r#"{"follow":"yes","lastKind":"mixer","snap":{"division":"bar"},"lightMode":"off"}"#,
    ));
    let d = EditorLayout::default();
    assert_eq!(wrong.follow, d.follow);
    assert_eq!(wrong.last_kind, d.last_kind);
    assert_eq!(wrong.snap, d.snap, "a partial snap is the default");
    assert_eq!(wrong.light_mode, LightMode::Off);
    let mut l = EditorLayout {
        detail: Some(vizdaw_app::editor::DetailState::new(
            vizdaw_app::editor::DetailEditorKind::List,
            vizdaw_app::editor::EditorTarget::Block {
                seq: 3,
                lane: 31,
                start: 0,
                end: 10,
            },
        )),
        ..EditorLayout::default()
    };
    l.snap.triplet = true;
    assert_eq!(EditorLayout::decode(Some(&l.encoded())), l);
}

#[test]
fn corrupt_lane_height_and_zoom_fall_back_or_clamp() {
    let d = EditorLayout::default();
    let zero = EditorLayout::decode(Some(r#"{"laneHeight":0,"ticksPerPoint":0}"#));
    assert_eq!(zero.lane_height, d.lane_height);
    assert_eq!(zero.ticks_per_point, d.ticks_per_point);
    let negative = EditorLayout::decode(Some(r#"{"laneHeight":-5,"ticksPerPoint":-1}"#));
    assert_eq!(negative.lane_height, d.lane_height);
    assert_eq!(negative.ticks_per_point, d.ticks_per_point);
    let huge = EditorLayout::decode(Some(r#"{"laneHeight":1e300,"ticksPerPoint":0.001}"#));
    assert_eq!(huge.lane_height, *EditorLayout::LANE_HEIGHT_RANGE.end());
    assert_eq!(
        huge.ticks_per_point,
        *EditorLayout::TICKS_PER_POINT_RANGE.start()
    );
    // JSON has no NaN literal; a string or a non-conforming value falls back.
    let nan = EditorLayout::decode(Some(r#"{"laneHeight":"NaN","ticksPerPoint":NaN}"#));
    assert_eq!(nan.lane_height, d.lane_height);
    assert_eq!(nan.ticks_per_point, d.ticks_per_point);
    let r = EditorLayout::LANE_HEIGHT_RANGE;
    assert_eq!(EditorLayout::sane(f64::NAN, &r, 48.), 48.);
    assert_eq!(
        EditorLayout::sane(f64::INFINITY, &EditorLayout::TICKS_PER_POINT_RANGE, 384.),
        384.
    );
    assert_eq!(EditorLayout::sane(0., &r, 48.), 48.);
    assert_eq!(EditorLayout::sane(60., &r, 48.), 60.);
    assert_eq!(d.lane_height, 48.);
    assert_eq!(d.ticks_per_point, 384.);
}

#[test]
fn saving_keeps_ui_json_keys_this_app_does_not_know() {
    let original = serde_json::json!({
        "version": 1,
        "pianoRoll": {"velocityLane": true, "keyWidth": 40},
        "windows": [{"id": "6F9619FF-8B86-D011-B42D-00C04FC964FF",
                     "detail": {"kind": "pianoRoll", "target": {"lane": {"seq": 3, "lane": 30}}},
                     "frame": [1, 2, 3, 4]}],
        "controllers": {"3:30": [{"kind": "velocity"}]},
        "sequenceOrder": [2, 1],
        "transport": {"punch": true},
        "detail": {"kind": "pianoRoll", "target": {"lane": {"seq": 3, "lane": 31}},
                   "lanes": [30], "stripRows": [{"cc": 7}]},
        "views": {"main": "arrange",
                  "arrange": {"left": {"shown": false, "view": "sequences", "size": 300},
                              "bottom": {"shown": true, "view": "editor", "size": 400},
                              "right": {"shown": true, "view": "markers", "size": 200}},
                  "frame": [10, 20, 800, 600]},
        "snap": {"division": "eighth", "triplet": false, "dotted": false, "enabled": true}
    });
    let f = Arc::new(editor_fake());
    f.set_layout(Some(&original.to_string()));
    let mut m = model(&f);
    assert_eq!(f.count("set_ui_layout"), 0, "opening writes nothing");
    let d = m.layout().detail.clone().unwrap();
    assert_eq!(
        d.target,
        vizdaw_app::editor::EditorTarget::Lane { seq: 3, lane: 31 }
    );
    assert_eq!(m.layout().views.arrange.bottom.size, 400.);
    m.update_layout(|l| l.snap.enabled = false);
    assert_eq!(m.save(), vizdaw_app::SaveAction::NeedsSaveAs);
    m.save_as(std::path::Path::new("/tmp/x/a.vizdaw")).unwrap();
    let text = f.layout().unwrap();
    let written: serde_json::Value = serde_json::from_str(&text).unwrap();
    for key in [
        "version",
        "pianoRoll",
        "windows",
        "controllers",
        "sequenceOrder",
        "transport",
    ] {
        assert_eq!(written[key], original[key], "{key}");
    }
    assert_eq!(written["detail"]["lanes"], original["detail"]["lanes"]);
    assert_eq!(
        written["detail"]["stripRows"],
        original["detail"]["stripRows"]
    );
    assert_eq!(written["snap"]["enabled"], false);
    assert_eq!(written["snap"]["division"], "eighth");
    assert_eq!(written["views"]["arrange"]["right"]["view"], "markers");
    assert_eq!(
        written["views"]["frame"],
        serde_json::json!([10., 20., 800., 600.])
    );
    // Sorted keys, as Swift writes them.
    let keys: Vec<_> = written.as_object().unwrap().keys().cloned().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let at = |k: &str| text.find(&format!("\"{k}\"")).unwrap();
    assert!(at("blockRule") < at("controllers") && at("controllers") < at("detail"));

    // Closing the detail removes the key (Swift omits a nil detail).
    m.close_detail();
    assert_eq!(m.save(), vizdaw_app::SaveAction::Saved);
    let written: serde_json::Value = serde_json::from_str(&f.layout().unwrap()).unwrap();
    assert!(written.get("detail").is_none());
    assert_eq!(written["pianoRoll"], original["pianoRoll"]);
}

#[test]
fn a_damaged_ui_json_opens_with_defaults_and_is_replaced_on_save() {
    let f = Arc::new(editor_fake());
    f.set_layout(Some("{not json"));
    let mut m = model(&f);
    assert_eq!(*m.layout(), EditorLayout::default());
    assert_eq!(
        f.layout().as_deref(),
        Some("{not json"),
        "kept until a save"
    );
    m.save_as(std::path::Path::new("/tmp/x/a.vizdaw")).unwrap();
    let written: serde_json::Value = serde_json::from_str(&f.layout().unwrap()).unwrap();
    assert_eq!(written["laneHeight"], 48.);
    assert_eq!(written["lastKind"], "pianoRoll");
}

#[test]
fn merging_writes_the_swift_keys() {
    let json = EditorLayout::default().merge_into(Some(r#"{"future":42,"detail":{"x":1}}"#));
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["future"], 42);
    assert!(v.get("detail").is_none());
    for key in [
        "snap",
        "ticksPerPoint",
        "laneHeight",
        "blockRule",
        "lightMode",
        "follow",
        "lastKind",
        "views",
    ] {
        assert!(v.get(key).is_some(), "{key}");
    }
    assert_eq!(
        v["blockRule"],
        serde_json::json!({"silenceGap": {"beats": 4}})
    );
    assert_eq!(v["lightMode"], "auto");
    assert_eq!(v["views"]["main"], "arrange");
    assert_eq!(
        v["views"]["arrange"]["bottom"],
        serde_json::json!({"shown": true, "view": "editor", "size": 260.})
    );
    assert_eq!(
        v["views"]["arrange"]["left"],
        serde_json::json!({"shown": true, "view": "strip", "size": 220.})
    );
    assert_eq!(v["views"]["mix"]["right"]["view"], "plugins");
    assert!(v["views"].get("frame").is_none());
    // A damaged existing layout is replaced.
    let fresh = EditorLayout::default().merge_into(Some("[]"));
    assert!(
        serde_json::from_str::<serde_json::Value>(&fresh)
            .unwrap()
            .is_object()
    );
}

#[test]
fn views_decode_tolerantly_and_clamp() {
    let l = EditorLayout::decode(Some(
        r#"{"views":{"main":"mix","arrange":{"bottom":{"size":5000,"view":"strip"},"left":{"shown":false}},"frame":[1,2,3]}}"#,
    ));
    let v = &l.views;
    assert_eq!(v.main, vizdaw_app::editor::MainView::Mix);
    assert_eq!(v.arrange.bottom.size, 900.);
    assert_eq!(
        v.arrange.bottom.view,
        vizdaw_app::editor::PaneView::Editor,
        "off-list views go back to the default"
    );
    assert!(!v.arrange.left.shown);
    assert_eq!(v.arrange.left.size, 220., "the other fields stay default");
    assert!(v.frame.is_none());
}

#[test]
fn the_fake_records_the_gesture_calls() {
    let f = editor_fake();
    f.begin_edit("Move Block").unwrap();
    assert!(matches!(
        DocSession::edit(&f, delete(&[1])),
        Err(VizError::Busy { .. })
    ));
    f.edit_gesture(delete(&[1])).unwrap();
    let s = f.end_edit().unwrap();
    assert_eq!(s.undo_label.as_deref(), Some("Move Block"));
    assert_eq!(f.gesture_edits(), [delete(&[1])]);
    f.fail_next(
        "begin_edit",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    assert!(f.begin_edit("Copy Block").is_err());
    f.begin_edit("Copy Block").unwrap();
    f.abort_edit().unwrap();
    assert!(f.edit_gesture(delete(&[1])).is_err(), "no edit is open");
    assert_eq!(f.seconds_at(3, vec![30_720]).unwrap(), [1.0]);
    assert_eq!(f.ticks_at(3, vec![0.5]).unwrap(), [15_360]);
    assert!(f.set_ui_layout("nope".into()).is_err());
    assert!(f.calls().contains(&"begin_edit(Move Block)".to_string()));
    assert_eq!(f.count("abort_edit"), 1);
}

#[test]
fn saving_keeps_nested_keys_this_app_does_not_know() {
    let original = serde_json::json!({
        "views": {"main": "arrange", "future": {"a": 1},
                  "arrange": {"future": 2,
                              "left": {"shown": true, "view": "strip", "size": 220, "future": 3}}},
        "snap": {"division": "quarter", "triplet": false, "dotted": false, "enabled": true,
                 "future": 4},
        "detail": {"kind": "list", "target": {"lane": {"seq": 3, "lane": 31}},
                   "listFilter": {"hiddenKinds": ["cc"], "shownDests": [2], "future": 5}},
        "blockRule": {"silenceGap": {"beats": 4}}
    });
    let f = Arc::new(editor_fake());
    f.set_layout(Some(&original.to_string()));
    let mut m = model(&f);
    m.update_layout(|l| {
        l.views.main = vizdaw_app::editor::MainView::Mix;
        l.views.arrange.left.size = 300.;
        l.snap.enabled = false;
        let filter = l.detail.as_mut().unwrap().list_filter.as_mut().unwrap();
        filter.hidden_kinds.insert(vizdaw_app::list::ListKind::Note);
        filter.shown_dests = None;
        l.block_rule = vizdaw_app::editor::BlockRuleSetting::FixedBars { bars: 2 };
    });
    m.save_as(std::path::Path::new("/tmp/x/a.vizdaw")).unwrap();
    let w: serde_json::Value = serde_json::from_str(&f.layout().unwrap()).unwrap();
    assert_eq!(w["views"]["future"], serde_json::json!({"a": 1}));
    assert_eq!(w["views"]["main"], "mix");
    assert_eq!(w["views"]["arrange"]["future"], 2);
    assert_eq!(w["views"]["arrange"]["left"]["future"], 3);
    assert_eq!(w["views"]["arrange"]["left"]["size"], 300.);
    assert_eq!(w["snap"]["future"], 4);
    assert_eq!(w["snap"]["enabled"], false);
    let lf = &w["detail"]["listFilter"];
    assert_eq!(lf["future"], 5);
    assert_eq!(lf["hiddenKinds"], serde_json::json!(["cc", "note"]));
    assert!(
        lf.get("shownDests").is_none(),
        "a modelled optional that went is gone"
    );
    assert_eq!(w["blockRule"], serde_json::json!({"fixedBars": {"_0": 2}}));
}
