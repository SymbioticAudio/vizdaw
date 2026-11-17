//! Note chores on named tracks (plan 8c Task 2, Ruling 2): a
//! `ModifyNotes` changes only the notes of its tracks in its range, never
//! the selection or Modify Again's memory, and refuses a track with no
//! notes there.

use core_model::command::{AudioCmd, Command, EventCmd, LaneCmd, MediaCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::error::ModelError;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::history::History;
use core_model::ids::{LaneId, MediaId, SeqId};
use core_model::modify::{Modify, Quantize, Transpose};
use core_model::selection::{Selection, ViewFilter};
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> Document {
    apply(d, &c).unwrap().doc
}

fn add_notes(d: &Document, seq: u64, lane: u64, notes: &[(i64, u8)]) -> Document {
    run(
        d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            events: notes
                .iter()
                .map(|(t, p)| EventSpec::new(Tick(*t), EventBody::note(*p, 0x8000, Tick(480))))
                .collect(),
        }),
    )
}

/// Sequence 0 with lane 0 "Keys" and lane 1 "Bass", four notes each,
/// slightly off a 3840-tick grid: at 100, 4000, 16000 and 20000.
fn song() -> Document {
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New {
            name: "Song".into(),
        }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Bass".into(),
            index: None,
        }),
    );
    let at = [(100, 60), (4000, 62), (16000, 64), (20000, 65)];
    let d = add_notes(&d, 0, 0, &at);
    add_notes(&d, 0, 1, &at)
}

/// `(tick, pitch)` of each note of `lane`'s active take, in order.
fn notes_of(d: &Document, lane: u64) -> Vec<(i64, u8)> {
    let l = d.lane(SeqId(0), LaneId(lane)).unwrap();
    l.active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note { pitch, .. } => Some((e.tick.0, pitch)),
            _ => None,
        })
        .collect()
}

fn transpose(lanes: Vec<u64>, semitones: i8) -> Command {
    Command::ModifyNotes {
        seq: SeqId(0),
        lanes: lanes.into_iter().map(LaneId).collect(),
        range: None,
        modify: Modify::Transpose(Transpose::Interval { semitones }),
    }
}

#[test]
fn modify_notes_touches_only_its_tracks() {
    let d = song();
    let mut h = History::new(d.clone());
    let sel = Selection {
        time_range: None,
        lanes: [(SeqId(0), LaneId(0))].into_iter().collect(),
        events: d
            .lane(SeqId(0), LaneId(0))
            .unwrap()
            .active()
            .unwrap()
            .events
            .values()
            .map(|e| e.id)
            .collect(),
    };
    h.set_selection(sel.clone());
    h.execute(&transpose(vec![1], 2), &ViewFilter::default())
        .unwrap();
    assert_eq!(notes_of(h.doc(), 0), notes_of(&d, 0));
    assert_eq!(
        notes_of(h.doc(), 1),
        vec![(100, 62), (4000, 64), (16000, 66), (20000, 67)]
    );
    assert_eq!(h.selection(), &sel);
    assert_eq!(h.last_modify(), None);
    assert_eq!(h.undo_label(), Some("Transpose"));
}

#[test]
fn modify_notes_keeps_to_the_range() {
    let d = song();
    let after = run(
        &d,
        Command::ModifyNotes {
            seq: SeqId(0),
            lanes: vec![LaneId(1)],
            range: Some((Tick(0), Tick(15360))),
            modify: Modify::Quantize(Quantize::Grid {
                grid: Tick(3840),
                strength: 100,
            }),
        },
    );
    assert_eq!(
        notes_of(&after, 1),
        vec![(0, 60), (3840, 62), (16000, 64), (20000, 65)]
    );
    assert_eq!(notes_of(&after, 0), notes_of(&d, 0));
}

fn audio_lane(d: &Document) -> Document {
    let d = run(
        d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Vox".into(),
            index: None,
        }),
    );
    let d = run(
        &d,
        Command::Media(MediaCmd::Add {
            path: "Media/a.caf".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 192_000,
            content_hash: None,
            name: None,
            origin: None,
        }),
    );
    let fade = Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    };
    run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(2),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 0,
            length: 1000,
            gain_db: 0.0,
            fade_in: fade,
            fade_out: fade,
        }),
    )
}

#[test]
fn note_chores_refuse_odd_targets() {
    let d = audio_lane(&song());
    // Sequence 1 with its own lane (ID 3) and notes.
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::New {
            name: "Other".into(),
        }),
    );
    let other = d.sequence(SeqId(1)).unwrap().lanes[0].id;
    let d = add_notes(&d, 1, other.0, &[(0, 60)]);
    let refused = |lanes: Vec<u64>, range: Option<(Tick, Tick)>, id: u64| {
        let c = Command::ModifyNotes {
            seq: SeqId(0),
            lanes: lanes.into_iter().map(LaneId).collect(),
            range,
            modify: Modify::Transpose(Transpose::Interval { semitones: 2 }),
        };
        let mut h = History::new(d.clone());
        let e = h.execute(&c, &ViewFilter::default()).unwrap_err();
        assert_eq!(
            e,
            ModelError::Invalid {
                reason: format!("Track {id} has no notes there."),
            }
        );
        assert_eq!(h.doc(), &d);
        assert_eq!(h.undo_label(), None);
    };
    refused(vec![1, other.0], None, other.0);
    refused(vec![2], None, 2);
    refused(vec![0], Some((Tick(25000), Tick(30000))), 0);
}

#[test]
fn transposing_clamps() {
    let d = add_notes(&song(), 0, 0, &[(30000, 127)]);
    let after = run(&d, transpose(vec![0], 2));
    assert_eq!(notes_of(&after, 0).last(), Some(&(30000, 127)));
}
