//! Fader groups (Swift `MixerGroups`): the group a strip is in, the Mixer
//! menu's rules on the selection, and the group edits. Every edit is one
//! `run_mixer` call, so the core's rules decide (one group per strip);
//! a refusal is its alert. A drag of a grouped fader is the core's: it
//! moves the followers (shown through the core's values) as one step
//! "Move Group <name>", and ⌥ at the press moves the fader alone.

use std::collections::BTreeSet;

use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::mixer::{GroupRow, StripKindRow};

use super::model::MixerModel;
use crate::document::DocumentModel;

/// The alert of a refused group edit.
pub const GROUP_FAILED: &str = "Group Failed";
/// The alert of a refused Ungroup.
pub const UNGROUP_FAILED: &str = "Ungroup Failed";

/// The group tag's menu items.
pub struct GroupLabels;

impl GroupLabels {
    pub const LINK_MUTES: &'static str = "Link Mutes";
    pub const LINK_SOLOS: &'static str = "Link Solos";
    pub const REMOVE: &'static str = "Remove from Group";
    pub const UNGROUP: &'static str = "Ungroup";
}

/// `strips` as the registry's `strips` text ("2,3"); `None` for none.
pub fn strips_text(strips: &[u64]) -> Option<String> {
    (!strips.is_empty()).then(|| {
        strips
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    })
}

/// `strips` as a `strips` argument; none for an empty list.
pub(super) fn strips_arg(strips: &[u64]) -> Vec<ControlArg> {
    strips_text(strips)
        .map(|value| ControlArg {
            name: "strips".into(),
            value: ControlValue::Text { value },
        })
        .into_iter()
        .collect()
}

pub(super) fn id_arg(name: &str, id: u64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int {
            value: i64::try_from(id).unwrap_or(i64::MAX),
        },
    }
}

pub(super) fn flag_arg(name: &str, on: bool) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Flag { value: on },
    }
}

impl DocumentModel {
    /// The selected strips a group or VCA may take, as the session's
    /// mixer has them now: in ID order, without master and without strips
    /// it no longer has (the selection alone, sorted, when it cannot be
    /// read).
    pub fn selected_member_strips(&self) -> Vec<u64> {
        let Ok(mixer) = self.session().mixer() else {
            return self.selected_strips().iter().copied().collect();
        };
        let members: BTreeSet<u64> = mixer
            .strips
            .iter()
            .filter(|s| s.kind != StripKindRow::Master)
            .map(|s| s.id)
            .collect();
        self.selected_strips()
            .intersection(&members)
            .copied()
            .collect()
    }
}

impl MixerModel {
    pub fn groups(&self) -> &[GroupRow] {
        &self.snapshot().groups
    }

    /// The group `s` is in.
    pub fn group_of(&self, s: &vizdaw_ffi::mixer::StripRow) -> Option<&GroupRow> {
        let g = s.group?;
        self.groups().iter().find(|x| x.id == g)
    }

    /// The selected strips a group or VCA may take: those this mixer
    /// shows but master, in ID order.
    pub fn selected_members(&self, doc: &DocumentModel) -> Vec<u64> {
        doc.selected_strips()
            .iter()
            .copied()
            .filter(|id| self.strips().any(|s| s.id == *id))
            .collect()
    }

    /// A strip menu's strips: the selection when it holds `strip`, else
    /// `strip`; none for master.
    pub fn menu_strips(&self, doc: &DocumentModel, strip: u64) -> Vec<u64> {
        if self.strips().all(|s| s.id != strip) {
            return vec![];
        }
        let sel = self.selected_members(doc);
        if sel.contains(&strip) {
            sel
        } else {
            vec![strip]
        }
    }

    /// Group takes `strips` when there is one and none is in a group (the
    /// core refuses a strip in another group).
    pub fn can_group(&self, strips: &[u64]) -> bool {
        !strips.is_empty()
            && strips
                .iter()
                .all(|id| self.strip(*id).is_some_and(|s| s.group.is_none()))
    }

    /// Mixer › Group.
    pub fn can_group_selection(&self, doc: &DocumentModel) -> bool {
        self.can_group(&self.selected_members(doc))
    }

    /// The one group every selected strip is in (Mixer › Ungroup), if any.
    pub fn selection_group(&self, doc: &DocumentModel) -> Option<&GroupRow> {
        let sel = self.selected_members(doc);
        let g = self.group_of(self.strip(*sel.first()?)?)?;
        sel.iter()
            .all(|id| self.strip(*id).is_some_and(|s| s.group == Some(g.id)))
            .then_some(g)
    }

    // Edits.

    /// `mixer.new_group` with the selected strips.
    pub fn group_selection(&mut self, doc: &mut DocumentModel) {
        let strips = self.selected_members(doc);
        self.new_group(doc, &strips);
    }

    /// `mixer.new_group` of `strips`.
    pub fn new_group(&mut self, doc: &mut DocumentModel, strips: &[u64]) {
        self.run_mixer(doc, "mixer.new_group", strips_arg(strips), GROUP_FAILED);
    }

    /// Mixer › Ungroup: deletes the selection's group.
    pub fn ungroup_selection(&mut self, doc: &mut DocumentModel) {
        if let Some(g) = self.selection_group(doc).map(|g| g.id) {
            self.delete_group(doc, g);
        }
    }

    pub fn delete_group(&mut self, doc: &mut DocumentModel, group: u64) {
        let args = vec![id_arg("group", group)];
        self.run_mixer(doc, "mixer.delete_group", args, UNGROUP_FAILED);
    }

    /// Takes `strips` out of `group`; none sends nothing.
    pub fn remove_from_group(&mut self, doc: &mut DocumentModel, group: u64, strips: &[u64]) {
        if strips.is_empty() {
            return;
        }
        let mut args = vec![id_arg("group", group)];
        args.extend(strips_arg(strips));
        self.run_mixer(doc, "mixer.remove_from_group", args, GROUP_FAILED);
    }

    /// The group's mute or solo link switch; neither sends nothing.
    pub fn set_group_links(
        &mut self,
        doc: &mut DocumentModel,
        group: u64,
        mute: Option<bool>,
        solo: Option<bool>,
    ) {
        let mut args = vec![id_arg("group", group)];
        args.extend(mute.map(|on| flag_arg("mute", on)));
        args.extend(solo.map(|on| flag_arg("solo", on)));
        if args.len() > 1 {
            self.run_mixer(doc, "mixer.set_group_links", args, GROUP_FAILED);
        }
    }

    /// One `run_mixer` call, its failure the alert `title`.
    pub(super) fn run_mixer(
        &mut self,
        doc: &mut DocumentModel,
        id: &str,
        args: Vec<ControlArg>,
        title: &str,
    ) -> bool {
        self.command(doc, title, |s| s.run_mixer(id, args))
    }
}
