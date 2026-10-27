//! Record's count-in (sub-project 5 spec §6.4; plan 5d-1 Ruling 7): one
//! click per beat, the first of each bar accented, with the timeline
//! standing still; the transport rolls on the block where the count-in
//! ends, and the clicks are delayed by the graph latency so the downbeat
//! is heard one beat after the last click. Stop cancels it.

use core_engine::backend::AudioBackend;
use core_engine::count_in::{ACCENT_LEVEL, BEAT_LEVEL, CountIn};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, clicks};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::transport::PlayState;

const BLOCK: u32 = 512;

/// An engine on an empty sequence, its graph latency `latency`.
fn rig(latency: u32) -> (Engine, OfflineBackend) {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("Song", "Lane", None).unwrap();
    if latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={latency}")))
            .unwrap();
    }
    let doc = b.finish();
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    assert_eq!(engine.latency_frames(), u64::from(latency));
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    (engine, out)
}

/// One 4/4 bar at 120 BPM: four beats of 24 000 frames.
const BAR: CountIn = CountIn {
    beats: 4,
    per_bar: 4,
    beat_frames: 24_000.0,
};

fn count_in(engine: &mut Engine, c: CountIn) {
    let mut p = engine.begin_play(None);
    p.set_count_in(Some(c));
    p.wait();
    engine.finish_play(p).unwrap();
}

/// Renders `blocks` blocks; the left channel, and per block whether the
/// transport played and whether a count-in was on after it.
fn render(
    engine: &Engine,
    out: &mut OfflineBackend,
    blocks: usize,
) -> (Vec<f32>, Vec<(bool, bool)>) {
    let mut left = Vec::new();
    let mut states = Vec::new();
    for _ in 0..blocks {
        out.block(BLOCK);
        left.extend_from_slice(out.output(BLOCK).0);
        states.push((engine.state() == PlayState::Playing, engine.counting_in()));
    }
    (left, states)
}

/// The peak of the 25 ms click starting at `at`.
fn peak(left: &[f32], at: usize) -> f32 {
    left[at..at + 1200].iter().fold(0f32, |m, x| m.max(x.abs()))
}

#[test]
fn a_count_in_clicks_each_beat_then_rolls_on_the_beat() {
    let (mut engine, mut out) = rig(0);
    count_in(&mut engine, BAR);
    let (left, states) = render(&engine, &mut out, 200);
    // 96 000 frames of count-in, padded so the roll lands on a block
    // boundary: 256 frames of silence (96 000 % 512 = 256), clicks at
    // 256 + 24 000·k, the roll at 96 256 (block 188).
    let at: Vec<usize> = (0..4).map(|k| 256 + 24_000 * k).collect();
    let got = clicks(&left, 0, 0.01, 200);
    assert_eq!(got.len(), 4, "{got:?}");
    for (g, a) in got.iter().zip(&at) {
        assert!(*g >= *a && *g <= a + 2, "click at {g}, beat at {a}");
    }
    assert!((peak(&left, at[0]) - ACCENT_LEVEL).abs() < 0.05);
    for a in &at[1..] {
        assert!((peak(&left, *a) - BEAT_LEVEL).abs() < 0.05);
    }
    // The timeline stands still until block 188, then plays.
    for (k, (playing, counting)) in states.iter().enumerate() {
        if k < 188 {
            assert!(!playing && *counting, "block {k}");
        } else {
            assert!(*playing && !counting, "block {k}");
        }
    }
    assert_eq!(engine.transport_frames(), (200 - 188) * u64::from(BLOCK));
}

#[test]
fn the_clicks_wait_for_the_graph_latency_and_the_roll_does_not() {
    let (mut engine, mut out) = rig(1000);
    count_in(&mut engine, BAR);
    let (left, states) = render(&engine, &mut out, 200);
    let got = clicks(&left, 0, 0.01, 200);
    assert_eq!(got.len(), 4, "{got:?}");
    for (k, g) in got.iter().enumerate() {
        let a = 256 + 1000 + 24_000 * k;
        assert!(*g >= a && *g <= a + 2, "click at {g}, beat at {a}");
    }
    // Rolled at 96 256, heard 1000 frames later: a beat after the last
    // click, which is heard at 73 256.
    assert!(states[187].1 && states[188].0);
}

#[test]
fn stop_cancels_a_count_in() {
    let (mut engine, mut out) = rig(0);
    count_in(&mut engine, BAR);
    let (_, states) = render(&engine, &mut out, 60);
    assert!(states.iter().all(|(p, c)| !p && *c));
    engine.stop().unwrap();
    let (left, states) = render(&engine, &mut out, 200);
    assert!(states.iter().all(|(p, c)| !p && !c), "it never rolls");
    assert!(
        clicks(&left, 0, 0.01, 100).is_empty(),
        "no click after the stop"
    );
}

#[test]
fn a_plain_play_during_a_count_in_takes_over() {
    let (mut engine, mut out) = rig(0);
    count_in(&mut engine, BAR);
    let (_, states) = render(&engine, &mut out, 10);
    assert!(states.iter().all(|(p, c)| !p && *c));
    let p = engine.begin_play(None);
    engine.finish_play(p).unwrap();
    let (left, states) = render(&engine, &mut out, 185);
    assert!(states[0].0 && !states[0].1, "rolls now, no longer counting");
    assert!(states.iter().all(|(p, c)| *p && !c), "never counts again");
    assert!(clicks(&left, 0, 0.01, 185).is_empty(), "the clicks stopped");
    // The count-in's end would have been at total block 188, rel. block 178:
    // a second roll there would show as a position that is not continuous.
    assert_eq!(engine.transport_frames(), 185 * u64::from(BLOCK));
}
