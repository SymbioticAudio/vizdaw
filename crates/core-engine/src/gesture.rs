//! Live gestures (sub-project 5 spec §7.1, §7.2; plan 5e-1 Rulings 6, 10,
//! 11): the control side's messages, the renderer's table of touched
//! targets and (Task 6) the gesture tap. // RT (the table and the tap)
//!
//! The status says when the tap has rolled: until then the transport still reads stopped (plan 5e-1 Ruling 24).

use crate::record::{Begun, MARK_CAPACITY, MarkReader, RunClock};
use crate::schedule::automation::AutoDest;
use core_model::automation::{fader_db, fader_norm, unit};
use core_model::ids::PluginInstanceId;
use core_model::mixer::ParamId;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Capacity of the gesture queue (control → RT).
pub const GESTURE_CAPACITY: usize = 4096;
/// Most targets live at once in the renderer (Ruling 6).
pub const MAX_LIVE: usize = 1024;
const _: () = assert!(
    MAX_LIVE >= 1024,
    "Ruling 6: every control of 16 Write strips fits"
);
/// Most gesture messages the renderer takes per block.
pub const GESTURES_PER_BLOCK: usize = 256;
/// Ended plugin entries the table keeps between two
/// [`LiveTable::take_ended_plugins`]; more set its overflow flag.
const ENDED_PLUGINS: usize = 64;

/// What a gesture message does (spec §7.1; plan 5e-1 Rulings 6, 10, 11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GesturePhase {
    /// Grabbed, at this normalised value.
    Touch(f64),
    /// Moved to this normalised value.
    Value(f64),
    /// Moved by this much since the touch, added to the automation
    /// (Trim: dB for fader and send targets, normalised units otherwise).
    Offset(f64),
    /// Let go: the override ends at once (Read, Off, stopped).
    Release,
    /// Let go: the value glides back to the automation over this many frames (Touch, Trim).
    Glide(u32),
    /// Let go: the value is held until the transport stops or wraps (Latch, Write).
    Hold,
}

/// One gesture message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GestureMsg {
    /// What it moves.
    pub dest: AutoDest,
    /// What it does.
    pub phase: GesturePhase,
    /// When it happened, host nanoseconds; 0: at the block that takes it.
    pub host_ns: u64,
    /// The control side's tag, copied to the stamp (Task 6; its write mode).
    pub tag: u8,
}

/// Feeds the renderer's gesture queue. Cheap to clone.
///
/// Control-side threads only; never the RT thread.
#[derive(Clone)]
pub struct GestureSender {
    tx: Arc<Mutex<Producer<GestureMsg>>>,
    dropped: Arc<AtomicU64>,
}

impl std::fmt::Debug for GestureSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GestureSender")
    }
}

impl GestureSender {
    /// Queues `m`, waiting for another sender (control-side threads only; never the RT thread); false only when the queue is full (dropped and counted).
    pub fn send(&self, m: GestureMsg) -> bool {
        // Pre-flight I3: a blocking lock, so two senders at once never
        // drop. A sender that panicked while pushing left the producer
        // sound (`push` either wrote the slot or did not).
        let ok = self
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(m)
            .is_ok();
        if !ok {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        ok
    }

    /// Messages dropped so far.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The gesture queue: the sender and the renderer's end.
pub(crate) fn gesture_queue() -> (GestureSender, Consumer<GestureMsg>) {
    let (tx, rx) = RingBuffer::new(GESTURE_CAPACITY);
    (
        GestureSender {
            tx: Arc::new(Mutex::new(tx)),
            dropped: Arc::new(AtomicU64::new(0)),
        },
        rx,
    )
}

/// What a live target plays.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LiveState {
    /// Held by the hand at this normalised value.
    Touched(f64),
    /// Held by the hand at this offset over what is under it (Trim).
    Offset(f64),
    /// Let go, kept at this value until a Stop, a wrap or a Locate.
    Held(f64),
    /// Let go, kept at this offset until a Stop, a wrap or a Locate.
    HeldOffset(f64),
    /// Let go, gliding from `from` to what is under it.
    Gliding { from: f64, done: u32, frames: u32 },
    /// Let go, an offset gliding from `from` to 0.
    GlidingOffset { from: f64, done: u32, frames: u32 },
    /// Released (a strip or send target): returns to what is under it
    /// over one run, then ends (controller ruling on Task 5 fix round 1).
    Releasing,
}

/// Where a strip or send target's next run starts (Task 5 review I2):
/// each live change ramps over one run, as automation does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Start {
    /// From what is under it (a new entry: the value heard before).
    Under,
    /// From the value committed at the last run's end.
    Was(f64),
}

/// One live target.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Live {
    dest: AutoDest,
    state: LiveState,
    start: Start,
    /// What was last heard under it while a lane read it (NaN: nothing
    /// yet): offsets and glides play over it when its lane reads nothing
    /// (stopped, past the end; Task 5 review I1).
    base: f64,
}

/// A placeholder entry (past `len`, never read).
const EMPTY: Live = Live {
    dest: AutoDest::Plugin(PluginInstanceId(0), ParamId(0)),
    state: LiveState::Touched(0.0),
    start: Start::Under,
    base: f64::NAN,
};

/// The renderer's touched targets (Ruling 6), sorted by dest. // RT
#[derive(Debug)]
pub struct LiveTable {
    /// `MAX_LIVE` slots; the first `len` are live, sorted by `dest`.
    entries: Box<[Live]>,
    len: usize,
    /// An entry ended since the last `take_released`.
    released: bool,
    /// `Plugin` dests whose entries ended since the last
    /// `take_ended_plugins`.
    ended_plugins: [AutoDest; ENDED_PLUGINS],
    ended_len: usize,
    /// More plugin entries ended than `ended_plugins` holds.
    ended_overflow: bool,
}

impl Default for LiveTable {
    fn default() -> LiveTable {
        LiveTable::new()
    }
}

/// The fraction of a glide still to go `at` frames past `done`. // RT
fn left(done: u32, frames: u32, at: u32) -> f64 {
    if frames == 0 {
        return 0.0;
    }
    let x = (f64::from(done) + f64::from(at)) / f64::from(frames);
    1.0 - x.clamp(0.0, 1.0)
}

/// `under` moved by offset `d`: in dB for fader and send levels, else in
/// normalised units. // RT
fn offset(dest: AutoDest, under: f64, d: f64) -> f64 {
    if dest.is_db() {
        fader_norm(fader_db(under) + d)
    } else {
        unit(under + d)
    }
}

impl LiveTable {
    /// An empty table (allocates its slots: not RT).
    pub fn new() -> LiveTable {
        LiveTable {
            entries: vec![EMPTY; MAX_LIVE].into_boxed_slice(),
            len: 0,
            released: false,
            ended_plugins: [EMPTY.dest; ENDED_PLUGINS],
            ended_len: 0,
            ended_overflow: false,
        }
    }

    fn live(&self) -> &[Live] {
        self.entries.get(..self.len).unwrap_or(&[])
    }

    fn find(&self, dest: AutoDest) -> Result<usize, usize> {
        self.live().binary_search_by_key(&dest, |l| l.dest)
    }

    fn state(&self, dest: AutoDest) -> Option<LiveState> {
        let i = self.find(dest).ok()?;
        self.entries.get(i).map(|l| l.state)
    }

    /// Puts `state` at `dest`, inserting it at `at` (shifting the rest);
    /// false when the table is full. // RT
    fn insert(&mut self, at: usize, dest: AutoDest, state: LiveState) -> bool {
        if self.len >= self.entries.len() || at > self.len {
            return false;
        }
        self.entries.copy_within(at..self.len, at + 1);
        if let Some(e) = self.entries.get_mut(at) {
            *e = Live {
                dest,
                state,
                start: Start::Under,
                base: f64::NAN,
            };
        }
        self.len += 1;
        true
    }

    /// Notes that `dest`'s entry ended. // RT
    fn ended(&mut self, dest: AutoDest) {
        self.released = true;
        if !matches!(dest, AutoDest::Plugin(..)) {
            return;
        }
        match self.ended_plugins.get_mut(self.ended_len) {
            Some(slot) => {
                *slot = dest;
                self.ended_len += 1;
            }
            None => self.ended_overflow = true,
        }
    }

    /// Removes entry `i` (shifting the rest). // RT
    fn remove(&mut self, i: usize) {
        let Some(dest) = self.live().get(i).map(|l| l.dest) else {
            return;
        };
        self.entries.copy_within(i + 1..self.len, i);
        self.len -= 1;
        self.ended(dest);
    }

    /// Keeps the entries `keep` says to (it may change them) and ends the
    /// rest, in one pass (Task 5 review M1). // RT
    fn retain(&mut self, mut keep: impl FnMut(&mut Live) -> bool) {
        let mut w = 0;
        for r in 0..self.len {
            let Some(mut e) = self.entries.get(r).copied() else {
                break;
            };
            if keep(&mut e) {
                if let Some(slot) = self.entries.get_mut(w) {
                    *slot = e;
                }
                w += 1;
            } else {
                self.ended(e.dest);
            }
        }
        self.len = w;
    }

    /// Applies `phase` to `dest`; false when a touch found no free entry (dropped). // RT
    pub fn apply(&mut self, dest: AutoDest, phase: GesturePhase) -> bool {
        let found = self.find(dest);
        let touched = match phase {
            GesturePhase::Touch(v) | GesturePhase::Value(v) => Some(LiveState::Touched(unit(v))),
            GesturePhase::Offset(d) if d.is_finite() => Some(LiveState::Offset(d)),
            GesturePhase::Offset(_) => Some(LiveState::Offset(0.0)),
            _ => None,
        };
        if let Some(state) = touched {
            return match found {
                Ok(i) => {
                    if let Some(e) = self.entries.get_mut(i) {
                        e.state = state;
                    }
                    true
                }
                Err(at) => self.insert(at, dest, state),
            };
        }
        // An end with no entry does nothing.
        let Ok(i) = found else {
            return true;
        };
        let Some(state) = self.entries.get(i).map(|e| e.state) else {
            return true;
        };
        let next = match (phase, state) {
            // A plugin keeps the user's value until its automation is sent
            // again: it ends at once.
            (GesturePhase::Release | GesturePhase::Glide(0), _)
                if matches!(dest, AutoDest::Plugin(..)) =>
            {
                None
            }
            // A strip or send ramps back over one run, from the value its
            // last run ended at (`Start::Was`).
            (GesturePhase::Release | GesturePhase::Glide(0), _) => Some(LiveState::Releasing),
            (GesturePhase::Hold, LiveState::Touched(v) | LiveState::Held(v)) => {
                Some(LiveState::Held(v))
            }
            (GesturePhase::Hold, LiveState::Offset(d) | LiveState::HeldOffset(d)) => {
                Some(LiveState::HeldOffset(d))
            }
            (GesturePhase::Glide(n), LiveState::Touched(v) | LiveState::Held(v)) => {
                Some(LiveState::Gliding {
                    from: v,
                    done: 0,
                    frames: n,
                })
            }
            (GesturePhase::Glide(n), LiveState::Offset(d) | LiveState::HeldOffset(d)) => {
                Some(LiveState::GlidingOffset {
                    from: d,
                    done: 0,
                    frames: n,
                })
            }
            // Already let go and gliding: a later Hold or Glide leaves it.
            (_, s) => Some(s),
        };
        match next {
            Some(s) => {
                if let Some(e) = self.entries.get_mut(i) {
                    e.state = s;
                }
            }
            None => self.remove(i),
        }
        true
    }

    /// The value `dest` plays `at` frames into the run instead of `under` (its automation or static value); None when it is not live. // RT
    ///
    /// For a strip or send target, at 0 (the run's start commit) it is
    /// the value committed at the last run's end ([`LiveTable::note`]), or
    /// `under` for a new entry, so each change ramps over one run (Task 5
    /// review I2).
    pub fn value(&self, dest: AutoDest, under: f64, at: u32) -> Option<f64> {
        let e = self.find(dest).ok().and_then(|i| self.entries.get(i))?;
        if at == 0 && !matches!(dest, AutoDest::Plugin(..)) {
            return Some(match e.start {
                Start::Under => under,
                Start::Was(v) => v,
            });
        }
        Some(match e.state {
            LiveState::Touched(v) | LiveState::Held(v) => v,
            LiveState::Offset(d) | LiveState::HeldOffset(d) => offset(dest, under, d),
            LiveState::Gliding { from, done, frames } => {
                under + (from - under) * left(done, frames, at)
            }
            LiveState::GlidingOffset { from, done, frames } => {
                offset(dest, under, from * left(done, frames, at))
            }
            LiveState::Releasing => under,
        })
    }

    /// Notes what `dest`'s run ended at: `end`, the value committed, and
    /// (`under`) what its lane read under it. // RT
    pub fn note(&mut self, dest: AutoDest, end: f64, under: Option<f64>) {
        let Some(e) = self.find(dest).ok().and_then(|i| self.entries.get_mut(i)) else {
            return;
        };
        e.start = Start::Was(end);
        if let Some(u) = under {
            e.base = u;
        }
    }

    /// What was last heard under `dest` while its lane read it ([`LiveTable::note`]). // RT
    pub fn base(&self, dest: AutoDest) -> Option<f64> {
        let e = self.find(dest).ok().and_then(|i| self.entries.get(i))?;
        e.base.is_finite().then_some(e.base)
    }

    /// Remembers `under` as what is heard under `dest` (its lane read
    /// nothing yet: the control's current value). // RT
    pub fn set_base(&mut self, dest: AutoDest, under: f64) {
        if let Some(e) = self.find(dest).ok().and_then(|i| self.entries.get_mut(i)) {
            e.base = under;
        }
    }

    /// The value `dest`'s last run ended at, when it is being released
    /// (the value the hand left). // RT
    pub fn left_at(&self, dest: AutoDest) -> Option<f64> {
        let e = self.find(dest).ok().and_then(|i| self.entries.get(i))?;
        match (e.state, e.start) {
            (LiveState::Releasing, Start::Was(v)) => Some(v),
            _ => None,
        }
    }

    /// Whether `dest`'s entry has played no run yet (it starts from what
    /// is under it). // RT
    pub fn is_new(&self, dest: AutoDest) -> bool {
        self.find(dest)
            .ok()
            .and_then(|i| self.entries.get(i))
            .is_some_and(|e| e.start == Start::Under)
    }

    /// The destination of the `i`th live entry (by dest order). // RT
    pub fn dest_at(&self, i: usize) -> Option<AutoDest> {
        self.live().get(i).map(|l| l.dest)
    }

    /// How many targets are live. // RT
    pub fn len(&self) -> usize {
        self.len
    }

    /// No target is live. // RT
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether `dest` has an entry. // RT
    pub fn has(&self, dest: AutoDest) -> bool {
        self.find(dest).is_ok()
    }

    /// `dest` is touched or held: a plugin keeps the user's value, so no automation is sent to it. // RT
    pub fn holds(&self, dest: AutoDest) -> bool {
        matches!(
            self.state(dest),
            Some(LiveState::Touched(_) | LiveState::Held(_))
        )
    }

    /// Glides move on by `frames`; finished glides end. // RT
    pub fn advance(&mut self, frames: u32) {
        self.retain(|e| match &mut e.state {
            LiveState::Gliding {
                done, frames: n, ..
            }
            | LiveState::GlidingOffset {
                done, frames: n, ..
            } => {
                *done = done.saturating_add(frames);
                *done < *n
            }
            LiveState::Releasing => false,
            _ => true,
        });
    }

    /// Held and gliding entries end (a Stop, a loop wrap, a Locate). // RT
    pub fn end_holds(&mut self) {
        self.retain(|e| matches!(e.state, LiveState::Touched(_) | LiveState::Offset(_)));
    }

    /// Whether an entry ended since the last call (its override must go).
    /// The renderer drops it before its next run's commits; an entry that
    /// ends in `advance` lets go in the same `automate`, by the end-of-run
    /// release (Task 5 re-review N4). // RT
    pub fn take_released(&mut self) -> bool {
        std::mem::take(&mut self.released)
    }

    /// Whether an entry ended since the last `take_released`, without
    /// taking it. // RT
    pub fn released(&self) -> bool {
        self.released
    }

    /// Calls `f` with each `Plugin` dest whose entry ended since the last call, then forgets them (pre-flight I10); true when more ended than it kept (64), so every plugin lane must be reset. // RT
    pub fn take_ended_plugins(&mut self, mut f: impl FnMut(AutoDest)) -> bool {
        for d in self.ended_plugins.iter().take(self.ended_len) {
            f(*d);
        }
        self.ended_len = 0;
        std::mem::take(&mut self.ended_overflow)
    }

    /// The live entries' destinations. // RT
    pub fn dests(&self) -> impl Iterator<Item = AutoDest> + '_ {
        self.live().iter().map(|l| l.dest)
    }
}

/// Stamps a gesture tap can hold until the control side reads them.
pub const STAMP_CAPACITY: usize = 16_384;

/// A gesture as the renderer took it (Ruling 7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stamped {
    /// What it moves.
    pub dest: AutoDest,
    /// The output run frame it was heard at (graph latency not yet taken off), as `RecordedMidi.run`.
    ///
    /// A live value acts at once at its control, which renders the
    /// timeline its arrival behind the renderer (Ruling 5; Task 5 review
    /// M3): it is heard at the master the graph latency less that arrival
    /// later, so the placement (`S(run − L)`) puts it at the frame the
    /// control's lane reads, across a loop seam too.
    pub run: i64,
    /// What it did.
    pub phase: GesturePhase,
    /// The control side's tag ([`GestureMsg::tag`]).
    pub tag: u8,
}

/// What a gesture tap shares with the control side.
#[derive(Debug)]
pub struct GestureStatus {
    run: AtomicU64,
    rolled: AtomicBool,
    closed: AtomicBool,
    lost_marks: AtomicU64,
    lost_stamps: AtomicU64,
}

impl GestureStatus {
    /// Run frames played so far.
    pub fn run(&self) -> u64 {
        self.run.load(Ordering::Acquire)
    }

    /// The tap has pushed its roll mark (set with Release ordering when it does; pre-flight C1, Ruling 24).
    pub fn rolled(&self) -> bool {
        self.rolled.load(Ordering::Acquire)
    }

    /// The tap is gone: removed, or dropped with its renderer.
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Marks that did not fit.
    pub fn lost_marks(&self) -> u64 {
        self.lost_marks.load(Ordering::Relaxed)
    }

    /// Stamps that did not fit.
    pub fn lost_stamps(&self) -> u64 {
        self.lost_stamps.load(Ordering::Relaxed)
    }
}

/// The control side of a gesture tap.
#[derive(Debug)]
pub struct GestureHandles {
    /// Marks, oldest first.
    pub marks: MarkReader,
    /// Stamped gestures, oldest first.
    pub stamps: Consumer<Stamped>,
    /// Counters, the run and whether it rolled.
    pub status: Arc<GestureStatus>,
}

/// The real-time side of the gesture tap (Ruling 7), owned by the
/// renderer: the record tap's run clock and the stamps.
#[derive(Debug)]
pub struct GestureTap {
    clock: RunClock,
    stamps: Producer<Stamped>,
    status: Arc<GestureStatus>,
    /// The tap rolled this block: `rolled` is set once the block has
    /// published its transport state ([`GestureTap::published`]).
    roll_due: bool,
}

/// A tap and its control side, its marks carrying graph latency `latency`. Allocates; call off the RT thread.
pub fn gesture_tap(latency: u64) -> (GestureTap, GestureHandles) {
    let (mark_tx, mark_rx) = RingBuffer::new(MARK_CAPACITY);
    let (stamp_tx, stamp_rx) = RingBuffer::new(STAMP_CAPACITY);
    let status = Arc::new(GestureStatus {
        run: AtomicU64::new(0),
        rolled: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        lost_marks: AtomicU64::new(0),
        lost_stamps: AtomicU64::new(0),
    });
    (
        GestureTap {
            clock: RunClock::new(mark_tx, latency),
            stamps: stamp_tx,
            status: Arc::clone(&status),
            roll_due: false,
        },
        GestureHandles {
            marks: mark_rx,
            stamps: stamp_rx,
            status,
        },
    )
}

impl Drop for GestureTap {
    fn drop(&mut self) {
        self.status.closed.store(true, Ordering::Release);
    }
}

impl GestureTap {
    /// Counts a mark that did not fit. // RT
    fn count(&self, fit: bool) {
        if !fit {
            self.status.lost_marks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// At a block's start while playing (see `RecordTap::begin_block`);
    /// the roll sets the status's `rolled`. // RT
    pub fn begin_block(&mut self, frame: u64) {
        let (begun, fit) = self.clock.begin_block(frame);
        self.count(fit);
        if begun == Begun::Rolled {
            self.roll_due = true;
        }
    }

    /// After the block published its transport state: a roll this block
    /// sets the status's `rolled` now, so a control side that reads
    /// `rolled` and then the state never sees the roll with the state
    /// from before the Play. // RT
    pub fn published(&mut self) {
        if std::mem::take(&mut self.roll_due) {
            self.status.rolled.store(true, Ordering::Release);
        }
    }

    /// The transport left Playing at block offset `offset`, standing at
    /// `frame`. // RT
    pub fn stop(&mut self, offset: u32, frame: u64) {
        let fit = self.clock.stop(offset, frame);
        self.count(fit);
    }

    /// A loop wrap at block offset `offset` continues at `to`. // RT
    pub fn wrap(&mut self, offset: u32, to: u64) {
        let fit = self.clock.wrap(offset, to);
        self.count(fit);
    }

    /// The schedule changed or the transport jumped: the next block marks
    /// where it is. // RT
    pub fn resync(&mut self) {
        self.clock.resync();
    }

    /// The graph latency is `latency` from the next block on. // RT
    pub fn set_latency(&mut self, latency: u64) {
        self.clock.set_latency(latency);
    }

    /// At a block's end: advances the run over the frames that played. // RT
    pub fn end_block(&mut self, frames: u32, advanced: bool) {
        if self.clock.end_block(frames, advanced).is_some() {
            self.status.run.store(self.clock.run(), Ordering::Release);
        }
    }

    /// Stamps a message heard at block offset `offset` (negative: before the block), by `RecordTap::midi`'s rule.
    ///
    /// `ahead` is how long after the renderer its change is heard at the
    /// master: the graph latency less its control's arrival (Task 5
    /// review M3; see [`Stamped::run`]). // RT
    pub fn stamp(&mut self, dest: AutoDest, offset: i64, ahead: i64, phase: GesturePhase, tag: u8) {
        let Some(run) = self.clock.stamp_run(offset) else {
            return;
        };
        let run = run.saturating_add(ahead);
        if self
            .stamps
            .push(Stamped {
                dest,
                run,
                phase,
                tag,
            })
            .is_err()
        {
            self.status.lost_stamps.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The tap is removed: the control side reads it closed now, before
    /// the retire thread frees it. // RT
    pub fn close(&self) {
        self.status.closed.store(true, Ordering::Release);
    }
}
