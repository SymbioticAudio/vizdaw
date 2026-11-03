//! Surfaces in the core (plan 5f-1 Tasks 7–8; spec §8, §12.1–§12.3).
//! The fake port transport only: no CoreMIDI client is created.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::surfaces::{
    SurfaceKindRow as Kind, debug_advance, debug_reset, debug_sent, debug_set_missing,
    debug_stale_sources, debug_support_file, surface_add, surface_list, surface_remove,
    surfaces_load,
};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::set_fake_input(None);
    vizdaw_ffi::host::reset_host();
    debug_reset();
    let _ = std::fs::remove_file(debug_support_file());
    g
}

#[test]
fn surfaces_are_added_listed_saved_and_removed() {
    let _g = serial();
    let a = surface_add(Kind::Mcu, "MCU In".into(), "MCU Out".into(), None).unwrap();
    let b = surface_add(Kind::McuXt, "XT In".into(), "XT Out".into(), None).unwrap();
    let list = surface_list();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0].id, list[0].position, list[1].position), (a, 0, 1));
    assert!(list.iter().all(|s| s.connected), "fake ports are present");
    assert_eq!(
        debug_sent("MCU Out").first(),
        Some(&vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7]),
        "the device query on connect"
    );
    let saved = std::fs::read_to_string(debug_support_file()).unwrap();
    assert!(
        saved.contains("\"version\": 1") || saved.contains("\"version\":1"),
        "{saved}"
    );
    debug_reset();
    surfaces_load().unwrap();
    assert_eq!(surface_list().len(), 2, "read back");
    surface_remove(b).unwrap();
    assert_eq!(surface_list().len(), 1);
    assert!(surface_remove(b).is_err());
    let c = surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    assert_eq!((a, b, c), (0, 1, 1), "IDs are the lowest free ones");
}

#[test]
fn a_fifth_unit_or_a_second_main_is_refused() {
    let _g = serial();
    surface_add(Kind::Mcu, "A".into(), "A".into(), None).unwrap();
    assert!(surface_add(Kind::Mcu, "B".into(), "B".into(), None).is_err());
    surface_add(Kind::McuXt, "X0".into(), "X0".into(), Some(2)).unwrap();
    let e = surface_add(Kind::McuXt, "X1".into(), "X1".into(), Some(2)).unwrap_err();
    assert!(e.to_string().contains("position 2 is taken"), "{e}");
    for n in 1..3 {
        surface_add(Kind::McuXt, format!("X{n}"), format!("X{n}"), None).unwrap();
    }
    let e = surface_add(Kind::McuXt, "X4".into(), "X4".into(), None).unwrap_err();
    assert!(e.to_string().contains("up to three extenders"), "{e}");
    surface_add(Kind::Hui, "H".into(), "H".into(), None).unwrap();
}

#[test]
fn hostile_surface_calls_are_refused() {
    let _g = serial();
    for (i, o) in [
        ("", "x"),
        ("x", ""),
        (&*"p".repeat(256), "x"),
        ("a\nb", "x"),
    ] {
        assert!(
            surface_add(Kind::Hui, i.to_string(), o.to_string(), None).is_err(),
            "{i:?} {o:?}"
        );
    }
    assert!(
        surface_add(Kind::McuXt, "x".into(), "x".into(), Some(9)).is_err(),
        "positions 0–3"
    );
    assert!(surface_remove(u32::MAX).is_err());
    assert!(surface_list().is_empty());
    surface_add(Kind::Hui, "P In".into(), "P Out".into(), None).unwrap();
    let e = surface_add(Kind::Hui, "P In".into(), "Other Out".into(), None).unwrap_err();
    assert!(e.to_string().contains("is used by surface"), "{e}");
    assert!(
        surface_add(Kind::Hui, "Other In".into(), "P Out".into(), None).is_err(),
        "outputs too"
    );
    for n in 1..16 {
        surface_add(Kind::Hui, format!("H{n} In"), format!("H{n} Out"), None).unwrap();
    }
    let e = surface_add(Kind::Hui, "H16 In".into(), "H16 Out".into(), None).unwrap_err();
    assert!(e.to_string().contains("at most 16 surfaces"), "{e}");
    assert!(surface_list().iter().all(|s| s.id < 16));
}

#[test]
fn a_vanished_port_disconnects() {
    let _g = serial();
    let id = surface_add(Kind::Mcu, "U In".into(), "U Out".into(), None).unwrap();
    debug_set_missing(vec!["U In".into()]);
    debug_advance(2_100);
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    s.idle().unwrap();
    let row = surface_list().into_iter().find(|r| r.id == id).unwrap();
    assert!(!row.connected);
    // The touch half is Task 8's `a_surface_touch_is_released_when_its_port_goes`.
}

/// Pre-flight I2: CoreMIDI's source list changes only at a refresh, so
/// the refresh must run while every unit is connected.
#[test]
fn a_vanished_port_is_seen_while_connected() {
    let _g = serial();
    debug_stale_sources(true);
    let id = surface_add(Kind::Mcu, "V In".into(), "V Out".into(), None).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    debug_set_missing(vec!["V In".into()]);
    debug_advance(100);
    s.idle().unwrap();
    let connected = || {
        surface_list()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap()
            .connected
    };
    assert!(connected(), "the stale source list still has it");
    debug_advance(2_100);
    s.idle().unwrap();
    assert!(
        !connected(),
        "the 2 s refresh runs while connected and sees it go"
    );
}

#[test]
fn a_damaged_surfaces_file_is_kept_aside() {
    let _g = serial();
    let f = debug_support_file();
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "{ not json").unwrap();
    surfaces_load().unwrap();
    assert!(surface_list().is_empty());
    assert!(f.with_extension("json.bad").exists());
    std::fs::write(&f, r#"{"version": 99, "surfaces": [], "commands": []}"#).unwrap();
    surfaces_load().unwrap();
    assert!(
        std::fs::read_to_string(f.with_extension("json.bad"))
            .unwrap()
            .contains("99")
    );
    for (bad, why) in [
        (
            r#"{"version": 1, "surfaces": [{"id": 1000, "kind": "hui", "input": "a", "output": "b", "position": 0}], "commands": []}"#,
            "an ID above 15",
        ),
        (
            r#"{"version": 1, "surfaces": [{"id": 0, "kind": "mcu", "input": "a", "output": "b", "position": 0}, {"id": 1, "kind": "mcu", "input": "c", "output": "d", "position": 1}], "commands": []}"#,
            "two main units",
        ),
        (
            r#"{"version": 1, "surfaces": [{"id": 0, "kind": "hui", "input": "a", "output": "b", "position": 0}, {"id": 1, "kind": "hui", "input": "a", "output": "d", "position": 0}], "commands": []}"#,
            "a shared port",
        ),
    ] {
        let _ = std::fs::remove_file(f.with_extension("json.bad"));
        std::fs::write(&f, bad).unwrap();
        surfaces_load().unwrap();
        assert!(surface_list().is_empty(), "{why}");
        assert!(f.with_extension("json.bad").exists(), "{why}");
    }
}

pub fn keep(_: Arc<vizdaw_ffi::Session>) {
    let _ = common::wait_limit();
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = std::time::Instant::now() + common::wait_limit();
    while !ok() {
        assert!(
            std::time::Instant::now() < until,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn song(s: &vizdaw_ffi::Session) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

fn synth(s: &vizdaw_ffi::Session) -> (u64, usize) {
    let m = s.mixer().unwrap();
    let i = m
        .strips
        .iter()
        .position(|x| x.name == "Built-in Synth")
        .unwrap();
    (m.strips[i].id, i)
}

fn mcu(s: &vizdaw_ffi::Session) -> u32 {
    let id = surface_add(Kind::Mcu, "T In".into(), "T Out".into(), None).unwrap();
    s.focus_surfaces().unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let _ = debug_sent("T Out");
    id
}

/// The values (0–16383) sent to fader channel `ch` on `port`, in order.
fn faders(port: &str, ch: u8) -> Vec<u16> {
    debug_sent(port)
        .iter()
        .filter(|m| m.len() == 3 && m[0] == 0xE0 + ch)
        .map(|m| u16::from(m[1]) | u16::from(m[2]) << 7)
        .collect()
}

/// A note held for 64 beats on the synth, and a bus whose fader ramps
/// 0.2 → 0.8 over 32 beats (`tests/automation.rs`' `ramped_bus`):
/// (session, sequence, bus).
fn ramped_bus() -> (Arc<vizdaw_ffi::Session>, u64, u64) {
    use core_engine::fixtures::{Builder, Q};
    use core_model::automation::AutomationTarget;
    use core_model::event::EventBody;
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(synth)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 64 * Q)]).unwrap();
    let bus = b.bus("Bus").unwrap();
    let auto = b.lane(seq, "Auto", None).unwrap();
    for (tick, v) in [(0, 0.2), (32 * Q, 0.8)] {
        b.event(
            seq,
            auto,
            tick,
            EventBody::Automation {
                target: AutomationTarget::StripFader(bus),
                value: v,
            },
        )
        .unwrap();
    }
    (
        vizdaw_ffi::Session::from_document(b.finish()).unwrap(),
        seq.0,
        bus.0,
    )
}

#[test]
fn an_mcu_fader_ride_is_one_static_step_and_its_mute_a_click() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8; // the synth's index in the display order (the demo's strips fit one bank)
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F, 0xE0 + ch, 0x00, 0x20])
        .unwrap();
    let st = s.automation_state().unwrap();
    assert_eq!(
        st.touched,
        vec![vizdaw_ffi::AutomationTargetRow::StripFader { strip: synth }]
    );
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x00]).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.inject_surface(id, vec![0x90, 0x10 + ch, 0x7F]).unwrap();
    assert!(s.mixer().unwrap().strips[k].mute);
    debug_advance(30);
    s.idle().unwrap();
    let sent = debug_sent("T Out");
    assert!(
        sent.contains(&vec![0x90, 0x10 + ch, 0x7F]),
        "the mute LED follows: {sent:?}"
    );
    let st = s.surface_state().unwrap();
    assert!(
        st.feedback[0]
            .messages
            .iter()
            .any(|m| m == &format!("90 {:02x} 7f", 0x10 + ch))
    );
    assert!(
        st.feedback[0]
            .messages
            .iter()
            .all(|m| !m.starts_with("d0 ")),
        "meters are counted, not listed"
    );
}

#[test]
fn buttons_become_events_for_the_app() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(
        id,
        vec![
            0x90,
            0x5E,
            0x7F,
            0x90,
            0x18 + k as u8,
            0x7F,
            0xB0,
            0x3C,
            0x02,
        ],
    )
    .unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![
            E::Command {
                id: "transport.play".into(),
                programmatic: true
            },
            E::Select { strip: synth },
            E::Jog { detents: 2 },
        ]
    );
    assert!(s.take_surface_events().unwrap().is_empty());
}

#[test]
fn feedback_waits_20_ms_but_partners_go_at_once() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let a = mcu(&s);
    let b = surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let _ = (debug_sent("T Out"), debug_sent("H Out"));
    let ch = k as u8;
    s.inject_surface(a, vec![0x90, 0x68 + ch, 0x7F, 0xE0 + ch, 0x7F, 0x7F])
        .unwrap();
    let hui = debug_sent("H Out");
    assert!(
        hui.contains(&vec![0xB0, ch, 0x7F]),
        "the HUI fader follows at once: {hui:?}"
    );
    debug_advance(30);
    s.idle().unwrap();
    assert!(
        debug_sent("T Out").iter().all(|m| m[0] & 0xF0 != 0xE0),
        "no fader feedback to the hand"
    );
    let _ = b;
}

/// Pre-flight C1: what plays, not the static value, drives the motor.
#[test]
fn motor_faders_follow_automation_playback() {
    let _g = serial();
    let (s, seq, bus) = ramped_bus();
    let k = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .position(|x| x.id == bus)
        .unwrap() as u8;
    let _ = mcu(&s);
    s.play(seq).unwrap();
    let mut seen = Vec::new();
    wait_until("four beats of the ramp", || {
        debug_advance(30);
        s.idle().unwrap();
        seen.extend(faders("T Out", k));
        s.transport().unwrap().tick > 4 * 15_360
    });
    s.stop().unwrap();
    assert!(
        seen.len() >= 3,
        "the fader moved with its automation: {seen:?}"
    );
    assert!(
        seen.windows(2).all(|w| w[1] > w[0]),
        "rising along the ramp: {seen:?}"
    );
    assert!(
        seen[0] >= (0.2 * 16383.0) as u16 - 2 && *seen.last().unwrap() < (0.3 * 16383.0) as u16,
        "0.2 at the start, under 0.3 four beats in: {seen:?}"
    );
}

/// Pre-flight C1: a Touch ride leaves the static value alone, so the
/// partner must follow the heard value, out and back.
#[test]
fn a_touch_ride_moves_the_partner_and_glides_back_on_release() {
    let _g = serial();
    let (s, seq, bus) = ramped_bus();
    let k = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .position(|x| x.id == bus)
        .unwrap() as u8;
    let id = mcu(&s);
    surface_add(Kind::Hui, "H In".into(), "H Out".into(), None).unwrap();
    s.set_automation_mode(vec![bus], vizdaw_ffi::automation::AutomationModeRow::Touch)
        .unwrap();
    s.play(seq).unwrap();
    wait_until("the roll", || s.transport().unwrap().tick > 15_360);
    debug_advance(30);
    s.idle().unwrap();
    let _ = (debug_sent("T Out"), debug_sent("H Out"));
    s.inject_surface(id, vec![0x90, 0x68 + k, 0x7F, 0xE0 + k, 0x7F, 0x7F])
        .unwrap();
    assert!(
        debug_sent("H Out").contains(&vec![0xB0, k, 0x7F]),
        "the HUI follows the ride at once"
    );
    s.inject_surface(id, vec![0x90, 0x68 + k, 0x00]).unwrap();
    let mut his: Vec<u8> = Vec::new();
    wait_until("the glide back to the automation", || {
        debug_advance(30);
        s.idle().unwrap();
        his.extend(
            debug_sent("H Out")
                .into_iter()
                .filter(|m| m.len() == 3 && m[0] == 0xB0 && m[1] == k)
                .map(|m| m[2]),
        );
        his.last().is_some_and(|hi| *hi < 0x40)
    });
    s.stop().unwrap();
    assert!(
        his.windows(2).all(|w| w[1] <= w[0]),
        "falling from the top toward the ramp: {his:?}"
    );
}

#[test]
fn a_surface_touch_and_a_mouse_touch_share_the_target() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8;
    let t = vizdaw_ffi::AutomationTargetRow::StripFader { strip: synth };
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F]).unwrap();
    s.gesture(
        t,
        vizdaw_ffi::automation::GesturePhaseRow::Touch { value: None },
    )
    .unwrap();
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x00]).unwrap();
    assert!(
        s.automation_state().unwrap().touched.contains(&t),
        "the mouse still holds it"
    );
    s.gesture(t, vizdaw_ffi::automation::GesturePhaseRow::Release)
        .unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

/// Pre-flight C2, on a real unit's touch.
#[test]
fn a_resting_surface_touch_does_not_open_an_edit() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8;
    let gain = |s: &vizdaw_ffi::Session| s.mixer().unwrap().strips[k].gain_db;
    let before = gain(&s);
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F]).unwrap();
    s.begin_edit("Drag".into()).unwrap();
    s.end_edit().unwrap();
    s.add_bus("Rest".into()).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Bus"),
        "its own step while the hand rests"
    );
    s.inject_surface(id, vec![0xE0 + ch, 0x00, 0x20, 0x90, 0x68 + ch, 0x00])
        .unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
    s.undo().unwrap();
    assert!(
        (gain(&s) - before).abs() < 1e-9,
        "Undo restores only the fader"
    );
    assert!(
        s.mixer().unwrap().strips.iter().any(|x| x.name == "Rest"),
        "and keeps the bus"
    );
}

#[test]
fn a_surface_touch_is_released_when_its_port_goes() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F])
        .unwrap();
    debug_set_missing(vec!["T In".into()]);
    debug_advance(2_100);
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

#[test]
fn a_returning_port_reconnects_with_a_full_refresh() {
    let _g = serial();
    surface_add(Kind::Mcu, "R In".into(), "R Out".into(), None).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    s.focus_surfaces().unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let full = debug_sent("R Out").len();
    assert!(full > 20);
    debug_set_missing(vec!["R Out".into()]);
    debug_advance(2_100);
    s.idle().unwrap();
    debug_set_missing(vec![]);
    debug_advance(2_100);
    s.idle().unwrap();
    let again = debug_sent("R Out");
    assert!(again.len() >= full, "everything again: {}", again.len());
    assert_eq!(
        again.first(),
        Some(&vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x00, 0xF7])
    );
}

/// Pre-flight I2: the unit forgets its touch at the unplug, so the fader
/// it held moves again after the reconnect.
#[test]
fn a_unit_unplugged_while_touched_gets_fader_feedback_after_reconnect() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    let ch = k as u8;
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F]).unwrap();
    debug_set_missing(vec!["T In".into()]);
    debug_advance(2_100);
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "released at the vanish"
    );
    debug_set_missing(vec![]);
    debug_advance(2_100);
    s.idle().unwrap();
    let sent = debug_sent("T Out");
    assert!(
        sent.iter().any(|m| m.first() == Some(&(0xE0 + ch))),
        "the fader held at the unplug moves again: {sent:?}"
    );
}

#[test]
fn removing_a_surface_releases_its_touches() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(
        id,
        vec![0x90, 0x68 + k as u8, 0x7F, 0xB0, 0x10 + k as u8, 0x01],
    )
    .unwrap();
    assert_eq!(
        s.automation_state().unwrap().touched.len(),
        2,
        "a touched fader and a V-pot's idle hold"
    );
    surface_remove(id).unwrap();
    s.idle().unwrap();
    assert!(s.automation_state().unwrap().touched.is_empty());
}

/// Pre-flight I1: the old session is owed its releases, and takes them
/// in its own idle although it is no longer focused.
#[test]
fn focus_moving_releases_the_old_sessions_surface_and_learn_touches() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    s.learn_target(vizdaw_ffi::AutomationTargetRow::StripPan { strip: synth })
        .unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 64]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    let map = s.learn_maps().unwrap()[0].source.clone();
    s.set_learn_pickup(map, false).unwrap();
    s.inject_midi("Knobs".into(), vec![0xB0, 7, 100]).unwrap();
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F])
        .unwrap();
    s.idle().unwrap();
    assert_eq!(
        s.automation_state().unwrap().touched.len(),
        2,
        "the fader and the learned pan"
    );
    let other = vizdaw_ffi::Session::new_demo().unwrap();
    other.focus_surfaces().unwrap();
    other.idle().unwrap();
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "focus moved: released"
    );
}

/// Pre-flight I1 and M9: closing the focused window ends its touches and
/// its learn, and the units go blank.
#[test]
fn closing_the_focused_window_releases_and_blanks() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F])
        .unwrap();
    s.learn_target(vizdaw_ffi::AutomationTargetRow::StripPan { strip: synth })
        .unwrap();
    let _ = debug_sent("T Out");
    s.close().unwrap();
    let sent = debug_sent("T Out");
    assert!(
        sent.contains(&vec![0xE1, 0x00, 0x00]),
        "faders down: {sent:?}"
    );
    assert!(
        !sent.iter().any(|m| m.first() == Some(&(0xE0 + k as u8))),
        "but not the one under the hand (spec §8.1): {sent:?}"
    );
    assert!(
        sent.iter().any(
            |m| m.starts_with(&[0xF0, 0x00, 0x00, 0x66, 0x14, 0x12, 0x00])
                && m[7..m.len() - 1].iter().all(|b| *b == b' ')
        ),
        "names cleared: {sent:?}"
    );
    assert!(
        vizdaw_ffi::surfaces::learn_state().waiting.is_none(),
        "its learn is cancelled"
    );
}

#[test]
fn a_strip_deleted_while_banked_is_skipped() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = mcu(&s);
    s.add_bus("Gone".into()).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let m = s.mixer().unwrap();
    let k = m.strips.iter().position(|x| x.name == "Gone").unwrap();
    assert!(k < 8, "the demo's strips and the bus fit the main unit");
    s.remove_bus(m.strips[k].id).unwrap();
    let before = s.surface_state().unwrap().surfaces[0].ignored;
    s.inject_surface(id, vec![0x90, 0x10 + k as u8, 0x7F])
        .unwrap();
    assert_eq!(
        s.surface_state().unwrap().surfaces[0].ignored,
        before + 1,
        "its mute does nothing"
    );
}

/// Pre-flight M20: the offset is clamped after a mixer change.
#[test]
fn banks_clamp_after_strips_are_removed() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = mcu(&s);
    for i in 0..12 {
        s.add_bus(format!("Extra {i}")).unwrap();
    }
    s.inject_surface(id, vec![0x90, 0x2F, 0x7F]).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let buses: Vec<u64> = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .filter(|x| x.name.starts_with("Extra "))
        .map(|x| x.id)
        .collect();
    for b in buses {
        s.remove_bus(b).unwrap();
    }
    debug_advance(30);
    s.idle().unwrap();
    let first = s.mixer().unwrap().strips[0].id;
    s.inject_surface(id, vec![0x90, 0x18, 0x7F]).unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![E::Select { strip: first }],
        "back at the first bank"
    );
}

/// Pre-flight M20: units show consecutive strips in position order,
/// whatever order they were added in.
#[test]
fn positions_out_of_order_show_consecutive_strips() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    for i in 0..12 {
        s.add_bus(format!("Extra {i}")).unwrap();
    }
    let xt = surface_add(Kind::McuXt, "X In".into(), "X Out".into(), Some(1)).unwrap();
    let main = surface_add(Kind::Mcu, "M In".into(), "M Out".into(), Some(0)).unwrap();
    s.focus_surfaces().unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let strips = s.mixer().unwrap().strips;
    s.inject_surface(main, vec![0x90, 0x18, 0x7F]).unwrap();
    s.inject_surface(xt, vec![0x90, 0x18, 0x7F]).unwrap();
    use vizdaw_ffi::surfaces::SurfaceEventRow as E;
    assert_eq!(
        s.take_surface_events().unwrap(),
        vec![
            E::Select {
                strip: strips[0].id
            },
            E::Select {
                strip: strips[8].id
            }
        ],
        "the extender at position 1 shows strips 9–16 though it was added first"
    );
}

/// Pre-flight I4: the service reads the meters without taking them from
/// the mixer window.
#[test]
fn surface_meters_leave_the_mixers_peaks() {
    use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, MonitorModeRow};
    let _g = serial();
    // Offline, the built-in synth plays into the MIDI log, not audio: the
    // fake input's sine, monitored through the demo's audio path, is what
    // sounds through a strip (as `live_input.rs` hears it).
    vizdaw_ffi::host::set_fake_input(Some(core_engine::offline::FakeInput::Signals));
    vizdaw_ffi::host::reset_host();
    let s = vizdaw_ffi::Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Loop")
        .unwrap()
        .id;
    let lane = |e: LaneEdit| s.edit(EditCommand::Lane { edit: e }, None).unwrap();
    lane(LaneEdit::SetInput {
        seq,
        lane: lp,
        input: Some(LaneInputRow::Audio {
            first: 0,
            stereo: false,
            return_of: None,
        }),
    });
    lane(LaneEdit::SetMonitor {
        seq,
        lane: lp,
        monitor: MonitorModeRow::Input,
    });
    let _ = mcu(&s);
    s.play(seq).unwrap();
    let mut loud = None;
    wait_until("a strip to sound", || {
        loud = s
            .levels()
            .unwrap()
            .meters
            .into_iter()
            .find(|m| m.peak_l.max(m.peak_r) > 0.0)
            .map(|m| m.strip);
        loud.is_some()
    });
    let strip = loud.unwrap();
    let peak = |s: &vizdaw_ffi::Session| {
        s.levels()
            .unwrap()
            .meters
            .iter()
            .find(|m| m.strip == strip)
            .map_or(0.0, |m| m.peak_l.max(m.peak_r))
    };
    let mut heard = 0;
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        debug_advance(300);
        s.idle().unwrap();
        if peak(&s) > 0.0 {
            heard += 1;
        }
    }
    s.stop().unwrap();
    vizdaw_ffi::host::set_fake_input(None);
    assert!(
        heard >= 18,
        "the mixer still sees the peaks the surfaces read: {heard} of 20"
    );
}

/// Pre-flight I5 and the owner's answer (Ruling 17): a surface port and
/// a learned control reach no lane; unlearned messages still do.
#[test]
fn surface_ports_and_learned_controls_stay_out_of_the_lanes() {
    use vizdaw_ffi::host::{debug_midi_from_source, drain_offline_log};
    use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow};
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Chords")
        .unwrap()
        .id;
    let lane = |e: LaneEdit| s.edit(EditCommand::Lane { edit: e }, None).unwrap();
    lane(LaneEdit::SetInput {
        seq,
        lane: chords,
        input: Some(LaneInputRow::Midi {
            port: None,
            channel: None,
        }),
    });
    lane(LaneEdit::SetFlags {
        seq,
        lane: chords,
        mute: None,
        solo: None,
        arm: Some(true),
    });
    s.play(seq).unwrap();
    wait_until("the engine", || s.transport().unwrap().playing);
    s.stop().unwrap();
    let _ = mcu(&s);
    let (synth, _) = synth(&s);
    s.learn_target(vizdaw_ffi::AutomationTargetRow::StripMute { strip: synth })
        .unwrap();
    s.inject_midi("Pads".into(), vec![0x99, 36, 100]).unwrap();
    debug_advance(301);
    s.idle().unwrap();
    assert_eq!(
        s.learn_maps().unwrap().len(),
        1,
        "a pad learned to the synth's mute"
    );
    std::thread::sleep(std::time::Duration::from_millis(50));
    drain_offline_log();
    debug_midi_from_source("T In", 0, &[0x90, 0x68, 0x7F]);
    debug_midi_from_source("T In", 0, &[0xE0, 0x00, 0x40]);
    assert!(
        s.inject_midi("T In".into(), vec![0x90, 0x5E, 0x7F])
            .unwrap(),
        "the surface took it"
    );
    debug_midi_from_source("Pads", 0, &[0x99, 36, 100]);
    assert!(
        s.inject_midi("Pads".into(), vec![0x89, 36, 0]).unwrap(),
        "the map took it"
    );
    debug_midi_from_source("Pads", 0, &[0x99, 38, 100]);
    let mut log = Vec::new();
    wait_until("the unlearned pad's thru", || {
        log.extend(drain_offline_log());
        log.iter()
            .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 38, 100])
    });
    assert!(
        !log.iter()
            .any(|(_, b)| b.get(1) == Some(&0x68) || b.get(1) == Some(&0x5E)),
        "nothing from the surface port: {log:?}"
    );
    assert!(
        !log.iter()
            .any(|(_, b)| b.first().is_some_and(|x| x & 0xF0 == 0xE0)),
        "no pitch bend: {log:?}"
    );
    assert!(
        !log.iter().any(|(_, b)| b.get(1) == Some(&36)),
        "nothing the map takes: {log:?}"
    );
}

/// Pre-flight M18, through the core: a V-pot in Plug-in moves the
/// selected strip's first insert's first parameter (`Source::Ui`).
#[test]
fn a_vpot_in_plugin_mode_moves_the_plugin() {
    use vizdaw_ffi::{PluginFormatTag, PluginRefRow, SlotStatus};
    let _g = serial();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(
        synth,
        None,
        PluginRefRow {
            format: PluginFormatTag::Clap,
            id: "vizdaw.test.gain".into(),
            name: "Test Gain".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
    )
    .unwrap();
    let slot = s.plugin_slots().unwrap()[0].id;
    s.play(song(&s)).unwrap();
    wait_until("the plugin to run", || {
        s.idle().unwrap();
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
    });
    let gain = |s: &vizdaw_ffi::Session| {
        s.params(slot)
            .unwrap()
            .into_iter()
            .find(|p| p.id == 0)
            .unwrap()
            .value
    };
    let before = gain(&s);
    let id = mcu(&s);
    s.set_surface_selection(vec![synth]).unwrap();
    s.inject_surface(id, vec![0x90, 0x2B, 0x7F]).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    s.inject_surface(id, vec![0xB0, 0x10, 0x05]).unwrap();
    s.stop().unwrap();
    assert!(
        gain(&s) > before,
        "V-pot 1 moved the Test Gain's first parameter"
    );
}

/// Owner answer 2026-11-02 (Ruling 5): a tap skips to the next marker,
/// else bar; a hold winds.
#[test]
fn rewind_and_forward_skip_to_markers_then_bars_and_wind_while_held() {
    use vizdaw_ffi::{EditCommand, MarkerRow, SequenceEdit};
    const BAR: i64 = 4 * 15_360; // the demo is in 4/4
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let seq = song(&s);
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::SetMarkers {
                seq,
                markers: vec![MarkerRow {
                    tick: 2 * BAR,
                    name: "B".into(),
                }],
            },
        },
        None,
    )
    .unwrap();
    s.play(seq).unwrap();
    wait_until("the engine", || s.transport().unwrap().playing);
    s.stop().unwrap();
    wait_until("the stop", || !s.transport().unwrap().playing);
    s.locate(0).unwrap();
    // The hold's wind is the integral of the rate over the clock: only the
    // hook moves it here, so the 500 ms hold is exactly 500 ms.
    vizdaw_ffi::surfaces::debug_freeze_clock(true);
    let id = mcu(&s);
    let at = |tick: i64| {
        wait_until(&format!("tick {tick}"), || {
            s.transport().unwrap().tick == tick
        })
    };
    at(0);
    let tap = |note: u8| {
        s.inject_surface(id, vec![0x90, note, 0x7F, 0x90, note, 0x00])
            .unwrap()
    };
    tap(0x5C);
    at(2 * BAR);
    tap(0x5C);
    at(3 * BAR);
    tap(0x5B);
    at(2 * BAR);
    s.inject_surface(id, vec![0x90, 0x5C, 0x7F]).unwrap();
    debug_advance(500);
    s.idle().unwrap();
    at(2 * BAR + 15_360 * 4 / 10);
    s.inject_surface(id, vec![0x90, 0x5C, 0x00]).unwrap();
    // Back: the marker, then bar lines (no marker before), never below 0.
    tap(0x5B);
    at(2 * BAR);
    tap(0x5B);
    at(BAR);
    tap(0x5B);
    at(0);
    tap(0x5B);
    at(0);
    vizdaw_ffi::surfaces::debug_freeze_clock(false);
}

#[test]
fn malformed_bytes_are_ignored_and_counted() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let id = mcu(&s);
    s.inject_surface(id, vec![0xF0, 0x01, 0x90]).unwrap();
    assert!(s.surface_state().unwrap().surfaces[0].ignored >= 1);
    assert!(s.inject_surface(id, vec![]).is_err(), "no bytes");
    assert!(s.inject_surface(id, vec![0; 70_000]).is_err(), "too many");
    assert!(
        s.inject_surface(9_999, vec![0x90, 0, 0]).is_err(),
        "no such surface"
    );
    s.set_surface_selection(vec![987_654]).unwrap();
    assert!(
        s.set_surface_selection(vec![1; 100_000]).is_err(),
        "at most 1024 strips"
    );
}

/// Task 3 review M8: every HUI hears from VizDAW at least once a second
/// (its ping goes with the feedback), so a HUI never times out while a
/// project is focused.
#[test]
fn every_hui_is_pinged_at_least_once_a_second() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    for n in 0..3 {
        surface_add(Kind::Hui, format!("P{n} In"), format!("P{n} Out"), None).unwrap();
    }
    s.focus_surfaces().unwrap();
    s.idle().unwrap();
    for n in 0..3 {
        let _ = debug_sent(&format!("P{n} Out"));
    }
    for _ in 0..3 {
        debug_advance(1_000);
        s.idle().unwrap();
        for n in 0..3 {
            let sent = debug_sent(&format!("P{n} Out"));
            assert!(
                sent.contains(&vec![0x90, 0x00, 0x00]),
                "HUI {n} was pinged: {sent:?}"
            );
        }
    }
}

/// Task 7 review M7: a port name of exactly 255 bytes is a port name.
#[test]
fn a_255_byte_port_name_is_accepted() {
    let _g = serial();
    let long = "p".repeat(255);
    let id = surface_add(Kind::Hui, long.clone(), "Out".into(), None).unwrap();
    assert_eq!(surface_list()[0].input, long);
    // Multibyte up to the bound: 127 two-byte characters and one byte.
    let wide = format!("{}x", "é".repeat(127));
    assert_eq!(wide.len(), 255);
    surface_add(Kind::Hui, wide, "Out 2".into(), None).unwrap();
    surface_remove(id).unwrap();
}

/// Task 2 concern 3: an extender cannot see the main unit's assignment
/// buttons, so the group passes it on: Send on the main unit puts the
/// extender's V-pots on the sends too.
#[test]
fn an_extender_follows_the_main_units_v_pot_assignment() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    for i in 0..12 {
        s.add_bus(format!("Extra {i}")).unwrap();
    }
    let main = surface_add(Kind::Mcu, "M In".into(), "M Out".into(), Some(0)).unwrap();
    let xt = surface_add(Kind::McuXt, "X In".into(), "X Out".into(), Some(1)).unwrap();
    s.focus_surfaces().unwrap();
    debug_advance(30);
    s.idle().unwrap();
    let _ = (debug_sent("M Out"), debug_sent("X Out"));
    s.inject_surface(main, vec![0x90, 0x29, 0x7F]).unwrap();
    debug_advance(30);
    s.idle().unwrap();
    // The extender's first strip is the ninth; in Send its V-pot drives
    // that strip's first send, which a bus does not have: nothing to move
    // (not an error), where Pan would have touched its pan.
    let strip = s.mixer().unwrap().strips[8].id;
    s.inject_surface(xt, vec![0xB0, 0x10, 0x01]).unwrap();
    assert!(
        !s.automation_state()
            .unwrap()
            .touched
            .contains(&vizdaw_ffi::AutomationTargetRow::StripPan { strip }),
        "the extender's V-pot follows the main unit into Send"
    );
    s.inject_surface(main, vec![0x90, 0x2A, 0x7F]).unwrap();
    s.inject_surface(xt, vec![0xB0, 0x10, 0x01]).unwrap();
    assert!(
        s.automation_state()
            .unwrap()
            .touched
            .contains(&vizdaw_ffi::AutomationTargetRow::StripPan { strip }),
        "and back into Pan"
    );
}

/// Task 3 ruling T3-1, delivered: a unit that says it goes offline while
/// its port stays connected releases its touch in the session.
#[test]
fn a_unit_going_offline_releases_its_touch_in_the_session() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F])
        .unwrap();
    assert_eq!(s.automation_state().unwrap().touched.len(), 1);
    s.inject_surface(id, vec![0xF0, 0x00, 0x00, 0x66, 0x14, 0x04, 0xF7])
        .unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "released by the offline message"
    );
    assert!(surface_list()[0].connected, "the port is still there");
}

/// Ruling 11: owed releases reach the session that holds them, also when
/// it is not focused: a surface removed after focus moved on releases the
/// old session's touch, which the old session takes in its own idle.
#[test]
fn a_removal_after_focus_moved_owes_nothing_to_the_new_session() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (_, k) = synth(&s);
    let id = mcu(&s);
    s.inject_surface(id, vec![0x90, 0x68 + k as u8, 0x7F])
        .unwrap();
    assert_eq!(
        s.automation_state().unwrap().touched.len(),
        1,
        "the unit holds the fader"
    );
    let other = vizdaw_ffi::Session::new_demo().unwrap();
    other.focus_surfaces().unwrap();
    surface_remove(id).unwrap();
    other.idle().unwrap();
    assert!(other.automation_state().unwrap().touched.is_empty());
    s.idle().unwrap();
    assert!(
        s.automation_state().unwrap().touched.is_empty(),
        "the focus move owed it"
    );
}

/// Plan 5f-1 Task 11 (Ruling 11, the e2e's case): `surface.inject` into a
/// window that never had the surfaces' focus (no key window, so the ticker
/// focused nothing) focuses it and refreshes the units before it routes
/// the bytes, so the unit's strip controls address the strips it now
/// shows instead of being ignored (Ruling 3).
#[test]
fn an_injection_into_an_unfocused_window_refreshes_before_it_routes() {
    let _g = serial();
    let s = vizdaw_ffi::Session::new_demo().unwrap();
    let (synth, k) = synth(&s);
    let id = surface_add(Kind::Mcu, "T In".into(), "T Out".into(), None).unwrap();
    let ch = k as u8;
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x7F, 0xE0 + ch, 0x00, 0x20])
        .unwrap();
    assert_eq!(
        s.automation_state().unwrap().touched,
        vec![vizdaw_ffi::AutomationTargetRow::StripFader { strip: synth }]
    );
    assert_eq!(surface_list()[0].ignored, 0, "nothing was ignored");
    s.inject_surface(id, vec![0x90, 0x68 + ch, 0x00]).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Fader")
    );
}
