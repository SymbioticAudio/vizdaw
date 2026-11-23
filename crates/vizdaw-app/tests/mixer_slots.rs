//! A strip's slots apart from their windows (Swift `MixerStripTests`
//! `theEqSlotLoadsBypassesAndEmpties`, `clearingAnEmptyEqSlotSendsNothing`,
//! `theEqSlotIsSeenByEveryStripSlotReader`; `MixerModelTests`
//! `safeModeLoadsAStripsHeldSlots`,
//! `theMixerIsRefetchedOnlyWhenTheDocumentOrSlotsChange`,
//! `aBusyLoadRefetchIsOwedEvenWithoutAVersionChange`; `SlotView.dot`,
//! `SlotView.menu`, `EqSlotView.menu`).

mod support;

use std::sync::Arc;

use support::{FakeSession, mixer_fake, slot_row, strip_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::mixer::{
    InstrumentRow, MixerModel, SidechainChoice, SlotLabels, SlotMenuItem, SlotPlace, SlotState,
};
use vizdaw_ffi::VizError;
use vizdaw_ffi::mixer::{MixerLevels, StripKindRow};
use vizdaw_ffi::snapshot::SlotStatus;

/// Synth (1): instrument 11, EQ 12, inserts Gain (10) and Comp (13);
/// Audio 1 (3, an audio path) with insert Gate (14); Bus 1 (2); Master.
fn slots_fake() -> FakeSession {
    let f = mixer_fake();
    f.edit_mixer(|m| {
        let s = &mut m.strips[0];
        s.instrument = Some(slot_row(11, "Inst"));
        s.eq = Some(slot_row(12, "EQ"));
        s.inserts = vec![slot_row(10, "Gain"), slot_row(13, "Comp")];
        let mut audio = strip_row(3, "Audio 1", StripKindRow::AudioPath { path: 1 });
        audio.inserts = vec![slot_row(14, "Gate")];
        m.strips.insert(1, audio);
    });
    f
}

fn make_with(f: FakeSession) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(f);
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.ensure_mixer());
    (f, d)
}

fn mx<R>(d: &mut DocumentModel, f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R) -> R {
    d.with_mixer(f).expect("a mixer")
}

fn model_err(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn busy() -> VizError {
    VizError::Busy {
        message: "b".into(),
    }
}

#[test]
fn every_reader_sees_the_instrument_then_the_eq_then_the_inserts() {
    let (_f, d) = make_with(slots_fake());
    let m = d.mixer().unwrap();
    let synth = m.strip(1).unwrap();
    let ids: Vec<u64> = MixerModel::slots(synth).map(|x| x.id).collect();
    assert_eq!(ids, [11, 12, 10, 13]);
    assert_eq!(
        m.slot(11).map(|(s, _, p)| (s.id, p)),
        Some((1, SlotPlace::Instrument))
    );
    assert_eq!(
        m.slot(12).map(|(s, _, p)| (s.id, p)),
        Some((1, SlotPlace::Eq))
    );
    assert_eq!(
        m.slot(13).map(|(s, _, p)| (s.id, p)),
        Some((1, SlotPlace::Insert(1)))
    );
    assert_eq!(
        m.slot(14).map(|(s, _, p)| (s.id, p)),
        Some((3, SlotPlace::Insert(0)))
    );
    assert!(m.slot(99).is_none());
}

#[test]
fn the_instrument_row_is_the_plugin_or_the_built_in_synth() {
    let (_f, d) = make_with(slots_fake());
    let m = d.mixer().unwrap();
    let synth = m.strip(1).unwrap();
    assert!(matches!(
        MixerModel::instrument_row(synth),
        Some(InstrumentRow::Plugin(x)) if x.id == 11
    ));
    assert_eq!(MixerModel::instrument_row(m.strip(3).unwrap()), None);
    assert_eq!(MixerModel::instrument_row(m.strip(2).unwrap()), None);
    // The Built-in Synth strip without a plugin slot shows the synth's.
    let f = slots_fake();
    f.edit_mixer(|m| m.strips[0].instrument = None);
    let (_f, d) = make_with(f);
    let m = d.mixer().unwrap();
    assert_eq!(
        MixerModel::instrument_row(m.strip(1).unwrap()),
        Some(InstrumentRow::BuiltinSynth)
    );
}

#[test]
fn a_slots_dot_is_faulted_then_its_status() {
    let f = slots_fake();
    f.edit_mixer(|m| {
        let s = &mut m.strips[0];
        s.inserts[0].bypass = true;
        s.inserts[1].status = SlotStatus::Loading;
        s.eq.as_mut().unwrap().status = SlotStatus::Placeholder {
            reason: "Missing: EQ".into(),
        };
    });
    f.set_levels(MixerLevels {
        faulted: vec![14],
        ..MixerLevels::empty()
    });
    let (_f, mut d) = make_with(f);
    mx(&mut d, |m, d| m.poll_levels(d, std::time::Instant::now()));
    let m = d.mixer().unwrap();
    let state = |id| m.slot_state(m.slot(id).unwrap().1);
    assert_eq!(state(11), SlotState::Live);
    assert_eq!(state(10), SlotState::Bypassed);
    assert_eq!(state(13), SlotState::Loading);
    assert_eq!(state(12), SlotState::Placeholder);
    assert_eq!(state(14), SlotState::Faulted);
    assert!(m.is_faulted(14));
    // The reason dims the name and is its help.
    let reason = |id| m.slot_reason(m.slot(id).unwrap().1);
    assert_eq!(reason(12).as_deref(), Some("Missing: EQ"));
    assert_eq!(
        reason(14).as_deref(),
        Some("Bypassed: the plugin misbehaved")
    );
    assert_eq!(reason(11), None);
    assert_eq!(m.slot_help(m.slot(11).unwrap().1), "Inst (V)");
    assert_eq!(m.slot_help(m.slot(14).unwrap().1), SlotLabels::FAULTED);
}

#[test]
fn slot_labels_are_swifts() {
    let x = slot_row(11, "Inst");
    assert_eq!(
        SlotLabels::slot(&x, SlotPlace::Instrument),
        "Inst, instrument"
    );
    assert_eq!(SlotLabels::slot(&x, SlotPlace::Insert(0)), "Inst");
    assert_eq!(SlotLabels::slot(&x, SlotPlace::Eq), "Inst");
    assert_eq!(SlotLabels::add_effect("Synth"), "Add an effect to Synth");
    assert_eq!(SlotLabels::eq("Synth"), "EQ of Synth");
    assert_eq!(SlotLabels::eq_bypass("Synth", false), "Bypass EQ of Synth");
    assert_eq!(SlotLabels::eq_bypass("Synth", true), "Turn On EQ of Synth");
    assert_eq!(SlotLabels::eq_bypass_help(false), "Bypass the EQ");
    assert_eq!(SlotLabels::eq_bypass_help(true), "Turn the EQ on");
    assert_eq!(SlotLabels::EQ_ADD_FACE, "EQ \u{FF0B}");
    assert_eq!(
        SlotLabels::BUILTIN_SYNTH_LABEL,
        "Built-in Synth, instrument"
    );
    assert_eq!(SlotLabels::REPLACE, "Replace\u{2026}");
}

#[test]
fn menus_follow_swifts_and_leave_the_window_and_catalogue_off() {
    let f = slots_fake();
    f.edit_mixer(|m| {
        m.strips[0].inserts[1].has_sidechain_input = true;
        m.strips[0].inserts[1].sidechain = Some(3);
        m.strips[0].inserts[0].bypass = true;
        m.strips[0].eq.as_mut().unwrap().has_sidechain_input = true;
        m.strips[0].instrument.as_mut().unwrap().has_sidechain_input = true;
    });
    f.set_held(vec![10]);
    let (_f, d) = make_with(f);
    let m = d.mixer().unwrap();
    let synth = m.strip(1).unwrap();
    let menu = |id| {
        let (s, x, p) = m.slot(id).unwrap();
        m.slot_menu(s, x, p)
    };
    let titles = |id| menu(id).iter().map(SlotMenuItem::title).collect::<Vec<_>>();
    // A held, bypassed insert without a sidechain input.
    assert_eq!(titles(10), ["Open", "Load", "Turn On", "Remove"]);
    // An insert with a sidechain input, fed by Audio 1.
    assert_eq!(titles(13), ["Open", "Bypass", "Sidechain", "Remove"]);
    let choices = menu(13)
        .into_iter()
        .find_map(|i| match i {
            SlotMenuItem::Sidechain(c) => Some(c),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        choices,
        [
            SidechainChoice {
                title: "None".into(),
                source: None
            },
            SidechainChoice {
                title: "\u{2713} Audio 1".into(),
                source: Some(3)
            },
            SidechainChoice {
                title: "Bus 1".into(),
                source: Some(2)
            },
        ],
        "not itself, not master"
    );
    // The EQ: Replace…, its sidechain, a divider and Remove EQ.
    assert_eq!(
        titles(12),
        [
            "Open",
            "Replace\u{2026}",
            "Bypass",
            "Sidechain",
            "",
            "Remove EQ"
        ]
    );
    // The instrument's own slot: no sidechain, no Remove.
    assert_eq!(titles(11), ["Open", "Bypass"]);
    // Open and Replace… are disabled (their slices come later).
    for item in menu(12) {
        let off = matches!(
            item,
            SlotMenuItem::Open | SlotMenuItem::Replace | SlotMenuItem::Divider
        );
        assert_eq!(item.enabled(), !off, "{}", item.title());
    }
    assert_eq!(m.sidechain_sources(synth).len(), 2);
}

#[test]
fn slot_edits_are_one_call_each() {
    let (f, mut d) = make_with(slots_fake());
    mx(&mut d, |m, d| {
        m.toggle_bypass(d, 10);
        m.toggle_bypass(d, 10);
        m.toggle_bypass(d, 99);
        m.set_sidechain(d, 13, Some(3));
        m.set_sidechain(d, 13, None);
        m.move_insert(d, 13, 1, 0);
        m.move_insert(d, 14, 1, 2);
        m.remove_insert(d, 10);
    });
    assert_eq!(
        f.calls(),
        [
            "set_bypass(10,true)",
            "set_bypass(10,false)",
            "set_sidechain(13,Some(3))",
            "set_sidechain(13,None)",
            "move_insert(13,1,0)",
            "move_insert(14,1,2)",
            "remove_insert(10)",
        ],
        "an unknown slot sends nothing"
    );
    // The mixer is read again after each: the fake's edits show.
    let m = d.mixer().unwrap();
    let ids: Vec<u64> = m.strip(1).unwrap().inserts.iter().map(|x| x.id).collect();
    assert_eq!(ids, [13, 14]);
    assert!(m.strip(3).unwrap().inserts.is_empty());
    assert!(d.alert.is_none());
}

type Edit = fn(&mut MixerModel, &mut DocumentModel);

#[test]
fn refused_slot_edits_raise_swifts_titles() {
    let (f, mut d) = make_with(slots_fake());
    let cases: [(&str, &str, Edit); 5] = [
        ("set_bypass", "Bypass Failed", |m, d| m.toggle_bypass(d, 10)),
        ("remove_insert", "Remove Plugin Failed", |m, d| {
            m.remove_insert(d, 10)
        }),
        ("move_insert", "Move Plugin Failed", |m, d| {
            m.move_insert(d, 10, 1, 1)
        }),
        ("set_sidechain", "Sidechain Failed", |m, d| {
            m.set_sidechain(d, 13, Some(2))
        }),
        ("set_eq", "Clear EQ Failed", |m, d| m.clear_eq(d, 1)),
    ];
    for (call, title, run) in cases {
        d.alert = None;
        f.fail_next(call, model_err("no"));
        mx(&mut d, run);
        assert_eq!(d.alert.as_ref().map(|a| a.title.as_str()), Some(title));
    }
    d.alert = None;
    f.fail_next("load_plugins", model_err("no"));
    mx(&mut d, |m, d| m.load(d, vec![10]));
    assert_eq!(
        d.alert.as_ref().map(|a| a.title.as_str()),
        Some("Load Failed")
    );
}

#[test]
fn remove_eq_empties_the_slot_and_an_empty_one_sends_nothing() {
    let (f, mut d) = make_with(slots_fake());
    mx(&mut d, |m, d| m.clear_eq(d, 1));
    assert_eq!(f.calls(), ["set_eq(1,None)"]);
    assert!(d.mixer().unwrap().strip(1).unwrap().eq.is_none());
    // Now empty: no call, and no "Clear EQ Failed" from the core's refusal.
    f.set_call_error("set_eq", Some(model_err("empty slot")));
    mx(&mut d, |m, d| {
        m.clear_eq(d, 1);
        m.clear_eq(d, 3);
        m.clear_eq(d, 99);
    });
    assert_eq!(f.calls().len(), 1);
    assert!(d.alert.is_none());
}

#[test]
fn safe_mode_loads_a_strips_held_slots() {
    let f = slots_fake();
    f.set_held(vec![13, 12, 14]);
    let (f, mut d) = make_with(f);
    let m = d.mixer().unwrap();
    assert_eq!(
        m.held_slots_of(m.strip(1).unwrap()),
        [12, 13],
        "in slot order"
    );
    assert!(m.is_held(14) && !m.is_held(10));
    mx(&mut d, |m, d| m.load_all(d, 1));
    assert_eq!(f.loaded(), [vec![12, 13]]);
    let m = d.mixer().unwrap();
    assert!(
        m.held_slots_of(m.strip(1).unwrap()).is_empty(),
        "read again"
    );
    assert!(m.is_held(14));
    // A strip with nothing held loads nothing; one slot's Load loads it.
    mx(&mut d, |m, d| {
        m.load_all(d, 1);
        m.load(d, vec![14]);
    });
    assert_eq!(f.loaded(), [vec![12, 13], vec![14]]);
    assert!(!d.mixer().unwrap().is_held(14));
}

#[test]
fn a_busy_load_refetch_is_owed_even_without_a_version_change() {
    let f = slots_fake();
    f.set_held(vec![10]);
    let (f, mut d) = make_with(f);
    f.fail_next("mixer", busy());
    mx(&mut d, |m, d| m.load(d, vec![10]));
    assert!(
        d.mixer().unwrap().is_held(10),
        "the Busy read kept the last"
    );
    assert!(d.mixer().unwrap().needs_refresh(&d));
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert!(!d.mixer().unwrap().is_held(10));
}

#[test]
fn the_mixer_is_read_again_when_the_slots_change() {
    let (f, mut d) = make_with(slots_fake());
    let before = f.mixer_reads();
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(f.mixer_reads(), before, "nothing moved");
    f.set_idle_slots_changed();
    assert!(d.idle(), "the slots' news is worth a redraw");
    assert_eq!(d.slots_version(), 1);
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(f.mixer_reads(), before + 1);
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(f.mixer_reads(), before + 1, "once");
    // A loaded slot's new status shows after the read.
    f.edit_mixer(|m| m.strips[0].inserts[0].status = SlotStatus::Loading);
    f.set_idle_slots_changed();
    d.idle();
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    let m = d.mixer().unwrap();
    assert_eq!(m.slot_state(m.slot(10).unwrap().1), SlotState::Loading);
}

#[test]
fn a_refused_held_read_skips_the_refetch() {
    let (f, mut d) = make_with(slots_fake());
    f.set_held(vec![10]);
    f.fail_next("held_slots", busy());
    mx(&mut d, |m, d| m.toggle_bypass(d, 13));
    assert!(
        !d.mixer().unwrap().is_held(10),
        "Busy: the last state stays"
    );
    assert!(d.alert.is_none(), "quietly");
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert!(d.mixer().unwrap().is_held(10));
}
