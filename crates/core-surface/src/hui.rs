//! The HUI protocol state machine (spec §8.2; Rulings 5, 13 and 15), from
//! public protocol descriptions. Switches arrive as a zone select
//! (`B0 0F zz`) and a port (`B0 2F 4p` press, `0p` release); faders as
//! hi/lo CC pairs; V-pots drive pan only in 5f-1 (Ruling 5).

use crate::idle::IdleTouches;
use crate::mcu::{
    ECHO_LSB, HEADER, Heard, click, detents, fader_raw, lcd_chars, meter_level, toggled,
};
use crate::shuttle::Shuttle;
use crate::{
    DETENT, ECHO_WINDOW, Held, Link, MixerView, Phase, StripView, Surface, SurfaceAction,
    SurfaceKind, Unit, midi,
};
use core_model::automation::{AutomationTarget, unit};
use core_model::ids::StripId;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The ping VizDAW sends; the unit answers `90 00 7F` (Ruling 15).
pub const PING: [u8; 3] = [0x90, 0x00, 0x00];

/// How often the ping is sent.
pub const PING_EVERY: Duration = Duration::from_secs(1);

/// With no ping reply for this long the unit is offline (still connected).
const OFFLINE_AFTER: Duration = Duration::from_secs(3);
/// Meters are resent at most this often (Ruling 13).
const METER_EVERY: Duration = Duration::from_millis(100);

/// Inbound CCs: zone select, port, jog; fader hi (0x00–0x07) and lo
/// (0x20–0x27) bytes; V-pots (0x40–0x47).
const ZONE: u8 = 0x0F;
const PORT: u8 = 0x2F;
const JOG: u8 = 0x0D;
const FADER_HI: u8 = 0x00;
const FADER_LO: u8 = 0x20;
const VPOT: u8 = 0x40;
/// Outbound CCs: LED zone and port; V-pot rings (0x10–0x17).
const LED_ZONE: u8 = 0x0C;
const LED_PORT: u8 = 0x2C;
const RING: u8 = 0x10;
/// A port byte: bit 6 is the press, the low 3 bits the port.
const PRESS: u8 = 0x40;

/// Strip zone (0x00–0x07) ports.
const TOUCH: u8 = 0;
const SELECT: u8 = 1;
const MUTE: u8 = 2;
const SOLO: u8 = 3;
const AUTO: u8 = 4;
const V_SEL: u8 = 5;
const INSERT: u8 = 6;
const REC_ARM: u8 = 7;
/// The bank zone: ports 0 channel left, 1 bank left, 2 channel right, 3 bank right.
const BANK_ZONE: u8 = 0x0A;
/// The assignment switches' zone, ignored in 5f-1 (Ruling 5).
const ASSIGN_ZONE: u8 = 0x0B;
/// The transport zone and its ports.
const TRANSPORT_ZONE: u8 = 0x0E;
const REWIND: u8 = 1;
const FFWD: u8 = 2;
const STOP: u8 = 3;
const PLAY: u8 = 4;
const RECORD: u8 = 5;

/// The SysEx device ID and commands (after `HEADER`): names, timecode,
/// the 40-character display.
const DEVICE: u8 = 0x05;
const NAME: u8 = 0x10;
const TIMECODE: u8 = 0x11;
const DISPLAY: u8 = 0x12;
/// Characters per scribble name; timecode digits; characters per display
/// zone and per display line (Ruling T2-1: the layouts fit exactly).
const NAME_LEN: usize = 4;
const DIGITS: usize = 8;
const ZONE_LEN: usize = 10;
const LINE: usize = 40;
const _: () = assert!(LINE == 4 * ZONE_LEN);

/// What this unit was last sent, per element (Ruling 13).
#[derive(Debug, Default)]
struct Sent {
    /// Fader values, strips 1–8.
    faders: [Option<u16>; 8],
    /// LEDs by (zone, port).
    leds: BTreeMap<(u8, u8), bool>,
    /// V-pot ring positions.
    rings: [Option<u8>; 8],
    /// Scribble names.
    names: [Option<[u8; NAME_LEN]>; 8],
    /// The 40 × 2 display's zones 0–7.
    zones: [Option<Vec<u8>>; 8],
    /// The timecode digits, rightmost first.
    timecode: Option<[u8; DIGITS]>,
    /// When the meters are next due (`None`: now).
    meters_due: Option<Instant>,
    /// Whether each slot's meter was last sent lit (`None`: unknown), so a
    /// slot that empties goes dark once (review M4).
    lit: [Option<bool>; 8],
}

/// A HUI-compatible unit: 8 strips, no master fader, no extenders (Ruling 4).
pub struct Hui {
    /// The strips the last feedback showed, by slot.
    shown: [Option<StripView>; 8],
    /// Touched faders, holding the strip touched (Ruling T1).
    touched: [Option<StripId>; 8],
    /// Slots a hand still rests on since a focus move released their
    /// touch (final review N2): no fader feedback until the touch-up,
    /// which releases nothing and sends the fader once.
    resting: [bool; 8],
    /// The last zone select, which the next port belongs to.
    zone: Option<u8>,
    /// A fader's hi byte waiting for its lo: (slot, hi).
    pending: Option<(usize, u8)>,
    /// Each fader's last hi byte, for a lo that comes alone.
    last_hi: [Option<u8>; 8],
    idle: IdleTouches,
    shuttle: Shuttle,
    /// The last value sent to each fader, with its time (the echo guard).
    sent: [Option<(u16, Instant)>; 8],
    cache: Sent,
    /// When the last ping went out.
    pinged: Option<Instant>,
    /// `hello` sent a ping that the next feedback times (review M5).
    hello_pinged: bool,
    /// When the last ping reply arrived.
    replied: Option<Instant>,
    link: Link,
    /// Releases owed since the unit went offline in `feedback`, which
    /// cannot answer actions: the next `tick` answers them (Ruling T3-1).
    owed: Vec<SurfaceAction>,
    ignored: u64,
}

impl Default for Hui {
    fn default() -> Hui {
        Hui::new()
    }
}

/// `text` cut or padded with spaces to `width` LCD characters.
fn field(text: &str, width: usize) -> Vec<u8> {
    let mut chars: Vec<u8> = lcd_chars(text).take(width).collect();
    chars.resize(width, b' ');
    chars
}

/// The timecode digits of `position` (`0x00`–`0x09`, `+ 0x10` for a dot
/// after it), rightmost first; at most 8, the leftmost kept, as on the MCU.
fn timecode(position: &str) -> [u8; DIGITS] {
    let mut ds: Vec<u8> = Vec::new();
    for c in position.chars() {
        if let Some(d) = c.to_digit(10).and_then(|d| u8::try_from(d).ok()) {
            ds.push(d);
        } else if matches!(c, '.' | ':' | '|')
            && let Some(last) = ds.last_mut()
        {
            *last |= 0x10;
        }
    }
    ds.truncate(DIGITS);
    let mut out = [0; DIGITS];
    for (o, d) in out.iter_mut().zip(ds.iter().rev()) {
        *o = *d;
    }
    out
}

/// One SysEx message: `HEADER 05 00 <cmd> <body> F7`.
fn sysex(cmd: u8, body: &[u8]) -> Vec<u8> {
    let mut msg = HEADER.to_vec();
    msg.extend_from_slice(&[DEVICE, 0x00, cmd]);
    msg.extend_from_slice(body);
    msg.push(0xF7);
    msg
}

impl Hui {
    /// A new unit.
    pub fn new() -> Hui {
        Hui {
            shown: Default::default(),
            touched: [None; 8],
            resting: [false; 8],
            zone: None,
            pending: None,
            last_hi: [None; 8],
            idle: IdleTouches::default(),
            shuttle: Shuttle::default(),
            sent: [None; 8],
            cache: Sent::default(),
            pinged: None,
            hello_pinged: false,
            replied: None,
            link: Link::default(),
            owed: Vec::new(),
            ignored: 0,
        }
    }

    fn strip(&self, slot: usize) -> Option<&StripView> {
        self.shown.get(slot).and_then(Option::as_ref)
    }

    /// Forgets what fader `slot` was sent: the next feedback sends it once.
    fn resend_fader(&mut self, slot: usize) {
        if let Some(cached) = self.cache.faders.get_mut(slot) {
            *cached = None;
        }
    }

    /// Resends every fader showing `id`.
    fn resend_strip(&mut self, id: StripId) {
        for slot in 0..8 {
            if self.strip(slot).is_some_and(|s| s.id == id) {
                self.resend_fader(slot);
            }
        }
    }

    /// The releases a timeout still owes (Ruling T3-1), as holds: a touch's
    /// `Release` is touched, an `IdleRelease` idle.
    fn take_owed(&mut self) -> Held {
        let mut held = Held::default();
        for a in std::mem::take(&mut self.owed) {
            match a {
                SurfaceAction::Gesture(t, _) => held.add(t, true),
                SurfaceAction::IdleRelease(t, _) => held.add(t, false),
                _ => {}
            }
        }
        held
    }

    /// No ping reply for OFFLINE_AFTER takes the unit offline.
    fn check_online(&mut self, now: Instant) {
        if self
            .replied
            .is_none_or(|at| now.saturating_duration_since(at) >= OFFLINE_AFTER)
        {
            // Ruling T3-1: releases go out with the next tick.
            let owed = self.set_online(false);
            self.owed.extend(owed);
        }
    }

    fn message(&mut self, msg: &[u8], now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        match *msg {
            [0x90, 0x00, 0x7F] => {
                self.replied = Some(now);
                out.extend(self.set_online(true));
                Heard::Known
            }
            [0xB0, ZONE, z] => {
                self.zone = Some(z);
                Heard::Known
            }
            [0xB0, PORT, p] => self.port(p, now, out),
            [0xB0, JOG, d] => {
                let d = detents(d);
                if d != 0 {
                    out.push(SurfaceAction::Jog(d));
                }
                Heard::Known
            }
            [0xB0, cc @ FADER_HI..=0x07, hi] => {
                self.hi(usize::from(cc.saturating_sub(FADER_HI)), hi, now, out)
            }
            [0xB0, cc @ FADER_LO..=0x27, lo] => {
                self.lo(usize::from(cc.saturating_sub(FADER_LO)), lo, now, out)
            }
            [0xB0, cc @ VPOT..=0x47, d] => {
                self.vpot(usize::from(cc.saturating_sub(VPOT)), d, now, out)
            }
            // Anything else, SysEx included, is unknown.
            _ => Heard::Unknown,
        }
    }

    /// A port of the last selected zone.
    fn port(&mut self, p: u8, now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        let Some(zone) = self.zone else {
            return Heard::Unknown;
        };
        if p & !(PRESS | 0x07) != 0 {
            return Heard::Unknown;
        }
        let press = p & PRESS != 0;
        let port = p & 0x07;
        let command = |id: &str| SurfaceAction::Command(id.into());
        match zone {
            0x00..=0x07 => self.strip_switch(usize::from(zone), port, press, out),
            BANK_ZONE => {
                let a = match port {
                    0 => SurfaceAction::Channel(-1),
                    1 => SurfaceAction::Bank(-1),
                    2 => SurfaceAction::Channel(1),
                    3 => SurfaceAction::Bank(1),
                    _ => return Heard::Unknown,
                };
                if press {
                    out.push(a);
                }
                Heard::Known
            }
            // Assignment switches: HUI V-pots are pan only (Ruling 5).
            ASSIGN_ZONE => Heard::Known,
            TRANSPORT_ZONE => {
                match port {
                    REWIND | FFWD => {
                        let forward = port == FFWD;
                        if press {
                            self.shuttle.press(forward, now);
                        } else {
                            out.extend(self.shuttle.release(forward, now));
                        }
                    }
                    STOP | PLAY | RECORD if !press => {}
                    STOP => out.push(command("transport.stop")),
                    PLAY => out.push(command("transport.play")),
                    RECORD => out.push(command("transport.record")),
                    _ => return Heard::Unknown,
                }
                Heard::Known
            }
            _ => Heard::Unknown,
        }
    }

    /// A switch of strip zone `slot`.
    fn strip_switch(
        &mut self,
        slot: usize,
        port: u8,
        press: bool,
        out: &mut Vec<SurfaceAction>,
    ) -> Heard {
        match port {
            TOUCH => return self.touch(slot, press, out),
            // Auto and insert drive nothing in 5f-1; V-Sel is ignored, not
            // counted (Ruling 5).
            AUTO | V_SEL | INSERT => return Heard::Known,
            _ => {}
        }
        // A release does nothing, shown or not (review M6).
        if !press {
            return Heard::Known;
        }
        let Some(s) = self.strip(slot) else {
            return Heard::Unknown;
        };
        let id = s.id;
        match port {
            SELECT => out.push(SurfaceAction::Select(id)),
            MUTE => out.extend(click(AutomationTarget::StripMute(id), toggled(s.mute))),
            SOLO => out.extend(click(AutomationTarget::StripSolo(id), toggled(s.solo))),
            REC_ARM => out.push(SurfaceAction::Arm(id)),
            _ => return Heard::Unknown,
        }
        Heard::Known
    }

    fn touch(&mut self, slot: usize, down: bool, out: &mut Vec<SurfaceAction>) -> Heard {
        // A hand that rested over a focus move lifts: this project holds
        // nothing for it; the fader is sent once (final review N2).
        if let Some(r) = self.resting.get_mut(slot)
            && std::mem::take(r)
            && !down
        {
            self.resend_fader(slot);
            return Heard::Known;
        }
        // A touch-up releases the strip that was touched, whatever the slot
        // shows now (Ruling T1).
        if !down && let Some(was) = self.touched.get_mut(slot).and_then(Option::take) {
            self.resend_fader(slot);
            // A hi byte whose lo was lost dies with its touch (review M1).
            if self.pending.is_some_and(|(s, _)| s == slot) {
                self.pending = None;
            }
            out.push(SurfaceAction::Gesture(
                AutomationTarget::StripFader(was),
                Phase::Release,
            ));
            return Heard::Known;
        }
        let Some(id) = self.strip(slot).map(|s| s.id) else {
            return Heard::Unknown;
        };
        let Some(held) = self.touched.get_mut(slot) else {
            return Heard::Unknown;
        };
        if down && held.is_none() {
            *held = Some(id);
            let t = AutomationTarget::StripFader(id);
            // The touch owns the gesture now: an idle hold opened by a move
            // before the touch must not release it (Ruling T1).
            self.idle.forget(t);
            out.push(SurfaceAction::Gesture(t, Phase::Touch(None)));
        }
        Heard::Known
    }

    /// A fader's hi byte: held for its lo; a pending one is applied alone.
    fn hi(&mut self, slot: usize, hi: u8, now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        if let Some((s, h)) = self.pending.take()
            && let Heard::Unknown = self.fader_value(s, u16::from(h) << 7, true, now, out)
        {
            self.ignored = self.ignored.saturating_add(1);
        }
        if self.strip(slot).is_none() && self.touched.get(slot).is_none_or(Option::is_none) {
            return Heard::Unknown;
        }
        if let Some(last) = self.last_hi.get_mut(slot) {
            *last = Some(hi);
        }
        self.pending = Some((slot, hi));
        Heard::Known
    }

    /// A fader's lo byte: with its pending hi, else the fader's last hi.
    fn lo(&mut self, slot: usize, lo: u8, now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        let hi = match self.pending {
            Some((s, h)) if s == slot => {
                self.pending = None;
                h
            }
            _ => match self.last_hi.get(slot).copied().flatten() {
                Some(h) => h,
                None => return Heard::Unknown,
            },
        };
        self.fader_value(slot, u16::from(hi) << 7 | u16::from(lo), false, now, out)
    }

    /// Fader `slot` moved to `raw` (0–16383); `alone`: a hi byte applied
    /// without its lo.
    fn fader_value(
        &mut self,
        slot: usize,
        raw: u16,
        alone: bool,
        now: Instant,
        out: &mut Vec<SurfaceAction>,
    ) -> Heard {
        let v = f64::from(raw) / 16383.0;
        if let Some(Some(id)) = self.touched.get(slot) {
            out.push(SurfaceAction::Gesture(
                AutomationTarget::StripFader(*id),
                Phase::Value(v),
            ));
            return Heard::Known;
        }
        let Some(id) = self.strip(slot).map(|s| s.id) else {
            return Heard::Unknown;
        };
        if let Some(Some((sent, at))) = self.sent.get(slot)
            && (raw.abs_diff(*sent) <= ECHO_LSB || alone && raw >> 7 == *sent >> 7)
            && now.saturating_duration_since(*at) < ECHO_WINDOW
        {
            // The echo of a motor move: ignored, not counted (Ruling 13).
            // A hi byte alone echoes when it is the hi that was sent (review M2).
            return Heard::Known;
        }
        out.push(self.idle.value(AutomationTarget::StripFader(id), v, now));
        Heard::Known
    }

    /// A V-pot: `d & 0x40` set is clockwise, `d & 0x3F` the detents; pan.
    fn vpot(&mut self, slot: usize, d: u8, now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        let Some(s) = self.strip(slot) else {
            return Heard::Unknown;
        };
        let n = i32::from(d & 0x3F);
        if n == 0 {
            return Heard::Known;
        }
        let n = if d & 0x40 != 0 { n } else { -n };
        let t = AutomationTarget::StripPan(s.id);
        let current = self.idle.last(t).unwrap_or(s.pan);
        let next = (current + DETENT * f64::from(n)).clamp(0.0, 1.0);
        out.push(self.idle.value(t, next, now));
        Heard::Known
    }

    /// Fader values, except while touched or idle-held (spec §8; Ruling T2 I1).
    fn feedback_faders(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) {
        for k in 0..8u8 {
            let slot = usize::from(k);
            if self.touched.get(slot).is_some_and(Option::is_some)
                || self.resting.get(slot).is_some_and(|r| *r)
                || self
                    .strip(slot)
                    .is_some_and(|s| self.idle.holds(AutomationTarget::StripFader(s.id)))
            {
                continue;
            }
            let raw = fader_raw(self.strip(slot).map_or(0.0, |s| s.fader));
            let Some(cached) = self.cache.faders.get_mut(slot) else {
                continue;
            };
            if *cached == Some(raw) {
                continue;
            }
            *cached = Some(raw);
            if let Some(sent) = self.sent.get_mut(slot) {
                *sent = Some((raw, now));
            }
            // A lone lo after this belongs to the hi sent (review M3).
            if let Some(last) = self.last_hi.get_mut(slot) {
                *last = Some((raw >> 7) as u8);
            }
            out.push(vec![0xB0, FADER_HI + k, (raw >> 7) as u8]);
            out.push(vec![0xB0, FADER_LO + k, (raw & 0x7F) as u8]);
        }
    }

    /// LEDs that changed, each zone's select once before its ports.
    fn feedback_leds(&mut self, view: &MixerView, out: &mut Vec<Vec<u8>>) {
        let mut leds: Vec<(u8, u8, bool)> = Vec::new();
        for k in 0..8u8 {
            let s = self.strip(usize::from(k));
            let (select, mute, solo, arm) = s.map_or((false, false, false, false), |s| {
                (s.select, s.mute, s.solo, s.arm)
            });
            leds.extend([
                (k, SELECT, select),
                (k, MUTE, mute),
                (k, SOLO, solo),
                (k, REC_ARM, arm),
            ]);
        }
        leds.extend([
            (TRANSPORT_ZONE, STOP, !view.playing),
            (TRANSPORT_ZONE, PLAY, view.playing),
            (TRANSPORT_ZONE, RECORD, view.recording),
        ]);
        let mut zone = None;
        for (z, port, on) in leds {
            if self.cache.leds.insert((z, port), on) == Some(on) {
                continue;
            }
            if zone != Some(z) {
                zone = Some(z);
                out.push(vec![0xB0, LED_ZONE, z]);
            }
            out.push(vec![0xB0, LED_PORT, if on { PRESS | port } else { port }]);
        }
    }

    /// V-pot rings: pan as positions 1–11, dark (0) with no strip.
    fn feedback_rings(&mut self, out: &mut Vec<Vec<u8>>) {
        for k in 0..8u8 {
            let slot = usize::from(k);
            // In 1–11 after `unit`; float-to-int `as` saturates anyway.
            let p = self
                .strip(slot)
                .map_or(0, |s| 1 + (unit(s.pan) * 10.0).round() as u8);
            let Some(cached) = self.cache.rings.get_mut(slot) else {
                continue;
            };
            if *cached != Some(p) {
                *cached = Some(p);
                out.push(vec![0xB0, RING + k, p]);
            }
        }
    }

    /// The 4-character scribble names.
    fn feedback_names(&mut self, out: &mut Vec<Vec<u8>>) {
        for k in 0..8u8 {
            let slot = usize::from(k);
            let text = self.strip(slot).map_or("", |s| s.name.as_str());
            let mut name = [b' '; NAME_LEN];
            for (n, c) in name.iter_mut().zip(field(text, NAME_LEN)) {
                *n = c;
            }
            let Some(cached) = self.cache.names.get_mut(slot) else {
                continue;
            };
            if *cached != Some(name) {
                *cached = Some(name);
                let mut body = vec![k];
                body.extend_from_slice(&name);
                out.push(sysex(NAME, &body));
            }
        }
    }

    /// The 40 × 2 display in 8 zones of 10 (line 1: position and the V-pot
    /// assignment; line 2: the selected strip's name and mode), and the
    /// 8-digit timecode display.
    fn feedback_displays(&mut self, view: &MixerView, out: &mut Vec<Vec<u8>>) {
        let selected = view
            .strips
            .iter()
            .chain(view.master.as_ref())
            .find(|s| s.select);
        let mut lines = field(&view.position, LINE / 2);
        lines.extend(field("V-Pots: Pan", LINE / 2));
        match selected {
            Some(s) => {
                lines.extend(field(&s.name, LINE - ZONE_LEN));
                lines.extend(field(s.mode.name(), ZONE_LEN));
            }
            None => lines.extend(field("", LINE)),
        }
        for (z, chunk) in (0u8..).zip(lines.chunks(ZONE_LEN)) {
            let Some(cached) = self.cache.zones.get_mut(usize::from(z)) else {
                continue;
            };
            if cached.as_deref() != Some(chunk) {
                *cached = Some(chunk.to_vec());
                let mut body = vec![z];
                body.extend_from_slice(chunk);
                out.push(sysex(DISPLAY, &body));
            }
        }
        let tc = timecode(&view.position);
        if self.cache.timecode != Some(tc) {
            self.cache.timecode = Some(tc);
            out.push(sysex(TIMECODE, &tc));
        }
    }

    /// Meters for every shown strip, both sides, at most every METER_EVERY.
    fn feedback_meters(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) {
        if self.cache.meters_due.is_some_and(|due| now < due) {
            return;
        }
        self.cache.meters_due = now.checked_add(METER_EVERY);
        for k in 0..8u8 {
            let (left, right) = match self.strip(usize::from(k)) {
                Some(s) => (meter_level(s.meter.0), meter_level(s.meter.1)),
                None => (0, 0),
            };
            let shown = self.strip(usize::from(k)).is_some();
            let Some(lit) = self.cache.lit.get_mut(usize::from(k)) else {
                continue;
            };
            // An empty slot is sent dark once, then nothing.
            if shown || *lit != Some(false) {
                *lit = Some(shown);
                out.push(vec![0xA0, k, left]);
                out.push(vec![0xA0, k, 0x10 | right]);
            }
        }
    }
}

impl Surface for Hui {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::Hui
    }

    fn input(&mut self, bytes: &[u8], now: Instant) -> Vec<SurfaceAction> {
        let mut out = Vec::new();
        for part in midi::split(bytes) {
            let heard = match part {
                Ok(msg) => self.message(&msg, now, &mut out),
                Err(_) => Heard::Unknown,
            };
            if let Heard::Unknown = heard {
                self.ignored = self.ignored.saturating_add(1);
            }
        }
        out
    }

    fn tick(&mut self, now: Instant) -> Vec<SurfaceAction> {
        self.check_online(now);
        let mut out = std::mem::take(&mut self.owed);
        out.extend(self.idle.tick(now));
        // An idle-released fader is sent once by the next feedback (Ruling T2 I1).
        for a in &out {
            if let SurfaceAction::IdleRelease(AutomationTarget::StripFader(id), _) = *a {
                self.resend_strip(id);
            }
        }
        out.extend(self.shuttle.tick(now));
        out
    }

    fn feedback(&mut self, view: &MixerView, first: usize, now: Instant) -> Vec<Vec<u8>> {
        for (k, slot) in self.shown.iter_mut().enumerate() {
            let next = view.strips.get(first.saturating_add(k)).cloned();
            // A pending hi byte belongs to the strip it came for (review M1).
            if slot.as_ref().map(|s| s.id) != next.as_ref().map(|s| s.id)
                && self.pending.is_some_and(|(p, _)| p == k)
            {
                self.pending = None;
            }
            *slot = next;
        }
        self.check_online(now);
        let mut out = Vec::new();
        if std::mem::take(&mut self.hello_pinged) && self.pinged.is_none() {
            // `hello` sent this ping: time it from now.
            self.pinged = Some(now);
        }
        if self
            .pinged
            .is_none_or(|at| now.saturating_duration_since(at) >= PING_EVERY)
        {
            self.pinged = Some(now);
            out.push(PING.to_vec());
        }
        self.feedback_faders(now, &mut out);
        self.feedback_leds(view, &mut out);
        self.feedback_rings(&mut out);
        self.feedback_names(&mut out);
        self.feedback_displays(view, &mut out);
        self.feedback_meters(now, &mut out);
        out
    }

    fn refresh(&mut self) {
        self.cache = Sent::default();
    }

    fn hello(&mut self) -> Vec<Vec<u8>> {
        self.hello_pinged = true;
        vec![PING.to_vec()]
    }

    fn online(&self) -> bool {
        self.link.get()
    }

    fn ignored(&self) -> u64 {
        self.ignored
    }

    fn touched(&self) -> Vec<AutomationTarget> {
        self.touched
            .iter()
            .flatten()
            .map(|&id| AutomationTarget::StripFader(id))
            .collect()
    }

    fn release_holds(&mut self) -> Held {
        let resting = self.touched.map(|t| t.is_some());
        let mut held = self.take_owed();
        let rest = self.forget_all();
        for t in rest.touched {
            held.add(t, true);
        }
        for t in rest.idle {
            held.add(t, false);
        }
        self.resting = resting;
        held
    }

    fn release_all(&mut self) -> Held {
        // Releases still owed by a timeout go with the rest.
        let mut held = self.take_owed();
        let rest = self.forget_all();
        for t in rest.touched {
            held.add(t, true);
        }
        for t in rest.idle {
            held.add(t, false);
        }
        // A reconnected unit is online once it answers a ping (review M5).
        self.link = Link::default();
        self.replied = None;
        held
    }
}

impl Unit for Hui {
    fn link(&mut self) -> &mut Link {
        &mut self.link
    }

    fn idle(&self) -> &IdleTouches {
        &self.idle
    }

    fn forget_all(&mut self) -> Held {
        let mut held = Held::default();
        for t in self.touched() {
            held.add(t, true);
        }
        for t in self.idle.take_all() {
            held.add(t, false);
        }
        // Every fader it held gets feedback again.
        for slot in 0..8 {
            if self.touched.get(slot).is_some_and(Option::is_some) {
                self.resend_fader(slot);
            }
        }
        for t in held.all() {
            if let AutomationTarget::StripFader(id) = t {
                self.resend_strip(id);
            }
        }
        self.touched = [None; 8];
        self.resting = [false; 8];
        self.zone = None;
        self.pending = None;
        self.last_hi = [None; 8];
        self.shuttle.clear();
        held
    }
}
