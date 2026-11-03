//! Control surfaces in the core (plan 5f-1 Task 7; spec §8.1, §8.3): the
//! process-wide registry of units, their ports, `surfaces.json`, adding
//! and removing units, and their connection.
//!
//! - The registry ([`Registry`], behind [`lock`]) holds the units, the bank
//!   groups (Ruling 4), the focused session (Ruling 11), the releases each
//!   session is owed, the port transport (Ruling 10) and the 2 s check.
//! - The input queue ([`input`]) has its own lock and never waits: CoreMIDI's
//!   thread pushes into it.
//! - Lock order (Global Constraints): `Session.inner` → the registry → the
//!   input queue; the registry also rewrites the listened-ports set under
//!   its own lock.
//!
//! Task 8 adds the service (input → actions → gestures, feedback), the
//! claims and MIDI learn.

pub(crate) mod ports;

use crate::automation::{SurfaceId, TouchHolder};
use crate::error::VizError;
use crate::guard::{ffi_guard, lock as lock_mutex};
use core_model::automation::AutomationTarget;
use core_model::learn::{LearnMode, LearnSource};
use core_surface::{Bank, Surface, SurfaceKind, new_surface};
use ports::Ports;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError, RwLock, TryLockError};
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

/// "VizDAW drives one MCU-compatible main unit and up to three extenders".
const GROUP_LIMIT: &str = "VizDAW drives one MCU-compatible main unit and up to three extenders";

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
    /// Malformed or unknown messages so far (spec §12.2).
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
            ignored: self.surface.ignored(),
        }
    }

    fn group(&self) -> Group {
        match self.kind {
            SurfaceKind::Mcu | SurfaceKind::McuXt => Group::Mcu,
            SurfaceKind::Hui => Group::Hui(self.id),
        }
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

/// The process-wide surfaces state (`surfaces::lock()`).
pub(crate) struct Registry {
    /// The units, by ID.
    units: Vec<Unit>,
    /// The bank groups and their offsets (Ruling 4).
    #[allow(dead_code)] // Task 8's service banks and reads them.
    banks: BTreeMap<Group, Bank>,
    /// The session the surfaces drive (Ruling 11).
    focus: Option<u64>,
    /// Releases each session is owed (Ruling 11); Task 8 applies them.
    owed: BTreeMap<u64, Vec<(TouchHolder, AutomationTarget)>>,
    /// The transport, made on the first surface (Ruling 10).
    ports: Option<Box<dyn Ports>>,
    /// When the ports were last refreshed.
    checked: Option<Instant>,
    /// The learned commands of `surfaces.json` (Task 8 runs them).
    commands: Vec<CommandEntry>,
    /// Problem lines for `state.get surfaces` (Task 8 shows them).
    problems: Vec<String>,
}

impl Registry {
    const fn new() -> Registry {
        Registry {
            units: Vec::new(),
            banks: BTreeMap::new(),
            focus: None,
            owed: BTreeMap::new(),
            ports: None,
            checked: None,
            commands: Vec::new(),
            problems: Vec::new(),
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
        self.owe(id, held);
    }

    /// Records the releases of `targets` held by surface `id` for the
    /// focused session (none focused: nobody holds them).
    fn owe(&mut self, id: SurfaceId, targets: Vec<AutomationTarget>) {
        if targets.is_empty() {
            return;
        }
        if let Some(f) = self.focus {
            self.owed
                .entry(f)
                .or_default()
                .extend(targets.into_iter().map(|t| (TouchHolder::Surface(id), t)));
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

    /// The lowest free ID (Ruling 4).
    fn free_id(&self) -> Option<SurfaceId> {
        (0..u32::from(SurfaceId::COUNT))
            .filter_map(SurfaceId::new)
            .find(|id| self.units.iter().all(|u| u.id != *id))
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
        self.owe(u.id, held);
        self.rebank();
        Ok(())
    }

    /// Forgets every unit (each owes what it holds).
    fn clear(&mut self) {
        for mut u in std::mem::take(&mut self.units) {
            let held = u.surface.release_all();
            self.owe(u.id, held);
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

    /// Writes `surfaces.json` (temp file, then rename); a failure becomes
    /// a problem line.
    fn save(&mut self) {
        if let Err(e) = write_file(&support_file(), &self.file()) {
            self.problems
                .push(format!("Could not save the surfaces ({FILE}): {e}"));
        }
    }

    /// Rewrites the ports [`input`] listens to.
    fn publish(&self) {
        let inputs: BTreeSet<String> = self.units.iter().map(|u| u.input.clone()).collect();
        *LISTENS.write().unwrap_or_else(PoisonError::into_inner) = inputs;
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

/// A learned command's checks on load: its source's (Ruling 6) and a
/// registry ID that exists (Task 8's `learn_command` adds the bindable
/// rule).
fn check_command(c: &CommandEntry) -> Result<(), String> {
    c.source.check().map_err(|e| e.to_string())?;
    if core_control::registry::find(&c.command).is_none() {
        return Err(format!("unknown command “{}”", c.command));
    }
    Ok(())
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

/// Writes `file` to `path` atomically: a temp file beside it, then a rename.
fn write_file(path: &Path, file: &SurfacesFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(file).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// `support_dir()/surfaces.json`.
fn support_file() -> PathBuf {
    crate::recovery::support_dir().join(FILE)
}

/// `surfaces.json.bad`, beside the file.
fn bad_file(path: &Path) -> PathBuf {
    path.with_extension("json.bad")
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

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

/// The surfaces clock: real time plus what `debug_advance` added.
pub(crate) fn now() -> Instant {
    let start = *START.get_or_init(Instant::now);
    let ahead = Duration::from_millis(ADVANCED_MS.load(Ordering::Relaxed));
    start
        .checked_add(start.elapsed())
        .and_then(|t| t.checked_add(ahead))
        .unwrap_or_else(Instant::now)
}

// ---------------------------------------------------------------------
// The input queue (Ruling 10).

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

/// Pushes dropped because the queue was being taken (`try_lock`).
static INPUT_BUSY: AtomicU64 = AtomicU64::new(0);

/// The input ports of the configured surfaces (`try_read` in [`input`]).
static LISTENS: RwLock<BTreeSet<String>> = RwLock::new(BTreeSet::new());

/// Raw input from a port (CoreMIDI's thread, `midi.inject`, tests): queued
/// with its host time (0: now), never waits. True when a surface listens to
/// `port` (Task 8 adds a waiting learn and the maps). While the listened
/// set is being rewritten the message is passed on (false), as Ruling 17's
/// claims do.
#[cfg_attr(
    any(feature = "test-hooks", not(target_os = "macos")),
    allow(dead_code) // Task 8's `midi.inject` and `surface.inject` call it too.
)]
pub(crate) fn input(port: &str, bytes: &[u8], host_ns: u64) -> bool {
    let listens = match LISTENS.try_read() {
        Ok(l) => l.contains(port),
        Err(TryLockError::Poisoned(p)) => p.into_inner().contains(port),
        Err(TryLockError::WouldBlock) => return false,
    };
    if !listens {
        return false;
    }
    let at = now();
    let mut q = match INPUT.try_lock() {
        Ok(q) => q,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => {
            INPUT_BUSY.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    };
    if q.items.len() >= INPUT_LIMIT {
        q.dropped = q.dropped.saturating_add(1);
    } else {
        q.items.push_back(Input {
            port: port.to_string(),
            host_ns,
            at,
            bytes: bytes.to_vec(),
        });
    }
    true
}

/// Takes the whole queue, and how many messages were dropped so far.
#[allow(dead_code)] // Task 8's service drains it.
pub(crate) fn take_input() -> (Vec<Input>, u64) {
    let mut q = lock_mutex(&INPUT);
    let items = std::mem::take(&mut q.items);
    let dropped = q.dropped.saturating_add(INPUT_BUSY.load(Ordering::Relaxed));
    (items.into(), dropped)
}

// ---------------------------------------------------------------------
// The session's side (`Session::focus_surfaces`, `idle`).

/// Makes session `id` the one the surfaces drive (Ruling 11; Task 8 adds
/// the releases and refresh a focus change owes).
pub(crate) fn focus(id: u64) {
    lock().focus = Some(id);
}

/// Session `id`'s `idle`: when it is focused, the 2 s port check (Task 8
/// adds input, actions and feedback).
pub(crate) fn service(id: u64) {
    let mut r = lock();
    if r.focus == Some(id) {
        r.service(now());
    }
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
        let loaded = match std::fs::read_to_string(&path) {
            Ok(text) => Some(parse_file(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(e.to_string())),
        };
        let mut r = lock();
        r.clear();
        r.commands.clear();
        match loaded {
            None => {}
            Some(Ok((units, commands))) => {
                r.units = units;
                r.commands = commands;
            }
            Some(Err(why)) => {
                let bad = bad_file(&path);
                let kept = match std::fs::rename(&path, &bad) {
                    Ok(()) => format!("kept aside as {FILE}.bad"),
                    Err(e) => format!("could not be kept aside ({e})"),
                };
                r.problems.push(format!(
                    "{FILE} could not be read and was {kept}; VizDAW starts with no surfaces: {why}"
                ));
            }
        }
        r.rebank();
        r.publish();
        if !r.units.is_empty() {
            r.refresh_and_check(now());
        }
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
#[uniffi::export]
pub fn surface_add(
    kind: SurfaceKindRow,
    input: String,
    output: String,
    position: Option<u8>,
) -> Result<u32, VizError> {
    ffi_guard(|| {
        let mut r = lock();
        let id = r.add(kind.into(), input, output, position, now())?;
        r.publish();
        r.save();
        Ok(id.get())
    })
}

/// Removes a surface and saves. Owes the unit's `release_all()` to the
/// session holding it (Task 8 applies it).
#[uniffi::export]
pub fn surface_remove(id: u32) -> Result<(), VizError> {
    ffi_guard(|| {
        let mut r = lock();
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

// ---------------------------------------------------------------------
// Test hooks.

/// Forgets everything: no surfaces, fresh `FakePorts`, stale lists off, an
/// empty input queue. The file is left alone.
#[cfg(feature = "test-hooks")]
pub fn debug_reset() {
    let mut r = lock();
    *r = Registry::new();
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

    #[test]
    fn input_is_queued_only_for_surface_ports_and_bounded() {
        *LISTENS.write().unwrap() = ["S".to_string()].into_iter().collect();
        let _ = take_input();
        assert!(!input("Keyboard", &[0x90, 60, 100], 0), "not a surface's");
        for _ in 0..INPUT_LIMIT + 3 {
            assert!(input("S", &[0x90, 0x68, 0x7F], 5));
        }
        let (items, dropped) = take_input();
        assert_eq!(items.len(), INPUT_LIMIT);
        assert_eq!(dropped, 3);
        assert_eq!(
            items.first().map(|i| (i.port.as_str(), i.host_ns)),
            Some(("S", 5))
        );
        LISTENS.write().unwrap().clear();
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
        ] {
            assert!(parse_file(bad).is_err(), "{why}");
        }
    }
}
