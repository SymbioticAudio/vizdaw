//! External MIDI destinations' strips (Swift `MidiStripTests`,
//! `SynthCommandsTests` `setMidiMixChecksItsArguments`,
//! `setMidiMixSetsAndKeepsAbsentValues`): the 0…127 arithmetic and
//! labels, drags that preview every 20 ms and commit once, clicks, typed
//! values, drafts dropped under an undo, and `destination.set_midi_mix`.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use support::{FakeSession, dest_row, ext_dest, lane_row, mixer_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{self, MIXER_COMMANDS, Ran};
use vizdaw_app::mixer::{
    MIDI_PREVIEW_INTERVAL, MidiControl, MidiStripLabels, MidiStripMath, MixerModel,
    MixerPlacement as P,
};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::mixer::{MidiLevelRow, MidiMixRow, MixerLevels};
use vizdaw_ffi::mpe::{MpeRow, MpeZoneRow};
use vizdaw_ffi::tracks::TrackKindRow;

fn mix(volume: Option<u8>, pan: Option<u8>, mute: bool) -> MidiMixRow {
    MidiMixRow { volume, pan, mute }
}

/// [`mixer_fake`] with Juno (7), an external destination, unset.
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.set_destinations(vec![ext_dest(7, "Juno", MidiMixRow::default())]);
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

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn labels_and_parsing() {
    assert_eq!(MidiStripMath::volume_label(None), "100");
    assert_eq!(MidiStripMath::volume_label(Some(90)), "90");
    assert_eq!(MidiStripMath::pan_label(Some(64)), "C");
    assert_eq!(MidiStripMath::pan_label(Some(0)), "L 64");
    assert_eq!(MidiStripMath::pan_label(Some(127)), "R 63");
    assert_eq!(MidiStripMath::pan_label(None), "C");
    assert_eq!(MidiStripMath::parse_volume("0"), Some(0));
    assert_eq!(MidiStripMath::parse_volume(" 127 "), Some(127));
    for bad in ["128", "-1", "x", "", "+1", "1e2", "0127", "\u{0661}"] {
        assert_eq!(MidiStripMath::parse_volume(bad), None, "{bad}");
    }
    let pans = [
        ("C", Some(64)),
        ("c", Some(64)),
        ("L 64", Some(0)),
        ("L 1", Some(63)),
        ("R 63", Some(127)),
        ("R 10", Some(74)),
        ("r10", Some(74)),
        ("L 65", None),
        ("R 64", None),
        ("L 0", None),
        ("R", None),
        ("0", Some(0)),
        ("127", Some(127)),
        ("128", None),
    ];
    for (text, want) in pans {
        assert_eq!(MidiStripMath::parse_pan(text), want, "{text}");
    }
    assert_eq!(MidiStripMath::position(None), 100.0 / 127.0);
    assert_eq!(MidiStripMath::position(Some(127)), 1.0);
    assert_eq!(MidiStripMath::value_at(0.0), 0);
    assert_eq!(MidiStripMath::value_at(1.0), 127);
    assert_eq!(MidiStripMath::value_at(2.0), 127);
    assert_eq!(MidiStripMath::value_at(-1.0), 0);
    assert_eq!(MidiStripMath::value_at(f64::NAN), 0);
    for v in [0u8, 1, 64, 100, 127] {
        assert_eq!(MidiStripMath::value_at(MidiStripMath::position(Some(v))), v);
    }
}

#[test]
fn the_fader_scale_and_the_kind_line_are_the_cc_values() {
    let marks = MidiStripMath::fader_marks();
    let labels: Vec<&str> = marks.iter().map(|(_, l)| l.as_str()).collect();
    assert_eq!(labels, ["127", "100", "64", "32", "0"]);
    let positions: Vec<f64> = marks.iter().map(|(p, _)| *p).collect();
    let want: Vec<f64> = [127., 100., 64., 32., 0.]
        .iter()
        .map(|v| v / 127.)
        .collect();
    assert_eq!(positions, want);
    assert_eq!(MidiStripMath::pan_position(None), 64.0 / 127.0);
    assert_eq!(MidiStripMath::pan_position(Some(0)), 0.0);
    assert_eq!(MidiStripMath::pan_position(Some(127)), 1.0);
    assert_eq!(
        MidiStripMath::kind_label(Some(0), None),
        "MIDI \u{00B7} Ch 1"
    );
    assert_eq!(
        MidiStripMath::kind_label(Some(15), None),
        "MIDI \u{00B7} Ch 16"
    );
    assert_eq!(MidiStripMath::kind_label(None, None), "MIDI");
    let lower = MpeRow {
        zone: MpeZoneRow::Lower,
        members: 15,
        bend_range: 48,
    };
    let upper = MpeRow {
        zone: MpeZoneRow::Upper,
        ..lower
    };
    assert_eq!(
        MidiStripMath::kind_label(Some(2), Some(&lower)),
        "MIDI \u{00B7} MPE Lower"
    );
    assert_eq!(
        MidiStripMath::kind_label(None, Some(&upper)),
        "MIDI \u{00B7} MPE Upper"
    );
    assert_eq!(MidiStripMath::NO_DESTINATION, "No Destination");
    assert_eq!(MidiStripMath::METER_WIDTH, 6.0);
    assert_eq!(MidiStripLabels::fader("Juno"), "Fader of Juno");
    assert_eq!(MidiStripLabels::volume_box("Juno"), "Volume of Juno");
    assert_eq!(MidiStripLabels::pan_knob("Juno"), "Pan knob of Juno");
    assert_eq!(MidiStripLabels::pan_box("Juno"), "Pan of Juno");
    assert_eq!(MidiStripLabels::mute("Juno"), "Mute Juno");
    assert_eq!(MidiStripLabels::arm("Lead"), "Record arm Lead");
    assert!(!MidiStripMath::VOLUME_HINT.to_lowercase().contains("lane"));
    assert!(!MidiStripMath::PAN_HINT.to_lowercase().contains("lane"));
}

#[test]
fn a_drag_previews_every_20_ms_and_commits_once() {
    let (f, mut d) = make_with(fake());
    let t0 = Instant::now();
    assert_eq!(MIDI_PREVIEW_INTERVAL, ms(20));
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 7, Some(10), None, t0);
        m.midi_drag_to(d, 7, Some(11), None, t0 + ms(5));
        m.midi_drag_to(d, 7, Some(12), None, t0 + ms(10));
    });
    assert_eq!(f.midi_previews().len(), 1);
    assert_eq!(d.mixer().unwrap().midi_mix(7).unwrap().volume, Some(12));
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 7, Some(13), None, t0 + ms(35))
    });
    assert_eq!(
        f.midi_previews(),
        [(7, Some(10), None), (7, Some(13), None)]
    );
    assert!(f.midi_mixes().is_empty(), "a preview is no edit");
    let version = d.snapshot().version;
    assert_eq!(f.snap().version, version, "nor a new document");
    mx(&mut d, |m, d| m.midi_release(d, 7));
    assert_eq!(f.midi_mixes(), [(7, mix(Some(13), None, false))]);
    mx(&mut d, |m, d| m.midi_release(d, 7));
    assert_eq!(f.midi_mixes().len(), 1, "once");
    assert_eq!(d.mixer().unwrap().midi_mix(7).unwrap().volume, Some(13));
}

#[test]
fn a_click_commits_once() {
    let (f, mut d) = make_with(fake());
    mx(&mut d, |m, d| m.set_midi_volume(d, 7, 55));
    assert_eq!(f.midi_mixes(), [(7, mix(Some(55), None, false))]);
    mx(&mut d, |m, d| m.set_midi_pan(d, 7, 20));
    assert_eq!(f.midi_mixes()[1].1, mix(Some(55), Some(20), false));
    mx(&mut d, |m, d| m.toggle_midi_mute(d, 7));
    assert_eq!(f.midi_mixes()[2].1, mix(Some(55), Some(20), true));
    assert_eq!(f.midi_mixes().len(), 3);
    assert!(f.midi_previews().is_empty());
}

#[test]
fn an_undo_during_a_draft_drops_the_draft() {
    let (f, mut d) = make_with(fake());
    let t0 = Instant::now();
    mx(&mut d, |m, d| m.midi_drag_to(d, 7, Some(30), None, t0));
    assert_eq!(d.mixer().unwrap().midi_mix(7).unwrap().volume, Some(30));
    // The committed mix changes under the drag (an undo, the channel).
    f.edit_destinations(|ds| ds[0].midi_mix = mix(Some(80), None, false));
    let mut s = f.snap();
    s.version += 1;
    f.set_idle(Some(s));
    d.idle();
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(d.mixer().unwrap().midi_mix(7).unwrap().volume, Some(80));
    mx(&mut d, |m, d| m.toggle_midi_mute(d, 7));
    assert_eq!(f.midi_mixes(), [(7, mix(Some(80), None, true))]);
    mx(&mut d, |m, d| m.midi_release(d, 7));
    assert_eq!(f.midi_mixes().len(), 1, "the dropped draft does not commit");
}

#[test]
fn a_gone_destination_drops_its_draft() {
    let (f, mut d) = make_with(fake());
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 7, Some(30), None, Instant::now())
    });
    f.set_destinations(vec![]);
    let mut s = f.snap();
    s.version += 1;
    f.set_idle(Some(s));
    d.idle();
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(d.mixer().unwrap().midi_mix(7), None);
    mx(&mut d, |m, d| m.midi_release(d, 7));
    assert!(f.midi_mixes().is_empty());
    assert!(d.alert.is_none());
}

#[test]
fn a_destination_that_is_no_longer_external_drops_its_draft() {
    let (f, mut d) = make_with(fake());
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 7, Some(30), None, Instant::now())
    });
    f.set_destinations(vec![dest_row(7, "Juno")]);
    let mut s = f.snap();
    s.version += 1;
    f.set_idle(Some(s));
    d.idle();
    mx(&mut d, |m, d| {
        m.refresh_if_needed(d);
        m.midi_release(d, 7);
    });
    assert!(f.midi_mixes().is_empty());
}

#[test]
fn a_cancelled_drag_commits_nothing() {
    let (f, mut d) = make_with(fake());
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 7, Some(30), None, Instant::now());
        m.midi_cancel(d, 7);
    });
    assert_eq!(d.mixer().unwrap().midi_mix(7).unwrap().volume, None);
    mx(&mut d, |m, d| {
        m.midi_release(d, 7);
        m.toggle_midi_mute(d, 7);
    });
    assert_eq!(f.midi_mixes(), [(7, mix(None, None, true))]);
}

/// Swift final review m3: a cancelled drag sends the committed values
/// back once, so the synth and the strip agree; an unset one sends
/// nothing.
#[test]
fn a_cancelled_drag_puts_the_synth_back() {
    let f = mixer_fake();
    f.set_destinations(vec![
        ext_dest(7, "Juno", mix(Some(50), None, false)),
        ext_dest(8, "Moog", MidiMixRow::default()),
    ]);
    let (f, mut d) = make_with(f);
    let t0 = Instant::now();
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 7, Some(30), None, t0);
        m.midi_cancel(d, 7);
    });
    assert_eq!(
        f.midi_previews(),
        [(7, Some(30), None), (7, Some(50), None)]
    );
    mx(&mut d, |m, d| m.midi_cancel(d, 7));
    assert_eq!(f.midi_previews().len(), 2, "no draft: nothing to put back");
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 8, Some(30), None, t0);
        m.midi_cancel(d, 8);
    });
    assert_eq!(
        f.midi_previews().len(),
        3,
        "unset: the synth stays as it is"
    );
    assert!(f.midi_mixes().is_empty());
}

#[test]
fn a_drag_that_ends_where_it_began_commits_nothing() {
    let f = mixer_fake();
    f.set_destinations(vec![ext_dest(7, "Juno", mix(Some(50), None, false))]);
    let (f, mut d) = make_with(f);
    let t0 = Instant::now();
    mx(&mut d, |m, d| {
        m.midi_drag_to(d, 7, None, None, t0);
        m.midi_drag_to(d, 7, Some(50), None, t0 + ms(100));
        m.midi_release(d, 7);
    });
    assert!(f.midi_mixes().is_empty());
}

#[test]
fn a_failed_commit_raises_an_alert_and_an_unknown_destination_is_ignored() {
    let (f, mut d) = make_with(fake());
    f.fail_next(
        "set_midi_mix",
        VizError::Busy {
            message: "x".into(),
        },
    );
    mx(&mut d, |m, d| m.set_midi_volume(d, 7, 55));
    assert_eq!(
        d.alert.as_ref().map(|a| a.title.as_str()),
        Some("Set MIDI Mix Failed")
    );
    d.alert = None;
    mx(&mut d, |m, d| {
        m.set_midi_volume(d, 99, 55);
        m.toggle_midi_mute(d, 99);
    });
    assert!(
        f.midi_mixes().is_empty(),
        "an unknown destination is ignored"
    );
    assert!(d.alert.is_none());
}

#[test]
fn typed_values_are_parsed_and_range_checked() {
    let (f, mut d) = make_with(fake());
    assert!(!mx(&mut d, |m, d| m.type_midi_volume(d, 7, "200")));
    assert_eq!(d.status(), Some(MidiStripMath::VOLUME_HINT));
    assert!(!mx(&mut d, |m, d| m.type_midi_pan(d, 7, "L 70")));
    assert_eq!(d.status(), Some(MidiStripMath::PAN_HINT));
    assert!(!mx(&mut d, |m, d| m.type_midi_volume(d, 99, "90")));
    assert!(f.midi_mixes().is_empty());
    assert!(mx(&mut d, |m, d| m.type_midi_volume(d, 7, "90")));
    assert!(mx(&mut d, |m, d| m.type_midi_pan(d, 7, "R 10")));
    assert_eq!(f.midi_mixes().len(), 2);
    assert_eq!(f.midi_mixes()[1].1, mix(Some(90), Some(74), false));
}

#[test]
fn columns_put_midi_strips_before_buses() {
    let f = mixer_fake();
    f.set_destinations(vec![
        ext_dest(7, "Juno", MidiMixRow::default()),
        dest_row(2, "Built-in Synth"),
        ext_dest(8, "Moog", MidiMixRow::default()),
    ]);
    let (_f, d) = make_with(f);
    let ids: Vec<String> = d
        .mixer()
        .unwrap()
        .columns()
        .iter()
        .map(|c| c.id())
        .collect();
    assert_eq!(ids, ["s1", "d7", "d8", "s2"]);
}

#[test]
fn activity_decays_and_is_dropped_when_spent() {
    let (f, mut d) = make_with(fake());
    let t0 = Instant::now();
    f.set_levels(MixerLevels {
        midi: vec![MidiLevelRow {
            dest: 7,
            velocity: 127,
        }],
        ..MixerLevels::empty()
    });
    mx(&mut d, |m, d| m.poll_levels(d, t0));
    assert_eq!(d.mixer().unwrap().midi_activity(7), 1.0);
    f.set_levels(MixerLevels::empty());
    for i in 1..200 {
        mx(&mut d, |m, d| m.poll_levels(d, t0 + ms(30 * i)));
    }
    assert_eq!(d.mixer().unwrap().midi_activity(7), 0.0);
}

// The drag tracker.

#[test]
fn a_fader_drag_previews_from_the_first_real_move_and_commits_on_lift() {
    let f = mixer_fake();
    f.set_destinations(vec![ext_dest(7, "Juno", mix(Some(64), None, false))]);
    let (f, mut d) = make_with(f);
    let t0 = Instant::now();
    // 127 points for the whole throw: one value a point.
    mx(&mut d, |m, d| {
        m.midi_press(d, P::Window, 7, MidiControl::Volume, 127.0, t0);
        m.midi_drag(d, P::Window, 0.0, -1.0, false, t0);
    });
    assert!(f.midi_previews().is_empty(), "under the least distance");
    assert!(!d.mixer().unwrap().is_midi_dragging(P::Window));
    mx(&mut d, |m, d| {
        m.midi_drag(d, P::Window, 0.0, -2.0, false, t0 + ms(1));
        m.midi_drag(d, P::Window, 0.0, -12.0, false, t0 + ms(30));
    });
    assert!(d.mixer().unwrap().is_midi_dragging(P::Window));
    assert_eq!(
        d.mixer().unwrap().midi_pressed(P::Window),
        Some((7, MidiControl::Volume))
    );
    // The first move anchors the drag; ten points up are ten values.
    assert_eq!(
        f.midi_previews(),
        [(7, Some(64), None), (7, Some(74), None)]
    );
    // ⌥ re-anchors where it is first seen (no jump), then is ten times
    // finer: twenty points more are two values.
    mx(&mut d, |m, d| {
        m.midi_drag(d, P::Window, 0.0, -32.0, true, t0 + ms(60));
        m.midi_drag(d, P::Window, 0.0, -52.0, true, t0 + ms(90));
    });
    assert_eq!(f.midi_previews().last(), Some(&(7, Some(96), None)));
    assert!(f.midi_mixes().is_empty());
    mx(&mut d, |m, d| m.midi_lift(d, P::Window, t0 + ms(100)));
    assert_eq!(f.midi_mixes(), [(7, mix(Some(96), None, false))]);
    assert_eq!(d.mixer().unwrap().midi_pressed(P::Window), None);
}

#[test]
fn a_press_without_a_move_commits_nothing_and_a_pan_drag_sends_pan() {
    let (f, mut d) = make_with(fake());
    let t0 = Instant::now();
    mx(&mut d, |m, d| {
        m.midi_press(d, P::Pane, 7, MidiControl::Pan, 200.0, t0);
        m.midi_lift(d, P::Pane, t0);
    });
    assert!(f.midi_previews().is_empty() && f.midi_mixes().is_empty());
    mx(&mut d, |m, d| {
        m.midi_press(d, P::Pane, 7, MidiControl::Pan, 127.0, t0);
        m.midi_drag(d, P::Pane, 0.0, 5.0, false, t0);
        m.midi_drag(d, P::Pane, 0.0, 15.0, false, t0 + ms(25));
        m.midi_lift(d, P::Pane, t0 + ms(30));
    });
    // An unset pan starts at the centre; down is left.
    assert_eq!(
        f.midi_previews(),
        [(7, None, Some(64)), (7, None, Some(54))]
    );
    assert_eq!(f.midi_mixes(), [(7, mix(None, Some(54), false))]);
}

#[test]
fn a_cancelled_drag_or_a_gone_placement_commits_once() {
    let (f, mut d) = make_with(fake());
    let t0 = Instant::now();
    mx(&mut d, |m, d| {
        m.appeared(P::Window);
        m.midi_press(d, P::Window, 7, MidiControl::Volume, 127.0, t0);
        m.midi_drag(d, P::Window, 0.0, -3.0, false, t0);
        m.midi_drag(d, P::Window, 0.0, -8.0, false, t0 + ms(30));
        m.cancel_drags(d, P::Window, t0 + ms(40));
        m.cancel_drags(d, P::Window, t0 + ms(50));
    });
    assert_eq!(f.midi_mixes(), [(7, mix(Some(105), None, false))]);
    mx(&mut d, |m, d| {
        m.midi_press(d, P::Window, 7, MidiControl::Volume, 127.0, t0 + ms(500));
        m.midi_drag(d, P::Window, 0.0, 10.0, false, t0 + ms(500));
        m.midi_drag(d, P::Window, 0.0, 20.0, false, t0 + ms(530));
        m.disappeared(d, P::Window, t0 + ms(540));
    });
    assert_eq!(f.midi_mixes().len(), 2);
    assert_eq!(f.midi_mixes()[1].1.volume, Some(95));
    // The document closing ends a drag in any placement.
    mx(&mut d, |m, d| {
        m.midi_press(d, P::Pane, 7, MidiControl::Volume, 127.0, t0 + ms(900));
        m.midi_drag(d, P::Pane, 0.0, -3.0, false, t0 + ms(900));
        m.midi_drag(d, P::Pane, 0.0, -13.0, false, t0 + ms(930));
    });
    d.end_mixer_drags(t0 + ms(940));
    assert_eq!(f.midi_mixes().len(), 3);
    assert_eq!(f.midi_mixes()[2].1.volume, Some(105));
}

/// A press whose mouse-up never came ends (and commits) before the next
/// press in that placement, a MIDI control's or a strip fader's.
#[test]
fn a_new_press_ends_a_drag_whose_mouse_up_never_came() {
    let (f, mut d) = make_with(fake());
    let t0 = Instant::now();
    let fader = vizdaw_ffi::events::AutomationTargetRow::StripFader { strip: 1 };
    mx(&mut d, |m, d| {
        m.midi_press(d, P::Window, 7, MidiControl::Volume, 127.0, t0);
        m.midi_drag(d, P::Window, 0.0, -3.0, false, t0);
        m.midi_drag(d, P::Window, 0.0, -8.0, false, t0 + ms(30));
        m.midi_press(d, P::Window, 7, MidiControl::Pan, 127.0, t0 + ms(40));
    });
    assert_eq!(f.midi_mixes(), [(7, mix(Some(105), None, false))]);
    assert_eq!(
        d.mixer().unwrap().midi_pressed(P::Window),
        Some((7, MidiControl::Pan))
    );
    mx(&mut d, |m, d| {
        m.midi_drag(d, P::Window, 0.0, 3.0, false, t0 + ms(50));
        m.midi_drag(d, P::Window, 0.0, 13.0, false, t0 + ms(80));
        m.press(d, P::Window, fader, 200.0, false, t0 + ms(90));
    });
    assert_eq!(f.midi_mixes().len(), 2);
    assert_eq!(f.midi_mixes()[1].1.pan, Some(54));
    assert_eq!(d.mixer().unwrap().midi_pressed(P::Window), None);
}

#[test]
fn a_double_click_resets_unless_a_drag_just_ended() {
    let f = mixer_fake();
    f.set_destinations(vec![ext_dest(7, "Juno", mix(Some(20), Some(10), false))]);
    let (f, mut d) = make_with(f);
    let t0 = Instant::now();
    mx(&mut d, |m, d| {
        m.midi_double_click(d, P::Window, 7, MidiControl::Volume, t0);
        m.midi_double_click(d, P::Window, 7, MidiControl::Pan, t0);
    });
    assert_eq!(
        f.midi_mixes(),
        [
            (7, mix(Some(100), Some(10), false)),
            (7, mix(Some(100), Some(64), false))
        ]
    );
    mx(&mut d, |m, d| {
        m.midi_press(d, P::Window, 7, MidiControl::Volume, 127.0, t0);
        m.midi_drag(d, P::Window, 0.0, 5.0, false, t0);
        m.midi_drag(d, P::Window, 0.0, 10.0, false, t0 + ms(30));
        m.midi_lift(d, P::Window, t0 + ms(40));
        m.midi_double_click(d, P::Window, 7, MidiControl::Volume, t0 + ms(100));
    });
    assert_eq!(f.midi_mixes().len(), 3, "part of the drag, not a reset");
    mx(&mut d, |m, d| {
        m.midi_double_click(d, P::Window, 7, MidiControl::Volume, t0 + ms(400))
    });
    assert_eq!(f.midi_mixes().len(), 4);
    assert_eq!(f.midi_mixes()[3].1.volume, Some(100));
}

#[test]
fn a_key_step_is_one_value_and_one_commit() {
    let f = mixer_fake();
    f.set_destinations(vec![ext_dest(7, "Juno", mix(Some(20), None, false))]);
    let (f, mut d) = make_with(f);
    let t0 = Instant::now();
    mx(&mut d, |m, d| {
        m.midi_nudge(d, 7, MidiControl::Volume, MidiStripMath::STEP, t0);
        m.midi_nudge(d, 7, MidiControl::Volume, 10.0 * MidiStripMath::STEP, t0);
        m.midi_nudge(d, 7, MidiControl::Pan, -MidiStripMath::STEP, t0);
    });
    let mixes: Vec<MidiMixRow> = f.midi_mixes().into_iter().map(|(_, m)| m).collect();
    assert_eq!(
        mixes,
        [
            mix(Some(21), None, false),
            mix(Some(31), None, false),
            mix(Some(31), Some(63), false)
        ]
    );
}

#[test]
fn r_arms_the_first_armed_track_that_plays_the_destination() {
    let f = fake();
    let mut a = lane_row(30, "A", false);
    a.midi_dest = Some(7);
    let mut b = lane_row(31, "B", false);
    b.midi_dest = Some(7);
    b.arm = true;
    let mut audio = lane_row(32, "Audio", false);
    audio.midi_dest = Some(7);
    audio.kind = TrackKindRow::Audio;
    f.set_lanes(3, vec![audio, a, b]);
    let (f, mut d) = make_with(f);
    d.select(Some(3));
    assert_eq!(
        d.mixer().unwrap().midi_lane(&d, 7, P::View).map(|l| l.id),
        Some(31)
    );
    assert_eq!(
        d.mixer().unwrap().midi_lane(&d, 8, P::View).map(|l| l.id),
        None
    );
    mx(&mut d, |m, d| m.toggle_midi_arm(d, 31));
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: 3,
                lane: 31,
                mute: None,
                solo: None,
                arm: Some(false)
            }
        })
    );
}

#[test]
fn the_left_pane_arms_the_focused_track_when_it_plays_the_destination() {
    // Swift `playingLane(in: .strip)`: the focused track (the selection,
    // else the armed tracks), only when it plays `dest`.
    let f = fake();
    let mut a = lane_row(30, "A", false);
    a.midi_dest = Some(7);
    let mut b = lane_row(31, "B", false);
    b.midi_dest = Some(7);
    b.arm = true;
    let mut audio = lane_row(32, "Audio", false);
    audio.midi_dest = Some(7);
    audio.kind = TrackKindRow::Audio;
    f.set_lanes(3, vec![audio, a, b]);
    let (_f, mut d) = make_with(f);
    d.select(Some(3));
    let lane = |d: &DocumentModel, dest, p| d.mixer().unwrap().midi_lane(d, dest, p).map(|l| l.id);
    // Nothing selected: the armed track is the focus.
    assert_eq!(lane(&d, 7, P::Strip), Some(31));
    d.select_track(30, false);
    assert_eq!(lane(&d, 7, P::Strip), Some(30), "the selected track");
    assert_eq!(lane(&d, 7, P::View), Some(31), "the mixer: the first armed");
    assert_eq!(lane(&d, 8, P::Strip), None, "it does not play 8");
    d.select_track(32, false);
    assert_eq!(lane(&d, 7, P::Strip), None, "an audio track plays no MIDI");
}

#[test]
fn a_midi_strip_is_selected_while_a_focused_track_plays_it() {
    // Swift `isMidiSelected`: the selection's tracks, else the armed ones.
    let f = fake();
    f.set_destinations(vec![
        ext_dest(7, "Juno", MidiMixRow::default()),
        ext_dest(8, "Moog", MidiMixRow::default()),
    ]);
    let mut a = lane_row(30, "A", false);
    a.midi_dest = Some(7);
    let mut b = lane_row(31, "B", false);
    b.midi_dest = Some(8);
    f.set_lanes(3, vec![a, b]);
    let (_f, mut d) = make_with(f);
    d.select(Some(3));
    let on = |d: &DocumentModel, dest| d.mixer().unwrap().is_midi_selected(d, dest);
    assert!(!on(&d, 7) && !on(&d, 8), "nothing focused");
    d.select_track(31, false);
    assert!(!on(&d, 7) && on(&d, 8));
    d.select_track(30, true);
    assert!(on(&d, 7) && on(&d, 8), "both selected");
}

// `destination.set_midi_mix`.

fn i(n: &str, v: i64) -> (String, ArgValue) {
    (n.into(), ArgValue::Int(v))
}
fn t(n: &str, v: &str) -> (String, ArgValue) {
    (n.into(), ArgValue::Text(v.into()))
}
fn b(n: &str, v: bool) -> (String, ArgValue) {
    (n.into(), ArgValue::Flag(v))
}

fn run(args: &[(String, ArgValue)], d: &mut DocumentModel) -> Result<Ran, ControlError> {
    commands::run("destination.set_midi_mix", args, d)
}

/// Juno (7) is external; the Built-in Synth (1) is not.
fn command_fake() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(mixer_fake());
    f.set_destinations(vec![
        dest_row(1, "Built-in Synth"),
        ext_dest(7, "Juno", MidiMixRow::default()),
    ]);
    let d = DocumentModel::new(f.clone()).unwrap();
    (f, d)
}

#[test]
fn set_midi_mix_checks_its_arguments() {
    let (f, mut d) = command_fake();
    assert!(MIXER_COMMANDS.contains(&"destination.set_midi_mix"));
    assert!(commands::enabled("destination.set_midi_mix", &d));
    let dest = i("dest", 7);
    let cases: Vec<Vec<(String, ArgValue)>> = vec![
        vec![i("volume", 90)],
        vec![t("dest", "Juno"), i("volume", 90)],
        vec![i("dest", -7), i("volume", 90)],
        // The built-in synth and an unknown destination are not external.
        vec![i("dest", 1), i("volume", 90)],
        vec![i("dest", 99), i("volume", 90)],
        vec![dest.clone(), i("volume", 128)],
        vec![dest.clone(), i("volume", -1)],
        vec![dest.clone(), i("volume", i64::MAX)],
        vec![dest.clone(), i("pan", 128)],
        vec![dest.clone(), t("volume", "90")],
        vec![dest.clone(), t("pan", "None")],
        vec![dest.clone(), b("volume", true)],
        vec![dest.clone(), t("mute", "yes")],
        vec![dest.clone(), i("mute", 1)],
        vec![dest.clone()],
        vec![dest.clone(), i("colour", 1)],
        vec![dest.clone(), b("mute", true), b("mute", false)],
    ];
    for args in cases {
        let e = run(&args, &mut d).expect_err("refused");
        assert_eq!(e.code, ErrorCode::InvalidParams, "{args:?}");
    }
    assert!(f.midi_mixes().is_empty());
    assert_eq!(
        run(&[i("dest", 1), b("mute", true)], &mut d)
            .unwrap_err()
            .message,
        "destination.set_midi_mix: destination 1 is not an external MIDI destination"
    );
    f.fail_next(
        "set_midi_mix",
        VizError::Model {
            message: "MIDI mix out of range".into(),
        },
    );
    let e = run(&[dest.clone(), b("mute", true)], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Set MIDI Mix Failed: MIDI mix out of range");
    f.fail_next(
        "set_midi_mix",
        VizError::Busy {
            message: "Release the touched control first.".into(),
        },
    );
    let e = run(&[dest.clone(), b("mute", true)], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    f.fail_next(
        "destinations",
        VizError::Model {
            message: "no".into(),
        },
    );
    let e = run(&[dest, b("mute", true)], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(
        e.message,
        "Set MIDI Mix Failed: the destinations cannot be read"
    );
}

#[test]
fn set_midi_mix_sets_and_keeps_absent_values() {
    let (f, mut d) = command_fake();
    let dest = i("dest", 7);
    let mut set = |args: Vec<(String, ArgValue)>| {
        let mut all = vec![dest.clone()];
        all.extend(args);
        run(&all, &mut d).expect("ran");
    };
    let now = |f: &FakeSession| f.midi_mixes().last().unwrap().1.clone();
    set(vec![i("volume", 90)]);
    assert_eq!(now(&f), mix(Some(90), None, false));
    set(vec![i("pan", 20)]);
    assert_eq!(now(&f), mix(Some(90), Some(20), false));
    set(vec![b("mute", true)]);
    assert_eq!(now(&f), mix(Some(90), Some(20), true));
    // The channel's null reaches the app as the text `none`: unset.
    set(vec![t("volume", "none")]);
    assert_eq!(now(&f), mix(None, Some(20), true));
    set(vec![i("pan", 0), i("volume", 127), b("mute", false)]);
    assert_eq!(now(&f), mix(Some(127), Some(0), false));
    assert_eq!(f.midi_mixes().len(), 5);
    assert!(f.midi_mixes().iter().all(|(dest, _)| *dest == 7));
    assert_eq!(d.snapshot().version, f.snap().version, "each step applied");
}

/// Carried from Task 3: on the real core, a MIDI fader drag previews
/// without touching the document (the core takes the previews it sends)
/// and its release is one step "Set MIDI Mix" holding the new volume.
#[test]
fn a_real_midi_drag_previews_and_commits_on_the_core() {
    use core_model::command::{Command, RoutingCmd, apply};
    use core_model::document::{DestKind, Document};
    use vizdaw_app::DocSession;
    let add = Command::Routing(RoutingCmd::AddDestination {
        name: "Juno".into(),
        kind: DestKind::MidiEndpoint {
            endpoint_ref: "Juno Port".into(),
            group: 0,
            channel: 0,
        },
        default_patch: None,
    });
    let doc = apply(&Document::new(), &add).unwrap().doc;
    // A package holding it, opened on the core.
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("Juno.vizdaw");
    std::fs::create_dir(&package).unwrap();
    let json = core_model::file::to_json(&doc).unwrap();
    std::fs::write(package.join(core_model::file::PROJECT_FILE), json).unwrap();
    let session = vizdaw_ffi::Session::open(package.display().to_string()).unwrap();
    let mut d = DocumentModel::new(session.clone() as Arc<dyn DocSession>).unwrap();
    assert!(d.ensure_mixer());
    let dest = d
        .mixer()
        .unwrap()
        .destinations()
        .iter()
        .find(|x| x.external)
        .expect("Juno")
        .id;
    // The previews' shape is one the core takes (the app drops a refused
    // preview silently, as Swift does).
    assert!(session.preview_midi_mix(dest, Some(90), None).is_ok());
    assert!(session.preview_midi_mix(dest, None, Some(64)).is_ok());
    let t0 = Instant::now();
    mx(&mut d, |m, d| {
        m.midi_press(d, P::View, dest, MidiControl::Volume, 127.0, t0);
        m.midi_drag(d, P::View, 0.0, 10.0, false, t0);
        m.midi_drag(d, P::View, 0.0, 30.0, false, t0 + ms(30));
    });
    let core_mix = |s: &vizdaw_ffi::Session| {
        s.destinations()
            .unwrap()
            .into_iter()
            .find(|x| x.id == dest)
            .unwrap()
            .midi_mix
    };
    assert_eq!(core_mix(&session), MidiMixRow::default(), "a preview only");
    assert!(session.snapshot().unwrap().undo_label.is_none());
    let shown = d.mixer().unwrap().midi_mix(dest).unwrap().volume;
    // 20 of 127 points down from where it began moving.
    assert_eq!(shown, Some(80));
    mx(&mut d, |m, d| m.midi_lift(d, P::View, t0 + ms(40)));
    assert!(d.alert.is_none(), "{:?}", d.alert);
    assert_eq!(core_mix(&session).volume, Some(80));
    assert_eq!(d.snapshot().undo_label.as_deref(), Some("Set MIDI Mix"));
    session.close().unwrap();
}
