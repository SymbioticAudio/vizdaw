use core_model::command::{Command, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventKind, EventSpec, LoopCount};
use core_model::ids::{Counters, DestId, EventId, LaneId, SeqId};
use core_model::query::flatten::{PathStep, ResolvedEvent, flatten};
use core_model::selection::ViewFilter;
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn run(d: &Document, c: Command) -> Document {
    apply(d, &c).unwrap().doc
}

fn insert(d: &Document, seq: u64, lane: u64, specs: Vec<EventSpec>) -> Document {
    run(
        d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            events: specs,
        }),
    )
}

fn note(beat: i64, pitch: u8, beats: i64) -> EventSpec {
    EventSpec::new(Tick(beat * Q), EventBody::note(pitch, 100, Tick(beats * Q)))
}

fn reference(
    beat: i64,
    length_beats: Option<i64>,
    loop_count: LoopCount,
    transpose: i8,
) -> EventSpec {
    EventSpec::new(
        Tick(beat * Q),
        EventBody::Reference {
            seq: SeqId(1),
            length: length_beats.map(|b| Tick(b * Q)),
            loop_count,
            transpose,
        },
    )
}

/// dest0 (plugin) and dest1 (MIDI endpoint group 2 channel 5).
/// seq0 "Song" (lane0, dest0) and seq1 "Sub" (lane1, dest1), both 120 BPM 4/4.
fn base() -> Document {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![
            Destination {
                id: DestId(0),
                name: "Plug".into(),
                kind: DestKind::PluginInstrument {
                    plugin_ref: "p".into(),
                },
                default_patch: None,
            },
            Destination {
                id: DestId(1),
                name: "Ext".into(),
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: "e".into(),
                    group: 2,
                    channel: 5,
                },
                default_patch: None,
            },
        ],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        counters: Counters {
            dest: 2,
            ..Counters::default()
        },
    })
    .unwrap();
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::New {
            name: "Song".into(),
        }),
    );
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::New { name: "Sub".into() }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    );
    run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(1),
            lane: LaneId(1),
            dest: Some(DestId(1)),
        }),
    )
}

fn flat(d: &Document) -> Vec<ResolvedEvent> {
    flatten(d, SeqId(0), None, &ViewFilter::default())
        .unwrap()
        .collect()
}

/// `(beat, pitch, duration in beats)` of every resolved note.
fn notes(d: &Document) -> Vec<(f64, u8, f64)> {
    flat(d)
        .into_iter()
        .filter_map(|r| match r.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some((
                r.tick.0 as f64 / Q as f64,
                pitch,
                duration.0 as f64 / Q as f64,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn plain_notes_resolve_with_lane_routing() {
    let d = insert(&base(), 0, 0, vec![note(1, 62, 1), note(0, 60, 1)]);
    let r = flat(&d);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].tick, Tick(0));
    assert_eq!(
        (r[0].dest, r[0].group_channel),
        (Some(DestId(0)), Some((0, 0)))
    );
    assert!((r[1].seconds - 0.5).abs() < 1e-9);
    assert_eq!(
        r[1].path,
        vec![PathStep {
            seq: SeqId(0),
            lane: LaneId(0),
            event: EventId(0)
        }]
    );
}

#[test]
fn nested_events_use_inner_routing_and_carry_their_path() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1)]);
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(1), 0)]);
    let r = flat(&d);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].tick, Tick(4 * Q));
    assert_eq!(
        (r[0].dest, r[0].group_channel),
        (Some(DestId(1)), Some((2, 5)))
    );
    assert_eq!(
        r[0].path,
        vec![
            PathStep {
                seq: SeqId(0),
                lane: LaneId(0),
                event: EventId(1)
            },
            PathStep {
                seq: SeqId(1),
                lane: LaneId(1),
                event: EventId(0)
            },
        ]
    );
}

#[test]
fn nested_timing_uses_the_playing_sequences_tempo() {
    let d = insert(&base(), 1, 1, vec![note(1, 60, 1)]);
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::SetTempo {
            seq: SeqId(1),
            tempo: TempoMap::constant(Bpm::whole(60)).unwrap(),
        }),
    );
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(1), 0)]);
    let r = flat(&d);
    // Beat 5 at the song's 120 BPM is 2.5 s; the sub's own 60 BPM is ignored.
    assert_eq!(r[0].tick, Tick(5 * Q));
    assert!((r[0].seconds - 2.5).abs() < 1e-9);
}

// Review focus #2: loop_count combined with a per-use length shorter than the sequence.
#[test]
fn loop_count_with_length_shorter_than_sequence() {
    let d = insert(
        &base(),
        1,
        1,
        vec![
            note(0, 60, 1),
            note(1, 61, 2),
            note(2, 62, 1),
            note(3, 63, 1),
        ],
    );
    let d = insert(
        &d,
        0,
        0,
        vec![reference(0, Some(2), LoopCount::Count(3), 0)],
    );
    assert_eq!(
        notes(&d),
        vec![
            (0.0, 60, 1.0),
            (1.0, 61, 1.0),
            (2.0, 60, 1.0),
            (3.0, 61, 1.0),
            (4.0, 60, 1.0),
            (5.0, 61, 1.0)
        ]
    );
}

#[test]
fn length_longer_than_sequence_lets_looped_lanes_fill() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(1),
            lane: LaneId(1),
            length: Some(Tick(Q)),
            looped: true,
            locked: false,
        }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(1),
            name: "once".into(),
            index: None,
        }),
    );
    let d = insert(&d, 1, 2, vec![note(0, 70, 1)]);
    let d = insert(
        &d,
        0,
        0,
        vec![reference(0, Some(6), LoopCount::Count(1), 0)],
    );
    let n = notes(&d);
    assert_eq!(n.iter().filter(|x| x.1 == 60).count(), 6);
    assert_eq!(n.iter().filter(|x| x.1 == 70).count(), 1);
}

#[test]
fn fill_repeats_sequence_and_truncates_last_pass() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1), note(3, 63, 1)]);
    let d = insert(&d, 0, 0, vec![reference(0, Some(6), LoopCount::Fill, 0)]);
    assert_eq!(
        notes(&d),
        vec![(0.0, 60, 1.0), (3.0, 63, 1.0), (4.0, 60, 1.0)]
    );
}

#[test]
fn transpose_accumulates_and_clamps() {
    let d = insert(&base(), 1, 1, vec![note(0, 120, 1), note(1, 5, 1)]);
    let d = insert(&d, 0, 0, vec![reference(0, None, LoopCount::Count(1), 12)]);
    assert_eq!(notes(&d), vec![(0.0, 127, 1.0), (1.0, 17, 1.0)]);
    let d = insert(&base(), 1, 1, vec![note(0, 3, 1)]);
    let d = insert(&d, 0, 0, vec![reference(0, None, LoopCount::Count(1), -12)]);
    assert_eq!(notes(&d), vec![(0.0, 0, 1.0)]);
}

#[test]
fn mute_solo_and_active_take() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1)]);
    let mut muted_ref = reference(0, None, LoopCount::Count(1), 0);
    muted_ref.mute = true;
    let d = insert(&d, 0, 0, vec![muted_ref, note(1, 50, 1)]);
    assert_eq!(notes(&d), vec![(1.0, 50, 1.0)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "solo".into(),
            index: None,
        }),
    );
    let d = insert(&d, 0, 2, vec![note(2, 52, 1)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(2),
            mute: None,
            solo: Some(true),
            arm: None,
        }),
    );
    assert_eq!(notes(&d), vec![(2.0, 52, 1.0)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::AddTake {
            seq: SeqId(0),
            lane: LaneId(2),
            name: "T2".into(),
        }),
    );
    assert_eq!(notes(&d), vec![]);
}

#[test]
fn pickup_plays_only_before_the_first_pass() {
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::SetStartOffset {
            seq: SeqId(1),
            offset: Tick(Q),
        }),
    );
    let d = insert(&d, 1, 1, vec![note(-1, 59, 1), note(0, 60, 1)]);
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(2), 0)]);
    assert_eq!(
        notes(&d),
        vec![(3.0, 59, 1.0), (4.0, 60, 1.0), (8.0, 60, 1.0)]
    );
}

#[test]
fn range_and_view_filter() {
    let cc = EventSpec::new(Tick(0), EventBody::Cc { index: 1, value: 1 });
    let d = insert(&base(), 0, 0, vec![note(0, 60, 1), note(2, 62, 1), cc]);
    let in_range: Vec<_> = flatten(
        &d,
        SeqId(0),
        Some((Tick(Q), Tick(3 * Q))),
        &ViewFilter::default(),
    )
    .unwrap()
    .collect();
    assert_eq!(in_range.len(), 1);
    let mut f = ViewFilter::default();
    f.hidden_kinds.insert(EventKind::Cc);
    assert_eq!(flatten(&d, SeqId(0), None, &f).unwrap().count(), 2);
    let mut f = ViewFilter::default();
    f.hidden_lanes.insert(LaneId(0));
    assert_eq!(flatten(&d, SeqId(0), None, &f).unwrap().count(), 0);
}

#[test]
fn three_levels_of_nesting_offset_correctly() {
    // seq2 (one note at beat 1) <- seq1 at beat 2 <- seq0 at beat 4.
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::New {
            name: "Inner".into(),
        }),
    );
    let d = insert(&d, 2, 2, vec![note(1, 64, 1)]);
    let inner = EventSpec::new(Tick(2 * Q), EventBody::reference(SeqId(2)));
    let d = insert(&d, 1, 1, vec![inner]);
    let d = insert(&d, 0, 0, vec![reference(4, None, LoopCount::Count(1), 0)]);
    let r = flat(&d);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].tick, Tick(7 * Q));
    assert_eq!(r[0].path.len(), 3);
}

#[test]
fn reference_starting_before_the_window_plays_its_inside_part() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1), note(2, 62, 1)]);
    // Starts one beat before bar 1: the note at its beat 0 is cut, beat 2 plays at beat 1.
    let d = insert(&d, 0, 0, vec![reference(-1, None, LoopCount::Count(1), 0)]);
    assert_eq!(notes(&d), vec![(1.0, 62, 1.0)]);
}

#[test]
fn lane_length_cuts_notes_and_references() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1), note(3, 63, 1)]);
    let d = insert(
        &d,
        0,
        0,
        vec![note(0, 50, 4), reference(1, None, LoopCount::Count(1), 0)],
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(2 * Q)),
            looped: false,
            locked: false,
        }),
    );
    // The long note is cut at the lane end; the referenced note at beat 4 does not play.
    assert_eq!(notes(&d), vec![(0.0, 50, 2.0), (1.0, 60, 1.0)]);
}

#[test]
fn looped_lane_passes_start_at_the_loop_point() {
    let d = insert(&base(), 0, 0, vec![note(-1, 40, 1), note(0, 41, 1)]);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(2 * Q)),
            looped: true,
            locked: false,
        }),
    );
    // Negative ticks never play in a looped lane; the beat-0 note repeats every 2 beats.
    assert_eq!(notes(&d), vec![(0.0, 41, 1.0), (2.0, 41, 1.0)]);
}
