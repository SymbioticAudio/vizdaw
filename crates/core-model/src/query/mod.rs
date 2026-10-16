//! Read-only queries over a [`crate::document::Document`].

pub mod length;
pub mod routing;
pub mod usage;

pub use length::{reference_span, seq_length};
pub use routing::{Routing, resolve, resolve_event};
pub use usage::{
    drum_view, media_in_use, unresolved_events, unused_sequences, use_count, use_counts, users_of,
};
