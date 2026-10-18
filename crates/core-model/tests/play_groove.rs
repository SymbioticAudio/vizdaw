//! Play quantize with a groove (editors spec §4e: per drum lane play
//! quantize, grid or groove).

use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{LaneId, SeqId};
use core_model::lane::{MAX_GROOVE, QuantizeSpec};
use core_model::query::flatten;
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

const E: i64 = PPQ / 2;

#[test]
fn swing_moves_every_second_line_late() {
    let s = QuantizeSpec::swing(Tick(E), 66);
    // 66 % of a quarter for the first eighth: the second starts 16 % of a
    // quarter (0.32 of an eighth) late.
    assert_eq!(s.groove, vec![Tick(0), Tick(2_457)]);
    assert_eq!(s.apply(Tick(10)), Tick(0));
    assert_eq!(s.apply(Tick(E + 10)), Tick(E + 2_457));
    assert_eq!(s.apply(Tick(2 * E - 30)), Tick(2 * E));
    assert_eq!(
        s.apply(Tick(-E)),
        Tick(-E + 2_457),
        "lines count from tick 0"
    );
    assert_eq!(
        QuantizeSpec::swing(Tick(E), 50).apply(Tick(E + 10)),
        Tick(E)
    );
    assert_eq!(
        QuantizeSpec::swing(Tick(E), 99).groove[1],
        Tick(E / 2),
        "clamped to 75 %"
    );
    let mut half = s.clone();
    half.strength = 50;
    assert_eq!(half.apply(Tick(E)), Tick(E + 1_228));
    assert!(s.check().is_ok());
}

#[test]
fn check_refuses_bad_grooves() {
    let bad = [
        QuantizeSpec::grid(Tick(0), 100),
        QuantizeSpec::grid(Tick((1 << 40) + 1), 100),
        QuantizeSpec::grid(Tick(E), 101),
        QuantizeSpec {
            groove: vec![Tick(E)],
            ..QuantizeSpec::grid(Tick(E), 100)
        },
        QuantizeSpec {
            groove: vec![Tick(-E)],
            ..QuantizeSpec::grid(Tick(E), 100)
        },
        QuantizeSpec {
            groove: vec![Tick(0); MAX_GROOVE + 1],
            ..QuantizeSpec::grid(Tick(E), 100)
        },
    ];
    for q in bad {
        assert!(q.check().is_err(), "{q:?}");
    }
    assert!(
        QuantizeSpec {
            groove: vec![Tick(E - 1), Tick(1 - E)],
            ..QuantizeSpec::grid(Tick(E), 100)
        }
        .check()
        .is_ok()
    );
}

/// One sequence with a drum lane: notes at 0, an eighth and a quarter
/// plus an eighth (off the grid by 20 ticks each).
fn drums() -> Document {
    let mut d = Document::new();
    for c in [
        Command::Routing(RoutingCmd::AddDestination {
            name: "Kit".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "e".into(),
                group: 0,
                channel: 9,
            },
            default_patch: None,
        }),
        Command::Sequence(SequenceCmd::New {
            name: "Beat".into(),
        }),
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(core_model::ids::DestId(0)),
        }),
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: [20, E + 20, PPQ + E - 20]
                .iter()
                .map(|&t| EventSpec::new(Tick(t), EventBody::note(42, 100, Tick(E / 2))))
                .collect(),
        }),
        Command::Drum(DrumCmd::MarkDrumLane {
            seq: SeqId(0),
            lane: LaneId(0),
            drum: true,
        }),
    ] {
        d = apply(&d, &c).unwrap().doc;
    }
    d
}

fn starts(d: &Document) -> Vec<i64> {
    flatten(d, SeqId(0), None, &ViewFilter::default())
        .unwrap()
        .map(|r| r.tick.0)
        .collect()
}

#[test]
fn a_groove_plays_through_flatten_and_survives_save() {
    let set = |q: Option<QuantizeSpec>| {
        Command::Lane(LaneCmd::SetPlayQuantize {
            seq: SeqId(0),
            lane: LaneId(0),
            quantize: q,
        })
    };
    let d = apply(&drums(), &set(Some(QuantizeSpec::swing(Tick(E), 66))))
        .unwrap()
        .doc;
    assert_eq!(starts(&d), vec![0, E + 2_457, PPQ + E + 2_457]);
    assert!(validate(&d).is_empty());
    let text = core_model::file::to_json(&d).unwrap();
    assert!(text.contains("\"groove\""), "the groove is saved");
    assert_eq!(core_model::file::from_json(&text).unwrap(), d);
    // A plain grid writes no groove key: older builds read the file.
    let d = apply(&d, &set(Some(QuantizeSpec::grid(Tick(E), 100))))
        .unwrap()
        .doc;
    assert!(!core_model::file::to_json(&d).unwrap().contains("groove"));
    assert_eq!(starts(&d), vec![0, E, PPQ + E]);
    // A bad groove is refused and changes nothing.
    let bad = QuantizeSpec {
        groove: vec![Tick(E)],
        ..QuantizeSpec::grid(Tick(E), 100)
    };
    assert!(apply(&d, &set(Some(bad))).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any grid, strength, groove and tick: applying never panics, and a
    /// spec that passes `check` moves a start by less than two grids.
    #[test]
    fn hostile_quantize_never_panics_and_stays_near(
        grid in prop_oneof![-10i64..20_000, Just(i64::MAX), Just(i64::MIN), Just(1i64 << 40)],
        strength in any::<u8>(),
        groove in prop::collection::vec(prop_oneof![-20_000i64..20_000, Just(i64::MAX), Just(i64::MIN)], 0..8),
        tick in prop_oneof![any::<i64>(), -(1i64 << 40)..(1i64 << 40)],
    ) {
        let q = QuantizeSpec { grid: Tick(grid), strength, groove: groove.into_iter().map(Tick).collect() };
        let out = q.apply(Tick(tick));
        if q.check().is_ok() && tick.unsigned_abs() <= 1 << 40 {
            prop_assert!(i128::from(out.0 - tick).abs() < 2 * i128::from(grid), "{tick} -> {} by {q:?}", out.0);
        }
    }
}
