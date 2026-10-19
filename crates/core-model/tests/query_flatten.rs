use core_model::command::{
    AudioCmd, Command, DrumCmd, EventCmd, LaneCmd, MediaCmd, SequenceCmd, apply,
};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventKind, EventSpec, Fade, LoopCount};
use core_model::ids::{Counters, DestId, EventId, LaneId, MediaId, SeqId};
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
                kind: DestKind::plugin("p"),
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
        mixer: None,
        counters: Counters {
            dest: 2,
            plugin: 1,
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
fn reference_transpose_skips_drum_lanes() {
    // Sub gets a second lane (lane2) marked as a drum lane (ruling O21).
    let d = run(
        &base(),
        Command::Lane(LaneCmd::Add {
            seq: SeqId(1),
            name: "Drums".into(),
            index: None,
        }),
    );
    let d = run(
        &d,
        Command::Drum(DrumCmd::MarkDrumLane {
            seq: SeqId(1),
            lane: LaneId(2),
            drum: true,
        }),
    );
    let d = insert(&d, 1, 1, vec![note(0, 60, 1)]);
    let d = insert(
        &d,
        1,
        2,
        vec![
            note(1, 36, 1),
            EventSpec::new(
                Tick(2 * Q),
                EventBody::PolyPressure {
                    pitch: 36,
                    value: 1,
                },
            ),
        ],
    );
    let d = insert(&d, 0, 0, vec![reference(0, None, LoopCount::Count(1), 7)]);
    assert_eq!(notes(&d), vec![(0.0, 67, 1.0), (1.0, 36, 1.0)]);
    let pp: Vec<u8> = flat(&d)
        .into_iter()
        .filter_map(|r| match r.body {
            EventBody::PolyPressure { pitch, .. } => Some(pitch),
            _ => None,
        })
        .collect();
    assert_eq!(pp, vec![36]);
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
    // Song has no pickup (start_offset 0), so the beat -1 note is outside the
    // playing window; the beat-0 note repeats every 2 beats.
    assert_eq!(notes(&d), vec![(0.0, 41, 1.0), (2.0, 41, 1.0)]);
}

fn set_length(d: &Document, seq: u64, lane: u64, beats: i64, looped: bool) -> Document {
    run(
        d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(seq),
            lane: LaneId(lane),
            length: Some(Tick(beats * Q)),
            looped,
            locked: false,
        }),
    )
}

#[test]
fn looped_lane_pickup_plays_only_before_the_first_pass() {
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::SetStartOffset {
            seq: SeqId(0),
            offset: Tick(Q),
        }),
    );
    let d = insert(&d, 0, 0, vec![note(-1, 40, 1), note(0, 41, 1)]);
    let d = set_length(&d, 0, 0, 2, true);
    assert_eq!(
        notes(&d),
        vec![(-1.0, 40, 1.0), (0.0, 41, 1.0), (2.0, 41, 1.0)]
    );
}

#[test]
fn negative_play_shift_reaches_the_last_pass() {
    // Note at beat 0.5 in a 2-beat loop shifted one beat early: passes at
    // -0.5 (cut by the window), 1.5 and 3.5 (pass base 4 shifted to 3).
    let half = EventSpec::new(Tick(Q / 2), EventBody::note(60, 100, Tick(Q / 4)));
    let d = insert(&base(), 0, 0, vec![half]);
    let d = set_length(&d, 0, 0, 2, true);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetPlayShift {
            seq: SeqId(0),
            lane: LaneId(0),
            shift: Tick(-Q),
        }),
    );
    let beats: Vec<f64> = notes(&d).into_iter().map(|n| n.0).collect();
    assert_eq!(beats, vec![1.5, 3.5]);
}

#[test]
fn huge_fill_length_does_not_overflow() {
    let d = insert(&base(), 1, 1, vec![note(0, 60, 1)]);
    let mut r = reference(0, None, LoopCount::Fill, 0);
    if let EventBody::Reference { length, .. } = &mut r.body {
        // The longest a document may hold (the model refuses beyond it).
        *length = Some(Tick(core_model::rules::MAX_TICKS - 1));
    }
    let d = insert(&d, 0, 0, vec![r]);
    let n = flatten(
        &d,
        SeqId(0),
        Some((Tick(0), Tick(9 * Q))),
        &ViewFilter::default(),
    )
    .unwrap()
    .count();
    assert_eq!(n, 3);
}

/// Adds media0 (48 kHz, 4 beats at 120 BPM) and a region of all of it at
/// `beat` on `seq`/`lane`.
fn with_region(d: &Document, seq: u64, lane: u64, beat: i64) -> Document {
    let d = run(
        d,
        Command::Media(MediaCmd::Add {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
            name: None,
            origin: None,
        }),
    );
    run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            tick: Tick(beat * Q),
            media: MediaId(0),
            source_offset: 0,
            length: 96_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        }),
    )
}

fn region_lengths(d: &Document) -> Vec<u64> {
    flat(d)
        .into_iter()
        .filter_map(|r| match r.body {
            EventBody::AudioRegion { length, .. } => Some(length),
            _ => None,
        })
        .collect()
}

#[test]
fn audio_region_is_clipped_at_the_lane_length() {
    let d = with_region(&base(), 0, 0, 0);
    let d = set_length(&d, 0, 0, 2, false);
    // Two beats at 120 BPM = 1 s = 48 000 frames.
    assert_eq!(region_lengths(&d), vec![48_000]);
}

#[test]
fn audio_region_is_clipped_at_a_reference_length_using_the_playing_tempo() {
    let d = with_region(&base(), 1, 1, 0);
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::SetTempo {
            seq: SeqId(1),
            tempo: TempoMap::constant(Bpm::whole(60)).unwrap(),
        }),
    );
    let d = insert(
        &d,
        0,
        0,
        vec![reference(0, Some(1), LoopCount::Count(1), 0)],
    );
    // One beat at the song's 120 BPM = 0.5 s = 24 000 frames.
    assert_eq!(region_lengths(&d), vec![24_000]);
}

#[test]
fn short_loop_over_many_out_of_range_events_emits_only_in_range_passes() {
    use core_model::sequence::SeqLength;
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::SetLength {
            seq: SeqId(0),
            length: SeqLength::Locked(Tick(256 * Q)),
        }),
    );
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(Q)),
            looped: true,
            locked: false,
        }),
    );
    let mut specs: Vec<EventSpec> = (0..4)
        .map(|i| {
            EventSpec::new(
                Tick(i * Q / 4),
                EventBody::note(60 + i as u8, 100, Tick(Q / 8)),
            )
        })
        .collect();
    // 10 000 events beyond the 1-beat loop limit never play.
    specs.extend(
        (0..10_000).map(|i| EventSpec::new(Tick(Q + 1 + i), EventBody::note(100, 100, Tick(1)))),
    );
    let d = insert(&d, 0, 0, specs);
    let out: Vec<_> = flatten(&d, SeqId(0), None, &ViewFilter::default())
        .unwrap()
        .collect();
    assert_eq!(out.len(), 4 * 256);
    assert!(
        out.iter()
            .all(|e| !matches!(e.body, EventBody::Note { pitch: 100, .. }))
    );
}

#[test]
fn positions_and_lengths_beyond_the_limit_are_refused() {
    use core_model::error::ModelError;
    use core_model::rules::MAX_TICKS;
    let d = base();
    let far = EventSpec::new(Tick(MAX_TICKS + 1), EventBody::note(60, 100, Tick(Q)));
    let long = EventSpec::new(Tick(0), EventBody::note(60, 100, Tick(MAX_TICKS + 1)));
    let before = EventSpec::new(Tick(-MAX_TICKS - 1), EventBody::note(60, 100, Tick(Q)));
    for (spec, want) in [(far, "tick"), (long, "duration"), (before, "tick")] {
        let r = apply(
            &d,
            &Command::Event(EventCmd::Insert {
                seq: SeqId(1),
                lane: LaneId(1),
                events: vec![spec],
            }),
        );
        match r {
            Err(ModelError::OutOfRange { field, .. }) => assert_eq!(field, want),
            other => panic!(
                "accepted a tick beyond ±2^40 (wanted {want}): {:?}",
                other.map(|_| ())
            ),
        }
    }
    let mut r = reference(
        0,
        None,
        LoopCount::Count(core_model::rules::MAX_LOOPS + 1),
        0,
    );
    r.tick = Tick(0);
    let refused = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![r],
        }),
    );
    match refused {
        Err(ModelError::OutOfRange { field, .. }) => assert_eq!(field, "reference.loop_count"),
        other => panic!("accepted 2^20 + 1 loops: {:?}", other.map(|_| ())),
    }
}
