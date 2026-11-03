//! The Kontrol S-Series MK3 in DAW mode (plan 5f-4; spec §8.4): the wire
//! codec and the state machine behind the [`Surface`] trait.
//!
//! Protocol facts from the public DrivenByMoss sources (LGPL-3.0,
//! github.com/git-moss/DrivenByMoss), cross-checked against ReaKontrol and
//! flmidi; no code copied.
//!
//! A SysEx message is `HEADER, cmd, value, index, info…, F7`; every other
//! message is a control change on MIDI channel 16 (status `BF`). The host
//! says hello with `BF 01 04`; the keyboard answers `BF 01 <version>` and
//! says goodbye with `BF 02 xx`.

use crate::idle::IdleTouches;
use crate::mcu::{click, toggled};
use crate::{
    Held, Link, MixerView, PluginPageView, StripView, Surface, SurfaceAction, SurfaceKind,
    TrackKind, Unit, midi,
};
use core_model::automation::{AutomationTarget, fader_db, pan_of, unit};
use core_model::ids::StripId;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// NI's SysEx header for the DAW protocol.
pub const HEADER: [u8; 10] = [0xF0, 0x00, 0x21, 0x09, 0x00, 0x00, 0x44, 0x43, 0x01, 0x00];
/// The protocol version VizDAW asks for and speaks.
pub const PROTOCOL: u8 = 4;
/// How often the hello is repeated until the keyboard answers.
pub const HELLO_EVERY: Duration = Duration::from_secs(2);
/// Display text is at most this many bytes (Ruling 8).
pub const MAX_TEXT: usize = 32;
/// One relative step of a knob, in normalised units (Ruling 5).
pub const KNOB_STEP: f64 = 0.005;
/// Tempo messages less than this apart are one gesture (Ruling 5).
pub const TEMPO_RUN: Duration = Duration::from_millis(500);
/// A tap more than this after the last starts a new series (Ruling 5).
pub const TAP_RESET: Duration = Duration::from_secs(2);
/// The name VizDAW gives itself (Ruling 4).
pub const IDENTITY_NAME: &str = "Generic";
/// The DAW version sent with it.
pub const IDENTITY_VERSION: (u8, u8) = (1, 0);

/// The control-change status of every non-SysEx message (channel 16).
const CC: u8 = 0xBF;
/// The handshake and goodbye controllers.
const HANDSHAKE: u8 = 0x01;
const GOODBYE: u8 = 0x02;
/// The identity command (DAW name and version).
const CMD_IDENTITY: u8 = 0x07;
/// The VU command.
const CMD_VU: u8 = 0x49;
/// Track buttons (SysEx): select, mute, solo, arm.
const CMD_SELECT: u8 = 0x42;
const CMD_MUTE: u8 = 0x43;
const CMD_SOLO: u8 = 0x44;
const CMD_ARM: u8 = 0x45;
/// Per-slot display commands (SysEx): available/type, name, volume text,
/// pan text, colour, track instance, muted-by-solo.
const CMD_TYPE: u8 = 0x40;
const CMD_INSTANCE: u8 = 0x41;
const CMD_NAME: u8 = 0x48;
const CMD_VOLUME: u8 = 0x46;
const CMD_PAN: u8 = 0x47;
const CMD_COLOR: u8 = 0x4B;
const CMD_MUTED_BY_SOLO: u8 = 0x4A;
/// Plug-in mode's chain, parameter names and value texts, page name and
/// preset name (SysEx).
const CMD_CHAIN: u8 = 0x71;
const CMD_PARAM_NAME: u8 = 0x72;
const CMD_PARAM_TEXT: u8 = 0x73;
const CMD_PAGE_NAME: u8 = 0x75;
const CMD_PRESET_NAME: u8 = 0x76;
/// The tempo (SysEx, both ways).
const CMD_TEMPO: u8 = 0x19;
/// The chain's soft buttons and the page buttons (SysEx).
const CMD_SLOT: u8 = 0x70;
const CMD_PAGE: u8 = 0x74;
/// Strips on screen.
const SLOTS: usize = 8;
/// Taps kept for the tempo (four intervals).
const TAPS: usize = 5;
/// The tempo is a quarter note in units of 10 ns: 6e9 / bpm.
const TEMPO_UNITS: f64 = 6e9;
/// What five 7-bit bytes hold.
const TEMPO_LIMIT: f64 = 34_359_738_368.0;

/// `HEADER, cmd, value, index, info…, F7`; `value` and `index` above 0x7F
/// are sent as 0x7F (pre-flight I3).
pub fn sysex(cmd: u8, value: u8, index: u8, info: &[u8]) -> Vec<u8> {
    let mut m = HEADER.to_vec();
    m.extend_from_slice(&[cmd, value.min(0x7F), index.min(0x7F)]);
    m.extend_from_slice(info);
    m.push(0xF7);
    m
}

/// `(cmd, value, index, info)` of a whole, 7-bit-clean message with
/// [`HEADER`]; else `None`.
pub fn parse_sysex(m: &[u8]) -> Option<(u8, u8, u8, &[u8])> {
    let n = HEADER.len();
    if m.len() < n.saturating_add(4) || !m.starts_with(&HEADER) || m.last() != Some(&0xF7) {
        return None;
    }
    let body = m.get(1..m.len().saturating_sub(1))?;
    if body.iter().any(|b| *b > 0x7F) {
        return None;
    }
    let cmd = *m.get(n)?;
    let value = *m.get(n.saturating_add(1))?;
    let index = *m.get(n.saturating_add(2))?;
    let info = m.get(n.saturating_add(3)..m.len().saturating_sub(1))?;
    Some((cmd, value, index, info))
}

/// Printable ASCII kept, every other character `?`, at most [`MAX_TEXT`]
/// bytes.
pub fn ascii(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            if (' '..='~').contains(&c) {
                u8::try_from(c).unwrap_or(b'?')
            } else {
                b'?'
            }
        })
        .take(MAX_TEXT)
        .collect()
}

/// `round(6e9 / bpm)` in five 7-bit bytes, least significant first; `None`
/// when `bpm` is not finite and positive or the value does not fit 35 bits
/// (or rounds to zero, which [`tempo_bpm`] refuses too).
pub fn tempo_bytes(bpm: f64) -> Option<[u8; 5]> {
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let units = (TEMPO_UNITS / bpm).round();
    if !(1.0..TEMPO_LIMIT).contains(&units) {
        return None;
    }
    // In range, so the conversion is exact.
    let mut v = units as u64;
    let mut out = [0u8; 5];
    for b in &mut out {
        *b = u8::try_from(v & 0x7F).unwrap_or(0);
        v >>= 7;
    }
    Some(out)
}

/// The inverse; `None` for a byte above 0x7F or a zero length.
pub fn tempo_bpm(b: [u8; 5]) -> Option<f64> {
    if b.iter().any(|x| *x > 0x7F) {
        return None;
    }
    let v = b
        .iter()
        .rev()
        .fold(0u64, |acc, x| (acc << 7) | u64::from(*x));
    if v == 0 {
        return None;
    }
    Some(TEMPO_UNITS / v as f64)
}

/// 1–63 → +v, 65–127 → v − 128, 0 and 64 → 0 (and anything above 127 → 0).
pub fn relative(v: u8) -> i32 {
    match v {
        1..=63 => i32::from(v),
        65..=127 => i32::from(v) - 128,
        _ => 0,
    }
}

/// Whether a port name is a Kontrol S-Series MK3's DAW port (Ruling 3): its
/// words, split at every character that is not a letter or digit, include
/// `KONTROL`, `MK3` and `DAW`.
pub fn is_daw_port(name: &str) -> bool {
    let up = name.to_uppercase();
    let words: Vec<&str> = up
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    ["KONTROL", "MK3", "DAW"].iter().all(|w| words.contains(w))
}

/// `x` as a 7-bit value (at most 127).
fn b7(x: usize) -> u8 {
    u8::try_from(x.min(127)).unwrap_or(127)
}

/// A ring or light value: `round(v × 127)`, 0–127.
fn ring(v: f64) -> u8 {
    // Clamped to 0..=127, so the conversion is exact.
    (unit(v) * 127.0).round().clamp(0.0, 127.0) as u8
}

/// A volume text of at most 8 characters: "-6.1 dB", "-inf dB" below
/// −99.95 dB, never "-0.0".
fn volume_text(fader: f64) -> String {
    let db = fader_db(fader);
    if unit(fader) <= 0.0 || db < -99.95 {
        return "-inf dB".into();
    }
    let t = format!("{db:.1}");
    if t == "-0.0" {
        "0.0 dB".into()
    } else {
        format!("{t} dB")
    }
}

/// A pan text: "C", "L50", "R100".
fn pan_text(pan: f64) -> String {
    let p = pan_of(pan);
    let n = (p.abs() * 100.0).round();
    if n == 0.0 {
        "C".into()
    } else if p < 0.0 {
        format!("L{n}")
    } else {
        format!("R{n}")
    }
}

/// The messages of one feedback, sent only when they differ from the last
/// with their key (Ruling 8).
struct Sender<'a> {
    cache: &'a mut BTreeMap<Vec<u8>, Vec<u8>>,
    out: Vec<Vec<u8>>,
}

impl Sender<'_> {
    fn put(&mut self, key: Vec<u8>, msg: Vec<u8>) {
        if self.cache.get(&key) != Some(&msg) {
            self.cache.insert(key, msg.clone());
            self.out.push(msg);
        }
    }

    fn cc(&mut self, cc: u8, v: u8) {
        self.put(vec![CC, cc], vec![CC, cc, v.min(0x7F)]);
    }

    /// Keyed by the command and the index.
    fn sx(&mut self, cmd: u8, value: u8, index: u8, info: &[u8]) {
        let m = sysex(cmd, value, index, info);
        self.put(vec![cmd, index.min(0x7F)], m);
    }

    /// Keyed by the command alone (the index or value is the datum).
    fn one(&mut self, cmd: u8, value: u8, index: u8, info: &[u8]) {
        let m = sysex(cmd, value, index, info);
        self.put(vec![cmd, 0], m);
    }
}

/// A TRACK_VU message (counted as a meter).
pub fn is_meter(m: &[u8]) -> bool {
    parse_sysex(m).is_some_and(|(cmd, ..)| cmd == CMD_VU)
}

/// Ruling 8's meter scale: `round(127 × (dB + 60) / 60)` clamped to 0–127;
/// 0 for silence, a NaN or a negative peak.
pub fn vu(peak: f32) -> u8 {
    if peak.is_nan() || peak <= 0.0 {
        return 0;
    }
    let db = 20.0 * f64::from(peak).log10();
    let v = (127.0 * (db + 60.0) / 60.0).round().clamp(0.0, 127.0);
    // Clamped to 0..=127, so the conversion is exact.
    v as u8
}

/// Ruling 8's colour of a kind, `"#RRGGBB"`.
pub fn color(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Instrument => "#3C8CE6",
        TrackKind::Audio => "#46B45A",
        TrackKind::Bus => "#E6963C",
        TrackKind::Master => "#B4B4B4",
    }
}

/// Ruling 8's track type (`None`: an empty slot).
pub fn track_type(kind: Option<TrackKind>) -> u8 {
    match kind {
        None => 0,
        Some(TrackKind::Instrument) => 2,
        Some(TrackKind::Audio) => 3,
        Some(TrackKind::Bus) => 5,
        Some(TrackKind::Master) => 6,
    }
}

/// The keyboard's mode (CC 0x05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// The mixer of eight tracks.
    #[default]
    Mixer,
    /// VizDAW's plug-in pages.
    Plugin,
}

/// A Kontrol S-Series MK3 on its DAW port (plan 5f-4).
pub struct Kontrol {
    link: Link,
    idle: IdleTouches,
    mode: Mode,
    /// The strip IDs of the last feedback's view, in display order.
    ids: Vec<StripId>,
    /// The index in `ids` of the selected strip, if any.
    selected: Option<usize>,
    /// The first strip the last feedback showed.
    first: usize,
    /// The (up to 8) strips the last feedback showed.
    shown: Vec<StripView>,
    /// The plug-in page the last feedback showed (kept in Plug-in mode only).
    page: Option<PluginPageView>,
    /// The time and kind (`true`: a tap) of the last tempo action.
    last_tempo: Option<(Instant, bool)>,
    /// The taps of the current series (at most [`TAPS`]).
    taps: Vec<Instant>,
    /// The five tempo bytes last sent.
    sent_tempo: Option<[u8; 5]>,
    /// What was last sent, by key (Ruling 8).
    cache: BTreeMap<Vec<u8>, Vec<u8>>,
    /// `hello` was sent; the next feedback starts the repeat timer.
    hello_sent: bool,
    /// When the hello goes again (`None`: not waiting for an answer).
    next_hello: Option<Instant>,
    /// The identity is due with the next online feedback.
    identity_due: bool,
    /// A problem found, said once.
    problem: Option<String>,
    ignored: u64,
}

impl Default for Kontrol {
    fn default() -> Kontrol {
        Kontrol::new()
    }
}

impl Kontrol {
    /// A new unit, offline until it answers the hello.
    pub fn new() -> Kontrol {
        Kontrol {
            link: Link::default(),
            idle: IdleTouches::default(),
            mode: Mode::default(),
            ids: Vec::new(),
            selected: None,
            first: 0,
            shown: Vec::new(),
            page: None,
            last_tempo: None,
            taps: Vec::new(),
            sent_tempo: None,
            cache: BTreeMap::new(),
            hello_sent: false,
            next_hello: None,
            identity_due: false,
            problem: None,
            ignored: 0,
        }
    }

    /// The keyboard's mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    /// The strip shown in slot `k`.
    fn slot(&self, k: usize) -> Option<&StripView> {
        self.shown.get(k)
    }

    /// The first shown strip that is selected.
    fn selected_shown(&self) -> Option<&StripView> {
        self.shown.iter().find(|s| s.select)
    }

    /// A relative move of `target` from `shown` (or its open idle touch's
    /// last value), by `steps` knob steps.
    fn knob(
        &mut self,
        target: AutomationTarget,
        shown: f64,
        steps: i32,
        now: Instant,
        out: &mut Vec<SurfaceAction>,
    ) {
        let current = self.idle.last(target).unwrap_or(shown);
        let next = (current + f64::from(steps) * KNOB_STEP).clamp(0.0, 1.0);
        out.push(self.idle.value(target, next, now));
    }

    /// A volume (`pan` false) or pan knob of the strip in `strip`.
    fn strip_knob(
        &mut self,
        strip: Option<StripView>,
        pan: bool,
        steps: i32,
        now: Instant,
        out: &mut Vec<SurfaceAction>,
    ) {
        if steps == 0 {
            return;
        }
        let Some(s) = strip else {
            self.ignore();
            return;
        };
        if pan {
            self.knob(AutomationTarget::StripPan(s.id), s.pan, steps, now, out);
        } else {
            self.knob(AutomationTarget::StripFader(s.id), s.fader, steps, now, out);
        }
    }

    /// The previous or next strip, from the selected one (else the first
    /// shown); the bank follows when it leaves the screen.
    fn navigate(&mut self, steps: i32, out: &mut Vec<SurfaceAction>) {
        let next = match (self.selected, steps.signum()) {
            (_, 0) => return,
            (Some(i), 1) => i.checked_add(1),
            (Some(i), _) => i.checked_sub(1),
            (None, _) => Some(self.first),
        };
        let Some(idx) = next else { return };
        let Some(&id) = self.ids.get(idx) else { return };
        out.push(SurfaceAction::Select(id));
        if self.selected.is_none() {
            return;
        }
        let top = self.first.saturating_add(SLOTS - 1);
        let by = if idx < self.first {
            i32::try_from(self.first - idx).ok().map(|d| -d)
        } else if idx > top {
            i32::try_from(idx - top).ok()
        } else {
            None
        };
        if let Some(d) = by {
            out.push(SurfaceAction::Channel(d));
        }
    }

    /// A tempo from the keyboard's control.
    fn tempo(&mut self, info: &[u8], now: Instant, out: &mut Vec<SurfaceAction>) {
        let Some(bytes) = info.get(..5).and_then(|b| <[u8; 5]>::try_from(b).ok()) else {
            self.ignore();
            return;
        };
        let Some(bpm) = tempo_bpm(bytes) else {
            self.ignore();
            return;
        };
        if self.sent_tempo == Some(bytes) {
            // The keyboard's echo of what VizDAW sent.
            return;
        }
        // The next feedback sends VizDAW's tempo again.
        self.sent_tempo = None;
        self.cache.remove(&vec![CMD_TEMPO, 0]);
        let new_step = self.tempo_step(now, false);
        out.push(SurfaceAction::Tempo { bpm, new_step });
    }

    /// Whether a tempo action at `now` starts a gesture (`tap`: it is a
    /// tap's); records it as the last.
    fn tempo_step(&mut self, now: Instant, tap: bool) -> bool {
        let new_step = match self.last_tempo {
            None => true,
            Some((_, true)) => true,
            Some((at, false)) => now.saturating_duration_since(at) >= TEMPO_RUN,
        };
        self.last_tempo = Some((now, tap));
        new_step
    }

    /// Tap Tempo pressed at `now`.
    fn tap(&mut self, now: Instant, out: &mut Vec<SurfaceAction>) {
        if self
            .taps
            .last()
            .is_some_and(|&t| now.saturating_duration_since(t) > TAP_RESET)
        {
            self.taps.clear();
        }
        self.taps.push(now);
        if self.taps.len() > TAPS {
            self.taps.remove(0);
        }
        let (Some(&first), Some(&last)) = (self.taps.first(), self.taps.last()) else {
            return;
        };
        let intervals = self.taps.len().saturating_sub(1);
        if intervals == 0 {
            return;
        }
        let mean = last.saturating_duration_since(first).as_secs_f64() / intervals as f64;
        if mean <= 0.0 {
            return;
        }
        let bpm = (600.0 / mean).round() / 10.0;
        let _ = self.tempo_step(now, true);
        out.push(SurfaceAction::Tempo {
            bpm,
            new_step: intervals == 1,
        });
    }

    /// A control change other than the handshake.
    fn control(&mut self, cc: u8, v: u8, now: Instant, out: &mut Vec<SurfaceAction>) {
        let cmd = |s: &str| SurfaceAction::Command(s.into());
        let press = v > 0;
        let steps = relative(v);
        match cc {
            0x10..=0x14 | 0x16 | 0x20 | 0x21 | 0x23 => {
                if !press {
                    return;
                }
                match cc {
                    0x10 => out.push(cmd("transport.play")),
                    0x11 => {
                        out.push(cmd("transport.return_to_start"));
                        out.push(cmd("transport.play"));
                    }
                    0x12 => out.push(cmd("transport.record")),
                    0x13 => out.push(SurfaceAction::CountIn),
                    0x14 => out.push(cmd("transport.stop")),
                    0x16 => out.push(cmd("transport.loop")),
                    0x20 => out.push(cmd("edit.undo")),
                    0x21 => out.push(cmd("edit.redo")),
                    _ => out.push(cmd("automation.suspend")),
                }
            }
            0x18 => {
                if press {
                    self.tap(now, out);
                }
            }
            // Clear, Metro, Quantize, Shift, clip and scene navigation and
            // Move Loop: no VizDAW counterpart, and no fault.
            0x04 | 0x15 | 0x17 | 0x22 | 0x32 | 0x33 | 0x35 => {}
            0x05 => match v {
                0 | 1 => {
                    let mode = if v == 0 { Mode::Mixer } else { Mode::Plugin };
                    if mode != self.mode {
                        self.mode = mode;
                        if mode == Mode::Mixer {
                            self.page = None;
                        }
                        self.refresh();
                    }
                }
                _ => self.ignore(),
            },
            0x30 => self.navigate(steps, out),
            0x31 => {
                if steps != 0 {
                    let n = steps.signum();
                    out.push(match self.mode {
                        Mode::Mixer => SurfaceAction::Bank(n),
                        Mode::Plugin => SurfaceAction::PluginPage(n),
                    });
                }
            }
            0x34 => {
                if steps != 0 {
                    out.push(SurfaceAction::Jog(steps));
                }
            }
            0x36 => {
                if steps != 0 {
                    out.push(SurfaceAction::Preset(steps.signum()));
                }
            }
            0x50..=0x5F => {
                let k = usize::from(cc - 0x50);
                let strip = self.slot(k % SLOTS).cloned();
                self.strip_knob(strip, k >= SLOTS, steps, now, out);
            }
            0x64 | 0x65 => {
                let strip = self.selected_shown().cloned();
                self.strip_knob(strip, cc == 0x65, steps, now, out);
            }
            0x66 | 0x67 => {
                if !press {
                    return;
                }
                match self.selected_shown() {
                    Some(s) if cc == 0x66 => {
                        out.extend(click(AutomationTarget::StripMute(s.id), toggled(s.mute)))
                    }
                    Some(s) => {
                        out.extend(click(AutomationTarget::StripSolo(s.id), toggled(s.solo)))
                    }
                    None => self.ignore(),
                }
            }
            0x70..=0x77 => {
                if steps == 0 && self.mode == Mode::Plugin {
                    return;
                }
                let k = usize::from(cc - 0x70);
                let param = match self.mode {
                    Mode::Plugin => self
                        .page
                        .as_ref()
                        .and_then(|p| p.params.get(k))
                        .map(|p| (p.target, p.value)),
                    Mode::Mixer => None,
                };
                match param {
                    Some((t, shown)) => self.knob(t, shown, steps, now, out),
                    None => self.ignore(),
                }
            }
            _ => self.ignore(),
        }
    }

    /// A SysEx message from the keyboard.
    fn command(
        &mut self,
        (cmd, _value, index, info): (u8, u8, u8, &[u8]),
        now: Instant,
        out: &mut Vec<SurfaceAction>,
    ) {
        let slot = usize::from(index);
        match cmd {
            CMD_SELECT | CMD_MUTE | CMD_SOLO | CMD_ARM => {
                let Some(s) = self.slot(slot) else {
                    self.ignore();
                    return;
                };
                let id = s.id;
                match cmd {
                    CMD_SELECT => out.push(SurfaceAction::Select(id)),
                    CMD_MUTE => out.extend(click(AutomationTarget::StripMute(id), toggled(s.mute))),
                    CMD_SOLO => out.extend(click(AutomationTarget::StripSolo(id), toggled(s.solo))),
                    _ => out.push(SurfaceAction::Arm(id)),
                }
            }
            CMD_TEMPO => self.tempo(info, now, out),
            CMD_SLOT => match &self.page {
                Some(p) if slot < p.chain.len() => out.push(SurfaceAction::PluginSlot(slot)),
                _ => self.ignore(),
            },
            CMD_PAGE => match self.page.as_ref().map(|p| (p.page, p.pages)) {
                Some((shown, pages)) if slot < pages => {
                    if slot != shown {
                        let d = i32::try_from(slot)
                            .unwrap_or(i32::MAX)
                            .saturating_sub(i32::try_from(shown).unwrap_or(0));
                        out.push(SurfaceAction::PluginPage(d));
                    }
                }
                _ => self.ignore(),
            },
            _ => self.ignore(),
        }
    }

    /// The keyboard's answer `BF 01 v` (Ruling 4).
    fn answer(&mut self, v: u8, now: Instant, out: &mut Vec<SurfaceAction>) {
        if v >= PROTOCOL {
            out.extend(self.set_online(true));
            // The next feedback is the identity and the full state.
            self.refresh();
            self.identity_due = true;
            self.next_hello = None;
            self.hello_sent = false;
        } else {
            if self.link.get() {
                out.extend(self.set_online(false));
                self.next_hello = Some(now + HELLO_EVERY);
            }
            self.identity_due = false;
            self.problem = Some(format!(
                "the keyboard answered protocol version {v}; VizDAW needs version {PROTOCOL} (keyboard firmware 2.0 or later)"
            ));
        }
    }

    /// Everything the keyboard shows, into `tx` (Ruling 8).
    fn send_state(&self, view: &MixerView, tx: &mut Sender) -> Option<[u8; 5]> {
        // Per slot.
        for k in 0..SLOTS {
            let slot = b7(k);
            let s = self.shown.get(k);
            let kind = s.map(|s| s.kind);
            let flag = |on: bool| u8::from(on);
            tx.sx(CMD_TYPE, track_type(kind), slot, &[]);
            tx.sx(
                CMD_NAME,
                0,
                slot,
                &s.map(|s| ascii(&s.name)).unwrap_or_default(),
            );
            tx.sx(
                CMD_VOLUME,
                0,
                slot,
                s.map(|s| volume_text(s.fader))
                    .unwrap_or_default()
                    .as_bytes(),
            );
            tx.sx(
                CMD_PAN,
                0,
                slot,
                s.map(|s| pan_text(s.pan)).unwrap_or_default().as_bytes(),
            );
            tx.sx(CMD_SELECT, flag(s.is_some_and(|s| s.select)), slot, &[]);
            tx.sx(CMD_MUTE, flag(s.is_some_and(|s| s.mute)), slot, &[]);
            tx.sx(CMD_SOLO, flag(s.is_some_and(|s| s.solo)), slot, &[]);
            tx.sx(CMD_ARM, flag(s.is_some_and(|s| s.arm)), slot, &[]);
            tx.sx(
                CMD_MUTED_BY_SOLO,
                flag(view.soloing && s.is_some_and(|s| !s.solo)),
                slot,
                &[],
            );
            tx.sx(
                CMD_COLOR,
                0,
                slot,
                kind.map(|k| color(k).as_bytes()).unwrap_or_default(),
            );
            tx.cc(0x50 + slot, s.map_or(0, |s| ring(s.fader)));
            tx.cc(0x58 + slot, s.map_or(0, |s| ring(s.pan)));
        }
        // Once per feedback.
        let mut levels: Vec<u8> = self
            .shown
            .iter()
            .flat_map(|s| [vu(s.meter.0), vu(s.meter.1)])
            .collect();
        levels.resize(2 * SLOTS, 0);
        tx.one(CMD_VU, 2, 0, &levels);
        tx.one(CMD_INSTANCE, 0, 0, &[]);
        let mut tempo = None;
        if view.tempo > 0.0 {
            if let Some(b) = tempo_bytes(view.tempo) {
                tx.one(CMD_TEMPO, 0, 0, &b);
                tempo = Some(b);
            }
        }
        // Transport.
        let on = u8::from;
        tx.cc(0x10, on(view.playing));
        tx.cc(0x14, on(!view.playing));
        tx.cc(0x12, on(view.recording));
        tx.cc(0x16, on(view.looping));
        tx.cc(0x11, 1);
        tx.cc(0x20, 1);
        tx.cc(0x21, 1);
        tx.cc(0x13, on(view.count_in));
        tx.cc(0x23, on(view.automation_suspended));
        tx.cc(0x18, on(!view.recording));
        tx.cc(0x17, 0);
        tx.cc(0x22, 0);
        // Navigation: bit 0 previous, bit 1 next.
        let mask = |prev: bool, next: bool| u8::from(prev) | (u8::from(next) << 1);
        let n = self.ids.len();
        tx.cc(
            0x30,
            match self.selected {
                Some(i) => mask(i > 0, i.saturating_add(1) < n),
                None => mask(false, n > 0),
            },
        );
        let page = self.page.as_ref();
        tx.cc(
            0x31,
            match (self.mode, page) {
                (Mode::Mixer, _) => mask(self.first > 0, self.first.saturating_add(SLOTS) < n),
                (Mode::Plugin, Some(p)) => mask(p.page > 0, p.page.saturating_add(1) < p.pages),
                (Mode::Plugin, None) => 0,
            },
        );
        tx.cc(0x32, 0);
        tx.cc(0x33, 0);
        tx.cc(
            0x36,
            match page {
                Some(p) => match p.preset {
                    Some(i) => mask(i > 0, i.saturating_add(1) < p.presets),
                    None => mask(p.presets > 0, p.presets > 0),
                },
                None => 0,
            },
        );
        // The selected track.
        let sel = self.selected_shown();
        tx.cc(0x68, track_type(sel.map(|s| s.kind)));
        tx.cc(0x66, u8::from(sel.is_some_and(|s| s.mute)));
        tx.cc(0x67, u8::from(sel.is_some_and(|s| s.solo)));
        tx.cc(0x69, u8::from(view.soloing && sel.is_some_and(|s| !s.solo)));
        tx.cc(
            0x05,
            match self.mode {
                Mode::Mixer => 0,
                Mode::Plugin => 1,
            },
        );
        if self.mode == Mode::Plugin {
            self.send_page(tx);
        }
        tempo
    }

    /// Plug-in mode's chain, parameters, page and preset.
    fn send_page(&self, tx: &mut Sender) {
        let mut chain = Vec::new();
        match &self.page {
            Some(p) => {
                for name in &p.chain {
                    chain.extend(ascii(name));
                    chain.push(0);
                }
                chain.push(0);
            }
            None => chain.push(0),
        }
        tx.one(CMD_CHAIN, 0, 0, &chain);
        let p = self.page.as_ref();
        tx.one(CMD_SLOT, 0, p.map_or(0, |p| b7(p.slot)), &[]);
        for k in 0..SLOTS {
            let param = p.and_then(|p| p.params.get(k));
            let i = b7(k);
            tx.sx(
                CMD_PARAM_NAME,
                0,
                i,
                &param.map(|q| ascii(&q.name)).unwrap_or_default(),
            );
            tx.sx(
                CMD_PARAM_TEXT,
                0,
                i,
                &param.map(|q| ascii(&q.text)).unwrap_or_default(),
            );
            tx.cc(0x70 + i, param.map_or(0, |q| ring(q.value)));
        }
        tx.one(
            CMD_PAGE,
            p.map_or(0, |p| b7(p.pages)),
            p.map_or(0, |p| b7(p.page)),
            &[],
        );
        tx.one(
            CMD_PAGE_NAME,
            0,
            0,
            &p.map(|p| ascii(&p.page_name)).unwrap_or_default(),
        );
        tx.one(
            CMD_PRESET_NAME,
            0,
            0,
            &p.map(|p| ascii(&p.preset_name)).unwrap_or_default(),
        );
    }

    fn message(&mut self, msg: &[u8], now: Instant, out: &mut Vec<SurfaceAction>) {
        match *msg {
            [CC, HANDSHAKE, v] => self.answer(v, now, out),
            [CC, GOODBYE, _] if self.link.get() => {
                out.extend(self.set_online(false));
                self.identity_due = false;
                self.hello_sent = false;
                self.next_hello = Some(now + HELLO_EVERY);
            }
            _ if !self.link.get() => self.ignore(),
            [CC, cc, v] => self.control(cc, v, now, out),
            _ => match parse_sysex(msg) {
                Some(m) => self.command(m, now, out),
                None => self.ignore(),
            },
        }
    }
}

impl Surface for Kontrol {
    fn kind(&self) -> SurfaceKind {
        SurfaceKind::KontrolMk3
    }

    fn input(&mut self, bytes: &[u8], now: Instant) -> Vec<SurfaceAction> {
        let mut out = Vec::new();
        for m in midi::split(bytes) {
            match m {
                Ok(msg) => self.message(&msg, now, &mut out),
                Err(_) => self.ignored = self.ignored.saturating_add(1),
            }
        }
        out
    }

    fn tick(&mut self, now: Instant) -> Vec<SurfaceAction> {
        self.idle.tick(now)
    }

    fn feedback(&mut self, view: &MixerView, first: usize, now: Instant) -> Vec<Vec<u8>> {
        self.ids = view.strips.iter().map(|s| s.id).collect();
        self.selected = view.strips.iter().position(|s| s.select);
        self.first = first;
        self.shown = view
            .strips
            .iter()
            .skip(first)
            .take(SLOTS)
            .cloned()
            .collect();
        self.page = if self.mode == Mode::Plugin {
            view.page.clone()
        } else {
            None
        };
        if !self.link.get() {
            if std::mem::take(&mut self.hello_sent) {
                self.next_hello = Some(now + HELLO_EVERY);
                return Vec::new();
            }
            return match self.next_hello {
                Some(at) if now >= at => {
                    self.next_hello = Some(now + HELLO_EVERY);
                    vec![vec![CC, HANDSHAKE, PROTOCOL]]
                }
                _ => Vec::new(),
            };
        }
        let mut head = Vec::new();
        if std::mem::take(&mut self.identity_due) {
            let (major, minor) = IDENTITY_VERSION;
            // The version rides in value and index, the name is the info.
            head.push(sysex(CMD_IDENTITY, major, minor, &ascii(IDENTITY_NAME)));
        }
        let mut cache = std::mem::take(&mut self.cache);
        let mut tx = Sender {
            cache: &mut cache,
            out: head,
        };
        let tempo = self.send_state(view, &mut tx);
        let out = tx.out;
        self.cache = cache;
        // Remembered for the echo rule (Ruling 5).
        if tempo.is_some() {
            self.sent_tempo = tempo;
        }
        out
    }

    fn shows_page(&self) -> bool {
        self.mode == Mode::Plugin
    }

    fn refresh(&mut self) {
        self.cache.clear();
    }

    fn hello(&mut self) -> Vec<Vec<u8>> {
        self.hello_sent = true;
        vec![vec![CC, HANDSHAKE, PROTOCOL]]
    }

    fn online(&self) -> bool {
        self.link.get()
    }

    fn ignored(&self) -> u64 {
        self.ignored
    }

    fn touched(&self) -> Vec<core_model::automation::AutomationTarget> {
        Vec::new()
    }

    fn release_all(&mut self) -> Held {
        let held = self.release_holds();
        // A reconnected unit is online once it answers the hello.
        self.link = Link::default();
        held
    }

    fn release_holds(&mut self) -> Held {
        Held {
            touched: Vec::new(),
            idle: self.idle.take_all(),
        }
    }

    fn goodbye(&mut self) -> Vec<Vec<u8>> {
        // The registry has answered the holds already (I1): only the
        // offline mark here. The slots are emptied outside the cache.
        let _ = self.link.set(false);
        self.cache.clear();
        let mut out: Vec<Vec<u8>> = (0..SLOTS).map(|i| sysex(CMD_TYPE, 0, b7(i), &[])).collect();
        out.push(vec![CC, GOODBYE, 0x00]);
        out
    }

    fn take_problem(&mut self) -> Option<String> {
        self.problem.take()
    }
}

impl Unit for Kontrol {
    fn link(&mut self) -> &mut Link {
        &mut self.link
    }

    fn idle(&self) -> &IdleTouches {
        &self.idle
    }

    fn forget_all(&mut self) -> Held {
        self.release_holds()
    }
}
