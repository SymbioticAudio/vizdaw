//! Drum lanes in playback (editors spec §4e): a drum lane plays a note
//! only if the note starts after playback, unmute or solo began (Plan 4e
//! Ruling 9: no lane ever starts a note mid-way, so this pins the rule
//! for drum lanes), and a lane's groove play quantize moves its notes.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{EventLog, LogRecord, OfflineBackend, OfflineSinks};
use core_model::command::{Command, LaneCmd};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use core_model::lane::QuantizeSpec;
use core_model::time::Tick;

const SR: f64 = 48_000.0;
/// Frames per quarter at the default 120 BPM.
const FQ: u64 = 24_000;

struct Run {
    engine: Engine,
    backend: OfflineBackend,
    log: EventLog,
}

fn start(doc: &Document, seq: SeqId) -> Run {
    let sinks = OfflineSinks::new(10_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(doc, seq).unwrap();
    let mut backend = OfflineBackend::new(SR, 512);
    backend.start(renderer).unwrap();
    Run {
        engine,
        backend,
        log,
    }
}

/// Note-ons only: `frame pitch`.
fn note_ons(log: &EventLog) -> Vec<(u64, u8)> {
    log.drain()
        .iter()
        .map(LogRecord::line)
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            match f.as_slice() {
                [frame, _, "90", pitch, vel] if *vel != "00" => {
                    Some((frame.parse().ok()?, u8::from_str_radix(pitch, 16).ok()?))
                }
                _ => None,
            }
        })
        .collect()
}

/// Sequence "Beat": drum lane "Drums" (kick held for two beats at 0,
/// snare at beat 2, hat at beat 3) and a plain lane "Keys" (a note at
/// beat 4); `flags` sets the drum lane's mute and the keys' solo first.
fn beat(drum_mute: bool, keys_solo: bool) -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, drums) = b.sequence("Beat", "Drums", Some(synth)).unwrap();
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38), ("Hat", 42)])
        .unwrap();
    b.notes(
        seq,
        drums,
        &[
            (0, 36, 0x8000, 2 * Q),
            (Q, 38, 0x8000, Q / 4),
            (2 * Q, 42, 0x8000, Q / 4),
        ],
    )
    .unwrap();
    let keys = b.lane(seq, "Keys", Some(synth)).unwrap();
    b.notes(seq, keys, &[(3 * Q, 60, 0x8000, Q / 4)]).unwrap();
    for (lane, mute, solo) in [(drums, drum_mute, false), (keys, false, keys_solo)] {
        b.run(Command::Lane(LaneCmd::SetFlags {
            seq,
            lane,
            mute: Some(mute),
            solo: Some(solo),
            arm: None,
        }))
        .unwrap();
    }
    (b.finish(), seq, drums)
}

#[test]
fn a_drum_note_that_started_before_play_never_sounds() {
    let (doc, seq, _) = beat(false, false);
    let mut run = start(&doc, seq);
    // Half a beat into the held kick.
    run.engine.locate_tick(Tick(Q / 2)).unwrap();
    run.engine.play().unwrap();
    run.backend.run_frames(512 * 200);
    assert_eq!(
        note_ons(&run.log),
        vec![(FQ / 2, 38), (3 * FQ / 2, 42), (5 * FQ / 2, 60)],
        "the kick that began before the play position is not started"
    );
}

#[test]
fn unmuting_or_soloing_mid_note_waits_for_the_next_note() {
    // Muted drum lane, then soloed keys: either way the drum lane is
    // silent until a third of a beat in, when it is let through.
    for (muted, soloed) in [(true, false), (false, true)] {
        let (silent, seq, _) = beat(muted, soloed);
        let mut run = start(&silent, seq);
        run.engine.play().unwrap();
        run.backend.run_frames(512 * 16);
        let (heard, seq2, _) = if soloed {
            // Soloing the drum lane too lets it play beside the keys.
            let (d, s, drums) = beat(false, true);
            let mut b = Builder::from_doc(d);
            b.run(Command::Lane(LaneCmd::SetFlags {
                seq: s,
                lane: drums,
                mute: None,
                solo: Some(true),
                arm: None,
            }))
            .unwrap();
            (b.finish(), s, drums)
        } else {
            beat(false, false)
        };
        run.engine.load(&heard, seq2).unwrap();
        run.backend.run_frames(512 * 200);
        assert_eq!(
            note_ons(&run.log),
            vec![(FQ, 38), (2 * FQ, 42), (3 * FQ, 60)],
            "muted {muted} soloed {soloed}: the held kick does not start mid-way"
        );
    }
}

#[test]
fn a_swung_drum_lane_plays_its_offbeats_late() {
    let (doc, seq, drums) = beat(false, false);
    let mut b = Builder::from_doc(doc);
    b.notes(seq, drums, &[(Q / 2 + 30, 42, 0x8000, Q / 8)])
        .unwrap();
    b.run(Command::Lane(LaneCmd::SetPlayQuantize {
        seq,
        lane: drums,
        quantize: Some(QuantizeSpec::swing(Tick(Q / 2), 66)),
    }))
    .unwrap();
    let mut run = start(&b.finish(), seq);
    run.engine.play().unwrap();
    run.backend.run_frames(512 * 200);
    // The offbeat eighth plays at 66 % of the beat: tick 7680 + 2457,
    // frame 15 839 (rounded down) at 120 BPM.
    let ons = note_ons(&run.log);
    assert_eq!(ons.first(), Some(&(0, 36)));
    assert_eq!(ons.get(1), Some(&(15_839, 42)), "{ons:?}");
    assert_eq!(&ons[2..], &[(FQ, 38), (2 * FQ, 42), (3 * FQ, 60)]);
}
