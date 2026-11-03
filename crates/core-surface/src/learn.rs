//! MIDI learn: detecting a control and driving its target (spec §8.2,
//! §8.4; plan 5f-1 Rulings 7, 8, 17 and 18).
//!
//! [`Learner`] watches the messages after a learn starts and decides what
//! control sent them. [`Mapper`] turns the messages of learned controls into
//! gestures (holder `Learn`, released [`IDLE_RELEASE`](crate::IDLE_RELEASE)
//! after the last value), runs learned commands, says which messages it
//! takes, and sends absolute maps' values back.

use crate::idle::IdleTouches;
use crate::midi::split;
use crate::{Phase, SurfaceAction};
use core_model::automation::AutomationTarget;
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How long a learn collects messages after the first (Ruling 7).
pub const LEARN_WINDOW: Duration = Duration::from_millis(300);

/// How near the target's value a picking-up control must come (Ruling 18).
pub const PICKUP_TOLERANCE: f64 = 0.02;

/// The most messages a learn collects before it decides (Ruling 7).
const LEARN_MESSAGES: usize = 8;

/// The largest 14-bit value.
const MAX14: f64 = 16383.0;

/// A learned command (app preferences, Ruling 9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandMap {
    /// Where its messages come from.
    pub source: LearnSource,
    /// The registry ID it runs.
    pub command: String,
}

/// A channel message a learn or a map can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Msg {
    /// A controller change.
    Cc { ch: u8, cc: u8, v: u8 },
    /// A pitch bend, 14-bit.
    Bend { ch: u8, v: u16 },
    /// A note-on with a velocity above 0.
    On { ch: u8, note: u8, vel: u8 },
    /// A note-off (or a note-on at velocity 0).
    Off { ch: u8, note: u8 },
}

/// The message in `m` (three bytes), if it is a CC, a bend or a note.
fn parse(m: &[u8]) -> Option<Msg> {
    let [status, d1, d2] = *m else { return None };
    if d1 > 0x7F || d2 > 0x7F {
        return None;
    }
    let ch = status & 0x0F;
    match status & 0xF0 {
        0x80 => Some(Msg::Off { ch, note: d1 }),
        0x90 if d2 == 0 => Some(Msg::Off { ch, note: d1 }),
        0x90 => Some(Msg::On {
            ch,
            note: d1,
            vel: d2,
        }),
        0xB0 => Some(Msg::Cc { ch, cc: d1, v: d2 }),
        0xE0 => Some(Msg::Bend {
            ch,
            v: u16::from(d2) << 7 | u16::from(d1),
        }),
        _ => None,
    }
}

/// Every whole message in `bytes` that [`parse`] understands.
fn messages(bytes: &[u8]) -> impl Iterator<Item = Msg> {
    split(bytes)
        .into_iter()
        .filter_map(|m| m.ok().and_then(|m| parse(&m)))
}

/// What a message is to a learned kind.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Part {
    /// A 7-bit controller value.
    Cc(u8),
    /// The MSB of a 14-bit pair.
    Msb(u8),
    /// The LSB of a 14-bit pair.
    Lsb(u8),
    /// A 14-bit pitch bend.
    Bend(u16),
    /// A note-on at this velocity.
    On(u8),
    /// A note-off.
    Off,
}

/// What `m` is to `kind`, or `None` when `kind` does not take it (Ruling 17).
fn part(kind: LearnKind, m: Msg) -> Option<Part> {
    match (kind, m) {
        (LearnKind::Cc7 { channel, cc }, Msg::Cc { ch, cc: c, v }) if ch == channel && c == cc => {
            Some(Part::Cc(v))
        }
        (LearnKind::Cc14 { channel, cc }, Msg::Cc { ch, cc: c, v }) if ch == channel => {
            if c == cc {
                Some(Part::Msb(v))
            } else if Some(c) == cc.checked_add(32) {
                Some(Part::Lsb(v))
            } else {
                None
            }
        }
        (LearnKind::PitchBend { channel }, Msg::Bend { ch, v }) if ch == channel => {
            Some(Part::Bend(v))
        }
        (LearnKind::Note { channel, note }, Msg::On { ch, note: n, vel })
            if ch == channel && n == note =>
        {
            Some(Part::On(vel))
        }
        (LearnKind::Note { channel, note }, Msg::Off { ch, note: n })
            if ch == channel && n == note =>
        {
            Some(Part::Off)
        }
        _ => None,
    }
}

/// A 14-bit value, normalised.
fn norm14(v: u16) -> f64 {
    f64::from(v) / MAX14
}

/// `v` (normalised) on `0..=max`, rounded.
fn scale(v: f64, max: u16) -> u16 {
    // `as` saturates; `v` is clamped and never NaN here.
    (v.clamp(0.0, 1.0) * f64::from(max)).round() as u16
}

/// Detects the next control (Ruling 7): after the first CC, pitch bend or
/// note-on, it collects that port's messages for [`LEARN_WINDOW`] or until
/// eight have arrived.
#[derive(Debug, Default)]
pub struct Learner {
    /// The port of the first message.
    port: Option<String>,
    /// When the first message arrived.
    first: Option<Instant>,
    /// The collected CCs, bends and note-ons, at most eight.
    msgs: Vec<Msg>,
}

impl Learner {
    /// One message from `port` (channel messages only; others ignored).
    pub fn feed(&mut self, port: &str, msg: &[u8], now: Instant) {
        for m in messages(msg) {
            if matches!(m, Msg::Off { .. }) || self.msgs.len() >= LEARN_MESSAGES {
                continue;
            }
            match &self.port {
                None => {
                    self.port = Some(port.to_string());
                    self.first = Some(now);
                }
                Some(p) if p != port => continue,
                Some(_) => {}
            }
            self.msgs.push(m);
        }
    }

    /// The decision once LEARN_WINDOW passed since the first message or 8 arrived (Ruling 7).
    pub fn decide(&mut self, now: Instant) -> Option<(LearnSource, LearnMode)> {
        let first = self.first?;
        if self.msgs.len() < LEARN_MESSAGES && now.saturating_duration_since(first) < LEARN_WINDOW {
            return None;
        }
        let done = std::mem::take(self);
        let port = done.port?;
        let (kind, mode) = Self::kind(&done.msgs)?;
        Some((LearnSource { port, kind }, mode))
    }

    /// The control the first message came from, and its mode.
    fn kind(msgs: &[Msg]) -> Option<(LearnKind, LearnMode)> {
        match *msgs.first()? {
            Msg::Bend { ch, .. } => {
                Some((LearnKind::PitchBend { channel: ch }, LearnMode::Absolute))
            }
            Msg::On { ch, note, .. } => {
                Some((LearnKind::Note { channel: ch, note }, LearnMode::Absolute))
            }
            Msg::Off { .. } => None,
            Msg::Cc { ch, cc, .. } => {
                let seen = |c: u8| {
                    msgs.iter()
                        .any(|m| matches!(*m, Msg::Cc { ch: h, cc: x, .. } if h == ch && x == c))
                };
                let msb = if cc < 32 {
                    Some(cc).filter(|&c| seen(c.saturating_add(32)))
                } else if cc < 64 {
                    Some(cc.saturating_sub(32)).filter(|&c| seen(c))
                } else {
                    None
                };
                if let Some(cc) = msb {
                    return Some((LearnKind::Cc14 { channel: ch, cc }, LearnMode::Absolute));
                }
                let values: Vec<u8> = msgs
                    .iter()
                    .filter_map(|m| match *m {
                        Msg::Cc { ch: h, cc: x, v } if h == ch && x == cc => Some(v),
                        _ => None,
                    })
                    .collect();
                let all =
                    |ok: &dyn Fn(u8) -> bool| values.len() >= 2 && values.iter().all(|&v| ok(v));
                let mode = if all(&|v| matches!(v, 1..=3 | 125..=127)) {
                    LearnMode::Relative2sComplement
                } else if all(&|v| matches!(v, 61..=67) && v != 64) {
                    LearnMode::RelativeBinOffset
                } else {
                    LearnMode::Absolute
                };
                Some((LearnKind::Cc7 { channel: ch, cc }, mode))
            }
        }
    }
}

/// One map's running state.
#[derive(Debug, Default, Clone)]
struct Slot {
    /// A held MSB and the target's value when it arrived (Ruling 8).
    msb: Option<(u8, Option<f64>)>,
    /// The last MSB this map applied.
    last_msb: Option<u8>,
    /// The last LSB this map saw (0 before any).
    lsb: u8,
    /// The last control value this map saw, engaged or not (Ruling 18).
    prev: Option<f64>,
    /// Pick-up: the control has passed the target's value.
    engaged: bool,
    /// The feedback last sent.
    sent: Option<Vec<Vec<u8>>>,
}

/// Drives the targets of learned maps and runs learned commands
/// (Rulings 8, 17 and 18).
#[derive(Debug, Default)]
pub struct Mapper {
    /// The learned maps.
    maps: Vec<LearnMap>,
    /// One slot per map.
    slots: Vec<Slot>,
    /// The learned commands.
    commands: Vec<CommandMap>,
    /// Per command map: its CC (or bend) is at or above the middle.
    high: Vec<bool>,
    /// The idle touches the learned gestures hold.
    idle: IdleTouches,
}

impl Mapper {
    /// Sets the maps and command maps. A map whose source and target are
    /// unchanged keeps its state (MSB hold, pick-up, feedback); idle touches
    /// of targets no map drives any more are forgotten without a release
    /// (the caller owes those, from `held()` before the change; Ruling 17).
    pub fn set(&mut self, maps: Vec<LearnMap>, commands: Vec<CommandMap>) {
        let old_maps = std::mem::take(&mut self.maps);
        let mut old_slots = std::mem::take(&mut self.slots);
        self.slots = maps
            .iter()
            .map(|m| {
                old_maps
                    .iter()
                    .position(|o| o.source == m.source && o.target == m.target)
                    .and_then(|i| old_slots.get_mut(i).map(std::mem::take))
                    .unwrap_or_default()
            })
            .collect();
        for t in self.idle.held() {
            if !maps.iter().any(|m| m.target == t) {
                self.idle.forget(t);
            }
        }
        let old_cmds = std::mem::take(&mut self.commands);
        let old_high = std::mem::take(&mut self.high);
        self.high = commands
            .iter()
            .map(|c| {
                old_cmds
                    .iter()
                    .position(|o| o.source == c.source)
                    .and_then(|i| old_high.get(i).copied())
                    .unwrap_or(false)
            })
            .collect();
        self.maps = maps;
        self.commands = commands;
    }

    /// Whether a map or a command map listens to `port`.
    pub fn listens(&self, port: &str) -> bool {
        self.maps.iter().any(|m| m.source.port == port)
            || self.commands.iter().any(|c| c.source.port == port)
    }

    /// Whether a map or command map takes `msg` from `port` (Ruling 17).
    /// `msg` is one message; a buffer of several is taken when any is.
    pub fn takes(&self, port: &str, msg: &[u8]) -> bool {
        let sources = self
            .maps
            .iter()
            .map(|m| &m.source)
            .chain(self.commands.iter().map(|c| &c.source))
            .filter(|s| s.port == port)
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return false;
        }
        messages(msg).any(|m| sources.iter().any(|s| part(s.kind, m).is_some()))
    }

    /// Rulings 8 and 18; `current` gives a target's heard value now (None: unknown → the message is dropped for relative and picking-up maps).
    pub fn input(
        &mut self,
        port: &str,
        msg: &[u8],
        now: Instant,
        current: &dyn Fn(AutomationTarget) -> Option<f64>,
    ) -> Vec<SurfaceAction> {
        let mut out = Vec::new();
        for m in messages(msg) {
            self.command(port, m, &mut out);
            for i in 0..self.maps.len() {
                let Some(map) = self.maps.get(i) else { break };
                if map.source.port != port {
                    continue;
                }
                if let Some(p) = part(map.source.kind, m) {
                    self.drive(i, p, now, current, &mut out);
                }
            }
        }
        out
    }

    /// Runs the command maps `m` triggers: a note-on, or a CC (or a bend)
    /// rising through the middle, once.
    fn command(&mut self, port: &str, m: Msg, out: &mut Vec<SurfaceAction>) {
        for (c, high) in self.commands.iter().zip(self.high.iter_mut()) {
            if c.source.port != port {
                continue;
            }
            let level = match part(c.source.kind, m) {
                Some(Part::On(_)) => {
                    out.push(SurfaceAction::Command(c.command.clone()));
                    continue;
                }
                Some(Part::Cc(v) | Part::Msb(v)) => v >= 64,
                Some(Part::Bend(v)) => v >= 8192,
                Some(Part::Lsb(_) | Part::Off) | None => continue,
            };
            if level && !*high {
                out.push(SurfaceAction::Command(c.command.clone()));
            }
            *high = level;
        }
    }

    /// One message part for map `i`.
    fn drive(
        &mut self,
        i: usize,
        p: Part,
        now: Instant,
        current: &dyn Fn(AutomationTarget) -> Option<f64>,
        out: &mut Vec<SurfaceAction>,
    ) {
        let Some(map) = self.maps.get(i) else { return };
        let (t, mode) = (map.target, map.mode);
        let relative = mode != LearnMode::Absolute;
        match p {
            Part::Cc(v) | Part::Msb(v) if relative => {
                let detent = match mode {
                    LearnMode::Relative2sComplement => match v {
                        0 => 0,
                        1..=63 => i32::from(v),
                        _ => i32::from(v) - 128,
                    },
                    _ => i32::from(v) - 64,
                };
                if detent == 0 {
                    return;
                }
                let Some(base) = self.idle.last(t).or_else(|| current(t)) else {
                    return;
                };
                let v = (base + f64::from(detent) / 127.0).clamp(0.0, 1.0);
                out.push(self.idle.value(t, v, now));
            }
            Part::Lsb(_) if relative => {}
            Part::Cc(v) => self.absolute(i, f64::from(v) / 127.0, now, current(t), out),
            Part::Bend(v) => self.absolute(i, norm14(v), now, current(t), out),
            Part::Msb(v) => {
                let Some(slot) = self.slots.get_mut(i) else {
                    return;
                };
                let held = slot.msb.replace((v, current(t)));
                if let Some((msb, _)) = held {
                    // The next MSB of the pair: the held one applies, at the value heard now.
                    self.apply_msb(i, msb, now, current(t), out);
                }
            }
            Part::Lsb(v) => {
                let Some(slot) = self.slots.get_mut(i) else {
                    return;
                };
                slot.lsb = v;
                let msb = slot.msb.take().map(|(m, _)| m).or(slot.last_msb);
                if let Some(msb) = msb {
                    self.apply_msb(i, msb, now, current(t), out);
                }
            }
            Part::On(_) if t.is_stepped() => {
                let Some(base) = self.idle.last(t).or_else(|| current(t)) else {
                    return;
                };
                let v = if base >= 0.5 { 0.0 } else { 1.0 };
                // A click: touch, value and release at once (Ruling 5).
                self.idle.forget(t);
                out.extend([
                    SurfaceAction::Gesture(t, Phase::Touch(Some(v))),
                    SurfaceAction::Gesture(t, Phase::Value(v)),
                    SurfaceAction::Gesture(t, Phase::Release),
                ]);
            }
            Part::On(vel) => out.push(self.idle.value(t, f64::from(vel) / 127.0, now)),
            Part::Off => {}
        }
    }

    /// Applies map `i`'s MSB with its last LSB.
    fn apply_msb(
        &mut self,
        i: usize,
        msb: u8,
        now: Instant,
        cur: Option<f64>,
        out: &mut Vec<SurfaceAction>,
    ) {
        let Some(slot) = self.slots.get_mut(i) else {
            return;
        };
        slot.last_msb = Some(msb);
        let v = norm14(u16::from(msb) << 7 | u16::from(slot.lsb));
        self.absolute(i, v, now, cur, out);
    }

    /// An absolute value `v` for map `i`, through pick-up when the map uses it
    /// (Ruling 18); `cur` is the target's value heard now.
    fn absolute(
        &mut self,
        i: usize,
        v: f64,
        now: Instant,
        cur: Option<f64>,
        out: &mut Vec<SurfaceAction>,
    ) {
        let (Some(map), Some(slot)) = (self.maps.get(i), self.slots.get_mut(i)) else {
            return;
        };
        let prev = slot.prev.replace(v);
        if map.pickup && map.picks_up() && !slot.engaged {
            let Some(cur) = cur else { return };
            let near = (v - cur).abs() <= PICKUP_TOLERANCE;
            let passed = prev.is_some_and(|p| p.min(v) <= cur && cur <= p.max(v));
            if !near && !passed {
                return;
            }
            slot.engaged = true;
        }
        out.push(self.idle.value(map.target, v, now));
    }

    /// Held MSBs (Ruling 8), then idle releases (`IdleRelease`), which disengage picking-up maps.
    pub fn tick(&mut self, now: Instant) -> Vec<SurfaceAction> {
        let mut out = Vec::new();
        for i in 0..self.slots.len() {
            let held = self.slots.get_mut(i).and_then(|s| s.msb.take());
            if let Some((msb, cur)) = held {
                self.apply_msb(i, msb, now, cur, &mut out);
            }
        }
        let released = self.idle.tick(now);
        for a in &released {
            if let SurfaceAction::IdleRelease(t, _) = *a {
                for (m, s) in self.maps.iter().zip(self.slots.iter_mut()) {
                    if m.target == t {
                        s.engaged = false;
                    }
                }
            }
        }
        out.extend(released);
        out
    }

    /// `(output port, message)` for absolute maps whose value changed.
    pub fn feedback(
        &mut self,
        current: &dyn Fn(AutomationTarget) -> Option<f64>,
    ) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for (m, s) in self.maps.iter().zip(self.slots.iter_mut()) {
            if m.mode != LearnMode::Absolute {
                continue;
            }
            if self.idle.holds(m.target) {
                // Learn holds it: nothing now, the value once it lets go.
                s.sent = None;
                continue;
            }
            let Some(v) = current(m.target).filter(|v| !v.is_nan()) else {
                continue;
            };
            let msgs = match m.source.kind {
                LearnKind::Cc7 { channel, cc } => {
                    let v = u8::try_from(scale(v, 127)).unwrap_or(127);
                    vec![vec![0xB0 | channel, cc, v]]
                }
                LearnKind::Cc14 { channel, cc } => {
                    let v = scale(v, 16383);
                    let msb = u8::try_from(v >> 7).unwrap_or(127);
                    let lsb = u8::try_from(v & 0x7F).unwrap_or(127);
                    vec![
                        vec![0xB0 | channel, cc, msb],
                        vec![0xB0 | channel, cc.saturating_add(32), lsb],
                    ]
                }
                LearnKind::PitchBend { channel } => {
                    let v = scale(v, 16383);
                    let msb = u8::try_from(v >> 7).unwrap_or(127);
                    let lsb = u8::try_from(v & 0x7F).unwrap_or(127);
                    vec![vec![0xE0 | channel, lsb, msb]]
                }
                // A pad that echoed a note back would toggle its target again.
                LearnKind::Note { .. } => continue,
            };
            if s.sent.as_ref() != Some(&msgs) {
                out.extend(msgs.iter().map(|b| (m.source.port.clone(), b.clone())));
                s.sent = Some(msgs);
            }
        }
        out
    }

    /// The targets the learned gestures hold now.
    pub fn held(&self) -> Vec<AutomationTarget> {
        self.idle.held()
    }
}
