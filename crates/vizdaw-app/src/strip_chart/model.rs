//! What each Strip Chart shows, per document (`M/StripChartModel.swift:
//! 62-330`): the track and rows of the bottom pane's chart and of the
//! right pane's Controllers chart, their values and the write in progress,
//! and the one automation target cache they share (Ruling 4).

use std::sync::{Arc, Mutex, MutexGuard};

use vizdaw_ffi::events::{AutomationTargetRow, EventKindRow, EventRow, MeterEntryRow};

use super::{
    AutomationTargets, MAX_ROWS, StripChartSettings, StripLane, StripMode, StripPoint, StripValue,
    v32,
};
use crate::document::DocumentModel;
use crate::editor::{DetailEditorKind, DetailState, EditorTarget, controllers_key};

/// Which chart: the bottom pane's, or the right pane's Controllers (no
/// window slot: torn-off editors come later, Ruling 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartSlot {
    Pane,
    Controllers,
}

/// A Strip Chart drag in progress, in track space
/// (`M/StripChartModel.swift:62-78`).
#[derive(Debug, Clone, PartialEq)]
pub struct StripStroke {
    pub row: usize,
    pub lane: StripLane,
    pub start: StripPoint,
    pub current: StripPoint,
    /// Every pointer point so far (a free line's).
    pub points: Vec<StripPoint>,
    pub mode: StripMode,
    /// A Compress rectangle (Modify with Compress).
    pub compress: bool,
    /// ⌥ (Alt) at the press: the selected events only.
    pub selected_only: bool,
    pub seed: u64,
    /// Its gesture is open.
    pub open: bool,
}

/// One chart's own state; not saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StripChartModel {
    /// For a sequence target: the track chosen.
    pub chosen_lane: Option<(u64, u64)>,
    pub stroke: Option<StripStroke>,
    /// The seed of the next random line (`None`: a fresh one; tests set it).
    pub seed: Option<u64>,
}

/// The automation targets with the snapshot and slots versions they were
/// read at.
struct TargetsCache {
    version: u64,
    slots: u64,
    targets: Arc<AutomationTargets>,
}

/// A document's two charts and the target cache.
#[derive(Default)]
pub(crate) struct StripCharts {
    pane: StripChartModel,
    controllers: StripChartModel,
    targets: Mutex<Option<TargetsCache>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The one target `events` automate, when they are all automation points
/// of it (Ruling 10); `None` for an empty track, notes or more than one
/// target (`M/StripChartModel.swift:254-262`).
pub fn only_target(events: &[EventRow]) -> Option<AutomationTargetRow> {
    let first = events.first()?.automation.as_ref()?.target;
    events
        .iter()
        .all(|e| {
            e.kind == EventKindRow::Automation
                && e.automation.as_ref().map(|a| a.target) == Some(first)
        })
        .then_some(first)
}

/// The rows a track automating only `t` opens on (ruling T6-2): `t`'s row
/// first, then `kept` without it. The row cap drops none of `kept`
/// (`M/StripChartModel.swift:264-271`).
pub fn opening(t: AutomationTargetRow, kept: &[StripLane]) -> Vec<StripLane> {
    let own = StripLane::Automation(t);
    std::iter::once(own)
        .chain(kept.iter().copied().filter(|l| *l != own))
        .collect()
}

impl DocumentModel {
    /// The chart in `slot`.
    pub fn strip_chart(&self, slot: ChartSlot) -> &StripChartModel {
        match slot {
            ChartSlot::Pane => &self.strip_charts.pane,
            ChartSlot::Controllers => &self.strip_charts.controllers,
        }
    }

    /// The chart in `slot`, to change (Task 4's strokes).
    pub fn strip_chart_mut(&mut self, slot: ChartSlot) -> &mut StripChartModel {
        match slot {
            ChartSlot::Pane => &mut self.strip_charts.pane,
            ChartSlot::Controllers => &mut self.strip_charts.controllers,
        }
    }

    /// The document's automation targets, read once per snapshot version
    /// and slots version (`M/StripChartModel.swift:312-329`). While a
    /// gesture is open the cache is used as it is (each step bumps the
    /// version; the mixer cannot change mid-gesture). Busy keeps the
    /// previous value (or an empty one) and caches nothing, so the next
    /// call retries.
    pub fn automation_targets(&self) -> Arc<AutomationTargets> {
        let version = self.snapshot().version;
        let slots = self.slots_version();
        let mut cache = lock(&self.strip_charts.targets);
        if let Some(c) = cache.as_ref()
            && ((c.version == version && c.slots == slots) || self.gesture().is_some())
        {
            return c.targets.clone();
        }
        match AutomationTargets::load(self.session.as_ref()) {
            Ok(t) => {
                let t = Arc::new(t);
                *cache = Some(TargetsCache {
                    version,
                    slots,
                    targets: t.clone(),
                });
                t
            }
            Err(_) => cache
                .as_ref()
                .map_or_else(Default::default, |c| c.targets.clone()),
        }
    }

    /// What `slot`'s chart shows: the pane's detail; for the Controllers,
    /// the focused track of the shown sequence as a Strip Chart with that
    /// track's saved rows (`M/StripChartModel.swift:118-131`); `None`
    /// with no track focused.
    pub fn strip_state(&self, slot: ChartSlot) -> Option<DetailState> {
        match slot {
            ChartSlot::Pane => self.layout().detail.clone(),
            ChartSlot::Controllers => {
                let seq = self.selected_seq()?;
                let lane = self.focus_lanes().first()?.id;
                let mut d = DetailState::new(
                    DetailEditorKind::StripChart,
                    EditorTarget::Lane { seq, lane },
                );
                // Capped as `stripRows` is (Task 2 review).
                let rows = self
                    .layout()
                    .controllers
                    .get(&controllers_key(seq, lane))
                    .filter(|r| (1..=MAX_ROWS + 1).contains(&r.len()))
                    .cloned();
                d.set_strip_rows(rows);
                d.set_strip_rows_lane(Some((seq, lane)));
                Some(d)
            }
        }
    }

    pub fn strip_target(&self, slot: ChartSlot) -> Option<EditorTarget> {
        self.strip_state(slot).map(|d| d.target)
    }

    /// The track shown: the target's, else the chosen one while it
    /// exists, else the sequence's first (`M/StripChartModel.swift:166-174`).
    pub fn strip_lane(&self, slot: ChartSlot) -> Option<(u64, u64)> {
        let t = self.strip_target(slot)?;
        self.lane_of(slot, &t)
    }

    fn lane_of(&self, slot: ChartSlot, t: &EditorTarget) -> Option<(u64, u64)> {
        let seq = t.seq();
        if let Some(l) = t.lane() {
            return Some((seq, l));
        }
        let rows = self.lane_rows(seq);
        if let Some(c) = self.strip_chart(slot).chosen_lane
            && c.0 == seq
            && rows.iter().any(|r| r.id == c.1)
        {
            return Some(c);
        }
        rows.first().map(|r| (seq, r.id))
    }

    /// For a sequence target: shows `lane`.
    pub fn set_strip_chosen_lane(&mut self, slot: ChartSlot, lane: (u64, u64)) {
        self.strip_chart_mut(slot).chosen_lane = Some(lane);
    }

    /// The rows: the saved ones (else Velocity) without automation rows
    /// whose strip, slot, destination or VCA is gone (Velocity when none
    /// is left). A track holding one target's automation and nothing else
    /// opens on that target's row unless the rows were saved for this
    /// very track (`M/StripChartModel.swift:180-204`).
    pub fn strip_rows(&self, slot: ChartSlot) -> Vec<StripLane> {
        let Some(state) = self.strip_state(slot) else {
            return vec![StripLane::Velocity];
        };
        self.rows_of(slot, &state)
    }

    fn rows_of(&self, slot: ChartSlot, state: &DetailState) -> Vec<StripLane> {
        let saved = state.strip_rows();
        let kept = saved
            .as_deref()
            .map_or_else(|| vec![StripLane::Velocity], |s| self.known(s));
        let lane = self.lane_of(slot, &state.target);
        if saved.is_some() && state.strip_rows_lane() == lane {
            return kept;
        }
        let events = lane.map_or_else(|| Arc::from([]), |(s, l)| self.lane_events(s, l));
        match only_target(&events) {
            Some(t) => opening(t, &kept),
            None => kept,
        }
    }

    /// `saved` without automation rows the mixer no longer has.
    fn known(&self, saved: &[StripLane]) -> Vec<StripLane> {
        if !saved.iter().any(|l| matches!(l, StripLane::Automation(_))) {
            return saved.to_vec();
        }
        let targets = self.automation_targets();
        let kept: Vec<_> = saved
            .iter()
            .copied()
            .filter(|l| match l {
                StripLane::Automation(t) => targets.names.knows(t),
                _ => true,
            })
            .collect();
        if kept.is_empty() {
            vec![StripLane::Velocity]
        } else {
            kept
        }
    }

    pub fn set_strip_row(&mut self, slot: ChartSlot, i: usize, lane: StripLane) {
        self.update_strip_rows(slot, |r| {
            if let Some(x) = r.get_mut(i) {
                *x = lane;
            }
        });
    }

    /// Adds a Controller 7 row while there are fewer than [`MAX_ROWS`].
    pub fn add_strip_row(&mut self, slot: ChartSlot) {
        self.update_strip_rows(slot, |r| {
            if r.len() < MAX_ROWS {
                r.push(StripLane::Controller(7));
            }
        });
    }

    /// Removes row `i`; one row stays.
    pub fn remove_strip_row(&mut self, slot: ChartSlot, i: usize) {
        self.update_strip_rows(slot, |r| {
            if r.len() > 1 && i < r.len() {
                r.remove(i);
            }
        });
    }

    /// Writes the changed rows, with the track shown as theirs: into the
    /// detail for the pane; for the Controllers into `controllers` under
    /// the key of the track the state was read for, never the focus now
    /// (`M/StripChartModel.swift:120-131,224-231`).
    fn update_strip_rows(&mut self, slot: ChartSlot, change: impl FnOnce(&mut Vec<StripLane>)) {
        let Some(mut state) = self.strip_state(slot) else {
            return;
        };
        let mut rows = self.rows_of(slot, &state);
        change(&mut rows);
        let lane = self.lane_of(slot, &state.target);
        state.set_strip_rows(Some(rows.clone()));
        state.set_strip_rows_lane(lane);
        match slot {
            ChartSlot::Pane => self.update_layout(|l| l.detail = Some(state)),
            ChartSlot::Controllers => {
                if let EditorTarget::Lane { seq, lane } = state.target {
                    self.update_layout(|l| {
                        l.controllers.insert(controllers_key(seq, lane), rows);
                    });
                }
            }
        }
    }

    /// The bar's settings, shared by every chart.
    pub fn strip_settings(&self) -> &StripChartSettings {
        &self.layout().strip_chart
    }

    pub fn update_strip_settings(&mut self, change: impl FnOnce(&mut StripChartSettings)) {
        self.update_layout(|l| change(&mut l.strip_chart));
    }

    /// The meter map of the chart's sequence.
    pub fn strip_meter(&self, slot: ChartSlot) -> Vec<MeterEntryRow> {
        self.strip_target(slot)
            .and_then(|t| self.sequence_info(t.seq()))
            .map(|i| i.meter.clone())
            .unwrap_or_default()
    }

    /// The shown track's events.
    pub fn strip_events(&self, slot: ChartSlot) -> Arc<[EventRow]> {
        self.strip_lane(slot)
            .map_or_else(|| Arc::from([]), |(s, l)| self.lane_events(s, l))
    }

    /// `row`'s values on the shown track (the tempo from the sequence).
    pub fn strip_values(&self, slot: ChartSlot, row: StripLane) -> Vec<StripValue> {
        let tempo = self
            .strip_target(slot)
            .and_then(|t| self.sequence_info(t.seq()))
            .map(|i| i.tempo.as_slice())
            .unwrap_or_default();
        row.values(&self.strip_events(slot), tempo)
    }

    /// The write in progress on `row` (Plan 5e-2 Ruling 9): the session's
    /// preview of its target while `playing_seq` is the chart's sequence;
    /// empty for other rows, when stopped, while another sequence plays,
    /// or on an error (`M/StripChartModel.swift:273-288`).
    pub fn strip_preview(
        &self,
        slot: ChartSlot,
        row: StripLane,
        playing_seq: Option<u64>,
    ) -> Vec<StripValue> {
        let StripLane::Automation(t) = row else {
            return vec![];
        };
        let Some(seq) = self.strip_target(slot).map(|t| t.seq()) else {
            return vec![];
        };
        if playing_seq != Some(seq) {
            return vec![];
        }
        self.session
            .automation_preview(t)
            .unwrap_or_default()
            .into_iter()
            .map(|p| StripValue {
                id: None,
                tick: p.tick,
                length: 0,
                value: v32(p.value),
                ramp: true,
            })
            .collect()
    }
}
