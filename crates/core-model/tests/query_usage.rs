use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{
    DestKind, Destination, Document, DocumentRepr, MediaItem, MediaOrigin, Settings,
};
use core_model::event::{EventBody, EventSpec, Fade, LoopCount};
use core_model::ids::{Counters, DestId, EventId, LaneId, MediaId, SeqId};
use core_model::query::{
    Routing, drum_view, media_in_use, reference_span, resolve_event, seq_length, unresolved_events,
    unused_sequences, use_count, use_counts, users_of,
};
use core_model::sequence::SeqLength;
use core_model::time::{PPQ, Tick};

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

/// dest0 = MIDI endpoint group 1 channel 9, dest1 = plugin; media0 = 1 s at
/// 48 kHz; seq0 (lane0, dest0) and seq1 (lane1, no dest).
fn base() -> Document {
    let d = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![
            Destination {
                id: DestId(0),
                name: "Ext".into(),
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: "e".into(),
                    group: 1,
                    channel: 9,
                },
                default_patch: None,
                return_latency_ms: 0.0,
            },
            Destination {
                id: DestId(1),
                name: "Plug".into(),
                kind: DestKind::plugin("p"),
                default_patch: None,
                return_latency_ms: 0.0,
            },
        ],
        outputs: vec![],
        media: vec![MediaItem {
            id: MediaId(0),
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 48_000,
            content_hash: None,
            name: "a".into(),
            origin: MediaOrigin::default(),
        }],
        sequences: vec![],
        mixer: None,
        counters: Counters {
            dest: 2,
            plugin: 1,
            media: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    let d = run(&d, Command::Sequence(SequenceCmd::New { name: "A".into() }));
    let d = run(&d, Command::Sequence(SequenceCmd::New { name: "B".into() }));
    run(
        &d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(DestId(0)),
        }),
    )
}

fn note(t: i64, dur: i64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::note(60, 100, Tick(dur)))
}

fn reference(t: i64, length: Option<i64>, loop_count: LoopCount) -> EventSpec {
    EventSpec::new(
        Tick(t),
        EventBody::Reference {
            seq: SeqId(1),
            length: length.map(Tick),
            loop_count,
            transpose: 0,
        },
    )
}

#[test]
fn empty_sequence_is_one_bar() {
    assert_eq!(seq_length(&base(), SeqId(0)).unwrap(), Tick(PPQ * 4));
}

#[test]
fn derived_length_rounds_up_to_a_barline() {
    let d = insert(&base(), 0, 0, vec![note(PPQ * 4, PPQ / 2)]);
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 8));
    let d = run(
        &d,
        Command::Sequence(SequenceCmd::SetLength {
            seq: SeqId(0),
            length: SeqLength::Locked(Tick(PPQ * 3)),
        }),
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 3));
}

#[test]
fn lane_length_counts_even_when_empty() {
    let d = run(
        &base(),
        Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(PPQ * 9)),
            looped: false,
            locked: false,
        }),
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 12));
}

#[test]
fn reference_spans() {
    let bar = Tick(PPQ * 4);
    assert_eq!(
        reference_span(bar, None, LoopCount::Count(3)),
        Tick(PPQ * 12)
    );
    assert_eq!(
        reference_span(bar, Some(Tick(PPQ * 2)), LoopCount::Count(2)),
        bar
    );
    assert_eq!(
        reference_span(bar, Some(Tick(PPQ * 6)), LoopCount::Fill),
        Tick(PPQ * 6)
    );
    assert_eq!(reference_span(bar, None, LoopCount::Fill), bar);
    // Length longer than the sequence and shorter than the sequence.
    let d = insert(
        &base(),
        0,
        0,
        vec![reference(0, Some(PPQ * 10), LoopCount::Count(1))],
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 12));
    let d = insert(
        &base(),
        0,
        0,
        vec![reference(0, Some(PPQ), LoopCount::Count(2))],
    );
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 4));
}

#[test]
fn audio_region_extent_uses_the_tempo_map() {
    let region = EventSpec::new(
        Tick(PPQ * 3),
        EventBody::AudioRegion {
            media: MediaId(0),
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        },
    );
    // 1 s at 120 BPM = 2 beats, so it ends at beat 5 -> rounded to 2 bars.
    let d = insert(&base(), 0, 0, vec![region]);
    assert_eq!(seq_length(&d, SeqId(0)).unwrap(), Tick(PPQ * 8));
}

#[test]
fn routing_resolution_with_and_without_overrides() {
    let mut gc = note(0, 1);
    gc.group_channel = Some((2, 3));
    let mut plug = note(0, 1);
    plug.dest = Some(DestId(1));
    let mut gc_plug = note(0, 1);
    gc_plug.dest = Some(DestId(1));
    gc_plug.group_channel = Some((4, 5));
    let d = insert(&base(), 0, 0, vec![note(0, 1), gc, plug, gc_plug]);
    let d = insert(&d, 1, 1, vec![note(0, 1)]);
    let r = |id| resolve_event(&d, EventId(id)).unwrap();
    assert_eq!(
        r(0),
        Routing {
            dest: Some(DestId(0)),
            group_channel: Some((1, 9))
        }
    );
    assert_eq!(
        r(1),
        Routing {
            dest: Some(DestId(0)),
            group_channel: Some((2, 3))
        }
    );
    assert_eq!(
        r(2),
        Routing {
            dest: Some(DestId(1)),
            group_channel: Some((0, 0))
        }
    );
    assert_eq!(
        r(3),
        Routing {
            dest: Some(DestId(1)),
            group_channel: Some((4, 5))
        }
    );
    assert_eq!(
        r(4),
        Routing {
            dest: None,
            group_channel: None
        }
    );
    assert_eq!(unresolved_events(&d), vec![EventId(4)]);
}

#[test]
fn users_counts_and_unused() {
    let d = insert(
        &base(),
        0,
        0,
        vec![
            reference(0, None, LoopCount::Count(1)),
            reference(PPQ * 4, None, LoopCount::Count(1)),
        ],
    );
    assert_eq!(users_of(&d, SeqId(1)), vec![EventId(0), EventId(1)]);
    assert_eq!(use_count(&d, SeqId(1)), 2);
    assert_eq!(use_counts(&d).get(&SeqId(0)), Some(&0));
    assert_eq!(unused_sequences(&d), vec![SeqId(0)]);
}

#[test]
fn media_in_use_lists_regions() {
    let region = EventSpec::new(
        Tick(0),
        EventBody::AudioRegion {
            media: MediaId(0),
            source_offset: 0,
            length: 10,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        },
    );
    let d = insert(&base(), 1, 1, vec![region]);
    assert_eq!(media_in_use(&d).get(&MediaId(0)), Some(&vec![EventId(0)]));
}

#[test]
fn drum_view_groups_notes_by_row() {
    let d = insert(&base(), 0, 0, vec![note(0, 1), note(PPQ, 1)]);
    let d = run(
        &d,
        Command::Drum(DrumCmd::MarkDrumLane {
            seq: SeqId(0),
            lane: LaneId(0),
            drum: true,
        }),
    );
    let view = drum_view(&d, SeqId(0), LaneId(0)).unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].0.note, 60);
    assert_eq!(view[0].1, vec![EventId(0), EventId(1)]);
}

#[test]
fn text_events_are_not_midi_for_routing() {
    use core_model::event::TextKind;
    use core_model::query::flatten;
    use core_model::selection::ViewFilter;
    let text = EventSpec::new(
        Tick(0),
        EventBody::Text {
            kind: TextKind::Lyric,
            text: "la".into(),
        },
    );
    // seq1's lane has no destination: the text is not "unresolved".
    let d = insert(&base(), 1, 1, vec![text.clone()]);
    assert!(unresolved_events(&d).is_empty());
    assert_eq!(
        resolve_event(&d, EventId(0)).unwrap(),
        Routing {
            dest: None,
            group_channel: None
        }
    );
    // On a lane with a destination it still flattens, but unrouted.
    let d = insert(&d, 0, 0, vec![text]);
    let out: Vec<_> = flatten(&d, SeqId(0), None, &ViewFilter::default())
        .unwrap()
        .collect();
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0].body, EventBody::Text { .. }));
    assert_eq!(
        (out[0].dest, out[0].group_channel, out[0].audio_out),
        (None, None, None)
    );
}
