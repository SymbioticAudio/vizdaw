//! The document: settings, instrument list, outputs, media pool and
//! sequences, plus a derived event index.

use crate::error::{ModelError, Result};
use crate::event::Event;
use crate::ids::{
    AnyId, Counters, DestId, EventId, LaneId, MediaId, OutputPathId, PluginInstanceId, SeqId,
    StripId, TakeId,
};
use crate::lane::Lane;
use crate::mixer::{BUILTIN_SYNTH_REF, Mixer, PluginRef, PluginSlot, derive_mixer};
use crate::modify::ModifyTemplate;
use crate::sequence::Sequence;
use crate::time::Tick;
use im::OrdMap;
use serde::{Deserialize, Serialize};

/// Project-wide settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Undo depth limit (default 200).
    pub history_depth: u32,
    /// Autosave copies to keep (default 10).
    pub autosave_keep: u32,
    /// Named select-and-modify templates.
    pub templates: OrdMap<String, ModifyTemplate>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            history_depth: 200,
            autosave_keep: 10,
            templates: OrdMap::new(),
        }
    }
}

/// An instrument-list entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// Destination ID.
    pub id: DestId,
    /// Display name.
    pub name: String,
    /// Where events go.
    pub kind: DestKind,
    /// Patch sent when the destination is first used.
    pub default_patch: Option<Patch>,
}

/// Destination kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DestKind {
    /// A MIDI endpoint with a UMP group and channel (each 0–15).
    MidiEndpoint {
        /// Opaque endpoint reference (defined by 2b).
        endpoint_ref: String,
        /// UMP group.
        group: u8,
        /// Channel.
        channel: u8,
    },
    /// A hosted plugin instrument (group 0, channel 0) and its own slot.
    PluginInstrument {
        /// The instrument's plugin instance.
        slot: PluginSlot,
    },
}

impl DestKind {
    /// A plugin instrument whose slot holds `PluginRef::from_legacy(name)`
    /// with instance ID 0; `AddDestination` replaces the ID with a fresh one.
    pub fn plugin(name: &str) -> DestKind {
        DestKind::PluginInstrument {
            slot: PluginSlot::new(PluginInstanceId(0), PluginRef::from_legacy(name)),
        }
    }

    /// True for MIDI endpoints that play on the built-in synth.
    pub fn is_builtin_synth(&self) -> bool {
        matches!(self, DestKind::MidiEndpoint { endpoint_ref, .. } if endpoint_ref == BUILTIN_SYNTH_REF)
    }
}

/// Where a plugin slot lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotOwner {
    /// A plugin instrument destination's own slot.
    Instrument(DestId),
    /// Insert `index` of a strip.
    Insert(StripId, usize),
}

/// Program and optional bank `(msb, lsb)`, each 0–127.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// Program 0–127.
    pub program: u8,
    /// Bank select.
    pub bank: Option<(u8, u8)>,
}

/// An audio output path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPath {
    /// Output ID.
    pub id: OutputPathId,
    /// Display name.
    pub name: String,
    /// Opaque engine bus or hardware output reference.
    pub reference: String,
}

/// A media-pool item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaItem {
    /// Media ID.
    pub id: MediaId,
    /// Path relative to the package, e.g. `Media/kick.wav`.
    pub path: String,
    /// Sample rate in Hz (> 0).
    pub sample_rate: u32,
    /// Channel count (> 0).
    pub channels: u16,
    /// Length in sample frames.
    pub length_frames: u64,
    /// Optional content hash.
    pub content_hash: Option<String>,
}

/// Where an event lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventLoc {
    /// Owning sequence.
    pub seq: SeqId,
    /// Owning lane.
    pub lane: LaneId,
    /// Owning take.
    pub take: TakeId,
    /// The event's tick (part of its take key).
    pub tick: Tick,
}

/// A whole project. Immutable from outside the crate: change it with
/// commands. Cloning is cheap (persistent collections).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "DocumentRepr", try_from = "DocumentRepr")]
pub struct Document {
    settings: Settings,
    instruments: OrdMap<DestId, Destination>,
    outputs: OrdMap<OutputPathId, OutputPath>,
    media: OrdMap<MediaId, MediaItem>,
    sequences: OrdMap<SeqId, Sequence>,
    mixer: Mixer,
    counters: Counters,
    /// Derived, never serialized: event ID -> location.
    index: OrdMap<EventId, EventLoc>,
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

/// The serialized shape of a [`Document`]: collections are arrays ordered by
/// ID, each element carrying its own ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentRepr {
    /// Settings.
    pub settings: Settings,
    /// Instrument list.
    pub instruments: Vec<Destination>,
    /// Output paths.
    pub outputs: Vec<OutputPath>,
    /// Media pool.
    pub media: Vec<MediaItem>,
    /// Sequences.
    pub sequences: Vec<Sequence>,
    /// The mixer; `None` (version-1 input, programmatic construction)
    /// derives one (Plan Ruling 6).
    #[serde(default)]
    pub mixer: Option<Mixer>,
    /// ID counters.
    pub counters: Counters,
}

impl From<Document> for DocumentRepr {
    fn from(d: Document) -> Self {
        DocumentRepr {
            settings: d.settings,
            instruments: d.instruments.values().cloned().collect(),
            outputs: d.outputs.values().cloned().collect(),
            media: d.media.values().cloned().collect(),
            sequences: d.sequences.values().cloned().collect(),
            mixer: Some(d.mixer),
            counters: d.counters,
        }
    }
}

fn collect_unique<K: Ord + Clone + std::fmt::Display, V: Clone>(
    items: Vec<V>,
    key: impl Fn(&V) -> K,
    what: &str,
) -> std::result::Result<OrdMap<K, V>, String> {
    let mut m = OrdMap::new();
    for v in items {
        let k = key(&v);
        if m.insert(k.clone(), v).is_some() {
            return Err(format!("duplicate {what} id {k}"));
        }
    }
    Ok(m)
}

impl TryFrom<DocumentRepr> for Document {
    type Error = String;
    fn try_from(r: DocumentRepr) -> std::result::Result<Self, String> {
        let instruments = collect_unique(r.instruments, |v| v.id, "destination")?;
        let outputs = collect_unique(r.outputs, |v| v.id, "output")?;
        let mut counters = r.counters;
        let mixer = match r.mixer {
            Some(m) => m,
            None => derive_mixer(instruments.values(), outputs.values(), &mut counters),
        };
        let mut d = Document {
            settings: r.settings,
            instruments,
            outputs,
            media: collect_unique(r.media, |v| v.id, "media")?,
            sequences: collect_unique(r.sequences, |v| v.id, "sequence")?,
            mixer,
            counters,
            index: OrdMap::new(),
        };
        d.rebuild_index()?;
        Ok(d)
    }
}

impl Document {
    /// An empty document with default settings and a mixer holding only
    /// the master strip.
    pub fn new() -> Document {
        let mut counters = Counters::default();
        let mixer = Mixer::new(counters.alloc_strip());
        Document {
            settings: Settings::default(),
            instruments: OrdMap::new(),
            outputs: OrdMap::new(),
            media: OrdMap::new(),
            sequences: OrdMap::new(),
            mixer,
            counters,
            index: OrdMap::new(),
        }
    }

    /// Builds a document from parts (used by importers and tests). Rejects
    /// duplicate IDs; run [`crate::rules::validate`] for the full checks.
    pub fn from_parts(repr: DocumentRepr) -> Result<Document> {
        Document::try_from(repr).map_err(|p| ModelError::Corrupt { problems: vec![p] })
    }

    /// Settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// Instrument list.
    pub fn instruments(&self) -> &OrdMap<DestId, Destination> {
        &self.instruments
    }
    /// Output paths.
    pub fn outputs(&self) -> &OrdMap<OutputPathId, OutputPath> {
        &self.outputs
    }
    /// Media pool.
    pub fn media(&self) -> &OrdMap<MediaId, MediaItem> {
        &self.media
    }
    /// Sequences.
    pub fn sequences(&self) -> &OrdMap<SeqId, Sequence> {
        &self.sequences
    }
    /// ID counters.
    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// The mixer.
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// Where plugin instance `id` lives.
    pub fn find_slot(&self, id: PluginInstanceId) -> Option<SlotOwner> {
        for d in self.instruments.values() {
            if let DestKind::PluginInstrument { slot } = &d.kind {
                if slot.id == id {
                    return Some(SlotOwner::Instrument(d.id));
                }
            }
        }
        self.mixer
            .inserts()
            .find(|(_, _, p)| p.id == id)
            .map(|(s, i, _)| SlotOwner::Insert(s, i))
    }

    /// Plugin instance `id`'s slot.
    pub fn plugin_slot(&self, id: PluginInstanceId) -> Option<&PluginSlot> {
        match self.find_slot(id)? {
            SlotOwner::Instrument(d) => match &self.instruments.get(&d)?.kind {
                DestKind::PluginInstrument { slot } => Some(slot),
                DestKind::MidiEndpoint { .. } => None,
            },
            SlotOwner::Insert(s, i) => self.mixer.strip(s).ok()?.inserts.get(i),
        }
    }

    /// A sequence by ID.
    pub fn sequence(&self, id: SeqId) -> Result<&Sequence> {
        self.sequences
            .get(&id)
            .ok_or(ModelError::UnknownId(AnyId::Seq(id)))
    }

    /// A lane by ID.
    pub fn lane(&self, seq: SeqId, lane: LaneId) -> Result<&Lane> {
        self.sequence(seq)?
            .lane(lane)
            .ok_or(ModelError::UnknownId(AnyId::Lane(lane)))
    }

    /// Where an event lives.
    pub fn locate(&self, id: EventId) -> Result<EventLoc> {
        self.index
            .get(&id)
            .copied()
            .ok_or(ModelError::UnknownId(AnyId::Event(id)))
    }

    /// An event and its location.
    pub fn event(&self, id: EventId) -> Result<(&Event, EventLoc)> {
        let loc = self.locate(id)?;
        let ev = self
            .lane(loc.seq, loc.lane)?
            .take(loc.take)
            .and_then(|t| t.events.get(&(loc.tick, id)))
            .ok_or(ModelError::UnknownId(AnyId::Event(id)))?;
        Ok((ev, loc))
    }

    /// Every event in every take, with its location.
    pub fn all_events(&self) -> impl Iterator<Item = (EventLoc, &Event)> + '_ {
        self.sequences.values().flat_map(|s| {
            s.lanes.iter().flat_map(move |l| {
                l.takes.iter().flat_map(move |t| {
                    t.events.values().map(move |e| {
                        (
                            EventLoc {
                                seq: s.id,
                                lane: l.id,
                                take: t.id,
                                tick: e.tick,
                            },
                            e,
                        )
                    })
                })
            })
        })
    }
}

/// Crate-internal mutation, used only by commands.
impl Document {
    pub(crate) fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }
    pub(crate) fn instruments_mut(&mut self) -> &mut OrdMap<DestId, Destination> {
        &mut self.instruments
    }
    pub(crate) fn outputs_mut(&mut self) -> &mut OrdMap<OutputPathId, OutputPath> {
        &mut self.outputs
    }
    pub(crate) fn media_mut(&mut self) -> &mut OrdMap<MediaId, MediaItem> {
        &mut self.media
    }
    pub(crate) fn counters_mut(&mut self) -> &mut Counters {
        &mut self.counters
    }

    /// Mutable sequence. Callers must not add or remove take events through
    /// it; use the event functions below so the index stays correct.
    pub(crate) fn seq_mut(&mut self, id: SeqId) -> Result<&mut Sequence> {
        self.sequences
            .get_mut(&id)
            .ok_or(ModelError::UnknownId(AnyId::Seq(id)))
    }

    /// Mutable lane, with the same caveat as [`Document::seq_mut`].
    pub(crate) fn lane_mut(&mut self, seq: SeqId, lane: LaneId) -> Result<&mut Lane> {
        self.seq_mut(seq)?
            .lanes
            .iter_mut()
            .find(|l| l.id == lane)
            .ok_or(ModelError::UnknownId(AnyId::Lane(lane)))
    }

    /// Inserts an event into a take (the active take when `take` is None).
    pub(crate) fn insert_event(
        &mut self,
        seq: SeqId,
        lane: LaneId,
        take: Option<TakeId>,
        ev: Event,
    ) -> Result<EventLoc> {
        if self.index.contains_key(&ev.id) {
            return Err(ModelError::invalid(format!(
                "event {} already exists",
                ev.id
            )));
        }
        let l = self.lane_mut(seq, lane)?;
        let take_id = take.unwrap_or(l.active_take);
        let t = l
            .takes
            .iter_mut()
            .find(|t| t.id == take_id)
            .ok_or(ModelError::UnknownId(AnyId::Take(take_id)))?;
        let loc = EventLoc {
            seq,
            lane,
            take: take_id,
            tick: ev.tick,
        };
        let id = ev.id;
        t.events.insert(ev.key(), ev);
        self.index.insert(id, loc);
        Ok(loc)
    }

    /// Removes an event wherever it is.
    pub(crate) fn remove_event(&mut self, id: EventId) -> Result<(Event, EventLoc)> {
        let loc = self.locate(id)?;
        let l = self.lane_mut(loc.seq, loc.lane)?;
        let ev = l
            .takes
            .iter_mut()
            .find(|t| t.id == loc.take)
            .and_then(|t| t.events.remove(&(loc.tick, id)))
            .ok_or(ModelError::UnknownId(AnyId::Event(id)))?;
        self.index.remove(&id);
        Ok((ev, loc))
    }

    /// Replaces an event (matched by ID) in place; its tick may change.
    pub(crate) fn replace_event(&mut self, ev: Event) -> Result<EventLoc> {
        let (_, loc) = self.remove_event(ev.id)?;
        self.insert_event(loc.seq, loc.lane, Some(loc.take), ev)
    }

    fn unindex_seq(&mut self, s: &Sequence) {
        for l in &s.lanes {
            self.unindex_lane(l);
        }
    }

    fn unindex_lane(&mut self, l: &Lane) {
        for t in &l.takes {
            for id in t.events.keys().map(|(_, id)| *id) {
                self.index.remove(&id);
            }
        }
    }

    fn index_seq(&mut self, s: &Sequence) {
        for l in &s.lanes {
            for t in &l.takes {
                for e in t.events.values() {
                    self.index.insert(
                        e.id,
                        EventLoc {
                            seq: s.id,
                            lane: l.id,
                            take: t.id,
                            tick: e.tick,
                        },
                    );
                }
            }
        }
    }

    /// Inserts or replaces a whole sequence, re-indexing its events.
    pub(crate) fn put_sequence(&mut self, s: Sequence) {
        if let Some(old) = self.sequences.get(&s.id).cloned() {
            self.unindex_seq(&old);
        }
        self.index_seq(&s);
        self.sequences.insert(s.id, s);
    }

    /// Removes a sequence and its events from the index.
    pub(crate) fn remove_sequence(&mut self, id: SeqId) -> Result<Sequence> {
        let s = self
            .sequences
            .remove(&id)
            .ok_or(ModelError::UnknownId(AnyId::Seq(id)))?;
        self.unindex_seq(&s);
        Ok(s)
    }

    /// Removes a lane and its events from the index.
    pub(crate) fn remove_lane(&mut self, seq: SeqId, lane: LaneId) -> Result<Lane> {
        let s = self.seq_mut(seq)?;
        let i = s
            .lane_index(lane)
            .ok_or(ModelError::UnknownId(AnyId::Lane(lane)))?;
        let l = s.lanes.remove(i);
        self.unindex_lane(&l);
        Ok(l)
    }

    /// Rebuilds the event index; fails on duplicate event IDs.
    pub(crate) fn rebuild_index(&mut self) -> std::result::Result<(), String> {
        let mut index = OrdMap::new();
        for (loc, e) in self.all_events() {
            if index.insert(e.id, loc).is_some() {
                return Err(format!("duplicate event id {}", e.id));
            }
        }
        self.index = index;
        Ok(())
    }
}
