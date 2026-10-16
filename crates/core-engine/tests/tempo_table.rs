use core_engine::EngineError;
use core_engine::backend::MidiPortInfo;
use core_engine::schedule::TempoTable;
use core_engine::schedule::targets::{
    BuiltinRoute, MAX_TARGETS, TargetIdx, TargetKind, TargetTable, find_port, kinds_for,
};
use core_model::document::DestKind;
use core_model::ids::PluginInstanceId;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use proptest::prelude::*;

const SR: f64 = 48_000.0;
const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

#[test]
fn tempo_table_is_exact_for_constant_tempo() {
    let tempo = TempoMap::constant(Bpm::whole(120)).unwrap();
    let t = TempoTable::build(&tempo, Tick::ZERO, SR);
    assert_eq!(t.tick_to_frame(Tick(Q)), 24_000);
    assert_eq!(t.frame_to_tick(24_000), Tick(Q));
    assert_eq!(t.frame_to_tick(10 * 96_000), Tick(40 * Q));
    assert_eq!(t.tick_to_frame(Tick(-5)), 0);
}

#[test]
fn ramp_knots_are_at_most_a_sixteenth_apart() {
    let tempo = TempoMap::new(vec![
        TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(120),
            ramp: true,
        },
        TempoPoint {
            tick: Tick(4 * BAR),
            bpm: Bpm::whole(60),
            ramp: false,
        },
    ])
    .unwrap();
    let t = TempoTable::build(&tempo, Tick::ZERO, SR);
    let knots = t.knots();
    assert!(knots.len() > (4 * BAR / 960) as usize);
    for w in knots.windows(2) {
        assert!(w[1].tick - w[0].tick <= 960, "{:?}", w);
        assert!(w[1].frame > w[0].frame);
    }
}

#[test]
fn target_table_interns_and_caps_at_max_targets() {
    let mut t = TargetTable::new();
    assert_eq!(t.intern(TargetKind::BuiltinSynth).unwrap(), TargetIdx(0));
    assert_eq!(
        t.intern(TargetKind::MidiVirtualSource).unwrap(),
        TargetIdx(1)
    );
    assert_eq!(t.intern(TargetKind::BuiltinSynth).unwrap(), TargetIdx(0));
    for i in 0..MAX_TARGETS - 2 {
        let p = MidiPortInfo {
            name: format!("P{i}"),
            unique_id: None,
        };
        t.intern(TargetKind::MidiDestination(p)).unwrap();
    }
    assert_eq!(t.len(), MAX_TARGETS);
    let p = MidiPortInfo {
        name: "one too many".into(),
        unique_id: None,
    };
    assert_eq!(
        t.intern(TargetKind::MidiDestination(p)),
        Err(EngineError::TooManyTargets)
    );
    assert_eq!(t.get(TargetIdx(1)), Some(&TargetKind::MidiVirtualSource));
    assert_eq!(t.iter().count(), MAX_TARGETS);
    // A released index is reused only after `recycle` (the next load).
    t.release(TargetIdx(1));
    assert_eq!(t.get(TargetIdx(1)), None);
    assert_eq!(t.next_index(), None, "released, not free");
    t.recycle();
    assert_eq!(t.next_index(), Some(TargetIdx(1)));
    assert_eq!(
        t.intern(TargetKind::PluginInstrument(PluginInstanceId(7))),
        Ok(TargetIdx(1))
    );
}

#[test]
fn endpoint_refs_map_to_target_kinds() {
    let ports = vec![
        MidiPortInfo {
            name: "IAC Bus 1".into(),
            unique_id: Some(11),
        },
        MidiPortInfo {
            name: "Synth".into(),
            unique_id: Some(-5),
        },
    ];
    let ep = |r: &str| DestKind::MidiEndpoint {
        endpoint_ref: r.into(),
        group: 0,
        channel: 0,
    };
    assert_eq!(
        kinds_for(&ep("builtin:synth"), BuiltinRoute::Builtin, &ports),
        Ok(vec![TargetKind::BuiltinSynth])
    );
    assert_eq!(
        kinds_for(&ep("builtin:synth"), BuiltinRoute::Virtual, &ports),
        Ok(vec![TargetKind::MidiVirtualSource])
    );
    assert_eq!(
        kinds_for(&ep("builtin:synth"), BuiltinRoute::Both, &ports),
        Ok(vec![
            TargetKind::BuiltinSynth,
            TargetKind::MidiVirtualSource
        ])
    );
    assert_eq!(
        kinds_for(&ep("virtual:out"), BuiltinRoute::Both, &ports),
        Ok(vec![TargetKind::MidiVirtualSource])
    );
    assert_eq!(
        kinds_for(&ep("Synth"), BuiltinRoute::Builtin, &ports),
        Ok(vec![TargetKind::MidiDestination(ports[1].clone())])
    );
    assert!(
        kinds_for(&ep("Nope"), BuiltinRoute::Builtin, &ports)
            .unwrap_err()
            .contains("Nope")
    );
    let plugin = DestKind::plugin("au:x");
    assert_eq!(
        kinds_for(&plugin, BuiltinRoute::Builtin, &ports),
        Ok(vec![TargetKind::PluginInstrument(PluginInstanceId(0))])
    );
    assert_eq!(find_port("11", &ports), Some(&ports[0]));
    assert_eq!(find_port("-5", &ports), Some(&ports[1]));
    assert_eq!(find_port("12", &ports), None);
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
    fn table_lookups_stay_within_one(tempo in arb_tempo(), probes in prop::collection::vec(0i64..16 * BAR, 1..40)) {
        let table = TempoTable::build(&tempo, Tick::ZERO, SR);
        for t in probes {
            let secs = tempo.tick_to_seconds(Tick(t));
            let frame_exact = secs * SR;
            let f = table.tick_to_frame(Tick(t));
            prop_assert!((f as f64 - frame_exact).abs() <= 1.0, "tick {} -> {} vs {}", t, f, frame_exact);
            let frame = frame_exact.round() as u64;
            let back = table.frame_to_tick(frame);
            let want = tempo.seconds_to_tick(frame as f64 / SR);
            prop_assert!((back.0 - want.0).abs() <= 1, "frame {} -> {:?} vs {:?}", frame, back, want);
        }
    }
}
