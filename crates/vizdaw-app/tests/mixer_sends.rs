//! The mixer's sends, outputs and strip colours (Swift `MixerModelTests`
//! `busesGetFreeNamesAndEditsReachTheSession`,
//! `masterIsSeparateAndOutputsAreNamed`, `identicalCommandAlertsAreNotReSet`,
//! `StripColourTests` `theColourMenuListsNoneAndTheEightSwatchesInOrder`),
//! and the send index rule: 1-based wherever a person or the channel
//! reads it, 0-based at the session.

mod support;

use std::sync::Arc;
use std::time::Instant;

use core_control::registry::ArgValue;
use support::{FakeSession, mixer_fake, strip_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands;
use vizdaw_app::mixer::gesture::DRAG_TRAVEL;
use vizdaw_app::mixer::{MixerModel, MixerPlacement, SendLabels, StripPalette, send_index};
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::GesturePhaseRow;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{BusRow, SendRow, StripKindRow, SwatchRow};

fn send(target: u64, level_db: f64) -> SendRow {
    SendRow {
        target,
        level_db,
        pre_fader: false,
        enabled: true,
    }
}

/// Synth (1) sends to Bus 1 (2) at 0 dB and to Bus 2 (3) at −6 dB.
fn two_sends() -> FakeSession {
    let f = mixer_fake();
    f.edit_mixer(|m| {
        m.strips.insert(2, strip_row(3, "Bus 2", StripKindRow::Bus));
        m.buses.push(BusRow {
            id: 3,
            name: "Bus 2".into(),
        });
        m.strips[0].sends = vec![send(2, 0.0), send(3, -6.0)];
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

#[test]
fn send_edits_reach_the_session_counting_from_zero() {
    let (f, mut d) = make_with(two_sends());
    mx(&mut d, |m, d| {
        m.add_send(d, 1, 3);
        // The second send (shown as "Send 2") is index 1 at the session.
        m.set_send_pre_fader(d, 1, 1, true);
        m.set_send_enabled(d, 1, 0, false);
        m.remove_send(d, 1, 1);
    });
    assert_eq!(
        f.calls(),
        [
            "add_send(1,3)",
            "edit_send(1,1,3,-6.0,true,true)",
            "edit_send(1,0,2,0.0,false,false)",
            "remove_send(1,1)",
        ]
    );
    // The mixer is read again after each: the fake's edits show.
    let m = d.mixer().unwrap();
    let synth = m.strip(1).unwrap();
    assert_eq!(synth.sends.len(), 2, "one added, one removed");
    assert!(!synth.sends[0].enabled);
    assert_eq!(synth.sends[1].target, 3, "the added send is last");
    assert!(d.alert.is_none());
}

#[test]
fn a_send_that_is_not_there_sends_nothing() {
    let (f, mut d) = make_with(two_sends());
    mx(&mut d, |m, d| {
        m.set_send_pre_fader(d, 1, 2, true);
        m.set_send_enabled(d, 9, 0, false);
    });
    assert!(f.calls().is_empty());
}

#[test]
fn refused_send_edits_alert_send_failed_once() {
    for call in ["add_send", "edit_send", "remove_send"] {
        let (f, mut d) = make_with(two_sends());
        f.set_call_error(call, Some(model_err("Strip 9 is not in this song.")));
        let reads = f.mixer_reads();
        let run = |m: &mut MixerModel, d: &mut DocumentModel| match call {
            "add_send" => m.add_send(d, 1, 3),
            "edit_send" => m.set_send_pre_fader(d, 1, 0, true),
            _ => m.remove_send(d, 1, 0),
        };
        mx(&mut d, run);
        let alert = d.alert.clone().expect(call);
        assert_eq!(alert.title, "Send Failed", "{call}");
        assert_eq!(alert.message, "Strip 9 is not in this song.", "{call}");
        assert!(f.mixer_reads() > reads, "{call}: the mixer is read again");
        mx(&mut d, run);
        assert_eq!(
            d.alert.as_ref().map(|a| a.id),
            Some(alert.id),
            "{call}: the same alert is not raised twice"
        );
    }
}

#[test]
fn send_indexes_count_from_one_for_people_and_from_zero_for_the_core() {
    assert_eq!(SendLabels::knob(0, "Synth"), "Send knob 1 of Synth");
    assert_eq!(SendLabels::level_box(1, "Synth"), "Send 2 of Synth");
    assert_eq!(SendLabels::add_menu("Synth"), "Add a send to Synth");
    assert_eq!(SendLabels::new_bus_plus("Synth"), "New bus fed by Synth");
    assert_eq!(
        SendLabels::NEW_BUS_HELP,
        "New Bus: a bus with a send from this strip"
    );
    assert_eq!(SendLabels::NEW_BUS, "New Bus");
    assert_eq!(SendLabels::PRE_FADER, "Pre-Fader");
    assert_eq!(SendLabels::ON, "On");
    assert_eq!(SendLabels::REMOVE, "Remove Send");
    assert_eq!(SendLabels::PRE_CHIP, "PRE");
    // The automation target of the first send is send 0.
    assert_eq!(send_index(0), 0);
    let (_f, d) = make_with(two_sends());
    let m = d.mixer().unwrap();
    assert_eq!(m.send_target(1, 0), T::SendLevel { strip: 1, send: 0 });
    assert_eq!(m.send_name(&m.strip(1).unwrap().sends[1]), "Bus 2");
    assert_eq!(
        m.send_level_db(m.strip(1).unwrap(), 1),
        -6.0,
        "index 1 is the second send"
    );
}

#[test]
fn outputs_offer_master_then_the_other_buses() {
    let f = two_sends();
    f.edit_mixer(|m| m.strips[0].output = Some(3));
    let (f, mut d) = make_with(f);
    let m = d.mixer().unwrap();
    let synth = m.strip(1).unwrap();
    let choices = m.output_choices(synth);
    let titles: Vec<_> = choices.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Master", "Bus 1", "Bus 2"]);
    let checked: Vec<_> = choices.iter().map(|c| c.checked).collect();
    assert_eq!(
        checked,
        [false, false, true],
        "the current output is checked"
    );
    assert_eq!(choices[0].bus, None);
    assert_eq!(choices[1].bus, Some(2));
    assert_eq!(m.output_text(synth), "\u{2192} Bus 2");
    // A bus offers neither itself as output nor as a send.
    let bus = m.strip(2).unwrap();
    let titles: Vec<_> = m
        .output_choices(bus)
        .iter()
        .map(|c| c.title.clone())
        .collect();
    assert_eq!(titles, ["Master", "Bus 2"]);
    assert_eq!(m.output_text(bus), "\u{2192} Master");
    // Master has no output, send or pan rows.
    let master = m.master().unwrap();
    assert!(!MixerModel::routes(master));
    assert!(MixerModel::routes(synth));
    assert!(m.output_choices(master).is_empty());
    mx(&mut d, |m, d| {
        m.set_output(d, 1, None);
        m.add_send(d, 0, 2);
    });
    assert_eq!(
        f.calls(),
        ["set_output(1,None)"],
        "master gets no send from the model"
    );
}

#[test]
fn new_bus_fed_by_a_strip_adds_its_send() {
    let (f, mut d) = make_with(mixer_fake());
    mx(&mut d, |m, d| m.new_bus(d, 1));
    assert_eq!(f.calls(), ["new_bus_from(1,Bus 2)"]);
    let m = d.mixer().unwrap();
    let synth = m.strip(1).unwrap();
    assert_eq!(synth.sends.len(), 1);
    assert_eq!(m.send_name(&synth.sends[0]), "Bus 2");
    assert!(!synth.sends[0].pre_fader, "post-fader");
    assert_eq!(synth.sends[0].level_db, 0.0);
}

#[test]
fn the_colour_menu_lists_none_and_the_eight_swatches_in_order() {
    assert_eq!(StripPalette::NO_COLOUR_TITLE, "None");
    let titles: Vec<_> = StripPalette::SWATCHES
        .iter()
        .map(|c| StripPalette::title(*c))
        .collect();
    assert_eq!(
        titles,
        [
            "Red", "Orange", "Yellow", "Green", "Teal", "Blue", "Purple", "Pink"
        ]
    );
    assert_eq!(StripPalette::named("teal"), Some(SwatchRow::Teal));
    assert_eq!(StripPalette::named("pink"), Some(SwatchRow::Pink));
    assert_eq!(StripPalette::named("Teal"), None, "the channel's words");
    assert_eq!(StripPalette::named("none"), None);
    assert_eq!(StripPalette::named("cyan"), None);
    // Every registry colour but `none` is a swatch, in the same order.
    let words: Vec<_> = core_control::registry::STRIP_COLOURS[1..].to_vec();
    let named: Vec<_> = StripPalette::SWATCHES
        .iter()
        .map(|c| StripPalette::title(*c).to_lowercase())
        .collect();
    assert_eq!(named, words);
}

#[test]
fn a_colour_change_is_one_session_call_and_a_refusal_alerts() {
    let (f, mut d) = make_with(mixer_fake());
    mx(&mut d, |m, d| {
        m.set_colour(d, 1, Some(SwatchRow::Red));
        m.set_colour(d, 1, None);
    });
    assert_eq!(
        f.calls(),
        ["set_strip_colour(1,Some(Red))", "set_strip_colour(1,None)"]
    );
    f.set_call_error("set_strip_colour", Some(model_err("no")));
    mx(&mut d, |m, d| m.set_colour(d, 1, Some(SwatchRow::Blue)));
    assert_eq!(d.alert.as_ref().unwrap().title, "Strip Colour Failed");
}

/// Holds Synth's first send knob (Bus 1) through the tracker and moves it.
fn hold_first_send(m: &mut MixerModel, d: &mut DocumentModel, now: Instant) {
    let t = m.send_target(1, 0);
    m.press(d, MixerPlacement::View, t, DRAG_TRAVEL, false, now);
    m.drag(d, MixerPlacement::View, 0.0, -10.0, false, now);
    assert!(m.is_touched(t));
}

/// After the held send moved away: the mouse moves on and comes up.
fn move_on_and_lift(d: &mut DocumentModel, now: Instant) {
    mx(d, |m, d| {
        m.refresh_if_needed(d);
        m.drag(d, MixerPlacement::View, 0.0, -30.0, false, now);
        m.drag(d, MixerPlacement::View, 0.0, -50.0, false, now);
        m.lift(d, MixerPlacement::View, now);
    });
}

fn send_phases(f: &FakeSession) -> Vec<GesturePhaseRow> {
    f.gestures().iter().map(|g| g.1).collect()
}

#[test]
fn a_held_send_knob_lets_go_when_the_channel_removes_its_send() {
    let (f, mut d) = make_with(two_sends());
    let now = Instant::now();
    mx(&mut d, |m, d| hold_first_send(m, d, now));
    let before = send_phases(&f).len();
    // The channel removes send 1 (Bus 1): Bus 2's send is index 0 now.
    f.edit_mixer(|m| {
        m.strips[0].sends.remove(0);
    });
    let args = [
        ("strip".to_owned(), ArgValue::Int(1)),
        ("index".to_owned(), ArgValue::Int(1)),
    ];
    commands::run("mixer.remove_send", &args, &mut d).expect("the channel's edit");
    move_on_and_lift(&mut d, now);
    let after = &send_phases(&f)[before..];
    assert_eq!(
        after,
        [GesturePhaseRow::Release],
        "released once, nothing else"
    );
    let first = T::SendLevel { strip: 1, send: 0 };
    assert!(f.gestures()[before..].iter().all(|g| g.0 == first));
    assert!(!d.mixer().unwrap().is_touched(first));
    assert!(!d.mixer().unwrap().is_dragging(MixerPlacement::View));
    assert!(d.alert.is_none());
}

#[test]
fn a_held_send_knob_lets_go_when_undo_moves_its_send() {
    // Bus 1's send was removed: Bus 2's is send 1, and its knob is held.
    let f = two_sends();
    f.edit_mixer(|m| {
        m.strips[0].sends.remove(0);
    });
    let (f, mut d) = make_with(f);
    let now = Instant::now();
    mx(&mut d, |m, d| hold_first_send(m, d, now));
    let before = send_phases(&f).len();
    // ⌘Z while the mouse is down brings Bus 1's send back at index 0.
    f.edit_mixer(|m| m.strips[0].sends.insert(0, send(2, 0.0)));
    d.undo();
    move_on_and_lift(&mut d, now);
    assert_eq!(
        &send_phases(&f)[before..],
        [GesturePhaseRow::Release],
        "released once; Bus 1's send is not moved"
    );
    let m = d.mixer().unwrap();
    assert_eq!(m.strip(1).unwrap().sends[0].level_db, 0.0);
    assert!(!m.is_dragging(MixerPlacement::View));
}

#[test]
fn a_held_send_knob_lets_go_when_undo_of_its_add_removes_it() {
    let (f, mut d) = make_with(two_sends());
    let now = Instant::now();
    mx(&mut d, |m, d| {
        let t = m.send_target(1, 1);
        m.press(d, MixerPlacement::View, t, DRAG_TRAVEL, false, now);
        m.drag(d, MixerPlacement::View, 0.0, -10.0, false, now);
    });
    let before = send_phases(&f).len();
    // ⌘Z of the Add Send that made it.
    f.edit_mixer(|m| {
        m.strips[0].sends.pop();
    });
    d.undo();
    move_on_and_lift(&mut d, now);
    assert_eq!(&send_phases(&f)[before..], [GesturePhaseRow::Release]);
}

#[test]
fn a_held_send_knob_keeps_its_drag_when_its_send_stays() {
    let (f, mut d) = make_with(two_sends());
    let now = Instant::now();
    mx(&mut d, |m, d| hold_first_send(m, d, now));
    let before = send_phases(&f).len();
    // A later send is removed: send 1 is still Bus 1's.
    f.edit_mixer(|m| {
        m.strips[0].sends.pop();
    });
    d.undo();
    mx(&mut d, |m, d| {
        m.refresh_if_needed(d);
        m.drag(d, MixerPlacement::View, 0.0, -30.0, false, now);
    });
    assert!(matches!(
        send_phases(&f)[before..],
        [GesturePhaseRow::Value { .. }]
    ));
    assert!(d.mixer().unwrap().is_dragging(MixerPlacement::View));
}
