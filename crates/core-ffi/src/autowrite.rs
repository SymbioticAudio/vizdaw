//! Placing recorded automation (sub-project 5 spec §7.3, §7.4; plan 5e-1
//! Rulings 7, 9–13): turns one pass's gesture stamps and marks into
//! automation writes. Pure: no I/O, no locks, no engine; control-side code.
//!
//! **Placement.** A stamp is placed as recorded MIDI is (Ruling 7): its
//! run gives the piece (and so the pass) it was heard in, and the piece the
//! schedule frame. The gesture tap already moved each stamp to the frame
//! its control's lane reads (plan 5e-1 Task 6), so nothing here adds an
//! arrival or a latency of its own. A stamp the first piece places before
//! its pass's start (a strip behind latency, in the first frames after the
//! roll: Task 6 review m2) is clipped to that start, so no frame is
//! negative or before its pass.
//!
//! **Pieces and jumps.** Stamps are assigned to pieces by their own run,
//! not by the mark that governed the block that took them: a stamp just
//! after a resync whose run still lies before the resync's mark (its strip
//! was still rendering the old piece: Task 6 review m4) lands in the piece
//! before, at a frame inside it, and the segment walk takes it before the
//! jump breaks the segment. A target's stamps are walked in piece order and
//! by frame within a piece (the order of their runs), so no segment's
//! frames go back.

use crate::passes::{Piece, pass_window, piece_of, place, writing_pieces};
use core_engine::gesture::{GesturePhase, Stamped};
use core_engine::record::Mark;
use core_engine::schedule::TempoTable;
use core_model::automation::{AutomationMode, AutomationTarget, fader_db, fader_norm, unit};
use core_model::command::{AutomationWrite, MAX_WRITE_POINTS};
use core_model::ids::SeqId;
use core_model::time::Tick;

/// The writing mode a gesture was made in, carried as the stamp's tag (0:
/// not writing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// From touch to release, then a glide back over the return time.
    Touch,
    /// From touch to the pass's end, holding the last value.
    Latch,
    /// Every held control from the roll; touches held to the pass's end.
    Write,
    /// An offset added to the existing automation.
    Trim,
}

impl WriteMode {
    /// The stamp tag: 1, 2, 3, 4.
    pub fn tag(self) -> u8 {
        match self {
            WriteMode::Touch => 1,
            WriteMode::Latch => 2,
            WriteMode::Write => 3,
            WriteMode::Trim => 4,
        }
    }

    /// The mode of a stamp tag; 0 and others: `None`.
    pub fn of_tag(tag: u8) -> Option<WriteMode> {
        match tag {
            1 => Some(WriteMode::Touch),
            2 => Some(WriteMode::Latch),
            3 => Some(WriteMode::Write),
            4 => Some(WriteMode::Trim),
            _ => None,
        }
    }

    /// The writing mode of a strip mode; Off and Read: `None`.
    pub fn of(mode: AutomationMode) -> Option<WriteMode> {
        match mode {
            AutomationMode::Off | AutomationMode::Read => None,
            AutomationMode::Touch => Some(WriteMode::Touch),
            AutomationMode::Latch => Some(WriteMode::Latch),
            AutomationMode::Write => Some(WriteMode::Write),
            AutomationMode::Trim => Some(WriteMode::Trim),
        }
    }
}

/// How closely written automation follows the gesture (Ruling 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Thinning {
    /// The finest.
    Low,
    /// The spec's tolerances.
    #[default]
    Medium,
    /// The coarsest.
    High,
}

impl Thinning {
    /// Ruling 12: 0.04/0.1/0.2 dB for fader and send targets,
    /// 0.001/0.0025/0.005 otherwise.
    pub fn tolerance(self, target: AutomationTarget) -> f64 {
        match (self, is_db(target)) {
            (Thinning::Low, true) => 0.04,
            (Thinning::Medium, true) => 0.1,
            (Thinning::High, true) => 0.2,
            (Thinning::Low, false) => 0.001,
            (Thinning::Medium, false) => 0.0025,
            (Thinning::High, false) => 0.005,
        }
    }
}

/// A level in dB (a fader or a send): thinned in dB, Trim offsets it in
/// dB (Rulings 11, 12).
fn is_db(target: AutomationTarget) -> bool {
    matches!(
        target,
        AutomationTarget::StripFader(_) | AutomationTarget::SendLevel(..)
    )
}

/// What one pass captured, and what the writer reads of the document.
pub struct AutoCapture<'a> {
    /// The pass's marks, oldest first (the gesture tap's).
    pub marks: &'a [Mark],
    /// The gesture tap's run when the pass ended (or now, for a preview).
    pub end_run: u64,
    /// The gesture tap's stamps, in the order it took them.
    pub stamps: &'a [Stamped],
    /// Write strips' controls held from the roll, with the value each had
    /// then (Ruling 9).
    pub holds: &'a [(AutomationTarget, f64)],
    /// The pass's thinning.
    pub thinning: Thinning,
    /// The After Write mode as a writing mode (None for Off and Read):
    /// Write-tagged stamps in a pass after the first are written in it, or
    /// not at all (pre-flight M3).
    pub after_write: Option<WriteMode>,
    /// Whether a target is stepped (keeps only its changes).
    pub stepped: &'a dyn Fn(AutomationTarget) -> bool,
    /// The target's existing automation at a schedule frame (None: it has
    /// none).
    pub existing: &'a dyn Fn(AutomationTarget, u64) -> Option<f64>,
    /// The frames of its existing points in `[from, to]`.
    pub existing_points: &'a dyn Fn(AutomationTarget, u64, u64) -> Vec<u64>,
    /// Its static value, normalised.
    pub fixed: &'a dyn Fn(AutomationTarget) -> f64,
}

/// One write of one target in one pass: thinned points in schedule frames,
/// no edges yet.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetWrite {
    /// What it drives.
    pub target: AutomationTarget,
    /// The pass it was made in.
    pub pass: usize,
    /// `(schedule frame, normalised value)`, by frame.
    pub points: Vec<(u64, f64)>,
}

/// What a segment's samples are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Only its touch so far. A touch with nothing after it is written
    /// as an absolute value, also in Trim: a Trim grab on an automated
    /// target is sent as `Offset(0.0)` (Ruling 11), so only an
    /// unautomated Trim target, which writes absolute values, gets here
    /// (review m4).
    Untyped,
    /// Values.
    Absolute,
    /// Trim offsets.
    Offset,
}

/// One stretch of one target's writing.
#[derive(Debug, Clone)]
struct Segment {
    pass: usize,
    /// The live piece it starts in.
    piece: usize,
    mode: WriteMode,
    kind: Kind,
    samples: Vec<(i64, f64)>,
    held: bool,
    /// Reopened at a wrap or a jump, with no stamp of its own yet.
    reopened: bool,
}

impl Segment {
    // A segment always holds at least the sample it was opened with, so
    // the defaults of `last` and `first_frame` are never used (review m7).
    fn last(&self) -> f64 {
        self.samples.last().map_or(0.0, |s| s.1)
    }

    fn first_frame(&self) -> i64 {
        self.samples.first().map_or(0, |s| s.0)
    }

    /// Appends a sample; one at the last sample's frame (or, never in
    /// practice, before it) replaces that one's value, so frames rise.
    fn push(&mut self, frame: i64, value: f64) {
        match self.samples.last_mut() {
            Some(l) if frame <= l.0 => l.1 = value,
            _ => self.samples.push((frame, value)),
        }
    }

    /// Ends it at `frame` with its last value.
    fn end_at(&mut self, frame: i64) {
        let v = self.last();
        match self.samples.last() {
            Some(l) if frame < l.0 => {}
            _ => self.push(frame, v),
        }
    }
}

/// One live piece's place in the walk.
#[derive(Debug, Clone, Copy)]
struct Span {
    pass: usize,
    /// The frame it starts at.
    start: i64,
    /// The frame it ends at.
    end: i64,
    /// A wrap starts it (its pass differs from the piece before's).
    wrap: bool,
    /// A non-wrap mark that does not continue the piece before starts it
    /// (Ruling 23).
    jump: bool,
    /// Where the contiguous stretch it is part of ends (the pass's end
    /// unless a jump comes first).
    stretch_end: i64,
}

/// The live pieces' spans, in order.
fn spans(live: &[&Piece]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(live.len());
    for p in live {
        let start = place(p, p.from);
        let end = place(p, p.end);
        let (wrap, jump) = match out.last() {
            Some(q) if q.pass != p.pass => (true, false),
            Some(q) => (false, (start - q.end).abs() > 1),
            None => (false, false),
        };
        out.push(Span {
            pass: p.pass,
            start,
            end,
            wrap,
            jump,
            stretch_end: end,
        });
    }
    // Each stretch ends where its last piece does.
    let mut tail: Option<i64> = None;
    for s in out.iter_mut().rev() {
        s.stretch_end = *tail.get_or_insert(s.end);
        if s.wrap || s.jump {
            tail = None;
        }
    }
    out
}

/// One stamp, placed.
#[derive(Debug, Clone, Copy)]
struct Placed {
    piece: usize,
    frame: i64,
    phase: GesturePhase,
    mode: WriteMode,
}

/// A phase's values as the live table plays them (review I1;
/// `LiveTable::apply`): values clamped to 0–1 (NaN is 0), a non-finite
/// offset 0.
fn sanitised(phase: GesturePhase) -> GesturePhase {
    match phase {
        GesturePhase::Touch(v) => GesturePhase::Touch(unit(v)),
        GesturePhase::Value(v) => GesturePhase::Value(unit(v)),
        GesturePhase::Offset(d) if !d.is_finite() => GesturePhase::Offset(0.0),
        other => other,
    }
}

/// A target's segments, walking its placed stamps (sorted by piece, then
/// frame) through the live pieces (algorithm step 2).
fn segments(
    c: &AutoCapture<'_>,
    target: AutomationTarget,
    stamps: &[Placed],
    spans: &[Span],
    pieces: &[Piece],
) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut open: Option<Segment> = None;
    let mut at = 0usize;
    // Crosses the piece boundaries up to `to`: a wrap or a jump ends the
    // open segment at the earlier piece's end and, unless it was released,
    // reopens it at the new piece's start with its last value.
    let cross = |open: &mut Option<Segment>, out: &mut Vec<Segment>, from: usize, to: usize| {
        for j in from.saturating_add(1)..=to {
            let (Some(prev), Some(next)) = (spans.get(j - 1), spans.get(j)) else {
                continue;
            };
            if !(next.wrap || next.jump) {
                continue;
            }
            let Some(mut seg) = open.take() else {
                continue;
            };
            seg.end_at(prev.end);
            let v = seg.last();
            let (held, kind, mode) = (seg.held, seg.kind, seg.mode);
            out.push(seg);
            if held {
                continue;
            }
            // From the first wrap on, Write is the After Write mode
            // (Ruling 9).
            let mode = if next.wrap && mode == WriteMode::Write && next.pass > 0 {
                c.after_write
            } else {
                Some(mode)
            };
            if let Some(mode) = mode {
                *open = Some(Segment {
                    pass: next.pass,
                    piece: j,
                    mode,
                    kind,
                    samples: vec![(next.start, v)],
                    held: false,
                    reopened: true,
                });
            }
        }
    };
    let open_seg = |s: &Placed, kind: Kind, value: f64| Segment {
        pass: spans.get(s.piece).map_or(0, |x| x.pass),
        piece: s.piece,
        mode: s.mode,
        kind,
        samples: vec![(s.frame, value)],
        held: false,
        reopened: false,
    };
    for s in stamps {
        cross(&mut open, &mut out, at, s.piece);
        at = at.max(s.piece);
        let f = s.frame;
        if let (
            Some(seg),
            GesturePhase::Touch(_) | GesturePhase::Value(_) | GesturePhase::Offset(_),
        ) = (open.as_mut(), s.phase)
        {
            seg.reopened = false;
        }
        match s.phase {
            GesturePhase::Touch(v) => match open.as_mut() {
                Some(seg) => {
                    seg.held = false;
                    seg.mode = s.mode;
                    if seg.kind != Kind::Offset {
                        seg.push(f, v);
                    }
                }
                None => open = Some(open_seg(s, Kind::Untyped, v)),
            },
            GesturePhase::Value(v) => match open.as_mut() {
                Some(seg) => {
                    seg.held = false;
                    if seg.kind == Kind::Untyped {
                        seg.kind = Kind::Absolute;
                    }
                    seg.push(f, v);
                }
                // A value with no touch is a touch first (Ruling 20).
                None => open = Some(open_seg(s, Kind::Absolute, v)),
            },
            GesturePhase::Offset(d) => {
                let seg = open.get_or_insert_with(|| open_seg(s, Kind::Offset, 0.0));
                seg.held = false;
                if seg.kind == Kind::Untyped {
                    // Its first non-touch stamp is an offset: the touch
                    // is offset 0.
                    seg.kind = Kind::Offset;
                    for x in seg.samples.iter_mut() {
                        x.1 = 0.0;
                    }
                }
                seg.push(f, d);
            }
            GesturePhase::Release => {
                if let Some(mut seg) = open.take() {
                    // Released where it reopened (an end stamp a gap moved
                    // to the next piece's start): nothing to write there.
                    let empty = seg.reopened && seg.samples.iter().all(|x| x.0 == f);
                    if !empty {
                        seg.end_at(f);
                        out.push(seg);
                    }
                }
            }
            GesturePhase::Glide(n) => {
                if let Some(mut seg) = open.take() {
                    // Like a Release where its segment reopened: the
                    // engine had begun the glide before, so nothing is
                    // written there (re-review 2 nit).
                    if seg.reopened && seg.samples.iter().all(|x| x.0 == f) {
                        continue;
                    }
                    seg.end_at(f);
                    let stretch_end = spans.get(s.piece).map_or(f, |x| x.stretch_end);
                    // A glide of 0 frames is a step one frame after the
                    // release (review I2): the engine holds the value up
                    // to the release, so `(f, last)` must survive.
                    let to = f.saturating_add(i64::from(n.max(1))).min(stretch_end);
                    if to > f {
                        let v = if seg.kind == Kind::Offset {
                            0.0
                        } else {
                            let frame = u64::try_from(to).unwrap_or(0);
                            unit((c.existing)(target, frame).unwrap_or_else(|| (c.fixed)(target)))
                        };
                        seg.push(to, v);
                    }
                    out.push(seg);
                }
            }
            GesturePhase::Hold => {
                if let Some(seg) = open.as_mut() {
                    seg.held = true;
                }
            }
        }
    }
    let last = spans.len().saturating_sub(1);
    cross(&mut open, &mut out, at, last);
    if let Some(mut seg) = open.take() {
        // Held or still touched: it ends at its pass's end.
        let end = pass_window(pieces, seg.pass).map_or(seg.first_frame(), |w| w.1);
        seg.end_at(end);
        out.push(seg);
    }
    out
}

/// Ruling 9: pass 0's Write segments of a held target merge into one per
/// contiguous stretch of pass 0, from the stretch's start at the held
/// value, each touch changing it from its first frame on.
///
/// Only held targets are merged: a Write strip's plugin parameter (no
/// hold, written from its touch) keeps its own segment, which its Hold
/// carries to the pass's end (review m2).
fn merge_holds(segs: Vec<Segment>, hold: f64, spans: &[Span]) -> Vec<Segment> {
    let (mut writes, mut rest): (Vec<Segment>, Vec<Segment>) = segs
        .into_iter()
        .partition(|s| s.pass == 0 && s.mode == WriteMode::Write && s.kind != Kind::Offset);
    writes.sort_by_key(|s| (s.piece, s.first_frame()));
    let mut value = unit(hold);
    let mut k = 0usize;
    while let Some(first) = spans.get(k) {
        if first.pass != 0 {
            break;
        }
        // The stretch: pieces k..m.
        let mut m = k.saturating_add(1);
        while spans.get(m).is_some_and(|s| !s.wrap && !s.jump) {
            m = m.saturating_add(1);
        }
        let mut merged = Segment {
            pass: 0,
            piece: k,
            mode: WriteMode::Write,
            kind: Kind::Absolute,
            samples: vec![(first.start, value)],
            held: true,
            reopened: false,
        };
        for seg in writes.iter().filter(|s| (k..m).contains(&s.piece)) {
            let so_far = merged.last();
            merged.push(seg.first_frame().saturating_sub(1), so_far);
            for (f, v) in &seg.samples {
                merged.push(*f, *v);
            }
        }
        merged.end_at(first.stretch_end);
        value = merged.last();
        rest.push(merged);
        k = m;
    }
    rest
}

fn frames(samples: &[(i64, f64)]) -> Vec<(u64, f64)> {
    samples
        .iter()
        .map(|(f, v)| (u64::try_from(*f).unwrap_or(0), *v))
        .collect()
}

/// A segment's points: offsets made absolute (step 4), values thinned
/// (step 5).
fn points(c: &AutoCapture<'_>, target: AutomationTarget, seg: &Segment) -> Vec<(u64, f64)> {
    let samples = frames(&seg.samples);
    if seg.kind != Kind::Offset {
        return thin(
            &samples,
            c.thinning.tolerance(target),
            is_db(target),
            (c.stepped)(target),
        );
    }
    let db = is_db(target);
    // Offsets are in dB for dB targets: thinned with the dB tolerance in
    // their own unit.
    let offsets = thin(&samples, c.thinning.tolerance(target), false, false);
    let (Some(first), Some(last)) = (offsets.first(), offsets.last()) else {
        return Vec::new();
    };
    let mut at: Vec<u64> = offsets.iter().map(|o| o.0).collect();
    at.extend((c.existing_points)(target, first.0, last.0));
    at.retain(|f| *f >= first.0 && *f <= last.0);
    at.sort_unstable();
    at.dedup();
    let offset_at = |f: u64| -> f64 {
        let n = offsets.partition_point(|o| o.0 <= f);
        match (
            n.checked_sub(1).and_then(|i| offsets.get(i)),
            offsets.get(n),
        ) {
            (Some(a), Some(b)) if b.0 > a.0 => {
                a.1 + (b.1 - a.1) * (f - a.0) as f64 / (b.0 - a.0) as f64
            }
            (Some(a), _) => a.1,
            (None, Some(b)) => b.1,
            (None, None) => 0.0,
        }
    };
    at.into_iter()
        .map(|f| {
            let e = (c.existing)(target, f).unwrap_or_else(|| (c.fixed)(target));
            let d = offset_at(f);
            let v = if db {
                fader_norm(fader_db(e) + d)
            } else {
                unit(e + d)
            };
            (f, v)
        })
        .collect()
}

/// Every write the capture makes, by pass, then the piece it starts in,
/// then first frame (Rulings 9–12, 23): the order they were made in, so a
/// later one wins where they overlap.
pub fn writes(c: &AutoCapture<'_>) -> Vec<TargetWrite> {
    let all = writing_pieces(c.marks, i64::try_from(c.end_run).unwrap_or(i64::MAX));
    let live: Vec<&Piece> = all.iter().filter(|p| p.end > p.from).collect();
    if live.is_empty() {
        return Vec::new();
    }
    let spans = spans(&live);
    // Targets in the order they first appear (stamps, then holds).
    let mut targets: Vec<AutomationTarget> = Vec::new();
    let mut placed: Vec<(AutomationTarget, Placed)> = Vec::with_capacity(c.stamps.len());
    for s in c.stamps {
        let Some(mode) = WriteMode::of_tag(s.tag) else {
            continue;
        };
        // Never before the pass's start (Task 6 review m2): a later
        // piece's placement already starts at its own start.
        let (p, frame) = match piece_of(&all, s.run) {
            Some(p) => (p, place(p, s.run).max(place(p, p.from))),
            // An end no piece covers (a latency rise's catch region a
            // quick fall cut away; review m1) still ends its segment, at
            // the end of the piece before: dropped, it would leave the
            // segment running on as a Latch.
            None if matches!(
                s.phase,
                GesturePhase::Release | GesturePhase::Glide(_) | GesturePhase::Hold
            ) =>
            {
                // At the start of the piece the gap precedes, so the
                // segment crosses the break first and a Latch held there
                // reopens (re-review N2); at the end of the piece before
                // only when none follows.
                match live.iter().find(|q| q.from > s.run) {
                    Some(q) => (*q, place(q, q.from)),
                    None => match live.iter().rev().find(|q| q.end <= s.run) {
                        Some(q) => (*q, place(q, q.end)),
                        None => continue,
                    },
                }
            }
            None => continue,
        };
        let Some(piece) = live.iter().position(|q| std::ptr::eq(*q, p)) else {
            continue;
        };
        // Write after the first pass is the After Write mode (M3).
        let mode = if mode == WriteMode::Write && p.pass > 0 {
            match c.after_write {
                Some(m) => m,
                None => continue,
            }
        } else {
            mode
        };
        let target = s.dest.target();
        if !targets.contains(&target) {
            targets.push(target);
        }
        placed.push((
            target,
            Placed {
                piece,
                frame,
                phase: sanitised(s.phase),
                mode,
            },
        ));
    }
    for (t, _) in c.holds {
        if !targets.contains(t) {
            targets.push(*t);
        }
    }
    let mut made: Vec<(usize, usize, i64, TargetWrite)> = Vec::new();
    for target in targets {
        let mut mine: Vec<Placed> = placed
            .iter()
            .filter(|(t, _)| *t == target)
            .map(|(_, p)| *p)
            .collect();
        // Stable: stamps keep their order on equal places (the clipped
        // ones at a pass's start).
        mine.sort_by_key(|p| (p.piece, p.frame));
        let mut segs = segments(c, target, &mine, &spans, &all);
        if let Some((_, h)) = c.holds.iter().find(|(t, _)| *t == target) {
            segs = merge_holds(segs, *h, &spans);
        }
        for seg in &segs {
            let points = points(c, target, seg);
            if points.is_empty() {
                continue;
            }
            made.push((
                seg.pass,
                seg.piece,
                seg.first_frame(),
                TargetWrite {
                    target,
                    pass: seg.pass,
                    points,
                },
            ));
        }
    }
    made.sort_by_key(|(pass, piece, frame, _)| (*pass, *piece, *frame));
    made.into_iter().map(|(_, _, _, w)| w).collect()
}

/// Ramer–Douglas–Peucker on vertical distance (Ruling 12): in dB when `db`
/// (normalised fader values), keeping only changes when `stepped`; the
/// first and last samples always stay; on equal maximum distance the
/// earliest index splits; iterative, with an explicit stack (pre-flight
/// M7).
pub fn thin(samples: &[(u64, f64)], tolerance: f64, db: bool, stepped: bool) -> Vec<(u64, f64)> {
    let n = samples.len();
    if n <= 2 {
        return samples.to_vec();
    }
    if stepped {
        let mut prev: Option<f64> = None;
        return samples
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                let changed = prev.is_none_or(|p| p != s.1);
                prev = Some(s.1);
                changed || *i + 1 == n
            })
            .map(|(_, s)| *s)
            .collect();
    }
    let y: Vec<f64> = samples
        .iter()
        .map(|s| if db { fader_db(s.1) } else { s.1 })
        .collect();
    let at = |i: usize| -> (f64, f64) {
        (
            samples.get(i).map_or(0.0, |s| s.0 as f64),
            y.get(i).copied().unwrap_or(0.0),
        )
    };
    let mut keep = vec![false; n];
    for i in [0, n - 1] {
        if let Some(k) = keep.get_mut(i) {
            *k = true;
        }
    }
    let mut stack: Vec<(usize, usize)> = vec![(0, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a.saturating_add(1) {
            continue;
        }
        let ((xa, ya), (xb, yb)) = (at(a), at(b));
        let mut best: Option<(usize, f64)> = None;
        for i in a + 1..b {
            let (x, v) = at(i);
            let line = if xb > xa {
                ya + (yb - ya) * (x - xa) / (xb - xa)
            } else {
                ya
            };
            let d = (v - line).abs();
            // Strictly greater: the earliest of equal distances splits.
            if best.is_none_or(|(_, m)| d > m) {
                best = Some((i, d));
            }
        }
        if let Some((i, d)) = best
            && d > tolerance
        {
            if let Some(k) = keep.get_mut(i) {
                *k = true;
            }
            stack.push((i, b));
            stack.push((a, i));
        }
    }
    samples
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(s, _)| *s)
        .collect()
}

/// The model's writes (Ruling 13): ticks through `tempo` (a point
/// rounding onto the tick before goes to the next tick when the next
/// point leaves room, else, and for a `stepped` target, equal ticks keep
/// the last value), the range one tick beyond the first and last, an edge
/// point at each holding `outside(target, tick)`, or the value of the
/// latest earlier write in `writes` of the same target covering it (by
/// the lane rule, stepped when `stepped`); a write longer than
/// `MAX_WRITE_POINTS` is split into consecutive writes (Ruling 25).
pub fn to_model(
    writes: &[TargetWrite],
    seq: SeqId,
    tempo: &TempoTable,
    outside: &dyn Fn(AutomationTarget, Tick) -> f64,
    name: &dyn Fn(AutomationTarget) -> String,
    stepped: &dyn Fn(AutomationTarget) -> bool,
) -> Vec<AutomationWrite> {
    let mut out = Vec::new();
    let chunk = MAX_WRITE_POINTS.saturating_sub(2).max(1);
    // The writes made so far (target, range, every point with its edges):
    // they apply before the later ones, so an edge inside one of them
    // holds its value, not the value before the pass (review I3).
    let mut prior: Vec<Prior> = Vec::new();
    for w in writes {
        let step = stepped(w.target);
        let ticks: Vec<Tick> = w
            .points
            .iter()
            .map(|(f, _)| tempo.frame_to_tick(*f))
            .collect();
        let mut pts: Vec<(Tick, f64)> = Vec::with_capacity(w.points.len());
        for (i, (t, (_, v))) in ticks.iter().zip(&w.points).enumerate() {
            // The last point holds the value of the one before it (the
            // corner before a step: a release's hold, Write's hold before
            // a touch).
            let corner = !step
                && pts
                    .len()
                    .checked_sub(2)
                    .and_then(|k| pts.get(k))
                    .zip(pts.last())
                    .is_some_and(|(a, b)| a.1 == b.1);
            match pts.last_mut() {
                // A point rounding onto the tick before (adjacent frames
                // share a tick about a third of the time at 120 BPM)
                // goes to the next tick, so a hold then a step stays a
                // step (re-review I2, N1). Only when the next point does
                // not already own that tick or one before it; else, and
                // for a stepped target (whose lane holds anyway), the
                // later value wins on the tick. The write's end follows
                // its last point, so a bump never passes it.
                Some(l) if l.0 >= *t => {
                    let next = Tick(l.0.0.saturating_add(1));
                    let room = ticks.get(i + 1).is_none_or(|n| *n > next);
                    if !step && l.1 != *v && room {
                        pts.push((next, *v));
                    } else if !(corner && l.1 != *v) {
                        l.1 = *v;
                    }
                    // Else no room and the last point is a hold corner:
                    // it stays and `v`, which lasted under a tick before
                    // the next point, goes (re-review 2 N4), so the hold
                    // does not become a ramp from the point before.
                }
                _ => pts.push((*t, *v)),
            }
        }
        let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
            continue;
        };
        let start = Tick(first.0.0.saturating_sub(1));
        let end = Tick(last.0.0.saturating_add(1));
        let lane_name = name(w.target);
        let edge = |t: Tick| -> f64 {
            prior
                .iter()
                .rev()
                .find(|(x, a, b, _)| *x == w.target && *a <= t && t <= *b)
                .map_or_else(|| outside(w.target, t), |(_, _, _, p)| lane_at(p, t, step))
        };
        let (start_value, end_value) = (edge(start), edge(end));
        let mut all = Vec::with_capacity(pts.len() + 2);
        all.push((start, start_value));
        all.extend_from_slice(&pts);
        all.push((end, end_value));
        let chunks: Vec<&[(Tick, f64)]> = pts.chunks(chunk).collect();
        let count = chunks.len();
        for (i, c) in chunks.iter().enumerate() {
            let is_last = i + 1 == count;
            let from = if i == 0 {
                start
            } else {
                c.first().map_or(start, |p| p.0)
            };
            let to = if is_last {
                end
            } else {
                chunks
                    .get(i + 1)
                    .and_then(|n| n.first())
                    .map_or(end, |p| Tick(p.0.0.saturating_sub(1)))
            };
            let mut points = Vec::with_capacity(c.len() + 2);
            if i == 0 {
                points.push((start, start_value));
            }
            points.extend_from_slice(c);
            if is_last {
                points.push((end, end_value));
            }
            out.push(AutomationWrite {
                seq,
                target: w.target,
                start: from,
                end: to,
                points,
                lane_name: lane_name.clone(),
            });
        }
        prior.push((w.target, start, end, all));
    }
    out
}

/// An earlier write in `to_model`: its target, range and every point
/// with its edges.
type Prior = (AutomationTarget, Tick, Tick, Vec<(Tick, f64)>);

/// The value of `points` (by tick) at `t` by Plan 4d's lane rule: the
/// first value before the first point, the last after the last, linear
/// between unless `stepped`.
fn lane_at(points: &[(Tick, f64)], t: Tick, stepped: bool) -> f64 {
    let n = points.partition_point(|p| p.0 <= t);
    match (n.checked_sub(1).and_then(|i| points.get(i)), points.get(n)) {
        (Some(a), Some(b)) if !stepped && b.0 > a.0 => {
            a.1 + (b.1 - a.1) * (t.0 - a.0.0) as f64 / (b.0.0 - a.0.0) as f64
        }
        (Some(a), _) => a.1,
        (None, Some(b)) => b.1,
        (None, None) => 0.0,
    }
}
