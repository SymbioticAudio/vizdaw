//! The MIDI input effect chain (spec §6.8; plan 5g-1 Rulings 4–8, 12):
//! live input as MIDI 2.0-shaped [`LiveMsg`]s, a lane's [`InputChain`] of
//! built-in effects, and the output stage that merges duplicate pitches.
//!
//! Everything reachable from [`InputChain::process`], [`InputChain::tick`],
//! [`InputChain::flush`] and the other `// RT` items runs on the real-time
//! thread: no allocation, free, lock or I/O. Only [`InputChain::new`] (and
//! `Default`, `Clone`) allocate, on the control thread.

mod arp;

use crate::input::on_channel;
use crate::midi1::{Midi1Msg, note_on_velocity, release_velocity, value7};
use crate::schedule::TempoTable;
use arp::ArpState;
use core_model::input_fx::{InputFx, InputFxKind, MAX_CHORD_NOTES, MAX_INPUT_FX, VelocityFx};
use core_model::lane::QuantizeSpec;
use core_model::time::Tick;

/// 32-bit centre (pitch bend, timbre).
pub const CENTER: u32 = 0x8000_0000;
/// Notes an arpeggiator holds (Ruling 7).
pub const MAX_HELD: usize = 32;
/// Notes an arpeggiator sounds (Ruling 7).
pub const MAX_SOUNDING: usize = 128;
/// Outputs per input event or arpeggiator step (Ruling 7).
pub const MAX_FANOUT: usize = 128;

/// Stage visits one input event or step may spend (Ruling 7's work bound,
/// `MAX_INPUT_FX × 7 × MAX_FANOUT`). Leaves are capped by [`MAX_FANOUT`];
/// this also caps paths whose notes are all dropped for range, which take
/// no leaf budget.
const MAX_WORK: usize = MAX_INPUT_FX * (1 + MAX_CHORD_NOTES) * MAX_FANOUT;

/// A per-note expression dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expr {
    /// Per-note pitch bend.
    Bend,
    /// Per-note (poly) pressure.
    Pressure,
    /// Per-note timbre (MPE's CC 74).
    Timbre,
}

/// A note's expression at its start (5g-2 fills it from MPE; defaults
/// otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteExpr {
    /// 32-bit pitch bend, [`CENTER`] for none.
    pub bend: u32,
    /// 32-bit pressure.
    pub pressure: u32,
    /// 32-bit timbre, [`CENTER`] for none.
    pub timbre: u32,
}

impl Default for NoteExpr {
    fn default() -> Self {
        NoteExpr {
            bend: CENTER,
            pressure: 0,
            timbre: CENTER,
        }
    }
}

/// A live input event (Ruling 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveMsg {
    /// A note start with 16-bit velocity.
    NoteOn {
        /// 0–127.
        pitch: u8,
        /// 16-bit velocity, never 0.
        velocity: u16,
        /// Expression at the start.
        expr: NoteExpr,
    },
    /// A note end with 16-bit release velocity.
    NoteOff {
        /// 0–127.
        pitch: u8,
        /// 16-bit release velocity.
        velocity: u16,
    },
    /// A per-note message addressed by pitch.
    PerNote {
        /// 0–127.
        pitch: u8,
        /// Which dimension.
        expr: Expr,
        /// 32-bit value.
        value: u32,
    },
    /// Every other channel message, as received.
    Channel(Midi1Msg),
}

/// MIDI 2.0 min-center-max up-scaling of a 7-bit value to 16 bits.
pub fn up7(v: u8) -> u16 {
    let v = u16::from(v & 0x7F);
    let shifted = v << 9;
    if v <= 64 {
        return shifted;
    }
    let rep = v & 0x3F;
    shifted | (rep << 3) | (rep >> 3)
}

/// MIDI 2.0 min-center-max up-scaling of a `bits`-bit value to 32 bits
/// (`bits` 1–32; 0 gives 0; 32 or more gives `v`).
pub fn up32(v: u32, bits: u32) -> u32 {
    if bits == 0 {
        return 0;
    }
    if bits >= 32 {
        return v;
    }
    let v = v & ((1u32 << bits) - 1);
    let scale = 32 - bits;
    let shifted = v << scale;
    if v <= 1u32 << (bits - 1) {
        return shifted;
    }
    let repeat_bits = bits - 1;
    if repeat_bits == 0 {
        return shifted;
    }
    let mut rep = v & ((1u32 << repeat_bits) - 1);
    rep = if scale > repeat_bits {
        rep << (scale - repeat_bits)
    } else {
        rep >> (repeat_bits - scale)
    };
    let mut out = shifted;
    while rep != 0 {
        out |= rep;
        rep >>= repeat_bits;
    }
    out
}

impl LiveMsg {
    /// Decodes a MIDI 1.0 channel message (Ruling 4). // RT
    pub fn from_midi1(m: Midi1Msg) -> LiveMsg {
        let [s, d1, d2] = m.bytes;
        if m.len < 3 {
            return LiveMsg::Channel(m);
        }
        let pitch = d1 & 0x7F;
        let d2 = d2 & 0x7F;
        match s & 0xF0 {
            0x90 if d2 > 0 => LiveMsg::NoteOn {
                pitch,
                velocity: up7(d2),
                expr: NoteExpr::default(),
            },
            0x80 | 0x90 => LiveMsg::NoteOff {
                pitch,
                velocity: up7(d2),
            },
            0xA0 => LiveMsg::PerNote {
                pitch,
                expr: Expr::Pressure,
                value: up32(u32::from(d2), 7),
            },
            _ => LiveMsg::Channel(m),
        }
    }

    /// Encodes as MIDI 1.0 on `channel` (0–15); `None` for per-note bend
    /// and timbre, which have no MIDI 1.0 form. System messages keep their
    /// status. // RT
    pub fn to_midi1(&self, channel: u8) -> Option<Midi1Msg> {
        let ch = channel & 0x0F;
        match *self {
            LiveMsg::NoteOn {
                pitch, velocity, ..
            } => Some(Midi1Msg::three(
                0x90 | ch,
                pitch & 0x7F,
                note_on_velocity(velocity),
            )),
            LiveMsg::NoteOff { pitch, velocity } => Some(Midi1Msg::three(
                0x80 | ch,
                pitch & 0x7F,
                release_velocity(velocity),
            )),
            LiveMsg::PerNote {
                pitch,
                expr: Expr::Pressure,
                value,
            } => Some(Midi1Msg::three(0xA0 | ch, pitch & 0x7F, value7(value))),
            LiveMsg::PerNote { .. } => None,
            LiveMsg::Channel(m) => {
                let status = m.bytes[0];
                Some(if (0x80..0xF0).contains(&status) {
                    on_channel(m, ch)
                } else {
                    m
                })
            }
        }
    }

    /// A note-on at 7-bit velocity `v` (up-scaled) with default expression.
    pub fn on(pitch: u8, v: u8) -> LiveMsg {
        LiveMsg::NoteOn {
            pitch,
            velocity: up7(v),
            expr: NoteExpr::default(),
        }
    }

    /// A note-off with release velocity 0.
    pub fn off(pitch: u8) -> LiveMsg {
        LiveMsg::NoteOff { pitch, velocity: 0 }
    }

    /// The pitch of a note or per-note message. // RT
    pub fn pitch(&self) -> Option<u8> {
        match *self {
            LiveMsg::NoteOn { pitch, .. }
            | LiveMsg::NoteOff { pitch, .. }
            | LiveMsg::PerNote { pitch, .. } => Some(pitch),
            LiveMsg::Channel(_) => None,
        }
    }

    /// The same message at `pitch` (channel messages unchanged). // RT
    pub fn with_pitch(self, pitch: u8) -> LiveMsg {
        match self {
            LiveMsg::NoteOn { velocity, expr, .. } => LiveMsg::NoteOn {
                pitch,
                velocity,
                expr,
            },
            LiveMsg::NoteOff { velocity, .. } => LiveMsg::NoteOff { pitch, velocity },
            LiveMsg::PerNote { expr, value, .. } => LiveMsg::PerNote { pitch, expr, value },
            LiveMsg::Channel(m) => LiveMsg::Channel(m),
        }
    }
}

/// Where an output goes: thru at block offset `block`; the recorder at
/// `record` (Ruling 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct At {
    /// Frame offset in the block, for thru.
    pub block: u32,
    /// Offset for the recorder.
    pub record: i64,
}

impl At {
    /// The block's first frame, for both.
    pub const NOW: At = At {
        block: 0,
        record: 0,
    };
}

/// The block a tick covers (Ruling 10).
pub struct ChainClock<'a> {
    /// Frames in the block.
    pub frames: u32,
    /// Frames per second.
    pub sample_rate: f64,
    /// The tempo while stopped (free-running), BPM.
    pub bpm: f64,
    /// While playing: the schedule's tempo map and the block's first
    /// schedule frame.
    pub transport: Option<(&'a TempoTable, u64)>,
    /// Added to a generated output's block offset for the recorder: the
    /// graph latency in frames (Ruling 11).
    pub record_lead: i64,
}

/// The Input Quantize a recorded message carries (Ruling 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordQuantize {
    /// Grid in ticks.
    pub grid: i64,
    /// Strength, percent.
    pub strength: u8,
}

impl RecordQuantize {
    /// `QuantizeSpec::grid(Tick(grid), strength)`.
    pub fn spec(self) -> QuantizeSpec {
        QuantizeSpec::grid(Tick(self.grid), self.strength)
    }
}

/// One running stage of a chain.
enum Stage {
    /// An unused slot.
    Empty,
    Transpose(i8),
    Velocity(VelocityFx),
    /// Intervals and how many are used.
    Chord([i8; MAX_CHORD_NOTES], u8),
    Arp(Box<ArpState>),
}

/// The outputs of one stage for one message.
struct Outs {
    msgs: [LiveMsg; 1 + MAX_CHORD_NOTES],
    len: usize,
}

impl Outs {
    fn new() -> Outs {
        Outs {
            msgs: [LiveMsg::off(0); 1 + MAX_CHORD_NOTES],
            len: 0,
        }
    }

    fn push(&mut self, m: LiveMsg) {
        if let Some(slot) = self.msgs.get_mut(self.len) {
            *slot = m;
            self.len += 1;
        }
    }

    fn as_slice(&self) -> &[LiveMsg] {
        self.msgs.get(..self.len).unwrap_or(&[])
    }
}

/// `pitch` moved by `by`, if it stays in 0–127 (Ruling 8). // RT
fn shifted(pitch: u8, by: i8) -> Option<u8> {
    u8::try_from(i16::from(pitch) + i16::from(by))
        .ok()
        .filter(|p| *p <= 127)
}

/// The Velocity effect on a 16-bit note-on velocity (Ruling 14). // RT
fn scale_velocity(v: u16, f: &VelocityFx) -> u16 {
    if let Some(x) = f.fixed {
        return up7(x.max(1));
    }
    let lo = i64::from(up7(f.min.max(1)));
    let hi = i64::from(up7(f.max)).max(lo);
    let raw = i64::from(v) * i64::from(f.scale) / 100 + i64::from(f.offset) * 512;
    u16::try_from(raw.max(lo).min(hi)).unwrap_or(u16::MAX)
}

impl Stage {
    /// What this stage makes of `msg`. // RT
    fn map(&self, msg: LiveMsg, outs: &mut Outs) {
        match self {
            Stage::Transpose(by) => match msg.pitch() {
                Some(p) => {
                    if let Some(q) = shifted(p, *by) {
                        outs.push(msg.with_pitch(q));
                    }
                }
                None => outs.push(msg),
            },
            Stage::Velocity(f) => match msg {
                LiveMsg::NoteOn {
                    pitch,
                    velocity,
                    expr,
                } => outs.push(LiveMsg::NoteOn {
                    pitch,
                    velocity: scale_velocity(velocity, f),
                    expr,
                }),
                other => outs.push(other),
            },
            Stage::Chord(intervals, n) => match msg.pitch() {
                Some(p) => {
                    outs.push(msg);
                    for by in intervals.iter().take(usize::from(*n)) {
                        if let Some(q) = shifted(p, *by) {
                            outs.push(msg.with_pitch(q));
                        }
                    }
                }
                None => outs.push(msg),
            },
            // `run` gives the arpeggiator its input (it needs the clock).
            Stage::Arp(_) | Stage::Empty => outs.push(msg),
        }
    }
}

/// The running state of one event's (or step's) pass through the stages.
struct Cx<'a> {
    /// Leaves left (Ruling 7).
    budget: usize,
    /// Stage visits left.
    work: usize,
    /// The cap cut this event short.
    truncated: bool,
    /// `run` calls.
    visits: u64,
    /// The block's first frame on the chain's counter.
    now: u64,
    /// Notes the arpeggiators dropped for `MAX_HELD`.
    held_dropped: u64,
    /// The output stage's per-pitch note-on counts (Ruling 6).
    counts: &'a mut [u8; 128],
    out: &'a mut dyn FnMut(At, LiveMsg),
}

/// The output stage (Ruling 6). // RT
fn merge(cx: &mut Cx<'_>, msg: LiveMsg, at: At) {
    match msg {
        LiveMsg::NoteOn { pitch, .. } => {
            let Some(c) = cx.counts.get_mut(usize::from(pitch)) else {
                return;
            };
            let first = *c == 0;
            *c = c.saturating_add(1);
            if first {
                (cx.out)(at, msg);
            }
        }
        LiveMsg::NoteOff { pitch, .. } => {
            let Some(c) = cx.counts.get_mut(usize::from(pitch)) else {
                return;
            };
            if *c == 0 {
                return;
            }
            *c = c.saturating_sub(1);
            if *c == 0 {
                (cx.out)(at, msg);
            }
        }
        LiveMsg::PerNote { .. } | LiveMsg::Channel(_) => (cx.out)(at, msg),
    }
}

/// Runs `msg` through `stages`, then the output stage. Recursion depth is
/// at most `MAX_INPUT_FX + 1`. // RT
fn run(stages: &mut [Stage], msg: LiveMsg, at: At, cx: &mut Cx<'_>) {
    if cx.truncated {
        return;
    }
    cx.visits += 1;
    let Some((stage, rest)) = stages.split_first_mut() else {
        if cx.budget == 0 {
            cx.truncated = true;
            return;
        }
        cx.budget -= 1;
        merge(cx, msg, at);
        return;
    };
    if cx.work == 0 {
        cx.truncated = true;
        return;
    }
    cx.work -= 1;
    let mut outs = Outs::new();
    match stage {
        Stage::Arp(arp) => arp.input(msg, at, cx.now, &mut cx.held_dropped, &mut |_, m| {
            outs.push(m)
        }),
        other => other.map(msg, &mut outs),
    }
    for m in outs.as_slice() {
        if cx.truncated {
            break;
        }
        run(rest, *m, at, cx);
    }
}

/// A chain's running state: one allocation, made on the control thread.
struct ChainState {
    stages: [Stage; MAX_INPUT_FX],
    len: usize,
    /// The output stage's per-pitch counts (Ruling 6).
    counts: [u8; 128],
    /// Chain-local frames ticked so far.
    frame: u64,
    dropped: u64,
    visits: u64,
}

/// Runs messages through `stages` and the merge, one budget per event
/// (Ruling 7). An arpeggiator's tick output is grouped into events as it
/// arrives: a run of messages at one block offset that are all note-ons
/// (a step) or all note-offs (gate ends, and a step's retriggered notes)
/// is one event. The arpeggiator sends a step's retrigger offs before all
/// of its ons, so a step's ons are one event. No buffer: each message runs
/// as it is emitted.
struct Events<'a> {
    stages: &'a mut [Stage],
    cx: Cx<'a>,
    /// The open event's offset and kind (note-on or not).
    open: Option<(At, bool)>,
    /// Events the cap truncated.
    truncated: u64,
}

impl<'a> Events<'a> {
    fn new(
        stages: &'a mut [Stage],
        counts: &'a mut [u8; 128],
        now: u64,
        out: &'a mut dyn FnMut(At, LiveMsg),
    ) -> Events<'a> {
        Events {
            stages,
            cx: Cx {
                budget: MAX_FANOUT,
                work: MAX_WORK,
                truncated: false,
                visits: 0,
                counts,
                out,
                now,
                held_dropped: 0,
            },
            open: None,
            truncated: 0,
        }
    }

    /// Runs `msg` as part of the open event, opening a new one when its
    /// offset or kind differs. // RT
    fn push(&mut self, at: At, msg: LiveMsg) {
        let key = (at, matches!(msg, LiveMsg::NoteOn { .. }));
        if self.open != Some(key) {
            self.close();
            self.open = Some(key);
        }
        run(self.stages, msg, at, &mut self.cx);
    }

    /// Ends the open event: counts a truncation and renews the budget.
    /// // RT
    fn close(&mut self) {
        if self.cx.truncated {
            self.truncated = self.truncated.saturating_add(1);
        }
        self.cx.budget = MAX_FANOUT;
        self.cx.work = MAX_WORK;
        self.cx.truncated = false;
    }

    /// Closes the open event; returns `(dropped, visits)`. // RT
    fn finish(mut self) -> (u64, u64) {
        self.close();
        (
            self.truncated.saturating_add(self.cx.held_dropped),
            self.cx.visits,
        )
    }
}

impl ChainState {
    /// Runs one input event with a fresh budget. // RT
    fn run_event(&mut self, msg: LiveMsg, at: At, out: &mut dyn FnMut(At, LiveMsg)) {
        let stages = self.stages.get_mut(..self.len).unwrap_or(&mut []);
        let mut events = Events::new(stages, &mut self.counts, self.frame, out);
        run(events.stages, msg, at, &mut events.cx);
        let (dropped, visits) = events.finish();
        self.dropped = self.dropped.saturating_add(dropped);
        self.visits = self.visits.saturating_add(visits);
    }
}

/// A lane's input effect chain.
pub struct InputChain {
    config: Box<[InputFx]>,
    state: Box<ChainState>,
}

impl InputChain {
    /// Builds the chain for `fx` (at most [`MAX_INPUT_FX`] effects run).
    /// Allocates; control thread only.
    pub fn new(fx: &[InputFx]) -> InputChain {
        let mut stages: [Stage; MAX_INPUT_FX] = std::array::from_fn(|_| Stage::Empty);
        let mut len = 0;
        let running = fx
            .iter()
            .filter(|f| !f.bypass)
            .filter_map(|f| match &f.kind {
                InputFxKind::Transpose { semitones } => Some(Stage::Transpose(*semitones)),
                InputFxKind::Velocity(v) => Some(Stage::Velocity(*v)),
                InputFxKind::Chord { intervals } => {
                    let mut ints = [0i8; MAX_CHORD_NOTES];
                    let mut n = 0u8;
                    for (slot, i) in ints.iter_mut().zip(intervals) {
                        *slot = *i;
                        n += 1;
                    }
                    Some(Stage::Chord(ints, n))
                }
                InputFxKind::Arpeggiator(a) => Some(Stage::Arp(ArpState::new(a))),
                InputFxKind::InputQuantize { .. } => None,
            });
        for (slot, stage) in stages.iter_mut().zip(running) {
            *slot = stage;
            len += 1;
        }
        InputChain {
            config: fx.into(),
            state: Box::new(ChainState {
                stages,
                len,
                counts: [0; 128],
                frame: 0,
                dropped: 0,
                visits: 0,
            }),
        }
    }

    /// The effects as configured.
    pub fn config(&self) -> &[InputFx] {
        &self.config
    }

    /// Runs one input message through the chain. // RT
    pub fn process(&mut self, msg: LiveMsg, at: At, out: &mut dyn FnMut(At, LiveMsg)) {
        self.state.run_event(msg, at, out);
    }

    /// Advances the chain's generators over one block (Ruling 10). // RT
    pub fn tick(&mut self, clock: &ChainClock<'_>, out: &mut dyn FnMut(At, LiveMsg)) {
        let st = &mut *self.state;
        let now = st.frame;
        let stages = st.stages.get_mut(..st.len).unwrap_or(&mut []);
        for i in 0..stages.len() {
            // The arpeggiator at `i` and the stages after it are disjoint
            // borrows, so its output runs downstream as it is emitted.
            let Some((head, rest)) = stages.split_at_mut_checked(i + 1) else {
                break;
            };
            let Some(Stage::Arp(arp)) = head.last_mut() else {
                continue;
            };
            let mut events = Events::new(rest, &mut st.counts, now, out);
            arp.tick(clock, now, &mut |at, m| events.push(at, m));
            let (dropped, visits) = events.finish();
            st.dropped = st
                .dropped
                .saturating_add(dropped)
                .saturating_add(arp.take_dropped());
            st.visits = st.visits.saturating_add(visits);
        }
        st.frame = st.frame.wrapping_add(u64::from(clock.frames));
    }

    /// Note-offs at `at` for every note the chain sounds (ascending pitch),
    /// then forgets held and latched notes. // RT
    pub fn flush(&mut self, at: At, out: &mut dyn FnMut(At, LiveMsg)) {
        // Every note an arpeggiator sounds reached the merge's counts (or
        // was dropped downstream), so the sweep below ends it; the
        // arpeggiators' own offs are not needed.
        for stage in self.state.stages.iter_mut() {
            if let Stage::Arp(arp) = stage {
                arp.flush(at, &mut |_, _| {});
            }
        }
        for (pitch, c) in (0u8..=127).zip(self.state.counts.iter_mut()) {
            if *c > 0 {
                *c = 0;
                out(at, LiveMsg::off(pitch));
            }
        }
    }

    /// Takes `old`'s running state when the configs are equal (Ruling 5);
    /// else nothing. A swap: no allocation or free. // RT
    pub fn carry_from(&mut self, old: &mut InputChain) {
        if self.config == old.config {
            std::mem::swap(&mut self.state, &mut old.state);
        }
    }

    /// The first Input Quantize that is not bypassed. // RT
    pub fn quantize(&self) -> Option<RecordQuantize> {
        self.config
            .iter()
            .filter(|f| !f.bypass)
            .find_map(|f| match f.kind {
                InputFxKind::InputQuantize { grid, strength } => Some(RecordQuantize {
                    grid: grid.0,
                    strength,
                }),
                _ => None,
            })
    }

    /// Notes dropped for capacity since the last call (Ruling 7). // RT
    pub fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.state.dropped)
    }

    /// Stage calls since the last call: the work bound's probe (Ruling 7).
    /// // RT
    #[doc(hidden)]
    pub fn take_visits(&mut self) -> u64 {
        std::mem::take(&mut self.state.visits)
    }
}

impl Default for InputChain {
    /// The empty chain.
    fn default() -> Self {
        InputChain::new(&[])
    }
}

impl Clone for InputChain {
    /// The config cloned with fresh state (control thread).
    fn clone(&self) -> Self {
        InputChain::new(&self.config)
    }
}

impl PartialEq for InputChain {
    /// Configs only.
    fn eq(&self, other: &Self) -> bool {
        self.config == other.config
    }
}

impl Eq for InputChain {}

impl std::fmt::Debug for InputChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputChain")
            .field("config", &self.config)
            .finish()
    }
}
