//! Lane revisions (spec §3.1): only lanes that changed get a new one.

use core_engine::fixtures::Builder;
use core_model::command::Command;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::sequence::KeySlot;
use vizdaw_ffi::{DocSnapshot, EditCommand, MeterEntryRow, SequenceEdit, Session, TempoPointRow};

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
fn mixer_edits_and_renames_keep_every_revision() {
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

/// Every lane of `seq` changed; every lane of another sequence did not.
fn only_sequence_bumped(before: &DocSnapshot, after: &DocSnapshot, seq: u64) {
    assert_eq!(before.lane_revs.len(), after.lane_revs.len());
    for (old, new) in before.lane_revs.iter().zip(&after.lane_revs) {
        assert_eq!((old.seq, old.lane), (new.seq, new.lane));
        if old.seq == seq {
            assert_ne!(old.rev, new.rev, "lane {} of the edited sequence", old.lane);
        } else {
            assert_eq!(
                old.rev, new.rev,
                "lane {} of sequence {}",
                old.lane, old.seq
            );
        }
    }
}

#[test]
fn a_tempo_change_bumps_every_lane_of_that_sequence_only() {
    // Region durations are computed through the tempo map.
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let song = id_of(&before, "Song");
    let after = s
        .edit(
            EditCommand::Sequence {
                edit: SequenceEdit::SetTempo {
                    seq: song,
                    tempo: vec![TempoPointRow {
                        tick: 0,
                        bpm: 97.0,
                        ramp: false,
                    }],
                },
            },
            None,
        )
        .unwrap();
    only_sequence_bumped(&before, &after, song);
}

#[test]
fn a_meter_change_bumps_every_lane_of_that_sequence_only() {
    // Fixed-bar blocks are computed through the meter map.
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let beat = id_of(&before, "Beat");
    let after = s
        .edit(
            EditCommand::Sequence {
                edit: SequenceEdit::SetMeter {
                    seq: beat,
                    meter: vec![MeterEntryRow {
                        tick: 0,
                        num: 3,
                        den: 4,
                        sharps: 0,
                        minor: false,
                    }],
                },
            },
            None,
        )
        .unwrap();
    only_sequence_bumped(&before, &after, beat);
}

#[test]
fn a_media_pool_change_bumps_only_lanes_holding_regions() {
    use core_model::command::MediaCmd;
    use core_model::event::{EventBody, Fade};
    use core_model::ids::AnyId;
    use vizdaw_ffi::revs::Revisions;

    let mut b = Builder::new();
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
            name: None,
            origin: None,
        }))
        .unwrap();
    let media = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap();
    let (seq, audio) = b.sequence("S", "Audio", None).unwrap();
    let notes = b.lane(seq, "Notes", None).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 960)]).unwrap();
    b.event(
        seq,
        audio,
        0,
        EventBody::AudioRegion {
            media,
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        },
    )
    .unwrap();
    let mut revs = Revisions::new(b.doc());
    let (audio_rev, notes_rev) = (revs.rev(seq, audio).unwrap(), revs.rev(seq, notes).unwrap());
    b.run(Command::Media(MediaCmd::Add {
        path: "Media/b.wav".into(),
        sample_rate: 44_100,
        channels: 1,
        length_frames: 10,
        content_hash: None,
        name: None,
        origin: None,
    }))
    .unwrap();
    revs.update(b.doc());
    assert_ne!(
        revs.rev(seq, audio).unwrap(),
        audio_rev,
        "region durations are computed through the media pool"
    );
    assert_eq!(revs.rev(seq, notes).unwrap(), notes_rev);
}
