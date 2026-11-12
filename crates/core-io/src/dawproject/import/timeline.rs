//! The import's arrangement (plan 6d Task 7, Rulings 4, 10, 12–14 and 19
//! inverted, spec §7.4): each track's clips, notes, controller points,
//! audio and automation, the sequences the file shares, and the clip
//! launcher's slots.
//!
//! **Time.** A timeline's times are in its own `timeUnit`, else its
//! parent's (beats at the top); a clip's content is in its
//! `contentTimeUnit`, else the clip's unit. Every place is a [`Frame`]: the
//! tick and second where content time 0 lies, and the window of ticks that
//! plays. Seconds go through the imported tempo map ([`Clock`], the model's
//! own arithmetic with a binary search). A clip plays its content from
//! `playStart` to `playStop` (or for its `duration`), its loop repeating
//! from `loopStart` to `loopEnd`; nested clips compose and cut to the outer
//! window. Anything placed before 0 or past the timeline is left out.
//!
//! **Re-sharing.** A top-level note clip with a `reference` (or whose
//! content another clip references), a loop that starts at its
//! `loopStart`, or a name it shares with clips of identical content becomes
//! a use of one new sequence, one per destination; the rest go straight
//! onto their track. A note's channel is kept only when it is not its
//! track's main one (the one with the most notes).
//!
//! **Audio.** Regions take their media item from the converted sources
//! (`MediaSource`); fades become frames, linear; overlapping regions whose
//! fades cover the overlap, or a negative `fadeInTime`, become crossfades
//! (Ruling 10). All of a track's events go into one insert.
//!
//! Everything is bounded: clip nesting and reference chains at 16, and the
//! whole walk (each clip, pass, note and point it makes) at [`EXPANSION`]
//! times the file's own parts plus [`EXPANSION_FLOOR`], at most
//! [`Limits::default`]'s event count, so a small file of references cannot
//! grow into millions of notes; the cancel flag is looked at as everywhere
//! else in the mapping.

use super::*;
use crate::limits::{Limits, MAX_TIMELINE_NESTING};
use core_model::automation::{fader_norm, from_u32, to_u32};
use core_model::event::{EventBody, EventSpec, LoopCount, PerNoteCtl, PerNoteKind};
use core_model::mixer::ParamId;
use core_model::rules::MAX_LOOPS;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// ---------------------------------------------------------------- the report

/// Changed: notes that ran past their clip's end (or a loop's end).
pub const NOTES_CUT: &str = "Notes cut at clip ends";
/// Left out: notes with a key, channel, time or length VizDAW cannot hold.
pub const BAD_NOTES: &str = "Notes that could not be read";
/// Changed: velocities outside 0 to 1.
pub const VEL_CLAMPED: &str = "Velocities outside 0 to 1 were clamped";
/// Changed (L5): looped clips whose pass cannot start a sequence.
pub const UNROLLED: &str = "Looped clips with a different start were unrolled";
/// Note: shared clips split by destination.
pub const SHARED_SPLIT: &str =
    "Shared clips on tracks with different instruments became separate sequences";
/// Changed: note transposes past ±48 semitones.
pub const TRANSPOSE_CLAMPED: &str = "Note transposes beyond 48 semitones were clamped";
/// Left out: formant expression.
pub const FORMANT: &str = "Formant expression";
/// Left out: note expression VizDAW has no per-note kind for, or outside a
/// note.
pub const OTHER_EXPRESSION: &str = "Note expression outside notes, or of other kinds";
/// Changed: linear ramps between MIDI controller points.
pub const MIDI_RAMPS: &str = "MIDI ramps were written as their points";
/// Changed: controller and automation values outside 0 to 1.
pub const VALUES_CLAMPED: &str =
    "Controller and automation values outside their range were clamped";
/// Left out: warps that stretch time (by track).
pub const TIME_STRETCH: &str = "Time-stretching (clips placed unstretched)";
/// Note (Ruling 19): audio files that could not be read (`<name>: <why>`).
pub const OFFLINE: &str = "Audio files that could not be read (they are offline; relink them)";
/// Changed (Ruling 19): files wider than stereo, by new track.
pub const WIDE_AUDIO: &str = "Audio files wider than stereo were split into mono tracks";
/// Changed: audio clips that play past their file's end.
pub const AUDIO_CUT: &str = "Audio clips cut at the end of their file";
/// Changed: fades longer than their region.
pub const FADES_CUT: &str = "Fades longer than their clip were shortened";
/// Left out: fades of a clip that holds clips.
pub const NESTED_FADES: &str = "Fades of clips that hold clips";
/// Changed (Ruling 10): a negative `fadeInTime` over a region that leaves
/// no room for a crossfade.
pub const CROSSFADES_PLAIN: &str = "Crossfades the clips could not hold became fades";
/// Changed (Ruling 14): held automation steps.
pub const HELD_STEPS: &str = "Held automation steps were written as one-tick ramps";
/// Note (Ruling 14): automation with no track of its own.
pub const AUTOMATION_TRACK: &str =
    "Automation of the master and buses was put on a new track 'Automation'";
/// Left out: automation of parts the import does not hold (by parameter).
pub const UNPLACED: &str = "Automation of parts VizDAW does not hold";
/// Changed: clip launcher slots (a count).
pub const SLOTS: &str = "Clip launcher slots became unplaced sequences";
/// Left out: audio and automation in clip launcher slots, and slots
/// outside the scenes.
pub const SLOT_OTHER: &str = "Audio and automation in clip launcher slots";
/// Left out: video.
pub const VIDEO: &str = "Video";
/// Left out: markers inside a track's timelines.
pub const TRACK_MARKERS: &str = "Markers inside tracks";
/// Left out: clips, notes and audio on no note or audio track.
pub const NO_TRACK: &str = "Clips on no note or audio track";
/// Left out: clips and references nested past the limit.
pub const NESTED_TOO_DEEP: &str = "Clips nested more than 16 deep";
/// Left out: what lies past the walk's budget ([`EXPANSION`] times the
/// file's own clips, notes and points, plus [`EXPANSION_FLOOR`], at most
/// [`Limits::default`]'s 10 000 000 events).
pub const TOO_MUCH: &str = "Clips and notes repeated past the import's limit";
/// How many times its own parts the walk may make of a file (loops and
/// references inside clips are unrolled).
pub const EXPANSION: usize = 16;
/// The walk's budget beyond [`EXPANSION`] times the file's parts.
pub const EXPANSION_FLOOR: usize = 65_536;

/// The name of the track that holds automation with no track of its own.
const AUTOMATION_NAME: &str = "Automation";
/// The velocity of a note with no `vel` (MIDI's 100).
const DEFAULT_VEL: f64 = 100.0 / 127.0;
/// How far a warp may move a time and still count as none, in seconds.
const WARP_SLACK: f64 = 1e-3;
/// The last tick a part may start on, plus one.
const END: i64 = MAX_TICKS + 1;

// ---------------------------------------------------------------- the plan

/// A strip of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StripAt {
    /// The strip a note or audio track owns (its instrument's, else its
    /// audio path's, else the built-in synth's).
    Track(usize),
    /// A bus.
    Bus(usize),
    /// The master.
    Master,
}

/// A plug-in slot of a strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlotAt {
    /// A track's plug-in instrument.
    Instrument,
    /// The EQ slot.
    Eq,
    /// An insert, by index.
    Insert(usize),
}

/// What an automation event drives, before the mixer exists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum AutoAt {
    /// A fader.
    Fader(StripAt),
    /// A pan.
    Pan(StripAt),
    /// A mute.
    Mute(StripAt),
    /// A send's level, by index.
    Send(StripAt, usize),
    /// A plug-in parameter.
    Param(StripAt, SlotAt, ParamId),
}

/// An audio region before its media item exists.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RegionPlan {
    /// Its item in [`Arranged::media`].
    pub(super) media: usize,
    pub(super) source_offset: u64,
    pub(super) length: u64,
    pub(super) fade_in: u64,
    pub(super) fade_out: u64,
    pub(super) name: Option<String>,
    pub(super) crossfade: bool,
}

/// What a placed item is.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Body {
    /// A MIDI event with its group and channel override.
    Midi(EventBody, Option<(u8, u8)>),
    /// A use of [`Arranged::seqs`]' sequence.
    Use {
        seq: usize,
        length: Option<Tick>,
        loop_count: LoopCount,
    },
    /// An audio region.
    Region(RegionPlan),
    /// An automation point.
    Auto { at: AutoAt, value: f64 },
}

/// One item of a track.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Item {
    pub(super) tick: Tick,
    pub(super) mute: bool,
    pub(super) body: Body,
}

/// A new sequence: one track on track `track`'s destination.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SeqPlan {
    pub(super) name: String,
    pub(super) track: usize,
    /// Its locked length (> 0).
    pub(super) length: Tick,
    pub(super) events: Vec<EventSpec>,
}

/// A media item to add.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MediaPlan {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) rate: u32,
    pub(super) channels: u16,
    pub(super) frames: u64,
    pub(super) hash: Option<String>,
    pub(super) original: String,
}

/// The arrangement's part of the plan.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct Arranged {
    /// Each plan track's items, in time order.
    pub(super) items: Vec<Vec<Item>>,
    pub(super) seqs: Vec<SeqPlan>,
    pub(super) media: Vec<MediaPlan>,
}

// ---------------------------------------------------------------- time

/// One tempo segment: its start and end ticks, its tempos, and the seconds
/// at its start.
#[derive(Debug, Clone, Copy)]
struct Seg {
    start: i64,
    end: Option<i64>,
    b0: f64,
    b1: f64,
    acc: f64,
}

/// Seconds and ticks through a tempo map, as `TempoMap::tick_to_seconds`
/// and `seconds_to_tick` compute them (the same float operations in the
/// same order), found by binary search so a long map stays fast.
struct Clock {
    seg: Vec<Seg>,
    first: f64,
}

fn spt(bpm: f64) -> f64 {
    60.0 / (bpm * PPQ as f64)
}

fn seg_seconds(dx: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return dx * spt(b0);
    }
    let k = (b1 - b0) / len;
    60.0 / (PPQ as f64 * k) * (k * dx / b0).ln_1p()
}

fn seg_ticks(s: f64, len: f64, b0: f64, b1: f64) -> f64 {
    if (b1 - b0).abs() < 1e-9 || len <= 0.0 {
        return s / spt(b0);
    }
    let k = (b1 - b0) / len;
    b0 * (s * PPQ as f64 * k / 60.0).exp_m1() / k
}

impl Clock {
    fn new(map: &TempoMap) -> Clock {
        let pts: Vec<TempoPoint> = map.points().iter().copied().collect();
        let first = pts.first().map_or(120.0, |p| p.bpm.as_f64().max(1.0));
        let mut seg = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let next = pts.get(i + 1);
            let b0 = p.bpm.as_f64().max(1.0);
            let b1 = match next {
                Some(q) if p.ramp => q.bpm.as_f64().max(1.0),
                _ => b0,
            };
            let end = next.map(|q| q.tick.0);
            seg.push(Seg {
                start: p.tick.0,
                end,
                b0,
                b1,
                acc,
            });
            if let Some(e) = end {
                let len = (e - p.tick.0) as f64;
                acc += seg_seconds(len, len, b0, b1);
            }
        }
        Clock { seg, first }
    }

    /// Seconds from tick 0 to `t`.
    fn secs(&self, t: i64) -> f64 {
        if t < 0 || self.seg.is_empty() {
            return t as f64 * spt(self.first);
        }
        let i = self.seg.partition_point(|s| s.start <= t);
        let Some(s) = i.checked_sub(1).and_then(|i| self.seg.get(i)) else {
            return 0.0;
        };
        let seg_end = s.end.unwrap_or(i64::MAX);
        let len = (seg_end - s.start) as f64;
        let dx = (t.min(seg_end) - s.start) as f64;
        s.acc + seg_seconds(dx, len, s.b0, s.b1)
    }

    /// The tick at `s` seconds, rounded (saturating).
    fn tick(&self, s: f64) -> i64 {
        if s.is_nan() {
            return i64::MIN;
        }
        if s.is_infinite() {
            return if s > 0.0 { i64::MAX } else { i64::MIN };
        }
        if s < 0.0 || self.seg.is_empty() {
            return (s / spt(self.first)).round() as i64;
        }
        // The segments that end at or before `s`.
        let i = self
            .seg
            .get(1..)
            .map_or(0, |rest| rest.partition_point(|x| s >= x.acc));
        let Some(g) = self.seg.get(i) else {
            return 0;
        };
        let dx = match g.end {
            Some(e) => {
                let len = (e - g.start) as f64;
                seg_ticks(s - g.acc, len, g.b0, g.b1)
            }
            None => (s - g.acc) / spt(g.b0),
        };
        (g.start as f64 + dx).round() as i64
    }
}

/// A clip's fades, in `unit`.
#[derive(Debug, Clone, Copy)]
struct Fades {
    fin: Option<f64>,
    fout: Option<f64>,
    unit: DpTimeUnit,
}

/// Where a timeline's times land: content time 0 is at tick `zt` and
/// second `zs`; ticks in `[lo, hi)` play.
#[derive(Debug, Clone, Copy)]
struct Frame {
    unit: DpTimeUnit,
    zt: i64,
    zs: f64,
    lo: i64,
    hi: i64,
    /// The innermost clip's content window `[from, to)` in its content
    /// unit, for audio frames to come out exact.
    cut: Option<(f64, f64, DpTimeUnit)>,
    /// The innermost clip's fades.
    fades: Option<Fades>,
    /// The innermost named clip's name, in `Tl::clip_names`.
    name: Option<usize>,
    muted: bool,
}

impl Frame {
    fn top(unit: DpTimeUnit) -> Frame {
        Frame {
            unit,
            zt: 0,
            zs: 0.0,
            lo: 0,
            hi: END,
            cut: None,
            fades: None,
            name: None,
            muted: false,
        }
    }

    fn with_unit(mut self, u: Option<DpTimeUnit>) -> Frame {
        if let Some(u) = u {
            self.unit = u;
        }
        self
    }
}

/// The head of a timeline.
fn head_of(t: &DpTimeline) -> Option<&DpHead> {
    match t {
        DpTimeline::Lanes(x) => Some(&x.head),
        DpTimeline::Clips(x) => Some(&x.head),
        DpTimeline::Notes(x) => Some(&x.head),
        DpTimeline::Points(x) => Some(&x.head),
        DpTimeline::Audio(x) => Some(&x.head),
        DpTimeline::Warps(x) => Some(&x.head),
        DpTimeline::Markers(h, _) | DpTimeline::ClipSlot(h, _) | DpTimeline::Video(h) => Some(h),
        DpTimeline::Unknown(_) => None,
    }
}

/// A clip's loop, when it has a usable one: finite, at least a tick long
/// in beats (seconds are checked as the passes advance), and ending after
/// `playStart`.
fn loop_for(c: &DpClip) -> Option<(f64, f64)> {
    let (ls, le) = c.loop_start.zip(c.loop_end)?;
    let ps = c.play_start.unwrap_or(0.0);
    let long = le - ls >= 1.0 / PPQ as f64 || c.content_time_unit == Some(DpTimeUnit::Seconds);
    (ls.is_finite() && le.is_finite() && le > ls && ps < le && long).then_some((ls, le))
}

// ---------------------------------------------------------------- the work

/// A MIDI event at an absolute tick, with its channel.
#[derive(Debug, Clone, PartialEq)]
struct Ev {
    tick: i64,
    ch: u8,
    mute: bool,
    body: EventBody,
}

/// A region on its way to a track.
#[derive(Debug, Clone)]
struct Reg {
    track: usize,
    tick: i64,
    mute: bool,
    rate: f64,
    /// Its fade-in came from a negative `fadeInTime`.
    negative: bool,
    plan: RegionPlan,
}

/// An automation point on its way to a track (`None`: the automation
/// track).
#[derive(Debug, Clone)]
struct AutoEv {
    owner: Option<usize>,
    tick: i64,
    mute: bool,
    at: AutoAt,
    value: f64,
}

/// What makes clips share.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Kind<'p> {
    /// The timeline they play.
    Ref(&'p str),
    /// Their name.
    Name(Option<String>),
}

/// A top-level note clip that may become a use.
#[derive(Debug, Clone)]
struct Cand<'p> {
    track: usize,
    tick: i64,
    kind: Kind<'p>,
    name: Option<String>,
    /// Its number among its track's clips (from 1).
    n: usize,
    /// One pass of it, from tick 0.
    pass: Vec<Ev>,
    /// The pass's length (> 0).
    len: i64,
    /// How long it plays.
    total: i64,
    looped: bool,
    mute: bool,
}

/// A new sequence before its channels are known.
struct RawSeq {
    name: String,
    track: usize,
    len: i64,
    evs: Vec<Ev>,
}

/// Where flattened content goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum To {
    /// Onto a track (`None`: no track, which only automation may use).
    Track(Option<usize>),
    /// Into one pass of a clip that may be shared.
    Pass,
}

/// What a parameter of the structure is.
#[derive(Debug, Clone, Copy)]
enum Role<'p> {
    Volume(&'p DpRealParam),
    Pan(&'p DpRealParam),
    Mute,
    Send(&'p str, &'p DpRealParam),
    Device(&'p str, &'p DpRealParam),
}

enum Work<'p> {
    /// A timeline in a frame, inside `depth` clips.
    T(&'p DpTimeline, Frame, usize),
    /// A clip of a frame, inside `depth` clips.
    C(&'p DpClip, Frame, usize),
    /// The pass of a looped clip that plays content from `from` at clip
    /// offset `offset`, and the passes after it.
    P(Place, &'p DpTimeline, usize, f64, f64),
}

/// Where a clip lies in its parent's frame.
#[derive(Debug, Clone, Copy)]
struct Place {
    /// Its content's frame before placing (unit, window, fades, mute).
    base: Frame,
    start: i64,
    start_s: f64,
    end: Option<i64>,
    ps: f64,
    pe: Option<f64>,
    duration: Option<f64>,
    /// The unit of its time and duration.
    parent: DpTimeUnit,
    lp: Option<(f64, f64)>,
}

struct Tl<'c, 'a, 'p> {
    cx: &'c mut Cx<'a, 'p>,
    sources: &'c BTreeMap<usize, MediaSource>,
    clock: Clock,
    /// Timelines by ID.
    index: HashMap<&'p str, &'p DpTimeline>,
    /// The IDs clips reference.
    referenced: HashSet<&'p str>,
    /// Parameters by ID, with their channel's ID.
    params: HashMap<&'p str, (Role<'p>, &'p str)>,
    /// What the walk may still do.
    budget: usize,
    /// Every track's name: the plan's, then the new ones.
    names: Vec<String>,
    /// New tracks: (name, audio).
    extra: Vec<(String, bool)>,
    extra_at: HashMap<(usize, usize), usize>,
    automation_track: Option<usize>,
    /// Per track: its straight MIDI events and top-level clip count.
    evs: Vec<Vec<Ev>>,
    clips: Vec<usize>,
    regs: Vec<Reg>,
    autos: Vec<AutoEv>,
    cands: Vec<Cand<'p>>,
    slots: Vec<RawSeq>,
    pass: Vec<Ev>,
    /// The pass held audio or automation (so the clip is not shared).
    pass_other: bool,
    media: Vec<MediaPlan>,
    media_at: HashMap<usize, Vec<usize>>,
    paths: HashSet<String>,
    /// Per track: whether it owns a strip automation can drive.
    has_strip: Vec<bool>,
    /// The names of clips holding audio.
    clip_names: Vec<String>,
}

impl<'a, 'p> Cx<'a, 'p> {
    /// Fixes the send indices after `cut_loops` removed `cut` (the last
    /// first).
    pub(super) fn sends_cut(&mut self, cut: &[(usize, usize)]) {
        for &(bus, k) in cut {
            for (chan, i) in self.send_at.values_mut() {
                if self.targets.get(*chan) != Some(&Target::Bus(bus)) || *i == usize::MAX {
                    continue;
                }
                if *i == k {
                    *i = usize::MAX;
                } else if *i > k {
                    *i -= 1;
                }
            }
        }
    }

    /// The arrangement and the scenes of the package, mapped onto `plan`'s
    /// tracks (it may add some).
    pub(super) fn arrangement(
        &mut self,
        plan: &mut Plan,
        sources: &BTreeMap<usize, MediaSource>,
    ) -> Result<Arranged, IoError> {
        let n = plan.tracks.len();
        let mut tl = Tl {
            clock: Clock::new(&plan.tempo),
            index: HashMap::new(),
            referenced: HashSet::new(),
            params: HashMap::new(),
            budget: Limits::default().max_events,
            names: plan.tracks.iter().map(|t| t.name.clone()).collect(),
            extra: Vec::new(),
            extra_at: HashMap::new(),
            automation_track: None,
            evs: vec![Vec::new(); n],
            clips: vec![0; n],
            regs: Vec::new(),
            autos: Vec::new(),
            cands: Vec::new(),
            slots: Vec::new(),
            pass: Vec::new(),
            pass_other: false,
            media: Vec::new(),
            media_at: HashMap::new(),
            paths: HashSet::new(),
            has_strip: plan
                .tracks
                .iter()
                .map(|t| t.midi.is_some() || t.audio)
                .collect(),
            clip_names: Vec::new(),
            sources,
            cx: self,
        };
        tl.index()?;
        tl.params();
        tl.walk()?;
        tl.scenes()?;
        tl.finish(plan)
    }
}

impl<'p> Tl<'_, '_, 'p> {
    fn tick(&mut self) -> Result<(), IoError> {
        self.cx.tick()
    }

    /// Takes one unit of the walk's budget; false (listed once) when it
    /// is spent.
    fn spend(&mut self) -> bool {
        match self.budget {
            0 => false,
            1 => {
                self.budget = 0;
                self.cx.add(ItemKind::LeftOut, TOO_MUCH, None);
                false
            }
            _ => {
                self.budget -= 1;
                true
            }
        }
    }

    fn left_out(&mut self, what: &str) {
        self.cx.add(ItemKind::LeftOut, what, None);
    }

    fn bad_value(&mut self, what: &str) {
        self.cx
            .add(ItemKind::LeftOut, UNREADABLE_VALUES, Some(what.to_owned()));
    }

    fn name_of(&self, track: usize) -> String {
        self.names.get(track).cloned().unwrap_or_default()
    }

    // ---- indexes

    /// Every timeline with an ID, wherever it is; sets the budget from the
    /// parts the file holds.
    fn index(&mut self) -> Result<(), IoError> {
        let mut parts = 0usize;
        let p = &self.cx.pkg.project;
        let mut stack: Vec<&'p DpTimeline> = Vec::new();
        for s in p.scenes.iter().rev() {
            stack.extend(&s.content);
        }
        if let Some(l) = p.arrangement.as_ref().and_then(|a| a.lanes.as_ref()) {
            stack.extend(l.items.iter().rev());
        }
        while let Some(t) = stack.pop() {
            self.tick()?;
            if let Some(id) = head_of(t).and_then(|h| h.named.id.as_deref()) {
                self.index.insert(id, t);
            }
            parts = parts.saturating_add(1);
            match t {
                DpTimeline::Lanes(l) => stack.extend(l.items.iter().rev()),
                DpTimeline::Clips(c) => {
                    parts = parts.saturating_add(c.clips.len());
                    for clip in c.clips.iter().rev() {
                        self.referenced.extend(clip.reference.as_deref());
                        stack.extend(clip.content.as_deref());
                    }
                }
                DpTimeline::Notes(n) => {
                    parts = parts.saturating_add(n.notes.len());
                    for note in n.notes.iter().rev() {
                        stack.extend(note.content.as_deref());
                    }
                }
                DpTimeline::Points(p) => parts = parts.saturating_add(p.points.len()),
                DpTimeline::Warps(w) => stack.push(&w.content),
                DpTimeline::ClipSlot(_, Some(clip)) => {
                    self.referenced.extend(clip.reference.as_deref());
                    stack.extend(clip.content.as_deref());
                }
                _ => {}
            }
        }
        self.budget = EXPANSION
            .saturating_mul(parts)
            .saturating_add(EXPANSION_FLOOR)
            .min(Limits::default().max_events);
        Ok(())
    }

    /// Every channel's volume, pan, mute, send levels and device
    /// parameters.
    fn params(&mut self) {
        let p = &self.cx.pkg.project;
        let mut chans: Vec<&'p DpChannel> = Vec::new();
        let mut tracks: Vec<&'p DpTrack> = Vec::new();
        for item in &p.structure {
            match item {
                DpStructureItem::Track(t) => tracks.push(t),
                DpStructureItem::Channel(c) => chans.push(c),
            }
        }
        while let Some(t) = tracks.pop() {
            chans.extend(t.channel.as_deref());
            tracks.extend(&t.children);
        }
        for c in chans {
            let Some(chan) = c.named.id.as_deref() else {
                continue;
            };
            let mut put = |id: Option<&'p str>, role: Role<'p>| {
                if let Some(id) = id {
                    self.params.insert(id, (role, chan));
                }
            };
            if let Some(v) = c.volume.as_deref() {
                put(v.named.id.as_deref(), Role::Volume(v));
            }
            if let Some(v) = c.pan.as_deref() {
                put(v.named.id.as_deref(), Role::Pan(v));
            }
            if let Some(m) = c.mute.as_deref() {
                put(m.named.id.as_deref(), Role::Mute);
            }
            for s in &c.sends {
                if let Some(sid) = s.named.id.as_deref() {
                    put(s.volume.named.id.as_deref(), Role::Send(sid, &s.volume));
                }
            }
            for d in &c.devices {
                let Some(did) = d.named.id.as_deref() else {
                    continue;
                };
                for q in &d.parameters {
                    put(q.named.id.as_deref(), Role::Device(did, q));
                }
            }
        }
    }

    // ---- the walk

    /// A track ID as the walk sees it: `Some(Some(i))` for a note or audio
    /// track, `Some(None)` for another part, `None` when unknown.
    fn track_of(&self, id: Option<&str>) -> Option<Option<usize>> {
        id.and_then(|id| self.cx.track_index.get(id).copied())
    }

    /// The arrangement's timelines, down to the clips on each track.
    fn walk(&mut self) -> Result<(), IoError> {
        let Some(top) = self
            .cx
            .pkg
            .project
            .arrangement
            .as_ref()
            .and_then(|a| a.lanes.as_ref())
        else {
            return Ok(());
        };
        let frame = Frame::top(top.head.time_unit.unwrap_or(DpTimeUnit::Beats));
        let track = self.track_of(top.head.track.as_deref()).flatten();
        let mut stack: Vec<(&'p DpTimeline, Frame, Option<usize>)> =
            top.items.iter().rev().map(|t| (t, frame, track)).collect();
        while let Some((t, f, track)) = stack.pop() {
            self.tick()?;
            if self.budget == 0 {
                break;
            }
            let f = f.with_unit(head_of(t).and_then(|h| h.time_unit));
            let track = match self.track_of(head_of(t).and_then(|h| h.track.as_deref())) {
                Some(t) => t,
                None => track,
            };
            match t {
                DpTimeline::Lanes(l) => {
                    for i in l.items.iter().rev() {
                        stack.push((i, f, track));
                    }
                }
                DpTimeline::Clips(c) => {
                    for clip in &c.clips {
                        self.tick()?;
                        match track {
                            Some(i) => self.top_clip(clip, &f, i)?,
                            None => self.left_out(NO_TRACK),
                        }
                    }
                }
                _ => self.flatten(Work::T(t, f, 0), To::Track(track))?,
            }
        }
        Ok(())
    }

    /// A clip on track `track`'s top level: a candidate for sharing, or
    /// straight onto the track.
    fn top_clip(&mut self, c: &'p DpClip, f: &Frame, track: usize) -> Result<(), IoError> {
        let n = match self.clips.get_mut(track) {
            Some(k) => {
                *k = k.saturating_add(1);
                *k
            }
            None => 0,
        };
        let content = self.content_of(c);
        // The timeline it plays, when clips share it.
        let shared = c.reference.as_deref().or_else(|| {
            content
                .and_then(head_of)
                .and_then(|h| h.named.id.as_deref())
                .filter(|id| self.referenced.contains(id))
        });
        let name = c
            .named
            .name
            .as_deref()
            .map(|n| clean_name(n).0)
            .filter(|n| !n.is_empty());
        let ps = c.play_start.unwrap_or(0.0);
        let lp = loop_for(c);
        let looped = lp.is_some_and(|(ls, _)| (ps - ls).abs() < 1e-9);
        if lp.is_some() && !looped {
            self.cx.add(ItemKind::Changed, UNROLLED, None);
        }
        let candidate = (shared.is_some() || looped || name.is_some()) && (lp.is_none() || looped);
        if !candidate || content.is_none() {
            return self.flatten(Work::C(c, *f, 0), To::Track(Some(track)));
        }
        // One pass.
        let mut pf = *f;
        pf.muted = false;
        let Some(mut fr) = self.one_pass(c, &pf) else {
            return Ok(());
        };
        // The use carries the clip's mute; its sequence plays.
        fr.muted = false;
        let (start, pass_end) = (fr.lo, fr.hi);
        let total = match c.duration {
            Some(d) => self
                .at(f, c.time + d)
                .map_or(pass_end, |e| e.min(END))
                .saturating_sub(start),
            None => pass_end.saturating_sub(start),
        };
        let len = pass_end.saturating_sub(start);
        if pass_end >= END || len <= 0 || total <= 0 {
            return self.flatten(Work::C(c, *f, 0), To::Track(Some(track)));
        }
        self.pass.clear();
        self.pass_other = false;
        if let Some(t) = content {
            self.flatten(Work::T(t, fr, 1), To::Pass)?;
        }
        if self.pass_other {
            self.pass.clear();
            return self.flatten(Work::C(c, *f, 0), To::Track(Some(track)));
        }
        let mut pass = std::mem::take(&mut self.pass);
        for e in &mut pass {
            e.tick = e.tick.saturating_sub(start);
        }
        let kind = match shared {
            Some(id) => Kind::Ref(id),
            None => Kind::Name(name.clone()),
        };
        self.cands.push(Cand {
            track,
            tick: start,
            kind,
            name,
            n,
            pass,
            len,
            total,
            looped,
            mute: f.muted || c.enable == Some(false),
        });
        Ok(())
    }

    /// The frame of one pass of clip `c` of `f`: its loop's span from
    /// `loopStart` (not cut by the clip's end), else its play window.
    fn one_pass(&mut self, c: &DpClip, f: &Frame) -> Option<Frame> {
        let p = self.place_clip(c, f)?;
        match p.lp {
            Some((ls, _)) => self.pass(&p, ls, 0.0, true).0,
            None => self.plain(&p),
        }
    }

    /// The timeline a clip plays: its own, else the one it references.
    fn content_of(&self, c: &'p DpClip) -> Option<&'p DpTimeline> {
        c.content.as_deref().or_else(|| {
            c.reference
                .as_deref()
                .and_then(|r| self.index.get(r).copied())
        })
    }

    /// The scenes' clip slots, each as an unplaced sequence.
    fn scenes(&mut self) -> Result<(), IoError> {
        let scenes = &self.cx.pkg.project.scenes;
        for (si, s) in scenes.iter().enumerate() {
            let Some(content) = &s.content else { continue };
            let scene = s
                .named
                .name
                .as_deref()
                .map(|n| clean_name(n).0)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Scene {}", si.saturating_add(1)));
            let mut stack: Vec<(&'p DpTimeline, Frame, Option<usize>)> =
                vec![(content, Frame::top(DpTimeUnit::Beats), None)];
            while let Some((t, f, track)) = stack.pop() {
                self.tick()?;
                if self.budget == 0 {
                    return Ok(());
                }
                let f = f.with_unit(head_of(t).and_then(|h| h.time_unit));
                let track = match self.track_of(head_of(t).and_then(|h| h.track.as_deref())) {
                    Some(t) => t,
                    None => track,
                };
                match t {
                    DpTimeline::Lanes(l) => {
                        for i in l.items.iter().rev() {
                            stack.push((i, f, track));
                        }
                    }
                    DpTimeline::ClipSlot(_, None) => {}
                    DpTimeline::ClipSlot(_, Some(clip)) => match track {
                        Some(i) => self.slot(clip, &f, i, &scene)?,
                        None => self.left_out(NO_TRACK),
                    },
                    _ => self.left_out(SLOT_OTHER),
                }
            }
        }
        Ok(())
    }

    /// One clip launcher slot's clip on track `track`.
    fn slot(&mut self, c: &'p DpClip, f: &Frame, track: usize, scene: &str) -> Result<(), IoError> {
        let mut pf = *f;
        pf.muted = false;
        let Some(mut fr) = self.one_pass(c, &pf) else {
            return Ok(());
        };
        // The use carries the clip's mute; its sequence plays.
        fr.muted = false;
        self.pass.clear();
        self.pass_other = false;
        if let Some(t) = self.content_of(c) {
            self.flatten(Work::T(t, fr, 1), To::Pass)?;
        }
        if self.pass_other {
            self.left_out(SLOT_OTHER);
        }
        let mut evs = std::mem::take(&mut self.pass);
        let start = fr.lo;
        for e in &mut evs {
            e.tick = e.tick.saturating_sub(start);
        }
        let len = if fr.hi < END {
            fr.hi.saturating_sub(start)
        } else {
            evs.iter().map(ev_end).max().unwrap_or(0)
        };
        self.cx.add(ItemKind::Changed, SLOTS, None);
        let name = format!("{scene} \u{2013} {}", self.name_of(track));
        let (name, _) = clean_name(&name);
        self.slots.push(RawSeq {
            name,
            track,
            len: len.max(1),
            evs,
        });
        Ok(())
    }

    // ---- frames

    /// Content time `c` of frame `f` as a tick; `None` when it is not one.
    fn at(&self, f: &Frame, c: f64) -> Option<i64> {
        if !c.is_finite() {
            return None;
        }
        match f.unit {
            DpTimeUnit::Beats => crate::dawproject::units::ticks(c)
                .and_then(|(t, _)| f.zt.checked_add(t.0))
                .filter(|t| t.unsigned_abs() <= MAX_TICKS.unsigned_abs().saturating_mul(4)),
            DpTimeUnit::Seconds => {
                let s = f.zs + c;
                let t = self.clock.tick(s);
                (s.is_finite() && t.unsigned_abs() <= MAX_TICKS.unsigned_abs().saturating_mul(4))
                    .then_some(t)
            }
        }
    }

    /// The content time of `f` at tick `t`.
    fn content_at(&self, f: &Frame, unit: DpTimeUnit, t: i64) -> f64 {
        match unit {
            DpTimeUnit::Beats => t.saturating_sub(f.zt) as f64 / PPQ as f64,
            DpTimeUnit::Seconds => self.clock.secs(t) - f.zs,
        }
    }

    /// Seconds of the media at content time `c` of a clip's content in
    /// `unit` (content time 0 is the file's start).
    fn media_secs(&self, f: &Frame, unit: DpTimeUnit, c: f64) -> f64 {
        match unit {
            DpTimeUnit::Seconds => c,
            DpTimeUnit::Beats => {
                let t = crate::dawproject::units::ticks(c)
                    .and_then(|(t, _)| f.zt.checked_add(t.0))
                    .unwrap_or(i64::MAX);
                self.clock.secs(t) - f.zs
            }
        }
    }

    /// Where clip `c` of frame `f` lies; `None` (listed) for a clip that
    /// cannot be placed.
    fn place_clip(&mut self, c: &DpClip, f: &Frame) -> Option<Place> {
        let Some(start) = self.at(f, c.time).filter(|t| (0..END).contains(t)) else {
            self.left_out(PAST_END);
            return None;
        };
        let end = match c.duration {
            Some(d) if d.is_finite() && d >= 0.0 => {
                Some(self.at(f, c.time + d).map_or(END, |e| e.min(END)))
            }
            Some(_) => {
                self.bad_value("Clip@duration");
                return None;
            }
            None => None,
        };
        let cu = c.content_time_unit.unwrap_or(f.unit);
        let ps = c.play_start.unwrap_or(0.0);
        if !ps.is_finite() {
            self.bad_value("Clip@playStart");
            return None;
        }
        let lp = loop_for(c);
        if c.loop_start.is_some() && c.loop_end.is_some() && lp.is_none() {
            self.bad_value("Clip@loopEnd");
        }
        let fades = (c.fade_in.is_some() || c.fade_out.is_some()).then(|| Fades {
            fin: c.fade_in.filter(|v| v.is_finite()),
            fout: c.fade_out.filter(|v| v.is_finite() && *v >= 0.0),
            unit: c.fade_time_unit.unwrap_or(f.unit),
        });
        let name = match c.named.name.as_deref().map(|n| clean_name(n).0) {
            Some(n) if !n.is_empty() => {
                self.clip_names.push(n);
                Some(self.clip_names.len() - 1)
            }
            _ => f.name,
        };
        Some(Place {
            base: Frame {
                unit: cu,
                zt: 0,
                zs: 0.0,
                lo: f.lo.max(start),
                hi: f.hi.min(end.unwrap_or(END)),
                cut: None,
                fades,
                name,
                muted: f.muted || c.enable == Some(false),
            },
            start,
            start_s: self.clock.secs(start),
            end,
            ps,
            pe: c.play_stop.filter(|v| v.is_finite()),
            duration: c.duration,
            parent: f.unit,
            lp,
        })
    }

    /// The frame of `p` whose content time `from` lies at tick `at`
    /// (second `at_s`).
    fn zero(&self, p: &Place, from: f64, at: i64, at_s: f64) -> Option<Frame> {
        let mut fr = p.base;
        match fr.unit {
            DpTimeUnit::Beats => {
                let (t, _) = crate::dawproject::units::ticks(from)?;
                fr.zt = at.checked_sub(t.0)?;
                fr.zs = self.clock.secs(fr.zt);
            }
            DpTimeUnit::Seconds => {
                fr.zs = at_s - from;
                fr.zt = self.clock.tick(fr.zs);
            }
        }
        Some(fr)
    }

    /// The frame of a clip with no loop: its content from `playStart` to
    /// `playStop` (or for its duration).
    fn plain(&mut self, p: &Place) -> Option<Frame> {
        let Some(mut fr) = self.zero(p, p.ps, p.start, p.start_s) else {
            self.left_out(PAST_END);
            return None;
        };
        let cu = fr.unit;
        let to = match p.pe {
            Some(pe) => {
                if let Some(t) = self.at(&fr, pe) {
                    fr.hi = fr.hi.min(t);
                }
                pe
            }
            None => match (p.end, p.duration) {
                (Some(_), Some(d)) if cu == p.parent => p.ps + d,
                (Some(e), _) => self.content_at(&fr, cu, e),
                (None, _) => f64::INFINITY,
            },
        };
        fr.cut = Some((p.ps, to, cu));
        Some(fr)
    }

    /// Pass `at` of a looped clip: content from `from` to `loopEnd`, at
    /// clip offset `offset` (content units). Returns its frame (`None` when
    /// it does not play) and the next pass's `(from, offset)` (`None` after
    /// the last). With `one_pass`, the pass is not cut by the clip's end.
    fn pass(
        &mut self,
        p: &Place,
        from: f64,
        offset: f64,
        one_pass: bool,
    ) -> (Option<Frame>, Option<(f64, f64)>) {
        let Some((ls, le)) = p.lp else {
            return (None, None);
        };
        let (at, at_s) = match p.base.unit {
            DpTimeUnit::Beats => {
                match crate::dawproject::units::ticks(offset)
                    .and_then(|(t, _)| p.start.checked_add(t.0))
                {
                    Some(t) => (t, self.clock.secs(t)),
                    None => return (None, None),
                }
            }
            DpTimeUnit::Seconds => {
                let s = p.start_s + offset;
                (self.clock.tick(s), s)
            }
        };
        if at >= p.base.hi && !(one_pass && offset == 0.0) {
            return (None, None);
        }
        let fr = self.zero(p, from, at, at_s).map(|mut fr| {
            fr.lo = p.base.lo.max(at);
            let pass_end = self.at(&fr, le).unwrap_or(END).min(END);
            fr.hi = if one_pass {
                pass_end
            } else {
                p.base.hi.min(pass_end)
            };
            fr.cut = Some((from, le, fr.unit));
            fr
        });
        let next = (!one_pass && p.end.is_some() && fr.is_some_and(|f| f.hi > at))
            .then_some((ls, offset + (le - from)));
        (fr, next)
    }

    // ---- flattening

    /// Places `root` (and everything inside it) as `to` says.
    fn flatten(&mut self, root: Work<'p>, to: To) -> Result<(), IoError> {
        let mut stack = vec![root];
        while let Some(w) = stack.pop() {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            match w {
                Work::C(c, f, depth) => {
                    let depth = depth.saturating_add(1);
                    if depth > MAX_TIMELINE_NESTING {
                        self.left_out(NESTED_TOO_DEEP);
                        continue;
                    }
                    let Some(t) = self.content_of(c) else {
                        continue;
                    };
                    let Some(p) = self.place_clip(c, &f) else {
                        continue;
                    };
                    match p.lp {
                        Some(_) => stack.push(Work::P(p, t, depth, p.ps, 0.0)),
                        None => {
                            if let Some(fr) = self.plain(&p) {
                                stack.push(Work::T(t, fr, depth));
                            }
                        }
                    }
                }
                Work::P(p, t, depth, from, offset) => {
                    // One pass at a time, so a long loop holds no more
                    // than this one.
                    let (fr, next) = self.pass(&p, from, offset, false);
                    if let Some((from, offset)) = next {
                        stack.push(Work::P(p, t, depth, from, offset));
                    }
                    if let Some(fr) = fr {
                        stack.push(Work::T(t, fr, depth));
                    }
                }
                Work::T(t, f, depth) => {
                    let f = f.with_unit(head_of(t).and_then(|h| h.time_unit));
                    match t {
                        DpTimeline::Lanes(l) => {
                            for i in l.items.iter().rev() {
                                stack.push(Work::T(i, f, depth));
                            }
                        }
                        DpTimeline::Clips(cs) => {
                            if f.fades.is_some() {
                                self.left_out(NESTED_FADES);
                            }
                            let mut inner = f;
                            inner.fades = None;
                            for c in cs.clips.iter().rev() {
                                stack.push(Work::C(c, inner, depth));
                            }
                        }
                        DpTimeline::Notes(n) => self.notes(n, &f, to)?,
                        DpTimeline::Points(p) => self.points(p, &f, to)?,
                        DpTimeline::Audio(a) => self.audio(a, &f, to)?,
                        DpTimeline::Warps(w) => {
                            self.warps(w, &f, to);
                            stack.push(Work::T(&w.content, f, depth));
                        }
                        DpTimeline::Markers(..) => self.left_out(TRACK_MARKERS),
                        DpTimeline::Video(_) => self.left_out(VIDEO),
                        DpTimeline::ClipSlot(..) => self.left_out(SLOT_OTHER),
                        // Listed by the reader.
                        DpTimeline::Unknown(_) => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// A MIDI event: into the pass, onto the track, or (listed) nowhere.
    fn put(&mut self, to: To, e: Ev) {
        match to {
            To::Pass => self.pass.push(e),
            To::Track(Some(t)) => {
                if let Some(v) = self.evs.get_mut(t) {
                    v.push(e);
                }
            }
            To::Track(None) => self.left_out(NO_TRACK),
        }
    }

    /// A tick for a part that starts at content time `c`: `Some` inside the
    /// window; `None` outside it (listed when before 0 or past the end).
    fn start_at(&mut self, f: &Frame, c: f64) -> Option<i64> {
        match self.at(f, c) {
            Some(t) if (f.lo..f.hi).contains(&t) => Some(t),
            Some(t) if (0..END).contains(&t) => None,
            _ => {
                self.left_out(PAST_END);
                None
            }
        }
    }

    fn notes(&mut self, n: &'p DpNotes, f: &Frame, to: To) -> Result<(), IoError> {
        for note in &n.notes {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            let pitch = u8::try_from(note.key).ok().filter(|k| *k <= 127);
            let ch = u8::try_from(note.channel).ok().filter(|c| *c <= 15);
            let vel = note.vel.unwrap_or(DEFAULT_VEL);
            let rel = note.rel.unwrap_or(0.0);
            let (Some(pitch), Some(ch)) = (pitch, ch) else {
                self.left_out(BAD_NOTES);
                continue;
            };
            if !(note.duration.is_finite() && note.duration >= 0.0) {
                self.left_out(BAD_NOTES);
                continue;
            }
            let Some(start) = self.start_at(f, note.time) else {
                continue;
            };
            let Some(mut end) = self.at(f, note.time + note.duration) else {
                self.left_out(BAD_NOTES);
                continue;
            };
            if end > f.hi {
                end = f.hi;
                self.cx.add(ItemKind::Changed, NOTES_CUT, None);
            }
            let duration = end.saturating_sub(start).max(0);
            let scale = |tl: &mut Self, v: f64| -> u16 {
                let c = v.clamp(0.0, 1.0);
                if c != v {
                    tl.cx.add(ItemKind::Changed, VEL_CLAMPED, None);
                }
                (c * 65_535.0).round() as u16
            };
            let velocity = scale(self, vel);
            let release_velocity = scale(self, rel);
            let mut per_note: Vec<PerNoteCtl> = Vec::new();
            if let Some(content) = note.content.as_deref() {
                self.expression(
                    content,
                    f,
                    note,
                    start,
                    duration,
                    (pitch, ch),
                    &mut per_note,
                    to,
                )?;
            }
            per_note.sort_by_key(|c| c.offset);
            self.put(
                to,
                Ev {
                    tick: start,
                    ch,
                    mute: f.muted,
                    body: EventBody::Note {
                        pitch,
                        velocity,
                        release_velocity,
                        duration: Tick(duration),
                        attribute: None,
                        per_note: per_note.into_iter().collect(),
                    },
                },
            );
        }
        Ok(())
    }

    /// A note's expression (Ruling 12): point times from the note's start.
    #[allow(clippy::too_many_arguments)]
    fn expression(
        &mut self,
        content: &'p DpTimeline,
        f: &Frame,
        note: &DpNote,
        start: i64,
        duration: i64,
        (pitch, ch): (u8, u8),
        out: &mut Vec<PerNoteCtl>,
        to: To,
    ) -> Result<(), IoError> {
        let mut stack: Vec<(&'p DpTimeline, Frame)> = vec![(content, *f)];
        while let Some((t, f)) = stack.pop() {
            self.tick()?;
            let f = f.with_unit(head_of(t).and_then(|h| h.time_unit));
            let pts = match t {
                DpTimeline::Lanes(l) => {
                    for i in l.items.iter().rev() {
                        stack.push((i, f));
                    }
                    continue;
                }
                DpTimeline::Points(p) => p,
                _ => {
                    self.left_out(OTHER_EXPRESSION);
                    continue;
                }
            };
            let kind = match pts.target.expression {
                Some(DpExpression::Transpose) => None,
                Some(DpExpression::Timbre) => Some(PerNoteKind::Registered(74)),
                Some(DpExpression::Gain) => Some(PerNoteKind::Registered(7)),
                Some(DpExpression::Pan) => Some(PerNoteKind::Registered(10)),
                Some(DpExpression::Pressure) => Some(PerNoteKind::Assignable(0)),
                Some(DpExpression::Formant) => {
                    self.left_out(FORMANT);
                    continue;
                }
                _ => {
                    self.left_out(OTHER_EXPRESSION);
                    continue;
                }
            };
            self.ramps(&pts.points);
            for pt in &pts.points {
                self.tick()?;
                if !self.spend() {
                    return Ok(());
                }
                let DpPointValue::Real(v, _) = pt.value else {
                    self.bad_value("note expression point");
                    continue;
                };
                let Some(at) = self.at(&f, note.time + pt.time) else {
                    self.bad_value("note expression point");
                    continue;
                };
                let offset = at.saturating_sub(start);
                if offset < 0 {
                    self.bad_value("note expression point");
                    continue;
                }
                // Past a cut note's end: cut with it.
                if offset > duration {
                    continue;
                }
                match kind {
                    None => {
                        let c = v.clamp(-48.0, 48.0);
                        if c != v {
                            self.cx.add(ItemKind::Changed, TRANSPOSE_CLAMPED, None);
                        }
                        const CENTRE: f64 = 2_147_483_648.0;
                        let value = (CENTRE + c * CENTRE / 48.0)
                            .round()
                            .clamp(0.0, f64::from(u32::MAX))
                            as u32;
                        out.push(PerNoteCtl {
                            kind: PerNoteKind::PitchBend,
                            offset: Tick(offset),
                            value,
                        });
                    }
                    Some(PerNoteKind::Assignable(_)) => {
                        let value = self.norm(v);
                        self.put(
                            to,
                            Ev {
                                tick: start.saturating_add(offset),
                                ch,
                                mute: f.muted,
                                body: EventBody::PolyPressure { pitch, value },
                            },
                        );
                    }
                    Some(kind) => {
                        let value = self.norm(v);
                        out.push(PerNoteCtl {
                            kind,
                            offset: Tick(offset),
                            value,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// A normalised value as 32 bits; one outside 0 to 1 is clamped
    /// (Changed).
    fn norm(&mut self, v: f64) -> u32 {
        if !(0.0..=1.0).contains(&v) {
            self.cx.add(ItemKind::Changed, VALUES_CLAMPED, None);
        }
        to_u32(v)
    }

    /// Lists a linear ramp between points (MIDI keeps only the points).
    fn ramps(&mut self, pts: &[DpPoint]) {
        let ramp = pts.split_last().is_some_and(|(_, rest)| {
            rest.iter().any(|p| {
                matches!(
                    p.value,
                    DpPointValue::Real(_, Some(DpInterpolation::Linear))
                )
            })
        });
        if ramp {
            self.cx.add(ItemKind::Changed, MIDI_RAMPS, None);
        }
    }

    /// Controller points (Ruling 13) or automation (Ruling 14).
    fn points(&mut self, p: &'p DpPoints, f: &Frame, to: To) -> Result<(), IoError> {
        if let Some(param) = p.target.parameter.as_deref() {
            return match to {
                To::Pass => {
                    self.pass_other = true;
                    Ok(())
                }
                To::Track(track) => self.automation(p, param, f, track),
            };
        }
        let tg = &p.target;
        let ch = match tg.channel {
            None => None,
            Some(c) => match u8::try_from(c).ok().filter(|c| *c <= 15) {
                Some(c) => Some(c),
                None => {
                    self.bad_value("Target@channel");
                    return Ok(());
                }
            },
        };
        let seven = |v: Option<i32>| v.and_then(|v| u8::try_from(v).ok()).filter(|v| *v <= 127);
        enum K {
            Cc(u8),
            Bend,
            Chan,
            Poly(u8),
            Program,
        }
        let k = match tg.expression {
            Some(DpExpression::ChannelController) => match seven(tg.controller) {
                Some(n) => K::Cc(n),
                None => {
                    self.bad_value("Target@controller");
                    return Ok(());
                }
            },
            Some(DpExpression::PitchBend) => K::Bend,
            Some(DpExpression::ChannelPressure) => K::Chan,
            Some(DpExpression::PolyPressure) => match seven(tg.key) {
                Some(n) => K::Poly(n),
                None => {
                    self.bad_value("Target@key");
                    return Ok(());
                }
            },
            Some(DpExpression::ProgramChange) => K::Program,
            Some(DpExpression::Formant) => {
                self.left_out(FORMANT);
                return Ok(());
            }
            Some(_) => {
                self.left_out(OTHER_EXPRESSION);
                return Ok(());
            }
            None => {
                self.bad_value("Points target");
                return Ok(());
            }
        };
        self.ramps(&p.points);
        for pt in &p.points {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            let Some(tick) = self.start_at(f, pt.time) else {
                continue;
            };
            let body = match (&k, &pt.value) {
                (K::Program, DpPointValue::Integer(v) | DpPointValue::Enum(v)) => {
                    match seven(Some(*v)) {
                        Some(program) => EventBody::Program {
                            program,
                            bank: None,
                        },
                        None => {
                            self.bad_value("programChange point");
                            continue;
                        }
                    }
                }
                (K::Program, DpPointValue::Real(v, _)) if v.fract() == 0.0 => {
                    match (*v >= 0.0 && *v <= 127.0).then_some(*v as u8) {
                        Some(program) => EventBody::Program {
                            program,
                            bank: None,
                        },
                        None => {
                            self.bad_value("programChange point");
                            continue;
                        }
                    }
                }
                (K::Program, _) => {
                    self.bad_value("programChange point");
                    continue;
                }
                (_, DpPointValue::Real(v, _)) => {
                    let value = self.norm(*v);
                    match k {
                        K::Cc(index) => EventBody::Cc { index, value },
                        K::Bend => EventBody::PitchBend { value },
                        K::Chan => EventBody::ChannelPressure { value },
                        K::Poly(pitch) => EventBody::PolyPressure { pitch, value },
                        K::Program => continue,
                    }
                }
                _ => {
                    self.bad_value("controller point");
                    continue;
                }
            };
            self.put(
                to,
                Ev {
                    tick,
                    // No channel: the track's main one (filled in later).
                    ch: ch.unwrap_or(u8::MAX),
                    mute: f.muted,
                    body,
                },
            );
        }
        Ok(())
    }

    /// Automation of parameter `param` (Ruling 14, inverted).
    fn automation(
        &mut self,
        p: &'p DpPoints,
        param: &'p str,
        f: &Frame,
        track: Option<usize>,
    ) -> Result<(), IoError> {
        let Some(&(role, chan)) = self.params.get(param) else {
            self.cx
                .add(ItemKind::LeftOut, UNPLACED, Some(param.to_owned()));
            return Ok(());
        };
        let strip = match self.cx.chan_track.get(chan) {
            Some(&i) => Some(StripAt::Track(i)),
            None => match self.cx.targets.get(chan) {
                Some(Target::Master) => Some(StripAt::Master),
                Some(Target::Bus(b)) => Some(StripAt::Bus(*b)),
                _ => None,
            },
        };
        let at = strip.and_then(|s| match role {
            Role::Volume(_) => Some(AutoAt::Fader(s)),
            Role::Pan(_) if s != StripAt::Master => Some(AutoAt::Pan(s)),
            Role::Pan(_) => None,
            Role::Mute => Some(AutoAt::Mute(s)),
            Role::Send(id, _) => self
                .cx
                .send_at
                .get(id)
                .filter(|(_, i)| *i != usize::MAX)
                .map(|(_, i)| AutoAt::Send(s, *i)),
            Role::Device(dev, q) => {
                let slot = self.cx.dev_slot.get(dev).copied()?;
                let id = q.parameter_id?;
                // The parameter's 32 bits (Ruling 7).
                Some(AutoAt::Param(s, slot, ParamId(u64::from(id as u32))))
            }
        });
        let owns = |tl: &Self, i: usize| tl.has_strip.get(i).copied().unwrap_or(false);
        let at = at.filter(|a| match a {
            AutoAt::Fader(StripAt::Track(i))
            | AutoAt::Pan(StripAt::Track(i))
            | AutoAt::Mute(StripAt::Track(i))
            | AutoAt::Send(StripAt::Track(i), _)
            | AutoAt::Param(StripAt::Track(i), _, _) => owns(self, *i),
            _ => true,
        });
        let Some(at) = at else {
            self.cx
                .add(ItemKind::LeftOut, UNPLACED, Some(param.to_owned()));
            return Ok(());
        };
        let owner = track.or(match strip {
            Some(StripAt::Track(i)) => Some(i),
            _ => None,
        });
        if owner.is_none() && self.automation_track.is_none() {
            let i = self.new_track(AUTOMATION_NAME.to_owned(), false);
            self.automation_track = Some(i);
            self.cx.add(ItemKind::Note, AUTOMATION_TRACK, None);
        }
        let owner = owner.or(self.automation_track);
        let unit = p.unit.unwrap_or(match role {
            Role::Volume(r) | Role::Pan(r) | Role::Send(_, r) | Role::Device(_, r) => r.unit,
            Role::Mute => DpUnit::Normalized,
        });
        let stepped = matches!(at, AutoAt::Mute(_));
        let mut held = false;
        let mut last: Option<(i64, f64, bool)> = None;
        for pt in &p.points {
            self.tick()?;
            if !self.spend() {
                return Ok(());
            }
            let Some(tick) = self.start_at(f, pt.time) else {
                continue;
            };
            let Some(value) = self.auto_value(role, unit, &pt.value) else {
                self.bad_value("automation point");
                continue;
            };
            if let Some((t0, v0, true)) = last
                && !stepped
                && tick > t0.saturating_add(1)
            {
                held = true;
                self.autos.push(AutoEv {
                    owner,
                    tick: tick - 1,
                    mute: f.muted,
                    at,
                    value: v0,
                });
            }
            let hold = matches!(pt.value, DpPointValue::Real(_, Some(DpInterpolation::Hold)));
            last = Some((tick, value, hold));
            self.autos.push(AutoEv {
                owner,
                tick,
                mute: f.muted,
                at,
                value,
            });
        }
        if held {
            self.cx.add(ItemKind::Changed, HELD_STEPS, None);
        }
        Ok(())
    }

    /// An automation point's value, 0 to 1.
    fn auto_value(&mut self, role: Role<'p>, unit: DpUnit, v: &DpPointValue) -> Option<f64> {
        let scaled = |r: &DpRealParam, v: f64| -> Option<f64> {
            match (r.min, r.max) {
                (Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() && hi > lo => {
                    Some((v - lo) / (hi - lo))
                }
                _ => None,
            }
        };
        let raw = match (role, v) {
            (Role::Mute, DpPointValue::Bool(b)) => f64::from(u8::from(*b)),
            (Role::Mute, DpPointValue::Real(x, _)) => f64::from(u8::from(*x >= 0.5)),
            (Role::Mute, DpPointValue::Integer(x)) => f64::from(u8::from(*x != 0)),
            (Role::Volume(_) | Role::Send(..), DpPointValue::Real(x, _)) => match unit {
                DpUnit::Linear | DpUnit::Normalized => fader_norm(db_of_linear(*x)),
                DpUnit::Decibel => fader_norm(*x),
                _ => return None,
            },
            (Role::Pan(r) | Role::Device(_, r), DpPointValue::Real(x, _)) => match unit {
                DpUnit::Normalized => *x,
                _ => scaled(r, *x)?,
            },
            (Role::Device(_, r), DpPointValue::Integer(x) | DpPointValue::Enum(x)) => {
                scaled(r, f64::from(*x))?
            }
            (Role::Device(..), DpPointValue::Bool(b)) => f64::from(u8::from(*b)),
            _ => return None,
        };
        if !raw.is_finite() {
            return None;
        }
        if !(0.0..=1.0).contains(&raw) {
            self.cx.add(ItemKind::Changed, VALUES_CLAMPED, None);
        }
        Some(raw.clamp(0.0, 1.0))
    }

    /// Lists warps that stretch time (Ruling 4: the audio is placed
    /// unstretched either way).
    fn warps(&mut self, w: &DpWarps, f: &Frame, to: To) {
        let secs = |tl: &Self, unit: DpTimeUnit, t: f64| tl.media_secs(f, unit, t);
        let stretched = w.warps.iter().any(|&(t, ct)| {
            let a = secs(self, f.unit, t);
            let b = secs(self, w.content_time_unit, ct);
            !(a - b).abs().le(&WARP_SLACK)
        });
        if stretched {
            let detail = match to {
                To::Track(Some(t)) => Some(self.name_of(t)),
                _ => None,
            };
            self.cx.add(ItemKind::LeftOut, TIME_STRETCH, detail);
        }
    }

    /// A new track (named, with an audio path when `audio`); its index.
    fn new_track(&mut self, name: String, audio: bool) -> usize {
        let i = self.names.len();
        self.names.push(name.clone());
        self.extra.push((name, audio));
        self.evs.push(Vec::new());
        self.clips.push(0);
        self.has_strip.push(audio);
        i
    }

    // ---- audio

    /// The plan's media items for audio file `key`, one per channel part;
    /// made on first use (Ruling 19).
    fn media_for(&mut self, key: usize) -> Vec<usize> {
        if let Some(v) = self.media_at.get(&key) {
            return v.clone();
        }
        let Some(aref) = self.cx.pkg.audio().get(key).cloned() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let why = match self.sources.get(&key) {
            Some(MediaSource::Online(items)) if !items.is_empty() => {
                for it in items {
                    self.paths.insert(it.path.clone());
                    self.media.push(MediaPlan {
                        path: it.path.clone(),
                        name: it.name.clone(),
                        rate: it.sample_rate,
                        channels: it.channels,
                        frames: it.length_frames,
                        hash: Some(it.content_hash.clone()),
                        original: it.original_name.clone(),
                    });
                    out.push(self.media.len() - 1);
                }
                None
            }
            Some(MediaSource::Offline { why }) => Some(why.clone()),
            _ => Some("it was not read".to_owned()),
        };
        if let Some(why) = why {
            self.cx.add(
                ItemKind::Note,
                OFFLINE,
                Some(format!("{}: {why}", aref.name)),
            );
            if aref.sample_rate > 0 && aref.channels > 0 && aref.frames > 0 {
                let path = self.offline_path(&aref.name);
                let original = match &aref.source {
                    crate::dawproject::package::AudioSource::Entry(p) => p.clone(),
                    crate::dawproject::package::AudioSource::External(p) => {
                        p.to_string_lossy().into_owned()
                    }
                };
                let original = original
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                self.media.push(MediaPlan {
                    path,
                    name: aref.name.clone(),
                    rate: aref.sample_rate,
                    channels: aref.channels,
                    frames: aref.frames,
                    hash: None,
                    original,
                });
                out.push(self.media.len() - 1);
            }
        }
        self.media_at.insert(key, out.clone());
        out
    }

    /// `Media/<name>.caf`, made safe and unique among the import's items.
    fn offline_path(&mut self, name: &str) -> String {
        let stem: String = name
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':') || c.is_control() {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        let mut stem = stem.trim().trim_start_matches('.').to_owned();
        let (cut, _) = super::maps::cut_to(&stem, 200);
        stem = cut.trim_end().to_owned();
        if stem.is_empty() {
            stem = "Audio".to_owned();
        }
        let mut path = format!("Media/{stem}.caf");
        let mut n = 2u32;
        while self.paths.contains(&path) {
            path = format!("Media/{stem} {n}.caf");
            n = n.saturating_add(1);
        }
        self.paths.insert(path.clone());
        path
    }

    fn audio(&mut self, a: &'p DpAudio, f: &Frame, to: To) -> Result<(), IoError> {
        let track = match to {
            To::Pass => {
                self.pass_other = true;
                return Ok(());
            }
            To::Track(None) => {
                self.left_out(NO_TRACK);
                return Ok(());
            }
            To::Track(Some(t)) => t,
        };
        if a.sample_rate <= 0 {
            self.bad_value("Audio@sampleRate");
            return Ok(());
        }
        if a.channels <= 0 {
            self.bad_value("Audio@channels");
            return Ok(());
        }
        let Some(key) = self.cx.pkg.audio_key(&a.file) else {
            return Ok(());
        };
        let items = self.media_for(key);
        let Some(first) = items.first().and_then(|i| self.media.get(*i)).cloned() else {
            return Ok(());
        };
        let rate = f64::from(first.rate.max(1));
        // The media window in seconds: the clip's content window, cut by
        // the outer windows.
        let (mut from_s, mut to_s) = match f.cut {
            Some((from, to, DpTimeUnit::Seconds)) => (from, to),
            Some((from, to, unit)) => (
                self.media_secs(f, unit, from),
                if to.is_finite() {
                    self.media_secs(f, unit, to)
                } else {
                    f64::INFINITY
                },
            ),
            None => (0.0, f64::INFINITY),
        };
        if f.lo > self.clock.tick(f.zs + from_s) {
            from_s = self.clock.secs(f.lo) - f.zs;
        }
        if f.hi < END && f.hi < self.clock.tick(f.zs + to_s) {
            to_s = self.clock.secs(f.hi) - f.zs;
        }
        from_s = from_s.max(0.0);
        if !(from_s.is_finite() && to_s > from_s) {
            return Ok(());
        }
        let tick = self.clock.tick(f.zs + from_s);
        if !(0..END).contains(&tick) {
            self.left_out(PAST_END);
            return Ok(());
        }
        let offset = (from_s * rate).round();
        let mut end = if to_s.is_finite() {
            (to_s * rate).round()
        } else {
            f64::INFINITY
        };
        let frames = first.frames as f64;
        if offset >= frames {
            self.cx.add(ItemKind::Changed, AUDIO_CUT, None);
            return Ok(());
        }
        if end > frames {
            if end.is_finite() {
                self.cx.add(ItemKind::Changed, AUDIO_CUT, None);
            }
            end = frames;
        }
        // Within 0 … the file's frames, so the casts are exact.
        let (mut source_offset, mut length) = (offset as u64, (end - offset) as u64);
        if length == 0 {
            return Ok(());
        }
        let mut tick = tick;
        let (mut fade_in, mut fade_out, mut negative) = (0u64, 0u64, false);
        if let Some(fd) = f.fades {
            let start_s = self.clock.secs(tick);
            let end_tick = self.clock.tick(f.zs + to_s.min(frames / rate));
            if let Some(fin) = fd.fin {
                if fin < 0.0 {
                    // The format's crossfade form: the clip starts |f|
                    // earlier, fading in over it.
                    let ext_s = match fd.unit {
                        DpTimeUnit::Seconds => -fin,
                        DpTimeUnit::Beats => crate::dawproject::units::ticks(-fin)
                            .map_or(0.0, |(t, _)| start_s - self.clock.secs(tick - t.0)),
                    };
                    let ext = ((ext_s * rate).round().max(0.0) as u64)
                        .min(source_offset)
                        .min((start_s * rate).round().max(0.0) as u64);
                    source_offset -= ext;
                    length = length.saturating_add(ext);
                    tick = self.clock.tick(start_s - ext as f64 / rate);
                    fade_in = ext;
                    negative = ext > 0;
                } else {
                    fade_in = self.fade_frames(fin, fd.unit, tick, rate, true);
                }
            }
            if let Some(fout) = fd.fout {
                fade_out = self.fade_frames(fout, fd.unit, end_tick, rate, false);
            }
        }
        if fade_in.saturating_add(fade_out) > length {
            self.cx.add(ItemKind::Changed, FADES_CUT, None);
            fade_out = fade_out.min(length.saturating_sub(fade_in.min(length)));
            fade_in = fade_in.min(length);
        }
        for (part, &m) in items.iter().enumerate() {
            let t = if part == 0 {
                track
            } else {
                self.part_track(track, part)
            };
            self.regs.push(Reg {
                track: t,
                tick,
                mute: f.muted,
                rate,
                negative,
                plan: RegionPlan {
                    media: m,
                    source_offset,
                    length,
                    fade_in,
                    fade_out,
                    name: f.name.and_then(|i| self.clip_names.get(i)).cloned(),
                    crossfade: false,
                },
            });
        }
        Ok(())
    }

    /// A fade of `v` in `unit` as frames: from `at` forward (`fade_in`) or
    /// back.
    fn fade_frames(&self, v: f64, unit: DpTimeUnit, at: i64, rate: f64, fade_in: bool) -> u64 {
        let secs = match unit {
            DpTimeUnit::Seconds => v,
            DpTimeUnit::Beats => match crate::dawproject::units::ticks(v) {
                Some((t, _)) if fade_in => {
                    self.clock.secs(at.saturating_add(t.0)) - self.clock.secs(at)
                }
                Some((t, _)) => self.clock.secs(at) - self.clock.secs(at.saturating_sub(t.0)),
                None => 0.0,
            },
        };
        let f = (secs * rate).round();
        if f.is_finite() && f > 0.0 {
            f.min(u64::MAX as f64) as u64
        } else {
            0
        }
    }

    /// The track for channel `part` (from 0) of a wide file on `track`.
    fn part_track(&mut self, track: usize, part: usize) -> usize {
        if let Some(&t) = self.extra_at.get(&(track, part)) {
            return t;
        }
        let name = format!("{} ch {}", self.name_of(track), part.saturating_add(1));
        let (name, _) = clean_name(&name);
        let t = self.new_track(name.clone(), true);
        self.extra_at.insert((track, part), t);
        self.cx.add(ItemKind::Changed, WIDE_AUDIO, Some(name));
        t
    }
}

impl<'p> Tl<'_, '_, 'p> {
    /// Makes the plan's items: new tracks join the plan, candidates group
    /// into sequences, regions get their crossfades, channels their
    /// overrides.
    fn finish(mut self, plan: &mut Plan) -> Result<Arranged, IoError> {
        for (name, audio) in std::mem::take(&mut self.extra) {
            plan.tracks.push(TrackPlan {
                name,
                midi: None,
                audio,
                mix: Mix::default(),
                flags: None,
            });
        }
        let n = plan.tracks.len();
        // Notes or audio on a track that did not say it holds them.
        let mut midi_used = vec![false; n];
        for (i, v) in self.evs.iter().enumerate() {
            if !v.is_empty()
                && let Some(u) = midi_used.get_mut(i)
            {
                *u = true;
            }
        }
        for t in self
            .cands
            .iter()
            .map(|c| c.track)
            .chain(self.slots.iter().map(|s| s.track))
        {
            if let Some(u) = midi_used.get_mut(t) {
                *u = true;
            }
        }
        for (i, t) in plan.tracks.iter_mut().enumerate() {
            if midi_used.get(i).copied().unwrap_or(false) && t.midi.is_none() {
                t.midi = Some(Midi::Builtin);
                self.cx
                    .report
                    .add(ItemKind::Note, PLAYS_BUILTIN, Some(t.name.clone()));
            }
        }
        if plan.builtin.is_none() && plan.tracks.iter().any(|t| t.midi == Some(Midi::Builtin)) {
            plan.builtin = Some(Mix::default());
        }
        for r in &self.regs {
            if let Some(t) = plan.tracks.get_mut(r.track) {
                t.audio = true;
            }
        }

        let mut items: Vec<Vec<Item>> = vec![Vec::new(); n];
        let mut seqs: Vec<RawSeq> = Vec::new();
        self.group(plan, &mut items, &mut seqs)?;
        seqs.append(&mut self.slots);

        // Each track's main channel: the one most of its notes are on.
        let mut counts = vec![[0u64; 16]; n];
        let mut count = |t: usize, e: &Ev| {
            if matches!(e.body, EventBody::Note { .. })
                && let Some(c) = counts.get_mut(t).and_then(|c| c.get_mut(usize::from(e.ch)))
            {
                *c = c.saturating_add(1);
            }
        };
        for (t, v) in self.evs.iter().enumerate() {
            v.iter().for_each(|e| count(t, e));
        }
        for s in &seqs {
            s.evs.iter().for_each(|e| count(s.track, e));
        }
        let main: Vec<u8> = counts
            .iter()
            .map(|c| {
                let mut best = 0u8;
                for (ch, k) in (0u8..).zip(c.iter()) {
                    if *k > c.get(usize::from(best)).copied().unwrap_or(0) {
                        best = ch;
                    }
                }
                best
            })
            .collect();
        let gc = |main: u8, ch: u8| (ch != u8::MAX && ch != main).then_some((0, ch));

        for (t, v) in self.evs.iter_mut().enumerate() {
            pair_banks(v);
            let m = main.get(t).copied().unwrap_or(0);
            if let Some(out) = items.get_mut(t) {
                for e in v.drain(..) {
                    out.push(Item {
                        tick: Tick(e.tick),
                        mute: e.mute,
                        body: Body::Midi(e.body, gc(m, e.ch)),
                    });
                }
            }
        }

        // Regions, by track, with their crossfades (Ruling 10).
        let mut regs = std::mem::take(&mut self.regs);
        regs.sort_by_key(|r| (r.track, r.tick));
        let mut from = 0;
        while from < regs.len() {
            self.tick()?;
            let track = regs.get(from).map_or(0, |r| r.track);
            let to = from
                + regs
                    .get(from..)
                    .map_or(0, |r| r.partition_point(|x| x.track == track));
            if let Some(slice) = regs.get_mut(from..to) {
                self.crossfades(slice);
            }
            from = to.max(from + 1);
        }
        for r in regs {
            if let Some(out) = items.get_mut(r.track) {
                out.push(Item {
                    tick: Tick(r.tick),
                    mute: r.mute,
                    body: Body::Region(r.plan),
                });
            }
        }
        for a in std::mem::take(&mut self.autos) {
            let Some(t) = a.owner else { continue };
            if let Some(out) = items.get_mut(t) {
                out.push(Item {
                    tick: Tick(a.tick),
                    mute: a.mute,
                    body: Body::Auto {
                        at: a.at,
                        value: a.value,
                    },
                });
            }
        }
        for v in &mut items {
            v.sort_by_key(|i| i.tick);
        }

        let seqs = seqs
            .into_iter()
            .map(|mut s| {
                pair_banks(&mut s.evs);
                let m = main.get(s.track).copied().unwrap_or(0);
                SeqPlan {
                    name: s.name,
                    track: s.track,
                    length: Tick(s.len.max(1)),
                    events: s
                        .evs
                        .into_iter()
                        .map(|e| EventSpec {
                            tick: Tick(e.tick),
                            dest: None,
                            group_channel: gc(m, e.ch),
                            mute: e.mute,
                            body: e.body,
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(Arranged {
            items,
            seqs,
            media: self.media,
        })
    }

    /// Groups the candidates (spec §7.4): clips that play one timeline,
    /// or share a name and have the same content, are one sequence per
    /// destination; a lone named clip goes straight onto its track.
    fn group(
        &mut self,
        plan: &Plan,
        items: &mut [Vec<Item>],
        seqs: &mut Vec<RawSeq>,
    ) -> Result<(), IoError> {
        struct Group {
            first: usize,
            members: Vec<usize>,
        }
        let mut cands = std::mem::take(&mut self.cands);
        let mut groups: Vec<Group> = Vec::new();
        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
        for (ci, c) in cands.iter().enumerate() {
            self.tick()?;
            let h = fingerprint(c);
            let same = |g: &Group| {
                cands
                    .get(g.first)
                    .is_some_and(|f| f.kind == c.kind && f.len == c.len && f.pass == c.pass)
            };
            let found = buckets
                .get(&h)
                .and_then(|v| v.iter().copied().find(|&g| groups.get(g).is_some_and(same)));
            match found.and_then(|g| groups.get_mut(g)) {
                Some(g) => g.members.push(ci),
                None => {
                    buckets.entry(h).or_default().push(groups.len());
                    groups.push(Group {
                        first: ci,
                        members: vec![ci],
                    });
                }
            }
        }
        for g in groups {
            self.tick()?;
            let Some(first) = cands.get(g.first) else {
                continue;
            };
            let shared = matches!(first.kind, Kind::Ref(_))
                || g.members
                    .iter()
                    .any(|&m| cands.get(m).is_some_and(|c| c.looped))
                || (g.members.len() >= 2 && matches!(first.kind, Kind::Name(Some(_))));
            if !shared {
                for &m in &g.members {
                    let Some(c) = cands.get(m) else { continue };
                    if let Some(v) = self.evs.get_mut(c.track) {
                        v.extend(c.pass.iter().map(|e| Ev {
                            tick: e.tick.saturating_add(c.tick),
                            mute: e.mute || c.mute,
                            ..e.clone()
                        }));
                    }
                }
                continue;
            }
            // One sequence per destination: a plug-in track's own, or the
            // shared built-in synth.
            let dest = |t: usize| match plan.tracks.get(t).and_then(|t| t.midi.as_ref()) {
                Some(Midi::Plugin(_)) => Some(t),
                _ => None,
            };
            let mut subs: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
            for &m in &g.members {
                let Some(c) = cands.get(m) else { continue };
                let d = dest(c.track);
                match subs.iter_mut().find(|(k, _)| *k == d) {
                    Some((_, v)) => v.push(m),
                    None => subs.push((d, vec![m])),
                }
            }
            if subs.len() > 1 {
                self.cx.add(ItemKind::Note, SHARED_SPLIT, None);
            }
            for (_, ms) in subs {
                let Some(head) = ms.first().and_then(|&m| cands.get(m)) else {
                    continue;
                };
                let name = head.name.clone().unwrap_or_else(|| {
                    clean_name(&format!("{} clip {}", self.name_of(head.track), head.n)).0
                });
                let (track, len) = (head.track, head.len);
                // Every member's pass is the same: the sequence takes the
                // first's.
                let evs = ms
                    .first()
                    .and_then(|&m| cands.get_mut(m))
                    .map(|c| std::mem::take(&mut c.pass))
                    .unwrap_or_default();
                let seq = seqs.len();
                seqs.push(RawSeq {
                    name,
                    track,
                    len,
                    evs,
                });
                for m in ms {
                    let Some(c) = cands.get(m) else { continue };
                    let (length, loop_count) = if c.looped {
                        looped_use(c.total, c.len)
                    } else {
                        (None, LoopCount::Count(1))
                    };
                    if let Some(v) = items.get_mut(c.track) {
                        v.push(Item {
                            tick: Tick(c.tick),
                            mute: c.mute,
                            body: Body::Use {
                                seq,
                                length,
                                loop_count,
                            },
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Ruling 10's import rule over one track's regions (in time order):
    /// a region that starts inside the one before it, with a fade-in that
    /// fits the overlap (and the earlier fade-out covering it, unless the
    /// fade-in came from a negative `fadeInTime`), crossfades with it.
    fn crossfades(&mut self, regs: &mut [Reg]) {
        // (index, tick, seconds, length in seconds) of the regions still
        // sounding, as `crossfade::overlaps` finds partners.
        let mut sounding: Vec<(usize, i64, f64, f64)> = Vec::new();
        let frames_at = |s: f64, rate: f64| (s * rate).round() as i128;
        for j in 0..regs.len() {
            let Some(b) = regs.get(j) else { continue };
            let (tick, rate, negative) = (b.tick, b.rate, b.negative);
            let (fin, fout, len) = (b.plan.fade_in, b.plan.fade_out, b.plan.length);
            let secs = self.clock.secs(tick);
            let start = frames_at(secs, rate);
            sounding.retain(|&(_, _, s, l)| frames_at(s + l, rate) > start);
            let partner = sounding.iter().rev().find(|p| p.1 < tick).copied();
            let mut made = false;
            if let Some((i, _, s, l)) = partner {
                let overlap = u64::try_from(frames_at(s + l, rate) - start).unwrap_or(0);
                let a_out = regs.get(i).map_or(0, |a| a.plan.fade_out);
                let covered = negative || a_out.saturating_add(1) >= overlap;
                let room = overlap.min(len.saturating_sub(fout));
                if fin >= 1 && fin <= overlap.saturating_add(1) && covered && room >= 1 {
                    made = true;
                    if let Some(b) = regs.get_mut(j) {
                        b.plan.crossfade = true;
                        b.plan.fade_in = fin.min(room);
                    }
                    if a_out.abs_diff(overlap) <= 1
                        && let Some(a) = regs.get_mut(i)
                    {
                        a.plan.fade_out = 0;
                    }
                }
            }
            if negative && !made && partner.is_some() {
                self.cx.add(ItemKind::Changed, CROSSFADES_PLAIN, None);
            }
            sounding.push((j, tick, secs, len as f64 / rate));
        }
    }
}

/// A use of a looped clip's sequence: whole passes as a count, else filled
/// to its length.
fn looped_use(total: i64, len: i64) -> (Option<Tick>, LoopCount) {
    if len > 0 && total % len == 0 {
        if let Some(n) = u32::try_from(total / len)
            .ok()
            .filter(|n| (1..=MAX_LOOPS).contains(n))
        {
            return (None, LoopCount::Count(n));
        }
    }
    (Some(Tick(total)), LoopCount::Fill)
}

/// A hash of what makes candidates the same (equal ones hash the same).
fn fingerprint(c: &Cand<'_>) -> u64 {
    let mut h = DefaultHasher::new();
    c.kind.hash(&mut h);
    c.len.hash(&mut h);
    c.pass.len().hash(&mut h);
    for e in &c.pass {
        (e.tick, e.ch, e.mute).hash(&mut h);
        match &e.body {
            EventBody::Note {
                pitch,
                velocity,
                duration,
                ..
            } => (0u8, *pitch, *velocity, duration.0).hash(&mut h),
            EventBody::Cc { index, value } => (1u8, *index, *value).hash(&mut h),
            EventBody::PitchBend { value } => (2u8, *value).hash(&mut h),
            EventBody::ChannelPressure { value } => (3u8, *value).hash(&mut h),
            EventBody::PolyPressure { pitch, value } => (4u8, *pitch, *value).hash(&mut h),
            EventBody::Program { program, .. } => (5u8, *program).hash(&mut h),
            _ => 6u8.hash(&mut h),
        }
    }
    h.finish()
}

/// Joins each program with the bank select (CC 0 and 32) at its tick on
/// its channel (Ruling 13, inverted); the two controllers are dropped.
fn pair_banks(evs: &mut Vec<Ev>) {
    if !evs
        .iter()
        .any(|e| matches!(e.body, EventBody::Program { bank: None, .. }))
    {
        return;
    }
    evs.sort_by_key(|e| e.tick);
    let seven = |v: u32| (from_u32(v) * 127.0).round().clamp(0.0, 127.0) as u8;
    let mut drop = vec![false; evs.len()];
    let mut from = 0;
    while from < evs.len() {
        let tick = evs.get(from).map_or(0, |e| e.tick);
        let to = from
            + evs
                .get(from..)
                .map_or(0, |r| r.partition_point(|e| e.tick == tick));
        let to = to.max(from + 1);
        for p in from..to {
            let Some(Ev {
                ch,
                body: EventBody::Program { bank: None, .. },
                ..
            }) = evs.get(p)
            else {
                continue;
            };
            let ch = *ch;
            let find = |n: u8, drop: &[bool]| {
                (from..to).find(|&k| {
                    !drop.get(k).copied().unwrap_or(true)
                        && evs.get(k).is_some_and(|e| {
                            e.ch == ch
                                && matches!(e.body, EventBody::Cc { index, .. } if index == n)
                        })
                })
            };
            let (Some(m), Some(l)) = (find(0, &drop), find(32, &drop)) else {
                continue;
            };
            let value = |k: usize| match evs.get(k).map(|e| &e.body) {
                Some(EventBody::Cc { value, .. }) => seven(*value),
                _ => 0,
            };
            let bank = (value(m), value(l));
            for k in [m, l] {
                if let Some(d) = drop.get_mut(k) {
                    *d = true;
                }
            }
            if let Some(Ev {
                body: EventBody::Program { bank: b, .. },
                ..
            }) = evs.get_mut(p)
            {
                *b = Some(bank);
            }
        }
        from = to;
    }
    let mut k = 0;
    evs.retain(|_| {
        let keep = !drop.get(k).copied().unwrap_or(false);
        k += 1;
        keep
    });
}

/// Where an event ends.
fn ev_end(e: &Ev) -> i64 {
    match &e.body {
        EventBody::Note { duration, .. } => e.tick.saturating_add(duration.0.max(0)),
        _ => e.tick.saturating_add(1),
    }
}
