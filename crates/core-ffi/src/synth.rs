//! Tracking a hardware synth (sub-project 5 spec §6.7; plan 5c-2): the
//! shift a `return_of` lane's recording gets and the mutes "Mute the MIDI
//! lane after recording" owes a pass. Control side only. A synth's return
//! lane is `core_model::sequence::Sequence::return_lane`.

use core_model::command::RecordPass;
use core_model::document::Document;
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::lane::LaneInput;
use std::collections::BTreeMap;

/// Frames a lane recording `dest`'s return moves earlier (spec §6.7; plan
/// 5c-2 Ruling 4): the destination's return latency at `rate`, while it
/// is an external MIDI destination; else 0.
pub fn return_frames(doc: &Document, dest: Option<DestId>, rate: f64) -> i64 {
    dest.and_then(|d| doc.instruments().get(&d))
        .filter(|d| d.kind.is_external())
        .map_or(0, |d| {
            (f64::from(d.return_latency_ms) * rate / 1000.0).round() as i64
        })
}

/// Keeps the "mute after" entries (keyed by sequence and return lane,
/// naming the MIDI lane) whose return lane still records the return of
/// the MIDI lane's destination in `doc`: an undone Track Synth leaves
/// none behind (Task 8 ruling M10).
pub fn keep_live_mutes(doc: &Document, mutes: &mut BTreeMap<(SeqId, LaneId), LaneId>) {
    mutes.retain(|(seq, ret), midi| {
        let dest = doc.lane(*seq, *midi).ok().and_then(|l| l.midi_dest);
        let returns = doc
            .lane(*seq, *ret)
            .ok()
            .and_then(|l| l.input.as_ref())
            .and_then(LaneInput::return_of);
        dest.is_some() && returns == dest
    });
}

/// What a pass owes "Mute the MIDI lane after recording" (plan 5c-2
/// Ruling 6): for every return lane in `mutes` (keyed by sequence and
/// return lane, naming the MIDI lane) that the pass placed audio on, the
/// key and the MIDI lane to mute.
pub fn mutes_after(
    pass: &RecordPass,
    mutes: &BTreeMap<(SeqId, LaneId), LaneId>,
) -> Vec<((SeqId, LaneId), SeqId, LaneId)> {
    pass.lanes
        .iter()
        .filter(|l| {
            !l.active.regions.is_empty() || l.takes.iter().any(|(_, c)| !c.regions.is_empty())
        })
        .filter_map(|l| {
            mutes
                .get(&(l.seq, l.lane))
                .map(|midi| ((l.seq, l.lane), l.seq, *midi))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::fixtures::Builder;
    use core_model::command::{
        Command, LaneCmd, LaneRecord, RecordedContent, RecordedRegion, RoutingCmd,
    };
    use core_model::document::DestKind;
    use core_model::lane::InputChannels;
    use core_model::time::Tick;

    /// Review Focus 3: only an external synth's latency shifts a take.
    #[test]
    fn a_return_shift_needs_an_external_destination() {
        let mut b = Builder::new();
        let juno = b.dest("Juno", "Juno", 0).unwrap();
        let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
        let pad = b
            .dest_kind("Pad", DestKind::plugin("au:pad"), None)
            .unwrap();
        b.run(Command::Routing(RoutingCmd::SetReturnLatency {
            dest: juno,
            ms: 10.0,
        }))
        .unwrap();
        let doc = b.finish();
        assert_eq!(return_frames(&doc, Some(juno), 48_000.0), 480);
        assert_eq!(return_frames(&doc, Some(juno), 44_100.0), 441);
        for d in [Some(piano), Some(pad), Some(DestId(99)), None] {
            assert_eq!(return_frames(&doc, d, 48_000.0), 0, "{d:?}");
        }
    }

    /// Ruling M10: only entries whose return lane still records the MIDI
    /// lane's synth stay.
    #[test]
    fn only_live_return_links_keep_their_mute() {
        let mut b = Builder::new();
        let juno = b.dest("Juno", "Juno", 0).unwrap();
        let (seq, part) = b.sequence("S", "Part", Some(juno)).unwrap();
        let ret = b.lane(seq, "Juno audio", None).unwrap();
        let ret2 = b.lane(seq, "Juno audio 2", None).unwrap();
        let plain = b.lane(seq, "Plain", None).unwrap();
        for (lane, return_of) in [(ret, Some(juno)), (ret2, Some(juno)), (plain, None)] {
            b.run(Command::Lane(LaneCmd::SetInput {
                seq,
                lane,
                input: Some(LaneInput::Audio {
                    channels: InputChannels::Mono(0),
                    return_of,
                }),
            }))
            .unwrap();
        }
        let doc = b.finish();
        let mut mutes = BTreeMap::from([
            ((seq, ret), part),
            // A plain lane, a gone one, a gone sequence, a gone MIDI lane.
            ((seq, plain), part),
            ((seq, LaneId(99)), part),
            ((SeqId(97), ret), part),
            ((seq, ret2), LaneId(98)),
        ]);
        keep_live_mutes(&doc, &mut mutes);
        assert_eq!(mutes, BTreeMap::from([((seq, ret), part)]));
    }

    #[test]
    fn a_pass_owes_the_mutes_of_the_return_lanes_it_placed_audio_on() {
        let region = RecordedRegion {
            media: 0,
            tick: Tick(0),
            source_offset: 0,
            length: 10,
        };
        let content = |regions: Vec<RecordedRegion>| RecordedContent {
            events: Vec::new(),
            regions,
        };
        let lane =
            |l: u64, active: RecordedContent, takes: Vec<(String, RecordedContent)>| LaneRecord {
                seq: SeqId(1),
                lane: LaneId(l),
                clear: Vec::new(),
                active,
                takes,
            };
        let pass = RecordPass {
            with_gaps: false,
            media: Vec::new(),
            lanes: vec![
                lane(5, content(vec![region]), Vec::new()),
                lane(
                    6,
                    RecordedContent::default(),
                    vec![("Take 2".into(), content(vec![region]))],
                ),
                lane(7, RecordedContent::default(), Vec::new()),
            ],
        };
        let mutes = BTreeMap::from([
            ((SeqId(1), LaneId(5)), LaneId(1)),
            ((SeqId(1), LaneId(6)), LaneId(2)),
            ((SeqId(1), LaneId(7)), LaneId(3)),
        ]);
        assert_eq!(
            mutes_after(&pass, &mutes),
            vec![
                ((SeqId(1), LaneId(5)), SeqId(1), LaneId(1)),
                ((SeqId(1), LaneId(6)), SeqId(1), LaneId(2)),
            ]
        );
    }
}
