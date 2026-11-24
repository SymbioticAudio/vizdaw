//! The left pane's Channel Strip in the Arrange view (Swift
//! `LeftStripPane`, `StripFocus`), through GPUI's test platform with the
//! scripted session: the strip follows the focused track (the selection,
//! else the armed tracks) and shows its audio path's, instrument's,
//! Built-in Synth's or MIDI strip; the Swift note with nothing focused and
//! the inspector's placeholder for a track with no strip; one strip sized
//! to the pane, scrolling down when the pane is too short; and its drags,
//! through the shared controls, keep the mixer's placement rules.

mod mixer_support;

use gpui::{Entity, TestAppContext, point, px};
use mixer_support::{Setup, setup};
use vizdaw_app::mixer::MixerPlacement;
use vizdaw_app::testing::{FakeSession, dest_row, ext_dest, lane_row, mixer_fake, strip_row};
use vizdaw_ffi::automation::GesturePhaseRow as G;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{BusRow, MidiMixRow, MixerSnapshot, StripKindRow};
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_gpui::mixer_view::MixerView;

const NOTE: &str = "Select a track to see its channel strip.";
const INSPECTOR: &str = "Inspector comes in a later version.";

/// Song (3): Keys (40, the built-in synth, 5), Vox (41, audio path 9),
/// Juno 2 (45, external Juno 7), Juno track (42, Juno too), Chords (43,
/// MIDI, no destination), Empty (44, an audio track with no path). Strips: Synth (1, built-in
/// synth), Audio 1 (3, path 9), Bus 1 (2), Master.
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.set_mixer(MixerSnapshot {
        version: 0,
        strips: vec![
            strip_row(1, "Synth", StripKindRow::BuiltinSynth),
            strip_row(3, "Audio 1", StripKindRow::AudioPath { path: 9 }),
            strip_row(2, "Bus 1", StripKindRow::Bus),
            strip_row(0, "Master", StripKindRow::Master),
        ],
        buses: vec![BusRow {
            id: 2,
            name: "Bus 1".into(),
        }],
        groups: vec![],
        vcas: vec![],
    });
    f.set_destinations(vec![
        dest_row(5, "Built-in Synth"),
        ext_dest(7, "Juno", MidiMixRow::default()),
    ]);
    let mut keys = lane_row(40, "Keys", false);
    keys.midi_dest = Some(5);
    let mut vox = lane_row(41, "Vox", false);
    vox.kind = TrackKindRow::Audio;
    vox.audio_out = Some(9);
    let mut juno2 = lane_row(45, "Juno 2", false);
    juno2.midi_dest = Some(7);
    let mut juno = lane_row(42, "Juno track", false);
    juno.midi_dest = Some(7);
    let chords = lane_row(43, "Chords", false);
    let mut empty = lane_row(44, "Empty", false);
    empty.kind = TrackKindRow::Audio;
    f.set_lanes(3, vec![keys, vox, juno2, juno, chords, empty]);
    f
}

/// The demo window in the Arrange view (the left pane shows the Channel
/// Strip), nothing selected or armed.
fn arrange(cx: &mut TestAppContext) -> Setup {
    let mut s = setup(cx, fake());
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("ui.view_arrange"));
    s.cx.run_until_parked();
    s.fake.clear_gestures();
    s
}

fn select(s: &mut Setup, lane: u64) {
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store
            .with_document(doc, |d, _| d.select_track(lane, false));
        cx.notify();
    });
    s.cx.run_until_parked();
}

fn strip_view(s: &mut Setup) -> Option<Entity<MixerView>> {
    s.window
        .read_with(&s.cx, |v, _| v.mixer_view(MixerPlacement::Strip))
        .unwrap()
}

fn left(s: &mut Setup) -> gpui::Bounds<gpui::Pixels> {
    s.element("group", "Left Pane").bounds
}

/// The left pane's strip has a press open.
fn strip_pressed(s: &mut Setup) -> bool {
    s.model(|d| {
        d.mixer().is_some_and(|m| {
            m.pressed(MixerPlacement::Strip).is_some()
                || m.midi_pressed(MixerPlacement::Strip).is_some()
        })
    })
}

#[gpui::test]
fn nothing_focused_shows_the_swift_note(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    assert!(s.find("text", NOTE).is_some());
    assert!(
        !s.elements()
            .iter()
            .any(|e| e.label.starts_with("Level of ")),
        "no strip"
    );
    assert!(strip_view(&mut s).is_none(), "no placement");
}

#[gpui::test]
fn a_selected_track_shows_the_strip_it_plays_through(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    // The built-in synth's track: the one Built-in Synth strip.
    select(&mut s, 40);
    assert!(s.find("text", NOTE).is_none());
    assert_eq!(s.fader_labels(), ["Fader of Synth"]);
    assert!(s.find("textfield", "Level of Synth").is_some());
    assert!(s.find("button", "Add Bus").is_none(), "no bar");
    let pane = left(&mut s);
    let fader = s.element("slider", "Fader of Synth").bounds;
    assert!(
        pane.contains(&fader.origin) && pane.contains(&fader.bottom_right()),
        "the strip {fader:?} is in the pane {pane:?}"
    );
    assert!(strip_view(&mut s).is_some());
    // An audio track: its path's strip.
    select(&mut s, 41);
    assert_eq!(s.fader_labels(), ["Fader of Audio 1"]);
    // A track with no strip: the inspector's placeholder.
    select(&mut s, 44);
    assert!(s.fader_labels().is_empty());
    assert!(s.find("text", INSPECTOR).is_some());
}

#[gpui::test]
fn the_armed_tracks_lead_when_nothing_is_selected(cx: &mut TestAppContext) {
    let f = fake();
    let mut keys = lane_row(40, "Keys", false);
    keys.midi_dest = Some(5);
    let mut vox = lane_row(41, "Vox", false);
    vox.kind = TrackKindRow::Audio;
    vox.audio_out = Some(9);
    vox.arm = true;
    f.set_lanes(3, vec![keys, vox]);
    let mut s = setup(cx, f);
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("ui.view_arrange"));
    s.cx.run_until_parked();
    assert_eq!(s.fader_labels(), ["Fader of Audio 1"]);
    // The selection wins over the armed tracks.
    select(&mut s, 40);
    assert_eq!(s.fader_labels(), ["Fader of Synth"]);
}

#[gpui::test]
fn a_midi_track_shows_its_destination_strip(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    select(&mut s, 42);
    assert_eq!(s.fader_labels(), ["Fader of Juno"]);
    assert!(s.find("text", "Juno").is_some());
    // R arms the focused track (the mixer's would arm Juno 2, the first
    // that plays Juno).
    assert!(s.find("button", "Record arm Juno 2").is_none());
    let arm = s.element("button", "Record arm Juno track");
    assert!(arm.enabled);
    s.click("button", "Record arm Juno track");
    assert!(
        s.fake.edits().iter().any(|e| matches!(
            e,
            vizdaw_ffi::edit::EditCommand::Lane {
                edit: vizdaw_ffi::edit::LaneEdit::SetFlags {
                    lane: 42,
                    arm: Some(true),
                    ..
                }
            }
        )),
        "{:?}",
        s.fake.edits()
    );
    // No destination: the "No Destination" strip, only R works.
    select(&mut s, 43);
    assert_eq!(s.fader_labels(), ["Fader of No Destination"]);
    assert!(s.find("text", "No Destination").is_some());
    assert!(!s.element("slider", "Fader of No Destination").enabled);
    assert!(!s.element("slider", "Pan knob of No Destination").enabled);
    assert!(!s.element("button", "Mute No Destination").enabled);
    assert!(s.element("button", "Record arm Chords").enabled);
}

#[gpui::test]
fn the_strip_fills_the_pane_height_less_16(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    select(&mut s, 40);
    let view = strip_view(&mut s).unwrap();
    let fits = view.read_with(&s.cx, |v, _| v.fits().to_vec());
    let (strip, area) = *fits.last().unwrap();
    assert!(
        (strip - area).abs() < 0.5,
        "strip {strip} tall (padding of 8 + 8 included) in an area {area} tall"
    );
    assert_eq!(view.read_with(&s.cx, |v, _| v.room()).1, 0.);
    // Centred across the pane (the name's line spans the strip).
    let pane = left(&mut s);
    let name = s.element("text", "Synth").bounds;
    let gap = (name.left() - pane.left(), pane.right() - name.right());
    assert!(
        (gap.0 - gap.1).abs() < px(1.),
        "the name {name:?} in the pane {pane:?}"
    );
}

#[gpui::test]
fn a_short_pane_scrolls_the_strip_down(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    select(&mut s, 40);
    s.cx.simulate_resize(gpui::size(px(1440.), px(420.)));
    s.cx.run_until_parked();
    let view = strip_view(&mut s).unwrap();
    let room = view.read_with(&s.cx, |v, _| v.room());
    assert!(room.1 > 0., "the strip is taller than the pane: {room:?}");
    let before = s.element("slider", "Fader of Synth").bounds.origin.y;
    let at = left(&mut s).center();
    s.cx.simulate_event(gpui::ScrollEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-40.))),
        modifiers: gpui::Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
        ..Default::default()
    });
    s.cx.run_until_parked();
    assert!(view.read_with(&s.cx, |v, _| v.scroll()).1 > 0.);
    let after = s.element("slider", "Fader of Synth").bounds.origin.y;
    assert!(after < before, "{before:?} -> {after:?}");
}

#[gpui::test]
fn a_fader_drag_in_the_pane_is_a_strip_gesture(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    select(&mut s, 40);
    let at = s.fader_at("Fader of Synth");
    s.drag_up(at);
    let t = T::StripFader { strip: 1 };
    let phases = s.of(t);
    assert!(
        matches!(phases.first(), Some(G::Touch { .. })),
        "{phases:?}"
    );
    assert_eq!(phases.last(), Some(&G::Release), "{phases:?}");
    assert!(phases.iter().any(|p| matches!(p, G::Value { .. })));
    assert!(!strip_pressed(&mut s));
}

#[gpui::test]
fn a_press_and_release_in_one_frame_leaves_no_drag_open(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    select(&mut s, 40);
    let at = s.fader_at("Fader of Synth");
    s.press_and_release_in_one_frame(at);
    assert!(!strip_pressed(&mut s), "the press is gone");
    assert!(s.fake.gestures().is_empty(), "{:?}", s.fake.gestures());
}

#[gpui::test]
fn hiding_the_pane_ends_its_drag(cx: &mut TestAppContext) {
    let mut s = arrange(cx);
    select(&mut s, 40);
    let at = s.fader_at("Fader of Synth");
    s.down(at);
    s.move_to(point(at.x, at.y - px(10.)), true);
    assert!(strip_pressed(&mut s));
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("ui.toggle_left"));
    s.cx.run_until_parked();
    assert!(strip_view(&mut s).is_none(), "the placement went");
    assert!(!strip_pressed(&mut s));
    let phases = s.of(T::StripFader { strip: 1 });
    assert_eq!(phases.last(), Some(&G::Release), "{phases:?}");
}
