//! A strip's plugin slots apart from their editor windows (Swift
//! `SlotView`, `BuiltinSynthSlotView`, `EqSlotView`, `StripInserts`,
//! `MixerModel.removeInsert/moveInsert/toggleBypass/setSidechain/load/
//! clearEq`): the instrument (or the Built-in Synth), the EQ and the
//! inserts, each with its dot (faulted, live, bypassed, loading or a
//! placeholder), its menu and the edits that need no plugin window or
//! catalogue. Each edit is one session call; the core names its undo
//! step (`Bypass Plugin`, `Remove Plugin`, `Move Plugin`, `Set Sidechain`,
//! `Remove EQ`), a refusal is the Swift alert title.
//!
//! "Open" (and a slot's double-click, the EQ slot's click) opens the
//! slot's plug-in window, and the Built-in Synth slot's double-click its
//! sound window; the GPUI crate makes those windows. Picking a plug-in
//! ("+", "EQ ＋", "Replace…" and the instrument menu) opens the browser or
//! the instrument menu of `mixer/browser.rs`, whose picks are the session's
//! add-insert, set-EQ and instrument calls.

use vizdaw_ffi::mixer::{StripKindRow, StripRow};
use vizdaw_ffi::snapshot::{SlotRow, SlotStatus};

use super::model::MixerModel;
use crate::document::DocumentModel;

pub const BYPASS_FAILED: &str = "Bypass Failed";
pub const REMOVE_PLUGIN_FAILED: &str = "Remove Plugin Failed";
pub const MOVE_PLUGIN_FAILED: &str = "Move Plugin Failed";
pub const SIDECHAIN_FAILED: &str = "Sidechain Failed";
pub const LOAD_FAILED: &str = "Load Failed";
pub const CLEAR_EQ_FAILED: &str = "Clear EQ Failed";

/// What a slot's dot shows (Swift `SlotView.dot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    /// The engine bypassed it for misbehaving (NaN output, errors).
    Faulted,
    /// Running.
    Live,
    /// Running, bypassed.
    Bypassed,
    Loading,
    /// It failed to load, is disabled, missing or blocklisted, or safe
    /// mode keeps it off; audio passes through.
    Placeholder,
}

/// Where a slot sits on its strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPlace {
    /// A plugin instrument's own slot.
    Instrument,
    Eq,
    /// Insert `n` (from 0).
    Insert(usize),
}

/// What a strip's instrument row shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstrumentRow<'a> {
    /// A plugin instrument's own slot.
    Plugin(&'a SlotRow),
    /// The shared Built-in Synth strip's slot.
    BuiltinSynth,
}

/// One item of a sidechain menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidechainChoice {
    /// "None", or the strip's name ("✓ " in front of the current one).
    pub title: String,
    /// `None`: no sidechain.
    pub source: Option<u64>,
}

/// One item of a slot's menu, in Swift's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotMenuItem {
    /// The plugin's window.
    Open,
    /// Safe mode held it off.
    Load,
    /// "Bypass", or "Turn On" while bypassed.
    Bypass {
        bypassed: bool,
    },
    /// The EQ's plug-in pick: reopens the browser for the EQ slot.
    Replace,
    Sidechain(Vec<SidechainChoice>),
    /// An insert's Remove.
    Remove,
    Divider,
    RemoveEq,
}

impl SlotMenuItem {
    pub fn title(&self) -> &'static str {
        match self {
            SlotMenuItem::Open => SlotLabels::OPEN,
            SlotMenuItem::Load => SlotLabels::LOAD,
            SlotMenuItem::Bypass { bypassed } => SlotLabels::bypass_item(*bypassed),
            SlotMenuItem::Replace => SlotLabels::REPLACE,
            SlotMenuItem::Sidechain(_) => SlotLabels::SIDECHAIN,
            SlotMenuItem::Remove => SlotLabels::REMOVE,
            SlotMenuItem::Divider => "",
            SlotMenuItem::RemoveEq => SlotLabels::REMOVE_EQ,
        }
    }

    /// Every item but the divider.
    pub fn enabled(&self) -> bool {
        !matches!(self, SlotMenuItem::Divider)
    }
}

/// The slots' labels, helps and menu titles (Swift's exact strings).
pub struct SlotLabels;

impl SlotLabels {
    pub const OPEN: &'static str = "Open";
    pub const LOAD: &'static str = "Load";
    pub const BYPASS: &'static str = "Bypass";
    pub const TURN_ON: &'static str = "Turn On";
    pub const SIDECHAIN: &'static str = "Sidechain";
    pub const NO_SIDECHAIN: &'static str = "None";
    pub const REMOVE: &'static str = "Remove";
    pub const REPLACE: &'static str = "Replace\u{2026}";
    pub const REMOVE_EQ: &'static str = "Remove EQ";
    /// The strip header's button while safe mode holds slots off.
    pub const LOAD_ALL: &'static str = "Load All";
    pub const LOAD_ALL_HELP: &'static str = "Safe mode kept these plugins off";
    /// A faulted slot's reason.
    pub const FAULTED: &'static str = "Bypassed: the plugin misbehaved";
    pub const INSERTS: &'static str = "Inserts";
    /// The inserts' add button (opens the effect browser).
    pub const ADD_FACE: &'static str = "+";
    pub const ADD_HELP: &'static str = "Add an effect";
    /// The empty EQ slot (opens the effect browser for the EQ).
    pub const EQ_ADD_FACE: &'static str = "EQ \u{FF0B}";
    pub const EQ_ADD_HELP: &'static str = "Add an EQ";
    pub const EQ_EMPTY: &'static str = "empty";
    pub const BUILTIN_SYNTH: &'static str = "Built-in Synth";
    pub const BUILTIN_SYNTH_LABEL: &'static str = "Built-in Synth, instrument";
    pub const BUILTIN_SYNTH_HINT: &'static str =
        "Opens the instrument menu to choose a plugin; double-click for its sounds";
    pub const BUILTIN_SYNTH_HELP: &'static str =
        "Built-in Synth: click to choose a plugin instrument, double-click to choose its sound";
    pub const INSTRUMENT_HINT: &'static str = "Opens the instrument menu to choose another plugin";

    /// "Bypass", or "Turn On" while bypassed.
    pub fn bypass_item(bypassed: bool) -> &'static str {
        if bypassed {
            Self::TURN_ON
        } else {
            Self::BYPASS
        }
    }

    /// An instrument strip's own slot: "<plugin>, instrument"; any other
    /// slot: the plugin's name.
    pub fn slot(slot: &SlotRow, place: SlotPlace) -> String {
        match place {
            SlotPlace::Instrument => format!("{}, instrument", slot.plugin.name),
            _ => slot.plugin.name.clone(),
        }
    }

    pub fn add_effect(strip: &str) -> String {
        format!("Add an effect to {strip}")
    }

    /// The EQ slot, empty or loaded (its value: "empty" or the plugin).
    pub fn eq(strip: &str) -> String {
        format!("EQ of {strip}")
    }

    /// The loaded EQ's bypass button.
    pub fn eq_bypass(strip: &str, bypassed: bool) -> String {
        if bypassed {
            format!("Turn On EQ of {strip}")
        } else {
            format!("Bypass EQ of {strip}")
        }
    }

    pub fn eq_bypass_help(bypassed: bool) -> &'static str {
        if bypassed {
            "Turn the EQ on"
        } else {
            "Bypass the EQ"
        }
    }
}

impl MixerModel {
    /// `s`'s slots in the order the strip shows and the engine processes
    /// them: the instrument, the EQ, then the inserts.
    pub fn slots(s: &StripRow) -> impl Iterator<Item = &SlotRow> {
        s.instrument
            .iter()
            .chain(s.eq.iter())
            .chain(s.inserts.iter())
    }

    /// The strip holding slot `id`, the slot, and where it sits.
    pub fn slot(&self, id: u64) -> Option<(&StripRow, &SlotRow, SlotPlace)> {
        self.snapshot().strips.iter().find_map(|s| {
            if let Some(x) = s.instrument.as_ref().filter(|x| x.id == id) {
                return Some((s, x, SlotPlace::Instrument));
            }
            if let Some(x) = s.eq.as_ref().filter(|x| x.id == id) {
                return Some((s, x, SlotPlace::Eq));
            }
            let at = s.inserts.iter().position(|x| x.id == id)?;
            Some((s, &s.inserts[at], SlotPlace::Insert(at)))
        })
    }

    /// The instrument row: a plugin instrument's slot, the Built-in
    /// Synth's, or none.
    pub fn instrument_row(s: &StripRow) -> Option<InstrumentRow<'_>> {
        match &s.instrument {
            Some(x) => Some(InstrumentRow::Plugin(x)),
            None if s.kind == StripKindRow::BuiltinSynth => Some(InstrumentRow::BuiltinSynth),
            None => None,
        }
    }

    /// The engine bypassed slot `id` for misbehaving.
    pub fn is_faulted(&self, id: u64) -> bool {
        self.meters().faulted.contains(&id)
    }

    /// Safe mode holds slot `id` off (its menu has Load).
    pub fn is_held(&self, id: u64) -> bool {
        self.held.contains(&id)
    }

    /// `s`'s held slots, in slot order (its "Load All").
    pub fn held_slots_of(&self, s: &StripRow) -> Vec<u64> {
        Self::slots(s)
            .map(|x| x.id)
            .filter(|id| self.is_held(*id))
            .collect()
    }

    /// The dot: faulted first, then the status (a live slot bypassed).
    pub fn slot_state(&self, x: &SlotRow) -> SlotState {
        if self.is_faulted(x.id) {
            return SlotState::Faulted;
        }
        match x.status {
            SlotStatus::Live if x.bypass => SlotState::Bypassed,
            SlotStatus::Live => SlotState::Live,
            SlotStatus::Loading => SlotState::Loading,
            SlotStatus::Placeholder { .. } => SlotState::Placeholder,
        }
    }

    /// Why a slot passes audio through: a placeholder's reason, else
    /// "Bypassed: the plugin misbehaved" when faulted. Its name dims.
    pub fn slot_reason(&self, x: &SlotRow) -> Option<String> {
        if let SlotStatus::Placeholder { reason } = &x.status {
            return Some(reason.clone());
        }
        self.is_faulted(x.id)
            .then(|| SlotLabels::FAULTED.to_owned())
    }

    /// The slot's help: its reason, else "<plugin> (<vendor>)".
    pub fn slot_help(&self, x: &SlotRow) -> String {
        self.slot_reason(x)
            .unwrap_or_else(|| format!("{} ({})", x.plugin.name, x.plugin.vendor))
    }

    /// Strips that may feed a sidechain of `s`'s slots: every strip but
    /// itself and master.
    pub fn sidechain_sources(&self, s: &StripRow) -> Vec<&StripRow> {
        self.strips().filter(|x| x.id != s.id).collect()
    }

    /// The sidechain menu: "None", then the sources ("✓ " before the one
    /// feeding it).
    pub fn sidechain_choices(&self, s: &StripRow, x: &SlotRow) -> Vec<SidechainChoice> {
        let none = SidechainChoice {
            title: SlotLabels::NO_SIDECHAIN.into(),
            source: None,
        };
        std::iter::once(none)
            .chain(self.sidechain_sources(s).into_iter().map(|src| {
                let tick = if x.sidechain == Some(src.id) {
                    "\u{2713} "
                } else {
                    ""
                };
                SidechainChoice {
                    title: format!("{tick}{}", src.name),
                    source: Some(src.id),
                }
            }))
            .collect()
    }

    /// Slot `x`'s menu at `place` on `s` (Swift `SlotView.menu`,
    /// `EqSlotView.menu`): Open, Load (held), then for the EQ Replace…;
    /// Bypass or Turn On; Sidechain (inserts and the EQ, with a sidechain
    /// input); an insert's Remove, or the EQ's divider and Remove EQ.
    pub fn slot_menu(&self, s: &StripRow, x: &SlotRow, place: SlotPlace) -> Vec<SlotMenuItem> {
        let mut out = vec![SlotMenuItem::Open];
        if self.is_held(x.id) {
            out.push(SlotMenuItem::Load);
        }
        if place == SlotPlace::Eq {
            out.push(SlotMenuItem::Replace);
        }
        out.push(SlotMenuItem::Bypass { bypassed: x.bypass });
        if place != SlotPlace::Instrument && x.has_sidechain_input {
            out.push(SlotMenuItem::Sidechain(self.sidechain_choices(s, x)));
        }
        match place {
            SlotPlace::Insert(_) => out.push(SlotMenuItem::Remove),
            SlotPlace::Eq => out.extend([SlotMenuItem::Divider, SlotMenuItem::RemoveEq]),
            SlotPlace::Instrument => {}
        }
        out
    }

    // Edits.

    /// Bypasses slot `id`, or turns it on (one undo step). An unknown slot
    /// sends nothing.
    pub fn toggle_bypass(&mut self, doc: &mut DocumentModel, id: u64) {
        let Some(on) = self.slot(id).map(|(_, x, _)| x.bypass) else {
            return;
        };
        self.command(doc, BYPASS_FAILED, |s| s.set_bypass(id, !on));
    }

    /// Removes insert `slot` (one undo step).
    pub fn remove_insert(&mut self, doc: &mut DocumentModel, slot: u64) {
        self.command(doc, REMOVE_PLUGIN_FAILED, |s| s.remove_insert(slot));
    }

    /// Drops insert `slot` at position `index` of `strip`'s inserts (a
    /// drag to reorder; `index` = the inserts' count drops it last, on
    /// "+").
    pub fn move_insert(&mut self, doc: &mut DocumentModel, slot: u64, strip: u64, index: usize) {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        self.command(doc, MOVE_PLUGIN_FAILED, |s| {
            s.move_insert(slot, strip, index)
        });
    }

    /// Feeds slot `slot`'s sidechain from strip `source` (`None`:
    /// nothing), one undo step.
    pub fn set_sidechain(&mut self, doc: &mut DocumentModel, slot: u64, source: Option<u64>) {
        self.command(doc, SIDECHAIN_FAILED, |s| s.set_sidechain(slot, source));
    }

    /// Loads held slots (safe mode's Load and Load All); the mixer is read
    /// again either way.
    pub fn load(&mut self, doc: &mut DocumentModel, slots: Vec<u64>) {
        let session = doc.session().clone();
        if let Err(e) = session.load_plugins(slots) {
            super::model::raise(doc, crate::alerts::AlertItem::from_error(&e, LOAD_FAILED));
        }
        self.refetch(doc);
    }

    /// The strip header's Load All: every held slot of `strip`.
    pub fn load_all(&mut self, doc: &mut DocumentModel, strip: u64) {
        let Some(held) = self.strip(strip).map(|s| self.held_slots_of(s)) else {
            return;
        };
        if held.is_empty() {
            return;
        }
        self.load(doc, held);
    }

    /// Empties `strip`'s EQ slot (one undo step "Remove EQ"). An empty slot
    /// is no call: the core would refuse it with a spurious alert.
    pub fn clear_eq(&mut self, doc: &mut DocumentModel, strip: u64) {
        if self.strip(strip).is_none_or(|s| s.eq.is_none()) {
            return;
        }
        self.command(doc, CLEAR_EQ_FAILED, |s| s.set_eq(strip, None));
    }
}
