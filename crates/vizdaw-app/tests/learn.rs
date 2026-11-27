//! The per-document MIDI learn model (Swift `LearnModelTests`, plan 5f-1
//! Task 11; slice 9d Task 4): the banner, modes and pick-up, refusals,
//! target titles, and closing a learning document cancels its learn.

mod support;

use std::sync::Arc;

use support::{FakeSession, mixer_fake, slot_row, strip_row, vca_row};
use vizdaw_app::learn::{
    self, FAILED_TITLE, LearnModel, MISSING, MODES, NO_MAPS, PICKUP_HELP, WAITING_TEXT,
};
use vizdaw_app::testing::FakeCore;
use vizdaw_app::{DocumentModel, DocumentStore, WindowToken};
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::mixer::{BusRow, MixerSnapshot, SendRow, StripKindRow};
use vizdaw_ffi::snapshot::ParamRow;
use vizdaw_ffi::surfaces::{
    LearnKindRow, LearnMapRow, LearnModeRow, LearnSourceRow, SurfaceStateRow,
};

fn model() -> (Arc<FakeSession>, DocumentModel, FakeCore) {
    let f = Arc::new(mixer_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m, FakeCore::new())
}

fn row(mode: LearnModeRow, pickup: bool, picks_up: bool) -> LearnMapRow {
    LearnMapRow {
        source: LearnSourceRow {
            port: "Knobs".into(),
            kind: LearnKindRow::Cc7,
            channel: 0,
            number: 7,
        },
        text: "Knobs · CC 7 ch 1".into(),
        target: AutomationTargetRow::StripFader { strip: 1 },
        missing: false,
        mode,
        pickup,
        picks_up,
    }
}

fn plain() -> LearnMapRow {
    row(LearnModeRow::Absolute, true, true)
}

/// The session's surface state says a learn waits for `what`.
fn waiting(f: &FakeSession, what: Option<&str>) {
    f.set_surface_state(SurfaceStateRow {
        learning: what.map(str::to_owned),
        ..SurfaceStateRow::default()
    });
}

fn model_error(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

#[test]
fn learning_shows_a_banner_until_the_map_lands() {
    let (f, mut m, _) = model();
    waiting(&f, Some("Fader 1"));
    m.learn_target(AutomationTargetRow::StripFader { strip: 1 });
    assert_eq!(
        f.learn_targets(),
        vec![AutomationTargetRow::StripFader { strip: 1 }]
    );
    assert!(m.learn.is_learning() && m.learn.is_active());
    assert!(m.learn.banner_text(None).is_some());
    f.set_learn_maps(vec![plain()]);
    waiting(&f, None);
    m.idle();
    assert_eq!(m.learn.banner_text(None), None);
    assert!(!m.learn.is_learning());
    let texts: Vec<&str> = m.learn.maps().iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, vec!["Knobs · CC 7 ch 1"]);
}

#[test]
fn cancel_ends_the_learn() {
    let (f, mut m, core) = model();
    waiting(&f, Some("Undo"));
    m.learn_command("edit.undo", &core);
    assert_eq!(core.learned_commands(), vec!["edit.undo".to_owned()]);
    assert!(m.learn.is_learning());
    // Final review T11 m8: the learn's document took the surfaces' focus.
    assert_eq!(f.focus_calls(), 1);
    m.cancel_learn(&core);
    assert_eq!(core.learn_cancels(), 1);
    assert!(!m.learn.is_learning() && m.learn.banner_text(None).is_none());
}

/// Final review R2 and T11 m9: the banner names what the learn waits
/// for and, with two or more projects open, the project.
#[test]
fn the_banner_names_what_it_waits_for_and_the_project() {
    assert_eq!(learn::waiting(None, None), WAITING_TEXT);
    assert_eq!(
        WAITING_TEXT,
        "Waiting for MIDI\u{2026} move a control (Esc cancels)"
    );
    assert_eq!(
        learn::waiting(Some("Song"), None),
        "Waiting for MIDI in \u{201C}Song\u{201D}\u{2026} move a control (Esc cancels)"
    );
    assert_eq!(
        learn::waiting(Some("Song"), Some("Undo")),
        "Waiting for MIDI for Undo in \u{201C}Song\u{201D}\u{2026} move a control (Esc cancels)"
    );
    let (f, mut m, _) = model();
    waiting(&f, Some("Fader 1"));
    m.learn_target(AutomationTargetRow::StripFader { strip: 1 });
    assert_eq!(
        m.learn.banner_text(None).as_deref(),
        Some("Waiting for MIDI for Fader 1\u{2026} move a control (Esc cancels)")
    );
    assert_eq!(
        m.learn.banner_text(Some("Song")).as_deref(),
        Some(
            "Waiting for MIDI for Fader 1 in \u{201C}Song\u{201D}\u{2026} move a control (Esc cancels)"
        )
    );
}

#[test]
fn modes_and_removal_reach_the_session() {
    let (f, mut m, _) = model();
    let r = plain();
    m.set_learn_mode(&r, LearnModeRow::RelativeBinOffset);
    assert_eq!(
        f.learn_modes(),
        vec![(r.source.clone(), LearnModeRow::RelativeBinOffset)]
    );
    m.remove_learn(&r);
    assert_eq!(f.removed_learns(), vec![r.source.clone()]);
    f.fail_next("set_learn_mode", model_error("unknown learn source"));
    m.set_learn_mode(&r, LearnModeRow::Absolute);
    assert_eq!(
        m.alert.as_ref().map(|a| a.title.as_str()),
        Some("MIDI Learn Failed")
    );
}

/// Owner answer 2026-11-02 (Ruling 18): pick-up per map, set in the
/// sheet, only where it applies.
#[test]
fn pick_up_is_shown_and_set() {
    let (f, mut m, _) = model();
    m.set_learn_pickup(&plain(), false);
    let ons: Vec<bool> = f.learn_pickups().iter().map(|p| p.1).collect();
    assert_eq!(ons, vec![false]);
    m.set_learn_pickup(&row(LearnModeRow::RelativeBinOffset, false, false), true);
    assert_eq!(f.learn_pickups().len(), 1, "a relative map has no pick-up");
    assert_eq!(
        PICKUP_HELP,
        "The control takes over only once it reaches the current value"
    );
}

#[test]
fn mode_titles() {
    assert_eq!(learn::mode_title(LearnModeRow::Absolute), "Absolute");
    assert_eq!(
        learn::mode_title(LearnModeRow::Relative2sComplement),
        "Relative (2's complement)"
    );
    assert_eq!(
        learn::mode_title(LearnModeRow::RelativeBinOffset),
        "Relative (binary offset)"
    );
    assert_eq!(
        MODES,
        [
            LearnModeRow::Absolute,
            LearnModeRow::Relative2sComplement,
            LearnModeRow::RelativeBinOffset
        ]
    );
    assert_eq!(
        NO_MAPS,
        "No MIDI controls are learned in this project. Choose MIDI Learn in a control's menu."
    );
    assert_eq!(MISSING, "(missing)");
}

/// Step 3: a refused learn raises "MIDI Learn Failed" and shows no banner.
#[test]
fn a_refused_learn_is_an_alert() {
    let (f, mut m, core) = model();
    waiting(&f, Some("Open"));
    core.fail_learn_command(model_error("\u{201C}file.open\u{201D} cannot be learned"));
    m.learn_command("file.open", &core);
    assert_eq!(FAILED_TITLE, "MIDI Learn Failed");
    assert_eq!(
        m.alert.as_ref().map(|a| a.title.as_str()),
        Some(FAILED_TITLE)
    );
    assert!(!m.learn.is_learning() && m.learn.banner_text(None).is_none());
    // A refused target learn too.
    m.alert = None;
    f.fail_next("learn_target", model_error("no such strip"));
    m.learn_target(AutomationTargetRow::StripFader { strip: 9 });
    assert_eq!(
        m.alert.as_ref().map(|a| a.title.as_str()),
        Some(FAILED_TITLE)
    );
    assert!(!m.learn.is_learning());
}

/// Review Focus 2: closing the document whose learn waits cancels it, so
/// no learn lands in a closed session; closing another cancels nothing.
#[test]
fn closing_the_learning_document_cancels_the_learn() {
    let core = Arc::new(FakeCore::new());
    let a = Arc::new(mixer_fake());
    let b = Arc::new(mixer_fake());
    core.hand_out("new_demo", a.clone());
    core.hand_out("new_demo", b.clone());
    let mut store = DocumentStore::with_core(core.clone());
    let da = store.new_demo().unwrap();
    let db = store.new_demo().unwrap();
    store.focus_document(db, WindowToken(2));
    assert_eq!(store.learning_document(), None);
    waiting(&a, Some("Fader 1"));
    waiting(&b, Some("Fader 1"));
    store
        .document_mut(da)
        .unwrap()
        .learn_target(AutomationTargetRow::StripFader { strip: 1 });
    assert_eq!(store.learning_document(), Some(da));
    store.close(db);
    assert_eq!(
        core.learn_cancels(),
        0,
        "the closed document was not learning"
    );
    store.close(da);
    assert_eq!(core.learn_cancels(), 1);
    assert_eq!(store.learning_document(), None);
}

#[test]
fn an_edit_is_one_undo_step() {
    let (f, mut m, _) = model();
    f.set_learn_maps(vec![plain()]);
    m.set_learn_mode(&plain(), LearnModeRow::Relative2sComplement);
    assert_eq!(m.snapshot().undo_label.as_deref(), Some("MIDI Learn"));
    assert_eq!(m.learn.maps()[0].mode, LearnModeRow::Relative2sComplement);
    m.remove_learn(&plain());
    assert_eq!(
        m.snapshot().undo_label.as_deref(),
        Some("Remove MIDI Learn")
    );
    assert!(m.learn.maps().is_empty());
}

/// Ruling 5: with the MIDI Learn Maps sheet open, a refusal is the
/// sheet's line, not an alert behind it.
#[test]
fn a_refusal_with_the_sheet_open_is_its_line() {
    let (f, mut m, _) = model();
    f.set_learn_maps(vec![plain()]);
    m.learn.open_maps();
    assert!(m.learn.maps_open() && m.learn.is_active());
    assert_eq!(m.learn.maps().len(), 1, "opening reads the maps");
    f.fail_next("remove_learn", model_error("unknown learn source"));
    m.remove_learn(&plain());
    assert!(m.alert.is_none());
    assert_eq!(m.learn.problem(), Some("unknown learn source"));
    m.learn.close_maps();
    assert!(!m.learn.maps_open() && !m.learn.is_active());
    assert_eq!(m.learn.problem(), None);
    // Closed, a refusal is an alert again.
    f.fail_next("remove_learn", model_error("unknown learn source"));
    m.remove_learn(&plain());
    assert_eq!(
        m.alert.as_ref().map(|a| a.title.as_str()),
        Some(FAILED_TITLE)
    );
}

fn param(id: u64, name: &str) -> ParamRow {
    ParamRow {
        id,
        name: name.into(),
        module: String::new(),
        min: 0.0,
        max: 1.0,
        default: 0.5,
        value: 0.5,
        stepped: false,
        automatable: true,
        hidden: false,
        read_only: false,
    }
}

/// `M/StripLane.swift:155-173`: what a learned target is called.
#[test]
fn target_titles_name_strips_sends_and_params() {
    let f = Arc::new(mixer_fake());
    let mut synth = strip_row(1, "Synth", StripKindRow::BuiltinSynth);
    synth.sends = vec![SendRow {
        target: 2,
        level_db: 0.0,
        pre_fader: false,
        enabled: true,
    }];
    synth.inserts = vec![slot_row(10, "Echo")];
    f.set_mixer(MixerSnapshot {
        version: 0,
        strips: vec![
            synth,
            strip_row(2, "Bus 1", StripKindRow::Bus),
            strip_row(0, "Master", StripKindRow::Master),
        ],
        buses: vec![BusRow {
            id: 2,
            name: "Bus 1".into(),
        }],
        groups: vec![],
        vcas: vec![vca_row(5, "Drums", &[1], 0.0)],
    });
    f.set_params(10, vec![param(3, "Time")]);
    let m = DocumentModel::new(f.clone()).unwrap();
    let t = |t: AutomationTargetRow| learn::target_title(&t, &m);
    assert_eq!(
        t(AutomationTargetRow::StripFader { strip: 1 }),
        "Fader · Synth"
    );
    assert_eq!(t(AutomationTargetRow::StripPan { strip: 1 }), "Pan · Synth");
    assert_eq!(
        t(AutomationTargetRow::StripMute { strip: 2 }),
        "Mute · Bus 1"
    );
    assert_eq!(
        t(AutomationTargetRow::StripSolo { strip: 0 }),
        "Solo · Master"
    );
    assert_eq!(
        t(AutomationTargetRow::SendLevel { strip: 1, send: 0 }),
        "Send 1 to Bus 1 · Synth"
    );
    assert_eq!(
        t(AutomationTargetRow::SendLevel { strip: 1, send: 4 }),
        "Send 5 · Synth"
    );
    assert_eq!(
        t(AutomationTargetRow::PluginParam { slot: 10, param: 3 }),
        "Time · Echo"
    );
    assert_eq!(
        t(AutomationTargetRow::PluginParam { slot: 10, param: 8 }),
        "Parameter 8 · Echo"
    );
    assert_eq!(
        t(AutomationTargetRow::PluginParam { slot: 77, param: 1 }),
        "Parameter 1 · Plugin 77"
    );
    assert_eq!(
        t(AutomationTargetRow::VcaFader { vca: 5 }),
        "VCA Drums Fader"
    );
    assert_eq!(
        t(AutomationTargetRow::StripFader { strip: 42 }),
        "Fader · Strip 42"
    );
}

/// The learn model of a document no one asked to learn is idle.
#[test]
fn a_new_document_is_not_learning() {
    let (_, m, _) = model();
    let l: &LearnModel = &m.learn;
    assert!(!l.is_learning() && !l.is_active() && !l.maps_open());
    assert_eq!(l.banner_text(Some("Song")), None);
    assert!(l.problem().is_none());
}
