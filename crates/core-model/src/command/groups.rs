//! Fader-group and VCA edits (plan groups-vcas Rulings 1–6). Every
//! command checks everything before it changes anything; on error the
//! document is unchanged (commands work on a copy). No VCA command touches
//! a strip (spec §2).

use super::Report;
use crate::automation::AutomationMode;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::groups::{Group, MAX_MEMBERS, VCA_MODES, Vca, check_name, group_of};
use crate::ids::{AnyId, GroupId, StripId, VcaId};
use crate::mixer::Swatch;
use crate::mixer_rules::level;
use std::collections::BTreeSet;

/// What a group's name is called in messages.
const GROUP_NAME: &str = "A group's name";
/// What a VCA's name is called in messages.
const VCA_NAME: &str = "A VCA's name";

/// Fader-group and VCA edits.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupCmd {
    /// Make a group of `members`, with both links on.
    NewGroup {
        /// Name ([`check_name`]).
        name: String,
        /// Colour.
        colour: Swatch,
        /// Its first members, in order; may be empty.
        members: Vec<StripId>,
    },
    /// Rename a group.
    RenameGroup {
        /// Target.
        group: GroupId,
        /// New name ([`check_name`]).
        name: String,
    },
    /// Recolour a group.
    SetGroupColour {
        /// Target.
        group: GroupId,
        /// New colour.
        colour: Swatch,
    },
    /// Add strips to a group, at the end, in order.
    AddToGroup {
        /// Target.
        group: GroupId,
        /// Strips in no group.
        strips: Vec<StripId>,
    },
    /// Take strips out of a group; an emptied group stays.
    RemoveFromGroup {
        /// Target.
        group: GroupId,
        /// Members of it.
        strips: Vec<StripId>,
    },
    /// Turn a group's link switches on or off; `None` leaves one as is.
    SetGroupLinks {
        /// Target.
        group: GroupId,
        /// Linked mute.
        mute: Option<bool>,
        /// Linked solo.
        solo: Option<bool>,
    },
    /// Delete a group; its members keep their faders and switches.
    DeleteGroup {
        /// Target.
        group: GroupId,
    },
    /// Make a VCA of `members` at 0 dB, unmuted, unsoloed, in Read.
    NewVca {
        /// Name ([`check_name`]).
        name: String,
        /// Colour.
        colour: Swatch,
        /// Its first members, in order; may be empty.
        members: Vec<StripId>,
    },
    /// Rename a VCA.
    RenameVca {
        /// Target.
        vca: VcaId,
        /// New name ([`check_name`]).
        name: String,
    },
    /// Recolour a VCA.
    SetVcaColour {
        /// Target.
        vca: VcaId,
        /// New colour.
        colour: Swatch,
    },
    /// Assign strips to a VCA, at the end, in order.
    AssignVca {
        /// Target.
        vca: VcaId,
        /// Strips not in it yet.
        strips: Vec<StripId>,
    },
    /// Take strips off a VCA; an emptied VCA stays.
    UnassignVca {
        /// Target.
        vca: VcaId,
        /// Members of it.
        strips: Vec<StripId>,
    },
    /// Set a VCA's level.
    SetVcaLevel {
        /// Target.
        vca: VcaId,
        /// Level in dB, `GAIN_FLOOR_DB..=GAIN_MAX_DB`.
        gain_db: f64,
    },
    /// Mute or unmute a VCA.
    SetVcaMute {
        /// Target.
        vca: VcaId,
        /// Muted.
        mute: bool,
    },
    /// Solo or unsolo a VCA.
    SetVcaSolo {
        /// Target.
        vca: VcaId,
        /// Soloed.
        solo: bool,
    },
    /// Set a VCA's automation mode, one of [`VCA_MODES`].
    SetVcaMode {
        /// Target.
        vca: VcaId,
        /// Mode.
        mode: AutomationMode,
    },
    /// Delete a VCA; its members keep their faders and switches.
    DeleteVca {
        /// Target.
        vca: VcaId,
    },
}

impl GroupCmd {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            GroupCmd::NewGroup { .. } => "New Group",
            GroupCmd::RenameGroup { .. } => "Rename Group",
            GroupCmd::SetGroupColour { .. } => "Group Colour",
            GroupCmd::AddToGroup { .. } => "Add to Group",
            GroupCmd::RemoveFromGroup { .. } => "Remove from Group",
            GroupCmd::SetGroupLinks { .. } => "Group Links",
            GroupCmd::DeleteGroup { .. } => "Delete Group",
            GroupCmd::NewVca { .. } => "New VCA",
            GroupCmd::RenameVca { .. } => "Rename VCA",
            GroupCmd::SetVcaColour { .. } => "VCA Colour",
            GroupCmd::AssignVca { .. } => "Assign to VCA",
            GroupCmd::UnassignVca { .. } => "Remove from VCA",
            GroupCmd::SetVcaLevel { .. } => "Set VCA",
            GroupCmd::SetVcaMute { .. } => "Mute VCA",
            GroupCmd::SetVcaSolo { .. } => "Solo VCA",
            GroupCmd::SetVcaMode { .. } => "Set VCA Automation Mode",
            GroupCmd::DeleteVca { .. } => "Delete VCA",
        }
    }
}

/// Refuses more than [`MAX_MEMBERS`] members in `what` ("A group", "A VCA").
fn check_size(what: &str, n: usize) -> Result<()> {
    if n > MAX_MEMBERS {
        return Err(ModelError::invalid(format!(
            "{what} holds at most {MAX_MEMBERS} strips."
        )));
    }
    Ok(())
}

/// Refuses a list with more than [`MAX_MEMBERS`] strips or a strip twice;
/// `what` names the holder in the message.
fn check_list(what: &str, strips: &[StripId]) -> Result<()> {
    check_size(what, strips.len())?;
    let mut seen = BTreeSet::new();
    for s in strips {
        if !seen.insert(*s) {
            return Err(ModelError::invalid(format!("{s} is listed twice.")));
        }
    }
    Ok(())
}

/// Refuses strips that may not join a group: unknown ones, the master and
/// any in a group already (Ruling 3).
fn check_joiners(d: &Document, strips: &[StripId]) -> Result<()> {
    let m = d.mixer();
    for id in strips {
        let s = m.strip(*id)?;
        if *id == m.master() {
            return Err(ModelError::invalid("Master cannot join a group."));
        }
        if let Some(g) = group_of(m, *id) {
            return Err(ModelError::invalid(format!(
                "Strip {} is in group {}; remove it from that group first.",
                s.name, g.name
            )));
        }
    }
    Ok(())
}

/// Refuses strips that may not join VCA `vca` (`None` for a new one):
/// unknown ones, the master and its members already.
fn check_vca_joiners(d: &Document, vca: Option<&Vca>, strips: &[StripId]) -> Result<()> {
    let m = d.mixer();
    for id in strips {
        let s = m.strip(*id)?;
        if *id == m.master() {
            return Err(ModelError::invalid("Master cannot join a VCA."));
        }
        if let Some(v) = vca.filter(|v| v.members.contains(id)) {
            return Err(ModelError::invalid(format!(
                "Strip {} is already in VCA {}.",
                s.name, v.name
            )));
        }
    }
    Ok(())
}

/// Refuses an empty strip list.
fn need_strips(strips: &[StripId]) -> Result<()> {
    if strips.is_empty() {
        return Err(ModelError::invalid("Name at least one strip."));
    }
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &GroupCmd, r: &mut Report) -> Result<()> {
    match cmd {
        GroupCmd::NewGroup {
            name,
            colour,
            members,
        } => {
            let name = check_name(GROUP_NAME, name)?;
            check_list("A group", members)?;
            check_joiners(d, members)?;
            let id = d.counters_mut().alloc_group();
            if d.mixer().groups().contains_key(&id) {
                return Err(ModelError::invalid("no free group ID"));
            }
            d.mixer_mut().insert_group(Group {
                id,
                name,
                colour: *colour,
                members: members.iter().copied().collect(),
                link_mute: true,
                link_solo: true,
            });
            r.created.push(AnyId::Group(id));
        }
        GroupCmd::RenameGroup { group, name } => {
            let name = check_name(GROUP_NAME, name)?;
            d.mixer_mut().group_mut(*group)?.name = name;
        }
        GroupCmd::SetGroupColour { group, colour } => {
            d.mixer_mut().group_mut(*group)?.colour = *colour;
        }
        GroupCmd::AddToGroup { group, strips } => {
            let held = d.mixer().group(*group)?.members.len();
            need_strips(strips)?;
            check_list("A group", strips)?;
            check_size("A group", held.saturating_add(strips.len()))?;
            check_joiners(d, strips)?;
            d.mixer_mut()
                .group_mut(*group)?
                .members
                .extend(strips.iter().copied());
        }
        GroupCmd::RemoveFromGroup { group, strips } => {
            let g = d.mixer().group(*group)?;
            need_strips(strips)?;
            check_list("A group", strips)?;
            for id in strips {
                let s = d.mixer().strip(*id)?;
                if !g.members.contains(id) {
                    return Err(ModelError::invalid(format!(
                        "Strip {} is not in group {}.",
                        s.name, g.name
                    )));
                }
            }
            d.mixer_mut()
                .group_mut(*group)?
                .members
                .retain(|x| !strips.contains(x));
        }
        GroupCmd::SetGroupLinks { group, mute, solo } => {
            d.mixer().group(*group)?;
            if mute.is_none() && solo.is_none() {
                return Err(ModelError::invalid(
                    "Set the mute link, the solo link or both.",
                ));
            }
            let g = d.mixer_mut().group_mut(*group)?;
            if let Some(m) = mute {
                g.link_mute = *m;
            }
            if let Some(s) = solo {
                g.link_solo = *s;
            }
        }
        GroupCmd::DeleteGroup { group } => {
            d.mixer().group(*group)?;
            d.mixer_mut().remove_group(*group);
            r.removed.push(AnyId::Group(*group));
        }
        GroupCmd::NewVca {
            name,
            colour,
            members,
        } => {
            let name = check_name(VCA_NAME, name)?;
            check_list("A VCA", members)?;
            check_vca_joiners(d, None, members)?;
            let id = d.counters_mut().alloc_vca();
            if d.mixer().vcas().contains_key(&id) {
                return Err(ModelError::invalid("no free VCA ID"));
            }
            d.mixer_mut().insert_vca(Vca {
                id,
                name,
                colour: *colour,
                members: members.iter().copied().collect(),
                gain_db: 0.0,
                mute: false,
                solo: false,
                automation: AutomationMode::Read,
            });
            r.created.push(AnyId::Vca(id));
        }
        GroupCmd::RenameVca { vca, name } => {
            let name = check_name(VCA_NAME, name)?;
            d.mixer_mut().vca_mut(*vca)?.name = name;
        }
        GroupCmd::SetVcaColour { vca, colour } => {
            d.mixer_mut().vca_mut(*vca)?.colour = *colour;
        }
        GroupCmd::AssignVca { vca, strips } => {
            let v = d.mixer().vca(*vca)?;
            need_strips(strips)?;
            check_list("A VCA", strips)?;
            check_size("A VCA", v.members.len().saturating_add(strips.len()))?;
            check_vca_joiners(d, Some(v), strips)?;
            d.mixer_mut()
                .vca_mut(*vca)?
                .members
                .extend(strips.iter().copied());
        }
        GroupCmd::UnassignVca { vca, strips } => {
            let v = d.mixer().vca(*vca)?;
            need_strips(strips)?;
            check_list("A VCA", strips)?;
            for id in strips {
                let s = d.mixer().strip(*id)?;
                if !v.members.contains(id) {
                    return Err(ModelError::invalid(format!(
                        "Strip {} is not in VCA {}.",
                        s.name, v.name
                    )));
                }
            }
            d.mixer_mut()
                .vca_mut(*vca)?
                .members
                .retain(|x| !strips.contains(x));
        }
        GroupCmd::SetVcaLevel { vca, gain_db } => {
            d.mixer().vca(*vca)?;
            level("gain_db", *gain_db)?;
            d.mixer_mut().vca_mut(*vca)?.gain_db = *gain_db;
        }
        GroupCmd::SetVcaMute { vca, mute } => {
            d.mixer_mut().vca_mut(*vca)?.mute = *mute;
        }
        GroupCmd::SetVcaSolo { vca, solo } => {
            d.mixer_mut().vca_mut(*vca)?.solo = *solo;
        }
        GroupCmd::SetVcaMode { vca, mode } => {
            d.mixer().vca(*vca)?;
            check_vca_mode(*mode)?;
            d.mixer_mut().vca_mut(*vca)?.automation = *mode;
        }
        GroupCmd::DeleteVca { vca } => {
            d.mixer().vca(*vca)?;
            d.mixer_mut().remove_vca(*vca);
            r.removed.push(AnyId::Vca(*vca));
        }
    }
    Ok(())
}

/// Refuses Write and Trim (Ruling 6).
fn check_vca_mode(mode: AutomationMode) -> Result<()> {
    if VCA_MODES.contains(&mode) {
        Ok(())
    } else {
        Err(ModelError::invalid(
            "A VCA's automation mode is Off, Read, Touch or Latch.",
        ))
    }
}
