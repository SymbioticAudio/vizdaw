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
pub mod kontrol;
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

/// One V-pot detent, in normalised units (Ruling 5), for MCU and HUI.
pub(crate) const DETENT: f64 = 0.01;

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
    /// A Kontrol S-Series MK3 keyboard's DAW port (plan 5f-4).
    KontrolMk3,
}

/// What kind of track a strip is, for surfaces that show it (plan 5f-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackKind {
    /// Fed by an instrument (plug-in or built-in).
    Instrument,
    /// An audio output path.
    Audio,
    /// An aux bus.
    Bus,
    /// The master.
    Master,
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

/// What a unit let go of (final review T8 m6): touched faders, released
/// now, and idle holds, released at their last value's time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Held {
    /// Faders a touch held.
    pub touched: Vec<AutomationTarget>,
    /// Targets an idle hold held (none of them in `touched`).
    pub idle: Vec<AutomationTarget>,
}

impl Held {
    /// Every target: the touched ones, then the idle ones.
    pub fn all(&self) -> Vec<AutomationTarget> {
        self.touched.iter().chain(&self.idle).copied().collect()
    }

    /// Nothing was held.
    pub fn is_empty(&self) -> bool {
        self.touched.is_empty() && self.idle.is_empty()
    }

    /// Adds `t` as touched or idle, unless it is there already.
    fn add(&mut self, t: AutomationTarget, touched: bool) {
        if self.touched.contains(&t) || self.idle.contains(&t) {
            return;
        }
        if touched {
            self.touched.push(t);
        } else {
            self.idle.push(t);
        }
    }
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
    /// a disconnect or a removal); a reconnected unit is online once it
    /// says so.
    fn release_all(&mut self) -> Held;
    /// The holds `release_all` answers, forgotten the same way, but the
    /// unit stays online: the focus moved to another project (final review
    /// T8 m2).
    fn release_holds(&mut self) -> Held;
    /// The V-pot assignment, for units that have one (MCU and MCU XT).
    fn assign(&self) -> Option<Assign> {
        None
    }
    /// Sets the V-pot assignment: the group manager passes the main unit's
    /// to its extenders, which cannot see it (Ruling 3; Ruling T1-M2).
    fn set_assign(&mut self, _assign: Assign) {}
    /// Messages that let the unit go (a removal, the app quitting); the
    /// unit is offline after them (plan 5f-4 Ruling 4).
    fn goodbye(&mut self) -> Vec<Vec<u8>> {
        Vec::new()
    }
    /// A problem the unit found since the last call, said once (plan
    /// 5f-4 Ruling 4).
    fn take_problem(&mut self) -> Option<String> {
        None
    }
}

/// A unit's online state (Ruling T3-1). `Down` is the online→offline
/// change; `Up` is a return after a `Down` (the first time a unit comes
/// online is not one: connecting already refreshed it).
#[derive(Debug, Default)]
pub(crate) struct Link {
    online: bool,
    /// It went offline since it was last online.
    dropped: bool,
}

/// What a `Link::set` changed.
pub(crate) enum Edge {
    /// Nothing changed (or a first coming online).
    Same,
    /// Back online after going offline.
    Up,
    /// Online → offline.
    Down,
}

impl Link {
    /// Online now.
    pub(crate) fn get(&self) -> bool {
        self.online
    }

    /// Sets the state, answering the change.
    pub(crate) fn set(&mut self, on: bool) -> Edge {
        let was = std::mem::replace(&mut self.online, on);
        match (was, on) {
            (true, false) => {
                self.dropped = true;
                Edge::Down
            }
            (false, true) if std::mem::take(&mut self.dropped) => Edge::Up,
            _ => Edge::Same,
        }
    }
}

/// What MCU and HUI share for Ruling T3-1: a unit that goes offline while
/// its port stays connected releases everything it holds, once, as
/// `release_all` does; back online, it gets a full refresh.
pub(crate) trait Unit: Surface {
    /// The unit's online state.
    fn link(&mut self) -> &mut Link;
    /// The unit's idle touches.
    fn idle(&self) -> &IdleTouches;
    /// `release_all` without the online reset: forgets every hold, touch
    /// flag, pending byte and the shuttle; held faders are resent.
    fn forget_all(&mut self) -> Held;

    /// Sets the online state: going offline answers the releases of what
    /// the unit held (touched faders `Release`, idle holds `IdleRelease` at
    /// their last value's time), then forgets them; coming back refreshes.
    fn set_online(&mut self, on: bool) -> Vec<SurfaceAction> {
        match self.link().set(on) {
            Edge::Same => Vec::new(),
            Edge::Up => {
                self.refresh();
                Vec::new()
            }
            Edge::Down => {
                let mut out: Vec<SurfaceAction> = self
                    .touched()
                    .into_iter()
                    .map(|t| SurfaceAction::Gesture(t, Phase::Release))
                    .collect();
                out.extend(self.idle().releases());
                let _ = self.forget_all();
                out
            }
        }
    }
}

/// A new state machine of `kind`.
pub fn new_surface(kind: SurfaceKind) -> Box<dyn Surface> {
    match kind {
        SurfaceKind::Mcu => Box::new(mcu::Mcu::new(false)),
        SurfaceKind::McuXt => Box::new(mcu::Mcu::new(true)),
        SurfaceKind::Hui => Box::new(hui::Hui::new()),
        SurfaceKind::KontrolMk3 => Box::new(kontrol::Kontrol::new()),
    }
}
