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

mod audio;
mod automation;
mod clock;
mod notes;
mod scenes;
mod share;
mod walk;

use super::*;
use crate::limits::{Limits, MAX_TIMELINE_NESTING};
use clock::Clock;
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
}

/// Where an event ends.
fn ev_end(e: &Ev) -> i64 {
    match &e.body {
        EventBody::Note { duration, .. } => e.tick.saturating_add(duration.0.max(0)),
        _ => e.tick.saturating_add(1),
    }
}
