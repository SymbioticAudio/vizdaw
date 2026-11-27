//! MIDI learn for one document (Swift `M/LearnModel.swift`, plan 5f-1
//! Task 11; slice 9d Task 4): the "MIDI Learn" items, Settings › Keys
//! "Learn MIDI", and the MIDI Learn Maps sheet.
//!
//! A learn waits in the core until a control moves, it is cancelled or
//! another learn starts; while it waits the banner shows. The project's
//! maps are this document's (`DocSession::learn_maps`); the command learn
//! and its cancel are process-wide, so they go through `CoreCalls`.

use std::sync::Arc;

use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::mixer::{MixerSnapshot, StripRow};
use vizdaw_ffi::surfaces::{LearnMapRow, LearnModeRow};

use crate::alerts::{AlertItem, error_message};
use crate::document::DocumentModel;
use crate::mixer::{MixerModel, VcaStripLabels};
use crate::session::{CoreCalls, DocSession};

/// The banner while a learn waits (`M/LearnModel.swift:16`).
pub const WAITING_TEXT: &str = "Waiting for MIDI\u{2026} move a control (Esc cancels)";
/// The title of every refusal (`M/LearnModel.swift:36`).
pub const FAILED_TITLE: &str = "MIDI Learn Failed";
/// The pick-up toggle's help (`M/LearnModel.swift:164`).
pub const PICKUP_HELP: &str = "The control takes over only once it reaches the current value";
/// The MIDI Learn Maps sheet with no maps (`S/LearnMapsSheet.swift:18`).
pub const NO_MAPS: &str =
    "No MIDI controls are learned in this project. Choose MIDI Learn in a control's menu.";
/// A map whose target is gone (`M/LearnModel.swift:169`).
pub const MISSING: &str = "(missing)";
/// Every mode, in the picker's order (`M/LearnModel.swift:161`).
pub const MODES: [LearnModeRow; 3] = [
    LearnModeRow::Absolute,
    LearnModeRow::Relative2sComplement,
    LearnModeRow::RelativeBinOffset,
];

/// The banner's text (`M/LearnModel.swift:20-25`): what the learn waits
/// for when known, and the project when given (two or more are open),
/// else [`WAITING_TEXT`].
pub fn waiting(project: Option<&str>, what: Option<&str>) -> String {
    if project.is_none() && what.is_none() {
        return WAITING_TEXT.to_owned();
    }
    let for_text = what.map(|w| format!(" for {w}")).unwrap_or_default();
    let in_text = project
        .map(|p| format!(" in \u{201C}{p}\u{201D}"))
        .unwrap_or_default();
    format!("Waiting for MIDI{for_text}{in_text}\u{2026} move a control (Esc cancels)")
}

/// "Absolute", "Relative (2's complement)", "Relative (binary offset)"
/// (`M/LearnModel.swift:152-158`).
pub fn mode_title(m: LearnModeRow) -> &'static str {
    match m {
        LearnModeRow::Absolute => "Absolute",
        LearnModeRow::Relative2sComplement => "Relative (2's complement)",
        LearnModeRow::RelativeBinOffset => "Relative (binary offset)",
    }
}

/// What `t` is called, from the document's mixer (`M/StripLane.swift:155-173`):
/// "Fader · Synth", "Send 1 to Bus 1 · Synth", "Time · Echo"; a strip,
/// slot or parameter not found is named by its ID.
pub fn target_title(t: &AutomationTargetRow, doc: &DocumentModel) -> String {
    let session = doc.session();
    let mixer = session.mixer().ok();
    let strips: &[StripRow] = mixer.as_ref().map_or(&[], |m: &MixerSnapshot| &m.strips);
    let name = |id: u64| strips.iter().find(|s| s.id == id).map(|s| s.name.clone());
    let strip = |id: u64| name(id).unwrap_or_else(|| format!("Strip {id}"));
    match *t {
        AutomationTargetRow::StripFader { strip: s } => format!("Fader · {}", strip(s)),
        AutomationTargetRow::StripPan { strip: s } => format!("Pan · {}", strip(s)),
        AutomationTargetRow::StripMute { strip: s } => format!("Mute · {}", strip(s)),
        AutomationTargetRow::StripSolo { strip: s } => format!("Solo · {}", strip(s)),
        AutomationTargetRow::SendLevel { strip: s, send } => {
            let to = strips
                .iter()
                .find(|r| r.id == s)
                .and_then(|r| r.sends.get(usize::from(send)))
                .map(|x| {
                    let bus = name(x.target).unwrap_or_else(|| format!("Bus {}", x.target));
                    format!(" to {bus}")
                })
                .unwrap_or_default();
            format!("Send {}{to} · {}", u32::from(send) + 1, strip(s))
        }
        AutomationTargetRow::PluginParam { slot, param } => {
            let plugin = strips
                .iter()
                .flat_map(MixerModel::slots)
                .find(|x| x.id == slot)
                .map(|x| x.plugin.name.clone());
            let p = plugin
                .as_ref()
                .and_then(|_| session.params(slot).ok())
                .and_then(|ps| {
                    ps.into_iter()
                        .find(|p| p.id == param && p.automatable && !p.hidden)
                })
                .map_or_else(|| format!("Parameter {param}"), |p| p.name);
            let plugin = plugin.unwrap_or_else(|| format!("Plugin {slot}"));
            format!("{p} · {plugin}")
        }
        AutomationTargetRow::VelocityScale { dest } => {
            let d = session
                .destinations()
                .ok()
                .and_then(|ds| ds.into_iter().find(|d| d.id == dest))
                .map_or_else(|| format!("Destination {dest}"), |d| d.name);
            format!("Velocity Scale · {d}")
        }
        AutomationTargetRow::VcaFader { vca } => {
            let v = mixer
                .as_ref()
                .and_then(|m| m.vcas.iter().find(|v| v.id == vca))
                .map_or_else(|| vca.to_string(), |v| v.name.clone());
            format!("{} Fader", VcaStripLabels::title(&v))
        }
    }
}

/// One document's learn state (`DocumentModel.learn`).
pub struct LearnModel {
    session: Arc<dyn DocSession>,
    /// A learn this document started waits (Swift `banner != nil`).
    learning: bool,
    /// What it waits for, as the last read of `surface_state` said.
    waiting_for: Option<String>,
    maps: Vec<LearnMapRow>,
    /// The MIDI Learn Maps sheet is open (it polls while open).
    maps_open: bool,
    /// The refusal the open sheet shows (Ruling 5).
    problem: Option<String>,
}

impl LearnModel {
    pub fn new(session: Arc<dyn DocSession>) -> Self {
        LearnModel {
            session,
            learning: false,
            waiting_for: None,
            maps: vec![],
            maps_open: false,
            problem: None,
        }
    }

    pub fn is_learning(&self) -> bool {
        self.learning
    }

    /// The idle poll runs only while this holds (`M/LearnModel.swift:76`).
    pub fn is_active(&self) -> bool {
        self.learning || self.maps_open
    }

    /// The banner for a window, naming `project` (`None` with one project
    /// open) and what the learn waits for; `None` when not learning.
    pub fn banner_text(&self, project: Option<&str>) -> Option<String> {
        self.learning
            .then(|| waiting(project, self.waiting_for.as_deref()))
    }

    pub fn maps(&self) -> &[LearnMapRow] {
        &self.maps
    }

    pub fn maps_open(&self) -> bool {
        self.maps_open
    }

    /// Opens the MIDI Learn Maps sheet, reading the maps again
    /// (`M/LearnModel.swift:50-52`).
    pub fn open_maps(&mut self) {
        if !self.maps_open {
            self.maps_open = true;
            self.problem = None;
            self.refresh_maps();
        }
    }

    pub fn close_maps(&mut self) {
        self.maps_open = false;
        self.problem = None;
    }

    /// The refusal shown in the open maps sheet (Ruling 5).
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// What `m` drives, as the sheet shows it: its target's name, or
    /// [`MISSING`] once the target is gone.
    pub fn target_text(m: &LearnMapRow, doc: &DocumentModel) -> String {
        if m.missing {
            MISSING.to_owned()
        } else {
            target_title(&m.target, doc)
        }
    }

    /// Rereads the project's maps; true when they changed.
    fn refresh_maps(&mut self) -> bool {
        match self.session.learn_maps() {
            Ok(m) if m != self.maps => {
                self.maps = m;
                true
            }
            _ => false,
        }
    }

    /// A learn started (or was refused).
    fn started(&mut self, result: Result<(), VizError>) -> Option<AlertItem> {
        match result {
            Ok(()) => {
                self.learning = true;
                self.waiting_for = self.read_waiting();
                None
            }
            Err(e) => {
                self.learning = false;
                self.waiting_for = None;
                self.refusal(&e)
            }
        }
    }

    /// A refusal: the open sheet's line, else an alert for the document.
    fn refusal(&mut self, e: &VizError) -> Option<AlertItem> {
        if self.maps_open {
            self.problem = Some(error_message(e));
            None
        } else {
            Some(AlertItem::from_error(e, FAILED_TITLE))
        }
    }

    fn read_waiting(&self) -> Option<String> {
        self.session.surface_state().ok().and_then(|s| s.learning)
    }

    fn cancelled(&mut self) {
        self.learning = false;
        self.waiting_for = None;
    }

    /// The idle poll, while active (`M/LearnModel.swift:113-124`): the
    /// banner goes once the core no longer waits, and the maps follow the
    /// project (a learn landed, undo, redo). True when something changed.
    fn poll(&mut self) -> bool {
        let mut changed = false;
        if self.learning {
            match self.session.surface_state() {
                Ok(s) if s.learning.is_none() => {
                    self.cancelled();
                    changed = true;
                }
                Ok(s) if s.learning != self.waiting_for => {
                    self.waiting_for = s.learning;
                    changed = true;
                }
                _ => {}
            }
        }
        self.refresh_maps() || changed
    }
}

/// The learn calls of a document (`M/LearnModel.swift:80-149`).
impl DocumentModel {
    /// Learns the next control for `t` (a fader, pan, mute, solo, send
    /// or plug-in parameter of this project).
    pub fn learn_target(&mut self, t: AutomationTargetRow) {
        let r = self.session.learn_target(t);
        if let Some(a) = self.learn.started(r) {
            self.alert = Some(a);
        }
    }

    /// Learns the next control for command `id`. The learn completes in
    /// this document's session, which takes the surfaces' focus first.
    pub fn learn_command(&mut self, id: &str, core: &dyn CoreCalls) {
        let r = self
            .session
            .focus_surfaces()
            .and_then(|()| core.learn_command(id));
        if let Some(a) = self.learn.started(r) {
            self.alert = Some(a);
        }
    }

    /// Esc: the waiting learn ends with nothing learned.
    pub fn cancel_learn(&mut self, core: &dyn CoreCalls) {
        core.cancel_learn();
        self.learn.cancelled();
    }

    pub fn set_learn_mode(&mut self, m: &LearnMapRow, mode: LearnModeRow) {
        let source = m.source.clone();
        self.learn_edit(|s| s.set_learn_mode(source, mode));
    }

    /// Ruling 18; does nothing for a map whose kind or mode does not pick
    /// up.
    pub fn set_learn_pickup(&mut self, m: &LearnMapRow, on: bool) {
        if !m.picks_up {
            return;
        }
        let source = m.source.clone();
        self.learn_edit(|s| s.set_learn_pickup(source, on));
    }

    pub fn remove_learn(&mut self, m: &LearnMapRow) {
        let source = m.source.clone();
        self.learn_edit(|s| s.remove_learn(source));
    }

    /// One undo step ("MIDI Learn", "Remove MIDI Learn"); a refusal is an
    /// alert, or the open sheet's line.
    fn learn_edit(
        &mut self,
        body: impl FnOnce(&dyn DocSession) -> Result<vizdaw_ffi::snapshot::DocSnapshot, VizError>,
    ) {
        let session = self.session.clone();
        match body(session.as_ref()) {
            Ok(s) => {
                self.learn.problem = None;
                self.apply(s);
            }
            Err(e) => {
                if let Some(a) = self.learn.refusal(&e) {
                    self.alert = Some(a);
                }
            }
        }
        self.learn.refresh_maps();
    }

    /// The learn's idle poll, while it is active.
    pub(crate) fn learn_tick(&mut self) -> bool {
        self.learn.is_active() && self.learn.poll()
    }
}
