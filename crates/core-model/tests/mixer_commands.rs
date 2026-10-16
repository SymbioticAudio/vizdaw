use core_model::ModelError;
use core_model::command::{Command, MixerCmd, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, PluginInstanceId, StripId};
use core_model::mixer::{AuxSend, Output, ParamId, PluginRef, StripKind};
use core_model::mixer_rules::audible;
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use proptest::prelude::*;

fn mx(d: &Document, c: MixerCmd) -> core_model::Result<Document> {
    apply(d, &Command::Mixer(c)).map(|a| a.doc)
}

/// master 0, builtin synth strip 1 (dest0), Pad instrument strip 2 (dest1,
/// plugin 0), audio path strip 3 (out0), buses 4 "A" and 5 "B".
fn base() -> Document {
    let mut d = Document::new();
    for c in [
        RoutingCmd::AddDestination {
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "builtin:synth".into(),
                group: 0,
                channel: 0,
            },
            default_patch: None,
        },
        RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        },
        RoutingCmd::AddOutput {
            name: "Main".into(),
            reference: "bus:0".into(),
        },
    ] {
        d = apply(&d, &Command::Routing(c)).unwrap().doc;
    }
    for name in ["A", "B"] {
        d = mx(
            &d,
            MixerCmd::AddBus {
                name: name.into(),
                index: None,
            },
        )
        .unwrap();
    }
    d
}

fn plug(name: &str) -> PluginRef {
    PluginRef::from_legacy(name)
}

#[test]
fn base_layout() {
    let d = base();
    let kinds: Vec<StripKind> = d.mixer().strips().values().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        vec![
            StripKind::Master,
            StripKind::BuiltinSynth,
            StripKind::Instrument(DestId(1)),
            StripKind::AudioPath(core_model::ids::OutputPathId(0)),
            StripKind::Bus,
            StripKind::Bus,
        ]
    );
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
}

#[test]
fn faders_pans_and_names() {
    let d = base();
    let d = mx(
        &d,
        MixerCmd::SetGain {
            strip: StripId(1),
            gain_db: -6.0,
        },
    )
    .unwrap();
    let d = mx(
        &d,
        MixerCmd::SetPan {
            strip: StripId(1),
            pan: -0.5,
        },
    )
    .unwrap();
    let d = mx(
        &d,
        MixerCmd::RenameStrip {
            strip: StripId(4),
            name: "Verb".into(),
        },
    )
    .unwrap();
    let d = mx(
        &d,
        MixerCmd::MoveStrip {
            strip: StripId(5),
            index: 0,
        },
    )
    .unwrap();
    let s = d.mixer().strip(StripId(1)).unwrap();
    assert_eq!((s.gain_db, s.pan), (-6.0, -0.5));
    assert_eq!(d.mixer().strip(StripId(4)).unwrap().name, "Verb");
    assert_eq!(d.mixer().order().front(), Some(&StripId(5)));
    assert!(matches!(
        mx(
            &d,
            MixerCmd::SetGain {
                strip: StripId(1),
                gain_db: 12.5
            }
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(
        mx(
            &d,
            MixerCmd::SetPan {
                strip: StripId(1),
                pan: f64::NAN
            }
        )
        .is_err()
    );
    assert!(
        mx(
            &d,
            MixerCmd::MoveStrip {
                strip: StripId(0),
                index: 0
            }
        )
        .is_err()
    );
}

#[test]
fn output_cycle_is_refused_and_changes_nothing() {
    let d = mx(
        &base(),
        MixerCmd::SetOutput {
            strip: StripId(4),
            output: Output::Bus(StripId(5)),
        },
    )
    .unwrap();
    let r = apply(
        &d,
        &Command::Mixer(MixerCmd::SetOutput {
            strip: StripId(5),
            output: Output::Bus(StripId(4)),
        }),
    );
    assert!(matches!(r, Err(ModelError::RoutingCycle { .. })), "{r:?}");
    // A send that closes the same loop is refused too.
    let r = mx(
        &d,
        MixerCmd::AddSend {
            strip: StripId(5),
            send: AuxSend::new(StripId(4)),
        },
    );
    assert!(matches!(r, Err(ModelError::RoutingCycle { .. })));
    // Targets: no sends to non-buses, no output for master.
    let r = mx(
        &d,
        MixerCmd::AddSend {
            strip: StripId(1),
            send: AuxSend::new(StripId(2)),
        },
    );
    assert!(matches!(r, Err(ModelError::InvalidRoutingTarget { .. })));
    let r = mx(
        &d,
        MixerCmd::SetOutput {
            strip: StripId(0),
            output: Output::Bus(StripId(4)),
        },
    );
    assert!(matches!(r, Err(ModelError::InvalidRoutingTarget { .. })));
}

#[test]
fn sends_add_edit_remove() {
    let d = mx(
        &base(),
        MixerCmd::AddSend {
            strip: StripId(1),
            send: AuxSend::new(StripId(4)),
        },
    )
    .unwrap();
    let mut s = AuxSend::new(StripId(4));
    s.level_db = -12.0;
    s.pre_fader = true;
    let d = mx(
        &d,
        MixerCmd::EditSend {
            strip: StripId(1),
            index: 0,
            send: s,
        },
    )
    .unwrap();
    assert_eq!(d.mixer().strip(StripId(1)).unwrap().sends.front(), Some(&s));
    assert!(
        mx(
            &d,
            MixerCmd::EditSend {
                strip: StripId(1),
                index: 3,
                send: s
            }
        )
        .is_err()
    );
    let d = mx(
        &d,
        MixerCmd::RemoveSend {
            strip: StripId(1),
            index: 0,
        },
    )
    .unwrap();
    assert!(d.mixer().strip(StripId(1)).unwrap().sends.is_empty());
}

#[test]
fn inserts_add_move_bypass_and_remove() {
    let a = apply(
        &base(),
        &Command::Mixer(MixerCmd::AddInsert {
            strip: StripId(1),
            index: None,
            plugin: plug("clap:eq"),
        }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Plugin(PluginInstanceId(1))]);
    let d = mx(
        &a.doc,
        MixerCmd::AddInsert {
            strip: StripId(1),
            index: Some(0),
            plugin: plug("clap:comp"),
        },
    )
    .unwrap();
    let ids: Vec<PluginInstanceId> = d
        .mixer()
        .strip(StripId(1))
        .unwrap()
        .inserts
        .iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, vec![PluginInstanceId(2), PluginInstanceId(1)]);
    let d = mx(
        &d,
        MixerCmd::MoveInsert {
            slot: PluginInstanceId(2),
            strip: StripId(4),
            index: 9,
        },
    )
    .unwrap();
    assert_eq!(
        d.mixer()
            .strip(StripId(4))
            .unwrap()
            .inserts
            .front()
            .map(|p| p.id),
        Some(PluginInstanceId(2))
    );
    let d = mx(
        &d,
        MixerCmd::SetBypass {
            slot: PluginInstanceId(2),
            bypass: true,
        },
    )
    .unwrap();
    assert!(d.plugin_slot(PluginInstanceId(2)).unwrap().bypass);
    let r = apply(
        &d,
        &Command::Mixer(MixerCmd::RemoveInsert {
            slot: PluginInstanceId(2),
        }),
    )
    .unwrap();
    assert_eq!(r.report.removed, vec![AnyId::Plugin(PluginInstanceId(2))]);
    // The instrument's own slot is not an insert.
    assert!(
        mx(
            &d,
            MixerCmd::RemoveInsert {
                slot: PluginInstanceId(0)
            }
        )
        .is_err()
    );
    assert!(
        mx(
            &d,
            MixerCmd::SetSidechain {
                slot: PluginInstanceId(0),
                source: Some(StripId(1))
            }
        )
        .is_err()
    );
    assert!(matches!(
        mx(
            &d,
            MixerCmd::SetBypass {
                slot: PluginInstanceId(42),
                bypass: true
            }
        ),
        Err(ModelError::UnknownId(AnyId::Plugin(PluginInstanceId(42))))
    ));
}

#[test]
fn sidechain_cycles_are_refused() {
    let d = mx(
        &base(),
        MixerCmd::AddInsert {
            strip: StripId(4),
            index: None,
            plugin: plug("clap:duck"),
        },
    )
    .unwrap();
    let p = PluginInstanceId(1);
    let d2 = mx(
        &d,
        MixerCmd::SetSidechain {
            slot: p,
            source: Some(StripId(1)),
        },
    )
    .unwrap();
    assert_eq!(d2.plugin_slot(p).unwrap().sidechain, Some(StripId(1)));
    for bad in [StripId(4), StripId(0)] {
        let r = mx(
            &d,
            MixerCmd::SetSidechain {
                slot: p,
                source: Some(bad),
            },
        );
        assert!(
            matches!(r, Err(ModelError::RoutingCycle { .. })),
            "{bad}: {r:?}"
        );
    }
    assert!(matches!(
        mx(
            &d,
            MixerCmd::SetSidechain {
                slot: p,
                source: Some(StripId(99))
            }
        ),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn removing_a_bus_retargets_and_undo_restores() {
    let mut h = History::new(base());
    let f = ViewFilter::default();
    for c in [
        MixerCmd::SetOutput {
            strip: StripId(1),
            output: Output::Bus(StripId(4)),
        },
        MixerCmd::AddSend {
            strip: StripId(2),
            send: AuxSend::new(StripId(4)),
        },
        MixerCmd::AddInsert {
            strip: StripId(5),
            index: None,
            plugin: plug("clap:duck"),
        },
        MixerCmd::SetSidechain {
            slot: PluginInstanceId(1),
            source: Some(StripId(4)),
        },
    ] {
        h.execute(&Command::Mixer(c), &f).unwrap();
    }
    let before = h.doc().clone();
    let r = h
        .execute(
            &Command::Mixer(MixerCmd::RemoveBus { strip: StripId(4) }),
            &f,
        )
        .unwrap();
    assert_eq!(r.removed, vec![AnyId::Strip(StripId(4))]);
    let m = h.doc().mixer();
    assert_eq!(m.strip(StripId(1)).unwrap().output, Output::Master);
    assert!(m.strip(StripId(2)).unwrap().sends.is_empty());
    assert_eq!(
        h.doc().plugin_slot(PluginInstanceId(1)).unwrap().sidechain,
        None
    );
    assert!(validate(h.doc()).is_empty());
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
    // Only buses are removed directly.
    assert!(mx(&before, MixerCmd::RemoveBus { strip: StripId(1) }).is_err());
}

#[test]
fn plugin_params_gesture_is_one_undo_step() {
    let mut h = History::new(base());
    let f = ViewFilter::default();
    let depth = h.undo_depth();
    h.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot: PluginInstanceId(0),
            params: vec![(ParamId(1), 0.25), (ParamId(7), 1.0), (ParamId(1), 0.5)],
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.undo_depth(), depth + 1);
    let slot = h.doc().plugin_slot(PluginInstanceId(0)).unwrap();
    assert_eq!(slot.params.get(&ParamId(1)), Some(&0.5));
    assert_eq!(slot.params.get(&ParamId(7)), Some(&1.0));
    assert!(
        apply(
            h.doc(),
            &Command::Mixer(MixerCmd::SetPluginParams {
                slot: PluginInstanceId(0),
                params: vec![(ParamId(2), f64::INFINITY)]
            })
        )
        .is_err()
    );
    h.undo().unwrap();
    assert!(
        h.doc()
            .plugin_slot(PluginInstanceId(0))
            .unwrap()
            .params
            .is_empty()
    );
}

#[test]
fn instrument_outputs_and_solo() {
    let a = apply(
        &base(),
        &Command::Mixer(MixerCmd::AddInstrumentOutput {
            dest: DestId(1),
            index: 1,
            name: "Pad 3-4".into(),
        }),
    )
    .unwrap();
    assert_eq!(a.report.created, vec![AnyId::Strip(StripId(6))]);
    assert_eq!(
        a.doc.mixer().strip(StripId(6)).unwrap().kind,
        StripKind::InstrumentOutput(DestId(1), 1)
    );
    for bad in [(DestId(1), 1), (DestId(1), 0), (DestId(0), 1)] {
        assert!(
            mx(
                &a.doc,
                MixerCmd::AddInstrumentOutput {
                    dest: bad.0,
                    index: bad.1,
                    name: "x".into()
                }
            )
            .is_err()
        );
    }
    let d = mx(
        &a.doc,
        MixerCmd::SetSolo {
            strip: StripId(1),
            solo: true,
        },
    )
    .unwrap();
    let d = mx(
        &d,
        MixerCmd::SetMute {
            strip: StripId(2),
            mute: true,
        },
    )
    .unwrap();
    assert!(d.mixer().strip(StripId(2)).unwrap().mute);
    let set = audible(d.mixer());
    assert!(set.contains(&StripId(1)) && set.contains(&StripId(0)) && !set.contains(&StripId(4)));
}

fn arb_cmd() -> impl Strategy<Value = MixerCmd> {
    let strip = (0u64..8).prop_map(StripId);
    let slot = (0u64..5).prop_map(PluginInstanceId);
    let out = prop_oneof![
        Just(Output::Master),
        (0u64..8).prop_map(|b| Output::Bus(StripId(b)))
    ];
    prop_oneof![
        (0usize..4).prop_map(|i| MixerCmd::AddBus {
            name: format!("Bus{i}"),
            index: Some(i)
        }),
        strip
            .clone()
            .prop_map(|strip| MixerCmd::RemoveBus { strip }),
        (strip.clone(), -150.0f64..14.0)
            .prop_map(|(strip, gain_db)| MixerCmd::SetGain { strip, gain_db }),
        (strip.clone(), -1.2f64..1.2).prop_map(|(strip, pan)| MixerCmd::SetPan { strip, pan }),
        (strip.clone(), any::<bool>()).prop_map(|(strip, mute)| MixerCmd::SetMute { strip, mute }),
        (strip.clone(), any::<bool>()).prop_map(|(strip, solo)| MixerCmd::SetSolo { strip, solo }),
        (strip.clone(), out).prop_map(|(strip, output)| MixerCmd::SetOutput { strip, output }),
        (strip.clone(), 0u64..8, any::<bool>()).prop_map(|(strip, t, pre)| {
            let mut send = AuxSend::new(StripId(t));
            send.pre_fader = pre;
            MixerCmd::AddSend { strip, send }
        }),
        (strip.clone(), 0usize..2).prop_map(|(strip, index)| MixerCmd::RemoveSend { strip, index }),
        strip.clone().prop_map(|strip| MixerCmd::AddInsert {
            strip,
            index: None,
            plugin: PluginRef::from_legacy("clap:fx")
        }),
        slot.clone()
            .prop_map(|slot| MixerCmd::RemoveInsert { slot }),
        (slot.clone(), strip.clone(), 0usize..3)
            .prop_map(|(slot, strip, index)| MixerCmd::MoveInsert { slot, strip, index }),
        (slot.clone(), any::<bool>())
            .prop_map(|(slot, bypass)| MixerCmd::SetBypass { slot, bypass }),
        (slot.clone(), proptest::option::of(strip.clone()))
            .prop_map(|(slot, source)| MixerCmd::SetSidechain { slot, source }),
        (slot, 0u64..4, -1.0f64..1.0).prop_map(|(slot, p, v)| MixerCmd::SetPluginParams {
            slot,
            params: vec![(ParamId(p), v)]
        }),
        (strip, 0usize..8).prop_map(|(strip, index)| MixerCmd::MoveStrip { strip, index }),
    ]
}

proptest! {
    #[test]
    fn mixer_commands_undo_to_the_original(cmds in proptest::collection::vec(arb_cmd(), 1..40)) {
        let original = base();
        let mut h = History::new(original.clone());
        let f = ViewFilter::default();
        for c in &cmds {
            let _ = h.execute(&Command::Mixer(c.clone()), &f);
            prop_assert!(validate(h.doc()).is_empty(), "{:?} after {:?}", validate(h.doc()), c);
        }
        let after = h.doc().clone();
        while h.undo().is_ok() {}
        prop_assert_eq!(h.doc(), &original);
        while h.redo().is_ok() {}
        prop_assert_eq!(h.doc(), &after);
    }
}
