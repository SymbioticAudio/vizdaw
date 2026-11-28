//! What the bottom pane's Drum Grid shows, per document
//! (`M/DrumGridModel.swift:5-180,373-399`, `M/DocumentModel.swift:393-428`):
//! the drum tracks of the target's sequence, read once per snapshot; their
//! layout; the selected row; and opening a track, which gives a MIDI or
//! Instrument track its default drum layout (one undo step). There is no
//! window slot: torn-off editors come later (Ruling 2).

use std::sync::{Arc, Mutex, MutexGuard};

use vizdaw_ffi::VizError;
use vizdaw_ffi::drum::{
    DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow, DrumLaneRow, DrumRowRow, DrumSpotRow,
    drum_grid_layout, drum_grid_spot,
};
use vizdaw_ffi::edit::{EditCommand, LaneRef};
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::tracks::TrackKindRow;

use super::DrumGridSettings;
use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::editor::{DetailEditorKind, DetailState, EditorTarget};

/// A drum row by track and row ID (`M/DrumGridModel.swift:5-14`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DrumRowKey {
    pub lane: u64,
    pub row: u64,
}

/// The drum tracks read at a snapshot version.
struct LanesCache {
    seq: u64,
    version: u64,
    lanes: Arc<[DrumLaneRow]>,
}

/// A layout and what it was computed from.
struct LayoutCache {
    shapes: Vec<Vec<bool>>,
    settings: DrumGridSettingsRow,
    layout: Arc<DrumGridLayoutRow>,
}

/// The pane's Drum Grid; not saved (the bar's settings are the layout's,
/// `EditorLayout.drum_grid`).
#[derive(Default)]
pub struct DrumGridModel {
    /// The row the row commands act on.
    selected: Option<DrumRowKey>,
    lanes: Mutex<Option<LanesCache>>,
    /// A read that failed at `(seq, version)`: not retried there.
    failure: Mutex<Option<(u64, u64)>>,
    /// The failure's message, for the status line on the next idle
    /// (Ruling 11: reads are `&self`).
    message: Mutex<Option<String>>,
    layout: Mutex<Option<LayoutCache>>,
    /// The stroke in progress (`stroke.rs`).
    pub(super) stroke: Option<super::stroke::DrumStroke>,
    /// The extend strokes call (`None`: the core's).
    pub(super) extend: Option<super::stroke::DrumExtend>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the Drum Grid can edit `row`: a MIDI or Instrument track
/// (`M/DrumGridModel.swift:382`).
pub fn drum_grid_offers(row: &LaneRow) -> bool {
    row.kind != TrackKindRow::Audio
}

impl DocumentModel {
    /// The pane's detail shows the Drum Grid.
    pub fn drum_showing(&self) -> bool {
        self.layout()
            .detail
            .as_ref()
            .is_some_and(|d| d.kind == DetailEditorKind::DrumGrid)
    }

    fn drum_target(&self) -> Option<EditorTarget> {
        self.layout()
            .detail
            .as_ref()
            .filter(|d| d.kind == DetailEditorKind::DrumGrid)
            .map(|d| d.target)
    }

    /// The shown Drum Grid's sequence.
    pub fn drum_seq(&self) -> Option<u64> {
        self.drum_target().map(|t| t.seq())
    }

    /// A block target's range (at least one tick), scrolled to when shown.
    pub fn drum_focus_range(&self) -> Option<TickRange> {
        match self.drum_target()? {
            EditorTarget::Block { start, end, .. } => Some(TickRange {
                start,
                end: end.max(start + 1),
            }),
            _ => None,
        }
    }

    /// The drum tracks of the shown sequence, read once per snapshot
    /// (`M/DrumGridModel.swift:88-108`). Busy keeps what was read before
    /// for the same sequence (and reads again next time); another failure
    /// reads as none, is not retried at that version, forgets the rows
    /// read before it and shows once in the status line on the next idle.
    pub fn drum_lanes(&self) -> Arc<[DrumLaneRow]> {
        let Some(seq) = self.drum_seq() else {
            return Arc::from([]);
        };
        let version = self.snapshot().version;
        let g = &self.drum_grid;
        let mut cache = lock(&g.lanes);
        if let Some(c) = cache.as_ref()
            && c.seq == seq
            && c.version == version
        {
            return c.lanes.clone();
        }
        if *lock(&g.failure) == Some((seq, version)) {
            return Arc::from([]);
        }
        match self.session.drum_lanes(seq) {
            Ok(rows) => {
                let lanes: Arc<[DrumLaneRow]> = rows.into();
                *cache = Some(LanesCache {
                    seq,
                    version,
                    lanes: lanes.clone(),
                });
                lanes
            }
            Err(VizError::Busy { .. }) => match cache.as_ref() {
                Some(c) if c.seq == seq => c.lanes.clone(),
                _ => Arc::from([]),
            },
            Err(e) => {
                *lock(&g.failure) = Some((seq, version));
                // A later Busy read must not bring back rows from before
                // the failure.
                *cache = None;
                *lock(&g.message) = Some(error_message(&e));
                Arc::from([])
            }
        }
    }

    /// A failed drum tracks read's message becomes the status, once
    /// (Ruling 11). Whether there was one.
    pub(crate) fn take_drum_failure(&mut self) -> bool {
        let Some(message) = lock(&self.drum_grid.message).take() else {
            return false;
        };
        self.status = Some(message);
        true
    }

    /// The bands of the shown tracks (the core's layout), computed once
    /// per row shape and settings (`M/DrumGridModel.swift:134-141`). An
    /// error is an empty layout.
    pub fn drum_layout(&self) -> Arc<DrumGridLayoutRow> {
        let shapes: Vec<Vec<bool>> = self
            .drum_lanes()
            .iter()
            .map(|l| l.rows.iter().map(|r| r.show_velocity).collect())
            .collect();
        let settings = self.drum_settings().row();
        let mut cache = lock(&self.drum_grid.layout);
        if let Some(c) = cache.as_ref()
            && c.shapes == shapes
            && c.settings == settings
        {
            return c.layout.clone();
        }
        let layout = Arc::new(drum_grid_layout(shapes.clone(), settings).unwrap_or(
            DrumGridLayoutRow {
                bands: vec![],
                height: 0.,
            },
        ));
        *cache = Some(LayoutCache {
            shapes,
            settings,
            layout: layout.clone(),
        });
        layout
    }

    /// What height `y` (content coordinates) is over; an error is nothing.
    pub fn drum_spot(&self, y: f64) -> DrumSpotRow {
        drum_grid_spot((*self.drum_layout()).clone(), y).unwrap_or(DrumSpotRow::Nothing)
    }

    /// Row `row` of shown track `lane`, while both exist.
    pub fn drum_row(&self, lane: usize, row: usize) -> Option<DrumRowRow> {
        self.drum_lanes().get(lane)?.rows.get(row).cloned()
    }

    /// The selected row's key (it may no longer exist).
    pub fn drum_selected_row(&self) -> Option<DrumRowKey> {
        self.drum_grid.selected
    }

    /// The selected row and its track, while the row still exists.
    pub fn drum_selected(&self) -> Option<(DrumLaneRow, DrumRowRow)> {
        let key = self.drum_grid.selected?;
        let lanes = self.drum_lanes();
        let l = lanes.iter().find(|l| l.lane == key.lane)?;
        let r = l.rows.iter().find(|r| r.id == key.row)?;
        Some((l.clone(), r.clone()))
    }

    /// Selects row `row` of shown track `lane`; indexes that do not exist
    /// do nothing.
    pub fn drum_select(&mut self, lane: usize, row: usize) {
        let lanes = self.drum_lanes();
        if let Some(l) = lanes.get(lane)
            && let Some(r) = l.rows.get(row)
        {
            self.drum_grid.selected = Some(DrumRowKey {
                lane: l.lane,
                row: r.id,
            });
        }
    }

    /// The track the track controls act on: the selected row's, else the
    /// first.
    pub fn drum_current_lane(&self) -> Option<DrumLaneRow> {
        self.drum_selected()
            .map(|(l, _)| l)
            .or_else(|| self.drum_lanes().first().cloned())
    }

    pub fn drum_settings(&self) -> &DrumGridSettings {
        &self.layout().drum_grid
    }

    /// Changes the bar's settings; the next save writes them.
    pub fn update_drum_settings(&mut self, change: impl FnOnce(&mut DrumGridSettings)) {
        self.update_layout(|l| change(&mut l.drum_grid));
    }

    /// A ruler drag: the time range over the shown drum tracks, no events
    /// (`E/DrumGridView.swift:55-58`).
    pub fn drum_select_range(&mut self, range: TickRange) {
        let Some(seq) = self.drum_seq() else { return };
        let lanes = self
            .drum_lanes()
            .iter()
            .map(|l| LaneRef { seq, lane: l.lane })
            .collect();
        self.set_selection(SelectionRow {
            time_range: Some(range),
            lanes,
            events: vec![],
        });
    }

    /// The shown sequence's tracks the grid can open (MIDI and
    /// Instrument), for the empty state (`M/DrumGridModel.swift:112-115`).
    pub fn drum_tracks(&self) -> Vec<LaneRow> {
        let Some(seq) = self.drum_seq() else {
            return vec![];
        };
        self.lane_rows(seq)
            .iter()
            .filter(|l| drum_grid_offers(l))
            .cloned()
            .collect()
    }

    /// Opens track `lane` of the shown sequence in the grid; it gets a
    /// default drum layout if it has none (`M/DrumGridModel.swift:118-125`).
    pub fn drum_open_lane(&mut self, lane: u64) {
        let Some(seq) = self.drum_seq() else { return };
        self.open_detail(
            EditorTarget::Lane { seq, lane },
            Some(DetailEditorKind::DrumGrid),
        );
    }

    /// Makes track `lane` of `seq` a drum track (rows for its notes) or a
    /// normal track again, as one undo step (`M/DrumGridModel.swift:377-379`).
    pub fn mark_drum_lane(&mut self, seq: u64, lane: u64, on: bool) -> bool {
        self.edit(EditCommand::Drum {
            edit: DrumEdit::MarkLane {
                seq,
                lane,
                drum: on,
            },
        })
    }

    /// Opening `target` in the Drum Grid: a MIDI or Instrument track with
    /// no drum layout gets one (`M/DrumGridModel.swift:393-399`). Audio
    /// tracks, tracks with a layout and sequence targets are left alone.
    /// Only this version's tracks decide: a busy core's older rows could
    /// say a track already marked is not, and mark it again.
    pub(crate) fn prepare_drum_grid(&mut self, target: EditorTarget) {
        let Some(lane) = target.lane() else { return };
        let seq = target.seq();
        let read = self.lane_rows_read(seq);
        if !read.fresh {
            return;
        }
        let Some(row) = read.rows.iter().find(|l| l.id == lane) else {
            return;
        };
        if drum_grid_offers(row) && !row.is_drum {
            self.mark_drum_lane(seq, lane, true);
        }
    }

    /// The pane's detail changed from `before`: a pane that no longer
    /// shows the Drum Grid forgets its selected row
    /// (`M/DocumentModel.swift:418-420`). A change of kind or target, or
    /// closing it, ends the stroke.
    pub(crate) fn drum_detail_changed(&mut self, before: Option<&DetailState>) {
        self.drum_stroke_detail_changed(before);
        if !self.drum_showing() {
            self.drum_grid.selected = None;
        }
    }
}
