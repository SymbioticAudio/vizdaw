//! Placing a recording (sub-project 5 spec §6.3–§6.5; plan 5c Rulings
//! 11–17): turns what the engine captured (input files, marks, recorded
//! MIDI) into the [`RecordPass`] the model applies at Stop. Pure: no I/O,
//! no locks, no engine.
//!
//! **Heard runs.** The renderer counts output frames from the roll (run
//! frames) and marks which schedule frame each run plays after the roll,
//! every loop wrap and every resync. The output played at run `x` is heard
//! at run `x + L` (the graph latency), so the timeline position heard at
//! run `u` is `S(u − L)`, where `S` maps runs to schedule frames through
//! the marks. A MIDI message recorded at output run `r` was played against
//! `S(r − L)`; input file frame `k` was captured while `S(k − D − L)` was
//! heard, `D` being the device's input delay minus the latency trim.
//!
//! **Latency per piece (final review I1).** Each mark carries the `L` in
//! effect from it on (a graph swap that changes it marks where it took
//! effect), so each piece is moved into heard runs by its own `L`. Where
//! `L` grew, the heard runs between the pieces played the new graph's
//! empty delay: no audio is placed from them, and a message played then
//! lands where the next piece starts. Where `L` shrank, the newer piece is
//! heard over the tail of every earlier piece that reaches past its start
//! (the one just before, and one before that when a resync landed less
//! than the fall earlier: 5c final review carry, plan 5c-2 Ruling 13),
//! and cuts each there.
//!
//! **Synth returns (plan 5c-2 Ruling 4).** A lane recording an external
//! synth's return (spec §6.7) captured the synth's answer `R` frames (its
//! return latency) after the note that made it was heard: its file frames
//! are placed with `D + R` ([`AudioTake::return_frames`]), so its regions
//! land earlier by `R`. MIDI does not move.
//!
//! **Passes.** The roll and every wrap start a pass. A pass's window is the
//! timeline it played, inside the record range. Discarded passes are
//! dropped; every other pass with something in its window is kept (plan
//! 5c Ruling 13), except a short last pass that would become a take
//! (below).
//!
//! **Cuts and joins (pre-flight M1).** A non-wrap mark whose frame is
//! more than a frame below where the piece before it had got to (the
//! transport stood still through an xrun, or it resumed after a stop from
//! further back) cuts that piece where the timeline reached the mark's
//! frame (or entirely, when the mark is before the piece's start): the
//! newer piece replaces it, so a piece never lies under the one after it.
//! A stop and the resume after it are marks on one run, judged on the
//! resume's frame. Pieces that follow each other in the file and within
//! one frame on the timeline (a schedule swap moves the frame by ±1) stay
//! one region. Only consecutive pieces are compared: this assumes the
//! transport cannot jump while recording (plan 5c Ruling 17 refuses Locate
//! and Play then); allowing it would need a full newest-wins pass, or a
//! jump back would drop the earlier piece's tail without notice.
//!
//! **Region starts.** A region starts on a tick, and its file offset
//! follows the frame that tick plays at. The tick is the one nearest the
//! start frame (`TempoTable::frame_to_tick` rounds to the nearest), then
//! stepped up a tick at a time while it would start the region before the
//! punch-in or under the region before it.
//!
//! **Ruling (pre-flight M3, adopted for plan 5c Task 7; narrowed by the
//! Task 7 review I1 and re-review): a short last pass after a wrap is no
//! take.** When loop recording makes takes of passes, the final pass (the
//! one playing at Stop) that played less than one beat (one quarter note,
//! [`PPQ`] ticks) of timeline is dropped, per lane, unless it holds that
//! lane's only content: it is kept when the lane recorded something in it
//! and in no earlier pass that is not discarded. For MIDI only notes count
//! as content: a controller, pressure or bend alone (a sustain pedal
//! resting through a count-in) neither stands in nor keeps a sliver.
//! Stopping just after a
//! wrap therefore leaves no sliver of a take, and the sliver does not
//! become the lane's active take, while a MIDI lane silent until the wrap
//! keeps the note played just after it. The rule never drops pass 0 or
//! an only pass that recorded something, and never applies when MIDI passes merge into the active
//! take (`passes_as_takes` off: no take is made, so the notes stay; audio
//! still makes takes when looped). The playing pass's fate is never
//! decided by `decide`, so only a caller-set `PassFate::Kept` exempts it.
//! Earlier passes are never dropped by length.
//!
//! Control-side code: nothing here runs on the real-time thread.

use core_engine::midi1::Midi1Msg;
use core_engine::record::{Mark, RecordedMidi};
use core_engine::schedule::TempoTable;
use core_model::command::{LaneRecord, RecordPass, RecordedContent, RecordedMedia, RecordedRegion};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{LaneId, SeqId};
use core_model::time::{PPQ, Tick};
use std::collections::BTreeMap;

/// The record range in schedule frames (spec §6.4): the punch range, the
/// loop range (narrowed by the punch), or from where recording started on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordRange {
    /// First frame.
    pub start: u64,
    /// One past the last frame; `None`: until Stop.
    pub end: Option<u64>,
}

impl RecordRange {
    fn clip(&self, a: i64, b: i64) -> Option<(i64, i64)> {
        let lo = a.max(i64::try_from(self.start).unwrap_or(i64::MAX));
        let hi = match self.end {
            Some(e) => b.min(i64::try_from(e).unwrap_or(i64::MAX)),
            None => b,
        };
        (lo < hi).then_some((lo, hi))
    }
}

/// What the user decided about a pass (`record.keep_pass`,
/// `record.discard_pass`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PassFate {
    /// Nothing: it is kept at Stop.
    #[default]
    Undecided,
    /// Kept: a later discard leaves it.
    Kept,
    /// Dropped.
    Discarded,
}

/// Applies a keep (`keep`) or a discard to the passes that completed:
/// a keep keeps every undecided one, a discard drops every undecided one
/// (those since the last keep). The pass still playing is not touched
/// (plan 5c Ruling 13).
pub fn decide(fates: &mut Vec<PassFate>, completed: usize, keep: bool) {
    if fates.len() < completed {
        fates.resize(completed, PassFate::Undecided);
    }
    for f in fates.iter_mut().take(completed) {
        if *f == PassFate::Undecided {
            *f = if keep {
                PassFate::Kept
            } else {
                PassFate::Discarded
            };
        }
    }
}

/// One armed audio lane's finished file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTake {
    /// The lane.
    pub lane: LaneId,
    /// The file (package-relative path, format, frames, hash, name, pass).
    pub media: RecordedMedia,
    /// Frames this lane's input arrived after what it records was heard,
    /// besides the device's delay: an external synth's return latency for
    /// a `return_of` lane (spec §6.7), else 0. Its regions move earlier by
    /// that much.
    pub return_frames: i64,
}

/// Everything a finished recording captured.
#[derive(Debug, Clone, PartialEq)]
pub struct Captured<'a> {
    /// The document as it is at Stop.
    pub doc: &'a Document,
    /// The sequence recorded into.
    pub seq: SeqId,
    /// The marks, oldest first, each with the graph latency `L` from it
    /// on.
    pub marks: &'a [Mark],
    /// Output run frames played until the tap closed.
    pub end_run: u64,
    /// The device's input delay minus the latency trim (frames).
    pub input_delay: i64,
    /// The record range.
    pub range: RecordRange,
    /// Loop recording: every wrap ends a pass.
    pub looped: bool,
    /// Replace (else overdub).
    pub replace: bool,
    /// Kept passes become takes (spec §6.4).
    pub passes_as_takes: bool,
    /// Each pass's fate, by pass number (missing: undecided).
    pub fates: &'a [PassFate],
    /// The armed audio lanes' files.
    pub audio: &'a [AudioTake],
    /// The armed MIDI lanes.
    pub midi_lanes: &'a [LaneId],
    /// What they recorded.
    pub midi: &'a [RecordedMidi],
    /// Input was lost somewhere (silence was written).
    pub with_gaps: bool,
}

/// A stretch of heard runs `[run, end)` that played schedule frames from
/// `frame` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    pass: usize,
    /// The heard run of the mark that started it (`run` reaches back
    /// further for the first piece).
    from: i64,
    run: i64,
    end: i64,
    frame: i64,
    /// Where a message is placed in this piece from: before `run` only
    /// after a latency rise, whose gap is this piece's (at its start).
    catch: i64,
}

impl Piece {
    fn at(&self, u: i64) -> i64 {
        self.frame + (u - self.run)
    }
}

/// The pieces the marks describe in heard runs, numbered by pass: marks
/// on one run keep the last frame and latency (and a wrap); the first
/// piece reaches back before the roll (pre-roll handles); the last ends
/// at output run `end_run`, as heard. Each piece is moved by its own
/// latency (module docs).
fn pieces(marks: &[Mark], end_run: i64) -> Vec<Piece> {
    let mut m: Vec<Mark> = Vec::with_capacity(marks.len());
    for x in marks {
        match m.last_mut() {
            Some(last) if last.run == x.run => {
                last.frame = x.frame;
                last.wrap |= x.wrap;
                last.latency = x.latency;
            }
            _ => m.push(*x),
        }
    }
    let lat = |x: &Mark| i64::try_from(x.latency).unwrap_or(0);
    // In output runs, the last piece ends where its output was heard by
    // the end.
    let end = end_run - m.last().map_or(0, lat);
    let mut out: Vec<Piece> = Vec::with_capacity(m.len());
    let mut pass = 0usize;
    for (i, x) in m.iter().enumerate() {
        if i > 0 && x.wrap {
            pass += 1;
        }
        let at = i64::try_from(x.run).unwrap_or(i64::MAX);
        if !x.wrap
            && let Some(prev) = out.last_mut()
        {
            // Pre-flight M1: cut where the timeline reached the mark's
            // frame when the mark is more than a frame behind (a ±1
            // schedule swap joins instead); a mark before the piece's
            // start replaces all of it.
            let cut = i64::try_from(x.frame).unwrap_or(i64::MAX);
            if cut < prev.at(at) - 1 {
                let to = prev.run + (cut - prev.frame);
                prev.end = prev.end.min(to.max(prev.from));
            }
        }
        let run = if i == 0 { i64::MIN / 4 } else { at };
        let next = m
            .get(i + 1)
            .map_or(end.max(at), |n| i64::try_from(n.run).unwrap_or(i64::MAX));
        let frame = i64::try_from(x.frame).unwrap_or(i64::MAX);
        out.push(Piece {
            pass,
            from: at,
            run,
            end: next,
            // The frame at `run`: a piece reaching back keeps its slope.
            frame: frame - (at - run),
            catch: run,
        });
    }
    // Into heard runs, each by its own latency (final review I1).
    let lats: Vec<i64> = m.iter().map(lat).collect();
    for i in 0..out.len() {
        let l = lats.get(i).copied().unwrap_or(0);
        let (done, rest) = out.split_at_mut(i);
        let Some(p) = rest.first_mut() else {
            break;
        };
        p.from += l;
        p.run += l;
        p.end += l;
        p.catch += l;
        let Some(ql) = i.checked_sub(1).and_then(|j| lats.get(j)).copied() else {
            continue;
        };
        // The latency shrank: the newer piece is heard over the tail of
        // every earlier piece reaching past its start, not only the one
        // just before (plan 5c-2 Ruling 13); each is cut there.
        let mut fell = false;
        for q in done.iter_mut() {
            if q.end > p.from {
                q.end = p.from.max(q.from);
                fell = true;
            }
        }
        let rise = l - ql;
        if !fell
            && rise > 0
            && let Some(q) = done.last()
        {
            // It grew: what is played in the gap it opened lands at this
            // piece's start (a cut before it stays a cut).
            p.catch = (p.from - rise).max(q.end);
        }
    }
    out
}

/// The piece a message heard at run `u` was played in.
fn piece_of(pieces: &[Piece], u: i64) -> Option<&Piece> {
    pieces.iter().find(|p| u >= p.catch && u < p.end)
}

/// Where in piece `p` a message heard at run `u` lands.
fn place(p: &Piece, u: i64) -> i64 {
    p.at(u.max(p.run))
}

/// The timeline `[t0, t1)` piece `p` played, inside the range.
fn window(p: &Piece, range: &RecordRange) -> Option<(i64, i64)> {
    range.clip(p.at(p.run), p.at(p.end))
}

/// MIDI 2.0 min-center-max upscale of a `bits`-bit value to 32 bits.
fn up32(v: u32, bits: u32) -> u32 {
    let scale = 32 - bits;
    let shifted = v << scale;
    let center = 1u32 << (bits - 1);
    if v <= center {
        return shifted;
    }
    let repeat_bits = bits - 1;
    let mask = (1u32 << repeat_bits) - 1;
    let mut repeat = v & mask;
    repeat = if scale > repeat_bits {
        repeat << (scale - repeat_bits)
    } else {
        repeat >> (repeat_bits - scale)
    };
    let mut out = shifted;
    while repeat != 0 {
        out |= repeat;
        repeat >>= repeat_bits;
    }
    out
}

/// The event a non-note channel message records as.
fn body_of(msg: Midi1Msg) -> Option<EventBody> {
    let [s, a, b] = msg.bytes;
    Some(match s & 0xF0 {
        0xA0 => EventBody::PolyPressure {
            pitch: a & 0x7F,
            value: up32(u32::from(b & 0x7F), 7),
        },
        0xB0 => EventBody::Cc {
            index: a & 0x7F,
            value: up32(u32::from(b & 0x7F), 7),
        },
        0xC0 => EventBody::Program {
            program: a & 0x7F,
            bank: None,
        },
        0xD0 => EventBody::ChannelPressure {
            value: up32(u32::from(a & 0x7F), 7),
        },
        0xE0 => EventBody::PitchBend {
            value: up32(u32::from(a & 0x7F) | (u32::from(b & 0x7F) << 7), 14),
        },
        _ => return None,
    })
}

/// One recorded message in heard runs.
#[derive(Debug, Clone, Copy)]
struct Heard {
    u: i64,
    msg: Midi1Msg,
}

/// A lane's MIDI per pass: events placed on the timeline, notes paired
/// with their offs (cut at the pass window's end).
fn midi_events(
    heard: &[Heard],
    pieces: &[Piece],
    range: &RecordRange,
    tempo: &TempoTable,
) -> BTreeMap<usize, Vec<EventSpec>> {
    let mut out: BTreeMap<usize, Vec<EventSpec>> = BTreeMap::new();
    let tick = |f: i64| tempo.frame_to_tick(u64::try_from(f.max(0)).unwrap_or(0));
    for (i, h) in heard.iter().enumerate() {
        let Some(p) = piece_of(pieces, h.u) else {
            continue;
        };
        let Some((t0, t1)) = window(p, range) else {
            continue;
        };
        let t = place(p, h.u);
        if t < t0 || t >= t1 {
            continue;
        }
        let [s, pitch, vel] = h.msg.bytes;
        let body = match s & 0xF0 {
            0x90 if vel > 0 => {
                let ch = s & 0x0F;
                // The off: the next note-off of this pitch and channel in
                // the same pass; else the window's end.
                let off = heard.iter().skip(i + 1).find(|o| {
                    let [os, op, ov] = o.msg.bytes;
                    os & 0x0F == ch
                        && op == pitch
                        && (os & 0xF0 == 0x80 || (os & 0xF0 == 0x90 && ov == 0))
                });
                let (end, release) = match off.and_then(|o| piece_of(pieces, o.u).map(|q| (o, q))) {
                    Some((o, q)) if q.pass == p.pass => (
                        place(q, o.u).min(t1),
                        crate::input::upscale_velocity(o.msg.bytes[2]),
                    ),
                    _ => (t1, 0),
                };
                let mut b = EventBody::note(
                    pitch & 0x7F,
                    crate::input::upscale_velocity(vel),
                    Tick((tick(end).0 - tick(t).0).max(0)),
                );
                if let EventBody::Note {
                    release_velocity, ..
                } = &mut b
                {
                    *release_velocity = release;
                }
                b
            }
            0x80 | 0x90 => continue,
            _ => match body_of(h.msg) {
                Some(b) => b,
                None => continue,
            },
        };
        out.entry(p.pass)
            .or_default()
            .push(EventSpec::new(tick(t), body));
    }
    out
}

/// A lane's audio regions per pass: one per piece, joined when pieces
/// continue each other, cut to the file and the window.
fn audio_regions(
    frames: u64,
    pieces: &[Piece],
    range: &RecordRange,
    d: i64,
    tempo: &TempoTable,
) -> BTreeMap<usize, Vec<RecordedRegion>> {
    let mut out: BTreeMap<usize, Vec<RecordedRegion>> = BTreeMap::new();
    let len = i64::try_from(frames).unwrap_or(i64::MAX);
    let mut last: Option<(usize, i64, i64)> = None; // (pass, timeline end, file end)
    for p in pieces {
        let Some((t0, t1)) = window(p, range) else {
            continue;
        };
        // Heard run → file frame: k = u + D.
        let k_at = |t: i64| (t - p.frame) + p.run + d;
        let (mut t0, mut k0) = (t0, k_at(t0));
        if k0 < 0 {
            t0 -= k0;
            k0 = 0;
        }
        let t1 = t1.min(t0 + (len - k0));
        if t1 <= t0 {
            continue;
        }
        let regions = out.entry(p.pass).or_default();
        let joined = match (last, regions.last_mut()) {
            // Pre-flight M1: within a frame on the timeline, continuous in
            // the file.
            (Some((pass, te, ke)), Some(r))
                if pass == p.pass && (te - t0).abs() <= 1 && ke == k0 =>
            {
                r.length += u64::try_from(t1 - t0).unwrap_or(0);
                true
            }
            _ => false,
        };
        if !joined {
            // The region starts on a tick: the file offset follows the
            // frame that tick plays at, so the audio lands exactly.
            let mut tick = tempo.frame_to_tick(u64::try_from(t0).unwrap_or(0));
            let mut f = i64::try_from(tempo.tick_to_frame(tick)).unwrap_or(t0);
            // The nearest tick may lie before `t0`: step it up while it
            // would start the region before the punch-in (review m3) or,
            // after a cut, under the region before it.
            let mut lo = i64::try_from(range.start).unwrap_or(i64::MAX).min(t0);
            if let Some((pass, te, _)) = last
                && pass == p.pass
                && te <= t0
            {
                lo = lo.max(te);
            }
            while f < lo && f < t1 {
                tick = Tick(tick.0 + 1);
                f = i64::try_from(tempo.tick_to_frame(tick)).unwrap_or(t1);
            }
            let (start, k) = if k0 + (f - t0) >= 0 {
                (f, k0 + (f - t0))
            } else {
                (t0, k0)
            };
            let length = t1 - start;
            if length <= 0 {
                continue;
            }
            regions.push(RecordedRegion {
                media: 0,
                tick,
                source_offset: u64::try_from(k).unwrap_or(0),
                length: u64::try_from(length).unwrap_or(0),
            });
        }
        last = Some((p.pass, t1, k0 + (t1 - t0)));
    }
    out
}

/// The pass to apply, or `None` when nothing was recorded.
pub fn plan(c: &Captured<'_>) -> Result<Option<RecordPass>, String> {
    let seq = c.doc.sequence(c.seq).map_err(|e| e.to_string())?;
    let rate = f64::from(c.doc.settings().format.sample_rate);
    let tempo = TempoTable::build(&seq.tempo, Tick(-seq.start_offset.0), rate);
    let pieces = pieces(c.marks, i64::try_from(c.end_run).unwrap_or(i64::MAX));
    if pieces.is_empty() {
        return Ok(None);
    }
    let d = c.input_delay;
    let fate = |pass: usize| c.fates.get(pass).copied().unwrap_or_default();
    let passes = pieces.last().map_or(0, |p| p.pass + 1);
    let tick_at = |f: i64| tempo.frame_to_tick(u64::try_from(f.max(0)).unwrap_or(0));
    let kept: Vec<usize> = (0..passes)
        .filter(|p| fate(*p) != PassFate::Discarded)
        .filter(|p| {
            pieces
                .iter()
                .any(|x| x.pass == *p && window(x, &c.range).is_some())
        })
        .collect();
    // The Ruling (pre-flight M3 as narrowed by review I1, module docs): a
    // final loop pass under a beat is no take when an earlier pass of the
    // same lane stands in for it.
    let last = passes.saturating_sub(1);
    let short_last =
        c.looped && last > 0 && kept.last() == Some(&last) && fate(last) != PassFate::Kept && {
            let played: i64 = pieces
                .iter()
                .filter(|x| x.pass == last)
                .map(|x| (tick_at(x.at(x.end.max(x.from))).0 - tick_at(x.at(x.from)).0).max(0))
                .sum();
            played < PPQ
        };
    // The passes that become a lane's takes, `has(p)` saying whether the
    // lane recorded something in pass `p`: the short last pass is kept only
    // when it holds the lane's only content (re-review I1).
    let as_takes = |has: &dyn Fn(usize) -> bool| -> Vec<usize> {
        let drop = short_last && (!has(last) || kept.iter().any(|p| *p != last && has(*p)));
        kept.iter()
            .copied()
            .filter(|p| !(drop && *p == last))
            .collect()
    };
    let windows: Vec<(Tick, Tick)> = pieces
        .iter()
        .filter(|p| kept.contains(&p.pass))
        .filter_map(|p| window(p, &c.range))
        .map(|(a, b)| (tick_at(a), tick_at(b)))
        .filter(|(a, b)| a < b)
        .collect();
    let takes_from_passes = c.looped && c.passes_as_takes;
    let mut media = Vec::new();
    let mut lanes: BTreeMap<LaneId, LaneRecord> = BTreeMap::new();
    let lane_record = |lane: LaneId| LaneRecord {
        seq: c.seq,
        lane,
        clear: Vec::new(),
        active: RecordedContent::default(),
        takes: Vec::new(),
    };
    let take_name = |lane: LaneId, k: usize| {
        let n = seq.lane(lane).map_or(1, |l| l.takes.len()) + 1 + k;
        format!("Take {n}")
    };
    // Audio makes a take of every pass when looped (spec §6.4).
    for a in c.audio {
        // A synth's return lands earlier by its return latency (plan 5c-2
        // Ruling 4).
        let mut regions = audio_regions(
            a.media.length_frames,
            &pieces,
            &c.range,
            d.saturating_add(a.return_frames),
            &tempo,
        );
        let takes = if c.looped {
            as_takes(&|p| regions.get(&p).is_some_and(|r| !r.is_empty()))
        } else {
            kept.clone()
        };
        regions.retain(|p, _| takes.contains(p));
        if regions.is_empty() {
            continue;
        }
        let index = media.len();
        media.push(a.media.clone());
        let rec = lanes.entry(a.lane).or_insert_with(|| lane_record(a.lane));
        if c.looped {
            for (k, p) in takes.iter().enumerate() {
                let mut content = RecordedContent::default();
                for r in regions.get(p).into_iter().flatten() {
                    content.regions.push(RecordedRegion { media: index, ..*r });
                }
                rec.takes.push((take_name(a.lane, k), content));
            }
        } else {
            for r in regions.values().flatten() {
                rec.active
                    .regions
                    .push(RecordedRegion { media: index, ..*r });
            }
        }
    }
    for lane in c.midi_lanes {
        let heard: Vec<Heard> = c
            .midi
            .iter()
            .filter(|m| m.lane == *lane)
            .map(|m| Heard {
                u: m.run,
                msg: m.msg,
            })
            .collect();
        let mut events = midi_events(&heard, &pieces, &c.range, &tempo);
        events.retain(|p, _| kept.contains(p));
        if takes_from_passes {
            // Only notes count as content (re-review 2 m5): a controller
            // alone (a sustain pedal resting through a count-in) is none.
            let takes = as_takes(&|p| {
                events
                    .get(&p)
                    .is_some_and(|e| e.iter().any(|x| matches!(x.body, EventBody::Note { .. })))
            });
            if takes.is_empty() {
                continue;
            }
            let rec = lanes.entry(*lane).or_insert_with(|| lane_record(*lane));
            for (k, p) in takes.iter().enumerate() {
                let content = RecordedContent {
                    events: events.remove(p).unwrap_or_default(),
                    regions: Vec::new(),
                };
                rec.takes.push((take_name(*lane, k), content));
            }
        } else {
            let all: Vec<EventSpec> = events.into_values().flatten().collect();
            if all.is_empty() && !c.replace {
                continue;
            }
            let rec = lanes.entry(*lane).or_insert_with(|| lane_record(*lane));
            if c.replace {
                rec.clear = windows.clone();
            }
            rec.active.events = all;
        }
    }
    let lanes: Vec<LaneRecord> = lanes
        .into_values()
        .filter(|l| !l.clear.is_empty() || !l.active.is_empty() || !l.takes.is_empty())
        .collect();
    if lanes.is_empty() {
        return Ok(None);
    }
    Ok(Some(RecordPass {
        with_gaps: c.with_gaps,
        media,
        lanes,
    }))
}
