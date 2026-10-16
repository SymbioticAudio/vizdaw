use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, MAX_NODES, NodeSpec, PlanError, StripSpec, Tap, plan,
};
use core_engine::schedule::TargetIdx;
use core_model::ids::PluginInstanceId;

fn input(from: usize) -> InputSpec {
    InputSpec {
        from,
        tap: Tap::Post,
        gain: 1.0,
    }
}

fn ins(latency: u32) -> InsertSpec {
    InsertSpec {
        instance: None,
        latency,
        bypass: false,
        sidechain: None,
    }
}

fn strip(inputs: &[usize], inserts: Vec<InsertSpec>) -> NodeSpec {
    let mut s = StripSpec::new(None);
    s.inputs = inputs.iter().map(|&f| input(f)).collect();
    s.inserts = inserts;
    NodeSpec::Strip(s)
}

const MAX: u64 = 96_000;

#[test]
fn a_chain_adds_insert_latencies() {
    let g = GraphSpec {
        nodes: vec![
            NodeSpec::Instrument {
                target: TargetIdx(0),
                latency: 0,
            },
            strip(&[0], vec![ins(10), ins(20)]),
            strip(&[1], vec![]),
        ],
        master: 2,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.order, vec![0, 1, 2]);
    assert_eq!(p.t_out, vec![0, 30, 30]);
    assert_eq!(p.latency, 30);
    assert_eq!(p.input_delays, vec![vec![], vec![0], vec![0]]);
    assert_eq!(p.sidechain_delays, vec![vec![], vec![0, 0], vec![]]);
}

fn parallel(y_latency: u32, y_bypass: bool) -> GraphSpec {
    let mut y = ins(y_latency);
    y.bypass = y_bypass;
    GraphSpec {
        nodes: vec![
            strip(&[], vec![ins(0)]),
            strip(&[0], vec![ins(0)]),
            strip(&[0], vec![y]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    }
}

#[test]
fn parallel_paths_are_aligned_to_the_latest() {
    let p = plan(&parallel(100, false), MAX).unwrap();
    assert_eq!(p.t_in, vec![0, 0, 0, 100]);
    assert_eq!(p.input_delays[3], vec![100, 0]);
    assert_eq!(p.latency, 100);
}

#[test]
fn bypass_keeps_latency() {
    assert_eq!(
        plan(&parallel(100, true), MAX).unwrap(),
        plan(&parallel(100, false), MAX).unwrap()
    );
}

/// 0 K (kick, insert latency `k`), 1 S (source), 2 P (input S; inserts:
/// latency 50, then a ducker fed by K), 3 master (inputs P, K).
fn sidechained(k: u32) -> GraphSpec {
    let mut duck = ins(0);
    duck.sidechain = Some(input(0));
    GraphSpec {
        nodes: vec![
            strip(&[], vec![ins(k)]),
            strip(&[], vec![]),
            strip(&[1], vec![ins(50), duck]),
            strip(&[2, 0], vec![]),
        ],
        master: 3,
    }
}

#[test]
fn sidechains_align_to_the_insert_position() {
    let p = plan(&sidechained(0), MAX).unwrap();
    assert_eq!(p.t_in[2], 0);
    assert_eq!(p.sidechain_delays[2], vec![0, 50]);
    assert_eq!(p.input_delays[3], vec![0, 50]);
    assert_eq!(p.latency, 50);
    // A late sidechain source delays the strip's main input instead.
    let p = plan(&sidechained(80), MAX).unwrap();
    assert_eq!(p.t_in[2], 30);
    assert_eq!(p.input_delays[2], vec![30]);
    assert_eq!(p.sidechain_delays[2], vec![0, 0]);
    assert_eq!(p.input_delays[3], vec![0, 0]);
    assert_eq!(p.latency, 80);
}

#[test]
fn cycles_bad_indices_and_limits_are_refused() {
    let cyc = GraphSpec {
        nodes: vec![strip(&[1], vec![]), strip(&[0], vec![])],
        master: 1,
    };
    assert_eq!(plan(&cyc, MAX), Err(PlanError::Cycle));
    let selfie = GraphSpec {
        nodes: vec![strip(&[0], vec![])],
        master: 0,
    };
    assert_eq!(plan(&selfie, MAX), Err(PlanError::Cycle));
    let bad = GraphSpec {
        nodes: vec![strip(&[9], vec![])],
        master: 0,
    };
    assert_eq!(plan(&bad, MAX), Err(PlanError::BadIndex { node: 9 }));
    let no_master = GraphSpec {
        nodes: vec![strip(&[], vec![])],
        master: 3,
    };
    assert_eq!(plan(&no_master, MAX), Err(PlanError::BadIndex { node: 3 }));
    let big = GraphSpec {
        nodes: (0..=MAX_NODES).map(|_| strip(&[], vec![])).collect(),
        master: 0,
    };
    assert_eq!(
        plan(&big, MAX),
        Err(PlanError::TooLarge {
            nodes: 1025,
            inserts: 0
        })
    );
    assert_eq!(
        plan(&parallel(96_001, false), MAX),
        Err(PlanError::LatencyTooLarge { frames: 96_001 })
    );
    assert!(plan(&parallel(96_000, false), MAX).is_ok());
}

#[test]
fn shape_ignores_parameters_but_not_structure() {
    let a = parallel(100, false);
    let mut b = parallel(100, true);
    if let NodeSpec::Strip(s) = &mut b.nodes[1] {
        s.gain_l = 0.25;
        s.audible = false;
        s.inputs[0].gain = 0.0;
    }
    assert_eq!(a.shape(), b.shape());
    assert_ne!(a.shape(), parallel(99, false).shape());
    let mut c = parallel(100, false);
    if let NodeSpec::Strip(s) = &mut c.nodes[3] {
        s.inputs[0].tap = Tap::Pre;
    }
    assert_ne!(a.shape(), c.shape());
    let mut d = parallel(100, false);
    if let NodeSpec::Strip(s) = &mut d.nodes[1] {
        s.inserts[0].instance = Some(PluginInstanceId(4));
    }
    assert_ne!(a.shape(), d.shape());
    assert_eq!(a.deps(3), vec![1, 2]);
    assert_eq!(sidechained(0).deps(2), vec![1, 0]);
}

#[test]
fn a_single_huge_latency_is_refused_even_off_any_edge() {
    let one = |l: u32, bypass: bool| {
        let mut i = ins(l);
        i.bypass = bypass;
        GraphSpec {
            nodes: vec![strip(&[], vec![i])],
            master: 0,
        }
    };
    assert_eq!(
        plan(&one(u32::MAX, false), MAX),
        Err(PlanError::LatencyTooLarge {
            frames: u64::from(u32::MAX)
        })
    );
    assert_eq!(
        plan(&one(96_001, true), MAX),
        Err(PlanError::LatencyTooLarge { frames: 96_001 })
    );
    assert!(plan(&one(96_000, true), MAX).is_ok());
    let inst = |l: u32| GraphSpec {
        nodes: vec![
            NodeSpec::Instrument {
                target: TargetIdx(0),
                latency: l,
            },
            strip(&[0], vec![]),
        ],
        master: 1,
    };
    assert_eq!(
        plan(&inst(96_001), MAX),
        Err(PlanError::LatencyTooLarge { frames: 96_001 })
    );
    assert!(plan(&inst(96_000), MAX).is_ok());
}
