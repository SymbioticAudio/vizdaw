//! The MPE codec (plan 5g-2 Task 2; Rulings 4-7): pitch-bend scaling, the
//! configuration messages, [`MpeDecoder`] (a controller's channel-per-note
//! MIDI 1.0 into per-note [`LiveMsg`]s) and [`MpeEncoder`] (the reverse,
//! onto rotating member channels). Pure; nothing here allocates.

use crate::input_fx::{CENTER, Expr, LiveMsg, NoteExpr, up7, up32};
use crate::midi1::{Midi1Msg, note_on_velocity, release_velocity, value7};
use core_model::mpe::{DEFAULT_BEND_RANGE, MAX_BEND_RANGE, MAX_MEMBERS, MpeConfig};

/// Semitones of VizDAW's full-scale per-note pitch bend (Ruling 5).
const SCALE_SEMITONES: i64 = 48;
/// 2^18: 32-bit bend units per 14-bit unit.
const UNIT: i64 = 1 << 18;
/// The pitch-bend centre at 14 bits.
const CENTER14: i64 = 8192;
/// The timbre controller (CC 74).
const TIMBRE_CC: u8 = 74;

/// Rounds `num / den` (`den > 0`) half away from zero.
fn div_round(num: i64, den: i64) -> i64 {
    let half = den / 2;
    if num >= 0 {
        (num + half) / den
    } else {
        (num - half) / den
    }
}

/// 14-bit pitch bend at `range` semitones to VizDAW's ±48 scale
/// (Ruling 5). // RT
pub fn bend_in(v14: u16, range: u8) -> u32 {
    let range = i64::from(range.max(1));
    let d = i64::from(v14.min(16383)) - CENTER14;
    let off = div_round(d * range * UNIT, SCALE_SEMITONES);
    (i64::from(CENTER) + off).clamp(0, i64::from(u32::MAX)) as u32
}

/// The inverse of [`bend_in`], clamped to 0-16383. // RT
pub fn bend_out(v32: u32, range: u8) -> u16 {
    let range = i64::from(range.max(1));
    let d = i64::from(v32) - i64::from(CENTER);
    let off = div_round(d * SCALE_SEMITONES, range * UNIT);
    (CENTER14 + off).clamp(0, 16383) as u16
}

/// The MCM and the member pitch-bend range (Ruling 7). // RT
pub fn configuration(cfg: &MpeConfig) -> [Midi1Msg; 11] {
    let mgr = 0xB0 | cfg.manager();
    let mem = 0xB0 | cfg.member(0).unwrap_or_else(|| cfg.manager());
    [
        Midi1Msg::three(mgr, 101, 0),
        Midi1Msg::three(mgr, 100, 6),
        Midi1Msg::three(mgr, 6, cfg.members),
        Midi1Msg::three(mgr, 101, 127),
        Midi1Msg::three(mgr, 100, 127),
        Midi1Msg::three(mem, 101, 0),
        Midi1Msg::three(mem, 100, 0),
        Midi1Msg::three(mem, 6, cfg.bend_range),
        Midi1Msg::three(mem, 38, 0),
        Midi1Msg::three(mem, 101, 127),
        Midi1Msg::three(mem, 100, 127),
    ]
}

/// One channel's decoder state.
#[derive(Debug, Clone, Copy)]
struct Chan {
    /// Selected RPN, MSB then LSB; (127, 127) is null.
    rpn: (u8, u8),
    expr: NoteExpr,
    /// Pitches sounding on this channel.
    sounding: u128,
}

impl Chan {
    const NEW: Chan = Chan {
        rpn: (127, 127),
        expr: NoteExpr {
            bend: CENTER,
            pressure: 0,
            timbre: CENTER,
        },
        sounding: 0,
    };
}

/// MPE input (Rulings 4, 6).
#[derive(Debug, Clone)]
pub struct MpeDecoder {
    initial: MpeConfig,
    live: Option<MpeConfig>,
    chans: [Chan; 16],
    changed: bool,
}

impl PartialEq for MpeDecoder {
    fn eq(&self, other: &Self) -> bool {
        self.initial == other.initial
    }
}

impl Eq for MpeDecoder {}

impl MpeDecoder {
    /// A decoder for `cfg`.
    pub fn new(cfg: MpeConfig) -> MpeDecoder {
        MpeDecoder {
            initial: cfg,
            live: Some(cfg),
            chans: [Chan::NEW; 16],
            changed: false,
        }
    }

    /// The setting it was made with.
    pub fn initial(&self) -> MpeConfig {
        self.initial
    }

    /// The live setting; `None`: an MCM turned the zone off (plain MIDI). // RT
    pub fn config(&self) -> Option<MpeConfig> {
        self.live
    }

    /// Channel `ch` (0-15) is this lane's now: the zone's manager or a
    /// member; every channel when the zone is off. // RT
    pub fn accepts(&self, ch: u8) -> bool {
        match self.live {
            None => ch < 16,
            Some(c) => ch == c.manager() || c.is_member(ch),
        }
    }

    /// The live setting, once after each change. // RT
    pub fn take_changed(&mut self) -> Option<Option<MpeConfig>> {
        if std::mem::take(&mut self.changed) {
            Some(self.live)
        } else {
            None
        }
    }

    /// Decodes one channel message. // RT
    pub fn decode(&mut self, msg: Midi1Msg, out: &mut dyn FnMut(LiveMsg)) {
        let [status, d1, d2] = msg.bytes;
        let Some(cfg) = self.live else {
            // Off: plain MIDI, except an MCM on the zone's manager channel,
            // which can turn the zone back on.
            let (ch, d1) = (status & 0x0F, d1 & 0x7F);
            let is_rpn = status & 0xF0 == 0xB0
                && msg.len >= 3
                && ch == self.initial.manager()
                && matches!(d1, 101 | 100 | 99 | 98 | 6 | 38);
            if is_rpn {
                self.rpn_byte(ch, d1, d2 & 0x7F, self.initial, out);
                if !matches!(d1, 99 | 98) {
                    return;
                }
            }
            out(LiveMsg::from_midi1(msg));
            return;
        };
        if !(0x80..0xF0).contains(&status) {
            out(LiveMsg::from_midi1(msg));
            return;
        }
        let ch = status & 0x0F;
        if !self.accepts(ch) {
            return;
        }
        let (d1, d2) = (d1 & 0x7F, d2 & 0x7F);
        let kind = status & 0xF0;
        if kind == 0xB0 && msg.len >= 3 && matches!(d1, 101 | 100 | 99 | 98 | 6 | 38) {
            self.rpn_byte(ch, d1, d2, cfg, out);
            // The RPN bytes are consumed; an NRPN select passes as a channel message.
            if !matches!(d1, 99 | 98) {
                return;
            }
        }
        if ch == cfg.manager() {
            out(LiveMsg::from_midi1(msg));
            return;
        }
        let Some(chan) = self.chans.get_mut(usize::from(ch)) else {
            return;
        };
        match kind {
            0x90 if d2 > 0 && msg.len >= 3 => {
                chan.sounding |= 1u128 << d1;
                out(LiveMsg::NoteOn {
                    pitch: d1,
                    velocity: up7(d2),
                    expr: chan.expr,
                });
            }
            0x80 | 0x90 if msg.len >= 3 => {
                chan.sounding &= !(1u128 << d1);
                out(LiveMsg::NoteOff {
                    pitch: d1,
                    velocity: up7(d2),
                });
            }
            0xE0 if msg.len >= 3 => {
                let v14 = u16::from(d1) | (u16::from(d2) << 7);
                chan.expr.bend = bend_in(v14, cfg.bend_range);
                spread(chan.sounding, Expr::Bend, chan.expr.bend, out);
            }
            0xD0 if msg.len >= 2 => {
                chan.expr.pressure = up32(u32::from(d1), 7);
                spread(chan.sounding, Expr::Pressure, chan.expr.pressure, out);
            }
            0xB0 if d1 == TIMBRE_CC && msg.len >= 3 => {
                chan.expr.timbre = up32(u32::from(d2), 7);
                spread(chan.sounding, Expr::Timbre, chan.expr.timbre, out);
            }
            _ => out(LiveMsg::from_midi1(msg)),
        }
    }

    /// RPN selection (CC 101/100; NRPN CC 99/98 deselects) and data entry
    /// (CC 6) on channel `ch`. `cfg` is the setting a change builds on.
    fn rpn_byte(&mut self, ch: u8, cc: u8, v: u8, cfg: MpeConfig, out: &mut dyn FnMut(LiveMsg)) {
        let Some(chan) = self.chans.get_mut(usize::from(ch)) else {
            return;
        };
        let rpn = chan.rpn;
        match cc {
            101 => chan.rpn.0 = v,
            100 => chan.rpn.1 = v,
            99 | 98 => chan.rpn = (127, 127),
            6 => match rpn {
                // An MCM also resets the members' bend range to the MPE default.
                (0, 6) if ch == cfg.manager() => {
                    let next = (v > 0).then_some(MpeConfig {
                        members: v.min(MAX_MEMBERS),
                        bend_range: DEFAULT_BEND_RANGE,
                        ..cfg
                    });
                    self.set_live(next, out);
                }
                (0, 0) if ch != cfg.manager() && v > 0 => {
                    let next = MpeConfig {
                        bend_range: v.min(MAX_BEND_RANGE),
                        ..cfg
                    };
                    self.set_live(Some(next), out);
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Applies a new setting. Channels that leave the zone end their notes
    /// and forget their state.
    fn set_live(&mut self, next: Option<MpeConfig>, out: &mut dyn FnMut(LiveMsg)) {
        if self.live == next {
            return;
        }
        for ch in 0..16u8 {
            let was = self.live.is_some_and(|c| c.is_member(ch));
            let is = next.is_some_and(|c| c.is_member(ch));
            if was && !is {
                if let Some(chan) = self.chans.get_mut(usize::from(ch)) {
                    end_all(chan.sounding, out);
                    *chan = Chan::NEW;
                }
            }
        }
        self.live = next;
        self.changed = true;
    }
}

/// Ends every pitch in `sounding`. // RT
fn end_all(mut sounding: u128, out: &mut dyn FnMut(LiveMsg)) {
    while sounding != 0 {
        let pitch = sounding.trailing_zeros() as u8;
        sounding &= sounding - 1;
        out(LiveMsg::NoteOff { pitch, velocity: 0 });
    }
}

/// Sends a per-note message for every pitch in `sounding`. // RT
fn spread(mut sounding: u128, expr: Expr, value: u32, out: &mut dyn FnMut(LiveMsg)) {
    while sounding != 0 {
        let pitch = sounding.trailing_zeros() as u8;
        sounding &= sounding - 1;
        out(LiveMsg::PerNote { pitch, expr, value });
    }
}

/// One member channel's encoder state.
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    /// The sounding pitch and the stamp it started at.
    note: Option<(u8, u64)>,
    /// The stamp of its last release; 0 for never used.
    released: u64,
}

/// MPE output (Ruling 7).
#[derive(Debug, Clone)]
pub struct MpeEncoder {
    cfg: MpeConfig,
    /// Per pitch: 0 for none, else its channel + 1.
    ch_of: [u8; 128],
    slots: [Slot; 16],
    stamp: u64,
    stolen: u64,
}

impl PartialEq for MpeEncoder {
    fn eq(&self, other: &Self) -> bool {
        self.cfg == other.cfg
    }
}

impl Eq for MpeEncoder {}

impl MpeEncoder {
    /// An encoder for `cfg`.
    pub fn new(cfg: MpeConfig) -> MpeEncoder {
        MpeEncoder {
            cfg,
            ch_of: [0; 128],
            slots: [Slot::default(); 16],
            stamp: 0,
            stolen: 0,
        }
    }

    /// The setting it was made with.
    pub fn config(&self) -> MpeConfig {
        self.cfg
    }

    /// Encodes one message. // RT
    pub fn encode(&mut self, msg: LiveMsg, out: &mut dyn FnMut(Midi1Msg)) {
        match msg {
            LiveMsg::NoteOn {
                pitch,
                velocity,
                expr,
            } => self.note_on(pitch & 0x7F, velocity, expr, out),
            LiveMsg::NoteOff { pitch, velocity } => {
                let pitch = pitch & 0x7F;
                if let Some(ch) = self.channel_of(pitch) {
                    out(Midi1Msg::three(
                        0x80 | ch,
                        pitch,
                        release_velocity(velocity),
                    ));
                    self.free(pitch, ch);
                }
            }
            LiveMsg::PerNote { pitch, expr, value } => {
                if let Some(ch) = self.channel_of(pitch & 0x7F) {
                    out(per_note(ch, expr, value, self.cfg.bend_range));
                }
            }
            LiveMsg::Channel(_) => {
                if let Some(m) = msg.to_midi1(self.cfg.manager()) {
                    out(m);
                }
            }
        }
    }

    /// Note-offs for every sounding note. // RT
    pub fn flush(&mut self, out: &mut dyn FnMut(Midi1Msg)) {
        for ch in 0..16u8 {
            if let Some((pitch, _)) = self.slot(ch).note {
                out(Midi1Msg::three(0x80 | ch, pitch, 0));
                self.free(pitch, ch);
            }
        }
    }

    /// Notes stolen since the last call. // RT
    pub fn take_stolen(&mut self) -> u64 {
        std::mem::take(&mut self.stolen)
    }

    fn channel_of(&self, pitch: u8) -> Option<u8> {
        match self.ch_of.get(usize::from(pitch)) {
            Some(&c) if c > 0 => Some(c - 1),
            _ => None,
        }
    }

    fn next_stamp(&mut self) -> u64 {
        self.stamp = self.stamp.saturating_add(1);
        self.stamp
    }

    /// Releases `pitch` from `ch`.
    fn free(&mut self, pitch: u8, ch: u8) {
        let stamp = self.next_stamp();
        if let Some(c) = self.ch_of.get_mut(usize::from(pitch)) {
            *c = 0;
        }
        if let Some(s) = self.slots.get_mut(usize::from(ch)) {
            s.note = None;
            s.released = stamp;
        }
    }

    fn slot(&self, ch: u8) -> Slot {
        self.slots.get(usize::from(ch)).copied().unwrap_or_default()
    }

    /// The member channel for a new note: the free one released longest
    /// ago, else (stealing) the one whose note started first.
    fn allocate(&mut self, out: &mut dyn FnMut(Midi1Msg)) -> Option<u8> {
        let cfg = self.cfg;
        let members = (0..cfg.members).filter_map(|k| cfg.member(k));
        let free = members
            .clone()
            .filter(|&c| self.slot(c).note.is_none())
            .min_by_key(|&c| self.slot(c).released);
        if free.is_some() {
            return free;
        }
        let oldest = members.min_by_key(|&c| self.slot(c).note.map_or(u64::MAX, |n| n.1))?;
        let (pitch, _) = self.slot(oldest).note?;
        out(Midi1Msg::three(0x80 | oldest, pitch, 0));
        self.free(pitch, oldest);
        self.stolen = self.stolen.saturating_add(1);
        Some(oldest)
    }

    fn note_on(&mut self, pitch: u8, velocity: u16, expr: NoteExpr, out: &mut dyn FnMut(Midi1Msg)) {
        if let Some(ch) = self.channel_of(pitch) {
            out(Midi1Msg::three(0x80 | ch, pitch, 0));
            self.free(pitch, ch);
        }
        let Some(ch) = self.allocate(out) else {
            return;
        };
        let range = self.cfg.bend_range;
        out(per_note(ch, Expr::Bend, expr.bend, range));
        out(per_note(ch, Expr::Timbre, expr.timbre, range));
        out(per_note(ch, Expr::Pressure, expr.pressure, range));
        out(Midi1Msg::three(
            0x90 | ch,
            pitch,
            note_on_velocity(velocity),
        ));
        let stamp = self.next_stamp();
        if let Some(s) = self.slots.get_mut(usize::from(ch)) {
            s.note = Some((pitch, stamp));
        }
        if let Some(c) = self.ch_of.get_mut(usize::from(pitch)) {
            *c = ch + 1;
        }
    }
}

/// A per-note value as the channel message of member `ch`. // RT
fn per_note(ch: u8, expr: Expr, value: u32, range: u8) -> Midi1Msg {
    match expr {
        Expr::Bend => {
            let v = bend_out(value, range);
            Midi1Msg::three(0xE0 | ch, (v & 0x7F) as u8, (v >> 7) as u8)
        }
        Expr::Timbre => Midi1Msg::three(0xB0 | ch, TIMBRE_CC, value7(value)),
        Expr::Pressure => Midi1Msg::two(0xD0 | ch, value7(value)),
    }
}
