use core_engine::graph::exec::{Graph, InsertInit, Runner};
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap, TestMode, TestNode, plan,
};
use core_engine::pool::{RtEnv, RtPlatform, RtWaker, RtWorkgroup, StdPlatform, WorkgroupToken};
use core_engine::render::Faults;
use core_model::ids::PluginInstanceId;
use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// One random strip: inputs `(raw source, pre-fader, gain index)` and
/// inserts `(kind, latency, bypass, raw sidechain source)`; raw sources
/// are reduced modulo the strip's index so the graph is a DAG.
#[derive(Debug, Clone)]
struct RStrip {
    inputs: Vec<(u16, bool, u8)>,
    inserts: Vec<(u8, u16, bool, Option<u16>)>,
}

const GAINS: [f32; 4] = [1.0, 0.5, 0.25, 0.0];

fn build(strips: &[RStrip]) -> (GraphSpec, Vec<(TestMode, u32)>) {
    let mut spec = GraphSpec {
        nodes: Vec::new(),
        master: strips.len(),
    };
    let mut modes: Vec<(TestMode, u32)> = Vec::new();
    for (i, r) in strips.iter().enumerate() {
        let mut s = StripSpec::new(None);
        if i > 0 {
            for &(raw, pre, g) in &r.inputs {
                s.inputs.push(InputSpec {
                    from: raw as usize % i,
                    tap: if pre { Tap::Pre } else { Tap::Post },
                    gain: GAINS[g as usize % 4],
                });
            }
        }
        if s.inputs.is_empty() {
            s.inserts.push(InsertSpec {
                instance: Some(PluginInstanceId(modes.len() as u64)),
                latency: 0,
                bypass: false,
                sidechain: None,
            });
            modes.push((
                TestMode::Noise {
                    seed: i as u32 + 1,
                    level: 0.25,
                },
                0,
            ));
        }
        for &(kind, lat, bypass, sc) in &r.inserts {
            let mode = match kind % 4 {
                0 => TestMode::Gain(0.5),
                1 => TestMode::Gain(1.0),
                2 => TestMode::Noise {
                    seed: i as u32 * 31 + u32::from(lat),
                    level: 0.125,
                },
                _ => TestMode::AddSidechain,
            };
            let sidechain = if i > 0 {
                sc.map(|raw| InputSpec {
                    from: raw as usize % i,
                    tap: Tap::Post,
                    gain: 1.0,
                })
            } else {
                None
            };
            s.inserts.push(InsertSpec {
                instance: Some(PluginInstanceId(modes.len() as u64)),
                latency: u32::from(lat % 300),
                bypass,
                sidechain,
            });
            modes.push((mode, u32::from(lat % 300)));
        }
        s.gain_l = 0.75;
        s.gain_r = 0.5;
        spec.nodes.push(NodeSpec::Strip(s));
    }
    let mut master = StripSpec::new(None);
    master.inputs = (0..strips.len())
        .map(|from| InputSpec {
            from,
            tap: Tap::Post,
            gain: 1.0,
        })
        .collect();
    spec.nodes.push(NodeSpec::Strip(master));
    (spec, modes)
}

fn graph(spec: &GraphSpec, modes: &[(TestMode, u32)]) -> Graph {
    let p = plan(spec, 96_000).unwrap();
    Graph::build(spec, &p, 512, |idx, _| {
        InsertInit::Fresh(Box::new(TestNode::new(modes[idx].0.clone(), modes[idx].1)))
    })
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

const SIZES: [u32; 5] = [1, 7, 64, 333, 512];

fn assert_same(strips: &[RStrip], workers: usize) {
    let (spec, modes) = build(strips);
    let (rl, rr) = Runner::serial(graph(&spec, &modes)).render(3000, &[512]);
    let mut p = Runner::parallel(graph(&spec, &modes), workers, Arc::new(StdPlatform)).unwrap();
    let (pl, pr) = p.render(3000, &SIZES);
    assert_eq!(bits(&rl), bits(&pl), "left, {workers} worker(s)");
    assert_eq!(bits(&rr), bits(&pr), "right, {workers} worker(s)");
}

/// A fixed 14-strip graph from a small LCG.
fn fixed() -> Vec<RStrip> {
    let mut x: u32 = 12345;
    let mut next = || {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (x >> 16) as u16
    };
    (0..14)
        .map(|_| RStrip {
            inputs: (0..(next() % 3))
                .map(|_| (next(), next() % 2 == 0, (next() % 4) as u8))
                .collect(),
            inserts: (0..(next() % 3))
                .map(|_| {
                    let (kind, lat, bypass, pick, src) = (next(), next(), next(), next(), next());
                    (
                        (kind % 4) as u8,
                        lat % 300,
                        bypass % 5 == 0,
                        (pick % 2 == 0).then_some(src),
                    )
                })
                .collect(),
        })
        .collect()
}

#[test]
fn workers_match_serial_for_every_worker_count() {
    let strips = fixed();
    for w in 1..=8 {
        assert_same(&strips, w);
    }
}

fn arb_strip() -> impl Strategy<Value = RStrip> {
    (
        proptest::collection::vec((any::<u16>(), any::<bool>(), 0u8..4), 0..3),
        proptest::collection::vec(
            (
                0u8..4,
                0u16..300,
                any::<bool>(),
                proptest::option::of(any::<u16>()),
            ),
            0..3,
        ),
    )
        .prop_map(|(inputs, inserts)| RStrip { inputs, inserts })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn random_graphs_render_identically(strips in proptest::collection::vec(arb_strip(), 2..16), w in 1usize..=8) {
        assert_same(&strips, w);
    }
}

/// S (noise) → B (stall) → master, one worker, the callback not helping.
fn stalled(flag: Arc<AtomicBool>) -> Runner {
    let mut spec = GraphSpec {
        nodes: Vec::new(),
        master: 2,
    };
    let mut s = StripSpec::new(None);
    s.inserts.push(InsertSpec {
        instance: None,
        latency: 0,
        bypass: false,
        sidechain: None,
    });
    spec.nodes.push(NodeSpec::Strip(s));
    let mut b = StripSpec::new(None);
    b.inputs.push(InputSpec {
        from: 0,
        tap: Tap::Post,
        gain: 1.0,
    });
    b.inserts.push(InsertSpec {
        instance: None,
        latency: 0,
        bypass: false,
        sidechain: None,
    });
    spec.nodes.push(NodeSpec::Strip(b));
    let mut m = StripSpec::new(None);
    m.inputs.push(InputSpec {
        from: 1,
        tap: Tap::Post,
        gain: 1.0,
    });
    spec.nodes.push(NodeSpec::Strip(m));
    let p = plan(&spec, 96_000).unwrap();
    let g = Graph::build(&spec, &p, 512, |idx, _| {
        if idx == 0 {
            InsertInit::Fresh(Box::new(TestNode::new(
                TestMode::Noise {
                    seed: 3,
                    level: 0.25,
                },
                0,
            )))
        } else {
            InsertInit::Fresh(Box::new(TestNode::new(
                TestMode::Stall(Arc::clone(&flag)),
                0,
            )))
        }
    });
    let mut r = Runner::parallel(g, 1, Arc::new(StdPlatform)).unwrap();
    r.set_callback_helps(false);
    r.set_deadline(Some(Duration::from_millis(5)));
    r
}

#[test]
fn missed_deadline_is_silent_and_recovers() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut r = stalled(Arc::clone(&flag));
    let mut l = [1.0f32; 64];
    let mut rr = [1.0f32; 64];
    let t = Instant::now();
    assert!(
        !r.block(64, &mut l, &mut rr),
        "the stalled node misses the deadline"
    );
    assert!(t.elapsed() < Duration::from_millis(500));
    assert!(l.iter().all(|x| *x == 0.0));
    assert!(
        !r.block(64, &mut l, &mut rr),
        "the next block finds the straggler busy"
    );
    flag.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(50));
    // No deadline for the check, so a slow machine cannot make it flaky.
    r.set_deadline(None);
    assert!(r.block(64, &mut l, &mut rr), "recovered");
    assert!(l.iter().any(|x| *x != 0.0));
}

#[test]
fn dropping_a_runner_waits_for_stragglers() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut r = stalled(Arc::clone(&flag));
    let (mut l, mut rr) = ([0.0f32; 64], [0.0f32; 64]);
    assert!(!r.block(64, &mut l, &mut rr));
    let f2 = Arc::clone(&flag);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        f2.store(true, Ordering::Release);
    });
    let t = Instant::now();
    drop(r);
    assert!(
        t.elapsed() >= Duration::from_millis(40),
        "drop waited for the stalled node"
    );
    releaser.join().unwrap();
}

#[derive(Default)]
struct FakePlatform {
    promotes: AtomicUsize,
    fail: bool,
}

impl RtPlatform for FakePlatform {
    fn waker(&self) -> Box<dyn RtWaker> {
        StdPlatform.waker()
    }
    fn promote(&self, _: u64) -> Result<(), String> {
        self.promotes.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err("refused".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct FakeWorkgroup {
    joins: AtomicUsize,
    leaves: AtomicUsize,
    fail: bool,
}

impl RtWorkgroup for FakeWorkgroup {
    fn join(&self) -> Result<WorkgroupToken, String> {
        if self.fail {
            return Err("cancelled".into());
        }
        self.joins.fetch_add(1, Ordering::SeqCst);
        Ok(WorkgroupToken(Box::new(7u32)))
    }
    fn leave(&self, token: WorkgroupToken) {
        assert_eq!(token.0.downcast::<u32>().map(|b| *b).ok(), Some(7));
        self.leaves.fetch_add(1, Ordering::SeqCst);
    }
}

fn eventually(f: impl Fn() -> bool) -> bool {
    for _ in 0..400 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn small() -> Graph {
    let (spec, modes) = build(&fixed()[..3]);
    graph(&spec, &modes)
}

#[test]
fn env_changes_rejoin_the_workgroup() {
    let platform = Arc::new(FakePlatform::default());
    let r = Runner::parallel(small(), 3, platform.clone()).unwrap();
    let a = Arc::new(FakeWorkgroup::default());
    r.set_env(RtEnv {
        period_ns: 10_666_667,
        workgroup: Some(a.clone()),
    });
    assert!(eventually(|| a.joins.load(Ordering::SeqCst) == 3));
    assert!(eventually(|| platform.promotes.load(Ordering::SeqCst) == 3));
    let b = Arc::new(FakeWorkgroup::default());
    r.set_env(RtEnv {
        period_ns: 5_333_333,
        workgroup: Some(b.clone()),
    });
    assert!(eventually(
        || a.leaves.load(Ordering::SeqCst) == 3 && b.joins.load(Ordering::SeqCst) == 3
    ));
    drop(r);
    assert_eq!(b.leaves.load(Ordering::SeqCst), 3, "workers leave on exit");
    assert_eq!(
        platform.promotes.load(Ordering::SeqCst),
        6,
        "promoted again for the new period"
    );
}

#[test]
fn failed_join_and_promotion_are_reported() {
    let platform = Arc::new(FakePlatform {
        promotes: AtomicUsize::new(0),
        fail: true,
    });
    let r = Runner::parallel(small(), 2, platform).unwrap();
    let wg = Arc::new(FakeWorkgroup {
        fail: true,
        ..FakeWorkgroup::default()
    });
    r.set_env(RtEnv {
        period_ns: 10_000_000,
        workgroup: Some(wg),
    });
    assert!(eventually(|| {
        let f = r.shared().take_faults();
        f.contains(Faults::WORKGROUP_JOIN_FAILED) || f.contains(Faults::THREAD_POLICY_FAILED)
    }));
}
