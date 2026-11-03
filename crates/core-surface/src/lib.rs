//! `core-surface`: control-surface protocol state machines and MIDI learn
//! (spec §8). Pure Rust, no I/O: bytes in, actions and bytes out; time is
//! passed in.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod hui;
mod idle;
pub mod learn;
pub mod mcu;
pub mod midi;
pub mod shuttle;

pub use idle::{IDLE_RELEASE, IdleTouches};
pub use shuttle::{SHUTTLE_HOLD, Shuttle, WIND_EVERY, wind_rate};

use core_model::automation::{AutomationMode, AutomationTarget};
use core_model::ids::StripId;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How long a sent fader value is an echo (Ruling 13).
pub const ECHO_WINDOW: Duration = Duration::from_millis(100);

/// A bank group's layout (Ruling 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bank {
    /// The first strip the group shows.
    pub offset: usize,
    /// How many units the group has.
    pub units: usize,
}

impl Bank {
    /// The strips the group shows: 8 × units.
    pub fn width(&self) -> usize {
        self.units.saturating_mul(8)
    }

    /// The first strip unit `k` (0-based, in position order) shows.
    pub fn first(&self, k: usize) -> usize {
        self.offset.saturating_add(k.saturating_mul(8))
    }

    /// Applies Bank(±n) / Channel(±n) and clamps to `0..=strips − width` (0 when fewer).
    pub fn shift(&mut self, a: &SurfaceAction, strips: usize) {
        let step = match *a {
            SurfaceAction::Bank(n) => Some((n, self.width())),
            SurfaceAction::Channel(n) => Some((n, 1)),
            _ => None,
        };
        if let Some((n, by)) = step {
            let d = usize::try_from(n.unsigned_abs())
                .unwrap_or(usize::MAX)
                .saturating_mul(by);
            self.offset = if n < 0 {
                self.offset.saturating_sub(d)
            } else {
                self.offset.saturating_add(d)
            };
        }
        self.clamp(strips);
    }

    /// Clamps the offset to `0..=strips − width` (0 when fewer).
    pub fn clamp(&mut self, strips: usize) {
        self.offset = self.offset.min(strips.saturating_sub(self.width()));
    }
}

/// The 7-segment code of one display character (Ruling 14): ASCII
/// `0x40`–`0x5F` as `− 0x40`, `0x20`–`0x3F` as itself (lower case as upper
/// case, anything else a blank), plus `0x40` for a following dot.
pub fn seg(c: char, dot: bool) -> u8 {
    let c = c.to_ascii_uppercase();
    let code = match u8::try_from(c) {
        Ok(b @ 0x40..=0x5F) => b - 0x40,
        Ok(b @ 0x20..=0x3F) => b,
        _ => 0x20,
    };
    if dot { code | 0x40 } else { code }
}

/// Unit types (spec §8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    /// An MCU-compatible main unit.
    Mcu,
    /// An MCU XT-compatible extender.
    McuXt,
    /// A HUI-compatible unit.
    Hui,
}

/// A gesture phase (spec §7.1); `Touch(None)`: at the value heard now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// The target is touched, at this value or (`None`) at the value heard now.
    Touch(Option<f64>),
    /// The touched target moves to this normalised value.
    Value(f64),
    /// The target is let go.
    Release,
}

/// What a surface asks for (spec §8.1).
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceAction {
    /// A gesture phase on a target.
    Gesture(AutomationTarget, Phase),
    /// A release after idle (Ruling 2), stamped at the target's last value.
    IdleRelease(AutomationTarget, Instant),
    /// A registry ID (Ruling 5) or a learned command.
    Command(String),
    /// Move the bank by this many banks.
    Bank(i32),
    /// Move the bank by this many strips.
    Channel(i32),
    /// Select this strip.
    Select(StripId),
    /// Toggle this strip's record arm.
    Arm(StripId),
    /// The jog wheel turned by this many detents (negative: back).
    Jog(i32),
    /// Rewind (−1) / Fast Forward (+1) tapped: the previous/next marker, else bar line (Ruling 5).
    Skip(i32),
    /// Rewind / Fast Forward held: move by this many quarter notes (negative: back).
    Wind(f64),
}

/// The V-pot assignment (Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Assign {
    /// V-pots drive the strips' pan.
    #[default]
    Pan,
    /// V-pots drive the strips' first send.
    Send,
    /// V-pots drive the selected strip's first insert's parameters.
    Plugin,
}

/// One plugin parameter a V-pot can drive.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamView {
    /// The parameter's automation target.
    pub target: AutomationTarget,
    /// The parameter's name.
    pub name: String,
    /// The heard normalised value.
    pub value: f64,
    /// The value as the plugin shows it.
    pub text: String,
}

/// One strip as the surface shows it (normalised heard values, as automation uses them; Ruling 13).
#[derive(Debug, Clone, PartialEq)]
pub struct StripView {
    /// The strip.
    pub id: StripId,
    /// The strip's name.
    pub name: String,
    /// The fader's normalised value.
    pub fader: f64,
    /// The pan's normalised value.
    pub pan: f64,
    /// The first send's normalised level, if the strip has a send.
    pub send: Option<f64>,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Armed for recording.
    pub arm: bool,
    /// Selected.
    pub select: bool,
    /// The strip's automation mode.
    pub mode: AutomationMode,
    /// Linear peaks since the last view.
    pub meter: (f32, f32),
}

/// The MixerView snapshot (spec §8.1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MixerView {
    /// The strips in display order, master excluded.
    pub strips: Vec<StripView>,
    /// The master strip.
    pub master: Option<StripView>,
    /// The selected strip's first insert's first 8 parameters (Ruling 5).
    pub plugin: Vec<ParamView>,
    /// Bars·beats as the transport shows it ("12.3.2.120").
    pub position: String,
    /// The transport plays.
    pub playing: bool,
    /// The transport records.
    pub recording: bool,
    /// The loop is on.
    pub looping: bool,
}

/// A protocol state machine (spec §8.1; Ruling 3).
pub trait Surface: Send {
    /// The unit type.
    fn kind(&self) -> SurfaceKind;
    /// Decodes bytes from the unit. Never panics, whatever the bytes.
    fn input(&mut self, bytes: &[u8], now: Instant) -> Vec<SurfaceAction>;
    /// Timed actions: idle releases, the shuttle's wind, pings.
    fn tick(&mut self, now: Instant) -> Vec<SurfaceAction>;
    /// Messages that bring the unit to `view`, showing strips from `first`.
    fn feedback(&mut self, view: &MixerView, first: usize, now: Instant) -> Vec<Vec<u8>>;
    /// Forget what was sent: the next feedback is complete.
    fn refresh(&mut self);
    /// Messages to send on connect (MCU device query; HUI first ping).
    fn hello(&mut self) -> Vec<Vec<u8>>;
    /// The unit answered the handshake or sent a valid message.
    fn online(&self) -> bool;
    /// Malformed or unknown messages so far (spec §12.2).
    fn ignored(&self) -> u64;
    /// Faders it holds touched now (touch-sensing faders only).
    fn touched(&self) -> Vec<AutomationTarget>;
    /// Every target it holds (touched faders and idle holds), then forgets
    /// them, its touch flags, pending hi bytes and the shuttle (Ruling 3:
    /// a disconnect or a removal).
    fn release_all(&mut self) -> Vec<AutomationTarget>;
    /// The V-pot assignment, for units that have one (MCU and MCU XT).
    fn assign(&self) -> Option<Assign> {
        None
    }
    /// Sets the V-pot assignment: the group manager passes the main unit's
    /// to its extenders, which cannot see it (Ruling 3; Ruling T1-M2).
    fn set_assign(&mut self, _assign: Assign) {}
}

/// A new state machine of `kind`.
pub fn new_surface(kind: SurfaceKind) -> Box<dyn Surface> {
    match kind {
        SurfaceKind::Mcu => Box::new(mcu::Mcu::new(false)),
        SurfaceKind::McuXt => Box::new(mcu::Mcu::new(true)),
        SurfaceKind::Hui => Box::new(hui::Hui::new()),
    }
}
