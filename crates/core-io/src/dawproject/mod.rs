//! DAWproject (spec §6): the typed model of `project.xml` and
//! `metadata.xml`, read from a bounded XML [`Tree`](crate::xml::Tree) and
//! written in the XSD's order.

pub mod schema;

pub use schema::*;
