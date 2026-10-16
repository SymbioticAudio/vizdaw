//! Routing resolution: event dest, else lane dest; event group/channel,
//! else the destination's (plugins use group 0, channel 0).

use crate::document::{DestKind, Document};
use crate::error::Result;
use crate::event::Event;
use crate::ids::{DestId, EventId};
use crate::lane::Lane;

/// Where a MIDI event resolves to. `dest == None` means unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Routing {
    /// Resolved destination.
    pub dest: Option<DestId>,
    /// Resolved `(group, channel)`; `None` when there is neither an override
    /// nor a destination.
    pub group_channel: Option<(u8, u8)>,
}

/// Resolves an event's destination override against `fallback` (the lane's
/// destination).
pub fn resolve_with(doc: &Document, fallback: Option<DestId>, ev: &Event) -> Routing {
    let dest = ev.dest.or(fallback);
    let from_dest = dest
        .and_then(|d| doc.instruments().get(&d))
        .map(|d| match d.kind {
            DestKind::MidiEndpoint { group, channel, .. } => (group, channel),
            DestKind::PluginInstrument { .. } => (0, 0),
        });
    Routing {
        dest,
        group_channel: ev.group_channel.or(from_dest),
    }
}

/// Resolves an event on `lane`.
pub fn resolve(doc: &Document, lane: &Lane, ev: &Event) -> Routing {
    resolve_with(doc, lane.midi_dest, ev)
}

/// Resolves an event by ID.
pub fn resolve_event(doc: &Document, id: EventId) -> Result<Routing> {
    let (ev, loc) = doc.event(id)?;
    let lane = doc.lane(loc.seq, loc.lane)?;
    Ok(resolve(doc, lane, ev))
}
