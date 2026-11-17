//! Built-in Synth sounds (plan release-fixes Task 8): five General MIDI
//! sounds, kept in the destination's own patch and channel, so a change
//! reaches every track that plays it.

use core_engine::fixtures::Builder;
use core_model::document::{DestKind, Document, Patch};
use core_model::ids::DestId;
use core_model::mixer::BUILTIN_SYNTH_REF;
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::host::force_offline;
use vizdaw_ffi::{BuiltinSound, Session};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    g
}

/// A document with one Built-in Synth destination on channel 0 and one
/// external destination: `(session, synth, external)`.
fn fixture() -> (Arc<Session>, u64, u64) {
    let mut b = Builder::new();
    let synth = b.dest("Built-in Synth", BUILTIN_SYNTH_REF, 0).unwrap();
    let ext = b.dest("Outboard", "midi:outboard", 0).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    (s, synth.0, ext.0)
}

fn doc(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

/// The destination's (kind channel, default patch).
fn sound_of(s: &Session, dest: u64) -> (u8, Option<Patch>) {
    let d = doc(s);
    let x = d.instruments().get(&DestId(dest)).unwrap().clone();
    match x.kind {
        DestKind::MidiEndpoint { channel, .. } => (channel, x.default_patch),
        DestKind::PluginInstrument { .. } => panic!("not a MIDI endpoint"),
    }
}

fn row_sound(s: &Session, dest: u64) -> Option<BuiltinSound> {
    s.destinations()
        .unwrap()
        .into_iter()
        .find(|r| r.id == dest)
        .unwrap()
        .builtin_sound
}

#[test]
fn set_builtin_sound_sets_program_and_channel() {
    let _g = serial();
    let (s, synth, ext) = fixture();
    s.set_builtin_sound(synth, BuiltinSound::Drums).unwrap();
    assert_eq!(
        sound_of(&s, synth),
        (
            9,
            Some(Patch {
                program: 0,
                bank: None
            })
        )
    );
    assert_eq!(row_sound(&s, synth), Some(BuiltinSound::Drums));
    s.set_builtin_sound(synth, BuiltinSound::Pad).unwrap();
    assert_eq!(
        sound_of(&s, synth),
        (
            0,
            Some(Patch {
                program: 89,
                bank: None
            })
        )
    );
    assert_eq!(row_sound(&s, synth), Some(BuiltinSound::Pad));
    assert_eq!(row_sound(&s, ext), None);
}

#[test]
fn set_builtin_sound_undoes_as_one_step() {
    let _g = serial();
    let (s, synth, _) = fixture();
    s.set_builtin_sound(synth, BuiltinSound::Drums).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Built-in Synth Sound")
    );
    s.undo().unwrap();
    assert_eq!(sound_of(&s, synth), (0, None));
    assert_eq!(row_sound(&s, synth), Some(BuiltinSound::Piano));
}

#[test]
fn set_builtin_sound_refuses_other_destinations() {
    let _g = serial();
    let (s, _, ext) = fixture();
    let before = s.debug_document_json();
    let label = s.snapshot().unwrap().undo_label;
    let e = s.set_builtin_sound(ext, BuiltinSound::Bass).unwrap_err();
    assert_eq!(e.to_string(), "Only a Built-in Synth has these sounds.");
    assert!(s.set_builtin_sound(u64::MAX, BuiltinSound::Bass).is_err());
    assert_eq!(s.debug_document_json(), before);
    assert_eq!(s.snapshot().unwrap().undo_label, label);
}

#[test]
fn builtin_sound_of_round_trips() {
    for b in [
        BuiltinSound::Piano,
        BuiltinSound::Bass,
        BuiltinSound::Pad,
        BuiltinSound::Lead,
        BuiltinSound::Drums,
    ] {
        let (p, c) = b.program_and_channel();
        assert_eq!(BuiltinSound::of(Some(p), c), Some(b));
    }
    assert_eq!(BuiltinSound::of(None, 0), Some(BuiltinSound::Piano));
    assert_eq!(BuiltinSound::of(Some(5), 0), None);
}
