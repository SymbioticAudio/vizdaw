//! The arpeggiator stage (plan 5g-1 Task 3; spec §6.8; Rulings 7, 9–11).
//!
//! An [`ArpState`] holds the keys played into it (at most [`MAX_HELD`]) and
//! plays them back one step at a time, synced to the transport's tick grid
//! while it plays and free-running at the project tempo while it is
//! stopped. Everything but [`ArpState::new`] runs on the real-time thread:
//! fixed-size arrays only, no allocation, free, lock or I/O.

use super::{At, ChainClock, LiveMsg, MAX_HELD, MAX_SOUNDING, up7};
use crate::schedule::TempoTable;
use core_model::input_fx::{Arp, ArpMode};
use core_model::time::{PPQ, Tick};

/// Ruling 9's xorshift seed.
const SEED: u32 = 0x2545_F491;

/// The longest pattern: every held note in every octave (Ruling 9).
const MAX_PATTERN: usize = MAX_HELD * 4;

/// An arpeggiator's running state.
pub(crate) struct ArpState {
    mode: ArpMode,
    /// Step length in ticks, at least 1.
    step: i64,
    /// Octaves spanned, 1–4.
    octaves: u8,
    /// Gate, percent of a step.
    gate: u16,
    /// Fixed velocity (7-bit), or `None` for As Played.
    fixed: Option<u8>,
    latch: bool,
    /// Held notes `(pitch, velocity)` in press order.
    held: [(u8, u16); MAX_HELD],
    held_len: usize,
    /// Keys physically down, one bit per pitch.
    down: u128,
    /// The step index (Ruling 9).
    index: usize,
    /// Sounding notes `(pitch, off_at)` on the chain's frame counter.
    sounding: [(u8, u64); MAX_SOUNDING],
    sounding_len: usize,
    /// The next free-running step on the chain's frame counter; `None`
    /// plays at once.
    next_free: Option<f64>,
    /// The earliest frame a step may play: the press that started the set.
    earliest: u64,
    /// Ruling 9's xorshift state.
    rng: u32,
    /// Notes dropped for [`MAX_SOUNDING`] since the last
    /// [`ArpState::take_dropped`].
    dropped: u64,
}

/// One playable step, on the chain's frame counter.
#[derive(Clone, Copy)]
struct Step {
    /// The step's frame, unrounded (free-running keeps the fraction).
    exact: f64,
    frame: u64,
    off_at: u64,
    /// Block offset.
    offset: u32,
}

impl ArpState {
    /// A fresh arpeggiator for `cfg`. Allocates; control thread only.
    pub(crate) fn new(cfg: &Arp) -> Box<ArpState> {
        Box::new(ArpState {
            mode: cfg.mode,
            step: cfg.rate.ticks().0.max(1),
            octaves: cfg.octaves.clamp(1, 4),
            gate: cfg.gate,
            fixed: cfg.velocity,
            latch: cfg.latch,
            held: [(0, 0); MAX_HELD],
            held_len: 0,
            down: 0,
            index: 0,
            sounding: [(0, 0); MAX_SOUNDING],
            sounding_len: 0,
            next_free: None,
            earliest: 0,
            rng: SEED,
            dropped: 0,
        })
    }

    /// Takes one message at block offset `at` of the block that starts at
    /// chain frame `now`. Notes change the held set and are not passed on
    /// (steps play them, in [`ArpState::tick`]); a note-on past
    /// [`MAX_HELD`] is dropped and counted in `dropped`. Per-note messages
    /// are dropped: the notes downstream are the arpeggiator's, not the
    /// keys' (Ruling 9). Channel messages pass at once. // RT
    pub(crate) fn input(
        &mut self,
        msg: LiveMsg,
        at: At,
        now: u64,
        dropped: &mut u64,
        emit: &mut dyn FnMut(At, LiveMsg),
    ) {
        match msg {
            LiveMsg::NoteOn {
                pitch, velocity, ..
            } => {
                let bit = 1u128 << (pitch & 0x7F);
                if self.down == 0 && self.latch {
                    // Latch: the first key with none down starts a new set.
                    self.held_len = 0;
                }
                if let Some(h) = self.held().iter().position(|(p, _)| *p == pitch) {
                    if let Some(slot) = self.held.get_mut(h) {
                        slot.1 = velocity;
                    }
                    self.down |= bit;
                    return;
                }
                let Some(slot) = self.held.get_mut(self.held_len) else {
                    *dropped = dropped.saturating_add(1);
                    return;
                };
                *slot = (pitch, velocity);
                if self.held_len == 0 {
                    self.start_set(now.saturating_add(u64::from(at.block)));
                }
                self.held_len += 1;
                self.down |= bit;
            }
            LiveMsg::NoteOff { pitch, .. } => {
                let bit = 1u128 << (pitch & 0x7F);
                if self.down & bit == 0 {
                    return;
                }
                self.down &= !bit;
                if self.latch {
                    return;
                }
                if let Some(h) = self.held().iter().position(|(p, _)| *p == pitch)
                    && let Some(rest) = self.held.get_mut(h..self.held_len)
                {
                    rest.rotate_left(1);
                    self.held_len -= 1;
                }
            }
            LiveMsg::PerNote { .. } => {}
            LiveMsg::Channel(_) => emit(at, msg),
        }
    }

    /// Plays this block's gate ends and steps in time order, offs before
    /// ons at equal offsets, so the emitted block offsets never decrease.
    /// `now` is the block's first frame on the chain's counter. // RT
    pub(crate) fn tick(
        &mut self,
        clock: &ChainClock<'_>,
        now: u64,
        emit: &mut dyn FnMut(At, LiveMsg),
    ) {
        let end = now.saturating_add(u64::from(clock.frames));
        let free_step = free_step_frames(self.step, clock);
        // Synced: the grid line to look at next, from the line at or
        // before the block's first tick (Ruling 10).
        let mut grid = clock
            .transport
            .map(|(table, f0)| table.frame_to_tick(f0).0.div_euclid(self.step));
        // At most one step per frame: bounded work at any tempo.
        let mut steps_left = clock.frames;
        loop {
            let step = if self.held_len == 0 || steps_left == 0 {
                None
            } else {
                match (clock.transport, grid.as_mut()) {
                    (Some((table, f0)), Some(k)) => self.next_synced(table, f0, now, end, k),
                    _ => self.next_free_running(now, end, free_step),
                }
            };
            let next_off = self.next_off().filter(|f| *f < end);
            match (next_off, step) {
                (Some(off), s) if s.is_none_or(|s| off <= s.frame) => {
                    self.end_due(off, now, clock, emit);
                }
                (_, Some(s)) => {
                    steps_left -= 1;
                    self.play(s, clock, emit);
                    if let Some(k) = grid.as_mut() {
                        *k = k.saturating_add(1);
                    }
                    // Stopping mid-hold goes on one step after this one.
                    self.next_free = Some(s.exact + free_step);
                }
                _ => break,
            }
        }
    }

    /// Note-offs at `at` for every sounding note, then forgets the held and
    /// latched notes. // RT
    pub(crate) fn flush(&mut self, at: At, emit: &mut dyn FnMut(At, LiveMsg)) {
        for (pitch, _) in self.sounding() {
            emit(at, LiveMsg::off(*pitch));
        }
        self.sounding_len = 0;
        self.held_len = 0;
        self.down = 0;
        self.index = 0;
        self.next_free = None;
    }

    /// Notes dropped for [`MAX_SOUNDING`] since the last call. // RT
    pub(crate) fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    /// The held notes. // RT
    fn held(&self) -> &[(u8, u16)] {
        self.held.get(..self.held_len).unwrap_or(&[])
    }

    /// The sounding notes. // RT
    fn sounding(&self) -> &[(u8, u64)] {
        self.sounding.get(..self.sounding_len).unwrap_or(&[])
    }

    /// A new held set starts at chain frame `frame`: the step index
    /// restarts and free-running plays at once (Rulings 9, 10). // RT
    fn start_set(&mut self, frame: u64) {
        self.index = 0;
        self.next_free = None;
        self.earliest = frame;
    }

    /// The next synced step in this block: the first grid line from `k` on
    /// whose frame falls in `[f0, f0 + frames)` and not before the set's
    /// first press. Lines before the block are skipped, never caught up
    /// (Ruling 10). // RT
    fn next_synced(
        &self,
        table: &TempoTable,
        f0: u64,
        now: u64,
        end: u64,
        k: &mut i64,
    ) -> Option<Step> {
        let block_end = f0.saturating_add(end.saturating_sub(now));
        loop {
            let tick = k.saturating_mul(self.step);
            let at = table.tick_to_frame(Tick(tick));
            if at >= block_end {
                return None;
            }
            let frame = now.saturating_add(at.saturating_sub(f0));
            if at < f0 || frame < self.earliest {
                *k = k.saturating_add(1);
                continue;
            }
            let gate = self.step.saturating_mul(i64::from(self.gate)) / 100;
            let off = table.tick_to_frame(Tick(tick.saturating_add(gate)));
            let off_at = now
                .saturating_add(off.saturating_sub(f0))
                .max(frame.saturating_add(1));
            return Some(Step {
                exact: frame as f64,
                frame,
                off_at,
                offset: block_offset(frame, now),
            });
        }
    }

    /// The next free-running step in this block (Ruling 10): the stored
    /// next step, or at once when it is past or not set. // RT
    fn next_free_running(&self, now: u64, end: u64, free_step: f64) -> Option<Step> {
        let start = self.earliest.max(now) as f64;
        let exact = self.next_free.map_or(start, |f| f.max(start));
        let frame = exact.round() as u64;
        if frame >= end {
            return None;
        }
        let gate = (free_step * f64::from(self.gate) / 100.0).round() as u64;
        Some(Step {
            exact,
            frame,
            off_at: frame.saturating_add(gate.max(1)),
            offset: block_offset(frame, now),
        })
    }

    /// The earliest pending gate end. // RT
    fn next_off(&self) -> Option<u64> {
        self.sounding().iter().map(|(_, off)| *off).min()
    }

    /// Ends every sounding note due at or before `frame`, at `frame`. // RT
    fn end_due(
        &mut self,
        frame: u64,
        now: u64,
        clock: &ChainClock<'_>,
        emit: &mut dyn FnMut(At, LiveMsg),
    ) {
        let at = generated(block_offset(frame, now), clock);
        let mut i = 0;
        while let Some((pitch, off)) = self.sounding().get(i).copied() {
            if off <= frame {
                emit(at, LiveMsg::off(pitch));
                self.remove_sounding(i);
            } else {
                i += 1;
            }
        }
    }

    /// Plays step `s`: the pattern's note for this step, or all of it for
    /// Chord. A pitch still sounding (gate over 100 %) is ended first on
    /// the same frame (Ruling 10). A note past [`MAX_SOUNDING`] is dropped
    /// and counted. // RT
    fn play(&mut self, s: Step, clock: &ChainClock<'_>, emit: &mut dyn FnMut(At, LiveMsg)) {
        let mut pattern = [(0u8, 0u16); MAX_PATTERN];
        let len = self.pattern(&mut pattern);
        if len == 0 {
            return;
        }
        let pick = match self.mode {
            ArpMode::Chord => None,
            ArpMode::Random => {
                let mut x = self.rng;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                self.rng = x;
                Some(x as usize % len)
            }
            mode => Some(pattern_position(mode, self.index, len)),
        };
        self.index = self.index.wrapping_add(1);
        let notes = pattern.get(..len).unwrap_or(&[]);
        let chosen = match pick {
            Some(j) => notes.get(j..=j).unwrap_or(&[]),
            None => notes,
        };
        let at = generated(s.offset, clock);
        for (pitch, _) in chosen {
            if let Some(i) = self.sounding().iter().position(|(p, _)| p == pitch) {
                emit(at, LiveMsg::off(*pitch));
                self.remove_sounding(i);
            }
        }
        for (pitch, velocity) in chosen {
            let Some(slot) = self.sounding.get_mut(self.sounding_len) else {
                self.dropped = self.dropped.saturating_add(1);
                continue;
            };
            *slot = (*pitch, s.off_at);
            self.sounding_len += 1;
            let velocity = self.fixed.map_or(*velocity, |v| up7(v.max(1)));
            emit(
                at,
                LiveMsg::NoteOn {
                    pitch: *pitch,
                    velocity,
                    expr: Default::default(),
                },
            );
        }
    }

    /// Removes sounding note `i`, keeping the order. // RT
    fn remove_sounding(&mut self, i: usize) {
        if let Some(rest) = self.sounding.get_mut(i..self.sounding_len) {
            rest.rotate_left(1);
            self.sounding_len -= 1;
        }
    }

    /// Fills `out` with the pattern `P` (Ruling 9): the held notes (by
    /// pitch, or in press order for As Played) in each octave, lowest
    /// first, pitches over 127 dropped. Returns its length. // RT
    fn pattern(&self, out: &mut [(u8, u16); MAX_PATTERN]) -> usize {
        let mut base = [(0u8, 0u16); MAX_HELD];
        for (slot, note) in base.iter_mut().zip(self.held()) {
            *slot = *note;
        }
        let base = base.get_mut(..self.held_len).unwrap_or(&mut []);
        if self.mode != ArpMode::AsPlayed {
            base.sort_unstable_by_key(|(p, _)| *p);
        }
        let mut len = 0;
        for octave in 0..self.octaves {
            for (pitch, velocity) in base.iter() {
                let Some(p) = pitch.checked_add(12 * octave).filter(|p| *p <= 127) else {
                    continue;
                };
                if let Some(slot) = out.get_mut(len) {
                    *slot = (p, *velocity);
                    len += 1;
                }
            }
        }
        len
    }
}

/// `frame`'s offset in the block starting at `now`. // RT
fn block_offset(frame: u64, now: u64) -> u32 {
    u32::try_from(frame.saturating_sub(now)).unwrap_or(u32::MAX)
}

/// Where a generated output goes: thru at `offset`, the recorder at
/// `offset` plus the graph latency (Ruling 11). // RT
fn generated(offset: u32, clock: &ChainClock<'_>) -> At {
    At {
        block: offset,
        record: i64::from(offset).saturating_add(clock.record_lead),
    }
}

/// A free-running step in frames at the clock's tempo,
/// `step × 60 × rate ÷ (bpm × PPQ)` (Ruling 10), at least one frame. // RT
fn free_step_frames(step: i64, clock: &ChainClock<'_>) -> f64 {
    let bpm = if clock.bpm.is_finite() && clock.bpm >= 1.0 {
        clock.bpm
    } else {
        120.0
    };
    (step as f64 * 60.0 * clock.sample_rate / (bpm * PPQ as f64)).max(1.0)
}

/// The pattern position step `index` plays over a pattern of `len ≥ 1`
/// notes (Ruling 9): Up and As Played walk it, Down walks it backwards,
/// Up-Down and Down-Up bounce without doubling the ends. // RT
fn pattern_position(mode: ArpMode, index: usize, len: usize) -> usize {
    let last = len.saturating_sub(1);
    let bounce = |i: usize| {
        if last == 0 {
            return 0;
        }
        let j = i % (2 * last);
        if j <= last { j } else { 2 * last - j }
    };
    match mode {
        ArpMode::Down => last - index % len,
        ArpMode::UpDown => bounce(index),
        ArpMode::DownUp => last - bounce(index),
        _ => index % len,
    }
}
