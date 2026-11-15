//! The typed model of a DAWproject file (`project.xml` and `metadata.xml`,
//! format version 1; the XSD at `tests/data/dawproject/Project.xsd` is the
//! reference), reading it from a [`Tree`](crate::xml::Tree), writing it in the XSD's order and
//! checking its IDs.
//!
//! Every type is prefixed `Dp`, so a glob import never shadows a name in
//! `core_model`.
//!
//! **Reading** is lenient where it can be and reports what it leaves out:
//!
//! - an element or attribute the model lacks is skipped and counted
//!   ([`UNKNOWN_PARTS`]; a namespace declaration is not one);
//! - an element with a required attribute (or child) missing or unreadable is
//!   left out with its contents ([`UNREADABLE_PARTS`], the element's name as
//!   the detail), as is a timeline nested deeper than
//!   [`MAX_TIMELINE_NESTING`](crate::limits::MAX_TIMELINE_NESTING);
//! - an optional attribute that cannot be read is dropped
//!   ([`UNREADABLE_VALUES`], `Element@attribute` as the detail);
//! - every string is cleaned by [`text::sanitize`](crate::text::sanitize);
//! - references are kept as written: [`check_ids`] reports the ones that name
//!   nothing or the wrong kind of part, and the mappers treat them as absent;
//! - text the tree cut or dropped is not reported here: it is kept in
//!   [`DpMetaData::cut`] and `dropped_text`, for the import mappers to
//!   report.
//!
//! - the model is built under a byte budget
//!   ([`MAX_MODEL_BYTES`](crate::limits::MAX_MODEL_BYTES)); past it, or with
//!   an ID over [`MAX_ID_BYTES`], the file is refused. [`check_ids`] is
//!   bounded the same way.
//!
//! Reading recurses once per nested `Track` (the tree allows 256 levels) and
//! per level of timeline (16). A [`DpTrack`] is about 150 bytes, but the
//! reader's frames are larger: a thread that reads or drops a project needs
//! at least a 2 MiB stack (the test threads' size). Job threads (plan 6d
//! Tasks 9 and 10) must not be smaller.
//!
//! **Writing** keeps the XSD's element order, writes `id` only on the types
//! the XSD makes referenceable and which have one here (never on clips,
//! markers, notes, points and scenes: nothing refers to them). It never
//! writes what the XSD would refuse: a [`DpTimeline::Video`] (its media file
//! is not modelled), a [`DpTimeline::Unknown`], an empty `markers` timeline,
//! `Warps` with no warp or nothing to warp and a scene with no content are
//! left out and counted by [`write_project_reporting`]
//! ([`UNWRITABLE_PARTS`]); a project with no application gets VizDAW's. A
//! built-in device's [`DpDevice::settings`] (text for the report) are not
//! written.

mod ids;
mod model;
mod read;
mod write;

pub use ids::*;
pub use model::*;
pub use read::*;
pub use write::*;
