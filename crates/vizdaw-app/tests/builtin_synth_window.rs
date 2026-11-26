//! The Built-in Synth window's model (Swift
//! `SynthSheetsTests.soundsRoundTripThroughTheSession`).

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake, dest_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::builtin_synth_window::{
    BuiltinSynthWindowModel, CAPTION, NOT_BUILTIN_SYNTH, SOUND, SOUNDS, SoundOutcome, TITLE,
    builtin_synth_destination, sound_title,
};
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::{BuiltinSound, VizError};

fn sound_fake() -> Arc<FakeSession> {
    let f = demo_fake();
    f.edit_destinations(|d| {
        *d = vec![
            DestRow {
                channel: Some(0),
                builtin_sound: Some(BuiltinSound::Piano),
                ..dest_row(7, "Built-in Synth")
            },
            DestRow {
                external: true,
                return_latency_ms: 12.,
                channel: Some(0),
                port: Some("Ext MIDI".into()),
                ..dest_row(9, "Ext Synth")
            },
        ]
    });
    Arc::new(f)
}

#[test]
fn sounds_round_trip_through_the_session() {
    let f = sound_fake();
    let mut d = DocumentModel::new(f.clone()).unwrap();
    let mut w = BuiltinSynthWindowModel::new(&d, 7);
    assert_eq!(w.dest(), 7);
    assert_eq!(w.sound(), Some(BuiltinSound::Piano));
    assert_eq!(BuiltinSynthWindowModel::new(&d, 9).sound(), None);
    assert_eq!(BuiltinSynthWindowModel::new(&d, 999).sound(), None);
    let version = d.snapshot().version;
    assert_eq!(w.set_sound(&mut d, BuiltinSound::Bass), SoundOutcome::Done);
    assert_eq!(f.builtin_sounds(), [(7, BuiltinSound::Bass)]);
    assert_eq!(
        w.sound(),
        Some(BuiltinSound::Bass),
        "read back from the session's row"
    );
    assert!(d.snapshot().version > version, "the step was applied");
    // Not a Built-in Synth: no call.
    let mut other = BuiltinSynthWindowModel::new(&d, 9);
    assert_eq!(
        other.set_sound(&mut d, BuiltinSound::Drums),
        SoundOutcome::Unavailable(NOT_BUILTIN_SYNTH.into())
    );
    let mut unknown = BuiltinSynthWindowModel::new(&d, 999);
    assert_ne!(
        unknown.set_sound(&mut d, BuiltinSound::Drums),
        SoundOutcome::Done
    );
    assert_eq!(f.builtin_sounds().len(), 1);
    // The core's refusal is said.
    f.fail_next(
        "set_builtin_sound",
        VizError::Busy {
            message: "Finish the open edit first.".into(),
        },
    );
    assert_eq!(
        w.set_sound(&mut d, BuiltinSound::Lead),
        SoundOutcome::Busy("Finish the open edit first.".into())
    );
    assert_eq!(d.status(), Some("Finish the open edit first."));
    f.fail_next(
        "set_builtin_sound",
        VizError::Model {
            message: NOT_BUILTIN_SYNTH.into(),
        },
    );
    assert_eq!(
        w.set_sound(&mut d, BuiltinSound::Lead),
        SoundOutcome::Refused(NOT_BUILTIN_SYNTH.into())
    );
    assert_eq!(w.sound(), Some(BuiltinSound::Bass));
    assert_eq!(
        SOUNDS,
        [
            BuiltinSound::Piano,
            BuiltinSound::Bass,
            BuiltinSound::Pad,
            BuiltinSound::Lead,
            BuiltinSound::Drums
        ]
    );
    let titles: Vec<&str> = SOUNDS.iter().map(|s| sound_title(*s)).collect();
    assert_eq!(titles, ["Piano", "Bass", "Pad", "Lead", "Drums"]);
    assert_eq!(
        (TITLE, SOUND, CAPTION),
        (
            "Built-in Synth",
            "Sound",
            "Changes every track that plays this Built-in Synth."
        )
    );
}

/// The window follows every step (an undo, too), reading once per
/// document version.
#[test]
fn the_window_follows_every_snapshot() {
    let f = sound_fake();
    let mut d = DocumentModel::new(f.clone()).unwrap();
    let mut w = BuiltinSynthWindowModel::new(&d, 7);
    assert!(!w.needs_refresh(&d));
    f.edit_destinations(|ds| ds[0].builtin_sound = Some(BuiltinSound::Pad));
    let mut next = f.snap();
    next.version += 1;
    f.set_idle(Some(next));
    d.idle();
    assert!(w.needs_refresh(&d));
    w.refresh(&d);
    assert_eq!(w.sound(), Some(BuiltinSound::Pad));
    assert!(!w.needs_refresh(&d));
    // A refused read keeps the last and is read again.
    f.fail_next(
        "destinations",
        VizError::Busy {
            message: "b".into(),
        },
    );
    let mut next = f.snap();
    next.version += 1;
    f.set_idle(Some(next));
    d.idle();
    w.refresh(&d);
    assert_eq!(w.sound(), Some(BuiltinSound::Pad));
    assert!(w.needs_refresh(&d));
}

/// The shared Built-in Synth strip's destination: the focused one when it
/// is a Built-in Synth, else the first.
#[test]
fn the_builtin_synth_strip_opens_its_destination() {
    let dests = vec![
        dest_row(4, "Ext"),
        dest_row(5, "Built-in Synth"),
        dest_row(6, "Built-in Synth 2"),
    ];
    let mut dests = dests;
    dests[0].external = true;
    assert_eq!(builtin_synth_destination(&dests, None), Some(5));
    assert_eq!(builtin_synth_destination(&dests, Some(6)), Some(6));
    assert_eq!(builtin_synth_destination(&dests, Some(4)), Some(5));
    assert_eq!(builtin_synth_destination(&dests[..1], None), None);
}
