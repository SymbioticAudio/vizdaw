//! Plugin delay compensation and evaluation order (spec §6, non-RT).

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
    #[error("graph too large: {nodes} node(s), {inserts} insert(s) (limits 1024 and 4096)")]
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
    /// An edge would need more than the maximum delay.
    #[error("a compensation delay of {frames} frames exceeds the 2 s limit")]
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
    /// Per node, when its output is ready (`t_in + latency`).
    pub t_out: Vec<u64>,
    /// Per node, the delay of each input edge.
    pub input_delays: Vec<Vec<u32>>,
    /// Per strip node, the delay of each insert's sidechain (0 when none).
    pub sidechain_delays: Vec<Vec<u32>>,
    /// Total latency `L` (master's `t_out`).
    pub latency: u64,
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

/// Plans `spec` (see the PDC rule in spec §6). `max_delay` bounds any one
/// edge delay (2 s of frames in the engine).
pub fn plan(spec: &GraphSpec, max_delay: u64) -> Result<Plan, PlanError> {
    let n = spec.nodes.len();
    let inserts: usize = spec
        .nodes
        .iter()
        .map(|x| match x {
            NodeSpec::Strip(s) => s.inserts.len(),
            NodeSpec::Instrument { .. } => 0,
        })
        .sum();
    if n > MAX_NODES || inserts > MAX_INSERTS {
        return Err(PlanError::TooLarge { nodes: n, inserts });
    }
    if spec.master >= n {
        return Err(PlanError::BadIndex { node: spec.master });
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
    let mut t_in = vec![0u64; n];
    let mut t_out = vec![0u64; n];
    let mut input_delays = vec![Vec::new(); n];
    let mut sidechain_delays = vec![Vec::new(); n];
    for &i in &order {
        let Some(node) = spec.nodes.get(i) else {
            continue;
        };
        match node {
            NodeSpec::Instrument { latency, .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = u64::from(*latency);
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
    Ok(Plan {
        latency: at(&t_out, spec.master),
        order,
        t_in,
        t_out,
        input_delays,
        sidechain_delays,
    })
}
