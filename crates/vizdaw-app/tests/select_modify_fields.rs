//! The Select and Modify panel's fields and their text (slice 10b Task 2;
//! Swift `SelectModifyTests.swift`, `M/PanelFields.swift`).

use vizdaw_app::arrangement::PPQ;
use vizdaw_app::list::{Value7, Velocity7};
use vizdaw_app::select_modify::{
    AmountKind, ChangeOpKind, ConditionKind, Edge, Field, FieldKind, GROOVE_HELP, GROOVE_LABEL,
    ModifyKind, NOTE_MAP_PLACEHOLDER, condition_fields, condition_numbers, condition_with,
    field_text, modify_fields, modify_numbers, modify_with, note_map_text, offsets_text,
    parse_field, parse_note_map, parse_offsets,
};
use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::modify::{
    AmountRow, ChangeOpRow, ConditionTest, ModifyRow, NoteMapEntry, SubstituteRow, ValueTargetRow,
};

const Q: i64 = PPQ;

fn meter(num: u8, den: u8) -> Vec<MeterEntryRow> {
    vec![MeterEntryRow {
        tick: 0,
        num,
        den,
        sharps: 0,
        minor: false,
    }]
}

fn meter44() -> Vec<MeterEntryRow> {
    meter(4, 4)
}

fn parse(s: &str, k: FieldKind) -> Option<i64> {
    parse_field(s, k, &meter44())
}

fn text(n: i64, k: FieldKind) -> String {
    field_text(n, k, &meter44())
}

#[test]
fn fields_and_numbers_agree() {
    let mut tests: Vec<ConditionTest> = ConditionKind::OFFERED
        .iter()
        .map(|k| k.default_test(None))
        .collect();
    tests.push(ConditionTest::Metric {
        grid: Q,
        offset: 5,
        tolerance: 7,
    });
    for t in &tests {
        assert_eq!(
            condition_fields(t).len(),
            condition_numbers(t).len(),
            "{t:?}"
        );
        assert_eq!(
            condition_with(t, &condition_numbers(t)).as_ref(),
            Some(t),
            "{t:?}"
        );
    }
    let mut rows: Vec<ModifyRow> = ModifyKind::OFFERED.iter().map(|k| k.default_op()).collect();
    rows.push(ModifyRow::Substitute {
        to: SubstituteRow::Pitch { pitch: 40 },
    });
    rows.push(ModifyRow::Substitute {
        to: SubstituteRow::PitchBend,
    });
    for op in ChangeOpKind::ALL {
        for a in AmountKind::ALL {
            rows.push(ModifyRow::Change {
                target: ValueTargetRow::Value,
                op: op.row(ValueTargetRow::Value),
                amount: a.row(3),
            });
        }
    }
    for r in &rows {
        assert_eq!(modify_fields(r).len(), modify_numbers(r).len(), "{r:?}");
        assert_eq!(
            modify_with(r, &modify_numbers(r)).as_ref(),
            Some(r),
            "{r:?}"
        );
    }
    assert_eq!(
        modify_fields(&ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Scale,
            amount: AmountRow::Constant { value: 100 },
        }),
        vec![Field {
            label: "amount",
            kind: FieldKind::Percent
        }]
    );
    assert_eq!(
        modify_fields(&ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: 0 },
        }),
        vec![Field {
            label: "amount",
            kind: FieldKind::Velocity(Edge::Delta)
        }]
    );
    // The out-of-scope Strip Chart line shows no fields.
    let line = ModifyKind::Line.default_op();
    assert!(modify_fields(&line).is_empty());
    assert_eq!(modify_with(&line, &[]), Some(line));
}

#[test]
fn the_labels_are_swifts() {
    let labels = |f: Vec<Field>| f.iter().map(|f| f.label).collect::<Vec<_>>();
    let c = |k: ConditionKind| labels(condition_fields(&k.default_test(None)));
    assert_eq!(c(ConditionKind::Time), ["from", "to"]);
    assert_eq!(c(ConditionKind::Metric), ["grid", "offset", "within"]);
    assert_eq!(c(ConditionKind::EveryNth), ["every", "starting at"]);
    assert!(c(ConditionKind::Kind).is_empty());
    let m = |k: ModifyKind| labels(modify_fields(&k.default_op()));
    assert_eq!(m(ModifyKind::Transpose), ["semitones"]);
    assert_eq!(m(ModifyKind::Diatonic), ["steps"]);
    assert_eq!(m(ModifyKind::Invert), ["around"]);
    assert_eq!(m(ModifyKind::Quantize), ["grid", "strength"]);
    assert_eq!(m(ModifyKind::Move), ["by"]);
    assert_eq!(m(ModifyKind::ScaleTime), ["times", "divided by", "around"]);
    assert_eq!(m(ModifyKind::TrimStart), ["by"]);
    assert_eq!(m(ModifyKind::Substitute), ["controller"]);
    assert_eq!(m(ModifyKind::Thin), ["spacing"]);
    assert_eq!(m(ModifyKind::Thicken), ["spacing"]);
    let change = |op: ChangeOpKind, a: AmountKind| {
        labels(modify_fields(&ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: op.row(ValueTargetRow::Velocity),
            amount: a.row(1),
        }))
    };
    assert_eq!(change(ChangeOpKind::Set, AmountKind::Constant), ["amount"]);
    assert_eq!(change(ChangeOpKind::Set, AmountKind::Line), ["from", "to"]);
    assert_eq!(
        change(ChangeOpKind::Set, AmountKind::Oscillator),
        ["low", "high", "period", "phase"]
    );
    assert_eq!(
        change(ChangeOpKind::Set, AmountKind::Random),
        ["low", "high"]
    );
    assert_eq!(
        change(ChangeOpKind::Limit, AmountKind::Constant),
        ["lowest", "highest"]
    );
    assert_eq!(
        change(ChangeOpKind::Compress, AmountKind::Constant),
        ["lowest", "highest"]
    );
    assert!(change(ChangeOpKind::Legato, AmountKind::Constant).is_empty());
    assert_eq!(
        labels(modify_fields(&ModifyRow::Substitute {
            to: SubstituteRow::Pitch { pitch: 36 }
        })),
        ["pitch"]
    );
}

#[test]
fn field_text_round_trips() {
    let v = FieldKind::Velocity;
    assert_eq!(parse("100", v(Edge::Exact)), Some(0xC924));
    assert_eq!(text(0xC924, v(Edge::Exact)), "100");
    assert_eq!(parse("100", v(Edge::High)), Some(51_711));
    assert_eq!(parse("-10", v(Edge::Delta)), Some(-5120));
    assert_eq!(text(-5120, v(Edge::Delta)), "-10");
    assert_eq!(
        parse("127", FieldKind::Value(Edge::Exact)),
        Some(0xFFFF_FFFF)
    );
    assert_eq!(parse("-1.480", FieldKind::Length), Some(-(Q + Q / 2)));
    assert_eq!(text(-(Q + Q / 2), FieldKind::Length), "-1.480");
    assert_eq!(parse("2", FieldKind::Position), Some(4 * Q));
    assert_eq!(text(4 * Q, FieldKind::Position), "2.1.0");
    assert_eq!(parse("80%", FieldKind::Percent), Some(80));
    assert_eq!(parse("80", FieldKind::Percent), Some(80));
    assert_eq!(text(80, FieldKind::Percent), "80%");
    assert_eq!(parse("C4", FieldKind::Pitch), Some(60));
    assert_eq!(text(60, FieldKind::Pitch), "C4");
    assert_eq!(parse("abc", FieldKind::Count), None);
    assert_eq!(parse("200", v(Edge::Exact)), None);
    assert_eq!(parse("1000001", FieldKind::Count), None);
    assert_eq!(parse("-1000000", FieldKind::Count), Some(-1_000_000));
    assert_eq!(parse("120", FieldKind::Bpm), Some(120_000));
    assert_eq!(parse("-12.5 BPM", FieldKind::Bpm), Some(-12_500));
    assert_eq!(text(120_000, FieldKind::Bpm), "120.00");
    assert_eq!(parse("1000", FieldKind::Bpm), None);
    assert_eq!(parse("inf", FieldKind::Bpm), None);
}

#[test]
fn note_map_and_groove_text() {
    let m = parse_note_map("C2>D2, E2>F#2");
    assert_eq!(
        m,
        Some(vec![
            NoteMapEntry { from: 36, to: 38 },
            NoteMapEntry { from: 40, to: 42 }
        ])
    );
    assert_eq!(note_map_text(&m.unwrap_or_default()), "C2>D2, E2>F#2");
    assert_eq!(parse_note_map("C2"), None);
    assert_eq!(parse_note_map(""), Some(vec![]));
    assert_eq!(parse_offsets("0 120 -60"), Some(vec![0, 120, -60]));
    assert_eq!(offsets_text(&[0, 120, -60]), "0 120 -60");
    assert_eq!(parse_offsets("0 x"), None);
    assert_eq!(parse_offsets("2000000"), None);
    assert_eq!(NOTE_MAP_PLACEHOLDER, "C2>D2, E2>F#2");
    assert_eq!(GROOVE_LABEL, "Groove offsets");
    assert_eq!(
        GROOVE_HELP,
        "Groove offsets in ticks, one per grid line; empty for a plain grid"
    );
}

#[test]
fn an_empty_groove_field_is_a_plain_grid() {
    assert_eq!(parse_offsets(""), Some(vec![]));
    assert_eq!(parse_offsets("   "), Some(vec![]));
    assert_eq!(offsets_text(&[]), "");
}

#[test]
fn typed_numbers_outside_their_range_are_rejected() {
    let semis = modify_fields(&ModifyRow::TransposeInterval { semitones: 0 })[0].kind;
    assert_eq!(parse("127", semis), Some(127));
    assert_eq!(parse("-127", semis), Some(-127));
    assert_eq!(parse("128", semis), None);
    assert_eq!(parse("-200", semis), None);
    let controller = condition_fields(&ConditionKind::Controller.default_test(None))[0].kind;
    assert_eq!(parse("128", controller), None);
    let every = condition_fields(&ConditionKind::EveryNth.default_test(None))[0].kind;
    assert_eq!(parse("0", every), None);
    let times = modify_fields(&ModifyRow::ScaleTime {
        num: 1,
        den: 1,
        anchor: 0,
    })[0]
        .kind;
    assert_eq!(parse("0", times), None);
    let vel = condition_fields(&ConditionKind::Velocity.default_test(None))[0].kind;
    assert_eq!(parse("128", vel), None);
    assert_eq!(parse("-1", FieldKind::Pitch), None);
    assert_eq!(parse("10001%", FieldKind::Percent), None);
    assert_eq!(parse("10000%", FieldKind::Percent), Some(10_000));
    assert_eq!(parse("-1%", FieldKind::Percent), None);
}

#[test]
fn every_kind_round_trips_its_text_on_grid() {
    let v = |x: i64| i64::from(Velocity7::up(x));
    let val = |x: i64| i64::from(Value7::up(x));
    let cases: Vec<(FieldKind, Vec<i64>)> = vec![
        (FieldKind::Count, vec![0, 7, 1000]),
        (FieldKind::Bounded(-127, 127), vec![-127, 0, 127]),
        (FieldKind::Percent, vec![0, 80, 100, 200]),
        (FieldKind::Pitch, vec![0, 60, 127]),
        (FieldKind::Velocity(Edge::Exact), vec![v(0), v(100), v(127)]),
        (FieldKind::Velocity(Edge::Low), vec![0, 100 << 9, 127 << 9]),
        (
            FieldKind::Velocity(Edge::High),
            vec![511, (100 << 9) | 511, 0xFFFF],
        ),
        (FieldKind::Velocity(Edge::Delta), vec![-512, 0, 5120]),
        (
            FieldKind::Value(Edge::Exact),
            vec![val(0), val(64), val(127)],
        ),
        (FieldKind::Value(Edge::Low), vec![0, 64 << 25]),
        (FieldKind::Value(Edge::High), vec![0x1FF_FFFF, 0xFFFF_FFFF]),
        (FieldKind::Value(Edge::Delta), vec![-(1 << 25), 3 << 25]),
        (FieldKind::Length, vec![0, Q / 4, Q, -Q, -(Q + Q / 2)]),
        (FieldKind::Position, vec![0, Q, 4 * Q, 9 * Q + Q / 2]),
        (FieldKind::Bpm, vec![120_000, -5_250, 0]),
    ];
    for (k, ns) in cases {
        for n in ns {
            let t = text(n, k);
            assert_eq!(parse(&t, k), Some(n), "{k:?} {n} -> {t}");
        }
    }
}

#[test]
fn the_high_velocity_edge_covers_the_top_step() {
    assert_eq!(
        parse_field("127", FieldKind::Velocity(Edge::High), &[]),
        Some(0xFFFF)
    );
    assert_eq!(
        parse_field("127", FieldKind::Velocity(Edge::Low), &[]),
        Some(127 << 9)
    );
    assert_eq!(
        parse_field("127", FieldKind::Value(Edge::High), &[]),
        Some(0xFFFF_FFFF)
    );
}

#[test]
fn positions_parse_in_the_given_meter() {
    let k = FieldKind::Position;
    assert_eq!(parse_field("2.1.0", k, &meter(4, 4)), Some(4 * Q));
    assert_eq!(parse_field("2.1.0", k, &meter(3, 4)), Some(3 * Q));
    assert_eq!(field_text(3 * Q, k, &meter(3, 4)), "2.1.0");
    // An empty meter is 4/4.
    assert_eq!(parse_field("2.1.0", k, &[]), Some(4 * Q));
}

#[test]
fn a_negative_length_reads_and_writes_with_a_minus() {
    let k = FieldKind::Length;
    assert_eq!(field_text(-Q / 2, k, &[]), "-0.480");
    assert_eq!(parse_field("-0.480", k, &[]), Some(-Q / 2));
    assert_eq!(parse_field("-2", k, &[]), Some(-2 * Q));
    assert_eq!(
        field_text(i64::MIN, k, &[]),
        format!("-{}", field_text(i64::MAX, k, &[]))
    );
    let moved = modify_with(&ModifyRow::Move { delta: 0 }, &[-Q]);
    assert_eq!(moved, Some(ModifyRow::Move { delta: -Q }));
}

#[test]
fn with_clamps_to_the_rows_types_and_rejects_a_wrong_count() {
    assert_eq!(
        condition_with(&ConditionKind::Pitch.default_test(None), &[-5, 300]),
        Some(ConditionTest::Pitch { low: 0, high: 127 })
    );
    assert_eq!(
        condition_with(&ConditionKind::Velocity.default_test(None), &[-1, 70_000]),
        Some(ConditionTest::Velocity {
            low: 0,
            high: 0xFFFF
        })
    );
    assert_eq!(
        modify_with(&ModifyRow::TransposeInterval { semitones: 0 }, &[500]),
        Some(ModifyRow::TransposeInterval { semitones: 127 })
    );
    assert_eq!(
        modify_with(&ModifyKind::Quantize.default_op(), &[0, 300]),
        Some(ModifyRow::Quantize {
            grid: 1,
            strength: 100,
            offsets: vec![]
        })
    );
    assert_eq!(
        condition_with(&ConditionKind::Pitch.default_test(None), &[60]),
        None
    );
    assert_eq!(modify_with(&ModifyRow::Move { delta: 0 }, &[1, 2]), None);
}
