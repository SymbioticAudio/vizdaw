use core_engine::backend::{InstrumentNode, StereoBuf};
use core_engine::graph::exec::{Graph, InsertInit, Runner};
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap, TestMode, TestNode, plan,
};
use core_engine::render::{Faults, Sink};
use core_engine::schedule::TargetIdx;
use core_model::ids::PluginInstanceId;

/// A spec under construction plus the test node of every insert, in
/// global insert order. The last strip added is master.
struct Net {
    spec: GraphSpec,
    modes: Vec<(TestMode, u32)>,
}

impl Net {
    fn new() -> Net {
        Net {
            spec: GraphSpec {
                nodes: Vec::new(),
                master: 0,
            },
            modes: Vec::new(),
        }
    }

    fn strip(&mut self, inputs: &[usize], inserts: &[(TestMode, u32)]) -> usize {
        let mut s = StripSpec::new(None);
        s.inputs = inputs
            .iter()
            .map(|&from| InputSpec {
                from,
                tap: Tap::Post,
                gain: 1.0,
            })
            .collect();
        for (mode, latency) in inserts {
            s.inserts.push(InsertSpec {
                instance: Some(PluginInstanceId(self.modes.len() as u64)),
                latency: *latency,
                bypass: false,
                sidechain: None,
            });
            self.modes.push((mode.clone(), *latency));
        }
        self.spec.nodes.push(NodeSpec::Strip(s));
        self.spec.master = self.spec.nodes.len() - 1;
        self.spec.master
    }

    fn strip_mut(&mut self, i: usize) -> &mut StripSpec {
        match &mut self.spec.nodes[i] {
            NodeSpec::Strip(s) => s,
            NodeSpec::Instrument { .. } => panic!("not a strip"),
        }
    }

    fn graph(&self) -> Graph {
        let p = plan(&self.spec, 96_000).unwrap();
        Graph::build(&self.spec, &p, 1024, |idx, _| {
            let (m, l) = &self.modes[idx];
            InsertInit::Fresh(Box::new(TestNode::new(m.clone(), *l)))
        })
    }
}

fn nonzero(v: &[f32]) -> Vec<(usize, f32)> {
    v.iter()
        .enumerate()
        .filter(|(_, x)| **x != 0.0)
        .map(|(i, x)| (i, *x))
        .collect()
}

/// S (impulse) feeds X (latency 0), Y (latency 100) and Z (37 + 213);
/// master sums them.
fn three_paths() -> Net {
    let mut n = Net::new();
    let s = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let x = n.strip(&[s], &[(TestMode::Gain(1.0), 0)]);
    let y = n.strip(&[s], &[(TestMode::Gain(1.0), 100)]);
    let z = n.strip(
        &[s],
        &[(TestMode::Gain(1.0), 37), (TestMode::Gain(1.0), 213)],
    );
    n.strip(&[x, y, z], &[]);
    n
}

#[test]
fn impulse_arrives_at_master_on_the_same_sample_on_every_path() {
    let net = three_paths();
    assert_eq!(net.graph().latency(), 250);
    for sizes in [&[128u32][..], &[1, 7, 64][..]] {
        let mut r = Runner::serial(net.graph());
        let (l, rr) = r.render(1000, sizes);
        assert_eq!(nonzero(&l), vec![(250, 3.0)], "{sizes:?}");
        assert_eq!(nonzero(&rr), vec![(250, 3.0)]);
    }
}

#[test]
fn bypass_keeps_timing() {
    let mut net = three_paths();
    net.strip_mut(2).inserts[0].bypass = true;
    let mut r = Runner::serial(net.graph());
    let (l, _) = r.render(1000, &[256]);
    assert_eq!(nonzero(&l), vec![(250, 3.0)]);
}

#[test]
fn sidechain_is_aligned_to_its_insert() {
    let mut n = Net::new();
    let k = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let s = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let p = n.strip(
        &[s],
        &[(TestMode::Gain(1.0), 50), (TestMode::AddSidechain, 0)],
    );
    n.strip_mut(p).inserts[1].sidechain = Some(InputSpec {
        from: k,
        tap: Tap::Post,
        gain: 1.0,
    });
    n.strip(&[p], &[]);
    let mut r = Runner::serial(n.graph());
    let (l, _) = r.render(300, &[64]);
    assert_eq!(nonzero(&l), vec![(50, 2.0)]);
}

fn noise_into_master() -> Net {
    let mut n = Net::new();
    let s = n.strip(
        &[],
        &[(
            TestMode::Noise {
                seed: 1,
                level: 0.25,
            },
            0,
        )],
    );
    n.strip(&[s], &[]);
    n
}

#[test]
fn muted_edges_are_silent_and_unmetered() {
    let mut r = Runner::serial(noise_into_master().graph());
    let (l, _) = r.render(256, &[256]);
    assert!(!nonzero(&l).is_empty());
    assert!(r.shared().meter(0).unwrap().take().0 > 0.0);
    assert!(r.shared().meter(1).unwrap().take().0 > 0.0);

    let mut net = noise_into_master();
    net.strip_mut(1).inputs[0].gain = 0.0;
    net.strip_mut(0).audible = false;
    let mut r = Runner::serial(net.graph());
    let (l, _) = r.render(256, &[256]);
    assert!(nonzero(&l).is_empty());
    assert_eq!(r.shared().meter(0).unwrap().take(), (0.0, 0.0));

    let mut net = noise_into_master();
    net.strip_mut(1).audible = false;
    let mut r = Runner::serial(net.graph());
    let (l, _) = r.render(256, &[256]);
    assert!(nonzero(&l).is_empty(), "a muted master writes silence");
}

fn faulty(mode: TestMode) {
    let mut n = Net::new();
    let s = n.strip(
        &[],
        &[
            (
                TestMode::Noise {
                    seed: 7,
                    level: 0.25,
                },
                0,
            ),
            (mode, 0),
        ],
    );
    n.strip(&[s], &[]);
    let mut r = Runner::serial(n.graph());
    let mut l = [0.0f32; 64];
    let mut rr = [0.0f32; 64];
    for block in 1..=16 {
        assert!(r.block(64, &mut l, &mut rr));
        assert!(nonzero(&l).is_empty(), "block {block} is silenced");
        let f = r.shared().take_faults();
        assert_eq!(
            f.contains(Faults::PLUGIN_FAULT),
            block == 16,
            "block {block}"
        );
    }
    assert_eq!(r.shared().take_plugin_faults(), vec![PluginInstanceId(1)]);
    assert!(r.shared().take_plugin_faults().is_empty(), "read once");
    assert!(r.block(64, &mut l, &mut rr));
    assert!(
        !nonzero(&l).is_empty(),
        "the bypassed insert passes the noise"
    );
}

#[test]
fn faulty_insert_is_zeroed_then_bypassed() {
    faulty(TestMode::Nan);
}

#[test]
fn error_status_counts_as_fault() {
    faulty(TestMode::Error);
}

#[test]
fn placeholder_inserts_pass_audio() {
    let net = noise_into_master();
    let mut with = net.spec.clone();
    if let NodeSpec::Strip(s) = &mut with.nodes[0] {
        s.inserts.push(InsertSpec {
            instance: Some(PluginInstanceId(9)),
            latency: 0,
            bypass: false,
            sidechain: None,
        });
    }
    let p = plan(&with, 96_000).unwrap();
    let g = Graph::build(&with, &p, 1024, |idx, _| {
        if idx == 0 {
            InsertInit::Fresh(Box::new(TestNode::new(
                TestMode::Noise {
                    seed: 1,
                    level: 0.25,
                },
                0,
            )))
        } else {
            InsertInit::Placeholder
        }
    });
    let (a, _) = Runner::serial(g).render(512, &[128]);
    let (b, _) = Runner::serial(net.graph()).render(512, &[128]);
    assert_eq!(a, b);
}

/// S (noise) feeds X (latency 0) and Y (×0.5, latency 40); master's
/// input from X carries a 40-frame compensation delay.
fn compensated() -> Net {
    let mut n = Net::new();
    let s = n.strip(
        &[],
        &[(
            TestMode::Noise {
                seed: 5,
                level: 0.25,
            },
            0,
        )],
    );
    let x = n.strip(&[s], &[(TestMode::Gain(1.0), 0)]);
    let y = n.strip(&[s], &[(TestMode::Gain(0.5), 40)]);
    n.strip(&[x, y], &[]);
    n
}

#[test]
fn same_shape_adoption_keeps_delay_state() {
    let net = compensated();
    let (reference, _) = Runner::serial(net.graph()).render(200, &[50]);
    let mut r = Runner::serial(net.graph());
    let (first, _) = r.render(100, &[50]);
    // Same structure, new master gain: everything is adopted.
    let mut louder = compensated();
    louder.strip_mut(3).gain_l = 2.0;
    louder.strip_mut(3).gain_r = 2.0;
    assert_eq!(louder.spec.shape(), net.spec.shape());
    let p = plan(&louder.spec, 96_000).unwrap();
    let g = Graph::build(&louder.spec, &p, 1024, |idx, _| InsertInit::Adopt(idx));
    r.replace_graph(g);
    let (second, _) = r.render(100, &[50]);
    assert_eq!(first, reference[..100].to_vec());
    let doubled: Vec<f32> = reference[100..].iter().map(|x| x * 2.0).collect();
    assert_eq!(second, doubled);
}

#[test]
fn structural_change_keeps_instances() {
    let net = noise_into_master();
    let (reference, _) = Runner::serial(net.graph()).render(200, &[50]);
    let mut r = Runner::serial(net.graph());
    let _ = r.render(100, &[50]);
    // Add a silent strip feeding master: a new shape, the noise insert adopted.
    let mut wider = Net::new();
    let s = wider.strip(
        &[],
        &[(
            TestMode::Noise {
                seed: 1,
                level: 0.25,
            },
            0,
        )],
    );
    let q = wider.strip(&[], &[]);
    wider.strip(&[s, q], &[]);
    assert_ne!(wider.spec.shape(), net.spec.shape());
    let p = plan(&wider.spec, 96_000).unwrap();
    let g = Graph::build(&wider.spec, &p, 1024, |idx, _| InsertInit::Adopt(idx));
    r.replace_graph(g);
    let (second, _) = r.render(100, &[50]);
    assert_eq!(second, reference[100..].to_vec());
}

struct Dc(f32);

impl InstrumentNode for Dc {
    fn midi(&mut self, _: u32, _: &[u8]) {}
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        out.left[..frames as usize].fill(self.0);
        out.right[..frames as usize].fill(-self.0);
    }
    fn reset(&mut self) {}
}

#[test]
fn instrument_nodes_render_their_sink() {
    let mut s = StripSpec::new(None);
    s.inputs.push(InputSpec {
        from: 0,
        tap: Tap::Post,
        gain: 1.0,
    });
    let spec = GraphSpec {
        nodes: vec![
            NodeSpec::Instrument {
                target: TargetIdx(2),
                latency: 0,
                outputs: 1,
            },
            NodeSpec::Strip(s),
        ],
        master: 1,
    };
    let p = plan(&spec, 96_000).unwrap();
    let g = Graph::build(&spec, &p, 256, |_, _| InsertInit::Placeholder);
    assert_eq!(g.instruments(), 1 << 2);
    let mut r = Runner::serial(g);
    *r.sinks_mut().get_mut(2).unwrap() = Some(Sink::Instrument(Box::new(Dc(0.5))));
    let (l, rr) = r.render(64, &[64]);
    assert!(l.iter().all(|x| *x == 0.5) && rr.iter().all(|x| *x == -0.5));
}

#[test]
fn a_repeated_instrument_target_renders_its_sink_once() {
    let mut s = StripSpec::new(None);
    s.inputs.push(InputSpec {
        from: 0,
        tap: Tap::Post,
        gain: 1.0,
    });
    s.inputs.push(InputSpec {
        from: 1,
        tap: Tap::Post,
        gain: 1.0,
    });
    let spec = GraphSpec {
        nodes: vec![
            NodeSpec::Instrument {
                target: TargetIdx(2),
                latency: 0,
                outputs: 1,
            },
            NodeSpec::Instrument {
                target: TargetIdx(2),
                latency: 0,
                outputs: 1,
            },
            NodeSpec::Strip(s),
        ],
        master: 2,
    };
    let p = plan(&spec, 96_000).unwrap();
    let g = Graph::build(&spec, &p, 256, |_, _| InsertInit::Placeholder);
    let mut r = Runner::serial(g);
    *r.sinks_mut().get_mut(2).unwrap() = Some(Sink::Instrument(Box::new(Dc(0.5))));
    let (l, _) = r.render(64, &[64]);
    assert!(
        l.iter().all(|x| *x == 0.5),
        "only the first node of a target renders it"
    );
}

#[test]
fn bypass_line_follows_the_spec_latency() {
    // The node reports latency 0 but the spec (what plan compensates) says 50.
    let mut n = Net::new();
    let s = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let x = n.strip(&[s], &[(TestMode::Gain(1.0), 0)]);
    n.strip(&[x], &[]);
    n.strip_mut(x).inserts[0].latency = 50;
    n.strip_mut(x).inserts[0].bypass = true;
    let p = plan(&n.spec, 96_000).unwrap();
    assert_eq!(p.latency, 50);
    let g = Graph::build(&n.spec, &p, 1024, |idx, _| {
        let mode = if idx == 0 {
            TestMode::Impulse
        } else {
            TestMode::Gain(1.0)
        };
        InsertInit::Fresh(Box::new(TestNode::new(mode, 0)))
    });
    let (l, _) = Runner::serial(g).render(200, &[64]);
    assert_eq!(nonzero(&l), vec![(50, 1.0)]);
}

#[test]
fn adopted_bypass_line_is_rebuilt_for_a_new_latency() {
    let mut n = Net::new();
    let s = n.strip(&[], &[(TestMode::Impulse, 0)]);
    let x = n.strip(&[s], &[(TestMode::Gain(1.0), 0)]);
    n.strip(&[x], &[]);
    let mut r = Runner::serial(n.graph());
    let _ = r.render(64, &[64]);
    // Same instance, now reported at latency 50 and bypassed; a fresh
    // impulse source shows where the bypass line puts the audio.
    n.strip_mut(x).inserts[0].latency = 50;
    n.strip_mut(x).inserts[0].bypass = true;
    let p = plan(&n.spec, 96_000).unwrap();
    let g = Graph::build(&n.spec, &p, 1024, |idx, _| {
        if idx == 0 {
            InsertInit::Fresh(Box::new(TestNode::new(TestMode::Impulse, 0)))
        } else {
            InsertInit::Adopt(idx)
        }
    });
    r.replace_graph(g);
    let (l, _) = r.render(200, &[64]);
    assert_eq!(nonzero(&l), vec![(50, 1.0)]);
}

#[test]
fn re_entering_bypass_does_not_replay_stale_audio() {
    let mut n = Net::new();
    let s = n.strip(
        &[],
        &[
            (
                TestMode::Noise {
                    seed: 9,
                    level: 0.25,
                },
                0,
            ),
            (TestMode::Gain(1.0), 100),
        ],
    );
    n.strip(&[s], &[]);
    let mut r = Runner::serial(n.graph());
    let swap = |r: &mut Runner, n: &mut Net, bypass: bool| {
        n.strip_mut(s).inserts[1].bypass = bypass;
        let p = plan(&n.spec, 96_000).unwrap();
        r.replace_graph(Graph::build(&n.spec, &p, 1024, |idx, _| {
            InsertInit::Adopt(idx)
        }));
    };
    let _ = r.render(300, &[64]);
    swap(&mut r, &mut n, true);
    let _ = r.render(300, &[64]);
    swap(&mut r, &mut n, false);
    let _ = r.render(300, &[64]);
    swap(&mut r, &mut n, true);
    let (l, _) = r.render(200, &[64]);
    assert!(
        nonzero(&l[..100]).is_empty(),
        "the bypass line starts silent"
    );
    assert!(!nonzero(&l[100..]).is_empty());
}

#[test]
fn pre_tap_of_an_instrument_reads_its_output() {
    // Master reads the instrument pre-fader through a 10-frame compensation
    // delay (Y's insert has latency 10).
    let mut y = StripSpec::new(None);
    y.inserts.push(InsertSpec {
        instance: None,
        latency: 10,
        bypass: false,
        sidechain: None,
    });
    let mut m = StripSpec::new(None);
    m.inputs.push(InputSpec {
        from: 0,
        tap: Tap::Pre,
        gain: 1.0,
    });
    m.inputs.push(InputSpec {
        from: 1,
        tap: Tap::Post,
        gain: 1.0,
    });
    let spec = GraphSpec {
        nodes: vec![
            NodeSpec::Instrument {
                target: TargetIdx(2),
                latency: 0,
                outputs: 1,
            },
            NodeSpec::Strip(y),
            NodeSpec::Strip(m),
        ],
        master: 2,
    };
    let p = plan(&spec, 96_000).unwrap();
    let g = Graph::build(&spec, &p, 256, |_, _| {
        InsertInit::Fresh(Box::new(TestNode::new(TestMode::Gain(1.0), 10)))
    });
    let mut r = Runner::serial(g);
    *r.sinks_mut().get_mut(2).unwrap() = Some(Sink::Instrument(Box::new(Dc(0.5))));
    let (l, _) = r.render(128, &[64]);
    assert!(l[..10].iter().all(|x| *x == 0.0));
    assert!(l[10..].iter().all(|x| *x == 0.5));
}
