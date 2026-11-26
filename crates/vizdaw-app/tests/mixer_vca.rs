//! VCAs in the mixer (Swift `MixerGroupsTests`
//! `assignAndRemoveTakeOnlyWhatTheCoreWouldDo`,
//! `vcaOffsetReadoutSumsShownValues`, `theOffsetIsTheCoresAtRest`,
//! `theVcaModesAreTheCoresChoice`, `vcaColumnsSitBeforeMaster`,
//! `vcaStripControlsAreLabelled`, `aVcaStripsEditsAreSetVca`).

mod support;

use std::sync::Arc;
use std::time::Instant;

use core_model::automation::fader_norm;
use support::{FakeSession, groups_fake, vca_row};
use vizdaw_app::mixer::{
    AutomationModes, MixerMath, MixerModel, MixerPlacement, StripEntry, VcaStripLabels,
};
use vizdaw_app::{DocSession, DocumentModel};
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{AutomationModeRow, GesturePhaseRow};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{StripKindRow, VcaRow};
use vizdaw_ffi::session::Session;

const FLOOR: f64 = MixerMath::FLOOR_DB;

fn make(vcas: Vec<VcaRow>) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(groups_fake(vcas, vec![]));
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.ensure_mixer());
    (f, d)
}

fn mx<R>(d: &mut DocumentModel, f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R) -> R {
    d.with_mixer(f).expect("a mixer")
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

fn offset(d: &DocumentModel, strip: u64) -> f64 {
    let m = d.mixer().unwrap();
    m.vca_offset_db(m.strip(strip).unwrap())
}

fn level(d: &DocumentModel, strip: u64) -> f64 {
    let m = d.mixer().unwrap();
    m.member_level_db(m.strip(strip).unwrap())
}

fn ids(f: &FakeSession) -> Vec<String> {
    f.run_mixers().into_iter().map(|r| r.0).collect()
}

#[test]
fn assign_and_remove_take_only_what_the_core_would_do() {
    let v = vca_row(1, "Drums", &[2], 0.0);
    let (f, mut d) = make(vec![v.clone()]);
    let m = d.mixer().unwrap();
    assert_eq!(m.assignable(&[2, 3], &v), [3], "a member is not again");
    assert!(m.vcas_holding(&[3]).is_empty());
    assert_eq!(m.vcas_holding(&[2, 3]), [&v]);
    assert_eq!(m.vca(1), Some(&v));
    mx(&mut d, |m, d| {
        m.assign(d, &[3], 1, true);
        m.assign(d, &[2], 1, false);
        m.assign(d, &[], 1, true);
        m.new_vca_from_selection(d, false);
    });
    assert_eq!(
        ids(&f),
        ["mixer.assign_vca", "mixer.unassign_vca", "mixer.new_vca"],
        "no strips: nothing assigned"
    );
    let runs = f.run_mixers();
    assert_eq!(runs[0].1, [int("vca", 1), text("strips", "3")]);
    assert_eq!(runs[1].1, [int("vca", 1), text("strips", "2")]);
    assert_eq!(
        runs[2].1,
        [],
        "a new VCA without the selection has no strips"
    );
    d.set_selected_strips([0, 4, 3].into());
    mx(&mut d, |m, d| m.new_vca_from_selection(d, true));
    assert_eq!(
        f.run_mixers()[3].1,
        [text("strips", "3,4")],
        "the selection, master left out"
    );
}

#[test]
fn vca_offset_readout_sums_shown_values() {
    let (_, mut d) = make(vec![
        vca_row(1, "A", &[2, 3], -3.0),
        vca_row(2, "B", &[2], 1.0),
        vca_row(3, "Off", &[4], FLOOR),
    ]);
    assert_eq!(offset(&d, 2), -2.0);
    let text = |db| MixerModel::vca_offset_text(db);
    assert_eq!(text(offset(&d, 2)).as_deref(), Some("VCA \u{2212}2.0"));
    assert_eq!(text(offset(&d, 5)), None, "in no VCA: 0");
    assert_eq!(text(0.0), None);
    assert_eq!(text(-0.04), None, "reads 0.0");
    assert_eq!(text(2.0).as_deref(), Some("VCA +2.0"));
    assert_eq!(text(-3.0).as_deref(), Some("VCA \u{2212}3.0"));
    assert_eq!(text(offset(&d, 4)).as_deref(), Some("VCA \u{2212}\u{221E}"));
    assert_eq!(text(f64::NAN).as_deref(), Some("VCA \u{2212}\u{221E}"));
    assert_eq!(
        VcaStripLabels::offset("VCA \u{2212}3.0"),
        "VCA offset VCA \u{2212}3.0"
    );
    // A drag on VCA A moves the readout before the snapshot does.
    let t = T::VcaFader { vca: 1 };
    let now = Instant::now();
    mx(&mut d, |m, d| {
        m.touch(d, t, false);
        m.move_to(d, t, fader_norm(-10.0), now);
    });
    assert!((offset(&d, 3) + 10.0).abs() < 1e-6);
}

/// Review Focus 3: a member's level includes its VCAs' offset; a VCA at
/// −∞ silences it; removing the VCA or the member restores its own level.
#[test]
fn a_members_level_includes_the_vca_offset() {
    let (f, mut d) = make(vec![
        vca_row(1, "A", &[2, 3], -3.0),
        vca_row(2, "Off", &[4], FLOOR),
    ]);
    f.edit_mixer(|m| {
        for s in &mut m.strips {
            s.gain_db = -2.0;
        }
    });
    mx(&mut d, |m, d| m.set_output(d, 5, None));
    assert_eq!(level(&d, 2), -5.0, "own −2 and the VCA's −3");
    assert_eq!(level(&d, 4), FLOOR, "a VCA at −∞ silences its members");
    assert_eq!(level(&d, 5), -2.0, "in no VCA: its own");
    assert_eq!(
        d.mixer()
            .unwrap()
            .gain_db(d.mixer().unwrap().strip(2).unwrap()),
        -2.0,
        "the fader stays its own"
    );
    // Snare leaves VCA A; VCA Off is deleted.
    f.edit_mixer(|m| {
        m.vcas.retain(|v| v.id != 2);
        m.vcas[0].members = vec![2];
        for s in &mut m.strips {
            if s.id != 2 {
                s.vcas.clear();
            }
        }
    });
    mx(&mut d, |m, d| m.delete_vca(d, 2));
    assert_eq!(level(&d, 3), -2.0, "a removed member: its own level");
    assert_eq!(level(&d, 4), -2.0, "a deleted VCA: its own level");
    assert_eq!(level(&d, 2), -5.0);
}

#[test]
fn the_mark_is_the_snapshots() {
    let (f, mut d) = make(vec![vca_row(1, "A", &[2], 0.0)]);
    f.edit_mixer(|m| m.strips[0].vca_mark = true);
    mx(&mut d, |m, d| m.set_output(d, 5, None));
    let m = d.mixer().unwrap();
    assert!(m.vca_mark(m.strip(2).unwrap()));
    assert!(!m.vca_mark(m.strip(3).unwrap()));
    assert_eq!(VcaStripLabels::mark("Kick"), "VCA mute or solo on Kick");
}

/// Pre-flight F25: at rest, the readout's sum is the core's.
#[test]
fn the_offset_is_the_cores_at_rest() {
    let session = Session::new_demo().unwrap();
    let mut d = DocumentModel::new(session.clone() as Arc<dyn DocSession>).unwrap();
    assert!(d.ensure_mixer());
    let member = d
        .mixer()
        .unwrap()
        .strips()
        .find(|s| s.kind != StripKindRow::Bus)
        .unwrap()
        .id;
    let run = |d: &mut DocumentModel, id: &str, args: Vec<ControlArg>| {
        let s = session.run_mixer(id.into(), args).unwrap();
        d.apply(s);
    };
    let strips = text("strips", &member.to_string());
    run(&mut d, "mixer.new_vca", vec![strips.clone()]);
    run(&mut d, "mixer.new_vca", vec![strips]);
    let vcas: Vec<u64> = session.mixer().unwrap().vcas.iter().map(|v| v.id).collect();
    assert_eq!(vcas.len(), 2);
    let gain = |v: u64, db: &str| vec![int("vca", v as i64), text("gain_db", db)];
    run(&mut d, "mixer.set_vca", gain(vcas[0], "-4.5"));
    run(&mut d, "mixer.set_vca", gain(vcas[1], "1"));
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    let m = d.mixer().unwrap();
    let row = m.strip(member).unwrap();
    assert_eq!(row.vca_offset_db, -3.5);
    assert_eq!(m.vca_offset_db(row), row.vca_offset_db);
    assert_eq!(m.member_level_db(row), row.gain_db - 3.5);
    session.close().unwrap();
}

/// Pre-flight F10: the VCA modes are the registry's; Write and Trim are
/// refused.
#[test]
fn the_vca_modes_are_the_cores_choice() {
    assert_eq!(
        AutomationModes::VCA,
        [
            AutomationModeRow::Off,
            AutomationModeRow::Read,
            AutomationModeRow::Touch,
            AutomationModeRow::Latch
        ]
    );
    let names: Vec<&str> = AutomationModes::VCA
        .iter()
        .map(|m| AutomationModes::name(*m))
        .collect();
    assert_eq!(names, core_control::registry::VCA_MODE_NAMES);
    let session = Session::new_demo().unwrap();
    session.run_mixer("mixer.new_vca".into(), vec![]).unwrap();
    let vca = session.mixer().unwrap().vcas[0].id;
    let set = |m: AutomationModeRow| {
        session.run_mixer(
            "mixer.set_vca".into(),
            vec![
                int("vca", vca as i64),
                text("mode", AutomationModes::name(m)),
            ],
        )
    };
    for m in AutomationModes::VCA {
        set(m).unwrap();
        assert_eq!(session.mixer().unwrap().vcas[0].automation, m);
    }
    for m in AutomationModes::ALL {
        if !AutomationModes::VCA.contains(&m) {
            assert!(set(m).is_err(), "{m:?}");
        }
    }
    session.close().unwrap();
}

#[test]
fn vca_columns_sit_before_master() {
    let (_, d) = make(vec![vca_row(1, "A", &[], 0.0), vca_row(2, "B", &[], 0.0)]);
    let m = d.mixer().unwrap();
    let cols: Vec<String> = m.columns().iter().map(|c| c.id()).collect();
    assert_eq!(
        cols,
        ["s2", "s3", "s4", "s5", "v1", "v2"],
        "buses, then VCAs"
    );
    assert_eq!(m.master().map(|s| s.id), Some(0));
}

#[test]
fn vca_strip_controls_are_labelled() {
    assert_eq!(VcaStripLabels::fader("Drums"), "Fader of VCA Drums");
    assert_eq!(VcaStripLabels::level("Drums"), "Level of VCA Drums");
    assert_eq!(VcaStripLabels::mute("Drums"), "Mute VCA Drums");
    assert_eq!(VcaStripLabels::solo("Drums"), "Solo VCA Drums");
    assert_eq!(VcaStripLabels::MODE, "VCA automation mode");
    assert_eq!(VcaStripLabels::fader("VCA 1"), "Fader of VCA 1");
    assert_eq!(VcaStripLabels::level("VCA 1"), "Level of VCA 1");
    assert_eq!(VcaStripLabels::mute("VCA 1"), "Mute VCA 1");
    assert_eq!(VcaStripLabels::solo("VCA 1"), "Solo VCA 1");
    assert_eq!(VcaStripLabels::fader("Strings"), "Fader of VCA Strings");
    assert_eq!(VcaStripLabels::title("VCA"), "VCA");
    assert_eq!(VcaStripLabels::title("VCAs and more"), "VCA VCAs and more");
    assert_eq!(VcaStripLabels::members(1), "1 member");
    assert_eq!(VcaStripLabels::members(3), "3 members");
    assert_eq!(VcaStripLabels::members(0), "0 members");
    assert_eq!(VcaStripLabels::kind_line(3), "VCA · 3 members");
    assert_eq!(VcaStripLabels::kind_line(1), "VCA · 1 member");
    assert_eq!(VcaStripLabels::RENAME_FIELD, "Name of the VCA");
    assert_eq!(VcaStripLabels::RENAME, "Rename VCA\u{2026}");
    assert_eq!(VcaStripLabels::DELETE, "Delete VCA");
}

#[test]
fn a_vca_strips_edits_are_set_vca() {
    let v = vca_row(3, "Drums", &[2], 0.0);
    let (f, mut d) = make(vec![v.clone()]);
    mx(&mut d, |m, d| {
        m.toggle_vca_mute(d, 3);
        m.toggle_vca_solo(d, 3);
        m.set_vca_mode(d, 3, AutomationModeRow::Latch);
        m.set_vca_mode(d, 3, AutomationModeRow::Read);
        m.set_vca_mode(d, 9, AutomationModeRow::Latch);
    });
    assert_eq!(ids(&f), ["mixer.set_vca"; 3], "Read is no change; no VCA 9");
    let runs = f.run_mixers();
    assert_eq!(runs[0].1, [int("vca", 3), flag("mute", true)]);
    assert_eq!(runs[1].1, [int("vca", 3), flag("solo", true)]);
    assert_eq!(runs[2].1, [int("vca", 3), text("mode", "latch")]);
    let p = MixerPlacement::Window;
    mx(&mut d, |m, d| {
        m.begin_vca_rename(3, p);
        assert!(m.is_renaming_vca(3, p));
        assert!(!m.is_renaming_vca(3, MixerPlacement::View));
        assert_eq!(m.rename_draft, "Drums");
        m.rename_draft = "  Kit ".into();
        m.commit_vca_rename(d);
        assert_eq!(m.renaming_vca(), None);
        m.begin_vca_rename(3, p);
        m.rename_draft = " Drums ".into();
        m.commit_vca_rename(d);
        m.begin_vca_rename(3, p);
        m.rename_draft = "   ".into();
        m.commit_vca_rename(d);
        m.begin_vca_rename(3, p);
        m.rename_draft = "Other".into();
        m.cancel_vca_rename();
        m.commit_vca_rename(d);
    });
    assert_eq!(
        f.run_mixers()[3],
        (
            "mixer.rename_vca".to_owned(),
            vec![int("vca", 3), text("name", "Kit")]
        )
    );
    assert_eq!(
        f.run_mixers().len(),
        4,
        "unchanged, blank and cancelled send nothing"
    );
    mx(&mut d, |m, d| m.delete_vca(d, 3));
    assert_eq!(
        f.run_mixers()[4],
        ("mixer.delete_vca".to_owned(), vec![int("vca", 3)])
    );
}

#[test]
fn a_vca_rename_ends_when_the_vca_goes() {
    let (f, mut d) = make(vec![vca_row(3, "Drums", &[2], 0.0)]);
    mx(&mut d, |m, _| m.begin_vca_rename(3, MixerPlacement::View));
    f.edit_mixer(|m| m.vcas.clear());
    mx(&mut d, |m, d| m.set_output(d, 5, None));
    assert_eq!(d.mixer().unwrap().renaming_vca(), None);
}

#[test]
fn a_typed_vca_level_is_one_click_on_its_fader() {
    let (f, mut d) = make(vec![vca_row(3, "Drums", &[2], 0.0)]);
    let t = T::VcaFader { vca: 3 };
    let now = Instant::now();
    assert!(mx(&mut d, |m, d| m.type_vca_level(d, 3, "-6", now)));
    let g = f.gestures();
    assert_eq!(
        g.iter().map(|g| (g.0, g.1)).collect::<Vec<_>>(),
        [
            (
                t,
                GesturePhaseRow::Touch {
                    value: Some(fader_norm(-6.0))
                }
            ),
            (
                t,
                GesturePhaseRow::Value {
                    value: fader_norm(-6.0)
                }
            ),
            (t, GesturePhaseRow::Release),
        ]
    );
    assert!(!mx(&mut d, |m, d| m.type_vca_level(d, 3, "loud", now)));
    assert_eq!(d.status(), Some(StripEntry::LEVEL_HINT));
    mx(&mut d, |m, d| m.touch(d, t, false));
    assert!(!mx(&mut d, |m, d| m.type_vca_level(d, 3, "-3", now)));
    assert_eq!(d.status(), Some(StripEntry::TOUCHED_HINT));
    assert_eq!(f.gestures().len(), 4, "only the touch since");
}

type Edit = dyn Fn(&mut MixerModel, &mut DocumentModel);

#[test]
fn refused_vca_edits_alert_with_their_titles() {
    let (f, mut d) = make(vec![vca_row(3, "Drums", &[2], 0.0)]);
    let cases: Vec<(&str, Box<Edit>)> = vec![
        ("New VCA Failed", Box::new(|m, d| m.new_vca(d, &[2]))),
        ("VCA Failed", Box::new(|m, d| m.assign(d, &[3], 3, true))),
        ("VCA Failed", Box::new(|m, d| m.toggle_vca_mute(d, 3))),
        ("Delete VCA Failed", Box::new(|m, d| m.delete_vca(d, 3))),
        (
            "Rename Failed",
            Box::new(|m, d| {
                m.begin_vca_rename(3, MixerPlacement::View);
                m.rename_draft = "Kit".into();
                m.commit_vca_rename(d);
            }),
        ),
    ];
    for (title, edit) in cases {
        d.alert = None;
        f.fail_next("run_mixer", model_err("No."));
        mx(&mut d, |m, d| edit(m, d));
        assert_eq!(d.alert.as_ref().map(|a| a.title.as_str()), Some(title));
    }
}
