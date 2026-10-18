//! Arrangement blocks (spec §3.1, §6): each rule, reference isolation.

use core_engine::fixtures::{Builder, Q};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{EventId, LaneId, SeqId};
use core_model::time::Tick;
use std::time::Instant;
use vizdaw_ffi::{BlockKind, BlockRow, BlockRule, Session};

const BAR: i64 = 4 * Q;

/// Lane "L" of sequence "S" with `notes` `(tick, duration)`, plus a
/// sequence "Sub" (one bar) referenced at each tick of `refs`.
fn lane(notes: &[(i64, i64)], refs: &[i64]) -> (Document, SeqId, LaneId, Vec<EventId>) {
    let mut b = Builder::new();
    let d = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (sub, sub_lane) = b.sequence("Sub", "L", Some(d)).unwrap();
    b.notes(sub, sub_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    let list: Vec<(i64, u8, u16, i64)> = notes.iter().map(|(t, d)| (*t, 60, 0x8000, *d)).collect();
    let mut ids = b.notes(seq, lane, &list).unwrap();
    for t in refs {
        ids.push(b.reference(seq, lane, *t, sub, 1, 0).unwrap());
    }
    (b.finish(), seq, lane, ids)
}

fn run(doc: Document, seq: SeqId, lane: LaneId, rule: BlockRule) -> Vec<BlockRow> {
    Session::from_document(doc)
        .unwrap()
        .blocks(seq.0, lane.0, rule)
        .unwrap()
}

fn spans(blocks: &[BlockRow]) -> Vec<(i64, i64, usize)> {
    blocks
        .iter()
        .map(|b| (b.start, b.end, b.events.len()))
        .collect()
}

#[test]
fn silence_gap_splits_where_the_gap_reaches_the_size() {
    let notes = [(0, Q), (Q, Q), (4 * Q, Q)];
    let (doc, seq, l, _) = lane(&notes, &[]);
    let split = run(doc.clone(), seq, l, BlockRule::SilenceGap { ticks: 2 * Q });
    assert_eq!(spans(&split), vec![(0, 2 * Q, 2), (4 * Q, 5 * Q, 1)]);
    let joined = run(doc, seq, l, BlockRule::SilenceGap { ticks: 2 * Q + 1 });
    assert_eq!(spans(&joined), vec![(0, 5 * Q, 3)]);
    assert!(
        joined
            .iter()
            .all(|b| b.kind == BlockKind::Events && b.label.is_none())
    );
}

#[test]
fn references_are_always_their_own_block() {
    let notes = [(0, Q), (2 * BAR, Q)];
    let (doc, seq, l, ids) = lane(&notes, &[BAR]);
    let blocks = run(doc, seq, l, BlockRule::WholeLane);
    assert_eq!(spans(&blocks), vec![(0, 2 * BAR + Q, 2), (BAR, 2 * BAR, 1)]);
    let r = &blocks[1];
    assert_eq!(r.kind, BlockKind::Reference);
    assert_eq!(r.events, vec![ids[2].0]);
    assert_eq!(r.label.as_deref(), Some("Sub"));
    assert!(r.ref_seq.is_some());
    // A reference between two close notes does not split their block.
    let notes = [(0, Q), (Q + 10, Q)];
    let (doc, seq, l, _) = lane(&notes, &[Q]);
    let blocks = run(doc, seq, l, BlockRule::SilenceGap { ticks: Q });
    assert_eq!(
        blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Events)
            .count(),
        1
    );
    assert_eq!(
        blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Reference)
            .count(),
        1
    );
}

#[test]
fn fixed_bars_group_by_bar_number() {
    let notes = [(0, Q), (BAR + 1, Q), (2 * BAR + 5, 3 * BAR)];
    let (doc, seq, l, _) = lane(&notes, &[]);
    let blocks = run(doc, seq, l, BlockRule::FixedBars { bars: 2 });
    assert_eq!(spans(&blocks), vec![(0, 2 * BAR, 2), (2 * BAR, 4 * BAR, 1)]);
}

#[test]
fn fixed_bars_put_the_pickup_in_bar_zero() {
    let notes = [(-Q, Q), (0, Q)];
    let (doc, seq, l, _) = lane(&notes, &[]);
    let blocks = run(doc, seq, l, BlockRule::FixedBars { bars: 1 });
    assert_eq!(spans(&blocks), vec![(-BAR, 0, 1), (0, BAR, 1)]);
}

#[test]
fn whole_lane_and_empty_lanes() {
    let (doc, seq, l, _) = lane(&[(Q, Q), (10 * BAR, Q)], &[]);
    let blocks = run(doc, seq, l, BlockRule::WholeLane);
    assert_eq!(spans(&blocks), vec![(Q, 10 * BAR + Q, 2)]);
    let (doc, seq, l, _) = lane(&[], &[]);
    assert!(run(doc, seq, l, BlockRule::WholeLane).is_empty());
}

#[test]
fn a_block_of_muted_events_is_muted() {
    let mut b = Builder::new();
    let d = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, l) = b.sequence("S", "L", Some(d)).unwrap();
    let mut a = EventSpec::new(Tick(0), EventBody::note(60, 0x8000, Tick(Q)));
    a.mute = true;
    let mut c = EventSpec::new(Tick(Q), EventBody::note(62, 0x8000, Tick(Q)));
    c.mute = true;
    b.events(seq, l, vec![a, c]).unwrap();
    let blocks = run(b.finish(), seq, l, BlockRule::WholeLane);
    assert!(blocks[0].muted);
}

#[test]
fn silence_gap_blocks_of_100_000_notes() {
    let mut b = Builder::new();
    let d = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, l) = b.sequence("Big", "L", Some(d)).unwrap();
    let specs: Vec<EventSpec> = (0..100_000_i64)
        .map(|i| EventSpec::new(Tick(i * Q / 8), EventBody::note(60, 0x8000, Tick(Q / 8))))
        .collect();
    b.events(seq, l, specs).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let start = Instant::now();
    let blocks = s
        .blocks(seq.0, l.0, BlockRule::SilenceGap { ticks: Q })
        .unwrap();
    let took = start.elapsed();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].events.len(), 100_000);
    let bound_ms = if cfg!(debug_assertions) { 2_000 } else { 100 };
    assert!(took.as_millis() < bound_ms, "blocks took {took:?}");
}

#[test]
fn huge_ticks_and_sizes_do_not_overflow() {
    // The furthest a document may hold (the model refuses beyond it).
    let big = core_model::rules::MAX_TICKS - Q;
    let (doc, seq, l, _) = lane(&[(0, Q), (big, 4 * Q)], &[]);
    for rule in [
        BlockRule::SilenceGap { ticks: i64::MAX },
        BlockRule::SilenceGap { ticks: 1 },
        BlockRule::FixedBars { bars: u32::MAX },
        BlockRule::FixedBars { bars: 1 },
        BlockRule::WholeLane,
    ] {
        let blocks = run(doc.clone(), seq, l, rule);
        let total: usize = blocks.iter().map(|b| b.events.len()).sum();
        assert_eq!(total, 2, "{rule:?}");
        assert!(blocks.iter().all(|b| b.start <= b.end), "{rule:?}");
    }
}
