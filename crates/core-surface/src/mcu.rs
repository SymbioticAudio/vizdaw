//! The MCU and MCU XT protocol state machine (spec §8.2; Rulings 5, 13 and
//! 14), from public protocol descriptions.

use crate::idle::IdleTouches;
use crate::shuttle::Shuttle;
use crate::{
    Assign, ECHO_WINDOW, MixerView, ParamView, Phase, StripView, Surface, SurfaceAction,
    SurfaceKind, midi, seg,
};
use core_model::automation::{AutomationMode, AutomationTarget, SendIndex, fader_db, pan_of, unit};
use core_model::ids::StripId;
use std::collections::BTreeMap;
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
/// A fader value within this many LSBs of what was sent, less than
/// `ECHO_WINDOW` after the send, is an echo (Ruling 13).
const ECHO_LSB: u16 = 1;
/// Meters are resent at most this often (Ruling 13).
const METER_EVERY: Duration = Duration::from_millis(300);
/// The V-pot ring CCs, strips 1–8.
const RING: u8 = 0x30;
/// The timecode/BBT display's digits, CC 0x40 (rightmost) to 0x49.
const DIGITS: u8 = 0x40;
/// The assignment display's right and left characters.
const ASSIGN_RIGHT: u8 = 0x4A;
const ASSIGN_LEFT: u8 = 0x4B;
/// The scribble strips' SysEx command, and the characters per row.
const LCD: u8 = 0x12;
const ROW: usize = 56;
/// Characters per strip on a scribble-strip row: 7 and a space.
const CELL: usize = 7;
/// One V-pot detent, in normalised units (Ruling 5).
const DETENT: f64 = 0.01;

/// What became of one message.
enum Heard {
    /// Understood (taken or deliberately ignored).
    Known,
    /// Malformed or unknown: counted.
    Unknown,
}

/// What this unit was last sent, per element (Ruling 13): feedback sends
/// only what differs.
#[derive(Debug, Default)]
struct Sent {
    /// Fader values (slots 0–7, 8 the master).
    faders: [Option<u16>; 9],
    /// LEDs by note.
    leds: BTreeMap<u8, bool>,
    /// V-pot ring codes.
    rings: [Option<u8>; 8],
    /// Scribble-strip rows 1 and 2.
    rows: [Option<Vec<u8>>; 2],
    /// Timecode/BBT digits, rightmost first.
    digits: [Option<u8>; 10],
    /// The assignment display, left and right.
    assign: Option<[u8; 2]>,
    /// When the meters are next due (`None`: now).
    meters_due: Option<Instant>,
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
    /// (Ruling 13).
    sent: [Option<(u16, Instant)>; 9],
    /// What feedback last sent.
    cache: Sent,
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

/// The 14-bit pitch-bend value of normalised `v`.
fn fader_raw(v: f64) -> u16 {
    // In 0–16383 after `unit`; float-to-int `as` saturates anyway.
    (unit(v) * 16383.0).round() as u16
}

/// A V-pot ring code: `mode << 4 | pos`, `pos = 1 + round(v × 10)`; 0 (dark)
/// with no value.
fn ring(mode: u8, v: Option<f64>) -> u8 {
    v.map_or(0, |v| mode << 4 | (1 + (unit(v) * 10.0).round() as u8))
}

/// The meter level of a linear peak: 0 below −60 dBFS, else
/// `min(12, floor((dB + 60) / 5) + 1)` (Ruling 14).
fn meter_level(peak: f32) -> u8 {
    let db = 20.0 * f64::from(peak).log10();
    // Silence is −∞; a NaN peak shows nothing.
    if db.is_nan() || db < -60.0 {
        return 0;
    }
    ((db + 60.0) / 5.0).floor().clamp(0.0, 11.0) as u8 + 1
}

/// Pan text: `<L50`, `C`, `R30>` (Ruling 14).
fn pan_text(v: f64) -> String {
    let p = pan_of(v);
    let n = (p.abs() * 100.0).round();
    if n == 0.0 {
        "C".into()
    } else if p < 0.0 {
        format!("<L{n}")
    } else {
        format!("R{n}>")
    }
}

/// Send level text in dB: `-6.0`, `-inf` at the floor (Ruling 14).
fn send_text(v: f64) -> String {
    if unit(v) <= 0.0 || fader_db(v) <= core_model::mixer::GAIN_FLOOR_DB {
        "-inf".into()
    } else {
        format!("{:.1}", fader_db(v))
    }
}

/// `text` as LCD characters: printable ASCII as itself, anything else `?`.
fn lcd_chars(text: &str) -> impl Iterator<Item = u8> + '_ {
    text.chars().map(|c| {
        u8::try_from(c)
            .ok()
            .filter(|b| (0x20..=0x7E).contains(b))
            .unwrap_or(b'?')
    })
}

/// One strip's cell, left-aligned: 7 characters (truncated or padded) and a space.
fn cell_left(text: &str, row: &mut Vec<u8>) {
    let mut chars: Vec<u8> = lcd_chars(text).take(CELL).collect();
    chars.resize(CELL, b' ');
    row.extend(chars);
    row.push(b' ');
}

/// One strip's cell, centred: 7 characters and a space.
fn cell_centred(text: &str, row: &mut Vec<u8>) {
    let chars: Vec<u8> = lcd_chars(text).take(CELL).collect();
    let pad = CELL.saturating_sub(chars.len()) / 2;
    let mut cell = vec![b' '; pad];
    cell.extend(chars);
    cell.resize(CELL, b' ');
    row.extend(cell);
    row.push(b' ');
}

/// The display codes of `position`, separators turned into dots on the
/// character before them (Ruling 14); at most 10, the leftmost kept.
fn digits(position: &str) -> Vec<u8> {
    let mut codes: Vec<u8> = Vec::new();
    for c in position.chars() {
        if matches!(c, '.' | ':' | '|') {
            if let Some(last) = codes.last_mut() {
                *last |= 0x40;
            }
        } else {
            codes.push(seg(c, false));
        }
    }
    codes.truncate(10);
    codes
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
            cache: Sent::default(),
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

    /// The V-pots drive plugin parameters: the main unit in Plug-in (an
    /// extender's V-pots have no parameters to drive).
    fn plugin_mode(&self) -> bool {
        self.assign == Assign::Plugin && !self.extender
    }

    /// Sends LED `note` when it changed.
    fn led(&mut self, note: u8, on: bool, out: &mut Vec<Vec<u8>>) {
        if self.cache.leds.insert(note, on) != Some(on) {
            out.push(vec![0x90, note, if on { 0x7F } else { 0x00 }]);
        }
    }

    /// Fader values, except while touched; each sent value is kept for
    /// the echo guard.
    fn feedback_faders(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) {
        let slots = if self.extender { 8 } else { 9 };
        for slot in 0..slots {
            if self.touched.get(slot).is_some_and(Option::is_some) {
                continue;
            }
            let v = if slot == 8 {
                self.master.as_ref().map_or(0.0, |s| s.fader)
            } else {
                self.strip(slot).map_or(0.0, |s| s.fader)
            };
            let raw = fader_raw(v);
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
            let status = 0xE0 | (slot as u8 & 0x0F);
            out.push(vec![status, (raw & 0x7F) as u8, (raw >> 7) as u8]);
        }
    }

    /// Strip LEDs; on the main unit also transport, assignment and automation.
    fn feedback_leds(&mut self, view: &MixerView, out: &mut Vec<Vec<u8>>) {
        for k in 0..8u8 {
            let s = self.strip(usize::from(k));
            let (arm, solo, mute, select) = s.map_or((false, false, false, false), |s| {
                (s.arm, s.solo, s.mute, s.select)
            });
            self.led(REC + k, arm, out);
            self.led(SOLO + k, solo, out);
            self.led(MUTE + k, mute, out);
            self.led(SELECT + k, select, out);
        }
        if self.extender {
            return;
        }
        self.led(PLAY, view.playing, out);
        self.led(STOP, !view.playing, out);
        self.led(RECORD, view.recording, out);
        self.led(CYCLE, view.looping, out);
        let a = self.assign;
        self.led(ASSIGN_SEND, a == Assign::Send, out);
        self.led(ASSIGN_PAN, a == Assign::Pan, out);
        self.led(ASSIGN_PLUGIN, a == Assign::Plugin, out);
        // The first selected strip's mode (Off lights none).
        let mode = view
            .strips
            .iter()
            .chain(view.master.as_ref())
            .find(|s| s.select)
            .map(|s| s.mode);
        for (note, m) in [
            (READ, AutomationMode::Read),
            (WRITE, AutomationMode::Write),
            (TRIM, AutomationMode::Trim),
            (TOUCH, AutomationMode::Touch),
            (LATCH, AutomationMode::Latch),
        ] {
            self.led(note, mode == Some(m), out);
        }
    }

    /// V-pot rings: pan in mode 1, send in mode 2, plugin in mode 0.
    fn feedback_rings(&mut self, out: &mut Vec<Vec<u8>>) {
        for k in 0..8u8 {
            let slot = usize::from(k);
            let code = match self.assign {
                Assign::Pan => ring(1, self.strip(slot).map(|s| s.pan)),
                Assign::Send => ring(2, self.strip(slot).and_then(|s| s.send)),
                Assign::Plugin if self.plugin_mode() => {
                    ring(0, self.plugin.get(slot).map(|p| p.value))
                }
                Assign::Plugin => 0,
            };
            let Some(cached) = self.cache.rings.get_mut(slot) else {
                continue;
            };
            if *cached != Some(code) {
                *cached = Some(code);
                out.push(vec![0xB0, RING + k, code]);
            }
        }
    }

    /// The scribble strips: names (or parameter names) and value texts.
    fn feedback_rows(&mut self, out: &mut Vec<Vec<u8>>) {
        let mut names = Vec::with_capacity(ROW);
        let mut values = Vec::with_capacity(ROW);
        for slot in 0..8 {
            let (name, value) = if self.plugin_mode() {
                self.plugin
                    .get(slot)
                    .map_or((String::new(), String::new()), |p| {
                        (p.name.clone(), p.text.clone())
                    })
            } else {
                match self.strip(slot) {
                    None => (String::new(), String::new()),
                    Some(s) => {
                        let value = match self.assign {
                            Assign::Pan => pan_text(s.pan),
                            Assign::Send => s.send.map(send_text).unwrap_or_default(),
                            // An extender in Plug-in has no values to show.
                            Assign::Plugin => String::new(),
                        };
                        (s.name.clone(), value)
                    }
                }
            };
            cell_left(&name, &mut names);
            cell_centred(&value, &mut values);
        }
        // Eight cells of 8 are 64 characters; a row holds 56, so the
        // eighth strip's cell does not fit (see the Task 2 report).
        names.truncate(ROW);
        values.truncate(ROW);
        let id = self.device_id();
        for (r, row) in [names, values].into_iter().enumerate() {
            let Some(cached) = self.cache.rows.get_mut(r) else {
                continue;
            };
            if cached.as_ref() == Some(&row) {
                continue;
            }
            let mut msg = HEADER.to_vec();
            msg.extend_from_slice(&[id, LCD, if r == 0 { 0 } else { ROW as u8 }]);
            msg.extend_from_slice(&row);
            msg.push(0xF7);
            *cached = Some(row);
            out.push(msg);
        }
    }

    /// The timecode/BBT and assignment displays (main unit only).
    fn feedback_displays(&mut self, view: &MixerView, out: &mut Vec<Vec<u8>>) {
        let codes = digits(&view.position);
        for i in 0..10u8 {
            // CC 0x40 is the rightmost digit: right-aligned.
            let code = usize::from(i)
                .checked_add(1)
                .and_then(|n| codes.len().checked_sub(n))
                .and_then(|j| codes.get(j))
                .copied()
                .unwrap_or_else(|| seg(' ', false));
            let Some(cached) = self.cache.digits.get_mut(usize::from(i)) else {
                continue;
            };
            if *cached != Some(code) {
                *cached = Some(code);
                out.push(vec![0xB0, DIGITS + i, code]);
            }
        }
        let text = match self.assign {
            Assign::Pan => ['P', 'N'],
            Assign::Send => ['S', 'E'],
            Assign::Plugin => ['P', 'L'],
        };
        let codes = text.map(|c| seg(c, false));
        // The two characters are one element: both go when either changes.
        if self.cache.assign != Some(codes) {
            self.cache.assign = Some(codes);
            let [left, right] = codes;
            out.push(vec![0xB0, ASSIGN_LEFT, left]);
            out.push(vec![0xB0, ASSIGN_RIGHT, right]);
        }
    }

    /// Meters for every shown strip, at most every METER_EVERY.
    fn feedback_meters(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) {
        if self.cache.meters_due.is_some_and(|due| now < due) {
            return;
        }
        self.cache.meters_due = now.checked_add(METER_EVERY);
        for k in 0..8u8 {
            if let Some(s) = self.strip(usize::from(k)) {
                let level = meter_level(s.meter.0.max(s.meter.1));
                out.push(vec![0xD0, k << 4 | level]);
            }
        }
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
        // A touch-up releases the strip that was touched, whatever the slot
        // shows now: a bank change or a deleted strip may have emptied it.
        if !down && let Some(was) = self.touched.get_mut(slot).and_then(Option::take) {
            // The next feedback sends this fader once (motor back to the heard value).
            if let Some(cached) = self.cache.faders.get_mut(slot) {
                *cached = None;
            }
            out.push(SurfaceAction::Gesture(
                AutomationTarget::StripFader(was),
                Phase::Release,
            ));
            return Heard::Known;
        }
        let Some(id) = self.fader_strip(slot) else {
            return Heard::Unknown;
        };
        let Some(held) = self.touched.get_mut(slot) else {
            return Heard::Unknown;
        };
        // A repeated touch or a release of an untouched fader does nothing.
        if down && held.is_none() {
            *held = Some(id);
            let t = AutomationTarget::StripFader(id);
            // The touch owns the gesture now: an idle hold opened by a move
            // before the touch note must not release it (Ruling 2).
            self.idle.forget(t);
            out.push(SurfaceAction::Gesture(t, Phase::Touch(None)));
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
            && now.saturating_duration_since(*at) < ECHO_WINDOW
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

    fn feedback(&mut self, view: &MixerView, first: usize, now: Instant) -> Vec<Vec<u8>> {
        for (k, slot) in self.shown.iter_mut().enumerate() {
            *slot = view.strips.get(first.saturating_add(k)).cloned();
        }
        self.master = if self.extender {
            None
        } else {
            view.master.clone()
        };
        self.plugin = view.plugin.clone();
        let mut out = std::mem::take(&mut self.replies);
        self.feedback_faders(now, &mut out);
        self.feedback_leds(view, &mut out);
        self.feedback_rings(&mut out);
        self.feedback_rows(&mut out);
        if !self.extender {
            self.feedback_displays(view, &mut out);
        }
        self.feedback_meters(now, &mut out);
        out
    }

    fn refresh(&mut self) {
        self.cache = Sent::default();
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
        // A formerly touched fader gets feedback again.
        for (slot, t) in self.touched.iter().enumerate() {
            if t.is_some()
                && let Some(cached) = self.cache.faders.get_mut(slot)
            {
                *cached = None;
            }
        }
        self.touched = [None; 9];
        self.shuttle.clear();
        held
    }

    fn assign(&self) -> Option<Assign> {
        Some(self.assign)
    }

    fn set_assign(&mut self, assign: Assign) {
        self.assign = assign;
    }
}
