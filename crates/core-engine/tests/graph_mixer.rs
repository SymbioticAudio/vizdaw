use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::mixer::mixer_spec;
use core_engine::graph::{InputSpec, InstrumentFeed, NodeSpec, StripSpec, Tap};
use core_engine::schedule::TargetIdx;
use core_model::command::MixerCmd;
use core_model::document::{DestKind, Document, DocumentRepr, Settings};
use core_model::ids::{Counters, StripId};
use core_model::mixer::{AuxSend, Mixer, PluginRef, PluginSlot, StripKind};
use std::f32::consts::FRAC_1_SQRT_2;

fn strip(spec: &core_engine::graph::GraphSpec, i: usize) -> &StripSpec {
    match &spec.nodes[i] {
        NodeSpec::Strip(s) => s,
        NodeSpec::Instrument { .. } => panic!("node {i} is an instrument"),
    }
}

fn post(from: usize, gain: f32) -> InputSpec {
    InputSpec {
        from,
        tap: Tap::Post,
        gain,
    }
}

fn zero(_: &PluginSlot) -> u32 {
    0
}

#[test]
fn the_demo_plays_through_the_builtin_strip() {
    let doc = demo_document().unwrap();
    let g = mixer_spec(&doc, &[InstrumentFeed::builtin(TargetIdx(0), 0)], &mut zero);
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    assert_eq!(
        g.spec.nodes[0],
        NodeSpec::Instrument {
            target: TargetIdx(0),
            latency: 0
        }
    );
    assert_eq!(g.spec.master, 1);
    assert_eq!(g.strips, vec![(1, StripId(0)), (2, StripId(1))]);
    let master = strip(&g.spec, 1);
    let synth = strip(&g.spec, 2);
    assert_eq!(synth.inputs, vec![post(0, 1.0)]);
    assert_eq!(master.inputs, vec![post(2, 1.0)]);
    assert_eq!(
        (master.gain_l, master.gain_r),
        (1.0, 1.0),
        "no pan on master"
    );
    assert!((synth.gain_l - FRAC_1_SQRT_2).abs() < 1e-6 && synth.gain_l == synth.gain_r);
    assert!(master.audible && synth.audible);
}

/// Synth (builtin strip 1) sends pre-fader at −6 dB to bus A (2); bus B (3)
/// has a ducker sidechained from the synth. Nodes: 0 instrument, 1 master,
/// 2 synth, 3 A, 4 B.
fn routed() -> Builder {
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let a = b.bus("A").unwrap();
    let bus_b = b.bus("B").unwrap();
    let mut send = AuxSend::new(a);
    send.level_db = -6.020_6;
    send.pre_fader = true;
    b.mixer(MixerCmd::AddSend { strip: synth, send }).unwrap();
    let duck = b
        .insert(bus_b, PluginRef::from_legacy("clap:duck"))
        .unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: duck,
        source: Some(synth),
    })
    .unwrap();
    b
}

#[test]
fn outputs_sends_and_sidechains_become_edges() {
    let b = routed();
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut zero,
    );
    assert_eq!(
        strip(&g.spec, 1).inputs,
        vec![post(2, 1.0), post(3, 1.0), post(4, 1.0)]
    );
    let a = &strip(&g.spec, 3).inputs;
    assert_eq!(a.len(), 1);
    assert_eq!((a[0].from, a[0].tap), (2, Tap::Pre));
    assert!((a[0].gain - 0.5).abs() < 1e-4);
    let duck = &strip(&g.spec, 4).inserts[0];
    assert_eq!(duck.sidechain, Some(post(2, 1.0)));
    assert_eq!(duck.instance, Some(core_model::ids::PluginInstanceId(0)));
}

#[test]
fn mute_and_solo_become_gains() {
    // Explicit mute: every edge from the synth is 0, its sidechain feed too.
    let mut b = routed();
    b.mixer(MixerCmd::SetMute {
        strip: StripId(1),
        mute: true,
    })
    .unwrap();
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut zero,
    );
    assert_eq!(strip(&g.spec, 1).inputs[0], post(2, 0.0));
    assert_eq!(strip(&g.spec, 3).inputs[0].gain, 0.0);
    assert_eq!(strip(&g.spec, 4).inserts[0].sidechain, Some(post(2, 0.0)));
    assert!(!strip(&g.spec, 2).audible);

    // Solo B: the synth is solo-muted, but still drives B's sidechain.
    let mut b = routed();
    b.mixer(MixerCmd::SetSolo {
        strip: StripId(3),
        solo: true,
    })
    .unwrap();
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut zero,
    );
    assert_eq!(
        strip(&g.spec, 1).inputs,
        vec![post(2, 0.0), post(3, 0.0), post(4, 1.0)]
    );
    assert_eq!(strip(&g.spec, 4).inserts[0].sidechain, Some(post(2, 1.0)));
    assert!(!strip(&g.spec, 3).audible && strip(&g.spec, 4).audible);

    // Solo A keeps its source (the synth, upstream through the send).
    let mut b = routed();
    b.mixer(MixerCmd::SetSolo {
        strip: StripId(2),
        solo: true,
    })
    .unwrap();
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut zero,
    );
    assert_eq!(
        strip(&g.spec, 1).inputs,
        vec![post(2, 1.0), post(3, 1.0), post(4, 0.0)]
    );
}

#[test]
fn faders_pans_and_disabled_sends() {
    let mut b = routed();
    b.mixer(MixerCmd::SetPan {
        strip: StripId(1),
        pan: -1.0,
    })
    .unwrap();
    b.mixer(MixerCmd::SetGain {
        strip: StripId(0),
        gain_db: -6.020_6,
    })
    .unwrap();
    b.mixer(MixerCmd::SetPan {
        strip: StripId(0),
        pan: 1.0,
    })
    .unwrap();
    let mut off = AuxSend::new(StripId(2));
    off.enabled = false;
    b.mixer(MixerCmd::EditSend {
        strip: StripId(1),
        index: 0,
        send: off,
    })
    .unwrap();
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut zero,
    );
    let synth = strip(&g.spec, 2);
    assert_eq!(synth.gain_l, 1.0);
    assert!(synth.gain_r.abs() < 1e-7);
    let master = strip(&g.spec, 1);
    assert!((master.gain_l - 0.5).abs() < 1e-4 && master.gain_l == master.gain_r);
    assert_eq!(strip(&g.spec, 3).inputs[0].gain, 0.0);
}

#[test]
fn insert_latencies_and_bypass_come_from_the_slots() {
    let mut b = routed();
    let eq = b
        .insert(StripId(1), PluginRef::from_legacy("clap:eq"))
        .unwrap();
    b.mixer(MixerCmd::SetBypass {
        slot: eq,
        bypass: true,
    })
    .unwrap();
    let mut lat = |s: &PluginSlot| if s.id == eq { 64 } else { 0 };
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 7)],
        &mut lat,
    );
    assert_eq!(
        g.spec.nodes[0],
        NodeSpec::Instrument {
            target: TargetIdx(0),
            latency: 7
        }
    );
    let ins = &strip(&g.spec, 2).inserts[0];
    assert_eq!(
        (ins.instance, ins.latency, ins.bypass),
        (Some(eq), 64, true)
    );
}

#[test]
fn missing_builtin_strip_and_silent_instrument_strips_are_reported() {
    let doc = Document::from_parts(DocumentRepr {
        settings: Settings::default(),
        instruments: vec![],
        outputs: vec![],
        media: vec![],
        sequences: vec![],
        mixer: Some(Mixer::new(StripId(0))),
        counters: Counters {
            strip: 1,
            ..Counters::default()
        },
    })
    .unwrap();
    let g = mixer_spec(&doc, &[InstrumentFeed::builtin(TargetIdx(0), 0)], &mut zero);
    assert_eq!(strip(&g.spec, 1).inputs, vec![post(0, 1.0)]);
    assert!(g.warnings[0].contains("no built-in synth strip"));

    let mut b = Builder::new();
    b.dest_kind("Pad", DestKind::plugin("au:pad"), None)
        .unwrap();
    let g = mixer_spec(b.doc(), &[], &mut zero);
    assert_eq!(
        g.warnings,
        vec!["strip \"Pad\" has no instrument".to_string()]
    );
}

#[test]
fn post_fader_sends_and_bus_outputs_become_post_edges() {
    // Synth (1) → bus A (2) by its output, post-fader send at −6 dB to bus
    // B (3); A → master. Nodes: 0 instrument, 1 master, 2 synth, 3 A, 4 B.
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let a = b.bus("A").unwrap();
    let bus_b = b.bus("B").unwrap();
    b.mixer(MixerCmd::SetOutput {
        strip: synth,
        output: core_model::mixer::Output::Bus(a),
    })
    .unwrap();
    let mut send = AuxSend::new(bus_b);
    send.level_db = -6.020_6;
    b.mixer(MixerCmd::AddSend { strip: synth, send }).unwrap();
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut zero,
    );
    // Master hears A and B, not the synth directly.
    assert_eq!(strip(&g.spec, 1).inputs, vec![post(3, 1.0), post(4, 1.0)]);
    assert_eq!(strip(&g.spec, 3).inputs, vec![post(2, 1.0)]);
    let to_b = &strip(&g.spec, 4).inputs;
    assert_eq!(to_b.len(), 1);
    assert_eq!((to_b[0].from, to_b[0].tap), (2, Tap::Post));
    assert!((to_b[0].gain - 0.5).abs() < 1e-4);
    // Muting the synth silences its output and its post send alike.
    b.mixer(MixerCmd::SetMute {
        strip: synth,
        mute: true,
    })
    .unwrap();
    let g = mixer_spec(
        b.doc(),
        &[InstrumentFeed::builtin(TargetIdx(0), 0)],
        &mut zero,
    );
    assert_eq!(strip(&g.spec, 3).inputs, vec![post(2, 0.0)]);
    assert_eq!(strip(&g.spec, 4).inputs[0].gain, 0.0);
}
