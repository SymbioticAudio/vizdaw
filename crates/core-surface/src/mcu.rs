//! The MCU and MCU XT protocol state machine (spec §8.2; Rulings 5, 13 and
//! 14), from public protocol descriptions.

use crate::idle::IdleTouches;
use crate::shuttle::Shuttle;
use crate::{
    Assign, MixerView, ParamView, Phase, StripView, Surface, SurfaceAction, SurfaceKind, midi,
};
use core_model::automation::{AutomationTarget, SendIndex};
use core_model::ids::StripId;
use std::time::{Duration, Instant};

/// Fader touch notes: strips 1–8, then the master (main unit only).
const FADER_TOUCH: u8 = 0x68;
/// The master fader's touch note.
const MASTER_TOUCH: u8 = 0x70;
/// Strip button rows, 8 notes each.
const REC: u8 = 0x00;
const SOLO: u8 = 0x08;
const MUTE: u8 = 0x10;
const SELECT: u8 = 0x18;
const VPOT_PUSH: u8 = 0x20;
/// V-pot assignment buttons.
const ASSIGN_SEND: u8 = 0x29;
const ASSIGN_PAN: u8 = 0x2A;
const ASSIGN_PLUGIN: u8 = 0x2B;
/// Bank and channel buttons.
const BANK_LEFT: u8 = 0x2E;
const BANK_RIGHT: u8 = 0x2F;
const CHANNEL_LEFT: u8 = 0x30;
const CHANNEL_RIGHT: u8 = 0x31;
/// Automation mode buttons.
const READ: u8 = 0x4A;
const WRITE: u8 = 0x4B;
const TRIM: u8 = 0x4C;
const TOUCH: u8 = 0x4D;
const LATCH: u8 = 0x4E;
/// Utility and transport buttons.
const SAVE: u8 = 0x50;
const UNDO: u8 = 0x51;
const CYCLE: u8 = 0x56;
const REWIND: u8 = 0x5B;
const FFWD: u8 = 0x5C;
const STOP: u8 = 0x5D;
const PLAY: u8 = 0x5E;
const RECORD: u8 = 0x5F;
/// The jog wheel's CC.
const JOG: u8 = 0x3C;
/// The V-pots' CCs, strips 1–8.
const VPOT: u8 = 0x10;

/// The SysEx header before the device ID.
const HEADER: [u8; 4] = [0xF0, 0x00, 0x00, 0x66];
/// A fader value within this many LSBs of what was sent is an echo (Ruling 13).
const ECHO_LSB: u16 = 1;
/// ... when it arrives within this long of the send.
const ECHO_WINDOW: Duration = Duration::from_millis(100);
/// One V-pot detent, in normalised units (Ruling 5).
const DETENT: f64 = 0.01;

/// What became of one message.
enum Heard {
    /// Understood (taken or deliberately ignored).
    Known,
    /// Malformed or unknown: counted.
    Unknown,
}

/// An MCU-compatible main unit or extender.
pub struct Mcu {
    extender: bool,
    /// The strips the last feedback showed, by slot.
    shown: [Option<StripView>; 8],
    /// The master the last feedback showed (main unit only).
    master: Option<StripView>,
    /// The parameters the last feedback showed for plug-in assignment.
    plugin: Vec<ParamView>,
    /// Touched faders (slots 0–7, 8 the master), holding the strip touched,
    /// so a bank change while touched releases the strip that was touched.
    touched: [Option<StripId>; 9],
    assign: Assign,
    idle: IdleTouches,
    shuttle: Shuttle,
    /// The last value sent to each fader, with its time, for the echo guard
    /// (Ruling 13). Task 2's feedback writes it.
    sent: [Option<(u16, Instant)>; 9],
    /// Replies to send with the next feedback (the handshake).
    replies: Vec<Vec<u8>>,
    online: bool,
    ignored: u64,
}

/// The answer to a host connection query's challenge (Ruling 14).
pub fn handshake_response(challenge: [u8; 4]) -> [u8; 4] {
    let [c0, c1, c2, c3] = challenge;
    [
        0x7F & c0.wrapping_add(c1 ^ 0x0A).wrapping_sub(c3),
        0x7F & ((c2 >> 4) ^ c0.wrapping_add(c3)),
        0x7F & (c3.wrapping_sub(c2 << 2) ^ (c0 | c1)),
        0x7F & c1.wrapping_sub(c2).wrapping_add(0xF0 ^ (c3 << 4)),
    ]
}

/// A click: touch, value and release at once (Ruling 5).
fn click(t: AutomationTarget, v: f64) -> Vec<SurfaceAction> {
    vec![
        SurfaceAction::Gesture(t, Phase::Touch(Some(v))),
        SurfaceAction::Gesture(t, Phase::Value(v)),
        SurfaceAction::Gesture(t, Phase::Release),
    ]
}

/// The toggled value of a switch that is `on`.
fn toggled(on: bool) -> f64 {
    if on { 0.0 } else { 1.0 }
}

/// Detents of a relative control: bit 6 set is counter-clockwise, the low
/// 6 bits the count.
fn detents(v: u8) -> i32 {
    let n = i32::from(v & 0x3F);
    if v & 0x40 != 0 { -n } else { n }
}

impl Mcu {
    /// A new unit: an extender (MCU XT) or the main unit.
    pub fn new(extender: bool) -> Mcu {
        Mcu {
            extender,
            shown: Default::default(),
            master: None,
            plugin: Vec::new(),
            touched: [None; 9],
            assign: Assign::default(),
            idle: IdleTouches::default(),
            shuttle: Shuttle::default(),
            sent: [None; 9],
            replies: Vec::new(),
            online: false,
            ignored: 0,
        }
    }

    /// The SysEx device ID: 0x14 main, 0x15 extender.
    pub fn device_id(&self) -> u8 {
        if self.extender { 0x15 } else { 0x14 }
    }

    /// The strip shown in fader slot `slot` (8: the master, main unit only).
    fn fader_strip(&self, slot: usize) -> Option<StripId> {
        if slot == 8 {
            if self.extender {
                return None;
            }
            return self.master.as_ref().map(|s| s.id);
        }
        self.strip(slot).map(|s| s.id)
    }

    fn strip(&self, slot: usize) -> Option<&StripView> {
        self.shown.get(slot).and_then(Option::as_ref)
    }

    fn message(&mut self, msg: &[u8], now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        if msg.first() == Some(&0xF0) {
            return self.sysex(msg);
        }
        let heard = self.channel(msg, now, out);
        if let Heard::Known = heard {
            // Any valid message marks the unit online (Ruling 14).
            self.online = true;
        }
        heard
    }

    fn channel(&mut self, msg: &[u8], now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        match *msg {
            [0x90, note, vel] => self.note(note, vel, now, out),
            [0x80, note, _] => self.note(note, 0, now, out),
            [0xB0, cc, v] => self.cc(cc, v, now, out),
            [s @ 0xE0..=0xEF, lsb, msb] => self.bend(usize::from(s & 0x0F), lsb, msb, now, out),
            _ => Heard::Unknown,
        }
    }

    fn sysex(&mut self, msg: &[u8]) -> Heard {
        let id = self.device_id();
        let Some(body) = msg.strip_prefix(&HEADER[..]) else {
            return Heard::Unknown;
        };
        match body {
            [d, 0x01, rest @ ..] if *d == id => {
                // Serial ×7, challenge ×4, F7.
                let (Some(serial), Some(Ok(challenge)), 12) = (
                    rest.get(..7),
                    rest.get(7..11).map(<[u8; 4]>::try_from),
                    rest.len(),
                ) else {
                    return Heard::Unknown;
                };
                let mut reply = HEADER.to_vec();
                reply.extend_from_slice(&[id, 0x02]);
                reply.extend_from_slice(serial);
                reply.extend_from_slice(&handshake_response(challenge));
                reply.push(0xF7);
                self.replies.push(reply);
                self.online = true;
                Heard::Known
            }
            [d, 0x03, ..] if *d == id => {
                self.online = true;
                Heard::Known
            }
            [d, 0x04, ..] if *d == id => {
                self.online = false;
                Heard::Known
            }
            _ => Heard::Unknown,
        }
    }

    fn note(&mut self, note: u8, vel: u8, now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        let press = vel > 0;
        let slot = usize::from(note & 0x07);
        match note {
            FADER_TOUCH..=MASTER_TOUCH => {
                let slot = usize::from(note.saturating_sub(FADER_TOUCH));
                self.touch(slot, vel >= 0x40, out)
            }
            REC..=0x27 => {
                let Some(s) = self.strip(slot) else {
                    return Heard::Unknown;
                };
                if !press {
                    return Heard::Known;
                }
                let id = s.id;
                match note & 0xF8 {
                    REC => out.push(SurfaceAction::Arm(id)),
                    SOLO => out.extend(click(AutomationTarget::StripSolo(id), toggled(s.solo))),
                    MUTE => out.extend(click(AutomationTarget::StripMute(id), toggled(s.mute))),
                    SELECT => out.push(SurfaceAction::Select(id)),
                    // V-pot pushes drive nothing in 5f-1.
                    VPOT_PUSH => {}
                    _ => return Heard::Unknown,
                }
                Heard::Known
            }
            ASSIGN_SEND
            | ASSIGN_PAN
            | ASSIGN_PLUGIN
            | BANK_LEFT..=CHANNEL_RIGHT
            | READ..=LATCH
            | SAVE
            | UNDO
            | CYCLE
            | REWIND..=RECORD => {
                // The global buttons are the main unit's only (Ruling 4); an
                // extender's are ignored, not counted.
                if !self.extender {
                    self.button(note, press, now, out);
                }
                Heard::Known
            }
            _ => Heard::Unknown,
        }
    }

    /// A global button of the main unit.
    fn button(&mut self, note: u8, press: bool, now: Instant, out: &mut Vec<SurfaceAction>) {
        match note {
            REWIND | FFWD => {
                let forward = note == FFWD;
                if press {
                    self.shuttle.press(forward, now);
                } else {
                    out.extend(self.shuttle.release(forward, now));
                }
                return;
            }
            _ if !press => return,
            _ => {}
        }
        let command = |id: &str| SurfaceAction::Command(id.into());
        match note {
            ASSIGN_SEND => self.assign = Assign::Send,
            ASSIGN_PAN => self.assign = Assign::Pan,
            ASSIGN_PLUGIN => self.assign = Assign::Plugin,
            BANK_LEFT => out.push(SurfaceAction::Bank(-1)),
            BANK_RIGHT => out.push(SurfaceAction::Bank(1)),
            CHANNEL_LEFT => out.push(SurfaceAction::Channel(-1)),
            CHANNEL_RIGHT => out.push(SurfaceAction::Channel(1)),
            READ => out.push(command("automation.mode.read")),
            WRITE => out.push(command("automation.mode.write")),
            TRIM => out.push(command("automation.mode.trim")),
            TOUCH => out.push(command("automation.mode.touch")),
            LATCH => out.push(command("automation.mode.latch")),
            SAVE => out.push(command("file.save")),
            UNDO => out.push(command("edit.undo")),
            CYCLE => out.push(command("transport.loop")),
            STOP => out.push(command("transport.stop")),
            PLAY => out.push(command("transport.play")),
            RECORD => out.push(command("transport.record")),
            _ => {}
        }
    }

    fn touch(&mut self, slot: usize, down: bool, out: &mut Vec<SurfaceAction>) -> Heard {
        let Some(id) = self.fader_strip(slot) else {
            return Heard::Unknown;
        };
        let Some(held) = self.touched.get_mut(slot) else {
            return Heard::Unknown;
        };
        match (down, *held) {
            (true, None) => {
                *held = Some(id);
                out.push(SurfaceAction::Gesture(
                    AutomationTarget::StripFader(id),
                    Phase::Touch(None),
                ));
            }
            (false, Some(was)) => {
                *held = None;
                out.push(SurfaceAction::Gesture(
                    AutomationTarget::StripFader(was),
                    Phase::Release,
                ));
            }
            // A repeated touch or a release of an untouched fader.
            _ => {}
        }
        Heard::Known
    }

    fn bend(
        &mut self,
        slot: usize,
        lsb: u8,
        msb: u8,
        now: Instant,
        out: &mut Vec<SurfaceAction>,
    ) -> Heard {
        let raw = u16::from(msb) << 7 | u16::from(lsb);
        let v = f64::from(raw) / 16383.0;
        if let Some(Some(id)) = self.touched.get(slot) {
            out.push(SurfaceAction::Gesture(
                AutomationTarget::StripFader(*id),
                Phase::Value(v),
            ));
            return Heard::Known;
        }
        let Some(id) = self.fader_strip(slot) else {
            return Heard::Unknown;
        };
        if let Some(Some((sent, at))) = self.sent.get(slot)
            && raw.abs_diff(*sent) <= ECHO_LSB
            && now.saturating_duration_since(*at) <= ECHO_WINDOW
        {
            // The echo of a motor move: ignored, not counted.
            return Heard::Known;
        }
        out.push(self.idle.value(AutomationTarget::StripFader(id), v, now));
        Heard::Known
    }

    fn cc(&mut self, cc: u8, v: u8, now: Instant, out: &mut Vec<SurfaceAction>) -> Heard {
        let d = detents(v);
        match cc {
            JOG => {
                if !self.extender && d != 0 {
                    out.push(SurfaceAction::Jog(d));
                }
                Heard::Known
            }
            VPOT..=0x17 => {
                let slot = usize::from(cc.saturating_sub(VPOT));
                let (target, heard) = match self.assign {
                    Assign::Pan => {
                        let Some(s) = self.strip(slot) else {
                            return Heard::Unknown;
                        };
                        (AutomationTarget::StripPan(s.id), Some(s.pan))
                    }
                    Assign::Send => {
                        let Some(s) = self.strip(slot) else {
                            return Heard::Unknown;
                        };
                        (AutomationTarget::SendLevel(s.id, SendIndex(0)), s.send)
                    }
                    Assign::Plugin => {
                        let Some(p) = self.plugin.get(slot) else {
                            return Heard::Known;
                        };
                        (p.target, Some(p.value))
                    }
                };
                // A strip without a send has nothing to drive.
                let Some(heard) = heard else {
                    return Heard::Known;
                };
                if d == 0 {
                    return Heard::Known;
                }
                let current = self.idle.last(target).unwrap_or(heard);
                let next = (current + DETENT * f64::from(d)).clamp(0.0, 1.0);
                out.push(self.idle.value(target, next, now));
                Heard::Known
            }
            _ => Heard::Unknown,
        }
    }
}

impl Surface for Mcu {
    fn kind(&self) -> SurfaceKind {
        if self.extender {
            SurfaceKind::McuXt
        } else {
            SurfaceKind::Mcu
        }
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
        let mut out = self.idle.tick(now);
        out.extend(self.shuttle.tick(now));
        out
    }

    fn feedback(&mut self, view: &MixerView, first: usize, _now: Instant) -> Vec<Vec<u8>> {
        for (k, slot) in self.shown.iter_mut().enumerate() {
            *slot = view.strips.get(first.saturating_add(k)).cloned();
        }
        self.master = if self.extender {
            None
        } else {
            view.master.clone()
        };
        self.plugin = view.plugin.clone();
        // Task 2 adds the real feedback.
        std::mem::take(&mut self.replies)
    }

    fn refresh(&mut self) {
        // Task 2: nothing is diffed yet.
    }

    fn hello(&mut self) -> Vec<Vec<u8>> {
        let mut query = HEADER.to_vec();
        query.extend_from_slice(&[self.device_id(), 0x00, 0xF7]);
        vec![query]
    }

    fn online(&self) -> bool {
        self.online
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

    fn release_all(&mut self) -> Vec<AutomationTarget> {
        let mut held = self.touched();
        for t in self.idle.take_all() {
            if !held.contains(&t) {
                held.push(t);
            }
        }
        self.touched = [None; 9];
        self.shuttle.clear();
        held
    }
}
