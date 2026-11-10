//! The MIDI mix on external destinations (plan 5g-1b Task 1; spec §6.8a):
//! volume, pan and mute are stored on the destination, checked, undoable,
//! external-only, and written to files only when set.

use core_model::ModelError;
use core_model::command::{Command, RoutingCmd, apply};
use core_model::document::{DestKind, Document, MIDI_CENTRE, MidiMix};
use core_model::file::to_json;
use core_model::history::History;
use core_model::ids::{AnyId, DestId};
use core_model::selection::ViewFilter;
use serde_json::Value;

fn add_dest(d: &mut Document, name: &str, kind: DestKind) -> DestId {
    let a = apply(
        d,
        &Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: None,
        }),
    )
    .unwrap();
    *d = a.doc;
    a.report
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Dest(x) => Some(*x),
            _ => None,
        })
        .unwrap()
}

fn endpoint(name: &str) -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: name.into(),
        group: 0,
        channel: 0,
    }
}

fn set(dest: DestId, mix: MidiMix) -> Command {
    Command::Routing(RoutingCmd::SetMidiMix { dest, mix })
}

fn mix(volume: Option<u8>, pan: Option<u8>, mute: bool) -> MidiMix {
    MidiMix { volume, pan, mute }
}

#[test]
fn midi_mix_is_checked_and_external_only() {
    let mut d = Document::new();
    let juno = add_dest(&mut d, "Juno", endpoint("Juno"));
    let pad = add_dest(&mut d, "Pad", DestKind::plugin("au:pad"));
    let synth = add_dest(&mut d, "Synth", endpoint("builtin:synth"));
    assert_eq!(MIDI_CENTRE, 64);
    assert!(d.instruments().get(&juno).unwrap().midi_mix.is_default());

    // Stored, and Undo restores the default.
    let mut h = History::new(d.clone());
    let m = mix(Some(90), Some(20), true);
    h.execute(&set(juno, m), &ViewFilter::default()).unwrap();
    assert_eq!(h.undo_label(), Some("Set MIDI Mix"));
    assert_eq!(h.doc().instruments().get(&juno).unwrap().midi_mix, m);
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
    assert_eq!(MidiMix::default(), mix(None, None, false));

    // Range.
    match apply(&d, &set(juno, mix(Some(128), None, false))) {
        Err(ModelError::OutOfRange { field, .. }) => assert_eq!(field, "midi_volume"),
        other => panic!("expected Range, got {other:?}"),
    }
    match apply(&d, &set(juno, mix(None, Some(128), false))) {
        Err(ModelError::OutOfRange { field, .. }) => assert_eq!(field, "midi_pan"),
        other => panic!("expected Range, got {other:?}"),
    }
    assert!(apply(&d, &set(juno, mix(Some(127), Some(127), false))).is_ok());

    // Not external, or unknown.
    for dest in [pad, synth, DestId(999)] {
        assert!(apply(&d, &set(dest, mix(Some(90), None, false))).is_err());
        let mut h = History::new(d.clone());
        assert!(
            h.execute(
                &set(dest, mix(Some(90), None, false)),
                &ViewFilter::default()
            )
            .is_err()
        );
        assert_eq!(h.doc(), &d);
    }
}

#[test]
fn an_untouched_destination_writes_no_midi_mix() {
    let mut d = Document::new();
    let juno = add_dest(&mut d, "Juno", endpoint("Juno"));
    assert!(!to_json(&d).unwrap().contains("midi_mix"));
    let d2 = apply(&d, &set(juno, mix(Some(90), None, false)))
        .unwrap()
        .doc;
    let v: Value = serde_json::from_str(&to_json(&d2).unwrap()).unwrap();
    assert!(v.to_string().contains("midi_mix"));
}
