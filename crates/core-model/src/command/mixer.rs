//! Mixer edits (spec §4). Every command re-checks routing; on error the
//! document is unchanged (commands work on a copy).

use super::Report;
use crate::document::{DestKind, Document, SlotOwner};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, PluginInstanceId, StripId};
use crate::mixer::{AuxSend, Output, ParamId, PluginRef, PluginSlot, Strip, StripKind};
use crate::mixer_rules::{check_routing, check_strip_values};

/// Mixer edits.
#[derive(Debug, Clone, PartialEq)]
pub enum MixerCmd {
    /// Add an aux bus at display position `index` (default: the end).
    AddBus {
        /// Name.
        name: String,
        /// Display position.
        index: Option<usize>,
    },
    /// Remove a bus: outputs to it go to master, sends to it are dropped,
    /// sidechains from it are cleared.
    RemoveBus {
        /// The bus.
        strip: StripId,
    },
    /// Rename a strip.
    RenameStrip {
        /// Target.
        strip: StripId,
        /// New name.
        name: String,
    },
    /// Move a strip (not master) in the display order.
    MoveStrip {
        /// Target.
        strip: StripId,
        /// New position (clamped).
        index: usize,
    },
    /// Set the fader.
    SetGain {
        /// Target.
        strip: StripId,
        /// dB, −144 (−∞) … +12.
        gain_db: f64,
    },
    /// Set the pan.
    SetPan {
        /// Target.
        strip: StripId,
        /// −1 … +1.
        pan: f64,
    },
    /// Mute or unmute.
    SetMute {
        /// Target.
        strip: StripId,
        /// Muted.
        mute: bool,
    },
    /// Solo or unsolo.
    SetSolo {
        /// Target.
        strip: StripId,
        /// Soloed.
        solo: bool,
    },
    /// Route a strip's main output.
    SetOutput {
        /// Target.
        strip: StripId,
        /// Master or a bus.
        output: Output,
    },
    /// Append a send.
    AddSend {
        /// Source strip.
        strip: StripId,
        /// The send.
        send: AuxSend,
    },
    /// Replace send `index`.
    EditSend {
        /// Source strip.
        strip: StripId,
        /// Send position.
        index: usize,
        /// The new send.
        send: AuxSend,
    },
    /// Remove send `index`.
    RemoveSend {
        /// Source strip.
        strip: StripId,
        /// Send position.
        index: usize,
    },
    /// Insert a plugin at `index` (default: the end) with a new instance ID.
    AddInsert {
        /// Strip.
        strip: StripId,
        /// Position.
        index: Option<usize>,
        /// The plugin.
        plugin: PluginRef,
    },
    /// Remove an insert.
    RemoveInsert {
        /// The instance.
        slot: PluginInstanceId,
    },
    /// Move an insert to `strip` at `index` (clamped); it keeps its ID.
    MoveInsert {
        /// The instance.
        slot: PluginInstanceId,
        /// Destination strip.
        strip: StripId,
        /// Destination position.
        index: usize,
    },
    /// Bypass or activate a plugin.
    SetBypass {
        /// The instance.
        slot: PluginInstanceId,
        /// Bypassed.
        bypass: bool,
    },
    /// Route a strip's post-fader output to an insert's sidechain input.
    SetSidechain {
        /// The insert.
        slot: PluginInstanceId,
        /// Source strip, or none.
        source: Option<StripId>,
    },
    /// One parameter gesture: values go into the slot's overlay.
    SetPluginParams {
        /// The instance.
        slot: PluginInstanceId,
        /// `(parameter, value)` in gesture order (later wins).
        params: Vec<(ParamId, f64)>,
    },
    /// Add a strip for extra output `index` (≥ 1) of a plugin instrument.
    AddInstrumentOutput {
        /// The plugin instrument destination.
        dest: DestId,
        /// Output index (0 is the main output's `Instrument` strip).
        index: u16,
        /// Strip name.
        name: String,
    },
}

impl MixerCmd {
    /// Stable command name (undo label).
    pub fn name(&self) -> &'static str {
        match self {
            MixerCmd::AddBus { .. } => "Add Bus",
            MixerCmd::RemoveBus { .. } => "Remove Bus",
            MixerCmd::RenameStrip { .. } => "Rename Strip",
            MixerCmd::MoveStrip { .. } => "Move Strip",
            MixerCmd::SetGain { .. } => "Set Fader",
            MixerCmd::SetPan { .. } => "Set Pan",
            MixerCmd::SetMute { .. } => "Mute",
            MixerCmd::SetSolo { .. } => "Solo",
            MixerCmd::SetOutput { .. } => "Set Output",
            MixerCmd::AddSend { .. } => "Add Send",
            MixerCmd::EditSend { .. } => "Edit Send",
            MixerCmd::RemoveSend { .. } => "Remove Send",
            MixerCmd::AddInsert { .. } => "Add Plugin",
            MixerCmd::RemoveInsert { .. } => "Remove Plugin",
            MixerCmd::MoveInsert { .. } => "Move Plugin",
            MixerCmd::SetBypass { .. } => "Bypass Plugin",
            MixerCmd::SetSidechain { .. } => "Set Sidechain",
            MixerCmd::SetPluginParams { .. } => "Change Plugin Parameter",
            MixerCmd::AddInstrumentOutput { .. } => "Add Instrument Output",
        }
    }
}

fn insert_owner(d: &Document, slot: PluginInstanceId) -> Result<(StripId, usize)> {
    match d.find_slot(slot) {
        Some(SlotOwner::Insert(s, i)) => Ok((s, i)),
        Some(SlotOwner::Instrument(_)) => Err(ModelError::invalid(
            "an instrument's own slot is not an insert",
        )),
        None => Err(ModelError::UnknownId(AnyId::Plugin(slot))),
    }
}

pub(crate) fn apply(d: &mut Document, cmd: &MixerCmd, r: &mut Report) -> Result<()> {
    match cmd {
        MixerCmd::AddBus { name, index } => {
            let id = d.counters_mut().alloc_strip();
            d.mixer_mut()
                .add(Strip::new(id, name.clone(), StripKind::Bus), *index);
            r.created.push(AnyId::Strip(id));
        }
        MixerCmd::RemoveBus { strip } => {
            if d.mixer().strip(*strip)?.kind != StripKind::Bus {
                return Err(ModelError::invalid("only buses are removed directly"));
            }
            d.mixer_mut().remove(*strip);
            r.removed.push(AnyId::Strip(*strip));
        }
        MixerCmd::RenameStrip { strip, name } => {
            d.mixer_mut().strip_mut(*strip)?.name = name.clone();
        }
        MixerCmd::MoveStrip { strip, index } => {
            d.mixer().strip(*strip)?;
            if *strip == d.mixer().master() {
                return Err(ModelError::invalid("master is not in the display order"));
            }
            d.mixer_mut().move_to(*strip, *index);
        }
        MixerCmd::SetGain { strip, gain_db } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            s.gain_db = *gain_db;
            check_strip_values(s)?;
        }
        MixerCmd::SetPan { strip, pan } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            s.pan = *pan;
            check_strip_values(s)?;
        }
        MixerCmd::SetMute { strip, mute } => d.mixer_mut().strip_mut(*strip)?.mute = *mute,
        MixerCmd::SetSolo { strip, solo } => d.mixer_mut().strip_mut(*strip)?.solo = *solo,
        MixerCmd::SetOutput { strip, output } => {
            d.mixer_mut().strip_mut(*strip)?.output = *output;
        }
        MixerCmd::AddSend { strip, send } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            s.sends.push_back(*send);
            check_strip_values(s)?;
        }
        MixerCmd::EditSend { strip, index, send } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            *s.sends
                .get_mut(*index)
                .ok_or(ModelError::range("send", index))? = *send;
            check_strip_values(s)?;
        }
        MixerCmd::RemoveSend { strip, index } => {
            let s = d.mixer_mut().strip_mut(*strip)?;
            if *index >= s.sends.len() {
                return Err(ModelError::range("send", index));
            }
            s.sends.remove(*index);
        }
        MixerCmd::AddInsert {
            strip,
            index,
            plugin,
        } => {
            d.mixer().strip(*strip)?;
            let id = d.counters_mut().alloc_plugin();
            let s = d.mixer_mut().strip_mut(*strip)?;
            let i = index.unwrap_or(s.inserts.len()).min(s.inserts.len());
            s.inserts.insert(i, PluginSlot::new(id, plugin.clone()));
            r.created.push(AnyId::Plugin(id));
        }
        MixerCmd::RemoveInsert { slot } => {
            let (s, i) = insert_owner(d, *slot)?;
            d.mixer_mut().strip_mut(s)?.inserts.remove(i);
            r.removed.push(AnyId::Plugin(*slot));
        }
        MixerCmd::MoveInsert { slot, strip, index } => {
            d.mixer().strip(*strip)?;
            let (s, i) = insert_owner(d, *slot)?;
            let p = d.mixer_mut().strip_mut(s)?.inserts.remove(i);
            let t = d.mixer_mut().strip_mut(*strip)?;
            let at = (*index).min(t.inserts.len());
            t.inserts.insert(at, p);
        }
        MixerCmd::SetBypass { slot, bypass } => d.slot_mut(*slot)?.bypass = *bypass,
        MixerCmd::SetSidechain { slot, source } => {
            insert_owner(d, *slot)?;
            d.slot_mut(*slot)?.sidechain = *source;
        }
        MixerCmd::SetPluginParams { slot, params } => {
            if params.iter().any(|(_, v)| !v.is_finite()) {
                return Err(ModelError::range("params", "non-finite value"));
            }
            let s = d.slot_mut(*slot)?;
            for (p, v) in params {
                s.params.insert(*p, *v);
            }
        }
        MixerCmd::AddInstrumentOutput { dest, index, name } => {
            let plugin = matches!(
                d.instruments().get(dest).map(|x| &x.kind),
                Some(DestKind::PluginInstrument { .. })
            );
            if !plugin {
                return Err(ModelError::invalid(format!(
                    "{dest} is not a plugin instrument"
                )));
            }
            if *index == 0 {
                return Err(ModelError::range("instrument_output", index));
            }
            let kind = StripKind::InstrumentOutput(*dest, *index);
            if d.mixer().find_kind(kind).is_some() {
                return Err(ModelError::invalid("that output already has a strip"));
            }
            let id = d.counters_mut().alloc_strip();
            d.mixer_mut().add(Strip::new(id, name.clone(), kind), None);
            r.created.push(AnyId::Strip(id));
        }
    }
    check_routing(d.mixer())
}
