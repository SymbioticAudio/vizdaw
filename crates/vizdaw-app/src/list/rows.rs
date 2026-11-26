//! A list's rows, columns and cell text (Swift `ListRows.swift`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use vizdaw_ffi::edit::{LaneRef, ViewFilterRow};
use vizdaw_ffi::events::{
    EventKindRow, EventRow, MarkerRow, MeterEntryRow, SequenceInfo, TempoPointRow, TextKindRow,
};

use super::format::{Bbu, Bend14, DurationText, KeyName, PitchName, Smpte, Value7, Velocity7};
use crate::arrangement::{bar_at, beat_ticks};

/// What a list row is: an event kind or a sequence map entry. `ui.json`
/// and the preferences spell them as Swift does (`note`, `pitchBend`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListKind {
    Note,
    Cc,
    PitchBend,
    ChannelPressure,
    PolyPressure,
    Program,
    SysEx,
    Text,
    Reference,
    AudioRegion,
    Tempo,
    Meter,
    Marker,
    Automation,
}

impl ListKind {
    /// Every kind, in Swift's order.
    pub const ALL: [ListKind; 14] = [
        ListKind::Note,
        ListKind::Cc,
        ListKind::PitchBend,
        ListKind::ChannelPressure,
        ListKind::PolyPressure,
        ListKind::Program,
        ListKind::SysEx,
        ListKind::Text,
        ListKind::Reference,
        ListKind::AudioRegion,
        ListKind::Tempo,
        ListKind::Meter,
        ListKind::Marker,
        ListKind::Automation,
    ];

    /// What Insert offers.
    pub const INSERTABLE: [ListKind; 11] = [
        ListKind::Note,
        ListKind::Cc,
        ListKind::PitchBend,
        ListKind::ChannelPressure,
        ListKind::PolyPressure,
        ListKind::Program,
        ListKind::SysEx,
        ListKind::Text,
        ListKind::Tempo,
        ListKind::Meter,
        ListKind::Marker,
    ];

    pub fn event_kind(self) -> Option<EventKindRow> {
        Some(match self {
            ListKind::Note => EventKindRow::Note,
            ListKind::Cc => EventKindRow::Cc,
            ListKind::PitchBend => EventKindRow::PitchBend,
            ListKind::ChannelPressure => EventKindRow::ChannelPressure,
            ListKind::PolyPressure => EventKindRow::PolyPressure,
            ListKind::Program => EventKindRow::Program,
            ListKind::SysEx => EventKindRow::SysEx,
            ListKind::Text => EventKindRow::Text,
            ListKind::Reference => EventKindRow::Reference,
            ListKind::AudioRegion => EventKindRow::AudioRegion,
            ListKind::Automation => EventKindRow::Automation,
            ListKind::Tempo | ListKind::Meter | ListKind::Marker => return None,
        })
    }

    /// Routed to a MIDI destination (the destination filter applies).
    pub fn is_midi(self) -> bool {
        matches!(
            self,
            ListKind::Note
                | ListKind::Cc
                | ListKind::PitchBend
                | ListKind::ChannelPressure
                | ListKind::PolyPressure
                | ListKind::Program
                | ListKind::SysEx
        )
    }

    pub fn title(self) -> &'static str {
        match self {
            ListKind::Note => "Note",
            ListKind::Cc => "Controller",
            ListKind::PitchBend => "Pitch Bend",
            ListKind::ChannelPressure => "Channel Pressure",
            ListKind::PolyPressure => "Poly Pressure",
            ListKind::Program => "Program",
            ListKind::SysEx => "SysEx",
            ListKind::Text => "Text",
            ListKind::Reference => "Reference",
            ListKind::AudioRegion => "Audio Region",
            ListKind::Tempo => "Tempo",
            ListKind::Meter => "Meter and Key",
            ListKind::Marker => "Marker",
            ListKind::Automation => "Automation",
        }
    }

    /// The Swift raw name (`ui.json`, preferences).
    pub fn name(self) -> &'static str {
        match self {
            ListKind::Note => "note",
            ListKind::Cc => "cc",
            ListKind::PitchBend => "pitchBend",
            ListKind::ChannelPressure => "channelPressure",
            ListKind::PolyPressure => "polyPressure",
            ListKind::Program => "program",
            ListKind::SysEx => "sysEx",
            ListKind::Text => "text",
            ListKind::Reference => "reference",
            ListKind::AudioRegion => "audioRegion",
            ListKind::Tempo => "tempo",
            ListKind::Meter => "meter",
            ListKind::Marker => "marker",
            ListKind::Automation => "automation",
        }
    }

    fn named(name: &str) -> Option<ListKind> {
        ListKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

impl From<EventKindRow> for ListKind {
    fn from(k: EventKindRow) -> Self {
        match k {
            EventKindRow::Note => ListKind::Note,
            EventKindRow::Cc => ListKind::Cc,
            EventKindRow::PitchBend => ListKind::PitchBend,
            EventKindRow::ChannelPressure => ListKind::ChannelPressure,
            EventKindRow::PolyPressure => ListKind::PolyPressure,
            EventKindRow::Program => ListKind::Program,
            EventKindRow::SysEx => ListKind::SysEx,
            EventKindRow::Text => ListKind::Text,
            EventKindRow::Reference => ListKind::Reference,
            EventKindRow::AudioRegion => ListKind::AudioRegion,
            EventKindRow::Automation => ListKind::Automation,
        }
    }
}

/// What a list hides. Encoded with Swift's keys; hidden kinds this app
/// does not know are kept, so a save writes them back.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(from = "FilterWire", into = "FilterWire")]
pub struct ListFilter {
    pub hidden_kinds: BTreeSet<ListKind>,
    /// Destinations shown (`None`: all).
    pub shown_dests: Option<BTreeSet<u64>>,
    /// Hidden kind names this app does not know.
    other_kinds: BTreeSet<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilterWire {
    #[serde(default)]
    hidden_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shown_dests: Option<BTreeSet<u64>>,
}

impl From<FilterWire> for ListFilter {
    fn from(w: FilterWire) -> Self {
        let mut f = ListFilter {
            shown_dests: w.shown_dests,
            ..ListFilter::default()
        };
        for name in w.hidden_kinds {
            match ListKind::named(&name) {
                Some(k) => {
                    f.hidden_kinds.insert(k);
                }
                None => {
                    f.other_kinds.insert(name);
                }
            }
        }
        f
    }
}

impl From<ListFilter> for FilterWire {
    fn from(f: ListFilter) -> Self {
        let mut names: Vec<String> = f
            .hidden_kinds
            .iter()
            .map(|k| k.name().to_owned())
            .chain(f.other_kinds)
            .collect();
        names.sort();
        names.dedup();
        FilterWire {
            hidden_kinds: names,
            shown_dests: f.shown_dests,
        }
    }
}

impl ListFilter {
    pub fn new(
        hidden_kinds: impl IntoIterator<Item = ListKind>,
        shown_dests: Option<BTreeSet<u64>>,
    ) -> Self {
        ListFilter {
            hidden_kinds: hidden_kinds.into_iter().collect(),
            shown_dests,
            other_kinds: BTreeSet::new(),
        }
    }

    /// Whether a row of `kind` routed to `dest` (its own, else its
    /// track's) shows.
    pub fn shows(&self, kind: ListKind, dest: Option<u64>) -> bool {
        if self.hidden_kinds.contains(&kind) {
            return false;
        }
        match &self.shown_dests {
            Some(shown) if kind.is_midi() => dest.is_some_and(|d| shown.contains(&d)),
            _ => true,
        }
    }

    /// The hidden event kinds as the core's view filter.
    pub fn view_filter(&self) -> Option<ViewFilterRow> {
        let kinds: Vec<EventKindRow> = ListKind::ALL
            .into_iter()
            .filter(|k| self.hidden_kinds.contains(k))
            .filter_map(ListKind::event_kind)
            .collect();
        (!kinds.is_empty()).then_some(ViewFilterRow {
            hidden_kinds: kinds,
            hidden_lanes: vec![],
        })
    }

    /// What "Save as Default" keeps: the hidden kinds only (destinations
    /// belong to one project).
    pub fn kinds_only(&self) -> ListFilter {
        ListFilter {
            shown_dests: None,
            ..self.clone()
        }
    }
}

/// List columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListColumn {
    Position,
    End,
    Duration,
    Kind,
    Lane,
    Dest,
    Data1,
    Data2,
    Release,
    Smpte,
    Mute,
}

impl ListColumn {
    pub const ALL: [ListColumn; 11] = [
        ListColumn::Position,
        ListColumn::End,
        ListColumn::Duration,
        ListColumn::Kind,
        ListColumn::Lane,
        ListColumn::Dest,
        ListColumn::Data1,
        ListColumn::Data2,
        ListColumn::Release,
        ListColumn::Smpte,
        ListColumn::Mute,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ListColumn::Position => "Position",
            ListColumn::End => "End",
            ListColumn::Duration => "Length",
            ListColumn::Kind => "Kind",
            ListColumn::Lane => "Track",
            ListColumn::Dest => "Destination",
            ListColumn::Data1 => "Data 1",
            ListColumn::Data2 => "Data 2",
            ListColumn::Release => "Release",
            ListColumn::Smpte => "SMPTE",
            ListColumn::Mute => "Mute",
        }
    }
}

/// The optional columns shown (an app preference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListColumns {
    pub end: bool,
    pub duration: bool,
    pub release: bool,
    pub smpte: bool,
    /// SMPTE frames per second: 24, 25 or 30.
    pub fps: u32,
}

impl Default for ListColumns {
    fn default() -> Self {
        ListColumns {
            end: false,
            duration: true,
            release: false,
            smpte: false,
            fps: 30,
        }
    }
}

impl ListColumns {
    /// The columns a list over `lanes` tracks shows (Track only for more
    /// than one).
    pub fn visible(&self, lanes: usize) -> Vec<ListColumn> {
        let mut c = vec![ListColumn::Position];
        if self.end {
            c.push(ListColumn::End);
        }
        if self.duration {
            c.push(ListColumn::Duration);
        }
        c.push(ListColumn::Kind);
        if lanes > 1 {
            c.push(ListColumn::Lane);
        }
        c.extend([ListColumn::Dest, ListColumn::Data1, ListColumn::Data2]);
        if self.release {
            c.push(ListColumn::Release);
        }
        if self.smpte {
            c.push(ListColumn::Smpte);
        }
        c.push(ListColumn::Mute);
        c
    }
}

/// Where a row comes from.
#[allow(clippy::large_enum_variant)] // most rows are events; boxing would allocate per row
#[derive(Debug, Clone, PartialEq)]
pub enum ListSource {
    Event { lane: LaneRef, row: EventRow },
    Tempo { index: usize, point: TempoPointRow },
    Meter { index: usize, entry: MeterEntryRow },
    Marker { index: usize, marker: MarkerRow },
}

/// One row of a list editor.
#[derive(Debug, Clone, PartialEq)]
pub struct ListRow {
    id: String,
    pub source: ListSource,
}

impl ListRow {
    pub fn new(source: ListSource) -> Self {
        let id = match &source {
            ListSource::Event { row, .. } => format!("e{}", row.id),
            ListSource::Tempo { index, .. } => format!("t{index}"),
            ListSource::Meter { index, .. } => format!("m{index}"),
            ListSource::Marker { index, .. } => format!("k{index}"),
        };
        ListRow { id, source }
    }

    pub fn event_row(lane: LaneRef, row: EventRow) -> Self {
        Self::new(ListSource::Event { lane, row })
    }

    /// Events keep their ID across edits (`e<id>`); map rows are numbered
    /// (`t<i>`, `m<i>`, `k<i>`).
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn tick(&self) -> i64 {
        match &self.source {
            ListSource::Event { row, .. } => row.tick,
            ListSource::Tempo { point, .. } => point.tick,
            ListSource::Meter { entry, .. } => entry.tick,
            ListSource::Marker { marker, .. } => marker.tick,
        }
    }

    pub fn kind(&self) -> ListKind {
        match &self.source {
            ListSource::Event { row, .. } => row.kind.into(),
            ListSource::Tempo { .. } => ListKind::Tempo,
            ListSource::Meter { .. } => ListKind::Meter,
            ListSource::Marker { .. } => ListKind::Marker,
        }
    }

    pub fn event(&self) -> Option<&EventRow> {
        match &self.source {
            ListSource::Event { row, .. } => Some(row),
            _ => None,
        }
    }

    pub fn lane(&self) -> Option<LaneRef> {
        match &self.source {
            ListSource::Event { lane, .. } => Some(*lane),
            _ => None,
        }
    }
}

/// A track's events for a list.
#[derive(Debug, Clone)]
pub struct ListLane {
    pub key: LaneRef,
    pub events: Arc<[EventRow]>,
    /// The track's MIDI destination.
    pub dest: Option<u64>,
}

/// What a list shows.
#[derive(Debug, Clone, Default)]
pub struct ListInput {
    pub lanes: Vec<ListLane>,
    /// A block's range (events starting inside it).
    pub range: Option<Range<i64>>,
    /// The sequence's maps: tempo, meter and marker rows (sequence
    /// targets).
    pub info: Option<SequenceInfo>,
}

/// Rows by tick, then tempo, meter, marker, events, then track order,
/// then ID.
pub fn build_rows(input: &ListInput, filter: &ListFilter) -> Vec<ListRow> {
    let in_range = |t: i64| input.range.as_ref().is_none_or(|r| r.contains(&t));
    let mut keyed: Vec<(i64, u8, usize, u64, ListRow)> = vec![];
    if let Some(info) = &input.info {
        if filter.shows(ListKind::Tempo, None) {
            for (i, p) in info
                .tempo
                .iter()
                .enumerate()
                .filter(|(_, p)| in_range(p.tick))
            {
                let row = ListRow::new(ListSource::Tempo {
                    index: i,
                    point: *p,
                });
                keyed.push((p.tick, 0, 0, i as u64, row));
            }
        }
        if filter.shows(ListKind::Meter, None) {
            for (i, m) in info
                .meter
                .iter()
                .enumerate()
                .filter(|(_, m)| in_range(m.tick))
            {
                let row = ListRow::new(ListSource::Meter {
                    index: i,
                    entry: *m,
                });
                keyed.push((m.tick, 1, 0, i as u64, row));
            }
        }
        if filter.shows(ListKind::Marker, None) {
            for (i, m) in info
                .markers
                .iter()
                .enumerate()
                .filter(|(_, m)| in_range(m.tick))
            {
                let row = ListRow::new(ListSource::Marker {
                    index: i,
                    marker: m.clone(),
                });
                keyed.push((m.tick, 2, 0, i as u64, row));
            }
        }
    }
    for (li, lane) in input.lanes.iter().enumerate() {
        for e in lane.events.iter().filter(|e| in_range(e.tick)) {
            if !filter.shows(e.kind.into(), e.dest.or(lane.dest)) {
                continue;
            }
            keyed.push((e.tick, 3, li, e.id, ListRow::event_row(lane.key, e.clone())));
        }
    }
    keyed.sort_by_key(|k| (k.0, k.1, k.2, k.3));
    keyed.into_iter().map(|k| k.4).collect()
}

/// The tick at a time in seconds (typed SMPTE).
pub type TickAtSeconds = Arc<dyn Fn(f64) -> Option<i64> + Send + Sync>;

/// What cell text needs besides the row.
#[derive(Clone)]
pub struct ListContext {
    pub seq: u64,
    pub meter: Vec<MeterEntryRow>,
    pub info: Option<SequenceInfo>,
    pub lane_names: HashMap<LaneRef, String>,
    pub dest_names: BTreeMap<u64, String>,
    pub seq_names: HashMap<u64, String>,
    /// Seconds by row ID (the SMPTE column).
    pub seconds: Arc<HashMap<String, f64>>,
    pub fps: u32,
    pub tick_at_seconds: TickAtSeconds,
}

impl fmt::Debug for ListContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListContext")
            .field("seq", &self.seq)
            .field("fps", &self.fps)
            .finish_non_exhaustive()
    }
}

impl ListContext {
    pub fn new(seq: u64) -> Self {
        ListContext {
            seq,
            meter: vec![],
            info: None,
            lane_names: HashMap::new(),
            dest_names: BTreeMap::new(),
            seq_names: HashMap::new(),
            seconds: Arc::default(),
            fps: 30,
            tick_at_seconds: Arc::new(|_| None),
        }
    }

    /// The beat length at `tick`.
    pub fn beat(&self, tick: i64) -> i64 {
        beat_ticks(&bar_at(tick, &self.meter).meter)
    }
}

/// Cell text, editability and the row summary.
pub struct ListText;

impl ListText {
    pub(crate) const TEXT_KINDS: [TextKindRow; 5] = [
        TextKindRow::Text,
        TextKindRow::Lyric,
        TextKindRow::Cue,
        TextKindRow::Marker,
        TextKindRow::Copyright,
    ];

    pub(crate) fn text_title(k: Option<TextKindRow>) -> &'static str {
        match k {
            Some(TextKindRow::Text) | None => "Text",
            Some(TextKindRow::Lyric) => "Lyric",
            Some(TextKindRow::Cue) => "Cue",
            Some(TextKindRow::Marker) => "Marker Text",
            Some(TextKindRow::Copyright) => "Copyright",
        }
    }

    fn kind_title(r: &ListRow) -> &'static str {
        match r.event() {
            Some(e) if e.kind == EventKindRow::Text => Self::text_title(e.text_kind),
            _ => r.kind().title(),
        }
    }

    /// A note's length, a reference's span or a region's length.
    fn span(r: &ListRow) -> Option<i64> {
        let e = r.event()?;
        matches!(
            e.kind,
            EventKindRow::Note | EventKindRow::Reference | EventKindRow::AudioRegion
        )
        .then_some(e.duration)
    }

    pub fn cell(r: &ListRow, col: ListColumn, c: &ListContext) -> String {
        match col {
            ListColumn::Position => Bbu::of(r.tick(), &c.meter).text(),
            ListColumn::End => match Self::span(r) {
                Some(len) => Bbu::of(r.tick().saturating_add(len), &c.meter).text(),
                None => String::new(),
            },
            ListColumn::Duration => match Self::span(r) {
                Some(len) => DurationText::text(len, c.beat(r.tick())),
                None => String::new(),
            },
            ListColumn::Kind => Self::kind_title(r).to_owned(),
            ListColumn::Lane => r
                .lane()
                .and_then(|l| c.lane_names.get(&l).cloned())
                .unwrap_or_default(),
            ListColumn::Dest => match r.event() {
                Some(e) if r.kind().is_midi() => match e.dest {
                    Some(d) => c
                        .dest_names
                        .get(&d)
                        .cloned()
                        .unwrap_or_else(|| format!("Destination {d}")),
                    None => "Track".into(),
                },
                _ => String::new(),
            },
            ListColumn::Data1 => Self::data1(r, c),
            ListColumn::Data2 => Self::data2(r),
            ListColumn::Release => match r.event() {
                Some(e) if e.kind == EventKindRow::Note => {
                    Velocity7::of(e.release_velocity).to_string()
                }
                _ => String::new(),
            },
            ListColumn::Smpte => c
                .seconds
                .get(r.id())
                .map(|s| Smpte::text(*s, c.fps))
                .unwrap_or_default(),
            ListColumn::Mute => {
                if r.event().is_some_and(|e| e.mute) {
                    "M".into()
                } else {
                    String::new()
                }
            }
        }
    }

    fn data1(r: &ListRow, c: &ListContext) -> String {
        match &r.source {
            ListSource::Tempo { point, .. } => format!("{:.2}", point.bpm),
            ListSource::Meter { entry, .. } => format!("{}/{}", entry.num, entry.den),
            ListSource::Marker { .. } => String::new(),
            ListSource::Event { row: e, .. } => match e.kind {
                EventKindRow::Note | EventKindRow::PolyPressure => PitchName::name(e.pitch),
                EventKindRow::Cc => e.value2.to_string(),
                EventKindRow::Program => e.value.to_string(),
                EventKindRow::SysEx => format!("{} bytes", e.data.len()),
                EventKindRow::Text => Self::text_title(e.text_kind).to_owned(),
                EventKindRow::Reference => e
                    .ref_seq
                    .and_then(|s| c.seq_names.get(&s).cloned())
                    .unwrap_or_else(|| "Sequence".into()),
                EventKindRow::AudioRegion => "Audio".into(),
                // Automation names come with the automation editors.
                EventKindRow::Automation
                | EventKindRow::PitchBend
                | EventKindRow::ChannelPressure => String::new(),
            },
        }
    }

    fn data2(r: &ListRow) -> String {
        match &r.source {
            ListSource::Tempo { point, .. } => if point.ramp { "ramp" } else { "jump" }.into(),
            ListSource::Meter { entry, .. } => KeyName::name(entry.sharps, entry.minor),
            ListSource::Marker { marker, .. } => marker.name.clone(),
            ListSource::Event { row: e, .. } => match e.kind {
                EventKindRow::Note => Velocity7::of(e.velocity).to_string(),
                EventKindRow::Cc | EventKindRow::ChannelPressure | EventKindRow::PolyPressure => {
                    Value7::of(e.value).to_string()
                }
                EventKindRow::PitchBend => Bend14::of(e.value).to_string(),
                EventKindRow::Program if e.value2 < 0 => String::new(),
                EventKindRow::Program => format!("{}:{}", e.value2 / 128, e.value2 % 128),
                EventKindRow::SysEx => {
                    let head: Vec<String> =
                        e.data.iter().take(8).map(|b| format!("{b:02X}")).collect();
                    let head = head.join(" ");
                    if e.data.len() > 8 {
                        head + " \u{2026}"
                    } else {
                        head
                    }
                }
                EventKindRow::Text => e.text.clone().unwrap_or_default(),
                EventKindRow::Reference => {
                    let loops = if e.ref_loops == 0 {
                        "fill".to_owned()
                    } else {
                        format!("\u{d7}{}", e.ref_loops)
                    };
                    let sign = if e.ref_transpose > 0 { "+" } else { "" };
                    format!("{loops} {sign}{}", e.ref_transpose)
                }
                EventKindRow::AudioRegion => format!("{:.1} dB", e.gain_db),
                EventKindRow::Automation => String::new(),
            },
        }
    }

    /// Whether a cell takes typing. References, audio regions and
    /// automation points are shown but not edited here yet.
    pub fn editable(r: &ListRow, col: ListColumn) -> bool {
        match &r.source {
            ListSource::Tempo { index, .. } | ListSource::Meter { index, .. } => {
                col == ListColumn::Data1
                    || col == ListColumn::Data2
                    || (col == ListColumn::Position && *index > 0)
            }
            ListSource::Marker { .. } => col == ListColumn::Position || col == ListColumn::Data2,
            ListSource::Event { row: e, .. } => {
                use EventKindRow as K;
                if matches!(e.kind, K::Reference | K::AudioRegion | K::Automation) {
                    return false;
                }
                match col {
                    ListColumn::Position | ListColumn::Smpte | ListColumn::Mute => true,
                    ListColumn::End | ListColumn::Duration | ListColumn::Release => {
                        e.kind == K::Note
                    }
                    ListColumn::Dest => ListKind::from(e.kind).is_midi(),
                    ListColumn::Kind | ListColumn::Lane => false,
                    ListColumn::Data1 => matches!(
                        e.kind,
                        K::Note | K::PolyPressure | K::Cc | K::Program | K::Text
                    ),
                    ListColumn::Data2 => matches!(
                        e.kind,
                        K::Note
                            | K::Cc
                            | K::PitchBend
                            | K::ChannelPressure
                            | K::PolyPressure
                            | K::Program
                            | K::Text
                    ),
                }
            }
        }
    }

    /// The row as a screen reader reads it ("Note C4, bar 3 beat 1,
    /// velocity 100").
    pub fn summary(r: &ListRow, c: &ListContext) -> String {
        let b = Bbu::of(r.tick(), &c.meter);
        let at = format!("bar {} beat {}", b.bar, b.beat);
        if r.kind() == ListKind::Note {
            return format!(
                "Note {}, {at}, velocity {}",
                Self::data1(r, c),
                Self::data2(r)
            );
        }
        let d: Vec<String> = [Self::data1(r, c), Self::data2(r)]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        let mut out = format!("{}, {at}", Self::kind_title(r));
        if !d.is_empty() {
            out.push_str(", ");
            out.push_str(&d.join(", "));
        }
        out
    }
}
