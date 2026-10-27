use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, MAX_NODES, NodeSpec, PlanError, StripSpec, Tap, plan,
    plan_with,
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
                outputs: 1,
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
                outputs: 1,
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

fn regions() -> NodeSpec {
    NodeSpec::Regions {
        path: core_model::ids::OutputPathId(0),
    }
}

/// Plan 5d-1 Ruling 2: a Regions node on a path with no plugins leads by
/// the whole latency, so no delay follows it; without leads the same
/// compensation is a delay after its strip.
#[test]
fn a_regions_node_leads_by_what_its_path_lacks() {
    // 0 Regions → 1 audio strip; 2 a bus with a 1000-frame insert;
    // 3 master (1, 2).
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.latency, 1000);
    assert_eq!(p.leads, vec![1000, 0, 0, 0]);
    assert_eq!(p.t_out[0], 1000);
    assert_eq!(p.input_delays[1], vec![0]);
    assert_eq!(p.input_delays[3], vec![0, 0]);
    let q = plan_with(&g, MAX, false).unwrap();
    assert_eq!(q.leads, vec![0; 4]);
    assert_eq!(q.t_out[0], 0);
    assert_eq!(q.input_delays[3], vec![1000, 0]);
    assert_eq!(q.latency, 1000);
}

/// The plugins on the node's own path still delay it: the lead is what
/// the path lacks of `L`.
#[test]
fn a_regions_nodes_own_inserts_still_delay_it() {
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![ins(300)]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![700, 0, 0, 0]);
    assert_eq!(p.t_out[1], 1000);
    assert_eq!(p.input_delays[1], vec![0]);
    assert_eq!(p.input_delays[3], vec![0, 0]);
    assert_eq!(p.latency, 1000);
}

/// A send through a latent bus is part of the node's own path: the lead
/// stops at the slowest path, and its direct path keeps the difference
/// as a delay.
#[test]
fn a_send_through_a_latent_bus_limits_the_lead() {
    // 0 Regions → 1 audio strip → master and → 2 bus (500) → master;
    // 3 another strip (1000) → master; 4 master.
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[1], vec![ins(500)]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2, 3], vec![]),
        ],
        master: 4,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![500, 0, 0, 0, 0]);
    assert_eq!(p.input_delays[1], vec![0]);
    assert_eq!(p.input_delays[2], vec![0]);
    assert_eq!(p.input_delays[4], vec![500, 0, 0]);
    assert_eq!(p.latency, 1000);
}

/// No latency, no path to master, or a lead above the cap.
#[test]
fn leads_need_latency_and_a_path_to_master_and_stay_under_the_cap() {
    let flat = GraphSpec {
        nodes: vec![regions(), strip(&[0], vec![]), strip(&[1], vec![])],
        master: 2,
    };
    assert_eq!(plan(&flat, MAX).unwrap().leads, vec![0, 0, 0]);
    // Node 0's strip feeds nothing; master is fed by a latent bus.
    let dead = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![ins(1000)]),
            strip(&[2], vec![]),
        ],
        master: 3,
    };
    assert_eq!(plan(&dead, MAX).unwrap().leads, vec![0, 0, 0, 0]);
    // Max delay 2000: the lead stops at 1000 and the rest is a delay.
    let big = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![ins(1500)]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    };
    let p = plan(&big, 2000).unwrap();
    assert_eq!(p.leads, vec![1000, 0, 0, 0]);
    assert_eq!(p.input_delays[3], vec![500, 0]);
    assert_eq!(p.latency, 1500);
}

/// A sidechain fed from a led path would make the latency grow: then
/// nothing leads.
#[test]
fn a_lead_that_would_raise_the_latency_through_a_sidechain_is_dropped() {
    let mut duck = ins(0);
    duck.sidechain = Some(input(1));
    // 0 Regions → 1 audio strip → master; 2 a strip whose ducker listens
    // to 1, then a 200-frame insert; 3 a latent bus (1000); 4 master.
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![duck, ins(200)]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2, 3], vec![]),
        ],
        master: 4,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![0; 5]);
    assert_eq!(p.latency, 1000);
    assert_eq!(p, plan_with(&g, MAX, false).unwrap());
}

/// A lead can push an edge delay past the maximum; the graph then plans
/// without leads instead of being refused.
#[test]
fn a_lead_that_overflows_the_delay_cap_falls_back_to_no_leads() {
    let mut duck = ins(0);
    duck.sidechain = Some(input(1));
    // Led, the ducked strip would be ready at 1000 + 1500 and node 3's
    // edge would need a 2500-frame delay.
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![duck, ins(1500)]),
            strip(&[], vec![]),
            strip(&[1, 2, 3], vec![]),
        ],
        master: 4,
    };
    let p = plan(&g, 2000).unwrap();
    assert_eq!(p.leads, vec![0; 5]);
    assert_eq!(p.latency, 1500);
    assert_eq!(p, plan_with(&g, 2000, false).unwrap());
}

/// Each `Regions` node leads by its own path's slack.
#[test]
fn two_regions_nodes_with_different_slack_get_their_own_leads() {
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![ins(300)]),
            regions(),
            strip(&[2], vec![]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 3, 4], vec![]),
        ],
        master: 5,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![700, 0, 1000, 0, 0, 0]);
    assert_eq!(p.input_delays[5], vec![0, 0, 0]);
    assert_eq!(p.latency, 1000);
}

/// A led `Regions` input and a latent `Instrument` input to one strip:
/// the instrument is delayed up to the lead.
#[test]
fn a_strip_mixing_a_led_regions_and_a_latent_instrument_delays_the_instrument() {
    let g = GraphSpec {
        nodes: vec![
            regions(),
            NodeSpec::Instrument {
                target: core_engine::schedule::TargetIdx(0),
                latency: 300,
                outputs: 1,
            },
            strip(&[0, 1], vec![]),
            strip(&[], vec![ins(1000)]),
            strip(&[2, 3], vec![]),
        ],
        master: 4,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![1000, 0, 0, 0, 0]);
    assert_eq!(p.input_delays[2], vec![0, 700]);
    assert_eq!(p.input_delays[4], vec![0, 0]);
    assert_eq!(p.latency, 1000);
}

/// The master's own inserts are on every path: they add to `L` but not to
/// what a node lacks.
#[test]
fn the_masters_own_inserts_do_not_lengthen_the_lead() {
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2], vec![ins(400)]),
        ],
        master: 3,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![1000, 0, 0, 0]);
    assert_eq!(p.input_delays[3], vec![0, 0]);
    assert_eq!(p.latency, 1400);
}
