//! MIDI export (plan 6a Task 6): the document → a Standard MIDI File
//! ([`to_smf`]) or a MIDI Clip File ([`to_clip`]).
//!
//! What is written is what plays (Ruling 4): the chosen tracks are
//! flattened with their mute and solo cleared (event mutes, play quantize,
//! shift, own length and loop applied, active take only), so a muted or
//! unsoloed track is still written, with a Note. Text and meta events
//! follow the one table in [`crate::midi_in`] (F21). File position 0 is
//! the pickup's start (Ruling 8).
//!
//! Events at one tick are ordered by Ruling 10: meta and Flex Data first,
//! then channel messages by [`Class`] (note-off, patch, control, note-on),
//! SysEx after the other control messages, otherwise in the order
//! `translate` gives them. Per-note messages follow their note-on.

use crate::IoError;
use crate::clip::{ClipEvent, ClipFile};
use crate::report::{ItemKind, Report};
use crate::smf::{Division, SmfEvent, SmfFile, SmfTrack};
use crate::ump::{self, BANK_METADATA, BANK_PERFORMANCE, MAX_FLEX_TEXT_BYTES};
use core_engine::input_fx::{CENTER, Expr, LiveMsg, NoteExpr, up7, up32};
use core_engine::midi1::{
    Class, Midi1Msg, bend14, note_on_velocity, release_velocity, translate, value7,
};
use core_engine::mpe::{MpeEncoder, configuration};
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, PerNoteKind, TextKind};
use core_model::ids::{DestId, EventId, LaneId, SeqId};
use core_model::lane::{Lane, LaneInput};
use core_model::meter::{KeySig, Meter};
use core_model::mpe::MpeConfig;
use core_model::query::{ResolvedEvent, flatten, seq_length};
use core_model::rules::MAX_TICKS;
use core_model::selection::ViewFilter;
use core_model::sequence::{SeqLength, Sequence};
use core_model::tempo::TempoMap;
use core_model::time::{PPQ, Tick};
use std::collections::{BTreeMap, BTreeSet};

/// What a Standard MIDI File export writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmfOptions {
    /// The sequence.
    pub seq: SeqId,
    /// The tracks to write, in any order; empty: every track.
    pub lanes: Vec<LaneId>,
    /// Write type 0 (one track) instead of type 1.
    pub type0: bool,
    /// Ticks per quarter: 480, 960 or 15 360.
    pub ppq: u16,
    /// Write per-note expression as MPE where the destination uses MPE.
    pub mpe: bool,
}

/// What a MIDI Clip File export writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipOptions {
    /// The sequence.
    pub seq: SeqId,
    /// The track.
    pub lane: LaneId,
    /// The events starting in `[start, end)`, moved to start at 0; `None`:
    /// the whole sequence.
    pub range: Option<(Tick, Tick)>,
}

const NO_SEQ: &str = "The sequence to export is not in this project.";
const NO_TRACK: &str = "A track to export is not in this sequence.";
const BAD_PPQ: &str = "MIDI files are written at 480, 960 or 15 360 ticks per quarter note.";
const EMPTY_RANGE: &str = "The range to export is empty.";

const VELOCITIES: &str = "Velocities rounded to 7 bits (MIDI file)";
const VALUES: &str = "Controller, pressure and pitch bend values rounded (MIDI file)";
const MUTED: &str = "Muted notes";
const OUTSIDE: &str = "Events outside the sequence";
const ATTRIBUTES: &str = "Note attributes";
const SYSEX8: &str = "8-bit SysEx";
const DRUM_ROWS: &str = "Drum row names";
const OTHER_INFO: &str = "Song info other than title and copyright";
const ZERO: &str = "Notes of zero length";
const FAR: &str = "Events too far apart for a MIDI file";
const PICKUP_BAR: &str = "Pickup written as a full bar";
const FLEX_CUT: &str = "Texts over 384 bytes cut";
const BANKS: &str = "Bank selects merged into program changes";
const MPE_OTHER: &str = "Per-note controllers MPE does not carry";
const PER_NOTE_LATE: &str = "Per-note values at or after the end of their note";
const STOLEN: &str = "Notes cut short: more notes than MPE member channels";
const TYPE0_NAMES: &str = "Track names (type 0 file)";
const TYPE0_GROUPS: &str = "Groups 2 to 16 written as group 1 (type 0 file)";
const INACTIVE_TAKES: &str = "Inactive takes";
const MPE_INPUT: &str = "MPE input settings are not in MIDI files";
const INPUT_FX: &str = "Input effects are not in MIDI files";
const INPUTS: &str = "Track inputs and record arm are not in MIDI files";
const SYNTH_TRACKING: &str = "Synth tracking (Track Synth) is not in MIDI files";
const OTHER_SEQS: &str = "Other sequences are not in this file";
const ENDED_EARLY: &str = "Notes ended early: too far apart for a MIDI file";
const CUT_AT_END: &str = "Notes cut at the end of the range";
const STATE_BEFORE: &str = "Program and controller settings before the range are not written";
const SHORT_BAR: &str =
    "Bar 1 is shorter than bar 2; other programs, and VizDAW, may read it as a pickup";

/// A tempo ramp is written as a step every 1/16 note.
const STEP: i64 = PPQ / 4;
/// At most this many steps per ramp; a longer ramp takes longer steps.
const MAX_STEPS: i64 = 65_536;
/// The largest delta a 4-byte variable-length quantity holds.
const MAX_DELTA: u64 = 0x0FFF_FFFF;

fn refused(s: &str) -> IoError {
    IoError::Refused(s.to_string())
}

/// Counts `n` occurrences of `what` at once.
fn add_n(report: &mut Report, kind: ItemKind, what: &str, n: u64) {
    if n == 0 {
        return;
    }
    report.add(kind, what, None);
    if let Some(i) = report
        .items
        .iter_mut()
        .find(|i| i.kind == kind && i.what == what)
    {
        i.count = i.count.saturating_add(n - 1);
    }
}

// ---- what plays ----

/// The chosen tracks of a sequence and what they play.
struct Source<'a> {
    doc: &'a Document,
    seq: &'a Sequence,
    /// The chosen tracks, in display order.
    lanes: Vec<&'a Lane>,
    /// What each plays, by tick (stable).
    played: BTreeMap<LaneId, Vec<ResolvedEvent>>,
}

/// The tracks `chosen` (empty: all) of sequence `seq`, flattened with
/// their mute and every solo of the sequence cleared, the other tracks
/// hidden. Notes muted tracks, and counts what `flatten` drops silently:
/// muted notes and events outside the sequence (F10).
fn source<'a>(
    doc: &'a Document,
    seq: SeqId,
    chosen: &[LaneId],
    report: &mut Report,
) -> Result<Source<'a>, IoError> {
    let s = doc.sequence(seq).map_err(|_| refused(NO_SEQ))?;
    if chosen.iter().any(|l| s.lane(*l).is_none()) {
        return Err(refused(NO_TRACK));
    }
    let picked = |l: &Lane| chosen.is_empty() || chosen.contains(&l.id);
    let lanes: Vec<&Lane> = s.lanes.iter().filter(|l| picked(l)).collect();
    let any_solo = s.lanes.iter().any(|l| l.solo);
    for l in &lanes {
        if l.mute {
            let what = format!("Track '{}' is muted in the project", l.name);
            report.add(ItemKind::Note, &what, None);
        } else if any_solo && !l.solo {
            let what = format!("Track '{}' is silenced by a solo in the project", l.name);
            report.add(ItemKind::Note, &what, None);
        }
    }
    let mut copy = doc.clone();
    for l in &s.lanes {
        let mute = (picked(l) && l.mute).then_some(false);
        let solo = l.solo.then_some(false);
        if mute.is_some() || solo.is_some() {
            let cmd = Command::Lane(LaneCmd::SetFlags {
                seq,
                lane: l.id,
                mute,
                solo,
                arm: None,
            });
            copy = apply(&copy, &cmd)
                .map_err(|e| IoError::Refused(format!("This MIDI file can't be written: {e}")))?
                .doc;
        }
    }
    let filter = ViewFilter {
        hidden_kinds: Default::default(),
        hidden_lanes: s
            .lanes
            .iter()
            .filter(|l| !picked(l))
            .map(|l| l.id)
            .collect(),
    };
    let mut played: BTreeMap<LaneId, Vec<ResolvedEvent>> = BTreeMap::new();
    let events = flatten(&copy, seq, None, &filter).map_err(|_| refused(NO_SEQ))?;
    for r in events {
        if let Some(step) = r.path.first() {
            played.entry(step.lane).or_default().push(r);
        }
    }

    vizdaw_only(doc, s, &lanes, &played, report);
    cut_reference_notes(&copy, s, &lanes, &filter, report);

    let mut stack = vec![seq];
    let muted = muted_notes(doc, lanes.iter().copied(), true, &mut stack);
    add_n(report, ItemKind::LeftOut, MUTED, muted);
    let len = seq_length(doc, seq).map_or(i64::MAX, |t| t.0);
    let lo = s.start_offset.0.checked_neg().unwrap_or(i64::MIN);
    for l in &lanes {
        let Some(take) = l.active() else { continue };
        let limit = l.length.map_or(i64::MAX, |t| t.0);
        for e in take.events.values() {
            if e.mute || e.tick.0 >= limit || !carried(&e.body) {
                continue;
            }
            let mut lt = e.tick;
            if let (Some(q), EventBody::Note { .. }) = (&l.play_quantize, &e.body) {
                lt = q.apply(lt);
            }
            let is_ref = matches!(e.body, EventBody::Reference { .. });
            let outside = match lt.0.checked_add(l.play_shift.0) {
                Some(lt) => (lt < lo && !is_ref) || lt >= len,
                None => true,
            };
            if outside {
                report.add(ItemKind::LeftOut, OUTSIDE, Some(l.name.clone()));
            }
        }
    }
    Ok(Source {
        doc,
        seq: s,
        lanes,
        played,
    })
}

/// What export does not carry beyond the events (spec §7.1's listed
/// rows): a MIDI strip's mute, inactive takes, and VizDAW-only settings.
fn vizdaw_only(
    doc: &Document,
    s: &Sequence,
    lanes: &[&Lane],
    played: &BTreeMap<LaneId, Vec<ResolvedEvent>>,
    report: &mut Report,
) {
    let used: BTreeSet<DestId> = played
        .values()
        .flatten()
        .filter(|r| r.body.is_midi())
        .filter_map(|r| r.dest)
        .collect();
    for d in used.iter().filter_map(|d| doc.instruments().get(d)) {
        if d.kind.is_external() && d.midi_mix.mute {
            let what = format!(
                "{} is muted in its MIDI strip; its notes were written anyway",
                d.name
            );
            report.add(ItemKind::Note, &what, None);
        }
    }
    for l in lanes {
        let name = || Some(l.name.clone());
        for t in l.takes.iter() {
            if t.id != l.active_take && !t.events.is_empty() {
                report.add(ItemKind::LeftOut, INACTIVE_TAKES, name());
            }
        }
        if l.mpe.is_some() {
            report.add(ItemKind::Note, MPE_INPUT, name());
        }
        if !l.input_fx.is_empty() {
            report.add(ItemKind::Note, INPUT_FX, name());
        }
        if l.input.is_some() || l.arm {
            report.add(ItemKind::Note, INPUTS, name());
        }
        if l.input.as_ref().and_then(LaneInput::return_of).is_some() {
            report.add(ItemKind::Note, SYNTH_TRACKING, name());
        }
    }
    // Sequences the export reaches through references, in any take.
    let mut reached: BTreeSet<SeqId> = BTreeSet::from([s.id]);
    let mut todo: Vec<SeqId> = lanes.iter().flat_map(|l| refs_of(l)).collect();
    while let Some(id) = todo.pop() {
        if reached.insert(id)
            && let Ok(t) = doc.sequence(id)
        {
            todo.extend(t.lanes.iter().flat_map(refs_of));
        }
    }
    let others = doc
        .sequences()
        .keys()
        .filter(|id| !reached.contains(id))
        .count();
    add_n(
        report,
        ItemKind::Note,
        OTHER_SEQS,
        u64::try_from(others).unwrap_or(u64::MAX),
    );
}

/// The sequences `l`'s active take references.
fn refs_of(l: &Lane) -> Vec<SeqId> {
    l.active()
        .into_iter()
        .flat_map(|t| t.events.values())
        .filter_map(|e| match e.body {
            EventBody::Reference { seq, .. } => Some(seq),
            _ => None,
        })
        .collect()
}

/// Counts as "Events outside the sequence" the events inside references
/// that the sequence's window cuts off: `flatten` of `copy` (mutes and
/// solos already cleared) with the window opened to the timeline's limits
/// and loops off, against the real window. Only events of references whose
/// own start plays are counted (a reference starting outside is counted
/// once, by the caller's pass).
fn cut_reference_notes(
    copy: &Document,
    s: &Sequence,
    lanes: &[&Lane],
    filter: &ViewFilter,
    report: &mut Report,
) {
    if !lanes.iter().any(|l| !refs_of(l).is_empty()) {
        return;
    }
    let (seq, len) = (s.id, seq_length(copy, s.id).map_or(i64::MAX, |t| t.0));
    let lo = s.start_offset.0.checked_neg().unwrap_or(i64::MIN);
    let mut wide = copy.clone();
    let mut cmds = vec![
        Command::Sequence(SequenceCmd::SetStartOffset {
            seq,
            offset: Tick(MAX_TICKS),
        }),
        Command::Sequence(SequenceCmd::SetLength {
            seq,
            length: SeqLength::Locked(Tick(MAX_TICKS)),
        }),
    ];
    for l in lanes.iter().filter(|l| l.looped) {
        cmds.push(Command::Lane(LaneCmd::SetLength {
            seq,
            lane: l.id,
            length: l.length,
            looped: false,
            locked: l.length_locked,
        }));
    }
    for cmd in &cmds {
        match apply(&wide, cmd) {
            Ok(a) => wide = a.doc,
            Err(_) => return,
        }
    }
    let Ok(events) = flatten(&wide, seq, None, filter) else {
        return;
    };
    // Where each top-level reference starts, as it plays.
    let starts: BTreeMap<EventId, i64> = lanes
        .iter()
        .flat_map(|l| {
            l.active()
                .into_iter()
                .flat_map(|t| t.events.values())
                .filter(|e| matches!(e.body, EventBody::Reference { .. }))
                .filter_map(|e| Some((e.id, e.tick.0.checked_add(l.play_shift.0)?)))
        })
        .collect();
    for r in events {
        let [top, _, ..] = r.path.as_slice() else {
            continue;
        };
        let plays = starts.get(&top.event).is_some_and(|t| *t < len);
        if plays && carried(&r.body) && (r.tick.0 < lo || r.tick.0 >= len) {
            report.add(ItemKind::LeftOut, OUTSIDE, None);
        }
    }
}

/// Bar 1 shorter than bar 2 with no pickup: Ruling 8 reads it back as a
/// pickup, here and elsewhere (a Note).
fn short_first_bar(s: &Sequence, report: &mut Report) {
    let e = s.meter.entries();
    if let (Some(a), Some(b)) = (e.front(), e.get(1))
        && s.start_offset.0 == 0
        && b.tick.0 == a.meter.bar_ticks()
        && a.meter.bar_ticks() < b.meter.bar_ticks()
    {
        report.add(ItemKind::Note, SHORT_BAR, None);
    }
}

/// Whether a MIDI file carries this kind of event.
fn carried(b: &EventBody) -> bool {
    !matches!(
        b,
        EventBody::AudioRegion { .. } | EventBody::Automation { .. }
    )
}

/// Notes of `lanes` that do not play because they or a reference holding
/// them are muted; inside references, notes of muted or unsoloed tracks
/// too. `top` tracks play whatever their own mute and solo.
fn muted_notes<'a>(
    doc: &Document,
    lanes: impl Iterator<Item = &'a Lane>,
    top: bool,
    stack: &mut Vec<SeqId>,
) -> u64 {
    let lanes: Vec<&Lane> = lanes.collect();
    let any_solo = lanes.iter().any(|l| l.solo);
    let mut n = 0u64;
    for l in &lanes {
        let silent = !top && (l.mute || (any_solo && !l.solo));
        let Some(take) = l.active() else { continue };
        for e in take.events.values() {
            let add = match &e.body {
                EventBody::Note { .. } => u64::from(silent || e.mute),
                EventBody::Reference { seq, .. } => {
                    if stack.contains(seq) {
                        0
                    } else if let Ok(target) = doc.sequence(*seq) {
                        stack.push(*seq);
                        let k = if silent || e.mute {
                            all_notes(doc, target, stack)
                        } else {
                            muted_notes(doc, target.lanes.iter(), false, stack)
                        };
                        stack.pop();
                        k
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            n = n.saturating_add(add);
        }
    }
    n
}

/// Every note of `s`'s active takes, through its references.
fn all_notes(doc: &Document, s: &Sequence, stack: &mut Vec<SeqId>) -> u64 {
    let mut n = 0u64;
    for e in s
        .lanes
        .iter()
        .filter_map(Lane::active)
        .flat_map(|t| t.events.values())
    {
        let add = match &e.body {
            EventBody::Note { .. } => 1,
            EventBody::Reference { seq, .. } if !stack.contains(seq) => match doc.sequence(*seq) {
                Ok(target) => {
                    stack.push(*seq);
                    let k = all_notes(doc, target, stack);
                    stack.pop();
                    k
                }
                Err(_) => 0,
            },
            _ => 0,
        };
        n = n.saturating_add(add);
    }
    n
}

/// Whether the track writes anything: an audio track (its take holds
/// regions and it plays nothing a MIDI file carries) does not.
fn writes(lane: &Lane, played: &[ResolvedEvent]) -> bool {
    let audio = lane.active().is_some_and(|t| {
        t.events
            .values()
            .any(|e| matches!(e.body, EventBody::AudioRegion { .. }))
    });
    !audio || played.iter().any(|r| carried(&r.body))
}

/// Song info a MIDI file does not carry; the title only when it is not the
/// sequence's name (which is written as the title).
fn other_info(doc: &Document, s: &Sequence, report: &mut Report) {
    let info = &doc.settings().info;
    let set = info.fields().iter().filter(|f| f.is_some()).count();
    let kept = usize::from(info.copyright.is_some())
        + usize::from(info.title.as_deref() == Some(s.name.as_str()));
    let n = set.saturating_sub(kept);
    add_n(
        report,
        ItemKind::LeftOut,
        OTHER_INFO,
        u64::try_from(n).unwrap_or(u64::MAX),
    );
}

fn drum_rows(lane: &Lane, report: &mut Report) {
    for row in lane.drum.iter().flat_map(|d| d.rows.iter()) {
        report.add(ItemKind::LeftOut, DRUM_ROWS, Some(row.name.clone()));
    }
}

// ---- positions, tempo and meter ----

/// Where file position 0 is: the model tick, and the pickup bar's meter
/// when it has one of its own (Ruling 8).
#[derive(Debug, Clone, Copy)]
struct Origin {
    tick: i64,
    pickup: Option<Meter>,
}

/// File position 0 of a whole sequence: its pickup's start. A pickup of a
/// whole number of beats, shorter than a bar, is a first bar of its own
/// meter; any other pickup is written as full bars (Changed).
fn origin(s: &Sequence, report: &mut Report) -> Origin {
    let so = s.start_offset.0;
    if so <= 0 {
        return Origin {
            tick: 0,
            pickup: None,
        };
    }
    let first = s.meter.entry_at(Tick::ZERO).meter;
    let beat = first.beat_ticks().max(1);
    let bar = first.bar_ticks().max(1);
    if so % beat == 0
        && so < bar
        && let Ok(num) = u8::try_from(so / beat)
    {
        return Origin {
            tick: -so,
            pickup: Some(Meter {
                num,
                den: first.den,
            }),
        };
    }
    report.add(ItemKind::Changed, PICKUP_BAR, None);
    // Whole bars at least as long as the pickup (`so` is within
    // `MAX_TICKS`, so none of this overflows; the pickup itself is the
    // fallback).
    let padded = so
        .checked_add(bar - 1)
        .map(|n| n / bar)
        .and_then(|bars| bars.checked_mul(bar))
        .and_then(i64::checked_neg)
        .unwrap_or(-so);
    Origin {
        tick: padded,
        pickup: None,
    }
}

/// A tempo: exact milli-BPM, or seconds per quarter note (a ramp's step).
#[derive(Debug, Clone, Copy)]
enum Tempo {
    Milli(u32),
    Spq(f64),
}

impl Tempo {
    /// Microseconds per quarter note (SMF).
    fn micros(self) -> u32 {
        let us = match self {
            Tempo::Milli(m) => {
                let m = u64::from(m.max(1));
                (60_000_000_000u64 + m / 2) / m
            }
            Tempo::Spq(s) => (s * 1e6).round().clamp(1.0, 16_777_215.0) as u64,
        };
        u32::try_from(us.clamp(1, 0x00FF_FFFF)).unwrap_or(0x00FF_FFFF)
    }

    /// 10-ns units per quarter note (Clip File).
    fn ten_ns(self) -> u32 {
        match self {
            Tempo::Milli(m) => {
                let m = u64::from(m.max(1));
                u32::try_from((6_000_000_000_000u64 + m / 2) / m).unwrap_or(u32::MAX)
            }
            Tempo::Spq(s) => (s * 1e8).round().clamp(1.0, f64::from(u32::MAX)) as u32,
        }
    }
}

/// One tempo or meter change.
#[derive(Debug, Clone, Copy)]
enum MapEvent {
    Tempo(Tempo),
    Time(Meter),
    Key(KeySig),
}

/// The tempo points, each ramp as steps of [`STEP`] ticks (each as long in
/// time as the stretch of the ramp it stands for), Changed per ramp.
fn tempo_steps(s: &Sequence, report: &mut Report) -> Vec<(i64, Tempo)> {
    let map: &TempoMap = &s.tempo;
    let points: Vec<_> = map.points().iter().copied().collect();
    let mut out = Vec::new();
    for (i, p) in points.iter().enumerate() {
        let next = points.get(i.saturating_add(1)).filter(|_| p.ramp);
        let Some(next) = next else {
            out.push((p.tick.0, Tempo::Milli(p.bpm.0)));
            continue;
        };
        // Points are ordered within `MAX_TICKS`: the span is positive.
        let span = next.tick.0.checked_sub(p.tick.0).unwrap_or(0);
        let step = STEP.max(span / MAX_STEPS + 1);
        let mut t = p.tick.0;
        let mut n = 0u64;
        while t < next.tick.0 {
            let end = t
                .checked_add(step)
                .map_or(next.tick.0, |e| e.min(next.tick.0));
            let secs = map.tick_to_seconds(Tick(end)) - map.tick_to_seconds(Tick(t));
            let len = end.checked_sub(t).unwrap_or(1).max(1);
            out.push((t, Tempo::Spq(secs * PPQ as f64 / len as f64)));
            n = n.saturating_add(1);
            t = end;
        }
        let bar = s.meter.tick_to_bbu(p.tick, 1).bar;
        let unit = if n == 1 { "step" } else { "steps" };
        let what = format!("Tempo ramp at bar {bar} written as {n} {unit}");
        report.add(ItemKind::Changed, &what, None);
    }
    out
}

/// The tempo, meter and key changes from `o.tick` (before `end`), at ticks
/// from it: what is in force there at 0 (tempo, time, key), then each
/// change after it.
fn map_changes(
    s: &Sequence,
    o: Origin,
    end: Option<i64>,
    report: &mut Report,
) -> Vec<(i64, MapEvent)> {
    let inside = |t: i64| t > o.tick && end.is_none_or(|e| t < e);
    // `t > o.tick`, both within `MAX_TICKS` of 0: no overflow.
    let rel = |t: i64| t.checked_sub(o.tick).unwrap_or(0);
    let mut out = Vec::new();
    let steps = tempo_steps(s, report);
    let first = steps
        .iter()
        .take_while(|(t, _)| *t <= o.tick)
        .last()
        .or_else(|| steps.first());
    if let Some((_, v)) = first {
        out.push((0, MapEvent::Tempo(*v)));
    }
    out.extend(
        steps
            .iter()
            .filter(|(t, _)| inside(*t))
            .map(|(t, v)| (rel(*t), MapEvent::Tempo(*v))),
    );
    let at = s.meter.entry_at(Tick(o.tick));
    let mut meter = at.meter;
    let mut key = at.key;
    out.push((0, MapEvent::Time(o.pickup.unwrap_or(meter))));
    out.push((0, MapEvent::Key(key)));
    if o.pickup.is_some() && inside(0) {
        out.push((rel(0), MapEvent::Time(meter)));
    }
    for e in s.meter.entries().iter().filter(|e| inside(e.tick.0)) {
        if e.meter != meter {
            meter = e.meter;
            out.push((rel(e.tick.0), MapEvent::Time(meter)));
        }
        if e.key != key {
            key = e.key;
            out.push((rel(e.tick.0), MapEvent::Key(key)));
        }
    }
    out
}

// ---- ordering ----

/// The order of events at one tick (Ruling 10): metas and Flex Data first
/// (by their own sub-order), then channel messages by class, then within a
/// class by `sub` (SysEx after the control messages, per-note messages
/// after their note-on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key(u8, u8, u8);

fn meta_key(sub: u8) -> Key {
    Key(0, sub, 0)
}

fn chan_key(class: Class, sub: u8) -> Key {
    Key(1, class as u8, sub)
}

const SUB_PORT: u8 = 0;
const SUB_NAME: u8 = 1;
const SUB_COPYRIGHT: u8 = 2;
const SUB_TEMPO: u8 = 3;
const SUB_TIME: u8 = 4;
const SUB_KEY: u8 = 5;
const SUB_MARKER: u8 = 6;
const SUB_TEXT: u8 = 7;

/// One event to write at `tick` (model ticks from file position 0).
struct Item<E> {
    tick: i64,
    key: Key,
    ev: E,
}

/// Checks a note's velocities and a value's precision for 7 (14) bits.
fn check_values(body: &EventBody, report: &mut Report) {
    let lossy = match *body {
        EventBody::Note {
            velocity,
            release_velocity: rel,
            ..
        } => {
            if up7(note_on_velocity(velocity)) != velocity || up7(release_velocity(rel)) != rel {
                report.add(ItemKind::Changed, VELOCITIES, None);
            }
            false
        }
        EventBody::Cc { value, .. }
        | EventBody::ChannelPressure { value }
        | EventBody::PolyPressure { value, .. } => up32(u32::from(value7(value)), 7) != value,
        EventBody::PitchBend { value } => {
            let (lsb, msb) = bend14(value);
            up32(u32::from(lsb) | (u32::from(msb) << 7), 14) != value
        }
        _ => false,
    };
    if lossy {
        report.add(ItemKind::Changed, VALUES, None);
    }
}

/// The SMF meta type of a text kind.
fn text_meta(kind: TextKind) -> u8 {
    match kind {
        TextKind::Text => 0x01,
        TextKind::Copyright => 0x02,
        TextKind::Lyric => 0x05,
        TextKind::Marker => 0x06,
        TextKind::Cue => 0x07,
    }
}

fn smf_meta(kind: u8, data: Vec<u8>) -> SmfEvent {
    SmfEvent::Meta { kind, data }
}

fn map_meta(e: MapEvent) -> (u8, SmfEvent) {
    match e {
        MapEvent::Tempo(t) => {
            let [_, a, b, c] = t.micros().to_be_bytes();
            (SUB_TEMPO, smf_meta(0x51, vec![a, b, c]))
        }
        MapEvent::Time(m) => {
            let pow = m.den.max(1).trailing_zeros() as u8;
            let clocks = (96 / u16::from(m.den.max(1))).max(1) as u8;
            (SUB_TIME, smf_meta(0x58, vec![m.num, pow, clocks, 8]))
        }
        MapEvent::Key(k) => (
            SUB_KEY,
            smf_meta(0x59, vec![k.sharps as u8, u8::from(k.minor)]),
        ),
    }
}

/// `(group, channel)` of a MIDI event; with none, group 1 channel 1 and a
/// Note naming the track (once).
fn routed(r: &ResolvedEvent, lane: &Lane, noted: &mut bool, report: &mut Report) -> (u8, u8) {
    match r.group_channel {
        Some((g, c)) => (g & 0x0F, c & 0x0F),
        None => {
            if !*noted {
                *noted = true;
                let what = format!(
                    "Track '{}' has events with no instrument; they were written on channel 1",
                    lane.name
                );
                report.add(ItemKind::Note, &what, None);
            }
            (0, 0)
        }
    }
}

/// The channel a destination's patch and mix go on, and its group; none
/// for a plug-in instrument outside MPE (as the engine sends them).
fn preamble_channel(doc: &Document, dest: DestId, mpe: Option<MpeConfig>) -> Option<(u8, u8)> {
    let d = doc.instruments().get(&dest)?;
    let group = match d.kind {
        DestKind::MidiEndpoint { group, .. } => group & 0x0F,
        DestKind::PluginInstrument { .. } => 0,
    };
    match (&d.kind, mpe) {
        (_, Some(cfg)) => Some((group, cfg.manager())),
        (DestKind::MidiEndpoint { channel, .. }, None) => Some((group, channel & 0x0F)),
        _ => None,
    }
}

/// A destination's default patch and MIDI mix (external destinations
/// only, as the engine sends it) as events at file position 0.
fn preamble_bodies(doc: &Document, dest: DestId) -> Vec<EventBody> {
    let Some(d) = doc.instruments().get(&dest) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(p) = d.default_patch {
        out.push(EventBody::Program {
            program: p.program,
            bank: p.bank,
        });
    }
    if d.kind.is_external() {
        for (index, v) in [(7u8, d.midi_mix.volume), (10, d.midi_mix.pan)] {
            if let Some(v) = v {
                out.push(EventBody::Cc {
                    index,
                    value: up32(u32::from(v & 0x7F), 7),
                });
            }
        }
    }
    out
}

/// The MIDI 2.0 class of a body, as `translate` sorts its messages.
fn body_class(b: &EventBody) -> Class {
    match b {
        EventBody::Program { .. } => Class::Patch,
        EventBody::Cc { index: 0 | 32, .. } => Class::Patch,
        _ => Class::Control,
    }
}

// ---- Standard MIDI File ----

/// The Standard MIDI File of `o`'s tracks as they play: type 1 with a
/// conductor (title, copyright, tempo, meter, key and markers) and one
/// track per track and group (a group other than 1 gets a port meta and
/// "Name (group N)"), or type 0 with everything in one track. Ticks scale
/// from 15 360 per quarter with round-half-up. Refuses an unknown
/// sequence or track and a `ppq` other than 480, 960 or 15 360.
pub fn to_smf(doc: &Document, o: &SmfOptions) -> Result<(SmfFile, Report), IoError> {
    if !matches!(o.ppq, 480 | 960 | 15_360) {
        return Err(refused(BAD_PPQ));
    }
    let mut report = Report::default();
    let src = source(doc, o.seq, &o.lanes, &mut report)?;
    let s = src.seq;
    other_info(doc, s, &mut report);
    short_first_bar(s, &mut report);
    let origin = origin(s, &mut report);

    let mut conductor: Vec<Item<SmfEvent>> = Vec::new();
    if !s.name.is_empty() {
        conductor.push(Item {
            tick: 0,
            key: meta_key(SUB_NAME),
            ev: smf_meta(0x03, s.name.as_bytes().to_vec()),
        });
    }
    if let Some(c) = &doc.settings().info.copyright {
        conductor.push(Item {
            tick: 0,
            key: meta_key(SUB_COPYRIGHT),
            ev: smf_meta(0x02, c.as_bytes().to_vec()),
        });
    }
    for (tick, e) in map_changes(s, origin, None, &mut report) {
        let (sub, ev) = map_meta(e);
        conductor.push(Item {
            tick,
            key: meta_key(sub),
            ev,
        });
    }
    let mut markers = 0u64;
    for m in &s.markers {
        match m.tick.0.checked_sub(origin.tick).filter(|t| *t >= 0) {
            Some(tick) => {
                markers = markers.saturating_add(1);
                conductor.push(Item {
                    tick,
                    key: meta_key(SUB_MARKER),
                    ev: smf_meta(0x06, m.name.as_bytes().to_vec()),
                });
            }
            None => report.add(ItemKind::LeftOut, OUTSIDE, None),
        }
    }

    let mut w = SmfWriter {
        src: &src,
        origin,
        mpe: o.mpe,
        report: &mut report,
        notes: 0,
        per_note: 0,
    };
    let mut tracks: Vec<(String, u8, Vec<Item<SmfEvent>>)> = Vec::new();
    let mut written = 0u64;
    for lane in &src.lanes {
        let played = src.played.get(&lane.id).map_or(&[][..], Vec::as_slice);
        if !writes(lane, played) {
            continue;
        }
        written = written.saturating_add(1);
        drum_rows(lane, w.report);
        let groups = w.lane(lane, played);
        for (i, (g, items)) in groups.into_iter().enumerate() {
            let name = if i == 0 {
                lane.name.clone()
            } else {
                format!("{} (group {})", lane.name, u16::from(g).saturating_add(1))
            };
            tracks.push((name, g, items));
        }
    }
    let (notes, per_note) = (w.notes, w.per_note);
    if per_note > 0 {
        let unit = if per_note == 1 { "note" } else { "notes" };
        let what = format!("Per-note expression on {per_note} {unit}");
        report.add(ItemKind::LeftOut, &what, None);
    }

    let mut out = Vec::new();
    if o.type0 {
        add_n(&mut report, ItemKind::LeftOut, TYPE0_NAMES, written);
        let mut all = conductor;
        for (_, g, items) in tracks {
            if g != 0 {
                report.add(ItemKind::Changed, TYPE0_GROUPS, None);
            }
            all.extend(items);
        }
        out.push(finish(all, o.ppq, &mut report));
    } else {
        out.push(finish(conductor, o.ppq, &mut report));
        for (name, g, mut items) in tracks {
            items.push(Item {
                tick: 0,
                key: meta_key(SUB_NAME),
                ev: smf_meta(0x03, name.into_bytes()),
            });
            if g != 0 {
                items.push(Item {
                    tick: 0,
                    key: meta_key(SUB_PORT),
                    ev: smf_meta(0x21, vec![g]),
                });
            }
            out.push(finish(items, o.ppq, &mut report));
        }
    }
    let sum = &mut report.summary;
    sum.tracks = written;
    sum.notes = notes;
    sum.markers = markers;
    sum.tempo_map = true;
    Ok((
        SmfFile {
            format: u16::from(!o.type0),
            division: Division::Ppq(o.ppq),
            tracks: out,
        },
        report,
    ))
}

/// The track of `items`: sorted by Ruling 10, ticks scaled to `ppq`
/// (Changed when one moved), an event more than `0x0FFF_FFFF` file ticks
/// after the last left out, then End of Track at the last event.
fn finish(mut items: Vec<Item<SmfEvent>>, ppq: u16, report: &mut Report) -> SmfTrack {
    items.sort_by_key(|i| (i.tick, i.key));
    let rounded = format!("Positions rounded to {ppq} ticks per quarter");
    let mut events = Vec::with_capacity(items.len().saturating_add(1));
    let mut prev = 0u64;
    // Notes sounding in the file, by channel and pitch, so a note-off too
    // far away still ends its note and one whose note-on was left out is
    // left out too.
    let mut sounding: BTreeMap<(u8, u8), u32> = BTreeMap::new();
    let mut dropped = false;
    for it in items {
        let Some((mut tick, moved)) = scale(it.tick, ppq) else {
            report.add(ItemKind::LeftOut, OUTSIDE, None);
            continue;
        };
        let note = match &it.ev {
            SmfEvent::Channel(m) => match m.bytes {
                [s, p, v] if s & 0xF0 == 0x90 && v > 0 => Some((true, (s & 0x0F, p))),
                [s, p, _] if matches!(s & 0xF0, 0x80 | 0x90) => Some((false, (s & 0x0F, p))),
                _ => None,
            },
            _ => None,
        };
        if tick.checked_sub(prev).is_none_or(|d| d > MAX_DELTA) {
            let ends_written =
                matches!(note, Some((false, k)) if sounding.get(&k).is_some_and(|n| *n > 0));
            match prev.checked_add(MAX_DELTA).filter(|_| ends_written) {
                Some(t) => {
                    report.add(ItemKind::Changed, ENDED_EARLY, None);
                    tick = t;
                }
                None => {
                    report.add(ItemKind::LeftOut, FAR, None);
                    dropped = true;
                    continue;
                }
            }
        }
        match note {
            Some((true, k)) => {
                let n = sounding.entry(k).or_insert(0);
                *n = n.saturating_add(1);
            }
            Some((false, k)) => match sounding.get_mut(&k) {
                Some(n) if *n > 0 => *n -= 1,
                // Its note-on was left out (too far): so is the note-off.
                _ if dropped => continue,
                _ => {}
            },
            None => {}
        }
        if moved {
            report.add(ItemKind::Changed, &rounded, None);
        }
        prev = tick;
        events.push((tick, it.ev));
    }
    events.push((prev, smf_meta(0x2F, Vec::new())));
    SmfTrack { events }
}

/// Model tick `t` (from file position 0) at `ppq`, rounded half up, and
/// whether it moved; `None` before file position 0.
fn scale(t: i64, ppq: u16) -> Option<(u64, bool)> {
    let n = u128::try_from(t).ok()?.checked_mul(u128::from(ppq))?;
    let den = u128::from(PPQ.unsigned_abs());
    let q = n.checked_add(den / 2)?.checked_div(den)?;
    Some((u64::try_from(q).ok()?, n % den != 0))
}

/// One MPE timeline message: its tick, order and message.
struct Timed {
    tick: i64,
    key: Key,
    msg: LiveMsg,
}

struct SmfWriter<'s, 'a> {
    src: &'s Source<'a>,
    origin: Origin,
    mpe: bool,
    report: &'s mut Report,
    notes: u64,
    per_note: u64,
}

impl SmfWriter<'_, '_> {
    /// The items of one track by group (lowest first); its texts go with
    /// the first group.
    fn lane(&mut self, lane: &Lane, played: &[ResolvedEvent]) -> BTreeMap<u8, Vec<Item<SmfEvent>>> {
        let doc = self.src.doc;
        let mpe = lane
            .midi_dest
            .filter(|_| self.mpe)
            .and_then(|d| doc.instruments().get(&d)?.mpe.map(|cfg| (d, cfg)));
        let mut groups: BTreeMap<u8, Vec<Item<SmfEvent>>> = BTreeMap::new();
        let mut texts: Vec<Item<SmfEvent>> = Vec::new();
        let mut dests: BTreeMap<u8, BTreeSet<DestId>> = BTreeMap::new();
        let mut timeline: Vec<Timed> = Vec::new();
        let mut mpe_group = 0u8;
        let mut noted = false;
        // Bank selects seen per pair at a tick, waiting for a program.
        let mut banks: BTreeMap<(u8, u8), (i64, bool, bool)> = BTreeMap::new();
        for r in played {
            let Some(tick) = r.tick.0.checked_sub(self.origin.tick).filter(|t| *t >= 0) else {
                self.report.add(ItemKind::LeftOut, OUTSIDE, None);
                continue;
            };
            if let EventBody::Text { kind, text } = &r.body {
                texts.push(Item {
                    tick,
                    key: meta_key(SUB_TEXT),
                    ev: smf_meta(text_meta(*kind), text.as_bytes().to_vec()),
                });
                continue;
            }
            if !r.body.is_midi() {
                continue;
            }
            let (g, c) = routed(r, lane, &mut noted, self.report);
            if let EventBody::Note { duration, .. } = r.body
                && duration.0 <= 0
            {
                self.report.add(ItemKind::LeftOut, ZERO, None);
                continue;
            }
            check_values(&r.body, self.report);
            if let Some(d) = r.dest {
                dests.entry(g).or_default().insert(d);
            }
            let items = groups.entry(g).or_default();
            match mpe {
                Some((d, _)) if r.dest == Some(d) && !matches!(r.body, EventBody::SysEx { .. }) => {
                    mpe_group = g;
                    self.timeline(r, tick, &mut timeline);
                }
                _ => {
                    self.translated(r, tick, (g, c), items, &mut banks);
                }
            }
        }
        if let Some((dest, cfg)) = mpe
            && !timeline.is_empty()
        {
            let items = groups.entry(mpe_group).or_default();
            self.encode(cfg, timeline, items);
            dests.entry(mpe_group).or_default().insert(dest);
        }
        if groups.is_empty() {
            groups.insert(lane_group(doc, lane), Vec::new());
        }
        for (g, items) in groups.iter_mut() {
            let mut pre = Vec::new();
            for d in dests.get(g).into_iter().flatten() {
                let cfg = mpe.filter(|(m, _)| m == d).map(|(_, cfg)| cfg);
                let Some((_, ch)) = preamble_channel(doc, *d, cfg) else {
                    continue;
                };
                if let Some(cfg) = cfg {
                    for m in configuration(&cfg) {
                        pre.push(Item {
                            tick: 0,
                            key: chan_key(Class::Patch, 0),
                            ev: SmfEvent::Channel(m),
                        });
                    }
                }
                for body in preamble_bodies(doc, *d) {
                    for out in translate(&body, ch).msgs() {
                        pre.push(Item {
                            tick: 0,
                            key: chan_key(out.class, 0),
                            ev: SmfEvent::Channel(out.msg),
                        });
                    }
                }
            }
            pre.append(items);
            *items = pre;
        }
        if let Some(first) = groups.values_mut().next() {
            first.extend(texts);
        }
        groups
    }

    /// One event through `translate` on channel `c`.
    fn translated(
        &mut self,
        r: &ResolvedEvent,
        tick: i64,
        (g, c): (u8, u8),
        items: &mut Vec<Item<SmfEvent>>,
        banks: &mut BTreeMap<(u8, u8), (i64, bool, bool)>,
    ) {
        match r.body {
            EventBody::Cc {
                index: i @ (0 | 32),
                ..
            } => {
                let b = banks.entry((g, c)).or_insert((tick, false, false));
                if b.0 != tick {
                    *b = (tick, false, false);
                }
                if i == 0 {
                    b.1 = true;
                } else {
                    b.2 = true;
                }
            }
            EventBody::Program { bank, .. } => {
                if let Some((t, true, true)) = banks.remove(&(g, c))
                    && t == tick
                    && bank.is_none()
                {
                    self.report.add(ItemKind::Changed, BANKS, None);
                }
            }
            _ => {}
        }
        let t = translate(&r.body, c);
        let duration = match &r.body {
            EventBody::Note {
                duration,
                attribute,
                per_note,
                ..
            } => {
                self.notes = self.notes.saturating_add(1);
                if attribute.is_some() {
                    self.report.add(ItemKind::LeftOut, ATTRIBUTES, None);
                }
                if !per_note.is_empty() {
                    self.per_note = self.per_note.saturating_add(1);
                }
                duration.0
            }
            _ => 0,
        };
        for out in t.msgs() {
            let at = match out.when {
                core_engine::midi1::When::Start => Some(tick),
                core_engine::midi1::When::End => tick.checked_add(duration),
            };
            if let Some(at) = at {
                items.push(Item {
                    tick: at,
                    key: chan_key(out.class, 0),
                    ev: SmfEvent::Channel(out.msg),
                });
            }
        }
        if let Some(data) = t.sysex {
            items.push(Item {
                tick,
                key: chan_key(Class::Control, 1),
                ev: SmfEvent::SysEx(data.to_vec()),
            });
        }
        if t.drops.sysex8 {
            self.report.add(ItemKind::LeftOut, SYSEX8, None);
        }
    }

    /// One event of an MPE track as timeline messages for the encoder.
    fn timeline(&mut self, r: &ResolvedEvent, tick: i64, out: &mut Vec<Timed>) {
        match &r.body {
            EventBody::Note {
                pitch,
                velocity,
                release_velocity: rel,
                duration,
                attribute,
                per_note,
            } => {
                self.notes = self.notes.saturating_add(1);
                if attribute.is_some() {
                    self.report.add(ItemKind::LeftOut, ATTRIBUTES, None);
                }
                let Some(end) = tick.checked_add(duration.0) else {
                    return;
                };
                out.push(Timed {
                    tick,
                    key: chan_key(Class::NoteOn, 0),
                    msg: LiveMsg::NoteOn {
                        pitch: *pitch,
                        velocity: (*velocity).max(1),
                        expr: NoteExpr {
                            bend: CENTER,
                            pressure: 0,
                            timbre: CENTER,
                        },
                    },
                });
                for p in per_note {
                    let expr = match p.kind {
                        PerNoteKind::PitchBend => Expr::Bend,
                        PerNoteKind::Registered(74) => Expr::Timbre,
                        _ => {
                            self.report.add(ItemKind::LeftOut, MPE_OTHER, None);
                            continue;
                        }
                    };
                    match tick
                        .checked_add(p.offset.0)
                        .filter(|t| *t < end && *t >= tick)
                    {
                        Some(at) => out.push(Timed {
                            tick: at,
                            key: chan_key(Class::NoteOn, 1),
                            msg: LiveMsg::PerNote {
                                pitch: *pitch,
                                expr,
                                value: p.value,
                            },
                        }),
                        None => self.report.add(ItemKind::LeftOut, PER_NOTE_LATE, None),
                    }
                }
                out.push(Timed {
                    tick: end,
                    key: chan_key(Class::NoteOff, 0),
                    msg: LiveMsg::NoteOff {
                        pitch: *pitch,
                        velocity: *rel,
                    },
                });
            }
            EventBody::PolyPressure { pitch, value } => out.push(Timed {
                tick,
                key: chan_key(Class::NoteOn, 1),
                msg: LiveMsg::PerNote {
                    pitch: *pitch,
                    expr: Expr::Pressure,
                    value: *value,
                },
            }),
            body => {
                for m in translate(body, 0).msgs() {
                    out.push(Timed {
                        tick,
                        key: chan_key(m.class, 0),
                        msg: LiveMsg::Channel(m.msg),
                    });
                }
            }
        }
    }

    /// The timeline through one encoder, in order; its messages keep their
    /// timeline message's place.
    fn encode(
        &mut self,
        cfg: MpeConfig,
        mut timeline: Vec<Timed>,
        items: &mut Vec<Item<SmfEvent>>,
    ) {
        timeline.sort_by_key(|t| (t.tick, t.key));
        let mut enc = MpeEncoder::new(cfg);
        for t in timeline {
            let mut msgs: Vec<Midi1Msg> = Vec::new();
            enc.encode(t.msg, &mut |m| msgs.push(m));
            items.extend(msgs.into_iter().map(|m| Item {
                tick: t.tick,
                key: t.key,
                ev: SmfEvent::Channel(m),
            }));
        }
        add_n(self.report, ItemKind::Changed, STOLEN, enc.take_stolen());
    }
}

/// The group of a track's destination (0 without one).
fn lane_group(doc: &Document, lane: &Lane) -> u8 {
    lane.midi_dest
        .and_then(|d| doc.instruments().get(&d))
        .map_or(0, |d| match d.kind {
            DestKind::MidiEndpoint { group, .. } => group & 0x0F,
            DestKind::PluginInstrument { .. } => 0,
        })
}

// ---- MIDI Clip File ----

/// `s` cut to [`MAX_FLEX_TEXT_BYTES`] on a character boundary (Ruling
/// T4-a), Changed when cut.
fn flex_text(s: String, report: &mut Report) -> String {
    if s.len() <= MAX_FLEX_TEXT_BYTES {
        return s;
    }
    let mut end = MAX_FLEX_TEXT_BYTES;
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    report.add(ItemKind::Changed, FLEX_CUT, None);
    s.get(..end).unwrap_or_default().to_string()
}

fn flex(bank: u8, status: u8, text: String, report: &mut Report) -> ClipEvent {
    ClipEvent::Text {
        group: 0,
        bank,
        status,
        text: flex_text(text, report),
    }
}

/// The MIDI Clip File of `o`'s track as it plays, at 15 360 ticks per
/// quarter: the tempo and meter in force at the start in the header, the
/// key and every change inside the range in the sequence data (F8), the
/// sequence name as the composition name and the track name as the clip
/// name; 16-bit velocities, 32-bit values, attributes and per-note
/// controllers as they are. With a range, the events starting inside it,
/// moved to start at 0. Refuses an unknown sequence or track and an empty
/// or reversed range.
pub fn to_clip(doc: &Document, o: &ClipOptions) -> Result<(ClipFile, Report), IoError> {
    if let Some((a, b)) = o.range
        && a >= b
    {
        return Err(refused(EMPTY_RANGE));
    }
    let mut report = Report::default();
    let src = source(doc, o.seq, &[o.lane], &mut report)?;
    let s = src.seq;
    let lane = src
        .lanes
        .first()
        .copied()
        .ok_or_else(|| refused(NO_TRACK))?;
    other_info(doc, s, &mut report);
    drum_rows(lane, &mut report);
    if o.range.is_none() {
        short_first_bar(s, &mut report);
    }
    let (origin, end) = match o.range {
        Some((a, b)) => (
            Origin {
                tick: a.0,
                pickup: None,
            },
            Some(b.0),
        ),
        None => (origin(s, &mut report), None),
    };
    let r = &mut report;
    let inside = |t: i64| t >= origin.tick && end.is_none_or(|e| t < e);

    let mut header = Vec::new();
    let mut items: Vec<Item<ClipEvent>> = Vec::new();
    meta(
        &mut items,
        0,
        0,
        flex(BANK_METADATA, 0x02, s.name.clone(), r),
    );
    meta(
        &mut items,
        0,
        1,
        flex(BANK_METADATA, 0x03, lane.name.clone(), r),
    );
    if let Some(c) = &doc.settings().info.copyright {
        meta(
            &mut items,
            0,
            SUB_COPYRIGHT,
            flex(BANK_METADATA, 0x04, c.clone(), r),
        );
    }
    let (mut tempo_set, mut time_set) = (false, false);
    for (tick, e) in map_changes(s, origin, end, r) {
        let (sub, p) = match e {
            MapEvent::Tempo(t) => (SUB_TEMPO, ump::set_tempo(t.ten_ns())),
            MapEvent::Time(m) => {
                let pow = m.den.max(1).trailing_zeros() as u8;
                (SUB_TIME, ump::time_sig(m.num, pow, 8))
            }
            MapEvent::Key(k) => (SUB_KEY, ump::key_sig(k.sharps, k.minor)),
        };
        let to_header = tick == 0
            && match e {
                MapEvent::Tempo(_) => !std::mem::replace(&mut tempo_set, true),
                MapEvent::Time(_) => !std::mem::replace(&mut time_set, true),
                MapEvent::Key(_) => false,
            };
        if to_header {
            header.push(ClipEvent::Packet(p));
        } else {
            meta(&mut items, tick, sub, ClipEvent::Packet(p));
        }
    }
    let mut markers = 0u64;
    if end.is_none() {
        for _ in s.markers.iter().filter(|m| m.tick.0 < origin.tick) {
            r.add(ItemKind::LeftOut, OUTSIDE, None);
        }
    }
    for m in s.markers.iter().filter(|m| inside(m.tick.0)) {
        let Some(tick) = m.tick.0.checked_sub(origin.tick) else {
            continue;
        };
        markers = markers.saturating_add(1);
        let ev = flex(BANK_METADATA, 0x00, format!("marker: {}", m.name), r);
        meta(&mut items, tick, SUB_MARKER, ev);
    }

    let played = src.played.get(&lane.id).map_or(&[][..], Vec::as_slice);
    if o.range.is_some() {
        // The state in force at the range's start is not carried over.
        let before = played.iter().filter(|e| {
            e.tick.0 < origin.tick
                && matches!(
                    e.body,
                    EventBody::Program { .. }
                        | EventBody::Cc { .. }
                        | EventBody::PitchBend { .. }
                        | EventBody::ChannelPressure { .. }
                )
        });
        for _ in before {
            r.add(ItemKind::Note, STATE_BEFORE, None);
        }
    }
    let end_rel = end.and_then(|b| b.checked_sub(origin.tick));
    let mut dests: BTreeSet<DestId> = BTreeSet::new();
    let mut noted = false;
    let mut notes = 0u64;
    let mut events: Vec<Item<ClipEvent>> = Vec::new();
    for e in played.iter().filter(|e| inside(e.tick.0)) {
        let Some(tick) = e.tick.0.checked_sub(origin.tick) else {
            continue;
        };
        let push = |events: &mut Vec<Item<ClipEvent>>, tick: i64, key: Key, p: ump::Packet| {
            events.push(Item {
                tick,
                key,
                ev: ClipEvent::Packet(p),
            });
        };
        if let EventBody::Text { kind, text } = &e.body {
            let ev = match kind {
                TextKind::Text => flex(BANK_METADATA, 0x00, text.clone(), r),
                TextKind::Lyric => flex(BANK_PERFORMANCE, 0x01, text.clone(), r),
                TextKind::Cue => flex(BANK_METADATA, 0x00, format!("cue: {text}"), r),
                TextKind::Marker => flex(BANK_METADATA, 0x00, format!("marker: {text}"), r),
                TextKind::Copyright => flex(BANK_METADATA, 0x04, text.clone(), r),
            };
            meta(&mut events, tick, SUB_TEXT, ev);
            continue;
        }
        if !e.body.is_midi() {
            continue;
        }
        let (g, c) = routed(e, lane, &mut noted, r);
        if let Some(d) = e.dest {
            dests.insert(d);
        }
        match &e.body {
            EventBody::Note {
                pitch,
                velocity,
                release_velocity,
                duration,
                attribute,
                per_note,
            } => {
                if duration.0 <= 0 {
                    r.add(ItemKind::LeftOut, ZERO, None);
                    continue;
                }
                let Some(mut off) = tick.checked_add(duration.0) else {
                    continue;
                };
                if let Some(e) = end_rel.filter(|e| off > *e) {
                    r.add(ItemKind::Changed, CUT_AT_END, None);
                    off = e;
                }
                notes = notes.saturating_add(1);
                let attr = attribute.map(|a| ump::NoteAttribute {
                    kind: a.kind,
                    data: a.data,
                });
                let on = ump::note_on(g, c, *pitch, *velocity, attr);
                push(&mut events, tick, chan_key(Class::NoteOn, 0), on);
                for p in per_note {
                    let Some(at) = tick
                        .checked_add(p.offset.0)
                        .filter(|t| *t < off && *t >= tick)
                    else {
                        r.add(ItemKind::LeftOut, PER_NOTE_LATE, None);
                        continue;
                    };
                    let packet = match p.kind {
                        PerNoteKind::Registered(i) => {
                            ump::per_note_registered(g, c, *pitch, i, p.value)
                        }
                        PerNoteKind::Assignable(i) => {
                            ump::per_note_assignable(g, c, *pitch, i, p.value)
                        }
                        PerNoteKind::PitchBend => ump::per_note_pitch_bend(g, c, *pitch, p.value),
                    };
                    push(&mut events, at, chan_key(Class::NoteOn, 1), packet);
                }
                let off_p = ump::note_off(g, c, *pitch, *release_velocity, None);
                push(&mut events, off, chan_key(Class::NoteOff, 0), off_p);
            }
            EventBody::SysEx { data, eight_bit } => {
                let ev = if *eight_bit {
                    ClipEvent::SysEx8 {
                        group: g,
                        data: data.to_vec(),
                    }
                } else {
                    ClipEvent::SysEx7 {
                        group: g,
                        data: data.to_vec(),
                    }
                };
                events.push(Item {
                    tick,
                    key: chan_key(Class::Control, 1),
                    ev,
                });
            }
            body => {
                if let Some(p) = channel_packet(g, c, body) {
                    push(&mut events, tick, chan_key(body_class(body), 0), p);
                }
            }
        }
    }
    for d in &dests {
        let Some((g, ch)) = preamble_channel(doc, *d, None) else {
            continue;
        };
        for body in preamble_bodies(doc, *d) {
            if let Some(p) = channel_packet(g, ch, &body) {
                items.push(Item {
                    tick: 0,
                    key: chan_key(body_class(&body), 0),
                    ev: ClipEvent::Packet(p),
                });
            }
        }
    }
    items.extend(events);
    items.sort_by_key(|i| (i.tick, i.key));
    let last = items.iter().map(|i| i.tick).max().unwrap_or(0);
    let end_tick = match end {
        Some(b) => b.checked_sub(origin.tick).unwrap_or(last),
        None => last,
    };
    let events: Vec<(u64, ClipEvent)> = items
        .into_iter()
        .filter_map(|i| u64::try_from(i.tick).ok().map(|t| (t, i.ev)))
        .collect();
    let sum = &mut report.summary;
    sum.tracks = 1;
    sum.notes = notes;
    sum.markers = markers;
    sum.tempo_map = true;
    Ok((
        ClipFile {
            tpq: 15_360,
            header,
            events,
            end: u64::try_from(end_tick).unwrap_or(0),
        },
        report,
    ))
}

/// Adds a Flex Data or text item.
fn meta(items: &mut Vec<Item<ClipEvent>>, tick: i64, sub: u8, ev: ClipEvent) {
    items.push(Item {
        tick,
        key: meta_key(sub),
        ev,
    });
}

/// The MIDI 2.0 packet of a controller, bend, pressure or program event.
fn channel_packet(g: u8, c: u8, body: &EventBody) -> Option<ump::Packet> {
    Some(match *body {
        EventBody::Cc { index, value } => ump::control_change(g, c, index, value),
        EventBody::PitchBend { value } => ump::pitch_bend(g, c, value),
        EventBody::ChannelPressure { value } => ump::channel_pressure(g, c, value),
        EventBody::PolyPressure { pitch, value } => ump::poly_pressure(g, c, pitch, value),
        EventBody::Program { program, bank } => ump::program_change(g, c, program, bank),
        _ => return None,
    })
}
