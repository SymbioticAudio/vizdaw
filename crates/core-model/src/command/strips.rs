//! Keeping the mixer in step with the instrument list and output paths
//! (spec §4: every plugin instrument and output path has exactly one
//! strip; Plan Ruling 1 for the built-in synth).

use super::Report;
use crate::document::Document;
use crate::ids::{AnyId, DestId, OutputPathId, StripId};
use crate::mixer::{BUILTIN_SYNTH_STRIP, PluginRef, PluginSlot, Strip, StripKind};

fn add_strip(d: &mut Document, kind: StripKind, name: &str, r: &mut Report) -> StripId {
    let id = d.counters_mut().alloc_strip();
    d.mixer_mut().add(Strip::new(id, name, kind), None);
    r.created.push(AnyId::Strip(id));
    id
}

/// Adds the built-in synth strip when a destination uses the built-in
/// synth and the mixer has none.
pub(crate) fn ensure_builtin(d: &mut Document, r: &mut Report) {
    let used = d.instruments().values().any(|x| x.kind.is_builtin_synth());
    if used && d.mixer().find_kind(StripKind::BuiltinSynth).is_none() {
        add_strip(d, StripKind::BuiltinSynth, BUILTIN_SYNTH_STRIP, r);
    }
}

/// Adds plugin destination `dest`'s `Instrument` strip.
pub(crate) fn add_instrument_strip(d: &mut Document, dest: DestId, name: &str, r: &mut Report) {
    add_strip(d, StripKind::Instrument(dest), name, r);
}

/// Adds output path `out`'s `AudioPath` strip.
pub(crate) fn add_audio_path_strip(
    d: &mut Document,
    out: OutputPathId,
    name: &str,
    r: &mut Report,
) {
    add_strip(d, StripKind::AudioPath(out), name, r);
}

/// Removes every strip fed by `dest` (its `Instrument` and
/// `InstrumentOutput` strips).
pub(crate) fn remove_dest_strips(d: &mut Document, dest: DestId, r: &mut Report) {
    let ids: Vec<StripId> = d
        .mixer()
        .strips()
        .values()
        .filter(|s| {
            matches!(s.kind, StripKind::Instrument(x) | StripKind::InstrumentOutput(x, _) if x == dest)
        })
        .map(|s| s.id)
        .collect();
    for id in ids {
        if d.mixer_mut().remove(id).is_some() {
            r.removed.push(AnyId::Strip(id));
        }
    }
}

/// Removes `dest`'s `InstrumentOutput` strips (not its main strip).
pub(crate) fn remove_output_strips(d: &mut Document, dest: DestId, r: &mut Report) {
    let ids: Vec<StripId> = d
        .mixer()
        .strips()
        .values()
        .filter(|s| matches!(s.kind, StripKind::InstrumentOutput(x, _) if x == dest))
        .map(|s| s.id)
        .collect();
    for id in ids {
        if d.mixer_mut().remove(id).is_some() {
            r.removed.push(AnyId::Strip(id));
        }
    }
}

/// Removes the strip of `kind`, if any.
pub(crate) fn remove_kind(d: &mut Document, kind: StripKind, r: &mut Report) {
    if let Some(id) = d.mixer().find_kind(kind) {
        if d.mixer_mut().remove(id).is_some() {
            r.removed.push(AnyId::Strip(id));
        }
    }
}

/// Renames the strip of `kind`, if any.
pub(crate) fn rename_kind(d: &mut Document, kind: StripKind, name: &str) {
    if let Some(id) = d.mixer().find_kind(kind) {
        if let Ok(s) = d.mixer_mut().strip_mut(id) {
            s.name = name.to_string();
        }
    }
}

/// Fills the EQ slot of each eligible strip `report` created (plan 6g
/// Rulings 3, 5): an `AudioPath`, `Instrument` or `BuiltinSynth` strip that
/// still exists and whose slot is still empty (a command that fills its own
/// EQ keeps it) gets a fresh `plugin` instance with no state or overlay.
/// Each new slot's [`AnyId::Plugin`] is pushed onto `report.created`.
/// Answers how many it filled.
pub(crate) fn fill_new_eqs(doc: &mut Document, report: &mut Report, plugin: &PluginRef) -> usize {
    let strips: Vec<StripId> = report
        .created
        .iter()
        .filter_map(|id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        })
        .collect();
    let mut filled = 0;
    for id in strips {
        let eligible = doc.mixer().strip(id).is_ok_and(|s| {
            s.eq.is_none()
                && matches!(
                    s.kind,
                    StripKind::AudioPath(_) | StripKind::Instrument(_) | StripKind::BuiltinSynth
                )
        });
        if !eligible {
            continue;
        }
        let slot = doc.counters_mut().alloc_plugin();
        if let Ok(s) = doc.mixer_mut().strip_mut(id) {
            s.eq = Some(PluginSlot::new(slot, plugin.clone()));
            report.created.push(AnyId::Plugin(slot));
            filled += 1;
        }
    }
    filled
}
