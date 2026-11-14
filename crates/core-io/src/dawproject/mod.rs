//! DAWproject (spec §6): the typed model of `project.xml` and
//! `metadata.xml`, read from a bounded XML [`Tree`](crate::xml::Tree) and
//! written in the XSD's order; the export mapping, its units, plug-in state
//! files and the return record.

pub mod export;
mod export_timeline;
pub mod import;
pub mod merge;
pub mod multiq;
pub mod package;
pub mod plugins;
pub mod record;
pub mod schema;
pub mod units;

pub use schema::*;
