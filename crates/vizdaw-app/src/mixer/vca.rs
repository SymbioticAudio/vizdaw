//! VCAs (Swift `MixerGroups`, `VcaStripLabels`, `VcaStrip`): the VCA
//! columns' labels, the members' offsets and marks, the Mixer menu's
//! assign rules and the VCA edits. A VCA's fader is a gesture on its
//! automation target; every other edit is one `run_mixer` call.

use std::time::Instant;

use core_model::automation::fader_norm;
use core_model::groups::heard_db;
use vizdaw_ffi::automation::AutomationModeRow;
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::mixer::{StripRow, VcaRow};

use super::groups::{flag_arg, id_arg, strips_arg};
use super::math::{AutomationModes, MixerMath, StripEntry};
use super::model::{MixerModel, MixerPlacement};
use crate::document::DocumentModel;

/// The alert of a refused VCA edit (assign, remove, mute, solo, mode).
pub const VCA_FAILED: &str = "VCA Failed";
pub const NEW_VCA_FAILED: &str = "New VCA Failed";
pub const DELETE_VCA_FAILED: &str = "Delete VCA Failed";
const RENAME_FAILED: &str = "Rename Failed";

/// The labels of the group tag, the VCA offset readout and the VCA
/// strip's controls (Swift `VcaStripLabels`).
pub struct VcaStripLabels;

impl VcaStripLabels {
    pub const MODE: &'static str = "VCA automation mode";
    /// The VCA menu's items and the rename field.
    pub const RENAME: &'static str = "Rename VCA\u{2026}";
    pub const DELETE: &'static str = "Delete VCA";
    pub const RENAME_FIELD: &'static str = "Name of the VCA";
    /// The member's mark's text.
    pub const MARK_TEXT: &'static str = "VCA";

    pub fn group(name: &str) -> String {
        format!("Group {name}")
    }

    pub fn offset(text: &str) -> String {
        format!("VCA offset {text}")
    }

    /// A VCA's label, as the core's `vca_label`: its name when that is
    /// "VCA" or starts with "VCA ", else "VCA <name>".
    pub fn title(name: &str) -> String {
        if name == "VCA" || name.starts_with("VCA ") {
            name.to_owned()
        } else {
            format!("VCA {name}")
        }
    }

    pub fn fader(name: &str) -> String {
        format!("Fader of {}", Self::title(name))
    }

    pub fn level(name: &str) -> String {
        format!("Level of {}", Self::title(name))
    }

    pub fn mute(name: &str) -> String {
        format!("Mute {}", Self::title(name))
    }

    pub fn solo(name: &str) -> String {
        format!("Solo {}", Self::title(name))
    }

    /// The strip's "VCA" mark: a VCA mutes or solos it.
    pub fn mark(strip: &str) -> String {
        format!("VCA mute or solo on {strip}")
    }

    /// "1 member", "3 members".
    pub fn members(n: usize) -> String {
        if n == 1 {
            "1 member".into()
        } else {
            format!("{n} members")
        }
    }

    /// The VCA column's kind line: "VCA · 3 members".
    pub fn kind_line(n: usize) -> String {
        format!("VCA \u{b7} {}", Self::members(n))
    }
}

impl AutomationModes {
    /// A VCA's modes, in menu order (the registry's `VCA_MODE_NAMES`):
    /// Write and Trim are not among them.
    pub const VCA: [AutomationModeRow; 4] = [
        AutomationModeRow::Off,
        AutomationModeRow::Read,
        AutomationModeRow::Touch,
        AutomationModeRow::Latch,
    ];

    /// The registry's name of `m` ("off" … "trim").
    pub fn name(m: AutomationModeRow) -> &'static str {
        match m {
            AutomationModeRow::Off => "off",
            AutomationModeRow::Read => "read",
            AutomationModeRow::Touch => "touch",
            AutomationModeRow::Latch => "latch",
            AutomationModeRow::Write => "write",
            AutomationModeRow::Trim => "trim",
        }
    }
}

impl MixerModel {
    pub fn vca(&self, id: u64) -> Option<&VcaRow> {
        self.vcas().iter().find(|v| v.id == id)
    }

    // Offsets.

    /// The sum of `s`'s VCAs' shown values (live, the core's, heard, the
    /// snapshot's), in dB: the floor when any is at it or the sum reaches
    /// it, 0 in none. It re-derives `StripRow::vca_offset_db` so the
    /// readout follows a VCA drag.
    pub fn vca_offset_db(&self, s: &StripRow) -> f64 {
        let mut sum = 0.0;
        for v in s.vcas.iter().filter_map(|id| self.vca(*id)) {
            let db = self.vca_gain_db(v);
            if db.is_nan() || db <= MixerMath::FLOOR_DB {
                return MixerMath::FLOOR_DB;
            }
            sum += db;
        }
        if sum <= MixerMath::FLOOR_DB {
            MixerMath::FLOOR_DB
        } else {
            sum
        }
    }

    /// What a member is heard at: its own fader as shown plus its VCAs'
    /// offset (the floor when either silences it). A strip in no VCA is
    /// at its own level. The fader itself shows its own level.
    pub fn member_level_db(&self, s: &StripRow) -> f64 {
        let own = self.gain_db(s);
        if s.vcas.iter().all(|id| self.vca(*id).is_none()) {
            return own;
        }
        heard_db(own, self.vca_offset_db(s))
    }

    /// "VCA −3.0", "VCA +2.0", "VCA −∞"; `None` when the offset reads
    /// 0.0.
    pub fn vca_offset_text(db: f64) -> Option<String> {
        if db.is_nan() || db <= MixerMath::FLOOR_DB {
            return Some("VCA \u{2212}\u{221E}".into());
        }
        let text = format!("{:.1}", db.abs());
        if text == "0.0" {
            return None;
        }
        Some(format!(
            "VCA {}{text}",
            if db < 0.0 { '\u{2212}' } else { '+' }
        ))
    }

    /// A VCA mutes or solos `s` (the snapshot's mark).
    pub fn vca_mark(&self, s: &StripRow) -> bool {
        s.vca_mark
    }

    // The Mixer menu.

    /// Of `strips`, those `v` does not hold yet (assigning a member again
    /// is refused).
    pub fn assignable(&self, strips: &[u64], v: &VcaRow) -> Vec<u64> {
        strips
            .iter()
            .copied()
            .filter(|s| !v.members.contains(s))
            .collect()
    }

    /// The VCAs holding any of `strips`.
    pub fn vcas_holding(&self, strips: &[u64]) -> Vec<&VcaRow> {
        self.vcas()
            .iter()
            .filter(|v| strips.iter().any(|s| v.members.contains(s)))
            .collect()
    }

    // Edits.

    /// `mixer.new_vca`, with the selected strips when `from_selection`.
    pub fn new_vca_from_selection(&mut self, doc: &mut DocumentModel, from_selection: bool) {
        let strips = if from_selection {
            self.selected_members(doc)
        } else {
            vec![]
        };
        self.new_vca(doc, &strips);
    }

    pub fn new_vca(&mut self, doc: &mut DocumentModel, strips: &[u64]) {
        self.run_mixer(doc, "mixer.new_vca", strips_arg(strips), NEW_VCA_FAILED);
    }

    /// Assigns `strips` to `vca` (`on`) or removes them from it; none
    /// sends nothing.
    pub fn assign(&mut self, doc: &mut DocumentModel, strips: &[u64], vca: u64, on: bool) {
        if strips.is_empty() {
            return;
        }
        let id = if on {
            "mixer.assign_vca"
        } else {
            "mixer.unassign_vca"
        };
        let mut args = vec![id_arg("vca", vca)];
        args.extend(strips_arg(strips));
        self.run_mixer(doc, id, args, VCA_FAILED);
    }

    /// Deletes `vca`; its members keep their own faders.
    pub fn delete_vca(&mut self, doc: &mut DocumentModel, vca: u64) {
        let args = vec![id_arg("vca", vca)];
        self.run_mixer(doc, "mixer.delete_vca", args, DELETE_VCA_FAILED);
    }

    pub fn toggle_vca_mute(&mut self, doc: &mut DocumentModel, vca: u64) {
        if let Some(on) = self.vca(vca).map(|v| v.mute) {
            self.set_vca(doc, vca, flag_arg("mute", !on));
        }
    }

    pub fn toggle_vca_solo(&mut self, doc: &mut DocumentModel, vca: u64) {
        if let Some(on) = self.vca(vca).map(|v| v.solo) {
            self.set_vca(doc, vca, flag_arg("solo", !on));
        }
    }

    /// Sets `vca`'s mode; the one it has sends nothing.
    pub fn set_vca_mode(&mut self, doc: &mut DocumentModel, vca: u64, mode: AutomationModeRow) {
        if self.vca(vca).is_none_or(|v| v.automation == mode) {
            return;
        }
        let arg = ControlArg {
            name: "mode".into(),
            value: ControlValue::Text {
                value: AutomationModes::name(mode).into(),
            },
        };
        self.set_vca(doc, vca, arg);
    }

    /// A typed VCA level, as one click on its fader. False, with the
    /// status line set, when the text is refused or the fader is touched.
    pub fn type_vca_level(
        &mut self,
        doc: &mut DocumentModel,
        vca: u64,
        text: &str,
        now: Instant,
    ) -> bool {
        let Some(db) = StripEntry::level(text) else {
            doc.set_status(Some(StripEntry::LEVEL_HINT.into()));
            return false;
        };
        let t = AutomationTargetRow::VcaFader { vca };
        if self.is_touched(t) {
            doc.set_status(Some(StripEntry::TOUCHED_HINT.into()));
            return false;
        }
        self.click(doc, t, fader_norm(db), now);
        true
    }

    // Renaming a VCA.

    pub fn begin_vca_rename(&mut self, vca: u64, p: MixerPlacement) {
        let Some(name) = self.vca(vca).map(|v| v.name.clone()) else {
            return;
        };
        self.rename_draft = name;
        self.renaming_vca = Some((vca, p));
    }

    pub fn is_renaming_vca(&self, vca: u64, p: MixerPlacement) -> bool {
        self.renaming_vca == Some((vca, p))
    }

    /// The VCA whose name is being edited, and where.
    pub fn renaming_vca(&self) -> Option<(u64, MixerPlacement)> {
        self.renaming_vca
    }

    /// Renames to the draft; a blank or unchanged name sends nothing.
    pub fn commit_vca_rename(&mut self, doc: &mut DocumentModel) {
        let Some((id, _)) = self.renaming_vca.take() else {
            return;
        };
        let name = self.rename_draft.trim().to_owned();
        if name.is_empty() || self.vca(id).is_some_and(|v| v.name == name) {
            return;
        }
        let args = vec![
            id_arg("vca", id),
            ControlArg {
                name: "name".into(),
                value: ControlValue::Text { value: name },
            },
        ];
        self.run_mixer(doc, "mixer.rename_vca", args, RENAME_FAILED);
    }

    pub fn cancel_vca_rename(&mut self) {
        self.renaming_vca = None;
    }

    fn set_vca(&mut self, doc: &mut DocumentModel, vca: u64, arg: ControlArg) {
        self.run_mixer(
            doc,
            "mixer.set_vca",
            vec![id_arg("vca", vca), arg],
            VCA_FAILED,
        );
    }
}
