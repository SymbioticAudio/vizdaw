//! Strip Chart lines (Plan 4d Rulings 16–18).

use core_model::automation::{AutomationTarget, to_u32};
use core_model::event::EventBody;
use core_model::ids::StripId;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::modify::{Curve, Wave};
use core_model::strip::{Line, LineShape, MAX_SAMPLES, Period, Series, TimeCtx, reduce};
use core_model::tempo::TempoMap;
use core_model::time::{PPQ, Tick};

const Q: i64 = PPQ;

fn line(shape: LineShape) -> Line {
    Line {
        from: Tick(0),
        to: Tick(4 * Q),
        start: 0,
        end: 1000,
        step: Tick(Q),
        shape,
    }
}

fn at(l: &Line, t: i64) -> i64 {
    let (tempo, meter) = (TempoMap::default(), MeterMap::default());
    l.at(
        Tick(t),
        TimeCtx {
            tempo: &tempo,
            meter: &meter,
        },
    )
}

#[test]
fn shapes_give_their_values() {
    let s = line(LineShape::Straight);
    assert_eq!(
        [
            at(&s, -5),
            at(&s, 0),
            at(&s, 2 * Q),
            at(&s, 4 * Q),
            at(&s, 9 * Q)
        ],
        [0, 0, 500, 1000, 1000]
    );
    let c = line(LineShape::Curved {
        curve: Curve::EaseIn,
    });
    assert_eq!(at(&c, 2 * Q), 250, "x² at the middle");
    assert_eq!(
        at(&line(LineShape::Flat), 3 * Q),
        0,
        "flat holds the start value"
    );
    let f = line(LineShape::Free {
        points: vec![(Tick(Q), 100), (Tick(3 * Q), 900)],
    });
    assert_eq!([at(&f, 0), at(&f, 2 * Q), at(&f, 4 * Q)], [100, 500, 900]);
    let r = line(LineShape::Random { seed: 7 });
    let values: Vec<i64> = (0..64).map(|k| at(&r, k * 100)).collect();
    assert!(values.iter().all(|v| (0..=1000).contains(v)));
    assert_eq!(
        values,
        (0..64).map(|k| at(&r, k * 100)).collect::<Vec<_>>(),
        "fixed by the seed"
    );
    let other: Vec<i64> = (0..64)
        .map(|k| at(&line(LineShape::Random { seed: 8 }), k * 100))
        .collect();
    assert_ne!(values, other);
}

#[test]
fn oscillators_count_their_period_in_ticks_milliseconds_or_bars() {
    let osc = |wave, period| {
        line(LineShape::Oscillator {
            wave,
            period,
            phase: 0,
        })
    };
    let sine = osc(Wave::Sine, Period::Ticks(Tick(Q)));
    assert_eq!(
        [
            at(&sine, 0),
            at(&sine, Q / 4),
            at(&sine, Q / 2),
            at(&sine, 3 * Q / 4)
        ],
        [500, 1000, 500, 0]
    );
    let square = osc(Wave::Square, Period::Ticks(Tick(Q)));
    assert_eq!([at(&square, Q / 4), at(&square, 3 * Q / 4)], [1000, 0]);
    // At 120 BPM one second is two quarters.
    let clock = osc(Wave::Saw, Period::Millis(1000));
    let musical = osc(Wave::Saw, Period::Ticks(Tick(2 * Q)));
    for t in [0, Q / 3, Q, 3 * Q / 2] {
        assert!((at(&clock, t) - at(&musical, t)).abs() <= 1, "tick {t}");
    }
    // A phase of 250 thousandths starts a quarter period in.
    let shifted = line(LineShape::Oscillator {
        wave: Wave::Sine,
        period: Period::Ticks(Tick(Q)),
        phase: 250,
    });
    assert_eq!(at(&shifted, 0), 1000);
    // Following the meter: 4/4 then 3/4 from bar 2 (tick 4Q): one bar is
    // one period whatever its length.
    let meter = MeterMap::new(vec![
        MeterEntry {
            tick: Tick(0),
            meter: Meter { num: 4, den: 4 },
            key: KeySig::default(),
        },
        MeterEntry {
            tick: Tick(4 * Q),
            meter: Meter { num: 3, den: 4 },
            key: KeySig::default(),
        },
    ])
    .unwrap();
    assert_eq!(meter.bars_at(Tick(2 * Q)), 0.5);
    assert_eq!(meter.bars_at(Tick(4 * Q + 3 * Q)), 2.0);
    assert_eq!(meter.bars_at(Tick(-2 * Q)), -0.5, "pickup bars count back");
    let bars = Line {
        to: Tick(16 * Q),
        ..osc(Wave::Saw, Period::Bars(1))
    };
    let tempo = TempoMap::default();
    let ctx = TimeCtx {
        tempo: &tempo,
        meter: &meter,
    };
    // Saw: low at each bar start, half-way up mid-bar.
    assert_eq!(bars.at(Tick(4 * Q), ctx), 0);
    assert_eq!(bars.at(Tick(4 * Q + 3 * Q / 2), ctx), 500);
    assert_eq!(bars.at(Tick(4 * Q + 3 * Q), ctx), 0);
}

#[test]
fn samples_fall_on_steps_ends_and_free_corners() {
    let tempo = TempoMap::default();
    let meter = MeterMap::default();
    let ctx = TimeCtx {
        tempo: &tempo,
        meter: &meter,
    };
    let s = line(LineShape::Straight).samples(ctx).unwrap();
    assert_eq!(
        s,
        vec![
            (Tick(0), 0),
            (Tick(Q), 250),
            (Tick(2 * Q), 500),
            (Tick(3 * Q), 750),
            (Tick(4 * Q), 1000)
        ]
    );
    let click = Line {
        to: Tick(0),
        ..line(LineShape::Straight)
    };
    assert_eq!(
        click.samples(ctx).unwrap(),
        vec![(Tick(0), 0)],
        "a click is one point"
    );
    let free = Line {
        step: Tick(2 * Q),
        ..line(LineShape::Free {
            points: vec![(Tick(0), 0), (Tick(Q), 800), (Tick(4 * Q), 0)],
        })
    };
    let ticks: Vec<i64> = free.samples(ctx).unwrap().iter().map(|p| p.0.0).collect();
    assert_eq!(ticks, vec![0, Q, 2 * Q, 4 * Q], "the corner at Q is kept");
    let dense = Line {
        step: Tick(1),
        to: Tick(MAX_SAMPLES as i64 + 5),
        ..line(LineShape::Straight)
    };
    assert!(dense.samples(ctx).is_err(), "more than MAX_SAMPLES");
}

#[test]
fn bad_lines_are_refused() {
    let ok = line(LineShape::Straight);
    assert!(ok.check().is_ok());
    assert!(
        Line {
            to: Tick(-1),
            ..ok.clone()
        }
        .check()
        .is_err(),
        "to before from"
    );
    assert!(
        Line {
            step: Tick(0),
            ..ok.clone()
        }
        .check()
        .is_err()
    );
    assert!(
        line(LineShape::Free {
            points: vec![(Tick(5), 0), (Tick(5), 1)],
        })
        .check()
        .is_err(),
        "points out of order"
    );
    for (period, phase) in [
        (Period::Ticks(Tick(0)), 0),
        (Period::Millis(0), 0),
        (Period::Bars(0), 0),
        (Period::Bars(1), 1000),
    ] {
        assert!(
            line(LineShape::Oscillator {
                wave: Wave::Sine,
                period,
                phase,
            })
            .check()
            .is_err(),
            "{period:?} {phase}"
        );
    }
}

#[test]
fn reduce_keeps_what_changes_the_curve() {
    let ramp = [
        (Tick(0), 0),
        (Tick(10), 10),
        (Tick(20), 20),
        (Tick(30), 31),
        (Tick(40), 40),
    ];
    assert_eq!(
        reduce(&ramp, true),
        vec![(Tick(0), 0), (Tick(40), 40)],
        "within one unit of the chord"
    );
    let bend = [(Tick(0), 0), (Tick(10), 10), (Tick(20), 50), (Tick(30), 50)];
    assert_eq!(
        reduce(&bend, true),
        vec![(Tick(0), 0), (Tick(10), 10), (Tick(20), 50), (Tick(30), 50)]
    );
    let held = [
        (Tick(0), 5),
        (Tick(10), 5),
        (Tick(20), 7),
        (Tick(30), 7),
        (Tick(40), 7),
    ];
    assert_eq!(
        reduce(&held, false),
        vec![(Tick(0), 5), (Tick(20), 7), (Tick(40), 7)],
        "a holding series drops repeats but keeps its last sample"
    );
    assert_eq!(reduce(&[(Tick(3), 1)], true), vec![(Tick(3), 1)]);
    assert!(reduce(&[], false).is_empty());
}

#[test]
fn series_read_and_write_the_32_bit_scale() {
    let prog = EventBody::Program {
        program: 5,
        bank: Some((0, 1)),
    };
    assert_eq!(Series::of(&prog), Some(Series::Program));
    assert_eq!(Series::value(&prog), Some(5 << 25));
    let mut p = prog.clone();
    Series::set_value(&mut p, (9 << 25) + 12);
    assert_eq!(
        p,
        EventBody::Program {
            program: 9,
            bank: Some((0, 1)),
        },
        "the bank stays"
    );
    let fader = AutomationTarget::StripFader(StripId(1));
    let a = Series::Automation(fader).body(i64::from(u32::MAX) / 2);
    assert!(matches!(a, EventBody::Automation { value, .. } if (value - 0.5).abs() < 1e-9));
    assert_eq!(
        Series::value(&a),
        Some(i64::from(u32::MAX) / 2),
        "round-trips"
    );
    assert_eq!(
        Series::value(&Series::Automation(fader).body(i64::from(to_u32(1.0)))),
        Some(i64::from(u32::MAX))
    );
    let cc = Series::Controller(7).body(-5);
    assert_eq!(cc, EventBody::Cc { index: 7, value: 0 }, "clamped");
    assert_eq!(
        Series::PolyPressure(60).body(i64::MAX),
        EventBody::PolyPressure {
            pitch: 60,
            value: u32::MAX,
        }
    );
    assert!(Series::Automation(fader).ramps());
    assert!(!Series::Automation(AutomationTarget::StripSolo(StripId(1))).ramps());
    assert!(!Series::Controller(1).ramps());
    assert!(Series::Controller(128).check().is_err());
    assert_eq!(Series::of(&EventBody::note(60, 1, Tick(1))), None);
}
