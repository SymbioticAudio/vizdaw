//! The mixer render graph (spec §6, §7). Built up over several modules:
//! strip DSP (`dsp`), delay lines (`delay`) and insert processors
//! (`node`).

pub mod delay;
pub mod dsp;
pub mod node;

pub use node::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
};
