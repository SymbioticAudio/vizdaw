//! Plugin delay compensation and evaluation order (spec §6, non-RT).
//!
//! Every path to master is aligned to the slowest one, the total latency
//! `L`. Since plan 5d-1 (sub-project 5 spec §6.6, Ruling 33) a `Regions`
//! node is compensated by reading ahead instead of being delayed: it gets
//! a *lead*, the compensation its slowest path to master would otherwise
//! add, and plays its regions that many frames before the renderer's
//! frame (`RegionRun` reads `frame − lead`). The plan treats the lead as
//! the node's latency, so no delay line sits after it on that path, and a
//! live input mixed into the node is delayed only by the inserts on its
//! own path.

use crate::graph::spec::{GraphSpec, NodeSpec};
use std::collections::BTreeSet;

/// Most nodes a graph may have.
pub const MAX_NODES: usize = 1024;
/// Most inserts a graph may have.
pub const MAX_INSERTS: usize = 4096;

/// Why a graph cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// Beyond [`MAX_NODES`] or [`MAX_INSERTS`].
    #[error("graph too large: {nodes} node(s), {inserts} insert(s) (limits {MAX_NODES} and {MAX_INSERTS})", MAX_NODES = MAX_NODES, MAX_INSERTS = MAX_INSERTS)]
    TooLarge {
        /// Node count.
        nodes: usize,
        /// Insert count.
        inserts: usize,
    },
    /// The edges are cyclic.
    #[error("graph has a cycle")]
    Cycle,
    /// An edge or the master names a node that does not exist.
    #[error("graph refers to node {node}, which does not exist")]
    BadIndex {
        /// The missing index.
        node: usize,
    },
    /// An insert, instrument or edge needs more than the maximum delay.
    #[error("a latency or compensation delay of {frames} frames exceeds the maximum delay")]
    LatencyTooLarge {
        /// The delay needed.
        frames: u64,
    },
}

/// The result of planning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Topological order, ties broken by lower index.
    pub order: Vec<usize>,
    /// Per node, when its (aligned) inputs arrive.
    pub t_in: Vec<u64>,
    /// Per node, when its output is ready (`t_in + latency`; a `Regions`
    /// node's is its lead).
    pub t_out: Vec<u64>,
    /// Per node, the delay of each input edge.
    pub input_delays: Vec<Vec<u32>>,
    /// Per strip node, the delay of each insert's sidechain (0 when none).
    pub sidechain_delays: Vec<Vec<u32>>,
    /// Total latency `L` (master's `t_out`).
    pub latency: u64,
    /// Per node, its lead: a `Regions` node reads its regions at the
    /// renderer's frame − lead (ahead of what is heard; plan 5d-1
    /// Ruling 2). 0 for every other node.
    pub leads: Vec<u64>,
}

fn at(v: &[u64], i: usize) -> u64 {
    v.get(i).copied().unwrap_or(0)
}

fn bounded(frames: u64, max: u64) -> Result<u32, PlanError> {
    if frames > max {
        return Err(PlanError::LatencyTooLarge { frames });
    }
    u32::try_from(frames).map_err(|_| PlanError::LatencyTooLarge { frames })
}

/// Plans `spec` (see the PDC rule in spec §6), giving `Regions` nodes
/// their leads. `max_delay` bounds any one edge delay (2 s of frames in
/// the engine); a lead is at most half of it (1 s), so a read never runs
/// further behind the streamer's playhead than its rings hold.
pub fn plan(spec: &GraphSpec, max_delay: u64) -> Result<Plan, PlanError> {
    plan_with(spec, max_delay, true)
}

/// [`plan`], with `with_leads` false planning every `Regions` node with no
/// lead: its compensation is a delay line after it, as before plan 5d-1
/// (tests compare the two).
pub fn plan_with(spec: &GraphSpec, max_delay: u64, with_leads: bool) -> Result<Plan, PlanError> {
    let n = spec.nodes.len();
    let inserts: usize = spec
        .nodes
        .iter()
        .map(|x| match x {
            NodeSpec::Strip(s) => s.inserts.len(),
            NodeSpec::Instrument { .. } | NodeSpec::Regions { .. } => 0,
        })
        .sum();
    if n > MAX_NODES || inserts > MAX_INSERTS {
        return Err(PlanError::TooLarge { nodes: n, inserts });
    }
    if spec.master >= n {
        return Err(PlanError::BadIndex { node: spec.master });
    }
    for x in &spec.nodes {
        let lats: Vec<u32> = match x {
            NodeSpec::Instrument { latency, .. } => vec![*latency],
            NodeSpec::Regions { .. } => Vec::new(),
            NodeSpec::Strip(s) => s.inserts.iter().map(|k| k.latency).collect(),
        };
        for l in lats {
            bounded(u64::from(l), max_delay)?;
        }
    }
    let mut indeg = vec![0usize; n];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for d in spec.deps(i) {
            if d >= n {
                return Err(PlanError::BadIndex { node: d });
            }
            if let Some(v) = succ.get_mut(d) {
                v.push(i);
            }
            if let Some(x) = indeg.get_mut(i) {
                *x += 1;
            }
        }
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|i| indeg.get(*i) == Some(&0)).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for &s in succ.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(x) = indeg.get_mut(s) {
                *x -= 1;
                if *x == 0 {
                    ready.insert(s);
                }
            }
        }
    }
    if order.len() != n {
        return Err(PlanError::Cycle);
    }
    let none = vec![0u64; n];
    let base = forward(spec, &order, &none, max_delay)?;
    if !with_leads
        || !spec
            .nodes
            .iter()
            .any(|x| matches!(x, NodeSpec::Regions { .. }))
    {
        return Ok(base);
    }
    let leads = region_leads(spec, &order, &succ, base.latency, max_delay / 2);
    if leads.iter().all(|l| *l == 0) {
        return Ok(base);
    }
    // A lead can push an edge delay past `max_delay` (a sidechain-fed
    // insert or a dead-end branch behind a led path): then nothing leads.
    let Ok(led) = forward(spec, &order, &leads, max_delay) else {
        return Ok(base);
    };
    // A lead never raises `L` along a signal path; it can only through a
    // sidechain fed by a led path (Ruling 2). Then nothing leads.
    if led.latency != base.latency {
        return Ok(base);
    }
    Ok(led)
}

/// The forward pass: arrival and ready times, edge and sidechain delays,
/// with `lead[i]` as `Regions` node `i`'s output time.
fn forward(
    spec: &GraphSpec,
    order: &[usize],
    lead: &[u64],
    max_delay: u64,
) -> Result<Plan, PlanError> {
    let n = spec.nodes.len();
    let mut t_in = vec![0u64; n];
    let mut t_out = vec![0u64; n];
    let mut input_delays = vec![Vec::new(); n];
    let mut sidechain_delays = vec![Vec::new(); n];
    for &i in order {
        let Some(node) = spec.nodes.get(i) else {
            continue;
        };
        match node {
            NodeSpec::Instrument { latency, .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = u64::from(*latency);
                }
            }
            NodeSpec::Regions { .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = at(lead, i);
                }
            }
            NodeSpec::Strip(s) => {
                let mut tin = s
                    .inputs
                    .iter()
                    .map(|x| at(&t_out, x.from))
                    .max()
                    .unwrap_or(0);
                let mut prefix = 0u64;
                for k in &s.inserts {
                    if let Some(sc) = &k.sidechain {
                        tin = tin.max(at(&t_out, sc.from).saturating_sub(prefix));
                    }
                    prefix += u64::from(k.latency);
                }
                let mut d = Vec::with_capacity(s.inputs.len());
                for x in &s.inputs {
                    d.push(bounded(tin.saturating_sub(at(&t_out, x.from)), max_delay)?);
                }
                let mut sd = Vec::with_capacity(s.inserts.len());
                let mut before = 0u64;
                for k in &s.inserts {
                    sd.push(match &k.sidechain {
                        Some(sc) => bounded(
                            (tin + before).saturating_sub(at(&t_out, sc.from)),
                            max_delay,
                        )?,
                        None => 0,
                    });
                    before += u64::from(k.latency);
                }
                if let (Some(a), Some(b), Some(c), Some(e)) = (
                    t_in.get_mut(i),
                    t_out.get_mut(i),
                    input_delays.get_mut(i),
                    sidechain_delays.get_mut(i),
                ) {
                    *a = tin;
                    *b = tin + prefix;
                    *c = d;
                    *e = sd;
                }
            }
        }
    }
    let leads = (0..n)
        .map(|i| match spec.nodes.get(i) {
            Some(NodeSpec::Regions { .. }) => at(lead, i),
            _ => 0,
        })
        .collect();
    Ok(Plan {
        latency: at(&t_out, spec.master),
        order: order.to_vec(),
        t_in,
        t_out,
        input_delays,
        sidechain_delays,
        leads,
    })
}

/// Each `Regions` node's lead: `latency` minus the insert latency on its
/// slowest signal path to master (inputs only: a sidechain carries no
/// signal to master), at most `cap`; 0 for a node that reaches no master.
fn region_leads(
    spec: &GraphSpec,
    order: &[usize],
    succ: &[Vec<usize>],
    latency: u64,
    cap: u64,
) -> Vec<u64> {
    let n = spec.nodes.len();
    // `down[i]`: insert latency from node i's output to master's output
    // along its slowest path; `None` when no path reaches master.
    let mut down: Vec<Option<u64>> = vec![None; n];
    if let Some(m) = down.get_mut(spec.master) {
        *m = Some(0);
    }
    for &i in order.iter().rev() {
        if i == spec.master {
            continue;
        }
        let mut best: Option<u64> = None;
        for &k in succ.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            let Some(NodeSpec::Strip(s)) = spec.nodes.get(k) else {
                continue;
            };
            if !s.inputs.iter().any(|x| x.from == i) {
                continue;
            }
            let Some(below) = down.get(k).copied().flatten() else {
                continue;
            };
            let own: u64 = s.inserts.iter().map(|x| u64::from(x.latency)).sum();
            let via = own.saturating_add(below);
            best = Some(best.map_or(via, |b| b.max(via)));
        }
        if let Some(d) = down.get_mut(i) {
            *d = best;
        }
    }
    (0..n)
        .map(
            |i| match (spec.nodes.get(i), down.get(i).copied().flatten()) {
                (Some(NodeSpec::Regions { .. }), Some(d)) => latency.saturating_sub(d).min(cap),
                _ => 0,
            },
        )
        .collect()
}
