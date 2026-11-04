//! Automation lanes of a compiled schedule (editors spec §4d; Plan 4d
//! Rulings 7, 8, 10): per target, its points `(frame, normalised value)`.
//! Built off the audio thread; read on it by binary search. // RT (reads)

use super::TargetIdx;
use crate::schedule::TargetKind;
use core_model::automation::{AutomationTarget, SendIndex, unit};
use core_model::document::{Document, SlotOwner};
use core_model::ids::{PluginInstanceId, StripId};
use core_model::mixer::ParamId;

/// Most lanes (targets) one schedule plays; more are dropped and reported.
pub const MAX_AUTO_LANES: usize = 1024;
/// Shortest run the renderer splits a block into at an automation point
/// (Plan 4d Ruling 10).
pub const MIN_RUN: u32 = 32;

/// Which control of a strip a lane drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StripParam {
    /// The fader.
    Fader,
    /// The pan.
    Pan,
    /// The mute.
    Mute,
    /// The solo.
    Solo,
}

/// What a lane drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AutoDest {
    /// A strip control.
    Strip(StripId, StripParam),
    /// The level of a strip's send (by position).
    Send(StripId, u16),
    /// A plugin parameter.
    Plugin(PluginInstanceId, ParamId),
}

impl AutoDest {
    /// The lane destination of a target; `None` for velocity scale, which
    /// is applied when the schedule is compiled (Plan 4d Ruling 9).
    pub fn of(t: AutomationTarget) -> Option<AutoDest> {
        Some(match t {
            AutomationTarget::StripFader(s) => AutoDest::Strip(s, StripParam::Fader),
            AutomationTarget::StripPan(s) => AutoDest::Strip(s, StripParam::Pan),
            AutomationTarget::StripMute(s) => AutoDest::Strip(s, StripParam::Mute),
            AutomationTarget::StripSolo(s) => AutoDest::Strip(s, StripParam::Solo),
            AutomationTarget::SendLevel(s, i) => AutoDest::Send(s, i.0),
            AutomationTarget::PluginParam(p, id) => AutoDest::Plugin(p, id),
            AutomationTarget::VelocityScale(_) => return None,
        })
    }

    /// The target this destination drives (the inverse of
    /// [`AutoDest::of`]).
    pub fn target(self) -> AutomationTarget {
        match self {
            AutoDest::Strip(s, StripParam::Fader) => AutomationTarget::StripFader(s),
            AutoDest::Strip(s, StripParam::Pan) => AutomationTarget::StripPan(s),
            AutoDest::Strip(s, StripParam::Mute) => AutomationTarget::StripMute(s),
            AutoDest::Strip(s, StripParam::Solo) => AutomationTarget::StripSolo(s),
            AutoDest::Send(s, i) => AutomationTarget::SendLevel(s, SendIndex(i)),
            AutoDest::Plugin(p, id) => AutomationTarget::PluginParam(p, id),
        }
    }

    /// A level in dB (a fader or a send): Trim offsets it in dB (plan
    /// 5e-1 Ruling 11). // RT
    pub fn is_db(self) -> bool {
        matches!(
            self,
            AutoDest::Strip(_, StripParam::Fader) | AutoDest::Send(..)
        )
    }
}

/// One point: from `frame` on, `value` (normalised 0–1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoPoint {
    /// Schedule frame.
    pub frame: u64,
    /// Normalised value.
    pub value: f64,
}

/// One target's points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoLane {
    /// What it drives.
    pub dest: AutoDest,
    /// Holds each value until the next point (mute, solo, stepped
    /// parameters) instead of ramping.
    pub stepped: bool,
    /// For a plugin instrument's parameter: its target (`None`: an insert,
    /// found in the graph).
    pub instrument: Option<TargetIdx>,
    start: u32,
    end: u32,
}

/// Every lane of a schedule, sorted by destination.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Automation {
    lanes: Box<[AutoLane]>,
    points: Box<[AutoPoint]>,
    /// Every point frame, sorted, without repeats.
    cuts: Box<[u64]>,
}

impl Automation {
    /// Lanes from `(dest, frame, value)` given in play order: each lane
    /// sorted by frame, play order kept on a shared frame; at most
    /// [`MAX_AUTO_LANES`] lanes (returns how many more were dropped); mute
    /// and solo lanes are stepped. Not RT.
    pub fn build(mut raw: Vec<(AutoDest, u64, f64)>) -> (Automation, usize) {
        // Stable: play order stays on a shared frame.
        raw.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut lanes: Vec<AutoLane> = Vec::new();
        let mut points: Vec<AutoPoint> = Vec::with_capacity(raw.len());
        let mut dropped = 0usize;
        let mut current: Option<AutoDest> = None;
        for (dest, frame, value) in raw {
            if current != Some(dest) {
                current = Some(dest);
                if lanes.len() >= MAX_AUTO_LANES {
                    dropped += 1;
                    continue;
                }
                let start = u32::try_from(points.len()).unwrap_or(u32::MAX);
                lanes.push(AutoLane {
                    dest,
                    stepped: matches!(
                        dest,
                        AutoDest::Strip(_, StripParam::Mute | StripParam::Solo)
                    ),
                    instrument: None,
                    start,
                    end: start,
                });
            }
            let Some(lane) = lanes.last_mut().filter(|l| l.dest == dest) else {
                continue;
            };
            points.push(AutoPoint {
                frame,
                value: unit(value),
            });
            lane.end = u32::try_from(points.len()).unwrap_or(u32::MAX);
        }
        let mut cuts: Vec<u64> = points.iter().map(|p| p.frame).collect();
        cuts.sort_unstable();
        cuts.dedup();
        (
            Automation {
                lanes: lanes.into_boxed_slice(),
                points: points.into_boxed_slice(),
                cuts: cuts.into_boxed_slice(),
            },
            dropped,
        )
    }

    /// No lane. // RT
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    /// The lanes, sorted by destination. // RT
    pub fn lanes(&self) -> &[AutoLane] {
        &self.lanes
    }

    /// Points over every lane.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// A lane's points, by frame. // RT
    pub fn points(&self, lane: &AutoLane) -> &[AutoPoint] {
        self.points
            .get(lane.start as usize..lane.end as usize)
            .unwrap_or(&[])
    }

    /// `lane`'s value at `frame` (Plan 4d Ruling 8): the first value before
    /// the first point, the last after the last, linear between unless the
    /// lane is stepped. With `left`, the limit from the left: a point
    /// exactly at `frame` is not yet in effect on a stepped lane, and of
    /// two points on one frame the earlier counts. // RT
    pub fn value(&self, lane: &AutoLane, frame: u64, left: bool) -> f64 {
        let pts = self.points(lane);
        let n = if left {
            pts.partition_point(|p| p.frame < frame)
        } else {
            pts.partition_point(|p| p.frame <= frame)
        };
        let Some(prev) = n.checked_sub(1).and_then(|i| pts.get(i)) else {
            return pts.first().map_or(0.0, |p| p.value);
        };
        if lane.stepped {
            return prev.value;
        }
        match pts.get(n) {
            Some(next) if next.frame > prev.frame => {
                let x = (frame - prev.frame) as f64 / (next.frame - prev.frame) as f64;
                prev.value + (next.value - prev.value) * x
            }
            _ => prev.value,
        }
    }

    /// The first point frame after `frame`, over every lane. The renderer
    /// no longer uses it (it cuts each lane at its own arrival, plan 5e-1
    /// Ruling 5); kept for tests and diagnostics. // RT
    pub fn next_cut(&self, frame: u64) -> Option<u64> {
        let i = self.cuts.partition_point(|c| *c <= frame);
        self.cuts.get(i).copied()
    }

    /// Whether some lane drives `dest`. // RT
    pub fn drives(&self, dest: AutoDest) -> bool {
        self.lanes.binary_search_by_key(&dest, |l| l.dest).is_ok()
    }

    /// Makes the lanes `stepped` says hold their values (parameters the
    /// plugin declares stepped). Not RT.
    pub fn mark_stepped(&mut self, stepped: impl Fn(AutoDest) -> bool) {
        for l in self.lanes.iter_mut() {
            if stepped(l.dest) {
                l.stepped = true;
            }
        }
    }

    /// Gives plugin-parameter lanes the target of their plugin instrument
    /// (`None`: an insert). Not RT.
    pub fn bind_instruments(&mut self, target: impl Fn(PluginInstanceId) -> Option<TargetIdx>) {
        for l in self.lanes.iter_mut() {
            if let AutoDest::Plugin(id, _) = l.dest {
                l.instrument = target(id);
            }
        }
    }

    /// Binds plugin-parameter lanes by the owner of their slot (Plan 4d
    /// P10): a plugin instrument's lane gets that instrument's target
    /// whether or not notes are routed to it; an insert's lane stays
    /// unbound. Returns how many lanes were bound. Not RT.
    pub fn bind_plugins(&mut self, doc: &Document, targets: &super::TargetTable) -> usize {
        let mut bound = 0;
        self.bind_instruments(|id| match doc.find_slot(id) {
            Some(SlotOwner::Instrument(_)) => targets.find(&TargetKind::PluginInstrument(id)),
            _ => None,
        });
        for l in self.lanes.iter() {
            bound += usize::from(l.instrument.is_some());
        }
        bound
    }
}
