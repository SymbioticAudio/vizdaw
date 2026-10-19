//! The mixer render graph (spec §6, §7): strip DSP (`dsp`), delay lines
//! (`delay`), insert processors (`node`), the graph description (`spec`),
//! PDC planning (`pdc`), the runtime graph and executor (`exec`) and the
//! mixer translation (`mixer`).

pub mod controls;
pub mod delay;
pub mod dsp;
pub mod exec;
pub mod mixer;
pub mod node;
pub mod pdc;
pub mod regions;
pub mod spec;

use core_model::ids::PluginInstanceId;

pub use controls::{FeedSpec, OverrideSnapshot, SendSpec, StripControlSpec};
pub use exec::{Graph, InsertInit, Runner, SinkTable};
pub use mixer::mixer_spec;
pub use mixer::{FeedSource, InstrumentFeed};
pub use node::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestInstrument, TestMode,
    TestNode,
};
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
pub use regions::RegionRun;
pub use spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};

/// What compiling the mixer graph did (`CompileReport::graph`, Plan
/// Ruling 13).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphReport {
    /// Nodes in the graph.
    pub nodes: usize,
    /// Total latency `L` in frames.
    pub latency_frames: u64,
    /// Slots whose processor could not be created, and why (they pass
    /// audio with no latency).
    pub placeholders: Vec<(PluginInstanceId, String)>,
    /// Other problems (no built-in strip, strips without an instrument).
    pub warnings: Vec<String>,
    /// Why this graph was not used; the previous one keeps playing.
    pub error: Option<String>,
}
