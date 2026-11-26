//! Settings › Surfaces' model (Swift `SurfaceSettings`,
//! `M/SurfaceSettings.swift`): the core's surfaces and learned commands,
//! the Add Surface sheet's draft, and the problem lines. Everything goes
//! through [`CoreCalls`]: the app passes the live core, tests the fake.

use vizdaw_ffi::surfaces::{CommandMapRow, SurfaceKindRow, SurfacePortsRow, SurfaceRow};

use crate::AlertItem;
use crate::alerts::error_message;
use crate::session::CoreCalls;

/// The empty list (`S/SurfacesSettingsView.swift:20`).
pub const NO_SURFACES: &str = "No surfaces. Add an MCU-compatible, MCU XT-compatible or HUI-compatible unit, or a host-integration keyboard.";
/// No learned commands (`S/SurfacesSettingsView.swift:54`).
pub const NO_COMMAND_MAPS: &str = "None. Learn MIDI for a command in Settings › Keys.";
/// The Input picker with no ports (`S/SurfacesSettingsView.swift:116`).
pub const NO_SOURCES: &str = "No MIDI sources";
/// The Output picker with no ports (`S/SurfacesSettingsView.swift:120`).
pub const NO_DESTINATIONS: &str = "No MIDI destinations";
/// Under an XT's position (`S/SurfacesSettingsView.swift:136`).
pub const XT_HELP: &str =
    "Units are banked left to right by position; the MCU-compatible main unit takes a free one.";
/// The keyboard chosen but no DAW port found (`M/SurfaceSettings.swift:40`).
pub const NO_KONTROL: &str =
    "No host-integration keyboard “DAW” port is connected: pick its ports by hand.";
/// Under the keyboard's ports (`M/SurfaceSettings.swift:41`).
pub const KONTROL_HELP: &str = "Use the keyboard's “DAW” port for input and output. Its “Main” port stays a MIDI keyboard for your tracks.";

/// The alert titles (`M/SurfaceSettings.swift:103,115,123,133`).
pub const ADD_FAILED: &str = "Add Surface Failed";
pub const REMOVE_FAILED: &str = "Remove Surface Failed";
pub const REMOVE_LEARN_FAILED: &str = "Remove MIDI Learn Failed";

/// The Add Surface sheet's fields (`M/SurfaceSettings.swift:6-19`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDraft {
    pub kind: SurfaceKindRow,
    pub input: String,
    pub output: String,
    /// The MCU XT-compatible unit's place in the bank group (0–3).
    pub position: u8,
}

impl Default for SurfaceDraft {
    fn default() -> Self {
        SurfaceDraft {
            kind: SurfaceKindRow::Mcu,
            input: String::new(),
            output: String::new(),
            position: 1,
        }
    }
}

/// The four reads of a refresh (`M/SurfaceSettings.swift:52-61`), taken
/// together so the 2 Hz poll can run them off the UI thread.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurfaceRead {
    pub rows: Vec<SurfaceRow>,
    pub ports: SurfacePortsRow,
    pub command_maps: Vec<CommandMapRow>,
    /// `state.get surfaces`' problem lines.
    pub problems: Vec<String>,
}

impl SurfaceRead {
    /// Reads the core (the port list is a CoreMIDI listing on a live Mac).
    pub fn read(core: &dyn CoreCalls) -> SurfaceRead {
        SurfaceRead {
            rows: core.surface_list(),
            ports: core.surface_ports(),
            command_maps: core.command_maps(),
            problems: core.surfaces_state().problems,
        }
    }
}

/// The core's surfaces as Settings › Surfaces edits them.
#[derive(Debug, Default)]
pub struct SurfaceSettings {
    rows: Vec<SurfaceRow>,
    ports: SurfacePortsRow,
    command_maps: Vec<CommandMapRow>,
    problems: Vec<String>,
    alert: Option<AlertItem>,
    /// The open Add Surface sheet.
    draft: Option<SurfaceDraft>,
    /// Why the core refused the sheet's unit (shown in the sheet, which
    /// stays open).
    draft_problem: Option<String>,
    /// What the open sheet says about its type.
    draft_note: Option<String>,
}

/// Replaces `old` with `new` only when they differ; true if it did.
fn replace<T: PartialEq>(old: &mut T, new: T) -> bool {
    if *old == new {
        return false;
    }
    *old = new;
    true
}

impl SurfaceSettings {
    /// Reads nothing yet (`:43-47`): Settings refreshes when it opens, so
    /// making this at launch asks the core nothing before `surfaces_load`.
    pub fn new() -> Self {
        SurfaceSettings::default()
    }

    /// Reads the core now; true when something changed.
    pub fn refresh(&mut self, core: &dyn CoreCalls) -> bool {
        self.apply_read(SurfaceRead::read(core))
    }

    /// Takes a read (the poll's, made off the UI thread); unchanged values
    /// are not replaced. True when something changed.
    pub fn apply_read(&mut self, read: SurfaceRead) -> bool {
        let rows = replace(&mut self.rows, read.rows);
        let ports = replace(&mut self.ports, read.ports);
        let maps = replace(&mut self.command_maps, read.command_maps);
        let problems = replace(&mut self.problems, read.problems);
        rows || ports || maps || problems
    }

    pub fn rows(&self) -> &[SurfaceRow] {
        &self.rows
    }

    pub fn ports(&self) -> &SurfacePortsRow {
        &self.ports
    }

    pub fn command_maps(&self) -> &[CommandMapRow] {
        &self.command_maps
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The last line about surface `id`, without "surface {id}: "
    /// (`:66-70`); `None` while it answers (a line left over must not sit
    /// under "Connected").
    pub fn problem_for(&self, id: u32) -> Option<&str> {
        if self
            .rows
            .iter()
            .any(|r| r.id == id && r.connected && r.online)
        {
            return None;
        }
        let prefix = format!("surface {id}: ");
        self.problems
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix(prefix.as_str()))
    }

    /// The lines that name no listed row (`:73-76`): an unknown kind kept,
    /// the file's.
    pub fn other_problems(&self) -> Vec<&str> {
        let prefixes: Vec<String> = self
            .rows
            .iter()
            .map(|r| format!("surface {}: ", r.id))
            .collect();
        self.problems
            .iter()
            .filter(|l| !prefixes.iter().any(|p| l.starts_with(p.as_str())))
            .map(String::as_str)
            .collect()
    }

    pub fn draft(&self) -> Option<&SurfaceDraft> {
        self.draft.as_ref()
    }

    pub fn draft_problem(&self) -> Option<&str> {
        self.draft_problem.as_deref()
    }

    pub fn draft_note(&self) -> Option<&str> {
        self.draft_note.as_deref()
    }

    /// Opens the Add sheet with the first ports picked (`:139-144`).
    pub fn begin_add(&mut self, core: &dyn CoreCalls) {
        self.refresh(core);
        self.draft_problem = None;
        self.draft_note = None;
        self.draft = Some(SurfaceDraft {
            input: self.ports.sources.first().cloned().unwrap_or_default(),
            output: self.ports.destinations.first().cloned().unwrap_or_default(),
            ..SurfaceDraft::default()
        });
    }

    /// Sets the sheet's type (`:82-97`); the keyboard takes its DAW port
    /// where one is found, each side on its own. The ports are read again
    /// first, so one that appeared since the sheet opened is in the
    /// pickers. Without an open sheet, nothing.
    pub fn set_draft_kind(&mut self, kind: SurfaceKindRow, core: &dyn CoreCalls) {
        if self.draft.is_none() {
            return;
        }
        self.refresh(core);
        let Some(d) = self.draft.as_mut() else {
            return;
        };
        d.kind = kind;
        self.draft_note = None;
        if kind == SurfaceKindRow::KontrolMk3 {
            let found = core.kontrol_ports();
            if let Some(i) = found.sources.first() {
                d.input = i.clone();
            }
            if let Some(o) = found.destinations.first() {
                d.output = o.clone();
            }
            if found.sources.is_empty() && found.destinations.is_empty() {
                self.draft_note = Some(NO_KONTROL.to_owned());
            }
        }
    }

    pub fn set_draft_input(&mut self, input: String) {
        if let Some(d) = self.draft.as_mut() {
            d.input = input;
        }
    }

    pub fn set_draft_output(&mut self, output: String) {
        if let Some(d) = self.draft.as_mut() {
            d.output = output;
        }
    }

    /// The XT's position, 0–3 (the sheet's stepper range).
    pub fn set_draft_position(&mut self, position: u8) {
        if let Some(d) = self.draft.as_mut() {
            d.position = position.min(3);
        }
    }

    /// Adds what the sheet says and closes it (`:148-157`); a refusal
    /// keeps it open with the reason in it. Only an XT passes a position.
    pub fn commit_add(&mut self, core: &dyn CoreCalls) {
        let Some(d) = self.draft.clone() else {
            return;
        };
        let position = (d.kind == SurfaceKindRow::McuXt).then_some(d.position);
        match self.try_add(d.kind, &d.input, &d.output, position, core) {
            Some(problem) => self.draft_problem = Some(problem),
            None => {
                self.draft_problem = None;
                self.draft_note = None;
                self.draft = None;
            }
        }
    }

    /// Closes the sheet without adding.
    pub fn cancel_add(&mut self) {
        self.draft_problem = None;
        self.draft_note = None;
        self.draft = None;
    }

    /// Adds a unit; a refusal is an alert. A HUI-compatible unit and the
    /// keyboard are banks of their own, so they are added without a
    /// position.
    pub fn add(
        &mut self,
        kind: SurfaceKindRow,
        input: &str,
        output: &str,
        position: Option<u8>,
        core: &dyn CoreCalls,
    ) {
        if let Some(problem) = self.try_add(kind, input, output, position, core) {
            self.alert = Some(AlertItem::new(ADD_FAILED, problem));
        }
    }

    /// The core's refusal, if any; refreshes either way.
    fn try_add(
        &mut self,
        kind: SurfaceKindRow,
        input: &str,
        output: &str,
        position: Option<u8>,
        core: &dyn CoreCalls,
    ) -> Option<String> {
        let own_bank = matches!(kind, SurfaceKindRow::Hui | SurfaceKindRow::KontrolMk3);
        let position = if own_bank { None } else { position };
        let problem = core
            .surface_add(kind, input, output, position)
            .err()
            .map(|e| error_message(&e));
        self.refresh(core);
        problem
    }

    pub fn remove(&mut self, id: u32, core: &dyn CoreCalls) {
        if let Err(e) = core.surface_remove(id) {
            self.alert = Some(AlertItem::from_error(&e, REMOVE_FAILED));
        }
        self.refresh(core);
    }

    /// Removes the MIDI controls learned to command `command`.
    pub fn remove_command_map(&mut self, command: &str, core: &dyn CoreCalls) {
        if let Err(e) = core.remove_command_map(command) {
            self.alert = Some(AlertItem::from_error(&e, REMOVE_LEARN_FAILED));
        }
        self.refresh(core);
    }

    /// The alert to show, once.
    pub fn take_alert(&mut self) -> Option<AlertItem> {
        self.alert.take()
    }
}

/// "MCU-compatible", "MCU XT-compatible", "HUI-compatible",
/// "Host-integration keyboard (DAW mode)" (`M/SurfaceSettings.swift:166-175`).
pub fn kind_title(kind: SurfaceKindRow) -> &'static str {
    match kind {
        SurfaceKindRow::Mcu => "MCU-compatible",
        SurfaceKindRow::McuXt => "MCU XT-compatible",
        SurfaceKindRow::Hui => "HUI-compatible",
        SurfaceKindRow::KontrolMk3 => "Host-integration keyboard (DAW mode)",
    }
}

/// "Connected", "Connected · not answering", "Disconnected"
/// (`M/SurfaceSettings.swift:178-181`).
pub fn status(row: &SurfaceRow) -> &'static str {
    match (row.connected, row.online) {
        (false, _) => "Disconnected",
        (true, true) => "Connected",
        (true, false) => "Connected · not answering",
    }
}

/// A row's title; an XT's says its position
/// (`S/SurfacesSettingsView.swift:93-96`).
pub fn row_title(row: &SurfaceRow) -> String {
    let t = kind_title(row.kind);
    if row.kind == SurfaceKindRow::McuXt {
        format!("{t} · position {}", row.position)
    } else {
        t.to_owned()
    }
}
