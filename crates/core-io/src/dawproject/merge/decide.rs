//! Deciding every unit of a merge (plan 6e Rulings 5, 7, 10, 11 and 15):
//! the rule table of spec §7.8 per unit, the preview's counts and the
//! report's lines.
//!
//! **The rule table,** per unit, with "changed" meaning outside the
//! tolerances ([`super::NOTE_TICKS`] and friends):
//!
//! | base → theirs | base → ours | outcome |
//! |---|---|---|
//! | changed | unchanged | [`Outcome::Theirs`] |
//! | unchanged | changed | [`Outcome::Ours`] |
//! | changed | changed, the same way | [`Outcome::Same`] |
//! | changed | changed differently | [`Outcome::Conflict`] |
//! | deleted | unchanged | [`Outcome::Removed`], or [`Outcome::KeptEmpty`] for a track holding what the file cannot carry (Ruling 7) |
//! | deleted | changed | [`Outcome::DeletedThere`] |
//! | changed | deleted | [`Outcome::DeletedHere`] |
//! | unchanged | deleted | [`Outcome::Ours`] |
//! | deleted | deleted | [`Outcome::Same`] |
//! | new | — | [`Outcome::Added`], or, when ours has a new part of the same name (the same file merged before), [`Outcome::Same`] if they are equal and [`Outcome::Ours`] if not |
//! | — | new | [`Outcome::Ours`] |
//!
//! Units unchanged on both sides make no decision. Existence ("new",
//! "deleted") is decided for tracks, buses and markers; a track's strip
//! exists with its track.
//!
//! **What a unit is** (Ruling 5): a track's name, notes (with MIDI
//! events) and regions (compared without media hash, gain and file stem,
//! Ruling 11), and each automation curve (by target, the strip given as
//! its row in [`Matches::strips`]); a strip's level, pan, mute, solo,
//! colour, name and output, each send (by target), its device list
//! (formats and IDs, in order), and each device's bypass and state; each
//! marker; the tempo map; the meter map; each song-info field. The import
//! names an audio track's strip after the track, so theirs' name for it is
//! the file's channel name (`Origins::channel_names`).
//!
//! **Not applied** (Task 6 fix round 1), listed under
//! [`text::NOT_APPLIED`] as [`Outcome::Conflict`]s, so none counts as
//! applied:
//! - theirs' automation curves on a track that plays its own loop, its
//!   own length or with play quantize (the reason is in the detail): the
//!   export's tick transform has no inverse. Theirs' notes and regions on
//!   such a track are a take conflict instead ([`text::CONFLICT_TAKE`]);
//! - theirs' instrument swap ([`Unit::Instrument`]);
//! - a device state theirs dropped (no command clears one).
//!
//! A send or output theirs aims at a bus ours deleted is
//! [`Outcome::DeletedHere`]: the send is not made; a matched strip keeps
//! ours' output as it is, and a new track's or bus's plays into the
//! master (it has no output of ours).
//!
//! **Ruling 10.** A strip whose devices theirs changed while the file
//! named effects VizDAW cannot host (VST plug-ins, ones not installed)
//! keeps ours ([`Outcome::Ours`]), listed under [`text::VST_KEPT`].

use super::matching::{Matched, MatchedBy, Matches, match_all, notes, regions};
use super::target::{self, Holds, Missing};
use super::text;
use super::theirs::Theirs;
use super::{LEVEL_DB, NOTE_TICKS, PAN, VALUE, normalised};
use crate::IoError;
use crate::dawproject::import::{Origins, VST};
use crate::dawproject::record::{
    EventAt, Identity, PNote, PRegion, PStrip, PTrack, Projection, Snapshot, StripIds, TrackIds,
    projection_with_identity, returned,
};
use crate::dawproject::units::nearest_swatch;
use crate::report::{ItemKind, Report};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{EventId, LaneId, SeqId, StripId};
use core_model::lane::Lane;
use core_model::time::PPQ;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------- the API

/// What is compared and replaced as one thing (Ruling 5).
#[derive(Debug, Clone, PartialEq)]
pub enum Unit {
    /// A track's existence (row of [`Matches::tracks`]).
    Track,
    /// A track's name.
    TrackName,
    /// A track part's notes and MIDI events, as a set.
    Notes,
    /// A track part's regions, as a set.
    Regions,
    /// One field of a strip (row of [`Matches::strips`]).
    Strip(StripField),
    /// A strip's send to a target, the target's row in [`Matches::strips`].
    Send(usize),
    /// A strip's device list after its track's instrument: formats and
    /// IDs, in order.
    Devices,
    /// A track strip's instrument (format and ID). The merge never changes
    /// it: theirs' swap is listed under [`text::NOT_APPLIED`].
    Instrument,
    /// One device's bypass, by its place in base's list.
    Bypass(usize),
    /// One device's state, by its place in base's list.
    State(usize),
    /// One automation curve of a track: its target, with the strip given
    /// as its row in [`Matches::strips`] (`"fader 0"`, `"param 0 1 7"`).
    Automation(String),
    /// A marker's existence, name and tick (row of [`Matches::markers`]).
    Marker,
    /// The tempo map, whole.
    Tempo,
    /// The meter map, whole.
    Meter,
    /// One song-info field, by its index in `SongInfo::fields`.
    Info(usize),
    /// A bus's existence (row of [`Matches::strips`]).
    Bus,
}

/// A strip's fields that are units of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripField {
    /// Its fader.
    Level,
    /// Its pan.
    Pan,
    /// Its mute.
    Mute,
    /// Its solo.
    Solo,
    /// Its colour.
    Colour,
    /// Its name.
    Name,
    /// The strip it plays into.
    Output,
}

/// The rule table's result for one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Theirs changed it, ours did not: theirs is applied.
    Theirs,
    /// Ours changed it (or deleted or added it), theirs did not: ours is
    /// kept.
    Ours,
    /// Both changed it the same way: applied once (nothing to do).
    Same,
    /// Both changed it differently: ours is kept, theirs is listed or
    /// becomes an alternate take.
    Conflict,
    /// Theirs deleted it, ours did not change it: removed.
    Removed,
    /// Theirs deleted a track that holds what the file cannot carry: kept,
    /// emptied of what the file carries (Ruling 7).
    KeptEmpty,
    /// Theirs deleted it, ours changed it: kept.
    DeletedThere,
    /// Ours deleted it, theirs changed it: left deleted.
    DeletedHere,
    /// New in theirs: added, as an import would add it.
    Added,
}

/// One unit's decision.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// The unit.
    pub unit: Unit,
    /// The unit's row: in [`Matches::tracks`] for track units, in
    /// [`Matches::strips`] for strip and bus units, in
    /// [`Matches::markers`] for markers, the field for song info, 0 for
    /// the tempo and meter maps.
    pub row: usize,
    /// What the rule table says.
    pub outcome: Outcome,
    /// "Track 'Bass': notes", "Strip 'Bass': level", "Tempo map", "Song info: title".
    pub what: String,
    /// Ours' value as the app shows it ("−6.0 dB"), when ours has one.
    pub ours: Option<String>,
    /// Theirs' value as the app shows it, when theirs has one.
    pub theirs: Option<String>,
}

/// The preview's counts (Ruling 15).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergePreview {
    /// [`Outcome::Theirs`] and [`Outcome::Same`].
    pub applied: u32,
    /// [`Outcome::Ours`].
    pub kept: u32,
    /// [`Outcome::Added`].
    pub added: u32,
    /// [`Outcome::Removed`].
    pub removed: u32,
    /// [`Outcome::Conflict`], [`Outcome::KeptEmpty`],
    /// [`Outcome::DeletedThere`] and [`Outcome::DeletedHere`].
    pub conflicts: u32,
    /// One line per conflict, in decision order: a setting's
    /// [`text::conflict_setting`] detail, else the decision's `what`.
    pub conflict_lines: Vec<String>,
}

/// A planned merge: the three sides, how their rows match, every decision,
/// the preview and the report so far.
pub struct MergePlan {
    /// The snapshot (base).
    pub base: Snapshot,
    /// The returning file as a project of its own.
    pub theirs: Theirs,
    /// Every row of the three sides, matched.
    pub matches: Matches,
    /// One per unit changed on either side, in order: tracks, strips,
    /// markers, the tempo map, the meter map, song info.
    pub decisions: Vec<Decision>,
    /// The counts and conflict lines.
    pub preview: MergePreview,
    /// The import's lines (without `import::FROM_VIZDAW`) and the merge's
    /// own: [`text::MERGED`], the matching notes, the settings in conflict,
    /// the deletions and the plug-ins kept, and (from a dry run of the
    /// apply, Ruling 8) the private copies, the uses unrolled and the edits
    /// that play elsewhere. The alternate takes are listed when the merge
    /// is applied.
    pub report: Report,
}

/// Decides every unit (Rulings 5, 7, 11). Units unchanged on both sides
/// make no decision. `chosen`: the user chose the snapshot (the file lost
/// its export ID), so the report notes [`text::SNAPSHOT_CHOSEN`].
/// `Refused(SEQUENCE_GONE)` when `ours` no longer has the snapshot's
/// sequence.
pub fn plan(
    base: Snapshot,
    theirs: Theirs,
    ours: &Document,
    chosen: bool,
    cancel: &AtomicBool,
) -> Result<MergePlan, IoError> {
    stop(cancel)?;
    let seq = SeqId(base.seq);
    if ours.sequence(seq).is_err() {
        return Err(IoError::Refused(text::SEQUENCE_GONE.to_owned()));
    }
    let ours_side = projection_with_identity(ours, seq)?;
    let b = normalised(&base.projection, &base);
    let o = normalised(&ours_side.0, &base);
    // Matching reads no region, so it runs on theirs as imported; `held`
    // then needs it to tell which of theirs' tracks have regions as base's.
    let mut matches = match_all(&base, &theirs, &ours_side, cancel)?;
    stop(cancel)?;
    pair_twins(&mut matches, &theirs.projection, &o);
    let base_ids = base_track_ids(&b, base.identity.as_ref());
    let untouched = untouched_regions(&matches, (&b, &base_ids), &theirs);
    let (mut theirs, settled) = held(theirs, &b, &o, ours, seq, base.origin, &untouched)?;
    let doubled = rehome_curves(&mut theirs, &matches);
    stop(cancel)?;
    let ours_origins = self_origins(&ours_side.1);
    let mut d = Decider {
        p: [&b, &theirs.projection, &o],
        ids: [&base_ids, &theirs.identity.tracks, &ours_side.1.tracks],
        theirs_origins: &theirs.origins,
        ours_origins: &ours_origins,
        theirs_strips: &theirs.identity,
        ours_strips: &ours_side.1,
        base_strips: base.identity.as_ref(),
        theirs_doc: &theirs.doc,
        not_applied: BTreeSet::new(),
        plain: BTreeSet::new(),
        m: &matches,
        skey: strip_keys(
            &matches,
            [
                b.strips.len(),
                theirs.projection.strips.len(),
                o.strips.len(),
            ],
        ),
        tstrips: [
            track_strips(&b),
            track_strips(&theirs.projection),
            track_strips(&o),
        ],
        audio_paths: theirs
            .projection
            .tracks
            .iter()
            .filter(|t| t.audio)
            .filter_map(|t| t.strip)
            .collect(),
        ours_doc: ours,
        seq,
        elsewhere: used_elsewhere(ours, seq),
        strip_labels: Vec::new(),
        out: Vec::new(),
        vst: Vec::new(),
    };
    d.strip_labels = d.labels();
    for row in 0..matches.tracks.len() {
        stop(cancel)?;
        d.track(row);
        let Some(t) = matches.tracks.get(row).and_then(|r| r.theirs) else {
            continue;
        };
        for (_, what) in doubled.iter().filter(|(a, _)| *a == t) {
            d.plain.insert(d.out.len());
            let unit = Unit::Automation(what.clone());
            d.push_not_applied(unit, row, what.clone(), None, None);
        }
    }
    stop(cancel)?;
    for row in 0..matches.strips.len() {
        d.strip(row);
    }
    d.unplaced_curves();
    stop(cancel)?;
    for row in 0..matches.markers.len() {
        d.marker(row);
    }
    d.maps();
    d.info();
    stop(cancel)?;

    let mut report = theirs.report.clone();
    report.add(ItemKind::Note, &text::merged(base.exported_at), None);
    if base.identity.is_none() {
        report.add(ItemKind::Note, text::OLD_EXPORT, None);
    } else if matches.heuristic {
        report.add(ItemKind::Note, text::BY_NAME, None);
    }
    if chosen {
        report.add(ItemKind::Note, text::SNAPSHOT_CHOSEN, None);
    }
    let (preview, take_parts) = d.listed(&mut report);
    for lane in take_parts {
        report.add(ItemKind::Note, text::TAKE_PARTS, Some(lane));
    }
    for (detail, vst) in std::mem::take(&mut d.vst) {
        report.add(ItemKind::LeftOut, text::VST_KEPT, Some(detail));
        // `VST_KEPT` lists it; the import's own line need not.
        if let Some(name) = vst {
            drop_detail(&mut report, ItemKind::LeftOut, VST, &name);
        }
    }
    let decisions = d.out;
    held_lines(&settled, &theirs, &matches, &decisions, &mut report);
    let mut plan = MergePlan {
        base,
        theirs,
        matches,
        decisions,
        preview,
        report,
    };
    stop(cancel)?;
    let lines = dry_run(&plan, ours, cancel);
    stop(cancel)?;
    plan.report.merge(lines);
    stop(cancel)?;
    Ok(plan)
}

/// Theirs as the model will hold it in the merged layout (Task 7 fix
/// round 2). When the tempo map the merge leaves is ours' (theirs did not
/// change it, or both did and ours is kept) and differs from theirs',
/// theirs' crossfaded pairs may overlap less there, or not at all: the
/// model's crossfade rules (plan 6c) then shorten their fades in or clear
/// their flags as it places them. So theirs' scratch project is settled
/// under ours' map (in theirs' frame: ticks less `origin`), then given its
/// own map back (a settled fade is never lengthened), and its regions are
/// projected again; every decision then judges theirs' regions as they will
/// be, so the same file merged again finds them the same. Answers the
/// model's settle lines, by region.
///
/// Only tracks whose regions theirs changed are held (Task 7 fix round 4,
/// N1): a track of `untouched` (theirs' regions as imported are base's)
/// keeps them as imported, in the projection and in the scratch project,
/// so ours' tempo never makes theirs look edited. If the settled project
/// cannot be lined up with theirs (a track or region count differs, or an
/// untouched track cannot be given back), theirs is answered unchanged:
/// the projection and the scratch project change together or not at all.
fn held(
    theirs: Theirs,
    b: &Projection,
    o: &Projection,
    ours: &Document,
    seq: SeqId,
    origin: i64,
    untouched: &BTreeSet<usize>,
) -> Result<(Theirs, Vec<(EventId, String)>), IoError> {
    use core_model::command::{Command, EventCmd, SequenceCmd};
    use core_model::event::EventSpec;
    use core_model::history::History;
    use core_model::selection::ViewFilter;
    use core_model::tempo::{Bpm, TempoMap, TempoPoint};
    use core_model::time::Tick;
    let t = &theirs.projection.tempo;
    let theirs_lands = !tempo_eq(&b.tempo, t) && tempo_eq(&b.tempo, &o.tempo);
    let crossfades = theirs
        .projection
        .tracks
        .iter()
        .enumerate()
        .any(|(i, x)| !untouched.contains(&i) && x.regions.iter().any(|r| r.crossfade));
    if theirs_lands || tempo_eq(&o.tempo, t) || !crossfades {
        return Ok((theirs, Vec::new()));
    }
    let refused = |e: core_model::error::ModelError| IoError::Refused(e.to_string());
    let map = &ours.sequence(seq).map_err(refused)?.tempo;
    let mut points: Vec<TempoPoint> = map
        .points()
        .iter()
        .filter(|p| p.tick.0 > origin)
        .map(|p| TempoPoint {
            tick: Tick(p.tick.0.saturating_sub(origin)),
            ..*p
        })
        .collect();
    let held_at = map.points().iter().rev().find(|p| p.tick.0 <= origin);
    let bpm = map.bpm_at(Tick(origin)) * 1000.0;
    let bpm = if bpm.is_finite() && bpm > 0.0 {
        Bpm(bpm.round().min(f64::from(u32::MAX)) as u32)
    } else {
        Bpm::whole(120)
    };
    points.insert(
        0,
        TempoPoint {
            tick: Tick(0),
            bpm,
            ramp: held_at.is_some_and(|p| p.ramp),
        },
    );
    let Ok(to) = TempoMap::new(points) else {
        return Ok((theirs, Vec::new()));
    };
    let back = theirs
        .doc
        .sequence(theirs.seq)
        .map_err(refused)?
        .tempo
        .clone();
    let mut h = History::new(theirs.doc.clone());
    let mut lines = Vec::new();
    for tempo in [to, back] {
        let cmd = Command::Sequence(SequenceCmd::SetTempo {
            seq: theirs.seq,
            tempo,
        });
        if let Ok(r) = h.execute(&cmd, &ViewFilter::default()) {
            lines.extend(r.settled);
        }
    }
    // The untouched tracks' events as imported, again: under theirs' own
    // map the scratch project held them before, so it holds them now.
    let mine: BTreeSet<u64> = untouched
        .iter()
        .filter_map(|&i| theirs.identity.tracks.get(i))
        .flat_map(|ids| {
            let lane = theirs
                .doc
                .lane(theirs.seq, LaneId(ids.lane))
                .ok()
                .into_iter()
                .flat_map(|l| l.takes.iter())
                .flat_map(|t| t.events.values().map(|e| e.id.0));
            ids.regions
                .iter()
                .flat_map(|at| [at.event, at.top])
                .chain(lane)
                .collect::<Vec<_>>()
        })
        .collect();
    for &id in &mine {
        let (Ok((was, _)), Ok((now, _))) =
            (theirs.doc.event(EventId(id)), h.doc().event(EventId(id)))
        else {
            continue;
        };
        if was == now {
            continue;
        }
        let spec = EventSpec {
            tick: was.tick,
            dest: was.dest,
            group_channel: was.group_channel,
            mute: was.mute,
            body: was.body.clone(),
        };
        let cmd = Command::Event(EventCmd::Replace {
            event: EventId(id),
            spec,
        });
        if h.execute(&cmd, &ViewFilter::default()).is_err() {
            return Ok((theirs, Vec::new()));
        }
    }
    lines.retain(|(e, _)| !mine.contains(&e.0));
    let (raw, _) = projection_with_identity(h.doc(), theirs.seq)?;
    let now = returned(&raw, origin);
    // Both change, or neither (fix round 4).
    let lined_up = now.tracks.len() == theirs.projection.tracks.len()
        && theirs
            .projection
            .tracks
            .iter()
            .zip(&now.tracks)
            .enumerate()
            .all(|(i, (t, n))| {
                if untouched.contains(&i) {
                    t.regions == n.regions
                } else {
                    t.regions.len() == n.regions.len()
                }
            });
    if !lined_up {
        return Ok((theirs, Vec::new()));
    }
    let mut theirs = theirs;
    for (t, n) in theirs.projection.tracks.iter_mut().zip(now.tracks) {
        t.regions = n.regions;
    }
    theirs.doc = h.doc().clone();
    Ok((theirs, lines))
}

/// Moves each curve of a track new in theirs (added, or new on both sides)
/// that drives another track's strip, one no new track plays through, to
/// that track: it is theirs' automation for that track, decided by the
/// rule table as any other (Task 7 fix round 4, automation case A). The
/// apply's arrangement of the added tracks then leaves it out
/// (`import::apply::arrange_tracks`, a strip it did not place), and a
/// second merge, where the track is new on both sides, finds it without
/// that curve, as ours holds it. A curve whose target track already has
/// one for that parameter in the file is taken off the new track and not
/// moved; each is answered (theirs' track, its [`text::doubled_curve`]
/// line) for [`text::NOT_APPLIED`].
fn rehome_curves(theirs: &mut Theirs, m: &Matches) -> Vec<(usize, String)> {
    let added: BTreeSet<usize> = m
        .tracks
        .iter()
        .filter_map(|r| match (r.base, r.theirs) {
            (None, Some(t)) => Some(t),
            _ => None,
        })
        .collect();
    let tracks = &theirs.projection.tracks;
    let added_strips: BTreeSet<usize> =
        added.iter().filter_map(|&t| tracks.get(t)?.strip).collect();
    let owner = |c: usize| {
        (!added_strips.contains(&c))
            .then(|| {
                tracks
                    .iter()
                    .enumerate()
                    .find(|(i, x)| x.strip == Some(c) && !added.contains(i))
                    .map(|(i, _)| i)
            })
            .flatten()
    };
    // A curve for a parameter its track already automates in the file
    // is not moved: two curves for one parameter cannot be merged safely
    // (fix round 5). It is listed instead, and the track's own curve is
    // decided as any other.
    let mut moves: Vec<(usize, usize, Option<usize>)> = Vec::new();
    let mut doubled: Vec<(usize, String)> = Vec::new();
    let mut moved: BTreeSet<(usize, &str)> = BTreeSet::new();
    for &a in &added {
        let Some(t) = tracks.get(a) else { continue };
        for (k, (target, _)) in t.automation.iter().enumerate() {
            let c = target
                .split(' ')
                .nth(1)
                .and_then(|c| c.parse::<usize>().ok());
            let Some(to) = c.and_then(owner) else {
                continue;
            };
            let Some(tt) = tracks.get(to) else { continue };
            let has = tt.automation.iter().any(|(x, _)| x == target);
            if has || !moved.insert((to, target.as_str())) {
                doubled.push((
                    a,
                    text::doubled_curve(&t.name, target_kind(target), &tt.name),
                ));
                moves.push((a, k, None));
            } else {
                moves.push((a, k, Some(to)));
            }
        }
    }
    // Back to front, so each index still names its curve.
    for &(a, k, to) in moves.iter().rev() {
        let p = &mut theirs.projection.tracks;
        let i = &mut theirs.identity.tracks;
        if k >= p.get(a).map_or(0, |x| x.automation.len())
            || k >= i.get(a).map_or(0, |x| x.automation.len())
        {
            continue;
        }
        let curve = p.get_mut(a).map(|x| x.automation.remove(k));
        let ats = i.get_mut(a).map(|x| x.automation.remove(k));
        let Some(to) = to else { continue };
        if let (Some(curve), Some(ats), Some(pt), Some(it)) = (
            curve,
            ats,
            p.get_mut(to),
            i.get_mut(to).map(|x| &mut x.automation),
        ) {
            pt.automation.push(curve);
            it.push(ats);
        }
    }
    doubled
}

/// Theirs' tracks whose regions, as imported, are their base track's
/// (the same set, as [`Decider::regions_eq`] compares them): theirs did
/// not change them, whatever the tempo map the merge leaves (fix round 4).
fn untouched_regions(
    m: &Matches,
    (b, base_ids): (&Projection, &[TrackIds]),
    theirs: &Theirs,
) -> BTreeSet<usize> {
    m.tracks
        .iter()
        .filter_map(|r| {
            let (bi, ti) = (r.base?, r.theirs?);
            let (bt, bids) = (b.tracks.get(bi)?, base_ids.get(bi)?);
            let tt = theirs.projection.tracks.get(ti)?;
            let tids = theirs.identity.tracks.get(ti)?;
            same_regions((bt, bids), (tt, tids, &theirs.origins)).then_some(ti)
        })
        .collect()
}

/// Whether the regions of two track parts are the same set: `b`'s are
/// paired with `a`'s by tag (through `b`'s origins), then media and
/// position, and each pair is the same region.
fn same_regions(a: (&PTrack, &TrackIds), b: (&PTrack, &TrackIds, &Origins)) -> bool {
    let (at, bt) = (a.0, b.0);
    if at.regions.len() != bt.regions.len() {
        return false;
    }
    regions(a, b).iter().all(|p| match *p {
        (Some(i), Some(j)) => match (at.regions.get(i), bt.regions.get(j)) {
            (Some(r), Some(s)) => same_region(r, s),
            _ => false,
        },
        _ => false,
    })
}

/// [`held`]'s settle lines, each under the import's line for crossfades it
/// could not keep (`import::CROSSFADES_PLAIN`), for the regions the merge
/// takes from theirs: on a track whose regions are theirs or a conflict
/// take, or a track theirs added.
fn held_lines(
    settled: &[(EventId, String)],
    theirs: &Theirs,
    matches: &Matches,
    decisions: &[Decision],
    report: &mut Report,
) {
    for (event, line) in settled {
        let Some(t) = theirs
            .identity
            .tracks
            .iter()
            .position(|x| x.regions.iter().any(|r| r.event == event.0))
        else {
            continue;
        };
        let Some(row) = matches.tracks.iter().position(|m| m.theirs == Some(t)) else {
            continue;
        };
        let taken = decisions.iter().any(|d| {
            d.row == row
                && match d.unit {
                    Unit::Regions => matches!(d.outcome, Outcome::Theirs | Outcome::Conflict),
                    Unit::Track => d.outcome == Outcome::Added,
                    _ => false,
                }
        });
        if taken {
            report.add(
                ItemKind::Changed,
                crate::dawproject::import::CROSSFADES_PLAIN,
                Some(line.clone()),
            );
        }
    }
}

/// The Ruling 8 lines (private copies, uses unrolled, edits that play
/// elsewhere) `plan`'s merge would list, from a dry run of the apply on a
/// clone of `ours` (Task 6b fix round 1), so the preview lists what the
/// merge will. Only edits inside uses make them: without a `Theirs` notes,
/// regions or curve decision there is nothing to run. A failing dry run
/// lists nothing (the real apply reports the failure); `cancel` stops it
/// midway (and `plan` then answers `Cancelled`). Each line keeps the real
/// apply's count, past the details it keeps (Task 7).
fn dry_run(plan: &MergePlan, ours: &Document, cancel: &AtomicBool) -> Report {
    let mut out = Report::default();
    let edits = plan.decisions.iter().any(|d| {
        d.outcome == Outcome::Theirs
            && matches!(d.unit, Unit::Notes | Unit::Regions | Unit::Automation(_))
    });
    if !edits {
        return out;
    }
    let mut h = core_model::history::History::new(ours.clone());
    let mut report = Report::default();
    if h.begin(super::apply::LABEL).is_err()
        || super::apply::apply_merge_until(&mut h, plan, "0000-00-00", &mut report, cancel).is_err()
    {
        return out;
    }
    out.items = report
        .items
        .into_iter()
        .filter(|i| super::apply::APPLIED_LINES.contains(&i.what.as_str()))
        .collect();
    out
}

// ---------------------------------------------------------------- sides

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

/// The three sides, as indices into [`Decider`]'s arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Base = 0,
    Theirs = 1,
    Ours = 2,
}

// `target::key`'s marks are these sides'.
const _: () =
    assert!(Side::Theirs as usize == target::THEIRS && Side::Ours as usize == target::OURS);

/// An event no project has: what a format-1 base's regions are "tagged"
/// with, so only their media and position match them (Ruling 3).
const NO_EVENT: EventAt = EventAt {
    event: u64::MAX,
    top: u64::MAX,
};

/// Base's track identities, or, for a format-1 snapshot, ones that tag
/// nothing (parallel to `b`'s rows).
fn base_track_ids(b: &Projection, ids: Option<&Identity>) -> Vec<TrackIds> {
    match ids {
        Some(i) => i.tracks.clone(),
        None => b
            .tracks
            .iter()
            .map(|t| TrackIds {
                regions: vec![NO_EVENT; t.regions.len()],
                ..TrackIds::default()
            })
            .collect(),
    }
}

/// Ours' top-level events as their own origins, so base↔ours regions pair
/// by event (both sides are the same project).
fn self_origins(ids: &Identity) -> Origins {
    let mut o = Origins::default();
    for t in &ids.tracks {
        for r in &t.regions {
            o.events.insert(EventId(r.top), r.top);
        }
    }
    o
}

/// Per side, each strip's row in [`Matches::strips`].
fn strip_keys(m: &Matches, lens: [usize; 3]) -> [Vec<Option<usize>>; 3] {
    let mut keys = lens.map(|n| vec![None; n]);
    for (k, r) in m.strips.iter().enumerate() {
        for (side, i) in [r.base, r.theirs, r.ours].into_iter().enumerate() {
            if let Some(slot) = i.and_then(|i| keys.get_mut(side)?.get_mut(i)) {
                *slot = Some(k);
            }
        }
    }
    keys
}

/// The sequences of `doc` used outside sequence `seq`: by another
/// sequence's reference use, or by a keyboard slot.
pub(super) fn used_elsewhere(doc: &Document, seq: SeqId) -> BTreeSet<SeqId> {
    let mut out = BTreeSet::new();
    for s in doc.sequences().values() {
        if s.keyboard_slot.is_some() {
            out.insert(s.id);
        }
        if s.id == seq {
            continue;
        }
        for l in &s.lanes {
            for t in &l.takes {
                for e in t.events.values() {
                    if let EventBody::Reference { seq, .. } = e.body {
                        out.insert(seq);
                    }
                }
            }
        }
    }
    out
}

/// Ours-only rows by key, first come.
struct Twins<K>(HashMap<K, VecDeque<usize>>);

impl<K: Hash + Eq> Twins<K> {
    fn new(rows: impl Iterator<Item = (K, usize)>) -> Twins<K> {
        let mut free: HashMap<K, VecDeque<usize>> = HashMap::new();
        for (k, i) in rows {
            free.entry(k).or_default().push_back(i);
        }
        Twins(free)
    }

    /// The first free row of key `k`, now taken.
    fn take(&mut self, k: &K) -> Option<usize> {
        self.0.get_mut(k)?.pop_front()
    }
}

/// Joins `rows`' theirs-only rows with the ours-only rows `key` pairs them
/// with (first come), and drops the ours-only rows so taken. `match_all`
/// puts every ours-only row last, so no other row moves.
fn join_twins<K: Hash + Eq>(
    rows: &mut Vec<Matched>,
    key_t: impl Fn(usize) -> Option<K>,
    key_o: impl Fn(usize) -> Option<K>,
) {
    let mut twins = Twins::new(
        rows.iter()
            .filter_map(|r| match (r.base, r.theirs, r.ours) {
                (None, None, Some(o)) => Some((key_o(o)?, o)),
                _ => None,
            }),
    );
    let mut taken = BTreeSet::new();
    for r in rows.iter_mut() {
        if let (None, Some(t), None) = (r.base, r.theirs, r.ours)
            && let Some(o) = key_t(t).and_then(|k| twins.take(&k))
        {
            r.ours = Some(o);
            r.by = MatchedBy::Name;
            taken.insert(o);
        }
    }
    rows.retain(
        |r| !matches!((r.base, r.theirs, r.ours), (None, None, Some(o)) if taken.contains(&o)),
    );
}

/// Pairs the parts new on both sides that are one by name (the same
/// file merged before, or the same name chosen twice): a track by name
/// and type, a bus by name and role, a marker by name and tick (within
/// [`NOTE_TICKS`]). Done before any decision, so a paired row is one row.
fn pair_twins(m: &mut Matches, t: &Projection, o: &Projection) {
    let track_key = |p: &Projection, i: usize| p.tracks.get(i).map(|x| (x.name.clone(), x.audio));
    join_twins(&mut m.tracks, |i| track_key(t, i), |i| track_key(o, i));
    let (tt, ot) = (track_strips(t), track_strips(o));
    let bus_key = |p: &Projection, tracks: &BTreeSet<usize>, i: usize| {
        let s = p.strips.get(i)?;
        (!tracks.contains(&i) && s.role != "master").then(|| (s.name.clone(), s.role.clone()))
    };
    join_twins(
        &mut m.strips,
        |i| bus_key(t, &tt, i),
        |i| bus_key(o, &ot, i),
    );
    // Markers by name, then the nearest free tick.
    let mut free: HashMap<&str, BTreeSet<(i64, usize)>> = HashMap::new();
    for r in &m.markers {
        if let (None, None, Some(i)) = (r.base, r.theirs, r.ours)
            && let Some(x) = o.markers.get(i)
        {
            free.entry(x.1.as_str()).or_default().insert((x.0, i));
        }
    }
    let mut taken = BTreeSet::new();
    for r in &mut m.markers {
        let (None, Some(i), None) = (r.base, r.theirs, r.ours) else {
            continue;
        };
        let Some(x) = t.markers.get(i) else { continue };
        let Some(set) = free.get_mut(x.1.as_str()) else {
            continue;
        };
        let lo = (x.0.saturating_sub(NOTE_TICKS), 0);
        let hi = (x.0.saturating_add(NOTE_TICKS), usize::MAX);
        if let Some(found) = set.range(lo..=hi).next().copied() {
            set.remove(&found);
            r.ours = Some(found.1);
            r.by = MatchedBy::Name;
            taken.insert(found.1);
        }
    }
    m.markers.retain(
        |r| !matches!((r.base, r.theirs, r.ours), (None, None, Some(o)) if taken.contains(&o)),
    );
}

/// Takes one `detail` out of `report`'s item `what`, with its count.
fn drop_detail(report: &mut Report, kind: ItemKind, what: &str, detail: &str) {
    let Some(at) = report
        .items
        .iter()
        .position(|i| i.kind == kind && i.what == what)
    else {
        return;
    };
    let Some(item) = report.items.get_mut(at) else {
        return;
    };
    let Some(d) = item.details.iter().position(|d| d == detail) else {
        return;
    };
    item.details.remove(d);
    item.count = item.count.saturating_sub(1);
    if item.count == 0 {
        report.items.remove(at);
    }
}

/// The strips some track of `p` plays through.
fn track_strips(p: &Projection) -> BTreeSet<usize> {
    p.tracks.iter().filter_map(|t| t.strip).collect()
}

// ---------------------------------------------------------------- tolerances

pub(super) fn ticks_near(a: i64, b: i64) -> bool {
    (i128::from(a) - i128::from(b)).abs() <= i128::from(NOTE_TICKS)
}

fn frames_near(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= super::FRAMES
}

/// Within [`LEVEL_DB`]; −∞ equals anything below −120 dB. A value that is
/// not finite equals only itself.
fn level_eq(a: f64, b: f64) -> bool {
    if a < -120.0 && b < -120.0 {
        return true;
    }
    if !a.is_finite() || !b.is_finite() {
        return a == b;
    }
    (a - b).abs() <= LEVEL_DB
}

fn pan_eq(a: f64, b: f64) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return a == b;
    }
    (a - b).abs() <= PAN
}

fn value_eq(a: f64, b: f64) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return a == b;
    }
    (a - b).abs() <= VALUE
}

pub(super) fn same_note(a: &PNote, b: &PNote) -> bool {
    a.velocity == b.velocity && a.release == b.release && a.expression == b.expression
}

/// Two regions are the same without their media hash, gain and file stem
/// (Ruling 11).
pub(super) fn same_region(a: &PRegion, b: &PRegion) -> bool {
    ticks_near(a.tick, b.tick)
        && frames_near(a.offset, b.offset)
        && frames_near(a.length, b.length)
        && frames_near(a.fade_in, b.fade_in)
        && frames_near(a.fade_out, b.fade_out)
        && a.muted == b.muted
        && a.name == b.name
        && a.crossfade == b.crossfade
}

/// MIDI events as a set: equal texts at ticks within [`NOTE_TICKS`].
fn midi_eq(a: &[(i64, String)], b: &[(i64, String)]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    fn sorted(v: &[(i64, String)]) -> Vec<&(i64, String)> {
        let mut v: Vec<&(i64, String)> = v.iter().collect();
        v.sort_by(|x, y| (&x.1, x.0).cmp(&(&y.1, y.0)));
        v
    }
    sorted(a)
        .iter()
        .zip(sorted(b))
        .all(|(x, y)| x.1 == y.1 && ticks_near(x.0, y.0))
}

fn notes_eq(a: &PTrack, b: &PTrack) -> bool {
    if a.notes_played.len() != b.notes_played.len() || !midi_eq(&a.midi, &b.midi) {
        return false;
    }
    notes(a, b).iter().all(|p| match *p {
        (Some(i), Some(j)) => match (a.notes_played.get(i), b.notes_played.get(j)) {
            (Some(x), Some(y)) => same_note(x, y),
            _ => false,
        },
        _ => false,
    })
}

fn curve_eq(a: &[(i64, f64)], b: &[(i64, f64)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| ticks_near(x.0, y.0) && value_eq(x.1, y.1))
}

fn tempo_eq(a: &[(i64, u32, bool)], b: &[(i64, u32, bool)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| ticks_near(x.0, y.0) && x.1 == y.1 && x.2 == y.2)
}

fn meter_eq(a: &[(i64, u8, u8)], b: &[(i64, u8, u8)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| ticks_near(x.0, y.0) && x.1 == y.1 && x.2 == y.2)
}

/// The rule table for values `[base, theirs, ours]` under `eq`.
fn outcome_of<T>(v: &[T; 3], eq: impl Fn(&T, &T) -> bool) -> Option<Outcome> {
    let [b, t, o] = v;
    outcome(eq(b, t), eq(b, o), || eq(t, o))
}

/// The rule table for a unit present on all three sides, from whether
/// base equals theirs, base equals ours, and theirs equals ours.
fn outcome(bt: bool, bo: bool, to: impl FnOnce() -> bool) -> Option<Outcome> {
    match (bt, bo) {
        (true, true) => None,
        (false, true) => Some(Outcome::Theirs),
        (true, false) => Some(Outcome::Ours),
        (false, false) if to() => Some(Outcome::Same),
        (false, false) => Some(Outcome::Conflict),
    }
}

// ---------------------------------------------------------------- text

/// The bar (1 at tick 0, each bar as long as its meter) of `tick`, with
/// `meter` in the same frame.
fn bar_of(meter: &[(i64, u8, u8)], tick: i64) -> i128 {
    let bar_len = |num: u8, den: u8| -> i128 {
        let den = i128::from(den.max(1));
        (i128::from(num) * i128::from(PPQ) * 4 / den).max(1)
    };
    let tick = i128::from(tick);
    let (mut bar, mut at, mut len) = (1i128, 0i128, bar_len(4, 4));
    for &(t, num, den) in meter {
        let t = i128::from(t);
        if t > tick {
            break;
        }
        if t > at {
            bar = bar.saturating_add((t - at) / len);
            at = t;
        }
        len = bar_len(num, den);
    }
    bar.saturating_add((tick - at).div_euclid(len))
}

/// A song-info field's name, in `SongInfo::fields`' order.
fn info_name(i: usize) -> String {
    const NAMES: [&str; 13] = [
        "title",
        "artist",
        "album",
        "original artist",
        "composer",
        "songwriter",
        "producer",
        "arranger",
        "year",
        "genre",
        "copyright",
        "website",
        "comment",
    ];
    NAMES.get(i).map_or_else(
        || format!("field {}", i.saturating_add(1)),
        |n| (*n).to_owned(),
    )
}

fn colour_text(c: &Option<String>) -> String {
    match c.as_deref() {
        None => "none".to_owned(),
        Some(hex) => nearest_swatch(hex).map_or_else(|| hex.to_owned(), |(s, _)| format!("{s:?}")),
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// An automation target's kind, for `what`.
fn target_kind(target: &str) -> &'static str {
    match target.split(' ').next() {
        Some("fader") => "level",
        Some("pan") => "pan",
        Some("mute") => "mute",
        Some("send") => "send level",
        Some("param") => "plug-in parameter",
        _ => "other",
    }
}

// ---------------------------------------------------------------- the decider

/// Ours as the merge leaves it, for [`target::resolve`]: ours' strips and
/// the buses theirs adds (step 1); ours' sends and those the strip step
/// adds (a [`Unit::Send`] decided [`Outcome::Theirs`]), and an added bus's
/// own; ours' devices.
struct Leaves<'d, 'a>(&'d Decider<'a>);

impl Holds for Leaves<'_, '_> {
    type Strip = ();
    type Send = ();
    type Slot = ();

    fn strip(&self, row: usize) -> Option<()> {
        let d = self.0;
        let r = d.m.strips.get(row)?;
        let ours = r
            .ours
            .and_then(|o| d.ours_strips.strips.get(o))
            .is_some_and(|s| s.strip.is_some());
        (ours || (r.ours.is_none() && d.decided(&Unit::Bus, row, Outcome::Added))).then_some(())
    }

    fn send(&self, (row, _): (usize, ()), (to, _): (usize, ())) -> Option<()> {
        let d = self.0;
        let r = d.m.strips.get(row)?;
        let sends_to = |s: Side, i: usize| {
            d.strip_of(s, i)
                .is_some_and(|st| st.sends.iter().any(|x| d.skey(s, x.0) == Some(to)))
        };
        let held = match r.ours {
            Some(o) => sends_to(Side::Ours, o) || d.decided(&Unit::Send(to), row, Outcome::Theirs),
            // An added bus comes with its own mix.
            None => r.theirs.is_some_and(|t| sends_to(Side::Theirs, t)),
        };
        held.then_some(())
    }

    fn slot(&self, row: usize, device: usize) -> Option<()> {
        let d = self.0;
        let o = d.m.strips.get(row)?.ours?;
        d.ours_strips.strips.get(o)?.devices.get(device).map(|_| ())
    }
}

/// A send's level, pre-fader and enabled.
type Send = (f64, bool, bool);

struct Decider<'a> {
    /// The three projections, in the file's frame.
    p: [&'a Projection; 3],
    /// Their track identities.
    ids: [&'a [TrackIds]; 3],
    theirs_origins: &'a Origins,
    ours_origins: &'a Origins,
    theirs_strips: &'a Identity,
    /// Ours' strip identities.
    ours_strips: &'a Identity,
    /// Base's, when the snapshot has them (format 2).
    base_strips: Option<&'a Identity>,
    /// Theirs' scratch project.
    theirs_doc: &'a Document,
    /// The decisions (by index in `out`) theirs changed and the merge does
    /// not apply: listed under `text::NOT_APPLIED`.
    not_applied: BTreeSet<usize>,
    /// Of those, the ones listed by their `what` alone (no values).
    plain: BTreeSet<usize>,
    m: &'a Matches,
    /// Per side, each strip's row in `m.strips`.
    skey: [Vec<Option<usize>>; 3],
    /// Per side, the strips its tracks play through.
    tstrips: [BTreeSet<usize>; 3],
    /// The strips theirs' audio tracks play through.
    audio_paths: BTreeSet<usize>,
    ours_doc: &'a Document,
    seq: SeqId,
    /// The sequences used outside the exported one (by another sequence
    /// or a keyboard slot), for Ruling 7.
    elsewhere: BTreeSet<SeqId>,
    /// Each row of `m.strips`' label ("Strip 'Bass'").
    strip_labels: Vec<String>,
    out: Vec<Decision>,
    /// The details for `text::VST_KEPT`, each with the name the import
    /// listed under `import::VST` (a VST plug-in's).
    vst: Vec<(String, Option<String>)>,
}

impl<'a> Decider<'a> {
    fn side(&self, s: Side) -> &'a Projection {
        let [b, t, o] = self.p;
        match s {
            Side::Base => b,
            Side::Theirs => t,
            Side::Ours => o,
        }
    }

    fn ids_of(&self, s: Side, i: usize) -> Option<&'a TrackIds> {
        let [b, t, o] = self.ids;
        match s {
            Side::Base => b.get(i),
            Side::Theirs => t.get(i),
            Side::Ours => o.get(i),
        }
    }

    fn track_of(&self, s: Side, i: usize) -> Option<&'a PTrack> {
        self.side(s).tracks.get(i)
    }

    fn strip_of(&self, s: Side, i: usize) -> Option<&'a PStrip> {
        self.side(s).strips.get(i)
    }

    /// Whether `unit` of row `row` is decided `outcome`.
    fn decided(&self, unit: &Unit, row: usize, outcome: Outcome) -> bool {
        self.out
            .iter()
            .any(|d| d.row == row && d.outcome == outcome && d.unit == *unit)
    }

    fn skey(&self, s: Side, i: usize) -> Option<usize> {
        self.skey.get(s as usize)?.get(i).copied().flatten()
    }

    fn push(&mut self, unit: Unit, row: usize, outcome: Outcome, what: String) {
        self.push_with(unit, row, outcome, what, None, None);
    }

    fn push_with(
        &mut self,
        unit: Unit,
        row: usize,
        outcome: Outcome,
        what: String,
        ours: Option<String>,
        theirs: Option<String>,
    ) {
        self.out.push(Decision {
            unit,
            row,
            outcome,
            what,
            ours,
            theirs,
        });
    }

    /// Pushes a [`Outcome::Conflict`] the merge does not apply, listed
    /// under `text::NOT_APPLIED`.
    fn push_not_applied(
        &mut self,
        unit: Unit,
        row: usize,
        what: String,
        ours: Option<String>,
        theirs: Option<String>,
    ) {
        self.not_applied.insert(self.out.len());
        self.push_with(unit, row, Outcome::Conflict, what, ours, theirs);
    }

    /// Pushes the decision for a unit on all three sides, with its values.
    fn three<T>(
        &mut self,
        unit: Unit,
        row: usize,
        what: String,
        v: [T; 3],
        eq: impl Fn(&T, &T) -> bool,
        show: impl Fn(&T) -> String,
    ) {
        let [b, t, o] = &v;
        if let Some(out) = outcome(eq(b, t), eq(b, o), || eq(t, o)) {
            self.push_with(unit, row, out, what, Some(show(o)), Some(show(t)));
        }
    }

    /// A bar in side `s`'s own meter.
    fn bar(&self, s: Side, tick: i64) -> i128 {
        bar_of(&self.side(s).meter, tick)
    }

    // ------------------------------------------------------------ tracks

    fn track_name(&self, row: usize) -> String {
        let Some(r) = self.m.tracks.get(row) else {
            return String::new();
        };
        [
            (Side::Ours, r.ours),
            (Side::Base, r.base),
            (Side::Theirs, r.theirs),
        ]
        .into_iter()
        .find_map(|(s, i)| Some(self.track_of(s, i?)?.name.clone()))
        .unwrap_or_default()
    }

    /// Whether the regions of track `x` and track `y` are the same set.
    fn regions_eq(&self, x: (Side, usize), y: (Side, usize)) -> bool {
        let (x, y) = if x.0 == Side::Theirs { (y, x) } else { (x, y) };
        let (Some(a), Some(b)) = (self.track_of(x.0, x.1), self.track_of(y.0, y.1)) else {
            return false;
        };
        if a.regions.len() != b.regions.len() {
            return false;
        }
        let (Some(aids), Some(bids)) = (self.ids_of(x.0, x.1), self.ids_of(y.0, y.1)) else {
            return false;
        };
        let origins = if y.0 == Side::Theirs {
            self.theirs_origins
        } else {
            self.ours_origins
        };
        same_regions((a, aids), (b, bids, origins))
    }

    /// Track `i` of side `s`'s automation, each by its [`target::key`].
    fn curves(&self, s: Side, i: usize) -> BTreeMap<String, &'a [(i64, f64)]> {
        let mut out = BTreeMap::new();
        let Some(t) = self.track_of(s, i) else {
            return out;
        };
        let strips = &self.side(s).strips;
        for (target, points) in &t.automation {
            let key = target::key(target, s as usize, |c| self.skey(s, c), strips);
            out.entry(key).or_insert(points.as_slice());
        }
        out
    }

    /// Whether track `x` and track `y` (on two sides) carry the same
    /// content: name, notes, regions, automation and their strips.
    fn track_eq(&self, x: (Side, usize), y: (Side, usize)) -> bool {
        self.track_eq_with(x, y, true, false)
    }

    /// [`Self::track_eq`], the notes and MIDI events compared only when
    /// `with_notes`. For `twins` (a track new on both sides) the name of a
    /// strip theirs made for an audio track is not compared: the merge
    /// adds that strip named after its track, as the import does, so ours'
    /// copy of a track merged before never has the file's channel name.
    fn track_eq_with(
        &self,
        x: (Side, usize),
        y: (Side, usize),
        with_notes: bool,
        twins: bool,
    ) -> bool {
        let (Some(a), Some(b)) = (self.track_of(x.0, x.1), self.track_of(y.0, y.1)) else {
            return false;
        };
        let curves = {
            let (ca, cb) = (self.curves(x.0, x.1), self.curves(y.0, y.1));
            ca.len() == cb.len()
                && ca
                    .iter()
                    .zip(&cb)
                    .all(|((ka, a), (kb, b))| ka == kb && curve_eq(a, b))
        };
        let strips = match (a.strip, b.strip) {
            (Some(sa), Some(sb)) => self.strip_eq_with((x.0, sa), (y.0, sb), !twins),
            (None, None) => true,
            _ => false,
        };
        a.name == b.name
            && (!with_notes || notes_eq(a, b))
            && self.regions_eq(x, y)
            && curves
            && strips
    }

    fn track(&mut self, row: usize) {
        let Some(r) = self.m.tracks.get(row).copied() else {
            return;
        };
        let name = self.track_name(row);
        let whole = format!("Track '{name}'");
        match (r.base, r.theirs, r.ours) {
            (Some(b), Some(t), Some(o)) => self.track_units(row, &name, [b, t, o]),
            (Some(b), None, Some(o)) => {
                let out = if !self.track_eq((Side::Base, b), (Side::Ours, o)) {
                    Outcome::DeletedThere
                } else if self.holds_uncarried(o) {
                    Outcome::KeptEmpty
                } else {
                    Outcome::Removed
                };
                self.push(Unit::Track, row, out, whole);
            }
            (Some(b), Some(t), None) => {
                let out = if self.track_eq((Side::Base, b), (Side::Theirs, t)) {
                    Outcome::Ours
                } else {
                    Outcome::DeletedHere
                };
                self.push(Unit::Track, row, out, whole);
            }
            (Some(_), None, None) => {
                self.push(Unit::Track, row, Outcome::Same, whole);
            }
            (None, Some(_), None) => self.push(Unit::Track, row, Outcome::Added, whole),
            (None, None, Some(_)) => self.push(Unit::Track, row, Outcome::Ours, whole),
            (None, Some(t), Some(o)) => self.twin_track(row, &name, t, o),
            _ => {}
        }
    }

    /// A track new on both sides with one name and type (`pair_twins`):
    /// equal, `Same` (the same file merged before). Otherwise ours stays.
    /// A notes track's differing notes are a `Notes` conflict, so theirs
    /// becomes the alternate take as in any notes conflict (Ruling 6),
    /// unless theirs has no notes (then only ours' notes differ: `Ours`);
    /// anything else that differs, or any difference on an audio track,
    /// is a `Track` conflict listed under `text::ADDED_BOTH`.
    fn twin_track(&mut self, row: usize, name: &str, t: usize, o: usize) {
        let whole = format!("Track '{name}'");
        let (x, y) = ((Side::Theirs, t), (Side::Ours, o));
        if self.track_eq_with(x, y, true, true) {
            self.push(Unit::Track, row, Outcome::Same, whole);
            return;
        }
        let (Some(tt), Some(ot)) = (self.track_of(Side::Theirs, t), self.track_of(Side::Ours, o))
        else {
            return;
        };
        let rest_same = self.track_eq_with(x, y, false, true);
        // Theirs with no notes has no take to give: ours' notes are kept.
        let empty = tt.notes_played.is_empty() && tt.midi.is_empty();
        if !tt.audio && empty && rest_same {
            self.push(Unit::Track, row, Outcome::Ours, whole);
            return;
        }
        let take = !tt.audio && !empty && !notes_eq(ot, tt);
        if take {
            let notes_of = |x: &PTrack| count(x.notes_played.len(), "note", "notes");
            let what = format!("Track '{name}': notes");
            let (ours, theirs) = (Some(notes_of(ot)), Some(notes_of(tt)));
            self.push_with(Unit::Notes, row, Outcome::Conflict, what, ours, theirs);
        }
        if !take || !rest_same {
            self.push(Unit::Track, row, Outcome::Conflict, whole);
        }
    }

    fn track_units(&mut self, row: usize, name: &str, [b, t, o]: [usize; 3]) {
        let (Some(bt), Some(tt), Some(ot)) = (
            self.track_of(Side::Base, b),
            self.track_of(Side::Theirs, t),
            self.track_of(Side::Ours, o),
        ) else {
            return;
        };
        self.three(
            Unit::TrackName,
            row,
            format!("Track '{name}': name"),
            [&bt.name, &tt.name, &ot.name],
            |x, y| x == y,
            |x| (*x).clone(),
        );
        // A track that plays with its own loop, length or play quantize:
        // the export's tick transform has no inverse, so a change of
        // theirs is a take conflict (notes, regions) or not applied
        // (automation), never edits (Task 6 fix round 1, C1).
        let why = self.transformed(o);
        let fixed = why.is_some();
        let take = |out: Outcome| match out {
            Outcome::Theirs if fixed => Outcome::Conflict,
            out => out,
        };
        let notes_of = |x: &PTrack| count(x.notes_played.len(), "note", "notes");
        if let Some(out) = outcome(notes_eq(bt, tt), notes_eq(bt, ot), || notes_eq(ot, tt)) {
            let out = take(out);
            let what = format!("Track '{name}': notes");
            self.push_with(
                Unit::Notes,
                row,
                out,
                what,
                Some(notes_of(ot)),
                Some(notes_of(tt)),
            );
        }
        let regions_of = |x: &PTrack| count(x.regions.len(), "region", "regions");
        let (rb, ro, rt) = (
            self.regions_eq((Side::Base, b), (Side::Theirs, t)),
            self.regions_eq((Side::Base, b), (Side::Ours, o)),
            self.regions_eq((Side::Ours, o), (Side::Theirs, t)),
        );
        if let Some(out) = outcome(rb, ro, || rt) {
            let out = take(out);
            let what = format!("Track '{name}': regions");
            self.push_with(
                Unit::Regions,
                row,
                out,
                what,
                Some(regions_of(ot)),
                Some(regions_of(tt)),
            );
        }
        let curves = [
            self.curves(Side::Base, b),
            self.curves(Side::Theirs, t),
            self.curves(Side::Ours, o),
        ];
        let targets: BTreeSet<&String> = curves.iter().flat_map(|c| c.keys()).collect();
        for target in targets {
            let v = curves.each_ref().map(|c| c.get(target).copied());
            let eq = |x: &Option<&[(i64, f64)]>, y: &Option<&[(i64, f64)]>| match (x, y) {
                (Some(a), Some(b)) => curve_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            let Some(out) = outcome_of(&v, eq) else {
                continue;
            };
            let what = format!("Track '{name}': {} automation", target_kind(target));
            let [_, theirs, ours] = v.map(|x| Some(text::points(x.map_or(0, |p| p.len()))));
            let unit = Unit::Automation(target.clone());
            if let Some(why) = why
                && out == Outcome::Theirs
            {
                let what = format!("{what} (the track plays {why})");
                self.push_not_applied(unit, row, what, ours, theirs);
            } else {
                self.push_with(unit, row, out, what, ours, theirs);
            }
        }
    }

    /// Theirs' curves decided [`Outcome::Theirs`] whose target ours, as
    /// the merge leaves it, does not hold (Task 7 fix round 6, automation
    /// case B): each becomes a [`text::NOT_APPLIED`] conflict, the reason
    /// in its `what`, and the apply runs nothing for it. The same
    /// [`target::resolve`] places the others in the apply. A curve theirs
    /// deleted needs no target: ours' points go.
    fn unplaced_curves(&mut self) {
        let mut unplaced = Vec::new();
        for (at, d) in self.out.iter().enumerate() {
            let Unit::Automation(key) = &d.unit else {
                continue;
            };
            if d.outcome != Outcome::Theirs || self.not_applied.contains(&at) {
                continue;
            }
            let Some(t) = self.m.tracks.get(d.row).and_then(|r| r.theirs) else {
                continue;
            };
            if !self.curves(Side::Theirs, t).contains_key(key) {
                continue;
            }
            if let Err(why) = target::resolve(key, &Leaves(self)) {
                let part = match why {
                    Missing::Strip => "strip".to_owned(),
                    Missing::Send(to) => {
                        let to = to.map(|k| self.strip_label(k));
                        match to.as_deref().map(|l| l.trim_start_matches("Strip ")) {
                            Some(l) if !l.is_empty() => format!("send to {l}"),
                            _ => "send".to_owned(),
                        }
                    }
                    Missing::Plugin => "plug-in".to_owned(),
                };
                unplaced.push((at, text::missing_target(&part)));
            }
        }
        for (at, why) in unplaced {
            if let Some(d) = self.out.get_mut(at) {
                d.outcome = Outcome::Conflict;
                d.what = format!("{} ({why})", d.what);
                self.not_applied.insert(at);
            }
        }
    }

    /// Why ours' lane of ours track `o` has ticks the export transformed
    /// beyond undoing ("its own loop", "its own length", "with play
    /// quantize"); `None` when it has not (a play shift can be undone, and
    /// is).
    fn transformed(&self, o: usize) -> Option<&'static str> {
        let l = self
            .ids_of(Side::Ours, o)
            .and_then(|ids| self.ours_doc.lane(self.seq, LaneId(ids.lane)).ok())?;
        if l.looped {
            Some("its own loop")
        } else if l.play_quantize.is_some() {
            Some("with play quantize")
        } else if l.length.is_some() {
            Some("its own length")
        } else {
            None
        }
    }

    /// Ruling 7: whether ours' lane of ours track `o` holds what the file
    /// cannot carry.
    fn holds_uncarried(&self, o: usize) -> bool {
        let Some(ids) = self.ids_of(Side::Ours, o) else {
            return false;
        };
        let Ok(lane) = self.ours_doc.lane(self.seq, LaneId(ids.lane)) else {
            return false;
        };
        lane.takes.len() > 1
            || lane.drum.is_some()
            || lane.input.is_some()
            || !lane.monitor.is_auto()
            || !lane.input_fx.is_empty()
            || lane.mpe.is_some()
            || lane.play_quantize.is_some()
            || lane.play_shift.0 != 0
            || lane.length.is_some()
            || lane.looped
            || self.uses_shared(lane)
    }

    /// Whether `lane` uses a sequence used outside the exported sequence
    /// (by another sequence, or a keyboard slot).
    fn uses_shared(&self, lane: &Lane) -> bool {
        lane.takes.iter().any(|t| {
            t.events.values().any(
                |e| matches!(e.body, EventBody::Reference { seq, .. } if self.elsewhere.contains(&seq)),
            )
        })
    }

    // ------------------------------------------------------------ strips

    /// Each row of `m.strips`' label, "Strip 'Bass'": a track's strip by
    /// its (first) track's name, else its own; ours' name first, then
    /// base's, then theirs'.
    fn labels(&self) -> Vec<String> {
        let first_track = [Side::Base, Side::Theirs, Side::Ours].map(|s| {
            let mut by: HashMap<usize, &str> = HashMap::new();
            for t in &self.side(s).tracks {
                if let Some(i) = t.strip {
                    by.entry(i).or_insert(t.name.as_str());
                }
            }
            by
        });
        let [fb, ft, fo] = &first_track;
        self.m
            .strips
            .iter()
            .map(|r| {
                let name = [
                    (Side::Ours, r.ours, fo),
                    (Side::Base, r.base, fb),
                    (Side::Theirs, r.theirs, ft),
                ]
                .into_iter()
                .find_map(|(s, i, by)| {
                    let i = i?;
                    by.get(&i)
                        .map(|n| (*n).to_owned())
                        .or_else(|| self.strip_of(s, i).map(|x| x.name.clone()))
                })
                .unwrap_or_default();
                format!("Strip '{name}'")
            })
            .collect()
    }

    fn strip_label(&self, row: usize) -> String {
        self.strip_labels.get(row).cloned().unwrap_or_default()
    }

    /// The label of side `s`'s strip `i`, by its row.
    fn label_of(&self, s: Side, i: usize) -> String {
        self.skey(s, i)
            .map_or_else(|| "a new strip".to_owned(), |k| self.strip_label(k))
    }

    /// Strip `i`'s sends on side `s`, by target row.
    fn sends(&self, s: Side, i: usize) -> BTreeMap<usize, Send> {
        let mut out = BTreeMap::new();
        if let Some(st) = self.strip_of(s, i) {
            for &(to, level, pre, on) in &st.sends {
                if let Some(k) = self.skey(s, to) {
                    out.entry(k).or_insert((level, pre, on));
                }
            }
        }
        out
    }

    fn devices(&self, s: Side, i: usize) -> Vec<(&'a str, &'a str)> {
        self.strip_of(s, i).map_or_else(Vec::new, |st| {
            st.devices
                .iter()
                .map(|d| (d.0.as_str(), d.1.as_str()))
                .collect()
        })
    }

    /// How many of side `s`'s strip `i`'s devices are its track's
    /// instrument (0 or 1): the export lists the instrument first, then
    /// the EQ slot and the inserts (`Chan::slots`). Base's is read in ours'
    /// project (the same one); a slot ours no longer has counts as ours'
    /// strip counts.
    fn instruments(&self, s: Side, i: usize) -> usize {
        // Whether the first device is the strip's own (EQ or insert), in
        // `doc`; `None` when there is no device or no such strip.
        let own = |doc: &Document, ids: &StripIds| -> Option<bool> {
            let first = *ids.devices.first()?;
            let Some(strip) = ids.strip else {
                return Some(false);
            };
            let st = doc.mixer().strip(StripId(strip)).ok()?;
            Some(
                st.eq
                    .iter()
                    .chain(st.inserts.iter())
                    .any(|x| x.id.0 == first),
            )
        };
        let of = |own: Option<bool>| usize::from(own == Some(false));
        match s {
            Side::Ours => of(self
                .ours_strips
                .strips
                .get(i)
                .and_then(|x| own(self.ours_doc, x))),
            Side::Theirs => of(self
                .theirs_strips
                .strips
                .get(i)
                .and_then(|x| own(self.theirs_doc, x))),
            Side::Base => {
                let ids = self.base_strips.and_then(|b| b.strips.get(i));
                if let Some(ids) = ids {
                    let Some(first) = ids.devices.first() else {
                        return 0;
                    };
                    let known = self
                        .ours_doc
                        .plugin_slot(core_model::ids::PluginInstanceId(*first))
                        .is_some();
                    if known {
                        return of(own(self.ours_doc, ids));
                    }
                }
                let r = self.skey(Side::Base, i).and_then(|k| self.m.strips.get(k));
                match (r.and_then(|r| r.ours), r.and_then(|r| r.theirs)) {
                    (Some(o), _) => self.instruments(Side::Ours, o),
                    (None, Some(t)) => self.instruments(Side::Theirs, t),
                    _ => 0,
                }
            }
        }
    }

    /// Side `s`'s strip `i`'s devices: its instrument, then the rest.
    fn split_devices(&self, s: Side, i: usize) -> [Vec<(&'a str, &'a str)>; 2] {
        let all = self.devices(s, i);
        let n = self.instruments(s, i);
        [
            all.iter().take(n).copied().collect(),
            all.iter().skip(n).copied().collect(),
        ]
    }

    /// Whether row `k` of `m.strips` is a strip ours deleted that base and
    /// theirs have.
    fn deleted_here(&self, k: usize) -> bool {
        self.m
            .strips
            .get(k)
            .is_some_and(|r| r.base.is_some() && r.theirs.is_some() && r.ours.is_none())
    }

    /// For a strip only theirs has (row `row`, theirs' strip `t`): its
    /// output and sends to a bus ours deleted, each
    /// [`Outcome::DeletedHere`] (the send is not made; the output is the
    /// master).
    fn aimed_at_deleted(&mut self, row: usize, t: usize) {
        let Some(ts) = self.strip_of(Side::Theirs, t) else {
            return;
        };
        let label = self.strip_label(row);
        let to_name = |k: usize| self.strip_label(k).replacen("Strip ", "", 1);
        let mut found = Vec::new();
        if let Some(k) = ts.output.and_then(|i| self.skey(Side::Theirs, i))
            && self.deleted_here(k)
        {
            found.push((
                Unit::Strip(StripField::Output),
                format!("{label}: output to {}", to_name(k)),
            ));
        }
        for &(to, ..) in &ts.sends {
            if let Some(k) = self.skey(Side::Theirs, to)
                && self.deleted_here(k)
            {
                found.push((Unit::Send(k), format!("{label}: send to {}", to_name(k))));
            }
        }
        for (unit, what) in found {
            self.push(unit, row, Outcome::DeletedHere, what);
        }
    }

    /// Whether strip `i` is one theirs made for an audio track: the import
    /// names an audio track's output path after the track, so the file
    /// does not carry that strip's name.
    fn audio_path(&self, s: Side, i: usize) -> bool {
        s == Side::Theirs && self.audio_paths.contains(&i)
    }

    /// Strip `i`'s name on side `s`. For a strip theirs made for an audio
    /// track, the file's channel name (`Origins::channel_names`); `None`
    /// when the file gave none, which counts as unchanged.
    fn strip_name(&self, s: Side, i: usize) -> Option<&'a str> {
        if !self.audio_path(s, i) {
            return self.strip_of(s, i).map(|x| x.name.as_str());
        }
        let id = self.theirs_strips.strips.get(i)?.strip?;
        self.theirs_origins
            .channel_names
            .get(&StripId(id))
            .map(String::as_str)
    }

    /// Whether two strips carry the same settings (a strip theirs made for
    /// an audio track by the file's channel name, when it gave one).
    fn strip_eq(&self, x: (Side, usize), y: (Side, usize)) -> bool {
        self.strip_eq_with(x, y, true)
    }

    /// [`Self::strip_eq`]; without `audio_names`, the name of a strip theirs
    /// made for an audio track is not compared.
    fn strip_eq_with(&self, x: (Side, usize), y: (Side, usize), audio_names: bool) -> bool {
        let skip_name = !audio_names && (self.audio_path(x.0, x.1) || self.audio_path(y.0, y.1));
        let (Some(a), Some(b)) = (self.strip_of(x.0, x.1), self.strip_of(y.0, y.1)) else {
            return false;
        };
        let out = |s: Side, p: &PStrip| p.output.and_then(|o| self.skey(s, o));
        let sends = {
            let (sa, sb) = (self.sends(x.0, x.1), self.sends(y.0, y.1));
            sa.len() == sb.len()
                && sa
                    .iter()
                    .zip(&sb)
                    .all(|((ka, a), (kb, b))| ka == kb && send_eq(&Some(*a), &Some(*b)))
        };
        let devices = a.devices.len() == b.devices.len()
            && a.devices.iter().zip(&b.devices).all(|(p, q)| p == q);
        level_eq(a.gain_db, b.gain_db)
            && pan_eq(a.pan, b.pan)
            && a.mute == b.mute
            && a.solo == b.solo
            && a.colour == b.colour
            && (skip_name
                || match (self.strip_name(x.0, x.1), self.strip_name(y.0, y.1)) {
                    (Some(p), Some(q)) => p == q,
                    _ => true,
                })
            && out(x.0, a) == out(y.0, b)
            && sends
            && devices
    }

    fn is_track_strip(&self, r: &Matched) -> bool {
        [r.base, r.theirs, r.ours]
            .into_iter()
            .enumerate()
            .any(|(s, i)| i.is_some_and(|i| self.tstrips.get(s).is_some_and(|t| t.contains(&i))))
    }

    fn is_master(&self, r: &Matched) -> bool {
        [
            (Side::Base, r.base),
            (Side::Theirs, r.theirs),
            (Side::Ours, r.ours),
        ]
        .into_iter()
        .any(|(s, i)| {
            i.and_then(|i| self.strip_of(s, i))
                .is_some_and(|p| p.role == "master")
        })
    }

    fn strip(&mut self, row: usize) {
        let Some(r) = self.m.strips.get(row).copied() else {
            return;
        };
        if let (Some(b), Some(t), Some(o)) = (r.base, r.theirs, r.ours) {
            self.strip_units(row, [b, t, o]);
            return;
        }
        // A strip only theirs has (a new track's or bus's) aimed at a bus
        // ours deleted (Task 6 fix round 1, I4).
        if let (None, Some(t), None) = (r.base, r.theirs, r.ours) {
            self.aimed_at_deleted(row, t);
        }
        // A track's strip exists with its track; the master always does.
        if self.is_track_strip(&r) || self.is_master(&r) {
            return;
        }
        let name = self.strip_label(row).replacen("Strip", "Bus", 1);
        let out = match (r.base, r.theirs, r.ours) {
            (Some(b), None, Some(o)) => Some(if self.strip_eq((Side::Base, b), (Side::Ours, o)) {
                Outcome::Removed
            } else {
                Outcome::DeletedThere
            }),
            (Some(b), Some(t), None) => {
                Some(if self.strip_eq((Side::Base, b), (Side::Theirs, t)) {
                    Outcome::Ours
                } else {
                    Outcome::DeletedHere
                })
            }
            (Some(_), None, None) => Some(Outcome::Same),
            (None, Some(_), None) => Some(Outcome::Added),
            (None, None, Some(_)) => Some(Outcome::Ours),
            // New on both sides under one name (`pair_twins`).
            (None, Some(t), Some(o)) => {
                Some(if self.strip_eq((Side::Theirs, t), (Side::Ours, o)) {
                    Outcome::Same
                } else {
                    Outcome::Conflict
                })
            }
            _ => None,
        };
        if let Some(out) = out {
            self.push(Unit::Bus, row, out, name);
        }
    }

    fn strip_units(&mut self, row: usize, [b, t, o]: [usize; 3]) {
        let (Some(bs), Some(ts), Some(os)) = (
            self.strip_of(Side::Base, b),
            self.strip_of(Side::Theirs, t),
            self.strip_of(Side::Ours, o),
        ) else {
            return;
        };
        let label = self.strip_label(row);
        let what = |f: &str| format!("{label}: {f}");
        let all = [bs, ts, os];
        self.three(
            Unit::Strip(StripField::Level),
            row,
            what("level"),
            all.map(|s| s.gain_db),
            |x, y| level_eq(*x, *y),
            |x| text::level(*x),
        );
        self.three(
            Unit::Strip(StripField::Pan),
            row,
            what("pan"),
            all.map(|s| s.pan),
            |x, y| pan_eq(*x, *y),
            |x| text::pan(*x),
        );
        self.three(
            Unit::Strip(StripField::Mute),
            row,
            what("mute"),
            all.map(|s| s.mute),
            |x, y| x == y,
            |x| text::on_off(*x).to_owned(),
        );
        self.three(
            Unit::Strip(StripField::Solo),
            row,
            what("solo"),
            all.map(|s| s.solo),
            |x, y| x == y,
            |x| text::on_off(*x).to_owned(),
        );
        self.three(
            Unit::Strip(StripField::Colour),
            row,
            what("colour"),
            all.map(|s| s.colour.clone()),
            |x, y| x == y,
            colour_text,
        );
        // Theirs' name of an audio track's strip is the file's channel
        // name; none given counts as base's. A track whose instrument the
        // import could not make (a VST one, or one not installed) plays the
        // built-in synth, whose strip the import names: that name is not
        // the file's either (Task 7).
        let substitute =
            self.instruments(Side::Theirs, t) == 0 && self.instruments(Side::Base, b) > 0;
        let theirs_name = if substitute {
            bs.name.as_str()
        } else {
            self.strip_name(Side::Theirs, t).unwrap_or(bs.name.as_str())
        };
        self.three(
            Unit::Strip(StripField::Name),
            row,
            what("name"),
            [bs.name.as_str(), theirs_name, os.name.as_str()],
            |x, y| x == y,
            |x| (*x).to_owned(),
        );
        let outputs = [(Side::Base, bs), (Side::Theirs, ts), (Side::Ours, os)]
            .map(|(s, p)| p.output.map(|i| (self.skey(s, i), self.label_of(s, i))));
        let out_eq =
            |x: &Option<(Option<usize>, String)>, y: &Option<(Option<usize>, String)>| match (x, y)
            {
                (Some((Some(a), _)), Some((Some(b), _))) => a == b,
                (None, None) => true,
                _ => false,
            };
        let out_text = |x: &Option<(Option<usize>, String)>| {
            x.as_ref()
                .map_or_else(|| "none".to_owned(), |(_, l)| l.clone())
        };
        if let Some(out) = outcome_of(&outputs, out_eq) {
            let [_, theirs, ours] = &outputs;
            let gone = theirs
                .as_ref()
                .and_then(|(k, _)| *k)
                .filter(|k| self.deleted_here(*k));
            let unit = Unit::Strip(StripField::Output);
            match gone {
                // Theirs plays into a bus ours deleted: ours' output stays.
                Some(k) if out == Outcome::Theirs => {
                    let to = self.strip_label(k).replacen("Strip ", "", 1);
                    self.push(
                        unit,
                        row,
                        Outcome::DeletedHere,
                        what(&format!("output to {to}")),
                    );
                }
                _ => {
                    let (o, t) = (Some(out_text(ours)), Some(out_text(theirs)));
                    self.push_with(unit, row, out, what("output"), o, t);
                }
            }
        }

        let sends = [
            self.sends(Side::Base, b),
            self.sends(Side::Theirs, t),
            self.sends(Side::Ours, o),
        ];
        let targets: BTreeSet<usize> = sends.iter().flat_map(|s| s.keys().copied()).collect();
        for k in targets {
            let to = self.strip_label(k);
            let what = what(&format!("send to {}", to.trim_start_matches("Strip ")));
            let v = sends.each_ref().map(|s| s.get(&k).copied());
            let Some(out) = outcome_of(&v, send_eq) else {
                continue;
            };
            let [_, theirs, ours] = &v;
            // Theirs' send to a bus ours deleted is not made.
            if self.deleted_here(k)
                && theirs.is_some()
                && matches!(out, Outcome::Theirs | Outcome::Conflict)
            {
                self.push(Unit::Send(k), row, Outcome::DeletedHere, what);
            } else {
                let (o, t) = (Some(send_text(ours)), Some(send_text(theirs)));
                self.push_with(Unit::Send(k), row, out, what, o, t);
            }
        }

        // The instrument first: the merge never changes it, so theirs' swap
        // is listed and not applied.
        let [[ib, db], [it, dt], [io, dov]] = [
            self.split_devices(Side::Base, b),
            self.split_devices(Side::Theirs, t),
            self.split_devices(Side::Ours, o),
        ];
        let instruments = [ib, it, io];
        if let Some(out) = outcome_of(&instruments, |x, y| x == y) {
            let [_, theirs, ours] = &instruments;
            let (o, t) = (Some(devices_text(ours)), Some(devices_text(theirs)));
            if out == Outcome::Theirs {
                self.push_not_applied(Unit::Instrument, row, what("instrument"), o, t);
            } else {
                self.push_with(Unit::Instrument, row, out, what("instrument"), o, t);
            }
        }
        let devices = [db, dt, dov];
        let unhosted = self
            .theirs_strips
            .strips
            .get(t)
            .and_then(|s| s.strip)
            .and_then(|id| self.theirs_origins.unhosted.get(&StripId(id)));
        let [db, dt, dov] = &devices;
        // Ruling 10: effects the file names that VizDAW cannot host, other
        // than ones base already had.
        let new_unhosted: Vec<&(String, Option<String>)> = unhosted
            .map(|v| {
                v.iter()
                    .filter(|(_, id)| {
                        id.as_ref()
                            .is_none_or(|id| !bs.devices.iter().any(|d| &d.1 == id))
                    })
                    .collect()
            })
            .unwrap_or_default();
        match new_unhosted {
            // Theirs' list changed and names them: ours stays, listed.
            names if !names.is_empty() && db != dt => {
                let (ours, theirs) = (Some(devices_text(dov)), Some(devices_text(dt)));
                self.push_with(
                    Unit::Devices,
                    row,
                    Outcome::Ours,
                    what("plug-ins"),
                    ours,
                    theirs,
                );
                for (name, id) in names {
                    let vst = id.is_none().then(|| name.clone());
                    self.vst.push((format!("{label}: {name}"), vst));
                }
            }
            _ => self.three(
                Unit::Devices,
                row,
                what("plug-ins"),
                devices.clone(),
                |x, y| x == y,
                |x| devices_text(x),
            ),
        }
        for (i, base_dev) in bs.devices.iter().enumerate() {
            let same_device = |s: &PStrip| {
                s.devices
                    .get(i)
                    .is_some_and(|d| d.0 == base_dev.0 && d.1 == base_dev.1)
            };
            if !(same_device(ts) && same_device(os)) {
                continue;
            }
            let dev = |s: &PStrip| s.devices.get(i).cloned();
            let v = all.map(dev);
            let plugin = base_dev.1.clone();
            self.three(
                Unit::Bypass(i),
                row,
                what(&format!("bypass of {plugin}")),
                v.clone().map(|d| d.is_some_and(|d| d.2)),
                |x, y| x == y,
                |x| text::on_off(*x).to_owned(),
            );
            let states = v.map(|d| d.and_then(|d| d.3));
            let Some(out) = outcome_of(&states, |x, y| x == y) else {
                continue;
            };
            let show = |x: &Option<u64>| {
                x.map_or_else(|| "none".to_owned(), |h| format!("settings {h:016x}"))
            };
            let [_, theirs, ours] = &states;
            let what = what(&format!("settings of {plugin}"));
            let (o, t) = (Some(show(ours)), Some(show(theirs)));
            // A state theirs dropped: no command clears one, so ours stays.
            if out == Outcome::Theirs && theirs.is_none() {
                self.push_not_applied(Unit::State(i), row, what, o, t);
            } else {
                self.push_with(Unit::State(i), row, out, what, o, t);
            }
        }
    }

    // ------------------------------------------------------------ markers

    fn marker_of(&self, s: Side, i: usize) -> Option<&'a (i64, String)> {
        self.side(s).markers.get(i)
    }

    fn marker_text(&self, s: Side, m: &(i64, String)) -> String {
        format!("{} at bar {}", m.1, self.bar(s, m.0))
    }

    fn marker(&mut self, row: usize) {
        let Some(r) = self.m.markers.get(row).copied() else {
            return;
        };
        let eq = |a: &(i64, String), b: &(i64, String)| a.1 == b.1 && ticks_near(a.0, b.0);
        let name = [
            (Side::Ours, r.ours),
            (Side::Base, r.base),
            (Side::Theirs, r.theirs),
        ]
        .into_iter()
        .find_map(|(s, i)| Some(self.marker_of(s, i?)?.1.clone()))
        .unwrap_or_default();
        let what = format!("Marker '{name}'");
        let get = |s: Side, i: Option<usize>| i.and_then(|i| self.marker_of(s, i));
        let (b, t, o) = (
            get(Side::Base, r.base),
            get(Side::Theirs, r.theirs),
            get(Side::Ours, r.ours),
        );
        let out = match (b, t, o) {
            (Some(b), Some(t), Some(o)) => {
                let Some(out) = outcome(eq(b, t), eq(b, o), || eq(t, o)) else {
                    return;
                };
                let ours = Some(self.marker_text(Side::Ours, o));
                let theirs = Some(self.marker_text(Side::Theirs, t));
                self.push_with(Unit::Marker, row, out, what, ours, theirs);
                return;
            }
            (Some(b), None, Some(o)) => {
                if eq(b, o) {
                    Outcome::Removed
                } else {
                    Outcome::DeletedThere
                }
            }
            (Some(b), Some(t), None) => {
                if eq(b, t) {
                    Outcome::Ours
                } else {
                    Outcome::DeletedHere
                }
            }
            (Some(_), None, None) => Outcome::Same,
            (None, Some(_), None) => Outcome::Added,
            (None, None, Some(_)) => Outcome::Ours,
            // New on both sides with one name and tick (`pair_twins`):
            // the same file merged before.
            (None, Some(t), Some(o)) => {
                if eq(t, o) {
                    Outcome::Same
                } else {
                    Outcome::Conflict
                }
            }
            _ => return,
        };
        self.push(Unit::Marker, row, out, what);
    }

    // ------------------------------------------------------------ maps and info

    fn maps(&mut self) {
        let tempo_text = |s: Side, v: &[(i64, u32, bool)]| {
            v.iter()
                .map(|p| format!("{} at bar {}", text::bpm(p.1), self.bar(s, p.0)))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let [b, t, o] = self.p;
        if let Some(out) = outcome(
            tempo_eq(&b.tempo, &t.tempo),
            tempo_eq(&b.tempo, &o.tempo),
            || tempo_eq(&t.tempo, &o.tempo),
        ) {
            let ours = Some(tempo_text(Side::Ours, &o.tempo));
            let theirs = Some(tempo_text(Side::Theirs, &t.tempo));
            self.push_with(Unit::Tempo, 0, out, "Tempo map".to_owned(), ours, theirs);
        }
        let meter_text = |s: Side, v: &[(i64, u8, u8)]| {
            v.iter()
                .map(|m| format!("{}/{} at bar {}", m.1, m.2, self.bar(s, m.0)))
                .collect::<Vec<_>>()
                .join("; ")
        };
        if let Some(out) = outcome(
            meter_eq(&b.meter, &t.meter),
            meter_eq(&b.meter, &o.meter),
            || meter_eq(&t.meter, &o.meter),
        ) {
            let ours = Some(meter_text(Side::Ours, &o.meter));
            let theirs = Some(meter_text(Side::Theirs, &t.meter));
            self.push_with(Unit::Meter, 0, out, "Meter map".to_owned(), ours, theirs);
        }
    }

    fn info(&mut self) {
        let [b, t, o] = self.p;
        let n = b.info.len().max(t.info.len()).max(o.info.len());
        for i in 0..n {
            let v = [b, t, o].map(|p| p.info.get(i).cloned().flatten());
            self.three(
                Unit::Info(i),
                i,
                format!("Song info: {}", info_name(i)),
                v,
                |x, y| x == y,
                |x| x.clone().unwrap_or_else(|| "empty".to_owned()),
            );
        }
    }

    // ------------------------------------------------------------ the lines

    /// Adds the decisions' report lines to `report` and counts the
    /// preview. Also returns the lanes for `text::TAKE_PARTS`: split tracks
    /// with one part's notes or regions in conflict and not the other's.
    fn listed(&self, report: &mut Report) -> (MergePreview, Vec<String>) {
        let mut p = MergePreview::default();
        let bump = |n: &mut u32| *n = n.saturating_add(1);
        for (at, d) in self.out.iter().enumerate() {
            let take = matches!(d.unit, Unit::Notes | Unit::Regions);
            // A part new on both sides (`pair_twins`); a marker on all three
            // sides is a setting.
            let part = match d.unit {
                Unit::Track | Unit::Bus => true,
                Unit::Marker => self.m.markers.get(d.row).is_some_and(|r| r.base.is_none()),
                _ => false,
            };
            match d.outcome {
                Outcome::Theirs | Outcome::Same => bump(&mut p.applied),
                Outcome::Ours => bump(&mut p.kept),
                Outcome::Added => bump(&mut p.added),
                Outcome::Removed => bump(&mut p.removed),
                Outcome::Conflict => {
                    bump(&mut p.conflicts);
                    if take {
                        p.conflict_lines.push(d.what.clone());
                    } else if self.not_applied.contains(&at) {
                        let line = if self.plain.contains(&at) {
                            d.what.clone()
                        } else {
                            text::conflict_setting(
                                &d.what,
                                d.ours.as_deref().unwrap_or("none"),
                                d.theirs.as_deref().unwrap_or("none"),
                            )
                        };
                        report.add(ItemKind::Conflict, text::NOT_APPLIED, Some(line.clone()));
                        p.conflict_lines.push(line);
                    } else if part {
                        report.add(ItemKind::Conflict, text::ADDED_BOTH, Some(d.what.clone()));
                        p.conflict_lines.push(d.what.clone());
                    } else {
                        let line = text::conflict_setting(
                            &d.what,
                            d.ours.as_deref().unwrap_or("none"),
                            d.theirs.as_deref().unwrap_or("none"),
                        );
                        report.add(
                            ItemKind::Conflict,
                            text::CONFLICT_SETTING,
                            Some(line.clone()),
                        );
                        p.conflict_lines.push(line);
                    }
                }
                Outcome::KeptEmpty | Outcome::DeletedThere | Outcome::DeletedHere => {
                    bump(&mut p.conflicts);
                    let what = match d.outcome {
                        Outcome::KeptEmpty => text::KEPT_EMPTY,
                        Outcome::DeletedThere => text::DELETED_THERE,
                        _ => text::DELETED_HERE,
                    };
                    report.add(ItemKind::Conflict, what, Some(d.what.clone()));
                    p.conflict_lines.push(d.what.clone());
                }
            }
        }
        (p, self.take_parts())
    }

    /// The lanes (by name) of split tracks where one part's notes or
    /// regions are in conflict and the other part's are not.
    fn take_parts(&self) -> Vec<String> {
        let lane_of = |row: usize| -> Option<u64> {
            let o = self.m.tracks.get(row)?.ours?;
            Some(self.ids_of(Side::Ours, o)?.lane)
        };
        let mut parts: HashMap<u64, usize> = HashMap::new();
        for row in 0..self.m.tracks.len() {
            if let Some(l) = lane_of(row) {
                *parts.entry(l).or_default() += 1;
            }
        }
        let mut in_conflict: BTreeMap<u64, BTreeSet<usize>> = BTreeMap::new();
        for d in &self.out {
            if d.outcome == Outcome::Conflict
                && matches!(d.unit, Unit::Notes | Unit::Regions)
                && let Some(l) = lane_of(d.row)
            {
                in_conflict.entry(l).or_default().insert(d.row);
            }
        }
        let mut out = Vec::new();
        for (lane, rows) in in_conflict {
            if parts.get(&lane).copied().unwrap_or(0) > rows.len()
                && let Ok(l) = self.ours_doc.lane(self.seq, LaneId(lane))
            {
                out.push(l.name.clone());
            }
        }
        out
    }
}

fn send_eq(a: &Option<Send>, b: &Option<Send>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => level_eq(x.0, y.0) && x.1 == y.1 && x.2 == y.2,
        (None, None) => true,
        _ => false,
    }
}

fn send_text(s: &Option<Send>) -> String {
    match s {
        None => "none".to_owned(),
        Some((level, pre, on)) => {
            let mut t = text::level(*level);
            if *pre {
                t.push_str(", pre-fader");
            }
            if !*on {
                t.push_str(", off");
            }
            t
        }
    }
}

fn devices_text(d: &[(&str, &str)]) -> String {
    if d.is_empty() {
        "none".to_owned()
    } else {
        d.iter().map(|(_, id)| *id).collect::<Vec<_>>().join(", ")
    }
}
