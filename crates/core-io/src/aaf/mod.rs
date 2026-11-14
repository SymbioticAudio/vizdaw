//! AAF (spec §7.5, plan 6f): our own reader and writer of the Advanced
//! Authoring Format on the `cfb` compound-file crate.
//!
//! - [`store`]: the object store over the compound file. It reads and
//!   writes objects, properties, strong references (single, vector, set),
//!   weak references and data streams, within [`AafLimits`]. Every file is
//!   hostile: names resolve only below the storage that holds them, every
//!   count in an index is checked against its stream's real length before
//!   anything is allocated, and a panic inside `cfb` becomes
//!   [`store::DAMAGED`].
//! - [`dict`]: the class, property and definition IDs we use, and the
//!   decoders of the values we read.
//! - [`timeline`]: the composition read into a neutral form (slots, clips,
//!   fades, gain, pan, markers, timecode) and the audio files it plays.
//! - [`import`]: a composition mapped onto 6d's import plan, its L/R pairs
//!   joined, and applied with the MIDI file beside it.
//! - [`metadict`]: the fixed MetaDictionary every export writes whole,
//!   generated from the model tables named below (their MIT notice is kept
//!   there).
//! - [`export`]: a sequence written as an Edit Protocol composition, with
//!   the audio to stage and the MIDI file beside it.
//!
//! The storage layout follows the public AAF Low-Level Container
//! Specification. Writers' habits were checked against pyaaf2
//! (<https://github.com/markreidvfx/pyaaf2>, MIT) and LibAAF
//! (<https://github.com/agfline/LibAAF>); no code is ported from either.
//!
//! [`AafLimits`]: crate::limits::AafLimits

pub mod dict;
pub mod export;
pub mod import;
pub mod metadict;
pub mod store;
pub mod timeline;
