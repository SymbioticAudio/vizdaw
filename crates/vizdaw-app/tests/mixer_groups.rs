//! Fader groups in the mixer (Swift `MixerGroupsTests`
//! `groupingTheSelectionRunsNewGroup`, `theMixerMenusRulesFollowTheCommands`,
//! `ungroupDeletesTheSelectionsGroup`, `optionDragTouchesAlone`,
//! `followersShowTheCoresValues`, `keysFillTheSelection`,
//! `theChannelNeverFillsTheSelection`), and the group and VCA ids over the
//! channel and the keys.

mod support;

use std::sync::Arc;
use std::time::Instant;

use core_control::registry::{self, ArgValue, GROUP_IDS};
use core_control::{ControlError, ErrorCode};
use core_model::automation::fader_norm;
use support::{FakeSession, group_row, groups_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{self, MIXER_COMMANDS, Ran, perform_from_menu, run};
use vizdaw_app::mixer::{GroupLabels, MixerModel, MixerPlacement, VcaStripLabels};
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{AutomationStateRow, GestureHolderRow, GesturePhaseRow};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::GroupRow;

fn make(groups: Vec<GroupRow>) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(groups_fake(vec![], groups));
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.ensure_mixer());
    (f, d)
}

fn mx<R>(d: &mut DocumentModel, f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R) -> R {
    d.with_mixer(f).expect("a mixer")
}

fn select(d: &mut DocumentModel, strips: &[u64]) {
    d.set_selected_strips(strips.iter().copied().collect());
}

fn text(name: &str, v: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text { value: v.into() },
    }
}

fn int(name: &str, v: i64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int { value: v },
    }
}

fn flag(name: &str, v: bool) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Flag { value: v },
    }
}

fn model_err(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn busy(m: &str) -> VizError {
    VizError::Busy { message: m.into() }
}

fn err(r: Result<Ran, ControlError>) -> ControlError {
    r.expect_err("refused")
}

fn gain(d: &DocumentModel, strip: u64) -> f64 {
    let m = d.mixer().unwrap();
    m.gain_db(m.strip(strip).unwrap())
}

#[test]
fn grouping_the_selection_runs_new_group() {
    let (f, mut d) = make(vec![]);
    select(&mut d, &[3, 2, 0]);
    assert!(mx(&mut d, |m, d| m.can_group_selection(d)));
    mx(&mut d, |m, d| m.group_selection(d));
    assert_eq!(
        f.run_mixers(),
        [("mixer.new_group".to_owned(), vec![text("strips", "2,3")])],
        "in ID order, master left out"
    );
    assert!(d.alert.is_none());
}

#[test]
fn the_mixer_menus_rules_follow_the_commands() {
    let g = group_row(1, "Drums", &[2, 3]);
    let (_, mut d) = make(vec![g.clone()]);
    let rules = |d: &mut DocumentModel| {
        mx(d, |m, d| {
            (
                m.can_group_selection(d),
                m.selection_group(d).cloned(),
                m.selected_members(d),
            )
        })
    };
    select(&mut d, &[0]);
    assert_eq!(
        rules(&mut d),
        (false, None, vec![]),
        "master alone: no strip"
    );
    select(&mut d, &[2, 3]);
    let (can, group, _) = rules(&mut d);
    assert_eq!(group, Some(g.clone()), "the selection shares one group");
    assert!(!can, "a strip in a group is refused");
    select(&mut d, &[2, 4]);
    assert_eq!(rules(&mut d).1, None, "Bass is in no group");
    select(&mut d, &[4]);
    assert!(rules(&mut d).0);
    select(&mut d, &[]);
    assert_eq!(rules(&mut d), (false, None, vec![]));
    let m = d.mixer().unwrap();
    assert_eq!(m.group_of(m.strip(3).unwrap()), Some(&g));
    assert_eq!(m.group_of(m.strip(4).unwrap()), None);
    assert_eq!(m.groups(), [g]);
}

#[test]
fn ungroup_deletes_the_selections_group() {
    let (f, mut d) = make(vec![group_row(7, "Drums", &[2, 3])]);
    mx(&mut d, |m, d| m.ungroup_selection(d));
    assert!(
        f.run_mixers().is_empty(),
        "no selection: nothing to ungroup"
    );
    select(&mut d, &[2, 3]);
    mx(&mut d, |m, d| m.ungroup_selection(d));
    assert_eq!(
        f.run_mixers(),
        [("mixer.delete_group".to_owned(), vec![int("group", 7)])]
    );
}

#[test]
fn the_group_tags_menu_runs_its_edits() {
    let (f, mut d) = make(vec![group_row(7, "Drums", &[2, 3])]);
    mx(&mut d, |m, d| {
        m.set_group_links(d, 7, Some(false), None);
        m.set_group_links(d, 7, None, Some(true));
        m.set_group_links(d, 7, None, None);
        m.remove_from_group(d, 7, &[3]);
        m.remove_from_group(d, 7, &[]);
        m.delete_group(d, 7);
    });
    assert_eq!(
        f.run_mixers(),
        [
            (
                "mixer.set_group_links".to_owned(),
                vec![int("group", 7), flag("mute", false)]
            ),
            (
                "mixer.set_group_links".to_owned(),
                vec![int("group", 7), flag("solo", true)]
            ),
            (
                "mixer.remove_from_group".to_owned(),
                vec![int("group", 7), text("strips", "3")]
            ),
            ("mixer.delete_group".to_owned(), vec![int("group", 7)]),
        ],
        "no link given and no strips send nothing"
    );
    assert_eq!(VcaStripLabels::group("Drums"), "Group Drums");
    assert_eq!(GroupLabels::LINK_MUTES, "Link Mutes");
    assert_eq!(GroupLabels::LINK_SOLOS, "Link Solos");
    assert_eq!(GroupLabels::REMOVE, "Remove from Group");
    assert_eq!(GroupLabels::UNGROUP, "Ungroup");
}

#[test]
fn a_refused_group_edit_alerts_with_its_title() {
    let (f, mut d) = make(vec![group_row(7, "Drums", &[2, 3])]);
    f.fail_next("run_mixer", model_err("Strip 2 is in another group."));
    select(&mut d, &[4]);
    mx(&mut d, |m, d| m.group_selection(d));
    assert_eq!(d.alert.as_ref().unwrap().title, "Group Failed");
    d.alert = None;
    f.fail_next("run_mixer", model_err("No group 7."));
    mx(&mut d, |m, d| m.delete_group(d, 7));
    assert_eq!(d.alert.as_ref().unwrap().title, "Ungroup Failed");
}

#[test]
fn option_drag_touches_alone() {
    let (f, mut d) = make(vec![]);
    let t = T::StripFader { strip: 2 };
    let now = Instant::now();
    mx(&mut d, |m, d| {
        m.touch(d, t, true);
        m.move_to(d, t, 0.5, now);
        m.release(d, t, now);
    });
    let alones: Vec<bool> = f.gestures().iter().map(|g| g.2).collect();
    assert_eq!(alones, [true, true, true], "touch, value and release");
    assert_eq!(f.holders(), [GestureHolderRow::Mouse; 3]);
    mx(&mut d, |m, d| {
        m.touch(d, t, false);
        m.release(d, t, now);
    });
    let g = f.gestures();
    assert_eq!(g.len(), 5);
    assert!(!g[3].2 && !g[4].2, "a plain drag is the group's");
}

/// ⌥ at the press makes a strip fader's drag alone; ⌥ later is only fine
/// control; a pan with ⌥ is never alone.
#[test]
fn option_at_the_press_is_alone_and_only_for_a_strip_fader() {
    let (f, mut d) = make(vec![]);
    let now = Instant::now();
    let p = MixerPlacement::View;
    let fader = T::StripFader { strip: 2 };
    let pan = T::StripPan { strip: 2 };
    mx(&mut d, |m, d| {
        m.press(d, p, fader, 160.0, true, now);
        m.drag(d, p, 0.0, -10.0, false, now);
        m.lift(d, p, now);
    });
    assert!(f.gestures().iter().all(|g| g.2), "alone throughout");
    f.clear_gestures();
    mx(&mut d, |m, d| {
        m.press(d, p, fader, 160.0, false, now);
        m.drag(d, p, 0.0, -10.0, true, now);
        m.lift(d, p, now);
        m.press(d, p, pan, 200.0, true, now);
        m.drag(d, p, 0.0, -10.0, true, now);
        m.lift(d, p, now);
    });
    assert!(!f.gestures().is_empty());
    assert!(f.gestures().iter().all(|g| !g.2), "⌥ mid-drag is fine only");
}

#[test]
fn followers_show_the_cores_values() {
    let (f, mut d) = make(vec![group_row(1, "Drums", &[2, 3])]);
    let lead = T::StripFader { strip: 2 };
    let follower = T::StripFader { strip: 3 };
    let now = Instant::now();
    mx(&mut d, |m, d| {
        m.touch(d, lead, false);
        m.move_to(d, lead, fader_norm(-6.0), now);
    });
    f.set_automation_state(AutomationStateRow {
        modes: vec![],
        touched: vec![lead, follower],
        touched_values: vec![fader_norm(-1.0), fader_norm(-6.0)],
        suspended: false,
        writing: false,
    });
    mx(&mut d, |m, d| m.poll_automation(d));
    assert!((gain(&d, 3) + 6.0).abs() < 1e-6, "the follower moves");
    assert!((gain(&d, 2) + 6.0).abs() < 1e-6, "the hand here wins");
    assert_eq!(gain(&d, 4), 0.0);
    assert!(d.mixer().unwrap().is_touched(follower), "its ring");
    f.set_automation_state(AutomationStateRow {
        modes: vec![],
        touched: vec![],
        touched_values: vec![],
        suspended: false,
        writing: false,
    });
    mx(&mut d, |m, d| {
        m.release(d, lead, now);
        m.poll_automation(d);
    });
    assert_eq!(gain(&d, 3), 0.0, "the snapshot's again");
}

/// A follower that goes mid-drag is the core's to release: the app sends
/// it nothing, and the lead's release is sent once.
#[test]
fn a_follower_that_goes_mid_drag_is_released_by_nobody_here() {
    let (f, mut d) = make(vec![group_row(1, "Drums", &[2, 3])]);
    let lead = T::StripFader { strip: 2 };
    let follower = T::StripFader { strip: 3 };
    let now = Instant::now();
    mx(&mut d, |m, d| {
        m.touch(d, lead, false);
        m.move_to(d, lead, fader_norm(-6.0), now);
    });
    f.set_automation_state(AutomationStateRow {
        modes: vec![],
        touched: vec![lead, follower],
        touched_values: vec![fader_norm(-6.0), fader_norm(-6.0)],
        suspended: false,
        writing: false,
    });
    mx(&mut d, |m, d| m.poll_automation(d));
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 3));
    // An edit refetches the mixer: Snare is gone.
    mx(&mut d, |m, d| {
        m.set_output(d, 4, None);
        m.release(d, lead, now);
    });
    let releases: Vec<T> = f
        .gestures()
        .iter()
        .filter(|g| g.1 == GesturePhaseRow::Release)
        .map(|g| g.0)
        .collect();
    assert_eq!(releases, [lead]);
}

/// Every group and VCA id, with arguments the channel might send.
fn ids() -> Vec<(&'static str, Vec<(String, ArgValue)>)> {
    let g = || ("group".to_owned(), ArgValue::Int(1));
    let v = || ("vca".to_owned(), ArgValue::Int(1));
    let s = || ("strips".to_owned(), ArgValue::Text("2".into()));
    let n = || ("name".to_owned(), ArgValue::Text("Kit".into()));
    let c = || ("colour".to_owned(), ArgValue::Text("red".into()));
    vec![
        ("mixer.new_group", vec![s()]),
        ("mixer.rename_group", vec![g(), n()]),
        ("mixer.set_group_colour", vec![g(), c()]),
        ("mixer.add_to_group", vec![g(), s()]),
        ("mixer.remove_from_group", vec![g(), s()]),
        (
            "mixer.set_group_links",
            vec![g(), ("mute".into(), ArgValue::Flag(true))],
        ),
        ("mixer.delete_group", vec![g()]),
        ("mixer.new_vca", vec![s()]),
        ("mixer.rename_vca", vec![v(), n()]),
        ("mixer.set_vca_colour", vec![v(), c()]),
        ("mixer.assign_vca", vec![v(), s()]),
        ("mixer.unassign_vca", vec![v(), s()]),
        (
            "mixer.set_vca",
            vec![v(), ("gain_db".into(), ArgValue::Text("-6".into()))],
        ),
        ("mixer.delete_vca", vec![v()]),
    ]
}

fn control_args(a: &[(String, ArgValue)]) -> Vec<ControlArg> {
    a.iter()
        .map(|(n, v)| match v {
            ArgValue::Text(t) => text(n, t),
            ArgValue::Flag(b) => flag(n, *b),
            ArgValue::Int(i) => int(n, *i),
        })
        .collect()
}

#[test]
fn group_and_vca_ids_run_through_one_call_and_answer_their_title() {
    assert_eq!(
        ids().iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        GROUP_IDS
    );
    for (id, args) in ids() {
        let (f, mut d) = make(vec![]);
        assert!(MIXER_COMMANDS.contains(&id), "{id}");
        assert!(commands::enabled(id, &d), "{id}");
        let before = d.snapshot().version;
        let ran = run(id, &args, &mut d).unwrap_or_else(|e| panic!("{id}: {e:?}"));
        let title = registry::find(id).unwrap().title;
        assert_eq!(ran.status.as_deref(), Some(title), "{id}");
        assert_eq!(
            f.run_mixers(),
            [(id.to_owned(), control_args(&args))],
            "{id}: the arguments unchanged"
        );
        assert!(d.snapshot().version > before, "{id}");
        assert!(d.alert.is_none(), "{id}");
    }
}

#[test]
fn a_refused_group_or_vca_id_is_failed_or_busy_and_a_drag_stops_it() {
    for (id, args) in ids() {
        let (f, mut d) = make(vec![]);
        f.set_call_error("run_mixer", Some(model_err("No VCA 1.")));
        let e = err(run(id, &args, &mut d));
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::Failed, "No VCA 1.")
        );
        f.set_call_error("run_mixer", Some(busy("Finish the edit first.")));
        assert_eq!(err(run(id, &args, &mut d)).code, ErrorCode::Busy, "{id}");
        assert!(d.alert.is_none(), "{id}: no dialog");
        f.set_call_error("run_mixer", None);
        let calls = f.run_mixers().len();
        assert!(d.begin_gesture("Move", vec![]));
        let e = err(run(id, &args, &mut d));
        assert_eq!(e.code, ErrorCode::Unavailable, "{id}");
        assert_eq!(e.message, "Finish the drag first.", "{id}");
        assert_eq!(f.run_mixers().len(), calls, "{id}");
    }
}

#[test]
fn keys_fill_the_selection() {
    let (f, mut d) = make(vec![]);
    select(&mut d, &[4, 2, 0]);
    perform_from_menu("mixer.new_vca", &mut d).unwrap();
    assert_eq!(
        f.run_mixers(),
        [("mixer.new_vca".to_owned(), vec![text("strips", "2,4")])]
    );
    perform_from_menu("mixer.new_group", &mut d).unwrap();
    assert_eq!(f.run_mixers()[1].1, [text("strips", "2,4")]);
    let _ = perform_from_menu("mixer.rename_vca", &mut d);
    assert_eq!(f.run_mixers()[2], ("mixer.rename_vca".to_owned(), vec![]));
    select(&mut d, &[]);
    perform_from_menu("mixer.new_vca", &mut d).unwrap();
    assert_eq!(
        f.run_mixers()[3],
        ("mixer.new_vca".to_owned(), vec![]),
        "no selection: no strips"
    );
}

/// The selected strips a key fills are the mixer's now: a strip gone
/// since (no refetch saw it) is left out.
#[test]
fn keys_fill_only_strips_the_mixer_still_has() {
    let (f, mut d) = make(vec![]);
    select(&mut d, &[3, 2]);
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 3));
    perform_from_menu("mixer.new_group", &mut d).unwrap();
    assert_eq!(f.run_mixers()[0].1, [text("strips", "2")]);
}

#[test]
fn the_channel_never_fills_the_selection() {
    let (f, mut d) = make(vec![]);
    select(&mut d, &[2, 3]);
    for id in ["mixer.new_group", "mixer.new_vca"] {
        run(id, &[], &mut d).unwrap();
        assert_eq!(
            f.run_mixers().last(),
            Some(&(id.to_owned(), vec![])),
            "{id}"
        );
    }
}
