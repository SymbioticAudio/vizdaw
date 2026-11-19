//! Fader-group edits (plan groups-vcas Rulings 1–4). Every command checks
//! everything before it changes anything; on error the document is
//! unchanged (commands work on a copy).

use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::groups::{Group, MAX_MEMBERS, check_name, group_of};
use crate::ids::{AnyId, GroupId, StripId};
use crate::mixer::Swatch;
use std::collections::BTreeSet;

/// What a group's name is called in messages.
const GROUP_NAME: &str = "A group's name";

/// Fader-group edits.
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
        }
    }
}

/// Refuses a list with more than [`MAX_MEMBERS`] strips or a strip twice.
fn check_list(strips: &[StripId]) -> Result<()> {
    if strips.len() > MAX_MEMBERS {
        return Err(ModelError::invalid(format!(
            "A group holds at most {MAX_MEMBERS} strips."
        )));
    }
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
            check_list(members)?;
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
            check_list(strips)?;
            if held.saturating_add(strips.len()) > MAX_MEMBERS {
                return Err(ModelError::invalid(format!(
                    "A group holds at most {MAX_MEMBERS} strips."
                )));
            }
            check_joiners(d, strips)?;
            d.mixer_mut()
                .group_mut(*group)?
                .members
                .extend(strips.iter().copied());
        }
        GroupCmd::RemoveFromGroup { group, strips } => {
            let g = d.mixer().group(*group)?;
            need_strips(strips)?;
            check_list(strips)?;
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
    }
    Ok(())
}
