use core_engine::fixtures::{BAR, Builder, Q};
use core_engine::schedule::{CompileOptions, Schedule, TargetKind, TargetTable, compile};
use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

const SR: f64 = 48_000.0;

fn compile_seq(doc: &Document, seq: SeqId) -> Schedule {
    let mut table = TargetTable::new();
    compile(doc, seq, &CompileOptions::new(SR), &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap()
    .0
}

/// Note-on frames, in order.
fn on_frames(s: &Schedule) -> Vec<u64> {
    s.entries
        .iter()
        .filter(|e| s.bytes(&e.msg)[0] & 0xF0 == 0x90)
        .map(|e| e.frame)
        .collect()
}

/// Closed form for a ramp linear in BPM per tick from `b0` at tick 0 to
/// `b1` at tick `len`: t(x) = 60 / (PPQ·k) · ln(1 + k·x / b0).
fn ramp_seconds(x: f64, len: f64, b0: f64, b1: f64) -> f64 {
    let k = (b1 - b0) / len;
    60.0 / (PPQ as f64 * k) * (1.0 + k * x / b0).ln()
}

#[test]
fn linear_ramp_matches_2a_and_closed_form() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Ramp", "L", Some(synth)).unwrap();
    let ticks: Vec<i64> = (0..32).map(|i| i * Q / 2).collect();
    let notes: Vec<(i64, u8, u16, i64)> = ticks.iter().map(|t| (*t, 60, 0x8000, Q / 4)).collect();
    b.notes(seq, lane, &notes).unwrap();
    b.tempo(seq, &[(0, 120, true), (4 * BAR, 60, false)])
        .unwrap();
    let doc = b.finish();
    let s = compile_seq(&doc, seq);
    let tempo = &doc.sequence(seq).unwrap().tempo;
    let frames = on_frames(&s);
    assert_eq!(frames.len(), ticks.len());
    for (t, f) in ticks.iter().zip(&frames) {
        let via_2a = (tempo.tick_to_seconds(Tick(*t)) * SR).round() as u64;
        assert_eq!(*f, via_2a, "tick {t}");
        let closed = ramp_seconds(*t as f64, (4 * BAR) as f64, 120.0, 60.0) * SR;
        assert!(
            (*f as f64 - closed).abs() <= 1.0,
            "tick {t}: {f} vs {closed}"
        );
    }
    // The ramp takes ln(2)·16 beats·0.5 s/beat·2 = 16·ln 2 seconds.
    let end = s.tempo.tick_to_frame(Tick(4 * BAR));
    let expected = 16.0 * std::f64::consts::LN_2 * SR;
    assert!((end as f64 - expected).abs() <= 1.0, "{end} vs {expected}");
}

#[test]
fn nested_sequence_uses_the_parents_tempo() {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (inner, il) = b.sequence("Inner", "L", Some(synth)).unwrap();
    b.notes(inner, il, &[(0, 60, 0x8000, Q / 2), (Q, 62, 0x8000, Q / 2)])
        .unwrap();
    b.tempo(inner, &[(0, 60, false)]).unwrap();
    let (outer, ol) = b.sequence("Outer", "Refs", None).unwrap();
    b.reference(outer, ol, BAR, inner, 1, 0).unwrap();
    b.tempo(outer, &[(0, 120, false)]).unwrap();
    let doc = b.finish();
    let s = compile_seq(&doc, outer);
    // Bar 2 at 120 BPM is 2 s; the inner quarter is 0.5 s at the parent's
    // tempo, not 1 s at the inner 60 BPM.
    assert_eq!(on_frames(&s), vec![96_000, 120_000]);
}

fn arb_tempo() -> impl Strategy<Value = TempoMap> {
    prop::collection::vec((1i64..4 * BAR, 5_000u32..=999_000, any::<bool>()), 0..6).prop_map(
        |steps| {
            let mut points = vec![];
            let mut tick = 0i64;
            let mut first = true;
            for (gap, bpm, ramp) in steps {
                if !first {
                    tick += gap;
                }
                first = false;
                points.push(TempoPoint {
                    tick: Tick(tick),
                    bpm: Bpm(bpm),
                    ramp,
                });
            }
            if points.is_empty() {
                points.push(TempoPoint {
                    tick: Tick(0),
                    bpm: Bpm(120_000),
                    ramp: false,
                });
            }
            TempoMap::new(points).unwrap()
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn compiled_frames_match_2a(tempo in arb_tempo(), ticks in prop::collection::vec(0i64..16 * BAR, 1..40)) {
        let mut b = Builder::new();
        let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
        let (seq, lane) = b.sequence("P", "L", Some(synth)).unwrap();
        let notes: Vec<(i64, u8, u16, i64)> = ticks.iter().map(|t| (*t, 60, 0x8000, 1)).collect();
        b.notes(seq, lane, &notes).unwrap();
        b.run(core_model::command::Command::Sequence(core_model::command::SequenceCmd::SetTempo {
            seq,
            tempo: tempo.clone(),
        }))
        .unwrap();
        let doc = b.finish();
        let s = compile_seq(&doc, seq);
        let mut want: Vec<u64> = ticks.iter().map(|t| (tempo.tick_to_seconds(Tick(*t)) * SR).round() as u64).collect();
        want.sort_unstable();
        let got = on_frames(&s);
        prop_assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(&want) {
            prop_assert!(g.abs_diff(*w) <= 1, "{} vs {}", g, w);
        }
    }
}
