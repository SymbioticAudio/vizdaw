//! What each Strip Chart shows, per document (`M/StripChartModel.swift:
//! 62-330`): the track and rows of the bottom pane's chart and of the
//! right pane's Controllers chart, their values and the write in progress,
//! and the one automation target cache they share (Ruling 4).

use std::sync::{Arc, Mutex, MutexGuard};

use vizdaw_ffi::edit::{EditCommand, LaneRef};
use vizdaw_ffi::events::{AutomationTargetRow, EventKindRow, EventRow, MeterEntryRow};
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::strip::LineOpRow;

use super::{
    AutomationTargets, MAX_ROWS, StripChartSettings, StripLane, StripMode, StripOp, StripPoint,
    StripValue, compress_command, erase_command, line_command, stroke_label, stroke_line,
    stroke_op, v32,
};
use crate::document::DocumentModel;
use crate::editor::{DetailEditorKind, DetailState, EditorTarget, GESTURE_OPEN, controllers_key};
use crate::select_modify::GestureOrigin;

/// An Erase stroke on a row with no series to erase
/// (`M/StripChartModel.swift:104`).
pub const ERASE_NOTES: &str = "Erase works on controller, program and automation rows.";

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
    /// The serial of the gesture it opened: a stroke whose gesture went
    /// (its track removed) never touches another one.
    pub serial: Option<u64>,
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

    /// `saved` without automation rows the mixer no longer has. Targets
    /// never read (a Busy first read) have no strips, and such names know
    /// every target (`StripNames::knows`): the rows are unknown yet, not
    /// gone, so an edit then keeps them (fix round 1).
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

/// A fresh random-line seed: a counter through splitmix64, xored with the
/// clock, so two strokes always differ (as `next_seed` of Select and
/// Modify).
fn fresh_seed() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut z = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    z ^ (now & 0xFFFF_0000_0000_0000)
}

// MARK: Strokes

impl DocumentModel {
    /// The stroke in progress in `slot`'s chart.
    pub fn strip_stroke(&self, slot: ChartSlot) -> Option<&StripStroke> {
        self.strip_chart(slot).stroke.as_ref()
    }

    /// Every later stroke in `slot` seeds its random line with `seed`.
    pub fn set_strip_seed_for_test(&mut self, slot: ChartSlot, seed: u64) {
        self.strip_chart_mut(slot).seed = Some(seed);
    }

    /// `slot`'s chart is on screen: the pane's while its detail is a
    /// Strip Chart; the Controllers' while it has a track.
    fn strip_showing(&self, slot: ChartSlot) -> bool {
        match slot {
            ChartSlot::Pane => self
                .layout()
                .detail
                .as_ref()
                .is_some_and(|d| d.kind == DetailEditorKind::StripChart),
            ChartSlot::Controllers => self.strip_state(slot).is_some(),
        }
    }

    /// The open gesture is `s`'s own.
    fn owns(&self, s: &StripStroke) -> bool {
        s.open && s.serial.is_some() && self.gesture_serial() == s.serial
    }

    /// The pointer went down on row `row` at `p` (`selected_only`: ⌥, Alt
    /// off macOS). A stroke still held (its release was lost) is
    /// cancelled first; nothing opens before the pointer moves
    /// (`M/StripChartModel.swift:333-342`).
    pub fn strip_stroke_began(
        &mut self,
        slot: ChartSlot,
        row: usize,
        p: StripPoint,
        selected_only: bool,
    ) {
        if self.strip_stroke(slot).is_some() {
            self.strip_stroke_cancelled(slot);
        }
        if !self.strip_showing(slot) || self.strip_lane(slot).is_none() {
            return;
        }
        let Some(&lane) = self.strip_rows(slot).get(row) else {
            return;
        };
        let set = *self.strip_settings();
        let seed = self.strip_chart(slot).seed.unwrap_or_else(fresh_seed);
        self.strip_chart_mut(slot).stroke = Some(StripStroke {
            row,
            lane,
            start: p,
            current: p,
            points: vec![p],
            mode: set.mode,
            compress: set.mode == StripMode::Modify && set.op == StripOp::Compress,
            selected_only,
            seed,
            open: false,
            serial: None,
        });
    }

    /// The pointer moved past the view's dead zone: the first move opens
    /// the gesture, each one re-sends the stroke's full command
    /// (`M/StripChartModel.swift:344-358`).
    pub fn strip_stroke_moved(&mut self, slot: ChartSlot, p: StripPoint) {
        let Some(mut s) = self.strip_chart_mut(slot).stroke.take() else {
            return;
        };
        if !self.strip_showing(slot) {
            if self.owns(&s) {
                self.cancel_gesture();
            }
            return;
        }
        s.current = p;
        s.points.push(p);
        if s.open {
            // Its gesture went under it (its track was removed).
            if !self.owns(&s) {
                return;
            }
        } else if !self.open_stroke(slot, &mut s) {
            return;
        }
        let cmd = self.stroke_command(&s);
        self.strip_chart_mut(slot).stroke = Some(s);
        self.update_gesture(cmd);
    }

    /// The pointer went up at `p`: the stroke's final command closes its
    /// gesture as one undo step. A click in Draw or Erase is a one-tick
    /// gesture; in Modify it does nothing
    /// (`M/StripChartModel.swift:360-370`).
    pub fn strip_stroke_ended(&mut self, slot: ChartSlot, p: StripPoint) {
        let Some(mut s) = self.strip_chart_mut(slot).stroke.take() else {
            return;
        };
        if !self.strip_showing(slot) {
            if self.owns(&s) {
                self.cancel_gesture();
            }
            return;
        }
        s.current = p;
        if s.points.last() != Some(&p) {
            s.points.push(p);
        }
        if s.open {
            if !self.owns(&s) {
                return;
            }
        } else if s.mode == StripMode::Modify || !self.open_stroke(slot, &mut s) {
            return;
        }
        let cmd = self.stroke_command(&s);
        self.end_gesture(cmd);
    }

    /// Escape, or the chart went away: no undo step
    /// (`M/StripChartModel.swift:372-376`).
    pub fn strip_stroke_cancelled(&mut self, slot: ChartSlot) {
        if let Some(s) = self.strip_chart_mut(slot).stroke.take()
            && self.owns(&s)
        {
            self.cancel_gesture();
        }
    }

    /// The pane's detail changed from `before`: a stroke of a pane that no
    /// longer shows that Strip Chart (another editor, another target, or
    /// none) is cancelled (Review Focus 1).
    pub(crate) fn detail_changed(&mut self, before: Option<DetailState>) {
        if self.strip_stroke(ChartSlot::Pane).is_none() {
            return;
        }
        let now = self.layout().detail.as_ref();
        let same = match (before.as_ref(), now) {
            (Some(b), Some(n)) => b.kind == n.kind && b.target == n.target,
            _ => false,
        };
        if !same || !self.strip_showing(ChartSlot::Pane) {
            self.strip_stroke_cancelled(ChartSlot::Pane);
        }
    }

    /// The stroke's command (`M/StripChartModel.swift:378-392`): the
    /// bar's settings now, with the stroke's own mode.
    fn stroke_command(&self, s: &StripStroke) -> Option<EditCommand> {
        let mut set = *self.strip_settings();
        set.mode = s.mode;
        match s.mode {
            StripMode::Erase => erase_command(s.lane, s.start.tick, s.current.tick),
            StripMode::Modify if s.compress => Some(compress_command(
                s.lane,
                s.start,
                s.current,
                set.step,
                s.selected_only,
            )),
            _ => {
                let line = stroke_line(s.start, s.current, &s.points, &set, s.seed);
                Some(line_command(
                    s.lane,
                    stroke_op(s.lane, &set),
                    line,
                    s.selected_only,
                ))
            }
        }
    }

    /// Opens the stroke's gesture on its tracks (Plan 4d Ruling 22,
    /// `M/StripChartModel.swift:362-393`): Modify keeps the selection's
    /// tracks when there are any, else the shown track. Only a stroke
    /// that opened moves the selection to its track.
    fn open_stroke(&mut self, slot: ChartSlot, s: &mut StripStroke) -> bool {
        let Some(lane) = self.strip_lane(slot) else {
            return false;
        };
        if s.mode == StripMode::Erase && s.lane.erase_rule(0, 0).is_none() {
            self.status = Some(ERASE_NOTES.into());
            return false;
        }
        let sel = self.selection().clone();
        let keep = s.mode == StripMode::Modify && !sel.lanes.is_empty();
        let lanes: Vec<(u64, u64)> = if keep {
            sel.lanes.iter().map(|r| (r.seq, r.lane)).collect()
        } else {
            vec![lane]
        };
        let mut set = *self.strip_settings();
        set.mode = s.mode;
        let label = if s.mode == StripMode::Erase {
            "Erase Events"
        } else {
            let op = if s.compress {
                LineOpRow::Compress
            } else {
                stroke_op(s.lane, &set)
            };
            stroke_label(s.lane, op)
        };
        let pending = self.gesture().is_some();
        if !self.begin_gesture_from(label, lanes, GestureOrigin::StripChart) {
            if pending {
                self.status = Some(GESTURE_OPEN.into());
            }
            return false;
        }
        let r = LaneRef {
            seq: lane.0,
            lane: lane.1,
        };
        if !keep && sel.lanes != [r] {
            self.set_selection(SelectionRow {
                time_range: sel.time_range,
                lanes: vec![r],
                events: sel.events,
            });
        }
        s.open = true;
        s.serial = self.gesture_serial();
        true
    }
}
