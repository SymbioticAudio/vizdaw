//! Automatable mixer controls (Plan 4d Rulings 11, 13).

use core_engine::backend::StereoBuf;
use core_engine::fixtures::Builder;
use core_engine::graph::dsp::{mix_into, mix_into_ramp, scale, scale_ramp, strip_gains};
use core_engine::graph::mixer::mixer_spec;
use core_engine::graph::{
    FeedSpec, Graph, GraphSpec, InputSpec, InsertInit, InsertSpec, InstrumentFeed, NodeSpec,
    Runner, StripControlSpec, StripSpec, Tap, TestMode, TestNode, plan,
};
use core_engine::schedule::{AutoDest, StripParam, TargetIdx};
use core_model::automation::{fader_db, fader_norm, pan_norm};
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::{AuxSend, PluginRef, PluginSlot, StripKind, db_to_gain};
use core_model::mixer_rules::audible;

fn zero(_: &PluginSlot) -> u32 {
    0
}

/// `doc`'s graph with its controls, and each strip's node.
fn built(doc: &Document) -> (Graph, Vec<(usize, StripId)>) {
    let mg = mixer_spec(doc, &[InstrumentFeed::builtin(TargetIdx(0), 0)], &mut zero);
    let p = plan(&mg.spec, 96_000).unwrap();
    let mut g = Graph::build(&mg.spec, &p, 512, |_, _| InsertInit::Placeholder);
    g.set_controls(&mg.controls);
    (g, mg.strips)
}

fn node(strips: &[(usize, StripId)], s: StripId) -> usize {
    strips.iter().find(|x| x.1 == s).unwrap().0
}

/// The built-in synth to master with a post-fader send to bus A; bus B
/// empty. Returns the builder, synth, A, B and master.
fn routed() -> (Builder, StripId, StripId, StripId, StripId) {
    let mut b = Builder::new();
    b.dest("Synth", "builtin:synth", 0).unwrap();
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let a = b.bus("A").unwrap();
    let bus_b = b.bus("B").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip: synth,
        send: AuxSend::new(a),
    })
    .unwrap();
    let master = b.doc().mixer().master();
    (b, synth, a, bus_b, master)
}

#[test]
fn automated_faders_pans_and_sends_set_the_gains() {
    let (b, synth, a, _, master) = routed();
    let (mut g, strips) = built(b.doc());
    let (sn, an, mn) = (
        node(&strips, synth),
        node(&strips, a),
        node(&strips, master),
    );
    let (l0, r0) = strip_gains(0.0, 0.0, true);
    assert_eq!(g.gains(sn), Some((l0, r0, true)));
    // A commit without overrides changes nothing.
    g.commit_controls(false);
    assert_eq!(g.gains(sn), Some((l0, r0, true)));
    g.automate_strip(synth, StripParam::Fader, fader_norm(-6.0));
    g.automate_strip(synth, StripParam::Pan, pan_norm(0.5));
    g.commit_controls(false);
    let (gl, gr) = strip_gains(fader_db(fader_norm(-6.0)), 0.5, true);
    assert_eq!(g.gains(sn), Some((gl, gr, true)));
    g.automate_send(synth, 0, fader_norm(-12.0));
    g.commit_controls(false);
    assert_eq!(
        g.edge_gains(an),
        vec![db_to_gain(fader_db(fader_norm(-12.0))) as f32]
    );
    // Master has no pan.
    g.automate_strip(master, StripParam::Pan, 0.0);
    g.commit_controls(false);
    assert_eq!(g.gains(mn), Some((1.0, 1.0, true)));
}

#[test]
fn mute_and_solo_follow_the_routing() {
    let (b, synth, a, bus_b, master) = routed();
    let (mut g, strips) = built(b.doc());
    let n = |s| node(&strips, s);
    g.automate_strip(synth, StripParam::Mute, 1.0);
    g.commit_controls(false);
    assert_eq!(g.gains(n(synth)).map(|x| x.2), Some(false));
    assert_eq!(
        g.edge_gains(n(a)),
        vec![0.0],
        "a muted strip's send carries nothing"
    );
    assert!(g.edge_gains(n(master)).contains(&0.0), "nor its output");
    g.automate_strip(synth, StripParam::Mute, 0.0);
    // Soloing B keeps B and master; the synth and A go quiet.
    g.automate_strip(bus_b, StripParam::Solo, 1.0);
    g.commit_controls(false);
    let on = |g: &Graph, s| g.gains(n(s)).unwrap().2;
    assert!(on(&g, bus_b) && on(&g, master));
    assert!(!on(&g, synth) && !on(&g, a));
    // Soloing A keeps the synth that feeds it.
    g.automate_strip(bus_b, StripParam::Solo, 0.0);
    g.automate_strip(a, StripParam::Solo, 1.0);
    g.commit_controls(false);
    assert!(on(&g, synth) && on(&g, a) && on(&g, master) && !on(&g, bus_b));
}

#[test]
fn a_new_graph_keeps_overrides_only_while_the_static_value_is_unchanged() {
    let (mut b, synth, a, _, _) = routed();
    let (g, strips) = built(b.doc());
    let (sn, an) = (node(&strips, synth), node(&strips, a));
    let mut r = Runner::serial(g);
    r.graph_mut()
        .automate_strip(synth, StripParam::Fader, fader_norm(-12.0));
    r.graph_mut().automate_send(synth, 0, fader_norm(-20.0));
    r.graph_mut().commit_controls(false);
    let held = r.graph_mut().gains(sn);
    let held_send = r.graph_mut().edge_gains(an);
    r.replace_graph(built(b.doc()).0);
    assert_eq!(
        r.graph_mut().gains(sn),
        held,
        "an unchanged fader keeps its override"
    );
    assert_eq!(r.graph_mut().edge_gains(an), held_send);
    // The user moves the fader: its override goes; the send's stays.
    b.mixer(MixerCmd::SetGain {
        strip: synth,
        gain_db: -3.0,
    })
    .unwrap();
    r.replace_graph(built(b.doc()).0);
    let (gl, gr) = strip_gains(-3.0, 0.0, true);
    assert_eq!(r.graph_mut().gains(sn), Some((gl, gr, true)));
    assert_eq!(r.graph_mut().edge_gains(an), held_send);
    // Dropping the overrides no lane drives brings the static values back.
    r.graph_mut()
        .release_overrides(|d| d == AutoDest::Strip(synth, StripParam::Pan));
    r.graph_mut().commit_controls(false);
    assert_eq!(r.graph_mut().edge_gains(an), vec![1.0]);
}

#[test]
fn ramps_move_linearly_across_a_run() {
    let mut buf = StereoBuf::new(4);
    buf.left.copy_from_slice(&[1.0; 4]);
    buf.right.copy_from_slice(&[1.0; 4]);
    scale_ramp(&mut buf, (0.0, 1.0), (1.0, 1.0), 4);
    assert_eq!(&buf.left[..], &[0.0, 0.25, 0.5, 0.75]);
    assert_eq!(&buf.right[..], &[1.0; 4]);
    let mut src = StereoBuf::new(4);
    src.left.copy_from_slice(&[1.0; 4]);
    src.right.copy_from_slice(&[2.0; 4]);
    let mut dst = StereoBuf::new(4);
    mix_into_ramp(&mut dst, &src, 1.0, 0.0, 4);
    assert_eq!(&dst.left[..], &[1.0, 0.75, 0.5, 0.25]);
    assert_eq!(&dst.right[..], &[2.0, 1.5, 1.0, 0.5]);
    // Equal ends are the constant functions.
    let (mut x, mut y) = (src.clone(), src.clone());
    scale_ramp(&mut x, (0.3, 0.7), (0.3, 0.7), 4);
    scale(&mut y, 0.3, 0.7, 4);
    assert_eq!((&x.left[..], &x.right[..]), (&y.left[..], &y.right[..]));
    let (mut x, mut y) = (StereoBuf::new(4), StereoBuf::new(4));
    mix_into_ramp(&mut x, &src, 0.5, 0.5, 4);
    mix_into(&mut y, &src, 0.5, 4);
    assert_eq!(&x.left[..], &y.left[..]);
}

const S: StripId = StripId(7);
const M: StripId = StripId(0);

/// A noise strip `S` (node 0) into master `M` (node 1), with controls;
/// `reference` leaves the controls out (the static render).
fn noise_runner(reference: bool) -> Runner {
    let mut src = StripSpec::new(Some(S));
    src.inserts.push(InsertSpec {
        instance: Some(PluginInstanceId(0)),
        latency: 0,
        bypass: false,
        sidechain: None,
    });
    let mut master = StripSpec::new(Some(M));
    master.inputs.push(InputSpec {
        from: 0,
        tap: Tap::Post,
        gain: 1.0,
    });
    let spec = GraphSpec {
        nodes: vec![NodeSpec::Strip(src), NodeSpec::Strip(master)],
        master: 1,
    };
    let p = plan(&spec, 96_000).unwrap();
    let mut g = Graph::build(&spec, &p, 512, |_, _| {
        InsertInit::Fresh(Box::new(TestNode::new(
            TestMode::Noise {
                seed: 0x1234_5678,
                level: 1.0,
            },
            0,
        )))
    });
    if !reference {
        let ctl = |strip, node, feeds| StripControlSpec {
            strip,
            node,
            gain_db: 0.0,
            pan: 0.0,
            apply_pan: false,
            mute: false,
            solo: false,
            feeds,
            solo_keeps: vec![S, M],
        };
        g.set_controls(&[
            ctl(
                S,
                0,
                vec![FeedSpec {
                    to: 1,
                    input: 0,
                    send: None,
                }],
            ),
            ctl(M, 1, Vec::new()),
        ]);
    }
    Runner::serial(g)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-5 * (1.0 + b.abs())
}

#[test]
fn a_rendered_ramp_moves_across_the_run_then_settles() {
    let mut want = noise_runner(true);
    let mut r = noise_runner(false);
    let g = db_to_gain(fader_db(fader_norm(-6.0))) as f32;
    // The run's start values (the static ones), then its end values.
    r.graph_mut()
        .automate_strip(S, StripParam::Fader, fader_norm(0.0));
    r.graph_mut().commit_controls(false);
    r.graph_mut()
        .automate_strip(S, StripParam::Fader, fader_norm(-6.0));
    r.graph_mut().commit_controls(true);
    let (rl, _) = want.render(64, &[64]);
    let (l, _) = r.render(64, &[64]);
    assert!(rl.iter().any(|x| *x != 0.0));
    for (i, (x, y)) in l.iter().zip(&rl).enumerate() {
        let k = 1.0 + (g - 1.0) * i as f32 / 64.0;
        assert!(close(*x, y * k), "frame {i}: {x} vs {}", y * k);
    }
    // Settled: the next run holds the end gain.
    r.graph_mut().settle();
    let (rl, _) = want.render(64, &[64]);
    let (l, _) = r.render(64, &[64]);
    for (i, (x, y)) in l.iter().zip(&rl).enumerate() {
        assert!(close(*x, y * g), "frame {i}: {x} vs {}", y * g);
    }
}

#[test]
fn an_automated_mute_inside_a_run_steps_hard() {
    let mut want = noise_runner(true);
    let mut r = noise_runner(false);
    // A mute point inside the run: start value unmuted, end value muted.
    r.graph_mut().automate_strip(S, StripParam::Mute, 0.0);
    r.graph_mut().commit_controls(false);
    r.graph_mut().automate_strip(S, StripParam::Mute, 1.0);
    r.graph_mut().commit_controls(true);
    let _ = want.render(64, &[64]);
    let (l, _) = r.render(64, &[64]);
    assert!(l.iter().all(|x| *x == 0.0), "no fade: {:?}", &l[..4]);
    assert_eq!(r.graph_mut().edge_gains(1), vec![0.0]);
    // And back: the unmute is a step to full level, no intermediate values.
    r.graph_mut().settle();
    r.graph_mut().automate_strip(S, StripParam::Mute, 0.0);
    r.graph_mut().commit_controls(true);
    let (rl, _) = want.render(64, &[64]);
    let (l, _) = r.render(64, &[64]);
    assert_eq!(l, rl);
}

#[test]
fn an_automated_solo_combines_with_a_static_solo() {
    let (mut b, synth, a, bus_b, master) = routed();
    b.mixer(MixerCmd::SetSolo {
        strip: bus_b,
        solo: true,
    })
    .unwrap();
    let (mut g, strips) = built(b.doc());
    let all = [synth, a, bus_b, master];
    // `g`'s state against the static graph of `doc`.
    let same = |g: &Graph, doc: &Document| {
        let (s, _) = built(doc);
        let want = audible(doc.mixer());
        for x in all {
            let n = node(&strips, x);
            assert_eq!(g.gains(n), s.gains(n), "{x:?}");
            assert_eq!(g.gains(n).unwrap().2, want.contains(&x), "{x:?}");
            assert_eq!(g.edge_gains(n), s.edge_gains(n), "{x:?}");
        }
    };
    g.automate_strip(a, StripParam::Solo, 1.0);
    g.commit_controls(false);
    let mut both = Builder::from_doc(b.doc().clone());
    both.mixer(MixerCmd::SetSolo {
        strip: a,
        solo: true,
    })
    .unwrap();
    same(&g, both.doc());
    g.automate_strip(a, StripParam::Solo, 0.0);
    g.commit_controls(false);
    same(&g, b.doc());
}

#[test]
fn an_automated_mute_leaves_the_sidechain_feed() {
    let (mut b, synth, a, bus_b, master) = routed();
    let duck = b
        .insert(bus_b, PluginRef::from_legacy("clap:duck"))
        .unwrap();
    b.mixer(MixerCmd::SetSidechain {
        slot: duck,
        source: Some(synth),
    })
    .unwrap();
    let (mut g, strips) = built(b.doc());
    let n = |s| node(&strips, s);
    assert_eq!(g.sidechain_gains(n(bus_b)), vec![1.0]);
    g.automate_strip(synth, StripParam::Mute, 1.0);
    g.commit_controls(false);
    assert_eq!(g.edge_gains(n(a)), vec![0.0]);
    assert!(g.edge_gains(n(master)).contains(&0.0));
    assert_eq!(
        g.sidechain_gains(n(bus_b)),
        vec![1.0],
        "an automated mute still feeds the sidechain"
    );
}
