//! DAWproject export of the arrangement (plan 6d Task 5, Rulings 4 and
//! 9–14): each exported track's clips and track-level points.
//!
//! What is written is what plays: the layout's events come from
//! `midi_out::source` over a copy of the document whose top-level reference
//! uses and regions are unmuted (the muted ones are written with
//! `enable="false"`). Events group by the top step of their path:
//! - top-level MIDI events go into note clips by the [`ClipRule`];
//! - events inside a reference use make that use's clip, looped when every
//!   pass is the same, else unrolled;
//! - every region is an audio clip of its own (one inside a use too);
//! - automation becomes the track's `Points`, after the destination's
//!   default patch and MIDI mix at time 0.
//!
//! Times are beats from file time 0 (the layout's origin); inside a note
//! clip they are beats from the clip's start, inside a note from the note's
//! start (Ruling 12); inside an audio clip seconds of the media.

use super::export::{AudioFile, ClipRule, Layout, LeftOutAudio, Part, RegionGain, Ticker};
use super::schema::*;
use super::units::{TagKind, beats, linear_gain, pass_comment, tag};
use crate::IoError;
use crate::midi_out;
use crate::report::{ItemKind, Report};
use core_model::automation::{AutomationTarget, FULL_SCALE, fader_db, switch_on};
use core_model::document::{DestKind, Destination};
use core_model::event::{Event, EventBody, FadeShape, LoopCount, PerNoteKind};
use core_model::ids::{DestId, EventId, MediaId};
use core_model::meter::MeterMap;
use core_model::query::{PathStep, ResolvedEvent, seq_length};
use core_model::time::{BarBeatUnit, Tick};
use std::collections::{BTreeMap, BTreeSet};

/// How a reference use plays, as the export writes its clips (and the
/// snapshot's `UseAt` records it): one pass's length, the number of
/// passes, and the length of them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UseSpan {
    /// One pass, in ticks (at least 1).
    pub pass: i64,
    /// How many passes it plays (at least 1).
    pub passes: i64,
    /// All of them, in ticks.
    pub total: i64,
}

/// The [`UseSpan`] of a use of `target` with its own `length` and
/// `loop_count`: a counted use plays its length that many times; a filling
/// use plays its length once, in passes of the sequence's length.
pub(super) fn use_span(
    doc: &core_model::document::Document,
    target: core_model::ids::SeqId,
    length: Option<Tick>,
    loop_count: LoopCount,
) -> UseSpan {
    let b = seq_length(doc, target).map_or(0, |t| t.0).max(1);
    let use_len = length.map_or(b, |t| t.0).max(1);
    match loop_count {
        LoopCount::Count(n) => {
            let n = i64::from(n.max(1));
            UseSpan {
                pass: use_len,
                passes: n,
                total: use_len.saturating_mul(n),
            }
        }
        LoopCount::Fill => UseSpan {
            pass: b,
            passes: use_len.saturating_add(b - 1) / b,
            total: use_len,
        },
    }
}

// ---------------------------------------------------------------- the report

/// Left out: muted notes (a count), as `midi_out::source` counts them.
pub const MUTED_NOTES: &str = midi_out::MUTED;
/// Left out: events the sequence's window cuts off (by track), as
/// `midi_out::source` finds them.
pub const EVENTS_OUTSIDE: &str = midi_out::OUTSIDE;
/// Left out: SysEx messages (a count).
pub const SYSEX: &str = "SysEx messages";
/// Left out: text, lyric, cue, marker and copyright events (a count).
pub const TEXTS: &str = "Texts, lyrics and cues";
/// Left out: MIDI 2.0 note attributes (a count).
pub const NOTE_ATTRIBUTES: &str = "Note attributes";
/// Left out: per-note controllers the format has no expression for.
pub const OTHER_PER_NOTE: &str = "Per-note controllers other than pitch, timbre, volume and pan";
/// Left out: solo automation (the format has no solo parameter; by track).
pub const SOLO_AUTOMATION: &str = "Solo automation";
/// Left out: velocity-scale automation (by track).
pub const VELOCITY_SCALE_AUTOMATION: &str = "Velocity-scale automation";
/// Left out: automation of a mixer part the file does not hold (by target).
pub const UNPLACED_AUTOMATION: &str = "Automation of mixer parts not in the file";
/// Left out: region gain, with [`RegionGain::LeaveOut`] (by region).
pub const REGION_GAIN: &str = "Region gain";
/// Changed: fades of a shape other than linear (a count).
pub const FADE_SHAPES: &str = "Fade shapes (the file keeps fade lengths only)";
/// Changed: crossfades (Ruling 10; a count).
pub const CROSSFADES: &str =
    "Crossfades were written as overlapping clips; the earlier clip's fade-out covers the overlap";
/// Changed: events on MIDI groups 2 to 16 (a count).
pub const MIDI_GROUPS: &str = "MIDI groups other than group 1 were written as group 1";
/// Changed: audio regions inside reference uses (a count).
pub const NESTED_AUDIO: &str = "Audio inside nested sequences was written as plain clips";
/// Note: regions that start inside another without a crossfade (by track).
pub const OVERLAPS: &str =
    "Overlapping regions: in VizDAW the later one takes over; other DAWs may play both";
/// Note: muted regions and muted uses inside a used sequence, which do not
/// play and are not written (by sequence).
pub const NESTED_MUTED: &str =
    "Muted regions and uses inside a used sequence were not written (they do not play)";
/// Note: transposed reference uses (a count).
pub const TRANSPOSED: &str = "Transposed uses of a sequence were written as played";
/// Changed (final review I2): the notes of tracks, or of sequences they
/// use, that play quantized were written where they play (by track); the
/// file has no play quantize.
pub const PLAY_QUANTIZED: &str = "Play quantize was applied to the notes";

// ---------------------------------------------------------------- the API

/// The arrangement's track timelines and what they need.
pub(super) struct Arranged {
    /// One `Lanes track="t<lane>"` per exported track, in track order.
    pub(super) lanes: Vec<DpTimeline>,
    /// The audio entries, numbered in first-use order.
    pub(super) audio: Vec<AudioFile>,
    /// Notes written.
    pub(super) notes: u64,
    /// Audio clips written.
    pub(super) regions: u64,
    /// Automation points written.
    pub(super) points: u64,
}

/// The timelines of every track of `l`, with their report lines.
pub(super) fn arrange(
    l: &Layout<'_>,
    rule: ClipRule,
    gain: RegionGain,
    left_out: &LeftOutAudio,
    report: &mut Report,
    ticker: &mut Ticker<'_>,
) -> Result<Arranged, IoError> {
    let mut a = Arr {
        l,
        rule,
        gain,
        left_out,
        listed_media: BTreeSet::new(),
        report,
        ticker,
        entries: BTreeMap::new(),
        audio: Vec::new(),
        notes: 0,
        regions: 0,
        points: 0,
        transposes: BTreeSet::new(),
        seq_len: seq_length(l.doc, l.seq.id).map_or(i64::MAX, |t| t.0),
    };
    a.nested_muted()?;
    let mut lanes = Vec::new();
    for (pi, p) in l.parts.iter().enumerate() {
        lanes.push(DpTimeline::Lanes(a.track(pi, p)?));
    }
    Ok(Arranged {
        lanes,
        audio: a.audio,
        notes: a.notes,
        regions: a.regions,
        points: a.points,
    })
}

// ---------------------------------------------------------------- helpers

type Ev<'e> = &'e ResolvedEvent;

/// Where an event of a clip ends: a note after its duration.
fn end_of(r: &ResolvedEvent) -> i64 {
    match &r.body {
        EventBody::Note { duration, .. } => r.tick.0.saturating_add(duration.0.max(0)),
        _ => r.tick.0,
    }
}

/// The tick a clip holding `evs` must reach so that its last controller
/// point (which is at one tick, not a span) is inside it: a clip's content
/// is `[start, start + duration)`, so a point on the clip's last tick needs
/// one more.
fn points_end(evs: &[Ev<'_>]) -> i64 {
    evs.iter()
        .filter(|r| !matches!(r.body, EventBody::Note { .. }))
        .map(|r| r.tick.0.saturating_add(1))
        .max()
        .unwrap_or(i64::MIN)
}

/// The span `[start, end)` of the `bars`-bar window holding `tick`, counted
/// from bar 1 (as the arrangement's blocks are).
fn bar_span(meter: &MeterMap, tick: i64, bars: u32) -> (i64, i64) {
    let n = i32::try_from(bars.max(1)).unwrap_or(i32::MAX);
    let bar = meter.tick_to_bbu(Tick(tick), 1).bar;
    let first = (bar - 1).div_euclid(n).saturating_mul(n).saturating_add(1);
    let at = |bar: i32| {
        meter
            .bbu_to_tick(
                BarBeatUnit {
                    bar,
                    beat: 1,
                    unit: 0,
                },
                1,
            )
            .0
    };
    (at(first), at(first.saturating_add(n)))
}

/// A 32-bit controller value as the format's normalised 0 … 1.
fn norm32(v: u32) -> f64 {
    f64::from(v) / FULL_SCALE
}

/// A 7-bit value as normalised 0 … 1.
fn norm7(v: u8) -> f64 {
    f64::from(v & 0x7F) / 127.0
}

/// A per-note pitch bend in semitones: `(v − 2³¹) × 48 / 2³¹`.
fn semitones(v: u32) -> f64 {
    const CENTRE: f64 = 2_147_483_648.0;
    (f64::from(v) - CENTRE) * 48.0 / CENTRE
}

fn hold(time: f64, v: f64) -> DpPoint {
    DpPoint {
        time,
        value: DpPointValue::Real(v, Some(DpInterpolation::Hold)),
    }
}

/// The order and key of a controller `Points` in a clip: controllers by
/// number, then programs, pitch bend, channel pressure, poly pressure by
/// key.
type PointsKey = (u8, i32, i32);

fn points_for(key: PointsKey) -> DpPoints {
    let (rank, channel, n) = key;
    let (expression, unit, controller, k) = match rank {
        0 => (
            DpExpression::ChannelController,
            Some(DpUnit::Normalized),
            Some(n),
            None,
        ),
        1 => (DpExpression::ProgramChange, None, None, None),
        2 => (
            DpExpression::PitchBend,
            Some(DpUnit::Normalized),
            None,
            None,
        ),
        3 => (
            DpExpression::ChannelPressure,
            Some(DpUnit::Normalized),
            None,
            None,
        ),
        _ => (
            DpExpression::PolyPressure,
            Some(DpUnit::Normalized),
            None,
            Some(n),
        ),
    };
    DpPoints {
        head: DpHead::default(),
        unit,
        target: DpTarget {
            parameter: None,
            expression: Some(expression),
            channel: Some(channel),
            key: k,
            controller,
        },
        points: Vec::new(),
    }
}

fn push_point(map: &mut BTreeMap<PointsKey, DpPoints>, key: PointsKey, p: DpPoint) {
    map.entry(key)
        .or_insert_with(|| points_for(key))
        .points
        .push(p);
}

/// The channel the export writes a destination's events and default patch
/// on: an endpoint's, 0 for a plug-in.
pub(super) fn dest_channel(d: &Destination) -> u8 {
    match &d.kind {
        DestKind::MidiEndpoint { channel, .. } => channel & 0x0F,
        DestKind::PluginInstrument { .. } => 0,
    }
}

/// A default patch as written: channel, program and bank (MSB, LSB).
pub(super) type WrittenPatch = (u8, u8, Option<(u8, u8)>);

/// A destination's default patch as the export writes it: its channel
/// ([`dest_channel`]), program and bank, 7 bits each.
pub(super) fn written_patch(d: &Destination) -> Option<WrittenPatch> {
    let patch = d.default_patch?;
    Some((
        dest_channel(d),
        patch.program & 0x7F,
        patch.bank.map(|(msb, lsb)| (msb & 0x7F, lsb & 0x7F)),
    ))
}

/// The destinations whose default patch and MIDI mix start track part
/// `p`'s points (Ruling 13): its track's, and every one its MIDI events
/// play on; none for an audio part. The snapshot's
/// [`TrackIds::patches`](super::record::TrackIds::patches) are theirs too
/// (plan 6e Task 11).
pub(super) fn patched_dests(l: &Layout<'_>, p: &Part<'_>) -> BTreeSet<DestId> {
    let mut dests = BTreeSet::new();
    if p.audio {
        return dests;
    }
    dests.extend(p.lane.midi_dest);
    for r in l.played.get(&p.lane.id).map_or(&[][..], Vec::as_slice) {
        match &r.body {
            EventBody::AudioRegion { .. }
            | EventBody::Automation { .. }
            | EventBody::Text { .. }
            | EventBody::SysEx { .. }
            | EventBody::Reference { .. } => {}
            _ => dests.extend(r.dest),
        }
    }
    dests
}

/// A default patch as points at `time`: the bank as CC 0 and 32, then the
/// program.
fn patch_points(
    map: &mut BTreeMap<PointsKey, DpPoints>,
    ch: i32,
    time: f64,
    program: u8,
    bank: Option<(u8, u8)>,
) {
    if let Some((msb, lsb)) = bank {
        push_point(map, (0, ch, 0), hold(time, norm7(msb)));
        push_point(map, (0, ch, 32), hold(time, norm7(lsb)));
    }
    push_point(
        map,
        (1, ch, 0),
        DpPoint {
            time,
            value: DpPointValue::Integer(i32::from(program & 0x7F)),
        },
    );
}

/// One timeline, or several side by side; nothing for none.
fn together(mut items: Vec<DpTimeline>) -> Option<DpTimeline> {
    match items.len() {
        0 => None,
        1 => items.pop(),
        _ => Some(DpTimeline::Lanes(DpLanes {
            head: DpHead::default(),
            items,
        })),
    }
}

/// A note clip: `time duration playStart="0"`.
fn note_clip(time: f64, duration: f64, content: Option<DpTimeline>) -> DpClip {
    DpClip {
        named: DpNamed::default(),
        time,
        duration: Some(duration),
        content_time_unit: None,
        play_start: Some(0.0),
        play_stop: None,
        loop_start: None,
        loop_end: None,
        fade_time_unit: None,
        fade_in: None,
        fade_out: None,
        enable: None,
        reference: None,
        content: content.map(Box::new),
    }
}

/// A region on its way to an audio clip.
struct Region<'e> {
    r: Ev<'e>,
    id: EventId,
    crossfade: bool,
    muted: bool,
    rate: f64,
    secs: f64,
    end_secs: f64,
    fade_in: f64,
    fade_out: f64,
    clip: DpClip,
}

// ---------------------------------------------------------------- the work

struct Arr<'l, 'a, 'r, 't, 'c> {
    l: &'l Layout<'a>,
    rule: ClipRule,
    gain: RegionGain,
    /// Media written without audio (S9), and those already listed.
    left_out: &'l LeftOutAudio,
    listed_media: BTreeSet<MediaId>,
    report: &'r mut Report,
    ticker: &'t mut Ticker<'c>,
    /// Audio entries by (media, applied gain's bits).
    entries: BTreeMap<(MediaId, u32), usize>,
    audio: Vec<AudioFile>,
    notes: u64,
    regions: u64,
    points: u64,
    /// Transposed reference events already noted.
    transposes: BTreeSet<EventId>,
    /// The exported sequence's length.
    seq_len: i64,
}

impl Arr<'_, '_, '_, '_, '_> {
    fn rel(&self, t: i64) -> f64 {
        beats(Tick(t.saturating_sub(self.l.origin)))
    }

    /// The `Lanes` of track `p`: its clips, then its track-level points.
    fn track(&mut self, pi: usize, p: &Part<'_>) -> Result<DpLanes, IoError> {
        let l = self.l;
        let events = l.played.get(&p.lane.id).map_or(&[][..], Vec::as_slice);
        // A split track's audio half holds only the regions.
        let midi_here = !(p.audio && p.split);
        let mut top: Vec<Ev<'_>> = Vec::new();
        let mut uses: Vec<(EventId, Vec<Ev<'_>>)> = Vec::new();
        let mut use_at: BTreeMap<EventId, usize> = BTreeMap::new();
        let mut regions: Vec<Ev<'_>> = Vec::new();
        let mut autos: Vec<Ev<'_>> = Vec::new();
        // The played notes carry the play quantize of the track (or used
        // sequence's track) they are on, as `flatten` applies it.
        let mut quantized = false;
        for r in events {
            self.ticker.tick()?;
            // What a muted use holds is written in its disabled clip (MIDI,
            // regions) or not at all (automation, texts, SysEx: they never
            // play).
            let in_muted_use =
                r.path.len() > 1 && r.path.first().is_some_and(|s| l.muted.contains(&s.event));
            match &r.body {
                EventBody::AudioRegion { .. } => {
                    if p.audio {
                        regions.push(r);
                    }
                }
                _ if !midi_here => {}
                EventBody::Automation { .. } | EventBody::Text { .. } | EventBody::SysEx { .. }
                    if in_muted_use => {}
                EventBody::Automation { .. } => autos.push(r),
                EventBody::Text { .. } => self.report.add(ItemKind::LeftOut, TEXTS, None),
                EventBody::SysEx { .. } => self.report.add(ItemKind::LeftOut, SYSEX, None),
                EventBody::Reference { .. } => {}
                _ => {
                    self.transposed(&r.path);
                    if !quantized && matches!(r.body, EventBody::Note { .. }) {
                        quantized = r.path.last().is_some_and(|s| {
                            l.doc
                                .sequence(s.seq)
                                .ok()
                                .and_then(|q| q.lane(s.lane))
                                .is_some_and(|x| x.play_quantize.is_some())
                        });
                    }
                    match r.path.first() {
                        Some(step) if r.path.len() > 1 => {
                            let i = *use_at.entry(step.event).or_insert_with(|| {
                                uses.push((step.event, Vec::new()));
                                uses.len() - 1
                            });
                            if let Some((_, v)) = uses.get_mut(i) {
                                v.push(r);
                            }
                        }
                        _ => top.push(r),
                    }
                }
            }
        }

        if quantized {
            let name = p.lane.name.clone();
            self.report
                .add(ItemKind::Changed, PLAY_QUANTIZED, Some(name));
        }
        let mut clips: Vec<(i64, DpClip)> = Vec::new();
        self.rule_clips(&top, &mut clips)?;
        // The track's reference events, looked up once.
        let refs: BTreeMap<EventId, &Event> = if uses.is_empty() {
            BTreeMap::new()
        } else {
            p.lane
                .active()
                .into_iter()
                .flat_map(|t| t.events.values())
                .filter(|e| matches!(e.body, EventBody::Reference { .. }))
                .map(|e| (e.id, e))
                .collect()
        };
        for (id, evs) in &uses {
            self.use_clips(p, refs.get(id).copied(), *id, evs, &mut clips)?;
        }
        for (tick, clip) in self.audio_clips(p, &regions)? {
            clips.push((tick, clip));
        }
        clips.sort_by_key(|(t, _)| *t);

        let mut items = Vec::new();
        if !clips.is_empty() {
            items.push(DpTimeline::Clips(DpClips {
                head: DpHead::default(),
                clips: clips.into_iter().map(|(_, c)| c).collect(),
            }));
        }
        if !p.audio {
            items.extend(
                self.patch_and_mix(&patched_dests(l, p))
                    .into_values()
                    .map(DpTimeline::Points),
            );
        }
        items.extend(
            self.automation(pi, p, &autos)?
                .into_iter()
                .map(DpTimeline::Points),
        );
        Ok(DpLanes {
            head: DpHead {
                track: Some(p.id()),
                ..DpHead::default()
            },
            items,
        })
    }

    /// Notes the muted regions and uses inside the sequences the exported
    /// tracks use, through every level (each sequence once).
    fn nested_muted(&mut self) -> Result<(), IoError> {
        let l = self.l;
        let refs = |lane: &core_model::lane::Lane| -> Vec<core_model::ids::SeqId> {
            lane.active()
                .into_iter()
                .flat_map(|t| t.events.values())
                .filter_map(|e| match e.body {
                    EventBody::Reference { seq, .. } => Some(seq),
                    _ => None,
                })
                .collect()
        };
        let mut todo: Vec<core_model::ids::SeqId> = l.seq.lanes.iter().flat_map(refs).collect();
        let mut seen = BTreeSet::from([l.seq.id]);
        while let Some(id) = todo.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Ok(s) = l.doc.sequence(id) else { continue };
            for lane in &s.lanes {
                for e in lane.active().into_iter().flat_map(|t| t.events.values()) {
                    self.ticker.tick()?;
                    match e.body {
                        EventBody::Reference { seq, .. } => {
                            if e.mute {
                                self.report
                                    .add(ItemKind::Note, NESTED_MUTED, Some(s.name.clone()));
                            }
                            todo.push(seq);
                        }
                        EventBody::AudioRegion { .. } if e.mute => {
                            self.report
                                .add(ItemKind::Note, NESTED_MUTED, Some(s.name.clone()));
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// Notes once each transposed reference on `path` (the played notes
    /// carry the transpose).
    fn transposed(&mut self, path: &[PathStep]) {
        let Some((_, refs)) = path.split_last() else {
            return;
        };
        for step in refs {
            if self.transposes.contains(&step.event) {
                continue;
            }
            self.transposes.insert(step.event);
            if let Ok((e, _)) = self.l.doc.event(step.event)
                && matches!(e.body, EventBody::Reference { transpose, .. } if transpose != 0)
            {
                self.report.add(ItemKind::Note, TRANSPOSED, None);
            }
        }
    }

    /// The note clips of a track's top-level MIDI events, by the rule.
    fn rule_clips(&mut self, top: &[Ev<'_>], out: &mut Vec<(i64, DpClip)>) -> Result<(), IoError> {
        struct Group<'e> {
            start: i64,
            end: i64,
            bars: Option<(i64, i64)>,
            evs: Vec<Ev<'e>>,
        }
        let meter = &self.l.seq.meter;
        let mut groups: Vec<Group<'_>> = Vec::new();
        for r in top {
            self.ticker.tick()?;
            let (tick, end) = (r.tick.0, end_of(r));
            let bars = match self.rule {
                ClipRule::FixedBars(n) => Some(bar_span(meter, tick, n)),
                _ => None,
            };
            let joins = match (groups.last(), self.rule) {
                (None, _) => false,
                (Some(g), ClipRule::SilenceGap(gap)) => tick.saturating_sub(g.end) < gap.0.max(1),
                (Some(g), ClipRule::FixedBars(_)) => g.bars == bars,
                (Some(_), ClipRule::WholeTrack) => true,
            };
            match groups.last_mut() {
                Some(g) if joins => {
                    g.end = g.end.max(end);
                    g.evs.push(r);
                }
                _ => groups.push(Group {
                    start: tick,
                    end,
                    bars,
                    evs: vec![r],
                }),
            }
        }
        for g in groups {
            // A window holds its notes whole; it never starts before file
            // time 0.
            let (start, end) = match g.bars {
                Some((s, e)) => (s.max(self.l.origin), e.max(g.end)),
                None => (g.start, g.end),
            };
            let end = end.max(points_end(&g.evs));
            let content = self.content(&g.evs, start)?;
            let duration = beats(Tick(end.saturating_sub(start).max(1)));
            out.push((start, note_clip(self.rel(start), duration, content)));
        }
        Ok(())
    }

    /// The clips of reference use `id` (`event` on track `p`): one per pass
    /// of the track's loop, looped when every pass of the use is the same.
    /// Each carries the use's tag; when there are several, each carries
    /// [`pass_comment`]`(use, k)`, which the import reads to take them as one
    /// use (`import::timeline`'s `top_clip`).
    fn use_clips(
        &mut self,
        p: &Part<'_>,
        event: Option<&Event>,
        id: EventId,
        evs: &[Ev<'_>],
        out: &mut Vec<(i64, DpClip)>,
    ) -> Result<(), IoError> {
        let l = self.l;
        let lane = p.lane;
        let Some((e, target, use_len, loop_count)) = event.and_then(|e| match e.body {
            EventBody::Reference {
                seq,
                length,
                loop_count,
                ..
            } => Some((e, seq, length, loop_count)),
            _ => None,
        }) else {
            // Not a use of this track (cannot happen): a plain clip.
            let start = evs.first().map_or(0, |r| r.tick.0);
            let end = evs
                .iter()
                .map(|r| end_of(r))
                .max()
                .unwrap_or(start)
                .max(points_end(evs));
            let content = self.content(evs, start)?;
            out.push((
                start,
                note_clip(
                    self.rel(start),
                    beats(Tick(end.saturating_sub(start).max(1))),
                    content,
                ),
            ));
            return Ok(());
        };
        let (name, pickup) = l.doc.sequence(target).map_or((String::new(), 0), |s| {
            (s.name.clone(), s.start_offset.0.max(0))
        });
        let UseSpan {
            pass: iter_len,
            passes: count,
            total,
        } = use_span(l.doc, target, use_len, loop_count);
        // The track's own loop repeats the use: one clip per pass of it.
        let period = match lane.length {
            Some(t) if lane.looped && t.0 > 0 => Some(t.0),
            _ => None,
        };
        let first = e.tick.0.saturating_add(lane.play_shift.0);
        let mut passes: BTreeMap<i64, Vec<Ev<'_>>> = BTreeMap::new();
        for r in evs {
            let k = period.map_or(0, |per| {
                (r.tick.0.saturating_sub(first).saturating_add(pickup))
                    .div_euclid(per)
                    .max(0)
            });
            passes.entry(k).or_default().push(r);
        }
        let muted = l.muted.contains(&id);
        let split = passes.len() > 1;
        for (k, evs) in passes {
            let base = period.map_or(0, |per| per.saturating_mul(k));
            let lt = first.saturating_add(base);
            let lane_end = lane.length.map_or(i64::MAX, |t| {
                base.saturating_add(t.0).saturating_add(lane.play_shift.0)
            });
            let full = lt.saturating_add(total);
            let end_play = full.min(self.seq_len).min(lane_end);
            let looped = count > 1
                && iter_len.checked_mul(count) == Some(total)
                && end_play == full
                && same_passes(&evs, lt, iter_len, count);
            let mut clip = if looped {
                let pass0: Vec<Ev<'_>> = evs
                    .iter()
                    .copied()
                    .filter(|r| r.tick.0 < lt.saturating_add(iter_len))
                    .collect();
                let content = self.content(&pass0, lt)?;
                let mut c = note_clip(self.rel(lt), beats(Tick(total)), content);
                c.loop_start = Some(0.0);
                c.loop_end = Some(beats(Tick(iter_len)));
                (lt, c)
            } else {
                // What plays of a use ends before the song, the track and
                // the use do, so the clip's end is `end_play` or a note's.
                debug_assert!(
                    evs.iter().all(|r| r.tick.0 < end_play),
                    "a use's event at or past its end"
                );
                let lo = evs.iter().map(|r| r.tick.0).min().unwrap_or(lt);
                let start = lt.min(lo).max(l.origin);
                let end = evs
                    .iter()
                    .map(|r| end_of(r))
                    .max()
                    .unwrap_or(end_play)
                    .max(end_play)
                    .max(points_end(&evs));
                let content = self.content(&evs, start)?;
                (
                    start,
                    note_clip(
                        self.rel(start),
                        beats(Tick(end.saturating_sub(start).max(1))),
                        content,
                    ),
                )
            };
            let pass = u64::try_from(k).unwrap_or(0);
            clip.1.named = DpNamed {
                id: None,
                name: (!name.is_empty()).then(|| name.clone()),
                color: None,
                comment: Some(if split {
                    pass_comment(id.0, pass)
                } else {
                    tag(TagKind::Event, id.0)
                }),
            };
            if muted {
                clip.1.enable = Some(false);
            }
            out.push(clip);
        }
        Ok(())
    }

    /// The content of a note clip starting at `start`: its notes (with
    /// their expression) and controller points.
    fn content(&mut self, evs: &[Ev<'_>], start: i64) -> Result<Option<DpTimeline>, IoError> {
        struct N {
            tick: i64,
            end: i64,
            key: u8,
            channel: i32,
            note: DpNote,
            /// transpose, timbre, pressure, gain, pan.
            expr: [Vec<DpPoint>; 5],
        }
        let rel = |t: i64| beats(Tick(t.saturating_sub(start)));
        let mut notes: Vec<N> = Vec::new();
        let mut points: BTreeMap<PointsKey, DpPoints> = BTreeMap::new();
        let mut pressures: Vec<Ev<'_>> = Vec::new();
        for r in evs {
            self.ticker.tick()?;
            let (group, ch) = r.group_channel.unwrap_or((0, 0));
            if group != 0 {
                self.report.add(ItemKind::Changed, MIDI_GROUPS, None);
            }
            let ch = i32::from(ch & 0x0F);
            let time = rel(r.tick.0);
            match &r.body {
                EventBody::Note {
                    pitch,
                    velocity,
                    release_velocity,
                    duration,
                    attribute,
                    per_note,
                } => {
                    if attribute.is_some() {
                        self.report.add(ItemKind::LeftOut, NOTE_ATTRIBUTES, None);
                    }
                    let mut expr: [Vec<DpPoint>; 5] = Default::default();
                    for c in per_note {
                        let at = beats(c.offset);
                        let (slot, v) = match c.kind {
                            PerNoteKind::PitchBend => (0, semitones(c.value)),
                            PerNoteKind::Registered(74) => (1, norm32(c.value)),
                            PerNoteKind::Registered(7) => (3, norm32(c.value)),
                            PerNoteKind::Registered(10) => (4, norm32(c.value)),
                            PerNoteKind::Registered(_) | PerNoteKind::Assignable(_) => {
                                self.report.add(ItemKind::LeftOut, OTHER_PER_NOTE, None);
                                continue;
                            }
                        };
                        if let Some(list) = expr.get_mut(slot) {
                            list.push(hold(at, v));
                        }
                    }
                    notes.push(N {
                        tick: r.tick.0,
                        end: end_of(r),
                        key: *pitch,
                        channel: ch,
                        note: DpNote {
                            time,
                            duration: beats(*duration),
                            channel: ch,
                            key: i32::from(*pitch),
                            vel: Some(f64::from(*velocity) / 65_535.0),
                            rel: (*release_velocity > 0)
                                .then(|| f64::from(*release_velocity) / 65_535.0),
                            content: None,
                        },
                        expr,
                    });
                }
                EventBody::Cc { index, value } => {
                    push_point(
                        &mut points,
                        (0, ch, i32::from(*index)),
                        hold(time, norm32(*value)),
                    );
                }
                EventBody::Program { program, bank } => {
                    patch_points(&mut points, ch, time, *program, *bank);
                }
                EventBody::PitchBend { value } => {
                    push_point(&mut points, (2, ch, 0), hold(time, norm32(*value)));
                }
                EventBody::ChannelPressure { value } => {
                    push_point(&mut points, (3, ch, 0), hold(time, norm32(*value)));
                }
                EventBody::PolyPressure { .. } => pressures.push(r),
                EventBody::SysEx { .. }
                | EventBody::Text { .. }
                | EventBody::Reference { .. }
                | EventBody::AudioRegion { .. }
                | EventBody::Automation { .. } => {}
            }
        }
        // After every note, so pressure at a note's own tick finds it.
        // The notes of each key and channel, in time order.
        let mut by_key: BTreeMap<(u8, i32), Vec<usize>> = BTreeMap::new();
        for (i, n) in notes.iter().enumerate() {
            by_key.entry((n.key, n.channel)).or_default().push(i);
        }
        for r in pressures {
            let EventBody::PolyPressure { pitch, value } = &r.body else {
                continue;
            };
            let ch = i32::from(r.group_channel.map_or(0, |g| g.1) & 0x0F);
            let t = r.tick.0;
            // On a sounding note of its key: that note's pressure.
            let on = by_key.get(&(*pitch, ch)).and_then(|list| {
                let upto = list.partition_point(|&i| notes.get(i).is_some_and(|n| n.tick <= t));
                list.iter()
                    .take(upto)
                    .rev()
                    // Notes of one key rarely overlap deeply; the bound
                    // keeps a hostile clip linear.
                    .take(64)
                    .copied()
                    .find(|&i| notes.get(i).is_some_and(|n| t < n.end))
            });
            match on.and_then(|i| notes.get_mut(i)) {
                Some(n) => {
                    let at = beats(Tick(t.saturating_sub(n.tick)));
                    if let Some(list) = n.expr.get_mut(2) {
                        list.push(hold(at, norm32(*value)));
                    }
                }
                None => push_point(
                    &mut points,
                    (4, ch, i32::from(*pitch)),
                    hold(rel(t), norm32(*value)),
                ),
            }
        }
        let kinds = [
            (DpExpression::Transpose, DpUnit::Semitones),
            (DpExpression::Timbre, DpUnit::Normalized),
            (DpExpression::Pressure, DpUnit::Normalized),
            (DpExpression::Gain, DpUnit::Normalized),
            (DpExpression::Pan, DpUnit::Normalized),
        ];
        let mut written = Vec::new();
        for n in notes {
            let mut lanes = Vec::new();
            for ((expression, unit), mut pts) in kinds.into_iter().zip(n.expr) {
                if pts.is_empty() {
                    continue;
                }
                pts.sort_by(|a, b| a.time.total_cmp(&b.time));
                lanes.push(DpTimeline::Points(DpPoints {
                    head: DpHead::default(),
                    unit: Some(unit),
                    target: DpTarget {
                        expression: Some(expression),
                        ..DpTarget::default()
                    },
                    points: pts,
                }));
            }
            let mut note = n.note;
            note.content = together(lanes).map(Box::new);
            written.push(note);
        }
        self.notes = self
            .notes
            .saturating_add(u64::try_from(written.len()).unwrap_or(u64::MAX));
        let mut items = Vec::new();
        if !written.is_empty() {
            items.push(DpTimeline::Notes(DpNotes {
                head: DpHead::default(),
                notes: written,
            }));
        }
        items.extend(points.into_values().map(DpTimeline::Points));
        Ok(together(items))
    }

    /// The audio entry of `media` at `gain_db`, numbered on first use.
    fn entry(&mut self, media: MediaId, gain_db: f32, name: &str) -> String {
        // -0 and 0 share an entry.
        let gain_db = if gain_db == 0.0 { 0.0 } else { gain_db };
        let n = self.audio.len();
        let i = *self.entries.entry((media, gain_db.to_bits())).or_insert(n);
        if i == n {
            self.audio.push(AudioFile {
                entry: format!("audio/{:03}.wav", n.saturating_add(1)),
                media,
                gain_db,
                name: name.to_owned(),
            });
        }
        self.audio
            .get(i)
            .map(|a| a.entry.clone())
            .unwrap_or_default()
    }

    /// The audio clips of track `p`'s regions, with each crossfade's
    /// earlier clip fading out over the overlap (Ruling 10).
    fn audio_clips(
        &mut self,
        p: &Part<'_>,
        regions: &[Ev<'_>],
    ) -> Result<Vec<(i64, DpClip)>, IoError> {
        let l = self.l;
        let doc = l.doc;
        let tempo = &l.seq.tempo;
        let mut out: Vec<Region<'_>> = Vec::new();
        for r in regions {
            self.ticker.tick()?;
            let EventBody::AudioRegion {
                media,
                source_offset,
                length,
                gain_db,
                fade_in,
                fade_out,
                name,
                crossfade,
            } = &r.body
            else {
                continue;
            };
            // The model refuses a region whose media item is missing, on
            // insert and on load (`rules::check_event`, `rules::validate`).
            let Some(m) = doc.media().get(media) else {
                continue;
            };
            let Some(last) = r.path.last() else { continue };
            let top = r.path.len() == 1;
            if !top {
                self.report.add(ItemKind::Changed, NESTED_AUDIO, None);
            }
            // Media written without audio: listed once, its clips empty.
            let left_out = self.left_out.get(media);
            if let Some(what) = left_out
                && self.listed_media.insert(*media)
            {
                self.report
                    .add(ItemKind::LeftOut, what, Some(m.name.clone()));
            }
            let applied = match self.gain {
                _ if left_out.is_some() => 0.0,
                RegionGain::Apply => *gain_db,
                RegionGain::LeaveOut => {
                    if *gain_db != 0.0 {
                        let who = name.clone().unwrap_or_else(|| m.name.clone());
                        self.report.add(ItemKind::LeftOut, REGION_GAIN, Some(who));
                    }
                    0.0
                }
            };
            for f in [fade_in, fade_out] {
                if f.length_frames > 0 && f.shape != FadeShape::Linear {
                    self.report.add(ItemKind::Changed, FADE_SHAPES, None);
                }
            }
            let entry = match left_out {
                Some(_) => None,
                None => Some(self.entry(*media, applied, &m.name)),
            };
            let rate = f64::from(m.sample_rate.max(1));
            let secs = r.seconds;
            let end_secs = secs + *length as f64 / rate;
            let end = tempo.seconds_to_tick(end_secs).0;
            let duration = end.saturating_sub(r.tick.0).max(1);
            // A muted region, or one inside a muted use.
            let muted = r.path.first().is_some_and(|s| l.muted.contains(&s.event));
            let clip = DpClip {
                named: DpNamed {
                    id: None,
                    name: name.clone(),
                    color: None,
                    comment: Some(tag(TagKind::Event, last.event.0)),
                },
                time: self.rel(r.tick.0),
                duration: Some(beats(Tick(duration))),
                content_time_unit: entry.is_some().then_some(DpTimeUnit::Seconds),
                play_start: entry.is_some().then(|| *source_offset as f64 / rate),
                play_stop: entry
                    .is_some()
                    .then(|| source_offset.saturating_add(*length) as f64 / rate),
                loop_start: None,
                loop_end: None,
                fade_time_unit: None,
                fade_in: None,
                fade_out: None,
                enable: muted.then_some(false),
                reference: None,
                content: entry.map(|entry| {
                    Box::new(DpTimeline::Audio(DpAudio {
                        head: DpHead {
                            named: DpNamed {
                                name: Some(m.name.clone()),
                                ..DpNamed::default()
                            },
                            time_unit: Some(DpTimeUnit::Seconds),
                            track: None,
                        },
                        duration: m.length_frames as f64 / rate,
                        channels: i32::from(m.channels),
                        sample_rate: i32::try_from(m.sample_rate).unwrap_or(i32::MAX),
                        algorithm: None,
                        file: DpFileRef {
                            path: entry,
                            external: false,
                        },
                    }))
                }),
            };
            out.push(Region {
                r,
                id: last.event,
                crossfade: *crossfade,
                muted,
                rate,
                secs,
                end_secs,
                fade_in: fade_in.length_frames as f64 / rate,
                fade_out: fade_out.length_frames as f64 / rate,
                clip,
            });
        }

        // Crossfades: the earlier clip fades out over the whole overlap.
        let mut overlapped = false;
        // The latest end of the playing regions so far, in seconds.
        let mut last_end = f64::NEG_INFINITY;
        for j in 0..out.len() {
            self.ticker.tick()?;
            let Some(b) = out.get(j) else { continue };
            if b.muted {
                continue;
            }
            let sounding = (last_end * b.rate).round() > (b.secs * b.rate).round();
            last_end = last_end.max(b.end_secs);
            if !b.crossfade {
                overlapped |= sounding;
                continue;
            }
            let Ok(Some(o)) = core_model::crossfade::overlap(doc, b.id) else {
                continue;
            };
            let (prefix, tick, rate) = (
                b.r.path.split_last().map_or(&[][..], |(_, p)| p),
                b.r.tick.0,
                b.rate,
            );
            let partner = out
                .iter()
                .enumerate()
                .take(j)
                .filter(|(_, a)| {
                    a.id == o.with
                        && a.r.tick.0 < tick
                        && a.r.path.split_last().map_or(&[][..], |(_, p)| p) == prefix
                })
                .max_by_key(|(_, a)| a.r.tick.0)
                .map(|(k, _)| k);
            if let Some(a) = partner.and_then(|k| out.get_mut(k)) {
                a.fade_out = a.fade_out.max(o.frames as f64 / rate);
                self.report.add(ItemKind::Changed, CROSSFADES, None);
            }
        }
        if overlapped {
            self.report.add(ItemKind::Note, OVERLAPS, Some(p.name()));
        }
        self.regions = self
            .regions
            .saturating_add(u64::try_from(out.len()).unwrap_or(u64::MAX));
        Ok(out
            .into_iter()
            .map(|x| {
                let mut c = x.clip;
                if x.fade_in > 0.0 || x.fade_out > 0.0 {
                    c.fade_time_unit = Some(DpTimeUnit::Seconds);
                }
                c.fade_in = (x.fade_in > 0.0).then_some(x.fade_in);
                c.fade_out = (x.fade_out > 0.0).then_some(x.fade_out);
                (x.r.tick.0, c)
            })
            .collect())
    }

    /// The default patch and MIDI mix of the track's destinations, as
    /// points at time 0 (Ruling 13).
    fn patch_and_mix(&self, dests: &BTreeSet<DestId>) -> BTreeMap<PointsKey, DpPoints> {
        let mut map = BTreeMap::new();
        for d in dests.iter().filter_map(|d| self.l.doc.instruments().get(d)) {
            let ch = i32::from(dest_channel(d));
            if let Some((_, program, bank)) = written_patch(d) {
                patch_points(&mut map, ch, 0.0, program, bank);
            }
            if d.kind.is_external() {
                for (cc, v) in [(7, d.midi_mix.volume), (10, d.midi_mix.pan)] {
                    if let Some(v) = v {
                        push_point(&mut map, (0, ch, cc), hold(0.0, norm7(v)));
                    }
                }
            }
        }
        map
    }

    /// The track's automation, one `Points` per parameter in order of first
    /// use (Ruling 14).
    fn automation(
        &mut self,
        pi: usize,
        p: &Part<'_>,
        autos: &[Ev<'_>],
    ) -> Result<Vec<DpPoints>, IoError> {
        let l = self.l;
        let mut out: Vec<DpPoints> = Vec::new();
        let mut at: BTreeMap<String, usize> = BTreeMap::new();
        for r in autos {
            self.ticker.tick()?;
            let EventBody::Automation { target, value } = r.body else {
                continue;
            };
            match target {
                AutomationTarget::StripSolo(_) => {
                    self.report
                        .add(ItemKind::LeftOut, SOLO_AUTOMATION, Some(p.name()));
                    continue;
                }
                AutomationTarget::VelocityScale(_) => {
                    self.report
                        .add(ItemKind::LeftOut, VELOCITY_SCALE_AUTOMATION, Some(p.name()));
                    continue;
                }
                // Listed with the device (Task 4).
                AutomationTarget::PluginParam(_, q) if u32::try_from(q.0).is_err() => continue,
                _ => {}
            }
            let chan = l.target(pi, target).and_then(|(c, _)| l.chans.get(c));
            let Some(chan) = chan else {
                self.report.add(
                    ItemKind::LeftOut,
                    UNPLACED_AUTOMATION,
                    Some(target.describe()),
                );
                continue;
            };
            let id = &chan.id;
            let level = |v: f64| linear_gain(fader_db(v));
            let lin = |v: f64| DpPointValue::Real(v, Some(DpInterpolation::Linear));
            let (param, unit, v) = match target {
                AutomationTarget::StripFader(_) => {
                    (format!("{id}v"), Some(DpUnit::Linear), lin(level(value)))
                }
                AutomationTarget::StripPan(_) => {
                    (format!("{id}p"), Some(DpUnit::Normalized), lin(value))
                }
                AutomationTarget::StripMute(_) => {
                    (format!("{id}m"), None, DpPointValue::Bool(switch_on(value)))
                }
                AutomationTarget::SendLevel(_, i) => {
                    let has = chan.strip.is_some_and(|s| usize::from(i.0) < s.sends.len());
                    if !has {
                        self.report.add(
                            ItemKind::LeftOut,
                            UNPLACED_AUTOMATION,
                            Some(target.describe()),
                        );
                        continue;
                    }
                    (
                        format!("{id}s{}v", i.0),
                        Some(DpUnit::Linear),
                        lin(level(value)),
                    )
                }
                AutomationTarget::PluginParam(slot, q) => (
                    format!("{id}d{}q{}", slot.0, q.0),
                    Some(DpUnit::Normalized),
                    lin(value),
                ),
                AutomationTarget::StripSolo(_) | AutomationTarget::VelocityScale(_) => continue,
            };
            let i = match at.get(&param) {
                Some(i) => *i,
                None => {
                    out.push(DpPoints {
                        head: DpHead::default(),
                        unit,
                        target: DpTarget {
                            parameter: Some(param.clone()),
                            ..DpTarget::default()
                        },
                        points: Vec::new(),
                    });
                    at.insert(param, out.len() - 1);
                    out.len() - 1
                }
            };
            if let Some(a) = out.get_mut(i) {
                a.points.push(DpPoint {
                    time: self.rel(r.tick.0),
                    value: v,
                });
                self.points = self.points.saturating_add(1);
            }
        }
        Ok(out)
    }
}

/// Whether the `count` passes of `iter_len` from `lt` hold the same events
/// at the same offsets (none before `lt`).
fn same_passes(evs: &[Ev<'_>], lt: i64, iter_len: i64, count: i64) -> bool {
    type Key<'e> = (i64, &'e EventBody, Option<(u8, u8)>);
    let n = usize::try_from(count)
        .unwrap_or(usize::MAX)
        .min(evs.len().saturating_add(1));
    let mut passes: Vec<Vec<Key<'_>>> = vec![Vec::new(); n];
    for r in evs {
        let Some(d) = r.tick.0.checked_sub(lt).filter(|d| *d >= 0) else {
            return false;
        };
        let i = d / iter_len;
        let Some(pass) = usize::try_from(i).ok().and_then(|i| passes.get_mut(i)) else {
            return false;
        };
        pass.push((d - i * iter_len, &r.body, r.group_channel));
    }
    passes.windows(2).all(|w| w.first() == w.get(1))
}
