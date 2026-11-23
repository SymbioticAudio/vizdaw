//! A strip's sends and output (Swift `MixerModel.addSend/editSend/
//! removeSend/setOutput`, `SendKnobRow`, `StripOutputMenu`). A send is
//! counted from 1 wherever a person or the channel reads it ("Send 2 of
//! Synth", `mixer.set_send {index: 2}`, `send:1:2`) and from 0 at the
//! session (`edit_send(strip, 1, …)`, `SendLevel { send: 1 }`).

use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::mixer::{SendRow, StripKindRow, StripRow};

use super::model::{MixerModel, send_index};
use crate::document::DocumentModel;

/// The alert title of a refused send edit.
pub const SEND_FAILED: &str = "Send Failed";

/// The sends' labels and menu items (Swift `SendKnobRow`, `SendKnob`).
pub struct SendLabels;

impl SendLabels {
    /// The send menu's last item, and its label.
    pub const NEW_BUS: &'static str = "New Bus";
    /// The new-bus plus's help (shown only while the strip has no sends).
    pub const NEW_BUS_HELP: &'static str = "New Bus: a bus with a send from this strip";
    /// The send menu's help.
    pub const ADD_HELP: &'static str = "Add a send";
    /// The send menu's face.
    pub const MENU_TEXT: &'static str = "Send";
    /// The sends row's heading.
    pub const HEADING: &'static str = "Sends";
    /// A send's menu items.
    pub const PRE_FADER: &'static str = "Pre-Fader";
    pub const ON: &'static str = "On";
    pub const REMOVE: &'static str = "Remove Send";
    /// The chip of a pre-fader send, and its help.
    pub const PRE_CHIP: &'static str = "PRE";
    pub const PRE_HELP: &'static str = "Pre-fader";

    /// The knob of send `index` (from 0): "Send knob <n> of <strip>", n
    /// from 1.
    pub fn knob(index: usize, strip: &str) -> String {
        format!("Send knob {} of {strip}", index + 1)
    }

    /// The level box of send `index` (from 0): "Send <n> of <strip>".
    pub fn level_box(index: usize, strip: &str) -> String {
        format!("Send {} of {strip}", index + 1)
    }

    pub fn add_menu(strip: &str) -> String {
        format!("Add a send to {strip}")
    }

    pub fn new_bus_plus(strip: &str) -> String {
        format!("New bus fed by {strip}")
    }
}

/// One item of a strip's output menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputChoice {
    pub title: String,
    /// `None`: master.
    pub bus: Option<u64>,
    /// The strip's output now.
    pub checked: bool,
}

impl MixerModel {
    /// Whether `s` has the send, output and pan rows: every strip but
    /// master.
    pub fn routes(s: &StripRow) -> bool {
        s.kind != StripKindRow::Master
    }

    /// The output menu: Master, then the buses but the strip itself, the
    /// current one checked. Empty for master.
    pub fn output_choices(&self, s: &StripRow) -> Vec<OutputChoice> {
        if !Self::routes(s) {
            return vec![];
        }
        let master = OutputChoice {
            title: "Master".into(),
            bus: None,
            checked: s.output.is_none(),
        };
        std::iter::once(master)
            .chain(self.targets(s).into_iter().map(|b| OutputChoice {
                title: b.name.clone(),
                bus: Some(b.id),
                checked: s.output == Some(b.id),
            }))
            .collect()
    }

    /// The output menu's face: "→ <Master or bus>".
    pub fn output_text(&self, s: &StripRow) -> String {
        format!("\u{2192} {}", self.output_name(s))
    }

    /// The name of the bus `send` feeds.
    pub fn send_name(&self, send: &SendRow) -> String {
        self.strip_name(send.target)
    }

    /// The automation target of `strip`'s send `index` (from 0).
    pub fn send_target(&self, strip: u64, index: usize) -> AutomationTargetRow {
        AutomationTargetRow::SendLevel {
            strip,
            send: send_index(index),
        }
    }

    /// A post-fader 0 dB send from `strip` to `bus`, last (one undo step).
    /// Master sends nothing.
    pub fn add_send(&mut self, doc: &mut DocumentModel, strip: u64, bus: u64) {
        if self.strip(strip).is_none_or(|s| !Self::routes(s)) {
            return;
        }
        self.command(doc, SEND_FAILED, |s| s.add_send(strip, bus));
    }

    /// Edits `strip`'s send `index` (from 0) by `change`, one undo step;
    /// nothing for a send the strip does not have.
    pub fn edit_send(
        &mut self,
        doc: &mut DocumentModel,
        strip: u64,
        index: usize,
        change: impl FnOnce(&mut SendRow),
    ) {
        let Some(mut send) = self.strip(strip).and_then(|s| s.sends.get(index)).cloned() else {
            return;
        };
        let Ok(at) = u32::try_from(index) else {
            return;
        };
        change(&mut send);
        self.command(doc, SEND_FAILED, |s| s.edit_send(strip, at, send));
    }

    /// The send menu's Pre-Fader toggle.
    pub fn set_send_pre_fader(
        &mut self,
        doc: &mut DocumentModel,
        strip: u64,
        index: usize,
        on: bool,
    ) {
        self.edit_send(doc, strip, index, |s| s.pre_fader = on);
    }

    /// The send menu's On toggle.
    pub fn set_send_enabled(
        &mut self,
        doc: &mut DocumentModel,
        strip: u64,
        index: usize,
        on: bool,
    ) {
        self.edit_send(doc, strip, index, |s| s.enabled = on);
    }

    /// Removes `strip`'s send `index` (from 0), one undo step.
    pub fn remove_send(&mut self, doc: &mut DocumentModel, strip: u64, index: usize) {
        let Ok(at) = u32::try_from(index) else {
            return;
        };
        self.command(doc, SEND_FAILED, |s| s.remove_send(strip, at));
    }
}
