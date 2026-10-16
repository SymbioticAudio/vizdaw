use core_model::ModelError;
use core_model::command::{
    AudioCmd, Command, Context, EventCmd, LaneCmd, ReferenceCmd, RoutingCmd, SequenceCmd, apply,
    apply_in,
};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventKind, EventSpec, Fade, LoopCount};
use core_model::ids::{AnyId, Counters, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId};
use core_model::lane::QuantizeSpec;
use core_model::query::{flatten, seq_length, use_count, users_of};
use core_model::rules::{find_cycle, validate};
use core_model::selection::{Selection, ViewFilter};
use core_model::sequence::SeqLength;
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn rc(c: ReferenceCmd) -> Command {
    Command::Reference(c)
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
    .unwrap()
}

fn note(beat: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(beat * Q), EventBody::note(pitch, 100, Tick(Q)))
}

fn insert_ref(
    d: &Document,
    seq: u64,
    lane: u64,
    beat: i64,
    target: u64,
) -> core_model::Result<Document> {
    run(
        d,
        rc(ReferenceCmd::Insert {
            seq: SeqId(seq),
            lane: LaneId(lane),
            tick: Tick(beat * Q),
            target: SeqId(target),
            length: None,
            loop_count: LoopCount::Count(1),
            transpose: 0,
        }),
    )
}

/// dest0, dest1 (plugins). Sequences 0..3, each with one lane (lane i);
/// lane0 -> dest0, lane1 -> dest1. seq1 has notes 60 @0 and 62 @1.
fn base() -> Document {
    let dest = |i: u64| Destination {
        id: DestId(i),
        name: format!("D{i}"),
        kind: DestKind::PluginInstrument {
            plugin_ref: "p".into(),
        },
        default_patch: None,
    };
    let mut d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![dest(0), dest(1)],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        counters: Counters {
            dest: 2,
            ..Counters::default()
        },
    })
    .unwrap();
    for name in ["Song", "Verse", "Fill"] {
        d = run(
            &d,
            Command::Sequence(SequenceCmd::New { name: name.into() }),
        )
        .unwrap();
    }
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(1),
            lane: LaneId(1),
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    insert(&d, 1, 1, vec![note(0, 60), note(1, 62)])
}

/// Comparable view of a flattened sequence: (tick, dest, group/channel, body).
fn played(d: &Document, seq: u64) -> Vec<String> {
    let mut v: Vec<String> = flatten(d, SeqId(seq), None, &ViewFilter::default())
        .unwrap()
        .map(|r| {
            format!(
                "{:?}|{:?}|{:?}|{:?}",
                r.tick, r.dest, r.group_channel, r.body
            )
        })
        .collect();
    v.sort();
    v
}

// Review focus #1: re-pointing must not be able to create a cycle.
#[test]
fn re_point_refuses_cycles() {
    // seq3 "x" is a harmless target.
    let d = run(
        &base(),
        Command::Sequence(SequenceCmd::New { name: "x".into() }),
    )
    .unwrap();
    let d = insert_ref(&d, 0, 0, 0, 1).unwrap(); // ev2: song -> verse
    let d = insert_ref(&d, 1, 1, 4, 2).unwrap(); // ev3: verse -> fill
    let d = insert_ref(&d, 2, 2, 0, 3).unwrap(); // ev4: fill -> x
    assert_eq!(find_cycle(&d), None);
    // fill -> song would close song -> verse -> fill -> song.
    let err = run(
        &d,
        rc(ReferenceCmd::RePoint {
            event: EventId(4),
            target: SeqId(0),
        }),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ModelError::Cycle {
            path: vec![SeqId(2), SeqId(0), SeqId(1), SeqId(2)]
        }
    );
    // fill -> fill is a self-cycle.
    let err = run(
        &d,
        rc(ReferenceCmd::RePoint {
            event: EventId(4),
            target: SeqId(2),
        }),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ModelError::Cycle {
            path: vec![SeqId(2), SeqId(2)]
        }
    );
    // Inserting fill -> verse directly is refused the same way.
    assert!(matches!(
        insert_ref(&d, 2, 2, 0, 1),
        Err(ModelError::Cycle { .. })
    ));
    // A harmless re-point succeeds; re-pointing a note is invalid.
    let ok = run(
        &d,
        rc(ReferenceCmd::RePoint {
            event: EventId(2),
            target: SeqId(3),
        }),
    )
    .unwrap();
    assert_eq!(users_of(&ok, SeqId(3)), vec![EventId(2), EventId(4)]);
    assert!(matches!(
        run(
            &d,
            rc(ReferenceCmd::RePoint {
                event: EventId(0),
                target: SeqId(3)
            })
        ),
        Err(ModelError::Invalid { .. })
    ));
}

#[test]
fn private_copy_versus_flatten() {
    let d = insert_ref(&base(), 0, 0, 4, 1).unwrap(); // ev2
    let before = played(&d, 0);

    let a = apply(&d, &rc(ReferenceCmd::MakePrivateCopy { event: EventId(2) })).unwrap();
    // The copy and its lanes are reported.
    let copy_lane = a.doc.sequence(SeqId(3)).unwrap().lanes[0].id;
    assert_eq!(
        a.report.created,
        vec![AnyId::Seq(SeqId(3)), AnyId::Lane(copy_lane)]
    );
    assert_eq!(a.doc.sequence(SeqId(3)).unwrap().name, "Verse (copy)");
    assert_eq!(use_count(&a.doc, SeqId(1)), 0);
    assert_eq!(users_of(&a.doc, SeqId(3)), vec![EventId(2)]);
    assert_eq!(played(&a.doc, 0), before);
    assert!(validate(&a.doc).is_empty());

    let f = apply(&d, &rc(ReferenceCmd::Flatten { event: EventId(2) })).unwrap();
    assert_eq!(use_count(&f.doc, SeqId(1)), 0);
    assert!(f.doc.event(EventId(2)).is_err());
    assert_eq!(f.report.removed, vec![AnyId::Event(EventId(2))]);
    assert_eq!(played(&f.doc, 0), before);
    // Inlined notes carry the inner lane's destination explicitly.
    let (e, loc) = f.doc.event(EventId(3)).unwrap();
    assert_eq!(
        (e.tick, e.dest, loc.lane),
        (Tick(4 * Q), Some(DestId(1)), LaneId(0))
    );
    assert!(validate(&f.doc).is_empty());
}

#[test]
fn flatten_applies_length_loop_and_transpose() {
    let d = run(
        &base(),
        rc(ReferenceCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            target: SeqId(1),
            length: Some(Tick(Q)),
            loop_count: LoopCount::Count(3),
            transpose: 2,
        }),
    )
    .unwrap();
    let before = played(&d, 0);
    let f = run(&d, rc(ReferenceCmd::Flatten { event: EventId(2) })).unwrap();
    assert_eq!(played(&f, 0), before);
    let pitches: Vec<(Tick, EventBody)> = f
        .lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| (e.tick, e.body.clone()))
        .collect();
    assert_eq!(
        pitches,
        vec![
            (Tick(0), EventBody::note(62, 100, Tick(Q))),
            (Tick(Q), EventBody::note(62, 100, Tick(Q))),
            (Tick(2 * Q), EventBody::note(62, 100, Tick(Q))),
        ]
    );
}

#[test]
fn set_params_validates() {
    let d = insert_ref(&base(), 0, 0, 0, 1).unwrap();
    let d2 = run(
        &d,
        rc(ReferenceCmd::SetParams {
            event: EventId(2),
            length: Some(Tick(Q * 8)),
            loop_count: LoopCount::Fill,
            transpose: -5,
        }),
    )
    .unwrap();
    assert_eq!(seq_length(&d2, SeqId(0)).unwrap(), Tick(Q * 8));
    assert!(
        run(
            &d,
            rc(ReferenceCmd::SetParams {
                event: EventId(2),
                length: None,
                loop_count: LoopCount::Count(0),
                transpose: 0
            })
        )
        .is_err()
    );
    assert!(matches!(
        run(
            &d,
            rc(ReferenceCmd::SetParams {
                event: EventId(0),
                length: None,
                loop_count: LoopCount::Count(1),
                transpose: 0
            })
        ),
        Err(ModelError::Invalid { .. })
    ));
}

fn selection(range: (i64, i64), lanes: &[(u64, u64)]) -> Selection {
    Selection {
        time_range: Some((Tick(range.0), Tick(range.1))),
        lanes: lanes.iter().map(|(s, l)| (SeqId(*s), LaneId(*l))).collect(),
        events: Default::default(),
    }
}

/// Song with two lanes (lane0, lane3) holding notes at beats 1, 5 and 9.
fn two_lane_song() -> Document {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap();
    let d = insert(&d, 0, 0, vec![note(1, 40), note(5, 41), note(9, 42)]);
    insert(&d, 0, 3, vec![note(1, 50), note(5, 51), note(9, 52)])
}

#[test]
fn make_reference_from_selection_multi_lane_rounded() {
    let d = two_lane_song();
    let before = played(&d, 0);
    let sel = selection((5 * Q, 6 * Q), &[(0, 0), (0, 3)]);
    let filter = ViewFilter::default();
    let a = apply_in(
        &d,
        Context {
            selection: &sel,
            filter: &filter,
        },
        &rc(ReferenceCmd::FromSelection {
            seq: SeqId(0),
            per_lane: false,
            round_to_bars: true,
            loop_count: LoopCount::Count(1),
        }),
    )
    .unwrap();
    let new = a.doc.sequence(SeqId(3)).unwrap();
    assert_eq!(new.lanes.len(), 2);
    assert_eq!(new.length, SeqLength::Locked(Tick(4 * Q)));
    assert_eq!(new.lanes[0].midi_dest, Some(DestId(0)));
    // The moved notes keep their ids and sit at beat 1 of the new sequence.
    let (e, loc) = a.doc.event(EventId(3)).unwrap();
    assert_eq!((e.tick, loc.seq), (Tick(Q), SeqId(3)));
    assert_eq!(users_of(&a.doc, SeqId(3)).len(), 1);
    assert_eq!(played(&a.doc, 0), before);
    assert!(validate(&a.doc).is_empty(), "{:?}", validate(&a.doc));
}

#[test]
fn make_reference_from_selection_per_lane_with_loop() {
    let d = two_lane_song();
    let sel = selection((4 * Q, 8 * Q), &[(0, 0), (0, 3)]);
    let filter = ViewFilter::default();
    let a = apply_in(
        &d,
        Context {
            selection: &sel,
            filter: &filter,
        },
        &rc(ReferenceCmd::FromSelection {
            seq: SeqId(0),
            per_lane: true,
            round_to_bars: false,
            loop_count: LoopCount::Count(2),
        }),
    )
    .unwrap();
    assert_eq!(a.doc.sequences().len(), 5);
    assert_eq!(users_of(&a.doc, SeqId(3)).len(), 1);
    assert_eq!(users_of(&a.doc, SeqId(4)).len(), 1);
    let refs = users_of(&a.doc, SeqId(4));
    let (r, loc) = a.doc.event(refs[0]).unwrap();
    assert_eq!(loc.lane, LaneId(3));
    assert!(matches!(
        r.body,
        EventBody::Reference {
            loop_count: LoopCount::Count(2),
            ..
        }
    ));
}

#[test]
fn make_reference_from_selection_needs_range_and_lanes() {
    let d = two_lane_song();
    let filter = ViewFilter::default();
    let cmd = rc(ReferenceCmd::FromSelection {
        seq: SeqId(0),
        per_lane: false,
        round_to_bars: false,
        loop_count: LoopCount::Count(1),
    });
    let empty = Selection::default();
    assert!(
        apply_in(
            &d,
            Context {
                selection: &empty,
                filter: &filter
            },
            &cmd
        )
        .is_err()
    );
    let no_lanes = selection((0, Q), &[]);
    assert!(
        apply_in(
            &d,
            Context {
                selection: &no_lanes,
                filter: &filter
            },
            &cmd
        )
        .is_err()
    );
}

#[test]
fn flattening_a_span_longer_than_its_events_keeps_the_length() {
    // Song references the empty "Fill" (one bar) twice: two bars long.
    let d = run(
        &base(),
        rc(ReferenceCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            target: SeqId(2),
            length: None,
            loop_count: LoopCount::Count(2),
            transpose: 0,
        }),
    )
    .unwrap();
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(8 * Q));
    let a = apply(&d, &rc(ReferenceCmd::Flatten { event: EventId(2) })).unwrap();
    assert_eq!(
        a.doc.sequence(SeqId(0)).unwrap().length,
        SeqLength::Locked(Tick(8 * Q))
    );
    assert_eq!(a.report.warnings.len(), 1);
}

#[test]
fn flattening_unresolved_events_warns() {
    // Verse's lane has a destination; Fill's does not.
    let d = insert(&base(), 2, 2, vec![note(0, 70)]);
    let d = insert_ref(&d, 0, 0, 0, 2).unwrap();
    let a = apply(&d, &rc(ReferenceCmd::Flatten { event: EventId(3) })).unwrap();
    assert!(
        a.report
            .warnings
            .iter()
            .any(|w| w.contains("no destination"))
    );
}

fn flags(d: &Document, seq: u64, lane: u64, mute: Option<bool>, solo: Option<bool>) -> Document {
    run(
        d,
        Command::Lane(LaneCmd::SetFlags {
            seq: SeqId(seq),
            lane: LaneId(lane),
            mute,
            solo,
            arm: None,
        }),
    )
    .unwrap()
}

fn lane_events(d: &Document, seq: u64, lane: u64) -> Vec<(Tick, bool, EventBody)> {
    d.lane(SeqId(seq), LaneId(lane))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| (e.tick, e.mute, e.body.clone()))
        .collect()
}

#[test]
fn flattening_a_muted_reference_inlines_muted_events() {
    let mut r = EventSpec::new(Tick(0), EventBody::reference(SeqId(1)));
    r.mute = true;
    let d = insert(&base(), 0, 0, vec![r]); // ev2
    let before = played(&d, 0);
    assert!(before.is_empty());
    let f = run(&d, rc(ReferenceCmd::Flatten { event: EventId(2) })).unwrap();
    assert_eq!(played(&f, 0), before);
    let ev = lane_events(&f, 0, 0);
    assert_eq!(ev.len(), 2);
    assert!(ev.iter().all(|(_, mute, _)| *mute));
}

#[test]
fn flattening_a_two_level_nested_reference() {
    // Fill (dest0, note 70 @0) <- Verse at beat 2 <- Song at beat 4.
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(2),
            lane: LaneId(2),
            dest: Some(DestId(0)),
        }),
    )
    .unwrap();
    let d = insert(&d, 2, 2, vec![note(0, 70)]); // ev2
    let d = insert_ref(&d, 1, 1, 2, 2).unwrap(); // ev3
    let d = insert_ref(&d, 0, 0, 4, 1).unwrap(); // ev4
    let before = played(&d, 0);
    let f = run(&d, rc(ReferenceCmd::Flatten { event: EventId(4) })).unwrap();
    assert_eq!(played(&f, 0), before);
    let ev: Vec<(Tick, u8)> = lane_events(&f, 0, 0)
        .into_iter()
        .filter_map(|(t, _, b)| match b {
            EventBody::Note { pitch, .. } => Some((t, pitch)),
            _ => None,
        })
        .collect();
    assert_eq!(
        ev,
        vec![(Tick(4 * Q), 60), (Tick(5 * Q), 62), (Tick(6 * Q), 70)]
    );
    assert_eq!(use_count(&f, SeqId(1)), 0);
    assert_eq!(use_count(&f, SeqId(2)), 1); // only Verse's own use remains
    assert!(validate(&f).is_empty());
}

#[test]
fn flatten_warns_about_lost_audio_output_and_owner_quantize() {
    let mut d = base();
    for c in [
        RoutingCmd::AddOutput {
            name: "Main".into(),
            reference: "bus:0".into(),
        },
        RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 48_000,
            content_hash: None,
        },
    ] {
        d = run(&d, Command::Routing(c)).unwrap();
    }
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetOutput {
            seq: SeqId(1),
            lane: LaneId(1),
            output: Some(OutputPathId(0)),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(1),
            lane: LaneId(1),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 0,
            length: 24_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetPlayQuantize {
            seq: SeqId(0),
            lane: LaneId(0),
            quantize: Some(QuantizeSpec {
                grid: Tick(Q),
                strength: 100,
            }),
        }),
    )
    .unwrap();
    let d = insert_ref(&d, 0, 0, 0, 1).unwrap();
    let r = users_of(&d, SeqId(1))[0];
    let a = apply(&d, &rc(ReferenceCmd::Flatten { event: r })).unwrap();
    let w = a.report.warnings.join("\n");
    assert!(w.contains("audio output"), "{w}");
    assert!(w.contains("play quantize"), "{w}");
}

#[test]
fn make_reference_from_selection_keeps_lane_properties() {
    // lane0 muted, lane3 shifted by half a beat and a drum lane; a CC on
    // lane3 is hidden by the view and must stay behind.
    let d = two_lane_song();
    let d = flags(&d, 0, 0, Some(true), None);
    let d = run(
        &d,
        Command::Lane(LaneCmd::SetPlayShift {
            seq: SeqId(0),
            lane: LaneId(3),
            shift: Tick(Q / 2),
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Drum(core_model::command::DrumCmd::MarkDrumLane {
            seq: SeqId(0),
            lane: LaneId(3),
            drum: true,
        }),
    )
    .unwrap();
    let cc = EventSpec::new(Tick(5 * Q), EventBody::Cc { index: 1, value: 9 });
    let d = insert(&d, 0, 3, vec![cc]);
    let cc_id = EventId(8);
    assert!(matches!(
        d.event(cc_id).unwrap().0.body,
        EventBody::Cc { .. }
    ));
    let before = played(&d, 0);
    let sel = selection((4 * Q, 8 * Q), &[(0, 0), (0, 3)]);
    let mut filter = ViewFilter::default();
    filter.hidden_kinds.insert(EventKind::Cc);
    let a = apply_in(
        &d,
        Context {
            selection: &sel,
            filter: &filter,
        },
        &rc(ReferenceCmd::FromSelection {
            seq: SeqId(0),
            per_lane: false,
            round_to_bars: false,
            loop_count: LoopCount::Count(1),
        }),
    )
    .unwrap();
    assert_eq!(played(&a.doc, 0), before);
    let new = a.doc.sequence(SeqId(3)).unwrap();
    assert!(new.lanes[0].mute);
    assert!(!new.lanes[1].mute);
    assert_eq!(new.lanes[1].play_shift, Tick(Q / 2));
    assert!(new.lanes[1].drum.is_some());
    // The hidden CC stayed on the original lane.
    assert_eq!(a.doc.event(cc_id).unwrap().1.lane, LaneId(3));
    // The report lists the new lanes and takes and the moved events.
    for l in &new.lanes {
        assert!(a.report.created.contains(&AnyId::Lane(l.id)));
        assert!(a.report.created.contains(&AnyId::Take(l.active_take)));
    }
    // Notes at beat 5: ev3 on lane0, ev6 on lane3.
    assert!(a.report.moved.contains(&AnyId::Event(EventId(3))));
    assert!(a.report.moved.contains(&AnyId::Event(EventId(6))));
    assert!(!a.report.moved.contains(&AnyId::Event(cc_id)));
    assert!(validate(&a.doc).is_empty(), "{:?}", validate(&a.doc));
}
