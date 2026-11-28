//! The detail pane's content (Swift `EditorTypes.swift`): which editor,
//! on what. Encoded as Swift's `Codable` writes them, so `ui.json` reads
//! in both apps.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::strip_chart::{MAX_ROWS, StripLane};

pub use crate::list::ListFilter;

/// The detail editors. `ui.json` spells them camel-case (`pianoRoll`), the
/// control channel snake-case ([`DetailEditorKind::name`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DetailEditorKind {
    #[default]
    PianoRoll,
    List,
    StripChart,
    DrumGrid,
}

impl DetailEditorKind {
    pub const ALL: [DetailEditorKind; 4] = [
        DetailEditorKind::PianoRoll,
        DetailEditorKind::List,
        DetailEditorKind::StripChart,
        DetailEditorKind::DrumGrid,
    ];

    /// This app has the editor (the Drum Grid comes in a later slice).
    pub fn is_available(self) -> bool {
        matches!(
            self,
            DetailEditorKind::PianoRoll | DetailEditorKind::List | DetailEditorKind::StripChart
        )
    }

    pub fn title(self) -> &'static str {
        match self {
            DetailEditorKind::PianoRoll => "Piano Roll",
            DetailEditorKind::List => "List",
            DetailEditorKind::StripChart => "Strip Chart",
            DetailEditorKind::DrumGrid => "Drum Grid",
        }
    }

    /// The control channel's name (`wait detail_kind`, `state.get editor`).
    pub fn name(self) -> &'static str {
        match self {
            DetailEditorKind::PianoRoll => "piano_roll",
            DetailEditorKind::List => "list",
            DetailEditorKind::StripChart => "strip_chart",
            DetailEditorKind::DrumGrid => "drum_grid",
        }
    }

    /// The digit of its ⌘ shortcut.
    pub fn key(self) -> &'static str {
        match self {
            DetailEditorKind::PianoRoll => "1",
            DetailEditorKind::List => "2",
            DetailEditorKind::StripChart => "3",
            DetailEditorKind::DrumGrid => "4",
        }
    }
}

/// What a detail editor shows. Swift writes `{"block":{"seq":…}}`,
/// `{"lane":{"seq":…,"lane":…}}` and `{"sequence":{"_0":…}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorTarget {
    Block {
        seq: u64,
        lane: u64,
        start: i64,
        end: i64,
    },
    Lane {
        seq: u64,
        lane: u64,
    },
    Sequence {
        #[serde(rename = "_0")]
        seq: u64,
    },
}

impl EditorTarget {
    pub fn seq(&self) -> u64 {
        match *self {
            EditorTarget::Block { seq, .. }
            | EditorTarget::Lane { seq, .. }
            | EditorTarget::Sequence { seq } => seq,
        }
    }

    pub fn lane(&self) -> Option<u64> {
        match *self {
            EditorTarget::Block { lane, .. } | EditorTarget::Lane { lane, .. } => Some(lane),
            EditorTarget::Sequence { .. } => None,
        }
    }
}

/// A detail editor and its target. Fields this app does not model (the
/// piano roll's `lanes`, the Strip Chart's `stripRows`, …) ride along in
/// `extra`, so a save keeps them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailState {
    pub kind: DetailEditorKind,
    pub target: EditorTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_filter: Option<ListFilter>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl DetailState {
    pub fn new(kind: DetailEditorKind, target: EditorTarget) -> Self {
        DetailState {
            kind,
            target,
            list_filter: None,
            extra: Map::new(),
        }
    }

    /// The piano roll's shown tracks (`lanes`): besides a track target's
    /// own, or exactly those of a sequence target; `None` (also when
    /// malformed) is the default set.
    pub fn lanes(&self) -> Option<Vec<u64>> {
        Vec::<u64>::deserialize(self.extra.get("lanes")?).ok()
    }

    /// Sets the shown tracks; `None` leaves the key out (as Swift omits a
    /// nil optional).
    pub fn set_lanes(&mut self, lanes: Option<Vec<u64>>) {
        match lanes {
            Some(l) => {
                self.extra.insert("lanes".into(), Value::from(l));
            }
            None => {
                self.extra.remove("lanes");
            }
        }
    }

    /// The Strip Chart's rows (`stripRows`); `None` when missing or
    /// malformed: an unknown entry, no rows, or more than `MAX_ROWS + 1`
    /// (the saved rows plus an opening target's row,
    /// `M/StripChartModel.swift:180-190`).
    pub fn strip_rows(&self) -> Option<Vec<StripLane>> {
        Vec::<StripLane>::deserialize(self.extra.get("stripRows")?)
            .ok()
            .filter(|r| (1..=MAX_ROWS + 1).contains(&r.len()))
    }

    /// Sets the Strip Chart's rows; `None` leaves the key out.
    pub fn set_strip_rows(&mut self, rows: Option<Vec<StripLane>>) {
        match rows.and_then(|r| serde_json::to_value(r).ok()) {
            Some(v) => {
                self.extra.insert("stripRows".into(), v);
            }
            None => {
                self.extra.remove("stripRows");
            }
        }
    }

    /// The track the rows were saved for (`stripRowsLane`, Swift's
    /// `LaneKey` `{"seq":…,"lane":…}`); `None` when missing or malformed.
    pub fn strip_rows_lane(&self) -> Option<(u64, u64)> {
        let k = RowsLane::deserialize(self.extra.get("stripRowsLane")?).ok()?;
        Some((k.seq, k.lane))
    }

    /// Sets the rows' track; `None` leaves the key out.
    pub fn set_strip_rows_lane(&mut self, lane: Option<(u64, u64)>) {
        match lane {
            Some((seq, lane)) => {
                self.extra
                    .insert("stripRowsLane".into(), json!({"seq": seq, "lane": lane}));
            }
            None => {
                self.extra.remove("stripRowsLane");
            }
        }
    }
}

/// Swift's `LaneKey` JSON.
#[derive(Deserialize)]
struct RowsLane {
    seq: u64,
    lane: u64,
}
