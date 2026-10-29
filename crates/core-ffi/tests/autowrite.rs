//! The automation writer's golden tests (plan 5e-1 Task 7; sub-project 5
//! spec §12.3 "Automation: golden tests per mode from scripted gesture
//! sequences (touch, values and release at known frames)"): Touch's glide,
//! Latch until Stop and at a wrap, Write's held controls and After Write,
//! Trim on a ramp, stepped targets, thinning bounds, edge points, latency,
//! a jump inside a pass, the split at the model's limit.

use core_engine::gesture::{GesturePhase, Stamped};
use core_engine::record::Mark;
use core_engine::schedule::{AutoDest, StripParam, TempoTable};
use core_model::automation::{AutomationTarget, fader_db, fader_norm};
use core_model::command::{Command, EventCmd, RecordPass, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{LaneId, SeqId, StripId};
use core_model::time::{PPQ, Tick};
use vizdaw_ffi::autowrite::{
    AutoCapture, TargetWrite, Thinning, WriteMode, thin, to_model, writes,
};

const S: StripId = StripId(1);
const PAN: AutomationTarget = AutomationTarget::StripPan(S);
const FADER: AutomationTarget = AutomationTarget::StripFader(S);
const MUTE: AutomationTarget = AutomationTarget::StripMute(S);

fn roll(frame: u64, latency: u64) -> Mark {
    Mark {
        run: 0,
        frame,
        wrap: false,
        latency,
    }
}

fn wrap(run: u64, to: u64) -> Mark {
    Mark {
        run,
        frame: to,
        wrap: true,
        latency: 0,
    }
}

fn dest(t: AutomationTarget) -> AutoDest {
    match t {
        AutomationTarget::StripPan(s) => AutoDest::Strip(s, StripParam::Pan),
        AutomationTarget::StripFader(s) => AutoDest::Strip(s, StripParam::Fader),
        AutomationTarget::StripMute(s) => AutoDest::Strip(s, StripParam::Mute),
        _ => unreachable!(),
    }
}

fn st(t: AutomationTarget, run: i64, phase: GesturePhase, mode: WriteMode) -> Stamped {
    Stamped {
        dest: dest(t),
        run,
        phase,
        tag: mode.tag(),
    }
}

/// Runs the writer with existing automation `existing` (constant 0.5 by
/// default), points `points`, static value 0.3, Medium thinning, After
/// Write Latch.
fn run(
    marks: &[Mark],
    end_run: u64,
    stamps: &[Stamped],
    holds: &[(AutomationTarget, f64)],
    existing: &dyn Fn(AutomationTarget, u64) -> Option<f64>,
    points: &dyn Fn(AutomationTarget, u64, u64) -> Vec<u64>,
) -> Vec<TargetWrite> {
    run_after(
        Some(WriteMode::Latch),
        marks,
        end_run,
        stamps,
        holds,
        existing,
        points,
    )
}

/// [`run`] with After Write `after_write`.
fn run_after(
    after_write: Option<WriteMode>,
    marks: &[Mark],
    end_run: u64,
    stamps: &[Stamped],
    holds: &[(AutomationTarget, f64)],
    existing: &dyn Fn(AutomationTarget, u64) -> Option<f64>,
    points: &dyn Fn(AutomationTarget, u64, u64) -> Vec<u64>,
) -> Vec<TargetWrite> {
    writes(&AutoCapture {
        marks,
        end_run,
        stamps,
        holds,
        thinning: Thinning::Medium,
        after_write,
        stepped: &|t| t.is_stepped(),
        existing,
        existing_points: points,
        fixed: &|_| 0.3,
    })
}

fn half(_: AutomationTarget, _: u64) -> Option<f64> {
    Some(0.5)
}

fn no_points(_: AutomationTarget, _: u64, _: u64) -> Vec<u64> {
    Vec::new()
}

fn close(got: &[(u64, f64)], want: &[(u64, f64)]) {
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            g.0 == w.0 && (g.1 - w.1).abs() < 1e-9,
            "{got:?} vs {want:?}"
        );
    }
}

/// Spec §7.3 Touch: from touch to release, then a straight ramp back to
/// the existing automation over the return time (12 000 frames = 250 ms).
#[test]
fn touch_writes_from_touch_to_release_and_glides_back() {
    let mut stamps = vec![st(PAN, 4800, GesturePhase::Touch(0.8), WriteMode::Touch)];
    for k in 1..=10 {
        let r = 4800 + 480 * k;
        stamps.push(st(
            PAN,
            r,
            GesturePhase::Value(0.8 + 0.01 * k as f64),
            WriteMode::Touch,
        ));
    }
    stamps.push(st(PAN, 9600, GesturePhase::Glide(12_000), WriteMode::Touch));
    let w = run(&[roll(48_000, 0)], 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 1);
    assert_eq!((w[0].target, w[0].pass), (PAN, 0));
    close(&w[0].points, &[(52_800, 0.8), (57_600, 0.9), (69_600, 0.5)]);
}

/// Latch: from touch to the pass's end holding the last value; a loop
/// wrap ends it and the next pass writes nothing.
#[test]
fn latch_holds_to_the_end_of_its_pass() {
    let stamps = [
        st(PAN, 48_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 50_000, GesturePhase::Value(0.75), WriteMode::Latch),
        st(PAN, 60_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(
        &[roll(0, 0), wrap(96_000, 0)],
        150_000,
        &stamps,
        &[],
        &half,
        &no_points,
    );
    assert_eq!(w.len(), 1, "{w:?}");
    close(
        &w[0].points,
        &[(48_000, 0.7), (50_000, 0.75), (96_000, 0.75)],
    );
    // Without the wrap it lasts until Stop.
    let w = run(&[roll(0, 0)], 150_000, &stamps, &[], &half, &no_points);
    close(
        &w[0].points,
        &[(48_000, 0.7), (50_000, 0.75), (150_000, 0.75)],
    );
}

/// Review Focus 2, Ruling 10: a target still touched at a wrap writes to
/// the end of that pass and again from the next pass's start.
#[test]
fn a_target_held_over_a_wrap_writes_both_passes() {
    let stamps = [
        st(PAN, 80_000, GesturePhase::Touch(0.6), WriteMode::Latch),
        st(PAN, 100_000, GesturePhase::Value(0.65), WriteMode::Latch),
        st(PAN, 110_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(
        &[roll(0, 0), wrap(96_000, 0)],
        150_000,
        &stamps,
        &[],
        &half,
        &no_points,
    );
    assert_eq!(w.iter().map(|x| x.pass).collect::<Vec<_>>(), vec![0, 1]);
    close(&w[0].points, &[(80_000, 0.6), (96_000, 0.6)]);
    close(&w[1].points, &[(0, 0.6), (4_000, 0.65), (54_000, 0.65)]);
}

/// Write (Ruling 9): a held control writes from the roll at its value,
/// a touch changes it from then on, held to the pass's end; after a wrap
/// (After Write) an untouched control writes nothing.
#[test]
fn write_writes_held_controls_from_the_roll() {
    let stamps = [
        st(PAN, 24_000, GesturePhase::Touch(0.9), WriteMode::Write),
        st(PAN, 30_000, GesturePhase::Hold, WriteMode::Write),
    ];
    let holds = [(PAN, 0.6)];
    let w = run(&[roll(0, 0)], 96_000, &stamps, &holds, &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    close(
        &w[0].points,
        &[(0, 0.6), (23_999, 0.6), (24_000, 0.9), (96_000, 0.9)],
    );
    let w = run(
        &[roll(0, 0), wrap(48_000, 0)],
        96_000,
        &stamps,
        &holds,
        &half,
        &no_points,
    );
    assert_eq!(w.len(), 1, "only the first pass: {w:?}");
    close(
        &w[0].points,
        &[(0, 0.6), (23_999, 0.6), (24_000, 0.9), (48_000, 0.9)],
    );
    // Untouched: the held value alone.
    let w = run(&[roll(0, 0)], 96_000, &[], &holds, &half, &no_points);
    close(&w[0].points, &[(0, 0.6), (96_000, 0.6)]);
}

/// Pre-flight M3, Ruling 9: a Write strip's touch after a wrap is written
/// in the After Write mode (Latch: held to the pass's end); with After
/// Write Off or Read it writes nothing.
#[test]
fn write_stamps_after_a_wrap_take_the_after_write_mode() {
    let stamps = [
        st(PAN, 120_000, GesturePhase::Touch(0.4), WriteMode::Write),
        st(PAN, 130_000, GesturePhase::Hold, WriteMode::Write),
    ];
    let marks = [roll(0, 0), wrap(96_000, 0)];
    let w = run(&marks, 150_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].pass, 1);
    close(&w[0].points, &[(24_000, 0.4), (54_000, 0.4)]);
    assert!(run_after(None, &marks, 150_000, &stamps, &[], &half, &no_points).is_empty());
}

/// Ruling 23 (pre-flight C2): a mark that jumps inside a pass (a resync
/// whose frame does not continue the piece) ends the open segment where
/// the earlier piece ended and reopens it at the new piece's start while
/// the target is still touched, so no write spans the jump; a held
/// (released) segment ends there, as at a wrap.
#[test]
fn a_jump_inside_a_pass_breaks_the_segment() {
    let jump = Mark {
        run: 48_000,
        frame: 10_000,
        wrap: false,
        latency: 0,
    };
    let stamps = [
        st(PAN, 40_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 50_000, GesturePhase::Value(0.8), WriteMode::Latch),
        st(PAN, 60_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), jump], 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w.iter().all(|x| x.pass == 0 && x.target == PAN));
    close(&w[0].points, &[(40_000, 0.7), (48_000, 0.7)]);
    close(&w[1].points, &[(10_000, 0.7), (12_000, 0.8), (58_000, 0.8)]);
    assert!(
        w.iter()
            .all(|x| x.points.windows(2).all(|p| p[0].0 < p[1].0)),
        "frames rise in each write"
    );
    // Released (held) before the jump: it ends there and does not reopen.
    let held = [
        st(PAN, 40_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 41_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), jump], 96_000, &held, &[], &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    close(&w[0].points, &[(40_000, 0.7), (48_000, 0.7)]);
}

/// Trim (Ruling 11): +6 dB over a ramp from −20 dB to 0 dB keeps the
/// ramp's shape: the existing point at 48 000 is moved in place, the
/// offset glides back to 0 over the return time.
#[test]
fn trim_adds_its_offset_to_an_existing_ramp() {
    let ramp = |_: AutomationTarget, f: u64| Some(fader_norm(-20.0 + 20.0 * f as f64 / 96_000.0));
    let pts = |_: AutomationTarget, a: u64, b: u64| -> Vec<u64> {
        [0, 48_000, 96_000]
            .into_iter()
            .filter(|f| (a..=b).contains(f))
            .collect()
    };
    let mut stamps = vec![st(
        FADER,
        24_000,
        GesturePhase::Touch(ramp(FADER, 24_000).unwrap()),
        WriteMode::Trim,
    )];
    for r in (24_000..=72_000).step_by(4_800) {
        stamps.push(st(FADER, r, GesturePhase::Offset(6.0), WriteMode::Trim));
    }
    stamps.push(st(
        FADER,
        72_000,
        GesturePhase::Glide(12_000),
        WriteMode::Trim,
    ));
    let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &ramp, &pts);
    assert_eq!(w.len(), 1);
    let got: Vec<(u64, f64)> = w[0]
        .points
        .iter()
        .map(|(f, v)| (*f, fader_db(*v)))
        .collect();
    let want = [
        (24_000, -9.0),
        (48_000, -4.0),
        (72_000, 1.0),
        (84_000, -2.5),
    ];
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, x) in got.iter().zip(want) {
        assert!(g.0 == x.0 && (g.1 - x.1).abs() < 1e-6, "{got:?}");
    }
}

/// Spec §7.3 stepped targets keep only their changes (and the last
/// sample); in Touch a click is held (Ruling 10's Hold).
#[test]
fn stepped_targets_keep_only_their_changes() {
    let stamps = [
        st(MUTE, 12_000, GesturePhase::Touch(1.0), WriteMode::Touch),
        st(MUTE, 12_480, GesturePhase::Value(1.0), WriteMode::Touch),
        st(MUTE, 12_960, GesturePhase::Value(1.0), WriteMode::Touch),
        st(MUTE, 13_000, GesturePhase::Hold, WriteMode::Touch),
    ];
    let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &half, &no_points);
    close(&w[0].points, &[(12_000, 1.0), (96_000, 1.0)]);
}

/// Ruling 12: every thinned-away sample lies within the tolerance of the
/// line through the kept ones (in dB for faders), and points go.
#[test]
fn thinning_stays_within_its_tolerance() {
    let db: Vec<(u64, f64)> = (0..400u64)
        .map(|k| (k * 480, fader_norm(-10.0 + 10.0 * (k as f64 / 23.0).sin())))
        .collect();
    let pan: Vec<(u64, f64)> = (0..400u64)
        .map(|k| (k * 480, 0.5 + 0.4 * (k as f64 / 31.0).sin()))
        .collect();
    for th in [Thinning::Low, Thinning::Medium, Thinning::High] {
        for (samples, target, in_db) in [(&db, FADER, true), (&pan, PAN, false)] {
            let tol = th.tolerance(target);
            let kept = thin(samples, tol, in_db, false);
            assert!(
                kept.len() < samples.len() / 2,
                "{th:?} {target:?}: {}",
                kept.len()
            );
            assert_eq!(kept.first(), samples.first());
            assert_eq!(kept.last(), samples.last());
            let y = |v: f64| if in_db { fader_db(v) } else { v };
            for (f, v) in samples.iter() {
                let i = kept.partition_point(|k| k.0 <= *f);
                let (a, b) = (kept[i.saturating_sub(1)], kept[i.min(kept.len() - 1)]);
                let line = if b.0 == a.0 {
                    y(a.1)
                } else {
                    y(a.1) + (y(b.1) - y(a.1)) * (*f - a.0) as f64 / (b.0 - a.0) as f64
                };
                assert!(
                    (y(*v) - line).abs() <= tol + 1e-9,
                    "{th:?} {target:?} at {f}"
                );
            }
        }
    }
    assert!((Thinning::Medium.tolerance(FADER) - 0.1).abs() < 1e-12);
    assert!((Thinning::Medium.tolerance(PAN) - 0.0025).abs() < 1e-12);
    assert!((Thinning::High.tolerance(PAN) - 0.005).abs() < 1e-12);
    assert!((Thinning::Low.tolerance(FADER) - 0.04).abs() < 1e-12);
}

/// Ruling 12 (pre-flight M7): on equal maximum distance the earliest
/// index splits, so the result is deterministic.
#[test]
fn thinning_splits_the_earliest_of_equal_distances() {
    let samples = [(0, 0.0), (100, 1.0), (200, 1.0), (300, 0.0)];
    assert_eq!(
        thin(&samples, 0.6, false, false),
        vec![(0, 0.0), (100, 1.0), (300, 0.0)]
    );
}

/// Ruling 7: stamps are heard the graph latency after their run, as
/// recorded MIDI is.
#[test]
fn latency_moves_stamps_to_the_heard_position() {
    let stamps = [
        st(PAN, 9_600, GesturePhase::Touch(0.2), WriteMode::Touch),
        st(PAN, 14_400, GesturePhase::Release, WriteMode::Touch),
    ];
    let w = run(
        &[roll(48_000, 4_800)],
        96_000,
        &stamps,
        &[],
        &half,
        &no_points,
    );
    close(&w[0].points, &[(52_800, 0.2), (57_600, 0.2)]);
    // Stamps without a write mode (the static path) write nothing.
    let mut quiet = stamps;
    for s in quiet.iter_mut() {
        s.tag = 0;
    }
    assert!(run(&[roll(48_000, 0)], 96_000, &quiet, &[], &half, &no_points).is_empty());
    assert_eq!(
        WriteMode::of_tag(WriteMode::Trim.tag()),
        Some(WriteMode::Trim)
    );
    assert_eq!(WriteMode::of_tag(0), None);
}

/// The lane rule of Plan 4d (linear; first value before the first point).
fn lane_value(points: &[(i64, f64)], t: i64) -> f64 {
    let n = points.partition_point(|p| p.0 <= t);
    match (n.checked_sub(1).map(|i| points[i]), points.get(n)) {
        (None, Some(b)) => b.1,
        (Some(a), Some(b)) if b.0 > a.0 => {
            a.1 + (b.1 - a.1) * (t - a.0) as f64 / (b.0 - a.0) as f64
        }
        (Some(a), _) => a.1,
        (None, None) => 0.0,
    }
}

/// Spec §7.3, Ruling 13: a point one tick beyond each end holds the value
/// the target had there, so every value outside the written range is what
/// it was; with no automation, the static value.
#[test]
fn edge_points_leave_the_values_outside_unchanged() {
    let q = PPQ;
    let doc = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let master = doc.mixer().master();
    let target = AutomationTarget::StripFader(master);
    let before = [(0, 0.2), (8 * q, 0.8)];
    let doc = apply(
        &doc,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: before
                .iter()
                .map(|(t, v)| EventSpec::new(Tick(*t), EventBody::Automation { target, value: *v }))
                .collect(),
        }),
    )
    .unwrap()
    .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    // Beats 2 to 4 (24 000 frames a beat at 120 BPM).
    let tw = TargetWrite {
        target,
        pass: 0,
        points: vec![(48_000, 0.9), (96_000, 0.9)],
    };
    let outside = |_: AutomationTarget, t: Tick| lane_value(&before, t.0);
    let model = to_model(
        std::slice::from_ref(&tw),
        SeqId(0),
        &tempo,
        &outside,
        &|_| "Master Fader".into(),
        &|t| t.is_stepped(),
    );
    assert_eq!(model.len(), 1);
    let w = &model[0];
    assert_eq!((w.start, w.end), (Tick(2 * q - 1), Tick(4 * q + 1)));
    assert_eq!(w.points.first().unwrap().0, Tick(2 * q - 1));
    assert_eq!(w.points.last().unwrap().0, Tick(4 * q + 1));
    let out = apply(
        &doc,
        &Command::Record(RecordPass {
            automation: model,
            ..RecordPass::default()
        }),
    )
    .unwrap()
    .doc;
    let mut after: Vec<(i64, f64)> = out
        .lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Automation { value, .. } => Some((e.tick.0, value)),
            _ => None,
        })
        .collect();
    after.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for t in [0, q, 2 * q - 1, 4 * q + 1, 6 * q, 8 * q, 9 * q] {
        assert!(
            (lane_value(&after, t) - lane_value(&before, t)).abs() < 1e-12,
            "at {t}: {after:?}"
        );
    }
    assert!((lane_value(&after, 3 * q) - 0.9).abs() < 1e-12);
    // No automation: the edges hold the static value.
    let model = to_model(
        &[tw],
        SeqId(0),
        &tempo,
        &|_, _| 0.3,
        &|_| "Master Fader".into(),
        &|t| t.is_stepped(),
    );
    let p = &model[0].points;
    assert!(p.first().unwrap().1 == 0.3 && p.last().unwrap().1 == 0.3);
}

/// Ruling 25 (pre-flight I6): a write longer than the model's limit is
/// split into consecutive writes the model takes: none over the limit,
/// ranges apart, edges only at the outer ends, no point lost.
#[test]
fn a_write_longer_than_the_model_limit_is_split() {
    use core_model::command::MAX_WRITE_POINTS;
    let doc = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    // 4 frames apart is 2.56 ticks at 120 BPM: no two share a tick.
    let input: Vec<(u64, f64)> = (0..250_000u64)
        .map(|k| (k * 4, if k % 2 == 0 { 0.2 } else { 0.8 }))
        .collect();
    let tw = TargetWrite {
        target: PAN,
        pass: 0,
        points: input.clone(),
    };
    let model = to_model(
        &[tw],
        SeqId(0),
        &tempo,
        &|_, _| 0.3,
        &|_| "Pan".into(),
        &|t| t.is_stepped(),
    );
    assert!(model.len() >= 3, "{}", model.len());
    for w in &model {
        assert!(w.points.len() <= MAX_WRITE_POINTS, "{}", w.points.len());
        assert!(w.points.iter().all(|(t, _)| *t >= w.start && *t <= w.end));
    }
    assert!(
        model.windows(2).all(|p| p[0].end.0 + 1 == p[1].start.0),
        "the ranges are contiguous and disjoint"
    );
    let all: Vec<(Tick, f64)> = model
        .iter()
        .flat_map(|w| w.points.iter().copied())
        .collect();
    assert_eq!(all.len(), input.len() + 2, "the input and the two edges");
    assert!(all.windows(2).all(|p| p[0].0 < p[1].0), "ticks rise");
    assert!(
        all[0].1 == 0.3 && all[all.len() - 1].1 == 0.3,
        "the edges hold the value outside"
    );
    let inner: Vec<f64> = all[1..all.len() - 1].iter().map(|p| p.1).collect();
    assert_eq!(inner, input.iter().map(|p| p.1).collect::<Vec<_>>());
}

/// Task 6 review m2: a stamp in the first `arrival` frames after the roll
/// places before the roll's frame (below 0 when the roll's frame is less
/// than the latency); it is clipped to the pass's start, never before it.
#[test]
fn stamps_before_the_pass_start_are_clipped_to_it() {
    let stamps = [
        st(PAN, 1_000, GesturePhase::Touch(0.4), WriteMode::Latch),
        st(PAN, 5_000, GesturePhase::Value(0.5), WriteMode::Latch),
        st(PAN, 6_000, GesturePhase::Value(0.6), WriteMode::Latch),
        st(PAN, 7_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    // Roll frame 1 000 < latency 4 800: the touch would place at −2 800.
    let w = run(
        &[roll(1_000, 4_800)],
        20_000,
        &stamps,
        &[],
        &half,
        &no_points,
    );
    assert_eq!(w.len(), 1, "{w:?}");
    close(
        &w[0].points,
        &[(1_000, 0.4), (1_200, 0.5), (2_200, 0.6), (16_200, 0.6)],
    );
    // Roll frame 10 000: the touch would place at 6 200, before the pass.
    let w = run(
        &[roll(10_000, 4_800)],
        20_000,
        &stamps,
        &[],
        &half,
        &no_points,
    );
    close(
        &w[0].points,
        &[(10_000, 0.4), (10_200, 0.5), (11_200, 0.6), (25_200, 0.6)],
    );
    // Two stamps clipped to one frame: the later wins.
    let early = [
        st(PAN, 1_000, GesturePhase::Touch(0.4), WriteMode::Latch),
        st(PAN, 2_000, GesturePhase::Value(0.45), WriteMode::Latch),
        st(PAN, 6_000, GesturePhase::Value(0.6), WriteMode::Latch),
        st(PAN, 7_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(
        &[roll(1_000, 4_800)],
        20_000,
        &early,
        &[],
        &half,
        &no_points,
    );
    close(&w[0].points, &[(1_000, 0.45), (2_200, 0.6), (16_200, 0.6)]);
}

/// Task 6 review m4: a stamp taken just after a resync whose run lies
/// before the resync's mark (its strip was still rendering the old piece)
/// is written in the piece before the jump, where its strip read it; no
/// write's frames go back.
#[test]
fn a_stamp_before_a_jumps_mark_stays_in_the_piece_before() {
    let jump = Mark {
        run: 48_000,
        frame: 10_000,
        wrap: false,
        latency: 0,
    };
    let stamps = [
        st(PAN, 40_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 47_000, GesturePhase::Value(0.8), WriteMode::Latch),
        st(PAN, 50_000, GesturePhase::Value(0.9), WriteMode::Latch),
        st(PAN, 60_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), jump], 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 2, "{w:?}");
    close(&w[0].points, &[(40_000, 0.7), (47_000, 0.8), (48_000, 0.8)]);
    close(&w[1].points, &[(10_000, 0.8), (12_000, 0.9), (58_000, 0.9)]);
}

/// Task 7 review I1: values are written as the engine plays them: a NaN
/// or out-of-range value is clamped (`unit`), a non-finite offset is 0,
/// and neither collapses the thinning; the model takes the write.
#[test]
fn hostile_values_are_written_as_the_engine_plays_them() {
    let stamps = [
        st(PAN, 1_000, GesturePhase::Touch(0.6), WriteMode::Latch),
        st(PAN, 10_000, GesturePhase::Value(f64::NAN), WriteMode::Latch),
        st(PAN, 20_000, GesturePhase::Value(0.1), WriteMode::Latch),
        st(PAN, 30_000, GesturePhase::Value(0.9), WriteMode::Latch),
        st(PAN, 40_000, GesturePhase::Value(1.2), WriteMode::Latch),
        st(PAN, 50_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    close(
        &w[0].points,
        &[
            (1_000, 0.6),
            (10_000, 0.0),
            (20_000, 0.1),
            (30_000, 0.9),
            (40_000, 1.0),
            (96_000, 1.0),
        ],
    );
    let doc = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    let target = AutomationTarget::StripPan(doc.mixer().master());
    let tw = TargetWrite {
        target,
        pass: 0,
        points: w[0].points.clone(),
    };
    let model = to_model(
        &[tw],
        SeqId(0),
        &tempo,
        &|_, _| 0.5,
        &|_| "Pan".into(),
        &|t| t.is_stepped(),
    );
    assert!(
        apply(
            &doc,
            &Command::Record(RecordPass {
                automation: model,
                ..RecordPass::default()
            })
        )
        .is_ok()
    );
    // A held value is clamped too (re-review nit).
    let w = run(
        &[roll(0, 0)],
        96_000,
        &[],
        &[(PAN, f64::NAN), (FADER, 1.5)],
        &half,
        &no_points,
    );
    assert!(
        w.iter()
            .all(|x| x.points.iter().all(|p| (0.0..=1.0).contains(&p.1))),
        "{w:?}"
    );
    // A non-finite Trim offset is 0.
    let trim = [
        st(FADER, 1_000, GesturePhase::Touch(0.5), WriteMode::Trim),
        st(
            FADER,
            2_000,
            GesturePhase::Offset(f64::INFINITY),
            WriteMode::Trim,
        ),
        st(FADER, 3_000, GesturePhase::Release, WriteMode::Trim),
    ];
    let w = run(&[roll(0, 0)], 96_000, &trim, &[], &half, &no_points);
    assert!(
        w[0].points.iter().all(|p| (p.1 - 0.5).abs() < 1e-9),
        "{w:?}"
    );
}

/// Task 7 review I2: a return time of 0 holds the value up to the release
/// and steps there, as the engine plays it; it is no ramp.
#[test]
fn a_glide_of_zero_holds_then_steps() {
    let stamps = [
        st(PAN, 1_000, GesturePhase::Touch(0.6), WriteMode::Touch),
        st(PAN, 10_000, GesturePhase::Value(0.9), WriteMode::Touch),
        st(PAN, 40_000, GesturePhase::Glide(0), WriteMode::Touch),
    ];
    let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &half, &no_points);
    close(
        &w[0].points,
        &[(1_000, 0.6), (10_000, 0.9), (40_000, 0.9), (40_001, 0.5)],
    );
}

/// Task 7 review I3: a later write of a target that starts inside an
/// earlier one of the same capture (a re-grab during a Touch glide, a
/// jump back shorter than the segment) takes its start edge from the
/// earlier write, not from the value before the pass: no notch.
#[test]
fn an_overlapping_write_takes_its_edge_from_the_earlier_one() {
    let doc = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    let stamps = [
        st(PAN, 10_000, GesturePhase::Touch(0.9), WriteMode::Touch),
        st(PAN, 20_000, GesturePhase::Glide(12_000), WriteMode::Touch),
        st(PAN, 26_000, GesturePhase::Touch(0.7), WriteMode::Touch),
        st(PAN, 26_500, GesturePhase::Value(0.75), WriteMode::Touch),
        st(PAN, 30_000, GesturePhase::Release, WriteMode::Touch),
    ];
    let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 2, "{w:?}");
    let model = to_model(&w, SeqId(0), &tempo, &|_, _| 0.1, &|_| "Pan".into(), &|t| {
        t.is_stepped()
    });
    assert_eq!(model.len(), 2);
    let edge = model[1].points[0];
    assert_eq!(edge.0, model[1].start);
    // The glide 0.9 → 0.5 over 20 000–32 000 is at 0.7 at 26 000.
    assert!((edge.1 - 0.7).abs() < 1e-3, "{model:?}");
    // Its end edge (30 000) is inside the glide too: 0.9 − 0.4 × 10/12.
    assert!(
        (model[1].points.last().unwrap().1 - (0.9 - 0.4 * 10.0 / 12.0)).abs() < 1e-3,
        "{model:?}"
    );
    // The first write's own edges are outside every earlier write.
    assert_eq!(model[0].points.first().unwrap().1, 0.1);
    assert_eq!(model[0].points.last().unwrap().1, 0.1);
    // A jump back into the segment: the reopened write starts inside it.
    let jump = Mark {
        run: 48_000,
        frame: 44_000,
        wrap: false,
        latency: 0,
    };
    let held = [
        st(PAN, 40_000, GesturePhase::Touch(0.7), WriteMode::Latch),
        st(PAN, 48_000, GesturePhase::Value(0.8), WriteMode::Latch),
        st(PAN, 49_000, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&[roll(0, 0), jump], 60_000, &held, &[], &half, &no_points);
    assert_eq!(w.len(), 2, "{w:?}");
    let model = to_model(&w, SeqId(0), &tempo, &|_, _| 0.1, &|_| "Pan".into(), &|t| {
        t.is_stepped()
    });
    assert!((model[1].points[0].1 - 0.7).abs() < 1e-9, "{model:?}");
}

/// Task 7 review m1: a release in runs no piece covers (a latency rise's
/// catch region a quick fall cut away) still ends its segment, at the end
/// of the piece before, instead of leaving it to run on as a Latch.
#[test]
fn a_release_no_piece_covers_still_ends_its_segment() {
    let marks = [
        roll(0, 0),
        Mark {
            run: 100,
            frame: 100,
            wrap: false,
            latency: 1_000,
        },
        Mark {
            run: 200,
            frame: 200,
            wrap: false,
            latency: 0,
        },
    ];
    let stamps = [
        st(PAN, 20, GesturePhase::Touch(0.4), WriteMode::Touch),
        st(PAN, 60, GesturePhase::Value(0.6), WriteMode::Touch),
        st(PAN, 150, GesturePhase::Release, WriteMode::Touch),
    ];
    let w = run(&marks, 96_000, &stamps, &[], &half, &no_points);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].points.last().unwrap().0 <= 200, "{w:?}");
    // Re-review N2: a Latch's Hold in the gap goes to the start of the
    // piece after it, so the Latch reopens there and holds to the end.
    let latch = [
        st(PAN, 20, GesturePhase::Touch(0.4), WriteMode::Latch),
        st(PAN, 60, GesturePhase::Value(0.6), WriteMode::Latch),
        st(PAN, 150, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&marks, 96_000, &latch, &[], &half, &no_points);
    assert_eq!(w.len(), 2, "{w:?}");
    close(&w[0].points, &[(20, 0.4), (60, 0.6), (100, 0.6)]);
    close(&w[1].points, &[(200, 0.6), (96_000, 0.6)]);
    // The same over a wrap: held to the next pass's end.
    let marks = [
        roll(0, 0),
        Mark {
            run: 1_000,
            frame: 0,
            wrap: true,
            latency: 1_000,
        },
        Mark {
            run: 1_100,
            frame: 100,
            wrap: false,
            latency: 0,
        },
    ];
    let latch = [
        st(PAN, 500, GesturePhase::Touch(0.4), WriteMode::Latch),
        st(PAN, 600, GesturePhase::Value(0.6), WriteMode::Latch),
        st(PAN, 1_050, GesturePhase::Hold, WriteMode::Latch),
    ];
    let w = run(&marks, 96_000, &latch, &[], &half, &no_points);
    assert_eq!(
        w.iter().map(|x| x.pass).collect::<Vec<_>>(),
        vec![0, 1],
        "{w:?}"
    );
    close(&w[1].points, &[(100, 0.6), (95_000, 0.6)]);
}

/// Re-review I2 and N1: a hold then a step on adjacent frames (a glide of
/// 0, Write's hold merge) survives the frames' rounding to ticks: a
/// point rounding onto the tick before goes to the next tick, so the
/// model holds and then steps instead of ramping.
#[test]
fn a_step_survives_the_tick_rounding() {
    let doc = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    let model_of = |w: &[TargetWrite]| {
        let m = to_model(w, SeqId(0), &tempo, &|_, _| 0.5, &|_| "Pan".into(), &|t| {
            t.is_stepped()
        });
        assert_eq!(m.len(), 1);
        let p: Vec<(i64, f64)> = m[0].points.iter().map(|(t, v)| (t.0, *v)).collect();
        assert!(p.windows(2).all(|x| x[0].0 < x[1].0), "ticks rise: {p:?}");
        p
    };
    let mut collided = 0;
    for g in 40_000..40_010u64 {
        collided += usize::from(tempo.frame_to_tick(g) == tempo.frame_to_tick(g + 1));
        let stamps = [
            st(PAN, 1_000, GesturePhase::Touch(0.6), WriteMode::Touch),
            st(PAN, 10_000, GesturePhase::Value(0.9), WriteMode::Touch),
            st(PAN, g as i64, GesturePhase::Glide(0), WriteMode::Touch),
        ];
        let w = run(&[roll(0, 0)], 96_000, &stamps, &[], &half, &no_points);
        let p = model_of(&w);
        let held = tempo.frame_to_tick(g).0;
        assert!((lane_value(&p, held) - 0.9).abs() < 1e-12, "g={g}: {p:?}");
        assert!(
            (lane_value(&p, tempo.frame_to_tick(30_000).0) - 0.9).abs() < 1e-12,
            "g={g}: {p:?}"
        );
        assert!(
            (lane_value(&p, held + 1) - 0.5).abs() < 1e-12,
            "g={g}: {p:?}"
        );
    }
    assert!(collided > 0, "some frame pair shares a tick");
    for f in 48_000..48_010u64 {
        let stamps = [
            st(PAN, f as i64, GesturePhase::Touch(0.8), WriteMode::Write),
            st(PAN, f as i64 + 5_000, GesturePhase::Hold, WriteMode::Write),
        ];
        let w = run(
            &[roll(0, 0)],
            96_000,
            &stamps,
            &[(PAN, 0.2)],
            &half,
            &no_points,
        );
        let p = model_of(&w);
        let before = tempo.frame_to_tick(f - 1).0;
        assert!((lane_value(&p, before) - 0.2).abs() < 1e-12, "f={f}: {p:?}");
        assert!(
            (lane_value(&p, tempo.frame_to_tick(30_000).0) - 0.2).abs() < 1e-12,
            "f={f}: {p:?}"
        );
        assert!(
            (lane_value(&p, before + 1) - 0.8).abs() < 1e-12,
            "f={f}: {p:?}"
        );
    }
    // No room for the bump (the next point already owns the next tick):
    // the later value wins on its tick, as before.
    let tw = TargetWrite {
        target: PAN,
        pass: 0,
        points: vec![(0, 0.1), (1, 0.2), (2, 0.3), (3, 0.4)],
    };
    let ticks: Vec<i64> = (0..4).map(|f| tempo.frame_to_tick(f).0).collect();
    assert_eq!(ticks, vec![0, 1, 1, 2]);
    let p = model_of(&[tw]);
    assert_eq!(p[1..p.len() - 1], [(0, 0.1), (1, 0.3), (2, 0.4)]);
}

/// Re-review N3: `to_model` takes the target's stepped flag (a plugin's
/// stepped parameter too): an overlap edge takes the earlier write's
/// step, not an interpolation, and a step on a shared tick is one point.
#[test]
fn to_model_steps_stepped_targets() {
    let doc = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let s = doc.sequence(SeqId(0)).unwrap();
    let tempo = TempoTable::build(&s.tempo, Tick(-s.start_offset.0), 48_000.0);
    let a = TargetWrite {
        target: PAN,
        pass: 0,
        points: vec![(0, 0.2), (48_000, 0.8)],
    };
    let b = TargetWrite {
        target: PAN,
        pass: 0,
        points: vec![(24_000, 0.5), (30_000, 0.5)],
    };
    let both = [a, b];
    let stepped = to_model(
        &both,
        SeqId(0),
        &tempo,
        &|_, _| 0.1,
        &|_| "Pan".into(),
        &|_| true,
    );
    assert_eq!(stepped[1].points.first().unwrap().1, 0.2, "{stepped:?}");
    assert_eq!(stepped[1].points.last().unwrap().1, 0.2, "{stepped:?}");
    let linear = to_model(
        &both,
        SeqId(0),
        &tempo,
        &|_, _| 0.1,
        &|_| "Pan".into(),
        &|_| false,
    );
    assert!(
        (linear[1].points.first().unwrap().1 - 0.5).abs() < 1e-3,
        "{linear:?}"
    );
    // A step on a shared tick: one point, the later value.
    let g = (40_000..41_000u64)
        .find(|g| tempo.frame_to_tick(*g) == tempo.frame_to_tick(g + 1))
        .unwrap();
    let tw = TargetWrite {
        target: PAN,
        pass: 0,
        points: vec![(1_000, 0.0), (g, 0.0), (g + 1, 1.0)],
    };
    let m = to_model(
        &[tw],
        SeqId(0),
        &tempo,
        &|_, _| 0.0,
        &|_| "Pan".into(),
        &|_| true,
    );
    let inner = &m[0].points[1..m[0].points.len() - 1];
    assert_eq!(
        inner,
        [
            (tempo.frame_to_tick(1_000), 0.0),
            (tempo.frame_to_tick(g), 1.0)
        ]
    );
}

/// A tempo table from `(tick, bpm, ramp)` points.
fn table(pts: &[(i64, u32, bool)]) -> TempoTable {
    use core_model::tempo::{Bpm, TempoMap, TempoPoint};
    let m = TempoMap::new(
        pts.iter()
            .map(|(t, b, r)| TempoPoint {
                tick: Tick(*t),
                bpm: Bpm::whole(*b),
                ramp: *r,
            })
            .collect(),
    )
    .unwrap();
    TempoTable::build(&m, Tick(0), 48_000.0)
}

/// Re-review 2 N4: a Write touch whose first value comes 1–4 frames
/// after it (click-to-position: Touch and Value microseconds apart) keeps
/// the hold before the touch at any tempo: where the step and the hold
/// share a tick and the next point owns the tick after, the hold stays
/// and the value that lasted under a tick goes.
#[test]
fn a_write_touch_with_a_value_just_after_keeps_its_hold() {
    let tables = [
        table(&[(0, 60, false)]),
        table(&[(0, 120, false)]),
        table(&[(0, 177, false)]),
        table(&[(0, 120, false), (19_200, 177, false)]),
        table(&[(0, 60, true), (40_000, 177, false)]),
    ];
    for (n, tempo) in tables.iter().enumerate() {
        for k in 1..5i64 {
            for f in 48_000..48_400i64 {
                let stamps = [
                    st(PAN, f, GesturePhase::Touch(0.8), WriteMode::Write),
                    st(PAN, f + k, GesturePhase::Value(0.6), WriteMode::Write),
                    st(PAN, f + 5_000, GesturePhase::Hold, WriteMode::Write),
                ];
                let w = run(
                    &[roll(0, 0)],
                    96_000,
                    &stamps,
                    &[(PAN, 0.2)],
                    &half,
                    &no_points,
                );
                let m = to_model(&w, SeqId(0), tempo, &|_, _| 0.5, &|_| "Pan".into(), &|t| {
                    t.is_stepped()
                });
                let p: Vec<(i64, f64)> = m[0].points.iter().map(|(t, v)| (t.0, *v)).collect();
                assert!(
                    p.windows(2).all(|x| x[0].0 < x[1].0),
                    "table {n} k={k} f={f}: {p:?}"
                );
                let mid = tempo.frame_to_tick(20_000).0;
                assert!(
                    (lane_value(&p, mid) - 0.2).abs() < 1e-12,
                    "table {n} k={k} f={f}: {p:?}"
                );
                let late = tempo.frame_to_tick(f as u64 + 2_000).0;
                assert!(
                    (lane_value(&p, late) - 0.6).abs() < 1e-12,
                    "table {n} k={k} f={f}: {p:?}"
                );
            }
        }
    }
}

/// Re-review 2 nit: a Touch glide stamped in a gap lands where its segment
/// reopened, after the engine had begun it: like a Release there, it
/// writes nothing (a Glide(0) no stale frame).
#[test]
fn a_glide_where_its_segment_reopened_writes_nothing() {
    let marks = [
        roll(0, 0),
        Mark {
            run: 100,
            frame: 100,
            wrap: false,
            latency: 1_000,
        },
        Mark {
            run: 200,
            frame: 200,
            wrap: false,
            latency: 0,
        },
    ];
    for n in [0, 1_000] {
        let stamps = [
            st(PAN, 20, GesturePhase::Touch(0.4), WriteMode::Touch),
            st(PAN, 60, GesturePhase::Value(0.6), WriteMode::Touch),
            st(PAN, 150, GesturePhase::Glide(n), WriteMode::Touch),
        ];
        let w = run(&marks, 96_000, &stamps, &[], &half, &no_points);
        assert_eq!(w.len(), 1, "Glide({n}): {w:?}");
        close(&w[0].points, &[(20, 0.4), (60, 0.6), (100, 0.6)]);
    }
}
