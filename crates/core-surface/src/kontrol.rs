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
use crate::{Held, Link, MixerView, Surface, SurfaceAction, SurfaceKind, TrackKind, Unit, midi};
use std::time::{Duration, Instant};

/// NI's SysEx header for the DAW protocol.
pub const HEADER: [u8; 10] = [0xF0, 0x00, 0x21, 0x09, 0x00, 0x00, 0x44, 0x43, 0x01, 0x00];
/// The protocol version VizDAW asks for and speaks.
pub const PROTOCOL: u8 = 4;
/// How often the hello is repeated until the keyboard answers.
pub const HELLO_EVERY: Duration = Duration::from_secs(2);
/// Display text is at most this many bytes (Ruling 8).
pub const MAX_TEXT: usize = 32;
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
    /// What the last feedback showed: read by input in Task 2.
    #[allow(dead_code)]
    view: MixerView,
    /// The first strip the last feedback showed.
    #[allow(dead_code)]
    first: usize,
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
            view: MixerView::default(),
            first: 0,
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

    fn message(&mut self, msg: &[u8], now: Instant, out: &mut Vec<SurfaceAction>) {
        match *msg {
            [CC, HANDSHAKE, v] => self.answer(v, now, out),
            [CC, GOODBYE, _] if self.link.get() => {
                out.extend(self.set_online(false));
                self.identity_due = false;
                self.hello_sent = false;
                self.next_hello = Some(now + HELLO_EVERY);
            }
            // Task 2 replaces this arm.
            _ => self.ignored = self.ignored.saturating_add(1),
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
        self.view = view.clone();
        self.first = first;
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
        let mut out = Vec::new();
        if std::mem::take(&mut self.identity_due) {
            let (major, minor) = IDENTITY_VERSION;
            // The version rides in value and index, the name is the info.
            out.push(sysex(CMD_IDENTITY, major, minor, &ascii(IDENTITY_NAME)));
        }
        out
    }

    fn refresh(&mut self) {
        // Task 3 clears its send cache here.
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
        // Task 3 adds the empty slots before the goodbye. The registry has
        // answered the holds already (I1): only the offline mark here.
        let _ = self.link.set(false);
        vec![vec![CC, GOODBYE, 0x00]]
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
