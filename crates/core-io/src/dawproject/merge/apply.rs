//! Applying a merge plan (plan 6e Task 6, Rulings 5–7 and 9–11): every
//! decision as commands, inside the caller's one transaction.
//!
//! **Order of commands:**
//!
//! 1. the buses theirs added;
//! 2. the tracks theirs added, as 6d's import adds them (`apply_tracks`,
//!    then `arrange_tracks`), each new slot's state through
//!    `MixerCmd::SetPluginState`;
//! 3. per matched track: its name, notes, regions and automation (one
//!    delete and one insert per track, so a crossfaded pair is inserted in
//!    one command), then the conflict takes (Ruling 6);
//! 4. strips: fields, sends, devices, bypass, state;
//! 5. the markers (one `SetMarkers`), the tempo map, the meter map and the
//!    song info (one `SetInfo`), then theirs' crossfades a step settled
//!    away set again (a region placed before the tempo map it was cut for,
//!    or crossfading into a region of ours that stays);
//! 6. deletions last (Ruling 7).
//!
//! `Same`, `Ours`, setting conflicts, parts added on both sides, a
//! conflict whose take already exists and the deletions that keep a part
//! run no command, so a plan with nothing to do makes no step. Converted
//! media no region uses are left for the caller to remove.
//!
//! **Ticks.** Theirs' projection is in the file's frame; a tick goes back
//! into ours' by adding the snapshot's origin (Ruling 1), less the
//! track's play shift (the projection is what plays). A track with its own
//! loop, length or play quantize has no inverse: `decide` never gives it
//! theirs' notes, regions or curves as edits (they are a conflict take or
//! listed), so no edit here is placed through one.
//!
//! **Regions** (Ruling 5): only the changed ones are replaced; one that is
//! the same keeps its event, so a use it plays through stays a use and
//! the next file's tags still name it. A crossfaded pair is replaced
//! together. **Devices:** a strip's bypass and state units after a device
//! list change name the slots the change left.
//!
//! **Shared sequences (Ruling 8, Task 6b; [`shared`]).** With a format-2
//! snapshot, theirs' changes to what plays through one of ours' reference
//! uses are gathered per use and go into the used sequence, or a private
//! copy of it, after the lanes' edits. A use that cannot take them as one
//! edit (passes edited differently, or an edit the sequence cannot play
//! the same) is replaced by what theirs plays (`LaneEdit`'s `unroll`,
//! listed under [`text::LOOPS_UNROLLED`]). With a format-1 snapshot every
//! such use is replaced so (Ruling 3: a use counts as its notes).

use super::decide::{MergePlan, Outcome, StripField, Unit, same_note, same_region, ticks_near};
use super::matching::{Matched, notes, regions};
use super::normalised;
use super::text;
use super::theirs::Theirs;
use crate::dawproject::import::{
    UNPLACED, add_buses_with_mix, apply_tracks, arrange_tracks, bus_of_strip,
};
use crate::dawproject::record::{
    EventAt, Identity, PStrip, Projection, TrackIds, projection_with_identity,
};
use crate::dawproject::units::nearest_swatch;
use crate::report::{ItemKind, Report};
use core_model::automation::{AutomationTarget, SendIndex};
use core_model::command::{
    Command, EventCmd, LaneCmd, MediaCmd, MixerCmd, Report as CmdReport, SequenceCmd, SettingsCmd,
};
use core_model::document::Document;
use core_model::error::ModelError;
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, MediaId, PluginInstanceId, SeqId, StripId};
use core_model::meter::{Meter, MeterEntry, MeterMap};
use core_model::mixer::{AuxSend, Output, PluginRef};
use core_model::selection::ViewFilter;
use core_model::sequence::Marker;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod shared;

/// The merge's one undo step.
pub const LABEL: &str = "Merge DAWproject";

/// The Ruling 8 lines only an apply knows; `decide::plan` lists them in the
/// preview from a dry run of [`apply_merge`].
pub(super) const APPLIED_LINES: [&str; 4] = [
    text::PRIVATE_COPIES,
    text::LOOPS_UNROLLED,
    text::USES_AS_NOTES,
    text::SHARED_ELSEWHERE,
];

/// Inside the caller's open transaction on the project `plan` was made
/// against: carries out each decision (Rulings 5–11), then merges the
/// plan's report into `report`.
/// `report.summary` keeps its counts; the merge's summary line is
/// `text::summary(&plan.preview)` (Ruling 15). `date` names conflict takes
/// ("Returned <date>").
pub fn apply_merge(
    h: &mut History,
    plan: &MergePlan,
    date: &str,
    report: &mut Report,
) -> Result<(), ModelError> {
    apply_merge_until(h, plan, date, report, &AtomicBool::new(false))
}

/// What a cancelled [`apply_merge_until`] answers.
pub const CANCELLED: &str = "the merge was cancelled";

/// What [`apply_merge_until`] asks, at each of its checks, whether to stop.
/// A job's cancel flag is one; a test can stop at a chosen check.
pub trait Stop {
    /// Whether to stop now.
    fn stopped(&self) -> bool;
}

impl Stop for AtomicBool {
    fn stopped(&self) -> bool {
        self.load(Ordering::Relaxed)
    }
}

/// [`apply_merge`], stopping with [`CANCELLED`] (a `ModelError`) when
/// `cancel` is set, looked at before each step, each added bus or track
/// batch, each plug-in state, each matched track's decision, each track's
/// edit and conflict take, each strip decision and each deletion, so
/// `decide::plan`'s dry run and a merge job can stop midway (Task 7). The
/// caller aborts its transaction.
pub fn apply_merge_until(
    h: &mut History,
    plan: &MergePlan,
    date: &str,
    report: &mut Report,
    cancel: &dyn Stop,
) -> Result<(), ModelError> {
    let seq = SeqId(plan.base.seq);
    let (raw, ids) =
        projection_with_identity(h.doc(), seq).map_err(|e| ModelError::invalid(e.to_string()))?;
    let normal = normalised(&raw, &plan.base);
    let mut cx = Cx::new(plan, &raw, &normal, &ids, seq, cancel);
    let mut out = Out::default();

    // 1. The buses theirs added.
    cx.stop()?;
    let mut buses = cx.matched_buses();
    let mut states = Vec::new();
    let added_buses: Vec<(usize, usize)> = cx
        .decided(|u| *u == Unit::Bus, Outcome::Added)
        .into_iter()
        .filter_map(|row| Some((row, cx.theirs_bus(cx.m_strip(row)?.theirs?)?)))
        .collect();
    if !added_buses.is_empty() {
        let which: Vec<usize> = added_buses.iter().map(|(_, b)| *b).collect();
        add_buses_with_mix(h, &cx.th.imp, &which, &mut buses, &mut states)?;
        for (row, b) in &added_buses {
            if let Some(id) = buses.get(b) {
                cx.new_strips.insert(*row, *id);
            }
        }
    }
    // 2. The tracks theirs added (a split track is one).
    cx.stop()?;
    let added_tracks: BTreeSet<usize> = cx
        .decided(|u| *u == Unit::Track, Outcome::Added)
        .into_iter()
        .filter_map(|row| cx.theirs_plan_track(cx.m_track(row)?.theirs?))
        .collect();
    if !added_tracks.is_empty() {
        let which: Vec<usize> = added_tracks.into_iter().collect();
        let mut lines = Report::default();
        // A bus ours deleted stays deleted: a new track plays into the
        // master instead and makes no send to it (`decide` lists both).
        let absent = cx.deleted_buses();
        let added = apply_tracks(h, &cx.th.imp, seq, &which, &mut buses, &absent)?;
        cx.stop()?;
        let flagged = arrange_tracks(h, &cx.th.imp, seq, &which, &added, cx.origin, &mut lines)?;
        out.flagged.extend(flagged);
        states.extend(added.states.iter().cloned());
        out.report.merge(lines);
    }
    for (slot, bytes) in states {
        cx.stop()?;
        run(h, Command::Mixer(MixerCmd::SetPluginState { slot, bytes }))?;
    }

    // 3. Matched tracks.
    tracks(h, &cx, date, &mut out)?;
    // 4. Strips.
    cx.stop()?;
    strips(h, &cx)?;
    // 5. Markers, maps and song info.
    cx.stop()?;
    markers(h, &cx)?;
    maps(h, &cx)?;
    // Theirs' crossfades placed in step 3, against theirs' tempo map now.
    reflag(h, &out.flagged)?;
    // 6. Deletions.
    cx.stop()?;
    deletions(h, &cx)?;

    let counts = report.summary;
    // The plan's report holds the preview's dry run of these lines
    // (`decide::plan`); this apply lists them as it made them.
    let mut planned = plan.report.clone();
    planned
        .items
        .retain(|i| !APPLIED_LINES.contains(&i.what.as_str()));
    report.merge(planned);
    for d in out.takes {
        report.add(ItemKind::Conflict, text::CONFLICT_TAKE, Some(d));
    }
    for (what, d) in out.unrolled {
        report.add(ItemKind::Changed, what, Some(d));
    }
    for d in out.copies {
        report.add(ItemKind::Changed, text::PRIVATE_COPIES, Some(d));
    }
    for d in out.elsewhere {
        report.add(ItemKind::Note, text::SHARED_ELSEWHERE, Some(d));
    }
    report.merge(out.report);
    report.summary = counts;
    Ok(())
}

// ---------------------------------------------------------------- context

fn run(h: &mut History, cmd: Command) -> Result<CmdReport, ModelError> {
    h.execute(&cmd, &ViewFilter::default())
}

/// What the merge lists as it goes.
#[derive(Default)]
struct Out {
    /// [`text::conflict_take`] details.
    takes: Vec<String>,
    /// [`text::LOOPS_UNROLLED`] and [`text::USES_AS_NOTES`] lines, with
    /// their details.
    unrolled: Vec<(&'static str, String)>,
    /// [`text::PRIVATE_COPIES`] details.
    copies: Vec<String>,
    /// [`text::SHARED_ELSEWHERE`] details.
    elsewhere: Vec<String>,
    /// Lines from adding theirs' new tracks and placing their automation.
    report: Report,
    /// The regions placed as theirs crossfaded, with their specs: a later
    /// step (the tempo map) may settle their flag away, so it is set again
    /// last (Task 7).
    flagged: Vec<(EventId, EventSpec)>,
}

/// The plan and ours, as the merge reads them.
struct Cx<'a> {
    plan: &'a MergePlan,
    th: &'a Theirs,
    /// Ours' projection as the project plays it.
    raw: &'a Projection,
    /// Ours' projection in the file's frame (the plan compared it).
    normal: &'a Projection,
    ids: &'a Identity,
    seq: SeqId,
    origin: i64,
    /// Per side (theirs, ours), each strip's row in `matches.strips`.
    srow: [Vec<Option<usize>>; 2],
    /// Theirs' scratch lanes, each with its plan track (its place).
    theirs_tracks: BTreeMap<u64, usize>,
    /// Theirs' scratch bus strips, each with its plan bus.
    theirs_buses: BTreeMap<StripId, usize>,
    /// The rows of `matches.strips` whose bus step 1 added.
    new_strips: BTreeMap<usize, StripId>,
    /// Whether edits inside uses are refined (Ruling 8): a format-2
    /// snapshot.
    refine: bool,
    /// Set to stop ([`apply_merge_until`]).
    cancel: &'a dyn Stop,
}

impl<'a> Cx<'a> {
    /// [`CANCELLED`] once `cancel` is set.
    fn stop(&self) -> Result<(), ModelError> {
        if self.cancel.stopped() {
            Err(ModelError::invalid(CANCELLED))
        } else {
            Ok(())
        }
    }

    fn new(
        plan: &'a MergePlan,
        raw: &'a Projection,
        normal: &'a Projection,
        ids: &'a Identity,
        seq: SeqId,
        cancel: &'a dyn Stop,
    ) -> Cx<'a> {
        let th = &plan.theirs;
        let mut srow = [
            vec![None; th.projection.strips.len()],
            vec![None; raw.strips.len()],
        ];
        for (k, r) in plan.matches.strips.iter().enumerate() {
            for (side, i) in [r.theirs, r.ours].into_iter().enumerate() {
                if let Some(slot) = i.and_then(|i| srow.get_mut(side)?.get_mut(i)) {
                    *slot = Some(k);
                }
            }
        }
        let theirs_tracks = th
            .doc
            .sequence(th.seq)
            .map(|s| {
                s.lanes
                    .iter()
                    .enumerate()
                    .map(|(i, l)| (l.id.0, i))
                    .collect()
            })
            .unwrap_or_default();
        Cx {
            plan,
            th,
            raw,
            normal,
            ids,
            seq,
            origin: plan.base.origin,
            srow,
            theirs_tracks,
            theirs_buses: bus_of_strip(&th.doc),
            new_strips: BTreeMap::new(),
            refine: plan.base.identity.is_some(),
            cancel,
        }
    }

    /// The rows of the decisions on units `unit` with outcome `outcome`.
    fn decided(&self, unit: impl Fn(&Unit) -> bool, outcome: Outcome) -> Vec<usize> {
        self.plan
            .decisions
            .iter()
            .filter(|d| d.outcome == outcome && unit(&d.unit))
            .map(|d| d.row)
            .collect()
    }

    fn m_track(&self, row: usize) -> Option<&Matched> {
        self.plan.matches.tracks.get(row)
    }

    fn m_strip(&self, row: usize) -> Option<&Matched> {
        self.plan.matches.strips.get(row)
    }

    /// Theirs projection track `t`'s plan track.
    fn theirs_plan_track(&self, t: usize) -> Option<usize> {
        let lane = self.th.identity.tracks.get(t)?.lane;
        self.theirs_tracks.get(&lane).copied()
    }

    /// Theirs projection strip `t`'s plan bus, when it is a bus.
    fn theirs_bus(&self, t: usize) -> Option<usize> {
        let id = self.th.identity.strips.get(t)?.strip?;
        self.theirs_buses.get(&StripId(id)).copied()
    }

    /// The plan buses theirs and ours both have, with ours' strips.
    fn matched_buses(&self) -> BTreeMap<usize, StripId> {
        self.plan
            .matches
            .strips
            .iter()
            .filter_map(|r| Some((self.theirs_bus(r.theirs?)?, self.ours_strip_at(r.ours?)?)))
            .collect()
    }

    /// The plan buses base and theirs have and ours deleted.
    fn deleted_buses(&self) -> BTreeSet<usize> {
        self.plan
            .matches
            .strips
            .iter()
            .filter(|r| r.base.is_some() && r.ours.is_none())
            .filter_map(|r| self.theirs_bus(r.theirs?))
            .collect()
    }

    /// Ours' projection strip `o`'s strip.
    fn ours_strip_at(&self, o: usize) -> Option<StripId> {
        self.ids.strips.get(o)?.strip.map(StripId)
    }

    /// Ours' strip for row `row` of `matches.strips`: matched, or added by
    /// step 1.
    fn ours_strip_of_row(&self, row: usize) -> Option<StripId> {
        self.m_strip(row)
            .and_then(|r| r.ours)
            .and_then(|o| self.ours_strip_at(o))
            .or_else(|| self.new_strips.get(&row).copied())
    }

    /// The row of `matches.strips` of theirs projection strip `t`.
    fn theirs_row(&self, t: usize) -> Option<usize> {
        self.srow.first()?.get(t).copied().flatten()
    }

    /// Ours' strip standing for theirs projection strip `t`.
    fn ours_strip_for_theirs(&self, t: usize) -> Option<StripId> {
        self.ours_strip_of_row(self.theirs_row(t)?)
    }

    /// Theirs' projection strip of scratch strip `id` (its first channel).
    fn theirs_strip_index(&self, id: StripId) -> Option<usize> {
        self.th
            .identity
            .strips
            .iter()
            .position(|s| s.strip == Some(id.0))
    }

    /// A tick of theirs' projection in ours' frame, on a track shifted by
    /// `shift`.
    fn to_ours(&self, tick: i64, shift: i64) -> Tick {
        Tick(tick.saturating_add(self.origin).saturating_sub(shift))
    }
}

// ---------------------------------------------------------------- tracks

/// What one track's edits do: events deleted, uses replaced by what they
/// play, events inserted (one delete, one insert).
#[derive(Default)]
struct LaneEdit {
    /// Top-level events deleted.
    delete: BTreeSet<EventId>,
    /// Played events inside uses that are deleted: (event, use, the tick
    /// it plays at), so one pass of a looped use loses it alone.
    gone_in_uses: BTreeSet<(u64, u64, i64)>,
    /// Uses replaced by what they play.
    unroll: BTreeSet<u64>,
    /// Those of them whose passes were edited differently (listed under
    /// [`text::LOOPS_UNROLLED`]; the others under [`text::USES_AS_NOTES`]).
    looped: BTreeSet<u64>,
    insert: Vec<EventSpec>,
    /// Top-level regions edited in place (`EventCmd::Replace`), so they
    /// keep their events (Task 6 fix round 2).
    replace: Vec<(EventId, EventSpec)>,
    /// Changes inside uses, for Ruling 8 ([`shared::resolve`]).
    inside: Vec<shared::Change>,
}

impl LaneEdit {
    /// Deletes ours' played event `at`, playing at `tick`: itself at the
    /// top level, else the use it plays through is unrolled without it.
    fn delete(&mut self, at: &EventAt, tick: i64) {
        if at.event == at.top {
            self.delete.insert(EventId(at.event));
        } else {
            self.gone_in_uses.insert((at.event, at.top, tick));
            self.unroll.insert(at.top);
        }
    }
}

/// Ours' projection rows of lane `lane` (two for a split track).
fn lane_rows(cx: &Cx<'_>, lane: u64) -> Vec<usize> {
    cx.ids
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.lane == lane)
        .map(|(i, _)| i)
        .collect()
}

fn play_shift(doc: &Document, seq: SeqId, lane: u64) -> i64 {
    doc.lane(seq, LaneId(lane)).map_or(0, |l| l.play_shift.0)
}

/// Step 3: theirs' track names, notes, regions and curves, lane by lane,
/// then the conflict takes.
fn tracks(h: &mut History, cx: &Cx<'_>, date: &str, out: &mut Out) -> Result<(), ModelError> {
    let mut edits: Vec<(u64, LaneEdit)> = Vec::new();
    // Per lane, ours' rows whose notes or regions conflict.
    let mut takes: Vec<(u64, Vec<(usize, Unit)>)> = Vec::new();
    for d in &cx.plan.decisions {
        cx.stop()?;
        let take = d.outcome == Outcome::Conflict && matches!(d.unit, Unit::Notes | Unit::Regions);
        let theirs = d.outcome == Outcome::Theirs
            && matches!(
                d.unit,
                Unit::TrackName | Unit::Notes | Unit::Regions | Unit::Automation(_)
            );
        if !take && !theirs {
            continue;
        }
        let Some(r) = cx.m_track(d.row).copied() else {
            continue;
        };
        let (Some(t), Some(o)) = (r.theirs, r.ours) else {
            continue;
        };
        let Some(lane) = cx.ids.tracks.get(o).map(|x| x.lane) else {
            continue;
        };
        if take {
            match takes.iter_mut().find(|(l, _)| *l == lane) {
                Some((_, parts)) => parts.push((o, d.unit.clone())),
                None => takes.push((lane, vec![(o, d.unit.clone())])),
            }
            continue;
        }
        if d.unit == Unit::TrackName {
            rename(h, cx, lane, o, t)?;
            continue;
        }
        let at = match edits.iter().position(|(l, _)| *l == lane) {
            Some(at) => at,
            None => {
                edits.push((lane, LaneEdit::default()));
                edits.len().saturating_sub(1)
            }
        };
        let shift = play_shift(h.doc(), cx.seq, lane);
        let mut edit = match edits.get_mut(at) {
            Some((_, e)) => std::mem::take(e),
            None => continue,
        };
        match &d.unit {
            Unit::Notes => edit_notes(h.doc(), cx, &mut edit, (o, t), lane, shift),
            Unit::Regions => edit_regions(h, cx, &mut edit, o, t, shift)?,
            Unit::Automation(key) => edit_curve(h.doc(), cx, &mut edit, key, (o, t), shift, out),
            _ => {}
        }
        if let Some((_, e)) = edits.get_mut(at) {
            *e = edit;
        }
    }
    // Ruling 8: the uses theirs edited inside, then the lanes (an unrolled
    // use plays its sequence as it was), then the sequences.
    let shared = shared::resolve(h.doc(), cx, &mut edits);
    for (lane, edit) in edits {
        cx.stop()?;
        apply_edit(h, cx, lane, edit, out)?;
    }
    shared::write(h, &shared)?;
    out.copies.extend(shared.copies);
    out.elsewhere.extend(shared.elsewhere);
    for (lane, parts) in takes {
        cx.stop()?;
        conflict_take(h, cx, lane, &parts, date, out)?;
    }
    Ok(())
}

fn rename(h: &mut History, cx: &Cx<'_>, lane: u64, o: usize, t: usize) -> Result<(), ModelError> {
    let (Some(tt), Some(ot)) = (cx.th.projection.tracks.get(t), cx.raw.tracks.get(o)) else {
        return Ok(());
    };
    let Ok(l) = h.doc().lane(cx.seq, LaneId(lane)) else {
        return Ok(());
    };
    // A split track's audio part is "<name> audio".
    let audio_part = ot.audio && ot.name == format!("{} audio", l.name);
    let name = match tt.name.strip_suffix(" audio") {
        Some(n) if audio_part => n.to_owned(),
        _ => tt.name.clone(),
    };
    if name == l.name {
        return Ok(());
    }
    run(
        h,
        Command::Lane(LaneCmd::Rename {
            seq: cx.seq,
            lane: LaneId(lane),
            name,
        }),
    )?;
    Ok(())
}

/// Theirs' played event `at` at projected tick `tick` (theirs' frame) as
/// a spec for ours, `pitch` a note's played key.
fn theirs_spec(
    cx: &Cx<'_>,
    at: &EventAt,
    tick: i64,
    pitch: Option<u8>,
    shift: i64,
) -> Option<EventSpec> {
    let (e, _) = cx.th.doc.event(EventId(at.event)).ok()?;
    let mut body = e.body.clone();
    if let (Some(p), EventBody::Note { pitch: q, .. }) = (pitch, &mut body) {
        *q = p;
    }
    Some(EventSpec {
        tick: cx.to_ours(tick, shift),
        dest: None,
        group_channel: e.group_channel,
        mute: false,
        body,
    })
}

/// Theirs track `t`'s notes `which` picks, as ours' specs, flattened as
/// they play.
fn theirs_notes(
    cx: &Cx<'_>,
    t: usize,
    shift: i64,
    which: impl Fn(usize) -> bool,
) -> Vec<EventSpec> {
    let (Some(tt), Some(ti)) = (cx.th.projection.tracks.get(t), cx.th.identity.tracks.get(t))
    else {
        return Vec::new();
    };
    tt.notes_played
        .iter()
        .zip(&ti.notes)
        .enumerate()
        .filter(|(j, _)| which(*j))
        .filter_map(|(_, (n, at))| theirs_spec(cx, at, n.tick, Some(n.pitch), shift))
        .collect()
}

/// Theirs track `t`'s MIDI events `which` picks, as ours' specs.
fn theirs_midi(cx: &Cx<'_>, t: usize, shift: i64, which: impl Fn(usize) -> bool) -> Vec<EventSpec> {
    let (Some(tt), Some(ti)) = (cx.th.projection.tracks.get(t), cx.th.identity.tracks.get(t))
    else {
        return Vec::new();
    };
    tt.midi
        .iter()
        .zip(&ti.midi)
        .enumerate()
        .filter(|(j, _)| which(*j))
        .filter_map(|(_, (m, at))| theirs_spec(cx, at, m.0, None, shift))
        .collect()
}

/// MIDI events (not notes) as a set: equal texts at ticks within
/// `NOTE_TICKS`. The unpaired of each side.
fn midi_unpaired(a: &[(i64, String)], b: &[(i64, String)]) -> (Vec<usize>, BTreeSet<usize>) {
    let mut free: BTreeMap<(&str, i64), Vec<usize>> = BTreeMap::new();
    for (j, (tick, text)) in b.iter().enumerate() {
        free.entry((text.as_str(), *tick)).or_default().push(j);
    }
    let mut lone_a = Vec::new();
    for (i, (tick, text)) in a.iter().enumerate() {
        let lo = (text.as_str(), tick.saturating_sub(super::NOTE_TICKS));
        let hi = (text.as_str(), tick.saturating_add(super::NOTE_TICKS));
        let found = free
            .range_mut(lo..=hi)
            .find(|(k, v)| !v.is_empty() && ticks_near(k.1, *tick))
            .and_then(|(_, v)| v.pop());
        if found.is_none() {
            lone_a.push(i);
        }
    }
    (lone_a, free.into_values().flatten().collect())
}

/// Ruling 5: note-level edits. Ours' notes theirs does not have (within
/// the tolerances) are deleted and theirs' are inserted; MIDI events as a
/// set. With [`Cx::refine`], a change to what plays through a use is that
/// use's (Ruling 8, [`shared`]).
fn edit_notes(
    doc: &Document,
    cx: &Cx<'_>,
    edit: &mut LaneEdit,
    (o, t): (usize, usize),
    lane: u64,
    shift: i64,
) {
    let (Some(ot), Some(oi), Some(tt), Some(ti)) = (
        cx.normal.tracks.get(o),
        cx.ids.tracks.get(o),
        cx.th.projection.tracks.get(t),
        cx.th.identity.tracks.get(t),
    ) else {
        return;
    };
    let owners = cx.refine.then(|| shared::Owners::new(doc, cx, lane, t));
    let raw = |tick: i64| tick.saturating_add(cx.origin);
    let ours_of = |at: &EventAt| owners.as_ref().and_then(|w| w.of_ours(at));
    let theirs_of = |at: &EventAt, tick: i64| {
        owners
            .as_ref()
            .and_then(|w| w.of_theirs(doc, cx, at, raw(tick)))
    };
    let lost = |at: &EventAt, tick: i64| shared::Lost {
        at: *at,
        tick: raw(tick),
    };
    let note = |j: usize| -> Option<shared::Gained> {
        let (n, at) = (tt.notes_played.get(j)?, ti.notes.get(j)?);
        Some(shared::Gained {
            tick: raw(n.tick),
            key: shared::Key::Note(n.clone()),
            spec: theirs_spec(cx, at, n.tick, Some(n.pitch), shift)?,
        })
    };
    let mut add = BTreeSet::new();
    for (i, j) in notes(ot, tt) {
        let (a, b) = (
            i.and_then(|i| ot.notes_played.get(i)),
            j.and_then(|j| tt.notes_played.get(j)),
        );
        if let (Some(a), Some(b)) = (a, b)
            && same_note(a, b)
        {
            continue;
        }
        let ours = i.and_then(|i| Some((oi.notes.get(i)?, a?.tick)));
        let theirs = j.and_then(|j| Some((j, theirs_of(ti.notes.get(j)?, b?.tick))));
        let ou = ours.and_then(|(at, _)| ours_of(at));
        if let Some((at, tick)) = ours {
            match ou {
                Some(u) => {
                    let with = theirs
                        .filter(|(_, tu)| *tu == Some(u))
                        .and_then(|(j, _)| note(j));
                    edit.inside.push(shared::Change {
                        use_ev: u,
                        ours: Some(lost(at, tick)),
                        theirs: with,
                    });
                }
                None => edit.delete(at, raw(tick)),
            }
        }
        match theirs {
            Some((j, Some(u))) if ou != Some(u) => edit.inside.push(shared::Change {
                use_ev: u,
                ours: None,
                theirs: note(j),
            }),
            Some((j, None)) => {
                add.insert(j);
            }
            _ => {}
        }
    }
    edit.insert
        .extend(theirs_notes(cx, t, shift, |j| add.contains(&j)));
    let (lone_o, lone_t) = midi_unpaired(&ot.midi, &tt.midi);
    for i in lone_o {
        let (Some(at), Some(m)) = (oi.midi.get(i), ot.midi.get(i)) else {
            continue;
        };
        match ours_of(at) {
            Some(u) => edit.inside.push(shared::Change {
                use_ev: u,
                ours: Some(lost(at, m.0)),
                theirs: None,
            }),
            None => edit.delete(at, raw(m.0)),
        }
    }
    let mut top = BTreeSet::new();
    for j in lone_t {
        let (Some(at), Some(m)) = (ti.midi.get(j), tt.midi.get(j)) else {
            continue;
        };
        match theirs_of(at, m.0) {
            Some(u) => edit.inside.push(shared::Change {
                use_ev: u,
                ours: None,
                theirs: theirs_spec(cx, at, m.0, None, shift).map(|spec| shared::Gained {
                    tick: raw(m.0),
                    key: shared::Key::Midi,
                    spec,
                }),
            }),
            None => {
                top.insert(j);
            }
        }
    }
    edit.insert
        .extend(theirs_midi(cx, t, shift, |j| top.contains(&j)));
}

/// Ours' media for theirs' scratch media item `m`: ours' item of the same
/// content hash (Ruling 9), else, with `add`, a new one; `None` when there
/// is none and `add` is false.
fn media_for(
    h: &mut History,
    cx: &Cx<'_>,
    m: MediaId,
    add: bool,
) -> Result<Option<MediaId>, ModelError> {
    let Some(item) = cx.th.doc.media().get(&m) else {
        return Ok(None);
    };
    // By content hash (Ruling 9); an item without one (offline in the
    // scratch import) by its path, so the same file merged twice finds
    // the copy the first merge added.
    let found = match &item.content_hash {
        Some(hash) => h
            .doc()
            .media()
            .values()
            .find(|x| x.content_hash.as_ref() == Some(hash)),
        None => h
            .doc()
            .media()
            .values()
            .find(|x| x.content_hash.is_none() && x.path == item.path),
    };
    if let Some(found) = found {
        return Ok(Some(found.id));
    }
    if !add {
        return Ok(None);
    }
    let r = run(
        h,
        Command::Media(MediaCmd::Add {
            path: item.path.clone(),
            sample_rate: item.sample_rate,
            channels: item.channels,
            length_frames: item.length_frames,
            content_hash: item.content_hash.clone(),
            name: Some(item.name.clone()),
            origin: Some(item.origin.clone()),
        }),
    )?;
    Ok(r.created.iter().find_map(|id| match id {
        AnyId::Media(m) => Some(*m),
        _ => None,
    }))
}

/// Theirs track `t`'s regions `which` picks as ours' specs: one paired
/// with a region of ours track `o` keeps ours' media and gain (Ruling 11);
/// another uses theirs' converted copy (made ours' with `add`). `None`
/// when, without `add`, some region's media is not ours'.
fn theirs_regions(
    h: &mut History,
    cx: &Cx<'_>,
    (o, t): (usize, usize),
    shift: i64,
    add: bool,
    which: impl Fn(usize) -> bool,
) -> Result<Option<Vec<EventSpec>>, ModelError> {
    Ok(theirs_region_specs(h, cx, (o, t), shift, add, which)?
        .map(|v| v.into_iter().map(|(_, s)| s).collect()))
}

/// [`theirs_regions`], each spec with its theirs region index.
fn theirs_region_specs(
    h: &mut History,
    cx: &Cx<'_>,
    (o, t): (usize, usize),
    shift: i64,
    add: bool,
    which: impl Fn(usize) -> bool,
) -> Result<Option<Vec<(usize, EventSpec)>>, ModelError> {
    let (Some(tt), Some(ti)) = (cx.th.projection.tracks.get(t), cx.th.identity.tracks.get(t))
    else {
        return Ok(Some(Vec::new()));
    };
    let mut ours_of: BTreeMap<usize, (MediaId, f32)> = BTreeMap::new();
    if let (Some(ot), Some(oi)) = (cx.normal.tracks.get(o), cx.ids.tracks.get(o)) {
        for (i, j) in regions((ot, oi), (tt, ti, &cx.th.origins)) {
            let (Some(i), Some(j)) = (i, j) else { continue };
            let Some(at) = oi.regions.get(i) else {
                continue;
            };
            if let Ok((e, _)) = h.doc().event(EventId(at.event))
                && let EventBody::AudioRegion { media, gain_db, .. } = e.body
            {
                ours_of.insert(j, (media, gain_db));
            }
        }
    }
    let mut out = Vec::new();
    for (j, (r, at)) in tt.regions.iter().zip(&ti.regions).enumerate() {
        if !which(j) {
            continue;
        }
        let Some(mut s) = theirs_spec(cx, at, r.tick, None, shift) else {
            continue;
        };
        s.mute = r.muted;
        let EventBody::AudioRegion { media, gain_db, .. } = &mut s.body else {
            continue;
        };
        match ours_of.get(&j) {
            Some((m, g)) => {
                *media = *m;
                *gain_db = *g;
            }
            None => match media_for(h, cx, *media, add)? {
                Some(m) => *media = m,
                None => return Ok(None),
            },
        }
        out.push((j, s));
    }
    Ok(Some(out))
}

/// Union-find over the region pairs of [`edit_regions`].
struct Groups(Vec<usize>);

impl Groups {
    fn find(&mut self, mut x: usize) -> usize {
        while let Some(&p) = self.0.get(x) {
            if p == x {
                break;
            }
            // Path halving.
            let up = self.0.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.0.get_mut(x) {
                *slot = up;
            }
            x = p;
        }
        x
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if let Some(slot) = self.0.get_mut(a) {
            *slot = b;
        }
    }
}

/// Each flagged region of lane `lane`'s active take in `doc`, with the
/// region its crossfade crosses.
fn crossfades(doc: &Document, seq: SeqId, lane: u64) -> Vec<(u64, u64)> {
    doc.lane(seq, LaneId(lane))
        .ok()
        .and_then(|l| l.active())
        .map(|take| {
            let flagged: BTreeSet<EventId> = take
                .events
                .values()
                .filter(|e| {
                    matches!(
                        e.body,
                        EventBody::AudioRegion {
                            crossfade: true,
                            ..
                        }
                    )
                })
                .map(|e| e.id)
                .collect();
            if flagged.is_empty() {
                return Vec::new();
            }
            core_model::crossfade::overlaps(doc, seq, take)
                .into_iter()
                .filter(|(id, _)| flagged.contains(id))
                .map(|(id, o)| (id.0, o.with.0))
                .collect()
        })
        .unwrap_or_default()
}

/// Ruling 5: only the changed regions are replaced (Task 6 fix round 1,
/// I5). Ours' regions paired with theirs and the same keep their events,
/// so a reference use they play through stays a use and the next export's
/// tags still name them. A crossfaded pair is replaced together when
/// either changes (on either side), in the lane's one insert.
fn edit_regions(
    h: &mut History,
    cx: &Cx<'_>,
    edit: &mut LaneEdit,
    o: usize,
    t: usize,
    shift: i64,
) -> Result<(), ModelError> {
    let (Some(ot), Some(oi), Some(tt), Some(ti)) = (
        cx.normal.tracks.get(o),
        cx.ids.tracks.get(o),
        cx.th.projection.tracks.get(t),
        cx.th.identity.tracks.get(t),
    ) else {
        return Ok(());
    };
    let pairs = regions((ot, oi), (tt, ti, &cx.th.origins));
    let changed: Vec<bool> = pairs
        .iter()
        .map(|&(i, j)| {
            match (
                i.and_then(|i| ot.regions.get(i)),
                j.and_then(|j| tt.regions.get(j)),
            ) {
                (Some(a), Some(b)) => !same_region(a, b),
                _ => true,
            }
        })
        .collect();
    // Each top-level region's pair, per side.
    let mut ours_pair: BTreeMap<u64, usize> = BTreeMap::new();
    let mut theirs_pair: BTreeMap<u64, usize> = BTreeMap::new();
    for (p, &(i, j)) in pairs.iter().enumerate() {
        if let Some(at) = i.and_then(|i| oi.regions.get(i))
            && at.event == at.top
        {
            ours_pair.insert(at.event, p);
        }
        if let Some(at) = j.and_then(|j| ti.regions.get(j)) {
            theirs_pair.insert(at.event, p);
        }
    }
    let mut groups = Groups((0..pairs.len()).collect());
    let sides = [
        (crossfades(h.doc(), cx.seq, oi.lane), &ours_pair),
        (crossfades(&cx.th.doc, cx.th.seq, ti.lane), &theirs_pair),
    ];
    for (fades, pair) in &sides {
        for (later, earlier) in fades {
            if let (Some(&a), Some(&b)) = (pair.get(later), pair.get(earlier)) {
                groups.join(a, b);
            }
        }
    }
    let dirty: BTreeSet<usize> = (0..pairs.len())
        .filter(|&p| changed.get(p).copied().unwrap_or(true))
        .map(|p| groups.find(p))
        .collect();
    // A paired top-level region is edited in place, keeping its event (so
    // the same file merged again pairs it by its tag). A change to what
    // plays through a use is that use's (Ruling 8, with `refine`; else it
    // unrolls the use). Anything else is deleted and theirs' inserted.
    let owners = cx
        .refine
        .then(|| shared::Owners::new(h.doc(), cx, oi.lane, t));
    let raw = |tick: i64| tick.saturating_add(cx.origin);
    let mut add = BTreeSet::new();
    let mut in_place: BTreeMap<usize, EventId> = BTreeMap::new();
    // Theirs' regions that are a use's change, with ours' region they
    // replace.
    let mut in_use: BTreeMap<usize, (u64, Option<shared::Lost>)> = BTreeMap::new();
    for (p, &(i, j)) in pairs.iter().enumerate() {
        if !dirty.contains(&groups.find(p)) {
            continue;
        }
        let at = i.and_then(|i| Some((oi.regions.get(i)?, ot.regions.get(i)?.tick)));
        match (at, j) {
            (Some((at, _)), Some(j)) if at.event == at.top => {
                in_place.insert(j, EventId(at.event));
            }
            _ => {
                if let Some((at, tick)) = at {
                    let lost = shared::Lost {
                        at: *at,
                        tick: raw(tick),
                    };
                    match owners.as_ref().and_then(|w| w.of_ours(at)) {
                        Some(u) => match j {
                            Some(j) => {
                                in_use.insert(j, (u, Some(lost)));
                            }
                            None => edit.inside.push(shared::Change {
                                use_ev: u,
                                ours: Some(lost),
                                theirs: None,
                            }),
                        },
                        None => edit.delete(at, raw(tick)),
                    }
                }
            }
        }
        if let Some(j) = j {
            add.insert(j);
        }
    }
    // Theirs' regions of no use of ours that lie in one.
    if let Some(w) = &owners {
        for &j in &add {
            if in_place.contains_key(&j) || in_use.contains_key(&j) {
                continue;
            }
            let (Some(at), Some(r)) = (ti.regions.get(j), tt.regions.get(j)) else {
                continue;
            };
            if let Some(u) = w.of_theirs(h.doc(), cx, at, raw(r.tick)) {
                in_use.insert(j, (u, None));
            }
        }
    }
    if let Some(specs) = theirs_region_specs(h, cx, (o, t), shift, true, |j| add.contains(&j))? {
        for (j, spec) in specs {
            if let Some(e) = in_place.get(&j) {
                edit.replace.push((*e, spec));
            } else if let Some((u, lost)) = in_use.remove(&j) {
                let Some(r) = tt.regions.get(j) else { continue };
                edit.inside.push(shared::Change {
                    use_ev: u,
                    ours: lost,
                    theirs: Some(shared::Gained {
                        tick: raw(r.tick),
                        key: shared::Key::Region(r.clone()),
                        spec,
                    }),
                });
            } else {
                edit.insert.push(spec);
            }
        }
    }
    // A use's change whose region theirs left out (no spec).
    for (_, (u, lost)) in in_use {
        if lost.is_some() {
            edit.inside.push(shared::Change {
                use_ev: u,
                ours: lost,
                theirs: None,
            });
        }
    }
    Ok(())
}

/// A curve's key as the plan gives it: its strip as a row of
/// `matches.strips` (`side` 0 theirs, 1 ours).
fn curve_key(cx: &Cx<'_>, side: usize, target: &str) -> String {
    let mut words: Vec<String> = target.split(' ').map(str::to_owned).collect();
    let row = words
        .get(1)
        .and_then(|c| c.parse::<usize>().ok())
        .and_then(|c| cx.srow.get(side)?.get(c).copied().flatten());
    if let (Some(k), Some(w)) = (row, words.get_mut(1)) {
        *w = k.to_string();
    }
    words.join(" ")
}

/// Theirs' scratch automation target as ours'.
fn ours_target(doc: &Document, cx: &Cx<'_>, target: AutomationTarget) -> Option<AutomationTarget> {
    let strip = |s: StripId| {
        cx.theirs_strip_index(s)
            .and_then(|t| cx.ours_strip_for_theirs(t))
    };
    Some(match target {
        AutomationTarget::StripFader(s) => AutomationTarget::StripFader(strip(s)?),
        AutomationTarget::StripPan(s) => AutomationTarget::StripPan(strip(s)?),
        AutomationTarget::StripMute(s) => AutomationTarget::StripMute(strip(s)?),
        AutomationTarget::SendLevel(s, i) => {
            let to = cx
                .th
                .doc
                .mixer()
                .strip(s)
                .ok()?
                .sends
                .get(usize::from(i.0))?
                .target;
            let (ours, ours_to) = (strip(s)?, strip(to)?);
            let k = doc
                .mixer()
                .strip(ours)
                .ok()?
                .sends
                .iter()
                .position(|x| x.target == ours_to)?;
            AutomationTarget::SendLevel(ours, SendIndex(u16::try_from(k).ok()?))
        }
        AutomationTarget::PluginParam(slot, q) => {
            let (t, d) = cx
                .th
                .identity
                .strips
                .iter()
                .enumerate()
                .find_map(|(t, s)| Some((t, s.devices.iter().position(|x| *x == slot.0)?)))?;
            let o = cx.m_strip(cx.theirs_row(t)?)?.ours?;
            let ours = cx.ids.strips.get(o)?.devices.get(d)?;
            AutomationTarget::PluginParam(PluginInstanceId(*ours), q)
        }
        _ => return None,
    })
}

/// Ruling 5: one curve, whole: ours' points deleted and theirs inserted
/// (with the hold points the projection leaves out: they shape it).
fn edit_curve(
    doc: &Document,
    cx: &Cx<'_>,
    edit: &mut LaneEdit,
    key: &str,
    (o, t): (usize, usize),
    shift: i64,
    out: &mut Out,
) {
    if let (Some(ot), Some(oi)) = (cx.raw.tracks.get(o), cx.ids.tracks.get(o)) {
        for ((target, points), ats) in ot.automation.iter().zip(&oi.automation) {
            if curve_key(cx, 1, target) == key {
                for (p, at) in points.iter().zip(ats) {
                    edit.delete(at, p.0);
                }
            }
        }
    }
    let (Some(tt), Some(ti)) = (cx.th.projection.tracks.get(t), cx.th.identity.tracks.get(t))
    else {
        return;
    };
    for ((target, _), ats) in tt.automation.iter().zip(&ti.automation) {
        if curve_key(cx, 0, target) != key {
            continue;
        }
        let Some(first) = ats.first() else { continue };
        let Ok((e, loc)) = cx.th.doc.event(EventId(first.event)) else {
            continue;
        };
        let EventBody::Automation { target: theirs, .. } = e.body else {
            continue;
        };
        let Some(mapped) = ours_target(doc, cx, theirs) else {
            out.report.add(ItemKind::LeftOut, UNPLACED, None);
            continue;
        };
        let Ok(lane) = cx.th.doc.lane(loc.seq, loc.lane) else {
            continue;
        };
        for e in lane.active().into_iter().flat_map(|tk| tk.events.values()) {
            if let EventBody::Automation { target, value } = e.body
                && target == theirs
            {
                edit.insert.push(EventSpec {
                    tick: cx.to_ours(e.tick.0, shift),
                    dest: None,
                    group_channel: None,
                    mute: false,
                    body: EventBody::Automation {
                        target: mapped,
                        value,
                    },
                });
            }
        }
    }
}

/// Carries out one lane's edit: the uses it unrolls become what they play
/// (less what it deletes), then one delete and one insert.
fn apply_edit(
    h: &mut History,
    cx: &Cx<'_>,
    lane: u64,
    mut edit: LaneEdit,
    out: &mut Out,
) -> Result<(), ModelError> {
    let shift = play_shift(h.doc(), cx.seq, lane);
    if !edit.unroll.is_empty() {
        let specs = unrolled(h.doc(), cx, lane, &edit, shift);
        edit.insert.extend(specs);
        for u in &edit.unroll {
            edit.delete.insert(EventId(*u));
        }
        if let Ok(l) = h.doc().lane(cx.seq, LaneId(lane)) {
            for u in &edit.unroll {
                let used = match h.doc().event(EventId(*u)).map(|(e, _)| &e.body) {
                    Ok(EventBody::Reference { seq, .. }) => h
                        .doc()
                        .sequence(*seq)
                        .map(|s| s.name.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                let what = if edit.looped.contains(u) {
                    text::LOOPS_UNROLLED
                } else {
                    text::USES_AS_NOTES
                };
                out.unrolled
                    .push((what, format!("Track '{}': use of '{used}'", l.name)));
            }
        }
    }
    if !edit.delete.is_empty() {
        run(
            h,
            Command::Event(EventCmd::Delete {
                events: edit.delete.into_iter().collect(),
            }),
        )?;
    }
    // Regions edited in place: partners first, crossfaded ones after, so
    // each crossfade is checked against its partner's new place; then the
    // inserts; then any crossfade an intermediate step settled is set
    // again, now that every region is where theirs has it.
    let crossfaded = |s: &EventSpec| {
        matches!(
            s.body,
            EventBody::AudioRegion {
                crossfade: true,
                ..
            }
        )
    };
    edit.replace
        .sort_by_key(|(e, s)| (crossfaded(s), s.tick, *e));
    for (event, spec) in &edit.replace {
        run(
            h,
            Command::Event(EventCmd::Replace {
                event: *event,
                spec: spec.clone(),
            }),
        )?;
    }
    let replaced = std::mem::take(&mut edit.replace);
    out.flagged.extend(
        replaced
            .iter()
            .filter(|(_, s)| crossfaded(s))
            .map(|(e, s)| (*e, s.clone())),
    );
    if !edit.insert.is_empty() {
        let r = run(
            h,
            Command::Event(EventCmd::Insert {
                seq: cx.seq,
                lane: LaneId(lane),
                events: edit.insert.clone(),
            }),
        )?;
        // A new region crossfading into one of ours that stays loses its
        // flag (the model's rule for a lone copy, plan 6c); theirs has the
        // crossfade, so [`reflag`] sets it again (Task 7).
        flag_made(&r, &edit.insert, &mut out.flagged)?;
    }
    for (event, spec) in replaced {
        let now = h.doc().event(event).ok().map(|(e, _)| {
            (
                e.tick == spec.tick,
                e.mute == spec.mute,
                e.body == spec.body,
            )
        });
        if now.is_some_and(|x| x != (true, true, true)) {
            run(h, Command::Event(EventCmd::Replace { event, spec }))?;
        }
    }
    Ok(())
}

/// Records each crossfaded region of `specs` that the insert `r` made, for
/// [`reflag`]. The insert makes one event per spec, in order; anything
/// else is refused rather than skipped (Task 7 fix round 1).
fn flag_made(
    r: &CmdReport,
    specs: &[EventSpec],
    flagged: &mut Vec<(EventId, EventSpec)>,
) -> Result<(), ModelError> {
    let made: Vec<EventId> = r
        .created
        .iter()
        .filter_map(|i| match i {
            AnyId::Event(e) => Some(*e),
            _ => None,
        })
        .collect();
    if made.len() != specs.len() {
        return Err(ModelError::invalid(
            "the merge's insert made a different number of events",
        ));
    }
    for (event, spec) in made.into_iter().zip(specs) {
        if matches!(
            spec.body,
            EventBody::AudioRegion {
                crossfade: true,
                ..
            }
        ) {
            flagged.push((event, spec.clone()));
        }
    }
    Ok(())
}

/// Sets again each of `flagged`'s crossfades a later command settled away
/// (a region placed before the tempo map it was cut for, Task 7).
fn reflag(h: &mut History, flagged: &[(EventId, EventSpec)]) -> Result<(), ModelError> {
    let fade = |b: &EventBody| match b {
        EventBody::AudioRegion {
            crossfade: true,
            fade_in,
            ..
        } => Some(fade_in.length_frames),
        _ => None,
    };
    for (event, spec) in flagged {
        // Its flag cleared, or its fade in shortened, under an earlier
        // step's tempo map (Task 7 fix round 2): set again as theirs has
        // it; the model keeps what the final layout can hold.
        let want = fade(&spec.body);
        let lost = h
            .doc()
            .event(*event)
            .is_ok_and(|(e, _)| match (fade(&e.body), want) {
                (None, _) => true,
                (Some(now), Some(w)) => now < w,
                (Some(_), None) => false,
            });
        if lost {
            run(
                h,
                Command::Event(EventCmd::Replace {
                    event: *event,
                    spec: spec.clone(),
                }),
            )?;
        }
    }
    Ok(())
}

/// What `edit`'s unrolled uses of `lane` play, as top-level events, less
/// the played events `edit` deletes.
fn unrolled(doc: &Document, cx: &Cx<'_>, lane: u64, edit: &LaneEdit, shift: i64) -> Vec<EventSpec> {
    let keep = |at: &EventAt, tick: i64| {
        edit.unroll.contains(&at.top) && !edit.gone_in_uses.contains(&(at.event, at.top, tick))
    };
    let spec = |at: &EventAt, tick: i64, pitch: Option<u8>| -> Option<EventSpec> {
        let (e, _) = doc.event(EventId(at.event)).ok()?;
        let mut body = e.body.clone();
        if let (Some(p), EventBody::Note { pitch: q, .. }) = (pitch, &mut body) {
            *q = p;
        }
        Some(EventSpec {
            tick: Tick(tick.saturating_sub(shift)),
            dest: e.dest,
            group_channel: e.group_channel,
            mute: false,
            body,
        })
    };
    let mut out = Vec::new();
    for o in lane_rows(cx, lane) {
        let (Some(t), Some(ids)) = (cx.raw.tracks.get(o), cx.ids.tracks.get(o)) else {
            continue;
        };
        for (n, at) in t.notes_played.iter().zip(&ids.notes) {
            if keep(at, n.tick) {
                out.extend(spec(at, n.tick, Some(n.pitch)));
            }
        }
        for (m, at) in t.midi.iter().zip(&ids.midi) {
            if keep(at, m.0) {
                out.extend(spec(at, m.0, None));
            }
        }
        for (r, at) in t.regions.iter().zip(&ids.regions) {
            if keep(at, r.tick) {
                out.extend(spec(at, r.tick, None));
            }
        }
        for ((_, points), ats) in t.automation.iter().zip(&ids.automation) {
            for (p, at) in points.iter().zip(ats) {
                if keep(at, p.0) {
                    out.extend(spec(at, p.0, None));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- takes

/// Whether `events` (a take's) are `specs`, ticks within `NOTE_TICKS`.
fn same_events(events: &[EventSpec], specs: &[EventSpec]) -> bool {
    if events.len() != specs.len() {
        return false;
    }
    let mut free: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, e) in events.iter().enumerate() {
        free.entry(e.tick.0).or_default().push(i);
    }
    let alike = |e: &EventSpec, s: &EventSpec| {
        e.body == s.body && e.group_channel == s.group_channel && e.mute == s.mute
    };
    for s in specs {
        let t = s.tick.0;
        let range = t.saturating_sub(super::NOTE_TICKS)..=t.saturating_add(super::NOTE_TICKS);
        let found = free.range_mut(range).find_map(|(_, v)| {
            let p = v
                .iter()
                .position(|&i| events.get(i).is_some_and(|e| alike(e, s)))?;
            Some(v.remove(p))
        });
        if found.is_none() {
            return false;
        }
    }
    true
}

/// Ruling 6: theirs' version of `lane`'s conflicting parts as an
/// alternate take "Returned <date>" (" (2)", … when that name is taken),
/// ours' take staying active; none when an inactive take already holds
/// it (its name is listed instead).
fn conflict_take(
    h: &mut History,
    cx: &Cx<'_>,
    lane: u64,
    parts: &[(usize, Unit)],
    date: &str,
    out: &mut Out,
) -> Result<(), ModelError> {
    let shift = play_shift(h.doc(), cx.seq, lane);
    let theirs_of = |o: usize| -> Option<usize> {
        cx.plan
            .matches
            .tracks
            .iter()
            .find(|r| r.ours == Some(o))
            .and_then(|r| r.theirs)
    };
    let build = |h: &mut History, add: bool| -> Result<Option<Vec<EventSpec>>, ModelError> {
        let mut specs = Vec::new();
        for (o, unit) in parts {
            let Some(t) = theirs_of(*o) else { continue };
            match unit {
                Unit::Notes => {
                    specs.extend(theirs_notes(cx, t, shift, |_| true));
                    specs.extend(theirs_midi(cx, t, shift, |_| true));
                }
                Unit::Regions => match theirs_regions(h, cx, (*o, t), shift, add, |_| true)? {
                    Some(r) => specs.extend(r),
                    None => return Ok(None),
                },
                _ => {}
            }
        }
        Ok(Some(specs))
    };
    let Ok(l) = h.doc().lane(cx.seq, LaneId(lane)) else {
        return Ok(());
    };
    let track = l.name.clone();
    let active = l.active_take;
    // An inactive take that already holds theirs' version (without adding
    // media: a version with media ours lacks is new).
    if let Some(specs) = build(h, false)?
        && let Ok(l) = h.doc().lane(cx.seq, LaneId(lane))
    {
        let found = l.takes.iter().filter(|tk| tk.id != active).find(|tk| {
            let events: Vec<EventSpec> = tk
                .events
                .values()
                .map(|e| EventSpec {
                    tick: e.tick,
                    dest: e.dest,
                    group_channel: e.group_channel,
                    mute: e.mute,
                    body: e.body.clone(),
                })
                .collect();
            same_events(&events, &specs)
        });
        if let Some(tk) = found {
            out.takes.push(text::conflict_take(&track, &tk.name));
            return Ok(());
        }
    }
    let Some(specs) = build(h, true)? else {
        return Ok(());
    };
    let name = take_name(h.doc(), cx.seq, lane, date);
    let (seq, lane) = (cx.seq, LaneId(lane));
    run(
        h,
        Command::Lane(LaneCmd::AddTake {
            seq,
            lane,
            name: name.clone(),
        }),
    )?;
    if !specs.is_empty() {
        let r = run(
            h,
            Command::Event(EventCmd::Insert {
                seq,
                lane,
                events: specs.clone(),
            }),
        )?;
        // Theirs' crossfades in the take, set again once the tempo map is
        // theirs ([`reflag`]).
        flag_made(&r, &specs, &mut out.flagged)?;
    }
    run(
        h,
        Command::Lane(LaneCmd::SetActiveTake {
            seq,
            lane,
            take: active,
        }),
    )?;
    out.takes.push(text::conflict_take(&track, &name));
    Ok(())
}

/// "Returned <date>", or with " (2)", " (3)", … when the track has a take
/// of that name.
fn take_name(doc: &Document, seq: SeqId, lane: u64, date: &str) -> String {
    let base = format!("Returned {date}");
    let taken: BTreeSet<String> = doc
        .lane(seq, LaneId(lane))
        .map(|l| l.takes.iter().map(|t| t.name.clone()).collect())
        .unwrap_or_default();
    if !taken.contains(&base) {
        return base;
    }
    (2u64..)
        .map(|n| format!("{base} ({n})"))
        .find(|n| !taken.contains(n))
        .unwrap_or(base)
}

// ---------------------------------------------------------------- strips

/// Step 4: each strip unit theirs changed. A strip whose device list
/// theirs changed has new slots: its bypass and state units then name the
/// slots `devices` left (Task 6 fix round 1, I2).
fn strips(h: &mut History, cx: &Cx<'_>) -> Result<(), ModelError> {
    // Ours' strip `o`'s device slots after a `Devices` change, in the
    // projection's order.
    let mut slots: BTreeMap<usize, Vec<u64>> = BTreeMap::new();
    for d in &cx.plan.decisions {
        cx.stop()?;
        // A `DeletedHere` output (theirs aimed at a bus ours deleted) runs
        // nothing: ours' output stays as it is (VizDAW wins).
        if d.outcome != Outcome::Theirs {
            continue;
        }
        let Some(r) = cx.m_strip(d.row).copied() else {
            continue;
        };
        let (Some(t), Some(o)) = (r.theirs, r.ours) else {
            continue;
        };
        let (Some(strip), Some(ts)) = (cx.ours_strip_at(o), cx.th.projection.strips.get(t)) else {
            continue;
        };
        let slot_of = |i: usize| -> Option<u64> {
            match slots.get(&o) {
                Some(v) => v.get(i).copied(),
                None => cx.ids.strips.get(o)?.devices.get(i).copied(),
            }
        };
        let cmd = match &d.unit {
            Unit::Strip(StripField::Level) => MixerCmd::SetGain {
                strip,
                gain_db: ts.gain_db,
            },
            Unit::Strip(StripField::Pan) => MixerCmd::SetPan { strip, pan: ts.pan },
            Unit::Strip(StripField::Mute) => MixerCmd::SetMute {
                strip,
                mute: ts.mute,
            },
            Unit::Strip(StripField::Solo) => MixerCmd::SetSolo {
                strip,
                solo: ts.solo,
            },
            Unit::Strip(StripField::Colour) => MixerCmd::SetColour {
                strip,
                colour: ts
                    .colour
                    .as_deref()
                    .and_then(nearest_swatch)
                    .map(|(s, _)| s),
            },
            Unit::Strip(StripField::Name) => MixerCmd::RenameStrip {
                strip,
                name: theirs_strip_name(cx, t).unwrap_or_else(|| ts.name.clone()),
            },
            Unit::Strip(StripField::Output) => MixerCmd::SetOutput {
                strip,
                output: match ts.output.and_then(|i| cx.ours_strip_for_theirs(i)) {
                    Some(to) if to != h.doc().mixer().master() => Output::Bus(to),
                    _ => Output::Master,
                },
            },
            Unit::Send(k) => {
                send(h, cx, strip, ts, *k)?;
                continue;
            }
            Unit::Devices => {
                let before = h.doc().mixer().strip(strip)?.clone();
                devices(h, cx, strip, t)?;
                let after = h.doc().mixer().strip(strip)?;
                let own = |s: &core_model::mixer::Strip| -> Vec<u64> {
                    s.eq.iter()
                        .chain(s.inserts.iter())
                        .map(|x| x.id.0)
                        .collect()
                };
                let old_own = own(&before);
                // The instrument (not the strip's own) stays; then the new
                // EQ slot and inserts.
                let mut now: Vec<u64> = cx
                    .ids
                    .strips
                    .get(o)
                    .map(|s| {
                        s.devices
                            .iter()
                            .filter(|id| !old_own.contains(id))
                            .copied()
                            .collect()
                    })
                    .unwrap_or_default();
                now.extend(own(after));
                slots.insert(o, now);
                continue;
            }
            Unit::Bypass(i) => {
                let (Some(slot), Some(dev)) = (slot_of(*i), ts.devices.get(*i)) else {
                    continue;
                };
                MixerCmd::SetBypass {
                    slot: PluginInstanceId(slot),
                    bypass: dev.2,
                }
            }
            Unit::State(i) => {
                let (Some(slot), Some(from)) = (
                    slot_of(*i),
                    cx.th.identity.strips.get(t).and_then(|s| s.devices.get(*i)),
                ) else {
                    continue;
                };
                // `decide` lists a state theirs dropped (`NOT_APPLIED`).
                let Some(bytes) = theirs_state(&cx.th.doc, PluginInstanceId(*from)) else {
                    continue;
                };
                MixerCmd::SetPluginState {
                    slot: PluginInstanceId(slot),
                    bytes,
                }
            }
            _ => continue,
        };
        run(h, Command::Mixer(cmd))?;
    }
    Ok(())
}

/// Theirs' name for theirs strip `t` when the import made it for an audio
/// track: the file's channel name.
fn theirs_strip_name(cx: &Cx<'_>, t: usize) -> Option<String> {
    let id = cx.th.identity.strips.get(t)?.strip?;
    cx.th.origins.channel_names.get(&StripId(id)).cloned()
}

fn theirs_state(doc: &Document, slot: PluginInstanceId) -> Option<Arc<[u8]>> {
    doc.state_arc(doc.plugin_slot(slot)?.state?)
}

/// Theirs' send to target row `k`, on ours' `strip`.
fn send(
    h: &mut History,
    cx: &Cx<'_>,
    strip: StripId,
    ts: &PStrip,
    k: usize,
) -> Result<(), ModelError> {
    let Some(target) = cx.ours_strip_of_row(k) else {
        return Ok(());
    };
    let theirs = ts.sends.iter().find(|s| cx.theirs_row(s.0) == Some(k));
    let index = h
        .doc()
        .mixer()
        .strip(strip)?
        .sends
        .iter()
        .position(|s| s.target == target);
    let aux = |&(_, level_db, pre_fader, enabled): &(usize, f64, bool, bool)| AuxSend {
        target,
        level_db,
        pre_fader,
        enabled,
    };
    let cmd = match (theirs, index) {
        (Some(s), Some(index)) => MixerCmd::EditSend {
            strip,
            index,
            send: aux(s),
        },
        (Some(s), None) => MixerCmd::AddSend {
            strip,
            send: aux(s),
        },
        (None, Some(index)) => MixerCmd::RemoveSend { strip, index },
        (None, None) => return Ok(()),
    };
    run(h, Command::Mixer(cmd))?;
    Ok(())
}

fn same_plugin(a: &PluginRef, b: &PluginRef) -> bool {
    a.format == b.format && a.id == b.id
}

/// Theirs' device list on ours' `strip` (Ruling 5): the EQ slot, then the
/// inserts from the first that differs, each new slot with theirs' bypass
/// and state (`SetPluginState`). A track's instrument belongs to its
/// destination and is not changed here.
fn devices(h: &mut History, cx: &Cx<'_>, strip: StripId, t: usize) -> Result<(), ModelError> {
    let Some(from) = cx.th.identity.strips.get(t).and_then(|s| s.strip) else {
        return Ok(());
    };
    let Ok(ts) = cx.th.doc.mixer().strip(StripId(from)) else {
        return Ok(());
    };
    let settle = |h: &mut History, r: &CmdReport, from: PluginInstanceId| {
        let Some(slot) = r.created.iter().find_map(|id| match id {
            AnyId::Plugin(p) => Some(*p),
            _ => None,
        }) else {
            return Ok::<(), ModelError>(());
        };
        if cx.th.doc.plugin_slot(from).is_some_and(|s| s.bypass) {
            run(
                h,
                Command::Mixer(MixerCmd::SetBypass { slot, bypass: true }),
            )?;
        }
        if let Some(bytes) = theirs_state(&cx.th.doc, from) {
            run(h, Command::Mixer(MixerCmd::SetPluginState { slot, bytes }))?;
        }
        Ok(())
    };
    let ours = h.doc().mixer().strip(strip)?.clone();
    let eq_same = match (&ours.eq, &ts.eq) {
        (Some(a), Some(b)) => same_plugin(&a.plugin, &b.plugin),
        (None, None) => true,
        _ => false,
    };
    if !eq_same {
        let r = run(
            h,
            Command::Mixer(MixerCmd::SetEq {
                strip,
                plugin: ts.eq.as_ref().map(|s| s.plugin.clone()),
            }),
        )?;
        if let Some(e) = &ts.eq {
            settle(h, &r, e.id)?;
        }
    }
    let common = ours
        .inserts
        .iter()
        .zip(ts.inserts.iter())
        .take_while(|(a, b)| same_plugin(&a.plugin, &b.plugin))
        .count();
    for s in ours.inserts.iter().skip(common) {
        run(h, Command::Mixer(MixerCmd::RemoveInsert { slot: s.id }))?;
    }
    for s in ts.inserts.iter().skip(common) {
        let r = run(
            h,
            Command::Mixer(MixerCmd::AddInsert {
                strip,
                index: None,
                plugin: s.plugin.clone(),
            }),
        )?;
        settle(h, &r, s.id)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- markers and maps

/// Step 5: the merged marker list, in one `SetMarkers`.
fn markers(h: &mut History, cx: &Cx<'_>) -> Result<(), ModelError> {
    let mut theirs_for: BTreeMap<usize, usize> = BTreeMap::new();
    let mut removed = BTreeSet::new();
    let mut added = Vec::new();
    for d in &cx.plan.decisions {
        if d.unit != Unit::Marker {
            continue;
        }
        let Some(r) = cx.plan.matches.markers.get(d.row) else {
            continue;
        };
        match (d.outcome, r.theirs, r.ours) {
            (Outcome::Theirs, Some(t), Some(o)) => {
                theirs_for.insert(o, t);
            }
            (Outcome::Removed, _, Some(o)) => {
                removed.insert(o);
            }
            (Outcome::Added, Some(t), None) => added.push(t),
            _ => {}
        }
    }
    if theirs_for.is_empty() && removed.is_empty() && added.is_empty() {
        return Ok(());
    }
    let theirs = |t: usize| {
        cx.th.projection.markers.get(t).map(|(tick, name)| Marker {
            tick: cx.to_ours(*tick, 0),
            name: name.clone(),
        })
    };
    let mut list: Vec<Marker> = Vec::new();
    for (o, m) in h.doc().sequence(cx.seq)?.markers.iter().enumerate() {
        if removed.contains(&o) {
            continue;
        }
        match theirs_for.get(&o) {
            Some(t) => list.extend(theirs(*t)),
            None => list.push(m.clone()),
        }
    }
    list.extend(added.into_iter().filter_map(theirs));
    list.sort_by_key(|m| m.tick);
    run(
        h,
        Command::Sequence(SequenceCmd::SetMarkers {
            seq: cx.seq,
            markers: list.into_iter().collect(),
        }),
    )?;
    Ok(())
}

/// Step 5: theirs' tempo and meter maps, whole (the key signatures stay
/// ours'), and the song-info fields theirs changed, in one `SetInfo`.
fn maps(h: &mut History, cx: &Cx<'_>) -> Result<(), ModelError> {
    let theirs = |u: &Unit| {
        cx.plan
            .decisions
            .iter()
            .any(|d| &d.unit == u && d.outcome == Outcome::Theirs)
    };
    if theirs(&Unit::Tempo) {
        let points = cx
            .th
            .projection
            .tempo
            .iter()
            .map(|&(tick, bpm, ramp)| TempoPoint {
                tick: cx.to_ours(tick, 0),
                bpm: Bpm(bpm),
                ramp,
            })
            .collect();
        let tempo = TempoMap::new(points)?;
        run(
            h,
            Command::Sequence(SequenceCmd::SetTempo { seq: cx.seq, tempo }),
        )?;
    }
    if theirs(&Unit::Meter) {
        let ours = h.doc().sequence(cx.seq)?.meter.clone();
        let entries = cx
            .th
            .projection
            .meter
            .iter()
            .map(|&(tick, num, den)| {
                let tick = cx.to_ours(tick, 0);
                let key = ours
                    .entries()
                    .iter()
                    .rev()
                    .find(|e| e.tick <= tick)
                    .or_else(|| ours.entries().front())
                    .map(|e| e.key)
                    .unwrap_or_default();
                MeterEntry {
                    tick,
                    meter: Meter { num, den },
                    key,
                }
            })
            .collect();
        let meter = MeterMap::new(entries)?;
        run(
            h,
            Command::Sequence(SequenceCmd::SetMeter { seq: cx.seq, meter }),
        )?;
    }
    let fields: Vec<usize> = cx
        .plan
        .decisions
        .iter()
        .filter(|d| d.outcome == Outcome::Theirs)
        .filter_map(|d| match d.unit {
            Unit::Info(i) => Some(i),
            _ => None,
        })
        .collect();
    if !fields.is_empty() {
        let mut info = h.doc().settings().info.clone();
        for i in fields {
            let v = cx.th.projection.info.get(i).cloned().flatten();
            if let Some(f) = info.fields_mut().into_iter().nth(i) {
                *f = v;
            }
        }
        run(h, Command::Settings(SettingsCmd::SetInfo { info }))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- deletions

/// Step 6 (Ruling 7): a track theirs deleted and ours did not change is
/// removed; one holding what the file cannot carry, or a split track's
/// part whose other part stays, is emptied of what the file carries. A
/// bus is removed.
fn deletions(h: &mut History, cx: &Cx<'_>) -> Result<(), ModelError> {
    let mut gone: Vec<(u64, Vec<(usize, Outcome)>)> = Vec::new();
    for d in &cx.plan.decisions {
        if d.unit != Unit::Track || !matches!(d.outcome, Outcome::Removed | Outcome::KeptEmpty) {
            continue;
        }
        let Some(o) = cx.m_track(d.row).and_then(|r| r.ours) else {
            continue;
        };
        let Some(lane) = cx.ids.tracks.get(o).map(|t| t.lane) else {
            continue;
        };
        match gone.iter_mut().find(|(l, _)| *l == lane) {
            Some((_, parts)) => parts.push((o, d.outcome)),
            None => gone.push((lane, vec![(o, d.outcome)])),
        }
    }
    for (lane, parts) in gone {
        cx.stop()?;
        let whole = parts.len() == lane_rows(cx, lane).len()
            && parts.iter().all(|(_, o)| *o == Outcome::Removed);
        if whole {
            run(
                h,
                Command::Lane(LaneCmd::Remove {
                    seq: cx.seq,
                    lane: LaneId(lane),
                }),
            )?;
            continue;
        }
        let events: BTreeSet<EventId> = parts
            .iter()
            .filter_map(|(o, _)| cx.ids.tracks.get(*o))
            .flat_map(carried)
            .map(|at| EventId(at.top))
            .collect();
        if !events.is_empty() {
            run(
                h,
                Command::Event(EventCmd::Delete {
                    events: events.into_iter().collect(),
                }),
            )?;
        }
    }
    for d in &cx.plan.decisions {
        cx.stop()?;
        if d.unit == Unit::Bus
            && d.outcome == Outcome::Removed
            && let Some(strip) = cx
                .m_strip(d.row)
                .and_then(|r| r.ours)
                .and_then(|o| cx.ours_strip_at(o))
        {
            run(h, Command::Mixer(MixerCmd::RemoveBus { strip }))?;
        }
    }
    Ok(())
}

/// Every event a track part plays that the file carries.
fn carried(ids: &TrackIds) -> impl Iterator<Item = &EventAt> {
    ids.notes
        .iter()
        .chain(&ids.midi)
        .chain(&ids.regions)
        .chain(ids.automation.iter().flatten())
}
