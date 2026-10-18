//! The runtime graph and its executor (spec §7). // RT
//!
//! # Protocol
//!
//! A run (one block) resets every node's pending-input counter to its
//! number of input edges, sets `remaining` to the node count and pushes the
//! source nodes onto the ready queue. Every participating thread (the
//! callback thread and the pool's workers) pops a node, renders it,
//! decrements each successor's counter (`AcqRel`), pushes the successors
//! that reach zero, then decrements `remaining`. Hence:
//! - a node is popped exactly once per run, so while a thread renders node
//!   `i` no other thread touches `i`'s state or its inserts;
//! - every input of `i` completed before `i` was pushed and is not written
//!   again in this run, so reading it through a shared reference is sound;
//!   its writes happen-before the read through the counter's release
//!   sequence and the queue's own synchronisation;
//! - an instrument node is the only node of its target, so the only user of
//!   that target's sink during the run;
//! - between runs (`remaining == 0` and no worker inside the job, which
//!   `Exec::try_finish` establishes) the owner has exclusive access.

use crate::backend::{BlockInfo, StereoBuf};
use crate::error::EngineError;
use crate::graph::controls::{Controls, StripControlSpec};
use crate::graph::delay::DelayLine;
use crate::graph::dsp;
use crate::graph::node::{EffectNode, ProcessStatus};
use crate::graph::pdc::Plan;
use crate::graph::spec::{GraphSpec, InsertSpec, NodeSpec, Tap};
use crate::pool::{Pool, PoolShared, RtEnv, RtPlatform};
use crate::render::{Shared, Sink};
use crate::schedule::MAX_TARGETS;
use crate::schedule::automation::{AutoDest, StripParam};
use core_model::ids::{PluginInstanceId, StripId};
use crossbeam_queue::ArrayQueue;
use std::cell::UnsafeCell;
use std::collections::BTreeMap;
use std::ops::Range;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Consecutive faulty blocks before an insert is bypassed (spec §7).
pub const FAULT_LIMIT: u32 = 16;

/// The instrument sinks by target, shared with instrument nodes.
#[derive(Debug)]
pub struct SinkTable {
    cells: Box<[UnsafeCell<Option<Sink>>]>,
}

// SAFETY: a sink is used by its owner between runs (through
// `&mut SinkTable`) and during a run only by the one instrument node of its
// target (module protocol).
unsafe impl Sync for SinkTable {}

impl Default for SinkTable {
    fn default() -> SinkTable {
        SinkTable {
            cells: (0..MAX_TARGETS).map(|_| UnsafeCell::new(None)).collect(),
        }
    }
}

impl SinkTable {
    /// A target's sink slot (owner side).
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Option<Sink>> {
        self.cells.get_mut(i).map(UnsafeCell::get_mut)
    }

    /// Every sink slot (owner side).
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Option<Sink>> {
        self.cells.iter_mut().map(UnsafeCell::get_mut)
    }
}

/// An insert's runtime state; it moves to the next graph with its
/// instance (Plan Rulings 15 and 23).
pub struct InsertState {
    node: Option<Box<dyn EffectNode>>,
    instance: Option<PluginInstanceId>,
    bad_run: u32,
    faulted: bool,
    /// Delays by the spec latency while bypassed or faulted.
    bypass: DelayLine,
    /// The last block went through `bypass` (so entering bypass again
    /// starts it from silence, not from audio of the previous bypass).
    in_bypass: bool,
}

impl InsertState {
    /// `latency` is the spec's (bounded by `plan`); a placeholder has no
    /// latency to keep, so its line is empty. Not RT.
    fn new(
        node: Option<Box<dyn EffectNode>>,
        instance: Option<PluginInstanceId>,
        latency: u32,
    ) -> InsertState {
        let latency = if node.is_some() { latency } else { 0 };
        InsertState {
            node,
            instance,
            bad_run: 0,
            faulted: false,
            bypass: DelayLine::new(latency),
            in_bypass: false,
        }
    }
}

/// Where a new graph's insert comes from.
pub enum InsertInit {
    /// The running graph's insert at this index (moved on swap).
    Adopt(usize),
    /// A newly created processor.
    Fresh(Box<dyn EffectNode>),
    /// No processor: audio passes through with no latency.
    Placeholder,
}

struct Input {
    from: usize,
    tap: Tap,
    /// Gain at the end of the run (the static gain without automation).
    gain: f32,
    /// Gain at the start of the run; differs from `gain` only while a
    /// control ramps (Plan 4d Ruling 11).
    start: f32,
}

struct InsertDef {
    bypass: bool,
    sidechain: Option<Input>,
}

enum NodeDef {
    Instrument {
        target: usize,
    },
    Strip {
        inputs: Range<usize>,
        inserts: Range<usize>,
        gain_l: f32,
        gain_r: f32,
        /// Gains at the start of the run (see `Input::start`).
        start_l: f32,
        start_r: f32,
        audible: bool,
        meter: usize,
    },
}

struct NodeState {
    pre: StereoBuf,
    post: StereoBuf,
    /// An instrument node's outputs (empty for strips).
    outs: Box<[StereoBuf]>,
    scratch: StereoBuf,
    sc: StereoBuf,
    delays: Box<[DelayLine]>,
    sc_delays: Box<[DelayLine]>,
}

impl NodeState {
    fn instrument(max: u32, outputs: u16) -> NodeState {
        NodeState {
            pre: StereoBuf::new(0),
            post: StereoBuf::new(0),
            outs: (0..outputs.max(1)).map(|_| StereoBuf::new(max)).collect(),
            scratch: StereoBuf::new(0),
            sc: StereoBuf::new(0),
            delays: Box::new([]),
            sc_delays: Box::new([]),
        }
    }

    fn strip(
        max: u32,
        has_sc: bool,
        delays: Vec<DelayLine>,
        sc_delays: Vec<DelayLine>,
    ) -> NodeState {
        let delayed = delays.iter().any(|d| d.delay() > 0);
        NodeState {
            pre: StereoBuf::new(max),
            post: StereoBuf::new(max),
            outs: Box::new([]),
            scratch: StereoBuf::new(if delayed { max } else { 0 }),
            sc: StereoBuf::new(if has_sc { max } else { 0 }),
            delays: delays.into_boxed_slice(),
            sc_delays: sc_delays.into_boxed_slice(),
        }
    }

    fn resize(&mut self, max: u32) {
        for b in [
            &mut self.pre,
            &mut self.post,
            &mut self.scratch,
            &mut self.sc,
        ] {
            if b.capacity() > 0 {
                *b = StereoBuf::new(max);
            }
        }
        for b in self.outs.iter_mut() {
            *b = StereoBuf::new(max);
        }
    }
}

/// An immutable render graph with its runtime state.
pub struct Graph {
    defs: Box<[NodeDef]>,
    inputs: Box<[Input]>,
    /// `(node, spec input position)` of every input, for the controls.
    input_keys: Box<[(usize, usize)]>,
    /// `(instance, insert index)`, sorted by instance.
    insert_ids: Box<[(PluginInstanceId, usize)]>,
    controls: Controls,
    /// A commit ramps the gains of the next run; `settle` ends it.
    ramping: bool,
    insert_defs: Box<[InsertDef]>,
    succ: Box<[Box<[usize]>]>,
    indegree: Box<[u32]>,
    sources: Box<[usize]>,
    master: usize,
    latency: u64,
    shape: u64,
    max_frames: u32,
    instruments: u64,
    /// `(new index, old index, bypass line for the new spec latency)`.
    adopt: Box<[(usize, usize, DelayLine)]>,
    nodes: Box<[UnsafeCell<NodeState>]>,
    inserts: Box<[UnsafeCell<Option<InsertState>>]>,
    pending: Box<[AtomicU32]>,
    remaining: AtomicU32,
    queue: ArrayQueue<usize>,
}

// SAFETY: the only non-`Sync` fields are the `UnsafeCell`s of `nodes` and
// `inserts`. Through `&Graph` they are reached only by `render_node` (node
// `i` and its inserts mutably, by the one thread that popped `i`; finished
// predecessors of `i` immutably) and `write_master` (after a completed
// run), per the module protocol; their contents are `Send`
// (`Box<dyn EffectNode>` is `Send`), so handing exclusive use between
// threads is sound. Everything else is immutable after `build` or atomic.
unsafe impl Sync for Graph {}

impl std::fmt::Debug for Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graph")
            .field("nodes", &self.defs.len())
            .field("inserts", &self.insert_defs.len())
            .field("latency", &self.latency)
            .finish_non_exhaustive()
    }
}

/// Kahn's algorithm over the built edges: true when every node can be
/// reached in topological order, i.e. every run completes. Not RT.
fn acyclic(succ: &[Vec<usize>], indegree: &[u32], sources: &[usize]) -> bool {
    let mut left = indegree.to_vec();
    let mut ready = sources.to_vec();
    let mut seen = 0usize;
    while let Some(v) = ready.pop() {
        seen += 1;
        for &s in succ.get(v).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(d) = left.get_mut(s) {
                *d = d.saturating_sub(1);
                if *d == 0 {
                    ready.push(s);
                }
            }
        }
    }
    seen == indegree.len()
}

/// The buffer an edge reads from node `from`: of an instrument, its output
/// `k` (`Pre` and `Post` read output 0; a missing output reads an empty
/// buffer, i.e. silence). // RT
fn tap_buf<'a>(g: &Graph, from: usize, src: &'a NodeState, tap: Tap) -> &'a StereoBuf {
    match (tap, g.defs.get(from)) {
        (Tap::Pre, Some(NodeDef::Strip { .. })) => &src.pre,
        (_, Some(NodeDef::Instrument { .. })) => {
            let k = match tap {
                Tap::Out(k) => usize::from(k),
                Tap::Pre | Tap::Post => 0,
            };
            src.outs.get(k).unwrap_or(&src.post)
        }
        _ => &src.post,
    }
}

fn delay_at(v: &[Vec<u32>], i: usize, k: usize) -> u32 {
    v.get(i).and_then(|d| d.get(k)).copied().unwrap_or(0)
}

impl Graph {
    /// Builds the runtime graph for `spec` as planned by `plan`, with
    /// buffers for blocks of up to `max_frames`. `init(index, insert)` says
    /// where each insert's processor comes from (indices count inserts in
    /// node order). Self-edges are dropped (Plan Ruling 29). A cyclic spec
    /// (which `plan` refuses, but a mismatched plan lets through) gives a
    /// graph with no nodes: every run completes at once and master is
    /// silent. Not RT.
    pub fn build(
        spec: &GraphSpec,
        plan: &Plan,
        max_frames: u32,
        mut init: impl FnMut(usize, &InsertSpec) -> InsertInit,
    ) -> Graph {
        let n = spec.nodes.len();
        let mut defs = Vec::with_capacity(n);
        let mut inputs = Vec::new();
        let mut input_keys: Vec<(usize, usize)> = Vec::new();
        let mut insert_ids: Vec<(PluginInstanceId, usize)> = Vec::new();
        let mut insert_defs = Vec::new();
        let mut nodes = Vec::with_capacity(n);
        let mut inserts = Vec::new();
        let mut adopt = Vec::new();
        let mut instruments = 0u64;
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indegree = vec![0u32; n];
        let mut edge = |from: usize, to: usize, succ: &mut Vec<Vec<usize>>| {
            if let Some(v) = succ.get_mut(from) {
                v.push(to);
            }
            if let Some(d) = indegree.get_mut(to) {
                *d += 1;
            }
        };
        for (i, node) in spec.nodes.iter().enumerate() {
            match node {
                NodeSpec::Instrument {
                    target, outputs, ..
                } => {
                    // Only the first node of a target renders its sink (the
                    // module protocol needs one user per sink); a repeat, or
                    // a target beyond the table, renders silence.
                    let bit = 1u64.checked_shl(u32::from(target.0)).unwrap_or(0);
                    let target = if bit != 0 && instruments & bit == 0 {
                        instruments |= bit;
                        usize::from(target.0)
                    } else {
                        usize::MAX
                    };
                    defs.push(NodeDef::Instrument { target });
                    nodes.push(UnsafeCell::new(NodeState::instrument(max_frames, *outputs)));
                }
                NodeSpec::Strip(s) => {
                    let in_start = inputs.len();
                    let mut delays = Vec::new();
                    for (k, x) in s.inputs.iter().enumerate() {
                        if x.from == i || x.from >= n {
                            continue;
                        }
                        inputs.push(Input {
                            from: x.from,
                            tap: x.tap,
                            gain: x.gain,
                            start: x.gain,
                        });
                        input_keys.push((i, k));
                        delays.push(DelayLine::new(delay_at(&plan.input_delays, i, k)));
                        edge(x.from, i, &mut succ);
                    }
                    let ins_start = insert_defs.len();
                    let mut sc_delays = Vec::new();
                    let mut has_sc = false;
                    for (k, ins) in s.inserts.iter().enumerate() {
                        let sidechain =
                            ins.sidechain
                                .filter(|x| x.from != i && x.from < n)
                                .map(|x| Input {
                                    from: x.from,
                                    tap: x.tap,
                                    gain: x.gain,
                                    start: x.gain,
                                });
                        if let Some(x) = &sidechain {
                            edge(x.from, i, &mut succ);
                            has_sc = true;
                        }
                        sc_delays.push(DelayLine::new(delay_at(&plan.sidechain_delays, i, k)));
                        insert_defs.push(InsertDef {
                            bypass: ins.bypass,
                            sidechain,
                        });
                        let idx = inserts.len();
                        if let Some(id) = ins.instance {
                            insert_ids.push((id, idx));
                        }
                        let state = match init(idx, ins) {
                            InsertInit::Adopt(old) => {
                                // A line for the spec latency, swapped in on
                                // adoption if the adopted one differs.
                                adopt.push((idx, old, DelayLine::new(ins.latency)));
                                None
                            }
                            InsertInit::Fresh(node) => {
                                Some(InsertState::new(Some(node), ins.instance, ins.latency))
                            }
                            InsertInit::Placeholder => {
                                Some(InsertState::new(None, ins.instance, ins.latency))
                            }
                        };
                        inserts.push(UnsafeCell::new(state));
                    }
                    defs.push(NodeDef::Strip {
                        inputs: in_start..inputs.len(),
                        inserts: ins_start..insert_defs.len(),
                        gain_l: s.gain_l,
                        gain_r: s.gain_r,
                        start_l: s.gain_l,
                        start_r: s.gain_r,
                        audible: s.audible,
                        meter: s.meter.unwrap_or(i),
                    });
                    nodes.push(UnsafeCell::new(NodeState::strip(
                        max_frames, has_sc, delays, sc_delays,
                    )));
                }
            }
        }
        let mut sources: Vec<usize> = (0..n).filter(|i| indegree.get(*i) == Some(&0)).collect();
        if !acyclic(&succ, &indegree, &sources) {
            // A run could never complete (and `wait_idle` would block
            // forever): keep the inserts so they can be adopted, but render
            // no node, so every run is `Done` at once with master silent.
            // (No `debug_assert!`: this path is tested, so it must not panic.)
            defs.clear();
            inputs.clear();
            input_keys.clear();
            succ.clear();
            indegree.clear();
            nodes.clear();
            sources.clear();
            instruments = 0;
        }
        insert_ids.sort_by_key(|x| x.0);
        Graph {
            defs: defs.into_boxed_slice(),
            inputs: inputs.into_boxed_slice(),
            input_keys: input_keys.into_boxed_slice(),
            insert_ids: insert_ids.into_boxed_slice(),
            controls: Controls::default(),
            ramping: false,
            insert_defs: insert_defs.into_boxed_slice(),
            succ: succ.into_iter().map(Vec::into_boxed_slice).collect(),
            pending: indegree.iter().map(|_| AtomicU32::new(0)).collect(),
            indegree: indegree.into_boxed_slice(),
            sources: sources.into_boxed_slice(),
            master: spec.master,
            latency: plan.latency,
            shape: spec.shape(),
            max_frames,
            instruments,
            adopt: adopt.into_boxed_slice(),
            nodes: nodes.into_boxed_slice(),
            inserts: inserts.into_boxed_slice(),
            remaining: AtomicU32::new(0),
            queue: ArrayQueue::new(n.max(1)),
        }
    }

    /// Total latency `L` in frames.
    pub fn latency(&self) -> u64 {
        self.latency
    }

    /// The structure hash of the spec it was built from.
    pub fn shape(&self) -> u64 {
        self.shape
    }

    /// Largest block the buffers hold.
    pub fn max_frames(&self) -> u32 {
        self.max_frames
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.defs.len()
    }

    /// Bitmask of the targets that have an instrument node.
    pub fn instruments(&self) -> u64 {
        self.instruments
    }

    /// Installs the mixer's controls (after `build`). Not RT.
    pub fn set_controls(&mut self, specs: &[StripControlSpec]) {
        let keys: BTreeMap<(usize, usize), usize> = self
            .input_keys
            .iter()
            .enumerate()
            .map(|(i, k)| (*k, i))
            .collect();
        self.controls = Controls::build(specs, |node, k| keys.get(&(node, k)).copied());
    }

    /// Automation's normalised value for a strip control; it takes effect
    /// at the next `commit_controls`. // RT
    ///
    /// Only while no run is in flight: after `Exec::try_finish` returned
    /// true (or `Runner::graph_mut`), never while a straggling worker of a
    /// missed run may still hold `&Graph` (module protocol).
    pub fn automate_strip(&mut self, strip: StripId, p: StripParam, v: f64) {
        self.controls.set_strip(strip, p, v);
    }

    /// Automation's normalised level for send `index` of `strip`. // RT
    ///
    /// Only while no run is in flight: after `Exec::try_finish` returned
    /// true (or `Runner::graph_mut`), never while a straggling worker of a
    /// missed run may still hold `&Graph` (module protocol).
    pub fn automate_send(&mut self, strip: StripId, index: u16, v: f64) {
        self.controls.set_send(strip, index, v);
    }

    /// Drops the overrides of the controls `driven` says no lane drives;
    /// the next commit writes their static values. // RT
    ///
    /// Only while no run is in flight: after `Exec::try_finish` returned
    /// true (or `Runner::graph_mut`), never while a straggling worker of a
    /// missed run may still hold `&Graph` (module protocol).
    pub fn release_overrides(&mut self, driven: impl Fn(AutoDest) -> bool) {
        self.controls.release(driven);
    }

    /// Writes the effective control values into the strip and edge gains.
    /// With `ramp` the next run moves from the current gains to them (the
    /// renderer commits a run's start values without, then its end values
    /// with, Plan 4d Ruling 11); without, they apply at once.
    ///
    /// Mute and solo never fade: when a strip's audibility changes in a
    /// ramped commit, its output and send edges step to the new gain at
    /// the run's start, where its `audible` (meters, master output)
    /// switches too. Which run boundary a switch point inside a run lands
    /// on (the nearest) is the caller's choice of value. // RT
    ///
    /// Only while no run is in flight: after `Exec::try_finish` returned
    /// true (or `Runner::graph_mut`), never while a straggling worker of a
    /// missed run may still hold `&Graph` (module protocol).
    pub fn commit_controls(&mut self, ramp: bool) {
        let defs = &mut self.defs;
        let inputs = &mut self.inputs;
        let wrote = self.controls.commit(
            |node, gl, gr, on| {
                if let Some(NodeDef::Strip {
                    gain_l,
                    gain_r,
                    start_l,
                    start_r,
                    audible,
                    ..
                }) = defs.get_mut(node)
                {
                    if ramp {
                        *start_l = *gain_l;
                        *start_r = *gain_r;
                    } else {
                        *start_l = gl;
                        *start_r = gr;
                    }
                    *gain_l = gl;
                    *gain_r = gr;
                    std::mem::replace(audible, on) != on
                } else {
                    false
                }
            },
            |i, g, switched| {
                if let Some(x) = inputs.get_mut(i) {
                    x.start = if ramp && !switched { x.gain } else { g };
                    x.gain = g;
                }
            },
        );
        if wrote && ramp {
            self.ramping = true;
        }
    }

    /// After a run: ramps have arrived. // RT
    ///
    /// Only while no run is in flight: after `Exec::try_finish` returned
    /// true (or `Runner::graph_mut`), never while a straggling worker of a
    /// missed run may still hold `&Graph` (module protocol).
    pub fn settle(&mut self) {
        if !std::mem::take(&mut self.ramping) {
            return;
        }
        for d in self.defs.iter_mut() {
            if let NodeDef::Strip {
                gain_l,
                gain_r,
                start_l,
                start_r,
                ..
            } = d
            {
                *start_l = *gain_l;
                *start_r = *gain_r;
            }
        }
        for x in self.inputs.iter_mut() {
            x.start = x.gain;
        }
    }

    /// The processor of insert instance `id`, between runs (plugin
    /// automation). // RT
    ///
    /// Only while no run is in flight: after `Exec::try_finish` returned
    /// true (or `Runner::graph_mut`), never while a straggling worker of a
    /// missed run may still hold `&Graph` (module protocol).
    pub fn insert_mut(&mut self, id: PluginInstanceId) -> Option<&mut (dyn EffectNode + 'static)> {
        let i = self.insert_ids.binary_search_by_key(&id, |x| x.0).ok()?;
        let idx = self.insert_ids.get(i)?.1;
        self.inserts
            .get_mut(idx)?
            .get_mut()
            .as_mut()?
            .node
            .as_deref_mut()
    }

    /// A strip node's gains and audibility at the end of the run
    /// (diagnostics and tests). Not RT.
    pub fn gains(&self, node: usize) -> Option<(f32, f32, bool)> {
        match self.defs.get(node) {
            Some(NodeDef::Strip {
                gain_l,
                gain_r,
                audible,
                ..
            }) => Some((*gain_l, *gain_r, *audible)),
            _ => None,
        }
    }

    /// The gains of node `node`'s insert sidechain feeds, in insert order
    /// (inserts without one left out; diagnostics and tests). Not RT.
    pub fn sidechain_gains(&self, node: usize) -> Vec<f32> {
        match self.defs.get(node) {
            Some(NodeDef::Strip { inserts, .. }) => self
                .insert_defs
                .get(inserts.clone())
                .unwrap_or(&[])
                .iter()
                .filter_map(|d| d.sidechain.as_ref().map(|x| x.gain))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The gains of node `node`'s input edges, in spec order (dropped edges
    /// left out; diagnostics and tests). Not RT.
    pub fn edge_gains(&self, node: usize) -> Vec<f32> {
        self.input_keys
            .iter()
            .zip(self.inputs.iter())
            .filter(|(k, _)| k.0 == node)
            .map(|(_, x)| x.gain)
            .collect()
    }

    /// Moves adopted inserts out of `old` (the graph this one replaces) and,
    /// when both have the same shape and block size, swaps in `old`'s node
    /// state (buffers and delay lines). Runs between runs. // RT
    pub(crate) fn adopt_from(&mut self, old: &mut Graph) {
        for (new, prev, line) in self.adopt.iter_mut() {
            let mut taken = old.inserts.get_mut(*prev).and_then(|c| c.get_mut().take());
            if let Some(st) = taken.as_mut() {
                // The spec latency changed: use the line `build` allocated
                // (a swap, so nothing is freed here; the old line goes with
                // this graph's `adopt`).
                if st.node.is_some() && st.bypass.delay() != line.delay() {
                    std::mem::swap(&mut st.bypass, line);
                    st.in_bypass = false;
                }
            }
            if let Some(cell) = self.inserts.get_mut(*new) {
                *cell.get_mut() = taken;
            }
        }
        if self.shape == old.shape
            && self.max_frames == old.max_frames
            && self.nodes.len() == old.nodes.len()
        {
            std::mem::swap(&mut self.nodes, &mut old.nodes);
        }
        // Automation overrides carry over while their static values stay
        // (Plan 4d Ruling 13).
        if self.controls.adopt(&old.controls) {
            self.commit_controls(false);
        }
    }

    /// Reallocates node buffers for blocks of up to `max_frames` (the
    /// renderer calls this from `Renderer::prepare`). Not RT.
    pub fn prepare(&mut self, max_frames: u32) {
        for c in self.nodes.iter_mut() {
            c.get_mut().resize(max_frames);
        }
        self.max_frames = max_frames;
    }

    /// Writes master's output (silence when it is muted) into frames
    /// `offset..offset + frames` of channels 0–1 of `out`, which the caller
    /// zeroed. Call only after a completed run. // RT
    pub(crate) fn write_master(&self, out: &mut [&mut [f32]], offset: usize, frames: u32) {
        let audible = matches!(
            self.defs.get(self.master),
            Some(NodeDef::Strip { audible: true, .. })
        );
        let Some(cell) = self.nodes.get(self.master) else {
            return;
        };
        if !audible {
            return;
        }
        // SAFETY: the caller calls this only after `Exec::run` returned
        // `Done` (`remaining == 0`, read with `Acquire`, so every node's
        // writes are visible) and before it starts another run; threads still
        // leaving the job touch only the queue and the atomic counters, never
        // node state.
        let st = unsafe { &*cell.get() };
        let n = frames as usize;
        for (ch, src) in out.iter_mut().zip([&st.post.left, &st.post.right]) {
            if let (Some(d), Some(s)) = (ch.get_mut(offset..offset + n), src.get(..n)) {
                d.copy_from_slice(s);
            }
        }
    }
}

/// Runs one insert with the fault rule of spec §7. // RT
fn run_insert(
    ins: &mut InsertState,
    bypass: bool,
    frames: u32,
    io: &mut StereoBuf,
    sc: Option<&StereoBuf>,
    shared: &Shared,
    info: &BlockInfo,
) {
    if bypass || ins.faulted {
        if !ins.in_bypass {
            ins.bypass.clear();
            ins.in_bypass = true;
        }
        ins.bypass.process_buf(io, frames);
        return;
    }
    ins.in_bypass = false;
    let Some(node) = ins.node.as_mut() else {
        return;
    };
    let status = node.process_block(frames, io, sc, info);
    if status == ProcessStatus::Error || dsp::is_faulty(io, frames) {
        io.clear(frames);
        ins.bad_run = ins.bad_run.saturating_add(1);
        if ins.bad_run >= FAULT_LIMIT {
            ins.faulted = true;
            node.reset();
            shared.report_plugin_fault(ins.instance);
        }
    } else {
        ins.bad_run = 0;
    }
}

/// Renders node `i`. // RT
///
/// # Safety
/// `i` was popped from `g.queue` in the current run by the calling thread,
/// and `sinks` are that run's sinks, which the owner does not touch until
/// the run is finished (module protocol).
unsafe fn render_node(
    g: &Graph,
    sinks: &SinkTable,
    shared: &Shared,
    frames: u32,
    info: &BlockInfo,
    i: usize,
) {
    let (Some(def), Some(cell)) = (g.defs.get(i), g.nodes.get(i)) else {
        return;
    };
    // SAFETY: a node is pushed once per run (as a source, or by the thread
    // that took its pending count to zero) and this thread popped `i`, so
    // no other thread renders it; its predecessors are finished and never
    // read `i`, its successors are not yet pushed, and the owner touches no
    // node state during a run. Hence this is the only reference to node `i`.
    let st = unsafe { &mut *cell.get() };
    match def {
        NodeDef::Instrument { target } => {
            // SAFETY: `Graph::build` gives each target to at most one node,
            // so this thread (which popped `i`) is the only one using this
            // sink during the run; the owner only reaches sinks through
            // `&mut SinkTable` between runs.
            let sink = sinks.cells.get(*target).map(|c| unsafe { &mut *c.get() });
            match sink {
                Some(Some(Sink::Instrument(node))) => {
                    node.render_outputs(frames, &mut st.outs, info);
                    if let Some(id) = node.take_fault() {
                        shared.report_plugin_fault(Some(id));
                    }
                }
                _ => {
                    for o in st.outs.iter_mut() {
                        o.clear(frames);
                    }
                }
            }
        }
        NodeDef::Strip {
            inputs,
            inserts,
            gain_l,
            gain_r,
            start_l,
            start_r,
            audible,
            meter,
        } => {
            st.pre.clear(frames);
            for (k, inp) in g
                .inputs
                .get(inputs.clone())
                .unwrap_or(&[])
                .iter()
                .enumerate()
            {
                // SAFETY: `build` drops self-edges, so `inp.from != i`.
                // Node `inp.from` is a predecessor of `i`: it completed and
                // decremented `pending[i]` (`AcqRel`) before `i` was pushed,
                // and the queue's push/pop synchronise, so its writes are
                // visible; it is not rendered again in this run, so only
                // shared references to it exist.
                let Some(src) = g.nodes.get(inp.from).map(|c| unsafe { &*c.get() }) else {
                    continue;
                };
                let buf = tap_buf(g, inp.from, src, inp.tap);
                match st.delays.get_mut(k) {
                    Some(d) if d.delay() > 0 => {
                        dsp::copy(&mut st.scratch, buf, frames);
                        d.process_buf(&mut st.scratch, frames);
                        dsp::mix_into_ramp(&mut st.pre, &st.scratch, inp.start, inp.gain, frames);
                    }
                    _ => dsp::mix_into_ramp(&mut st.pre, buf, inp.start, inp.gain, frames),
                }
            }
            for (k, idx) in inserts.clone().enumerate() {
                let (Some(idef), Some(icell)) = (g.insert_defs.get(idx), g.inserts.get(idx)) else {
                    continue;
                };
                // SAFETY: `build` gives every insert index to exactly one
                // strip's `inserts` range, so insert `idx` is touched only
                // while strip `i` renders, by this thread alone (as above).
                let Some(ins) = (unsafe { &mut *icell.get() }).as_mut() else {
                    continue;
                };
                let sc = match &idef.sidechain {
                    Some(x) => {
                        st.sc.clear(frames);
                        // SAFETY: as for inputs: `build` drops self
                        // sidechains, and `x.from` is a finished predecessor
                        // of `i` (its sidechain edge counts in `pending`).
                        if let Some(src) = g.nodes.get(x.from).map(|c| unsafe { &*c.get() }) {
                            let buf = tap_buf(g, x.from, src, x.tap);
                            dsp::mix_into(&mut st.sc, buf, x.gain, frames);
                        }
                        if let Some(d) = st.sc_delays.get_mut(k) {
                            d.process_buf(&mut st.sc, frames);
                        }
                        Some(&st.sc)
                    }
                    None => None,
                };
                run_insert(ins, idef.bypass, frames, &mut st.pre, sc, shared, info);
            }
            dsp::copy(&mut st.post, &st.pre, frames);
            dsp::scale_ramp(
                &mut st.post,
                (*start_l, *start_r),
                (*gain_l, *gain_r),
                frames,
            );
            if *audible {
                let (l, r) = dsp::peaks(&st.post, frames);
                if let Some(m) = shared.meter(*meter) {
                    m.hold(l, r);
                }
            }
        }
    }
}

/// One block's work, published to workers while it runs.
pub(crate) struct Job {
    graph: *const Graph,
    sinks: *const SinkTable,
    shared: *const Shared,
    pool: Option<*const PoolShared>,
    frames: u32,
    info: BlockInfo,
}

// SAFETY: the pointees (`Graph`, `SinkTable`, `Shared`, `PoolShared`) are
// all `Sync`, and the pointers are dereferenced only under the contracts of
// `step` and `remaining`, which keep the pointees alive and unchanged while
// any thread uses the job.
unsafe impl Send for Job {}
// SAFETY: as for `Send`; `Job` has no interior mutability, so shared use
// from several threads only reads its fields.
unsafe impl Sync for Job {}

impl Job {
    fn empty() -> Job {
        Job {
            graph: std::ptr::null(),
            sinks: std::ptr::null(),
            shared: std::ptr::null(),
            pool: None,
            frames: 0,
            info: BlockInfo::default(),
        }
    }

    /// Renders one ready node and completes it; false when the queue was
    /// empty. // RT
    ///
    /// # Safety
    /// The job belongs to the run in flight: `Exec::run` filled it with
    /// pointers to that run's graph, sinks and shared state, and either the
    /// caller is that `Exec`'s owner (before `try_finish` succeeds) or a
    /// worker that saw it published (and counts in `in_job`). Those pointees
    /// stay alive and the owner does not touch them mutably until then.
    pub(crate) unsafe fn step(&self) -> bool {
        // SAFETY: the caller's contract makes all three pointers valid for
        // shared use for the duration of this call.
        let (g, sinks, shared) = unsafe { (&*self.graph, &*self.sinks, &*self.shared) };
        let Some(i) = g.queue.pop() else {
            return false;
        };
        // SAFETY: this thread popped `i` from this run's queue just above,
        // and `sinks` are this run's sinks (caller's contract).
        unsafe { render_node(g, sinks, shared, self.frames, &self.info, i) };
        let mut ready = 0u32;
        for &s in g.succ.get(i).map(|v| &**v).unwrap_or(&[]) {
            if let Some(p) = g.pending.get(s) {
                if p.fetch_sub(1, Ordering::AcqRel) == 1 {
                    let _ = g.queue.push(s);
                    ready += 1;
                }
            }
        }
        if ready > 1 {
            if let Some(pool) = self.pool {
                // SAFETY: `pool` points into the `Arc<PoolShared>` the
                // publishing `Exec` holds; `Exec` drops that `Arc` only after
                // `wait_idle`, i.e. after every thread left this job.
                unsafe { &*pool }.wake(ready - 1);
            }
        }
        g.remaining.fetch_sub(1, Ordering::AcqRel);
        true
    }

    /// Nodes of this run not yet completed. // RT
    ///
    /// # Safety
    /// As for [`Job::step`]; an empty job (null graph) is always fine.
    pub(crate) unsafe fn remaining(&self) -> u32 {
        if self.graph.is_null() {
            return 0;
        }
        // SAFETY: the caller's contract keeps the graph alive for this call.
        unsafe { &*self.graph }.remaining.load(Ordering::Acquire)
    }
}

/// What a run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunResult {
    /// Every node completed; master's output is ready.
    Done,
    /// The deadline passed first; nodes still running finish in the
    /// background.
    Missed,
}

/// The executor, owned by a renderer or a [`Runner`].
///
/// The published [`Job`] lives in its own heap allocation that `Exec` only
/// reaches through a raw pointer, so borrowing the `Exec` mutably (to call
/// `try_finish` while stragglers of a missed run still read the job) never
/// asserts unique access to memory the workers read. Dropping an `Exec`
/// waits for a run in flight, so it must be dropped before that run's
/// graph, sinks and shared state.
pub(crate) struct Exec {
    pool: Option<Arc<PoolShared>>,
    /// From `Box::into_raw`; freed in `Drop` once no run is in flight.
    job: NonNull<Job>,
    running: bool,
}

// SAFETY: `job` is owned by this `Exec` (a leaked box) and its pointers are
// only dereferenced under the module protocol; moving an `Exec` between
// threads (a renderer handed to or back from the audio thread) does not
// move what they point at.
unsafe impl Send for Exec {}

impl Exec {
    pub(crate) fn new(pool: Option<Arc<PoolShared>>) -> Box<Exec> {
        Box::new(Exec {
            pool,
            job: NonNull::from(Box::leak(Box::new(Job::empty()))),
            running: false,
        })
    }

    /// The job, read-only. // RT
    fn job(&self) -> &Job {
        // SAFETY: `job` came from `Box::leak` and is freed only in `Drop`;
        // `run` writes it only while no run is in flight, when no reference
        // from `job()` is alive (it borrows `self`) and no worker holds it.
        unsafe { self.job.as_ref() }
    }

    /// True when no run is in flight, finishing a completed one. Nodes
    /// still pending (a missed deadline) give false at once; when every
    /// node is done but a worker is still leaving the job, this spins until
    /// it has left or `until` passes (`None`: no limit). // RT
    pub(crate) fn try_finish(&mut self, until: Option<Instant>) -> bool {
        if !self.running {
            return true;
        }
        // SAFETY: `running` means the job is the run in flight, and the owner
        // keeps that run's graph alive until this returns true (the job
        // contract of `Job::step`).
        if unsafe { self.job().remaining() } != 0 {
            return false;
        }
        if let Some(p) = &self.pool {
            if !p.retire_job(until) {
                return false;
            }
        }
        self.running = false;
        true
    }

    /// Blocks until no run is in flight. Not RT.
    pub(crate) fn wait_idle(&mut self) {
        while !self.try_finish(None) {
            std::thread::sleep(Duration::from_micros(200));
        }
    }

    /// Renders `g` for one block. Call only after `try_finish` returned
    /// true. With a deadline, returns `Missed` once it passes with nodes
    /// still pending; `Done` may leave a worker still leaving the job, which
    /// the next `try_finish` waits for. // RT
    pub(crate) fn run(
        &mut self,
        g: &Graph,
        sinks: &SinkTable,
        shared: &Shared,
        frames: u32,
        info: BlockInfo,
        deadline: Option<Instant>,
    ) -> RunResult {
        // Never reset counters or the queue under a run still in flight.
        if !self.try_finish(deadline) {
            return RunResult::Missed;
        }
        for (p, d) in g.pending.iter().zip(g.indegree.iter()) {
            p.store(*d, Ordering::Relaxed);
        }
        g.remaining.store(
            u32::try_from(g.defs.len()).unwrap_or(u32::MAX),
            Ordering::Relaxed,
        );
        while g.queue.pop().is_some() {}
        for &s in g.sources.iter() {
            let _ = g.queue.push(s);
        }
        let pool = self.pool.as_deref().filter(|p| p.active());
        // SAFETY: no run is in flight (`try_finish` returned true above), so
        // no worker holds the job and the pool's pointer to it is null; this
        // is the only access to the job's memory until it is published.
        unsafe {
            *self.job.as_ptr() = Job {
                graph: g,
                sinks,
                shared,
                pool: pool.map(|p| p as *const PoolShared),
                frames,
                info,
            };
        }
        self.running = true;
        let helps = pool.is_none_or(PoolShared::callback_helps);
        if let Some(p) = pool {
            p.publish(self.job.as_ptr(), g.sources.len());
        }
        let job = self.job();
        loop {
            // SAFETY: this thread filled the job with pointers to `g`,
            // `sinks` and `shared`, borrowed for this call; after it returns
            // the owner keeps them alive and unchanged until `try_finish`
            // succeeds.
            if helps && unsafe { job.step() } {
                continue;
            }
            // SAFETY: as above.
            if unsafe { job.remaining() } == 0 {
                break;
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return RunResult::Missed;
            }
            std::hint::spin_loop();
        }
        let _ = self.try_finish(deadline);
        RunResult::Done
    }
}

impl Drop for Exec {
    fn drop(&mut self) {
        // Workers may still read the job of a missed run; free it only once
        // they have left.
        self.wait_idle();
        // SAFETY: `job` came from `Box::leak` in `new` and no run is in
        // flight, so nothing else points at it.
        drop(unsafe { Box::from_raw(self.job.as_ptr()) });
    }
}

/// Runs a graph without a renderer or schedule: the harness for graph
/// tests and benchmarks. `block` drives exactly the renderer's executor.
pub struct Runner {
    exec: Box<Exec>,
    graph: Box<Graph>,
    sinks: Box<SinkTable>,
    shared: Arc<Shared>,
    pool: Option<Pool>,
    deadline: Option<Duration>,
}

impl Runner {
    /// Everything on the calling thread (`W = 0`).
    pub fn serial(graph: Graph) -> Runner {
        Runner {
            exec: Exec::new(None),
            graph: Box::new(graph),
            sinks: Box::default(),
            shared: Arc::new(Shared::default()),
            pool: None,
            deadline: None,
        }
    }

    /// The calling thread plus `workers` pool threads from `platform`.
    pub fn parallel(
        graph: Graph,
        workers: usize,
        platform: Arc<dyn RtPlatform>,
    ) -> Result<Runner, EngineError> {
        let shared = Arc::new(Shared::default());
        let pool = Pool::new(workers, platform, Arc::clone(&shared))?;
        Ok(Runner {
            exec: Exec::new(Some(pool.handle())),
            graph: Box::new(graph),
            sinks: Box::default(),
            shared,
            pool: Some(pool),
            deadline: None,
        })
    }

    /// Faults, meters and counters.
    pub fn shared(&self) -> &Shared {
        &self.shared
    }

    /// The instrument sinks (install test instruments here).
    pub fn sinks_mut(&mut self) -> &mut SinkTable {
        self.exec.wait_idle();
        &mut self.sinks
    }

    /// Each block must finish within `d` of its start.
    pub fn set_deadline(&mut self, d: Option<Duration>) {
        self.deadline = d;
    }

    /// Whether the calling thread renders nodes too (default yes).
    pub fn set_callback_helps(&self, on: bool) {
        if let Some(p) = &self.pool {
            p.set_callback_helps(on);
        }
    }

    /// The graph, between blocks (tests drive automation through it).
    pub fn graph_mut(&mut self) -> &mut Graph {
        self.exec.wait_idle();
        &mut self.graph
    }

    /// Hands the workers a new real-time environment.
    pub fn set_env(&self, env: RtEnv) {
        if let Some(p) = &self.pool {
            p.set_env(env);
        }
    }

    /// Renders one block into `left`/`right` (zeroed first); false when it
    /// is silent because the deadline was missed or stragglers still run.
    pub fn block(&mut self, frames: u32, left: &mut [f32], right: &mut [f32]) -> bool {
        left.fill(0.0);
        right.fill(0.0);
        let deadline = self.deadline.map(|d| Instant::now() + d);
        if !self.exec.try_finish(deadline) {
            return false;
        }
        match self.exec.run(
            &self.graph,
            &self.sinks,
            &self.shared,
            frames,
            BlockInfo::default(),
            deadline,
        ) {
            RunResult::Done => {
                self.graph.write_master(&mut [left, right], 0, frames);
                true
            }
            RunResult::Missed => false,
        }
    }

    /// Renders `total` frames in blocks cycling through `sizes` (clamped to
    /// the graph's block size); returns both channels.
    pub fn render(&mut self, total: usize, sizes: &[u32]) -> (Vec<f32>, Vec<f32>) {
        let max = self.graph.max_frames().max(1) as usize;
        let (mut l, mut r) = (vec![0.0f32; max], vec![0.0f32; max]);
        let (mut out_l, mut out_r) = (Vec::with_capacity(total), Vec::with_capacity(total));
        let mut k = 0usize;
        while out_l.len() < total {
            let size = sizes.get(k % sizes.len().max(1)).copied().unwrap_or(64) as usize;
            k += 1;
            let n = size.clamp(1, max).min(total - out_l.len());
            let (Some(bl), Some(br)) = (l.get_mut(..n), r.get_mut(..n)) else {
                break;
            };
            self.block(u32::try_from(n).unwrap_or(1), bl, br);
            out_l.extend_from_slice(bl);
            out_r.extend_from_slice(br);
        }
        (out_l, out_r)
    }

    /// Swaps in `graph`, which adopts from the current one (as a renderer
    /// does on `SetGraph`); returns the old graph.
    pub fn replace_graph(&mut self, mut graph: Graph) -> Graph {
        self.exec.wait_idle();
        graph.adopt_from(&mut self.graph);
        std::mem::replace(&mut *self.graph, graph)
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.exec.wait_idle();
    }
}
