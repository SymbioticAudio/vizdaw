//! Automation recording in the session (sub-project 5 spec §7; plan 5e-1
//! Rulings 3, 7–10, 14–22): rows, the process-wide settings and suspend,
//! the 10 ms sampler, and the helpers the session's gesture path and
//! landing use.

use crate::autowrite::{Thinning, WriteMode};
use crate::error::VizError;
use crate::events::AutomationTargetRow;
use core_engine::gesture::{GestureHandles, GestureMsg, Stamped};
use core_engine::record::Mark;
use core_model::automation::{
    AutomationMode, AutomationTarget, fader_db, fader_norm, pan_norm, pan_of, strip_of, switch_on,
    unit,
};
use core_model::command::{Command, MixerCmd};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{PluginInstanceId, SeqId, StripId};
use core_model::mixer::AuxSend;
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// A strip's automation mode (sub-project 5 spec §4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AutomationModeRow {
    /// Plays the static values and ignores automation.
    Off,
    /// Plays automation.
    Read,
    /// Writes from touch to release, then returns to the automation.
    Touch,
    /// Writes from touch until Stop or the next loop wrap.
    Latch,
    /// Writes every control from play start to Stop or the next wrap.
    Write,
    /// Adds the touch's offset to the existing automation.
    Trim,
}

impl From<AutomationMode> for AutomationModeRow {
    fn from(m: AutomationMode) -> AutomationModeRow {
        match m {
            AutomationMode::Off => AutomationModeRow::Off,
            AutomationMode::Read => AutomationModeRow::Read,
            AutomationMode::Touch => AutomationModeRow::Touch,
            AutomationMode::Latch => AutomationModeRow::Latch,
            AutomationMode::Write => AutomationModeRow::Write,
            AutomationMode::Trim => AutomationModeRow::Trim,
        }
    }
}

impl From<AutomationModeRow> for AutomationMode {
    fn from(m: AutomationModeRow) -> AutomationMode {
        match m {
            AutomationModeRow::Off => AutomationMode::Off,
            AutomationModeRow::Read => AutomationMode::Read,
            AutomationModeRow::Touch => AutomationMode::Touch,
            AutomationModeRow::Latch => AutomationMode::Latch,
            AutomationModeRow::Write => AutomationMode::Write,
            AutomationModeRow::Trim => AutomationMode::Trim,
        }
    }
}

/// What a gesture does (spec §7.1).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum GesturePhaseRow {
    /// Grabbed; `value` absent: at the value heard now (Ruling 26; the
    /// mixer sends none).
    Touch {
        /// Normalised 0–1, or `None` for the value heard now.
        value: Option<f64>,
    },
    /// Moved (normalised 0–1).
    Value {
        /// Normalised 0–1.
        value: f64,
    },
    /// Let go.
    Release,
}

/// One strip's automation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct StripModeRow {
    /// Strip ID.
    pub strip: u64,
    /// Its mode.
    pub mode: AutomationModeRow,
}

/// What automation is doing in a session (`state.get automation`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AutomationStateRow {
    /// Every strip's mode, in the mixer's display order (master last).
    pub modes: Vec<StripModeRow>,
    /// The targets touched now: active touches only, never Write holds
    /// or released Latch holds (pre-flight M8; 5e-2's rings).
    pub touched: Vec<AutomationTargetRow>,
    /// Automation is suspended (process-wide).
    pub suspended: bool,
    /// This session is playing and its gesture tap is installed.
    pub writing: bool,
}

/// One point of a write in progress (Ruling 18).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct PreviewPointRow {
    /// In the playing sequence.
    pub tick: i64,
    /// Normalised 0–1.
    pub value: f64,
}

/// How closely written automation follows the gesture (Ruling 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Enum)]
pub enum ThinningRow {
    /// The finest.
    Low,
    /// The spec's tolerances.
    #[default]
    Medium,
    /// The coarsest.
    High,
}

impl From<ThinningRow> for Thinning {
    fn from(t: ThinningRow) -> Thinning {
        match t {
            ThinningRow::Low => Thinning::Low,
            ThinningRow::Medium => Thinning::Medium,
            ThinningRow::High => Thinning::High,
        }
    }
}

/// Settings › Automation (spec §7.5; Ruling 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct AutomationSettingsRow {
    /// The mode a Write strip takes after its pass (any but Write).
    pub after_write: AutomationModeRow,
    /// Touch's and Trim's return time, 0–2000 ms.
    pub return_ms: u32,
    /// The thinning.
    pub thinning: ThinningRow,
}

impl Default for AutomationSettingsRow {
    fn default() -> AutomationSettingsRow {
        AutomationSettingsRow {
            after_write: AutomationModeRow::Latch,
            return_ms: DEFAULT_RETURN_MS,
            thinning: ThinningRow::Medium,
        }
    }
}

/// The longest return time (spec §7.3: 0–2000 ms).
pub const MAX_RETURN_MS: u32 = 2000;
const DEFAULT_RETURN_MS: u32 = 250;

/// The After Write mode, as its index in `AutomationMode::ALL` (3: Latch).
static AFTER_WRITE: AtomicU8 = AtomicU8::new(3);
static RETURN_MS: AtomicU32 = AtomicU32::new(DEFAULT_RETURN_MS);
/// 0 Low, 1 Medium, 2 High.
static THINNING: AtomicU8 = AtomicU8::new(1);
static SUSPENDED: AtomicBool = AtomicBool::new(false);

fn mode_index(m: AutomationMode) -> u8 {
    AutomationMode::ALL
        .iter()
        .position(|x| *x == m)
        .and_then(|i| u8::try_from(i).ok())
        .unwrap_or(3)
}

fn settings_now() -> AutomationSettingsRow {
    let after = AutomationMode::ALL
        .get(usize::from(AFTER_WRITE.load(Ordering::Relaxed)))
        .copied()
        .unwrap_or(AutomationMode::Latch);
    AutomationSettingsRow {
        after_write: after.into(),
        return_ms: RETURN_MS.load(Ordering::Relaxed),
        thinning: match THINNING.load(Ordering::Relaxed) {
            0 => ThinningRow::Low,
            2 => ThinningRow::High,
            _ => ThinningRow::Medium,
        },
    }
}

/// The automation settings now (Ruling 19).
#[uniffi::export]
pub fn automation_settings() -> AutomationSettingsRow {
    std::panic::catch_unwind(settings_now).unwrap_or_default()
}

/// Sets the automation settings: `Model` for `return_ms` over 2000 or
/// `after_write` Write; nothing changes then (Ruling 19). A pass reads
/// them when it starts, so a change applies from the next Play.
#[uniffi::export]
pub fn set_automation_settings(settings: AutomationSettingsRow) -> Result<(), VizError> {
    crate::guard::ffi_guard(|| {
        if settings.return_ms > MAX_RETURN_MS {
            return Err(crate::events::bad("the return time is 0–2000 ms"));
        }
        if settings.after_write == AutomationModeRow::Write {
            return Err(crate::events::bad("After Write cannot be Write"));
        }
        AFTER_WRITE.store(mode_index(settings.after_write.into()), Ordering::Relaxed);
        RETURN_MS.store(settings.return_ms, Ordering::Relaxed);
        THINNING.store(
            match settings.thinning {
                ThinningRow::Low => 0,
                ThinningRow::Medium => 1,
                ThinningRow::High => 2,
            },
            Ordering::Relaxed,
        );
        Ok(())
    })
}

/// Suspends automation (spec §7.5 `automation.suspend`; Ruling 3):
/// process-wide; every strip plays as Read and no touch writes. The
/// engine holder reloads at its next idle (the audio settings' epoch).
#[uniffi::export]
pub fn set_automation_suspended(on: bool) {
    let _ = std::panic::catch_unwind(|| {
        if SUSPENDED.swap(on, Ordering::Relaxed) != on {
            crate::audio::bump_epoch();
        }
    });
}

/// Whether automation is suspended.
#[uniffi::export]
pub fn automation_suspended() -> bool {
    std::panic::catch_unwind(suspended).unwrap_or(false)
}

/// Whether automation is suspended (crate side).
pub(crate) fn suspended() -> bool {
    SUSPENDED.load(Ordering::Relaxed)
}

/// A mode as the channel spells it (`automation.set_mode`'s Choice).
pub fn mode_text(m: AutomationModeRow) -> &'static str {
    match m {
        AutomationModeRow::Off => "off",
        AutomationModeRow::Read => "read",
        AutomationModeRow::Touch => "touch",
        AutomationModeRow::Latch => "latch",
        AutomationModeRow::Write => "write",
        AutomationModeRow::Trim => "trim",
    }
}

/// The longest target text `parse_automation_target` reads.
const MAX_TARGET_TEXT: usize = 64;

/// `target` as the channel spells it (Ruling 17): `fader:<strip>`,
/// `pan:<strip>`, `mute:<strip>`, `solo:<strip>`, `send:<strip>:<n>` (n
/// from 1), `param:<slot>:<param>`; empty for velocity scale, which has
/// no spelling (it cannot be gestured).
#[uniffi::export]
pub fn automation_target_text(target: AutomationTargetRow) -> String {
    std::panic::catch_unwind(|| match target {
        AutomationTargetRow::StripFader { strip } => format!("fader:{strip}"),
        AutomationTargetRow::StripPan { strip } => format!("pan:{strip}"),
        AutomationTargetRow::StripMute { strip } => format!("mute:{strip}"),
        AutomationTargetRow::StripSolo { strip } => format!("solo:{strip}"),
        AutomationTargetRow::SendLevel { strip, send } => {
            format!("send:{strip}:{}", u32::from(send).saturating_add(1))
        }
        AutomationTargetRow::PluginParam { slot, param } => format!("param:{slot}:{param}"),
        AutomationTargetRow::VelocityScale { .. } => String::new(),
    })
    .unwrap_or_default()
}

/// A run of ASCII digits as a number: no sign, no spaces, nothing else;
/// `None` when empty or it overflows.
fn digits(t: &str) -> Option<u64> {
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

/// The target `text` spells (Ruling 17), the inverse of
/// [`automation_target_text`]: at most 64 characters, ASCII digits only
/// (no sign, no spaces), `send:` counting from 1; `None` for anything
/// else, velocity scale included.
#[uniffi::export]
pub fn parse_automation_target(text: String) -> Option<AutomationTargetRow> {
    std::panic::catch_unwind(|| {
        if text.len() > MAX_TARGET_TEXT {
            return None;
        }
        let mut parts = text.split(':');
        let kind = parts.next()?;
        let a = digits(parts.next()?)?;
        let b = parts.next();
        if parts.next().is_some() {
            return None;
        }
        match (kind, b) {
            ("fader", None) => Some(AutomationTargetRow::StripFader { strip: a }),
            ("pan", None) => Some(AutomationTargetRow::StripPan { strip: a }),
            ("mute", None) => Some(AutomationTargetRow::StripMute { strip: a }),
            ("solo", None) => Some(AutomationTargetRow::StripSolo { strip: a }),
            ("send", Some(n)) => {
                let send = u16::try_from(digits(n)?.checked_sub(1)?).ok()?;
                Some(AutomationTargetRow::SendLevel { strip: a, send })
            }
            ("param", Some(p)) => Some(AutomationTargetRow::PluginParam {
                slot: a,
                param: digits(p)?,
            }),
            _ => None,
        }
    })
    .unwrap_or(None)
}

/// Puts the settings back to their defaults (test hook).
#[cfg(feature = "test-hooks")]
pub fn reset_automation_settings() {
    let _ = set_automation_settings(AutomationSettingsRow::default());
}

/// The settings a pass reads at its start: the After Write mode, the
/// return time (ms) and the thinning (Ruling 19, pre-flight M4).
pub(crate) fn pass_settings() -> (AutomationMode, u32, Thinning) {
    let s = settings_now();
    (s.after_write.into(), s.return_ms, s.thinning.into())
}

/// Where a gesture comes from (Ruling 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The mixer, the plugin window's generic sliders, the channel.
    Ui,
    /// The plugin's own editor (its events forwarded from the host).
    Plugin,
}

/// How a writing touch ends (Ruling 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Release {
    /// Glides back over the return time (Touch, Trim on no automation).
    Glide,
    /// Holds to the pass's end (Latch, Write, stepped Touch and Trim).
    Hold,
    /// Trim on automation: offsets, gliding back to 0 (Ruling 11).
    Offsets,
}

/// The path a touch took (Ruling 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Path {
    /// Inside the shared static edit.
    Static,
    /// Heard only.
    Live,
    /// Written while playing.
    Write(WriteMode, Release),
}

/// One touched target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Touching {
    /// Its path.
    pub path: Path,
    /// Its last value.
    pub value: f64,
    /// The value it was touched at (Trim's offsets are from it).
    pub base: f64,
    /// Who touches it.
    pub source: Source,
}

/// A writing pass (Rulings 7, 9, 19, 23, 24).
#[derive(Debug)]
pub(crate) struct Writing {
    /// The playing sequence.
    pub seq: SeqId,
    /// The gesture tap's control side.
    pub handles: GestureHandles,
    /// Its marks so far.
    pub marks: Vec<Mark>,
    /// Its stamps so far, by target (M15).
    pub stamps: BTreeMap<AutomationTarget, Vec<Stamped>>,
    /// Write strips' controls held from the roll (Ruling 9).
    pub holds: Vec<(AutomationTarget, f64)>,
    /// The strips that were Write at Play.
    pub write_strips: BTreeSet<StripId>,
    /// A wrap was marked: Write strips write in the After Write mode.
    pub wrapped: bool,
    /// The return time in frames at the engine's rate.
    pub return_frames: u64,
    /// The After Write mode read at Play.
    pub after_write: AutomationMode,
    /// The thinning read at Play.
    pub thinning: Thinning,
    /// The engine's dropped-gesture count at Play (M10).
    pub dropped_at_start: u64,
    /// The engine's rate (the frames' rate).
    pub rate: f64,
    /// The targets sent a Glide or a Hold this pass (Write holds
    /// included): released at the pass's end, so none plays on past it.
    pub ended: BTreeSet<AutomationTarget>,
    /// The holds were sent again once the tap rolled (re-review m4).
    pub holds_resent: bool,
    /// The targets touched since the pass began: their holds are not
    /// sent again (re-review m4).
    pub moved: BTreeSet<AutomationTarget>,
}

/// Ruling 24 (pre-flight C1, M17): a pass ends when its tap closed, when
/// another session took the engine (`transport_stopped` is `None`), or
/// once it has rolled and the transport reads stopped; never before the
/// roll while this session holds the engine.
pub(crate) fn writing_ended(closed: bool, rolled: bool, transport_stopped: Option<bool>) -> bool {
    closed || transport_stopped.is_none() || (rolled && transport_stopped == Some(true))
}

/// Who holds the engine, as a session's pass sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holder {
    /// This session.
    This,
    /// Another session.
    Other,
    /// None: the slot is empty (a device rebuild between dropping the old
    /// host and installing the new one, or no engine at all).
    Nobody,
}

/// What `idle` does with a writing pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PassStep {
    /// It goes on.
    Go,
    /// It ends and lands.
    Land,
    /// It ends and lands, and a new pass starts now: the engine was
    /// rebuilt under it and plays on (controller ruling 3 on Task 8).
    Restart,
    /// It ends and lands; a new pass starts once this session's rebuilt
    /// engine plays (the slot is empty meanwhile).
    RestartLater,
}

/// Ruling 24 with controller ruling 3: another session taking the engine
/// lands the pass (M17); a tap that went with its engine lands it, and the
/// pass starts again when this session's rebuilt engine plays on; an
/// empty slot with the tap still open is a rebuild under way, so the pass
/// waits for the tap to close; else [`writing_ended`]. `wants_play` is
/// the host's intent to play (a rebuilt engine reads stopped until its
/// Play applies).
pub(crate) fn pass_step(
    closed: bool,
    rolled: bool,
    transport_stopped: Option<bool>,
    wants_play: bool,
    holder: Holder,
) -> PassStep {
    match holder {
        Holder::Other => PassStep::Land,
        Holder::Nobody if closed => PassStep::RestartLater,
        Holder::Nobody => PassStep::Go,
        Holder::This if closed && wants_play => PassStep::Restart,
        Holder::This if writing_ended(closed, rolled, transport_stopped) => PassStep::Land,
        Holder::This => PassStep::Go,
    }
}

/// Ends the queue refused (pre-flight I3, Ruling 10), per target.
#[derive(Debug, Default)]
pub(crate) struct UnsentEnds {
    ends: BTreeMap<AutomationTarget, GestureMsg>,
}

impl UnsentEnds {
    /// Keeps `m`, the end of `t` that could not be sent.
    pub fn put(&mut self, t: AutomationTarget, m: GestureMsg) {
        self.ends.insert(t, m);
    }

    /// A later message for `t` went through: its kept end goes.
    pub fn clear(&mut self, t: AutomationTarget) {
        self.ends.remove(&t);
    }

    /// Sends every kept end; keeps those `send` refuses.
    pub fn resend(&mut self, mut send: impl FnMut(GestureMsg) -> bool) {
        self.ends.retain(|_, m| !send(*m));
    }

    /// How many are kept.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.ends.len()
    }
}

/// The 10 ms sampler (Ruling 8).
#[derive(Debug, Default)]
pub(crate) struct Sampler {
    /// When each target last sent a value.
    sent: BTreeMap<AutomationTarget, Duration>,
    /// The value kept for each target, with its host time.
    kept: BTreeMap<AutomationTarget, (f64, u64)>,
}

impl Sampler {
    /// At most one value per 10 ms.
    pub const INTERVAL: Duration = Duration::from_millis(10);

    /// A new value `v` of `t` at `now` (host time `host_ns`): `Some(v)` to
    /// send now; else it is kept with its host time (pre-flight M6).
    pub fn value(
        &mut self,
        t: AutomationTarget,
        v: f64,
        now: Duration,
        host_ns: u64,
    ) -> Option<f64> {
        let due = self
            .sent
            .get(&t)
            .is_none_or(|last| now.saturating_sub(*last) >= Self::INTERVAL);
        if due {
            self.sent.insert(t, now);
            self.kept.remove(&t);
            Some(v)
        } else {
            self.kept.insert(t, (v, host_ns));
            None
        }
    }

    /// The kept values whose 10 ms passed by `now`, with their host times.
    pub fn due(&mut self, now: Duration) -> Vec<(AutomationTarget, f64, u64)> {
        let ready: Vec<AutomationTarget> = self
            .kept
            .keys()
            .filter(|t| {
                self.sent
                    .get(t)
                    .is_none_or(|last| now.saturating_sub(*last) >= Self::INTERVAL)
            })
            .copied()
            .collect();
        let mut out = Vec::with_capacity(ready.len());
        for t in ready {
            if let Some((v, ns)) = self.kept.remove(&t) {
                self.sent.insert(t, now);
                out.push((t, v, ns));
            }
        }
        out
    }

    /// The release of `t`: its kept value, if any; it is forgotten.
    pub fn flush(&mut self, t: AutomationTarget) -> Option<(f64, u64)> {
        self.sent.remove(&t);
        self.kept.remove(&t)
    }
}

/// One parameter event of a plugin's own editor, its value normalised
/// (Ruling 15).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PluginMove {
    /// The user grabbed the control.
    Begin,
    /// A value.
    Value(f64),
    /// The user let go.
    End,
}

/// What a plugin event does on the gesture path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Move {
    /// A touch, at this value (`None`: the plugin's value now, Ruling 26).
    Touch(Option<f64>),
    /// A move.
    Value(f64),
    /// A release.
    Release,
}

/// One plugin parameter a plugin touches.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PluginTouch {
    /// Touched by a `Begin` (released by its `End` only), not by a value.
    gesture: bool,
    /// When its last value (or its `Begin`) came.
    last: Duration,
}

/// The plugin windows' touches (spec §7.1, Ruling 15, Ruling 27): a
/// `Begin` touches until its `End`; a value with no touch touches at that
/// value and is released [`PluginTouches::IDLE`] after the last value,
/// except on an automated parameter, where it is an echo of playback.
#[derive(Debug, Default)]
pub(crate) struct PluginTouches {
    touches: BTreeMap<AutomationTarget, PluginTouch>,
}

impl PluginTouches {
    /// A value-only touch is released this long after its last value
    /// (spec §7.1).
    pub(crate) const IDLE: Duration = Duration::from_millis(500);

    /// Plugin event `m` of `t` at `now`; `automated`: the playing schedule
    /// automates `t`. Answers what the gesture path does.
    pub fn event(
        &mut self,
        t: AutomationTarget,
        m: PluginMove,
        now: Duration,
        automated: bool,
    ) -> Vec<Move> {
        match (m, self.touches.get_mut(&t)) {
            (PluginMove::Begin, Some(p)) => {
                // A repeated Begin, or one after the values that touched
                // it: from now on its End releases it.
                p.gesture = true;
                p.last = now;
                Vec::new()
            }
            (PluginMove::Begin, None) => {
                self.touches.insert(
                    t,
                    PluginTouch {
                        gesture: true,
                        last: now,
                    },
                );
                vec![Move::Touch(None)]
            }
            (PluginMove::Value(v), Some(p)) => {
                p.last = now;
                vec![Move::Value(v)]
            }
            // Ruling 27: an echo of the automation it plays.
            (PluginMove::Value(_), None) if automated => Vec::new(),
            (PluginMove::Value(v), None) => {
                self.touches.insert(
                    t,
                    PluginTouch {
                        gesture: false,
                        last: now,
                    },
                );
                vec![Move::Touch(Some(v))]
            }
            (PluginMove::End, Some(_)) => {
                self.touches.remove(&t);
                vec![Move::Release]
            }
            (PluginMove::End, None) => Vec::new(),
        }
    }

    /// The value-only touches whose last value is [`PluginTouches::IDLE`]
    /// old by `now`, forgotten, each with its release time (its last value
    /// + `IDLE`).
    pub fn idle(&mut self, now: Duration) -> Vec<(AutomationTarget, Duration)> {
        let mut out = Vec::new();
        self.touches.retain(|t, p| {
            let at = p.last.saturating_add(Self::IDLE);
            let due = !p.gesture && now >= at;
            if due {
                out.push((*t, at));
            }
            !due
        });
        out
    }

    /// Forgets and answers the touches of the slots `live` denies
    /// (pre-flight I7).
    pub fn release_slots(
        &mut self,
        live: impl Fn(PluginInstanceId) -> bool,
    ) -> Vec<AutomationTarget> {
        let mut out = Vec::new();
        self.touches.retain(|t, _| {
            let keep = match t {
                AutomationTarget::PluginParam(slot, _) => live(*slot),
                _ => true,
            };
            if !keep {
                out.push(*t);
            }
            keep
        });
        out
    }

    /// The slots it touches parameters of.
    pub fn slots(&self) -> BTreeSet<PluginInstanceId> {
        self.touches
            .keys()
            .filter_map(|t| match t {
                AutomationTarget::PluginParam(slot, _) => Some(*slot),
                _ => None,
            })
            .collect()
    }

    /// Forgets `t` (its touch was released another way).
    pub fn forget(&mut self, t: AutomationTarget) {
        self.touches.remove(&t);
    }
}

/// The sampler's clock.
pub(crate) fn now() -> Duration {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed()
}

/// What the host says of a plugin parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamInfo {
    /// Lowest plain value.
    pub min: f64,
    /// Highest plain value.
    pub max: f64,
    /// Default plain value.
    pub default: f64,
    /// Whole values only.
    pub stepped: bool,
    /// Its name.
    pub name: String,
    /// Its current plain value.
    pub value: Option<f64>,
}

impl ParamInfo {
    /// A plain value, normalised by the range.
    pub fn norm(&self, plain: f64) -> f64 {
        norm(plain, self.min, self.max)
    }
}

/// Plain value `plain` of a parameter ranging `min`–`max`, normalised
/// (clamped to 0–1; 0.5 for an empty range).
pub(crate) fn norm(plain: f64, min: f64, max: f64) -> f64 {
    if max > min {
        unit((plain - min) / (max - min))
    } else {
        0.5
    }
}

/// Every plugin parameter's automation as `doc` plays it (each
/// sequence flattened: lane mute and solo, event mute, the active take and
/// references honoured), as `(sequence, tick, value bits)`: compared
/// before and after a change to find the parameters whose played
/// automation it added, removed or changed (Task 9 review I-1, re-review
/// N1). A document with no plugin-parameter automation event at all is
/// not flattened (re-review N2).
pub(crate) fn plugin_automation(
    doc: &Document,
) -> BTreeMap<AutomationTarget, Vec<(SeqId, Tick, u64)>> {
    let mut out: BTreeMap<AutomationTarget, Vec<(SeqId, Tick, u64)>> = BTreeMap::new();
    let any = doc.sequences().values().any(|seq| {
        seq.lanes.iter().any(|lane| {
            lane.takes.iter().any(|take| {
                take.events.values().any(|e| {
                    matches!(
                        e.body,
                        EventBody::Automation {
                            target: AutomationTarget::PluginParam(..),
                            ..
                        }
                    )
                })
            })
        })
    });
    if !any {
        return out;
    }
    for id in doc.sequences().keys() {
        for (target, points) in existing_all(doc, *id) {
            if matches!(target, AutomationTarget::PluginParam(..)) {
                out.entry(target)
                    .or_default()
                    .extend(points.into_iter().map(|(t, v)| (*id, t, v.to_bits())));
            }
        }
    }
    out
}

/// The host's word on `t` (a plugin parameter) in session `owner`'s
/// running instance; `None` for other targets, a slot not running, a
/// parameter it does not list, or a busy registry.
pub(crate) fn param_info(owner: u64, t: AutomationTarget) -> Option<ParamInfo> {
    let AutomationTarget::PluginParam(slot, param) = t else {
        return None;
    };
    crate::plugins::with(owner, |p| {
        let inst = p?.instance(slot)?;
        let i = inst.info().params.iter().find(|i| i.id == param)?.clone();
        Some(ParamInfo {
            min: i.min,
            max: i.max,
            default: i.default,
            stepped: i.stepped,
            name: i.name,
            value: inst.param_value(param),
        })
    })
    .ok()
    .flatten()
}

/// The lane a new write goes into (Ruling 4): "<strip> Fader", "<strip>
/// Pan", "<strip> Mute", "<strip> Solo", "<strip> Send <n>" (n from 1),
/// "<strip> <parameter name>", "<strip> Parameter <id>".
pub(crate) fn lane_name(doc: &Document, t: AutomationTarget, param_name: Option<&str>) -> String {
    let strip = strip_of(doc, t)
        .and_then(|s| doc.mixer().strip(s).ok())
        .map(|s| s.name.clone());
    let what = match t {
        AutomationTarget::StripFader(_) => "Fader".to_string(),
        AutomationTarget::StripPan(_) => "Pan".to_string(),
        AutomationTarget::StripMute(_) => "Mute".to_string(),
        AutomationTarget::StripSolo(_) => "Solo".to_string(),
        AutomationTarget::SendLevel(_, i) => format!("Send {}", u32::from(i.0) + 1),
        AutomationTarget::PluginParam(_, id) => match param_name {
            Some(n) if !n.trim().is_empty() => n.to_string(),
            _ => format!("Parameter {}", id.0),
        },
        AutomationTarget::VelocityScale(_) => return t.describe(),
    };
    match strip {
        Some(s) => format!("{s} {what}"),
        None => what,
    }
}

/// Every target's points in `seq` flattened as the engine plays it
/// (Ruling 22), by tick.
pub(crate) fn existing_all(
    doc: &Document,
    seq: SeqId,
) -> BTreeMap<AutomationTarget, Vec<(Tick, f64)>> {
    let mut out: BTreeMap<AutomationTarget, Vec<(Tick, f64)>> = BTreeMap::new();
    if let Ok(events) = core_model::query::flatten(doc, seq, None, &ViewFilter::default()) {
        for e in events {
            if let EventBody::Automation { target, value } = e.body {
                out.entry(target).or_default().push((e.tick, value));
            }
        }
    }
    out
}

/// The target's points in `seq` flattened (Ruling 22), by tick.
pub(crate) fn existing(doc: &Document, seq: SeqId, t: AutomationTarget) -> Vec<(Tick, f64)> {
    existing_all(doc, seq).remove(&t).unwrap_or_default()
}

/// The value of `points` (by tick) at `tick` by Plan 4d's lane rule: the
/// first value before the first point, the last after the last, linear
/// between unless `stepped`; `None` with no points.
pub(crate) fn value_at(points: &[(Tick, f64)], tick: Tick, stepped: bool) -> Option<f64> {
    let n = points.partition_point(|p| p.0 <= tick);
    match (n.checked_sub(1).and_then(|i| points.get(i)), points.get(n)) {
        (Some(a), Some(b)) if !stepped && b.0 > a.0 => {
            let f = (tick.0 - a.0.0) as f64 / (b.0.0 - a.0.0) as f64;
            Some(a.1 + (b.1 - a.1) * f)
        }
        (Some(a), _) => Some(a.1),
        (None, Some(b)) => Some(b.1),
        (None, None) => None,
    }
}

/// The static value of `t`, normalised: a strip's or send's from the
/// document; a plugin parameter's from the slot's overlay, else its
/// default, normalised by the range `param` gives; else 0.5.
pub(crate) fn fixed_value(doc: &Document, t: AutomationTarget, param: Option<&ParamInfo>) -> f64 {
    let m = doc.mixer();
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    match t {
        AutomationTarget::StripFader(s) => m.strip(s).map_or(0.5, |x| fader_norm(x.gain_db)),
        AutomationTarget::StripPan(s) => m.strip(s).map_or(0.5, |x| pan_norm(x.pan)),
        AutomationTarget::StripMute(s) => m.strip(s).map_or(0.0, |x| on(x.mute)),
        AutomationTarget::StripSolo(s) => m.strip(s).map_or(0.0, |x| on(x.solo)),
        AutomationTarget::SendLevel(s, i) => m
            .strip(s)
            .ok()
            .and_then(|x| x.sends.get(usize::from(i.0)))
            .map_or(0.5, |x| fader_norm(x.level_db)),
        AutomationTarget::PluginParam(slot, id) => match param {
            Some(p) => {
                let plain = doc
                    .plugin_slot(slot)
                    .and_then(|x| x.params.get(&id).copied())
                    .unwrap_or(p.default);
                p.norm(plain)
            }
            None => 0.5,
        },
        AutomationTarget::VelocityScale(_) => 0.5,
    }
}

/// The command that sets `t`'s static value to `v` (normalised):
/// SetGain, SetPan, SetMute, SetSolo, EditSend; `None` for plugin
/// parameters (they commit `SetPluginParams` at the release) and the
/// rest.
pub(crate) fn static_command(doc: &Document, t: AutomationTarget, v: f64) -> Option<Command> {
    Some(Command::Mixer(match t {
        AutomationTarget::StripFader(strip) => MixerCmd::SetGain {
            strip,
            gain_db: fader_db(v),
        },
        AutomationTarget::StripPan(strip) => MixerCmd::SetPan {
            strip,
            pan: pan_of(v),
        },
        AutomationTarget::StripMute(strip) => MixerCmd::SetMute {
            strip,
            mute: switch_on(v),
        },
        AutomationTarget::StripSolo(strip) => MixerCmd::SetSolo {
            strip,
            solo: switch_on(v),
        },
        AutomationTarget::SendLevel(strip, i) => {
            let old = doc.mixer().strip(strip).ok()?.sends.get(usize::from(i.0))?;
            MixerCmd::EditSend {
                strip,
                index: usize::from(i.0),
                send: AuxSend {
                    level_db: fader_db(v),
                    ..*old
                },
            }
        }
        AutomationTarget::PluginParam(..) | AutomationTarget::VelocityScale(_) => return None,
    }))
}

/// What the writer reads of the document for one pass (plan 5e-1 Task 7's
/// closures): the playing sequence's tempo at the engine's rate, its
/// existing automation (Ruling 22) and the host's word on the plugin
/// parameters written.
pub(crate) struct PassDoc {
    /// The playing sequence.
    pub seq: SeqId,
    /// Its tempo, in the engine's frames.
    pub tempo: core_engine::schedule::TempoTable,
    /// Every target's existing points, by tick.
    pub existing: BTreeMap<AutomationTarget, Vec<(Tick, f64)>>,
    /// The plugin parameters among the targets.
    pub params: BTreeMap<AutomationTarget, ParamInfo>,
}

impl PassDoc {
    /// Reads `doc` for a pass of `seq` at `rate`, asking session `owner`'s
    /// plugin host about the plugin parameters among `targets`.
    pub fn read(
        owner: u64,
        doc: &Document,
        seq: SeqId,
        rate: f64,
        targets: impl Iterator<Item = AutomationTarget>,
    ) -> PassDoc {
        let tempo = match doc.sequence(seq) {
            Ok(s) => {
                core_engine::schedule::TempoTable::build(&s.tempo, Tick(-s.start_offset.0), rate)
            }
            Err(_) => core_engine::schedule::TempoTable::build(
                &core_model::tempo::TempoMap::default(),
                Tick::ZERO,
                rate,
            ),
        };
        let mut params = BTreeMap::new();
        for t in targets {
            if let Some(p) = param_info(owner, t) {
                params.insert(t, p);
            }
        }
        PassDoc {
            seq,
            tempo,
            existing: existing_all(doc, seq),
            params,
        }
    }

    /// Whether `t` is stepped: mute, solo, or a plugin-declared stepped
    /// parameter (the predicate `AutoCapture.stepped` and `to_model` share,
    /// Task 7).
    pub fn stepped(&self, t: AutomationTarget) -> bool {
        t.is_stepped() || self.params.get(&t).is_some_and(|p| p.stepped)
    }

    /// The existing automation of `t` at `tick` (Ruling 22).
    pub fn at_tick(&self, t: AutomationTarget, tick: Tick) -> Option<f64> {
        value_at(self.existing.get(&t)?, tick, self.stepped(t))
    }

    /// The writes `stamps` and `holds` make in the pass `marks` describe,
    /// up to `end_run` (Task 7's writer with the session's closures).
    #[allow(clippy::too_many_arguments)]
    pub fn writes(
        &self,
        doc: &Document,
        marks: &[Mark],
        end_run: u64,
        stamps: &[Stamped],
        holds: &[(AutomationTarget, f64)],
        thinning: Thinning,
        after_write: AutomationMode,
    ) -> Vec<crate::autowrite::TargetWrite> {
        let stepped = |t: AutomationTarget| self.stepped(t);
        let existing = |t: AutomationTarget, f: u64| self.at_tick(t, self.tempo.frame_to_tick(f));
        let existing_points = |t: AutomationTarget, from: u64, to: u64| -> Vec<u64> {
            self.existing.get(&t).map_or_else(Vec::new, |p| {
                p.iter()
                    .map(|(k, _)| self.tempo.tick_to_frame(*k))
                    .filter(|f| from <= *f && *f <= to)
                    .collect()
            })
        };
        let fixed = |t: AutomationTarget| fixed_value(doc, t, self.params.get(&t));
        crate::autowrite::writes(&crate::autowrite::AutoCapture {
            marks,
            end_run,
            stamps,
            holds,
            thinning,
            after_write: WriteMode::of(after_write),
            stepped: &stepped,
            existing: &existing,
            existing_points: &existing_points,
            fixed: &fixed,
        })
    }

    /// The model's writes of `writes` (Ruling 13), in the playing
    /// sequence, lanes named by [`lane_name`].
    pub fn to_model(
        &self,
        doc: &Document,
        writes: &[crate::autowrite::TargetWrite],
    ) -> Vec<core_model::command::AutomationWrite> {
        let stepped = |t: AutomationTarget| self.stepped(t);
        let outside = |t: AutomationTarget, tick: Tick| {
            self.at_tick(t, tick)
                .unwrap_or_else(|| fixed_value(doc, t, self.params.get(&t)))
        };
        let name =
            |t: AutomationTarget| lane_name(doc, t, self.params.get(&t).map(|p| p.name.as_str()));
        crate::autowrite::to_model(writes, self.seq, &self.tempo, &outside, &name, &stepped)
    }
}

/// The undo label of a static gesture on `t` (Ruling 14).
pub(crate) fn static_label(t: AutomationTarget) -> &'static str {
    match t {
        AutomationTarget::StripFader(_) => "Set Fader",
        AutomationTarget::StripPan(_) => "Set Pan",
        AutomationTarget::StripMute(_) => "Mute",
        AutomationTarget::StripSolo(_) => "Solo",
        AutomationTarget::SendLevel(..) => "Set Send Level",
        AutomationTarget::PluginParam(..) | AutomationTarget::VelocityScale(_) => {
            "Change Plugin Parameter"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_model::automation::AutomationTarget;
    use core_model::ids::StripId;
    use core_model::mixer::ParamId;
    use std::time::Duration;

    /// Ruling 8: at most one value per 10 ms; the last kept value is sent
    /// once 10 ms passed, or first at the release, with the host time it
    /// arrived at (pre-flight M6).
    #[test]
    fn values_are_sent_at_most_every_10_ms_and_the_last_is_kept() {
        let t = AutomationTarget::StripFader(StripId(1));
        let ms = Duration::from_millis;
        let mut s = Sampler::default();
        assert_eq!(s.value(t, 0.1, ms(0), 100), Some(0.1));
        assert_eq!(s.value(t, 0.2, ms(4), 104), None);
        assert_eq!(s.value(t, 0.3, ms(8), 108), None);
        assert!(s.due(ms(9)).is_empty());
        assert_eq!(s.due(ms(10)), vec![(t, 0.3, 108)]);
        assert!(s.due(ms(30)).is_empty(), "sent once");
        assert_eq!(s.value(t, 0.4, ms(31), 131), Some(0.4));
        assert_eq!(s.value(t, 0.5, ms(33), 133), None);
        assert_eq!(s.flush(t), Some((0.5, 133)));
        assert_eq!(s.flush(t), None);
    }

    /// Ruling 24 (pre-flight C1, M17): a pass ends when its tap closed, or
    /// once it has rolled and the transport no longer reads playing for
    /// this session; never before the roll.
    #[test]
    fn a_pass_ends_only_after_its_roll() {
        for (closed, rolled, stopped, ended) in [
            (false, false, Some(true), false), // right after Play: still reads stopped
            (false, false, None, true),        // another session took the engine before the roll
            (false, false, Some(false), false),
            (false, true, Some(false), false), // playing
            (false, true, Some(true), true),   // stopped by itself
            (false, true, None, true),         // another session took the engine
            (true, false, Some(true), true),   // the tap went with the engine
            (true, true, Some(false), true),
        ] {
            assert_eq!(
                writing_ended(closed, rolled, stopped),
                ended,
                "{closed} {rolled} {stopped:?}"
            );
        }
    }

    /// Controller ruling 3 on Task 8: a pass under a device rebuild
    /// waits for its tap to close, then starts again when the rebuilt
    /// engine plays; another session's engine never restarts it.
    #[test]
    fn a_rebuild_restarts_the_pass_and_another_session_does_not() {
        use Holder::{Nobody, Other, This};
        for (closed, rolled, stopped, wants, holder, step) in [
            (false, true, None, false, Nobody, PassStep::Go),
            (true, true, None, false, Nobody, PassStep::RestartLater),
            // The rebuilt engine's Play is still on its way.
            (true, true, Some(true), true, This, PassStep::Restart),
            (true, true, Some(false), true, This, PassStep::Restart),
            (true, true, Some(true), false, This, PassStep::Land),
            (false, true, Some(false), true, This, PassStep::Go),
            (false, false, Some(true), true, This, PassStep::Go),
            (false, true, Some(true), false, This, PassStep::Land),
            (false, true, None, true, Other, PassStep::Land),
            (true, true, None, true, Other, PassStep::Land),
        ] {
            assert_eq!(
                pass_step(closed, rolled, stopped, wants, holder),
                step,
                "{closed} {rolled} {stopped:?} {wants} {holder:?}"
            );
        }
    }

    /// Spec §7.1 (Ruling 15): a plugin that sends values without gesture
    /// events is touched by its first value and released 500 ms after its
    /// last; one with Begin/End is released by its End only; on an
    /// automated parameter a value with no Begin is an echo, not a touch
    /// (Ruling 27); a slot that went releases its touches.
    #[test]
    fn values_without_gesture_events_release_500_ms_after_the_last() {
        let t = AutomationTarget::PluginParam(PluginInstanceId(3), ParamId(0));
        let ms = Duration::from_millis;
        let mut p = PluginTouches::default();
        assert_eq!(
            p.event(t, PluginMove::Value(0.2), ms(0), false),
            vec![Move::Touch(Some(0.2))]
        );
        assert_eq!(
            p.event(t, PluginMove::Value(0.3), ms(100), false),
            vec![Move::Value(0.3)]
        );
        assert!(p.idle(ms(599)).is_empty());
        assert_eq!(p.idle(ms(600)), vec![(t, ms(600))]);
        assert!(p.idle(ms(2000)).is_empty(), "released once");
        assert_eq!(
            p.event(t, PluginMove::Begin, ms(3000), false),
            vec![Move::Touch(None)]
        );
        assert!(
            p.event(t, PluginMove::Begin, ms(3001), false).is_empty(),
            "a repeated Begin"
        );
        assert_eq!(
            p.event(t, PluginMove::Value(0.5), ms(3010), false),
            vec![Move::Value(0.5)]
        );
        assert!(
            p.idle(ms(9000)).is_empty(),
            "gesture-driven: no idle release"
        );
        assert_eq!(
            p.event(t, PluginMove::End, ms(9001), false),
            vec![Move::Release]
        );
        assert!(
            p.event(t, PluginMove::End, ms(9002), false).is_empty(),
            "an End with no touch"
        );
        // A Begin on a value-only touch (review m7): from then on only its
        // End releases it.
        assert_eq!(
            p.event(t, PluginMove::Value(0.6), ms(9100), false),
            vec![Move::Touch(Some(0.6))]
        );
        assert!(p.event(t, PluginMove::Begin, ms(9110), false).is_empty());
        assert!(p.idle(ms(9700)).is_empty(), "gesture-driven now");
        assert_eq!(
            p.event(t, PluginMove::End, ms(9800), false),
            vec![Move::Release]
        );
        // Pre-flight I8: an automated parameter's value-only events are
        // echoes of playback; a Begin still touches it.
        let t2 = AutomationTarget::PluginParam(PluginInstanceId(3), ParamId(1));
        assert!(
            p.event(t2, PluginMove::Value(0.2), ms(10_000), true)
                .is_empty(),
            "an echo"
        );
        assert!(p.idle(ms(20_000)).is_empty(), "no touch was made");
        assert_eq!(
            p.event(t2, PluginMove::Begin, ms(20_001), true),
            vec![Move::Touch(None)]
        );
        assert_eq!(
            p.event(t2, PluginMove::Value(0.4), ms(20_002), true),
            vec![Move::Value(0.4)]
        );
        assert_eq!(
            p.event(t2, PluginMove::End, ms(20_003), true),
            vec![Move::Release]
        );
        // Pre-flight I7: the touches of a slot that went are released once.
        assert_eq!(
            p.event(t2, PluginMove::Value(0.5), ms(21_000), false),
            vec![Move::Touch(Some(0.5))]
        );
        assert_eq!(p.release_slots(|_| false), vec![t2]);
        assert!(p.idle(ms(30_000)).is_empty(), "released once");
    }

    /// Pre-flight I3, Ruling 10: an end the queue refused is re-sent until
    /// it goes; a later message sent for its target replaces it.
    #[test]
    fn an_unsent_end_is_resent_until_it_goes_and_a_later_message_replaces_it() {
        use core_engine::gesture::{GestureMsg, GesturePhase};
        use core_engine::schedule::AutoDest;
        let (t, u) = (
            AutomationTarget::StripFader(StripId(1)),
            AutomationTarget::StripPan(StripId(1)),
        );
        let msg = |x: AutomationTarget, phase| GestureMsg {
            dest: AutoDest::of(x).unwrap(),
            phase,
            host_ns: 0,
            tag: 2,
        };
        let mut q = UnsentEnds::default();
        q.put(t, msg(t, GesturePhase::Hold));
        q.put(u, msg(u, GesturePhase::Release));
        let mut tried = Vec::new();
        q.resend(|m| {
            tried.push(m.phase);
            false
        });
        assert_eq!(
            (tried.len(), q.len()),
            (2, 2),
            "both kept while the queue is full"
        );
        q.clear(u); // a later message for the pan went through
        tried.clear();
        q.resend(|m| {
            tried.push(m.phase);
            true
        });
        assert_eq!(tried, vec![GesturePhase::Hold]);
        assert_eq!(q.len(), 0);
        q.resend(|_| panic!("nothing is left"));
    }
}
