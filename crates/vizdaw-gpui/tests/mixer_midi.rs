//! External MIDI destinations' strips in the Mix view (Swift
//! `MidiStripCells`), through GPUI's test platform with the scripted
//! session: the column's place and labels, fader and pan drags that
//! preview and commit once on release (and the press rules every drag
//! keeps), M, R, the typed boxes and the note-activity meter.

mod mixer_support;

use gpui::{TestAppContext, point, px};
use mixer_support::{Setup, setup};
use vizdaw_app::mixer::MidiStripMath;
use vizdaw_app::testing::{FakeSession, ext_dest, lane_row, mixer_fake};
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::mixer::{MidiLevelRow, MidiMixRow, MixerLevels};

/// Synth (1), Juno (7, external, unset), Bus 1 (2), Master.
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.set_destinations(vec![ext_dest(7, "Juno", MidiMixRow::default())]);
    f
}

fn juno(cx: &mut TestAppContext) -> Setup {
    setup(cx, fake())
}

#[gpui::test]
fn a_midi_strip_sits_before_the_first_bus_with_the_swift_labels(cx: &mut TestAppContext) {
    let mut s = juno(cx);
    assert_eq!(
        s.fader_labels(),
        [
            "Fader of Synth",
            "Fader of Juno",
            "Fader of Bus 1",
            "Fader of Master"
        ]
    );
    assert!(s.find("text", "Juno").is_some());
    assert!(s.find("text", "MIDI").is_some(), "no channel");
    assert_eq!(s.value("slider", "Fader of Juno").as_deref(), Some("100"));
    assert_eq!(
        s.value("textfield", "Volume of Juno").as_deref(),
        Some("100")
    );
    assert_eq!(s.value("slider", "Pan knob of Juno").as_deref(), Some("C"));
    assert_eq!(s.value("textfield", "Pan of Juno").as_deref(), Some("C"));
    assert_eq!(s.value("button", "Mute Juno").as_deref(), Some("off"));
    let arm = s.element("button", "Record arm Juno");
    assert!(!arm.enabled, "no track plays Juno");
    assert!(s.find("button", "Solo Juno").is_none(), "no solo");
    assert!(s.find("menu", "Automation of Juno").is_none());
    s.faders_line_up();
    let drawn = s.view().read_with(&s.cx, |v, _| v.midi_drawn().to_vec());
    assert_eq!(drawn, [(7, 0.0)]);
}

#[gpui::test]
fn a_midi_fader_drag_previews_and_commits_once(cx: &mut TestAppContext) {
    let mut s = juno(cx);
    let at = s.fader_at("Fader of Juno");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(30.)), true);
    let previews = s.fake.midi_previews();
    assert!(!previews.is_empty());
    assert!(
        previews
            .iter()
            .all(|p| p.0 == 7 && p.1.is_some() && p.2.is_none()),
        "{previews:?}"
    );
    assert!(s.fake.midi_mixes().is_empty(), "nothing committed yet");
    let shown = s.value("slider", "Fader of Juno").unwrap();
    assert_ne!(shown, "100", "the draft shows");
    s.up(point(at.x, at.y - px(30.)), 1);
    let mixes = s.fake.midi_mixes();
    assert_eq!(mixes.len(), 1, "{mixes:?}");
    assert_eq!(mixes[0].0, 7);
    assert_eq!(mixes[0].1.volume.map(|v| v.to_string()), Some(shown));
    assert_eq!(mixes[0].1.pan, None);
    assert!(!s.pressed());
}

#[gpui::test]
fn a_midi_pan_drag_previews_the_pan_and_commits_once(cx: &mut TestAppContext) {
    let mut s = juno(cx);
    let at = s.element("slider", "Pan knob of Juno").bounds.center();
    s.drag_up(at);
    let previews = s.fake.midi_previews();
    assert!(!previews.is_empty());
    assert!(
        previews
            .iter()
            .all(|p| p.0 == 7 && p.1.is_none() && p.2.is_some()),
        "{previews:?}"
    );
    let mixes = s.fake.midi_mixes();
    assert_eq!(mixes.len(), 1, "{mixes:?}");
    assert!(
        mixes[0]
            .1
            .pan
            .is_some_and(|p| p > MidiStripMath::PAN_CENTRE)
    );
}

#[gpui::test]
fn midi_presses_and_releases_in_one_frame_leave_no_drag_open(cx: &mut TestAppContext) {
    let mut s = juno(cx);
    let fader = s.fader_at("Fader of Juno");
    s.press_and_release_in_one_frame(fader);
    assert!(!s.pressed(), "the fader's press is gone");
    let knob = s.element("slider", "Pan knob of Juno").bounds.center();
    s.press_and_release_in_one_frame(knob);
    assert!(!s.pressed(), "the knob's press is gone");
    assert!(s.fake.midi_previews().is_empty());
    assert!(s.fake.midi_mixes().is_empty());
}

#[gpui::test]
fn a_midi_drag_whose_mouse_up_was_lost_commits_at_the_next_press(cx: &mut TestAppContext) {
    let mut s = juno(cx);
    let at = s.fader_at("Fader of Juno");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(30.)), true);
    // The up never came. The next press, anywhere (here the bar), ends
    // the drag at once: its volume commits once, and dragging on from
    // there previews nothing.
    let elsewhere = s.element("button", "Add Bus").bounds.center();
    s.down(elsewhere);
    assert!(!s.pressed(), "the press ended the drag");
    assert_eq!(s.fake.midi_mixes().len(), 1);
    let previews = s.fake.midi_previews().len();
    s.move_to(point(elsewhere.x, elsewhere.y + px(40.)), true);
    s.up(point(elsewhere.x, elsewhere.y + px(40.)), 1);
    assert_eq!(s.fake.midi_previews().len(), previews, "nothing more moved");
    // A lost up followed by a click on M: the volume commits, then M
    // builds on it.
    let at = s.fader_at("Fader of Juno");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(20.)), true);
    s.click("button", "Mute Juno");
    assert!(!s.pressed());
    let mixes = s.fake.midi_mixes();
    assert_eq!(mixes.len(), 3, "{mixes:?}");
    assert!(!mixes[1].1.mute);
    assert!(mixes[2].1.mute);
}

#[gpui::test]
fn a_move_without_the_button_lifts_the_midi_drag(cx: &mut TestAppContext) {
    let mut s = juno(cx);
    let at = s.fader_at("Fader of Juno");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    s.move_to(point(at.x, at.y - px(30.)), true);
    s.move_to(point(at.x, at.y - px(40.)), false);
    assert!(!s.pressed());
    assert_eq!(s.fake.midi_mixes().len(), 1);
}

#[gpui::test]
fn mute_arm_and_the_typed_boxes_commit(cx: &mut TestAppContext) {
    let f = fake();
    let mut keys = lane_row(40, "Keys", false);
    keys.midi_dest = Some(7);
    f.set_lanes(3, vec![keys]);
    let mut s = setup(cx, f);
    s.click("button", "Mute Juno");
    assert!(s.fake.midi_mixes().last().unwrap().1.mute);
    assert_eq!(s.value("button", "Mute Juno").as_deref(), Some("on"));
    // R names and arms the track that plays Juno.
    s.click("button", "Record arm Keys");
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: 3,
                lane: 40,
                mute: None,
                solo: None,
                arm: Some(true),
            }
        })
    );
    s.click("textfield", "Volume of Juno");
    s.cx.simulate_input("90");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.fake.midi_mixes().last().unwrap().1.volume, Some(90));
    s.click("textfield", "Pan of Juno");
    s.cx.simulate_input("L 10");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.fake.midi_mixes().last().unwrap().1.pan, Some(54));
    let n = s.fake.midi_mixes().len();
    s.click("textfield", "Volume of Juno");
    s.cx.simulate_input("200");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.fake.midi_mixes().len(), n);
    assert_eq!(s.status().as_deref(), Some(MidiStripMath::VOLUME_HINT));
}

#[gpui::test]
fn the_activity_meter_shows_the_notes_going_out(cx: &mut TestAppContext) {
    let mut s = juno(cx);
    s.fake.set_levels(MixerLevels {
        meters: vec![],
        dsp_load: 0.,
        dsp_peak: 0.,
        latency_frames: 0,
        faulted: vec![],
        midi: vec![MidiLevelRow {
            dest: 7,
            velocity: 127,
        }],
    });
    s.period();
    let drawn = s.view().read_with(&s.cx, |v, _| v.midi_drawn().to_vec());
    assert_eq!(drawn.len(), 1);
    assert!(drawn[0].1 > 0.9, "{drawn:?}");
}
