//! Property tests over generated documents and command sequences.

use core_model::command::{
    Command, EventCmd, LaneCmd, ReferenceCmd, RoutingCmd, SequenceCmd, apply,
};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, LoopCount};
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{DestId, EventId, LaneId, SeqId};
use core_model::lane::Lane;
use core_model::query::{flatten, seq_length, use_counts};
use core_model::rules::{find_cycle, validate};
use core_model::selection::ViewFilter;
use core_model::sequence::Sequence;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;

const Q: i64 = PPQ;

/// An abstract edit; indices are resolved modulo the current ID lists so
/// every generated op names something that exists.
#[derive(Debug, Clone)]
enum Op {
    NewSeq,
    AddLane(usize),
    Note {
        seq: usize,
        lane: usize,
        beat: i64,
        pitch: u8,
        beats: i64,
    },
    Ref {
        seq: usize,
        lane: usize,
        target: usize,
        beat: i64,
        len: Option<i64>,
        loops: u32,
        fill: bool,
        transpose: i8,
    },
    RePoint {
        ev: usize,
        target: usize,
    },
    Delete {
        ev: usize,
    },
    Move {
        ev: usize,
        beats: i64,
    },
    DeleteSeq {
        seq: usize,
        confirm: bool,
    },
    PrivateCopy {
        ev: usize,
    },
    Flatten {
        ev: usize,
    },
    LaneLoop {
        seq: usize,
        lane: usize,
        beats: i64,
        looped: bool,
    },
    LaneMute {
        seq: usize,
        lane: usize,
        mute: bool,
        solo: bool,
    },
    SetDest {
        seq: usize,
        lane: usize,
        dest: bool,
    },
    Transpose {
        ev: usize,
        semis: i8,
    },
    Automation {
        seq: usize,
        lane: usize,
        beat: i64,
        strip: u64,
        value: f64,
    },
}

fn arb_op() -> impl Strategy<Value = Op> {
    let i = || 0usize..8;
    prop_oneof![
        1 => Just(Op::NewSeq),
        1 => i().prop_map(Op::AddLane),
        6 => (i(), i(), 0i64..16, 0u8..=127, 1i64..4)
            .prop_map(|(seq, lane, beat, pitch, beats)| Op::Note { seq, lane, beat, pitch, beats }),
        4 => (i(), i(), i(), 0i64..16, prop::option::of(1i64..12), 1u32..4, any::<bool>(), -24i8..24)
            .prop_map(|(seq, lane, target, beat, len, loops, fill, transpose)| Op::Ref { seq, lane, target, beat, len, loops, fill, transpose }),
        2 => (i(), i()).prop_map(|(ev, target)| Op::RePoint { ev, target }),
        2 => i().prop_map(|ev| Op::Delete { ev }),
        2 => (i(), -4i64..4).prop_map(|(ev, beats)| Op::Move { ev, beats }),
        1 => (i(), any::<bool>()).prop_map(|(seq, confirm)| Op::DeleteSeq { seq, confirm }),
        1 => i().prop_map(|ev| Op::PrivateCopy { ev }),
        1 => i().prop_map(|ev| Op::Flatten { ev }),
        1 => (i(), i(), 1i64..8, any::<bool>()).prop_map(|(seq, lane, beats, looped)| Op::LaneLoop { seq, lane, beats, looped }),
        1 => (i(), i(), any::<bool>(), any::<bool>()).prop_map(|(seq, lane, mute, solo)| Op::LaneMute { seq, lane, mute, solo }),
        1 => (i(), i(), any::<bool>()).prop_map(|(seq, lane, dest)| Op::SetDest { seq, lane, dest }),
        1 => (i(), -12i8..12).prop_map(|(ev, semis)| Op::Transpose { ev, semis }),
        2 => (i(), i(), 0i64..16, 0u64..4, (0u32..=1000).prop_map(|k| f64::from(k) / 1000.0))
            .prop_map(|(seq, lane, beat, strip, value)| Op::Automation { seq, lane, beat, strip, value }),
    ]
}

fn pick<T: Copy>(v: &[T], i: usize) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        v.get(i % v.len()).copied()
    }
}

fn seqs(d: &Document) -> Vec<SeqId> {
    d.sequences().keys().copied().collect()
}

fn lane_of(d: &Document, s: SeqId, i: usize) -> Option<LaneId> {
    let ids: Vec<LaneId> = d.sequence(s).ok()?.lanes.iter().map(|l| l.id).collect();
    pick(&ids, i)
}

fn events(d: &Document, refs_only: bool) -> Vec<EventId> {
    d.all_events()
        .filter(|(_, e)| !refs_only || matches!(e.body, EventBody::Reference { .. }))
        .map(|(_, e)| e.id)
        .collect()
}

/// Turns an abstract op into a concrete command, if it can name targets.
fn concrete(d: &Document, op: &Op) -> Option<Command> {
    let s = seqs(d);
    Some(match op.clone() {
        Op::NewSeq => Command::Sequence(SequenceCmd::New { name: "gen".into() }),
        Op::AddLane(i) => Command::Lane(LaneCmd::Add {
            seq: pick(&s, i)?,
            name: "l".into(),
            index: None,
        }),
        Op::Note {
            seq,
            lane,
            beat,
            pitch,
            beats,
        } => {
            let seq = pick(&s, seq)?;
            Command::Event(EventCmd::Insert {
                seq,
                lane: lane_of(d, seq, lane)?,
                events: vec![EventSpec::new(
                    Tick(beat * Q),
                    EventBody::note(pitch, 100, Tick(beats * Q)),
                )],
            })
        }
        Op::Automation {
            seq,
            lane,
            beat,
            strip,
            value,
        } => {
            let seq = pick(&s, seq)?;
            Command::Event(EventCmd::Insert {
                seq,
                lane: lane_of(d, seq, lane)?,
                events: vec![EventSpec::new(
                    Tick(beat * Q),
                    EventBody::Automation {
                        target: core_model::automation::AutomationTarget::StripFader(
                            core_model::ids::StripId(strip),
                        ),
                        value,
                    },
                )],
            })
        }
        Op::Ref {
            seq,
            lane,
            target,
            beat,
            len,
            loops,
            fill,
            transpose,
        } => {
            let seq = pick(&s, seq)?;
            Command::Reference(ReferenceCmd::Insert {
                seq,
                lane: lane_of(d, seq, lane)?,
                tick: Tick(beat * Q),
                target: pick(&s, target)?,
                length: len.map(|b| Tick(b * Q)),
                loop_count: if fill {
                    LoopCount::Fill
                } else {
                    LoopCount::Count(loops)
                },
                transpose,
            })
        }
        Op::RePoint { ev, target } => Command::Reference(ReferenceCmd::RePoint {
            event: pick(&events(d, true), ev)?,
            target: pick(&s, target)?,
        }),
        Op::Delete { ev } => Command::Event(EventCmd::Delete {
            events: vec![pick(&events(d, false), ev)?],
        }),
        Op::Move { ev, beats } => Command::Event(EventCmd::Move {
            events: vec![pick(&events(d, false), ev)?],
            delta: Tick(beats * Q),
            to_lane: None,
        }),
        Op::DeleteSeq { seq, confirm } => Command::Sequence(SequenceCmd::Delete {
            seq: pick(&s, seq)?,
            delete_references: confirm,
        }),
        Op::PrivateCopy { ev } => Command::Reference(ReferenceCmd::MakePrivateCopy {
            event: pick(&events(d, true), ev)?,
        }),
        Op::Flatten { ev } => Command::Reference(ReferenceCmd::Flatten {
            event: pick(&events(d, true), ev)?,
        }),
        Op::LaneLoop {
            seq,
            lane,
            beats,
            looped,
        } => {
            let seq = pick(&s, seq)?;
            Command::Lane(LaneCmd::SetLength {
                seq,
                lane: lane_of(d, seq, lane)?,
                length: Some(Tick(beats * Q)),
                looped,
                locked: false,
            })
        }
        Op::LaneMute {
            seq,
            lane,
            mute,
            solo,
        } => {
            let seq = pick(&s, seq)?;
            Command::Lane(LaneCmd::SetFlags {
                seq,
                lane: lane_of(d, seq, lane)?,
                mute: Some(mute),
                solo: Some(solo),
                arm: None,
            })
        }
        Op::SetDest { seq, lane, dest } => {
            let seq = pick(&s, seq)?;
            Command::Lane(LaneCmd::SetDest {
                seq,
                lane: lane_of(d, seq, lane)?,
                dest: dest.then_some(DestId(0)),
            })
        }
        Op::Transpose { ev, semis } => Command::Event(EventCmd::Nudge {
            events: vec![pick(&events(d, false), ev)?],
            ticks: Tick(0),
            semitones: semis,
        }),
    })
}

/// Two sequences and one MIDI destination to start from.
fn seed() -> Document {
    let mut d = Document::new();
    for c in [
        Command::Routing(RoutingCmd::AddDestination {
            name: "d".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "e".into(),
                group: 1,
                channel: 2,
            },
            default_patch: None,
        }),
        Command::Sequence(SequenceCmd::New { name: "A".into() }),
        Command::Sequence(SequenceCmd::New { name: "B".into() }),
    ] {
        d = apply(&d, &c).unwrap().doc;
    }
    d
}

/// Applies the ops through a History, skipping refused commands. Returns the
/// history and the number of applied steps.
/// Runs the ops; returns the history and its number of undo steps (failed
/// and no-op commands add none).
fn build(ops: &[Op]) -> (History, usize) {
    let mut h = History::new(seed());
    for op in ops {
        if let Some(cmd) = concrete(h.doc(), op) {
            let _ = h.execute(&cmd, &ViewFilter::default());
        }
    }
    let n = h.undo_depth();
    (h, n)
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(arb_op(), 1..40)
}

/// Counts reference events per target by walking the serialized JSON.
fn brute_use_counts(d: &Document) -> BTreeMap<u64, usize> {
    fn walk(v: &Value, out: &mut BTreeMap<u64, usize>) {
        match v {
            Value::Object(m) => {
                if m.get("type").and_then(Value::as_str) == Some("reference") {
                    if let Some(s) = m.get("seq").and_then(Value::as_u64) {
                        *out.entry(s).or_insert(0) += 1;
                    }
                }
                m.values().for_each(|x| walk(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out: BTreeMap<u64, usize> = d.sequences().keys().map(|k| (k.0, 0)).collect();
    walk(&serde_json::to_value(d).unwrap(), &mut out);
    out
}

/// Naive reference implementation of flatten: expand every pass of every
/// reference recursively, then filter by window, without pruning.
fn naive(
    d: &Document,
    s: &Sequence,
    offset: i64,
    lo: i64,
    hi: i64,
    tr: i32,
    out: &mut Vec<String>,
) {
    let any_solo = s.lanes.iter().any(|l| l.solo);
    for lane in s.lanes.iter().filter(|l| !l.mute && (!any_solo || l.solo)) {
        let Some(take) = lane.active() else { continue };
        let (period, limit) = match lane.length {
            Some(l) if lane.looped => (Some(l.0), l.0),
            Some(l) => (None, l.0),
            None => (None, i64::MAX),
        };
        let passes = match period {
            Some(p) => (hi.max(0) + p - 1) / p,
            None => 1,
        };
        for k in 0..passes.max(1) {
            let pass_end = (k * period.unwrap_or(0))
                .saturating_add(limit)
                .saturating_add(lane.play_shift.0);
            let hi = hi.min(pass_end);
            let lo = match period {
                Some(p) => lo.max(k * p + lane.play_shift.0),
                None => lo,
            };
            for e in take.events.values() {
                if e.mute || e.tick.0 >= limit {
                    continue;
                }
                let lt = e.tick.0 + k * period.unwrap_or(0) + lane.play_shift.0;
                let is_ref = matches!(e.body, EventBody::Reference { .. });
                if (lt < lo && !is_ref) || lt >= hi {
                    continue;
                }
                naive_event(d, lane, e, lt, offset, lo, hi, tr, out);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn naive_event(
    d: &Document,
    lane: &Lane,
    e: &core_model::event::Event,
    lt: i64,
    offset: i64,
    win_lo: i64,
    hi: i64,
    tr: i32,
    out: &mut Vec<String>,
) {
    match &e.body {
        EventBody::Reference {
            seq,
            length,
            loop_count,
            transpose,
        } => {
            let Ok(t) = d.sequence(*seq) else { return };
            let b = seq_length(d, *seq).unwrap().0;
            let use_len = length.map_or(b, |l| l.0);
            let (iter, total) = match loop_count {
                LoopCount::Count(n) => (use_len, use_len * i64::from(*n)),
                LoopCount::Fill => (b, use_len),
            };
            let end = (lt + total).min(hi);
            let mut start = lt;
            let mut first = true;
            while start < end {
                let lo = (if first { -t.start_offset.0 } else { 0 }).max(win_lo - start);
                naive(
                    d,
                    t,
                    offset + start,
                    lo,
                    iter.min(end - start),
                    tr + i32::from(*transpose),
                    out,
                );
                start += iter;
                first = false;
            }
        }
        EventBody::Note {
            pitch, duration, ..
        } => {
            let p = (i32::from(*pitch) + tr).clamp(0, 127);
            let dur = duration.0.min(hi - lt);
            let dest = e.dest.or(lane.midi_dest).map(|x| x.0);
            out.push(format!("{} note {p} {dur} {dest:?}", offset + lt));
        }
        _ => {}
    }
}

fn played(d: &Document, s: SeqId) -> Vec<String> {
    let mut v: Vec<String> = flatten(d, s, None, &ViewFilter::default())
        .unwrap()
        .filter_map(|r| match r.body {
            EventBody::Note {
                pitch, duration, ..
            } => Some(format!(
                "{} note {pitch} {} {:?}",
                r.tick.0,
                duration.0,
                r.dest.map(|x| x.0)
            )),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn apply_n_then_undo_n_restores_the_original(ops in ops()) {
        let (mut h, n) = build(&ops);
        let after = h.doc().clone();
        for _ in 0..n { h.undo().unwrap(); }
        prop_assert_eq!(h.doc(), &seed());
        prop_assert!(h.undo().is_err());
        for _ in 0..n { h.redo().unwrap(); }
        prop_assert_eq!(h.doc(), &after);
    }

    #[test]
    fn redo_after_undo_returns_the_same_document(ops in ops()) {
        let (mut h, n) = build(&ops);
        prop_assume!(n > 0);
        let after = h.doc().clone();
        h.undo().unwrap();
        h.redo().unwrap();
        prop_assert_eq!(h.doc(), &after);
    }

    #[test]
    fn save_then_load_is_identical(ops in ops()) {
        let (h, _) = build(&ops);
        let text = to_json(h.doc()).unwrap();
        prop_assert_eq!(&from_json(&text).unwrap(), h.doc());
    }

    #[test]
    fn no_command_sequence_creates_a_cycle(ops in ops()) {
        let (h, _) = build(&ops);
        prop_assert_eq!(find_cycle(h.doc()), None);
        prop_assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));
    }

    #[test]
    fn use_counts_match_a_brute_force_recount(ops in ops()) {
        let (h, _) = build(&ops);
        let derived: BTreeMap<u64, usize> = use_counts(h.doc()).into_iter().map(|(k, v)| (k.0, v)).collect();
        prop_assert_eq!(derived, brute_use_counts(h.doc()));
    }

    #[test]
    fn flatten_matches_the_naive_implementation(ops in ops()) {
        let (h, _) = build(&ops);
        let d = h.doc();
        for s in d.sequences().values() {
            let mut expected = Vec::new();
            let len = seq_length(d, s.id).unwrap().0;
            naive(d, s, 0, -s.start_offset.0, len, 0, &mut expected);
            expected.sort();
            prop_assert_eq!(played(d, s.id), expected);
        }
    }

    #[test]
    fn private_copy_and_flatten_keep_playback(ops in ops(), pick_ev in 0usize..8) {
        let (h, _) = build(&ops);
        let d = h.doc();
        let refs = events(d, true);
        prop_assume!(!refs.is_empty());
        let ev = refs[pick_ev % refs.len()];
        let owner = d.locate(ev).unwrap().seq;
        let before = played(d, owner);
        // Flatten makes destinations explicit; events that had none follow
        // the owning lane afterwards (and the report warns), so compare
        // without destinations in that case.
        let strip = |v: Vec<String>| -> Vec<String> {
            v.into_iter().map(|x| x.rsplit_once(' ').map_or(x.clone(), |(a, _)| a.to_string())).collect()
        };
        let unresolved = before.iter().any(|x| x.ends_with("None"));
        for cmd in [ReferenceCmd::MakePrivateCopy { event: ev }, ReferenceCmd::Flatten { event: ev }] {
            let a = apply(d, &Command::Reference(cmd)).unwrap();
            let after = played(&a.doc, owner);
            if unresolved {
                prop_assert_eq!(strip(after), strip(before.clone()));
            } else {
                prop_assert_eq!(after, before.clone());
            }
            prop_assert!(validate(&a.doc).is_empty());
        }
    }
}
