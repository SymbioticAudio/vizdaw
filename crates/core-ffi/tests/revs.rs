//! Lane revisions (spec §3.1): only lanes that changed get a new one.

use core_engine::fixtures::Builder;
use core_model::command::Command;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::sequence::KeySlot;
use vizdaw_ffi::{DocSnapshot, Session};

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

fn rev(s: &DocSnapshot, seq: u64, lane: u64) -> u64 {
    s.lane_revs
        .iter()
        .find(|r| r.seq == seq && r.lane == lane)
        .unwrap_or_else(|| panic!("no revision for {seq}/{lane}"))
        .rev
}

#[test]
fn every_lane_has_a_revision() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    let lanes: u32 = snap.sequences.iter().map(|r| r.lane_count).sum();
    assert_eq!(snap.lane_revs.len(), lanes as usize);
    for row in &snap.sequences {
        for lane in s.lanes(row.id).unwrap() {
            rev(&snap, row.id, lane.id);
        }
    }
    let mut all: Vec<u64> = snap.lane_revs.iter().map(|r| r.rev).collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), snap.lane_revs.len(), "revisions are unique");
}

#[test]
fn only_the_changed_lane_gets_a_new_revision() {
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let (song, beat) = (id_of(&before, "Song"), id_of(&before, "Beat"));
    let lanes = s.lanes(song).unwrap();
    let (groove, chords) = (lanes[0].id, lanes[1].id);
    let kit = s.lanes(beat).unwrap()[0].id;
    let after = s.set_lane_mute(song, chords, true).unwrap();
    assert_ne!(rev(&after, song, chords), rev(&before, song, chords));
    assert_eq!(rev(&after, song, groove), rev(&before, song, groove));
    assert_eq!(rev(&after, beat, kit), rev(&before, beat, kit));
}

#[test]
fn undo_and_redo_give_fresh_revisions() {
    let s = Session::new_demo().unwrap();
    let a = s.snapshot().unwrap();
    let song = id_of(&a, "Song");
    let lanes = s.lanes(song).unwrap();
    let (groove, chords) = (lanes[0].id, lanes[1].id);
    let b = s.set_lane_mute(song, chords, true).unwrap();
    let c = s.undo().unwrap();
    let d = s.redo().unwrap();
    let revs = [a, b.clone(), c.clone(), d.clone()].map(|x| rev(&x, song, chords));
    let mut unique = revs.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 4, "never an old number again: {revs:?}");
    assert_eq!(rev(&b, song, groove), rev(&d, song, groove));
    assert_eq!(rev(&c, song, groove), rev(&d, song, groove));
}

#[test]
fn mixer_and_sequence_level_edits_keep_every_revision() {
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let after = s.add_bus("Verb".into()).unwrap();
    assert_eq!(after.lane_revs, before.lane_revs);
    let renamed = s
        .rename_sequence(id_of(&before, "Beat"), "Kit".into())
        .unwrap();
    assert_eq!(renamed.lane_revs, before.lane_revs);
}

#[test]
fn editing_a_referenced_sequence_keeps_the_parent_lane_revision() {
    // Plan Ruling 2: the parent's reference lane keeps its revision; the
    // app refetches reference blocks when a sequence row changes instead.
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let (song, groove) = (id_of(&before, "Song"), id_of(&before, "Groove"));
    let song_groove = s.lanes(song).unwrap()[0].id;
    let bass = s.lanes(groove).unwrap()[1].id;
    let after = s.set_lane_mute(groove, bass, true).unwrap();
    assert_ne!(rev(&after, groove, bass), rev(&before, groove, bass));
    assert_eq!(
        rev(&after, song, song_groove),
        rev(&before, song, song_groove)
    );
}

#[test]
fn rows_carry_arm_routing_and_keyboard_slots() {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("A", "Keys", Some(piano)).unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetKeyboardSlot {
        seq,
        slot: Some(KeySlot {
            key: "a".into(),
            modifiers: 4,
        }),
    }))
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let row = &s.lanes(seq.0).unwrap()[0];
    assert!(row.arm);
    assert_eq!(row.midi_dest, Some(piano.0));
    assert_eq!(row.audio_out, None);
    let snap = s.snapshot().unwrap();
    let slot = snap.sequences[0].keyboard_slot.clone().unwrap();
    assert_eq!((slot.key.as_str(), slot.modifiers), ("a", 4));
}
