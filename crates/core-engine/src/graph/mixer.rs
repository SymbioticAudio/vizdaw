//! The mixer → graph translation (spec §6; Plan Rulings 1, 4, 5, 25).

use crate::graph::dsp::strip_gains;
use crate::graph::spec::{GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap};
use crate::schedule::TargetIdx;
use core_model::document::Document;
use core_model::ids::StripId;
use core_model::mixer::{Output, PluginSlot, Strip, StripKind, db_to_gain};
use core_model::mixer_rules::audible;
use std::collections::BTreeMap;

/// A graph spec made from a document's mixer.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerGraph {
    /// The spec.
    pub spec: GraphSpec,
    /// `(node index, strip)` of every strip node (meters).
    pub strips: Vec<(usize, StripId)>,
    /// Problems worth showing (not errors).
    pub warnings: Vec<String>,
}

/// Builds the graph of `doc`'s mixer. `instruments` are the built-in synth
/// targets that have an instrument sink, with their latency;
/// `insert_latency` gives each insert's latency (0 for placeholders).
pub fn mixer_spec(
    doc: &Document,
    instruments: &[(TargetIdx, u32)],
    insert_latency: &mut dyn FnMut(&PluginSlot) -> u32,
) -> MixerGraph {
    let m = doc.mixer();
    let on = audible(m);
    let base = instruments.len();
    let index: BTreeMap<StripId, usize> = m
        .strips()
        .keys()
        .enumerate()
        .map(|(i, id)| (*id, base + i))
        .collect();
    let node = |id: StripId| index.get(&id).copied();
    let mut warnings = Vec::new();
    let mut strips: Vec<StripSpec> = m
        .strips()
        .values()
        .map(|s| StripSpec::new(Some(s.id)))
        .collect();
    let master = node(m.master()).unwrap_or(base);
    let feed = m.find_kind(StripKind::BuiltinSynth).and_then(node);
    if feed.is_none() && !instruments.is_empty() {
        warnings.push(
            "no built-in synth strip: the built-in synth plays straight to master".to_string(),
        );
    }
    let to = feed.unwrap_or(master);
    for (i, _) in instruments.iter().enumerate() {
        if let Some(s) = to.checked_sub(base).and_then(|k| strips.get_mut(k)) {
            s.inputs.push(InputSpec {
                from: i,
                tap: Tap::Post,
                gain: 1.0,
            });
        }
    }
    let muted = |s: &Strip| s.mute || !on.contains(&s.id);
    for s in m.strips().values() {
        let Some(from) = node(s.id) else {
            continue;
        };
        let pass = if muted(s) { 0.0 } else { 1.0 };
        if s.id != m.master() {
            let target = match s.output {
                Output::Master => m.master(),
                Output::Bus(b) => b,
            };
            if let Some(t) = node(target)
                .and_then(|t| t.checked_sub(base))
                .and_then(|k| strips.get_mut(k))
            {
                t.inputs.push(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: pass,
                });
            }
        }
        for x in s.sends.iter() {
            let level = if x.enabled {
                db_to_gain(x.level_db) as f32
            } else {
                0.0
            };
            if let Some(t) = node(x.target)
                .and_then(|t| t.checked_sub(base))
                .and_then(|k| strips.get_mut(k))
            {
                t.inputs.push(InputSpec {
                    from,
                    tap: if x.pre_fader { Tap::Pre } else { Tap::Post },
                    gain: pass * level,
                });
            }
        }
    }
    for (k, s) in m.strips().values().enumerate() {
        let Some(spec) = strips.get_mut(k) else {
            continue;
        };
        let (gl, gr) = strip_gains(s.gain_db, s.pan, s.id != m.master());
        spec.gain_l = gl;
        spec.gain_r = gr;
        spec.audible = !muted(s);
        for slot in s.inserts.iter() {
            let sidechain = slot.sidechain.and_then(|src| {
                let from = node(src)?;
                let explicit = m.strip(src).map(|x| x.mute).unwrap_or(false);
                Some(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: if explicit { 0.0 } else { 1.0 },
                })
            });
            spec.inserts.push(InsertSpec {
                instance: Some(slot.id),
                latency: insert_latency(slot),
                bypass: slot.bypass,
                sidechain,
            });
        }
        if matches!(
            s.kind,
            StripKind::Instrument(_) | StripKind::InstrumentOutput(..)
        ) {
            warnings.push(format!(
                "strip {:?} has no instrument until plugin hosting exists",
                s.name
            ));
        }
    }
    let strip_nodes = m
        .strips()
        .keys()
        .enumerate()
        .map(|(i, id)| (base + i, *id))
        .collect();
    let mut nodes: Vec<NodeSpec> = instruments
        .iter()
        .map(|(t, l)| NodeSpec::Instrument {
            target: *t,
            latency: *l,
        })
        .collect();
    nodes.extend(strips.into_iter().map(NodeSpec::Strip));
    MixerGraph {
        spec: GraphSpec { nodes, master },
        strips: strip_nodes,
        warnings,
    }
}
