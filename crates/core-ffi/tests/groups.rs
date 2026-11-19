//! Fader groups through a session (groups-vcas-transport Task 2): the
//! seven group commands through `run_mixer`, `mixer.set_strip` moving the
//! group (or one strip, alone), linked mute and solo, and the gesture
//! fan-out every hand shares: the mouse, the channel and the surfaces.
//! A group drag is one undo step "Move Group ‹name›" while stopped; while
//! a pass writes, each member takes its own strip's path. Review Focus 1
//! and 2: the balance survives a drag past the top and back, and a member
//! another hand holds stays with that hand.
//!
//! No plug-in loads: the strips are audio tracks' with no EQ. Offline only.

mod common;

use core_model::automation::{fader_db, fader_norm};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::automation::{
    AutomationModeRow as Mode, GestureHolderRow as Holder, GesturePhaseRow as Phase,
    reset_automation_settings, set_automation_suspended,
};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    AutomationTargetRow as Target, EventRow, GroupRow, Session, StripRow, SwatchRow, TrackKindRow,
    VizError, set_new_channel_eq,
};

static SERIAL: Mutex<()> = Mutex::new(());

const Q: i64 = 15_360;

/// One test at a time; offline; no EQ on new channels, so nothing loads.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    reset_automation_settings();
    set_automation_suspended(false);
    set_new_channel_eq(false);
    g
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn int(name: &str, value: i64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int { value },
    }
}

fn id(name: &str, value: u64) -> ControlArg {
    int(name, value as i64)
}

fn text(name: &str, value: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text {
            value: value.into(),
        },
    }
}

fn flag(name: &str, value: bool) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Flag { value },
    }
}

fn strips(s: &Session) -> Vec<StripRow> {
    s.mixer().unwrap().strips
}

fn strip_row(s: &Session, id: u64) -> StripRow {
    strips(s).into_iter().find(|x| x.id == id).unwrap()
}

fn gain(s: &Session, id: u64) -> f64 {
    strip_row(s, id).gain_db
}

fn strip_id(s: &Session, name: &str) -> u64 {
    strips(s)
        .into_iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("no strip {name}"))
        .id
}

fn groups(s: &Session) -> Vec<GroupRow> {
    s.mixer().unwrap().groups
}

fn undo_label(s: &Session) -> Option<String> {
    s.snapshot().unwrap().undo_label
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Lets the 10 ms sampler pass, and while playing the renderer take a
/// block.
fn pace(s: &Session) {
    let t0 = s.transport().unwrap().tick;
    std::thread::sleep(Duration::from_millis(12));
    if s.transport().unwrap().playing {
        wait_until("the renderer to take a block", || {
            s.transport().unwrap().tick > t0
        });
    }
    s.idle().unwrap();
}

/// A saved song with audio tracks Kick, Snare and Hat (each on its own
/// strip at 0 dB, to master) in the sequence `seq`, grouped as "Drums".
struct Song {
    s: Arc<Session>,
    seq: u64,
    kick: u64,
    snare: u64,
    hat: u64,
    group: u64,
    _dir: tempfile::TempDir,
}

fn song() -> Song {
    let s = Session::new_empty().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let mut ids = Vec::new();
    for (i, name) in ["Kick", "Snare", "Hat"].iter().enumerate() {
        s.new_track(seq, TrackKindRow::Audio, None).unwrap();
        let id = strip_id(&s, &format!("Audio {}", i + 1));
        s.rename_strip(id, (*name).into()).unwrap();
        ids.push(id);
    }
    let list = format!("{},{},{}", ids[0], ids[1], ids[2]);
    s.run_mixer(
        "mixer.new_group".into(),
        vec![text("strips", &list), text("name", "Drums")],
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    s.save_as(
        dir.path()
            .join("Groups.vizdaw")
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap();
    let group = groups(&s)[0].id;
    Song {
        seq,
        kick: ids[0],
        snare: ids[1],
        hat: ids[2],
        group,
        s,
        _dir: dir,
    }
}

fn fader(strip: u64) -> Target {
    Target::StripFader { strip }
}

/// A mouse drag on `t`: touch, the values (paced) and release.
fn drag(s: &Session, t: Target, values: &[f64]) {
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    for v in values {
        pace(s);
        s.gesture(t, Phase::Value { value: *v }).unwrap();
    }
    s.gesture(t, Phase::Release).unwrap();
    s.idle().unwrap();
}

#[test]
fn new_group_from_strips_is_one_step() {
    let _g = serial();
    let s = Session::new_empty().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    for _ in 0..2 {
        s.new_track(seq, TrackKindRow::Audio, None).unwrap();
    }
    let (k, n) = (strip_id(&s, "Audio 1"), strip_id(&s, "Audio 2"));
    s.run_mixer(
        "mixer.new_group".into(),
        vec![text("strips", &format!("{k},{n}"))],
    )
    .unwrap();
    assert_eq!(undo_label(&s).as_deref(), Some("New Group"));
    let g = &groups(&s)[0];
    assert_eq!(g.members, vec![k, n]);
    assert_eq!(g.name, "Group 1", "Ruling 4's default name");
    assert_eq!(g.colour, SwatchRow::Red, "Ruling 4: the first swatch");
    assert!(g.link_mute && g.link_solo);
    assert_eq!(strip_row(&s, k).group, Some(g.id));
    assert_eq!(strip_row(&s, n).group, Some(g.id));
    // The next group takes the next name and swatch; with no strips it is
    // empty (Ruling 11).
    s.run_mixer("mixer.new_group".into(), vec![]).unwrap();
    let second = &groups(&s)[1];
    assert_eq!(
        (second.name.as_str(), second.colour, second.members.len()),
        ("Group 2", SwatchRow::Orange, 0)
    );
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(groups(&s).is_empty(), "one step each");
    assert_eq!(strip_row(&s, k).group, None);
    // `state.get mixer` lists the groups and each strip's.
    s.redo().unwrap();
    let json = vizdaw_ffi::control::result_json(&vizdaw_ffi::control::ControlResult::Mixer {
        doc: "d".into(),
        mixer: s.mixer().unwrap(),
    });
    assert_eq!(json["groups"][0]["name"], "Group 1");
    assert_eq!(json["groups"][0]["colour"], "red");
    assert_eq!(json["groups"][0]["members"], serde_json::json!([k, n]));
    assert_eq!(json["groups"][0]["link_mute"], true);
    let kick = json["strips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == k)
        .unwrap();
    assert_eq!(kick["group"], serde_json::json!(groups(&s)[0].id));
    s.close().unwrap();
}

#[test]
fn every_group_command_runs_as_its_own_step() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    let g = id("group", d.group);
    for (cmd, args, label) in [
        (
            "mixer.rename_group",
            vec![g.clone(), text("name", "  Kit ")],
            "Rename Group",
        ),
        (
            "mixer.set_group_colour",
            vec![g.clone(), text("colour", "teal")],
            "Group Colour",
        ),
        (
            "mixer.remove_from_group",
            vec![g.clone(), text("strips", &d.hat.to_string())],
            "Remove from Group",
        ),
        (
            "mixer.add_to_group",
            vec![g.clone(), text("strips", &d.hat.to_string())],
            "Add to Group",
        ),
        (
            "mixer.set_group_links",
            vec![g.clone(), flag("solo", false)],
            "Group Links",
        ),
    ] {
        s.run_mixer(cmd.into(), args).unwrap();
        assert_eq!(undo_label(s).as_deref(), Some(label), "{cmd}");
    }
    let row = &groups(s)[0];
    assert_eq!(row.name, "Kit");
    assert_eq!(row.colour, SwatchRow::Teal);
    assert_eq!(row.members, vec![d.kick, d.snare, d.hat]);
    assert!(row.link_mute && !row.link_solo);
    s.run_mixer("mixer.delete_group".into(), vec![g]).unwrap();
    assert_eq!(undo_label(s).as_deref(), Some("Delete Group"));
    assert!(groups(s).is_empty());
    assert_eq!(strip_row(s, d.kick).group, None);
    s.close().unwrap();
}

#[test]
fn set_strip_moves_the_group() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    s.run_mixer(
        "mixer.set_strip".into(),
        vec![id("strip", d.kick), text("gain_db", "-3")],
    )
    .unwrap();
    assert_eq!(undo_label(s).as_deref(), Some("Move Group Drums"));
    for x in [d.kick, d.snare, d.hat] {
        assert!(close(gain(s, x), -3.0), "{x}: {}", gain(s, x));
    }
    s.undo().unwrap();
    for x in [d.kick, d.snare, d.hat] {
        assert!(close(gain(s, x), 0.0), "one step: {x}");
    }
    s.run_mixer(
        "mixer.set_strip".into(),
        vec![
            id("strip", d.kick),
            text("gain_db", "-3"),
            flag("alone", true),
        ],
    )
    .unwrap();
    assert_eq!(undo_label(s).as_deref(), Some("Set Strip"));
    assert!(close(gain(s, d.kick), -3.0));
    assert!(close(gain(s, d.snare), 0.0) && close(gain(s, d.hat), 0.0));
    s.close().unwrap();
}

#[test]
fn linked_mute_and_solo_follow_the_switches() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    let set = |args: Vec<ControlArg>| s.run_mixer("mixer.set_strip".into(), args).unwrap();
    set(vec![id("strip", d.kick), flag("mute", true)]);
    assert_eq!(undo_label(s).as_deref(), Some("Set Strip"));
    for x in [d.kick, d.snare, d.hat] {
        assert!(strip_row(s, x).mute, "{x}");
    }
    set(vec![id("strip", d.snare), flag("solo", true)]);
    for x in [d.kick, d.snare, d.hat] {
        assert!(strip_row(s, x).solo, "{x}");
    }
    set(vec![
        id("strip", d.kick),
        flag("mute", false),
        flag("solo", false),
    ]);
    s.run_mixer(
        "mixer.set_group_links".into(),
        vec![id("group", d.group), flag("mute", false)],
    )
    .unwrap();
    set(vec![id("strip", d.kick), flag("mute", true)]);
    assert!(strip_row(s, d.kick).mute);
    assert!(!strip_row(s, d.snare).mute && !strip_row(s, d.hat).mute);
    // Solo is still linked; alone, only the one strip.
    set(vec![
        id("strip", d.hat),
        flag("solo", true),
        flag("alone", true),
    ]);
    assert!(strip_row(s, d.hat).solo);
    assert!(!strip_row(s, d.kick).solo && !strip_row(s, d.snare).solo);
    // A mute gesture goes to the linked members only; solo to all.
    s.run_mixer(
        "mixer.set_group_links".into(),
        vec![id("group", d.group), flag("mute", true)],
    )
    .unwrap();
    let t = Target::StripMute { strip: d.snare };
    s.gesture(t, Phase::Touch { value: Some(0.0) }).unwrap();
    s.gesture(t, Phase::Release).unwrap();
    for x in [d.kick, d.snare, d.hat] {
        assert!(!strip_row(s, x).mute, "{x}");
    }
    assert_eq!(undo_label(s).as_deref(), Some("Mute"));
    s.close().unwrap();
}

#[test]
fn a_fader_gesture_moves_the_group_as_one_step() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    assert!(!s.transport().unwrap().playing);
    drag(
        s,
        fader(d.kick),
        &[fader_norm(-1.0), fader_norm(-2.0), fader_norm(-4.0)],
    );
    for x in [d.kick, d.snare, d.hat] {
        assert!(close(gain(s, x), -4.0), "{x}: {}", gain(s, x));
    }
    assert_eq!(undo_label(s).as_deref(), Some("Move Group Drums"));
    assert!(s.automation_state().unwrap().touched.is_empty());
    s.undo().unwrap();
    for x in [d.kick, d.snare, d.hat] {
        assert!(close(gain(s, x), 0.0), "one undo restores {x}");
    }
    // A channel drag fans out the same way.
    let t = fader(d.hat);
    s.gesture_from(t, Phase::Touch { value: None }, Holder::Channel)
        .unwrap();
    s.gesture_from(
        t,
        Phase::Value {
            value: fader_norm(-6.0),
        },
        Holder::Channel,
    )
    .unwrap();
    s.gesture_from(t, Phase::Release, Holder::Channel).unwrap();
    for x in [d.kick, d.snare, d.hat] {
        assert!(close(gain(s, x), -6.0), "{x}");
    }
    s.close().unwrap();
}

#[test]
fn an_alone_gesture_moves_one_fader() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    let t = fader(d.kick);
    let alone = |phase| s.gesture_as(t, phase, Holder::Mouse, true).unwrap();
    alone(Phase::Touch { value: None });
    alone(Phase::Value {
        value: fader_norm(-5.0),
    });
    alone(Phase::Release);
    assert!(close(gain(s, d.kick), -5.0));
    assert!(close(gain(s, d.snare), 0.0) && close(gain(s, d.hat), 0.0));
    assert_eq!(undo_label(s).as_deref(), Some("Set Fader"));
    s.close().unwrap();
}

/// Review Focus 1: the move is measured from the bases taken at the touch.
#[test]
fn a_group_drag_past_the_top_and_back_keeps_the_balance() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    s.run_mixer(
        "mixer.remove_from_group".into(),
        vec![id("group", d.group), text("strips", &d.hat.to_string())],
    )
    .unwrap();
    s.run_mixer(
        "mixer.set_strip".into(),
        vec![
            id("strip", d.snare),
            text("gain_db", "10"),
            flag("alone", true),
        ],
    )
    .unwrap();
    let t = fader(d.kick);
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    pace(s);
    s.gesture(
        t,
        Phase::Value {
            value: fader_norm(6.0),
        },
    )
    .unwrap();
    assert!(close(gain(s, d.kick), 2.0), "{}", gain(s, d.kick));
    assert!(close(gain(s, d.snare), 12.0), "{}", gain(s, d.snare));
    pace(s);
    s.gesture(
        t,
        Phase::Value {
            value: fader_norm(0.0),
        },
    )
    .unwrap();
    s.gesture(t, Phase::Release).unwrap();
    assert!(close(gain(s, d.kick), 0.0), "{}", gain(s, d.kick));
    assert!(close(gain(s, d.snare), 10.0), "{}", gain(s, d.snare));
    s.close().unwrap();
}

/// Review Focus 2: a member the surface holds is neither moved nor moved
/// twice, and does not jump when the surface lets go.
#[test]
fn a_member_held_by_another_hand_is_not_moved() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    let hat = fader(d.hat);
    // The surface holds Hat alone (a surface touch would take the group
    // with it, as `surface_gestures_move_the_group` shows).
    s.debug_surface_gesture(0, hat, Phase::Touch { value: None }, true)
        .unwrap();
    drag(s, fader(d.kick), &[fader_norm(-2.0), fader_norm(-3.0)]);
    assert!(close(gain(s, d.kick), -3.0));
    assert!(close(gain(s, d.snare), -3.0));
    assert!(close(gain(s, d.hat), 0.0), "held: {}", gain(s, d.hat));
    assert_eq!(
        s.automation_state().unwrap().touched,
        vec![hat],
        "the surface still holds Hat"
    );
    s.debug_surface_gesture(0, hat, Phase::Release, false)
        .unwrap();
    s.idle().unwrap();
    assert!(close(gain(s, d.hat), 0.0), "no jump: {}", gain(s, d.hat));
    assert!(s.automation_state().unwrap().touched.is_empty());
    s.close().unwrap();
}

#[test]
fn surface_gestures_move_the_group() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    let t = fader(d.snare);
    s.debug_surface_gesture(3, t, Phase::Touch { value: None }, false)
        .unwrap();
    s.debug_surface_gesture(
        3,
        t,
        Phase::Value {
            value: fader_norm(-7.0),
        },
        false,
    )
    .unwrap();
    s.debug_surface_gesture(3, t, Phase::Release, false)
        .unwrap();
    for x in [d.kick, d.snare, d.hat] {
        assert!(close(gain(s, x), -7.0), "{x}: {}", gain(s, x));
    }
    assert_eq!(undo_label(s).as_deref(), Some("Move Group Drums"));
    s.close().unwrap();
}

#[test]
fn touched_values_follow_the_followers() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    let t = fader(d.kick);
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    pace(s);
    s.gesture(
        t,
        Phase::Value {
            value: fader_norm(-9.0),
        },
    )
    .unwrap();
    let st = s.automation_state().unwrap();
    assert_eq!(st.touched.len(), 3, "{:?}", st.touched);
    assert_eq!(st.touched_values.len(), st.touched.len());
    for (target, v) in st.touched.iter().zip(&st.touched_values) {
        assert!(close(fader_db(*v), -9.0), "{target:?}: {v}");
    }
    s.gesture(t, Phase::Release).unwrap();
    s.close().unwrap();
}

/// Ruling 5: while a pass writes, each member takes the path its own
/// strip's mode gives: Kick (Touch) writes its lane, Snare (Read, the
/// leader) moves its fader in the step.
#[test]
fn writing_members_write_their_own_lanes() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    s.set_automation_mode(vec![d.kick], Mode::Touch).unwrap();
    s.run_mixer(
        "mixer.remove_from_group".into(),
        vec![id("group", d.group), text("strips", &d.hat.to_string())],
    )
    .unwrap();
    s.play(d.seq).unwrap();
    wait_until("beat 1", || s.transport().unwrap().tick > Q);
    let t = fader(d.snare);
    s.gesture(t, Phase::Touch { value: None }).unwrap();
    for db in [-2.0, -4.0, -6.0] {
        pace(s);
        s.gesture(
            t,
            Phase::Value {
                value: fader_norm(db),
            },
        )
        .unwrap();
    }
    pace(s);
    s.gesture(t, Phase::Release).unwrap();
    let at = s.transport().unwrap().tick;
    wait_until("the glide to pass", || s.transport().unwrap().tick > at + Q);
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    s.idle().unwrap();
    assert!(close(gain(s, d.snare), -6.0), "{}", gain(s, d.snare));
    assert!(close(gain(s, d.kick), 0.0), "Kick writes, its fader stays");
    let lanes = s.lanes(d.seq).unwrap();
    let kick_lane = lanes
        .iter()
        .find(|l| l.name == "Kick Fader")
        .expect("Kick's lane");
    assert!(
        !lanes.iter().any(|l| l.name == "Snare Fader"),
        "no lane for Snare"
    );
    let values: Vec<f64> = s
        .lane_events(d.seq, kick_lane.id)
        .unwrap()
        .events
        .iter()
        .filter_map(|e: &EventRow| e.automation.map(|a| a.value))
        .collect();
    assert!(
        values.iter().any(|v| close(fader_db(*v), -6.0)),
        "{values:?}"
    );
    s.close().unwrap();
}

/// F12: a group move takes each member's own fader as its base.
#[test]
fn followers_move_from_their_own_faders() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    s.run_mixer(
        "mixer.set_strip".into(),
        vec![
            id("strip", d.hat),
            text("gain_db", "-6"),
            flag("alone", true),
        ],
    )
    .unwrap();
    drag(s, fader(d.kick), &[fader_norm(-1.0)]);
    assert!(close(gain(s, d.hat), -7.0), "{}", gain(s, d.hat));
    assert!(close(gain(s, d.snare), -1.0));
    s.close().unwrap();
}

fn refused(r: Result<vizdaw_ffi::DocSnapshot, VizError>) -> String {
    match r {
        Err(VizError::Model { message }) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn run_group_commands_refuse_hostile_input() {
    let _g = serial();
    let d = song();
    let s = &d.s;
    let other = Session::new_empty().unwrap();
    other.run_mixer("mixer.new_group".into(), vec![]).unwrap();
    other.run_mixer("mixer.new_group".into(), vec![]).unwrap();
    let foreign = other.mixer().unwrap().groups[1].id;
    other.close().unwrap();
    let master = strips(s).last().unwrap().id;
    let before = s.debug_document_json();
    let version = s.snapshot().unwrap().version;
    let g = id("group", d.group);
    let many = (1..=65)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let huge = "x".repeat(70_000);
    let cases: Vec<(&str, Vec<ControlArg>)> = vec![
        ("mixer.no_such_group", vec![]),
        (
            "mixer.rename_group",
            vec![int("group", i64::MAX), text("name", "x")],
        ),
        ("mixer.delete_group", vec![id("group", u64::MAX)]),
        ("mixer.delete_group", vec![id("group", foreign)]),
        (
            "mixer.set_group_colour",
            vec![id("group", foreign), text("colour", "red")],
        ),
        ("mixer.new_group", vec![text("strips", "1,1")]),
        ("mixer.new_group", vec![text("strips", "0")]),
        ("mixer.new_group", vec![text("strips", &many)]),
        ("mixer.new_group", vec![text("strips", &master.to_string())]),
        ("mixer.new_group", vec![text("strips", &d.kick.to_string())]),
        ("mixer.new_group", vec![text("strips", "999999")]),
        ("mixer.new_group", vec![text("name", &huge)]),
        ("mixer.new_group", vec![text("name", "Dr\0ums")]),
        ("mixer.new_group", vec![text("name", "   ")]),
        ("mixer.new_group", vec![text("colour", "none")]),
        ("mixer.rename_group", vec![g.clone(), text("name", &huge)]),
        ("mixer.rename_group", vec![g.clone(), text("name", "a\0b")]),
        (
            "mixer.set_group_colour",
            vec![g.clone(), text("colour", "none")],
        ),
        ("mixer.add_to_group", vec![g.clone(), text("strips", "1,1")]),
        (
            "mixer.add_to_group",
            vec![g.clone(), text("strips", &d.kick.to_string())],
        ),
        (
            "mixer.remove_from_group",
            vec![g.clone(), text("strips", "0")],
        ),
        ("mixer.set_group_links", vec![g.clone()]),
        (
            "mixer.set_group_links",
            vec![g.clone(), text("mute", "yes")],
        ),
        ("mixer.delete_group", vec![g.clone(), g.clone()]),
        (
            "mixer.set_strip",
            vec![id("strip", d.kick), flag("alone", true)],
        ),
    ];
    for (cmd, args) in cases {
        let r = s.run_mixer(cmd.into(), args.clone());
        assert!(
            matches!(r, Err(VizError::Model { .. })),
            "{cmd} {args:?}: {r:?}"
        );
        assert_eq!(s.snapshot().unwrap().version, version, "{cmd}: no change");
    }
    assert_eq!(s.debug_document_json(), before);
    let m = refused(s.run_mixer(
        "mixer.new_group".into(),
        vec![text("strips", &d.kick.to_string())],
    ));
    assert!(m.contains("is in group Drums"), "{m}");
    s.close().unwrap();
}
