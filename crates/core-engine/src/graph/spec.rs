//! A platform-neutral description of a render graph, built from the mixer
//! by `graph::mixer` or directly by tests.

use crate::schedule::TargetIdx;
use core_model::ids::{PluginInstanceId, StripId};

/// Which buffer of a source strip an edge reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tap {
    /// After the inserts, before the fader (pre-fader sends).
    Pre,
    /// After fader and pan (outputs, post-fader sends, sidechains;
    /// instrument nodes write here).
    Post,
}

/// An edge into a strip: `gain × tap(from)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputSpec {
    /// Source node index.
    pub from: usize,
    /// Which buffer of the source.
    pub tap: Tap,
    /// Linear gain (send level, mute factor).
    pub gain: f32,
}

/// One insert of a strip.
#[derive(Debug, Clone, PartialEq)]
pub struct InsertSpec {
    /// Plugin instance (keys instance reuse); `None` in some tests.
    pub instance: Option<PluginInstanceId>,
    /// Latency in frames (kept while bypassed).
    pub latency: u32,
    /// Bypassed: audio passes through a delay of `latency`.
    pub bypass: bool,
    /// Sidechain input, aligned to this insert's position.
    pub sidechain: Option<InputSpec>,
}

/// A strip node: sums `inputs` in order, runs `inserts`, applies
/// `gain_l`/`gain_r` (fader × pan).
#[derive(Debug, Clone, PartialEq)]
pub struct StripSpec {
    /// The model strip (for meters); `None` in tests.
    pub strip: Option<StripId>,
    /// Inputs in summing order.
    pub inputs: Vec<InputSpec>,
    /// Inserts in processing order.
    pub inserts: Vec<InsertSpec>,
    /// Left gain after the inserts.
    pub gain_l: f32,
    /// Right gain after the inserts.
    pub gain_r: f32,
    /// False when muted (explicitly or by solo): metered silent and, for
    /// master, not written to the device.
    pub audible: bool,
    /// Meter cell this strip writes (`Shared::meter`); `None` uses the
    /// node index (graph tests). The engine gives every strip a cell keyed
    /// by its `StripId`, so meters survive structural edits.
    pub meter: Option<usize>,
}

impl StripSpec {
    /// A strip with no inputs or inserts, unity gains, audible.
    pub fn new(strip: Option<StripId>) -> StripSpec {
        StripSpec {
            strip,
            inputs: Vec::new(),
            inserts: Vec::new(),
            gain_l: 1.0,
            gain_r: 1.0,
            audible: true,
            meter: None,
        }
    }
}

/// A graph node.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeSpec {
    /// An instrument sink (the built-in synth in 3a) rendering into its
    /// node's `Post` buffer.
    Instrument {
        /// The engine target whose sink renders.
        target: TargetIdx,
        /// The instrument's latency in frames.
        latency: u32,
    },
    /// A mixer strip.
    Strip(StripSpec),
}

/// A whole graph; `master`'s output goes to the device.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphSpec {
    /// Nodes, by index.
    pub nodes: Vec<NodeSpec>,
    /// Index of the master strip.
    pub master: usize,
}

struct Fnv(u64);

impl Fnv {
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

fn tap_code(t: Tap) -> u64 {
    match t {
        Tap::Pre => 1,
        Tap::Post => 2,
    }
}

impl GraphSpec {
    /// The nodes node `i` reads: its inputs, then its sidechain sources
    /// (repeats kept; out-of-range indices included for `plan` to refuse).
    pub fn deps(&self, i: usize) -> Vec<usize> {
        match self.nodes.get(i) {
            Some(NodeSpec::Strip(s)) => s
                .inputs
                .iter()
                .map(|x| x.from)
                .chain(s.inserts.iter().filter_map(|k| k.sidechain.map(|x| x.from)))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A hash of the structure (Plan Ruling 15): equal shapes have equal
    /// plans and node layouts, so a new graph may take over the running
    /// graph's node state.
    pub fn shape(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.u64(self.nodes.len() as u64);
        h.u64(self.master as u64);
        for n in &self.nodes {
            match n {
                NodeSpec::Instrument { target, latency } => {
                    h.u64(1);
                    h.u64(u64::from(target.0));
                    h.u64(u64::from(*latency));
                }
                NodeSpec::Strip(s) => {
                    h.u64(2);
                    h.u64(s.inputs.len() as u64);
                    for x in &s.inputs {
                        h.u64(x.from as u64);
                        h.u64(tap_code(x.tap));
                    }
                    h.u64(s.inserts.len() as u64);
                    for k in &s.inserts {
                        h.u64(k.instance.map_or(u64::MAX, |p| p.0));
                        h.u64(u64::from(k.latency));
                        match &k.sidechain {
                            Some(x) => {
                                h.u64(x.from as u64);
                                h.u64(tap_code(x.tap));
                            }
                            None => h.u64(u64::MAX),
                        }
                    }
                }
            }
        }
        h.0
    }
}
