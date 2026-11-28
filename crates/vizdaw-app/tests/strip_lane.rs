//! Swift `StripLaneTests` (and the names half of `AutomationRowsTests`'
//! `vcaFaderTargetsAreNamed`): Strip Chart rows, names, value text and the
//! row scale.

use core_model::automation::{fader_db, fader_norm, pan_of, velocity_scale_percent};
use core_model::mixer::GAIN_FLOOR_DB;
use vizdaw_app::list::{Bend14, Velocity7};
use vizdaw_app::strip_chart::{
    StripLane, StripMode, StripNames, StripOp, StripScale, StripStyle, StripValue, after,
    automation_text, is_switch, normalized, percent, plain_text, short_title, v32,
};
use vizdaw_ffi::events::{
    AutomationRow, AutomationTargetRow as T, EventKindRow, EventRow, TempoPointRow,
};
use vizdaw_ffi::modify::{ConditionTest, ValueTargetRow};
use vizdaw_ffi::snapshot::ParamRow;
use vizdaw_ffi::strip::{LineOpRow, LineTargetRow};

const Q: i64 = 15_360;

fn param_row(id: u64, name: &str, min: f64, max: f64, stepped: bool) -> ParamRow {
    ParamRow {
        id,
        name: name.into(),
        module: String::new(),
        min,
        max,
        default: min,
        value: min,
        stepped,
        automatable: true,
        hidden: false,
        read_only: false,
    }
}

fn note_row(id: u64, tick: i64, duration: i64) -> EventRow {
    EventRow {
        pitch: 60,
        velocity: 0x8000,
        duration,
        ..EventRow::blank(id, tick, EventKindRow::Note)
    }
}

fn cc_row(id: u64, tick: i64, index: u8, value: i64) -> EventRow {
    EventRow {
        value2: i64::from(index),
        value,
        ..EventRow::blank(id, tick, EventKindRow::Cc)
    }
}

fn automation_row(id: u64, tick: i64, target: T, v: f64) -> EventRow {
    EventRow {
        automation: Some(AutomationRow { target, value: v }),
        value: v32(v),
        ..EventRow::blank(id, tick, EventKindRow::Automation)
    }
}

fn tests_of(lane: StripLane, from: i64, to: i64) -> Option<Vec<ConditionTest>> {
    lane.erase_rule(from, to)
        .map(|r| r.conditions.into_iter().map(|c| c.test).collect())
}

#[test]
fn laws_match_the_core() {
    assert_eq!(fader_db(1.), 12.);
    assert_eq!(fader_db(0.), GAIN_FLOOR_DB);
    assert!((fader_norm(0.) - 0.630_957).abs() < 1e-6);
    assert!((fader_db(fader_norm(-6.)) + 6.).abs() < 1e-9);
    assert_eq!(pan_of(0.25), -0.5);
    assert_eq!(velocity_scale_percent(0.5), 100.);
    assert_eq!(velocity_scale_percent(1.), 250.);
    assert_eq!(v32(1.), i64::from(u32::MAX));
    assert_eq!(normalized(0), 0.);
    let fader = T::StripFader { strip: 1 };
    let pan = T::StripPan { strip: 1 };
    assert_eq!(automation_text(&fader, 0., None), "-∞ dB");
    assert_eq!(automation_text(&fader, fader_norm(-6.), None), "-6.0 dB");
    assert_eq!(automation_text(&pan, 0.5, None), "C");
    assert_eq!(automation_text(&pan, 0.25, None), "L50");
    assert_eq!(automation_text(&pan, 0.75, None), "R50");
    assert_eq!(automation_text(&T::StripMute { strip: 1 }, 0.7, None), "On");
    let p = param_row(3, "Cutoff", 20., 20_020., false);
    assert_eq!(
        automation_text(&T::PluginParam { slot: 2, param: 3 }, 0.5, Some(&p)),
        "10020"
    );
    for (x, want) in [
        (999., "999"),
        (1000., "1000"),
        (10_020., "10020"),
        (0.001, "0.001"),
        (0.0123, "0.0123"),
    ] {
        assert_eq!(plain_text(x), want, "{x}");
    }
    assert_eq!(plain_text(-0.000_000_1), "0", "-0 is 0");
    assert_eq!(
        automation_text(&T::VelocityScale { dest: 1 }, 0.75, None),
        "175%"
    );
}

#[test]
fn law_edges_and_texts() {
    assert_eq!(fader_norm(20.), 1., "above 12 dB is full scale");
    assert_eq!(fader_norm(GAIN_FLOOR_DB), 0.);
    assert_eq!(fader_norm(-200.), 0.);
    assert_eq!(fader_norm(f64::NAN), 0.);
    assert_eq!(fader_db(f64::NAN), GAIN_FLOOR_DB);
    assert_eq!(pan_of(f64::NAN), -1.);
    let solo = T::StripSolo { strip: 1 };
    assert_eq!(automation_text(&solo, 0.5, None), "On");
    assert_eq!(automation_text(&solo, 0.4999, None), "Off");
    let send = T::SendLevel { strip: 1, send: 0 };
    assert_eq!(automation_text(&send, fader_norm(-12.), None), "-12.0 dB");
    assert_eq!(automation_text(&send, 1., None), "12.0 dB");
    let fader = T::StripFader { strip: 1 };
    let near_floor = fader_norm(-143.97);
    assert!(near_floor > 0.);
    assert_eq!(
        automation_text(&fader, near_floor, None),
        "-∞ dB",
        "never -144.0 dB"
    );
    assert_eq!(
        automation_text(&fader, fader_norm(-143.9), None),
        "-143.9 dB"
    );
    let t = T::PluginParam { slot: 1, param: 1 };
    assert_eq!(
        automation_text(&t, 0.63, Some(&param_row(1, "P", 0., 4., true))),
        "3"
    );
    assert_eq!(
        automation_text(
            &t,
            0.5,
            Some(&param_row(1, "P", f64::NEG_INFINITY, 1., true))
        ),
        "0"
    );
    assert!(
        automation_text(&t, 1., Some(&param_row(1, "P", 0., 1e300, true)))
            .chars()
            .count()
            > 100
    );
    assert_eq!(automation_text(&t, f64::NAN, None), "0.0%");
    assert_eq!(percent(f64::NAN), "0.0%");
    assert_eq!(percent(2.), "100.0%");
    assert_eq!(
        short_title(&T::StripFader { strip: 4 }),
        StripNames::default().title(&T::StripFader { strip: 4 })
    );
}

#[test]
fn rules_at_the_last_tick_do_not_overflow() {
    assert_eq!(after(i64::MAX), i64::MAX);
    assert_eq!(after(5), 6);
    let tests = tests_of(StripLane::Program, i64::MAX, 0).unwrap();
    assert_eq!(
        tests[0],
        ConditionTest::TimeRange {
            start: 0,
            end: i64::MAX
        }
    );
    assert_eq!(
        tests_of(StripLane::PitchBend, 0, Q).unwrap().last(),
        Some(&ConditionTest::Kinds {
            kinds: vec![EventKindRow::PitchBend]
        })
    );
    assert_eq!(
        tests_of(StripLane::ChannelPressure, 0, Q).unwrap().last(),
        Some(&ConditionTest::Kinds {
            kinds: vec![EventKindRow::ChannelPressure]
        })
    );
    assert_eq!(
        tests_of(StripLane::PolyPressure(60), 0, Q).unwrap()[1..],
        [
            ConditionTest::Kinds {
                kinds: vec![EventKindRow::PolyPressure]
            },
            ConditionTest::Pitch { low: 60, high: 60 },
        ]
    );
    assert_eq!(
        tests_of(StripLane::Program, 0, Q).unwrap().last(),
        Some(&ConditionTest::Kinds {
            kinds: vec![EventKindRow::Program]
        })
    );
    assert_eq!(StripLane::ReleaseVelocity.erase_rule(0, Q), None);
    assert_eq!(StripLane::Duration.erase_rule(0, Q), None);
}

#[test]
fn targets_round_trip_their_json() {
    let all = [
        T::StripFader { strip: 1 },
        T::StripPan { strip: 2 },
        T::StripMute { strip: 3 },
        T::StripSolo { strip: 4 },
        T::SendLevel { strip: 5, send: 2 },
        T::PluginParam {
            slot: 6,
            param: 1 << 40,
        },
        T::VelocityScale { dest: 7 },
        T::VcaFader { vca: 8 },
    ];
    for t in all {
        let lane = StripLane::Automation(t);
        let json = serde_json::to_string(&lane).unwrap();
        assert_eq!(
            serde_json::from_str::<StripLane>(&json).unwrap(),
            lane,
            "{json}"
        );
    }
    assert!(is_switch(&T::StripMute { strip: 1 }));
    assert!(is_switch(&T::StripSolo { strip: 1 }));
    assert!(!is_switch(&T::StripPan { strip: 1 }));
    // Swift's `Codable` shapes.
    let shapes: [(StripLane, &str); 12] = [
        (StripLane::Velocity, r#"{"velocity":{}}"#),
        (StripLane::ReleaseVelocity, r#"{"releaseVelocity":{}}"#),
        (StripLane::Duration, r#"{"duration":{}}"#),
        (StripLane::Controller(7), r#"{"controller":{"_0":7}}"#),
        (StripLane::PitchBend, r#"{"pitchBend":{}}"#),
        (StripLane::ChannelPressure, r#"{"channelPressure":{}}"#),
        (StripLane::PolyPressure(60), r#"{"polyPressure":{"_0":60}}"#),
        (StripLane::Program, r#"{"program":{}}"#),
        (StripLane::Tempo, r#"{"tempo":{}}"#),
        (
            StripLane::Automation(T::StripFader { strip: 1 }),
            r#"{"automation":{"_0":{"fader":{"strip":1}}}}"#,
        ),
        (
            StripLane::Automation(T::PluginParam { slot: 6, param: 2 }),
            r#"{"automation":{"_0":{"param":{"slot":6,"param":2}}}}"#,
        ),
        (
            StripLane::Automation(T::VcaFader { vca: 3 }),
            r#"{"automation":{"_0":{"vcaFader":{"vca":3}}}}"#,
        ),
    ];
    for (lane, json) in shapes {
        assert_eq!(serde_json::to_string(&lane).unwrap(), json);
        assert_eq!(serde_json::from_str::<StripLane>(json).unwrap(), lane);
    }
    for (json, t) in [
        (
            r#"{"automation":{"_0":{"pan":{"strip":2}}}}"#,
            T::StripPan { strip: 2 },
        ),
        (
            r#"{"automation":{"_0":{"mute":{"strip":3}}}}"#,
            T::StripMute { strip: 3 },
        ),
        (
            r#"{"automation":{"_0":{"solo":{"strip":4}}}}"#,
            T::StripSolo { strip: 4 },
        ),
        (
            r#"{"automation":{"_0":{"send":{"strip":5,"index":2}}}}"#,
            T::SendLevel { strip: 5, send: 2 },
        ),
        (
            r#"{"automation":{"_0":{"velocityScale":{"dest":7}}}}"#,
            T::VelocityScale { dest: 7 },
        ),
    ] {
        assert_eq!(
            serde_json::from_str::<StripLane>(json).unwrap(),
            StripLane::Automation(t)
        );
    }
    assert!(serde_json::from_str::<StripLane>(r#"{"nope":{}}"#).is_err());
    assert_eq!(
        serde_json::to_string(&StripMode::Erase).unwrap(),
        r#""erase""#
    );
    assert_eq!(
        serde_json::to_string(&StripOp::LimitMax).unwrap(),
        r#""limitMax""#
    );
    assert_eq!(
        serde_json::from_str::<StripOp>(r#""limitMin""#).unwrap(),
        StripOp::LimitMin
    );
}

#[test]
fn lanes_read_their_series() {
    let note = note_row(1, 0, Q);
    let events = vec![
        note,
        cc_row(2, 0, 7, 100),
        cc_row(3, Q, 1, 5),
        automation_row(4, Q, T::StripFader { strip: 9 }, 0.5),
        automation_row(5, 2 * Q, T::StripPan { strip: 9 }, 1.),
    ];
    assert_eq!(
        StripLane::Velocity.values(&events, &[]),
        vec![StripValue {
            id: Some(1),
            tick: 0,
            length: Q,
            value: 0x8000,
            ramp: false
        }]
    );
    let values = |lane: StripLane| -> Vec<i64> {
        lane.values(&events, &[]).iter().map(|v| v.value).collect()
    };
    assert_eq!(values(StripLane::Duration), vec![Q]);
    let ids: Vec<Option<u64>> = StripLane::Controller(7)
        .values(&events, &[])
        .iter()
        .map(|v| v.id)
        .collect();
    assert_eq!(ids, vec![Some(2)]);
    assert_eq!(
        StripLane::Automation(T::StripFader { strip: 9 }).values(&events, &[]),
        vec![StripValue {
            id: Some(4),
            tick: Q,
            length: 0,
            value: v32(0.5),
            ramp: true
        }]
    );
    let prog = EventRow {
        value: 5,
        ..EventRow::blank(6, 0, EventKindRow::Program)
    };
    let prog_values: Vec<i64> = StripLane::Program
        .values(&[prog], &[])
        .iter()
        .map(|v| v.value)
        .collect();
    assert_eq!(prog_values, vec![5 << 25]);
    let tempo = [
        TempoPointRow {
            tick: 0,
            bpm: 120.,
            ramp: true,
        },
        TempoPointRow {
            tick: Q,
            bpm: 90.,
            ramp: false,
        },
    ];
    assert_eq!(
        StripLane::Tempo.values(&[], &tempo),
        vec![
            StripValue {
                id: None,
                tick: 0,
                length: 0,
                value: 120_000,
                ramp: true
            },
            StripValue {
                id: None,
                tick: Q,
                length: 0,
                value: 90_000,
                ramp: false
            },
        ]
    );
    assert_eq!(
        StripLane::Automation(T::StripMute { strip: 1 }).style(),
        StripStyle::Steps
    );
    assert_eq!(
        StripLane::Automation(T::StripFader { strip: 1 }).style(),
        StripStyle::Ramps
    );
    assert_eq!(StripLane::ReleaseVelocity.style(), StripStyle::Bars);
    assert_eq!(StripLane::Controller(1).style(), StripStyle::Steps);
    assert_eq!(StripLane::Tempo.style(), StripStyle::Ramps);
    // Ranges and centres (Swift :315-332).
    assert_eq!(StripLane::Velocity.range(), (0, 0xFFFF));
    assert_eq!(StripLane::Duration.range(), (0, 4 * Q));
    assert_eq!(StripLane::Tempo.range(), (30_000, 300_000));
    assert_eq!(StripLane::Controller(1).range(), (0, i64::from(u32::MAX)));
    assert_eq!(StripLane::PitchBend.center(), Some(0x8000_0000));
    assert_eq!(
        StripLane::Automation(T::StripPan { strip: 1 }).center(),
        Some(0x8000_0000)
    );
    assert_eq!(StripLane::Velocity.center(), None);
}

#[test]
fn lanes_name_targets_text_values_and_build_rules() {
    let names = StripNames {
        strips: [(1, "Synth".to_owned())].into(),
        sends: [(1, vec!["Reverb".to_owned()])].into(),
        slots: [(6, "EQ".to_owned())].into(),
        params: [(6, vec![param_row(2, "Gain", 0., 2., false)])].into(),
        dests: [(3, "Piano".to_owned())].into(),
        ..StripNames::default()
    };
    let a = StripLane::Automation;
    assert_eq!(a(T::StripFader { strip: 1 }).title(&names), "Fader · Synth");
    assert_eq!(a(T::StripPan { strip: 1 }).title(&names), "Pan · Synth");
    assert_eq!(a(T::StripMute { strip: 1 }).title(&names), "Mute · Synth");
    assert_eq!(a(T::StripSolo { strip: 1 }).title(&names), "Solo · Synth");
    assert_eq!(
        a(T::SendLevel { strip: 1, send: 0 }).title(&names),
        "Send 1 to Reverb · Synth"
    );
    assert_eq!(
        a(T::SendLevel { strip: 1, send: 1 }).title(&names),
        "Send 2 · Synth"
    );
    assert_eq!(
        a(T::PluginParam { slot: 6, param: 2 }).title(&names),
        "Gain · EQ"
    );
    assert_eq!(
        a(T::VelocityScale { dest: 3 }).title(&names),
        "Velocity Scale · Piano"
    );
    assert_eq!(
        a(T::StripFader { strip: 8 }).title(&names),
        "Fader · Strip 8"
    );
    assert_eq!(
        StripLane::Controller(7).title(&names),
        "Controller 7 (Volume)"
    );
    assert_eq!(StripLane::Controller(3).title(&names), "Controller 3");
    assert_eq!(
        StripLane::PolyPressure(60).title(&names),
        "Poly Pressure C4"
    );
    assert_eq!(StripLane::Duration.title(&names), "Length");
    assert_eq!(StripLane::ReleaseVelocity.title(&names), "Release Velocity");
    assert_eq!(StripLane::Tempo.title(&names), "Tempo");
    assert_eq!(
        StripLane::Velocity.text(i64::from(Velocity7::up(100)), &names),
        "100"
    );
    assert_eq!(
        StripLane::PitchBend.text(i64::from(Bend14::up(-8192)), &names),
        "-8192"
    );
    assert_eq!(StripLane::Program.text(9 << 25, &names), "9");
    assert_eq!(StripLane::Tempo.text(120_500, &names), "120.5 BPM");
    assert_eq!(
        a(T::PluginParam { slot: 6, param: 2 }).text(v32(0.25), &names),
        "0.5"
    );
    assert_eq!(
        tests_of(StripLane::Controller(7), 2 * Q, Q).unwrap(),
        vec![
            ConditionTest::TimeRange {
                start: Q,
                end: 2 * Q + 1
            },
            ConditionTest::Kinds {
                kinds: vec![EventKindRow::Cc]
            },
            ConditionTest::Controller { low: 7, high: 7 },
        ]
    );
    let rule = a(T::StripMute { strip: 1 }).erase_rule(0, 0).unwrap();
    assert!(rule.match_all);
    assert!(rule.conditions.iter().all(|c| !c.negate));
    assert_eq!(
        rule.conditions.last().map(|c| &c.test),
        Some(&ConditionTest::Automation {
            target: T::StripMute { strip: 1 }
        })
    );
    assert_eq!(StripLane::Velocity.erase_rule(0, 1), None);
    assert_eq!(StripLane::Tempo.erase_rule(0, 1), None);
    for lane in [
        StripLane::Velocity,
        StripLane::ReleaseVelocity,
        StripLane::Duration,
        StripLane::Controller(3),
        StripLane::PitchBend,
        StripLane::ChannelPressure,
        StripLane::PolyPressure(61),
        StripLane::Program,
        StripLane::Tempo,
        a(T::StripSolo { strip: 2 }),
    ] {
        assert_eq!(StripLane::from_line_target(lane.line_target()), Some(lane));
    }
    assert_eq!(
        StripLane::from_line_target(LineTargetRow::Value {
            target: ValueTargetRow::Value
        }),
        None
    );
    assert!(StripLane::Duration.is_note_value());
    assert!(!StripLane::Tempo.is_note_value());
    assert_eq!(StripLane::CONTROLLERS, [1, 2, 4, 7, 10, 11, 64, 74]);
}

#[test]
fn tempo_skips_non_finite_bpm() {
    let tempo = [
        TempoPointRow {
            tick: 0,
            bpm: f64::NAN,
            ramp: false,
        },
        TempoPointRow {
            tick: 1,
            bpm: f64::INFINITY,
            ramp: false,
        },
        TempoPointRow {
            tick: Q,
            bpm: 100.,
            ramp: false,
        },
    ];
    let v: Vec<i64> = StripLane::Tempo
        .values(&[], &tempo)
        .iter()
        .map(|v| v.value)
        .collect();
    assert_eq!(v, vec![100_000]);
}

#[test]
fn scale_clamps_outside_the_row() {
    let s = StripScale::new((0, 1000), 100., 106.);
    assert_eq!(s.inset, 3.);
    assert_eq!(s.y(1000), 103.);
    assert_eq!(s.y(0), 203.);
    assert_eq!(s.y(500), 153.);
    assert_eq!(s.value_at(153.), 500);
    assert_eq!(s.value_at(-500.), 1000, "above the row is the top");
    assert_eq!(s.value_at(900.), 0, "below the row is the bottom");
    assert_eq!(s.value_at(f32::NAN), 0);
    assert_eq!(s.fraction(f32::INFINITY), 0.);
    assert_eq!(s.amount_at(103., StripOp::Scale), 200);
    assert_eq!(s.amount_at(153., StripOp::Scale), 100);
    assert_eq!(s.amount_at(203., StripOp::Add), -500);
    assert_eq!(s.amount_at(103., StripOp::Add), 500);
    assert_eq!(s.amount_at(153., StripOp::LimitMax), 500);
    let odd = StripScale::new((0, 10), f32::NAN, -5.);
    assert_eq!((odd.top, odd.height), (0., 0.));
    assert!(StripOp::choices(StripLane::Duration).contains(&StripOp::Legato));
    assert!(!StripOp::choices(StripLane::Velocity).contains(&StripOp::Legato));
    assert!(StripOp::choices(StripLane::Controller(1)).contains(&StripOp::Thin));
    assert!(StripOp::choices(StripLane::Tempo).contains(&StripOp::Thicken));
    assert!(!StripOp::choices(StripLane::Velocity).contains(&StripOp::Thin));
    assert_eq!(StripOp::Compress.row(), LineOpRow::Compress);
    assert_eq!(StripOp::LimitMin.row(), LineOpRow::LimitMin);
    let titles: Vec<&str> = StripOp::ALL.iter().map(|o| o.title()).collect();
    assert_eq!(
        titles,
        [
            "Set",
            "Scale",
            "Add",
            "Max Limit",
            "Min Limit",
            "Compress",
            "Thin",
            "Thicken",
            "Legato"
        ]
    );
    let modes: Vec<&str> = StripMode::ALL.iter().map(|m| m.title()).collect();
    assert_eq!(modes, ["Draw", "Modify", "Erase"]);
}

#[test]
fn vca_fader_targets_are_named() {
    let t = T::VcaFader { vca: 3 };
    let mut names = StripNames::default();
    names.vcas.insert(3, "Drums".into());
    assert_eq!(StripLane::Automation(t).title(&names), "VCA Drums Fader");
    assert_eq!(short_title(&t), "VCA 3 Fader");
    assert_eq!(automation_text(&t, fader_norm(-6.), None), "-6.0 dB");
    assert!(
        StripNames::default().knows(&T::VcaFader { vca: 4 }),
        "no strips: every target passes"
    );
    names.strips.insert(1, "Synth".into());
    assert!(names.knows(&t));
    assert!(
        !names.knows(&T::VcaFader { vca: 4 }),
        "a deleted VCA is missing"
    );
    assert!(names.knows(&T::StripFader { strip: 1 }));
    assert!(!names.knows(&T::SendLevel { strip: 2, send: 0 }));
    assert!(!names.knows(&T::PluginParam { slot: 6, param: 1 }));
    assert!(!names.knows(&T::VelocityScale { dest: 3 }));
    assert_eq!(
        short_title(&T::PluginParam { slot: 2, param: 7 }),
        "Parameter 7 · Plugin 2"
    );
    assert_eq!(
        short_title(&T::VelocityScale { dest: 5 }),
        "Velocity Scale · Destination 5"
    );
}
