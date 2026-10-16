//! The mixer render graph (spec §6, §7): strip DSP (`dsp`), delay lines
//! (`delay`), insert processors (`node`), the graph description (`spec`),
//! PDC planning (`pdc`), the runtime graph and executor (`exec`) and the
//! mixer translation (`mixer`).

pub mod delay;
pub mod dsp;
pub mod exec;
pub mod mixer;
pub mod node;
pub mod pdc;
pub mod spec;

pub use exec::{Graph, InsertInit, Runner, SinkTable};
pub use node::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
};
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
pub use spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
