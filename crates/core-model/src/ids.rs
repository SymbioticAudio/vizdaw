//! Typed ID newtypes and the per-document counters that allocate them.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident, $prefix:literal) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, self.0)
            }
        }
    };
}

id_type!(
    /// Identifies a sequence.
    SeqId,
    "seq"
);
id_type!(
    /// Identifies a lane (track) within a sequence.
    LaneId,
    "lane"
);
id_type!(
    /// Identifies an event; unique across the whole document.
    EventId,
    "ev"
);
id_type!(
    /// Identifies an instrument-list destination.
    DestId,
    "dest"
);
id_type!(
    /// Identifies an audio output path.
    OutputPathId,
    "out"
);
id_type!(
    /// Identifies a media-pool item.
    MediaId,
    "media"
);
id_type!(
    /// Identifies a take within a lane.
    TakeId,
    "take"
);
id_type!(
    /// Identifies a drum row within a drum lane.
    DrumRowId,
    "row"
);
id_type!(
    /// Identifies a mixer strip.
    StripId,
    "strip"
);
id_type!(
    /// Identifies a plugin instance (an insert or a plugin instrument's
    /// own slot); stable for the instance's life, never reused.
    PluginInstanceId,
    "plug"
);
id_type!(
    /// Identifies an immutable plugin state blob (the FNV-1a hash of its
    /// bytes), stored as `PluginState/<16 hex digits>.bin`.
    StateBlobId,
    "state"
);

/// Any ID, used in errors and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnyId {
    /// A sequence.
    Seq(SeqId),
    /// A lane.
    Lane(LaneId),
    /// An event.
    Event(EventId),
    /// A destination.
    Dest(DestId),
    /// An output path.
    Output(OutputPathId),
    /// A media item.
    Media(MediaId),
    /// A take.
    Take(TakeId),
    /// A drum row.
    DrumRow(DrumRowId),
    /// A mixer strip.
    Strip(StripId),
    /// A plugin instance.
    Plugin(PluginInstanceId),
}

impl fmt::Display for AnyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnyId::Seq(i) => i.fmt(f),
            AnyId::Lane(i) => i.fmt(f),
            AnyId::Event(i) => i.fmt(f),
            AnyId::Dest(i) => i.fmt(f),
            AnyId::Output(i) => i.fmt(f),
            AnyId::Media(i) => i.fmt(f),
            AnyId::Take(i) => i.fmt(f),
            AnyId::DrumRow(i) => i.fmt(f),
            AnyId::Strip(i) => i.fmt(f),
            AnyId::Plugin(i) => i.fmt(f),
        }
    }
}

macro_rules! any_from {
    ($t:ident, $v:ident) => {
        impl From<$t> for AnyId {
            fn from(i: $t) -> Self {
                AnyId::$v(i)
            }
        }
    };
}
any_from!(SeqId, Seq);
any_from!(LaneId, Lane);
any_from!(EventId, Event);
any_from!(DestId, Dest);
any_from!(OutputPathId, Output);
any_from!(MediaId, Media);
any_from!(TakeId, Take);
any_from!(DrumRowId, DrumRow);
any_from!(StripId, Strip);
any_from!(PluginInstanceId, Plugin);

/// Per-document ID counters. Each holds the next value to hand out; IDs are
/// never reused within a document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    /// Next sequence ID.
    pub seq: u64,
    /// Next lane ID.
    pub lane: u64,
    /// Next event ID.
    pub event: u64,
    /// Next destination ID.
    pub dest: u64,
    /// Next output-path ID.
    pub output: u64,
    /// Next media ID.
    pub media: u64,
    /// Next take ID.
    pub take: u64,
    /// Next drum-row ID.
    pub drum_row: u64,
    /// Next strip ID.
    pub strip: u64,
    /// Next plugin instance ID.
    pub plugin: u64,
}

fn bump(c: &mut u64) -> u64 {
    let v = *c;
    *c = c.saturating_add(1);
    v
}

impl Counters {
    /// Allocates a sequence ID.
    pub fn alloc_seq(&mut self) -> SeqId {
        SeqId(bump(&mut self.seq))
    }
    /// Allocates a lane ID.
    pub fn alloc_lane(&mut self) -> LaneId {
        LaneId(bump(&mut self.lane))
    }
    /// Allocates an event ID.
    pub fn alloc_event(&mut self) -> EventId {
        EventId(bump(&mut self.event))
    }
    /// Allocates a destination ID.
    pub fn alloc_dest(&mut self) -> DestId {
        DestId(bump(&mut self.dest))
    }
    /// Allocates an output-path ID.
    pub fn alloc_output(&mut self) -> OutputPathId {
        OutputPathId(bump(&mut self.output))
    }
    /// Allocates a media ID.
    pub fn alloc_media(&mut self) -> MediaId {
        MediaId(bump(&mut self.media))
    }
    /// Allocates a take ID.
    pub fn alloc_take(&mut self) -> TakeId {
        TakeId(bump(&mut self.take))
    }
    /// Allocates a drum-row ID.
    pub fn alloc_drum_row(&mut self) -> DrumRowId {
        DrumRowId(bump(&mut self.drum_row))
    }
    /// Allocates a strip ID.
    pub fn alloc_strip(&mut self) -> StripId {
        StripId(bump(&mut self.strip))
    }
    /// Allocates a plugin instance ID.
    pub fn alloc_plugin(&mut self) -> PluginInstanceId {
        PluginInstanceId(bump(&mut self.plugin))
    }
}
