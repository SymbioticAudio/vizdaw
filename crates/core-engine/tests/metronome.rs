//! The metronome (groups-vcas-transport Ruling 19): the count-in's click
//! on every beat, the first of each bar accented, following the tempo map
//! (ramps included) and the meter map, delayed by the graph latency. The
//! click level scales both peaks; `record_only` clicks only during a
//! record pass; a count-in hands over at the roll without a doubled
//! downbeat, and a loop wrap clicks its first beat.
//!
//! Preflight F15: the click starts at sin(0) = 0, so each click is checked
//! sample for sample against `click_sample` from its onset rather than by
//! its first non-zero sample, and levels by the ratio of two renders.

use core_engine::backend::AudioBackend;
use core_engine::count_in::{ACCENT_LEVEL, BEAT_LEVEL, CLICK_SECONDS, CountIn, click_sample};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::metronome::ClickSpec;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::schedule::TempoTable;
use core_model::command::{Command, SequenceCmd};
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::sequence::SeqLength;
use core_model::time::Tick;

const RATE: f64 = 48_000.0;
const BLOCK: u32 = 512;
const BAR: i64 = 4 * Q;
/// A click's frames.
const LEN: usize = (CLICK_SECONDS * RATE) as usize;

const ON: ClickSpec = ClickSpec {
    on: true,
    gain: 1.0,
    record_only: false,
};

/// An empty song `bars` bars long, with a latent bus when `latency` > 0.
fn song(bars: i64, latency: u32) -> (Builder, SeqId) {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("Song", "Track", None).unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(bars * BAR)),
    }))
    .unwrap();
    if latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={latency}")))
            .unwrap();
    }
    (b, seq)
}

fn meter(entries: &[(i64, u8, u8)]) -> MeterMap {
    MeterMap::new(
        entries
            .iter()
            .map(|(t, num, den)| MeterEntry {
                tick: Tick(*t),
                meter: Meter {
                    num: *num,
                    den: *den,
                },
                key: KeySig::default(),
            })
            .collect(),
    )
    .unwrap()
}

/// The engine on `doc`'s `seq`, its click `spec`, rendering offline.
fn rig(doc: &Document, seq: SeqId, spec: ClickSpec) -> (Engine, OfflineBackend) {
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(RATE), Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(doc, seq).unwrap();
    engine.set_click(spec);
    let mut out = OfflineBackend::new(RATE, BLOCK);
    out.start(renderer).unwrap();
    (engine, out)
}

/// Renders blocks until `frames` frames; the left channel (the right is
/// checked to match).
fn render(out: &mut OfflineBackend, frames: usize) -> Vec<f32> {
    let mut left = Vec::with_capacity(frames + BLOCK as usize);
    while left.len() < frames {
        out.block(BLOCK);
        let (l, r) = out.output(BLOCK);
        assert_eq!(l, r, "both channels click");
        left.extend_from_slice(l);
    }
    left
}

fn tempo_of(doc: &Document, seq: SeqId) -> TempoTable {
    let s = doc.sequence(seq).unwrap();
    TempoTable::build(&s.tempo, -s.start_offset, RATE)
}

/// Clicks `(onset, accented)` at `gain`, as `click_sample` sounds them.
fn expected(onsets: &[(usize, bool)], len: usize, gain: f32) -> Vec<f32> {
    let mut e = vec![0.0f32; len];
    for (at, accent) in onsets {
        for i in 0..LEN {
            if let Some(x) = e.get_mut(at + i) {
                *x += click_sample(u32::from(!accent), i as u64, RATE, gain);
            }
        }
    }
    e
}

/// Asserts `got` holds exactly `want`, naming the first frame that
/// differs.
fn same(got: &[f32], want: &[f32], what: &str) {
    let n = want.len().min(got.len());
    if let Some(f) = (0..n).find(|&f| got[f].to_bits() != want[f].to_bits()) {
        panic!("{what}: frame {f}: got {} want {}", got[f], want[f]);
    }
}

/// The peak of the click starting at `at`.
fn peak(left: &[f32], at: usize) -> f32 {
    left[at..at + LEN].iter().fold(0f32, |m, x| m.max(x.abs()))
}

#[test]
fn clicks_fall_on_every_beat_across_a_tempo_change() {
    let latency = 1000;
    let (mut b, seq) = song(9, latency);
    // 120 BPM, 90 from bar 3, a ramp from 90 to 150 over bars 5–6.
    b.tempo(
        seq,
        &[
            (0, 120, false),
            (2 * BAR, 90, false),
            (4 * BAR, 90, true),
            (6 * BAR, 150, false),
        ],
    )
    .unwrap();
    let doc = b.finish();
    let tempo = tempo_of(&doc, seq);
    let (mut engine, mut out) = rig(&doc, seq, ON);
    engine.play().unwrap();
    let end = tempo.tick_to_frame(Tick(8 * BAR)) as usize + latency as usize;
    let left = render(&mut out, end + LEN);
    let onsets: Vec<(usize, bool)> = (0..8 * 4)
        .map(|k| {
            let f = tempo.tick_to_frame(Tick(k * Q)) as usize + latency as usize;
            (f, k % 4 == 0)
        })
        .collect();
    // Each beat's frame really moves with the tempo.
    assert_eq!(onsets[1].0 - onsets[0].0, 24_000);
    assert_eq!(onsets[9].0 - onsets[8].0, 32_000);
    let want = expected(&onsets, end, 1.0);
    same(&left[..end], &want, "every beat through bar 8");
    // The first non-zero sample of each click is the one after its
    // onset (sin 0 = 0).
    for (at, _) in &onsets {
        assert_eq!(left[*at], 0.0);
        assert_ne!(left[at + 1], 0.0, "a click at {at}");
    }
}

#[test]
fn clicks_follow_a_meter_change() {
    let (mut b, seq) = song(6, 0);
    let m = meter(&[(0, 4, 4), (2 * BAR, 7, 8)]);
    b.run(Command::Sequence(SequenceCmd::SetMeter { seq, meter: m }))
        .unwrap();
    let doc = b.finish();
    let tempo = tempo_of(&doc, seq);
    let (mut engine, mut out) = rig(&doc, seq, ON);
    engine.play().unwrap();
    // Two 4/4 bars, then three 7/8 bars, on eighth notes.
    let mut beats: Vec<(i64, bool)> = (0..8).map(|k| (k * Q, k % 4 == 0)).collect();
    let eighth = Q / 2;
    beats.extend((0..21).map(|k| (2 * BAR + k * eighth, k % 7 == 0)));
    let onsets: Vec<(usize, bool)> = beats
        .iter()
        .map(|(t, a)| (tempo.tick_to_frame(Tick(*t)) as usize, *a))
        .collect();
    let end = onsets.last().unwrap().0 + LEN;
    let left = render(&mut out, end);
    same(
        &left[..end],
        &expected(&onsets, end, 1.0),
        "the meter's beats",
    );
    for (at, accent) in &onsets {
        let want = if *accent { ACCENT_LEVEL } else { BEAT_LEVEL };
        let p = peak(&left, *at);
        assert!((p - want).abs() < 0.05, "click at {at}: peak {p}");
    }
    // A 7/8 bar is 7 clicks an eighth note apart: 12 000 frames at 120.
    assert_eq!(onsets[9].0 - onsets[8].0, 12_000);
    assert_eq!(onsets[15].0 - onsets[8].0, 7 * 12_000);
}

#[test]
fn the_level_scales_the_click() {
    let (b, seq) = song(2, 0);
    let doc = b.finish();
    let take = |spec: ClickSpec| {
        let (mut engine, mut out) = rig(&doc, seq, spec);
        engine.play().unwrap();
        render(&mut out, 100_000)
    };
    let full = take(ON);
    let g = 10f64.powf(-6.0 / 20.0);
    let quiet = take(ClickSpec {
        gain: g as f32,
        ..ON
    });
    let mut compared = 0;
    for (x, y) in full.iter().zip(&quiet) {
        if x.abs() > 1e-3 {
            let r = f64::from(*y) / f64::from(*x);
            assert!((r - g).abs() < 1e-6, "{y} / {x} = {r}");
            compared += 1;
        }
    }
    assert!(compared > 1000, "{compared}");
    let p = peak(&full, 0);
    assert!((p - ACCENT_LEVEL).abs() < 0.05, "{p}");
    let off = take(ClickSpec { on: false, ..ON });
    assert!(off.iter().all(|x| *x == 0.0), "off is silent");
}

#[test]
fn record_only_clicks_only_while_recording() {
    let (b, seq) = song(4, 0);
    let doc = b.finish();
    let spec = ClickSpec {
        record_only: true,
        ..ON
    };
    let (mut engine, mut out) = rig(&doc, seq, spec);
    engine.play().unwrap();
    let played = render(&mut out, 100_000);
    assert!(played.iter().all(|x| *x == 0.0), "silent while playing");
    engine.stop().unwrap();
    render(&mut out, 4096);
    engine.locate_frame(0).unwrap();
    // A pass whose record range starts at beat 2: beats 0 and 1 are its
    // pre-roll.
    let _h = engine
        .start_recording(&RecordRequest {
            channels: vec![],
            range: Some((48_000, 200_000)),
            ring_frames: 4800,
        })
        .unwrap();
    engine.play().unwrap();
    // The Play reaches the renderer at the next block.
    let left = render(&mut out, 100_000);
    let onsets: Vec<(usize, bool)> = (0..4).map(|k| (k * 24_000, k == 0)).collect();
    same(
        &left[..96_000],
        &expected(&onsets, 96_000, 1.0),
        "the pass clicks",
    );
}

#[test]
fn the_count_in_hands_over_without_a_double_click() {
    let (b, seq) = song(4, 0);
    let doc = b.finish();
    let (mut engine, mut out) = rig(&doc, seq, ON);
    let mut p = engine.begin_play(None);
    p.set_count_in(Some(CountIn {
        beats: 4,
        per_bar: 4,
        beat_frames: 24_000.0,
    }));
    p.wait();
    engine.finish_play(p).unwrap();
    let left = render(&mut out, 200_000);
    // The count-in's four clicks at 256 + 24 000·k (padded so the roll
    // falls on block 188's boundary, as count_in.rs shows), then the
    // metronome from the roll's downbeat at 96 256: one click there.
    let mut onsets: Vec<(usize, bool)> = (0..4).map(|k| (256 + k * 24_000, k == 0)).collect();
    onsets.extend((0..4).map(|k| (96_256 + k * 24_000, k == 0)));
    same(
        &left[..190_000],
        &expected(&onsets, 190_000, 1.0),
        "one click per beat",
    );
    let p = peak(&left, 96_256);
    assert!(
        (p - ACCENT_LEVEL).abs() < 0.05,
        "one downbeat, not two: {p}"
    );
}

#[test]
fn a_loop_wrap_clicks_its_first_beat() {
    let (b, seq) = song(4, 0);
    let doc = b.finish();
    let (mut engine, mut out) = rig(&doc, seq, ON);
    // A beat and a half: the wrap falls mid-block at 36 000.
    engine.set_loop_range(Some((0, 36_000))).unwrap();
    engine.play().unwrap();
    let left = render(&mut out, 150_000);
    let onsets: Vec<(usize, bool)> = (0..4)
        .flat_map(|n| [(n * 36_000, true), (n * 36_000 + 24_000, false)])
        .collect();
    same(
        &left[..144_000],
        &expected(&onsets, 144_000, 1.0),
        "each pass clicks",
    );
}
