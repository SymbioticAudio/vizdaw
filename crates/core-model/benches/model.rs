//! Benchmarks for the spec §8 targets:
//! one edit on a 100k-event document < 1 ms; flattening a 64-bar song with
//! three levels of references < 5 ms; loading a 100k-event project < 200 ms.

use core_model::command::{Command, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::file::{from_json, to_json};
use core_model::ids::{Counters, EventId, LaneId, SeqId, TakeId};
use core_model::lane::Lane;
use core_model::query::flatten;
use core_model::selection::ViewFilter;
use core_model::sequence::{SeqLength, Sequence};
use core_model::time::{PPQ, Tick};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use im::Vector;

/// One sequence with 10 lanes of 10 000 sixteenth notes each.
fn big_doc() -> Document {
    let mut lanes = Vector::new();
    let mut next = 0u64;
    for l in 0..10u64 {
        let mut lane = Lane::new(LaneId(l), TakeId(l), format!("L{l}"));
        let take = lane.takes.get_mut(0).unwrap();
        for i in 0..10_000i64 {
            let ev = EventSpec::new(
                Tick(i * PPQ / 4),
                EventBody::note((i % 88) as u8 + 21, 30_000, Tick(PPQ / 8)),
            )
            .with_id(EventId(next));
            next += 1;
            take.events.insert(ev.key(), ev);
        }
        lanes.push_back(lane);
    }
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![Sequence::new(SeqId(0), "Big", lanes)],
        counters: Counters {
            seq: 1,
            lane: 10,
            take: 10,
            event: next,
            ..Counters::default()
        },
    })
    .unwrap()
}

/// Song (64 bars) -> 8 x Verse (8 bars) -> 4 x Riff (2 bars, 2 lanes of
/// 16th notes); each level also has a lane of its own notes.
fn nested_song() -> Document {
    let mut next = 0u64;
    let mut ev = |t: i64, body: EventBody| {
        let e = EventSpec::new(Tick(t), body).with_id(EventId(next));
        next += 1;
        e
    };
    let bar = PPQ * 4;
    let notes_lane =
        |id: u64, bars: i64, ev: &mut dyn FnMut(i64, EventBody) -> core_model::event::Event| {
            let mut lane = Lane::new(LaneId(id), TakeId(id), "notes");
            for i in 0..bars * 16 {
                let e = ev(
                    i * PPQ / 4,
                    EventBody::note(60 + (i % 12) as u8, 20_000, Tick(PPQ / 8)),
                );
                lane.takes.get_mut(0).unwrap().events.insert(e.key(), e);
            }
            lane
        };
    let riff = Sequence::new(
        SeqId(2),
        "Riff",
        Vector::from(vec![notes_lane(0, 2, &mut ev), notes_lane(1, 2, &mut ev)]),
    );
    let mut verse_refs = Lane::new(LaneId(2), TakeId(2), "refs");
    for i in 0..4 {
        let e = ev(i * 2 * bar, EventBody::reference(SeqId(2)));
        verse_refs
            .takes
            .get_mut(0)
            .unwrap()
            .events
            .insert(e.key(), e);
    }
    let verse = Sequence::new(
        SeqId(1),
        "Verse",
        Vector::from(vec![verse_refs, notes_lane(3, 8, &mut ev)]),
    );
    let mut song_refs = Lane::new(LaneId(4), TakeId(4), "refs");
    for i in 0..8 {
        let e = ev(
            i * 8 * bar,
            EventBody::Reference {
                seq: SeqId(1),
                length: None,
                loop_count: LoopCount::Count(1),
                transpose: (i % 3) as i8,
            },
        );
        song_refs
            .takes
            .get_mut(0)
            .unwrap()
            .events
            .insert(e.key(), e);
    }
    let song = Sequence::new(
        SeqId(0),
        "Song",
        Vector::from(vec![song_refs, notes_lane(5, 64, &mut ev)]),
    );
    Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![song, verse, riff],
        counters: Counters {
            seq: 3,
            lane: 6,
            take: 6,
            event: next,
            ..Counters::default()
        },
    })
    .unwrap()
}

/// A 64-bar locked sequence whose one lane loops every beat over 10 000
/// events (almost all beyond the loop limit).
fn looped_doc() -> Document {
    let d = Document::new();
    let d = apply(
        &d,
        &Command::Sequence(SequenceCmd::New { name: "L".into() }),
    )
    .unwrap()
    .doc;
    let d = apply(
        &d,
        &Command::Sequence(SequenceCmd::SetLength {
            seq: SeqId(0),
            length: SeqLength::Locked(Tick(256 * PPQ)),
        }),
    )
    .unwrap()
    .doc;
    let d = apply(
        &d,
        &Command::Lane(LaneCmd::SetLength {
            seq: SeqId(0),
            lane: LaneId(0),
            length: Some(Tick(PPQ)),
            looped: true,
            locked: false,
        }),
    )
    .unwrap()
    .doc;
    let specs = (0..10_000i64)
        .map(|i| EventSpec::new(Tick(i * 4), EventBody::note(60, 100, Tick(1))))
        .collect();
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: specs,
        }),
    )
    .unwrap()
    .doc
}

fn benches(c: &mut Criterion) {
    let big = big_doc();
    let edit = Command::Event(EventCmd::SetVelocity {
        events: vec![EventId(54_321)],
        velocity: 1,
    });
    c.bench_function("one edit on 100k events", |b| {
        b.iter(|| apply(black_box(&big), black_box(&edit)).unwrap())
    });

    let song = nested_song();
    let filter = ViewFilter::default();
    c.bench_function("flatten 64 bars, 3 levels", |b| {
        b.iter(|| {
            flatten(black_box(&song), SeqId(0), None, &filter)
                .unwrap()
                .count()
        })
    });

    let looped = looped_doc();
    c.bench_function("flatten 1-beat loop over 10k events, 64 bars", |b| {
        b.iter(|| {
            flatten(black_box(&looped), SeqId(0), None, &filter)
                .unwrap()
                .count()
        })
    });

    let text = to_json(&big).unwrap();
    let mut g = c.benchmark_group("load");
    g.sample_size(10);
    g.bench_function("load 100k events", |b| {
        b.iter(|| from_json(black_box(&text)).unwrap())
    });
    g.finish();
}

criterion_group!(model, benches);
criterion_main!(model);
