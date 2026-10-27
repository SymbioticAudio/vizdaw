//! Tracking a hardware synth (sub-project 5 spec §6.7; plan 5c-2): the
//! shift a `return_of` lane's recording gets and the mutes "Mute the MIDI
//! lane after recording" owes a pass. Control side only. A synth's return
//! lane is `core_model::sequence::Sequence::return_lane`.

use crate::error::VizError;
use crate::snapshot::DocSnapshot;
use core_engine::measure::{MeasureStatus, PINGS};
use core_model::command::RecordPass;
use core_model::document::{Destination, Document, MAX_RETURN_LATENCY_MS};
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput};
use std::collections::BTreeMap;
use std::time::Duration;

/// How long Measure waits at most (its notes take about 2.6 s, plus the
/// device's latencies).
pub const MEASURE_WAIT: Duration = Duration::from_secs(10);
/// Onsets Measure needs (spec §6.7: fewer than 3 fails).
pub const MIN_ONSETS: usize = 3;
/// Why Measure fails without them (spec §6.7).
pub const NO_SIGNAL: &str = "No signal on the synth's return input";

/// How long Measure waits: [`MEASURE_WAIT`]; with the test hooks, longer
/// when `VIZDAW_TEST_WAIT_SECS` says so (pre-flight M4).
pub fn measure_wait() -> Duration {
    #[cfg(feature = "test-hooks")]
    {
        let env = std::env::var("VIZDAW_TEST_WAIT_SECS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .map(Duration::from_secs)
            .unwrap_or_default();
        MEASURE_WAIT.max(env)
    }
    #[cfg(not(feature = "test-hooks"))]
    MEASURE_WAIT
}

/// What Measure found (`destination.measure_return`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MeasureRow {
    /// The return latency set, in ms.
    pub return_latency_ms: f32,
    /// Notes whose onset was found (3–5).
    pub onsets: u32,
    /// The document after the step "Set Return Latency".
    pub snapshot: DocSnapshot,
}

/// A measurement's result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// The return latency in ms, 0–500.
    pub ms: f32,
    /// Notes whose onset was found.
    pub onsets: u32,
}

/// The return latency the notes' heard runs `sent` and their onsets'
/// input runs `onsets` (by note) say (plan 5c-2 Ruling 8): the median of
/// `onset − heard − (delay − trim)` over the notes that found one (the
/// mean of the middle two for an even count), in ms at `rate`, clamped to
/// 0–500. Fewer than [`MIN_ONSETS`] onsets: [`NO_SIGNAL`].
pub fn measured(
    sent: &[Option<i64>],
    onsets: &[Option<i64>],
    delay: i64,
    trim: i64,
    rate: f64,
) -> Result<Measured, String> {
    let d = delay.saturating_sub(trim);
    let mut found: Vec<i64> = sent
        .iter()
        .zip(onsets)
        .filter_map(|(u, k)| match (u, k) {
            (Some(u), Some(k)) => Some(k.saturating_sub(*u).saturating_sub(d)),
            _ => None,
        })
        .collect();
    if found.len() < MIN_ONSETS {
        return Err(NO_SIGNAL.into());
    }
    found.sort_unstable();
    let n = found.len();
    let at = |i: usize| found.get(i).copied().unwrap_or(0) as f64;
    let median = if n % 2 == 1 {
        at(n / 2)
    } else {
        (at(n / 2 - 1) + at(n / 2)) / 2.0
    };
    let ms = (median * 1000.0 / rate.max(1.0)).clamp(0.0, f64::from(MAX_RETURN_LATENCY_MS));
    Ok(Measured {
        ms: ms as f32,
        onsets: u32::try_from(n).unwrap_or(u32::MAX),
    })
}

/// [`measured`] of a finished measurement, with the recording latency
/// trim its windows were placed with (the device's round trip as
/// recording counts it; final review I1).
pub fn measured_status(s: &MeasureStatus) -> Result<Measured, String> {
    let sent: Vec<Option<i64>> = (0..PINGS).map(|i| s.sent(i)).collect();
    let onsets: Vec<Option<i64>> = (0..PINGS).map(|i| s.onset(i)).collect();
    measured(
        &sent,
        &onsets,
        s.delay().unwrap_or(0),
        s.trim(),
        s.sample_rate(),
    )
}

/// External MIDI destination `dest` of `doc`, the only kind Measure
/// measures (plan 5c-2 Rulings 2 and 9; the engine leaves this check to
/// its caller): `Model` for an unknown one and for any other kind (the
/// built-in synth and plugin instruments).
pub fn external_dest(doc: &Document, dest: DestId) -> Result<&Destination, VizError> {
    let d = doc
        .instruments()
        .get(&dest)
        .ok_or_else(|| crate::events::bad(&format!("unknown id {dest}")))?;
    if !d.kind.is_external() {
        return Err(crate::events::bad(&format!(
            "{} is not an external MIDI destination: Measure measures a hardware synth",
            d.name
        )));
    }
    Ok(d)
}

/// The first lane recording `dest`'s return (sequences in ID order, lanes
/// in order, by `Sequence::return_lane`; plan 5c-2 Ruling 9): its
/// sequence, ID, name and channels.
pub fn return_lane(doc: &Document, dest: DestId) -> Option<(SeqId, LaneId, String, InputChannels)> {
    doc.sequences().values().find_map(|s| {
        let id = s.return_lane(dest, None)?;
        let l = s.lanes.iter().find(|l| l.id == id)?;
        match &l.input {
            Some(LaneInput::Audio { channels, .. }) => {
                Some((s.id, l.id, l.name.clone(), *channels))
            }
            _ => None,
        }
    })
}

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

    /// Spec §6.7 and plan 5c-2 Ruling 8: the median less the round trip;
    /// a stray onset does not move it.
    #[test]
    fn measured_takes_the_median_less_the_round_trip() {
        let sent = [
            Some(0),
            Some(24_000),
            Some(48_000),
            Some(72_000),
            Some(96_000),
        ];
        // 1200, 1210, 1190, 5000 (a stray) and 1200 frames, less 100.
        let onsets = [
            Some(1300),
            Some(25_310),
            Some(49_290),
            Some(77_100),
            Some(97_300),
        ];
        let m = measured(&sent, &onsets, 100, 0, 48_000.0).unwrap();
        assert_eq!((m.ms, m.onsets), (25.0, 5));
        // The latency trim counts as the device's (as recording does).
        let t = measured(&sent, &onsets, 100, 100, 48_000.0).unwrap();
        assert!((t.ms - 1300.0 * 1000.0 / 48_000.0).abs() < 1e-3, "{t:?}");
        // An even count: the mean of the middle two.
        let four = [onsets[0], onsets[1], None, onsets[3], onsets[4]];
        let e = measured(&sent, &four, 100, 0, 48_000.0).unwrap();
        assert!((e.ms - 1205.0 * 1000.0 / 48_000.0).abs() < 1e-3, "{e:?}");
        assert_eq!(e.onsets, 4);
    }

    /// Spec §6.7: fewer than 3 onsets fails with the spec's message; the
    /// result stays within 0–500 ms.
    #[test]
    fn measured_needs_three_onsets_and_stays_in_range() {
        let sent = [
            Some(0),
            Some(24_000),
            Some(48_000),
            Some(72_000),
            Some(96_000),
        ];
        let two = [Some(1200), None, Some(49_200), None, None];
        assert_eq!(
            measured(&sent, &two, 0, 0, 48_000.0),
            Err(NO_SIGNAL.to_string())
        );
        assert_eq!(
            measured(&sent, &[None; 5], 0, 0, 48_000.0),
            Err(NO_SIGNAL.to_string())
        );
        let late = sent.map(|u| u.map(|u| u + 30_000));
        assert_eq!(measured(&sent, &late, 0, 0, 48_000.0).unwrap().ms, 500.0);
        let early = sent.map(|u| u.map(|u| u + 10));
        assert_eq!(measured(&sent, &early, 100, 0, 48_000.0).unwrap().ms, 0.0);
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
