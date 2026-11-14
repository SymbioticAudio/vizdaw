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

fn swatch() -> impl Strategy<Value = core_model::mixer::Swatch> {
    use core_model::mixer::Swatch;
    prop_oneof![
        Just(Swatch::Red),
        Just(Swatch::Orange),
        Just(Swatch::Yellow),
        Just(Swatch::Green),
        Just(Swatch::Teal),
        Just(Swatch::Blue),
        Just(Swatch::Purple),
        Just(Swatch::Pink),
    ]
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
        (slot.clone(), 0u64..4, -1.0f64..1.0).prop_map(|(slot, p, v)| {
            MixerCmd::SetPluginParams {
                slot,
                params: vec![(ParamId(p), v)],
            }
        }),
        (slot, proptest::collection::vec(any::<u8>(), 0..=64)).prop_map(|(slot, bytes)| {
            MixerCmd::SetPluginState {
                slot,
                bytes: std::sync::Arc::from(bytes),
            }
        }),
        (strip.clone(), 0usize..8).prop_map(|(strip, index)| MixerCmd::MoveStrip { strip, index }),
        (
            strip.clone(),
            proptest::option::of(Just(PluginRef::from_legacy("clap:eq")))
        )
            .prop_map(|(strip, plugin)| MixerCmd::SetEq { strip, plugin }),
        (strip, proptest::option::of(swatch()))
            .prop_map(|(strip, colour)| MixerCmd::SetColour { strip, colour }),
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

/// Sub-project 5 spec §4.6, plan 5e-1 Ruling 2: the mode is set on any
/// number of strips as one undo step, refused for none or an unknown
/// strip (a strip listed twice is set once), and saved only when it is
/// not Read (older files and all-Read projects are unchanged).
#[test]
fn the_automation_mode_is_one_undo_step_and_saved_only_when_not_read() {
    use core_model::automation::AutomationMode;
    use core_model::file::{from_json, to_json};
    let d = base();
    let plain = to_json(&d).unwrap();
    assert!(!plain.contains("\"automation\""), "Read is not written");
    assert!(
        d.mixer()
            .strips()
            .values()
            .all(|s| s.automation == AutomationMode::Read)
    );
    let mut h = History::new(d.clone());
    h.execute(
        &Command::Mixer(MixerCmd::SetAutomationMode {
            strips: vec![StripId(1), StripId(4)],
            mode: AutomationMode::Touch,
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_label(), Some("Set Automation Mode"));
    let modes = |d: &Document| -> Vec<(u64, AutomationMode)> {
        d.mixer()
            .strips()
            .values()
            .map(|s| (s.id.0, s.automation))
            .collect()
    };
    let touched = modes(h.doc());
    assert!(
        touched.contains(&(1, AutomationMode::Touch))
            && touched.contains(&(4, AutomationMode::Touch))
    );
    assert!(touched.contains(&(5, AutomationMode::Read)));
    // Saved and read back.
    let text = to_json(h.doc()).unwrap();
    assert!(text.contains("\"automation\": \"touch\""), "{text}");
    assert_eq!(modes(&from_json(&text).unwrap()), touched);
    h.undo().unwrap();
    assert_eq!(to_json(h.doc()).unwrap(), plain);
    // A strip listed twice is set once (pre-flight M21). Setting it twice
    // to one mode would look the same, so this half of the dedupe is not
    // observable here; the app's half is pinned by `"1,1"` (Task 10).
    let twice = mx(
        &d,
        MixerCmd::SetAutomationMode {
            strips: vec![StripId(1), StripId(1)],
            mode: AutomationMode::Latch,
        },
    )
    .unwrap();
    assert_eq!(
        twice.mixer().strip(StripId(1)).unwrap().automation,
        AutomationMode::Latch
    );
    // Refused, each for its own reason (Task 2 review M2): no strips, an
    // unknown strip (nothing changes).
    let refused = |strips: Vec<StripId>| {
        mx(
            &d,
            MixerCmd::SetAutomationMode {
                strips,
                mode: AutomationMode::Off,
            },
        )
        .unwrap_err()
    };
    assert!(
        matches!(refused(vec![]), ModelError::Invalid { .. }),
        "no strips"
    );
    assert_eq!(
        refused(vec![StripId(1), StripId(99)]),
        ModelError::UnknownId(AnyId::Strip(StripId(99)))
    );
    assert_eq!(
        AutomationMode::ALL.map(AutomationMode::name),
        ["Off", "Read", "Touch", "Latch", "Write", "Trim"]
    );
    assert!(AutomationMode::ALL.iter().filter(|m| m.writes()).count() == 4);
}

/// Plan 5e-1 Ruling 3: a target's strip: its own controls and sends, an
/// insert's strip, a plugin instrument's strip; velocity scale has none.
#[test]
fn every_target_names_the_strip_whose_mode_governs_it() {
    use core_model::automation::{AutomationTarget as T, SendIndex, strip_of, target_exists};
    let mut d = base();
    let plugin = PluginRef {
        format: core_model::mixer::PluginFormat::Clap,
        id: "vizdaw.test:gain=1".into(),
        name: "Gain".into(),
        vendor: "V".into(),
        version: "1".into(),
    };
    d = mx(
        &d,
        MixerCmd::AddInsert {
            strip: StripId(4),
            index: None,
            plugin,
        },
    )
    .unwrap();
    let insert = d.mixer().strip(StripId(4)).unwrap().inserts[0].id;
    let pad = match &d.instruments().get(&DestId(1)).unwrap().kind {
        DestKind::PluginInstrument { slot } => slot.id,
        _ => unreachable!(),
    };
    assert_eq!(strip_of(&d, T::StripFader(StripId(3))), Some(StripId(3)));
    assert_eq!(
        strip_of(&d, T::SendLevel(StripId(1), SendIndex(0))),
        Some(StripId(1))
    );
    assert_eq!(
        strip_of(&d, T::PluginParam(insert, ParamId(0))),
        Some(StripId(4))
    );
    assert_eq!(
        strip_of(&d, T::PluginParam(pad, ParamId(0))),
        Some(StripId(2))
    );
    assert_eq!(strip_of(&d, T::VelocityScale(DestId(0))), None);
    assert_eq!(
        strip_of(&d, T::PluginParam(PluginInstanceId(77), ParamId(0))),
        None
    );
    assert!(target_exists(&d, T::StripMute(StripId(5))));
    assert!(!target_exists(&d, T::StripMute(StripId(50))));
    assert!(!target_exists(&d, T::SendLevel(StripId(1), SendIndex(3))));
}

/// The chain ids of `strip` in `d`: the EQ, then the inserts.
fn chain_ids(d: &Document, strip: StripId) -> Vec<PluginInstanceId> {
    d.mixer()
        .strip(strip)
        .unwrap()
        .chain()
        .map(|p| p.id)
        .collect()
}

fn set_eq(strip: StripId, plugin: Option<&str>) -> Command {
    Command::Mixer(MixerCmd::SetEq {
        strip,
        plugin: plugin.map(plug),
    })
}

/// Plan 7b Ruling 7: `SetEq` loads, replaces and empties the EQ slot, each
/// one undo step titled "Load EQ" or "Remove EQ"; an empty slot cannot be
/// emptied again; master has one too.
#[test]
fn the_eq_slot_loads_replaces_and_empties_as_undo_steps() {
    let s = StripId(1);
    let mut h = History::new(base());
    let f = ViewFilter::default();
    let c = set_eq(s, Some("eq-a"));
    assert_eq!(c.name(), "Load EQ");
    let r = h.execute(&c, &f).unwrap();
    let id1 = PluginInstanceId(1);
    assert_eq!(r.created, vec![AnyId::Plugin(id1)]);
    assert_eq!(h.undo_label(), Some("Load EQ"));
    let eq = h.doc().mixer().strip(s).unwrap().eq.clone().unwrap();
    assert_eq!((eq.id, eq.plugin.id.as_str()), (id1, "eq-a"));
    let after_a = h.doc().clone();

    let r = h.execute(&set_eq(s, Some("eq-b")), &f).unwrap();
    let id2 = match r.created.as_slice() {
        [AnyId::Plugin(p)] => *p,
        other => panic!("{other:?}"),
    };
    assert_ne!(id2, id1);
    assert_eq!(r.removed, vec![AnyId::Plugin(id1)]);
    let eq = h.doc().mixer().strip(s).unwrap().eq.clone().unwrap();
    assert_eq!((eq.id, eq.plugin.id.as_str()), (id2, "eq-b"));

    let c = set_eq(s, None);
    assert_eq!(c.name(), "Remove EQ");
    let r = h.execute(&c, &f).unwrap();
    assert_eq!(r.removed, vec![AnyId::Plugin(id2)]);
    assert_eq!(h.undo_label(), Some("Remove EQ"));
    assert_eq!(h.doc().mixer().strip(s).unwrap().eq, None);
    assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));

    let before = h.doc().clone();
    let again = apply(&before, &set_eq(s, None));
    assert!(
        matches!(&again, Err(ModelError::Invalid { reason }) if reason == "the EQ slot is empty"),
        "{again:?}"
    );
    assert_eq!(h.doc(), &before);

    // Each was one undo step.
    h.undo().unwrap();
    h.undo().unwrap();
    assert_eq!(h.doc(), &after_a);
    h.undo().unwrap();
    assert_eq!(h.doc(), &base());

    // Master takes an EQ too; an unknown strip is refused.
    let m = base().mixer().master();
    let d = apply(&base(), &set_eq(m, Some("eq-m"))).unwrap().doc;
    assert!(d.mixer().strip(m).unwrap().eq.is_some());
    assert!(validate(&d).is_empty(), "{:?}", validate(&d));
    assert!(matches!(
        apply(&base(), &set_eq(StripId(99), Some("eq"))),
        Err(ModelError::UnknownId(AnyId::Strip(StripId(99))))
    ));
}

/// Ruling 7: insert indices count inserts only, so no insert move or add
/// reaches the EQ slot.
#[test]
fn the_eq_slot_stays_first_when_inserts_move() {
    let s = StripId(1);
    let mut d = apply(&base(), &set_eq(s, Some("eq"))).unwrap().doc;
    let eq = PluginInstanceId(1);
    for name in ["a", "b", "c"] {
        d = mx(
            &d,
            MixerCmd::AddInsert {
                strip: s,
                index: None,
                plugin: plug(name),
            },
        )
        .unwrap();
    }
    let (a, b, c) = (
        PluginInstanceId(2),
        PluginInstanceId(3),
        PluginInstanceId(4),
    );
    assert_eq!(chain_ids(&d, s), vec![eq, a, b, c]);
    let eq_before = d.mixer().strip(s).unwrap().eq.clone();
    let d = mx(
        &d,
        MixerCmd::MoveInsert {
            slot: c,
            strip: s,
            index: 0,
        },
    )
    .unwrap();
    assert_eq!(chain_ids(&d, s), vec![eq, c, a, b]);
    assert_eq!(d.mixer().strip(s).unwrap().eq, eq_before);
    let r = apply(
        &d,
        &Command::Mixer(MixerCmd::AddInsert {
            strip: s,
            index: Some(0),
            plugin: plug("first"),
        }),
    )
    .unwrap();
    let n = PluginInstanceId(5);
    assert_eq!(r.report.created, vec![AnyId::Plugin(n)]);
    assert_eq!(chain_ids(&r.doc, s), vec![eq, n, c, a, b]);
}

/// Ruling 7: the EQ is not an insert (no move, no remove), but bypass,
/// parameters, sidechain and automation reach it.
#[test]
fn the_eq_slot_is_not_an_insert() {
    use core_model::automation::{AutomationTarget, strip_of};
    let s = StripId(1);
    let d = apply(&base(), &set_eq(s, Some("eq"))).unwrap().doc;
    let eq = PluginInstanceId(1);
    for c in [
        MixerCmd::MoveInsert {
            slot: eq,
            strip: StripId(4),
            index: 0,
        },
        MixerCmd::RemoveInsert { slot: eq },
    ] {
        let r = mx(&d, c.clone());
        assert!(
            matches!(&r, Err(ModelError::Invalid { reason }) if reason == "the EQ slot is not an insert"),
            "{c:?}: {r:?}"
        );
    }
    let d2 = mx(
        &d,
        MixerCmd::SetBypass {
            slot: eq,
            bypass: true,
        },
    )
    .unwrap();
    assert!(d2.plugin_slot(eq).unwrap().bypass);
    let d2 = mx(
        &d2,
        MixerCmd::SetPluginParams {
            slot: eq,
            params: vec![(ParamId(3), 0.75)],
        },
    )
    .unwrap();
    assert_eq!(
        d2.plugin_slot(eq).unwrap().params.get(&ParamId(3)),
        Some(&0.75)
    );
    let d2 = mx(
        &d2,
        MixerCmd::SetSidechain {
            slot: eq,
            source: Some(StripId(3)),
        },
    )
    .unwrap();
    assert_eq!(d2.plugin_slot(eq).unwrap().sidechain, Some(StripId(3)));
    assert!(validate(&d2).is_empty(), "{:?}", validate(&d2));
    // A sidechain from its own strip is still a cycle.
    assert!(matches!(
        mx(
            &d,
            MixerCmd::SetSidechain {
                slot: eq,
                source: Some(s)
            }
        ),
        Err(ModelError::RoutingCycle { .. })
    ));
    // A non-finite EQ value is refused, and validate checks EQ values.
    assert!(
        mx(
            &d,
            MixerCmd::SetPluginParams {
                slot: eq,
                params: vec![(ParamId(3), f64::NAN)]
            }
        )
        .is_err()
    );
    assert_eq!(
        strip_of(&d2, AutomationTarget::PluginParam(eq, ParamId(3))),
        Some(s)
    );
}

/// Ruling 7: every "all slots" lookup sees the EQ.
#[test]
fn every_slot_lookup_sees_the_eq() {
    use core_model::document::SlotOwner;
    let s = StripId(1);
    let d = apply(&base(), &set_eq(s, Some("eq"))).unwrap().doc;
    let eq = PluginInstanceId(1);
    let d = mx(
        &d,
        MixerCmd::AddInsert {
            strip: s,
            index: None,
            plugin: plug("a"),
        },
    )
    .unwrap();
    let a = PluginInstanceId(2);
    assert_eq!(d.find_slot(eq), Some(SlotOwner::Eq(s)));
    assert_eq!(d.find_slot(a), Some(SlotOwner::Insert(s, 0)));
    assert_eq!(d.plugin_slot(eq).unwrap().plugin.id, "eq");
    let slots: Vec<(StripId, PluginInstanceId)> =
        d.mixer().slots().map(|(st, p)| (st, p.id)).collect();
    assert_eq!(slots, vec![(s, eq), (s, a)]);
    // Inserts alone are unchanged.
    let inserts: Vec<PluginInstanceId> = d.mixer().inserts().map(|(_, _, p)| p.id).collect();
    assert_eq!(inserts, vec![a]);
    let (d, blob) = core_model::state::capture(&d, eq, b"eq state").unwrap();
    assert!(d.referenced_states().contains(&blob));
    assert_eq!(d.state_blob(blob), Some(&b"eq state"[..]));
    assert!(d.missing_states().is_empty());
}

#[test]
fn removing_a_bus_clears_an_eq_sidechain_from_it() {
    let mut h = History::new(base());
    let f = ViewFilter::default();
    h.execute(&set_eq(StripId(5), Some("eq")), &f).unwrap();
    let eq = PluginInstanceId(1);
    h.execute(
        &Command::Mixer(MixerCmd::SetSidechain {
            slot: eq,
            source: Some(StripId(4)),
        }),
        &f,
    )
    .unwrap();
    let before = h.doc().clone();
    h.execute(
        &Command::Mixer(MixerCmd::RemoveBus { strip: StripId(4) }),
        &f,
    )
    .unwrap();
    assert_eq!(h.doc().plugin_slot(eq).unwrap().sidechain, None);
    assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));
    h.undo().unwrap();
    assert_eq!(h.doc(), &before);
}

/// C-1, Ruling 12: a strip colour is one undo step, saved, and cleared by
/// `None`; an unknown strip is refused.
#[test]
fn a_strip_colour_is_set_saved_and_undone() {
    use core_model::file::{from_json, to_json};
    use core_model::mixer::Swatch;
    let s = StripId(2);
    let mut h = History::new(base());
    let f = ViewFilter::default();
    let c = Command::Mixer(MixerCmd::SetColour {
        strip: s,
        colour: Some(Swatch::Red),
    });
    assert_eq!(c.name(), "Set Strip Colour");
    h.execute(&c, &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Strip Colour"));
    assert_eq!(h.doc().mixer().strip(s).unwrap().colour, Some(Swatch::Red));
    let back = from_json(&to_json(h.doc()).unwrap()).unwrap();
    assert_eq!(&back, h.doc());
    h.undo().unwrap();
    assert_eq!(h.doc().mixer().strip(s).unwrap().colour, None);
    h.redo().unwrap();
    h.execute(
        &Command::Mixer(MixerCmd::SetColour {
            strip: s,
            colour: None,
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.doc().mixer().strip(s).unwrap().colour, None);
    let before = h.doc().clone();
    assert!(matches!(
        apply(
            &before,
            &Command::Mixer(MixerCmd::SetColour {
                strip: StripId(99),
                colour: Some(Swatch::Blue),
            })
        ),
        Err(ModelError::UnknownId(AnyId::Strip(StripId(99))))
    ));
    assert_eq!(h.doc(), &before);
}
