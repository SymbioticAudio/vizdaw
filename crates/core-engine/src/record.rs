//! The renderer's record tap (sub-project 5 spec §6.2–§6.4; plan 5c
//! Rulings 3–6). While a tap is installed and the transport plays, every
//! block of device input goes into the input ring stamped with its *run
//! frame*: frames counted from the first block that played, continuous
//! across loop wraps. *Marks* say which schedule frame the output played
//! at a run frame: one when the transport rolls and one at every wrap (and
//! after a block the transport did not advance through). When the transport
//! leaves Playing (a Stop, or the sequence end) a last non-wrap mark says
//! where, and the run stops growing until it plays again (pre-flight M2:
//! the planner ends the last piece there). The control side
//! (core-ffi) turns runs into timeline positions with the marks, the
//! device's input delay and the graph latency each mark carries (a graph
//! swap that changes the latency mid-pass marks where it took effect:
//! final review I1), so the real-time side does no mapping at all. // RT
//! where marked

use crate::midi1::Midi1Msg;
use core_media::record::{RingReader, RingWriter, input_ring};
use core_model::ids::LaneId;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

/// Marks a tap can hold until the control side reads them.
pub const MARK_CAPACITY: usize = 4096;
/// Recorded MIDI messages a tap can hold until the control side reads
/// them.
pub const MIDI_CAPACITY: usize = 16_384;

/// Output run frame `run` played schedule frame `frame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    /// The run frame.
    pub run: u64,
    /// The schedule frame.
    pub frame: u64,
    /// A loop wrap: a new pass starts here (spec §6.4). The first mark is
    /// the roll; others are resyncs (after a block the transport did not
    /// advance through, or a schedule swap), a stop (the transport left
    /// Playing at `run`; nothing plays after it until the next mark), or
    /// the resume after one.
    pub wrap: bool,
    /// The graph latency `L` (frames) from this mark until the next: the
    /// output played at run `x` is heard at `x + L`.
    pub latency: u64,
}

/// A MIDI message an armed lane recorded, at the output run frame that
/// was being heard when it arrived (spec §6.3; the graph latency is not
/// yet taken off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedMidi {
    /// The lane.
    pub lane: LaneId,
    /// The output run frame (negative: before the transport rolled).
    pub run: i64,
    /// The message (after the lane's input chain).
    pub msg: Midi1Msg,
}

/// Where the control side reads a tap's marks.
pub type MarkReader = Consumer<Mark>;
/// Where the control side reads a tap's recorded MIDI.
pub type MidiReader = Consumer<RecordedMidi>;

/// What a tap shares with the control side.
#[derive(Debug)]
pub struct RecordStatus {
    run: AtomicU64,
    delay: AtomicI64,
    closed: AtomicBool,
    lost_blocks: AtomicU64,
    lost_marks: AtomicU64,
    lost_midi: AtomicU64,
}

impl RecordStatus {
    /// Run frames played (or lost) so far.
    pub fn run(&self) -> u64 {
        self.run.load(Ordering::Acquire)
    }

    /// The device's input delay at the first captured block (spec §6.3),
    /// once one was captured; refreshed when playing resumes after a stop
    /// (it then holds for the frames after the resume mark).
    pub fn delay(&self) -> Option<i64> {
        let d = self.delay.load(Ordering::Acquire);
        (d != i64::MIN).then_some(d)
    }

    /// The tap is gone: removed, or dropped with its renderer (an output
    /// that stopped).
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Input blocks that did not fit into the ring.
    pub fn lost_blocks(&self) -> u64 {
        self.lost_blocks.load(Ordering::Relaxed)
    }

    /// Marks that did not fit (the pass cannot be placed exactly).
    pub fn lost_marks(&self) -> u64 {
        self.lost_marks.load(Ordering::Relaxed)
    }

    /// Recorded MIDI messages that did not fit.
    pub fn lost_midi(&self) -> u64 {
        self.lost_midi.load(Ordering::Relaxed)
    }
}

/// What to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRequest {
    /// The device input channels the ring carries, in ring order (empty:
    /// MIDI only).
    pub channels: Vec<u16>,
    /// The record range in schedule frames, for auto-input monitoring
    /// (`None`: from the start until Stop).
    pub range: Option<(u64, u64)>,
    /// Ring length in frames.
    pub ring_frames: usize,
}

/// The control side of a tap.
#[derive(Debug)]
pub struct RecordHandles {
    /// The input ring's reader (for the writer thread).
    pub ring: RingReader,
    /// Marks, oldest first.
    pub marks: MarkReader,
    /// Recorded MIDI, oldest first.
    pub midi: MidiReader,
    /// Counters and the run.
    pub status: Arc<RecordStatus>,
}

/// The real-time side of a recording, owned by the renderer.
#[derive(Debug)]
pub struct RecordTap {
    ring: RingWriter,
    channels: Box<[u16]>,
    marks: Producer<Mark>,
    midi: Producer<RecordedMidi>,
    status: Arc<RecordStatus>,
    range: Option<(u64, u64)>,
    rolling: bool,
    run: u64,
    remark: bool,
    /// The transport left Playing: the run does not grow until it plays
    /// again (pre-flight M2).
    stopped: bool,
    /// The block offset it stopped at, in the block that stopped.
    stop_at: Option<u32>,
    /// The run of the last resume after a stop: a message stamped before
    /// it arrived while the transport stood still (Task 6 review carry
    /// 4), where the frozen run already played the timeline before the
    /// stop, and is dropped.
    resumed_at: Option<u64>,
    /// The `(run, frame)` of the last mark pushed: a wrap repeating it adds
    /// nothing (a roll exactly at the loop end is no wrap too).
    last_mark: Option<(u64, u64)>,
    /// The graph latency the marks carry (the renderer's, kept current by
    /// [`RecordTap::set_latency`]).
    latency: u64,
}

impl Drop for RecordTap {
    fn drop(&mut self) {
        self.status.closed.store(true, Ordering::Release);
    }
}

/// A tap and its control side, its marks carrying graph latency `latency`
/// until the renderer says otherwise. Allocates; call off the real-time
/// thread.
pub fn tap(req: &RecordRequest, latency: u64) -> (RecordTap, RecordHandles) {
    let (ring, reader) = input_ring(req.channels.len().max(1), req.ring_frames.max(1));
    let (mark_tx, mark_rx) = RingBuffer::new(MARK_CAPACITY);
    let (midi_tx, midi_rx) = RingBuffer::new(MIDI_CAPACITY);
    let status = Arc::new(RecordStatus {
        run: AtomicU64::new(0),
        delay: AtomicI64::new(i64::MIN),
        closed: AtomicBool::new(false),
        lost_blocks: AtomicU64::new(0),
        lost_marks: AtomicU64::new(0),
        lost_midi: AtomicU64::new(0),
    });
    (
        RecordTap {
            ring,
            channels: req.channels.clone().into_boxed_slice(),
            marks: mark_tx,
            midi: midi_tx,
            status: Arc::clone(&status),
            range: req.range,
            rolling: false,
            run: 0,
            remark: false,
            stopped: false,
            stop_at: None,
            resumed_at: None,
            last_mark: None,
            latency,
        },
        RecordHandles {
            ring: reader,
            marks: mark_rx,
            midi: midi_rx,
            status,
        },
    )
}

impl RecordTap {
    /// Whether the transport rolled since the tap was installed.
    pub fn rolling(&self) -> bool {
        self.rolling
    }

    /// The run frame of the block about to render.
    pub fn run(&self) -> u64 {
        self.run
    }

    /// The tap is removed: the control side reads it closed now, before
    /// the retire thread frees it. // RT
    pub fn close(&self) {
        self.ring.close();
        self.status.closed.store(true, Ordering::Release);
    }

    /// The record range, `(0, u64::MAX)` when it has none. // RT
    pub fn range_or_all(&self) -> (u64, u64) {
        self.range.unwrap_or((0, u64::MAX))
    }

    /// Whether `frame` lies in the record range. // RT
    pub fn in_range(&self, frame: u64) -> bool {
        self.range.is_none_or(|(a, b)| frame >= a && frame < b)
    }

    /// Pushes a mark; a wrap that repeats the last mark is dropped (review
    /// fix M3: a roll exactly at the loop end would also mark a wrap at run
    /// 0). Other repeats stay: a resume after a stop must be marked. // RT
    fn mark(&mut self, run: u64, frame: u64, wrap: bool) {
        if wrap && self.last_mark == Some((run, frame)) {
            return;
        }
        let latency = self.latency;
        if self
            .marks
            .push(Mark {
                run,
                frame,
                wrap,
                latency,
            })
            .is_err()
        {
            self.status.lost_marks.fetch_add(1, Ordering::Relaxed);
        } else {
            self.last_mark = Some((run, frame));
        }
    }

    /// At a block's start while playing: rolls (run 0 plays `frame`), or
    /// marks where the transport is after a block it did not advance
    /// through, after a jump or after a stop. A resume after a stop takes
    /// the device's input delay as it is then (review fix M4). // RT
    pub fn begin_block(&mut self, frame: u64, delay: i64) {
        if !self.rolling {
            self.rolling = true;
            self.run = 0;
            self.status.delay.store(delay, Ordering::Release);
            self.mark(0, frame, false);
        } else if self.remark || self.stopped {
            if self.stopped {
                self.status.delay.store(delay, Ordering::Release);
                self.resumed_at = Some(self.run);
            }
            self.mark(self.run, frame, false);
        }
        self.remark = false;
        self.stopped = false;
        self.stop_at = None;
    }

    /// The transport left Playing at block offset `offset` (0: before the
    /// block, a Stop), standing at schedule frame `frame`: marks the stop
    /// and stops the run there (pre-flight M2). // RT
    pub fn stop(&mut self, offset: u32, frame: u64) {
        if !self.rolling || self.stopped {
            return;
        }
        let run = self.run.saturating_add(u64::from(offset));
        self.mark(run, frame, false);
        self.stopped = true;
        self.stop_at = Some(offset);
        self.remark = false;
    }

    /// A loop wrap at block offset `offset` continues at schedule frame
    /// `to`. // RT
    pub fn wrap(&mut self, offset: u32, to: u64) {
        if self.rolling {
            let run = self.run.saturating_add(u64::from(offset));
            self.mark(run, to, true);
        }
    }

    /// The schedule changed under the transport (an edit while recording
    /// recompiled it), or the transport jumped (a Locate): the next block
    /// marks where it is. // RT
    pub fn resync(&mut self) {
        if self.rolling {
            self.remark = true;
        }
    }

    /// The graph latency is `latency` from the next block on (the renderer
    /// installed the tap, or swapped in a graph): a change while rolling
    /// marks where it took effect, so what follows is placed with its own
    /// latency (final review I1). // RT
    pub fn set_latency(&mut self, latency: u64) {
        if latency != self.latency {
            self.latency = latency;
            self.resync();
        }
    }

    /// Records an armed lane's message heard at block offset `offset`
    /// (negative: before the block), unless it was heard before the last
    /// resume after a stop. // RT
    pub fn midi(&mut self, lane: LaneId, offset: i64, msg: Midi1Msg) {
        if !self.rolling {
            return;
        }
        if self.stopped {
            // Only what was heard before the stop in its own block.
            match self.stop_at {
                Some(k) if offset < i64::from(k) => {}
                _ => return,
            }
        }
        let run = i64::try_from(self.run)
            .unwrap_or(i64::MAX)
            .saturating_add(offset);
        if self
            .resumed_at
            .is_some_and(|r| run < i64::try_from(r).unwrap_or(i64::MAX))
        {
            return;
        }
        if self.midi.push(RecordedMidi { lane, run, msg }).is_err() {
            self.status.lost_midi.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// At a block's end: pushes its input (device channel `c` from
    /// `input[c]`) and advances the run. `advanced` is false when the
    /// transport did not play the whole block (an xrun or a missed
    /// deadline): the next block marks where it is. In the block the
    /// transport stopped in, only the frames before the stop are pushed;
    /// after it nothing is. // RT
    pub fn end_block(&mut self, input: &[&[f32]], frames: u32, advanced: bool) {
        if !self.rolling {
            return;
        }
        let frames = if self.stopped {
            match self.stop_at.take() {
                Some(k) => k.min(frames),
                None => return,
            }
        } else {
            frames
        };
        if frames == 0 {
            return;
        }
        let channels = &self.channels;
        if !channels.is_empty() {
            let pushed = self.ring.push(self.run, frames, |c, f| {
                channels
                    .get(c)
                    .and_then(|d| input.get(usize::from(*d)))
                    .and_then(|ch| ch.get(f))
                    .copied()
                    .unwrap_or(0.0)
            });
            if !pushed {
                self.status.lost_blocks.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.run = self.run.saturating_add(u64::from(frames));
        self.ring.set_end(self.run);
        self.status.run.store(self.run, Ordering::Release);
        if !advanced && !self.stopped {
            self.remark = true;
        }
    }
}
