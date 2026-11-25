//! The take drawn while it records (Swift `M/GrowingTake.swift`, plan
//! 5d-2 Task 9; spec §6.2 "the region draws as it grows"): where each
//! armed audio track's take sits in the shown sequence, which file frames
//! fill it, and the pieces of its visible part whose live peaks the
//! record poll asks for. The reads run off the UI thread (the store runs
//! a [`TakeRead`]); render draws only what is held.
//!
//! The maths is checked throughout (Swift 5d-2 final review I2): the core
//! saturates the pass's file frame at `i64::MAX`, and a trap mid-recording
//! is a crash; a value out of range draws no take.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use vizdaw_ffi::OverviewRows;

use crate::arrangement::{Geometry, Rect, TimeAxis};
use crate::document::DocumentModel;
use crate::session::DocSession;

/// The width of one piece of a growing take, in points.
pub const PIECE_WIDTH: f32 = 64.;
/// The most pieces a take is cut into.
pub const MAX_PIECES: usize = 32;
/// The core's column limit for one request, shared by the pieces.
pub const MAX_COLUMNS: u32 = 8192;
/// The least time between two take reads' starts: about ten a second,
/// however often the poll runs (fix round 1 I1(b)).
pub const READ_EVERY: Duration = Duration::from_millis(100);

/// A take being written, where the arrangement draws it: its span in the
/// sequence and the file frames that fill it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrowingTake {
    pub lane: u64,
    pub start_tick: i64,
    pub end_tick: i64,
    pub from_frame: u64,
    pub to_frame: u64,
}

/// One piece of a growing take's visible part: its span in points (and
/// the ticks at its edges), and the file frames and columns that fill it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrowingSlice {
    pub min_x: f32,
    pub max_x: f32,
    pub from_tick: i64,
    pub to_tick: i64,
    pub from: u64,
    pub to: u64,
    pub columns: u32,
}

/// Where the arrangement looks, as its view last laid out: the axis and
/// the canvas's width. The record poll cuts the takes' visible parts
/// from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TakeView {
    pub axis: TimeAxis,
    pub width: f32,
}

/// A piece's rows as the poll read them, between two ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldPiece {
    pub from_tick: i64,
    pub to_tick: i64,
    pub rows: Arc<OverviewRows>,
}

/// A take as held: where the last poll put it and its pieces' rows as
/// the last read landed them.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldTake {
    pub take: GrowingTake,
    pub pieces: Arc<[HeldPiece]>,
}

/// One track's pieces to read.
#[derive(Debug, Clone, PartialEq)]
struct TakeAsk {
    lane: u64,
    slices: Vec<GrowingSlice>,
}

/// The pieces' reads of one poll, run off the UI thread
/// ([`TakeRead::run`]): one `recording_waveform` call per piece.
pub struct TakeRead {
    pub ticket: u64,
    pub pass: u64,
    asks: Vec<TakeAsk>,
    session: Arc<dyn DocSession>,
}

impl std::fmt::Debug for TakeRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TakeRead")
            .field("ticket", &self.ticket)
            .field("pass", &self.pass)
            .field("asks", &self.asks)
            .finish()
    }
}

/// What a [`TakeRead`] read: each track's pieces, `None` when a read
/// failed.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeReply {
    pub ticket: u64,
    pub pass: u64,
    pub lanes: Vec<(u64, Option<Arc<[HeldPiece]>>)>,
}

impl TakeRead {
    /// Reads (core calls: never on the UI thread).
    pub fn run(self) -> TakeReply {
        let lanes = self
            .asks
            .into_iter()
            .map(|a| {
                let pieces: Option<Vec<HeldPiece>> = a
                    .slices
                    .iter()
                    .map(|s| {
                        self.session
                            .recording_waveform(a.lane, s.from, s.to, s.columns)
                            .ok()
                            .map(|rows| HeldPiece {
                                from_tick: s.from_tick,
                                to_tick: s.to_tick,
                                rows: Arc::new(rows),
                            })
                    })
                    .collect();
                (a.lane, pieces.map(Arc::from))
            })
            .collect();
        TakeReply {
            ticket: self.ticket,
            pass: self.pass,
            lanes,
        }
    }
}

/// What the record poll holds for the takes: the view handed over on
/// layout, where each take is, the rows by track as the last read landed
/// them, and the read waiting for the store or in flight.
#[derive(Debug, Default)]
pub struct GrowingRows {
    view: Option<TakeView>,
    takes: Vec<GrowingTake>,
    rows: HashMap<u64, Arc<[HeldPiece]>>,
    /// Bumped when a pass ends: a read from before lands nowhere.
    pass: u64,
    next_ticket: u64,
    /// The read out now (one at a time: a slow read slows the take's
    /// refresh, never queues reads up).
    in_flight: Option<u64>,
    pending: Option<TakeRead>,
    /// When the last read started (at most one per [`READ_EVERY`]).
    last_read: Option<Instant>,
    /// The takes as held at Stop, and the snapshot version Stop landed:
    /// the placed regions draw them until their own rows are held (fix
    /// round 1 M3).
    handover: Option<(u64, Vec<HeldTake>)>,
}

impl Clone for GrowingRows {
    /// A copy holds what is drawn, not the read waiting.
    fn clone(&self) -> Self {
        GrowingRows {
            view: self.view,
            takes: self.takes.clone(),
            rows: self.rows.clone(),
            pass: self.pass,
            next_ticket: self.next_ticket,
            in_flight: self.in_flight,
            pending: None,
            last_read: self.last_read,
            handover: self.handover.clone(),
        }
    }
}

impl GrowingRows {
    /// The pass ended: the takes and their rows go (the placed region
    /// draws it), and a read out now lands nowhere. The view stays.
    pub(crate) fn clear_takes(&mut self) {
        self.takes.clear();
        self.rows.clear();
        self.pending = None;
        self.in_flight = None;
        self.last_read = None;
        self.pass += 1;
    }
}

impl GrowingTake {
    /// The frames to ask the writer's file for, and the columns to draw
    /// them in, for the part of a take's `rect` that is `visible` (at most
    /// 8192 columns); `None` when nothing is visible or the frames are
    /// empty.
    pub fn slice(rect: Rect, visible: Rect, from: u64, to: u64) -> Option<(u64, u64, u32)> {
        let vis = rect.intersection(&visible)?;
        if !(vis.width > 0. && rect.width > 0. && to > from) {
            return None;
        }
        let span = (to - from) as f64;
        let at = |x: f32| {
            let share = f64::from((x - rect.x) / rect.width).clamp(0., 1.);
            from.saturating_add((span * share) as u64)
        };
        let (f0, f1) = (at(vis.x), at(vis.max_x()));
        if f1 <= f0 {
            return None;
        }
        let columns = (vis.width.ceil() as u32).min(MAX_COLUMNS);
        (columns > 0).then_some((f0, f1, columns))
    }

    /// Its rect's x and width on `axis` (Swift `drawGrowingTakes`: at
    /// least the block's least width).
    pub fn x_span(&self, axis: &TimeAxis) -> (f32, f32) {
        let x0 = axis.x(self.start_tick);
        (
            x0,
            (axis.x(self.end_tick) - x0).max(Geometry::MIN_BLOCK_WIDTH),
        )
    }
}

/// `x` rounded to whole frames, or `None` when it is not finite or out of
/// range (Swift `Int64(exactly:)`).
fn frames(x: f64) -> Option<i64> {
    let r = x.round();
    const LIMIT: f64 = 9_223_372_036_854_775_808.; // 2^63
    (r.is_finite() && (-LIMIT..LIMIT).contains(&r)).then_some(r as i64)
}

impl DocumentModel {
    /// The shown sequence's armed audio tracks' takes while a pass records:
    /// from the later of the pass's start and the record range's start, to
    /// what the writer has (at most the punch-out). Each track is placed
    /// where Stop will place it (plan 5d-1 Task 10 review I1): file frame
    /// `pass_start_file_frame + return_frames` (signed: a large latency
    /// trim can put it before the file's start) is heard at
    /// `pass_start_tick`, and a take whose wanted start is before the
    /// file's first frame starts at that frame, later. Ticks and seconds
    /// map through the sequence's tempo; a Locate during the pass is not
    /// followed. It calls the session: the poll's, never render's.
    pub fn growing_takes(&self) -> Vec<GrowingTake> {
        let Some(s) = self.record.state().filter(|s| s.recording) else {
            return vec![];
        };
        let (Some(seq), Some(pass_tick)) = (s.seq, s.pass_start_tick) else {
            return vec![];
        };
        let Some(rate) = self.project_info().map(|i| i.format.sample_rate) else {
            return vec![];
        };
        if Some(seq) != self.selected_seq() || rate == 0 {
            return vec![];
        }
        let start = pass_tick.max(s.range_start_tick);
        let mut ticks = vec![pass_tick, start];
        if let Some(p) = s.spec.as_ref().and_then(|p| p.punch) {
            ticks.push(p.end);
        }
        let Ok(secs) = self.session.seconds_at(seq, ticks.clone()) else {
            return vec![];
        };
        if secs.len() != ticks.len() {
            return vec![];
        }
        let hz = f64::from(rate);
        let written = i64::try_from(s.frames).unwrap_or(i64::MAX);
        let Some(start_off) = frames((secs[1] - secs[0]) * hz) else {
            return vec![];
        };
        let punch_off = match secs.get(2) {
            Some(p) => match frames((p - secs[0]) * hz) {
                Some(p) => Some(p),
                None => return vec![],
            },
            None => None,
        };
        let mut out = vec![];
        for lane in s.lanes.iter().filter(|l| l.audio) {
            let Some(anchor) = s.pass_start_file_frame.checked_add(lane.return_frames) else {
                continue;
            };
            let Some(wanted) = anchor.checked_add(start_off.max(0)) else {
                continue;
            };
            let from = wanted.max(0);
            let mut to = written;
            if let Some(p) = punch_off {
                let Some(end) = anchor.checked_add(p) else {
                    continue;
                };
                to = to.min(end);
            }
            if to <= from {
                continue;
            }
            let (Some(from_off), Some(to_off)) = (from.checked_sub(anchor), to.checked_sub(anchor))
            else {
                continue;
            };
            let at = |off: i64| secs[0] + off as f64 / hz;
            let Ok(t) = self.session.ticks_at(seq, vec![at(from_off), at(to_off)]) else {
                continue;
            };
            if t.len() != 2 {
                continue;
            }
            let start_tick = if from == wanted { start } else { t[0] };
            if t[1] <= start_tick {
                continue;
            }
            out.push(GrowingTake {
                lane: lane.lane,
                start_tick,
                end_tick: t[1],
                from_frame: from as u64,
                to_frame: to as u64,
            });
        }
        out
    }

    /// The visible part of a growing take's `rect`, cut into pieces of
    /// [`PIECE_WIDTH`] points (at most 32, widened when needed), each with
    /// the file frames that sound under it. Every piece edge's tick
    /// (`tick_at` gives the tick at an x; the take's own edges are its
    /// ticks) goes through the sequence's tempo map in one `seconds_at`
    /// call, so a tempo change inside the take bends the waveform to where
    /// it is heard: frame `from_frame + round((secs(tick) −
    /// secs(start_tick)) × rate)`, held within `from_frame..=to_frame`.
    /// The columns of all pieces total at most 8192. It calls the session.
    pub fn growing_slices(
        &self,
        t: &GrowingTake,
        rect: Rect,
        visible: Rect,
        tick_at: impl Fn(f32) -> i64,
    ) -> Vec<GrowingSlice> {
        let Some(vis) = rect.intersection(&visible) else {
            return vec![];
        };
        let (Some(seq), Some(rate)) = (
            self.selected_seq(),
            self.project_info().map(|i| i.format.sample_rate),
        ) else {
            return vec![];
        };
        if !(vis.width > 0. && vis.width.is_finite()) || t.to_frame <= t.from_frame || rate == 0 {
            return vec![];
        }
        let count = ((vis.width / PIECE_WIDTH).ceil() as usize).clamp(1, MAX_PIECES);
        let step = vis.width / count as f32;
        let xs: Vec<f32> = (0..=count)
            .map(|i| {
                if i == count {
                    vis.max_x()
                } else {
                    vis.x + i as f32 * step
                }
            })
            .collect();
        let edge = |x: f32| {
            if x == rect.x {
                t.start_tick
            } else if x == rect.max_x() && rect.width > Geometry::MIN_BLOCK_WIDTH {
                t.end_tick
            } else {
                tick_at(x)
            }
        };
        let edges: Vec<i64> = xs.iter().map(|x| edge(*x)).collect();
        let mut ticks = vec![t.start_tick];
        ticks.extend(&edges);
        let Ok(secs) = self.session.seconds_at(seq, ticks.clone()) else {
            return vec![];
        };
        if secs.len() != ticks.len() {
            return vec![];
        }
        let hz = f64::from(rate);
        let span = (t.to_frame - t.from_frame) as f64;
        let mut at_edges: Vec<u64> = Vec::with_capacity(xs.len());
        for s in &secs[1..] {
            let off = ((s - secs[0]) * hz).round();
            if !off.is_finite() {
                return vec![];
            }
            let f = t.from_frame.saturating_add(off.clamp(0., span) as u64);
            // Never behind the previous edge, so the pieces stay contiguous.
            at_edges.push(f.max(at_edges.last().copied().unwrap_or(0)));
        }
        let per_piece = (MAX_COLUMNS / count as u32).max(1);
        (0..count)
            .filter(|&i| at_edges[i + 1] > at_edges[i])
            .map(|i| {
                let cols = (xs[i + 1] - xs[i]).ceil().max(0.) as u32;
                GrowingSlice {
                    min_x: xs[i],
                    max_x: xs[i + 1],
                    from_tick: edges[i],
                    to_tick: edges[i + 1],
                    from: at_edges[i],
                    to: at_edges[i + 1],
                    columns: cols.clamp(1, per_piece),
                }
            })
            .collect()
    }

    /// The view the arrangement last laid out, which the record poll cuts
    /// the takes from (the view hands it over on layout).
    pub fn set_take_view(&mut self, view: TakeView) {
        self.record.growing.view = Some(view);
    }

    pub fn take_view(&self) -> Option<TakeView> {
        self.record.growing.view
    }

    /// The takes as held (render draws only these): where the last poll
    /// put them, with their rows as the last read landed them.
    pub fn held_takes(&self) -> Vec<HeldTake> {
        let g = &self.record.growing;
        g.takes
            .iter()
            .map(|t| HeldTake {
                take: *t,
                pieces: g
                    .rows
                    .get(&t.lane)
                    .cloned()
                    .unwrap_or_else(|| Arc::from([])),
            })
            .collect()
    }

    /// The record poll's part for the takes (the poll's clock `now`):
    /// while a pass records, each shown track's take, and a read of its
    /// visible pieces' `recording_waveform` rows for the store to run off
    /// the UI thread, unless one is still out or one started less than
    /// [`READ_EVERY`] ago. True when a take appeared, went, or grew by a
    /// whole point on the view (a redraw).
    /// The rows a region placed by Stop draws until its own are held: the
    /// last rows of the take on `lane` that started at the region's tick,
    /// while the snapshot is still the one Stop landed (the owner's ruling:
    /// the take looks the same just before and after Stop).
    pub fn handed_over_take(&self, lane: u64, region_tick: i64) -> Option<Arc<[HeldPiece]>> {
        let (version, takes) = self.record.growing.handover.as_ref()?;
        if *version != self.snapshot().version {
            return None;
        }
        takes
            .iter()
            .find(|h| h.take.lane == lane && h.take.start_tick == region_tick)
            .map(|h| h.pieces.clone())
            .filter(|p| !p.is_empty())
    }

    /// The pass ended: its takes and rows go, kept for the placed regions
    /// to draw until theirs land (`version`: the snapshot Stop landed).
    pub(crate) fn hand_over_takes(&mut self, before: Vec<HeldTake>) {
        let version = self.snapshot().version;
        self.record.growing.handover = (!before.is_empty()).then_some((version, before));
    }

    /// A take read that will never land (it panicked) is forgotten: the
    /// next poll reads again.
    pub fn lose_growing_read(&mut self) {
        self.record.growing.in_flight = None;
    }

    pub fn poll_growing_takes_at(&mut self, now: Instant) -> bool {
        let shown: Vec<u64> = self.lanes().iter().map(|l| l.id).collect();
        let takes: Vec<GrowingTake> = if self.record.is_recording() {
            self.growing_takes()
                .into_iter()
                .filter(|t| shown.contains(&t.lane))
                .collect()
        } else {
            vec![]
        };
        let g = &self.record.growing;
        let read = match g.view {
            Some(v)
                if g.in_flight.is_none()
                    && g.pending.is_none()
                    && !takes.is_empty()
                    && g.last_read
                        .is_none_or(|t| now.saturating_duration_since(t) >= READ_EVERY) =>
            {
                let asks: Vec<TakeAsk> = takes
                    .iter()
                    .map(|t| TakeAsk {
                        lane: t.lane,
                        slices: self.visible_slices(t, v),
                    })
                    .collect();
                Some(asks)
            }
            _ => None,
        };
        let session = self.session.clone();
        let g = &mut self.record.growing;
        if let Some(asks) = read {
            g.last_read = Some(now);
            let ticket = g.next_ticket;
            g.next_ticket += 1;
            g.pending = Some(TakeRead {
                ticket,
                pass: g.pass,
                asks,
                session,
            });
        }
        g.rows
            .retain(|lane, _| takes.iter().any(|t| t.lane == *lane));
        let span = |ts: &[GrowingTake]| -> Vec<(u64, i64, i64)> {
            ts.iter()
                .map(|t| match g.view {
                    Some(v) => {
                        let (x, w) = t.x_span(&v.axis);
                        (t.lane, x.floor() as i64, (x + w).floor() as i64)
                    }
                    None => (t.lane, t.start_tick, t.end_tick),
                })
                .collect()
        };
        let changed = span(&g.takes) != span(&takes);
        g.takes = takes;
        changed
    }

    /// The pieces of take `t`'s part visible in `view`.
    fn visible_slices(&self, t: &GrowingTake, view: TakeView) -> Vec<GrowingSlice> {
        let (x0, width) = t.x_span(&view.axis);
        let rect = Rect {
            x: x0,
            y: 0.,
            width,
            height: 1.,
        };
        let visible = Rect {
            x: 0.,
            y: 0.,
            width: view.width,
            height: 1.,
        };
        let axis = view.axis;
        self.growing_slices(t, rect, visible, |x| axis.tick_at(x))
    }

    /// The pieces' read the last poll asked for, taken: the store runs it
    /// off the UI thread and lands it with [`Self::land_growing_read`].
    pub fn take_growing_read(&mut self) -> Option<TakeRead> {
        let g = &mut self.record.growing;
        let r = g.pending.take()?;
        g.in_flight = Some(r.ticket);
        Some(r)
    }

    /// A pieces' read landed: each track's rows replace what was held; a
    /// failed read keeps that track's last rows; a read from an ended pass
    /// changes nothing. True when the rows changed (redraw).
    pub fn land_growing_read(&mut self, reply: TakeReply) -> bool {
        let g = &mut self.record.growing;
        if g.in_flight == Some(reply.ticket) {
            g.in_flight = None;
        }
        if reply.pass != g.pass {
            return false;
        }
        let mut changed = false;
        for (lane, pieces) in reply.lanes {
            if let Some(p) = pieces
                && g.takes.iter().any(|t| t.lane == lane)
                && g.rows.get(&lane) != Some(&p)
            {
                g.rows.insert(lane, p);
                changed = true;
            }
        }
        changed
    }
}
