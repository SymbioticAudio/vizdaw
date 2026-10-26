//! Instrument-list and output-path commands (media-pool commands are in
//! `media`). Removing an item in use is refused unless a replacement is
//! given.

use super::{Report, strips};
use crate::document::{DestKind, Destination, Document, MAX_RETURN_LATENCY_MS, OutputPath, Patch};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId};
use crate::lane::LaneInput;
use crate::mixer::StripKind;
use crate::rules::check_group_channel;

/// Destination and output edits.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingCmd {
    /// Add an instrument-list entry.
    AddDestination {
        /// Name.
        name: String,
        /// Kind.
        kind: DestKind,
        /// Default patch.
        default_patch: Option<Patch>,
    },
    /// Rename a destination.
    RenameDestination {
        /// Target.
        dest: DestId,
        /// New name.
        name: String,
    },
    /// Change a destination; every lane and event using it re-routes.
    EditDestination {
        /// Target.
        dest: DestId,
        /// New kind.
        kind: DestKind,
        /// New default patch.
        default_patch: Option<Patch>,
    },
    /// Remove a destination, re-pointing users to `replacement`.
    RemoveDestination {
        /// Target.
        dest: DestId,
        /// Replacement (required when in use).
        replacement: Option<DestId>,
    },
    /// Add an output path.
    AddOutput {
        /// Name.
        name: String,
        /// Opaque engine reference.
        reference: String,
    },
    /// Rename an output path.
    RenameOutput {
        /// Target.
        output: OutputPathId,
        /// New name.
        name: String,
    },
    /// Change an output path's engine reference.
    EditOutput {
        /// Target.
        output: OutputPathId,
        /// New reference.
        reference: String,
    },
    /// Remove an output path, re-pointing lanes to `replacement`.
    RemoveOutput {
        /// Target.
        output: OutputPathId,
        /// Replacement (required when in use).
        replacement: Option<OutputPathId>,
    },
    /// Set an external destination's return latency (spec §4.5a).
    SetReturnLatency {
        /// Target (an external MIDI destination).
        dest: DestId,
        /// Milliseconds, 0–[`MAX_RETURN_LATENCY_MS`].
        ms: f32,
    },
}

impl RoutingCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            RoutingCmd::AddDestination { .. } => "Add Destination",
            RoutingCmd::RenameDestination { .. } => "Rename Destination",
            RoutingCmd::EditDestination { .. } => "Edit Destination",
            RoutingCmd::RemoveDestination { .. } => "Remove Destination",
            RoutingCmd::AddOutput { .. } => "Add Output",
            RoutingCmd::RenameOutput { .. } => "Rename Output",
            RoutingCmd::EditOutput { .. } => "Edit Output",
            RoutingCmd::RemoveOutput { .. } => "Remove Output",
            RoutingCmd::SetReturnLatency { .. } => "Set Return Latency",
        }
    }
}

fn check_dest(kind: &DestKind, patch: &Option<Patch>) -> Result<()> {
    match kind {
        DestKind::MidiEndpoint { group, channel, .. } => {
            check_group_channel(Some((*group, *channel)))?;
        }
        DestKind::PluginInstrument { slot } => {
            if slot.sidechain.is_some() {
                return Err(ModelError::invalid(
                    "a plugin instrument's own slot has no sidechain input",
                ));
            }
            if slot.params.values().any(|v| !v.is_finite()) {
                return Err(ModelError::range("params", "non-finite value"));
            }
        }
    }
    if let Some(p) = patch {
        if p.program > 127 || p.bank.is_some_and(|(m, l)| m > 127 || l > 127) {
            return Err(ModelError::range("default_patch", p.program));
        }
    }
    Ok(())
}

/// Refuses `dest` unless it is an external MIDI destination (plan 5c-2
/// Ruling 2).
pub(crate) fn check_external(d: &Document, dest: DestId) -> Result<()> {
    let x = d
        .instruments()
        .get(&dest)
        .ok_or(ModelError::UnknownId(AnyId::Dest(dest)))?;
    if x.kind.is_external() {
        Ok(())
    } else {
        Err(ModelError::invalid(format!(
            "{} is not an external MIDI destination",
            x.name
        )))
    }
}

fn all_lanes(d: &Document) -> Vec<(SeqId, LaneId)> {
    d.sequences()
        .values()
        .flat_map(|s| s.lanes.iter().map(move |l| (s.id, l.id)))
        .collect()
}

pub(crate) fn require_replacement<T: Copy + PartialEq>(
    target: T,
    replacement: Option<T>,
    exists: bool,
    users: Vec<AnyId>,
) -> Result<Option<T>> {
    match replacement {
        None if !users.is_empty() => Err(ModelError::InUse { users }),
        Some(r) if r == target => Err(ModelError::invalid("replacement is the item itself")),
        Some(_) if !exists => Err(ModelError::invalid("replacement does not exist")),
        other => Ok(other),
    }
}

/// Adds output path `name` (with its `AudioPath` strip) and returns its ID.
pub(crate) fn add_output(
    d: &mut Document,
    name: &str,
    reference: &str,
    r: &mut Report,
) -> OutputPathId {
    let id = d.counters_mut().alloc_output();
    d.outputs_mut().insert(
        id,
        OutputPath {
            id,
            name: name.to_string(),
            reference: reference.to_string(),
        },
    );
    r.created.push(AnyId::Output(id));
    strips::add_audio_path_strip(d, id, name, r);
    id
}

fn remove_destination(
    d: &mut Document,
    dest: DestId,
    replacement: Option<DestId>,
    r: &mut Report,
) -> Result<()> {
    if !d.instruments().contains_key(&dest) {
        return Err(ModelError::UnknownId(AnyId::Dest(dest)));
    }
    let mut users = Vec::new();
    for s in d.sequences().values() {
        for l in &s.lanes {
            if l.midi_dest == Some(dest) {
                users.push(AnyId::Lane(l.id));
            }
            if let Some(layout) = &l.drum {
                for row in &layout.rows {
                    if row.dest == Some(dest) {
                        users.push(AnyId::DrumRow(row.id));
                    }
                }
            }
        }
    }
    let events: Vec<_> = d
        .all_events()
        .filter(|(_, e)| e.dest == Some(dest))
        .map(|(_, e)| e.id)
        .collect();
    users.extend(events.iter().map(|e| AnyId::Event(*e)));
    let exists = replacement.is_some_and(|x| d.instruments().contains_key(&x));
    let rep = require_replacement(dest, replacement, exists, users)?;
    // A lane recording the synth's return follows an external replacement
    // and is unlinked otherwise; the link alone is no use (plan 5c-2
    // Ruling 3).
    let rep_return = rep.filter(|r| d.instruments().get(r).is_some_and(|x| x.kind.is_external()));
    for (s, l) in all_lanes(d) {
        let lane = d.lane_mut(s, l)?;
        if lane.midi_dest == Some(dest) {
            lane.midi_dest = rep;
        }
        if let Some(LaneInput::Audio { return_of, .. }) = lane.input.as_mut()
            && *return_of == Some(dest)
        {
            *return_of = rep_return;
        }
        if let Some(layout) = lane.drum.as_mut() {
            for row in layout.rows.iter_mut() {
                if row.dest == Some(dest) {
                    row.dest = rep;
                }
            }
        }
    }
    for id in events {
        let mut ev = d.event(id)?.0.clone();
        ev.dest = rep;
        d.replace_event(ev)?;
    }
    d.instruments_mut().remove(&dest);
    r.removed.push(AnyId::Dest(dest));
    strips::remove_dest_strips(d, dest, r);
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &RoutingCmd, r: &mut Report) -> Result<()> {
    match cmd {
        RoutingCmd::AddDestination {
            name,
            kind,
            default_patch,
        } => {
            check_dest(kind, default_patch)?;
            let id = d.counters_mut().alloc_dest();
            r.created.push(AnyId::Dest(id));
            let mut kind = kind.clone();
            if let DestKind::PluginInstrument { slot } = &mut kind {
                slot.id = d.counters_mut().alloc_plugin();
                r.created.push(AnyId::Plugin(slot.id));
            }
            let plugin = matches!(kind, DestKind::PluginInstrument { .. });
            d.instruments_mut().insert(
                id,
                Destination {
                    id,
                    name: name.clone(),
                    kind,
                    default_patch: *default_patch,
                    return_latency_ms: 0.0,
                },
            );
            if plugin {
                strips::add_instrument_strip(d, id, name, r);
            }
            strips::ensure_builtin(d, r);
        }
        RoutingCmd::RenameDestination { dest, name } => {
            d.instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?
                .name = name.clone();
            strips::rename_kind(d, StripKind::Instrument(*dest), name);
        }
        RoutingCmd::EditDestination {
            dest,
            kind,
            default_patch,
        } => {
            check_dest(kind, default_patch)?;
            let old = d
                .instruments()
                .get(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?
                .clone();
            let mut kind = kind.clone();
            if let DestKind::PluginInstrument { slot } = &mut kind {
                match &old.kind {
                    DestKind::PluginInstrument { slot: o }
                        if o.plugin.format == slot.plugin.format
                            && o.plugin.id == slot.plugin.id =>
                    {
                        let plugin = slot.plugin.clone();
                        *slot = o.clone();
                        slot.plugin = plugin;
                    }
                    other => {
                        slot.id = d.counters_mut().alloc_plugin();
                        r.created.push(AnyId::Plugin(slot.id));
                        if let DestKind::PluginInstrument { slot: o } = other {
                            // A different plugin: the old instance and its
                            // extra-output strips go (their port layout is
                            // the old plugin's).
                            r.removed.push(AnyId::Plugin(o.id));
                            strips::remove_output_strips(d, *dest, r);
                        }
                    }
                }
            }
            let was = matches!(old.kind, DestKind::PluginInstrument { .. });
            let is = matches!(kind, DestKind::PluginInstrument { .. });
            let x = d
                .instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?;
            x.kind = kind;
            x.default_patch = *default_patch;
            match (was, is) {
                (true, false) => strips::remove_dest_strips(d, *dest, r),
                (false, true) => strips::add_instrument_strip(d, *dest, &old.name, r),
                _ => {}
            }
            strips::ensure_builtin(d, r);
        }
        RoutingCmd::RemoveDestination { dest, replacement } => {
            remove_destination(d, *dest, *replacement, r)?
        }
        RoutingCmd::AddOutput { name, reference } => {
            add_output(d, name, reference, r);
        }
        RoutingCmd::RenameOutput { output, name } => {
            d.outputs_mut()
                .get_mut(output)
                .ok_or(ModelError::UnknownId(AnyId::Output(*output)))?
                .name = name.clone();
            strips::rename_kind(d, StripKind::AudioPath(*output), name);
        }
        RoutingCmd::EditOutput { output, reference } => {
            d.outputs_mut()
                .get_mut(output)
                .ok_or(ModelError::UnknownId(AnyId::Output(*output)))?
                .reference = reference.clone();
        }
        RoutingCmd::RemoveOutput {
            output,
            replacement,
        } => {
            if !d.outputs().contains_key(output) {
                return Err(ModelError::UnknownId(AnyId::Output(*output)));
            }
            let users: Vec<AnyId> = d
                .sequences()
                .values()
                .flat_map(|s| s.lanes.iter())
                .filter(|l| l.audio_out == Some(*output))
                .map(|l| AnyId::Lane(l.id))
                .collect();
            let exists = replacement.is_some_and(|x| d.outputs().contains_key(&x));
            let rep = require_replacement(*output, *replacement, exists, users)?;
            for (s, l) in all_lanes(d) {
                let lane = d.lane_mut(s, l)?;
                if lane.audio_out == Some(*output) {
                    lane.audio_out = rep;
                }
            }
            d.outputs_mut().remove(output);
            r.removed.push(AnyId::Output(*output));
            strips::remove_kind(d, StripKind::AudioPath(*output), r);
        }
        RoutingCmd::SetReturnLatency { dest, ms } => {
            check_external(d, *dest)?;
            if !ms.is_finite() || !(0.0..=MAX_RETURN_LATENCY_MS).contains(ms) {
                return Err(ModelError::range("return_latency_ms", ms));
            }
            d.instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?
                .return_latency_ms = *ms;
        }
    }
    Ok(())
}
