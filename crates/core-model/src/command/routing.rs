//! Instrument-list, output-path and media-pool commands. Removing an item
//! in use is refused unless a replacement is given.

use super::{Report, strips};
use crate::document::{DestKind, Destination, Document, MediaItem, OutputPath, Patch};
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::ids::{AnyId, DestId, LaneId, MediaId, OutputPathId, SeqId};
use crate::mixer::StripKind;
use crate::rules::{check_event_fields, check_group_channel, check_media_path};

/// Destination, output and media edits.
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
    /// Add a media item.
    AddMedia {
        /// Package-relative path.
        path: String,
        /// Sample rate (> 0).
        sample_rate: u32,
        /// Channels (> 0).
        channels: u16,
        /// Length in frames.
        length_frames: u64,
        /// Optional content hash.
        content_hash: Option<String>,
    },
    /// Remove a media item, re-pointing regions to `replacement`.
    RemoveMedia {
        /// Target.
        media: MediaId,
        /// Replacement (required when in use; must be long enough).
        replacement: Option<MediaId>,
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
            RoutingCmd::AddMedia { .. } => "Add Media",
            RoutingCmd::RemoveMedia { .. } => "Remove Media",
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

fn all_lanes(d: &Document) -> Vec<(SeqId, LaneId)> {
    d.sequences()
        .values()
        .flat_map(|s| s.lanes.iter().map(move |l| (s.id, l.id)))
        .collect()
}

fn require_replacement<T: Copy + PartialEq>(
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
    for (s, l) in all_lanes(d) {
        let lane = d.lane_mut(s, l)?;
        if lane.midi_dest == Some(dest) {
            lane.midi_dest = rep;
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
                    _ => {
                        slot.id = d.counters_mut().alloc_plugin();
                        r.created.push(AnyId::Plugin(slot.id));
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
            let id = d.counters_mut().alloc_output();
            d.outputs_mut().insert(
                id,
                OutputPath {
                    id,
                    name: name.clone(),
                    reference: reference.clone(),
                },
            );
            r.created.push(AnyId::Output(id));
            strips::add_audio_path_strip(d, id, name, r);
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
        RoutingCmd::AddMedia {
            path,
            sample_rate,
            channels,
            length_frames,
            content_hash,
        } => {
            if *sample_rate == 0 || *channels == 0 {
                return Err(ModelError::range("media.format", sample_rate));
            }
            check_media_path(path)?;
            let id = d.counters_mut().alloc_media();
            d.media_mut().insert(
                id,
                MediaItem {
                    id,
                    path: path.clone(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                    length_frames: *length_frames,
                    content_hash: content_hash.clone(),
                },
            );
            r.created.push(AnyId::Media(id));
        }
        RoutingCmd::RemoveMedia { media, replacement } => {
            if !d.media().contains_key(media) {
                return Err(ModelError::UnknownId(AnyId::Media(*media)));
            }
            let regions: Vec<_> = d
                .all_events()
                .filter(|(_, e)| matches!(e.body, EventBody::AudioRegion { media: m, .. } if m == *media))
                .map(|(_, e)| e.id)
                .collect();
            let users = regions.iter().map(|e| AnyId::Event(*e)).collect();
            let exists = replacement.is_some_and(|x| d.media().contains_key(&x));
            let rep = require_replacement(*media, *replacement, exists, users)?;
            if let Some(rep) = rep {
                for id in regions {
                    let mut ev = d.event(id)?.0.clone();
                    if let EventBody::AudioRegion { media: m, .. } = &mut ev.body {
                        *m = rep;
                    }
                    check_event_fields(d, &ev.spec())?;
                    d.replace_event(ev)?;
                }
            }
            d.media_mut().remove(media);
            r.removed.push(AnyId::Media(*media));
        }
    }
    Ok(())
}
