//! Control surfaces and MIDI learn in the core (plan 5f-1 Tasks 7–8; spec
//! §8.1–§8.3): the process-wide registry of units, their ports,
//! `surfaces.json`, adding and removing units, their connection, the
//! service's registry half, the claims and MIDI learn.
//!
//! - The registry ([`Registry`], behind [`lock`]) holds the units, the bank
//!   groups (Ruling 4), the focused session (Ruling 11), the releases each
//!   session is owed, the port transport (Ruling 10), the 2 s check, the
//!   focused session's [`Mapper`], the learned commands and the waiting
//!   learn.
//! - The input queue ([`input`]) has its own lock and never waits: CoreMIDI's
//!   thread pushes into it.
//! - The claims ([`takes`], Ruling 17) are a `RwLock` the registry rewrites
//!   on every surface, map, learn or focus change; the lanes and note input
//!   only `try_read` them.
//! - Lock order (Global Constraints): `Session.inner` → the registry → the
//!   input queue; the registry also rewrites the claims under its own lock.
//!   The session's half of the service (`Session::service_surfaces`) copies
//!   actions out of the registry, drops it, then applies them under `inner`.

pub(crate) mod ports;

use crate::automation::{SurfaceId, TouchHolder};
use crate::error::VizError;
use crate::events::{AutomationTargetRow, automation_target_row};
use crate::guard::{ffi_guard, lock as lock_mutex};
use core_model::automation::AutomationTarget;
use core_model::ids::StripId;
use core_model::learn::{LearnKind, LearnMap, LearnMode, LearnSource};
use core_surface::learn::{CommandMap, Learner, Mapper};
use core_surface::{Bank, MixerView, Surface, SurfaceAction, SurfaceKind, new_surface};
use ports::Ports;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, PoisonError, RwLock, TryLockError};
use std::time::{Duration, Instant};

/// The file's name in the support folder (Ruling 9).
const FILE: &str = "surfaces.json";

/// The file format this build reads and writes (Ruling 9).
const VERSION: u32 = 1;

/// The most surfaces (Ruling 4).
const MAX_SURFACES: usize = SurfaceId::COUNT as usize;

/// The most MCU XT units in the group (Ruling 4).
const MAX_EXTENDERS: usize = 3;

/// Positions in the MCU group: 0–3 (Ruling 4).
const POSITIONS: u8 = 4;

/// The longest port name (as `crate::live::parse_injection`).
const MAX_PORT: usize = 255;

/// How often the ports are looked at again while any surface exists
/// (Ruling 10).
pub const PORT_CHECK: Duration = Duration::from_secs(2);

/// The input queue's bound (Ruling 10).
pub const INPUT_LIMIT: usize = 4096;

/// Feedback goes to a unit at most this often, unless it shows a target
/// another hand moved (Ruling 13).
pub const FEEDBACK_EVERY: Duration = Duration::from_millis(20);

/// The non-meter messages kept per unit for `state.get surfaces`.
const LOG_LIMIT: usize = 64;

/// The problem lines kept (Task 7 review M5).
const PROBLEM_LIMIT: usize = 16;

/// "VizDAW drives one MCU-compatible main unit and up to three extenders".
const GROUP_LIMIT: &str = "VizDAW drives one MCU-compatible main unit and up to three extenders";

/// Learning a command needs an open project (Ruling 11).
const NO_PROJECT: &str = "Open a project to learn MIDI";

/// A unit's type, over the FFI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SurfaceKindRow {
    /// An MCU-compatible main unit.
    Mcu,
    /// An MCU XT-compatible extender.
    McuXt,
    /// A HUI-compatible unit.
    Hui,
}

impl From<SurfaceKindRow> for SurfaceKind {
    fn from(k: SurfaceKindRow) -> SurfaceKind {
        match k {
            SurfaceKindRow::Mcu => SurfaceKind::Mcu,
            SurfaceKindRow::McuXt => SurfaceKind::McuXt,
            SurfaceKindRow::Hui => SurfaceKind::Hui,
        }
    }
}

impl From<SurfaceKind> for SurfaceKindRow {
    fn from(k: SurfaceKind) -> SurfaceKindRow {
        match k {
            SurfaceKind::Mcu => SurfaceKindRow::Mcu,
            SurfaceKind::McuXt => SurfaceKindRow::McuXt,
            SurfaceKind::Hui => SurfaceKindRow::Hui,
        }
    }
}

/// One configured surface (Settings › Surfaces, `state.get surfaces`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SurfaceRow {
    /// 0–15 (Ruling 4).
    pub id: u32,
    /// The unit's type.
    pub kind: SurfaceKindRow,
    /// The MIDI source it is heard on.
    pub input: String,
    /// The MIDI destination it is sent to.
    pub output: String,
    /// Its place in the MCU bank group (0–3; 0 for a HUI-compatible unit).
    pub position: u8,
    /// Both ports present.
    pub connected: bool,
    /// Answered (MCU handshake or any valid message; HUI ping).
    pub online: bool,
    /// Malformed or unknown messages so far, and actions that did nothing
    /// (spec §12.2).
    pub ignored: u64,
}

/// The MIDI ports the surfaces' transport sees.
#[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
pub struct SurfacePortsRow {
    /// Source names.
    pub sources: Vec<String>,
    /// Destination names.
    pub destinations: Vec<String>,
}

/// What a surface or a learned command asks the app for (Ruling 12).
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SurfaceEventRow {
    /// A registry command. `programmatic`: caused by `surface.inject` or
    /// `midi.inject` (Ruling 12).
    Command {
        /// The registry ID.
        id: String,
        /// Caused by the control channel: never opens a dialog.
        programmatic: bool,
    },
    /// Select this strip.
    Select {
        /// Strip ID.
        strip: u64,
    },
    /// Toggle the arm of this strip's input lane.
    Arm {
        /// Strip ID.
        strip: u64,
        /// The strip's output path when it is an AudioPath, else None.
        path: Option<u64>,
    },
    /// The jog wheel turned.
    Jog {
        /// Detents (negative: back).
        detents: i32,
    },
}

/// What was last sent to one unit.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SurfaceFeedbackRow {
    /// The surface's ID.
    pub surface: u32,
    /// Hex, the last 64, meters left out.
    pub messages: Vec<String>,
    /// How many meter messages were sent.
    pub meters_sent: u64,
}

/// `state.get surfaces`.
#[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
pub struct SurfaceStateRow {
    /// The configured surfaces.
    pub surfaces: Vec<SurfaceRow>,
    /// Per surface, what feedback sent (empty without a document).
    pub feedback: Vec<SurfaceFeedbackRow>,
    /// What a waiting learn is for.
    pub learning: Option<String>,
    /// Problem lines: the file, saving, dropped input (Ruling 10).
    pub problems: Vec<String>,
}

/// What a learned control sends (spec §4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LearnKindRow {
    /// A 7-bit controller (`number`: the CC).
    Cc7,
    /// A 14-bit pair (`number`: the MSB's CC, 0–31).
    Cc14,
    /// Pitch bend (`number` unused, 0).
    PitchBend,
    /// A note (`number`: the note).
    Note,
}

/// How a learned CC's values move the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LearnModeRow {
    /// The value is the position.
    Absolute,
    /// Relative detents, 2's complement.
    Relative2sComplement,
    /// Relative detents around 64.
    RelativeBinOffset,
}

impl From<LearnMode> for LearnModeRow {
    fn from(m: LearnMode) -> LearnModeRow {
        match m {
            LearnMode::Absolute => LearnModeRow::Absolute,
            LearnMode::Relative2sComplement => LearnModeRow::Relative2sComplement,
            LearnMode::RelativeBinOffset => LearnModeRow::RelativeBinOffset,
        }
    }
}

impl From<LearnModeRow> for LearnMode {
    fn from(m: LearnModeRow) -> LearnMode {
        match m {
            LearnModeRow::Absolute => LearnMode::Absolute,
            LearnModeRow::Relative2sComplement => LearnMode::Relative2sComplement,
            LearnModeRow::RelativeBinOffset => LearnMode::RelativeBinOffset,
        }
    }
}

/// A port and what arrives on it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LearnSourceRow {
    /// The MIDI source port's name.
    pub port: String,
    /// The message.
    pub kind: LearnKindRow,
    /// MIDI channel 0–15.
    pub channel: u8,
    /// The CC or note (0 for pitch bend).
    pub number: u8,
}

impl LearnSourceRow {
    /// The model's source, checked (Ruling 6).
    pub(crate) fn source(&self) -> Result<LearnSource, VizError> {
        let (channel, number) = (self.channel, self.number);
        let kind = match self.kind {
            LearnKindRow::Cc7 => LearnKind::Cc7 {
                channel,
                cc: number,
            },
            LearnKindRow::Cc14 => LearnKind::Cc14 {
                channel,
                cc: number,
            },
            LearnKindRow::PitchBend => LearnKind::PitchBend { channel },
            LearnKindRow::Note => LearnKind::Note {
                channel,
                note: number,
            },
        };
        let s = LearnSource {
            port: self.port.clone(),
            kind,
        };
        s.check()?;
        Ok(s)
    }
}

impl From<&LearnSource> for LearnSourceRow {
    fn from(s: &LearnSource) -> LearnSourceRow {
        let (kind, channel, number) = match s.kind {
            LearnKind::Cc7 { channel, cc } => (LearnKindRow::Cc7, channel, cc),
            LearnKind::Cc14 { channel, cc } => (LearnKindRow::Cc14, channel, cc),
            LearnKind::PitchBend { channel } => (LearnKindRow::PitchBend, channel, 0),
            LearnKind::Note { channel, note } => (LearnKindRow::Note, channel, note),
        };
        LearnSourceRow {
            port: s.port.clone(),
            kind,
            channel,
            number,
        }
    }
}

/// One learned control of the project (MIDI Learn Maps sheet).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LearnMapRow {
    /// Where its messages come from.
    pub source: LearnSourceRow,
    /// "Knobs · CC 7 ch 1".
    pub text: String,
    /// What it drives.
    pub target: AutomationTargetRow,
    /// The target no longer exists: the map does nothing ("(missing)").
    pub missing: bool,
    /// How its values move the target.
    pub mode: LearnModeRow,
    /// Ruling 18: pick-up on.
    pub pickup: bool,
    /// Whether the map's kind and mode use pick-up.
    pub picks_up: bool,
}

impl From<(&LearnMap, bool)> for LearnMapRow {
    fn from((m, missing): (&LearnMap, bool)) -> LearnMapRow {
        LearnMapRow {
            source: (&m.source).into(),
            text: m.source.describe(),
            target: automation_target_row(m.target),
            missing,
            mode: m.mode.into(),
            pickup: m.pickup,
            picks_up: m.picks_up(),
        }
    }
}

/// One learned command (app preferences, `surfaces.json`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CommandMapRow {
    /// Where its messages come from.
    pub source: LearnSourceRow,
    /// "Pads · Note C2 ch 10".
    pub text: String,
    /// The registry ID it runs.
    pub command: String,
}

/// What a learn is for.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum LearnForRow {
    /// An automatable control of the project.
    Target {
        /// The target.
        target: AutomationTargetRow,
    },
    /// A key-bindable registry command.
    Command {
        /// The registry ID.
        id: String,
    },
}

/// The learn's state.
#[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
pub struct LearnStateRow {
    /// What a waiting learn is for.
    pub waiting: Option<String>,
    /// What the last learn learnt.
    pub last: Option<String>,
}

/// A bank group (Ruling 4): the MCU group, or one HUI unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    /// Every MCU and MCU XT unit.
    Mcu,
    /// One HUI-compatible unit.
    Hui(SurfaceId),
}

/// One configured unit.
struct Unit {
    id: SurfaceId,
    kind: SurfaceKind,
    input: String,
    output: String,
    position: u8,
    /// Both ports were present at the last check.
    connected: bool,
    surface: Box<dyn Surface>,
    /// Actions of this unit that did nothing (a vanished target, a Busy
    /// first value, a skip while recording), counted in `ignored`.
    refused: u64,
    /// The last non-meter messages sent, in hex.
    log: VecDeque<String>,
    /// Meter messages sent.
    meters_sent: u64,
    /// When feedback last went to it.
    fed: Option<Instant>,
}

impl Unit {
    fn new(id: SurfaceId, kind: SurfaceKind, input: String, output: String, position: u8) -> Unit {
        Unit {
            id,
            kind,
            input,
            output,
            position,
            connected: false,
            surface: new_surface(kind),
            refused: 0,
            log: VecDeque::new(),
            meters_sent: 0,
            fed: None,
        }
    }

    fn row(&self) -> SurfaceRow {
        SurfaceRow {
            id: self.id.get(),
            kind: self.kind.into(),
            input: self.input.clone(),
            output: self.output.clone(),
            position: self.position,
            connected: self.connected,
            online: self.surface.online(),
            ignored: self.surface.ignored().saturating_add(self.refused),
        }
    }

    fn feedback_row(&self) -> SurfaceFeedbackRow {
        SurfaceFeedbackRow {
            surface: self.id.get(),
            messages: self.log.iter().cloned().collect(),
            meters_sent: self.meters_sent,
        }
    }

    fn group(&self) -> Group {
        match self.kind {
            SurfaceKind::Mcu | SurfaceKind::McuXt => Group::Mcu,
            SurfaceKind::Hui => Group::Hui(self.id),
        }
    }

    /// Notes a message sent: meters are counted, the rest kept in hex.
    fn sent(&mut self, m: &[u8]) {
        // MCU meters are channel pressure, HUI's polyphonic pressure.
        if m.first().is_some_and(|b| matches!(b & 0xF0, 0xD0 | 0xA0)) {
            self.meters_sent = self.meters_sent.saturating_add(1);
            return;
        }
        if self.log.len() >= LOG_LIMIT {
            self.log.pop_front();
        }
        self.log.push_back(core_surface::midi::hex(m));
    }
}

/// One surface in `surfaces.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: u32,
    kind: SurfaceKind,
    input: String,
    output: String,
    position: u8,
}

/// One learned command in `surfaces.json` (Ruling 9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandEntry {
    /// Where its messages come from.
    pub source: LearnSource,
    /// How its values are read.
    #[serde(default)]
    pub mode: LearnMode,
    /// The registry ID it runs.
    pub command: String,
}

/// `surfaces.json` (Ruling 9).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfacesFile {
    version: u32,
    #[serde(default)]
    surfaces: Vec<Entry>,
    #[serde(default)]
    commands: Vec<CommandEntry>,
}

/// What a learn is for (Ruling 7).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LearnFor {
    /// A project's control.
    Target(AutomationTarget),
    /// A registry command.
    Command(String),
}

/// The waiting learn: the session that asked, what for, and its detector.
struct Learning {
    session: u64,
    what: LearnFor,
    /// What `learn_state` shows.
    text: String,
    learner: Learner,
}

/// A target learn the asking session decided (Ruling 7): it executes
/// `SetLearn` itself.
pub(crate) struct Learnt {
    /// The control.
    pub source: LearnSource,
    /// Its detected mode.
    pub mode: LearnMode,
    /// What it drives.
    pub target: AutomationTarget,
}

/// One action for the session to apply, copied out of the registry.
#[derive(Debug, Clone)]
pub(crate) struct Act {
    /// Whose hand: the unit's or learn's.
    pub holder: TouchHolder,
    /// The unit it came from (for its `ignored`), `None` for learn.
    pub unit: Option<SurfaceId>,
    /// The input message (or tick) it came from: one refusal per message.
    pub tag: u64,
    /// The action.
    pub action: SurfaceAction,
    /// The message's host time (`None`: now).
    pub host_ns: Option<u64>,
    /// Caused by `surface.inject` or `midi.inject` (Ruling 12).
    pub programmatic: bool,
}

/// Where a unit's actions came from: their input message (or tick).
#[derive(Debug, Clone, Copy)]
struct Origin {
    /// One refusal is counted per tag.
    tag: u64,
    /// The message's host time (`None`: now).
    host_ns: Option<u64>,
    /// Caused by `surface.inject` or `midi.inject`.
    programmatic: bool,
}

/// A target a hand moved in this pass, for the partners' feedback.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Moved {
    /// The hand.
    pub holder: TouchHolder,
    /// The target.
    pub target: AutomationTarget,
    /// The strip it belongs to.
    pub strip: Option<StripId>,
}

/// Which units are due feedback this pass (Ruling 13).
pub(crate) struct Due {
    /// The due units and the first strip each shows.
    units: Vec<(SurfaceId, usize)>,
    /// The strip indices the due units show (heard values needed).
    pub visible: BTreeSet<usize>,
    /// A due main MCU in Plug-in shows the plugin's parameters.
    pub plugin: bool,
    /// The learn maps' feedback is due.
    mapper: bool,
}

/// The claims (Ruling 17): what surfaces and learn take out of the lanes.
#[derive(Default)]
struct Claims {
    /// The port keys of configured surfaces' inputs.
    inputs: BTreeSet<u32>,
    /// Their names (for [`input`]).
    names: BTreeSet<String>,
    /// A learn waits: every channel message of other ports is taken.
    learning: bool,
    /// The ports of the command maps and the focused session's maps, by key.
    keys: BTreeMap<u32, String>,
    /// Those maps, for `Mapper::takes`.
    mapper: Mapper,
}

impl Claims {
    /// Whether `bytes` from the port with key `key` are taken.
    fn takes(&self, key: u32, bytes: &[u8]) -> bool {
        if self.inputs.contains(&key) {
            return true;
        }
        if self.learning && learnable(bytes) {
            return true;
        }
        self.keys
            .get(&key)
            .is_some_and(|name| self.mapper.takes(name, bytes))
    }

    /// Whether a surface, a learn or a map listens to `port`.
    fn listens(&self, port: &str) -> bool {
        self.names.contains(port) || self.learning || self.mapper.listens(port)
    }
}

/// One whole channel message a learn takes: a CC, a pitch bend, a note on
/// or off (Ruling 17).
fn learnable(bytes: &[u8]) -> bool {
    match *bytes {
        [s, d1, d2] => d1 < 0x80 && d2 < 0x80 && matches!(s & 0xF0, 0x80 | 0x90 | 0xB0 | 0xE0),
        _ => false,
    }
}

static CLAIMS: LazyLock<RwLock<Claims>> = LazyLock::new(|| RwLock::new(Claims::default()));

/// The process-wide surfaces state (`surfaces::lock()`).
pub(crate) struct Registry {
    /// The units, by ID.
    units: Vec<Unit>,
    /// The bank groups and their offsets (Ruling 4).
    banks: BTreeMap<Group, Bank>,
    /// The session the surfaces drive (Ruling 11).
    focus: Option<u64>,
    /// Releases each session is owed (Ruling 11).
    owed: BTreeMap<u64, Vec<(TouchHolder, AutomationTarget)>>,
    /// The transport, made on the first surface or learn (Ruling 10).
    ports: Option<Box<dyn Ports>>,
    /// When the ports were last refreshed.
    checked: Option<Instant>,
    /// The learned commands of `surfaces.json`.
    commands: Vec<CommandEntry>,
    /// Problem lines for `state.get surfaces` (the last 16).
    problems: Vec<String>,
    /// `surfaces.json` was read (or found missing): saving cannot lose a
    /// file not yet read (Task 7 review M4).
    loaded: bool,
    /// The focused session's maps and the command maps, driving targets.
    mapper: Mapper,
    /// The focused session's maps as `mapper` has them.
    maps: Vec<LearnMap>,
    /// When the maps' feedback last went out.
    mapper_fed: Option<Instant>,
    /// The waiting learn.
    learn: Option<Learning>,
    /// What the last learn learnt.
    last: Option<String>,
    /// Input messages routed so far (tags for [`Act`]).
    tags: u64,
}

impl Registry {
    fn new() -> Registry {
        Registry {
            units: Vec::new(),
            banks: BTreeMap::new(),
            focus: None,
            owed: BTreeMap::new(),
            ports: None,
            checked: None,
            commands: Vec::new(),
            problems: Vec::new(),
            loaded: false,
            mapper: Mapper::default(),
            maps: Vec::new(),
            mapper_fed: None,
            learn: None,
            last: None,
            tags: 0,
        }
    }

    /// The transport, made now if there is none.
    fn ports(&mut self) -> &mut dyn Ports {
        self.ports.get_or_insert_with(new_ports).as_mut()
    }

    /// Every port name the units use.
    fn names(&self) -> BTreeSet<String> {
        self.units
            .iter()
            .flat_map(|u| [u.input.clone(), u.output.clone()])
            .collect()
    }

    /// A problem line, kept with the last [`PROBLEM_LIMIT`] (a repeat of the
    /// last one is not added again).
    fn problem(&mut self, line: String) {
        if self.problems.last() == Some(&line) {
            return;
        }
        self.problems.push(line);
        if self.problems.len() > PROBLEM_LIMIT {
            self.problems.remove(0);
        }
    }

    /// Refreshes the transport, stamps the 2 s timer, then checks every unit.
    fn refresh_and_check(&mut self, now: Instant) {
        let names = self.names();
        let ports = self.ports();
        ports.want(&names);
        ports.refresh();
        self.checked = Some(now);
        self.check();
    }

    /// Marks each unit connected or not from the transport's lists: a unit
    /// that lost a port releases what it holds; one that got both back is
    /// refreshed and greeted.
    fn check(&mut self) {
        let ports = self.ports();
        let sources = ports.sources();
        let dests = ports.destinations();
        for i in 0..self.units.len() {
            let Some(u) = self.units.get(i) else { continue };
            let present = sources.contains(&u.input) && dests.contains(&u.output);
            match (u.connected, present) {
                (true, false) => self.disconnect(i),
                (false, true) => self.connect(i),
                _ => {}
            }
        }
    }

    /// Unit `i`'s ports are present: a full refresh and its `hello()`.
    fn connect(&mut self, i: usize) {
        let Some(u) = self.units.get_mut(i) else {
            return;
        };
        u.connected = true;
        u.surface.refresh();
        let hello = u.surface.hello();
        self.send(i, &hello);
    }

    /// Sends `messages` to unit `i`; a failed send disconnects it at once.
    fn send(&mut self, i: usize, messages: &[Vec<u8>]) -> bool {
        let Some(out) = self.units.get(i).map(|u| u.output.clone()) else {
            return false;
        };
        for m in messages {
            if !self.ports().send(&out, m) {
                self.disconnect(i);
                return false;
            }
            if let Some(u) = self.units.get_mut(i) {
                u.sent(m);
            }
        }
        true
    }

    /// Unit `i` lost a port: what it holds is owed to the focused session
    /// (which holds its touches), and it reads Disconnected.
    fn disconnect(&mut self, i: usize) {
        let Some(u) = self.units.get_mut(i) else {
            return;
        };
        if !u.connected {
            return;
        }
        u.connected = false;
        let id = u.id;
        let held = u.surface.release_all();
        self.owe(TouchHolder::Surface(id), held);
    }

    /// Records the releases of `targets` held by `holder` for the focused
    /// session (none focused: nobody holds them).
    fn owe(&mut self, holder: TouchHolder, targets: Vec<AutomationTarget>) {
        if targets.is_empty() {
            return;
        }
        if let Some(f) = self.focus {
            self.owed
                .entry(f)
                .or_default()
                .extend(targets.into_iter().map(|t| (holder, t)));
        }
    }

    /// Recomputes the bank groups from the units, keeping their offsets.
    fn rebank(&mut self) {
        let mut groups: BTreeMap<Group, usize> = BTreeMap::new();
        for u in &self.units {
            *groups.entry(u.group()).or_default() += 1;
        }
        self.banks.retain(|g, _| groups.contains_key(g));
        for (g, n) in groups {
            self.banks.entry(g).or_default().units = n;
        }
    }

    /// The first strip unit `i` shows: its group's offset plus 8 per unit
    /// before it in position order (Ruling 4).
    fn first(&self, i: usize) -> usize {
        let Some(u) = self.units.get(i) else {
            return 0;
        };
        let g = u.group();
        let rank = self
            .units
            .iter()
            .filter(|o| o.group() == g && o.position < u.position)
            .count();
        self.banks.get(&g).map_or(0, |b| b.first(rank))
    }

    /// Clamps every group's offset to the strips there are (pre-flight
    /// M20: after a mixer change).
    fn clamp(&mut self, strips: usize) {
        for b in self.banks.values_mut() {
            b.clamp(strips);
        }
    }

    /// The lowest free ID (Ruling 4).
    fn free_id(&self) -> Option<SurfaceId> {
        (0..u32::from(SurfaceId::COUNT))
            .filter_map(SurfaceId::new)
            .find(|id| self.units.iter().all(|u| u.id != *id))
    }

    /// Reads `path` once (Task 7 review M4), before anything is saved: a
    /// save before the load would replace the user's file.
    fn ensure_loaded_from(&mut self, path: &Path, now: Instant) {
        if self.loaded {
            return;
        }
        self.load_from(path, now);
    }

    /// Reads `path` (Ruling 9) into the registry, replacing the units and
    /// commands; a bad file is kept aside.
    fn load_from(&mut self, path: &Path, now: Instant) {
        let loaded = match std::fs::read_to_string(path) {
            Ok(text) => Some(parse_file(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(e.to_string())),
        };
        self.clear();
        self.commands.clear();
        self.loaded = true;
        match loaded {
            None => {}
            Some(Ok((units, commands))) => {
                self.units = units;
                self.commands = commands;
            }
            Some(Err(why)) => {
                let bad = bad_file(path);
                let kept = match std::fs::rename(path, &bad) {
                    Ok(()) => format!("kept aside as {FILE}.bad"),
                    Err(e) => format!("could not be kept aside ({e})"),
                };
                self.problem(format!(
                    "{FILE} could not be read and was {kept}; VizDAW starts with no surfaces: {why}"
                ));
            }
        }
        self.rebank();
        self.reset_mapper_commands();
        self.publish();
        if !self.units.is_empty() {
            self.refresh_and_check(now);
        }
    }

    /// Adds a unit (Ruling 4's checks), refreshes the ports and greets it
    /// if connected. The caller saves.
    fn add(
        &mut self,
        kind: SurfaceKind,
        input: String,
        output: String,
        position: Option<u8>,
        now: Instant,
    ) -> Result<SurfaceId, VizError> {
        let position = check_unit(&self.units, kind, &input, &output, position)?;
        let id = self
            .free_id()
            .ok_or_else(|| crate::events::bad("VizDAW drives at most 16 surfaces"))?;
        self.units
            .push(Unit::new(id, kind, input, output, position));
        self.units.sort_by_key(|u| u.id);
        self.rebank();
        self.refresh_and_check(now);
        Ok(id)
    }

    /// Removes unit `id`: what it holds is owed to the focused session. The
    /// caller saves.
    fn remove(&mut self, id: u32) -> Result<(), VizError> {
        let i = self
            .units
            .iter()
            .position(|u| u.id.get() == id)
            .ok_or_else(|| crate::events::bad(&format!("there is no surface {id}")))?;
        let mut u = self.units.remove(i);
        let held = u.surface.release_all();
        self.owe(TouchHolder::Surface(u.id), held);
        self.rebank();
        Ok(())
    }

    /// Forgets every unit (each owes what it holds).
    fn clear(&mut self) {
        for mut u in std::mem::take(&mut self.units) {
            let held = u.surface.release_all();
            self.owe(TouchHolder::Surface(u.id), held);
        }
        self.rebank();
    }

    /// The file as it stands now.
    fn file(&self) -> SurfacesFile {
        SurfacesFile {
            version: VERSION,
            surfaces: self
                .units
                .iter()
                .map(|u| Entry {
                    id: u.id.get(),
                    kind: u.kind,
                    input: u.input.clone(),
                    output: u.output.clone(),
                    position: u.position,
                })
                .collect(),
            commands: self.commands.clone(),
        }
    }

    /// Writes `surfaces.json` (atomically, fsynced; Task 7 review M2); a
    /// failure becomes a problem line.
    fn save(&mut self) {
        if let Err(e) = write_file(&support_file(), &self.file()) {
            self.problem(format!("Could not save the surfaces ({FILE}): {e}"));
        }
    }

    /// The command maps as the mapper takes them.
    fn command_maps(&self) -> Vec<CommandMap> {
        self.commands
            .iter()
            .map(|c| CommandMap {
                source: c.source.clone(),
                command: c.command.clone(),
            })
            .collect()
    }

    /// Gives the mapper the command maps now (the maps stay).
    fn reset_mapper_commands(&mut self) {
        let cmds = self.command_maps();
        self.mapper.set(self.maps.clone(), cmds);
    }

    /// Rewrites the claims (Ruling 17) from the units, the waiting learn,
    /// the command maps and the focused session's maps.
    fn publish(&self) {
        let names: BTreeSet<String> = self.units.iter().map(|u| u.input.clone()).collect();
        let maps = if self.focus.is_some() {
            self.maps.clone()
        } else {
            Vec::new()
        };
        let keys: BTreeMap<u32, String> = maps
            .iter()
            .map(|m| &m.source.port)
            .chain(self.commands.iter().map(|c| &c.source.port))
            .map(|p| (core_engine::input::port_key(p), p.clone()))
            .collect();
        let mut mapper = Mapper::default();
        mapper.set(maps, self.command_maps());
        let claims = Claims {
            inputs: names
                .iter()
                .map(|n| core_engine::input::port_key(n))
                .collect(),
            names,
            learning: self.learn.is_some(),
            keys,
            mapper,
        };
        *CLAIMS.write().unwrap_or_else(PoisonError::into_inner) = claims;
    }

    /// The 2 s port check (Ruling 10), while any surface exists.
    fn service(&mut self, now: Instant) {
        if self.units.is_empty() {
            return;
        }
        let due = self
            .checked
            .is_none_or(|c| now.saturating_duration_since(c) >= PORT_CHECK);
        if due {
            self.refresh_and_check(now);
        }
    }

    /// Focus moves to `id` with its document's `maps` (Ruling 11): the old
    /// session is owed every unit's holds and the mapper's; the units get a
    /// full refresh from the new one.
    fn focus(&mut self, id: u64, maps: Vec<LearnMap>) {
        if self.focus == Some(id) {
            self.set_maps(maps);
            return;
        }
        for i in 0..self.units.len() {
            let Some(u) = self.units.get_mut(i) else {
                continue;
            };
            let held = u.surface.release_all();
            u.surface.refresh();
            let holder = TouchHolder::Surface(u.id);
            self.owe(holder, held);
        }
        let learnt: Vec<AutomationTarget> = self
            .mapper
            .release_all()
            .into_iter()
            .filter_map(|a| match a {
                SurfaceAction::IdleRelease(t, _) | SurfaceAction::Gesture(t, _) => Some(t),
                _ => None,
            })
            .collect();
        self.owe(TouchHolder::Learn, learnt);
        self.focus = Some(id);
        self.mapper = Mapper::default();
        self.maps = maps.clone();
        self.mapper.set(maps, self.command_maps());
        self.mapper_fed = None;
        self.publish();
    }

    /// The focused session's maps are `maps` now (a learn, undo, redo,
    /// revert or open; Ruling 17): the mapper keeps the state of unchanged
    /// maps, and queues the releases of targets no map drives (Ruling
    /// T5-1); the claims are rewritten.
    fn set_maps(&mut self, maps: Vec<LearnMap>) {
        if self.maps == maps {
            return;
        }
        self.maps = maps.clone();
        let cmds = self.command_maps();
        self.mapper.set(maps, cmds);
        self.publish();
    }

    /// The unit index of `id`.
    fn index(&self, id: SurfaceId) -> Option<usize> {
        self.units.iter().position(|u| u.id == id)
    }

    /// Unit `i`'s actions: Bank and Channel move its group (then a full
    /// refresh of the group); the main unit's V-pot assignment goes to the
    /// extenders (Ruling T1-M2); the rest are handed out.
    fn unit_actions(
        &mut self,
        i: usize,
        acts: Vec<SurfaceAction>,
        from: Origin,
        strips: usize,
        out: &mut Vec<Act>,
    ) {
        let Some(u) = self.units.get(i) else { return };
        let (id, group, kind) = (u.id, u.group(), u.kind);
        if kind == SurfaceKind::Mcu
            && let Some(a) = u.surface.assign()
        {
            for x in &mut self.units {
                if x.kind == SurfaceKind::McuXt {
                    x.surface.set_assign(a);
                }
            }
        }
        for a in acts {
            match a {
                SurfaceAction::Bank(_) | SurfaceAction::Channel(_) => {
                    if let Some(b) = self.banks.get_mut(&group) {
                        b.shift(&a, strips);
                    }
                    for x in &mut self.units {
                        if x.group() == group {
                            x.surface.refresh();
                        }
                    }
                }
                action => out.push(Act {
                    holder: TouchHolder::Surface(id),
                    unit: Some(id),
                    tag: from.tag,
                    action,
                    host_ns: from.host_ns,
                    programmatic: from.programmatic,
                }),
            }
        }
    }

    /// The next tag.
    fn tag(&mut self) -> u64 {
        self.tags = self.tags.wrapping_add(1);
        self.tags
    }

    /// Steps 2–3: the queue's messages, each routed to the surface whose
    /// input it came from; else to a waiting learn (ports no surface uses);
    /// else to the mapper.
    fn route(
        &mut self,
        strips: usize,
        current: &dyn Fn(AutomationTarget) -> Option<f64>,
    ) -> Vec<Act> {
        let items = take_input();
        let mut out = Vec::new();
        for item in items {
            let tag = self.tag();
            // Injected bytes and an unknown host time are stamped now.
            let host_ns = (item.host_ns != 0).then_some(item.host_ns);
            if let Some(i) = self.units.iter().position(|u| u.input == item.port) {
                let acts = match self.units.get_mut(i) {
                    Some(u) => u.surface.input(&item.bytes, item.at),
                    None => continue,
                };
                let from = Origin {
                    tag,
                    host_ns,
                    programmatic: item.programmatic,
                };
                self.unit_actions(i, acts, from, strips, &mut out);
            } else if let Some(l) = self.learn.as_mut() {
                l.learner.feed(&item.port, &item.bytes, item.at);
            } else {
                let acts = self.mapper.input(&item.port, &item.bytes, item.at, current);
                out.extend(acts.into_iter().map(|action| Act {
                    holder: TouchHolder::Learn,
                    unit: None,
                    tag,
                    action,
                    host_ns,
                    programmatic: item.programmatic,
                }));
            }
        }
        out
    }

    /// Step 5: every unit's and the mapper's timed actions.
    fn tick(&mut self, now: Instant, strips: usize) -> Vec<Act> {
        let mut out = Vec::new();
        for i in 0..self.units.len() {
            let acts = match self.units.get_mut(i) {
                Some(u) => u.surface.tick(now),
                None => continue,
            };
            let tag = self.tag();
            let from = Origin {
                tag,
                host_ns: None,
                programmatic: false,
            };
            self.unit_actions(i, acts, from, strips, &mut out);
        }
        for action in self.mapper.tick(now) {
            let tag = self.tag();
            out.push(Act {
                holder: TouchHolder::Learn,
                unit: None,
                tag,
                action,
                host_ns: None,
                programmatic: false,
            });
        }
        out
    }

    /// Step 8's choice (Ruling 13): connected units whose last feedback was
    /// at least 20 ms ago, and at once those showing a target another hand
    /// moved in this pass; the same for the maps' feedback.
    fn due(&mut self, now: Instant, order: &[StripId], moved: &[Moved]) -> Due {
        self.clamp(order.len());
        let index = |s: StripId| order.iter().position(|x| *x == s);
        let mut due = Due {
            units: Vec::new(),
            visible: BTreeSet::new(),
            plugin: false,
            mapper: false,
        };
        for i in 0..self.units.len() {
            let first = self.first(i);
            let Some(u) = self.units.get(i) else { continue };
            if !u.connected {
                continue;
            }
            let main = u.kind == SurfaceKind::Mcu;
            let shows = |m: &Moved| {
                if m.holder == TouchHolder::Surface(u.id) {
                    return false;
                }
                if matches!(m.target, AutomationTarget::PluginParam(..)) {
                    return main;
                }
                match m.strip.map(index) {
                    Some(Some(k)) => (first..first.saturating_add(8)).contains(&k),
                    // The master (not in the order) shows on the main unit.
                    Some(None) => main,
                    None => false,
                }
            };
            let timed = u
                .fed
                .is_none_or(|f| now.saturating_duration_since(f) >= FEEDBACK_EVERY);
            if timed || moved.iter().any(shows) {
                due.visible.extend(first..first.saturating_add(8));
                due.plugin |= main && u.surface.assign() == Some(core_surface::Assign::Plugin);
                due.units.push((u.id, first));
            }
        }
        let timed = self
            .mapper_fed
            .is_none_or(|f| now.saturating_duration_since(f) >= FEEDBACK_EVERY);
        let partner = moved.iter().any(|m| {
            m.holder != TouchHolder::Learn && self.maps.iter().any(|x| x.target == m.target)
        });
        due.mapper = !self.maps.is_empty() && (timed || partner);
        due
    }

    /// Step 8: feedback to the due units from `view`, and the maps' with
    /// the heard values `current`.
    fn feedback(
        &mut self,
        now: Instant,
        due: &Due,
        view: &MixerView,
        current: &dyn Fn(AutomationTarget) -> Option<f64>,
    ) {
        for (id, first) in &due.units {
            let Some(i) = self.index(*id) else { continue };
            let msgs = match self.units.get_mut(i) {
                Some(u) if u.connected => {
                    u.fed = Some(now);
                    u.surface.feedback(view, *first, now)
                }
                _ => continue,
            };
            self.send(i, &msgs);
        }
        if due.mapper {
            self.mapper_fed = Some(now);
            let dests = self.ports().destinations();
            for (port, msg) in self.mapper.feedback(current) {
                // Back to the output with the source's name, if there is one.
                if dests.contains(&port) {
                    let _ = self.ports().send(&port, &msg);
                }
            }
        }
    }

    /// The units go blank (Ruling 11): names cleared, faders down, LEDs
    /// off, displays blank; a fader the unit reports touched is left alone.
    fn blank(&mut self, now: Instant) {
        let empty = MixerView::default();
        for i in 0..self.units.len() {
            let msgs = match self.units.get_mut(i) {
                Some(u) if u.connected => {
                    u.surface.refresh();
                    u.fed = Some(now);
                    u.surface.feedback(&empty, 0, now)
                }
                _ => continue,
            };
            self.send(i, &msgs);
            if let Some(u) = self.units.get_mut(i) {
                // The next session's feedback is complete.
                u.surface.refresh();
            }
        }
    }

    /// Session `id` closes (Ruling 11): its learn is cancelled, its owed
    /// releases are void (`release_touches` ended its touches); if it was
    /// focused, focus goes to none and the units go blank.
    fn session_closed(&mut self, id: u64, now: Instant) {
        if self.learn.as_ref().is_some_and(|l| l.session == id) {
            self.learn = None;
        }
        self.owed.remove(&id);
        if self.focus == Some(id) {
            self.focus = None;
            // Its touches ended with the window: nothing is owed for them.
            let _ = self.mapper.release_all();
            self.mapper = Mapper::default();
            self.maps.clear();
            self.reset_mapper_commands();
            self.blank(now);
        }
        self.publish();
    }

    /// Step 6 for session `id`: its learn decides, if it waited long
    /// enough. A command map is stored and saved here; a target learn is
    /// handed to the session.
    fn decide(&mut self, id: u64, now: Instant) -> Option<Learnt> {
        let l = self.learn.as_mut().filter(|l| l.session == id)?;
        let (source, mode) = l.learner.decide(now)?;
        let l = self.learn.take()?;
        let what = format!("{} → {}", source.describe(), l.text);
        self.last = Some(what);
        let out = match l.what {
            LearnFor::Target(target) => Some(Learnt {
                source,
                mode,
                target,
            }),
            LearnFor::Command(command) => {
                self.commands.retain(|c| c.source != source);
                self.commands.push(CommandEntry {
                    source,
                    mode,
                    command,
                });
                self.reset_mapper_commands();
                self.save();
                None
            }
        };
        self.publish();
        out
    }

    /// A learn for `what`, asked by `session`: the transport is made if
    /// there is none (Ruling 10), and every other learn is replaced.
    fn start_learn(&mut self, session: u64, what: LearnFor, text: String) {
        let _ = self.ports();
        self.learn = Some(Learning {
            session,
            what,
            text,
            learner: Learner::default(),
        });
        self.publish();
    }

    /// `state.get surfaces`: with feedback rows for a document.
    fn state(&self, feedback: bool) -> SurfaceStateRow {
        let mut problems = self.problems.clone();
        let dropped = dropped();
        if dropped > 0 {
            problems.push(format!(
                "{dropped} MIDI messages from surfaces were dropped"
            ));
        }
        SurfaceStateRow {
            surfaces: self.units.iter().map(Unit::row).collect(),
            feedback: if feedback {
                self.units.iter().map(Unit::feedback_row).collect()
            } else {
                Vec::new()
            },
            learning: self.learn.as_ref().map(|l| l.text.clone()),
            problems,
        }
    }
}

/// The transport for this build and mode (Ruling 10; Global Constraints).
fn new_ports() -> Box<dyn Ports> {
    #[cfg(feature = "test-hooks")]
    {
        Box::new(ports::FakePorts::default())
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        if crate::host::offline_mode() {
            return Box::new(ports::FakePorts::default());
        }
        #[cfg(target_os = "macos")]
        {
            Box::new(ports::CoreMidiPorts::new())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Box::new(ports::NoPorts)
        }
    }
}

/// A port name is not empty, at most 255 bytes, without control characters
/// (`crate::live::parse_injection`'s rule).
fn check_port(what: &str, name: &str) -> Result<(), VizError> {
    if name.is_empty() || name.len() > MAX_PORT || name.chars().any(char::is_control) {
        return Err(crate::events::bad(&format!(
            "`{what}` must name a MIDI port (1 to {MAX_PORT} bytes, no control characters)"
        )));
    }
    Ok(())
}

/// Ruling 4's checks for a new unit beside `units`; the position it gets
/// (`None`: the next free one in its group).
fn check_unit(
    units: &[Unit],
    kind: SurfaceKind,
    input: &str,
    output: &str,
    position: Option<u8>,
) -> Result<u8, VizError> {
    check_port("input", input)?;
    check_port("output", output)?;
    if units.len() >= MAX_SURFACES {
        return Err(crate::events::bad("VizDAW drives at most 16 surfaces"));
    }
    let count = |k: SurfaceKind| units.iter().filter(|u| u.kind == k).count();
    match kind {
        SurfaceKind::Mcu if count(SurfaceKind::Mcu) >= 1 => {
            return Err(crate::events::bad(GROUP_LIMIT));
        }
        SurfaceKind::McuXt if count(SurfaceKind::McuXt) >= MAX_EXTENDERS => {
            return Err(crate::events::bad(GROUP_LIMIT));
        }
        _ => {}
    }
    let position = match kind {
        SurfaceKind::Hui => match position {
            None | Some(0) => 0,
            Some(_) => {
                return Err(crate::events::bad(
                    "a HUI-compatible unit is a bank of its own: `position` must be 0",
                ));
            }
        },
        SurfaceKind::Mcu | SurfaceKind::McuXt => {
            let taken = |p: u8| {
                units
                    .iter()
                    .any(|u| u.group() == Group::Mcu && u.position == p)
            };
            match position {
                Some(p) if p >= POSITIONS => {
                    return Err(crate::events::bad("`position` must be 0–3"));
                }
                Some(p) if taken(p) => {
                    return Err(crate::events::bad(&format!("position {p} is taken")));
                }
                Some(p) => p,
                None => (0..POSITIONS)
                    .find(|p| !taken(*p))
                    .ok_or_else(|| crate::events::bad(GROUP_LIMIT))?,
            }
        }
    };
    for u in units {
        if u.input == input {
            return Err(crate::events::bad(&format!(
                "“{input}” is used by surface {}",
                u.id.get()
            )));
        }
        if u.output == output {
            return Err(crate::events::bad(&format!(
                "“{output}” is used by surface {}",
                u.id.get()
            )));
        }
    }
    Ok(position)
}

/// Whether Settings › Keys can bind command `id` (Ruling 8): it exists,
/// needs no parameter and never always opens a dialog; why not, if not.
/// The Surfaces group is the channel's: `surface.learn` and
/// `surface.unlearn` need no required parameter, but without one of
/// theirs they do nothing (Task 9 review M2).
pub(crate) fn bindable(id: &str) -> Result<(), String> {
    let Some(spec) = core_control::registry::find(id) else {
        return Err(format!("unknown command “{id}”"));
    };
    if spec.menu == "Surfaces"
        || spec.params.iter().any(|p| p.required)
        || spec.dialogs.iter().any(|d| d.always)
    {
        return Err(format!(
            "“{id}” cannot be learned: only commands a key can run (no required parameter, no dialog)"
        ));
    }
    Ok(())
}

/// A learned command's checks, on load and in `learn_command` alike (Task
/// 7 review M6): its source's (Ruling 6) and a key-bindable registry ID.
fn check_command(c: &CommandEntry) -> Result<(), String> {
    c.source.check().map_err(|e| e.to_string())?;
    bindable(&c.command)
}

/// The units and commands of a file's text, or why it is refused (Ruling
/// 9: it parses, has version 1, and every entry passes `surface_add`'s
/// checks with a unique ID below 16).
fn parse_file(text: &str) -> Result<(Vec<Unit>, Vec<CommandEntry>), String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(v) if v == u64::from(VERSION) => {}
        Some(v) => return Err(format!("it has version {v}; this VizDAW reads {VERSION}")),
        None => return Err("it has no version".to_string()),
    }
    let file: SurfacesFile = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let mut units: Vec<Unit> = Vec::new();
    for e in file.surfaces {
        let id = SurfaceId::new(e.id).ok_or_else(|| format!("surface ID {} is not 0–15", e.id))?;
        if units.iter().any(|u| u.id == id) {
            return Err(format!("surface ID {} appears twice", e.id));
        }
        let position = check_unit(&units, e.kind, &e.input, &e.output, Some(e.position))
            .map_err(|err| format!("surface {}: {err}", e.id))?;
        units.push(Unit::new(id, e.kind, e.input, e.output, position));
    }
    units.sort_by_key(|u| u.id);
    let mut sources = BTreeSet::new();
    for c in &file.commands {
        check_command(c)?;
        if !sources.insert(c.source.clone()) {
            return Err(format!("{} is learned twice", c.source.describe()));
        }
    }
    Ok((units, file.commands))
}

/// Writes `file` to `path` with the codebase's atomic writer (temp file,
/// fsync, rename, directory fsync; Task 7 review M2).
fn write_file(path: &Path, file: &SurfacesFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(file).map_err(std::io::Error::other)?;
    core_model::file::write_atomic(path, text.as_bytes()).map_err(std::io::Error::other)
}

/// `support_dir()/surfaces.json`.
fn support_file() -> PathBuf {
    crate::recovery::support_dir().join(FILE)
}

/// `surfaces.json.bad`, beside the file.
fn bad_file(path: &Path) -> PathBuf {
    path.with_extension("json.bad")
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::new()));

/// The registry (poison-tolerant). Never taken while holding the input
/// queue; a session's `inner` may be held (lock order).
pub(crate) fn lock() -> MutexGuard<'static, Registry> {
    lock_mutex(&REGISTRY)
}

// ---------------------------------------------------------------------
// The surfaces clock.

/// The clock's start, captured at first use.
static START: OnceLock<Instant> = OnceLock::new();

/// Added by `debug_advance` (milliseconds).
static ADVANCED_MS: AtomicU64 = AtomicU64::new(0);

/// The real time the clock stands at while frozen (`u64::MAX`: running).
static FROZEN_NS: AtomicU64 = AtomicU64::new(u64::MAX);

/// The surfaces clock: real time plus what `debug_advance` added (real
/// time stands still while `debug_freeze_clock` holds it).
pub(crate) fn now() -> Instant {
    let start = *START.get_or_init(Instant::now);
    let ahead = Duration::from_millis(ADVANCED_MS.load(Ordering::Relaxed));
    let real = match FROZEN_NS.load(Ordering::Relaxed) {
        u64::MAX => start.elapsed(),
        ns => Duration::from_nanos(ns),
    };
    start
        .checked_add(real)
        .and_then(|t| t.checked_add(ahead))
        .unwrap_or_else(Instant::now)
}

// ---------------------------------------------------------------------
// The input queue (Ruling 10) and the claims (Ruling 17).

/// One message from a port.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Input {
    /// The source's name.
    pub port: String,
    /// Its host time in nanoseconds (0: now).
    pub host_ns: u64,
    /// The surfaces clock at the push.
    pub at: Instant,
    /// The bytes.
    pub bytes: Vec<u8>,
    /// From `surface.inject` or `midi.inject` (Ruling 12).
    pub programmatic: bool,
}

/// The queue behind [`INPUT`].
#[derive(Debug)]
pub(crate) struct Queue {
    items: VecDeque<Input>,
    /// Pushes refused because the queue was full.
    dropped: u64,
}

/// The input queue, its own lock (Ruling 10): at most [`INPUT_LIMIT`]
/// messages, drops counted.
static INPUT: Mutex<Queue> = Mutex::new(Queue {
    items: VecDeque::new(),
    dropped: 0,
});

/// Messages lost because a lock was held: the queue being taken, the
/// claims or the CoreMIDI name map being rewritten (Task 7 review M3).
pub(crate) static INPUT_BUSY: AtomicU64 = AtomicU64::new(0);

/// Whether surfaces or learn take `bytes` from the port with this key
/// (Ruling 17): a surface's input, a channel message while a learn waits,
/// or a message a command map or the focused session's learn map takes.
/// Never waits: while the claims are being rewritten the message is passed
/// on (false). It answers for one message (`surfaces::takes` is asked per
/// message on every path; a packet of several is never taken whole).
pub(crate) fn takes(port_key: u32, bytes: &[u8]) -> bool {
    match CLAIMS.try_read() {
        Ok(c) => c.takes(port_key, bytes),
        Err(TryLockError::Poisoned(p)) => p.into_inner().takes(port_key, bytes),
        Err(TryLockError::WouldBlock) => false,
    }
}

/// Raw input from a port (CoreMIDI's surfaces listener): queued with its
/// host time (0: now) when a surface, a learn or a map listens to `port`;
/// never waits. True when one listens.
#[cfg_attr(
    any(feature = "test-hooks", not(target_os = "macos")),
    allow(dead_code) // Only CoreMIDI's sink calls it; tests inject.
)]
pub(crate) fn input(port: &str, bytes: &[u8], host_ns: u64) -> bool {
    push(port, bytes, host_ns, false)
}

/// Injected bytes (`midi.inject`, Ruling 16): as [`input`] at host time 0,
/// marked programmatic (Ruling 12).
pub(crate) fn inject(port: &str, bytes: &[u8]) -> bool {
    push(port, bytes, 0, true)
}

/// Queues `bytes` from `port` when something listens to it.
fn push(port: &str, bytes: &[u8], host_ns: u64, programmatic: bool) -> bool {
    let listens = match CLAIMS.try_read() {
        Ok(c) => c.listens(port),
        Err(TryLockError::Poisoned(p)) => p.into_inner().listens(port),
        Err(TryLockError::WouldBlock) => {
            // Lost to a rewrite: counted, so `problems` explains it.
            INPUT_BUSY.fetch_add(1, Ordering::Relaxed);
            return false;
        }
    };
    if !listens {
        return false;
    }
    enqueue(port, bytes, host_ns, programmatic);
    true
}

/// Pushes one item (`try_lock`, drop and count, never wait).
fn enqueue(port: &str, bytes: &[u8], host_ns: u64, programmatic: bool) -> bool {
    let at = now();
    let mut q = match INPUT.try_lock() {
        Ok(q) => q,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => {
            INPUT_BUSY.fetch_add(1, Ordering::Relaxed);
            return false;
        }
    };
    if q.items.len() >= INPUT_LIMIT {
        q.dropped = q.dropped.saturating_add(1);
        return false;
    }
    q.items.push_back(Input {
        port: port.to_string(),
        host_ns,
        at,
        bytes: bytes.to_vec(),
        programmatic,
    });
    true
}

/// Takes the whole queue.
pub(crate) fn take_input() -> Vec<Input> {
    let mut q = lock_mutex(&INPUT);
    std::mem::take(&mut q.items).into()
}

/// How many messages were dropped so far (full queue or a busy lock).
fn dropped() -> u64 {
    lock_mutex(&INPUT)
        .dropped
        .saturating_add(INPUT_BUSY.load(Ordering::Relaxed))
}

// ---------------------------------------------------------------------
// The session's side (`Session::focus_surfaces`, `idle`, `close`).

/// Makes session `id`, whose document has learn maps `maps`, the one the
/// surfaces drive (Ruling 11).
pub(crate) fn focus(id: u64, maps: Vec<LearnMap>) {
    lock().focus(id, maps);
}

/// Steps 0–1 for session `id`: when focused, the 2 s port check; then the
/// releases it is owed. Answers them and whether it is focused.
pub(crate) fn begin(id: u64, now: Instant) -> (Vec<(TouchHolder, AutomationTarget)>, bool) {
    let mut r = lock();
    let focused = r.focus == Some(id);
    if focused {
        r.service(now);
    }
    (r.owed.remove(&id).unwrap_or_default(), focused)
}

/// Step 3's map check: session `id`'s document has `maps` (only the
/// focused session's count).
pub(crate) fn sync_maps(id: u64, maps: &[LearnMap]) {
    let mut r = lock();
    if r.focus == Some(id) && r.maps != maps {
        r.set_maps(maps.to_vec());
    }
}

/// Steps 2–5 for the focused session `id`: input routed, then every timed
/// action; `strips` is its mixer's strip count (banks), `current` its
/// heard values. Nothing for another session.
pub(crate) fn route_and_tick(
    id: u64,
    now: Instant,
    strips: usize,
    current: &dyn Fn(AutomationTarget) -> Option<f64>,
) -> Vec<Act> {
    let mut r = lock();
    if r.focus != Some(id) {
        return Vec::new();
    }
    let mut out = r.route(strips, current);
    out.extend(r.tick(now, strips));
    out
}

/// Counts each refused message once in its unit's `ignored`.
pub(crate) fn refused(units: &[(SurfaceId, u64)]) {
    if units.is_empty() {
        return;
    }
    let mut r = lock();
    let mut seen = BTreeSet::new();
    for (id, tag) in units {
        if seen.insert((*id, *tag))
            && let Some(u) = r.units.iter_mut().find(|u| u.id == *id)
        {
            u.refused = u.refused.saturating_add(1);
        }
    }
}

/// Which units of the focused session `id` are due feedback.
pub(crate) fn due(id: u64, now: Instant, order: &[StripId], moved: &[Moved]) -> Option<Due> {
    let mut r = lock();
    if r.focus != Some(id) {
        return None;
    }
    let due = r.due(now, order, moved);
    (!due.units.is_empty() || due.mapper).then_some(due)
}

/// Sends the due feedback of session `id`.
pub(crate) fn feedback(
    id: u64,
    now: Instant,
    due: &Due,
    view: &MixerView,
    current: &dyn Fn(AutomationTarget) -> Option<f64>,
) {
    let mut r = lock();
    if r.focus == Some(id) {
        r.feedback(now, due, view, current);
    }
}

/// Step 6 for session `id` (focused or not).
pub(crate) fn decide(id: u64, now: Instant) -> Option<Learnt> {
    lock().decide(id, now)
}

/// Whether session `id` asked for the waiting learn.
pub(crate) fn learning_for(id: u64) -> bool {
    lock().learn.as_ref().is_some_and(|l| l.session == id)
}

/// Starts a target learn for session `id` (Ruling 7).
pub(crate) fn learn_target(id: u64, t: AutomationTarget, text: String) {
    lock().start_learn(id, LearnFor::Target(t), text);
}

/// Feeds `bytes` to surface `surface` as if from its input (programmatic).
/// `Model` for an unknown surface, `Busy` when the queue is full.
pub(crate) fn inject_unit(surface: u32, bytes: &[u8]) -> Result<(), VizError> {
    let port = lock()
        .units
        .iter()
        .find(|u| u.id.get() == surface)
        .map(|u| u.input.clone())
        .ok_or_else(|| crate::events::bad(&format!("there is no surface {surface}")))?;
    if !enqueue(&port, bytes, 0, true) {
        return Err(VizError::Busy {
            message: "the surfaces' input queue is full".into(),
        });
    }
    Ok(())
}

/// The state with feedback rows (a document asked).
pub(crate) fn state_with_feedback() -> SurfaceStateRow {
    lock().state(true)
}

/// The session closes: cancel its learn; if it was focused, blank the
/// units (Ruling 11).
pub(crate) fn session_closed(id: u64) {
    lock().session_closed(id, now());
}

/// The meters a session holds for its two readers (Ruling 13, pre-flight
/// I4): one take of the engine's read-and-clear peaks feeds both, and each
/// is cleared only by its own reader.
#[derive(Debug, Default)]
pub(crate) struct MeterHolds {
    /// What `Session::levels` hands the mixer window next.
    mixer: Option<crate::mixer::MixerLevels>,
    /// The surfaces' peaks per strip since their last view.
    surfaces: BTreeMap<u64, (f32, f32)>,
}

impl MeterHolds {
    /// Folds one take into both holds as maxima (the DSP peak into the
    /// mixer's only; the rest of the mixer's is the latest).
    pub(crate) fn fold(&mut self, take: crate::mixer::MixerLevels) {
        for m in &take.meters {
            let e = self.surfaces.entry(m.strip).or_insert((0.0, 0.0));
            e.0 = e.0.max(m.peak_l);
            e.1 = e.1.max(m.peak_r);
        }
        match self.mixer.as_mut() {
            None => self.mixer = Some(take),
            Some(held) => {
                for m in take.meters {
                    match held.meters.iter_mut().find(|h| h.strip == m.strip) {
                        Some(h) => {
                            h.peak_l = h.peak_l.max(m.peak_l);
                            h.peak_r = h.peak_r.max(m.peak_r);
                        }
                        None => held.meters.push(m),
                    }
                }
                held.dsp_peak = held.dsp_peak.max(take.dsp_peak);
                held.dsp_load = take.dsp_load;
                held.latency_frames = take.latency_frames;
                held.faulted = take.faulted;
            }
        }
    }

    /// The mixer's hold, cleared.
    pub(crate) fn take_mixer(&mut self) -> Option<crate::mixer::MixerLevels> {
        self.mixer.take()
    }

    /// The surfaces' hold, cleared.
    pub(crate) fn take_surfaces(&mut self) -> BTreeMap<u64, (f32, f32)> {
        std::mem::take(&mut self.surfaces)
    }
}

/// A session's surface state (in its `inner`): the events for the app,
/// the app's selection, the last skip or wind, and the existing-automation
/// cache of the heard values (Ruling 13).
#[derive(Debug, Default)]
pub(crate) struct SessionSurfaces {
    /// Events for `take_surface_events`.
    pub events: Vec<SurfaceEventRow>,
    /// The app's selected strips (`set_surface_selection`).
    pub selection: Vec<StripId>,
    /// Where the last skip or wind went, and when (surfaces clock).
    pub wind: Option<(Instant, i64)>,
    /// The existing automation, by (document version, sequence).
    pub existing: Option<(
        u64,
        core_model::ids::SeqId,
        std::sync::Arc<crate::automation::Existing>,
    )>,
    /// The host time of each surface or learn hand's last value, which
    /// its idle release is stamped at (Ruling 2).
    pub last_ns: BTreeMap<(TouchHolder, AutomationTarget), u64>,
}

// ---------------------------------------------------------------------
// The exports.

/// Reads surfaces.json (Ruling 9); problems go to `state.get surfaces`.
/// A file that does not parse, is newer or fails a check is renamed
/// `surfaces.json.bad` and VizDAW starts with no surfaces.
#[uniffi::export]
pub fn surfaces_load() -> Result<(), VizError> {
    ffi_guard(|| {
        let path = support_file();
        lock().load_from(&path, now());
        Ok(())
    })
}

/// The configured surfaces, by ID.
#[uniffi::export]
pub fn surface_list() -> Vec<SurfaceRow> {
    std::panic::catch_unwind(|| lock().units.iter().map(Unit::row).collect()).unwrap_or_default()
}

/// Adds a surface and saves (Ruling 4). `position` None: the next free one
/// in its group. The ID is the lowest free one (Ruling 4). Refused: a
/// second MCU, a fourth XT, a 17th surface, a taken position, a port
/// another surface uses, an empty or control-character or > 255-byte port.
/// `surfaces.json` is read first if it was not yet (Task 7 review M4).
#[uniffi::export]
pub fn surface_add(
    kind: SurfaceKindRow,
    input: String,
    output: String,
    position: Option<u8>,
) -> Result<u32, VizError> {
    ffi_guard(|| {
        let mut r = lock();
        r.ensure_loaded_from(&support_file(), now());
        let id = r.add(kind.into(), input, output, position, now())?;
        r.publish();
        r.save();
        Ok(id.get())
    })
}

/// Removes a surface and saves. Owes the unit's `release_all()` to the
/// session holding it.
#[uniffi::export]
pub fn surface_remove(id: u32) -> Result<(), VizError> {
    ffi_guard(|| {
        let mut r = lock();
        r.ensure_loaded_from(&support_file(), now());
        r.remove(id)?;
        r.publish();
        r.save();
        Ok(())
    })
}

/// The ports the transport sees (before any surface exists: the system's,
/// read without a client on macOS live builds).
#[uniffi::export]
pub fn surface_ports() -> SurfacePortsRow {
    std::panic::catch_unwind(|| {
        let r = lock();
        match r.ports.as_ref() {
            Some(p) => SurfacePortsRow {
                sources: p.sources(),
                destinations: p.destinations(),
            },
            None => system_ports(),
        }
    })
    .unwrap_or_default()
}

/// The system's ports, before the transport exists.
fn system_ports() -> SurfacePortsRow {
    #[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
    {
        if !crate::host::offline_mode() {
            return SurfacePortsRow {
                sources: platform_macos::midi::list_sources(),
                destinations: platform_macos::midi::list_destinations()
                    .into_iter()
                    .map(|d| d.name)
                    .collect(),
            };
        }
    }
    SurfacePortsRow::default()
}

/// `state.get surfaces` without a document (pre-flight M29): no feedback
/// rows.
#[uniffi::export]
pub fn surfaces_state() -> SurfaceStateRow {
    std::panic::catch_unwind(|| lock().state(false)).unwrap_or_default()
}

/// The learned commands, in the order they were learnt.
#[uniffi::export]
pub fn command_maps() -> Vec<CommandMapRow> {
    std::panic::catch_unwind(|| {
        lock()
            .commands
            .iter()
            .map(|c| CommandMapRow {
                source: (&c.source).into(),
                text: c.source.describe(),
                command: c.command.clone(),
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Removes every command map for `command` (pre-flight M14) and saves;
/// `Model` when there is none.
#[uniffi::export]
pub fn remove_command_map(command: String) -> Result<(), VizError> {
    ffi_guard(|| {
        let mut r = lock();
        r.ensure_loaded_from(&support_file(), now());
        let before = r.commands.len();
        r.commands.retain(|c| c.command != command);
        if r.commands.len() == before {
            return Err(crate::events::bad(&format!(
                "no MIDI control is learned for “{command}”"
            )));
        }
        r.reset_mapper_commands();
        r.publish();
        r.save();
        Ok(())
    })
}

/// Learns the next control for command `id` (Ruling 11): it needs a
/// focused session to complete; only key-bindable IDs (Ruling 8).
#[uniffi::export]
pub fn learn_command(id: String) -> Result<(), VizError> {
    ffi_guard(|| {
        bindable(&id).map_err(|e| crate::events::bad(&e))?;
        let mut r = lock();
        let Some(session) = r.focus else {
            return Err(crate::events::bad(NO_PROJECT));
        };
        r.ensure_loaded_from(&support_file(), now());
        r.start_learn(session, LearnFor::Command(id.clone()), id);
        Ok(())
    })
}

/// Cancels a waiting learn.
#[uniffi::export]
pub fn cancel_learn() {
    let _ = std::panic::catch_unwind(|| {
        let mut r = lock();
        if r.learn.take().is_some() {
            r.publish();
        }
    });
}

/// What a learn waits for and what the last one learnt.
#[uniffi::export]
pub fn learn_state() -> LearnStateRow {
    std::panic::catch_unwind(|| {
        let r = lock();
        LearnStateRow {
            waiting: r.learn.as_ref().map(|l| l.text.clone()),
            last: r.last.clone(),
        }
    })
    .unwrap_or_default()
}

// ---------------------------------------------------------------------
// Test hooks.

/// Forgets everything: no surfaces, fresh `FakePorts`, stale lists off, an
/// empty input queue, no focus, no learn. The file is left alone, and it
/// counts as loaded (tests call `surfaces_load` to read it).
#[cfg(feature = "test-hooks")]
pub fn debug_reset() {
    FROZEN_NS.store(u64::MAX, Ordering::Relaxed);
    let mut r = lock();
    *r = Registry::new();
    r.loaded = true;
    r.ports = Some(Box::new(ports::FakePorts::default()));
    r.publish();
    let mut q = lock_mutex(&INPUT);
    q.items.clear();
    q.dropped = 0;
    INPUT_BUSY.store(0, Ordering::Relaxed);
}

/// The fake transport (made if there is none).
#[cfg(feature = "test-hooks")]
fn with_fake<T>(f: impl FnOnce(&mut ports::FakePorts) -> T) -> T {
    let mut r = lock();
    let p = r
        .ports
        .get_or_insert_with(|| Box::new(ports::FakePorts::default()));
    f(p.as_fake())
}

/// Makes `ports` missing from the fake transport.
#[cfg(feature = "test-hooks")]
pub fn debug_set_missing(ports: Vec<String>) {
    with_fake(|f| f.set_missing(ports));
}

/// Makes `sources()` and `destinations()` answer the last `refresh()`.
#[cfg(feature = "test-hooks")]
pub fn debug_stale_sources(on: bool) {
    with_fake(|f| f.set_stale(on));
}

/// Drains what was sent to `port`.
#[cfg(feature = "test-hooks")]
pub fn debug_sent(port: &str) -> Vec<Vec<u8>> {
    with_fake(|f| f.take_sent(port))
}

/// Moves the surfaces clock on by `ms`.
#[cfg(feature = "test-hooks")]
pub fn debug_advance(ms: u64) {
    ADVANCED_MS.fetch_add(ms, Ordering::Relaxed);
}

/// Stops (`true`) or restarts the real-time part of the surfaces clock, so
/// only `debug_advance` moves it (a test that needs exact durations).
/// Restarting never moves the clock back.
#[cfg(feature = "test-hooks")]
pub fn debug_freeze_clock(on: bool) {
    let start = *START.get_or_init(Instant::now);
    let ns = if on {
        u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX - 1)
    } else {
        u64::MAX
    };
    FROZEN_NS.store(ns, Ordering::Relaxed);
}

/// Where `surfaces.json` lives in this process.
#[cfg(feature = "test-hooks")]
pub fn debug_support_file() -> PathBuf {
    support_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_model::automation::AutomationMode;
    use core_model::ids::StripId;
    use core_surface::{MixerView, StripView};

    fn view() -> MixerView {
        MixerView {
            strips: vec![StripView {
                id: StripId(1),
                name: "One".into(),
                fader: 0.5,
                pan: 0.5,
                send: None,
                mute: false,
                solo: false,
                arm: false,
                select: false,
                mode: AutomationMode::Read,
                meter: (0.0, 0.0),
            }],
            ..MixerView::default()
        }
    }

    /// A registry of its own with the fake transport, focused on session 7.
    fn registry() -> Registry {
        let mut r = Registry::new();
        r.ports = Some(Box::new(ports::FakePorts::default()));
        r.focus = Some(7);
        r
    }

    /// Unit 0 shows strip 1 and has its fader touched.
    fn touch(r: &mut Registry) {
        let t = now();
        let u = r.units.first_mut().unwrap();
        u.surface.feedback(&view(), 0, t);
        u.surface.input(&[0x90, 0x68, 0x7F], t);
        assert_eq!(u.surface.touched().len(), 1);
    }

    fn fake(r: &mut Registry) -> &mut ports::FakePorts {
        r.ports.as_mut().unwrap().as_fake()
    }

    #[test]
    fn a_lost_port_and_a_removal_owe_what_the_unit_holds() {
        let mut r = registry();
        let id = r
            .add(SurfaceKind::Mcu, "I".into(), "O".into(), None, now())
            .unwrap();
        assert!(r.units.first().unwrap().connected);
        touch(&mut r);
        fake(&mut r).set_missing(vec!["O".into()]);
        r.refresh_and_check(now());
        let fader = (
            TouchHolder::Surface(id),
            AutomationTarget::StripFader(StripId(1)),
        );
        assert_eq!(r.owed.get(&7), Some(&vec![fader]));
        assert!(!r.units.first().unwrap().connected);
        fake(&mut r).set_missing(Vec::new());
        r.refresh_and_check(now());
        assert!(r.units.first().unwrap().connected, "back");
        assert_eq!(
            fake(&mut r).take_sent("O").len(),
            2,
            "greeted on connect and on reconnect"
        );
        touch(&mut r);
        r.remove(id.get()).unwrap();
        assert_eq!(r.owed.get(&7), Some(&vec![fader, fader]));
        assert!(r.banks.is_empty());
    }

    #[test]
    fn a_failed_send_disconnects_at_once() {
        let mut r = registry();
        r.add(SurfaceKind::Hui, "I".into(), "O".into(), None, now())
            .unwrap();
        assert!(r.units.first().unwrap().connected);
        // Gone since the last check: the send itself finds out.
        fake(&mut r).set_missing(vec!["O".into()]);
        assert!(!r.send(0, &[vec![0x90, 0, 0]]));
        assert!(!r.units.first().unwrap().connected);
    }

    #[test]
    fn groups_bank_the_mcu_units_together_and_each_hui_alone() {
        let mut r = registry();
        r.add(SurfaceKind::McuXt, "A".into(), "A".into(), Some(3), now())
            .unwrap();
        r.add(SurfaceKind::Mcu, "B".into(), "B".into(), None, now())
            .unwrap();
        let h = r
            .add(SurfaceKind::Hui, "C".into(), "C".into(), None, now())
            .unwrap();
        assert_eq!(
            r.units.iter().map(|u| u.position).collect::<Vec<_>>(),
            [3, 0, 0]
        );
        assert_eq!(r.banks.get(&Group::Mcu).map(|b| b.units), Some(2));
        assert_eq!(r.banks.get(&Group::Hui(h)).map(|b| b.units), Some(1));
        assert!(
            r.add(SurfaceKind::Hui, "D".into(), "D".into(), Some(1), now())
                .is_err(),
            "a HUI is its own bank"
        );
    }

    /// Serializes the tests that set the process's claims.
    static CLAIMS_TESTS: Mutex<()> = Mutex::new(());

    /// The claims with surface input "S", for the queue and claim tests
    /// (they share the process's claims and queue; no other lib test does).
    fn claim_s(learning: bool, maps: Vec<LearnMap>) {
        // Callers hold `CLAIMS_TESTS`.
        let mut mapper = Mapper::default();
        let keys = maps
            .iter()
            .map(|m| {
                (
                    core_engine::input::port_key(&m.source.port),
                    m.source.port.clone(),
                )
            })
            .collect();
        mapper.set(maps, Vec::new());
        *CLAIMS.write().unwrap() = Claims {
            inputs: [core_engine::input::port_key("S")].into_iter().collect(),
            names: ["S".to_string()].into_iter().collect(),
            learning,
            keys,
            mapper,
        };
    }

    #[test]
    fn input_is_queued_only_for_surface_ports_and_bounded() {
        let _g = CLAIMS_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
        claim_s(false, Vec::new());
        let _ = take_input();
        let before = lock_mutex(&INPUT).dropped;
        assert!(!input("Keyboard", &[0x90, 60, 100], 0), "not a surface's");
        for _ in 0..INPUT_LIMIT + 3 {
            assert!(input("S", &[0x90, 0x68, 0x7F], 5));
        }
        let items = take_input();
        assert_eq!(items.len(), INPUT_LIMIT);
        assert_eq!(lock_mutex(&INPUT).dropped - before, 3);
        assert_eq!(
            items
                .first()
                .map(|i| (i.port.as_str(), i.host_ns, i.programmatic)),
            Some(("S", 5, false))
        );
        assert!(inject("S", &[0x90, 0x68, 0x7F]));
        assert!(
            take_input().first().is_some_and(|i| i.programmatic),
            "injected"
        );
        *CLAIMS.write().unwrap() = Claims::default();
    }

    /// Ruling 17: a surface port is taken whole; a learn takes channel
    /// messages of other ports; a map takes only its own messages, one at a
    /// time (a buffer of several is never taken as a whole, T5-1 carry).
    #[test]
    fn the_claims_take_surface_ports_learned_messages_and_a_waiting_learns() {
        let _g = CLAIMS_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
        use core_model::learn::{LearnKind, LearnSource};
        let key = core_engine::input::port_key;
        claim_s(false, Vec::new());
        assert!(
            takes(key("S"), &[0xF0, 0x00, 0xF7]),
            "anything from a surface port"
        );
        assert!(!takes(key("Keys"), &[0xB0, 7, 1]));
        claim_s(true, Vec::new());
        assert!(
            takes(key("Keys"), &[0xB0, 7, 1]),
            "a waiting learn takes a CC"
        );
        assert!(takes(key("Keys"), &[0x80, 60, 0]), "and a note-off");
        assert!(!takes(key("Keys"), &[0xF8]), "not a clock");
        let map = LearnMap {
            source: LearnSource {
                port: "Keys".into(),
                kind: LearnKind::Cc7 { channel: 0, cc: 7 },
            },
            target: AutomationTarget::StripFader(StripId(1)),
            mode: LearnMode::Absolute,
            pickup: true,
        };
        claim_s(false, vec![map]);
        assert!(takes(key("Keys"), &[0xB0, 7, 1]), "the learned CC");
        assert!(!takes(key("Keys"), &[0xB0, 8, 1]), "not another");
        assert!(!takes(key("Keys"), &[0x90, 60, 100]), "nor a note");
        assert!(
            !takes(key("Keys"), &[0xB0, 7, 1, 0xB0, 7, 2]),
            "a buffer of two is asked per message, never taken whole"
        );
        assert!(!takes(key("Pads"), &[0xB0, 7, 1]), "another port's CC 7");
        *CLAIMS.write().unwrap() = Claims::default();
    }

    /// Task 7 review M4: nothing is saved before the file was read, so a
    /// unit added early cannot replace the user's surfaces.
    #[test]
    fn the_file_is_read_before_anything_is_saved() {
        // Loading rewrites the process's claims.
        let _g = CLAIMS_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
        let dir =
            std::env::temp_dir().join(format!("vizdaw-surfaces-guard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(
            &path,
            r#"{"version": 1, "surfaces": [{"id": 0, "kind": "hui", "input": "a", "output": "b", "position": 0}]}"#,
        )
        .unwrap();
        let mut r = registry();
        assert!(!r.loaded);
        r.ensure_loaded_from(&path, now());
        assert_eq!(r.units.len(), 1, "the user's unit is read first");
        r.units.clear();
        r.ensure_loaded_from(&path, now());
        assert!(r.units.is_empty(), "read once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn problems_are_bounded_and_not_repeated() {
        let mut r = registry();
        for i in 0..40 {
            r.problem(format!("p{i}"));
            r.problem(format!("p{i}"));
        }
        assert_eq!(r.problems.len(), PROBLEM_LIMIT);
        assert_eq!(r.problems.last().map(String::as_str), Some("p39"));
    }

    #[test]
    fn a_file_is_checked_entry_by_entry() {
        let ok = r#"{"version": 1, "surfaces": [{"id": 3, "kind": "mcu_xt", "input": "a", "output": "b", "position": 2}], "commands": [{"source": {"port": "K", "kind": {"note": {"channel": 0, "note": 60}}}, "command": "transport.stop"}]}"#;
        let (units, commands) = parse_file(ok).unwrap();
        assert_eq!(
            units.first().map(|u| (u.id.get(), u.position)),
            Some((3, 2))
        );
        assert_eq!(commands.len(), 1);
        for (bad, why) in [
            ("[]", "no version"),
            (
                r#"{"version": 1, "surfaces": [], "extra": 1}"#,
                "an unknown field",
            ),
            (
                r#"{"version": 1, "surfaces": [{"id": 0, "kind": "mcu_xt", "input": "a", "output": "b", "position": 4}]}"#,
                "position 4",
            ),
            (
                r#"{"version": 1, "surfaces": [{"id": 0, "kind": "hui", "input": "a", "output": "b", "position": 0}, {"id": 0, "kind": "hui", "input": "c", "output": "d", "position": 0}]}"#,
                "a duplicate ID",
            ),
            (
                r#"{"version": 1, "surfaces": [{"id": 0, "kind": "tape", "input": "a", "output": "b", "position": 0}]}"#,
                "an unknown kind",
            ),
            (
                r#"{"version": 1, "surfaces": [{"id": 0, "kind": "hui", "input": "", "output": "b", "position": 0}]}"#,
                "an empty port",
            ),
            (
                r#"{"version": 1, "commands": [{"source": {"port": "K", "kind": {"note": {"channel": 0, "note": 60}}}, "command": "no.such"}]}"#,
                "an unknown command",
            ),
            (
                r#"{"version": 1, "commands": [{"source": {"port": "K", "kind": {"note": {"channel": 16, "note": 60}}}, "command": "transport.stop"}]}"#,
                "channel 16",
            ),
            (
                r#"{"version": 1, "commands": [{"source": {"port": "K", "kind": {"note": {"channel": 0, "note": 60}}}, "command": "automation.gesture"}]}"#,
                "a command no key can run (Task 7 review M6)",
            ),
        ] {
            assert!(parse_file(bad).is_err(), "{why}");
        }
    }
}
